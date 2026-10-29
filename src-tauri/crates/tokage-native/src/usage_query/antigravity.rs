//! Structured zero-turn management response; groups never become account-wide quota.
use super::contract::{QueryError, QueryFailure, QueryOutput, QuerySource, QuotaWindow};
use crate::{
    env::ShellEnv,
    online_usage::QueryPermit,
    proxy::PreparedTurnProxy,
    usage::{UsageSnapshot, UsageSource, UsageWindow},
};
use serde_json::Value;
use std::{
    path::Path,
    process::{ExitStatus, Stdio},
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncReadExt};

const MAX_STDOUT: usize = 256 * 1024;
const MAX_STDERR: usize = 64 * 1024;

fn failure(category: QueryError) -> QueryFailure {
    QueryFailure {
        category,
        retry_after: None,
    }
}

async fn bounded_output(
    stream: impl AsyncRead + Unpin,
    limit: usize,
) -> Result<Vec<u8>, QueryFailure> {
    let mut bytes = Vec::new();
    stream
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| failure(QueryError::Network))?;
    if bytes.len() > limit {
        return Err(failure(QueryError::InvalidData));
    }
    Ok(bytes)
}

/// Both pipes are drained concurrently. Dropping any failure/cancel/timeout path reaps the group.
async fn execute_command(
    command: &mut tokio::process::Command,
    permit: &QueryPermit,
    timeout: Duration,
) -> Result<(ExitStatus, Vec<u8>, Vec<u8>), QueryFailure> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    crate::proc::isolate(command);
    permit
        .run(async {
            Ok(tokio::time::timeout(timeout, async {
                let mut child = permit
                    .publish(|| command.spawn())
                    .map_err(|_| failure(QueryError::Cancelled))?
                    .map_err(|_| failure(QueryError::Network))?;
                let _group = crate::proc::Group::new(&child);
                let stdout = child
                    .stdout
                    .take()
                    .ok_or_else(|| failure(QueryError::Network))?;
                let stderr = child
                    .stderr
                    .take()
                    .ok_or_else(|| failure(QueryError::Network))?;
                tokio::try_join!(
                    async { child.wait().await.map_err(|_| failure(QueryError::Network)) },
                    bounded_output(stdout, MAX_STDOUT),
                    bounded_output(stderr, MAX_STDERR),
                )
            })
            .await
            .unwrap_or_else(|_| Err(failure(QueryError::Timeout))))
        })
        .await
        .map_err(|_| failure(QueryError::Cancelled))?
}

pub(crate) async fn query(
    permit: &QueryPermit,
    exe: &Path,
    env: &ShellEnv,
    proxy: Option<&PreparedTurnProxy>,
) -> Result<QueryOutput, QueryFailure> {
    let cwd = tempfile::tempdir().map_err(|_| failure(QueryError::Persistence))?;
    let args = ["-p", "/usage", "--output-format", "json"].map(String::from);
    let mut command = crate::sandbox::create_usage_command(
        tokage_core::ProviderId::AntigravityCli,
        exe,
        &args,
        &cwd.path().to_string_lossy(),
        env.vars.get("HOME").map(String::as_str),
    )
    .map_err(|_| failure(QueryError::Persistence))?;
    command.current_dir(cwd.path());
    crate::chat::apply_env(
        &mut command,
        tokage_core::ProviderId::AntigravityCli,
        env,
        &[],
        proxy,
    );
    let (status, stdout, stderr) =
        execute_command(&mut command, permit, Duration::from_secs(20)).await?;
    decode(status.success(), &stdout, &stderr, super::now())
}

fn decode(
    success: bool,
    stdout: &[u8],
    stderr: &[u8],
    time: i64,
) -> Result<QueryOutput, QueryFailure> {
    let value = serde_json::from_slice::<Value>(stdout).ok();
    if !success || value.as_ref().is_some_and(|v| v["status"] == "ERROR") {
        return Err(command_failure(value.as_ref(), stdout, stderr, time));
    }
    let text = std::str::from_utf8(stdout).map_err(|_| failure(QueryError::InvalidData))?;
    parse(text).map_err(|_| failure(QueryError::InvalidData))
}

