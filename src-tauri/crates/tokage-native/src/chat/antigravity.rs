//! Google Antigravity CLI: `agy -p … --output-format stream-json`.
//! Sample: tests/fixtures/antigravity.jsonl (Antigravity CLI 1.14.0).
//!
//! Antigravity CLI outputs structured events including deltas for reasoning and text,
//! tool start and completion, denials, quota signals, and turn usage summary.

use super::{rel, s, ChatRequest, EventKind, FileTouch, Launch, Out, Parser, ToolKind, READONLY_SHELL};
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
    if role.is_read_only() {
        args.extend(["--mode".into(), "plan".into()]);
        args.extend(["--disallowedTools".into(), "edit,write,shell,bash".into()]);
    } else {
        if req.auth_mode == Some(tokage_core::auth::AuthMode::FullAuto) && role != tokage_core::Role::Tester {
            args.extend(["--allow-all-tools".into()]);
        } else {
            let mut allowed = vec!["read".into(), "grep".into(), "glob".into(), "edit".into(), "write".into()];
            for c in READONLY_SHELL {
                allowed.push(format!("shell({c})"));
            }
            for pattern in &req.session_allowed {
                allowed.push(format!("shell({pattern})"));
            }
            args.extend(["--allowedTools".into(), allowed.join(",")]);
        }
    }
    if let Some(e) = &req.effort {
        args.extend(["--effort".into(), e.clone()]);
    }
    if let Some(id) = &req.resume {
        args.extend(["--resume".into(), id.clone()]);
    }

    // P2.m5a: Credential environment variable stripping
    let secret_vars = super::child_env::CREDENTIAL_ENV.join(",");
    args.push(format!("--secret-env-vars={secret_vars}"));

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
}

impl Parser for Antigravity {
    fn line(&mut self, v: &serde_json::Value, out: &mut Out) {
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

#[cfg(test)]
mod tests {
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
