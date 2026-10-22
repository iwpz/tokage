//! Local process host used by transports. Waiting is observation, never a second scheduler.
use crate::{
    owned::Runtime,
    repository::{Reader, SqliteRepository},
    service::NativePlanner,
    *,
};
use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
pub type NativeRuntime = Runtime<NativePlanner>;
pub fn identifier() -> String {
    format!(
        "r-{:x}-{:x}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    )
}
pub fn request(s: &Snapshot, command: Command) -> Request {
    Request {
        schema_version: SCHEMA_VERSION,
        command_id: identifier(),
        run_id: s.run_id.clone(),
        expected_revision: s.revision,
        command,
    }
}
pub fn open(
    directory: &Path,
    id: &str,
    env: tokage_native::env::ShellEnv,
) -> Result<NativeRuntime> {
    Runtime::new(
        SqliteRepository::open(directory, id)?,
        id,
        NativePlanner::new(env, &directory.with_extension("native"))?,
    )
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Cancellation {
    run_id: String,
    config_hash: String,
    revision: u64,
}
fn cancel_path(directory: &Path, id: &str) -> Result<PathBuf> {
    Ok(directory.join(format!("cancel-{}.json", digest(&id)?)))
}
/// A cross-process cancellation intent, not a state write or proof that children exited.
/// The lease owner applies CancelRun using its current revision. All other external mutations
/// still require the exclusive owner lease.
pub fn request_cancel(directory: &Path, id: &str) -> Result<()> {
    let s = Reader::open(directory)?
        .snapshot(id)?
        .ok_or(Error::Transition)?;
    if !matches!(
        s.state,
        RunState::Planning
            | RunState::Executing
            | RunState::Reviewing
            | RunState::AwaitingReview
            | RunState::AwaitingTurnApproval
            | RunState::AwaitingMerge
            | RunState::CancelRequested
            | RunState::Stopping
    ) {
        return Err(Error::Transition);
    }
    let cancellation = Cancellation {
        run_id: id.into(),
        config_hash: s.config_hash,
        revision: s.revision,
    };
    let path = cancel_path(directory, id)?;
    let temp = directory.join(format!("{}.tmp", identifier()));
    let persist = || -> Result<()> {
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut f = options
            .open(&temp)
            .map_err(|e| Error::Repository(e.to_string()))?;
        f.write_all(&serde_json::to_vec(&cancellation).map_err(|e| Error::Invalid(e.to_string()))?)
            .and_then(|_| f.sync_all())
            .map_err(|e| Error::Repository(e.to_string()))?;
        std::fs::rename(&temp, &path).map_err(|e| Error::Repository(e.to_string()))?;
        #[cfg(unix)]
        std::fs::File::open(directory)
            .and_then(|f| f.sync_all())
            .map_err(|e| Error::Repository(e.to_string()))?;
        Ok(())
    };
    let result = persist();
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result
}
/// CLI lifetime wait. Desktop callers retain Runtime handles in application state instead.
/// Polls durable cancellation intents; native execution and all transitions remain in Runtime.
pub async fn wait(runtime: &NativeRuntime, directory: &Path) -> Result<Snapshot> {
    loop {
        if let Some(error) = runtime.worker_error() {
            if !runtime.busy() {
                return Err(error);
            }
        }
        if poll_cancellation(runtime, directory).await? {
            continue;
        }
        if !runtime.busy() {
            if let Some(error) = runtime.worker_error() {
                return Err(error);
            }
            return runtime.snapshot().await?.ok_or(Error::Transition);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Consume only explicit durable cancellation intents. Safe to run without any view attached.
pub async fn poll_cancellation(runtime: &NativeRuntime, directory: &Path) -> Result<bool> {
    let path = cancel_path(directory, runtime.run_id())?;
    if path.exists() {
        let s = runtime.snapshot().await?.ok_or(Error::Transition)?;
        let text = std::fs::read(&path).map_err(|e| Error::Repository(e.to_string()))?;
        let c: Cancellation =
            serde_json::from_slice(&text).map_err(|e| Error::Invalid(e.to_string()))?;
        // Stale requests cannot cancel a different config or an earlier revision.
        if c.run_id == s.run_id
            && c.config_hash == s.config_hash
            && c.revision <= s.revision
            && matches!(
                s.state,
                RunState::Planning
                    | RunState::Executing
                    | RunState::Reviewing
                    | RunState::AwaitingReview
                    | RunState::AwaitingTurnApproval
                    | RunState::AwaitingMerge
            )
        {
            match runtime.apply_cancellation(&c.config_hash, c.revision).await {
                Ok(_) | Err(Error::Transition) => {}
                Err(error) => return Err(error),
            }
        }
        // Remove only the exact intent observed (a concurrent newer intent stays queued).
        if std::fs::read(&path).ok().as_deref() == Some(text.as_slice()) {
            std::fs::remove_file(path).map_err(|e| Error::Repository(e.to_string()))?;
        }
        return Ok(true);
    }
    Ok(false)
}
