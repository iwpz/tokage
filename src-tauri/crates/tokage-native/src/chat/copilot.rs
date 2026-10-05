//! GitHub Copilot CLI: `copilot -p … --output-format json` (JSONL session events).
//! Sample: tests/fixtures/copilot.jsonl (Copilot CLI 1.0.91).
//!
//! Reasoning and reply text stream as `*_delta` events and are repeated whole afterwards.
//! Read tools run without asking; writes and shell commands need `--allow-tool`, so Direct
//! allows file writes plus read-only shell commands, and Plan denies both.

use super::{rel, s, ChatRequest, EventKind, FileTouch, Launch, Out, Parser, ToolKind, READONLY_SHELL};
use crate::providers::{trust_args, ProviderId};

pub fn launch(req: &ChatRequest, prompt: String) -> Launch {
    let mut args: Vec<String> = vec![
        "-p".into(),
        prompt,
        "--output-format".into(),
        "json".into(),
        "-s".into(),
        "--model".into(),
        req.model.clone(),
    ];
    let role = req.effective_role();
    if role.is_read_only() {
        args.push("--plan".into());
        args.extend(["--deny-tool".into(), "write".into(), "--deny-tool".into(), "shell".into()]);
    } else {
        args.extend(["--allow-tool".into(), "write".into()]);
        if req.auth_mode == Some(tokage_core::auth::AuthMode::FullAuto) && role != tokage_core::Role::Tester {
            args.extend(["--allow-tool".into(), "shell".into()]);
        } else {
            for c in READONLY_SHELL {
                args.extend(["--allow-tool".into(), format!("shell({c}:*)")]);
            }
        }
    }
    if let Some(e) = &req.effort {
        args.extend(["--reasoning-effort".into(), e.clone()]);
    }
    if let Some(t) = req.tier.as_ref().filter(|_| req.model == "auto") {
        args.extend(["--auto-tier".into(), t.clone()]);
    }
    if let Some(id) = &req.resume {
        args.extend(["--resume".into(), id.clone()]);
    }
    // P2.m5a: Instruct Copilot to strip all credential variables from shell & MCP tool environments
    let secret_vars = super::child_env::CREDENTIAL_ENV.join(",");
    args.push(format!("--secret-env-vars={secret_vars}"));

    args.extend(trust_args(ProviderId::CopilotCli, &req.cwd));
    Launch {
        args,
        stdin: None,
        env_remove: vec![],
        parser: Box::new(Copilot {
            cwd: req.cwd.clone(),
            ..Default::default()
        }),
    }
}

#[derive(Default)]
pub struct Copilot {
    cwd: String,
    model: Option<String>,
    /// Message ids whose text already streamed as deltas.
    streamed: std::collections::HashSet<String>,
    reasoning_streamed: std::collections::HashSet<String>,
    /// Tool name and arguments per call id, for the completion event.
    tools: std::collections::HashMap<String, (String, serde_json::Value)>,
    api_calls: Option<u32>,
}

impl Parser for Copilot {
    fn line(&mut self, v: &serde_json::Value, out: &mut Out) {
        let d = &v["data"];
        if let Some(tid) = d.get("turnId").and_then(|t| t.as_str()).and_then(|s| s.parse::<u32>().ok()) {
            self.api_calls = Some(self.api_calls.map_or(tid, |prev| prev.max(tid)));
        }
        match v["type"].as_str().unwrap_or("") {
            "session.auto_mode_resolved" => self.model(s(d, "chosenModel"), out),
            "assistant.reasoning_delta" => {
                if let Some(id) = s(d, "reasoningId") {
                    self.reasoning_streamed.insert(id);
                }
                out.reasoning(d["deltaContent"].as_str().unwrap_or(""));
            }
            "assistant.reasoning" => {
                let id = s(d, "reasoningId").unwrap_or_default();
                if !self.reasoning_streamed.contains(&id) {
                    out.reasoning(d["content"].as_str().unwrap_or(""));
                }
            }
            "assistant.message_start" => out.begin_text(),
            "assistant.message_delta" => {
                if let Some(id) = s(d, "messageId") {
                    self.streamed.insert(id);
                }
                out.text(d["deltaContent"].as_str().unwrap_or(""));
            }
            "assistant.message" => {
                self.model(s(d, "model"), out);
                let id = s(d, "messageId").unwrap_or_default();
                let text = d["content"].as_str().unwrap_or("");
                if let Some(qm) = super::quota::match_quota(Some(tokage_core::ProviderId::CopilotCli), text) {
                    out.quota(qm.raw, qm.reset_at, qm.retrying);
                }
                if !self.streamed.contains(&id) && !text.is_empty() {
                    out.begin_text();
                    out.text(text);
                }
            }
            "tool.execution_start" => self.start(d, out),
            "tool.execution_complete" => self.complete(d, out),
            "session.error" => {
                let msg = s(d, "message").unwrap_or_else(|| "Copilot error".into());
                if let Some(qm) = super::quota::match_quota(Some(tokage_core::ProviderId::CopilotCli), &msg) {
                    out.quota(qm.raw, qm.reset_at, qm.retrying);
                }
                out.fail(msg);
            }
            "result" => {
                if let Some(id) = s(v, "sessionId") {
                    out.emit(EventKind::Thread { id });
                }
                if v["exitCode"].as_i64().unwrap_or(0) != 0 {
                    out.fail(format!("Copilot exited with code {}", v["exitCode"]));
                    return;
                }
                out.emit(EventKind::Done {
                    // Copilot bills premium requests and doesn't report tokens.
                    input_tokens: 0,
                    output_tokens: 0,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                    api_calls: self.api_calls,
                    context_tokens: None,
                    cost_usd: None,
                    premium_requests: v["usage"]["premiumRequests"].as_f64(),
                    source: tokage_core::UsageSource::Structured,
                });
            }
            _ => {}
        }
    }
}