/// Inspect only bounded error content, and return categories rather than vendor text or credentials.
fn command_failure(value: Option<&Value>, stdout: &[u8], stderr: &[u8], time: i64) -> QueryFailure {
    use regex::Regex;
    use std::sync::OnceLock;
    let error = value.and_then(|v| v.get("error"));
    let code = error
        .and_then(|v| v.get("code").or_else(|| v.get("statusCode")))
        .and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        });
    let message = error
        .and_then(|e| e.as_str().or_else(|| e["message"].as_str()))
        .unwrap_or("");
    let status = error
        .and_then(|e| e["status"].as_str().or_else(|| e["code"].as_str()))
        .unwrap_or("");
    let stdout_text = if value.is_none() {
        String::from_utf8_lossy(stdout)
    } else {
        "".into()
    };
    let stderr_text = String::from_utf8_lossy(stderr);
    let text = format!("{status}\n{message}\n{stdout_text}\n{stderr_text}");
    static AUTH: OnceLock<Regex> = OnceLock::new();
    static RATE: OnceLock<Regex> = OnceLock::new();
    static TIMEOUT: OnceLock<Regex> = OnceLock::new();
    let auth = AUTH.get_or_init(|| Regex::new(r"(?i)\b(?:401|403|unauthorized|unauthenticated|authentication_error|invalid_grant)\b|not logged in|authentication (?:failed|rejected|required)|token (?:has )?(?:expired|revoked)|please (?:run /login|log in|login)").unwrap());
    let rate = RATE.get_or_init(|| {
        Regex::new(r"(?i)\b(?:429|resource_exhausted|too many requests|rate[ _-]*limit(?:ed)?)\b")
            .unwrap()
    });
    let timeout =
        TIMEOUT.get_or_init(|| Regex::new(r"(?i)\b(?:timed out|timeout|etimedout)\b").unwrap());
    let category = if matches!(code, Some(401 | 403)) {
        QueryError::Authentication
    } else if code == Some(429) {
        QueryError::RateLimited
    } else if auth.is_match(&text) {
        QueryError::Authentication
    } else if rate.is_match(&text) {
        QueryError::RateLimited
    } else if timeout.is_match(&text) {
        QueryError::Timeout
    } else {
        QueryError::Network
    };
    if category != QueryError::RateLimited {
        return failure(category);
    }
    let retry = error
        .and_then(|e| e.get("retry_after").or_else(|| e.get("retryAfter")))
        .or_else(|| value.and_then(|v| v.get("retry_after").or_else(|| v.get("retryAfter"))))
        .and_then(|v| {
            v.as_str()
                .map(str::to_owned)
                .or_else(|| v.as_u64().map(|n| n.to_string()))
        })
        .or_else(|| {
            stderr_text.lines().find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.trim()
                    .eq_ignore_ascii_case("retry-after")
                    .then(|| value.trim().to_owned())
            })
        });
    super::http::response_status(429, retry.as_deref(), time)
        .err()
        .unwrap()
}

