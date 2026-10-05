//! Learning loop, Beta posterior updates, usage profile learning, and preference calibration (docs/04 §9, P6.m6).
//!
//! Tracks:
//! - Success probability Beta posterior updates (alpha += 1.0 on FIRST_PASS, alpha += 0.5 on FIXED_IN_WINDOW; beta += 1.0 on failure).
//! - In-window fix success rate `q` update.
//! - Usage profile learning using Exponential Moving Average (EMA, alpha = 0.20).
//! - Cost calibration coefficient tracking (actual_cost / est_cost).
//! - User reassignment preference signal: increases prior strength / alpha for user-chosen model in that domain.

use serde::{Deserialize, Serialize};
use crate::pack::UsageProfileEntry;

/// Result of an outcome evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskOutcomeResult {
    FirstPass,
    FixedInWindow,
    Reassigned,
    Blocked,
    UserTakeover,
}

impl TaskOutcomeResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::FirstPass => "FIRST_PASS",
            Self::FixedInWindow => "FIXED_IN_WINDOW",
            Self::Reassigned => "REASSIGNED",
            Self::Blocked => "BLOCKED",
            Self::UserTakeover => "USER_TAKEOVER",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "FIRST_PASS" => Some(Self::FirstPass),
            "FIXED_IN_WINDOW" => Some(Self::FixedInWindow),
            "REASSIGNED" => Some(Self::Reassigned),
            "BLOCKED" => Some(Self::Blocked),
            "USER_TAKEOVER" => Some(Self::UserTakeover),
            _ => None,
        }
    }
}

/// Beta posterior accumulator state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BetaAccumulator {
    pub alpha: f64,
    pub beta: f64,
    pub samples: u32,
}

impl BetaAccumulator {
    /// Creates a new Beta accumulator from prior p0 and kappa.
    pub fn new(prior_p0: f64, kappa: f64) -> Self {
        let p = prior_p0.clamp(0.01, 0.99);
        let k = if kappa > 0.0 { kappa } else { 6.0 };
        Self {
            alpha: k * p,
            beta: k * (1.0 - p),
            samples: 0,
        }
    }

    /// Updates Beta distribution from a task outcome (docs/04 §4 & §9).
    /// - FirstPass: +1.0 alpha
    /// - FixedInWindow: +0.5 alpha, +0.5 beta
    /// - Reassigned / Blocked / UserTakeover: +1.0 beta
    pub fn update(&mut self, outcome: TaskOutcomeResult) {
        match outcome {
            TaskOutcomeResult::FirstPass => {
                self.alpha += 1.0;
            }
            TaskOutcomeResult::FixedInWindow => {
                self.alpha += 0.5;
                self.beta += 0.5;
            }
            TaskOutcomeResult::Reassigned
            | TaskOutcomeResult::Blocked
            | TaskOutcomeResult::UserTakeover => {
                self.beta += 1.0;
            }
        }
        self.samples += 1;
    }

    /// Expected posterior probability p = alpha / (alpha + beta).
    pub fn posterior_p(&self) -> f64 {
        if self.alpha + self.beta <= 0.0 {
            0.5
        } else {
            (self.alpha / (self.alpha + self.beta)).clamp(0.01, 0.999)
        }
    }
}

/// Exponential Moving Average factor (alpha = 0.20, docs/04 §9).
pub const EMA_ALPHA: f64 = 0.20;

/// Updates a scalar metric using Exponential Moving Average (EMA) or weighted average for low samples.
pub fn update_metric_ema(current_val: f64, new_sample: f64, total_samples: u32) -> f64 {
    if total_samples <= 1 {
        new_sample
    } else if total_samples < 5 {
        // Weighted mix with definition pack initial value (docs/04 §9)
        let weight = (total_samples as f64) / 5.0;
        current_val * (1.0 - weight) + new_sample * weight
    } else {
        // EMA: value = (1 - alpha) * current + alpha * sample
        (1.0 - EMA_ALPHA) * current_val + EMA_ALPHA * new_sample
    }
}

