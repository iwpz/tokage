//! Offline Antigravity adapter process tests.
use super::*;

#[cfg(unix)]
#[tokio::test]
async fn cancellation_stops_antigravity_process_without_done() {
    use super::adapter::ProviderAdapter;
    use super::fake::{FakeAdapter, Step};
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join(".gemini/antigravity-cli");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(config.join("settings.json"), crate::providers::ANTIGRAVITY_GEMINI_SETTINGS).unwrap();
    let env = ShellEnv {
        vars: [
            ("HOME".into(), dir.path().display().to_string()),
            ("PATH".into(), "/usr/bin:/bin".into()),
            ("GEMINI_API_KEY".into(), "synthetic-api-key".into()),
        ]
        .into(),
    };
    let mut req = testing::request(ProviderId::AntigravityCli);
    req.cwd = dir.path().display().to_string();
    req.model = "gemini-3.8-flash-low".into();
    req.mode = TurnMode::Plan;
    req.timeout_tier = Some(TimeoutTier::Agent);
    req.hard_timeout_secs = Some(5);
    let fake = FakeAdapter::new(
        ProviderId::AntigravityCli,
        vec![
            Step::Emit(serde_json::json!({"fake":"thread","id":"antigravity-cancel"})),
            Step::DelayMs(2000),
            Step::Emit(serde_json::json!({"fake":"done"})),
        ],
    );
    let events = std::sync::Mutex::new(vec![]);
    let emit = |e| events.lock().unwrap().push(e);
    let (tx, rx) = tokio::sync::watch::channel(false);
    let cancel = async {
        tokio::time::sleep(Duration::from_millis(150)).await;
        tx.send(true).unwrap();
    };
    let exe = FakeAdapter::exe();
    let run = run_cli(
        &exe,
        &req,
        fake.launch(&req, "".into()),
        &env,
        None,
        None,
        &emit,
        Some(rx),
    );
    let (result, _) = tokio::join!(run, cancel);
    assert_eq!(result.unwrap_err(), TURN_CANCELLED);
    assert!(!events
        .lock()
        .unwrap()
        .iter()
        .any(|e| matches!(e, EventKind::Done { .. })));
}

#[cfg(unix)]
#[tokio::test]
async fn antigravity_idle_timeout_is_not_success() {
    use super::fake::{run_fake_adapter, FakeAdapter, Step};
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join(".gemini/antigravity-cli");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(config.join("settings.json"), crate::providers::ANTIGRAVITY_GEMINI_SETTINGS).unwrap();
    let env = ShellEnv {
        vars: [
            ("HOME".into(), dir.path().display().to_string()),
            ("PATH".into(), "/usr/bin:/bin".into()),
            ("GEMINI_API_KEY".into(), "synthetic-api-key".into()),
        ]
        .into(),
    };
    let mut req = testing::request(ProviderId::AntigravityCli);
    req.cwd = dir.path().display().to_string();
    req.model = "gemini-3.8-flash-low".into();
    req.mode = TurnMode::Plan;
    req.timeout_tier = Some(TimeoutTier::Agent);
    req.idle_timeout_secs = Some(1);
    req.hard_timeout_secs = Some(5);
    let fake = FakeAdapter::new(
        ProviderId::AntigravityCli,
        vec![
            Step::DelayMs(2000),
            Step::Emit(serde_json::json!({"fake":"done"})),
        ],
    );
    let result = run_fake_adapter(&fake, &env, &req).await;
    assert!(result.result.unwrap_err().contains("idle timeout"));
    assert!(!result
        .events
        .iter()
        .any(|e| matches!(e, EventKind::Done { .. })));
}

#[cfg(unix)]
#[tokio::test]
async fn antigravity_turn_rechecks_key_mode_before_spawn() {
    let dir = tempfile::tempdir().unwrap();
    let env = ShellEnv {
        vars: [("HOME".into(), dir.path().display().to_string())].into(),
    };
    let mut req = testing::request(ProviderId::AntigravityCli);
    req.cwd = dir.path().display().to_string();
    let launch = super::launch(&req).unwrap();
    assert_eq!(launch.args, ["acp"]);
    let error = run_cli(std::path::Path::new("/must-not-run"), &req, launch, &env, None, None, &|_| {}, None)
        .await.unwrap_err();
    assert!(error.contains("Antigravity requires"));
}
