//! Local CONNECT proxy relay (docs/12 §4, R18, P3.m6f).
//!
//! Features:
//! - Pure CONNECT byte-stream tunneling (no TLS decryption, no content payload inspection, docs/09 R18)
//! - Forwards to upstream HTTP, HTTPS, or SOCKS5 proxies, or direct TCP connection
//! - Inject upstream proxy credentials at the relay level, keeping them out of child process envs (P3.m6h)
//! - Ephemeral loopback port per CLI or shared across turns

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokage_core::proxy::{ProxyProtocol, ProxyUpstream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

pub struct RelayHandle {
    pub port: u16,
    pub bytes_transferred: Arc<AtomicU64>,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl RelayHandle {
    pub fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

impl Drop for RelayHandle {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

/// Starts a local CONNECT proxy relay bound to `127.0.0.1:0`.
pub async fn start_relay(upstream: Option<ProxyUpstream>) -> Result<RelayHandle, String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("Failed to bind local relay: {e}"))?;

    let port = listener
        .local_addr()
        .map_err(|e| e.to_string())?
        .port();

    let (shutdown_tx, mut shutdown_rx) = oneshot::channel();
    let bytes_transferred = Arc::new(AtomicU64::new(0));
    let bytes_acc = Arc::clone(&bytes_transferred);
    let running = Arc::new(AtomicBool::new(true));
    let running_flag = Arc::clone(&running);

    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut shutdown_rx => {
                    running_flag.store(false, Ordering::SeqCst);
                    break;
                }
                res = listener.accept() => {
                    match res {
                        Ok((client, _)) => {
                            let up = upstream.clone();
                            let b = Arc::clone(&bytes_acc);
                            tokio::spawn(async move {
                                let _ = handle_client(client, up, b).await;
                            });
                        }
                        Err(e) => {
                            tracing::warn!("Relay accept error: {e}");
                            tokio::time::sleep(Duration::from_millis(50)).await;
                        }
                    }
                }
            }
        }
    });

    Ok(RelayHandle {
        port,
        bytes_transferred,
        shutdown_tx: Some(shutdown_tx),
    })
}

async fn handle_client(
    mut client: TcpStream,
    upstream: Option<ProxyUpstream>,
    bytes_counter: Arc<AtomicU64>,
) -> Result<(), String> {
    // Read the HTTP request header up to \r\n\r\n
    let mut buf = [0u8; 4096];
    let n = client.read(&mut buf).await.map_err(|e| e.to_string())?;
    if n == 0 {
        return Ok(());
    }

    let header_text = String::from_utf8_lossy(&buf[..n]);
    let first_line = header_text.lines().next().unwrap_or_default().trim();

    // Parse CONNECT host:port HTTP/1.1
    let (target_host, target_port) = if first_line.starts_with("CONNECT ") {
        let parts: Vec<&str> = first_line.split_whitespace().collect();
        if parts.len() < 2 {
            let _ = client.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n").await;
            return Err("Invalid CONNECT request".into());
        }
        let authority = parts[1];
        parse_host_port(authority, 443)?
    } else {
        // Plain HTTP proxying: GET http://host:port/path HTTP/1.1
        let parts: Vec<&str> = first_line.split_whitespace().collect();
        if parts.len() < 2 {
            let _ = client.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n").await;
            return Err("Invalid request".into());
        }
        let url = parts[1];
        let without_proto = url
            .strip_prefix("http://")
            .or_else(|| url.strip_prefix("https://"))
            .unwrap_or(url);
        let host_part = without_proto.split('/').next().unwrap_or(without_proto);
        parse_host_port(host_part, 80)?
    };

    tracing::debug!("Relay tunneling to {target_host}:{target_port}");

    // Connect to upstream or target
    let mut upstream_conn = match upstream {
        Some(ref up) => connect_via_upstream(up, &target_host, target_port).await?,
        None => {
            TcpStream::connect((target_host.as_str(), target_port))
                .await
                .map_err(|e| format!("Failed to connect to {target_host}:{target_port}: {e}"))?
        }
    };

    // Respond to client that connection is established
    client
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await
        .map_err(|e| format!("Failed to write CONNECT response: {e}"))?;

    // Bidirectionally copy raw TCP bytes without decryption (R18)
    let (bytes_in, bytes_out) = tokio::io::copy_bidirectional(&mut client, &mut upstream_conn)
        .await
        .map_err(|e| e.to_string())?;

    bytes_counter.fetch_add(bytes_in + bytes_out, Ordering::Relaxed);
    Ok(())
}

