//! Quota exhaustion, rate limiting, and 429 recognition and time parsing (docs/06 §4.1, §4.3, P3.m7, P4.m3b, P4.m3d).
//!
//! Tokage never queries vendor endpoints directly or accesses API keys for quota checks.
//! Quota signals are passively collected from CLI JSON streams and stderr outputs during chat turns,
//! or read from local statusline/rollout cache files, or executed via official CLI subcommands (P3.m7g)
//! or slash commands executed via PTY (P4.m3d).
//! Quota regex patterns are dynamically sourced from definition packs (`usage_patterns`, P4.m3b).

use regex::Regex;
use std::{
    collections::HashMap,
    path::Path,
    sync::OnceLock,
    time::SystemTime,
};
use tokage_core::provider::ProviderId;
use crate::{
    env::ShellEnv,
    usage::{UsageDetails, UsageDetailRow, UsageRowKind, UsageSnapshot, UsageSource, UsageWindow},
};

#[derive(Debug, Clone, PartialEq)]
pub struct QuotaMatch {
    pub raw: String,
    pub reset_at: Option<String>,
    pub retrying: bool,
    pub percent: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct CompiledPattern {
    pub id: String,
    pub kind: String,
    pub window: Option<String>,
    pub source: Option<String>,
    pub regex: Regex,
}

static PATTERNS_MAP: OnceLock<HashMap<String, Vec<CompiledPattern>>> = OnceLock::new();

/// Returns precompiled usage patterns from definition pack `usage_patterns` (docs/04 §5, docs/06 §4.3, P4.m3b).
pub fn get_usage_patterns() -> &'static HashMap<String, Vec<CompiledPattern>> {
    PATTERNS_MAP.get_or_init(|| {
        let mut map: HashMap<String, Vec<CompiledPattern>> = HashMap::new();
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(crate::sandbox::BUILTIN_KNOWLEDGE) {
            if let Some(patterns_obj) = v.get("usage_patterns").and_then(|o| o.as_object()) {
                for (provider_key, list) in patterns_obj {
                    if let Some(arr) = list.as_array() {
                        let mut compiled = Vec::new();
                        for item in arr {
                            if let Some(pat_str) = item.get("pattern").and_then(|p| p.as_str()) {
                                if let Ok(re) = Regex::new(pat_str) {
                                    compiled.push(CompiledPattern {
                                        id: item.get("id").and_then(|s| s.as_str()).unwrap_or("").to_string(),
                                        kind: item.get("kind").and_then(|s| s.as_str()).unwrap_or("").to_string(),
                                        window: item.get("window").and_then(|s| s.as_str()).map(|s| s.to_string()),
                                        source: item.get("source").and_then(|s| s.as_str()).map(|s| s.to_string()),
                                        regex: re,
                                    });
                                }
                            }
                        }
                        map.insert(provider_key.clone(), compiled);
                    }
                }
            }
        }
        map
    })
}

