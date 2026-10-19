//! Codex CLI: `codex exec --json` (JSONL, one event per line).
//! Sample: tests/fixtures/codex.jsonl (codex-cli 0.160.0).

use super::{rel, s, ChatRequest, EventKind, FileTouch, Launch, Out, Parser, ToolKind};
use crate::providers::{trust_args, ProviderId};

pub fn launch(req: &ChatRequest, prompt: String) -> Launch {
    // Only an explicit workspace execution grant enables writes. A command allowlist
    // cannot be enforced by `exec`, and must never widen the whole sandbox.
    let role = req.effective_role();
    let sandbox = if role.allows_writes()
        && req.auth_mode == Some(tokage_core::auth::AuthMode::FullAuto)
    {
        "workspace-write"
    } else {
        "read-only"
    };
    let mut args: Vec<String> = vec!["exec".into()];
    if let Some(id) = &req.resume {
        args.extend(["resume".into(), id.clone()]);
    }
    args.extend(["--json".into(), "--ignore-rules".into(), "-m".into(), req.model.clone()]);
    args.extend(trust_args(ProviderId::CodexCli, &req.cwd));
    let mut config = vec![
        format!("sandbox_mode=\"{sandbox}\""),
        // Headless turns cannot approve an escape. Do not inherit broader user roots,
        // temp write exceptions or execpolicy rules, including on native resume.
        "approval_policy=\"never\"".into(),
        "sandbox_workspace_write.writable_roots=[]".into(),
        "sandbox_workspace_write.network_access=false".into(),
        "sandbox_workspace_write.exclude_tmpdir_env_var=true".into(),
        "sandbox_workspace_write.exclude_slash_tmp=true".into(),
        // Reasoning summaries arrive as `reasoning` items; without this they are omitted.
        "model_reasoning_summary=\"auto\"".to_string(),
    ];
    if let Some(e) = &req.effort {
        config.push(format!("model_reasoning_effort=\"{e}\""));
    }
    if req.fast {
        config.push("service_tier=\"fast\"".into());
    }
    // P2.m5a: Restrict shell environment inheritance for model-executed commands
    config.push("shell_environment_policy.inherit=\"core\"".into());
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
            api_calls: 1,
        }),
    }
}

pub struct Codex {
    pub cwd: String,
    pub api_calls: u32,
}

impl Default for Codex {
    fn default() -> Self {
        Self {
            cwd: String::new(),
            api_calls: 1,
        }
    }
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
                    cache_read_tokens: n("cached_input_tokens"),
                    cache_write_tokens: n("cache_write_input_tokens"),
                    api_calls: Some(self.api_calls),
                    context_tokens: Some(n("input_tokens")),
                    cost_usd: None,
                    premium_requests: None, credits: None,
                    source: tokage_core::UsageSource::Structured,
                });
            }
            "turn.failed" => {
                let msg = v.get("error")
                    .and_then(|e| s(e, "message"))
                    .unwrap_or_else(|| "turn failed".into());
                if let Some(qm) = super::quota::match_quota(Some(tokage_core::ProviderId::CodexCli), &msg) {
                    out.quota(qm.raw, qm.reset_at, qm.retrying);
                }
                out.fail(msg);
            }
            "error" => {
                let msg = s(v, "message").unwrap_or_else(|| "codex error".into());
                if let Some(qm) = super::quota::match_quota(Some(tokage_core::ProviderId::CodexCli), &msg) {
                    out.quota(qm.raw, qm.reset_at, qm.retrying);
                }
                out.fail(msg);
            }
            _ => {}
        }
    }
}

