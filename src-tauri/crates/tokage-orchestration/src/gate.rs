//! Automated Acceptance Gate execution and Write Scope validation (docs/03 §5, docs/09 §4-§5, P7.m3).
//!
//! Features:
//! - Local command execution with independent timeouts (default 300s).
//! - Guardrail R5: Acceptance command allowlist matching and arguments-array execution (no `sh -c`).
//! - Output filtering and capturing tail 200 lines as evidence.
//! - Guardrail R6: Write scope validation (actual changed files vs declared write_scope).
//! - Reverting out-of-scope changes on user request (`git checkout -- <files>`).

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use regex::Regex;
use serde::{Deserialize, Serialize};
use tokage_native::chat::filter::OutputFilter;
use tokage_native::env::ShellEnv;
use tokage_native::proc::{self, Group};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{watch, Semaphore};

/// Default timeout for individual acceptance command (300 seconds, docs/03 §5).
pub const DEFAULT_GATE_TIMEOUT_SECS: u64 = 300;

/// Maximum evidence lines captured from gate command output (docs/03 §5).
pub const GATE_EVIDENCE_TAIL_LINES: usize = 200;

/// Result of evaluating whether a gate command is allowed (Guardrail R5).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GateCommandPermission {
    AllowedByPattern(String),
    AllowedByUserConfirmation,
    Denied(String),
}

/// Gate execution request (docs/fix-todo F5).
pub struct GateRequest<'a> {
    pub command: &'a str,
    pub workdir: &'a Path,
    pub env: &'a ShellEnv,
    pub timeout_secs: Option<u64>,
    pub confirmed: bool,
    pub allowlist: &'a [String],
    pub env_allow: &'a [String],
    pub run_id: Option<&'a str>,
}

/// Checks whether an acceptance command complies with allowlist patterns and command safety (R5).
///
/// Order: metacharacters → Denied; allowlist match → Allowed; confirmed → Allowed; else Denied.
pub fn check_gate_command_safety(
    cmd: &str,
    confirmed: bool,
    allowlist: &[String],
) -> GateCommandPermission {
    let trimmed = cmd.trim();
    if trimmed.is_empty() {
        return GateCommandPermission::Denied("Empty command".into());
    }

    if trimmed.contains('|')
        || trimmed.contains('>')
        || trimmed.contains('<')
        || trimmed.contains(';')
        || trimmed.contains("&&")
        || trimmed.contains("||")
        || trimmed.contains('`')
        || trimmed.contains("$(")
    {
        return GateCommandPermission::Denied(
            "Command contains shell piping, redirection, or chaining metacharacters".into(),
        );
    }

    for pat in allowlist {
        if let Ok(re) = Regex::new(pat) {
            if re.is_match(trimmed) {
                return GateCommandPermission::AllowedByPattern(pat.clone());
            }
        }
    }

    if confirmed {
        return GateCommandPermission::AllowedByUserConfirmation;
    }

    GateCommandPermission::Denied(
        "Command does not match gate allowlist patterns and was not explicitly approved".into(),
    )
}

/// Execution outcome of a single acceptance command.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GateCommandResult {
    pub command: String,
    pub passed: bool,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub output_tail: String,
    pub error_message: Option<String>,
}

struct GateRunEntry {
    cancel_tx: watch::Sender<bool>,
}

/// In-flight gate runs keyed by `run_id` (F5.e).
#[derive(Clone, Default)]
pub struct GateRuns {
    inner: Arc<Mutex<HashMap<String, GateRunEntry>>>,
}

impl GateRuns {
    pub fn new() -> Self {
        Self::default()
    }

    fn register(&self, run_id: &str, cancel_tx: watch::Sender<bool>) {
        if let Ok(mut g) = self.inner.lock() {
            g.insert(run_id.to_string(), GateRunEntry { cancel_tx });
        }
    }

