#![cfg(unix)]
mod common;
#[path = "common/native.rs"]
mod native;
use common::command;
use tokage_runtime::{owned::Runtime, repository::SqliteRepository, service::NativePlanner, *};

#[tokio::test]
async fn lost_native_planner_completion_requires_idle_receipt_before_explicit_replan() {
    for scenario in [
        "valid",
        "missing_launch",
        "missing_cleanup",
        "active_process",
    ] {
        let (dir, _cwd, request) = native::fixture();
        std::fs::write(
            dir.path().join("codex"),
            include_str!("../../../../cli/tests/fixtures/codex.py"),
        )
        .unwrap();
        std::fs::write(
            dir.path().join("plan.md"),
            common::PLAN.replace("src/feature/**", "hello.txt"),
        )
        .unwrap();
        let repo = SqliteRepository::open(&dir.path().join("db"), "run").unwrap();
        let planner = NativePlanner::new(
            tokage_native::env::ShellEnv {
                vars: std::env::vars().collect(),
            },
            &dir.path().join("native"),
        )
        .unwrap();
        let service = Runtime::new(repo, "run", planner).unwrap();
        service.submit(&request).await.unwrap();
        let db = rusqlite::Connection::open(dir.path().join("db/tokage.db")).unwrap();
        db.execute_batch("CREATE TRIGGER reject_planning_completion BEFORE INSERT ON runtime_events WHEN json_extract(NEW.event,'$.kind')='TurnFinished' BEGIN SELECT RAISE(ABORT,'planning completion unavailable'); END;").unwrap();
        let s = service.snapshot().await.unwrap().unwrap();
        service
            .submit(&command(&s, "plan", Command::GeneratePlan))
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while service.worker_error().is_none() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        db.execute_batch("DROP TRIGGER reject_planning_completion")
            .unwrap();
        let s = service.snapshot().await.unwrap().unwrap();
        assert_eq!(s.state, RunState::Planning);
        assert!(s.execution.is_none());
        let turn_id = s.planning_attempt.as_ref().unwrap().id.clone();
        assert!(!s.turns[&turn_id].cleanup_confirmed);
        assert!(service
            .submit(&command(&s, "unsafe-replan", Command::GeneratePlan))
            .await
            .is_err());
        if scenario.starts_with("missing_") {
            let root = dir
                .path()
                .join("native")
                .join(digest(&s.run_id).unwrap())
                .join(&turn_id)
                .join(if scenario == "missing_launch" {
                    "receipts"
                } else {
                    "idle-receipts"
                });
            std::fs::remove_file(tokage_native::recovery::receipt_path(&root, &turn_id)).unwrap();
        }
        if scenario == "active_process" {
            // Liveness-only signal-0 probe of this test process; no termination.
            let registry = dir
                .path()
                .join("native")
                .join(digest(&s.run_id).unwrap())
                .join(&turn_id)
                .join("processes/children.json");
            std::fs::write(registry,serde_json::to_vec(&serde_json::json!([{"pid":std::process::id(),"pgid":std::process::id(),"started_at":0,"command":"offline liveness proof"}])).unwrap()).unwrap();
        }
        let result = service
            .submit(&command(&s, "recover", Command::ResumeRun))
            .await;
        let recovered = service.snapshot().await.unwrap().unwrap();
        assert_eq!(recovered.state, RunState::Blocked);
        assert!(recovered.plan.is_none());
        let calls = || {
            std::fs::read_to_string(dir.path().join("calls.jsonl"))
                .unwrap()
                .lines()
                .count()
        };
        assert_eq!(calls(), 1);
        if scenario != "valid" {
            assert!(result.is_err());
            assert!(!recovered.turns[&turn_id].cleanup_confirmed);
            assert!(service
                .submit(&command(&recovered, "still-unsafe", Command::GeneratePlan))
                .await
                .is_err());
        } else {
            result.unwrap();
            assert!(recovered.turns[&turn_id].ended && recovered.turns[&turn_id].cleanup_confirmed);
            assert_eq!(recovered.turns[&turn_id].status, turn::TurnStatus::Failed);
            service
                .submit(&command(&recovered, "recover-again", Command::ResumeRun))
                .await
                .unwrap();
            assert_eq!(calls(), 1);
            let s = service.snapshot().await.unwrap().unwrap();
            service
                .submit(&command(&s, "explicit-replan", Command::GeneratePlan))
                .await
                .unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(10), async {
                loop {
                    let s = service.snapshot().await.unwrap().unwrap();
                    if s.state == RunState::AwaitingApproval {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            assert_eq!(calls(), 2);
        }
    }
}
