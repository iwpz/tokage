use super::*;

#[test]
fn saved_accounts_require_a_real_credential_connection() {
    assert_eq!(opencode_connected_ids("[]"), Some(vec![]));
    assert_eq!(
        opencode_connected_ids(r#"[{"id":"deepseek","connections":[]}]"#),
        Some(vec![])
    );
    assert_eq!(
        opencode_connected_ids(
            r#"[{"id":"deepseek","connections":[{"type":"env","id":"DEEPSEEK_API_KEY"}]}]"#
        ),
        Some(vec![])
    );
    assert_eq!(
        opencode_connected_ids(
            r#"[{"id":"deepseek","connections":[{"type":"credential","id":"cred_fixture","method":"key"}]}]"#
        ),
        Some(vec!["deepseek".into()])
    );
    assert_eq!(opencode_connected_ids("not json"), None);
    assert_eq!(opencode_connected_ids(r#"[{"id":"deepseek"}]"#), None);
}

#[cfg(unix)]
#[tokio::test]
async fn sqlite_login_is_discovered_without_auth_json_and_dual_version_range_accepts_v2() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let exe = root.path().join("opencode");
    std::fs::write(&exe, r#"#!/bin/sh
case "$1" in
--version) echo 'opencode v2.0.29';;
auth) echo '[{"id":"deepseek","connections":[{"type":"credential","id":"cred_fixture","method":"key"}]}]';;
models) printf '%s\n' 'deepseek/deepseek-flash' 'opencode/free-model' 'unconfigured/model';;
esac
"#).unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o700)).unwrap();
    let env = ShellEnv {
        vars: [
            ("HOME".into(), root.path().display().to_string()),
            ("PATH".into(), "/usr/bin:/bin".into()),
        ]
        .into(),
    };
    let p = probe_opencode(&exe, &env).await;
    assert_eq!(p.status, Some(ProviderStatus::Ready));
    assert_eq!(p.models.len(), 1);
    assert_eq!(p.models[0].id, "deepseek/deepseek-flash");
    let info = detect_one(&env, ProviderId::Opencode, exe.to_str())
        .await
        .unwrap();
    assert_eq!(info.status, ProviderStatus::Ready);
    assert_eq!(info.auth_method.as_deref(), Some("stored: deepseek"));
    std::fs::write(&exe, "#!/bin/sh\necho '[]'\n").unwrap();
    assert_eq!(
        probe_opencode(&exe, &env).await.status,
        Some(ProviderStatus::NeedsLogin)
    );
    std::fs::write(&exe, "#!/bin/sh\necho broken\nexit 1\n").unwrap();
    assert_eq!(
        probe_opencode(&exe, &env).await.status,
        Some(ProviderStatus::UnknownAuth)
    );
}

#[tokio::test]
#[ignore = "requires installed OpenCode and locally saved account; no inference calls"]
async fn installed_v2_discovery() {
    let env = ShellEnv {
        vars: std::env::vars().collect(),
    };
    let path = home(&env).unwrap().join(".opencode/bin/opencode");
    let exe = path.as_path();
    let p = probe_opencode(exe, &env).await;
    assert_eq!(p.status, Some(ProviderStatus::Ready));
    assert!(p.models.iter().any(|m| m.id == "deepseek/deepseek-flash"));
    assert_eq!(p.auth_method.as_deref(), Some("stored: deepseek"));
    let info = detect_one(&env, ProviderId::Opencode, exe.to_str())
        .await
        .unwrap();
    assert_eq!(info.status, ProviderStatus::Ready);
    println!(
        "OpenCode {}: credentials recognized, {} configured models; v2 accepted",
        info.version.unwrap(),
        info.models.len()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn v1_legacy_credentials_respect_xdg_and_reject_empty_or_expired_entries() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    std::fs::create_dir_all(data.join("opencode")).unwrap();
    let auth = data.join("opencode/auth.json");
    std::fs::write(&auth, r#"{"deepseek":{"type":"api","key":"fixture-not-real"},"empty":{"type":"api","key":""},"expired":{"type":"oauth","access":"old","expires":1}}"#).unwrap();
    let exe = root.path().join("opencode");
    std::fs::write(
        &exe,
        r#"#!/bin/sh
case "$1" in
--version) echo '1.19.0';;
auth) if [ "$3" = --format ]; then echo 'help'; else echo 'Credentials'; fi;;
models) echo 'deepseek/deepseek-flash';;
esac
"#,
    )
    .unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o700)).unwrap();
    let env = ShellEnv {
        vars: [
            ("HOME".into(), root.path().display().to_string()),
            ("XDG_DATA_HOME".into(), data.display().to_string()),
        ]
        .into(),
    };
    assert_eq!(
        probe_opencode(&exe, &env).await.status,
        Some(ProviderStatus::Ready)
    );
    assert_eq!(
        detect_one(&env, ProviderId::Opencode, exe.to_str())
            .await
            .unwrap()
            .status,
        ProviderStatus::Ready
    );
    std::fs::write(&auth, "{}").unwrap();
    assert_eq!(
        probe_opencode(&exe, &env).await.status,
        Some(ProviderStatus::NeedsLogin)
    );
    std::fs::write(&auth, "broken").unwrap();
    assert_eq!(
        probe_opencode(&exe, &env).await.status,
        Some(ProviderStatus::UnknownAuth)
    );
}
