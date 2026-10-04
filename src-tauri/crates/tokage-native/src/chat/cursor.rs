//! Cursor Agent: `cursor-agent -p --output-format stream-json --stream-partial-output`.
//! Sample: tests/fixtures/cursor.jsonl (cursor-agent 2026.10.01).
//!
//! Text arrives as partial `assistant` messages, then once more as the whole block; thinking
//! arrives as `thinking` deltas. Tool calls carry one `<name>ToolCall` object whose result
//! holds the file's full content before and after an edit.

use std::path::{Path, PathBuf};

use super::{rel, s, ChatRequest, EventKind, FileTouch, Launch, Out, Parser, ToolKind, TurnMode, READONLY_SHELL};
use crate::{
    env::ShellEnv,
    providers::{trust_args, ProviderId},
};

/// Settings that would let `-p` run shell commands without asking (docs/11 §4.2).
///
/// Measured on cursor-agent 2026.10.01: in `-p` mode with the default `approvalMode:
/// "allowlist"`, a shell command outside `permissions.allow` is not run and not asked about;
/// it comes back as a `rejected` tool result. So Direct is safe exactly when the approval mode
/// is the allowlist and the allowlist holds read-only commands only. Tokage never passes
/// `--force` / `--yolo`. Only `approvalMode` and `permissions.allow` are read.
pub fn direct_risks(env: &ShellEnv, cwd: Option<&str>) -> Vec<String> {
    let home = env.vars.get("HOME").map(PathBuf::from);
    let mut files: Vec<PathBuf> = home.iter().map(|h| h.join(".cursor/cli-config.json")).collect();
    if let Some(c) = cwd {
        files.push(Path::new(c).join(".cursor/cli.json"));
    }
    files.iter().flat_map(|f| config_risks(f)).collect()
}

fn config_risks(path: &Path) -> Vec<String> {
    let Some(v) = std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
    else {
        return vec![];
    };
    let mut risks = vec![];
    if let Some(mode) = v["approvalMode"].as_str().filter(|m| *m != "allowlist") {
        risks.push(format!("approvalMode: {mode}"));
    }
    for rule in v["permissions"]["allow"].as_array().into_iter().flatten() {
        if let Some(r) = rule.as_str().filter(|r| !safe_allow(r)) {
            risks.push(format!("allow {r}"));
        }
    }
    risks
}

/// `Shell(cmd)` rules are safe when `cmd` is a read-only command; other tool kinds (Read,
/// Write) don't run commands.
fn safe_allow(rule: &str) -> bool {
    let Some(cmd) = rule.strip_prefix("Shell(").and_then(|r| r.strip_suffix(')')) else {
        return !rule.starts_with("Shell");
    };
    let cmd = cmd.trim();
    let cmd = cmd.strip_suffix(":*").or_else(|| cmd.strip_suffix(" *")).unwrap_or(cmd).trim();
    !cmd.contains(['*', ';', '&', '|', '`', '$', '>', '<']) && READONLY_SHELL.contains(&cmd)
}

pub fn launch(req: &ChatRequest, prompt: String) -> Launch {
    let mut args: Vec<String> = [
        "-p",
        "--output-format",
        "stream-json",
        "--stream-partial-output",
        "--model",
    ]
    .iter()
    .map(|a| a.to_string())
    .collect();
    args.push(req.model.clone());
    if req.mode == TurnMode::Plan {
        args.extend(["--mode".into(), "plan".into()]);
    }
    if let Some(id) = &req.resume {
        args.extend(["--resume".into(), id.clone()]);
    }
    args.extend(trust_args(ProviderId::CursorCli, &req.cwd));
    // Cursor takes the prompt as an argument; `--` keeps a leading dash from reading as a flag.
    args.extend(["--".into(), prompt]);
    Launch {
        args,
        stdin: None,
        env_remove: vec![],
        parser: Box::new(Cursor {
            cwd: req.cwd.clone(),
            pending: String::new(),
        }),
    }
}

const REJECTED: &str = "Not run: the command isn't on Cursor CLI's allowlist.";

pub struct Cursor {
    cwd: String,
    /// Text streamed for the current block, to recognise its repeat.
    pending: String,
}

