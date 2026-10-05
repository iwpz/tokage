//! Conversation turns (docs/02 §2 PLAN / DIRECT paths, first cut).
//!
//! Each turn runs on the CLI the user picked, headless with structured output (docs/01 §7.1).
//! Every adapter turns its CLI's event stream into one shared event model: reasoning, reply
//! text, tool steps and file changes. Diffs come from `files::Files`, so they look the same
//! whichever CLI made the edit.

pub mod adapter;
mod aider;
mod antigravity;
pub mod child_env;
mod claude;
mod codex;
mod copilot;
mod cursor;
pub mod error;
pub mod fake;
mod files;
pub mod filter;
mod opencode;
pub mod pty;
pub mod quota;
pub mod redact;

use std::{path::PathBuf, process::Stdio, time::Duration};

use serde::{Deserialize, Serialize};
use tokage_core::{Bus, DomainEvent, ProviderId, ProviderStatus};
pub use tokage_core::{AuthMode, Change, EventKind, Role, ToolKind};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::{env::ShellEnv, providers::ProviderInfo};

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
    adapter::adapter(id)?.direct_limit(env, cwd)
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TurnMode {
    Plan,
    Direct,
}

/// Execution timeout tier (docs/09 §2.1, R2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TimeoutTier {
    Quick,
    Standard,
    Long,
    Planning,
    Gate,
}

impl TimeoutTier {
    /// Returns (idle_timeout, hard_timeout) durations for this tier.
    pub fn base_durations(&self) -> (Duration, Duration) {
        match self {
            TimeoutTier::Quick => (Duration::from_secs(15), Duration::from_secs(30)),
            TimeoutTier::Standard => (Duration::from_secs(90), Duration::from_secs(180)),
            TimeoutTier::Long => (Duration::from_secs(180), Duration::from_secs(420)),
            TimeoutTier::Planning => (Duration::from_secs(180), Duration::from_secs(420)),
            TimeoutTier::Gate => (Duration::from_secs(120), Duration::from_secs(300)),
        }
    }
}

/// Resolved timeout limits for a supervised child process (docs/09 §2.1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimeoutLimits {
    pub idle: Duration,
    pub hard: Duration,
    pub exit: Duration,
}

impl TimeoutLimits {
    pub fn new(tier: TimeoutTier, multiplier: f64) -> Self {
        let (idle_base, hard_base) = tier.base_durations();
        let mult = if multiplier <= 0.0 { 1.0 } else { multiplier.clamp(0.001, 10.0) };
        Self {
            idle: Duration::from_secs_f64(idle_base.as_secs_f64() * mult),
            hard: Duration::from_secs_f64(hard_base.as_secs_f64() * mult),
            exit: Duration::from_secs(5),
        }
    }

    /// Resolves limits from tier and multiplier, with optional test environment overrides.
    pub fn resolved(tier: TimeoutTier, multiplier: f64) -> Self {
        let mut limits = Self::new(tier, multiplier);
        if let Some(ms) = std::env::var("TOKAGE_IDLE_TIMEOUT_MS").ok().and_then(|s| s.parse::<u64>().ok()) {
            limits.idle = Duration::from_millis(ms);
        }
        if let Some(ms) = std::env::var("TOKAGE_HARD_TIMEOUT_MS").ok().and_then(|s| s.parse::<u64>().ok()) {
            limits.hard = Duration::from_millis(ms);
        }
        limits
    }
}

/// Reason for supervision timeout (docs/09 §2.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeoutKind {
    Idle,
    Hard,
    Exit,
}