impl Copilot {
    fn model(&mut self, name: Option<String>, out: &mut Out) {
        if let Some(n) = name.filter(|n| self.model.as_ref() != Some(n)) {
            self.model = Some(n.clone());
            out.emit(EventKind::Model { name: n });
        }
    }

    fn start(&mut self, d: &serde_json::Value, out: &mut Out) {
        let id = s(d, "toolCallId").unwrap_or_default();
        let name = s(d, "toolName").unwrap_or_default();
        let a = d["arguments"].clone();
        let path = |k: &str| s(&a, k).map(|p| rel(&self.cwd, &p)).unwrap_or_default();
        let (kind, title, detail) = match name.as_str() {
            "bash" | "shell" | "powershell" => (ToolKind::Command, s(&a, "command").unwrap_or_default(), s(&a, "description")),
            "view" | "read" => (ToolKind::Read, path("path"), None),
            "glob" | "grep" | "rg" | "search" => (ToolKind::Search, s(&a, "pattern").unwrap_or_default(), None),
            "edit" | "create" | "write" | "str_replace_editor" => (ToolKind::Edit, path("path"), None),
            "apply_patch" => {
                let files: Vec<String> = patch_files(a.as_str().unwrap_or("")).into_iter().map(|(p, _)| rel(&self.cwd, &p)).collect();
                (ToolKind::Edit, files.join(", "), None)
            }
            "web_fetch" | "fetch" => (ToolKind::Web, s(&a, "url").unwrap_or_default(), None),
            "web_search" => (ToolKind::Web, s(&a, "query").unwrap_or_default(), None),
            "task" | "agent" => (ToolKind::Agent, s(&a, "description").unwrap_or_default(), None),
            // Bookkeeping tools (todo SQL, skill loading) aren't work the user asked for.
            "sql" | "skill" | "report_intent" | "think" => return,
            other => (ToolKind::Other, d["toolTitle"].as_str().unwrap_or(other).to_string(), None),
        };
        out.tool_start(&id, kind, title, detail);
        self.tools.insert(id, (name, a));
    }

    fn complete(&mut self, d: &serde_json::Value, out: &mut Out) {
        let id = s(d, "toolCallId").unwrap_or_default();
        let Some((name, a)) = self.tools.remove(&id) else {
            return;
        };
        let ok = d["success"].as_bool();
        let output = s(&d["result"], "content")
            .map(|c| strip_shell_footer(&c))
            .or_else(|| s(&d["error"], "message"));
        out.tool_end(&id, ok, output.clone());
        if ok != Some(true) {
            let out_str = output.as_deref().unwrap_or("");
            let lower = out_str.to_lowercase();
            if lower.contains("denied")
                || lower.contains("not allowed")
                || lower.contains("permission")
                || lower.contains("disallowed")
                || lower.contains("rejected")
                || lower.contains("operation not permitted")
                || lower.contains("permission denied")
                || lower.contains("access denied")
                || lower.contains("eperm")
                || lower.contains("eacces")
            {
                let cmd = s(&a, "command")
                    .or_else(|| s(&a, "path"))
                    .unwrap_or_else(|| name.clone());
                out.denial(Some(id), cmd, if out_str.is_empty() { "disallowed_tool".into() } else { out_str.into() });
            }
            return;
        }
        match name.as_str() {
            "apply_patch" => {
                for (path, op) in patch_files(a.as_str().unwrap_or("")) {
                    out.file(FileTouch {
                        tool_id: Some(id.clone()),
                        path,
                        before: (op == PatchOp::Add).then_some(None),
                        after: None,
                        deleted: op == PatchOp::Delete,
                    });
                }
            }
            "edit" | "create" | "write" | "str_replace_editor" => {
                if let Some(path) = s(&a, "path") {
                    out.file(FileTouch {
                        tool_id: Some(id),
                        path,
                        before: (name == "create").then_some(None),
                        after: None,
                        deleted: false,
                    });
                }
            }
            _ => {}
        }
    }
}

