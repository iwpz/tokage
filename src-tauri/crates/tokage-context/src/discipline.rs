//! Context and cache discipline enforcement (docs/05 §8, R9, R10, P8.m1f).
//!
//! Enforces:
//! - R9: Same window must not change model, permissions (role), or startup parameters.
//! - R10: Preserve Prompt Cache integrity:
//!   - Fixed execution headers in task briefs are byte-for-byte stable.
//!   - User messages are transmitted verbatim without synthetic prefixes.
//!   - Prohibits fake keepalive requests.

use serde::{Deserialize, Serialize};
use tokage_orchestration::brief::TASK_BRIEF_FIXED_HEADER;

/// Violations of R9 or R10 discipline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DisciplineViolation {
    /// R9: Model changed inside the same window.
    ModelChanged { from: String, to: String },
    /// R9: Role / permission changed inside the same window.
    RoleChanged { from: String, to: String },
    /// R9: Startup parameters changed inside the same window.
    StartupParamsChanged { detail: String },
    /// R10: Fixed task brief header was modified or corrupted.
    CorruptedFixedHeader { diff_summary: String },
    /// R10: Volatile prefix prepended to user message.
    VolatileMessagePrefix { prefix: String },
    /// R10: Keepalive dummy request detected.
    FakeKeepaliveDetected,
}

/// Window configuration state for verifying R9 continuity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowConfig {
    pub model: String,
    pub role: String,
    pub startup_args: Vec<String>,
}

/// Verifies R9: same window does not alter model, role, or startup configuration.
pub fn validate_window_continuity(
    initial: &WindowConfig,
    current: &WindowConfig,
) -> Result<(), DisciplineViolation> {
    if initial.model != current.model {
        return Err(DisciplineViolation::ModelChanged {
            from: initial.model.clone(),
            to: current.model.clone(),
        });
    }

    if initial.role != current.role {
        return Err(DisciplineViolation::RoleChanged {
            from: initial.role.clone(),
            to: current.role.clone(),
        });
    }

    if initial.startup_args != current.startup_args {
        return Err(DisciplineViolation::StartupParamsChanged {
            detail: format!("{:?} -> {:?}", initial.startup_args, current.startup_args),
        });
    }

    Ok(())
}

/// Verifies R10: task brief fixed header is byte-for-byte stable (no timestamps or dynamic prefixes).
pub fn validate_task_brief_header(brief_text: &str) -> Result<(), DisciplineViolation> {
    if !brief_text.starts_with(TASK_BRIEF_FIXED_HEADER) {
        let first_lines: String = brief_text.lines().take(5).collect::<Vec<_>>().join("\n");
        return Err(DisciplineViolation::CorruptedFixedHeader {
            diff_summary: format!("Header mismatch. Found:\n{}", first_lines),
        });
    }
    Ok(())
}

/// Verifies R10: user message is unmodified and free from synthetic status prefixes.
pub fn validate_user_message_integrity(
    raw_user_input: &str,
    transmitted_message: &str,
) -> Result<(), DisciplineViolation> {
    if transmitted_message == raw_user_input {
        return Ok(());
    }

    // Check if a synthetic prefix was added
    if transmitted_message.ends_with(raw_user_input) {
        let prefix = &transmitted_message[..transmitted_message.len() - raw_user_input.len()];
        return Err(DisciplineViolation::VolatileMessagePrefix {
            prefix: prefix.trim().to_string(),
        });
    }

    Err(DisciplineViolation::VolatileMessagePrefix {
        prefix: "Message altered during transmission".to_string(),
    })
}

/// Verifies R10: no dummy keepalive calls are permitted.
pub fn validate_no_fake_keepalives(is_keepalive: bool) -> Result<(), DisciplineViolation> {
    if is_keepalive {
        return Err(DisciplineViolation::FakeKeepaliveDetected);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_r9_model_or_role_change_violates_continuity() {
        let init = WindowConfig {
            model: "claude-3-5-sonnet".to_string(),
            role: "EXECUTOR".to_string(),
            startup_args: vec!["--headless".to_string()],
        };

        let changed_model = WindowConfig {
            model: "gpt-4o".to_string(),
            ..init.clone()
        };
        assert!(matches!(
            validate_window_continuity(&init, &changed_model),
            Err(DisciplineViolation::ModelChanged { .. })
        ));

        let changed_role = WindowConfig {
            role: "PLANNER".to_string(),
            ..init.clone()
        };
        assert!(matches!(
            validate_window_continuity(&init, &changed_role),
            Err(DisciplineViolation::RoleChanged { .. })
        ));
    }

    #[test]
    fn test_r10_brief_header_and_message_integrity() {
        let valid_brief = format!("{}## 任务\n- 编号：T1", TASK_BRIEF_FIXED_HEADER);
        assert!(validate_task_brief_header(&valid_brief).is_ok());

        let invalid_brief = format!("# Tokage Task Brief [2026-10-06 01:00]\n\n{}", TASK_BRIEF_FIXED_HEADER);
        assert!(matches!(
            validate_task_brief_header(&invalid_brief),
            Err(DisciplineViolation::CorruptedFixedHeader { .. })
        ));

        let raw = "请编写一个单元测试";
        assert!(validate_user_message_integrity(raw, raw).is_ok());

        let prefixed = format!("[STATUS: RUNNING] {}", raw);
        assert!(matches!(
            validate_user_message_integrity(raw, &prefixed),
            Err(DisciplineViolation::VolatileMessagePrefix { .. })
        ));

        assert!(matches!(
            validate_no_fake_keepalives(true),
            Err(DisciplineViolation::FakeKeepaliveDetected)
        ));
    }
}
