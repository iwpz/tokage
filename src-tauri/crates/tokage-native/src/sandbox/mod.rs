//! System-level sandbox (macOS `sandbox-exec` / SBPL, docs/11 §5).
//!
//! Enforces path-level access rules on the child CLI process:
//! - Write: restricted to worktree, tmp, and CLI-specific cache/config dirs.
//! - Read: allowed for project, system runtime, toolchains, but denies redline credential paths.
//! - Deny: redline credential paths (docs/11 §6.1 `forbidden_paths`), other CLI config dirs,
//!   and Tokage's application data folder.
//! - Keychain: denied by default (reads to `~/Library/Keychains` and `com.apple.SecurityServer` mach-lookup),
//!   allowed only when `keychain: true` in the profile (e.g. Claude Code).

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tokage_core::ProviderId;

pub static BUILTIN_KNOWLEDGE: &str = include_str!("../../../../resources/knowledge/builtin.json");

/// Sandbox profile as defined in knowledge packs (`sandbox_profiles`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxProfile {
    #[serde(default)]
    pub write: Vec<String>,
    #[serde(default)]
    pub read: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
    #[serde(default)]
    pub keychain: bool,
}

/// Runtime paths used to interpolate sandbox profile templates.
#[derive(Clone, Debug)]
pub struct SandboxConfig {
    pub home: PathBuf,
    pub worktree: PathBuf,
    pub project_root: PathBuf,
    pub app_data: PathBuf,
    pub temp_dirs: Vec<PathBuf>,
    pub forbidden_paths: Vec<String>,
}

impl SandboxConfig {
    pub fn new(
        worktree: impl AsRef<Path>,
        project_root: Option<&Path>,
        home: Option<&Path>,
        app_data: Option<&Path>,
    ) -> Self {
        let home = home
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("/Users/default"));

        let worktree = worktree.as_ref().to_path_buf();
        let project_root = project_root
            .map(PathBuf::from)
            .unwrap_or_else(|| worktree.clone());

        let app_data = app_data
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("Library/Application Support/tokage"));

        let mut temp_dirs = vec![
            PathBuf::from("/tmp"),
            PathBuf::from("/private/tmp"),
            std::env::temp_dir(),
        ];
        if let Ok(c) = std::env::temp_dir().canonicalize() {
            if !temp_dirs.contains(&c) {
                temp_dirs.push(c);
            }
        }

        Self {
            home,
            worktree,
            project_root,
            app_data,
            temp_dirs,
            forbidden_paths: builtin_forbidden_paths(),
        }
    }
}

/// Returns builtin sandbox profiles from `builtin.json`.
pub fn builtin_profiles() -> HashMap<ProviderId, SandboxProfile> {
    let v: serde_json::Value =
        serde_json::from_str(BUILTIN_KNOWLEDGE).expect("valid builtin.json");
    serde_json::from_value(v["sandbox_profiles"].clone()).unwrap_or_default()
}

/// Returns builtin forbidden credential paths from `builtin.json`.
pub fn builtin_forbidden_paths() -> Vec<String> {
    let v: serde_json::Value =
        serde_json::from_str(BUILTIN_KNOWLEDGE).expect("valid builtin.json");
    serde_json::from_value(v["command_policy"]["forbidden_paths"].clone()).unwrap_or_default()
}

/// Returns builtin command policy pack from `builtin.json`.
pub fn builtin_pack_policy() -> tokage_core::command_policy::PackPolicy {
    let v: serde_json::Value =
        serde_json::from_str(BUILTIN_KNOWLEDGE).expect("valid builtin.json");
    serde_json::from_value(v["command_policy"].clone()).unwrap_or_default()
}

/// Checks whether a given path pattern belongs to the provider's own allowed configs/credentials.
pub fn is_own_path(path_pattern: &str, profile: &SandboxProfile) -> bool {
    let clean = path_pattern
        .trim_end_matches("/**")
        .trim_end_matches("/*")
        .trim_end_matches('*');
    profile
        .read
        .iter()
        .chain(profile.write.iter())
        .any(|p| {
            let p_clean = p
                .trim_end_matches("/**")
                .trim_end_matches("/*")
                .trim_end_matches('*');
            p == path_pattern
                || p_clean == clean
                || (clean.starts_with(p_clean) && clean[p_clean.len()..].starts_with('/'))
                || (p_clean.starts_with(clean) && p_clean[clean.len()..].starts_with('/'))
        })
}