fn parse_host_port(authority: &str, default_port: u16) -> Result<(String, u16), String> {
    if let Some((h, p)) = authority.rsplit_once(':') {
        let port = p
            .parse::<u16>()
            .map_err(|_| format!("Invalid port in {authority}"))?;
        Ok((h.trim_matches(['[', ']']).to_string(), port))
    } else {
        Ok((authority.trim_matches(['[', ']']).to_string(), default_port))
    }
}

async fn connect_via_upstream(
    upstream: &ProxyUpstream,
    target_host: &str,
    target_port: u16,
) -> Result<TcpStream, String> {
    let mut stream = TcpStream::connect((upstream.host.as_str(), upstream.port))
        .await
        .map_err(|e| format!("Failed to connect to upstream proxy {}: {e}", upstream.to_url(false)))?;

    match upstream.protocol {
        ProxyProtocol::Http | ProxyProtocol::Https => {
            let mut req = format!(
                "CONNECT {target_host}:{target_port} HTTP/1.1\r\nHost: {target_host}:{target_port}\r\n"
            );

            // Inject upstream credentials if present (P3.m6h)
            if let (Some(ref u), Some(ref p)) = (&upstream.username, &upstream.password) {
                let token = base64_encode(format!("{u}:{p}").as_bytes());
                req.push_str(&format!("Proxy-Authorization: Basic {token}\r\n"));
            }
            req.push_str("\r\n");

            stream
                .write_all(req.as_bytes())
                .await
                .map_err(|e| format!("Failed to send CONNECT to upstream: {e}"))?;

            let mut resp = [0u8; 1024];
            let n = stream
                .read(&mut resp)
                .await
                .map_err(|e| format!("Failed to read upstream CONNECT response: {e}"))?;

            let resp_text = String::from_utf8_lossy(&resp[..n]);
            if !resp_text.contains(" 200 ") {
                let status_line = resp_text.lines().next().unwrap_or("Unknown error");
                return Err(format!("Upstream proxy rejected CONNECT: {status_line}"));
            }

            Ok(stream)
        }
        ProxyProtocol::Socks5 => {
            // SOCKS5 greeting
            let has_auth = upstream.username.is_some() && upstream.password.is_some();
            if has_auth {
                stream.write_all(&[0x05, 0x02, 0x00, 0x02]).await.map_err(|e| e.to_string())?;
            } else {
                stream.write_all(&[0x05, 0x01, 0x00]).await.map_err(|e| e.to_string())?;
            }

            let mut resp = [0u8; 2];
            stream.read_exact(&mut resp).await.map_err(|e| e.to_string())?;
            if resp[0] != 0x05 {
                return Err("Invalid SOCKS5 version from upstream".into());
            }

            if resp[1] == 0x02 {
                // User/password auth
                let u = upstream.username.as_deref().unwrap_or_default();
                let p = upstream.password.as_deref().unwrap_or_default();
                let mut auth_req = Vec::new();
                auth_req.push(0x01);
                auth_req.push(u.len() as u8);
                auth_req.extend_from_slice(u.as_bytes());
                auth_req.push(p.len() as u8);
                auth_req.extend_from_slice(p.as_bytes());

                stream.write_all(&auth_req).await.map_err(|e| e.to_string())?;
                let mut auth_resp = [0u8; 2];
                stream.read_exact(&mut auth_resp).await.map_err(|e| e.to_string())?;
                if auth_resp[1] != 0x00 {
                    return Err("SOCKS5 authentication failed".into());
                }
            } else if resp[1] != 0x00 {
                return Err("No acceptable SOCKS5 authentication method".into());
            }

            // SOCKS5 Connect
            let mut conn_req = Vec::new();
            conn_req.extend_from_slice(&[0x05, 0x01, 0x00, 0x03, target_host.len() as u8]);
            conn_req.extend_from_slice(target_host.as_bytes());
            conn_req.extend_from_slice(&target_port.to_be_bytes());

            stream.write_all(&conn_req).await.map_err(|e| e.to_string())?;

            let mut head = [0u8; 4];
            stream.read_exact(&mut head).await.map_err(|e| e.to_string())?;
            if head[1] != 0x00 {
                return Err(format!("SOCKS5 connect error code: {}", head[1]));
            }

            // Discard bound address
            match head[3] {
                0x01 => {
                    let mut b = [0u8; 6]; // 4 bytes IP + 2 bytes port
                    stream.read_exact(&mut b).await.map_err(|e| e.to_string())?;
                }
                0x03 => {
                    let mut len = [0u8; 1];
                    stream.read_exact(&mut len).await.map_err(|e| e.to_string())?;
                    let mut b = vec![0u8; len[0] as usize + 2];
                    stream.read_exact(&mut b).await.map_err(|e| e.to_string())?;
                }
                0x04 => {
                    let mut b = [0u8; 18]; // 16 bytes IPv6 + 2 bytes port
                    stream.read_exact(&mut b).await.map_err(|e| e.to_string())?;
                }
                _ => {}
            }

            Ok(stream)
        }
    }
}

