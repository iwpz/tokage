//! Opt-in Claude / Codex acceptance. Uses the existing subscription and fixed
//! inexpensive models; never logs out, changes credentials, or falls back.
use super::*;

async fn turn(req: ChatRequest, provider: &ProviderInfo, env: &ShellEnv) -> Vec<EventKind> {
    let role = req.effective_role();
    let resumed = req.resume.is_some();
    let bus = Bus::new();
    let mut rx = bus.subscribe();
    assert!(!run_turn(&bus, req, vec![provider.clone()], env.clone(), None, None).await);
    let mut events = vec![];
    let mut succeeded = false;
    while let Ok(event) = rx.try_recv() {
        match event {
            DomainEvent::ChatOutput { event, .. } => events.push(event),
            DomainEvent::TurnSucceeded { .. } => succeeded = true,
            DomainEvent::TurnFailed { message, .. } => panic!("Baseline CLI failed: {message}"),
            _ => {}
        }
    }
    assert!(succeeded);
    assert!(!text(&events).trim().is_empty());
    let errors: Vec<_> = events.iter().filter_map(|e| match e { EventKind::Error { message } => Some(message), _ => None }).collect();
    assert!(errors.is_empty(), "{role:?} resume={resumed}: CLI errors: {errors:?}");
    assert!(events.iter().any(|e| matches!(e, EventKind::Done { input_tokens, output_tokens, cache_read_tokens, .. } if input_tokens + output_tokens + cache_read_tokens > 0)));
    println!("PASS {:?} {role:?} resume={resumed}: actual text and structured usage", provider.id);
    events
}
fn text(events: &[EventKind]) -> String {
    events.iter().filter_map(|e| match e { EventKind::Text { text } => Some(text.as_str()), _ => None }).collect()
}
fn thread(events: &[EventKind]) -> String {
    events.iter().find_map(|e| match e { EventKind::Thread { id } => Some(id.clone()), _ => None }).expect("native session id")
}
async fn subscription_env(id: ProviderId) -> ShellEnv {
    let mut env = crate::env::capture().await;
    if id == ProviderId::ClaudeCode {
        env.vars.retain(|key, _| !key.starts_with("ANTHROPIC_") && ![
            "CLAUDE_CODE_OAUTH_TOKEN", "CLAUDE_CODE_OAUTH_TOKEN_FILE_DESCRIPTOR",
            "CLAUDE_CODE_USE_BEDROCK", "CLAUDE_CODE_USE_VERTEX", "CLAUDE_CODE_USE_FOUNDRY",
        ].contains(&key.as_str()));
    }
    env
}

