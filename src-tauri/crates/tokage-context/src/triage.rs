//! Main session appended message triage engine (docs/05 §6, P8.m1e).
//!
//! Routes user messages typed in the main session or composer:
//! - Specific task detail modification -> Routed to that task's execution window (via §5 context planning).
//! - New requirement or cross-task change -> Incremental replanning in planning engine (docs/02 §9).
//! - Follow-up to previous direct dispatch -> Routed to previous direct dispatch window.
//! - Simple question -> Direct dispatch fresh window (docs/04 §8).

use serde::{Deserialize, Serialize};

/// Summary of an active task for triage matching.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSummaryForTriage {
    pub task_id: String,
    pub title: String,
    pub write_scope: Vec<String>,
}

/// Destination route for an appended user message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AppendMessageRoute {
    /// Routes to a specific task execution window.
    TaskExecutionWindow {
        task_id: String,
        task_title: String,
        reason: String,
    },
    /// Triggers incremental replanning.
    IncrementalReplan {
        prompt: String,
        reason: String,
    },
    /// Continues previous direct dispatch window.
    PreviousDirectWindow {
        attempt_id: i64,
        reason: String,
    },
    /// Fresh direct dispatch for a standalone question.
    DirectDispatch {
        fresh: bool,
        reason: String,
    },
}

/// Parameters for triaging an appended user message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriageMessageParams {
    pub message: String,
    pub active_tasks: Vec<TaskSummaryForTriage>,
    pub last_direct_attempt_id: Option<i64>,
    pub last_direct_timestamp_ms: Option<u64>,
    pub current_time_ms: u64,
    pub explicit_task_id: Option<String>,
}

