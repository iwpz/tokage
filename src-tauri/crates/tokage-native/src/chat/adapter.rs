//! `ProviderAdapter` (docs/01 §5): one per CLI with a headless path. Upper layers look an
//! adapter up by `ProviderId` and decide from its `AdapterMeta` (capability bits, tested
//! versions) instead of matching on the CLI.

use std::{cmp::Ordering, fmt};

use serde::{Serialize, Serializer};

use super::{
    aider, antigravity, kiro, claude, codex, copilot, cursor, opencode, reasonix, ChatRequest, DirectLimit,
    Launch,
};
use crate::{env::ShellEnv, providers::ProviderId};

/// Capability bits (docs/01 §5, docs/11 §4.1). Set only for what Tokage actually uses and
/// has verified on the tested version.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Caps(u32);

macro_rules! caps {
    ($($name:ident = $bit:expr, $doc:literal;)*) => {
        impl Caps {
            $(#[doc = $doc] pub const $name: Caps = Caps(1 << $bit);)*
            const NAMES: &'static [(Caps, &'static str)] = &[$((Caps::$name, stringify!($name)),)*];
        }
    };
}

caps! {
    STRUCTURED_OUTPUT = 0, "Machine-readable event stream (JSONL).";
    RESUME = 1, "Continues an earlier CLI session by id.";
    MODEL_SELECT = 2, "Takes the model id as a parameter.";
    READ_ONLY_MODE = 3, "Can run a turn that doesn't write files or run state-changing commands.";
    WRITE_SCOPE = 4, "Path-level write restriction.";
    COMPACT_HEADLESS = 5, "Native compaction in headless mode.";
    COMPACT_PTY = 6, "Native compaction through the interactive UI only.";
    USAGE_QUERY = 7, "Quota query that spends no tokens.";
    NATIVE_SESSIONS = 8, "Its own session files can be read.";
    NATIVE_FORK = 9, "Branches a resumed session into a new one, keeping the original.";
    APPROVAL_CALLBACK = 10, "Asks an external approver before running a tool.";
    TOOL_RULES = 11, "Takes allow / deny tool rules at launch.";
    DENIAL_EVENT = 12, "Reports a refused tool call in its structured output.";
    SHELL_WRAP = 13, "The shell or environment of the model's commands can be wrapped.";
    OS_SANDBOX = 14, "Works inside Tokage's OS-level sandbox.";
    OWN_SANDBOX = 15, "Has its own sandbox.";
}

impl Caps {
    pub const fn with(self, other: Caps) -> Caps {
        Caps(self.0 | other.0)
    }
    pub const fn has(self, other: Caps) -> bool {
        self.0 & other.0 == other.0
    }
    pub fn names(self) -> Vec<&'static str> {
        Self::NAMES
            .iter()
            .filter(|(c, _)| self.has(*c))
            .map(|(_, n)| *n)
            .collect()
    }
}

impl fmt::Debug for Caps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.names().join(" | "))
    }
}

impl Serialize for Caps {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(self.names())
    }
}

/// Versions the parser was checked against: `min` inclusive, `below` exclusive.
#[derive(Clone, Copy, Debug)]
pub struct VersionRange {
    pub min: &'static str,
    pub below: Option<&'static str>,
}

impl VersionRange {
    /// `None` when `v` has no numeric version to compare.
    pub fn contains(&self, v: &str) -> Option<bool> {
        let v = parse(v)?;
        let min = parse(self.min).is_some_and(|m| cmp(&v, &m) != Ordering::Less);
        let below = self
            .below
            .and_then(parse)
            .map_or(true, |b| cmp(&v, &b) == Ordering::Less);
        Some(min && below)
    }
}

impl fmt::Display for VersionRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.below {
            Some(b) => write!(f, ">= {}, < {b}", self.min),
            None => write!(f, ">= {}", self.min),
        }
    }
}

/// Leading numeric dot segments: "2.1.288" → [2, 1, 288]; "2026.10.01" → [2026, 10, 1].
/// Pre-release / build suffixes are ignored.
fn parse(v: &str) -> Option<Vec<u64>> {
    let core = v.split(['-', '+', ' ']).next()?;
    let parts: Vec<u64> = core
        .split('.')
        .map_while(|p| p.parse().ok())
        .collect();
    (!parts.is_empty()).then_some(parts)
}