/// Convert Unix epoch seconds to an ISO 8601 UTC string (YYYY-MM-DDTHH:MM:SSZ).
pub fn format_unix_to_utc_iso(secs: i64) -> String {
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let h = rem / 3600;
    let m = (rem % 3600) / 60;
    let s = rem % 60;

    // Howard Hinnant's algorithm for Gregorian date from days since 1970-01-01
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1024 + doe / 1461 - doe / 142401) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m_month = if mp < 10 { mp + 3 } else { mp - 9 };
    let y_final = if m_month <= 2 { y + 1 } else { y };

    format!("{y_final:04}-{m_month:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

/// Parse a reset time string to UTC ISO 8601 format (docs/06 §4.3, P3.m7b).
/// Relative durations (e.g. "in 2h 13m", "30s") and local clock times (e.g. "3pm", "15:30")
/// are converted to UTC ISO 8601. If parsing fails, the raw text is preserved.
pub fn parse_reset_time_to_utc(raw: &str) -> String {
    let trimmed = raw.trim().trim_start_matches("at ").trim_start_matches("in ").trim();
    if trimmed.is_empty() {
        return raw.to_string();
    }

    // Already in ISO 8601 UTC format (e.g. 2026-10-05T20:00:00Z)
    if trimmed.len() >= 19 && trimmed.contains('T') && (trimmed.ends_with('Z') || trimmed.contains('+') || trimmed.contains('-')) {
        return trimmed.to_string();
    }

    // Direct Unix timestamp (e.g. 1791071437)
    if trimmed.len() >= 9 && trimmed.chars().all(|c| c.is_ascii_digit()) {
        if let Ok(ts) = trimmed.parse::<i64>() {
            return format_unix_to_utc_iso(ts);
        }
    }

    let now_secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    // 1. Try relative duration: "2h 13m", "2 hours", "45s", "1d 2h", "5m"
    static REL_RE: OnceLock<Regex> = OnceLock::new();
    let rel_re = REL_RE.get_or_init(|| {
        Regex::new(r"(?i)(?:(?P<d>\d+)\s*d(?:ays?)?\s*)?(?:(?P<h>\d+)\s*h(?:ours?)?\s*)?(?:(?P<m>\d+)\s*m(?:in(?:utes?)?)?\s*)?(?:(?P<s>\d+)\s*s(?:ec(?:onds?)?)?)?").unwrap()
    });

    if let Some(caps) = rel_re.captures(trimmed) {
        let has_any = caps.name("d").is_some()
            || caps.name("h").is_some()
            || caps.name("m").is_some()
            || caps.name("s").is_some();
        if has_any {
            let d: i64 = caps.name("d").and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
            let h: i64 = caps.name("h").and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
            let m: i64 = caps.name("m").and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
            let s: i64 = caps.name("s").and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
            let total_secs = d * 86400 + h * 3600 + m * 60 + s;
            if total_secs > 0 {
                return format_unix_to_utc_iso(now_secs + total_secs);
            }
        }
    }

    // 2. Try clock time: "3pm", "3:15pm", "15:00", "3:00 am"
    static CLOCK_RE: OnceLock<Regex> = OnceLock::new();
    let clock_re = CLOCK_RE.get_or_init(|| {
        Regex::new(r"(?i)^(?P<hour>\d{1,2})(?::(?P<min>\d{2}))?(?::(?P<sec>\d{2}))?\s*(?P<ampm>am|pm)?$").unwrap()
    });

    if let Some(caps) = clock_re.captures(trimmed) {
        if let Some(h_match) = caps.name("hour") {
            if let Ok(mut hour) = h_match.as_str().parse::<i64>() {
                let minute: i64 = caps.name("min").and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
                let second: i64 = caps.name("sec").and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
                if let Some(ampm) = caps.name("ampm") {
                    let ap = ampm.as_str().to_ascii_lowercase();
                    if ap == "pm" && hour < 12 {
                        hour += 12;
                    } else if ap == "am" && hour == 12 {
                        hour = 0;
                    }
                }
                if (0..24).contains(&hour) && (0..60).contains(&minute) {
                    let tz_offset = get_local_timezone_offset_secs();
                    let local_now = now_secs + tz_offset;
                    let local_day_start = local_now - local_now.rem_euclid(86400);
                    let mut target_local = local_day_start + hour * 3600 + minute * 60 + second;
                    if target_local <= local_now {
                        target_local += 86400; // Next day
                    }
                    let target_utc = target_local - tz_offset;
                    return format_unix_to_utc_iso(target_utc);
                }
            }
        }
    }

    // Preserves raw text when unparseable
    raw.trim().to_string()
}

fn get_local_timezone_offset_secs() -> i64 {
    #[cfg(unix)]
    unsafe {
        let mut t: libc::time_t = 0;
        libc::time(&mut t);
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        tm.tm_gmtoff as i64
    }
    #[cfg(not(unix))]
    {
        0
    }
}

/// Matches lines or error payloads for quota exhaustion, rate limiting, and 429 errors (P3.m7a, P4.m3b).
/// Dynamically uses patterns from knowledge pack `usage_patterns`.
pub fn match_quota(provider: Option<ProviderId>, text: &str) -> Option<QuotaMatch> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    // 1. Check for recoverable / retrying signals
    let is_retrying = {
        let l = trimmed.to_ascii_lowercase();
        l.contains("retrying")
            || l.contains("retry in")
            || l.contains("try again in")
            || l.contains("continuing automatically")
            || l.contains("will retry")
    };

    let patterns_map = get_usage_patterns();

    // 2. Provider-specific patterns from knowledge pack
    if let Some(p) = provider {
        if let Some(plist) = patterns_map.get(p.as_str()) {
            for entry in plist {
                if let Some(caps) = entry.regex.captures(trimmed) {
                    let reset_raw = caps
                        .name("reset")
                        .or_else(|| caps.name("reset_in"))
                        .or_else(|| caps.name("auto_reset"))
                        .map(|m| m.as_str());
                    let reset_at = reset_raw.map(parse_reset_time_to_utc);
                    let percent = caps
                        .name("percent")
                        .and_then(|m| m.as_str().parse::<f64>().ok())
                        .or_else(|| {
                            if entry.kind == "limit_reached" || entry.kind == "rate_limited" || entry.kind == "budget_exhausted" {
                                Some(100.0)
                            } else {
                                None
                            }
                        });
                    return Some(QuotaMatch {
                        raw: trimmed.to_string(),
                        reset_at,
                        retrying: is_retrying || caps.name("auto_reset").is_some(),
                        percent,
                    });
                }
            }
        }
    }

    // 3. Generic patterns from knowledge pack
    if let Some(gen_list) = patterns_map.get("_generic") {
        for entry in gen_list {
            if let Some(caps) = entry.regex.captures(trimmed) {
                let reset_raw = caps.name("reset").or_else(|| caps.name("reset_in")).map(|m| m.as_str());
                let reset_at = reset_raw.map(parse_reset_time_to_utc);
                let percent = caps
                    .name("percent")
                    .and_then(|m| m.as_str().parse::<f64>().ok())
                    .or(Some(100.0));
                return Some(QuotaMatch {
                    raw: trimmed.to_string(),
                    reset_at,
                    retrying: is_retrying,
                    percent,
                });
            }
        }
    }

    // 4. Hardcoded fallbacks if pattern table was not loaded
    static HTTP_429_RE: OnceLock<Regex> = OnceLock::new();
    let http_429_re = HTTP_429_RE.get_or_init(|| {
        Regex::new(r"(?i)\b(?:429|too\s+many\s+requests|rate[_ ]limit(?:ed|_exceeded)?)\b(?:[^.\n]*?(?:retry|try\s+again|resets?)\s+(?:after|in|at)\s+(?P<reset>[^.\n]+))?").unwrap()
    });

    static INSUFFICIENT_RE: OnceLock<Regex> = OnceLock::new();
    let insufficient_re = INSUFFICIENT_RE.get_or_init(|| {
        Regex::new(r"(?i)\b(?:insufficient[_ ](?:balance|quota|credits?)|402\s+payment\s+required|余额不足|quota\s+exhausted|out\s+of\s+credits?)\b").unwrap()
    });

    if let Some(caps) = http_429_re.captures(trimmed) {
        let reset_at = caps.name("reset").map(|m| parse_reset_time_to_utc(m.as_str()));
        return Some(QuotaMatch {
            raw: trimmed.to_string(),
            reset_at,
            retrying: is_retrying,
            percent: Some(100.0),
        });
    }

    if insufficient_re.is_match(trimmed) {
        return Some(QuotaMatch {
            raw: trimmed.to_string(),
            reset_at: None,
            retrying: false,
            percent: Some(100.0),
        });
    }

    None
}

/// Never accept a trust prompt for quota queries or trust the shared temp root.
/// Use only an existing workspace Claude's own configuration already trusts.
fn claude_quota_workdir(env: &ShellEnv, requested: &Path) -> Result<std::path::PathBuf, String> {
    let config = env.vars.get("CLAUDE_CONFIG_DIR").map(std::path::PathBuf::from)
        .map(|p| p.join(".claude.json"))
        .or_else(|| env.vars.get("HOME").map(|p| Path::new(p).join(".claude.json")))
        .ok_or("Claude quota query requires HOME or CLAUDE_CONFIG_DIR")?;
    let value: serde_json::Value = std::fs::read(&config).ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .ok_or("Claude quota query requires an existing CLI-trusted workspace")?;
    let requested = requested.canonicalize().ok();
    let mut trusted = value["projects"].as_object().into_iter().flatten()
        .filter(|(_, value)| value["hasTrustDialogAccepted"].as_bool() == Some(true))
        .filter_map(|(path, _)| {
            let path = Path::new(path);
            if !path.is_absolute() || !path.is_dir() { return None; }
            path.canonicalize().ok()
        });
    let first = trusted.next().ok_or("Claude quota query requires an existing CLI-trusted workspace; open and trust a project in Claude first")?;
    if requested.as_ref() == Some(&first) { return Ok(first); }
    Ok(trusted.find(|p| Some(p) == requested.as_ref()).unwrap_or(first))
}

