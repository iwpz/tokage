//! DAG scheduler for Tokage orchestration engine (docs/03 §3, P7.m1b, P7.m1c).
//!
//! Scheduling features:
//! - Dependency tracking: Moves PENDING -> READY when all upstream tasks are DONE.
//! - Critical path calculation: Prioritizes READY tasks with the longest remaining dependency chain.
//! - Global concurrency slot limits (default: 4, docs/09 §3).
//! - Per-provider concurrency slot limits (from hints or default: 2).
//! - Write scope collision detection: Active running tasks with overlapping `write_scope` block candidates.
//! - Contract deviation downstream pause (P7.m1c): When an upstream task reports deviation from contract,
//!   its direct and indirect downstream tasks are PAUSED / BLOCKED pending review.

use std::collections::{HashMap, HashSet};
use serde::{Deserialize, Serialize};
use tokage_core::planning::{Plan, TaskItem};

use crate::state::TaskState;

/// Concurrency constraints for task execution (docs/09 §3).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConcurrencyLimits {
    /// Maximum global parallel agent child processes (default: 4).
    pub max_global_concurrent: usize,
    /// Default maximum parallel processes per provider (default: 2).
    pub default_provider_concurrent: usize,
    /// Per-provider specific concurrency overrides.
    pub provider_overrides: HashMap<String, usize>,
}

impl Default for ConcurrencyLimits {
    fn default() -> Self {
        Self {
            max_global_concurrent: 4,
            default_provider_concurrent: 2,
            provider_overrides: HashMap::new(),
        }
    }
}

impl ConcurrencyLimits {
    pub fn get_provider_limit(&self, provider: &str) -> usize {
        self.provider_overrides
            .get(provider)
            .copied()
            .unwrap_or(self.default_provider_concurrent)
    }
}

/// Information about a task currently being tracked in the scheduler.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTask {
    pub item: TaskItem,
    pub state: TaskState,
    pub assigned_provider: String,
    pub assigned_model: String,
    pub critical_path_length: usize,
    pub contract_deviation_paused: bool,
    pub deviation_source_task: Option<String>,
}

/// Reason why a READY task cannot be scheduled immediately.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DispatchIneligibleReason {
    GlobalSlotsFull { current: usize, max: usize },
    ProviderSlotsFull { provider: String, current: usize, max: usize },
    WriteScopeCollision { conflicting_task_id: String, overlapping_paths: Vec<String> },
    ContractDeviationPaused { source_task_id: String },
}

/// DAG Scheduler managing plan task lifecycles and dispatching.
pub struct DagScheduler {
    pub plan_id: String,
    pub tasks: HashMap<String, ScheduledTask>,
    pub limits: ConcurrencyLimits,
    /// Upstream dependency graph: task_id -> list of upstream task_ids it depends on.
    pub dependencies: HashMap<String, Vec<String>>,
    /// Downstream dependency graph: task_id -> list of downstream task_ids depending on it.
    pub dependents: HashMap<String, Vec<String>>,
}

impl DagScheduler {
    /// Initializes a new DAG scheduler from a Plan and assignments.
    pub fn new(plan: &Plan, provider_assignments: &HashMap<String, (String, String)>, limits: ConcurrencyLimits) -> Self {
        let mut tasks = HashMap::new();
        let mut dependencies = HashMap::new();
        let mut dependents = HashMap::new();

        for item in &plan.tasks {
            dependencies.insert(item.id.clone(), item.depends_on.clone());
            for dep in &item.depends_on {
                dependents.entry(dep.clone()).or_insert_with(Vec::new).push(item.id.clone());
            }
        }

        // Calculate critical path length for each task (longest path from task to any terminal downstream task)
        let mut critical_paths = HashMap::new();
        for item in &plan.tasks {
            let cp = calculate_downstream_depth(&item.id, &dependents, &mut HashMap::new());
            critical_paths.insert(item.id.clone(), cp);
        }

        for item in &plan.tasks {
            let (provider, model) = provider_assignments
                .get(&item.id)
                .cloned()
                .unwrap_or_else(|| ("mock".to_string(), "default".to_string()));

            let cp = critical_paths.get(&item.id).copied().unwrap_or(1);
            let initial_state = if item.depends_on.is_empty() {
                TaskState::Ready
            } else {
                TaskState::Pending
            };

            tasks.insert(
                item.id.clone(),
                ScheduledTask {
                    item: item.clone(),
                    state: initial_state,
                    assigned_provider: provider,
                    assigned_model: model,
                    critical_path_length: cp,
                    contract_deviation_paused: false,
                    deviation_source_task: None,
                },
            );
        }

        Self {
            plan_id: "plan-1".to_string(),
            tasks,
            limits,
            dependencies,
            dependents,
        }
    }