/// Missing segments count as 0, so "1.0" == "1.0.0".
fn cmp(a: &[u64], b: &[u64]) -> Ordering {
    (0..a.len().max(b.len()))
        .map(|i| a.get(i).unwrap_or(&0).cmp(b.get(i).unwrap_or(&0)))
        .find(|o| o.is_ne())
        .unwrap_or(Ordering::Equal)
}

#[derive(Debug)]
pub struct AdapterMeta {
    pub id: ProviderId,
    pub versions: VersionRange,
    pub caps: Caps,
    /// Version the recorded sample comes from; always inside `versions`.
    pub tested: &'static str,
    /// Recorded output under tests/fixtures, replayed by the snapshot tests.
    #[cfg_attr(not(test), allow(dead_code))]
    pub sample: &'static str,
}

/// One CLI's headless path: how to start a turn and how Direct mode is restricted on it.
/// The returned `Launch` carries the stream parser for that turn.
pub trait ProviderAdapter: Send + Sync {
    fn meta(&self) -> &'static AdapterMeta;
    /// Arguments, stdin and parser for one turn. Arguments go to `Command::args`, never a shell.
    fn launch(&self, req: &ChatRequest, prompt: String) -> Launch;
    /// Why Direct can't run with full access on this CLI. `cwd` adds the project's own config.
    fn direct_limit(&self, _env: &ShellEnv, _cwd: Option<&str>) -> Option<DirectLimit> {
        None
    }
    /// Reads native CLI session files for this provider (docs/01 §5, P4.m2).
    fn read_native_sessions(&self, project: Option<&std::path::Path>) -> Vec<tokage_core::NativeSession> {
        crate::session::read_native_sessions(self.meta().id, project)
    }
    /// Interactive terminal resume argv including the registry command name (F10 / A.15).
    /// `None` = this CLI cannot resume by session id (e.g. Aider).
    fn terminal_resume(&self, _session_id: &str) -> Option<Vec<String>> {
        None
    }
}

struct Claude;
struct Codex;
struct Cursor;
struct Copilot;

const BASE: Caps = Caps::STRUCTURED_OUTPUT
    .with(Caps::RESUME)
    .with(Caps::MODEL_SELECT)
    .with(Caps::READ_ONLY_MODE);

static CLAUDE: AdapterMeta = AdapterMeta {
    id: ProviderId::ClaudeCode,
    versions: VersionRange { min: "2.1.288", below: Some("3") },
    // `--allowedTools` for Direct's read-only shell; `--fork-session` with `--resume`.
    caps: BASE
        .with(Caps::TOOL_RULES)
        .with(Caps::NATIVE_FORK)
        .with(Caps::DENIAL_EVENT)
        .with(Caps::OS_SANDBOX)
        .with(Caps::SHELL_WRAP)
        .with(Caps::NATIVE_SESSIONS)
        .with(Caps::USAGE_QUERY),
    tested: "2.1.288",
    sample: "claude.jsonl",
};

static CODEX: AdapterMeta = AdapterMeta {
    id: ProviderId::CodexCli,
    versions: VersionRange { min: "0.160.0", below: Some("1") },
    // `exec` has no approval channel or command rules; the read-only sandbox is its own.
    // Codex has OWN_SANDBOX; Seatbelt prohibits nested sandbox-exec (P2.m4e / docs/11 §5.2).
    caps: BASE
        .with(Caps::OWN_SANDBOX)
        .with(Caps::DENIAL_EVENT)
        .with(Caps::SHELL_WRAP)
        .with(Caps::NATIVE_SESSIONS),
    tested: "0.160.0",
    sample: "codex.jsonl",
};

static CURSOR: AdapterMeta = AdapterMeta {
    id: ProviderId::CursorCli,
    // Date-versioned and released often; no upper bound until a format break shows up.
    versions: VersionRange { min: "2026.10.01", below: None },
    // Refused shell calls come back as `rejected` results (docs/11 §4.2). Rules live only in
    // its config files, so no TOOL_RULES.
    caps: BASE
        .with(Caps::DENIAL_EVENT)
        .with(Caps::OS_SANDBOX)
        .with(Caps::NATIVE_SESSIONS),
    tested: "2026.10.01",
    sample: "cursor.jsonl",
};

