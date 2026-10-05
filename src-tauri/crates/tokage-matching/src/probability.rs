//! Success probability calculation and threshold filtering (docs/04 §4, P6.m4b).
//!
//! Prior:
//!   p₀(m, t) = sigmoid( a * (skill[m][d] - req(δ, σ)) + b )
//!   where:
//!   - req(δ, σ) is requirement score from definition pack.
//!   - a, b are success_model parameters.
//!
//! Posterior (Beta distribution):
//!   α₀ = κ * p₀, β₀ = κ * (1 - p₀)
//!   α = α₀ + successes, β = β₀ + failures
//!   p(m, t) = α / (α + β)

use crate::pack::SuccessModel;

/// Standard sigmoid function 1 / (1 + exp(-x)).
pub fn sigmoid(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

/// Computes prior success probability p₀(m, t) (docs/04 §4).
///
/// If skill is missing, defaults to 1.0 (or minimum baseline).
/// If requirement score for (difficulty, size) is missing, falls back to difficulty baseline.
pub fn calculate_prior_p0(
    skill: f64,
    difficulty: u8,
    size: &str,
    model: &SuccessModel,
) -> f64 {
    let diff_str = difficulty.to_string();
    let size_upper = size.to_ascii_uppercase();

    let req_score = model
        .req
        .get(&diff_str)
        .and_then(|sizes| sizes.get(&size_upper))
        .copied()
        .unwrap_or_else(|| {
            match difficulty {
                1 => 2.0,
                2 => 3.5,
                _ => 4.5,
            }
        });

    let z = model.a * (skill - req_score) + model.b;
    sigmoid(z)
}

/// Computes posterior success probability p(m, t) incorporating local observations.
///
/// - `prior_p0`: Prior probability p₀ calculated from definition pack.
/// - `prior_strength`: Weight κ of the prior (default: 6.0).
/// - `successes`: Observed first-pass successes (weight 1.0) and in-window fixes (weight 0.5).
/// - `failures`: Observed failures (reassigned, blocked, user takeover).
pub fn calculate_posterior_p(
    prior_p0: f64,
    prior_strength: f64,
    successes: f64,
    failures: f64,
) -> f64 {
    let kappa = if prior_strength > 0.0 { prior_strength } else { 6.0 };
    let alpha_0 = kappa * prior_p0;
    let beta_0 = kappa * (1.0 - prior_p0);

    let alpha = alpha_0 + successes;
    let beta = beta_0 + failures;

    if alpha + beta <= 0.0 {
        prior_p0
    } else {
        (alpha / (alpha + beta)).clamp(0.01, 0.999)
    }
}

/// Returns the minimum confidence threshold p_min for a task (docs/04 §5).
///
/// Default: 0.80.
/// Difficulty 3: 0.85.
pub fn get_p_min_threshold(difficulty: u8, success_model: &SuccessModel) -> f64 {
    if let Some(ref cfg) = success_model.p_min {
        if difficulty >= 3 {
            cfg.difficulty_3
        } else {
            cfg.default
        }
    } else if difficulty >= 3 {
        0.85
    } else {
        0.80
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_test_success_model() -> SuccessModel {
        let mut req = HashMap::new();
        let mut d1 = HashMap::new();
        d1.insert("S".into(), 2.0);
        d1.insert("M".into(), 2.5);
        d1.insert("L".into(), 3.0);
        req.insert("1".into(), d1);

        let mut d2 = HashMap::new();
        d2.insert("S".into(), 3.0);
        d2.insert("M".into(), 3.5);
        d2.insert("L".into(), 4.0);
        req.insert("2".into(), d2);

        let mut d3 = HashMap::new();
        d3.insert("S".into(), 4.0);
        d3.insert("M".into(), 4.3);
        d3.insert("L".into(), 4.5);
        req.insert("3".into(), d3);

        SuccessModel {
            a: 1.6,
            b: 1.2,
            prior_strength: 6.0,
            req,
            planner_min_skill: 5.0,
            p_min: None,
        }
    }

    #[test]
    fn test_sigmoid_values() {
        assert!((sigmoid(0.0) - 0.5).abs() < 1e-6);
        assert!(sigmoid(10.0) > 0.999);
        assert!(sigmoid(-10.0) < 0.001);
    }

    #[test]
    fn test_prior_success_probability() {
        let model = make_test_success_model();
        // Skill = 3.5, Difficulty = 2, Size = M (req = 3.5)
        // delta = 3.5 - 3.5 = 0
        // z = 1.6 * 0 + 1.2 = 1.2
        // sigmoid(1.2) = 1 / (1 + exp(-1.2)) = ~0.7685
        let p0 = calculate_prior_p0(3.5, 2, "M", &model);
        let expected = 1.0 / (1.0 + (-1.2f64).exp());
        assert!((p0 - expected).abs() < 1e-4);

        // High skill 5.0 on difficulty 1, S (req = 2.0) -> delta = 3.0, z = 1.6 * 3 + 1.2 = 6.0 -> ~0.9975
        let p0_high = calculate_prior_p0(5.0, 1, "S", &model);
        assert!(p0_high > 0.99);

        // Low skill 1.0 on difficulty 3, L (req = 4.5) -> delta = -3.5, z = -4.4 -> ~0.012
        let p0_low = calculate_prior_p0(1.0, 3, "L", &model);
        assert!(p0_low < 0.05);
    }

    #[test]
    fn test_beta_posterior_convergence() {
        let p0 = 0.70;
        let kappa = 6.0;

        // With zero observations, posterior equals prior
        let p_init = calculate_posterior_p(p0, kappa, 0.0, 0.0);
        assert!((p_init - p0).abs() < 1e-6);

        // 10 straight successes pulls posterior up
        let p_success = calculate_posterior_p(p0, kappa, 10.0, 0.0);
        assert!(p_success > p0);

        // 10 straight failures pulls posterior down
        let p_fail = calculate_posterior_p(p0, kappa, 0.0, 10.0);
        assert!(p_fail < p0);
    }

    #[test]
    fn test_threshold_logic() {
        let model = make_test_success_model();
        assert_eq!(get_p_min_threshold(1, &model), 0.80);
        assert_eq!(get_p_min_threshold(2, &model), 0.80);
        assert_eq!(get_p_min_threshold(3, &model), 0.85);
    }
}