pub fn parse(text: &str) -> Result<QueryOutput, &'static str> {
    let v: Value = serde_json::from_str(text).map_err(|_| "Invalid quota JSON")?;
    if v["status"] != "SUCCESS"
        || v["command"]["name"] != "usage"
        || v["num_turns"].as_u64() != Some(0)
    {
        return Err("Not a successful zero-turn usage response");
    }
    let groups = v["command"]["data"]["groups"]
        .as_array()
        .ok_or("Missing quota groups")?;
    let mut windows = vec![];
    let mut ids = std::collections::HashSet::new();
    for (group_index, group) in groups.iter().enumerate() {
        let name = group["name"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 200);
        for (index, bucket) in group["buckets"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            let Some(remaining) = bucket["remaining_fraction"]
                .as_f64()
                .filter(|v| v.is_finite() && (0.0..=1.0).contains(v))
            else {
                continue;
            };
            let Some(period) = bucket["window"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 100)
            else {
                continue;
            };
            let bucket_id = bucket["id"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 200)
                .map(str::to_string)
                .unwrap_or_else(|| format!("position:{index}"));
            // JSON tuple avoids separator collisions in names and identifiers.
            let group_id = name
                .map(str::to_string)
                .unwrap_or_else(|| format!("position:{group_index}"));
            let id = serde_json::json!([group_id, bucket_id, period]).to_string();
            if !ids.insert(id.clone()) {
                return Err("Invalid duplicate quota bucket");
            }
            windows.push(QuotaWindow {
                id,
                scope: name
                    .map(|n| format!("group:{n}"))
                    .unwrap_or_else(|| "unclassified".into()),
                period: period.into(),
                window_minutes: match period {
                    "5h" => Some(300),
                    "weekly" => Some(10080),
                    _ => None,
                },
                used_percent: Some((1.0 - remaining) * 100.0),
                resets_at: bucket["reset_time"].as_str().and_then(|s| reset_time(s)),
            });
        }
    }
    if windows.is_empty() {
        return Err("Missing valid quota windows");
    }
    let display = UsageSnapshot {
        windows: windows
            .iter()
            .map(|w| UsageWindow {
                window_minutes: w.window_minutes.unwrap_or(0),
                used_percent: w.used_percent.unwrap(),
                resets_at: w.resets_at,
            })
            .collect(),
        observed_at: None,
        details: None,
        source: UsageSource::CliOutput,
    };
    Ok(QueryOutput {
        available: None,
        source: QuerySource::Cli,
        windows,
        quantities: vec![],
        display: Some(display),
    })
}

