//! Incremental replanning engine: plan diffing, versioning, and application (docs/02 §9, P5.m6a).
//!
//! Handles:
//! 1. Plan diff computation (`compute_plan_diff`): identifies additions, modifications, and removals
//!    for Spec, Contracts, and Tasks between an old Plan and a new Plan / patch.
//! 2. Plan diff application (`apply_plan_diff`): rewrites spec and contracts completely to the latest
//!    version rather than appending changelog notes, increments plan version, records parent plan ID,
//!    and generates an updated consolidated `raw_markdown`.
//! 3. Structured persistence with history archiving: archives earlier versions under
//!    `.tokage/plans/<plan_id>/history/<v>.md` while keeping the current version at the root of the plan directory.

use std::collections::{HashMap, HashSet};
use tokage_core::planning::{Plan, PlanContract, PlanSpec, TaskItem, TaskSize};

/// The change action performed on a plan entity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiffAction {
    Added,
    Modified,
    Removed,
    Unchanged,
}

/// Diff details for a Contract.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractDiff {
    pub id: String,
    pub title: String,
    pub action: DiffAction,
    pub old_content: Option<String>,
    pub new_content: Option<String>,
}

/// Diff details for a Task.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDiff {
    pub id: String,
    pub title: String,
    pub action: DiffAction,
    pub old_task: Option<TaskItem>,
    pub new_task: Option<TaskItem>,
    pub contracts_changed: bool,
    pub dependencies_changed: bool,
    pub scope_changed: bool,
}

/// Diff details for the Spec.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpecDiff {
    pub goal_changed: bool,
    pub old_goal: String,
    pub new_goal: String,
    pub in_scope_added: Vec<String>,
    pub in_scope_removed: Vec<String>,
    pub out_of_scope_added: Vec<String>,
    pub out_of_scope_removed: Vec<String>,
    pub decisions_added: Vec<String>,
    pub decisions_removed: Vec<String>,
}

/// Execution status of a task used during replanning input assembly (docs/02 §9, P5.m6c).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReplanTaskStatus {
    Pending,
    Ready,
    Running,
    Verifying,
    Repairing,
    Done,
    Failed,
    Cancelled,
    Skipped,
}

impl Default for ReplanTaskStatus {
    fn default() -> Self {
        Self::Pending
    }
}

/// Execution report for a task (docs/02 §9, P5.m6c).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskExecutionReport {
    pub task_id: String,
    pub status: ReplanTaskStatus,
    /// What was completed by the executor (e.g. key output files, route endpoints, DB schemas).
    #[serde(default)]
    pub completed_summary: String,
    /// Detailed report / logs if available.
    #[serde(default)]
    pub full_report: Option<String>,
    /// Artifacts or files modified.
    #[serde(default)]
    pub modified_files: Vec<String>,
}

/// Arguments to assemble an incremental replanning prompt (docs/02 §9, P5.m6c).
#[derive(Clone, Debug)]
pub struct ReplanInputArgs<'a> {
    pub current_plan: &'a Plan,
    pub change_description: &'a str,
    pub task_reports: &'a [TaskExecutionReport],
    pub affected_contract_ids: Option<&'a [String]>,
    pub domains: Option<&'a [String]>,
    pub replan_input_max_tokens: Option<usize>,
}

impl SpecDiff {
    pub fn is_empty(&self) -> bool {
        !self.goal_changed
            && self.in_scope_added.is_empty()
            && self.in_scope_removed.is_empty()
            && self.out_of_scope_added.is_empty()
            && self.out_of_scope_removed.is_empty()
            && self.decisions_added.is_empty()
            && self.decisions_removed.is_empty()
    }
}

/// Complete diff representation between an existing plan and an updated plan (docs/02 §9).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanDiff {
    pub from_version: u32,
    pub to_version: u32,
    pub spec_diff: SpecDiff,
    pub contracts: Vec<ContractDiff>,
    pub tasks: Vec<TaskDiff>,
    pub notes_changed: bool,
}

