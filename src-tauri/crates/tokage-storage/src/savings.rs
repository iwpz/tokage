//! Savings ledger and usage snapshots storage operations (docs/08 schema.md §5, docs/06 §5.2 & §7, P8.m3a & P8.m3b).

use std::collections::HashMap;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

/// Record for a savings event in `savings_events`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SavingsEventRow {
    pub id: Option<i64>,
    pub main_session_id: Option<String>,
    pub plan_id: Option<i64>,
    pub kind: String, // 'ASSIGNMENT', 'CONTEXT', 'DIRECT'
    pub ref_id: Option<String>,
    pub baseline_usd: f64,
    pub actual_usd: Option<f64>,
    pub settled: bool,
    pub detail_json: String,
    pub created_at: Option<String>,
}

/// Record for a snapshot in `usage_snapshots`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageSnapshotRow {
    pub id: Option<i64>,
    pub provider_name: String,
    pub window_kind: String, // 'SESSION_5H', 'WEEKLY', 'WEEKLY_MODEL', 'DAILY_BUDGET'
    pub usage_percentage: Option<f64>,
    pub quota_remaining_usd: Option<f64>,
    pub next_reset_time: Option<String>,
    pub raw_text: Option<String>,
    pub data_source: String, // 'PASSIVE', 'STRUCTURED', 'POLL', 'MANUAL'
    pub captured_at: Option<String>,
}

/// Inserts a savings event into SQLite `savings_events`.
pub fn insert_savings_event(conn: &Connection, event: &SavingsEventRow) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO savings_events (
            main_session_id, plan_id, kind, ref_id, baseline_usd, actual_usd, settled, detail_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            event.main_session_id,
            event.plan_id,
            event.kind,
            event.ref_id,
            event.baseline_usd,
            event.actual_usd,
            if event.settled { 1 } else { 0 },
            event.detail_json,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Settles a savings event with the final actual cost and marks it settled.
pub fn settle_savings_event(conn: &Connection, id: i64, actual_usd: f64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE savings_events
         SET actual_usd = ?1, settled = 1
         WHERE id = ?2",
        params![actual_usd, id],
    )?;
    Ok(())
}

/// Lists savings events with an optional range filter ("today", "7d", "30d").
pub fn list_savings_events(conn: &Connection, range: Option<&str>) -> rusqlite::Result<Vec<SavingsEventRow>> {
    let sql = match range {
        Some("today") => {
            "SELECT id, main_session_id, plan_id, kind, ref_id, baseline_usd, actual_usd, settled, detail_json, created_at
             FROM savings_events
             WHERE created_at >= date('now', 'start of day')
             ORDER BY id DESC"
        }
        Some("7d") => {
            "SELECT id, main_session_id, plan_id, kind, ref_id, baseline_usd, actual_usd, settled, detail_json, created_at
             FROM savings_events
             WHERE created_at >= datetime('now', '-7 days')
             ORDER BY id DESC"
        }
        Some("30d") => {
            "SELECT id, main_session_id, plan_id, kind, ref_id, baseline_usd, actual_usd, settled, detail_json, created_at
             FROM savings_events
             WHERE created_at >= datetime('now', '-30 days')
             ORDER BY id DESC"
        }
        _ => {
            "SELECT id, main_session_id, plan_id, kind, ref_id, baseline_usd, actual_usd, settled, detail_json, created_at
             FROM savings_events
             ORDER BY id DESC"
        }
    };

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], |row| {
        Ok(SavingsEventRow {
            id: Some(row.get(0)?),
            main_session_id: row.get(1)?,
            plan_id: row.get(2)?,
            kind: row.get(3)?,
            ref_id: row.get(4)?,
            baseline_usd: row.get(5)?,
            actual_usd: row.get(6)?,
            settled: row.get::<_, i32>(7)? != 0,
            detail_json: row.get(8)?,
            created_at: Some(row.get(9)?),
        })
    })?;

    let mut events = Vec::new();
    for r in rows {
        events.push(r?);
    }
    Ok(events)
}

/// Computes the total savings and breakdown by kind (ASSIGNMENT, CONTEXT, DIRECT).
pub fn get_savings_summary(
    conn: &Connection,
    range: Option<&str>,
) -> rusqlite::Result<(f64, HashMap<String, f64>)> {
    let events = list_savings_events(conn, range)?;
    let mut total_usd = 0.0;
    let mut by_kind: HashMap<String, f64> = HashMap::new();
    by_kind.insert("ASSIGNMENT".to_string(), 0.0);
    by_kind.insert("CONTEXT".to_string(), 0.0);
    by_kind.insert("DIRECT".to_string(), 0.0);

    for ev in events {
        let actual = ev.actual_usd.unwrap_or(0.0);
        let saved = (ev.baseline_usd - actual).max(0.0);
        total_usd += saved;
        *by_kind.entry(ev.kind.clone()).or_insert(0.0) += saved;
    }

    Ok((total_usd, by_kind))
}

