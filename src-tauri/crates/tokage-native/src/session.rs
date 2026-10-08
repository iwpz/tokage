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
        ProviderId::Reasonix => read_reasonix_sessions(project_filter),
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
    sessions.extend(read_reasonix_sessions(project_filter));
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

// ==========================================
// Reasonix CLI: ~/.reasonix/projects/<slug>/sessions/<id>.jsonl(.meta)
// Slug encodes the workspace path with `/` → `-` (e.g. /tmp/foo → -tmp-foo).
// ==========================================

fn reasonix_project_slug(path: &Path) -> String {
    path.to_string_lossy().replace('/', "-")
}

fn reasonix_slug_matches(project_dir_name: &str, filter: &Path) -> bool {
    if project_dir_name == reasonix_project_slug(filter) {
        return true;
    }
    if let Ok(canon) = fs::canonicalize(filter) {
        if project_dir_name == reasonix_project_slug(&canon) {
            return true;
        }
    }
    false
}

fn read_reasonix_sessions(project_filter: Option<&Path>) -> Vec<NativeSession> {
    let home = match home_dir() {
        Some(h) => h,
        None => return Vec::new(),
    };
    let projects_dir = home.join(".reasonix").join("projects");
    if !projects_dir.is_dir() {
        return Vec::new();
    }

    let mut sessions = Vec::new();
    let project_entries = match fs::read_dir(&projects_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    for p_entry in project_entries.flatten() {
        let project_path = p_entry.path();
        if !project_path.is_dir() {
            continue;
        }
        let slug = project_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        if let Some(target) = project_filter {
            if !reasonix_slug_matches(slug, target) {
                continue;
            }
        }
        let sessions_dir = project_path.join("sessions");
        if !sessions_dir.is_dir() {
            continue;
        }
        let session_entries = match fs::read_dir(&sessions_dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for s_entry in session_entries.flatten() {
            let s_path = s_entry.path();
            let name = s_path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            // Prefer sidecar meta; fall back to the transcript itself.
            let meta_path = if name.ends_with(".jsonl.meta") {
                s_path.clone()
            } else if s_path.extension().and_then(|s| s.to_str()) == Some("jsonl")
                && !name.contains(".jsonl.")
            {
                let sidecar = PathBuf::from(format!("{}.meta", s_path.display()));
                if sidecar.is_file() {
                    // Handled when we visit the .meta entry.
                    continue;
                }
                s_path.clone()
            } else {
                continue;
            };
            if let Some(meta) = parse_reasonix_session_meta(&meta_path, project_filter) {
                sessions.push(meta);
            }
        }
    }

    sessions
}

fn parse_reasonix_session_meta(
    meta_or_jsonl: &Path,
    project_filter: Option<&Path>,
) -> Option<NativeSession> {
    let meta_path = meta_or_jsonl;
    let is_meta = meta_path
        .file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|n| n.ends_with(".jsonl.meta"));
    let jsonl_path = if is_meta {
        // foo.jsonl.meta → foo.jsonl
        let s = meta_path.to_string_lossy();
        PathBuf::from(s.trim_end_matches(".meta"))
    } else {
        meta_path.to_path_buf()
    };

    let mut session_id = jsonl_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();
    let mut title = None;
    let mut updated_at = None;
    let mut cwd = project_filter
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();

    if is_meta {
        if let Ok(text) = fs::read_to_string(meta_path) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(id) = v.get("id").and_then(|x| x.as_str()) {
                    session_id = id.to_string();
                }
                if let Some(p) = v.get("preview").and_then(|x| x.as_str()) {
                    let p = p.trim();
                    if !p.is_empty() {
                        title = Some(p.to_string());
                    }
                }
                if let Some(ts) = v
                    .get("updated_at")
                    .or_else(|| v.get("created_at"))
                    .and_then(|x| x.as_str())
                {
                    updated_at = Some(ts.to_string());
                }
            }
        }
    }

    if cwd.is_empty() || title.is_none() {
        if let Some((extracted_cwd, first_prompt)) = reasonix_cwd_and_prompt(&jsonl_path) {
            if cwd.is_empty() {
                cwd = extracted_cwd.unwrap_or_default();
            }
            if title.is_none() {
                title = first_prompt;
            }
        }
    }

    let file_size = jsonl_path.metadata().map(|m| m.len()).unwrap_or(0)
        + meta_path
            .metadata()
            .map(|m| if is_meta { m.len() } else { 0 })
            .unwrap_or(0);

    let updated_at = updated_at.unwrap_or_else(|| {
        jsonl_path
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .map(format_system_time)
            .unwrap_or_default()
    });

    Some(NativeSession {
        id: session_id.clone(),
        provider_id: ProviderId::Reasonix,
        title: title.unwrap_or_else(|| session_id.clone()),
        cwd,
        updated_at,
        file_size,
        path: jsonl_path.to_string_lossy().to_string(),
    })
}

