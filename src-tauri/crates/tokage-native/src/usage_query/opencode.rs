//! V2 default-model DeepSeek wallet. CLI owns credential selection and storage.
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
            return Err(failure(QueryError::Authentication));
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
fn eligible(model: &Value, provider: &Value) -> bool {
    let package = "@opencode/ai/providers/deepseek";
    !(model["providerID"] != "deepseek"
        || provider["id"] != "deepseek"
        || provider["integrationID"] != "deepseek"
        || model["package"] != package
        || provider["package"] != package
        || !settings(&model["settings"])
        || !settings(&provider["settings"])
        || [model, provider].iter().any(|v| {
            ["headers", "body"].iter().any(|key| {
                v.get(key)
                    .is_some_and(|v| !v.is_null() && v.as_object().is_none_or(|o| !o.is_empty()))
            })
        }))
}
fn source_from(
    model: &Value,
    provider: &Value,
    accounts: &Value,
    exe: &Path,
    salt: &[u8],
) -> Result<Source, QueryFailure> {
    let model = &model["data"];
    let provider = &provider["data"];
    if !eligible(model, provider) {
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
                b"opencode-v2-deepseek-default",
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
    let model = cli(exe, env, &["api", "model.default"]).await?;
    // Reject other backends before asking the CLI for any credential.
    if model["data"]["providerID"] != "deepseek" {
        return Err(failure(QueryError::Identity));
    }
    let provider = cli(
        exe,
        env,
        &["api", "provider.get", "--param", "providerID=deepseek"],
    )
    .await?;
    if !eligible(&model["data"], &provider["data"]) {
        return Err(failure(QueryError::Identity));
    }
    // Explicit provider target; never export every integration or open the database.
    let accounts = cli(exe, env, &["auth", "export", "deepseek"]).await?;
    source_from(&model, &provider, &accounts, exe, salt)
}
pub(crate) async fn resolve(exe: &Path, env: &ShellEnv, salt: &[u8]) -> Option<Identity> {
    source(exe, env, salt).await.ok().map(|s| s.identity)
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
    fn model() -> Value {
        json!({"data":{"providerID":"deepseek","package":"@opencode/ai/providers/deepseek","settings":{"baseURL":"https://api.deepseek.com"}}})
    }
    fn provider() -> Value {
        json!({"data":{"id":"deepseek","integrationID":"deepseek","package":"@opencode/ai/providers/deepseek","settings":{"baseURL":"https://api.deepseek.com"}}})
    }
    fn accounts() -> Value {
        json!([{"id":"fixture","integrationID":"deepseek","active":true,"value":{"type":"key","key":"synthetic"}}])
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn cli_rejects_free_model_before_exporting_any_credential() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("cli");
        std::fs::write(
            &exe,
            r#"#!/bin/sh
if [ "$1" = api ] && [ "$2" = model.default ]; then
  : > "$0.read-model"
  printf '%s\n' '{"data":{"providerID":"opencode"}}'
else
  : > "$0.unexpected"
  exit 1
fi
"#,
        )
        .unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o700)).unwrap();
        let env = ShellEnv {
            vars: [("HOME".into(), dir.path().to_string_lossy().into_owned())]
                .into_iter()
                .collect(),
        };
        let result = source(&exe, &env, b"synthetic-salt").await;
        assert_eq!(result.err().unwrap().category, QueryError::Identity);
        assert!(dir.path().join("cli.read-model").exists());
        assert!(!dir.path().join("cli.unexpected").exists());
    }
    #[test]
    fn source_is_active_cli_account_not_environment_or_model_prefix() {
        let a = source_from(
            &model(),
            &provider(),
            &accounts(),
            Path::new("/cli"),
            b"salt",
        )
        .unwrap()
        .identity;
        let mut changed = accounts();
        changed[0]["value"]["key"] = json!("rotated");
        assert!(
            a != source_from(&model(), &provider(), &changed, Path::new("/cli"), b"salt")
                .unwrap()
                .identity
        );
        let mut free = model();
        free["data"]["providerID"] = json!("opencode");
        assert!(source_from(&free, &provider(), &accounts(), Path::new("/cli"), b"salt").is_err());
        let mut inactive = accounts();
        inactive[0]["active"] = json!(false);
        assert!(source_from(&model(), &provider(), &inactive, Path::new("/cli"), b"salt").is_err());
    }
    #[test]
    fn gateway_alias_override_and_ambiguous_accounts_are_rejected() {
        for url in [
            "http://api.deepseek.com",
            "https://api.deepseek.com.evil.test",
            "https://api.deepseek.com@evil.test",
            "https://api.deepseek.com?key=x",
        ] {
            let mut p = provider();
            p["data"]["settings"]["baseURL"] = json!(url);
            assert!(source_from(&model(), &p, &accounts(), Path::new("/cli"), b"s").is_err());
        }
        let mut m = model();
        m["data"]["settings"]["apiKey"] = json!("override");
        assert!(source_from(&m, &provider(), &accounts(), Path::new("/cli"), b"s").is_err());
        let mut a = accounts();
        a.as_array_mut().unwrap().push(accounts()[0].clone());
        assert!(source_from(&model(), &provider(), &a, Path::new("/cli"), b"s").is_err());
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
