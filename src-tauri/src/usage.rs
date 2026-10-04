//! Billing channel and subscription quota per provider (docs/06 §2, §4.1). Everything is read
//! from local files and read-only CLI commands; no request is sent to any vendor. Credential
//! files are parsed only for non-secret claims (auth mode, plan); tokens are dropped on read.
//! Base URLs leave this module only through `display_url` (no userinfo / query / fragment,
//! token-like path segments masked).

use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

use serde::Serialize;

use crate::env::ShellEnv;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Billing {
    Subscription,
    PayAsYouGo,
    Unknown,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub billing: Billing,
    /// Talks to the vendor's own endpoint. Quotas and list prices only apply when true.
    pub official: bool,
    /// Custom endpoint as shown to the user (see `display_url`), or a cloud / provider name.
    pub endpoint: Option<String>,
    /// Subscription tier as the vendor names it ("plus", "Pro+", "max").
    pub plan: Option<String>,
    pub usage: Option<UsageSnapshot>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSnapshot {
    pub windows: Vec<UsageWindow>,
    /// When the CLI recorded these numbers (ISO 8601), not when Tokage read them.
    pub observed_at: Option<String>,
    pub source: UsageSource,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    pub window_minutes: u32,
    pub used_percent: f64,
    /// Unix seconds. A past value means the window has reset since the snapshot.
    pub resets_at: Option<i64>,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UsageSource {
    /// Rate-limit event in the CLI's own session log.
    SessionLog,
}

impl Account {
    fn new(billing: Billing) -> Self {
        Account {
            billing,
            official: true,
            endpoint: None,
            plan: None,
            usage: None,
        }
    }

    fn custom_endpoint(&mut self, url: &str) {
        self.official = false;
        self.endpoint = display_url(url);
    }
}

/// "https://api.example.com:8443/v1" → "api.example.com".
fn host_of(url: &str) -> Option<String> {
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

/// `auth_method` comes from `claude auth status`; `subscription` from the same JSON when the
/// installed version reports it.
pub fn claude_account(
    s: &ClaudeSettings,
    auth_method: Option<&str>,
    subscription: Option<&str>,
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
    }
    a.plan = subscription.map(str::to_string);
    a
}

fn is_official_host(url: &str, domains: &[&str]) -> bool {
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
        source: UsageSource::SessionLog,
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
    a
}

// ---------------------------------------------------------------- Antigravity

pub fn antigravity_account(env: &ShellEnv, uses_api_key: bool) -> Account {
    let mut a = Account::new(if uses_api_key {
        Billing::PayAsYouGo
    } else {
        Billing::Subscription
    });
    if let Some(url) = env_val(env, "GOOGLE_GEMINI_BASE_URL")
        .filter(|u| !is_official_host(u, &["googleapis.com", "google.com"]))
    {
        a.custom_endpoint(url);
    }
    a
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
            claude_account(&settings, Some("api_key"), None),
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
        let a = claude_account(&relay, Some("none"), None);
        assert!(!a.official);
        assert_eq!(a.billing, Billing::PayAsYouGo);
        assert_eq!(a.endpoint.as_deref(), Some("https://relay.example.org"));
        let sub = claude_account(&ClaudeSettings::default(), Some("claude.ai"), Some("max"));
        assert!(sub.official);
        assert_eq!(sub.billing, Billing::Subscription);
        assert_eq!(sub.plan.as_deref(), Some("max"));
    }
}
