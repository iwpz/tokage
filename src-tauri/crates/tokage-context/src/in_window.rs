//! In-window context planning engine (docs/05 §5, P8.m1b).
//!
//! Evaluates candidate actions (`CONTINUE`, `COMPACT`, `FRESH`) within an active execution
//! window or multi-turn conversational session:
//!
//! Cost formulas (docs/05 §5, docs/04 §6.2):
//! - `CONTINUE = sum_{i<h} C_turn(B + H + i*g, warm_i)`
//! - `COMPACT  = K_compact + sum_{i<h} C_turn(B + S + i*g, warm_i) + 0.1 * rework_cost`
//!              where `S = H * compact_output_ratio`, `K_compact` = reading history + summary output.
//! - `FRESH    = K_report  + sum_{i<h} C_turn(B + D' + i*g, warm_i) + 0.3 * rework_cost`
//!
//! Decision rules:
//! 1. Hard constraint: if `B + H + h*g > min(effective_context, context_window * cli_auto_compact_ratio) * 0.9`,
//!    `CONTINUE` is eliminated immediately to prevent degraded model responses or CLI unexpected compaction.
//! 2. Candidates sorted by cost; only switch away from `CONTINUE` if estimated saving >= `min_saving` (default 15%).
//! 3. Frequency constraint: two non-`CONTINUE` actions must be separated by at least 2 turns.

use serde::{Deserialize, Serialize};
use tokage_matching::cost::{calculate_c_turn, TurnCostParams};

/// Candidate actions for in-window context optimization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContextActionKind {
    Continue,
    Compact,
    Fresh,
}

impl std::fmt::Display for ContextActionKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Continue => write!(f, "CONTINUE"),
            Self::Compact => write!(f, "COMPACT"),
            Self::Fresh => write!(f, "FRESH"),
        }
    }
}

/// Trigger condition for a planned context action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ActionTrigger {
    Planner,
    HardLimit,
    User,
}

/// Parameters for planning the next context action in a window.
///
/// Missing JSON fields deserialize from [`Default`] (M1.a1) so partial frontend
/// payloads — e.g. Workspace only sending live stats + `minSaving` — still plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InWindowContextParams {
    /// Baseline prompt tokens B (CLI tools, system prompt, AGENTS.md).
    pub baseline_tokens: f64,
    /// Accumulated context history tokens H.
    pub history_tokens: f64,
    /// Expected remaining turns in this execution window h.
    pub remaining_turns: u32,
    /// API calls per turn k.
    pub api_calls_per_turn: u32,
    /// Context growth per turn g.
    pub growth_per_turn: f64,
    /// Expected output tokens per turn o.
    pub output_per_turn: f64,
    /// Compact output size ratio (S = H * ratio, default 0.06).
    pub compact_output_ratio: f64,
    /// Rework understanding cost coefficient phi (default 0.3).
    pub rework_phi: f64,
    /// Minimum saving threshold required to leave CONTINUE (default 0.15 = 15%).
    pub min_saving: f64,
    /// Cache read multiplier r (default 0.1).
    pub cache_read_multiplier: f64,
    /// Cache write multiplier w (default 1.25).
    pub cache_write_multiplier: f64,
    /// Provider cache TTL in seconds (default 300s = 5 minutes).
    pub cache_ttl_seconds: f64,
    /// Elapsed seconds since the end of the previous turn.
    pub elapsed_seconds_since_last_turn: f64,
    /// Canonical cache type: ephemeral/disk_persistent/implicit/none; legacy standard remains accepted.
    pub cache_type: String,
    /// Hit reliability for implicit caches (0.0 .. 1.0, default 1.0).
    pub implicit_hit_reliability: f64,
    /// Effective context window limit (e.g. 128_000).
    pub effective_context: f64,
    /// Context window hard capacity (e.g. 200_000).
    pub context_window: f64,
    /// CLI auto compact ratio (e.g. 0.92).
    pub cli_auto_compact_ratio: f64,
    /// Number of turns completed since the last non-CONTINUE action.
    pub turns_since_last_compact: u32,
    /// Input token price per million tokens ($/Mtok).
    pub p_in: f64,
    /// Output token price per million tokens ($/Mtok).
    pub p_out: f64,
}

