mod common;
#[cfg(unix)]
#[path="common/native.rs"]
mod native_fixture;
use common::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokage_runtime::{
    owned::Runtime,
    repository::SqliteRepository,
    service::{Planner, TurnControl},
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
    calls: Arc<AtomicUsize>,
    fail: bool,
}
impl Planner for OfflinePlanner {
    async fn generate_observed(
        &self,
        _: &Snapshot,
        _: &str,
        control: TurnControl,
    ) -> Result<String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            return Err(Error::Invalid("offline provider failure".into()));
        }
        control
            .events
            .send(tokage_core::EventKind::Text { text: PLAN.into() })
            .await
            .unwrap();
        let done:tokage_core::EventKind=serde_json::from_value(serde_json::json!({"kind":"done","inputTokens":10,"outputTokens":5,"source":"STRUCTURED"})).unwrap();
        control.events.send(done).await.unwrap();
        Ok(PLAN.into())
    }
}
async fn settled<P: Planner + 'static>(service: &Runtime<P>) -> Snapshot {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let s = service.snapshot().await.unwrap().unwrap();
            if matches!(s.state, RunState::AwaitingApproval | RunState::Blocked) {
                return s;
            }
            if let Some(e) = service.worker_error() {
                panic!("{e}");
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap()
}
#[tokio::test]
async fn planning_intent_and_claim_survive_replay_failure_and_restart() {
    for fail in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let repo = SqliteRepository::open(dir.path(), "run").unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let service = Runtime::new(
            repo,
            "run",
            OfflinePlanner {
                calls: calls.clone(),
                fail,
            },
        )
        .unwrap();
        service.submit(&create()).await.unwrap();
        let s = service.snapshot().await.unwrap().unwrap();
        let req = command(&s, "plan", Command::GeneratePlan);
        let receipt = service.submit(&req).await.unwrap();
        let s = settled(&service).await;
        assert_eq!(
            s.state,
            if fail {
                RunState::Blocked
            } else {
                RunState::AwaitingApproval
            }
        );
        assert_eq!(service.submit(&req).await.unwrap(), receipt);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        drop(service);
        let repo = SqliteRepository::open(dir.path(), "run").unwrap();
        assert_eq!(repo.snapshot("run").unwrap().unwrap(), s);
    }
}
#[tokio::test]
async fn lost_planning_completion_blocks_duplicate_model_invocation() {
    let dir = tempfile::tempdir().unwrap();
    let repo = SqliteRepository::open(dir.path(), "run").unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let service = Runtime::new(
        repo,
        "run",
        OfflinePlanner {
            calls: calls.clone(),
            fail: false,
        },
    )
    .unwrap();
    service.submit(&create()).await.unwrap();
    let conn = rusqlite::Connection::open(dir.path().join("tokage.db")).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_completion BEFORE INSERT ON runtime_events WHEN json_extract(NEW.event,'$.kind')='TurnFinished' BEGIN SELECT RAISE(ABORT,'completion write failed'); END;").unwrap();
    let s = service.snapshot().await.unwrap().unwrap();
    let req = command(&s, "plan", Command::GeneratePlan);
    let receipt = service.submit(&req).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while service.worker_error().is_none() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    conn.execute_batch("DROP TRIGGER fail_completion").unwrap();
    assert_eq!(
        service.snapshot().await.unwrap().unwrap().state,
        RunState::Planning
    );
    assert_eq!(service.submit(&req).await.unwrap(), receipt);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    drop(service);
    let repo = SqliteRepository::open(dir.path(), "run").unwrap();
    let service = Runtime::new(
        repo,
        "run",
        OfflinePlanner {
            calls: calls.clone(),
            fail: false,
        },
    )
    .unwrap();
    assert_eq!(service.submit(&req).await.unwrap(), receipt);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let s = service.snapshot().await.unwrap().unwrap();
    service
        .submit(&command(&s, "recover", Command::ResumeRun))
        .await
        .unwrap();
    assert_eq!(
        service.snapshot().await.unwrap().unwrap().state,
        RunState::Blocked
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
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
    let service = Runtime::new(repo, "run", native.clone()).unwrap();
    // The committed unclaimed intent is picked up by retrying its original command.
    let initial = command(&s, "plan", Command::GeneratePlan);
    service.submit(&initial).await.unwrap();
    let accepted = settled(&service).await;
    assert_eq!(
        accepted
            .turns
            .values()
            .next()
            .unwrap()
            .usage
            .as_ref()
            .unwrap()
            .input_tokens,
        10
    );
    assert_eq!(
        service.snapshot().await.unwrap().unwrap().state,
        RunState::AwaitingApproval
    );
    let calls = std::fs::read_to_string(dir.path().join("calls.jsonl")).unwrap();
    assert_eq!(calls.lines().count(), 1);
    assert!(calls.contains("read-only"));
    assert_eq!(git(&["status", "--porcelain"]), "");
    // Exercise explicit cancellation through a second ordinary run and a real child process.
    std::fs::write(dir.path().join("mode.txt"),"hang").unwrap();
    let repo2=SqliteRepository::open(&dir.path().join("db"),"run2").unwrap();
    let second=Runtime::new(repo2,"run2",native).unwrap();let mut req2=req.clone();req2.run_id="run2".into();req2.command_id="create2".into();second.submit(&req2).await.unwrap();
    let s=second.snapshot().await.unwrap().unwrap();second.submit(&command(&s,"plan2",Command::GeneratePlan)).await.unwrap();
    let running=tokio::time::timeout(std::time::Duration::from_secs(10),async{loop{let s=second.snapshot().await.unwrap().unwrap();if s.turns.values().any(|t|t.last_sequence>=1){return s;}tokio::task::yield_now().await;}}).await.unwrap();
    second.submit(&command(&running,"cancel2",Command::CancelRun)).await.unwrap();
    let stopped=tokio::time::timeout(std::time::Duration::from_secs(15),async{loop{let s=second.snapshot().await.unwrap().unwrap();if s.state==RunState::Stopped{return s;}if let Some(e)=second.worker_error(){panic!("{e}");}tokio::task::yield_now().await;}}).await.unwrap();
    assert!(stopped.turns.values().all(|t|t.cleanup_confirmed && t.status==tokage_runtime::turn::TurnStatus::Stopped));
    assert_eq!(std::fs::read_to_string(dir.path().join("calls.jsonl")).unwrap().lines().count(),2);
}
#[test]
fn frozen_learning_changes_automatic_choice_but_never_overrides_user_lock(){
    let (s,_)=transition(None,&create()).unwrap();let mut config=s.config.clone();
    let mut alternative=config.models["executor"].clone();alternative.provider="CLAUDE_CODE".into();alternative.requested_model="sonnet".into();alternative.user_locked=false;config.models.insert("alternative".into(),alternative);
    let pack=planning::effective_pack(&config).unwrap();let base=pack.candidates.iter().find(|m|m.provider=="CODEX_CLI" && m.cli_model_arg=="gpt-5.6-sol").unwrap();let other=pack.candidates.iter().find(|m|m.provider=="CLAUDE_CODE" && m.cli_model_arg=="sonnet").unwrap();
    let row=|provider:&str,model:&str,alpha,beta|planning::LearnedObservation{provider:provider.into(),model:model.into(),domain:"backend-api".into(),bucket:"d2".into(),alpha,beta,samples:10000,fix_rate:Some(0.1)};
    config.planning_policy=Some(planning::PlanningPolicy{learned:vec![row(&base.provider,&base.model_id,0.0,10000.0),row(&other.provider,&other.model_id,10000.0,0.0)],..Default::default()});
    let locked=planning::normalize(PLAN,&config).unwrap();assert_eq!(locked.assignments["T1"].requested_model,"gpt-5.6-sol");
    config.models.get_mut("executor").unwrap().user_locked=false;
    let automatic=planning::normalize(PLAN,&config).unwrap();assert_eq!(automatic.assignments["T1"].provider,"CLAUDE_CODE");assert_eq!(automatic.assignments["T1"].requested_model,"sonnet");assert_ne!(locked.confirmation_digest,automatic.confirmation_digest);
    config.planning_policy.as_mut().unwrap().learned[0].alpha=-1.0;assert!(config.validate().is_err());
}

struct CommandPlanner {
    calls: AtomicUsize,
    mismatch: bool,
}
impl Planner for CommandPlanner {
    async fn generate_observed(&self, _: &Snapshot, _: &str, control: TurnControl) -> Result<String> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        let commands = if call == 0 { "cargo test && cargo check" } else { "cargo test, cargo check" };
        let text = format!("I inspected the project.\n\n{}", PLAN.replace("commands: [cargo test]", &format!("commands: [{commands}]")));
        for fragment in [&text[..5], &text[5..]] {
            control.events.send(tokage_core::EventKind::Text { text: fragment.into() }).await.unwrap();
        }
        let done = serde_json::from_value(serde_json::json!({"kind":"done","inputTokens":10,"outputTokens":5,"costUsd":0.001,"source":"STRUCTURED"})).unwrap();
        control.events.send(done).await.unwrap();
        Ok(if self.mismatch { PLAN.into() } else { text })
    }
}

