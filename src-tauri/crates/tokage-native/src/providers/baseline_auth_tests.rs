//! Isolated login / logout / expired-output probes. Never replaces real keys or
//! signs the installed CLI out. Scripts run through the production auth sandbox.
use super::*;

#[cfg(unix)]
#[tokio::test]
async fn claude_and_codex_login_state_transitions_do_not_reuse_stale_identity() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("cli");
    std::fs::write(&exe, "#!/bin/sh\ncat \"$HOME/auth-output\"\nexit $(cat \"$HOME/auth-exit\")\n").unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o700)).unwrap();
    let env = ShellEnv { vars: [("HOME".into(), dir.path().to_string_lossy().into_owned()), ("PATH".into(), "/usr/bin:/bin".into())].into() };
    for id in [ProviderId::CodexCli] {
        let ready = if id == ProviderId::ClaudeCode { r#"{"loggedIn":true,"authMethod":"claude.ai","subscriptionType":"pro"}"# } else { "Logged in using ChatGPT" };
        let signed_out = if id == ProviderId::ClaudeCode { r#"{"loggedIn":false,"authMethod":"none"}"# } else { "Not logged in" };
        for (output, code, expected) in [(ready.to_string(), 0, ProviderStatus::Ready), (signed_out.to_string(), 1, ProviderStatus::NeedsLogin), (format!("{ready}\nOAuth token revoked"), 0, ProviderStatus::NeedsLogin), ("invalid JSON / no positive identity".into(), 0, ProviderStatus::NeedsLogin), (ready.to_string(), 1, ProviderStatus::NeedsLogin), (ready.to_string(), 0, ProviderStatus::Ready)] {
            std::fs::write(dir.path().join("auth-output"), output).unwrap();
            std::fs::write(dir.path().join("auth-exit"), code.to_string()).unwrap();
            let probe = if id == ProviderId::ClaudeCode { probe_claude(&exe, &env).await } else { probe_codex(&exe, &env).await };
            assert_eq!(probe.status, Some(expected), "{id:?}");
            if expected != ProviderStatus::Ready { assert!(probe.account.is_none()); assert!(probe.auth_method.is_none()); }
        }
    }
}

#[test]
fn expired_evidence_overrides_positive_login_text() {
    assert!(!status_confirms_login(ProviderId::CodexCli, true, "Logged in using ChatGPT\nOAuth token revoked"));
    assert!(!status_confirms_login(ProviderId::CodexCli, true, "Logged in using ChatGPT\nToken expired"));
    assert!(!claude_confirms_login(true, "Invalid API key", Some(&serde_json::json!({"loggedIn":true,"authMethod":"api_key"}))));
}

#[tokio::test]
async fn claude_login_detection_is_read_only_and_tracks_cleared_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join(".claude");
    std::fs::create_dir(&root).unwrap();
    let exe = dir.path().join("must-not-run");
    let env = ShellEnv { vars: [("HOME".into(), dir.path().to_string_lossy().into_owned())].into() };
    for (access, refresh, expected) in [("synthetic-access", "synthetic-refresh", ProviderStatus::Ready), ("", "", ProviderStatus::NeedsLogin)] {
        std::fs::write(root.join(".credentials.json"), serde_json::json!({"claudeAiOauth":{"accessToken":access,"refreshToken":refresh,"expiresAt":0}}).to_string()).unwrap();
        assert_eq!(probe_claude(&exe, &env).await.status, Some(expected));
    }
}
