//! OpenCode adapter: `opencode run … --format json`.
//! Legacy sample: tests/fixtures/opencode.jsonl (0.8.4).
//! Current contract samples: opencode-v1.jsonl (1.19.0), opencode-v2.jsonl (2.0.29).
//! V2 usage fallback: opencode-v2-export.json; current samples use a local synthetic model.
//!
//! OpenCode streams JSON lines containing session lifecycle, reasoning and text deltas,
//! tool invocations, usage metrics, and completion events.

use super::{rel, s, ChatRequest, EventKind, FileTouch, Launch, Out, Parser, ToolKind};
use crate::env::ShellEnv;

pub fn launch(req: &ChatRequest, prompt: String) -> Launch {
    let mut args: Vec<String> = vec![
        "run".into(),
        prompt,
        "--format".into(),
        "json".into(),
        "--model".into(),
        req.model.clone(),
    ];
    args.extend([
        "--agent".into(),
        if req.effective_role().is_read_only() {
            "plan"
        } else {
            "build"
        }
        .into(),
    ]);
    if let Some(id) = &req.resume {
        args.extend(["--session".into(), id.clone()]);
    }
    Launch {
        args,
        stdin: None,
        env_remove: vec![],
        parser: Box::new(Opencode {
            cwd: req.cwd.clone(),
            ..Default::default()
        }),
    }
}

/// Both CLI generations use JSON parts. V2 must own a private server so tools
/// inherit Tokage's environment and sandbox rather than an existing daemon's.
pub async fn prepare(
    exe: &std::path::Path,
    env: &ShellEnv,
    args: &mut Vec<String>,
) -> Result<(), String> {
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        tokio::process::Command::new(exe)
            .args(["run", "--help"])
            .env_clear()
            .envs(&env.vars)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| "OpenCode capability probe timed out")?
    .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("OpenCode run capabilities unavailable".into());
    }
    let help = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if help.contains("--standalone") {
        args.push("--standalone".into());
    } else if help.contains("--pure") {
        args.push("--pure".into());
    }
    Ok(())
}

pub fn runtime_config(req: &ChatRequest, env: &ShellEnv) -> Result<String, String> {
    let mut config: serde_json::Value = match env.vars.get("OPENCODE_CONFIG_CONTENT") {
        Some(s) => serde_json::from_str(s).map_err(|_| "Invalid OPENCODE_CONFIG_CONTENT JSON")?,
        None => serde_json::json!({}),
    };
    if !config.is_object() {
        return Err("OPENCODE_CONFIG_CONTENT must be an object".into());
    }
    // Native permission rules are also accepted by V2's V1 compatibility normalizer.
    // Deny shell and delegation here; Tokage executes task gates independently.
    let edit = if req.effective_role().is_read_only() {
        "deny"
    } else {
        "allow"
    };
    config["permission"] = serde_json::json!({"*":"deny", "read":"allow", "glob":"allow", "grep":"allow", "list":"allow", "edit":edit, "bash":"deny", "task":"deny", "external_directory":"deny"});
    // Drop a V2 rules array that could override the normalized V1 rules.
    config.as_object_mut().unwrap().remove("permissions");
    config["share"] = serde_json::json!("disabled");
    config["snapshot"] = serde_json::json!(false);
    if let Ok(wrapper) = super::child_env::ensure_shell_wrapper() {
        config["shell"] = serde_json::json!(wrapper);
    }
    Ok(config.to_string())
}

