//! Orchestration database operations (docs/08 schema.md §2-§4, P7.m1 & P7.m2).

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

/// Record for a task report in `task_reports`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskReportRow {
    pub id: Option<i64>,
    pub task_id: String,
    pub model_id: String,
    pub attempt_id: String,
    pub first_pass: bool,
    pub is_locally_generated: bool,
    pub summary_md: String,
    pub full_md: String,
    pub contract_deviation: Option<String>,
    pub changed_files_stat: String,
    pub changed_files_list: Vec<String>,
    pub gate_results: String,
    pub cost_usd: Option<f64>,
    pub created_at: Option<String>,
}

/// Record for a brief in `briefs`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BriefRow {
    pub id: Option<i64>,
    pub task_id: String,
    pub kind: String, // 'TASK_BRIEF','PROGRESS_REPORT','DIRECT'
    pub content: String,
    pub token_count: i64,
    pub edited_by_user: bool,
    pub created_at: Option<String>,
}

/// Inserts a task report into SQLite `task_reports`.
pub fn insert_task_report(conn: &Connection, report: &TaskReportRow) -> rusqlite::Result<i64> {
    let changed_files_json = serde_json::to_string(&report.changed_files_list).unwrap_or_else(|_| "[]".to_string());
    conn.execute(
        "INSERT INTO task_reports (
            task_id, model_id, attempt_id, first_pass, is_locally_generated,
            summary_md, full_md, contract_deviation, changed_files_stat,
            changed_files_list, gate_results, cost_usd
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            report.task_id,
            report.model_id,
            report.attempt_id,
            if report.first_pass { 1 } else { 0 },
            if report.is_locally_generated { 1 } else { 0 },
            report.summary_md,
            report.full_md,
            report.contract_deviation,
            report.changed_files_stat,
            changed_files_json,
            report.gate_results,
            report.cost_usd,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Retrieves all task reports for a given task ID.
pub fn get_task_reports(conn: &Connection, task_id: &str) -> rusqlite::Result<Vec<TaskReportRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, task_id, model_id, attempt_id, first_pass, is_locally_generated,
                summary_md, full_md, contract_deviation, changed_files_stat,
                changed_files_list, gate_results, cost_usd, created_at
         FROM task_reports
         WHERE task_id = ?1
         ORDER BY id ASC",
    )?;

    let rows = stmt.query_map(params![task_id], |row| {
        let changed_files_raw: String = row.get(10)?;
        let changed_files_list: Vec<String> = serde_json::from_str(&changed_files_raw).unwrap_or_default();
        Ok(TaskReportRow {
            id: Some(row.get(0)?),
            task_id: row.get(1)?,
            model_id: row.get(2)?,
            attempt_id: row.get(3)?,
            first_pass: row.get::<_, i32>(4)? != 0,
            is_locally_generated: row.get::<_, i32>(5)? != 0,
            summary_md: row.get(6)?,
            full_md: row.get(7)?,
            contract_deviation: row.get(8)?,
            changed_files_stat: row.get(9)?,
            changed_files_list,
            gate_results: row.get(11)?,
            cost_usd: row.get(12)?,
            created_at: Some(row.get(13)?),
        })
    })?;

    let mut result = Vec::new();
    for r in rows {
        result.push(r?);
    }
    Ok(result)
}

/// Inserts a task brief or progress report into `briefs`.
pub fn insert_brief(conn: &Connection, brief: &BriefRow) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO briefs (task_id, kind, content, token_count, edited_by_user)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            brief.task_id,
            brief.kind,
            brief.content,
            brief.token_count,
            if brief.edited_by_user { 1 } else { 0 },
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Gets the latest brief for a task ID.
pub fn get_latest_brief(conn: &Connection, task_id: &str) -> rusqlite::Result<Option<BriefRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, task_id, kind, content, token_count, edited_by_user, created_at
         FROM briefs
         WHERE task_id = ?1
         ORDER BY id DESC LIMIT 1",
    )?;

    let mut rows = stmt.query(params![task_id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(BriefRow {
            id: Some(row.get(0)?),
            task_id: row.get(1)?,
            kind: row.get(2)?,
            content: row.get(3)?,
            token_count: row.get(4)?,
            edited_by_user: row.get::<_, i32>(5)? != 0,
            created_at: Some(row.get(6)?),
        }))
    } else {
        Ok(None)
    }
}

