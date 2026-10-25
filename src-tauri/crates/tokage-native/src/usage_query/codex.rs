//! Pure decoder for the public app-server account/rateLimits/read response.
//! Fixed management-only app-server transport; the official CLI owns authentication refresh.
use super::contract::{decimal, Quantity, QueryOutput, QuerySource, QuotaWindow};
use serde_json::Value;

use super::{
    contract::{QueryError, QueryFailure},
    identity::{self, Identity},
};
use crate::{env::ShellEnv, online_usage::QueryPermit, proxy::PreparedTurnProxy};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
fn fail(category: QueryError) -> QueryFailure {
    QueryFailure {
        category,
        retry_after: None,
    }
}
struct Source {
    identity: Identity,
    account_id: String,
    credential_revision: String,
}
fn root(env: &ShellEnv) -> Option<PathBuf> {
    env.vars.get("CODEX_HOME").map(PathBuf::from).or_else(|| {
        env.vars
            .get("HOME")
            .map(|h| PathBuf::from(h).join(".codex"))
    })
}
fn source_from(
    auth: &Value,
    config: &str,
    exe: &Path,
    root: &Path,
    env: &ShellEnv,
    salt: &[u8],
) -> Result<Source, QueryFailure> {
    let c: toml::Value = toml::from_str(config).map_err(|_| fail(QueryError::InvalidData))?;
    // A generic API-key/provider account cannot establish ChatGPT workspace limits.
    if auth["auth_mode"] != "chatgpt"
        || env.vars.get("CODEX_API_KEY").is_some_and(|v| !v.is_empty())
        || env
            .vars
            .get("OPENAI_BASE_URL")
            .is_some_and(|v| !v.is_empty())
        || c.get("cli_auth_credentials_store").and_then(|v|v.as_str()).is_some_and(|s|s != "file")
        || c.get("model_provider")
            .and_then(|v| v.as_str())
            .is_some_and(|p| p != "openai")
        || ["profile", "openai_base_url", "chatgpt_base_url"]
            .iter()
            .any(|k| c.get(*k).is_some())
        || c.get("model_providers")
            .and_then(|v| v.get("openai"))
            .is_some()
    {
        return Err(fail(QueryError::Identity));
    }
    let id = auth
        .pointer("/tokens/account_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control))
        .ok_or_else(|| fail(QueryError::Identity))?;
    Ok(Source {
        credential_revision: identity::opaque_key(salt, &[b"codex-local-credential-revision", auth.pointer("/tokens/access_token").and_then(Value::as_str).unwrap_or("").as_bytes(), auth.pointer("/tokens/refresh_token").and_then(Value::as_str).unwrap_or("").as_bytes()]),
        identity: Identity {
            key: identity::opaque_key(
                salt,
                &[
                    b"codex-cli-chatgpt-workspace",
                    exe.as_os_str().as_encoded_bytes(),
                    root.as_os_str().as_encoded_bytes(),
                    id.as_bytes(),
                ],
            ),
        },
        account_id: id.into(),
    })
}
fn source(exe: &Path, env: &ShellEnv, salt: &[u8]) -> Result<Source, QueryFailure> {
    let root = root(env).ok_or_else(|| fail(QueryError::Identity))?;
    let auth = super::read_bounded(&root.join("auth.json"), 131072)
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .ok_or_else(|| fail(QueryError::Identity))?;
    let config = match std::fs::metadata(root.join("config.toml")) {
        Ok(_) => String::from_utf8(
            super::read_bounded(&root.join("config.toml"), 1048576)
                .ok_or_else(|| fail(QueryError::Identity))?,
        )
        .map_err(|_| fail(QueryError::InvalidData))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(_) => return Err(fail(QueryError::Identity)),
    };
    source_from(&auth, &config, exe, &root, env, salt)
}
pub(crate) fn resolve(exe: &Path, env: &ShellEnv, salt: &[u8]) -> Option<Identity> {
    source(exe, env, salt).ok().map(|s| s.identity)
}
pub(crate) fn observe(exe: &Path, env: &ShellEnv, salt: &[u8]) -> Result<identity::Observation, QueryFailure> {
    let s=source(exe, env, salt)?;
    Ok(identity::Observation { identity:s.identity, source_login:None, credential_revision:Some(s.credential_revision) })
}
async fn line<R: tokio::io::AsyncBufRead + Unpin>(reader: &mut R) -> Result<Value, QueryFailure> {
    let mut text = Vec::new();
    loop {
        let chunk = reader
            .fill_buf()
            .await
            .map_err(|_| fail(QueryError::Network))?;
        if chunk.is_empty() {
            return Err(fail(QueryError::Network));
        }
        let n = chunk
            .iter()
            .position(|b| *b == b'\n')
            .map_or(chunk.len(), |i| i + 1);
        if text.len() + n > 65536 {
            return Err(fail(QueryError::InvalidData));
        }
        text.extend_from_slice(&chunk[..n]);
        reader.consume(n);
        if text.ends_with(b"\n") {
            return serde_json::from_slice(&text).map_err(|_| fail(QueryError::InvalidData));
        }
    }
}
async fn rpc<R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut R,
    input: &mut tokio::process::ChildStdin,
    id: u32,
    method: &str,
    params: Value,
) -> Result<Value, QueryFailure> {
    let mut bytes =
        serde_json::to_vec(&serde_json::json!({"id":id,"method":method,"params":params})).unwrap();
    bytes.push(b'\n');
    input
        .write_all(&bytes)
        .await
        .map_err(|_| fail(QueryError::Network))?;
    for _ in 0..128 {
        let v = line(reader).await?;
        // No approvals, external-token callbacks or other server requests are serviced.
        if v.get("method").is_some() {
            if v.get("id").is_some() {
                return Err(fail(QueryError::InvalidData));
            }
            continue;
        }
        if v["id"] != id {
            return Err(fail(QueryError::InvalidData));
        }
        if let Some(e) = v.get("error") {
            let message = e["message"].as_str().unwrap_or("").to_ascii_lowercase();
            let category = if [
                "401",
                "403",
                "unauthorized",
                "not logged",
                "not authenticated",
                "authentication",
                "requires chatgpt",
            ]
            .iter()
            .any(|s| message.contains(s))
            {
                QueryError::Authentication
            } else if message.contains("429") || message.contains("rate limit") {
                QueryError::RateLimited
            } else if e["code"] == -32601 || e["code"] == -32602 {
                QueryError::InvalidData
            } else {
                QueryError::Network
            };
            return Err(fail(category));
        }
        return v
            .get("result")
            .cloned()
            .ok_or_else(|| fail(QueryError::InvalidData));
    }
    Err(fail(QueryError::InvalidData))
}
pub(crate) async fn query(
    permit: &QueryPermit,
    exe: &Path,
    env: &ShellEnv,
    salt: &[u8],
    expected: &Identity,
    proxy: &PreparedTurnProxy,
) -> Result<QueryOutput, QueryFailure> {
    let src = source(exe, env, salt)?;
    if &src.identity != expected {
        return Err(fail(QueryError::Identity));
    }
    let dir = tempfile::tempdir().map_err(|_| fail(QueryError::Persistence))?;
    let args = ["app-server", "-c", "mcp_servers={}", "--listen", "stdio://"].map(String::from);
    let mut cmd = crate::sandbox::create_usage_command(
        tokage_core::ProviderId::CodexCli,
        exe,
        &args,
        &dir.path().to_string_lossy(),
        env.vars.get("HOME").map(String::as_str),
    )
    .map_err(|_| fail(QueryError::Persistence))?;
    crate::chat::apply_env(
        &mut cmd,
        tokage_core::ProviderId::CodexCli,
        env,
        &[],
        Some(proxy),
    );
    cmd.current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    crate::proc::isolate(&mut cmd);
    let mut child = permit
        .publish(|| cmd.spawn())
        .map_err(|_| fail(QueryError::Cancelled))?
        .map_err(|_| fail(QueryError::Network))?;
    let _group = crate::proc::Group::new(&child);
    let mut input = child
        .stdin
        .take()
        .ok_or_else(|| fail(QueryError::Network))?;
    let mut reader = tokio::io::BufReader::new(
        child
            .stdout
            .take()
            .ok_or_else(|| fail(QueryError::Network))?,
    );
    let work = async {
        rpc(&mut reader,&mut input,1,"initialize",serde_json::json!({"clientInfo":{"name":"tokage-usage","title":"Tokage usage","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}})).await?;
        input
            .write_all(b"{\"method\":\"initialized\"}\n")
            .await
            .map_err(|_| fail(QueryError::Network))?;
        let before = rpc(
            &mut reader,
            &mut input,
            2,
            "account/read",
            serde_json::json!({"refreshToken":false}),
        )
        .await?;
        if before["account"]["type"] != "chatgpt" {
            return Err(fail(QueryError::Authentication));
        }
        let quota = rpc(
            &mut reader,
            &mut input,
            3,
            "account/rateLimits/read",
            Value::Null,
        )
        .await?;
        let after = rpc(
            &mut reader,
            &mut input,
            4,
            "account/read",
            serde_json::json!({"refreshToken":false}),
        )
        .await?;
        if before != after || quota["accountId"].as_str() != Some(src.account_id.as_str()) {
            return Err(fail(QueryError::Identity));
        }
        if source(exe, env, salt)?.identity != *expected {
            return Err(fail(QueryError::Identity));
        }
        parse(&quota).map_err(|_| fail(QueryError::InvalidData))
    };
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        permit.run(async { Ok::<_, String>(work.await) }),
    )
    .await
    .map_err(|_| fail(QueryError::Timeout))?;
    // Normal bounded management connection: drop kills the entire process group, no thread exists.
    result.map_err(|_| fail(QueryError::Cancelled))?
}

pub fn parse(response: &Value) -> Result<QueryOutput, &'static str> {
    let mut out = QueryOutput {
        available: None,
        source: QuerySource::Cli,
        windows: vec![],
        quantities: vec![],
        display: None,
    };
    let buckets: Vec<(String, &Value)> = if let Some(map) = response
        .get("rateLimitsByLimitId")
        .and_then(Value::as_object)
        .filter(|m| !m.is_empty())
    {
        map.iter()
            .map(|(key, value)| (key.clone(), value))
            .collect()
    } else {
        let legacy = response
            .get("rateLimits")
            .filter(|v| v.is_object())
            .ok_or("Missing Codex rate limits")?;
        vec![(
            legacy
                .get("limitId")
                .and_then(Value::as_str)
                .unwrap_or("legacy")
                .into(),
            legacy,
        )]
    };
    for (id, bucket) in buckets {
        if id.is_empty()
            || id.len() > 200
            || id.chars().any(char::is_control)
            || bucket
                .get("limitId")
                .and_then(Value::as_str)
                .is_some_and(|n| n != id)
        {
            return Err("Invalid Codex bucket identifier");
        }
        for slot in ["primary", "secondary"] {
            let Some(window) = bucket.get(slot).filter(|v| !v.is_null()) else {
                continue;
            };
            let used = window
                .get("usedPercent")
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite() && (0.0..=100.0).contains(v))
                .ok_or("Invalid Codex usage percentage")?;
            let minutes = match window.get("windowDurationMins").filter(|v| !v.is_null()) {
                None => None,
                Some(v) => Some(
                    v.as_u64()
                        .and_then(|n| u32::try_from(n).ok())
                        .filter(|n| *n > 0)
                        .ok_or("Invalid Codex window duration")?,
                ),
            };
            let reset = match window.get("resetsAt").filter(|v| !v.is_null()) {
                None => None,
                Some(v) => Some(
                    v.as_i64()
                        .filter(|n| *n > 0)
                        .ok_or("Invalid Codex reset time")?,
                ),
            };
            out.windows.push(QuotaWindow {
                id: format!("{id}/{slot}"),
                scope: format!("bucket:{id}"),
                period: slot.into(),
                window_minutes: minutes,
                used_percent: Some(used),
                resets_at: reset,
            });
        }
        if let Some(credit) = bucket.get("credits") {
            // Unlimited is a flag, never an invented numeric balance or denominator.
            if credit["unlimited"] == true {
                out.windows.push(QuotaWindow {
                    id: format!("{id}/unlimited-credits"),
                    scope: format!("bucket:{id}"),
                    period: "unlimited-credits".into(),
                    window_minutes: None,
                    used_percent: None,
                    resets_at: None,
                });
            } else if let Some(balance) = credit.get("balance").and_then(Value::as_str) {
                if !decimal(balance) {
                    return Err("Invalid Codex credits balance");
                }
                out.quantities.push(Quantity {
                    id: format!("{id}/credits"),
                    unit: "credits".into(),
                    currency: None,
                    used: None,
                    limit: None,
                    remaining: Some(balance.into()),
                });
            }
        }
    }
    if out.windows.is_empty() && out.quantities.is_empty() {
        return Err("Missing Codex quota quantities");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn workspace_identity_survives_token_refresh_but_not_account_or_backend_switch() {
        let env = ShellEnv {
            vars: Default::default(),
        };
        let mut auth = json!({"auth_mode":"chatgpt","tokens":{"account_id":"workspace-a","refresh_token":"synthetic"}});
        let key = source_from(
            &auth,
            "",
            Path::new("/cli"),
            Path::new("/home"),
            &env,
            b"salt",
        )
        .unwrap()
        .identity;
        auth["tokens"]["refresh_token"] = json!("rotated");
        assert!(
            key == source_from(
                &auth,
                "",
                Path::new("/cli"),
                Path::new("/home"),
                &env,
                b"salt"
            )
            .unwrap()
            .identity
        );
        auth["tokens"]["account_id"] = json!("workspace-b");
        assert!(
            key != source_from(
                &auth,
                "",
                Path::new("/cli"),
                Path::new("/home"),
                &env,
                b"salt"
            )
            .unwrap()
            .identity
        );
        for c in [
            "model_provider='gateway'",
            "chatgpt_base_url='https://evil.invalid'",
            "profile='other'",
            "[model_providers.openai]\nbase_url='https://evil.invalid'",
            "broken = [",
        ] {
            assert!(source_from(
                &auth,
                c,
                Path::new("/cli"),
                Path::new("/home"),
                &env,
                b"salt"
            )
            .is_err());
        }
    }
    #[tokio::test]
    async fn bounded_decoder_rejects_oversize_and_wrong_rpc_frames() {
        let input = vec![b'x'; 65537];
        let mut r = tokio::io::BufReader::new(input.as_slice());
        assert_eq!(
            line(&mut r).await.unwrap_err().category,
            QueryError::InvalidData
        );
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn management_only_transport_handles_refresh_switch_errors_and_revoke() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("codex-home");
        std::fs::create_dir_all(&home).unwrap();
        let exe = dir.path().join("codex");
        std::fs::write(&exe, include_str!("../../tests/fixtures/codex-usage.py")).unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let env = ShellEnv {
            vars: [
                ("HOME".into(), dir.path().display().to_string()),
                ("CODEX_HOME".into(), home.display().to_string()),
                ("PATH".into(), "/usr/bin:/bin".into()),
            ]
            .into(),
        };
        let permission =
            crate::online_usage::OnlineUsagePermission::open(&dir.path().join("consent"));
        permission.set(true).unwrap();
        let proxy = crate::proxy::prepare_turn_proxy(
            &Default::default(),
            tokage_core::ProviderId::CodexCli,
            &env,
        )
        .await
        .unwrap();
        for (mode, expected) in [
            ("ok", None),
            ("refresh", None),
            ("switch", Some(QueryError::Identity)),
            ("mismatch", Some(QueryError::Identity)),
            ("401", Some(QueryError::Authentication)),
            ("429", Some(QueryError::RateLimited)),
            ("unsupported", Some(QueryError::InvalidData)),
            ("server-request", Some(QueryError::InvalidData)),
            ("apikey", Some(QueryError::Authentication)),
        ] {
            std::fs::write(home.join("auth.json"),json!({"auth_mode":"chatgpt","tokens":{"account_id":"synthetic-workspace","refresh_token":"synthetic"}}).to_string()).unwrap();
            std::fs::write(dir.path().join("mode"), mode).unwrap();
            let identity = resolve(&exe, &env, b"salt").unwrap();
            let out = query(
                &permission.permit().unwrap(),
                &exe,
                &env,
                b"salt",
                &identity,
                &proxy,
            )
            .await;
            match expected {
                Some(c) => assert_eq!(out.err().unwrap().category, c, "{mode}"),
                None => assert_eq!(out.unwrap().windows[0].used_percent, Some(12.5)),
            }
        }
        let frames = std::fs::read_to_string(dir.path().join("requests")).unwrap();
        for l in frames.lines() {
            let v: Value = serde_json::from_str(l).unwrap();
            assert!(matches!(
                v["method"].as_str(),
                Some("initialize" | "initialized" | "account/read" | "account/rateLimits/read")
            ));
            if v["method"] == "account/read" {
                assert_eq!(v["params"]["refreshToken"], false);
            }
        }
        assert!(!dir.path().join("unexpected").exists());
        assert_eq!(
            serde_json::from_str::<Vec<String>>(
                &std::fs::read_to_string(dir.path().join("argv")).unwrap()
            )
            .unwrap(),
            ["app-server", "-c", "mcp_servers={}", "--listen", "stdio://"]
        );
        std::fs::write(dir.path().join("mode"), "hang").unwrap();
        std::fs::write(dir.path().join("requests"), "").unwrap();
        let identity = resolve(&exe, &env, b"salt").unwrap();
        let permit = permission.permit().unwrap();
        let work = query(&permit, &exe, &env, b"salt", &identity, &proxy);
        let revoke = async {
            for _ in 0..100 {
                if std::fs::read_to_string(dir.path().join("requests"))
                    .unwrap_or_default()
                    .contains("account/rateLimits/read")
                {
                    permission.set(false).unwrap();
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            panic!("fixture never reached management query")
        };
        let (out, ()) = tokio::join!(work, revoke);
        assert_eq!(out.err().unwrap().category, QueryError::Cancelled);
        let pid = std::fs::read_to_string(dir.path().join("pid")).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        assert!(!std::process::Command::new("/bin/kill")
            .args(["-0", pid.trim()])
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success());
    }
    #[tokio::test]
    #[ignore = "installed Codex non-inference management diagnostic; not desktop acceptance"]
    async fn installed_account_query() {
        let env = crate::env::capture().await;
        let exe = which::which_in("codex", env.path(), std::env::current_dir().unwrap()).unwrap();
        let identity = resolve(&exe, &env, b"diagnostic").expect("supported current CLI account");
        let proxy = crate::proxy::prepare_turn_proxy(
            &Default::default(),
            tokage_core::ProviderId::CodexCli,
            &env,
        )
        .await
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let permission =
            crate::online_usage::OnlineUsagePermission::open(&dir.path().join("consent"));
        permission.set(true).unwrap();
        let start = std::time::Instant::now();
        let q = query(
            &permission.permit().unwrap(),
            &exe,
            &env,
            b"diagnostic",
            &identity,
            &proxy,
        )
        .await
        .map_err(|e| e.category)
        .expect("management quota");
        println!(
            "PASS Codex management: {} windows, {} quantities, {}ms; no thread/turn",
            q.windows.len(),
            q.quantities.len(),
            start.elapsed().as_millis()
        );
    }
    #[test]
    fn multi_bucket_wins_and_unknown_fields_are_ignored() {
        let out = parse(&json!({"rateLimits":{"primary":{"usedPercent":99}},"rateLimitsByLimitId":{
            "codex":{"primary":{"usedPercent":25,"windowDurationMins":300},"credits":{"balance":"10.50","unlimited":false}},
            "spark":{"secondary":{"usedPercent":0,"resetsAt":2000000000},"future":true}
        }})).unwrap();
        assert_eq!(out.windows.len(), 2);
        assert_eq!(out.windows[0].scope, "bucket:codex");
        assert_eq!(out.windows[0].used_percent, Some(25.0));
        assert!(out.windows[1].window_minutes.is_none());
        assert_eq!(out.quantities[0].remaining.as_deref(), Some("10.50"));
        assert!(out.quantities[0].limit.is_none());
    }
    #[test]
    fn missing_is_unknown_and_invalid_is_rejected() {
        assert!(parse(&json!({"rateLimits":{}})).is_err());
        assert!(parse(&json!({"rateLimits":{"primary":{"usedPercent":101}}})).is_err());
        assert!(parse(
            &json!({"rateLimits":{"primary":{"usedPercent":0,"windowDurationMins":-1}}})
        )
        .is_err());
        let unlimited =
            parse(&json!({"rateLimits":{"credits":{"unlimited":true,"balance":"0"}}})).unwrap();
        assert!(unlimited.quantities.is_empty());
        assert_eq!(unlimited.windows[0].period, "unlimited-credits");
        assert!(unlimited.windows[0].used_percent.is_none());
        let out =
            parse(&json!({"rateLimits":{"primary":null,"secondary":{"usedPercent":100}}})).unwrap();
        assert_eq!(out.windows.len(), 1);
        assert!(out.windows[0].resets_at.is_none());
        assert!(out.quantities.is_empty());
    }
}
