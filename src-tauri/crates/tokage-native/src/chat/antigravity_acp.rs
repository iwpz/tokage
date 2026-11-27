//! Antigravity's public ACP v1 stdio interface. The CLI owns login, tools and history.
use super::{ChatRequest, EventKind, Launch, Out, Parser, Role, ToolKind};
use crate::{env::ShellEnv, providers::ModelInfo};
use serde_json::{json, Value};
use std::{collections::HashSet, path::Path, process::Stdio, time::Duration};
use tokage_core::{AuthMode, ProviderId, UsageSource};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub(crate) const ACCOUNT_LABEL: &str = "Google account (ACP)";
const VERIFY_REPLY: &str = "TOKAGE_KEY_OK";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Auth {
    Google,
    GeminiKey,
}

pub(crate) fn auth(env: &ShellEnv) -> Result<Auth, String> {
    if crate::providers::antigravity_api_key_mode(env) {
        crate::providers::require_antigravity_api_key(env)?;
        Ok(Auth::GeminiKey)
    } else {
        Ok(Auth::Google)
    }
}

pub(crate) fn personal_turn(req: &ChatRequest) -> bool {
    req.desktop_user_turn
        && req.direct_execution
        && req.mode == super::TurnMode::Direct
        && req.effective_role() == Role::Executor
        && req.phase.is_none()
        && req.plan_id.is_none()
        && req.task_id.is_none()
        && req.task_code.is_none()
        && req.context_compact.is_none()
        && req.output_schema.is_none()
        && req.auth_mode != Some(AuthMode::FullAuto)
        && req
            .debug
            .as_ref()
            .is_none_or(|d| d.output == super::debug::DebugOutput::Off)
}

pub(crate) fn configure(
    req: &mut ChatRequest,
    env: &ShellEnv,
    provider: &crate::providers::ProviderInfo,
) -> Result<(), String> {
    let auth = auth(env)?;
    if (provider.auth_method.as_deref() == Some(ACCOUNT_LABEL)) != (auth == Auth::Google) {
        return Err(
            "Antigravity authentication changed. Refresh CLI management before sending.".into(),
        );
    }
    if auth == Auth::Google && !personal_turn(req) {
        return Err("Antigravity requires a Gemini API key for automation, context generation, and Full-auto. Google accounts are available for interactive desktop Direct work.".into());
    }
    req.antigravity_auth = Some(auth);
    Ok(())
}

pub(crate) fn launch(req: &ChatRequest, prompt: String) -> Launch {
    let auth = req.antigravity_auth.unwrap_or(Auth::GeminiKey);
    let role = req.effective_role();
    let mode = if role.is_read_only() {
        "plan"
    } else {
        "accept-edits"
    };
    let mut args = vec!["acp".into()];
    if auth == Auth::GeminiKey
        && req.auth_mode == Some(AuthMode::FullAuto)
        && role == Role::Executor
    {
        args.push("--dangerously-skip-permissions".into());
    }
    Launch {
        args,
        stdin: Some(initialize()),
        env_remove: env_remove(auth),
        parser: Box::new(Acp::new(
            &req.cwd,
            req.resume.clone(),
            auth,
            Some(Turn {
                prompt: if auth == Auth::Google {
                    // Only send the user-authored text. Native resume owns earlier messages.
                    req.conversation
                        .as_ref()
                        .and_then(|m| m["userText"].as_str())
                        .unwrap_or(&req.prompt)
                        .into()
                } else {
                    prompt
                },
                model: Some(req.model.clone()),
                effort: req.effort.clone(),
                mode,
            }),
        )),
    }
}

fn env_remove(auth: Auth) -> Vec<String> {
    // Keep the CLI's normal login store. An ambient API key or gateway must not change
    // a turn the desktop presented as Google-account authentication.
    if auth == Auth::Google {
        [
            "GEMINI_API_KEY",
            "GOOGLE_API_KEY",
            "GOOGLE_GEMINI_BASE_URL",
            "AGY_ADC_AUTH",
        ]
        .into_iter()
        .map(str::to_string)
        .collect()
    } else {
        ["GOOGLE_API_KEY", "AGY_ADC_AUTH"]
            .into_iter()
            .map(str::to_string)
            .collect()
    }
}

fn initialize() -> String {
    format!(
        "{}\n",
        request(
            1,
            "initialize",
            json!({
                "protocolVersion": 1,
                "clientInfo": {"name": "tokage", "title": "Tokage", "version": env!("CARGO_PKG_VERSION")},
                "clientCapabilities": {}
            })
        )
    )
}

