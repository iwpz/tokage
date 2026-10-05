//! External native session introspection (docs/01 §5, P4.m2).
//! Reads native CLI session files (Claude, Codex, Cursor, Copilot) read-only,
//! without modifying or creating any external files.

use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use tokage_core::{NativeSession, ProviderId};

/// Read native sessions for a given provider, filtered by project path if specified.
pub fn read_native_sessions(
    provider_id: ProviderId,
    project_filter: Option<&Path>,
) -> Vec<NativeSession> {
    match provider_id {
        ProviderId::ClaudeCode => read_claude_sessions(project_filter),
        ProviderId::CodexCli => read_codex_sessions(project_filter),
        ProviderId::CursorCli => read_cursor_sessions(project_filter),
        ProviderId::CopilotCli => read_copilot_sessions(project_filter),
        _ => Vec::new(),
    }
}

/// Read all native sessions across all supported providers, sorted by updated_at descending.
pub fn read_all_native_sessions(project_filter: Option<&Path>) -> Vec<NativeSession> {
    let mut sessions = Vec::new();
    sessions.extend(read_claude_sessions(project_filter));
    sessions.extend(read_codex_sessions(project_filter));
    sessions.extend(read_cursor_sessions(project_filter));
    sessions.extend(read_copilot_sessions(project_filter));
    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    sessions
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn format_system_time(time: std::time::SystemTime) -> String {
    let duration = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = duration.as_secs() as i64;
    format_unix_to_utc_iso(secs)
}

fn format_unix_to_utc_iso(secs: i64) -> String {
    crate::chat::quota::format_unix_to_utc_iso(secs)
}

fn paths_match(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    // Also compare canonicalized paths if possible
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => false,
    }
}

// ==========================================
// Claude Code: ~/.claude/projects/<slug>/<id>.jsonl
// ==========================================

fn read_claude_sessions(project_filter: Option<&Path>) -> Vec<NativeSession> {
    let home = match home_dir() {
        Some(h) => h,
        None => return Vec::new(),
    };
    let projects_dir = home.join(".claude").join("projects");
    if !projects_dir.is_dir() {
        return Vec::new();
    }

    let mut sessions = Vec::new();
    let project_entries = match fs::read_dir(&projects_dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    for p_entry in project_entries.flatten() {
        let path = p_entry.path();
        if !path.is_dir() {
            continue;
        }
        let session_entries = match fs::read_dir(&path) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for s_entry in session_entries.flatten() {
            let s_path = s_entry.path();
            if s_path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }

            if let Some(meta) = parse_claude_session_file(&s_path) {
                if let Some(target) = project_filter {
                    if !paths_match(Path::new(&meta.cwd), target) {
                        continue;
                    }
                }
                sessions.push(meta);
            }
        }
    }

    sessions
}

fn parse_claude_session_file(path: &Path) -> Option<NativeSession> {
    let file = File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    let file_size = metadata.len();
    let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
    let mut updated_at = format_system_time(modified);

    let session_id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();

    let reader = BufReader::new(file);
    let mut title = None;
    let mut cwd = None;
    let mut first_user_prompt = None;

    // Scan lines for metadata
    for line in reader.lines().flatten() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
            let typ = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
            if typ == "custom-title" || typ == "agent-name" {
                if let Some(ct) = v
                    .get("customTitle")
                    .or_else(|| v.get("agentName"))
                    .and_then(|t| t.as_str())
                {
                    if !ct.trim().is_empty() {
                        title = Some(ct.trim().to_string());
                    }
                }
            } else if typ == "user" || typ == "queue-operation" {
                if cwd.is_none() {
                    if let Some(c) = v.get("cwd").and_then(|s| s.as_str()) {
                        cwd = Some(c.to_string());
                    }
                }
                if first_user_prompt.is_none() {
                    if let Some(content) = v.get("content").and_then(|s| s.as_str()) {
                        first_user_prompt = Some(content.trim().to_string());
                    } else if let Some(msg_content) = v
                        .get("message")
                        .and_then(|m| m.get("content"))
                        .and_then(|c| c.as_str())
                    {
                        first_user_prompt = Some(msg_content.trim().to_string());
                    }
                }
            }
            if let Some(ts) = v.get("timestamp").and_then(|s| s.as_str()) {
                if !ts.is_empty() {
                    updated_at = ts.to_string();
                }
            }
        }
    }

    let final_title = title
        .or(first_user_prompt)
        .unwrap_or_else(|| session_id.clone());

    let final_cwd = cwd.unwrap_or_default();

    Some(NativeSession {
        id: session_id,
        provider_id: ProviderId::ClaudeCode,
        title: final_title,
        cwd: final_cwd,
        updated_at,
        file_size,
        path: path.to_string_lossy().to_string(),
    })
}