/// Generates symlink / canonical aliases for `/tmp` and `/var` on macOS.
fn with_canonical_aliases(p: &str) -> Vec<String> {
    if p.starts_with("/tmp/") || p == "/tmp" {
        vec![p.to_string(), p.replacen("/tmp", "/private/tmp", 1)]
    } else if p.starts_with("/private/tmp/") || p == "/private/tmp" {
        vec![p.to_string(), p.replacen("/private/tmp", "/tmp", 1)]
    } else if p.starts_with("/var/") || p == "/var" {
        vec![p.to_string(), p.replacen("/var", "/private/var", 1)]
    } else if p.starts_with("/private/var/") || p == "/private/var" {
        vec![p.to_string(), p.replacen("/private/var", "/var", 1)]
    } else {
        vec![p.to_string()]
    }
}

/// Converts a path pattern to SBPL rule clauses.
fn format_path_rules(pattern: &str) -> Vec<String> {
    let p = pattern.trim();
    if p.is_empty() || p.contains("%APPDATA%") {
        return vec![];
    }

    let is_dir_wildcard = p.ends_with("/**") || p.ends_with("/*");
    let base = if is_dir_wildcard {
        p.trim_end_matches("/**").trim_end_matches("/*").trim_end_matches('/')
    } else {
        p.trim_end_matches('/')
    };

    if base.is_empty() {
        return vec![];
    }

    let has_wildcards = base.contains('*') || base.contains('?');
    if has_wildcards {
        let mut regex_str = String::from("^");
        for ch in base.chars() {
            match ch {
                '*' => regex_str.push_str(".*"),
                '?' => regex_str.push('.'),
                '.' | '+' | '(' | ')' | '[' | ']' | '{' | '}' | '^' | '$' | '\\' | '|' => {
                    regex_str.push('\\');
                    regex_str.push(ch);
                }
                _ => regex_str.push(ch),
            }
        }
        if is_dir_wildcard {
            regex_str.push_str("(/.*)?$");
        } else {
            regex_str.push('$');
        }
        with_canonical_aliases(&regex_str)
            .into_iter()
            .map(|r| format!("(regex \"{r}\")"))
            .collect()
    } else {
        with_canonical_aliases(base)
            .into_iter()
            .map(|a| {
                let escaped = a.replace('\\', "\\\\").replace('"', "\\\"");
                format!("(subpath \"{escaped}\")")
            })
            .collect()
    }
}

/// Expands placeholders in a path pattern against `config`.
fn expand_pattern(
    pattern: &str,
    profile: &SandboxProfile,
    config: &SandboxConfig,
) -> Vec<String> {
    let home_str = config.home.to_string_lossy();
    let worktree_str = config.worktree.to_string_lossy();
    let project_str = config.project_root.to_string_lossy();
    let app_data_str = config.app_data.to_string_lossy();

    match pattern {
        "{worktree}" => vec![worktree_str.into_owned()],
        "{project}" => vec![project_str.into_owned()],
        "{tmp}" => config
            .temp_dirs
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
        "{tokage_app_data}" => vec![app_data_str.into_owned()],
        "{other_cli_dirs}" | "{forbidden_paths}" => config
            .forbidden_paths
            .iter()
            .filter(|p| !is_own_path(p, profile))
            .flat_map(|p| expand_pattern(p, profile, config))
            .collect(),
        p => {
            let mut resolved = p.to_string();
            if resolved.starts_with("~/") {
                resolved = format!("{home_str}/{}", &resolved[2..]);
            } else if resolved == "~" {
                resolved = home_str.into_owned();
            }
            if resolved.contains("{worktree}") {
                resolved = resolved.replace("{worktree}", &worktree_str);
            }
            if resolved.contains("{project}") {
                resolved = resolved.replace("{project}", &project_str);
            }
            if resolved.contains("{tokage_app_data}") {
                resolved = resolved.replace("{tokage_app_data}", &app_data_str);
            }
            vec![resolved]
        }
    }
}

