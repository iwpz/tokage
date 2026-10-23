//! Claude Code: `claude -p --output-format stream-json --verbose --include-partial-messages`.
//! Sample: tests/fixtures/claude.jsonl (Claude Code 2.1.288).
//!
//! With partial messages, text and thinking stream as `stream_event` deltas and each block is
//! repeated in an aggregated `assistant` message. Tool inputs are taken from the aggregate
//! (complete JSON); tool results come back in `user` messages, with `tool_use_result`
//! carrying the file's original content for edits.

use std::collections::HashMap;

use super::{rel, s, ChatRequest, EventKind, FileTouch, Launch, Out, Parser, ToolKind, READONLY_SHELL};
use crate::providers::{trust_args, ProviderId};

/// Set when Claude Code runs inside another Claude session; a child that sees them tries to
/// use the host's auth channel and reports "Not logged in".
pub(crate) const HOST_SESSION_VARS: &[&str] = &[
    "CLAUDECODE",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_PROVIDER_MANAGED_BY_HOST",
    "CLAUDE_CODE_HOST_AUTH_ENV_VAR",
    "CLAUDE_CODE_HOST_SESSION_ID",
    "CLAUDE_CODE_SDK_HAS_HOST_AUTH_REFRESH",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_AGENT_SDK_VERSION",
];

/// Tools a planning turn never gets.
const PLAN_DENIED: &[&str] = &["Edit", "MultiEdit", "Write", "NotebookEdit", "ExitPlanMode"];

pub fn launch(req: &ChatRequest, prompt: String) -> Launch {
    let mut args = vec!["-p".to_string(), "--output-format".to_string(),
        if req.output_schema.is_some() { "json".into() } else { "stream-json".into() }];
    if req.output_schema.is_none() {
        args.extend(["--verbose".into(), "--include-partial-messages".into()]);
    }
    args.extend(["--model".into(), req.model.clone()]);
    if let Some(schema) = &req.output_schema {
        // Claude validates the native result against this schema. Keep conversational
        // text separate; never extract a verdict from a Markdown explanation.
        args.extend(["--json-schema".into(), schema.to_string()]);
    }
    let role = req.effective_role();
    if role.is_read_only() {
        // Read-only role (Planner or Reviewer): not Claude's own plan mode (which writes to ~/.claude/plans/).
        // Edits and non-read-only commands are prohibited (R7).
        args.extend(["--permission-mode".into(), "default".into()]);
        args.push("--allowedTools".into());
        args.extend(["Read", "Grep", "Glob"].iter().map(|t| t.to_string()));
        args.extend(READONLY_SHELL.iter().map(|c| format!("Bash({c}:*)")));
        args.push("--disallowedTools".into());
        args.extend(PLAN_DENIED.iter().map(|t| t.to_string()));
    } else {
        args.extend(["--permission-mode".into(), "acceptEdits".into()]);
        args.push("--allowedTools".into());
        let allowed: Vec<String> = if req.auth_mode == Some(tokage_core::auth::AuthMode::FullAuto) && role != tokage_core::Role::Tester {
            vec!["Bash".into()]
        } else {
            let mut a: Vec<String> = READONLY_SHELL.iter().map(|c| format!("Bash({c}:*)")).collect();
            for pattern in &req.session_allowed {
                if !pattern.trim().is_empty()
                    && !pattern.chars().any(|c| matches!(c, '*' | '?' | '(' | ')' | '[' | ']' | '\\' | '\n' | '\r')) {
                    a.push(format!("Bash({pattern})"));
                }
            }
            a
        };
        args.extend(allowed);
    }
    if let Some(e) = &req.effort {
        args.extend(["--effort".into(), e.clone()]);
    }
    if let Some(id) = &req.resume {
        args.extend(["--resume".into(), id.clone()]);
        if req.fork {
            args.push("--fork-session".into());
        }
    }
    args.extend(trust_args(ProviderId::ClaudeCode, &req.cwd));
    Launch {
        args,
        stdin: Some(prompt),
        env_remove: HOST_SESSION_VARS.iter().map(|s| s.to_string()).collect(),
        parser: Box::new(Claude {
            cwd: req.cwd.clone(),
            expect_structured: req.output_schema.is_some(),
            ..Default::default()
        }),
    }
}

#[derive(Default)]
pub struct Claude {
    cwd: String,
    /// Deltas arrived, so aggregated text/thinking blocks are repeats.
    streamed: bool,
    expect_structured: bool,
    structured_seen: bool,
    /// Tool name and optional command per tool_use id.
    tools: HashMap<String, (String, Option<String>)>,
    /// Accumulated steps and outputs of subagents keyed by parent tool id.
    subagent_logs: HashMap<String, Vec<String>>,
    last_context_tokens: Option<u64>,
}

