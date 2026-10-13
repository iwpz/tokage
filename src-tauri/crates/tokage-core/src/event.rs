//! The shared turn event model (docs/01 §7.1): every adapter maps its CLI stream onto it.

use serde::{Deserialize, Serialize};

/// Source classification for reported usage (docs/06 §3).
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UsageSource {
    Structured,
    StdoutRegex,
    Estimated,
    CliOutput,
}

impl Default for UsageSource {
    fn default() -> Self {
        Self::Structured
    }
}

impl UsageSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Structured => "STRUCTURED",
            Self::StdoutRegex => "STDOUT_REGEX",
            Self::Estimated => "ESTIMATED",
            Self::CliOutput => "CLI_OUTPUT",
        }
    }
}

/// Classification of turn runtime errors (docs/01 §9, P4.m3a).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TurnErrorKind {
    /// Failed to spawn process or binary not found.
    SpawnFailed,
    /// Login required / credentials missing. Changes status to NEEDS_LOGIN.
    AuthRequired,
    /// Rate limited / 429 / quota exhausted. Trips circuit breaker (MELTED).
    RateLimited,
    /// Idle, hard, or exit timeout reached.
    Timeout,
    /// Output could not be parsed by structured parser; degraded to plain text.
    ParseError,
    /// Process exited with non-zero exit code.
    NonZeroExit,
}

/// Comprehensive usage summary for a finished conversation turn (docs/01 §5, P3.m3a).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TurnUsage {
    /// Non-cached input tokens.
    pub input_tokens: u64,
    /// Cached prompt tokens read.
    #[serde(default)]
    pub cache_read_tokens: u64,
    /// Cache creation/write tokens.
    #[serde(default)]
    pub cache_write_tokens: u64,
    /// Generated output tokens.
    pub output_tokens: u64,
    /// Count of model/tool API calls in this turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_calls: Option<u32>,
    /// Total context tokens for the final request of this turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_tokens: Option<u64>,
    /// Estimated or billed cost in USD.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    /// Copilot/subscription premium requests consumed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub premium_requests: Option<f64>,
    /// How the usage was gathered.
    #[serde(default)]
    pub source: UsageSource,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatEvent {
    pub turn_id: String,
    #[serde(flatten)]
    pub kind: EventKind,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Change {
    Added,
    Modified,
    Deleted,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum EventKind {
    /// Explicit execution provenance; selected models remain separate from debug output.
    DebugRoute { output: String, requested_provider: String, requested_model: String, actual_provider: String, actual_model: String, workdir: String },
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
        #[serde(default)]
        cache_read_tokens: u64,
        #[serde(default)]
        cache_write_tokens: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        api_calls: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        context_tokens: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cost_usd: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        premium_requests: Option<f64>,
        #[serde(default)]
        source: UsageSource,
    },
    Error { message: String },
    /// A command or tool call requires user authorization before proceeding (P2.m3).
    ApprovalRequest {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_id: Option<String>,
        command: String,
        level: String,
        reason: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        recoverable: Option<bool>,
    },
    /// An approval was decided (P2.m3).
    ApprovalResolved {
        id: String,
        decision: String,
    },
    /// A security warning or notice (e.g. credential detected in diff, P2.m5e).
    Warning { message: String },
    /// Quota exhaustion, rate limiting or 429 encountered during a turn (docs/06 §4.1, P3.m7a).
    Quota {
        /// Raw error or notice text from the CLI / stderr.
        raw: String,
        /// Reset time in UTC (ISO 8601 string) or raw text if unparseable.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reset_at: Option<String>,
        /// Whether the CLI is automatically retrying (recoverable) or the turn was interrupted.
        retrying: bool,
    },
}

impl EventKind {
    pub fn usage(&self) -> Option<TurnUsage> {
        match self {
            EventKind::Done {
                input_tokens,
                output_tokens,
                cache_read_tokens,
                cache_write_tokens,
                api_calls,
                context_tokens,
                cost_usd,
                premium_requests,
                source,
            } => Some(TurnUsage {
                input_tokens: *input_tokens,
                cache_read_tokens: *cache_read_tokens,
                cache_write_tokens: *cache_write_tokens,
                output_tokens: *output_tokens,
                api_calls: *api_calls,
                context_tokens: *context_tokens,
                cost_usd: *cost_usd,
                premium_requests: *premium_requests,
                source: *source,
            }),
            _ => None,
        }
    }
}
