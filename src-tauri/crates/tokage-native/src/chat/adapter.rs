//! `ProviderAdapter` (docs/01 §5): one per CLI with a headless path. Upper layers look an
//! adapter up by `ProviderId` and decide from its `AdapterMeta` (capability bits, tested
//! versions) instead of matching on the CLI.

use std::{cmp::Ordering, fmt};

use serde::{Serialize, Serializer};

use super::{claude, codex, copilot, cursor, ChatRequest, DirectLimit, Launch};
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
    caps: BASE.with(Caps::TOOL_RULES).with(Caps::NATIVE_FORK),
    tested: "2.1.288",
    sample: "claude.jsonl",
};

static CODEX: AdapterMeta = AdapterMeta {
    id: ProviderId::CodexCli,
    versions: VersionRange { min: "0.160.0", below: Some("1") },
    // `exec` has no approval channel or command rules; the read-only sandbox is its own.
    caps: BASE.with(Caps::OWN_SANDBOX),
    tested: "0.160.0",
    sample: "codex.jsonl",
};

static CURSOR: AdapterMeta = AdapterMeta {
    id: ProviderId::CursorCli,
    // Date-versioned and released often; no upper bound until a format break shows up.
    versions: VersionRange { min: "2026.10.01", below: None },
    // Refused shell calls come back as `rejected` results (docs/11 §4.2). Rules live only in
    // its config files, so no TOOL_RULES.
    caps: BASE.with(Caps::DENIAL_EVENT),
    tested: "2026.10.01",
    sample: "cursor.jsonl",
};

static COPILOT: AdapterMeta = AdapterMeta {
    id: ProviderId::CopilotCli,
    versions: VersionRange { min: "1.0.91", below: Some("2") },
    // `--allow-tool` / `--deny-tool`.
    caps: BASE.with(Caps::TOOL_RULES),
    tested: "1.0.91",
    sample: "copilot.jsonl",
};

impl ProviderAdapter for Claude {
    fn meta(&self) -> &'static AdapterMeta {
        &CLAUDE
    }
    fn launch(&self, req: &ChatRequest, prompt: String) -> Launch {
        claude::launch(req, prompt)
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
}

impl ProviderAdapter for Copilot {
    fn meta(&self) -> &'static AdapterMeta {
        &COPILOT
    }
    fn launch(&self, req: &ChatRequest, prompt: String) -> Launch {
        copilot::launch(req, prompt)
    }
}

static REGISTRY: [&dyn ProviderAdapter; 4] = [&Claude, &Codex, &Cursor, &Copilot];

/// The adapter for a CLI; `None` for whitelisted CLIs that are only detected.
pub fn adapter(id: ProviderId) -> Option<&'static dyn ProviderAdapter> {
    REGISTRY.iter().copied().find(|a| a.meta().id == id)
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
            assert!(m.caps.has(BASE), "{:?}", m.id);
            let mut req = request(m.id);
            req.cwd = "/work".into();
            let mut parser = a.launch(&req, "p".into()).parser;
            let evs = replay(parser.as_mut(), m.sample);
            assert_eq!(count(&evs, |k| matches!(k, EventKind::Done { .. })), 1, "{:?}", m.id);
            assert!(count(&evs, |k| matches!(k, EventKind::Thread { .. })) >= 1, "{:?}", m.id);
        }
        assert!(adapter(ProviderId::Aider).is_none());
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
}
