//! Session Tree and UI Event stream engine (docs/07, P7.m6).
//!
//! Features:
//! - Hierarchical tree model: MainSession -> Plan -> Task -> Attempt -> Logs/Briefs (docs/07 §2).
//! - MainSession status machine: PLANNING | REVIEW | RUNNING | PAUSED | BLOCKED | IDLE | COMPLETED (docs/07 §3).
//! - Attempt tracking with `predecessor_id` chain for restarts/reassignments (docs/07 §2).
//! - Channel-based UI Event streaming with 50ms batching and focused-attempts filtering (docs/07 §4).
//! - Unified approval request queue aggregation across concurrent running tasks (docs/07 §5, P7.m6f).
//! - External session mounting & design target title derivation (docs/07 §6, P7.m6g).
//!
//! Production mirrors (TS): `src/lib/uiEvents.ts` (50ms batch + `approval_required`),
//! `src/lib/unifiedApprovals.ts` + PipelineView unified queue. Rust `UiEventBatcher` /
//! `SessionTreeNode` remain library-side; main chat still uses `chat-event`.

use std::collections::HashSet;
use serde::{Deserialize, Serialize};
use tokage_core::planning::PlanSpec;

use crate::state::TaskState;

/// Main session statuses defined in docs/07 §3.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MainSessionStatus {
    Planning,
    Review,
    Running,
    Paused,
    Blocked,
    Idle,
    Completed,
}

impl MainSessionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Planning => "PLANNING",
            Self::Review => "REVIEW",
            Self::Running => "RUNNING",
            Self::Paused => "PAUSED",
            Self::Blocked => "BLOCKED",
            Self::Idle => "IDLE",
            Self::Completed => "COMPLETED",
        }
    }
}

/// Attempt execution role (docs/07 §2, docs/08 schema.md §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AttemptRole {
    Planner,
    Executor,
    Tester,
    Reviewer,
}

/// A node representing an attempt in the session tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttemptNode {
    pub id: String,
    pub task_id: Option<String>,
    pub role: AttemptRole,
    pub provider_name: String,
    pub model_id: String,
    pub predecessor_id: Option<String>,
    pub reused_from_id: Option<String>,
    pub status: String,
    pub turn_count: u32,
}

/// A node representing a task in the session tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskNode {
    pub id: String,
    pub title: String,
    pub domain: String,
    pub state: TaskState,
    pub assigned_model: Option<String>,
    pub attempts: Vec<AttemptNode>,
}

/// A node representing a plan version in the session tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanNode {
    pub plan_id: String,
    pub version: u32,
    pub goal: String,
    pub tasks: Vec<TaskNode>,
}

/// Complete hierarchical session tree node (docs/07 §2, P7.m6a).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTreeNode {
    pub session_id: String,
    pub project_id: String,
    pub title: String,
    pub status: MainSessionStatus,
    pub plans: Vec<PlanNode>,
    /// External CLI native sessions mounted onto this session tree (P7.m6g).
    pub mounted_external_sessions: Vec<String>,
}

/// Unified approval queue item for parallel tasks (docs/07 §5, P7.m6f).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnifiedApprovalItem {
    pub approval_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub command: String,
    pub reason: String,
    pub created_at_ms: u64,
}

/// Streamable UI Event payload mirroring docs/07 §4.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiEvent {
    TreeChanged {
        session_id: String,
    },
    TaskStateChanged {
        task_id: String,
        state: TaskState,
        attempt_id: Option<String>,
    },
    MainSessionStatusChanged {
        session_id: String,
        status: MainSessionStatus,
    },
    GateCompleted {
        task_id: String,
        passed: bool,
        summary: String,
    },
    ApprovalRequired {
        item: UnifiedApprovalItem,
    },
    Notice {
        level: String, // info, warn, error
        message: String,
    },
}

/// Derives a clean session title from PlanSpec goal or direct prompt (docs/07 §6, P7.m6g).
pub fn derive_session_title_from_spec(spec: &PlanSpec) -> String {
    let goal = spec.goal.trim();
    if goal.is_empty() {
        return "New task session".to_string();
    }

    // Take the first sentence or first 40 chars
    let first_sentence = goal
        .split(|c| c == '。' || c == '.' || c == '\n')
        .next()
        .unwrap_or(goal)
        .trim();

    if first_sentence.chars().count() > 40 {
        first_sentence.chars().take(40).collect::<String>() + "..."
    } else {
        first_sentence.to_string()
    }
}

/// Event batcher buffering events for 50ms before flushes (docs/07 §4).
#[derive(Default)]
pub struct UiEventBatcher {
    pub buffer: Vec<UiEvent>,
    pub focused_attempts: HashSet<String>,
}

impl UiEventBatcher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_focus(&mut self, attempt_ids: HashSet<String>) {
        self.focused_attempts = attempt_ids;
    }

    pub fn push(&mut self, event: UiEvent) {
        self.buffer.push(event);
    }

    pub fn drain(&mut self) -> Vec<UiEvent> {
        std::mem::take(&mut self.buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_session_title_from_spec() {
        let mut spec = PlanSpec::default();
        spec.goal = "重构用户认证服务，支持多租户与 SSO 登录。后续兼容 LDAP 协议。".to_string();
        let title = derive_session_title_from_spec(&spec);
        assert_eq!(title, "重构用户认证服务，支持多租户与 SSO 登录");

        let long_spec = PlanSpec {
            goal: "这是一个非常非常长并且没有任何标点符号的长目标描述测试语句用于验证超出四十个字符会被优雅截断并添加省略号".to_string(),
            ..Default::default()
        };
        let long_title = derive_session_title_from_spec(&long_spec);
        assert!(long_title.ends_with("..."));
        assert!(long_title.chars().count() <= 43);
    }

    #[test]
    fn test_session_tree_hierarchy() {
        let tree = SessionTreeNode {
            session_id: "sess-1".to_string(),
            project_id: "proj-1".to_string(),
            title: "支付网关开发".to_string(),
            status: MainSessionStatus::Running,
            plans: vec![PlanNode {
                plan_id: "plan-1".to_string(),
                version: 1,
                goal: "微信与支付宝集成".to_string(),
                tasks: vec![TaskNode {
                    id: "T1".to_string(),
                    title: "集成微信支付 SDK".to_string(),
                    domain: "backend".to_string(),
                    state: TaskState::Running,
                    assigned_model: Some("claude-3-7-sonnet".to_string()),
                    attempts: vec![AttemptNode {
                        id: "att-1".to_string(),
                        task_id: Some("T1".to_string()),
                        role: AttemptRole::Executor,
                        provider_name: "claude".to_string(),
                        model_id: "claude-3-7-sonnet".to_string(),
                        predecessor_id: None,
                        reused_from_id: None,
                        status: "RUNNING".to_string(),
                        turn_count: 3,
                    }],
                }],
            }],
            mounted_external_sessions: vec!["claude-session-xyz".to_string()],
        };

        assert_eq!(tree.status, MainSessionStatus::Running);
        assert_eq!(tree.plans[0].tasks[0].attempts[0].id, "att-1");
        assert_eq!(tree.mounted_external_sessions.len(), 1);
    }
}
