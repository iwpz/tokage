//! One task's pre-follow-up context preparation. No dispatch, approvals or model calls.
use crate::*;
use tokage_context::{
    compact::{ProgressContext, RetainedContract, RetentionContext},
    inputs::{InputRequest, Observation, ResolvedInputs},
    ContextActionKind,
};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FollowUp {
    pub source_turn: String,
    pub message: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Prepared {
    pub source_turn: String,
    pub target_turn: String,
    pub source_revision: u64,
    pub config_hash: String,
    pub plan_digest: String,
    pub source_digest: String,
    pub policy: String,
    pub status: String,
    pub preview: serde_json::Value,
    pub prefix: String,
    pub require_new_window: bool,
    pub applies: bool,
    pub bound_at: Option<u64>,
    pub native_confirmed: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub user_accepted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
}
fn invalid(e: impl ToString) -> Error {
    Error::Invalid(e.to_string())
}
/// Same pure resolver as ordinary main-session planning, with durable task/window observations.
pub fn inputs(s: &Snapshot, source_id: &str, task_id: &str, now: u64) -> Result<InputRequest> {
    let source = s.turns.get(source_id).ok_or(Error::Transition)?;
    if source.task_id.as_deref() != Some(task_id) || source.role != "executor" {
        return Err(invalid("Context source belongs to another task or role"));
    }
    let plan = s.plan.as_ref().ok_or(Error::Transition)?;
    let task = plan
        .plan
        .tasks
        .iter()
        .find(|t| t.id == task_id)
        .ok_or(Error::Transition)?;
    if source.observed_model.as_ref().is_some_and(|model| {
        !tokage_matching::assignment::id_matches(model, &source.requested.requested_model)
    }) || source
        .observed_provider
        .as_ref()
        .is_some_and(|p| p != &source.requested.provider)
    {
        return Err(invalid("Observed task context identity changed"));
    }
    let window = source
        .thread_id
        .as_ref()
        .ok_or_else(|| invalid("Context source thread unknown"))?;
    let mut history = s
        .turns
        .values()
        .filter(|t| {
            t.id == source.id
                || source.window_origin.is_some()
                    && t.window_origin == source.window_origin
                    && t.thread_id.as_ref() == Some(window)
        })
        .filter(|t| t.ended && t.cleanup_confirmed && t.requested == source.requested)
        .collect::<Vec<_>>();
    history.sort_by_key(|t| (t.finished_at.unwrap_or(0), t.id == source.id, t.id.clone()));
    let complete = history.len() <= 512
        && history.iter().all(|t| t.finished_at.is_some())
        && source.window_origin.as_ref().is_some_and(|id| {
            s.turns
                .get(id)
                .is_some_and(|t| t.window_origin.as_ref() == Some(id))
        });
    let observations = history
        .iter()
        .skip(history.len().saturating_sub(512))
        .map(|t| Observation {
            id: t.id.clone(),
            window: window.clone(),
            provider: source.requested.provider.clone(),
            model: source.requested.requested_model.clone(),
            ended_at: t.finished_at.unwrap_or(now),
            completed: t.status == turn::TurnStatus::Completed,
            context_tokens: t
                .usage
                .as_ref()
                .filter(|u| {
                    t.context_metrics_version == Some(2)
                        && u.source != tokage_core::UsageSource::Estimated
                })
                .and_then(|u| u.context_tokens)
                .map(|n| n as f64),
            output_tokens: t
                .usage
                .as_ref()
                .filter(|u| {
                    t.requested.provider != "CODEX_CLI"
                        && t.context_metrics_version == Some(2)
                        && u.source != tokage_core::UsageSource::Estimated
                })
                .map(|u| u.output_tokens as f64),
            api_calls: t
                .usage
                .as_ref()
                .filter(|u| {
                    t.context_metrics_version == Some(2)
                        && u.source != tokage_core::UsageSource::Estimated
                })
                .and_then(|u| u.api_calls),
            visible_tokens: (t.visible_prompt_bytes.unwrap_or(0) as f64
                + s.work
                    .as_ref()
                    .and_then(|w| w.attempts.get(&t.id))
                    .and_then(|a| a.report.as_ref())
                    .map(|r| r.to_markdown().len())
                    .unwrap_or(0) as f64)
                / 4.,
        })
        .collect();
    let evidence = s.work.as_ref().and_then(|w| w.attempts.get(source_id));
    let mut pending_errors = vec![];
    if let Some(error) = s
        .execution
        .as_ref()
        .and_then(|e| e.tasks.get(task_id))
        .and_then(|t| t.error.as_ref())
    {
        pending_errors.push(error.chars().take(8000).collect());
    }
    if let Some(error) = &source.error {
        pending_errors.push(error.chars().take(8000).collect());
    }
    let local = serde_json::json!({"sourceTurn":source_id,"task":task_id,"commit":evidence.and_then(|a|a.commit.as_ref()),"gates":evidence.map(|a|a.gates.iter().map(|g|serde_json::json!({"command":g.command,"result":g.result})).collect::<Vec<_>>())});
    let retention = RetentionContext {
        session_id: s.run_id.clone(),
        turn_id: source.id.clone(),
        role: "executor".into(),
        task_id: Some(task_id.into()),
        revision: Some(s.revision),
        task_goal: task.goal.clone(),
        goal_source: "FROZEN_TASK_GOAL".into(),
        contract_paths: task.write_scope.clone(),
        contracts: plan
            .plan
            .contracts
            .iter()
            .filter(|c| task.contracts.contains(&c.id))
            .map(|c| RetainedContract {
                id: c.id.clone(),
                title: c.title.clone(),
                content: c.content.clone(),
            })
            .collect(),
        pending_errors,
        progress: Some(ProgressContext {
            generated_report: None,
            source_turn_ids: vec![source.id.clone()],
            local_report: format!(
                "Retained local evidence (may be truncated): {}",
                local.to_string().chars().take(24000).collect::<String>()
            ),
            model_report: evidence
                .and_then(|a| a.report.as_ref())
                .map(|r| r.to_markdown())
                .filter(|r| r.len() <= 32768),
            changed_files: vec![],
        }),
    };
    Ok(InputRequest {
        session_id: s.run_id.clone(),
        role: "executor".into(),
        task_id: Some(task_id.into()),
        revision: Some(s.revision),
        history_complete: complete,
        retention: Some(retention),
        model_identity: source
            .observed_model
            .as_ref()
            .map(|_| "CLI_OBSERVED".into()),
        provider: source.requested.provider.clone(),
        model: source.requested.requested_model.clone(),
        window: window.clone(),
        observations,
        cache_aware: s
            .config
            .context_options
            .clone()
            .unwrap_or_default()
            .cache_aware,
        min_saving: s
            .config
            .context_options
            .clone()
            .unwrap_or_default()
            .min_saving,
    })
}
pub fn resolve(s: &Snapshot, source_id: &str, task_id: &str, now: u64) -> Result<ResolvedInputs> {
    let request = inputs(s, source_id, task_id, now)?;
    let source = &s.turns[source_id];
    let resets = s
        .turns
        .values()
        .filter(|t| t.window_origin == source.window_origin && source.window_origin.is_some())
        .filter_map(|t| {
            let p = t.context_preparation.as_ref()?;
            (p.applies && p.status == "BOUND").then(|| tokage_context::cooldown::ResetAction {
                action_id: 0,
                attempt_id: p.source_turn.clone(),
                new_attempt_id: Some(t.id.clone()),
                applied_at_ms: p.bound_at.unwrap_or(0).saturating_mul(1000),
            })
        })
        .collect::<Vec<_>>();
    let mut resolved = tokage_context::inputs::resolve_inputs_with_history(
        &planning::effective_pack(&s.config)?,
        &request,
        now,
        &resets,
    )
    .map_err(invalid)?;
    let origin = if s.config.context_options.is_some() {
        "FROZEN_RUN_CONTEXT_OPTIONS"
    } else {
        "LEGACY_CONFIG_DEFAULT"
    };
    resolved.sources.insert("minSaving".into(), origin.into());
    resolved.sources.insert("cacheAware".into(), origin.into());
    Ok(resolved)
}
pub(crate) fn prepare(
    s: &Snapshot,
    r: &execution::Reservation,
    resuming: bool,
    now: u64,
) -> Result<Option<Prepared>> {
    let queued = s
        .execution
        .as_ref()
        .and_then(|e| e.tasks.get(&r.task_id))
        .and_then(|t| t.context_actions.last())
        .is_some_and(|p| p.status == "PREPARED");
    if queued
        && (s.config.context_policy == "off" || r.approval_continuation == Some(true) || !resuming)
    {
        return Err(invalid(
            "Queued context source cannot be resumed; undo and review it again",
        ));
    }
    if s.config.context_policy == "off" || r.approval_continuation == Some(true) || !resuming {
        return Ok(None);
    }
    let source_id = r.resume_turn.as_ref().ok_or(Error::Transition)?;
    let source = s.turns.get(source_id).ok_or(Error::Transition)?;
    if source.task_id.as_deref() != Some(&r.task_id) {
        if queued {
            return Err(invalid("Queued context belongs to another task"));
        }
        return Ok(None);
    } // Cross-task selection stays choose_context's job.
    if source.requested != r.assignment || !source.ended || !source.cleanup_confirmed {
        return Err(invalid("Unsafe task context source"));
    }
    let choice = s
        .execution
        .as_ref()
        .and_then(|e| e.tasks.get(&r.task_id))
        .and_then(|t| t.context_actions.last());
    if let Some(p) = choice.filter(|p| p.status == "PREPARED") {
        if p.source_turn != *source_id
            || p.config_hash != s.config_hash
            || s.approved_digest.as_ref() != Some(&p.plan_digest)
            || p.source_digest != digest(source)?
        {
            return Err(invalid(
                "Queued context action is stale; cancel and review it again",
            ));
        }
    }
    let accepted = choice.is_some_and(|p| p.status == "PREPARED");
    let cancelled = choice.is_some_and(|p| {
        p.status == "CANCELLED"
            && p.source_turn == *source_id
            && p.config_hash == s.config_hash
            && s.approved_digest.as_ref() == Some(&p.plan_digest)
            && digest(source).ok().as_ref() == Some(&p.source_digest)
    });
    let mut resolved = resolve(s, source_id, &r.task_id, now)?;
    let hard = resolved.preview.continue_disqualified;
    let action = matches!(
        resolved.preview.recommended_action,
        ContextActionKind::Compact | ContextActionKind::Fresh
    );
    if resolved.preview.cooldown_blocked
        || hard
            && (!resolved.cooldown.known
                || !action
                || (s.config.context_policy != "auto" && !accepted || cancelled))
    {
        return Err(invalid(
            "Task context hard limit/cooldown requires an explicit fresh window; no follow-up sent",
        ));
    }
    let mut p = Prepared {
        source_turn: source_id.clone(),
        target_turn: r.attempt_id.clone(),
        source_revision: s.revision,
        config_hash: s.config_hash.clone(),
        plan_digest: s.approved_digest.clone().ok_or(Error::Transition)?,
        source_digest: digest(source)?,
        policy: s.config.context_policy.clone(),
        status: "PREPARED".into(),
        preview: serde_json::to_value(&resolved).map_err(invalid)?,
        prefix: String::new(),
        require_new_window: false,
        applies: false,
        bound_at: None,
        native_confirmed: false,
        user_accepted: false,
        failure: None,
    };
    p.user_accepted = accepted;
    if action && (s.config.context_policy == "auto" || accepted) && !cancelled {
        let intent = resolved.compact_intent.as_mut().ok_or_else(|| {
            invalid(
                resolved
                    .compact_unavailable_reason
                    .as_deref()
                    .unwrap_or("Task retention unavailable"),
            )
        })?;
        // Task-window native compact has no independently attributed overhead receipt yet.
        // Preserve the existing prompt fallback; never enable a slash command by provider name.
        tokage_context::compact::select_native_transport(intent, false);
        let expected = intent.clone();
        let draft = tokage_context::compact::prepare_accepted_task_turn(
            &resolved,
            &expected,
            r.followup
                .as_ref()
                .map(|f| f.message.as_str())
                .unwrap_or(""),
        )
        .map_err(invalid)?;
        if hard && !draft.require_new_window {
            return Err(invalid(
                "Unverified same-window compression cannot cross a hard limit",
            ));
        }
        p.prefix = draft.context_prefix;
        p.require_new_window = draft.require_new_window;
        p.applies = true;
    }
    if let Some(queued) = choice.filter(|p| p.status == "PREPARED") {
        if !p.applies
            || queued.require_new_window != p.require_new_window
            || queued
                .preview
                .get("preview")
                .and_then(|v| v.get("recommendedAction"))
                != p.preview
                    .get("preview")
                    .and_then(|v| v.get("recommendedAction"))
        {
            return Err(invalid("Context recommendation changed; review it again"));
        }
    }
    Ok(Some(p))
}
pub(crate) fn bind(s: &Snapshot, p: &Prepared, r: &execution::Reservation) -> Result<()> {
    if s.config_hash != p.config_hash
        || s.approved_digest.as_ref() != Some(&p.plan_digest)
        || p.target_turn != r.attempt_id
        || digest(s.turns.get(&p.source_turn).ok_or(Error::Transition)?)? != p.source_digest
        || s.execution
            .as_ref()
            .and_then(|e| e.tasks.get(&r.task_id))
            .and_then(|t| t.attempts.last())
            != Some(&r.attempt_id)
    {
        return Err(invalid("Stale task context preparation"));
    }
    Ok(())
}

pub(crate) fn preflight(s: &Snapshot, task_id: &str, source_id: &str, message: &str) -> Result<()> {
    if s.config.context_policy == "off" {
        return Ok(());
    }
    let source = s.turns.get(source_id).ok_or(Error::Transition)?;
    let r = execution::Reservation {
        task_id: task_id.into(),
        attempt_id: "preflight-only".into(),
        assignment: source.requested.clone(),
        estimated_usd: None,
        resume_turn: Some(source_id.into()),
        followup: Some(FollowUp {
            source_turn: source_id.into(),
            message: message.into(),
        }),
        approval_continuation: None,
    };
    prepare(s, &r, source.thread_id.is_some(), execution::now()).map(|_| ())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) fn fixture() -> (Snapshot, execution::Reservation) {
        let config:EffectiveRunConfig=serde_json::from_value(serde_json::json!({"source":"unit","policyVersion":"1","workspace":"/repo","targetIdentity":"sha","scope":["src"],"models":{"executor":{"provider":"CODEX_CLI","requestedModel":"gpt-5.6-sol","providerPath":null,"capabilities":[],"userLocked":true}},"commandPolicyHash":"policy","trustWorkspace":true,"permissionMode":"full-auto","concurrency":1,"budgetUsd":null,"blockUnknownCost":false,"timeoutSeconds":30,"repairPolicy":"manual","contextPolicy":"auto","confirmedCommands":[],"customPackHashes":[],"coAuthor":false})).unwrap();
        let req = Request {
            schema_version: 1,
            command_id: "create".into(),
            run_id: "run".into(),
            expected_revision: 0,
            command: Command::CreateRun {
                config,
                requirement: "preserve API".into(),
            },
        };
        let (mut s, _) = transition(None, &req).unwrap();
        s.plan=Some(planning::normalize("# Spec\n## Goals\nGoal\n# Tasks\n```yaml\n- id: T1\n  title: Implement\n  goal: Preserve the goal\n  domain: backend-api\n  write_scope: [src/**]\n  acceptance:\n    commands: [cargo test]\n    criteria: [API unchanged]\n```",&s.config).unwrap());
        s.approved_digest = Some(s.plan.as_ref().unwrap().confirmation_digest.clone());
        s.execution = Some(execution::Execution::new(&s).unwrap());
        s.work = Some(Default::default());
        let model = s.config.models["executor"].clone();
        for (id, at) in [("a", 10), ("b", 20)] {
            let mut t =
                turn::TurnRecord::prepared(id.into(), "executor", Some("T1".into()), model.clone());
            t.window_origin = Some("a".into());
            t.context_metrics_version = Some(2);
            t.thread_id = Some("native-window".into());
            t.ended = true;
            t.cleanup_confirmed = true;
            t.status = turn::TurnStatus::Completed;
            t.finished_at = Some(at);
            t.usage = Some(tokage_core::TurnUsage {
                input_tokens: 10,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                output_tokens: 5,
                api_calls: Some(1),
                context_tokens: Some(10000000),
                cost_usd: None,
                credits: None,
                premium_requests: None,
                source: tokage_core::UsageSource::Structured,
            });
            s.turns.insert(id.into(), t);
        }
        s.execution
            .as_mut()
            .unwrap()
            .tasks
            .get_mut("T1")
            .unwrap()
            .attempts = vec!["a".into(), "b".into(), "target".into()];
        let r = execution::Reservation {
            task_id: "T1".into(),
            attempt_id: "target".into(),
            assignment: model,
            estimated_usd: None,
            resume_turn: Some("b".into()),
            followup: Some(FollowUp {
                source_turn: "b".into(),
                message: " user text\n".into(),
            }),
            approval_continuation: None,
        };
        (s, r)
    }
    #[test]
    fn explicit_suggestion_queue_cancel_and_stale_scope_are_fenced() {
        let (mut s, mut r) = fixture();
        s.state = RunState::Blocked;
        s.config.context_policy = "suggest".into();
        let task = s.execution.as_mut().unwrap().tasks.get_mut("T1").unwrap();
        task.state = tokage_orchestration::TaskState::Blocked;
        task.attempts.pop();
        let approval = s.approved_digest.clone().unwrap();
        let hash = s.config_hash.clone();
        let action = serde_json::to_value(
            resolve(&s, "b", "T1", execution::now())
                .unwrap()
                .preview
                .recommended_action,
        )
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned();
        assert!(queue(&mut s, "T1", "a", &approval, &hash, &action).is_err());
        assert!(queue(&mut s, "T1", "b", &approval, "old-config", &action).is_err());
        assert!(queue(&mut s, "T1", "b", &approval, &hash, "CONTINUE").is_err());
        queue(&mut s, "T1", "b", &approval, &hash, &action).unwrap();
        assert_eq!(s.config.context_policy, "suggest");
        assert!(queue(&mut s, "T1", "b", &approval, &hash, &action).is_err());
        assert!(cancel(&mut s, "T1", "a").is_err());
        assert!(cancel(&mut s, "T2", "b").is_err());
        assert!(prepare(&s, &r, false, execution::now()).is_err());
        let p = prepare(&s, &r, true, execution::now()).unwrap().unwrap();
        assert!(p.user_accepted && p.applies && p.require_new_window);
        let mut changed = s.clone();
        changed.config_hash = "changed".into();
        assert!(prepare(&changed, &r, true, execution::now()).is_err());
        r.resume_turn = Some("a".into());
        assert!(prepare(&s, &r, true, execution::now()).is_err());
        r.resume_turn = Some("b".into());
        cancel(&mut s, "T1", "b").unwrap();
        assert_eq!(
            s.execution.as_ref().unwrap().tasks["T1"].context_actions[0].status,
            "CANCELLED"
        );
        // A hard limit stays blocked after undo, rather than silently sending unchanged context.
        assert!(prepare(&s, &r, true, execution::now()).is_err());
        queue(&mut s, "T1", "b", &approval, &hash, &action).unwrap();
        assert!(
            prepare(&s, &r, true, execution::now())
                .unwrap()
                .unwrap()
                .applies
        );
        cancel_unbound(&mut s);
        assert!(s.execution.as_ref().unwrap().tasks["T1"]
            .context_actions
            .iter()
            .all(|p| p.status == "CANCELLED"));
    }
    #[test]
    fn undo_auto_action_keeps_hard_limit_and_is_scoped_to_config() {
        let (mut s, r) = fixture();
        s.state = RunState::Blocked;
        let task = s.execution.as_mut().unwrap().tasks.get_mut("T1").unwrap();
        task.state = tokage_orchestration::TaskState::Blocked;
        task.attempts.pop();
        let approval = s.approved_digest.clone().unwrap();
        let hash = s.config_hash.clone();
        let action = serde_json::to_value(
            resolve(&s, "b", "T1", execution::now())
                .unwrap()
                .preview
                .recommended_action,
        )
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned();
        queue(&mut s, "T1", "b", &approval, &hash, &action).unwrap();
        cancel(&mut s, "T1", "b").unwrap();
        assert!(prepare(&s, &r, true, execution::now()).is_err());
        s.config_hash = "different-config".into();
        assert!(
            prepare(&s, &r, true, execution::now())
                .unwrap()
                .unwrap()
                .applies
        );
    }
    #[test]
    fn failure_before_binding_is_durable_and_does_not_keep_an_acceptance_alive() {
        let (mut s, r) = fixture();
        s.state = RunState::Blocked;
        s.config.context_policy = "suggest".into();
        let task = s.execution.as_mut().unwrap().tasks.get_mut("T1").unwrap();
        task.state = tokage_orchestration::TaskState::Blocked;
        task.attempts.pop();
        let action = serde_json::to_value(
            resolve(&s, "b", "T1", execution::now())
                .unwrap()
                .preview
                .recommended_action,
        )
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned();
        let approval = s.approved_digest.clone().unwrap();
        let hash = s.config_hash.clone();
        queue(&mut s, "T1", "b", &approval, &hash, &action).unwrap();
        fail_unbound(&mut s, "Context recommendation changed");
        let p = &s.execution.as_ref().unwrap().tasks["T1"].context_actions[0];
        assert_eq!(p.status, "FAILED");
        assert!(p
            .failure
            .as_ref()
            .unwrap()
            .contains("Context recommendation changed"));
        assert!(p.bound_at.is_none() && !p.native_confirmed);
        validate_snapshot(&s).unwrap();
        assert!(prepare(&s, &r, true, execution::now()).is_err());
    }
    #[test]
    fn main_and_task_use_identical_planner_inputs_and_preserve_retention() {
        let (s, r) = fixture();
        let req = inputs(&s, "b", "T1", 30).unwrap();
        let direct = tokage_context::inputs::resolve_inputs(
            &planning::effective_pack(&s.config).unwrap(),
            &req,
            30,
        )
        .unwrap();
        let task = resolve(&s, "b", "T1", 30).unwrap();
        assert_eq!(
            serde_json::to_value(&direct.preview).unwrap(),
            serde_json::to_value(&task.preview).unwrap()
        );
        let prepared = prepare(&s, &r, true, 30).unwrap().unwrap();
        assert!(prepared.applies && prepared.require_new_window);
        assert!(prepared.prefix.contains("Preserve the goal"));
        assert!(!prepared.native_confirmed);
        assert!(bind(&s, &prepared, &r).is_ok());
        let mut changed = s.clone();
        changed.config_hash = "different".into();
        assert!(bind(&changed, &prepared, &r).is_err());
    }
    #[test]
    fn off_approval_and_unknown_history_never_consume_a_new_action() {
        let (mut s, mut r) = fixture();
        s.config.context_policy = "off".into();
        assert!(prepare(&s, &r, true, 30).unwrap().is_none());
        s.config.context_policy = "auto".into();
        r.approval_continuation = Some(true);
        assert!(prepare(&s, &r, true, 30).unwrap().is_none());
        r.approval_continuation = None;
        s.turns.get_mut("b").unwrap().window_origin = None;
        assert!(prepare(&s, &r, true, 30).is_err());
    }
    #[test]
    fn hard_limits_do_not_bypass_cooldown_or_suggest_and_failure_does_not_advance_it() {
        let (mut s, r) = fixture();
        s.turns.get_mut("a").unwrap().status = turn::TurnStatus::Failed;
        assert!(resolve(&s, "b", "T1", 30).unwrap().preview.cooldown_blocked);
        assert!(prepare(&s, &r, true, 30).is_err());
        s.turns.get_mut("a").unwrap().status = turn::TurnStatus::Completed;
        s.config.context_policy = "suggest".into();
        assert!(prepare(&s, &r, true, 30).is_err());
    }
    #[test]
    fn pending_followup_cancel_and_exact_approval_do_not_consume_an_action() {
        let (mut s, _) = fixture();
        s.state = RunState::Blocked;
        let t = s.execution.as_mut().unwrap().tasks.get_mut("T1").unwrap();
        t.state = tokage_orchestration::TaskState::Blocked;
        t.attempts = vec!["a".into(), "b".into()];
        let req = Request {
            schema_version: 1,
            run_id: s.run_id.clone(),
            command_id: "append".into(),
            expected_revision: s.revision,
            command: Command::AppendTaskMessage {
                task_id: "T1".into(),
                confirmation_digest: s.approved_digest.clone().unwrap(),
                message: " exact message\n ".into(),
            },
        };
        let (queued, _) = transition(Some(&s), &req).unwrap();
        assert!(queued.execution.as_ref().unwrap().tasks["T1"]
            .pending_followup
            .is_some());
        assert_eq!(queued.turns.len(), s.turns.len());
        let stop = Request {
            schema_version: 1,
            run_id: queued.run_id.clone(),
            command_id: "cancel".into(),
            expected_revision: queued.revision,
            command: Command::CancelRun,
        };
        let mut blocked_again = queued.clone();
        blocked_again.state = RunState::Blocked;
        let (cancelled_blocked, _) = transition(Some(&blocked_again), &stop).unwrap();
        assert_eq!(cancelled_blocked.state, RunState::Stopped);
        assert!(cancelled_blocked.execution.unwrap().tasks["T1"]
            .pending_followup
            .is_none());
        let (cancelled, _) = transition(Some(&queued), &stop).unwrap();
        assert!(cancelled.execution.unwrap().tasks["T1"]
            .pending_followup
            .is_none());
        assert!(cancelled
            .turns
            .values()
            .all(|t| t.context_preparation.is_none()));
        let mut wrong = req.clone();
        if let Command::AppendTaskMessage {
            confirmation_digest,
            ..
        } = &mut wrong.command
        {
            *confirmation_digest = "stale".into();
        }
        assert!(transition(Some(&s), &wrong).is_err());
        let mut wrong = req;
        if let Command::AppendTaskMessage { task_id, .. } = &mut wrong.command {
            *task_id = "T2".into();
        }
        assert!(transition(Some(&s), &wrong).is_err());
    }
}