    /// Evaluates dependencies and marks PENDING tasks as READY when all upstreams are DONE.
    pub fn refresh_ready_tasks(&mut self) -> Vec<String> {
        let mut newly_ready = Vec::new();

        // First find tasks that qualify
        for (task_id, task) in self.tasks.iter() {
            if task.state == TaskState::Pending {
                let deps = &task.item.depends_on;
                let all_done = deps.iter().all(|dep_id| {
                    self.tasks.get(dep_id).map(|t| t.state == TaskState::Done).unwrap_or(false)
                });

                if all_done {
                    newly_ready.push(task_id.clone());
                }
            }
        }

        // Then update their states
        for id in &newly_ready {
            if let Some(t) = self.tasks.get_mut(id) {
                t.state = TaskState::Ready;
            }
        }

        newly_ready
    }

    /// Triggers contract deviation pause (P7.m1c).
    /// When an upstream task completes with a contract deviation, all downstream tasks
    /// that depend on it (directly or transitively) are marked paused.
    pub fn handle_contract_deviation(&mut self, upstream_task_id: &str) -> Vec<String> {
        let mut affected = Vec::new();
        let mut queue = vec![upstream_task_id.to_string()];
        let mut visited = HashSet::new();

        while let Some(current) = queue.pop() {
            if let Some(downstreams) = self.dependents.get(&current) {
                for d in downstreams {
                    if visited.insert(d.clone()) {
                        if let Some(task) = self.tasks.get_mut(d) {
                            task.contract_deviation_paused = true;
                            task.deviation_source_task = Some(upstream_task_id.to_string());
                            affected.push(d.clone());
                        }
                        queue.push(d.clone());
                    }
                }
            }
        }

        affected
    }

    /// Unpauses tasks after human review or contract adaptation has resolved deviation.
    pub fn unpause_contract_deviation(&mut self, upstream_source_id: &str) {
        for task in self.tasks.values_mut() {
            if task.deviation_source_task.as_deref() == Some(upstream_source_id) {
                task.contract_deviation_paused = false;
                task.deviation_source_task = None;
            }
        }
    }

    /// Returns list of runnable tasks sorted by critical path length descending, then task ID.
    /// Filters by global slots, provider concurrency, scope collisions, and contract pauses.
    pub fn pick_next_dispatchable_tasks(&self) -> Vec<String> {
        let active_tasks: Vec<&ScheduledTask> = self
            .tasks
            .values()
            .filter(|t| t.state == TaskState::Running)
            .collect();

        let global_running = active_tasks.len();
        if global_running >= self.limits.max_global_concurrent {
            return Vec::new();
        }

        let mut provider_running_counts: HashMap<String, usize> = HashMap::new();
        for t in &active_tasks {
            *provider_running_counts.entry(t.assigned_provider.clone()).or_insert(0) += 1;
        }

        // Active write scopes currently occupied by running tasks
        let mut occupied_write_scopes: Vec<(String, &[String])> = Vec::new();
        for t in &active_tasks {
            occupied_write_scopes.push((t.item.id.clone(), &t.item.write_scope));
        }

        // Candidates that are READY and not paused
        let mut candidates: Vec<&ScheduledTask> = self
            .tasks
            .values()
            .filter(|t| t.state == TaskState::Ready && !t.contract_deviation_paused)
            .collect();

        // Sort by critical path descending, then by id ascending (docs/03 §3)
        candidates.sort_by(|a, b| {
            b.critical_path_length
                .cmp(&a.critical_path_length)
                .then_with(|| a.item.id.cmp(&b.item.id))
        });

        let mut dispatchable = Vec::new();
        let mut simulated_global_running = global_running;
        let mut simulated_occupied_scopes = occupied_write_scopes;

        for cand in candidates {
            if simulated_global_running >= self.limits.max_global_concurrent {
                break;
            }

            let provider_limit = self.limits.get_provider_limit(&cand.assigned_provider);
            let current_p_running = provider_running_counts
                .get(&cand.assigned_provider)
                .copied()
                .unwrap_or(0);

            if current_p_running >= provider_limit {
                continue;
            }

            // Check write scope collisions against running & already chosen tasks
            let has_collision = simulated_occupied_scopes.iter().any(|(_id, scopes)| {
                scopes_overlap(&cand.item.write_scope, scopes)
            });

            if has_collision {
                continue;
            }

            // Task is eligible!
            dispatchable.push(cand.item.id.clone());
            simulated_global_running += 1;
            *provider_running_counts.entry(cand.assigned_provider.clone()).or_insert(0) += 1;
            simulated_occupied_scopes.push((cand.item.id.clone(), &cand.item.write_scope));
        }

        dispatchable
    }

