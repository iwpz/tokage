//! Usage and quality dashboard data aggregation (docs/06 §7, docs/10 §3.4, P8.m3b).

use std::collections::HashMap;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tokage_matching::{
    calculate_plan_assignment_savings, determine_reference_model, EffectiveKnowledgePack,
    LayeredPacks, merge_layers, builtin_knowledge_pack,
};
use tokage_core::planning::{TaskItem, TaskSize};
use tokage_storage::savings::{self, SavingsEventRow};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardModelUsage {
    pub model_id: String,
    pub input_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    pub equiv_cost_usd: f64,
    pub calls: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyModelUsagePoint {
    pub model_id: String,
    pub provider: String,
    pub total_tokens: u64,
    pub input_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    pub equiv_cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyUsagePoint {
    pub date: String,
    pub total_tokens: u64,
    pub input_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    pub equiv_cost_usd: f64,
    #[serde(default)]
    pub models: Vec<DailyModelUsagePoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageDashboard {
    pub range: String, // 'today' | '7d' | '30d'
    pub providers: Vec<DashboardProviderInfo>,
    pub quality: DashboardQuality,
    pub savings: DashboardSavings,
    pub cost_split: DashboardCostSplit,
    pub token_series: Vec<DashboardTokenSeriesPoint>,
    pub daily_usage: Vec<DailyUsagePoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardProviderInfo {
    pub provider: String,
    pub billing: String, // 'SUBSCRIPTION' | 'PAY_AS_YOU_GO' | 'UNKNOWN'
    pub is_gateway: bool,
    pub custom_endpoint: Option<String>,
    pub usage_pct: Option<f64>,
    pub usage_pct_source: Option<String>, // 'SNAPSHOT' | 'ESTIMATED'
    pub reset_at: Option<String>,
    pub spent_usd: Option<f64>,
    pub equiv_cost_usd: Option<f64>,
    pub cache_hit_rate: f64,
    pub status: String,
    pub melt_reason: Option<String>,
    pub models: Vec<DashboardModelUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardQuality {
    pub first_pass_rate: f64,
    pub matrix: Vec<QualityMatrixEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityMatrixEntry {
    pub model: String,
    pub domain: String,
    pub tasks: u32,
    pub first_pass_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardSavings {
    pub total_usd: f64,
    pub by_kind: HashMap<String, f64>, // "ASSIGNMENT", "CONTEXT", "DIRECT"
    pub reference_model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardCostSplit {
    pub planning_usd: f64,
    pub execution_usd: f64,
    pub repair_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardTokenSeriesPoint {
    pub bucket: String,
    pub provider: String,
    pub input: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub output: u64,
}

/// Builds effective knowledge pack for calculation (F14: includes enabled customs + overrides).
pub fn get_effective_pack(conn: &Connection) -> EffectiveKnowledgePack {
    let builtin = builtin_knowledge_pack().clone();
    let overrides = tokage_storage::knowledge::list_overrides(conn)
        .unwrap_or_default()
        .into_iter()
        .map(|r| tokage_matching::ModelOverride {
            provider_name: r.provider_name,
            model_id: r.model_id,
            patch: serde_json::from_str(&r.patch_json).unwrap_or_default(),
        })
        .collect();

    let mut customs = Vec::new();
    if let Ok(rows) = tokage_storage::knowledge::list_custom_packs(conn) {
        for row in rows.into_iter().filter(|r| r.enabled) {
            if let Ok(pack) = tokage_matching::parse_knowledge_pack(&row.pack_json) {
                customs.push((row.name, pack));
            }
        }
    }

    let layers = LayeredPacks {
        builtin,
        official: None,
        customs,
        overrides,
        learned_usage_profiles: None,
    };
    merge_layers(&layers).unwrap_or_else(|_| {
        let empty_layers = LayeredPacks {
            builtin: builtin_knowledge_pack().clone(),
            official: None,
            customs: vec![],
            overrides: vec![],
            learned_usage_profiles: None,
        };
        merge_layers(&empty_layers).unwrap()
    })
}

/// Computes the complete UsageDashboard for a given range (docs/06 §7, P8.m3b).
pub fn build_usage_dashboard(conn: &Connection, range: Option<&str>) -> rusqlite::Result<UsageDashboard> {
    let range_str = match range {
        Some("today") => "today",
        Some("30d") => "30d",
        _ => "7d",
    };

    let time_clause = match range_str {
        "today" => "created_at >= date('now', 'start of day')",
        "30d" => "created_at >= datetime('now', '-30 days')",
        _ => "created_at >= datetime('now', '-7 days')",
    };

    let pack = get_effective_pack(conn);
    let ref_model = determine_reference_model(&pack, None);

    // 1. Providers
    let featured_providers = [
        ("CLAUDE_CODE", "SUBSCRIPTION"),
        ("CODEX_CLI", "SUBSCRIPTION"),
        ("CURSOR_CLI", "SUBSCRIPTION"),
        ("COPILOT_CLI", "SUBSCRIPTION"),
        ("ANTIGRAVITY_CLI", "SUBSCRIPTION"),
        ("AIDER", "PAY_AS_YOU_GO"),
        ("OPENCODE", "PAY_AS_YOU_GO"),
    ];

    let mut provider_infos = Vec::new();
    for (pid, billing) in featured_providers {
        // Health
        let (status, melt_reason): (String, Option<String>) = conn
            .query_row(
                "SELECT status, melt_reason FROM provider_health WHERE provider_name = ?1",
                params![pid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap_or_else(|_| ("READY".to_string(), None));

        // Usage events in range
        let query_sql = format!(
            "SELECT COALESCE(SUM(cost_usd), 0.0),
                    COALESCE(SUM(equiv_cost_usd), 0.0),
                    COALESCE(SUM(input_tokens), 0),
                    COALESCE(SUM(cache_read_tokens), 0),
                    COALESCE(SUM(cache_write_tokens), 0)
             FROM usage_events
             WHERE provider_name = ?1 AND {}",
            time_clause
        );

        let (spent, equiv, inp, cr, cw): (f64, f64, i64, i64, i64) = conn
            .query_row(&query_sql, params![pid], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .unwrap_or((0.0, 0.0, 0, 0, 0));

        let provider_id = match pid {
            "CLAUDE_CODE" => Some(tokage_core::provider::ProviderId::ClaudeCode),
            "CODEX_CLI" => Some(tokage_core::provider::ProviderId::CodexCli),
            "CURSOR_CLI" => Some(tokage_core::provider::ProviderId::CursorCli),
            "COPILOT_CLI" => Some(tokage_core::provider::ProviderId::CopilotCli),
            "ANTIGRAVITY_CLI" => Some(tokage_core::provider::ProviderId::AntigravityCli),
            "AIDER" => Some(tokage_core::provider::ProviderId::Aider),
            "OPENCODE" => Some(tokage_core::provider::ProviderId::Opencode),
            _ => None,
        };

        // Snapshot or projection
        let snap = savings::get_latest_usage_snapshot(conn, pid).ok().flatten();
        let (usage_pct, usage_pct_source, reset_at) = if let Some(s) = snap {
            (s.usage_percentage, Some(s.data_source), s.next_reset_time)
        } else if let Some(p_id) = provider_id {
            let (u_now, src) = tokage_native::usage::project_utilization_now(p_id, equiv);
            if u_now > 0.0 {
                let src_str = match src {
                    tokage_native::usage::UsageSource::Estimated => "ESTIMATED",
                    _ => "SNAPSHOT",
                };
                (Some(u_now), Some(src_str.to_string()), None)
            } else {
                (None, None, None)
            }
        } else {
            (None, None, None)
        };

        let denom = (inp + cr + cw) as f64;
        let cache_hit_rate = if denom > 0.0 {
            ((cr as f64) / denom * 100.0).round() / 100.0
        } else {
            0.0
        };

        // Per-model usage for this provider in range
        let model_sql = format!(
            "SELECT COALESCE(model_id, 'default'),
                    COALESCE(SUM(input_tokens), 0),
                    COALESCE(SUM(cache_read_tokens), 0),
                    COALESCE(SUM(cache_write_tokens), 0),
                    COALESCE(SUM(output_tokens), 0),
                    COALESCE(SUM(cost_usd), 0.0),
                    COALESCE(SUM(equiv_cost_usd), 0.0),
                    COUNT(*)
             FROM usage_events
             WHERE provider_name = ?1 AND {}
             GROUP BY 1
             ORDER BY SUM(input_tokens + output_tokens + cache_read_tokens) DESC",
            time_clause
        );
        let mut m_stmt = conn.prepare(&model_sql)?;
        let m_rows = m_stmt.query_map(params![pid], |r| {
            Ok(DashboardModelUsage {
                model_id: r.get(0)?,
                input_tokens: r.get::<_, i64>(1)? as u64,
                cache_read_tokens: r.get::<_, i64>(2)? as u64,
                cache_write_tokens: r.get::<_, i64>(3)? as u64,
                output_tokens: r.get::<_, i64>(4)? as u64,
                cost_usd: r.get(5)?,
                equiv_cost_usd: r.get(6)?,
                calls: r.get::<_, i64>(7)? as u32,
            })
        })?;
        let mut models = Vec::new();
        for m in m_rows {
            models.push(m?);
        }

        // Custom gateway detection (e.g. Claude Code via ANTHROPIC_BASE_URL)
        let (is_gateway, custom_endpoint, final_billing) = if pid == "CLAUDE_CODE" {
            let base_url = std::env::var("ANTHROPIC_BASE_URL").ok().filter(|u| !u.trim().is_empty());
            let custom_url = base_url.or_else(|| {
                let home_dir = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok()?;
                let path = std::path::Path::new(&home_dir).join(".claude").join("settings.json");
                let content = std::fs::read_to_string(path).ok()?;
                let val: serde_json::Value = serde_json::from_str(&content).ok()?;
                val.get("env")?.get("ANTHROPIC_BASE_URL")?.as_str().map(str::to_string)
            });
            if let Some(ref url) = custom_url {
                if !tokage_native::usage::is_official_host(url, &["anthropic.com"]) {
                    (true, tokage_native::usage::display_url(url), "UNKNOWN".to_string())
                } else {
                    (false, None, billing.to_string())
                }
            } else {
                (false, None, billing.to_string())
            }
        } else {
            (false, None, billing.to_string())
        };

        provider_infos.push(DashboardProviderInfo {
            provider: pid.to_string(),
            billing: final_billing,
            is_gateway,
            custom_endpoint,
            usage_pct,
            usage_pct_source,
            reset_at,
            spent_usd: if spent > 0.0 { Some(spent) } else { None },
            equiv_cost_usd: if equiv > 0.0 { Some(equiv) } else { None },
            cache_hit_rate,
            status,
            melt_reason,
            models,
        });
    }

    // 2. Quality from task_outcomes
    let outcomes_sql = format!(
        "SELECT model_id, domain, result FROM task_outcomes WHERE {}",
        time_clause
    );

    let mut stmt = conn.prepare(&outcomes_sql)?;
    let outcome_rows = stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
    })?;

    let mut all_outcomes = Vec::new();
    for row in outcome_rows {
        all_outcomes.push(row?);
    }

    // If no outcomes in range, fallback to querying all to give meaningful quality baseline
    if all_outcomes.is_empty() {
        let mut fallback_stmt = conn.prepare("SELECT model_id, domain, result FROM task_outcomes")?;
        let fb_rows = fallback_stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        })?;
        for row in fb_rows {
            all_outcomes.push(row?);
        }
    }

    let total_tasks = all_outcomes.len() as u32;
    let first_pass_count = all_outcomes.iter().filter(|(_, _, res)| res == "FIRST_PASS").count() as u32;
    let overall_fpr = if total_tasks > 0 {
        ((first_pass_count as f64 / total_tasks as f64) * 100.0).round() / 100.0
    } else {
        1.0
    };

    let mut matrix_map: HashMap<(String, String), (u32, u32)> = HashMap::new();
    for (model, domain, res) in &all_outcomes {
        let entry = matrix_map.entry((model.clone(), domain.clone())).or_insert((0, 0));
        entry.0 += 1;
        if res == "FIRST_PASS" {
            entry.1 += 1;
        }
    }

    let mut matrix = Vec::new();
    for ((model, domain), (tasks, pass)) in matrix_map {
        let fpr = if tasks > 0 {
            ((pass as f64 / tasks as f64) * 100.0).round() / 100.0
        } else {
            1.0
        };
        matrix.push(QualityMatrixEntry {
            model,
            domain,
            tasks,
            first_pass_rate: fpr,
        });
    }
    matrix.sort_by(|a, b| b.tasks.cmp(&a.tasks));

    // 3. Savings from savings_events
    let (total_saved, by_kind) = savings::get_savings_summary(conn, Some(range_str))?;

    // 4. Cost split from usage_events
    let cost_split_sql = format!(
        "SELECT phase, COALESCE(SUM(cost_usd), 0.0)
         FROM usage_events
         WHERE {}
         GROUP BY phase",
        time_clause
    );

    let mut cs_stmt = conn.prepare(&cost_split_sql)?;
    let cs_rows = cs_stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
    })?;

    let mut planning_usd = 0.0;
    let mut execution_usd = 0.0;
    let mut repair_usd = 0.0;

    for row in cs_rows {
        let (phase, cost) = row?;
        match phase.as_str() {
            "PLANNING" => planning_usd += cost,
            "EXECUTION" => execution_usd += cost,
            "REPAIR" => repair_usd += cost,
            _ => execution_usd += cost,
        }
    }

    // 5. Token series
    let bucket_expr = match range_str {
        "today" => "strftime('%H:00', created_at)",
        _ => "strftime('%Y-%m-%d', created_at)",
    };

    let token_sql = format!(
        "SELECT {}, provider_name,
                COALESCE(SUM(input_tokens), 0),
                COALESCE(SUM(cache_read_tokens), 0),
                COALESCE(SUM(cache_write_tokens), 0),
                COALESCE(SUM(output_tokens), 0)
         FROM usage_events
         WHERE {}
         GROUP BY 1, provider_name
         ORDER BY 1 ASC",
        bucket_expr, time_clause
    );

    let mut ts_stmt = conn.prepare(&token_sql)?;
    let ts_rows = ts_stmt.query_map([], |r| {
        Ok(DashboardTokenSeriesPoint {
            bucket: r.get(0)?,
            provider: r.get(1)?,
            input: r.get::<_, i64>(2)? as u64,
            cache_read: r.get::<_, i64>(3)? as u64,
            cache_write: r.get::<_, i64>(4)? as u64,
            output: r.get::<_, i64>(5)? as u64,
        })
    })?;

    let mut token_series = Vec::new();
    for p in ts_rows {
        token_series.push(p?);
    }

    // 6. Daily usage series (for GitHub-style multi-row calendar activity heatmap)
    let daily_clause = "created_at >= datetime('now', '-365 days')";
    let daily_sql = format!(
        "SELECT strftime('%Y-%m-%d', created_at) AS day,
                COALESCE(model_id, 'unknown') AS model_id,
                COALESCE(provider_name, '') AS provider_name,
                COALESCE(SUM(input_tokens), 0),
                COALESCE(SUM(cache_read_tokens), 0),
                COALESCE(SUM(cache_write_tokens), 0),
                COALESCE(SUM(output_tokens), 0),
                COALESCE(SUM(cost_usd), 0.0),
                COALESCE(SUM(equiv_cost_usd), 0.0)
         FROM usage_events
         WHERE {}
         GROUP BY day, model_id, provider_name
         ORDER BY day ASC",
        daily_clause
    );
    let mut d_stmt = conn.prepare(&daily_sql)?;
    let d_rows = d_stmt.query_map([], |r| {
        let day: String = r.get(0)?;
        let model_id: String = r.get(1)?;
        let provider: String = r.get(2)?;
        let inp = r.get::<_, i64>(3)? as u64;
        let cr = r.get::<_, i64>(4)? as u64;
        let cw = r.get::<_, i64>(5)? as u64;
        let outp = r.get::<_, i64>(6)? as u64;
        let cost: f64 = r.get(7)?;
        let equiv: f64 = r.get(8)?;
        Ok((day, model_id, provider, inp, cr, cw, outp, cost, equiv))
    })?;

    let mut day_map: std::collections::BTreeMap<String, DailyUsagePoint> = std::collections::BTreeMap::new();
    for row in d_rows {
        let (day, model_id, provider, inp, cr, cw, outp, cost, equiv) = row?;
        let model_total = inp + cr + cw + outp;
        let entry = day_map.entry(day.clone()).or_insert_with(|| DailyUsagePoint {
            date: day,
            total_tokens: 0,
            input_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            output_tokens: 0,
            cost_usd: 0.0,
            equiv_cost_usd: 0.0,
            models: Vec::new(),
        });
        entry.total_tokens += model_total;
        entry.input_tokens += inp;
        entry.cache_read_tokens += cr;
        entry.cache_write_tokens += cw;
        entry.output_tokens += outp;
        entry.cost_usd += cost;
        entry.equiv_cost_usd += equiv;
        entry.models.push(DailyModelUsagePoint {
            model_id,
            provider,
            total_tokens: model_total,
            input_tokens: inp,
            cache_read_tokens: cr,
            cache_write_tokens: cw,
            output_tokens: outp,
            cost_usd: cost,
            equiv_cost_usd: equiv,
        });
    }

    for pt in day_map.values_mut() {
        pt.models.sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens));
    }
    let daily_usage: Vec<DailyUsagePoint> = day_map.into_values().collect();

    Ok(UsageDashboard {
        range: range_str.to_string(),
        providers: provider_infos,
        quality: DashboardQuality {
            first_pass_rate: overall_fpr,
            matrix,
        },
        savings: DashboardSavings {
            total_usd: total_saved,
            by_kind,
            reference_model: ref_model,
        },
        cost_split: DashboardCostSplit {
            planning_usd,
            execution_usd,
            repair_usd,
        },
        token_series,
        daily_usage,
    })
}

/// Settles a completed plan's savings against the reference model counterfactual baseline (docs/06 §5.2, P8.m3a).
pub fn settle_plan_savings(
    conn: &Connection,
    plan_id: i64,
    user_override: Option<&str>,
) -> rusqlite::Result<Option<i64>> {
    let pack = get_effective_pack(conn);
    let ref_model = determine_reference_model(&pack, user_override);

    // F14.g: backfill tasks.actual_cost_usd from usage_events (COALESCE cost/equiv)
    conn.execute(
        "UPDATE tasks SET actual_cost_usd = (
            SELECT COALESCE(SUM(COALESCE(cost_usd, equiv_cost_usd, 0)), 0)
            FROM usage_events WHERE task_id = tasks.id
         )
         WHERE plan_id = ?1
           AND EXISTS (SELECT 1 FROM usage_events WHERE task_id = tasks.id)",
        params![plan_id],
    )?;

    // Load tasks from plans and tasks table
    let mut stmt = conn.prepare(
        "SELECT code, domain, size, difficulty, COALESCE(actual_cost_usd, 0)
         FROM tasks
         WHERE plan_id = ?1",
    )?;

    let task_rows = stmt.query_map(params![plan_id], |r| {
        let code: String = r.get(0)?;
        let domain: String = r.get(1)?;
        let size_str: String = r.get(2)?;
        let diff: u8 = r.get(3)?;
        let actual_cost: f64 = r.get(4)?;

        let size = match size_str.as_str() {
            "S" => TaskSize::S,
            "L" => TaskSize::L,
            _ => TaskSize::M,
        };

        Ok((
            TaskItem {
                id: code,
                domain,
                title: String::new(),
                goal: String::new(),
                depends_on: vec![],
                contracts: vec![],
                write_scope: vec![],
                deliverables: vec![],
                acceptance: Default::default(),
                size,
                difficulty: diff,
                assigned_model: None,
                model_params: None,
            },
            actual_cost,
        ))
    })?;

    let mut tasks = Vec::new();
    let mut tasks_actual_cost = 0.0;
    for r in task_rows {
        let (task, cost) = r?;
        tasks_actual_cost += cost;
        tasks.push(task);
    }

    if tasks.is_empty() {
        return Ok(None);
    }

    // Plan-level usage with COALESCE (subscription may only have equiv_cost_usd)
    let usage_cost: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(COALESCE(cost_usd, equiv_cost_usd, 0)), 0.0)
             FROM usage_events WHERE plan_id = ?1",
            params![plan_id],
            |r| r.get(0),
        )
        .unwrap_or(0.0);
    let effective_actual_cost = tasks_actual_cost.max(usage_cost);

    let (baseline, actual, _) =
        calculate_plan_assignment_savings(&tasks, effective_actual_cost, &ref_model, &pack);

    let detail = serde_json::json!({
        "referenceModel": ref_model,
        "taskCount": tasks.len(),
        "baselineUsd": baseline,
        "actualUsd": actual,
    })
    .to_string();

    let event = SavingsEventRow {
        id: None,
        main_session_id: conn
            .query_row(
                "SELECT main_session_id FROM plans WHERE id = ?1",
                params![plan_id],
                |r| r.get(0),
            )
            .ok(),
        plan_id: Some(plan_id),
        kind: "ASSIGNMENT".to_string(),
        ref_id: Some(format!("plan-{}", plan_id)),
        baseline_usd: baseline,
        actual_usd: Some(actual),
        settled: true,
        detail_json: detail,
        created_at: None,
    };

    let id = savings::insert_savings_event(conn, &event)?;
    Ok(Some(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        tokage_storage::run_migrations(&mut conn).unwrap();
        conn
    }

    #[test]
    fn test_build_usage_dashboard_empty_db() {
        let conn = in_memory_db();
        let dash = build_usage_dashboard(&conn, Some("7d")).expect("Dashboard should build cleanly");
        assert_eq!(dash.range, "7d");
        assert!(!dash.providers.is_empty());
        assert_eq!(dash.quality.first_pass_rate, 1.0);
        assert_eq!(dash.savings.total_usd, 0.0);
    }

    #[test]
    fn test_settle_plan_savings_flow() {
        let conn = in_memory_db();
        conn.execute("INSERT INTO projects (id, name, local_path) VALUES ('p1', 'Test', '/tmp')", []).unwrap();
        conn.execute("INSERT INTO main_sessions (id, project_id, title) VALUES ('s1', 'p1', 'Session')", []).unwrap();
        conn.execute("INSERT INTO plans (id, main_session_id, goal, spec_json, est_cost_usd) VALUES (1, 's1', 'Goal', '{}', 0.50)", []).unwrap();
        conn.execute(
            "INSERT INTO tasks (id, plan_id, code, domain, title, goal, size, difficulty, actual_cost_usd)
             VALUES (1, 1, 'T1', 'architecture', 'Arch', 'Design', 'M', 2, 0.20)",
            [],
        )
        .unwrap();

        let event_id = settle_plan_savings(&conn, 1, None).unwrap().expect("Event created");
        assert!(event_id > 0);

        let dash = build_usage_dashboard(&conn, Some("7d")).unwrap();
        assert!(dash.savings.total_usd >= 0.0);
        assert!(dash.savings.by_kind.contains_key("ASSIGNMENT"));
    }

    #[test]
    fn test_dashboard_model_breakdown_and_daily_usage() {
        let conn = in_memory_db();
        conn.execute(
            "INSERT INTO usage_events (provider_name, model_id, phase, source, input_tokens, cache_read_tokens, cache_write_tokens, output_tokens, cost_usd, equiv_cost_usd, created_at)
             VALUES ('CLAUDE_CODE', 'claude-3-7-sonnet', 'EXECUTION', 'STRUCTURED', 1000, 2000, 500, 300, 0.05, 0.08, datetime('now'))",
            [],
        )
        .unwrap();

        let dash = build_usage_dashboard(&conn, Some("7d")).unwrap();
        let claude = dash.providers.iter().find(|p| p.provider == "CLAUDE_CODE").unwrap();
        assert_eq!(claude.models.len(), 1);
        assert_eq!(claude.models[0].model_id, "claude-3-7-sonnet");
        assert_eq!(claude.models[0].input_tokens, 1000);
        assert_eq!(claude.models[0].cache_read_tokens, 2000);
        assert_eq!(claude.models[0].output_tokens, 300);
        assert!(!dash.daily_usage.is_empty());
        assert_eq!(dash.daily_usage[0].total_tokens, 3800);
        assert_eq!(dash.daily_usage[0].models.len(), 1);
        assert_eq!(dash.daily_usage[0].models[0].model_id, "claude-3-7-sonnet");
        assert_eq!(dash.daily_usage[0].models[0].total_tokens, 3800);
    }
}
