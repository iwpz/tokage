//! Opt-in Cursor subscription acceptance. Every model request is explicitly `auto`.
//! Run `scripts/run-live-cursor-auto.sh` for subscription acceptance.
use super::*;

async fn turn(req: ChatRequest, provider: &ProviderInfo, env: &ShellEnv) -> Vec<EventKind> {
    assert_eq!(req.provider, ProviderId::CursorCli);
    assert_eq!(
        req.model, "auto",
        "live acceptance must only use Cursor auto"
    );
    let bus = Bus::new();
    let mut rx = bus.subscribe();
    let cancelled = run_turn(&bus, req, vec![provider.clone()], env.clone(), None, None).await;
    assert!(!cancelled, "unexpected cancellation");
    let mut events = vec![];
    let mut succeeded = false;
    while let Ok(event) = rx.try_recv() {
        match event {
            DomainEvent::ChatOutput { event, .. } => events.push(event),
            DomainEvent::TurnSucceeded { .. } => succeeded = true,
            DomainEvent::TurnFailed { message, .. } => panic!("Cursor turn failed: {message}"),
            _ => {}
        }
    }
    assert!(
        succeeded,
        "Cursor did not publish TurnSucceeded: {events:?}"
    );
    assert!(
        events.iter().any(|e| matches!(e, EventKind::Done { .. })),
        "no Done event"
    );
    assert!(
        !events.iter().any(|e| matches!(e, EventKind::Error { .. })),
        "error event: {events:?}"
    );
    assert!(!text(&events).trim().is_empty(), "no actual response");
    for event in &events {
        if let EventKind::Done {
            input_tokens,
            output_tokens,
            cache_read_tokens,
            api_calls,
            ..
        } = event
        {
            println!("usage: input={input_tokens} output={output_tokens} cache_read={cache_read_tokens} calls={api_calls:?}");
            assert!(
                *input_tokens + *output_tokens + *cache_read_tokens > 0,
                "missing actual usage"
            );
        }
    }
    events
}