    fn unregister(&self, run_id: &str) {
        if let Ok(mut g) = self.inner.lock() {
            g.remove(run_id);
        }
    }

    /// Signal cancel for `run_id`. Returns whether a run was found.
    pub fn cancel(&self, run_id: &str) -> bool {
        let Ok(g) = self.inner.lock() else {
            return false;
        };
        if let Some(entry) = g.get(run_id) {
            let _ = entry.cancel_tx.send(true);
            true
        } else {
            false
        }
    }
}

static GATE_RUNS: OnceLock<GateRuns> = OnceLock::new();
static GATE_SERIAL: OnceLock<Semaphore> = OnceLock::new();

/// Process-wide gate run registry.
pub fn gate_runs() -> &'static GateRuns {
    GATE_RUNS.get_or_init(GateRuns::new)
}

fn gate_serial() -> &'static Semaphore {
    GATE_SERIAL.get_or_init(|| Semaphore::new(1))
}

/// Cancel an in-flight gate by `run_id` (F5.e).
pub fn cancel_gate_command(run_id: &str) -> bool {
    gate_runs().cancel(run_id)
}

fn push_tail_line(tail: &Mutex<VecDeque<String>>, line: String) {
    if let Ok(mut q) = tail.lock() {
        if q.len() >= GATE_EVIDENCE_TAIL_LINES {
            q.pop_front();
        }
        q.push_back(line);
    }
}

fn join_tail(tail: &Mutex<VecDeque<String>>) -> String {
    let Ok(q) = tail.lock() else {
        return String::new();
    };
    q.iter().cloned().collect::<Vec<_>>().join("\n")
}

