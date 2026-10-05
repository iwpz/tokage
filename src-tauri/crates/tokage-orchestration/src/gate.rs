//! Automated Acceptance Gate execution and Write Scope validation (docs/03 §5, docs/09 §4-§5, P7.m3).
//!
//! Features:
//! - Local command execution with independent timeouts (default 300s).
//! - Guardrail R5: Acceptance command allowlist matching and arguments-array execution (no `sh -c`).
//! - Output filtering and capturing tail 200 lines as evidence.
//! - Guardrail R6: Write scope validation (actual changed files vs declared write_scope).
//! - Reverting out-of-scope changes on user request (`git checkout -- <files>`).

use std::path::Path;
use std::time::{Duration, Instant};
use regex::Regex;
use serde::{Deserialize, Serialize};
use tokage_native::env::ShellEnv;

/// Default timeout for individual acceptance command (300 seconds, docs/03 §5).
pub const DEFAULT_GATE_TIMEOUT_SECS: u64 = 300;

/// Maximum evidence lines captured from gate command output (docs/03 §5).
pub const GATE_EVIDENCE_TAIL_LINES: usize = 200;

/// Default allowlist patterns for automated acceptance commands (docs/09 §5).
/// Examples: npm test, pnpm build, cargo test, pytest, go test, make test, etc.
pub const DEFAULT_GATE_ALLOWLIST_PATTERNS: &[&str] = &[
    r"^(npm|pnpm|yarn|bun)(\.cmd)?\s+(run\s+)?(test|build|lint|typecheck|check)(\s+.*)?$",
    r"^cargo(\.exe)?\s+(test|build|check|clippy)(\s+.*)?$",
    r"^(pytest|python3?\s+-m\s+pytest|poetry\s+run\s+pytest)(\s+.*)?$",
    r"^go\s+test(\s+.*)?$",
    r"^make\s+[a-zA-Z0-9_\-]+$",
    r"^ctest(\s+.*)?$",
    r"^dotnet\s+(test|build)(\s+.*)?$",
    r"^gradlew?\s+(test|build|check)(\s+.*)?$",
    r"^mvn\s+(test|package|verify)(\s+.*)?$",
];

/// Result of evaluating whether a gate command is allowed (Guardrail R5).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GateCommandPermission {
    AllowedByPattern(String),
    AllowedByUserConfirmation,
    Denied(String),
}

/// Checks whether an acceptance command complies with allowlist patterns and command safety (R5).
pub fn check_gate_command_safety(
    cmd: &str,
    user_confirmed: bool,
    custom_patterns: Option<&[String]>,
) -> GateCommandPermission {
    let trimmed = cmd.trim();
    if trimmed.is_empty() {
        return GateCommandPermission::Denied("Empty command".into());
    }

    // Guardrail R5: Reject shell metacharacters (pipes, redirects, command chaining)
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

    if user_confirmed {
        return GateCommandPermission::AllowedByUserConfirmation;
    }

    // Check default patterns
    for pat in DEFAULT_GATE_ALLOWLIST_PATTERNS {
        if let Ok(re) = Regex::new(pat) {
            if re.is_match(trimmed) {
                return GateCommandPermission::AllowedByPattern((*pat).to_string());
            }
        }
    }

    // Check custom patterns
    if let Some(custom) = custom_patterns {
        for pat in custom {
            if let Ok(re) = Regex::new(pat) {
                if re.is_match(trimmed) {
                    return GateCommandPermission::AllowedByPattern(pat.clone());
                }
            }
        }
    }

    GateCommandPermission::Denied(
        "Command does not match gate allowlist patterns and was not explicitly approved".into(),
    )
}

/// Execution outcome of a single acceptance gate command.
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

