//! Explicit V2 DeepSeek wallet, independent of session/task model selection.
use super::{
    contract::*,
    http,
    identity::{self, Identity},
};
use crate::{env::ShellEnv, online_usage::QueryPermit, proxy::PreparedTurnProxy};
use serde_json::Value;
use std::{path::Path, process::Stdio, time::Duration};
use tokage_core::ProviderId;
use tokio::io::AsyncReadExt;
fn failure(category: QueryError) -> QueryFailure {
    QueryFailure {
        category,
        retry_after: None,
    }
}

// Not Debug/Serialize: exported credentials exist only in native memory for this query.
struct Source {
    identity: Identity,
    credential: http::UsageCredential,
}
async fn cli(exe: &Path, env: &ShellEnv, args: &[&str]) -> Result<Value, QueryFailure> {
    let mut command = crate::providers::auth_command(ProviderId::Opencode, exe, args, env)
        .ok_or_else(|| failure(QueryError::Identity))?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    crate::proc::isolate(&mut command);
    let mut child = command.spawn().map_err(|_| failure(QueryError::Network))?;
    let _group = crate::proc::Group::new(&child);
    tokio::time::timeout(Duration::from_secs(8), async {
        let mut data = Vec::new();
        child
            .stdout
            .take()
            .ok_or_else(|| failure(QueryError::InvalidData))?
            .take(1_048_577)
            .read_to_end(&mut data)
            .await
            .map_err(|_| failure(QueryError::Network))?;
        if data.len() > 1_048_576 {
            return Err(failure(QueryError::InvalidData));
        }
        if !child
            .wait()
            .await
            .map_err(|_| failure(QueryError::Network))?
            .success()
        {
            // A failed management process is not proof that the saved CLI login is invalid.
            return Err(failure(QueryError::Network));
        }
        serde_json::from_slice(&data).map_err(|_| failure(QueryError::InvalidData))
    })
    .await
    .map_err(|_| failure(QueryError::Timeout))?
}
fn official(value: &Value) -> bool {
    value.as_str().is_some_and(|s| {
        matches!(
            s,
            "https://api.deepseek.com"
                | "https://api.deepseek.com/"
                | "https://api.deepseek.com/v1"
                | "https://api.deepseek.com/v1/"
        )
    })
}
fn settings(value: &Value) -> bool {
    let Some(map) = value.as_object() else {
        return false;
    };
    map.keys().all(|key| key == "baseURL") && official(&value["baseURL"])
}
fn eligible(provider: &Value) -> bool {
    provider["id"] == "deepseek"
        && provider["integrationID"] == "deepseek"
        && provider["package"] == "@opencode/ai/providers/deepseek"
        && settings(&provider["settings"])
        && ["headers", "body"].iter().all(|key| {
            provider
                .get(key)
                .is_none_or(|v| v.is_null() || v.as_object().is_some_and(|o| o.is_empty()))
        })
}
fn source_from(
    provider: &Value,
    accounts: &Value,
    exe: &Path,
    salt: &[u8],
) -> Result<Source, QueryFailure> {
    let provider = &provider["data"];
    if !eligible(provider) {
        return Err(failure(QueryError::Identity));
    }
    let accounts = accounts
        .as_array()
        .ok_or_else(|| failure(QueryError::InvalidData))?;
    if accounts.iter().any(|v| v["integrationID"] != "deepseek") {
        return Err(failure(QueryError::Identity));
    }
    let active: Vec<_> = accounts.iter().filter(|v| v["active"] == true).collect();
    if active.len() != 1 || active[0]["value"]["type"] != "key" {
        return Err(failure(QueryError::Identity));
    }
    let id = active[0]["id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| failure(QueryError::Identity))?;
    let key = active[0]["value"]["key"]
        .as_str()
        .ok_or_else(|| failure(QueryError::Authentication))?;
    let credential = http::UsageCredential::bearer(key.into())?;
    let identity = Identity {
        key: identity::opaque_key(
            salt,
            &[
                b"opencode-v2-explicit-deepseek-wallet",
                exe.as_os_str().as_encoded_bytes(),
                id.as_bytes(),
                key.as_bytes(),
                provider["settings"]["baseURL"].as_str().unwrap().as_bytes(),
            ],
        ),
    };
    Ok(Source {
        identity,
        credential,
    })
}
async fn source(exe: &Path, env: &ShellEnv, salt: &[u8]) -> Result<Source, QueryFailure> {
    // Query the explicitly named connected backend, never a default or arbitrary session.
    let provider = cli(
        exe,
        env,
        &["api", "provider.get", "--param", "providerID=deepseek"],
    )
    .await?;
    if !eligible(&provider["data"]) {
        return Err(failure(QueryError::Identity));
    }
    // Explicit provider target; never export every integration or open the database.
    let accounts = cli(exe, env, &["auth", "export", "deepseek"]).await?;
    source_from(&provider, &accounts, exe, salt)
}
pub(crate) async fn resolve_checked(exe: &Path, env: &ShellEnv, salt: &[u8]) -> Result<Identity, QueryFailure> {
    source(exe, env, salt).await.map(|s| s.identity)
}
pub(crate) async fn resolve(exe: &Path, env: &ShellEnv, salt: &[u8]) -> Option<Identity> {
    resolve_checked(exe, env, salt).await.ok()
}
pub(crate) async fn query(
    permit: &QueryPermit,
    exe: &Path,
    env: &ShellEnv,
    salt: &[u8],
    expected: &Identity,
    proxy: &PreparedTurnProxy,
) -> Result<QueryOutput, QueryFailure> {
    let source = source(exe, env, salt).await?;
    if &source.identity != expected {
        return Err(failure(QueryError::Identity));
    }
    let value = http::get(
        permit,
        http::UsageHttpTarget::DeepSeekBalance,
        source.credential,
        env,
        proxy,
    )
    .await?;
    parse(&value)
}
pub fn parse(value: &Value) -> Result<QueryOutput, QueryFailure> {
    let available = value["is_available"]
        .as_bool()
        .ok_or_else(|| failure(QueryError::InvalidData))?;
    let rows = value["balance_infos"]
        .as_array()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| failure(QueryError::InvalidData))?;
    let mut quantities = vec![];
    let mut seen = std::collections::HashSet::new();
    for row in rows {
        let currency = row["currency"]
            .as_str()
            .filter(|s| s.len() == 3 && s.bytes().all(|b| b.is_ascii_uppercase()))
            .ok_or_else(|| failure(QueryError::InvalidData))?;
        if !seen.insert(currency) {
            return Err(failure(QueryError::InvalidData));
        }
        for field in ["total_balance", "granted_balance", "topped_up_balance"] {
            let amount = row[field]
                .as_str()
                .filter(|s| decimal(s))
                .ok_or_else(|| failure(QueryError::InvalidData))?;
            quantities.push(Quantity {
                id: format!("{currency}/{field}"),
                unit: "currency".into(),
                currency: Some(currency.into()),
                used: None,
                limit: None,
                remaining: Some(amount.into()),
            });
        }
    }
    Ok(QueryOutput {
        available: Some(available),
        source: QuerySource::Http,
        windows: vec![],
        quantities,
        display: None,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn provider() -> Value {
        json!({"data":{"id":"deepseek","integrationID":"deepseek","package":"@opencode/ai/providers/deepseek","settings":{"baseURL":"https://api.deepseek.com"}}})
    }
    fn accounts() -> Value {
        json!([{"id":"fixture","integrationID":"deepseek","active":true,"value":{"type":"key","key":"synthetic"}}])
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn wallet_never_reads_model_defaults_or_sessions_or_unrelated_credentials() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("cli");
        std::fs::write(dir.path().join("provider.json"), provider().to_string()).unwrap();
        std::fs::write(dir.path().join("accounts.json"), accounts().to_string()).unwrap();
        std::fs::write(&exe,r#"#!/bin/sh
base=${0%/*}
if [ "$1" = api ] && [ "$2" = provider.get ] && [ "$3" = --param ] && [ "$4" = providerID=deepseek ]; then
 /bin/cat "$base/provider.json"
elif [ "$1" = auth ] && [ "$2" = export ] && [ "$3" = deepseek ]; then
 /bin/cat "$base/accounts.json"
else
 : > "$base/unexpected"
 exit 1
fi
"#).unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o700)).unwrap();
        let env = ShellEnv {
            vars: [("HOME".into(), dir.path().to_string_lossy().into_owned())]
                .into_iter()
                .collect(),
        };
        assert!(source(&exe, &env, b"synthetic-salt").await.is_ok());
        assert!(!dir.path().join("unexpected").exists());
        let mut gateway = provider();
        gateway["data"]["settings"]["baseURL"] = json!("https://gateway.invalid");
        std::fs::write(dir.path().join("provider.json"), gateway.to_string()).unwrap();
        // Remove the credential fixture: provider rejection must precede credential access.
        std::fs::remove_file(dir.path().join("accounts.json")).unwrap();
        assert_eq!(
            source(&exe, &env, b"s").await.err().unwrap().category,
            QueryError::Identity
        );
    }
    #[test]
    fn wallet_binds_unique_active_account_and_rotation_without_model_prefixes() {
        let a = source_from(&provider(), &accounts(), Path::new("/cli"), b"salt")
            .unwrap()
            .identity;
        let mut changed = accounts();
        changed[0]["value"]["key"] = json!("rotated");
        assert!(
            a != source_from(&provider(), &changed, Path::new("/cli"), b"salt")
                .unwrap()
                .identity
        );
        let mut inactive = accounts();
        inactive[0]["active"] = json!(false);
        assert!(source_from(&provider(), &inactive, Path::new("/cli"), b"salt").is_err());
        let mut other = provider();
        other["data"]["integrationID"] = json!("opencode");
        assert!(source_from(&other, &accounts(), Path::new("/cli"), b"salt").is_err());
    }
    #[test]
    fn gateway_override_and_ambiguous_accounts_are_rejected() {
        for url in [
            "http://api.deepseek.com",
            "https://api.deepseek.com.evil.test",
            "https://api.deepseek.com@evil.test",
            "https://api.deepseek.com?key=x",
        ] {
            let mut p = provider();
            p["data"]["settings"]["baseURL"] = json!(url);
            assert!(source_from(&p, &accounts(), Path::new("/cli"), b"s").is_err());
        }
        let mut p = provider();
        p["data"]["settings"]["apiKey"] = json!("override");
        assert!(source_from(&p, &accounts(), Path::new("/cli"), b"s").is_err());
        let mut a = accounts();
        a.as_array_mut().unwrap().push(accounts()[0].clone());
        assert!(source_from(&provider(), &a, Path::new("/cli"), b"s").is_err());
    }
    #[test]
    fn wallets_keep_precision_currency_and_availability_without_percentage() {
        let out=parse(&json!({"is_available":false,"balance_infos":[{"currency":"CNY","total_balance":"000.0100","granted_balance":"0.0000","topped_up_balance":"000.0100"},{"currency":"USD","total_balance":"2.50","granted_balance":"0","topped_up_balance":"2.50"}]})).unwrap();
        assert_eq!(out.available, Some(false));
        assert!(out.windows.is_empty());
        assert_eq!(out.quantities.len(), 6);
        assert_eq!(out.quantities[0].remaining.as_deref(), Some("000.0100"));
        assert!(out
            .quantities
            .iter()
            .all(|q| q.limit.is_none() && q.used.is_none()));
    }
    #[test]
    fn invalid_or_incomplete_wallet_is_not_zero_balance() {
        for row in [
            json!({"currency":"cny","total_balance":"1","granted_balance":"0","topped_up_balance":"1"}),
            json!({"currency":"CNY","total_balance":1}),
            json!({"currency":"CNY","total_balance":"-1","granted_balance":"0","topped_up_balance":"0"}),
        ] {
            assert!(parse(&json!({"is_available":true,"balance_infos":[row]})).is_err());
        }
        assert!(parse(&json!({"balance_infos":[]})).is_err());
    }
}
