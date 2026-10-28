//! Identity proof comes from non-chat CLI status, never from a plan label or log.
use crate::{env::ShellEnv, providers};
use sha2::{Digest, Sha256};
use std::path::Path;
use tokage_core::ProviderId;

/// No Debug or serialization: raw status identities and source digests stay native.
#[derive(Clone, PartialEq, Eq)]
pub struct Identity {
    pub(crate) key: String,
}
pub(crate) struct Observation {
    pub identity: Identity,
    pub source_login: Option<super::contract::SourceLogin>,
    pub credential_revision: Option<String>,
    pub claude_lifecycle: Option<super::claude_diagnostics::CredentialState>,
}
pub fn opaque_key(salt: &[u8], parts: &[&[u8]]) -> String {
    // Domain-separated keyed digest; raw Token hashes are never returned.
    let key = Sha256::digest(salt);
    let mut inner_pad = [0x36u8; 64];
    let mut outer_pad = [0x5cu8; 64];
    for (i, b) in key.iter().enumerate() {
        inner_pad[i] ^= b;
        outer_pad[i] ^= b;
    }
    let mut h = Sha256::new();
    h.update(inner_pad);
    h.update(b"tokage-usage-identity-v2");
    for part in parts {
        h.update((part.len() as u64).to_le_bytes());
        h.update(part);
    }
    let inner = h.finalize();
    let mut outer = Sha256::new();
    outer.update(outer_pad);
    outer.update(inner);
    format!("{:x}", outer.finalize())
}
fn field<'a>(v: &'a serde_json::Value, paths: &[&str]) -> Option<&'a str> {
    paths.iter().find_map(|p| {
        v.pointer(p)
            .and_then(|s| s.as_str())
            .filter(|s| !s.trim().is_empty())
    })
}
pub(crate) fn status_identity(provider: ProviderId, v: &serde_json::Value) -> Option<&str> {
    match provider {
        ProviderId::CursorCli if v["isAuthenticated"] == true && v["status"] == "authenticated" => {
            field(v, &["/userInfo/id", "/userInfo/email"])
        }
        ProviderId::ClaudeCode if v["loggedIn"] == true => field(v, &["/accountUuid", "/email"]),
        ProviderId::CopilotCli if v["isAuthenticated"] == true => {
            field(v, &["/login", "/user/login", "/user/id"])
        }
        _ => None,
    }
}
fn context(value: &serde_json::Value) -> Option<Vec<u8>> {
    serde_json::to_vec(&serde_json::json!({
        "auth":value.get("authMethod").or_else(||value.get("authType")),
        "org":value.get("orgId").or_else(||value.get("organizationId")),
        "workspace":value.get("workspaceId"),
        "host":value.get("host").or_else(||value.get("hostname")).or_else(||value.get("enterpriseUrl")),
        "tier":value.get("subscriptionType").or_else(||value.get("subscriptionTier")),
        "source":value.get("apiKeySource"),
    })).ok()
}
/// Preserve management transport failures instead of misreporting them as a missing login.
pub(crate) async fn observe(provider: ProviderId, exe: &Path, env: &ShellEnv, salt: &[u8]) -> Result<Observation, super::contract::QueryFailure> {
    if provider == ProviderId::CursorCli { return cursor_observe(exe, env, salt).await; }
    if provider == ProviderId::KiroCli { return super::kiro::observe(exe, env, salt).ok_or(super::contract::QueryFailure { category: super::contract::QueryError::Identity, retry_after: None }); }
    if provider == ProviderId::CodexCli { return super::codex::observe(exe, env, salt); }
    if provider == ProviderId::AntigravityCli { return super::credentials::antigravity_observe(exe,env,salt).await?.ok_or(super::contract::QueryFailure { category:super::contract::QueryError::Identity, retry_after:None }); }
    if provider == ProviderId::ClaudeCode {
        let profile=super::credentials::claude(exe, env, salt).await?;
        return profile.and_then(|p| p.identity.map(|identity| Observation { claude_lifecycle: Some(p.lifecycle), identity, source_login: None, credential_revision: Some(p.credential_revision) })).ok_or(super::contract::QueryFailure { category: super::contract::QueryError::Identity, retry_after: None });
    }
    let identity = if provider == ProviderId::Opencode { super::opencode::resolve_checked(exe, env, salt).await? }
    else { resolve(provider, exe, env, salt).await.ok_or(super::contract::QueryFailure { category: super::contract::QueryError::Identity, retry_after: None })? };
    Ok(Observation { claude_lifecycle: None, identity, source_login: None, credential_revision: None })
}
async fn cursor_observe(exe: &Path, env: &ShellEnv, salt: &[u8]) -> Result<Observation, super::contract::QueryFailure> {
    use super::contract::{QueryError, QueryFailure};
    let account = crate::usage::cursor_account(env, None);
    if !account.official || account.billing != crate::usage::Billing::Subscription {
        return Err(QueryFailure { category: QueryError::Identity, retry_after: None });
    }
    let (ok, text) = providers::run_auth_checked(ProviderId::CursorCli, exe, &["status", "--format", "json"], env, 12).await?;
    cursor_observation(ok, &text, exe, env, salt)
}
fn cursor_observation(ok: bool, text: &str, exe: &Path, env: &ShellEnv, salt: &[u8]) -> Result<Observation, super::contract::QueryFailure> {
    use super::contract::{QueryError, QueryFailure, SourceLogin, SourceLoginStatus};
    let failure = |category| QueryFailure { category, retry_after: None };
    let range = text.find('{').zip(text.rfind('}'));
    let value: serde_json::Value = range.and_then(|(a,b)| serde_json::from_str(&text[a..=b]).ok()).ok_or_else(|| {
        let lower=text.to_ascii_lowercase();
        failure(if ["not logged in", "not signed in", "unauthorized", "token expired"].iter().any(|s| lower.contains(s)) { QueryError::Authentication } else if ok { QueryError::InvalidData } else { QueryError::Network })
    })?;
    if value["isAuthenticated"] == false || matches!(value["status"].as_str(), Some("unauthenticated" | "logged_out")) { return Err(failure(QueryError::Authentication)); }
    if !ok {
        let lower=text.to_ascii_lowercase();
        return Err(failure(if ["not logged in", "not signed in", "unauthorized", "token expired"].iter().any(|s|lower.contains(s)) { QueryError::Authentication } else { QueryError::Network }));
    }
    let raw = status_identity(ProviderId::CursorCli, &value).ok_or_else(|| failure(QueryError::InvalidData))?;
    let metadata = context(&value).ok_or_else(|| failure(QueryError::InvalidData))?;
    let mut parts = vec![ProviderId::CursorCli.as_str().as_bytes(), raw.as_bytes(), exe.as_os_str().as_encoded_bytes(), metadata.as_slice()];
    for key in ["CURSOR_API_KEY", "CURSOR_API_ENDPOINT"] { if let Some(value)=env.vars.get(key) { parts.push(key.as_bytes()); parts.push(value.as_bytes()); } }
    let account = crate::usage::Account { billing: crate::usage::Billing::Subscription, official: true, endpoint: None,
        plan: field(&value, &["/subscriptionType", "/subscriptionTier"]).map(str::to_owned), balance: None, usage: None };
    Ok(Observation { claude_lifecycle: None, credential_revision: None, identity: Identity { key: opaque_key(salt, &parts) }, source_login: Some(SourceLogin {
        path: exe.to_string_lossy().into_owned(), observed_at: super::now(), status: SourceLoginStatus::Ready,
        auth_method: Some(field(&value, &["/userInfo/email"]).unwrap_or("Cursor account").into()), account: Some(account),
    }) })
}
pub async fn resolve(
    provider: ProviderId,
    exe: &Path,
    env: &ShellEnv,
    salt: &[u8],
) -> Option<Identity> {
    if provider == ProviderId::CursorCli { return cursor_observe(exe, env, salt).await.ok().map(|o| o.identity); }
    if provider == ProviderId::KiroCli { return super::kiro::resolve(exe, env, salt); }
    if provider == ProviderId::CodexCli { return super::codex::resolve(exe, env, salt); }
    if provider == ProviderId::Aider { return super::aider::resolve(exe, env, salt); }
    if provider == ProviderId::ClaudeCode { return super::credentials::claude(exe, env, salt).await.ok()??.identity; }
    if provider == ProviderId::AntigravityCli { return super::credentials::antigravity(exe, env, salt).await; }
    if provider == ProviderId::Reasonix { return super::reasonix::resolve(env, salt); }
    if provider == ProviderId::Opencode {
        return super::opencode::resolve(exe, env, salt).await;
    }
    if provider != ProviderId::CopilotCli { return None; }
    let (auth, _) = providers::copilot_status_rpc(exe, env).await?;
    let raw = status_identity(provider, &auth)?;
    if env
        .vars
        .get("COPILOT_PROVIDER_BASE_URL")
        .is_some_and(|s| !s.trim().is_empty())
    {
        return None;
    }
    let token = ["COPILOT_GITHUB_TOKEN", "GH_TOKEN", "GITHUB_TOKEN"]
        .iter()
        .find_map(|k| {
            env.vars
                .get(*k)
                .filter(|s| !s.trim().is_empty())
                .map(|value| (*k, value))
        });
    let metadata = context(&auth)?;
    return Some(Identity {
        key: opaque_key(
            salt,
            &[
                provider.as_str().as_bytes(),
                raw.as_bytes(),
                exe.as_os_str().as_encoded_bytes(),
                token.map_or(b"CLI-owned-auth".as_slice(), |(name, _)| name.as_bytes()),
                token.map_or(b"CLI-owned-auth".as_slice(), |(_, value)| value.as_bytes()),
                &metadata,
            ],
        ),
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cursor_status_proof_preserves_transport_authentication_and_data_failures() {
        use super::super::contract::QueryError;
        let env=ShellEnv { vars: Default::default() }; let exe=Path::new("/fixture/cursor");
        let raw=r#"{"isAuthenticated":true,"status":"authenticated","userInfo":{"id":"a","email":"person@example.invalid"},"subscriptionTier":"Pro+"}"#;
        let observed=cursor_observation(true,raw,exe,&env,b"salt").ok().unwrap();
        let proof=observed.source_login.unwrap();assert_eq!(proof.auth_method.as_deref(),Some("person@example.invalid"));assert_eq!(proof.account.unwrap().plan.as_deref(),Some("Pro+"));
        assert!(cursor_observation(true,raw,exe,&env,b"salt").ok().unwrap().identity==observed.identity);
        for (ok,text,category) in [(false,"network unavailable",QueryError::Network),(true,"loading",QueryError::InvalidData),(true,r#"{"isAuthenticated":false,"status":"logged_out"}"#,QueryError::Authentication),(true,r#"{"isAuthenticated":true,"status":"authenticated"}"#,QueryError::InvalidData)] {
            assert_eq!(cursor_observation(ok,text,exe,&env,b"salt").err().unwrap().category,category);
        }
    }
    #[test]
    fn identity_requires_positive_status_and_never_uses_a_plan() {
        assert!(status_identity(ProviderId::CursorCli,&serde_json::json!({"isAuthenticated":true,"status":"authenticated","subscriptionTier":"Pro"})).is_none());
        assert!(status_identity(
            ProviderId::ClaudeCode,
            &serde_json::json!({"loggedIn":false,"email":"a"})
        )
        .is_none());
        assert_ne!(
            opaque_key(b"local-a", &[b"account"]),
            opaque_key(b"local-b", &[b"account"])
        );
        assert_ne!(
            opaque_key(b"local-a", &[b"account", b"token-a"]),
            opaque_key(b"local-a", &[b"account", b"token-b"])
        );
        assert!(!opaque_key(b"secret", &[b"account"]).contains("account"));
    }
}
