//! Knowledge pack download from a user-supplied URL (docs/04 §10). The pack is fetched
//! here rather than from the webview so the CSP can keep outside hosts blocked; parsing and
//! validation happen in the frontend, which renders the result for the user to review.

use std::time::Duration;

use crate::env::ShellEnv;

/// Packs are small JSON documents; anything larger is refused.
const MAX_BYTES: u64 = 5 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(30);

/// Fetches `url` (https only, or http to localhost for pack authors) and returns the body.
/// Uses global proxy configuration when provided (P3.m6e).
pub async fn fetch(
    env: &ShellEnv,
    url: &str,
    proxy_config: Option<&tokage_core::proxy::GlobalProxyConfig>,
) -> Result<String, String> {
    validate_url(url)?;
    let mut cmd = tokio::process::Command::new(if cfg!(windows) { "curl.exe" } else { "curl" });
    cmd.args([
        "--fail",
        "--silent",
        "--show-error",
        "--location",
        "--max-redirs",
        "3",
        "--proto",
        "=https,http",
        "--proto-redir",
        "=https",
        "--max-time",
        "25",
        "--max-filesize",
        &MAX_BYTES.to_string(),
    ]);

    if let Some(cfg) = proxy_config {
        match cfg.mode {
            tokage_core::proxy::ProxyMode::None => {
                cmd.arg("--noproxy").arg("*");
            }
            tokage_core::proxy::ProxyMode::CliOwn => {
                // Relies on login shell proxy variables in env.vars
            }
            tokage_core::proxy::ProxyMode::System => {
                if let Some(sys_up) = tokage_native::proxy::detect_system_proxy() {
                    cmd.arg("--proxy").arg(sys_up.to_url(true));
                }
                if let Some(sys_np) = tokage_native::proxy::detect_system_no_proxy() {
                    cmd.arg("--noproxy").arg(sys_np);
                } else if !cfg.no_proxy.trim().is_empty() {
                    cmd.arg("--noproxy").arg(&cfg.no_proxy);
                }
            }
            tokage_core::proxy::ProxyMode::Manual => {
                if let Some(ref up) = cfg.upstream {
                    cmd.arg("--proxy").arg(up.to_url(true));
                }
                if !cfg.no_proxy.trim().is_empty() {
                    cmd.arg("--noproxy").arg(&cfg.no_proxy);
                }
            }
            tokage_core::proxy::ProxyMode::InheritGlobal => {}
        }
        if let Some(ref ca) = cfg.ca_file {
            if !ca.trim().is_empty() {
                cmd.arg("--cacert").arg(ca);
            }
        }
    }

    cmd.args(["--", url])
        .env_clear()
        .envs(&env.vars)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    tokage_native::proc::isolate(&mut cmd);
    let out = tokio::time::timeout(TIMEOUT, cmd.output())
        .await
        .map_err(|_| "Request timed out".to_string())?
        .map_err(|e| format!("curl not available: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() { format!("Download failed ({})", out.status) } else { err });
    }
    if out.stdout.len() as u64 > MAX_BYTES {
        return Err("Pack is larger than 5 MB".into());
    }
    String::from_utf8(out.stdout).map_err(|_| "Pack is not UTF-8 text".into())
}

/// Official metadata returned by `GET https://<release-host>/tokage/models/latest.json` (docs/04 §10).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct RemotePackMetadata {
    pub pack_version: String,
    pub sha256: String,
    pub url: String,
    pub minisig: String,
}

/// Status of the staged / pending official knowledge pack update.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct StagedUpdateInfo {
    pub pack_version: String,
    pub pending_path: String,
    pub sha256: String,
}

/// Official Tokage Minisign public key for verifying official knowledge packs.
/// `None` while D6 (official update channel) is deferred — no network check runs.
pub const OFFICIAL_MINISIGN_PUBKEY: Option<&str> = None;

/// Returned when official updates are attempted without a configured signing key (F8).
pub const ERR_UPDATE_KEY_UNSET: &str = "UPDATE_KEY_UNSET";