impl PlanDiff {
    /// Returns true if there are no semantic differences between the two plans.
    pub fn is_empty(&self) -> bool {
        self.spec_diff.is_empty()
            && self.contracts.iter().all(|c| c.action == DiffAction::Unchanged)
            && self.tasks.iter().all(|t| t.action == DiffAction::Unchanged)
            && !self.notes_changed
    }

    /// List of contract IDs that were modified or removed.
    pub fn affected_contract_ids(&self) -> Vec<String> {
        self.contracts
            .iter()
            .filter(|c| c.action == DiffAction::Modified || c.action == DiffAction::Removed)
            .map(|c| c.id.clone())
            .collect()
    }

    /// List of task IDs that were added, modified or removed.
    pub fn affected_task_ids(&self) -> Vec<String> {
        self.tasks
            .iter()
            .filter(|t| t.action != DiffAction::Unchanged)
            .map(|t| t.id.clone())
            .collect()
    }
}

/// Computes the semantic diff from `current_plan` to `proposed_plan` (P5.m6a).
pub fn compute_plan_diff(current_plan: &Plan, proposed_plan: &Plan) -> PlanDiff {
    let from_version = current_plan.version;
    let to_version = if proposed_plan.version > current_plan.version {
        proposed_plan.version
    } else {
        current_plan.version + 1
    };

    // 1. Spec Diff
    let spec_diff = compute_spec_diff(&current_plan.spec, &proposed_plan.spec);

    // 2. Contracts Diff
    let mut contracts_diff = Vec::new();
    let current_contracts_map: HashMap<&str, &PlanContract> =
        current_plan.contracts.iter().map(|c| (c.id.as_str(), c)).collect();
    let proposed_contracts_map: HashMap<&str, &PlanContract> =
        proposed_plan.contracts.iter().map(|c| (c.id.as_str(), c)).collect();

    // Check existing
    for (id, curr_c) in &current_contracts_map {
        if let Some(prop_c) = proposed_contracts_map.get(id) {
            if curr_c.title != prop_c.title || curr_c.content != prop_c.content {
                contracts_diff.push(ContractDiff {
                    id: (*id).to_string(),
                    title: prop_c.title.clone(),
                    action: DiffAction::Modified,
                    old_content: Some(curr_c.content.clone()),
                    new_content: Some(prop_c.content.clone()),
                });
            } else {
                contracts_diff.push(ContractDiff {
                    id: (*id).to_string(),
                    title: curr_c.title.clone(),
                    action: DiffAction::Unchanged,
                    old_content: Some(curr_c.content.clone()),
                    new_content: Some(prop_c.content.clone()),
                });
            }
        } else {
            contracts_diff.push(ContractDiff {
                id: (*id).to_string(),
                title: curr_c.title.clone(),
                action: DiffAction::Removed,
                old_content: Some(curr_c.content.clone()),
                new_content: None,
            });
        }
    }

    // Check new added contracts
    for (id, prop_c) in &proposed_contracts_map {
        if !current_contracts_map.contains_key(id) {
            contracts_diff.push(ContractDiff {
                id: (*id).to_string(),
                title: prop_c.title.clone(),
                action: DiffAction::Added,
                old_content: None,
                new_content: Some(prop_c.content.clone()),
            });
        }
    }

    // Sort contracts diff by ID
    contracts_diff.sort_by(|a, b| a.id.cmp(&b.id));

    // 3. Tasks Diff
    let mut tasks_diff = Vec::new();
    let current_tasks_map: HashMap<&str, &TaskItem> =
        current_plan.tasks.iter().map(|t| (t.id.as_str(), t)).collect();
    let proposed_tasks_map: HashMap<&str, &TaskItem> =
        proposed_plan.tasks.iter().map(|t| (t.id.as_str(), t)).collect();

    for (id, curr_t) in &current_tasks_map {
        if let Some(prop_t) = proposed_tasks_map.get(id) {
            let contracts_changed = curr_t.contracts != prop_t.contracts;
            let dependencies_changed = curr_t.depends_on != prop_t.depends_on;
            let scope_changed = curr_t.write_scope != prop_t.write_scope;
            let other_changed = curr_t.title != prop_t.title
                || curr_t.goal != prop_t.goal
                || curr_t.domain != prop_t.domain
                || curr_t.deliverables != prop_t.deliverables
                || curr_t.acceptance != prop_t.acceptance
                || curr_t.size != prop_t.size
                || curr_t.difficulty != prop_t.difficulty;

            let action = if contracts_changed || dependencies_changed || scope_changed || other_changed {
                DiffAction::Modified
            } else {
                DiffAction::Unchanged
            };

            tasks_diff.push(TaskDiff {
                id: (*id).to_string(),
                title: prop_t.title.clone(),
                action,
                old_task: Some((*curr_t).clone()),
                new_task: Some((*prop_t).clone()),
                contracts_changed,
                dependencies_changed,
                scope_changed,
            });
        } else {
            tasks_diff.push(TaskDiff {
                id: (*id).to_string(),
                title: curr_t.title.clone(),
                action: DiffAction::Removed,
                old_task: Some((*curr_t).clone()),
                new_task: None,
                contracts_changed: false,
                dependencies_changed: false,
                scope_changed: false,
            });
        }
    }

    for (id, prop_t) in &proposed_tasks_map {
        if !current_tasks_map.contains_key(id) {
            tasks_diff.push(TaskDiff {
                id: (*id).to_string(),
                title: prop_t.title.clone(),
                action: DiffAction::Added,
                old_task: None,
                new_task: Some((*prop_t).clone()),
                contracts_changed: false,
                dependencies_changed: false,
                scope_changed: false,
            });
        }
    }

    tasks_diff.sort_by(|a, b| a.id.cmp(&b.id));

    let notes_changed = current_plan.notes != proposed_plan.notes;

    PlanDiff {
        from_version,
        to_version,
        spec_diff,
        contracts: contracts_diff,
        tasks: tasks_diff,
        notes_changed,
    }
}

