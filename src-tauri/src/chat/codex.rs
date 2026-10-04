//! Codex CLI: `codex exec --json` (JSONL, one event per line).
//! Sample: tests/fixtures/codex.jsonl (codex-cli 0.160.0).

use super::{rel, s, ChatRequest, EventKind, FileTouch, Launch, Out, Parser, ToolKind};
use crate::providers::{trust_args, ProviderId};

pub fn launch(req: &ChatRequest, prompt: String) -> Launch {
    // `exec` has no approval channel: under `workspace-write` it runs any command (`rm`
    // included) without asking, whatever `approval_policy` says (checked on codex-cli 0.160.0).
    // Until Tokage can approve commands (P2.m3d), both modes stay read-only.
    let sandbox = "read-only";
    let mut args: Vec<String> = vec!["exec".into()];
    if let Some(id) = &req.resume {
        args.extend(["resume".into(), id.clone()]);
    }
    args.extend(["--json".into(), "-m".into(), req.model.clone()]);
    args.extend(trust_args(ProviderId::CodexCli, &req.cwd));
    let mut config = vec![
        format!("sandbox_mode=\"{sandbox}\""),
        // Reasoning summaries arrive as `reasoning` items; without this they are omitted.
        "model_reasoning_summary=\"auto\"".to_string(),
    ];
    if let Some(e) = &req.effort {
        config.push(format!("model_reasoning_effort=\"{e}\""));
    }
    if req.fast {
        config.push("service_tier=\"fast\"".into());
    }
    for c in config {
        args.extend(["-c".into(), c]);
    }
    args.push("-".into());
    Launch {
        args,
        stdin: Some(prompt),
        env_remove: vec![],
        parser: Box::new(Codex {
            cwd: req.cwd.clone(),
        }),
    }
}

pub struct Codex {
    cwd: String,
}

impl Parser for Codex {
    fn line(&mut self, v: &serde_json::Value, out: &mut Out) {
        let Some(t) = v.get("type").and_then(|t| t.as_str()) else {
            return;
        };
        match t {
            "thread.started" => {
                if let Some(id) = s(v, "thread_id") {
                    out.emit(EventKind::Thread { id });
                }
            }
            "item.started" | "item.completed" => {
                if let Some(item) = v.get("item") {
                    self.item(item, t == "item.completed", out);
                }
            }
            "turn.completed" => {
                let u = v.get("usage");
                let n = |k: &str| u.and_then(|u| u.get(k)).and_then(|x| x.as_u64()).unwrap_or(0);
                out.emit(EventKind::Done {
                    input_tokens: n("input_tokens"),
                    output_tokens: n("output_tokens") + n("reasoning_output_tokens"),
                    cost_usd: None,
                    premium_requests: None,
                });
            }
            "turn.failed" => out.fail(
                v.get("error")
                    .and_then(|e| s(e, "message"))
                    .unwrap_or_else(|| "turn failed".into()),
            ),
            "error" => out.fail(s(v, "message").unwrap_or_else(|| "codex error".into())),
            _ => {}
        }
    }
}

