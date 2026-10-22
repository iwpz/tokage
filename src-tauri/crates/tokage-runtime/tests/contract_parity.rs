use serde_json::{json, Value};
use tokage_runtime::{host, repository::Reader, *};
fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../../../tests/fixtures/shared-runtime-v1.json"
    ))
    .unwrap()
}
#[test]
fn app_and_cli_wire_fixture_is_exactly_the_public_rust_contract() {
    for case in fixture()["commands"].as_array().unwrap() {
        let decoded: Command = serde_json::from_value(case["wire"].clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), case["wire"]);
    }
}
#[tokio::test]
async fn repeated_service_runs_have_the_declared_semantic_state_and_effect_ledger() {
    let f = fixture();
    for _ in 0..2 {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("runtime-v1");
        let service = host::open(
            &dir,
            "contract",
            tokage_native::env::ShellEnv {
                vars: Default::default(),
            },
        )
        .unwrap();
        for (index, kind) in ["CreateRun", "ImportPlan", "ConfirmPlan", "CancelRun"]
            .into_iter()
            .enumerate()
        {
            let mut c = f["commands"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["wire"]["type"] == kind)
                .unwrap()["wire"]
                .clone();
            if kind == "ConfirmPlan" {
                c["payload"]["confirmation_digest"] = json!(
                    service
                        .snapshot()
                        .await
                        .unwrap()
                        .unwrap()
                        .plan
                        .unwrap()
                        .confirmation_digest
                );
            }
            let request:Request=serde_json::from_value(json!({"schemaVersion":1,"runId":"contract","commandId":kind,"expectedRevision":index,"command":c})).unwrap();
            let receipt = service.submit(&request).await.unwrap();
            assert_eq!(service.submit(&request).await.unwrap(), receipt);
            let s = service.snapshot().await.unwrap().unwrap();
            assert_eq!(
                serde_json::to_value(s.state).unwrap(),
                f["scenario"]["states"][index]
            );
            assert_eq!(s.revision, index as u64 + 1);
            assert!(
                s.turns.is_empty()
                    && s.execution.is_none()
                    && s.work.is_none()
                    && s.delivery.is_none()
            );
            if let Some(p) = s.plan {
                assert_eq!(
                    p.assignments["T1"].requested_model,
                    f["scenario"]["expectedModel"]
                );
            }
            let mut stale = request.clone();
            stale.command_id = format!("stale-{index}");
            stale.expected_revision = 0;
            assert!(service.submit(&stale).await.is_err());
            assert_eq!(
                service.snapshot().await.unwrap().unwrap().revision,
                s.revision
            );
        }
        let mut reader = Reader::open(&dir).unwrap();
        let first = reader.events("contract", 0, 2).unwrap();
        let rest = reader.events("contract", 2, 2).unwrap();
        let all: Vec<_> = first.into_iter().chain(rest).collect();
        assert_eq!(
            all.iter()
                .map(|e| serde_json::to_value(&e.kind).unwrap())
                .collect::<Vec<_>>(),
            f["scenario"]["events"].as_array().unwrap().clone()
        );
        assert!(reader.events("contract", 4, 2).unwrap().is_empty());
        assert_eq!(reader.events("contract", 0, 4).unwrap(), all);
        assert!(!temp
            .path()
            .join("runtime-v1.native")
            .join("worktrees")
            .exists());
    }
}
#[cfg(unix)]
mod common;
#[cfg(unix)]
#[path = "common/native.rs"]
mod native;
#[cfg(unix)]
#[tokio::test]
async fn equal_native_results_have_equal_business_decisions_and_effect_ledger() {
    // Assert an independent semantic oracle, not equality between two orchestration engines.
    // Paths, event batching, timestamps, attempt IDs and Git OIDs are deliberately excluded.
    for _ in 0..2 {
        let (dir, cwd, service, start) =
            native::prepared("normal", "git status --porcelain", false).await;
        service.submit(&start).await.unwrap();
        let s = native::settled(&service).await;
        assert_eq!(s.state, RunState::AwaitingMerge);
        let task = &s.execution.as_ref().unwrap().tasks["T1"];
        assert_eq!(task.state, tokage_orchestration::TaskState::Done);
        assert_eq!(task.attempts.len(), 1);
        let a = &s.work.as_ref().unwrap().attempts[&task.attempts[0]];
        assert!(
            a.commit_intent && a.native_intent && a.commit.is_some() && a.integrated_head.is_some()
        );
        assert!(a.manual_receipt.is_none());
        assert_eq!(a.gates.len(), 1);
        assert!(a.gates[0].result.as_ref().unwrap().passed && a.gates[0].cleanup_confirmed);
        assert_eq!(a.gates[0].before, a.gates[0].after.clone().unwrap());
        assert_eq!(s.turns.len(), 2); // one executor and one independent final reviewer
        for turn in s.turns.values() {
            assert!(turn.ended && turn.cleanup_confirmed);
            assert_eq!(turn.status, tokage_runtime::turn::TurnStatus::Completed);
            assert!(turn.cost_usd.is_none());
        }
        let integration = s.work.as_ref().unwrap().integration.as_ref().unwrap();
        assert_eq!(
            native::git(
                std::path::Path::new(&integration.cwd),
                &["show", "HEAD:hello.txt"]
            ),
            "after"
        );
        assert_eq!(std::fs::read(cwd.join("hello.txt")).unwrap(), b"before\n");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("calls.jsonl"))
                .unwrap()
                .lines()
                .count(),
            2
        );
        assert!(s.delivery.as_ref().unwrap().confirmation_digest.is_some());
    }
}