/// Pull workspace path / first user prompt from the transcript when meta is thin.
fn reasonix_cwd_and_prompt(jsonl: &Path) -> Option<(Option<String>, Option<String>)> {
    let file = File::open(jsonl).ok()?;
    let reader = BufReader::new(file);
    let mut cwd = None;
    let mut prompt = None;
    for line in reader.lines().flatten().take(40) {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        // Direct role messages, or nested under events `messages`.
        let messages: Vec<&serde_json::Value> = if let Some(arr) = v.get("messages").and_then(|m| m.as_array()) {
            arr.iter().collect()
        } else {
            vec![&v]
        };
        for msg in messages {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
            let content = msg
                .get("raw_content")
                .or_else(|| msg.get("content"))
                .and_then(|c| c.as_str())
                .unwrap_or("");
            if cwd.is_none() {
                if let Some(rest) = content.split("Current workspace: \"").nth(1) {
                    if let Some(path) = rest.split('"').next() {
                        if !path.is_empty() {
                            cwd = Some(path.to_string());
                        }
                    }
                }
            }
            if prompt.is_none() && role == "user" {
                let text = msg
                    .get("raw_content")
                    .and_then(|c| c.as_str())
                    .unwrap_or(content)
                    .trim();
                if !text.is_empty() && !text.starts_with('<') {
                    prompt = Some(text.to_string());
                }
            }
        }
        if cwd.is_some() && prompt.is_some() {
            break;
        }
    }
    Some((cwd, prompt))
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

    #[test]
    fn test_parse_mock_reasonix_session() {
        let temp = tempdir().unwrap();
        let sessions = temp
            .path()
            .join(".reasonix/projects/-tmp-demo/sessions");
        fs::create_dir_all(&sessions).unwrap();
        let id = "20261008-100710.967195000-deepseek-flash";
        let jsonl = sessions.join(format!("{id}.jsonl"));
        let meta = sessions.join(format!("{id}.jsonl.meta"));
        fs::write(
            &jsonl,
            r#"{"role":"user","content":"<workspace>\nCurrent workspace: \"/tmp/demo\".\n</workspace>\n\nok please","raw_content":"ok please"}
"#,
        )
        .unwrap();
        fs::write(
            &meta,
            r#"{"id":"20261008-100710.967195000-deepseek-flash","updated_at":"2026-10-08T10:07:12Z","preview":"Reply with exactly the word ok"}"#,
        )
        .unwrap();

        // Point HOME at temp so read_reasonix_sessions finds the tree.
        let prev = std::env::var_os("HOME");
        std::env::set_var("HOME", temp.path());
        let found = read_reasonix_sessions(Some(Path::new("/tmp/demo")));
        match prev {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, id);
        assert_eq!(found[0].provider_id, ProviderId::Reasonix);
        assert_eq!(found[0].title, "Reply with exactly the word ok");
        assert_eq!(found[0].cwd, "/tmp/demo");
        assert_eq!(found[0].updated_at, "2026-10-08T10:07:12Z");
    }
}
