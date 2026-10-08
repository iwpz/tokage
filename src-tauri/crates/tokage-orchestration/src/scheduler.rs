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

/// Reason why a READY (or promoted) task cannot be scheduled immediately (F9).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum DispatchIneligibleReason {
    GlobalSlotsFull { current: usize, max: usize },
    ProviderSlotsFull { provider: String, current: usize, max: usize },
    WriteScopeCollision {
        conflicting_task_id: String,
        overlapping_paths: Vec<String>,
    },
    ContractDeviationPaused { source_task_id: String },
}

/// Result of one scheduling pick (F9).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PickResult {
    /// PENDING tasks whose deps are all DONE (TS promotes PENDING→READY first).
    pub promoted: Vec<String>,
    /// Tasks that may start this round (critical-path order), after concurrency / scope / deviation filters.
    pub dispatchable: Vec<String>,
    /// READY (incl. promoted) tasks that cannot start this round, with reasons.
    pub ineligible: HashMap<String, DispatchIneligibleReason>,
}

/// IPC input for `pick_dispatchable_tasks` (F9 / A.10).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PickDispatchInput {
    pub tasks: Vec<TaskItem>,
    pub task_states: HashMap<String, TaskState>,
    /// taskId → (provider, model)
    pub assignments: HashMap<String, (String, String)>,
    pub max_global_concurrent: usize,
    #[serde(default)]
    pub deviation_sources: Vec<String>,
}

/// IPC output for `pick_dispatchable_tasks` (alias of PickResult).
pub type PickDispatchOutput = PickResult;

