//! Non-token-consuming TLS connectivity probe (docs/12 §5, P3.m6d).
//!
//! Initiates a lightweight HTTP/TLS handshake to verify that the active or configured proxy
//! can connect to the provider's official API endpoints without consuming any subscription
//! tokens or quota.

use std::time::Instant;

use tokage_core::proxy::{ProxyTestResult, ProxyUpstream};
use tokage_core::ProviderId;

/// Default official API endpoint to probe for each provider.
pub fn default_endpoint_for(provider: ProviderId) -> &'static str {
    match provider {
        ProviderId::ClaudeCode => "https://api.anthropic.com",
        ProviderId::CodexCli => "https://api.openai.com",
        ProviderId::CursorCli => "https://api2.cursor.sh",
        ProviderId::CopilotCli => "https://api.github.com",
        ProviderId::AntigravityCli => "https://generativelanguage.googleapis.com",
        _ => "https://api.openai.com",
    }
}

/// Tests proxy connectivity by executing a lightweight TLS probe to the specified endpoint.
pub async fn test_proxy_connection(
    endpoint: Option<&str>,
    upstream: Option<&ProxyUpstream>,
    no_proxy: Option<&str>,
    ca_file: Option<&str>,
) -> ProxyTestResult {
    let target = endpoint.unwrap_or("https://api.openai.com");
    let target_url = if target.starts_with("http://") || target.starts_with("https://") {
        target.to_string()
    } else {
        format!("https://{target}")
    };

    let start = Instant::now();

    let mut cmd = tokio::process::Command::new(if cfg!(windows) { "curl.exe" } else { "curl" });
    cmd.args([
        "--silent",
        "--show-error",
        "--connect-timeout",
        "8",
        "--max-time",
        "10",
        "-o",
        "/dev/null",
        "-w",
        "%{http_code}",
        "--proto",
        "=https,http",
    ]);

    if let Some(up) = upstream {
        let proxy_url = up.to_url(true);
        cmd.arg("--proxy").arg(proxy_url);
    }

    if let Some(np) = no_proxy {
        if !np.trim().is_empty() {
            cmd.arg("--noproxy").arg(np);
        }
    }

    if let Some(ca) = ca_file {
        if !ca.trim().is_empty() {
            cmd.arg("--cacert").arg(ca);
        }
    }

    cmd.arg("--").arg(&target_url);

    // Run isolated without passing full parent shell env
    crate::proc::isolate(&mut cmd);

    let output = match cmd.output().await {
        Ok(out) => out,
        Err(e) => {
            return ProxyTestResult {
                success: false,
                endpoint: target_url,
                latency_ms: start.elapsed().as_millis() as u64,
                message: Some(format!("Failed to execute probe: {e}")),
            };
        }
    };

    let latency_ms = start.elapsed().as_millis() as u64;

    if output.status.success() {
        let code_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        ProxyTestResult {
            success: true,
            endpoint: target_url,
            latency_ms,
            message: Some(format!("HTTP {code_str}")),
        }
    } else {
        let err_msg = String::from_utf8_lossy(&output.stderr).trim().to_string();
        ProxyTestResult {
            success: false,
            endpoint: target_url,
            latency_ms,
            message: Some(if err_msg.is_empty() {
                format!("Probe exited with code {}", output.status)
            } else {
                err_msg
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_endpoints() {
        assert_eq!(default_endpoint_for(ProviderId::ClaudeCode), "https://api.anthropic.com");
        assert_eq!(default_endpoint_for(ProviderId::CodexCli), "https://api.openai.com");
        assert_eq!(default_endpoint_for(ProviderId::CursorCli), "https://api2.cursor.sh");
        assert_eq!(default_endpoint_for(ProviderId::CopilotCli), "https://api.github.com");
    }
}
