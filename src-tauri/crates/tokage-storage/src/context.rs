//! Context action persistence and actual savings settlement (docs/08 schema.md §4, P8.m1f).

use rusqlite::{params, Connection, OptionalExtension};
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
    if !matches!(action.kind.as_str(),"COMPACT"|"FRESH") || action.status != "SUGGESTED" || target_id.is_empty() {
        return Err(rusqlite::Error::InvalidQuery);
    }
    if action.kind=="FRESH" && serde_json::from_str::<serde_json::Value>(&action.est_costs_json)
        .ok().is_none_or(|m|m["settlementV1"]["method"].as_str()!=Some("fresh")) { return Err(rusqlite::Error::InvalidQuery); }
    let id = insert_context_action(conn, action)?;
    update_context_action_status(conn, id, "APPLIED", Some(target_id))?;
    Ok(id)
}

/// The most recent queue receipt for this source/session controls auto requeue
/// after reopening. Explicit manual acceptance remains possible.
pub fn queued_compact_is_dismissed(conn:&Connection, attempt_id:&str, session_id:&str)->rusqlite::Result<bool> {
    let rows=list_context_actions(conn,attempt_id)?;
    for row in rows.iter().rev() {
        let metadata:serde_json::Value=serde_json::from_str(&row.est_costs_json).map_err(|_|rusqlite::Error::InvalidQuery)?;
        if metadata["queuedIntent"]["scope"]["sessionId"].as_str()==Some(session_id) {
            return Ok(row.status=="UNDONE" && row.new_attempt_id.is_none());
        }
    }
    Ok(false)
}

/// Undo an unsent suggestion without creating a cooldown/reset anchor.
pub fn undo_queued_compact(conn: &Connection, action_id: i64, session_id: &str) -> rusqlite::Result<()> {
    let row = get_context_action(conn, action_id)?.ok_or(rusqlite::Error::InvalidQuery)?;
    let metadata: serde_json::Value = serde_json::from_str(&row.est_costs_json).map_err(|_| rusqlite::Error::InvalidQuery)?;
    if !matches!(row.kind.as_str(),"COMPACT"|"FRESH") || row.new_attempt_id.is_some()
        || metadata["queuedIntent"]["scope"]["sessionId"].as_str() != Some(session_id)
        || !matches!(row.status.as_str(), "SUGGESTED" | "UNDONE") {
        return Err(rusqlite::Error::InvalidQuery);
    }
    conn.execute("UPDATE context_actions SET status='UNDONE' WHERE id=?1", [action_id])?;
    Ok(())
}

/// Bind the original suggestion to one carrier using freshly resolved estimates.
/// Only the exact queued scope/material may be consumed; undone/applied rows fail closed.
pub fn prepare_queued_compact(conn: &Connection, action_id: i64, expected: &serde_json::Value,
    action: &ContextActionRow, target_id: &str) -> rusqlite::Result<i64> {
    let row = get_context_action(conn, action_id)?.ok_or(rusqlite::Error::InvalidQuery)?;
    let metadata: serde_json::Value = serde_json::from_str(&row.est_costs_json).map_err(|_| rusqlite::Error::InvalidQuery)?;
    if !matches!(row.kind.as_str(),"COMPACT"|"FRESH") || row.kind!=action.kind || row.status != "SUGGESTED" || row.new_attempt_id.is_some()
        || row.attempt_id != action.attempt_id || metadata["queuedIntent"] != *expected || target_id.is_empty() {
        return Err(rusqlite::Error::InvalidQuery);
    }
    if action.kind=="FRESH" && serde_json::from_str::<serde_json::Value>(&action.est_costs_json)
        .ok().is_none_or(|m|m["settlementV1"]["method"].as_str()!=Some("fresh")) { return Err(rusqlite::Error::InvalidQuery); }
    conn.execute("UPDATE context_actions SET est_costs_json=?1, est_saving_pct=?2 WHERE id=?3",
        params![action.est_costs_json, action.est_saving_pct, action_id])?;
    update_context_action_status(conn, action_id, "APPLIED", Some(target_id))?;
    Ok(action_id)
}

/// Only the bound carrier may mark an accepted action failed or undone.
pub fn fail_compact_carrier(conn: &Connection, action_id: i64, target_id: &str, cancelled_before_send: bool) -> rusqlite::Result<()> {
    let row = get_context_action(conn, action_id)?.ok_or(rusqlite::Error::InvalidQuery)?;
    let completed: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM savings_events WHERE kind='CONTEXT' AND ref_id=?1)", [format!("context_action:{action_id}")], |r| r.get(0))?;
    if completed || row.new_attempt_id.as_deref() != Some(target_id) || !matches!(row.kind.as_str(),"COMPACT"|"FRESH") {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let status = if cancelled_before_send { "UNDONE" } else { "FAILED" };
    update_context_action_status(conn, action_id, status, None)
}

