use std::path::PathBuf;

use tokage_core::ProviderId;

use super::*;

#[cfg(target_os = "macos")]
#[tokio::test]
async fn restricted_seatbelt_rejects_nested_profiles() {
    let dir = tempfile::tempdir().unwrap();
    let output = create_sandboxed_command(
        ProviderId::Reasonix,
        Path::new("/usr/bin/sandbox-exec"),
        &["-p".into(), "(version 1)(allow default)".into(), "/bin/echo".into(), "nested-ok".into()],
        dir.path().to_str().unwrap(), None, None, None,
    ).unwrap().output().await.unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("sandbox_apply"));
}

#[test]
fn builtin_profiles_are_loaded() {
    let profiles = builtin_profiles();
    assert!(profiles.contains_key(&ProviderId::ClaudeCode));
    assert!(profiles.contains_key(&ProviderId::CodexCli));
    assert!(profiles.contains_key(&ProviderId::CursorCli));
    assert!(profiles.contains_key(&ProviderId::CopilotCli));
    assert!(profiles.contains_key(&ProviderId::AntigravityCli));
    assert!(profiles.len() >= 15);
}

#[test]
fn builtin_forbidden_paths_loaded() {
    let paths = builtin_forbidden_paths();
    assert!(paths.iter().any(|p| p.contains(".ssh")));
    assert!(paths.iter().any(|p| p.contains(".aws")));
    assert!(paths.iter().any(|p| p.contains(".claude")));
    assert!(paths.iter().any(|p| p.contains(".codex")));
}

#[test]
fn is_own_path_detection() {
    let claude = builtin_profiles().remove(&ProviderId::ClaudeCode).unwrap();
    assert!(is_own_path("~/.claude/**", &claude));
    assert!(is_own_path("~/.claude.json", &claude));
    assert!(!is_own_path("~/.codex/**", &claude));
    assert!(!is_own_path("~/.ssh/**", &claude));
    assert!(!is_own_path("~/.aws/**", &claude));
}

#[test]
fn sbpl_interpolates_paths_and_preserves_own_access() {
    let home = PathBuf::from("/Users/testuser");
    let worktree = PathBuf::from("/Users/testuser/work/myproj");
    let app_data = PathBuf::from("/Users/testuser/Library/Application Support/tokage");
    let config = SandboxConfig {
        home,
        worktree,
        project_root: PathBuf::from("/Users/testuser/work/myproj"),
        app_data,
        temp_dirs: vec![PathBuf::from("/tmp"), PathBuf::from("/private/tmp")],
        forbidden_paths: vec![
            "~/.claude/**".into(),
            "~/.claude.json".into(),
            "~/.codex/**".into(),
            "~/.ssh/**".into(),
            "~/.aws/**".into(),
        ],
    };

    let claude_profile = SandboxProfile {
        write: vec!["{worktree}".into(), "{tmp}".into(), "~/.claude/**".into()],
        read: vec!["{project}".into(), "~/.claude/**".into()],
        deny: vec!["{other_cli_dirs}".into(), "{forbidden_paths}".into()],
        keychain: true,
    };

    let sbpl = generate_sbpl(ProviderId::ClaudeCode, &claude_profile, &config, None);

    // Header
    assert!(sbpl.starts_with("(version 1)\n(allow default)"));

    // Allowed writes
    assert!(sbpl.contains("(subpath \"/Users/testuser/work/myproj\")"));
    assert!(sbpl.contains("(subpath \"/Users/testuser/.claude\")"));
    assert!(sbpl.contains("(subpath \"/tmp\")"));

    // Claude Code's own config must NOT be in deny
    assert!(!sbpl.contains("(deny file-read* file-write*\n    (subpath \"/Users/testuser/.claude\")"));

    // Other CLIs and credentials MUST be in deny
    assert!(sbpl.contains("(subpath \"/Users/testuser/.codex\")"));
    assert!(sbpl.contains("(subpath \"/Users/testuser/.ssh\")"));
    assert!(sbpl.contains("(subpath \"/Users/testuser/.aws\")"));

    // Keychain: true -> no SecurityServer deny
    assert!(!sbpl.contains("com.apple.SecurityServer"));
}

#[test]
fn sbpl_enforces_keychain_deny_by_default() {
    let home = PathBuf::from("/Users/testuser");
    let worktree = PathBuf::from("/Users/testuser/work/myproj");
    let app_data = PathBuf::from("/Users/testuser/Library/Application Support/tokage");
    let config = SandboxConfig {
        home,
        worktree,
        project_root: PathBuf::from("/Users/testuser/work/myproj"),
        app_data,
        temp_dirs: vec![PathBuf::from("/tmp")],
        forbidden_paths: vec!["~/.ssh/**".into()],
    };

    let codex_profile = SandboxProfile {
        write: vec!["{worktree}".into(), "{tmp}".into()],
        read: vec!["{project}".into()],
        deny: vec!["{forbidden_paths}".into()],
        keychain: false,
    };

    let sbpl = generate_sbpl(ProviderId::CodexCli, &codex_profile, &config, None);

    // Keychain: false -> SecurityServer & Keychain paths denied
    assert!(sbpl.contains("(deny mach-lookup (global-name \"com.apple.SecurityServer\"))"));
    assert!(sbpl.contains("(subpath \"/Library/Keychains\")"));
    assert!(sbpl.contains("(subpath \"/Users/testuser/Library/Keychains\")"));
}

