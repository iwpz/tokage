//! Provider detection (docs/01 §8): which → --version → login state → model list.
//! Automatic detection never sends a prompt. Explicit verification functions are user-initiated
//! model requests; credential fingerprints remain in memory and are never displayed.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::Serialize;

pub use tokage_core::{ProviderId, ProviderStatus};

use crate::{
    chat::adapter::{self, Caps},
    env::ShellEnv,
    install::{self, InstallMethod, Toolbox},
    models::{self, ModelGroup},
    usage::{self, Account},
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    pub id: ProviderId,
    pub name: &'static str,
    pub executable: &'static str,
    /// Shown by default in CLI management; the rest of the whitelist is opt-in.
    pub featured: bool,
    pub status: ProviderStatus,
    pub version: Option<String>,
    pub path: Option<String>,
    /// How the CLI is authenticated, e.g. "ChatGPT", "claude.ai", "api_key", "DEEPSEEK_API_KEY".
    pub auth_method: Option<String>,
    pub models: Vec<ModelInfo>,
    /// `models` with per-mode entries (effort / thinking / fast) merged, for display.
    pub model_groups: Vec<ModelGroup>,
    pub models_source: Option<ModelsSource>,
    /// Billing channel, plan and quota, for CLIs where they can be read locally.
    pub account: Option<Account>,
    /// What the user should do next, most important first. Rendered by the UI per `code`.
    pub guidance: Vec<Guidance>,
    pub install_methods: Vec<InstallMethod>,
    /// Flags Tokage adds to pass workspace trust, with `<workspace>` as the path placeholder.
    /// Shown in the consent dialog; empty when the CLI has no such flag.
    pub trust_args: Vec<String>,
    /// Tokage has a headless adapter for this CLI, so it can run conversation turns.
    pub chat: bool,
    pub usage_query: crate::usage_query::contract::QueryCapability,
    /// The adapter's capability bits and tested versions; `None` when `chat` is false.
    pub adapter: Option<AdapterInfo>,
    /// How Direct mode is restricted on this CLI until approvals exist (docs/11 §4.2).
    pub direct_limit: Option<crate::chat::DirectLimit>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterInfo {
    pub caps: Caps,
    /// Tested version range, e.g. ">= 2.1.288, < 3".
    pub versions: String,
    /// Version the adapter's recorded sample comes from.
    pub tested: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ModelsSource {
    /// Read from the CLI's own local model catalog / cache.
    Cli,
    /// Aliases the CLI documents; actual availability depends on the account.
    Builtin,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GuidanceCode {
    Install,
    Login,
    SetApiKey,
    VerifyLogin,
    ThirdPartyBackend,
    NoModels,
    Timeout,
    /// Installed version is outside the adapter's tested range; turns still run.
    UnsupportedVersion,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Guidance {
    pub code: GuidanceCode,
    /// Copyable command, if one applies. Never executed by Tokage.
    pub command: Option<String>,
}

fn guide(code: GuidanceCode, command: Option<&str>) -> Guidance {
    Guidance {
        code,
        command: command.map(str::to_string),
    }
}

/// How a whitelisted CLI's login state is checked without sending a prompt.
enum Auth {
    /// Dedicated probe function (Claude Code, Codex, Antigravity, Copilot, Aider).
    Custom,
    /// A status subcommand: exit 0 means signed in.
    Status(&'static [&'static str]),
    /// Presence of credential files (relative to HOME) or env vars. `definitive` means a
    /// missing file really means signed out; otherwise the CLI may use the OS keychain.
    Files {
        files: &'static [&'static str],
        env: &'static [&'static str],
        definitive: bool,
    },
}

/// One entry of the provider whitelist. Only CLIs listed here are ever detected or run.
struct Spec {
    id: ProviderId,
    name: &'static str,
    /// Candidate executable names, tried in order.
    exes: &'static [&'static str],
    featured: bool,
    auth: Auth,
    /// Copyable login command shown when signed out.
    login: Option<&'static str>,
    /// Subcommand that lists models (catalog query only, no prompt).
    models: Option<&'static [&'static str]>,
}

const fn spec(id: ProviderId, name: &'static str, exes: &'static [&'static str]) -> Spec {
    Spec {
        id,
        name,
        exes,
        featured: false,
        auth: Auth::Custom,
        login: None,
        models: None,
    }
}

const fn files(files: &'static [&'static str], env: &'static [&'static str]) -> Auth {
    Auth::Files {
        files,
        env,
        definitive: false,
    }
}

/// Sources: each vendor's CLI docs (see install.rs for URLs). Behaviour of status and model
/// subcommands that vendors don't document precisely is handled defensively below.
const REGISTRY: &[Spec] = &[
    Spec {
        featured: true,
        ..spec(ProviderId::ClaudeCode, "Claude Code", &["claude"])
    },
    Spec {
        featured: true,
        ..spec(ProviderId::CodexCli, "Codex CLI", &["codex"])
    },
    Spec {
        featured: true,
        auth: Auth::Status(&["status"]),
        login: Some("cursor-agent login"),
        models: Some(&["models"]),
        // `agent` is the new primary name; `cursor-agent` is less likely to collide.
        ..spec(
            ProviderId::CursorCli,
            "Cursor CLI",
            &["cursor-agent", "agent"],
        )
    },
    Spec {
        featured: true,
        ..spec(ProviderId::AntigravityCli, "Antigravity CLI", &["agy"])
    },
    Spec {
        featured: true,
        login: Some("copilot login"),
        ..spec(ProviderId::CopilotCli, "GitHub Copilot CLI", &["copilot"])
    },
    Spec {
        featured: true,
        login: Some("export DEEPSEEK_API_KEY=<your key>"),
        ..spec(ProviderId::Aider, "Aider", &["aider"])
    },
    Spec {
        featured: true,
        login: Some("opencode auth login"),
        models: Some(&["models"]),
        ..spec(ProviderId::Opencode, "OpenCode", &["opencode"])
    },
    Spec {
        // Hidden by default; user adds from the CLI picker (same pattern as Grok / Kimi).
        login: Some("reasonix setup"),
        ..spec(ProviderId::Reasonix, "Reasonix", &["reasonix"])
    },
    Spec {
        auth: Auth::Files {
            files: &[".grok/auth.json"],
            env: &["XAI_API_KEY"],
            definitive: true,
        },
        login: Some("grok login"),
        models: Some(&["models"]),
        ..spec(ProviderId::GrokCli, "Grok Build", &["grok"])
    },
    Spec {
        auth: Auth::Status(&["whoami", "--format", "json"]),
        login: Some("kiro-cli login"),
        models: Some(&["chat", "--list-models", "--format", "json"]),
        ..spec(ProviderId::KiroCli, "Kiro CLI", &["kiro-cli"])
    },
    Spec {
        auth: files(
            &[],
            &[
                "DASHSCOPE_API_KEY",
                "BAILIAN_CODING_PLAN_API_KEY",
                "OPENAI_API_KEY",
            ],
        ),
        login: Some("qwen"),
        ..spec(ProviderId::QwenCode, "Qwen Code", &["qwen"])
    },
    Spec {
        auth: files(&[".kimi-code/config.toml"], &[]),
        login: Some("kimi login"),
        models: Some(&["provider", "catalog", "list", "--json"]),
        ..spec(ProviderId::KimiCode, "Kimi Code", &["kimi"])
    },
    Spec {
        auth: Auth::Status(&["status"]),
        login: Some("qoder login"),
        models: Some(&["--list-models"]),
        ..spec(ProviderId::QoderCli, "Qoder CLI", &["qoder", "qodercli"])
    },
    Spec {
        auth: files(&[], &["AMP_API_KEY"]),
        login: Some("amp login"),
        ..spec(ProviderId::Amp, "Amp", &["amp"])
    },
    Spec {
        auth: files(
            &[".local/share/crush/crush.json"],
            &["ANTHROPIC_API_KEY", "OPENAI_API_KEY"],
        ),
        login: Some("crush"),
        ..spec(ProviderId::Crush, "Crush", &["crush"])
    },
    Spec {
        auth: files(&[".config/goose/config.yaml"], &["GOOSE_PROVIDER"]),
        login: Some("goose configure"),
        ..spec(ProviderId::Goose, "Goose", &["goose"])
    },
    Spec {
        auth: files(&[".cline/data"], &[]),
        login: Some("cline auth"),
        ..spec(ProviderId::Cline, "Cline CLI", &["cline"])
    },
    Spec {
        auth: files(&[], &["FACTORY_API_KEY"]),
        login: Some("droid"),
        ..spec(ProviderId::Droid, "Factory Droid", &["droid"])
    },
    Spec {
        auth: files(&[], &["AUGMENT_SESSION_AUTH"]),
        login: Some("auggie login"),
        models: Some(&["models", "list", "--json"]),
        ..spec(ProviderId::Auggie, "Augment Auggie", &["auggie"])
    },
    Spec {
        auth: files(&[], &["META_API_KEY"]),
        login: Some("muse"),
        ..spec(ProviderId::MuseCode, "Muse Code", &["muse"])
    },
    Spec {
        auth: Auth::Status(&["login", "status"]),
        login: Some("traecli login"),
        ..spec(ProviderId::TraeCli, "TraeCode CLI", &["traecli"])
    },
];

/// Result of the per-provider auth + model probe.
#[derive(Default)]
struct Probe {
    status: Option<ProviderStatus>,
    auth_method: Option<String>,
    models: Vec<ModelInfo>,
    /// Set by probes whose catalog carries mode metadata; otherwise derived from `models`.
    model_groups: Option<Vec<ModelGroup>>,
    models_source: Option<ModelsSource>,
    account: Option<Account>,
    guidance: Vec<Guidance>,
}

/// Resolve only an installed executable; usage refresh never runs a general scan.
pub fn usage_executable(id:ProviderId,env:&ShellEnv,custom:Option<&str>)->Option<std::path::PathBuf>{
    let spec=REGISTRY.iter().find(|s|s.id==id)?;
    if let Some(path)=custom.filter(|s|!s.trim().is_empty()){
        let path=Path::new(path);return path.is_file().then(||path.to_path_buf());
    }
    spec.exes.iter().find_map(|name|which::which_in(name,env.path(),std::env::temp_dir()).ok())
}

pub async fn detect_all(env: &ShellEnv, custom: &HashMap<ProviderId, String>) -> Vec<ProviderInfo> {
    detect_each(env, custom, |_| {}).await
}

/// Re-detect one CLI without performing a dedicated online usage query.
pub async fn detect_one(
    env: &ShellEnv,
    id: ProviderId,
    custom: Option<&str>,
) -> Option<ProviderInfo> {
    let spec = REGISTRY.iter().find(|s| s.id == id)?;
    let custom = custom.map(str::trim).filter(|s| !s.is_empty());
    let info = detect_with_timeout(spec, env, &Toolbox::detect(env), custom).await;
    Some(info)
}

/// Detects every whitelisted CLI concurrently and calls `on_each` as soon as each one
/// finishes, so the UI can update row by row. Returns the full list in registry order.
pub async fn detect_each(
    env: &ShellEnv,
    custom: &HashMap<ProviderId, String>,
    mut on_each: impl FnMut(&ProviderInfo),
) -> Vec<ProviderInfo> {
    use futures::stream::{FuturesUnordered, StreamExt};

    let tools = &Toolbox::detect(env);
    let mut pending: FuturesUnordered<_> = REGISTRY
        .iter()
        .enumerate()
        .map(|(i, spec)| {
            let custom = custom
                .get(&spec.id)
                .map(|s| s.trim())
                .filter(|s| !s.is_empty());
            async move { (i, detect_with_timeout(spec, env, tools, custom).await) }
        })
        .collect();
    let mut out: Vec<Option<ProviderInfo>> = vec![None; REGISTRY.len()];
    while let Some((i, info)) = pending.next().await {
        on_each(&info);
        out[i] = Some(info);
    }
    out.into_iter().flatten().collect()
}

async fn detect_with_timeout(
    spec: &Spec,
    env: &ShellEnv,
    tools: &Toolbox,
    custom: Option<&str>,
) -> ProviderInfo {
    match tokio::time::timeout(Duration::from_secs(40), detect(spec, env, tools, custom)).await {
        Ok(info) => info,
        Err(_) => {
            let mut i = base(spec, tools, ProviderStatus::Degraded, None, None);
            i.guidance.push(guide(GuidanceCode::Timeout, None));
            i
        }
    }
}

async fn detect(
    spec: &Spec,
    env: &ShellEnv,
    tools: &Toolbox,
    custom: Option<&str>,
) -> ProviderInfo {
    let Some((exe, name)) = resolve(spec, env, custom) else {
        let mut i = base(spec, tools, ProviderStatus::NotInstalled, None, None);
        i.guidance.push(guide(GuidanceCode::Install, None));
        return i;
    };
    let (version, probe) = tokio::join!(version(&exe, env), probe(spec, &exe, env));
    let mut i = base(
        spec,
        tools,
        probe.status.unwrap_or(ProviderStatus::UnknownAuth),
        version,
        Some(exe.display().to_string()),
    );
    i.executable = name;
    i.auth_method = probe.auth_method;
    i.model_groups = probe
        .model_groups
        .unwrap_or_else(|| models::group(&probe.models));
    // Large third-party catalogs come in no useful order; Claude Code and Codex are curated.
    if matches!(spec.id, ProviderId::CursorCli | ProviderId::CopilotCli) {
        models::sort(&mut i.model_groups);
    }
    i.models = probe.models;
    i.models_source = probe.models_source;
    i.account = probe.account;
    i.guidance = probe.guidance;
    if i.chat {
        i.direct_limit = crate::chat::direct_limit(spec.id, env, None);
    }
    check_version(&mut i);
    i
}

/// Marks a signed-in CLI whose version is outside its adapter's tested range (docs/01 §8).
/// It stays usable; output the parser doesn't recognise is skipped.
fn check_version(i: &mut ProviderInfo) {
    let Some(a) = adapter::adapter(i.id) else {
        return;
    };
    let outside = i.version.as_deref().and_then(|v| a.meta().versions.contains(v)) == Some(false);
    if outside && matches!(i.status, ProviderStatus::Ready) {
        i.status = ProviderStatus::UnsupportedVersion;
        i.guidance.insert(0, guide(GuidanceCode::UnsupportedVersion, None));
    }
}