#[tokio::test]
async fn invalid_plan_reports_the_actual_rule_and_explicit_retry_preserves_failed_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let repo = SqliteRepository::open(dir.path(), "run").unwrap();
    let service = Runtime::new(repo, "run", CommandPlanner { calls: AtomicUsize::new(0), mismatch: false }).unwrap();
    service.submit(&create()).await.unwrap();
    let s = service.snapshot().await.unwrap().unwrap();
    service.submit(&command(&s, "unsafe-plan", Command::GeneratePlan)).await.unwrap();
    let failed = settled(&service).await;
    assert_eq!(failed.state, RunState::Blocked);
    assert!(failed.plan.is_none());
    assert!(failed.approved_digest.is_none());
    let old = failed.turns.values().next().unwrap();
    let diagnostic = old.error.as_deref().unwrap();
    assert!(diagnostic.contains("unsafe_acceptance_command"), "{diagnostic}");
    assert!(diagnostic.contains("cargo test && cargo check"));
    assert!(!diagnostic.contains("evidence mismatch"));
    let old = old.clone();
    service.submit(&command(&failed, "retry-plan", Command::GeneratePlan)).await.unwrap();
    let accepted = settled(&service).await;
    assert_eq!(accepted.state, RunState::AwaitingApproval, "{:?}", accepted.turns);
    assert_eq!(accepted.turns[&old.id], old);
    assert_eq!(accepted.turns.len(), 2);
    assert!(accepted.approved_digest.is_none());
    assert_eq!(accepted.plan.as_ref().unwrap().plan.tasks[0].acceptance.commands, vec!["cargo test", "cargo check"]);
}

