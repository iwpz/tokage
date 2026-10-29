//! Aider CLI adapter: single messages use the CLI message-file transport at preparation.
//! Sample: tests/fixtures/aider.txt (Aider 0.72.0).
//!
//! Aider does not produce JSONL; it prints formatted text to stdout.
//! This adapter parses model lines, applied edits, message text, and the tail
//! `Tokens: ... sent, ... received. Cost: $... message` summary into structured events.

use super::{rel, ChatRequest, EventKind, FileTouch, Launch, Out, Parser, ToolKind};
use crate::env::ShellEnv;
use sha2::{Digest, Sha256};
use std::{io::Write, path::Path};

fn valid_session(id: &str) -> bool {
    id.strip_prefix("aider-")
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
}
fn session_id(req: &ChatRequest) -> String {
    if let Some(id) = req.resume.as_deref().filter(|id| valid_session(id)) {
        return id.into();
    }
    let mut hash = Sha256::new();
    for value in [
        &req.cwd,
        &req.turn_id,
        &req.model,
        req.session_id.as_deref().unwrap_or(""),
    ] {
        hash.update(value.as_bytes());
        hash.update([0]);
    }
    format!("aider-{:x}", hash.finalize())
}

/// Histories belong to one native turn/session, never a shared project default.
/// Attach bounded context explicitly: Aider is an editor, not a file-browsing agent.
pub async fn prepare(
    req: &ChatRequest,
    env: &ShellEnv,
    args: &mut Vec<String>,
) -> Result<tempfile::NamedTempFile, String> {
    if req.resume.as_deref().is_some_and(|id| !valid_session(id)) {
        return Err("Unsupported Aider history identifier; start a new session".into());
    }
    let home = env
        .vars
        .get("HOME")
        .or_else(|| env.vars.get("USERPROFILE"))
        .ok_or("Aider history requires a home directory")?;
    let root = Path::new(&req.cwd)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let workspace = format!("{:x}", Sha256::digest(root.to_string_lossy().as_bytes()));
    let history_dir = Path::new(home)
        .join(".aider/tokage")
        .join(workspace)
        .join(session_id(req));
    std::fs::create_dir_all(&history_dir).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&history_dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    // Override startup actions from default config files without copying credentials.
    let config = history_dir.join("runtime.yml");
    std::fs::write(
        &config,
        "lint: false\ntest: false\ncommit: false\nshow-prompts: false\nexit: false\nload: ''\n",
    )
    .map_err(|e| e.to_string())?;
    args.extend(["--config".into(), config.to_string_lossy().into_owned()]);
    if req.resume.is_some() && !history_dir.join("chat.md").is_file() {
        return Err("Aider history is unavailable in this workspace; start a new session".into());
    }
    for (flag, name) in [
        ("--chat-history-file", "chat.md"),
        ("--input-history-file", "input.history"),
    ] {
        args.extend([
            flag.into(),
            history_dir.join(name).to_string_lossy().into_owned(),
        ]);
    }
    // Keep message bytes out of argv: large ordinary requests otherwise exceed
    // OS argument limits. The caller holds this private file until the child
    // exits (including errors/cancellation), then RAII removes it.
    let message_index = args.iter().position(|arg| arg == "--message")
        .ok_or("Aider message argument is missing")?;
    let message = args.get(message_index + 1).ok_or("Aider message is missing")?;
    let mut message_file = tempfile::Builder::new().prefix("message-").suffix(".txt")
        .tempfile_in(&history_dir).map_err(|e| e.to_string())?;
    message_file.write_all(message.as_bytes()).map_err(|e| e.to_string())?;
    message_file.flush().map_err(|e| e.to_string())?;
    args[message_index] = "--message-file".into();
    args[message_index + 1] = message_file.path().to_string_lossy().into_owned();

    let candidates = match crate::git::run(
        env,
        &root,
        &["ls-files", "-co", "--exclude-standard"],
        &[],
        10,
    )
    .await
    {
        Ok(paths) => paths.lines().map(str::to_string).collect::<Vec<_>>(),
        Err(_) => std::fs::read_dir(&root)
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .filter_map(|e| e.file_name().into_string().ok())
            .collect(),
    };
    let in_scope = |path: &str| {
        req.write_scope.is_empty()
            || tokage_core::command_policy::scope::any_covers(&req.write_scope, path, cfg!(windows))
    };
    let mut candidates = candidates;
    candidates.sort_by_key(|path| {
        (
            !in_scope(path),
            !req.prompt.contains(path.as_str()),
            path.clone(),
        )
    });
    let pack = crate::sandbox::builtin_pack_policy();
    let mut cx = tokage_core::command_policy::Context::new(&req.cwd);
    cx.home = Some(home.clone());
    let mut bytes = 0;
    let mut count = 0;
    for path in candidates {
        if count >= 32 {
            break;
        }
        if path.split('/').any(|s| {
            s.starts_with('.')
                || ["node_modules", "target", "dist", "vendor", "__pycache__"].contains(&s)
        }) {
            continue;
        }
        let absolute = root.join(&path);
        let Ok(meta) = std::fs::symlink_metadata(&absolute) else {
            continue;
        };
        if !meta.is_file() || meta.len() > 32 * 1024 || bytes + meta.len() > 128 * 1024 {
            continue;
        }
        let Ok(canonical) = absolute.canonicalize() else {
            continue;
        };
        if !canonical.starts_with(&root) {
            continue;
        }
        let cmd = format!(
            "cat '{}'",
            canonical.to_string_lossy().replace('\'', "'\"'\"'")
        );
        if tokage_core::command_policy::Policy::new(&pack, &[])
            .classify(&cmd, &cx)
            .level
            == tokage_core::command_policy::Level::Forbidden
        {
            continue;
        }
        if std::fs::read_to_string(&canonical).is_err() {
            continue;
        }
        let flag = if req.effective_role().is_read_only() || !in_scope(&path) {
            "--read"
        } else {
            "--file"
        };
        args.extend([flag.into(), canonical.to_string_lossy().into_owned()]);
        bytes += meta.len();
        count += 1;
    }
    Ok(message_file)
}

