//! Tokage Context Lifecycle Engine (docs/05, P8.m1).
//!
//! Provides:
//! - Window reuse decisions (docs/05 §4, P8.m1a)
//! - In-window context optimization: CONTINUE / COMPACT / FRESH (docs/05 §5, P8.m1b)
//! - Native CLI compact integration and fallback (docs/05 §5, P8.m1c)
//! - Main session appended user message routing (docs/05 §6, P8.m1e)
//! - R9 and R10 context & cache discipline enforcement (docs/05 §8, P8.m1f)
//! - Context action records and actual savings settlement (docs/05 §7, P8.m1f)

pub mod compact;
pub mod discipline;
pub mod in_window;
pub mod inputs;
pub mod reuse;
pub mod settlement;
pub mod triage;

#[cfg(test)]
pub mod acceptance;

pub use compact::{
    build_compact, get_provider_compact_capability, BuildCompactRequest, CompactExecutionPayload,
    CompactMethod, ProviderCompactCapability, STANDARD_COMPACT_INSTRUCTION,
};
pub use discipline::{
    validate_no_fake_keepalives, validate_task_brief_header, validate_user_message_integrity,
    validate_window_continuity, DisciplineViolation, WindowConfig,
};
pub use in_window::{
    is_cache_warm_now, plan_context_action, ActionTrigger, ContextActionKind, ContextActionPlan,
    InWindowContextParams,
};
pub use reuse::{
    decide_window_reuse, scopes_are_adjacent, WindowReuseDecision, WindowReuseParams,
};
pub use settlement::{
    create_context_action_record, settle_actual_savings, ContextActionRecord, ContextActionStatus,
};
pub use triage::{
    triage_main_session_message, AppendMessageRoute, TaskSummaryForTriage, TriageMessageParams,
};