/// Updates usage profile entry with new turn/call observations.
pub fn update_usage_profile_entry(
    current: &UsageProfileEntry,
    actual_turns: u32,
    actual_calls_per_turn: u32,
    actual_growth_per_turn: u64,
    actual_output_per_turn: u64,
    samples: u32,
) -> UsageProfileEntry {
    let updated_turns = update_metric_ema(current.turns as f64, actual_turns as f64, samples)
        .round()
        .max(1.0) as u32;

    let updated_calls = update_metric_ema(
        current.calls_per_turn as f64,
        actual_calls_per_turn as f64,
        samples,
    )
    .round()
    .max(1.0) as u32;

    let updated_growth = update_metric_ema(
        current.growth_per_turn as f64,
        actual_growth_per_turn as f64,
        samples,
    )
    .round()
    .max(500.0) as u64;

    let updated_output = update_metric_ema(
        current.output_per_turn as f64,
        actual_output_per_turn as f64,
        samples,
    )
    .round()
    .max(200.0) as u64;

    UsageProfileEntry {
        turns: updated_turns,
        calls_per_turn: updated_calls,
        growth_per_turn: updated_growth,
        output_per_turn: updated_output,
    }
}

/// Computes cost calibration factor update:
/// `ratio = actual_cost / estimated_cost`
pub fn update_cost_calibration(
    current_calibration: f64,
    actual_cost: f64,
    estimated_cost: f64,
    samples: u32,
) -> f64 {
    if estimated_cost <= 0.0 || actual_cost <= 0.0 {
        return current_calibration;
    }
    let ratio = actual_cost / estimated_cost;
    update_metric_ema(current_calibration, ratio, samples).clamp(0.2, 5.0)
}

/// Incorporates user reassignment signal as preference reinforcement (docs/04 §9).
/// Increases alpha accumulator for the preferred model in that domain.
pub fn apply_user_preference_signal(acc: &mut BetaAccumulator, boost_strength: f64) {
    acc.alpha += boost_strength.max(1.0);
    acc.samples += 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_beta_accumulator_progression() {
        // Prior p0 = 0.80, kappa = 6.0 -> alpha = 4.8, beta = 1.2
        let mut beta = BetaAccumulator::new(0.80, 6.0);
        assert!((beta.posterior_p() - 0.80).abs() < 1e-4);

        // First pass success (+1 alpha)
        beta.update(TaskOutcomeResult::FirstPass);
        assert!(beta.posterior_p() > 0.80);

        // Fixed in window (+0.5 alpha, +0.5 beta)
        beta.update(TaskOutcomeResult::FixedInWindow);
        assert_eq!(beta.samples, 2);

        // Failure (+1 beta)
        beta.update(TaskOutcomeResult::Reassigned);
        assert!(beta.posterior_p() < 0.85);
    }

    #[test]
    fn test_convergence_direction_after_outcome_sequence() {
        let mut beta_strong = BetaAccumulator::new(0.70, 6.0);
        // 10 consecutive first pass successes
        for _ in 0..10 {
            beta_strong.update(TaskOutcomeResult::FirstPass);
        }
        assert!(beta_strong.posterior_p() > 0.88);

        let mut beta_weak = BetaAccumulator::new(0.70, 6.0);
        // 10 consecutive failures
        for _ in 0..10 {
            beta_weak.update(TaskOutcomeResult::Blocked);
        }
        assert!(beta_weak.posterior_p() < 0.30);
    }

    #[test]
    fn test_ema_usage_profile_update() {
        let initial = UsageProfileEntry {
            turns: 2,
            calls_per_turn: 10,
            growth_per_turn: 15_000,
            output_per_turn: 5_000,
        };

        let updated = update_usage_profile_entry(&initial, 3, 14, 20_000, 7_000, 6);
        assert!(updated.calls_per_turn > 10);
        assert!(updated.growth_per_turn > 15_000);
    }

    #[test]
    fn test_preference_signal_boosts_alpha() {
        let mut beta = BetaAccumulator::new(0.75, 6.0);
        let p_before = beta.posterior_p();
        apply_user_preference_signal(&mut beta, 2.0);
        assert!(beta.posterior_p() > p_before);
    }
}
