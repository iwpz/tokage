//! Local database (docs/08): `app_data_dir()/tokage.db`, SQLite in WAL mode.
//!
//! Writes go through one dedicated thread that commits whatever arrives within 100ms (at most
//! 200 jobs) as one transaction, each job in its own savepoint so a failing job doesn't take
//! the others down. Reads use a small connection pool. Migrations run at open; when any are
//! pending on an existing database, a copy is written to `backups/` first.

mod repo;
mod rules;
mod writer;
pub mod approvals;
pub mod knowledge;
pub mod learning;
pub mod orchestration;

use std::path::{Path, PathBuf};

use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;
use tokage_core::command_policy::Rule;

pub use approvals::ApprovalRecord;
pub use knowledge::{ModelKnowledgeRow, ModelOfferingRow, ModelOverrideRow};
pub use learning::{LearnedStatRow, TaskOutcomeRow};
pub use orchestration::{BriefRow, GateRunRow, TaskReportRow};
pub use repo::{SettingRow, Snapshot, UsageEventRow, WriteBatch};

mod embedded {
    refinery::embed_migrations!("./migrations");
}

const PRAGMAS: &str = "PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;";

const DB_FILE: &str = "tokage.db";
const BACKUP_DIR: &str = "backups";
/// Pre-migration copies kept; older ones are deleted.
const BACKUPS_KEPT: usize = 5;
const READ_POOL: u32 = 4;

pub struct Storage {
    pool: r2d2::Pool<SqliteConnectionManager>,
    writer: writer::Writer,
}

/// Managed by Tauri. Opening can fail (disk, permissions); the frontend then keeps working
/// from its legacy data in memory and the error is shown.
pub struct StorageState(pub Result<Storage, String>);

impl StorageState {
    pub fn get(&self) -> Result<&Storage, String> {
        self.0.as_ref().map_err(|e| format!("Database unavailable: {e}"))
    }
}

fn open_conn(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(PRAGMAS)?;
    Ok(conn)
}