/// Verifies a pack byte array against a minisign signature string using the given public key.
pub fn verify_pack_minisign(data: &[u8], signature_str: &str, public_key_str: &str) -> Result<(), String> {
    use minisign_verify::PublicKey;

    let pubkey = PublicKey::from_base64(public_key_str.trim())
        .map_err(|e| format!("Invalid minisign public key: {e}"))?;

    let signature = minisign_verify::Signature::decode(signature_str.trim())
        .map_err(|e| format!("Invalid minisign signature format: {e}"))?;

    pubkey
        .verify(data, &signature, false)
        .map_err(|e| format!("Minisign signature verification failed: {e}"))
}

/// Verifies SHA-256 hash of the downloaded pack bytes matches the metadata checksum.
pub fn verify_sha256(data: &[u8], expected_hex: &str) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    let hash_bytes = hasher.finalize();
    let actual_hex = format!("{:x}", hash_bytes);
    if actual_hex.eq_ignore_ascii_case(expected_hex.trim()) {
        Ok(())
    } else {
        Err(format!(
            "SHA-256 mismatch: expected {expected_hex}, got {actual_hex}"
        ))
    }
}

/// Checks the official release metadata, downloads the new pack if newer, verifies minisign signature,
/// verifies schema_version == 1, and writes to `pending.json` under `app_data_dir/knowledge/` (docs/04 §10).
///
/// When neither `override_pubkey` nor [`OFFICIAL_MINISIGN_PUBKEY`] is set, returns
/// [`ERR_UPDATE_KEY_UNSET`] before any network request (F8 / D6 deferral).
pub async fn check_and_stage_update(
    env: &ShellEnv,
    metadata_url: &str,
    current_pack_version: &str,
    app_data_dir: &std::path::Path,
    proxy_config: Option<&tokage_core::proxy::GlobalProxyConfig>,
    override_pubkey: Option<&str>,
) -> Result<Option<StagedUpdateInfo>, String> {
    // F8: gate before any network I/O.
    let pubkey_str = match (override_pubkey, OFFICIAL_MINISIGN_PUBKEY) {
        (Some(k), _) => k,
        (None, Some(k)) => k,
        (None, None) => return Err(ERR_UPDATE_KEY_UNSET.to_string()),
    };

    // 1. Fetch metadata JSON
    let meta_json = fetch(env, metadata_url, proxy_config).await?;
    let meta: RemotePackMetadata = serde_json::from_str(&meta_json)
        .map_err(|e| format!("Failed to parse remote pack metadata: {e}"))?;

    // Check if new version
    if meta.pack_version == current_pack_version {
        return Ok(None);
    }

    // 2. Download new pack
    let pack_text = fetch(env, &meta.url, proxy_config).await?;
    let pack_bytes = pack_text.as_bytes();

    // 3. Verify SHA-256
    verify_sha256(pack_bytes, &meta.sha256)?;

    // 4. Verify Minisign signature
    verify_pack_minisign(pack_bytes, &meta.minisig, pubkey_str)?;

    // 5. Verify schema_version == 1 & structural validity
    let parsed: tokage_matching::KnowledgePack = tokage_matching::parse_knowledge_pack(&pack_text)
        .map_err(|e| format!("Downloaded pack validation failed: {e}"))?;

    if parsed.schema_version != 1 {
        return Err(format!(
            "Unsupported schema_version: {}, expected 1",
            parsed.schema_version
        ));
    }

    // 6. Write to pending.json
    let knowledge_dir = app_data_dir.join("knowledge");
    std::fs::create_dir_all(&knowledge_dir)
        .map_err(|e| format!("Failed to create knowledge directory: {e}"))?;

    let pending_path = knowledge_dir.join("pending.json");
    std::fs::write(&pending_path, &pack_text)
        .map_err(|e| format!("Failed to write pending.json: {e}"))?;

    Ok(Some(StagedUpdateInfo {
        pack_version: meta.pack_version,
        pending_path: pending_path.to_string_lossy().to_string(),
        sha256: meta.sha256,
    }))
}