pub fn launch(req: &ChatRequest, prompt: String) -> Launch {
    let mut args: Vec<String> = vec![
        "--message".into(),
        prompt,
        "--model".into(),
        req.model.clone(),
        "--no-git".into(),
        "--no-auto-commits".into(),
        "--no-check-update".into(),
        "--no-analytics".into(),
        "--no-fancy-input".into(),
        "--no-pretty".into(),
        "--yes-always".into(),
    ];
    args.extend(
        [
            "--no-suggest-shell-commands",
            "--no-auto-lint",
            "--no-auto-test",
            "--no-auto-accept-architect",
            "--no-detect-urls",
            "--no-notifications",
            "--no-stream",
            "--no-restore-chat-history",
        ]
        .map(str::to_string),
    );
    args.extend([
        "--load".into(),
        "".into(),
        "--weak-model".into(),
        req.model.clone(),
        "--editor-model".into(),
        req.model.clone(),
        "--cache-keepalive-pings".into(),
        "0".into(),
        "--map-tokens".into(),
        "0".into(),
        "--timeout".into(),
        "180".into(),
    ]);
    if let Some(effort) = &req.effort {
        args.extend(["--reasoning-effort".into(), effort.clone()]);
    }
    let role = req.effective_role();
    if role.is_read_only() {
        args.extend(["--dry-run".into(), "--chat-mode".into(), "ask".into()]);
    } else {
        args.extend(["--chat-mode".into(), "diff".into()]);
    }
    if let Some(_id) = &req.resume {
        args.push("--restore-chat-history".into());
    }

    Launch {
        args,
        stdin: None,
        env_remove: vec!["AIDER_MESSAGE".into(), "AIDER_MESSAGE_FILE".into()],
        parser: Box::new(Aider {
            cwd: req.cwd.clone(),
            session_id: session_id(req),
            model: Some(req.model.clone()),
            ..Default::default()
        }),
    }
}

#[derive(Default)]
pub struct Aider {
    cwd: String,
    model: Option<String>,
    session_id: String,
    session_id_emitted: bool,
    totals: Option<tokage_core::event::TurnUsage>,
    failed: bool,
    edit_count: usize,
    usage: Option<tokage_core::event::TurnUsage>,
    finished: bool,
}

impl Parser for Aider {
    fn line(&mut self, _v: &serde_json::Value, _out: &mut Out) {
        // Aider output is non-JSON; line events are handled via text_line.
    }

