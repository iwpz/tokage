//! Context action persistence and actual savings settlement (docs/08 schema.md §4, P8.m1f).

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

/// Database row mapping for table `context_actions`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContextActionRow {
    pub id: Option<i64>,
    pub attempt_id: String,
    pub kind: String,
    pub trigger: String,
    pub status: String,
    pub est_costs_json: String,
    pub est_saving_pct: f64,
    pub actual_saving_usd: Option<f64>,
    pub new_attempt_id: Option<String>,
    pub created_at: Option<String>,
    pub applied_at: Option<String>,
}

/// Inserts a new context action into SQLite.
pub fn insert_context_action(conn: &Connection, action: &ContextActionRow) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO context_actions (
            attempt_id, kind, trigger, status, est_costs_json,
            est_saving_pct, actual_saving_usd, new_attempt_id, applied_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            action.attempt_id,
            action.kind,
            action.trigger,
            action.status,
            action.est_costs_json,
            action.est_saving_pct,
            action.actual_saving_usd,
            action.new_attempt_id,
            action.applied_at,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Updates the status of a context action (e.g. SUGGESTED -> APPLIED / DISMISSED).
pub fn update_context_action_status(
    conn: &Connection,
    action_id: i64,
    new_status: &str,
    new_attempt_id: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE context_actions
         SET status = ?1,
             new_attempt_id = COALESCE(?2, new_attempt_id),
             applied_at = CASE WHEN ?1 = 'APPLIED' THEN (strftime('%Y-%m-%dT%H:%M:%fZ','now')) ELSE applied_at END
         WHERE id = ?3",
        params![new_status, new_attempt_id, action_id],
    )?;
    Ok(())
}

/// Settles the actual savings for a context action after execution completes.
pub fn settle_context_action_savings(
    conn: &Connection,
    action_id: i64,
    actual_saving_usd: f64,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE context_actions
         SET actual_saving_usd = ?1
         WHERE id = ?2",
        params![actual_saving_usd, action_id],
    )?;
    Ok(())
}

/// Lists context actions for an attempt.
pub fn list_context_actions(
    conn: &Connection,
    attempt_id: &str,
) -> rusqlite::Result<Vec<ContextActionRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, attempt_id, kind, trigger, status, est_costs_json,
                est_saving_pct, actual_saving_usd, new_attempt_id, created_at, applied_at
         FROM context_actions
         WHERE attempt_id = ?1
         ORDER BY id ASC",
    )?;

    let rows = stmt.query_map(params![attempt_id], |row| {
        Ok(ContextActionRow {
            id: Some(row.get(0)?),
            attempt_id: row.get(1)?,
            kind: row.get(2)?,
            trigger: row.get(3)?,
            status: row.get(4)?,
            est_costs_json: row.get(5)?,
            est_saving_pct: row.get(6)?,
            actual_saving_usd: row.get(7)?,
            new_attempt_id: row.get(8)?,
            created_at: Some(row.get(9)?),
            applied_at: row.get(10)?,
        })
    })?;

    let mut actions = Vec::new();
    for r in rows {
        actions.push(r?);
    }
    Ok(actions)
}

/// Retrieves a context action by ID.
pub fn get_context_action(
    conn: &Connection,
    action_id: i64,
) -> rusqlite::Result<Option<ContextActionRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, attempt_id, kind, trigger, status, est_costs_json,
                est_saving_pct, actual_saving_usd, new_attempt_id, created_at, applied_at
         FROM context_actions
         WHERE id = ?1",
    )?;

    let mut rows = stmt.query_map(params![action_id], |row| {
        Ok(ContextActionRow {
            id: Some(row.get(0)?),
            attempt_id: row.get(1)?,
            kind: row.get(2)?,
            trigger: row.get(3)?,
            status: row.get(4)?,
            est_costs_json: row.get(5)?,
            est_saving_pct: row.get(6)?,
            actual_saving_usd: row.get(7)?,
            new_attempt_id: row.get(8)?,
            created_at: Some(row.get(9)?),
            applied_at: row.get(10)?,
        })
    })?;

    if let Some(res) = rows.next() {
        Ok(Some(res?))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE context_actions (
                id                INTEGER PRIMARY KEY AUTOINCREMENT,
                attempt_id        TEXT NOT NULL,
                kind              TEXT NOT NULL CHECK (kind IN ('CONTINUE','COMPACT','FRESH','REUSE')),
                trigger           TEXT NOT NULL CHECK (trigger IN ('PLANNER','HARD_LIMIT','USER')),
                status            TEXT NOT NULL DEFAULT 'SUGGESTED'
                                  CHECK (status IN ('SUGGESTED','APPLIED','DISMISSED','UNDONE','FAILED')),
                est_costs_json    TEXT NOT NULL,
                est_saving_pct    REAL NOT NULL,
                actual_saving_usd REAL,
                new_attempt_id    TEXT,
                created_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
                applied_at        TEXT
            );",
        ).unwrap();
        conn
    }

    #[test]
    fn test_context_action_crud_and_settlement() {
        let conn = setup_test_db();
        let action = ContextActionRow {
            id: None,
            attempt_id: "att-123".to_string(),
            kind: "COMPACT".to_string(),
            trigger: "PLANNER".to_string(),
            status: "SUGGESTED".to_string(),
            est_costs_json: "{\"compact\": 0.38, \"continue\": 0.61}".to_string(),
            est_saving_pct: 0.38,
            actual_saving_usd: None,
            new_attempt_id: None,
            created_at: None,
            applied_at: None,
        };

        let id = insert_context_action(&conn, &action).unwrap();
        assert!(id > 0);

        let list = list_context_actions(&conn, "att-123").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].status, "SUGGESTED");

        update_context_action_status(&conn, id, "APPLIED", Some("att-124")).unwrap();
        let updated = get_context_action(&conn, id).unwrap().unwrap();
        assert_eq!(updated.status, "APPLIED");
        assert_eq!(updated.new_attempt_id.as_deref(), Some("att-124"));
        assert!(updated.applied_at.is_some());

        settle_context_action_savings(&conn, id, 0.23).unwrap();
        let settled = get_context_action(&conn, id).unwrap().unwrap();
        assert_eq!(settled.actual_saving_usd, Some(0.23));
    }
}