#[cfg(target_os = "macos")]
#[test]
fn sbpl_syntax_is_valid_on_macos() {
    let tmp = tempfile::tempdir().unwrap();
    let sbpl = build_sbpl_for_provider_turn(
        ProviderId::ClaudeCode,
        tmp.path().to_str().unwrap(),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let output = std::process::Command::new("/usr/bin/sandbox-exec")
        .args(["-p", &sbpl, "/bin/echo", "sbpl_valid"])
        .output()
        .expect("sandbox-exec executes");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "sbpl_valid");
}

#[cfg(target_os = "macos")]
#[test]
fn sbpl_syntax_with_forced_proxy_is_valid_on_macos() {
    let tmp = tempfile::tempdir().unwrap();
    let sbpl = build_sbpl_for_provider_turn(
        ProviderId::ClaudeCode,
        tmp.path().to_str().unwrap(),
        None,
        None,
        None,
        Some(18923),
    )
    .unwrap();

    assert!(sbpl.contains("(allow network-outbound (remote tcp \"localhost:18923\"))"));

    let output = std::process::Command::new("/usr/bin/sandbox-exec")
        .args(["-p", &sbpl, "/bin/echo", "sbpl_forced_proxy_valid"])
        .output()
        .expect("sandbox-exec executes");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "sbpl_forced_proxy_valid");
}

#[test]
fn landlock_profile_generation_and_credential_exclusion() {
    let home = PathBuf::from("/home/testuser");
    let worktree = PathBuf::from("/home/testuser/work/myproj");
    let app_data = PathBuf::from("/home/testuser/.local/share/tokage");
    let config = SandboxConfig {
        home: home.clone(),
        worktree: worktree.clone(),
        project_root: worktree.clone(),
        app_data: app_data.clone(),
        temp_dirs: vec![PathBuf::from("/tmp"), PathBuf::from("/var/tmp")],
        forbidden_paths: vec![
            "~/.claude/**".into(),
            "~/.codex/**".into(),
            "~/.ssh/**".into(),
            "~/.aws/**".into(),
        ],
    };

    let claude_profile = SandboxProfile {
        write: vec!["{worktree}".into(), "{tmp}".into(), "~/.claude/**".into()],
        read: vec!["{project}".into(), "~/.claude/**".into()],
        deny: vec!["{other_cli_dirs}".into(), "{forbidden_paths}".into()],
        keychain: false,
    };

    let landlock = build_landlock_profile(ProviderId::ClaudeCode, &claude_profile, &config);

    // Standard system dirs in allowed_reads
    assert!(landlock.allowed_reads.contains(&PathBuf::from("/usr")));
    assert!(landlock.allowed_reads.contains(&PathBuf::from("/lib")));
    assert!(landlock.allowed_reads.contains(&PathBuf::from("/bin")));
    assert!(landlock.allowed_reads.contains(&PathBuf::from("/etc")));

    // Project and Claude's own config in allowed_reads
    assert!(landlock.allowed_reads.contains(&worktree));
    assert!(landlock.allowed_reads.contains(&home.join(".claude")));

    // Worktree and temp dirs in allowed_writes
    assert!(landlock.allowed_writes.contains(&worktree));
    assert!(landlock.allowed_writes.contains(&PathBuf::from("/tmp")));
    assert!(landlock.allowed_writes.contains(&PathBuf::from("/var/tmp")));
    assert!(landlock.allowed_writes.contains(&home.join(".claude")));

    // Credential redlines and other CLIs must NEVER be in allowed_reads or allowed_writes
    assert!(!landlock.allowed_reads.contains(&home.join(".ssh")));
    assert!(!landlock.allowed_reads.contains(&home.join(".aws")));
    assert!(!landlock.allowed_reads.contains(&home.join(".codex")));
    assert!(!landlock.allowed_reads.contains(&app_data));

    assert!(!landlock.allowed_writes.contains(&home.join(".ssh")));
    assert!(!landlock.allowed_writes.contains(&home.join(".aws")));
    assert!(!landlock.allowed_writes.contains(&home.join(".codex")));
    assert!(!landlock.allowed_writes.contains(&app_data));
}