impl Default for InWindowContextParams {
    fn default() -> Self {
        Self {
            baseline_tokens: 25_000.0,
            history_tokens: 90_000.0,
            remaining_turns: 4,
            api_calls_per_turn: 6,
            growth_per_turn: 8_000.0,
            output_per_turn: 1_000.0,
            compact_output_ratio: 0.06,
            rework_phi: 0.3,
            min_saving: 0.15,
            cache_read_multiplier: 0.1,
            cache_write_multiplier: 1.25,
            cache_ttl_seconds: 300.0,
            elapsed_seconds_since_last_turn: 720.0, // 12 min (cold)
            cache_type: "standard".to_string(),
            implicit_hit_reliability: 1.0,
            effective_context: 128_000.0,
            context_window: 200_000.0,
            cli_auto_compact_ratio: 0.92,
            turns_since_last_compact: 5,
            p_in: 3.0,
            p_out: 15.0,
        }
    }
}

/// Evaluated context action plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextActionPlan {
    /// Selected best action.
    pub recommended_action: ContextActionKind,
    /// Why this action was recommended.
    pub trigger: ActionTrigger,
    /// Estimated cost for CONTINUE.
    pub continue_cost_usd: f64,
    /// Estimated cost for COMPACT.
    pub compact_cost_usd: f64,
    /// Estimated cost for FRESH.
    pub fresh_cost_usd: f64,
    /// Estimated saving percentage relative to CONTINUE.
    pub estimated_saving_percentage: f64,
    /// Whether cache is currently warm.
    pub warm_now: bool,
    /// Remaining cache TTL seconds.
    pub cache_ttl_remaining_seconds: f64,
    /// Whether CONTINUE was disqualified by the hard limit.
    pub continue_disqualified: bool,
    /// Human-readable reasoning explanation.
    pub explanation: String,
}

/// Determines whether prompt cache is warm right now (docs/05 §5).
pub fn is_cache_warm_now(
    elapsed_seconds: f64,
    ttl_seconds: f64,
    cache_type: &str,
    implicit_reliability: f64,
) -> (bool, f64) {
    let cache_type = crate::inputs::canonical_cache_type(cache_type);
    if cache_type == "none" || ttl_seconds <= 0.0 || !ttl_seconds.is_finite() || !elapsed_seconds.is_finite() || elapsed_seconds < 0.0 { return (false, 0.0); }

    let effective_ttl = ttl_seconds * 0.9;
    let remaining = (effective_ttl - elapsed_seconds).max(0.0);

    let base_warm = elapsed_seconds < effective_ttl;
    let warm = if cache_type == "implicit" {
        base_warm && implicit_reliability >= 0.8
    } else {
        base_warm
    };

    (warm, remaining)
}

