#![cfg(unix)]
mod common;
#[path = "common/native.rs"]
mod native;
use common::command;
use tokage_runtime::*;
async fn followup_flow(policy: &str, mode: &str) {
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
        let s = service.snapshot().await.unwrap().unwrap();
        let req = command(
            &s,
            &format!("append-{i}"),
            Command::AppendTaskMessage {
                task_id: "T1".into(),
                confirmation_digest: s.approved_digest.clone().unwrap(),
                message: format!("  repair request {i}\nKeep exact API.  "),
            },
        );
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
    } else if policy == "auto" {
        assert_ne!(second.thread_id, third.thread_id);
        assert!(!second.context_preparation.as_ref().unwrap().applies);
        let prepared = third.context_preparation.as_ref().unwrap();
        assert!(prepared.applies && prepared.require_new_window);
        assert_eq!(prepared.status, "BOUND");
        assert!(!prepared.native_confirmed);
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
    assert!(calls[2]["prompt"]
        .as_str()
        .unwrap()
        .contains("  repair request 1\nKeep exact API.  "));
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
        projected.turns[&third.id].context_preparation,
        third.context_preparation
    );
}

#[tokio::test]
async fn auto_followup_binds_retention_and_switches_native_thread() {
    followup_flow("auto", "context-window").await;
}
#[tokio::test]
async fn off_followup_performs_no_context_action() {
    followup_flow("off", "context-window").await;
}

#[tokio::test]
async fn a_claimed_fresh_request_that_reuses_the_old_native_thread_is_rejected() {
    followup_flow("auto", "context-stale-thread").await;
}
