//! External/native session introspection models (docs/01 §5, P4.m2).

use serde::{Deserialize, Serialize};
use crate::provider::ProviderId;

/// An external/native session created and stored by a CLI tool directly on the local machine.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NativeSession {
    /// Identifier of the session (e.g. UUID, rollout id).
    pub id: String,
    /// Which CLI owns this session.
    pub provider_id: ProviderId,
    /// Title or summarized prompt for the session.
    pub title: String,
    /// Absolute path of the working directory or project root.
    pub cwd: String,
    /// Last modified or updated timestamp in UTC (ISO 8601 string).
    pub updated_at: String,
    /// Total bytes of the session file(s).
    pub file_size: u64,
    /// Absolute path of the primary session file or directory.
    pub path: String,
}