/// Generates an SBPL Scheme profile string for the given provider and config.
pub fn generate_sbpl(
    _provider: ProviderId,
    profile: &SandboxProfile,
    config: &SandboxConfig,
    forced_port: Option<u16>,
) -> String {
    let mut sbpl = String::from("(version 1)\n(allow default)\n\n");

    // 1. Write restriction: deny by default, allow worktree, tmp, dev, caches
    sbpl.push_str(";; Limit writes to allowed worktree, caches, and temp directories\n");
    sbpl.push_str("(deny file-write*)\n");
    sbpl.push_str("(allow file-write-data\n");
    sbpl.push_str("    (literal \"/dev/null\")\n");
    sbpl.push_str("    (literal \"/dev/zero\")\n");
    sbpl.push_str("    (literal \"/dev/dtracehelper\")\n");
    sbpl.push_str("    (literal \"/dev/tty\")\n");
    sbpl.push_str("    (subpath \"/dev/fd\")\n");
    sbpl.push_str(")\n");
    sbpl.push_str("(allow file-write*\n");
    sbpl.push_str("    (literal \"/dev/null\")\n");
    sbpl.push_str("    (literal \"/dev/zero\")\n");
    sbpl.push_str("    (literal \"/dev/tty\")\n");
    sbpl.push_str("    (subpath \"/dev/fd\")\n");

    let mut write_clauses = HashSet::new();
    for p in &profile.write {
        for expanded in expand_pattern(p, profile, config) {
            for clause in format_path_rules(&expanded) {
                write_clauses.insert(clause);
            }
        }
    }
    // Always allow worktree and temp dirs for writes
    for expanded in expand_pattern("{worktree}", profile, config) {
        for clause in format_path_rules(&expanded) {
            write_clauses.insert(clause);
        }
    }
    for expanded in expand_pattern("{tmp}", profile, config) {
        for clause in format_path_rules(&expanded) {
            write_clauses.insert(clause);
        }
    }

    let mut sorted_writes: Vec<String> = write_clauses.into_iter().collect();
    sorted_writes.sort();
    for clause in sorted_writes {
        sbpl.push_str(&format!("    {clause}\n"));
    }
    sbpl.push_str(")\n\n");

    // 2. Deny credential redline paths, other CLI configs, and Tokage app data
    sbpl.push_str(";; Redline credential paths, other CLIs' configs, and app data\n");
    let mut deny_clauses = HashSet::new();
    for p in &profile.deny {
        for expanded in expand_pattern(p, profile, config) {
            for clause in format_path_rules(&expanded) {
                deny_clauses.insert(clause);
            }
        }
    }
    // Enforce Tokage app data is always denied
    for expanded in expand_pattern("{tokage_app_data}/**", profile, config) {
        for clause in format_path_rules(&expanded) {
            deny_clauses.insert(clause);
        }
    }

    if !deny_clauses.is_empty() {
        sbpl.push_str("(deny file-read* file-write*\n");
        let mut sorted_denies: Vec<String> = deny_clauses.into_iter().collect();
        sorted_denies.sort();
        for clause in sorted_denies {
            sbpl.push_str(&format!("    {clause}\n"));
        }
        sbpl.push_str(")\n\n");
    }

    // 3. Keychain access policy (P2.m4c):
    // Denied by default; granted only when `keychain: true` in the profile.
    if !profile.keychain {
        sbpl.push_str(";; Keychain access denied by default (P2.m4c)\n");
        sbpl.push_str("(deny mach-lookup (global-name \"com.apple.SecurityServer\"))\n");
        let home_kc = config.home.join("Library/Keychains").to_string_lossy().into_owned();
        let home_kc_escaped = home_kc.replace('\\', "\\\\").replace('"', "\\\"");
        sbpl.push_str(&format!(
            "(deny file-read* file-write*\n    (subpath \"/Library/Keychains\")\n    (subpath \"{home_kc_escaped}\")\n    (regex \"^/Users/[^/]+/Library/Keychains\")\n    (regex \"^/private/var/root/Library/Keychains\")\n)\n"
        ));
    }

    // 4. Forced network proxy restriction (P3.m6g, docs/12 §4):
    // When forced_port is Some, deny all general network outbound and allow only TCP to localhost:<port>.
    if let Some(port) = forced_port {
        sbpl.push_str(";; Forced network proxy restriction (P3.m6g)\n");
        sbpl.push_str("(deny network-outbound)\n");
        sbpl.push_str(&format!("(allow network-outbound (remote tcp \"localhost:{port}\"))\n\n"));
    }

    sbpl
}

pub mod bwrap;
pub mod landlock;

pub use bwrap::{build_bwrap_args, find_bwrap};
pub use landlock::{
    apply_landlock, build_landlock_profile, probe_landlock_supported, LandlockProfile,
};

/// Prepares the full SBPL profile string for a conversation turn.
pub fn build_sbpl_for_turn(
    provider: ProviderId,
    worktree: &str,
    project_root: Option<&str>,
    home: Option<&str>,
    app_data: Option<&str>,
    forced_port: Option<u16>,
) -> Result<String, String> {
    let profiles = builtin_profiles();
    let profile = profiles.get(&provider).cloned().unwrap_or_else(|| {
        // Fallback default profile with general development allowances
        SandboxProfile {
            write: vec!["{worktree}".into(), "{tmp}".into()],
            read: vec!["{project}".into()],
            deny: vec![
                "{other_cli_dirs}".into(),
                "{forbidden_paths}".into(),
                "{tokage_app_data}/**".into(),
            ],
            keychain: false,
        }
    });

    let config = SandboxConfig::new(
        Path::new(worktree),
        project_root.map(Path::new),
        home.map(Path::new),
        app_data.map(Path::new),
    );

    Ok(generate_sbpl(provider, &profile, &config, forced_port))
}