fn claude_management_error(frame: &str) -> Option<&'static str> {
    let lower = frame.to_ascii_lowercase();
    let rejected = ["oauth token revoked", "invalid_grant", "not logged in", "authentication_error"].iter().any(|message| lower.contains(message));
    // Expiry may be a normal refresh transition. Do not terminate the official
    // refresher unless the CLI explicitly requires user login.
    let expired_login = ["token has expired", "token expired"].iter().any(|s| lower.contains(s))
        && ["please log in", "please run /login", "please login again"].iter().any(|s| lower.contains(s));
    if rejected || expired_login {
        Some("Claude management authentication rejected")
    } else { None }
}
fn claude_input_ready(frame:&str)->bool {
    let compact=frame.split_whitespace().collect::<String>();
    compact.contains("ClaudeCode") && compact.contains('❯') && compact.contains("?forshort")
        && !["notloggedin","tokenexpired","Doyoutrustthefiles"].iter().any(|s|compact.contains(s))
}
fn claude_submit_ready(frame:&str)->bool {
    frame.lines().any(|line| line.contains("/usage") && line.split_whitespace().collect::<String>().contains("Showsessioncost,planusage,andactivitystats"))
}
fn claude_quota_frame_ready(frame: &str) -> bool {
    parse_slash_quota_output(ProviderId::ClaudeCode, frame).is_ok()
}

/// Use a workspace the user already trusted in Cursor. Never create a trust marker.
fn cursor_quota_workdir(env: &ShellEnv, requested: &Path) -> Result<std::path::PathBuf, String> {
    let root = env.vars.get("CURSOR_DATA_DIR").filter(|v| !v.trim().is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| env.vars.get("HOME").map(|h| Path::new(h).join(".cursor")))
        .ok_or("Cursor quota query requires HOME or CURSOR_DATA_DIR")?.join("projects");
    let requested = requested.canonicalize().ok();
    let mut candidates = std::fs::read_dir(root).into_iter().flatten().flatten()
        .filter_map(|entry| {
            let bytes = std::fs::read(entry.path().join(".workspace-trusted")).ok()?;
            let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
            let path = Path::new(value["workspacePath"].as_str()?);
            if !path.is_absolute() || !path.is_dir() { return None; }
            path.canonicalize().ok()
        }).collect::<Vec<_>>();
    candidates.sort();
    candidates.into_iter().min_by_key(|path| requested.as_ref() != Some(path))
        .ok_or_else(|| "Cursor quota query requires an existing CLI-trusted workspace; open and trust a workspace in agent first".into())
}

fn cursor_input_ready(frame: &str) -> bool {
    frame.contains("Cursor Agent") && frame.contains("Plan, search, build anything")
        && !frame.contains("Workspace Trust Required")
}

fn cursor_submit_ready(frame: &str) -> bool {
    frame.lines().any(|line| line.contains("/usage") && line.contains("Show plan and on-demand usage"))
}

fn cursor_quota_frame_ready(frame: &str) -> bool { parse_cursor_quota(frame).is_ok() }

/// `/usage` must be entered in the interactive UI, never as argv or a print-mode prompt.
async fn query_cursor_quota(
    exe: &Path, env: &ShellEnv, requested: &Path, proxy: Option<&crate::proxy::PreparedTurnProxy>,
) -> Result<UsageSnapshot, String> {
    let account = crate::usage::cursor_account(env, None);
    if !account.official || account.billing != crate::usage::Billing::Subscription {
        return Err("Cursor plan usage is unavailable for API key or gateway authentication".into());
    }
    let cwd = cursor_quota_workdir(env, requested)?;
    let vars = super::child_env::assemble_with_proxy(ProviderId::CursorCli, env, &[], proxy).into_iter().collect::<Vec<_>>();
    let frame = super::pty::execute_pty_interactive_after(exe, &[], &cwd, &vars, Some("/usage\r"),
        std::time::Duration::from_secs(30), super::pty::InteractiveOptions {
            input_ready: Some(cursor_input_ready), submit_ready: Some(cursor_submit_ready),
            frame_ready: Some(cursor_quota_frame_ready), ..Default::default()
        }).await?;
    parse_cursor_quota(&frame)
}

fn parse_cursor_quota(text: &str) -> Result<UsageSnapshot, String> {
    let lines: Vec<_> = text.lines().map(str::trim).collect();
    let headers: Vec<_> = lines.iter().enumerate().filter_map(|(i,line)| line.strip_prefix("Usage • ").or_else(|| line.strip_prefix("Usage · ")).map(|h|(i,h))).collect();
    if headers.len()!=1 { return Err("Cursor CLI output requires exactly one usage panel".into()); }
    let (header_index,header)=headers[0];
    let panel: Vec<_> = lines[header_index+1..].iter().take_while(|l| **l!="Esc to close").copied().collect();
    let (plan, reset_label) = if let Some((plan, date)) = header.split_once("Resets ") {
        (plan.trim(), Some(format!("Resets {}", date.trim())))
    } else { (header.trim_end_matches("Current period").trim(), None) };
    if plan.is_empty() || plan.len() > 100 { return Err("Cursor usage panel contains no plan".into()); }
    let mut rows = vec![];
    let mut included = None;
    let mut on_demand = false;
    static PERCENT: OnceLock<Regex> = OnceLock::new();
    let percent = PERCENT.get_or_init(|| Regex::new(r"^(\d+(?:\.\d+)?)%\s+used\b").unwrap());
    for (label, kind) in [("Included", UsageRowKind::Included), ("Auto", UsageRowKind::Auto),
        ("API", UsageRowKind::Api), ("On-Demand", UsageRowKind::OnDemand)] {
        let cells: Vec<_> = panel.iter().filter_map(|line| line.strip_prefix(label).filter(|rest| rest.starts_with(char::is_whitespace))).collect();
        if cells.len()>1 { return Err("Cursor usage panel contains duplicate allocation rows".into()); }
        let Some(rest) = cells.first() else { continue; };
        let rest = rest.trim();
        if let Some(caps) = percent.captures(rest) {
            let used = caps[1].parse::<f64>().ok().filter(|p| p.is_finite() && (0.0..=100.0).contains(p))
                .ok_or("Cursor usage percentage is out of range")?;
            if label == "Included" { included = Some(used); }
            if label == "On-Demand" { on_demand = true; }
            rows.push(UsageDetailRow { kind, used_percent: Some(used), value: caps[0].into() });
        } else if label == "On-Demand" {
            static CELL: OnceLock<Regex> = OnceLock::new();
            let value = CELL.get_or_init(|| Regex::new(r"\s{2,}").unwrap()).split(rest).next().unwrap_or("");
            static ON_DEMAND: OnceLock<Regex> = OnceLock::new();
            let valid=ON_DEMAND.get_or_init(|| { let amount=r"\$(?:\d+|\d{1,3}(?:,\d{3})+)(?:\.\d{1,2})?"; Regex::new(&format!(r"^(?:Disabled|{amount}(?:\s*/\s*{amount})?)$")).unwrap() });
            if value.len() > 100 || !valid.is_match(value) { return Err("Cursor on-demand usage is incomplete or unsupported".into()); }
            rows.push(UsageDetailRow { kind, used_percent: None, value: value.into() });
            on_demand = true;
        } else { return Err("Cursor usage row contains no valid percentage".into()); }
    }
    let used = included.ok_or("Cursor usage panel contains no included quota")?;
    if !on_demand { return Err("Cursor usage panel is incomplete".into()); }
    Ok(UsageSnapshot {
        windows: vec![UsageWindow { window_minutes: 0, used_percent: used, resets_at: None }],
        details: Some(UsageDetails { plan: plan.into(), reset_label, rows }),
        observed_at: Some(format_unix_to_utc_iso(SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0))),
        source: UsageSource::CliOutput,
    })
}