static COPILOT: AdapterMeta = AdapterMeta {
    id: ProviderId::CopilotCli,
    versions: VersionRange { min: "1.0.91", below: Some("2") },
    // `--allow-tool` / `--deny-tool`.
    caps: BASE
        .with(Caps::TOOL_RULES)
        .with(Caps::DENIAL_EVENT)
        .with(Caps::OS_SANDBOX)
        .with(Caps::SHELL_WRAP)
        .with(Caps::NATIVE_SESSIONS)
        .with(Caps::USAGE_QUERY),
    tested: "1.0.96",
    sample: "copilot-1.0.96.jsonl",
};

static ANTIGRAVITY: AdapterMeta = AdapterMeta {
    id: ProviderId::AntigravityCli,
    versions: VersionRange { min: "1.3.4", below: Some("2") },
    // Current CLI has no per-launch allow/deny tools or secret-env-vars flags.
    caps: BASE.with(Caps::DENIAL_EVENT).with(Caps::OS_SANDBOX)
        .with(Caps::NATIVE_SESSIONS).with(Caps::USAGE_QUERY),
    tested: "1.3.4",
    sample: "antigravity-native.jsonl",
};

static AIDER: AdapterMeta = AdapterMeta {
    id: ProviderId::Aider,
    versions: VersionRange { min: "0.50.0", below: Some("2") },
    caps: Caps::MODEL_SELECT
        .with(Caps::READ_ONLY_MODE)
        .with(Caps::RESUME)
        .with(Caps::OS_SANDBOX),
    tested: "0.72.0",
    sample: "aider.txt",
};

static OPENCODE: AdapterMeta = AdapterMeta {
    id: ProviderId::Opencode,
    // Real CLI acceptance: 1.19.0 and 2.0.29; keep both branches supported.
    versions: VersionRange { min: "1.19.0", below: Some("3") },
    caps: BASE
        .with(Caps::TOOL_RULES)
        .with(Caps::OS_SANDBOX)
        .with(Caps::SHELL_WRAP),
    tested: "2.0.29",
    sample: "opencode.jsonl",
};

static REASONIX: AdapterMeta = AdapterMeta {
    id: ProviderId::Reasonix,
    // CLI reports 2.x today; Studio desktop is a separate product/session store.
    versions: VersionRange { min: "1.0.0", below: None },
    caps: BASE
        .with(Caps::NATIVE_FORK)
        .with(Caps::TOOL_RULES)
        .with(Caps::DENIAL_EVENT)
        .with(Caps::OS_SANDBOX)
        .with(Caps::SHELL_WRAP)
        .with(Caps::NATIVE_SESSIONS),
    tested: "2.31.0",
    sample: "reasonix.jsonl",
};

struct Antigravity;
struct Aider;
struct Opencode;
struct Reasonix;

struct Kiro;

static KIRO: AdapterMeta = AdapterMeta {
    id: ProviderId::KiroCli,
    versions: VersionRange { min: "2.31.1", below: Some("3") },
    caps: BASE.with(Caps::TOOL_RULES).with(Caps::OS_SANDBOX).with(Caps::NATIVE_SESSIONS),
    tested: "2.31.1",
    sample: "kiro-v2.jsonl",
};

impl ProviderAdapter for Kiro {
    fn meta(&self) -> &'static AdapterMeta { &KIRO }
    fn launch(&self, req: &ChatRequest, prompt: String) -> Launch { kiro::launch(req,prompt) }
    fn terminal_resume(&self, id: &str) -> Option<Vec<String>> {
        let (engine, raw) = kiro::session(id).ok()?;
        Some(vec!["kiro-cli".into(), "chat".into(), format!("--{engine}"), "--resume-id".into(),raw.into()])
    }
}


impl ProviderAdapter for Claude {
    fn meta(&self) -> &'static AdapterMeta {
        &CLAUDE
    }
    fn launch(&self, req: &ChatRequest, prompt: String) -> Launch {
        claude::launch(req, prompt)
    }
    fn terminal_resume(&self, session_id: &str) -> Option<Vec<String>> {
        Some(vec![
            "claude".into(),
            "--resume".into(),
            session_id.to_string(),
        ])
    }
}

impl ProviderAdapter for Codex {
    fn meta(&self) -> &'static AdapterMeta {
        &CODEX
    }
    fn launch(&self, req: &ChatRequest, prompt: String) -> Launch {
        codex::launch(req, prompt)
    }
    fn direct_limit(&self, _env: &ShellEnv, _cwd: Option<&str>) -> Option<DirectLimit> {
        Some(DirectLimit::CodexReadOnly)
    }
    fn terminal_resume(&self, session_id: &str) -> Option<Vec<String>> {
        // Interactive takeover (not headless `codex exec … resume`).
        Some(vec!["codex".into(), "resume".into(), session_id.to_string()])
    }
}