/// Per-invocation flags that hand a workspace the user trusted in Tokage to the CLI, so it
/// runs there without its own trust prompt. Nothing is written to the CLI's config.
/// Sources: each CLI's `--help` (Claude Code 2.1, Codex, Cursor Agent, Copilot CLI 1.0.91).
/// CLIs whose flag isn't verified get none and rely on the working directory alone.
pub fn trust_args(id: ProviderId, workspace: &str) -> Vec<String> {
    let w = workspace.to_string();
    match id {
        // `-p` already skips the trust dialog; `--add-dir` scopes file tools to the workspace.
        ProviderId::ClaudeCode => vec!["--add-dir".into(), w],
        // Working root comes from the process cwd (`exec resume` has no `-C`).
        ProviderId::CodexCli => vec!["--skip-git-repo-check".into()],
        ProviderId::CursorCli => vec!["--trust".into(), "--workspace".into(), w],
        // Also loads the workspace's .github/skills and .github/agents as trusted.
        ProviderId::CopilotCli => vec!["--add-dir".into(), w],
        ProviderId::AntigravityCli => vec![],
        // Both generations use process cwd; neither accepts --workspace.
        ProviderId::Opencode => vec![],
        // Workspace root is passed as `--dir` in the adapter launch args.
        ProviderId::Reasonix => vec![],
        _ => vec![],
    }
}

/// First whitelisted executable name found on PATH (or the user's custom path).
fn resolve(spec: &Spec, env: &ShellEnv, custom: Option<&str>) -> Option<(PathBuf, &'static str)> {
    if let Some(p) = custom {
        let p = PathBuf::from(p);
        return p.is_file().then_some((p, spec.exes[0]));
    }
    let cwd = std::env::current_dir().ok()?;
    spec.exes.iter().find_map(|name| {
        which::which_in(name, env.path(), &cwd)
            .ok()
            .map(|p| (p, *name))
    })
}

/// Runs a short, read-only CLI command. Returns (success, stdout + stderr) or None on
/// spawn error / timeout. Some CLIs (e.g. `codex login status`) report on stderr.
async fn run(exe: &Path, args: &[&str], env: &ShellEnv, secs: u64) -> Option<(bool, String)> {
    let out = tokio::process::Command::new(exe)
        .args(args)
        .env_clear()
        .envs(&env.vars)
        .env("NO_COLOR", "1")
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    let out = tokio::time::timeout(Duration::from_secs(secs), out)
        .await
        .ok()?
        .ok()?;
    // Some CLIs (e.g. `codex login status`) report on stderr, so keep both streams.
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Some((out.status.success(), text))
}

/// Authentication must be visible under the same environment and sandbox as a turn.
/// Running outside the sandbox can see keychain credentials a turn cannot access.
pub(crate) fn auth_command(id: ProviderId, exe: &Path, args: &[&str], env: &ShellEnv) -> Option<tokio::process::Command> {
    let cwd = std::env::temp_dir();
    let args_owned: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let sandboxed = (cfg!(target_os = "macos") || cfg!(target_os = "linux"))
        && adapter::adapter(id).is_some_and(|a| a.meta().caps.has(Caps::OS_SANDBOX));
    let mut cmd = if sandboxed {
        crate::sandbox::create_sandboxed_command(id, exe, &args_owned,
            &cwd.to_string_lossy(), env.vars.get("HOME").map(String::as_str), None, None).ok()?
    } else {
        let mut cmd = tokio::process::Command::new(exe);
        cmd.args(args);
        cmd
    };
    let remove = if id == ProviderId::ClaudeCode {
        crate::chat::CLAUDE_HOST_SESSION_VARS.iter().map(|s| s.to_string()).collect()
    } else { Vec::new() };
    crate::chat::apply_env(&mut cmd, id, env, &remove, None);
    cmd.current_dir(cwd).kill_on_drop(true);
    Some(cmd)
}
pub(crate) async fn run_auth(id: ProviderId, exe: &Path, args: &[&str], env: &ShellEnv, secs: u64) -> Option<(bool, String)> {
    let mut cmd = auth_command(id, exe, args, env)?;
    cmd.stdin(std::process::Stdio::null());
    let out = tokio::time::timeout(Duration::from_secs(secs), cmd.output()).await.ok()?.ok()?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Some((out.status.success(), text))
}

fn explicitly_signed_out(out: &str) -> bool {
    let out = out.to_ascii_lowercase();
    ["not logged in", "not signed in", "logged out", "authentication required", "unauthenticated",
        "oauth token revoked", "token expired", "invalid api key"]
        .iter().any(|s| out.contains(s))
}

#[cfg(test)]
mod baseline_auth_tests;

fn status_confirms_login(id: ProviderId, ok: bool, out: &str) -> bool {
    ok && !explicitly_signed_out(out)
        && match id {
            ProviderId::CursorCli => signed_in_as(out).is_some(),
            ProviderId::CodexCli => out.lines().any(|l| l.trim().starts_with("Logged in using ")),
            _ => true,
        }
}

#[cfg(test)]
fn claude_confirms_login(ok: bool, out: &str, v: Option<&serde_json::Value>) -> bool {
    ok && !explicitly_signed_out(out)
        && v.and_then(|v| v.get("loggedIn")).and_then(|v| v.as_bool()) == Some(true)
        && v.and_then(|v| v.get("authMethod")).and_then(|v| v.as_str())
            .is_some_and(|m| !m.is_empty() && m != "none")
}

fn claude_config_auth(settings: &usage::ClaudeSettings) -> Option<&'static str> {
    settings.cloud.or_else(|| settings.has_token.then_some(
        if settings.base_url.is_some() { "gateway" } else { "api_key" }))
}

async fn version(exe: &Path, env: &ShellEnv) -> Option<String> {
    // Generous: interpreted CLIs (Python, Node) start slowly while the scan runs them in parallel.
    let (_, text) = run(exe, &["--version"], env, 25).await?;
    // Pick the first token that looks like a semver, e.g. "2.1.0 (Claude Code)" or "codex-cli 0.40.0".
    text.split_whitespace()
        // "v1.2.3", "1.0.91." (Copilot), "(2.1.0)" → bare version.
        .map(|t| {
            t.trim_start_matches(['v', '('])
                .trim_end_matches(['.', ',', ')', ':'])
        })
        .find(|t| t.chars().next().is_some_and(|c| c.is_ascii_digit()) && t.contains('.'))
        .map(str::to_string)
}

fn home(env: &ShellEnv) -> Option<PathBuf> {
    env.vars
        .get("HOME")
        .or_else(|| env.vars.get("USERPROFILE"))
        .map(PathBuf::from)
}

fn has_env(env: &ShellEnv, key: &str) -> bool {
    env.vars.get(key).is_some_and(|v| !v.trim().is_empty())
}

/// Every probe here is local: status subcommands, cache files, env var names.
/// None of them sends a prompt, so no tokens are spent.
async fn probe(spec: &Spec, exe: &Path, env: &ShellEnv) -> Probe {
    match spec.id {
        ProviderId::ClaudeCode => probe_claude(exe, env).await,
        ProviderId::CodexCli => probe_codex(exe, env).await,
        ProviderId::AntigravityCli => probe_antigravity(exe, env).await,
        ProviderId::Aider => probe_aider(exe, env).await,
        ProviderId::Opencode => probe_opencode(exe, env).await,
        ProviderId::Reasonix => probe_reasonix(exe, env).await,
        ProviderId::CopilotCli => probe_copilot(exe, env).await,
        ProviderId::KiroCli => probe_kiro(exe, env).await,
        ProviderId::CursorCli => probe_cursor(exe, env).await,
        _ => probe_generic(spec, exe, env).await,
    }
}

/// Ask the CLI rather than reading its version-dependent credential storage.
/// V2 moved credentials from auth.json to SQLite; neither storage is proof of a valid key.
async fn probe_opencode(exe: &Path, env: &ShellEnv) -> Probe {
    let mut p = Probe::default();
    let auth = run(exe, &["auth", "list", "--format", "json"], env, 15).await;
    let mut ids = auth.and_then(|(ok, text)| ok.then(|| opencode_connected_ids(&text)).flatten());
    if ids.is_none() {
        // V1 has no --format flag (some releases print help with exit 0).
        ids = match run(exe, &["auth", "list"], env, 15).await {
            Some((true, text)) if text.contains("Credentials") => opencode_legacy_ids(env),
            _ => None,
        };
    }
    let Some(ids) = ids else {
        p.status = Some(ProviderStatus::UnknownAuth);
        p.guidance
            .push(guide(GuidanceCode::VerifyLogin, Some("opencode auth list")));
        return p;
    };
    if ids.is_empty() {
        p.status = Some(ProviderStatus::NeedsLogin);
        p.guidance
            .push(guide(GuidanceCode::Login, Some("opencode auth login")));
        return p;
    }
    p.auth_method = Some(format!("stored: {}", ids.join(", ")));
    // This is local CLI-confirmed configuration, not a successful inference request.
    p.status = Some(ProviderStatus::Ready);
    if let Some((true, text)) = run(exe, &["models"], env, 15).await {
        p.models = parse_model_list(&text)
            .into_iter()
            .filter(|m| {
                m.id.split_once('/')
                    .is_some_and(|(prefix, _)| ids.iter().any(|id| id == prefix))
            })
            .collect();
        p.models_source = (!p.models.is_empty()).then_some(ModelsSource::Cli);
    }
    p
}

fn opencode_legacy_ids(env: &ShellEnv) -> Option<Vec<String>> {
    let data = env
        .vars
        .get("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| home(env).map(|h| h.join(".local/share")))?;
    let path = data.join("opencode/auth.json");
    if !path.exists() {
        return Some(vec![]);
    }
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    let rows = value.as_object()?;
    let mut ids: Vec<String> = rows
        .iter()
        .filter_map(|(id, auth)| {
            let nonempty = |key| {
                auth.get(key)
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| !s.trim().is_empty())
            };
            let configured = match auth.get("type").and_then(|v| v.as_str()) {
                Some("api") => nonempty("key"),
                Some("oauth") => {
                    nonempty("access")
                        && auth
                            .get("expires")
                            .and_then(|v| v.as_u64())
                            .is_some_and(|t| t > verification_now() * 1000)
                }
                _ => false,
            };
            configured.then(|| id.clone())
        })
        .collect();
    ids.sort();
    Some(ids)
}

fn opencode_connected_ids(text: &str) -> Option<Vec<String>> {
    let rows: Vec<serde_json::Value> = serde_json::from_str(text).ok()?;
    let mut ids = Vec::new();
    for row in rows {
        let id = row.get("id")?.as_str()?;
        let connections = row.get("connections")?.as_array()?;
        if !id.is_empty()
            && connections.iter().any(|c| {
                c.get("type").and_then(|v| v.as_str()) == Some("credential")
                    && c.get("id")
                        .and_then(|v| v.as_str())
                        .is_some_and(|id| !id.is_empty())
            })
        {
            ids.push(id.to_string());
        }
    }
    ids.sort();
    ids.dedup();
    Some(ids)
}

/// Cursor status reports stored account login even when CURSOR_API_KEY is invalid.
/// With an explicit key, validate via the authenticated model catalog, not that account.
async fn probe_cursor(exe: &Path, env: &ShellEnv) -> Probe {
    let (status, about) = tokio::join!(
        run_auth(ProviderId::CursorCli, exe, &["status", "--format", "json"], env, 12),
        run_auth(ProviderId::CursorCli, exe, &["about", "--format", "json"], env, 8)
    );
    let mut p = Probe::default();
    let keyed = has_env(env, "CURSOR_API_KEY");
    let custom_endpoint = has_env(env, "CURSOR_API_ENDPOINT");
    if !keyed {
        match status {
            Some((ok, out)) => {
                let (status, method) = cursor_login_status(ok, &out);
                p.status = Some(status);
                p.auth_method = method;
            }
            None => p.status = Some(ProviderStatus::UnknownAuth),
        }
    }
    if keyed || p.status == Some(ProviderStatus::Ready) {
        match run_auth(ProviderId::CursorCli, exe, &["models"], env, 15).await {
            Some((ok, out)) => {
                let models = parse_model_list(&out);
                if ok && !cursor_auth_failure(&out) && !models.is_empty() {
                    p.models = models;
                    p.models_source = Some(ModelsSource::Cli);
                    if keyed {
                        p.status = Some(ProviderStatus::Ready);
                        p.auth_method = Some("CURSOR_API_KEY".into());
                    }
                } else if cursor_auth_failure(&out) {
                    p.status = Some(ProviderStatus::NeedsLogin);
                    p.auth_method = None;
                } else if keyed || custom_endpoint {
                    p.status = Some(ProviderStatus::UnknownAuth);
                } else {
                    p.guidance.push(guide(GuidanceCode::NoModels, Some("cursor-agent models")));
                }
            }
            None if keyed || custom_endpoint => p.status = Some(ProviderStatus::UnknownAuth),
            None => p.guidance.push(guide(GuidanceCode::NoModels, Some("cursor-agent models"))),
        }
    }
    match p.status {
        Some(ProviderStatus::Ready) => {
            let about = about.filter(|(ok, _)| *ok).map(|(_, t)| t);
            p.account = Some(usage::cursor_account(env, about.as_deref()));
            if p.account.as_ref().is_some_and(|a| !a.official && a.endpoint.is_some()) {
                p.auth_method = Some("gateway".into());
                p.guidance.push(guide(GuidanceCode::ThirdPartyBackend, None));
            }
        }
        Some(ProviderStatus::NeedsLogin) => p.guidance.push(guide(GuidanceCode::Login, Some("cursor-agent login"))),
        _ => p.guidance.push(guide(GuidanceCode::VerifyLogin, Some("cursor-agent status --format json"))),
    }
    p
}

fn cursor_auth_failure(out: &str) -> bool {
    let lower = out.to_ascii_lowercase();
    explicitly_signed_out(out) || lower.contains("api key is invalid")
        || lower.contains("invalid api key") || lower.contains("unauthorized")
}