    fn text_line(&mut self, line: &str, out: &mut Out) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return;
        }

        let api_error = trimmed.strip_prefix("litellm.").unwrap_or(trimmed);
        if [
            "AuthenticationError:",
            "BadRequestError:",
            "APIConnectionError:",
            "APIError:",
            "Timeout:",
            "ServiceUnavailableError:",
            "PermissionDeniedError:",
            "NotFoundError:",
            "RateLimitError:",
        ]
        .iter()
        .any(|prefix| api_error.starts_with(prefix))
            || trimmed.starts_with("Unable to ")
            || trimmed.starts_with("Failed to ")
            || trimmed.starts_with("Invalid API key")
        {
            self.failed = true;
            out.fail(trimmed.to_string());
        }

        // 1. Quota check
        if let Some(qm) = super::quota::match_quota(Some(tokage_core::ProviderId::Aider), trimmed) {
            out.quota(qm.raw, qm.reset_at, qm.retrying);
        }

        // 2. Model declaration: "Model: deepseek/deepseek-chat ..."
        if let Some(rest) = trimmed
            .strip_prefix("Model: ")
            .or_else(|| trimmed.strip_prefix("Main model: "))
        {
            let model_name = rest.split_whitespace().next().unwrap_or(rest);
            self.model = Some(model_name.to_string());
            if !self.session_id_emitted && !self.session_id.is_empty() {
                out.emit(EventKind::Thread {
                    id: self.session_id.clone(),
                });
                self.session_id_emitted = true;
            }
            out.emit(EventKind::Model {
                name: model_name.to_string(),
            });
            return;
        }

        if let Some(path) = trimmed.strip_prefix("Added ").and_then(|p| {
            p.strip_suffix(" to the chat (read-only).")
                .or_else(|| p.strip_suffix(" to the chat."))
        }) {
            self.edit_count += 1;
            let id = format!("aider-read-{}", self.edit_count);
            out.tool_start(&id, ToolKind::Read, path.into(), None);
            out.tool_end(&id, Some(true), None);
            return;
        }

        // 3. Tokens & Cost line:
        // "Tokens: 1.2k sent, 420 received. Cost: $0.0120 message, $0.0120 session."
        // or "Tokens: 12,500 sent, 1,350 received."
        if trimmed.starts_with("Tokens:") {
            if let Some(u) = parse_aider_tokens_cost(trimmed) {
                self.flush_usage();
                self.usage = Some(u);
                return;
            }
        }

        if trimmed.starts_with("Cost:") {
            if let Some(usage) = &mut self.usage {
                usage.cost_usd = parse_cost(trimmed);
            }
            return;
        }

        // 4. File edit: "Applied edit to src/lib.rs"
        if let Some(rest) = trimmed.strip_prefix("Applied edit to ") {
            let path = rest.trim();
            self.edit_count += 1;
            let id = format!("aider-edit-{}", self.edit_count);
            out.tool_start(&id, ToolKind::Edit, path.into(), None);
            out.tool_end(&id, Some(true), None);
            out.file(FileTouch {
                tool_id: None,
                path: rel(&self.cwd, path),
                before: None,
                after: None,
                deleted: false,
            });
            return;
        }

        // 5. Skip banner/status lines
        if trimmed.starts_with("Aider v")
            || trimmed.starts_with("Git repo:")
            || trimmed.starts_with("Repo-map:")
            || trimmed.starts_with("Main model:")
            || trimmed.starts_with("Weak model:")
        {
            return;
        }

        // 6. Regular prose text
        out.begin_text();
        out.text(trimmed);
    }

    fn finish(&mut self, out: &mut Out) {
        if self.finished {
            return;
        }
        self.finished = true;
        self.flush_usage();
        if self.failed {
            return;
        }
        let Some(u) = &self.totals else {
            out.fail("Aider exited without reported model usage; no successful completion");
            return;
        };
        if u.input_tokens + u.output_tokens == 0 {
            out.fail("Aider reported no model usage");
            return;
        }
        if !self.session_id_emitted && !self.session_id.is_empty() {
            out.emit(EventKind::Thread {
                id: self.session_id.clone(),
            });
            self.session_id_emitted = true;
        }
        out.emit(EventKind::Done {
            input_tokens: u.input_tokens,
            output_tokens: u.output_tokens,
            cache_read_tokens: u.cache_read_tokens,
            cache_write_tokens: u.cache_write_tokens,
            api_calls: u.api_calls,
            context_tokens: None,
            cost_usd: u.cost_usd,
            premium_requests: None, credits: None,
            source: tokage_core::UsageSource::StdoutRegex,
        });
    }
}

