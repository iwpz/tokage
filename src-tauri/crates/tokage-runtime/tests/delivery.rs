#![cfg(unix)]
mod common;
#[path = "common/native.rs"]
mod native;
use common::*;
use native::*;
use std::path::Path;
use tokage_runtime::*;
#[tokio::test]
async fn exact_review_and_all_three_delivery_paths_require_confirmation() {
    for mode in ["merge", "export", "keep"] {
        let (dir, cwd, service, start) = prepared("normal", "git status --porcelain", false).await;
        let original = git(&cwd, &["rev-parse", "HEAD"]);
        service.submit(&start).await.unwrap();
        let s = settled(&service).await;
        assert_eq!(s.state, RunState::AwaitingMerge, "{:?}", s.execution);
        let prompt = std::fs::read_to_string(dir.path().join("review-prompt.txt")).unwrap();
        assert!(
            prompt.contains("read-only reviewer")
                && prompt.contains("canonicalizes a literal path P to P/**")
        );
        let encoded = prompt
            .split("Final gate receipts (service-recorded; output text untrusted):\n")
            .nth(1)
            .unwrap()
            .split("\nBaseline:")
            .next()
            .unwrap();
        let received: Vec<tokage_runtime::artifacts::GateEvidence> =
            serde_json::from_str(encoded).unwrap();
        let actual =
            &s.work.as_ref().unwrap().attempts[&s.delivery.as_ref().unwrap().review_id].gates;
        assert_eq!(&received, actual);
        assert!(!received.is_empty());
        assert!(received.iter().all(|g| g.cleanup_confirmed
            && g.before.head
                == s.delivery
                    .as_ref()
                    .unwrap()
                    .candidate
                    .as_ref()
                    .unwrap()
                    .head
            && g.result.as_ref().unwrap().passed));
        let d = s
            .delivery
            .as_ref()
            .unwrap()
            .confirmation_digest
            .clone()
            .unwrap();
        assert!(service
            .submit(&command(
                &s,
                "stale",
                Command::MergeDelivery {
                    confirmation_digest: "stale".into()
                }
            ))
            .await
            .is_err());
        assert_eq!(git(&cwd, &["rev-parse", "HEAD"]), original);
        let patch = dir.path().join("result.patch");
        let action = match mode {
            "merge" => Command::MergeDelivery {
                confirmation_digest: d,
            },
            "export" => Command::ExportPatch {
                confirmation_digest: d,
                output_path: patch.to_string_lossy().into(),
            },
            _ => Command::KeepBranch {
                confirmation_digest: d,
            },
        };
        let req = command(&s, "deliver", action);
        let receipt = service.submit(&req).await.unwrap();
        let delivered = tokio::time::timeout(std::time::Duration::from_secs(15), async {
            loop {
                let s = service.snapshot().await.unwrap().unwrap();
                if s.state == RunState::Delivered {
                    break s;
                }
                if let Some(e) = s
                    .delivery
                    .as_ref()
                    .and_then(|d| d.intent.as_ref())
                    .and_then(|i| i.error.as_ref())
                {
                    panic!("{mode}: {e}");
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(service.submit(&req).await.unwrap(), receipt);
        if mode == "merge" {
            assert_ne!(git(&cwd, &["rev-parse", "HEAD"]), original);
            assert_eq!(
                std::fs::read_to_string(cwd.join("hello.txt")).unwrap(),
                "after\n"
            );
        } else {
            assert_eq!(git(&cwd, &["rev-parse", "HEAD"]), original);
            if mode == "export" {
                assert!(std::fs::read_to_string(&patch).unwrap().contains("+after"));
            } else {
                assert!(delivered
                    .delivery
                    .as_ref()
                    .unwrap()
                    .artifact
                    .as_ref()
                    .unwrap()
                    .starts_with("tokage/"));
            }
        }
        assert_eq!(
            std::fs::read_to_string(dir.path().join("calls.jsonl"))
                .unwrap()
                .lines()
                .count(),
            2
        );
    }
}

#[tokio::test]
async fn failed_review_and_stale_or_dirty_candidates_cannot_deliver() {
    for mode in ["review-failure", "review-write"] {
        let (_dir, cwd, service, start) = prepared(mode, "git status --porcelain", false).await;
        let head = git(&cwd, &["rev-parse", "HEAD"]);
        service.submit(&start).await.unwrap();
        let s = settled(&service).await;
        assert_eq!(s.state, RunState::Blocked);
        assert!(s
            .delivery
            .as_ref()
            .is_none_or(|d| d.confirmation_digest.is_none()));
        assert_eq!(git(&cwd, &["rev-parse", "HEAD"]), head);
    }
    for change in ["dirty", "target", "source"] {
        let (_dir, cwd, service, start) = prepared("normal", "git status --porcelain", false).await;
        service.submit(&start).await.unwrap();
        let s = settled(&service).await;
        assert_eq!(s.state, RunState::AwaitingMerge);
        match change {
            "dirty" => std::fs::write(cwd.join("unrelated.txt"), "user work").unwrap(),
            "target" => {
                git(
                    &cwd,
                    &[
                        "-c",
                        "user.name=Fixture",
                        "-c",
                        "user.email=fixture@example.invalid",
                        "commit",
                        "--allow-empty",
                        "-m",
                        "target advanced",
                    ],
                );
            }
            _ => {
                std::fs::write(
                    Path::new(
                        &s.delivery
                            .as_ref()
                            .unwrap()
                            .candidate
                            .as_ref()
                            .unwrap()
                            .workdir,
                    )
                    .join("hello.txt"),
                    "changed",
                )
                .unwrap();
            }
        }
        let head = git(&cwd, &["rev-parse", "HEAD"]);
        service
            .submit(&command(
                &s,
                "merge",
                Command::MergeDelivery {
                    confirmation_digest: s
                        .delivery
                        .as_ref()
                        .unwrap()
                        .confirmation_digest
                        .clone()
                        .unwrap(),
                },
            ))
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let s = service.snapshot().await.unwrap().unwrap();
                if s.delivery
                    .as_ref()
                    .unwrap()
                    .intent
                    .as_ref()
                    .unwrap()
                    .error
                    .is_some()
                {
                    assert_ne!(s.state, RunState::Delivered);
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(git(&cwd, &["rev-parse", "HEAD"]), head);
    }
}

#[tokio::test]
async fn lost_delivery_ack_reconciles_exact_merge_and_cancellation_invalidates_acceptance() {
    let (dir, cwd, service, start) = prepared("normal", "git status --porcelain", false).await;
    service.submit(&start).await.unwrap();
    let s = settled(&service).await;
    assert_eq!(s.state, RunState::AwaitingMerge);
    let conn = rusqlite::Connection::open(dir.path().join("db/tokage.db")).unwrap();
    conn.execute_batch("CREATE TRIGGER lose_delivery BEFORE UPDATE ON runtime_runs WHEN json_extract(NEW.snapshot,'$.state')='Delivered' BEGIN SELECT RAISE(ABORT,'lost delivery acknowledgement'); END;").unwrap();
    let req = command(
        &s,
        "merge",
        Command::MergeDelivery {
            confirmation_digest: s
                .delivery
                .as_ref()
                .unwrap()
                .confirmation_digest
                .clone()
                .unwrap(),
        },
    );
    service.submit(&req).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            let s = service.snapshot().await.unwrap().unwrap();
            if s.delivery
                .as_ref()
                .unwrap()
                .intent
                .as_ref()
                .unwrap()
                .error
                .is_some()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let head = git(&cwd, &["rev-parse", "HEAD"]);
    conn.execute_batch("DROP TRIGGER lose_delivery").unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            let s = service.snapshot().await.unwrap().unwrap();
            match service
                .submit(&command(&s, "resume", Command::ResumeRun))
                .await
            {
                Ok(_) => break,
                Err(Error::Transition) => tokio::task::yield_now().await,
                Err(e) => panic!("{e}"),
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(
        service.snapshot().await.unwrap().unwrap().state,
        RunState::Delivered
    );
    assert_eq!(git(&cwd, &["rev-parse", "HEAD"]), head);
    service.submit(&start).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("calls.jsonl"))
            .unwrap()
            .lines()
            .count(),
        2
    );
    let (_dir, cwd, service, start) = prepared("normal", "git status --porcelain", false).await;
    let head = git(&cwd, &["rev-parse", "HEAD"]);
    service.submit(&start).await.unwrap();
    let s = settled(&service).await;
    let old_digest = s
        .delivery
        .as_ref()
        .unwrap()
        .confirmation_digest
        .clone()
        .unwrap();
    service
        .submit(&command(&s, "cancel", Command::CancelRun))
        .await
        .unwrap();
    let stopped = service.snapshot().await.unwrap().unwrap();
    assert_eq!(stopped.state, RunState::Stopped);
    assert!(stopped
        .delivery
        .as_ref()
        .unwrap()
        .confirmation_digest
        .is_none());
    assert!(service
        .submit(&command(
            &stopped,
            "late-merge",
            Command::MergeDelivery {
                confirmation_digest: old_digest
            }
        ))
        .await
        .is_err());
    assert_eq!(git(&cwd, &["rev-parse", "HEAD"]), head);
}