fn cursor_login_status(ok: bool, out: &str) -> (ProviderStatus, Option<String>) {
    let json = out.find('{').zip(out.rfind('}')).map(|(a, b)| &out[a..=b]);
    let value = json.and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());
    if let Some(v) = value {
        let flag = v.get("isAuthenticated").and_then(|v| v.as_bool());
        if flag == Some(false) || matches!(v["status"].as_str(), Some("unauthenticated" | "logged_out")) {
            return (ProviderStatus::NeedsLogin, None);
        }
        if ok && flag == Some(true) && v["status"] == "authenticated" {
            let identity = v["userInfo"]["email"].as_str().filter(|s| !s.trim().is_empty())
                .unwrap_or("Cursor account").to_string();
            return (ProviderStatus::Ready, Some(identity));
        }
        return (ProviderStatus::UnknownAuth, None);
    }
    if cursor_auth_failure(out) { return (ProviderStatus::NeedsLogin, None); }
    if ok {
        if let Some(identity) = signed_in_as(out) { return (ProviderStatus::Ready, Some(identity)); }
    }
    (ProviderStatus::UnknownAuth, None)
}

/// Table-driven probe for the rest of the whitelist.
async fn probe_generic(spec: &Spec, exe: &Path, env: &ShellEnv) -> Probe {
    let mut p = Probe::default();
    let signed_out = |p: &mut Probe, status| {
        p.status = Some(status);
        let code = if matches!(status, ProviderStatus::NeedsLogin) {
            GuidanceCode::Login
        } else {
            GuidanceCode::VerifyLogin
        };
        p.guidance.push(guide(code, spec.login));
    };
    match &spec.auth {
        Auth::Custom => p.status = Some(ProviderStatus::UnknownAuth),
        // Status commands often hit the network (Cursor's takes ~5s alone, far more while
        // other probes run), so they get a long timeout within the 40s per-provider budget.
        Auth::Status(args) => match run_auth(spec.id, exe, args, env, 25).await {
            Some((true, out)) if status_confirms_login(spec.id, true, &out) => {
                p.status = Some(ProviderStatus::Ready);
                p.auth_method = signed_in_as(&out);
            }
            Some((_, _)) => signed_out(&mut p, ProviderStatus::NeedsLogin),
            None => signed_out(&mut p, ProviderStatus::UnknownAuth),
        },
        Auth::Files {
            files,
            env: keys,
            definitive,
        } => {
            let key = keys.iter().find(|k| has_env(env, k));
            let file = home(env).is_some_and(|h| files.iter().any(|f| h.join(f).exists()));
            if let Some(k) = key {
                p.status = Some(ProviderStatus::Ready);
                p.auth_method = Some((*k).to_string());
            } else if file {
                p.status = Some(ProviderStatus::Ready);
            } else if *definitive {
                signed_out(&mut p, ProviderStatus::NeedsLogin);
            } else {
                // Credentials may live in the OS keychain, which we don't inspect.
                signed_out(&mut p, ProviderStatus::UnknownAuth);
            }
        }
    }
    if let (Some(args), Some(ProviderStatus::Ready)) = (spec.models, p.status) {
        if let Some((true, out)) = run(exe, args, env, 25).await {
            p.models = parse_model_list(&out);
            p.models_source = (!p.models.is_empty()).then_some(ModelsSource::Cli);
        }
    }
    if p.account.is_none() {
        if let Some(u) = usage::get_cached_usage(spec.id) {
            p.account = Some(usage::Account {
                billing: usage::Billing::Subscription,
                official: true,
                endpoint: None,
                plan: None,
                balance: None,
                usage: Some(u),
            });
        }
    }
    p
}

/// "✓ Logged in as someone@example.com" → "someone@example.com".
fn signed_in_as(out: &str) -> Option<String> {
    out.lines().find_map(|l| {
        let i = l.find("ogged in as ")?;
        let who = l[i + "ogged in as ".len()..].trim().trim_end_matches('.');
        (!who.is_empty()).then(|| who.to_string())
    })
}

/// Best-effort parser for model-list output: JSON (array of strings or objects with an
/// id-like field, possibly nested) or plain lines like `id`, `- id`, `id<TAB>Name`.
fn parse_model_list(text: &str) -> Vec<ModelInfo> {
    let mut out = Vec::new();
    if let Some(start) = text.find(['[', '{']) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(text[start..].trim()) {
            collect_json_models(&v, &mut out);
        }
    }
    if out.is_empty() {
        out = text
            .lines()
            .filter_map(|l| {
                let l = l.trim().trim_start_matches(['-', '*', '•']).trim();
                if l.ends_with(':') {
                    return None;
                }
                let (id, rest) = l.split_once(char::is_whitespace).unwrap_or((l, ""));
                // Cursor's router alias `auto - Auto` has no digit or separator to go by.
                let alias = id == "auto" && rest.trim_start().starts_with("- ");
                (alias || looks_like_model_id(id)).then(|| ModelInfo {
                    id: id.to_string(),
                    // Cursor prints `id - Name`.
                    // and tags the active one `(current, default)`, which isn't part of the name.
                    name: Some(strip_state_tag(rest.trim().trim_start_matches("- ").trim()))
                        .filter(|r| !r.is_empty())
                        .unwrap_or(id)
                        .to_string(),
                })
            })
            .collect();
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|m| seen.insert(m.id.clone()));
    out
}

/// "Auto (current, default)" → "Auto". Only state words are dropped, never mode words.
fn strip_state_tag(name: &str) -> &str {
    match name.rsplit_once(" (") {
        Some((head, tag))
            if tag.strip_suffix(')').is_some_and(|t| {
                t.split(',').all(|w| matches!(w.trim(), "current" | "default"))
            }) =>
        {
            head.trim_end()
        }
        _ => name,
    }
}

fn collect_json_models(v: &serde_json::Value, out: &mut Vec<ModelInfo>) {
    use serde_json::Value;
    match v {
        Value::Array(items) => {
            for item in items {
                match item {
                    Value::String(id) if looks_like_model_id(id) => out.push(ModelInfo {
                        id: id.clone(),
                        name: id.clone(),
                    }),
                    Value::Object(o) => {
                        let id = ["id", "slug", "model_id", "modelId", "model", "name"]
                            .iter()
                            .find_map(|k| o.get(*k).and_then(|x| x.as_str()))
                            .filter(|id| *id == "auto" || looks_like_model_id(id));
                        match id {
                            Some(id) => {
                                let name = ["display_name", "displayName", "name", "label"]
                                    .iter()
                                    .find_map(|k| o.get(*k).and_then(|x| x.as_str()))
                                    .unwrap_or(id);
                                out.push(ModelInfo {
                                    id: id.into(),
                                    name: name.into(),
                                });
                            }
                            None => collect_json_models(item, out),
                        }
                    }
                    _ => {}
                }
            }
        }
        Value::Object(o) => o.values().for_each(|x| collect_json_models(x, out)),
        _ => {}
    }
}

/// Rejects prose like "Fetching available models...": ids have a digit or a separator.
fn looks_like_model_id(s: &str) -> bool {
    (2..=96).contains(&s.len())
        && s.chars().any(|c| c.is_ascii_alphabetic())
        && s.chars().any(|c| c.is_ascii_digit() || "-./:".contains(c))
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._/:@".contains(c))
        && !s.ends_with("...")
        && !s.ends_with(':')
}

/// Read CLI-owned local authentication evidence without starting an OAuth refresh.
async fn probe_claude(exe: &Path, env: &ShellEnv) -> Probe {
    let mut p = Probe::default();
    let settings = usage::claude_settings(env);
    let mut subscription = None;
    // No short-lived `claude auth status` launch: some CLI versions can start a
    // refresh during initialization and exit before persisting the rotated grant.
    if let Some(method) = claude_config_auth(&settings) {
        p.status = Some(ProviderStatus::Ready);
        p.auth_method = Some(method.into());
    } else if let Some(profile) = crate::usage_query::credentials::claude(exe, env, b"local-status-only").await {
        subscription = profile.plan;
        p.status = Some(if profile.logged_in { ProviderStatus::Ready } else { ProviderStatus::NeedsLogin });
        p.auth_method = Some("claude.ai".into());
        if !profile.logged_in { p.guidance.push(guide(GuidanceCode::Login, Some("claude auth login"))); }
    } else {
        p.status = Some(ProviderStatus::UnknownAuth);
        p.guidance.push(guide(GuidanceCode::VerifyLogin, Some("claude")));
    }
    if matches!(p.status, Some(ProviderStatus::Ready)) {
        p.account = Some(usage::claude_account(
            &settings,
            p.auth_method.as_deref(),
            subscription.as_deref(),
            Some(env),
        ));
        p.models = claude_models(&usage::claude_model_sources(env));
        p.model_groups = Some(claude_model_groups(&p.models));
        p.models_source = Some(ModelsSource::Builtin);
    }
    p
}

/// Claude Code has no model-list command. The documented aliases are always offered
/// (`opus[1m]` / `sonnet[1m]` are the 1M-context ones); what its own config files add is
/// layered on top: the account's extra options (Fable), an `availableModels` allow-list, the
/// configured default, and what an alias resolves to when an env override pins it.
fn claude_models(src: &usage::ClaudeModelSources) -> Vec<ModelInfo> {
    let allowed = |id: &str| {
        let base = id.split('[').next().unwrap_or(id).to_ascii_lowercase();
        src.available.is_empty()
            // A full name in the list does not allow the alias, which may resolve elsewhere.
            || src.available.iter().any(|a| {
                a.split('[').next().unwrap_or(a).eq_ignore_ascii_case(&base)
            })
    };
    let mut out: Vec<ModelInfo> = Vec::new();
    for a in ["opus", "opus[1m]", "sonnet", "sonnet[1m]", "haiku"] {
        if !allowed(a) {
            continue;
        }
        let base = a.split('[').next().unwrap_or(a);
        let target = src.alias_targets.iter().find(|(k, _)| *k == base).map(|(_, v)| v);
        let mut name = match a.split_once('[') {
            Some((b, _)) => format!("{} 1M", capitalize(b)),
            None => capitalize(a),
        };
        if let Some(t) = target {
            // Shown in the picker's info tooltip.
            name = format!("{name} ({t})");
        }
        out.push(ModelInfo { id: a.into(), name });
    }
    let extras = src
        .extra
        .iter()
        .cloned()
        .chain(src.available.iter().map(|a| (a.clone(), a.clone())))
        .chain(src.default_model.iter().map(|m| (m.clone(), m.clone())));
    for (id, label) in extras {
        // `opus` / `sonnet[1m]` and friends are already listed.
        if out.iter().any(|m| m.id.eq_ignore_ascii_case(&id)) {
            continue;
        }
        out.push(ModelInfo { id, name: label });
    }
    out
}

/// Claude Code takes effort as a flag (`--effort`), not in the model id. Haiku has no
/// reasoning effort to set.
const CLAUDE_EFFORTS: &[&str] = &["default", "low", "medium", "high", "xhigh", "max"];

fn claude_model_groups(models: &[ModelInfo]) -> Vec<ModelGroup> {
    let mut groups = models::group(models);
    for g in &mut groups {
        if !g.key.to_ascii_lowercase().contains("haiku") {
            g.efforts = CLAUDE_EFFORTS.iter().map(|e| e.to_string()).collect();
            g.default_effort = Some("default".into());
        }
    }
    groups
}

/// `codex login status` exits 0 with "Logged in using …". Models come from the CLI's
/// own catalog cache (`$CODEX_HOME/models_cache.json`), which it refreshes itself.
async fn probe_codex(exe: &Path, env: &ShellEnv) -> Probe {
    let mut p = Probe::default();
    match run_auth(ProviderId::CodexCli, exe, &["login", "status"], env, 20).await {
        Some((true, out)) if status_confirms_login(ProviderId::CodexCli, true, &out) => {
            p.status = Some(ProviderStatus::Ready);
            p.auth_method = out
                .lines()
                .find_map(|l| l.trim().strip_prefix("Logged in using "))
                .map(|m| m.trim().to_string());
        }
        Some((_, _)) => {
            p.status = Some(ProviderStatus::NeedsLogin);
            p.guidance
                .push(guide(GuidanceCode::Login, Some("codex login")));
        }
        None => {
            p.status = Some(ProviderStatus::UnknownAuth);
            p.guidance
                .push(guide(GuidanceCode::VerifyLogin, Some("codex login status")));
        }
    }

    let codex_home = env
        .vars
        .get("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| home(env).map(|h| h.join(".codex")));
    let mut catalog = codex_home
        .and_then(|d| std::fs::read_to_string(d.join("models_cache.json")).ok())
        .unwrap_or_default();
    let mut models = parse_codex_models(&catalog);
    if models.is_empty() {
        if let Some((true, out)) = run(exe, &["debug", "models"], env, 8).await {
            models = parse_codex_models(&out);
            catalog = out;
        }
    }
    if matches!(p.status, Some(ProviderStatus::Ready)) {
        p.account = Some(usage::codex_account(env));
    }
    if !models.is_empty() {
        p.model_groups = Some(codex_model_groups(&catalog, &models));
        p.models = models;
        p.models_source = Some(ModelsSource::Cli);
    } else if matches!(p.status, Some(ProviderStatus::Ready)) {
        p.guidance
            .push(guide(GuidanceCode::NoModels, Some("codex")));
    }
    p
}

fn parse_codex_models(text: &str) -> Vec<ModelInfo> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return vec![];
    };
    let Some(list) = v.get("models").and_then(|m| m.as_array()) else {
        return vec![];
    };
    let mut rows: Vec<(i64, ModelInfo)> = list
        .iter()
        .filter(|m| m.get("visibility").and_then(|v| v.as_str()) != Some("hide"))
        .filter_map(|m| {
            let id = m.get("slug")?.as_str()?.to_string();
            let name = m
                .get("display_name")
                .and_then(|n| n.as_str())
                .unwrap_or(&id)
                .to_string();
            let prio = m
                .get("priority")
                .and_then(|p| p.as_i64())
                .unwrap_or(i64::MAX);
            Some((prio, ModelInfo { id, name }))
        })
        .collect();
    rows.sort_by_key(|(p, _)| *p);
    rows.into_iter().map(|(_, m)| m).collect()
}