/// Some V2 releases omit step_finish on stdout. Reconcile only assistant messages
/// observed in this invocation, never the cumulative usage of a resumed session.
#[derive(Default)]
pub struct Receipt {
    session: Option<String>,
    messages: std::collections::HashSet<String>,
    reported: std::collections::HashSet<String>,
}
impl Receipt {
    pub fn observe(&mut self, v: &serde_json::Value) {
        if let Some(id) = s(v, "sessionID") {
            self.session = Some(id);
        }
        if let Some(id) = s(&v["part"], "messageID") {
            self.messages.insert(id.clone());
            if s(v, "type").as_deref() == Some("step_finish") {
                self.reported.insert(id);
            }
        }
    }
    pub async fn reconcile(
        &self,
        exe: &std::path::Path,
        env: &ShellEnv,
        cwd: &str,
    ) -> Result<Vec<serde_json::Value>, String> {
        let missing: Vec<_> = self.messages.difference(&self.reported).collect();
        if missing.is_empty() {
            return Ok(vec![]);
        }
        let session = self
            .session
            .as_deref()
            .ok_or("OpenCode did not report a session")?;
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            tokio::process::Command::new(exe)
                .args(["session", "export", session, "--standalone"])
                .current_dir(cwd)
                .env_clear()
                .envs(super::child_env::assemble_with_proxy(
                    crate::providers::ProviderId::Opencode,
                    env,
                    &[],
                    None,
                ))
                .env("PWD", cwd)
                .kill_on_drop(true)
                .output(),
        )
        .await
        .map_err(|_| "OpenCode usage reconciliation timed out")?
        .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err("OpenCode usage export failed".into());
        }
        let value: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|_| "OpenCode usage export was not JSON")?;
        if value["info"]["id"].as_str() != Some(session) {
            return Err("OpenCode export session mismatch".into());
        }
        reconcile_messages(&value, &self.messages, &self.reported)
    }
}
fn reconcile_messages(
    value: &serde_json::Value,
    messages: &std::collections::HashSet<String>,
    reported: &std::collections::HashSet<String>,
) -> Result<Vec<serde_json::Value>, String> {
    let rows = value["messages"]
        .as_array()
        .ok_or("OpenCode export omitted messages")?;
    if !rows
        .last()
        .is_some_and(|m| m["type"] == "idle" && m["outcome"] == "succeeded")
    {
        return Err("OpenCode session did not confirm successful execution".into());
    }
    let mut result = vec![];
    for row in rows {
        let Some(id) = s(row, "id") else {
            continue;
        };
        if row["type"] != "assistant" || !messages.contains(&id) || reported.contains(&id) {
            continue;
        }
        if row.get("tokens").is_none() || row["time"].get("completed").is_none() {
            return Err("OpenCode assistant response has incomplete usage".into());
        }
        if let (Some(provider), Some(model)) =
            (s(&row["model"], "providerID"), s(&row["model"], "id"))
        {
            result.push(
                serde_json::json!({"type":"session.start", "model":format!("{provider}/{model}")}),
            );
        }
        result.push(serde_json::json!({"type":"step_finish", "part":{"messageID":id,"reason":row["finish"],"tokens":row["tokens"],"cost":row["cost"]}}));
    }
    if result.is_empty() {
        return Err("OpenCode export has no usage for this invocation".into());
    }
    Ok(result)
}

#[derive(Default)]
pub struct Opencode {
    cwd: String,
    model: Option<String>,
    tools: std::collections::HashMap<String, (String, serde_json::Value)>,
    usage: Option<tokage_core::event::TurnUsage>,
    thread: Option<String>,
    failed: bool,
    completed: bool,
}

