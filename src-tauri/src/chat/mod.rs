//! Conversation turns (docs/02 §2 PLAN / DIRECT paths, first cut).
//!
//! Each turn runs on the CLI the user picked, headless with structured output (docs/01 §7.1).
//! Every adapter turns its CLI's event stream into one shared event model: reasoning, reply
//! text, tool steps and file changes. Diffs come from `files::Files`, so they look the same
//! whichever CLI made the edit.

mod child_env;
mod claude;
mod codex;
mod copilot;
mod cursor;
mod files;

use std::{collections::HashMap, path::PathBuf, process::Stdio};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::{
    env::ShellEnv,
    providers::{ProviderId, ProviderInfo, ProviderStatus},
};

pub const EVENT: &str = "chat-event";

/// Why a CLI can't take Direct turns with full access (docs/11 §4.2 interim rules, P1.m1).
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "code", rename_all = "SCREAMING_SNAKE_CASE", rename_all_fields = "camelCase")]
pub enum DirectLimit {
    /// `codex exec` never asks before running a command, so Direct runs in Codex's read-only
    /// sandbox until an approval channel exists (P2.m3d).
    CodexReadOnly,
    /// Cursor's permission config would let commands run without a prompt. Direct is
    /// refused; `detail` lists the offending settings.
    CursorUnsafePermissions { detail: Vec<String> },
}

/// Direct-mode limit for a CLI. `cwd` adds the project's own config where one applies.
pub fn direct_limit(id: ProviderId, env: &ShellEnv, cwd: Option<&str>) -> Option<DirectLimit> {
    match id {
        ProviderId::CodexCli => Some(DirectLimit::CodexReadOnly),
        ProviderId::CursorCli => {
            let detail = cursor::direct_risks(env, cwd);
            (!detail.is_empty()).then_some(DirectLimit::CursorUnsafePermissions { detail })
        }
        _ => None,
    }
}