/// Executes an interactive slash usage command (such as `/usage`, `/status`) in a 200x50 PTY session
/// and parses the captured stable screen frame (docs/01 §7.2, docs/06 §4.1, P4.m3d).
async fn execute_slash_output(
    provider: ProviderId,
    exe: &Path,
    env: &ShellEnv,
    cwd: &Path,
    slash_cmd: &str,
    proxy: Option<&crate::proxy::PreparedTurnProxy>,
) -> Result<String, String> {
    if provider == ProviderId::CopilotCli && env.vars.get("COPILOT_PROVIDER_BASE_URL").is_some_and(|v| !v.trim().is_empty()) {
        return Err("Copilot subscription quota is unavailable in gateway mode".into());
    }
    if provider == ProviderId::CopilotCli && (slash_cmd != "/usage" || cwd.canonicalize().ok() != std::env::temp_dir().canonicalize().ok()) {
        return Err("Copilot quota query requires /usage in the system temporary directory".into());
    }
    let claude_cwd = if provider == ProviderId::ClaudeCode {
        let settings = crate::usage::claude_settings(env);
        if settings.base_url.is_some() || settings.has_token || settings.cloud.is_some() {
            return Err("Claude subscription quota is unavailable for API key or gateway authentication".into());
        }
        if slash_cmd != "/usage" { return Err("Claude quota query supports only /usage".into()); }
        Some(claude_quota_workdir(env, cwd)?)
    } else { None };
    let cwd = claude_cwd.as_deref().unwrap_or(cwd);
    let remove = if provider == ProviderId::ClaudeCode {
        super::CLAUDE_HOST_SESSION_VARS.iter().map(|s| s.to_string()).collect::<Vec<_>>()
    } else { vec![] };
    let env_vars: Vec<(String, String)> = crate::chat::child_env::assemble_with_proxy(provider, env, &remove, proxy).into_iter().collect();
    let input = if matches!(provider, ProviderId::Reasonix | ProviderId::CopilotCli | ProviderId::ClaudeCode) { format!("{slash_cmd}\r") } else { format!("{slash_cmd}\n") };
    let secret_vars = format!("--secret-env-vars={}", crate::chat::child_env::CREDENTIAL_ENV.join(","));
    let copilot_args = ["--model", "auto", "--auto-tier", "efficiency", "--disable-builtin-mcps", "--available-tools=", &secret_vars];
    let claude_args = ["--safe-mode", "--strict-mcp-config", "--mcp-config", "{\"mcpServers\":{}}", "--tools", "", "--model", "haiku"];
    let snapshot_text = crate::chat::pty::execute_pty_interactive_after(
        exe,
        if provider == ProviderId::CopilotCli { &copilot_args } else if provider == ProviderId::ClaudeCode { &claude_args } else { &[] },
        cwd,
        &env_vars,
        Some(&input),
        std::time::Duration::from_secs(if provider == ProviderId::ClaudeCode {45} else {15}),
        crate::chat::pty::InteractiveOptions {
            input_delay: if matches!(provider, ProviderId::CopilotCli | ProviderId::ClaudeCode) { std::time::Duration::from_secs(3) } else { std::time::Duration::ZERO },
            accept_folder_once: false,
            frame_ready: if provider == ProviderId::ClaudeCode { Some(claude_quota_frame_ready) } else { None },
            input_ready: if provider == ProviderId::ClaudeCode { Some(claude_input_ready) } else { None },
            submit_ready: if provider == ProviderId::ClaudeCode { Some(claude_submit_ready) } else { None },
            terminal_error: if provider == ProviderId::ClaudeCode { Some(claude_management_error) } else { None },
            ..Default::default()
        },
    )
    .await?;

    Ok(snapshot_text)
}

pub struct CliQueryOutput { pub snapshot: Option<UsageSnapshot>, pub balance: Option<String>, pub scoped: Option<crate::usage_query::contract::QueryOutput> }

#[cfg(test)]
pub async fn query_authorized(permit: &crate::online_usage::QueryPermit, provider: ProviderId, exe: &Path, env: &ShellEnv, proxy: Option<&crate::proxy::PreparedTurnProxy>) -> Result<Option<UsageSnapshot>,String> {
    Ok(query_with_permit(permit,provider,exe,env,proxy).await?.snapshot)
}

/// All application quota requests require a native consent lease. Cache publication is
/// atomic with revocation; raw CLI helpers are not exported outside this crate.
pub(crate) async fn query_with_permit(
    permit: &crate::online_usage::QueryPermit,
    provider: ProviderId,
    exe: &Path,
    env: &ShellEnv,
    proxy: Option<&crate::proxy::PreparedTurnProxy>,
) -> Result<CliQueryOutput, String> {
    permit.run(async {
        let cwd = std::env::temp_dir();
        let mut scoped = None;
        let (snapshot, balance) = match provider {
            ProviderId::CursorCli => {
                let snapshot = query_cursor_quota(exe, env, &cwd, proxy).await?;
                scoped = Some(crate::usage_query::scoped::cursor(snapshot.clone()));
                (Some(snapshot), None)
            },
            ProviderId::AntigravityCli => {
                let output = crate::usage_query::antigravity::query(permit, exe, env, proxy)
                    .await.map_err(|error| format!("Antigravity quota query failed: {:?}", error.category))?;
                let snapshot = output.display.clone();
                scoped = Some(output);
                (snapshot, None)
            },
            ProviderId::ClaudeCode | ProviderId::CodexCli | ProviderId::CopilotCli | ProviderId::Reasonix => {
                let cmd = if matches!(provider, ProviderId::CodexCli | ProviderId::Reasonix) { "/status" } else { "/usage" };
                let text = execute_slash_output(provider, exe, env, &cwd, cmd, proxy).await?;
                if matches!(provider, ProviderId::ClaudeCode | ProviderId::CopilotCli) {
                    scoped = Some(crate::usage_query::scoped::parse(provider, &text)?);
                }
                let balance = (provider == ProviderId::Reasonix).then(|| crate::usage::parse_reasonix_status_balance(&text)).flatten();
                let snapshot = match parse_slash_quota_output(provider, &text) {
                    Ok(s) => Some(s), Err(_) if balance.is_some() => None, Err(e) => return Err(e),
                };
                (snapshot, balance)
            }
            _ => return Err("Online usage query is not supported for this CLI".into()),
        };
        Ok(CliQueryOutput { snapshot, balance, scoped })
    }).await
}

