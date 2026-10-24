//! Fixed-resource HTTPS transport. No caller-controlled URL, redirects, cookie jar or curlrc.
use super::contract::QueryError;
use crate::{env::ShellEnv, online_usage::QueryPermit, proxy::PreparedTurnProxy};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub enum UsageHttpTarget {
    DeepSeekBalance,
}
impl UsageHttpTarget {
    fn url(&self) -> &'static str {
        match self {
            Self::DeepSeekBalance => "https://api.deepseek.com/user/balance",
        }
    }
}
/// Never Debug/Serialize. The adapter must supply the current matching official credential.
pub struct UsageCredential(String);
impl UsageCredential {
    pub fn bearer(value: String) -> Result<Self, HttpFailure> {
        if value.is_empty() || value.bytes().any(|b| b.is_ascii_control()) {
            return Err(HttpFailure {
                category: QueryError::Authentication,
                retry_after: None,
            });
        }
        Ok(Self(value))
    }
}
pub type HttpFailure = super::contract::QueryFailure;
fn failure(category: QueryError) -> HttpFailure {
    HttpFailure {
        category,
        retry_after: None,
    }
}
fn config(token: &UsageCredential) -> String {
    format!(
        "header = \"Authorization: Bearer {}\"\n",
        token.0.replace('\\', "\\\\").replace('"', "\\\"")
    )
}
fn retry_at(value: &str, time: i64) -> Option<i64> {
    if let Ok(seconds) = value.trim().parse::<u64>() {
        return Some(time.saturating_add(i64::try_from(seconds).ok()?));
    }
    let parts: Vec<_> = value.split_whitespace().collect();
    if parts.len() != 6 || parts[5] != "GMT" {
        return None;
    }
    let month = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .iter()
    .position(|m| *m == parts[2])?
        + 1;
    let year = parts[3].parse::<i64>().ok()?;
    let day = parts[1].parse::<u32>().ok()?;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let max_day = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ][month - 1];
    let clock: Vec<u32> = parts[4]
        .split(':')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    if !(1970..=9999).contains(&year)
        || day == 0
        || day > max_day
        || clock.len() != 3
        || clock[0] > 23
        || clock[1] > 59
        || clock[2] > 59
    {
        return None;
    }
    let iso = format!(
        "{}-{:02}-{:02}T{}Z",
        parts[3],
        month,
        parts[1].parse::<u32>().ok()?,
        parts[4]
    );
    crate::chat::quota::parse_utc_iso_to_unix(&iso)
        .ok()
        .map(|t| t.max(time))
}
pub fn response_status(code: u16, retry_after: Option<&str>, time: i64) -> Result<(), HttpFailure> {
    match code {
        200 => Ok(()),
        401 | 403 => Err(failure(QueryError::Authentication)),
        429 => Err(HttpFailure {
            category: QueryError::RateLimited,
            retry_after: retry_after.and_then(|s| retry_at(s, time)),
        }),
        300..=399 => Err(failure(QueryError::Redirect)),
        _ => Err(failure(QueryError::Network)),
    }
}
pub async fn get(
    permit: &QueryPermit,
    target: UsageHttpTarget,
    credential: UsageCredential,
    env: &ShellEnv,
    proxy: &PreparedTurnProxy,
) -> Result<serde_json::Value, HttpFailure> {
    let result = permit.run(async {
        let mut cmd = tokio::process::Command::new(if cfg!(windows) { "curl.exe" } else { "curl" });
        cmd.args([
            "--disable",
            "--silent",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-redirs",
            "0",
            "--connect-timeout",
            "5",
            "--max-time",
            "20",
            "--max-filesize",
            "65536",
            "--config",
            "-",
            "--write-out",
            "\n%{http_code}",
            "--include",
            "--suppress-connect-headers",
        ])
        .arg("--url")
        .arg(target.url())
        .env_clear();
        let vars = crate::chat::child_env::assemble_with_proxy(
            tokage_core::ProviderId::Opencode,
            env,
            &[],
            Some(proxy),
        );
        // curl needs networking/CA environment, not model credentials. Authorization is stdin-only.
        for key in [
            "PATH",
            "HOME",
            "HTTPS_PROXY",
            "HTTP_PROXY",
            "ALL_PROXY",
            "NO_PROXY",
            "https_proxy",
            "http_proxy",
            "all_proxy",
            "no_proxy",
            "SSL_CERT_FILE",
            "SSL_CERT_DIR",
        ] {
            if let Some(value) = vars.get(key) {
                cmd.env(key, value);
            }
        }
        if let Some(ca) = &proxy.ca_file {
            cmd.arg("--cacert").arg(ca);
        }
        cmd.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        crate::proc::isolate(&mut cmd);
        let mut child = cmd.spawn().map_err(|_| "HTTP launch failed".to_string())?;
        let _group = crate::proc::Group::new(&child);
        let mut stdin = child.stdin.take().ok_or("HTTP input unavailable")?;
        stdin
            .write_all(config(&credential).as_bytes())
            .await
            .map_err(|_| "HTTP input failed")?;
        drop(stdin);
        let mut body = Vec::new();
        child
            .stdout
            .take()
            .ok_or("HTTP output unavailable")?
            .take(65542)
            .read_to_end(&mut body)
            .await
            .map_err(|_| "HTTP read failed")?;
        if body.len() > 65541 {
            return Err("HTTP response too large".into());
        }
        let exit = child.wait().await.map_err(|_| "HTTP wait failed")?;
        if !exit.success() {
            return Err("HTTP transport failed".into());
        }
        let text = String::from_utf8(body).map_err(|_| "HTTP output invalid")?;
        let (body, code) = text.rsplit_once('\n').ok_or("HTTP status missing")?;
        let code = code.parse::<u16>().map_err(|_| "HTTP status invalid")?;
        let mut content = body;
        let mut retry = None;
        while content.starts_with("HTTP/") {
            let (headers, rest) = content
                .split_once("\r\n\r\n")
                .ok_or("HTTP headers invalid")?;
            retry = headers.lines().find_map(|line| {
                line.split_once(':')
                    .filter(|(k, _)| k.eq_ignore_ascii_case("retry-after"))
                    .map(|(_, v)| v.trim())
            });
            content = rest;
        }
        Ok((
            content.to_string(),
            response_status(code, retry, super::now()),
        ))
    });
    let (body, status) = tokio::time::timeout(std::time::Duration::from_secs(20), result)
        .await
        .map_err(|_| failure(QueryError::Timeout))?
        .map_err(|_| {
            failure(if permit.publish(|| ()).is_err() {
                QueryError::Cancelled
            } else {
                QueryError::Network
            })
        })?;
    status?;
    serde_json::from_str(&body).map_err(|_| failure(QueryError::InvalidData))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn auth_redirect_and_retry_contract() {
        assert_eq!(
            response_status(401, None, 10).unwrap_err().category,
            QueryError::Authentication
        );
        assert_eq!(
            response_status(403, None, 10).unwrap_err().category,
            QueryError::Authentication
        );
        assert_eq!(
            response_status(302, None, 10).unwrap_err().category,
            QueryError::Redirect
        );
        let e = response_status(429, Some("120"), 10).unwrap_err();
        assert_eq!(e.retry_after, Some(130));
        assert_eq!(
            response_status(429, Some("Thu, 01 Jan 1970 00:02:00 GMT"), 10)
                .unwrap_err()
                .retry_after,
            Some(120)
        );
        assert!(UsageCredential::bearer("secret\r\nX-Evil: true".into()).is_err());
        let c = UsageCredential::bearer("s\"ecret".into()).unwrap();
        assert_eq!(
            config(&c),
            "header = \"Authorization: Bearer s\\\"ecret\"\n"
        );
        assert_eq!(
            UsageHttpTarget::DeepSeekBalance.url(),
            "https://api.deepseek.com/user/balance"
        );
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn transport_uses_fixed_https_target_and_stdin_auth_without_redirects_or_credential_env()
    {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("curl");
        let root = serde_json::to_string(&dir.path().to_str().unwrap()).unwrap();
        let script = format!(
            r#"#!/usr/bin/python3
import json,os,sys
from pathlib import Path
root=Path({root})
(root/'argv').write_text(json.dumps(sys.argv[1:]))
(root/'env').write_text(json.dumps(dict(os.environ)))
(root/'input').write_text(sys.stdin.read())
code=int((root/'code').read_text())
sys.stdout.write('HTTP/1.1 '+str(code)+' Response\r\nRetry-After: 120\r\n\r\n'+json.dumps({{'is_available': True}})+'\n'+str(code))
"#
        );
        std::fs::write(&exe, script).unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let permission =
            crate::online_usage::OnlineUsagePermission::open(&dir.path().join("consent"));
        permission.set(true).unwrap();
        let env = ShellEnv {
            vars: [
                (
                    "PATH".into(),
                    format!("{}:/usr/bin:/bin", dir.path().display()),
                ),
                ("HOME".into(), dir.path().display().to_string()),
                ("DEEPSEEK_API_KEY".into(), "not-forwarded".into()),
            ]
            .into(),
        };
        let proxy = PreparedTurnProxy {
            mode: tokage_core::proxy::ProxyMode::None,
            relay_handle: None,
            proxy_url: None,
            no_proxy: None,
            ca_file: Some("/trusted/ca".into()),
            extra_env: Default::default(),
            forced: false,
        };
        for (code, expected) in [
            (200, None),
            (302, Some(QueryError::Redirect)),
            (401, Some(QueryError::Authentication)),
            (403, Some(QueryError::Authentication)),
            (429, Some(QueryError::RateLimited)),
        ] {
            std::fs::write(dir.path().join("code"), code.to_string()).unwrap();
            let response = get(
                &permission.permit().unwrap(),
                UsageHttpTarget::DeepSeekBalance,
                UsageCredential::bearer("synthetic-secret".into()).unwrap(),
                &env,
                &proxy,
            )
            .await;
            if let Some(expected) = expected {
                let error = response.unwrap_err();
                assert_eq!(error.category, expected);
                if expected == QueryError::RateLimited {
                    assert!(error.retry_after.is_some());
                }
            } else {
                assert_eq!(response.unwrap()["is_available"], true);
            }
        }
        let argv: Vec<String> =
            serde_json::from_str(&std::fs::read_to_string(dir.path().join("argv")).unwrap())
                .unwrap();
        assert_eq!(argv[0], "--disable");
        assert!(argv
            .iter()
            .any(|s| s == "https://api.deepseek.com/user/balance"));
        for forbidden in ["--location", "-L", "--insecure", "synthetic-secret"] {
            assert!(!argv.iter().any(|s| s.contains(forbidden)));
        }
        assert!(std::fs::read_to_string(dir.path().join("input"))
            .unwrap()
            .contains("Authorization: Bearer synthetic-secret"));
        assert!(!std::fs::read_to_string(dir.path().join("env"))
            .unwrap()
            .contains("DEEPSEEK_API_KEY"));
    }
}
