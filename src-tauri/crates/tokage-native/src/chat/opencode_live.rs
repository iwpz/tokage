//! Opt-in genuine OpenCode invocation; fixed DeepSeek, never silently substitutes a model.
use super::*;
#[tokio::test]
#[ignore = "uses installed OpenCode and configured DeepSeek API"]
async fn deepseek_chat() {
    let mut env = crate::env::capture().await;
    if let Ok(data) = std::env::var("TOKAGE_OPENCODE_TEST_DATA") { env.vars.insert("XDG_DATA_HOME".into(), data); }
    let custom = std::env::var("TOKAGE_OPENCODE_TEST_BIN").ok();
    let provider = crate::providers::detect_one(&env, ProviderId::Opencode, custom.as_deref())
        .await
        .unwrap();
    assert_eq!(
        provider.status,
        ProviderStatus::Ready,
        "{:?}",
        provider.status
    );
    println!(
        "OpenCode {:?}, credential configuration recognized",
        provider.version
    );
    let dir = tempfile::tempdir().unwrap();
    let mut req = testing::request(ProviderId::Opencode);
    req.cwd = dir.path().display().to_string();
    req.trusted = vec![ProviderId::Opencode];
    req.model = "deepseek/deepseek-flash".into();
    req.role = Some(Role::Planner);
    req.prompt = "Reply exactly TOKAGE_OPENCODE_CHAT_OK. Do not run tools.".into();
    req.idle_timeout_secs = Some(90);
    req.hard_timeout_secs = Some(120);
    let bus = Bus::new();
    let mut rx = bus.subscribe();
    run_turn(
        &bus,
        req.clone(),
        vec![provider.clone()],
        env.clone(),
        None,
        None,
    )
    .await;
    let mut success = false;
    let mut usage = false;
    let mut text = String::new();
    let mut failure = None;
    while let Ok(e) = rx.try_recv() {
        match e {
            DomainEvent::TurnSucceeded { .. } => success = true,
            DomainEvent::TurnFailed { message, .. } => failure = Some(message),
            DomainEvent::ChatOutput {
                event:
                    EventKind::Done {
                        input_tokens,
                        output_tokens,
                        ..
                    },
                ..
            } => usage = input_tokens + output_tokens > 0,
            DomainEvent::ChatOutput {
                event: EventKind::Text { text: t },
                ..
            } => text.push_str(&t),
            _ => {}
        }
    }
    assert!(
        success && usage && text.contains("TOKAGE_OPENCODE_CHAT_OK"),
        "failure={failure:?}; success={success}, usage={usage}, reply={text}"
    );
    std::fs::write(dir.path().join("m.py"), "def add(a,b):\n    return a-b\n").unwrap();
    req.role = Some(Role::Executor);
    req.turn_id = "opencode-edit".into();
    req.prompt = "Read m.py and fix add(a,b) so it returns their sum. Use file read/edit tools, not shell. Remember TOKAGE_OC_MEMORY_91. Reply TOKAGE_OPENCODE_EDIT_OK after editing.".into();
    req.write_scope = vec!["m.py".into()];
    let edited = checked_turn(req.clone(), &provider, &env, "TOKAGE_OPENCODE_EDIT_OK").await;
    assert!(
        edited
            .iter()
            .any(|e| matches!(e, EventKind::FileChange { .. })),
        "missing file change"
    );
    let python = std::process::Command::new("python3")
        .args([
            "-c",
            "from m import add; assert add(2,3)==5; assert add(-2,3)==1",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(python.status.success());
    req.resume = edited.iter().find_map(|e| {
        if let EventKind::Thread { id } = e {
            Some(id.clone())
        } else {
            None
        }
    });
    assert!(req.resume.is_some());
    req.turn_id = "opencode-resume".into();
    req.prompt =
        "What marker did I ask you to remember? Reply with the marker only, do not use tools."
            .into();
    checked_turn(req.clone(), &provider, &env, "TOKAGE_OC_MEMORY_91").await;
    req.resume = None;
    req.role = Some(Role::Reviewer);
    req.turn_id = "opencode-review".into();
    req.prompt = "Read m.py and review whether add returns the sum. If correct reply TOKAGE_OPENCODE_REVIEW_OK. Do not modify files.".into();
    let before = std::fs::read(dir.path().join("m.py")).unwrap();
    checked_turn(req, &provider, &env, "TOKAGE_OPENCODE_REVIEW_OK").await;
    assert_eq!(std::fs::read(dir.path().join("m.py")).unwrap(), before);
    println!("PASS real chat, edit + independent Python gate, resume, read-only review");
}

async fn checked_turn(
    req: ChatRequest,
    provider: &ProviderInfo,
    env: &ShellEnv,
    marker: &str,
) -> Vec<EventKind> {
    let bus = Bus::new();
    let mut rx = bus.subscribe();
    run_turn(&bus, req, vec![provider.clone()], env.clone(), None, None).await;
    let mut success = false;
    let mut events = vec![];
    let mut text = String::new();
    let mut failure = None;
    while let Ok(e) = rx.try_recv() {
        match e {
            DomainEvent::TurnSucceeded { .. } => success = true,
            DomainEvent::TurnFailed { message, .. } => failure = Some(message),
            DomainEvent::ChatOutput { event, .. } => {
                if let EventKind::Text { text: t } = &event {
                    text.push_str(t);
                }
                events.push(event);
            }
            _ => {}
        }
    }
    assert!(
        success && text.contains(marker),
        "failure={failure:?}, reply={text}"
    );
    assert!(events.iter().any(|e| matches!(e,EventKind::Done {input_tokens,output_tokens,..} if input_tokens+output_tokens>0)));
    events
}
