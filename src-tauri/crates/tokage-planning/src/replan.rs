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
use tokage_core::planning::{Plan, PlanContract, PlanSpec, TaskItem};

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
                    },
                    size: TaskSize::M,
                    difficulty: 2,
                    assigned_model: None,
                },
            ],
            notes: Some("初始注意项".into()),
            raw_markdown: "".into(),
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
}
