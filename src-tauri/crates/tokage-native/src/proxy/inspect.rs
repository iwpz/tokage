//! Provider proxy capability and configuration inspection (docs/12 §2, P3.m6a).
//!
//! Inspects each CLI's documented proxy environment variables, extra runtime flags
//! (e.g. Node 24+ fetch `NODE_USE_ENV_PROXY=1`), SOCKS support, and local config files
//! (e.g. `~/.claude/settings.json`, `~/.codex/config.toml`).
//!
//! Per R11 and docs/12 §5: If a CLI's own configuration defines a proxy with credentials,
//! only the host/port are displayed in the status; secret tokens and passwords are never
//! read or displayed.

use std::collections::HashMap;
use std::path::PathBuf;

use tokage_core::proxy::ProviderProxyStatus;
use tokage_core::ProviderId;

use crate::env::ShellEnv;

/// Inspects a provider's proxy capabilities and any active proxy settings in its own config files.
pub fn inspect_provider_proxy(id: ProviderId, env: &ShellEnv) -> ProviderProxyStatus {
    let own_config_proxy = inspect_own_config_proxy(id, env);

    match id {
        ProviderId::ClaudeCode => ProviderProxyStatus {
            provider: id,
            env_vars: vec![
                "HTTPS_PROXY".to_string(),
                "HTTP_PROXY".to_string(),
                "NO_PROXY".to_string(),
            ],
            extra_env: HashMap::new(),
            own_config_proxy,
            supports_socks: false,
            forced_supported: true,
            verified: true,
        },
        ProviderId::CodexCli => ProviderProxyStatus {
            provider: id,
            env_vars: vec![
                "HTTPS_PROXY".to_string(),
                "HTTP_PROXY".to_string(),
                "NO_PROXY".to_string(),
            ],
            extra_env: HashMap::new(),
            own_config_proxy,
            supports_socks: false,
            forced_supported: true,
            verified: true,
        },
        ProviderId::CursorCli => {
            let mut extra_env = HashMap::new();
            extra_env.insert("NODE_USE_ENV_PROXY".to_string(), "1".to_string());
            ProviderProxyStatus {
                provider: id,
                env_vars: vec![
                    "HTTPS_PROXY".to_string(),
                    "HTTP_PROXY".to_string(),
                    "NO_PROXY".to_string(),
                ],
                extra_env,
                own_config_proxy,
                supports_socks: false,
                forced_supported: true,
                verified: true,
            }
        }
        ProviderId::CopilotCli => {
            let mut extra_env = HashMap::new();
            extra_env.insert("NODE_USE_ENV_PROXY".to_string(), "1".to_string());
            ProviderProxyStatus {
                provider: id,
                env_vars: vec![
                    "HTTPS_PROXY".to_string(),
                    "HTTP_PROXY".to_string(),
                    "NO_PROXY".to_string(),
                ],
                extra_env,
                own_config_proxy,
                supports_socks: false,
                forced_supported: true,
                verified: true,
            }
        }
        ProviderId::AntigravityCli
        | ProviderId::Aider
        | ProviderId::Opencode
        | ProviderId::Reasonix => ProviderProxyStatus {
            provider: id,
            env_vars: vec![
                "HTTPS_PROXY".to_string(),
                "HTTP_PROXY".to_string(),
                "NO_PROXY".to_string(),
            ],
            extra_env: HashMap::new(),
            own_config_proxy,
            supports_socks: false,
            forced_supported: true,
            verified: true,
        },
        _ => {
            // General provider proxy status
            ProviderProxyStatus {
                provider: id,
                env_vars: vec![
                    "HTTPS_PROXY".to_string(),
                    "HTTP_PROXY".to_string(),
                    "NO_PROXY".to_string(),
                ],
                extra_env: HashMap::new(),
                own_config_proxy,
                supports_socks: false,
                forced_supported: true,
                verified: false,
            }
        }
    }
}

