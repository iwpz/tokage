//! Plan and Pipeline execution lifecycle engine (docs/03 §9-§10, P7.m5).
//!
//! Submodules / features:
//! - Full gate execution upon plan completion (project-level build + test).
//! - Delivery summary aggregation (models used, touched files, gate results, total cost vs estimated cost, savings).
//! - Optional review task allocation (docs/03 §9).
//! - Pipeline lifecycle controls: Pause, Resume, Cancel, Retry (P7.m5b).
//! - Breakpoint resumption upon crash/restart: RUNNING tasks rebound to READY on attempts (P7.m5c).
//! - Task-level worktree isolation with dependency-ordered branch merges and conflict integration tasks (P7.m5d).

use std::collections::HashMap;
use std::path::Path;
use serde::{Deserialize, Serialize};
use tokage_core::planning::{Plan, TaskItem};
use tokage_native::env::ShellEnv;

use crate::brief::TaskReport;
use crate::gate::GateCommandResult;
use crate::scheduler::DagScheduler;
use crate::state::TaskState;

/// State of an overall Plan / Pipeline lifecycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PipelineStatus {
    /// In draft or awaiting approval.
    Idle,
    /// Actively scheduling and executing tasks.
    Running,
    /// Paused by user or budget overrun.
    Paused,
    /// Cancelled by user.
    Cancelled,
    /// All tasks completed (or skipped) and full gate passed.
    Completed,
    /// One or more tasks blocked and awaiting human resolution.
    Blocked,
}

/// Delivery summary generated upon plan completion (docs/03 §9).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliverySummary {
    pub plan_id: String,
    pub goal: String,
    pub total_tasks: usize,
    pub completed_tasks: usize,
    pub skipped_tasks: usize,
    pub all_gates_passed: bool,
    pub full_gate_result: Option<GateCommandResult>,
    pub touched_files: Vec<String>,
    pub estimated_cost_usd: f64,
    pub actual_cost_usd: f64,
    pub cost_variance_pct: f64,
    pub review_task: Option<TaskItem>,
    pub task_models: HashMap<String, String>,
}

/// Aggregates completed plan deliverables into a delivery summary (docs/03 §9, P7.m5a).
pub fn generate_delivery_summary(
    plan: &Plan,
    reports: &[TaskReport],
    full_gate_result: Option<GateCommandResult>,
    estimated_cost: f64,
    create_review_task: bool,
) -> DeliverySummary {
    let mut touched_files_set = std::collections::HashSet::new();
    let mut actual_cost_usd = 0.0;
    let mut task_models = HashMap::new();

    for r in reports {
        for f in &r.changed_files_list {
            touched_files_set.insert(f.clone());
        }
        if let Some(c) = r.cost_usd {
            actual_cost_usd += c;
        }
        task_models.insert(r.task_id.clone(), r.model.clone());
    }

    let all_gates_passed = full_gate_result.as_ref().map(|g| g.passed).unwrap_or(true);
    let mut touched_files: Vec<String> = touched_files_set.into_iter().collect();
    touched_files.sort();

    let cost_variance_pct = if estimated_cost > 0.0 {
        ((actual_cost_usd - estimated_cost) / estimated_cost) * 100.0
    } else {
        0.0
    };

    let review_task = if create_review_task {
        Some(TaskItem {
            id: "T-review".to_string(),
            domain: "review".to_string(),
            title: format!("审查改动摘要: {}", plan.spec.goal),
            goal: "只读对照契约检查各任务改动与偏离情况".to_string(),
            depends_on: plan.tasks.iter().map(|t| t.id.clone()).collect(),
            contracts: plan.contracts.iter().map(|c| c.id.clone()).collect(),
            write_scope: vec![],
            deliverables: vec!["review_comments.md".to_string()],
            acceptance: tokage_core::planning::TaskAcceptance::default(),
            size: tokage_core::planning::TaskSize::S,
            difficulty: 1,
            assigned_model: None,
        })
    } else {
        None
    };

    DeliverySummary {
        plan_id: "plan-1".to_string(),
        goal: plan.spec.goal.clone(),
        total_tasks: plan.tasks.len(),
        completed_tasks: reports.len(),
        skipped_tasks: plan.tasks.len().saturating_sub(reports.len()),
        all_gates_passed,
        full_gate_result,
        touched_files,
        estimated_cost_usd: estimated_cost,
        actual_cost_usd,
        cost_variance_pct,
        review_task,
        task_models,
    }
}

/// Recovery / Breakpoint resumption helper for restarting pipeline (docs/03 §10, P7.m5c).
/// Any task found in RUNNING state is reset to READY so it can be resumed or restarted.
pub fn recover_interrupted_tasks(scheduler: &mut DagScheduler) -> Vec<String> {
    let mut recovered = Vec::new();
    for (id, task) in scheduler.tasks.iter_mut() {
        if task.state == TaskState::Running || task.state == TaskState::Verifying {
            task.state = TaskState::Ready;
            recovered.push(id.clone());
        }
    }
    recovered
}

