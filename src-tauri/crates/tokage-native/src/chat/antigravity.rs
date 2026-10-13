//! Google Antigravity CLI: `agy -p … --output-format stream-json`.
//! Recorded sample: tests/fixtures/antigravity-native.jsonl (CLI 1.3.4).
//! The older synthetic antigravity.jsonl is retained only for legacy parser regression.
//!
//! Antigravity CLI outputs structured events including deltas for reasoning and text,
//! tool start and completion, denials, quota signals, and turn usage summary.

use super::{rel, s, ChatRequest, EventKind, FileTouch, Launch, Out, Parser, ToolKind};
use crate::providers::{trust_args, ProviderId};

pub fn launch(req: &ChatRequest, prompt: String) -> Launch {
    let mut args: Vec<String> = vec![
        "-p".into(),
        prompt,
        "--output-format".into(),
        "stream-json".into(),
        "--model".into(),
        req.model.clone(),
    ];
    let role = req.effective_role();
    args.extend(["--mode".into(), if role.is_read_only() { "plan" } else { "accept-edits" }.into()]);
    if req.auth_mode == Some(tokage_core::auth::AuthMode::FullAuto) && !role.is_read_only() && role != tokage_core::Role::Tester {
        args.push("--dangerously-skip-permissions".into());
    }
    if let Some(e) = &req.effort {
        args.extend(["--effort".into(), e.clone()]);
    }
    if let Some(id) = &req.resume {
        args.extend(["--conversation".into(), id.clone()]);
    }

    args.extend(trust_args(ProviderId::AntigravityCli, &req.cwd));
    Launch {
        args,
        stdin: None,
        env_remove: vec![],
        parser: Box::new(Antigravity {
            cwd: req.cwd.clone(),
            ..Default::default()
        }),
    }
}

#[derive(Default)]
pub struct Antigravity {
    cwd: String,
    model: Option<String>,
    tools: std::collections::HashMap<String, (String, serde_json::Value)>,
    usage: Option<tokage_core::event::TurnUsage>,
    response: String,
    started_steps: std::collections::HashSet<u64>,
    finished_steps: std::collections::HashSet<u64>,
}

