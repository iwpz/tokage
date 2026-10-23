//! Durable native-turn projection. A Done frame is provisional until cleanup and exit.
use crate::*;
use tokage_core::{EventKind as NativeEvent, TurnUsage, UsageSource};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum TurnStatus {
    Prepared,
    Running,
    AwaitingApproval,
    Completed,
    Failed,
    Stopping,
    Stopped,
    Unknown,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Approval {
    pub id: String,
    pub turn_id: String,
    pub command: String,
    pub recoverable: bool,
    pub revision: u64,
    pub confirmation_digest: String,
    pub decision: Option<ApprovalDecision>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TurnRecord {
    pub id: String,
    pub role: String,
    pub task_id: Option<String>,
    pub status: TurnStatus,
    pub requested: ModelAssignment,
    pub observed_model: Option<String>,
    pub observed_provider: Option<String>,
    pub cost_usd: Option<f64>,
    pub reserved_usd: Option<f64>,
    pub thread_id: Option<String>,
    pub last_sequence: u64,
    pub usage: Option<TurnUsage>,
    pub approval: Option<Approval>,
    pub error: Option<String>,
    pub ended: bool,
    pub cleanup_confirmed: bool,
    #[serde(default)]
    pub created_at: Option<u64>,
    pub finished_at: Option<u64>,
}
impl TurnRecord {
    pub fn prepared(
        id: String,
        role: &str,
        task_id: Option<String>,
        requested: ModelAssignment,
    ) -> Self {
        Self {
            id,
            role: role.into(),
            task_id,
            status: TurnStatus::Prepared,
            requested,
            observed_model: None,
            observed_provider: None,
            cost_usd: None,
            reserved_usd: None,
            thread_id: None,
            last_sequence: 0,
            usage: None,
            approval: None,
            error: None,
            ended: false,
            cleanup_confirmed: false,
            created_at: Some(crate::execution::now()),
            finished_at: None,
        }
    }
    pub(crate) fn observe(&mut self, event: &NativeEvent, revision: u64) -> Result<()> {
        if self.ended {
            return Err(Error::Transition);
        }
        self.status = TurnStatus::Running;
        match event {
            NativeEvent::Model { name } => self.observed_model = Some(name.clone()),
            NativeEvent::Thread { id } => self.thread_id = Some(id.clone()),
            NativeEvent::Done { .. } => {
                let usage = event.usage().unwrap();
                if [usage.cost_usd, usage.credits, usage.premium_requests]
                    .iter()
                    .flatten()
                    .any(|n| !n.is_finite() || *n < 0.0)
                {
                    return Err(Error::Invalid("invalid native usage".into()));
                }
                // One receipt per native turn, never a sum of repeated completion frames.
                if usage.source == UsageSource::Structured {
                    if self
                        .usage
                        .as_ref()
                        .is_some_and(|u| u.source == UsageSource::Structured && u != &usage)
                    {
                        self.error = Some("conflicting structured usage receipts".into());
                    } else {
                        self.cost_usd = usage.cost_usd;
                        self.usage = Some(usage);
                    }
                } else if self.usage.is_none() {
                    self.cost_usd = None;
                    self.usage = Some(usage);
                }
            }
            NativeEvent::Error { message } => self.error = Some(message.clone()),
            NativeEvent::ApprovalRequest {
                id,
                command,
                recoverable,
                ..
            } => {
                if self.approval.as_ref().is_some_and(|a| a.id != *id) {
                    return Err(Error::Invalid(
                        "multiple approval windows in one turn".into(),
                    ));
                }
                let mut approval = Approval {
                    id: id.clone(),
                    turn_id: self.id.clone(),
                    command: command.clone(),
                    recoverable: recoverable.unwrap_or(false),
                    revision,
                    confirmation_digest: String::new(),
                    decision: None,
                };
                approval.confirmation_digest =
                    digest(&(&approval.id, &approval.turn_id, &approval.command, revision))?;
                self.approval = Some(approval);
                self.status = TurnStatus::AwaitingApproval;
            }
            NativeEvent::DebugRoute { .. } => {
                return Err(Error::Invalid(
                    "debug routing cannot advance a runtime turn".into(),
                ))
            }
            _ => {}
        }
        Ok(())
    }
    pub(crate) fn finish(
        &mut self,
        cancelled: bool,
        cleanup: bool,
        error: Option<String>,
        now: u64,
    ) -> TurnStatus {
        self.ended = true;
        self.finished_at = Some(now);
        self.cleanup_confirmed = cleanup;
        if error.is_some() {
            self.error = error;
        }
        self.status = if !cleanup {
            TurnStatus::Unknown
        } else if cancelled {
            TurnStatus::Stopped
        } else if self.error.is_some() {
            TurnStatus::Failed
        } else if self.approval.as_ref().is_some_and(|a| a.decision.is_none()) {
            TurnStatus::AwaitingApproval
        } else if self.error.is_some()
            || self
                .usage
                .as_ref()
                .is_none_or(|u| u.source != UsageSource::Structured)
        {
            if self.error.is_none() {
                self.error =
                    Some("native turn ended without structured completion evidence".into());
            }
            TurnStatus::Failed
        } else {
            TurnStatus::Completed
        };
        self.status.clone()
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TranscriptEvent {
    pub turn_id: String,
    pub sequence: u64,
    pub event: NativeEvent,
}
pub(crate) fn validate_snapshot(s: &Snapshot) -> Result<()> {
    for (id, t) in &s.turns {
        if id != &t.id
            || id.is_empty()
            || t.last_sequence > i64::MAX as u64
            || t.status == TurnStatus::Completed
                && (!t.ended
                    || !t.cleanup_confirmed
                    || t.error.is_some()
                    || t.usage
                        .as_ref()
                        .is_none_or(|u| u.source != UsageSource::Structured))
            || t.status == TurnStatus::Stopped && (!t.ended || !t.cleanup_confirmed)
        {
            return Err(Error::Invalid("turn evidence mismatch".into()));
        }
        if let Some(a) = &t.approval {
            if a.turn_id != *id
                || a.confirmation_digest != digest(&(&a.id, &a.turn_id, &a.command, a.revision))?
            {
                return Err(Error::Invalid("approval identity/hash mismatch".into()));
            }
        }
    }
    if s.state == RunState::AwaitingTurnApproval
        && !s.turns.values().any(|t| {
            t.ended
                && t.cleanup_confirmed
                && t.status == TurnStatus::AwaitingApproval
                && t.approval.as_ref().is_some_and(|a| a.decision.is_none())
        })
    {
        return Err(Error::Invalid("missing approval window".into()));
    }
    Ok(())
}