/// `<shellId: 0 completed with exit code 0>` is appended to every shell result.
fn strip_shell_footer(s: &str) -> String {
    match s.rfind("<shellId:") {
        Some(i) if s.trim_end().ends_with('>') => s[..i].trim_end().to_string(),
        _ => s.to_string(),
    }
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum PatchOp {
    Add,
    Update,
    Delete,
}

/// Files named in a `*** Begin Patch` envelope.
fn patch_files(patch: &str) -> Vec<(String, PatchOp)> {
    patch
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            [
                ("*** Add File: ", PatchOp::Add),
                ("*** Update File: ", PatchOp::Update),
                ("*** Delete File: ", PatchOp::Delete),
            ]
            .iter()
            .find_map(|(p, op)| l.strip_prefix(p).map(|f| (f.trim().to_string(), *op)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{testing::*, Ev};

    #[test]
    fn fixture() {
        let mut p = Copilot {
            cwd: "/work".into(),
            ..Default::default()
        };
        let evs = replay(&mut p, "copilot.jsonl");
        let text = text(&evs);
        assert_eq!(text.matches("Fixed `add`").count(), 1, "{text}");
        let reasoning: String = evs
            .iter()
            .filter_map(|e| match e {
                Ev::Emit(EventKind::Reasoning { text }) => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(reasoning.matches("**Inspecting repository files**").count(), 1);
        assert!(evs.iter().any(|e| matches!(e, Ev::Emit(EventKind::Model { name }) if name == "gpt-6-luna")));
        assert!(count(&evs, |k| matches!(k, EventKind::ToolStart { tool: ToolKind::Command, .. })) >= 3);
        let files: Vec<_> = evs
            .iter()
            .filter_map(|e| match e {
                Ev::File(f) => Some(f),
                _ => None,
            })
            .collect();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "m.py");
        assert_eq!(files[1].before, Some(None));
        assert!(matches!(evs.last(), Some(Ev::Emit(EventKind::Done { premium_requests: Some(_), .. }))));
        assert!(evs.iter().any(|e| matches!(e, Ev::Emit(EventKind::Thread { .. }))));
    }

    #[test]
    fn helpers() {
        assert_eq!(strip_shell_footer("a\n<shellId: 0 completed with exit code 0>"), "a");
        assert_eq!(
            patch_files("*** Begin Patch\n*** Update File: m.py\n@@\n*** Add File: t.py\n+x\n*** End Patch\n"),
            [("m.py".to_string(), PatchOp::Update), ("t.py".to_string(), PatchOp::Add)]
        );
    }

    #[test]
    fn denial_emitted_on_rejected_tool() {
        let mut p = Copilot {
            cwd: "/work".into(),
            ..Default::default()
        };
        let mut out = Out::default();
        p.line(
            &serde_json::json!({
                "type": "tool.execution_start",
                "data": {
                    "toolCallId": "call_99",
                    "toolName": "bash",
                    "arguments": {"command": "rm -rf build"}
                }
            }),
            &mut out,
        );
        p.line(
            &serde_json::json!({
                "type": "tool.execution_complete",
                "data": {
                    "toolCallId": "call_99",
                    "success": false,
                    "error": {"message": "Permission denied: command disallowed by policy"}
                }
            }),
            &mut out,
        );
        assert_eq!(
            out.evs.last(),
            Some(&Ev::Denial {
                tool_id: Some("call_99".into()),
                command: "rm -rf build".into(),
                reason: "Permission denied: command disallowed by policy".into(),
            })
        );
    }
}