/// Evaluates destination route for a user message typed in the main session.
pub fn triage_main_session_message(params: &TriageMessageParams) -> AppendMessageRoute {
    let msg = params.message.trim();

    // 1. Explicit user selection in composer takes highest priority
    if let Some(target_id) = &params.explicit_task_id {
        if let Some(task) = params.active_tasks.iter().find(|t| &t.task_id == target_id) {
            return AppendMessageRoute::TaskExecutionWindow {
                task_id: task.task_id.clone(),
                task_title: task.title.clone(),
                reason: format!("User explicitly targeted task {}", task.task_id),
            };
        }
    }

    let msg_lower = msg.to_lowercase();

    // 2. Check for incremental replan signals (new requirements, scope changes, multi-task refactoring)
    let replan_keywords = [
        "另外新增",
        "补充需求",
        "重新规划",
        "修改计划",
        "重新拆解",
        "调整架构",
        "增加一个模块",
        "还要再做",
        "除了这些还要",
        "add requirement",
        "replan",
        "redesign",
    ];
    for kw in &replan_keywords {
        if msg_lower.contains(kw) {
            return AppendMessageRoute::IncrementalReplan {
                prompt: msg.to_string(),
                reason: format!("Detected replanning keyword '{}'", kw),
            };
        }
    }

    // 3. Check for specific task references (task ID, title words, write scope files)
    for task in &params.active_tasks {
        let id_clean = task.task_id.to_lowercase();
        // Exact task ID match (e.g. "T1", "task-1", "t1:")
        if msg_lower.contains(&id_clean) {
            return AppendMessageRoute::TaskExecutionWindow {
                task_id: task.task_id.clone(),
                task_title: task.title.clone(),
                reason: format!("Mentioned task ID {}", task.task_id),
            };
        }

        // Match files in task write scope
        for scope in &task.write_scope {
            let scope_clean = scope.trim_start_matches("./").trim_end_matches('/');
            if !scope_clean.is_empty() && (msg.contains(scope_clean) || msg_lower.contains(&scope_clean.to_lowercase())) {
                return AppendMessageRoute::TaskExecutionWindow {
                    task_id: task.task_id.clone(),
                    task_title: task.title.clone(),
                    reason: format!("Referenced file/scope '{}' in task {}", scope, task.task_id),
                };
            }
        }

        // Match distinctive words in task title
        let title_words: Vec<&str> = task.title.split(|c: char| c.is_whitespace() || c == '-' || c == '_').collect();
        for word in title_words {
            if word.len() >= 4 && msg_lower.contains(&word.to_lowercase()) {
                return AppendMessageRoute::TaskExecutionWindow {
                    task_id: task.task_id.clone(),
                    task_title: task.title.clone(),
                    reason: format!("Matched title keyword '{}' to task {}", word, task.task_id),
                };
            }
        }
    }

    // 4. Check for follow-up to previous direct dispatch
    if let Some(direct_id) = params.last_direct_attempt_id {
        let follow_up_keywords = [
            "上个回答",
            "刚才那个",
            "再详细点",
            "为什么这样",
            "换种写法",
            "继续说",
            "elaborate",
            "what about",
            "why did you",
            "continue",
        ];
        let is_follow_up_phrase = follow_up_keywords.iter().any(|kw| msg_lower.contains(kw));

        // Time interval condition: within 5 minutes (300_000 ms)
        let is_recent = if let Some(last_ts) = params.last_direct_timestamp_ms {
            params.current_time_ms.saturating_sub(last_ts) < 300_000
        } else {
            false
        };

        if is_follow_up_phrase || (is_recent && msg.len() < 80) {
            return AppendMessageRoute::PreviousDirectWindow {
                attempt_id: direct_id,
                reason: "Contextually continuing previous direct dispatch conversation".to_string(),
            };
        }
    }

    // 5. Default: Fresh direct dispatch
    AppendMessageRoute::DirectDispatch {
        fresh: true,
        reason: "General standalone prompt or question".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_routes_to_task_by_id_and_file() {
        let tasks = vec![TaskSummaryForTriage {
            task_id: "T1".to_string(),
            title: "Setup SQLite migrations".to_string(),
            write_scope: vec!["crates/tokage-storage/migrations/".to_string()],
        }];

        let params1 = TriageMessageParams {
            message: "T1 里的字段类型有点问题，改成 INTEGER 吧".to_string(),
            active_tasks: tasks.clone(),
            last_direct_attempt_id: None,
            last_direct_timestamp_ms: None,
            current_time_ms: 1000,
            explicit_task_id: None,
        };
        let route1 = triage_main_session_message(&params1);
        match route1 {
            AppendMessageRoute::TaskExecutionWindow { task_id, .. } => assert_eq!(task_id, "T1"),
            _ => panic!("Expected TaskExecutionWindow"),
        }

        let params2 = TriageMessageParams {
            message: "请检查 crates/tokage-storage/migrations/ 的索引定义".to_string(),
            active_tasks: tasks,
            last_direct_attempt_id: None,
            last_direct_timestamp_ms: None,
            current_time_ms: 1000,
            explicit_task_id: None,
        };
        let route2 = triage_main_session_message(&params2);
        match route2 {
            AppendMessageRoute::TaskExecutionWindow { task_id, .. } => assert_eq!(task_id, "T1"),
            _ => panic!("Expected TaskExecutionWindow"),
        }
    }

    #[test]
    fn test_routes_to_incremental_replan() {
        let params = TriageMessageParams {
            message: "另外新增一个关于数据导出的需求，重新规划一下".to_string(),
            active_tasks: vec![],
            last_direct_attempt_id: None,
            last_direct_timestamp_ms: None,
            current_time_ms: 1000,
            explicit_task_id: None,
        };
        let route = triage_main_session_message(&params);
        match route {
            AppendMessageRoute::IncrementalReplan { .. } => (),
            _ => panic!("Expected IncrementalReplan"),
        }
    }

    #[test]
    fn test_routes_to_previous_direct_window() {
        let params = TriageMessageParams {
            message: "刚才那个回答能再详细点吗？".to_string(),
            active_tasks: vec![],
            last_direct_attempt_id: Some(42),
            last_direct_timestamp_ms: Some(1000),
            current_time_ms: 10_000,
            explicit_task_id: None,
        };
        let route = triage_main_session_message(&params);
        match route {
            AppendMessageRoute::PreviousDirectWindow { attempt_id, .. } => assert_eq!(attempt_id, 42),
            _ => panic!("Expected PreviousDirectWindow"),
        }
    }
}
