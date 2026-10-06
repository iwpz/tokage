//! Task state machine for Tokage orchestration engine (docs/03 §2, P7.m1a).
//!
//! State transitions:
//! ```text
//! PENDING ──dependencies complete──► READY ──slot allocated──► RUNNING ──► VERIFYING
//!    ▲                                                                      │
//!    │                                           pass ◄─────────────────────┤
//!    │                                             │                        │ fail
//!    │                                             ▼                        ▼
//!    │                                           DONE                   REPAIRING ──► RUNNING
//!    │                                                                      │
//!    └──── replan/reset ────────────────────────── BLOCKED ◄────────────────┘ fallback exhausted / user review
//! ```
//! Also supports CANCELLED and SKIPPED states.

use serde::{Deserialize, Serialize};

/// State of an individual task in a plan execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskState {
    /// Waiting for upstream task dependencies to complete.
    Pending,
    /// All dependencies completed, waiting for execution slot / provider availability.
    Ready,
    /// Currently executing in an agent / CLI process.
    Running,
    /// Execution completed; currently running local automated acceptance gates.
    Verifying,
    /// Passed acceptance verification.
    Done,
    /// Acceptance gate failed; undergoing in-window or reassigned repair.
    Repairing,
    /// Repair attempts exhausted or contract deviation requires human decision.
    Blocked,
    /// Cancelled by user or pipeline shutdown.
    Cancelled,
    /// Skipped because upstream dependency failed or was cancelled.
    Skipped,
    /// Paused due to model quota exhaustion / rate limit (429); can be resumed after recharge/model switch.
    PausedQuota,
    /// Upstream dependency paused due to quota exhaustion; waiting for upstream unblock.
    WaitingUpstream,
    /// Interrupted abnormally due to app crash, sudden power loss or process kill mid-turn; can be resumed.
    Interrupted,
}

impl TaskState {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Done | Self::Cancelled | Self::Skipped)
    }

    pub fn is_active(&self) -> bool {
        matches!(self, Self::Running | Self::Verifying | Self::Repairing)
    }

    pub fn can_transition_to(&self, target: TaskState) -> bool {
        if *self == target {
            return true;
        }

        match (self, target) {
            (Self::Pending, TaskState::Ready) => true,
            (Self::Pending, TaskState::Cancelled) => true,
            (Self::Pending, TaskState::Skipped) => true,
            (Self::Pending, TaskState::WaitingUpstream) => true,

            (Self::Ready, TaskState::Running) => true,
            (Self::Ready, TaskState::Cancelled) => true,
            (Self::Ready, TaskState::Skipped) => true,
            (Self::Ready, TaskState::PausedQuota) => true,
            (Self::Ready, TaskState::WaitingUpstream) => true,

            (Self::Running, TaskState::Verifying) => true,
            // If execution aborted / errored before verification
            (Self::Running, TaskState::Repairing) => true,
            (Self::Running, TaskState::Blocked) => true,
            (Self::Running, TaskState::Cancelled) => true,
            (Self::Running, TaskState::PausedQuota) => true,
            (Self::Running, TaskState::Interrupted) => true,

            (Self::Verifying, TaskState::Done) => true,
            (Self::Verifying, TaskState::Repairing) => true,
            (Self::Verifying, TaskState::Blocked) => true,
            (Self::Verifying, TaskState::Cancelled) => true,
            (Self::Verifying, TaskState::Interrupted) => true,

            (Self::Repairing, TaskState::Running) => true,
            (Self::Repairing, TaskState::Blocked) => true,
            (Self::Repairing, TaskState::Cancelled) => true,
            (Self::Repairing, TaskState::PausedQuota) => true,
            (Self::Repairing, TaskState::Interrupted) => true,

            // BLOCKED can transition back to PENDING or READY upon replan / human unblock, or CANCELLED / SKIPPED
            (Self::Blocked, TaskState::Pending) => true,
            (Self::Blocked, TaskState::Ready) => true,
            (Self::Blocked, TaskState::Cancelled) => true,
            (Self::Blocked, TaskState::Skipped) => true,

            // PAUSED_QUOTA can resume to READY or RUNNING after quota restored/reassigned, or CANCELLED
            (Self::PausedQuota, TaskState::Ready) => true,
            (Self::PausedQuota, TaskState::Running) => true,
            (Self::PausedQuota, TaskState::Cancelled) => true,

            // WAITING_UPSTREAM can return to PENDING or READY once upstream resolves
            (Self::WaitingUpstream, TaskState::Pending) => true,
            (Self::WaitingUpstream, TaskState::Ready) => true,
            (Self::WaitingUpstream, TaskState::Cancelled) => true,

            // INTERRUPTED can resume back to READY or RUNNING, or be cancelled
            (Self::Interrupted, TaskState::Ready) => true,
            (Self::Interrupted, TaskState::Running) => true,
            (Self::Interrupted, TaskState::Cancelled) => true,

            _ => false,
        }
    }
}

/// Reason or trigger for a task state change.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateChangeReason {
    pub trigger: String,
    pub message: Option<String>,
}

impl StateChangeReason {
    pub fn new(trigger: impl Into<String>) -> Self {
        Self {
            trigger: trigger.into(),
            message: None,
        }
    }

    pub fn with_message(trigger: impl Into<String>, msg: impl Into<String>) -> Self {
        Self {
            trigger: trigger.into(),
            message: Some(msg.into()),
        }
    }
}

/// A recorded transition of a task state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStateTransition {
    pub task_id: String,
    pub from: TaskState,
    pub to: TaskState,
    pub timestamp_ms: u64,
    pub reason: StateChangeReason,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_transitions() {
        assert!(TaskState::Pending.can_transition_to(TaskState::Ready));
        assert!(TaskState::Ready.can_transition_to(TaskState::Running));
        assert!(TaskState::Running.can_transition_to(TaskState::Verifying));
        assert!(TaskState::Verifying.can_transition_to(TaskState::Done));
        assert!(TaskState::Verifying.can_transition_to(TaskState::Repairing));
        assert!(TaskState::Repairing.can_transition_to(TaskState::Running));
        assert!(TaskState::Repairing.can_transition_to(TaskState::Blocked));
        assert!(TaskState::Blocked.can_transition_to(TaskState::Ready));
    }

    #[test]
    fn test_invalid_transitions() {
        assert!(!TaskState::Pending.can_transition_to(TaskState::Done));
        assert!(!TaskState::Done.can_transition_to(TaskState::Running));
        assert!(!TaskState::Cancelled.can_transition_to(TaskState::Running));
        assert!(!TaskState::Ready.can_transition_to(TaskState::Verifying));
    }

    #[test]
    fn test_terminal_and_active_predicates() {
        assert!(TaskState::Done.is_terminal());
        assert!(TaskState::Cancelled.is_terminal());
        assert!(TaskState::Skipped.is_terminal());
        assert!(!TaskState::Blocked.is_terminal());

        assert!(TaskState::Running.is_active());
        assert!(TaskState::Verifying.is_active());
        assert!(TaskState::Repairing.is_active());
        assert!(!TaskState::Pending.is_active());
    }
}