/// Runs a single acceptance command using parameter array (R5) with timeout and tail capture.
pub async fn execute_gate_command(req: GateRequest<'_>) -> GateCommandResult {
    let start = Instant::now();
    let command_str = req.command;

    match check_gate_command_safety(command_str, req.confirmed, req.allowlist) {
        GateCommandPermission::Denied(reason) => {
            return GateCommandResult {
                command: command_str.to_string(),
                passed: false,
                exit_code: None,
                duration_ms: start.elapsed().as_millis() as u64,
                output_tail: format!("Security rejection: {}", reason),
                error_message: Some(reason),
            };
        }
        GateCommandPermission::AllowedByPattern(_)
        | GateCommandPermission::AllowedByUserConfirmation => {}
    }

    let words = match shell_words_split(command_str) {
        Ok(w) if !w.is_empty() => w,
        Ok(_) => {
            return GateCommandResult {
                command: command_str.to_string(),
                passed: false,
                exit_code: None,
                duration_ms: start.elapsed().as_millis() as u64,
                output_tail: "Empty command arguments".into(),
                error_message: Some("Empty command arguments".into()),
            };
        }
        Err(e) => {
            return GateCommandResult {
                command: command_str.to_string(),
                passed: false,
                exit_code: None,
                duration_ms: start.elapsed().as_millis() as u64,
                output_tail: format!("Failed to parse command arguments: {}", e),
                error_message: Some(e),
            };
        }
    };

    let program = &words[0];
    let args = &words[1..];
    let timeout = Duration::from_secs(req.timeout_secs.unwrap_or(DEFAULT_GATE_TIMEOUT_SECS));
    let child_env = tokage_native::chat::child_env::assemble_gate_env(req.env, req.env_allow);

    // F5.h: only one acceptance command runs at a time (09 §3).
    if gate_serial().available_permits() == 0 {
        tracing::info!(command = %command_str, "排队等待验收");
    }
    let _permit = match gate_serial().acquire().await {
        Ok(p) => p,
        Err(_) => {
            return GateCommandResult {
                command: command_str.to_string(),
                passed: false,
                exit_code: None,
                duration_ms: start.elapsed().as_millis() as u64,
                output_tail: "Gate serial queue closed".into(),
                error_message: Some("Gate serial queue closed".into()),
            };
        }
    };

    let (cancel_tx, mut cancel_rx) = watch::channel(false);
    let run_id = req.run_id.map(str::to_string);
    if let Some(ref id) = run_id {
        gate_runs().register(id, cancel_tx.clone());
    }

    // F5.f: run under the OS gate sandbox when available; otherwise run unsandboxed and say so.
    let configure = |cmd: &mut tokio::process::Command| {
        cmd.current_dir(req.workdir)
            .env_clear()
            .envs(&child_env)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        proc::isolate(cmd);
    };
    let mut sandbox_note: Option<String> = None;
    let sandboxed_cmd = if std::env::var("TOKAGE_DISABLE_SANDBOX").is_ok() {
        sandbox_note = Some("TOKAGE_DISABLE_SANDBOX is set".to_string());
        None
    } else {
        match tokage_native::sandbox::create_sandboxed_gate_command(
            Path::new(program),
            args,
            &req.workdir.to_string_lossy(),
            req.env.vars.get("HOME").map(|s| s.as_str()),
            None,
        ) {
            Ok(c) => Some(c),
            Err(e) => {
                sandbox_note = Some(e);
                None
            }
        }
    };
    let plain_cmd = || {
        let mut c = tokio::process::Command::new(program);
        c.args(args);
        c
    };
    let spawned = match sandboxed_cmd {
        Some(mut c) => {
            configure(&mut c);
            match c.spawn() {
                Ok(child) => Ok(child),
                Err(e) => {
                    sandbox_note = Some(format!("failed to start sandbox: {e}"));
                    let mut c = plain_cmd();
                    configure(&mut c);
                    c.spawn()
                }
            }
        }
        None => {
            let mut c = plain_cmd();
            configure(&mut c);
            c.spawn()
        }
    };
    let sandbox_note = sandbox_note.map(|n| format!("[sandbox skipped: {n}; ran unsandboxed]"));

    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => {
            if let Some(ref id) = run_id {
                gate_runs().unregister(id);
            }
            return GateCommandResult {
                command: command_str.to_string(),
                passed: false,
                exit_code: None,
                duration_ms: start.elapsed().as_millis() as u64,
                output_tail: format!("Failed to spawn command '{}': {}", program, e),
                error_message: Some(e.to_string()),
            };
        }
    };

    let run_dir: Option<PathBuf> = proc::get_run_dir();
    let group = Group::with_dir_cmd(&child, run_dir, Some(program.to_string()));

    // F5.g: stream stdout/stderr, keep only the last N filtered lines.
    let tail: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(VecDeque::new()));
    let mut readers = Vec::new();
    if let Some(pipe) = child.stdout.take() {
        let tail = Arc::clone(&tail);
        let filter = OutputFilter::new(req.env);
        readers.push(tokio::spawn(async move {
            let mut lines = BufReader::new(pipe).lines();
            while let Ok(Some(raw)) = lines.next_line().await {
                let folded = OutputFilter::fold_cr(&raw);
                let stripped = filter.strip_ansi(&folded);
                let redacted = filter.redactor().redact_text(&stripped);
                if !redacted.trim().is_empty() {
                    push_tail_line(&tail, redacted);
                }
            }
        }));
    }
    if let Some(pipe) = child.stderr.take() {
        let tail = Arc::clone(&tail);
        let filter = OutputFilter::new(req.env);
        readers.push(tokio::spawn(async move {
            let mut lines = BufReader::new(pipe).lines();
            while let Ok(Some(raw)) = lines.next_line().await {
                let folded = OutputFilter::fold_cr(&raw);
                let stripped = filter.strip_ansi(&folded);
                let redacted = filter.redactor().redact_text(&stripped);
                if !redacted.trim().is_empty() {
                    push_tail_line(&tail, redacted);
                }
            }
        }));
    }

    let wait_res = tokio::select! {
        biased;
        _ = async {
            loop {
                if *cancel_rx.borrow() {
                    break;
                }
                if cancel_rx.changed().await.is_err() {
                    break;
                }
            }
        } => {
            group.kill().await;
            let _ = child.kill().await;
            Err("cancelled".to_string())
        }
        res = child.wait() => {
            res.map_err(|e| e.to_string()).map(|status| status)
        }
        _ = tokio::time::sleep(timeout) => {
            group.kill().await;
            let _ = child.kill().await;
            Err(format!("Timed out after {}s", timeout.as_secs()))
        }
    };
    for t in readers {
        let _ = t.await;
    }
    if let Some(ref id) = run_id {
        gate_runs().unregister(id);
    }
    let duration_ms = start.elapsed().as_millis() as u64;
    let output_tail = join_tail(&tail);

    let mut result = match wait_res {
        Ok(status) => {
            let passed = status.success();
            let exit_code = status.code();
            GateCommandResult {
                command: command_str.to_string(),
                passed,
                exit_code,
                duration_ms,
                output_tail,
                error_message: if passed {
                    None
                } else {
                    Some(format!("Exit code: {:?}", exit_code))
                },
            }
        }
        Err(msg) if msg == "cancelled" => GateCommandResult {
            command: command_str.to_string(),
            passed: false,
            exit_code: None,
            duration_ms,
            output_tail: if output_tail.is_empty() {
                "Gate command cancelled".into()
            } else {
                output_tail
            },
            error_message: Some("cancelled".into()),
        },
        Err(msg) if msg.starts_with("Timed out") => GateCommandResult {
            command: command_str.to_string(),
            passed: false,
            exit_code: None,
            duration_ms,
            output_tail: if output_tail.is_empty() {
                format!("Command timed out after {}s", timeout.as_secs())
            } else {
                output_tail
            },
            error_message: Some(msg),
        },
        Err(e) => GateCommandResult {
            command: command_str.to_string(),
            passed: false,
            exit_code: None,
            duration_ms,
            output_tail: if output_tail.is_empty() {
                format!("Command failed: {}", e)
            } else {
                output_tail
            },
            error_message: Some(e),
        },
    };
    if let Some(note) = sandbox_note {
        if result.output_tail.is_empty() {
            result.output_tail = note;
        } else {
            result.output_tail = format!("{}\n{}", result.output_tail, note);
        }
    }
    result
}

