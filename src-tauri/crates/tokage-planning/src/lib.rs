//! Tokage planning engine: repo digest, triage, and plan management (docs/02).

pub mod digest;
pub mod parser;
pub mod prompt;
pub mod replan;
pub mod storage;
pub mod triage;
pub mod validator;

pub use digest::{generate_digest, RepoDigestBuilder};
pub use parser::{parse_plan, PlanParseError, PlanParseOptions};
pub use prompt::{
    render_planning_prompt, PlanningPromptArgs, DEFAULT_DOMAINS, PLANNING_SYSTEM_TEMPLATE,
    PLAN_FORMAT_SPEC,
};
pub use replan::{
    apply_plan_diff, assemble_replan_prompt, compute_plan_diff, generate_adaptation_tasks,
    ContractDiff, DiffAction, PlanDiff, ReplanInputArgs, ReplanTaskStatus, SpecDiff, TaskDiff,
    TaskExecutionReport, DEFAULT_REPLAN_INPUT_MAX_TOKENS,
};
pub use storage::{
    ensure_git_info_exclude, load_persisted_plan, load_persisted_plan_version, persist_plan,
    render_full_plan_markdown, render_spec_markdown, PersistPlanResult,
};
pub use triage::{triage, default_domain_keywords, TriageEngine, TriageOptions};
pub use validator::{
    validate_plan, PlanValidationOptions, ValidationIssue, ValidationResult, ValidationSeverity,
};