impl Parser for Cursor {
    fn line(&mut self, v: &serde_json::Value, out: &mut Out) {
        match v["type"].as_str().unwrap_or("") {
            "system" if v["subtype"] == "init" => {
                if let Some(id) = s(v, "session_id") {
                    out.emit(EventKind::Thread { id });
                }
                if let Some(name) = s(v, "model") {
                    out.emit(EventKind::Model { name });
                }
            }
            "thinking" => {
                self.pending.clear();
                out.reasoning(v["text"].as_str().unwrap_or(""));
            }
            "assistant" => {
                let text: String = v["message"]["content"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|b| b["text"].as_str()).collect())
                    .unwrap_or_default();
                let partial = v.get("timestamp_ms").is_some() && v.get("model_call_id").is_none();
                if partial {
                    if self.pending.is_empty() {
                        out.begin_text();
                    }
                    out.text(&text);
                    self.pending.push_str(&text);
                } else if self.pending.is_empty() {
                    out.begin_text();
                    out.text(&text);
                } else {
                    // The whole block again; emit only what the partials missed.
                    if let Some(rest) = text.strip_prefix(self.pending.as_str()) {
                        out.text(rest);
                    }
                    self.pending.clear();
                }
            }
            "tool_call" => {
                self.pending.clear();
                self.tool(v, out);
            }
            "result" => {
                if v["is_error"].as_bool().unwrap_or(false) {
                    out.fail(s(v, "result").unwrap_or_else(|| "Cursor reported an error".into()));
                    return;
                }
                let u = &v["usage"];
                let n = |k: &str| u[k].as_u64().unwrap_or(0);
                out.emit(EventKind::Done {
                    input_tokens: n("inputTokens") + n("cacheReadTokens") + n("cacheWriteTokens"),
                    output_tokens: n("outputTokens"),
                    cost_usd: None,
                    premium_requests: None,
                });
            }
            _ => {}
        }
    }
}

