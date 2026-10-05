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

pub use state::{StateChangeReason, TaskState, TaskStateTransition};
pub use scheduler::{ConcurrencyLimits, DagScheduler, DispatchIneligibleReason, ScheduledTask};
pub use brief::{
    assemble_task_brief, parse_or_generate_task_report, TaskBriefInput, TaskReport, UpstreamArtifact,
    TASK_BRIEF_FIXED_HEADER,
};