impl ProviderAdapter for Cursor {
    fn meta(&self) -> &'static AdapterMeta {
        &CURSOR
    }
    fn launch(&self, req: &ChatRequest, prompt: String) -> Launch {
        cursor::launch(req, prompt)
    }
    fn direct_limit(&self, env: &ShellEnv, cwd: Option<&str>) -> Option<DirectLimit> {
        let detail = cursor::direct_risks(env, cwd);
        (!detail.is_empty()).then_some(DirectLimit::CursorUnsafePermissions { detail })
    }
    fn terminal_resume(&self, session_id: &str) -> Option<Vec<String>> {
        Some(vec![
            "cursor-agent".into(),
            "--resume".into(),
            session_id.to_string(),
        ])
    }
}

impl ProviderAdapter for Copilot {
    fn meta(&self) -> &'static AdapterMeta {
        &COPILOT
    }
    fn launch(&self, req: &ChatRequest, prompt: String) -> Launch {
        copilot::launch(req, prompt)
    }
    fn terminal_resume(&self, session_id: &str) -> Option<Vec<String>> {
        Some(vec![
            "copilot".into(),
            "--resume".into(),
            session_id.to_string(),
        ])
    }
}

impl ProviderAdapter for Antigravity {
    fn meta(&self) -> &'static AdapterMeta {
        &ANTIGRAVITY
    }
    fn launch(&self, req: &ChatRequest, prompt: String) -> Launch {
        antigravity::launch(req, prompt)
    }
    fn terminal_resume(&self, session_id: &str) -> Option<Vec<String>> {
        Some(vec!["agy".into(), "--conversation".into(), session_id.to_string()])
    }
}

impl ProviderAdapter for Aider {
    fn meta(&self) -> &'static AdapterMeta {
        &AIDER
    }
    fn launch(&self, req: &ChatRequest, prompt: String) -> Launch {
        aider::launch(req, prompt)
    }
    fn terminal_resume(&self, _session_id: &str) -> Option<Vec<String>> {
        // Aider has no id-based resume; UI should hint `--restore-chat-history`.
        None
    }
}

impl ProviderAdapter for Opencode {
    fn meta(&self) -> &'static AdapterMeta {
        &OPENCODE
    }
    fn launch(&self, req: &ChatRequest, prompt: String) -> Launch {
        opencode::launch(req, prompt)
    }
    fn terminal_resume(&self, session_id: &str) -> Option<Vec<String>> {
        Some(vec![
            "opencode".into(),
            "--session".into(),
            session_id.to_string(),
        ])
    }
}

impl ProviderAdapter for Reasonix {
    fn meta(&self) -> &'static AdapterMeta {
        &REASONIX
    }
    fn launch(&self, req: &ChatRequest, prompt: String) -> Launch {
        reasonix::launch(req, prompt)
    }
    fn terminal_resume(&self, session_id: &str) -> Option<Vec<String>> {
        Some(vec![
            "reasonix".into(),
            "--resume".into(),
            session_id.to_string(),
        ])
    }
}

static REGISTRY: [&dyn ProviderAdapter; 9] = [
    &Claude,
    &Codex,
    &Cursor,
    &Copilot,
    &Antigravity,
    &Aider,
    &Opencode,
    &Reasonix,
    &Kiro,
];

/// The adapter for a CLI; `None` for whitelisted CLIs that are only detected.
pub fn adapter(id: ProviderId) -> Option<&'static dyn ProviderAdapter> {
    REGISTRY.iter().copied().find(|a| a.meta().id == id)
}

