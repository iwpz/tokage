//! Reasonix adapter: `reasonix run … --output-format stream-json`.
//! Sample: tests/fixtures/reasonix.jsonl (Reasonix CLI 2.31.0).
//!
//! Stream lines use the shared eventwire contract (`kind`). The run ends with a
//! result object carrying `session_id`, `usage`, and structured `cost_quote`.

use super::{rel, s, ChatRequest, EventKind, FileTouch, Launch, Out, Parser, ToolKind};
use crate::providers::{trust_args, ProviderId};

pub fn launch(req: &ChatRequest, prompt: String) -> Launch {
    let mut args: Vec<String> = vec![
        "run".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--model".into(),
        req.model.clone(),
    ];
    let role = req.effective_role();
    if role.is_read_only() {
        // R8: never default to yolo / bypassPermissions.
        args.extend(["--permission-mode".into(), "read-only".into()]);
    } else if req.auth_mode == Some(tokage_core::auth::AuthMode::FullAuto)
        && role != tokage_core::Role::Tester
    {
        // `auto` still respects sandbox + deny rules; folder trust is a separate
        // user action (`reasonix trust --dir`), which Tokage never writes for them.
        args.extend(["--permission-mode".into(), "auto".into()]);
    } else {
        args.extend(["--permission-mode".into(), "acceptEdits".into()]);
    }
    if let Some(e) = &req.effort {
        args.extend(["--effort".into(), e.clone()]);
    }
    if let Some(id) = &req.resume {
        args.extend(["--resume".into(), id.clone()]);
        if req.fork {
            args.push("--copy".into());
        }
    }
    if !req.cwd.is_empty() {
        // Workspace root; does not imply trust. Untrusted folders may deny writes
        // with `permission.untrusted_folder` (user runs `reasonix trust --dir`).
        args.extend(["--dir".into(), req.cwd.clone()]);
    }
    args.extend(trust_args(ProviderId::Reasonix, &req.cwd));

    Launch {
        args,
        stdin: Some(prompt),
        env_remove: vec![],
        parser: Box::new(Reasonix {
            cwd: req.cwd.clone(),
            ..Default::default()
        }),
    }
}

#[derive(Default)]
pub struct Reasonix {
    cwd: String,
    tools: std::collections::HashMap<String, (String, serde_json::Value)>,
    usage: Option<tokage_core::event::TurnUsage>,
    thread_emitted: bool,
    model_emitted: bool,
    streamed_text: bool,
}