impl Parser for Antigravity {
    fn line(&mut self, v: &serde_json::Value, out: &mut Out) {
        if let Some(event) = v["event"].as_str() {
            self.native_event(event, v, out);
            return;
        }
        let typ = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match typ {
            "system" | "thread" => {
                if let Some(id) = s(v, "session_id").or_else(|| s(v, "id")) {
                    out.emit(EventKind::Thread { id });
                }
                if let Some(model) = s(v, "model") {
                    self.model = Some(model.clone());
                    out.emit(EventKind::Model { name: model });
                }
            }
            "reasoning_delta" | "reasoning" => {
                if let Some(text) = s(v, "text").or_else(|| s(v, "content")) {
                    out.reasoning(&text);
                }
            }
            "content_delta" | "text_delta" => {
                if let Some(text) = s(v, "text").or_else(|| s(v, "content")) {
                    out.begin_text();
                    out.text(&text);
                }
            }
            "tool_start" | "tool_use" => {
                let id = s(v, "id").unwrap_or_default();
                let name = s(v, "name").unwrap_or_default();
                let input = v.get("input").cloned().unwrap_or(serde_json::Value::Null);
                self.tools.insert(id.clone(), (name.clone(), input.clone()));

                let path_arg = |k: &str| s(&input, k).map(|p| rel(&self.cwd, &p)).unwrap_or_default();
                let (kind, title, detail) = match name.as_str() {
                    "bash" | "shell" | "terminal" => (
                        ToolKind::Command,
                        s(&input, "command").unwrap_or_default(),
                        s(&input, "description"),
                    ),
                    "read_file" | "read" | "view" => (ToolKind::Read, path_arg("path"), None),
                    "write_file" | "edit_file" | "write" | "edit" | "replace_file_content" => {
                        (ToolKind::Edit, path_arg("path"), None)
                    }
                    "search" | "grep" | "glob" => (
                        ToolKind::Search,
                        s(&input, "pattern").or_else(|| s(&input, "query")).unwrap_or_default(),
                        None,
                    ),
                    "web_fetch" | "fetch" => (ToolKind::Web, s(&input, "url").unwrap_or_default(), None),
                    _ => (ToolKind::Other, name, None),
                };
                out.tool_start(&id, kind, title, detail);
            }
            "tool_end" | "tool_result" => {
                let id = s(v, "id").unwrap_or_default();
                let ok = v.get("ok").and_then(|b| b.as_bool()).or_else(|| {
                    v.get("is_error").and_then(|b| b.as_bool()).map(|err| !err)
                });
                let output = s(v, "output");

                if let Some((name, input)) = self.tools.get(&id) {
                    if matches!(name.as_str(), "write_file" | "edit_file" | "write" | "edit" | "replace_file_content") {
                        if let Some(p) = s(input, "path") {
                            out.file(FileTouch {
                                tool_id: Some(id.clone()),
                                path: rel(&self.cwd, &p),
                                before: None,
                                after: None,
                                deleted: false,
                            });
                        }
                    }
                }
                out.tool_end(&id, ok, output);
            }
            "denial" => {
                let tool_id = s(v, "tool_id");
                let command = s(v, "command").unwrap_or_default();
                let reason = s(v, "reason").unwrap_or_else(|| "Denied by Antigravity policy".into());
                out.denial(tool_id, command, reason);
            }
            "quota" => {
                let raw = s(v, "message").unwrap_or_else(|| "Quota limit exceeded".into());
                let reset_at = s(v, "reset_at");
                let retrying = v.get("retrying").and_then(|b| b.as_bool()).unwrap_or(false);
                out.quota(raw, reset_at, retrying);
            }
            "usage" => {
                let inp = v.get("input_tokens").and_then(|x| x.as_u64()).unwrap_or(0);
                let out_tok = v.get("output_tokens").and_then(|x| x.as_u64()).unwrap_or(0);
                let cr = v.get("cache_read_tokens").and_then(|x| x.as_u64()).unwrap_or(0);
                let cw = v.get("cache_write_tokens").and_then(|x| x.as_u64()).unwrap_or(0);
                let calls = v.get("api_calls").and_then(|x| x.as_u64()).map(|n| n as u32);
                let ctx = v.get("context_tokens").and_then(|x| x.as_u64());
                let cost = v.get("cost_usd").or_else(|| v.get("cost")).and_then(|x| x.as_f64());

                self.usage = Some(tokage_core::event::TurnUsage {
                    input_tokens: inp,
                    output_tokens: out_tok,
                    cache_read_tokens: cr,
                    cache_write_tokens: cw,
                    api_calls: calls,
                    context_tokens: ctx,
                    cost_usd: cost,
                    premium_requests: None,
                    source: tokage_core::UsageSource::Structured,
                });
            }
            "done" => {
                let u = self.usage.clone().unwrap_or(tokage_core::event::TurnUsage {
                    input_tokens: 0,
                    output_tokens: 0,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                    api_calls: None,
                    context_tokens: None,
                    cost_usd: None,
                    premium_requests: None,
                    source: tokage_core::UsageSource::Structured,
                });
                out.emit(EventKind::Done {
                    input_tokens: u.input_tokens,
                    output_tokens: u.output_tokens,
                    cache_read_tokens: u.cache_read_tokens,
                    cache_write_tokens: u.cache_write_tokens,
                    api_calls: u.api_calls,
                    context_tokens: u.context_tokens,
                    cost_usd: u.cost_usd,
                    premium_requests: u.premium_requests,
                    source: u.source,
                });
            }
            "error" => {
                let msg = s(v, "message").unwrap_or_else(|| "Antigravity CLI error".into());
                if let Some(qm) = super::quota::match_quota(Some(tokage_core::ProviderId::AntigravityCli), &msg) {
                    out.quota(qm.raw, qm.reset_at, qm.retrying);
                }
                out.fail(msg);
            }
            _ => {}
        }
    }
}