#[cfg(test)]
pub async fn execute_slash_quota(provider: ProviderId, exe: &Path, env: &ShellEnv, cwd: &Path, slash_cmd: &str) -> Result<UsageSnapshot, String> {
    let text = execute_slash_output(provider, exe, env, cwd, slash_cmd, None).await?;
    parse_slash_quota_output(provider, &text)
}

/// Parses CLI slash command output captured from PTY stable frame into `UsageSnapshot`.
pub fn parse_slash_quota_output(provider: ProviderId, text: &str) -> Result<UsageSnapshot, String> {
    // PAYG wallet balance is not a five-hour subscription utilization window.
    if provider == ProviderId::Reasonix && crate::usage::parse_reasonix_status_balance(text).is_some() {
        return Ok(UsageSnapshot { windows: vec![], observed_at: None, details: None, source: UsageSource::CliOutput });
    }
    if provider == ProviderId::CopilotCli {
        static PLAN_RE: OnceLock<Regex> = OnceLock::new();
        let regex = PLAN_RE.get_or_init(|| Regex::new(r"(?im)^\s*Plan\s+[^\n]*?(?P<pct>\d{1,3}(?:\.\d+)?)%\s+used").unwrap());
        let percent = regex.captures(text).and_then(|c| c.name("pct")).and_then(|v| v.as_str().parse::<f64>().ok())
            .filter(|p| (0.0..=100.0).contains(p)).ok_or("Copilot CLI output contains no plan quota")?;
        // CLI reports a plan allocation but does not expose its period/reset here.
        return Ok(UsageSnapshot { windows: vec![UsageWindow { window_minutes: 0, used_percent: percent, resets_at: None }],
            observed_at: Some(format_unix_to_utc_iso(SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0))),
            details: None, source: UsageSource::CliOutput });
    }
    if provider == ProviderId::ClaudeCode {
        if let Some(error) = claude_management_error(text) { return Err(error.into()); }
        static PERCENT: OnceLock<Regex> = OnceLock::new();
        let percent = PERCENT.get_or_init(|| Regex::new(r"(?i)(\d{1,3}(?:\.\d+)?)%\s*used").unwrap());
        static RESET: OnceLock<Regex> = OnceLock::new();
        let reset = RESET.get_or_init(|| Regex::new(r"(?i)\bresets\s+(.+)$").unwrap());
        let lines: Vec<_> = text.lines().map(str::trim).collect();
        let header = |line: &str| {
            let lower = line.to_ascii_lowercase();
            if lower == "current session" { Some(300) }
            else if lower.starts_with("current week") { Some(7 * 24 * 60) }
            else { None }
        };
        let mut windows = vec![];
        for (index, line) in lines.iter().enumerate() {
            let Some(window_minutes) = header(line) else { continue };
            let section: Vec<_> = lines[index + 1..].iter().take_while(|line| header(line).is_none()).collect();
            let Some(used_percent) = section.iter().find_map(|line| percent.captures(line))
                .and_then(|caps| caps[1].parse::<f64>().ok()).filter(|pct| (0.0..=100.0).contains(pct)) else { continue };
            let resets_at = section.iter().find_map(|line| reset.captures(line))
                .and_then(|caps| parse_utc_iso_to_unix(&parse_reset_time_to_utc(&caps[1])).ok());
            windows.push(UsageWindow { window_minutes, used_percent, resets_at });
        }
        if windows.is_empty() { return Err("Claude CLI output contains no complete quota window".into()); }
        return Ok(UsageSnapshot { windows, observed_at: Some(format_unix_to_utc_iso(SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0))), details: None, source: UsageSource::CliOutput });
    }
    let patterns_map = get_usage_patterns();
    let mut percent = None;
    let mut resets_at = None;

    if let Some(plist) = patterns_map.get(provider.as_str()) {
        for entry in plist {
            if entry.source.as_deref() == Some("/usage") || entry.source.as_deref() == Some("/status") {
                if let Some(caps) = entry.regex.captures(text) {
                    if let Some(pct) = caps.name("percent").and_then(|m| m.as_str().parse::<f64>().ok()) {
                        percent = Some(pct);
                    }
                    if let Some(r) = caps.name("reset") {
                        let parsed = parse_reset_time_to_utc(r.as_str());
                        if let Ok(ts) = parse_utc_iso_to_unix(&parsed) {
                            resets_at = Some(ts);
                        }
                    }
                }
            }
        }
    }

    // Fallback parser if definition pack pattern didn't match
    if percent.is_none() {
        static FALLBACK_PERCENT_RE: OnceLock<Regex> = OnceLock::new();
        let fallback_pct = FALLBACK_PERCENT_RE.get_or_init(|| {
            Regex::new(r"(?i)(?P<pct>\d{1,3}(?:\.\d+)?)%\s+(?:used|utilization)").unwrap()
        });
        if let Some(caps) = fallback_pct.captures(text) {
            percent = caps.name("pct").and_then(|m| m.as_str().parse::<f64>().ok());
        }
    }

    Ok(UsageSnapshot {
        windows: vec![UsageWindow {
            window_minutes: 300,
            used_percent: percent.ok_or("CLI output contains no quota percentage")?,
            resets_at,
        }],
        observed_at: Some(format_unix_to_utc_iso(
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
        )),
        details: None, source: UsageSource::CliOutput,
    })
}

pub fn parse_antigravity_quota(text: &str) -> Result<UsageSnapshot, String> {
    crate::usage_query::antigravity::parse(text).map_err(str::to_string)?
        .display.ok_or_else(|| "Missing quota display".into())
}