/// Result of validating actual file changes against declared write_scope (Guardrail R6).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeValidationResult {
    pub passed: bool,
    pub declared_scopes: Vec<String>,
    pub changed_files: Vec<String>,
    pub allowed_files: Vec<String>,
    pub out_of_scope_files: Vec<String>,
}

/// Validates whether changed files fall strictly within declared write_scope (R6).
pub fn validate_write_scope(
    declared_scopes: &[String],
    changed_files: &[String],
) -> ScopeValidationResult {
    use tokage_core::command_policy::scope;

    let mut allowed_files = Vec::new();
    let mut out_of_scope_files = Vec::new();

    for file in changed_files {
        if scope::any_covers(declared_scopes, file, false) {
            allowed_files.push(file.clone());
        } else {
            out_of_scope_files.push(file.clone());
        }
    }

    let passed = out_of_scope_files.is_empty();

    ScopeValidationResult {
        passed,
        declared_scopes: declared_scopes.to_vec(),
        changed_files: changed_files.to_vec(),
        allowed_files,
        out_of_scope_files,
    }
}

/// Reverts out-of-scope files via `git checkout -- <files>` (docs/09 §4, P7.m3c).
pub async fn revert_out_of_scope_files(
    workdir: &Path,
    env: &ShellEnv,
    files: &[String],
) -> Result<String, String> {
    if files.is_empty() {
        return Ok("No files to revert".into());
    }

    let mut args = vec!["checkout", "--"];
    for f in files {
        args.push(f);
    }

    tokage_native::git::run(env, workdir, &args, &[], 30).await
}