impl Storage {
    /// Opens (creating if needed) `<dir>/tokage.db`, backs it up if migrations are pending,
    /// and migrates it.
    pub fn open(dir: &Path) -> Result<Storage, String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let path = dir.join(DB_FILE);
        let existed = path.exists();
        let mut conn = open_conn(&path).map_err(|e| e.to_string())?;
        if existed {
            if let Some(current) = pending_from(&conn).map_err(|e| e.to_string())? {
                backup(&conn, &dir.join(BACKUP_DIR), current)?;
            }
        }
        embedded::migrations::runner()
            .run(&mut conn)
            .map_err(|e| format!("Migration failed: {e}"))?;
        let manager = SqliteConnectionManager::file(&path).with_init(|c| c.execute_batch(PRAGMAS));
        let pool = r2d2::Pool::builder()
            .max_size(READ_POOL)
            .build(manager)
            .map_err(|e| e.to_string())?;
        Ok(Storage {
            pool,
            writer: writer::Writer::start(conn),
        })
    }

    /// Everything the frontend restores at startup.
    pub fn load(&self) -> Result<Snapshot, String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        repo::load(&conn).map_err(|e| e.to_string())
    }

    /// Queues a batch; returns once it is queued. Failures are logged by the writer.
    pub fn write(&self, batch: WriteBatch) -> Result<(), String> {
        for job in repo::jobs(batch) {
            self.writer.send(job)?;
        }
        Ok(())
    }

    /// One-time import of the localStorage data: one transaction, sets `meta.legacyImported`
    /// on success. Never deletes rows, so a retry after a partial session is safe.
    pub async fn import(&self, batch: WriteBatch) -> Result<(), String> {
        self.writer.call(repo::import_job(batch)).await
    }

    pub fn command_rules(&self) -> Result<Vec<Rule>, String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        rules::list(&conn).map_err(|e| e.to_string())
    }

    /// Adds a rule (or changes the level of the same rule); resolves with its id once committed.
    pub async fn upsert_command_rule(&self, rule: Rule) -> Result<i64, String> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.writer
            .call(Box::new(move |c| {
                let id = rules::upsert(c, &rule)?;
                let _ = tx.send(id);
                Ok(())
            }))
            .await?;
        rx.try_recv().map_err(|_| "Rule was not stored".to_string())
    }

    pub async fn delete_command_rule(&self, id: i64) -> Result<(), String> {
        self.writer.call(Box::new(move |c| rules::delete(c, id))).await
    }

    pub fn approvals(&self, session_id: Option<&str>) -> Result<Vec<ApprovalRecord>, String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        approvals::list(&conn, session_id).map_err(|e| e.to_string())
    }

    pub async fn record_approval(&self, rec: ApprovalRecord) -> Result<i64, String> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.writer
            .call(Box::new(move |c| {
                let id = approvals::insert(c, &rec)?;
                let _ = tx.send(id);
                Ok(())
            }))
            .await?;
        rx.try_recv().map_err(|_| "Approval was not recorded".to_string())
    }

    pub fn usage_events(&self, session_id: Option<&str>, plan_id: Option<i64>) -> Result<Vec<UsageEventRow>, String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        repo::list_usage_events(&conn, session_id, plan_id).map_err(|e| e.to_string())
    }

    pub async fn record_usage(&self, event: UsageEventRow) -> Result<i64, String> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.writer
            .call(Box::new(move |c| {
                let id = repo::insert_usage_event(c, &event)?;
                let _ = tx.send(id);
                Ok(())
            }))
            .await?;
        rx.try_recv().map_err(|_| "Usage event was not stored".to_string())
    }

    /// Atomically persists a full knowledge pack into the SQLite knowledge tables (P6.m1c).
    pub async fn save_knowledge_pack(&self, pack: tokage_matching::KnowledgePack) -> Result<(), String> {
        self.writer
            .call(Box::new(move |c| {
                knowledge::save_pack(c, &pack)?;
                Ok(())
            }))
            .await
    }

    /// Lists all models in the knowledge base.
    pub fn list_model_knowledge(&self) -> Result<Vec<ModelKnowledgeRow>, String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        knowledge::list_models(&conn).map_err(|e| e.to_string())
    }

    /// Lists all CLI model offerings in the knowledge base.
    pub fn list_model_offerings(&self) -> Result<Vec<ModelOfferingRow>, String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        knowledge::list_offerings(&conn).map_err(|e| e.to_string())
    }

    /// Gets domain skills for a specific model.
    pub fn get_model_skills(&self, model_id: &str) -> Result<Option<std::collections::HashMap<String, f64>>, String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        knowledge::get_model_skills(&conn, model_id).map_err(|e| e.to_string())
    }

    /// Gets the success model requirements and sigmoid parameters.
    pub fn get_success_model(&self) -> Result<Option<tokage_matching::SuccessModel>, String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        knowledge::get_success_model(&conn).map_err(|e| e.to_string())
    }

    /// Gets a raw JSON blob from knowledge_blobs by key.
    pub fn get_knowledge_blob(&self, key: &str) -> Result<Option<String>, String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        knowledge::get_knowledge_blob(&conn, key).map_err(|e| e.to_string())
    }

    /// Upserts a user model override.
    pub async fn upsert_model_override(&self, provider_name: String, model_id: String, patch_json: String) -> Result<(), String> {
        self.writer
            .call(Box::new(move |c| {
                knowledge::upsert_override(c, &provider_name, &model_id, &patch_json)?;
                Ok(())
            }))
            .await
    }

    /// Lists all active model overrides.
    pub fn list_model_overrides(&self) -> Result<Vec<ModelOverrideRow>, String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        knowledge::list_overrides(&conn).map_err(|e| e.to_string())
    }

    /// Deletes a user model override.
    pub async fn delete_model_override(&self, provider_name: String, model_id: String) -> Result<(), String> {
        self.writer
            .call(Box::new(move |c| {
                knowledge::delete_override(c, &provider_name, &model_id)?;
                Ok(())
            }))
            .await
    }

    /// Records a completed task execution outcome (docs/04 §9, P6.m6a).
    pub async fn record_task_outcome(&self, outcome: TaskOutcomeRow) -> Result<i64, String> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.writer
            .call(Box::new(move |c| {
                let id = learning::insert_task_outcome(c, &outcome)?;
                let _ = tx.send(id);
                Ok(())
            }))
            .await?;
        rx.try_recv().map_err(|_| "Task outcome was not stored".to_string())
    }

    /// Lists task outcomes with optional model or domain filters.
    pub fn list_task_outcomes(
        &self,
        model_id: Option<&str>,
        domain: Option<&str>,
        limit: u32,
    ) -> Result<Vec<TaskOutcomeRow>, String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        learning::list_task_outcomes(&conn, model_id, domain, limit).map_err(|e| e.to_string())
    }

    /// Records a task report (docs/03 §4.2, P7.m2b).
    pub async fn record_task_report(&self, report: TaskReportRow) -> Result<i64, String> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.writer
            .call(Box::new(move |c| {
                let id = orchestration::insert_task_report(c, &report)?;
                let _ = tx.send(id);
                Ok(())
            }))
            .await?;
        rx.try_recv().map_err(|_| "Task report was not stored".to_string())
    }

    /// Gets task reports for a task.
    pub fn get_task_reports(&self, task_id: &str) -> Result<Vec<TaskReportRow>, String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        orchestration::get_task_reports(&conn, task_id).map_err(|e| e.to_string())
    }

    /// Records a task brief or progress report (docs/05, P7.m2a).
    pub async fn record_brief(&self, brief: BriefRow) -> Result<i64, String> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.writer
            .call(Box::new(move |c| {
                let id = orchestration::insert_brief(c, &brief)?;
                let _ = tx.send(id);
                Ok(())
            }))
            .await?;
        rx.try_recv().map_err(|_| "Brief was not stored".to_string())
    }

    /// Gets the latest brief for a task.
    pub fn get_latest_brief(&self, task_id: &str) -> Result<Option<BriefRow>, String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        orchestration::get_latest_brief(&conn, task_id).map_err(|e| e.to_string())
    }

    /// Records an automated gate run execution (docs/03 §5, docs/08 schema.md §4, P7.m3).
    pub async fn record_gate_run(&self, run: GateRunRow) -> Result<i64, String> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.writer
            .call(Box::new(move |c| {
                let id = orchestration::insert_gate_run(c, &run)?;
                let _ = tx.send(id);
                Ok(())
            }))
            .await?;
        rx.try_recv().map_err(|_| "Gate run was not stored".to_string())
    }

    /// Lists gate runs with optional task or plan filters.
    pub fn list_gate_runs(&self, task_id: Option<&str>, plan_id: Option<&str>) -> Result<Vec<GateRunRow>, String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        orchestration::list_gate_runs(&conn, task_id, plan_id).map_err(|e| e.to_string())
    }

    /// Upserts a learned statistic row.
    pub async fn update_learned_stat(&self, stat: LearnedStatRow) -> Result<(), String> {
        self.writer
            .call(Box::new(move |c| {
                learning::upsert_learned_stat(c, &stat)?;
                Ok(())
            }))
            .await
    }

    /// Retrieves learned stats for a model across domains and metrics.
    pub fn list_learned_stats(&self, model_id: &str) -> Result<Vec<LearnedStatRow>, String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        learning::list_learned_stats_for_model(&conn, model_id).map_err(|e| e.to_string())
    }

    /// Waits until every queued write is committed (app exit).
    pub fn flush_blocking(&self) {
        self.writer.flush_blocking();
    }
}

