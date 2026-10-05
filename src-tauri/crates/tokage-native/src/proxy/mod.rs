//! Network proxy management (docs/12).

pub mod inspect;
pub mod keychain;
pub mod relay;
pub mod system;
pub mod test_connect;

use std::collections::HashMap;

use tokage_core::proxy::{GlobalProxyConfig, ProxyMode, ProxyProtocol};
use tokage_core::ProviderId;

pub use inspect::inspect_provider_proxy;
pub use keychain::{delete_proxy_credential, get_proxy_credential, store_proxy_credential};
pub use relay::{start_relay, RelayHandle};
pub use system::{detect_system_no_proxy, detect_system_proxy, parse_proxy_url};
pub use test_connect::{default_endpoint_for, test_proxy_connection};

use crate::env::ShellEnv;

/// Prepared proxy environment and optional local relay for a conversation turn.
pub struct PreparedTurnProxy {
    pub mode: ProxyMode,
    pub relay_handle: Option<RelayHandle>,
    pub proxy_url: Option<String>,
    pub no_proxy: Option<String>,
    pub ca_file: Option<String>,
    pub extra_env: HashMap<String, String>,
    pub forced: bool,
}

/// Resolves proxy settings and starts a local CONNECT relay if needed for the given turn.
pub async fn prepare_turn_proxy(
    config: &GlobalProxyConfig,
    provider: ProviderId,
    env: &ShellEnv,
) -> Result<PreparedTurnProxy, String> {
    let mode = config.resolved_mode_for(provider);
    let forced = config.is_forced_for(provider);
    let inspect_status = inspect_provider_proxy(provider, env);

    let ca_file = config
        .ca_file
        .as_ref()
        .filter(|s| !s.trim().is_empty())
        .cloned();

    let extra_env = inspect_status.extra_env.clone();

    match mode {
        ProxyMode::CliOwn => {
            if forced {
                // Forced mode requires local relay to gate outbound traffic to localhost
                let relay = start_relay(None).await?;
                let proxy_url = format!("http://127.0.0.1:{}", relay.port);
                Ok(PreparedTurnProxy {
                    mode,
                    relay_handle: Some(relay),
                    proxy_url: Some(proxy_url),
                    no_proxy: Some(config.no_proxy.clone()),
                    ca_file,
                    extra_env,
                    forced: true,
                })
            } else {
                Ok(PreparedTurnProxy {
                    mode,
                    relay_handle: None,
                    proxy_url: None,
                    no_proxy: None,
                    ca_file,
                    extra_env,
                    forced: false,
                })
            }
        }
        ProxyMode::None => {
            if forced {
                // None + Forced: local relay connects directly upstream, overriding any CLI configs (docs/12 §4)
                let relay = start_relay(None).await?;
                let proxy_url = format!("http://127.0.0.1:{}", relay.port);
                Ok(PreparedTurnProxy {
                    mode,
                    relay_handle: Some(relay),
                    proxy_url: Some(proxy_url),
                    no_proxy: Some(config.no_proxy.clone()),
                    ca_file,
                    extra_env,
                    forced: true,
                })
            } else {
                Ok(PreparedTurnProxy {
                    mode,
                    relay_handle: None,
                    proxy_url: None,
                    no_proxy: None,
                    ca_file,
                    extra_env,
                    forced: false,
                })
            }
        }
        ProxyMode::System => {
            let sys_proxy = detect_system_proxy();
            let sys_no_proxy = detect_system_no_proxy()
                .or_else(|| (!config.no_proxy.trim().is_empty()).then(|| config.no_proxy.clone()));

            if let Some(up) = sys_proxy {
                let needs_relay = config.local_relay
                    || forced
                    || up.protocol == ProxyProtocol::Socks5
                    || up.username.is_some();

                if needs_relay {
                    let relay = start_relay(Some(up)).await?;
                    let proxy_url = format!("http://127.0.0.1:{}", relay.port);
                    Ok(PreparedTurnProxy {
                        mode,
                        relay_handle: Some(relay),
                        proxy_url: Some(proxy_url),
                        no_proxy: sys_no_proxy,
                        ca_file,
                        extra_env,
                        forced,
                    })
                } else {
                    let proxy_url = up.to_url(false);
                    Ok(PreparedTurnProxy {
                        mode,
                        relay_handle: None,
                        proxy_url: Some(proxy_url),
                        no_proxy: sys_no_proxy,
                        ca_file,
                        extra_env,
                        forced: false,
                    })
                }
            } else if forced {
                // No system proxy detected, but forced mode is on: route direct via local relay
                let relay = start_relay(None).await?;
                let proxy_url = format!("http://127.0.0.1:{}", relay.port);
                Ok(PreparedTurnProxy {
                    mode,
                    relay_handle: Some(relay),
                    proxy_url: Some(proxy_url),
                    no_proxy: sys_no_proxy,
                    ca_file,
                    extra_env,
                    forced: true,
                })
            } else {
                Ok(PreparedTurnProxy {
                    mode,
                    relay_handle: None,
                    proxy_url: None,
                    no_proxy: sys_no_proxy,
                    ca_file,
                    extra_env,
                    forced: false,
                })
            }
        }
        ProxyMode::Manual => {
            let resolved_upstream = config.resolved_upstream_for(provider).cloned();

            if let Some(mut up) = resolved_upstream {
                // If upstream has username but no password, try retrieving from keychain
                if let (Some(ref u), None) = (&up.username, &up.password) {
                    if let Some(secret) = get_proxy_credential(u) {
                        up.password = Some(secret);
                    }
                }

                let needs_relay = config.local_relay
                    || forced
                    || up.protocol == ProxyProtocol::Socks5
                    || up.username.is_some();

                if needs_relay {
                    let relay = start_relay(Some(up)).await?;
                    let proxy_url = format!("http://127.0.0.1:{}", relay.port);
                    Ok(PreparedTurnProxy {
                        mode,
                        relay_handle: Some(relay),
                        proxy_url: Some(proxy_url),
                        no_proxy: Some(config.no_proxy.clone()),
                        ca_file,
                        extra_env,
                        forced,
                    })
                } else {
                    let proxy_url = up.to_url(false);
                    Ok(PreparedTurnProxy {
                        mode,
                        relay_handle: None,
                        proxy_url: Some(proxy_url),
                        no_proxy: Some(config.no_proxy.clone()),
                        ca_file,
                        extra_env,
                        forced: false,
                    })
                }
            } else if forced {
                let relay = start_relay(None).await?;
                let proxy_url = format!("http://127.0.0.1:{}", relay.port);
                Ok(PreparedTurnProxy {
                    mode,
                    relay_handle: Some(relay),
                    proxy_url: Some(proxy_url),
                    no_proxy: Some(config.no_proxy.clone()),
                    ca_file,
                    extra_env,
                    forced: true,
                })
            } else {
                Ok(PreparedTurnProxy {
                    mode,
                    relay_handle: None,
                    proxy_url: None,
                    no_proxy: Some(config.no_proxy.clone()),
                    ca_file,
                    extra_env,
                    forced: false,
                })
            }
        }
        ProxyMode::InheritGlobal => {
            // Already resolved by config.resolved_mode_for
            unreachable!("InheritGlobal should be resolved to actual mode");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokage_core::proxy::{CliProxyConfig, ProxyProtocol, ProxyUpstream};

    const CLIS: [ProviderId; 4] = [
        ProviderId::ClaudeCode,
        ProviderId::CodexCli,
        ProviderId::CursorCli,
        ProviderId::CopilotCli,
    ];

    #[tokio::test]
    async fn test_all_clis_five_proxy_modes_acceptance() {
        let mut vars = HashMap::new();
        vars.insert("HOME".into(), "/Users/testuser".into());
        vars.insert("HTTPS_PROXY".into(), "http://login-shell-proxy:8080".into());
        vars.insert("HTTP_PROXY".into(), "http://login-shell-proxy:8080".into());
        let env = ShellEnv { vars };

        for provider in CLIS {
            // 1. None mode
            let mut cfg_none = GlobalProxyConfig::default();
            cfg_none.mode = ProxyMode::None;
            let prep_none = prepare_turn_proxy(&cfg_none, provider, &env).await.unwrap();
            assert_eq!(prep_none.mode, ProxyMode::None);
            assert_eq!(prep_none.proxy_url, None);
            let child_env = crate::chat::child_env::assemble_with_proxy(provider, &env, &[], Some(&prep_none));
            assert!(!child_env.contains_key("HTTPS_PROXY"));
            assert!(!child_env.contains_key("HTTP_PROXY"));

            // 2. CliOwn mode
            let mut cfg_own = GlobalProxyConfig::default();
            cfg_own.mode = ProxyMode::CliOwn;
            let prep_own = prepare_turn_proxy(&cfg_own, provider, &env).await.unwrap();
            assert_eq!(prep_own.mode, ProxyMode::CliOwn);
            let child_env = crate::chat::child_env::assemble_with_proxy(provider, &env, &[], Some(&prep_own));
            assert_eq!(child_env.get("HTTPS_PROXY").map(String::as_str), Some("http://login-shell-proxy:8080"));

            // 3. System mode
            let mut cfg_sys = GlobalProxyConfig::default();
            cfg_sys.mode = ProxyMode::System;
            let prep_sys = prepare_turn_proxy(&cfg_sys, provider, &env).await.unwrap();
            assert_eq!(prep_sys.mode, ProxyMode::System);

            // 4. Manual mode (direct upstream URL)
            let mut cfg_manual = GlobalProxyConfig::default();
            cfg_manual.mode = ProxyMode::Manual;
            cfg_manual.upstream = Some(ProxyUpstream::new(ProxyProtocol::Http, "manual-proxy.example", 8080));
            cfg_manual.ca_file = Some("/etc/ssl/corp-ca.pem".into());
            let prep_manual = prepare_turn_proxy(&cfg_manual, provider, &env).await.unwrap();
            assert_eq!(prep_manual.mode, ProxyMode::Manual);
            assert_eq!(prep_manual.proxy_url.as_deref(), Some("http://manual-proxy.example:8080"));
            let child_env = crate::chat::child_env::assemble_with_proxy(provider, &env, &[], Some(&prep_manual));
            assert_eq!(child_env.get("HTTPS_PROXY").map(String::as_str), Some("http://manual-proxy.example:8080"));
            assert_eq!(child_env.get("SSL_CERT_FILE").map(String::as_str), Some("/etc/ssl/corp-ca.pem"));

            // 5. InheritGlobal mode with Per-CLI override
            let mut cfg_inherit = GlobalProxyConfig::default();
            cfg_inherit.mode = ProxyMode::None;
            // Override this CLI to manual
            let mut per_cli = HashMap::new();
            per_cli.insert(
                provider.as_str().to_string(),
                CliProxyConfig {
                    mode: ProxyMode::Manual,
                    forced: false,
                    upstream: Some(ProxyUpstream::new(ProxyProtocol::Http, "custom-cli-proxy", 9090)),
                },
            );
            cfg_inherit.per_cli = per_cli;
            let prep_inherit = prepare_turn_proxy(&cfg_inherit, provider, &env).await.unwrap();
            assert_eq!(prep_inherit.mode, ProxyMode::Manual);
            assert_eq!(prep_inherit.proxy_url.as_deref(), Some("http://custom-cli-proxy:9090"));
        }
    }

    #[tokio::test]
    async fn test_forced_mode_binds_local_relay_and_hides_creds() {
        let env = ShellEnv { vars: HashMap::new() };

        let mut cfg = GlobalProxyConfig::default();
        cfg.mode = ProxyMode::Manual;
        let mut up = ProxyUpstream::new(ProxyProtocol::Http, "upstream.corp", 8080);
        up.username = Some("proxy_user".into());
        up.password = Some("super_secret_proxy_pass".into());
        cfg.upstream = Some(up);

        let mut per_cli = HashMap::new();
        per_cli.insert(
            "CLAUDE_CODE".into(),
            CliProxyConfig {
                mode: ProxyMode::InheritGlobal,
                forced: true,
                upstream: None,
            },
        );
        cfg.per_cli = per_cli;

        let prepared = prepare_turn_proxy(&cfg, ProviderId::ClaudeCode, &env).await.unwrap();
        assert!(prepared.forced);
        assert!(prepared.relay_handle.is_some());
        let port = prepared.relay_handle.as_ref().unwrap().port;
        assert!(port > 0);
        assert_eq!(prepared.proxy_url.as_deref(), Some(format!("http://127.0.0.1:{port}").as_str()));

        // Check child environment
        let child_env = crate::chat::child_env::assemble_with_proxy(
            ProviderId::ClaudeCode,
            &env,
            &[],
            Some(&prepared),
        );

        assert_eq!(child_env.get("HTTPS_PROXY").map(String::as_str), Some(format!("http://127.0.0.1:{port}").as_str()));
        // Ensure proxy_user and super_secret_proxy_pass never appear in environment
        for (k, v) in &child_env {
            assert!(!v.contains("super_secret_proxy_pass"), "{k} leaked proxy password");
            assert!(!v.contains("proxy_user"), "{k} leaked proxy username");
        }

        prepared.relay_handle.unwrap().shutdown();
    }
}
