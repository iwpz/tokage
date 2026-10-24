//! Named official DeepSeek wallet from Reasonix's documented global credential store.
//! Never falls back to project/shell keys or infers a task's billing source.
use super::{
    contract::*,
    http,
    identity::{self, Identity},
};
use crate::{env::ShellEnv, online_usage::QueryPermit, proxy::PreparedTurnProxy};
use std::path::PathBuf;
fn fail(category: QueryError) -> QueryFailure {
    QueryFailure {
        category,
        retry_after: None,
    }
}
struct Source {
    identity: Identity,
    credential: http::UsageCredential,
}
fn dotenv(text: &str, name: &str) -> Option<String> {
    let mut found = None;
    for line in text.lines() {
        let line = line.trim().strip_prefix("export ").unwrap_or(line.trim());
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != name {
            continue;
        }
        if found.is_some() {
            return None;
        }
        let value = value.trim();
        if value.len() == 1 && matches!(value, "\"" | "'") { return None; }
        let value = if (value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\''))
        {
            &value[1..value.len() - 1]
        } else {
            value
        };
        // Fail closed for shell expansions/multiline escapes rather than implementing a different resolver.
        if value.is_empty()
            || value
                .chars()
                .any(|c| c.is_whitespace() || matches!(c, '\\' | '$' | '"' | '\''))
        {
            return None;
        }
        found = Some(value.to_string());
    }
    found
}
fn source_from(config: &str, keys: &str, root: &str, salt: &[u8]) -> Result<Source, QueryFailure> {
    let value: toml::Value = toml::from_str(config).map_err(|_| fail(QueryError::InvalidData))?;
    let providers = value
        .get("providers")
        .and_then(|v| v.as_array())
        .ok_or_else(|| fail(QueryError::Identity))?;
    let rows: Vec<_> = providers
        .iter()
        .filter(|v| v.get("name").and_then(|s| s.as_str()) == Some("deepseek"))
        .collect();
    if rows.len() != 1 {
        return Err(fail(QueryError::Identity));
    }
    let p = rows[0];
    let field = |k: &str| p.get(k).and_then(|v| v.as_str());
    if !matches!(field("kind"), Some("openai" | "anthropic"))
        || !matches!(
            field("base_url"),
            Some(
                "https://api.deepseek.com"
                    | "https://api.deepseek.com/"
                    | "https://api.deepseek.com/v1"
                    | "https://api.deepseek.com/anthropic"
            )
        )
        || field("balance_url") != Some("https://api.deepseek.com/user/balance")
        || ["headers", "extra_headers", "api_key", "auth_header"]
            .iter()
            .any(|k| p.get(k).is_some())
    {
        return Err(fail(QueryError::Identity));
    }
    let name = field("api_key_env")
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
        .ok_or_else(|| fail(QueryError::Identity))?;
    let key = dotenv(keys, name).ok_or_else(|| fail(QueryError::Authentication))?;
    let identity = Identity {
        key: identity::opaque_key(
            salt,
            &[
                b"reasonix-explicit-deepseek-wallet",
                root.as_bytes(),
                name.as_bytes(),
                key.as_bytes(),
                field("base_url").unwrap().as_bytes(),
            ],
        ),
    };
    Ok(Source {
        identity,
        credential: http::UsageCredential::bearer(key)?,
    })
}
fn source(env: &ShellEnv, salt: &[u8]) -> Result<Source, QueryFailure> {
    let root = env
        .vars
        .get("REASONIX_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            env.vars
                .get("HOME")
                .map(|s| PathBuf::from(s).join(".reasonix"))
        })
        .ok_or_else(|| fail(QueryError::Identity))?;
    let read = |name: &str| {
        super::read_bounded(&root.join(name), 1_048_576)
            .and_then(|b| String::from_utf8(b).ok())
            .ok_or_else(|| fail(QueryError::Identity))
    };
    source_from(
        &read("config.toml")?,
        &read(".env")?,
        &root.to_string_lossy(),
        salt,
    )
}
pub(crate) fn resolve(env: &ShellEnv, salt: &[u8]) -> Option<Identity> {
    source(env, salt).ok().map(|s| s.identity)
}
pub(crate) async fn query(
    permit: &QueryPermit,
    env: &ShellEnv,
    salt: &[u8],
    expected: &Identity,
    proxy: &PreparedTurnProxy,
) -> Result<QueryOutput, QueryFailure> {
    let s = source(env, salt)?;
    if &s.identity != expected {
        return Err(fail(QueryError::Identity));
    }
    let result = http::get(
        permit,
        http::UsageHttpTarget::DeepSeekBalance,
        s.credential,
        env,
        proxy,
    )
    .await?;
    super::opencode::parse(&result)
}
#[cfg(test)]
mod tests {
    use super::*;
    const CONFIG:&str="[[providers]]\nname='deepseek'\nkind='anthropic'\nbase_url='https://api.deepseek.com/anthropic'\nbalance_url='https://api.deepseek.com/user/balance'\napi_key_env='DS_KEY'\n";
    #[test]
    fn only_uses_global_store_and_detects_key_rotation() {
        let a = source_from(CONFIG, "DS_KEY=synthetic", "/root", b"s")
            .unwrap()
            .identity;
        assert!(
            a != source_from(CONFIG, "export DS_KEY='rotated'", "/root", b"s")
                .unwrap()
                .identity
        );
        assert!(source_from(CONFIG, "OTHER_KEY=present", "/root", b"s").is_err());
        assert!(dotenv("DS_KEY=x\nDS_KEY=y", "DS_KEY").is_none());
    }
    #[test]
    fn rejects_gateway_and_shell_expansion_in_credentials() {
        assert!(source_from(
            &CONFIG.replace(
                "https://api.deepseek.com/anthropic",
                "https://gateway.invalid"
            ),
            "DS_KEY=x",
            "/root",
            b"s"
        )
        .is_err());
        assert!(source_from(CONFIG, "DS_KEY=$SHELL_KEY", "/root", b"s").is_err());
        assert!(source_from(CONFIG, "# reasonix-cleared DS_KEY", "/root", b"s").is_err());
    }
}