fn reset_time(s: &str) -> Option<i64> {
    if s.len() != 20
        || !s.is_ascii()
        || &s[4..5] != "-"
        || &s[7..8] != "-"
        || &s[10..11] != "T"
        || &s[13..14] != ":"
        || &s[16..17] != ":"
        || &s[19..] != "Z"
    {
        return None;
    }
    for (start, end, min, max) in [
        (0, 4, 0, 9999),
        (5, 7, 1, 12),
        (8, 10, 1, 31),
        (11, 13, 0, 23),
        (14, 16, 0, 59),
        (17, 19, 0, 59),
    ] {
        let n = s[start..end].parse::<u32>().ok()?;
        if n < min || n > max {
            return None;
        }
    }
    let timestamp = crate::chat::quota::parse_utc_iso_to_unix(s).ok()?;
    (crate::chat::quota::format_unix_to_utc_iso(timestamp) == s).then_some(timestamp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn response(groups: Value) -> String {
        json!({"status":"SUCCESS","num_turns":0,"command":{"name":"usage","data":{"groups":groups}}}).to_string()
    }
    #[test]
    fn command_errors_preserve_categories_retry_after_and_never_vendor_text() {
        for (stdout, stderr, expected) in [
            (
                json!({"status":"ERROR","error":{"code":401,"message":"synthetic-private"}})
                    .to_string(),
                "",
                QueryError::Authentication,
            ),
            (
                json!({"status":"ERROR","error":{"code":"403"}}).to_string(),
                "",
                QueryError::Authentication,
            ),
            (
                json!({"status":"ERROR","error":{"status":"UNAUTHENTICATED"}}).to_string(),
                "",
                QueryError::Authentication,
            ),
            (
                json!({"status":"ERROR","error":{"status":"RESOURCE_EXHAUSTED","retry_after":120}})
                    .to_string(),
                "",
                QueryError::RateLimited,
            ),
            (
                "".into(),
                "HTTP 429 Too Many Requests\nRetry-After: 120\nsynthetic-private",
                QueryError::RateLimited,
            ),
            (
                "".into(),
                "OAuth token has expired. Please run /login",
                QueryError::Authentication,
            ),
            ("".into(), "request timed out", QueryError::Timeout),
            (
                "".into(),
                "network unavailable; request id 14010",
                QueryError::Network,
            ),
        ] {
            for success in [false, true] {
                // A zero exit status does not make a structured ERROR envelope successful.
                if success && stdout.is_empty() {
                    continue;
                }
                let error = decode(success, stdout.as_bytes(), stderr.as_bytes(), 1000)
                    .err()
                    .unwrap();
                assert_eq!(error.category, expected);
                if expected == QueryError::RateLimited {
                    assert_eq!(error.retry_after, Some(1120));
                }
                assert!(!format!("{error:?}").contains("synthetic-private"));
            }
        }
        let error = decode(
            false,
            br#"{"error":{"code":429,"retryAfter":"not-a-date"}}"#,
            b"",
            1000,
        )
        .err()
        .unwrap();
        assert_eq!(error.category, QueryError::RateLimited);
        assert!(error.retry_after.is_none());
        let error = decode(false, br#"{"error":{"code":429,"message":"previous request was 401","retry_after":60}}"#, b"", 1000).err().unwrap();
        assert_eq!(error.category, QueryError::RateLimited);
        assert_eq!(error.retry_after, Some(1060));
    }
    #[test]
    fn successful_exit_still_requires_valid_zero_turn_usage() {
        let raw = response(
            json!([{"name":"group","buckets":[{"id":"one","window":"5h","remaining_fraction":0.5}]}]),
        );
        assert_eq!(
            decode(true, raw.as_bytes(), b"warning", 1000)
                .unwrap()
                .windows
                .len(),
            1
        );
        assert!(decode(false, raw.as_bytes(), b"process failed", 1000).is_err());
        for raw in [
            b"not json".as_slice(),
            br#"{"status":"SUCCESS","num_turns":1,"command":{"name":"usage"}}"#,
        ] {
            assert_eq!(
                decode(true, raw, b"", 1000).err().unwrap().category,
                QueryError::InvalidData
            );
        }
    }
    #[cfg(unix)]
    fn shell(script: &str, marker: &Path) -> tokio::process::Command {
        let mut command = tokio::process::Command::new("/bin/sh");
        command.args(["-c", script, "agy-management"]).arg(marker);
        command.env_clear().env("PATH", "/usr/bin:/bin");
        command
    }
    #[cfg(unix)]
    async fn assert_reaped(marker: &Path) {
        let pid = std::fs::read_to_string(marker)
            .unwrap()
            .parse::<i32>()
            .unwrap();
        for _ in 0..100 {
            if unsafe { libc::kill(pid, 0) } != 0 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("management child survived cleanup");
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn either_output_stream_is_bounded_and_overflow_reaps_the_process() {
        let dir = tempfile::tempdir().unwrap();
        let permission =
            crate::online_usage::OnlineUsagePermission::open(&dir.path().join("consent"));
        permission.set(true).unwrap();
        for (name, script) in [
            (
                "stdout",
                r#"printf '%s' "$$" > "$1"; while :; do printf 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'; done"#,
            ),
            (
                "stderr",
                r#"printf '%s' "$$" > "$1"; while :; do printf 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx' >&2; done"#,
            ),
        ] {
            let marker = dir.path().join(name);
            let error = execute_command(
                &mut shell(script, &marker),
                &permission.permit().unwrap(),
                Duration::from_secs(5),
            )
            .await
            .err()
            .unwrap();
            assert_eq!(error.category, QueryError::InvalidData);
            assert_reaped(&marker).await;
        }
        assert_eq!(
            bounded_output(vec![b'x'; MAX_STDOUT].as_slice(), MAX_STDOUT)
                .await
                .unwrap()
                .len(),
            MAX_STDOUT
        );
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn timeout_revocation_and_disabled_start_cleanup_without_late_output() {
        let dir = tempfile::tempdir().unwrap();
        let permission =
            crate::online_usage::OnlineUsagePermission::open(&dir.path().join("consent"));
        permission.set(true).unwrap();
        let script = r#"printf '%s' "$$" > "$1"; exec sleep 30"#;
        let marker = dir.path().join("timeout");
        let error = execute_command(
            &mut shell(script, &marker),
            &permission.permit().unwrap(),
            Duration::from_millis(150),
        )
        .await
        .err()
        .unwrap();
        assert_eq!(error.category, QueryError::Timeout);
        assert_reaped(&marker).await;

        let marker = dir.path().join("cancelled");
        let mut command = shell(script, &marker);
        let permit = permission.permit().unwrap();
        let work = execute_command(&mut command, &permit, Duration::from_secs(5));
        tokio::pin!(work);
        tokio::time::timeout(Duration::from_secs(2), async {
            while !marker.exists() {
                tokio::select! {
                    result = &mut work => panic!("query ended before revoke: {:?}", result.err().map(|e| e.category)),
                    _ = tokio::time::sleep(Duration::from_millis(10)) => {}
                }
            }
        }).await.unwrap();
        permission.set(false).unwrap();
        assert_eq!(work.await.err().unwrap().category, QueryError::Cancelled);
        assert_reaped(&marker).await;

        let marker = dir.path().join("must-not-start");
        let error = execute_command(&mut shell(script, &marker), &permit, Duration::from_secs(1))
            .await
            .err()
            .unwrap();
        assert_eq!(error.category, QueryError::Cancelled);
        assert!(!marker.exists());
    }
    #[test]
    fn keeps_same_period_groups_and_unknown_period_without_invention() {
        let out = parse(&response(json!([
            {"name":"model-a","buckets":[{"id":"one","window":"5h","remaining_fraction":1.0}]},
            {"name":"model-b","buckets":[{"id":"one","window":"5h","remaining_fraction":0.0},{"id":"one","window":"future","remaining_fraction":0.25}]}
        ]))).unwrap();
        assert_eq!(out.windows.len(), 3);
        assert_ne!(out.windows[0].id, out.windows[1].id);
        assert_eq!(out.windows[0].used_percent, Some(0.0));
        assert_eq!(out.windows[1].used_percent, Some(100.0));
        assert_eq!(out.windows[2].used_percent, Some(75.0));
        assert!(out.windows[2].window_minutes.is_none());
        assert_eq!(out.windows[2].period, "future");
        assert!(out
            .windows
            .iter()
            .all(|w| w.scope != "account" && w.resets_at.is_none()));
    }
    #[test]
    fn reset_dates_are_real_calendar_dates_or_unknown() {
        assert!(reset_time("2026-10-24T00:00:00Z").is_some());
        for raw in [
            "2026-99-24T00:00:00Z",
            "2026-02-30T00:00:00Z",
            "2026-10-24T24:00:00Z",
            "unknown",
        ] {
            assert!(reset_time(raw).is_none());
        }
    }
    #[test]
    fn invalid_and_missing_buckets_do_not_erase_valid_sibling() {
        let out = parse(&response(json!([{"buckets":[{"window":"weekly","remaining_fraction":1.1},{"window":"weekly"},{"window":"weekly","remaining_fraction":0.5}]}]))).unwrap();
        assert_eq!(out.windows.len(), 1);
        assert_eq!(out.windows[0].scope, "unclassified");
    }
    #[test]
    fn rejects_model_turn_error_and_duplicate_buckets() {
        for bad in [
            json!({"status":"ERROR","error":"private credential message"}),
            json!({"status":"SUCCESS","num_turns":1,"command":{"name":"usage"}}),
            json!({"status":"SUCCESS","num_turns":0,"command":{"name":"chat"}}),
        ] {
            let err = parse(&bad.to_string()).err().unwrap();
            assert!(!err.contains("private"));
        }
        assert!(parse(&response(json!([{"name":"a","buckets":[{"id":"x","window":"5h","remaining_fraction":1.0},{"id":"x","window":"5h","remaining_fraction":0.0}]}]))).is_err());
    }
}
