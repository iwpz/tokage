//! Opt-in installed Copilot acceptance. Every model turn uses Auto + efficiency.
use super::*;

async fn turn(req: ChatRequest, provider: &ProviderInfo, env: &ShellEnv) -> Vec<EventKind> {
    assert_eq!(req.model, "auto");
    assert_eq!(req.tier.as_deref(), Some("efficiency"));
    let bus = Bus::new();
    let mut rx = bus.subscribe();
    assert!(!run_turn(&bus, req, vec![provider.clone()], env.clone(), None, None).await);
    let mut events = vec![];
    let mut succeeded = false;
    while let Ok(event) = rx.try_recv() {
        match event {
            DomainEvent::ChatOutput { event, .. } => events.push(event),
            DomainEvent::TurnSucceeded { .. } => succeeded = true,
            DomainEvent::TurnFailed { message, .. } => panic!("Copilot failed: {message}"),
            _ => {}
        }
    }
    assert!(succeeded, "{events:?}");
    assert!(events.iter().any(|e| matches!(e, EventKind::Done { premium_requests: Some(_), .. })), "missing billed usage");
    assert!(!events.iter().any(|e| matches!(e, EventKind::Error { .. })), "{events:?}");
    println!("PASS turn: model={:?}", events.iter().find_map(|e| match e { EventKind::Model { name } => Some(name), _ => None }));
    events
}
fn text(events: &[EventKind]) -> String {
    events.iter().filter_map(|e| match e { EventKind::Text { text } => Some(text.as_str()), _ => None }).collect()
}
fn thread(events: &[EventKind]) -> String {
    events.iter().find_map(|e| match e { EventKind::Thread { id } => Some(id.clone()), _ => None }).expect("native session")
}
#[tokio::test]
#[ignore = "uses logged-in Copilot subscription and Auto efficiency AI credits"]
async fn auto_efficiency_plan_edit_resume_review() {
    let env = crate::env::capture().await;
    let initial = crate::providers::detect_one(&env, ProviderId::CopilotCli, None).await.unwrap();
    assert!(matches!(initial.status, ProviderStatus::UnknownAuth | ProviderStatus::Ready), "{:?}", initial.status);
    crate::providers::verify_copilot_auth(std::path::Path::new(initial.path.as_deref().unwrap()), &env).await.unwrap();
    let provider = crate::providers::detect_one(&env, ProviderId::CopilotCli, None).await.unwrap();
    assert_eq!(provider.status, ProviderStatus::Ready);
    let dir = tempfile::tempdir().unwrap();
    let original = "def add(a, b):\n    return a - b\n";
    std::fs::write(dir.path().join("m.py"), original).unwrap();
    let mut req = testing::request(ProviderId::CopilotCli);
    req.cwd = dir.path().to_string_lossy().into_owned();
    req.model = "auto".into();
    req.tier = Some("efficiency".into());
    req.trusted = vec![ProviderId::CopilotCli];
    req.idle_timeout_secs = Some(90);
    req.hard_timeout_secs = Some(180);
    req.role = Some(Role::Planner);
    req.mode = TurnMode::Plan;
    req.prompt = "Read m.py using view. Explain the bug in one sentence. Do not edit files or run shell commands. Remember the marker TOKAGE_CP_42.".into();
    let plan = turn(req.clone(), &provider, &env).await;
    assert!(plan.iter().any(|e| matches!(e, EventKind::ToolStart { tool: ToolKind::Read, .. })), "no file read");
    assert_eq!(std::fs::read_to_string(dir.path().join("m.py")).unwrap(), original);
    req.role = Some(Role::Executor);
    req.mode = TurnMode::Direct;
    req.auth_mode = Some(AuthMode::FullAuto);
    req.write_scope = vec!["m.py".into()];
    req.history = vec![HistoryItem { role: "user".into(), text: req.prompt.clone() }, HistoryItem { role: "assistant".into(), text: text(&plan) }];
    req.prompt = "Fix only m.py so add(a, b) returns a + b. Use your file edit tool. Then run python3 -c 'from m import add; assert add(2, 3) == 5; print(\"TOKAGE_CP_TEST_OK\")'. In your final answer include our previous verification marker.".into();
    let edit = turn(req.clone(), &provider, &env).await;
    assert!(std::fs::read_to_string(dir.path().join("m.py")).unwrap().contains("a + b"));
    assert!(edit.iter().any(|e| matches!(e, EventKind::ToolStart { tool: ToolKind::Edit, .. })));
    assert!(edit.iter().any(|e| matches!(e, EventKind::ToolEnd { ok: Some(true), output: Some(o), .. } if o.contains("TOKAGE_CP_TEST_OK"))), "no successful Python validation");
    assert!(text(&edit).contains("TOKAGE_CP_42"), "history marker missing: {}", text(&edit));
    let original_session = thread(&edit);
    req.history.clear();
    req.resume = Some(original_session.clone());
    req.prompt = "Reply with our previous verification marker only. Do not use tools.".into();
    let resumed = turn(req.clone(), &provider, &env).await;
    assert_eq!(thread(&resumed), original_session);
    assert!(text(&resumed).contains("TOKAGE_CP_42"), "{}", text(&resumed));
    req.resume = None;
    req.role = Some(Role::Reviewer);
    req.mode = TurnMode::Plan;
    req.prompt = "Read m.py with view and review add(a, b). Do not modify files or run shell commands. Reply REVIEW_OK if correct.".into();
    let before = std::fs::read_to_string(dir.path().join("m.py")).unwrap();
    let review = turn(req, &provider, &env).await;
    assert!(text(&review).contains("REVIEW_OK"), "{}", text(&review));
    assert_eq!(std::fs::read_to_string(dir.path().join("m.py")).unwrap(), before);
    let sessions = crate::session::read_native_sessions(ProviderId::CopilotCli, Some(dir.path()));
    assert!(sessions.iter().any(|s| s.id == original_session), "session history missing");
}

