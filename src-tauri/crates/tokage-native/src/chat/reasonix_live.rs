//! Opt-in acceptance against the locally configured DeepSeek connection.
use super::*;

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
    let result = quota::execute_slash_quota(ProviderId::Reasonix, std::path::Path::new(provider.path.as_deref().unwrap()), &env, &std::env::temp_dir(), "/status").await;
    let balance = crate::usage::get_cached_balance(ProviderId::Reasonix);
    assert!(balance.is_some(), "no wallet balance: {result:?}");
    println!("Reasonix balance: {balance:?}");
}
