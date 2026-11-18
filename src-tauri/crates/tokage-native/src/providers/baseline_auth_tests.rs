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
}

#[cfg(unix)]
#[tokio::test]
async fn claude_login_detection_follows_cli_status_even_with_unreadable_credentials() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join(".claude");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(root.join(".credentials.json")).unwrap();
    let exe = dir.path().join("claude");
    std::fs::write(&exe, "#!/bin/sh\n[ \"$*\" = 'auth status' ] || exit 2\ncat \"$HOME/auth-output\"\nexit $(cat \"$HOME/auth-exit\")\n").unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o700)).unwrap();
    let env = ShellEnv { vars: [
        ("HOME".into(), dir.path().to_string_lossy().into_owned()),
        ("PATH".into(), "/usr/bin:/bin".into()),
    ].into() };
    let ready = r#"{"loggedIn":true,"authMethod":"claude.ai","subscriptionType":"pro"}"#;
    for (output, code, expected) in [
        (ready, 0, ProviderStatus::Ready),
        (r#"{"loggedIn":false,"authMethod":"none"}"#, 1, ProviderStatus::NeedsLogin),
        ("OAuth token revoked", 1, ProviderStatus::NeedsLogin),
        ("unrecognized output", 0, ProviderStatus::UnknownAuth),
        (ready, 1, ProviderStatus::UnknownAuth),
        (ready, 0, ProviderStatus::Ready),
    ] {
        std::fs::write(dir.path().join("auth-output"), output).unwrap();
        std::fs::write(dir.path().join("auth-exit"), code.to_string()).unwrap();
        let p = probe_claude(&exe, &env).await;
        assert_eq!(p.status, Some(expected));
        if expected == ProviderStatus::Ready {
            assert_eq!(p.account.unwrap().plan.as_deref(), Some("pro"));
        } else {
            assert!(p.account.is_none());
        }
    }
}