#[cfg(test)]
mod command_tests {
    use super::*;
    #[test]
    fn accepted_task_scope_is_strict_even_for_caller_overrides() {
        let scope = tokage_context::compact::CompactScope {
            session_id: "run".into(),
            turn_id: "source".into(),
            window: "window".into(),
            provider_name: "CLAUDE_CODE".into(),
            model: "model".into(),
            role: "executor".into(),
            task_id: Some("T1".into()),
            revision: Some(7),
        };
        let retained = RetentionContext {
            session_id: "run".into(),
            turn_id: "source".into(),
            role: "executor".into(),
            task_id: Some("T1".into()),
            revision: Some(7),
            task_goal: "Keep goal".into(),
            goal_source: "FROZEN_TASK_GOAL".into(),
            contract_paths: vec!["src/**".into()],
            contracts: vec![RetainedContract {
                id: "C1".into(),
                title: "API".into(),
                content: "Preserve signed zero".into(),
            }],
            pending_errors: vec!["Gate failed".into()],
            progress: None,
        };
        let mut intent =
            tokage_context::compact::build_compact_intent(scope.clone(), retained.clone()).unwrap();
        assert!(tokage_context::compact::compose_compact_turn_draft(
            &intent, &scope, &retained, " user\n "
        )
        .is_err());
        tokage_context::compact::select_native_transport(&mut intent, false);
        let draft = tokage_context::compact::compose_compact_turn_draft(
            &intent, &scope, &retained, " user\n ",
        )
        .unwrap();
        assert!(draft.native_command.is_none());
        assert!(!draft.require_new_window);
        assert_eq!(draft.user_message, " user\n ");
        assert!(draft.context_prefix.contains("Preserve signed zero"));
        assert!(draft.context_prefix.contains("Gate failed"));
        let mut foreign = scope;
        foreign.task_id = Some("T2".into());
        assert!(tokage_context::compact::compose_compact_turn_draft(
            &intent, &foreign, &retained, "user"
        )
        .is_err());
    }
}