fn request(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Initialize,
    Authenticate,
    Open,
    Config,
    Prompt,
    Done,
}

struct Turn {
    prompt: String,
    model: Option<String>,
    effort: Option<String>,
    mode: &'static str,
}

struct Acp {
    auth: Auth,
    cwd: String,
    resume: Option<String>,
    turn: Option<Turn>,
    session: Option<String>,
    step: Step,
    id: u64,
    next: Vec<Value>,
    options: Vec<Value>,
    selections: std::collections::VecDeque<(String, String)>,
    models: Vec<ModelInfo>,
    pending_option: Option<(String, String)>,
    tools: HashSet<String>,
    context_tokens: Option<u64>,
    saw_text: bool,
}

impl Acp {
    fn new(cwd: &str, resume: Option<String>, auth: Auth, turn: Option<Turn>) -> Self {
        Self {
            auth,
            cwd: cwd.into(),
            resume,
            turn,
            session: None,
            step: Step::Initialize,
            id: 1,
            next: vec![],
            options: vec![],
            selections: Default::default(),
            models: vec![],
            pending_option: None,
            tools: HashSet::new(),
            context_tokens: None,
            saw_text: false,
        }
    }

    fn send(&mut self, step: Step, method: &str, params: Value) {
        self.step = step;
        self.id += 1;
        self.next.push(request(self.id, method, params));
    }

    fn open(&mut self) {
        let (method, params) = match &self.resume {
            Some(session) => (
                "session/resume",
                json!({"sessionId": session, "cwd": self.cwd, "mcpServers": []}),
            ),
            None => ("session/new", json!({"cwd": self.cwd, "mcpServers": []})),
        };
        self.send(Step::Open, method, params);
    }

    fn fail(&mut self, out: &mut Out, message: &str) {
        self.step = Step::Done;
        self.next.clear();
        out.fail(message);
    }

    fn rpc(&mut self, frame: &Value, out: &mut Out) {
        if self.step == Step::Done || frame["id"].as_u64() != Some(self.id) {
            return;
        }
        if let Some(error) = frame.get("error") {
            let message = if error["code"].as_i64() == Some(-32000) {
                match self.auth {
                    Auth::Google => "Antigravity requires login. Run agy acp --login or sign in using agy, then refresh CLI management.",
                    Auth::GeminiKey => "Antigravity did not accept Gemini API key authentication.",
                }
            } else {
                // Agent diagnostics may include local credential details. Never expose them.
                match self.step {
                    Step::Initialize => "Antigravity failed to initialize ACP.",
                    Step::Authenticate => "Antigravity did not accept Gemini API key authentication.",
                    Step::Open if self.resume.is_some() => "Antigravity could not resume this session. Open a new chat or restore the original CLI account.",
                    Step::Open => "Antigravity could not open an ACP session.",
                    Step::Config => "Antigravity rejected the selected model, reasoning level, or permission mode.",
                    _ => "Antigravity ACP reported a failed turn.",
                }
            };
            self.fail(out, message);
            return;
        }
        let Some(result) = frame.get("result").filter(|r| r.is_object()) else {
            self.fail(out, "Invalid Antigravity ACP response.");
            return;
        };
        match self.step {
            Step::Initialize => {
                let supported = result["agentInfo"]["version"].as_str().and_then(|version| {
                    super::adapter::adapter(ProviderId::AntigravityCli)?
                        .meta()
                        .versions
                        .contains(version)
                }) == Some(true);
                if result["protocolVersion"].as_u64() != Some(1)
                    || result["agentInfo"]["name"].as_str() != Some("Antigravity ACP Server")
                    || !supported
                {
                    self.fail(
                        out,
                        "Antigravity ACP v1 is required. Update the official agy CLI to a supported release.",
                    );
                } else if self.auth == Auth::GeminiKey {
                    let offered = result["authMethods"].as_array().is_some_and(|methods| {
                        methods
                            .iter()
                            .any(|method| method["id"] == "gemini-api-key")
                    });
                    if offered {
                        self.send(
                            Step::Authenticate,
                            "authenticate",
                            json!({"methodId": "gemini-api-key"}),
                        );
                    } else {
                        self.fail(out, "The Antigravity ACP server does not offer Gemini API key authentication.");
                    }
                } else {
                    self.open();
                }
            }
            Step::Authenticate => self.open(),
            Step::Open => self.opened(result, out),
            Step::Config => {
                let acknowledged = self.pending_option.take().is_some_and(|(id, wanted)| {
                    result["configOptions"].as_array().is_some_and(|options| {
                        options
                            .iter()
                            .any(|o| o["id"] == id && o["currentValue"] == wanted)
                    })
                });
                if !acknowledged {
                    self.fail(
                        out,
                        "Antigravity did not confirm the requested ACP session option.",
                    );
                    return;
                }
                self.options = result["configOptions"]
                    .as_array()
                    .expect("confirmed ACP options")
                    .clone();
                self.advance(out);
            }
            Step::Prompt => {
                if result["stopReason"] != "end_turn" || !self.saw_text {
                    self.fail(out, "Antigravity ended without a complete reply.");
                    return;
                }
                let usage = &result["usage"];
                let input = usage["inputTokens"].as_u64();
                let output = usage["outputTokens"].as_u64();
                out.emit(EventKind::Done {
                    input_tokens: input.unwrap_or_default(),
                    output_tokens: output.unwrap_or_default(),
                    cache_read_tokens: usage["cachedReadTokens"].as_u64().unwrap_or_default(),
                    cache_write_tokens: usage["cachedWriteTokens"].as_u64().unwrap_or_default(),
                    api_calls: None,
                    context_tokens: self.context_tokens,
                    cost_usd: None,
                    premium_requests: None,
                    credits: None,
                    source: if input.is_some() && output.is_some() {
                        UsageSource::Structured
                    } else {
                        UsageSource::Estimated
                    },
                });
                self.step = Step::Done;
            }
            Step::Done => {}
        }
    }

