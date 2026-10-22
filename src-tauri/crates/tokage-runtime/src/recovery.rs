//! Resume reconciles recorded effects; it never reruns an unknown model or invents gate PASS.
use crate::{
    execution::Reservation,
    owned::Inner,
    service::Planner,
    workflow::{integrate, snapshot, update},
    *,
};
use std::{path::Path, sync::Arc};
use tokage_native::git;
fn invalid(e: impl ToString) -> Error {
    Error::Invalid(e.to_string())
}
pub(crate) async fn recover<P: Planner>(inner: &Arc<Inner<P>>) -> Result<()> {
    let s = snapshot(inner).await?;
    let native = inner.planner.native_backend().ok_or(Error::Unsupported)?;
    let git_root = native
        .directory
        .join("git-processes")
        .join(digest(&s.run_id)?);
    if git_root.exists() {
        for directory in std::fs::read_dir(&git_root).map_err(invalid)? {
            let directory = directory.map_err(invalid)?.path();
            tokage_native::recovery::reconcile_pending_operations(&directory)
                .map_err(|_| Error::CleanupUnknown)?;
            if !tokage_native::recovery::runtime_idle(Some(&directory)).map_err(invalid)? {
                return Err(Error::CleanupUnknown);
            }
        }
    }
    if s.delivery
        .as_ref()
        .and_then(|d| d.intent.as_ref())
        .is_some_and(|i| !i.completed)
    {
        return crate::delivery::deliver(inner.clone()).await;
    }
    if s.execution.is_none() {
        return Ok(());
    }
    for (id, turn) in &s.turns {
        if turn.ended && turn.cleanup_confirmed {
            continue;
        }
        let process_dir = native
            .directory
            .join(digest(&s.run_id)?)
            .join(id)
            .join("processes");
        let cwd = s
            .work
            .as_ref()
            .and_then(|w| w.attempts.get(id))
            .and_then(|a| a.workspace.as_ref())
            .map(|w| w.cwd.clone())
            .or_else(|| s.work.as_ref()?.integration.as_ref().map(|w| w.cwd.clone()))
            .unwrap_or_else(|| s.config.workspace.clone());
        let cwd = Path::new(&cwd).canonicalize().map_err(invalid)?;
        let receipt = native
            .directory
            .join(digest(&s.run_id)?)
            .join(id)
            .join("receipts");
        if process_dir.join("children.json").exists()
            && tokage_native::recovery::runtime_idle(Some(&process_dir)).map_err(invalid)?
            && tokage_native::recovery::verify_launch(&receipt, id, &cwd.to_string_lossy())
                .map_err(invalid)?
        {
            let id = id.clone();
            update(inner, move |s| {
                let turn = s.turns.get_mut(&id).unwrap();
                turn.ended = true;
                turn.cleanup_confirmed = true;
                turn.status = turn::TurnStatus::Failed;
                turn.error = Some("native process idle; lost completion cannot be accepted".into());
                turn.finished_at = Some(execution::now());
                Ok(())
            })
            .await?;
        } else {
            return Err(Error::CleanupUnknown);
        }
    }
    let current = snapshot(inner).await?;
    for (attempt, a) in &current.work.as_ref().ok_or(Error::Transition)?.attempts {
        for gate in a.gates.iter().filter(|g| !g.cleanup_confirmed) {
            let process_dir = native.directory.join("gate-processes").join(&gate.id);
            if !process_dir.join("children.json").exists()
                || !tokage_native::recovery::runtime_idle(Some(&process_dir)).map_err(invalid)?
            {
                return Err(Error::CleanupUnknown);
            }
            let aid = attempt.clone();
            let gid = gate.id.clone();
            update(inner, move |s| {
                s.work
                    .as_mut()
                    .unwrap()
                    .attempts
                    .get_mut(&aid)
                    .unwrap()
                    .gates
                    .iter_mut()
                    .find(|g| g.id == gid)
                    .unwrap()
                    .cleanup_confirmed = true;
                Ok(())
            })
            .await?;
        }
    }
    let current = snapshot(inner).await?;
    if current.approved_digest.as_ref() != current.plan.as_ref().map(|p| &p.confirmation_digest) {
        return Err(invalid("recovery cannot reuse cancelled authorization"));
    }
    let (_, cancel) = tokio::sync::watch::channel(false);
    let mut tasks = current
        .execution
        .as_ref()
        .unwrap()
        .tasks
        .iter()
        .collect::<Vec<_>>();
    tasks.sort_by_key(|(id, t)| {
        let pending = t
            .attempts
            .last()
            .and_then(|aid| current.work.as_ref()?.attempts.get(aid))
            .is_some_and(|a| a.integration_before.is_some() && a.integrated_head.is_none());
        (!pending, *id)
    });
    for (task_id, t) in tasks {
        if t.state == tokage_orchestration::TaskState::Done {
            continue;
        }
        let Some(aid) = t.attempts.last() else {
            continue;
        };
        let a = current
            .work
            .as_ref()
            .unwrap()
            .attempts
            .get(aid)
            .ok_or(Error::Transition)?;
        if !a.commit_intent {
            continue;
        }
        if !crate::manual::completed(&current, aid, a)
            || a.gates.is_empty()
            || a.gates.iter().any(|g| {
                !g.cleanup_confirmed
                    || a.accepted_scope.as_ref() != Some(&g.before)
                    || g.after.as_ref() != Some(&g.before)
                    || g.result.as_ref().is_none_or(|r| !r.passed)
            })
            || a.report
                .as_ref()
                .is_none_or(|r| r.is_locally_generated || r.contract_deviations.is_some())
        {
            return Err(invalid("recovery acceptance evidence incomplete"));
        }
        let tree = a.workspace.as_ref().ok_or(Error::Transition)?;
        let expected = a.accepted_scope.as_ref().ok_or(Error::Transition)?;
        let task = current
            .plan
            .as_ref()
            .unwrap()
            .plan
            .tasks
            .iter()
            .find(|t| t.id == *task_id)
            .unwrap();
        crate::workflow::git_idle(native, Path::new(&tree.cwd)).await?;
        git::scope::commit_verified_with_options(
            &native.env,
            Path::new(&tree.root),
            &format!("feat: {} ({})", task.title, task.id),
            expected,
            current.config.co_author,
        )
        .await
        .map_err(invalid)?;
        let commit = git::rev_parse(&native.env, Path::new(&tree.cwd), "HEAD")
            .await
            .map_err(invalid)?;
        if a.commit.as_ref().is_some_and(|old| old != &commit) {
            return Err(invalid("recovered source commit changed"));
        }
        let aid_copy = aid.clone();
        update(inner, move |s| {
            s.work
                .as_mut()
                .unwrap()
                .attempts
                .get_mut(&aid_copy)
                .unwrap()
                .commit = Some(commit);
            Ok(())
        })
        .await?;
        let reservation = Reservation {
            task_id: task_id.clone(),
            attempt_id: aid.clone(),
            assignment: t.assignment.clone(),
            estimated_usd: None,
            resume_turn: None,
        };
        integrate(inner, &reservation, &cancel).await?;
    }
    update(inner, |s| {
        s.execution.as_mut().unwrap().claimed = false;
        for t in s
            .execution
            .as_mut()
            .unwrap()
            .tasks
            .values_mut()
            .filter(|t| t.state != tokage_orchestration::TaskState::Done)
        {
            t.reservation_active = false;
            t.reserved_usd = None;
            t.state = tokage_orchestration::TaskState::Blocked;
        }
        s.state = if crate::delivery::all_done(s) {
            RunState::AwaitingReview
        } else {
            RunState::Blocked
        };
        Ok(())
    })
    .await
}
