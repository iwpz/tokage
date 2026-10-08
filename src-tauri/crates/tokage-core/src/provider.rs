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
    Reasonix,
}

impl ProviderId {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ClaudeCode => "CLAUDE_CODE",
            Self::CodexCli => "CODEX_CLI",
            Self::CursorCli => "CURSOR_CLI",
            Self::AntigravityCli => "ANTIGRAVITY_CLI",
            Self::CopilotCli => "COPILOT_CLI",
            Self::Aider => "AIDER",
            Self::Opencode => "OPENCODE",
            Self::GrokCli => "GROK_CLI",
            Self::KiroCli => "KIRO_CLI",
            Self::QwenCode => "QWEN_CODE",
            Self::KimiCode => "KIMI_CODE",
            Self::QoderCli => "QODER_CLI",
            Self::Amp => "AMP",
            Self::Crush => "CRUSH",
            Self::Goose => "GOOSE",
            Self::Cline => "CLINE",
            Self::Droid => "DROID",
            Self::Auggie => "AUGGIE",
            Self::MuseCode => "MUSE_CODE",
            Self::TraeCli => "TRAE_CLI",
            Self::Reasonix => "REASONIX",
        }
    }
}

impl std::fmt::Display for ProviderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
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
