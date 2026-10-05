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
pub mod context;
pub mod health;
pub mod knowledge;
pub mod learning;
pub mod orchestration;
pub mod savings;

use std::path::{Path, PathBuf};

use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Connection, OptionalExtension};
use tokage_core::command_policy::Rule;

pub use approvals::ApprovalRecord;
pub use context::ContextActionRow;
pub use health::{MeltReason, ProviderHealthRow};
pub use knowledge::{ModelKnowledgeRow, ModelOfferingRow, ModelOverrideRow};
pub use learning::{LearnedStatRow, TaskOutcomeRow};
pub use orchestration::{AttemptRow, BriefRow, GateRunRow, TaskReportRow};
pub use repo::{SettingRow, Snapshot, UsageEventRow, WriteBatch};
pub use savings::{SavingsEventRow, UsageSnapshotRow};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatLogRow {
    pub id: i64,
    pub message_id: String,
    pub main_session_id: String,
    pub attempt_id: String,
    pub seq: i64,
    pub turn_index: i64,
    pub role: String,
    pub content: String,
    pub content_truncated: bool,
    pub meta_json: String,
    pub created_at: String,
}

mod embedded {
    refinery::embed_migrations!("./migrations");
}

pub fn run_migrations(conn: &mut Connection) -> Result<(), refinery::Error> {
    embedded::migrations::runner().run(conn)?;
    Ok(())
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

    /// Obtains a pooled read connection.
    pub fn get_conn(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, String> {
        self.pool.get().map_err(|e| e.to_string())
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

    /// Atomically reloads the official knowledge pack into SQLite without wiping custom packs or overrides (P8.m4a).
    pub async fn reload_official_knowledge_pack(&self, pack: tokage_matching::KnowledgePack) -> Result<(), String> {
        self.writer
            .call(Box::new(move |c| {
                knowledge::reload_official_pack(c, &pack)?;
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

    /// Inserts a plan record into `plans` table and returns plan ID (docs/08 schema.md §2, P8.m2e).
    pub async fn insert_plan_record(
        &self,
        main_session_id: String,
        version: u32,
        goal: String,
        spec_json: String,
        est_cost_usd: f64,
        status: String,
    ) -> Result<i64, String> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.writer
            .call(Box::new(move |c| {
                let id = orchestration::insert_plan_record(c, &main_session_id, version, &goal, &spec_json, est_cost_usd, &status)?;
                let _ = tx.send(id);
                Ok(())
            }))
            .await?;
        rx.try_recv().map_err(|_| "Plan was not stored".to_string())
    }

    /// Retrieves a plan record by ID.
    pub fn get_plan_record(&self, plan_id: i64) -> Result<Option<(i64, String, u32, String, String, f64, String)>, String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        orchestration::get_plan_record(&conn, plan_id).map_err(|e| e.to_string())
    }

    /// Updates status of a plan record.
    pub async fn update_plan_status(&self, plan_id: i64, status: String) -> Result<(), String> {
        self.writer
            .call(Box::new(move |c| {
                orchestration::update_plan_status(c, plan_id, &status)?;
                Ok(())
            }))
            .await
    }

    /// Updates task actual cost in a plan.
    pub async fn update_task_actual_cost(&self, plan_id: i64, task_code: String, actual_cost_usd: f64) -> Result<(), String> {
        self.writer
            .call(Box::new(move |c| {
                orchestration::update_task_actual_cost(c, plan_id, &task_code, actual_cost_usd)?;
                Ok(())
            }))
            .await
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

    /// Records or updates an execution attempt (docs/08 schema.md §3, P7.m6a).
    pub async fn upsert_attempt(&self, att: AttemptRow) -> Result<(), String> {
        self.writer
            .call(Box::new(move |c| {
                orchestration::upsert_attempt(c, &att)?;
                Ok(())
            }))
            .await
    }

    /// Lists attempts for a session.
    pub fn list_attempts(&self, session_id: &str) -> Result<Vec<AttemptRow>, String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        orchestration::list_attempts_for_session(&conn, session_id).map_err(|e| e.to_string())
    }

    /// Records a context action (docs/08 schema.md §4, P8.m1f).
    pub async fn record_context_action(&self, action: ContextActionRow) -> Result<i64, String> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.writer
            .call(Box::new(move |c| {
                let id = context::insert_context_action(c, &action)?;
                let _ = tx.send(id);
                Ok(())
            }))
            .await?;
        rx.try_recv().map_err(|_| "Context action was not stored".to_string())
    }

    /// Updates context action status (SUGGESTED -> APPLIED / DISMISSED).
    pub async fn update_context_action_status(
        &self,
        action_id: i64,
        new_status: String,
        new_attempt_id: Option<String>,
    ) -> Result<(), String> {
        self.writer
            .call(Box::new(move |c| {
                context::update_context_action_status(c, action_id, &new_status, new_attempt_id.as_deref())?;
                Ok(())
            }))
            .await
    }

    /// Settles the actual savings for a context action.
    pub async fn settle_context_action_savings(
        &self,
        action_id: i64,
        actual_saving_usd: f64,
    ) -> Result<(), String> {
        self.writer
            .call(Box::new(move |c| {
                context::settle_context_action_savings(c, action_id, actual_saving_usd)?;
                Ok(())
            }))
            .await
    }

    /// Lists context actions for an attempt.
    pub fn list_context_actions(&self, attempt_id: &str) -> Result<Vec<ContextActionRow>, String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        context::list_context_actions(&conn, attempt_id).map_err(|e| e.to_string())
    }

    /// Gets a single context action by ID.
    pub fn get_context_action(&self, action_id: i64) -> Result<Option<ContextActionRow>, String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        context::get_context_action(&conn, action_id).map_err(|e| e.to_string())
    }

    /// Updates main session status (docs/07 §3, P7.m6b).
    pub async fn update_main_session_status(&self, session_id: String, status: String) -> Result<(), String> {
        self.writer
            .call(Box::new(move |c| {
                orchestration::update_main_session_status(c, &session_id, &status)?;
                Ok(())
            }))
            .await
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

    /// Retrieves health record for a provider (docs/06 §6).
    pub fn get_provider_health(&self, provider_name: &str) -> Result<Option<ProviderHealthRow>, String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        health::get_provider_health(&conn, provider_name).map_err(|e| e.to_string())
    }

    /// Lists health records for all providers.
    pub fn list_provider_health(&self) -> Result<Vec<ProviderHealthRow>, String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        health::list_provider_health(&conn).map_err(|e| e.to_string())
    }

    /// Trips circuit breaker for a provider (docs/06 §6.1).
    pub async fn trip_provider_melt(
        &self,
        provider_name: String,
        reason: MeltReason,
        melted_until: Option<String>,
    ) -> Result<(), String> {
        self.writer
            .call(Box::new(move |c| {
                health::trip_provider_melt(c, &provider_name, reason, melted_until.as_deref())?;
                Ok(())
            }))
            .await
    }

    /// Records failure for a provider and returns updated consecutive failure count.
    pub async fn record_provider_failure(&self, provider_name: String) -> Result<i64, String> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.writer
            .call(Box::new(move |c| {
                let fails = health::record_provider_failure(c, &provider_name)?;
                let _ = tx.send(fails);
                Ok(())
            }))
            .await?;
        rx.try_recv().map_err(|_| "Failed to record failure".to_string())
    }

    /// Resets consecutive failures on successful execution.
    pub async fn reset_provider_failures(&self, provider_name: String) -> Result<(), String> {
        self.writer
            .call(Box::new(move |c| {
                health::reset_provider_failures(c, &provider_name)?;
                Ok(())
            }))
            .await
    }

    /// Recovers a melted provider back to HEALTHY / READY (docs/06 §6.3).
    pub async fn recover_provider_health(&self, provider_name: String, new_status: Option<String>) -> Result<(), String> {
        self.writer
            .call(Box::new(move |c| {
                health::recover_provider_health(c, &provider_name, new_status.as_deref())?;
                Ok(())
            }))
            .await
    }

    /// Transitions a provider to HALF_OPEN probe state.
    pub async fn set_provider_half_open(&self, provider_name: String) -> Result<(), String> {
        self.writer
            .call(Box::new(move |c| {
                health::set_provider_half_open(c, &provider_name)?;
                Ok(())
            }))
            .await
    }

    /// Records a savings event in savings_events (P8.m3a).
    pub async fn record_savings_event(&self, event: SavingsEventRow) -> Result<i64, String> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.writer
            .call(Box::new(move |c| {
                let id = savings::insert_savings_event(c, &event)?;
                let _ = tx.send(id);
                Ok(())
            }))
            .await?;
        rx.await.map_err(|e| format!("Channel dropped: {e}"))
    }

    /// Settles a savings event with the final actual cost.
    pub async fn settle_savings_event(&self, id: i64, actual_usd: f64) -> Result<(), String> {
        self.writer
            .call(Box::new(move |c| {
                savings::settle_savings_event(c, id, actual_usd)?;
                Ok(())
            }))
            .await
    }

    /// Lists savings events with optional range.
    pub fn list_savings_events(&self, range: Option<String>) -> Result<Vec<SavingsEventRow>, String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        savings::list_savings_events(&conn, range.as_deref()).map_err(|e| e.to_string())
    }

    /// Gets summary of savings (total and by kind).
    pub fn get_savings_summary(&self, range: Option<String>) -> Result<(f64, std::collections::HashMap<String, f64>), String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        savings::get_savings_summary(&conn, range.as_deref()).map_err(|e| e.to_string())
    }

    /// Inserts a usage snapshot (P8.m3b).
    pub async fn record_usage_snapshot(&self, snapshot: UsageSnapshotRow) -> Result<i64, String> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.writer
            .call(Box::new(move |c| {
                let id = savings::insert_usage_snapshot(c, &snapshot)?;
                let _ = tx.send(id);
                Ok(())
            }))
            .await?;
        rx.await.map_err(|e| format!("Channel dropped: {e}"))
    }

    /// Retrieves the latest usage snapshot for a provider.
    pub fn get_latest_usage_snapshot(&self, provider_name: String) -> Result<Option<UsageSnapshotRow>, String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        savings::get_latest_usage_snapshot(&conn, &provider_name).map_err(|e| e.to_string())
    }

    /// Retrieves paginated chat logs for an attempt (after a log ID) (P8.m5a).
    pub fn get_chat_logs(&self, attempt_id: &str, after_id: Option<i64>, limit: Option<i64>) -> Result<Vec<ChatLogRow>, String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        let limit = limit.unwrap_or(100).min(500);
        let mut sql = String::from(
            "SELECT id, message_id, main_session_id, attempt_id, seq, turn_index, role, content, content_truncated, meta_json, created_at
             FROM chat_logs WHERE attempt_id = ?1"
        );
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(attempt_id.to_string())];
        if let Some(after) = after_id {
            sql.push_str(" AND id > ?2");
            params.push(Box::new(after));
            sql.push_str(" ORDER BY id ASC LIMIT ?3");
            params.push(Box::new(limit));
        } else {
            sql.push_str(" ORDER BY id ASC LIMIT ?2");
            params.push(Box::new(limit));
        }

        let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params_refs.as_slice(), |r| {
                Ok(ChatLogRow {
                    id: r.get(0)?,
                    message_id: r.get(1)?,
                    main_session_id: r.get(2)?,
                    attempt_id: r.get(3)?,
                    seq: r.get(4)?,
                    turn_index: r.get(5)?,
                    role: r.get(6)?,
                    content: r.get(7)?,
                    content_truncated: r.get::<_, i64>(8)? != 0,
                    meta_json: r.get(9)?,
                    created_at: r.get(10)?,
                })
            })
            .map_err(|e| e.to_string())?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    /// Full-text search over chat logs using FTS5 trigram index (P8.m5a).
    pub fn search_chat_logs(&self, query: &str, session_id: Option<&str>, limit: Option<i64>) -> Result<Vec<ChatLogRow>, String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        let limit = limit.unwrap_or(50).min(200);
        let mut sql = String::from(
            "SELECT c.id, c.message_id, c.main_session_id, c.attempt_id, c.seq, c.turn_index, c.role, c.content, c.content_truncated, c.meta_json, c.created_at
             FROM chat_logs c
             JOIN chat_logs_fts f ON c.id = f.rowid
             WHERE chat_logs_fts MATCH ?1"
        );
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(query.to_string())];
        if let Some(sid) = session_id {
            sql.push_str(" AND c.main_session_id = ?2");
            params.push(Box::new(sid.to_string()));
            sql.push_str(" ORDER BY c.id DESC LIMIT ?3");
            params.push(Box::new(limit));
        } else {
            sql.push_str(" ORDER BY c.id DESC LIMIT ?2");
            params.push(Box::new(limit));
        }

        let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params_refs.as_slice(), |r| {
                Ok(ChatLogRow {
                    id: r.get(0)?,
                    message_id: r.get(1)?,
                    main_session_id: r.get(2)?,
                    attempt_id: r.get(3)?,
                    seq: r.get(4)?,
                    turn_index: r.get(5)?,
                    role: r.get(6)?,
                    content: r.get(7)?,
                    content_truncated: r.get::<_, i64>(8)? != 0,
                    meta_json: r.get(9)?,
                    created_at: r.get(10)?,
                })
            })
            .map_err(|e| e.to_string())?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    /// Performs idle retention cleanup (chat_logs > 90 days, events/snapshots > 365 days)
    /// in batches of 5000 followed by incremental vacuum (P8.m5a).
    pub async fn cleanup_retention_records(&self) -> Result<(usize, usize), String> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.writer
            .call(Box::new(move |c| {
                let mut deleted_logs = 0;
                let mut deleted_events = 0;

                // 1. Delete chat logs older than 90 days in batches of 5000
                loop {
                    let n = c.execute(
                        "DELETE FROM chat_logs WHERE id IN (
                            SELECT id FROM chat_logs
                            WHERE created_at < strftime('%Y-%m-%dT%H:%M:%fZ','now','-90 days')
                            LIMIT 5000
                        )",
                        [],
                    )?;
                    deleted_logs += n;
                    if n < 5000 {
                        break;
                    }
                }

                // 2. Delete usage_events and snapshots older than 365 days
                loop {
                    let n1 = c.execute(
                        "DELETE FROM usage_events WHERE id IN (
                            SELECT id FROM usage_events
                            WHERE created_at < strftime('%Y-%m-%dT%H:%M:%fZ','now','-365 days')
                            LIMIT 5000
                        )",
                        [],
                    )?;
                    let n2 = c.execute(
                        "DELETE FROM savings_events WHERE id IN (
                            SELECT id FROM savings_events
                            WHERE created_at < strftime('%Y-%m-%dT%H:%M:%fZ','now','-365 days')
                            LIMIT 5000
                        )",
                        [],
                    )?;
                    deleted_events += n1 + n2;
                    if n1 < 5000 && n2 < 5000 {
                        break;
                    }
                }

                // 3. Incremental vacuum to reclaim space without locking entire db
                let _ = c.execute_batch("PRAGMA incremental_vacuum;");

                let _ = tx.send((deleted_logs, deleted_events));
                Ok(())
            }))
            .await?;
        rx.await.map_err(|e| format!("Channel dropped: {e}"))
    }

    /// Exports all project data and sessions into a JSON structure (P8.m5b).
    pub fn export_project_data(&self, project_id: &str) -> Result<serde_json::Value, String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        let project_val: Option<serde_json::Value> = conn
            .query_row(
                "SELECT id, name, local_path, position, expanded, trusted, created_at FROM projects WHERE id = ?1",
                [project_id],
                |r| {
                    Ok(serde_json::json!({
                        "id": r.get::<_, String>(0)?,
                        "name": r.get::<_, String>(1)?,
                        "localPath": r.get::<_, String>(2)?,
                        "position": r.get::<_, i64>(3)?,
                        "expanded": r.get::<_, bool>(4)?,
                        "trusted": r.get::<_, Option<bool>>(5)?,
                        "createdAt": r.get::<_, String>(6)?,
                    }))
                },
            )
            .optional()
            .map_err(|e| e.to_string())?;

        let Some(project) = project_val else {
            return Err(format!("Project {project_id} not found"));
        };

        let mut stmt = conn
            .prepare("SELECT id, title, status, position, archived, forked_from, worktree_json, created_at, updated_at FROM main_sessions WHERE project_id = ?1")
            .map_err(|e| e.to_string())?;
        let session_rows = stmt
            .query_map([project_id], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, String>(0)?,
                    "title": r.get::<_, String>(1)?,
                    "status": r.get::<_, String>(2)?,
                    "position": r.get::<_, i64>(3)?,
                    "archived": r.get::<_, bool>(4)?,
                    "forkedFrom": r.get::<_, Option<String>>(5)?,
                    "worktree": r.get::<_, Option<String>>(6)?.and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()),
                    "createdAt": r.get::<_, String>(7)?,
                    "updatedAt": r.get::<_, String>(8)?,
                }))
            })
            .map_err(|e| e.to_string())?;

        let mut sessions = Vec::new();
        for s in session_rows {
            sessions.push(s.map_err(|e| e.to_string())?);
        }

        // Collect chat logs for these sessions
        let mut chat_stmt = conn
            .prepare("SELECT c.id, c.message_id, c.main_session_id, c.seq, c.turn_index, c.role, c.content, c.content_truncated, c.meta_json, c.created_at FROM chat_logs c JOIN main_sessions s ON c.main_session_id = s.id WHERE s.project_id = ?1 ORDER BY c.id ASC")
            .map_err(|e| e.to_string())?;
        let log_rows = chat_stmt
            .query_map([project_id], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, i64>(0)?,
                    "messageId": r.get::<_, String>(1)?,
                    "mainSessionId": r.get::<_, String>(2)?,
                    "seq": r.get::<_, i64>(3)?,
                    "turnIndex": r.get::<_, i64>(4)?,
                    "role": r.get::<_, String>(5)?,
                    "content": r.get::<_, String>(6)?,
                    "contentTruncated": r.get::<_, i64>(7)? != 0,
                    "meta": serde_json::from_str::<serde_json::Value>(&r.get::<_, String>(8)?).unwrap_or(serde_json::Value::Null),
                    "createdAt": r.get::<_, String>(9)?,
                }))
            })
            .map_err(|e| e.to_string())?;

        let mut logs = Vec::new();
        for l in log_rows {
            logs.push(l.map_err(|e| e.to_string())?);
        }

        let now: String = conn
            .query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ','now')", [], |r| r.get(0))
            .unwrap_or_default();

        Ok(serde_json::json!({
            "project": project,
            "sessions": sessions,
            "chatLogs": logs,
            "exportedAt": now
        }))
    }

    /// Clears all database records belonging to a project without deleting the project itself (P8.m5b).
    pub async fn clear_project_records(&self, project_id: String) -> Result<(usize, usize), String> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.writer
            .call(Box::new(move |c| {
                // Delete sessions (cascades to attempts, chat_logs, plans, etc.)
                let deleted_sessions = c.execute("DELETE FROM main_sessions WHERE project_id = ?1", [&project_id])?;
                // Delete project command rules
                let deleted_rules = c.execute("DELETE FROM command_rules WHERE project_id = ?1", [&project_id])?;
                let _ = tx.send((deleted_sessions, deleted_rules));
                Ok(())
            }))
            .await?;
        rx.await.map_err(|e| format!("Channel dropped: {e}"))
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
            "savings_events",
            "usage_snapshots",
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

    #[tokio::test]
    async fn test_chat_logs_truncation_fts_and_retention_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let s = Storage::open(dir.path()).unwrap();

        // 1. Write project, session, attempt and chat messages
        let huge_content = "TokageDatabaseTest ".repeat(4000); // ~76KB > 64KB
        let normal_content = "Hello Tokage Search Trigram";

        let mut batch = repo::WriteBatch::default();
        batch.projects.push(repo::ProjectRow {
            id: "proj-1".into(),
            name: "Project One".into(),
            path: "/path/to/proj1".into(),
            position: 0,
            expanded: true,
            trusted: Some(true),
        });
        batch.sessions.push(repo::SessionRow {
            id: "sess-1".into(),
            project_id: "proj-1".into(),
            title: "Session One".into(),
            state: "idle".into(),
            position: 0,
            archived: false,
            forked_from: None,
            worktree: None,
            updated_at: 1000.0,
        });
        batch.messages.push(repo::MessageRow {
            session_id: "sess-1".into(),
            seq: 0,
            turn_index: 0,
            message: serde_json::json!({
                "id": "msg-1",
                "role": "user",
                "text": normal_content,
            }),
            provider: Some("anthropic".into()),
            model: Some("claude-3-7".into()),
            mode: Some("direct".into()),
        });
        batch.messages.push(repo::MessageRow {
            session_id: "sess-1".into(),
            seq: 1,
            turn_index: 0,
            message: serde_json::json!({
                "id": "msg-2",
                "role": "assistant",
                "text": huge_content,
                "status": "done",
            }),
            provider: Some("anthropic".into()),
            model: Some("claude-3-7".into()),
            mode: Some("direct".into()),
        });

        s.import(batch).await.unwrap();

        // Verify truncation
        let logs = s.get_chat_logs("sess-1|anthropic|claude-3-7", None, None).unwrap();
        assert_eq!(logs.len(), 2);
        assert!(!logs[0].content_truncated);
        assert!(logs[1].content_truncated);
        assert!(logs[1].content.contains("… [truncated "));
        assert!(logs[1].content.len() <= 65_536);

        // Verify FTS search
        let found = s.search_chat_logs("Trigram", None, None).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].message_id, "msg-1");

        let found_huge = s.search_chat_logs("TokageDatabaseTest", Some("sess-1"), None).unwrap();
        assert_eq!(found_huge.len(), 1);
        assert_eq!(found_huge[0].message_id, "msg-2");

        // Verify Export Project Data
        let exported = s.export_project_data("proj-1").unwrap();
        assert_eq!(exported["project"]["name"], "Project One");
        assert_eq!(exported["sessions"].as_array().unwrap().len(), 1);
        assert_eq!(exported["chatLogs"].as_array().unwrap().len(), 2);

        // Verify Retention Cleanup (no old records yet, returns 0)
        let (del_logs, del_evts) = s.cleanup_retention_records().await.unwrap();
        assert_eq!(del_logs, 0);
        assert_eq!(del_evts, 0);

        // Verify Clear Project Records
        let (del_s, _) = s.clear_project_records("proj-1".into()).await.unwrap();
        assert_eq!(del_s, 1);
        let logs_after_clear = s.get_chat_logs("sess-1|anthropic|claude-3-7", None, None).unwrap();
        assert!(logs_after_clear.is_empty());
    }
}