#[tokio::test]
async fn different_returned_and_persisted_plans_still_fail_evidence_validation() {
    let dir = tempfile::tempdir().unwrap();
    let repo = SqliteRepository::open(dir.path(), "run").unwrap();
    let service = Runtime::new(repo, "run", CommandPlanner { calls: AtomicUsize::new(1), mismatch: true }).unwrap();
    service.submit(&create()).await.unwrap();
    let s = service.snapshot().await.unwrap().unwrap();
    service.submit(&command(&s, "mismatch", Command::GeneratePlan)).await.unwrap();
    let failed = settled(&service).await;
    assert_eq!(failed.state, RunState::Blocked);
    assert!(failed.plan.is_none());
    assert!(failed.turns.values().next().unwrap().error.as_deref().unwrap().contains("evidence mismatch"));
}

#[cfg(unix)]
#[tokio::test]
async fn native_stream_rejects_changed_aggregates_missing_completion_and_late_failures() {
    use std::os::unix::fs::PermissionsExt;
    use tokage_runtime::service::NativePlanner;
    for mode in ["changed-aggregate", "missing-done", "success-nonzero", "late-error"] {
        let (dir, cwd, req) = native_fixture::fixture();
        let provider = dir.path().join("codex");
        std::fs::write(&provider, include_str!("../../../../cli/tests/fixtures/codex.py")).unwrap();
        std::fs::set_permissions(&provider, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(dir.path().join("mode.txt"), mode).unwrap();
        std::fs::write(dir.path().join("plan.md"), PLAN.replace("src/feature/**", "hello.txt")).unwrap();
        let repo = SqliteRepository::open(&dir.path().join("db"), "run").unwrap();
        let native = NativePlanner::new(tokage_native::env::ShellEnv {vars: std::env::vars().collect()}, &dir.path().join("native")).unwrap();
        let service = Runtime::new(repo, "run", native).unwrap();
        service.submit(&req).await.unwrap();
        let s = service.snapshot().await.unwrap().unwrap();
        service.submit(&command(&s, "plan", Command::GeneratePlan)).await.unwrap();
        let rejected = settled(&service).await;
        assert_eq!(rejected.state, RunState::Blocked, "{mode}");
        assert!(rejected.plan.is_none(), "{mode}");
        assert!(rejected.turns.values().all(|t| t.ended && t.cleanup_confirmed && t.error.is_some()), "{mode}");
        assert_eq!(native_fixture::git(&cwd, &["status", "--porcelain"]), "");
    }
}