impl Antigravity {
    fn native_event(&mut self, event: &str, v: &serde_json::Value, out: &mut Out) {
        match event {
            "init" => {
                if let Some(id) = s(v, "conversation_id") { out.emit(EventKind::Thread { id }); }
                if let Some(model) = s(&v["init"], "model") { out.emit(EventKind::Model { name: model }); }
            }
            "step_update" => {
                let step = &v["step_update"];
                let index = step["step_index"].as_u64().unwrap_or(0);
                if step["step_type"] == "agent_response" {
                    if let Some(text) = step["text_delta"].as_str() {
                        if self.response.is_empty() { out.begin_text(); }
                        self.response.push_str(text);
                        out.text(text);
                    }
                } else if step["step_type"] == "tool" {
                    let info = &step["tool_info"];
                    let name = s(info, "name").or_else(|| s(step, "tool_name")).unwrap_or_default();
                    let args = &info["parameters"];
                    let id = format!("agy-step-{index}");
                    let path = s(args, "TargetFile").or_else(|| s(args, "AbsolutePath")).or_else(|| s(args, "FilePath")).or_else(|| s(args, "path"));
                    if self.started_steps.insert(index) {
                        let kind = match name.as_str() {
                            "run_command" => ToolKind::Command,
                            "write_to_file" | "replace_file_content" | "multi_replace_file_content" => ToolKind::Edit,
                            "view_file" | "sed_file" => ToolKind::Read,
                            "grep_search" | "find_by_name" | "list_dir" => ToolKind::Search,
                            _ => ToolKind::Other,
                        };
                        let title = s(args, "CommandLine").or_else(|| path.clone()).unwrap_or(name.clone());
                        out.tool_start(&id, kind, title, None);
                    }
                    if matches!(step["state"].as_str(), Some("DONE" | "ERROR")) && self.finished_steps.insert(index) {
                        let error = info.get("error").filter(|v| !v.is_null());
                        let ok = error.is_none() && step["state"] != "ERROR";
                        let output = s(info, "output").or_else(|| error.and_then(|v| s(v, "message")));
                        out.tool_end(&id, Some(ok), output.clone());
                        if let Some(error) = error {
                            let msg = s(error, "message").unwrap_or_else(|| error.to_string());
                            if msg.to_ascii_lowercase().contains("permission") || msg.to_ascii_lowercase().contains("denied") {
                                out.denial(Some(id.clone()), s(args, "CommandLine").unwrap_or_default(), msg);
                            }
                        }
                        if ok && matches!(name.as_str(), "write_to_file" | "replace_file_content" | "multi_replace_file_content") {
                            if let Some(path) = path { out.file(FileTouch { tool_id: Some(id), path, before: None, after: None, deleted: false }); }
                        }
                    }
                }
            }
            "result" => {
                let result = &v["result"];
                if result["status"] != "SUCCESS" {
                    let message = s(result, "error").unwrap_or_else(|| format!("Antigravity ended with {}", result["status"]));
                    if let Some(qm) = super::quota::match_quota(Some(ProviderId::AntigravityCli), &message) { out.quota(qm.raw, qm.reset_at, qm.retrying); }
                    out.fail(message);
                    return;
                }
                // Final response repeats the streamed deltas. Only recover text if none arrived.
                if self.response.is_empty() {
                    if let Some(response) = s(result, "response") { out.begin_text(); out.text(&response); }
                }
                let usage = &result["usage"];
                let n = |key: &str| usage[key].as_u64().unwrap_or(0);
                out.emit(EventKind::Done { input_tokens: n("input_tokens"), output_tokens: n("output_tokens"),
                    cache_read_tokens: n("cache_read_tokens"), cache_write_tokens: n("cache_write_tokens"),
                    api_calls: None, context_tokens: None, cost_usd: None, premium_requests: None,
                    source: tokage_core::UsageSource::Structured });
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_stream_deltas_are_not_repeated_by_final_result() {
        use crate::chat::{testing::*, Ev};
        let mut parser = super::Antigravity::default();
        let events = replay(&mut parser, "antigravity-native.jsonl");
        assert_eq!(text(&events), "TOKAGE_AGY_OK\n");
        assert_eq!(count(&events, |e| matches!(e, super::EventKind::Done { .. })), 1);
        assert!(events.iter().any(|e| matches!(e, Ev::Emit(super::EventKind::Thread { .. }))));
    }

    #[test]
    fn failed_terminal_states_never_emit_done() {
        use crate::chat::{Parser, Out, Ev};
        for status in ["ERROR", "WAITING", "CANCELED", "INTERRUPTED", "RUNNING", "INVALID"] {
            let mut parser = super::Antigravity::default();
            let mut out = Out::default();
            parser.line(&serde_json::json!({"event":"result","result":{"status":status,"error":"authentication required"}}), &mut out);
            assert!(out.evs.iter().any(|e| matches!(e, Ev::Failed(_))));
            assert!(!out.evs.iter().any(|e| matches!(e, Ev::Emit(super::EventKind::Done { .. }))));
        }
    }

    #[test]
    fn native_tools_are_deduplicated_and_permission_denials_are_reported() {
        use crate::chat::{Parser, Out, Ev};
        let mut parser = super::Antigravity::default();
        let mut out = Out::default();
        let step = serde_json::json!({"event":"step_update","step_update":{"step_index":4,"state":"DONE","step_type":"tool",
            "tool_info":{"name":"run_command","parameters":{"CommandLine":"rm victim.txt"},"error":{"type":"permission","message":"Permission denied"}}}});
        parser.line(&step, &mut out);
        parser.line(&step, &mut out);
        assert_eq!(out.evs.iter().filter(|e| matches!(e, Ev::Emit(super::EventKind::ToolStart { .. }))).count(), 1);
        assert_eq!(out.evs.iter().filter(|e| matches!(e, Ev::Denial { command, .. } if command == "rm victim.txt")).count(), 1);
        assert!(out.evs.iter().any(|e| matches!(e, Ev::Emit(super::EventKind::ToolEnd { ok: Some(false), .. }))));
    }

    use super::*;
    use crate::chat::testing::*;

    #[test]
    fn parses_fixture() {
        let mut p = Antigravity {
            cwd: "/work".into(),
            ..Default::default()
        };
        let evs = replay(&mut p, "antigravity.jsonl");
        assert_eq!(count(&evs, |k| matches!(k, EventKind::Thread { .. })), 1);
        assert_eq!(count(&evs, |k| matches!(k, EventKind::Model { .. })), 1);
        assert_eq!(count(&evs, |k| matches!(k, EventKind::Reasoning { .. })), 1);
        assert_eq!(count(&evs, |k| matches!(k, EventKind::Text { .. })), 3);
        assert_eq!(count(&evs, |k| matches!(k, EventKind::ToolStart { .. })), 2);
        assert_eq!(count(&evs, |k| matches!(k, EventKind::ToolEnd { .. })), 2);
        assert_eq!(count(&evs, |k| matches!(k, EventKind::Done { .. })), 1);
    }
}
