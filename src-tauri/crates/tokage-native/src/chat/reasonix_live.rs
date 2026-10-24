//! Opt-in acceptance against the locally configured DeepSeek connection.
use super::*;

#[tokio::test]
#[ignore = "real DeepSeek Flash; trusts and revokes only a fresh temporary fixture via official CLI"]
async fn git_workspace_trust_refusal_then_explicit_grant() {
    let env = crate::env::capture().await;
    let initial = crate::providers::detect_one(&env, ProviderId::Reasonix, None)
        .await
        .unwrap();
    crate::providers::verify_reasonix_key(
        std::path::Path::new(initial.path.as_deref().unwrap()),
        &env,
    )
    .await
    .unwrap();
    let provider = crate::providers::detect_one(&env, ProviderId::Reasonix, None)
        .await
        .unwrap();
    assert_eq!(provider.status, ProviderStatus::Ready);
    let dir = tempfile::tempdir().unwrap();
    assert!(std::process::Command::new("git")
        .arg("init")
        .arg(dir.path())
        .output()
        .unwrap()
        .status
        .success());
    let exe = provider.path.as_deref().unwrap();
    struct Grant<'a> {
        exe: &'a str,
        dir: &'a std::path::Path,
        env: &'a ShellEnv,
    }
    impl Drop for Grant<'_> {
        fn drop(&mut self) {
            let _ = std::process::Command::new(self.exe)
                .args(["trust", "--revoke", "--dir"])
                .arg(self.dir)
                .env_clear()
                .envs(&self.env.vars)
                .output();
        }
    }
    let mut req = testing::request(ProviderId::Reasonix);
    req.cwd = dir.path().display().to_string();
    req.trusted = vec![ProviderId::Reasonix];
    req.model = "deepseek/deepseek-flash".into();
    req.role = Some(Role::Executor);
    req.mode = TurnMode::Direct;
    req.idle_timeout_secs = Some(90);
    req.hard_timeout_secs = Some(150);
    req.prompt="Use write_file to create proof.txt with exactly TOKAGE_RX_TRUST_OK. Do not use shell or simulate success; if permission is denied, report the denial.".into();
    // Probe the CLI's native default trust policy explicitly. Tokage's normal
    // acceptEdits / FullAuto modes grant edits for a turn without persistent trust.
    let events = std::sync::Mutex::new(vec![]);
    let emit = |event| events.lock().unwrap().push(event);
    let mut spec = launch(&req).unwrap();
    let permission = spec
        .args
        .iter()
        .position(|a| a == "--permission-mode")
        .unwrap();
    spec.args.drain(permission..permission + 2);
    let result = run_cli(
        std::path::Path::new(exe),
        &req,
        spec,
        &env,
        None,
        None,
        &emit,
        None,
    )
    .await;
    let events = events.into_inner().unwrap();
    assert!(
        !dir.path().join("proof.txt").exists(),
        "untrusted fixture was modified"
    );
    assert!(result.is_err() && events.iter().any(|e|matches!(e, EventKind::ApprovalRequest{reason,..} if reason.contains("untrusted_folder"))), "missing trust refusal: {result:?}; {events:?}");
    assert!(
        !events.iter().any(|e| matches!(e, EventKind::Done { .. })),
        "refusal emitted Done"
    );
    let _grant = Grant {
        exe,
        dir: dir.path(),
        env: &env,
    };
    let result = tokio::process::Command::new(exe)
        .args(["trust", "--yes", "--dir"])
        .arg(dir.path())
        .env_clear()
        .envs(&env.vars)
        .output()
        .await
        .unwrap();
    assert!(result.status.success(), "temporary fixture trust failed");
    req.turn_id = "reasonix-trusted".into();
    std::fs::write(dir.path().join("check.py"), "from pathlib import Path\nassert Path('proof.txt').read_text().strip() == 'TOKAGE_RX_TRUST_OK'\nprint('VERIFIED')\n").unwrap();
    req.prompt = "Use write_file to create proof.txt containing TOKAGE_RX_TRUST_OK. Then use Bash to run exactly python3 check.py. Report the actual verification output. Do not change check.py.".into();
    let events = std::sync::Mutex::new(vec![]);
    let emit = |event| events.lock().unwrap().push(event);
    let mut spec = launch(&req).unwrap();
    spec.args.drain(permission..permission + 2);
    let result = run_cli(
        std::path::Path::new(exe),
        &req,
        spec,
        &env,
        None,
        None,
        &emit,
        None,
    )
    .await;
    assert!(
        result.is_ok(),
        "{result:?}; tools={:?}",
        events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| matches!(e, EventKind::ToolEnd { .. }))
            .collect::<Vec<_>>()
    );
    assert!(events
        .into_inner()
        .unwrap()
        .iter()
        .any(|e| matches!(e, EventKind::Done { .. })));

    assert_eq!(
        std::fs::read_to_string(dir.path().join("proof.txt"))
            .unwrap()
            .trim(),
        "TOKAGE_RX_TRUST_OK"
    );
    let revoked = tokio::process::Command::new(exe)
        .args(["trust", "--revoke", "--dir"])
        .arg(dir.path())
        .env_clear()
        .envs(&env.vars)
        .output()
        .await
        .unwrap();
    assert!(
        revoked.status.success(),
        "temporary fixture trust revoke failed"
    );
    println!("PASS real Git workspace refusal without Done, explicit CLI trust, sandboxed edit, trust revoked on cleanup");
}

