//! Budget reads retain missing prices instead of treating them as known zero cost.
use rusqlite::{Connection, Result};

pub fn plan_cost_summary(conn: &Connection, plan_id: i64) -> Result<serde_json::Value> {
    let mut sums = [0.0; 4];
    let mut unknown = 0i64;
    let mut stmt = conn.prepare(
        "SELECT phase, SUM(COALESCE(cost_usd, equiv_cost_usd, 0)),
                SUM(CASE WHEN cost_usd IS NULL AND equiv_cost_usd IS NULL THEN 1 ELSE 0 END)
         FROM usage_events WHERE plan_id = ?1 GROUP BY phase",
    )?;
    let rows = stmt.query_map([plan_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?, r.get::<_, i64>(2)?))
    })?;
    for row in rows {
        let (phase, sum, missing) = row?;
        let index = match phase.as_str() { "PLANNING" => 0, "EXECUTION" => 1, "REPAIR" => 2, _ => 3 };
        sums[index] += sum;
        unknown += missing;
    }
    unknown += conn.query_row("SELECT COUNT(*) FROM recovery_usage_gaps g WHERE g.plan_id=?1
        AND NOT EXISTS(SELECT 1 FROM usage_receipts r WHERE r.event_key=g.turn_id)", [plan_id], |row| row.get::<_, i64>(0))?;
    Ok(serde_json::json!({ "planning": sums[0], "execution": sums[1], "repair": sums[2],
        "total": sums.iter().sum::<f64>(), "unknownCostEvents": unknown }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn m2d_unknown_is_not_free_and_fallback_prices_remain_visible() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE recovery_usage_gaps (turn_id TEXT, plan_id INTEGER);
            CREATE TABLE usage_receipts (event_key TEXT);
            CREATE TABLE usage_events (plan_id INTEGER, phase TEXT, cost_usd REAL, equiv_cost_usd REAL);
            INSERT INTO usage_events VALUES (1,'PLANNING',1,NULL),(1,'EXECUTION',NULL,2),
            (1,'REPAIR',NULL,NULL),(1,'OTHER',0,NULL),(2,'REPAIR',10,NULL);").unwrap();
        let summary = plan_cost_summary(&db, 1).unwrap();
        assert_eq!(summary["total"], 3.0);
        assert_eq!(summary["unknownCostEvents"], 1);
        db.execute_batch("INSERT INTO recovery_usage_gaps VALUES ('lost-done',1)").unwrap();
        assert_eq!(plan_cost_summary(&db, 1).unwrap()["unknownCostEvents"], 2);
        db.execute_batch("INSERT INTO usage_receipts VALUES ('lost-done')").unwrap();
        assert_eq!(plan_cost_summary(&db, 1).unwrap()["unknownCostEvents"], 1);
        assert_eq!(plan_cost_summary(&db, 3).unwrap()["unknownCostEvents"], 0);
        db.execute_batch("DROP TABLE usage_events").unwrap();
        assert!(plan_cost_summary(&db, 1).is_err());
    }
}
