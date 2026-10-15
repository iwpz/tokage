//! Kiro 2.31 headless ACP envelopes. Engine identity travels with the session ID.
use super::{s, ChatRequest, EventKind, FileTouch, Launch, Out, Parser, ToolKind};
use crate::env::ShellEnv;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

pub fn session(id: &str) -> Result<(&str, &str), String> {
    let (engine, raw) = if let Some(raw) = id.strip_prefix("kiro:v2:") {
        ("v2", raw)
    } else if let Some(raw) = id.strip_prefix("kiro:v3:") {
        ("v3", raw)
    } else {
        (if id.starts_with("sess_") { "v3" } else { "v2" }, id)
    };
    if raw.is_empty()
        || raw.len() > 128
        || !raw
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("Invalid Kiro session ID".into());
    }
    Ok((engine, raw))
}

pub fn launch(req: &ChatRequest, prompt: String) -> Launch {
    let mut args = vec![
        "chat".into(),
        "--no-interactive".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--model".into(),
        req.model.clone(),
        "--trust-tools=".into(),
    ];
    if let Some(id) = &req.resume {
        if let Ok((engine, raw)) = session(id) {
            args.extend([format!("--{engine}"), "--resume-id".into(), raw.into()]);
        }
    }
    args.push(prompt);
    Launch {
        args,
        stdin: None,
        env_remove: vec![],
        parser: Box::new(Kiro::default()),
    }
}

