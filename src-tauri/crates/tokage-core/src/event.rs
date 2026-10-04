//! The shared turn event model (docs/01 §7.1): every adapter maps its CLI stream onto it.

use serde::Serialize;

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