pub(crate) fn validate_snapshot(s: &Snapshot) -> Result<()> {
    if let Some(e) = &s.execution {
        for (task_id, task) in &e.tasks {
            for p in &task.context_actions {
                if s.turns
                    .get(&p.source_turn)
                    .is_none_or(|t| t.task_id.as_ref() != Some(task_id))
                    || p.source_revision > s.revision
                    || p.native_confirmed
                    || !p.user_accepted
                    || !matches!(
                        p.status.as_str(),
                        "PREPARED" | "BOUND" | "CANCELLED" | "FAILED"
                    )
                    || (p.status == "BOUND") != p.bound_at.is_some()
                {
                    return Err(invalid("Invalid queued context provenance"));
                }
            }
        }
    }

    for t in s.turns.values() {
        if let Some(p) = &t.context_preparation {
            if p.target_turn != t.id
                || p.source_turn == t.id
                || !s.turns.contains_key(&p.source_turn)
                || p.native_confirmed
                || p.applies && p.policy != "auto" && !p.user_accepted
                || p.source_revision > s.revision
                || !matches!(
                    p.status.as_str(),
                    "PREPARED" | "BOUND" | "FAILED" | "CANCELLED"
                )
                || p.status == "PREPARED" && p.bound_at.is_some()
                || p.status == "BOUND" && p.bound_at.is_none()
            {
                return Err(invalid("Invalid task context provenance"));
            }
        }
    }
    Ok(())
}

