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
    usage::{UsageSnapshot, UsageSource, UsageWindow},
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

/// Executes an interactive slash usage command (such as `/usage`, `/status`) in a 200x50 PTY session
/// and parses the captured stable screen frame (docs/01 §7.2, docs/06 §4.1, P4.m3d).
pub async fn execute_slash_quota(
    provider: ProviderId,
    exe: &Path,
    env: &ShellEnv,
    cwd: &Path,
    slash_cmd: &str,
) -> Result<UsageSnapshot, String> {
    let env_vars: Vec<(String, String)> = env.vars.clone().into_iter().collect();
    let input = if provider == ProviderId::Reasonix { format!("{slash_cmd}\r") } else { format!("{slash_cmd}\n") };
    let snapshot_text = crate::chat::pty::execute_pty_interactive(
        exe,
        &[],
        cwd,
        &env_vars,
        Some(&input),
        std::time::Duration::from_secs(15),
    )
    .await?;

    // Reasonix `/status` also prints a live wallet line — cache it for CLI management.
    if provider == ProviderId::Reasonix && slash_cmd.trim() == "/status" {
        if let Some(bal) = crate::usage::parse_reasonix_status_balance(&snapshot_text) {
            crate::usage::record_cli_balance(provider, bal);
        }
    }

    parse_slash_quota_output(provider, &snapshot_text)
}

/// Parses CLI slash command output captured from PTY stable frame into `UsageSnapshot`.
pub fn parse_slash_quota_output(provider: ProviderId, text: &str) -> Result<UsageSnapshot, String> {
    // PAYG wallet balance is not a five-hour subscription utilization window.
    if provider == ProviderId::Reasonix && crate::usage::parse_reasonix_status_balance(text).is_some() {
        return Ok(UsageSnapshot { windows: vec![], observed_at: None, source: UsageSource::CliOutput });
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
        source: UsageSource::CliOutput,
    })
}

/// Executes a subcommand-style quota inquiry (such as `amp usage`) as an argument array (P3.m7g).
/// Never concatenates shell commands.
pub async fn execute_subcommand_quota(
    exe: &Path,
    env: &ShellEnv,
    args: &[&str],
) -> Result<UsageSnapshot, String> {
    let mut cmd = tokio::process::Command::new(exe);
    cmd.args(args)
        .envs(&env.vars)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    crate::proc::isolate(&mut cmd);

    let out = tokio::time::timeout(std::time::Duration::from_secs(10), cmd.output())
        .await
        .map_err(|_| format!("{} {} timed out", exe.display(), args.join(" ")))?
        .map_err(|e| format!("Failed to execute {}: {e}", exe.display()))?;

    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        return Err(format!("Quota subcommand failed ({}): {stderr}", out.status));
    }

    parse_subcommand_quota_output(&stdout)
}

/// Official read-only JSON slash command: no prompt, no model quota is consumed.
pub async fn query_antigravity_quota(exe: &Path, env: &ShellEnv) -> Result<UsageSnapshot, String> {
    let (ok, out) = crate::providers::run_auth(ProviderId::AntigravityCli, exe,
        &["-p", "/usage", "--output-format", "json"], env, 20).await.ok_or("Antigravity quota probe timed out")?;
    if !ok { return Err(out); }
    parse_antigravity_quota(&out)
}

pub fn parse_antigravity_quota(text: &str) -> Result<UsageSnapshot, String> {
    let json = text.find('{').zip(text.rfind('}')).map(|(a,b)| &text[a..=b]).ok_or("Missing quota JSON")?;
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    if v["status"] != "SUCCESS" || v["command"]["name"] != "usage" || v["num_turns"].as_u64() != Some(0) {
        return Err(v["error"].as_str().unwrap_or("Not an authenticated quota response").into());
    }
    let groups = v["command"]["data"]["groups"].as_array().ok_or("Missing quota groups")?;
    let mut windows = vec![];
    for group in groups {
        for bucket in group["buckets"].as_array().into_iter().flatten() {
            let Some(remaining) = bucket["remaining_fraction"].as_f64().filter(|v| v.is_finite() && (0.0..=1.0).contains(v)) else { continue; };
            let minutes = match bucket["window"].as_str() { Some("weekly") => 10080, Some("5h") => 300, _ => continue };
            let reset = bucket["reset_time"].as_str().and_then(|s| parse_utc_iso_to_unix(s).ok());
            windows.push(UsageWindow { window_minutes: minutes, used_percent: (1.0 - remaining) * 100.0, resets_at: reset });
        }
    }
    if windows.is_empty() { return Err("No authenticated quota windows".into()); }
    Ok(UsageSnapshot { windows, observed_at: None, source: UsageSource::CliOutput })
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
        source: UsageSource::CliOutput,
    })
}

fn parse_utc_iso_to_unix(iso: &str) -> Result<i64, ()> {
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
    fn status_without_quota_does_not_invent_zero_percent() {
        assert!(parse_slash_quota_output(ProviderId::Reasonix, "balance ¥29.50").unwrap().windows.is_empty());
        assert!(parse_slash_quota_output(ProviderId::Reasonix, "loading status").is_err());
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