/// Parses CLI subcommand usage stdout into `UsageSnapshot`.
pub fn parse_subcommand_quota_output(text: &str) -> Result<UsageSnapshot, String> {
    static USED_RE: OnceLock<Regex> = OnceLock::new();
    let used_re = USED_RE.get_or_init(|| {
        Regex::new(r"(?i)(?:used|utilization|quota):\s*(?P<pct>\d{1,3}(?:\.\d+)?)%").unwrap()
    });

    static RESET_RE: OnceLock<Regex> = OnceLock::new();
    let reset_re = RESET_RE.get_or_init(|| {
        Regex::new(r"(?i)resets?\s+(?:in|at)\s+(?P<reset>[^.\n]+)").unwrap()
    });

    let mut percent = 0.0;
    let mut resets_at = None;

    for line in text.lines() {
        if let Some(caps) = used_re.captures(line) {
            if let Some(p) = caps.name("pct").and_then(|m| m.as_str().parse().ok()) {
                percent = p;
            }
        }
        if let Some(caps) = reset_re.captures(line) {
            if let Some(r) = caps.name("reset") {
                let parsed = parse_reset_time_to_utc(r.as_str());
                if let Ok(ts) = parse_utc_iso_to_unix(&parsed) {
                    resets_at = Some(ts);
                }
            }
        }
    }

    Ok(UsageSnapshot {
        windows: vec![UsageWindow {
            window_minutes: 300,
            used_percent: percent,
            resets_at,
        }],
        observed_at: Some(format_unix_to_utc_iso(
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
        )),
        details: None, source: UsageSource::CliOutput,
    })
}