impl Aider {
    fn flush_usage(&mut self) {
        let Some(mut u) = self.usage.take() else {
            return;
        };
        // DeepSeek's sent count includes cache hits; Anthropic's excludes them.
        if !self
            .model
            .as_deref()
            .is_some_and(|m| m.starts_with("anthropic/") || m.starts_with("claude-"))
        {
            u.input_tokens = u.input_tokens.saturating_sub(u.cache_read_tokens);
        }
        u.input_tokens = u.input_tokens.saturating_sub(u.cache_write_tokens);
        if let Some(total) = &mut self.totals {
            total.input_tokens += u.input_tokens;
            total.output_tokens += u.output_tokens;
            total.cache_read_tokens += u.cache_read_tokens;
            total.cache_write_tokens += u.cache_write_tokens;
            total.api_calls = Some(total.api_calls.unwrap_or(0) + 1);
            total.cost_usd = match (total.cost_usd, u.cost_usd) {
                (Some(a), Some(b)) => Some(a + b),
                _ => None,
            };
        } else {
            self.totals = Some(u);
        }
    }
}
fn parse_cost(line: &str) -> Option<f64> {
    let value = line
        .split("Cost:")
        .nth(1)?
        .split('$')
        .nth(1)?
        .split_whitespace()
        .next()?;
    value
        .trim_end_matches(',')
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v >= 0.0)
}

/// Parses "Tokens: 1.2k sent, 420 received. Cost: $0.0120 message, $0.0120 session."
fn parse_aider_tokens_cost(line: &str) -> Option<tokage_core::event::TurnUsage> {
    static COUNTS: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let regex = COUNTS.get_or_init(|| {
        regex::Regex::new(r"(?i)([\d,.]+[km]?)\s+(sent|received|cache hit|cache write)").unwrap()
    });
    let mut input = None;
    let mut output = None;
    let mut read = 0;
    let mut write = 0;
    for captures in regex.captures_iter(line) {
        let count = parse_token_count(&captures[1]);
        match &captures[2] {
            "sent" => input = Some(count),
            "received" => output = Some(count),
            "cache hit" => read = count,
            "cache write" => write = count,
            _ => {}
        }
    }
    let input_tokens = input?;
    let output_tokens = output?;
    let cost_usd = parse_cost(line);

    Some(tokage_core::event::TurnUsage {
        input_tokens,
        output_tokens,
        cache_read_tokens: read,
        cache_write_tokens: write,
        api_calls: Some(1),
        context_tokens: None,
        cost_usd,
        premium_requests: None, credits: None,
        source: tokage_core::UsageSource::StdoutRegex,
    })
}

