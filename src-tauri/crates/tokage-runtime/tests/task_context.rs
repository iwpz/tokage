#![cfg(unix)]
mod common;
#[path = "common/native.rs"]
mod native;
use common::command;
use tokage_runtime::*;
async fn followup_flow(policy: &str, mode: &str, routed: bool) {
    let (dir, cwd, service, _) = native::prepared(mode, "grep -q '^after$' hello.txt", false).await;
    let s = service.snapshot().await.unwrap().unwrap();
    let mut config = s.config.clone();
    config.context_policy = policy.into();
    service
        .submit(&command(&s, "policy", Command::UpdateConfig { config }))
        .await
        .unwrap();
    let s = service.snapshot().await.unwrap().unwrap();
    let digest = s.plan.as_ref().unwrap().confirmation_digest.clone();
    service
        .submit(&command(
            &s,
            "approve-auto",
            Command::ApprovePlan {
                confirmation_digest: digest,
            },
        ))
        .await
        .unwrap();
    let s = service.snapshot().await.unwrap().unwrap();
    service
        .submit(&command(
            &s,
            "start",
            Command::StartExecution {
                confirmation_digest: s.approved_digest.clone().unwrap(),
            },
        ))
        .await
        .unwrap();
    let first = native::settled(&service).await;
    assert_eq!(first.state, RunState::Blocked);
    while service.busy() {
        tokio::task::yield_now().await;
    }
    let first_id = first.execution.as_ref().unwrap().tasks["T1"]
        .attempts
        .last()
        .unwrap()
        .clone();
    let mut last_request = None;
    for i in 0..2 {
        if policy == "suggest" && i == 1 {
            Box::pin(queue_suggestion(&service)).await;
        }
        let s = service.snapshot().await.unwrap().unwrap();
        let message = if routed {
            format!("  T1: repair request {i}\nKeep exact API.  ")
        } else {
            format!("  repair request {i}\nKeep exact API.  ")
        };
        let cmd = if routed {
            let explicit_task_id = (i == 0).then_some("T1".to_owned());
            let preview = task_message::preview(&s, &message, explicit_task_id.as_deref()).unwrap();
            assert!(preview.unavailable_reason.is_none());
            Command::AppendRoutedTaskMessage {
                message,
                explicit_task_id,
                target_task_id: preview.task_id.unwrap(),
                source_turn: preview.source_turn.unwrap(),
                config_hash: preview.config_hash,
                confirmation_digest: preview.plan_digest.unwrap(),
            }
        } else {
            Command::AppendTaskMessage {
                task_id: "T1".into(),
                confirmation_digest: s.approved_digest.clone().unwrap(),
                message,
            }
        };
        let req = command(&s, &format!("append-{i}"), cmd);
        service.submit(&req).await.unwrap();
        let stopped = native::settled(&service).await;
        assert_eq!(stopped.state, RunState::Blocked);
        while service.busy() {
            tokio::task::yield_now().await;
        }
        last_request = Some(req);
    }
    let s = service.snapshot().await.unwrap().unwrap();
    let task = &s.execution.as_ref().unwrap().tasks["T1"];
    assert_eq!(task.attempts.len(), 3);
    let second = &s.turns[&task.attempts[1]];
    let third = &s.turns[&task.attempts[2]];
    assert_eq!(s.turns[&first_id].thread_id, second.thread_id);
    let work = s.work.as_ref().unwrap();
    assert_eq!(
        work.attempts[&first_id].workspace,
        work.attempts[&third.id].workspace
    );
    let calls = std::fs::read_to_string(dir.path().join("calls.jsonl"))
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str::<serde_json::Value>(s).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 3);
    assert!(calls[1]["args"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == "resume"));
    if mode == "context-stale-thread" {
        assert_eq!(second.thread_id, third.thread_id);
        assert_eq!(third.context_preparation.as_ref().unwrap().status, "FAILED");
        assert!(third.window_origin.is_none());
        assert!(task
            .error
            .as_ref()
            .unwrap()
            .contains("Fresh task context window was not confirmed"));
    } else if policy == "auto" || policy == "suggest" {
        assert_ne!(second.thread_id, third.thread_id);
        assert!(!second.context_preparation.as_ref().unwrap().applies);
        let prepared = third.context_preparation.as_ref().unwrap();
        assert!(prepared.applies && prepared.require_new_window);
        assert_eq!(prepared.status, "BOUND");
        assert!(!prepared.native_confirmed);
        if policy == "suggest" {
            assert!(prepared.user_accepted);
            let actions = &task.context_actions;
            assert_eq!(actions.len(), 2);
            assert_eq!(actions[0].status, "CANCELLED");
            assert_eq!(actions[1].status, "BOUND");
            assert_eq!(actions[1].target_turn, third.id);
            assert_eq!(s.config.context_policy, "suggest");
        }
        assert!(prepared.prefix.contains("pendingErrors"));
        assert!(calls[2]["prompt"]
            .as_str()
            .unwrap()
            .contains(&prepared.prefix));
        let wire = calls[2]["prompt"].as_str().unwrap();
        assert!(
            wire.find(&prepared.prefix).unwrap() < wire.find("## Current user follow-up").unwrap()
        );
        assert!(!calls[2]["args"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "resume"));
        assert_eq!(
            task_context::resolve(&s, &third.id, "T1", third.finished_at.unwrap() + 1)
                .unwrap()
                .cooldown
                .turns_since_last_compact,
            0
        );
    } else {
        assert_eq!(second.thread_id, third.thread_id);
        assert!(second.context_preparation.is_none() && third.context_preparation.is_none());
        assert!(calls[2]["args"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "resume"));
    }
    assert!(calls[2]["prompt"].as_str().unwrap().contains(if routed {
        "  T1: repair request 1\nKeep exact API.  "
    } else {
        "  repair request 1\nKeep exact API.  "
    }));
    service.submit(&last_request.unwrap()).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("calls.jsonl"))
            .unwrap()
            .lines()
            .count(),
        3
    );
    assert_eq!(
        std::fs::read_to_string(cwd.join("hello.txt")).unwrap(),
        "before\n"
    );
    let projected = tokage_runtime::repository::Reader::open(&dir.path().join("db"))
        .unwrap()
        .snapshot("run")
        .unwrap()
        .unwrap();
    assert_eq!(
        projected.execution.as_ref().unwrap().tasks["T1"].context_actions,
        task.context_actions
    );
    assert_eq!(
        projected.turns[&third.id].context_preparation,
        third.context_preparation
    );
}