/// Codex takes effort and speed as parameters, and its catalog lists them per model.
fn codex_model_groups(catalog: &str, models: &[ModelInfo]) -> Vec<ModelGroup> {
    let list = serde_json::from_str::<serde_json::Value>(catalog)
        .ok()
        .and_then(|v| v.get("models").cloned());
    models
        .iter()
        .map(|m| {
            let entry = list.as_ref().and_then(|l| l.as_array()).and_then(|l| {
                l.iter()
                    .find(|e| e.get("slug").and_then(|s| s.as_str()) == Some(&m.id))
            });
            let efforts: Vec<String> = entry
                .and_then(|e| e.get("supported_reasoning_levels")?.as_array().cloned())
                .unwrap_or_default()
                .iter()
                .filter_map(|l| l.get("effort")?.as_str().map(str::to_string))
                .collect();
            let default = entry
                .and_then(|e| e.get("default_reasoning_level")?.as_str())
                .map(str::to_string);
            let fast = entry
                .and_then(|e| e.get("additional_speed_tiers")?.as_array().cloned())
                .is_some_and(|t| t.iter().any(|x| x.as_str() == Some("fast")));
            models::single(m, &efforts, default, fast)
        })
        .collect()
}

/// Antigravity CLI (`agy`, Google's successor to Gemini CLI). `agy models` lists the models
/// the signed-in account can use; it only queries the catalog, no prompt is sent.
/// It fails when the user isn't signed in (or has no API key configured).
static ANTIGRAVITY_VERIFIED_KEYS: std::sync::Mutex<Vec<[u8; 32]>> = std::sync::Mutex::new(Vec::new());

fn antigravity_key_fingerprint(env: &ShellEnv) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    for name in ["HOME", "GEMINI_API_KEY", "GOOGLE_GEMINI_BASE_URL"] {
        let value = env.vars.get(name).map(String::as_str).unwrap_or("");
        hash.update((value.len() as u64).to_le_bytes());
        hash.update(value.as_bytes());
    }
    hash.finalize().into()
}

pub(crate) fn invalidate_antigravity_key(env: &ShellEnv) {
    if let Ok(mut keys) = ANTIGRAVITY_VERIFIED_KEYS.lock() { keys.retain(|k| *k != antigravity_key_fingerprint(env)); }
}

/// Explicit user-initiated check. Unlike routine detection, this consumes a small model turn.
pub async fn verify_antigravity_key(exe: &Path, env: &ShellEnv) -> Result<(), String> {
    if !antigravity_api_key_mode(env) || !has_env(env, "GEMINI_API_KEY") {
        return Err("Configure modelProvider=gemini and GEMINI_API_KEY first".into());
    }
    invalidate_antigravity_key(env);
    let result = run_auth(ProviderId::AntigravityCli, exe, &["-p", "Reply with exactly TOKAGE_KEY_OK. Do not use tools or read or write files.",
        "--model", "gemini-3.8-flash-low", "--mode", "plan", "--output-format", "json"], env, 45).await;
    let Some((ok, out)) = result else { return Err("Antigravity key verification timed out".into()); };
    let json = out.find('{').zip(out.rfind('}')).map(|(a,b)| &out[a..=b]);
    let v = json.and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());
    if !ok || v.as_ref().map_or(true, |v| v["status"] != "SUCCESS" || !v["response"].as_str().unwrap_or("").contains("TOKAGE_KEY_OK")) {
        // CLI errors can repeat credentials; keep diagnostics out of the IPC error.
        return Err("Antigravity API key verification failed; check CLI authentication or endpoint".into());
    }
    ANTIGRAVITY_VERIFIED_KEYS.lock().map_err(|_| "Verification cache unavailable")?.push(antigravity_key_fingerprint(env));
    Ok(())
}

async fn probe_antigravity(exe: &Path, env: &ShellEnv) -> Probe {
    let mut p = Probe::default();
    let keyed_mode = antigravity_api_key_mode(env);
    let models = run_auth(ProviderId::AntigravityCli, exe, &["models"], env, 12).await;
    if let Some((true, out)) = models {
        p.models = parse_model_list(&out);
        p.models_source = (!p.models.is_empty()).then_some(ModelsSource::Cli);
        if p.models.is_empty() { p.guidance.push(guide(GuidanceCode::NoModels, Some("agy models"))); }
    }
    if keyed_mode {
        if !has_env(env, "GEMINI_API_KEY") {
            p.status = Some(ProviderStatus::NeedsLogin);
            p.guidance.push(guide(GuidanceCode::SetApiKey, Some("export GEMINI_API_KEY=<your key>")));
        } else {
            // In BYOK mode `models` is offline and succeeds even for invalid keys.
            // Do not borrow a Google account's quota/login as proof of key validity.
            let verified = ANTIGRAVITY_VERIFIED_KEYS.lock().ok().is_some_and(|keys| keys.contains(&antigravity_key_fingerprint(env)));
            p.status = Some(if verified { ProviderStatus::Ready } else { ProviderStatus::UnknownAuth });
            p.auth_method = Some("Gemini API key".into());
            if !verified { p.guidance.push(guide(GuidanceCode::VerifyLogin, None)); }
            let mut account = usage::antigravity_account(env, true);
            account.usage = None; // Google subscription quotas do not apply to BYOK.
            p.account = Some(account);
            if p.account.as_ref().is_some_and(|a| !a.official) { p.auth_method = Some("gateway".into()); }
        }
    } else {
        // Bind the active CLI-owned OAuth grant; a catalog alone remains insufficient.
        p.status = Some(if crate::usage_query::credentials::antigravity(exe, env, b"local-status-only").await.is_some() { ProviderStatus::Ready } else { ProviderStatus::UnknownAuth });
        p.auth_method = Some("Google account".into());
        p.account = Some(usage::antigravity_account(env, false));
        p.guidance.push(guide(GuidanceCode::VerifyLogin, Some("agy")));
    }
    p
}

/// Copilot SDK's auth.getStatus + models.list through the installed CLI's stdio server.
/// No conversation is created and no model prompt is sent. Env-token precedence is explicit.
async fn probe_copilot(exe: &Path, env: &ShellEnv) -> Probe {
    let mut p = Probe::default();
    if has_env(env, "COPILOT_PROVIDER_BASE_URL") {
        p.status = Some(ProviderStatus::UnknownAuth);
        p.auth_method = Some("gateway".into());
        p.account = Some(usage::Account {
            billing: usage::Billing::PayAsYouGo, official: false,
            endpoint: env.vars.get("COPILOT_PROVIDER_BASE_URL").and_then(|s| usage::host_of(s)),
            plan: None, balance: None, usage: None,
        });
        if let Some(id) = env.vars.get("COPILOT_PROVIDER_MODEL_ID").or_else(|| env.vars.get("COPILOT_MODEL")).filter(|s| !s.trim().is_empty()) {
            p.models.push(ModelInfo { id: id.clone(), name: copilot_display_name(id) });
            p.models_source = Some(ModelsSource::Builtin);
        }
        return p;
    }
    match copilot_status_rpc(exe, env).await {
        Some((auth, models)) => {
            apply_copilot_rpc(&mut p, &auth, models.as_ref());
        }
        None => {
            p.status = Some(ProviderStatus::UnknownAuth);
            p.guidance.push(guide(GuidanceCode::VerifyLogin, Some("copilot login")));
        }
    }
    p
}

fn copilot_token_env(env: &ShellEnv) -> Option<&'static str> {
    ["COPILOT_GITHUB_TOKEN", "GH_TOKEN", "GITHUB_TOKEN"].into_iter().find(|k| has_env(env, k))
}

/// SDK stdio RPC: no session.create or session.send, so no model inference occurs.
pub(crate) async fn copilot_status_rpc(exe: &Path, env: &ShellEnv) -> Option<(serde_json::Value, Option<serde_json::Value>)> {
    copilot_management_rpc(exe, env, false).await
}
pub(crate) async fn copilot_quota_rpc(exe: &Path, env: &ShellEnv) -> Option<(serde_json::Value, Option<serde_json::Value>)> {
    copilot_management_rpc(exe, env, true).await
}
async fn copilot_management_rpc(exe: &Path, env: &ShellEnv, quota: bool) -> Option<(serde_json::Value, Option<serde_json::Value>)> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
    let mut args = vec!["--headless", "--stdio", "--no-auto-update"];
    // Headless doesn't automatically consume the CLI's env-token precedence.
    // Pass the variable name explicitly; an invalid override must never fall back to OAuth.
    if let Some(key) = copilot_token_env(env) { args.extend(["--auth-token-env", key, "--no-auto-login"]); }
    let mut cmd = auth_command(ProviderId::CopilotCli, exe, &args, env)?;
    cmd.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
    crate::proc::isolate(&mut cmd);
    let mut child = cmd.spawn().ok()?;
    let _group = crate::proc::Group::new(&child);
    let mut stdin = child.stdin.take()?;
    let mut stdout = tokio::io::BufReader::new(child.stdout.take()?);
    let result = tokio::time::timeout(Duration::from_secs(20), async {
        let mut auth = None;
        for (id, method) in [(1, "auth.getStatus"), (2, if quota { "account.getQuota" } else { "models.list" })] {
            let request = serde_json::json!({"jsonrpc":"2.0","id":id,"method":method,"params":{}}).to_string();
            stdin.write_all(format!("Content-Length: {}\r\n\r\n{}", request.len(), request).as_bytes()).await.ok()?;
            stdin.flush().await.ok()?;
            loop {
                let mut length = None;
                let mut header_bytes = 0;
                loop {
                    let mut line = String::new();
                    if stdout.read_line(&mut line).await.ok()? == 0 { return None; }
                    header_bytes += line.len();
                    if header_bytes > 8192 { return None; }
                    if line == "\r\n" || line == "\n" { break; }
                    if let Some((name, value)) = line.split_once(':') {
                        if name.eq_ignore_ascii_case("Content-Length") { length = value.trim().parse::<usize>().ok(); }
                    }
                }
                let length = length.filter(|n| *n <= 4 * 1024 * 1024)?;
                let mut body = vec![0; length];
                stdout.read_exact(&mut body).await.ok()?;
                let response: serde_json::Value = serde_json::from_slice(&body).ok()?;
                if response["id"].as_u64() != Some(id) {
                    // Do not answer server requests that might initiate authentication or tools.
                    if response.get("id").is_some() && response.get("method").is_some() { return None; }
                    continue;
                }
                if id == 1 {
                    let status = response.get("result")?.clone();
                    if status["isAuthenticated"].as_bool() != Some(true) { return Some((status, None)); }
                    auth = Some(status);
                } else {
                    return Some((auth?, response.get("result").cloned()));
                }
                break;
            }
        }
        None
    }).await.ok().flatten();
    drop(stdin);
    drop(stdout);
    let _ = child.kill().await;
    let _ = child.wait().await;
    result
}

fn apply_copilot_rpc(p: &mut Probe, auth: &serde_json::Value, catalog: Option<&serde_json::Value>) {
    match auth["isAuthenticated"].as_bool() {
        Some(false) => {
            p.status = Some(ProviderStatus::NeedsLogin);
            p.guidance.push(guide(GuidanceCode::Login, Some("copilot login")));
            return;
        }
        Some(true) => {},
        None => { p.status = Some(ProviderStatus::UnknownAuth); return; }
    }
    p.auth_method = auth["authType"].as_str().map(|t| match t {
        "user" => "GitHub OAuth".into(), "gh-cli" => "GitHub CLI (gh)".into(), _ => format!("GitHub ({t})"),
    });
    p.models = catalog.and_then(|v| v["models"].as_array()).map(|models| models.iter().filter_map(|m| {
        let id = m["id"].as_str()?.trim().to_string();
        if id.is_empty() { return None; }
        Some(ModelInfo { name: m["name"].as_str().map(str::to_owned).unwrap_or_else(|| copilot_display_name(&id)), id })
    }).collect()).unwrap_or_default();
    p.status = Some(if p.models.is_empty() { ProviderStatus::UnknownAuth } else { ProviderStatus::Ready });
    if p.models.is_empty() { p.guidance.push(guide(GuidanceCode::NoModels, Some("copilot login"))); }
    else { p.models_source = Some(ModelsSource::Cli); p.model_groups = Some(copilot_model_groups(&p.models)); }
    p.account = Some(usage::Account { billing: usage::Billing::Subscription, official: true,
        endpoint: None, plan: None, balance: None, usage: usage::get_cached_usage(ProviderId::CopilotCli) });
}

/// `--model auto` lets Copilot route each request; `--auto-tier` picks the routing profile.
/// Source: `copilot --help` and `copilot help config` (`autoTier`), Copilot CLI 1.0.91.
const COPILOT_AUTO_TIERS: &[&str] = &["efficiency", "balance", "intelligence"];

fn copilot_model_groups(models: &[ModelInfo]) -> Vec<ModelGroup> {
    let auto = ModelInfo {
        id: "auto".into(),
        name: "Auto".into(),
    };
    let mut groups = models::group(std::slice::from_ref(&auto));
    groups[0].tiers = COPILOT_AUTO_TIERS.iter().map(|t| t.to_string()).collect();
    groups[0].default_tier = Some("balance".into());
    groups.extend(models::group(models));
    groups
}

/// The `model` entry of `copilot help config` lists accepted ids as `- "id"` lines.
#[cfg(test)]
fn parse_copilot_models(text: &str) -> Vec<ModelInfo> {
    let mut seen = std::collections::HashSet::new();
    text.lines()
        .skip_while(|l| !l.trim_start().starts_with("`model`:"))
        .skip(1)
        .map_while(|l| {
            let id = l.trim().strip_prefix("- ")?.trim().trim_matches('"');
            looks_like_model_id(id).then(|| id.to_string())
        })
        .filter(|id| seen.insert(id.clone()))
        .map(|id| ModelInfo {
            name: copilot_display_name(&id),
            id,
        })
        .collect()
}

