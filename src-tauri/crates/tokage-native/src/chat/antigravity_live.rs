//! Opt-in Antigravity Google account acceptance. Every model request is explicitly Gemini Flash Low.
//! Run `scripts/run-live-antigravity.sh` for subscription acceptance.
use super::*;

async fn turn(req: ChatRequest, provider: &ProviderInfo, env: &ShellEnv) -> Vec<EventKind> {
    assert_eq!(req.provider, ProviderId::AntigravityCli);
    assert_eq!(
        req.model, "gemini-3.8-flash-low",
        "live acceptance must only use Antigravity Gemini Flash Low"
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
            DomainEvent::TurnFailed { message, .. } => panic!("Antigravity turn failed: {message}"),
            _ => {}
        }
    }
    assert!(
        succeeded,
        "Antigravity did not publish TurnSucceeded: {events:?}"
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
#[ignore = "uses installed Antigravity CLI Google account and Gemini Flash Low"]
async fn google_plan_execute_resume_review() {
    let mut env = crate::env::capture().await;
    // Never route this subscription test through a user gateway or API-key backend.
    env.vars.remove("GEMINI_API_KEY");
    env.vars.remove("GOOGLE_API_KEY");
    env.vars.remove("GOOGLE_GEMINI_BASE_URL");
    let provider = crate::providers::detect_one(&env, ProviderId::AntigravityCli, None)
        .await
        .expect("Antigravity registry");
    assert!(
        matches!(provider.status, ProviderStatus::Ready),
        "Antigravity is not READY: {:?}",
        provider.status
    );
    assert!(
        provider.models.iter().any(|m| m.id == "gemini-3.8-flash-low"),
        "Gemini Flash Low unavailable"
    );
    assert!(provider.account.as_ref().is_some_and(|a| a.official), "Google account channel required");
    let dir = tempfile::tempdir().unwrap();
    let original = "def add(a, b):\n    return a - b\n";
    std::fs::write(dir.path().join("m.py"), original).unwrap();
    let mut req = testing::request(ProviderId::AntigravityCli);
    req.cwd = dir.path().to_string_lossy().into_owned();
    req.model = "gemini-3.8-flash-low".into();
    req.trusted = vec![ProviderId::AntigravityCli];
    req.timeout_tier = Some(TimeoutTier::Agent);
    req.idle_timeout_secs = Some(120);
    req.hard_timeout_secs = Some(240);
    req.turn_id = "antigravity-google-plan".into();
    req.mode = TurnMode::Plan;
    req.role = Some(Role::Planner);
    req.prompt = "Read m.py. Do not edit any file. Explain the bug in one sentence and remember the verification marker TOKAGE_AGY_42 for our next turn.".into();
    let plan = turn(req.clone(), &provider, &env).await;
    assert_eq!(
        std::fs::read_to_string(dir.path().join("m.py")).unwrap(),
        original
    );
    println!("PASS planner: actual text, usage, no edits");

    req.turn_id = "antigravity-google-execute".into();
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
        text(&execute).contains("TOKAGE_AGY_42"),
        "session memory was not resumed: {}",
        text(&execute)
    );
    let edited = std::fs::read_to_string(dir.path().join("m.py")).unwrap();
    assert!(edited.contains("return a + b"), "file not fixed: {edited}");
    assert_ne!(thread(&execute), thread(&plan), "mode switch reused a read-only session");
    let native = crate::session::read_native_sessions(ProviderId::AntigravityCli, Some(dir.path()));
    assert!(
        native.iter().any(|s| s.id == thread(&execute)),
        "native session not readable"
    );
    assert_eq!(
        adapter::adapter(ProviderId::AntigravityCli)
            .unwrap()
            .terminal_resume(&thread(&execute))
            .unwrap(),
        ["agy", "--conversation", &thread(&execute)]
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

    req.turn_id = "antigravity-google-execute-resume".into();
    req.resume = Some(thread(&execute));
    req.history.clear();
    req.prompt = "Do not change files or run commands. Reply with the verification marker you remembered in the previous turn.".into();
    let resumed = turn(req.clone(), &provider, &env).await;
    assert_eq!(thread(&resumed), thread(&execute));
    assert!(text(&resumed).contains("TOKAGE_AGY_42"), "same-mode resume lost memory: {}", text(&resumed));
    assert_eq!(std::fs::read_to_string(dir.path().join("m.py")).unwrap(), edited);
    println!("PASS same-mode resume: same native session and remembered marker");

    req.turn_id = "antigravity-google-review".into();
    req.mode = TurnMode::Plan;
    req.role = Some(Role::Reviewer);
    req.resume = None;
    req.write_scope.clear();
    req.prompt = "Read m.py and verify the fix. Do not edit files or run shell commands. If add(2,3) is now 5, reply with TOKAGE_AGY_REVIEW_PASS.".into();
    let review = turn(req, &provider, &env).await;
    assert!(
        text(&review).contains("TOKAGE_AGY_REVIEW_PASS"),
        "review did not pass: {}",
        text(&review)
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("m.py")).unwrap(),
        edited
    );
    println!("PASS reviewer: read-only review and actual completion");
}

#[cfg(unix)]
#[tokio::test]
async fn cancellation_stops_antigravity_process_without_done() {
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
    let mut req = testing::request(ProviderId::AntigravityCli);
    req.cwd = dir.path().display().to_string();
    req.model = "gemini-3.8-flash-low".into();
    req.mode = TurnMode::Plan;
    req.timeout_tier = Some(TimeoutTier::Agent);
    req.hard_timeout_secs = Some(5);
    let fake = FakeAdapter::new(
        ProviderId::AntigravityCli,
        vec![
            Step::Emit(serde_json::json!({"fake":"thread","id":"antigravity-cancel"})),
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
async fn antigravity_idle_timeout_is_not_success() {
    use super::fake::{run_fake_adapter, FakeAdapter, Step};
    let dir = tempfile::tempdir().unwrap();
    let env = ShellEnv {
        vars: [
            ("HOME".into(), dir.path().display().to_string()),
            ("PATH".into(), "/usr/bin:/bin".into()),
        ]
        .into(),
    };
    let mut req = testing::request(ProviderId::AntigravityCli);
    req.cwd = dir.path().display().to_string();
    req.model = "gemini-3.8-flash-low".into();
    req.mode = TurnMode::Plan;
    req.timeout_tier = Some(TimeoutTier::Agent);
    req.idle_timeout_secs = Some(1);
    req.hard_timeout_secs = Some(5);
    let fake = FakeAdapter::new(
        ProviderId::AntigravityCli,
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

#[tokio::test]
#[ignore = "uses installed CLI read-only quota command; no model prompt"]
async fn authenticated_quota() {
    let mut env = crate::env::capture().await;
    for key in ["GEMINI_API_KEY", "GOOGLE_API_KEY", "GOOGLE_GEMINI_BASE_URL"] { env.vars.remove(key); }
    let path = which::which_in("agy", env.path(), std::env::current_dir().unwrap()).expect("agy missing");
    let consent_dir = tempfile::tempdir().unwrap();
    let permission = crate::online_usage::OnlineUsagePermission::open(&consent_dir.path().join("consent"));
    permission.set(true).unwrap();
    let snapshot = super::quota::query_authorized(&permission.permit().unwrap(),
        ProviderId::AntigravityCli, &path, &env, None).await.expect("authenticated quota").expect("quota snapshot");
    assert!(!snapshot.windows.is_empty());
    println!("PASS zero-token authenticated quota: {} windows", snapshot.windows.len());
}