    fn opened(&mut self, result: &Value, out: &mut Out) {
        let id = result["sessionId"].as_str().or(self.resume.as_deref());
        let Some(id) =
            id.filter(|id| !id.is_empty() && self.resume.as_deref().is_none_or(|old| old == *id))
        else {
            self.fail(out, "Antigravity ACP returned an unexpected session ID.");
            return;
        };
        self.session = Some(id.into());
        if let Some(options) = result["configOptions"].as_array() {
            self.options = options.clone();
        }
        if let Some(options) = self.option("model", "model") {
            self.models = option_values(options)
                .filter_map(|o| {
                    let id = o["value"].as_str()?.to_string();
                    Some(ModelInfo {
                        name: o["name"].as_str().unwrap_or(&id).to_string(),
                        id,
                    })
                })
                .collect();
        }
        let Some(turn) = &self.turn else {
            self.step = Step::Done;
            return;
        };
        let mut effort = turn.effort.clone();
        if let Some(model) = &turn.model {
            let mut selected = model.as_str();
            if !self.models.iter().any(|m| m.id == selected) {
                if let Some((name, suffix)) = model.rsplit_once('-') {
                    if ["low", "medium", "high", "xhigh", "max"].contains(&suffix)
                        && self.models.iter().any(|m| m.id == name)
                    {
                        selected = name;
                        effort.get_or_insert_with(|| suffix.into());
                    }
                }
            }
            self.selections.push_back(("model".into(), selected.into()));
        }
        if let Some(effort) = effort {
            self.selections.push_back(("thought_level".into(), effort));
        }
        self.selections.push_back(("mode".into(), turn.mode.into()));
        self.advance(out);
    }

    fn option(&self, id: &str, category: &str) -> Option<&Value> {
        self.options
            .iter()
            .find(|o| o["id"] == id || o["category"] == category)
    }

    fn advance(&mut self, out: &mut Out) {
        let session = self.session.clone().expect("ACP session opened");
        while let Some((kind, value)) = self.selections.pop_front() {
            let option = self.option(&kind, &kind).or_else(|| {
                (kind == "thought_level")
                    .then(|| self.option("thinking_level", "thought_level"))
                    .flatten()
            });
            let Some(option) = option else {
                self.fail(
                    out,
                    "Antigravity did not advertise a required ACP session option.",
                );
                return;
            };
            if option["currentValue"] == value {
                continue;
            }
            if !option_values(option).any(|o| o["value"] == value) {
                self.fail(out, "The requested Antigravity model, reasoning level, or mode is unavailable for this account.");
                return;
            }
            let Some(id) = option["id"].as_str() else {
                self.fail(out, "Antigravity returned an invalid ACP session option.");
                return;
            };
            let params = json!({"sessionId": session, "configId": id, "value": value});
            self.pending_option = Some((id.into(), value));
            self.send(Step::Config, "session/set_config_option", params);
            return;
        }
        out.emit(EventKind::Thread {
            id: session.clone(),
        });
        if let Some(model) = self
            .option("model", "model")
            .and_then(|option| option["currentValue"].as_str())
        {
            out.emit(EventKind::Model { name: model.into() });
        }
        let prompt = &self.turn.as_ref().expect("ACP inference turn").prompt;
        self.send(
            Step::Prompt,
            "session/prompt",
            json!({
                "sessionId": session, "prompt": [{"type": "text", "text": prompt}]
            }),
        );
    }

