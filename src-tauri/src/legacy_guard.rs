//! Compatibility fence: existing legacy runs may finish; new orchestration belongs to runtime.
use std::path::{Path, PathBuf};
use tokage_planning::PipelineState;
use tokage_runtime::repository::Reader;
fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}
pub fn shared_run(data: &Path, id: &str) -> Result<bool, String> {
    let dir = data.join("runtime-v1");
    if !dir.join("tokage.db").exists() {
        return Ok(false);
    }
    Reader::open(&dir)
        .and_then(|mut r| r.snapshot(id))
        .map(|s| s.is_some())
        .map_err(error)
}
pub fn refuse_shared_run(data: &Path, id: &str) -> Result<(), String> {
    if shared_run(data, id)? {
        Err("Run belongs to shared runtime; legacy writes are disabled".into())
    } else {
        Ok(())
    }
}
pub fn refuse_shared_path(data: &Path, path: &Path) -> Result<(), String> {
    let base = data.join("runtime-v1.native");
    if !base.exists() {
        return Ok(());
    }
    let base = base.canonicalize().map_err(error)?;
    let actual = path.canonicalize().map_err(error)?;
    if actual.starts_with(base) {
        Err("Shared runtime workspace cannot be mutated through legacy IPC".into())
    } else {
        Ok(())
    }
}
pub fn raw_pipeline(cwd: &Path, id: &str) -> Result<Option<PipelineState>, String> {
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("Invalid legacy session identity".into());
    }
    let path = cwd.join(".tokage/execution").join(format!("{id}.json"));
    if !path.exists() {
        return Ok(None);
    }
    if std::fs::symlink_metadata(&path)
        .map_err(error)?
        .file_type()
        .is_symlink()
    {
        return Err("Symlinked legacy pipeline is read-only".into());
    }
    let state: PipelineState =
        serde_json::from_slice(&std::fs::read(path).map_err(error)?).map_err(error)?;
    if state.session_id != id || ![1, 2].contains(&state.schema_version) {
        return Err("Unknown legacy schema or mismatched identity".into());
    }
    Ok(Some(state))
}
pub fn mock_workspace(data: &Path, cwd: &Path) -> bool {
    if !tokage_native::chat::debug::is_mock_workspace(cwd) {
        return false;
    }
    let Ok(path) = cwd.canonicalize() else {
        return false;
    };
    [data.join("debug-workspaces"), data.join("worktrees")]
        .iter()
        .filter_map(|p| p.canonicalize().ok())
        .any(|p| path.starts_with(p))
}
pub fn legacy_write(data: &Path, cwd: &Path, id: &str) -> Result<(), String> {
    refuse_shared_run(data, id)?;
    refuse_shared_path(data, cwd)?;
    if raw_pipeline(cwd, id)?.is_some() || mock_workspace(data, cwd) {
        Ok(())
    } else {
        Err("Creating legacy orchestration is disabled; import the plan into shared runtime".into())
    }
}
pub fn session_paths(conn: &rusqlite::Connection, id: &str) -> Result<Vec<PathBuf>, String> {
    use rusqlite::OptionalExtension;
    let row:Option<(String,Option<String>)>=conn.query_row("SELECT p.local_path,s.worktree_json FROM main_sessions s JOIN projects p ON p.id=s.project_id WHERE s.id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(error)?;
    let mut paths = Vec::new();
    if let Some((project, worktree)) = row {
        paths.push(PathBuf::from(project));
        if let Some(text) = worktree {
            let value: serde_json::Value = serde_json::from_str(&text).map_err(error)?;
            if let Some(cwd) = value["cwd"].as_str() {
                paths.push(PathBuf::from(cwd));
            }
        }
    }
    Ok(paths)
}
pub fn legacy_session(
    data: &Path,
    conn: &rusqlite::Connection,
    id: &str,
    additional: Option<&Path>,
) -> Result<(), String> {
    refuse_shared_run(data, id)?;
    let mut paths = session_paths(conn, id)?;
    if let Some(path) = additional {
        paths.push(path.into());
    }
    for path in paths {
        if raw_pipeline(&path, id)?.is_some() || mock_workspace(data, &path) {
            return Ok(());
        }
    }
    Err("Legacy run has no existing pipeline; shared runtime is required".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_creation_is_closed_and_raw_queries_do_not_recover_or_rewrite() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().join("repo");
        std::fs::create_dir(&cwd).unwrap();
        let data = dir.path().join("data");
        assert!(legacy_write(&data, &cwd, "session").is_err());
        assert!(!cwd.join(".tokage").exists());
        let path = cwd.join(".tokage/execution");
        std::fs::create_dir_all(&path).unwrap();
        let path = path.join("session.json");
        let original=serde_json::to_vec(&serde_json::json!({"schemaVersion":2,"sessionId":"session","status":"RUNNING","tasks":[],"startedAt":1})).unwrap();
        std::fs::write(&path, &original).unwrap();
        assert!(raw_pipeline(&cwd, "session").unwrap().is_some());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert!(legacy_write(&data, &cwd, "session").is_ok());
        std::fs::write(&path,serde_json::to_vec(&serde_json::json!({"schemaVersion":99,"sessionId":"session","status":"RUNNING","tasks":[],"startedAt":1})).unwrap()).unwrap();
        assert!(raw_pipeline(&cwd, "session").is_err());
    }
    #[test]
    fn runtime_workspaces_are_inaccessible_to_legacy_mutations() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = dir.path().join("runtime-v1.native/worktrees/run");
        std::fs::create_dir_all(&runtime).unwrap();
        let other = dir.path().join("other");
        std::fs::create_dir(&other).unwrap();
        assert!(refuse_shared_path(dir.path(), &runtime).is_err());
        assert!(refuse_shared_path(dir.path(), &other).is_ok());
    }
}
/// Serializes legacy metadata calls inside the app, then fences selection with the runtime OS lease.
/// Actual legacy model workers remain concurrent; a UI view never owns this guard.
pub struct LegacyLease {
    _file: tokage_native::file_lease::FileLease,
    _local: tokio::sync::OwnedMutexGuard<()>,
}
static LOCAL_LEASES: std::sync::OnceLock<
    tokio::sync::Mutex<std::collections::HashMap<PathBuf, std::sync::Weak<tokio::sync::Mutex<()>>>>,
