//! Opt-in real Aider acceptance. Fixed configured DeepSeek backend; no fallback.
use super::*;

async fn turn(req: ChatRequest, provider: &ProviderInfo, env: &ShellEnv) -> Vec<EventKind> {
    let bus = Bus::new();
    let mut rx = bus.subscribe();
    assert!(!run_turn(&bus, req, vec![provider.clone()], env.clone(), None, None).await);
    let mut events = vec![];
    let mut success = false;
    while let Ok(event) = rx.try_recv() {
        match event {
            DomainEvent::ChatOutput { event, .. } => events.push(event),
            DomainEvent::TurnSucceeded { .. } => success = true,
            DomainEvent::TurnFailed { message, .. } => {
                panic!("Aider failed: {message}; events={events:?}")
            }
            _ => {}
        }
    }
    assert!(success, "{events:?}");
    assert!(
        !events.iter().any(|e| matches!(e, EventKind::Error { .. })),
        "{events:?}"
    );
    assert!(events.iter().any(|e| matches!(e, EventKind::Done { input_tokens, output_tokens, .. } if input_tokens + output_tokens > 0)), "missing actual usage: {events:?}");
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
#[tokio::test]
#[ignore = "uses configured DeepSeek API key and real Aider model requests"]
async fn deepseek_chat_edit_resume_review() {
    let env = crate::env::capture().await;
    let provider = crate::providers::detect_one(&env, ProviderId::Aider, None)
        .await
        .unwrap();
    assert_eq!(provider.status, ProviderStatus::Ready);
    println!(
        "Aider {:?}; fixed deepseek/deepseek-flash",
        provider.version
    );
    let dir = tempfile::tempdir().unwrap();
    let mut req = testing::request(ProviderId::Aider);
    req.cwd = dir.path().to_string_lossy().into_owned();
    req.trusted = vec![ProviderId::Aider];
    req.model = "deepseek/deepseek-flash".into();
    req.role = Some(Role::Executor);
    req.timeout_tier = Some(TimeoutTier::Agent);
    req.idle_timeout_secs = Some(300);
    req.hard_timeout_secs = Some(420);
    req.prompt =
        "Reply with exactly TOKAGE_AIDER_CHAT_OK. Do not modify files or run commands.".into();
    let events = turn(req.clone(), &provider, &env).await;
    assert!(text(&events).contains("TOKAGE_AIDER_CHAT_OK"), "{events:?}");
    println!("PASS actual Aider chat and nonzero usage");
    let id = events
        .iter()
        .find_map(|event| match event {
            EventKind::Thread { id } => Some(id.clone()),
            _ => None,
        })
        .unwrap();
    std::fs::write(dir.path().join("m.py"), "def add(a,b):\n    return a-b\n").unwrap();
    req.turn_id = "aider-plan".into();
    req.role = Some(Role::Planner);
    req.mode = TurnMode::Plan;
    req.prompt = "Read m.py, explain the addition bug and remember marker TOKAGE_AIDER_42. Do not edit files or suggest commands.".into();
    let planned = turn(req.clone(), &provider, &env).await;
    assert!(text(&planned).contains("TOKAGE_AIDER_42"));
    assert!(std::fs::read_to_string(dir.path().join("m.py"))
        .unwrap()
        .contains("a-b"));
    req.turn_id = "aider-edit".into();
    req.role = Some(Role::Executor);
    req.mode = TurnMode::Direct;
    req.write_scope = vec!["m.py".into()];
    req.prompt = "Fix only m.py so add returns a+b. Do not run or suggest shell commands. In the final task report state the edit and remember marker TOKAGE_AIDER_EXEC_42.".into();
    let edited = turn(req.clone(), &provider, &env).await;
    assert!(edited
        .iter()
        .any(|e| matches!(e, EventKind::FileChange { path, .. } if path == "m.py")));
    let python = tokio::process::Command::new("python3")
        .args([
            "-c",
            "from m import add; assert add(2,3)==5; assert add(-2,3)==1",
        ])
        .current_dir(dir.path())
        .output()
        .await
        .unwrap();
    assert!(python.status.success());
    let edit_id = edited
        .iter()
        .find_map(|event| match event {
            EventKind::Thread { id } => Some(id.clone()),
            _ => None,
        })
        .unwrap();
    assert_ne!(id, edit_id);
    req.resume = Some(edit_id.clone());
    req.turn_id = "aider-resume".into();
    req.prompt = "Without making edits, reply with the marker from our previous edit turn.".into();
    let resumed = turn(req.clone(), &provider, &env).await;
    assert!(resumed
        .iter()
        .any(|e| matches!(e, EventKind::Thread { id } if id == &edit_id)));
    assert!(text(&resumed).contains("TOKAGE_AIDER_EXEC_42"));
    req.resume = None;
    req.turn_id = "aider-isolated".into();
    req.prompt = "If an earlier turn of this conversation supplied a marker, repeat it. Otherwise reply ISOLATED. Do not edit files.".into();
    let isolated = turn(req.clone(), &provider, &env).await;
    assert!(text(&isolated).contains("ISOLATED"));
    let fixed = std::fs::read_to_string(dir.path().join("m.py")).unwrap();
    req.turn_id = "aider-review".into();
    req.role = Some(Role::Reviewer);
    req.mode = TurnMode::Plan;
    req.prompt = "Read m.py and review add. Do not edit any file or suggest commands. Reply AIDER_REVIEW_OK if addition is correct.".into();
    let reviewed = turn(req, &provider, &env).await;
    assert!(text(&reviewed).contains("AIDER_REVIEW_OK"));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("m.py")).unwrap(),
        fixed
    );
    println!("PASS Aider: real chat, planner, actual edit/Python assertions, isolated history/resume, read-only reviewer and regex usage");
}