/// Applies a `PlanDiff` onto an existing `Plan`, returning a new Plan with:
/// - Version incremented to `diff.to_version`
/// - Parent plan ID set to the previous plan ID or version
/// - Spec and contracts completely rewritten to the latest version (docs/02 §9:
///   "应用差异后，设计稿与契约整体改写为最新版本，不以追加修订说明的方式变长")
/// - Tasks updated, removed, or appended
/// - Rendered markdown regenerated as single source of truth
pub fn apply_plan_diff(
    current_plan: &Plan,
    diff: &PlanDiff,
    proposed_plan: &Plan,
    parent_plan_id: Option<String>,
) -> Plan {
    let next_version = diff.to_version.max(current_plan.version + 1);

    // Spec: rewritten to the latest version
    let new_spec = proposed_plan.spec.clone();

    // Contracts: rewritten to the latest version
    let new_contracts = proposed_plan.contracts.clone();

    // Tasks: merge proposed tasks
    let mut updated_tasks = Vec::new();
    let removed_ids: HashSet<&str> = diff
        .tasks
        .iter()
        .filter(|t| t.action == DiffAction::Removed)
        .map(|t| t.id.as_str())
        .collect();

    let mut proposed_tasks_map: HashMap<&str, TaskItem> = proposed_plan
        .tasks
        .iter()
        .map(|t| (t.id.as_str(), t.clone()))
        .collect();

    // Keep order of current plan tasks if still present, with updates applied
    for curr in &current_plan.tasks {
        if removed_ids.contains(curr.id.as_str()) {
            continue;
        }
        if let Some(prop) = proposed_tasks_map.remove(curr.id.as_str()) {
            updated_tasks.push(prop);
        } else {
            updated_tasks.push(curr.clone());
        }
    }

    // Append newly added tasks
    for prop in &proposed_plan.tasks {
        if proposed_tasks_map.contains_key(prop.id.as_str()) {
            updated_tasks.push(prop.clone());
        }
    }

    let new_notes = proposed_plan.notes.clone();

    let mut new_plan = Plan {
        version: next_version,
        parent_plan_id: parent_plan_id.or_else(|| current_plan.parent_plan_id.clone()),
        spec: new_spec,
        contracts: new_contracts,
        tasks: updated_tasks,
        notes: new_notes,
        raw_markdown: String::new(),
        ..Default::default()
    };

    // Render consolidated canonical markdown
    new_plan.raw_markdown = crate::storage::render_full_plan_markdown(&new_plan);
    new_plan
}

