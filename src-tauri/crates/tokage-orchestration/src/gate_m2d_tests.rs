//! Faults use isolated real OS processes, without model requests or user projects.
use super::*;

#[cfg(unix)]
fn fixture(script: &str) -> (tempfile::TempDir, ShellEnv) {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gate.sh");
    std::fs::write(&file, format!("#!/bin/sh\n{script}\n")).unwrap();
    std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o700)).unwrap();
    let env = ShellEnv { vars: [("PATH".into(), "/usr/bin:/bin".into())].into() };
    (dir, env)
}

#[cfg(unix)]
fn request<'a>(dir: &'a Path, env: &'a ShellEnv, id: &'a str) -> GateRequest<'a> {
    GateRequest { command: "./gate.sh", workdir: dir, env, timeout_secs: Some(2),
        confirmed: true, allowlist: &[], env_allow: &[], run_id: Some(id) }
}

#[cfg(unix)]
#[tokio::test]
async fn m2d_queued_cancel_never_spawns_and_duplicate_ids_do_not_replace_owner() {
    let (dir, env) = fixture("touch started");
    let permit = gate_serial().acquire().await.unwrap();
    let id = "m2d-queued-cancel";
    let run = execute_gate_command(request(dir.path(), &env, id));
    let cancel = async {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if gate_runs().inner.lock().unwrap().contains_key(id) { break; }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }).await.unwrap();
        let duplicate = execute_gate_command(request(dir.path(), &env, id)).await;
        assert!(duplicate.error_message.unwrap().contains("already active"));
        assert!(cancel_gate_command_and_wait(id).await.unwrap());
    };
    let (result, _) = tokio::time::timeout(Duration::from_secs(4), async { tokio::join!(run, cancel) }).await.unwrap();
    assert_eq!(result.error_message.as_deref(), Some("cancelled"));
    assert!(!dir.path().join("started").exists());
    assert!(!cancel_gate_command(id));
    drop(permit);
}

#[cfg(unix)]
#[tokio::test]
async fn m2d_successful_parent_does_not_hang_on_background_child_pipes() {
    let (dir, env) = fixture("sleep 60 &\necho $! > child.pid\necho parent-finished\nexit 0");
    let result = tokio::time::timeout(Duration::from_secs(5),
        execute_gate_command(request(dir.path(), &env, "m2d-parent-exit"))).await.expect("pipe drain must be bounded");
    assert!(result.passed, "{result:?}");
    assert!(result.output_tail.contains("parent-finished"));
    let pid: i32 = std::fs::read_to_string(dir.path().join("child.pid")).unwrap().trim().parse().unwrap();
    for _ in 0..100 {
        if unsafe { libc::kill(pid, 0) } != 0 { return; }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("background process {pid} survived gate completion");
}

#[cfg(unix)]
#[tokio::test]
async fn m2d_cancel_running_gate_reaps_term_resistant_tree() {
    let (dir, env) = fixture("trap '' TERM\nsh -c 'trap \"\" TERM; echo $$ > grandchild.pid; exec sleep 60' &\necho $$ > parent.pid\nwait");
    let id = "m2d-running-cancel";
    let run = execute_gate_command(request(dir.path(), &env, id));
    let cancel = async {
        tokio::time::timeout(Duration::from_secs(10), async {
            while !dir.path().join("grandchild.pid").exists() { tokio::time::sleep(Duration::from_millis(10)).await; }
        }).await.unwrap();
        assert!(cancel_gate_command_and_wait(id).await.unwrap());
    };
    let (result, _) = tokio::time::timeout(Duration::from_secs(15), async { tokio::join!(run, cancel) }).await.unwrap();
    assert!(!result.passed);
    assert_eq!(result.error_message.as_deref(), Some("cancelled"));
    for name in ["parent.pid", "grandchild.pid"] {
        let pid: i32 = std::fs::read_to_string(dir.path().join(name)).unwrap().trim().parse().unwrap();
        for _ in 0..100 {
            if unsafe { libc::kill(pid, 0) } != 0 { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_ne!(unsafe { libc::kill(pid, 0) }, 0, "{name} still alive");
    }
}
