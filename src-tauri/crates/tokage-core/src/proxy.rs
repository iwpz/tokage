//! Proxy configuration and status models (docs/12).

use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use crate::provider::ProviderId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyMode {
    InheritGlobal,
    CliOwn,
    None,
    System,
    Manual,
}

impl Default for ProxyMode {
    fn default() -> Self {
        Self::CliOwn
    }
}

impl ProxyMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::InheritGlobal => "inherit_global",
            Self::CliOwn => "cli_own",
            Self::None => "none",
            Self::System => "system",
            Self::Manual => "manual",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProxyProtocol {
    Http,
    Https,
    Socks5,
}

impl Default for ProxyProtocol {
    fn default() -> Self {
        Self::Http
    }
}

impl std::fmt::Display for ProxyProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http => write!(f, "http"),
            Self::Https => write!(f, "https"),
            Self::Socks5 => write!(f, "socks5"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyUpstream {
    pub protocol: ProxyProtocol,
    pub host: String,
    pub port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

impl ProxyUpstream {
    pub fn new(protocol: ProxyProtocol, host: impl Into<String>, port: u16) -> Self {
        Self {
            protocol,
            host: host.into(),
            port,
            username: None,
            password: None,
        }
    }

    pub fn to_url(&self, include_auth: bool) -> String {
        if include_auth {
            if let (Some(u), Some(p)) = (&self.username, &self.password) {
                return format!("{}://{}:{}@{}:{}", self.protocol, u, p, self.host, self.port);
            }
        }
        format!("{}://{}:{}", self.protocol, self.host, self.port)
    }
}

pub fn default_no_proxy() -> String {
    "localhost,127.0.0.1,::1".to_string()
}

fn default_cli_mode() -> ProxyMode {
    ProxyMode::InheritGlobal
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliProxyConfig {
    #[serde(default = "default_cli_mode")]
    pub mode: ProxyMode,
    #[serde(default)]
    pub forced: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream: Option<ProxyUpstream>,
}

impl Default for CliProxyConfig {
    fn default() -> Self {
        Self {
            mode: ProxyMode::InheritGlobal,
            forced: false,
            upstream: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlobalProxyConfig {
    #[serde(default)]
    pub mode: ProxyMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream: Option<ProxyUpstream>,
    #[serde(default = "default_no_proxy")]
    pub no_proxy: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ca_file: Option<String>,
    #[serde(default)]
    pub local_relay: bool,
    #[serde(default)]
    pub per_cli: HashMap<String, CliProxyConfig>,
}

impl Default for GlobalProxyConfig {
    fn default() -> Self {
        Self {
            mode: ProxyMode::CliOwn,
            upstream: None,
            no_proxy: default_no_proxy(),
            ca_file: None,
            local_relay: false,
            per_cli: HashMap::new(),
        }
    }
}

impl GlobalProxyConfig {
    pub fn resolved_mode_for(&self, provider_id: ProviderId) -> ProxyMode {
        let key = provider_id.as_str();
        if let Some(cli_cfg) = self.per_cli.get(key) {
            if cli_cfg.mode != ProxyMode::InheritGlobal {
                return cli_cfg.mode;
            }
        }
        self.mode
    }

    pub fn resolved_upstream_for(&self, provider_id: ProviderId) -> Option<&ProxyUpstream> {
        let key = provider_id.as_str();
        if let Some(cli_cfg) = self.per_cli.get(key) {
            if cli_cfg.mode == ProxyMode::Manual {
                if let Some(ref up) = cli_cfg.upstream {
                    return Some(up);
                }
            }
        }
        self.upstream.as_ref()
    }

    pub fn is_forced_for(&self, provider_id: ProviderId) -> bool {
        let key = provider_id.as_str();
        self.per_cli.get(key).map_or(false, |c| c.forced)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderProxyConfig {
    #[serde(default)]
    pub env: Vec<String>,
    #[serde(default)]
    pub extra_env: HashMap<String, String>,
    #[serde(default)]
    pub socks: bool,
    #[serde(default)]
    pub own_config_overrides: bool,
    #[serde(default)]
    pub forced_ok: bool,
    #[serde(default)]
    pub verified: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderProxyStatus {
    pub provider: ProviderId,
    pub env_vars: Vec<String>,
    pub extra_env: HashMap<String, String>,
    pub own_config_proxy: Option<String>,
    pub supports_socks: bool,
    pub forced_supported: bool,
    pub verified: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProxyTestResult {
    pub success: bool,
    pub endpoint: String,
    pub latency_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}