pub(crate) fn parse_utc_iso_to_unix(iso: &str) -> Result<i64, ()> {
    if iso.len() >= 20 && iso.ends_with('Z') {
        let parts: Vec<&str> = iso[..iso.len() - 1].split('T').collect();
        if parts.len() == 2 {
            let ymd: Vec<i64> = parts[0].split('-').filter_map(|s| s.parse().ok()).collect();
            let hms: Vec<i64> = parts[1].split(':').filter_map(|s| s.parse().ok()).collect();
            if ymd.len() == 3 && hms.len() == 3 {
                let y = ymd[0];
                let m = ymd[1];
                let d = ymd[2];
                let (y_calc, m_calc) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
                let era = if y_calc >= 0 { y_calc } else { y_calc - 399 } / 400;
                let yoe = (y_calc - era * 400) as u32;
                let doy = (153 * m_calc + 2) / 5 + d - 1;
                let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy as u32;
                let days = era * 146097 + doe as i64 - 719468;
                return Ok(days * 86400 + hms[0] * 3600 + hms[1] * 60 + hms[2]);
            }
        }
    }
    Err(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn claude_rejection_frames_are_errors_not_quota_or_success() {
        for text in ["OAuth token revoked. Please log in", "Error: token has expired. Please run /login", "API Error: authentication_error", "Not logged in"] {
            assert_eq!(claude_management_error(text),Some("Claude management authentication rejected"));
            let error = parse_slash_quota_output(ProviderId::ClaudeCode,text).unwrap_err();
            assert_eq!(crate::usage_query::contract::QueryFailure::cli(&error).category,crate::usage_query::contract::QueryError::Authentication);
        }
        assert!(claude_management_error("Current session\n12% used").is_none());
        assert!(claude_management_error("Connection timed out").is_none());
        assert!(claude_management_error("Access token expired. Refreshing credentials...").is_none());
    }
    #[test]
    fn claude_usage_waits_for_real_prompt_and_builtin_completion() {
        assert!(super::claude_input_ready("Claude Code v2.1.288\n❯ Try something\n? for shortcuts"));
        assert!(!super::claude_input_ready("Claude Code initializing..."));
        assert!(!super::claude_input_ready("Claude Code\n❯ /usage"));
        assert!(super::claude_submit_ready("❯ /usage\n/usage Show session cost, plan usage, and activity stats"));
        assert!(!super::claude_submit_ready("❯ /usage"));
        assert!(!super::claude_submit_ready("/usage-credits Configure usage credits"));
        assert!(!super::claude_submit_ready("/usage Unknown command"));
    }
    #[tokio::test]
    #[ignore = "installed Claude read-only management diagnostic; no model prompt; not desktop acceptance"]
    async fn installed_claude_management() {
        fn observe(frame:&str)->bool {
            if let Ok(root)=std::env::var("TOKAGE_USAGE_DIAGNOSTIC_DIR") {
                let _=std::fs::write(std::path::Path::new(&root).join("claude-management-last-frame.txt"),frame);
            }
            super::claude_quota_frame_ready(frame)
        }
        fn input(frame:&str)->bool { observe(frame); super::claude_input_ready(frame) }
        fn submit(frame:&str)->bool { observe(frame); super::claude_submit_ready(frame) }
        let env=crate::env::capture().await;
        let exe=which::which_in("claude",env.path(),std::env::temp_dir()).expect("installed Claude");
        let cwd=super::claude_quota_workdir(&env,&std::env::temp_dir()).unwrap();
        let remove=crate::chat::CLAUDE_HOST_SESSION_VARS.iter().map(|s|s.to_string()).collect::<Vec<_>>();
        let vars=crate::chat::child_env::assemble(tokage_core::ProviderId::ClaudeCode,&env,&remove).into_iter().collect::<Vec<_>>();
        let started=std::time::Instant::now();
        let frame=crate::chat::pty::execute_pty_interactive_after(&exe,&["--safe-mode","--strict-mcp-config","--mcp-config","{\"mcpServers\":{}}","--tools","","--model","haiku"],&cwd,&vars,Some("/usage\r"),std::time::Duration::from_secs(45),crate::chat::pty::InteractiveOptions{input_delay:std::time::Duration::from_secs(3),accept_folder_once:false,frame_ready:Some(observe),input_ready:Some(input),submit_ready:Some(submit),..Default::default()}).await;
        eprintln!("Claude management diagnostic elapsed_ms={} success={}",started.elapsed().as_millis(),frame.is_ok());
        let parsed=super::parse_slash_quota_output(tokage_core::ProviderId::ClaudeCode,&frame.unwrap()).unwrap();
        eprintln!("Claude management windows={}",parsed.windows.len());
    }

    use super::*;

    #[test]
    fn test_format_unix_to_utc_iso() {
        assert_eq!(format_unix_to_utc_iso(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_unix_to_utc_iso(1767225600), "2026-01-01T00:00:00Z");
    }

    #[test]
    fn test_parse_reset_time_relative() {
        let parsed = parse_reset_time_to_utc("in 2h 13m");
        assert!(parsed.ends_with('Z'));
        assert!(parsed.contains('T'));

        let parsed_s = parse_reset_time_to_utc("30s");
        assert!(parsed_s.ends_with('Z'));
    }

    #[test]
    fn test_parse_reset_time_clock() {
        let parsed = parse_reset_time_to_utc("3pm");
        assert!(parsed.ends_with('Z'));
        assert!(parsed.contains('T'));

        let parsed_raw = parse_reset_time_to_utc("unknown date next month");
        assert_eq!(parsed_raw, "unknown date next month");
    }

    #[test]
    fn test_claude_quota_matching() {
        let line = "Hit your 5-hour limit · resets 3pm";
        let m = match_quota(Some(ProviderId::ClaudeCode), line).unwrap();
        assert_eq!(m.percent, Some(100.0));
        assert!(!m.retrying);
        assert!(m.reset_at.is_some());

        let auto = "usage limit reached · continuing automatically at 3:15pm";
        let m_auto = match_quota(Some(ProviderId::ClaudeCode), auto).unwrap();
        assert!(m_auto.retrying);
    }

    #[test]
    fn test_codex_quota_matching() {
        let line = "You've hit your usage limit. Try again in 2h 15m.";
        let m = match_quota(Some(ProviderId::CodexCli), line).unwrap();
        assert_eq!(m.percent, Some(100.0));
        assert!(m.reset_at.is_some());
    }

    #[test]
    fn test_generic_429_matching() {
        let line = "Error: 429 Too Many Requests, please retry after in 10s";
        let m = match_quota(None, line).unwrap();
        assert!(m.reset_at.is_some());

        let line2 = "Insufficient balance. 余额不足";
        let m2 = match_quota(None, line2).unwrap();
        assert_eq!(m2.percent, Some(100.0));
    }

    #[test]
    fn test_new_adapters_quota_matching() {
        // Antigravity CLI
        let agy_line = "resource_exhausted: 429 Resource has been exhausted";
        let m_agy = match_quota(Some(ProviderId::AntigravityCli), agy_line).unwrap();
        assert_eq!(m_agy.percent, Some(100.0));

        // Aider
        let aider_line = "insufficient_quota: You exceeded your current quota, please check your plan";
        let m_aider = match_quota(Some(ProviderId::Aider), aider_line).unwrap();
        assert_eq!(m_aider.percent, Some(100.0));

        // OpenCode
        let oc_line = "rate_limit_exceeded: monthly allowance exceeded resets in 1h";
        let m_oc = match_quota(Some(ProviderId::Opencode), oc_line).unwrap();
        assert_eq!(m_oc.percent, Some(100.0));
        assert!(m_oc.reset_at.is_some());
    }

    #[test]
    fn copilot_plan_allocation_is_not_a_five_hour_window() {
        let snap = parse_slash_quota_output(ProviderId::CopilotCli, "AI Credits 0 (4s)\nPlan ■■■ 29% used\n 5,900 / 20,000 AIC").unwrap();
        assert_eq!(snap.windows[0].window_minutes, 0);
        assert_eq!(snap.windows[0].used_percent, 29.0);
        assert!(snap.windows[0].resets_at.is_none());
        assert!(parse_slash_quota_output(ProviderId::CopilotCli, "Session: 0 AIC used").is_err());
        assert!(parse_slash_quota_output(ProviderId::CopilotCli, "Plan 200% used").is_err());
    }

    #[test]
    fn status_without_quota_does_not_invent_zero_percent() {
        assert!(parse_slash_quota_output(ProviderId::Reasonix, "balance ¥29.50").unwrap().windows.is_empty());
        assert!(parse_slash_quota_output(ProviderId::Reasonix, "loading status").is_err());
    }

    #[test]
    fn claude_quota_preserves_session_and_week_and_rejects_loading() {
        let snapshot = parse_slash_quota_output(ProviderId::ClaudeCode,
            "Current session\n7% used\nResets 12:30pm (Asia/Shanghai)\nCurrent week (all models)\n28% used\nResets Oct 21 at 2pm (Asia/Shanghai)").unwrap();
        assert_eq!(snapshot.windows.len(), 2);
        assert_eq!(snapshot.windows[0].window_minutes, 300);
        assert_eq!(snapshot.windows[0].used_percent, 7.0);
        assert_eq!(snapshot.windows[1].window_minutes, 10080);
        assert_eq!(snapshot.windows[1].used_percent, 28.0);
        // Unsupported timezone/date strings stay unknown rather than inventing a reset.
        assert_eq!(snapshot.windows[1].resets_at, None);
        assert!(parse_slash_quota_output(ProviderId::ClaudeCode, "Loading quota...").is_err());
        let partial = parse_slash_quota_output(ProviderId::ClaudeCode,
            "Current session\nLoading...\nCurrent week (all models)\n28% used").unwrap();
        assert_eq!(partial.windows.len(), 1);
        assert_eq!(partial.windows[0].window_minutes, 10080);
        assert!(parse_slash_quota_output(ProviderId::ClaudeCode, "Current session\n101% used").is_err());
    }

    #[test]
    fn claude_quota_uses_existing_trust_without_changing_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let trusted = dir.path().join("trusted");
        let untrusted = dir.path().join("untrusted");
        std::fs::create_dir_all(&trusted).unwrap();
        std::fs::create_dir_all(&untrusted).unwrap();
        let config = dir.path().join(".claude.json");
        let text = serde_json::json!({ "projects": {
            trusted.to_str().unwrap(): { "hasTrustDialogAccepted": true },
            untrusted.to_str().unwrap(): { "hasTrustDialogAccepted": false }
        }}).to_string();
        std::fs::write(&config, &text).unwrap();
        let env = ShellEnv { vars: [("HOME".into(), dir.path().to_string_lossy().into_owned())].into() };
        assert_eq!(claude_quota_workdir(&env, &untrusted).unwrap(), trusted.canonicalize().unwrap());
        assert_eq!(std::fs::read_to_string(&config).unwrap(), text);
        std::fs::write(&config, "{\"projects\":{}}").unwrap();
        assert!(claude_quota_workdir(&env, &trusted).is_err());
    }

    #[test]
    fn test_parse_slash_quota_output_claude() {
        let frame = r#"
        Current Session
        75% used · resets 4pm
        "#;
        let snap = parse_slash_quota_output(ProviderId::ClaudeCode, frame).unwrap();
        assert_eq!(snap.windows[0].used_percent, 75.0);
        assert!(snap.windows[0].resets_at.is_some());
    }

    #[test]
    fn test_parse_slash_quota_output_codex() {
        let frame = r#"
        Codex Status:
        5h limit: 80% used (resets 5pm)
        "#;
        let snap = parse_slash_quota_output(ProviderId::CodexCli, frame).unwrap();
        assert_eq!(snap.windows[0].used_percent, 80.0);
    }

    #[test]
    fn test_parse_slash_quota_output_antigravity() {
        let frame = r#"
        antigravity-gemini-3: 65% used (resets 3pm)
        "#;
        let snap = parse_slash_quota_output(ProviderId::AntigravityCli, frame).unwrap();
        assert_eq!(snap.windows[0].used_percent, 65.0);
    }
}

#[cfg(test)]
mod antigravity_quota_tests {
    use super::*;
    #[test]
    fn structured_quota_tracks_remaining_not_used() {
        let v = serde_json::json!({"status":"SUCCESS","num_turns":0,"command":{"name":"usage","data":{"groups":[{"buckets":[
            {"window":"weekly","remaining_fraction":0.25,"reset_time":"2026-10-18T08:29:06Z"},
            {"window":"5h","remaining_fraction":1.0,"reset_time":"2026-10-13T11:41:54Z"}]}]}}});
        let snap = parse_antigravity_quota(&v.to_string()).unwrap();
        assert_eq!(snap.windows[0].used_percent, 75.0);
        assert_eq!(snap.windows[0].window_minutes, 10080);
        assert_eq!(snap.windows[1].used_percent, 0.0);
        assert!(snap.windows[0].resets_at.is_some());
        for bad in [serde_json::json!({"status":"ERROR"}),
            serde_json::json!({"status":"SUCCESS","num_turns":1,"command":{"name":"usage"}}),
            serde_json::json!({"status":"SUCCESS","num_turns":0,"command":{"name":"usage","data":{"groups":[]}}})] {
            assert!(parse_antigravity_quota(&bad.to_string()).is_err());
        }
    }
}

#[cfg(test)]
mod cursor_quota_tests {
    use super::*;
    const PANEL: &str = "Usage • Pro+                 Resets Nov 6\nMonthly plan and on-demand usage\nIncluded   33% used   ███\n  Auto   34% used   ███\n  API   8% used   █\nOn-Demand   Disabled   ———\nEsc to close\n";

    #[test]
    fn cursor_preserves_categories_without_adding_their_percentages() {
        let snap = parse_cursor_quota(PANEL).unwrap();
        assert_eq!(snap.windows.len(), 1);
        assert_eq!(snap.windows[0].used_percent, 33.0);
        assert_eq!(snap.windows[0].window_minutes, 0);
        assert_eq!(snap.windows[0].resets_at, None);
        let details = snap.details.unwrap();
        assert_eq!(details.plan, "Pro+");
        assert_eq!(details.reset_label.as_deref(), Some("Resets Nov 6"));
        assert_eq!(details.rows.iter().map(|r| r.used_percent).collect::<Vec<_>>(), [Some(33.0), Some(34.0), Some(8.0), None]);
        assert_eq!(details.rows[3].value, "Disabled");
    }

    #[test]
    fn cursor_rejects_loading_partial_and_invalid_rows_but_preserves_zero() {
        for text in ["Loading usage data...".to_string(), PANEL.replace("Included", "Missing"),
            PANEL.replace("On-Demand", "Missing"), PANEL.replace("33%", "101%"),
            PANEL.replace("34%", "-1%"), PANEL.replace("33%", "NaN%"),
            PANEL.replace("Disabled", "Loading 25%")]
        { assert!(parse_cursor_quota(&text).is_err(), "{text}"); }
        assert_eq!(parse_cursor_quota(&PANEL.replace("33%", "0%")).unwrap().windows[0].used_percent, 0.0);
        assert_eq!(parse_cursor_quota(&PANEL.replace("Disabled", "$1.25 / $20")).unwrap().details.unwrap().rows[3].value, "$1.25 / $20");
        assert!(!cursor_submit_ready("→ /usage\nUnknown command"));
        assert!(cursor_submit_ready("→ /usage    Show plan and on-demand usage"));
        assert!(!cursor_input_ready("Workspace Trust Required\nCursor Agent\nPlan, search, build anything"));
    }
    #[test]
    fn cursor_rejects_ambiguous_or_loading_panels_and_preserves_reported_precision() {
        for text in [format!("{PANEL}\n{PANEL}"),PANEL.replace("Included   33% used", "Included  0% used\nIncluded   33% used"),PANEL.replace("Disabled","Loading..."),PANEL.replace("Disabled","Unlimited"),PANEL.replace("Disabled","$1,,00 / $20")] {
            assert!(parse_cursor_quota(&text).is_err(),"{text}");
        }
        let fractional=parse_cursor_quota(&PANEL.replace("33%","33.25%")).unwrap();assert_eq!(fractional.windows[0].used_percent,33.25);
        assert_eq!(fractional.details.unwrap().rows[0].value,"33.25% used");
        assert_eq!(parse_cursor_quota(&format!("Included 99% used\n{PANEL}")).unwrap().windows[0].used_percent,33.0,"ignore unrelated text before the panel");
    }

    #[test]
    fn cursor_only_uses_existing_trust_and_respects_data_directory() {
        let home = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let workspace = home.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let env = ShellEnv { vars: [("HOME".into(), home.path().to_string_lossy().into_owned()),
            ("CURSOR_DATA_DIR".into(), data.path().to_string_lossy().into_owned())].into() };
        assert!(cursor_quota_workdir(&env, &workspace).is_err());
        let project = data.path().join("projects/opaque-path");
        std::fs::create_dir_all(&project).unwrap();
        let marker = project.join(".workspace-trusted");
        let original = serde_json::json!({"workspacePath":workspace}).to_string();
        std::fs::write(&marker, &original).unwrap();
        assert_eq!(cursor_quota_workdir(&env, &workspace).unwrap(), workspace.canonicalize().unwrap());
        assert_eq!(std::fs::read_to_string(marker).unwrap(), original);
        assert!(!home.path().join(".cursor").exists());
    }

    #[tokio::test]
    async fn cursor_key_and_gateway_modes_fail_before_spawning_a_cli() {
        for (key, value) in [("CURSOR_API_KEY", "test-only-key"), ("CURSOR_API_ENDPOINT", "https://gateway.example.invalid")] {
            let env = ShellEnv { vars: [(key.into(), value.into())].into() };
            let error = query_cursor_quota(Path::new("/must-not-run"), &env, Path::new("/"), None).await.unwrap_err();
            assert!(error.contains("API key or gateway"));
            assert!(!error.contains(value));
        }
    }
}
