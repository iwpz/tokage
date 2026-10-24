//! Explicit installation-local consent, separate from importable application settings.
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PermissionState {
    pub allow_online_usage_query: bool,
    pub generation: u64,
}
struct Inner {
    state: PermissionState,
    changed: tokio::sync::watch::Sender<u64>,
}
pub struct OnlineUsagePermission {
    path: PathBuf,
    inner: Arc<Mutex<Inner>>,
}
pub struct QueryPermit {
    inner: Arc<Mutex<Inner>>,
    generation: u64,
}

impl OnlineUsagePermission {
    /// This file is deliberately outside the application DB and settings export/import.
    /// Missing, malformed and unreadable records fail closed. No migration grants consent.
    pub fn open(path: &Path) -> Self {
        let allowed = std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice::<PermissionState>(&b).ok())
            .is_some_and(|s| s.allow_online_usage_query);
        let (changed, _) = tokio::sync::watch::channel(0);
        Self {
            path: path.into(),
            inner: Arc::new(Mutex::new(Inner {
                state: PermissionState {
                    allow_online_usage_query: allowed,
                    generation: 0,
                },
                changed,
            })),
        }
    }
    pub fn state(&self) -> Result<PermissionState, String> {
        Ok(self
            .inner
            .lock()
            .map_err(|_| "Usage permission unavailable")?
            .state
            .clone())
    }
    pub fn set(&self, allowed: bool) -> Result<PermissionState, String> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "Usage permission unavailable")?;
        // Revoke in memory first even if disk is unavailable. No old lease can publish.
        inner.state.generation = inner
            .state
            .generation
            .checked_add(1)
            .ok_or("Usage generation exhausted")?;
        inner.state.allow_online_usage_query = false;
        inner.changed.send_replace(inner.state.generation);
        let state = PermissionState {
            allow_online_usage_query: allowed,
            generation: inner.state.generation,
        };
        // A durable disable is deletion: restoring an old DB cannot restore consent.
        if !allowed {
            match std::fs::remove_file(&self.path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => {
                    return Err("Usage disabled for this process; cannot persist revocation".into())
                }
            }
            return Ok(state);
        }
        let parent = self.path.parent().ok_or("Invalid usage permission path")?;
        std::fs::create_dir_all(parent).map_err(|_| "Cannot save usage permission")?;
        let mut file =
            tempfile::NamedTempFile::new_in(parent).map_err(|_| "Cannot save usage permission")?;
        use std::io::Write;
        file.write_all(&serde_json::to_vec(&state).map_err(|_| "Cannot encode usage permission")?)
            .and_then(|_| file.as_file().sync_all())
            .map_err(|_| "Cannot save usage permission")?;
        file.persist(&self.path)
            .map_err(|_| "Cannot save usage permission")?;
        inner.state = state.clone();
        Ok(state)
    }
    pub fn permit(&self) -> Result<QueryPermit, String> {
        let inner = self
            .inner
            .lock()
            .map_err(|_| "Usage permission unavailable")?;
        if !inner.state.allow_online_usage_query {
            return Err("Online usage query is disabled".into());
        }
        Ok(QueryPermit {
            inner: self.inner.clone(),
            generation: inner.state.generation,
        })
    }
}
impl QueryPermit {
    /// Validate and publish under the same lock as revocation.
    pub fn publish<T>(&self, publish: impl FnOnce() -> T) -> Result<T, String> {
        let inner = self
            .inner
            .lock()
            .map_err(|_| "Usage permission unavailable")?;
        if !inner.state.allow_online_usage_query || inner.state.generation != self.generation {
            return Err("Online usage permission changed; result discarded".into());
        }
        Ok(publish())
    }
    /// Dropping the query future cancels its child; callers must use cancellation-safe IO.
    pub async fn run<T>(
        &self,
        query: impl std::future::Future<Output = Result<T, String>>,
    ) -> Result<T, String> {
        let mut changed = {
            let inner = self
                .inner
                .lock()
                .map_err(|_| "Usage permission unavailable")?;
            if !inner.state.allow_online_usage_query || inner.state.generation != self.generation {
                return Err("Online usage permission changed".into());
            }
            inner.changed.subscribe()
        };
        tokio::select! {
            biased;
            _ = changed.changed() => Err("Online usage permission changed; query cancelled".into()),
            result = query => { let value = result?; self.publish(|| value) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn consent_restart_corruption_and_revocation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("consent.json");
        let p = OnlineUsagePermission::open(&path);
        assert!(p.permit().is_err());
        p.set(true).unwrap();
        assert!(OnlineUsagePermission::open(&path).permit().is_ok());
        let permit = p.permit().unwrap();
        p.set(false).unwrap();
        p.set(true).unwrap();
        assert!(permit.publish(|| panic!("stale result published")).is_err());
        assert!(permit
            .run(async {
                panic!("stale query started");
                #[allow(unreachable_code)]
                Ok(())
            })
            .await
            .is_err());
        std::fs::write(&path, b"{broken").unwrap();
        assert!(OnlineUsagePermission::open(&path).permit().is_err());
        std::fs::write(&path, br#"{"activeQuotaQuery":true}"#).unwrap();
        assert!(OnlineUsagePermission::open(&path).permit().is_err());
    }
    #[tokio::test]
    async fn revocation_cancels_pending_work_and_rejects_late_results() {
        let dir = tempfile::tempdir().unwrap();
        let p = OnlineUsagePermission::open(&dir.path().join("permission"));
        p.set(true).unwrap();
        let permit = p.permit().unwrap();
        let query = permit.run(std::future::pending::<Result<(), String>>());
        tokio::pin!(query);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), &mut query)
                .await
                .is_err()
        );
        p.set(false).unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(1), query)
                .await
                .unwrap()
                .is_err()
        );
        assert!(permit.publish(|| panic!("late publish")).is_err());
    }
    #[test]
    fn failed_persistence_does_not_grant_consent() {
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("file");
        std::fs::write(&parent, b"x").unwrap();
        let p = OnlineUsagePermission::open(&parent.join("permission"));
        assert!(p.set(true).is_err());
        assert!(p.permit().is_err());
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn disabled_query_never_launches_and_revocation_reaps_pty() {
        let dir = tempfile::tempdir().unwrap();
        let p = OnlineUsagePermission::open(&dir.path().join("consent"));
        let marker = dir.path().join("pid");
        let script = format!(
            "echo $$ > {}; sleep 30",
            crate::chat::adapter::shell_single_quote(&marker.display().to_string())
        );
        let query = async {
            let permit = p.permit()?;
            permit
                .run(crate::chat::pty::execute_pty_interactive(
                    std::path::Path::new("/bin/sh"),
                    &["-c", &script],
                    dir.path(),
                    &[],
                    None,
                    std::time::Duration::from_secs(30),
                ))
                .await
        };
        assert!(query.await.is_err());
        assert!(!marker.exists());
        p.set(true).unwrap();
        let permit = p.permit().unwrap();
        let arguments = ["-c", script.as_str()];
        let pending = permit.run(crate::chat::pty::execute_pty_interactive(
            std::path::Path::new("/bin/sh"),
            &arguments,
            dir.path(),
            &[],
            None,
            std::time::Duration::from_secs(30),
        ));
        tokio::pin!(pending);
        for _ in 0..100 {
            tokio::select! { result = &mut pending => panic!("query ended before cancellation: {result:?}"),
            _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => {} }
            if marker.exists() {
                break;
            }
        }
        let pid: i32 = std::fs::read_to_string(&marker)
            .expect("PTY did not launch")
            .trim()
            .parse()
            .unwrap();
        assert_eq!(unsafe { libc::kill(pid, 0) }, 0);
        p.set(false).unwrap();
        assert!(pending.await.is_err());
        assert_ne!(
            unsafe { libc::kill(pid, 0) },
            0,
            "revoked PTY child survived"
        );
        assert!(OnlineUsagePermission::open(&dir.path().join("consent"))
            .permit()
            .is_err());
    }
}