impl Parser for Claude {
    fn line(&mut self, v: &serde_json::Value, out: &mut Out) {
        match v.get("type").and_then(|t| t.as_str()).unwrap_or("") {
            "system" if v["subtype"] == "init" => {
                if let Some(id) = s(v, "session_id") {
                    out.emit(EventKind::Thread { id });
                }
                if let Some(name) = s(v, "model") {
                    out.emit(EventKind::Model { name });
                }
            }
            "stream_event" => self.stream(&v["event"], out),
            "assistant" => {
                // If this is subagent traffic, record its steps and findings under the parent tool id
                if let Some(parent_id) = s(v, "parent_tool_use_id").filter(|s| !s.is_empty()) {
                    let blocks = v["message"]["content"].as_array().cloned().unwrap_or_default();
                    let buf = self.subagent_logs.entry(parent_id).or_default();
                    for b in blocks {
                        match b["type"].as_str().unwrap_or("") {
                            "text" => {
                                let t = b["text"].as_str().unwrap_or("").trim();
                                if !t.is_empty() {
                                    buf.push(format!("💬 [Sub-agent analysis] {t}"));
                                }
                            }
                            "thinking" => {
                                let t = b["thinking"].as_str().unwrap_or("").trim();
                                if !t.is_empty() {
                                    buf.push(format!("💭 [Sub-agent thinking] {t}"));
                                }
                            }
                            "tool_use" => {
                                let tool_name = s(&b, "name").unwrap_or_default();
                                let input = &b["input"];
                                match tool_name.as_str() {
                                    "Read" => {
                                        let p = s(input, "file_path").unwrap_or_default();
                                        buf.push(format!("📖 [Read file] {p}"));
                                    }
                                    "Grep" | "Glob" => {
                                        let pat = s(input, "pattern").unwrap_or_default();
                                        buf.push(format!("🔍 [Code search] {pat}"));
                                    }
                                    "Bash" => {
                                        let cmd = s(input, "command").unwrap_or_default();
                                        buf.push(format!("⚡ [Run command] {cmd}"));
                                    }
                                    "WebSearch" | "WebFetch" => {
                                        let q = s(input, "query").or_else(|| s(input, "url")).unwrap_or_default();
                                        buf.push(format!("🌐 [Web query] {q}"));
                                    }
                                    other => {
                                        buf.push(format!("🔧 [Tool call: {other}]"));
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    return;
                }
                if let Some(u) = v["message"].get("usage") {
                    let inp = u["input_tokens"].as_u64().unwrap_or(0);
                    let cc = u["cache_creation_input_tokens"].as_u64().unwrap_or(0);
                    let cr = u["cache_read_input_tokens"].as_u64().unwrap_or(0);
                    let total = inp + cc + cr;
                    if total > 0 {
                        self.last_context_tokens = Some(total);
                    }
                }
                let blocks = v["message"]["content"].as_array().cloned().unwrap_or_default();
                for b in blocks {
                    match b["type"].as_str().unwrap_or("") {
                        "text" if !self.streamed => {
                            let text = b["text"].as_str().unwrap_or("");
                            if let Some(qm) = super::quota::match_quota(Some(tokage_core::ProviderId::ClaudeCode), text) {
                                out.quota(qm.raw, qm.reset_at, qm.retrying);
                            }
                            out.begin_text();
                            out.text(text);
                        }
                        "thinking" if !self.streamed => out.reasoning(b["thinking"].as_str().unwrap_or("")),
                        "tool_use" => self.tool_use(&b, out),
                        _ => {}
                    }
                }
            }
            "user" => {
                // If this is subagent tool result, record in subagent logs
                if let Some(parent_id) = s(v, "parent_tool_use_id").filter(|s| !s.is_empty()) {
                    let blocks = v["message"]["content"].as_array().cloned().unwrap_or_default();
                    let buf = self.subagent_logs.entry(parent_id).or_default();
                    for b in blocks.iter().filter(|b| b["type"] == "tool_result") {
                        let text = result_text(&b["content"]);
                        let trimmed = text.trim();
                        if !trimmed.is_empty() {
                            let preview = if trimmed.len() > 300 {
                                format!("{}...", &trimmed[..300])
                            } else {
                                trimmed.to_string()
                            };
                            buf.push(format!("   ↳ Result: {preview}"));
                        }
                    }
                    return;
                }
                let blocks = v["message"]["content"].as_array().cloned().unwrap_or_default();
                for b in blocks.iter().filter(|b| b["type"] == "tool_result") {
                    let id = s(b, "tool_use_id").unwrap_or_default();
                    let ok = !b["is_error"].as_bool().unwrap_or(false);
                    let text = result_text(&b["content"]);

                    // If this tool was an Agent or has collected subagent logs, assemble rich report
                    let sub_logs = self.subagent_logs.remove(&id);
                    let final_output = match sub_logs {
                        Some(logs) if !logs.is_empty() => {
                            let logs_text = logs.join("\n");
                            if text.trim().is_empty() {
                                logs_text
                            } else {
                                format!("{logs_text}\n\n📋 [Sub-agent final deliverable]:\n{text}")
                            }
                        }
                        _ => text.clone(),
                    };

                    out.tool_end(&id, Some(ok), Some(final_output));
                    if ok {
                        self.file_result(&id, &v["tool_use_result"], out);
                    } else if let Some((name, cmd)) = self.tools.get(&id) {
                        let lower = text.to_lowercase();
                        if lower.contains("not allowed")
                            || lower.contains("permission")
                            || lower.contains("disallowed")
                            || lower.contains("rejected")
                            || lower.contains("requires permission")
                            || lower.contains("operation not permitted")
                            || lower.contains("permission denied")
                            || lower.contains("access denied")
                            || lower.contains("eperm")
                            || lower.contains("eacces")
                        {
                            let cmd_str = cmd.clone().unwrap_or_else(|| name.clone());
                            out.denial(Some(id.clone()), cmd_str, "disallowed_tool".into());
                        }
                    }
                }
            }
            "error" => {
                let err_text = s(v, "error")
                    .or_else(|| s(v, "message"))
                    .unwrap_or_else(|| "Claude Code error".into());
                if let Some(qm) = super::quota::match_quota(Some(tokage_core::ProviderId::ClaudeCode), &err_text) {
                    out.quota(qm.raw, qm.reset_at, qm.retrying);
                }
                out.fail(err_text);
            }
            "result" => {
                if v["is_error"].as_bool().unwrap_or(false) {
                    let err_text = s(v, "result").unwrap_or_else(|| "Claude Code reported an error".into());
                    if let Some(qm) = super::quota::match_quota(Some(tokage_core::ProviderId::ClaudeCode), &err_text) {
                        out.quota(qm.raw, qm.reset_at, qm.retrying);
                    }
                    out.fail(err_text);
                    return;
                }
                if self.expect_structured {
                    if v["subtype"] != "success" {
                        out.fail("Claude structured result did not report success");
                        return;
                    }
                    if self.structured_seen {
                        out.fail("Claude returned duplicate structured results");
                        return;
                    }
                    let Some(value) = v.get("structured_output").filter(|value| value.is_object()) else {
                        out.fail("Claude ended without the required structured output");
                        return;
                    };
                    self.structured_seen = true;
                    if let Some(id) = s(v, "session_id") { out.emit(EventKind::Thread { id }); }
                    let model = s(v, "model").or_else(|| v["modelUsage"].as_object()
                        .filter(|models| models.len() == 1).and_then(|models| models.keys().next().cloned()));
                    if let Some(name) = model { out.emit(EventKind::Model { name }); }
                    if let Some(text) = s(v, "result") { out.text(&text); }
                    out.emit(EventKind::StructuredOutput { json: value.to_string() });
                }
                let u = &v["usage"];
                let n = |k: &str| u[k].as_u64().unwrap_or(0);
                let api_calls = v["num_turns"].as_u64().map(|n| n as u32);
                let cost_usd = v["total_cost_usd"].as_f64();
                out.emit(EventKind::Done {
                    input_tokens: n("input_tokens"),
                    output_tokens: n("output_tokens"),
                    cache_read_tokens: n("cache_read_input_tokens"),
                    cache_write_tokens: n("cache_creation_input_tokens"),
                    api_calls,
                    context_tokens: self.last_context_tokens,
                    cost_usd,
                    premium_requests: None, credits: None,
                    source: tokage_core::UsageSource::Structured,
                });
            }
            _ => {}
        }
    }
}

impl Claude {
    fn stream(&mut self, e: &serde_json::Value, out: &mut Out) {
        match e["type"].as_str().unwrap_or("") {
            "content_block_start" if e["content_block"]["type"] == "text" => out.begin_text(),
            "content_block_delta" => {
                let d = &e["delta"];
                match d["type"].as_str().unwrap_or("") {
                    "text_delta" => {
                        self.streamed = true;
                        out.text(d["text"].as_str().unwrap_or(""));
                    }
                    "thinking_delta" => {
                        self.streamed = true;
                        out.reasoning(d["thinking"].as_str().unwrap_or(""));
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn tool_use(&mut self, b: &serde_json::Value, out: &mut Out) {
        let id = s(b, "id").unwrap_or_default();
        let name = s(b, "name").unwrap_or_default();
        let i = &b["input"];
        let path = |k: &str| s(i, k).map(|p| rel(&self.cwd, &p)).unwrap_or_default();
        let (kind, title, detail, cmd) = match name.as_str() {
            "Bash" => {
                let c = s(i, "command").unwrap_or_default();
                (ToolKind::Command, c.clone(), s(i, "description"), Some(c))
            }
            "Read" => (ToolKind::Read, path("file_path"), None, s(i, "file_path")),
            "Grep" | "Glob" => (ToolKind::Search, s(i, "pattern").unwrap_or_default(), s(i, "path"), None),
            "Edit" | "MultiEdit" | "Write" => (ToolKind::Edit, path("file_path"), None, s(i, "file_path")),
            "NotebookEdit" => (ToolKind::Edit, path("notebook_path"), None, s(i, "notebook_path")),
            "WebFetch" => (ToolKind::Web, s(i, "url").unwrap_or_default(), None, None),
            "WebSearch" => (ToolKind::Web, s(i, "query").unwrap_or_default(), None, None),
            "Task" | "Agent" => (ToolKind::Agent, s(i, "description").unwrap_or_default(), None, None),
            _ => (ToolKind::Other, name.clone(), None, None),
        };
        out.tool_start(&id, kind, title, detail);
        self.tools.insert(id, (name, cmd));
    }

    fn file_result(&self, id: &str, r: &serde_json::Value, out: &mut Out) {
        let edits = matches!(
            self.tools.get(id).map(|(n, _)| n.as_str()),
            Some("Edit" | "MultiEdit" | "Write" | "NotebookEdit")
        );
        let Some(path) = s(r, "filePath").filter(|_| edits) else {
            return;
        };
        let before = match r.get("originalFile") {
            Some(serde_json::Value::String(t)) => Some(Some(t.clone())),
            Some(serde_json::Value::Null) => Some(None),
            _ if r["type"] == "create" => Some(None),
            _ => None,
        };
        out.file(FileTouch {
            tool_id: Some(id.to_string()),
            path,
            before,
            // Edit results carry only the strings; Write carries the full new content.
            after: s(r, "content").filter(|_| r["type"] == "create" || r["type"] == "update"),
            deleted: false,
        });
    }
}

/// Tool result content is a string or a list of text blocks.
fn result_text(c: &serde_json::Value) -> String {
    match c {
        serde_json::Value::String(t) => t.clone(),
        serde_json::Value::Array(a) => a
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{testing::*, Ev};

    #[test]
    fn fixture() {
        let mut p = Claude {
            cwd: "/work".into(),
            ..Default::default()
        };
        let evs = replay(&mut p, "claude.jsonl");
        assert!(matches!(&evs[0], Ev::Emit(EventKind::Thread { .. })));
        let text = text(&evs);
        // Streamed once, not repeated by the aggregates.
        assert_eq!(text.matches("I'll start by reading m.py").count(), 1, "{text}");
        assert!(text.contains("\n\n## Summary"), "{text}");
        let titles: Vec<_> = evs
            .iter()
            .filter_map(|e| match e {
                Ev::Emit(EventKind::ToolStart { tool, title, .. }) => Some((*tool, title.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(
            titles,
            [
                (ToolKind::Read, "m.py".to_string()),
                (ToolKind::Edit, "m.py".to_string()),
                (ToolKind::Edit, "test_m.py".to_string())
            ]
        );
        let files: Vec<_> = evs
            .iter()
            .filter_map(|e| match e {
                Ev::File(f) => Some(f),
                _ => None,
            })
            .collect();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].before, Some(Some("def add(a,b):\n    return a-b\n".into())));
        assert_eq!(files[1].before, Some(None));
        assert!(files[1].after.as_deref().unwrap().contains("assert add(2, 3) == 5"));
        assert!(matches!(evs.last(), Some(Ev::Emit(EventKind::Done { cost_usd: Some(_), .. }))));
    }

    #[test]
    fn service_schema_uses_native_json_result_and_keeps_prose_separate() {
        let mut req = request(ProviderId::ClaudeCode);
        req.output_schema = Some(serde_json::json!({"type":"object"}));
        let launch = launch(&req, "review".into());
        assert!(launch.args.windows(2).any(|a| a == ["--output-format", "json"]));
        assert!(launch.args.windows(2).any(|a| a == ["--json-schema", r#"{"type":"object"}"#]));
        assert!(!launch.args.contains(&"--include-partial-messages".into()));
        let mut p=Claude { expect_structured:true, ..Default::default() };
        let mut out=Out::default();
        let value=serde_json::json!({"type":"result","subtype":"success","is_error":false,"result":"Prose claiming PASS is not the result", "session_id":"structured-thread", "modelUsage":{"claude-haiku-test":{}}, "structured_output":{"verdict":"FAIL","findings":["bug"]},"usage":{},"total_cost_usd":0.1});
        p.line(&value,&mut out);
        assert!(out.evs.iter().any(|e|matches!(e,Ev::Emit(EventKind::StructuredOutput{json}) if json.contains("FAIL"))));
        assert!(out.evs.iter().any(|e|matches!(e,Ev::Emit(EventKind::Model{name}) if name=="claude-haiku-test")));
        assert!(out.evs.iter().any(|e|matches!(e,Ev::Emit(EventKind::Thread{id}) if id=="structured-thread")));
        assert!(out.evs.iter().any(|e|matches!(e,Ev::Emit(EventKind::Done{..}))));
        out.evs.clear();p.line(&value,&mut out);
        assert!(out.evs.iter().any(|e|matches!(e,Ev::Failed(_))));
        assert!(!out.evs.iter().any(|e|matches!(e,Ev::Emit(EventKind::Done{..}))));
    }
    #[test]
    fn schema_cannot_be_injected_from_transport_input() {
        let req:ChatRequest=serde_json::from_value(serde_json::json!({"turnId":"t","cwd":"/","prompt":"p","mode":"direct","provider":"CLAUDE_CODE","model":"haiku","outputSchema":{"type":"object"}})).unwrap();
        assert!(req.output_schema.is_none());
    }
    #[test]
    fn schema_results_missing_native_output_or_reporting_errors_never_complete() {
        for value in [serde_json::json!({"type":"result","result":"{\"verdict\":\"PASS\",\"findings\":[]}","is_error":false}),
            serde_json::json!({"type":"result","is_error":true,"result":"schema failure","structured_output":{"verdict":"PASS","findings":[]}}),
            serde_json::json!({"type":"result","subtype":"error_max_structured_output_retries","is_error":false,"structured_output":{"verdict":"PASS","findings":[]}})] {
            let mut p=Claude{expect_structured:true,..Default::default()};let mut out=Out::default();p.line(&value,&mut out);
            assert!(out.evs.iter().any(|e|matches!(e,Ev::Failed(_))));
            assert!(!out.evs.iter().any(|e|matches!(e,Ev::Emit(EventKind::Done{..}|EventKind::StructuredOutput{..}))));
        }
    }
    #[test]
    fn not_logged_in_is_an_error() {
        let mut p = Claude::default();
        let mut out = Out::default();
        let v = serde_json::json!({"type":"result","subtype":"success","is_error":true,"result":"Not logged in · Please run /login"});
        p.line(&v, &mut out);
        assert_eq!(out.evs, [Ev::Failed("Not logged in · Please run /login".into())]);
    }

    #[test]
    fn fork_only_with_resume() {
        let mut req = crate::chat::testing::request(ProviderId::ClaudeCode);
        req.fork = true;
        assert!(!launch(&req, String::new()).args.contains(&"--fork-session".to_string()));
        req.resume = Some("abc".into());
        let args = launch(&req, String::new()).args;
        let i = args.iter().position(|a| a == "--resume").unwrap();
        assert_eq!(args[i + 1], "abc");
        assert_eq!(args[i + 2], "--fork-session");
    }

    #[test]
    fn denial_emitted_on_disallowed_bash() {
        let mut p = Claude::default();
        let mut out = Out::default();
        // 1. Tool use
        p.line(
            &serde_json::json!({
                "type": "assistant",
                "message": {
                    "content": [{
                        "type": "tool_use",
                        "id": "toolu_123",
                        "name": "Bash",
                        "input": {"command": "pytest tests/"}
                    }]
                }
            }),
            &mut out,
        );
        // 2. Tool result with denial
        p.line(
            &serde_json::json!({
                "type": "user",
                "message": {
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": "toolu_123",
                        "is_error": true,
                        "content": "Tool 'Bash' is not allowed"
                    }]
                }
            }),
            &mut out,
        );
        assert_eq!(
            out.evs.last(),
            Some(&Ev::Denial {
                tool_id: Some("toolu_123".into()),
                command: "pytest tests/".into(),
                reason: "disallowed_tool".into(),
            })
        );
    }
}
