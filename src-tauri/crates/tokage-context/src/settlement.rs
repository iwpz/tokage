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

/// Settles the actual savings once execution completes:
/// `actual_saving = baseline_continue_cost - actual_turn_cost`.
pub fn settle_actual_savings(baseline_continue_cost: f64, actual_turn_cost: f64) -> f64 {
    (baseline_continue_cost - actual_turn_cost).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_savings_settlement() {
        let baseline = 0.50;
        let actual = 0.32;
        let saving = settle_actual_savings(baseline, actual);
        assert!((saving - 0.18).abs() < 1e-6);

        let negative = settle_actual_savings(0.30, 0.45);
        assert_eq!(negative, 0.0);
    }
}
