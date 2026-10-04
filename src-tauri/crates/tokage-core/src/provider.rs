//! Provider identity and detection status, shared by every engine.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProviderId {
    ClaudeCode,
    CodexCli,
    CursorCli,
    AntigravityCli,
    CopilotCli,
    Aider,
    Opencode,
    GrokCli,
    KiroCli,
    QwenCode,
    KimiCode,
    QoderCli,
    Amp,
    Crush,
    Goose,
    Cline,
    Droid,
    Auggie,
    MuseCode,
    TraeCli,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProviderStatus {
    Ready,
    NotInstalled,
    NeedsLogin,
    UnsupportedVersion,
    UnknownAuth,
    Degraded,
    Melted,
}