/// Reads the staged `pending.json`, applies it atomically into SQLite via `reload_official_pack`,
/// and deletes the pending file.
pub async fn apply_staged_update(
    storage: &tokage_storage::Storage,
    app_data_dir: &std::path::Path,
) -> Result<tokage_matching::KnowledgePack, String> {
    let pending_path = app_data_dir.join("knowledge").join("pending.json");
    if !pending_path.exists() {
        return Err("No pending knowledge pack update found".into());
    }

    let pack_text = std::fs::read_to_string(&pending_path)
        .map_err(|e| format!("Failed to read pending.json: {e}"))?;

    let pack = tokage_matching::parse_knowledge_pack(&pack_text)
        .map_err(|e| format!("Invalid pending pack: {e}"))?;

    // Atomic reload into SQLite
    storage.reload_official_knowledge_pack(pack.clone()).await?;

    // Clean up pending file
    let _ = std::fs::remove_file(pending_path);

    Ok(pack)
}

fn validate_url(url: &str) -> Result<(), String> {
    if url.len() > 2048 || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("Invalid URL".into());
    }
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("https://") {
        return Ok(());
    }
    // Plain http only for a locally served pack while authoring one.
    if let Some(rest) = lower.strip_prefix("http://") {
        let host = rest.split(['/', '?', '#']).next().unwrap_or("");
        let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
        let host = if host.starts_with('[') {
            host.split(']').next().map(|h| &h[1..]).unwrap_or("")
        } else {
            host.split(':').next().unwrap_or("")
        };
        if matches!(host, "localhost" | "127.0.0.1" | "::1") {
            return Ok(());
        }
    }
    Err("Only https:// URLs are allowed (http:// for localhost only)".into())
}

#[cfg(test)]
mod tests {
    use super::validate_url;

    /// The shipped pack must stay valid: the app reads it offline with no fallback.
    #[test]
    fn builtin_pack() {
        let text = include_str!("../resources/knowledge/builtin.json");
        let pack: serde_json::Value = serde_json::from_str(text).expect("builtin.json is valid JSON");
        assert_eq!(pack["schema_version"], 1);
        assert_eq!(pack["pack_version"], "builtin-0.0.2");
        let domains: Vec<&str> = pack["domains"].as_array().unwrap().iter().filter_map(|d| d.as_str()).collect();
        let models = pack["models"].as_object().expect("models is keyed by model id");
        let min = pack["success_model"]["planner_min_skill"].as_f64().unwrap();
        for (id, m) in models {
            let skills = m["skills"].as_object().unwrap();
            for d in &domains {
                let v = skills.get(*d).and_then(|v| v.as_f64()).unwrap_or_else(|| panic!("{id}: missing skill {d}"));
                assert!((1.0..=5.0).contains(&v), "{id}: skills.{d} = {v}");
            }
            let (ctx, eff) = (m["context_window"].as_u64().unwrap(), m["effective_context"].as_u64().unwrap());
            assert!(eff <= ctx, "{id}: effective_context > context_window");
            let price = &m["price_per_mtok"];
            assert!(price["input"].as_f64().unwrap() >= 0.0 && price["output"].as_f64().unwrap() >= 0.0);
            // Planner role needs top planning skill (docs/04 §5).
            if m["roles"].as_array().unwrap().iter().any(|r| r == "planner") {
                assert!(skills["planning"].as_f64().unwrap() >= min, "{id}");
            }
        }
        let mut offered = std::collections::HashSet::new();
        for (provider, p) in pack["providers"].as_object().unwrap() {
            assert!(p["billing"].is_string(), "{provider}: no billing");
            let mut seen = std::collections::HashSet::new();
            for o in p["models"].as_array().map(Vec::as_slice).unwrap_or_default() {
                let id = o["model"].as_str().unwrap();
                assert!(models.contains_key(id), "{provider}: unknown model {id}");
                assert!(seen.insert(id), "duplicate {provider}:{id}");
                assert!(o["cli_model_arg"].as_str().is_some_and(|a| !a.is_empty()), "{provider}:{id}: no cli_model_arg");
                // A CLI can only narrow the model's context.
                if let Some(ctx) = o["context_window"].as_u64() {
                    assert!(ctx <= models[id]["context_window"].as_u64().unwrap(), "{provider}:{id}");
                    assert!(o["effective_context"].as_u64().unwrap() <= ctx, "{provider}:{id}");
                }
                offered.insert(id);
            }
        }
        for id in models.keys() {
            assert!(offered.contains(id.as_str()), "{id} is not offered by any CLI");
        }
    }

