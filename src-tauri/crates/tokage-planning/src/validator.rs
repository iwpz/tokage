//! Plan validation logic (docs/02 §7, P5.m3a).
//!
//! Validates:
//! 1. Tasks count <= max_tasks (default 20).
//! 2. Task graph is a DAG (no cycles) and all `depends_on` refer to existing task IDs.
//! 3. All referenced `contracts` refer to declared contracts in the plan.
//! 4. Domain mapping: domain must belong to valid domains (or auto-mapped to nearest/general).
//! 5. Parallel write scope collision: parallel tasks (tasks without transitive dependency)
//!    must not have overlapping write_scope patterns. If overlapping, auto-serialized or flagged.
//! 6. Acceptance criteria: every task must have at least one acceptance criterion / command.
//! 7. Acceptance commands allowed list validation: commands should match project allow list / safe levels.

use std::collections::{HashMap, HashSet, VecDeque};
use tokage_core::planning::{Plan, TaskItem};

/// Severity of a validation issue.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ValidationSeverity {
    Info,
    Warning,
    Error,
}

/// A specific issue found during plan validation.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationIssue {
    pub severity: ValidationSeverity,
    pub rule: String,
    pub task_id: Option<String>,
    pub message: String,
}

/// Options to customize plan validation.
#[derive(Clone, Debug)]
pub struct PlanValidationOptions {
    /// Maximum allowed tasks (docs/02 §7 default is 20).
    pub max_tasks: usize,
    /// Known valid domains from knowledge pack.
    pub valid_domains: Vec<String>,
    /// Project build/test commands to check against or supplement.
    pub allowed_commands: Vec<String>,
    /// If true, automatically add dependencies to serialize parallel tasks with overlapping write scopes.
    pub auto_serialize_conflicts: bool,
}

impl Default for PlanValidationOptions {
    fn default() -> Self {
        Self {
            max_tasks: 20,
            valid_domains: vec![
                "backend-api".into(),
                "frontend-ui".into(),
                "database".into(),
                "devops".into(),
                "testing".into(),
                "security".into(),
                "docs".into(),
                "general".into(),
            ],
            allowed_commands: Vec::new(),
            auto_serialize_conflicts: true,
        }
    }
}

/// Outcome of plan validation.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationResult {
    pub is_valid: bool,
    pub issues: Vec<ValidationIssue>,
    /// The potentially modified/repaired plan (e.g. domain mapped, conflicts serialized).
    pub plan: Plan,
}

