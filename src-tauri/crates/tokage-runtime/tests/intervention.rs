#![cfg(unix)]
mod common;
use common::command;
#[path = "common/native.rs"]
mod native;
use native::{prepared, settled};
use tokage_runtime::*;
const REPORT:&str="### What was completed\nHuman updated greeting\n### Deviations from contracts\nNone\n### Hints for downstream\nNone\n### Self-check against acceptance criteria\nGreeting checked";
#[tokio::test]
async fn human_takeover_requires_exact_source_and_never_invents_model_success() {
    let (dir, cwd, service, start) =
        prepared("missing-done", "git status --porcelain", false).await;
    service.submit(&start).await.unwrap();
    let failed = settled(&service).await;
    assert_eq!(failed.state, RunState::Blocked);
    let inspection = service.inspect_task("T1").await.unwrap();
    std::fs::write(
        std::path::Path::new(&inspection.workspace.cwd).join("hello.txt"),
        "human changes\n",
    )
    .unwrap();
    service
        .submit(&command(
            &failed,
            "stale-takeover",
            Command::AcceptTakeover {
                task_id: "T1".into(),
                source_digest: inspection.source_digest,
                confirmation_digest: inspection.confirmation_digest,
                report: REPORT.into(),
            },
        ))
        .await
        .unwrap();
    let rejected = settled(&service).await;
    assert_eq!(rejected.state, RunState::Blocked);
    assert!(rejected.execution.as_ref().unwrap().tasks["T1"]
        .error
        .as_ref()
        .unwrap()
        .contains("source changed"));
    let inspection = service.inspect_task("T1").await.unwrap();
    std::fs::write(dir.path().join("mode.txt"), "normal").unwrap();
    service
        .submit(&command(
            &rejected,
            "accept-human",
            Command::AcceptTakeover {
                task_id: "T1".into(),
                source_digest: inspection.source_digest,
                confirmation_digest: inspection.confirmation_digest,
                report: REPORT.into(),
            },
        ))
        .await
        .unwrap();
    let accepted = settled(&service).await;
    assert_eq!(accepted.state, RunState::AwaitingMerge);
    let task = &accepted.execution.as_ref().unwrap().tasks["T1"];
    let aid = task.attempts.last().unwrap();
    let evidence = &accepted.work.as_ref().unwrap().attempts[aid];
    assert!(evidence.manual_receipt.is_some());
    assert!(!accepted.turns.contains_key(aid));
    assert!(evidence
        .gates
        .iter()
        .all(|g| g.result.as_ref().unwrap().passed));
    assert!(evidence.commit.is_some());
    assert!(evidence.integrated_head.is_some());
    assert!(failed
        .turns
        .values()
        .all(|t| t.status != turn::TurnStatus::Completed));
    assert_eq!(
        std::fs::read_to_string(cwd.join("hello.txt")).unwrap(),
        "before\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("calls.jsonl"))
            .unwrap()
            .lines()
            .count(),
        2
    ); // failed executor + independent final reviewer
}
#[tokio::test]
async fn amendments_invalidate_authorization_and_budget_changes_do_not_expand_security_scope() {
    let (_dir, _cwd, service, start) = prepared("nonzero", "git status --porcelain", false).await;
    service.submit(&start).await.unwrap();
    let s = settled(&service).await;
    let sha = s.plan.as_ref().unwrap().confirmation_digest.clone();
    let mut config = s.config.clone();
    config.scope = vec!["**".into()];
    assert!(service
        .submit(&command(
            &s,
            "expand",
            Command::AmendRun {
                confirmation_digest: sha.clone(),
                config,
                text: s.plan.as_ref().unwrap().plan.raw_markdown.clone()
            }
        ))
        .await
        .is_err());
    service
        .submit(&command(
            &s,
            "supplement",
            Command::SupplementTask {
                confirmation_digest: sha.clone(),
                task_id: "T1".into(),
                text: "Preserve greeting punctuation".into(),
            },
        ))
        .await
        .unwrap();
    let changed = service.snapshot().await.unwrap().unwrap();
    assert_eq!(changed.state, RunState::AwaitingApproval);
    assert!(changed.approved_digest.is_none());
    assert_ne!(changed.plan.as_ref().unwrap().confirmation_digest, sha);
    assert!(changed.plan.as_ref().unwrap().plan.tasks[0]
        .goal
        .contains("punctuation"));
    assert_eq!(
        changed.execution.as_ref().unwrap().sequence,
        s.execution.as_ref().unwrap().sequence
    );
    assert_eq!(
        changed.execution.as_ref().unwrap().tasks["T1"].attempts,
        s.execution.as_ref().unwrap().tasks["T1"].attempts
    );
    assert!(service
        .submit(&command(
            &changed,
            "old",
            Command::ApprovePlan {
                confirmation_digest: sha
            }
        ))
        .await
        .is_err());
}
#[tokio::test]
async fn accepted_tasks_stay_immutable_when_a_stopped_run_is_amended() {
    let (_dir, _cwd, service, start) = prepared("normal", "git status --porcelain", false).await;
    service.submit(&start).await.unwrap();
    let accepted = settled(&service).await;
    service
        .submit(&command(&accepted, "stop", Command::CancelRun))
        .await
        .unwrap();
    let stopped = service.snapshot().await.unwrap().unwrap();
    assert_eq!(stopped.state, RunState::Stopped);
    let mut config = stopped.config.clone();
    config.concurrency = 3;
    config.budget_usd = Some(30.0);
    let text = stopped.plan.as_ref().unwrap().plan.raw_markdown.clone();
    let sha = stopped.plan.as_ref().unwrap().confirmation_digest.clone();
    assert!(service
        .submit(&command(
            &stopped,
            "rewrite-done",
            Command::AmendRun {
                confirmation_digest: sha.clone(),
                config: config.clone(),
                text: text.replace("goal: Implement feature", "goal: Different requirement")
            }
        ))
        .await
        .is_err());
    service
        .submit(&command(
            &stopped,
            "policy",
            Command::AmendRun {
                confirmation_digest: sha,
                config,
                text,
            },
        ))
        .await
        .unwrap();
    let changed = service.snapshot().await.unwrap().unwrap();
    assert_eq!(changed.state, RunState::AwaitingApproval);
    assert!(changed.approved_digest.is_none());
    assert_eq!(
        changed.execution.as_ref().unwrap().tasks["T1"],
        accepted.execution.as_ref().unwrap().tasks["T1"]
    );
    assert_eq!(changed.work, accepted.work);
    assert!(service
        .submit(&command(
            &changed,
            "legacy-edit",
            Command::ImportPlan {
                text: changed.plan.as_ref().unwrap().plan.raw_markdown.clone()
            }
        ))
        .await
        .is_err());
}