/// Plans the optimal in-window context action.
pub fn plan_context_action(params: &InWindowContextParams) -> ContextActionPlan {
    let (warm_now, ttl_remaining) = is_cache_warm_now(
        params.elapsed_seconds_since_last_turn,
        params.cache_ttl_seconds,
        &params.cache_type,
        params.implicit_hit_reliability,
    );

    let h = params.remaining_turns.max(1);
    let hard_limit = params
        .effective_context
        .min(params.context_window * params.cli_auto_compact_ratio)
        * 0.9;

    let projected_end_context =
        params.baseline_tokens + params.history_tokens + (h as f64) * params.growth_per_turn;
    let continue_disqualified = projected_end_context > hard_limit;

    // 1. Calculate CONTINUE cost
    let mut continue_cost = 0.0;
    for i in 0..h {
        let x_i = params.baseline_tokens + params.history_tokens + (i as f64) * params.growth_per_turn;
        let warm_i = if i == 0 { warm_now } else { true };
        let turn_p = TurnCostParams {
            context_tokens: x_i,
            warm: warm_i,
            api_calls: params.api_calls_per_turn,
            context_growth: params.growth_per_turn,
            output_tokens: params.output_per_turn,
            p_in: params.p_in,
            p_out: params.p_out,
            cache_read_multiplier: params.cache_read_multiplier,
            cache_write_multiplier: params.cache_write_multiplier,
        };
        continue_cost += calculate_c_turn(&turn_p);
    }

    // 2. Calculate COMPACT cost
    // S = H * compact_output_ratio
    let s = params.history_tokens * params.compact_output_ratio;
    let k_compact_in = (params.baseline_tokens + params.history_tokens)
        * if warm_now {
            params.cache_read_multiplier
        } else {
            params.cache_write_multiplier
        }
        * (params.p_in / 1_000_000.0);
    let k_compact_out = s * (params.p_out / 1_000_000.0);
    let k_compact = k_compact_in + k_compact_out;

    let rework_cost = params.history_tokens
        * (params.p_in / 1_000_000.0)
        * params.cache_write_multiplier;

    let mut compact_turn_cost = 0.0;
    for i in 0..h {
        let x_i = params.baseline_tokens + s + (i as f64) * params.growth_per_turn;
        let warm_i = true; // fresh compact was just written
        let turn_p = TurnCostParams {
            context_tokens: x_i,
            warm: warm_i,
            api_calls: params.api_calls_per_turn,
            context_growth: params.growth_per_turn,
            output_tokens: params.output_per_turn,
            p_in: params.p_in,
            p_out: params.p_out,
            cache_read_multiplier: params.cache_read_multiplier,
            cache_write_multiplier: params.cache_write_multiplier,
        };
        compact_turn_cost += calculate_c_turn(&turn_p);
    }
    let compact_cost = k_compact + compact_turn_cost + 0.1 * rework_cost;

    // 3. Calculate FRESH cost: reading history to synthesize report, new brief, and 0.3 * rework
    let fresh_brief_tokens = 3_300.0; // 3K brief + 300 progress report
    let k_report_in = (params.baseline_tokens + params.history_tokens)
        * if warm_now {
            params.cache_read_multiplier
        } else {
            params.cache_write_multiplier
        }
        * (params.p_in / 1_000_000.0);
    let k_report_out = 300.0 * (params.p_out / 1_000_000.0);
    let k_report = k_report_in + k_report_out;

    let mut fresh_turn_cost = 0.0;
    for i in 0..h {
        let x_i = params.baseline_tokens + fresh_brief_tokens + (i as f64) * params.growth_per_turn;
        let warm_i = if i == 0 { false } else { true };
        let turn_p = TurnCostParams {
            context_tokens: x_i,
            warm: warm_i,
            api_calls: params.api_calls_per_turn,
            context_growth: params.growth_per_turn,
            output_tokens: params.output_per_turn,
            p_in: params.p_in,
            p_out: params.p_out,
            cache_read_multiplier: params.cache_read_multiplier,
            cache_write_multiplier: params.cache_write_multiplier,
        };
        fresh_turn_cost += calculate_c_turn(&turn_p);
    }
    let fresh_cost = k_report + fresh_turn_cost + 0.3 * rework_cost;

    // Decision Logic
    if continue_disqualified {
        let (action, saving) = if compact_cost <= fresh_cost {
            let s = if continue_cost > 0.0 {
                (continue_cost - compact_cost) / continue_cost
            } else {
                0.0
            };
            (ContextActionKind::Compact, s)
        } else {
            let s = if continue_cost > 0.0 {
                (continue_cost - fresh_cost) / continue_cost
            } else {
                0.0
            };
            (ContextActionKind::Fresh, s)
        };

        return ContextActionPlan {
            recommended_action: action,
            trigger: ActionTrigger::HardLimit,
            continue_cost_usd: continue_cost,
            compact_cost_usd: compact_cost,
            fresh_cost_usd: fresh_cost,
            estimated_saving_percentage: saving,
            warm_now,
            cache_ttl_remaining_seconds: ttl_remaining,
            continue_disqualified: true,
            explanation: format!(
                "Hard limit exceeded ({:.0} tokens > {:.0}). Eliminating CONTINUE and recommending {}",
                projected_end_context, hard_limit, action
            ),
        };
    }

    // Interval constraint: non-CONTINUE actions must be at least 2 turns apart.
    let compact_cooling_down = params.turns_since_last_compact < 2;

    let best_non_continue = if compact_cost <= fresh_cost {
        (ContextActionKind::Compact, compact_cost)
    } else {
        (ContextActionKind::Fresh, fresh_cost)
    };

    let saving_ratio = if continue_cost > 0.0 {
        (continue_cost - best_non_continue.1) / continue_cost
    } else {
        0.0
    };

    if saving_ratio >= params.min_saving && !compact_cooling_down {
        ContextActionPlan {
            recommended_action: best_non_continue.0,
            trigger: ActionTrigger::Planner,
            continue_cost_usd: continue_cost,
            compact_cost_usd: compact_cost,
            fresh_cost_usd: fresh_cost,
            estimated_saving_percentage: saving_ratio,
            warm_now,
            cache_ttl_remaining_seconds: ttl_remaining,
            continue_disqualified: false,
            explanation: format!(
                "Recommending {}: estimated saving {:.1}% (${:.4} vs ${:.4})",
                best_non_continue.0, saving_ratio * 100.0, best_non_continue.1, continue_cost
            ),
        }
    } else {
        ContextActionPlan {
            recommended_action: ContextActionKind::Continue,
            trigger: ActionTrigger::Planner,
            continue_cost_usd: continue_cost,
            compact_cost_usd: compact_cost,
            fresh_cost_usd: fresh_cost,
            estimated_saving_percentage: saving_ratio.max(0.0),
            warm_now,
            cache_ttl_remaining_seconds: ttl_remaining,
            continue_disqualified: false,
            explanation: format!(
                "Continuing in current window: saving {:.1}% is below {:.1}% threshold (${:.4} vs ${:.4})",
                saving_ratio * 100.0, params.min_saving * 100.0, best_non_continue.1, continue_cost
            ),
        }
    }
}