    fn update(&mut self, params: &Value, out: &mut Out) {
        if self.step != Step::Prompt || params["sessionId"].as_str() != self.session.as_deref() {
            return;
        }
        let update = &params["update"];
        let text = update["content"]["text"].as_str().unwrap_or_default();
        match update["sessionUpdate"].as_str() {
            Some("agent_message_chunk") => {
                self.saw_text |= !text.trim().is_empty();
                out.text(text);
            }
            Some("agent_thought_chunk") => out.reasoning(text),
            Some("usage_update") => self.context_tokens = update["used"].as_u64(),
            Some("tool_call" | "tool_call_update") => {
                let Some(id) = update["toolCallId"].as_str() else {
                    return;
                };
                if self.tools.insert(id.into()) {
                    let tool = match update["kind"].as_str() {
                        Some("read") => ToolKind::Read,
                        Some("edit" | "delete" | "move") => ToolKind::Edit,
                        Some("search") => ToolKind::Search,
                        Some("execute") => ToolKind::Command,
                        Some("fetch") => ToolKind::Web,
                        _ => ToolKind::Other,
                    };
                    out.tool_start(
                        id,
                        tool,
                        update["title"]
                            .as_str()
                            .unwrap_or("Antigravity tool")
                            .into(),
                        None,
                    );
                }
                let status = update["status"].as_str();
                if matches!(status, Some("completed" | "failed")) {
                    out.tool_end(id, Some(status == Some("completed")), None);
                }
            }
            _ => {}
        }
    }
}

fn option_values(option: &Value) -> impl Iterator<Item = &Value> {
    option["options"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|entry| {
            let group = entry["options"].as_array();
            group
                .into_iter()
                .flatten()
                .chain(group.is_none().then_some(entry))
        })
}

impl Parser for Acp {
    fn line(&mut self, frame: &Value, out: &mut Out) {
        if frame["jsonrpc"] != "2.0" {
            return;
        }
        if let Some(method) = frame["method"].as_str() {
            if method == "session/update" && frame.get("id").is_none() {
                if self.step != Step::Done {
                    self.update(&frame["params"], out);
                }
            } else if let Some(id) = frame.get("id") {
                // The adapter advertises no host tools. Permission requests needing interaction
                // are declined until Tokage can bind a live approval to the original ACP call.
                let response = if method == "session/request_permission" {
                    out.emit(EventKind::Warning { message: "Antigravity requested a tool permission that requires interactive approval. Use the native agy terminal to approve this action.".into() });
                    json!({"result": {"outcome": {"outcome": "cancelled"}}})
                } else {
                    json!({"error": {"code": -32601, "message": "Tokage does not provide this ACP client capability"}})
                };
                let mut response = response;
                response["jsonrpc"] = json!("2.0");
                response["id"] = id.clone();
                self.next.push(response);
            }
        } else {
            self.rpc(frame, out);
        }
    }

    fn finish(&mut self, out: &mut Out) {
        if !self.complete() {
            self.fail(
                out,
                "Antigravity disconnected before finishing the ACP request.",
            );
        }
    }
    fn interactive(&self) -> bool {
        true
    }
    fn take_input(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.next)
    }
    fn complete(&self) -> bool {
        self.step == Step::Done
    }
}