#[tokio::test]
async fn auto_followup_binds_retention_and_switches_native_thread() {
    followup_flow("auto", "context-window", false).await;
}
#[tokio::test]
async fn off_followup_performs_no_context_action() {
    followup_flow("off", "context-window", false).await;
}

#[tokio::test]
async fn a_claimed_fresh_request_that_reuses_the_old_native_thread_is_rejected() {
    followup_flow("auto", "context-stale-thread", false).await;
}

#[tokio::test]
async fn explicit_suggestion_queues_undoes_and_binds_once_through_public_commands() {
    followup_flow("suggest", "context-window", false).await;
}

async fn queue_suggestion(
    service: &tokage_runtime::owned::Runtime<tokage_runtime::service::NativePlanner>,
) {
    let s = service.snapshot().await.unwrap().unwrap();
    let source = s.execution.as_ref().unwrap().tasks["T1"]
        .attempts
        .last()
        .unwrap()
        .clone();
    let inputs = task_context::preview(&s, "T1", &source).unwrap();
    let queue = Command::QueueTaskContext {
        task_id: "T1".into(),
        source_turn: source.clone(),
        confirmation_digest: s.approved_digest.clone().unwrap(),
        config_hash: s.config_hash.clone(),
        recommended_action: serde_json::to_value(inputs.preview.recommended_action)
            .unwrap()
            .as_str()
            .unwrap()
            .into(),
    };
    let first_queue = command(&s, "queue-first", queue.clone());
    service.submit(&first_queue).await.unwrap();
    service.submit(&first_queue).await.unwrap();
    let queued = service.snapshot().await.unwrap().unwrap();
    assert_eq!(queued.state, RunState::Blocked);
    assert_eq!(queued.turns.len(), s.turns.len());
    let action = &queued.execution.as_ref().unwrap().tasks["T1"].context_actions[0];
    assert_eq!(action.status, "PREPARED");
    assert!(action.applies && action.user_accepted);
    service
        .submit(&command(
            &queued,
            "undo-context",
            Command::CancelTaskContext {
                task_id: "T1".into(),
                source_turn: source.clone(),
            },
        ))
        .await
        .unwrap();
    let cancelled = service.snapshot().await.unwrap().unwrap();
    service.submit(&first_queue).await.unwrap();
    assert_eq!(
        service
            .snapshot()
            .await
            .unwrap()
            .unwrap()
            .execution
            .as_ref()
            .unwrap()
            .tasks["T1"]
            .context_actions[0]
            .status,
        "CANCELLED"
    );
    assert!(service
        .submit(&command(
            &cancelled,
            "blocked-without-action",
            Command::AppendTaskMessage {
                task_id: "T1".into(),
                confirmation_digest: cancelled.approved_digest.clone().unwrap(),
                message: "must not send".into()
            }
        ))
        .await
        .is_err());
    service
        .submit(&command(&cancelled, "queue-again", queue))
        .await
        .unwrap();
}

#[tokio::test]
async fn routed_auto_and_explicit_messages_plan_context_in_the_selected_task_window() {
    Box::pin(followup_flow("auto", "context-window", true)).await;
}
#[tokio::test]
async fn routed_suggest_acceptance_is_bound_only_in_the_selected_window() {
    Box::pin(followup_flow("suggest", "context-window", true)).await;
}