impl Parser for Reasonix {
    fn line(&mut self, v: &serde_json::Value, out: &mut Out) {
        // Final structured result (after eventwire lines).
        if v.get("type").and_then(|t| t.as_str()) == Some("result") {
            if let Some(id) = s(v, "session_id") {
                self.emit_thread(&id, out);
            }
            for d in v
                .get("permission_denials")
                .and_then(|x| x.as_array())
                .into_iter()
                .flatten()
            {
                let tool_id = s(d, "tool_use_id");
                let command = s(d, "tool_name").unwrap_or_default();
                let reason = s(d, "code").unwrap_or_else(|| "Denied by Reasonix policy".into());
                out.denial(tool_id, command, reason);
            }
            if let Some(u) = v.get("usage") {
                self.usage = Some(turn_usage_from_result(u, v));
            } else if self.usage.is_none() {
                // cost_quote alone still yields a USD estimate when possible.
                if let Some(cost) = cost_usd_from_quote_or_legacy(v, None) {
                    let mut u = empty_usage();
                    u.cost_usd = Some(cost);
                    self.usage = Some(u);
                }
            }
            if v.get("is_error").and_then(|b| b.as_bool()) == Some(true) {
                let msg = s(v, "result")
                    .or_else(|| s(v, "subtype"))
                    .unwrap_or_else(|| "Reasonix run failed".into());
                if let Some(qm) =
                    super::quota::match_quota(Some(tokage_core::ProviderId::Reasonix), &msg)
                {
                    out.quota(qm.raw, qm.reset_at, qm.retrying);
                }
                out.fail(msg);
                return;
            }
            let u = self.usage.clone().unwrap_or_else(empty_usage);
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
            return;
        }

        let kind = v.get("kind").and_then(|t| t.as_str()).unwrap_or("");
        match kind {
            "turn_started" | "turn_status" => {
                if let Some(id) = s(v, "sessionId").or_else(|| s(v, "session_id")) {
                    self.emit_thread(&id, out);
                }
                if let Some(model) = s(v, "modelRef").or_else(|| s(v, "model")) {
                    self.emit_model(&model, out);
                }
            }
            "reasoning" => {
                if let Some(content) = s(v, "reasoning").or_else(|| s(v, "text")) {
                    out.reasoning(&content);
                }
            }
            "text" => {
                if let Some(content) = s(v, "text") {
                    self.streamed_text = true;
                    out.begin_text();
                    out.text(&content);
                }
            }
            "message" => {
                // Aggregated assistant message: skip text if deltas already streamed.
                if let Some(r) = s(v, "reasoning").filter(|t| !t.is_empty()) {
                    out.reasoning(&r);
                }
                if !self.streamed_text {
                    if let Some(content) = s(v, "text") {
                        out.begin_text();
                        out.text(&content);
                    }
                }
            }
            "tool_dispatch" | "tool_started" => {
                let tool = v.get("tool").unwrap_or(v);
                let id = s(tool, "id").unwrap_or_default();
                let name = s(tool, "name").unwrap_or_default();
                let params = parse_args(tool);
                self.tools
                    .insert(id.clone(), (name.clone(), params.clone()));
                let (kind, title, detail) = classify_tool(&name, &params, &self.cwd);
                out.tool_start(&id, kind, title, detail);
            }
            "tool_result" => {
                let tool = v.get("tool").unwrap_or(v);
                let id = s(tool, "id").unwrap_or_default();
                let err = s(tool, "err");
                let output = s(tool, "output");
                let ok = if err.as_ref().is_some_and(|e| !e.is_empty()) {
                    Some(false)
                } else {
                    Some(true)
                };
                if let Some(code) = s(tool, "refusalCode").or_else(|| s(tool, "refusal_code")) {
                    let command = s(tool, "name")
                        .or_else(|| self.tools.get(&id).map(|(n, _)| n.clone()))
                        .unwrap_or_default();
                    out.denial(Some(id.clone()), command, code);
                }
                if let Some((name, params)) = self.tools.get(&id) {
                    if is_write_tool(name) {
                        if let Some(p) = path_from_params(params) {
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
                let msg = err.filter(|e| !e.is_empty()).or(output);
                out.tool_end(&id, ok, msg);
            }
            "usage" => {
                if let Some(u) = v.get("usage") {
                    self.usage = Some(turn_usage_from_eventwire(u));
                }
            }
            "approval_request" => {
                let approval = v.get("approval").unwrap_or(v);
                let tool_id = s(approval, "id");
                let command = s(approval, "subject")
                    .or_else(|| s(approval, "tool"))
                    .unwrap_or_default();
                let reason = s(approval, "reason")
                    .unwrap_or_else(|| "Reasonix requested approval".into());
                out.denial(tool_id, command, reason);
            }
            "turn_done" => {
                if let Some(err) = s(v, "err").filter(|e| !e.is_empty()) {
                    if let Some(qm) =
                        super::quota::match_quota(Some(tokage_core::ProviderId::Reasonix), &err)
                    {
                        out.quota(qm.raw, qm.reset_at, qm.retrying);
                    }
                    out.fail(err);
                }
                // Prefer the trailing `type: result` for Done + usage when present.
            }
            "notice" if v.get("level").and_then(|l| l.as_str()) == Some("warn") => {
                if let Some(msg) = s(v, "text").or_else(|| s(v, "detail")) {
                    out.emit(EventKind::Warning { message: msg });
                }
            }
            _ => {}
        }
    }
}

impl Reasonix {
    fn emit_thread(&mut self, id: &str, out: &mut Out) {
        if self.thread_emitted || id.is_empty() {
            return;
        }
        self.thread_emitted = true;
        out.emit(EventKind::Thread { id: id.to_string() });
    }

    fn emit_model(&mut self, name: &str, out: &mut Out) {
        if self.model_emitted || name.is_empty() {
            return;
        }
        self.model_emitted = true;
        out.emit(EventKind::Model {
            name: name.to_string(),
        });
    }
}

fn empty_usage() -> tokage_core::event::TurnUsage {
    tokage_core::event::TurnUsage {
        input_tokens: 0,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        api_calls: None,
        context_tokens: None,
        cost_usd: None,
        premium_requests: None,
        source: tokage_core::UsageSource::Structured,
    }
}

/// Prefer `cost_quote` USD identity / selected; fall back to legacy scalars.
/// `total_cost_usd` is a compatibility alias and does **not** imply USD.
fn cost_usd_from_quote_or_legacy(
    container: &serde_json::Value,
    usage: Option<&serde_json::Value>,
) -> Option<f64> {
    let quote = container
        .get("cost_quote")
        .or_else(|| container.get("costQuote"))
        .or_else(|| usage.and_then(|u| u.get("costQuote").or_else(|| u.get("cost_quote"))));
    if let Some(q) = quote {
        if let Some(n) = money_usd(q.get("selected")) {
            return Some(n);
        }
        if let Some(n) = q
            .get("valuations")
            .and_then(|v| v.get("USD"))
            .and_then(|v| money_usd(v.get("money")).or_else(|| money_usd(Some(v))))
        {
            return Some(n);
        }
        if let Some(n) = money_usd(q.get("original")) {
            return Some(n);
        }
    }
    // Legacy scalars only when currency is USD / $ / absent.
    let currency = container
        .get("currency")
        .or_else(|| usage.and_then(|u| u.get("currencyCode").or_else(|| u.get("currency"))))
        .and_then(|c| c.as_str())
        .unwrap_or("");
    let usd_like = currency.is_empty()
        || currency.eq_ignore_ascii_case("USD")
        || currency == "$";
    if !usd_like {
        return None;
    }
    container
        .get("total_cost_usd")
        .or_else(|| container.get("total_cost"))
        .and_then(as_f64)
        .or_else(|| {
            usage.and_then(|u| {
                u.get("costUsd")
                    .or_else(|| u.get("cost_usd"))
                    .or_else(|| u.get("cost"))
                    .and_then(as_f64)
            })
        })
}

fn money_usd(v: Option<&serde_json::Value>) -> Option<f64> {
    let v = v?;
    let currency = v
        .get("currency")
        .or_else(|| v.get("currencyCode"))
        .and_then(|c| c.as_str())
        .unwrap_or("USD");
    if !(currency.eq_ignore_ascii_case("USD") || currency == "$") {
        return None;
    }
    v.get("amount").and_then(as_f64).or_else(|| as_f64(v))
}

fn as_f64(v: &serde_json::Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_i64().map(|n| n as f64))
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

fn turn_usage_from_result(
    u: &serde_json::Value,
    result: &serde_json::Value,
) -> tokage_core::event::TurnUsage {
    let cost = cost_usd_from_quote_or_legacy(result, Some(u));
    tokage_core::event::TurnUsage {
        input_tokens: u
            .get("input_tokens")
            .or_else(|| u.get("prompt_tokens"))
            .and_then(|x| x.as_u64())
            .unwrap_or(0),
        output_tokens: u
            .get("output_tokens")
            .or_else(|| u.get("completion_tokens"))
            .and_then(|x| x.as_u64())
            .unwrap_or(0),
        cache_read_tokens: u
            .get("cache_read_input_tokens")
            .or_else(|| u.get("cache_read_tokens"))
            .and_then(|x| x.as_u64())
            .unwrap_or(0),
        cache_write_tokens: u
            .get("cache_creation_input_tokens")
            .or_else(|| u.get("cache_write_tokens"))
            .and_then(|x| x.as_u64())
            .unwrap_or(0),
        api_calls: u.get("api_calls").and_then(|x| x.as_u64()).map(|n| n as u32),
        context_tokens: u.get("context_tokens").and_then(|x| x.as_u64()).or_else(|| {
            let inp = u.get("input_tokens").and_then(|x| x.as_u64())?;
            let out = u.get("output_tokens").and_then(|x| x.as_u64())?;
            Some(inp.saturating_add(out))
        }),
        cost_usd: cost,
        premium_requests: None,
        source: tokage_core::UsageSource::Structured,
    }
}

fn turn_usage_from_eventwire(u: &serde_json::Value) -> tokage_core::event::TurnUsage {
    let prompt = u
        .get("promptTokens")
        .or_else(|| u.get("prompt_tokens"))
        .and_then(|x| x.as_u64())
        .unwrap_or(0);
    let completion = u
        .get("completionTokens")
        .or_else(|| u.get("completion_tokens"))
        .and_then(|x| x.as_u64())
        .unwrap_or(0);
    let cache_hit = u
        .get("cacheHitTokens")
        .or_else(|| u.get("cache_hit_tokens"))
        .and_then(|x| x.as_u64())
        .unwrap_or(0);
    let cache_miss = u
        .get("cacheMissTokens")
        .or_else(|| u.get("cache_miss_tokens"))
        .and_then(|x| x.as_u64())
        .unwrap_or(0);
    let total = u
        .get("totalTokens")
        .or_else(|| u.get("total_tokens"))
        .and_then(|x| x.as_u64());
    tokage_core::event::TurnUsage {
        input_tokens: prompt,
        output_tokens: completion,
        cache_read_tokens: cache_hit,
        // Eventwire reports cache miss separately; treat miss as write when hit is 0.
        cache_write_tokens: if cache_hit == 0 { cache_miss } else { 0 },
        api_calls: None,
        context_tokens: total.or(Some(prompt.saturating_add(completion))),
        cost_usd: cost_usd_from_quote_or_legacy(u, Some(u)),
        premium_requests: None,
        source: tokage_core::UsageSource::Structured,
    }
}

fn parse_args(tool: &serde_json::Value) -> serde_json::Value {
    if let Some(obj) = tool.get("parameters").or_else(|| tool.get("args")) {
        if let Some(s) = obj.as_str() {
            return serde_json::from_str(s).unwrap_or(serde_json::Value::Null);
        }
        return obj.clone();
    }
    serde_json::Value::Null
}

fn path_from_params(params: &serde_json::Value) -> Option<String> {
    s(params, "path")
        .or_else(|| s(params, "file_path"))
        .or_else(|| s(params, "filePath"))
}

fn is_write_tool(name: &str) -> bool {
    matches!(
        name,
        "write_file" | "edit_file" | "write" | "edit" | "apply_patch" | "str_replace"
    )
}

fn classify_tool(
    name: &str,
    params: &serde_json::Value,
    cwd: &str,
) -> (ToolKind, String, Option<String>) {
    let path_arg = |k: &str| s(params, k).map(|p| rel(cwd, &p)).unwrap_or_default();
    match name {
        "bash" | "shell" | "terminal" | "Bash" => (
            ToolKind::Command,
            s(params, "command").unwrap_or_default(),
            s(params, "description"),
        ),
        "read_file" | "read" | "view" | "Read" => {
            let mut path = path_arg("path");
            if path.is_empty() {
                path = path_arg("file_path");
            }
            (ToolKind::Read, path, None)
        }
        "write_file" | "edit_file" | "write" | "edit" | "apply_patch" | "str_replace" => (
            ToolKind::Edit,
            path_from_params(params)
                .map(|p| rel(cwd, &p))
                .unwrap_or_default(),
            None,
        ),
        "search" | "grep" | "glob" | "Grep" | "Glob" => (
            ToolKind::Search,
            s(params, "pattern")
                .or_else(|| s(params, "query"))
                .unwrap_or_default(),
            None,
        ),
        "web_fetch" | "fetch" | "web_search" | "WebFetch" | "WebSearch" => (
            ToolKind::Web,
            s(params, "url")
                .or_else(|| s(params, "query"))
                .unwrap_or_default(),
            None,
        ),
        _ => (ToolKind::Other, name.to_string(), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::testing::*;

    #[test]
    fn parses_fixture() {
        let mut p = Reasonix {
            cwd: "/work".into(),
            ..Default::default()
        };
        let evs = replay(&mut p, "reasonix.jsonl");
        assert_eq!(count(&evs, |k| matches!(k, EventKind::Thread { .. })), 1);
        assert_eq!(count(&evs, |k| matches!(k, EventKind::Model { .. })), 1);
        assert!(count(&evs, |k| matches!(k, EventKind::Reasoning { .. })) >= 2);
        assert_eq!(count(&evs, |k| matches!(k, EventKind::Text { .. })), 1);
        assert_eq!(count(&evs, |k| matches!(k, EventKind::Done { .. })), 1);

        let done = evs.iter().find_map(|e| match e {
            crate::chat::Ev::Emit(k @ EventKind::Done { .. }) => Some(k.clone()),
            _ => None,
        });
        let usage = done.unwrap().usage().unwrap();
        assert_eq!(usage.input_tokens, 7492);
        assert_eq!(usage.output_tokens, 13);
        assert_eq!(usage.cache_read_tokens, 0);
        assert_eq!(usage.cache_write_tokens, 7492);
        assert!((usage.cost_usd.unwrap() - 0.0011316).abs() < 1e-9);
    }

    #[test]
    fn cost_quote_prefers_usd_identity_over_cny_selected() {
        let result = serde_json::json!({
            "cost_quote": {
                "original": {"amount": "0.01", "currency": "USD"},
                "valuations": {
                    "USD": {"money": {"amount": "0.01", "currency": "USD"}, "basis": "identity"},
                    "CNY": {"money": {"amount": "0.07", "currency": "CNY"}, "basis": "official_table"}
                },
                "selected": {"amount": "0.07", "currency": "CNY"}
            },
            "total_cost": 0.07,
            "currency": "CNY",
            "total_cost_usd": 0.07
        });
        // selected is CNY → fall through to valuations.USD
        assert!((cost_usd_from_quote_or_legacy(&result, None).unwrap() - 0.01).abs() < 1e-9);
    }

    #[test]
    fn launch_read_only_and_resume() {
        let mut req = request(ProviderId::Reasonix);
        req.cwd = "/work".into();
        req.model = "deepseek/deepseek-flash".into();
        req.role = Some(tokage_core::Role::Planner);
        let got = launch(&req, "p".into());
        assert!(got
            .args
            .windows(2)
            .any(|w| w[0] == "--permission-mode" && w[1] == "read-only"));
        assert!(got
            .args
            .windows(2)
            .any(|w| w[0] == "--output-format" && w[1] == "stream-json"));
        assert!(got.args.windows(2).any(|w| w[0] == "--dir" && w[1] == "/work"));
        assert!(!got.args.iter().any(|a| a.contains("yolo") || a.contains("bypass")));

        req.role = Some(tokage_core::Role::Executor);
        req.resume = Some("rx-sess-100".into());
        req.fork = true;
        let got = launch(&req, "p".into());
        assert!(got
            .args
            .windows(2)
            .any(|w| w[0] == "--permission-mode" && w[1] == "acceptEdits"));
        assert!(got
            .args
            .windows(2)
            .any(|w| w[0] == "--resume" && w[1] == "rx-sess-100"));
        assert!(got.args.iter().any(|a| a == "--copy"));
    }
}
