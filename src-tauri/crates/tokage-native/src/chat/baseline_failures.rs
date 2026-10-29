//! No-token regressions using real adapter parsers and production run_turn /
//! cancellation paths, with synthetic CLI output and isolated HOME directories.
use super::*;

#[cfg(unix)]
fn fixture(id: ProviderId, script: &str) -> (tempfile::TempDir, ShellEnv, std::path::PathBuf, ChatRequest) {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("cli");
    std::fs::write(&exe, format!("#!/bin/sh\nif [ \"$1\" = run ] && [ \"${{2:-}}\" = --help ]; then echo --standalone; exit 0; fi\n{script}\n")).unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o700)).unwrap();
    let env = ShellEnv { vars: [("HOME".into(), dir.path().to_string_lossy().into_owned()), ("PATH".into(), "/usr/bin:/bin".into())].into() };
    let mut req = testing::request(id);
    req.cwd = dir.path().to_string_lossy().into_owned();
    req.model = if id == ProviderId::ClaudeCode { "haiku" } else { "gpt-6-luna" }.into();
    req.trusted = vec![id];
    req.role = Some(Role::Planner);
    req.mode = TurnMode::Plan;
    req.timeout_tier = Some(TimeoutTier::Agent);
    req.idle_timeout_secs = Some(1);
    req.hard_timeout_secs = Some(3);
    (dir, env, exe, req)
}

