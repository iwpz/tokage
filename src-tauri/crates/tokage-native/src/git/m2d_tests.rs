//! Real Git hook failures must not leave hooks or descendants running after timeout.
use super::*;

#[cfg(unix)]
#[tokio::test]
async fn m2d_git_hook_timeout_reaps_tree_and_preserves_staged_work() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let env = ShellEnv { vars: [("PATH".into(), std::env::var("PATH").unwrap()),
        ("GIT_CONFIG_NOSYSTEM".into(), "1".into()), ("GIT_CONFIG_GLOBAL".into(), "/dev/null".into())].into() };
    run(&env, dir.path(), &["init", "-q"], &[], 5).await.unwrap();
    run(&env, dir.path(), &["config", "core.hooksPath", ".git/hooks"], &[], 5).await.unwrap();
    std::fs::write(dir.path().join("user-work"), "keep staged\n").unwrap();
    run(&env, dir.path(), &["add", "user-work"], &[], 5).await.unwrap();
    let hook = dir.path().join(".git/hooks/pre-commit");
    std::fs::write(&hook, "#!/bin/sh\ntrap '' TERM\nsh -c 'trap \"\" TERM; echo $$ > hook-child.pid; exec sleep 60' &\necho $$ > hook-parent.pid\nwait\n").unwrap();
    std::fs::set_permissions(hook, std::fs::Permissions::from_mode(0o700)).unwrap();
    let error = run(&env, dir.path(), &["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-m", "m2d"], &[], 1).await.unwrap_err();
    assert!(error.contains("timed out"), "{error}");
    for name in ["hook-parent.pid", "hook-child.pid"] {
        let pid: i32 = std::fs::read_to_string(dir.path().join(name)).unwrap().trim().parse().unwrap();
        for _ in 0..100 {
            if unsafe { libc::kill(pid, 0) } != 0 { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_ne!(unsafe { libc::kill(pid, 0) }, 0, "{name} survived timeout");
    }
    assert_eq!(run(&env, dir.path(), &["show", ":user-work"], &[], 5).await.unwrap(), "keep staged\n");
    assert!(run(&env, dir.path(), &["rev-parse", "--verify", "HEAD"], &[], 5).await.is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn m2d_signal_death_is_not_a_successful_diff() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("git");
    std::fs::write(&exe, "#!/bin/sh\nkill -KILL $$\n").unwrap();
    std::fs::set_permissions(exe, std::fs::Permissions::from_mode(0o700)).unwrap();
    let env = ShellEnv { vars: [("PATH".into(), dir.path().to_string_lossy().into_owned())].into() };
    assert!(run_diff(&env, dir.path(), &["diff", "--exit-code"], 5).await.is_err());
}
