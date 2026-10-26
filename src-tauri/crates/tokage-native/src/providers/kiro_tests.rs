use super::*;

#[test]
fn identity_requires_a_nonempty_positive_field() {
    for input in [
        "{}",
        r#"{"account":null}"#,
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
        (r#"{"account":null}"#, 0, ProviderStatus::UnknownAuth),
        (
            r#"{"email":"test@example.invalid"}"#,
            0,
            ProviderStatus::Ready,
        ),
        ("{}", 1, ProviderStatus::UnknownAuth),
        ("database is locked", 1, ProviderStatus::UnknownAuth),
        ("network request failed", 1, ProviderStatus::UnknownAuth),
        ("Operation not permitted", 1, ProviderStatus::UnknownAuth),
        ("Not logged in", 1, ProviderStatus::NeedsLogin),
        ("Authentication required", 1, ProviderStatus::NeedsLogin),
        ("Token expired", 1, ProviderStatus::NeedsLogin),
    ] {
        std::fs::write(&exe,format!("#!/bin/sh\nif [ \"$1\" = --version ]; then echo 2.31.1; exit 0; fi\nif [ \"$1\" = whoami ]; then printf '%s\\n' '{who}'; exit {code}; fi\nprintf '%s\\n' '{{\"models\":[{{\"model_id\":\"claude-haiku-4.5\"}}]}}'\n")).unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o700)).unwrap();
        let provider = detect_one(&env, ProviderId::KiroCli, exe.to_str())
            .await
            .unwrap();
        assert_eq!(provider.status, expected);
        match expected {
            ProviderStatus::UnknownAuth => {
                assert!(provider.guidance.iter().any(|g| matches!(g.code, GuidanceCode::VerifyLogin)));
                assert!(!provider.guidance.iter().any(|g| matches!(g.code, GuidanceCode::Login)));
            }
            ProviderStatus::NeedsLogin => {
                assert!(provider.guidance.iter().any(|g| matches!(g.code, GuidanceCode::Login)));
            }
            _ => {}
        }
    }
}

/// Real `kiro-cli chat --list-models` opens a browser login when unsigned. Ensure
/// Tokage never invokes that subcommand unless whoami already proved identity.
#[cfg(unix)]
#[tokio::test]
async fn unsigned_whoami_does_not_invoke_list_models() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("kiro-cli");
    // Auth probes cwd into the process temp dir (sandbox write allowlist = worktree).
    let log = std::env::temp_dir().join(format!(
        "tokage-kiro-invocations-{}.log",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&log);
    let env = ShellEnv {
        vars: [
            ("HOME".into(), dir.path().display().to_string()),
            ("PATH".into(), "/usr/bin:/bin".into()),
        ]
        .into(),
    };
    for (who, code, expected) in [
        (r#"{"account":null}"#, 0, ProviderStatus::UnknownAuth),
        ("database is locked", 1, ProviderStatus::UnknownAuth),
        ("Not logged in", 1, ProviderStatus::NeedsLogin),
    ] {
        let _ = std::fs::remove_file(&log);
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >>'{}'\nif [ \"$1\" = --version ]; then echo 2.31.1; exit 0; fi\nif [ \"$1\" = whoami ]; then printf '%s\\n' '{who}'; exit {code}; fi\necho 'list-models must not run' >&2; exit 99\n",
            log.display()
        );
        std::fs::write(&exe, script).unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o700)).unwrap();
        let provider = detect_one(&env, ProviderId::KiroCli, exe.to_str())
            .await
            .unwrap();
        assert_eq!(provider.status, expected);
        let invocations = std::fs::read_to_string(&log).unwrap_or_default();
        assert!(
            !invocations.contains("list-models"),
            "list-models ran without confirmed identity:\n{invocations}"
        );
        assert!(invocations.contains("whoami"), "{invocations}");
    }
    let _ = std::fs::remove_file(&log);
}

#[cfg(unix)]
#[tokio::test]
async fn catalog_is_pinned_to_dispatch_engine_and_invalid_engine_does_not_probe() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("kiro-cli");
    // The real auth sandbox runs in the process temp dir, where this log is writable.
    let log = std::env::temp_dir().join(format!("tokage-kiro-engine-{}.log",std::process::id()));
    std::fs::write(&exe,format!(r#"#!/bin/sh
printf '%s\n' "$*" >> '{}'
if [ "$1" = whoami ]; then echo '{{"accountType":"SocialGoogle"}}'; exit 0; fi
if [ "$1" = chat ] && [ "$2" = --v2 ]; then echo '{{"models":[{{"model_id":"v2-model"}}]}}'; exit 0; fi
if [ "$1" = chat ] && [ "$2" = --v3 ]; then echo '{{"models":[{{"model_id":"v3-model"}}]}}'; exit 0; fi
exit 99
"#,log.display())).unwrap();
    std::fs::set_permissions(&exe,std::fs::Permissions::from_mode(0o700)).unwrap();
    for setting in [None,Some("v2"),Some("v3"),Some("invalid")] {
        let _ = std::fs::remove_file(&log);
        let mut env = ShellEnv { vars: [("HOME".into(),dir.path().display().to_string()),("PATH".into(),"/usr/bin:/bin".into())].into() };
        if let Some(value) = setting { env.vars.insert("TOKAGE_KIRO_ENGINE".into(),value.into()); }
        let result = probe_kiro(&exe,&env).await;
        if setting == Some("invalid") {
            assert_eq!(result.status,Some(ProviderStatus::Degraded));
            assert!(!log.exists());
            assert!(result.models.is_empty());
        } else {
            let engine = setting.unwrap_or("v2");
            assert_eq!(result.status,Some(ProviderStatus::Ready));
            assert_eq!(result.models[0].id,format!("{engine}-model"));
            assert!(std::fs::read_to_string(&log).unwrap().contains(&format!("chat --{engine} --list-models --format json")));
        }
    }
    let _ = std::fs::remove_file(log);
}

#[tokio::test]
#[ignore = "installed Kiro read-only native detection; no prompt, login, wallet query or UI acceptance"]
async fn installed_engine_detection() {
    let env = crate::env::capture().await;
    let configured = env.vars.get("TOKAGE_KIRO_ENGINE").cloned();
    println!("captured_engine={}",configured.as_deref().unwrap_or("unset; default v2"));
    for setting in [None,Some("v2"),Some("v3")] {
        let mut check_env = env.clone();
        if let Some(engine)=setting { check_env.vars.insert("TOKAGE_KIRO_ENGINE".into(),engine.into()); }
        let p = detect_one(&check_env,ProviderId::KiroCli,None).await.unwrap();
        println!("KIRO_NATIVE_DETECTION {}",serde_json::json!({"setting":setting.unwrap_or("captured"),"status":p.status,"version":p.version,"modelCount":p.models.len(),"haikuAvailable":p.models.iter().any(|m|m.id=="claude-haiku-4.5"),"positiveAuth":p.auth_method.is_some(),"modelsSource":p.models_source}));
        assert!(matches!(p.status,ProviderStatus::Ready|ProviderStatus::UnsupportedVersion));
        assert!(!p.models.is_empty());
    }
}