/// A unique agent per invocation prevents role/scope races between parallel worktrees.
/// No user config is overwritten. The profile is removed after the child exits.
pub struct Runtime {
    path: PathBuf,
}
impl Drop for Runtime {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub fn prepare(
    req: &ChatRequest,
    env: &ShellEnv,
    args: &mut Vec<String>,
) -> Result<Runtime, String> {
    if req.fork {
        return Err("Kiro native fork is not enabled; replay history into a new session".into());
    }
    let engine = match &req.resume {
        Some(id) => session(id)?.0.to_string(),
        None => env
            .vars
            .get("TOKAGE_KIRO_ENGINE")
            .cloned()
            .unwrap_or_else(|| "v2".into()),
    };
    if !matches!(engine.as_str(), "v2" | "v3") {
        return Err("TOKAGE_KIRO_ENGINE must be v2 or v3".into());
    }
    let home = PathBuf::from(env.vars.get("HOME").ok_or("Kiro requires HOME")?);
    let cwd = Path::new(&req.cwd)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    // V3 loads standalone hooks independently of the selected agent. Refuse these
    // until they can be routed through Tokage, rather than running user commands silently.
    for root in std::iter::once(home.as_path()).chain(cwd.ancestors()) {
        let hooks = root.join(".kiro/hooks");
        if engine == "v3"
            && hooks
                .read_dir()
                .map(|mut d| d.next().is_some())
                .unwrap_or(false)
        {
            return Err(
                "Kiro V3 standalone hooks require isolation; use TOKAGE_KIRO_ENGINE=v2".into(),
            );
        }
    }
    static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let name = format!(
        "tokage-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos(),
        SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let dir = home.join(".kiro/agents");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{name}.json"));
    let config = agent_config(req, &cwd, &name, &engine)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    use std::io::Write;
    let mut file = options.open(&path).map_err(|e| e.to_string())?;
    let runtime = Runtime { path };
    file.write_all(config.to_string().as_bytes())
        .map_err(|e| e.to_string())?;
    if req.resume.is_none() {
        args.insert(1, format!("--{engine}"));
    }
    args.splice(1..1, ["--agent".into(), name]);
    Ok(runtime)
}

fn agent_config(req: &ChatRequest, cwd: &Path, name: &str, engine: &str) -> Result<Value, String> {
    let readonly = req.effective_role().is_read_only();
    let root = cwd.to_string_lossy();
    let mut scopes: Vec<String> = if req.write_scope.is_empty() {
        vec![format!("{root}/**")]
    } else {
        req.write_scope
            .iter()
            .map(|p| {
                if Path::new(p).is_absolute() || p.split(['/', '\\']).any(|part| part == "..") {
                    return Err("Kiro write_scope must be relative to the workspace".to_string());
                }
                Ok(format!("{root}/{p}"))
            })
            .collect::<Result<_, _>>()?
    };
    // Tokage normalizes a declared file to `file/**`; include that file itself
    // for engines whose filesystem glob requires a slash before `**`.
    let roots: Vec<_> = scopes
        .iter()
        .filter_map(|p| p.strip_suffix("/**").map(str::to_string))
        .collect();
    scopes.extend(roots);
    let mut config = json!({"name":name,"description":"Tokage controlled file tools",
        "prompt":"Use only the exposed file tools. Never delegate, execute shell commands, or change agent configuration. Tokage runs validation commands separately.",
        "tools":if readonly { vec!["read","glob","grep"] } else { vec!["read","glob","grep","write"] },
        "mcpServers":{},"includeMcpJson":false,"includePowers":false,"resources":[]});
    if engine == "v2" {
        config["allowedTools"] = json!(["read", "glob", "grep"]);
        config["toolsSettings"] =
            json!({"write":{"allowedPaths":if readonly { vec![] } else { scopes.clone() }}});
        config["hooks"] = json!({});
    } else {
        config["hooks"] = json!([]);
        let mut rules = vec![
            json!({"capability":"fs_read","match":[format!("{root}/**"),root.as_ref()],"effect":"allow"}),
            json!({"capability":"shell","match":["*"],"effect":"deny"}),
            json!({"capability":"subagent","match":["*"],"effect":"deny"}),
            json!({"capability":"mcp","match":["*"],"effect":"deny"}),
        ];
        if readonly {
            rules.push(json!({"capability":"fs_write","match":["**"],"effect":"deny"}));
        } else {
            rules.push(json!({"capability":"fs_write","match":scopes,"effect":"allow"}));
            rules.push(
                json!({"capability":"fs_write","match":["**"],"exclude":scopes,"effect":"deny"}),
            );
        }
        config["permissions"] = json!({"rules":rules});
    }
    Ok(config)
}

#[derive(Default)]
pub struct Kiro {
    engine: Option<String>,
    thread: Option<String>,
    credits: Option<f64>,
    completed: bool,
    failed: bool,
    streamed: bool,
    tools: HashMap<String, Value>,
    ended: HashSet<String>,
}
impl Kiro {
    fn metering(&mut self, rows: &Value, key: &str) {
        if let Some(rows) = rows.as_array() {
            let values: Vec<_> = rows
                .iter()
                .filter(|r| r["unit"] == "credit")
                .filter_map(|r| r[key].as_f64().filter(|v| v.is_finite() && *v >= 0.0))
                .collect();
            // These frames summarize the whole current turn, not additive snapshots.
            if !values.is_empty() {
                self.credits = Some(values.iter().sum());
            }
        }
    }
}
impl Parser for Kiro {
    fn line(&mut self, v: &Value, out: &mut Out) {
        let data = &v["data"];
        match v["type"].as_str().unwrap_or("") {
            "runStarted" => {
                self.engine = s(data, "engine");
                if !matches!(self.engine.as_deref(), Some("v2" | "v3")) {
                    self.failed = true;
                    out.fail("Unsupported Kiro engine");
                }
            }
            "metadata" => self.metering(&data["meteringUsage"], "value"),
            "sessionUpdate" => {
                let u = &data["update"];
                match u["sessionUpdate"].as_str().unwrap_or("") {
                    "agent_message_chunk" | "agent_thought_chunk" => {
                        if let Some(text) = s(&u["content"], "text") {
                            if u["sessionUpdate"] == "agent_thought_chunk" {
                                out.reasoning(&text);
                            } else {
                                self.streamed = true;
                                out.text(&text);
                            }
                        }
                    }
                    "tool_call" | "tool_call_update" => {
                        let Some(id) = s(u, "toolCallId") else {
                            return;
                        };
                        if !self.tools.contains_key(&id) {
                            let kind = match u["kind"].as_str().unwrap_or("") {
                                "read" => ToolKind::Read,
                                "edit" | "delete" => ToolKind::Edit,
                                "search" => ToolKind::Search,
                                "execute" => ToolKind::Command,
                                "fetch" => ToolKind::Web,
                                _ => ToolKind::Other,
                            };
                            out.tool_start(
                                &id,
                                kind,
                                s(u, "title").unwrap_or_else(|| "Kiro tool".into()),
                                None,
                            );
                            self.tools.insert(id.clone(), u.clone());
                        }
                        let state = u["status"].as_str().unwrap_or("");
                        if matches!(state, "completed" | "failed") && self.ended.insert(id.clone())
                        {
                            let ok = state == "completed";
                            let output = u.get("rawOutput").map(Value::to_string);
                            out.tool_end(&id, Some(ok), output);
                            let original = &self.tools[&id];
                            let kind = u["kind"].as_str().or_else(|| original["kind"].as_str());
                            if ok && matches!(kind, Some("edit" | "delete")) {
                                let locations = u["locations"]
                                    .as_array()
                                    .or_else(|| original["locations"].as_array());
                                if let Some(locations) = locations {
                                    for loc in locations {
                                        if let Some(path) = s(loc, "path") {
                                            out.file(FileTouch {
                                                tool_id: Some(id.clone()),
                                                path,
                                                before: None,
                                                after: None,
                                                deleted: kind == Some("delete"),
                                            });
                                        }
                                    }
                                } else if let Some(path) = s(&u["rawInput"], "path")
                                    .or_else(|| s(&original["rawInput"], "path"))
                                {
                                    out.file(FileTouch {
                                        tool_id: Some(id.clone()),
                                        path,
                                        before: None,
                                        after: None,
                                        deleted: kind == Some("delete"),
                                    });
                                }
                            }
                        }
                    }
                    "config_option_update" => {
                        if let Some(options) = u["configOptions"].as_array() {
                            for option in options {
                                if option["id"] == "model" {
                                    if let Some(name) = s(option, "currentValue") {
                                        out.emit(EventKind::Model { name });
                                    }
                                }
                            }
                        }
                    }
                    "session_info_update" => {
                        let meta = &u["_meta"]["kiro"];
                        if meta["kind"] == "turn_completion" {
                            self.metering(&meta["promptTurnSummaries"], "usage");
                        }
                    }
                    _ => {}
                }
            }
            "runFinished" => {
                if data["status"] == "success"
                    && data["stopReason"] == "end_turn"
                    && data["finalTextTruncated"] != true
                {
                    self.completed = true;
                    if !self.streamed {
                        if let Some(text) = s(data, "finalText") {
                            out.begin_text();
                            out.text(&text);
                        }
                    }
                } else {
                    self.failed = true;
                    out.fail(format!("Kiro run did not complete: {}", data));
                }
            }
            "error" | "runError" => {
                self.failed = true;
                out.fail(s(data, "message").unwrap_or_else(|| data.to_string()));
            }
            _ => {}
        }
        if let Some(id) = s(data, "sessionId") {
            if self.thread.as_ref() != Some(&id) {
                let engine = self
                    .engine
                    .as_deref()
                    .unwrap_or(if id.starts_with("sess_") { "v3" } else { "v2" });
                out.emit(EventKind::Thread {
                    id: format!("kiro:{engine}:{id}"),
                });
                self.thread = Some(id);
            }
        }
    }
    fn finish(&mut self, out: &mut Out) {
        if self.failed {
            return;
        }
        if !self.completed || self.thread.is_none() {
            out.fail("Kiro exited without a completed response");
            return;
        }
        let Some(credits) = self.credits else {
            out.fail("Kiro completed without credit usage");
            return;
        };
        out.emit(EventKind::Done {
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            api_calls: None,
            context_tokens: None,
            cost_usd: None,
            premium_requests: None,
            credits: Some(credits),
            source: tokage_core::UsageSource::Structured,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::super::{testing::*, Ev};
    use super::*;

    #[test]
    fn v3_tool_updates_can_omit_kind_and_locations() {
        let mut parser = Kiro::default();
        let mut out = Out::default();
        parser.line(&json!({"type":"sessionUpdate","data":{"sessionId":"sess_fixture","update":{"sessionUpdate":"tool_call","toolCallId":"edit-1","kind":"edit","title":"Replace in File","rawInput":{"path":"m.py"}}}}),&mut out);
        parser.line(&json!({"type":"sessionUpdate","data":{"sessionId":"sess_fixture","update":{"sessionUpdate":"tool_call_update","toolCallId":"edit-1","status":"completed","rawOutput":"Replaced text in m.py"}}}),&mut out);
        assert_eq!(
            out.evs
                .iter()
                .filter(|e| matches!(e,Ev::File(f) if f.path=="m.py"))
                .count(),
            1
        );
        assert_eq!(
            out.evs
                .iter()
                .filter(|e| matches!(e, Ev::Emit(EventKind::ToolEnd { ok: Some(true), .. })))
                .count(),
            1
        );
    }

    #[test]
    fn real_v2_v3_contracts_preserve_credits_without_inventing_tokens() {
        for (sample, engine) in [
            ("kiro-v2.jsonl", "v2"),
            ("kiro-v3.jsonl", "v3"),
            ("kiro-edit.jsonl", "v2"),
        ] {
            let mut parser = Kiro::default();
            let events = replay(&mut parser, sample);
            assert_eq!(count(&events, |k| matches!(k, EventKind::Done { .. })), 1);
            assert_eq!(count(&events, |k| matches!(k, EventKind::Thread { .. })), 1);
            assert!(events.iter().any(|e|matches!(e,Ev::Emit(EventKind::Thread{id}) if id.starts_with(&format!("kiro:{engine}:")))));
            assert!(events.iter().any(|e|matches!(e,Ev::Emit(EventKind::Done{credits:Some(c),input_tokens:0,output_tokens:0,cost_usd:None,..}) if *c>0.0)));
            let text: String = events
                .iter()
                .filter_map(|e| {
                    if let Ev::Emit(EventKind::Text { text }) = e {
                        Some(text.as_str())
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(
                text.matches(if sample == "kiro-v3.jsonl" {
                    "TOKAGE_KIRO_V3_791"
                } else if sample == "kiro-edit.jsonl" {
                    "TOKAGE_KIRO_EDIT_OK"
                } else {
                    "TOKAGE_KIRO_791"
                })
                .count(),
                1
            );
        }
    }

    #[test]
    fn errors_refusal_truncation_missing_finish_and_missing_usage_fail_closed() {
        for end in [
            json!({}),
            json!({"status":"success","stopReason":"refusal"}),
            json!({"status":"success","stopReason":"end_turn","finalTextTruncated":true}),
        ] {
            let mut parser = Kiro::default();
            let mut out = Out::default();
            parser.line(
                &json!({"type":"runStarted","data":{"engine":"v2"}}),
                &mut out,
            );
            parser.line(&json!({"type":"metadata","data":{"sessionId":"s","meteringUsage":[{"unit":"credit","value":0.1}]}}),&mut out);
            parser.line(&json!({"type":"runFinished","data":end}), &mut out);
            parser.finish(&mut out);
            assert!(!out
                .evs
                .iter()
                .any(|e| matches!(e, Ev::Emit(EventKind::Done { .. }))));
            assert!(out.evs.iter().any(|e| matches!(e, Ev::Failed(_))));
        }
        let mut parser = Kiro::default();
        let mut out = Out::default();
        parser.line(&json!({"type":"runFinished","data":{"sessionId":"s","status":"success","stopReason":"end_turn"}}),&mut out);
        parser.finish(&mut out);
        assert!(out
            .evs
            .iter()
            .any(|e| matches!(e,Ev::Failed(s) if s.contains("credit usage"))));
    }

    #[test]
    fn role_profiles_exclude_shell_delegation_mcp_and_reject_escaping_scope() {
        let mut req = request(crate::providers::ProviderId::KiroCli);
        for engine in ["v2", "v3"] {
            req.role = Some(super::super::Role::Planner);
            let config = agent_config(&req, Path::new("/work"), "test", engine).unwrap();
            assert!(!config["tools"]
                .as_array()
                .unwrap()
                .contains(&json!("write")));
            assert_eq!(config["includeMcpJson"], false);
            req.role = Some(super::super::Role::Executor);
            req.write_scope = vec!["src/**".into()];
            let config = agent_config(&req, Path::new("/work"), "test", engine).unwrap();
            assert!(config["tools"]
                .as_array()
                .unwrap()
                .contains(&json!("write")));
            req.write_scope = vec!["m.py/**".into()];
            let config = agent_config(&req, Path::new("/work"), "test", engine).unwrap();
            assert!(config.to_string().contains("\"/work/m.py\""));
            req.write_scope = vec!["../escape".into()];
            assert!(agent_config(&req, Path::new("/work"), "test", engine).is_err());
            req.write_scope.clear();
        }
        assert!(session("../../credentials").is_err());
        assert_eq!(session("kiro:v3:sess_123").unwrap(), ("v3", "sess_123"));
    }

    #[test]
    fn credentials_are_provider_scoped_and_metering_snapshots_are_not_double_counted() {
        let env = ShellEnv {
            vars: [
                ("KIRO_API_KEY".into(), "fixture-kiro".into()),
                ("OPENAI_API_KEY".into(), "fixture-other".into()),
            ]
            .into(),
        };
        let child =
            super::super::child_env::assemble(crate::providers::ProviderId::KiroCli, &env, &[]);
        assert!(child.contains_key("KIRO_API_KEY"));
        assert!(!child.contains_key("OPENAI_API_KEY"));
        let mut parser = Kiro::default();
        for _ in 0..2 {
            parser.metering(
                &json!([{"unit":"credit","value":0.2},{"unit":"credit","value":0.1}]),
                "value",
            );
        }
        assert!((parser.credits.unwrap() - 0.3).abs() < 1e-9);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cancel_timeout_and_nonzero_exit_cannot_publish_done() {
        use super::super::*;
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("kiro-fixture");
        let env = ShellEnv {
            vars: [
                ("HOME".into(), dir.path().to_string_lossy().into_owned()),
                ("PATH".into(), "/usr/bin:/bin".into()),
            ]
            .into(),
        };
        let mut req = request(crate::providers::ProviderId::KiroCli);
        req.cwd = dir.path().display().to_string();
        req.role = Some(Role::Planner);
        req.timeout_tier = Some(TimeoutTier::Agent);
        req.idle_timeout_secs = Some(1);
        req.hard_timeout_secs = Some(2);
        for mode in ["cancel", "timeout", "hard", "exit"] {
            let script = if mode == "exit" {
                format!(
                    "#!/bin/sh\ncat <<'EOF'\n{}\nEOF\nexit 1\n",
                    include_str!("../../tests/fixtures/kiro-v2.jsonl")
                )
            } else if mode == "hard" {
                "#!/bin/sh\nwhile true; do printf '%s\\n' '{\"type\":\"metadata\",\"data\":{\"sessionId\":\"heartbeat\"}}'; /bin/sleep 0.1; done\n".into()
            } else {
                "#!/bin/sh\nprintf '%s\\n' '{\"type\":\"runStarted\",\"data\":{\"engine\":\"v2\"}}'\nexec /bin/sleep 5\n".into()
            };
            std::fs::write(&exe, script).unwrap();
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o700)).unwrap();
            let events = std::sync::Mutex::new(vec![]);
            let emit = |e| events.lock().unwrap().push(e);
            let (tx, rx) = tokio::sync::watch::channel(false);
            let launch = super::launch(&req, "fixture".into());
            let run = run_cli(&exe, &req, launch, &env, None, None, &emit, Some(rx));
            let trigger = async {
                if mode == "cancel" {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    let _ = tx.send(true);
                }
            };
            let (result, _) = tokio::join!(run, trigger);
            let error = result.unwrap_err();
            if mode == "cancel" {
                assert_eq!(error, TURN_CANCELLED);
            }
            if mode == "hard" {
                assert!(error.contains("hard timeout"), "{error}");
            }
            if mode == "timeout" {
                assert!(error.contains("idle timeout"), "{error}");
            }
            assert!(!events
                .lock()
                .unwrap()
                .iter()
                .any(|e| matches!(e, EventKind::Done { .. })));
            assert_eq!(
                std::fs::read_dir(dir.path().join(".kiro/agents"))
                    .unwrap()
                    .count(),
                0
            );
        }
    }
}