impl Parser for Opencode {
    fn line(&mut self, v: &serde_json::Value, out: &mut Out) {
        if let Some(id) = s(v, "sessionID") {
            if self.thread.as_ref() != Some(&id) {
                self.thread = Some(id.clone());
                out.emit(EventKind::Thread { id });
            }
        }
        if let Some(part) = v.get("part") {
            let typ = s(v, "type").unwrap_or_default();
            match typ.as_str() {
                "text" | "reasoning" => {
                    if let Some(text) = s(part, "text") {
                        if typ == "reasoning" {
                            out.reasoning(&text);
                        } else {
                            out.begin_text();
                            out.text(&text);
                        }
                    }
                }
                "tool_use" => {
                    let state = &part["state"];
                    let id = s(part, "callID")
                        .or_else(|| s(part, "id"))
                        .unwrap_or_default();
                    let name = s(part, "tool").unwrap_or_default();
                    let mut params = state["input"].clone();
                    if params.get("path").is_none() {
                        if let Some(path) = s(&params, "filePath") {
                            params["path"] = serde_json::json!(path);
                        }
                    }
                    self.line(&serde_json::json!({"type":"tool_call", "id":id, "name":name, "parameters":params}), out);
                    let ok = s(state, "status").as_deref() == Some("completed");
                    self.line(&serde_json::json!({"type":"tool_result", "id":id, "ok":ok, "output":s(state,"output").or_else(|| s(state,"error"))}), out);
                }
                "step_finish" => {
                    let tokens = &part["tokens"];
                    let n = |v: &serde_json::Value| v.as_u64().unwrap_or(0);
                    let u = self
                        .usage
                        .get_or_insert_with(|| tokage_core::event::TurnUsage {
                            input_tokens: 0,
                            output_tokens: 0,
                            cache_read_tokens: 0,
                            cache_write_tokens: 0,
                            api_calls: Some(0),
                            context_tokens: None,
                            cost_usd: None,
                            premium_requests: None, credits: None,
                            source: tokage_core::UsageSource::Structured,
                        });
                    u.input_tokens += n(&tokens["input"]);
                    u.output_tokens += n(&tokens["output"]) + n(&tokens["reasoning"]);
                    u.cache_read_tokens += n(&tokens["cache"]["read"]);
                    u.cache_write_tokens += n(&tokens["cache"]["write"]);
                    u.api_calls = Some(u.api_calls.unwrap_or(0) + 1);
                    if let Some(cost) = part["cost"].as_f64() {
                        u.cost_usd = Some(u.cost_usd.unwrap_or(0.0) + cost);
                    }
                    self.completed =
                        matches!(s(part, "reason").as_deref(), Some("stop" | "end_turn"));
                }
                _ => {}
            }
            return;
        }
        let typ = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match typ {
            "session.start" => {
                if let Some(id) = s(v, "id") {
                    out.emit(EventKind::Thread { id });
                }
                if let Some(model) = s(v, "model") {
                    self.model = Some(model.clone());
                    out.emit(EventKind::Model { name: model });
                }
            }
            "reasoning_delta" | "reasoning" => {
                if let Some(content) = s(v, "content").or_else(|| s(v, "text")) {
                    out.reasoning(&content);
                }
            }
            "text_delta" | "text" | "content" => {
                if let Some(content) = s(v, "content").or_else(|| s(v, "text")) {
                    out.begin_text();
                    out.text(&content);
                }
            }
            "tool_call" => {
                let id = s(v, "id").unwrap_or_default();
                let name = s(v, "name").unwrap_or_default();
                let params = v
                    .get("parameters")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                self.tools
                    .insert(id.clone(), (name.clone(), params.clone()));

                let path_arg = |k: &str| {
                    s(&params, k)
                        .map(|p| rel(&self.cwd, &p))
                        .unwrap_or_default()
                };
                let (kind, title, detail) = match name.as_str() {
                    "bash" | "shell" | "terminal" => (
                        ToolKind::Command,
                        s(&params, "command").unwrap_or_default(),
                        s(&params, "description"),
                    ),
                    "read_file" | "read" | "view" => (ToolKind::Read, path_arg("path"), None),
                    "write_file" | "edit_file" | "write" | "edit" => {
                        (ToolKind::Edit, path_arg("path"), None)
                    }
                    "search" | "grep" | "glob" => (
                        ToolKind::Search,
                        s(&params, "pattern")
                            .or_else(|| s(&params, "query"))
                            .unwrap_or_default(),
                        None,
                    ),
                    "web_fetch" | "fetch" => {
                        (ToolKind::Web, s(&params, "url").unwrap_or_default(), None)
                    }
                    _ => (ToolKind::Other, name, None),
                };
                out.tool_start(&id, kind, title, detail);
            }
            "tool_result" => {
                let id = s(v, "id").unwrap_or_default();
                let ok = v.get("ok").and_then(|b| b.as_bool()).or(Some(true));
                let output = s(v, "output");

                if let Some((name, params)) = self.tools.get(&id) {
                    if ok == Some(true)
                        && matches!(name.as_str(), "write_file" | "edit_file" | "write" | "edit")
                    {
                        if let Some(p) = s(params, "path") {
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
                let reason = s(v, "reason").unwrap_or_else(|| "Denied by OpenCode policy".into());
                out.denial(tool_id, command, reason);
            }
            "quota" => {
                let raw = s(v, "message").unwrap_or_else(|| "Quota exceeded".into());
                let reset_at = s(v, "reset_at");
                let retrying = v.get("retrying").and_then(|b| b.as_bool()).unwrap_or(false);
                out.quota(raw, reset_at, retrying);
            }
            "usage" => {
                let inp = v
                    .get("input_tokens")
                    .or_else(|| v.get("prompt_tokens"))
                    .and_then(|x| x.as_u64())
                    .unwrap_or(0);
                let out_tok = v
                    .get("output_tokens")
                    .or_else(|| v.get("completion_tokens"))
                    .and_then(|x| x.as_u64())
                    .unwrap_or(0);
                let cr = v
                    .get("cache_read_tokens")
                    .and_then(|x| x.as_u64())
                    .unwrap_or(0);
                let cw = v
                    .get("cache_write_tokens")
                    .and_then(|x| x.as_u64())
                    .unwrap_or(0);
                let calls = v
                    .get("api_calls")
                    .and_then(|x| x.as_u64())
                    .map(|n| n as u32);
                let ctx = v.get("context_tokens").and_then(|x| x.as_u64());
                let cost = v
                    .get("cost")
                    .or_else(|| v.get("cost_usd"))
                    .and_then(|x| x.as_f64());

                self.usage = Some(tokage_core::event::TurnUsage {
                    input_tokens: inp,
                    output_tokens: out_tok,
                    cache_read_tokens: cr,
                    cache_write_tokens: cw,
                    api_calls: calls,
                    context_tokens: ctx,
                    cost_usd: cost,
                    premium_requests: None, credits: None,
                    source: tokage_core::UsageSource::Structured,
                });
            }
            "session.end" => {
                let u = self.usage.clone().unwrap_or(tokage_core::event::TurnUsage {
                    input_tokens: 0,
                    output_tokens: 0,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                    api_calls: None,
                    context_tokens: None,
                    cost_usd: None,
                    premium_requests: None, credits: None,
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
                    premium_requests: u.premium_requests, credits: None,
                    source: u.source,
                });
            }
            "error" => {
                self.failed = true;
                let msg = s(v, "message")
                    .or_else(|| s(&v["error"], "message"))
                    .or_else(|| s(&v["error"]["data"], "message"))
                    .unwrap_or_else(|| "OpenCode error".into());
                if let Some(qm) =
                    super::quota::match_quota(Some(tokage_core::ProviderId::Opencode), &msg)
                {
                    out.quota(qm.raw, qm.reset_at, qm.retrying);
                }
                out.fail(msg);
            }
            _ => {}
        }
    }
    fn finish(&mut self, out: &mut Out) {
        if self.thread.is_none() {
            return;
        }
        if self.failed {
            return;
        }
        if !self.completed {
            out.fail("OpenCode exited without a completed model response");
            return;
        }
        if let Some(u) = &self.usage {
            out.emit(EventKind::Done {
                input_tokens: u.input_tokens,
                output_tokens: u.output_tokens,
                cache_read_tokens: u.cache_read_tokens,
                cache_write_tokens: u.cache_write_tokens,
                api_calls: u.api_calls,
                context_tokens: u.context_tokens,
                cost_usd: u.cost_usd,
                premium_requests: u.premium_requests, credits: None,
                source: u.source,
            });
        } else {
            out.fail("OpenCode completed without usage");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::testing::*;

    #[test]
    fn parses_fixture() {
        let mut p = Opencode {
            cwd: "/work".into(),
            ..Default::default()
        };
        let evs = replay(&mut p, "opencode.jsonl");
        assert_eq!(count(&evs, |k| matches!(k, EventKind::Thread { .. })), 1);
        assert_eq!(count(&evs, |k| matches!(k, EventKind::Model { .. })), 1);
        assert_eq!(count(&evs, |k| matches!(k, EventKind::Reasoning { .. })), 1);
        assert_eq!(count(&evs, |k| matches!(k, EventKind::Text { .. })), 2);
        assert_eq!(count(&evs, |k| matches!(k, EventKind::ToolStart { .. })), 2);
        assert_eq!(count(&evs, |k| matches!(k, EventKind::ToolEnd { .. })), 2);
        assert_eq!(count(&evs, |k| matches!(k, EventKind::Done { .. })), 1);
    }
    #[test]
    fn modern_v1_and_v2_parts_report_only_current_turn_usage() {
        for version in [1, 2] {
            let mut p = Opencode::default();
            let mut out = Out::default();
            let mut receipt = Receipt::default();
            let fixture = if version == 1 {
                include_str!("../../tests/fixtures/opencode-v1.jsonl")
            } else {
                include_str!("../../tests/fixtures/opencode-v2.jsonl")
            };
            for line in fixture.lines() {
                let v: serde_json::Value = serde_json::from_str(line).unwrap();
                receipt.observe(&v);
                p.line(&v, &mut out);
            }
            if version == 2 {
                let mut export: serde_json::Value = serde_json::from_str(include_str!(
                    "../../tests/fixtures/opencode-v2-export.json"
                ))
                .unwrap();
                let mut old = export["messages"][0].clone();
                old["id"] = serde_json::json!("previous-turn");
                old["tokens"]["input"] = serde_json::json!(999999);
                export["messages"].as_array_mut().unwrap().insert(0, old);
                for event in
                    reconcile_messages(&export, &receipt.messages, &receipt.reported).unwrap()
                {
                    p.line(&event, &mut out);
                }
                export["messages"]
                    .as_array_mut()
                    .unwrap()
                    .last_mut()
                    .unwrap()["outcome"] = serde_json::json!("failed");
                assert!(reconcile_messages(&export, &receipt.messages, &receipt.reported).is_err());
            }
            p.finish(&mut out);
            assert!(out.evs.iter().any(|e| matches!(
                e,
                super::super::Ev::Emit(EventKind::Done {
                    input_tokens: 12,
                    output_tokens: 5,
                    ..
                })
            )));
            assert!(!out
                .evs
                .iter()
                .any(|e| matches!(e, super::super::Ev::Failed(_))));
        }
    }

    #[test]
    fn failed_or_truncated_modern_turns_cannot_succeed() {
        let mut p = Opencode::default();
        let mut out = Out::default();
        p.line(&serde_json::json!({"type":"error","sessionID":"fixture","error":{"message":"invalid API key"}}),&mut out);
        p.finish(&mut out);
        assert!(!out
            .evs
            .iter()
            .any(|e| matches!(e, super::super::Ev::Emit(EventKind::Done { .. }))));
        assert!(out
            .evs
            .iter()
            .any(|e| matches!(e,super::super::Ev::Failed(s) if s == "invalid API key")));
    }
}