/// User-requested setup check. Opening a personal session lists models without
/// a prompt. API key verification preserves the explicit small inference check.
pub(crate) async fn verify(
    exe: &Path,
    env: &ShellEnv,
    auth: Auth,
) -> Result<Vec<ModelInfo>, String> {
    let workspace = tempfile::tempdir().map_err(|_| "Cannot prepare Antigravity verification")?;
    let cwd = workspace
        .path()
        .canonicalize()
        .map_err(|_| "Cannot open Antigravity verification workspace")?;
    let mut cmd = crate::providers::auth_command(ProviderId::AntigravityCli, exe, &["acp"], env)
        .ok_or("Cannot start Antigravity ACP verification")?;
    super::apply_env(
        &mut cmd,
        ProviderId::AntigravityCli,
        env,
        &env_remove(auth),
        None,
    );
    cmd.current_dir(&cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    crate::proc::isolate(&mut cmd);
    let mut child = cmd
        .spawn()
        .map_err(|_| "Could not start Antigravity ACP verification")?;
    let group = crate::proc::Group::new(&child);
    let result = tokio::time::timeout(Duration::from_secs(35), async {
        let mut input = child
            .stdin
            .take()
            .ok_or("Antigravity ACP stdin unavailable")?;
        let mut lines = BufReader::new(
            child
                .stdout
                .take()
                .ok_or("Antigravity ACP stdout unavailable")?,
        )
        .lines();
        let turn = (auth == Auth::GeminiKey).then(|| Turn {
            prompt: format!(
                "Reply with exactly {VERIFY_REPLY}. Do not use tools or read or write files."
            ),
            model: None,
            effort: None,
            mode: "plan",
        });
        let mut parser = Acp::new(&cwd.to_string_lossy(), None, auth, turn);
        input
            .write_all(initialize().as_bytes())
            .await
            .map_err(|_| "Could not initialize Antigravity ACP")?;
        let mut out = Out::default();
        let mut reply = String::new();
        let mut completed = false;
        while !parser.complete() {
            let line = lines
                .next_line()
                .await
                .map_err(|_| "Could not read Antigravity ACP")?
                .ok_or("Antigravity disconnected during verification")?;
            if let Ok(frame) = serde_json::from_str(&line) {
                parser.line(&frame, &mut out);
            }
            for ev in out.evs.drain(..) {
                match ev {
                    super::Ev::Failed(msg) => return Err(msg),
                    super::Ev::Emit(EventKind::Text { text }) if reply.len() < 256 => {
                        reply.push_str(&text)
                    }
                    super::Ev::Emit(EventKind::Done { .. }) => completed = true,
                    _ => {}
                }
            }
            for message in parser.take_input() {
                input
                    .write_all(format!("{message}\n").as_bytes())
                    .await
                    .map_err(|_| "Could not send Antigravity ACP request")?;
            }
        }
        drop(input);
        if !child
            .wait()
            .await
            .map_err(|_| "Antigravity ACP did not exit cleanly")?
            .success()
        {
            return Err("Antigravity ACP exited with an error".into());
        }
        if parser.models.is_empty()
            || (auth == Auth::GeminiKey && (!completed || reply.trim() != VERIFY_REPLY))
        {
            return Err("Antigravity did not confirm authentication and model access.".into());
        }
        Ok(parser.models)
    })
    .await;
    group.kill().await;
    let _ = child.kill().await;
    result.map_err(|_| "Antigravity ACP verification timed out".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{testing, Ev, TurnMode};

    fn queue_reply(parser: &mut Acp, out: &mut Out, result: Value) -> Vec<Value> {
        parser.line(
            &json!({"jsonrpc": "2.0", "id": parser.id, "result": result}),
            out,
        );
        parser.take_input()
    }

    fn hello() -> Value {
        json!({
            "protocolVersion": 1,
            "agentInfo": {"name": "Antigravity ACP Server", "version": "1.5.17"},
            "authMethods": [{"id": "oauth-personal"}, {"id": "gemini-api-key"}]
        })
    }

    fn options(mode: &str) -> Value {
        json!([
            {"id": "model", "category": "model", "currentValue": "gemini-flash", "options": [{"value": "gemini-flash", "name": "Gemini Flash"}]},
            {"id": "thought_level", "currentValue": "medium", "options": [{"value": "medium"}, {"value": "low"}]},
            {"id": "mode", "category": "mode", "currentValue": mode, "options": [{"value": "plan"}, {"value": "accept-edits"}]}
        ])
    }

    fn turn(mode: &'static str) -> Turn {
        Turn {
            prompt: "hello".into(),
            model: Some("gemini-flash".into()),
            effort: None,
            mode,
        }
    }

    #[test]
    fn google_account_requires_desktop_user_message_and_rejects_automation() {
        let mut req = testing::request(ProviderId::AntigravityCli);
        req.mode = TurnMode::Direct;
        req.desktop_user_turn = true;
        req.direct_execution = true;
        assert!(personal_turn(&req));
        req.phase = Some("CONTEXT".into());
        assert!(!personal_turn(&req));
        req.phase = None;
        req.auth_mode = Some(AuthMode::FullAuto);
        assert!(!personal_turn(&req));
        req.auth_mode = None;
        req.desktop_user_turn = false;
        assert!(!personal_turn(&req));
    }

    #[test]
    fn resume_streams_only_the_owned_session_and_finishes_with_usage() {
        let mut parser = Acp::new(
            "/workspace",
            Some("ag1".into()),
            Auth::Google,
            Some(turn("accept-edits")),
        );
        let mut out = Out::default();
        let resume = queue_reply(&mut parser, &mut out, hello()).remove(0);
        assert_eq!(resume["method"], "session/resume");
        assert_eq!(resume["params"]["sessionId"], "ag1");
        assert_eq!(resume["params"]["mcpServers"], json!([]));

        let set = queue_reply(
            &mut parser,
            &mut out,
            json!({"sessionId":"ag1", "configOptions": options("default")}),
        )
        .remove(0);
        assert_eq!(set["method"], "session/set_config_option");
        assert_eq!(set["params"]["value"], "accept-edits");
        let prompt = queue_reply(
            &mut parser,
            &mut out,
            json!({"configOptions": options("accept-edits")}),
        )
        .remove(0);
        assert_eq!(prompt["method"], "session/prompt");
        assert_eq!(prompt["params"]["prompt"][0]["text"], "hello");
        for (session, text) in [("stray", "wrong"), ("ag1", "reply")] {
            parser.line(&json!({"jsonrpc":"2.0", "method":"session/update", "params":{
                "sessionId":session, "update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":text}}
            }}), &mut out);
        }
        queue_reply(
            &mut parser,
            &mut out,
            json!({"stopReason":"end_turn","usage":{"inputTokens":24,"outputTokens":8}}),
        );
        assert!(parser.complete());
        assert!(out.evs.iter().any(|e| matches!(
            e,
            Ev::Emit(EventKind::Done {
                input_tokens: 24,
                output_tokens: 8,
                ..
            })
        )));
        assert!(!out
            .evs
            .iter()
            .any(|e| matches!(e, Ev::Emit(EventKind::Text { text }) if text == "wrong")));
    }

    #[test]
    fn key_auth_and_permission_mode_must_succeed_before_prompt() {
        let mut out = Out::default();
        let mut parser = Acp::new("/workspace", None, Auth::GeminiKey, Some(turn("plan")));
        let auth = queue_reply(&mut parser, &mut out, hello()).remove(0);
        assert_eq!(auth["method"], "authenticate");
        assert_eq!(auth["params"]["methodId"], "gemini-api-key");
        assert_eq!(
            queue_reply(&mut parser, &mut out, json!({}))[0]["method"],
            "session/new"
        );
        let set = queue_reply(
            &mut parser,
            &mut out,
            json!({"sessionId":"ag1", "configOptions":options("accept-edits")}),
        );
        assert_eq!(set[0]["params"]["value"], "plan");
        assert!(queue_reply(
            &mut parser,
            &mut out,
            json!({"configOptions": options("accept-edits")})
        )
        .is_empty());
        assert!(parser.complete());
        assert!(out.evs.iter().any(|e| matches!(e, Ev::Failed(_))));
    }

    #[test]
    fn verification_opens_a_google_session_without_prompting_or_auto_approving() {
        let mut out = Out::default();
        let mut parser = Acp::new("/workspace", None, Auth::Google, None);
        assert_eq!(
            queue_reply(&mut parser, &mut out, hello())[0]["method"],
            "session/new"
        );
        assert!(queue_reply(
            &mut parser,
            &mut out,
            json!({"sessionId":"ag1","configOptions":options("plan")})
        )
        .is_empty());
        assert!(parser.complete());
        assert_eq!(parser.models[0].id, "gemini-flash");
        let mut parser = Acp::new("/workspace", None, Auth::Google, Some(turn("accept-edits")));
        parser.line(
            &json!({"jsonrpc":"2.0","id":"tool-1","method":"session/request_permission"}),
            &mut out,
        );
        assert_eq!(
            parser.take_input()[0]["result"]["outcome"]["outcome"],
            "cancelled"
        );
    }
}
