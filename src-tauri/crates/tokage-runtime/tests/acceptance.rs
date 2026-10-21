#![cfg(unix)]
mod common;
#[path = "common/native.rs"]
mod native;
use common::*;
use native::*;
use std::path::Path;
use tokage_runtime::*;
#[tokio::test]
async fn scope_gate_missing_done_and_deviation_never_accept_or_deliver() {
    for (mode, gate) in [
        ("out-of-scope", "git status --porcelain"),
        ("normal", "false"),
        ("missing-done", "git status --porcelain"),
        ("deviation", "git status --porcelain"),
    ] {
        let (dir, cwd, service, start) = prepared(mode, gate, false).await;
        let head = git(&cwd, &["rev-parse", "HEAD"]);
        service.submit(&start).await.unwrap();
        let s = settled(&service).await;
        assert_eq!(s.state, RunState::Blocked, "mode {mode}");
        assert_ne!(
            s.execution.as_ref().unwrap().tasks["T1"].state,
            tokage_orchestration::TaskState::Done
        );
        assert!(s
            .delivery
            .as_ref()
            .is_none_or(|d| d.confirmation_digest.is_none()));
        assert_eq!(git(&cwd, &["rev-parse", "HEAD"]), head);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("calls.jsonl"))
                .unwrap()
                .lines()
                .count(),
            1
        );
    }
}

#[tokio::test]
async fn commit_and_integration_lost_acknowledgements_reconcile_without_new_model_calls() {
    for operation in ["commit", "integration"] {
        let (dir, _cwd, service, start) = prepared("normal", "git status --porcelain", false).await;
        let conn = rusqlite::Connection::open(dir.path().join("db/tokage.db")).unwrap();
        let trigger = if operation == "commit" {
            "CREATE TRIGGER lose_ack BEFORE UPDATE ON runtime_runs WHEN EXISTS(SELECT 1 FROM json_tree(NEW.snapshot,'$.work.attempts') WHERE key='commit' AND type='text') BEGIN SELECT RAISE(ABORT,'lost task commit acknowledgement'); END;"
        } else {
            "CREATE TRIGGER lose_ack BEFORE UPDATE ON runtime_runs WHEN json_extract(NEW.snapshot,'$.execution.tasks.T1.state')='DONE' BEGIN SELECT RAISE(ABORT,'lost integration acknowledgement'); END;"
        };
        conn.execute_batch(trigger).unwrap();
        service.submit(&start).await.unwrap();
        let failed = settled(&service).await;
        assert_eq!(failed.state, RunState::Blocked);
        conn.execute_batch("DROP TRIGGER lose_ack").unwrap();
        let a = failed
            .work
            .as_ref()
            .unwrap()
            .attempts
            .values()
            .next()
            .unwrap();
        let tree = a.workspace.as_ref().unwrap();
        let task_head = git(Path::new(&tree.cwd), &["rev-parse", "HEAD"]);
        let integration = failed.work.as_ref().unwrap().integration.as_ref().unwrap();
        let integration_head = git(Path::new(&integration.cwd), &["rev-parse", "HEAD"]);
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            loop {
                let s = service.snapshot().await.unwrap().unwrap();
                match service
                    .submit(&command(&s, "resume", Command::ResumeRun))
                    .await
                {
                    Ok(_) => break,
                    Err(Error::Transition) => tokio::task::yield_now().await,
                    Err(e) => panic!("{operation}: {e}"),
                }
            }
        })
        .await
        .unwrap();
        let reconciled = service.snapshot().await.unwrap().unwrap();
        assert_eq!(reconciled.state, RunState::AwaitingReview);
        assert_eq!(
            reconciled.execution.as_ref().unwrap().tasks["T1"].state,
            tokage_orchestration::TaskState::Done
        );
        assert_eq!(git(Path::new(&tree.cwd), &["rev-parse", "HEAD"]), task_head);
        if operation == "integration" {
            assert_eq!(
                git(Path::new(&integration.cwd), &["rev-parse", "HEAD"]),
                integration_head
            );
        }
        assert_eq!(
            std::fs::read_to_string(dir.path().join("calls.jsonl"))
                .unwrap()
                .lines()
                .count(),
            1
        );
    }
}
