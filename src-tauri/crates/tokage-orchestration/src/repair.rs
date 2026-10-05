//! Repair and Fallback Engine for failed tasks (docs/03 §8, P7.m4).
//!
//! Three-step ladder (docs/03 §8):
//! 1. Step 1: In-window repair (`FixedInWindow`). Reuses same execution window/cache with failure evidence.
//! 2. Step 2: Reassign (`Reassigned`). Discards previous model, picks highest-p qualified model in domain.
//! 3. Step 3: Blocked (`Blocked`). Provides:
//!    - Decomposition by planning engine.
//!    - Terminal takeover (generate resume command e.g. `claude --resume <id>`).
//!    - User skip.
//!
//! When a testing task discovers defects in implementation, it generates a repair task for the implementation task.

use serde::{Deserialize, Serialize};
use tokage_core::planning::TaskItem;

/// The three fallback steps when acceptance gate fails (docs/03 §8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RepairStep {
    /// Step 0: Initial run (not in repair yet).
    Initial = 0,
    /// Step 1: Repair in the same execution window (warm cache).
    InWindowRepair = 1,
    /// Step 2: Reassign to alternative model with highest p_success in domain.
    Reassigned = 2,
    /// Step 3: Fallback exhausted, task becomes BLOCKED.
    Blocked = 3,
}

impl RepairStep {
    pub fn next(&self) -> Self {
        match self {
            Self::Initial => Self::InWindowRepair,
            Self::InWindowRepair => Self::Reassigned,
            Self::Reassigned => Self::Blocked,
            Self::Blocked => Self::Blocked,
        }
    }
}

/// Action to take on a BLOCKED task (P7.m4c).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BlockedResolution {
    /// Hand off to planning engine to re-decompose this task into smaller subtasks.
    Decompose,
    /// User takes over via external terminal; generates resume command.
    TerminalTakeover { resume_command: String },
    /// User decides to skip this task and continue pipeline.
    Skip,
}

/// Information needed to resolve or advance a repair cycle.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailureEvidence {
    pub failed_command: Option<String>,
    pub output_tail: String,
    pub out_of_scope_files: Vec<String>,
    pub previous_changed_files: Vec<String>,
}

impl FailureEvidence {
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();
        if let Some(cmd) = &self.failed_command {
            md.push_str(&format!("### 失败命令\n`{}`\n\n", cmd));
        }
        if !self.out_of_scope_files.is_empty() {
            md.push_str("### 越界改动文件（超出写入范围）\n");
            for f in &self.out_of_scope_files {
                md.push_str(&format!("- `{}`\n", f));
            }
            md.push('\n');
        }
        if !self.output_tail.is_empty() {
            md.push_str("### 命令输出证据（尾部 200 行）\n```text\n");
            md.push_str(&self.output_tail);
            md.push_str("\n```\n");
        }
        md
    }
}

/// Decision made by the repair fallback controller.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RepairDecision {
    /// Advance to Step 1: Retry in same execution window with evidence.
    TryInWindowRepair {
        attempt_id: String,
        evidence: String,
    },
    /// Advance to Step 2: Reassign to a different model.
    TryReassign {
        exclude_models: Vec<String>,
        evidence: String,
    },
    /// Advance to Step 3: Block task and offer human resolution options.
    MarkBlocked {
        terminal_resume_command: String,
        reason: String,
    },
}

/// Computes next fallback action according to the 3-step ladder.
pub fn decide_repair_action(
    _task: &TaskItem,
    current_step: RepairStep,
    current_attempt_id: &str,
    current_model: &str,
    provider: &str,
    evidence: &FailureEvidence,
) -> (RepairStep, RepairDecision) {
    let next_step = current_step.next();
    let evidence_md = evidence.to_markdown();

    match next_step {
        RepairStep::InWindowRepair => {
            (
                next_step,
                RepairDecision::TryInWindowRepair {
                    attempt_id: current_attempt_id.to_string(),
                    evidence: evidence_md,
                },
            )
        }
        RepairStep::Reassigned => {
            (
                next_step,
                RepairDecision::TryReassign {
                    exclude_models: vec![current_model.to_string()],
                    evidence: evidence_md,
                },
            )
        }
        RepairStep::Blocked => {
            let resume_cmd = generate_terminal_resume_command(provider, current_attempt_id);
            (
                next_step,
                RepairDecision::MarkBlocked {
                    terminal_resume_command: resume_cmd,
                    reason: "修复步骤已耗尽（同窗口与改派均失败），需要人工干预".to_string(),
                },
            )
        }
        RepairStep::Initial => unreachable!(),
    }
}

/// Generates external CLI terminal resume command for user takeover (P7.m4c).
pub fn generate_terminal_resume_command(provider: &str, session_or_attempt_id: &str) -> String {
    let p_clean = provider.to_lowercase();
    if p_clean.contains("claude") {
        format!("claude --resume {}", session_or_attempt_id)
    } else if p_clean.contains("codex") {
        format!("codex resume {}", session_or_attempt_id)
    } else if p_clean.contains("cursor") {
        format!("cursor-agent resume {}", session_or_attempt_id)
    } else if p_clean.contains("copilot") {
        format!("copilot-cli resume {}", session_or_attempt_id)
    } else {
        format!("tokage-cli resume {}", session_or_attempt_id)
    }
}