/// Prepares the full Landlock profile for a conversation turn (Linux).
pub fn build_landlock_profile_for_turn(
    provider: ProviderId,
    worktree: &str,
    project_root: Option<&str>,
    home: Option<&str>,
    app_data: Option<&str>,
) -> Result<LandlockProfile, String> {
    let profiles = builtin_profiles();
    let profile = profiles.get(&provider).cloned().unwrap_or_else(|| {
        SandboxProfile {
            write: vec!["{worktree}".into(), "{tmp}".into()],
            read: vec!["{project}".into()],
            deny: vec![
                "{other_cli_dirs}".into(),
                "{forbidden_paths}".into(),
                "{tokage_app_data}/**".into(),
            ],
            keychain: false,
        }
    });

    let config = SandboxConfig::new(
        Path::new(worktree),
        project_root.map(Path::new),
        home.map(Path::new),
        app_data.map(Path::new),
    );

    Ok(landlock::build_landlock_profile(provider, &profile, &config))
}

/// Prepares the full Bubblewrap arguments for a conversation turn (Linux).
pub fn build_bwrap_args_for_turn(
    provider: ProviderId,
    worktree: &str,
    project_root: Option<&str>,
    home: Option<&str>,
    app_data: Option<&str>,
    exe: &Path,
    args: &[String],
) -> Result<Vec<String>, String> {
    let profiles = builtin_profiles();
    let profile = profiles.get(&provider).cloned().unwrap_or_else(|| {
        SandboxProfile {
            write: vec!["{worktree}".into(), "{tmp}".into()],
            read: vec!["{project}".into()],
            deny: vec![
                "{other_cli_dirs}".into(),
                "{forbidden_paths}".into(),
                "{tokage_app_data}/**".into(),
            ],
            keychain: false,
        }
    });

    let config = SandboxConfig::new(
        Path::new(worktree),
        project_root.map(Path::new),
        home.map(Path::new),
        app_data.map(Path::new),
    );

    Ok(bwrap::build_bwrap_args(provider, &profile, &config, exe, args))
}

/// Creates a sandboxed `tokio::process::Command` wrapping `exe` and `args` according
/// to host platform capabilities:
/// - macOS: `sandbox-exec -p <sbpl>`
/// - Linux: `bwrap` (if available) or `Landlock` via `pre_exec`
/// - Other / Fallback: un-sandboxed `Command::new(exe)`
pub fn create_sandboxed_command(
    provider: ProviderId,
    exe: &Path,
    args: &[String],
    worktree: &str,
    home: Option<&str>,
    app_data: Option<&str>,
    forced_port: Option<u16>,
) -> Result<tokio::process::Command, String> {
    #[cfg(target_os = "macos")]
    {
        let sbpl = build_sbpl_for_turn(provider, worktree, None, home, app_data, forced_port)?;
        let mut c = tokio::process::Command::new("/usr/bin/sandbox-exec");
        c.arg("-p").arg(sbpl).arg(exe).args(args);
        Ok(c)
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(bwrap_path) = bwrap::find_bwrap() {
            let bwrap_args = build_bwrap_args_for_turn(
                provider, worktree, None, home, app_data, exe, args,
            )?;
            let mut c = tokio::process::Command::new(bwrap_path);
            c.args(&bwrap_args);
            Ok(c)
        } else if landlock::probe_landlock_supported() {
            let landlock_profile = build_landlock_profile_for_turn(
                provider, worktree, None, home, app_data,
            )?;
            let mut c = tokio::process::Command::new(exe);
            c.args(args);
            unsafe {
                c.pre_exec(move || {
                    landlock::apply_landlock(&landlock_profile)
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
                });
            }
            Ok(c)
        } else {
            tracing::warn!("Neither bubblewrap nor Landlock is available on this Linux system; running without OS sandbox");
            let mut c = tokio::process::Command::new(exe);
            c.args(args);
            Ok(c)
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (provider, worktree, home, app_data);
        let mut c = tokio::process::Command::new(exe);
        c.args(args);
        Ok(c)
    }
}

#[cfg(test)]
mod tests;

