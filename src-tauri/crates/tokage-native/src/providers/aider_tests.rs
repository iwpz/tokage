use super::*;

#[cfg(unix)]
#[tokio::test]
async fn configured_keys_are_ready_without_verification_or_model_requests() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let exe = root.path().join("aider");
    // Discovery may query version/catalog only. Any inference attempt fails the test.
    std::fs::write(
        &exe,
        r#"#!/bin/sh
case "$1" in
--version) echo 'aider 0.86.2';;
--list-models) echo "- ${2}fixture-model";;
*) touch "$HOME/unexpected-model-request"; exit 1;;
esac
"#,
    )
    .unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut env = ShellEnv {
        vars: [
            ("HOME".into(), root.path().display().to_string()),
            ("PATH".into(), "/usr/bin:/bin".into()),
        ]
        .into(),
    };
    let detect = |env: ShellEnv| {
        let exe = exe.clone();
        async move {
            detect_one(&env, ProviderId::Aider, exe.to_str())
                .await
                .unwrap()
        }
    };
    assert_eq!(detect(env.clone()).await.status, ProviderStatus::NeedsLogin);
    for value in ["", "YOUR_DEEPSEEK_API_KEY"] {
        std::fs::write(
            root.path().join(".env"),
            format!("DEEPSEEK_API_KEY=\"{value}\"\n"),
        )
        .unwrap();
        assert_eq!(detect(env.clone()).await.status, ProviderStatus::NeedsLogin);
    }
    std::fs::write(root.path().join(".env"), "DEEPSEEK_API_KEY=fixture-key\n").unwrap();
    env.vars
        .insert("OPENAI_API_KEY".into(), "fixture-openai-key".into());
    let info = detect(env.clone()).await;
    assert_eq!(info.status, ProviderStatus::Ready);
    assert!(!info
        .guidance
        .iter()
        .any(|g| matches!(g.code, GuidanceCode::VerifyLogin)));
    assert!(info.models.iter().any(|m| m.id.starts_with("deepseek/")));
    assert!(info.models.iter().any(|m| m.id.starts_with("openai/")));
    // Changes no longer require a receipt or additional verification.
    std::fs::write(
        root.path().join(".env"),
        "DEEPSEEK_API_KEY=changed-fixture-key\n",
    )
    .unwrap();
    assert_eq!(detect(env.clone()).await.status, ProviderStatus::Ready);
    env.vars.remove("OPENAI_API_KEY");
    std::fs::remove_file(root.path().join(".env")).unwrap();
    std::fs::write(
        root.path().join(".aider.conf.yml"),
        "api-key:\n  - deepseek=fixture-key\n",
    )
    .unwrap();
    assert_eq!(detect(env).await.status, ProviderStatus::Ready);
    assert!(!root.path().join(".aider/tokage/verified").exists());
    assert!(!root.path().join("unexpected-model-request").exists());
}