> = std::sync::OnceLock::new();
pub async fn legacy_lease(data: &Path, id: &str) -> Result<LegacyLease, String> {
    let directory = data.join("runtime-v1");
    std::fs::create_dir_all(&directory).map_err(error)?;
    let directory = directory.canonicalize().map_err(error)?;
    let lock = directory.join(format!(
        "runtime-{}.lock",
        tokage_runtime::digest(&id).map_err(error)?
    ));
    let mutex = {
        let mut locks = LOCAL_LEASES
            .get_or_init(|| tokio::sync::Mutex::new(Default::default()))
            .lock()
            .await;
        locks.retain(|_, w| w.strong_count() > 0);
        if let Some(m) = locks.get(&lock).and_then(std::sync::Weak::upgrade) {
            m
        } else {
            let m = std::sync::Arc::new(tokio::sync::Mutex::new(()));
            locks.insert(lock.clone(), std::sync::Arc::downgrade(&m));
            m
        }
    };
    let local = mutex.lock_owned().await;
    let file = tokage_native::file_lease::FileLease::acquire(&lock)
        .map_err(|e| format!("Engine owner conflict: {e}"))?;
    refuse_shared_run(data, id)?;
    Ok(LegacyLease {
        _file: file,
        _local: local,
    })
}
pub async fn legacy_write_lease(data: &Path, cwd: &Path, id: &str) -> Result<LegacyLease, String> {
    let lease = legacy_lease(data, id).await?;
    legacy_write(data, cwd, id)?;
    Ok(lease)
}
pub fn refuse_shared_reference(reference: &str) -> Result<(), String> {
    if reference
        .trim_start_matches("refs/heads/")
        .starts_with("tokage/rt-")
    {
        Err("Runtime integration/task branches require shared delivery commands".into())
    } else {
        Ok(())
    }
}
#[cfg(test)]
mod owner_tests {
    use super::*;
    use tokage_runtime::{Command, Repository, Request, SCHEMA_VERSION};
    #[tokio::test]
    async fn engine_selection_uses_the_same_owner_lock_and_legacy_calls_serialize() {
        let dir = tempfile::tempdir().unwrap();
        let first = legacy_lease(dir.path(), "shared").await.unwrap();
        assert!(matches!(
            tokage_runtime::repository::SqliteRepository::open(
                &dir.path().join("runtime-v1"),
                "shared"
            ),
            Err(tokage_runtime::Error::OwnerConflict)
        ));
        let second = legacy_lease(dir.path(), "shared");
        tokio::pin!(second);
        tokio::select! {result=&mut second=>panic!("legacy calls must serialize: {}",result.is_ok()),_=tokio::time::sleep(std::time::Duration::from_millis(20))=>{}}
        drop(first);
        drop(second.await.unwrap());
        let mut repo = tokage_runtime::repository::SqliteRepository::open(
            &dir.path().join("runtime-v1"),
            "shared",
        )
        .unwrap();
        let config=serde_json::from_value(serde_json::json!({"source":"unit","policyVersion":"1","workspace":dir.path().join("repo"),"targetIdentity":"sha","scope":["**"],"models":{"executor":{"provider":"CODEX_CLI","requestedModel":"explicit","providerPath":null,"capabilities":[],"userLocked":true}},"commandPolicyHash":"policy","trustWorkspace":true,"permissionMode":"full-auto","concurrency":1,"budgetUsd":null,"blockUnknownCost":false,"timeoutSeconds":60,"repairPolicy":"manual","contextPolicy":"off","confirmedCommands":[],"customPackHashes":[],"coAuthor":false})).unwrap();
        repo.submit(&Request {
            schema_version: SCHEMA_VERSION,
            command_id: "create".into(),
            run_id: "shared".into(),
            expected_revision: 0,
            command: Command::CreateRun {
                requirement: "user goal".into(),
                config,
            },
        })
        .await
        .unwrap();
        assert!(legacy_lease(dir.path(), "shared").await.is_err());
        drop(repo);
        assert!(legacy_lease(dir.path(), "shared").await.is_err()); // persisted engine binding outlives its owner
        assert!(refuse_shared_reference("refs/heads/tokage/rt-i-source").is_err());
        assert!(refuse_shared_reference("refs/heads/user-branch").is_ok());
    }
}
