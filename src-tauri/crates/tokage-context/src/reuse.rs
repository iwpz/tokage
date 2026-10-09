//! Window reuse decision engine (docs/05 §4, P8.m1a).
//!
//! Evaluates whether an agent execution window from Task A can be reused for Task B:
//!
//! Reuse conditions (must ALL be satisfied simultaneously):
//! 1. Same model and same role/permissions (role/auth changes MUST open fresh window, §8).
//! 2. Task A and B have same domain or adjacent write scopes (A's history is useful for B).
//! 3. Interval from A finish to B start < cache TTL * 0.9 (cache remains warm).
//! 4. Reuse cost < fresh window cost * (1 - min_saving) (cost-effective).
//! 5. Baseline + History_A + Brief_B <= effective context hard limit.

use serde::{Deserialize, Serialize};
use tokage_matching::cost::{calculate_c_turn, TurnCostParams};

/// Parameters for evaluating whether window A can be reused for task B.
///
/// Missing JSON fields deserialize from [`Default`] (M1.a1), matching the
/// in-window planner's partial-payload contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WindowReuseParams {
    pub model_a: String,
    pub model_b: String,
    pub role_a: String,
    pub role_b: String,
    pub domain_a: String,
    pub domain_b: String,
    pub write_scope_a: Vec<String>,
    pub write_scope_b: Vec<String>,
    /// Accumulated history tokens in window A (H_A).
    pub history_tokens_a: f64,
    /// Relevant history tokens in window A for task B (defaults to min(H_A, 30_000)).
    pub relevant_history_tokens_a: Option<f64>,
    /// CLI baseline tokens (B).
    pub baseline_tokens: f64,
    /// Task B brief tokens (D_B).
    pub brief_tokens_b: f64,
    /// Expected turns for task B (n).
    pub expected_turns_b: u32,
    /// API calls per turn for task B (k).
    pub api_calls_per_turn: u32,
    /// Context growth per turn for task B (g).
    pub growth_per_turn: f64,
    /// Output tokens per turn for task B (o).
    pub output_per_turn: f64,
    /// Elapsed seconds from task A completion to task B start.
    pub elapsed_seconds: f64,
    /// Provider cache TTL in seconds (default 300s = 5 minutes).
    pub cache_ttl_seconds: f64,
    /// Effective context window hard limit (e.g. 128_000).
    pub effective_context: f64,
    /// Total context window limit (e.g. 200_000).
    pub context_window: f64,
    /// CLI automatic compact threshold ratio (e.g. 0.92).
    pub cli_auto_compact_ratio: f64,
    /// Coefficient of rework understanding cost (phi, default 0.3).
    pub rework_phi: f64,
    /// Minimum saving percentage required to choose reuse (default 0.15 = 15%).
    pub min_saving: f64,
    /// Input price per million tokens.
    pub p_in: f64,
    /// Output price per million tokens.
    pub p_out: f64,
    /// Cache read multiplier (r).
    pub cache_read_multiplier: f64,
    /// Cache write multiplier (w).
    pub cache_write_multiplier: f64,
}

impl Default for WindowReuseParams {
    fn default() -> Self {
        Self {
            model_a: "claude-3-5-sonnet".to_string(),
            model_b: "claude-3-5-sonnet".to_string(),
            role_a: "EXECUTOR".to_string(),
            role_b: "EXECUTOR".to_string(),
            domain_a: "frontend".to_string(),
            domain_b: "frontend".to_string(),
            write_scope_a: vec!["src/components/".to_string()],
            write_scope_b: vec!["src/components/".to_string()],
            history_tokens_a: 15_000.0,
            relevant_history_tokens_a: None,
            baseline_tokens: 25_000.0,
            brief_tokens_b: 2_000.0,
            expected_turns_b: 2,
            api_calls_per_turn: 4,
            growth_per_turn: 4_000.0,
            output_per_turn: 1_000.0,
            elapsed_seconds: 60.0,
            cache_ttl_seconds: 300.0,
            effective_context: 128_000.0,
            context_window: 200_000.0,
            cli_auto_compact_ratio: 0.92,
            rework_phi: 0.3,
            min_saving: 0.15,
            p_in: 3.0,
            p_out: 15.0,
            cache_read_multiplier: 0.1,
            cache_write_multiplier: 1.25,
        }
    }
}

