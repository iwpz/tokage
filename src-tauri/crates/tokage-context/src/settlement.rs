//! Context action records and actual savings settlement (docs/05 §7, docs/08 schema.md §4, P8.m1f).

use serde::{Deserialize, Serialize};
use super::in_window::{ActionTrigger, ContextActionKind, ContextActionPlan};

/// Status of a context action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContextActionStatus {
    Suggested,
    Applied,
    Dismissed,
    Undone,
    Failed,
}

impl std::fmt::Display for ContextActionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Suggested => write!(f, "SUGGESTED"),
            Self::Applied => write!(f, "APPLIED"),
            Self::Dismissed => write!(f, "DISMISSED"),
            Self::Undone => write!(f, "UNDONE"),
            Self::Failed => write!(f, "FAILED"),
        }
    }
}

/// In-memory and IPC record for a context action.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextActionRecord {
    pub id: Option<i64>,
    pub attempt_id: i64,
    pub kind: ContextActionKind,
    pub trigger: ActionTrigger,
    pub status: ContextActionStatus,
    pub est_costs_json: String,
    pub est_saving_pct: f64,
    pub actual_saving_usd: Option<f64>,
    pub new_attempt_id: Option<i64>,
    pub created_at: String,
    pub applied_at: Option<String>,
}

/// Creates a new action record from a planned action.
pub fn create_context_action_record(
    attempt_id: i64,
    plan: &ContextActionPlan,
    status: ContextActionStatus,
    created_at_iso: String,
) -> ContextActionRecord {
    let est_costs = serde_json::json!({
        "continue": plan.continue_cost_usd,
        "compact": plan.compact_cost_usd,
        "fresh": plan.fresh_cost_usd,
    });

    ContextActionRecord {
        id: None,
        attempt_id,
        kind: plan.recommended_action,
        trigger: plan.trigger,
        status,
        est_costs_json: est_costs.to_string(),
        est_saving_pct: plan.estimated_saving_percentage,
        actual_saving_usd: None,
        new_attempt_id: None,
        created_at: created_at_iso,
        applied_at: None,
    }
}

/// Compare costs for the same explicitly identified horizon. Missing, invalid, or
/// incomparable observations are unknown, not zero savings.
pub fn settle_actual_savings(baseline: Option<(u32, f64)>, actual: Option<(u32, f64)>) -> Option<f64> {
    let ((baseline_horizon, baseline_cost), (actual_horizon, actual_cost)) = (baseline?, actual?);
    if baseline_horizon == 0 || baseline_horizon != actual_horizon ||
        !baseline_cost.is_finite() || !actual_cost.is_finite() || baseline_cost < 0.0 || actual_cost < 0.0 {
        return None;
    }
    Some((baseline_cost - actual_cost).max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_savings_settlement() {
        assert!((settle_actual_savings(Some((1, 0.50)), Some((1, 0.32))).unwrap() - 0.18).abs() < 1e-6);
        assert_eq!(settle_actual_savings(Some((1, 0.30)), Some((1, 0.45))), Some(0.0));
        assert_eq!(settle_actual_savings(Some((1, 0.30)), Some((1, 0.30))), Some(0.0));
        assert_eq!(settle_actual_savings(Some((1, 0.30)), None), None);
        assert_eq!(settle_actual_savings(Some((4, 0.30)), Some((1, 0.20))), None);
        assert_eq!(settle_actual_savings(Some((0, 0.30)), Some((0, 0.20))), None);
        assert_eq!(settle_actual_savings(Some((1, f64::NAN)), Some((1, 0.20))), None);
    }
}
