//! Read-only planning over the Codex app-server v2 stdio protocol.
//! Unlike exec's completed-message JSONL, agentMessage deltas arrive during generation.
use super::{ChatRequest, EventKind, Launch, Out, Parser, ToolKind};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub fn launch(req: &ChatRequest, prompt: String) -> Launch {
    let args = vec![
        "app-server".into(),
        "--listen".into(),
        "stdio://".into(),
        "-c".into(),
        "sandbox_mode=\"read-only\"".into(),
        "-c".into(),
        "approval_policy=\"never\"".into(),
        "-c".into(),
        "shell_environment_policy.inherit=\"core\"".into(),
    ];
    let init = json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"tokage","title":"Tokage","version":env!("CARGO_PKG_VERSION")}}});
    Launch {
        args,
        stdin: Some(format!("{init}\n")),
        env_remove: vec![],
        parser: Box::new(Stream {
            req: req.clone(),
            prompt,
            thread: None,
            turn: None,
            outgoing: vec![],
            messages: BTreeMap::new(),
            completed_messages: BTreeSet::new(),
            next_response: 1,
            usage: None,
            complete: false,
            reasoning_streamed: false,
            api_calls: 1,
        }),
    }
}
struct Stream {
    req: ChatRequest,
    prompt: String,
    thread: Option<String>,
    turn: Option<String>,
    outgoing: Vec<Value>,
    messages: BTreeMap<String, String>,
    completed_messages: BTreeSet<String>,
    next_response: u64,
    usage: Option<Value>,
    complete: bool,
    reasoning_streamed: bool,
    api_calls: u32,
}
impl Stream {
    fn fail(&mut self, out: &mut Out, message: impl Into<String>) {
        out.fail(message);
        self.complete = true;
    }
    fn thread_params(&self) -> Value {
        let mut p = json!({"cwd":self.req.cwd,"model":self.req.model,"approvalPolicy":"never","sandbox":"read-only",
            "config":{"shell_environment_policy.inherit":"core"}});
        if let Some(id) = &self.req.resume {
            p["threadId"] = json!(id);
        }
        if self.req.fast {
            p["serviceTier"] = json!("fast");
        }
        p
    }
    fn matches_turn(&self, p: &Value) -> bool {
        p["threadId"].as_str() == self.thread.as_deref()
            && self.turn.as_deref().is_some_and(|id| {
                p["turnId"].as_str().or_else(|| p["turn"]["id"].as_str()) == Some(id)
            })
    }
    fn message(&mut self, id: &str, text: &str, aggregate: bool, out: &mut Out) {
        if !aggregate && self.completed_messages.contains(id) {
            self.fail(out, "Codex sent text after the message completed");
            return;
        }
        let previous = self.messages.entry(id.to_owned()).or_insert_with(|| {
            out.begin_text();
            String::new()
        });
        let delta = if aggregate {
            if !text.starts_with(previous.as_str()) {
                self.fail(out, "Codex completed message differs from streamed text");
                return;
            }
            &text[previous.len()..]
        } else {
            text
        };
        out.text(delta);
        previous.push_str(delta);
        if aggregate {
            self.completed_messages.insert(id.into());
        }
    }
    fn item(&mut self, item: &Value, done: bool, out: &mut Out) {
        let id = item["id"].as_str().unwrap_or("");
        match item["type"].as_str().unwrap_or("") {
            "agentMessage" if done => {
                if id.is_empty() {
                    self.fail(out, "Codex message has no identity");
                    return;
                }
                self.message(id, item["text"].as_str().unwrap_or(""), true, out);
            }
            "reasoning" if done && !self.reasoning_streamed => {
                if let Some(parts) = item["summary"].as_array() {
                    for part in parts {
                        if let Some(t) = part.as_str() {
                            out.reasoning(t);
                        }
                    }
                }
            }
            "commandExecution" => {
                let command = item["command"].as_str().unwrap_or("");
                if done {
                    self.api_calls += 1;
                    let output = item["aggregatedOutput"].as_str().map(str::to_owned);
                    let ok = item["exitCode"].as_i64().map(|v| v == 0);
                    out.tool_end(id, ok, output.clone());
                    if ok == Some(false)
                        && output.as_deref().is_some_and(|s| {
                            let s = s.to_lowercase();
                            s.contains("permission denied")
                                || s.contains("read-only")
                                || s.contains("operation not permitted")
                        })
                    {
                        out.denial(Some(id.into()), command.into(), "sandbox_denied".into());
                    }
                } else {
                    out.tool_start(id, ToolKind::Command, command.into(), None);
                }
            }
            // Planning never permits file writes, including a protocol that claims they succeeded.
            "fileChange" => self.fail(out, "Read-only Codex planner reported a file modification"),
            "mcpToolCall" | "dynamicToolCall" | "webSearch" => {
                if done {
                    out.tool_end(id, Some(item["status"] != "failed"), None);
                } else {
                    out.tool_start(
                        id,
                        ToolKind::Other,
                        item["tool"]
                            .as_str()
                            .or_else(|| item["query"].as_str())
                            .unwrap_or("tool")
                            .into(),
                        None,
                    );
                }
            }
            _ => {}
        }
    }
}
impl Parser for Stream {
    fn interactive(&self) -> bool {
        true
    }
    fn take_input(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.outgoing)
    }
    fn complete(&self) -> bool {
        self.complete
    }
    fn finish(&mut self, out: &mut Out) {
        if !self.complete {
            self.fail(out, "Codex app-server disconnected before turn completion");
        }
    }
    fn line(&mut self, v: &Value, out: &mut Out) {
        if self.complete {
            if v.get("error").is_some() || v["method"] == "error" {
                out.fail("Codex reported a protocol error after completion");
            }
            return;
        }
        if let Some(id) = v["id"].as_u64().filter(|_| v.get("method").is_none()) {
            if let Some(e) = v.get("error") {
                self.fail(
                    out,
                    format!(
                        "Codex app-server: {}",
                        e["message"].as_str().unwrap_or("request failed")
                    ),
                );
                return;
            }
            if id != self.next_response {
                self.fail(
                    out,
                    "Codex app-server returned an unexpected or duplicate response",
                );
                return;
            }
            self.next_response += 1;
            match id {
                1 => {
                    self.outgoing
                        .push(json!({"method":"initialized","params":{}}));
                    self.outgoing.push(json!({"id":2,"method":if self.req.resume.is_some(){"thread/resume"}else{"thread/start"},"params":self.thread_params()}));
                }
                2 => {
                    let Some(id) = v["result"]["thread"]["id"].as_str() else {
                        self.fail(out, "Codex thread response has no identity");
                        return;
                    };
                    if self
                        .req
                        .resume
                        .as_deref()
                        .is_some_and(|expected| expected != id)
                    {
                        self.fail(out, "Codex resumed a different thread");
                        return;
                    }
                    // A persisted thread or user profile must not widen this read-only invocation.
                    if v["result"]["approvalPolicy"] != "never"
                        || v["result"]["sandbox"]["type"] != "readOnly"
                    {
                        self.fail(out, "Codex did not confirm the read-only planning policy");
                        return;
                    }
                    if let Some(model) = v["result"]["model"].as_str() {
                        out.emit(EventKind::Model { name: model.into() });
                    }
                    self.thread = Some(id.into());
                    out.emit(EventKind::Thread { id: id.into() });
                    let mut p = json!({"threadId":id,"cwd":self.req.cwd,"model":self.req.model,"approvalPolicy":"never",
                        "sandboxPolicy":{"type":"readOnly"},"input":[{"type":"text","text":self.prompt}]});
                    if let Some(e) = &self.req.effort {
                        p["effort"] = json!(e);
                    }
                    if self.req.fast {
                        p["serviceTier"] = json!("fast");
                    }
                    self.outgoing
                        .push(json!({"id":3,"method":"turn/start","params":p}));
                }
                3 => {
                    if let Some(id) = v["result"]["turn"]["id"].as_str() {
                        if self.turn.as_deref().is_some_and(|expected| expected != id) {
                            self.fail(out, "Codex turn identity changed");
                            return;
                        }
                        self.turn = Some(id.into());
                    } else {
                        self.fail(out, "Codex turn response has no identity");
                    }
                }
                _ => {}
            }
            return;
        }
        // Never answer unexpected server requests with approval or user input.
        if v.get("id").is_some() && v.get("method").is_some() {
            self.outgoing.push(json!({"id":v["id"],"error":{"code":-32601,"message":"Tokage read-only planning does not authorize this request"}}));
            self.fail(
                out,
                format!(
                    "Codex requested unsupported interaction: {}",
                    v["method"].as_str().unwrap_or("unknown")
                ),
            );
            return;
        }
        let p = &v["params"];
        if v["method"] == "turn/started" && p["threadId"].as_str() == self.thread.as_deref() {
            if let Some(id) = p["turn"]["id"].as_str() {
                if self.turn.as_deref().is_some_and(|expected| expected != id) {
                    self.fail(out, "Codex turn identity changed");
                    return;
                }
                self.turn = Some(id.into());
            }
        }
        if !self.matches_turn(p) {
            return;
        }
        match v["method"].as_str().unwrap_or("") {
            "item/agentMessage/delta" => {
                let Some(id) = p["itemId"].as_str() else {
                    self.fail(out, "Codex text delta has no item identity");
                    return;
                };
                self.message(id, p["delta"].as_str().unwrap_or(""), false, out);
            }
            "item/reasoning/summaryTextDelta" => {
                self.reasoning_streamed = true;
                out.reasoning(p["delta"].as_str().unwrap_or(""));
            }
            "item/started" => self.item(&p["item"], false, out),
            "item/completed" => self.item(&p["item"], true, out),
            "thread/tokenUsage/updated" => self.usage = Some(p["tokenUsage"]["last"].clone()),
            "error" => self.fail(
                out,
                p["error"]["message"].as_str().unwrap_or("Codex turn error"),
            ),
            "turn/completed" => {
                if p["turn"]["status"] != "completed" {
                    self.fail(
                        out,
                        p["turn"]["error"]["message"]
                            .as_str()
                            .unwrap_or("Codex turn did not complete"),
                    );
                    return;
                }
                if self
                    .messages
                    .keys()
                    .any(|id| !self.completed_messages.contains(id))
                {
                    self.fail(out, "Codex completed without final message evidence");
                    return;
                }
                let Some(u) = &self.usage else {
                    self.fail(out, "Codex completed without structured usage evidence");
                    return;
                };
                let (Some(input), Some(output), Some(cached)) = (
                    u["inputTokens"].as_u64(),
                    u["outputTokens"].as_u64(),
                    u["cachedInputTokens"].as_u64(),
                ) else {
                    self.fail(out, "Codex usage evidence is invalid");
                    return;
                };
                out.emit(EventKind::Done {
                    input_tokens: input,
                    output_tokens: output,
                    cache_read_tokens: cached,
                    cache_write_tokens: u["cacheWriteInputTokens"].as_u64().unwrap_or(0),
                    api_calls: Some(self.api_calls),
                    context_tokens: Some(input),
                    cost_usd: None,
                    premium_requests: None,
                    credits: None,
                    source: tokage_core::UsageSource::Structured,
                });
                self.complete = true;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{
        testing::{request, text},
        Ev, Role,
    };
    use tokage_core::ProviderId;
    fn ready() -> (Box<dyn Parser>, Out) {
        let mut req = request(ProviderId::CodexCli);
        req.role = Some(Role::Planner);
        req.effort = Some("medium".into());
        let mut parser = launch(&req, "prompt".into()).parser;
        let mut out = Out::default();
        parser.line(&json!({"id":1,"result":{}}), &mut out);
        let input = parser.take_input();
        assert_eq!(input[1]["params"]["sandbox"], "read-only");
        parser.line(&json!({"id":2,"result":{"thread":{"id":"thread"},"approvalPolicy":"never","sandbox":{"type":"readOnly"}}}),&mut out);
        let input = parser.take_input();
        assert_eq!(input[0]["params"]["approvalPolicy"], "never");
        assert_eq!(input[0]["params"]["sandboxPolicy"]["type"], "readOnly");
        assert_eq!(input[0]["params"]["effort"], "medium");
        parser.line(&json!({"id":3,"result":{"turn":{"id":"turn"}}}), &mut out);
        (parser, out)
    }
    fn notify(parser: &mut dyn Parser, out: &mut Out, method: &str, mut params: Value) {
        params["threadId"] = json!("thread");
        params["turnId"] = json!("turn");
        parser.line(&json!({"method":method,"params":params}), out);
    }
    fn complete(parser: &mut dyn Parser, out: &mut Out) {
        notify(
            parser,
            out,
            "thread/tokenUsage/updated",
            json!({"tokenUsage":{"last":{"inputTokens":42,"outputTokens":7,"cachedInputTokens":3}}}),
        );
        notify(
            parser,
            out,
            "turn/completed",
            json!({"turn":{"id":"turn","status":"completed"}}),
        );
    }
    #[test]
    fn deltas_arrive_before_done_and_completed_aggregate_never_duplicates_text() {
        let (mut p, mut o) = ready();
        notify(
            p.as_mut(),
            &mut o,
            "item/agentMessage/delta",
            json!({"itemId":"first","delta":"Reading."}),
        );
        notify(
            p.as_mut(),
            &mut o,
            "item/completed",
            json!({"item":{"id":"first","type":"agentMessage","text":"Reading."}}),
        );
        notify(
            p.as_mut(),
            &mut o,
            "item/agentMessage/delta",
            json!({"itemId":"plan","delta":"# Spec\n"}),
        );
        assert_eq!(text(&o.evs), "Reading.\n\n# Spec\n");
        assert!(!p.complete());
        p.line(&json!({"method":"item/agentMessage/delta","params":{"threadId":"other","turnId":"turn","itemId":"plan","delta":"wrong"}}),&mut o);
        notify(
            p.as_mut(),
            &mut o,
            "item/agentMessage/delta",
            json!({"itemId":"plan","delta":"## Goals\nBuild"}),
        );
        notify(
            p.as_mut(),
            &mut o,
            "item/completed",
            json!({"item":{"id":"plan","type":"agentMessage","text":"# Spec\n## Goals\nBuild"}}),
        );
        complete(p.as_mut(), &mut o);
        assert_eq!(text(&o.evs), "Reading.\n\n# Spec\n## Goals\nBuild");
        assert!(p.complete());
        assert!(matches!(
            o.evs.last(),
            Some(Ev::Emit(EventKind::Done {
                input_tokens: 42,
                output_tokens: 7,
                cache_read_tokens: 3,
                ..
            }))
        ));
    }
    #[test]
    fn changed_aggregate_missing_usage_and_failed_turn_never_emit_success() {
        for mode in [
            "changed",
            "no-usage",
            "failed",
            "disconnect",
            "file-write",
            "approval",
            "missing-aggregate",
        ] {
            let (mut p, mut o) = ready();
            notify(
                p.as_mut(),
                &mut o,
                "item/agentMessage/delta",
                json!({"itemId":"plan","delta":"# Spec"}),
            );
            if mode == "no-usage" {
                notify(
                    p.as_mut(),
                    &mut o,
                    "item/completed",
                    json!({"item":{"id":"plan","type":"agentMessage","text":"# Spec"}}),
                );
            }
            match mode {
                "changed" => notify(
                    p.as_mut(),
                    &mut o,
                    "item/completed",
                    json!({"item":{"id":"plan","type":"agentMessage","text":"different"}}),
                ),
                "no-usage" => notify(
                    p.as_mut(),
                    &mut o,
                    "turn/completed",
                    json!({"turn":{"id":"turn","status":"completed"}}),
                ),
                "failed" => notify(
                    p.as_mut(),
                    &mut o,
                    "turn/completed",
                    json!({"turn":{"id":"turn","status":"failed","error":{"message":"failed"}}}),
                ),
                "disconnect" => p.finish(&mut o),
                "missing-aggregate" => complete(p.as_mut(), &mut o),
                "file-write" => notify(
                    p.as_mut(),
                    &mut o,
                    "item/completed",
                    json!({"item":{"id":"edit","type":"fileChange"}}),
                ),
                _ => p.line(
                    &json!({"id":100,"method":"item/commandExecution/requestApproval","params":{}}),
                    &mut o,
                ),
            }
            assert!(o.evs.iter().any(|e| matches!(e, Ev::Failed(_))), "{mode}");
            assert!(
                !o.evs
                    .iter()
                    .any(|e| matches!(e, Ev::Emit(EventKind::Done { .. }))),
                "{mode}"
            );
            if mode == "approval" {
                let replies = p.take_input();
                assert!(replies[0].get("error").is_some());
                assert!(replies[0].get("result").is_none());
            }
        }
    }
    #[test]
    fn resumed_profile_cannot_widen_planning_sandbox() {
        let mut req = request(ProviderId::CodexCli);
        req.role = Some(Role::Planner);
        req.resume = Some("thread".into());
        req.fast = true;
        let mut p = launch(&req, "prompt".into()).parser;
        let mut o = Out::default();
        p.line(&json!({"id":1,"result":{}}), &mut o);
        let input = p.take_input();
        assert_eq!(input[1]["method"], "thread/resume");
        assert_eq!(input[1]["params"]["sandbox"], "read-only");
        p.line(&json!({"id":2,"result":{"thread":{"id":"thread"},"approvalPolicy":"never","sandbox":{"type":"dangerFullAccess"}}}),&mut o);
        assert!(p.complete());
        assert!(p.take_input().is_empty());
        assert!(o.evs.iter().any(|e| matches!(e, Ev::Failed(_))));
    }
    #[test]
    fn late_protocol_errors_are_not_hidden_by_completed_frame() {
        let (mut p, mut o) = ready();
        complete(p.as_mut(), &mut o);
        p.line(
            &json!({"method":"error","params":{"error":{"message":"late failure"}}}),
            &mut o,
        );
        assert!(o.evs.iter().any(|e| matches!(e, Ev::Failed(_))));
    }
    #[test]
    fn duplicate_thread_response_never_starts_another_model_turn() {
        let (mut p, mut o) = ready();
        p.line(&json!({"id":2,"result":{"thread":{"id":"thread"},"approvalPolicy":"never","sandbox":{"type":"readOnly"}}}), &mut o);
        assert!(p.complete());
        assert!(p.take_input().is_empty());
        assert!(o.evs.iter().any(|e| matches!(e, Ev::Failed(_))));
    }
}