/// Inserts a usage snapshot into SQLite `usage_snapshots`.
pub fn insert_usage_snapshot(conn: &Connection, snapshot: &UsageSnapshotRow) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO usage_snapshots (
            provider_name, window_kind, usage_percentage, quota_remaining_usd,
            next_reset_time, raw_text, data_source
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            snapshot.provider_name,
            snapshot.window_kind,
            snapshot.usage_percentage,
            snapshot.quota_remaining_usd,
            snapshot.next_reset_time,
            snapshot.raw_text,
            snapshot.data_source,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Gets the latest usage snapshot for a provider.
pub fn get_latest_usage_snapshot(
    conn: &Connection,
    provider_name: &str,
) -> rusqlite::Result<Option<UsageSnapshotRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, provider_name, window_kind, usage_percentage, quota_remaining_usd,
                next_reset_time, raw_text, data_source, captured_at
         FROM usage_snapshots
         WHERE provider_name = ?1
         ORDER BY id DESC
         LIMIT 1",
    )?;

    stmt.query_row(params![provider_name], |row| {
        Ok(UsageSnapshotRow {
            id: Some(row.get(0)?),
            provider_name: row.get(1)?,
            window_kind: row.get(2)?,
            usage_percentage: row.get(3)?,
            quota_remaining_usd: row.get(4)?,
            next_reset_time: row.get(5)?,
            raw_text: row.get(6)?,
            data_source: row.get(7)?,
            captured_at: Some(row.get(8)?),
        })
    })
    .optional()
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
    fn test_savings_events_roundtrip_and_summary() {
        let conn = in_memory_db();

        conn.execute("INSERT INTO projects (id, name, local_path) VALUES ('p1', 'Test', '/tmp')", []).unwrap();
        conn.execute("INSERT INTO main_sessions (id, project_id, title) VALUES ('session-1', 'p1', 'Session')", []).unwrap();
        conn.execute("INSERT INTO plans (id, main_session_id, goal, spec_json) VALUES (1, 'session-1', 'Goal', '{}')", []).unwrap();

        let event = SavingsEventRow {
            id: None,
            main_session_id: Some("session-1".into()),
            plan_id: Some(1),
            kind: "ASSIGNMENT".into(),
            ref_id: Some("T1".into()),
            baseline_usd: 1.50,
            actual_usd: Some(0.80),
            settled: true,
            detail_json: "{\"ref_model\":\"claude-3-7-sonnet\"}".into(),
            created_at: None,
        };

        let ev_id = insert_savings_event(&conn, &event).unwrap();
        assert!(ev_id > 0);

        let event2 = SavingsEventRow {
            id: None,
            main_session_id: Some("session-1".into()),
            plan_id: Some(1),
            kind: "CONTEXT".into(),
            ref_id: Some("att-1".into()),
            baseline_usd: 0.60,
            actual_usd: Some(0.20),
            settled: true,
            detail_json: "{}".into(),
            created_at: None,
        };
        insert_savings_event(&conn, &event2).unwrap();

        let list = list_savings_events(&conn, None).unwrap();
        assert_eq!(list.len(), 2);

        let (total_saved, by_kind) = get_savings_summary(&conn, None).unwrap();
        // 1.50 - 0.80 = 0.70; 0.60 - 0.20 = 0.40 -> total = 1.10
        assert!((total_saved - 1.10).abs() < 1e-4);
        assert!((by_kind.get("ASSIGNMENT").copied().unwrap_or(0.0) - 0.70).abs() < 1e-4);
        assert!((by_kind.get("CONTEXT").copied().unwrap_or(0.0) - 0.40).abs() < 1e-4);
    }

    #[test]
    fn test_usage_snapshot_roundtrip() {
        let conn = in_memory_db();

        let snap = UsageSnapshotRow {
            id: None,
            provider_name: "CLAUDE_CODE".into(),
            window_kind: "SESSION_5H".into(),
            usage_percentage: Some(0.42),
            quota_remaining_usd: None,
            next_reset_time: Some("2026-10-06T12:00:00Z".into()),
            raw_text: Some("42% used".into()),
            data_source: "PASSIVE".into(),
            captured_at: None,
        };

        let snap_id = insert_usage_snapshot(&conn, &snap).unwrap();
        assert!(snap_id > 0);

        let loaded = get_latest_usage_snapshot(&conn, "CLAUDE_CODE")
            .unwrap()
            .expect("snapshot found");
        assert_eq!(loaded.provider_name, "CLAUDE_CODE");
        assert_eq!(loaded.usage_percentage, Some(0.42));
    }
}
