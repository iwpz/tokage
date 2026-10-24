//! Read-only CLI-owned login evidence. Never renews/writes credentials or logs secrets.
use super::identity::{self, Identity};
use crate::env::ShellEnv;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tokio::io::AsyncReadExt;

async fn keychain(
    env: &ShellEnv,
    service: &str,
    account: Option<&str>,
) -> Result<Option<Value>, ()> {
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
        let mut child = cmd.spawn().map_err(|_| ())?;
        let _group = crate::proc::Group::new(&child);
        return tokio::time::timeout(std::time::Duration::from_secs(8), async {
            let mut b = vec![];
            child
                .stdout
                .take()
                .ok_or(())?
                .take(131073)
                .read_to_end(&mut b)
                .await
                .map_err(|_| ())?;
            if b.len() > 131072 {
                return Err(());
            }
            let status = child.wait().await.map_err(|_| ())?;
            if status.code() == Some(44) {
                return Ok(None);
            }
            if !status.success() {
                return Err(());
            }
            let s = std::str::from_utf8(&b).map_err(|_| ())?.trim();
            let bytes = if let Some(s) = s.strip_prefix("go-keyring-base64:") {
                crate::usage::base64url_decode(s).ok_or(())?
            } else {
                s.as_bytes().to_vec()
            };
            serde_json::from_slice(&bytes).map(Some).map_err(|_| ())
        })
        .await
        .map_err(|_| ())?;
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
        logged_in: usable,
        plan: oauth["subscriptionType"].as_str().map(str::to_string),
        identity,
    }
}
pub(crate) async fn claude(exe: &Path, env: &ShellEnv, salt: &[u8]) -> Option<ClaudeProfile> {
    let settings = crate::usage::claude_settings(env);
    if settings.has_token || settings.base_url.is_some() || settings.cloud.is_some() {
        return None;
    }
    // A standalone external token is a different source; don't silently borrow stored OAuth.
    if env
        .vars
        .get("CLAUDE_CODE_OAUTH_TOKEN")
        .is_some_and(|v| !v.trim().is_empty())
    {
        return None;
    }
    let root = env
        .vars
        .get("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| home(env).map(|h| h.join(".claude")))?;
    let custom = env
        .vars
        .get("CLAUDE_CONFIG_DIR")
        .is_some_and(|s| !s.is_empty());
    let account_path = if custom {
        root.join(".claude.json")
    } else {
        home(env)?.join(".claude.json")
    };
    let account = super::read_bounded(&account_path, 1_048_576)
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
    if !custom {
        match keychain(env, "Claude Code-credentials", None).await {
            Ok(Some(v)) => {
                return Some(claude_from(
                    &v,
                    b"os-keychain-default",
                    exe,
                    salt,
                    account.as_ref(),
                ))
            }
            Err(()) => return None,
            Ok(None) => {}
        }
    }
    let bytes = super::read_bounded(&root.join(".credentials.json"), 131072)?;
    let v: Value = serde_json::from_slice(&bytes).ok()?;
    Some(claude_from(
        &v,
        root.as_os_str().as_encoded_bytes(),
        exe,
        salt,
        account.as_ref(),
    ))
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
        return None;
    }
    let config = home(env)?.join(".gemini/config/config.json");
    if super::read_bounded(&config, 1048576)
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .is_some_and(|v| v["userSettings"]["enableAdc"] == true)
    {
        return None;
    }
    gemini_from(
        &keychain(env, "gemini", Some("antigravity")).await.ok()??,
        exe,
        salt,
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
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
