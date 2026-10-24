//! Billing channel and subscription quota per provider (docs/06 §2, §4.1). Everything is read
//! from local files and read-only CLI commands; Tokage does not call vendor HTTP endpoints.
//! Credential files are parsed only for non-secret claims (auth mode, plan); tokens are
//! dropped on read. Base URLs leave this module only through `display_url`.

use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

use serde::{Deserialize, Serialize};

use crate::env::ShellEnv;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Billing {
    Subscription,
    PayAsYouGo,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub billing: Billing,
    /// Talks to the vendor's own endpoint. Quotas and list prices only apply when true.
    pub official: bool,
    /// Custom endpoint as shown to the user (see `display_url`), or a cloud / provider name.
    pub endpoint: Option<String>,
    /// Subscription tier as the vendor names it ("plus", "Pro+", "max").
    pub plan: Option<String>,
    /// Live PAYG wallet from the CLI's own status surface (e.g. Reasonix `/status` → `¥29.55`).
    pub balance: Option<String>,
    pub usage: Option<UsageSnapshot>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSnapshot {
    pub windows: Vec<UsageWindow>,
    /// Additional plan rows from the CLI. Missing on older snapshots.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<UsageDetails>,
    /// When the CLI recorded these numbers (ISO 8601), not when Tokage read them.
    pub observed_at: Option<String>,
    pub source: UsageSource,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageDetails {
    pub plan: String,
    /// Preserve the CLI's date label: a month/day is not an exact reset timestamp.
    pub reset_label: Option<String>,
    pub rows: Vec<UsageDetailRow>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageDetailRow {
    pub kind: UsageRowKind,
    pub used_percent: Option<f64>,
    pub value: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UsageRowKind { Included, Auto, Api, OnDemand }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    /// Zero denotes a plan allocation whose period is not exposed by the CLI.
    pub window_minutes: u32,
    pub used_percent: f64,
    /// Unix seconds. A past value means the window has reset since the snapshot.
    pub resets_at: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UsageSource {
    /// Rate-limit event in the CLI's own session log.
    SessionLog,
    /// Passive quota collection from CLI stream/output during chat turn (P3.m7d).
    CliOutput,
    /// Claude Code statusline cache file (P3.m7e).
    StatuslineCache,
    /// Local projection from cost accumulator (docs/06 §4.2, P8.m2a).
    Estimated,
}

static PROVIDER_USAGE_CACHE: std::sync::RwLock<Option<std::collections::HashMap<tokage_core::provider::ProviderId, UsageSnapshot>>> =
    std::sync::RwLock::new(None);

static PROVIDER_BALANCE_CACHE: std::sync::RwLock<
    Option<std::collections::HashMap<tokage_core::provider::ProviderId, String>>,
> = std::sync::RwLock::new(None);

pub fn get_cached_usage(id: tokage_core::provider::ProviderId) -> Option<UsageSnapshot> {
    PROVIDER_USAGE_CACHE
        .read()
        .ok()?
        .as_ref()?
        .get(&id)
        .cloned()
}

pub fn record_cli_output_usage(id: tokage_core::provider::ProviderId, snapshot: UsageSnapshot) {
    if let Ok(mut lock) = PROVIDER_USAGE_CACHE.write() {
        let map = lock.get_or_insert_with(std::collections::HashMap::new);
        map.insert(id, snapshot);
    }
}

pub fn get_cached_balance(id: tokage_core::provider::ProviderId) -> Option<String> {
    PROVIDER_BALANCE_CACHE
        .read()
        .ok()?
        .as_ref()?
        .get(&id)
        .cloned()
}

pub fn record_cli_balance(id: tokage_core::provider::ProviderId, balance: String) {
    if balance.trim().is_empty() {
        return;
    }
    if let Ok(mut lock) = PROVIDER_BALANCE_CACHE.write() {
        let map = lock.get_or_insert_with(std::collections::HashMap::new);
        map.insert(id, balance);
    }
}

/// Dynamic calibration state for `quota_equiv_cost` (docs/06 §4.2, P8.m2a).
/// Default initial equivalent cost is $15.00 per full quota window.
static QUOTA_EQUIV_COST: std::sync::RwLock<Option<std::collections::HashMap<tokage_core::provider::ProviderId, f64>>> =
    std::sync::RwLock::new(None);

pub const DEFAULT_QUOTA_EQUIV_COST: f64 = 15.0;

/// Gets current calibrated `quota_equiv_cost` for a provider.
pub fn get_quota_equiv_cost(id: tokage_core::provider::ProviderId) -> f64 {
    QUOTA_EQUIV_COST
        .read()
        .ok()
        .and_then(|lock| lock.as_ref().and_then(|map| map.get(&id).copied()))
        .unwrap_or(DEFAULT_QUOTA_EQUIV_COST)
}

/// Calibrates `quota_equiv_cost` from two snapshots with observed usage delta and spent equivalent cost:
/// `quota_equiv_cost = equiv_cost_delta / (u2 - u1)` (docs/06 §4.2).
pub fn calibrate_quota_equiv_cost(
    id: tokage_core::provider::ProviderId,
    u_last: f64,
    u_new: f64,
    equiv_cost_between: f64,
) -> f64 {
    let delta_u = u_new - u_last;
    if delta_u > 0.01 && equiv_cost_between > 0.01 {
        let fitted = equiv_cost_between / delta_u;
        // Dampen with exponential moving average (EMA weight: 0.3)
        let current = get_quota_equiv_cost(id);
        let updated = 0.7 * current + 0.3 * fitted;
        if let Ok(mut lock) = QUOTA_EQUIV_COST.write() {
            let map = lock.get_or_insert_with(std::collections::HashMap::new);
            map.insert(id, updated);
        }
        updated
    } else {
        get_quota_equiv_cost(id)
    }
}

/// Locally projects current utilization `u_now ≈ u_last + Σ equiv_cost_since_last / quota_equiv_cost` (docs/06 §4.2, P8.m2a).
pub fn project_utilization_now(
    id: tokage_core::provider::ProviderId,
    equiv_cost_since_last: f64,
) -> (f64, UsageSource) {
    let base_snap = get_cached_usage(id);
    let (u_last, source) = match base_snap {
        Some(snap) => {
            let u = snap.windows.first().map(|w| w.used_percent / 100.0).unwrap_or(0.0);
            (u, snap.source)
        }
        None => (0.0, UsageSource::Estimated),
    };

    if equiv_cost_since_last <= 0.0 {
        return (u_last, source);
    }

    let quota_cost = get_quota_equiv_cost(id);
    let u_proj = (u_last + (equiv_cost_since_last / quota_cost)).clamp(0.0, 1.0);
    (u_proj, UsageSource::Estimated)
}

impl Account {
    fn new(billing: Billing) -> Self {
        Account {
            billing,
            official: true,
            endpoint: None,
            plan: None,
            balance: None,
            usage: None,
        }
    }

    fn custom_endpoint(&mut self, url: &str) {
        self.official = false;
        self.endpoint = display_url(url);
    }
}

/// "https://api.example.com:8443/v1" → "api.example.com".
pub(crate) fn host_of(url: &str) -> Option<String> {
    let rest = url.trim().split_once("://").map_or(url.trim(), |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = host.split(':').next()?;
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// Mask for a path segment that looks like a credential.
const MASK: &str = "***";

/// "https://user:pw@Api.Example.com:8443/v1/sk-abc…/x?key=y#f" → "https://api.example.com:8443/v1/***/x".
/// Drops userinfo, query and fragment; masks path segments that look like keys or tokens
/// (some relays put the key in the path). A bare name such as a Codex provider id passes through.
pub fn display_url(url: &str) -> Option<String> {
    let url = url.trim();
    let (scheme, rest) = match url.split_once("://") {
        Some((s, r)) => (Some(s.to_ascii_lowercase()), r),
        None => (None, url),
    };
    let end = rest.find(['?', '#']).unwrap_or(rest.len());
    let rest = &rest[..end];
    let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    if host.is_empty() {
        return None;
    }
    let mut out = String::new();
    if let Some(s) = scheme {
        out.push_str(&s);
        out.push_str("://");
    }
    out.push_str(&host.to_ascii_lowercase());
    for seg in path.split('/').skip(1) {
        out.push('/');
        out.push_str(if looks_secret(seg) { MASK } else { seg });
    }
    Some(out)
}

/// Known key prefixes, or a long run of mixed letters and digits (random ids, tokens).
/// Readable path words ("v1", "openai", "chat-completions") are kept.
fn looks_secret(seg: &str) -> bool {
    const PREFIXES: [&str; 8] = ["sk-", "sk_", "ghp_", "gho_", "github_pat_", "xai-", "AIza", "AKIA"];
    if PREFIXES.iter().any(|p| seg.starts_with(p)) && seg.len() > 8 {
        return true;
    }
    let digits = seg.bytes().filter(u8::is_ascii_digit).count();
    let alpha = seg.bytes().filter(u8::is_ascii_alphabetic).count();
    seg.len() >= 32 || (seg.len() >= 16 && digits >= 3 && alpha >= 3)
}

fn env_val<'a>(env: &'a ShellEnv, key: &str) -> Option<&'a str> {
    env.vars
        .get(key)
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
}

fn home(env: &ShellEnv) -> Option<PathBuf> {
    env_val(env, "HOME")
        .or_else(|| env_val(env, "USERPROFILE"))
        .map(PathBuf::from)
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

// ---------------------------------------------------------------- Claude Code

/// What `~/.claude/settings.json` contributes. Claude Code applies its `env` block on top of
/// the process environment, so these win over the login shell.
#[derive(Default)]
pub struct ClaudeSettings {
    pub base_url: Option<String>,
    pub has_token: bool,
    pub cloud: Option<&'static str>,
}

pub fn claude_settings(env: &ShellEnv) -> ClaudeSettings {
    let dir = env_val(env, "CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| home(env).map(|h| h.join(".claude")));
    let file_env = dir
        .and_then(|d| read_json(&d.join("settings.json")))
        .and_then(|v| v.get("env").cloned());
    let get = |k: &str| -> Option<String> {
        file_env
            .as_ref()
            .and_then(|e| e.get(k))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
            .or_else(|| env_val(env, k).map(str::to_string))
    };
    let on = |k: &str| get(k).is_some_and(|v| v != "0" && v != "false");
    ClaudeSettings {
        base_url: get("ANTHROPIC_BASE_URL"),
        has_token: get("ANTHROPIC_AUTH_TOKEN").is_some() || get("ANTHROPIC_API_KEY").is_some(),
        cloud: if on("CLAUDE_CODE_USE_BEDROCK") {
            Some("Amazon Bedrock")
        } else if on("CLAUDE_CODE_USE_VERTEX") {
            Some("Google Vertex AI")
        } else {
            None
        },
    }
}

/// Model names Claude Code's own config files expose, read without a prompt. Only model
/// names are taken from these files; nothing else in them is used.
#[derive(Default, Debug, PartialEq)]
pub struct ClaudeModelSources {
    /// Extra picker entries Claude Code caches for the account (`value`, `label`), e.g.
    /// `claude-fable-5-1[1m]` for plans that include Fable.
    pub extra: Vec<(String, String)>,
    /// `availableModels` from settings.json: an allow-list when non-empty.
    pub available: Vec<String>,
    /// The `model` default from settings.json.
    pub default_model: Option<String>,
    /// `ANTHROPIC_DEFAULT_{OPUS,SONNET,HAIKU}_MODEL`: what each alias resolves to here.
    pub alias_targets: Vec<(&'static str, String)>,
}

pub fn claude_model_sources(env: &ShellEnv) -> ClaudeModelSources {
    let dir = env_val(env, "CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| home(env).map(|h| h.join(".claude")));
    let settings = dir.as_ref().and_then(|d| read_json(&d.join("settings.json")));
    // The global state file lives beside the config dir (or in HOME when it is the default).
    let state = match env_val(env, "CLAUDE_CONFIG_DIR") {
        Some(d) => read_json(&PathBuf::from(d).join(".claude.json")),
        None => home(env).and_then(|h| read_json(&h.join(".claude.json"))),
    };
    let mut out = ClaudeModelSources::default();
    if let Some(items) = state
        .as_ref()
        .and_then(|v| v.get("additionalModelOptionsCache")?.as_array().cloned())
    {
        for it in items {
            let value = it.get("value").and_then(|v| v.as_str()).map(str::trim);
            let label = it.get("label").and_then(|v| v.as_str()).map(str::trim);
            if let Some(v) = value.filter(|v| !v.is_empty()) {
                out.extra.push((v.to_string(), label.filter(|l| !l.is_empty()).unwrap_or(v).to_string()));
            }
        }
    }
    if let Some(s) = &settings {
        out.available = s
            .get("availableModels")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(|x| x.trim().to_string())).filter(|x| !x.is_empty()).collect())
            .unwrap_or_default();
        out.default_model = s
            .get("model")
            .and_then(|v| v.as_str())
            .map(|m| m.trim().to_string())
            .filter(|m| !m.is_empty());
    }
    let file_env = settings.as_ref().and_then(|v| v.get("env"));
    for (alias, key) in [
        ("opus", "ANTHROPIC_DEFAULT_OPUS_MODEL"),
        ("sonnet", "ANTHROPIC_DEFAULT_SONNET_MODEL"),
        ("haiku", "ANTHROPIC_DEFAULT_HAIKU_MODEL"),
    ] {
        let v = file_env
            .and_then(|e| e.get(key))
            .and_then(|v| v.as_str())
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .or_else(|| env_val(env, key).map(str::to_string));
        if let Some(v) = v {
            out.alias_targets.push((alias, v));
        }
    }
    out
}

/// `auth_method` comes from `claude auth status`; `subscription` from the same JSON when the
/// installed version reports it.
pub fn claude_account(
    s: &ClaudeSettings,
    auth_method: Option<&str>,
    subscription: Option<&str>,
    env: Option<&ShellEnv>,
) -> Account {
    if let Some(cloud) = s.cloud {
        let mut a = Account::new(Billing::PayAsYouGo);
        a.official = false;
        a.endpoint = Some(cloud.into());
        return a;
    }
    let subscribed = matches!(auth_method, Some("claude.ai" | "oauth" | "oauth_token"));
    let mut a = Account::new(if subscribed && !s.has_token {
        Billing::Subscription
    } else if s.has_token || auth_method.is_some_and(|m| m.contains("api")) {
        Billing::PayAsYouGo
    } else {
        Billing::Unknown
    });
    if let Some(url) = s
        .base_url
        .as_deref()
        .filter(|u| !is_official_host(u, &["anthropic.com"]))
    {
        a.custom_endpoint(url);
        a.billing = Billing::Unknown;
    }
    a.plan = subscription.map(str::to_string);
    if a.official && a.billing == Billing::Subscription {
        if let Some(env) = env {
            a.usage = read_claude_statusline_cache(env);
        }
        if a.usage.is_none() {
            a.usage = get_cached_usage(tokage_core::provider::ProviderId::ClaudeCode);
        }
    }
    a
}

/// Parses Claude Code statusline JSON output saved by user's statusline script (P3.m7e).
pub fn parse_claude_statusline_json(val: &serde_json::Value) -> Option<UsageSnapshot> {
    let rl = val.get("rate_limits").unwrap_or(val);
    let mut windows = Vec::new();

    if let Some(util5) = rl.get("five_hour_utilization").and_then(|v| v.as_f64()) {
        let pct = if util5 <= 1.0 { util5 * 100.0 } else { util5 };
        let reset = rl.get("five_hour_resets_at").and_then(|v| v.as_i64());
        windows.push(UsageWindow {
            window_minutes: 300,
            used_percent: pct,
            resets_at: reset,
        });
    }
    if let Some(util7) = rl.get("seven_day_utilization").and_then(|v| v.as_f64()) {
        let pct = if util7 <= 1.0 { util7 * 100.0 } else { util7 };
        let reset = rl.get("seven_day_resets_at").and_then(|v| v.as_i64());
        windows.push(UsageWindow {
            window_minutes: 10080,
            used_percent: pct,
            resets_at: reset,
        });
    }

    if windows.is_empty() {
        for (key, win_mins) in [("primary", 300), ("secondary", 10080), ("session", 300), ("weekly", 10080)] {
            if let Some(w) = rl.get(key) {
                let pct = w.get("used_percent").or_else(|| w.get("utilization")).and_then(|v| v.as_f64()).unwrap_or(0.0);
                let pct = if pct <= 1.0 && pct > 0.0 { pct * 100.0 } else { pct };
                let reset = w.get("resets_at").and_then(|v| v.as_i64());
                windows.push(UsageWindow {
                    window_minutes: win_mins,
                    used_percent: pct,
                    resets_at: reset,
                });
            }
        }
    }

    if windows.is_empty() {
        return None;
    }

    Some(UsageSnapshot {
        windows,
        observed_at: val.get("timestamp").and_then(|t| t.as_str()).map(str::to_string),
        details: None, source: UsageSource::StatuslineCache,
    })
}

/// Reads Claude Code statusline cache file if configured by the user (P3.m7e).
/// Tokage only reads the agreed cache file (`~/.claude/tokage_statusline.json` or `~/.claude/rate_limits.json`);
/// it never modifies Claude Code settings.
pub fn read_claude_statusline_cache(env: &ShellEnv) -> Option<UsageSnapshot> {
    let dir = env_val(env, "CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| home(env).map(|h| h.join(".claude")))?;

    let candidates = [
        dir.join("tokage_statusline.json"),
        dir.join("rate_limits.json"),
    ];

    for path in &candidates {
        if let Some(val) = read_json(path) {
            if let Some(snapshot) = parse_claude_statusline_json(&val) {
                return Some(snapshot);
            }
        }
    }
    None
}

pub fn is_official_host(url: &str, domains: &[&str]) -> bool {
    host_of(url).is_some_and(|h| {
        domains
            .iter()
            .any(|d| h == *d || h.ends_with(&format!(".{d}")))
    })
}

// ---------------------------------------------------------------- Codex CLI

fn codex_home(env: &ShellEnv) -> Option<PathBuf> {
    env_val(env, "CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| home(env).map(|h| h.join(".codex")))
}

pub fn codex_account(env: &ShellEnv) -> Account {
    let Some(dir) = codex_home(env) else {
        return Account::new(Billing::Unknown);
    };
    let auth = read_json(&dir.join("auth.json"));
    let mode = auth
        .as_ref()
        .and_then(|a| a.get("auth_mode"))
        .and_then(|m| m.as_str())
        .map(str::to_ascii_lowercase);
    let mut a = Account::new(match mode.as_deref() {
        Some("chatgpt") => Billing::Subscription,
        Some("apikey" | "api_key") => Billing::PayAsYouGo,
        _ => Billing::Unknown,
    });
    if a.billing == Billing::Subscription {
        a.plan = auth
            .as_ref()
            .and_then(|v| v.pointer("/tokens/id_token"))
            .and_then(|t| t.as_str())
            .and_then(codex_plan_from_jwt);
    }
    drop(auth);

    let config = std::fs::read_to_string(dir.join("config.toml")).unwrap_or_default();
    if let Some(url) = codex_custom_endpoint(&config).or_else(|| {
        env_val(env, "OPENAI_BASE_URL")
            .filter(|u| !is_official_host(u, &["openai.com", "chatgpt.com"]))
            .map(str::to_string)
    }) {
        a.custom_endpoint(&url);
        a.billing = Billing::PayAsYouGo;
    }
    if a.official && a.billing == Billing::Subscription {
        a.usage = latest_codex_rate_limits(&dir.join("sessions"));
        if a.usage.is_none() {
            a.usage = get_cached_usage(tokage_core::provider::ProviderId::CodexCli);
        }
    }
    a
}

/// Reads the `chatgpt_plan_type` claim. The signature isn't checked: this is a display hint
/// about the user's own account, not an authorization decision.
fn codex_plan_from_jwt(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64url_decode(payload)?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    v.pointer("/https:~1~1api.openai.com~1auth/chatgpt_plan_type")
        .and_then(|p| p.as_str())
        .filter(|p| !p.is_empty())
        .map(str::to_string)
}

fn base64url_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut buf, mut bits) = (0u32, 0u32);
    for c in s.bytes().take_while(|c| *c != b'=') {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            _ => return None,
        } as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}

/// A non-default `model_provider` (top level of config.toml) routes Codex elsewhere. Returns
/// that provider's `base_url`, or its name when no URL is set. A tiny line scanner is enough
/// for these two keys and avoids a TOML dependency.
fn codex_custom_endpoint(config: &str) -> Option<String> {
    let unquote = |v: &str| {
        v.split('#')
            .next()
            .unwrap_or("")
            .trim()
            .trim_matches(['"', '\''])
            .to_string()
    };
    let mut section = String::new();
    let mut provider = None;
    let mut base_urls: Vec<(String, String)> = Vec::new();
    for line in config.lines() {
        let t = line.trim();
        if let Some(s) = t.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            section = s.trim().to_string();
            continue;
        }
        let Some((k, v)) = t.split_once('=') else {
            continue;
        };
        let k = k.trim();
        if section.is_empty() && k == "model_provider" {
            provider = Some(unquote(v));
        } else if k == "base_url" {
            if let Some(name) = section.strip_prefix("model_providers.") {
                base_urls.push((name.trim_matches(['"', '\'']).to_string(), unquote(v)));
            }
        } else if section.is_empty() && k == "openai_base_url" {
            let url = unquote(v);
            if !is_official_host(&url, &["openai.com", "chatgpt.com"]) {
                return Some(url);
            }
        }
    }
    let provider = provider.filter(|p| p != "openai")?;
    base_urls
        .into_iter()
        .find(|(n, _)| *n == provider)
        .map(|(_, u)| u)
        .or(Some(provider))
}

/// Codex appends `token_count` events carrying `rate_limits` to its rollout logs
/// (`sessions/YYYY/MM/DD/rollout-*.jsonl`). The most recently written log has the latest.
fn latest_codex_rate_limits(sessions: &Path) -> Option<UsageSnapshot> {
    let newest = recent_files(sessions, 3, 14)
        .into_iter()
        .max_by_key(|(_, mtime)| *mtime)?
        .0;
    let tail = read_tail(&newest, 2 * 1024 * 1024)?;
    tail.lines()
        .rev()
        .filter(|l| l.contains("\"rate_limits\""))
        .find_map(parse_codex_rate_limits)
}

fn parse_codex_rate_limits(line: &str) -> Option<UsageSnapshot> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let rl = v
        .pointer("/payload/rate_limits")
        .filter(|r| r.is_object())?;
    let windows: Vec<UsageWindow> = ["primary", "secondary"]
        .iter()
        .filter_map(|k| {
            let w = rl.get(*k)?;
            Some(UsageWindow {
                window_minutes: w.get("window_minutes")?.as_u64()? as u32,
                used_percent: w.get("used_percent")?.as_f64()?,
                resets_at: w.get("resets_at").and_then(|r| r.as_i64()),
            })
        })
        .collect();
    (!windows.is_empty()).then(|| UsageSnapshot {
        windows,
        observed_at: v
            .get("timestamp")
            .and_then(|t| t.as_str())
            .map(str::to_string),
        details: None, source: UsageSource::SessionLog,
    })
}

/// Files under the newest `days` leaf directories of a `YYYY/MM/DD` tree.
fn recent_files(root: &Path, depth: usize, days: usize) -> Vec<(PathBuf, SystemTime)> {
    fn sorted_dirs(p: &Path) -> Vec<PathBuf> {
        let mut d: Vec<PathBuf> = std::fs::read_dir(p)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        d.sort();
        d.reverse();
        d
    }
    let mut level = vec![root.to_path_buf()];
    for _ in 0..depth {
        level = level
            .iter()
            .flat_map(|p| sorted_dirs(p))
            .take(days)
            .collect();
    }
    level
        .iter()
        .flat_map(|d| std::fs::read_dir(d).into_iter().flatten().flatten())
        .filter(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
        .filter_map(|e| Some((e.path(), e.metadata().ok()?.modified().ok()?)))
        .collect()
}

fn read_tail(path: &Path, max: u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let start = len.saturating_sub(max);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::with_capacity((len - start) as usize);
    f.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf).into_owned();
    // Drop the partial first line when we started mid-file.
    Some(if start > 0 {
        text.split_once('\n')
            .map_or(String::new(), |(_, r)| r.to_string())
    } else {
        text
    })
}

// ---------------------------------------------------------------- Cursor CLI

/// `agent about --format json` reports `subscriptionTier`; it reads local state only.
pub fn cursor_account(env: &ShellEnv, about_json: Option<&str>) -> Account {
    let mut a = Account::new(if env_val(env, "CURSOR_API_KEY").is_some() {
        Billing::PayAsYouGo
    } else {
        Billing::Subscription
    });
    a.plan = about_json
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t[t.find('{')?..]).ok())
        .and_then(|v| v.get("subscriptionTier")?.as_str().map(str::to_string))
        .filter(|p| !p.is_empty());
    if let Some(url) = env_val(env, "CURSOR_API_ENDPOINT")
        .filter(|u| !is_official_host(u, &["cursor.sh", "cursor.com"]))
    {
        a.custom_endpoint(url);
    }
    if a.usage.is_none() {
        a.usage = get_cached_usage(tokage_core::provider::ProviderId::CursorCli);
    }
    a
}

// ---------------------------------------------------------------- Antigravity

pub fn antigravity_account(env: &ShellEnv, uses_api_key: bool) -> Account {
    let mut a = Account::new(if uses_api_key {
        Billing::PayAsYouGo
    } else {
        Billing::Subscription
    });
    if let Some(url) = env_val(env, "GOOGLE_GEMINI_BASE_URL").filter(|_| uses_api_key)
        .filter(|u| !is_official_host(u, &["googleapis.com", "google.com"]))
    {
        a.custom_endpoint(url);
    }
    if a.usage.is_none() {
        a.usage = get_cached_usage(tokage_core::provider::ProviderId::AntigravityCli);
    }
    a
}

// ---------------------------------------------------------------- Reasonix

/// `reasonix doctor billing --json` (local/redacted): PAYG vs subscription only.
/// Live wallet (optional) comes from CLI `/status` via [`record_cli_balance`].
/// Never invent a `plan` from billing currency — that shows as a fake "USD 套餐".
pub fn reasonix_account(billing_json: Option<&str>) -> Account {
    let mut a = Account::new(Billing::PayAsYouGo);
    a.usage = get_cached_usage(tokage_core::provider::ProviderId::Reasonix);
    a.balance = get_cached_balance(tokage_core::provider::ProviderId::Reasonix);
    let Some(text) = billing_json else {
        return a;
    };
    let Some(start) = text.find('{') else {
        return a;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text[start..].trim()) else {
        return a;
    };

    if let Some(providers) = v.get("providers").and_then(|p| p.as_array()) {
        for p in providers {
            let mode = p
                .get("billing_mode")
                .and_then(|m| m.as_str())
                .unwrap_or("payg");
            if mode.eq_ignore_ascii_case("subscription") {
                a.billing = Billing::Subscription;
            }
        }
    }
    a
}

/// Parse the `balance` line from Reasonix `/status` (PTY slash capture).
/// Example: `balance    ¥29.55`
pub fn parse_reasonix_status_balance(text: &str) -> Option<String> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"(?im)^\s*balance\s+(?P<bal>[¥$€]?\s*[\d][\d.,]*(?:\s*[A-Z]{3})?)\s*$")
            .expect("reasonix status balance regex")
    });
    let caps = re.captures(text)?;
    let bal = caps.name("bal")?.as_str().trim();
    (!bal.is_empty()).then(|| bal.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_url_strips_secrets() {
        assert_eq!(
            display_url("https://user:pw@Api.Example.com:8443/v1?key=x#f").as_deref(),
            Some("https://api.example.com:8443/v1")
        );
        assert_eq!(
            display_url("https://relay.example.org/sk-abcdefghijk123/v1/").as_deref(),
            Some("https://relay.example.org/***/v1/")
        );
        assert_eq!(
            display_url("https://relay.example.org/u/9f8e7d6c5b4a3f2e1d0c/chat-completions").as_deref(),
            Some("https://relay.example.org/u/***/chat-completions")
        );
        assert_eq!(display_url("ollama").as_deref(), Some("ollama"));
        assert_eq!(display_url("https://"), None);
    }

    /// R11: secrets in env vars and credential files never reach the IPC payload or `Debug`.
    #[test]
    fn secrets_are_dropped_on_read() {
        const SECRETS: [&str; 5] = [
            "sk-ant-SECRETclaude0001",
            "sk-SECRETopenai0002",
            "SECRETcursor0003",
            "SECRETgemini0004",
            "SECRETrefresh0005",
        ];
        let home = std::env::temp_dir().join(format!("tokage-r11-{}", std::process::id()));
        let (claude, codex) = (home.join(".claude"), home.join(".codex"));
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::create_dir_all(&codex).unwrap();
        let s0 = SECRETS[0];
        std::fs::write(
            claude.join("settings.json"),
            format!(
                r#"{{"env":{{"ANTHROPIC_API_KEY":"{s0}","ANTHROPIC_BASE_URL":"https://k:{s0}@relay.example.org/{s0}/v1?token={s0}"}}}}"#
            ),
        )
        .unwrap();
        // id_token payload: {"https://api.openai.com/auth":{"chatgpt_plan_type":"plus"}}
        let (s1, s4) = (SECRETS[1], SECRETS[4]);
        std::fs::write(
            codex.join("auth.json"),
            format!(
                r#"{{"auth_mode":"chatgpt","OPENAI_API_KEY":"{s1}","tokens":{{"id_token":"h.eyJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9wbGFuX3R5cGUiOiJwbHVzIn19.s","access_token":"{s1}","refresh_token":"{s4}"}}}}"#
            ),
        )
        .unwrap();
        std::fs::write(
            codex.join("config.toml"),
            format!("model_provider = \"r\"\n[model_providers.r]\nbase_url = \"https://relay.example.org/{s1}\"\n"),
        )
        .unwrap();
        let env = ShellEnv {
            vars: [
                ("HOME", home.to_str().unwrap().to_string()),
                ("CURSOR_API_KEY", SECRETS[2].into()),
                ("CURSOR_API_ENDPOINT", format!("https://c.example.org/?k={}", SECRETS[2])),
                ("GEMINI_API_KEY", SECRETS[3].into()),
                ("GOOGLE_GEMINI_BASE_URL", format!("https://g.example.org/{}", SECRETS[3])),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
        };

        let settings = claude_settings(&env);
        assert!(settings.has_token);
        let accounts = [
            claude_account(&settings, Some("api_key"), None, Some(&env)),
            codex_account(&env),
            cursor_account(&env, Some(r#"{"subscriptionTier":"pro"}"#)),
            antigravity_account(&env, true),
        ];
        for a in &accounts {
            assert!(!a.official, "{a:?}");
            let out = format!("{a:?}\n{}", serde_json::to_string(a).unwrap());
            for s in SECRETS {
                assert!(!out.contains(s), "{s} leaked: {out}");
            }
        }
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn host_only() {
        assert_eq!(
            host_of("https://user:pw@Api.Example.com:8443/v1?x").as_deref(),
            Some("api.example.com")
        );
        assert_eq!(
            host_of("api.example.com/v1").as_deref(),
            Some("api.example.com")
        );
        assert!(is_official_host(
            "https://api.anthropic.com",
            &["anthropic.com"]
        ));
        assert!(!is_official_host(
            "https://anthropic.com.evil.io",
            &["anthropic.com"]
        ));
    }

    #[test]
    fn codex_config_endpoint() {
        let cfg = "model = \"x\"\nmodel_provider = \"proxy\" # c\n[model_providers.proxy]\nname = \"P\"\nbase_url = \"https://relay.example.org/v1\"\n";
        assert_eq!(
            codex_custom_endpoint(cfg).as_deref(),
            Some("https://relay.example.org/v1")
        );
        assert_eq!(codex_custom_endpoint("model_provider = \"openai\"\n"), None);
        assert_eq!(
            codex_custom_endpoint("[profiles.a]\nmodel_provider = \"x\"\n"),
            None
        );
        assert_eq!(
            codex_custom_endpoint("model_provider = \"ollama\"\n").as_deref(),
            Some("ollama")
        );
    }

    #[test]
    fn codex_rate_limit_line() {
        let line = r#"{"timestamp":"2026-10-03T19:57:33.284Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":19.0,"window_minutes":300,"resets_at":1791071437},"secondary":{"used_percent":64.0,"window_minutes":10080,"resets_at":1791580413},"plan_type":"plus"}}}"#;
        let s = parse_codex_rate_limits(line).unwrap();
        assert_eq!(s.windows.len(), 2);
        assert_eq!(s.windows[1].window_minutes, 10080);
        assert_eq!(s.windows[0].used_percent, 19.0);
        assert!(parse_codex_rate_limits(r#"{"payload":{"rate_limits":null}}"#).is_none());
    }

    #[test]
    fn jwt_plan_claim() {
        // {"https://api.openai.com/auth":{"chatgpt_plan_type":"plus"}}
        let payload =
            "eyJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9wbGFuX3R5cGUiOiJwbHVzIn19";
        assert_eq!(
            codex_plan_from_jwt(&format!("h.{payload}.s")).as_deref(),
            Some("plus")
        );
        assert_eq!(codex_plan_from_jwt("garbage"), None);
    }

    #[test]
    fn claude_channel() {
        let relay = ClaudeSettings {
            base_url: Some("https://relay.example.org".into()),
            has_token: true,
            cloud: None,
        };
        let a = claude_account(&relay, Some("none"), None, None);
        assert!(!a.official);
        assert_eq!(a.billing, Billing::Unknown);
        assert_eq!(a.endpoint.as_deref(), Some("https://relay.example.org"));
        let sub = claude_account(&ClaudeSettings::default(), Some("claude.ai"), Some("max"), None);
        assert!(sub.official);
        assert_eq!(sub.billing, Billing::Subscription);
        assert_eq!(sub.plan.as_deref(), Some("max"));
    }

    #[test]
    fn claude_statusline_cache_parsing() {
        let sample = serde_json::json!({
            "rate_limits": {
                "five_hour_utilization": 0.42,
                "five_hour_resets_at": 1791071437,
                "seven_day_utilization": 0.15,
                "seven_day_resets_at": 1791580413
            }
        });
        let snap = parse_claude_statusline_json(&sample).unwrap();
        assert_eq!(snap.source, UsageSource::StatuslineCache);
        assert_eq!(snap.windows.len(), 2);
        assert_eq!(snap.windows[0].window_minutes, 300);
        assert_eq!(snap.windows[0].used_percent, 42.0);
        assert_eq!(snap.windows[0].resets_at, Some(1791071437));
        assert_eq!(snap.windows[1].window_minutes, 10080);
        assert_eq!(snap.windows[1].used_percent, 15.0);
    }

    #[test]
    fn reasonix_billing_payg_no_fake_plan() {
        let payg = r#"{
          "display_currency":"USD",
          "providers":[{"name":"deepseek","billing_currency":"USD","billing_mode":"payg"}]
        }"#;
        let a = reasonix_account(Some(payg));
        assert_eq!(a.billing, Billing::PayAsYouGo);
        assert!(a.plan.is_none(), "billing_currency must not become plan");
        assert!(a.balance.is_none());
        assert!(a.official);
    }

    #[test]
    fn reasonix_status_balance_line() {
        let frame = r#"
  › /status
  · Session status
  mode       Ask
  model      deepseek/deepseek-flash
  context    6.3K ctx (0%) · 85% to compact
  effort     effort auto
  balance    ¥29.55
  config     ~/.reasonix/config.toml
"#;
        assert_eq!(
            parse_reasonix_status_balance(frame).as_deref(),
            Some("¥29.55")
        );
    }
}

#[cfg(test)]
mod antigravity_account_tests {
    use super::*;
    #[test]
    fn gemini_gateway_endpoint_does_not_relabel_google_login() {
        let env = ShellEnv { vars: [("GOOGLE_GEMINI_BASE_URL".into(), "https://gateway.example.test".into())].into() };
        assert!(antigravity_account(&env, false).official);
        assert!(!antigravity_account(&env, true).official);
    }
}