impl TimeoutKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            TimeoutKind::Idle => "idle",
            TimeoutKind::Hard => "hard",
            TimeoutKind::Exit => "exit",
        }
    }
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
    /// The session's plan under `.tokage/plans/`, relative to `cwd`, once one was saved.
    #[serde(default)]
    pub plan_file: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    /// Authorized command patterns for this session (docs/11 §4.3, P2.m3).
    #[serde(default)]
    pub session_allowed: Vec<String>,
    /// Effective authorization mode for this turn (docs/11 §2, P2.m6).
    #[serde(default)]
    pub auth_mode: Option<tokage_core::auth::AuthMode>,
    /// Task role of the agent/window (docs/01 §6, docs/11 §2).
    /// If None, inferred from mode: Plan => Role::Planner, Direct => Role::Executor.
    #[serde(default)]
    pub role: Option<Role>,
    /// Allowed write paths for EXECUTOR / TESTER (docs/01 §3, docs/09 §4, docs/11 §2).
    /// Relative globs against workspace roots. If empty, the entire workspace is writable for EXECUTOR.
    #[serde(default)]
    pub write_scope: Vec<String>,
    /// Timeout tier for this turn (docs/09 §2.1, R2). Defaults based on TurnMode.
    #[serde(default)]
    pub timeout_tier: Option<TimeoutTier>,
    /// Timeout multiplier from settings (e.g. 1.0, 2.0).
    #[serde(default)]
    pub timeout_multiplier: Option<f64>,
    /// Proxy configuration for this turn (docs/12).
    #[serde(default)]
    pub proxy: Option<tokage_core::proxy::GlobalProxyConfig>,
}

