//! Claude Code owns login, refresh, and secure storage. Read only its public status output.
use super::{
    contract::{QueryError, QueryFailure},
    identity::{self, Identity, Observation},
};
use crate::{env::ShellEnv, providers};
use serde::Deserialize;
use std::{path::Path, time::Duration};
use tokage_core::ProviderId;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LoginStatus {
    pub logged_in: bool,
    pub auth_method: Option<String>,
    pub subscription_type: Option<String>,
    account_uuid: Option<String>,
    email: Option<String>,
    #[serde(alias = "organizationId", alias = "organizationUuid")]
    org_id: Option<String>,
    config_directory: Option<String>,
}

fn failure(category: QueryError) -> QueryFailure {
    QueryFailure {
        category,
        retry_after: None,
    }
}

fn parse_status(ok: bool, output: &str) -> Result<LoginStatus, QueryFailure> {
    if let Some((start, end)) = output
        .find('{')
        .zip(output.rfind('}'))
        .filter(|(start, end)| start < end)
    {
        if let Ok(status) = serde_json::from_str::<LoginStatus>(&output[start..=end]) {
            let diagnostic = format!("{}\n{}", &output[..start], &output[end + 1..]);
            let error = QueryFailure::cli(&diagnostic);
            if error.category == QueryError::Authentication || (!ok && status.logged_in) {
                return Err(error);
            }
            return Ok(status);
        }
    }
    let error = QueryFailure::cli(output);
    if ok && error.category != QueryError::Authentication {
        Err(failure(QueryError::InvalidData))
    } else {
        Err(error)
    }
}

pub(crate) async fn login_status(exe: &Path, env: &ShellEnv) -> Result<LoginStatus, QueryFailure> {
    // Avoid competing CLI refresh attempts from status scans and quota checks.
    static STATUS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = tokio::time::timeout(Duration::from_secs(15), STATUS.lock())
        .await
        .map_err(|_| failure(QueryError::Timeout))?;
    let (ok, output) =
        providers::run_auth_checked(ProviderId::ClaudeCode, exe, &["auth", "status"], env, 12)
            .await?;
    parse_status(ok, &output)
}

impl LoginStatus {
    pub(crate) fn method(&self) -> Option<&str> {
        self.auth_method.as_deref().filter(|method| {
            matches!(
                *method,
                "claude.ai"
                    | "oauth"
                    | "oauth_token"
                    | "api_key"
                    | "api_key_helper"
                    | "third_party"
            )
        })
    }

    fn identity(&self, exe: &Path, env: &ShellEnv, salt: &[u8]) -> Result<Identity, QueryFailure> {
        if !self.logged_in {
            return Err(failure(QueryError::Authentication));
        }
        if !matches!(self.method(), Some("claude.ai" | "oauth")) {
            return Err(failure(QueryError::Identity));
        }
        let account = self
            .account_uuid
            .as_deref()
            .filter(|id| !id.trim().is_empty())
            .or_else(|| {
                self.email
                    .as_deref()
                    .filter(|email| !email.trim().is_empty())
            })
            .ok_or_else(|| failure(QueryError::Identity))?;
        let config = self
            .config_directory
            .as_deref()
            .or_else(|| env.vars.get("CLAUDE_CONFIG_DIR").map(String::as_str))
            .or_else(|| env.vars.get("HOME").map(String::as_str))
            .unwrap_or("");
        Ok(Identity {
            key: identity::opaque_key(
                salt,
                &[
                    b"claude-cli-auth-status",
                    exe.as_os_str().as_encoded_bytes(),
                    config.as_bytes(),
                    account.as_bytes(),
                    self.org_id.as_deref().unwrap_or("").as_bytes(),
                ],
            ),
        })
    }
}

pub(crate) async fn observe(
    exe: &Path,
    env: &ShellEnv,
    salt: &[u8],
) -> Result<Observation, QueryFailure> {
    let settings = crate::usage::claude_settings(env);
    if settings.has_token
        || settings.base_url.is_some()
        || settings.cloud.is_some()
        || [
            "CLAUDE_CODE_OAUTH_TOKEN",
            "CLAUDE_CODE_OAUTH_TOKEN_FILE_DESCRIPTOR",
        ]
        .iter()
        .any(|key| env.vars.contains_key(*key))
    {
        return Err(failure(QueryError::Identity));
    }
    let identity = login_status(exe, env).await?.identity(exe, env, salt)?;
    Ok(Observation {
        identity,
        source_login: None,
        credential_revision: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quota_identity_uses_account_org_and_config_from_positive_cli_status() {
        let raw = r#"{"loggedIn":true,"authMethod":"claude.ai","email":"one@example.test","orgId":"org-401-a","configDirectory":"/cfg"}"#;
        let env = ShellEnv {
            vars: Default::default(),
        };
        let exe = Path::new("/fixture/claude");
        let parse = |output: &str| {
            parse_status(true, output)
                .unwrap()
                .identity(exe, &env, b"salt")
                .map(|id| id.key)
        };
        let first = parse(raw).unwrap();
        for (old, new) in [("one@", "two@"), ("org-401-a", "org-b"), ("/cfg", "/other")] {
            assert_ne!(parse(&raw.replace(old, new)).unwrap(), first);
        }
        assert_eq!(
            parse(&raw.replace("claude.ai", "api_key"))
                .unwrap_err()
                .category,
            QueryError::Identity
        );
        assert_eq!(
            parse(r#"{"loggedIn":true,"authMethod":"claude.ai","subscriptionType":"pro"}"#)
                .unwrap_err()
                .category,
            QueryError::Identity
        );
    }
}
