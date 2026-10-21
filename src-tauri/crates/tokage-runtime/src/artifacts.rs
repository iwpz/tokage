//! Evidence retained before and after Git side effects; no externally writable DONE flag.
use crate::*;
use tokage_native::git::{scope::ScopeSnapshot, CheckoutIdentity};
use tokage_orchestration::{GateCommandResult, TaskReport, TaskState};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Workspace {
    pub root: String,
    pub cwd: String,
    pub branch: String,
}
impl From<tokage_native::git::Worktree> for Workspace {
    fn from(w: tokage_native::git::Worktree) -> Self {
        Self {
            root: w.root,
            cwd: w.cwd,
            branch: w.branch,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GateEvidence {
    pub id: String,
    pub command: String,
    pub before: ScopeSnapshot,
    pub after: Option<ScopeSnapshot>,
    pub result: Option<GateCommandResult>,
    pub cleanup_confirmed: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttemptEvidence {
    pub error: Option<String>,
    pub native_intent: bool,
    pub workspace: Option<Workspace>,
    pub baseline: Option<ScopeSnapshot>,
    pub accepted_scope: Option<ScopeSnapshot>,
    pub gates: Vec<GateEvidence>,
    pub report: Option<TaskReport>,
    pub commit_intent: bool,
    pub commit: Option<String>,
    pub integration_before: Option<CheckoutIdentity>,
    pub integrated_head: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Work {
    pub target: Option<CheckoutIdentity>,
    pub integration: Option<Workspace>,
    pub integration_head: Option<String>,
    pub integration_intent: bool,
    pub attempts: BTreeMap<String, AttemptEvidence>,
}
pub(crate) fn validate_snapshot(s: &Snapshot) -> Result<()> {
    let Some(ex) = &s.execution else {
        return Ok(());
    };
    let Some(work) = &s.work else {
        if ex.tasks.values().any(|t| t.state == TaskState::Done) {
            return Err(Error::Invalid("DONE without work evidence".into()));
        }
        return Ok(());
    };
    for t in ex.tasks.values().filter(|t| t.state == TaskState::Done) {
        let id = t.attempts.last().ok_or(Error::Transition)?;
        let a = work.attempts.get(id).ok_or(Error::Transition)?;
        if a.commit.is_none()
            || a.integrated_head.is_none()
            || a.accepted_scope.is_none()
            || a.report
                .as_ref()
                .is_none_or(|r| r.is_locally_generated || r.contract_deviations.is_some())
            || a.gates.is_empty()
            || a.gates.iter().any(|g| {
                !g.cleanup_confirmed
                    || g.after.as_ref() != Some(&g.before)
                    || g.result.as_ref().is_none_or(|r| !r.passed)
            })
            || s.turns
                .get(id)
                .is_none_or(|r| r.status != turn::TurnStatus::Completed)
        {
            return Err(Error::Invalid(
                "DONE missing scope/report/gate/commit/integration evidence".into(),
            ));
        }
    }
    Ok(())
}