/// Shell-single-quote a path/arg for pasteable terminal commands (F10).
pub fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn shell_join_argv(argv: &[String]) -> String {
    argv.iter()
        .map(|a| {
            if a.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_./:=+".contains(c))
            {
                a.clone()
            } else {
                shell_single_quote(a)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Pasteable one-liner: `cd '<cwd>' && <cmd> <args…>` (F10 / A.15).
/// Returns `None` when `thread_id` is missing/empty or the CLI cannot resume by id.
pub fn terminal_resume_command_line(
    provider: ProviderId,
    thread_id: Option<&str>,
    cwd: &str,
) -> Option<String> {
    let id = thread_id.filter(|s| !s.is_empty())?;
    let argv = adapter(provider)?.terminal_resume(id)?;
    Some(format!(
        "cd {} && {}",
        shell_single_quote(cwd),
        shell_join_argv(&argv)
    ))
}

/// Join `terminal_resume` argv without `cd` (used by repair MarkBlocked).
pub fn terminal_resume_argv_line(provider: ProviderId, session_id: &str) -> Option<String> {
    adapter(provider)?
        .terminal_resume(session_id)
        .map(|argv| shell_join_argv(&argv))
}

/// Fuzzy-parse provider strings from repair / allocation (`codex`, `CLAUDE_CODE`, …).
pub fn parse_provider_id_fuzzy(s: &str) -> Option<ProviderId> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(id) = serde_json::from_value::<ProviderId>(serde_json::Value::String(
        trimmed.to_ascii_uppercase(),
    )) {
        return Some(id);
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("claude") {
        Some(ProviderId::ClaudeCode)
    } else if lower.contains("codex") {
        Some(ProviderId::CodexCli)
    } else if lower.contains("cursor") {
        Some(ProviderId::CursorCli)
    } else if lower.contains("copilot") {
        Some(ProviderId::CopilotCli)
    } else if lower.contains("antigravity") || lower == "agy" {
        Some(ProviderId::AntigravityCli)
    } else if lower.contains("opencode") {
        Some(ProviderId::Opencode)
    } else if lower.contains("aider") {
        Some(ProviderId::Aider)
    } else if lower.contains("reasonix") {
        Some(ProviderId::Reasonix)
    } else {
        None
    }
}

#[cfg(test)]
pub fn all() -> &'static [&'static dyn ProviderAdapter] {
    &REGISTRY
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{testing::*, EventKind};

    #[test]
    fn version_ranges() {
        let r = VersionRange { min: "2.1.0", below: Some("3") };
        assert_eq!(r.contains("2.1.0"), Some(true));
        assert_eq!(r.contains("2.10.4"), Some(true));
        assert_eq!(r.contains("2.0.99"), Some(false));
        assert_eq!(r.contains("3.0.0"), Some(false));
        assert_eq!(r.contains("2.9.9-beta.1"), Some(true));
        assert_eq!(r.contains("unknown"), None);
        let open = VersionRange { min: "2026.10.01", below: None };
        assert_eq!(open.contains("2026.9.30"), Some(false));
        assert_eq!(open.contains("2027.1.2"), Some(true));
        assert_eq!(r.to_string(), ">= 2.1.0, < 3");
    }

    #[test]
    fn caps_serialize_as_names() {
        let c = Caps::RESUME.with(Caps::DENIAL_EVENT);
        assert_eq!(serde_json::to_value(c).unwrap(), serde_json::json!(["RESUME", "DENIAL_EVENT"]));
        assert!(c.has(Caps::RESUME) && !c.has(Caps::TOOL_RULES));
        assert!(Caps(0).names().is_empty());
    }

    /// Every adapter's recorded sample is inside its range and replays to a finished turn.
    #[test]
    fn registry_samples() {
        for a in all() {
            let m = a.meta();
            assert!(std::ptr::eq(adapter(m.id).unwrap().meta(), m));
            assert_eq!(m.versions.contains(m.tested), Some(true), "{:?}", m.id);
            if m.id != ProviderId::Aider {
                assert!(m.caps.has(BASE), "{:?}", m.id);
            } else {
                assert!(m.caps.has(Caps::MODEL_SELECT.with(Caps::READ_ONLY_MODE).with(Caps::RESUME)), "{:?}", m.id);
            }
            let mut req = request(m.id);
            req.cwd = "/work".into();
            let mut parser = a.launch(&req, "p".into()).parser;
            let evs = replay(parser.as_mut(), m.sample);
            assert_eq!(count(&evs, |k| matches!(k, EventKind::Done { .. })), 1, "{:?}", m.id);
            assert!(count(&evs, |k| matches!(k, EventKind::Thread { .. })) >= 1, "{:?}", m.id);
        }
        assert!(adapter(ProviderId::GrokCli).is_none());
    }

    #[test]
    fn direct_limits() {
        let env = ShellEnv::default();
        assert_eq!(
            adapter(ProviderId::CodexCli).unwrap().direct_limit(&env, None),
            Some(DirectLimit::CodexReadOnly)
        );
        assert_eq!(adapter(ProviderId::ClaudeCode).unwrap().direct_limit(&env, None), None);
    }

    /// Acceptance: Verify that all featured CLI adapters produce expected usage fields matching fixtures.
    #[test]
    fn sample_usage_fields_match_fixtures() {
        for a in all() {
            let m = a.meta();
            let mut req = request(m.id);
            req.cwd = "/work".into();
            let mut parser = a.launch(&req, "p".into()).parser;
            let evs = replay(parser.as_mut(), m.sample);
            let done = evs
                .iter()
                .find_map(|e| match e {
                    crate::chat::Ev::Emit(k @ EventKind::Done { .. }) => Some(k.clone()),
                    _ => None,
                })
                .expect("Expected Done event");

            let usage = done.usage().expect("TurnUsage from Done event");
            if m.id == ProviderId::Aider {
                assert_eq!(usage.source, tokage_core::UsageSource::StdoutRegex);
            } else {
                assert_eq!(usage.source, tokage_core::UsageSource::Structured);
            }

            match m.id {
                ProviderId::ClaudeCode => {
                    assert_eq!(usage.input_tokens, 60);
                    assert_eq!(usage.output_tokens, 335);
                    assert_eq!(usage.cache_read_tokens, 107833);
                    assert_eq!(usage.cache_write_tokens, 28664);
                    assert_eq!(usage.api_calls, Some(4));
                    assert_eq!(usage.context_tokens, Some(34319));
                    assert_eq!(usage.cost_usd, Some(0.0483483));
                    assert_eq!(usage.premium_requests, None);
                }
                ProviderId::CodexCli => {
                    assert_eq!(usage.input_tokens, 90926);
                    assert_eq!(usage.output_tokens, 582);
                    assert_eq!(usage.cache_read_tokens, 81664);
                    assert_eq!(usage.cache_write_tokens, 0);
                    assert_eq!(usage.api_calls, Some(4));
                    assert_eq!(usage.context_tokens, Some(90926));
                    assert_eq!(usage.cost_usd, None);
                    assert_eq!(usage.premium_requests, None);
                }
                ProviderId::CursorCli => {
                    assert_eq!(usage.input_tokens, 14839);
                    assert_eq!(usage.output_tokens, 599);
                    assert_eq!(usage.cache_read_tokens, 56384);
                    assert_eq!(usage.cache_write_tokens, 0);
                    assert_eq!(usage.api_calls, Some(5));
                    assert_eq!(usage.context_tokens, Some(71223));
                    assert_eq!(usage.cost_usd, None);
                    assert_eq!(usage.premium_requests, None);
                }
                ProviderId::CopilotCli => {
                    assert_eq!(usage.input_tokens, 0);
                    assert_eq!(usage.output_tokens, 0);
                    assert_eq!(usage.cache_read_tokens, 0);
                    assert_eq!(usage.cache_write_tokens, 0);
                    assert_eq!(usage.api_calls, Some(2));
                    assert_eq!(usage.context_tokens, None);
                    assert_eq!(usage.cost_usd, None);
                    assert_eq!(usage.premium_requests, Some(1.0));
                }
                ProviderId::AntigravityCli => {
                    assert_eq!(usage.input_tokens, 12696);
                    assert_eq!(usage.output_tokens, 7);
                    assert_eq!(usage.cache_read_tokens, 0);
                    assert_eq!(usage.cache_write_tokens, 0);
                    assert_eq!(usage.api_calls, None);
                    assert_eq!(usage.context_tokens, None);
                    assert_eq!(usage.cost_usd, None);
                    assert_eq!(usage.premium_requests, None);
                }
                ProviderId::Aider => {
                    assert_eq!(usage.input_tokens, 1200);
                    assert_eq!(usage.output_tokens, 420);
                    assert_eq!(usage.cache_read_tokens, 0);
                    assert_eq!(usage.cache_write_tokens, 0);
                    assert_eq!(usage.api_calls, Some(1));
                    assert_eq!(usage.context_tokens, None);
                    assert_eq!(usage.cost_usd, Some(0.012));
                    assert_eq!(usage.premium_requests, None);
                }
                ProviderId::Opencode => {
                    assert_eq!(usage.input_tokens, 1500);
                    assert_eq!(usage.output_tokens, 320);
                    assert_eq!(usage.cache_read_tokens, 6000);
                    assert_eq!(usage.cache_write_tokens, 0);
                    assert_eq!(usage.api_calls, Some(2));
                    assert_eq!(usage.context_tokens, Some(7820));
                    assert_eq!(usage.cost_usd, Some(0.0045));
                    assert_eq!(usage.premium_requests, None);
                }
                ProviderId::Reasonix => {
                    assert_eq!(usage.input_tokens, 7492);
                    assert_eq!(usage.output_tokens, 13);
                    assert_eq!(usage.cache_read_tokens, 0);
                    assert_eq!(usage.cache_write_tokens, 7492);
                    assert_eq!(usage.api_calls, None);
                    assert_eq!(usage.context_tokens, Some(7505));
                    assert_eq!(usage.cost_usd, Some(0.0011316));
                    assert_eq!(usage.premium_requests, None);
                }
                ProviderId::KiroCli => {
                    assert_eq!(usage.input_tokens + usage.output_tokens, 0);
                    assert!(usage.credits.unwrap() > 0.0);
                    assert_eq!(usage.cost_usd, None);
                }
                _ => panic!("Unexpected provider {:?}", m.id),
            }
        }
    }

    /// Role mapping to CLI parameters across all featured providers.
    #[test]
    fn role_launch_parameter_mapping() {
        use tokage_core::Role;

        // 1. Claude Code
        let mut req_claude = crate::chat::testing::request(ProviderId::ClaudeCode);
        req_claude.role = Some(Role::Planner);
        let launch = adapter(ProviderId::ClaudeCode).unwrap().launch(&req_claude, "test".into());
        assert!(launch.args.windows(2).any(|w| w[0] == "--permission-mode" && w[1] == "default"));
        assert!(launch.args.iter().any(|a| a == "--disallowedTools"));

        req_claude.role = Some(Role::Reviewer);
        let launch = adapter(ProviderId::ClaudeCode).unwrap().launch(&req_claude, "test".into());
        assert!(launch.args.windows(2).any(|w| w[0] == "--permission-mode" && w[1] == "default"));

        req_claude.role = Some(Role::Executor);
        let launch = adapter(ProviderId::ClaudeCode).unwrap().launch(&req_claude, "test".into());
        assert!(launch.args.windows(2).any(|w| w[0] == "--permission-mode" && w[1] == "acceptEdits"));

        // 2. Codex
        let mut req_codex = crate::chat::testing::request(ProviderId::CodexCli);
        req_codex.role = Some(Role::Planner);
        let launch = adapter(ProviderId::CodexCli).unwrap().launch(&req_codex, "test".into());
        assert!(launch.args.iter().any(|a| a.contains("sandbox_mode=\"read-only\"")));

        req_codex.role = Some(Role::Reviewer);
        let launch = adapter(ProviderId::CodexCli).unwrap().launch(&req_codex, "test".into());
        assert!(launch.args.iter().any(|a| a.contains("sandbox_mode=\"read-only\"")));

        req_codex.role = Some(Role::Executor);
        req_codex.session_allowed = vec!["npm test".into()];
        let launch = adapter(ProviderId::CodexCli).unwrap().launch(&req_codex, "test".into());
        assert!(launch.args.iter().any(|a| a.contains("sandbox_mode=\"workspace-write\"")));

        // 3. Cursor
        let mut req_cursor = crate::chat::testing::request(ProviderId::CursorCli);
        req_cursor.role = Some(Role::Planner);
        let launch = adapter(ProviderId::CursorCli).unwrap().launch(&req_cursor, "test".into());
        assert!(launch.args.windows(2).any(|w| w[0] == "--mode" && w[1] == "ask"));

        req_cursor.role = Some(Role::Executor);
        let launch = adapter(ProviderId::CursorCli).unwrap().launch(&req_cursor, "test".into());
        assert!(!launch.args.iter().any(|a| a == "--mode"));

        // 4. Copilot
        let mut req_copilot = crate::chat::testing::request(ProviderId::CopilotCli);
        req_copilot.role = Some(Role::Planner);
        let launch = adapter(ProviderId::CopilotCli).unwrap().launch(&req_copilot, "test".into());
        assert!(launch.args.iter().any(|a| a == "--plan"));
        assert!(launch.args.windows(2).any(|w| w[0] == "--deny-tool" && w[1] == "write"));

        req_copilot.role = Some(Role::Executor);
        let launch = adapter(ProviderId::CopilotCli).unwrap().launch(&req_copilot, "test".into());
        assert!(launch.args.windows(2).any(|w| w[0] == "--allow-tool" && w[1] == "write"));

        // 5. Antigravity
        let mut req_antigravity = crate::chat::testing::request(ProviderId::AntigravityCli);
        req_antigravity.role = Some(Role::Planner);
        let launch = adapter(ProviderId::AntigravityCli).unwrap().launch(&req_antigravity, "test".into());
        assert!(launch.args.windows(2).any(|w| w[0] == "--mode" && w[1] == "plan"));
        assert!(!launch.args.iter().any(|a| a == "--disallowedTools"));

        req_antigravity.role = Some(Role::Executor);
        let launch = adapter(ProviderId::AntigravityCli).unwrap().launch(&req_antigravity, "test".into());
        assert!(launch.args.windows(2).any(|w| w[0] == "--mode" && w[1] == "accept-edits"));

        // 6. Aider
        let mut req_aider = crate::chat::testing::request(ProviderId::Aider);
        req_aider.role = Some(Role::Planner);
        let launch = adapter(ProviderId::Aider).unwrap().launch(&req_aider, "test".into());
        assert!(launch.args.iter().any(|a| a == "--dry-run"));

        req_aider.role = Some(Role::Executor);
        let launch = adapter(ProviderId::Aider).unwrap().launch(&req_aider, "test".into());
        assert!(!launch.args.iter().any(|a| a == "--dry-run"));

        // 7. OpenCode
        let mut req_opencode = crate::chat::testing::request(ProviderId::Opencode);
        req_opencode.role = Some(Role::Planner);
        let launch = adapter(ProviderId::Opencode).unwrap().launch(&req_opencode, "test".into());
        assert!(launch.args.windows(2).any(|w| w[0] == "--agent" && w[1] == "plan"));
        assert!(!launch.args.iter().any(|w| w == "--deny-tools"));

        req_opencode.role = Some(Role::Executor);
        let launch = adapter(ProviderId::Opencode).unwrap().launch(&req_opencode, "test".into());
        assert!(launch.args.windows(2).any(|w| w[0] == "--agent" && w[1] == "build"));

        // 8. Reasonix
        let mut req_rx = crate::chat::testing::request(ProviderId::Reasonix);
        req_rx.role = Some(Role::Planner);
        let launch = adapter(ProviderId::Reasonix).unwrap().launch(&req_rx, "test".into());
        assert!(launch
            .args
            .windows(2)
            .any(|w| w[0] == "--permission-mode" && w[1] == "read-only"));

        req_rx.role = Some(Role::Executor);
        let launch = adapter(ProviderId::Reasonix).unwrap().launch(&req_rx, "test".into());
        assert!(launch
            .args
            .windows(2)
            .any(|w| w[0] == "--permission-mode" && w[1] == "acceptEdits"));
        assert!(!launch.args.iter().any(|a| a.contains("yolo") || a.contains("bypass")));
    }

    /// F10.a / A.15: table-driven terminal_resume argv per provider.
    #[test]
    fn f10a_terminal_resume_matches_a15() {
        let id = "sess-abc";
        let cases: &[(ProviderId, Option<&[&str]>)] = &[
            (
                ProviderId::ClaudeCode,
                Some(&["claude", "--resume", id]),
            ),
            (ProviderId::CodexCli, Some(&["codex", "resume", id])),
            (
                ProviderId::CursorCli,
                Some(&["cursor-agent", "--resume", id]),
            ),
            (
                ProviderId::CopilotCli,
                Some(&["copilot", "--resume", id]),
            ),
            (
                ProviderId::AntigravityCli,
                Some(&["agy", "--conversation", id]),
            ),
            (
                ProviderId::Opencode,
                Some(&["opencode", "--session", id]),
            ),
            (
                ProviderId::Reasonix,
                Some(&["reasonix", "--resume", id]),
            ),
            (ProviderId::Aider, None),
        ];
        for (provider, expected) in cases {
            let got = adapter(*provider).unwrap().terminal_resume(id);
            match expected {
                None => assert!(got.is_none(), "{provider:?} should be None"),
                Some(exp) => {
                    let got = got.expect(&format!("{provider:?}"));
                    assert_eq!(got, *exp, "{provider:?}");
                }
            }
        }

        let line = terminal_resume_command_line(
            ProviderId::ClaudeCode,
            Some(id),
            "/tmp/proj's dir",
        )
        .unwrap();
        assert_eq!(
            line,
            "cd '/tmp/proj'\\''s dir' && claude --resume sess-abc"
        );
        assert!(terminal_resume_command_line(ProviderId::ClaudeCode, None, "/tmp").is_none());
        assert!(terminal_resume_command_line(ProviderId::Aider, Some(id), "/tmp").is_none());
    }
}