fn compute_spec_diff(old: &PlanSpec, new: &PlanSpec) -> SpecDiff {
    let goal_changed = old.goal.trim() != new.goal.trim();

    let old_in: HashSet<&str> = old.in_scope.iter().map(|s| s.trim()).collect();
    let new_in: HashSet<&str> = new.in_scope.iter().map(|s| s.trim()).collect();
    let in_scope_added: Vec<String> = new.in_scope.iter().filter(|s| !old_in.contains(s.trim())).cloned().collect();
    let in_scope_removed: Vec<String> = old.in_scope.iter().filter(|s| !new_in.contains(s.trim())).cloned().collect();

    let old_out: HashSet<&str> = old.out_of_scope.iter().map(|s| s.trim()).collect();
    let new_out: HashSet<&str> = new.out_of_scope.iter().map(|s| s.trim()).collect();
    let out_of_scope_added: Vec<String> = new.out_of_scope.iter().filter(|s| !old_out.contains(s.trim())).cloned().collect();
    let out_of_scope_removed: Vec<String> = old.out_of_scope.iter().filter(|s| !new_out.contains(s.trim())).cloned().collect();

    let old_dec: HashSet<&str> = old.decisions.iter().map(|s| s.trim()).collect();
    let new_dec: HashSet<&str> = new.decisions.iter().map(|s| s.trim()).collect();
    let decisions_added: Vec<String> = new.decisions.iter().filter(|s| !old_dec.contains(s.trim())).cloned().collect();
    let decisions_removed: Vec<String> = old.decisions.iter().filter(|s| !new_dec.contains(s.trim())).cloned().collect();

    SpecDiff {
        goal_changed,
        old_goal: old.goal.clone(),
        new_goal: new.goal.clone(),
        in_scope_added,
        in_scope_removed,
        out_of_scope_added,
        out_of_scope_removed,
        decisions_added,
        decisions_removed,
    }
}

/// Default maximum input tokens for incremental replanning (docs/02 §10).
pub const DEFAULT_REPLAN_INPUT_MAX_TOKENS: usize = 16000;