/// `claude-opus-4.8` → "Claude Opus 4.8": hyphens become spaces, words are capitalised.
/// Acronyms are upper-cased, and GPT keeps OpenAI's own spelling ("GPT-5.4 Mini").
fn copilot_display_name(id: &str) -> String {
    const ACRONYMS: &[&str] = &["gpt", "glm", "mai"];
    let words: Vec<String> = id
        .split('-')
        .filter(|w| !w.is_empty())
        .map(|w| {
            if ACRONYMS.contains(&w.to_ascii_lowercase().as_str()) {
                return w.to_ascii_uppercase();
            }
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect())
                .unwrap_or_default()
        })
        .collect();
    match words.split_first() {
        Some((first, rest)) if first == "GPT" && !rest.is_empty() => {
            format!("GPT-{}", rest.join(" "))
        }
        _ => words.join(" "),
    }
}

/// Headless setups put `"modelProvider": "gemini"` in the CLI's settings and export
/// GEMINI_API_KEY. The settings file holds no secrets; the key itself is never read.
pub(crate) fn antigravity_api_key_mode(env: &ShellEnv) -> bool {
    home(env).and_then(|h| std::fs::read_to_string(h.join(".gemini/antigravity-cli/settings.json")).ok())
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .is_some_and(|v| v.get("modelProvider").and_then(|m| m.as_str()) == Some("gemini"))
}

/// Reasonix: BYOK via `~/.reasonix/config.toml` (`api_key_env`) + `~/.reasonix/.env`.
/// `doctor` is local configuration evidence only; READY requires explicit model verification.
/// Billing: `reasonix doctor billing --json` (wallets when present; else PAYG currencies).
const REASONIX_VERIFICATION_MAX_AGE: u64 = 24 * 60 * 60;

fn reasonix_receipt_path(env: &ShellEnv) -> Option<PathBuf> {
    let home = home(env)?;
    // Use the native sandbox's protected Tokage data directory, not the CLI's own config.
    Some(crate::sandbox::SandboxConfig::new(std::env::temp_dir(), None, Some(&home), None)
        .app_data.join("provider-auth/reasonix.json"))
}
fn reasonix_key_fingerprint(env: &ShellEnv) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    let root = env.vars.get("REASONIX_HOME").map(PathBuf::from)
        .or_else(|| home(env).map(|h| h.join(".reasonix"))).unwrap_or_default();
    hash.update(root.to_string_lossy().as_bytes());
    for name in ["config.toml", ".env"] {
        let bytes = std::fs::read(root.join(name)).unwrap_or_default();
        hash.update((bytes.len() as u64).to_le_bytes()); hash.update(bytes);
    }
    let key_names = crate::chat::child_env::reasonix_env_keys(env);
    // Bind credential and endpoint overrides, not incidental PWD, TERM, SHLVL or PATH.
    let mut vars: Vec<_> = env.vars.iter().filter(|(key, _)| key_names.contains(key)
        || key.starts_with("REASONIX_") || matches!(key.as_str(), "HTTP_PROXY" | "HTTPS_PROXY" | "ALL_PROXY" | "NO_PROXY" | "http_proxy" | "https_proxy" | "all_proxy" | "no_proxy" | "SSL_CERT_FILE" | "SSL_CERT_DIR")).collect();
    vars.sort_by_key(|(key, _)| *key);
    for (key, value) in vars {
        hash.update((key.len() as u64).to_le_bytes()); hash.update(key.as_bytes());
        hash.update((value.len() as u64).to_le_bytes()); hash.update(value.as_bytes());
    }
    hash.finalize().into()
}
fn verification_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}
fn reasonix_key_verified(env: &ShellEnv) -> bool {
    let Some(path) = reasonix_receipt_path(env) else { return false; };
    let Some(receipt) = std::fs::read(path).ok().and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok()) else { return false; };
    let fingerprint = serde_json::to_value(reasonix_key_fingerprint(env)).ok();
    let age = receipt["verified_at"].as_u64().and_then(|when| verification_now().checked_sub(when));
    receipt["version"].as_u64() == Some(1) && fingerprint.as_ref() == Some(&receipt["fingerprint"])
        && age.is_some_and(|age| age < REASONIX_VERIFICATION_MAX_AGE)
}
pub(crate) fn invalidate_reasonix_key(env: &ShellEnv) {
    if let Some(path) = reasonix_receipt_path(env) { let _ = std::fs::remove_file(path); }
}
fn persist_reasonix_verification(env: &ShellEnv, fingerprint: [u8; 32]) -> Result<(), String> {
    use std::io::Write;
    let path = reasonix_receipt_path(env).ok_or("Reasonix verification requires a home directory")?;
    let parent = path.parent().ok_or("Invalid verification path")?;
    std::fs::create_dir_all(parent).map_err(|_| "Unable to create Tokage verification directory")?;
    let temporary = parent.join(format!("reasonix-{}-{}.tmp", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
    let result = (|| -> std::io::Result<()> {
        let mut file = options.open(&temporary)?;
        let receipt = serde_json::json!({"version":1,"fingerprint":fingerprint,"verified_at":verification_now()});
        file.write_all(receipt.to_string().as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&temporary, &path)
    })();
    if result.is_err() { let _ = std::fs::remove_file(&temporary); }
    result.map_err(|_| "Reasonix was verified but Tokage could not save its verification receipt".into())
}
/// Explicit verification uses the configured default model, never a hardcoded fallback provider.
pub async fn verify_reasonix_key(exe: &Path, env: &ShellEnv) -> Result<(), String> {
    invalidate_reasonix_key(env);
    let fingerprint = reasonix_key_fingerprint(env);
    let result = run_auth(ProviderId::Reasonix, exe, &["run", "--permission-mode", "read-only",
        "--max-steps", "1", "--output-format", "json",
        "Reply exactly TOKAGE_KEY_OK. Do not use tools."], env, 45).await;
    let Some((ok, out)) = result else { return Err("Reasonix verification timed out".into()); };
    let value = out.find('{').and_then(|i| serde_json::Deserializer::from_str(&out[i..]).into_iter::<serde_json::Value>().next()?.ok());
    if !ok || !value.as_ref().is_some_and(|v| v["is_error"] == false && v["subtype"] == "success" && v["result"].as_str().is_some_and(|t| t.trim() == "TOKAGE_KEY_OK")) {
        return Err("Reasonix authentication or model request failed; check credentials, balance and network with reasonix setup".into());
    }
    if reasonix_key_fingerprint(env) != fingerprint { return Err("Reasonix configuration changed during verification; retry".into()); }
    persist_reasonix_verification(env, fingerprint)
}

async fn probe_reasonix(exe: &Path, env: &ShellEnv) -> Probe {
    let mut p = Probe::default();
    let billing = run_auth(ProviderId::Reasonix, exe, &["doctor", "billing", "--json"], env, 15)
        .await
        .and_then(|(ok, out)| ok.then_some(out));
    if let Some((true, out)) = run_auth(ProviderId::Reasonix, exe, &["doctor", "--json"], env, 20).await {
        if let Some(report) = parse_reasonix_doctor(&out) {
            apply_reasonix_doctor(&mut p, report, billing.as_deref());
            if p.status == Some(ProviderStatus::UnknownAuth) && reasonix_key_verified(env) { p.status = Some(ProviderStatus::Ready); }
            return p;
        }
    }
    apply_reasonix_config_fallback(&mut p, env, billing.as_deref());
    if p.status == Some(ProviderStatus::UnknownAuth) && reasonix_key_verified(env) { p.status = Some(ProviderStatus::Ready); }
    p
}

struct ReasonixDoctor {
    gateway_host: Option<String>,
    models: Vec<ModelInfo>,
    auth_method: Option<String>,
    ready: bool,
}

fn parse_reasonix_doctor(text: &str) -> Option<ReasonixDoctor> {
    let start = text.find('{')?;
    let v: serde_json::Value = serde_json::Deserializer::from_str(&text[start..]).into_iter::<serde_json::Value>().next()?.ok()?;
    let providers = v.get("providers")?.as_array()?;
    let mut models = Vec::new();
    let mut auth_parts = Vec::new();
    let mut ready = false;
    let mut gateway_host = None;
    for prov in providers {
        let name = prov.get("name").and_then(|x| x.as_str()).unwrap_or("provider");
        let key_present = prov
            .get("key_present")
            .and_then(|x| x.as_bool())
            .unwrap_or(false);
        if key_present {
            if let Some(host) = prov.get("base_url_host").and_then(|v| v.as_str()) {
                if !matches!(host, "api.deepseek.com" | "api.openai.com" | "api.anthropic.com" | "generativelanguage.googleapis.com") { gateway_host = Some(host.to_string()); }
            }
            ready = true;
            if let Some(env_name) = prov.get("api_key_env").and_then(|x| x.as_str()) {
                if !env_name.is_empty() && !auth_parts.iter().any(|s| s == env_name) {
                    auth_parts.push(env_name.to_string());
                }
            }
        }
        let list = prov
            .get("models")
            .and_then(|m| m.as_array())
            .cloned()
            .unwrap_or_default();
        if list.is_empty() {
            if let Some(m) = prov.get("model").and_then(|x| x.as_str()) {
                models.push(ModelInfo {
                    id: format!("{name}/{m}"),
                    name: m.to_string(),
                });
            }
        } else {
            for m in list {
                if let Some(mid) = m.as_str() {
                    let id = if mid.contains('/') {
                        mid.to_string()
                    } else {
                        format!("{name}/{mid}")
                    };
                    models.push(ModelInfo {
                        name: mid.to_string(),
                        id,
                    });
                }
            }
        }
    }
    Some(ReasonixDoctor {
        gateway_host,
        models,
        auth_method: (!auth_parts.is_empty()).then(|| auth_parts.join(", ")),
        ready,
    })
}

fn apply_reasonix_doctor(p: &mut Probe, report: ReasonixDoctor, billing_json: Option<&str>) {
    if report.ready {
        p.status = Some(ProviderStatus::UnknownAuth);
        p.auth_method = report.auth_method.or(Some("api_key".into()));
        p.models = report.models;
        p.models_source = (!p.models.is_empty()).then_some(ModelsSource::Cli);
        let mut account = usage::reasonix_account(billing_json);
        if let Some(host) = report.gateway_host {
            p.auth_method = Some("gateway".into());
            account.endpoint = Some(host);
            account.official = false;
        }
        p.account = Some(account);
        if p.models.is_empty() {
            p.guidance.push(guide(GuidanceCode::NoModels, Some("reasonix setup")));
        }
        // Folder trust (`reasonix trust --dir`) is a separate user action; Tokage
        // never writes it. trust_args stay empty; denials surface `permission.untrusted_folder`.
    } else {
        p.status = Some(ProviderStatus::NeedsLogin);
        p.guidance
            .push(guide(GuidanceCode::SetApiKey, Some("reasonix setup")));
    }
}

fn apply_reasonix_config_fallback(p: &mut Probe, env: &ShellEnv, billing_json: Option<&str>) {
    let Some(home) = home(env) else {
        p.status = Some(ProviderStatus::UnknownAuth);
        p.guidance
            .push(guide(GuidanceCode::VerifyLogin, Some("reasonix setup")));
        return;
    };
    let rx = home.join(".reasonix");
    let config = std::fs::read_to_string(rx.join("config.toml")).unwrap_or_default();
    let env_text = std::fs::read_to_string(rx.join(".env")).unwrap_or_default();
    let env_names: std::collections::HashSet<String> = env_text
        .lines()
        .filter_map(|l| {
            let t = l.trim();
            if t.is_empty() || t.starts_with('#') {
                return None;
            }
            let (k, value) = t.split_once('=')?;
            if value.trim().trim_matches(['"', '\'']).is_empty() { return None; }
            let k = k.trim();
            (!k.is_empty()).then(|| k.to_string())
        })
        .collect();

    let mut models = Vec::new();
    let mut auth_parts = Vec::new();
    let mut current_name: Option<String> = None;
    let mut current_model: Option<String> = None;
    let mut current_models: Vec<String> = Vec::new();
    let mut current_key: Option<String> = None;

    for line in config.lines().chain(std::iter::once("[[providers]]")) {
        let t = line.trim();
        if t.starts_with("[[providers]]") {
            flush_reasonix_provider(
                &mut models,
                &mut auth_parts,
                &current_name,
                &current_model,
                &current_models,
                &current_key,
                &env_names,
                env,
            );
            current_name = None;
            current_model = None;
            current_models.clear();
            current_key = None;
            continue;
        }
        if let Some((k, v)) = t.split_once('=') {
            let k = k.trim();
            let v = v.split('#').next().unwrap_or("").trim();
            match k {
                "name" => current_name = Some(v.trim_matches(['"', '\'']).to_string()),
                "model" => current_model = Some(v.trim_matches(['"', '\'']).to_string()),
                "api_key_env" => current_key = Some(v.trim_matches(['"', '\'']).to_string()),
                "models" => current_models = parse_toml_string_array(v),
                _ => {}
            }
        }
    }

    if auth_parts.is_empty() {
        p.status = Some(ProviderStatus::NeedsLogin);
        p.guidance
            .push(guide(GuidanceCode::SetApiKey, Some("reasonix setup")));
        return;
    }
    p.status = Some(ProviderStatus::UnknownAuth);
    p.auth_method = Some(auth_parts.join(", "));
    p.models = models;
    p.models_source = (!p.models.is_empty()).then_some(ModelsSource::Cli);
    p.account = Some(usage::reasonix_account(billing_json));
    if p.models.is_empty() {
        p.guidance
            .push(guide(GuidanceCode::NoModels, Some("reasonix setup")));
    }
}

fn flush_reasonix_provider(
    models: &mut Vec<ModelInfo>,
    auth_parts: &mut Vec<String>,
    name: &Option<String>,
    model: &Option<String>,
    list: &[String],
    key: &Option<String>,
    env_names: &std::collections::HashSet<String>,
    env: &ShellEnv,
) {
    let Some(name) = name else { return };
    if let Some(k) = key {
        if env_names.contains(k) || has_env(env, k) {
            if !auth_parts.iter().any(|s| s == k) {
                auth_parts.push(k.clone());
            }
        }
    }
    if list.is_empty() {
        if let Some(m) = model {
            models.push(ModelInfo {
                id: format!("{name}/{m}"),
                name: m.clone(),
            });
        }
        return;
    }
    for m in list {
        let id = if m.contains('/') {
            m.clone()
        } else {
            format!("{name}/{m}")
        };
        models.push(ModelInfo {
            name: m.clone(),
            id,
        });
    }
}

fn parse_toml_string_array(v: &str) -> Vec<String> {
    let v = v.trim();
    let inner = v.strip_prefix('[').and_then(|s| s.strip_suffix(']')).unwrap_or(v);
    inner
        .split(',')
        .filter_map(|p| {
            let p = p.trim().trim_matches(['"', '\'']);
            (!p.is_empty()).then(|| p.to_string())
        })
        .collect()
}

/// Aider has no account; it needs at least one provider API key. Keys can come from the
/// environment, `~/.aider.conf.yml` or `~/.env`. Only key *names* are inspected.
pub(crate) const AIDER_KEYS: &[(&str, &str, &str)] = &[
    ("ANTHROPIC_API_KEY", "anthropic", "Anthropic"),
    ("OPENAI_API_KEY", "openai", "OpenAI"),
    ("DEEPSEEK_API_KEY", "deepseek", "DeepSeek"),
    ("GEMINI_API_KEY", "gemini", "Gemini"),
    ("OPENROUTER_API_KEY", "openrouter", "OpenRouter"),
    ("GROQ_API_KEY", "groq", "Groq"),
    ("MISTRAL_API_KEY", "mistral", "Mistral"),
    ("XAI_API_KEY", "xai", "xAI"),
    ("COHERE_API_KEY", "cohere", "Cohere"),
];

async fn probe_aider(exe: &Path, env: &ShellEnv) -> Probe {
    let mut names: Vec<String> = AIDER_KEYS
        .iter()
        .filter(|(k, ..)| has_env(env, k) && !env.vars[*k].starts_with("YOUR_"))
        .map(|(k, ..)| (*k).to_string())
        .collect();
    if let Some(h) = home(env) {
        names.extend(key_names_in(&h.join(".env")));
        names.extend(aider_conf_key_names(&h.join(".aider.conf.yml")));
    }

    let configured: Vec<_> = AIDER_KEYS
        .iter()
        .filter(|(key, prefix, _)| names.iter().any(|n| n == key || n == prefix))
        .collect();

    let mut p = Probe::default();
    if configured.is_empty() {
        p.status = Some(ProviderStatus::NeedsLogin);
        p.guidance.push(guide(
            GuidanceCode::SetApiKey,
            Some("export DEEPSEEK_API_KEY=<your key>"),
        ));
        return p;
    }
    p.status = Some(ProviderStatus::Ready);
    p.auth_method = Some(
        configured
            .iter()
            .map(|(k, ..)| *k)
            .collect::<Vec<_>>()
            .join(", "),
    );

    // Aider's model catalog ships with it (LiteLLM metadata), so listing is local.
    let lists = futures::future::join_all(
        configured
            .iter()
            .map(|(_, prefix, _)| aider_list_models(exe, env, prefix)),
    )
    .await;
    for ((_, prefix, label), list) in configured.iter().zip(lists) {
        match list {
            Some(ids) if !ids.is_empty() => p.models.extend(ids.into_iter().map(|id| {
                ModelInfo {
                    name: id
                        .strip_prefix(&format!("{prefix}/"))
                        .unwrap_or(&id)
                        .to_string(),
                    id,
                }
            })),
            // Listing failed: still show the provider so the user knows the key was found.
            _ => p.models.push(ModelInfo {
                id: format!("{prefix}/"),
                name: (*label).into(),
            }),
        }
    }
    p.models_source = Some(ModelsSource::Cli);
    p.account = Some(usage::Account {
        billing: usage::Billing::PayAsYouGo,
        official: true,
        endpoint: None,
        plan: None,
        balance: None,
        usage: usage::get_cached_usage(ProviderId::Aider),
    });
    p
}

/// `aider --list-models <prefix>/` in a throwaway directory (aider writes a chat-history
/// file into its cwd). Keeps only ids under that provider prefix.
async fn aider_list_models(exe: &Path, env: &ShellEnv, prefix: &str) -> Option<Vec<String>> {
    let dir = std::env::temp_dir().join(format!("tokage-aider-{}-{prefix}", std::process::id()));
    std::fs::create_dir_all(&dir).ok()?;
    let pattern = format!("{prefix}/");
    let out = tokio::process::Command::new(exe)
        .args([
            "--list-models",
            &pattern,
            "--no-git",
            "--no-check-update",
            "--no-analytics",
            "--no-fancy-input",
            "--no-pretty",
            "--yes-always",
        ])
        .current_dir(&dir)
        .env_clear()
        .envs(&env.vars)
        .env("NO_COLOR", "1")
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    // Aider is a Python app; startup alone takes several seconds, more while other
    // probes run in parallel (measured ~13s with six CLIs probing at once). Stays within
    // the 40s per-provider budget.
    let out = tokio::time::timeout(Duration::from_secs(30), out).await;
    let _ = std::fs::remove_dir_all(&dir);
    let out = out.ok()?.ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    Some(
        text.lines()
            .filter_map(|l| l.trim().strip_prefix("- "))
            .filter(|id| id.starts_with(&pattern))
            .map(str::to_string)
            .collect(),
    )
}

/// Kiro CLI (`kiro-cli`).
///
/// `chat --list-models` opens an interactive browser login when credentials are
/// missing or unreadable (sandbox / keychain / sqlite lock). Never run it until
/// `whoami` has confirmed a positive identity, or startup scans will pop
/// app.kiro.dev even though the user is already signed in outside the probe.
async fn probe_kiro(exe: &Path, env: &ShellEnv) -> Probe {
    let mut p = Probe::default();
    let whoami_res =
        run_auth(ProviderId::KiroCli, exe, &["whoami", "--format", "json"], env, 30).await;

    match whoami_res {
        Some((true, out)) => {
            // Exit zero alone is not proof of identity (`{"account":null}` exits 0).
            p.status = Some(if parse_kiro_account(&out).is_some() {
                ProviderStatus::Ready
            } else {
                ProviderStatus::UnknownAuth
            });
            // Parse JSON output if possible: {"accountType":"SocialGoogle","email":"..."}
            p.auth_method = parse_kiro_account(&out);
            if p.auth_method.is_none() {
                p.guidance
                    .push(guide(GuidanceCode::VerifyLogin, Some("kiro-cli whoami")));
            }
        }
        Some((false, out)) if explicitly_signed_out(&out) => {
            p.status = Some(ProviderStatus::NeedsLogin);
            p.guidance
                .push(guide(GuidanceCode::Login, Some("kiro-cli login")));
        }
        // A failed probe can mean a database lock, network failure or sandbox
        // denial. It does not prove the account is signed out. Do not invite
        // users to log in again when Kiro still has valid credentials.
        Some((false, _)) => {
            p.status = Some(ProviderStatus::UnknownAuth);
            p.guidance
                .push(guide(GuidanceCode::VerifyLogin, Some("kiro-cli whoami")));
        }
        None => {
            p.status = Some(ProviderStatus::UnknownAuth);
            p.guidance
                .push(guide(GuidanceCode::VerifyLogin, Some("kiro-cli whoami")));
        }
    }

    if matches!(p.status, Some(ProviderStatus::Ready)) {
        if let Some((true, out)) = run_auth(
            ProviderId::KiroCli,
            exe,
            &["chat", "--list-models", "--format", "json"],
            env,
            5,
        )
        .await
        {
            p.models = parse_model_list(&out);
            p.models_source = (!p.models.is_empty()).then_some(ModelsSource::Cli);
        }
    }

    p
}

fn parse_kiro_account(out: &str) -> Option<String> {
    if let Some(provider) = out.lines().map(str::trim).find_map(|s| s.strip_prefix("Logged in with ")) {
        if matches!(provider, "Google" | "GitHub" | "Builder ID") { return Some(provider.into()); }
    }
    if let Some(start) = out.find('{') {
        if let Some(end) = out.rfind('}') {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&out[start..=end]) {
                let email = v.get("email").and_then(|x| x.as_str()).filter(|s| !s.trim().is_empty());
                let act_type = v.get("accountType").and_then(|x| x.as_str()).filter(|s| !s.trim().is_empty());
                match (email, act_type) {
                    (Some(e), Some(t)) => {
                        let provider_name = t.strip_prefix("Social").unwrap_or(t);
                        return Some(format!("{provider_name} ({e})"));
                    }
                    (Some(e), None) => return Some(e.to_string()),
                    (None, Some(t)) => return Some(t.to_string()),
                    (None, None) => {}
                }
            }
        }
    }
    // Fallback: check plain text lines e.g. "Email: xyz"
    for l in out.lines() {
        if let Some(email) = l.strip_prefix("Email: ") {
            let email = email.trim();
            if !email.is_empty() {
                return Some(email.to_string());
            }
        }
    }
    signed_in_as(out)
}

/// `KEY=value` lines → key names. Values are dropped immediately.
fn key_names_in(path: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return vec![];
    };
    text.lines()
        .filter_map(|l| {
            let l = l.trim().strip_prefix("export ").unwrap_or(l.trim());
            let (k, v) = l.split_once('=')?;
            let value = v.trim().trim_matches(['\'', '"']);
            (!k.starts_with('#') && !value.is_empty() && !value.starts_with("YOUR_")).then(|| k.trim().to_string())
        })
        .collect()
}

