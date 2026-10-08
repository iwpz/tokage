//! F5.a contract tests for gate security (docs/fix-todo F5 / D3).
//!
//! These encode the target behaviour. Several fail against F5.a stubs / pre-F5
//! execute internals and go green as F5.b–F5.h land.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokage_matching::pack::builtin_knowledge_pack;
use tokage_native::chat::child_env::{assemble_gate_env, value_has_userinfo};
use tokage_native::env::ShellEnv;

use crate::gate::{
    check_gate_command_safety, execute_gate_command, GateCommandPermission, GateRequest,
    GATE_EVIDENCE_TAIL_LINES,
};

fn pack_allowlist() -> Vec<String> {
    builtin_knowledge_pack().gate_allowlist.clone()
}

#[test]
fn f5a_value_has_userinfo_detects_proxy_creds() {
    assert!(value_has_userinfo("http://user:secret@proxy.example:8080"));
    assert!(value_has_userinfo("https://alice:bob@host/path"));
    assert!(!value_has_userinfo("http://proxy.example:8080"));
    assert!(!value_has_userinfo("not-a-url"));
}

#[test]
fn f5a_assemble_gate_env_strips_credentials_and_userinfo() {
    let mut vars = HashMap::new();
    vars.insert("PATH".into(), "/usr/bin".into());
    vars.insert("ANTHROPIC_API_KEY".into(), "sk-secret".into());
    vars.insert(
        "HTTPS_PROXY".into(),
        "http://user:pass@proxy.example:8080".into(),
    );
    vars.insert("SAFE_PROXY".into(), "http://proxy.example:8080".into());
    let env = ShellEnv { vars };

    let scrubbed = assemble_gate_env(&env, &[]);
    assert_eq!(scrubbed.get("PATH").map(String::as_str), Some("/usr/bin"));
    assert!(
        !scrubbed.contains_key("ANTHROPIC_API_KEY"),
        "credentials must be stripped"
    );
    assert!(
        !scrubbed.contains_key("HTTPS_PROXY"),
        "userinfo proxy must be stripped"
    );
    assert_eq!(
        scrubbed.get("SAFE_PROXY").map(String::as_str),
        Some("http://proxy.example:8080")
    );

    let allowed = assemble_gate_env(&env, &["ANTHROPIC_API_KEY".into()]);
    assert_eq!(
        allowed.get("ANTHROPIC_API_KEY").map(String::as_str),
        Some("sk-secret"),
        "env_allow exact name keeps credential"
    );
}

#[test]
fn f5a_allowlist_uses_pack_not_code_constants() {
    let pack = pack_allowlist();
    assert!(
        matches!(
            check_gate_command_safety("make deploy", false, &pack),
            GateCommandPermission::Denied(_)
        ),
        "make deploy must be denied against pack allowlist"
    );
    assert!(
        matches!(
            check_gate_command_safety("npx tsc", false, &pack),
            GateCommandPermission::AllowedByPattern(_)
        ),
        "npx tsc must match pack allowlist"
    );
    assert!(
        matches!(
            check_gate_command_safety("cargo test | cat", true, &pack),
            GateCommandPermission::Denied(_)
        ),
        "metacharacters denied even when confirmed"
    );
}

#[tokio::test]
#[cfg(unix)]
async fn f5a_gate_env_probe_hides_secrets() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("printenv.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\necho \"KEY=${ANTHROPIC_API_KEY:-}\"\necho \"PROXY=${HTTPS_PROXY:-}\"\n",
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();
    }

    let mut vars = HashMap::new();
    vars.insert(
        "PATH".into(),
        std::env::var("PATH").unwrap_or_else(|_| "/usr/bin".into()),
    );
    vars.insert(
        "HOME".into(),
        std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()),
    );
    vars.insert("ANTHROPIC_API_KEY".into(), "must-not-leak".into());
    vars.insert(
        "HTTPS_PROXY".into(),
        "http://user:pass@proxy.example:8080".into(),
    );
    let env = ShellEnv { vars };
    let allowlist = pack_allowlist();

    let req = GateRequest {
        command: "./printenv.sh",
        workdir: dir.path(),
        env: &env,
        timeout_secs: Some(10),
        confirmed: true,
        allowlist: &allowlist,
        env_allow: &[],
        run_id: None,
    };
    let res = execute_gate_command(req).await;
    assert!(res.passed, "probe should run: {}", res.output_tail);
    assert!(
        !res.output_tail.contains("must-not-leak"),
        "API key leaked in output: {}",
        res.output_tail
    );
    assert!(
        !res.output_tail.contains("user:pass"),
        "proxy userinfo leaked: {}",
        res.output_tail
    );

    let allow_names = vec!["ANTHROPIC_API_KEY".to_string()];
    let req2 = GateRequest {
        command: "./printenv.sh",
        workdir: dir.path(),
        env: &env,
        timeout_secs: Some(10),
        confirmed: true,
        allowlist: &allowlist,
        env_allow: &allow_names,
        run_id: None,
    };
    let res2 = execute_gate_command(req2).await;
    assert!(res2.passed);
    // Child receives the key; OutputFilter then redacts the value in the evidence tail.
    assert!(
        res2.output_tail.contains("KEY=[REDACTED]") || res2.output_tail.contains("must-not-leak"),
        "env_allow should keep API key in child env: {}",
        res2.output_tail
    );
    assert!(
        !res2.output_tail.contains("user:pass"),
        "proxy userinfo must still be stripped when not allow-listed"
    );
}