/// Inspects local configuration files of a CLI to check if it has configured its own proxy.
/// Returns a sanitized proxy string (host:port, no credentials) if found.
fn inspect_own_config_proxy(id: ProviderId, env: &ShellEnv) -> Option<String> {
    let home_dir = env
        .vars
        .get("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from));

    match id {
        ProviderId::ClaudeCode => {
            let dir = env
                .vars
                .get("CLAUDE_CONFIG_DIR")
                .map(PathBuf::from)
                .or_else(|| home_dir.map(|h| h.join(".claude")));

            let file = dir?.join("settings.json");
            let text = std::fs::read_to_string(file).ok()?;
            let val: serde_json::Value = serde_json::from_str(&text).ok()?;
            let env_obj = val.get("env")?.as_object()?;

            for key in ["HTTPS_PROXY", "HTTP_PROXY", "ALL_PROXY", "https_proxy", "http_proxy", "all_proxy"] {
                if let Some(proxy_val) = env_obj.get(key).and_then(|v| v.as_str()) {
                    let sanitized = sanitize_proxy_url(proxy_val);
                    if !sanitized.is_empty() {
                        return Some(sanitized);
                    }
                }
            }
            None
        }
        ProviderId::CodexCli => {
            let dir = env
                .vars
                .get("CODEX_HOME")
                .map(PathBuf::from)
                .or_else(|| home_dir.map(|h| h.join(".codex")));

            let file = dir?.join("config.toml");
            let text = std::fs::read_to_string(file).ok()?;

            for line in text.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with('#') {
                    continue;
                }
                if let Some((k, v)) = trimmed.split_once('=') {
                    let k = k.trim().to_ascii_lowercase();
                    if k == "proxy" || k == "http_proxy" || k == "https_proxy" {
                        let raw = v.split('#').next().unwrap_or("").trim().trim_matches(['"', '\'']);
                        let sanitized = sanitize_proxy_url(raw);
                        if !sanitized.is_empty() {
                            return Some(sanitized);
                        }
                    }
                }
            }
            None
        }
        ProviderId::AntigravityCli => {
            let dir = home_dir?.join(".gemini/antigravity-cli");
            let file = dir.join("settings.json");
            let text = std::fs::read_to_string(file).ok()?;
            let val: serde_json::Value = serde_json::from_str(&text).ok()?;
            if let Some(proxy_val) = val.get("proxy").and_then(|v| v.as_str()) {
                let sanitized = sanitize_proxy_url(proxy_val);
                if !sanitized.is_empty() {
                    return Some(sanitized);
                }
            }
            None
        }
        _ => None,
    }
}

/// Sanitizes a proxy URL to ensure username/password are stripped per R11.
/// e.g. `http://user:pass@127.0.0.1:7890/` -> `http://127.0.0.1:7890`
pub fn sanitize_proxy_url(input: &str) -> String {
    let s = input.trim();
    if s.is_empty() {
        return String::new();
    }

    let (proto, rest) = if let Some(after) = s.strip_prefix("http://") {
        ("http://", after)
    } else if let Some(after) = s.strip_prefix("https://") {
        ("https://", after)
    } else if let Some(after) = s.strip_prefix("socks5://") {
        ("socks5://", after)
    } else if let Some(after) = s.strip_prefix("socks5h://") {
        ("socks5h://", after)
    } else if let Some(after) = s.strip_prefix("socks://") {
        ("socks://", after)
    } else {
        ("", s)
    };

    let authority = rest.split('/').next().unwrap_or(rest);
    let host_port = match authority.rsplit_once('@') {
        Some((_, hp)) => hp,
        None => authority,
    };

    format!("{proto}{host_port}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_proxy_url() {
        assert_eq!(
            sanitize_proxy_url("http://user:password@proxy.example.com:8080/path"),
            "http://proxy.example.com:8080"
        );
        assert_eq!(
            sanitize_proxy_url("socks5://127.0.0.1:1080"),
            "socks5://127.0.0.1:1080"
        );
        assert_eq!(
            sanitize_proxy_url("127.0.0.1:7890"),
            "127.0.0.1:7890"
        );
    }

    #[test]
    fn test_claude_settings_proxy_inspection() {
        let temp_dir = tempfile::tempdir().unwrap();
        let claude_dir = temp_dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();

        std::fs::write(
            claude_dir.join("settings.json"),
            r#"{"env": {"HTTPS_PROXY": "http://admin:secret123@10.0.0.1:8888"}}"#,
        )
        .unwrap();

        let mut env_vars = HashMap::new();
        env_vars.insert("HOME".to_string(), temp_dir.path().to_str().unwrap().to_string());
        let env = ShellEnv { vars: env_vars };

        let status = inspect_provider_proxy(ProviderId::ClaudeCode, &env);
        assert_eq!(status.own_config_proxy.as_deref(), Some("http://10.0.0.1:8888"));
        assert!(status.verified);
    }
}