/// `openai-api-key:` / `anthropic-api-key:` / `api-key: [deepseek=…]` → provider prefixes.
fn aider_conf_key_names(path: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return vec![];
    };
    let mut out = Vec::new();
    let mut in_api_key_list = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('#') || t.is_empty() {
            continue;
        }
        if let Some(rest) = t.strip_prefix("- ").filter(|_| in_api_key_list) {
            if let Some((prov, _)) = rest.trim_matches(['"', '\'']).split_once('=') {
                out.push(prov.trim().to_string());
            }
            continue;
        }
        in_api_key_list = false;
        let Some((key, value)) = t.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "openai-api-key" if !value.is_empty() => out.push("openai".into()),
            "anthropic-api-key" if !value.is_empty() => out.push("anthropic".into()),
            "api-key" => {
                if value.is_empty() {
                    in_api_key_list = true;
                }
                for item in value.trim_matches(['[', ']']).split(',') {
                    if let Some((prov, _)) = item.trim().trim_matches(['"', '\'']).split_once('=') {
                        out.push(prov.trim().to_string());
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

fn base(
    spec: &Spec,
    tools: &Toolbox,
    status: ProviderStatus,
    version: Option<String>,
    path: Option<String>,
) -> ProviderInfo {
    ProviderInfo {
        id: spec.id,
        usage_query: crate::usage_query::contract::capability(spec.id),
        name: spec.name,
        executable: spec.exes[0],
        featured: spec.featured,
        status,
        version,
        path,
        auth_method: None,
        models: vec![],
        model_groups: vec![],
        models_source: None,
        account: None,
        guidance: vec![],
        install_methods: install::methods(spec.id, tools),
        trust_args: trust_args(spec.id, "<workspace>"),
        chat: adapter::adapter(spec.id).is_some(),
        adapter: adapter::adapter(spec.id).map(|a| AdapterInfo {
            caps: a.meta().caps,
            versions: a.meta().versions.to_string(),
            tested: a.meta().tested,
        }),
        direct_limit: None,
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    #[tokio::test]
    async fn antigravity_scan_never_invokes_usage() {
        use super::*;
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("agy"); let marker = dir.path().join("queried");
        std::fs::write(&exe, format!("#!/bin/sh\nif [ \"$1\" = models ]; then echo gemini-3.8-flash-low; else touch {}; exit 1; fi\n", crate::chat::adapter::shell_single_quote(&marker.display().to_string()))).unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let env = ShellEnv { vars: [("HOME".into(), dir.path().display().to_string()), ("PATH".into(), "/usr/bin:/bin".into())].into() };
        assert_eq!(probe_antigravity(&exe, &env).await.status, Some(ProviderStatus::UnknownAuth));
        assert!(!marker.exists(), "scanning invoked a command other than models");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn antigravity_offline_models_are_not_login_evidence() {
        use super::*;
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("agy-stub");
        let config = dir.path().join(".gemini/antigravity-cli");
        std::fs::create_dir_all(&config).unwrap();
        let mut env = ShellEnv { vars: [("HOME".into(), dir.path().display().to_string()), ("PATH".into(), "/usr/bin:/bin".into())].into() };
        let write = |usage: &str, code: i32| {
            let q = crate::chat::adapter::shell_single_quote;
            std::fs::write(&exe, format!("#!/bin/sh\nif [ \"$1\" = models ]; then printf '%s\\n' 'gemini-3.8-flash-low Gemini Flash'; else printf '%s\\n' {}; exit {}; fi\n", q(usage), code)).unwrap();
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        write("authentication required", 1);
        assert_eq!(probe_antigravity(&exe, &env).await.status, Some(ProviderStatus::UnknownAuth));
        write("network timeout", 1);
        assert_eq!(probe_antigravity(&exe, &env).await.status, Some(ProviderStatus::UnknownAuth));
        let valid = serde_json::json!({"status":"SUCCESS","num_turns":0,"command":{"name":"usage","data":{"groups":[{"buckets":[{"window":"5h","remaining_fraction":0.5}]}]}}});
        write(&valid.to_string(), 0);
        let p = probe_antigravity(&exe, &env).await;
        assert_eq!(p.status, Some(ProviderStatus::UnknownAuth));
        assert!(p.account.unwrap().usage.is_none());
        std::fs::write(config.join("settings.json"), r#"{"modelProvider":"gemini"}"#).unwrap();
        assert_eq!(probe_antigravity(&exe, &env).await.status, Some(ProviderStatus::NeedsLogin));
        env.vars.insert("GEMINI_API_KEY".into(), "synthetic-invalid-key".into());
        let p = probe_antigravity(&exe, &env).await;
        assert_eq!(p.status, Some(ProviderStatus::UnknownAuth));
        assert_eq!(p.auth_method.as_deref(), Some("Gemini API key"));
        write(r#"{"status":"ERROR","response":"","error":"synthetic-invalid-key"}"#, 1);
        let err = verify_antigravity_key(&exe, &env).await.unwrap_err();
        assert!(!err.contains("synthetic-invalid-key"));
        write(r#"{"status":"SUCCESS","response":"TOKAGE_KEY_OK"}"#, 0);
        verify_antigravity_key(&exe, &env).await.unwrap();
        assert_eq!(probe_antigravity(&exe, &env).await.status, Some(ProviderStatus::Ready));
        invalidate_antigravity_key(&env);
        assert_eq!(probe_antigravity(&exe, &env).await.status, Some(ProviderStatus::UnknownAuth));
        verify_antigravity_key(&exe, &env).await.unwrap();
        env.vars.insert("GOOGLE_GEMINI_BASE_URL".into(), "https://gateway.example.test".into());
        let p = probe_antigravity(&exe, &env).await;
        assert_eq!(p.auth_method.as_deref(), Some("gateway"));
        assert_eq!(p.status, Some(ProviderStatus::UnknownAuth), "endpoint change must require new verification");
        verify_antigravity_key(&exe, &env).await.unwrap();
        assert_eq!(probe_antigravity(&exe, &env).await.status, Some(ProviderStatus::Ready));
        env.vars.insert("GEMINI_API_KEY".into(), "different-key".into());
        assert_eq!(probe_antigravity(&exe, &env).await.status, Some(ProviderStatus::UnknownAuth));
    }

    #[test]
    fn cursor_json_login_is_explicit_and_failures_are_not_login() {
        use super::*;
        let auth = r#"{"status":"authenticated","isAuthenticated":true,"userInfo":{"email":"one@example.test"}}"#;
        assert_eq!(cursor_login_status(true, auth), (ProviderStatus::Ready, Some("one@example.test".into())));
        assert_eq!(cursor_login_status(false, auth).0, ProviderStatus::UnknownAuth);
        for out in [r#"{"status":"unauthenticated","isAuthenticated":false}"#,
            r#"{"status":"authenticated","isAuthenticated":false}"#, "Authentication required"] {
            assert_eq!(cursor_login_status(true, out).0, ProviderStatus::NeedsLogin);
        }
        for out in ["", "network timeout", "{bad json}", r#"{"status":"authenticated"}"#] {
            assert_eq!(cursor_login_status(true, out).0, ProviderStatus::UnknownAuth);
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cursor_key_catalog_auth_gateway_and_account_changes() {
        use super::*;
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("cursor-stub");
        let mut env = ShellEnv::default();
        env.vars.insert("HOME".into(), dir.path().display().to_string());
        env.vars.insert("PATH".into(), "/usr/bin:/bin".into());
        let write = |email: &str, models: &str, code: i32, authenticated: bool| {
            let status = serde_json::json!({"status": if authenticated { "authenticated" } else { "unauthenticated" },
                "isAuthenticated": authenticated, "userInfo": { "email": email }}).to_string();
            let q = crate::chat::adapter::shell_single_quote;
            let script = format!("#!/bin/sh\ncase \"$1\" in\nstatus) printf '%s\\n' {};;\nabout) printf '%s\\n' '{{\"subscriptionTier\":\"Pro+\"}}';;\nmodels) printf '%s\\n' {}; exit {};;\nesac\n", q(&status), q(models), code);
            std::fs::write(&exe, script).unwrap();
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        write("one@example.test", "auto - Auto", 0, true);
        let p = probe_cursor(&exe, &env).await;
        assert_eq!(p.status, Some(ProviderStatus::Ready));
        assert_eq!(p.auth_method.as_deref(), Some("one@example.test"));
        assert_eq!(p.models[0].id, "auto");
        write("two@example.test", "auto - Auto", 0, true);
        assert_eq!(probe_cursor(&exe, &env).await.auth_method.as_deref(), Some("two@example.test"));
        write("", "auto - Auto", 0, false);
        let p = probe_cursor(&exe, &env).await;
        assert_eq!(p.status, Some(ProviderStatus::NeedsLogin));
        assert!(p.account.is_none() && p.models.is_empty());
        env.vars.insert("CURSOR_API_KEY".into(), "synthetic-test-key".into());
        write("one@example.test", "Warning: The provided API key is invalid.", 1, true);
        let p = probe_cursor(&exe, &env).await;
        assert_eq!(p.status, Some(ProviderStatus::NeedsLogin));
        assert!(p.auth_method.is_none() && p.account.is_none());
        write("one@example.test", "network timeout", 1, true);
        assert_eq!(probe_cursor(&exe, &env).await.status, Some(ProviderStatus::UnknownAuth));
        write("", "auto - Auto", 0, false);
        let p = probe_cursor(&exe, &env).await;
        assert_eq!(p.status, Some(ProviderStatus::Ready));
        assert_eq!(p.auth_method.as_deref(), Some("CURSOR_API_KEY"));
        assert!(matches!(p.account.unwrap().billing, usage::Billing::PayAsYouGo));
        env.vars.insert("CURSOR_API_ENDPOINT".into(), "https://gateway.example.test".into());
        let p = probe_cursor(&exe, &env).await;
        assert_eq!(p.auth_method.as_deref(), Some("gateway"));
        let account = p.account.unwrap();
        assert!(!account.official);
        assert!(account.endpoint.is_some());
        assert!(p.guidance.iter().any(|g| matches!(g.code, GuidanceCode::ThirdPartyBackend)));
        env.vars.remove("CURSOR_API_KEY");
        write("one@example.test", "network timeout", 1, true);
        assert_eq!(probe_cursor(&exe, &env).await.status, Some(ProviderStatus::UnknownAuth));
    }

    #[test]
    fn auth_requires_positive_evidence() {
        use super::*;
        for out in ["Not logged in", "Authentication required", "Logged out", "✓ Logged in as user\nNot signed in"] {
            assert!(!status_confirms_login(ProviderId::CursorCli, true, out));
            assert!(!status_confirms_login(ProviderId::CodexCli, true, out));
        }
        assert!(!status_confirms_login(ProviderId::CursorCli, true, ""));
        assert!(!status_confirms_login(ProviderId::CodexCli, true, ""));
        assert!(status_confirms_login(ProviderId::CodexCli, true, "Logged in using ChatGPT"));
        assert!(status_confirms_login(ProviderId::CursorCli, true, "✓ Logged in as user@example.com"));
        for v in [serde_json::json!({"loggedIn": false, "authMethod": "claude.ai"}),
            serde_json::json!({"authMethod": "claude.ai"}),
            serde_json::json!({"loggedIn": true, "authMethod": "none"})] {
            assert!(!claude_confirms_login(true, "", Some(&v)));
        }
        let v = serde_json::json!({"loggedIn": true, "authMethod": "claude.ai"});
        assert!(claude_confirms_login(true, "", Some(&v)));
        assert!(!claude_confirms_login(false, "", Some(&v)));
        assert!(!claude_confirms_login(true, "", None));
    }

    #[test]
    fn gateway_requires_credentials_and_overrides_subscription() {
        use super::*;
        let mut settings = usage::ClaudeSettings { base_url: Some("https://gateway.example".into()), has_token: false, cloud: None };
        assert_eq!(claude_config_auth(&settings), None);
        settings.has_token = true;
        assert_eq!(claude_config_auth(&settings), Some("gateway"));
        settings.base_url = None;
        assert_eq!(claude_config_auth(&settings), Some("api_key"));
        settings.cloud = Some("Amazon Bedrock");
        assert_eq!(claude_config_auth(&settings), Some("Amazon Bedrock"));
    }

    use super::*;

    #[test]
    fn codex_models_sorted_and_hidden_filtered() {
        let json = r#"{"models":[
            {"slug":"b","display_name":"B","visibility":"list","priority":5},
            {"slug":"h","display_name":"H","visibility":"hide","priority":1},
            {"slug":"a","display_name":"A","visibility":"list","priority":2}]}"#;
        let ids: Vec<_> = parse_codex_models(json).into_iter().map(|m| m.id).collect();
        assert_eq!(ids, ["a", "b"]);
    }

    #[test]
    fn aider_conf_parsing_keeps_names_only() {
        let dir = std::env::temp_dir().join(format!("tokage-aider-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("conf.yml");
        std::fs::write(
            &f,
            "openai-api-key: sk-x\napi-key:\n  - deepseek=abc\n  - \"gemini=def\"\nmodel: x\n",
        )
        .unwrap();
        assert_eq!(aider_conf_key_names(&f), ["openai", "deepseek", "gemini"]);
        std::fs::write(&f, "api-key: [deepseek=a, groq=b]\n").unwrap();
        assert_eq!(aider_conf_key_names(&f), ["deepseek", "groq"]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn copilot_rpc_auth_and_catalog_are_required() {
        let catalog = serde_json::json!({"models":[{"id":"gpt-6-luna","name":"GPT-6 Luna"}]});
        for (auth, models, expected) in [
            (serde_json::json!({"isAuthenticated":true,"authType":"user"}), Some(&catalog), ProviderStatus::Ready),
            (serde_json::json!({"isAuthenticated":false}), Some(&catalog), ProviderStatus::NeedsLogin),
            (serde_json::json!({"login":"local-entry"}), Some(&catalog), ProviderStatus::UnknownAuth),
            (serde_json::json!({"isAuthenticated":true}), None, ProviderStatus::UnknownAuth),
        ] {
            let mut probe = Probe::default();
            apply_copilot_rpc(&mut probe, &auth, models);
            assert_eq!(probe.status, Some(expected));
        }
        let mut env = ShellEnv { vars: [("COPILOT_GITHUB_TOKEN".into(), "first".into()), ("GH_TOKEN".into(), "second".into()), ("GITHUB_TOKEN".into(), "third".into())].into() };
        assert_eq!(copilot_token_env(&env), Some("COPILOT_GITHUB_TOKEN"));
        env.vars.remove("COPILOT_GITHUB_TOKEN");
        assert_eq!(copilot_token_env(&env), Some("GH_TOKEN"));
        env.vars.insert("GH_TOKEN".into(), " ".into());
        assert_eq!(copilot_token_env(&env), Some("GITHUB_TOKEN"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn copilot_rpc_probe_never_sends_chat_and_does_not_fall_back_on_invalid_override() {
        use std::os::unix::fs::PermissionsExt;
        let python = which::which("python3").expect("Python test runtime");
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("copilot-rpc-stub");
        let script = r#"
import json,sys
for i,method in [(1,'auth.getStatus'),(2,'models.list')]:
    header=sys.stdin.buffer.readline().decode()
    n=int(header.split(':',1)[1]); assert sys.stdin.buffer.readline()==b'\r\n'
    request=json.loads(sys.stdin.buffer.read(n))
    assert request['method']==method and request['id']==i
    if i==1:
        valid='--auth-token-env' not in sys.argv
        if not valid:
            assert sys.argv[sys.argv.index('--auth-token-env')+1]=='COPILOT_GITHUB_TOKEN'
            assert '--no-auto-login' in sys.argv
        result={'isAuthenticated':valid,'authType':'user'}
    else: result={'models':[{'id':'gpt-6-luna'}]}
    body=json.dumps({'jsonrpc':'2.0','id':i,'result':result}).encode()
    notification=json.dumps({'jsonrpc':'2.0','method':'status.changed','params':{}}).encode()
    for payload in [notification,body]:
        sys.stdout.buffer.write(('Content-Length: %d\r\n\r\n'%len(payload)).encode());sys.stdout.buffer.flush()
        sys.stdout.buffer.write(payload);sys.stdout.buffer.flush()
    if i==1 and not valid:break
"#;
        std::fs::write(&exe, format!("#!{}\n{script}", python.display())).unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut env = ShellEnv { vars: [("HOME".into(), dir.path().display().to_string()), ("PATH".into(), "/usr/bin:/bin".into())].into() };
        assert_eq!(probe_copilot(&exe, &env).await.status, Some(ProviderStatus::Ready));
        env.vars.insert("COPILOT_GITHUB_TOKEN".into(), "invalid-synthetic".into());
        assert_eq!(probe_copilot(&exe, &env).await.status, Some(ProviderStatus::NeedsLogin));
        env.vars.insert("COPILOT_PROVIDER_BASE_URL".into(), "https://user:secret@gateway.example.test/v1".into());
        let gateway = probe_copilot(&exe, &env).await;
        assert_eq!(gateway.status, Some(ProviderStatus::UnknownAuth));
        assert_eq!(gateway.auth_method.as_deref(), Some("gateway"));
        assert_eq!(gateway.account.unwrap().endpoint.as_deref(), Some("gateway.example.test"));
    }

    #[test]
    fn copilot_help_config_models() {
        let help = "  `logLevel`: x\n\n  `model`: AI model to use.\n    - \"claude-sonnet-5\"\n    - \"gpt-5.4-mini\"\n    - \"gpt-5.4-mini\"\n\n  `contextTier`: tier\n    - \"default\"\n";
        let ids: Vec<_> = parse_copilot_models(help).into_iter().map(|m| m.id).collect();
        assert_eq!(ids, ["claude-sonnet-5", "gpt-5.4-mini"]);
        for (id, name) in [
            ("claude-opus-4.8", "Claude Opus 4.8"),
            ("gpt-5.4-mini", "GPT-5.4 Mini"),
            ("gpt-6.1-sol", "GPT-6.1 Sol"),
            ("kimi-k2.7-code", "Kimi K2.7 Code"),
            ("mai-code-1.1-flash", "MAI Code 1.1 Flash"),
        ] {
            assert_eq!(copilot_display_name(id), name);
        }
    }

    #[test]
    fn model_list_parsing() {
        let agy = "Fetching available models...\ngemini-3.1-pro-high\tGemini 3.1 Pro (High)\nclaude-sonnet-4-6\tClaude Sonnet 4.6\n";
        let m = parse_model_list(agy);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].name, "Gemini 3.1 Pro (High)");
        let cursor = "Available models\n\nauto - Auto (current, default)\ngpt-5.2 - GPT-5.2\nTip: use --model <id>\n";
        let m = parse_model_list(cursor);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].id, "auto");
        assert_eq!(m[0].name, "Auto");
        assert_eq!(m[1].name, "GPT-5.2");
        let lines = "Available models:\n- anthropic/claude-sonnet-5\n- openai/gpt-6\n";
        assert_eq!(parse_model_list(lines).len(), 2);
        let json = r#"{"models":[{"model_id":"a-1","name":"A one"},{"id":"b-2"}]}"#;
        let ids: Vec<_> = parse_model_list(json).into_iter().map(|m| m.id).collect();
        assert_eq!(ids, ["a-1", "b-2"]);
        assert!(parse_model_list("No models\nerror").is_empty());
    }

    #[test]
    fn untested_version_is_flagged_but_usable() {
        let spec = REGISTRY.iter().find(|s| s.id == ProviderId::ClaudeCode).unwrap();
        let tools = Toolbox::detect(&ShellEnv::default());
        let mut i = base(spec, &tools, ProviderStatus::Ready, Some("9.0.0".into()), None);
        check_version(&mut i);
        assert!(matches!(i.status, ProviderStatus::UnsupportedVersion));
        assert!(matches!(i.guidance[0].code, GuidanceCode::UnsupportedVersion));
        let mut ok = base(spec, &tools, ProviderStatus::Ready, Some("2.1.300".into()), None);
        check_version(&mut ok);
        assert!(matches!(ok.status, ProviderStatus::Ready) && ok.guidance.is_empty());
        // Not signed in stays the more important status.
        let mut out = base(spec, &tools, ProviderStatus::NeedsLogin, Some("9.0.0".into()), None);
        check_version(&mut out);
        assert!(matches!(out.status, ProviderStatus::NeedsLogin));
        let json = serde_json::to_value(&i).unwrap();
        assert!(json["adapter"]["caps"].as_array().unwrap().iter().any(|c| c == "NATIVE_FORK"));
    }

    #[test]
    fn version_tokens_are_trimmed() {
        let pick = |text: &str| {
            text.split_whitespace()
                .map(|t| {
                    t.trim_start_matches(['v', '('])
                        .trim_end_matches(['.', ',', ')', ':'])
                })
                .find(|t| t.chars().next().is_some_and(|c| c.is_ascii_digit()) && t.contains('.'))
                .map(str::to_string)
        };
        assert_eq!(
            pick("GitHub Copilot CLI 1.0.91.\nRun 'copilot update'").as_deref(),
            Some("1.0.91")
        );
        assert_eq!(pick("2.1.288 (Claude Code)").as_deref(), Some("2.1.288"));
        assert_eq!(pick("codex-cli v0.160.0").as_deref(), Some("0.160.0"));
    }

    #[test]
    fn signed_in_account_is_extracted() {
        assert_eq!(
            signed_in_as("✓ Logged in as a@b.dev\n").as_deref(),
            Some("a@b.dev")
        );
        assert_eq!(signed_in_as("Not logged in"), None);
    }

    #[test]
    fn registry_ids_unique() {
        let mut ids: Vec<_> = REGISTRY.iter().map(|s| format!("{:?}", s.id)).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n);
        assert!(REGISTRY.iter().all(|s| !s.exes.is_empty()));
    }

    #[test]
    fn env_file_key_names() {
        let dir = std::env::temp_dir().join(format!("tokage-env-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join(".env");
        std::fs::write(
            &f,
            "# c\nexport DEEPSEEK_API_KEY=abc\nEMPTY=\nOPENAI_API_KEY = x\n",
        )
        .unwrap();
        assert_eq!(key_names_in(&f), ["DEEPSEEK_API_KEY", "OPENAI_API_KEY"]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn reasonix_configured_key_requires_success_and_config_changes_invalidate() {
        use super::*;
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join(".reasonix");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(".env"), "DEEPSEEK_API_KEY=synthetic-key").unwrap();
        let env = ShellEnv { vars: [("HOME".into(), dir.path().display().to_string()), ("PATH".into(), "/usr/bin:/bin".into())].into() };
        let exe = dir.path().join("reasonix-stub");
        let write = |result: &str| {
            std::fs::write(&exe, format!("#!/bin/sh\nprintf '%s\\n' {}\nprintf '%s\\n' 'sandbox warning' >&2\n", crate::chat::adapter::shell_single_quote(result))).unwrap();
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        assert!(!reasonix_key_verified(&env));
        write(r#"{"type":"result","subtype":"error_during_execution","is_error":true,"result":"401 invalid credential"}"#);
        assert!(verify_reasonix_key(&exe, &env).await.is_err());
        assert!(!reasonix_key_verified(&env));
        write(r#"{"type":"result","subtype":"success","is_error":false,"result":"TOKAGE_KEY_OK"}"#);
        verify_reasonix_key(&exe, &env).await.unwrap();
        assert!(reasonix_key_verified(&env));
        std::fs::write(root.join(".env"), "DEEPSEEK_API_KEY=changed-key").unwrap();
        assert!(!reasonix_key_verified(&env));
        assert!(parse_reasonix_doctor("{\"providers\":[]}\nsandbox warning").is_some());
    }

    #[test]
    fn reasonix_receipt_ignores_shell_metadata_but_expires_and_rejects_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("reasonix-custom");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("config.toml"), r#"api_key_env = "CUSTOM_API_KEY"
"#).unwrap();
        let env = ShellEnv { vars: [("HOME".into(), dir.path().display().to_string()), ("REASONIX_HOME".into(), root.display().to_string()), ("CUSTOM_API_KEY".into(), "synthetic-secret".into()), ("PWD".into(), "/first".into())].into() };
        assert_eq!(crate::chat::child_env::assemble_with_proxy(ProviderId::Reasonix, &env, &[], None).get("CUSTOM_API_KEY").map(String::as_str), Some("synthetic-secret"));
        assert!(!crate::chat::child_env::assemble_with_proxy(ProviderId::CopilotCli, &env, &[], None).contains_key("CUSTOM_API_KEY"));
        assert!(!reasonix_key_verified(&env));
        persist_reasonix_verification(&env, reasonix_key_fingerprint(&env)).unwrap();
        let mut recaptured = env.clone();
        recaptured.vars.insert("PWD".into(), "/second".into());
        recaptured.vars.insert("SHLVL".into(), "2".into());
        recaptured.vars.insert("TERM".into(), "xterm".into());
        recaptured.vars.insert("PATH".into(), "/new/path".into());
        assert!(reasonix_key_verified(&recaptured));
        recaptured.vars.insert("CUSTOM_API_KEY".into(), "changed-secret".into());
        assert!(!reasonix_key_verified(&recaptured));
        let path = reasonix_receipt_path(&env).unwrap();
        let receipt_text = std::fs::read_to_string(&path).unwrap();
        assert!(!receipt_text.contains("synthetic-secret"));
        #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600); }
        let mut receipt: serde_json::Value = serde_json::from_str(&receipt_text).unwrap();
        receipt["verified_at"] = serde_json::json!(verification_now().saturating_sub(REASONIX_VERIFICATION_MAX_AGE + 1));
        std::fs::write(&path, receipt.to_string()).unwrap();
        assert!(!reasonix_key_verified(&env));
        receipt["verified_at"] = serde_json::json!(verification_now() + 60);
        std::fs::write(&path, receipt.to_string()).unwrap();
        assert!(!reasonix_key_verified(&env));
        std::fs::write(&path, "corrupt").unwrap();
        assert!(!reasonix_key_verified(&env));
        persist_reasonix_verification(&env, reasonix_key_fingerprint(&env)).unwrap();
        invalidate_reasonix_key(&env);
        assert!(!reasonix_key_verified(&env));
    }

    #[test]
    fn reasonix_custom_endpoint_is_gateway_and_missing_key_is_not_ready() {
        let report = parse_reasonix_doctor(r#"{"providers":[{"name":"custom","base_url_host":"relay.example.test","key_present":true,"models":["model"]}]}"#).unwrap();
        let mut p = Probe::default();
        apply_reasonix_doctor(&mut p, report, None);
        assert_eq!(p.status, Some(ProviderStatus::UnknownAuth));
        assert_eq!(p.auth_method.as_deref(), Some("gateway"));
        assert_eq!(p.account.unwrap().endpoint.as_deref(), Some("relay.example.test"));
        let report = parse_reasonix_doctor(r#"{"providers":[{"name":"deepseek","key_present":false}]}"#).unwrap();
        let mut p = Probe::default();
        apply_reasonix_doctor(&mut p, report, None);
        assert_eq!(p.status, Some(ProviderStatus::NeedsLogin));
    }

    #[test]
    fn reasonix_doctor_json_models_and_keys() {
        let json = r#"{
          "version":"1.2.3",
          "providers":[
            {"name":"deepseek","models":["deepseek-v4-flash","deepseek-v4-pro"],"api_key_env":"DEEPSEEK_API_KEY","key_present":true},
            {"name":"mimo","model":"mimo-v2-flash","api_key_env":"MIMO_API_KEY","key_present":false}
          ]
        }"#;
        let report = parse_reasonix_doctor(json).expect("doctor json");
        assert!(report.ready);
        assert_eq!(report.auth_method.as_deref(), Some("DEEPSEEK_API_KEY"));
        let ids: Vec<_> = report.models.iter().map(|m| m.id.as_str()).collect();
        assert!(ids.contains(&"deepseek/deepseek-v4-flash"));
        assert!(ids.contains(&"deepseek/deepseek-v4-pro"));
        assert!(ids.contains(&"mimo/mimo-v2-flash"));

        let mut p = Probe::default();
        apply_reasonix_doctor(
            &mut p,
            report,
            Some(r#"{"providers":[{"name":"deepseek","billing_currency":"USD","billing_mode":"payg"}]}"#),
        );
        assert_eq!(p.status, Some(ProviderStatus::UnknownAuth));
        assert!(matches!(p.models_source, Some(ModelsSource::Cli)));
        let acct = p.account.as_ref().expect("account");
        assert_eq!(acct.billing, usage::Billing::PayAsYouGo);
        assert!(acct.plan.is_none());
        assert!(acct.balance.is_none());
    }

    #[test]
    fn claude_catalog_layers_config_on_the_aliases() {
        let dir = std::env::temp_dir().join(format!("tokage-cc-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".claude")).unwrap();
        std::fs::write(
            dir.join(".claude.json"),
            r#"{"oauthAccount":{"emailAddress":"x@y.z"},"additionalModelOptionsCache":[{"value":"claude-fable-5-1[1m]","label":"Fable","description":"d"}]}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join(".claude/settings.json"),
            r#"{"model":"claude-opus-5-5","env":{"ANTHROPIC_DEFAULT_SONNET_MODEL":"claude-sonnet-5-5"}}"#,
        )
        .unwrap();
        let mut vars = std::collections::HashMap::new();
        vars.insert("HOME".into(), dir.to_string_lossy().into_owned());
        let src = usage::claude_model_sources(&ShellEnv { vars });
        let models = claude_models(&src);
        let ids: Vec<_> = models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            ids,
            ["opus", "opus[1m]", "sonnet", "sonnet[1m]", "haiku", "claude-fable-5-1[1m]", "claude-opus-5-5"]
        );
        let groups = claude_model_groups(&models);
        let sonnet = groups.iter().find(|g| g.key == "sonnet").unwrap();
        assert_eq!(sonnet.name, "Sonnet");
        assert!(sonnet.tags.iter().any(|t| t == "claude-sonnet-5-5"), "{:?}", sonnet.tags);
        assert_eq!(sonnet.default_effort.as_deref(), Some("default"));
        assert!(sonnet.efforts.contains(&"max".to_string()));
        assert!(groups.iter().find(|g| g.key == "haiku").unwrap().efforts.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn claude_without_extra_options_has_no_fable_and_allowlist_filters() {
        let none = claude_models(&usage::ClaudeModelSources::default());
        assert!(none.iter().all(|m| !m.id.contains("fable")));
        let src = usage::ClaudeModelSources {
            available: vec!["sonnet".into(), "claude-opus-4-8".into()],
            ..Default::default()
        };
        let ids: Vec<_> = claude_models(&src).into_iter().map(|m| m.id).collect();
        assert_eq!(ids, ["sonnet", "sonnet[1m]", "claude-opus-4-8"]);
    }

    #[test]
    fn reasonix_config_fallback_reads_env_file() {
        let dir = std::env::temp_dir().join(format!("tokage-rx-{}", std::process::id()));
        let rx = dir.join(".reasonix");
        std::fs::create_dir_all(&rx).unwrap();
        std::fs::write(
            rx.join("config.toml"),
            r#"
[[providers]]
name = "deepseek"
models = ["deepseek-v4-flash", "deepseek-v4-pro"]
api_key_env = "DEEPSEEK_API_KEY"
"#,
        )
        .unwrap();
        std::fs::write(rx.join(".env"), "DEEPSEEK_API_KEY=sk-test\n").unwrap();
        let mut vars = std::collections::HashMap::new();
        vars.insert("HOME".into(), dir.to_string_lossy().into_owned());
        let env = ShellEnv { vars };
        let mut p = Probe::default();
        apply_reasonix_config_fallback(&mut p, &env, None);
        assert_eq!(p.status, Some(ProviderStatus::UnknownAuth));
        assert_eq!(p.auth_method.as_deref(), Some("DEEPSEEK_API_KEY"));
        assert_eq!(p.models.len(), 2);
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(test)]
mod live {
    /// Manual check against the real machine: `cargo test --lib live -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn detect_on_this_machine() {
        let env = crate::env::capture().await;
        for p in super::detect_all(&env, &Default::default()).await {
            println!(
                "{:<12} {:?} v={:?} auth={:?} models={:?} guidance={:?}",
                p.name,
                p.status,
                p.version,
                p.auth_method,
                p.models.iter().map(|m| &m.id).collect::<Vec<_>>(),
                p.guidance.iter().map(|g| g.code).collect::<Vec<_>>()
            );
            if let Some(a) = &p.account {
                println!("    account: {}", serde_json::to_string(a).unwrap());
            }
            for g in &p.model_groups {
                println!(
                    "    {} efforts={:?} thinking={} fast={} n={}",
                    g.name,
                    g.efforts,
                    g.thinking,
                    g.fast,
                    g.variants.len()
                );
            }
        }
    }
}

#[cfg(test)]
mod aider_tests;

#[cfg(test)]
mod opencode_tests;

#[cfg(test)]
mod kiro_tests;