/// Current schema version when the database is behind the embedded migrations.
fn pending_from(conn: &Connection) -> rusqlite::Result<Option<i64>> {
    let has_table: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='refinery_schema_history')",
        [],
        |r| r.get(0),
    )?;
    let current: i64 = if has_table {
        conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM refinery_schema_history",
            [],
            |r| r.get(0),
        )?
    } else {
        0
    };
    let latest = embedded::migrations::runner()
        .get_migrations()
        .iter()
        .map(|m| i64::from(m.version()))
        .max()
        .unwrap_or(0);
    Ok((current < latest).then_some(current))
}

/// `VACUUM INTO` gives a consistent copy including whatever is still in the WAL.
fn backup(conn: &Connection, dir: &Path, version: i64) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let stamp: String = conn
        .query_row("SELECT strftime('%Y%m%dT%H%M%S','now')", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let target = dir.join(format!("tokage-v{version}-{stamp}.db"));
    if target.exists() {
        std::fs::remove_file(&target).map_err(|e| e.to_string())?;
    }
    conn.execute("VACUUM INTO ?1", [target.to_string_lossy()])
        .map_err(|e| format!("Backup before migration failed: {e}"))?;
    prune_backups(dir);
    Ok(target)
}

fn prune_backups(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("tokage-v"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    for (_, p) in files.into_iter().skip(BACKUPS_KEPT) {
        let _ = std::fs::remove_file(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_database_gets_all_tables() {
        let dir = tempfile::tempdir().unwrap();
        let s = Storage::open(dir.path()).unwrap();
        let conn = s.pool.get().unwrap();
        for t in [
            "projects",
            "main_sessions",
            "attempts",
            "chat_logs",
            "usage_events",
            "provider_health",
            "custom_packs",
            "settings",
            "command_rules",
        ] {
            let n: i64 = conn
                .query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1", [t], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(n, 1, "{t}");
        }
        let mode: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
        assert_eq!(mode, "wal");
        // Fresh database: nothing to back up.
        assert!(!dir.path().join(BACKUP_DIR).exists());
    }

    #[test]
    fn existing_database_behind_schema_is_backed_up_first() {
        let dir = tempfile::tempdir().unwrap();
        {
            let conn = open_conn(&dir.path().join(DB_FILE)).unwrap();
            conn.execute_batch("CREATE TABLE keep (v TEXT); INSERT INTO keep VALUES ('old');")
                .unwrap();
        }
        Storage::open(dir.path()).unwrap();
        let backups: Vec<_> = std::fs::read_dir(dir.path().join(BACKUP_DIR))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(backups.len(), 1);
        let copy = Connection::open(backups[0].path()).unwrap();
        let v: String = copy.query_row("SELECT v FROM keep", [], |r| r.get(0)).unwrap();
        assert_eq!(v, "old");

        // Up to date now: reopening makes no further copy.
        Storage::open(dir.path()).unwrap();
        assert_eq!(std::fs::read_dir(dir.path().join(BACKUP_DIR)).unwrap().count(), 1);
    }
}
