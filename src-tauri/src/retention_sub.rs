//! F17.b: idle retention cleanup scheduler (docs/08).
//!
//! First run ~60s after DB open, then every 24h. Logs deleted counts via `tracing::info!`.

use std::time::Duration;

use tauri::{AppHandle, Manager};
use tokage_storage as storage;

const INITIAL_DELAY_SECS: u64 = 60;
const INTERVAL_SECS: u64 = 24 * 60 * 60;

/// Spawn low-priority retention cleanup loop. Safe to call once from `setup`.
pub fn spawn_retention_loop(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(INITIAL_DELAY_SECS)).await;
        loop {
            run_once(&app).await;
            tokio::time::sleep(Duration::from_secs(INTERVAL_SECS)).await;
        }
    });
}

async fn run_once(app: &AppHandle) {
    let Some(state) = app.try_state::<storage::StorageState>() else {
        return;
    };
    let Ok(db) = state.get() else {
        return;
    };
    match db.cleanup_retention_records().await {
        Ok((logs, events)) => {
            tracing::info!(
                deleted_chat_logs = logs,
                deleted_usage_or_savings = events,
                "retention cleanup finished"
            );
        }
        Err(e) => tracing::warn!("retention cleanup failed: {e}"),
    }
}
