//! Claude Code: `claude -p --output-format stream-json --verbose --include-partial-messages`.
//! Sample: tests/fixtures/claude.jsonl (Claude Code 2.1.288).
//!
//! With partial messages, text and thinking stream as `stream_event` deltas and each block is
//! repeated in an aggregated `assistant` message. Tool inputs are taken from the aggregate
//! (complete JSON); tool results come back in `user` messages, with `tool_use_result`
//! carrying the file's original content for edits.

use std::collections::HashMap;

use super::{rel, s, ChatRequest, EventKind, FileTouch, Launch, Out, Parser, ToolKind, TurnMode, READONLY_SHELL};
use crate::providers::{trust_args, ProviderId};

/// Set when Claude Code runs inside another Claude session; a child that sees them tries to
/// use the host's auth channel and reports "Not logged in".
const HOST_SESSION_VARS: &[&str] = &[
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

pub fn launch(req: &ChatRequest, prompt: String) -> Launch {
    let mut args: Vec<String> = [
        "-p",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--model",
    ]
    .iter()
    .map(|a| a.to_string())
    .collect();
    args.push(req.model.clone());
    match req.mode {
        TurnMode::Plan => args.extend(["--permission-mode".into(), "plan".into()]),
        TurnMode::Direct => {
            args.extend(["--permission-mode".into(), "acceptEdits".into()]);
            args.push("--allowedTools".into());
            args.extend(READONLY_SHELL.iter().map(|c| format!("Bash({c}:*)")));
        }
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
            ..Default::default()
        }),
    }
}

#[derive(Default)]
pub struct Claude {
    cwd: String,
    /// Deltas arrived, so aggregated text/thinking blocks are repeats.
    streamed: bool,
    /// Tool name per tool_use id.
    tools: HashMap<String, String>,
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
                // Subagent traffic carries a parent id; only the main thread is shown.
                if !v["parent_tool_use_id"].is_null() {
                    return;
                }
                let blocks = v["message"]["content"].as_array().cloned().unwrap_or_default();
                for b in blocks {
                    match b["type"].as_str().unwrap_or("") {
                        "text" if !self.streamed => {
                            out.begin_text();
                            out.text(b["text"].as_str().unwrap_or(""));
                        }
                        "thinking" if !self.streamed => out.reasoning(b["thinking"].as_str().unwrap_or("")),
                        "tool_use" => self.tool_use(&b, out),
                        _ => {}
                    }
                }
            }
            "user" => {
                if !v["parent_tool_use_id"].is_null() {
                    return;
                }
                let blocks = v["message"]["content"].as_array().cloned().unwrap_or_default();
                for b in blocks.iter().filter(|b| b["type"] == "tool_result") {
                    let id = s(b, "tool_use_id").unwrap_or_default();
                    let ok = !b["is_error"].as_bool().unwrap_or(false);
                    out.tool_end(&id, Some(ok), Some(result_text(&b["content"])));
                    if ok {
                        self.file_result(&id, &v["tool_use_result"], out);
                    }
                }
            }
            "result" => {
                if v["is_error"].as_bool().unwrap_or(false) {
                    out.fail(s(v, "result").unwrap_or_else(|| "Claude Code reported an error".into()));
                    return;
                }
                let u = &v["usage"];
                let n = |k: &str| u[k].as_u64().unwrap_or(0);
                out.emit(EventKind::Done {
                    input_tokens: n("input_tokens") + n("cache_creation_input_tokens") + n("cache_read_input_tokens"),
                    output_tokens: n("output_tokens"),
                    cost_usd: v["total_cost_usd"].as_f64(),
                    premium_requests: None,
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
        let (kind, title, detail) = match name.as_str() {
            "Bash" => (ToolKind::Command, s(i, "command").unwrap_or_default(), s(i, "description")),
            "Read" => (ToolKind::Read, path("file_path"), None),
            "Grep" | "Glob" => (ToolKind::Search, s(i, "pattern").unwrap_or_default(), s(i, "path")),
            "Edit" | "MultiEdit" | "Write" => (ToolKind::Edit, path("file_path"), None),
            "NotebookEdit" => (ToolKind::Edit, path("notebook_path"), None),
            "WebFetch" => (ToolKind::Web, s(i, "url").unwrap_or_default(), None),
            "WebSearch" => (ToolKind::Web, s(i, "query").unwrap_or_default(), None),
            "Task" | "Agent" => (ToolKind::Agent, s(i, "description").unwrap_or_default(), None),
            _ => (ToolKind::Other, name.clone(), None),
        };
        out.tool_start(&id, kind, title, detail);
        self.tools.insert(id, name);
    }

    fn file_result(&self, id: &str, r: &serde_json::Value, out: &mut Out) {
        let edits = matches!(
            self.tools.get(id).map(String::as_str),
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
}
