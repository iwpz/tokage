//! Orchestration database operations (docs/08 schema.md §2-§4, P7.m1 & P7.m2).

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

/// Record for a task report in `task_reports`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskReportRow {
    pub session_id: String,
    pub plan_record_id: i64,
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
    pub session_id: String,
    pub plan_record_id: i64,
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
            session_id, plan_record_id, task_id, model_id, attempt_id, first_pass, is_locally_generated,
            summary_md, full_md, contract_deviation, changed_files_stat,
            changed_files_list, gate_results, cost_usd
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            report.session_id,
            report.plan_record_id,
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
pub fn get_task_reports(conn: &Connection, session_id: &str, plan_record_id: i64, task_id: &str) -> rusqlite::Result<Vec<TaskReportRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, task_id, model_id, attempt_id, first_pass, is_locally_generated,
                summary_md, full_md, contract_deviation, changed_files_stat,
                changed_files_list, gate_results, cost_usd, created_at
         FROM task_reports
         WHERE task_id = ?1 AND session_id = ?2 AND plan_record_id = ?3 AND association_status = 'BOUND'
         ORDER BY id ASC",
    )?;

    let rows = stmt.query_map(params![task_id, session_id, plan_record_id], |row| {
        let changed_files_raw: String = row.get(10)?;
        let changed_files_list: Vec<String> = serde_json::from_str(&changed_files_raw).unwrap_or_default();
        Ok(TaskReportRow {
            session_id: session_id.to_string(),
            plan_record_id,
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
        "INSERT INTO briefs (task_id, kind, content, token_count, edited_by_user, session_id, plan_record_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            brief.task_id,
            brief.kind,
            brief.content,
            brief.token_count,
            if brief.edited_by_user { 1 } else { 0 },
            brief.session_id,
            brief.plan_record_id,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Gets the latest brief for a task ID.
pub fn get_latest_brief(conn: &Connection, session_id: &str, plan_record_id: i64, task_id: &str) -> rusqlite::Result<Option<BriefRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, task_id, kind, content, token_count, edited_by_user, created_at
         FROM briefs
         WHERE task_id = ?1 AND session_id = ?2 AND plan_record_id = ?3 AND association_status = 'BOUND'
         ORDER BY id DESC LIMIT 1",
    )?;

    let mut rows = stmt.query(params![task_id, session_id, plan_record_id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(BriefRow {
            session_id: session_id.to_string(),
            plan_record_id,
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
    pub session_id: String,
    pub plan_record_id: i64,
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
            duration_ms, output_tail, error_message, session_id, plan_record_id
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
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
            run.session_id,
            run.plan_record_id,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Lists all gate runs for a task or plan.
pub fn list_gate_runs(conn: &Connection, session_id: &str, plan_record_id: i64, task_id: Option<&str>) -> rusqlite::Result<Vec<GateRunRow>> {
    let mut query = "SELECT id, task_id, plan_id, attempt_id, command, exit_code, passed,
                            duration_ms, output_tail, error_message, created_at
                     FROM gate_runs WHERE session_id = ? AND plan_record_id = ? AND association_status = 'BOUND'".to_string();

    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(session_id.to_string()), Box::new(plan_record_id)];
    if let Some(t) = task_id {
        query.push_str(" AND task_id = ?");
        params_vec.push(Box::new(t.to_string()));
    }
    query.push_str(" ORDER BY id ASC");

    let mut stmt = conn.prepare(&query)?;
    let slice: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(&slice[..], |row| {
        let dur: i64 = row.get(7)?;
        Ok(GateRunRow {
            session_id: session_id.to_string(),
            plan_record_id,
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

/// Record for an attempt in `attempts` table (docs/08 schema.md §3).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AttemptRow {
    pub id: String,
    pub main_session_id: String,
    pub task_id: Option<i64>,
    pub role: String,
    pub provider_name: String,
    pub model_id: String,
    pub cli_window_identity: Option<String>,
    pub reused_from_id: Option<String>,
    pub predecessor_id: Option<String>,
    pub exec_mode: String,
    #[serde(default)]
    pub debug_output: Option<String>,
    #[serde(default)]
    pub requested_provider: Option<String>,
    #[serde(default)]
    pub requested_model: Option<String>,
    pub workdir: String,
    pub status: String,
    pub baseline_tokens: i64,
    pub context_tokens: i64,
    pub token_source: String,
    pub turn_count: u32,
    pub last_turn_ended_at: Option<String>,
    pub touched_files_json: String,
    pub created_at: Option<String>,
    pub ended_at: Option<String>,
}

/// Inserts or replaces an attempt into `attempts` table.
pub fn upsert_attempt(conn: &Connection, att: &AttemptRow) -> rusqlite::Result<()> {
    // Empty session/task fields are the status-only end-of-attempt update protocol.
    if let Some((session, task)) = conn.query_row(
        "SELECT main_session_id, task_id FROM attempts WHERE id=?1", [&att.id],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?))).optional()? {
        if (!att.main_session_id.is_empty() && att.main_session_id != session)
            || (att.task_id.is_some() && att.task_id != task) {
            return Err(rusqlite::Error::InvalidParameterName("Attempt execution identity cannot change".into()));
        }
    }
    conn.execute(
        "INSERT INTO attempts (
            id, main_session_id, task_id, role, provider_name, model_id,
            cli_window_identity, reused_from_id, predecessor_id, exec_mode,
            workdir, status, baseline_tokens, context_tokens, token_source,
            turn_count, last_turn_ended_at, touched_files_json, ended_at, debug_output, requested_provider, requested_model
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)
         ON CONFLICT(id) DO UPDATE SET
            status = excluded.status,
            turn_count = excluded.turn_count,
            last_turn_ended_at = excluded.last_turn_ended_at,
            touched_files_json = excluded.touched_files_json,
            ended_at = excluded.ended_at
         WHERE attempts.ended_at IS NULL AND attempts.status NOT IN ('DONE','FAILED','CANCELLED','TIMEOUT')",
        params![
            att.id,
            att.main_session_id,
            att.task_id,
            att.role,
            att.provider_name,
            att.model_id,
            att.cli_window_identity,
            att.reused_from_id,
            att.predecessor_id,
            att.exec_mode,
            att.workdir,
            att.status,
            att.baseline_tokens,
            att.context_tokens,
            att.token_source,
            att.turn_count as i64,
            att.last_turn_ended_at,
            att.touched_files_json,
            att.ended_at,
            att.debug_output,
            att.requested_provider,
            att.requested_model,
        ],
    )?;
    Ok(())
}

/// Lists all attempts for a main session.
pub fn list_attempts_for_session(conn: &Connection, session_id: &str) -> rusqlite::Result<Vec<AttemptRow>> {
    let mut result = Vec::new();
    if session_id.is_empty() {
        let mut stmt = conn.prepare(
            "SELECT id, main_session_id, task_id, role, provider_name, model_id,
                    cli_window_identity, reused_from_id, predecessor_id, exec_mode,
                    workdir, status, baseline_tokens, context_tokens, token_source,
                    turn_count, last_turn_ended_at, touched_files_json, created_at, ended_at, debug_output, requested_provider, requested_model
             FROM attempts
             ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            let tc: i64 = row.get(15)?;
            Ok(AttemptRow {
                id: row.get(0)?,
                main_session_id: row.get(1)?,
                task_id: row.get(2)?,
                role: row.get(3)?,
                provider_name: row.get(4)?,
                model_id: row.get(5)?,
                cli_window_identity: row.get(6)?,
                reused_from_id: row.get(7)?,
                predecessor_id: row.get(8)?,
                exec_mode: row.get(9)?,
                workdir: row.get(10)?,
                status: row.get(11)?,
                baseline_tokens: row.get(12)?,
                context_tokens: row.get(13)?,
                token_source: row.get(14)?,
                turn_count: tc as u32,
                last_turn_ended_at: row.get(16)?,
                touched_files_json: row.get(17)?,
                created_at: Some(row.get(18)?),
                ended_at: row.get(19)?,
                debug_output: row.get(20)?,
                requested_provider: row.get(21)?,
                requested_model: row.get(22)?,
            })
        })?;
        for r in rows {
            result.push(r?);
        }
    } else {
        let mut stmt = conn.prepare(
            "SELECT id, main_session_id, task_id, role, provider_name, model_id,
                    cli_window_identity, reused_from_id, predecessor_id, exec_mode,
                    workdir, status, baseline_tokens, context_tokens, token_source,
                    turn_count, last_turn_ended_at, touched_files_json, created_at, ended_at, debug_output, requested_provider, requested_model
             FROM attempts
             WHERE main_session_id = ?1
             ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            let tc: i64 = row.get(15)?;
            Ok(AttemptRow {
                id: row.get(0)?,
                main_session_id: row.get(1)?,
                task_id: row.get(2)?,
                role: row.get(3)?,
                provider_name: row.get(4)?,
                model_id: row.get(5)?,
                cli_window_identity: row.get(6)?,
                reused_from_id: row.get(7)?,
                predecessor_id: row.get(8)?,
                exec_mode: row.get(9)?,
                workdir: row.get(10)?,
                status: row.get(11)?,
                baseline_tokens: row.get(12)?,
                context_tokens: row.get(13)?,
                token_source: row.get(14)?,
                turn_count: tc as u32,
                last_turn_ended_at: row.get(16)?,
                touched_files_json: row.get(17)?,
                created_at: Some(row.get(18)?),
                ended_at: row.get(19)?,
                debug_output: row.get(20)?,
                requested_provider: row.get(21)?,
                requested_model: row.get(22)?,
            })
        })?;
        for r in rows {
            result.push(r?);
        }
    }
    Ok(result)
}

/// Lists all attempts for all sessions under a project.
pub fn list_attempts_for_project(conn: &Connection, project_id: &str) -> rusqlite::Result<Vec<AttemptRow>> {
    let mut stmt = conn.prepare(
        "SELECT a.id, a.main_session_id, a.task_id, a.role, a.provider_name, a.model_id,
                a.cli_window_identity, a.reused_from_id, a.predecessor_id, a.exec_mode,
                a.workdir, a.status, a.baseline_tokens, a.context_tokens, a.token_source,
                a.turn_count, a.last_turn_ended_at, a.touched_files_json, a.created_at, a.ended_at, a.debug_output, a.requested_provider, a.requested_model
         FROM attempts a
         JOIN main_sessions s ON a.main_session_id = s.id
         WHERE s.project_id = ?1
         ORDER BY a.created_at DESC",
    )?;
    let rows = stmt.query_map(params![project_id], |row| {
        let tc: i64 = row.get(15)?;
        Ok(AttemptRow {
            id: row.get(0)?,
            main_session_id: row.get(1)?,
            task_id: row.get(2)?,
            role: row.get(3)?,
            provider_name: row.get(4)?,
            model_id: row.get(5)?,
            cli_window_identity: row.get(6)?,
            reused_from_id: row.get(7)?,
            predecessor_id: row.get(8)?,
            exec_mode: row.get(9)?,
            workdir: row.get(10)?,
            status: row.get(11)?,
            baseline_tokens: row.get(12)?,
            context_tokens: row.get(13)?,
            token_source: row.get(14)?,
            turn_count: tc as u32,
            last_turn_ended_at: row.get(16)?,
            touched_files_json: row.get(17)?,
            created_at: Some(row.get(18)?),
            ended_at: row.get(19)?,
            debug_output: row.get(20)?,
            requested_provider: row.get(21)?,
            requested_model: row.get(22)?,
        })
    })?;
    let mut result = Vec::new();
    for r in rows {
        result.push(r?);
    }
    Ok(result)
}

/// Inserts a plan record into `plans` table and returns plan ID.
pub fn insert_plan_record(
    conn: &Connection,
    main_session_id: &str,
    version: u32,
    goal: &str,
    spec_json: &str,
    est_cost_usd: f64,
    status: &str,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO plans (main_session_id, version, goal, spec_json, est_cost_usd, status, approved_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        params![main_session_id, version as i64, goal, spec_json, est_cost_usd, status],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Find existing plan by session + version (F12.c idempotency).
pub fn find_plan_id_by_session_version(
    conn: &Connection,
    main_session_id: &str,
    version: u32,
) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT id FROM plans WHERE main_session_id = ?1 AND version = ?2 LIMIT 1",
        params![main_session_id, version as i64],
        |r| r.get(0),
    )
    .optional()
}

/// Map task domain → tasks.role (F12.c / F13.e).
pub fn role_from_domain(domain: &str) -> &'static str {
    match domain {
        "testing" => "TESTER",
        "review" => "REVIEWER",
        _ => "EXECUTOR",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RecordPlanApprovedResult {
    pub plan_id: i64,
    /// Task code → tasks.id
    pub task_ids: std::collections::HashMap<String, i64>,
}

/// Input assignment slice for plan approval (avoids coupling to matching crate).
#[derive(Debug, Clone)]
pub struct PlanApprovedAssignment<'a> {
    pub task_id: &'a str,
    pub est_cost_usd: f64,
}

/// Idempotent write of plans / contracts / tasks / edges + PLANNING usage backfill (F12.c).
pub fn record_plan_approved(
    conn: &Connection,
    main_session_id: &str,
    plan: &tokage_core::planning::Plan,
    assignments: &[PlanApprovedAssignment<'_>],
    est_cost_usd: f64,
) -> rusqlite::Result<RecordPlanApprovedResult> {
    // Main session must exist (FK).
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM main_sessions WHERE id = ?1)",
        params![main_session_id],
        |r| r.get(0),
    )?;
    if !exists {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }

    if let Some(existing) = find_plan_id_by_session_version(conn, main_session_id, plan.version)? {
        let mut task_ids = std::collections::HashMap::new();
        let mut stmt = conn.prepare("SELECT id, code FROM tasks WHERE plan_id = ?1")?;
        let rows = stmt.query_map(params![existing], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (id, code) = row?;
            task_ids.insert(code, id);
        }
        return Ok(RecordPlanApprovedResult {
            plan_id: existing,
            task_ids,
        });
    }

    let spec_json = serde_json::to_string(&plan.spec).unwrap_or_else(|_| "{}".into());
    let plan_id = insert_plan_record(
        conn,
        main_session_id,
        plan.version,
        &plan.spec.goal,
        &spec_json,
        est_cost_usd,
        "RUNNING",
    )?;

    for c in &plan.contracts {
        conn.execute(
            "INSERT INTO plan_contracts (plan_id, code, title, content, token_count)
             VALUES (?1, ?2, ?3, ?4, 0)",
            params![plan_id, c.id, c.title, c.content],
        )?;
    }

    let mut task_ids = std::collections::HashMap::new();
    for t in &plan.tasks {
        let id = ensure_execution_task(conn, main_session_id, plan_id, t,
            assignments.iter().find(|a| a.task_id == t.id).map(|a| a.est_cost_usd))?;
        task_ids.insert(t.id.clone(), id);
    }

    for t in &plan.tasks {
        for dep in &t.depends_on {
            conn.execute(
                "INSERT OR IGNORE INTO task_edges (plan_id, from_task, to_task) VALUES (?1, ?2, ?3)",
                params![plan_id, dep, t.id],
            )?;
        }
    }

    // Backfill PLANNING usage rows for this session (via attempts.main_session_id).
    conn.execute(
        "UPDATE usage_events
         SET plan_id = ?1
         WHERE plan_id IS NULL
           AND phase = 'PLANNING'
           AND attempt_id IN (
             SELECT id FROM attempts WHERE main_session_id = ?2
           )",
        params![plan_id, main_session_id],
    )?;

    Ok(RecordPlanApprovedResult { plan_id, task_ids })
}

/// Register dynamically injected tasks under the existing plan, never a local code alone.
pub fn ensure_execution_task(
    conn: &Connection,
    session_id: &str,
    plan_id: i64,
    t: &tokage_core::planning::TaskItem,
    est_cost_usd: Option<f64>,
) -> rusqlite::Result<i64> {
    let owned: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1 AND main_session_id=?2)",
        params![plan_id, session_id],
        |r| r.get(0),
    )?;
    if !owned {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    if let Some(id) = conn
        .query_row(
            "SELECT id FROM tasks WHERE plan_id=?1 AND code=?2",
            params![plan_id, t.id],
            |r| r.get(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    let state = if t.depends_on.is_empty() {
        "READY"
    } else {
        "PENDING"
    };
    let role = role_from_domain(&t.domain);
    let est = est_cost_usd;
    let size = match t.size {
        tokage_core::planning::TaskSize::S => "S",
        tokage_core::planning::TaskSize::M => "M",
        tokage_core::planning::TaskSize::L => "L",
    };
    let contracts = serde_json::to_string(&t.contracts).unwrap_or_else(|_| "[]".into());
    let write_scope = serde_json::to_string(&t.write_scope).unwrap_or_else(|_| "[]".into());
    let deliverables = serde_json::to_string(&t.deliverables).unwrap_or_else(|_| "[]".into());
    let acceptance = serde_json::to_string(&t.acceptance).unwrap_or_else(|_| "{}".into());
    conn.execute(
        "INSERT INTO tasks (
                plan_id, code, domain, title, goal, contracts, write_scope, deliverables,
                acceptance, size, difficulty, role, state, est_cost_usd
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            plan_id,
            t.id,
            t.domain,
            t.title,
            t.goal,
            contracts,
            write_scope,
            deliverables,
            acceptance,
            size,
            t.difficulty as i64,
            role,
            state,
            est,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Retrieves a plan by ID.
pub fn get_plan_record(
    conn: &Connection,
    plan_id: i64,
) -> rusqlite::Result<Option<(i64, String, u32, String, String, f64, String)>> {
    let mut stmt = conn.prepare(
        "SELECT id, main_session_id, version, goal, spec_json, est_cost_usd, status
         FROM plans WHERE id = ?1",
    )?;
    let mut rows = stmt.query(params![plan_id])?;
    if let Some(r) = rows.next()? {
        let id: i64 = r.get(0)?;
        let session_id: String = r.get(1)?;
        let ver: i64 = r.get(2)?;
        let goal: String = r.get(3)?;
        let spec: String = r.get(4)?;
        let est: f64 = r.get(5)?;
        let status: String = r.get(6)?;
        Ok(Some((id, session_id, ver as u32, goal, spec, est, status)))
    } else {
        Ok(None)
    }
}

/// Updates plan status (DRAFT | APPROVED | RUNNING | PAUSED | COMPLETED | SUPERSEDED | ABANDONED).
pub fn update_plan_status(conn: &Connection, plan_id: i64, status: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE plans SET status = ?1 WHERE id = ?2",
        params![status, plan_id],
    )?;
    Ok(())
}

/// Updates a task's actual cost and adds to total plan actual cost.
pub fn update_task_actual_cost(conn: &Connection, plan_id: i64, task_code: &str, actual_cost_usd: f64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE tasks SET actual_cost_usd = ?1 WHERE plan_id = ?2 AND code = ?3",
        params![actual_cost_usd, plan_id, task_code],
    )?;
    Ok(())
}

/// Updates status of a main session (PLANNING | REVIEW | RUNNING | PAUSED | BLOCKED | IDLE | COMPLETED).
pub fn update_main_session_status(conn: &Connection, session_id: &str, status: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE main_sessions SET status = ?1, updated_at = (strftime('%Y-%m-%dT%H:%M:%fZ','now')) WHERE id = ?2",
        params![status, session_id],
    )?;
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use tokage_core::planning::{Plan, PlanContract, PlanSpec, TaskItem, TaskSize};

    fn in_memory_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::embedded::migrations::runner().run(&mut conn).unwrap();
        conn
    }

    fn seed_session(conn: &Connection, sid: &str) {
        conn.execute(
            "INSERT OR IGNORE INTO projects (id, name, local_path) VALUES ('p1', 'P', '/tmp/f12')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO main_sessions (id, project_id, title) VALUES (?1, 'p1', 'S')",
            params![sid],
        )
        .unwrap();
    }

    fn sample_plan() -> Plan {
        Plan {
            version: 1,
            parent_plan_id: None,
            spec: PlanSpec {
                raw: "".into(),
                goal: "Ship F12".into(),
                in_scope: vec![],
                out_of_scope: vec![],
                decisions: vec![],
            },
            contracts: vec![PlanContract {
                id: "C1".into(),
                title: "API".into(),
                content: "stable".into(),
            }],
            tasks: vec![
                TaskItem {
                    id: "T1".into(),
                    domain: "backend".into(),
                    title: "Impl".into(),
                    goal: "code".into(),
                    depends_on: vec![],
                    contracts: vec!["C1".into()],
                    write_scope: vec!["src/**".into()],
                    deliverables: vec![],
                    acceptance: Default::default(),
                    size: TaskSize::S,
                    difficulty: 1,
                    assigned_model: None,
                    model_params: None,
                },
                TaskItem {
                    id: "T2".into(),
                    domain: "testing".into(),
                    title: "Test".into(),
                    goal: "test".into(),
                    depends_on: vec!["T1".into()],
                    contracts: vec![],
                    write_scope: vec!["tests/**".into()],
                    deliverables: vec![],
                    acceptance: Default::default(),
                    size: TaskSize::M,
                    difficulty: 2,
                    assigned_model: None,
                    model_params: None,
                },
            ],
            notes: None,
            raw_markdown: "".into(),
            is_user_edited: None,
        }
    }

    #[test]
    fn m2e_terminal_attempt_cannot_regress_from_late_usage_stub_or_duplicate_error() {
        let conn = in_memory_db(); seed_session(&conn, "m2e-attempt");
        let mut attempt: AttemptRow = serde_json::from_value(serde_json::json!({
            "id":"turn", "mainSessionId":"m2e-attempt", "role":"EXECUTOR", "providerName":"CLAUDE_CODE",
            "modelId":"fixture", "execMode":"HEADLESS", "workdir":"/repo", "status":"RUNNING",
            "baselineTokens":0, "contextTokens":0, "tokenSource":"ESTIMATED", "turnCount":1,
            "touchedFilesJson":"[]"
        })).unwrap();
        upsert_attempt(&conn, &attempt).unwrap();
        attempt.status = "DONE".into(); attempt.ended_at = Some("2026-10-19T00:00:00Z".into());
        upsert_attempt(&conn, &attempt).unwrap();
        attempt.status = "RUNNING".into(); attempt.ended_at = None;
        upsert_attempt(&conn, &attempt).unwrap();
        attempt.status = "FAILED".into(); attempt.ended_at = Some("later".into());
        upsert_attempt(&conn, &attempt).unwrap();
        let stored: (String,String) = conn.query_row("SELECT status,ended_at FROM attempts WHERE id='turn'", [], |row| Ok((row.get(0)?,row.get(1)?))).unwrap();
        assert_eq!(stored, ("DONE".into(), "2026-10-19T00:00:00Z".into()));
    }

    #[test]
    fn f12a_record_plan_approved_writes_and_is_idempotent() {
        let conn = in_memory_db();
        seed_session(&conn, "sess-f12");
        let plan = sample_plan();
        let assigns = [
            PlanApprovedAssignment {
                task_id: "T1",
                est_cost_usd: 0.1,
            },
            PlanApprovedAssignment {
                task_id: "T2",
                est_cost_usd: 0.05,
            },
        ];
        let r1 = record_plan_approved(&conn, "sess-f12", &plan, &assigns, 0.15).unwrap();
        assert!(r1.plan_id > 0);
        assert_eq!(r1.task_ids.len(), 2);
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM plan_contracts WHERE plan_id = ?1", [r1.plan_id], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM tasks WHERE plan_id = ?1", [r1.plan_id], |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM task_edges WHERE plan_id = ?1", [r1.plan_id], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        let role: String = conn
            .query_row(
                "SELECT role FROM tasks WHERE plan_id = ?1 AND code = 'T2'",
                [r1.plan_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(role, "TESTER");
        let state: String = conn
            .query_row(
                "SELECT state FROM tasks WHERE plan_id = ?1 AND code = 'T1'",
                [r1.plan_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(state, "READY");

        let r2 = record_plan_approved(&conn, "sess-f12", &plan, &assigns, 0.15).unwrap();
        assert_eq!(r1.plan_id, r2.plan_id);
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM plans WHERE main_session_id = 'sess-f12'", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn f12a_backfills_planning_usage_via_attempts() {
        let conn = in_memory_db();
        seed_session(&conn, "sess-plan");
        conn.execute(
            "INSERT INTO attempts (id, main_session_id, role, provider_name, model_id, workdir)
             VALUES ('att-plan', 'sess-plan', 'PLANNER', 'CLAUDE_CODE', 'sonnet', '/tmp')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO usage_events (attempt_id, provider_name, model_id, phase, source, input_tokens, output_tokens)
             VALUES ('att-plan', 'CLAUDE_CODE', 'sonnet', 'PLANNING', 'ESTIMATED', 10, 5)",
            [],
        )
        .unwrap();
        let plan = sample_plan();
        let r = record_plan_approved(&conn, "sess-plan", &plan, &[], 0.0).unwrap();
        let pid: Option<i64> = conn
            .query_row(
                "SELECT plan_id FROM usage_events WHERE attempt_id = 'att-plan'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(pid, Some(r.plan_id));
    }

    #[test]
    fn test_report_and_brief_roundtrip() {
        let conn = in_memory_db();

        seed_session(&conn, "report-session");
        let plan = record_plan_approved(&conn, "report-session", &sample_plan(), &[], 0.0).unwrap();
        conn.execute("INSERT INTO attempts (id, main_session_id, task_id, role, provider_name, model_id, workdir) VALUES ('att-1', 'report-session', ?1, 'EXECUTOR', 'CLAUDE_CODE', 'sonnet', '/tmp')", [plan.task_ids["T1"]]).unwrap();
        let brief = BriefRow {
            session_id: "report-session".into(),
            plan_record_id: plan.plan_id,
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

        let loaded_brief = get_latest_brief(&conn, "report-session", plan.plan_id, "T1").unwrap().expect("brief found");
        assert_eq!(loaded_brief.token_count, 240);
        assert_eq!(loaded_brief.content, brief.content);

        let report = TaskReportRow {
            session_id: "report-session".into(),
            plan_record_id: plan.plan_id,
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

        let reports = get_task_reports(&conn, "report-session", plan.plan_id, "T1").unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].model_id, "claude-3-7-sonnet");
        assert_eq!(reports[0].first_pass, true);
        assert_eq!(reports[0].changed_files_list, vec!["schema.rs".to_string()]);
    }
    fn scoped_report(session: &str, plan: i64, attempt: &str) -> TaskReportRow {
        serde_json::from_value(serde_json::json!({
            "sessionId": session, "planRecordId": plan, "taskId": "T1", "modelId": "model",
            "attemptId": attempt, "firstPass": true, "isLocallyGenerated": false,
            "summaryMd": attempt, "fullMd": format!("full-{attempt}"),
            "changedFilesStat": "1 file", "changedFilesList": [format!("{attempt}.rs")],
            "gateResults": "passed", "costUsd": 0.25
        }))
        .unwrap()
    }

    fn seed_attempt(conn: &Connection, session: &str, task: i64, attempt: &str) {
        conn.execute(
            "INSERT INTO attempts (id,main_session_id,task_id,role,provider_name,model_id,workdir)
          VALUES (?1,?2,?3,'EXECUTOR','CLAUDE_CODE','model','/tmp')",
            params![attempt, session, task],
        )
        .unwrap();
    }

    #[test]
    fn m1b_reports_briefs_and_gates_isolate_projects_sessions_and_versions() {
        let conn = in_memory_db();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        seed_session(&conn, "s1");
        seed_session(&conn, "s2");
        conn.execute_batch(
            "INSERT INTO projects(id,name,local_path) VALUES ('p2','other','/tmp/other');
          INSERT INTO main_sessions(id,project_id,title) VALUES ('s3','p2','other');",
        )
        .unwrap();
        let mut scopes = Vec::new();
        for session in ["s1", "s2", "s3"] {
            for version in [1, 2] {
                let mut plan = sample_plan();
                plan.version = version;
                let ids = record_plan_approved(&conn, session, &plan, &[], 0.0).unwrap();
                let attempt = format!("{session}-v{version}");
                seed_attempt(&conn, session, ids.task_ids["T1"], &attempt);
                let report = scoped_report(session, ids.plan_id, &attempt);
                insert_task_report(&conn, &report).unwrap();
                insert_brief(
                    &conn,
                    &BriefRow {
                        session_id: session.into(),
                        plan_record_id: ids.plan_id,
                        id: None,
                        task_id: "T1".into(),
                        kind: "TASK_BRIEF".into(),
                        content: attempt.clone(),
                        token_count: 1,
                        edited_by_user: false,
                        created_at: None,
                    },
                )
                .unwrap();
                insert_gate_run(
                    &conn,
                    &GateRunRow {
                        session_id: session.into(),
                        plan_record_id: ids.plan_id,
                        id: None,
                        task_id: Some("T1".into()),
                        plan_id: ids.plan_id.to_string(),
                        attempt_id: Some(attempt.clone()),
                        command: attempt.clone(),
                        exit_code: Some(0),
                        passed: true,
                        duration_ms: 1,
                        output_tail: attempt.clone(),
                        error_message: None,
                        created_at: None,
                    },
                )
                .unwrap();
                scopes.push((session, ids.plan_id, attempt));
            }
        }
        for (session, plan, attempt) in scopes {
            let rows = get_task_reports(&conn, session, plan, "T1").unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].summary_md, attempt);
            assert_eq!(rows[0].changed_files_list, vec![format!("{attempt}.rs")]);
            assert_eq!(rows[0].cost_usd, Some(0.25));
            assert_eq!(
                get_latest_brief(&conn, session, plan, "T1")
                    .unwrap()
                    .unwrap()
                    .content,
                attempt
            );
            assert_eq!(
                list_gate_runs(&conn, session, plan, Some("T1")).unwrap()[0].output_tail,
                attempt
            );
            assert!(get_task_reports(&conn, "unknown-session", plan, "T1")
                .unwrap()
                .is_empty());
            assert!(list_gate_runs(&conn, "unknown-session", plan, None)
                .unwrap()
                .is_empty());
        }
    }

    #[test]
    fn m1b_rejects_mismatched_attempts_and_registers_injected_tasks() {
        let conn = in_memory_db();
        seed_session(&conn, "owner");
        seed_session(&conn, "other");
        let a = record_plan_approved(&conn, "owner", &sample_plan(), &[], 0.0).unwrap();
        let b = record_plan_approved(&conn, "other", &sample_plan(), &[], 0.0).unwrap();
        seed_attempt(&conn, "owner", a.task_ids["T1"], "a1");
        seed_attempt(&conn, "owner", a.task_ids["T2"], "a2");
        seed_attempt(&conn, "other", b.task_ids["T1"], "b1");
        for attempt in ["missing", "a2", "b1"] {
            assert!(
                insert_task_report(&conn, &scoped_report("owner", a.plan_id, attempt)).is_err()
            );
        }
        assert!(insert_task_report(&conn, &scoped_report("other", a.plan_id, "a1")).is_err());
        assert!(insert_task_report(&conn, &scoped_report("owner", b.plan_id, "a1")).is_err());
        let gate: GateRunRow = serde_json::from_value(serde_json::json!({
          "sessionId":"owner", "planRecordId":a.plan_id, "taskId":"T1", "planId":a.plan_id.to_string(),
          "attemptId":"a1", "command":"test", "passed":true, "durationMs":1, "outputTail":"ok"
        })).unwrap();
        insert_gate_run(&conn, &gate).unwrap();
        for attempt in ["missing", "a2", "b1"] {
            let mut bad = gate.clone();
            bad.attempt_id = Some(attempt.into());
            assert!(insert_gate_run(&conn, &bad).is_err());
        }
        let mut bad = gate.clone();
        bad.session_id = "other".into();
        assert!(insert_gate_run(&conn, &bad).is_err());
        let mut bad = gate.clone();
        bad.plan_id = b.plan_id.to_string();
        assert!(insert_gate_run(&conn, &bad).is_err());
        let brief = BriefRow {
            session_id: "other".into(),
            plan_record_id: a.plan_id,
            id: None,
            task_id: "T1".into(),
            kind: "TASK_BRIEF".into(),
            content: "wrong".into(),
            token_count: 1,
            edited_by_user: false,
            created_at: None,
        };
        assert!(insert_brief(&conn, &brief).is_err());
        let mut v2 = sample_plan();
        v2.version = 2;
        let v2 = record_plan_approved(&conn, "owner", &v2, &[], 0.0).unwrap();
        assert!(insert_task_report(&conn, &scoped_report("owner", v2.plan_id, "a1")).is_err());
        let mut injected = sample_plan().tasks[0].clone();
        injected.id = "T1-conflict".into();
        assert!(ensure_execution_task(&conn, "other", a.plan_id, &injected, None).is_err());
        let id = ensure_execution_task(&conn, "owner", a.plan_id, &injected, None).unwrap();
        assert_eq!(
            ensure_execution_task(&conn, "owner", a.plan_id, &injected, None).unwrap(),
            id
        );
        seed_attempt(&conn, "owner", id, "fix");
        let mut report = scoped_report("owner", a.plan_id, "fix");
        report.task_id = injected.id;
        insert_task_report(&conn, &report).unwrap();
        assert!(conn.execute("INSERT INTO attempts(id,main_session_id,task_id,role,provider_name,model_id,workdir)
          VALUES ('wrong','other',?1,'EXECUTOR','CLAUDE_CODE','model','/tmp')",[id]).is_err());
        assert!(conn
            .execute(
                "UPDATE attempts SET task_id=?1 WHERE id='a1'",
                [a.task_ids["T2"]]
            )
            .is_err());
        assert_eq!(
            get_task_reports(&conn, "owner", a.plan_id, "T1")
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn m1b_v11_migration_preserves_unknown_history_without_guessing() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::embedded::migrations::runner()
            .set_target(refinery::Target::Version(11))
            .run(&mut conn)
            .unwrap();
        seed_session(&conn, "old");
        seed_session(&conn, "unrelated");
        let ids = record_plan_approved(&conn, "old", &sample_plan(), &[], 0.0).unwrap();
        record_plan_approved(&conn, "unrelated", &sample_plan(), &[], 0.0).unwrap();
        seed_attempt(&conn, "old", ids.task_ids["T1"], "known");
        // Even a single matching task code is not proof. Null/mismatched attempt bindings stay unknown.
        conn.execute(
            "INSERT INTO attempts(id,main_session_id,role,provider_name,model_id,workdir)
          VALUES ('unbound','old','EXECUTOR','CLAUDE_CODE','model','/tmp')",
            [],
        )
        .unwrap();
        for (attempt, code) in [
            ("known", "T1"),
            ("unbound", "T1"),
            ("missing", "T1"),
            ("known", "T2"),
        ] {
            conn.execute(
                "INSERT INTO task_reports(task_id,model_id,attempt_id,summary_md,full_md)
              VALUES (?1,'model',?2,?2,?2)",
                params![code, attempt],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO briefs(task_id,kind,content) VALUES ('T1','TASK_BRIEF','legacy brief')",
            [],
        )
        .unwrap();
        for (plan, attempt) in [
            (ids.plan_id.to_string(), "known"),
            ("old".into(), "known"),
            (ids.plan_id.to_string(), "missing"),
        ] {
            conn.execute("INSERT INTO gate_runs(task_id,plan_id,attempt_id,command,passed) VALUES ('T1',?1,?2,?2,1)",params![plan,attempt]).unwrap();
        }
        crate::run_migrations(&mut conn).unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM task_reports", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            4
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM task_reports WHERE association_status='LEGACY_UNASSOCIATED'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            3
        );
        assert_eq!(
            get_task_reports(&conn, "old", ids.plan_id, "T1")
                .unwrap()
                .len(),
            1
        );
        assert!(get_latest_brief(&conn, "old", ids.plan_id, "T1")
            .unwrap()
            .is_none());
        assert_eq!(
            conn.query_row(
                "SELECT content FROM briefs WHERE association_status='LEGACY_UNASSOCIATED'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "legacy brief"
        );
        assert_eq!(
            list_gate_runs(&conn, "old", ids.plan_id, None)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM gate_runs", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            3
        );
        assert!(conn.execute("INSERT INTO task_reports(task_id,model_id,attempt_id,summary_md,full_md) VALUES ('T1','m','known','bad','bad')",[]).is_err());
    }

    #[test]
    fn m1b_concurrent_writers_and_reopen_keep_exact_scope() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("artifacts.db");
        let mut conn = Connection::open(&path).unwrap();
        crate::run_migrations(&mut conn).unwrap();
        conn.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
        let mut scopes = Vec::new();
        for session in ["parallel-a", "parallel-b"] {
            seed_session(&conn, session);
            let ids = record_plan_approved(&conn, session, &sample_plan(), &[], 0.0).unwrap();
            seed_attempt(&conn, session, ids.task_ids["T1"], session);
            scopes.push((session, ids.plan_id));
        }
        drop(conn);
        std::thread::scope(|scope| {
            for &(session, plan) in &scopes {
                let path = &path;
                scope.spawn(move || {
                    let conn = Connection::open(path).unwrap();
                    conn.busy_timeout(std::time::Duration::from_secs(5))
                        .unwrap();
                    for _ in 0..20 {
                        insert_task_report(&conn, &scoped_report(session, plan, session)).unwrap();
                    }
                });
            }
        });
        let reopened = Connection::open(path).unwrap();
        for (session, plan) in scopes {
            let rows = get_task_reports(&reopened, session, plan, "T1").unwrap();
            assert_eq!(rows.len(), 20);
            assert!(rows
                .iter()
                .all(|r| r.attempt_id == session && r.summary_md == session));
        }
    }
}
