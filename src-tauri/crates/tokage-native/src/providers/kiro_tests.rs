use super::*;

#[test]
fn identity_requires_a_nonempty_positive_field() {
    for input in [
        "{}",
        r#"{"email":"","accountType":""}"#,
        "not logged in",
        r#"{"error":"unauthorized"}"#,
    ] {
        assert!(parse_kiro_account(input).is_none(), "{input}");
    }
    assert_eq!(
        parse_kiro_account(r#"{"email":"test@example.invalid","accountType":"SocialGoogle"}"#),
        Some("Google (test@example.invalid)".into())
    );
}

#[cfg(unix)]
#[tokio::test]
async fn zero_exit_without_identity_does_not_mark_ready() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("kiro-cli");
    let env = ShellEnv {
        vars: [
            ("HOME".into(), dir.path().display().to_string()),
            ("PATH".into(), "/usr/bin:/bin".into()),
        ]
        .into(),
    };
    for (who, code, expected) in [
        ("{}", 0, ProviderStatus::UnknownAuth),
        (
            r#"{"email":"test@example.invalid"}"#,
            0,
            ProviderStatus::Ready,
        ),
        ("{}", 1, ProviderStatus::NeedsLogin),
    ] {
        std::fs::write(&exe,format!("#!/bin/sh\nif [ \"$1\" = --version ]; then echo 2.31.1; exit 0; fi\nif [ \"$1\" = whoami ]; then printf '%s\\n' '{who}'; exit {code}; fi\nprintf '%s\\n' '{{\"models\":[{{\"model_id\":\"claude-haiku-4.5\"}}]}}'\n")).unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o700)).unwrap();
        let provider = detect_one(&env, ProviderId::KiroCli, exe.to_str())
            .await
            .unwrap();
        assert_eq!(provider.status, expected);
    }
}
