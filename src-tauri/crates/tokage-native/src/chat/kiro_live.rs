//! Explicit real-model acceptance, never part of automatic discovery or normal tests.
use super::*;

#[tokio::test]
#[ignore = "uses installed Kiro login and a small amount of credits"]
async fn haiku_roles() {
    let mut env = crate::env::capture().await;
    if let Ok(engine) = std::env::var("TOKAGE_KIRO_ENGINE") {
        env.vars.insert("TOKAGE_KIRO_ENGINE".into(), engine);
    }
    let provider = crate::providers::detect_one(&env, ProviderId::KiroCli, None)
        .await
        .unwrap();
    assert_eq!(provider.status, ProviderStatus::Ready);
    let dir = tempfile::tempdir().unwrap();
    let mut req = testing::request(ProviderId::KiroCli);
    req.cwd = dir.path().display().to_string();
    req.trusted = vec![ProviderId::KiroCli];
    req.model = "claude-haiku-4.5".into();
    req.role = Some(Role::Planner);
    req.timeout_tier = Some(TimeoutTier::Agent);
    req.idle_timeout_secs = Some(90);
    req.hard_timeout_secs = Some(150);
    req.prompt = "Reply TOKAGE_KIRO_CHAT_OK without tools.".into();
    checked(req.clone(), &provider, &env, "TOKAGE_KIRO_CHAT_OK").await;
    std::fs::write(dir.path().join("m.py"), "def add(a,b):\n    return a-b\n").unwrap();
    req.role = Some(Role::Executor);
    req.turn_id = "kiro-edit".into();
    req.write_scope = vec!["m.py".into()];
    req.prompt = "Read m.py and fix add(a,b) to return their sum using file tools only. Remember TOKAGE_KIRO_MEMORY_81. Reply TOKAGE_KIRO_EDIT_OK after editing.".into();
    let edited = checked(req.clone(), &provider, &env, "TOKAGE_KIRO_EDIT_OK").await;
    assert!(edited
        .iter()
        .any(|e| matches!(e, EventKind::FileChange { .. })));
    let gate = std::process::Command::new("python3")
        .args([
            "-c",
            "from m import add; assert add(2,3)==5; assert add(-2,3)==1",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(gate.status.success());
    req.resume = edited.iter().find_map(|e| {
        if let EventKind::Thread { id } = e {
            Some(id.clone())
        } else {
            None
        }
    });
    assert!(req.resume.is_some());
    req.turn_id = "kiro-resume".into();
    req.prompt = "What marker did I ask you to remember? Reply that marker only. No tools.".into();
    checked(req.clone(), &provider, &env, "TOKAGE_KIRO_MEMORY_81").await;
    let native = crate::session::read_native_sessions(ProviderId::KiroCli, Some(dir.path()));
    assert!(
        native.iter().any(|s| Some(&s.id) == req.resume.as_ref()),
        "native session missing"
    );
    req.resume = None;
    req.role = Some(Role::Reviewer);
    req.turn_id = "kiro-review".into();
    req.prompt = "Read m.py and review whether add returns the sum. If correct reply TOKAGE_KIRO_REVIEW_OK. Do not modify files.".into();
    let before = std::fs::read(dir.path().join("m.py")).unwrap();
    checked(req, &provider, &env, "TOKAGE_KIRO_REVIEW_OK").await;
    assert_eq!(std::fs::read(dir.path().join("m.py")).unwrap(), before);
    println!("PASS Kiro real chat, file edit, Python gate, resume, read-only review");
}

async fn checked(
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
        "failure={failure:?}, reply={text}, tools={:?}",
        events
            .iter()
            .filter(|e| matches!(e, EventKind::ToolEnd { .. }))
            .collect::<Vec<_>>()
    );
    assert!(events.iter().any(|e| matches!(e,EventKind::Done{credits:Some(c),cost_usd:None,input_tokens:0,output_tokens:0,..} if *c>=0.0)),"missing distinct credits");
    events
}
