//! System proxy configuration reader (docs/12 §3, P3.m6c).

use std::collections::HashMap;
use tokage_core::proxy::{ProxyProtocol, ProxyUpstream};

/// Reads the operating system's currently active proxy configuration.
pub fn detect_system_proxy() -> Option<ProxyUpstream> {
    #[cfg(target_os = "macos")]
    {
        if let Some(up) = detect_macos_proxy() {
            return Some(up);
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(up) = detect_windows_proxy() {
            return Some(up);
        }
    }

    // Common fallback: check standard environment variables
    detect_from_env()
}

/// Reads the system-level bypass / NO_PROXY list, if available.
pub fn detect_system_no_proxy() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        if let Some(np) = detect_macos_no_proxy() {
            return Some(np);
        }
    }

    for key in ["NO_PROXY", "no_proxy"] {
        if let Ok(val) = std::env::var(key) {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }

    None
}

fn detect_from_env() -> Option<ProxyUpstream> {
    for key in [
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "HTTP_PROXY",
        "http_proxy",
    ] {
        if let Ok(val) = std::env::var(key) {
            if let Some(up) = parse_proxy_url(&val) {
                return Some(up);
            }
        }
    }
    None
}

/// Parses a proxy string (e.g. `http://127.0.0.1:7890`, `socks5://127.0.0.1:1080`, `127.0.0.1:8080`)
pub fn parse_proxy_url(input: &str) -> Option<ProxyUpstream> {
    let s = input.trim();
    if s.is_empty() {
        return None;
    }

    let (proto, rest) = if let Some(after) = s.strip_prefix("http://") {
        (ProxyProtocol::Http, after)
    } else if let Some(after) = s.strip_prefix("https://") {
        (ProxyProtocol::Https, after)
    } else if let Some(after) = s.strip_prefix("socks5://").or_else(|| s.strip_prefix("socks5h://")) {
        (ProxyProtocol::Socks5, after)
    } else if let Some(after) = s.strip_prefix("socks://") {
        (ProxyProtocol::Socks5, after)
    } else {
        (ProxyProtocol::Http, s)
    };

    // Strip trailing path/slash
    let authority = rest.split('/').next().unwrap_or(rest);

    // Optional user:pass@host:port
    let (auth, host_port) = match authority.rsplit_once('@') {
        Some((a, hp)) => (Some(a), hp),
        None => (None, authority),
    };

    let (username, password) = if let Some(a) = auth {
        match a.split_once(':') {
            Some((u, p)) => (Some(u.to_string()), Some(p.to_string())),
            None => (Some(a.to_string()), None),
        }
    } else {
        (None, None)
    };

    let (host, port) = match host_port.rsplit_once(':') {
        Some((h, p)) => {
            let port_num = p.parse::<u16>().ok()?;
            (h.trim_matches(['[', ']']).to_string(), port_num)
        }
        None => {
            let default_port = match proto {
                ProxyProtocol::Http => 80,
                ProxyProtocol::Https => 443,
                ProxyProtocol::Socks5 => 1080,
            };
            (host_port.trim_matches(['[', ']']).to_string(), default_port)
        }
    };

    if host.is_empty() {
        return None;
    }

    Some(ProxyUpstream {
        protocol: proto,
        host,
        port,
        username,
        password,
    })
}

#[cfg(target_os = "macos")]
fn detect_macos_proxy() -> Option<ProxyUpstream> {
    let output = std::process::Command::new("/usr/sbin/scutil")
        .arg("--proxy")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    parse_scutil_output(&text)
}

#[cfg(target_os = "macos")]
fn detect_macos_no_proxy() -> Option<String> {
    let output = std::process::Command::new("/usr/sbin/scutil")
        .arg("--proxy")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    parse_scutil_exceptions(&text)
}