#[test]
fn bwrap_args_generation_and_masking() {
    let home = PathBuf::from("/home/testuser");
    let worktree = PathBuf::from("/home/testuser/work/myproj");
    let app_data = PathBuf::from("/home/testuser/.local/share/tokage");
    let config = SandboxConfig {
        home: home.clone(),
        worktree: worktree.clone(),
        project_root: worktree.clone(),
        app_data: app_data.clone(),
        temp_dirs: vec![PathBuf::from("/tmp"), PathBuf::from("/var/tmp")],
        forbidden_paths: vec![
            "~/.claude/**".into(),
            "~/.claude.json".into(),
            "~/.codex/**".into(),
            "~/.ssh/**".into(),
            "~/.aws/**".into(),
        ],
    };

    let claude_profile = SandboxProfile {
        write: vec!["{worktree}".into(), "{tmp}".into(), "~/.claude/**".into()],
        read: vec!["{project}".into(), "~/.claude/**".into()],
        deny: vec!["{other_cli_dirs}".into(), "{forbidden_paths}".into()],
        keychain: false,
    };

    let exe = PathBuf::from("/usr/bin/claude");
    let args = vec!["-p".into(), "hello".into()];

    let bargs = build_bwrap_args(ProviderId::ClaudeCode, &claude_profile, &config, &exe, &args);

    // Root read-only and devices
    let has_seq = |seq: &[&str]| {
        bargs.windows(seq.len()).any(|w| w == seq)
    };
    assert!(has_seq(&["--ro-bind", "/", "/"]));
    assert!(has_seq(&["--dev", "/dev"]));
    assert!(has_seq(&["--proc", "/proc"]));

    // Worktree read-write
    assert!(has_seq(&["--bind", "/home/testuser/work/myproj", "/home/testuser/work/myproj"]));

    // Claude Code's own config must be bound read-write
    assert!(has_seq(&["--bind-try", "/home/testuser/.claude", "/home/testuser/.claude"]));

    // Credential redlines and other CLIs must be masked with --tmpfs
    assert!(has_seq(&["--tmpfs", "/home/testuser/.ssh"]));
    assert!(has_seq(&["--tmpfs", "/home/testuser/.aws"]));
    assert!(has_seq(&["--tmpfs", "/home/testuser/.codex"]));

    // Tokage app data must be masked
    assert!(has_seq(&["--tmpfs", "/home/testuser/.local/share/tokage"]));

    // Claude Code's own config must NOT be masked
    assert!(!has_seq(&["--tmpfs", "/home/testuser/.claude"]));

    // Trailing command
    assert!(has_seq(&["--", "/usr/bin/claude", "-p", "hello"]));
}

#[test]
fn turn_helpers_construct_profiles() {
    let tmp = tempfile::tempdir().unwrap();
    let tmp_path = tmp.path().to_str().unwrap();

    let landlock = build_landlock_profile_for_provider_turn(
        ProviderId::ClaudeCode,
        tmp_path,
        None,
        None,
        None,
    ).unwrap();
    assert!(landlock.allowed_writes.contains(&tmp.path().to_path_buf()));

    let bargs = build_bwrap_args_for_provider_turn(
        ProviderId::ClaudeCode,
        tmp_path,
        None,
        None,
        None,
        Path::new("/bin/sh"),
        &["-c".into(), "echo hi".into()],
    ).unwrap();
    assert!(bargs.contains(&tmp_path.to_string()));
    assert!(bargs.contains(&"/bin/sh".to_string()));
}


#[test]
fn gate_profile_is_scoped_and_excludes_claude_dirs() {
    let p = gate_profile();
    assert!(p.write.contains(&"{worktree}".to_string()));
    assert!(p.write.contains(&"~/.cargo/.package-cache".to_string()));
    assert!(!p.keychain);
    assert!(p.read.iter().chain(p.write.iter()).all(|s| !s.starts_with("~/.claude")));
    assert!(p.deny.contains(&"{other_cli_dirs}".to_string()));
    assert!(p.deny.contains(&"{forbidden_paths}".to_string()));
    assert!(p.deny.contains(&"{tokage_app_data}/**".to_string()));
}

#[test]
fn builtin_keychain_permission_is_not_overridden_by_redline_paths() {
    let config = SandboxConfig::new("/tmp/tokage-auth-test", None,
        Some(std::path::Path::new("/Users/testuser")), None);
    let profiles = builtin_profiles();
    let claude = generate_sbpl(ProviderId::ClaudeCode, &profiles[&ProviderId::ClaudeCode], &config, None);
    assert!(!claude.contains("(subpath \"/Users/testuser/Library/Keychains\")"));
    assert!(claude.contains("(subpath \"/Users/testuser/.ssh\")"));
    let codex = generate_sbpl(ProviderId::CodexCli, &profiles[&ProviderId::CodexCli], &config, None);
    assert!(codex.contains("(subpath \"/Users/testuser/Library/Keychains\")"));
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn antigravity_read_only_workspace_blocks_writes_but_allows_reads() {
    let dir = tempfile::tempdir().unwrap();
    let victim = dir.path().join("victim.txt");
    std::fs::write(&victim, "keep me").unwrap();
    let cwd = dir.path().to_str().unwrap();
    let home = std::env::var("HOME").unwrap();
    let args = vec!["-c".into(), "cat victim.txt; printf changed > victim.txt".into()];
    let mut cmd = create_antigravity_read_only_command(std::path::Path::new("/bin/sh"), &args, cwd, Some(&home), None, None).unwrap();
    let out = cmd.current_dir(dir.path()).output().await.unwrap();
    assert!(!out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "keep me");
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep me");
}
