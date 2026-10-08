//! Tokage Orchestration Engine (docs/03, P7).
//!
//! Submodules:
//! - `state`: Task lifecycle state machine (P7.m1a).
//! - `scheduler`: DAG dependency scheduling, critical path prioritization, concurrency limits,
//!   and contract deviation pause (P7.m1b, P7.m1c).
//! - `brief`: Task brief assembly with fixed rules header and upstream artifacts (P7.m2a, P7.m2c).
//! - `report`: Task report parsing, local extraction fallbacks, and markdown rendering (P7.m2b, P7.m2d).

pub mod state;
pub mod scheduler;
pub mod brief;
pub mod gate;
pub mod repair;
pub mod lifecycle;
pub mod tree;

pub use state::{StateChangeReason, TaskState, TaskStateTransition};
pub use scheduler::{ConcurrencyLimits, DagScheduler, DispatchIneligibleReason, ScheduledTask};
pub use brief::{
    assemble_task_brief, parse_or_generate_task_report, TaskBriefInput, TaskReport, UpstreamArtifact,
    TASK_BRIEF_FIXED_HEADER,
};
pub use gate::{
    cancel_gate_command, check_gate_command_safety, execute_gate_command, gate_runs,
    revert_out_of_scope_files, validate_write_scope, GateCommandPermission, GateCommandResult,
    GateRequest, GateRuns, ScopeValidationResult, DEFAULT_GATE_TIMEOUT_SECS, GATE_EVIDENCE_TAIL_LINES,
};

#[cfg(test)]
mod gate_f5_tests;
pub use repair::{
    decide_repair_action, decide_repair_action_with_melted, generate_implementation_fix_task,
    generate_terminal_resume_command, BlockedResolution, FailureEvidence, RepairDecision, RepairStep,
};
pub use lifecycle::{
    generate_delivery_summary, merge_task_worktree, recover_interrupted_tasks, DeliverySummary,
    PipelineStatus, WorktreeMergeOutcome,
};
pub use tree::{
    derive_session_title_from_spec, AttemptNode, AttemptRole, MainSessionStatus, PlanNode,
    SessionTreeNode, TaskNode, UiEvent, UiEventBatcher, UnifiedApprovalItem,
};