/// Parses the output of `scutil --proxy` dictionary into a `ProxyUpstream`.
pub fn parse_scutil_output(text: &str) -> Option<ProxyUpstream> {
    let mut map: HashMap<String, String> = HashMap::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some((k, v)) = trimmed.split_once(':') {
            map.insert(k.trim().to_string(), v.trim().to_string());
        }
    }

    // Priority: HTTPS > SOCKS > HTTP
    if map.get("HTTPSEnable").map(|s| s.as_str()) == Some("1") {
        if let (Some(host), Some(port_str)) = (map.get("HTTPSProxy"), map.get("HTTPSPort")) {
            if let Ok(port) = port_str.parse::<u16>() {
                return Some(ProxyUpstream {
                    protocol: ProxyProtocol::Http, // CONNECT to HTTPS proxy uses HTTP tunnel
                    host: host.clone(),
                    port,
                    username: None,
                    password: None,
                });
            }
        }
    }

    if map.get("SOCKSEnable").map(|s| s.as_str()) == Some("1") {
        if let (Some(host), Some(port_str)) = (map.get("SOCKSProxy"), map.get("SOCKSPort")) {
            if let Ok(port) = port_str.parse::<u16>() {
                return Some(ProxyUpstream {
                    protocol: ProxyProtocol::Socks5,
                    host: host.clone(),
                    port,
                    username: None,
                    password: None,
                });
            }
        }
    }

    if map.get("HTTPEnable").map(|s| s.as_str()) == Some("1") {
        if let (Some(host), Some(port_str)) = (map.get("HTTPProxy"), map.get("HTTPPort")) {
            if let Ok(port) = port_str.parse::<u16>() {
                return Some(ProxyUpstream {
                    protocol: ProxyProtocol::Http,
                    host: host.clone(),
                    port,
                    username: None,
                    password: None,
                });
            }
        }
    }

    None
}

/// Extracts ExceptionsList from `scutil --proxy`
pub fn parse_scutil_exceptions(text: &str) -> Option<String> {
    let mut in_exceptions = false;
    let mut items = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("ExceptionsList : <array>") {
            in_exceptions = true;
            continue;
        }
        if in_exceptions {
            if trimmed.starts_with('}') {
                break;
            }
            if let Some((_, val)) = trimmed.split_once(':') {
                let v = val.trim();
                if !v.is_empty() {
                    items.push(v.to_string());
                }
            }
        }
    }

    if items.is_empty() {
        None
    } else {
        Some(items.join(","))
    }
}

#[cfg(target_os = "windows")]
fn detect_windows_proxy() -> Option<ProxyUpstream> {
    // Check registry: HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings
    // Via cmd query
    let output = std::process::Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
            "/v",
            "ProxyEnable",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let out_str = String::from_utf8_lossy(&output.stdout);
    if !out_str.contains("0x1") {
        return None;
    }

    let server_out = std::process::Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
            "/v",
            "ProxyServer",
        ])
        .output()
        .ok()?;

    let server_str = String::from_utf8_lossy(&server_out.stdout);
    for line in server_str.lines() {
        if line.contains("ProxyServer") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                let server = parts[2];
                // Could be `http=127.0.0.1:7890;https=127.0.0.1:7890` or `127.0.0.1:7890`
                if let Some(https_part) = server.split(';').find(|s| s.starts_with("https=")) {
                    return parse_proxy_url(&https_part["https=".len()..]);
                } else if let Some(first) = server.split(';').next() {
                    let clean = first.split('=').last().unwrap_or(first);
                    return parse_proxy_url(clean);
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_various_proxy_formats() {
        let p1 = parse_proxy_url("http://127.0.0.1:7890").unwrap();
        assert_eq!(p1.protocol, ProxyProtocol::Http);
        assert_eq!(p1.host, "127.0.0.1");
        assert_eq!(p1.port, 7890);
        assert_eq!(p1.username, None);

        let p2 = parse_proxy_url("socks5://user:pass@proxy.corp.internal:1080/").unwrap();
        assert_eq!(p2.protocol, ProxyProtocol::Socks5);
        assert_eq!(p2.host, "proxy.corp.internal");
        assert_eq!(p2.port, 1080);
        assert_eq!(p2.username.as_deref(), Some("user"));
        assert_eq!(p2.password.as_deref(), Some("pass"));

        let p3 = parse_proxy_url("my-proxy.com:8080").unwrap();
        assert_eq!(p3.protocol, ProxyProtocol::Http);
        assert_eq!(p3.host, "my-proxy.com");
        assert_eq!(p3.port, 8080);
    }

    #[test]
    fn parse_macos_scutil_sample() {
        let sample = r#"
<dictionary> {
  ExceptionsList : <array> {
    0 : *.local
    1 : 169.254/16
    2 : 127.0.0.1
  }
  HTTPEnable : 1
  HTTPPort : 7890
  HTTPProxy : 127.0.0.1
  HTTPSEnable : 1
  HTTPSPort : 7890
  HTTPSProxy : 127.0.0.1
  ProxyAutoConfigEnable : 0
  SOCKSEnable : 0
}
"#;
        let up = parse_scutil_output(sample).unwrap();
        assert_eq!(up.host, "127.0.0.1");
        assert_eq!(up.port, 7890);

        let exceptions = parse_scutil_exceptions(sample).unwrap();
        assert_eq!(exceptions, "*.local,169.254/16,127.0.0.1");
    }
}