/// Parses "1.2k", "12,500", "420" into numeric token count.
fn parse_token_count(s: &str) -> u64 {
    let clean = s.replace(',', "");
    let s = clean.trim();
    if let Some(n) = s.strip_suffix(['k', 'K']) {
        if let Ok(f) = n.parse::<f64>() {
            return (f * 1000.0).round() as u64;
        }
    }
    if let Some(n) = s.strip_suffix(['m', 'M']) {
        if let Ok(f) = n.parse::<f64>() {
            return (f * 1_000_000.0).round() as u64;
        }
    }
    s.parse::<u64>().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::testing::*;

    #[test]
    fn parses_tokens_and_cost() {
        let u = parse_aider_tokens_cost(
            "Tokens: 1.2k sent, 420 received. Cost: $0.0120 message, $0.0120 session.",
        )
        .unwrap();
        assert_eq!(u.input_tokens, 1200);
        assert_eq!(u.output_tokens, 420);
        assert_eq!(u.cost_usd, Some(0.012));
        assert_eq!(u.source, tokage_core::UsageSource::StdoutRegex);

        let u2 =
            parse_aider_tokens_cost("Tokens: 15,300 sent, 2,500 received. Cost: $0.045 message.")
                .unwrap();
        assert_eq!(u2.input_tokens, 15300);
        assert_eq!(u2.output_tokens, 2500);
        assert_eq!(u2.cost_usd, Some(0.045));
    }

    #[test]
    fn parses_fixture() {
        let mut p = Aider {
            cwd: "/work".into(),
            session_id: "fixture".into(),
            ..Default::default()
        };
        let evs = replay(&mut p, "aider.txt");
        assert_eq!(count(&evs, |k| matches!(k, EventKind::Thread { .. })), 1);
        assert_eq!(count(&evs, |k| matches!(k, EventKind::Model { .. })), 1);
        assert_eq!(count(&evs, |k| matches!(k, EventKind::Text { .. })), 2);
        assert!(evs
            .iter()
            .any(|e| matches!(e, crate::chat::Ev::File(f) if f.path == "src/lib.rs")));
        assert_eq!(count(&evs, |k| matches!(k, EventKind::Done { .. })), 1);

        let done = evs
            .iter()
            .find_map(|e| match e {
                crate::chat::Ev::Emit(k @ EventKind::Done { .. }) => Some(k.clone()),
                _ => None,
            })
            .expect("Done event");
        let u = done.usage().expect("usage");
        assert_eq!(u.input_tokens, 1200);
        assert_eq!(u.output_tokens, 420);
        assert_eq!(u.cost_usd, Some(0.012));
        assert_eq!(u.source, tokage_core::UsageSource::StdoutRegex);
    }
}

#[cfg(test)]
mod runtime_tests {
    use super::*;
    use crate::chat::{testing, Ev};

