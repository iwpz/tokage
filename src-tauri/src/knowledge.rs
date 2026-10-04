//! Knowledge pack download from a user-supplied URL (docs/04 §10). The pack is fetched
//! here rather than from the webview so the CSP can keep outside hosts blocked; parsing and
//! validation happen in the frontend, which renders the result for the user to review.

use std::time::Duration;

use crate::env::ShellEnv;

/// Packs are small JSON documents; anything larger is refused.
const MAX_BYTES: u64 = 5 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(30);

/// Fetches `url` (https only, or http to localhost for pack authors) and returns the body.
pub async fn fetch(env: &ShellEnv, url: &str) -> Result<String, String> {
    validate_url(url)?;
    let mut cmd = tokio::process::Command::new(if cfg!(windows) { "curl.exe" } else { "curl" });
    cmd.args([
        "--fail",
        "--silent",
        "--show-error",
        "--location",
        "--max-redirs",
        "3",
        "--proto",
        "=https,http",
        "--proto-redir",
        "=https",
        "--max-time",
        "25",
        "--max-filesize",
        &MAX_BYTES.to_string(),
        "--",
        url,
    ])
    .env_clear()
    .envs(&env.vars)
    .stdin(std::process::Stdio::null())
    .kill_on_drop(true);
    crate::proc::isolate(&mut cmd);
    let out = tokio::time::timeout(TIMEOUT, cmd.output())
        .await
        .map_err(|_| "Request timed out".to_string())?
        .map_err(|e| format!("curl not available: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() { format!("Download failed ({})", out.status) } else { err });
    }
    if out.stdout.len() as u64 > MAX_BYTES {
        return Err("Pack is larger than 5 MB".into());
    }
    String::from_utf8(out.stdout).map_err(|_| "Pack is not UTF-8 text".into())
}

fn validate_url(url: &str) -> Result<(), String> {
    if url.len() > 2048 || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("Invalid URL".into());
    }
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("https://") {
        return Ok(());
    }
    // Plain http only for a locally served pack while authoring one.
    if let Some(rest) = lower.strip_prefix("http://") {
        let host = rest.split(['/', '?', '#']).next().unwrap_or("");
        let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
        let host = if host.starts_with('[') {
            host.split(']').next().map(|h| &h[1..]).unwrap_or("")
        } else {
            host.split(':').next().unwrap_or("")
        };
        if matches!(host, "localhost" | "127.0.0.1" | "::1") {
            return Ok(());
        }
    }
    Err("Only https:// URLs are allowed (http:// for localhost only)".into())
}

#[cfg(test)]
mod tests {
    use super::validate_url;

    #[test]
    fn urls() {
        assert!(validate_url("https://example.com/pack.json").is_ok());
        assert!(validate_url("http://localhost:8080/pack.json").is_ok());
        assert!(validate_url("http://127.0.0.1/pack.json").is_ok());
        assert!(validate_url("http://[::1]:8000/p").is_ok());
        assert!(validate_url("http://example.com/pack.json").is_err());
        assert!(validate_url("http://localhost.evil.com/p").is_err());
        assert!(validate_url("http://localhost@evil.com/p").is_err());
        assert!(validate_url("file:///etc/passwd").is_err());
        assert!(validate_url("https://a.com/x y").is_err());
        assert!(validate_url("-o /tmp/x").is_err());
    }
}
