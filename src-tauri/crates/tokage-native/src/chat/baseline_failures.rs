//! No-token regressions using real adapter parsers and production run_turn /
//! cancellation paths, with synthetic CLI output and isolated HOME directories.
use super::*;

#[cfg(unix)]
fn fixture(id: ProviderId, script: &str) -> (tempfile::TempDir, ShellEnv, std::path::PathBuf, ChatRequest) {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("cli");
    std::fs::write(&exe, format!("#!/bin/sh\n{script}\n")).unwrap();
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
printf '%s\n' '{"type":"turn.failed","error":{"message":"401 Unauthorized: token expired"}}'
exit 1
"# };
        let (_dir, env, exe, req) = fixture(id, script);
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
            let (_dir, env, exe, req) = fixture(id, &script);
            let events = std::sync::Mutex::new(vec![]);
            let emit = |event| events.lock().unwrap().push(event);
            assert!(run_cli(&exe, &req, launch(&req).unwrap(), &env, None, None, &emit, None).await.is_err());
            assert!(!events.lock().unwrap().iter().any(|e| matches!(e, EventKind::Done { .. })));
        }
    }
}