async fn turn(req: ChatRequest, provider: &ProviderInfo, env: &ShellEnv) -> Vec<EventKind> {
    assert_eq!(req.model, "deepseek/deepseek-flash");
    let bus = Bus::new();
    let mut rx = bus.subscribe();
    assert!(!run_turn(&bus, req, vec![provider.clone()], env.clone(), None, None).await);
    let mut events = vec![];
    let mut succeeded = false;
    while let Ok(event) = rx.try_recv() {
        match event {
            DomainEvent::ChatOutput { event, .. } => events.push(event),
            DomainEvent::TurnSucceeded { .. } => succeeded = true,
            DomainEvent::TurnFailed { message, .. } => panic!("Reasonix failed: {message}"),
            _ => {}
        }
    }
    assert!(succeeded, "{events:?}");
    assert!(events.iter().any(|e| matches!(e, EventKind::Done { input_tokens, output_tokens, .. } if input_tokens + output_tokens > 0)));
    events
}
fn text(events: &[EventKind]) -> String {
    events.iter().filter_map(|e| match e { EventKind::Text { text } => Some(text.as_str()), _ => None }).collect()
}
fn thread(events: &[EventKind]) -> String {
    events.iter().find_map(|e| match e { EventKind::Thread { id } => Some(id.clone()), _ => None }).expect("native session")
}

#[tokio::test]
#[ignore = "uses configured DeepSeek Flash and consumes tokens"]
async fn deepseek_shell_execution() {
    let env = crate::env::capture().await;
    let provider = crate::providers::detect_one(&env, ProviderId::Reasonix, None).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut req = testing::request(ProviderId::Reasonix);
    req.cwd = dir.path().to_string_lossy().into_owned();
    req.model = "deepseek/deepseek-flash".into();
    req.trusted = vec![ProviderId::Reasonix];
    req.role = Some(Role::Planner);
    req.mode = TurnMode::Plan;
    req.idle_timeout_secs = Some(60);
    req.hard_timeout_secs = Some(120);
    req.prompt = "Use the Bash tool to run exactly: printf TOKAGE_RX_SHELL_OK. This is a read-only command. Report its actual output. Do not simulate execution or use a different tool.".into();
    let events = turn(req, &provider, &env).await;
    assert!(text(&events).contains("TOKAGE_RX_SHELL_OK"), "{}", text(&events));
    assert!(events.iter().any(|e| matches!(e, EventKind::ToolEnd { ok: Some(true), output: Some(output), .. } if output.contains("TOKAGE_RX_SHELL_OK"))));
    println!("PASS: Reasonix Bash returned actual output through the OS sandbox");
}

