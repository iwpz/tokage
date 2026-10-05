//! Cost model calculations (docs/04 §6, P6.m4a).
//!
//! Provides calculations for:
//! - `eff_price(m, kind)` with scarcity-based pricing for subscription providers.
//! - `scarcity(provider, u)` based on projected utilization `u`.
//! - `C_turn(X, warm)` single turn cost with prompt caching.
//! - `C_exec(m, t)` execution window cost using usage profiles.

use serde::{Deserialize, Serialize};
use crate::pack::{CacheRatesDef, PricePerMtok, UsageProfileEntry};

/// Scarcity multiplier for provider billing (docs/04 §6.1).
///
/// For usage-based (PAY_AS_YOU_GO / ON_DEMAND): 1.0.
/// For subscription-based:
/// - u < 0.50 (50%) -> 0.5
/// - 0.50 <= u < 0.80 (50-80%) -> 1.0
/// - 0.80 <= u < 0.95 (80-95%) -> 3.0
/// - u >= 0.95 (>= 95%) -> f64::INFINITY (melts/blocks)
pub fn calculate_scarcity(billing: &str, projected_utilization: f64) -> f64 {
    let billing_upper = billing.to_ascii_uppercase();
    if billing_upper != "SUBSCRIPTION" {
        return 1.0;
    }

    if projected_utilization < 0.50 {
        0.5
    } else if projected_utilization < 0.80 {
        1.0
    } else if projected_utilization < 0.95 {
        3.0
    } else {
        f64::INFINITY
    }
}

/// Token price kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenPriceKind {
    Input,
    Output,
}

/// Computes effective price per Mtok accounting for provider scarcity:
/// `eff_price(m, kind) = price_per_mtok[kind] * scarcity(provider)`
pub fn calculate_eff_price(
    price_per_mtok: &PricePerMtok,
    kind: TokenPriceKind,
    scarcity: f64,
) -> f64 {
    let base_price = match kind {
        TokenPriceKind::Input => price_per_mtok.input,
        TokenPriceKind::Output => price_per_mtok.output,
    };
    base_price * scarcity
}

/// Parameters for single turn cost calculation C_turn (docs/04 §6.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnCostParams {
    /// Context size at turn start: baseline B + history H (in tokens).
    pub context_tokens: f64,
    /// Whether initial cache is warm.
    pub warm: bool,
    /// Number of API requests / tool calls in this turn.
    pub api_calls: u32,
    /// New context tokens grown during this turn (g).
    pub context_growth: f64,
    /// Output tokens produced during this turn (o).
    pub output_tokens: f64,
    /// Input price per million tokens ($/Mtok).
    pub p_in: f64,
    /// Output price per million tokens ($/Mtok).
    pub p_out: f64,
    /// Cache read multiplier (r). Defaults to 1.0 if none.
    pub cache_read_multiplier: f64,
    /// Cache write multiplier (w). Defaults to 1.0 if none.
    pub cache_write_multiplier: f64,
}

/// Computes single-turn cost C_turn (docs/04 §6.2):
/// `C_turn(X, warm) = P_in * [ X * (warm ? r : w) + (k - 1) * X * r + g * w ] + P_out * o`
///
/// Prices p_in and p_out are per 1M tokens, so token counts are scaled by 1e-6.
pub fn calculate_c_turn(params: &TurnCostParams) -> f64 {
    let x = params.context_tokens;
    let k = params.api_calls.max(1) as f64;
    let g = params.context_growth;
    let o = params.output_tokens;
    let r = params.cache_read_multiplier;
    let w = params.cache_write_multiplier;

    let first_req_multiplier = if params.warm { r } else { w };
    let subsequent_multiplier = (k - 1.0) * x * r;

    let input_tokens_weighted = x * first_req_multiplier + subsequent_multiplier + g * w;

    let input_cost = (input_tokens_weighted / 1_000_000.0) * params.p_in;
    let output_cost = (o / 1_000_000.0) * params.p_out;

    input_cost + output_cost
}

