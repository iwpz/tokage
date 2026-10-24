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
pub async fn resolve(
    provider: ProviderId,
    exe: &Path,
    env: &ShellEnv,
    salt: &[u8],
) -> Option<Identity> {
    if provider == ProviderId::KiroCli { return super::kiro::resolve(exe, env, salt); }
    if provider == ProviderId::ClaudeCode { return super::credentials::claude(exe, env, salt).await?.identity; }
    if provider == ProviderId::AntigravityCli { return super::credentials::antigravity(exe, env, salt).await; }
    if provider == ProviderId::Reasonix { return super::reasonix::resolve(env, salt); }
    if provider == ProviderId::Opencode {
        return super::opencode::resolve(exe, env, salt).await;
    }
    let (status, account) = match provider {
        ProviderId::CursorCli => (
            providers::run_auth(provider, exe, &["status", "--format", "json"], env, 8).await?,
            crate::usage::cursor_account(env, None),
        ),
        ProviderId::ClaudeCode => (
            providers::run_auth(provider, exe, &["auth", "status", "--json"], env, 8).await?,
            crate::usage::claude_account(
                &crate::usage::claude_settings(env),
                Some("claude.ai"),
                None,
                Some(env),
            ),
        ),
        ProviderId::CopilotCli => {
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
        // Unknown effective credential / backend sources stay blocked for U3.
        _ => return None,
    };
    if !status.0 || !account.official || account.billing != crate::usage::Billing::Subscription {
        return None;
    }
    let text = status.1;
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    let value: serde_json::Value = serde_json::from_str(&text[start..=end]).ok()?;
    let raw = status_identity(provider, &value)?;
    let metadata = context(&value)?;
    // Only non-secret CLI status output plus effective credential env values are digested.
    // We do not open keychains, browser cookies or undocumented credential files.
    let mut parts = vec![
        provider.as_str().as_bytes(),
        raw.as_bytes(),
        exe.as_os_str().as_encoded_bytes(),
        metadata.as_slice(),
    ];
    let keys: &[&str] = if provider == ProviderId::CursorCli {
        &["CURSOR_API_KEY", "CURSOR_API_ENDPOINT"]
    } else {
        &[
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_BASE_URL",
            "CLAUDE_CONFIG_DIR",
        ]
    };
    for key in keys {
        if let Some(value) = env.vars.get(*key) {
            parts.push(key.as_bytes());
            parts.push(value.as_bytes());
        }
    }
    Some(Identity {
        key: opaque_key(salt, &parts),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
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