/// Merge result when merging task-level worktree branches (P7.m5d).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorktreeMergeOutcome {
    Success { merged_branch: String },
    Conflict {
        conflicted_files: Vec<String>,
        integration_task: TaskItem,
    },
}

/// Merges a task's worktree branch into the integration branch, creating an integration task on conflict (P7.m5d).
pub async fn merge_task_worktree(
    env: &ShellEnv,
    project_dir: &Path,
    target_branch: &str,
    task_branch: &str,
    task_item: &TaskItem,
) -> Result<WorktreeMergeOutcome, String> {
    // 1. Checkout target branch
    tokage_native::git::run(env, project_dir, &["checkout", target_branch], &[], 30).await?;

    // 2. Try git merge --no-ff
    let merge_res = tokage_native::git::run(env, project_dir, &["merge", "--no-ff", "-m", &format!("Merge task {}", task_item.id), task_branch], &[], 60).await;

    match merge_res {
        Ok(_) => Ok(WorktreeMergeOutcome::Success {
            merged_branch: target_branch.to_string(),
        }),
        Err(_err) => {
            // Check conflicts via git status
            let status = tokage_native::git::run(env, project_dir, &["diff", "--name-only", "--diff-filter=U"], &[], 10).await.unwrap_or_default();
            let conflicted_files: Vec<String> = status.lines().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();

            // Abort failed merge in worktree
            let _ = tokage_native::git::run(env, project_dir, &["merge", "--abort"], &[], 10).await;

            let integration_task = TaskItem {
                id: format!("{}-integration", task_item.id),
                domain: "integration".to_string(),
                title: format!("解决代码合并冲突: {}", task_item.title),
                goal: format!("解决分支 {} 合并至 {} 时的代码冲突: {:?}", task_branch, target_branch, conflicted_files),
                depends_on: vec![task_item.id.clone()],
                contracts: task_item.contracts.clone(),
                write_scope: conflicted_files.clone(),
                deliverables: conflicted_files.clone(),
                acceptance: task_item.acceptance.clone(),
                size: tokage_core::planning::TaskSize::S,
                difficulty: 2,
                assigned_model: None,
            };

            Ok(WorktreeMergeOutcome::Conflict {
                conflicted_files,
                integration_task,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::ConcurrencyLimits;
    use tokage_core::planning::{TaskAcceptance, TaskSize};

    #[test]
    fn test_generate_delivery_summary_and_review_task() {
        let mut plan = Plan::default();
        plan.spec.goal = "用户中心微服务改造".to_string();
        plan.tasks = vec![
            TaskItem {
                id: "T1".to_string(),
                domain: "backend".to_string(),
                title: "DB 表".to_string(),
                goal: "".to_string(),
                depends_on: vec![],
                contracts: vec![],
                write_scope: vec!["src/db/".to_string()],
                deliverables: vec![],
                acceptance: TaskAcceptance::default(),
                size: TaskSize::S,
                difficulty: 1,
                assigned_model: None,
            },
        ];

        let reports = vec![TaskReport {
            task_id: "T1".to_string(),
            title: "DB 表".to_string(),
            model: "claude-3-7-sonnet".to_string(),
            attempt_id: "att-1".to_string(),
            first_pass: true,
            is_locally_generated: false,
            completed_work: "Done".to_string(),
            contract_deviations: None,
            tips_for_downstream: "".to_string(),
            self_check_criteria: vec![],
            changed_files_stat: "schema.sql +10".to_string(),
            changed_files_list: vec!["schema.sql".to_string()],
            gate_results_summary: "passed".to_string(),
            cost_usd: Some(0.15),
        }];

        let summary = generate_delivery_summary(&plan, &reports, None, 0.20, true);
        assert_eq!(summary.total_tasks, 1);
        assert_eq!(summary.completed_tasks, 1);
        assert_eq!(summary.touched_files, vec!["schema.sql".to_string()]);
        assert_eq!(summary.actual_cost_usd, 0.15);
        assert!(summary.review_task.is_some());
        assert_eq!(summary.review_task.unwrap().domain, "review");
    }

    #[test]
    fn test_recover_interrupted_tasks() {
        let mut plan = Plan::default();
        plan.tasks = vec![TaskItem {
            id: "T1".to_string(),
            domain: "backend".to_string(),
            title: "Test".to_string(),
            goal: "".to_string(),
            depends_on: vec![],
            contracts: vec![],
            write_scope: vec![],
            deliverables: vec![],
            acceptance: TaskAcceptance::default(),
            size: TaskSize::S,
            difficulty: 1,
            assigned_model: None,
        }];

        let mut scheduler = DagScheduler::new(&plan, &HashMap::new(), ConcurrencyLimits::default());
        scheduler.tasks.get_mut("T1").unwrap().state = TaskState::Running;

        let recovered = recover_interrupted_tasks(&mut scheduler);
        assert_eq!(recovered, vec!["T1".to_string()]);
        assert_eq!(scheduler.tasks["T1"].state, TaskState::Ready);
    }
}