/// Validates and optionally repairs a Plan according to docs/02 §7.
pub fn validate_plan(mut plan: Plan, opts: &PlanValidationOptions) -> ValidationResult {
    let mut issues = Vec::new();

    // 1. Task count limit (<= max_tasks)
    if plan.tasks.len() > opts.max_tasks {
        issues.push(ValidationIssue {
            severity: ValidationSeverity::Error,
            rule: "max_tasks".to_string(),
            task_id: None,
            message: format!(
                "任务数量超过上限（当前 {}，上限 {}），建议拆分为多个阶段",
                plan.tasks.len(),
                opts.max_tasks
            ),
        });
    }

    if plan.tasks.is_empty() {
        issues.push(ValidationIssue {
            severity: ValidationSeverity::Error,
            rule: "empty_tasks".to_string(),
            task_id: None,
            message: "计划未包含任何任务".to_string(),
        });
        return ValidationResult {
            is_valid: false,
            issues,
            plan,
        };
    }

    // Index all task IDs and check duplicate IDs
    let mut task_id_set = HashSet::new();
    for task in &plan.tasks {
        if !task_id_set.insert(task.id.clone()) {
            issues.push(ValidationIssue {
                severity: ValidationSeverity::Error,
                rule: "duplicate_task_id".to_string(),
                task_id: Some(task.id.clone()),
                message: format!("存在重复的任务 ID: {}", task.id),
            });
        }
    }

    // Index declared contract IDs
    let contract_id_set: HashSet<String> = plan
        .contracts
        .iter()
        .map(|c| c.id.to_uppercase())
        .collect();

    // 2. Validate dependencies and contract references
    for task in &plan.tasks {
        for dep in &task.depends_on {
            if !task_id_set.contains(dep) {
                issues.push(ValidationIssue {
                    severity: ValidationSeverity::Error,
                    rule: "missing_dependency".to_string(),
                    task_id: Some(task.id.clone()),
                    message: format!("依赖的任务不存在: {}", dep),
                });
            }
            if dep == &task.id {
                issues.push(ValidationIssue {
                    severity: ValidationSeverity::Error,
                    rule: "self_dependency".to_string(),
                    task_id: Some(task.id.clone()),
                    message: format!("任务不能依赖自身: {}", task.id),
                });
            }
        }

        for contract_ref in &task.contracts {
            if !contract_id_set.contains(&contract_ref.to_uppercase()) {
                issues.push(ValidationIssue {
                    severity: ValidationSeverity::Warning,
                    rule: "missing_contract".to_string(),
                    task_id: Some(task.id.clone()),
                    message: format!("引用的契约未在 Contracts 段声明: {}", contract_ref),
                });
            }
        }
    }

    // 3. Cycle detection (Topological Sort / Kahn's algorithm)
    let has_cycle = check_dag_cycles(&plan.tasks);
    if has_cycle {
        issues.push(ValidationIssue {
            severity: ValidationSeverity::Error,
            rule: "dependency_cycle".to_string(),
            task_id: None,
            message: "任务依赖图中存在循环依赖（环）".to_string(),
        });
    }

    // 4. Domain mapping: domain belongs to valid domains or mapped
    let valid_domain_set: HashSet<String> = opts
        .valid_domains
        .iter()
        .map(|d| d.to_lowercase())
        .collect();

    for task in &mut plan.tasks {
        let domain_clean = task.domain.trim().to_lowercase();
        if !valid_domain_set.contains(&domain_clean) {
            let mapped = map_to_nearest_domain(&domain_clean, &opts.valid_domains);
            issues.push(ValidationIssue {
                severity: ValidationSeverity::Info,
                rule: "domain_mapping".to_string(),
                task_id: Some(task.id.clone()),
                message: format!(
                    "任务领域 '{}' 未在领域表中，已自动映射为 '{}'",
                    task.domain, mapped
                ),
            });
            task.domain = mapped;
        }
    }

    // 5. Acceptance criteria check & command check
    for task in &mut plan.tasks {
        let has_criteria = !task.acceptance.criteria.is_empty();
        let has_commands = !task.acceptance.commands.is_empty();

        if !has_criteria && !has_commands {
            if !opts.allowed_commands.is_empty() {
                task.acceptance.commands = opts.allowed_commands.clone();
                issues.push(ValidationIssue {
                    severity: ValidationSeverity::Info,
                    rule: "acceptance_auto_fill".to_string(),
                    task_id: Some(task.id.clone()),
                    message: "任务缺少验收标准，已自动填入项目默认构建/测试命令".to_string(),
                });
            } else {
                issues.push(ValidationIssue {
                    severity: ValidationSeverity::Warning,
                    rule: "missing_acceptance".to_string(),
                    task_id: Some(task.id.clone()),
                    message: "任务未声明任何验收命令或自查标准".to_string(),
                });
            }
        }

        // Validate command safety against allowed list if provided
        if !opts.allowed_commands.is_empty() {
            for cmd in &task.acceptance.commands {
                if !is_command_allowed(cmd, &opts.allowed_commands) {
                    issues.push(ValidationIssue {
                        severity: ValidationSeverity::Warning,
                        rule: "unverified_acceptance_command".to_string(),
                        task_id: Some(task.id.clone()),
                        message: format!("验收命令不在项目已知脚本列表中，执行时需注意确认: {}", cmd),
                    });
                }
            }
        }
    }

    // 6. Parallel write scope collision check
    if !has_cycle {
        check_and_resolve_write_conflicts(&mut plan, opts, &mut issues);
    }

    let is_valid = !issues.iter().any(|i| i.severity == ValidationSeverity::Error);

    ValidationResult {
        is_valid,
        issues,
        plan,
    }
}

/// Checks if DAG has cycles using Kahn's algorithm.
fn check_dag_cycles(tasks: &[TaskItem]) -> bool {
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();

    for t in tasks {
        in_degree.entry(&t.id).or_insert(0);
        adj.entry(&t.id).or_default();
    }

    for t in tasks {
        for dep in &t.depends_on {
            if in_degree.contains_key(dep.as_str()) {
                *in_degree.entry(&t.id).or_insert(0) += 1;
                adj.entry(dep.as_str()).or_default().push(&t.id);
            }
        }
    }

    let mut queue = VecDeque::new();
    for (id, &deg) in &in_degree {
        if deg == 0 {
            queue.push_back(*id);
        }
    }

    let mut visited_count = 0;
    while let Some(u) = queue.pop_front() {
        visited_count += 1;
        if let Some(neighbors) = adj.get(u) {
            for &v in neighbors {
                if let Some(deg) = in_degree.get_mut(v) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(v);
                    }
                }
            }
        }
    }

    visited_count < tasks.len()
}