#[tokio::test]
#[ignore = "queries installed Copilot interactive usage without a model prompt"]
async fn interactive_usage() {
    let env = crate::env::capture().await;
    let provider = crate::providers::detect_one(&env, ProviderId::CopilotCli, None).await.unwrap();
    let cwd = std::env::temp_dir();
    let cwd_arg = cwd.to_string_lossy();
    let snapshot = crate::chat::pty::execute_pty_interactive_after(
        std::path::Path::new(provider.path.as_deref().unwrap()),
        &["--model", "auto", "--auto-tier", "efficiency", "--disable-builtin-mcps", "--add-dir", cwd_arg.as_ref(), "--available-tools="],
        &std::env::temp_dir(), &env.vars.clone().into_iter().collect::<Vec<_>>(),
        Some("/usage\r"), Duration::from_secs(20), crate::chat::pty::InteractiveOptions { input_delay: Duration::from_secs(3), accept_folder_once: true }).await.unwrap();
    println!("Copilot usage screen:\n{snapshot}");
    let quota = quota::parse_slash_quota_output(ProviderId::CopilotCli, &snapshot);
    println!("parsed quota: {quota:?}");
    assert!(snapshot.contains("Session: 0 AIC used"), "usage query unexpectedly spent model credits");
    let quota = quota.unwrap();
    assert_eq!(quota.windows.len(), 1);
    assert_eq!(quota.windows[0].window_minutes, 0);
    assert!((0.0..=100.0).contains(&quota.windows[0].used_percent));
    let queried = quota::execute_slash_quota(ProviderId::CopilotCli, std::path::Path::new(provider.path.as_deref().unwrap()), &env, &cwd, "/usage").await.unwrap();
    assert_eq!(queried.windows[0].window_minutes, 0);
}