    #[test]
    fn missing_usage_and_api_errors_cannot_succeed() {
        for line in [
            "Aider v0.86.2",
            "litellm.ServiceUnavailableError: Service is too busy",
            "litellm.AuthenticationError: Invalid API key",
            "litellm.Timeout: timeout",
        ] {
            let mut parser = Aider::default();
            let mut out = Out::default();
            parser.text_line(line, &mut out);
            parser.finish(&mut out);
            assert!(!out
                .evs
                .iter()
                .any(|e| matches!(e, Ev::Emit(EventKind::Done { .. }))));
            assert!(out.evs.iter().any(|e| matches!(e, Ev::Failed(_))));
        }
    }
    #[test]
    fn discussing_api_errors_is_not_a_cli_failure() {
        let mut parser = Aider::default();
        let mut out = Out::default();
        parser.text_line(
            "Handle ServiceUnavailableError and Invalid API key in the UI.",
            &mut out,
        );
        assert!(!out.evs.iter().any(|e| matches!(e, Ev::Failed(_))));
    }
    #[test]
    fn cache_reports_and_multiline_costs_accumulate_per_request() {
        let mut parser = Aider {
            model: Some("deepseek/deepseek-flash".into()),
            ..Default::default()
        };
        let mut out = Out::default();
        for line in [
            "Tokens: 1.2k sent, 900 cache hit, 20 received.",
            "Cost: $0.001 message, $0.001 session.",
            "Tokens: 100 sent, 30 received. Cost: $0.002 message.",
        ] {
            parser.text_line(line, &mut out);
        }
        parser.finish(&mut out);
        let event = out
            .evs
            .iter()
            .find_map(|e| {
                if let Ev::Emit(e @ EventKind::Done { .. }) = e {
                    Some(e)
                } else {
                    None
                }
            })
            .unwrap();
        let usage = event.usage().unwrap();
        assert_eq!(
            (
                usage.input_tokens,
                usage.cache_read_tokens,
                usage.output_tokens,
                usage.api_calls
            ),
            (400, 900, 50, Some(2))
        );
        assert_eq!(usage.cost_usd, Some(0.003));
        assert_eq!(usage.context_tokens, None);
    }
    #[tokio::test]
    async fn histories_are_isolated_and_resume_is_validated() {
        let home = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("m.py"), "print(1)").unwrap();
        std::fs::write(root.path().join("other.py"), "print(2)").unwrap();
        std::fs::write(root.path().join(".env"), "SECRET=never-attach").unwrap();
        let env = ShellEnv {
            vars: [
                ("HOME".into(), home.path().to_string_lossy().into_owned()),
                ("PATH".into(), "/usr/bin:/bin".into()),
            ]
            .into(),
        };
        let mut req = testing::request(tokage_core::ProviderId::Aider);
        req.cwd = root.path().to_string_lossy().into_owned();
        req.write_scope = vec!["m.py".into()];
        let id = session_id(&req);
        let mut args = launch(&req, "test".into()).args;
        let _message = prepare(&req, &env, &mut args).await.unwrap();
        let path = args
            .windows(2)
            .find(|w| w[0] == "--chat-history-file")
            .unwrap()[1]
            .clone();
        assert!(Path::new(&path).starts_with(home.path()));
        assert!(args
            .windows(2)
            .any(|w| w[0] == "--file" && w[1].ends_with("m.py")));
        assert!(args
            .windows(2)
            .any(|w| w[0] == "--read" && w[1].ends_with("other.py")));
        assert!(!args.iter().any(|p| p.ends_with("/.env")));
        req.turn_id.push_str("-other");
        assert_ne!(session_id(&req), id);
        req.resume = Some(id.clone());
        assert!(prepare(&req, &env, &mut vec![]).await.is_err());
        std::fs::write(&path, "history").unwrap();
        let mut resumed_args = launch(&req, "resume".into()).args;
        let _resumed_message = prepare(&req, &env, &mut resumed_args).await.unwrap();
        assert_eq!(session_id(&req), id);
        req.resume = Some("../../escape".into());
        assert!(prepare(&req, &env, &mut vec![]).await.is_err());
    }
    #[tokio::test]
    async fn large_messages_preserve_bytes_without_entering_child_arguments() {
        let home = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let env = ShellEnv { vars: [
            ("HOME".into(), home.path().to_string_lossy().into_owned()),
            ("PATH".into(), "/usr/bin:/bin".into()),
        ].into() };
        let mut req = testing::request(tokage_core::ProviderId::Aider);
        req.cwd = root.path().to_string_lossy().into_owned();
        let prompt = format!("{}\nUnicode 保留信息\n'quotes' $literal `literal`\n", "source line\n".repeat(120_000));
        let mut args = launch(&req, prompt.clone()).args;
        let file = prepare(&req, &env, &mut args).await.unwrap();
        let path = file.path().to_owned();
        assert_eq!(std::fs::read(&path).unwrap(), prompt.as_bytes());
        assert!(!args.iter().any(|arg| arg == "--message" || arg == &prompt));
        assert!(args.windows(2).any(|pair| pair[0] == "--message-file" && Path::new(&pair[1]) == path));
        assert!(args.iter().map(|arg| arg.len()).sum::<usize>() < 16_384);
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        drop(file);
        assert!(!path.exists(), "message removed when launch resources are released");
    }

    #[test]
    fn inherited_message_sources_cannot_replace_the_current_request() {
        let req = testing::request(tokage_core::ProviderId::Aider);
        let launch = launch(&req, "current message".into());
        let env = ShellEnv { vars: [
            ("AIDER_MESSAGE".into(), "old message".into()),
            ("AIDER_MESSAGE_FILE".into(), "/old/message.txt".into()),
            ("DEEPSEEK_API_KEY".into(), "fixture-key".into()),
        ].into() };
        let child = crate::chat::child_env::assemble(tokage_core::ProviderId::Aider, &env, &launch.env_remove);
        assert!(!child.contains_key("AIDER_MESSAGE"));
        assert!(!child.contains_key("AIDER_MESSAGE_FILE"));
        assert_eq!(child.get("DEEPSEEK_API_KEY").map(String::as_str), Some("fixture-key"));
    }

    #[test]
    fn shell_and_startup_actions_are_disabled_in_every_role() {
        for role in [
            crate::chat::Role::Planner,
            crate::chat::Role::Executor,
            crate::chat::Role::Reviewer,
            crate::chat::Role::Tester,
        ] {
            let mut req = testing::request(tokage_core::ProviderId::Aider);
            req.role = Some(role);
            let args = launch(&req, "test".into()).args;
            for flag in [
                "--no-suggest-shell-commands",
                "--no-auto-lint",
                "--no-auto-test",
                "--no-auto-commits",
                "--no-notifications",
            ] {
                assert!(args.iter().any(|s| s == flag));
            }
        }
    }
}