/// Maps an unknown domain to the closest standard domain or "general".
fn map_to_nearest_domain(raw: &str, valid_domains: &[String]) -> String {
    let raw_lower = raw.to_lowercase();
    if raw_lower.contains("api") || raw_lower.contains("back") || raw_lower.contains("server") {
        return "backend-api".to_string();
    }
    if raw_lower.contains("ui") || raw_lower.contains("front") || raw_lower.contains("page") || raw_lower.contains("web") {
        return "frontend-ui".to_string();
    }
    if raw_lower.contains("db") || raw_lower.contains("data") || raw_lower.contains("sql") {
        return "database".to_string();
    }
    if raw_lower.contains("test") || raw_lower.contains("qa") {
        return "testing".to_string();
    }
    if raw_lower.contains("ops") || raw_lower.contains("ci") || raw_lower.contains("deploy") {
        return "devops".to_string();
    }
    if raw_lower.contains("doc") {
        return "docs".to_string();
    }
    if raw_lower.contains("sec") || raw_lower.contains("auth") {
        return "security".to_string();
    }
    if valid_domains.iter().any(|d| d == "general") {
        "general".to_string()
    } else {
        valid_domains.first().cloned().unwrap_or_else(|| "general".to_string())
    }
}

/// Checks if command is in allowed commands or matches simple prefix.
fn is_command_allowed(cmd: &str, allowed: &[String]) -> bool {
    let clean = cmd.trim();
    if allowed.iter().any(|a| a.trim() == clean) {
        return true;
    }
    // Also allow if command starts with an allowed binary (e.g. "cargo test", "npm test")
    let first_word = clean.split_whitespace().next().unwrap_or("");
    for a in allowed {
        let allowed_first = a.split_whitespace().next().unwrap_or("");
        if !first_word.is_empty() && first_word == allowed_first {
            return true;
        }
    }
    false
}

/// Computes reachable set of dependencies for each task (transitive closure).
fn compute_transitive_dependencies(tasks: &[TaskItem]) -> HashMap<String, HashSet<String>> {
    let mut direct_deps: HashMap<String, Vec<String>> = HashMap::new();
    for t in tasks {
        direct_deps.insert(t.id.clone(), t.depends_on.clone());
    }

    let mut closure: HashMap<String, HashSet<String>> = HashMap::new();
    for t in tasks {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::from(t.depends_on.clone());
        while let Some(curr) = queue.pop_front() {
            if visited.insert(curr.clone()) {
                if let Some(upstream) = direct_deps.get(&curr) {
                    for u in upstream {
                        queue.push_back(u.clone());
                    }
                }
            }
        }
        closure.insert(t.id.clone(), visited);
    }
    closure
}

/// Checks write scope collision between parallel tasks and serializes if requested.
fn check_and_resolve_write_conflicts(
    plan: &mut Plan,
    opts: &PlanValidationOptions,
    issues: &mut Vec<ValidationIssue>,
) {
    let trans_deps = compute_transitive_dependencies(&plan.tasks);
    let n = plan.tasks.len();
    let mut added_deps: Vec<(usize, String)> = Vec::new(); // (target_task_idx, upstream_id)

    for i in 0..n {
        for j in (i + 1)..n {
            let id_i = &plan.tasks[i].id;
            let id_j = &plan.tasks[j].id;

            // Are tasks parallel? (neither depends on the other)
            let i_depends_on_j = trans_deps.get(id_i).map(|s| s.contains(id_j)).unwrap_or(false);
            let j_depends_on_i = trans_deps.get(id_j).map(|s| s.contains(id_i)).unwrap_or(false);

            if !i_depends_on_j && !j_depends_on_i {
                // Check if write_scopes overlap
                let scope_i = &plan.tasks[i].write_scope;
                let scope_j = &plan.tasks[j].write_scope;
                if let Some(overlap) = find_scope_overlap(scope_i, scope_j) {
                    if opts.auto_serialize_conflicts {
                        // Serialize: make task j depend on task i
                        added_deps.push((j, id_i.clone()));
                        issues.push(ValidationIssue {
                            severity: ValidationSeverity::Info,
                            rule: "parallel_write_conflict_auto_resolved".to_string(),
                            task_id: Some(id_j.clone()),
                            message: format!(
                                "并行任务 {} 与 {} 写入范围重叠 ({})，已自动添加依赖使其串行执行",
                                id_i, id_j, overlap
                            ),
                        });
                    } else {
                        issues.push(ValidationIssue {
                            severity: ValidationSeverity::Warning,
                            rule: "parallel_write_conflict".to_string(),
                            task_id: Some(id_j.clone()),
                            message: format!(
                                "并行任务 {} 与 {} 写入范围重叠 ({})，请手动添加依赖或调整范围",
                                id_i, id_j, overlap
                            ),
                        });
                    }
                }
            }
        }
    }

    for (idx, upstream_id) in added_deps {
        if !plan.tasks[idx].depends_on.contains(&upstream_id) {
            plan.tasks[idx].depends_on.push(upstream_id);
        }
    }
}