    #[test]
    fn urls() {
        assert!(validate_url("https://example.com/pack.json").is_ok());
        assert!(validate_url("http://localhost:8080/pack.json").is_ok());
        assert!(validate_url("http://127.0.0.1/pack.json").is_ok());
        assert!(validate_url("http://[::1]:8000/p").is_ok());
        assert!(validate_url("http://example.com/pack.json").is_err());
        assert!(validate_url("http://localhost.evil.com/p").is_err());
        assert!(validate_url("http://localhost@evil.com/p").is_err());
        assert!(validate_url("file:///etc/passwd").is_err());
        assert!(validate_url("https://a.com/x y").is_err());
        assert!(validate_url("-o /tmp/x").is_err());
    }

    /// minisign-verify README example key — only for unit tests via override_pubkey (F8.a).
    /// Must never appear in the production constants above.
    const TEST_MINISIGN_PUBKEY: &str =
        "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";

    #[test]
    fn test_verify_pack_minisign_and_sha256() {
        let test_data = b"test";
        let valid_sig = "untrusted comment: signature from minisign secret key\nRUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=\ntrusted comment: timestamp:1556193335\tfile:test\ny/rUw2y8/hOUYjZU71eHp/Wo1KZ40fGy2VJEDl34XMJM+TX48Ss/17u3IvIfbVR1FkZZSNCisQbuQY+bHwhEBg==";

        // Valid signature with test override key
        assert!(super::verify_pack_minisign(test_data, valid_sig, TEST_MINISIGN_PUBKEY).is_ok());

        // Corrupted payload (one-byte change)
        assert!(super::verify_pack_minisign(b"tesu", valid_sig, TEST_MINISIGN_PUBKEY).is_err());

        // SHA-256 verification
        // sha256("test") = 9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08
        let valid_hash = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
        assert!(super::verify_sha256(test_data, valid_hash).is_ok());
        assert!(super::verify_sha256(test_data, "badhash").is_err());
    }

    /// F8.a: production source must not embed the minisign README example pubkey.
    #[test]
    fn test_official_pubkey_unset_and_source_has_no_example_key() {
        assert!(super::OFFICIAL_MINISIGN_PUBKEY.is_none());
        // Only the production portion of this file (before `#[cfg(test)]`).
        let src = include_str!("knowledge.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap_or(src);
        assert!(
            !prod.contains(TEST_MINISIGN_PUBKEY),
            "production knowledge.rs must not embed the minisign README example pubkey"
        );
    }

    /// F8.a: without a pubkey, unreachable metadata URLs never get contacted.
    #[tokio::test]
    async fn test_check_and_stage_update_returns_key_unset_before_network() {
        let env = crate::env::ShellEnv::default();
        let temp = tempfile::tempdir().unwrap();
        let err = super::check_and_stage_update(
            &env,
            "http://127.0.0.1:1/",
            "0.0.0",
            temp.path(),
            None,
            None,
        )
        .await
        .expect_err("must fail without pubkey");
        assert_eq!(err, super::ERR_UPDATE_KEY_UNSET);
    }

    #[tokio::test]
    async fn test_apply_staged_update_lifecycle() {
        let temp_dir = tempfile::tempdir().unwrap();
        let app_data_dir = temp_dir.path();
        let knowledge_dir = app_data_dir.join("knowledge");
        std::fs::create_dir_all(&knowledge_dir).unwrap();

        // Write a valid staged pending.json
        let pending_file = knowledge_dir.join("pending.json");
        let sample_pack = tokage_matching::builtin_knowledge_pack();
        let mut new_pack = sample_pack.clone();
        new_pack.pack_version = "2026.11.15".to_string();
        let pack_json = serde_json::to_string(&new_pack).unwrap();
        std::fs::write(&pending_file, &pack_json).unwrap();

        // Initialize storage
        let storage = tokage_storage::Storage::open(&app_data_dir.join("db")).unwrap();
        storage.save_knowledge_pack(sample_pack.clone()).await.unwrap();

        // Apply staged update
        let applied = super::apply_staged_update(&storage, app_data_dir).await.unwrap();
        assert_eq!(applied.pack_version, "2026.11.15");

        // Verify pending.json was deleted
        assert!(!pending_file.exists());

        // Verify storage models have new version
        let models = storage.list_model_knowledge().unwrap();
        assert_eq!(models[0].pack_version, "2026.11.15");
    }
}
