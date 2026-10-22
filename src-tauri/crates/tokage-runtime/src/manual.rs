//! Human takeover is explicit evidence, never a synthetic successful model turn.
use crate::{
    artifacts::*,
    execution::Reservation,
    owned::{Inner, Runtime},
    service::{NativePlanner, Planner},
    workflow, *,
};
use std::{path::Path, sync::Arc};
use tokio::sync::watch;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Input {
    pub command_id: String,
    pub source_attempt: String,
    pub source_digest: String,
    pub plan_digest: String,
    pub report: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Inspection {
    pub workspace: Workspace,
    pub source: tokage_native::git::scope::ScopeSnapshot,
    pub source_digest: String,
    pub confirmation_digest: String,
}
fn source<'a>(s: &'a Snapshot, task_id: &str) -> Result<(&'a String, &'a AttemptEvidence)> {
    let task = s
        .execution
        .as_ref()
        .and_then(|e| e.tasks.get(task_id))
        .ok_or(Error::Transition)?;
    task.attempts
        .iter()
        .rev()
        .find_map(|id| {
            s.work
                .as_ref()?
                .attempts
                .get(id)
                .filter(|a| a.workspace.is_some() && a.baseline.is_some())
                .map(|a| (id, a))
        })
        .ok_or(Error::Transition)
}
impl Runtime<NativePlanner> {
    pub async fn inspect_task(&self, task_id: &str) -> Result<Inspection> {
        let s = self.snapshot().await?.ok_or(Error::Transition)?;
        amend::quiescent(&s)?;
        let (_, a) = source(&s, task_id)?;
        let workspace = a.workspace.clone().ok_or(Error::Transition)?;
        let native = self
            .inner
            .planner
            .native_backend()
            .ok_or(Error::Unsupported)?;
        let root = Path::new(&workspace.root)
            .canonicalize()
            .map_err(|e| Error::Invalid(e.to_string()))?;
        if !root.starts_with(native.directory.join("worktrees")) {
            return Err(Error::Invalid("takeover workspace outside runtime".into()));
        }
        workflow::git_idle(native, Path::new(&workspace.cwd)).await?;
        let source = tokage_native::git::scope::snapshot(&native.env, Path::new(&workspace.cwd))
            .await
            .map_err(Error::Invalid)?;
        Ok(Inspection {
            source_digest: digest(&source)?,
            source,
            workspace,
            confirmation_digest: s
                .plan
                .as_ref()
                .ok_or(Error::Transition)?
                .confirmation_digest
                .clone(),
        })
    }
}
pub(crate) fn prepare(
    s: &Snapshot,
    command_id: &str,
    task_id: &str,
    source_digest: &str,
    confirmation: &str,
    report: &str,
) -> Result<Snapshot> {
    amend::quiescent(s)?;
    planning::approve(s, confirmation)?;
    if report.trim().is_empty() || source_digest.trim().is_empty() {
        return Err(Error::Invalid(
            "takeover requires source digest and human report".into(),
        ));
    }
    let mut next = s.clone();
    let ex = next.execution.as_mut().ok_or(Error::Transition)?;
    let task = ex.tasks.get_mut(task_id).ok_or(Error::Transition)?;
    if task.state == tokage_orchestration::TaskState::Done {
        return Err(Error::Transition);
    }
    let (previous, evidence) = source(s, task_id)?;
    let previous = previous.clone();
    if evidence.workspace.is_none() || evidence.baseline.is_none() {
        return Err(Error::Invalid(
            "takeover requires original workspace and scope baseline".into(),
        ));
    }
    task.manual = Some(Input {
        command_id: command_id.into(),
        source_attempt: previous,
        source_digest: source_digest.into(),
        plan_digest: confirmation.into(),
        report: report.into(),
    });
    task.deviation = false;
    task.error = None;
    task.state = tokage_orchestration::TaskState::Pending;
    ex.claimed = false;
    next.approved_digest = Some(confirmation.into());
    next.delivery = None;
    next.state = RunState::Executing;
    Ok(next)
}
pub(crate) async fn perform<P: Planner + 'static>(
    inner: &Arc<Inner<P>>,
    r: &Reservation,
    cancel: &watch::Receiver<bool>,
    input: Input,
) -> Result<()> {
    let s = workflow::snapshot(inner).await?;
    workflow::active(&s, cancel)?;
    let native = inner.planner.native_backend().ok_or(Error::Unsupported)?;
    let prior = s
        .work
        .as_ref()
        .and_then(|w| w.attempts.get(&input.source_attempt))
        .ok_or(Error::Transition)?;
    let tree = prior.workspace.clone().ok_or(Error::Transition)?;
    let baseline = prior.baseline.clone().ok_or(Error::Transition)?;
    let root = Path::new(&tree.root)
        .canonicalize()
        .map_err(|e| Error::Invalid(e.to_string()))?;
    if !root.starts_with(native.directory.join("worktrees")) {
        return Err(Error::Invalid("manual workspace outside runtime".into()));
    }
    workflow::git_idle(native, Path::new(&tree.cwd)).await?;
    let source = tokage_native::git::scope::snapshot(&native.env, Path::new(&tree.cwd))
        .await
        .map_err(Error::Invalid)?;
    if digest(&source)? != input.source_digest || source.head != baseline.head {
        return Err(Error::Invalid(
            "takeover source changed; inspect and confirm again".into(),
        ));
    }
    let id = r.attempt_id.clone();
    let workspace = tree.clone();
    let before = baseline.clone();
    let receipt = input.clone();
    workflow::update(inner, move |s| {
        let a = s
            .work
            .as_mut()
            .unwrap()
            .attempts
            .get_mut(&id)
            .ok_or(Error::Transition)?;
        a.workspace = Some(workspace);
        a.baseline = Some(before);
        a.manual_receipt = Some(receipt);
        Ok(())
    })
    .await?;
    workflow::accept_task(inner, r, cancel, tree, baseline, input.report, "human").await
}
pub(crate) fn completed(s: &Snapshot, id: &str, a: &AttemptEvidence) -> bool {
    if let Some(receipt) = &a.manual_receipt {
        !receipt.command_id.is_empty()
            && !receipt.plan_digest.is_empty()
            && !receipt.report.trim().is_empty()
            && !a.native_intent
            && !s.turns.contains_key(id)
            && a.gates
                .first()
                .is_some_and(|g| digest(&g.before).ok().as_ref() == Some(&receipt.source_digest))
    } else {
        s.turns
            .get(id)
            .is_some_and(|t| t.status == turn::TurnStatus::Completed)
    }
}
