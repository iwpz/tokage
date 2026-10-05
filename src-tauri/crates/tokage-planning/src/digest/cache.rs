//! In-memory cache for RepoDigest with Git HEAD and file mtime invalidation (P5.m1d).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokage_core::planning::RepoDigest;
use tokio::sync::RwLock;

/// Cache entry storing a cached digest alongside its invalidation fingerprint.
#[derive(Clone)]
struct CacheEntry {
    fingerprint: String,
    digest: RepoDigest,
}

/// Thread-safe in-memory cache for project RepoDigests.
#[derive(Clone, Default)]
pub struct DigestCache {
    entries: Arc<RwLock<HashMap<String, CacheEntry>>>,
}

impl DigestCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Computes the invalidation fingerprint for the specified project path.
    /// Checks current Git HEAD commit hash if available, or manifest mtimes as fallback.
    pub async fn compute_fingerprint(root: &Path) -> String {
        // 1. Try git rev-parse HEAD
        if let Ok(output) = tokio::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .await
        {
            if output.status.success() {
                let hash = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !hash.is_empty() {
                    return format!("git:{hash}");
                }
            }
        }

        // 2. Fallback: check mtimes of manifests
        let mut mtime_sum: u64 = 0;
        for manifest_name in &["Cargo.toml", "package.json", "pyproject.toml", "go.mod"] {
            let p = root.join(manifest_name);
            if let Ok(meta) = tokio::fs::metadata(&p).await {
                if let Ok(mtime) = meta.modified() {
                    if let Ok(dur) = mtime.duration_since(std::time::UNIX_EPOCH) {
                        mtime_sum += dur.as_secs();
                    }
                }
            }
        }

        format!("mtime:{mtime_sum}")
    }

    /// Retrieves a cached digest if it exists and the fingerprint is valid.
    pub async fn get(&self, root: &Path) -> Option<RepoDigest> {
        let key = root.to_string_lossy().to_string();
        let current_fingerprint = Self::compute_fingerprint(root).await;

        let map = self.entries.read().await;
        if let Some(entry) = map.get(&key) {
            if entry.fingerprint == current_fingerprint {
                return Some(entry.digest.clone());
            }
        }
        None
    }

    /// Stores a digest in cache.
    pub async fn put(&self, root: &Path, digest: RepoDigest) {
        let key = root.to_string_lossy().to_string();
        let current_fingerprint = Self::compute_fingerprint(root).await;

        let mut map = self.entries.write().await;
        map.insert(key, CacheEntry {
            fingerprint: current_fingerprint,
            digest,
        });
    }

    /// Invalidate cache for a specific project path.
    pub async fn invalidate(&self, root: &Path) {
        let key = root.to_string_lossy().to_string();
        let mut map = self.entries.write().await;
        map.remove(&key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_digest_cache_invalidation() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let cache = DigestCache::new();

        let mut digest = RepoDigest::default();
        digest.project_path = root.to_string_lossy().to_string();
        digest.markdown = "digest content".into();

        cache.put(root, digest.clone()).await;

        // Cache hit
        let cached = cache.get(root).await;
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().markdown, "digest content");

        // Explicit invalidation
        cache.invalidate(root).await;
        assert!(cache.get(root).await.is_none());
    }
}
