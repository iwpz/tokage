//! Knowledge pack download from a user-supplied URL (docs/04 §10). The pack is fetched
//! here rather than from the webview so the CSP can keep outside hosts blocked; parsing and
//! validation happen in the frontend, which renders the result for the user to review.

use std::time::Duration;

use crate::env::ShellEnv;

/// Packs are small JSON documents; anything larger is refused.
const MAX_BYTES: u64 = 5 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(30);

/// Fetches `url` (https only, or http to localhost for pack authors) and returns the body.
/// Uses global proxy configuration when provided (P3.m6e).
pub async fn fetch(
    env: &ShellEnv,
    url: &str,
    proxy_config: Option<&tokage_core::proxy::GlobalProxyConfig>,
) -> Result<String, String> {
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
    ]);

    if let Some(cfg) = proxy_config {
        match cfg.mode {
            tokage_core::proxy::ProxyMode::None => {
                cmd.arg("--noproxy").arg("*");
            }
            tokage_core::proxy::ProxyMode::CliOwn => {
                // Relies on login shell proxy variables in env.vars
            }
            tokage_core::proxy::ProxyMode::System => {
                if let Some(sys_up) = tokage_native::proxy::detect_system_proxy() {
                    cmd.arg("--proxy").arg(sys_up.to_url(true));
                }
                if let Some(sys_np) = tokage_native::proxy::detect_system_no_proxy() {
                    cmd.arg("--noproxy").arg(sys_np);
                } else if !cfg.no_proxy.trim().is_empty() {
                    cmd.arg("--noproxy").arg(&cfg.no_proxy);
                }
            }
            tokage_core::proxy::ProxyMode::Manual => {
                if let Some(ref up) = cfg.upstream {
                    cmd.arg("--proxy").arg(up.to_url(true));
                }
                if !cfg.no_proxy.trim().is_empty() {
                    cmd.arg("--noproxy").arg(&cfg.no_proxy);
                }
            }
            tokage_core::proxy::ProxyMode::InheritGlobal => {}
        }
        if let Some(ref ca) = cfg.ca_file {
            if !ca.trim().is_empty() {
                cmd.arg("--cacert").arg(ca);
            }
        }
    }

    cmd.args(["--", url])
        .env_clear()
        .envs(&env.vars)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    tokage_native::proc::isolate(&mut cmd);
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

    /// The shipped pack must stay valid: the app reads it offline with no fallback.
    #[test]
    fn builtin_pack() {
        let text = include_str!("../resources/knowledge/builtin.json");
        let pack: serde_json::Value = serde_json::from_str(text).expect("builtin.json is valid JSON");
        assert_eq!(pack["schema_version"], 1);
        assert_eq!(pack["pack_version"], "builtin-0.0.1");
        let domains: Vec<&str> = pack["domains"].as_array().unwrap().iter().filter_map(|d| d.as_str()).collect();
        let models = pack["models"].as_object().expect("models is keyed by model id");
        let min = pack["success_model"]["planner_min_skill"].as_f64().unwrap();
        for (id, m) in models {
            let skills = m["skills"].as_object().unwrap();
            for d in &domains {
                let v = skills.get(*d).and_then(|v| v.as_f64()).unwrap_or_else(|| panic!("{id}: missing skill {d}"));
                assert!((1.0..=5.0).contains(&v), "{id}: skills.{d} = {v}");
            }
            let (ctx, eff) = (m["context_window"].as_u64().unwrap(), m["effective_context"].as_u64().unwrap());
            assert!(eff <= ctx, "{id}: effective_context > context_window");
            let price = &m["price_per_mtok"];
            assert!(price["input"].as_f64().unwrap() >= 0.0 && price["output"].as_f64().unwrap() >= 0.0);
            // Planner role needs top planning skill (docs/04 §5).
            if m["roles"].as_array().unwrap().iter().any(|r| r == "planner") {
                assert!(skills["planning"].as_f64().unwrap() >= min, "{id}");
            }
        }
        let mut offered = std::collections::HashSet::new();
        for (provider, p) in pack["providers"].as_object().unwrap() {
            assert!(p["billing"].is_string(), "{provider}: no billing");
            let mut seen = std::collections::HashSet::new();
            for o in p["models"].as_array().map(Vec::as_slice).unwrap_or_default() {
                let id = o["model"].as_str().unwrap();
                assert!(models.contains_key(id), "{provider}: unknown model {id}");
                assert!(seen.insert(id), "duplicate {provider}:{id}");
                assert!(o["cli_model_arg"].as_str().is_some_and(|a| !a.is_empty()), "{provider}:{id}: no cli_model_arg");
                // A CLI can only narrow the model's context.
                if let Some(ctx) = o["context_window"].as_u64() {
                    assert!(ctx <= models[id]["context_window"].as_u64().unwrap(), "{provider}:{id}");
                    assert!(o["effective_context"].as_u64().unwrap() <= ctx, "{provider}:{id}");
                }
                offered.insert(id);
            }
        }
        for id in models.keys() {
            assert!(offered.contains(id.as_str()), "{id} is not offered by any CLI");
        }
    }

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