// ==========================================
// Codex CLI: ~/.codex/sessions/<yyyy>/<mm>/<dd>/rollout-<...>.jsonl
// ==========================================

fn read_codex_sessions(project_filter: Option<&Path>) -> Vec<NativeSession> {
    let home = match home_dir() {
        Some(h) => h,
        None => return Vec::new(),
    };
    let sessions_dir = home.join(".codex").join("sessions");
    if !sessions_dir.is_dir() {
        return Vec::new();
    }

    let mut sessions = Vec::new();
    scan_codex_dir_recursive(&sessions_dir, project_filter, &mut sessions, 0);
    sessions
}

fn scan_codex_dir_recursive(
    dir: &Path,
    project_filter: Option<&Path>,
    out: &mut Vec<NativeSession>,
    depth: usize,
) {
    if depth > 5 {
        return;
    }
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_codex_dir_recursive(&path, project_filter, out, depth + 1);
        } else if path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
            if let Some(meta) = parse_codex_session_file(&path) {
                if let Some(target) = project_filter {
                    if !paths_match(Path::new(&meta.cwd), target) {
                        continue;
                    }
                }
                out.push(meta);
            }
        }
    }
}

fn parse_codex_session_file(path: &Path) -> Option<NativeSession> {
    let file = File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    let file_size = metadata.len();
    let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
    let mut updated_at = format_system_time(modified);

    let mut session_id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();

    let reader = BufReader::new(file);
    let mut cwd = None;
    let mut first_prompt = None;

    for line in reader.lines().flatten() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
            let typ = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
            if typ == "session_meta" {
                if let Some(payload) = v.get("payload") {
                    if let Some(sid) = payload.get("session_id").and_then(|s| s.as_str()) {
                        session_id = sid.to_string();
                    }
                    if let Some(c) = payload.get("cwd").and_then(|s| s.as_str()) {
                        cwd = Some(c.to_string());
                    }
                }
            } else if typ == "response_item" && first_prompt.is_none() {
                if let Some(payload) = v.get("payload") {
                    if payload.get("role").and_then(|r| r.as_str()) == Some("user") {
                        if let Some(contents) = payload.get("content").and_then(|c| c.as_array()) {
                            for item in contents {
                                if let Some(txt) = item.get("text").and_then(|t| t.as_str()) {
                                    if !txt.starts_with("<environment_context>")
                                        && !txt.starts_with("<system_")
                                        && !txt.starts_with("<external_")
                                    {
                                        first_prompt = Some(txt.trim().to_string());
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if let Some(ts) = v.get("timestamp").and_then(|s| s.as_str()) {
                if !ts.is_empty() {
                    updated_at = ts.to_string();
                }
            }
        }
    }

    let title = first_prompt.unwrap_or_else(|| session_id.clone());
    let final_cwd = cwd.unwrap_or_default();

    Some(NativeSession {
        id: session_id,
        provider_id: ProviderId::CodexCli,
        title,
        cwd: final_cwd,
        updated_at,
        file_size,
        path: path.to_string_lossy().to_string(),
    })
}

// ==========================================
// Cursor CLI: ~/.cursor/chats/<hash>/<agentId>/meta.json
// ==========================================

fn read_cursor_sessions(project_filter: Option<&Path>) -> Vec<NativeSession> {
    let home = match home_dir() {
        Some(h) => h,
        None => return Vec::new(),
    };
    let chats_dir = home.join(".cursor").join("chats");
    if !chats_dir.is_dir() {
        return Vec::new();
    }

    let mut sessions = Vec::new();
    let hash_entries = match fs::read_dir(&chats_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    for h_entry in hash_entries.flatten() {
        let h_path = h_entry.path();
        if !h_path.is_dir() {
            continue;
        }
        let session_entries = match fs::read_dir(&h_path) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for s_entry in session_entries.flatten() {
            let s_path = s_entry.path();
            if !s_path.is_dir() {
                continue;
            }
            let meta_json = s_path.join("meta.json");
            if meta_json.is_file() {
                if let Some(meta) = parse_cursor_session_dir(&s_path, &meta_json) {
                    if let Some(target) = project_filter {
                        if !paths_match(Path::new(&meta.cwd), target) {
                            continue;
                        }
                    }
                    sessions.push(meta);
                }
            }
        }
    }

    sessions
}

fn parse_cursor_session_dir(dir: &Path, meta_file: &Path) -> Option<NativeSession> {
    let mut file = File::open(meta_file).ok()?;
    let mut content = String::new();
    file.read_to_string(&mut content).ok()?;

    let v: serde_json::Value = serde_json::from_str(&content).ok()?;
    let cwd = v.get("cwd").and_then(|s| s.as_str()).unwrap_or_default().to_string();
    let updated_ms = v.get("updatedAtMs").and_then(|n| n.as_i64()).unwrap_or(0);
    let updated_at = if updated_ms > 0 {
        format_unix_to_utc_iso(updated_ms / 1000)
    } else {
        let meta = dir.metadata().ok()?;
        format_system_time(meta.modified().unwrap_or(UNIX_EPOCH))
    };

    let session_id = dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();

    let mut total_size = meta_file.metadata().map(|m| m.len()).unwrap_or(0);
    let store_db = dir.join("store.db");
    if let Ok(m) = store_db.metadata() {
        total_size += m.len();
    }

    Some(NativeSession {
        id: session_id.clone(),
        provider_id: ProviderId::CursorCli,
        title: session_id,
        cwd,
        updated_at,
        file_size: total_size,
        path: dir.to_string_lossy().to_string(),
    })
}

// ==========================================
// GitHub Copilot CLI: ~/.copilot/session-state/<id>/workspace.yaml
// ==========================================

fn read_copilot_sessions(project_filter: Option<&Path>) -> Vec<NativeSession> {
    let home = match home_dir() {
        Some(h) => h,
        None => return Vec::new(),
    };
    let sessions_dir = home.join(".copilot").join("session-state");
    if !sessions_dir.is_dir() {
        return Vec::new();
    }

    let mut sessions = Vec::new();
    let entries = match fs::read_dir(&sessions_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let yaml_path = path.join("workspace.yaml");
        if yaml_path.is_file() {
            if let Some(meta) = parse_copilot_session_dir(&path, &yaml_path) {
                if let Some(target) = project_filter {
                    if !paths_match(Path::new(&meta.cwd), target) {
                        continue;
                    }
                }
                sessions.push(meta);
            }
        }
    }

    sessions
}

fn parse_copilot_session_dir(dir: &Path, yaml_path: &Path) -> Option<NativeSession> {
    let mut file = File::open(yaml_path).ok()?;
    let mut content = String::new();
    file.read_to_string(&mut content).ok()?;

    let mut session_id = dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();
    let mut cwd = String::new();
    let mut title = String::new();
    let mut updated_at = String::new();

    for line in content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("id:") {
            session_id = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("cwd:") {
            cwd = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("name:") {
            title = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("updated_at:") {
            updated_at = rest.trim().to_string();
        }
    }

    if updated_at.is_empty() {
        let meta = dir.metadata().ok()?;
        updated_at = format_system_time(meta.modified().unwrap_or(UNIX_EPOCH));
    }
    if title.is_empty() {
        title = session_id.clone();
    }

    let mut total_size = yaml_path.metadata().map(|m| m.len()).unwrap_or(0);
    let events_file = dir.join("events.jsonl");
    if let Ok(m) = events_file.metadata() {
        total_size += m.len();
    }

    Some(NativeSession {
        id: session_id,
        provider_id: ProviderId::CopilotCli,
        title,
        cwd,
        updated_at,
        file_size: total_size,
        path: dir.to_string_lossy().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_format_unix_to_utc_iso() {
        assert_eq!(format_unix_to_utc_iso(1791060860), "2026-10-03T20:54:20Z");
    }

    #[test]
    fn test_parse_mock_claude_session() {
        let temp = tempdir().unwrap();
        let file_path = temp.path().join("test-session.jsonl");
        let content = r#"{"type":"queue-operation","content":"Do something great","cwd":"/path/to/project","timestamp":"2026-10-03T20:54:21Z"}
{"type":"custom-title","customTitle":"Great Feature"}
"#;
        fs::write(&file_path, content).unwrap();

        let session = parse_claude_session_file(&file_path).unwrap();
        assert_eq!(session.id, "test-session");
        assert_eq!(session.provider_id, ProviderId::ClaudeCode);
        assert_eq!(session.title, "Great Feature");
        assert_eq!(session.cwd, "/path/to/project");
        assert_eq!(session.updated_at, "2026-10-03T20:54:21Z");
    }

    #[test]
    fn test_parse_mock_codex_session() {
        let temp = tempdir().unwrap();
        let file_path = temp.path().join("rollout-2026-10-03-test.jsonl");
        let content = r#"{"type":"session_meta","payload":{"session_id":"codex-123","cwd":"/path/to/codex"}}
{"type":"response_item","payload":{"role":"user","content":[{"type":"input_text","text":"Implement player balancing"}]},"timestamp":"2026-10-04T10:00:00Z"}
"#;
        fs::write(&file_path, content).unwrap();

        let session = parse_codex_session_file(&file_path).unwrap();
        assert_eq!(session.id, "codex-123");
        assert_eq!(session.provider_id, ProviderId::CodexCli);
        assert_eq!(session.title, "Implement player balancing");
        assert_eq!(session.cwd, "/path/to/codex");
        assert_eq!(session.updated_at, "2026-10-04T10:00:00Z");
    }

    #[test]
    fn test_parse_mock_cursor_session() {
        let temp = tempdir().unwrap();
        let session_dir = temp.path().join("cursor-agent-uuid");
        fs::create_dir_all(&session_dir).unwrap();
        let meta_file = session_dir.join("meta.json");
        let content = r#"{"schemaVersion":1,"cwd":"/path/to/cursor","updatedAtMs":1791098440946}"#;
        fs::write(&meta_file, content).unwrap();

        let session = parse_cursor_session_dir(&session_dir, &meta_file).unwrap();
        assert_eq!(session.id, "cursor-agent-uuid");
        assert_eq!(session.provider_id, ProviderId::CursorCli);
        assert_eq!(session.cwd, "/path/to/cursor");
        assert!(session.updated_at.starts_with("2026-10-"));
    }

    #[test]
    fn test_parse_mock_copilot_session() {
        let temp = tempdir().unwrap();
        let session_dir = temp.path().join("copilot-uuid");
        fs::create_dir_all(&session_dir).unwrap();
        let yaml_file = session_dir.join("workspace.yaml");
        let content = r#"id: copilot-uuid
cwd: /path/to/copilot
name: Fix bug in server
updated_at: 2026-10-04T07:37:37.886Z
"#;
        fs::write(&yaml_file, content).unwrap();

        let session = parse_copilot_session_dir(&session_dir, &yaml_file).unwrap();
        assert_eq!(session.id, "copilot-uuid");
        assert_eq!(session.provider_id, ProviderId::CopilotCli);
        assert_eq!(session.title, "Fix bug in server");
        assert_eq!(session.cwd, "/path/to/copilot");
        assert_eq!(session.updated_at, "2026-10-04T07:37:37.886Z");
    }
}
