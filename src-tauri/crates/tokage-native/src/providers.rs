//! Provider detection (docs/01 §8): which → --version → login state → model list.
//! Nothing here sends a prompt, so no tokens are spent. Credential files are only checked for
//! existence; config files are scanned for key *names*, never values.

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
        login: Some("copilot login"),
        ..spec(ProviderId::CopilotCli, "GitHub Copilot CLI", &["copilot"])
    },
    spec(ProviderId::Aider, "Aider", &["aider"]),
    Spec {
        featured: true,
        auth: files(&[".local/share/opencode/auth.json"], &[]),
        login: Some("opencode auth login"),
        models: Some(&["models"]),
        ..spec(ProviderId::Opencode, "OpenCode", &["opencode"])
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

pub async fn detect_all(env: &ShellEnv, custom: &HashMap<ProviderId, String>) -> Vec<ProviderInfo> {
    detect_each(env, custom, |_| {}).await
}

/// Re-detects a single whitelisted CLI, leaving the others untouched.
pub async fn detect_one(
    env: &ShellEnv,
    id: ProviderId,
    custom: Option<&str>,
) -> Option<ProviderInfo> {
    let spec = REGISTRY.iter().find(|s| s.id == id)?;
    let custom = custom.map(str::trim).filter(|s| !s.is_empty());
    Some(detect_with_timeout(spec, env, &Toolbox::detect(env), custom).await)
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
        ProviderId::CopilotCli => probe_copilot(exe, env).await,
        ProviderId::CursorCli => {
            let (mut p, about) = tokio::join!(
                probe_generic(spec, exe, env),
                run(exe, &["about", "--format", "json"], env, 15)
            );
            if matches!(p.status, Some(ProviderStatus::Ready)) {
                let about = about.filter(|(ok, _)| *ok).map(|(_, t)| t);
                p.account = Some(usage::cursor_account(env, about.as_deref()));
            }
            p
        }
        _ => probe_generic(spec, exe, env).await,
    }
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
        Auth::Status(args) => match run(exe, args, env, 25).await {
            Some((true, out)) => {
                p.status = Some(ProviderStatus::Ready);
                p.auth_method = signed_in_as(&out);
            }
            Some((false, _)) => signed_out(&mut p, ProviderStatus::NeedsLogin),
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
                            .find_map(|k| o.get(*k).and_then(|x| x.as_str()));
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

/// `claude auth status` prints JSON and exits 0 when logged in, 1 when not. A token in
/// `~/.claude/settings.json` (custom gateway) also counts as signed in, even when the
/// status command reports `none`.
async fn probe_claude(exe: &Path, env: &ShellEnv) -> Probe {
    let mut p = Probe::default();
    let settings = usage::claude_settings(env);
    let mut subscription = None;
    match run(exe, &["auth", "status"], env, 12).await {
        Some((ok, out)) => {
            // Stderr warnings may surround the JSON object; parse just the braces.
            let json = out
                .find('{')
                .zip(out.rfind('}'))
                .map_or("", |(a, b)| &out[a..=b]);
            let v = serde_json::from_str::<serde_json::Value>(json).ok();
            let field = |k: &str| {
                v.as_ref()
                    .and_then(|v| v.get(k))
                    .and_then(|m| m.as_str())
                    .filter(|m| !m.is_empty())
                    .map(str::to_string)
            };
            let mut method = field("authMethod");
            subscription = field("subscriptionType");
            let mut ok = ok && method.as_deref() != Some("none");
            if !ok && (settings.has_token || settings.cloud.is_some()) {
                ok = true;
                method = Some(settings.cloud.unwrap_or("api_key").to_string());
            }
            let third_party = method.as_deref() == Some("third_party")
                || settings.cloud.is_some()
                || settings.base_url.is_some();
            if ok {
                p.status = Some(ProviderStatus::Ready);
                if third_party {
                    p.guidance
                        .push(guide(GuidanceCode::ThirdPartyBackend, None));
                }
                p.auth_method = method;
            } else {
                p.status = Some(ProviderStatus::NeedsLogin);
                p.guidance
                    .push(guide(GuidanceCode::Login, Some("claude auth login")));
            }
        }
        // Gateway token configured: Claude Code will use it regardless of `auth status`.
        None if settings.has_token || settings.cloud.is_some() => {
            p.status = Some(ProviderStatus::Ready);
            p.auth_method = Some(settings.cloud.unwrap_or("api_key").to_string());
            p.guidance
                .push(guide(GuidanceCode::ThirdPartyBackend, None));
        }
        // Older builds without `auth status`: fall back to presence checks.
        None => {
            p.status = Some(ProviderStatus::UnknownAuth);
            p.guidance
                .push(guide(GuidanceCode::VerifyLogin, Some("claude")));
        }
    }
    if matches!(p.status, Some(ProviderStatus::Ready)) {
        p.account = Some(usage::claude_account(
            &settings,
            p.auth_method.as_deref(),
            subscription.as_deref(),
        ));
        // `opus[1m]` / `sonnet[1m]` are the documented 1M-context aliases.
        p.models = ["fable", "opus", "opus[1m]", "sonnet", "sonnet[1m]", "haiku"]
            .iter()
            .map(|a| ModelInfo {
                id: (*a).into(),
                name: match a.split_once('[') {
                    Some((base, _)) => format!("{} 1M", capitalize(base)),
                    None => capitalize(a),
                },
            })
            .collect();
        p.models_source = Some(ModelsSource::Builtin);
    }
    p
}

/// `codex login status` exits 0 with "Logged in using …". Models come from the CLI's
/// own catalog cache (`$CODEX_HOME/models_cache.json`), which it refreshes itself.
async fn probe_codex(exe: &Path, env: &ShellEnv) -> Probe {
    let mut p = Probe::default();
    match run(exe, &["login", "status"], env, 20).await {
        Some((true, out)) => {
            p.status = Some(ProviderStatus::Ready);
            p.auth_method = out
                .lines()
                .find_map(|l| l.trim().strip_prefix("Logged in using "))
                .map(|m| m.trim().to_string());
        }
        Some((false, _)) => {
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
async fn probe_antigravity(exe: &Path, env: &ShellEnv) -> Probe {
    let mut p = Probe::default();
    match run(exe, &["models"], env, 15).await {
        Some((true, out)) => {
            p.models = out
                .lines()
                .filter_map(|l| {
                    let (id, name) = l.split_once('\t')?;
                    let id = id.trim();
                    (!id.is_empty() && !id.contains(' ')).then(|| ModelInfo {
                        id: id.into(),
                        name: name.trim().into(),
                    })
                })
                .collect();
            p.status = Some(ProviderStatus::Ready);
            p.models_source = (!p.models.is_empty()).then_some(ModelsSource::Cli);
            let api_key = antigravity_uses_api_key(env);
            p.account = Some(usage::antigravity_account(env, api_key));
            p.auth_method = Some(if api_key {
                "Gemini API key".into()
            } else {
                "Google account".into()
            });
            if p.models.is_empty() {
                p.guidance
                    .push(guide(GuidanceCode::NoModels, Some("agy models")));
            }
        }
        Some((false, _)) => {
            p.status = Some(ProviderStatus::NeedsLogin);
            p.guidance.push(guide(GuidanceCode::Login, Some("agy")));
        }
        None => {
            p.status = Some(ProviderStatus::UnknownAuth);
            p.guidance
                .push(guide(GuidanceCode::VerifyLogin, Some("agy models")));
        }
    }
    p
}

/// Copilot CLI's own credential order: env tokens, then the OAuth token `copilot login`
/// stores in the OS keychain (service `copilot-cli`), or `config.json` when no keychain
/// exists, then the GitHub CLI's login. Only presence is checked; no secret is read.
/// Models come from `copilot help config`, which prints the catalog offline.
async fn probe_copilot(exe: &Path, env: &ShellEnv) -> Probe {
    let mut p = Probe::default();
    let (method, help) = tokio::join!(copilot_auth(env), run(exe, &["help", "config"], env, 10));
    match method {
        Some(m) => {
            p.status = Some(ProviderStatus::Ready);
            p.auth_method = Some(m);
            p.models = help
                .filter(|(ok, _)| *ok)
                .map(|(_, t)| parse_copilot_models(&t))
                .unwrap_or_default();
            if p.models.is_empty() {
                p.guidance
                    .push(guide(GuidanceCode::NoModels, Some("copilot help config")));
            } else {
                p.models_source = Some(ModelsSource::Cli);
                p.model_groups = Some(copilot_model_groups(&p.models));
            }
        }
        None => {
            p.status = Some(ProviderStatus::NeedsLogin);
            p.guidance
                .push(guide(GuidanceCode::Login, Some("copilot login")));
        }
    }
    p
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

async fn copilot_auth(env: &ShellEnv) -> Option<String> {
    if let Some(k) = ["COPILOT_GITHUB_TOKEN", "GH_TOKEN", "GITHUB_TOKEN"]
        .into_iter()
        .find(|k| has_env(env, k))
    {
        Some(k.to_string())
    } else if copilot_in_keychain(env).await {
        Some("GitHub OAuth (keychain)".into())
    } else if copilot_config_has_login(env) {
        Some("GitHub OAuth".into())
    } else if gh_logged_in(env).await {
        Some("GitHub CLI (gh)".into())
    } else {
        None
    }
}

/// `security find-generic-password` without `-w` prints only item attributes, never the
/// password, and needs no keychain unlock prompt. Exit 0 means the item exists.
async fn copilot_in_keychain(env: &ShellEnv) -> bool {
    if !cfg!(target_os = "macos") {
        return false;
    }
    let security = Path::new("/usr/bin/security");
    matches!(
        run(security, &["find-generic-password", "-s", "copilot-cli"], env, 5).await,
        Some((true, _))
    )
}

/// Keychain-less installs keep the login in `$COPILOT_HOME/config.json` (JSON with `//`
/// comment lines). Looks for a non-empty logged-in-user entry; token values are ignored.
fn copilot_config_has_login(env: &ShellEnv) -> bool {
    let dir = env
        .vars
        .get("COPILOT_HOME")
        .map(PathBuf::from)
        .or_else(|| home(env).map(|h| h.join(".copilot")));
    let Some(text) = dir.and_then(|d| std::fs::read_to_string(d.join("config.json")).ok()) else {
        return false;
    };
    let json: String = text
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&json) else {
        return false;
    };
    ["logged_in_users", "loggedInUsers", "last_logged_in_user", "lastLoggedInUser"]
        .iter()
        .filter_map(|k| v.get(*k))
        .any(|u| match u {
            serde_json::Value::Array(a) => !a.is_empty(),
            serde_json::Value::Object(o) => !o.is_empty(),
            serde_json::Value::String(s) => !s.is_empty(),
            _ => false,
        })
}

/// Copilot falls back to `gh auth token`; `gh auth status` exits 0 when gh is signed in.
async fn gh_logged_in(env: &ShellEnv) -> bool {
    let Ok(cwd) = std::env::current_dir() else {
        return false;
    };
    let Ok(gh) = which::which_in("gh", env.path(), &cwd) else {
        return false;
    };
    matches!(run(&gh, &["auth", "status"], env, 10).await, Some((true, _)))
}

/// Headless setups put `"modelProvider": "gemini"` in the CLI's settings and export
/// GEMINI_API_KEY. The settings file holds no secrets; the key itself is never read.
fn antigravity_uses_api_key(env: &ShellEnv) -> bool {
    has_env(env, "GEMINI_API_KEY")
        && home(env)
            .and_then(|h| {
                std::fs::read_to_string(h.join(".gemini/antigravity-cli/settings.json")).ok()
            })
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .is_some_and(|v| v.get("modelProvider").and_then(|m| m.as_str()) == Some("gemini"))
}

/// Aider has no account; it needs at least one provider API key. Keys can come from the
/// environment, `~/.aider.conf.yml` or `~/.env`. Only key *names* are inspected.
const AIDER_KEYS: &[(&str, &str, &str)] = &[
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
        .filter(|(k, ..)| has_env(env, k))
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

/// `KEY=value` lines → key names. Values are dropped immediately.
fn key_names_in(path: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return vec![];
    };
    text.lines()
        .filter_map(|l| {
            let l = l.trim().strip_prefix("export ").unwrap_or(l.trim());
            let (k, v) = l.split_once('=')?;
            (!k.starts_with('#') && !v.trim().is_empty()).then(|| k.trim().to_string())
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
    use super::*;

    #[test]
    fn copilot_config_login_detection() {
        let dir = std::env::temp_dir().join(format!("tokage-copilot-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let env = ShellEnv {
            vars: [("COPILOT_HOME".to_string(), dir.display().to_string())].into(),
        };
        let cfg = dir.join("config.json");
        assert!(!copilot_config_has_login(&env), "missing file");
        std::fs::write(&cfg, "// managed by copilot\n{\"logged_in_users\": []}").unwrap();
        assert!(!copilot_config_has_login(&env), "empty list");
        std::fs::write(&cfg, "// managed by copilot\n{\"logged_in_users\": [{\"login\": \"x\"}]}").unwrap();
        assert!(copilot_config_has_login(&env), "user with comment header");
        std::fs::write(&cfg, "{\"lastLoggedInUser\": {\"login\": \"x\"}}").unwrap();
        assert!(copilot_config_has_login(&env), "camelCase key");
        std::fs::remove_dir_all(&dir).unwrap();
    }

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
