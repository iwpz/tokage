//! Read-only quality/learning derived from this application's durable runtime records.
//! Legacy outcomes remain historical: their unscoped task IDs cannot prove first pass.
use std::path::Path;
use tokage_runtime::quality::Observation;

pub fn observations(data: &Path) -> Result<Vec<Observation>, String> {
    tokage_runtime::quality::read(&data.join("runtime-v1")).map_err(|e| e.to_string())
}
/// Quality observations are recomputed, not appended. Re-reading or retrying a run
/// cannot teach it twice; historical unverified success counters stay on disk.
pub fn learned_rows(
    storage: &tokage_storage::Storage,
    data: &Path,
) -> Result<Vec<tokage_storage::LearnedStatRow>, String> {
    let mut rows = storage.list_all_learned_stats()?;
    rows.retain(|r| {
        !matches!(
            r.metric.as_str(),
            "success_alpha" | "success_beta" | "fix_rate"
        )
    });
    for r in tokage_runtime::quality::learned(&observations(data)?) {
        for (metric, value) in [("success_alpha", r.alpha), ("success_beta", r.beta)] {
            rows.push(tokage_storage::LearnedStatRow {
                provider_name: r.provider.clone(),
                model_id: r.model.clone(),
                domain: r.domain.clone(),
                bucket: r.bucket.clone(),
                metric: metric.into(),
                value,
                samples: r.samples,
                updated_at: None,
            });
        }
    }
    Ok(rows)
}

pub fn dashboard(
    conn: &rusqlite::Connection,
    rows: Vec<Observation>,
    range: &str,
    now: u64,
) -> Result<crate::dashboard::DashboardQuality, String> {
    let seconds = match range {
        "today" => now % 86400,
        "30d" => 30 * 86400,
        _ => 7 * 86400,
    };
    let since = now.saturating_sub(seconds);
    let unknown_time_samples = rows
        .iter()
        .filter(|r| r.resolved() && r.observed_at.is_none())
        .count() as u32;
    let rows = rows
        .into_iter()
        .filter(|r| {
            r.observed_at.is_some_and(|t| t >= since && t <= now)
                || r.resolved() && r.observed_at.is_none()
        })
        .collect::<Vec<_>>();
    let resolved = rows
        .iter()
        .filter(|r| r.resolved() && r.observed_at.is_some())
        .collect::<Vec<_>>();
    let samples = resolved.len() as u32;
    let passed = resolved
        .iter()
        .filter(|r| r.outcome == "FIRST_PASS")
        .count() as u32;
    let mut groups = std::collections::BTreeMap::<(String, String), (u32, u32)>::new();
    for r in resolved {
        let g = groups
            .entry((r.model.clone(), r.domain.clone()))
            .or_default();
        g.0 += 1;
        if r.outcome == "FIRST_PASS" {
            g.1 += 1;
        }
    }
    let matrix = groups
        .into_iter()
        .map(
            |((model, domain), (tasks, passes))| crate::dashboard::QualityMatrixEntry {
                model,
                domain,
                tasks,
                first_pass_rate: passes as f64 / tasks as f64,
            },
        )
        .collect();
    let days = match range {
        "today" => "0 days",
        "30d" => "-30 days",
        _ => "-7 days",
    };
    let legacy_unverified_samples:u32=conn.query_row("SELECT count(*) FROM task_outcomes WHERE created_at >= CASE WHEN ?1='0 days' THEN date('now') ELSE datetime('now',?1) END",[days],|r|r.get(0)).map_err(|e|e.to_string())?;
    Ok(crate::dashboard::DashboardQuality {
        first_pass_rate: if samples == 0 {
            None
        } else {
            Some(passed as f64 / samples as f64)
        },
        matrix,
        samples,
        legacy_unverified_samples,
        unknown_time_samples,
        observations: rows,
    })
}