/// CLIs with a headless adapter. Others stay out of the model picker.
pub fn supported(id: ProviderId) -> bool {
    matches!(
        id,
        ProviderId::ClaudeCode | ProviderId::CodexCli | ProviderId::CursorCli | ProviderId::CopilotCli
    )
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TurnMode {
    Plan,
    Direct,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRequest {
    pub turn_id: String,
    pub cwd: String,
    pub prompt: String,
    pub mode: TurnMode,
    pub provider: ProviderId,
    /// Concrete model id the CLI accepts.
    pub model: String,
    /// Reasoning effort for CLIs that take it as a parameter; `None` when the id names it.
    #[serde(default)]
    pub effort: Option<String>,
    /// Priority speed tier for CLIs that take it as a parameter.
    #[serde(default)]
    pub fast: bool,
    /// Routing profile for a CLI's auto model (Copilot `--auto-tier`).
    #[serde(default)]
    pub tier: Option<String>,
    /// Prior messages, replayed when the CLI has no session to resume (e.g. after a model switch).
    #[serde(default)]
    pub history: Vec<HistoryItem>,
    /// CLI session to continue, from an earlier `Thread` event of the same CLI and model.
    pub resume: Option<String>,
    /// Continue `resume` as a new CLI session, leaving the original untouched (a forked
    /// Tokage session). Only CLIs with a native fork honour it; the others never get it.
    #[serde(default)]
    pub fork: bool,
    /// CLIs the user allowed to receive workspace trust. Others are never spawned.
    #[serde(default)]
    pub trusted: Vec<ProviderId>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct HistoryItem {
    pub role: String,
    pub text: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatEvent {
    pub turn_id: String,
    #[serde(flatten)]
    pub kind: EventKind,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ToolKind {
    Command,
    Read,
    Search,
    Edit,
    Web,
    Agent,
    Other,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Change {
    Added,
    Modified,
    Deleted,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum EventKind {
    /// CLI session id to pass back as `resume` on the next turn.
    Thread { id: String },
    /// The model the CLI reports it is running (resolved alias, or the pick of an auto router).
    Model { name: String },
    /// Append to the current reasoning block.
    Reasoning { text: String },
    /// Append to the reply text.
    Text { text: String },
    ToolStart {
        id: String,
        tool: ToolKind,
        title: String,
        detail: Option<String>,
    },
    ToolEnd {
        id: String,
        ok: Option<bool>,
        output: Option<String>,
    },
    /// A file the turn created, edited or deleted. `diff` is unified, `None` when the
    /// previous content is unknown (outside git) or the file is too large.
    FileChange {
        tool_id: Option<String>,
        path: String,
        abs_path: String,
        /// Tokage's copy of a file outside the working directory, for the preview pane.
        preview_path: Option<String>,
        change: Change,
        diff: Option<String>,
        additions: u32,
        deletions: u32,
    },
    Done {
        input_tokens: u64,
        output_tokens: u64,
        cost_usd: Option<f64>,
        premium_requests: Option<f64>,
    },
    Error { message: String },
}

/// What an adapter knows about a file change; `files::Files` fills in the rest.
#[derive(Debug, PartialEq)]
pub struct FileTouch {
    pub tool_id: Option<String>,
    pub path: String,
    /// `Some(None)`: the CLI says the file is new. `None`: unknown, use the turn's snapshot.
    pub before: Option<Option<String>>,
    /// `None`: read it from disk.
    pub after: Option<String>,
    pub deleted: bool,
}

#[derive(Debug, PartialEq)]
pub enum Ev {
    Emit(EventKind),
    File(FileTouch),
    Failed(String),
}

/// Adapter output buffer. Tracks text boundaries so separate reply blocks don't run together.
#[derive(Default)]
pub struct Out {
    pub evs: Vec<Ev>,
    /// Some reply text was written this turn.
    any_text: bool,
    pending_break: bool,
}

impl Out {
    pub fn emit(&mut self, k: EventKind) {
        self.evs.push(Ev::Emit(k));
    }
    /// A new text block starts; separated from earlier reply text by a blank line.
    pub fn begin_text(&mut self) {
        if self.any_text {
            self.pending_break = true;
        }
    }
    pub fn text(&mut self, t: &str) {
        if t.is_empty() {
            return;
        }
        let text = if self.pending_break {
            format!("\n\n{t}")
        } else {
            t.to_string()
        };
        self.pending_break = false;
        self.any_text = true;
        self.evs.push(Ev::Emit(EventKind::Text { text }));
    }
    pub fn reasoning(&mut self, t: &str) {
        if !t.is_empty() {
            self.emit(EventKind::Reasoning { text: t.into() });
        }
    }
    pub fn tool_start(&mut self, id: &str, tool: ToolKind, title: String, detail: Option<String>) {
        self.emit(EventKind::ToolStart {
            id: id.into(),
            tool,
            title,
            detail,
        });
    }
    pub fn tool_end(&mut self, id: &str, ok: Option<bool>, output: Option<String>) {
        self.emit(EventKind::ToolEnd {
            id: id.into(),
            ok,
            output: output.map(|o| clip(&o, OUTPUT_MAX)).filter(|o| !o.trim().is_empty()),
        });
    }
    pub fn file(&mut self, f: FileTouch) {
        self.evs.push(Ev::File(f));
    }
    pub fn fail(&mut self, msg: impl Into<String>) {
        self.evs.push(Ev::Failed(msg.into()));
    }
}

/// One CLI's stream parser. Stateful: partial deltas and their aggregates must be told apart.
pub trait Parser: Send {
    fn line(&mut self, v: &serde_json::Value, out: &mut Out);
}

const OUTPUT_MAX: usize = 16 * 1024;

/// Keeps the tail of long tool output; that's where errors and results end up.
fn clip(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut start = s.len() - max;
    while !s.is_char_boundary(start) {
        start += 1;
    }
    format!("…\n{}", &s[start..])
}

/// Shortens paths under the working directory for display. macOS reports temp dirs both
/// with and without the `/private` prefix.
pub fn rel(cwd: &str, p: &str) -> String {
    let cwd = cwd.trim_end_matches('/');
    for base in [
        cwd.to_string(),
        format!("/private{cwd}"),
        cwd.strip_prefix("/private").unwrap_or(cwd).to_string(),
    ] {
        if let Some(r) = p.strip_prefix(&base).and_then(|r| r.strip_prefix('/')) {
            return r.to_string();
        }
    }
    p.to_string()
}

pub fn s(v: &serde_json::Value, k: &str) -> Option<String> {
    v.get(k).and_then(|x| x.as_str()).map(str::to_string)
}

/// Read-only shell commands an executor may run without asking (CLIs that gate shell per
/// command). Tokage never turns on "skip all permissions" (docs/01 §6).
pub const READONLY_SHELL: &[&str] = &[
    "git status",
    "git diff",
    "git log",
    "git show",
    "git branch",
    "ls",
    "pwd",
    "cat",
    "head",
    "tail",
    "wc",
    "rg",
    "grep",
];

/// How to start one turn.
pub struct Launch {
    pub args: Vec<String>,
    pub stdin: Option<String>,
    /// Inherited variables the CLI must not see.
    pub env_remove: Vec<String>,
    pub parser: Box<dyn Parser>,
}

/// Instruction prefix per mode. Plan mode keeps the model read-only and asks for constraints,
/// not implementation (docs/02 §5).
fn framed_prompt(mode: TurnMode, prompt: &str, first_turn: bool) -> String {
    match (mode, first_turn) {
        (TurnMode::Plan, true) => format!(
            "You are the planner. Read the project as needed but do not modify any files or run \
             commands that change state. Produce a concise plan, written in the same language as the Requirement text below:\n\
             1. Design notes (key decisions, constraints)\n\
             2. Contracts (APIs, data structures, component props) as signatures/types only\n\
             3. Task list: id, domain, depends on, files in scope, acceptance criteria\n\
             Do not write implementation code.\n\nRequirement:\n{prompt}"
        ),
        (TurnMode::Plan, false) => format!(
            "(Planning mode: stay read-only, revise the plan rather than implementing.)\n\n{prompt}"
        ),
        (TurnMode::Direct, _) => prompt.to_string(),
    }
}

fn full_prompt(req: &ChatRequest) -> String {
    let prompt = framed_prompt(req.mode, &req.prompt, req.resume.is_none());
    // A fresh session mid-conversation (e.g. the user switched models) carries the prior turns.
    if req.resume.is_some() || req.history.is_empty() {
        return prompt;
    }
    let transcript: Vec<String> = req
        .history
        .iter()
        .map(|h| format!("[{}]\n{}", h.role, h.text))
        .collect();
    format!(
        "Conversation so far:\n\n{}\n\n---\n\n{prompt}",
        transcript.join("\n\n")
    )
}

fn launch(req: &ChatRequest) -> Result<Launch, String> {
    let prompt = full_prompt(req);
    Ok(match req.provider {
        ProviderId::ClaudeCode => claude::launch(req, prompt),
        ProviderId::CodexCli => codex::launch(req, prompt),
        ProviderId::CursorCli => cursor::launch(req, prompt),
        ProviderId::CopilotCli => copilot::launch(req, prompt),
        other => return Err(format!("{other:?} can't run conversation turns yet.")),
    })
}

fn executable(providers: &[ProviderInfo], req: &ChatRequest) -> Result<PathBuf, String> {
    let p = providers
        .iter()
        .find(|p| p.id == req.provider)
        .ok_or("Unknown CLI")?;
    if !req.trusted.contains(&p.id) {
        return Err(format!(
            "{} hasn't been allowed to work in this project. Allow it under Settings → CLI management.",
            p.name
        ));
    }
    if !matches!(p.status, ProviderStatus::Ready) {
        return Err(format!("{} is not ready. Check Settings → CLI management.", p.name));
    }
    p.path
        .as_ref()
        .map(PathBuf::from)
        .ok_or_else(|| format!("{} executable not found", p.name))
}

pub async fn run_turn(app: AppHandle, req: ChatRequest, providers: Vec<ProviderInfo>, env: ShellEnv) {
    let turn_id = req.turn_id.clone();
    let emit = |kind: EventKind| {
        let _ = app.emit(
            EVENT,
            ChatEvent {
                turn_id: turn_id.clone(),
                kind,
            },
        );
    };
    let mirror = mirror_dir(&app);
    let result = match (executable(&providers, &req), launch(&req), direct_allowed(&req, &env)) {
        (Ok(exe), Ok(l), Ok(())) => run_cli(&exe, req.provider, l, &env, &req.cwd, mirror, &emit).await,
        (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => Err(e),
    };
    if let Err(message) = result {
        emit(EventKind::Error { message });
    }
}

/// Re-checked at launch: the CLI's config may have changed since detection.
fn direct_allowed(req: &ChatRequest, env: &ShellEnv) -> Result<(), String> {
    if req.mode != TurnMode::Direct {
        return Ok(());
    }
    match direct_limit(req.provider, env, Some(&req.cwd)) {
        Some(DirectLimit::CursorUnsafePermissions { detail }) => Err(format!(
            "Cursor CLI's permission settings let commands run without asking ({}), so Tokage \
             won't use it in Direct mode. See Settings → CLI management.",
            detail.join(", ")
        )),
        _ => Ok(()),
    }
}

/// Starts from an empty environment: the login env trimmed to this CLI's own credentials
/// (docs/11 §6.3), minus `remove`.
pub(crate) fn apply_env(
    cmd: &mut tokio::process::Command,
    id: ProviderId,
    env: &ShellEnv,
    remove: &[String],
) {
    cmd.env_clear()
        .envs(child_env::assemble(id, env, remove))
        .env("NO_COLOR", "1");
}

/// Tokage's folder for copies of files a turn wrote outside its working directory.
pub fn mirror_dir(app: &AppHandle) -> Option<PathBuf> {
    use tauri::Manager;
    app.path().app_data_dir().ok().map(|d| d.join("files"))
}

async fn run_cli(
    exe: &std::path::Path,
    provider: ProviderId,
    l: Launch,
    env: &ShellEnv,
    cwd: &str,
    mirror: Option<PathBuf>,
    emit: &impl Fn(EventKind),
) -> Result<(), String> {
    let Launch {
        args,
        stdin,
        env_remove,
        mut parser,
    } = l;
    let mut files = files::Files::start(env, cwd, mirror).await;
    let mut cmd = tokio::process::Command::new(exe);
    apply_env(&mut cmd, provider, env, &env_remove);
    cmd.args(&args)
        .current_dir(cwd)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    crate::proc::isolate(&mut cmd);
    let name = exe
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to start {name}: {e}"))?;
    let _group = crate::proc::Group::new(&child);
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        pipe.write_all(text.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        // Dropping the pipe closes it, so the CLI stops waiting for more input.
    }
    let stderr = child.stderr.take();
    let stderr_task = tokio::spawn(async move {
        let mut buf = String::new();
        if let Some(s) = stderr {
            let mut lines = BufReader::new(s).lines();
            while let Ok(Some(l)) = lines.next_line().await {
                if buf.len() < 64 * 1024 {
                    buf.push_str(&l);
                    buf.push('\n');
                }
            }
        }
        buf
    });

    let stdout = child.stdout.take().ok_or("stdout unavailable")?;
    let mut lines = BufReader::new(stdout).lines();
    let mut out = Out::default();
    let mut done: Option<EventKind> = None;
    let mut failure: Option<String> = None;
    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        parser.line(&v, &mut out);
        for ev in std::mem::take(&mut out.evs) {
            match ev {
                // Held back so the end-of-turn file scan lands before the turn closes.
                Ev::Emit(k @ EventKind::Done { .. }) => done = Some(k),
                Ev::Emit(k) => emit(k),
                Ev::File(f) => emit(files.change(env, f).await),
                Ev::Failed(m) => failure = Some(m),
            }
        }
    }
    let status = child.wait().await.map_err(|e| e.to_string())?;
    let stderr = stderr_task.await.unwrap_or_default();
    // Edits no event mentioned (shell redirects, scripts) still show up, even after a failure.
    for k in files.finish(env).await {
        emit(k);
    }
    if let Some(d) = done {
        emit(d);
        return Ok(());
    }
    Err(failure.unwrap_or_else(|| {
        let tail: Vec<&str> = stderr.lines().filter(|l| !l.trim().is_empty()).collect();
        let tail = tail[tail.len().saturating_sub(5)..].join("\n");
        format!("{name} exited with {status}. {tail}").trim().to_string()
    }))
}

/// Running turns, so the UI can stop one. Aborting drops the child and its process group.
#[derive(Default)]
pub struct Turns(pub tokio::sync::Mutex<HashMap<String, tokio::task::AbortHandle>>);

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// A minimal turn request for launch tests.
    pub fn request(provider: ProviderId) -> ChatRequest {
        ChatRequest {
            turn_id: "t".into(),
            cwd: "/".into(),
            prompt: "p".into(),
            mode: TurnMode::Direct,
            provider,
            model: "m".into(),
            effort: None,
            fast: false,
            tier: None,
            history: vec![],
            resume: None,
            fork: false,
            trusted: vec![],
        }
    }

    /// Feeds a recorded fixture through a parser.
    pub fn replay(parser: &mut dyn Parser, name: &str) -> Vec<Ev> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        let text = std::fs::read_to_string(path).unwrap();
        let mut out = Out::default();
        let mut all = vec![];
        for line in text.lines() {
            if let Ok(v) = serde_json::from_str(line) {
                parser.line(&v, &mut out);
                all.append(&mut out.evs);
            }
        }
        all
    }

    pub fn text(evs: &[Ev]) -> String {
        evs.iter()
            .filter_map(|e| match e {
                Ev::Emit(EventKind::Text { text }) => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    pub fn count(evs: &[Ev], f: impl Fn(&EventKind) -> bool) -> usize {
        evs.iter()
            .filter(|e| matches!(e, Ev::Emit(k) if f(k)))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_blocks_are_separated() {
        let mut o = Out::default();
        o.begin_text();
        o.text("a");
        o.text("b");
        o.begin_text();
        o.text("c");
        o.reasoning("r");
        o.begin_text();
        o.text("d");
        let t: Vec<_> = o
            .evs
            .iter()
            .filter_map(|e| match e {
                Ev::Emit(EventKind::Text { text }) => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(t, ["a", "b", "\n\nc", "\n\nd"]);
    }

    #[test]
    fn rel_paths() {
        assert_eq!(rel("/var/x", "/private/var/x/a.rs"), "a.rs");
        assert_eq!(rel("/private/var/x", "/var/x/a/b.rs"), "a/b.rs");
        assert_eq!(rel("/p", "/q/a.rs"), "/q/a.rs");
        assert_eq!(clip("abcdef", 3), "…\ndef");
    }

    #[test]
    fn history_only_without_resume() {
        let mut req = ChatRequest {
            turn_id: "t".into(),
            cwd: "/".into(),
            prompt: "next".into(),
            mode: TurnMode::Direct,
            provider: ProviderId::CodexCli,
            model: "m".into(),
            effort: None,
            fast: false,
            tier: None,
            history: vec![HistoryItem {
                role: "user".into(),
                text: "first".into(),
            }],
            resume: None,
            fork: false,
            trusted: vec![],
        };
        assert!(full_prompt(&req).contains("first"));
        req.resume = Some("id".into());
        assert_eq!(full_prompt(&req), "next");
    }
}

#[cfg(test)]
mod live {
    use super::*;

    /// P1.m1 acceptance: Codex and Cursor in Direct can't delete a file without asking.
    /// `cargo test --lib chat::live::direct_rm -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn direct_rm_is_not_run() {
        let env = crate::env::capture().await;
        let providers = crate::providers::detect_all(&env, &Default::default()).await;
        for (id, model) in [(ProviderId::CodexCli, "gpt-6-luna"), (ProviderId::CursorCli, "auto")] {
            let dir = std::env::temp_dir().join(format!("tokage-rm-{id:?}"));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("victim.txt"), "keep me\n").unwrap();
            let mut req = testing::request(id);
            req.cwd = dir.display().to_string();
            req.model = model.into();
            req.trusted = vec![id];
            req.prompt = "Run exactly this shell command with your shell tool: rm victim.txt . Then reply with one word.".into();
            let exe = executable(&providers, &req).unwrap();
            let events = std::sync::Mutex::new(Vec::new());
            let emit = |k: EventKind| events.lock().unwrap().push(k);
            let r = match direct_allowed(&req, &env) {
                Ok(()) => run_cli(&exe, id, launch(&req).unwrap(), &env, &req.cwd, None, &emit).await,
                Err(e) => Err(e),
            };
            println!("== {id:?}: {r:?}");
            for e in events.lock().unwrap().iter().filter(|e| matches!(e, EventKind::ToolStart { .. } | EventKind::ToolEnd { .. })) {
                println!("  {e:?}");
            }
            assert!(dir.join("victim.txt").exists(), "{id:?} deleted the file");
            std::fs::remove_dir_all(&dir).ok();
        }
    }

    /// Real turns on this machine's CLIs, cheapest models:
    /// `cargo test --lib chat::live -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn turn_per_cli() {
        let env = crate::env::capture().await;
        let providers = crate::providers::detect_all(&env, &Default::default()).await;
        for (id, model) in [
            (ProviderId::ClaudeCode, "haiku"),
            (ProviderId::CodexCli, "gpt-6-luna"),
            (ProviderId::CursorCli, "auto"),
            (ProviderId::CopilotCli, "gpt-6-luna"),
        ] {
            let dir = std::env::temp_dir().join(format!("tokage-live-{id:?}"));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("m.py"), "def add(a,b):\n    return a-b\n").unwrap();
            let req = ChatRequest {
                turn_id: "t".into(),
                cwd: dir.display().to_string(),
                prompt: "Fix the bug in m.py. Reply with one sentence.".into(),
                mode: TurnMode::Direct,
                provider: id,
                model: model.into(),
                effort: None,
                fast: false,
                tier: None,
                history: vec![],
                resume: None,
                fork: false,
                trusted: vec![id],
            };
            let events = std::sync::Mutex::new(Vec::new());
            let emit = |k: EventKind| events.lock().unwrap().push(k);
            let r = match executable(&providers, &req) {
                Ok(exe) => run_cli(&exe, id, launch(&req).unwrap(), &env, &req.cwd, None, &emit).await,
                Err(e) => Err(e),
            };
            println!("== {id:?}: {r:?}");
            for e in events.lock().unwrap().iter() {
                println!("  {e:?}");
            }
            std::fs::remove_dir_all(&dir).ok();
        }
    }
}