/// Generates adaptation tasks for completed tasks affected by contract modifications (docs/02 §9, P5.m6b).
///
/// Rule: "已完成且不受影响的任务保持不变；受契约变更影响的已完成任务生成「适配任务」"。
pub fn generate_adaptation_tasks(
    current_plan: &Plan,
    changed_contract_ids: &[String],
    task_reports: &[TaskExecutionReport],
) -> Vec<TaskItem> {
    if changed_contract_ids.is_empty() {
        return Vec::new();
    }

    let changed_set: HashSet<&str> = changed_contract_ids.iter().map(|s| s.as_str()).collect();
    let completed_task_ids: HashSet<&str> = task_reports
        .iter()
        .filter(|r| r.status == ReplanTaskStatus::Done)
        .map(|r| r.task_id.as_str())
        .collect();

    let mut adaptation_tasks = Vec::new();
    let mut seq = current_plan.tasks.len() + 1;

    for task in &current_plan.tasks {
        let is_completed = completed_task_ids.contains(task.id.as_str());
        let touches_changed_contract = task.contracts.iter().any(|c| changed_set.contains(c.as_str()));

        if is_completed && touches_changed_contract {
            let affected_c: Vec<String> = task
                .contracts
                .iter()
                .filter(|c| changed_set.contains(c.as_str()))
                .cloned()
                .collect();

            let adapt_id = format!("T{}-adapt", seq);
            seq += 1;

            let adapt_task = TaskItem {
                id: adapt_id,
                domain: task.domain.clone(),
                title: format!("适配契约更新: {}", task.title),
                goal: format!(
                    "原任务 {} 已完成，但依赖的契约 [{}] 发生变更，需做适配与兼容调整",
                    task.id,
                    affected_c.join(", ")
                ),
                depends_on: vec![task.id.clone()],
                contracts: affected_c,
                write_scope: task.write_scope.clone(),
                deliverables: vec![format!("适配契约更新后的 {}", task.title)],
                acceptance: task.acceptance.clone(),
                size: TaskSize::S,
                difficulty: 1,
                assigned_model: task.assigned_model.clone(),
            };
            adaptation_tasks.push(adapt_task);
        }
    }

    adaptation_tasks
}