/// Read the same authoritative task inputs used immediately before sending. No model calls.
pub fn preview(s: &Snapshot, task_id: &str, source_id: &str) -> Result<ResolvedInputs> {
    let t = s
        .execution
        .as_ref()
        .and_then(|e| e.tasks.get(task_id))
        .ok_or(Error::Transition)?;
    if t.attempts.last().map(String::as_str) != Some(source_id) {
        return Err(Error::Transition);
    }
    resolve(s, source_id, task_id, execution::now())
}
pub(crate) fn queue(
    s: &mut Snapshot,
    task_id: &str,
    source_id: &str,
    confirmation: &str,
    config_hash: &str,
    recommended_action: &str,
) -> Result<()> {
    amend::quiescent(s)?;
    if config_hash != s.config_hash {
        return Err(Error::Transition);
    }
    planning::approve(s, confirmation)?;
    if s.state != RunState::Blocked
        || s.approved_digest.as_deref() != Some(confirmation)
        || s.config.context_policy == "off"
    {
        return Err(Error::Transition);
    }
    let t = s
        .execution
        .as_ref()
        .and_then(|e| e.tasks.get(task_id))
        .ok_or(Error::Transition)?;
    if t.state != tokage_orchestration::TaskState::Blocked
        || t.deviation
        || t.pending_followup.is_some()
        || t.attempts.last().map(String::as_str) != Some(source_id)
        || t.context_actions
            .last()
            .is_some_and(|p| p.status == "PREPARED")
    {
        return Err(Error::Transition);
    }
    let source = s.turns.get(source_id).ok_or(Error::Transition)?;
    if source.requested != t.assignment
        || source
            .approval
            .as_ref()
            .is_some_and(|a| a.decision.is_none())
    {
        return Err(Error::Transition);
    }
    let r = execution::Reservation {
        task_id: task_id.into(),
        attempt_id: format!("next:{source_id}"),
        assignment: t.assignment.clone(),
        estimated_usd: None,
        resume_turn: Some(source_id.into()),
        followup: None,
        approval_continuation: None,
    };
    let mut accepted = s.clone();
    accepted.config.context_policy = "auto".into();
    // The user is accepting this action only; the frozen run policy is unchanged.
    accepted
        .execution
        .as_mut()
        .unwrap()
        .tasks
        .get_mut(task_id)
        .unwrap()
        .context_actions
        .clear();
    let mut p = prepare(&accepted, &r, true, execution::now())?.ok_or(Error::Transition)?;
    if !p.applies || p.preview["preview"]["recommendedAction"].as_str() != Some(recommended_action)
    {
        return Err(invalid("Context recommendation changed or unavailable"));
    }
    p.policy = s.config.context_policy.clone();
    p.user_accepted = true;
    s.execution
        .as_mut()
        .unwrap()
        .tasks
        .get_mut(task_id)
        .unwrap()
        .context_actions
        .push(p);
    Ok(())
}
pub(crate) fn cancel(s: &mut Snapshot, task_id: &str, source_id: &str) -> Result<()> {
    amend::quiescent(s)?;
    let p = s
        .execution
        .as_mut()
        .and_then(|e| e.tasks.get_mut(task_id))
        .and_then(|t| t.context_actions.last_mut())
        .ok_or(Error::Transition)?;
    if p.source_turn != source_id || p.status != "PREPARED" {
        return Err(Error::Transition);
    }
    p.status = "CANCELLED".into();
    Ok(())
}

pub(crate) fn cancel_unbound(s: &mut Snapshot) {
    if let Some(e) = s.execution.as_mut() {
        for task in e.tasks.values_mut() {
            for p in &mut task.context_actions {
                if p.status == "PREPARED" {
                    p.status = "CANCELLED".into();
                }
            }
        }
    }
    for turn in s.turns.values_mut() {
        if let Some(p) = turn.context_preparation.as_mut() {
            if p.status == "PREPARED" {
                p.status = "CANCELLED".into();
            }
        }
    }
}

pub(crate) fn fail_unbound(s: &mut Snapshot, reason: &str) {
    if let Some(e) = s.execution.as_mut() {
        for task in e.tasks.values_mut() {
            for p in &mut task.context_actions {
                if p.status == "PREPARED" {
                    p.status = "FAILED".into();
                    p.failure = Some(format!("Run stopped before context binding: {reason}"));
                }
            }
        }
    }
    for t in s.turns.values_mut() {
        if let Some(p) = t.context_preparation.as_mut() {
            if p.status == "PREPARED" {
                p.status = "FAILED".into();
                p.failure = Some(format!("Run stopped before context binding: {reason}"));
            }
        }
    }
}