impl ChatRequest {
    pub fn effective_role(&self) -> Role {
        self.role.unwrap_or(match self.mode {
            TurnMode::Plan => Role::Planner,
            TurnMode::Direct => Role::Executor,
        })
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct HistoryItem {
    pub role: String,
    pub text: String,
}

/// What an adapter knows about a file change; `files::Files` fills in the rest.
#[derive(Clone, Debug, PartialEq)]
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
    /// A command or tool was refused by the CLI (docs/11 §4.3, P2.m3).
    Denial {
        tool_id: Option<String>,
        command: String,
        reason: String,
    },
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
    pub fn denial(&mut self, tool_id: Option<String>, command: String, reason: String) {
        self.evs.push(Ev::Denial {
            tool_id,
            command,
            reason,
        });
    }
    pub fn quota(&mut self, raw: String, reset_at: Option<String>, retrying: bool) {
        self.evs.push(Ev::Emit(EventKind::Quota {
            raw,
            reset_at,
            retrying,
        }));
    }
}

/// One CLI's stream parser. Stateful: partial deltas and their aggregates must be told apart.
pub trait Parser: Send {
    fn line(&mut self, v: &serde_json::Value, out: &mut Out);
    fn text_line(&mut self, _line: &str, _out: &mut Out) {}
    fn finish(&mut self, _out: &mut Out) {}
}

const OUTPUT_MAX: usize = 16 * 1024;
const PLAIN_MAX: usize = 64 * 1024;

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

/// Instruction prefix per role. Planning and review keep the model read-only and ask for constraints,
/// not implementation (docs/02 §5, docs/01 §6).
fn framed_prompt(role: Role, prompt: &str, first_turn: bool) -> String {
    match (role, first_turn) {
        (Role::Planner, true) => format!(
            "You are the planner. Read the project as needed but do not modify any files or run \
             commands that change state. Produce a concise plan, written in the same language as the Requirement text below:\n\
             1. Design notes (key decisions, constraints)\n\
             2. Contracts (APIs, data structures, component props) as signatures/types only\n\
             3. Task list: id, domain, depends on, files in scope, acceptance criteria\n\
             Do not write implementation code.\n\nRequirement:\n{prompt}"
        ),
        (Role::Planner, false) => format!(
            "(Planning mode: stay read-only, revise the plan rather than implementing.)\n\n{prompt}"
        ),
        (Role::Reviewer, _) => format!(
            "You are the reviewer. Review the project and code changes. Stay read-only and do not modify any files or execute state-changing commands.\n\nRequirement:\n{prompt}"
        ),
        _ => prompt.to_string(),
    }
}

fn full_prompt(req: &ChatRequest) -> String {
    let mut prompt = framed_prompt(req.effective_role(), &req.prompt, req.resume.is_none());
    if !req.write_scope.is_empty() && req.effective_role().allows_writes() {
        prompt = format!("(Allowed write scope: {})\n\n{prompt}", req.write_scope.join(", "));
    }
    if let Some(f) = &req.plan_file {
        prompt = format!("(This session's current plan is in `{f}`. Read it before continuing.)\n\n{prompt}");
    }
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
    let a = adapter::adapter(req.provider)
        .ok_or_else(|| format!("{:?} can't run conversation turns yet.", req.provider))?;
    Ok(a.launch(req, full_prompt(req)))
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
    // An untested version still runs; the UI already says so (docs/01 §4).
    if !matches!(p.status, ProviderStatus::Ready | ProviderStatus::UnsupportedVersion) {
        return Err(format!("{} is not ready. Check Settings → CLI management.", p.name));
    }
    p.path
        .as_ref()
        .map(PathBuf::from)
        .ok_or_else(|| format!("{} executable not found", p.name))
}

/// Pre-processes an event before emitting to bus or storage:
/// 1. Detects secrets written in `FileChange` diffs and raises a `Warning` event.
/// 2. Redacts all sensitive credential values and regex patterns to `[REDACTED]`.
/// 3. Bounds large chunks and formats text per R1 (OutputFilter).
pub fn process_event_for_emit(
    event: EventKind,
    filter: &filter::OutputFilter,
    mut emit_raw: impl FnMut(EventKind),
) {
    let mut warning = None;
    if let EventKind::FileChange { diff: Some(ref d), .. } = event {
        let additions: String = d
            .lines()
            .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
            .map(|l| &l[1..])
            .collect::<Vec<_>>()
            .join("\n");
        if filter.redactor().contains_secret(&additions) {
            warning = Some(EventKind::Warning {
                message: "A sensitive credential or API key was detected in file changes.".to_string(),
            });
        }
    }
    let filtered = filter.filter_event(event);
    emit_raw(filtered);
    if let Some(w) = warning {
        emit_raw(w);
    }
}

/// Runs one turn, publishing its events on the bus as `ChatOutput`. `mirror` is Tokage's folder
/// for copies of files the turn wrote outside its working directory.
pub async fn run_turn(
    bus: &Bus,
    req: ChatRequest,
    providers: Vec<ProviderInfo>,
    env: ShellEnv,
    mirror: Option<PathBuf>,
) {
    let filter = filter::OutputFilter::new(&env);
    let emit = |event: EventKind| {
        process_event_for_emit(event, &filter, |ev| {
            bus.publish(DomainEvent::ChatOutput {
                turn_id: req.turn_id.clone(),
                event: ev,
            });
        });
    };

    // P2.m6: Full-auto mode constraints (docs/11 §2)
    if req.auth_mode == Some(tokage_core::auth::AuthMode::FullAuto) {
        if cfg!(target_os = "windows") {
            emit(EventKind::Error {
                message: "Full-auto mode is not supported on Windows v1.".into(),
            });
            return;
        }
        let sandboxed = adapter::adapter(req.provider)
            .is_some_and(|a| a.meta().caps.has(adapter::Caps::OS_SANDBOX));
        if !sandboxed {
            emit(EventKind::Error {
                message: format!(
                    "Full-auto mode requires an OS sandboxed CLI, but {:?} does not have OS sandbox support.",
                    req.provider
                ),
            });
            return;
        }
        // P2.m6b: Must run inside a git worktree
        let git_info = crate::git::info(&env, std::path::Path::new(&req.cwd)).await;
        if git_info.is_repo && !git_info.linked_worktree {
            emit(EventKind::Error {
                message: "Full-auto mode must run inside a git worktree to protect the workspace repository.".into(),
            });
            return;
        }
        // P2.m6b: Take git snapshot before turn execution
        if let Err(e) = crate::git::record_turn_snapshot(&env, std::path::Path::new(&req.cwd), &req.turn_id).await {
            tracing::warn!("Failed to record turn snapshot: {e}");
        }
    }

    let prepared_proxy = if let Some(ref p_cfg) = req.proxy {
        crate::proxy::prepare_turn_proxy(p_cfg, req.provider, &env).await.ok()
    } else {
        None
    };

    let result = match (executable(&providers, &req), launch(&req), direct_allowed(&req, &env)) {
        (Ok(exe), Ok(l), Ok(())) => run_cli(&exe, &req, l, &env, mirror, prepared_proxy.as_ref(), &emit).await,
        (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => Err(e),
    };
    if let Err(message) = result {
        let kind = error::classify_error(Some(req.provider), &message, None);
        match kind {
            tokage_core::TurnErrorKind::AuthRequired => {
                bus.publish(DomainEvent::ProviderDetected {
                    provider: req.provider,
                    status: ProviderStatus::NeedsLogin,
                });
            }
            tokage_core::TurnErrorKind::RateLimited => {
                bus.publish(DomainEvent::ProviderMelted {
                    provider: req.provider,
                    reason: message.clone(),
                });
                bus.publish(DomainEvent::ProviderDetected {
                    provider: req.provider,
                    status: ProviderStatus::Melted,
                });
            }
            tokage_core::TurnErrorKind::SpawnFailed => {
                bus.publish(DomainEvent::ProviderDetected {
                    provider: req.provider,
                    status: ProviderStatus::NotInstalled,
                });
            }
            _ => {}
        }
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
/// (docs/11 §6.3), minus `remove`, with proxy configuration applied (docs/12).
pub(crate) fn apply_env(
    cmd: &mut tokio::process::Command,
    id: ProviderId,
    env: &ShellEnv,
    remove: &[String],
    proxy: Option<&crate::proxy::PreparedTurnProxy>,
) {
    cmd.env_clear()
        .envs(child_env::assemble_with_proxy(id, env, remove, proxy))
        .env("NO_COLOR", "1");
}

async fn run_cli(
    exe: &std::path::Path,
    req: &ChatRequest,
    l: Launch,
    env: &ShellEnv,
    mirror: Option<PathBuf>,
    proxy: Option<&crate::proxy::PreparedTurnProxy>,
    emit: &impl Fn(EventKind),
) -> Result<(), String> {
    let Launch {
        args,
        stdin,
        env_remove,
        mut parser,
    } = l;
    let mut files = files::Files::start(env, &req.cwd, mirror.clone()).await;

    let use_os_sandbox = (cfg!(target_os = "macos") || cfg!(target_os = "linux"))
        && adapter::adapter(req.provider).is_some_and(|a| a.meta().caps.has(adapter::Caps::OS_SANDBOX))
        && std::env::var("TOKAGE_DISABLE_SANDBOX").is_err();

    let forced_port = proxy.and_then(|p| {
        if p.forced {
            p.relay_handle.as_ref().map(|r| r.port)
        } else {
            None
        }
    });

    let mut cmd = if use_os_sandbox {
        let app_data = mirror
            .as_ref()
            .and_then(|m| m.parent().map(|p| p.to_string_lossy().into_owned()));
        crate::sandbox::create_sandboxed_command(
            req.provider,
            exe,
            &args,
            &req.cwd,
            env.vars.get("HOME").map(|s| s.as_str()),
            app_data.as_deref(),
            forced_port,
        )?
    } else {
        let mut c = tokio::process::Command::new(exe);
        c.args(&args);
        c
    };

    apply_env(&mut cmd, req.provider, env, &env_remove, proxy);
    cmd.current_dir(&req.cwd)
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
    let run_dir = crate::proc::get_run_dir()
        .or_else(|| mirror.as_ref().and_then(|m| m.parent()).map(|p| p.join("run")));
    let group = crate::proc::Group::with_dir_cmd(&child, run_dir, Some(name.clone()));
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

    let role = req.effective_role();
    let tier = req.timeout_tier.unwrap_or(match role {
        Role::Planner | Role::Reviewer => TimeoutTier::Planning,
        Role::Executor | Role::Tester => TimeoutTier::Standard,
    });
    let mult = req.timeout_multiplier.unwrap_or(1.0);
    let limits = TimeoutLimits::resolved(tier, mult);

    let stdout = child.stdout.take().ok_or("stdout unavailable")?;
    let mut lines = BufReader::new(stdout).lines();
    let mut out = Out::default();
    let mut done: Option<EventKind> = None;
    let mut failure: Option<String> = None;
    let mut approval_requested = false;
    let mut quota_emitted = false;
    let mut recognised = false;
    let mut plain = String::new();
    let mut filter = filter::OutputFilter::new(env);

    let hard = tokio::time::sleep(limits.hard);
    tokio::pin!(hard);

    loop {
        let next_line: Result<Result<Option<String>, std::io::Error>, TimeoutKind> = if approval_requested {
            // When waiting for approval, timeout supervision is paused per docs 09 §2.2 / docs 11 §8.
            Ok(lines.next_line().await)
        } else {
            tokio::select! {
                _ = &mut hard => Err(TimeoutKind::Hard),
                res = tokio::time::timeout(limits.idle, lines.next_line()) => match res {
                    Err(_) => Err(TimeoutKind::Idle),
                    Ok(line) => Ok(line),
                },
            }
        };

        let line = match next_line {
            Err(timeout_kind) => {
                group.kill().await;
                let _ = child.kill().await;
                for k in files.finish(env).await {
                    emit(k);
                }
                return Err(format!("Command timed out ({} timeout exceeded)", timeout_kind.as_str()));
            }
            Ok(Ok(Some(line))) => line,
            Ok(Ok(None)) => break,
            Ok(Err(e)) => {
                tracing::warn!("Failed to read child stdout line: {e}");
                break;
            }
        };

        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
            parser.line(&v, &mut out);
        } else {
            parser.text_line(&line, &mut out);
            for fl in filter.push_line(&line) {
                if plain.len() < PLAIN_MAX {
                    plain.push_str(&fl);
                    plain.push('\n');
                }
            }
        }
        recognised |= !out.evs.is_empty();
        for ev in std::mem::take(&mut out.evs) {
            match ev {
                // Held back so the end-of-turn file scan lands before the turn closes.
                Ev::Emit(k @ EventKind::Done { .. }) => done = Some(k),
                Ev::Emit(k @ EventKind::Quota { .. }) => {
                    quota_emitted = true;
                    if let EventKind::Quota { ref reset_at, .. } = k {
                        let resets_at = reset_at.as_deref().and_then(|r| r.parse::<i64>().ok());
                        crate::usage::record_cli_output_usage(
                            req.provider,
                            crate::usage::UsageSnapshot {
                                windows: vec![crate::usage::UsageWindow {
                                    window_minutes: 300,
                                    used_percent: 100.0,
                                    resets_at,
                                }],
                                observed_at: Some(quota::format_unix_to_utc_iso(
                                    std::time::SystemTime::now()
                                        .duration_since(std::time::SystemTime::UNIX_EPOCH)
                                        .map(|d| d.as_secs() as i64)
                                        .unwrap_or(0),
                                )),
                                source: crate::usage::UsageSource::CliOutput,
                            },
                        );
                    }
                    emit(k);
                }
                Ev::Emit(k) => emit(k),
                Ev::File(f) => {
                    if role.is_read_only() {
                        // R7: Planning and reviewer windows are read-only; terminate immediately and alert.
                        group.kill().await;
                        let _ = child.kill().await;
                        let ch = files.change(env, f.clone()).await;
                        emit(ch);
                        emit(EventKind::Warning {
                            message: format!(
                                "Read-only window ({}) attempted file modification: {} (R7)",
                                role.as_str(),
                                f.path
                            ),
                        });
                        emit(EventKind::Error {
                            message: format!(
                                "Turn terminated: file modification in read-only window ({}) (R7)",
                                role.as_str()
                            ),
                        });
                        return Err(format!(
                            "Turn terminated: file modification in read-only window ({}) (R7)",
                            role.as_str()
                        ));
                    }
                    if !req.write_scope.is_empty() {
                        let pack = crate::sandbox::builtin_pack_policy();
                        let mut cx = tokage_core::command_policy::Context::new(&req.cwd);
                        cx.home = env.vars.get("HOME").cloned();
                        cx.write_scope = Some(req.write_scope.clone());
                        let verdict = tokage_core::command_policy::Policy::new(&pack, &[]).classify_edit(&f.path, &cx);
                        if verdict.reasons.iter().any(|r| r.code == "outside_write_scope") {
                            emit(EventKind::Warning {
                                message: format!(
                                    "File edit outside write_scope: {} (scope: {:?})",
                                    f.path, req.write_scope
                                ),
                            });
                        }
                    }
                    emit(files.change(env, f).await);
                }
                Ev::Failed(m) => failure = Some(m),
                Ev::Denial { tool_id, command, reason } => {
                    let pack = crate::sandbox::builtin_pack_policy();
                    let mut cx = tokage_core::command_policy::Context::new(&req.cwd);
                    cx.home = env.vars.get("HOME").cloned();
                    cx.write_scope = if req.write_scope.is_empty() { None } else { Some(req.write_scope.clone()) };
                    let verdict = tokage_core::command_policy::Policy::new(&pack, &[]).classify(&command, &cx);
                    let outcome = tokage_core::auth::decide(
                        req.auth_mode.unwrap_or(tokage_core::auth::AuthMode::AskSensitive),
                        role,
                        &verdict,
                    );
                    match outcome.decision {
                        tokage_core::auth::Decision::Deny => {
                            if verdict.level == tokage_core::command_policy::Level::Forbidden {
                                emit(EventKind::Error {
                                    message: format!("Forbidden redline command rejected (R13): {command}"),
                                });
                            } else {
                                emit(EventKind::Error {
                                    message: format!("Command rejected by role/policy ({}): {command}", outcome.reason),
                                });
                            }
                        }
                        tokage_core::auth::Decision::Ask => {
                            approval_requested = true;
                            let req_id = format!("apr_{}_{}", req.turn_id, tool_id.as_deref().unwrap_or("0"));
                            emit(EventKind::ApprovalRequest {
                                id: req_id,
                                tool_id,
                                command,
                                level: "sensitive".into(),
                                reason,
                                recoverable: verdict.recoverable,
                            });
                        }
                        tokage_core::auth::Decision::Allow => {}
                    }
                }
            }
        }
    }

    parser.finish(&mut out);
    recognised |= !out.evs.is_empty();
    for ev in std::mem::take(&mut out.evs) {
        match ev {
            Ev::Emit(k @ EventKind::Done { .. }) => done = Some(k),
            Ev::Emit(k) => emit(k),
            Ev::File(f) => emit(files.change(env, f).await),
            Ev::Failed(m) => failure = Some(m),
            Ev::Denial { .. } => {}
        }
    }

    if let Some(fl) = filter.flush() {
        if plain.len() < PLAIN_MAX {
            plain.push_str(&fl);
            plain.push('\n');
        }
    }

    let status = match tokio::time::timeout(limits.exit, child.wait()).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            group.kill().await;
            return Err(e.to_string());
        }
        Err(_) => {
            group.kill().await;
            return Err("Command timed out waiting for process exit (exit timeout exceeded)".to_string());
        }
    };
    let stderr = stderr_task.await.unwrap_or_default();
    // Edits no event mentioned (shell redirects, scripts) still show up, even after a failure.
    let finishing_changes = files.finish(env).await;
    for k in &finishing_changes {
        emit(k.clone());
    }
    if role.is_read_only() && !finishing_changes.is_empty() {
        emit(EventKind::Warning {
            message: format!(
                "Read-only window ({}) modified files on disk (R7)",
                role.as_str()
            ),
        });
        emit(EventKind::Error {
            message: format!(
                "Turn terminated: file modification in read-only window ({}) (R7)",
                role.as_str()
            ),
        });
        return Err(format!(
            "Turn terminated: file modification in read-only window ({}) (R7)",
            role.as_str()
        ));
    }
    if !req.write_scope.is_empty() {
        let pack = crate::sandbox::builtin_pack_policy();
        let mut cx = tokage_core::command_policy::Context::new(&req.cwd);
        cx.home = env.vars.get("HOME").cloned();
        cx.write_scope = Some(req.write_scope.clone());
        for k in &finishing_changes {
            if let EventKind::FileChange { path, .. } = k {
                let verdict = tokage_core::command_policy::Policy::new(&pack, &[]).classify_edit(path, &cx);
                if verdict.reasons.iter().any(|r| r.code == "outside_write_scope") {
                    emit(EventKind::Warning {
                        message: format!(
                            "File edit outside write_scope: {} (scope: {:?})",
                            path, req.write_scope
                        ),
                    });
                }
            }
        }
    }
    if let Some(d) = done {
        emit(d);
        return Ok(());
    }
    if approval_requested {
        return Ok(());
    }
    // A format the parser doesn't know (e.g. an untested CLI version): show what it printed (ParseError degradation P4.m3a).
    if !recognised && status.success() && !plain.trim().is_empty() {
        emit(EventKind::Warning {
            message: "Output format not recognized; degraded to plain text display with estimated usage (P4.m3a)".into(),
        });
        emit(EventKind::Text {
            text: clip(plain.trim_end(), PLAIN_MAX),
        });
        emit(EventKind::Done {
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            api_calls: None,
            context_tokens: None,
            cost_usd: None,
            premium_requests: None,
            source: tokage_core::UsageSource::Estimated,
        });
        return Ok(());
    }
    let err_msg = failure.unwrap_or_else(|| {
        let tail: Vec<&str> = stderr.lines().filter(|l| !l.trim().is_empty()).collect();
        let tail = tail[tail.len().saturating_sub(10)..].join("\n");
        format!("{name} exited with {status}. {tail}").trim().to_string()
    });

    if !quota_emitted {
        if let Some(qm) = quota::match_quota(Some(req.provider), &err_msg) {
            let resets_at = qm.reset_at.as_deref().and_then(|r| r.parse::<i64>().ok());
            crate::usage::record_cli_output_usage(
                req.provider,
                crate::usage::UsageSnapshot {
                    windows: vec![crate::usage::UsageWindow {
                        window_minutes: 300,
                        used_percent: qm.percent.unwrap_or(100.0),
                        resets_at,
                    }],
                    observed_at: Some(quota::format_unix_to_utc_iso(
                        std::time::SystemTime::now()
                            .duration_since(std::time::SystemTime::UNIX_EPOCH)
                            .map(|d| d.as_secs() as i64)
                            .unwrap_or(0),
                    )),
                    source: crate::usage::UsageSource::CliOutput,
                },
            );
            emit(EventKind::Quota {
                raw: qm.raw,
                reset_at: qm.reset_at,
                retrying: false,
            });
        }
    }

    Err(err_msg)
}


pub mod testing {
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
            plan_file: None,
            session_id: None,
            project_id: None,
            session_allowed: vec![],
            auth_mode: None,
            role: None,
            write_scope: vec![],
            timeout_tier: None,
            timeout_multiplier: None,
            proxy: None,
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
            } else {
                parser.text_line(line, &mut out);
                all.append(&mut out.evs);
            }
        }
        parser.finish(&mut out);
        all.append(&mut out.evs);
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
        let mut req = testing::request(ProviderId::CodexCli);
        req.prompt = "next".into();
        req.history = vec![HistoryItem {
            role: "user".into(),
            text: "first".into(),
        }];
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
                Ok(()) => run_cli(&exe, &req, launch(&req).unwrap(), &env, None, None, &emit).await,
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
            let mut req = testing::request(id);
            req.cwd = dir.display().to_string();
            req.prompt = "Fix the bug in m.py. Reply with one sentence.".into();
            req.model = model.into();
            req.trusted = vec![id];
            let events = std::sync::Mutex::new(Vec::new());
            let emit = |k: EventKind| events.lock().unwrap().push(k);
            let r = match executable(&providers, &req) {
                Ok(exe) => run_cli(&exe, &req, launch(&req).unwrap(), &env, None, None, &emit).await,
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