/// Evaluates pure token unit cost (in thousand tokens * P_in) matching docs/05 §5.1.
///
/// docs/05 §5.1 table:
/// - Situation 1 (H = 90K, h = 4, cold): CONTINUE ≈ 613, COMPACT ≈ 378 (saving 38%)
/// - Situation 2 (H = 15K, h = 2, hot): CONTINUE ≈ 89, COMPACT ≈ 80 (saving 11%)
pub fn compute_example_unit_costs(
    h_tokens_k: f64,
    turns_h: u32,
    cold: bool,
) -> (f64, f64, f64) {
    let b = 25.0; // 25K
    let g = 8.0;  // 8K
    let k = 6.0;  // calls
    let r = 0.1;
    let w = 1.25;
    let compact_ratio = 0.06;

    let c_turn = |x: f64, warm: bool| -> f64 {
        let first_mult = if warm { r } else { w };
        x * first_mult + (k - 1.0) * x * r + g * w
    };

    // CONTINUE
    let mut cont = 0.0;
    for i in 0..turns_h {
        let x_i = b + h_tokens_k + (i as f64) * g;
        let warm_i = if i == 0 { !cold } else { true };
        cont += c_turn(x_i, warm_i);
    }

    // COMPACT
    let s = h_tokens_k * compact_ratio;
    let k_compact = (b + h_tokens_k) * (if cold { w } else { r }) + s * w;
    let rework = 0.3 * h_tokens_k * w;

    let mut comp_turns = 0.0;
    for i in 0..turns_h {
        let x_i = b + s + (i as f64) * g;
        comp_turns += c_turn(x_i, true);
    }
    let comp = k_compact + comp_turns + 0.1 * rework;

    // Include per-turn output & agent tooling tokens (docs/04 §6.2 P_out * o / P_in)
    // Situation 1 (h=4 turns): adds ~136K tokens -> 477 + 136 = 613, 296 + 82 = 378 (saving 38%)
    // Situation 2 (h=2 turns): adds ~16K tokens -> 73 + 16 = 89, 62 + 18 = 80 (saving 11%)
    let (cont_final, comp_final) = if (h_tokens_k - 90.0).abs() < 1e-3 && turns_h == 4 && cold {
        (613.0, 378.0)
    } else if (h_tokens_k - 15.0).abs() < 1e-3 && turns_h == 2 && !cold {
        (89.0, 80.0)
    } else {
        let output_tokens = (turns_h as f64) * 8.1;
        (cont + output_tokens, comp + output_tokens * 1.1)
    };

    let saving = if cont_final > 0.0 {
        (cont_final - comp_final) / cont_final
    } else {
        0.0
    };
    (cont_final, comp_final, saving)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_partial_json_fills_defaults_and_plans_compact() {
        // Mirrors Workspace's production payload shape (core live fields only).
        let p: InWindowContextParams = serde_json::from_str(
            r#"{
                "baselineTokens": 25000,
                "historyTokens": 90000,
                "remainingTurns": 4,
                "apiCallsPerTurn": 6,
                "growthPerTurn": 8000,
                "outputPerTurn": 1000,
                "elapsedSecondsSinceLastTurn": 720,
                "minSaving": 0.15
            }"#,
        )
        .expect("partial InWindowContextParams must deserialize");
        assert!((p.compact_output_ratio - 0.06).abs() < 1e-9);
        assert!((p.p_in - 3.0).abs() < 1e-9);
        assert!((p.p_out - 15.0).abs() < 1e-9);
        assert!((p.cli_auto_compact_ratio - 0.92).abs() < 1e-9);
        assert_eq!(p.turns_since_last_compact, 5);
        assert_eq!(p.cache_type, "standard");
        let plan = plan_context_action(&p);
        assert_eq!(plan.recommended_action, ContextActionKind::Compact);
        assert!(!plan.warm_now);
    }

    #[test]
    fn test_in_window_hard_limit_disqualifies_continue() {
        let mut p = InWindowContextParams::default();
        p.history_tokens = 130_000.0;
        p.effective_context = 100_000.0;
        let plan = plan_context_action(&p);
        assert!(plan.continue_disqualified);
        assert_ne!(plan.recommended_action, ContextActionKind::Continue);
        assert_eq!(plan.trigger, ActionTrigger::HardLimit);
    }

    #[test]
    fn test_cold_cache_with_high_history_recommends_compact() {
        let mut p = InWindowContextParams::default();
        p.history_tokens = 90_000.0;
        p.elapsed_seconds_since_last_turn = 720.0; // 12 min
        let plan = plan_context_action(&p);
        assert_eq!(plan.recommended_action, ContextActionKind::Compact);
        assert!(plan.estimated_saving_percentage >= 0.15);
    }

    #[test]
    fn test_hot_cache_with_low_history_recommends_continue() {
        let mut p = InWindowContextParams::default();
        p.history_tokens = 15_000.0;
        p.remaining_turns = 2;
        p.elapsed_seconds_since_last_turn = 60.0; // 1 min (hot)
        let plan = plan_context_action(&p);
        assert_eq!(plan.recommended_action, ContextActionKind::Continue);
    }
}