#[tokio::test]
#[ignore = "uses configured DeepSeek Flash and consumes tokens"]
async fn deepseek_edit_shell_resume_review() {
    let env = crate::env::capture().await;
    let provider = crate::providers::detect_one(&env, ProviderId::Reasonix, None).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("m.py");
    let broken = "def add(a, b):\n    return a - b\n";
    std::fs::write(&file, broken).unwrap();
    std::fs::write(dir.path().join("marker.txt"), "TOKAGE_RX_EXEC_42").unwrap();
    let mut req = testing::request(ProviderId::Reasonix);
    req.cwd = dir.path().to_string_lossy().into_owned();
    req.model = "deepseek/deepseek-flash".into();
    req.trusted = vec![ProviderId::Reasonix];
    req.role = Some(Role::Planner);
    req.mode = TurnMode::Plan;
    req.idle_timeout_secs = Some(90);
    req.hard_timeout_secs = Some(180);
    req.prompt = "Read marker.txt and m.py. Identify the addition bug and reply with the marker and a short plan. Do not modify files.".into();
    let plan = turn(req.clone(), &provider, &env).await;
    assert!(text(&plan).contains("TOKAGE_RX_EXEC_42"));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), broken);
    req.role = Some(Role::Executor);
    req.mode = TurnMode::Direct;
    req.auth_mode = Some(AuthMode::FullAuto);
    req.prompt = "Read marker.txt. Fix add in m.py to return a + b. Then use Bash to run exactly: python3 -c 'from m import add; assert add(2, 3) == 5; assert add(-2, 3) == 1; print(\"TOKAGE_RX_PYTHON_OK\")'. Report the marker and actual test output. Do not change any other file.".into();
    let edit = turn(req.clone(), &provider, &env).await;
    assert!(text(&edit).contains("TOKAGE_RX_EXEC_42"));
    assert!(edit.iter().any(|e| matches!(e, EventKind::ToolEnd { ok: Some(true), output: Some(output), .. } if output.contains("TOKAGE_RX_PYTHON_OK"))), "{edit:?}");
    let fixed = std::fs::read_to_string(&file).unwrap();
    assert_ne!(fixed, broken);
    let result = std::process::Command::new("python3").args(["-c", "from m import add; assert add(2, 3) == 5; assert add(-2, 3) == 1"])
        .current_dir(dir.path()).output().unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let session = thread(&edit);
    req.resume = Some(session.clone());
    req.prompt = "Without using tools, reply with the marker you read in the previous turn.".into();
    let resumed = turn(req.clone(), &provider, &env).await;
    assert_eq!(thread(&resumed), session);
    assert!(text(&resumed).contains("TOKAGE_RX_EXEC_42"));
    req.resume = None;
    req.role = Some(Role::Reviewer);
    req.mode = TurnMode::Plan;
    req.prompt = "Read m.py and review the addition fix. Do not modify files. Reply with TOKAGE_RX_REVIEW_OK if correct.".into();
    let review = turn(req, &provider, &env).await;
    assert!(text(&review).contains("TOKAGE_RX_REVIEW_OK"));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), fixed);
    assert_eq!(std::fs::read_to_string(dir.path().join("marker.txt")).unwrap(), "TOKAGE_RX_EXEC_42");
    assert!(crate::session::read_native_sessions(ProviderId::Reasonix, Some(dir.path())).iter().any(|s| s.id == session));
    println!("PASS: Reasonix Planner → edit + actual Python assertions → resume → read-only Reviewer");
}
#[tokio::test]
#[ignore = "uses configured DeepSeek Flash API key and consumes tokens"]
async fn deepseek_turn_resume_copy() {
    let env = crate::env::capture().await;
    let initial = crate::providers::detect_one(&env, ProviderId::Reasonix, None).await.unwrap();
    assert!(matches!(initial.status, ProviderStatus::UnknownAuth | ProviderStatus::Ready));
    crate::providers::verify_reasonix_key(std::path::Path::new(initial.path.as_deref().unwrap()), &env).await.unwrap();
    let provider = crate::providers::detect_one(&env, ProviderId::Reasonix, None).await.unwrap();
    assert_eq!(provider.status, ProviderStatus::Ready);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("marker.txt"), "TOKAGE_RX_42").unwrap();
    let mut req = testing::request(ProviderId::Reasonix);
    req.cwd = dir.path().to_string_lossy().into_owned();
    req.model = "deepseek/deepseek-flash".into();
    req.trusted = vec![ProviderId::Reasonix];
    req.role = Some(Role::Planner);
    req.mode = TurnMode::Plan;
    req.idle_timeout_secs = Some(60);
    req.hard_timeout_secs = Some(120);
    req.prompt = "Read marker.txt with a tool. Reply with its exact content. Do not modify any file.".into();
    let events = turn(req.clone(), &provider, &env).await;
    assert!(text(&events).contains("TOKAGE_RX_42"), "{}", text(&events));
    assert!(events.iter().any(|e| matches!(e, EventKind::ToolStart { .. })));
    let original = thread(&events);
    req.resume = Some(original.clone());
    req.prompt = "Reply exactly with the verification marker you read in the previous turn. Do not use tools.".into();
    let resumed = turn(req.clone(), &provider, &env).await;
    assert!(text(&resumed).contains("TOKAGE_RX_42"), "{}", text(&resumed));
    assert_eq!(thread(&resumed), original);
    req.fork = true;
    let copied = turn(req, &provider, &env).await;
    assert!(text(&copied).contains("TOKAGE_RX_42"), "{}", text(&copied));
    assert_ne!(thread(&copied), original);
    assert_eq!(std::fs::read_to_string(dir.path().join("marker.txt")).unwrap(), "TOKAGE_RX_42");
    let sessions = crate::session::read_native_sessions(ProviderId::Reasonix, Some(dir.path()));
    assert!(sessions.iter().any(|s| s.id == original));
}

