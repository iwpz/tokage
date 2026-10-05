//! `approvals` (docs/08 schema.md §7, docs/11 §8, P2.m3f): audit trail of authorization decisions.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRecord {
    pub id: Option<i64>,
    pub attempt_id: Option<String>,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub command: String,
    pub level: String,
    pub reason: Option<String>,
    pub decision: String,
    pub mode: String,
    pub decided_at: Option<String>,
}

pub fn list(c: &Connection, session_id: Option<&str>) -> rusqlite::Result<Vec<ApprovalRecord>> {
    let sql = if session_id.is_some() {
        "SELECT id, attempt_id, session_id, turn_id, command, level, reason, decision, mode, decided_at \
         FROM approvals WHERE session_id = ?1 ORDER BY id DESC"
    } else {
        "SELECT id, attempt_id, session_id, turn_id, command, level, reason, decision, mode, decided_at \
         FROM approvals ORDER BY id DESC"
    };
    let mut stmt = c.prepare(sql)?;
    let map_row = |r: &rusqlite::Row<'_>| {
        Ok(ApprovalRecord {
            id: Some(r.get(0)?),
            attempt_id: r.get(1)?,
            session_id: r.get(2)?,
            turn_id: r.get(3)?,
            command: r.get(4)?,
            level: r.get(5)?,
            reason: r.get(6)?,
            decision: r.get(7)?,
            mode: r.get(8)?,
            decided_at: r.get(9)?,
        })
    };
    let rows = if let Some(sid) = session_id {
        stmt.query_map([sid], map_row)?
    } else {
        stmt.query_map([], map_row)?
    };
    rows.collect()
}

pub fn insert(c: &Connection, rec: &ApprovalRecord) -> rusqlite::Result<i64> {
    c.execute(
        "INSERT INTO approvals (attempt_id, session_id, turn_id, command, level, reason, decision, mode) \
         VALUES (?1, (SELECT id FROM main_sessions WHERE id = ?2), ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            rec.attempt_id,
            rec.session_id,
            rec.turn_id,
            rec.command,
            rec.level,
            rec.reason,
            rec.decision,
            rec.mode,
        ],
    )?;
    Ok(c.last_insert_rowid())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        c.execute_batch(super::super::PRAGMAS).unwrap();
        super::super::embedded::migrations::runner().run(&mut c).unwrap();
        c.execute("INSERT INTO projects (id, name, local_path) VALUES ('p1', 'a', '/a')", []).unwrap();
        c.execute("INSERT INTO main_sessions (id, project_id, title) VALUES ('s1', 'p1', 'Session 1')", []).unwrap();
        c
    }

    #[test]
    fn insert_and_list_approvals() {
        let c = db();
        let rec = ApprovalRecord {
            id: None,
            attempt_id: None,
            session_id: Some("s1".into()),
            turn_id: Some("t1".into()),
            command: "rm -rf build".into(),
            level: "sensitive".into(),
            reason: Some("sensitive".into()),
            decision: "allow_session".into(),
            mode: "ask_sensitive".into(),
            decided_at: None,
        };
        let id = insert(&c, &rec).unwrap();
        assert!(id > 0);

        let list_all = list(&c, None).unwrap();
        assert_eq!(list_all.len(), 1);
        assert_eq!(list_all[0].command, "rm -rf build");
        assert_eq!(list_all[0].decision, "allow_session");

        let list_s1 = list(&c, Some("s1")).unwrap();
        assert_eq!(list_s1.len(), 1);

        let list_s2 = list(&c, Some("s2")).unwrap();
        assert_eq!(list_s2.len(), 0);
    }
}
