//! Compact execution and CLI native compression integration (docs/05 §5, P8.m1c).
//!
//! Investigates CLI native compact capabilities and generates compact prompts or commands:
//!
//! Research Summary:
//! - Claude Code: Supports native `/compact [instructions]` slash command.
//! - Codex CLI: No native `/compact` slash command (uses rolling session rollout files).
//! - Cursor CLI: No native `/compact` slash command (context managed via internal prompt cache).
//! - Copilot CLI: No native `/compact` command.
//! - Aider: Supports `/clear` or `--chat-history-file` reduction.
//! - OpenCode / Antigravity: Context managed per session.
//!
//! Tokage Standard Compact Instruction:
//! `"保留当前任务目标、契约路径、未解决错误"`

use serde::{Deserialize, Serialize};

/// Standard instruction appended to compaction requests (docs/05 §5).
pub const STANDARD_COMPACT_INSTRUCTION: &str = "保留当前任务目标、契约路径、未解决错误";

/// Method of compact execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CompactMethod {
    /// Native CLI slash command (e.g. Claude Code `/compact`).
    NativeSlashCommand,
    /// Prompt-based compact instruction for CLI without native slash command.
    InstructionPrompt,
    /// Fresh restart with synthesized compact brief.
    SynthesizedFreshBrief,
}

/// Compact capability metadata for a CLI provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCompactCapability {
    pub provider_name: String,
    pub supports_native_compact: bool,
    pub compact_command: Option<String>,
    pub compact_method: CompactMethod,
    pub notes: String,
}

/// Retrieves provider compact capability based on investigated support.
pub fn get_provider_compact_capability(provider_name: &str) -> ProviderCompactCapability {
    let name_upper = provider_name.to_ascii_uppercase();
    match name_upper.as_str() {
        "CLAUDE" | "CLAUDE_CODE" => ProviderCompactCapability {
            provider_name: provider_name.to_string(),
            supports_native_compact: true,
            compact_command: Some("/compact".to_string()),
            compact_method: CompactMethod::NativeSlashCommand,
            notes: "Claude Code supports native /compact with custom retention instruction.".to_string(),
        },
        "AIDER" => ProviderCompactCapability {
            provider_name: provider_name.to_string(),
            supports_native_compact: true,
            compact_command: Some("/clear".to_string()),
            compact_method: CompactMethod::NativeSlashCommand,
            notes: "Aider supports /clear and history trimming.".to_string(),
        },
        "CURSOR" => ProviderCompactCapability {
            provider_name: provider_name.to_string(),
            supports_native_compact: false,
            compact_command: None,
            compact_method: CompactMethod::InstructionPrompt,
            notes: "Cursor CLI does not expose native /compact; using prompt-guided compact instruction.".to_string(),
        },
        "COPILOT" => ProviderCompactCapability {
            provider_name: provider_name.to_string(),
            supports_native_compact: false,
            compact_command: None,
            compact_method: CompactMethod::InstructionPrompt,
            notes: "Copilot CLI does not expose native /compact; using prompt-guided compact instruction.".to_string(),
        },
        "CODEX" => ProviderCompactCapability {
            provider_name: provider_name.to_string(),
            supports_native_compact: false,
            compact_command: None,
            compact_method: CompactMethod::SynthesizedFreshBrief,
            notes: "Codex CLI uses immutable rollout jsonl sessions; fresh resume with compact brief is optimal.".to_string(),
        },
        _ => ProviderCompactCapability {
            provider_name: provider_name.to_string(),
            supports_native_compact: false,
            compact_command: None,
            compact_method: CompactMethod::InstructionPrompt,
            notes: "Standard CLI without native compact command; using prompt-guided compact instruction.".to_string(),
        },
    }
}

/// Structured request to build a compaction payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildCompactRequest {
    pub provider_name: String,
    pub task_goal: String,
    pub contract_paths: Vec<String>,
    pub pending_errors: Option<String>,
    pub custom_instruction: Option<String>,
}

/// Output payload from `build_compact`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactExecutionPayload {
    pub method: CompactMethod,
    pub command: Option<String>,
    pub prompt: String,
    pub instruction: String,
}

/// Builds the compact execution payload for the given provider and context state.
pub fn build_compact(req: &BuildCompactRequest) -> CompactExecutionPayload {
    let cap = get_provider_compact_capability(&req.provider_name);
    let instruction = req
        .custom_instruction
        .clone()
        .unwrap_or_else(|| STANDARD_COMPACT_INSTRUCTION.to_string());

    let mut context_summary = format!("【任务目标】{}\n", req.task_goal);
    if !req.contract_paths.is_empty() {
        context_summary.push_str(&format!("【契约路径】{}\n", req.contract_paths.join(", ")));
    }
    if let Some(err) = &req.pending_errors {
        if !err.is_empty() {
            context_summary.push_str(&format!("【未解决错误】{}\n", err.trim()));
        }
    }

    match cap.compact_method {
        CompactMethod::NativeSlashCommand => {
            let cmd = cap.compact_command.unwrap_or_else(|| "/compact".to_string());
            let full_cmd = format!("{} {}", cmd, instruction);
            CompactExecutionPayload {
                method: CompactMethod::NativeSlashCommand,
                command: Some(full_cmd),
                prompt: format!("{}\n{}", instruction, context_summary),
                instruction,
            }
        }
        CompactMethod::InstructionPrompt => {
            let prompt = format!(
                "请对当前对话上下文进行压缩总结，必须遵循指令：{}\n\n保留核心上下文：\n{}",
                instruction, context_summary
            );
            CompactExecutionPayload {
                method: CompactMethod::InstructionPrompt,
                command: None,
                prompt,
                instruction,
            }
        }
        CompactMethod::SynthesizedFreshBrief => {
            let prompt = format!(
                "# 上下文紧凑摘要 (Compact)\n\n## 保留信息\n{}\n\n## 指示\n{}",
                context_summary, instruction
            );
            CompactExecutionPayload {
                method: CompactMethod::SynthesizedFreshBrief,
                command: None,
                prompt,
                instruction,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claude_uses_native_compact_command() {
        let req = BuildCompactRequest {
            provider_name: "Claude".to_string(),
            task_goal: "Implement user authentication".to_string(),
            contract_paths: vec!["contracts/auth.md".to_string()],
            pending_errors: Some("token_expired error".to_string()),
            custom_instruction: None,
        };
        let payload = build_compact(&req);
        assert_eq!(payload.method, CompactMethod::NativeSlashCommand);
        assert!(payload.command.as_ref().unwrap().starts_with("/compact"));
        assert!(payload.instruction.contains("保留当前任务目标"));
    }

    #[test]
    fn test_cursor_and_copilot_fallback_to_instruction_prompt() {
        let req = BuildCompactRequest {
            provider_name: "Cursor".to_string(),
            task_goal: "Refactor state machine".to_string(),
            contract_paths: vec![],
            pending_errors: None,
            custom_instruction: None,
        };
        let payload = build_compact(&req);
        assert_eq!(payload.method, CompactMethod::InstructionPrompt);
        assert!(payload.prompt.contains("保留当前任务目标"));
    }
}
