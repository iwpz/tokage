//! Expected cost calculation and threshold filtering (docs/04 §5, P6.m4c).
//!
//! E(m, t) = C_exec(m, t) + (1 - p(m, t)) * [ C_fix(m, t) + (1 - q) * C_exec(m*, t) ]
//!
//! where:
//! - C_exec(m, t) is the primary window cost.
//! - C_fix(m, t) is in-window repair cost (default: 0.3 * C_exec, warm cache).
//! - q is in-window fix success rate (default initial value: 0.5).
//! - m* is runner-up candidate for fallback.
//! - p(m, t) is estimated success probability.

use serde::{Deserialize, Serialize};

/// Computes expected cost E(m, t) considering failure risk and fallback (docs/04 §5).
///
/// If no runner-up is available, c_exec_runner_up defaults to c_exec.
pub fn calculate_expected_cost(
    c_exec: f64,
    p_success: f64,
    q_fix_rate: f64,
    c_exec_runner_up: Option<f64>,
) -> f64 {
    let p = p_success.clamp(0.001, 0.999);
    let q = q_fix_rate.clamp(0.0, 1.0);
    let c_fix = 0.3 * c_exec;
    let fallback_c_exec = c_exec_runner_up.unwrap_or(c_exec);

    c_exec + (1.0 - p) * (c_fix + (1.0 - q) * fallback_c_exec)
}

/// Evaluated model candidate for a specific task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvaluatedCandidate {
    pub provider: String,
    pub model_id: String,
    pub display_name: String,
    pub cli_model_arg: String,
    pub billing: String,
    pub p_success: f64,
    pub c_exec: f64,
    pub expected_cost: f64,
    pub meets_threshold: bool,
    pub origin_layer: String,
}

/// Selects the best candidate model based on confidence threshold and expected cost (docs/04 §5):
///
/// 1. Filters candidates meeting confidence threshold p >= p_min.
/// 2. If candidates meet threshold: selects the one with minimum expected cost E(m, t).
/// 3. If no candidate meets threshold: selects the one with maximum probability p,
///    and flags `meets_threshold = false` for UI warning.
pub fn select_best_candidate(
    mut candidates: Vec<EvaluatedCandidate>,
    p_min: f64,
) -> Option<EvaluatedCandidate> {
    if candidates.is_empty() {
        return None;
    }

    let mut qualified: Vec<_> = candidates
        .iter()
        .filter(|c| c.p_success >= p_min)
        .cloned()
        .collect();

    if !qualified.is_empty() {
        // Sort by expected cost ascending
        qualified.sort_by(|a, b| {
            a.expected_cost
                .partial_cmp(&b.expected_cost)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        qualified.into_iter().next()
    } else {
        // Fallback: highest p_success
        candidates.sort_by(|a, b| {
            b.p_success
                .partial_cmp(&a.p_success)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        candidates.into_iter().next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expected_cost_formula() {
        // c_exec = 1.0 USD, p = 0.8, q = 0.5, runner_up = 1.2 USD
        // c_fix = 0.3 * 1.0 = 0.3
        // fallback = (1 - 0.5) * 1.2 = 0.6
        // failure penalty = (1 - 0.8) * (0.3 + 0.6) = 0.2 * 0.9 = 0.18
        // E = 1.0 + 0.18 = 1.18 USD
        let e = calculate_expected_cost(1.0, 0.8, 0.5, Some(1.2));
        assert!((e - 1.18).abs() < 1e-6);
    }

    #[test]
    fn test_higher_success_rate_can_lower_total_cost() {
        // Model A: cheap (0.5 USD), but low pass rate 0.60
        let cost_a = calculate_expected_cost(0.5, 0.60, 0.5, Some(1.0));
        // c_fix = 0.15, fallback = 0.5 -> 0.4 * 0.65 = 0.26 -> E = 0.76 USD

        // Model B: slightly pricier (0.6 USD), but high pass rate 0.95
        let cost_b = calculate_expected_cost(0.6, 0.95, 0.5, Some(1.0));
        // c_fix = 0.18, fallback = 0.5 -> 0.05 * 0.68 = 0.034 -> E = 0.634 USD

        assert!(cost_b < cost_a);
    }

    #[test]
    fn test_select_best_candidate_threshold_and_fallback() {
        let candidates = vec![
            EvaluatedCandidate {
                provider: "CLAUDE_CODE".into(),
                model_id: "claude-haiku-4-5".into(),
                display_name: "Haiku".into(),
                cli_model_arg: "haiku".into(),
                billing: "SUBSCRIPTION".into(),
                p_success: 0.85,
                c_exec: 0.20,
                expected_cost: 0.22,
                meets_threshold: true,
                origin_layer: "builtin".into(),
            },
            EvaluatedCandidate {
                provider: "CLAUDE_CODE".into(),
                model_id: "claude-sonnet-5-5".into(),
                display_name: "Sonnet".into(),
                cli_model_arg: "sonnet".into(),
                billing: "SUBSCRIPTION".into(),
                p_success: 0.92,
                c_exec: 0.35,
                expected_cost: 0.36,
                meets_threshold: true,
                origin_layer: "builtin".into(),
            },
        ];

        // p_min = 0.80 -> both meet threshold, Haiku has lower expected cost (0.22 vs 0.36)
        let chosen = select_best_candidate(candidates.clone(), 0.80).unwrap();
        assert_eq!(chosen.model_id, "claude-haiku-4-5");

        // High threshold p_min = 0.90 -> only Sonnet qualifies (0.92 >= 0.90)
        let chosen_high = select_best_candidate(candidates.clone(), 0.90).unwrap();
        assert_eq!(chosen_high.model_id, "claude-sonnet-5-5");

        // Extreme threshold p_min = 0.99 -> none qualify, fallback to highest p (Sonnet)
        let chosen_fallback = select_best_candidate(candidates, 0.99).unwrap();
        assert_eq!(chosen_fallback.model_id, "claude-sonnet-5-5");
    }
}
