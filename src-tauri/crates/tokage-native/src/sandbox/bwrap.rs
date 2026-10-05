//! Bubblewrap (bwrap) sandbox fallback for Linux (docs/11 §5.1, P2.m7b).
//!
//! Bubblewrap uses unprivileged Linux user namespaces to construct a sandboxed
//! filesystem view:
//! - Root mounted read-only (`--ro-bind / /`)
//! - Worktree and temp dirs mounted read-write (`--bind {worktree} {worktree}`)
//! - CLI cache and config dirs from profile mounted read-write (`--bind-try {cache} {cache}`)
//! - Redline credential paths and other CLIs' configs masked (`--tmpfs {cred_dir}`, `--ro-bind-try /dev/null {file}`)
//! - Preserves CLI's own paths so the CLI functions normally.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use tokage_core::ProviderId;

use super::{expand_pattern, SandboxConfig, SandboxProfile};

/// Detects the `bwrap` executable on the host system.
pub fn find_bwrap() -> Option<PathBuf> {
    if let Ok(forced) = std::env::var("TOKAGE_BWRAP_PATH") {
        let p = PathBuf::from(forced);
        if p.exists() {
            return Some(p);
        }
    }
    which::which("bwrap").ok()
}

/// Trims glob wildcards from a path pattern.
fn clean_path_pattern(p: &str) -> &str {
    p.trim_end_matches("/**")
        .trim_end_matches("/*")
        .trim_end_matches('*')
}

/// Builds the argument list to pass to `bwrap` for sandboxed child execution.
pub fn build_bwrap_args(
    _provider: ProviderId,
    profile: &SandboxProfile,
    config: &SandboxConfig,
    exe: &Path,
    args: &[String],
) -> Vec<String> {
    let mut bwrap_args = Vec::new();

    // 1. Root and standard devices
    bwrap_args.extend(["--ro-bind".into(), "/".into(), "/".into()]);
    bwrap_args.extend(["--dev".into(), "/dev".into()]);
    bwrap_args.extend(["--proc".into(), "/proc".into()]);

    // 2. Writable bindings
    let mut bound_writes = HashSet::new();

    let wt_str = config.worktree.to_string_lossy().into_owned();
    bwrap_args.extend(["--bind".into(), wt_str.clone(), wt_str.clone()]);
    bound_writes.insert(wt_str);

    for t in &config.temp_dirs {
        let t_str = t.to_string_lossy().into_owned();
        if bound_writes.insert(t_str.clone()) {
            bwrap_args.extend(["--bind-try".into(), t_str.clone(), t_str]);
        }
    }

    for p in &profile.write {
        for exp in expand_pattern(p, profile, config) {
            let clean = clean_path_pattern(&exp).to_string();
            if !clean.is_empty() && bound_writes.insert(clean.clone()) {
                bwrap_args.extend(["--bind-try".into(), clean.clone(), clean]);
            }
        }
    }

    // 3. Denied credential and other CLI paths
    let mut masked_paths = HashSet::new();

    for p in &profile.deny {
        for exp in expand_pattern(p, profile, config) {
            let clean = clean_path_pattern(&exp).to_string();
            if clean.is_empty() || bound_writes.contains(&clean) || !masked_paths.insert(clean.clone()) {
                continue;
            }

            // Mask files with /dev/null and directories with empty tmpfs
            let is_file = clean.ends_with(".json")
                || clean.ends_with(".key")
                || clean.ends_with(".rc")
                || clean.ends_with("config")
                || clean.ends_with("credentials")
                || clean.contains("id_rsa")
                || clean.contains("id_ed25519");

            if is_file {
                bwrap_args.extend(["--ro-bind-try".into(), "/dev/null".into(), clean]);
            } else {
                bwrap_args.extend(["--tmpfs".into(), clean]);
            }
        }
    }

    // Always mask Tokage's application data
    let app_data_str = config.app_data.to_string_lossy().into_owned();
    if masked_paths.insert(app_data_str.clone()) {
        bwrap_args.extend(["--tmpfs".into(), app_data_str]);
    }

    // 4. Executable and command arguments
    bwrap_args.push("--".into());
    bwrap_args.push(exe.to_string_lossy().into_owned());
    bwrap_args.extend(args.iter().cloned());

    bwrap_args
}