async fn acceptance(id: ProviderId, model: &str) {
    let env = subscription_env(id).await;
    let provider = crate::providers::detect_one(&env, id, None).await.expect("registered provider");
    assert_eq!(provider.status, ProviderStatus::Ready, "provider must be installed and authenticated; skip is not success");
    if id == ProviderId::ClaudeCode {
        assert!(provider.account.as_ref().is_some_and(|a| a.official && matches!(a.billing, crate::usage::Billing::Subscription)), "Claude subscription channel required");
    }
    println!("LIVE {id:?}: version={:?}, requested model={model}", provider.version);
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("m.py");
    let broken = "def add(a, b):\n    return a - b\n";
    std::fs::write(&file, broken).unwrap();
    std::fs::write(dir.path().join("marker.txt"), "TOKAGE_BASELINE_42").unwrap();
    std::fs::write(dir.path().join(".gitignore"), "__pycache__/\n.tokage/\n").unwrap();
    for args in [vec!["init", "-b", "main"], vec!["config", "user.name", "Tokage acceptance"], vec!["config", "user.email", "acceptance@tokage.invalid"], vec!["config", "commit.gpgsign", "false"], vec!["config", "core.hooksPath", "/dev/null"], vec!["add", "."], vec!["commit", "-m", "acceptance baseline"]] {
        let output = tokio::process::Command::new("git").args(args).current_dir(dir.path()).output().await.unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    }
    let mut req = testing::request(id);
    req.cwd = dir.path().to_string_lossy().into_owned();
    req.model = model.into();
    req.trusted = vec![id];
    req.timeout_tier = Some(TimeoutTier::Agent);
    req.idle_timeout_secs = Some(120);
    req.hard_timeout_secs = Some(240);
    req.role = Some(Role::Planner);
    req.mode = TurnMode::Plan;
    req.prompt = "Read marker.txt and m.py using file tools or read-only shell commands. Remember the marker and explain the addition bug in one sentence. Do not modify any file.".into();
    let plan = turn(req.clone(), &provider, &env).await;
    assert!(text(&plan).contains("TOKAGE_BASELINE_42"));
    assert!(plan.iter().any(|e| matches!(e, EventKind::ToolStart { tool: ToolKind::Read | ToolKind::Command, .. })));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), broken);
    req.role = Some(Role::Executor);
    req.mode = TurnMode::Direct;
    req.auth_mode = Some(AuthMode::AskSensitive);
    req.session_allowed = vec!["python3".into()];
    req.write_scope = vec!["m.py".into()];
    req.history = vec![HistoryItem { role: "user".into(), text: req.prompt.clone() }, HistoryItem { role: "assistant".into(), text: text(&plan) }];
    req.prompt = "Fix only m.py so add(a, b) returns a + b. Then use the approved Python command: python3 -c 'from m import add; assert add(2, 3) == 5; assert add(-2, 3) == 1; print(\"TOKAGE_BASELINE_TEST_OK\")'. Include our previous marker and the actual test output in your final answer.".into();
    let edit = turn(req.clone(), &provider, &env).await;
    let fixed = std::fs::read_to_string(&file).unwrap();
    assert_ne!(fixed, broken);
    assert!(text(&edit).contains("TOKAGE_BASELINE_42"));
    assert!(edit.iter().any(|e| matches!(e, EventKind::ToolEnd { ok: Some(true), output: Some(output), .. } if output.contains("TOKAGE_BASELINE_TEST_OK"))), "no successful actual Python command: {edit:?}");
    let result = tokio::process::Command::new("python3").args(["-c", "from m import add; assert add(2, 3) == 5; assert add(-2, 3) == 1"]).current_dir(dir.path()).output().await.unwrap();
    assert!(result.status.success());
    let original = thread(&edit);
    req.history.clear();
    req.resume = Some(original.clone());
    req.prompt = "Without using tools, reply with the marker from our previous turn.".into();
    let resumed = turn(req.clone(), &provider, &env).await;
    assert_eq!(thread(&resumed), original);
    assert!(text(&resumed).contains("TOKAGE_BASELINE_42"));
    if id == ProviderId::ClaudeCode {
        req.fork = true;
        let fork = turn(req.clone(), &provider, &env).await;
        assert_ne!(thread(&fork), original);
        assert!(text(&fork).contains("TOKAGE_BASELINE_42"));
        req.fork = false;
    }
    req.resume = None;
    req.role = Some(Role::Reviewer);
    req.mode = TurnMode::Plan;
    req.prompt = "Read m.py using file tools or read-only shell commands and review the addition fix. Do not change any file. Include TOKAGE_BASELINE_REVIEW_OK in your reply if correct.".into();
    let review = turn(req, &provider, &env).await;
    assert!(text(&review).contains("TOKAGE_BASELINE_REVIEW_OK"), "actual reviewer response: {}", text(&review));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), fixed);
    assert_eq!(std::fs::read_to_string(dir.path().join("marker.txt")).unwrap(), "TOKAGE_BASELINE_42");
    assert!(crate::session::read_native_sessions(id, Some(dir.path())).iter().any(|s| s.id == original));
    println!("PASS {id:?} {model}: Planner → actual edit + Python assertions → resume{} → read-only Reviewer", if id == ProviderId::ClaudeCode { " / native fork" } else { "" });
}

#[tokio::test]
#[ignore = "uses installed Claude subscription and Haiku tokens"]
async fn claude_haiku_role_chain() { acceptance(ProviderId::ClaudeCode, "haiku").await; }
#[tokio::test]
#[ignore = "uses installed Codex subscription and gpt-6-luna tokens"]
async fn codex_luna_role_chain() { acceptance(ProviderId::CodexCli, "gpt-6-luna").await; }


#[tokio::test]
#[ignore = "queries installed Claude subscription /usage via the production PTY path"]
async fn claude_subscription_usage() {
    let env = subscription_env(ProviderId::ClaudeCode).await;
    let provider = crate::providers::detect_one(&env, ProviderId::ClaudeCode, None).await.unwrap();
    assert_eq!(provider.status, ProviderStatus::Ready);
    let snapshot = super::quota::execute_slash_quota(
        ProviderId::ClaudeCode, std::path::Path::new(provider.path.as_deref().unwrap()),
        &env, &std::env::temp_dir(), "/usage",
    ).await.expect("real Claude /usage must return quota");
    assert!(!snapshot.windows.is_empty());
    assert!(snapshot.windows.iter().all(|w| (0.0..=100.0).contains(&w.used_percent)));
    println!("PASS Claude /usage: {} real quota windows, source={:?}", snapshot.windows.len(), snapshot.source);
}