/// Runs a single acceptance command using parameter array (R5) with timeout and tail capture.
pub async fn execute_gate_command(
    command_str: &str,
    workdir: &Path,
    env: &ShellEnv,
    timeout_secs: Option<u64>,
    user_confirmed: bool,
) -> GateCommandResult {
    let start = Instant::now();

    // 1. Check safety (R5)
    match check_gate_command_safety(command_str, user_confirmed, None) {
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
        GateCommandPermission::AllowedByPattern(_) | GateCommandPermission::AllowedByUserConfirmation => {}
    }

    // 2. Parse command into arguments array (no shell invocation)
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
    let timeout = Duration::from_secs(timeout_secs.unwrap_or(DEFAULT_GATE_TIMEOUT_SECS));

    let child = tokio::process::Command::new(program)
        .args(args)
        .current_dir(workdir)
        .env_clear()
        .envs(&env.vars)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn();

    let child = match child {
        Ok(c) => c,
        Err(e) => {
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

    let wait_res = tokio::time::timeout(timeout, child.wait_with_output()).await;

    let duration_ms = start.elapsed().as_millis() as u64;

    match wait_res {
        Ok(Ok(output)) => {
            let passed = output.status.success();
            let exit_code = output.status.code();

            // Merge stdout & stderr, filter ANSI, and keep last 200 lines
            let combined = format!(
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let tail = capture_tail_lines(&combined, GATE_EVIDENCE_TAIL_LINES);

            GateCommandResult {
                command: command_str.to_string(),
                passed,
                exit_code,
                duration_ms,
                output_tail: tail,
                error_message: if passed { None } else { Some(format!("Exit code: {:?}", exit_code)) },
            }
        }
        Ok(Err(e)) => GateCommandResult {
            command: command_str.to_string(),
            passed: false,
            exit_code: None,
            duration_ms,
            output_tail: format!("Process I/O error: {}", e),
            error_message: Some(e.to_string()),
        },
        Err(_) => GateCommandResult {
            command: command_str.to_string(),
            passed: false,
            exit_code: None,
            duration_ms,
            output_tail: format!("Command timed out after {}s", timeout.as_secs()),
            error_message: Some(format!("Timed out after {}s", timeout.as_secs())),
        },
    }
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
    let mut allowed_files = Vec::new();
    let mut out_of_scope_files = Vec::new();

    for file in changed_files {
        let clean_file = file.trim_matches('/').replace('\\', "/");
        let matches = declared_scopes.iter().any(|scope| {
            let clean_scope = scope.trim_matches('/').replace('\\', "/");
            if clean_file == clean_scope {
                return true;
            }
            if clean_file.starts_with(&format!("{}/", clean_scope)) {
                return true;
            }
            false
        });

        if matches {
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
                    args.push(current.clone());
                    current.clear();
                }
            }
            '\\' if !in_single_quote && i + 1 < chars.len() => {
                i += 1;
                current.push(chars[i]);
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

    #[test]
    fn test_gate_command_allowlist_matching() {
        assert!(matches!(
            check_gate_command_safety("cargo test --lib", false, None),
            GateCommandPermission::AllowedByPattern(_)
        ));
        assert!(matches!(
            check_gate_command_safety("pnpm test", false, None),
            GateCommandPermission::AllowedByPattern(_)
        ));
        assert!(matches!(
            check_gate_command_safety("npm run build", false, None),
            GateCommandPermission::AllowedByPattern(_)
        ));
        assert!(matches!(
            check_gate_command_safety("pytest tests/", false, None),
            GateCommandPermission::AllowedByPattern(_)
        ));

        // Metacharacter rejection
        assert!(matches!(
            check_gate_command_safety("cargo test && rm -rf /", true, None),
            GateCommandPermission::Denied(_)
        ));
        assert!(matches!(
            check_gate_command_safety("pnpm test | tee log.txt", true, None),
            GateCommandPermission::Denied(_)
        ));

        // Unknown unconfirmed command
        assert!(matches!(
            check_gate_command_safety("custom_tool --do-stuff", false, None),
            GateCommandPermission::Denied(_)
        ));

        // Unknown user confirmed command
        assert!(matches!(
            check_gate_command_safety("custom_tool --do-stuff", true, None),
            GateCommandPermission::AllowedByUserConfirmation
        ));
    }

    #[test]
    fn test_write_scope_validation() {
        let declared = vec!["src/users/".to_string(), "migrations/".to_string()];

        let changes = vec![
            "src/users/routes.rs".to_string(),
            "migrations/01_init.sql".to_string(),
            "src/auth/secret.rs".to_string(), // Out of scope!
        ];

        let res = validate_write_scope(&declared, &changes);
        assert!(!res.passed);
        assert_eq!(res.allowed_files.len(), 2);
        assert_eq!(res.out_of_scope_files, vec!["src/auth/secret.rs".to_string()]);

        let in_scope_only = vec![
            "src/users/routes.rs".to_string(),
            "migrations/01_init.sql".to_string(),
        ];
        let res2 = validate_write_scope(&declared, &in_scope_only);
        assert!(res2.passed);
        assert!(res2.out_of_scope_files.is_empty());
    }

    #[test]
    fn test_capture_tail_lines() {
        let long_text = (1..=300).map(|i| format!("Line {}", i)).collect::<Vec<_>>().join("\n");
        let tail = capture_tail_lines(&long_text, 200);
        let tail_lines = tail.lines().count();
        assert_eq!(tail_lines, 200);
        assert!(tail.contains("Line 300"));
        assert!(!tail.contains("Line 99\n"));
    }
}
