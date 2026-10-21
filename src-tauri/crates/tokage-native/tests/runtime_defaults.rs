//! Exercise the ordinary library build: cfg(test) in the library is disabled here.
use std::{process::Command, time::Duration};
use tokage_native::chat::{TimeoutLimits, TimeoutTier};

#[test]
fn inherited_test_timeout_variables_cannot_change_runtime_limits() {
    const PROBE: &str = "TOKAGE_RUNTIME_DEFAULTS_PROBE";
    if std::env::var_os(PROBE).is_some() {
        let quick = TimeoutLimits::resolved(TimeoutTier::Quick, 1.0, None, None);
        assert_eq!(quick.idle, Duration::from_secs(15));
        assert_eq!(quick.hard, Duration::from_secs(30));
        let agent = TimeoutLimits::resolved(TimeoutTier::Agent, 1.0, Some(5), Some(9));
        assert_eq!(agent.idle, Duration::from_secs(5));
        assert_eq!(agent.hard, Duration::from_secs(9));
        return;
    }
    // Child-only variables avoid racing any other test or changing host configuration.
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "inherited_test_timeout_variables_cannot_change_runtime_limits"])
        .env(PROBE, "1")
        .env("TOKAGE_IDLE_TIMEOUT_MS", "1")
        .env("TOKAGE_HARD_TIMEOUT_MS", "2")
        .output().unwrap();
    assert!(output.status.success(), "{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
}