/// Checks if two sets of path globs overlap.
fn find_scope_overlap(scopes_a: &[String], scopes_b: &[String]) -> Option<String> {
    tokage_core::command_policy::scope::scopes_overlap(scopes_a, scopes_b, false)
        .map(|(a, b)| format!("{} vs {}", a, b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokage_core::planning::{TaskAcceptance, TaskItem};


    #[test]
    fn test_validate_plan_cycle_detection() {
        let tasks = vec![
            TaskItem {
                id: "T1".into(),
                domain: "backend-api".into(),
                title: "Task 1".into(),
                goal: "Goal 1".into(),
                depends_on: vec!["T2".into()],
                ..Default::default()
            },
            TaskItem {
                id: "T2".into(),
                domain: "backend-api".into(),
                title: "Task 2".into(),
                goal: "Goal 2".into(),
                depends_on: vec!["T1".into()],
                ..Default::default()
            },
        ];

        let plan = Plan {
            tasks,
            ..Default::default()
        };

        let res = validate_plan(plan, &PlanValidationOptions::default());
        assert!(!res.is_valid);
        assert!(res.issues.iter().any(|i| i.rule == "dependency_cycle"));
    }

    #[test]
    fn test_validate_plan_max_tasks_exceeded() {
        let tasks: Vec<TaskItem> = (1..=25)
            .map(|i| TaskItem {
                id: format!("T{}", i),
                domain: "backend-api".into(),
                title: format!("Task {}", i),
                goal: format!("Goal {}", i),
                acceptance: TaskAcceptance {
                    commands: vec!["cargo test".into()],
                    criteria: vec![],
                },
                ..Default::default()
            })
            .collect();

        let plan = Plan {
            tasks,
            ..Default::default()
        };

        let res = validate_plan(plan, &PlanValidationOptions::default());
        assert!(!res.is_valid);
        assert!(res.issues.iter().any(|i| i.rule == "max_tasks"));
    }

    #[test]
    fn test_validate_plan_domain_mapping() {
        let tasks = vec![TaskItem {
            id: "T1".into(),
            domain: "frontend-components".into(), // Unlisted domain
            title: "Task 1".into(),
            goal: "Goal 1".into(),
            acceptance: TaskAcceptance {
                commands: vec!["pnpm test".into()],
                criteria: vec![],
            },
            ..Default::default()
        }];

        let plan = Plan {
            tasks,
            ..Default::default()
        };

        let res = validate_plan(plan, &PlanValidationOptions::default());
        assert!(res.is_valid);
        assert_eq!(res.plan.tasks[0].domain, "frontend-ui");
        assert!(res.issues.iter().any(|i| i.rule == "domain_mapping"));
    }

    #[test]
    fn test_validate_plan_write_scope_collision_auto_serialized() {
        let tasks = vec![
            TaskItem {
                id: "T1".into(),
                domain: "backend-api".into(),
                title: "Task 1".into(),
                goal: "Goal 1".into(),
                write_scope: vec!["src/routes/**".into()],
                acceptance: TaskAcceptance {
                    commands: vec!["cargo test".into()],
                    criteria: vec![],
                },
                ..Default::default()
            },
            TaskItem {
                id: "T2".into(),
                domain: "backend-api".into(),
                title: "Task 2".into(),
                goal: "Goal 2".into(),
                write_scope: vec!["src/routes/auth.rs".into()],
                acceptance: TaskAcceptance {
                    commands: vec!["cargo test".into()],
                    criteria: vec![],
                },
                ..Default::default()
            },
        ];

        let plan = Plan {
            tasks,
            ..Default::default()
        };

        let res = validate_plan(plan, &PlanValidationOptions::default());
        assert!(res.is_valid);
        // T2 should now depend on T1
        assert_eq!(res.plan.tasks[1].depends_on, vec!["T1"]);
        assert!(res.issues.iter().any(|i| i.rule == "parallel_write_conflict_auto_resolved"));
    }
}
