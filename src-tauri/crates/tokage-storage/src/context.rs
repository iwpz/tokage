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
    let old = get_context_action(conn, action_id)?.ok_or(rusqlite::Error::InvalidQuery)?;
    let transition = old.status==new_status || matches!((old.status.as_str(),new_status),("SUGGESTED","APPLIED"|"DISMISSED"|"FAILED")|("APPLIED","UNDONE"|"FAILED"));
    if !transition || (new_attempt_id.is_some() && new_status!="APPLIED") || old.new_attempt_id.as_deref().zip(new_attempt_id).is_some_and(|(a,b)|a!=b) {
        return Err(rusqlite::Error::InvalidQuery);
    }
    conn.execute(
        "UPDATE context_actions
         SET status = ?1,
             new_attempt_id = COALESCE(?2, new_attempt_id),
             applied_at = CASE WHEN ?1 = 'APPLIED' AND status != 'APPLIED' THEN (strftime('%Y-%m-%dT%H:%M:%fZ','now')) ELSE applied_at END
         WHERE id = ?3",
        params![new_status, new_attempt_id, action_id],
    )?;
    Ok(())
}

/// Atomically accept a compact action inside the caller's writer transaction.
pub fn accept_compact_carrier(conn: &Connection, action: &ContextActionRow, target_id: &str) -> rusqlite::Result<i64> {
    if action.kind != "COMPACT" || action.status != "SUGGESTED" || target_id.is_empty() {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let id = insert_context_action(conn, action)?;
    update_context_action_status(conn, id, "APPLIED", Some(target_id))?;
    Ok(id)
}

/// Only the bound carrier may mark an accepted action failed or undone.
pub fn fail_compact_carrier(conn: &Connection, action_id: i64, target_id: &str, cancelled_before_send: bool) -> rusqlite::Result<()> {
    let row = get_context_action(conn, action_id)?.ok_or(rusqlite::Error::InvalidQuery)?;
    if row.new_attempt_id.as_deref() != Some(target_id) || row.kind != "COMPACT" {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let status = if cancelled_before_send { "UNDONE" } else { "FAILED" };
    update_context_action_status(conn, action_id, status, None)
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

/// Read-only acceptance receipts, including FRESH targets and later failed/undone actions.
#[derive(Clone, Debug)]
pub struct ContextResetRow {
    pub action_id: i64,
    pub attempt_id: String,
    pub new_attempt_id: Option<String>,
    pub applied_at_ms: Option<i64>,
}
pub fn list_context_reset_actions(conn: &Connection, attempts: &[String]) -> rusqlite::Result<Vec<ContextResetRow>> {
    if attempts.len()>512 {return Err(rusqlite::Error::InvalidQuery);}
    let mut rows=std::collections::BTreeMap::new();
    // Stay below older SQLite's 999-variable limit; all values remain parameters.
    for chunk in attempts.chunks(128) {
        let marks=vec!["?";chunk.len()].join(",");
        let sql=format!("SELECT id,attempt_id,new_attempt_id,CAST(ROUND((julianday(applied_at)-2440587.5)*86400000) AS INTEGER) FROM context_actions WHERE (status='APPLIED' OR (status IN ('UNDONE','FAILED') AND applied_at IS NOT NULL)) AND kind IN ('COMPACT','FRESH') AND (attempt_id IN ({marks}) OR new_attempt_id IN ({marks})) ORDER BY id LIMIT 4097");
        let mut statement=conn.prepare(&sql)?;
        let found=statement.query_map(rusqlite::params_from_iter(chunk.iter().chain(chunk.iter())),|r|Ok(ContextResetRow{action_id:r.get(0)?,attempt_id:r.get(1)?,new_attempt_id:r.get(2)?,applied_at_ms:r.get(3)?}))?;
        for row in found {let row=row?;rows.insert(row.action_id,row);if rows.len()>4096{return Err(rusqlite::Error::InvalidQuery);}}
    }
    Ok(rows.into_values().collect())
}

#[cfg(test)]
mod cooldown_tests {
    use super::*;
    fn database()->Connection {
        let c=Connection::open_in_memory().unwrap();c.execute_batch(include_str!("../migrations/V7__context_actions.sql")).unwrap();c
    }
    fn row(kind:&str,status:&str)->ContextActionRow {
        ContextActionRow{id:None,attempt_id:"a".into(),kind:kind.into(),trigger:"USER".into(),status:status.into(),est_costs_json:"{}".into(),est_saving_pct:0.,actual_saving_usd:None,new_attempt_id:None,created_at:None,applied_at:Some("1970-01-01T00:00:01.500Z".into())}
    }
    #[test]
    fn carrier_receipts_are_bound_and_keep_the_acceptance_time() {
        let c = database();
        let id = accept_compact_carrier(&c, &row("COMPACT", "SUGGESTED"), "carrier").unwrap();
        let accepted = get_context_action(&c, id).unwrap().unwrap();
        assert_eq!(accepted.status, "APPLIED");
        assert!(accepted.applied_at.is_some());
        assert!(fail_compact_carrier(&c, id, "other", false).is_err());
        assert_eq!(get_context_action(&c, id).unwrap().unwrap().status, "APPLIED");
        fail_compact_carrier(&c, id, "carrier", false).unwrap();
        let failed = get_context_action(&c, id).unwrap().unwrap();
        assert_eq!(failed.status, "FAILED");
        assert_eq!(failed.applied_at, accepted.applied_at);
        fail_compact_carrier(&c, id, "carrier", false).unwrap();
        assert!(fail_compact_carrier(&c, id, "carrier", true).is_err());
        let next = accept_compact_carrier(&c, &row("COMPACT", "SUGGESTED"), "cancelled").unwrap();
        fail_compact_carrier(&c, next, "cancelled", true).unwrap();
        assert_eq!(get_context_action(&c, next).unwrap().unwrap().status, "UNDONE");
        assert!(accept_compact_carrier(&c, &row("FRESH", "SUGGESTED"), "carrier").is_err());
    }

    #[test]
    fn acceptance_is_idempotent_target_binding_is_one_way_and_terminal_rows_cannot_reactivate() {
        let c=database();let id=insert_context_action(&c,&row("COMPACT","SUGGESTED")).unwrap();
        update_context_action_status(&c,id,"APPLIED",None).unwrap();
        c.execute("UPDATE context_actions SET applied_at='1970-01-01T00:00:01.500Z' WHERE id=?1",[id]).unwrap();
        update_context_action_status(&c,id,"APPLIED",Some("b")).unwrap();
        update_context_action_status(&c,id,"APPLIED",Some("b")).unwrap();
        let actions=list_context_reset_actions(&c,&["a".into(),"b".into()]).unwrap();
        assert_eq!(actions.len(),1);assert_eq!(actions[0].applied_at_ms,Some(1500));
        assert!(update_context_action_status(&c,id,"APPLIED",Some("another")).is_err());
        update_context_action_status(&c,id,"UNDONE",None).unwrap();
        assert_eq!(list_context_reset_actions(&c,&["a".into(),"b".into()]).unwrap().len(),1);
        assert!(update_context_action_status(&c,id,"APPLIED",None).is_err());
        assert!(update_context_action_status(&c,999,"APPLIED",None).is_err());
    }
    #[test]
    fn only_persisted_applied_compact_or_fresh_resets_are_selected() {
        let c=database();
        for status in ["SUGGESTED","DISMISSED","FAILED","UNDONE"] {let mut r=row("COMPACT",status);r.applied_at=None;insert_context_action(&c,&r).unwrap();}
        insert_context_action(&c,&row("REUSE","APPLIED")).unwrap();insert_context_action(&c,&row("CONTINUE","APPLIED")).unwrap();
        let mut fresh=row("FRESH","APPLIED");fresh.new_attempt_id=Some("destination".into());let id=insert_context_action(&c,&fresh).unwrap();
        assert!(list_context_reset_actions(&c,&[]).unwrap().is_empty());
        assert!(list_context_reset_actions(&c,&["a') OR 1=1 --".into()]).unwrap().is_empty());
        assert_eq!(list_context_reset_actions(&c,&["destination".into()]).unwrap()[0].action_id,id);
        assert_eq!(list_context_reset_actions(&c,&vec!["a".into();260]).unwrap().len(),1);
    }
}
