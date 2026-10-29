//! Read-only CLI-owned login evidence. Never renews/writes credentials or logs secrets.
use super::{contract::{QueryError, QueryFailure}, identity::{self, Identity}};
use crate::env::ShellEnv;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tokio::io::AsyncReadExt;

fn failure(category: QueryError) -> QueryFailure {
    QueryFailure { category, retry_after: None }
}

/// Absence is different from a temporarily unreadable or malformed credential source.
fn local_json(path: &Path, limit: u64) -> Result<Option<Value>, QueryFailure> {
    use std::io::Read;
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(failure(QueryError::Persistence)),
    };
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)
        .map_err(|_| failure(QueryError::Persistence))?;
    if bytes.len() as u64 > limit { return Err(failure(QueryError::InvalidData)); }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| failure(QueryError::InvalidData))?;
    if !value.is_object() { return Err(failure(QueryError::InvalidData)); }
    Ok(Some(value))
}

#[cfg(target_os = "macos")]
fn keychain_json(bytes: &[u8]) -> Result<Value, QueryFailure> {
    let text = std::str::from_utf8(bytes).map_err(|_| failure(QueryError::InvalidData))?.trim();
    let bytes = if let Some(encoded) = text.strip_prefix("go-keyring-base64:") {
        crate::usage::base64url_decode(encoded).ok_or_else(|| failure(QueryError::InvalidData))?
    } else { text.as_bytes().to_vec() };
    serde_json::from_slice(&bytes).map_err(|_| failure(QueryError::InvalidData))
}