/// Generates an implementation adaptation / fix task when a testing task finds defects (P7.m4b).
pub fn generate_implementation_fix_task(
    test_task: &TaskItem,
    implementation_task: &TaskItem,
    defect_description: &str,
    _test_failure_evidence: &str,
) -> TaskItem {
    let mut fix_task = implementation_task.clone();
    fix_task.id = format!("{}-fix", implementation_task.id);
    fix_task.title = format!("修复缺陷: {}", implementation_task.title);
    fix_task.goal = format!(
        "根据测试任务 {} 发现的缺陷进行修复: {}",
        test_task.id, defect_description
    );
    fix_task.depends_on = vec![test_task.id.clone()];
    // Write scope remains constrained to the original implementation scope!
    fix_task.write_scope = implementation_task.write_scope.clone();
    fix_task.assigned_model = None;
    fix_task
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokage_core::planning::{TaskAcceptance, TaskSize};

    #[test]
    fn test_three_step_repair_ladder() {
        let task = TaskItem {
            id: "T1".to_string(),
            domain: "backend".to_string(),
            title: "API".to_string(),
            goal: "".to_string(),
            depends_on: vec![],
            contracts: vec![],
            write_scope: vec!["src/".to_string()],
            deliverables: vec![],
            acceptance: TaskAcceptance::default(),
            size: TaskSize::S,
            difficulty: 1,
            assigned_model: None,
        };

        let evidence = FailureEvidence {
            failed_command: Some("cargo test".to_string()),
            output_tail: "assertion failed".to_string(),
            out_of_scope_files: vec![],
            previous_changed_files: vec!["src/lib.rs".to_string()],
        };

        // 1. Initial -> Step 1 (InWindowRepair)
        let (step1, dec1) = decide_repair_action(
            &task,
            RepairStep::Initial,
            "att-1",
            "claude-3-7-sonnet",
            "claude",
            &evidence,
        );
        assert_eq!(step1, RepairStep::InWindowRepair);
        assert!(matches!(dec1, RepairDecision::TryInWindowRepair { .. }));

        // 2. Step 1 -> Step 2 (Reassigned)
        let (step2, dec2) = decide_repair_action(
            &task,
            RepairStep::InWindowRepair,
            "att-1",
            "claude-3-7-sonnet",
            "claude",
            &evidence,
        );
        assert_eq!(step2, RepairStep::Reassigned);
        if let RepairDecision::TryReassign { exclude_models, .. } = dec2 {
            assert_eq!(exclude_models, vec!["claude-3-7-sonnet".to_string()]);
        } else {
            panic!("Expected TryReassign");
        }

        // 3. Step 2 -> Step 3 (Blocked)
        let (step3, dec3) = decide_repair_action(
            &task,
            RepairStep::Reassigned,
            "att-2",
            "o3-mini",
            "codex",
            &evidence,
        );
        assert_eq!(step3, RepairStep::Blocked);
        if let RepairDecision::MarkBlocked { terminal_resume_command, .. } = dec3 {
            assert_eq!(terminal_resume_command, "codex resume att-2");
        } else {
            panic!("Expected MarkBlocked");
        }
    }

    #[test]
    fn test_testing_defect_generates_implementation_fix_task() {
        let impl_task = TaskItem {
            id: "T1".to_string(),
            domain: "backend".to_string(),
            title: "User Auth".to_string(),
            goal: "Login logic".to_string(),
            depends_on: vec![],
            contracts: vec!["C1".to_string()],
            write_scope: vec!["src/auth/".to_string()],
            deliverables: vec![],
            acceptance: TaskAcceptance::default(),
            size: TaskSize::M,
            difficulty: 2,
            assigned_model: None,
        };

        let test_task = TaskItem {
            id: "T3".to_string(),
            domain: "testing".to_string(),
            title: "Integration tests".to_string(),
            goal: "Test suite".to_string(),
            depends_on: vec!["T1".to_string()],
            contracts: vec![],
            write_scope: vec!["tests/".to_string()],
            deliverables: vec![],
            acceptance: TaskAcceptance::default(),
            size: TaskSize::S,
            difficulty: 1,
            assigned_model: None,
        };

        let fix = generate_implementation_fix_task(
            &test_task,
            &impl_task,
            "密码哈希算法未采用 Argon2",
            "Test failed: expected argon2 hash",
        );

        assert_eq!(fix.id, "T1-fix");
        assert_eq!(fix.depends_on, vec!["T3"]);
        // Crucial: write scope is retained to src/auth/ and NOT tests/ (P7.m4b)
        assert_eq!(fix.write_scope, vec!["src/auth/"]);
        assert!(fix.goal.contains("密码哈希算法未采用 Argon2"));
    }
}