impl Codex {
    fn item(&self, item: &serde_json::Value, done: bool, out: &mut Out) {
        let id = s(item, "id").unwrap_or_default();
        match item.get("type").and_then(|t| t.as_str()).unwrap_or("") {
            "agent_message" if done => {
                out.begin_text();
                out.text(&s(item, "text").unwrap_or_default());
            }
            "reasoning" if done => out.reasoning(&s(item, "text").unwrap_or_default()),
            "command_execution" => {
                if !done {
                    let cmd = s(item, "command").unwrap_or_default();
                    out.tool_start(&id, ToolKind::Command, unwrap_shell(&cmd), None);
                } else {
                    let ok = item.get("exit_code").and_then(|c| c.as_i64()).map(|c| c == 0);
                    out.tool_end(&id, ok, s(item, "aggregated_output"));
                }
            }
            "file_change" => {
                let changes: Vec<(String, String)> = item
                    .get("changes")
                    .and_then(|c| c.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|c| Some((s(c, "path")?, s(c, "kind").unwrap_or_default())))
                            .collect()
                    })
                    .unwrap_or_default();
                if !done {
                    let names: Vec<String> = changes.iter().map(|(p, _)| rel(&self.cwd, p)).collect();
                    out.tool_start(&id, ToolKind::Edit, names.join(", "), None);
                    return;
                }
                let ok = s(item, "status").map(|st| st == "completed");
                out.tool_end(&id, ok, None);
                if ok == Some(true) {
                    for (path, kind) in changes {
                        out.file(FileTouch {
                            tool_id: Some(id.clone()),
                            path,
                            before: (kind == "add").then_some(None),
                            after: None,
                            deleted: kind == "delete",
                        });
                    }
                }
            }
            "mcp_tool_call" => {
                if !done {
                    let name = [s(item, "server"), s(item, "tool")]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(" · ");
                    out.tool_start(&id, ToolKind::Other, name, None);
                } else {
                    let ok = s(item, "status").map(|st| st == "completed");
                    out.tool_end(&id, ok, None);
                }
            }
            "web_search" => {
                if !done {
                    out.tool_start(&id, ToolKind::Web, s(item, "query").unwrap_or_default(), None);
                } else {
                    out.tool_end(&id, Some(true), None);
                }
            }
            // `error` items are CLI warnings (e.g. unstable features), not replies.
            _ => {}
        }
    }
}

/// Codex wraps commands as `/bin/zsh -lc <cmd>`; show the inner command.
fn unwrap_shell(cmd: &str) -> String {
    let inner = cmd.split_once(" -lc ").map_or(cmd, |(_, c)| c);
    let q = inner.chars().next();
    match q {
        Some('"' | '\'') if inner.len() >= 2 && inner.ends_with(q.unwrap()) => {
            let body = &inner[1..inner.len() - 1];
            if q == Some('"') {
                body.replace("\\\"", "\"").replace("\\\\", "\\")
            } else {
                body.to_string()
            }
        }
        _ => inner.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{testing::*, Change, Ev};

    #[test]
    fn fixture() {
        let evs = replay(&mut Codex { cwd: "/work".into() }, "codex.jsonl");
        assert!(matches!(&evs[0], Ev::Emit(EventKind::Thread { .. })));
        let text = text(&evs);
        assert!(text.starts_with("I’ll inspect"), "{text}");
        assert!(text.contains("\n\n`add` currently subtracts"));
        assert_eq!(count(&evs, |k| matches!(k, EventKind::ToolStart { tool: ToolKind::Command, .. })), 3);
        // The failed `python` run.
        assert!(evs.iter().any(|e| matches!(e, Ev::Emit(EventKind::ToolEnd { ok: Some(false), .. }))));
        let files: Vec<_> = evs
            .iter()
            .filter_map(|e| match e {
                Ev::File(f) => Some(f),
                _ => None,
            })
            .collect();
        assert_eq!(files.len(), 2);
        assert_eq!(files[1].before, Some(None), "test_m.py is new");
        assert!(matches!(evs.last(), Some(Ev::Emit(EventKind::Done { input_tokens: 90926, .. }))));
        let _ = Change::Added;
    }

    #[test]
    fn direct_is_read_only() {
        let l = launch(&request(ProviderId::CodexCli), "p".into());
        assert!(l.args.contains(&"sandbox_mode=\"read-only\"".to_string()), "{:?}", l.args);
        assert!(!l.args.iter().any(|a| a.contains("workspace-write") || a.contains("danger")));
    }

    #[test]
    fn shell_unwrap() {
        assert_eq!(unwrap_shell("/bin/zsh -lc ls"), "ls");
        assert_eq!(unwrap_shell("/bin/zsh -lc 'cat m.py && ls'"), "cat m.py && ls");
        assert_eq!(unwrap_shell("/bin/zsh -lc \"rg -g 'm.py'\""), "rg -g 'm.py'");
    }
}