#[tokio::test]
#[cfg(unix)]
async fn f5a_timeout_reaps_grandchild_sleep() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("grandchild.pid");
    let script = dir.path().join("leak.sh");
    let marker_str = marker.display().to_string();
    std::fs::write(
        &script,
        format!("#!/bin/sh\n(sleep 300 & echo $! > '{marker_str}')\nsleep 60\n"),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();
    }

    let mut vars = HashMap::new();
    vars.insert(
        "PATH".into(),
        std::env::var("PATH").unwrap_or_else(|_| "/bin:/usr/bin".into()),
    );
    let env = ShellEnv { vars };
    let allowlist = pack_allowlist();
    let req = GateRequest {
        command: "./leak.sh",
        workdir: dir.path(),
        env: &env,
        timeout_secs: Some(1),
        confirmed: true,
        allowlist: &allowlist,
        env_allow: &[],
        run_id: None,
    };
    let res = execute_gate_command(req).await;
    assert!(!res.passed, "should time out");
    tokio::time::sleep(Duration::from_millis(300)).await;
    if marker.exists() {
        let pid_txt = std::fs::read_to_string(&marker).unwrap_or_default();
        let pid: i32 = pid_txt.trim().parse().unwrap_or(0);
        if pid > 0 {
            let still = unsafe { libc::kill(pid, 0) == 0 };
            assert!(
                !still,
                "grandchild sleep {pid} must be gone after gate timeout"
            );
        }
    }
}

#[tokio::test]
#[cfg(unix)]
async fn f5a_output_flood_keeps_tail_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("flood.sh");
    // Enough lines to exercise the tail bound without melting CI (F5.g will stream).
    std::fs::write(
        &script,
        "#!/bin/sh\ni=0\nwhile [ $i -lt 500 ]; do\n  echo \"LINE_$i\"\n  i=$((i+1))\ndone\n",
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();
    }

    let mut vars = HashMap::new();
    vars.insert(
        "PATH".into(),
        std::env::var("PATH").unwrap_or_else(|_| "/bin:/usr/bin".into()),
    );
    let env = ShellEnv { vars };
    let allowlist = pack_allowlist();
    let _before = Instant::now();
    let req = GateRequest {
        command: "./flood.sh",
        workdir: dir.path(),
        env: &env,
        timeout_secs: Some(120),
        confirmed: true,
        allowlist: &allowlist,
        env_allow: &[],
        run_id: None,
    };
    let res = execute_gate_command(req).await;
    assert!(
        res.passed || res.exit_code.is_some(),
        "flood should complete: {:?}",
        res.error_message
    );
    let lines = res.output_tail.lines().count();
    assert!(
        lines <= GATE_EVIDENCE_TAIL_LINES,
        "tail must be ≤ {GATE_EVIDENCE_TAIL_LINES} lines, got {lines}"
    );
    assert!(
        res.output_tail.contains("LINE_499") || res.output_tail.contains("LINE_4"),
        "tail should include late lines"
    );
}

#[tokio::test]
#[cfg(unix)]
async fn f5a_gates_run_serially() {
    let dir = tempfile::tempdir().unwrap();
    let mark_a = dir.path().join("a");
    let mark_b = dir.path().join("b");
    let script = dir.path().join("mark.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\n# $1 = marker path\ndate +%s%N > \"$1.start\"\nsleep 0.4\ndate +%s%N > \"$1.end\"\n",
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();
    }

    let mut vars = HashMap::new();
    vars.insert(
        "PATH".into(),
        std::env::var("PATH").unwrap_or_else(|_| "/bin:/usr/bin".into()),
    );
    let env = Arc::new(ShellEnv { vars });
    let allowlist = Arc::new(pack_allowlist());
    let workdir: PathBuf = dir.path().to_path_buf();

    let env_a = env.clone();
    let allow_a = allowlist.clone();
    let wd_a = workdir.clone();
    let cmd_a = format!("./mark.sh {}", mark_a.display());
    let h1 = tokio::spawn(async move {
        execute_gate_command(GateRequest {
            command: &cmd_a,
            workdir: &wd_a,
            env: &env_a,
            timeout_secs: Some(30),
            confirmed: true,
            allowlist: &allow_a,
            env_allow: &[],
            run_id: None,
        })
        .await
    });

    let env_b = env.clone();
    let allow_b = allowlist.clone();
    let wd_b = workdir.clone();
    let cmd_b = format!("./mark.sh {}", mark_b.display());
    let h2 = tokio::spawn(async move {
        execute_gate_command(GateRequest {
            command: &cmd_b,
            workdir: &wd_b,
            env: &env_b,
            timeout_secs: Some(30),
            confirmed: true,
            allowlist: &allow_b,
            env_allow: &[],
            run_id: None,
        })
        .await
    });

    let (r1, r2) = tokio::join!(h1, h2);
    assert!(r1.unwrap().passed);
    assert!(r2.unwrap().passed);

    let a_start: u128 = std::fs::read_to_string(format!("{}.start", mark_a.display()))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let a_end: u128 = std::fs::read_to_string(format!("{}.end", mark_a.display()))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let b_start: u128 = std::fs::read_to_string(format!("{}.start", mark_b.display()))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let b_end: u128 = std::fs::read_to_string(format!("{}.end", mark_b.display()))
        .unwrap()
        .trim()
        .parse()
        .unwrap();

    let overlap = a_start < b_end && b_start < a_end;
    assert!(
        !overlap,
        "gate intervals must not overlap (a=[{a_start},{a_end}] b=[{b_start},{b_end}])"
    );
}