    /// Updates task state and returns any newly READY tasks if this task reached DONE.
    pub fn update_task_state(&mut self, task_id: &str, new_state: TaskState) -> Result<Vec<String>, String> {
        let task = self.tasks.get_mut(task_id).ok_or_else(|| format!("Task {} not found", task_id))?;

        if !task.state.can_transition_to(new_state) {
            return Err(format!(
                "Invalid state transition from {:?} to {:?}",
                task.state, new_state
            ));
        }

        task.state = new_state;

        if new_state == TaskState::Done {
            Ok(self.refresh_ready_tasks())
        } else {
            Ok(Vec::new())
        }
    }
}

/// Recursively calculates longest downstream dependency depth.
fn calculate_downstream_depth(
    task_id: &str,
    dependents: &HashMap<String, Vec<String>>,
    memo: &mut HashMap<String, usize>,
) -> usize {
    if let Some(&depth) = memo.get(task_id) {
        return depth;
    }

    let mut max_child_depth = 0;
    if let Some(children) = dependents.get(task_id) {
        for child in children {
            let child_depth = calculate_downstream_depth(child, dependents, memo);
            if child_depth > max_child_depth {
                max_child_depth = child_depth;
            }
        }
    }

    let depth = 1 + max_child_depth;
    memo.insert(task_id.to_string(), depth);
    depth
}