/// Outcome of window reuse evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowReuseDecision {
    pub can_reuse: bool,
    pub reuse_cost_usd: f64,
    pub fresh_cost_usd: f64,
    pub rework_cost_usd: f64,
    pub saving_percentage: f64,
    pub reason: String,
}

/// Checks whether two tasks have adjacent or overlapping write scopes.
pub fn scopes_are_adjacent(scope_a: &[String], scope_b: &[String]) -> bool {
    if scope_a.is_empty() || scope_b.is_empty() {
        return false;
    }
    for a in scope_a {
        for b in scope_b {
            let a_clean = a.trim_end_matches('/');
            let b_clean = b.trim_end_matches('/');
            if a_clean == b_clean || a_clean.starts_with(b_clean) || b_clean.starts_with(a_clean) {
                return true;
            }
            // Check common directory prefix
            if let (Some(a_dir), Some(b_dir)) = (a_clean.rsplit_once('/'), b_clean.rsplit_once('/')) {
                if a_dir.0 == b_dir.0 {
                    return true;
                }
            }
        }
    }
    false
}

/// Evaluates window reuse according to docs/05 §4.
pub fn decide_window_reuse(params: &WindowReuseParams) -> WindowReuseDecision {
    // Condition 1: Same model and same role/permissions (role changes MUST open fresh window, §8).
    if params.model_a != params.model_b {
        return WindowReuseDecision {
            can_reuse: false,
            reuse_cost_usd: 0.0,
            fresh_cost_usd: 0.0,
            rework_cost_usd: 0.0,
            saving_percentage: 0.0,
            reason: format!("Model mismatch: {} vs {}", params.model_a, params.model_b),
        };
    }

    if params.role_a != params.role_b {
        return WindowReuseDecision {
            can_reuse: false,
            reuse_cost_usd: 0.0,
            fresh_cost_usd: 0.0,
            rework_cost_usd: 0.0,
            saving_percentage: 0.0,
            reason: format!("Role/permission mismatch: {} vs {} (fresh window required)", params.role_a, params.role_b),
        };
    }

    // Condition 2: Same domain or adjacent write scope.
    let same_domain = params.domain_a.eq_ignore_ascii_case(&params.domain_b);
    let adjacent_scope = scopes_are_adjacent(&params.write_scope_a, &params.write_scope_b);
    if !same_domain && !adjacent_scope {
        return WindowReuseDecision {
            can_reuse: false,
            reuse_cost_usd: 0.0,
            fresh_cost_usd: 0.0,
            rework_cost_usd: 0.0,
            saving_percentage: 0.0,
            reason: "Domains differ and write scopes are not adjacent".to_string(),
        };
    }

    // Condition 3: Interval from A finish to B start < cache TTL * 0.9.
    let cache_warm = params.elapsed_seconds < params.cache_ttl_seconds * 0.9;
    if !cache_warm {
        return WindowReuseDecision {
            can_reuse: false,
            reuse_cost_usd: 0.0,
            fresh_cost_usd: 0.0,
            rework_cost_usd: 0.0,
            saving_percentage: 0.0,
            reason: format!(
                "Cache is cold: elapsed {:.1}s exceeds TTL threshold {:.1}s",
                params.elapsed_seconds,
                params.cache_ttl_seconds * 0.9
            ),
        };
    }

    // Condition 5: Baseline + H_A + D_B below effective context hard limit.
    let hard_limit = params.effective_context.min(params.context_window * params.cli_auto_compact_ratio) * 0.9;
    let initial_reuse_tokens = params.baseline_tokens + params.history_tokens_a + params.brief_tokens_b;
    if initial_reuse_tokens > hard_limit {
        return WindowReuseDecision {
            can_reuse: false,
            reuse_cost_usd: 0.0,
            fresh_cost_usd: 0.0,
            rework_cost_usd: 0.0,
            saving_percentage: 0.0,
            reason: format!(
                "Context exceeds hard limit: {:.0} tokens > limit {:.0}",
                initial_reuse_tokens, hard_limit
            ),
        };
    }

    // Calculate reuse cost: sum_{i=0}^{n-1} C_turn(B + H_A + D_B + i*g, warm_i)
    let n = params.expected_turns_b.max(1);
    let mut reuse_cost = 0.0;
    for i in 0..n {
        let x_i = initial_reuse_tokens + (i as f64) * params.growth_per_turn;
        let warm_i = true; // because cache_warm is true and within same window
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
        reuse_cost += calculate_c_turn(&turn_p);
    }

    // Calculate fresh cost: sum_{i=0}^{n-1} C_turn(B + D_B + i*g, warm_i) + rework_cost
    let mut fresh_turn_cost = 0.0;
    for i in 0..n {
        let x_i = params.baseline_tokens + params.brief_tokens_b + (i as f64) * params.growth_per_turn;
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

    let h_relevant = params
        .relevant_history_tokens_a
        .unwrap_or_else(|| params.history_tokens_a.min(30_000.0));
    let rework_cost = params.rework_phi
        * h_relevant
        * (params.p_in / 1_000_000.0)
        * params.cache_write_multiplier;

    let fresh_total_cost = fresh_turn_cost + rework_cost;

    let saving = if fresh_total_cost > 0.0 {
        (fresh_total_cost - reuse_cost) / fresh_total_cost
    } else {
        0.0
    };

    let can_reuse = reuse_cost < fresh_total_cost * (1.0 - params.min_saving);

    let reason = if can_reuse {
        format!(
            "Reuse approved: saves {:.1}% (${:.4} vs ${:.4}) with warm cache",
            saving * 100.0, reuse_cost, fresh_total_cost
        )
    } else {
        format!(
            "Fresh window preferred: saving {:.1}% is below threshold {:.1}% (${:.4} vs ${:.4})",
            saving * 100.0, params.min_saving * 100.0, reuse_cost, fresh_total_cost
        )
    };

    WindowReuseDecision {
        can_reuse,
        reuse_cost_usd: reuse_cost,
        fresh_cost_usd: fresh_total_cost,
        rework_cost_usd: rework_cost,
        saving_percentage: saving,
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_partial_json_fills_defaults() {
        let p: WindowReuseParams = serde_json::from_str(
            r#"{
                "modelA": "claude-3-5-sonnet",
                "modelB": "claude-3-5-sonnet",
                "roleA": "EXECUTOR",
                "roleB": "EXECUTOR",
                "domainA": "frontend",
                "domainB": "frontend",
                "writeScopeA": ["src/components/"],
                "writeScopeB": ["src/components/"],
                "historyTokensA": 15000,
                "baselineTokens": 25000,
                "briefTokensB": 2000,
                "expectedTurnsB": 2,
                "apiCallsPerTurn": 4,
                "growthPerTurn": 4000,
                "outputPerTurn": 1000,
                "elapsedSeconds": 60
            }"#,
        )
        .expect("partial WindowReuseParams must deserialize");
        assert!((p.min_saving - 0.15).abs() < 1e-9);
        assert!((p.p_in - 3.0).abs() < 1e-9);
        assert!((p.cache_ttl_seconds - 300.0).abs() < 1e-9);
        assert!(p.relevant_history_tokens_a.is_none());
        let res = decide_window_reuse(&p);
        assert!(res.can_reuse || res.reason.contains("Fresh") || res.reason.contains("Reuse"));
    }

    #[test]
    fn test_role_mismatch_prevents_reuse() {
        let mut p = WindowReuseParams::default();
        p.role_a = "EXECUTOR".to_string();
        p.role_b = "TESTER".to_string();
        let res = decide_window_reuse(&p);
        assert!(!res.can_reuse);
        assert!(res.reason.contains("Role/permission mismatch"));
    }

    #[test]
    fn test_cold_cache_prevents_reuse() {
        let mut p = WindowReuseParams::default();
        p.elapsed_seconds = 400.0; // > 300s TTL
        let res = decide_window_reuse(&p);
        assert!(!res.can_reuse);
        assert!(res.reason.contains("Cache is cold"));
    }

    #[test]
    fn test_hard_context_limit_prevents_reuse() {
        let mut p = WindowReuseParams::default();
        p.history_tokens_a = 120_000.0;
        p.effective_context = 100_000.0;
        let res = decide_window_reuse(&p);
        assert!(!res.can_reuse);
        assert!(res.reason.contains("Context exceeds hard limit"));
    }

    #[test]
    fn test_adjacent_scope_allows_reuse_evaluation() {
        assert!(scopes_are_adjacent(&["src/auth/login.rs".to_string()], &["src/auth/session.rs".to_string()]));
        assert!(!scopes_are_adjacent(&["src/auth/".to_string()], &["docs/specs/".to_string()]));
    }
}