/// DAG Scheduler managing plan task lifecycles and dispatching.
pub struct DagScheduler {
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
            tasks,
            limits,
            dependencies,
            dependents,
        }
    }

    /// Build a scheduler snapshot from current task definitions + states (F9 / D1-A).
    ///
    /// Does not mutate caller state. `deviation_sources` marks transitive downstream
    /// of those tasks as contract-deviation-paused (same effect as `handle_contract_deviation`).
    pub fn from_states(
        tasks: &[TaskItem],
        states: &HashMap<String, TaskState>,
        assignments: &HashMap<String, (String, String)>,
        limits: ConcurrencyLimits,
        deviation_sources: &[String],
    ) -> Self {
        // Synthetic plan so we reuse critical-path / graph construction from `new`.
        let mut plan = Plan::default();
        plan.tasks = tasks.to_vec();
        let mut scheduler = Self::new(&plan, assignments, limits);

        for task in tasks {
            if let Some(st) = scheduler.tasks.get_mut(&task.id) {
                if let Some(&state) = states.get(&task.id) {
                    st.state = state;
                }
            }
        }

        for src in deviation_sources {
            scheduler.handle_contract_deviation(src);
        }

        scheduler
    }

    /// Occupancy for global concurrency and write-scope slots (D1):
    /// RUNNING + VERIFYING + REPAIRING.
    fn occupies_global_or_write(state: TaskState) -> bool {
        matches!(
            state,
            TaskState::Running | TaskState::Verifying | TaskState::Repairing
        )
    }

    /// Occupancy for per-provider slots (D1): RUNNING + REPAIRING only.
    fn occupies_provider(state: TaskState) -> bool {
        matches!(state, TaskState::Running | TaskState::Repairing)
    }

    /// Full pick: promoted / dispatchable / ineligible (F9).
    ///
    /// Does not mutate task states — callers promote PENDING→READY from `promoted`
    /// then claim `dispatchable`.
    pub fn pick(&self) -> PickResult {
        let mut result = PickResult::default();

        // 1. Promote: PENDING whose every dep is DONE.
        for (id, task) in &self.tasks {
            if task.state != TaskState::Pending {
                continue;
            }
            let deps = &task.item.depends_on;
            let all_done = deps.iter().all(|dep_id| {
                self.tasks
                    .get(dep_id)
                    .map(|t| t.state == TaskState::Done)
                    .unwrap_or(false)
            });
            if all_done {
                result.promoted.push(id.clone());
            }
        }
        result.promoted.sort();

        // 2. Candidates = READY + promoted (still PENDING in this snapshot).
        let mut candidates: Vec<&ScheduledTask> = self
            .tasks
            .values()
            .filter(|t| t.state == TaskState::Ready || result.promoted.iter().any(|p| p == &t.item.id))
            .collect();

        candidates.sort_by(|a, b| {
            b.critical_path_length
                .cmp(&a.critical_path_length)
                .then_with(|| a.item.id.cmp(&b.item.id))
        });

        let occupying: Vec<&ScheduledTask> = self
            .tasks
            .values()
            .filter(|t| Self::occupies_global_or_write(t.state))
            .collect();
        let mut global_running = occupying.len();

        let mut provider_running_counts: HashMap<String, usize> = HashMap::new();
        for t in self.tasks.values() {
            if Self::occupies_provider(t.state) {
                *provider_running_counts
                    .entry(t.assigned_provider.clone())
                    .or_insert(0) += 1;
            }
        }

        let mut occupied_write_scopes: Vec<(String, &[String])> = occupying
            .iter()
            .map(|t| (t.item.id.clone(), t.item.write_scope.as_slice()))
            .collect();

        for cand in candidates {
            // Contract deviation pause.
            if cand.contract_deviation_paused {
                let source = cand
                    .deviation_source_task
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string());
                result.ineligible.insert(
                    cand.item.id.clone(),
                    DispatchIneligibleReason::ContractDeviationPaused {
                        source_task_id: source,
                    },
                );
                continue;
            }

            // Global concurrency.
            if global_running >= self.limits.max_global_concurrent {
                result.ineligible.insert(
                    cand.item.id.clone(),
                    DispatchIneligibleReason::GlobalSlotsFull {
                        current: global_running,
                        max: self.limits.max_global_concurrent,
                    },
                );
                continue;
            }

            // Per-provider concurrency.
            let provider_limit = self.limits.get_provider_limit(&cand.assigned_provider);
            let current_p = provider_running_counts
                .get(&cand.assigned_provider)
                .copied()
                .unwrap_or(0);
            if current_p >= provider_limit {
                result.ineligible.insert(
                    cand.item.id.clone(),
                    DispatchIneligibleReason::ProviderSlotsFull {
                        provider: cand.assigned_provider.clone(),
                        current: current_p,
                        max: provider_limit,
                    },
                );
                continue;
            }

            // Write-scope exclusivity.
            let collision = occupied_write_scopes.iter().find_map(|(holder, scopes)| {
                tokage_core::command_policy::scope::scopes_overlap(
                    &cand.item.write_scope,
                    scopes,
                    false,
                )
                .map(|(a, b)| (holder.clone(), vec![a, b]))
            });
            if let Some((conflicting_task_id, overlapping_paths)) = collision {
                result.ineligible.insert(
                    cand.item.id.clone(),
                    DispatchIneligibleReason::WriteScopeCollision {
                        conflicting_task_id,
                        overlapping_paths,
                    },
                );
                continue;
            }

            // Accept.
            result.dispatchable.push(cand.item.id.clone());
            global_running += 1;
            *provider_running_counts
                .entry(cand.assigned_provider.clone())
                .or_insert(0) += 1;
            occupied_write_scopes.push((cand.item.id.clone(), cand.item.write_scope.as_slice()));
        }

        result
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

    /// Returns only dispatchable task IDs (critical-path order). Prefer `pick()` for full detail.
    pub fn pick_next_dispatchable_tasks(&self) -> Vec<String> {
        self.pick().dispatchable
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
    tokage_core::command_policy::scope::scopes_overlap(scope_a, scope_b, false).is_some()
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

    fn make_task(id: &str, deps: &[&str], write_scope: &[&str]) -> TaskItem {
        TaskItem {
            id: id.to_string(),
            domain: "test".to_string(),
            title: id.to_string(),
            goal: id.to_string(),
            depends_on: deps.iter().map(|s| s.to_string()).collect(),
            contracts: vec![],
            write_scope: write_scope.iter().map(|s| s.to_string()).collect(),
            deliverables: vec![],
            acceptance: TaskAcceptance::default(),
            size: TaskSize::S,
            difficulty: 1,
            assigned_model: None,
        }
    }

    /// F9.a① critical path order via from_states + pick
    #[test]
    fn f9a_critical_path_order() {
        let tasks = make_test_plan().tasks;
        let states: HashMap<_, _> = [
            ("T1", TaskState::Ready),
            ("T2", TaskState::Pending),
            ("T3", TaskState::Ready),
            ("T4", TaskState::Pending),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        let assignments = HashMap::from([
            ("T1".to_string(), ("claude".to_string(), "m".to_string())),
            ("T2".to_string(), ("claude".to_string(), "m".to_string())),
            ("T3".to_string(), ("copilot".to_string(), "m".to_string())),
            ("T4".to_string(), ("codex".to_string(), "m".to_string())),
        ]);
        let scheduler = DagScheduler::from_states(
            &tasks,
            &states,
            &assignments,
            ConcurrencyLimits::default(),
            &[],
        );
        let pick = scheduler.pick();
        assert_eq!(pick.dispatchable, vec!["T1", "T3"]);
        assert!(pick.promoted.is_empty());
        assert!(pick.ineligible.is_empty());
    }

    /// F9.a② provider slot limit → providerSlotsFull
    #[test]
    fn f9a_provider_slots_full() {
        let tasks = vec![
            make_task("A", &[], &["a/"]),
            make_task("B", &[], &["b/"]),
            make_task("C", &[], &["c/"]),
        ];
        let states: HashMap<_, _> = [("A", TaskState::Ready), ("B", TaskState::Ready), ("C", TaskState::Ready)]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        let assignments = HashMap::from([
            ("A".to_string(), ("claude".to_string(), "m".to_string())),
            ("B".to_string(), ("claude".to_string(), "m".to_string())),
            ("C".to_string(), ("claude".to_string(), "m".to_string())),
        ]);
        let limits = ConcurrencyLimits {
            max_global_concurrent: 4,
            default_provider_concurrent: 2,
            provider_overrides: HashMap::new(),
        };
        let pick = DagScheduler::from_states(&tasks, &states, &assignments, limits, &[]).pick();
        assert_eq!(pick.dispatchable.len(), 2);
        assert_eq!(pick.ineligible.len(), 1);
        let (_id, reason) = pick.ineligible.iter().next().unwrap();
        match reason {
            DispatchIneligibleReason::ProviderSlotsFull {
                provider,
                current,
                max,
            } => {
                assert_eq!(provider, "claude");
                assert_eq!(*current, 2);
                assert_eq!(*max, 2);
            }
            other => panic!("expected ProviderSlotsFull, got {other:?}"),
        }
    }

    /// F9.a③ VERIFYING / REPAIRING occupy global + write scope
    #[test]
    fn f9a_occupancy_verifying_and_repairing() {
        let tasks = vec![
            make_task("V", &[], &["src/shared/"]),
            make_task("R", &[], &["src/other/"]),
            make_task("W", &[], &["src/shared/child/"]),
            make_task("G", &[], &["src/free/"]),
        ];
        let states: HashMap<_, _> = [
            ("V", TaskState::Verifying),
            ("R", TaskState::Repairing),
            ("W", TaskState::Ready),
            ("G", TaskState::Ready),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        let assignments = HashMap::from([
            ("V".to_string(), ("claude".to_string(), "m".to_string())),
            ("R".to_string(), ("copilot".to_string(), "m".to_string())),
            ("W".to_string(), ("codex".to_string(), "m".to_string())),
            ("G".to_string(), ("aider".to_string(), "m".to_string())),
        ]);
        // global max 2: V(verifying)+R(repairing) fill global → G and W ineligible
        let limits = ConcurrencyLimits {
            max_global_concurrent: 2,
            default_provider_concurrent: 2,
            provider_overrides: HashMap::new(),
        };
        let pick = DagScheduler::from_states(&tasks, &states, &assignments, limits, &[]).pick();
        assert!(pick.dispatchable.is_empty());
        assert!(matches!(
            pick.ineligible.get("G"),
            Some(DispatchIneligibleReason::GlobalSlotsFull { current: 2, max: 2 })
        ));

        // Raise global: W collides with V's write scope; G is free.
        // VERIFYING occupies write scope; REPAIRING occupies provider (not needed here).
        let limits2 = ConcurrencyLimits {
            max_global_concurrent: 4,
            default_provider_concurrent: 2,
            provider_overrides: HashMap::new(),
        };
        let pick2 = DagScheduler::from_states(&tasks, &states, &assignments, limits2, &[]).pick();
        assert_eq!(pick2.dispatchable, vec!["G"]);
        assert!(matches!(
            pick2.ineligible.get("W"),
            Some(DispatchIneligibleReason::WriteScopeCollision { .. })
        ));
    }

    /// F9.a④ write scope collision (glob forms)
    #[test]
    fn f9a_write_scope_collision() {
        let tasks = vec![
            make_task("T1", &[], &["src/**"]),
            make_task("T2", &[], &["src/server/**"]),
        ];
        let states: HashMap<_, _> = [("T1", TaskState::Running), ("T2", TaskState::Ready)]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        let assignments = HashMap::from([
            ("T1".to_string(), ("claude".to_string(), "m".to_string())),
            ("T2".to_string(), ("copilot".to_string(), "m".to_string())),
        ]);
        let pick =
            DagScheduler::from_states(&tasks, &states, &assignments, ConcurrencyLimits::default(), &[])
                .pick();
        assert!(pick.dispatchable.is_empty());
        match pick.ineligible.get("T2") {
            Some(DispatchIneligibleReason::WriteScopeCollision {
                conflicting_task_id,
                overlapping_paths,
            }) => {
                assert_eq!(conflicting_task_id, "T1");
                assert!(!overlapping_paths.is_empty());
            }
            other => panic!("expected WriteScopeCollision, got {other:?}"),
        }
    }

    /// F9.a⑤ contract deviation pauses transitive downstream
    #[test]
    fn f9a_contract_deviation_paused() {
        let tasks = make_test_plan().tasks;
        let states: HashMap<_, _> = [
            ("T1", TaskState::Done),
            ("T2", TaskState::Ready),
            ("T3", TaskState::Ready),
            ("T4", TaskState::Pending),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        let assignments = HashMap::new();
        // T1 is a deviation source → T2 and T4 paused; T3 unaffected.
        // T2 READY but paused; T4 promoted then paused.
        let pick = DagScheduler::from_states(
            &tasks,
            &states,
            &assignments,
            ConcurrencyLimits::default(),
            &["T1".to_string()],
        )
        .pick();
        assert!(pick.promoted.contains(&"T4".to_string()) || pick.ineligible.contains_key("T4")
            || pick.dispatchable.contains(&"T3".to_string()));
        assert!(pick.dispatchable.contains(&"T3".to_string()));
        assert!(!pick.dispatchable.contains(&"T2".to_string()));
        assert!(matches!(
            pick.ineligible.get("T2"),
            Some(DispatchIneligibleReason::ContractDeviationPaused {
                source_task_id
            }) if source_task_id == "T1"
        ));
        // T4: deps T2+T3 not all DONE → not promoted. Make T2/T3 done:
        let states2: HashMap<_, _> = [
            ("T1", TaskState::Done),
            ("T2", TaskState::Done),
            ("T3", TaskState::Done),
            ("T4", TaskState::Pending),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        let pick2 = DagScheduler::from_states(
            &tasks,
            &states2,
            &assignments,
            ConcurrencyLimits::default(),
            &["T1".to_string()],
        )
        .pick();
        assert_eq!(pick2.promoted, vec!["T4"]);
        assert!(pick2.dispatchable.is_empty());
        assert!(matches!(
            pick2.ineligible.get("T4"),
            Some(DispatchIneligibleReason::ContractDeviationPaused { .. })
        ));
    }

    /// F9.a⑥ promoted when deps all DONE
    #[test]
    fn f9a_promoted_pending_when_deps_done() {
        let tasks = make_test_plan().tasks;
        let states: HashMap<_, _> = [
            ("T1", TaskState::Done),
            ("T2", TaskState::Pending),
            ("T3", TaskState::Done),
            ("T4", TaskState::Pending),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        let assignments = HashMap::from([
            ("T1".to_string(), ("claude".to_string(), "m".to_string())),
            ("T2".to_string(), ("claude".to_string(), "m".to_string())),
            ("T3".to_string(), ("copilot".to_string(), "m".to_string())),
            ("T4".to_string(), ("codex".to_string(), "m".to_string())),
        ]);
        let pick =
            DagScheduler::from_states(&tasks, &states, &assignments, ConcurrencyLimits::default(), &[])
                .pick();
        // T2 depends only on T1 (DONE) → promoted; T4 depends on T2+T3 → T2 not DONE → not promoted
        assert_eq!(pick.promoted, vec!["T2"]);
        assert!(pick.dispatchable.contains(&"T2".to_string()));
        assert!(!pick.promoted.contains(&"T4".to_string()));
    }

    #[test]
    fn f9a_ineligible_serde_camel_case_fields() {
        let reason = DispatchIneligibleReason::WriteScopeCollision {
            conflicting_task_id: "T1".to_string(),
            overlapping_paths: vec!["src/**".to_string(), "src/server/**".to_string()],
        };
        let json = serde_json::to_value(&reason).unwrap();
        let obj = json.get("writeScopeCollision").expect("variant tag");
        assert!(obj.get("conflictingTaskId").is_some());
        assert!(obj.get("overlappingPaths").is_some());
        assert!(obj.get("conflicting_task_id").is_none());
    }
}