#[cfg(unix)]
#[tokio::test]
async fn expired_credentials_revoke_ready_without_success_or_done() {
    for id in [ProviderId::ClaudeCode, ProviderId::CodexCli] {
        let script = if id == ProviderId::ClaudeCode { r#"
if [ "$1" = --version ]; then printf '2.1.296\n'; exit 0; fi
if [ "$1" = auth ]; then printf '%s\n' '{"loggedIn":true,"authMethod":"claude.ai"}'; exit 0; fi
printf '%s\n' '{"type":"result","subtype":"error_during_execution","is_error":true,"result":"Failed to authenticate: OAuth token revoked","session_id":"expired"}'
exit 1
"# } else { r#"
if [ "$1" = --version ]; then printf 'codex-cli 0.162.0\n'; exit 0; fi
if [ "$1" = login ]; then printf 'Logged in using ChatGPT\n'; exit 0; fi
if [ "$1" = debug ]; then printf '{"models":[]}\n'; exit 0; fi
printf '%s\n' '{"id":1,"error":{"code":-32000,"message":"401 Unauthorized: token expired"}}'
exit 1
"# };
        let (_dir, env, exe, req) = fixture(id, script);
        if id == ProviderId::ClaudeCode {
            std::fs::create_dir(_dir.path().join(".claude")).unwrap();
            std::fs::write(_dir.path().join(".claude/.credentials.json"), r#"{"claudeAiOauth":{"accessToken":"synthetic-access","refreshToken":"synthetic-refresh","expiresAt":0}}"#).unwrap();
        }
        let provider = crate::providers::detect_one(&env, id, exe.to_str()).await.unwrap();
        assert_eq!(provider.status, ProviderStatus::Ready);
        let bus = Bus::new(); let mut rx = bus.subscribe();
        assert!(!run_turn(&bus, req, vec![provider], env, None, None).await);
        let mut detected = false; let mut failed = false;
        while let Ok(event) = rx.try_recv() {
            match event {
                DomainEvent::ProviderDetected { status: ProviderStatus::NeedsLogin, .. } => detected = true,
                DomainEvent::TurnFailed { kind: tokage_core::TurnErrorKind::AuthRequired, .. } => failed = true,
                DomainEvent::TurnSucceeded { .. } => panic!("expired credential succeeded"),
                DomainEvent::ChatOutput { event: EventKind::Done { .. }, .. } => panic!("expired credential emitted Done"),
                _ => {},
            }
        }
        assert!(detected && failed, "{id:?}");
    }
}

#[cfg(unix)]
#[tokio::test]
async fn baseline_cancel_and_timeout_never_emit_done() {
    for id in [ProviderId::ClaudeCode, ProviderId::CodexCli] {
        let frame = if id == ProviderId::ClaudeCode { r#"{"type":"system","subtype":"init","session_id":"cancel-test","model":"haiku"}"# } else { r#"{"type":"thread.started","thread_id":"cancel-test"}"# };
        let script = format!("printf '%s\\n' '{frame}'\nexec /bin/sleep 5");
        let (_dir, env, exe, req) = fixture(id, &script);
        for cancel in [true, false] {
            let events = std::sync::Mutex::new(vec![]);
            let emit = |event| events.lock().unwrap().push(event);
            let (tx, rx) = tokio::sync::watch::channel(false);
            let run = run_cli(&exe, &req, launch(&req).unwrap(), &env, None, None, &emit, Some(rx));
            let trigger = async { if cancel { tokio::time::sleep(Duration::from_millis(150)).await; tx.send(true).unwrap(); } };
            let (result, _) = tokio::join!(run, trigger);
            let error = result.unwrap_err();
            assert!(if cancel { error == TURN_CANCELLED } else { error.contains("idle timeout") }, "{error}");
            assert!(!events.lock().unwrap().iter().any(|e| matches!(e, EventKind::Done { .. })));
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn provisional_done_cannot_mask_exit_failure_or_later_error() {
    for id in [ProviderId::ClaudeCode, ProviderId::CodexCli] {
        let done = if id == ProviderId::ClaudeCode { r#"{"type":"result","subtype":"success","is_error":false,"result":"ok","session_id":"test","usage":{"input_tokens":1,"output_tokens":1}}"# } else { r#"{"type":"turn.completed","usage":{"input_tokens":1,"output_tokens":1}}"# };
        let error = if id == ProviderId::ClaudeCode { r#"{"type":"result","subtype":"error_during_execution","is_error":true,"result":"OAuth token revoked"}"# } else { r#"{"type":"turn.failed","error":{"message":"OAuth token revoked"}}"# };
        for (extra, code) in [("".to_string(), 1), (format!("printf '%s\\n' '{error}'"), 0)] {
            let script = format!("printf '%s\\n' '{done}'\n{extra}\nexit {code}");
            let (_dir, env, exe, mut req) = fixture(id, &script);
            if id == ProviderId::CodexCli { req.role = Some(Role::Reviewer); }
            let events = std::sync::Mutex::new(vec![]);
            let emit = |event| events.lock().unwrap().push(event);
            assert!(run_cli(&exe, &req, launch(&req).unwrap(), &env, None, None, &emit, None).await.is_err());
            assert!(!events.lock().unwrap().iter().any(|e| matches!(e, EventKind::Done { .. })));
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn m2d_fault_matrix_real_processes_never_emit_success() {
    use tokage_core::TurnErrorKind;
    for id in [ProviderId::ClaudeCode, ProviderId::Opencode] {
        for (message, expected) in [
            ("network disconnected: connection reset", TurnErrorKind::NonZeroExit),
            ("429 quota exceeded", TurnErrorKind::RateLimited),
            ("402 payment required: insufficient credits", TurnErrorKind::RateLimited),
            ("OAuth token revoked", TurnErrorKind::AuthRequired),
        ] {
            let frame = if id == ProviderId::ClaudeCode {
                serde_json::json!({"type":"result", "subtype":"error_during_execution", "is_error":true, "result":message})
            } else { serde_json::json!({"type":"error", "error":{"message":message}}) };
            let script = format!("printf '%s\\n' '{frame}'\nexit 17");
            let (_dir, env, exe, req) = fixture(id, &script);
            let events = std::sync::Mutex::new(vec![]);
            let emit = |e| events.lock().unwrap().push(e);
            let failure = run_cli(&exe, &req, launch(&req).unwrap(), &env, None, None, &emit, None).await.unwrap_err();
            assert!(failure.contains(message), "{id:?}: {failure}");
            assert_eq!(super::error::classify_error(Some(id), &failure, Some(17)), expected);
            assert!(!events.lock().unwrap().iter().any(|e| matches!(e, EventKind::Done { .. })));
        }
        let (_dir, env, exe, req) = fixture(id, "kill -KILL $$");
        let emit = |e| assert!(!matches!(e, EventKind::Done { .. }));
        assert!(run_cli(&exe, &req, launch(&req).unwrap(), &env, None, None, &emit, None).await.is_err());
    }
}

#[cfg(unix)]
#[tokio::test]
async fn m2d_cancel_is_supervised_during_stdin_and_after_stdout_eof() {
    for phase in ["stdin", "exit"] {
        let script = if phase == "stdin" { "exec /bin/sleep 60" } else { "exec 1>&-\nexec /bin/sleep 60" };
        let (_dir, env, exe, mut req) = fixture(ProviderId::ClaudeCode, script);
        if phase == "stdin" { req.prompt = "x".repeat(2 * 1024 * 1024); }
        req.hard_timeout_secs = Some(30);
        let (tx, rx) = tokio::sync::watch::channel(false);
        let emit = |e| assert!(!matches!(e, EventKind::Done { .. }));
        let run = run_cli(&exe, &req, launch(&req).unwrap(), &env, None, None, &emit, Some(rx));
        let trigger = async { tokio::time::sleep(Duration::from_millis(150)).await; tx.send(true).unwrap(); };
        let (result, _) = tokio::time::timeout(Duration::from_secs(3), async { tokio::join!(run, trigger) }).await.expect(phase);
        assert_eq!(result.unwrap_err(), TURN_CANCELLED, "{phase}");
    }
}

#[cfg(unix)]
#[tokio::test]
async fn m2d_hard_timeout_survives_output_activity_and_approval_wait() {
    use super::fake::{FakeAdapter, Step};
    use super::adapter::ProviderAdapter;
    for approval in [false, true] {
        let (dir, env, _exe, mut req) = fixture(ProviderId::ClaudeCode, "exit 0");
        req.role = Some(Role::Executor);
        req.auth_mode = Some(tokage_core::auth::AuthMode::AskAlways);
        req.idle_timeout_secs = Some(1);
        req.hard_timeout_secs = Some(2);
        let steps = if approval {
            vec![Step::Denial { tool_id: "t".into(), command: "npm install".into(), reason: "disallowed_tool".into() }, Step::DelayMs(5000)]
        } else {
            (0..15).flat_map(|_| [Step::Emit(serde_json::json!({"fake":"text","text":"still alive"})), Step::DelayMs(200)]).collect()
        };
        let adapter = FakeAdapter::new(ProviderId::ClaudeCode, steps);
        let events = std::sync::Mutex::new(vec![]);
        let emit = |e| events.lock().unwrap().push(e);
        let launch = adapter.launch(&req, req.prompt.clone());
        let error = run_cli(&FakeAdapter::exe(), &req, launch, &env, Some(dir.path().join("mirror")), None, &emit, None).await.unwrap_err();
        assert!(error.contains("hard timeout"), "{error}");
        let events = events.lock().unwrap();
        assert!(!events.iter().any(|e| matches!(e, EventKind::Done { .. })));
        if approval { assert!(events.iter().any(|e| matches!(e, EventKind::ApprovalRequest { .. }))); }
    }
}

#[cfg(unix)]
#[test]
fn m2d_claude_session_permission_does_not_expand_to_prefix_or_pattern() {
    let mut req = testing::request(ProviderId::ClaudeCode);
    req.role = Some(Role::Executor);
    req.session_allowed = vec!["npm test".into(), "npm *".into(), "Bash(*)".into()];
    let args = super::claude::launch(&req, "test".into()).args;
    assert!(args.contains(&"Bash(npm test)".into()));
    assert!(!args.contains(&"Bash(npm test:*)".into()));
    assert!(!args.contains(&"Bash(npm *)".into()));
    assert!(!args.contains(&"Bash(Bash(*))".into()));
}

#[cfg(unix)]
#[tokio::test]
async fn m2d_closed_stdin_preserves_quota_error_instead_of_broken_pipe() {
    let script = r#"exec 0<&-
printf '%s\n' '{"type":"result","subtype":"error_during_execution","is_error":true,"result":"429 quota exceeded"}'
exit 1"#;
    let (_dir, env, exe, mut req) = fixture(ProviderId::ClaudeCode, script);
    req.prompt = "x".repeat(2 * 1024 * 1024);
    let events = std::sync::Mutex::new(vec![]);
    let emit = |event| events.lock().unwrap().push(event);
    let failure = run_cli(&exe, &req, launch(&req).unwrap(), &env, None, None, &emit, None).await.unwrap_err();
    assert!(failure.contains("429 quota exceeded"), "{failure}");
    let events = events.lock().unwrap();
    assert!(events.iter().any(|e| matches!(e, EventKind::Quota { .. })));
    assert!(!events.iter().any(|e| matches!(e, EventKind::Done { .. })));
}

#[cfg(unix)]
#[tokio::test]
async fn native_compact_supervises_two_phases_and_refuses_unconfirmed_or_foreign_boundaries() {
    // Synthetic protocol fixture: no vendor/model calls, not a recorded live acceptance.
    for outcome in ["confirmed","missing","foreign","auto","plain","carrier_error"] {
        let boundary=match outcome {
            "confirmed"|"carrier_error"=>r#"{"type":"system","subtype":"compact_boundary","session_id":"window","compact_metadata":{"trigger":"manual","pre_tokens":1234}}"#,
            "foreign"=>r#"{"type":"system","subtype":"compact_boundary","session_id":"other","compact_metadata":{"trigger":"manual","pre_tokens":1234}}"#,
            "auto"=>r#"{"type":"system","subtype":"compact_boundary","session_id":"window","compact_metadata":{"trigger":"auto","pre_tokens":1234}}"#,
            _=>"",
        };
        let script=format!(r#"
if [ "$1" = --version ]; then echo '2.1.288'; exit 0; fi
if [ "$1" = auth ]; then echo '{{"loggedIn":true,"authMethod":"claude.ai"}}'; exit 0; fi
prompt=$(cat)
printf '%s\n' "$prompt" >> calls.txt
case "$prompt" in
/compact\ *)
  if [ '{outcome}' = plain ]; then echo 'ordinary prose'; exit 0; fi
  printf '%s\n' '{boundary}'
  printf '%s\n' '{{"type":"result","subtype":"success","session_id":"window","usage":{{"input_tokens":10,"output_tokens":2}},"total_cost_usd":0.03}}'
  ;;
*)
  if [ '{outcome}' = carrier_error ]; then echo '{{"type":"result","is_error":true,"result":"carrier failed"}}'; exit 1; fi
  printf '%s\n' '{{"type":"result","subtype":"success","session_id":"window","usage":{{"input_tokens":20,"output_tokens":3}},"total_cost_usd":0.04}}'
  ;;
esac
"#);
        let (dir,env,exe,mut req)=fixture(ProviderId::ClaudeCode,&script);
        req.mode=TurnMode::Direct;req.role=None;req.resume=Some("window".into());
        req.prompt="ORIGINAL_USER_MESSAGE".into();req.context_compact=Some("/compact retain goal".into());
        std::fs::create_dir(dir.path().join(".claude")).unwrap();
        std::fs::write(dir.path().join(".claude/.credentials.json"),r#"{"claudeAiOauth":{"accessToken":"synthetic-access","refreshToken":"synthetic-refresh","expiresAt":9999999999999}}"#).unwrap();
        let mut provider=crate::providers::detect_one(&env,ProviderId::ClaudeCode,exe.to_str()).await.unwrap();
        assert_eq!(provider.status,ProviderStatus::Ready);
        assert!(adapter::native_compact_available(&provider));
        provider.version=Some("2.1.289".into());assert!(!adapter::native_compact_available(&provider));
        provider.version=Some("2.1.288".into());
        let bus=Bus::new();let mut rx=bus.subscribe();
        run_turn(&bus,req,vec![provider],env,None,None).await;
        let mut done=0;let mut confirmed=0;let mut failures=0;let mut overhead=None;let mut errors=vec![];
        while let Ok(event)=rx.try_recv() {
            match event {
                DomainEvent::ChatOutput{event:EventKind::Done{cost_usd,..},..}=>{done+=1;assert_eq!(cost_usd,Some(0.04));},
                DomainEvent::ChatOutput{event:EventKind::ContextCompact{confirmed:yes,receipt},..}=>{if yes {confirmed+=1;} overhead=receipt.usage().and_then(|u|u.cost_usd);},
                DomainEvent::TurnFailed{message,..}=>{failures+=1;errors.push(message);},
                _=>(),
            }
        }
        let calls=std::fs::read_to_string(dir.path().join("calls.txt")).unwrap_or_else(|e|panic!("{e}: {errors:?}"));
        assert!(calls.starts_with("/compact retain goal\n"));
        let runs_carrier=matches!(outcome,"confirmed"|"carrier_error");
        assert_eq!(calls.contains("ORIGINAL_USER_MESSAGE"),runs_carrier,"{outcome}");
        assert_eq!(done,usize::from(outcome=="confirmed"));
        assert_eq!(confirmed,usize::from(runs_carrier));
        assert_eq!(failures,usize::from(outcome!="confirmed"));
        if runs_carrier || matches!(outcome,"missing"|"auto"|"foreign") {assert_eq!(overhead,Some(0.03));}
    }
}

#[cfg(unix)]
#[tokio::test]
async fn cancellation_after_confirmed_native_compact_preserves_usage_and_stops_carrier() {
    let (dir,env,exe,mut req)=fixture(ProviderId::ClaudeCode,r#"
cat >> calls.txt
printf '%s\n' '{"type":"system","subtype":"compact_boundary","session_id":"window","compact_metadata":{"trigger":"manual","pre_tokens":1234}}'
printf '%s\n' '{"type":"result","subtype":"success","session_id":"window","usage":{},"total_cost_usd":0.03}'
"#);
    req.mode=TurnMode::Direct;req.role=None;req.resume=Some("window".into());
    req.context_compact=Some("/compact retain".into());req.prompt="DO_NOT_SEND".into();
    let (stop,rx)=tokio::sync::watch::channel(false);
    let receipts=std::sync::Mutex::new(0);
    let result=run_with_compact(&exe,&req,launch(&req).unwrap(),&env,None,None,&|event| {
        if let EventKind::ContextCompact{confirmed:true,receipt}=event {
            assert_eq!(receipt.usage().unwrap().cost_usd,Some(0.03));
            *receipts.lock().unwrap()+=1;stop.send(true).unwrap();
        }
    },Some(rx)).await;
    assert_eq!(result.unwrap_err(),TURN_CANCELLED);
    assert_eq!(*receipts.lock().unwrap(),1);
    assert_eq!(std::fs::read_to_string(dir.path().join("calls.txt")).unwrap(),"/compact retain");
}