/// Assembles the prompt input for incremental replanning with budget trimming (docs/02 §9, P5.m6c).
///
/// Budget algorithm:
/// 1. Assemble fixed instructions + Spec & Contracts (often cached).
/// 2. Summarize unaffected tasks with ID, title and status only.
/// 3. Filter reports of affected tasks. If token budget (`replan_input_max_tokens`, default 16K)
///    is exceeded, prune reports to `completed_summary` first; if still exceeding, discard by relevance.
pub fn assemble_replan_prompt(args: ReplanInputArgs<'_>) -> String {
    let max_tokens = args.replan_input_max_tokens.unwrap_or(DEFAULT_REPLAN_INPUT_MAX_TOKENS);

    let domains_str = if let Some(d) = args.domains {
        d.join(", ")
    } else {
        crate::prompt::DEFAULT_DOMAINS.join(", ")
    };

    let mut out = String::new();

    // 1. System instruction
    out.push_str("你是技术规划者。本次调用为【增量重规划】(Incremental Replan)。\n");
    out.push_str("请根据需求变更说明与当前计划，仅输出差异（新增、修改、取消的任务与契约）。\n");
    out.push_str("要求：\n");
    out.push_str("1. 已完成且不受影响的任务保持不变。\n");
    out.push_str("2. 若契约变更影响已完成的任务，需针对性安排适配与调整任务。\n");
    out.push_str("3. 重新输出完整且自洽的最新版设计稿 (Spec)、契约 (Contracts) 与任务 (Tasks)。\n");
    out.push_str("4. domain 必须从可用领域精确选取；数据库/迁移→data-model，CI/部署→infra，鉴权安全→backend-api；禁止 database/devops/security 旧标签。\n");
    out.push_str(&format!("可用领域：{}\n\n", domains_str));

    // 2. Current Spec & Contracts
    out.push_str("---\n# 当前计划 (Current Plan)\n\n");
    out.push_str(&crate::storage::render_spec_markdown(args.current_plan));
    if !args.current_plan.contracts.is_empty() {
        out.push_str("# Contracts\n\n");
        for c in &args.current_plan.contracts {
            out.push_str(&format!("## {} {}\n", c.id, c.title));
            out.push_str(&c.content);
            out.push_str("\n\n");
        }
    }

    // 3. Task Status Overview
    out.push_str("# 当前任务状态总览\n");
    let reports_map: HashMap<&str, &TaskExecutionReport> = args
        .task_reports
        .iter()
        .map(|r| (r.task_id.as_str(), r))
        .collect();

    for t in &args.current_plan.tasks {
        let status_str = match reports_map.get(t.id.as_str()).map(|r| r.status) {
            Some(ReplanTaskStatus::Done) => "DONE",
            Some(ReplanTaskStatus::Running) => "RUNNING",
            Some(ReplanTaskStatus::Failed) => "FAILED",
            Some(ReplanTaskStatus::Repairing) => "REPAIRING",
            _ => "PENDING",
        };
        out.push_str(&format!("- [{}] {} ({}): 领域={}\n", status_str, t.id, t.title, t.domain));
    }
    out.push('\n');

    // 4. Affected Task Execution Reports with Budget Management
    out.push_str("---\n# 受影响已完成任务的执行报告\n");
    let change_lower = args.change_description.to_lowercase();

    // Determine affected or completed tasks (affected contracts or change description mention)
    let affected_c_set: HashSet<String> = args
        .affected_contract_ids
        .map(|c_list| c_list.iter().map(|s| s.to_uppercase()).collect())
        .unwrap_or_default();

    // Map task id to its contracts in current plan
    let task_contracts_map: HashMap<&str, &[String]> = args
        .current_plan
        .tasks
        .iter()
        .map(|t| (t.id.as_str(), t.contracts.as_slice()))
        .collect();

    let mut relevant_reports: Vec<&TaskExecutionReport> = args
        .task_reports
        .iter()
        .filter(|r| r.status == ReplanTaskStatus::Done || r.status == ReplanTaskStatus::Failed)
        .collect();

    // Sort by relevance to change description or affected contracts (higher relevance first)
    relevant_reports.sort_by(|a, b| {
        let a_contracts = task_contracts_map.get(a.task_id.as_str()).copied().unwrap_or(&[]);
        let b_contracts = task_contracts_map.get(b.task_id.as_str()).copied().unwrap_or(&[]);
        let a_touches_c = a_contracts.iter().any(|c| affected_c_set.contains(&c.to_uppercase()));
        let b_touches_c = b_contracts.iter().any(|c| affected_c_set.contains(&c.to_uppercase()));

        let a_mentioned = change_lower.contains(&a.task_id.to_lowercase());
        let b_mentioned = change_lower.contains(&b.task_id.to_lowercase());

        let a_score = (if a_mentioned { 4 } else { 0 }) + (if a_touches_c { 2 } else { 1 });
        let b_score = (if b_mentioned { 4 } else { 0 }) + (if b_touches_c { 2 } else { 1 });

        b_score.cmp(&a_score)
    });

    let current_tokens = crate::digest::budget::estimate_tokens(&out);
    let mut available_budget = if max_tokens > current_tokens {
        max_tokens - current_tokens
    } else {
        500
    };

    let mut reports_text = String::new();
    let mut use_compact_summary = false;

    // Check if full reports exceed budget
    let full_tokens: usize = relevant_reports
        .iter()
        .map(|r| {
            let content = r.full_report.as_deref().unwrap_or(&r.completed_summary);
            crate::digest::budget::estimate_tokens(content)
        })
        .sum();

    if full_tokens > available_budget {
        use_compact_summary = true;
    }

    for report in relevant_reports {
        let report_body = if use_compact_summary || report.full_report.is_none() {
            format!("完成内容: {}", report.completed_summary)
        } else {
            report.full_report.as_deref().unwrap_or(&report.completed_summary).to_string()
        };

        let block = format!("### 任务 {} 报告 ({:?})\n{}\n\n", report.task_id, report.status, report_body.trim());
        let block_tokens = crate::digest::budget::estimate_tokens(&block);

        if block_tokens <= available_budget {
            reports_text.push_str(&block);
            available_budget -= block_tokens;
        } else if use_compact_summary {
            // Trim summary to fit remaining budget
            let max_chars = available_budget.saturating_mul(4);
            if max_chars > 30 {
                let trimmed: String = report.completed_summary.chars().take(max_chars).collect();
                reports_text.push_str(&format!("### 任务 {} 报告\n完成内容: {}...\n\n", report.task_id, trimmed));
            }
            break;
        }
    }

    if reports_text.trim().is_empty() {
        out.push_str("（无受影响任务报告）\n\n");
    } else {
        out.push_str(&reports_text);
    }

    // 5. Change Description
    out.push_str("---\n# 需求变更说明\n");
    out.push_str(args.change_description.trim());
    out.push('\n');

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokage_core::planning::{PlanContract, PlanSpec, TaskAcceptance, TaskItem, TaskSize};

    fn make_test_plan() -> Plan {
        Plan {
            version: 1,
            parent_plan_id: None,
            spec: PlanSpec {
                raw: "".into(),
                goal: "初始目标".into(),
                in_scope: vec!["用户管理".into()],
                out_of_scope: vec!["支付".into()],
                decisions: vec!["使用 PostgreSQL".into()],
            },
            contracts: vec![
                PlanContract {
                    id: "C1".into(),
                    title: "用户 API".into(),
                    content: "type User = { id: string };".into(),
                },
            ],
            tasks: vec![
                TaskItem {
                    id: "T1".into(),
                    domain: "backend-api".into(),
                    title: "用户 API 实现".into(),
                    goal: "实现 C1".into(),
                    depends_on: vec![],
                    contracts: vec!["C1".into()],
                    write_scope: vec!["src/users/**".into()],
                    deliverables: vec!["routes".into()],
                    acceptance: TaskAcceptance {
                        commands: vec!["cargo test".into()],
                        criteria: vec!["状态码 200".into()],
                ..Default::default()
            },
                    size: TaskSize::M,
                    difficulty: 2,
                    assigned_model: None,
                },
            ],
            notes: Some("初始注意项".into()),
            raw_markdown: "".into(),
            ..Default::default()
        }
    }

    #[test]
    fn test_compute_diff_and_apply_rewrites_cleanly() {
        let plan_v1 = make_test_plan();

        let mut proposed_plan = plan_v1.clone();
        proposed_plan.spec.goal = "升级目标：用户与权限模块".into();
        proposed_plan.spec.in_scope.push("权限 RBAC".into());
        proposed_plan.contracts[0].content = "type User = { id: string; role: string };".into();

        // Add C2 contract
        proposed_plan.contracts.push(PlanContract {
            id: "C2".into(),
            title: "权限定义".into(),
            content: "type Role = 'admin' | 'user';".into(),
        });

        // Update T1 and add T2
        proposed_plan.tasks[0].contracts = vec!["C1".into(), "C2".into()];
        proposed_plan.tasks.push(TaskItem {
            id: "T2".into(),
            domain: "frontend-ui".into(),
            title: "权限界面".into(),
            goal: "RBAC 界面".into(),
            depends_on: vec!["T1".into()],
            contracts: vec!["C2".into()],
            write_scope: vec!["web/src/**".into()],
            deliverables: vec!["pages".into()],
            acceptance: TaskAcceptance::default(),
            size: TaskSize::S,
            difficulty: 1,
            assigned_model: None,
        });

        let diff = compute_plan_diff(&plan_v1, &proposed_plan);

        assert_eq!(diff.from_version, 1);
        assert_eq!(diff.to_version, 2);
        assert!(diff.spec_diff.goal_changed);
        assert_eq!(diff.spec_diff.in_scope_added, vec!["权限 RBAC"]);

        assert_eq!(diff.contracts.len(), 2);
        let c1_diff = diff.contracts.iter().find(|c| c.id == "C1").unwrap();
        assert_eq!(c1_diff.action, DiffAction::Modified);
        let c2_diff = diff.contracts.iter().find(|c| c.id == "C2").unwrap();
        assert_eq!(c2_diff.action, DiffAction::Added);

        assert_eq!(diff.tasks.len(), 2);
        let t1_diff = diff.tasks.iter().find(|t| t.id == "T1").unwrap();
        assert_eq!(t1_diff.action, DiffAction::Modified);
        assert!(t1_diff.contracts_changed);
        let t2_diff = diff.tasks.iter().find(|t| t.id == "T2").unwrap();
        assert_eq!(t2_diff.action, DiffAction::Added);

        // Apply diff
        let plan_v2 = apply_plan_diff(&plan_v1, &diff, &proposed_plan, Some("plan-001".into()));

        assert_eq!(plan_v2.version, 2);
        assert_eq!(plan_v2.parent_plan_id.as_deref(), Some("plan-001"));
        assert_eq!(plan_v2.spec.goal, "升级目标：用户与权限模块");
        assert_eq!(plan_v2.contracts.len(), 2);
        assert_eq!(plan_v2.contracts[0].content, "type User = { id: string; role: string };");
        assert_eq!(plan_v2.tasks.len(), 2);
        assert_eq!(plan_v2.tasks[0].contracts, vec!["C1", "C2"]);
        assert!(plan_v2.raw_markdown.contains("# Spec\n\n## 目标\n升级目标：用户与权限模块"));
        assert!(plan_v2.raw_markdown.contains("## C2 权限定义"));
    }

    #[test]
    fn test_generate_adaptation_tasks_for_affected_completed_tasks() {
        let plan = make_test_plan();
        let changed_contracts = vec!["C1".to_string()];

        let reports = vec![
            TaskExecutionReport {
                task_id: "T1".into(),
                status: ReplanTaskStatus::Done,
                completed_summary: "已实现用户基础 API".into(),
                full_report: Some("用户基础 CRUD 已全部就绪并跑通".into()),
                modified_files: vec!["src/users/mod.rs".into()],
            },
        ];

        let adaptation = generate_adaptation_tasks(&plan, &changed_contracts, &reports);
        assert_eq!(adaptation.len(), 1);
        assert_eq!(adaptation[0].id, "T2-adapt");
        assert_eq!(adaptation[0].domain, "backend-api");
        assert!(adaptation[0].title.contains("适配契约更新"));
        assert_eq!(adaptation[0].depends_on, vec!["T1"]);
        assert_eq!(adaptation[0].contracts, vec!["C1"]);
    }

    #[test]
    fn test_assemble_replan_prompt_trims_within_budget() {
        let plan = make_test_plan();

        let long_report = "很长很长的详细测试报告日志... ".repeat(500); // approx 4000+ chars
        let reports = vec![
            TaskExecutionReport {
                task_id: "T1".into(),
                status: ReplanTaskStatus::Done,
                completed_summary: "基础用户接口已完成".into(),
                full_report: Some(long_report),
                modified_files: vec!["src/users/api.rs".into()],
            },
        ];

        // 1. Ample budget
        let ample_prompt = assemble_replan_prompt(ReplanInputArgs {
            current_plan: &plan,
            change_description: "由于安全审计，需要调整用户接口添加角色字段",
            task_reports: &reports,
            affected_contract_ids: Some(&["C1".into()]),
            domains: None,
            replan_input_max_tokens: Some(16000),
        });
        assert!(ample_prompt.contains("增量重规划"));
        assert!(ample_prompt.contains("很长很长的详细测试报告日志"));
        assert!(ample_prompt.contains("由于安全审计"));

        // 2. Strict tight budget (forces compact summary pruning)
        let tight_prompt = assemble_replan_prompt(ReplanInputArgs {
            current_plan: &plan,
            change_description: "调整用户接口",
            task_reports: &reports,
            affected_contract_ids: Some(&["C1".into()]),
            domains: None,
            replan_input_max_tokens: Some(400),
        });
        assert!(tight_prompt.contains("基础用户接口已完成"));
        assert!(!tight_prompt.contains("repeat"));
        let token_est = crate::digest::budget::estimate_tokens(&tight_prompt);
        assert!(token_est <= 450);
    }
}