/// Persist one completed carrier and its first-turn counterfactual in the caller's transaction.
pub fn settle_compact_carrier(conn: &Connection, action_id: i64, target_id: &str, cost_usd: Option<f64>) -> rusqlite::Result<()> {
    if cost_usd.is_some_and(|v| !v.is_finite() || v < 0.0) { return Err(rusqlite::Error::InvalidQuery); }
    let row = get_context_action(conn, action_id)?.ok_or(rusqlite::Error::InvalidQuery)?;
    if row.status != "APPLIED" || !matches!(row.kind.as_str(),"COMPACT"|"FRESH") || row.new_attempt_id.as_deref() != Some(target_id) { return Err(rusqlite::Error::InvalidQuery); }
    let metadata: serde_json::Value = serde_json::from_str(&row.est_costs_json).map_err(|_| rusqlite::Error::InvalidQuery)?;
    let spec = &metadata["settlementV1"];
    if spec["horizonTurns"].as_u64() != Some(1) { return Err(rusqlite::Error::InvalidQuery); }
    let baseline = spec["continueEstimateUsd"].as_f64().filter(|v| v.is_finite() && *v >= 0.0).ok_or(rusqlite::Error::InvalidQuery)?;
    let method = spec["method"].as_str().ok_or(rusqlite::Error::InvalidQuery)?;
    if (method != "fresh" && method != "instruction" && method!="native") || (row.kind=="FRESH" && method!="fresh") { return Err(rusqlite::Error::InvalidQuery); }
    let generated = spec["generatedReport"].as_bool()==Some(true);
    let report_cost = if generated {spec["reportCostUsd"].as_f64()} else {Some(0.0)};
    if report_cost.is_some_and(|v|!v.is_finite() || v<0.0) {return Err(rusqlite::Error::InvalidQuery);}
    let actual = if method == "fresh" || method=="native" { cost_usd.zip(report_cost).map(|(carrier,report)|carrier+report) } else { None };
    if actual.is_some_and(|v|!v.is_finite()) {return Err(rusqlite::Error::InvalidQuery);}

    let mut detail = serde_json::json!({"basis":"first_turn_counterfactual_v1","actionId":action_id,"targetId":target_id,"method":method,"horizonTurns":1,"continueEstimateUsd":baseline,"observedCarrierCostUsd":cost_usd,"params":spec["params"],"sessionId":spec["sessionId"],"provider":spec["provider"],"model":spec["model"],"packVersion":spec["packVersion"],"sources":spec["sources"],"reason":if method == "instruction" {"context_reduction_unverified"} else if report_cost.is_none() {"report_cost_unknown"} else if cost_usd.is_none() {if method=="native" {"intervention_cost_unknown"} else {"carrier_cost_unknown"}} else {"counterfactual_estimate"}});
    if method=="native" {
        detail.as_object_mut().unwrap().remove("observedCarrierCostUsd");
        detail["observedInterventionCostUsd"]=serde_json::json!(cost_usd);
        detail["costScope"]=serde_json::json!("native_compact_plus_first_user_turn");
    }
    // Keep legacy settlement detail stable for idempotent retries.
    if generated {
        detail["observedReportCostUsd"]=serde_json::json!(report_cost);
        detail["reportTurnId"]=spec["reportTurnId"].clone();
        detail["actualInterventionCostUsd"]=serde_json::json!(actual);
    }
    let detail=detail.to_string();
    let reference = format!("context_action:{action_id}");
    let existing: Option<String> = conn.query_row("SELECT detail_json FROM savings_events WHERE kind='CONTEXT' AND ref_id=?1", [&reference], |r| r.get(0)).optional()?;
    if let Some(existing) = existing { return if existing == detail { Ok(()) } else { Err(rusqlite::Error::InvalidQuery) }; }
    crate::savings::insert_savings_event(conn, &crate::savings::SavingsEventRow { id: None, main_session_id: None, plan_id: None, kind: "CONTEXT".into(), ref_id: Some(reference), baseline_usd: baseline, actual_usd: actual, settled: actual.is_some(), detail_json: detail, created_at: None })?;
    if let Some(actual) = actual {
        settle_context_action_savings(conn, action_id, (baseline - actual).max(0.0))?;
    }
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
        let mut c=Connection::open_in_memory().unwrap();crate::embedded::migrations::runner().run(&mut c).unwrap();c
    }
    fn row(kind:&str,status:&str)->ContextActionRow {
        ContextActionRow{id:None,attempt_id:"a".into(),kind:kind.into(),trigger:"USER".into(),status:status.into(),est_costs_json:"{}".into(),est_saving_pct:0.,actual_saving_usd:None,new_attempt_id:None,created_at:None,applied_at:Some("1970-01-01T00:00:01.500Z".into())}
    }
    #[test]
    fn queued_suggestions_undo_without_reset_and_bind_once_to_fresh_estimates() {
        let c = database();
        let intent = serde_json::json!({"scope":{"sessionId":"s","turnId":"a"},"retained":{"taskGoal":"goal"}});
        let mut queued = row("COMPACT", "SUGGESTED");
        queued.applied_at = None;
        queued.est_costs_json = serde_json::json!({"queuedIntent":intent}).to_string();
        let undone = insert_context_action(&c, &queued).unwrap();
        assert!(undo_queued_compact(&c, undone, "other").is_err());
        undo_queued_compact(&c, undone, "s").unwrap();
        undo_queued_compact(&c, undone, "s").unwrap();
        let saved = get_context_action(&c, undone).unwrap().unwrap();
        assert_eq!(saved.status, "UNDONE"); assert!(saved.applied_at.is_none());
        assert!(queued_compact_is_dismissed(&c,"a","s").unwrap());
        assert!(!queued_compact_is_dismissed(&c,"a","other").unwrap());
        assert!(list_context_reset_actions(&c, &["a".into()]).unwrap().is_empty());
        let mut action = row("COMPACT", "SUGGESTED");
        action.est_costs_json = serde_json::json!({"settlementV1":{"method":"fresh","horizonTurns":1,"continueEstimateUsd":0.5}}).to_string();
        assert!(prepare_queued_compact(&c, undone, &intent, &action, "target").is_err());
        let id = insert_context_action(&c, &queued).unwrap();
        assert!(!queued_compact_is_dismissed(&c,"a","s").unwrap());
        assert!(prepare_queued_compact(&c, id, &serde_json::json!({}), &action, "target").is_err());
        assert_eq!(prepare_queued_compact(&c, id, &intent, &action, "target").unwrap(), id);
        let saved = get_context_action(&c, id).unwrap().unwrap();
        assert_eq!(saved.status, "APPLIED"); assert_eq!(saved.new_attempt_id.as_deref(), Some("target"));
        assert_eq!(saved.est_costs_json, action.est_costs_json);
        assert!(prepare_queued_compact(&c, id, &intent, &action, "other").is_err());
        assert!(undo_queued_compact(&c, id, "s").is_err());
        settle_compact_carrier(&c, id, "target", Some(0.2)).unwrap();
        assert!((get_context_action(&c, id).unwrap().unwrap().actual_saving_usd.unwrap()-0.3).abs()<1e-9);
    }

    #[test]
    fn fresh_action_queues_undoes_binds_and_settles_only_a_new_window() {
        let c=database();let expected=serde_json::json!({"scope":{"sessionId":"s"}});
        let mut action=row("FRESH","SUGGESTED");action.applied_at=None;
        action.est_costs_json=serde_json::json!({"queuedIntent":expected}).to_string();
        let undone=insert_context_action(&c,&action).unwrap();undo_queued_compact(&c,undone,"s").unwrap();
        assert!(queued_compact_is_dismissed(&c,"a","s").unwrap());
        let id=insert_context_action(&c,&action).unwrap();
        action.est_costs_json=serde_json::json!({"settlementV1":{"method":"instruction","horizonTurns":1,"continueEstimateUsd":0.5}}).to_string();
        assert!(prepare_queued_compact(&c,id,&expected,&action,"new").is_err());
        assert!(get_context_action(&c,id).unwrap().unwrap().new_attempt_id.is_none());
        assert!(settle_compact_carrier(&c,id,"new",Some(0.1)).is_err());
        let id2=insert_context_action(&c,&row("FRESH","SUGGESTED")).unwrap();
        let mut fresh=row("FRESH","SUGGESTED");fresh.est_costs_json=serde_json::json!({"settlementV1":{"method":"fresh","horizonTurns":1,"continueEstimateUsd":0.5}}).to_string();
        let accepted=accept_compact_carrier(&c,&fresh,"target").unwrap();
        settle_compact_carrier(&c,accepted,"target",Some(0.1)).unwrap();
        assert!((get_context_action(&c,accepted).unwrap().unwrap().actual_saving_usd.unwrap()-0.4).abs()<1e-9);
        assert!(get_context_action(&c,id2).unwrap().unwrap().new_attempt_id.is_none());
    }

    #[test]
    fn carrier_settlement_is_scoped_idempotent_and_preserves_unknown() {
        let c = database();
        for (method, cost, expected) in [("fresh",Some(0.2),Some(0.3)),("fresh",Some(0.8),Some(0.0)),("fresh",Some(0.5),Some(0.0)),("fresh",Some(0.0),Some(0.5)),("fresh",None,None),("instruction",Some(0.2),None)] {
            let mut action = row("COMPACT", "SUGGESTED");
            action.est_costs_json = serde_json::json!({"settlementV1":{"method":method,"horizonTurns":1,"continueEstimateUsd":0.5,"params":{},"sessionId":"session"}}).to_string();
            let id = accept_compact_carrier(&c, &action, "carrier").unwrap();
            assert!(settle_compact_carrier(&c,id,"wrong",cost).is_err());
            settle_compact_carrier(&c,id,"carrier",cost).unwrap();
            settle_compact_carrier(&c,id,"carrier",cost).unwrap();
            assert!(settle_compact_carrier(&c,id,"carrier",Some(99.0)).is_err());
            assert!(fail_compact_carrier(&c,id,"carrier",false).is_err());
            let actual = get_context_action(&c,id).unwrap().unwrap().actual_saving_usd;
            match (actual,expected) { (Some(a),Some(e)) => assert!((a-e).abs()<1e-9), (None,None)=>(), _=>panic!("unexpected saving") }
        }
        let events = crate::savings::list_savings_events(&c,None).unwrap();
        assert_eq!(events.len(),6);
        assert_eq!(events.iter().filter(|e|e.settled).count(),4);
        assert!((crate::savings::get_savings_summary(&c,None).unwrap().0-0.8).abs()<1e-9);
    }
    #[test]
    fn native_settlement_uses_combined_cost_and_never_promotes_unknown() {
        let c=database();
        for cost in [Some(0.07),None] {
            let mut action=row("COMPACT","SUGGESTED");
            action.est_costs_json=serde_json::json!({"settlementV1":{"method":"native","horizonTurns":1,"continueEstimateUsd":0.5}}).to_string();
            let id=accept_compact_carrier(&c,&action,"carrier").unwrap();
            settle_compact_carrier(&c,id,"carrier",cost).unwrap();
            settle_compact_carrier(&c,id,"carrier",cost).unwrap();
            let events=crate::savings::list_savings_events(&c,None).unwrap();
            let event=events.iter().find(|e|e.ref_id==Some(format!("context_action:{id}"))).unwrap();
            assert_eq!(event.actual_usd,cost);assert_eq!(event.settled,cost.is_some());
            let detail:serde_json::Value=serde_json::from_str(&event.detail_json).unwrap();
            assert_eq!(detail["costScope"],"native_compact_plus_first_user_turn");
            assert!(detail.get("observedCarrierCostUsd").is_none());
        }
    }

    #[test]
    fn generated_report_cost_is_part_of_the_intervention_and_unknown_is_not_zero() {
        let c=database();
        for (report,cost,expected) in [(Some(0.1),Some(0.2),Some(0.2)),(Some(0.4),Some(0.2),Some(0.0)),(None,Some(0.2),None),(Some(0.1),None,None)] {
            let mut action=row("FRESH","SUGGESTED");
            action.est_costs_json=serde_json::json!({"settlementV1":{"method":"fresh","horizonTurns":1,"continueEstimateUsd":0.5,"generatedReport":true,"reportCostUsd":report,"reportTurnId":"report"}}).to_string();
            let id=accept_compact_carrier(&c,&action,"carrier").unwrap();
            settle_compact_carrier(&c,id,"carrier",cost).unwrap();
            settle_compact_carrier(&c,id,"carrier",cost).unwrap();
            match (get_context_action(&c,id).unwrap().unwrap().actual_saving_usd,expected) {
                (Some(a),Some(e))=>assert!((a-e).abs()<1e-9),(None,None)=>(),_=>panic!("incorrect combined cost"),
            }
            let events=crate::savings::list_savings_events(&c,None).unwrap();
            let event=events.iter().find(|e|e.ref_id==Some(format!("context_action:{id}"))).unwrap();
            let detail:serde_json::Value=serde_json::from_str(&event.detail_json).unwrap();
            assert_eq!(detail["observedReportCostUsd"],serde_json::json!(report));
            assert_eq!(event.settled,report.is_some() && cost.is_some());
        }
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