/// Checks whether two write scopes overlap.
/// Scopes can be directories or files (e.g. `src/server/` or `src/server/routes.rs`).
pub fn scopes_overlap(scope_a: &[String], scope_b: &[String]) -> bool {
    for a in scope_a {
        for b in scope_b {
            let clean_a = a.trim_matches('/');
            let clean_b = b.trim_matches('/');

            if clean_a == clean_b {
                return true;
            }

            // Path prefix check: if one is subdirectory/parent of another
            if clean_a.starts_with(&format!("{}/", clean_b)) || clean_b.starts_with(&format!("{}/", clean_a)) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokage_core::planning::{TaskAcceptance, TaskSize};

    fn make_test_plan() -> Plan {
        let mut plan = Plan::default();
        plan.tasks = vec![
            TaskItem {
                id: "T1".to_string(),
                domain: "backend".to_string(),
                title: "Setup DB".to_string(),
                goal: "DB migration".to_string(),
                depends_on: vec![],
                contracts: vec!["C1".to_string()],
                write_scope: vec!["src/db/".to_string()],
                deliverables: vec![],
                acceptance: TaskAcceptance::default(),
                size: TaskSize::S,
                difficulty: 1,
                assigned_model: None,
            },
            TaskItem {
                id: "T2".to_string(),
                domain: "backend".to_string(),
                title: "User Service".to_string(),
                goal: "Implement user CRUD".to_string(),
                depends_on: vec!["T1".to_string()],
                contracts: vec!["C1".to_string()],
                write_scope: vec!["src/users/".to_string()],
                deliverables: vec![],
                acceptance: TaskAcceptance::default(),
                size: TaskSize::M,
                difficulty: 2,
                assigned_model: None,
            },
            TaskItem {
                id: "T3".to_string(),
                domain: "frontend".to_string(),
                title: "Frontend Shell".to_string(),
                goal: "Setup App router".to_string(),
                depends_on: vec![],
                contracts: vec![],
                write_scope: vec!["src/web/".to_string()],
                deliverables: vec![],
                acceptance: TaskAcceptance::default(),
                size: TaskSize::S,
                difficulty: 1,
                assigned_model: None,
            },
            TaskItem {
                id: "T4".to_string(),
                domain: "testing".to_string(),
                title: "End to end test".to_string(),
                goal: "E2E tests".to_string(),
                depends_on: vec!["T2".to_string(), "T3".to_string()],
                contracts: vec![],
                write_scope: vec!["tests/e2e/".to_string()],
                deliverables: vec![],
                acceptance: TaskAcceptance::default(),
                size: TaskSize::M,
                difficulty: 2,
                assigned_model: None,
            },
        ];
        plan
    }

    #[test]
    fn test_critical_path_and_initial_ready() {
        let plan = make_test_plan();
        let assignments = HashMap::from([
            ("T1".to_string(), ("claude".to_string(), "claude-3-7-sonnet".to_string())),
            ("T2".to_string(), ("claude".to_string(), "claude-3-7-sonnet".to_string())),
            ("T3".to_string(), ("copilot".to_string(), "gpt-4o".to_string())),
            ("T4".to_string(), ("codex".to_string(), "o3-mini".to_string())),
        ]);

        let scheduler = DagScheduler::new(&plan, &assignments, ConcurrencyLimits::default());

        // T1 -> T2 -> T4 (length: 3)
        // T3 -> T4 (length: 2)
        assert_eq!(scheduler.tasks["T1"].critical_path_length, 3);
        assert_eq!(scheduler.tasks["T2"].critical_path_length, 2);
        assert_eq!(scheduler.tasks["T3"].critical_path_length, 2);
        assert_eq!(scheduler.tasks["T4"].critical_path_length, 1);

        // Initially T1 and T3 are READY (no depends_on)
        assert_eq!(scheduler.tasks["T1"].state, TaskState::Ready);
        assert_eq!(scheduler.tasks["T3"].state, TaskState::Ready);
        assert_eq!(scheduler.tasks["T2"].state, TaskState::Pending);
        assert_eq!(scheduler.tasks["T4"].state, TaskState::Pending);

        // Critical path prioritization: T1 (cp=3) > T3 (cp=2)
        let next = scheduler.pick_next_dispatchable_tasks();
        assert_eq!(next, vec!["T1", "T3"]);
    }

    #[test]
    fn test_write_scope_conflict() {
        assert!(scopes_overlap(&["src/backend".to_string()], &["src/backend/api".to_string()]));
        assert!(scopes_overlap(&["src/web/index.ts".to_string()], &["src/web/index.ts".to_string()]));
        assert!(!scopes_overlap(&["src/backend/".to_string()], &["src/frontend/".to_string()]));
    }

    #[test]
    fn test_contract_deviation_pauses_downstream() {
        let plan = make_test_plan();
        let assignments = HashMap::new();
        let mut scheduler = DagScheduler::new(&plan, &assignments, ConcurrencyLimits::default());

        // When T1 reports contract deviation, T2 and T4 should be paused
        let paused = scheduler.handle_contract_deviation("T1");
        assert!(paused.contains(&"T2".to_string()));
        assert!(paused.contains(&"T4".to_string()));
        assert!(!paused.contains(&"T3".to_string()));

        assert!(scheduler.tasks["T2"].contract_deviation_paused);
        assert!(scheduler.tasks["T4"].contract_deviation_paused);
        assert!(!scheduler.tasks["T3"].contract_deviation_paused);

        // Unpause
        scheduler.unpause_contract_deviation("T1");
        assert!(!scheduler.tasks["T2"].contract_deviation_paused);
    }
}
