//! Row mapping between the frontend's stores and the tables (docs/08 schema.md).
//!
//! The frontend sends changes as a `WriteBatch` of upserts and deletes. Each item becomes its
//! own job (savepoint), so one bad row doesn't drop the rest. Upserts never use `REPLACE`:
//! that deletes the row first and would cascade to its children.

use std::collections::HashMap;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::writer::Job;

/// Set once the localStorage data has been imported (P1.m3d).
pub const LEGACY_IMPORTED: &str = "meta.legacyImported";

/// Epoch milliseconds ↔ the ISO 8601 text the tables store.
const MS_TO_ISO: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', ?{} / 1000.0, 'unixepoch')";
const ISO_TO_MS: &str = "CAST(ROUND((julianday({}) - 2440587.5) * 86400000) AS INTEGER)";

fn ms_to_iso(param: usize) -> String {
    MS_TO_ISO.replace("{}", &param.to_string())
}
fn iso_to_ms(col: &str) -> String {
    ISO_TO_MS.replace("{}", col)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRow {
    pub id: String,
    pub name: String,
    pub path: String,
    pub position: i64,
    pub expanded: bool,
    pub trusted: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRow {
    pub id: String,
    pub project_id: String,
    pub title: String,
    /// Frontend state: draft / review / running / done / blocked.
    pub state: String,
    pub position: i64,
    #[serde(default)]
    pub archived: bool,
    pub forked_from: Option<String>,
    pub worktree: Option<Value>,
    /// Epoch ms.
    pub updated_at: f64,
}

/// The parts of a session the chat store owns.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionChat {
    pub session_id: String,
    pub thread: Option<Value>,
    #[serde(default)]
    pub queue: Value,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageRow {
    pub session_id: String,
    /// Index in the conversation.
    pub seq: i64,
    /// Which user turn the message belongs to, from 0.
    pub turn_index: i64,
    /// The frontend's `ChatMessage`; `text` goes to `content`, the rest to `meta_json`.
    pub message: Value,
    /// Model of the turn (the assistant reply's for a prompt); picks the attempt.
    pub provider: Option<String>,
    pub model: Option<String>,
    /// The turn's mode, `plan` or `direct`.
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SettingRow {
    pub key: String,
    pub value: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomPackRow {
    pub url: String,
    /// Epoch ms.
    pub loaded_at: f64,
    pub pack: Value,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageEventRow {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    #[serde(default)]
    pub attempt_id: Option<String>,
    #[serde(default)]
    pub task_id: Option<i64>,
    #[serde(default)]
    pub plan_id: Option<i64>,
    pub provider_name: String,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub turn_index: Option<i64>,
    #[serde(default)]
    pub input_tokens: i64,
    #[serde(default)]
    pub cache_read_tokens: i64,
    #[serde(default)]
    pub cache_write_tokens: i64,
    #[serde(default)]
    pub output_tokens: i64,
    #[serde(default)]
    pub api_calls: Option<i64>,
    #[serde(default)]
    pub context_tokens: Option<i64>,
    #[serde(default)]
    pub gap_seconds: Option<i64>,
    #[serde(default)]
    pub cost_usd: Option<f64>,
    #[serde(default)]
    pub equiv_cost_usd: Option<f64>,
    #[serde(default)]
    pub credits: Option<f64>,
    pub phase: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WriteBatch {
    pub projects: Vec<ProjectRow>,
    pub sessions: Vec<SessionRow>,
    pub session_chat: Vec<SessionChat>,
    pub messages: Vec<MessageRow>,
    pub usage_events: Vec<UsageEventRow>,
    pub settings: Vec<SettingRow>,
    /// Replaces the whole cached CLI list when set.
    pub providers: Option<Vec<Value>>,
    pub custom_pack: Option<CustomPackRow>,
    pub clear_custom_pack: bool,
    pub delete_sessions: Vec<String>,
    pub delete_projects: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionOut {
    id: String,
    title: String,
    state: &'static str,
    updated_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    worktree: Option<Value>,
    archived: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    forked_from: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectOut {
    id: String,
    name: String,
    path: String,
    expanded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    trusted: Option<bool>,
    sessions: Vec<SessionOut>,
}

#[derive(Debug, Serialize)]
pub struct ConversationOut {
    messages: Vec<Value>,
    queue: Value,
    thread: Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub projects: Vec<ProjectOut>,
    pub conversations: HashMap<String, ConversationOut>,
    pub settings: HashMap<String, Value>,
    pub providers: Vec<Value>,
    pub custom_pack: Option<CustomPackRow>,
}

fn state_to_db(state: &str) -> &'static str {
    match state {
        "review" => "REVIEW",
        "running" => "RUNNING",
        "done" => "COMPLETED",
        "blocked" => "BLOCKED",
        _ => "IDLE",
    }
}

fn state_from_db(status: &str) -> &'static str {
    match status {
        "REVIEW" => "review",
        "RUNNING" => "running",
        "COMPLETED" => "done",
        "BLOCKED" => "blocked",
        _ => "draft",
    }
}

fn json(v: &Value) -> String {
    v.to_string()
}

fn parse(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or(Value::Null)
}

fn upsert_project(c: &Connection, p: &ProjectRow) -> rusqlite::Result<()> {
    c.execute(
        "INSERT INTO projects (id, name, local_path, position, expanded, trusted)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(id) DO UPDATE SET name = ?2, local_path = ?3, position = ?4,
             expanded = ?5, trusted = ?6",
        params![p.id, p.name, p.path, p.position, p.expanded, p.trusted],
    )?;
    Ok(())
}

fn upsert_session(c: &Connection, s: &SessionRow) -> rusqlite::Result<()> {
    // A fork whose source was deleted keeps no link (the source row is gone).
    c.execute(
        &format!(
            "INSERT INTO main_sessions (id, project_id, title, status, position, archived,
                 forked_from, worktree_json, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6,
                 (SELECT id FROM main_sessions WHERE id = ?7), ?8, {upd})
             ON CONFLICT(id) DO UPDATE SET project_id = ?2, title = ?3, status = ?4,
                 position = ?5, archived = ?6,
                 forked_from = (SELECT id FROM main_sessions WHERE id = ?7),
                 worktree_json = ?8, updated_at = {upd}",
            upd = ms_to_iso(9)
        ),
        params![
            s.id,
            s.project_id,
            s.title,
            state_to_db(&s.state),
            s.position,
            s.archived,
            s.forked_from,
            s.worktree.as_ref().map(json),
            s.updated_at,
        ],
    )?;
    Ok(())
}

fn update_session_chat(c: &Connection, s: &SessionChat) -> rusqlite::Result<()> {
    let queue = if s.queue.is_null() { Value::Array(vec![]) } else { s.queue.clone() };
    c.execute(
        "UPDATE main_sessions SET thread_json = ?2, queue_json = ?3 WHERE id = ?1",
        params![s.session_id, s.thread.as_ref().filter(|t| !t.is_null()).map(json), json(&queue)],
    )?;
    // The thread is the CLI session of the attempt that ran the last turn.
    if let Some(t) = s.thread.as_ref().filter(|t| !t.is_null()) {
        let provider = t.pointer("/ref/provider").and_then(Value::as_str);
        let model = t.pointer("/ref/model").and_then(Value::as_str);
        let id = t.get("id").and_then(Value::as_str);
        if let (Some(p), Some(m), Some(id)) = (provider, model, id) {
            c.execute(
                "UPDATE attempts SET cli_window_identity = ?2 WHERE id = ?1",
                params![if let Some(encoded) = id.strip_prefix("tokage-debug:") {
                    let parts: Vec<String> = serde_json::from_str(encoded).unwrap_or_default();
                    if parts.len() == 4 {
                        let provider = if parts[0] == "mock" { "DEBUG_PROVIDER" } else { &parts[1] };
                        let model = if parts[0] == "mock" { "mock" } else { &parts[2] };
                        format!("{}|DEBUG_{}|{provider}|{model}", s.session_id, parts[0].to_ascii_uppercase())
                    } else { attempt_id(&s.session_id, p, m) }
                } else { attempt_id(&s.session_id, p, m) }, id],
            )?;
        }
    }
    Ok(())
}

/// One execution window per session and model: the CLI session a turn continues
/// while the model stays the same.
fn attempt_id(session: &str, provider: &str, model: &str) -> String {
    format!("{session}|{provider}|{model}")
}

fn upsert_message(c: &Connection, m: &MessageRow) -> rusqlite::Result<()> {
    let Value::Object(obj) = &m.message else {
        return Err(rusqlite::Error::InvalidParameterName("message must be an object".into()));
    };
    let get = |k: &str| obj.get(k).and_then(Value::as_str);
    let Some(id) = get("id") else {
        return Err(rusqlite::Error::InvalidParameterName("message without id".into()));
    };
    let role = if get("role") == Some("assistant") { "ASSISTANT" } else { "USER" };
    let provider = m.provider.as_deref().or(get("provider")).unwrap_or("UNKNOWN");
    let model = m.model.as_deref().unwrap_or("");
    let debug_output = m.message.pointer("/debugRoute/output").and_then(Value::as_str).filter(|s| matches!(*s, "mock" | "luna" | "deepseek"));
    let attempt = match debug_output {
        Some(output) => format!("{}|DEBUG_{}|{provider}|{model}", m.session_id, output.to_ascii_uppercase()),
        None => attempt_id(&m.session_id, provider, model),
    };

    let attempt_role = if m.mode.as_deref().or(get("mode")) == Some("plan") { "PLANNER" } else { "EXECUTOR" };
    c.execute(
        "INSERT INTO attempts (id, main_session_id, role, provider_name, model_id, workdir, debug_output, requested_provider, requested_model)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(id) DO NOTHING",
        params![attempt, m.session_id, attempt_role, provider, model, get("cwd").unwrap_or(""), debug_output, m.message.pointer("/debugRoute/requestedProvider").and_then(Value::as_str), m.message.pointer("/debugRoute/requestedModel").and_then(Value::as_str)],
    )?;

    let extracted_text: String;
    let raw_text = match get("text") {
        Some(s) if !s.is_empty() => s,
        _ => {
            // Fallback: extract text from parts array (e.g., parts with type "text" or "reasoning")
            if let Some(Value::Array(parts)) = obj.get("parts") {
                let mut pieces = Vec::new();
                for p in parts {
                    if let Value::Object(part_obj) = p {
                        if let Some(Value::String(txt)) = part_obj.get("text") {
                            if !txt.is_empty() {
                                pieces.push(txt.as_str());
                            }
                        }
                    }
                }
                extracted_text = pieces.join("\n\n");
                &extracted_text
            } else {
                ""
            }
        }
    };
    let (text, truncated) = if raw_text.len() > 65_536 {
        let head_bytes = 16 * 1024;
        let tail_bytes = 16 * 1024;
        let mut head_end = head_bytes;
        while head_end < raw_text.len() && !raw_text.is_char_boundary(head_end) {
            head_end += 1;
        }
        let mut tail_start = raw_text.len().saturating_sub(tail_bytes);
        while tail_start < raw_text.len() && !raw_text.is_char_boundary(tail_start) {
            tail_start += 1;
        }
        let omitted = raw_text.len().saturating_sub(head_end + (raw_text.len() - tail_start));
        (
            format!(
                "{}\n… [truncated {} bytes] …\n{}",
                &raw_text[..head_end],
                omitted,
                &raw_text[tail_start..]
            ),
            1,
        )
    } else {
        (raw_text.to_string(), 0)
    };
    let mut meta: Map<String, Value> = obj.clone();
    meta.remove("text");
    c.execute(
        "INSERT INTO chat_logs (message_id, main_session_id, attempt_id, seq, turn_index, role,
             content, content_truncated, meta_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(message_id) DO UPDATE SET main_session_id = ?2, attempt_id = ?3, seq = ?4,
             turn_index = ?5, role = ?6, content = ?7, content_truncated = ?8, meta_json = ?9",
        params![
            id,
            m.session_id,
            attempt,
            m.seq,
            m.turn_index,
            role,
            text,
            truncated,
            Value::Object(meta).to_string()
        ],
    )?;

    if role == "ASSISTANT" {
        let status = match get("status") {
            Some("streaming") => "RUNNING",
            Some("error") => "FAILED",
            Some("cancelled") => "CANCELLED",
            _ => "IDLE",
        };
        let ended = obj.get("endedAt").and_then(Value::as_f64);
        c.execute(
            &format!(
                "UPDATE attempts SET status = ?2,
                     turn_count = (SELECT COUNT(*) FROM chat_logs
                                   WHERE attempt_id = ?1 AND role = 'ASSISTANT'),
                     last_turn_ended_at = COALESCE({}, last_turn_ended_at)
                 WHERE id = ?1",
                ms_to_iso(3)
            ),
            params![attempt, status, ended],
        )?;
    }
    Ok(())
}

fn set_setting(c: &Connection, s: &SettingRow) -> rusqlite::Result<()> {
    c.execute(
        "INSERT INTO settings (key, value_json) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value_json = ?2",
        params![s.key, json(&s.value)],
    )?;
    Ok(())
}

/// Persist provider detection snapshot without clobbering melt / half-open ownership (F15.b).
fn replace_providers(c: &Connection, list: &[Value]) -> rusqlite::Result<()> {
    let mut keep: Vec<String> = Vec::new();
    for (i, info) in list.iter().enumerate() {
        let Some(id) = info.get("id").and_then(Value::as_str) else { continue };
        keep.push(id.to_string());
        c.execute(
            "INSERT INTO provider_health (provider_name, position, status, cli_version, info_json)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(provider_name) DO UPDATE SET
                 position = ?2,
                 status = CASE
                     WHEN provider_health.status IN ('MELTED', 'HALF_OPEN')
                     THEN provider_health.status
                     ELSE ?3
                 END,
                 cli_version = ?4,
                 info_json = ?5",
            params![
                id,
                i as i64,
                info.get("status").and_then(Value::as_str).unwrap_or("UNKNOWN"),
                info.get("version").and_then(Value::as_str),
                json(info)
            ],
        )?;
    }
    // Drop stale detection rows, but keep melt / half-open state for providers not in this scan.
    if keep.is_empty() {
        c.execute(
            "DELETE FROM provider_health WHERE status NOT IN ('MELTED', 'HALF_OPEN')",
            [],
        )?;
    } else {
        let placeholders = keep.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        c.execute(
            &format!(
                "DELETE FROM provider_health
                 WHERE provider_name NOT IN ({placeholders})
                   AND status NOT IN ('MELTED', 'HALF_OPEN')"
            ),
            rusqlite::params_from_iter(keep.iter()),
        )?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn replace_providers_for_test(c: &Connection, list: &[Value]) -> rusqlite::Result<()> {
    replace_providers(c, list)
}

/// Until P6.m3 there is at most one custom pack: setting one replaces it.
fn set_custom_pack(c: &Connection, p: Option<&CustomPackRow>) -> rusqlite::Result<()> {
    c.execute("DELETE FROM custom_packs", [])?;
    if let Some(p) = p {
        let name = p.pack.get("packVersion").and_then(Value::as_str).unwrap_or("custom");
        c.execute(
            &format!(
                "INSERT INTO custom_packs (name, source_url, position, pack_json, warnings_json,
                     imported_at)
                 VALUES (?1, ?2, 0, ?3, ?4, {})",
                ms_to_iso(5)
            ),
            params![
                name,
                p.url,
                json(&p.pack),
                serde_json::to_string(&p.warnings).unwrap_or_else(|_| "[]".into()),
                p.loaded_at
            ],
        )?;
    }
    Ok(())
}

pub fn insert_usage_event(c: &Connection, u: &UsageEventRow) -> rusqlite::Result<i64> {
    let valid_attempt = match &u.attempt_id {
        Some(att) => {
            let exists: bool = c
                .query_row("SELECT EXISTS(SELECT 1 FROM attempts WHERE id = ?1)", [att], |r| r.get(0))
                .unwrap_or(false);
            if exists { Some(att.as_str()) } else { None }
        }
        None => None,
    };
    c.execute(
        "INSERT INTO usage_events (
            attempt_id, task_id, plan_id, provider_name, model_id, role, domain, turn_index,
            input_tokens, cache_read_tokens, cache_write_tokens, output_tokens,
            api_calls, context_tokens, gap_seconds, cost_usd, equiv_cost_usd,
            phase, source, credits
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
            ?9, ?10, ?11, ?12,
            ?13, ?14, ?15, ?16, ?17,
            ?18, ?19, ?20
        )",
        params![
            valid_attempt,
            u.task_id,
            u.plan_id,
            u.provider_name,
            u.model_id,
            u.role,
            u.domain,
            u.turn_index,
            u.input_tokens,
            u.cache_read_tokens,
            u.cache_write_tokens,
            u.output_tokens,
            u.api_calls,
            u.context_tokens,
            u.gap_seconds,
            u.cost_usd,
            u.equiv_cost_usd,
            u.phase,
            u.source,
            u.credits,
        ],
    )?;
    Ok(c.last_insert_rowid())
}

/// F14.c: usage events for one attempt (turn), ordered by id.
pub fn list_usage_events_by_attempt(
    c: &Connection,
    attempt_id: &str,
) -> rusqlite::Result<Vec<UsageEventRow>> {
    let mut stmt = c.prepare(
        "SELECT id, attempt_id, task_id, plan_id, provider_name, model_id, role, domain, turn_index,
                input_tokens, cache_read_tokens, cache_write_tokens, output_tokens,
                api_calls, context_tokens, gap_seconds, cost_usd, equiv_cost_usd,
                phase, source, created_at, credits
         FROM usage_events WHERE attempt_id = ?1 ORDER BY id ASC",
    )?;
    let rows = stmt.query_map(params![attempt_id], |r| {
        Ok(UsageEventRow {
            id: Some(r.get(0)?),
            attempt_id: r.get(1)?,
            task_id: r.get(2)?,
            plan_id: r.get(3)?,
            provider_name: r.get(4)?,
            model_id: r.get(5)?,
            role: r.get(6)?,
            domain: r.get(7)?,
            turn_index: r.get(8)?,
            input_tokens: r.get(9)?,
            cache_read_tokens: r.get(10)?,
            cache_write_tokens: r.get(11)?,
            output_tokens: r.get(12)?,
            api_calls: r.get(13)?,
            context_tokens: r.get(14)?,
            gap_seconds: r.get(15)?,
            cost_usd: r.get(16)?,
            equiv_cost_usd: r.get(17)?,
            phase: r.get(18)?,
            source: r.get(19)?,
            created_at: r.get(20)?,
            credits: r.get(21)?,
        })
    })?;
    rows.collect()
}

pub fn list_usage_events(
    c: &Connection,
    session_id: Option<&str>,
    plan_id: Option<i64>,
) -> rusqlite::Result<Vec<UsageEventRow>> {
    let mut sql = String::from(
        "SELECT id, attempt_id, task_id, plan_id, provider_name, model_id, role, domain, turn_index,
                input_tokens, cache_read_tokens, cache_write_tokens, output_tokens,
                api_calls, context_tokens, gap_seconds, cost_usd, equiv_cost_usd,
                phase, source, created_at, credits
         FROM usage_events WHERE 1=1"
    );
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(sid) = session_id {
        sql.push_str(" AND (attempt_id LIKE ? || '|%' OR attempt_id = ?)");
        params_vec.push(Box::new(sid.to_string()));
        params_vec.push(Box::new(sid.to_string()));
    }
    if let Some(pid) = plan_id {
        sql.push_str(" AND plan_id = ?");
        params_vec.push(Box::new(pid));
    }
    sql.push_str(" ORDER BY id ASC");

    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|b| b.as_ref()).collect();
    let mut stmt = c.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), |r| {
        Ok(UsageEventRow {
            id: Some(r.get(0)?),
            attempt_id: r.get(1)?,
            task_id: r.get(2)?,
            plan_id: r.get(3)?,
            provider_name: r.get(4)?,
            model_id: r.get(5)?,
            role: r.get(6)?,
            domain: r.get(7)?,
            turn_index: r.get(8)?,
            input_tokens: r.get(9)?,
            cache_read_tokens: r.get(10)?,
            cache_write_tokens: r.get(11)?,
            output_tokens: r.get(12)?,
            api_calls: r.get(13)?,
            context_tokens: r.get(14)?,
            gap_seconds: r.get(15)?,
            cost_usd: r.get(16)?,
            equiv_cost_usd: r.get(17)?,
            phase: r.get(18)?,
            source: r.get(19)?,
            created_at: r.get(20)?,
            credits: r.get(21)?,
        })
    })?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// Applies a batch in a fixed order: parents before children, deletes last (a delete
/// cascades to rows written earlier in the same batch).
fn apply_each(b: WriteBatch, mut f: impl FnMut(Job)) {
    for p in b.projects {
        f(Box::new(move |c| upsert_project(c, &p)));
    }
    for s in b.sessions {
        f(Box::new(move |c| upsert_session(c, &s)));
    }
    for m in b.messages {
        f(Box::new(move |c| upsert_message(c, &m)));
    }
    // After messages: the thread names an attempt the messages create.
    for s in b.session_chat {
        f(Box::new(move |c| update_session_chat(c, &s)));
    }
    for u in b.usage_events {
        f(Box::new(move |c| insert_usage_event(c, &u).map(|_| ())));
    }
    for s in b.settings {
        f(Box::new(move |c| set_setting(c, &s)));
    }
    if let Some(list) = b.providers {
        f(Box::new(move |c| replace_providers(c, &list)));
    }
    if let Some(p) = b.custom_pack {
        f(Box::new(move |c| set_custom_pack(c, Some(&p))));
    } else if b.clear_custom_pack {
        f(Box::new(|c| set_custom_pack(c, None)));
    }
    for id in b.delete_sessions {
        f(Box::new(move |c| c.execute("DELETE FROM main_sessions WHERE id = ?1", [id]).map(|_| ())));
    }
    for id in b.delete_projects {
        f(Box::new(move |c| c.execute("DELETE FROM projects WHERE id = ?1", [id]).map(|_| ())));
    }
}

/// One job per item, for the batched writer.
pub fn jobs(b: WriteBatch) -> Vec<Job> {
    let mut out = Vec::new();
    apply_each(b, |j| out.push(j));
    out
}

/// The whole legacy import as one job: all of it lands, plus the done flag, or none of it.
/// Rows are only upserted, so retrying after a failure starts from a consistent state.
pub fn import_job(b: WriteBatch) -> Job {
    Box::new(move |c| {
        let mut parts = Vec::new();
        apply_each(b, |j| parts.push(j));
        for j in parts {
            j(c)?;
        }
        set_setting(
            c,
            &SettingRow {
                key: LEGACY_IMPORTED.into(),
                value: Value::Bool(true),
            },
        )
    })
}

pub fn load(c: &Connection) -> rusqlite::Result<Snapshot> {
    let mut projects: Vec<ProjectOut> = c
        .prepare("SELECT id, name, local_path, expanded, trusted FROM projects ORDER BY position, rowid")?
        .query_map([], |r| {
            Ok(ProjectOut {
                id: r.get(0)?,
                name: r.get(1)?,
                path: r.get(2)?,
                expanded: r.get(3)?,
                trusted: r.get(4)?,
                sessions: vec![],
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    let mut conversations = HashMap::new();
    {
        let mut stmt = c.prepare(&format!(
            "SELECT id, project_id, title, status, {}, worktree_json, archived, forked_from,
                 thread_json, queue_json
             FROM main_sessions ORDER BY position, rowid",
            iso_to_ms("updated_at")
        ))?;
        let rows = stmt.query_map([], |r| {
            let session = SessionOut {
                id: r.get(0)?,
                title: r.get(2)?,
                state: state_from_db(&r.get::<_, String>(3)?),
                updated_at: r.get(4)?,
                worktree: r.get::<_, Option<String>>(5)?.map(|s| parse(&s)),
                archived: r.get(6)?,
                forked_from: r.get(7)?,
            };
            let thread = r.get::<_, Option<String>>(8)?.map(|s| parse(&s)).unwrap_or(Value::Null);
            let queue = parse(&r.get::<_, String>(9)?);
            Ok((r.get::<_, String>(1)?, session, thread, queue))
        })?;
        let index: HashMap<String, usize> =
            projects.iter().enumerate().map(|(i, p)| (p.id.clone(), i)).collect();
        for row in rows {
            let (project_id, session, thread, queue) = row?;
            conversations.insert(
                session.id.clone(),
                ConversationOut {
                    messages: vec![],
                    queue,
                    thread,
                },
            );
            if let Some(&i) = index.get(&project_id) {
                projects[i].sessions.push(session);
            }
        }
    }

    {
        let mut stmt =
            c.prepare("SELECT main_session_id, content, meta_json FROM chat_logs ORDER BY main_session_id, seq, id")?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        })?;
        for row in rows {
            let (session, content, meta) = row?;
            let mut msg = match parse(&meta) {
                Value::Object(m) => m,
                _ => continue,
            };
            msg.insert("text".into(), Value::String(content));
            if let Some(conv) = conversations.get_mut(&session) {
                conv.messages.push(Value::Object(msg));
            }
        }
    }

    let settings = c
        .prepare("SELECT key, value_json FROM settings")?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, parse(&r.get::<_, String>(1)?))))?
        .collect::<rusqlite::Result<HashMap<_, _>>>()?;

    let providers = c
        .prepare("SELECT info_json FROM provider_health ORDER BY position")?
        .query_map([], |r| Ok(parse(&r.get::<_, String>(0)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let custom_pack = c
        .query_row(
            &format!(
                "SELECT source_url, {}, pack_json, warnings_json FROM custom_packs
                 WHERE enabled = 1 ORDER BY position LIMIT 1",
                iso_to_ms("imported_at")
            ),
            [],
            |r| {
                Ok(CustomPackRow {
                    url: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    loaded_at: r.get::<_, i64>(1)? as f64,
                    pack: parse(&r.get::<_, String>(2)?),
                    warnings: serde_json::from_str(&r.get::<_, String>(3)?).unwrap_or_default(),
                })
            },
        )
        .optional()?;

    Ok(Snapshot {
        projects,
        conversations,
        settings,
        providers,
        custom_pack,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn db() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        c.execute_batch(super::super::PRAGMAS).unwrap();
        super::super::embedded::migrations::runner().run(&mut c).unwrap();
        c
    }

    fn apply(c: &Connection, b: WriteBatch) {
        for j in jobs(b) {
            j(c).unwrap();
        }
    }

    #[test]
    fn debug_attempts_keep_actual_source_separate_from_normal_luna_sessions() {
        let c = db(); let mut batch = sample();
        batch.messages = serde_json::from_value(json!([
            {"sessionId":"s1","seq":0,"turnIndex":0,"provider":"CODEX_CLI","model":"gpt-6-luna","mode":"direct",
             "message":{"id":"debug-message","role":"assistant","text":"debug output","model":"opus","ref":{"provider":"CLAUDE_CODE","model":"opus"},"status":"done",
              "debugRoute":{"output":"luna","requestedProvider":"CLAUDE_CODE","requestedModel":"opus","actualProvider":"CODEX_CLI","actualModel":"gpt-6-luna"}}},
            {"sessionId":"s1","seq":1,"turnIndex":1,"provider":"CODEX_CLI","model":"gpt-6-luna","mode":"direct",
             "message":{"id":"normal-message","role":"assistant","text":"normal output","model":"gpt-6-luna","status":"done"}}
        ])).unwrap();
        batch.session_chat = serde_json::from_value(json!([{"sessionId":"s1","thread":{"ref":{"provider":"CLAUDE_CODE","model":"opus"},"id":"tokage-debug:[\"luna\",\"CODEX_CLI\",\"gpt-6-luna\",\"native-id\"]"},"queue":[]}])).unwrap();
        apply(&c, batch);
        let modes: Vec<(String, String)> = c.prepare("SELECT id, COALESCE(debug_output, exec_mode) FROM attempts ORDER BY id").unwrap().query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect();
        assert!(modes.contains(&("s1|DEBUG_LUNA|CODEX_CLI|gpt-6-luna".into(), "luna".into())));
        assert!(modes.contains(&("s1|CODEX_CLI|gpt-6-luna".into(), "HEADLESS".into())));
        let target: String = c.query_row("SELECT attempt_id FROM chat_logs WHERE message_id='debug-message'", [], |r| r.get(0)).unwrap();
        assert_eq!(target, "s1|DEBUG_LUNA|CODEX_CLI|gpt-6-luna");
        let restored = load(&c).unwrap();
        assert_eq!(restored.conversations["s1"].messages[0]["model"], "opus");
    }

    #[test]
    fn v10_preserves_existing_attempts_and_adds_nullable_provenance() {
        let mut c = Connection::open_in_memory().unwrap();
        super::super::embedded::migrations::runner().set_target(refinery::Target::Version(9)).run(&mut c).unwrap();
        c.execute_batch("INSERT INTO projects (id,name,local_path) VALUES ('p','old','/old'); INSERT INTO main_sessions (id,project_id,title) VALUES ('s','p','existing'); INSERT INTO attempts (id,main_session_id,role,provider_name,model_id,workdir) VALUES ('a','s','EXECUTOR','CODEX_CLI','gpt-6-luna','/old');").unwrap();
        super::super::run_migrations(&mut c).unwrap();
        let rows = super::super::orchestration::list_attempts_for_session(&c, "s").unwrap();
        assert_eq!(rows.len(), 1); assert_eq!(rows[0].model_id, "gpt-6-luna");
        assert_eq!(rows[0].exec_mode, "HEADLESS"); assert_eq!(rows[0].debug_output, None);
        assert_eq!(rows[0].requested_model, None);
    }

    fn sample() -> WriteBatch {
        serde_json::from_value(json!({
            "projects": [
                { "id": "p1", "name": "a", "path": "/a", "position": 0, "expanded": true, "trusted": true },
                { "id": "p2", "name": "b", "path": "/b", "position": 1, "expanded": false, "trusted": null }
            ],
            "sessions": [
                { "id": "s1", "projectId": "p1", "title": "one", "state": "draft", "position": 0,
                  "updatedAt": 1767225600123.0,
                  "worktree": { "root": "/wt", "cwd": "/wt/a", "branch": "tokage/x" } },
                { "id": "s2", "projectId": "p1", "title": "two", "state": "draft", "position": 1,
                  "archived": true, "forkedFrom": "s1", "updatedAt": 1767225600000.0 }
            ],
            "messages": [
                { "sessionId": "s1", "seq": 0, "turnIndex": 0, "provider": "CLAUDE_CODE", "model": "opus", "mode": "plan",
                  "message": { "id": "m1", "role": "user", "text": "hi" } },
                { "sessionId": "s1", "seq": 1, "turnIndex": 0, "provider": "CLAUDE_CODE", "model": "opus",
                  "message": { "id": "m2", "role": "assistant", "text": "hello", "mode": "plan",
                               "provider": "CLAUDE_CODE", "ref": { "provider": "CLAUDE_CODE", "model": "opus" },
                               "status": "done", "endedAt": 1767225601000.0,
                               "parts": [{ "type": "text", "text": "hello" }],
                               "usage": { "input": 10, "output": 2 } } }
            ],
            "sessionChat": [
                { "sessionId": "s1", "thread": { "ref": { "provider": "CLAUDE_CODE", "model": "opus" }, "id": "cli-1" },
                  "queue": [{ "id": "q1", "text": "later", "mode": "auto", "createdAt": 1 }] }
            ],
            "settings": [
                { "key": "settings.theme", "value": "dark" },
                { "key": "chat.picks", "value": { "plan": null, "direct": { "provider": "CODEX_CLI", "model": "gpt" } } }
            ],
            "providers": [{ "id": "CLAUDE_CODE", "status": "READY", "version": "2.0" }],
            "customPack": { "url": "https://x/pack.json", "loadedAt": 1767225600000.0,
                            "pack": { "packVersion": "9", "models": [] }, "warnings": ["w"] }
        }))
        .unwrap()
    }

    #[test]
    fn batch_round_trips_through_load() {
        let c = db();
        apply(&c, sample());
        let s = serde_json::to_value(load(&c).unwrap()).unwrap();

        assert_eq!(s["projects"][0]["id"], "p1");
        assert_eq!(s["projects"][0]["trusted"], true);
        assert!(s["projects"][1].get("trusted").is_none());
        let sessions = &s["projects"][0]["sessions"];
        assert_eq!(sessions[0]["id"], "s1");
        assert_eq!(sessions[0]["updatedAt"], 1767225600123i64);
        assert_eq!(sessions[0]["worktree"]["branch"], "tokage/x");
        assert_eq!(sessions[1]["archived"], true);
        assert_eq!(sessions[1]["forkedFrom"], "s1");

        let conv = &s["conversations"]["s1"];
        assert_eq!(conv["messages"][0]["text"], "hi");
        assert_eq!(conv["messages"][1]["text"], "hello");
        assert_eq!(conv["messages"][1]["usage"]["input"], 10);
        assert_eq!(conv["thread"]["id"], "cli-1");
        assert_eq!(conv["queue"][0]["text"], "later");
        assert_eq!(s["conversations"]["s2"]["messages"], json!([]));

        assert_eq!(s["settings"]["settings.theme"], "dark");
        assert_eq!(s["settings"]["chat.picks"]["direct"]["model"], "gpt");
        assert_eq!(s["providers"][0]["version"], "2.0");
        assert_eq!(s["customPack"]["url"], "https://x/pack.json");
        assert_eq!(s["customPack"]["loadedAt"], 1767225600000.0);
        assert_eq!(s["customPack"]["warnings"][0], "w");

        let (role, turns, cli): (String, i64, String) = c
            .query_row(
                "SELECT role, turn_count, cli_window_identity FROM attempts WHERE id = 's1|CLAUDE_CODE|opus'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!((role.as_str(), turns, cli.as_str()), ("PLANNER", 1, "cli-1"));
    }

    #[test]
    fn upserting_a_parent_keeps_its_children() {
        let c = db();
        apply(&c, sample());
        let mut again = sample();
        again.messages.clear();
        again.projects[0].name = "renamed".into();
        again.sessions[0].title = "retitled".into();
        apply(&c, again);
        let s = load(&c).unwrap();
        assert_eq!(s.projects[0].name, "renamed");
        assert_eq!(s.conversations["s1"].messages.len(), 2);
    }

    #[test]
    fn deletes_cascade_and_unlink_forks() {
        let c = db();
        apply(&c, sample());
        apply(
            &c,
            WriteBatch {
                delete_sessions: vec!["s1".into()],
                ..Default::default()
            },
        );
        let n: i64 = c.query_row("SELECT COUNT(*) FROM chat_logs", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
        let fork: Option<String> = c
            .query_row("SELECT forked_from FROM main_sessions WHERE id = 's2'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fork, None);

        apply(
            &c,
            WriteBatch {
                delete_projects: vec!["p1".into()],
                ..Default::default()
            },
        );
        let n: i64 = c.query_row("SELECT COUNT(*) FROM attempts", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn fork_of_a_missing_session_is_stored_unlinked() {
        let c = db();
        let mut b = sample();
        b.sessions[1].forked_from = Some("gone".into());
        apply(&c, b);
        let s = load(&c).unwrap();
        assert_eq!(s.projects[0].sessions[1].forked_from, None);
    }

    #[test]
    fn import_is_all_or_nothing_and_retryable() {
        let c = db();
        let mut bad = sample();
        bad.messages[1].message = json!("not an object");
        c.execute_batch("SAVEPOINT t").unwrap();
        assert!(import_job(bad)(&c).is_err());
        c.execute_batch("ROLLBACK TO t; RELEASE t").unwrap();
        let s = load(&c).unwrap();
        assert!(s.projects.is_empty());
        assert!(!s.settings.contains_key(LEGACY_IMPORTED));

        import_job(sample())(&c).unwrap();
        // A second run (e.g. a retry after the flag write was lost) changes nothing.
        import_job(sample())(&c).unwrap();
        let s = load(&c).unwrap();
        assert_eq!(s.projects.len(), 2);
        assert_eq!(s.conversations["s1"].messages.len(), 2);
        assert_eq!(s.settings[LEGACY_IMPORTED], Value::Bool(true));
    }

    #[test]
    fn clearing_the_custom_pack() {
        let c = db();
        apply(&c, sample());
        apply(
            &c,
            WriteBatch {
                clear_custom_pack: true,
                ..Default::default()
            },
        );
        assert!(load(&c).unwrap().custom_pack.is_none());
    }

    #[test]
    fn usage_events_insert_and_list() {
        let c = db();
        let u1 = UsageEventRow {
            id: None,
            attempt_id: None,
            task_id: None,
            plan_id: Some(101),
            provider_name: "CLAUDE_CODE".into(),
            model_id: Some("claude-haiku-4-5-20251001".into()),
            role: Some("PLANNER".into()),
            domain: None,
            turn_index: Some(0),
            input_tokens: 60,
            cache_read_tokens: 107833,
            cache_write_tokens: 28664,
            output_tokens: 335,
            api_calls: Some(4),
            context_tokens: Some(34319),
            gap_seconds: None,
            cost_usd: Some(0.0483483),
            equiv_cost_usd: None,
            credits: Some(0.0123),
            phase: "PLANNING".into(),
            source: "STRUCTURED".into(),
            created_at: None,
        };
        let id = insert_usage_event(&c, &u1).unwrap();
        assert!(id > 0);

        let list = list_usage_events(&c, None, Some(101)).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].provider_name, "CLAUDE_CODE");
        assert_eq!(list[0].input_tokens, 60);
        assert_eq!(list[0].cache_read_tokens, 107833);
        assert_eq!(list[0].cache_write_tokens, 28664);
        assert_eq!(list[0].output_tokens, 335);
        assert_eq!(list[0].api_calls, Some(4));
        assert_eq!(list[0].context_tokens, Some(34319));
        assert_eq!(list[0].phase, "PLANNING");
        assert_eq!(list[0].source, "STRUCTURED");
        assert_eq!(list[0].credits, Some(0.0123));

        // Batch insertion test
        let mut u2 = u1.clone();
        u2.provider_name = "CODEX_CLI".into();
        u2.phase = "DIRECT".into();
        apply(&c, WriteBatch {
            usage_events: vec![u2],
            ..Default::default()
        });
        let all = list_usage_events(&c, None, None).unwrap();
        assert_eq!(all.len(), 2);
    }
}