fn text(events: &[EventKind]) -> String {
    events
        .iter()
        .filter_map(|e| match e {
            EventKind::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}
fn thread(events: &[EventKind]) -> String {
    events
        .iter()
        .find_map(|e| match e {
            EventKind::Thread { id } => Some(id.clone()),
            _ => None,
        })
        .expect("missing native session id")
}

#[tokio::test]
#[ignore = "uses installed Cursor CLI subscription and auto model"]
async fn auto_plan_execute_resume_review() {
    let mut env = crate::env::capture().await;
    // Never route this subscription test through a user gateway or API-key backend.
    env.vars.remove("CURSOR_API_KEY");
    env.vars.remove("CURSOR_API_ENDPOINT");
    let provider = crate::providers::detect_one(&env, ProviderId::CursorCli, None)
        .await
        .expect("Cursor registry");
    assert!(
        matches!(provider.status, ProviderStatus::Ready),
        "Cursor is not READY: {:?}",
        provider.status
    );
    assert!(
        provider.models.iter().any(|m| m.id == "auto"),
        "auto unavailable"
    );
    assert!(provider.account.as_ref().is_some_and(|a| a.official && matches!(a.billing, crate::usage::Billing::Subscription)), "subscription channel required");
    let dir = tempfile::tempdir().unwrap();
    let original = "def add(a, b):\n    return a - b\n";
    std::fs::write(dir.path().join("m.py"), original).unwrap();
    let mut req = testing::request(ProviderId::CursorCli);
    req.cwd = dir.path().to_string_lossy().into_owned();
    req.model = "auto".into();
    req.trusted = vec![ProviderId::CursorCli];
    req.timeout_tier = Some(TimeoutTier::Agent);
    req.idle_timeout_secs = Some(120);
    req.hard_timeout_secs = Some(240);
    req.turn_id = "cursor-auto-plan".into();
    req.mode = TurnMode::Plan;
    req.role = Some(Role::Planner);
    req.prompt = "Read m.py. Do not edit any file. Explain the bug in one sentence and remember the verification marker TOKAGE_CURSOR_42 for our next turn.".into();
    let plan = turn(req.clone(), &provider, &env).await;
    assert_eq!(
        std::fs::read_to_string(dir.path().join("m.py")).unwrap(),
        original
    );
    println!("PASS planner: actual text, usage, no edits");

    req.turn_id = "cursor-auto-execute".into();
    req.mode = TurnMode::Direct;
    req.role = Some(Role::Executor);
    // Same as the UI: mode changes start a fresh CLI session and replay history.
    req.resume = None;
    req.history = vec![HistoryItem { role: "user".into(), text: req.prompt.clone() },
        HistoryItem { role: "assistant".into(), text: text(&plan) }];
    req.write_scope = vec!["m.py".into()];
    req.prompt = "Fix only m.py so add(a, b) returns a + b. Use your file edit tool. Do not run shell commands or create other files. Include the verification marker I asked you to remember in your final one-sentence reply.".into();
    let execute = turn(req.clone(), &provider, &env).await;
    assert!(
        text(&execute).contains("TOKAGE_CURSOR_42"),
        "session memory was not resumed: {}",
        text(&execute)
    );
    let edited = std::fs::read_to_string(dir.path().join("m.py")).unwrap();
    assert!(edited.contains("return a + b"), "file not fixed: {edited}");
    assert_ne!(thread(&execute), thread(&plan), "mode switch reused a read-only session");
    let native = crate::session::read_native_sessions(ProviderId::CursorCli, Some(dir.path()));
    assert!(
        native.iter().any(|s| s.id == thread(&execute)),
        "native session not readable"
    );
    assert_eq!(
        adapter::adapter(ProviderId::CursorCli)
            .unwrap()
            .terminal_resume(&thread(&execute))
            .unwrap(),
        ["cursor-agent", "--resume", &thread(&execute)]
    );
    let test = tokio::process::Command::new("python3")
        .args([
            "-c",
            "from m import add; assert add(2, 3) == 5; assert add(-2, 3) == 1",
        ])
        .current_dir(dir.path())
        .output()
        .await
        .unwrap();
    assert!(test.status.success(), "local acceptance failed");
    println!("PASS executor: fresh agent session, replayed history, actual edit, local acceptance");

    req.turn_id = "cursor-auto-execute-resume".into();
    req.resume = Some(thread(&execute));
    req.history.clear();
    req.prompt = "Do not change files or run commands. Reply with the verification marker you remembered in the previous turn.".into();
    let resumed = turn(req.clone(), &provider, &env).await;
    assert_eq!(thread(&resumed), thread(&execute));
    assert!(text(&resumed).contains("TOKAGE_CURSOR_42"), "same-mode resume lost memory: {}", text(&resumed));
    assert_eq!(std::fs::read_to_string(dir.path().join("m.py")).unwrap(), edited);
    println!("PASS same-mode resume: same native session and remembered marker");

    req.turn_id = "cursor-auto-review".into();
    req.mode = TurnMode::Plan;
    req.role = Some(Role::Reviewer);
    req.resume = None;
    req.write_scope.clear();
    req.prompt = "Read m.py and verify the fix. Do not edit files or run shell commands. If add(2,3) is now 5, reply with TOKAGE_CURSOR_REVIEW_PASS.".into();
    let review = turn(req, &provider, &env).await;
    assert!(
        text(&review).contains("TOKAGE_CURSOR_REVIEW_PASS"),
        "review did not pass: {}",
        text(&review)
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("m.py")).unwrap(),
        edited
    );
    println!("PASS reviewer: read-only review and actual completion");
}

#[tokio::test]
#[ignore = "queries installed Cursor auth/catalog; sends no model prompt"]
async fn invalid_key_cannot_borrow_subscription_login() {
    let mut env = crate::env::capture().await;
    env.vars.remove("CURSOR_API_ENDPOINT");
    env.vars
        .insert("CURSOR_API_KEY".into(), "tokage-invalid-key".into());
    let p = crate::providers::detect_one(&env, ProviderId::CursorCli, None)
        .await
        .unwrap();
    assert_eq!(
        p.status,
        ProviderStatus::NeedsLogin,
        "invalid key inherited subscription login"
    );
    assert!(p.account.is_none() && p.models.is_empty());
    println!("PASS invalid API key: NEEDS_LOGIN, no stale subscription account/model catalog");
}

#[cfg(unix)]
#[tokio::test]
async fn cancellation_stops_cursor_process_without_done() {
    use super::adapter::ProviderAdapter;
    use super::fake::{FakeAdapter, Step};
    let dir = tempfile::tempdir().unwrap();
    let env = ShellEnv {
        vars: [
            ("HOME".into(), dir.path().display().to_string()),
            ("PATH".into(), "/usr/bin:/bin".into()),
        ]
        .into(),
    };
    let mut req = testing::request(ProviderId::CursorCli);
    req.cwd = dir.path().display().to_string();
    req.model = "auto".into();
    req.mode = TurnMode::Plan;
    req.timeout_tier = Some(TimeoutTier::Agent);
    req.hard_timeout_secs = Some(5);
    let fake = FakeAdapter::new(
        ProviderId::CursorCli,
        vec![
            Step::Emit(serde_json::json!({"fake":"thread","id":"cursor-cancel"})),
            Step::DelayMs(2000),
            Step::Emit(serde_json::json!({"fake":"done"})),
        ],
    );
    let events = std::sync::Mutex::new(vec![]);
    let emit = |e| events.lock().unwrap().push(e);
    let (tx, rx) = tokio::sync::watch::channel(false);
    let cancel = async {
        tokio::time::sleep(Duration::from_millis(150)).await;
        tx.send(true).unwrap();
    };
    let exe = FakeAdapter::exe();
    let run = run_cli(
        &exe,
        &req,
        fake.launch(&req, "".into()),
        &env,
        None,
        None,
        &emit,
        Some(rx),
    );
    let (result, _) = tokio::join!(run, cancel);
    assert_eq!(result.unwrap_err(), TURN_CANCELLED);
    assert!(!events
        .lock()
        .unwrap()
        .iter()
        .any(|e| matches!(e, EventKind::Done { .. })));
}

#[cfg(unix)]
#[tokio::test]
async fn cursor_idle_timeout_is_not_success() {
    use super::fake::{run_fake_adapter, FakeAdapter, Step};
    let dir = tempfile::tempdir().unwrap();
    let env = ShellEnv {
        vars: [
            ("HOME".into(), dir.path().display().to_string()),
            ("PATH".into(), "/usr/bin:/bin".into()),
        ]
        .into(),
    };
    let mut req = testing::request(ProviderId::CursorCli);
    req.cwd = dir.path().display().to_string();
    req.model = "auto".into();
    req.mode = TurnMode::Plan;
    req.timeout_tier = Some(TimeoutTier::Agent);
    req.idle_timeout_secs = Some(1);
    req.hard_timeout_secs = Some(5);
    let fake = FakeAdapter::new(
        ProviderId::CursorCli,
        vec![
            Step::DelayMs(2000),
            Step::Emit(serde_json::json!({"fake":"done"})),
        ],
    );
    let result = run_fake_adapter(&fake, &env, &req).await;
    assert!(result.result.unwrap_err().contains("idle timeout"));
    assert!(!result
        .events
        .iter()
        .any(|e| matches!(e, EventKind::Done { .. })));
}