/// Computes execution cost C_exec for a task execution window (docs/04 §6.3):
/// `C_exec(m, t) = sum_{i=0}^{n-1} C_turn(B_m + D + i*g, warm_i)`
///
/// - `baseline_tokens`: baseline prompt tokens B_m for the model/provider.
/// - `brief_tokens`: task brief size D (typically <= 3000 tokens).
/// - `profile`: usage profile containing (turns n, calls_per_turn k, growth_per_turn g, output_per_turn o).
/// - `baseline_warm`: whether turn 0 starts with warm cache (e.g. shared baseline).
/// - `p_in`, `p_out`: effective input/output price per Mtok.
/// - `cache_rates`: cache read/write multipliers (r, w).
pub fn calculate_c_exec(
    baseline_tokens: f64,
    brief_tokens: f64,
    profile: &UsageProfileEntry,
    baseline_warm: bool,
    p_in: f64,
    p_out: f64,
    cache_rates: Option<&CacheRatesDef>,
) -> f64 {
    let n = profile.turns.max(1);
    let k = profile.calls_per_turn.max(1);
    let g = profile.growth_per_turn as f64;
    let o = profile.output_per_turn as f64;

    let r = cache_rates.map(|cr| cr.read_multiplier).unwrap_or(1.0);
    let w = cache_rates.map(|cr| cr.write_multiplier).unwrap_or(1.0);

    let mut total_cost = 0.0;

    for i in 0..n {
        let x_i = baseline_tokens + brief_tokens + (i as f64) * g;
        // Turn 0 follows baseline_warm; subsequent turns in the same window are warm (docs/04 §6.3).
        let warm_i = if i == 0 { baseline_warm } else { true };

        let turn_params = TurnCostParams {
            context_tokens: x_i,
            warm: warm_i,
            api_calls: k,
            context_growth: g,
            output_tokens: o,
            p_in,
            p_out,
            cache_read_multiplier: r,
            cache_write_multiplier: w,
        };

        total_cost += calculate_c_turn(&turn_params);
    }

    total_cost
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scarcity_multipliers() {
        assert_eq!(calculate_scarcity("PAY_AS_YOU_GO", 0.99), 1.0);
        assert_eq!(calculate_scarcity("SUBSCRIPTION", 0.20), 0.5);
        assert_eq!(calculate_scarcity("SUBSCRIPTION", 0.65), 1.0);
        assert_eq!(calculate_scarcity("SUBSCRIPTION", 0.88), 3.0);
        assert!(calculate_scarcity("SUBSCRIPTION", 0.96).is_infinite());
    }

    #[test]
    fn test_single_turn_cost() {
        // Example: X = 10,000, k = 3 calls, g = 2000, o = 1000.
        // P_in = 3.0 $/Mtok, P_out = 15.0 $/Mtok.
        // Cache read r = 0.1, write w = 1.25, warm = true.
        let params = TurnCostParams {
            context_tokens: 10_000.0,
            warm: true,
            api_calls: 3,
            context_growth: 2_000.0,
            output_tokens: 1_000.0,
            p_in: 3.0,
            p_out: 15.0,
            cache_read_multiplier: 0.1,
            cache_write_multiplier: 1.25,
        };

        // Weighted input = 10_000 * 0.1 + 2 * 10_000 * 0.1 + 2000 * 1.25 = 1000 + 2000 + 2500 = 5500 tokens
        // Input cost = (5500 / 1e6) * 3.0 = 0.0165 USD
        // Output cost = (1000 / 1e6) * 15.0 = 0.015 USD
        // Total = 0.0315 USD
        let cost = calculate_c_turn(&params);
        let expected = 0.0165 + 0.015;
        assert!((cost - expected).abs() < 1e-6);
    }

    #[test]
    fn test_execution_cost_multi_turn() {
        let profile = UsageProfileEntry {
            turns: 2,
            calls_per_turn: 2,
            growth_per_turn: 10_000,
            output_per_turn: 2_000,
        };
        let cache_rates = CacheRatesDef {
            read_multiplier: 0.1,
            write_multiplier: 1.25,
        };

        let cost = calculate_c_exec(
            5_000.0, // Baseline B
            2_000.0, // Brief D
            &profile,
            false, // Turn 0 cold
            3.0,
            15.0,
            Some(&cache_rates),
        );
        assert!(cost > 0.0);
    }
}
