//! Authoritative routing before same-task context planning. Never falls back to another executor.
use crate::*;
use tokage_context::{AppendMessageRoute, TaskSummaryForTriage, TriageMessageParams};
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub revision: u64,
    pub config_hash: String,
    pub plan_digest: Option<String>,
    pub route: AppendMessageRoute,
    pub task_id: Option<String>,
    pub source_turn: Option<String>,
    pub unavailable_reason: Option<String>,
}
pub fn preview(s: &Snapshot, message: &str, explicit: Option<&str>) -> Result<Preview> {
    if message.trim().is_empty() || message.len() > 65536 {
        return Err(Error::Invalid(
            "Message must contain text and fit within 65536 UTF-8 bytes".into(),
        ));
    }
    let tasks = s
        .plan
        .as_ref()
        .ok_or(Error::Transition)?
        .plan
        .tasks
        .iter()
        .map(|t| TaskSummaryForTriage {
            task_id: t.id.clone(),
            title: t.title.clone(),
            write_scope: t.write_scope.clone(),
        })
        .collect::<Vec<_>>();
    if explicit.is_some_and(|id| !tasks.iter().any(|t| t.task_id == id)) {
        return Err(Error::Invalid(
            "Explicit target task no longer exists; choose another target".into(),
        ));
    }
    let base_route = tokage_context::triage_main_session_message(&TriageMessageParams {
        message: message.into(),
        active_tasks: tasks.clone(),
        last_direct_attempt_id: None,
        last_direct_timestamp_ms: None,
        current_time_ms: execution::now().saturating_mul(1000),
        explicit_task_id: explicit.map(str::to_owned),
    });
    let replan = matches!(base_route, AppendMessageRoute::IncrementalReplan { .. });
    let mentioned = tasks
        .iter()
        .filter(|t| tokage_context::triage::mentions_task_id(message, &t.task_id))
        .collect::<Vec<_>>();
    if explicit.is_none() && !replan && mentioned.len() > 1 {
        return Err(Error::Invalid(
            "Message mentions multiple tasks; select one target explicitly".into(),
        ));
    }
    let target = explicit.or_else(|| {
        if replan {
            None
        } else {
            mentioned.first().map(|t| t.task_id.as_str())
        }
    });
    let params = |active_tasks, explicit_task_id| TriageMessageParams {
        message: message.into(),
        active_tasks,
        last_direct_attempt_id: None,
        last_direct_timestamp_ms: None,
        current_time_ms: execution::now().saturating_mul(1000),
        explicit_task_id,
    };
    let route = tokage_context::triage_main_session_message(&params(
        tasks.clone(),
        target.map(str::to_owned),
    ));
    if target.is_none() && matches!(route, AppendMessageRoute::TaskExecutionWindow { .. }) {
        let matches = tasks
            .iter()
            .filter(|t| {
                matches!(
                    tokage_context::triage_main_session_message(&params(vec![(*t).clone()], None)),
                    AppendMessageRoute::TaskExecutionWindow { .. }
                )
            })
            .count();
        if matches > 1 {
            return Err(Error::Invalid(
                "Message matches multiple tasks; select one target explicitly".into(),
            ));
        }
    }
    let task_id = if let AppendMessageRoute::TaskExecutionWindow { task_id, .. } = &route {
        Some(task_id.clone())
    } else {
        None
    };
    let source_turn = task_id.as_ref().and_then(|id| {
        s.execution
            .as_ref()?
            .tasks
            .get(id)?
            .attempts
            .last()
            .cloned()
    });
    let unavailable_reason = task_id
        .as_ref()
        .and_then(|id| eligible(s, id).err().map(|e| e.to_string()));
    Ok(Preview {
        revision: s.revision,
        config_hash: s.config_hash.clone(),
        plan_digest: s.approved_digest.clone(),
        route,
        task_id,
        source_turn,
        unavailable_reason,
    })
}
fn eligible(s: &Snapshot, task_id: &str) -> Result<()> {
    if s.state != RunState::Blocked {
        return Err(Error::Invalid(
            "Task follow-up requires a blocked run; stop or finish active work first".into(),
        ));
    }
    amend::quiescent(s)?;
    if s.turns
        .values()
        .any(|t| t.approval.as_ref().is_some_and(|a| a.decision.is_none()))
    {
        return Err(Error::AwaitingApproval);
    }
    let digest = s
        .approved_digest
        .as_ref()
        .ok_or_else(|| Error::Invalid("Current plan approval required".into()))?;
    planning::approve(s, digest)?;
    let t = s
        .execution
        .as_ref()
        .and_then(|e| e.tasks.get(task_id))
        .ok_or(Error::Transition)?;
    if t.state != tokage_orchestration::TaskState::Blocked
        || t.deviation
        || t.pending_followup.is_some()
    {
        return Err(Error::Invalid(
            "Task is finished, active, deviated or already has a pending follow-up".into(),
        ));
    }
    let source = t
        .attempts
        .last()
        .and_then(|id| s.turns.get(id))
        .ok_or_else(|| Error::Invalid("Task has no execution window yet".into()))?;
    if source.role != "executor"
        || source.task_id.as_deref() != Some(task_id)
        || source.requested != t.assignment
        || source.thread_id.is_none()
        || source
            .approval
            .as_ref()
            .is_some_and(|a| a.decision.is_none())
    {
        return Err(Error::Invalid(
            "Task source, model or pending approval is incompatible".into(),
        ));
    }
    Ok(())
}
pub(crate) fn append(
    s: &Snapshot,
    task_id: &str,
    confirmation_digest: &str,
    message: &str,
) -> Result<Snapshot> {
    eligible(s, task_id)?;
    amend::quiescent(s)?;
    planning::approve(s, confirmation_digest)?;
    if s.approved_digest.as_deref() != Some(confirmation_digest)
        || message.trim().is_empty()
        || message.len() > 65536
    {
        return Err(Error::Invalid("Invalid follow-up or stale approval".into()));
    }
    let mut next = s.clone();
    let t = next
        .execution
        .as_mut()
        .ok_or(Error::Transition)?
        .tasks
        .get_mut(task_id)
        .ok_or(Error::Transition)?;
    if t.state != tokage_orchestration::TaskState::Blocked
        || t.deviation
        || t.pending_followup.is_some()
    {
        return Err(Error::Transition);
    }
    let source = t
        .attempts
        .last()
        .and_then(|id| s.turns.get(id))
        .ok_or(Error::Transition)?;
    if source.requested != t.assignment
        || source.task_id.as_deref() != Some(task_id)
        || source.thread_id.is_none()
        || source
            .approval
            .as_ref()
            .is_some_and(|a| a.decision.is_none())
    {
        return Err(Error::Invalid("Follow-up identity changed".into()));
    }
    task_context::preflight(s, task_id, &source.id, message)?;
    t.continuation_of = None;
    t.pending_followup = Some(task_context::FollowUp {
        source_turn: source.id.clone(),
        message: message.to_owned(),
    });
    t.state = tokage_orchestration::TaskState::Pending;
    next.execution.as_mut().unwrap().claimed = false;
    next.state = RunState::Executing;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Snapshot {
        let (mut s, _) = crate::task_context::tests::fixture();
        s.state = RunState::Blocked;
        set_policy(&mut s, "off");
        let t = s.execution.as_mut().unwrap().tasks.get_mut("T1").unwrap();
        t.state = tokage_orchestration::TaskState::Blocked;
        t.attempts.pop();
        s
    }
    fn set_policy(s: &mut Snapshot, policy: &str) {
        s.config.context_policy = policy.into();
        s.config_hash = digest(&s.config).unwrap();
        let text = s.plan.as_ref().unwrap().plan.raw_markdown.clone();
        s.plan = Some(planning::normalize(&text, &s.config).unwrap());
        s.approved_digest = Some(s.plan.as_ref().unwrap().confirmation_digest.clone());
    }
    fn request(s: &Snapshot, explicit: Option<&str>) -> Request {
        Request {
            schema_version: 1,
            command_id: "routed".into(),
            run_id: s.run_id.clone(),
            expected_revision: s.revision,
            command: Command::AppendRoutedTaskMessage {
                message: "  T1: keep exact text\n ".into(),
                explicit_task_id: explicit.map(str::to_owned),
                target_task_id: "T1".into(),
                source_turn: "b".into(),
                config_hash: s.config_hash.clone(),
                confirmation_digest: s.approved_digest.clone().unwrap(),
            },
        }
    }
    #[test]
    fn routing_is_read_only_explicit_is_strict_and_auto_preserves_plan_and_direct_routes() {
        let s = fixture();
        let before = digest(&s).unwrap();
        assert!(preview(&s, "T1: hello", Some("deleted")).is_err());
        for explicit in [None, Some("T1")] {
            let p = preview(&s, "T1: hello", explicit).unwrap();
            assert_eq!(p.task_id.as_deref(), Some("T1"));
            assert_eq!(p.source_turn.as_deref(), Some("b"));
            assert!(p.unavailable_reason.is_none());
        }
        assert!(matches!(
            preview(&s, "replan T1", None).unwrap().route,
            AppendMessageRoute::IncrementalReplan { .. }
        ));
        assert!(matches!(
            preview(&s, "what is 2+2?", None).unwrap().route,
            AppendMessageRoute::DirectDispatch { .. }
        ));
        assert!(preview(&s, "  ", None).is_err());
        assert!(preview(&s, &"a".repeat(65537), None).is_err());
        assert_eq!(digest(&s).unwrap(), before);
    }
    #[test]
    fn auto_matches_exact_identifiers_and_refuses_ambiguous_titles_or_multiple_tasks() {
        let mut s = fixture();
        let mut other = s.plan.as_ref().unwrap().plan.tasks[0].clone();
        other.id = "T10".into();
        s.plan.as_mut().unwrap().plan.tasks.push(other);
        assert_eq!(
            preview(&s, "请 T10 修改", None).unwrap().task_id.as_deref(),
            Some("T10")
        );
        assert!(preview(&s, "T1 and T10", None).is_err());
        assert!(preview(&s, "Implement please", None).is_err());
        assert_eq!(
            preview(&s, "T10 and T1", Some("T1"))
                .unwrap()
                .task_id
                .as_deref(),
            Some("T1")
        );
    }
    #[test]
    fn routed_send_rechecks_terminal_cancelled_pending_approval_and_current_source() {
        let s = fixture();
        let req = request(&s, None);
        let (next, _) = transition(Some(&s), &req).unwrap();
        assert_eq!(
            next.execution.as_ref().unwrap().tasks["T1"]
                .pending_followup
                .as_ref()
                .unwrap()
                .message,
            "  T1: keep exact text\n "
        );
        assert!(next.turns.values().all(|t| t.context_preparation.is_none()));
        for state in [
            tokage_orchestration::TaskState::Done,
            tokage_orchestration::TaskState::Cancelled,
            tokage_orchestration::TaskState::Running,
        ] {
            let mut changed = s.clone();
            changed
                .execution
                .as_mut()
                .unwrap()
                .tasks
                .get_mut("T1")
                .unwrap()
                .state = state;
            assert!(preview(&changed, "x", Some("T1"))
                .unwrap()
                .unavailable_reason
                .is_some());
            assert!(transition(Some(&changed), &req).is_err());
        }
        let mut changed = s.clone();
        changed.state = RunState::Stopped;
        assert!(transition(Some(&changed), &req).is_err());
        let mut changed = s.clone();
        changed.config_hash = "changed".into();
        assert!(transition(Some(&changed), &req).is_err());
        let mut changed = s.clone();
        changed.approved_digest = None;
        assert!(transition(Some(&changed), &req).is_err());
        let mut changed = s.clone();
        changed
            .execution
            .as_mut()
            .unwrap()
            .tasks
            .get_mut("T1")
            .unwrap()
            .attempts = vec!["a".into()];
        assert!(transition(Some(&changed), &req).is_err());
        let mut changed = s.clone();
        changed.turns.get_mut("b").unwrap().approval = Some(turn::Approval {
            id: "pending".into(),
            turn_id: "b".into(),
            command: "git push".into(),
            recoverable: true,
            revision: 1,
            confirmation_digest: "permit".into(),
            decision: None,
        });
        assert!(transition(Some(&changed), &req).is_err());
        let mut changed = s.clone();
        changed
            .execution
            .as_mut()
            .unwrap()
            .tasks
            .get_mut("T1")
            .unwrap()
            .pending_followup = Some(task_context::FollowUp {
            source_turn: "b".into(),
            message: "pending".into(),
        });
        assert!(transition(Some(&changed), &req).is_err());
        let mut changed = s.clone();
        changed.turns.get_mut("b").unwrap().role = "reviewer".into();
        assert!(transition(Some(&changed), &req).is_err());
        let mut wrong = req.clone();
        if let Command::AppendRoutedTaskMessage { target_task_id, .. } = &mut wrong.command {
            *target_task_id = "T10".into();
        }
        assert!(transition(Some(&s), &wrong).is_err());
    }
    #[test]
    fn context_is_planned_only_after_routing_in_the_chosen_task() {
        let mut s = fixture();
        set_policy(&mut s, "suggest");
        let req = request(&s, Some("T1"));
        let before = digest(&s).unwrap();
        assert!(transition(Some(&s), &req).is_err());
        assert_eq!(digest(&s).unwrap(), before);
        let p = task_context::preview(&s, "T1", "b").unwrap();
        let action = serde_json::to_value(p.preview.recommended_action)
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned();
        task_context::queue(
            &mut s,
            "T1",
            "b",
            &req_confirmation(&req),
            &req_config(&req),
            &action,
        )
        .unwrap();
        assert!(transition(Some(&s), &req).is_ok());
    }
    fn req_confirmation(r: &Request) -> String {
        if let Command::AppendRoutedTaskMessage {
            confirmation_digest,
            ..
        } = &r.command
        {
            confirmation_digest.clone()
        } else {
            unreachable!()
        }
    }
    fn req_config(r: &Request) -> String {
        if let Command::AppendRoutedTaskMessage { config_hash, .. } = &r.command {
            config_hash.clone()
        } else {
            unreachable!()
        }
    }
}