impl Codex {
    fn item(&mut self, item: &serde_json::Value, done: bool, out: &mut Out) {
        let id = s(item, "id").unwrap_or_default();
        match item.get("type").and_then(|t| t.as_str()).unwrap_or("") {
            "agent_message" if done => {
                let txt = s(item, "text").unwrap_or_default();
                if let Some(qm) = super::quota::match_quota(Some(tokage_core::ProviderId::CodexCli), &txt) {
                    out.quota(qm.raw, qm.reset_at, qm.retrying);
                }
                out.begin_text();
                out.text(&txt);
            }
            "reasoning" if done => out.reasoning(&s(item, "text").unwrap_or_default()),
            "command_execution" => {
                if !done {
                    let cmd = s(item, "command").unwrap_or_default();
                    out.tool_start(&id, ToolKind::Command, unwrap_shell(&cmd), None);
                } else {
                    self.api_calls += 1;
                    let ok = item.get("exit_code").and_then(|c| c.as_i64()).map(|c| c == 0);
                    let output = s(item, "aggregated_output");
                    out.tool_end(&id, ok, output.clone());
                    if ok == Some(false) {
                        let out_str = output.as_deref().unwrap_or("");
                        let lower = out_str.to_lowercase();
                        let cmd = s(item, "command").unwrap_or_default();
                        let unwrapped = unwrap_shell(&cmd);
                        if lower.contains("operation not permitted")
                            || lower.contains("permission denied")
                            || lower.contains("read-only")
                            || lower.contains("sandbox")
                        {
                            out.denial(Some(id), unwrapped, "sandbox_denied".into());
                        }
                    }
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
        let evs = replay(&mut Codex { cwd: "/work".into(), ..Default::default() }, "codex.jsonl");
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
    fn workspace_grants_preserve_roles_and_are_rechecked_on_resume() {
        use tokage_core::{Role, auth::AuthMode};
        for resume in [None, Some("existing-thread".to_string())] {
            for role in [Role::Planner, Role::Reviewer, Role::Executor, Role::Tester] {
                for mode in [None, Some(AuthMode::AskAlways), Some(AuthMode::AskSensitive), Some(AuthMode::FullAuto)] {
                    let mut req = request(ProviderId::CodexCli);
                    req.role = Some(role);
                    req.auth_mode = mode;
                    req.resume = resume.clone();
                    // Even an approved command must not turn read-only into workspace access.
                    req.session_allowed = vec!["npm test".into()];
                    let args = launch(&req, "p".into()).args;
                    let expected = if role.allows_writes() && mode == Some(AuthMode::FullAuto) {
                        "workspace-write"
                    } else { "read-only" };
                    assert!(args.contains(&format!("sandbox_mode=\"{expected}\"")), "{role:?} {mode:?} {resume:?}: {args:?}");
                    assert!(args.contains(&"approval_policy=\"never\"".into()));
                    assert!(args.contains(&"--ignore-rules".into()));
                    assert!(args.contains(&"sandbox_workspace_write.writable_roots=[]".into()));
                    assert!(args.contains(&"sandbox_workspace_write.exclude_tmpdir_env_var=true".into()));
                    assert!(args.contains(&"sandbox_workspace_write.exclude_slash_tmp=true".into()));
                    assert!(!args.iter().any(|a| a.contains("danger") || a.contains("bypass")));
                }
            }
        }
        let mut req = request(ProviderId::CodexCli);
        req.role = Some(Role::Executor);
        req.auth_mode = Some(AuthMode::FullAuto);
        assert!(req.session_allowed.is_empty());
        assert!(launch(&req, "p".into()).args.contains(&"sandbox_mode=\"workspace-write\"".into()));
    }

    #[test]
    fn shell_unwrap() {
        assert_eq!(unwrap_shell("/bin/zsh -lc ls"), "ls");
        assert_eq!(unwrap_shell("/bin/zsh -lc 'cat m.py && ls'"), "cat m.py && ls");
        assert_eq!(unwrap_shell("/bin/zsh -lc \"rg -g 'm.py'\""), "rg -g 'm.py'");
    }

    #[test]
    fn denial_emitted_on_sandbox_denied() {
        let mut p = Codex {
            cwd: "/work".into(),
            ..Default::default()
        };
        let mut out = Out::default();
        p.line(
            &serde_json::json!({
                "type": "item.started",
                "item": {
                    "type": "command_execution",
                    "id": "item_cmd_1",
                    "command": "/bin/zsh -lc 'rm victim.txt'"
                }
            }),
            &mut out,
        );
        p.line(
            &serde_json::json!({
                "type": "item.completed",
                "item": {
                    "type": "command_execution",
                    "id": "item_cmd_1",
                    "command": "/bin/zsh -lc 'rm victim.txt'",
                    "exit_code": 1,
                    "aggregated_output": "rm: victim.txt: Operation not permitted (read-only filesystem)"
                }
            }),
            &mut out,
        );
        assert_eq!(
            out.evs.last(),
            Some(&Ev::Denial {
                tool_id: Some("item_cmd_1".into()),
                command: "rm victim.txt".into(),
                reason: "sandbox_denied".into(),
            })
        );
    }
}