pub fn base64_encode(input: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i < input.len() {
        let b0 = input[i] as u32;
        let b1 = if i + 1 < input.len() { input[i + 1] as u32 } else { 0 };
        let b2 = if i + 2 < input.len() { input[i + 2] as u32 } else { 0 };
        let trip = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARS[((trip >> 18) & 63) as usize] as char);
        out.push(CHARS[((trip >> 12) & 63) as usize] as char);
        if i + 1 < input.len() {
            out.push(CHARS[((trip >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if i + 2 < input.len() {
            out.push(CHARS[(trip & 63) as usize] as char);
        } else {
            out.push('=');
        }
        i += 3;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base64_encode() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"user:pass"), "dXNlcjpwYXNz");
    }

    #[tokio::test]
    async fn test_local_relay_direct_connect_tunnel() {
        // Start an echo TCP server acting as the target server
        let target_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target_port = target_listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            if let Ok((mut socket, _)) = target_listener.accept().await {
                let mut buf = [0u8; 12];
                if socket.read_exact(&mut buf).await.is_ok() {
                    let _ = socket.write_all(b"ECHO:").await;
                    let _ = socket.write_all(&buf).await;
                }
            }
        });

        // Start local relay with direct connection (no upstream)
        let relay = start_relay(None).await.unwrap();

        // Connect client to local relay
        let mut client = TcpStream::connect(("127.0.0.1", relay.port)).await.unwrap();
        let req = format!("CONNECT 127.0.0.1:{target_port} HTTP/1.1\r\nHost: 127.0.0.1:{target_port}\r\n\r\n");
        client.write_all(req.as_bytes()).await.unwrap();

        let mut resp = [0u8; 39];
        client.read_exact(&mut resp).await.unwrap();
        assert_eq!(&resp, b"HTTP/1.1 200 Connection Established\r\n\r\n");

        // Send payload through tunnel
        client.write_all(b"HELLO-TUNNEL").await.unwrap();
        let mut echo = [0u8; 17];
        client.read_exact(&mut echo).await.unwrap();
        assert_eq!(&echo, b"ECHO:HELLO-TUNNEL");

        drop(client);
        tokio::time::sleep(Duration::from_millis(50)).await;

        assert!(relay.bytes_transferred.load(Ordering::Relaxed) > 0);
        relay.shutdown();
    }
}