/// Captures the last N lines of output as evidence.
pub fn capture_tail_lines(text: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= max_lines {
        text.trim().to_string()
    } else {
        lines[lines.len() - max_lines..].join("\n").trim().to_string()
    }
}

/// Basic shell command argument split without shell expansion.
fn shell_words_split(cmd: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_single_quote = false;
    let mut in_double_quote = false;

    let chars = cmd.chars().collect::<Vec<char>>();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        match c {
            '\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
            }
            '"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
            }
            ' ' | '\t' if !in_single_quote && !in_double_quote => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            _ => {
                current.push(c);
            }
        }
        i += 1;
    }

    if in_single_quote || in_double_quote {
        return Err("Unclosed quote in command".into());
    }

    if !current.is_empty() {
        args.push(current);
    }

    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack_patterns() -> Vec<String> {
        tokage_matching::pack::builtin_knowledge_pack()
            .gate_allowlist
            .clone()
    }

    #[test]
    fn test_gate_command_allowlist_matching() {
        let pats = pack_patterns();
        assert!(matches!(
            check_gate_command_safety("cargo test --lib", false, &pats),
            GateCommandPermission::AllowedByPattern(_)
        ));
        assert!(matches!(
            check_gate_command_safety("pnpm test", false, &pats),
            GateCommandPermission::AllowedByPattern(_)
        ));
        assert!(matches!(
            check_gate_command_safety("npm run build", false, &pats),
            GateCommandPermission::AllowedByPattern(_)
        ));
        assert!(matches!(
            check_gate_command_safety("pytest tests/", false, &pats),
            GateCommandPermission::AllowedByPattern(_)
        ));

        assert!(matches!(
            check_gate_command_safety("cargo test && rm -rf /", true, &pats),
            GateCommandPermission::Denied(_)
        ));
        assert!(matches!(
            check_gate_command_safety("pnpm test | tee log.txt", true, &pats),
            GateCommandPermission::Denied(_)
        ));

        assert!(matches!(
            check_gate_command_safety("custom_tool --do-stuff", false, &pats),
            GateCommandPermission::Denied(_)
        ));

        assert!(matches!(
            check_gate_command_safety("custom_tool --do-stuff", true, &pats),
            GateCommandPermission::AllowedByUserConfirmation
        ));
    }

    #[test]
    fn test_write_scope_validation() {
        let declared = vec!["src/users/".to_string(), "migrations/".to_string()];

        let changes = vec![
            "src/users/routes.rs".to_string(),
            "migrations/01_init.sql".to_string(),
            "src/auth/secret.rs".to_string(),
        ];

        let res = validate_write_scope(&declared, &changes);
        assert!(!res.passed);
        assert_eq!(res.allowed_files.len(), 2);
        assert_eq!(
            res.out_of_scope_files,
            vec!["src/auth/secret.rs".to_string()]
        );

        let in_scope_only = vec![
            "src/users/routes.rs".to_string(),
            "migrations/01_init.sql".to_string(),
        ];
        let res2 = validate_write_scope(&declared, &in_scope_only);
        assert!(res2.passed);
        assert!(res2.out_of_scope_files.is_empty());

        let glob_declared = vec!["src/server/**".to_string()];
        let glob_ok =
            validate_write_scope(&glob_declared, &["src/server/routes.rs".to_string()]);
        assert!(glob_ok.passed, "src/server/** should cover routes.rs");
    }

    #[test]
    fn test_capture_tail_lines() {
        let long_text = (1..=300)
            .map(|i| format!("Line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let tail = capture_tail_lines(&long_text, 200);
        let tail_lines = tail.lines().count();
        assert_eq!(tail_lines, 200);
        assert!(tail.contains("Line 300"));
        assert!(!tail.contains("Line 99\n"));
    }
}
