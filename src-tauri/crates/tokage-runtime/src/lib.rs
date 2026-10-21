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
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EffectiveRunConfig {
    pub source: String,
    pub policy_version: String,
    pub workspace: String,
    pub target_identity: String,
    pub scope: Vec<String>,
    pub models: BTreeMap<String, ModelAssignment>,
    pub command_policy_hash: String,
    pub trust_workspace: bool,
    pub permission_mode: String,
    pub concurrency: u32,
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
        planning::effective_pack(self)?;
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum RunState {
    Draft,
    Planning,
    AwaitingApproval,
    Approved,
    Executing,
    CancelRequested,
    Stopping,
    Stopped,
    Blocked,
    AwaitingMerge,
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
}
impl Snapshot {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(Error::Schema);
        }
        self.config.validate()?;
        if self.revision == 0
            || self.run_id.trim().is_empty()
            || self.requirement.trim().is_empty()
            || self.config_hash != digest(&self.config)?
        {
            return Err(Error::Invalid("snapshot identity/config hash".into()));
        }
        planning::validate_snapshot(self)?;
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
                },
                EventKind::RunCreated,
            )
        }
        (Command::CreateRun { .. }, Some(_)) => return Err(Error::Transition),
        (Command::GeneratePlan, Some(s)) if planning::editable(s.state) => {
            if !s.config.trust_workspace {
                return Err(Error::Invalid(
                    "planning requires explicit workspace trust".into(),
                ));
            }
            let mut next = s.clone();
            next.plan = None;
            next.approved_digest = None;
            next.planning_attempt = Some(planning::PlanningAttempt {
                id: digest(&(request.run_id.as_str(), request.command_id.as_str()))?,
                claimed: false,
                error: None,
            });
            next.state = RunState::Planning;
            (next, EventKind::PlanningRequested)
        }
        (Command::GeneratePlan, _) => return Err(Error::Transition),
        (Command::ImportPlan { text }, Some(s)) if planning::editable(s.state) => {
            let mut next = s.clone();
            next.plan = Some(planning::normalize(text, &s.config)?);
            next.planning_attempt = None;
            next.approved_digest = None;
            next.state = RunState::AwaitingApproval;
            (next, EventKind::PlanImported)
        }
        (Command::UpdateConfig { config }, Some(s)) if planning::editable(s.state) => {
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
        (Command::CancelRun, Some(s)) if planning::editable(s.state) => {
            let mut next = s.clone();
            next.state = RunState::Stopped;
            next.approved_digest = None;
            (next, EventKind::DraftCancelled)
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