/// Shared by the production IPC and tests: the selected data root is explicit.
pub fn usage_dashboard(
    conn: &rusqlite::Connection,
    data: &Path,
    range: Option<&str>,
) -> Result<crate::dashboard::UsageDashboard, String> {
    let mut view =
        crate::dashboard::build_usage_dashboard(conn, range).map_err(|e| e.to_string())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    view.quality = dashboard(conn, observations(data)?, &view.range, now)?;
    Ok(view)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(key: &str, outcome: &str, at: Option<u64>) -> Observation {
        Observation {
            key: key.into(),
            run_id: "run".into(),
            task_id: key.into(),
            revision: 10,
            run_state: tokage_runtime::RunState::AwaitingMerge,
            provider: "CODEX_CLI".into(),
            model: "requested-model".into(),
            model_basis: "REQUESTED".into(),
            domain: "testing".into(),
            difficulty: 2,
            outcome: outcome.into(),
            learning_eligible: matches!(outcome, "FIRST_PASS" | "RETRIED"),
            attempt_ids: vec!["attempt".into()],
            observed_at: at,
            evidence_digest: "evidence".into(),
        }
    }
    #[test]
    fn range_is_not_replaced_with_all_time_and_legacy_rows_do_not_prove_success() {
        let c = rusqlite::Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE task_outcomes(created_at TEXT); INSERT INTO task_outcomes VALUES(datetime('now'));").unwrap();
        let now = 2_000_000;
        let rows = vec![
            row("first", "FIRST_PASS", Some(now - 1)),
            row("retry", "RETRIED", Some(now - 2)),
            row("blocked", "BLOCKED", Some(now - 1)),
            row("old", "FIRST_PASS", Some(1)),
            row("undated", "FIRST_PASS", None),
        ];
        let q = dashboard(&c, rows, "7d", now).unwrap();
        assert_eq!(q.samples, 2);
        assert_eq!(q.first_pass_rate, Some(0.5));
        assert_eq!(q.matrix[0].tasks, 2);
        assert_eq!(q.legacy_unverified_samples, 1);
        assert_eq!(q.unknown_time_samples, 1);
        assert_eq!(q.observations.len(), 4); // unresolved status visible, excluded from denominator
        let empty = dashboard(&c, vec![row("old", "FIRST_PASS", Some(1))], "7d", now).unwrap();
        assert_eq!(empty.first_pass_rate, None);
        assert_eq!(empty.samples, 0);
        assert!(empty.matrix.is_empty());
    }
    #[test]
    fn latest_projection_replaces_provisional_and_does_not_increment_learning_on_replay() {
        let first = row("same", "FIRST_PASS", Some(1));
        let mut newer = row("same", "BLOCKED", Some(2));
        newer.revision = 11;
        assert!(tokage_runtime::quality::learned(&[first.clone(), newer.clone()]).is_empty());
        let once = tokage_runtime::quality::learned(std::slice::from_ref(&first));
        let twice = tokage_runtime::quality::learned(&[first.clone(), first]);
        assert_eq!(
            serde_json::to_value(once).unwrap(),
            serde_json::to_value(twice).unwrap()
        );
    }
    #[tokio::test]
    async fn unverified_learning_is_preserved_on_disk_but_not_used_as_verified_quality() {
        let d = tempfile::tempdir().unwrap();
        let storage = tokage_storage::Storage::open(d.path()).unwrap();
        storage
            .update_learned_stat(tokage_storage::LearnedStatRow {
                provider_name: "*".into(),
                model_id: "old".into(),
                domain: "testing".into(),
                bucket: "d2".into(),
                metric: "success_alpha".into(),
                value: 99.0,
                samples: 99,
                updated_at: None,
            })
            .await
            .unwrap();
        assert!(learned_rows(&storage, d.path()).unwrap().is_empty());
        assert_eq!(storage.list_all_learned_stats().unwrap()[0].value, 99.0);
        assert!(!d.path().join("runtime-v1").exists());
    }
}

#[cfg(test)]
mod production_read_tests {
    #[test]
    fn ipc_read_path_keeps_legacy_unverified_and_rejects_corrupt_runtime_without_writing() {
        let d = tempfile::tempdir().unwrap();
        let storage = tokage_storage::Storage::open(d.path()).unwrap();
        let conn = storage.read_conn().unwrap();
        let view = super::usage_dashboard(&conn, d.path(), Some("7d")).unwrap();
        assert_eq!(view.quality.first_pass_rate, None);
        assert!(!d.path().join("runtime-v1").exists());
        let dir = d.path().join("runtime-v1");
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("tokage.db");
        std::fs::write(&path, b"invalid database").unwrap();
        assert!(super::usage_dashboard(&conn, d.path(), Some("7d")).is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"invalid database");
        assert_eq!(std::fs::read_dir(dir).unwrap().count(), 1); // no owner/migration artifacts
    }
}
