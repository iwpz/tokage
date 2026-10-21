//! Transport-independent application commands and durable orchestration decisions.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const SCHEMA_VERSION: u32 = 1;
#[derive(Clone, Debug, thiserror::Error, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "code", content = "detail")]
pub enum Error {
    #[error("unsupported schema version")]
    Schema,
    #[error("turn cancelled")]
    Cancelled,
    #[error("turn awaiting scoped approval")]
    AwaitingApproval,
    #[error("native process cleanup is unknown")]
    CleanupUnknown,
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("revision conflict")]
    RevisionConflict,
    #[error("command ID reused with different input")]
    CommandConflict,
    #[error("run owner conflict")]
    OwnerConflict,
    #[error("command unsupported in this service version")]
    Unsupported,
    #[error("invalid state transition")]
    Transition,
    #[error("repository unavailable or corrupt: {0}")]
    Repository(String),
}
pub type Result<T> = std::result::Result<T, Error>;
pub fn digest<T: Serialize>(value: &T) -> Result<String> {
    // Value maps use sorted keys (serde_json preserve_order is deliberately disabled).
    let value = serde_json::to_value(value).map_err(|e| Error::Invalid(e.to_string()))?;
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&value).map_err(|e| Error::Invalid(e.to_string()))?)
    ))
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelAssignment {
    pub provider: String,
    pub requested_model: String,
    pub provider_path: Option<String>,
    pub capabilities: Vec<String>,
    pub user_locked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_params: Option<tokage_core::planning::ModelParams>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EffectiveRunConfig {
    pub source: String,
    pub policy_version: String,
    pub workspace: String,
    pub target_identity: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_reference: Option<String>,
    pub scope: Vec<String>,
    pub models: BTreeMap<String, ModelAssignment>,
    pub command_policy_hash: String,
    pub trust_workspace: bool,
    pub permission_mode: String,
    pub concurrency: u32,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub provider_concurrency: BTreeMap<String, u32>,
    pub budget_usd: Option<f64>,
    pub block_unknown_cost: bool,
    pub timeout_seconds: u64,
    pub repair_policy: String,
    pub context_policy: String,
    pub confirmed_commands: Vec<String>,
    pub custom_pack_hashes: Vec<String>,
    pub co_author: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planning_policy: Option<planning::PlanningPolicy>,
}
impl EffectiveRunConfig {
    pub fn validate(&self) -> Result<()> {
        self.validate_shape()?;
        planning::effective_pack(self)?;
        Ok(())
    }
    fn validate_shape(&self) -> Result<()> {
        if [
            self.source.as_str(),
            &self.policy_version,
            &self.workspace,
            &self.target_identity,
        ]
        .iter()
        .any(|s| s.trim().is_empty())
            || self.command_policy_hash.trim().is_empty()
            || self.concurrency == 0
            || self
                .provider_concurrency
                .iter()
                .any(|(k, v)| k.trim().is_empty() || *v == 0)
            || self.timeout_seconds == 0
            || self.budget_usd.is_some_and(|n| !n.is_finite() || n < 0.0)
            || !["manual", "bounded-ladder"].contains(&self.repair_policy.as_str())
            || !["auto", "suggest", "off"].contains(&self.context_policy.as_str())
            || !["read-only", "workspace-write", "full-auto"]
                .contains(&self.permission_mode.as_str())
            || self.models.is_empty()
            || self.models.iter().any(|(k, v)| {
                k.trim().is_empty()
                    || v.provider.trim().is_empty()
                    || v.requested_model.trim().is_empty()
            })
        {
            return Err(Error::Invalid("effective config".into()));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum RunState {
    Draft,
    Planning,
    AwaitingApproval,
    AwaitingTurnApproval,
    Approved,
    Executing,
    CancelRequested,
    Stopping,
    Stopped,
    Blocked,
    AwaitingReview,
    Reviewing,
    AwaitingMerge,
    Delivering,
    Delivered,
    Historical,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Snapshot {
    pub schema_version: u32,
    pub run_id: String,
    pub revision: u64,
    pub state: RunState,
    pub requirement: String,
    pub config: EffectiveRunConfig,
    pub config_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<planning::PlanRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planning_attempt: Option<planning::PlanningAttempt>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub turns: BTreeMap<String, turn::TurnRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<execution::Execution>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work: Option<artifacts::Work>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery: Option<delivery::Delivery>,
}
impl Snapshot {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(Error::Schema);
        }
        self.config.validate_shape()?;
        if self.revision == 0
            || self.run_id.trim().is_empty()
            || self.requirement.trim().is_empty()
            || self.config_hash != digest(&self.config)?
        {
            return Err(Error::Invalid("snapshot identity/config hash".into()));
        }
        planning::validate_snapshot(self)?;
        turn::validate_snapshot(self)?;
        execution::validate_snapshot(self)?;
        artifacts::validate_snapshot(self)?;
        delivery::validate_snapshot(self)?;
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum ApprovalDecision {
    AllowOnce,
    AllowSession,
    Deny,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "payload", deny_unknown_fields)]
pub enum Command {
    CreateRun {
        requirement: String,
        config: EffectiveRunConfig,
    },
    GeneratePlan,
    UpdateConfig {
        config: EffectiveRunConfig,
    },
    ImportPlan {
        text: String,
    },
    ApprovePlan {
        confirmation_digest: String,
    },
    StartExecution {
        confirmation_digest: String,
    },
    ResolveApproval {
        decision: ApprovalDecision,
        approval_id: String,
        turn_id: String,
        confirmation_digest: String,
    },
    CancelRun,
    RetryTask {
        task_id: String,
    },
    ResumeRun,
    ReviewDelivery,
    MergeDelivery {
        confirmation_digest: String,
    },
    ExportPatch {
        confirmation_digest: String,
        output_path: String,
    },
    KeepBranch {
        confirmation_digest: String,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub command_id: String,
    pub run_id: String,
    pub expected_revision: u64,
    pub command: Command,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum EventKind {
    RunCreated,
    DraftCancelled,
    PlanImported,
    ConfigUpdated,
    PlanApproved,
    PlanningRequested,
    PlanningClaimed,
    PlanningCompleted,
    PlanningFailed,
    TurnObserved,
    TurnFinished,
    CancelRequested,
    Stopping,
    Stopped,
    ApprovalResolved,
    RecoveryRequested,
    ExecutionRequested,
    ExecutionProgress,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Event {
    pub schema_version: u32,
    pub run_id: String,
    pub sequence: u64,
    pub revision: u64,
    pub attempt_id: Option<String>,
    pub turn_id: Option<String>,
    pub kind: EventKind,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Receipt {
    pub run_id: String,
    pub command_id: String,
    pub revision: u64,
}
/// Pure decision: repository must fence revision and persist snapshot/event/receipt atomically.
pub fn transition(current: Option<&Snapshot>, request: &Request) -> Result<(Snapshot, Event)> {
    if request.schema_version != SCHEMA_VERSION {
        return Err(Error::Schema);
    }
    if request.command_id.trim().is_empty() || request.run_id.trim().is_empty() {
        return Err(Error::Invalid("empty identity".into()));
    }
    if let Some(s) = current {
        s.validate()?;
        if s.run_id != request.run_id {
            return Err(Error::Invalid("run identity".into()));
        }
    }
    if current.map_or(0, |s| s.revision) != request.expected_revision {
        return Err(Error::RevisionConflict);
    }
    let (mut next, kind) = match (&request.command, current) {
        (
            Command::CreateRun {
                requirement,
                config,
            },
            None,
        ) => {
            config.validate()?;
            if requirement.trim().is_empty() {
                return Err(Error::Invalid("empty requirement".into()));
            }
            (
                Snapshot {
                    schema_version: SCHEMA_VERSION,
                    run_id: request.run_id.clone(),
                    revision: 0,
                    state: RunState::Draft,
                    requirement: requirement.clone(),
                    config: config.clone(),
                    config_hash: digest(config)?,
                    plan: None,
                    approved_digest: None,
                    planning_attempt: None,
                    turns: BTreeMap::new(),
                    execution: None,
                    work: None,
                    delivery: None,
                },
                EventKind::RunCreated,
            )
        }
        (Command::CreateRun { .. }, Some(_)) => return Err(Error::Transition),
        (Command::GeneratePlan, Some(s)) if planning::editable_snapshot(s) => {
            if !s.config.trust_workspace {
                return Err(Error::Invalid(
                    "planning requires explicit workspace trust".into(),
                ));
            }
            let mut next = s.clone();
            next.plan = None;
            next.approved_digest = None;
            next.planning_attempt = Some(planning::PlanningAttempt {
                id: digest(&(
                    request.run_id.as_str(),
                    request.command_id.as_str(),
                    &s.config_hash,
                ))?,
                claimed: false,
                error: None,
                continuation_of: None,
            });
            next.state = RunState::Planning;
            (next, EventKind::PlanningRequested)
        }
        (Command::GeneratePlan, _) => return Err(Error::Transition),
        (Command::ImportPlan { text }, Some(s)) if planning::editable_snapshot(s) => {
            let mut next = s.clone();
            next.plan = Some(planning::normalize(text, &s.config)?);
            next.planning_attempt = None;
            next.approved_digest = None;
            next.state = RunState::AwaitingApproval;
            (next, EventKind::PlanImported)
        }
        (Command::UpdateConfig { config }, Some(s)) if planning::editable_snapshot(s) => {
            config.validate()?;
            let mut next = s.clone();
            next.config = config.clone();
            next.config_hash = digest(config)?;
            next.approved_digest = None;
            next.plan = s
                .plan
                .as_ref()
                .map(|p| planning::normalize(&p.plan.raw_markdown, config))
                .transpose()?;
            next.state = if next.plan.is_some() {
                RunState::AwaitingApproval
            } else {
                RunState::Draft
            };
            (next, EventKind::ConfigUpdated)
        }
        (
            Command::ApprovePlan {
                confirmation_digest,
            },
            Some(s),
        ) if s.state == RunState::AwaitingApproval => {
            let plan = s.plan.as_ref().ok_or(Error::Transition)?;
            planning::approve(s, confirmation_digest)?;
            let mut next = s.clone();
            next.approved_digest = Some(plan.confirmation_digest.clone());
            next.state = RunState::Approved;
            (next, EventKind::PlanApproved)
        }
        (
            Command::ImportPlan { .. } | Command::UpdateConfig { .. } | Command::ApprovePlan { .. },
            _,
        ) => return Err(Error::Transition),
        (
            Command::StartExecution {
                confirmation_digest,
            },
            Some(s),
        ) if s.state == RunState::Approved => {
            planning::approve(s, confirmation_digest)?;
            if s.approved_digest.as_deref() != Some(confirmation_digest)
                || s.config
                    .target_reference
                    .as_ref()
                    .is_none_or(|r| !r.starts_with("refs/heads/"))
            {
                return Err(Error::Invalid(
                    "approved execution requires exact target branch identity".into(),
                ));
            }
            let mut next = s.clone();
            next.execution = Some(execution::Execution::new(s)?);
            next.work = Some(artifacts::Work::default());
            next.state = RunState::Executing;
            (next, EventKind::ExecutionRequested)
        }
        (Command::ReviewDelivery, Some(s))
            if matches!(s.state, RunState::AwaitingReview | RunState::Blocked) =>
        {
            let mut next = s.clone();
            next.delivery = Some(delivery::new(s, &request.command_id)?);
            next.state = RunState::Reviewing;
            (next, EventKind::ExecutionProgress)
        }
        (
            Command::MergeDelivery {
                confirmation_digest,
            },
            Some(s),
        ) => {
            let mut next = s.clone();
            delivery::confirm(
                &mut next,
                &request.command_id,
                confirmation_digest,
                delivery::DeliveryMode::Merge,
            )?;
            (next, EventKind::ExecutionProgress)
        }
        (
            Command::ExportPatch {
                confirmation_digest,
                output_path,
            },
            Some(s),
        ) => {
            let mut next = s.clone();
            delivery::confirm(
                &mut next,
                &request.command_id,
                confirmation_digest,
                delivery::DeliveryMode::Export {
                    path: output_path.clone(),
                },
            )?;
            (next, EventKind::ExecutionProgress)
        }
        (
            Command::KeepBranch {
                confirmation_digest,
            },
            Some(s),
        ) => {
            let mut next = s.clone();
            delivery::confirm(
                &mut next,
                &request.command_id,
                confirmation_digest,
                delivery::DeliveryMode::KeepBranch,
            )?;
            (next, EventKind::ExecutionProgress)
        }
        (Command::RetryTask { task_id }, Some(s)) if s.state == RunState::Blocked => {
            let p = s.plan.as_ref().ok_or(Error::Transition)?;
            if s.approved_digest.as_ref() != Some(&p.confirmation_digest)
                || s.turns.values().any(|t| !t.ended || !t.cleanup_confirmed)
            {
                return Err(Error::Invalid(
                    "retry requires valid approval and confirmed native cleanup".into(),
                ));
            }
            let mut next = s.clone();
            let ex = next.execution.as_mut().ok_or(Error::Transition)?;
            if ex.tasks.values().any(|t| t.reservation_active) {
                return Err(Error::Transition);
            }
            let task = ex.tasks.get_mut(task_id).ok_or(Error::Transition)?;
            if task.state != tokage_orchestration::TaskState::Blocked || task.deviation {
                return Err(Error::Transition);
            }
            if task
                .attempts
                .last()
                .and_then(|id| s.work.as_ref()?.attempts.get(id))
                .is_some_and(|a| a.commit_intent || a.gates.iter().any(|g| !g.cleanup_confirmed))
            {
                return Err(Error::Invalid(
                    "Git/gate effects require reconciliation before retry".into(),
                ));
            }
            task.state = tokage_orchestration::TaskState::Pending;
            task.repair_step = tokage_orchestration::RepairStep::Initial;
            ex.claimed = false;
            next.state = RunState::Executing;
            (next, EventKind::ExecutionRequested)
        }
        (Command::ResumeRun, Some(s))
            if matches!(
                s.state,
                RunState::Planning
                    | RunState::Executing
                    | RunState::Reviewing
                    | RunState::Delivering
                    | RunState::Blocked
                    | RunState::CancelRequested
                    | RunState::Stopping
            ) =>
        {
            let mut next = s.clone();
            next.state = RunState::Blocked;
            for t in next
                .turns
                .values_mut()
                .filter(|t| !t.ended || !t.cleanup_confirmed)
            {
                t.status = turn::TurnStatus::Unknown;
                t.error=Some("interrupted native effect requires independent reconciliation; no automatic relaunch".into());
            }
            (next, EventKind::RecoveryRequested)
        }
        (
            Command::ResolveApproval {
                decision,
                approval_id,
                turn_id,
                confirmation_digest,
            },
            Some(s),
        ) if s.state == RunState::AwaitingTurnApproval => {
            let mut next = s.clone();
            let turn = next.turns.get_mut(turn_id).ok_or(Error::Transition)?;
            let approval = turn.approval.as_mut().ok_or(Error::Transition)?;
            if !turn.ended
                || !turn.cleanup_confirmed
                || approval.id != *approval_id
                || approval.turn_id != *turn_id
                || approval.decision.is_some()
                || approval.confirmation_digest != *confirmation_digest
            {
                return Err(Error::Invalid("expired approval window".into()));
            }
            if *decision != ApprovalDecision::Deny {
                planning::guard_approval_command(&s.config, &approval.command)?;
            }
            let scoped = *decision == ApprovalDecision::AllowSession
                && approval.recoverable
                && turn.thread_id.is_some()
                && turn.requested.provider == "CLAUDE_CODE"
                && !approval.command.trim().is_empty()
                && !approval
                    .command
                    .contains(['\r', '\n', '*', '?', '(', ')', '[', ']', '\\']);
            approval.decision = Some(decision.clone());
            if !scoped {
                next.state = RunState::Blocked;
                turn.error = Some(
                    if *decision == ApprovalDecision::Deny {
                        "approval denied"
                    } else {
                        "adapter cannot enforce this approval scope; manual intervention required"
                    }
                    .into(),
                );
            } else if turn.role == "reviewer" {
                let mut d = delivery::new(s, &request.command_id)?;
                d.continuation_of = Some(turn_id.clone());
                d.candidate = s.delivery.as_ref().and_then(|d| d.candidate.clone());
                d.session_allowed = s
                    .delivery
                    .as_ref()
                    .map(|d| d.session_allowed.clone())
                    .unwrap_or_default();
                d.session_allowed.push(approval.command.clone());
                next.delivery = Some(d);
                next.state = RunState::Reviewing;
            } else if turn.role == "executor" {
                let id = turn.task_id.clone().ok_or(Error::Transition)?;
                let ex = next.execution.as_mut().ok_or(Error::Transition)?;
                if ex.tasks.values().any(|t| t.reservation_active) {
                    return Err(Error::Transition);
                }
                let task = ex.tasks.get_mut(&id).ok_or(Error::Transition)?;
                task.continuation_of = Some(turn_id.clone());
                task.session_allowed.push(approval.command.clone());
                task.state = tokage_orchestration::TaskState::Pending;
                ex.claimed = false;
                next.state = RunState::Executing;
            } else {
                next.planning_attempt = Some(planning::PlanningAttempt {
                    id: digest(&(
                        request.run_id.as_str(),
                        request.command_id.as_str(),
                        &s.config_hash,
                    ))?,
                    claimed: false,
                    error: None,
                    continuation_of: Some(turn_id.clone()),
                });
                next.state = RunState::Planning;
            }
            (next, EventKind::ApprovalResolved)
        }
        (Command::ResolveApproval { .. }, _) => return Err(Error::Transition),
        (Command::CancelRun, Some(s)) if planning::editable_snapshot(s) => {
            let mut next = s.clone();
            next.state = RunState::Stopped;
            next.approved_digest = None;
            if let Some(d) = next.delivery.as_mut() {
                d.confirmation_digest = None;
            }
            (next, EventKind::DraftCancelled)
        }
        (Command::CancelRun, Some(s))
            if matches!(
                s.state,
                RunState::Planning
                    | RunState::AwaitingReview
                    | RunState::Reviewing
                    | RunState::AwaitingMerge
                    | RunState::Executing
                    | RunState::AwaitingTurnApproval
                    | RunState::CancelRequested
                    | RunState::Stopping
            ) =>
        {
            let mut next = s.clone();
            next.state = RunState::CancelRequested;
            next.approved_digest = None;
            if let Some(d) = next.delivery.as_mut() {
                d.confirmation_digest = None;
            }
            (next, EventKind::CancelRequested)
        }
        (Command::CancelRun, _) => return Err(Error::Transition),
        _ => return Err(Error::Unsupported),
    };
    next.revision = request
        .expected_revision
        .checked_add(1)
        .filter(|r| *r <= i64::MAX as u64)
        .ok_or(Error::RevisionConflict)?;
    let event = Event {
        schema_version: SCHEMA_VERSION,
        run_id: next.run_id.clone(),
        sequence: next.revision,
        revision: next.revision,
        attempt_id: None,
        turn_id: None,
        kind,
    };
    Ok((next, event))
}
/// Implementations expose commands, not arbitrary snapshot replacement.
pub trait Repository {
    fn submit(
        &mut self,
        request: &Request,
    ) -> impl std::future::Future<Output = Result<Receipt>> + Send;
    fn snapshot(&self, run_id: &str) -> Result<Option<Snapshot>>;
    fn events(&self, run_id: &str, after_sequence: u64, limit: u32) -> Result<Vec<Event>>;
}

pub mod repository;

pub mod planning;

pub mod service;

pub mod turn;

pub mod owned;

pub mod execution;

pub mod artifacts;

mod workflow;

pub mod delivery;

mod recovery;