#[tokio::test]
#[ignore = "queries the configured DeepSeek wallet via the official CLI status command"]
async fn status_balance_without_model_turn() {
    let env = crate::env::capture().await;
    let provider = crate::providers::detect_one(&env, ProviderId::Reasonix, None).await.unwrap();
    let consent_dir = tempfile::tempdir().unwrap();
    let permission = crate::online_usage::OnlineUsagePermission::open(&consent_dir.path().join("consent"));
    permission.set(true).unwrap();
    let result = quota::query_with_permit(&permission.permit().unwrap(), ProviderId::Reasonix,
        std::path::Path::new(provider.path.as_deref().unwrap()), &env, None).await;
    let balance = result.as_ref().ok().and_then(|r| r.balance.clone());
    assert!(balance.is_some(), "no wallet balance from authenticated query");
    println!("Reasonix balance: {balance:?}");
}

#[tokio::test]
#[ignore = "uses configured DeepSeek default model to verify and persist authentication"]
async fn verification_survives_process_restart() {
    let env = crate::env::capture().await;
    let provider = crate::providers::detect_one(&env, ProviderId::Reasonix, None).await.unwrap();
    crate::providers::verify_reasonix_key(std::path::Path::new(provider.path.as_deref().unwrap()), &env).await.unwrap();
    let output = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "chat::reasonix_live::persisted_auth_probe", "--ignored", "--nocapture"])
        .output().await.unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stdout));
    println!("PASS: fresh process detects Reasonix READY without another model request");
}
#[tokio::test]
#[ignore = "reads installed Reasonix persisted authentication, no model request"]
async fn persisted_auth_probe() {
    let env = crate::env::capture().await;
    let provider = crate::providers::detect_one(&env, ProviderId::Reasonix, None).await.unwrap();
    assert_eq!(provider.status, ProviderStatus::Ready);
}