/// Record for an acceptance gate run in `gate_runs`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GateRunRow {
    pub id: Option<i64>,
    pub task_id: Option<String>,
    pub plan_id: String,
    pub attempt_id: Option<String>,
    pub command: String,
    pub exit_code: Option<i32>,
    pub passed: bool,
    pub duration_ms: u64,
    pub output_tail: String,
    pub error_message: Option<String>,
    pub created_at: Option<String>,
}

/// Inserts a gate run record into SQLite `gate_runs`.
pub fn insert_gate_run(conn: &Connection, run: &GateRunRow) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO gate_runs (
            task_id, plan_id, attempt_id, command, exit_code, passed,
            duration_ms, output_tail, error_message
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            run.task_id,
            run.plan_id,
            run.attempt_id,
            run.command,
            run.exit_code,
            if run.passed { 1 } else { 0 },
            run.duration_ms as i64,
            run.output_tail,
            run.error_message,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Lists all gate runs for a task or plan.
pub fn list_gate_runs(conn: &Connection, task_id: Option<&str>, plan_id: Option<&str>) -> rusqlite::Result<Vec<GateRunRow>> {
    let mut query = "SELECT id, task_id, plan_id, attempt_id, command, exit_code, passed,
                            duration_ms, output_tail, error_message, created_at
                     FROM gate_runs WHERE 1=1".to_string();

    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(t) = task_id {
        query.push_str(" AND task_id = ?");
        params_vec.push(Box::new(t.to_string()));
    }
    if let Some(p) = plan_id {
        query.push_str(" AND plan_id = ?");
        params_vec.push(Box::new(p.to_string()));
    }
    query.push_str(" ORDER BY id ASC");

    let mut stmt = conn.prepare(&query)?;
    let slice: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(&slice[..], |row| {
        let dur: i64 = row.get(7)?;
        Ok(GateRunRow {
            id: Some(row.get(0)?),
            task_id: row.get(1)?,
            plan_id: row.get(2)?,
            attempt_id: row.get(3)?,
            command: row.get(4)?,
            exit_code: row.get(5)?,
            passed: row.get::<_, i32>(6)? != 0,
            duration_ms: dur as u64,
            output_tail: row.get(8)?,
            error_message: row.get(9)?,
            created_at: Some(row.get(10)?),
        })
    })?;

    let mut result = Vec::new();
    for r in rows {
        result.push(r?);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::embedded::migrations::runner().run(&mut conn).unwrap();
        conn
    }

    #[test]
    fn test_report_and_brief_roundtrip() {
        let conn = in_memory_db();

        let brief = BriefRow {
            id: None,
            task_id: "T1".to_string(),
            kind: "TASK_BRIEF".to_string(),
            content: "# Task Brief\nRule 1...".to_string(),
            token_count: 240,
            edited_by_user: false,
            created_at: None,
        };

        let b_id = insert_brief(&conn, &brief).unwrap();
        assert!(b_id > 0);

        let loaded_brief = get_latest_brief(&conn, "T1").unwrap().expect("brief found");
        assert_eq!(loaded_brief.token_count, 240);
        assert_eq!(loaded_brief.content, brief.content);

        let report = TaskReportRow {
            id: None,
            task_id: "T1".to_string(),
            model_id: "claude-3-7-sonnet".to_string(),
            attempt_id: "att-1".to_string(),
            first_pass: true,
            is_locally_generated: false,
            summary_md: "完成了基础表设计".to_string(),
            full_md: "## T1 完成...".to_string(),
            contract_deviation: None,
            changed_files_stat: "schema.rs +20".to_string(),
            changed_files_list: vec!["schema.rs".to_string()],
            gate_results: "cargo test ok".to_string(),
            cost_usd: Some(0.05),
            created_at: None,
        };

        let r_id = insert_task_report(&conn, &report).unwrap();
        assert!(r_id > 0);

        let reports = get_task_reports(&conn, "T1").unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].model_id, "claude-3-7-sonnet");
        assert_eq!(reports[0].first_pass, true);
        assert_eq!(reports[0].changed_files_list, vec!["schema.rs".to_string()]);
    }
}
