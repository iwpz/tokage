mod common;
use common::*;
use tokage_runtime::{
    repository::{Reader, SqliteRepository},
    *,
};
#[tokio::test]
async fn read_only_observation_does_not_take_owner_create_data_or_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing");
    assert!(Reader::open(&path).is_err());
    assert!(!path.exists());
    let mut owner = SqliteRepository::open(&path, "run").unwrap();
    owner.submit(&create()).await.unwrap();
    let mut observer = Reader::open(&path).unwrap();
    assert_eq!(
        observer.snapshot("run").unwrap().unwrap().state,
        RunState::Draft
    );
    assert_eq!(observer.list().unwrap().len(), 1);
    assert_eq!(observer.events("run", 0, 1).unwrap().len(), 1);
    assert!(observer.events("run", 1, 1).unwrap().is_empty());
    assert!(matches!(
        SqliteRepository::open(&path, "run"),
        Err(Error::OwnerConflict)
    ));
    assert!(observer.events("run", 0, 1001).is_err());
}
#[test]
fn combined_confirmation_binds_original_plan_and_never_trusts_imported_commands() {
    let (s, _) = transition(None, &create()).unwrap();
    let (s, _) = transition(
        Some(&s),
        &command(&s, "import", Command::ImportPlan { text: PLAN.into() }),
    )
    .unwrap();
    let sha = s.plan.as_ref().unwrap().confirmation_digest.clone();
    assert!(transition(
        Some(&s),
        &command(
            &s,
            "bad",
            Command::ConfirmPlan {
                confirmation_digest: "old".into(),
                confirmed_commands: vec![]
            }
        )
    )
    .is_err());
    assert!(transition(
        Some(&s),
        &command(
            &s,
            "bad",
            Command::ConfirmPlan {
                confirmation_digest: sha.clone(),
                confirmed_commands: vec!["unrelated command".into()]
            }
        )
    )
    .is_err());
    let (approved, _) = transition(
        Some(&s),
        &command(
            &s,
            "approve",
            Command::ConfirmPlan {
                confirmation_digest: sha,
                confirmed_commands: vec!["cargo test".into()],
            },
        ),
    )
    .unwrap();
    assert_eq!(approved.state, RunState::Approved);
    assert_eq!(
        approved.approved_digest,
        approved
            .plan
            .as_ref()
            .map(|p| p.confirmation_digest.clone())
    );
    assert_eq!(approved.config.confirmed_commands, vec!["cargo test"]);
    assert_ne!(approved.config_hash, s.config_hash);
}
#[test]
fn retry_preparation_requires_exact_confirmation_and_preserves_attempt_history() {
    let mut req = create();
    if let Command::CreateRun { config, .. } = &mut req.command {
        config.permission_mode = "full-auto".into();
        config.target_reference = Some("refs/heads/main".into());
        config.confirmed_commands = vec!["cargo test".into()];
    }
    let (s, _) = transition(None, &req).unwrap();
    let (s, _) = transition(
        Some(&s),
        &command(&s, "import", Command::ImportPlan { text: PLAN.into() }),
    )
    .unwrap();
    let sha = s.plan.as_ref().unwrap().confirmation_digest.clone();
    let (s, _) = transition(
        Some(&s),
        &command(
            &s,
            "approve",
            Command::ApprovePlan {
                confirmation_digest: sha.clone(),
            },
        ),
    )
    .unwrap();
    let (mut s, _) = transition(
        Some(&s),
        &command(
            &s,
            "start",
            Command::StartExecution {
                confirmation_digest: sha.clone(),
            },
        ),
    )
    .unwrap();
    s.state = RunState::Stopped;
    s.approved_digest = None;
    let task = s.execution.as_mut().unwrap().tasks.get_mut("T1").unwrap();
    task.state = tokage_orchestration::TaskState::Cancelled;
    task.attempts.push("prior-attempt".into());
    assert!(transition(
        Some(&s),
        &command(
            &s,
            "bad",
            Command::PrepareRetry {
                confirmation_digest: "old".into()
            }
        )
    )
    .is_err());
    let (ready, _) = transition(
        Some(&s),
        &command(
            &s,
            "retry",
            Command::PrepareRetry {
                confirmation_digest: sha,
            },
        ),
    )
    .unwrap();
    assert_eq!(ready.state, RunState::Approved);
    assert_eq!(
        ready.execution.as_ref().unwrap().tasks["T1"].attempts,
        vec!["prior-attempt"]
    );
    let (started, _) = transition(
        Some(&ready),
        &command(
            &ready,
            "restart",
            Command::StartExecution {
                confirmation_digest: ready.approved_digest.clone().unwrap(),
            },
        ),
    )
    .unwrap();
    assert_eq!(
        started.execution.as_ref().unwrap().tasks["T1"].attempts,
        vec!["prior-attempt"]
    );
}