impl Cursor {
    fn tool(&self, v: &serde_json::Value, out: &mut Out) {
        let id = s(v, "call_id").unwrap_or_default();
        let Some((name, call)) = v["tool_call"]
            .as_object()
            .and_then(|o| o.iter().find(|(k, _)| k.ends_with("ToolCall")))
        else {
            return;
        };
        let a = &call["args"];
        let path = |k: &str| s(a, k).map(|p| rel(&self.cwd, &p)).unwrap_or_default();
        if v["subtype"] == "started" {
            let (kind, title) = match name.as_str() {
                "shellToolCall" => (ToolKind::Command, s(a, "command").unwrap_or_default()),
                "readToolCall" => (ToolKind::Read, path("path")),
                "globToolCall" => (ToolKind::Search, s(a, "globPattern").unwrap_or_default()),
                "grepToolCall" => (ToolKind::Search, s(a, "pattern").unwrap_or_default()),
                "editToolCall" | "writeToolCall" | "deleteToolCall" => (ToolKind::Edit, path("path")),
                "webSearchToolCall" => (ToolKind::Web, s(a, "searchTerm").or(s(a, "query")).unwrap_or_default()),
                "webFetchToolCall" => (ToolKind::Web, s(a, "url").unwrap_or_default()),
                other => (ToolKind::Other, other.trim_end_matches("ToolCall").to_string()),
            };
            out.tool_start(&id, kind, title, None);
            return;
        }
        let r = &call["result"];
        if r.get("rejected").is_some() {
            out.tool_end(&id, Some(false), Some(REJECTED.into()));
            return;
        }
        let (ok, body) = match (r.get("success"), r.get("failure"), r.get("error")) {
            (Some(b), _, _) => (true, b),
            (_, Some(b), _) | (_, _, Some(b)) => (false, b),
            _ => (false, r),
        };
        let output = match name.as_str() {
            "shellToolCall" => s(body, "interleavedOutput").or_else(|| s(body, "stdout")),
            _ => s(body, "message").or_else(|| s(body, "error")),
        };
        let ok = match name.as_str() {
            "shellToolCall" => body["exitCode"].as_i64().map(|c| c == 0).or(Some(ok)),
            _ => Some(ok),
        };
        out.tool_end(&id, ok, output);
        if ok != Some(true) {
            return;
        }
        match name.as_str() {
            "editToolCall" | "writeToolCall" => {
                let new_file = s(body, "diffString").is_some_and(|d| d.starts_with("--- /dev/null"));
                out.file(FileTouch {
                    tool_id: Some(id),
                    path: s(body, "path").or_else(|| s(a, "path")).unwrap_or_default(),
                    before: match s(body, "beforeFullFileContent") {
                        Some(t) => Some(Some(t)),
                        None if new_file => Some(None),
                        None => None,
                    },
                    after: s(body, "afterFullFileContent"),
                    deleted: false,
                });
            }
            "deleteToolCall" => out.file(FileTouch {
                tool_id: Some(id),
                path: s(a, "path").unwrap_or_default(),
                before: None,
                after: None,
                deleted: true,
            }),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{testing::*, Ev};

    #[test]
    fn allow_rules() {
        for r in ["Shell(ls)", "Shell(git status)", "Shell(rg:*)", "Read(**)", "Write(src/**)"] {
            assert!(safe_allow(r), "{r}");
        }
        for r in ["Shell(*)", "Shell(rm)", "Shell(rm:*)", "Shell(ls; rm -rf /)", "Shell(npm test)", "Shell"] {
            assert!(!safe_allow(r), "{r}");
        }
    }

    #[test]
    fn config_files() {
        let dir = std::env::temp_dir().join(format!("tokage-cursor-cfg-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".cursor")).unwrap();
        let env = ShellEnv {
            vars: [("HOME".to_string(), dir.display().to_string())].into(),
        };
        let user = dir.join(".cursor/cli-config.json");
        // Cursor's own defaults.
        std::fs::write(&user, r#"{"permissions":{"allow":["Shell(ls)"],"deny":[]},"approvalMode":"allowlist"}"#).unwrap();
        assert!(direct_risks(&env, None).is_empty());
        std::fs::write(&user, r#"{"permissions":{"allow":["Shell(ls)","Shell(rm)"]},"approvalMode":"unrestricted"}"#).unwrap();
        assert_eq!(direct_risks(&env, None), ["approvalMode: unrestricted", "allow Shell(rm)"]);
        // A project's .cursor/cli.json counts too.
        std::fs::write(&user, "{}").unwrap();
        let project = dir.join("p");
        std::fs::create_dir_all(project.join(".cursor")).unwrap();
        std::fs::write(project.join(".cursor/cli.json"), r#"{"permissions":{"allow":["Shell(*)"]}}"#).unwrap();
        assert_eq!(direct_risks(&env, project.to_str()), ["allow Shell(*)"]);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn rejected_call() {
        let mut p = Cursor {
            cwd: "/work".into(),
            pending: String::new(),
        };
        let mut out = Out::default();
        let call = |sub: &str, result: serde_json::Value| {
            serde_json::json!({"type":"tool_call","subtype":sub,"call_id":"c1",
                "tool_call":{"shellToolCall":{"args":{"command":"rm victim.txt"},"result":result}}})
        };
        p.line(&call("started", serde_json::Value::Null), &mut out);
        // Recorded from cursor-agent 2026.10.01 with the default allowlist.
        p.line(&call("completed", serde_json::json!({"rejected":{"command":"rm victim.txt","reason":"","isReadonly":false}})), &mut out);
        assert!(matches!(&out.evs[1], Ev::Emit(EventKind::ToolEnd { ok: Some(false), output: Some(o), .. }) if o == REJECTED));
    }

    #[test]
    fn fixture() {
        let mut p = Cursor {
            cwd: "/work".into(),
            pending: String::new(),
        };
        let evs = replay(&mut p, "cursor.jsonl");
        let text = text(&evs);
        assert_eq!(text.matches("I'll inspect `m.py`").count(), 1, "{text}");
        assert!(text.ends_with("`assert add(2, 3) == 5`."), "{text}");
        assert!(text.contains("\n\n## Summary"), "{text}");
        assert!(count(&evs, |k| matches!(k, EventKind::Reasoning { .. })) > 5);
        assert_eq!(count(&evs, |k| matches!(k, EventKind::ToolStart { tool: ToolKind::Command, .. })), 2);
        // `python` isn't installed in the sample environment.
        assert!(evs.iter().any(|e| matches!(e, Ev::Emit(EventKind::ToolEnd { ok: Some(false), .. }))));
        let files: Vec<_> = evs
            .iter()
            .filter_map(|e| match e {
                Ev::File(f) => Some(f),
                _ => None,
            })
            .collect();
        assert_eq!(files.len(), 2);
        let test = files.iter().find(|f| f.path.ends_with("test_m.py")).unwrap();
        assert_eq!(test.before, Some(None));
        let m = files.iter().find(|f| f.path.ends_with("/m.py")).unwrap();
        assert_eq!(m.before.as_ref().unwrap().as_deref(), Some("def add(a,b):\n    return a-b\n"));
        assert!(matches!(evs.last(), Some(Ev::Emit(EventKind::Done { .. }))));
    }
}