async fn keychain(
    env: &ShellEnv,
    service: &str,
    account: Option<&str>,
) -> Result<Option<Value>, QueryFailure> {
    #[cfg(target_os = "macos")]
    {
        // OS keychains belong to the OS user, not a caller-supplied alternate HOME.
        if env.vars.get("HOME") != std::env::var("HOME").ok().as_ref() {
            return Ok(None);
        }
        let mut cmd = tokio::process::Command::new("/usr/bin/security");
        cmd.args(["find-generic-password", "-s", service]);
        if let Some(a) = account {
            cmd.args(["-a", a]);
        }
        cmd.arg("-w");
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        crate::proc::isolate(&mut cmd);
        let mut child = cmd.spawn().map_err(|_| failure(QueryError::Persistence))?;
        let _group = crate::proc::Group::new(&child);
        return tokio::time::timeout(std::time::Duration::from_secs(8), async {
            let mut b = vec![];
            child
                .stdout
                .take()
                .ok_or_else(|| failure(QueryError::Persistence))?
                .take(131073)
                .read_to_end(&mut b)
                .await
                .map_err(|_| failure(QueryError::Persistence))?;
            if b.len() > 131072 {
                return Err(failure(QueryError::InvalidData));
            }
            let status = child.wait().await.map_err(|_| failure(QueryError::Persistence))?;
            if status.code() == Some(44) {
                return Ok(None);
            }
            if !status.success() {
                return Err(failure(QueryError::Persistence));
            }
            keychain_json(&b).map(Some)
        })
        .await
        .map_err(|_| failure(QueryError::Timeout))?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (env, service, account);
        Ok(None)
    }
}
fn home(env: &ShellEnv) -> Option<PathBuf> {
    env.vars.get("HOME").map(PathBuf::from)
}
pub(crate) struct ClaudeProfile {
    pub logged_in: bool,
    pub plan: Option<String>,
    pub identity: Option<Identity>,
    pub credential_revision: String,
    pub lifecycle: super::claude_diagnostics::CredentialState,
}
pub(crate) fn claude_from(
    value: &Value,
    source: &[u8],
    exe: &Path,
    salt: &[u8],
    account: Option<&Value>,
) -> ClaudeProfile {
    let oauth = &value["claudeAiOauth"];
    let access = oauth["accessToken"].as_str().filter(|s| !s.is_empty());
    let refresh = oauth["refreshToken"].as_str().filter(|s| !s.is_empty());
    let expiry = oauth["expiresAt"].as_i64().unwrap_or(0) / 1000;
    let refresh_expiry = oauth["refreshTokenExpiresAt"]
        .as_i64()
        .filter(|t| *t > 0)
        .map(|t| t / 1000);
    let usable = access.is_some()
        && (refresh.is_some() && refresh_expiry.is_none_or(|t| t > super::now())
            || expiry > super::now());
    let identity = usable.then(|| {
        let stable = account
            .and_then(|v| v.pointer("/oauthAccount/accountUuid"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty());
        let org = account
            .and_then(|v| v.pointer("/oauthAccount/organizationUuid"))
            .and_then(Value::as_str)
            .unwrap_or("");
        Identity {
            key: identity::opaque_key(
                salt,
                &[
                    b"claude-cli-local-oauth",
                    source,
                    exe.as_os_str().as_encoded_bytes(),
                    stable
                        .unwrap_or_else(|| refresh.or(access).unwrap())
                        .as_bytes(),
                    org.as_bytes(),
                ],
            ),
        }
    });
    ClaudeProfile {
        lifecycle: super::claude_diagnostics::CredentialState::from_oauth(oauth, super::now()),
        credential_revision: identity::opaque_key(salt, &[b"claude-local-credential-revision", source, access.unwrap_or("").as_bytes(), refresh.unwrap_or("").as_bytes()]),
        logged_in: usable,
        plan: oauth["subscriptionType"].as_str().map(str::to_string),
        identity,
    }
}
pub(crate) async fn claude(exe: &Path, env: &ShellEnv, salt: &[u8]) -> Result<Option<ClaudeProfile>, QueryFailure> {
    let settings = crate::usage::claude_settings(env);
    if settings.has_token || settings.base_url.is_some() || settings.cloud.is_some() {
        return Ok(None);
    }
    // A standalone external token is a different source; don't silently borrow stored OAuth.
    if env
        .vars
        .get("CLAUDE_CODE_OAUTH_TOKEN")
        .is_some_and(|v| !v.trim().is_empty())
    {
        return Ok(None);
    }
    let root = env
        .vars
        .get("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| home(env).map(|h| h.join(".claude")))
        .ok_or_else(|| failure(QueryError::Identity))?;
    let custom = env
        .vars
        .get("CLAUDE_CONFIG_DIR")
        .is_some_and(|s| !s.is_empty());
    let account_path = if custom {
        root.join(".claude.json")
    } else {
        home(env).ok_or_else(|| failure(QueryError::Identity))?.join(".claude.json")
    };
    let account = local_json(&account_path, 1_048_576)?;
    if !custom {
        match keychain(env, "Claude Code-credentials", None).await {
            Ok(Some(v)) => {
                if !v["claudeAiOauth"].is_object() { return Err(failure(QueryError::InvalidData)); }
                return Ok(Some(claude_from(
                    &v,
                    b"os-keychain-default",
                    exe,
                    salt,
                    account.as_ref(),
                )))
            }
            Err(error) => return Err(error),
            Ok(None) => {}
        }
    }
    let Some(v) = local_json(&root.join(".credentials.json"), 131072)? else { return Ok(None); };
    if !v["claudeAiOauth"].is_object() { return Err(failure(QueryError::InvalidData)); }
    Ok(Some(claude_from(
        &v,
        root.as_os_str().as_encoded_bytes(),
        exe,
        salt,
        account.as_ref(),
    )))
}
fn gemini_from(value: &Value, exe: &Path, salt: &[u8]) -> Option<Identity> {
    if value["auth_method"] != "consumer" {
        return None;
    }
    let _refresh = value["token"]["refresh_token"]
        .as_str()
        .filter(|s| !s.is_empty())?;
    let jwt = value["id_token"].as_str()?;
    let payload = jwt.split('.').nth(1)?;
    let claims: Value = serde_json::from_slice(&crate::usage::base64url_decode(payload)?).ok()?;
    if !matches!(
        claims["iss"].as_str(),
        Some("https://accounts.google.com" | "accounts.google.com")
    ) {
        return None;
    }
    let subject = claims["sub"].as_str().filter(|s| !s.is_empty())?;
    Some(Identity {
        key: identity::opaque_key(
            salt,
            &[
                b"agy-consumer-keychain-profile",
                exe.as_os_str().as_encoded_bytes(),
                subject.as_bytes(),
            ],
        ),
    })
}
pub(crate) async fn antigravity(exe: &Path, env: &ShellEnv, salt: &[u8]) -> Option<Identity> {
    antigravity_observe(exe,env,salt).await.ok().flatten().map(|o|o.identity)
}
pub(crate) async fn antigravity_observe(exe: &Path, env: &ShellEnv, salt: &[u8]) -> Result<Option<identity::Observation>, QueryFailure> {
    if crate::providers::antigravity_api_key_mode(env)
        || [
            "AGY_ACCOUNT",
            "AGY_ADC_AUTH",
            "GOOGLE_APPLICATION_CREDENTIALS",
        ]
        .iter()
        .any(|k| {
            env.vars
                .get(*k)
                .is_some_and(|v| !v.is_empty() && v != "false" && v != "0")
        })
    {
        return Ok(None);
    }
    let config = home(env).ok_or_else(|| failure(QueryError::Identity))?.join(".gemini/config/config.json");
    if local_json(&config, 1048576)?
        .is_some_and(|v| v["userSettings"]["enableAdc"] == true)
    {
        return Ok(None);
    }
    let Some(value)=keychain(env,"gemini",Some("antigravity")).await? else { return Ok(None); };
    if value["auth_method"] != "consumer" { return Ok(None); }
    let identity=gemini_from(&value,exe,salt).ok_or_else(|| failure(QueryError::InvalidData))?;
    Ok(Some(identity::Observation { claude_lifecycle: None, identity, source_login:None,
        credential_revision:Some(identity::opaque_key(salt,&[b"agy-local-credential-revision", value["token"]["refresh_token"].as_str().unwrap().as_bytes(), value["token"]["access_token"].as_str().unwrap_or("").as_bytes()])) }))
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn local_source_missing_unreadable_invalid_and_oversize_are_distinct() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credentials.json");
        assert!(local_json(&path, 64).unwrap().is_none());
        std::fs::write(&path, br#"{"private":"synthetic","broken":"#).unwrap();
        assert_eq!(local_json(&path, 64).unwrap_err().category, QueryError::InvalidData);
        std::fs::write(&path, b"null").unwrap();
        assert_eq!(local_json(&path, 64).unwrap_err().category, QueryError::InvalidData);
        std::fs::write(&path, vec![b' '; 65]).unwrap();
        assert_eq!(local_json(&path, 64).unwrap_err().category, QueryError::InvalidData);
        assert_eq!(local_json(dir.path(), 64).unwrap_err().category, QueryError::Persistence);
        std::fs::write(&path, br#"{"ok":true}"#).unwrap();
        assert_eq!(local_json(&path, 64).unwrap().unwrap()["ok"], true);
    }
    #[tokio::test]
    async fn agy_config_read_failure_is_not_missing_identity_or_api_key_mode() {
        let dir = tempfile::tempdir().unwrap();
        let env = ShellEnv { vars: [("HOME".into(), dir.path().to_string_lossy().into_owned())].into() };
        let path = dir.path().join(".gemini/config/config.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{interrupted write").unwrap();
        let error = antigravity_observe(Path::new("/fixture/agy"), &env, b"salt").await.err().unwrap();
        assert_eq!(error.category, QueryError::InvalidData);
        std::fs::write(&path, br#"{"userSettings":{"enableAdc":true}}"#).unwrap();
        assert!(antigravity_observe(Path::new("/fixture/agy"), &env, b"salt").await.unwrap().is_none());
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn keychain_decode_errors_never_include_credential_text() {
        for raw in [b"go-keyring-base64:not valid".as_slice(), b"{synthetic-secret", &[0xff]] {
            let error = keychain_json(raw).unwrap_err();
            assert_eq!(error.category, QueryError::InvalidData);
            assert!(!format!("{error:?}").contains("synthetic-secret"));
        }
        assert_eq!(keychain_json(br#"{"ok":true}"#).unwrap()["ok"], true);
    }
    #[test]
    fn expired_access_with_live_refresh_grant_does_not_require_relogin() {
        let v = json!({"claudeAiOauth":{"accessToken":"synthetic-access","refreshToken":"synthetic-refresh","expiresAt":1,"subscriptionType":"max"}});
        let a = claude_from(&v, b"source", Path::new("/cli"), b"salt", None);
        assert!(a.logged_in);
        assert_eq!(a.plan.as_deref(), Some("max"));
        let mut rotated = v.clone();
        rotated["claudeAiOauth"]["accessToken"] = json!("new-access");
        assert!(
            a.identity
                == claude_from(&rotated, b"source", Path::new("/cli"), b"salt", None).identity
        );
        assert_ne!(a.credential_revision, claude_from(&rotated,b"source",Path::new("/cli"),b"salt",None).credential_revision);
        let cleared = json!({"claudeAiOauth":{"accessToken":"","refreshToken":"","expiresAt":0}});
        assert!(!claude_from(&cleared, b"source", Path::new("/cli"), b"s", None).logged_in);
    }
    #[test]
    fn claude_account_identity_survives_grant_rotation_but_not_account_switch() {
        let mut v = json!({"claudeAiOauth":{"accessToken":"synthetic","refreshToken":"synthetic","expiresAt":0}});
        let mut account = json!({"oauthAccount":{"accountUuid":"account-a","organizationUuid":"org"}});
        let a = claude_from(&v, b"source", Path::new("/cli"), b"salt", Some(&account)).identity;
        v["claudeAiOauth"]["refreshToken"] = json!("rotated");
        assert!(a == claude_from(&v, b"source", Path::new("/cli"), b"salt", Some(&account)).identity);
        account["oauthAccount"]["accountUuid"] = json!("account-b");
        assert!(a != claude_from(&v, b"source", Path::new("/cli"), b"salt", Some(&account)).identity);
    }
    #[test]
    fn expired_refresh_and_access_are_not_kept_alive_by_display_cache() {
        let v = json!({"claudeAiOauth":{"accessToken":"synthetic","refreshToken":"synthetic","expiresAt":1,"refreshTokenExpiresAt":1}});
        assert!(!claude_from(&v, b"source", Path::new("/cli"), b"salt", None).logged_in);
    }
    #[test]
    fn agy_identity_survives_token_rotation_for_the_same_google_subject() {
        // Base64 of {"iss":"accounts.google.com","sub":"synthetic-user"}.
        let jwt = "x.eyJpc3MiOiJhY2NvdW50cy5nb29nbGUuY29tIiwic3ViIjoic3ludGhldGljLXVzZXIifQ.x";
        let v = json!({"auth_method":"consumer","id_token":jwt,"token":{"refresh_token":"synthetic","access_token":"old"}});
        let a = gemini_from(&v, Path::new("/cli"), b"s").unwrap();
        let mut b = v.clone();
        b["token"]["access_token"] = json!("new");
        assert!(Some(a.clone()) == gemini_from(&b, Path::new("/cli"), b"s"));
        b["token"]["refresh_token"] = json!("different");
        assert!(Some(a) == gemini_from(&b, Path::new("/cli"), b"s"));
    }
}
