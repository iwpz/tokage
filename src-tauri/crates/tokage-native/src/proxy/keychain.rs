//! Secure proxy credential storage (docs/12 §4, P3.m6h).
//!
//! Stores upstream proxy credentials in the OS keychain (macOS Keychain Services)
//! with a secure in-memory vault fallback, ensuring credentials are only used by Tokage's
//! local relay and never exposed in child process environments.

use std::collections::HashMap;
use std::sync::Mutex;

const SERVICE_NAME: &str = "tokage.proxy";

static MEMORY_VAULT: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

fn with_memory_vault<R>(f: impl FnOnce(&mut HashMap<String, String>) -> R) -> R {
    let mut lock = MEMORY_VAULT.lock().unwrap();
    let map = lock.get_or_insert_with(HashMap::new);
    f(map)
}

/// Stores a proxy credential securely.
pub fn store_proxy_credential(account: &str, secret: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("/usr/bin/security")
            .args([
                "add-generic-password",
                "-s",
                SERVICE_NAME,
                "-a",
                account,
                "-w",
                secret,
                "-U",
            ])
            .output();

        if let Ok(out) = status {
            if out.status.success() {
                // Also cache in memory vault for fast access
                with_memory_vault(|m| {
                    m.insert(account.to_string(), secret.to_string());
                });
                return Ok(());
            }
        }
    }

    // Fallback: store in process-isolated secure memory vault
    with_memory_vault(|m| {
        m.insert(account.to_string(), secret.to_string());
    });
    Ok(())
}

/// Retrieves a stored proxy credential.
pub fn get_proxy_credential(account: &str) -> Option<String> {
    // Check in-memory cache first
    let cached = with_memory_vault(|m| m.get(account).cloned());
    if cached.is_some() {
        return cached;
    }

    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("/usr/bin/security")
            .args([
                "find-generic-password",
                "-s",
                SERVICE_NAME,
                "-a",
                account,
                "-w",
            ])
            .output()
            .ok()?;

        if out.status.success() {
            let secret = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !secret.is_empty() {
                with_memory_vault(|m| {
                    m.insert(account.to_string(), secret.clone());
                });
                return Some(secret);
            }
        }
    }

    None
}

/// Deletes a stored proxy credential.
pub fn delete_proxy_credential(account: &str) -> Result<(), String> {
    with_memory_vault(|m| {
        m.remove(account);
    });

    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("/usr/bin/security")
            .args([
                "delete-generic-password",
                "-s",
                SERVICE_NAME,
                "-a",
                account,
            ])
            .output();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_vault_roundtrip() {
        let account = "test-proxy-user-roundtrip";
        let secret = "super-secret-password-123";

        store_proxy_credential(account, secret).unwrap();
        assert_eq!(get_proxy_credential(account).as_deref(), Some(secret));

        delete_proxy_credential(account).unwrap();
        assert_eq!(get_proxy_credential(account), None);
    }
}
