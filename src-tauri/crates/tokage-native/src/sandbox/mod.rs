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
            // Respect the explicit keychain permission. Otherwise the shared redline
            // list silently blocks credentials even for profiles that allow keychain.
            .filter(|p| {
                !is_own_path(p, profile)
                    && !(profile.keychain && p.as_str() == "~/Library/Keychains/**")
            })
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
    provider: ProviderId,
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

    // File tools canonicalize paths by statting ancestors. Permit metadata
    // for those directories, without permitting their contents or peer files.
    if let (Ok(app_data), Ok(worktree)) =
        (config.app_data.canonicalize(), config.worktree.canonicalize())
    {
        let managed = app_data.join("worktrees");
        if worktree.starts_with(&managed)
            && worktree != managed
            && worktree.join(".git").is_file()
        {
            let exclusions = format_path_rules(&worktree.to_string_lossy())
                .into_iter().map(|rule| format!("(require-not {rule})"))
                .collect::<Vec<_>>().join(" ");
            let ancestors = worktree.ancestors().skip(1)
                .take_while(|p| p.starts_with(&app_data))
                .flat_map(|p| with_canonical_aliases(&p.to_string_lossy()))
                .map(|p| format!("(require-not (literal \"{}\"))", p.replace('\\', "\\\\").replace('"', "\\\"")))
                .collect::<Vec<_>>().join(" ");
            for expanded in expand_pattern("{tokage_app_data}/**", profile, config) {
                for clause in format_path_rules(&expanded) {
                    if deny_clauses.remove(&clause) {
                        sbpl.push_str(&format!("(deny file-read-data file-read-xattr file-write* (require-all {clause} {exclusions}))\n"));
                        sbpl.push_str(&format!("(deny file-read-metadata (require-all {clause} {exclusions} {ancestors}))\n"));
                    }
                }
            }
        }
    }

    // OpenCode V2 canonicalizes the global Claude directory during discovery.
    // Permit root directory discovery; credential file contents and writes stay denied.
    if provider == ProviderId::Opencode {
        for expanded in expand_pattern("~/.claude/**", profile, config) {
            for clause in format_path_rules(&expanded) {
                if deny_clauses.remove(&clause) {
                    let root = config.home.join(".claude").to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
                    sbpl.push_str(&format!("(deny file-read-data file-read-xattr (require-all {clause} (require-not (literal \"{root}\"))))\n(deny file-write* {clause})\n"));
                }
            }
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

/// Fallback profile for providers without a builtin entry (general development allowances).
fn default_turn_profile() -> SandboxProfile {
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
}

/// Looks up the builtin profile for `provider`, falling back to [`default_turn_profile`].
fn provider_profile(provider: ProviderId) -> SandboxProfile {
    builtin_profiles()
        .get(&provider)
        .cloned()
        .unwrap_or_else(default_turn_profile)
}

/// Sandbox profile for acceptance gate commands (docs/fix-todo F5.f).
///
/// Writes are limited to the worktree, tmp and common package-manager caches. Read / deny /
/// keychain are derived from the `CLAUDE_CODE` builtin profile, minus any `~/.claude*` entries
/// (the gate must not see a CLI's own credentials), with keychain access disabled.
/// Defined in code (not `builtin.json`) so `sandbox_profiles` stays keyed by `ProviderId` only.
pub fn gate_profile() -> SandboxProfile {
    let base = provider_profile(ProviderId::ClaudeCode);
    let not_claude = |p: &String| !p.starts_with("~/.claude");
    SandboxProfile {
        write: [
            "{worktree}",
            "{tmp}",
            "~/.npm/**",
            "~/.cache/**",
            "~/.cargo/registry/**",
            "~/.cargo/git/**",
            "~/.cargo/.package-cache",
            "~/go/pkg/mod/**",
            "~/.gradle/caches/**",
            "~/.m2/repository/**",
            "~/Library/Caches/**",
            "~/Library/pnpm/**",
            "~/.local/share/pnpm/**",
            "~/.bun/install/cache/**",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
        read: base.read.into_iter().filter(not_claude).collect(),
        deny: base.deny.into_iter().filter(not_claude).collect(),
        keychain: false,
    }
}

/// Prepares the full SBPL profile string for a conversation turn using `profile`.
pub fn build_sbpl_for_turn(
    provider: ProviderId,
    profile: &SandboxProfile,
    worktree: &str,
    project_root: Option<&str>,
    home: Option<&str>,
    app_data: Option<&str>,
    forced_port: Option<u16>,
) -> Result<String, String> {
    let config = SandboxConfig::new(
        Path::new(worktree),
        project_root.map(Path::new),
        home.map(Path::new),
        app_data.map(Path::new),
    );

    Ok(generate_sbpl(provider, profile, &config, forced_port))
}

/// Like [`build_sbpl_for_turn`], looking up the profile by `provider`.
pub fn build_sbpl_for_provider_turn(
    provider: ProviderId,
    worktree: &str,
    project_root: Option<&str>,
    home: Option<&str>,
    app_data: Option<&str>,
    forced_port: Option<u16>,
) -> Result<String, String> {
    build_sbpl_for_turn(
        provider,
        &provider_profile(provider),
        worktree,
        project_root,
        home,
        app_data,
        forced_port,
    )
}

/// Prepares the full Landlock profile for a conversation turn using `profile` (Linux).
pub fn build_landlock_profile_for_turn(
    provider: ProviderId,
    profile: &SandboxProfile,
    worktree: &str,
    project_root: Option<&str>,
    home: Option<&str>,
    app_data: Option<&str>,
) -> Result<LandlockProfile, String> {
    let config = SandboxConfig::new(
        Path::new(worktree),
        project_root.map(Path::new),
        home.map(Path::new),
        app_data.map(Path::new),
    );

    Ok(landlock::build_landlock_profile(provider, profile, &config))
}

/// Like [`build_landlock_profile_for_turn`], looking up the profile by `provider`.
pub fn build_landlock_profile_for_provider_turn(
    provider: ProviderId,
    worktree: &str,
    project_root: Option<&str>,
    home: Option<&str>,
    app_data: Option<&str>,
) -> Result<LandlockProfile, String> {
    build_landlock_profile_for_turn(
        provider,
        &provider_profile(provider),
        worktree,
        project_root,
        home,
        app_data,
    )
}

/// Prepares the full Bubblewrap arguments for a conversation turn using `profile` (Linux).
pub fn build_bwrap_args_for_turn(
    provider: ProviderId,
    profile: &SandboxProfile,
    worktree: &str,
    project_root: Option<&str>,
    home: Option<&str>,
    app_data: Option<&str>,
    exe: &Path,
    args: &[String],
) -> Result<Vec<String>, String> {
    let config = SandboxConfig::new(
        Path::new(worktree),
        project_root.map(Path::new),
        home.map(Path::new),
        app_data.map(Path::new),
    );

    Ok(bwrap::build_bwrap_args(provider, profile, &config, exe, args))
}

/// Like [`build_bwrap_args_for_turn`], looking up the profile by `provider`.
pub fn build_bwrap_args_for_provider_turn(
    provider: ProviderId,
    worktree: &str,
    project_root: Option<&str>,
    home: Option<&str>,
    app_data: Option<&str>,
    exe: &Path,
    args: &[String],
) -> Result<Vec<String>, String> {
    build_bwrap_args_for_turn(
        provider,
        &provider_profile(provider),
        worktree,
        project_root,
        home,
        app_data,
        exe,
        args,
    )
}

/// Wraps `exe` and `args` in the host sandbox described by `profile`:
/// - macOS: `sandbox-exec -p <sbpl>`
/// - Linux: `bwrap` (if available) or `Landlock` via `pre_exec`
///
/// Returns `Ok(None)` when no sandbox mechanism is available on this host.
fn wrap_command_with_profile(
    provider: ProviderId,
    profile: &SandboxProfile,
    exe: &Path,
    args: &[String],
    worktree: &str,
    home: Option<&str>,
    app_data: Option<&str>,
    forced_port: Option<u16>,
) -> Result<Option<tokio::process::Command>, String> {
    #[cfg(target_os = "macos")]
    {
        let sbpl =
            build_sbpl_for_turn(provider, profile, worktree, None, home, app_data, forced_port)?;
        let mut c = tokio::process::Command::new("/usr/bin/sandbox-exec");
        c.arg("-p").arg(sbpl).arg(exe).args(args);
        Ok(Some(c))
    }

    #[cfg(target_os = "linux")]
    {
        let _ = forced_port;
        if let Some(bwrap_path) = bwrap::find_bwrap() {
            let bwrap_args = build_bwrap_args_for_turn(
                provider, profile, worktree, None, home, app_data, exe, args,
            )?;
            let mut c = tokio::process::Command::new(bwrap_path);
            c.args(&bwrap_args);
            Ok(Some(c))
        } else if landlock::probe_landlock_supported() {
            let landlock_profile = build_landlock_profile_for_turn(
                provider, profile, worktree, None, home, app_data,
            )?;
            let mut c = tokio::process::Command::new(exe);
            c.args(args);
            unsafe {
                c.pre_exec(move || {
                    landlock::apply_landlock(&landlock_profile)
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
                });
            }
            Ok(Some(c))
        } else {
            Ok(None)
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (provider, profile, exe, args, worktree, home, app_data, forced_port);
        Ok(None)
    }
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
    let profile = provider_profile(provider);
    match wrap_command_with_profile(
        provider, &profile, exe, args, worktree, home, app_data, forced_port,
    )? {
        Some(c) => Ok(c),
        None => {
            #[cfg(target_os = "linux")]
            tracing::warn!("Neither bubblewrap nor Landlock is available on this Linux system; running without OS sandbox");
            let mut c = tokio::process::Command::new(exe);
            c.args(args);
            Ok(c)
        }
    }
}

/// Antigravity's plan flag is an instruction prefix, not a write permission.
/// Read-only roles therefore require a kernel-enforced read-only workspace.
pub fn create_antigravity_read_only_command(
    exe: &Path, args: &[String], worktree: &str, home: Option<&str>,
    app_data: Option<&str>, forced_port: Option<u16>,
) -> Result<tokio::process::Command, String> {
    create_read_only_command(ProviderId::AntigravityCli, exe, args, worktree, home, app_data, forced_port)
}

/// Usage management may read CLI credentials but must not open a browser or grant trust.
/// This restriction applies only to management probes, never to normal model turns.
fn usage_profile(provider: ProviderId, args: &[String]) -> SandboxProfile {
    let mut profile=provider_profile(provider);
    // The official account manager may consult macOS Keychain even with file auth.
    // Only this fixed management transport gets that capability; model execution does not.
    if provider==ProviderId::CodexCli && args.iter().map(String::as_str).eq(["app-server","-c","mcp_servers={}","--listen","stdio://"]) {
        profile.keychain=true;
    }
    profile
}

pub fn create_usage_command(provider: ProviderId, exe: &Path, args: &[String], cwd: &str, home: Option<&str>) -> Result<tokio::process::Command, String> {
    #[cfg(target_os = "macos")]
    {
        let profile = usage_profile(provider, args);
        let mut sbpl = build_sbpl_for_turn(provider, &profile, cwd, None, home, None, None)?;
        sbpl.push_str("\n(deny process-exec (literal \"/usr/bin/open\"))\n");
        let mut cmd = tokio::process::Command::new("/usr/bin/sandbox-exec");
        cmd.arg("-p").arg(sbpl).arg(exe).args(args);
        return Ok(cmd);
    }
    #[cfg(not(target_os = "macos"))]
    create_read_only_command(provider, exe, args, cwd, home, None, None)
}

pub fn create_read_only_command(
    provider: ProviderId, exe: &Path, args: &[String], worktree: &str, home: Option<&str>,
    app_data: Option<&str>, forced_port: Option<u16>,
) -> Result<tokio::process::Command, String> {
    let profile = provider_profile(provider);
    #[cfg(target_os = "macos")]
    {
        let mut sbpl = build_sbpl_for_turn(provider, &profile, worktree, None, home, app_data, forced_port)?;
        let path = Path::new(worktree).canonicalize().map_err(|e| e.to_string())?;
        let paths = [worktree.to_string(), path.to_string_lossy().into_owned()];
        for path in paths {
            let escaped = path.replace('\\', "\\\\").replace('"', "\\\"");
            sbpl.push_str(&format!("\n(deny file-write* (subpath \"{escaped}\"))\n"));
        }
        let mut cmd = tokio::process::Command::new("/usr/bin/sandbox-exec");
        cmd.arg("-p").arg(sbpl).arg(exe).args(args);
        return Ok(cmd);
    }
    #[cfg(target_os = "linux")]
    {
        let _ = forced_port;
        if let Some(bwrap) = bwrap::find_bwrap() {
            let mut argv = build_bwrap_args_for_turn(provider, &profile, worktree, None, home, app_data, exe, args)?;
            let index = argv.iter().position(|s| s == "--").ok_or("Invalid bubblewrap argv")?;
            argv.splice(index..index, ["--ro-bind".into(), worktree.into(), worktree.into()]);
            let mut cmd = tokio::process::Command::new(bwrap);
            cmd.args(argv);
            return Ok(cmd);
        }
        if landlock::probe_landlock_supported() {
            let mut rules = build_landlock_profile_for_turn(provider, &profile, worktree, None, home, app_data)?;
            let workspace = Path::new(worktree).canonicalize().map_err(|e| e.to_string())?;
            rules.allowed_writes.retain(|path| {
                let path = path.canonicalize().unwrap_or_else(|_| path.clone());
                !workspace.starts_with(&path) && !path.starts_with(&workspace)
            });
            let mut cmd = tokio::process::Command::new(exe);
            cmd.args(args);
            unsafe { cmd.pre_exec(move || landlock::apply_landlock(&rules)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))); }
            return Ok(cmd);
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let _ = (provider, profile, exe, args, worktree, home, app_data, forced_port);
    #[allow(unreachable_code)]
    Err(format!("{provider:?} read-only roles require an available OS sandbox"))
}

/// Like [`create_sandboxed_command`] but using [`gate_profile`] (docs/fix-todo F5.f).
///
/// Unlike the CLI variant this never falls back silently: `Err` is returned when the sandbox
/// cannot be built or no mechanism is available on this host, so the caller can run the command
/// unsandboxed and surface that fact in the gate result.
pub fn create_sandboxed_gate_command(
    exe: &Path,
    args: &[String],
    worktree: &str,
    home: Option<&str>,
    app_data: Option<&str>,
) -> Result<tokio::process::Command, String> {
    wrap_command_with_profile(
        ProviderId::ClaudeCode,
        &gate_profile(),
        exe,
        args,
        worktree,
        home,
        app_data,
        None,
    )?
    .ok_or_else(|| "no OS sandbox available on this platform".to_string())
}

#[cfg(test)]
mod tests;

