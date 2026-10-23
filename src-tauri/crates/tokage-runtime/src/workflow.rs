//! Service-owned task effects. Decisions and receipts precede acceptance and integration.
use crate::{
    artifacts::*,
    execution,
    owned::{run_owned, Inner},
    service::{Planner, TurnSpec},
    turn::TurnRecord,
    *,
};
use std::{collections::BTreeSet, path::Path, sync::Arc};
use tokage_core::Role;
use tokage_native::git::{self, scope};
use tokage_orchestration::{self as orchestration, TaskState};
use tokio::{sync::watch, task::JoinSet};
fn invalid(e: impl ToString) -> Error {
    Error::Invalid(e.to_string())
}
pub(crate) async fn snapshot<P: Planner>(inner: &Arc<Inner<P>>) -> Result<Snapshot> {
    inner
        .repo
        .lock()
        .await
        .snapshot(&inner.run_id)?
        .ok_or(Error::Transition)
}
pub(crate) async fn update<P: Planner, T, F>(inner: &Arc<Inner<P>>, change: F) -> Result<T>
where
    F: FnOnce(&mut Snapshot) -> Result<T> + Send,
    T: Send,
{
    let mut repo = inner.repo.lock().await;
    let current = repo.snapshot(&inner.run_id)?.ok_or(Error::Transition)?;
    let mut next = current.clone();
    let result = change(&mut next)?;
    if next.state != RunState::Blocked {
        next.run_error = None;
    }
    repo.advance(current.revision, move |s| {
        *s = next;
        Ok(EventKind::ExecutionProgress)
    })
    .await?;
    Ok(result)
}
pub(crate) fn active(s: &Snapshot, cancel: &watch::Receiver<bool>) -> Result<()> {
    if *cancel.borrow()
        || matches!(
            s.state,
            RunState::CancelRequested | RunState::Stopping | RunState::Stopped
        )
    {
        return Err(Error::Cancelled);
    }
    if s.approved_digest.as_ref() != s.plan.as_ref().map(|p| &p.confirmation_digest) {
        return Err(Error::Invalid("execution approval invalidated".into()));
    }
    Ok(())
}
pub(crate) async fn git_idle(native: &crate::service::NativePlanner, cwd: &Path) -> Result<()> {
    let directory = git::run(
        &native.env,
        cwd,
        &["rev-parse", "--absolute-git-dir"],
        &[],
        10,
    )
    .await
    .map_err(invalid)?;
    for marker in [
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "rebase-merge",
        "rebase-apply",
        "BISECT_LOG",
        "index.lock",
        "HEAD.lock",
    ] {
        if Path::new(directory.trim()).join(marker).exists() {
            return Err(invalid(format!(
                "Git operation already in progress: {marker}"
            )));
        }
    }
    Ok(())
}
pub(crate) async fn preflight<P: Planner>(inner: &Arc<Inner<P>>, s: &Snapshot) -> Result<()> {
    let native = inner.planner.native_backend().ok_or(Error::Unsupported)?;
    if s.config.permission_mode != "full-auto" || !s.config.trust_workspace || cfg!(windows) {
        return Err(Error::Unsupported);
    }
    git_idle(native, Path::new(&s.config.workspace)).await?;
    let target = git::checkout_identity(&native.env, Path::new(&s.config.workspace))
        .await
        .map_err(invalid)?;
    if target.head != s.config.target_identity
        || s.config.target_reference.as_ref() != Some(&target.reference)
        || !git::is_working_tree_clean(&native.env, Path::new(&s.config.workspace))
            .await
            .map_err(invalid)?
    {
        return Err(invalid("execution target changed or dirty"));
    }
    for m in s
        .plan
        .as_ref()
        .ok_or(Error::Transition)?
        .assignments
        .values()
    {
        let provider = serde_json::from_value(serde_json::json!(m.provider)).map_err(invalid)?;
        if !tokage_native::chat::adapter::adapter(provider)
            .is_some_and(|a| a.meta().caps.supports_full_auto())
        {
            return Err(Error::Unsupported);
        }
    }
    Ok(())
}
async fn create_tree<P: Planner>(
    inner: &Arc<Inner<P>>,
    name: &str,
    base: &str,
) -> Result<Workspace> {
    let native = inner.planner.native_backend().ok_or(Error::Unsupported)?;
    let s = snapshot(inner).await?;
    let branch = format!("refs/heads/tokage/{name}");
    // A deterministic Git identity reconciles a worktree creation whose reply was lost.
    let listing = git::run(
        &native.env,
        Path::new(&s.config.workspace),
        &["worktree", "list", "--porcelain"],
        &[],
        10,
    )
    .await
    .map_err(invalid)?;
    for block in listing.split("\n\n") {
        if block.lines().any(|line| line == format!("branch {branch}")) {
            let root = block
                .lines()
                .find_map(|line| line.strip_prefix("worktree "))
                .ok_or_else(|| invalid("worktree identity missing"))?;
            let root_path = Path::new(root).canonicalize().map_err(invalid)?;
            if !root_path.starts_with(native.directory.join("worktrees")) {
                return Err(invalid("worktree outside runtime directory"));
            }
            let prefix = git::run(
                &native.env,
                Path::new(&s.config.workspace),
                &["rev-parse", "--show-prefix"],
                &[],
                10,
            )
            .await
            .map_err(invalid)?;
            return Ok(Workspace {
                root: root.into(),
                cwd: root_path.join(prefix.trim()).to_string_lossy().into(),
                branch: format!("tokage/{name}"),
            });
        }
    }
    git::create_worktree(
        &native.env,
        Path::new(&s.config.workspace),
        &native.directory,
        name,
        Some(base),
    )
    .await
    .map(Into::into)
    .map_err(invalid)
}
async fn ensure_integration<P: Planner>(inner: &Arc<Inner<P>>) -> Result<()> {
    let s = snapshot(inner).await?;
    let native = inner.planner.native_backend().ok_or(Error::Unsupported)?;
    if let Some(w) = s.work.as_ref().and_then(|w| w.integration.as_ref()) {
        if !git::is_working_tree_clean(&native.env, Path::new(&w.cwd))
            .await
            .map_err(invalid)?
            || Some(
                git::rev_parse(&native.env, Path::new(&w.cwd), "HEAD")
                    .await
                    .map_err(invalid)?,
            ) != s.work.as_ref().unwrap().integration_head
        {
            return Err(invalid("integration changed"));
        }
        return Ok(());
    }
    let target = git::checkout_identity(&native.env, Path::new(&s.config.workspace))
        .await
        .map_err(invalid)?;
    if target.head != s.config.target_identity
        || Some(&target.reference) != s.config.target_reference.as_ref()
    {
        return Err(invalid("target identity stale"));
    }
    let target_copy = target.clone();
    update(inner, move |s| {
        let w = s.work.as_mut().ok_or(Error::Transition)?;
        w.target = Some(target_copy);
        w.integration_intent = true;
        Ok(())
    })
    .await?;
    let name = format!("rt-i-{}", &digest(&s.run_id)?[..32]);
    let tree = create_tree(inner, &name, &target.head).await?;
    let baseline = scope::snapshot(&native.env, Path::new(&tree.cwd))
        .await
        .map_err(invalid)?;
    if !baseline.clean || baseline.head != target.head {
        return Err(invalid("integration creation identity mismatch"));
    }
    update(inner, move |s| {
        let w = s.work.as_mut().unwrap();
        w.integration = Some(tree);
        w.integration_head = Some(baseline.head);
        Ok(())
    })
    .await
}
pub(crate) async fn execute<P: Planner + 'static>(
    inner: Arc<Inner<P>>,
    cancel: watch::Receiver<bool>,
) -> Result<()> {
    let result = execute_inner(&inner, &cancel).await;
    if let Err(error) = &result {
        let approval = matches!(error, Error::AwaitingApproval);
        let error = error.to_string();
        let cancelled = matches!(
            snapshot(&inner).await?.state,
            RunState::CancelRequested | RunState::Stopping | RunState::Stopped
        );
        update(&inner, move |s| {
            let uncertain = s.turns.values().any(|t| !t.ended || !t.cleanup_confirmed)
                || s.work.as_ref().is_some_and(|w| {
                    w.attempts
                        .values()
                        .any(|a| a.gates.iter().any(|g| !g.cleanup_confirmed))
                });
            s.state = if cancelled && !uncertain {
                RunState::Stopped
            } else if approval && !uncertain {
                RunState::AwaitingTurnApproval
            } else {
                RunState::Blocked
            };
            s.run_error = Some(error.clone());
            for t in s
                .execution
                .as_mut()
                .ok_or(Error::Transition)?
                .tasks
                .values_mut()
                .filter(|t| t.state != TaskState::Done)
            {
                if t.error.is_none() {
                    t.error = Some(error.clone());
                }
                if cancelled {
                    t.state = TaskState::Cancelled;
                } else if t.state.is_active() {
                    t.state = TaskState::Blocked;
                }
                t.reservation_active = false;
                t.reserved_usd = None;
            }
            Ok(())
        })
        .await?;
    }
    result
}
async fn execute_inner<P: Planner + 'static>(
    inner: &Arc<Inner<P>>,
    cancel: &watch::Receiver<bool>,
) -> Result<()> {
    active(&snapshot(inner).await?, cancel)?;
    ensure_integration(inner).await?;
    let mut jobs = JoinSet::new();
    let mut failure = None;
    loop {
        let s = snapshot(inner).await?;
        if active(&s, cancel).is_err() {
            failure = Some(Error::Cancelled);
        }
        if failure.is_none() {
            let reservations = update(inner, |s| {
                let reservations = execution::reserve_ready(s, execution::now())?;
                for r in &reservations {
                    if s.execution.as_ref().unwrap().tasks[&r.task_id]
                        .manual
                        .is_none()
                    {
                        s.turns.insert(
                            r.attempt_id.clone(),
                            TurnRecord::prepared(
                                r.attempt_id.clone(),
                                "executor",
                                Some(r.task_id.clone()),
                                r.assignment.clone(),
                            ),
                        );
                    }
                    s.work
                        .as_mut()
                        .unwrap()
                        .attempts
                        .insert(r.attempt_id.clone(), AttemptEvidence::default());
                }
                Ok(reservations)
            })
            .await;
            match reservations {
                Ok(reserved) => {
                    for r in reserved {
                        let owned = inner.clone();
                        let cancel = cancel.clone();
                        jobs.spawn(async move {
                            let dir = owned
                                .planner
                                .native_backend()
                                .ok_or(Error::Unsupported)
                                .map(|n| {
                                    n.directory
                                        .join("git-processes")
                                        .join(digest(&owned.run_id).unwrap_or_default())
                                        .join(&r.attempt_id)
                                });
                            let result = match dir {
                                Ok(dir) => {
                                    tokage_native::proc::in_run_directory(
                                        dir,
                                        task(&owned, &r, &cancel),
                                    )
                                    .await
                                }
                                Err(e) => Err(e),
                            };
                            (r, result)
                        });
                    }
                }
                Err(error) => failure = Some(error),
            }
        }
        if jobs.is_empty() {
            let s = snapshot(inner).await?;
            if let Some(error) = failure {
                return Err(error);
            }
            if s.execution
                .as_ref()
                .unwrap()
                .tasks
                .values()
                .all(|t| t.state == TaskState::Done)
            {
                break;
            }
            if s.config.repair_policy == "bounded-ladder" {
                let deadline = s
                    .execution
                    .as_ref()
                    .unwrap()
                    .tasks
                    .values()
                    .filter(|t| {
                        matches!(
                            t.state,
                            TaskState::Pending | TaskState::Ready | TaskState::PausedQuota
                        )
                    })
                    .filter_map(|t| {
                        s.execution
                            .as_ref()
                            .unwrap()
                            .health
                            .get(&t.assignment.provider)?
                            .melted_until
                    })
                    .filter(|until| *until > execution::now())
                    .min();
                if let Some(deadline) = deadline {
                    update(inner, |s| {
                        let ex = s.execution.as_mut().unwrap();
                        for task in ex
                            .tasks
                            .values_mut()
                            .filter(|t| matches!(t.state, TaskState::Pending | TaskState::Ready))
                        {
                            if ex.health.get(&task.assignment.provider).is_some_and(|h| {
                                h.melted_until.is_some_and(|until| until > execution::now())
                            }) {
                                task.state = TaskState::PausedQuota;
                            }
                        }
                        Ok(())
                    })
                    .await?;
                    let mut stop = cancel.clone();
                    tokio::select! {_=tokio::time::sleep(std::time::Duration::from_secs(deadline.saturating_sub(execution::now())))=>{},_=stop.changed()=>return Err(Error::Cancelled)}
                    continue;
                }
            }
            return Err(invalid(
                "DAG blocked by budget, provider health, repair limit or dependencies",
            ));
        }
        let completed = jobs.join_next().await.ok_or(Error::Transition)?;
        let (reservation, result) = match completed {
            Ok(r) => r,
            Err(_) => {
                failure = Some(invalid(
                    "task worker failed; effects require reconciliation",
                ));
                continue;
            }
        };
        match result {
            Ok(()) if failure.is_none() || matches!(failure, Some(Error::AwaitingApproval)) => {
                if let Err(error) = integrate(inner, &reservation, cancel).await {
                    failure = Some(error);
                }
            }
            Ok(()) => {}
            Err(error) => {
                let task_id = reservation.task_id.clone();
                let reason = error.to_string();
                update(inner, move |s| {
                    if let Some(id) = s
                        .execution
                        .as_ref()
                        .and_then(|e| e.tasks.get(&task_id))
                        .and_then(|t| t.attempts.last())
                        .cloned()
                    {
                        if s.work
                            .as_ref()
                            .and_then(|w| w.attempts.get(&id))
                            .is_some_and(|a| !a.native_intent)
                        {
                            if let Some(turn) = s.turns.get_mut(&id) {
                                turn.finish(false, true, Some(reason.clone()), execution::now());
                            }
                        }
                    }
                    if let Some(id) = s
                        .execution
                        .as_ref()
                        .and_then(|e| e.tasks.get(&task_id))
                        .and_then(|t| t.attempts.last())
                        .cloned()
                    {
                        s.work
                            .as_mut()
                            .unwrap()
                            .attempts
                            .get_mut(&id)
                            .unwrap()
                            .error = Some(reason.clone());
                    }
                    execution::fail_task(s, &task_id, reason, execution::now())
                })
                .await?;
                if matches!(
                    error,
                    Error::Cancelled | Error::CleanupUnknown | Error::AwaitingApproval
                ) {
                    failure = Some(error);
                }
            }
        }
    }
    update(inner, |s| {
        let id = format!("automatic-review-{}", s.revision);
        let mut delivery = crate::delivery::new(s, &id)?;
        delivery.claimed = true;
        s.delivery = Some(delivery);
        s.state = RunState::Reviewing;
        Ok(())
    })
    .await?;
    crate::delivery::review(inner.clone(), cancel.clone()).await
}
async fn task<P: Planner + 'static>(
    inner: &Arc<Inner<P>>,
    r: &execution::Reservation,
    cancel: &watch::Receiver<bool>,
) -> Result<()> {
    let native = inner.planner.native_backend().ok_or(Error::Unsupported)?;
    let s = snapshot(inner).await?;
    if let Some(input) = s
        .execution
        .as_ref()
        .and_then(|e| e.tasks.get(&r.task_id))
        .and_then(|t| t.manual.clone())
    {
        return crate::manual::perform(inner, r, cancel, input).await;
    }

    active(&s, cancel)?;
    let p = s.plan.as_ref().ok_or(Error::Transition)?;
    let task = p
        .plan
        .tasks
        .iter()
        .find(|t| t.id == r.task_id)
        .ok_or(Error::Transition)?
        .clone();
    let work = s.work.as_ref().ok_or(Error::Transition)?;
    let base = work.integration_head.as_ref().ok_or(Error::Transition)?;
    let mut resume = None;
    let mut original_baseline = None;
    let reusable = r
        .resume_turn
        .as_ref()
        .and_then(|id| s.turns.get(id))
        .and_then(|turn| {
            work.attempts
                .get(&turn.id)
                .and_then(|a| a.workspace.as_ref())
                .map(|w| (turn, w))
        });
    let tree = if let Some((turn, w)) = reusable {
        let previous = tokage_context::discipline::WindowConfig {
            model: format!(
                "{}:{}",
                turn.requested.provider, turn.requested.requested_model
            ),
            role: turn.role.clone(),
            startup_args: vec![s.config.permission_mode.clone(), digest(&turn.requested)?],
        };
        let current = tokage_context::discipline::WindowConfig {
            model: format!("{}:{}", r.assignment.provider, r.assignment.requested_model),
            role: "executor".into(),
            startup_args: vec![s.config.permission_mode.clone(), digest(&r.assignment)?],
        };
        tokage_context::discipline::validate_window_continuity(&previous, &current)
            .map_err(invalid)?;
        let same_tree = git::rev_parse(&native.env, Path::new(&w.cwd), "HEAD^{tree}")
            .await
            .map_err(invalid)?
            == git::rev_parse(
                &native.env,
                Path::new(&s.config.workspace),
                &format!("{base}^{{tree}}"),
            )
            .await
            .map_err(invalid)?;
        let repairing = turn.task_id.as_deref() == Some(r.task_id.as_str());
        let current_scope = scope::snapshot(&native.env, Path::new(&w.cwd))
            .await
            .map_err(invalid)?;
        let same_scope = turn
            .task_id
            .as_ref()
            .and_then(|id| p.plan.tasks.iter().find(|t| &t.id == id))
            .is_some_and(|t| t.write_scope == task.write_scope);
        if repairing || same_tree && current_scope.clean && same_scope {
            resume = turn.thread_id.clone();
            if repairing {
                original_baseline = work.attempts.get(&turn.id).and_then(|a| a.baseline.clone());
            }
            w.clone()
        } else {
            create_tree(inner, &format!("rt-t-{}", &r.attempt_id[..32]), base).await?
        }
    } else {
        create_tree(inner, &format!("rt-t-{}", &r.attempt_id[..32]), base).await?
    };
    let current_scope = scope::snapshot(&native.env, Path::new(&tree.cwd))
        .await
        .map_err(invalid)?;
    let before = if let Some(original) = original_baseline {
        if original.head != current_scope.head {
            return Err(invalid("repair source HEAD changed"));
        }
        original
    } else {
        if !current_scope.clean {
            return Err(invalid("task baseline is dirty"));
        }
        current_scope
    };
    let reuse_rejected = r.resume_turn.is_some() && resume.is_none();
    let task_code = r.task_id.clone();
    let id = r.attempt_id.clone();
    let tree_copy = tree.clone();
    let before_copy = before.clone();
    update(inner, move |s| {
        if reuse_rejected {
            if let Some(c) = s
                .execution
                .as_mut()
                .unwrap()
                .tasks
                .get_mut(&task_code)
                .unwrap()
                .context
                .as_mut()
            {
                c.reuse = false;
                c.reason = "workspace tree, cleanliness or startup scope changed".into();
            }
        }
        let a = s.work.as_mut().unwrap().attempts.get_mut(&id).unwrap();
        a.workspace = Some(tree_copy);
        a.baseline = Some(before_copy);
        Ok(())
    })
    .await?;
    let upstream = task
        .depends_on
        .iter()
        .filter_map(|id| {
            s.execution
                .as_ref()?
                .tasks
                .get(id)?
                .attempts
                .last()
                .and_then(|aid| s.work.as_ref()?.attempts.get(aid)?.report.as_ref())
                .map(|report| orchestration::UpstreamArtifact {
                    task_id: id.clone(),
                    title: id.clone(),
                    report: report.to_markdown(),
                    changed_files: report.changed_files_list.clone(),
                })
        })
        .collect();
    let brief = orchestration::assemble_task_brief(&orchestration::TaskBriefInput {
        task: task.clone(),
        contracts: p.plan.contracts.clone(),
        upstream_artifacts: upstream,
        failure_evidence: s.execution.as_ref().unwrap().tasks[&r.task_id]
            .error
            .clone(),
    });
    tokage_context::discipline::validate_task_brief_header(&brief).map_err(invalid)?;
    let prompt=format!("{brief}\n\nDo not commit, change Git metadata, edit ignored or unrelated files. Tokage performs acceptance and commits. Original requirement:\n{}",s.requirement);
    let spec = TurnSpec {
        role: Role::Executor,
        cwd: tree.cwd.clone().into(),
        prompt,
        selected: r.assignment.clone(),
        write_scope: task.write_scope.clone(),
        resume,
        allowed: s.execution.as_ref().unwrap().tasks[&r.task_id]
            .session_allowed
            .clone(),
    };
    let aid = r.attempt_id.clone();
    update(inner, move |s| {
        s.work
            .as_mut()
            .unwrap()
            .attempts
            .get_mut(&aid)
            .unwrap()
            .native_intent = true;
        Ok(())
    })
    .await?;
    let text = run_owned(
        inner,
        snapshot(inner).await?,
        r.attempt_id.clone(),
        cancel.clone(),
        Some(spec),
    )
    .await?;
    active(&snapshot(inner).await?, cancel)?;
    let changes = git::rollback::changes(&native.env, Path::new(&tree.cwd), &r.attempt_id)
        .await
        .map_err(invalid)?;
    if !orchestration::validate_write_scope(&task.write_scope, &relative(&before, &changes)?).passed
    {
        return Err(invalid("task reported out-of-scope changes"));
    }
    let after = scope::snapshot(&native.env, Path::new(&tree.cwd))
        .await
        .map_err(invalid)?;
    if after.head != before.head {
        return Err(invalid("executor changed Git HEAD"));
    }
    accept_task(
        inner,
        r,
        cancel,
        tree,
        before,
        text,
        &r.assignment.requested_model,
    )
    .await
}
pub(crate) async fn accept_task<P: Planner + 'static>(
    inner: &Arc<Inner<P>>,
    r: &execution::Reservation,
    cancel: &watch::Receiver<bool>,
    tree: Workspace,
    before: scope::ScopeSnapshot,
    text: String,
    actor: &str,
) -> Result<()> {
    let native = inner.planner.native_backend().ok_or(Error::Unsupported)?;
    let s = snapshot(inner).await?;
    active(&s, cancel)?;
    let task = s
        .plan
        .as_ref()
        .ok_or(Error::Transition)?
        .plan
        .tasks
        .iter()
        .find(|t| t.id == r.task_id)
        .ok_or(Error::Transition)?
        .clone();
    let id = r.attempt_id.clone();
    let task_id = task.id.clone();
    update(inner, move |s| {
        s.execution
            .as_mut()
            .unwrap()
            .tasks
            .get_mut(&task_id)
            .unwrap()
            .state = TaskState::Verifying;
        Ok(())
    })
    .await?;
    gates(inner, &id, &tree, &task.acceptance.commands, cancel).await?;
    let accepted = scope::snapshot(&native.env, Path::new(&tree.cwd))
        .await
        .map_err(invalid)?;
    if accepted.head != before.head {
        return Err(invalid("gate changed Git HEAD"));
    }
    let proof = snapshot(inner).await?;
    if proof
        .work
        .as_ref()
        .and_then(|w| w.attempts.get(&r.attempt_id))
        .is_none_or(|a| {
            a.gates.is_empty()
                || a.gates
                    .iter()
                    .any(|g| g.before != accepted || g.after.as_ref() != Some(&accepted))
        })
    {
        return Err(invalid("accepted source changed since gate verification"));
    }

    let files = relative(&before, &changed(&before, &accepted))?;
    let app_data = inner
        .repo
        .lock()
        .await
        .directory()
        .to_string_lossy()
        .into_owned();
    {
        let pack = planning::effective_pack(&s.config)?;
        let mut policy: tokage_core::command_policy::PackPolicy = serde_json::from_value(
            tokage_matching::pack::builtin_knowledge_pack()
                .command_policy
                .clone()
                .ok_or_else(|| invalid("command policy missing"))?,
        )
        .map_err(invalid)?;
        policy.forbidden_paths.extend(pack.forbidden_paths);
        let mut context = tokage_core::command_policy::Context::new(tree.cwd.clone());
        context.workspace_roots = vec![tree.cwd.clone()];
        context.home = native.env.vars.get("HOME").cloned();
        context.app_data = Some(app_data);
        context.write_scope = Some(task.write_scope.clone());
        for file in &files {
            if tokage_core::command_policy::Policy::new(&policy, &[])
                .classify_edit(file, &context)
                .level
                == tokage_core::command_policy::Level::Forbidden
            {
                return Err(invalid("accepted edit forbidden by effective policy"));
            }
        }
    }
    if !orchestration::validate_write_scope(&task.write_scope, &files).passed {
        return Err(invalid("accepted files outside task scope"));
    }
    let report = orchestration::parse_or_generate_task_report(
        &task,
        &text,
        actor,
        &r.attempt_id,
        s.execution.as_ref().unwrap().tasks[&r.task_id]
            .attempts
            .len()
            == 1,
        "",
        files,
        "Independent acceptance gates passed",
        snapshot(inner)
            .await?
            .turns
            .get(&r.attempt_id)
            .and_then(|t| t.cost_usd),
    );
    if report.is_locally_generated {
        return Err(invalid("model report missing required sections"));
    }
    if report.contract_deviations.is_some() || report.defects.is_some() {
        let id = r.task_id.clone();
        update(inner, move |s| {
            s.execution
                .as_mut()
                .unwrap()
                .tasks
                .get_mut(&id)
                .unwrap()
                .deviation = true;
            Ok(())
        })
        .await?;
        return Err(invalid("contract deviation requires user review"));
    }
    let aid = r.attempt_id.clone();
    let accepted_copy = accepted.clone();
    update(inner, move |s| {
        let a = s.work.as_mut().unwrap().attempts.get_mut(&aid).unwrap();
        a.accepted_scope = Some(accepted_copy);
        a.report = Some(report);
        a.commit_intent = true;
        Ok(())
    })
    .await?;
    active(&snapshot(inner).await?, cancel)?;
    git_idle(native, Path::new(&tree.cwd)).await?;
    if git::checkout_identity(&native.env, Path::new(&tree.cwd))
        .await
        .map_err(invalid)?
        .reference
        != format!("refs/heads/{}", tree.branch)
    {
        return Err(invalid("task checkout branch changed"));
    }
    scope::commit_verified_with_options(
        &native.env,
        Path::new(&tree.root),
        &format!("feat: {} ({})", task.title, task.id),
        &accepted,
        s.config.co_author,
    )
    .await
    .map_err(invalid)?;
    let commit = git::rev_parse(&native.env, Path::new(&tree.cwd), "HEAD")
        .await
        .map_err(invalid)?;
    let aid = r.attempt_id.clone();
    update(inner, move |s| {
        s.work
            .as_mut()
            .unwrap()
            .attempts
            .get_mut(&aid)
            .unwrap()
            .commit = Some(commit);
        Ok(())
    })
    .await
}
fn relative(snapshot: &scope::ScopeSnapshot, files: &[String]) -> Result<Vec<String>> {
    if snapshot.prefix.is_empty() {
        return Ok(files.to_vec());
    }
    let prefix = format!("{}/", snapshot.prefix.trim_end_matches('/'));
    files
        .iter()
        .map(|p| {
            p.strip_prefix(&prefix)
                .map(str::to_owned)
                .ok_or_else(|| invalid("change outside project subdirectory"))
        })
        .collect()
}
pub(crate) fn changed(before: &scope::ScopeSnapshot, after: &scope::ScopeSnapshot) -> Vec<String> {
    before
        .files
        .keys()
        .chain(after.files.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|p| before.files.get(*p) != after.files.get(*p))
        .cloned()
        .collect()
}
pub(crate) async fn gates<P: Planner>(
    inner: &Arc<Inner<P>>,
    attempt: &str,
    tree: &Workspace,
    commands: &[String],
    cancel: &watch::Receiver<bool>,
) -> Result<()> {
    let native = inner.planner.native_backend().ok_or(Error::Unsupported)?;
    let s = snapshot(inner).await?;
    let pack = planning::effective_pack(&s.config)?;
    for (index, command) in commands.iter().enumerate() {
        active(&snapshot(inner).await?, cancel)?;
        planning::guard_approval_command(&s.config, command)?;
        let before = scope::snapshot(&native.env, Path::new(&tree.cwd))
            .await
            .map_err(invalid)?;
        if s.work
            .as_ref()
            .and_then(|w| w.attempts.get(attempt))
            .and_then(|a| a.manual_receipt.as_ref())
            .is_some_and(|input| digest(&before).ok().as_ref() != Some(&input.source_digest))
        {
            return Err(invalid("takeover source changed before gate launch"));
        }

        let id = digest(&(attempt, index, command))?;
        let aid = attempt.to_owned();
        let gid = id.clone();
        let cmd = command.clone();
        let baseline = before.clone();
        update(inner, move |s| {
            s.work
                .as_mut()
                .unwrap()
                .attempts
                .get_mut(&aid)
                .ok_or(Error::Transition)?
                .gates
                .push(GateEvidence {
                    id: gid,
                    command: cmd,
                    before: baseline,
                    after: None,
                    result: None,
                    cleanup_confirmed: false,
                });
            Ok(())
        })
        .await?;
        let process_dir = native.directory.join("gate-processes").join(&id);
        tokage_native::recovery::claim_launch(
            &native.directory.join("gate-receipts"),
            &id,
            &serde_json::json!({"turnId":id,"cwd":tree.cwd}),
        )
        .map_err(invalid)?;
        let request = orchestration::GateRequest {
            command,
            workdir: Path::new(&tree.cwd),
            env: &native.env,
            timeout_secs: Some(s.config.timeout_seconds),
            confirmed: s.config.confirmed_commands.contains(command),
            allowlist: &pack.gate_allowlist,
            env_allow: &s.config.gate_env_allow,
            run_id: Some(&id),
        };
        let future = orchestration::gate::execute_gate_command_scoped(request, Some(&process_dir));
        tokio::pin!(future);
        let mut stop = cancel.clone();
        let mut cancelled = false;
        if *stop.borrow() {
            return Err(Error::Cancelled);
        }
        let result = loop {
            tokio::select! {biased;result=&mut future=>break result,_=stop.changed()=>{cancelled=true;orchestration::cancel_gate_command(&id);}}
        };
        let cleanup = tokage_native::recovery::runtime_idle(Some(&process_dir))
            .map_err(|_| Error::CleanupUnknown)?;
        let after = scope::snapshot(&native.env, Path::new(&tree.cwd))
            .await
            .map_err(invalid)?;
        let unchanged = after == before;
        let aid = attempt.to_owned();
        let gid = id.clone();
        let copy = result.clone();
        update(inner, move |s| {
            let gate = s
                .work
                .as_mut()
                .unwrap()
                .attempts
                .get_mut(&aid)
                .unwrap()
                .gates
                .iter_mut()
                .find(|g| g.id == gid)
                .ok_or(Error::Transition)?;
            gate.result = Some(copy);
            gate.after = Some(after);
            gate.cleanup_confirmed = cleanup;
            Ok(())
        })
        .await?;
        if !cleanup {
            return Err(Error::CleanupUnknown);
        }
        if cancelled || *cancel.borrow() {
            return Err(Error::Cancelled);
        }
        if !unchanged {
            return Err(invalid("gate changed candidate source"));
        }
        if !result.passed {
            return Err(invalid(format!(
                "gate failed: {command}: {}",
                result.error_message.unwrap_or(result.output_tail)
            )));
        }
    }
    Ok(())
}
pub(crate) async fn integrate<P: Planner>(
    inner: &Arc<Inner<P>>,
    r: &execution::Reservation,
    cancel: &watch::Receiver<bool>,
) -> Result<()> {
    let native = inner.planner.native_backend().ok_or(Error::Unsupported)?;
    let s = snapshot(inner).await?;
    active(&s, cancel)?;
    let w = s.work.as_ref().ok_or(Error::Transition)?;
    let integration = w.integration.as_ref().ok_or(Error::Transition)?;
    let a = w.attempts.get(&r.attempt_id).ok_or(Error::Transition)?;
    let source = a.commit.as_ref().ok_or(Error::Transition)?;
    let tree = a.workspace.as_ref().ok_or(Error::Transition)?;
    let current_source = scope::snapshot(&native.env, Path::new(&tree.cwd))
        .await
        .map_err(invalid)?;
    let accepted = a.accepted_scope.as_ref().ok_or(Error::Transition)?;
    let disk = |s: &scope::ScopeSnapshot| {
        s.files
            .iter()
            .map(|(p, f)| (p.clone(), f.disk.clone()))
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    if !current_source.clean
        || current_source.head != *source
        || disk(&current_source) != disk(accepted)
    {
        return Err(invalid("task source changed after acceptance"));
    }
    git_idle(native, Path::new(&integration.cwd)).await?;
    let before = git::checkout_identity(&native.env, Path::new(&integration.cwd))
        .await
        .map_err(invalid)?;
    if a.integration_before.is_none() {
        if Some(&before.head) != w.integration_head.as_ref() {
            return Err(invalid("integration advanced outside service"));
        }
        let aid = r.attempt_id.clone();
        let expected = before.clone();
        update(inner, move |s| {
            s.work
                .as_mut()
                .unwrap()
                .attempts
                .get_mut(&aid)
                .unwrap()
                .integration_before = Some(expected);
            Ok(())
        })
        .await?;
    }
    let before = a.integration_before.as_ref().unwrap_or(&before);
    if !git::reconcile_delivery_merge(&native.env, Path::new(&integration.cwd), source, before)
        .await
        .map_err(invalid)?
    {
        let task = s
            .plan
            .as_ref()
            .unwrap()
            .plan
            .tasks
            .iter()
            .find(|t| t.id == r.task_id)
            .unwrap();
        match orchestration::merge_task_worktree_with_options(
            &native.env,
            Path::new(&integration.cwd),
            &integration.branch,
            &tree.branch,
            task,
            Some(source),
            s.config.co_author,
        )
        .await
        .map_err(invalid)?
        {
            orchestration::WorktreeMergeOutcome::Success { .. } => {}
            orchestration::WorktreeMergeOutcome::Conflict { diagnostic, .. } => {
                return Err(invalid(format!(
                    "integration conflict preserved: {diagnostic}"
                )))
            }
        }
    }
    let head = git::rev_parse(&native.env, Path::new(&integration.cwd), "HEAD")
        .await
        .map_err(invalid)?;
    let aid = r.attempt_id.clone();
    let tid = r.task_id.clone();
    update(inner, move |s| {
        let w = s.work.as_mut().unwrap();
        w.attempts.get_mut(&aid).unwrap().integrated_head = Some(head.clone());
        w.integration_head = Some(head);
        let task = s.execution.as_mut().unwrap().tasks.get_mut(&tid).unwrap();
        task.state = TaskState::Done;
        task.reservation_active = false;
        task.reserved_usd = None;
        Ok(())
    })
    .await
}
