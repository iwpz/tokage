mod common;
use common::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokage_runtime::{
    repository::SqliteRepository,
    service::{drive_planning, Planner},
    *,
};

#[tokio::test]
async fn import_edit_and_config_changes_invalidate_authorization_durably() {
    let dir = tempfile::tempdir().unwrap();
    let mut repo = SqliteRepository::open(dir.path(), "run").unwrap();
    repo.submit(&create()).await.unwrap();
    let s = repo.snapshot("run").unwrap().unwrap();
    repo.submit(&command(
        &s,
        "import",
        Command::ImportPlan { text: PLAN.into() },
    ))
    .await
    .unwrap();
    let s = repo.snapshot("run").unwrap().unwrap();
    let p = s.plan.as_ref().unwrap();
    assert_eq!(p.assignments["T1"].requested_model, "gpt-5.6-sol");
    assert!(p.plan.tasks[0].assigned_model.is_none());
    let digest = p.confirmation_digest.clone();
    assert!(repo
        .submit(&command(
            &s,
            "stale",
            Command::ApprovePlan {
                confirmation_digest: "stale".into()
            }
        ))
        .await
        .is_err());
    let approval = command(
        &s,
        "approve",
        Command::ApprovePlan {
            confirmation_digest: digest.clone(),
        },
    );
    repo.submit(&approval).await.unwrap();
    let approved = repo.snapshot("run").unwrap().unwrap();
    assert_eq!(approved.state, RunState::Approved);
    assert_eq!(approved.approved_digest.as_deref(), Some(digest.as_str()));
    // Authorized edits are possible before any execution; no old approval survives.
    let mut config = approved.config.clone();
    config.policy_version = "2".into();
    config.models.get_mut("executor").unwrap().requested_model = "user-new-model".into();
    repo.submit(&command(
        &approved,
        "policy",
        Command::UpdateConfig { config },
    ))
    .await
    .unwrap();
    let changed = repo.snapshot("run").unwrap().unwrap();
    assert_eq!(changed.state, RunState::AwaitingApproval);
    assert!(changed.approved_digest.is_none());
    assert_ne!(changed.plan.as_ref().unwrap().confirmation_digest, digest);
    assert!(repo
        .submit(&command(
            &changed,
            "old-approval",
            Command::ApprovePlan {
                confirmation_digest: digest
            }
        ))
        .await
        .is_err());
    // Replaying a successful command never reauthorizes the changed plan.
    assert_eq!(
        repo.submit(&approval).await.unwrap().revision,
        approved.revision
    );
    drop(repo);
    let mut repo = SqliteRepository::open(dir.path(), "run").unwrap();
    let s = repo.snapshot("run").unwrap().unwrap();
    assert!(s.approved_digest.is_none());
    repo.submit(&command(
        &s,
        "edit",
        Command::ImportPlan {
            text: PLAN.replace("Feature", "Feature edited"),
        },
    ))
    .await
    .unwrap();
    let s = repo.snapshot("run").unwrap().unwrap();
    assert!(s.plan.unwrap().plan.tasks[0].title.contains("edited"));
}
#[test]
fn deterministic_normalization_preserves_locks_and_rejects_invalid_plans() {
    let (s, _) = transition(None, &create()).unwrap();
    let first = planning::normalize(PLAN, &s.config).unwrap();
    for _ in 0..20 {
        assert_eq!(planning::normalize(PLAN, &s.config).unwrap(), first);
    }
    let mut unlocked = s.config.clone();
    unlocked.models.get_mut("executor").unwrap().user_locked = false;
    let auto = planning::normalize(PLAN, &unlocked).unwrap();
    for _ in 0..20 {
        assert_eq!(planning::normalize(PLAN, &unlocked).unwrap(), auto);
    }
    assert!(
        planning::normalize(&PLAN.replace("src/feature/**", "../escape/**"), &s.config).is_err()
    );
    assert!(planning::normalize(&PLAN.replace("src/feature/**", "outside/**"), &s.config).is_err());
    assert!(planning::normalize(
        &PLAN.replace("  title:", "  depends_on: [missing]\n  title:"),
        &s.config
    )
    .is_err());
    assert!(planning::normalize(
        &PLAN.replace("  title:", "  depends_on: [T1]\n  title:"),
        &s.config
    )
    .is_err());
    assert!(planning::normalize(
        &PLAN.replace("cargo test", "cargo test; echo injected"),
        &s.config
    )
    .is_err());
}
#[test]
fn model_confirmation_is_discarded_and_explicit_config_confirmation_is_bound() {
    let (mut s, _) = transition(None, &create()).unwrap();
    let text = PLAN.replace(
        "commands: [cargo test]",
        "commands: [make custom-check]\n    confirmedCommands: [make custom-check]",
    );
    s.plan = Some(planning::normalize(&text, &s.config).unwrap());
    let d = s.plan.as_ref().unwrap().confirmation_digest.clone();
    assert!(s.plan.as_ref().unwrap().plan.tasks[0]
        .acceptance
        .confirmed_commands
        .is_empty());
    assert!(planning::approve(&s, &d).is_err());
    s.config.confirmed_commands.push("make custom-check".into());
    s.plan = Some(planning::normalize(&text, &s.config).unwrap());
    assert_ne!(s.plan.as_ref().unwrap().confirmation_digest, d);
    planning::approve(&s, &s.plan.as_ref().unwrap().confirmation_digest).unwrap();
    s.config.trust_workspace = false;
    assert!(planning::approve(&s, &s.plan.as_ref().unwrap().confirmation_digest).is_err());
}
struct OfflinePlanner {
    calls: AtomicUsize,
    fail: bool,
}
impl Planner for OfflinePlanner {
    async fn generate(&self, _s: &Snapshot, _attempt: &str) -> Result<String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            Err(Error::Invalid("offline provider failure".into()))
        } else {
            Ok(PLAN.into())
        }
    }
}
#[tokio::test]
async fn planning_intent_and_claim_survive_replay_failure_and_restart() {
    for fail in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut repo = SqliteRepository::open(dir.path(), "run").unwrap();
        repo.submit(&create()).await.unwrap();
        let s = repo.snapshot("run").unwrap().unwrap();
        let req = command(&s, "plan", Command::GeneratePlan);
        let receipt = repo.submit(&req).await.unwrap();
        let p = OfflinePlanner {
            calls: AtomicUsize::new(0),
            fail,
        };
        assert_eq!(drive_planning(&mut repo, "run", &p).await.is_err(), fail);
        assert_eq!(repo.submit(&req).await.unwrap(), receipt);
        assert!(drive_planning(&mut repo, "run", &p).await.is_err());
        assert_eq!(p.calls.load(Ordering::SeqCst), 1);
        drop(repo);
        let repo = SqliteRepository::open(dir.path(), "run").unwrap();
        let s = repo.snapshot("run").unwrap().unwrap();
        assert_eq!(
            s.state,
            if fail {
                RunState::Blocked
            } else {
                RunState::AwaitingApproval
            }
        );
        assert_eq!(repo.events("run", 0, 100).unwrap().len(), 4);
    }
}
#[tokio::test]
async fn lost_planning_completion_blocks_duplicate_model_invocation() {
    let dir = tempfile::tempdir().unwrap();
    let mut repo = SqliteRepository::open(dir.path(), "run").unwrap();
    repo.submit(&create()).await.unwrap();
    let s = repo.snapshot("run").unwrap().unwrap();
    repo.submit(&command(&s, "plan", Command::GeneratePlan))
        .await
        .unwrap();
    let conn = rusqlite::Connection::open(dir.path().join("tokage.db")).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_completion BEFORE INSERT ON runtime_events WHEN NEW.sequence=4 BEGIN SELECT RAISE(ABORT,'completion write failed'); END;").unwrap();
    let p = OfflinePlanner {
        calls: AtomicUsize::new(0),
        fail: false,
    };
    assert!(drive_planning(&mut repo, "run", &p).await.is_err());
    conn.execute_batch("DROP TRIGGER fail_completion").unwrap();
    drop(repo);
    let mut repo = SqliteRepository::open(dir.path(), "run").unwrap();
    assert_eq!(
        repo.snapshot("run").unwrap().unwrap().state,
        RunState::Planning
    );
    assert!(drive_planning(&mut repo, "run", &p).await.is_err());
    assert_eq!(p.calls.load(Ordering::SeqCst), 1);
}
#[cfg(unix)]
#[tokio::test]
async fn native_read_only_planner_uses_vendor_protocol_and_run_specific_process_registry() {
    use std::{os::unix::fs::PermissionsExt, process::Command as Process};
    use tokage_runtime::service::NativePlanner;
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().join("repo");
    std::fs::create_dir(&cwd).unwrap();
    let git = |args: &[&str]| {
        let out = Process::new("git")
            .arg("-C")
            .arg(&cwd)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    git(&["init"]);
    std::fs::write(cwd.join("README.md"), "fixture").unwrap();
    git(&["add", "README.md"]);
    git(&[
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@example.invalid",
        "commit",
        "-m",
        "fixture",
    ]);
    let provider = dir.path().join("codex");
    std::fs::write(
        &provider,
        include_str!("../../../../cli/tests/fixtures/codex.py"),
    )
    .unwrap();
    std::fs::set_permissions(&provider, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(dir.path().join("mode.txt"), "normal").unwrap();
    std::fs::write(dir.path().join("plan.md"), PLAN).unwrap();
    let mut req = create();
    if let Command::CreateRun { config, .. } = &mut req.command {
        config.workspace = cwd.to_string_lossy().into();
        config.target_identity = git(&["rev-parse", "HEAD"]);
        config.models.get_mut("executor").unwrap().provider_path =
            Some(provider.to_string_lossy().into());
    }
    let mut repo = SqliteRepository::open(&dir.path().join("db"), "run").unwrap();
    repo.submit(&req).await.unwrap();
    let s = repo.snapshot("run").unwrap().unwrap();
    repo.submit(&command(&s, "plan", Command::GeneratePlan))
        .await
        .unwrap();
    let native = NativePlanner::new(
        tokage_native::env::ShellEnv {
            vars: std::env::vars().collect(),
        },
        &dir.path().join("native"),
    )
    .unwrap();
    drive_planning(&mut repo, "run", &native).await.unwrap();
    assert_eq!(
        repo.snapshot("run").unwrap().unwrap().state,
        RunState::AwaitingApproval
    );
    let calls = std::fs::read_to_string(dir.path().join("calls.jsonl")).unwrap();
    assert_eq!(calls.lines().count(), 1);
    assert!(calls.contains("read-only"));
    assert_eq!(git(&["status", "--porcelain"]), "");
}
