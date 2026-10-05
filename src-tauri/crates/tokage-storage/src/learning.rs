//! Learning database operations and records (docs/04 §9, docs/08 schema.md §4 & §6, P6.m6).

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

/// Record inserted into `task_outcomes` table upon task completion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskOutcomeRow {
    pub id: Option<i64>,
    pub task_id: Option<String>,
    pub model_id: String,
    pub provider_name: String,
    pub domain: String,
    pub difficulty: u8,
    pub size: String,
    /// Result outcome: FIRST_PASS, FIXED_IN_WINDOW, REASSIGNED, BLOCKED, USER_TAKEOVER
    pub result: String,
    pub p_predicted: f64,
    pub est_cost_usd: Option<f64>,
    pub actual_cost_usd: Option<f64>,
    pub turns: Option<u32>,
    pub created_at: Option<String>,
}

/// Learned statistic entry in `learned_stats` table.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LearnedStatRow {
    pub provider_name: String, // '*' for model-level
    pub model_id: String,
    pub domain: String,
    pub bucket: String,        // e.g. 'd2', 'M'
    pub metric: String,        // success_alpha, success_beta, fix_rate, cost_calibration, etc.
    pub value: f64,
    pub samples: u32,
    pub updated_at: Option<String>,
}

/// Inserts a task outcome record into SQLite `task_outcomes`.
pub fn insert_task_outcome(conn: &Connection, outcome: &TaskOutcomeRow) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO task_outcomes (
            task_id, model_id, provider_name, domain, difficulty, size,
            result, p_predicted, est_cost_usd, actual_cost_usd, turns
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            outcome.task_id,
            outcome.model_id,
            outcome.provider_name,
            outcome.domain,
            outcome.difficulty,
            outcome.size,
            outcome.result,
            outcome.p_predicted,
            outcome.est_cost_usd,
            outcome.actual_cost_usd,
            outcome.turns,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Lists recorded task outcomes with optional filters.
pub fn list_task_outcomes(
    conn: &Connection,
    model_id: Option<&str>,
    domain: Option<&str>,
    limit: u32,
) -> rusqlite::Result<Vec<TaskOutcomeRow>> {
    let mut sql = "SELECT id, task_id, model_id, provider_name, domain, difficulty, size,
                          result, p_predicted, est_cost_usd, actual_cost_usd, turns, created_at
                   FROM task_outcomes WHERE 1=1".to_string();
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(m) = model_id {
        sql.push_str(" AND model_id = ?");
        params_vec.push(Box::new(m.to_string()));
    }
    if let Some(d) = domain {
        sql.push_str(" AND domain = ?");
        params_vec.push(Box::new(d.to_string()));
    }
    sql.push_str(" ORDER BY id DESC LIMIT ?");
    params_vec.push(Box::new(limit));

    let mut stmt = conn.prepare(&sql)?;
    let params_slice: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|b| &**b).collect();
    let rows = stmt.query_map(&params_slice[..], |row| {
        Ok(TaskOutcomeRow {
            id: Some(row.get(0)?),
            task_id: row.get(1)?,
            model_id: row.get(2)?,
            provider_name: row.get(3)?,
            domain: row.get(4)?,
            difficulty: row.get::<_, i64>(5)? as u8,
            size: row.get(6)?,
            result: row.get(7)?,
            p_predicted: row.get(8)?,
            est_cost_usd: row.get(9)?,
            actual_cost_usd: row.get(10)?,
            turns: row.get::<_, Option<i64>>(11)?.map(|t| t as u32),
            created_at: Some(row.get(12)?),
        })
    })?;

    rows.collect()
}

/// Upserts or updates a learned statistic using Exponential Moving Average (EMA) or Beta accumulators.
pub fn upsert_learned_stat(conn: &Connection, stat: &LearnedStatRow) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO learned_stats (
            provider_name, model_id, domain, bucket, metric, value, samples, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, (strftime('%Y-%m-%dT%H:%M:%fZ','now')))
         ON CONFLICT(provider_name, model_id, domain, bucket, metric) DO UPDATE SET
            value = excluded.value,
            samples = excluded.samples,
            updated_at = excluded.updated_at",
        params![
            stat.provider_name,
            stat.model_id,
            stat.domain,
            stat.bucket,
            stat.metric,
            stat.value,
            stat.samples,
        ],
    )?;
    Ok(())
}

/// Retrieves a learned statistic.
pub fn get_learned_stat(
    conn: &Connection,
    provider_name: &str,
    model_id: &str,
    domain: &str,
    bucket: &str,
    metric: &str,
) -> rusqlite::Result<Option<LearnedStatRow>> {
    let mut stmt = conn.prepare(
        "SELECT provider_name, model_id, domain, bucket, metric, value, samples, updated_at
         FROM learned_stats
         WHERE provider_name = ?1 AND model_id = ?2 AND domain = ?3 AND bucket = ?4 AND metric = ?5",
    )?;

    let mut rows = stmt.query_map(
        params![provider_name, model_id, domain, bucket, metric],
        |row| {
            Ok(LearnedStatRow {
                provider_name: row.get(0)?,
                model_id: row.get(1)?,
                domain: row.get(2)?,
                bucket: row.get(3)?,
                metric: row.get(4)?,
                value: row.get(5)?,
                samples: row.get::<_, i64>(6)? as u32,
                updated_at: Some(row.get(7)?),
            })
        },
    )?;

    if let Some(res) = rows.next() {
        Ok(Some(res?))
    } else {
        Ok(None)
    }
}

/// Lists all learned stats for a model across domains and metrics.
pub fn list_learned_stats_for_model(
    conn: &Connection,
    model_id: &str,
) -> rusqlite::Result<Vec<LearnedStatRow>> {
    let mut stmt = conn.prepare(
        "SELECT provider_name, model_id, domain, bucket, metric, value, samples, updated_at
         FROM learned_stats
         WHERE model_id = ?1
         ORDER BY domain ASC, metric ASC",
    )?;

    let rows = stmt.query_map(params![model_id], |row| {
        Ok(LearnedStatRow {
            provider_name: row.get(0)?,
            model_id: row.get(1)?,
            domain: row.get(2)?,
            bucket: row.get(3)?,
            metric: row.get(4)?,
            value: row.get(5)?,
            samples: row.get::<_, i64>(6)? as u32,
            updated_at: Some(row.get(7)?),
        })
    })?;

    rows.collect()
}
