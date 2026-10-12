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
    /// English fallback text; the UI localizes by `rule` + `params` when it can.
    pub message: String,
    /// Positional values interpolated into the localized message for `rule`.
    #[serde(default)]
    pub params: Vec<String>,
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
    /// Effective pack `gate_allowlist` patterns (F5.c). Empty skips gate confirmation rules.
    pub gate_allowlist: Vec<String>,
    /// If true, automatically add dependencies to serialize parallel tasks with overlapping write scopes.
    pub auto_serialize_conflicts: bool,
}

impl Default for PlanValidationOptions {
    fn default() -> Self {
        Self {
            max_tasks: 20,
            // Align with knowledge-pack domains (docs/04 §3) so skill lookup hits.
            valid_domains: crate::prompt::PACK_DOMAINS
                .iter()
                .map(|d| (*d).to_string())
                .collect(),
            allowed_commands: Vec::new(),
            gate_allowlist: Vec::new(),
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

    // Normalize write_scope entries to globs (D4-A / F4.e) before overlap checks.
    for task in &mut plan.tasks {
        task.write_scope = task
            .write_scope
            .iter()
            .map(|s| tokage_core::command_policy::scope::normalize_scope(s))
            .collect();
    }

    // 1. Task count limit (<= max_tasks)
    if plan.tasks.len() > opts.max_tasks {
        issues.push(ValidationIssue {
            severity: ValidationSeverity::Error,
            rule: "max_tasks".to_string(),
            params: vec![plan.tasks.len().to_string(), opts.max_tasks.to_string()],
            task_id: None,
            message: format!(
                "Task count exceeds the limit (current {}, limit {}); consider splitting into phases",
                plan.tasks.len(),
                opts.max_tasks
            ),
        });
    }

    if plan.tasks.is_empty() {
        issues.push(ValidationIssue {
            severity: ValidationSeverity::Error,
            rule: "empty_tasks".to_string(),
            params: vec![],
            task_id: None,
            message: "The plan contains no tasks".to_string(),
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
            params: vec![task.id.clone()],
                task_id: Some(task.id.clone()),
                message: format!("Duplicate task ID: {}", task.id),
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
            params: vec![dep.clone()],
                    task_id: Some(task.id.clone()),
                    message: format!("Dependency task does not exist: {}", dep),
                });
            }
            if dep == &task.id {
                issues.push(ValidationIssue {
                    severity: ValidationSeverity::Error,
                    rule: "self_dependency".to_string(),
            params: vec![task.id.clone()],
                    task_id: Some(task.id.clone()),
                    message: format!("A task cannot depend on itself: {}", task.id),
                });
            }
        }

        for contract_ref in &task.contracts {
            if !contract_id_set.contains(&contract_ref.to_uppercase()) {
                issues.push(ValidationIssue {
                    severity: ValidationSeverity::Warning,
                    rule: "missing_contract".to_string(),
            params: vec![contract_ref.clone()],
                    task_id: Some(task.id.clone()),
                    message: format!("Referenced contract is not declared in the Contracts section: {}", contract_ref),
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
            params: vec![],
            task_id: None,
            message: "The task dependency graph contains a cycle".to_string(),
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
            params: vec![task.domain.clone(), mapped.clone()],
                task_id: Some(task.id.clone()),
                message: format!(
                    "Task domain '{}' is not in the domain table; automatically mapped to '{}'",
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
            params: vec![],
                    task_id: Some(task.id.clone()),
                    message: "The task has no acceptance criteria; filled in the project default build/test commands".to_string(),
                });
            } else {
                issues.push(ValidationIssue {
                    severity: ValidationSeverity::Warning,
                    rule: "missing_acceptance".to_string(),
            params: vec![],
                    task_id: Some(task.id.clone()),
                    message: "The task declares no acceptance commands or self-check criteria".to_string(),
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
            params: vec![cmd.clone()],
                        task_id: Some(task.id.clone()),
                        message: format!("Acceptance command is not in the project's known scripts; confirm it before running: {}", cmd),
                    });
                }
            }
        }

        // F5.c: gate allowlist / confirmed_commands / metacharacters
        if !opts.gate_allowlist.is_empty() {
            for cmd in &task.acceptance.commands {
                if acceptance_has_metachar(cmd) {
                    issues.push(ValidationIssue {
                        severity: ValidationSeverity::Error,
                        rule: "unsafe_acceptance_command".to_string(),
            params: vec![cmd.clone()],
                        task_id: Some(task.id.clone()),
                        message: format!(
                            "Acceptance command contains shell metacharacters and cannot be confirmed: {}",
                            cmd
                        ),
                    });
                    continue;
                }
                let on_allowlist = gate_allowlist_matches(cmd, &opts.gate_allowlist);
                let confirmed = task.acceptance.confirmed_commands.iter().any(|c| c == cmd);
                if on_allowlist {
                    continue;
                }
                if confirmed {
                    issues.push(ValidationIssue {
                        severity: ValidationSeverity::Info,
                        rule: "acceptance_command_confirmed".to_string(),
            params: vec![cmd.clone()],
                        task_id: Some(task.id.clone()),
                        message: format!("Acceptance command was confirmed by the user and is not on the allowlist: {}", cmd),
                    });
                } else {
                    issues.push(ValidationIssue {
                        severity: ValidationSeverity::Error,
                        rule: "unconfirmed_acceptance_command".to_string(),
            params: vec![cmd.clone()],
                        task_id: Some(task.id.clone()),
                        message: format!(
                            "Acceptance command is not on the allowlist and not confirmed; confirm or adjust it in the task details: {}",
                            cmd
                        ),
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

/// Maps an unknown / legacy domain to the closest pack domain or "general".
///
/// Legacy aliases (`database`→`data-model`, `devops`→`infra`, `security`→`backend-api`)
/// are resolved before keyword heuristics so pack skill keys are hit.
fn map_to_nearest_domain(raw: &str, valid_domains: &[String]) -> String {
    let raw_lower = raw.to_lowercase().replace('_', "-");

    let pick = |candidate: &str| -> Option<String> {
        valid_domains
            .iter()
            .find(|d| d.eq_ignore_ascii_case(candidate))
            .cloned()
    };

    // Exact / legacy aliases first (avoid "data-model" → wrong remap via "data").
    if let Some(d) = pick(&raw_lower) {
        return d;
    }
    if matches!(raw_lower.as_str(), "database" | "db" | "sql") {
        if let Some(d) = pick("data-model") {
            return d;
        }
    }
    if matches!(raw_lower.as_str(), "devops" | "sre" | "ops") {
        if let Some(d) = pick("infra") {
            return d;
        }
    }
    if matches!(raw_lower.as_str(), "security" | "sec" | "auth" | "authn" | "authz") {
        if let Some(d) = pick("backend-api") {
            return d;
        }
    }

    let candidate = if raw_lower.contains("api")
        || raw_lower.contains("back")
        || raw_lower.contains("server")
        || raw_lower.contains("auth")
        || raw_lower.contains("sec")
    {
        "backend-api"
    } else if raw_lower.contains("ui")
        || raw_lower.contains("front")
        || raw_lower.contains("page")
        || raw_lower.contains("web")
        || raw_lower.contains("component")
    {
        "frontend-ui"
    } else if raw_lower.contains("db")
        || raw_lower.contains("sql")
        || raw_lower.contains("schema")
        || raw_lower.contains("migration")
        || raw_lower.contains("orm")
        || raw_lower == "data"
        || raw_lower.starts_with("data-")
        || raw_lower.ends_with("-data")
    {
        // Prefer data-model; never emit legacy "database".
        "data-model"
    } else if raw_lower.contains("algo")
        || raw_lower.contains("perf")
        || raw_lower.contains("optim")
        || raw_lower.contains("complex")
    {
        "algorithm"
    } else if raw_lower.contains("integrat") || raw_lower.contains("e2e")
    {
        "integration"
    } else if raw_lower.contains("refactor") || raw_lower.contains("migrate")
    {
        "refactor"
    } else if raw_lower.contains("ops")
        || raw_lower.contains("ci")
        || raw_lower.contains("deploy")
        || raw_lower.contains("docker")
        || raw_lower.contains("k8s")
        || raw_lower.contains("infra")
    {
        "infra"
    } else if raw_lower.contains("test") || raw_lower.contains("qa") {
        "testing"
    } else if raw_lower.contains("doc") {
        "docs"
    } else if raw_lower.contains("review") || raw_lower.contains("audit") {
        "review"
    } else {
        "general"
    };

    pick(candidate)
        .or_else(|| pick("general"))
        .unwrap_or_else(|| {
            valid_domains
                .first()
                .cloned()
                .unwrap_or_else(|| "general".to_string())
        })
}

/// Checks if command is in allowed commands or matches simple prefix.
fn acceptance_has_metachar(cmd: &str) -> bool {
    let t = cmd.trim();
    t.contains('|')
        || t.contains('>')
        || t.contains('<')
        || t.contains(';')
        || t.contains("&&")
        || t.contains("||")
        || t.contains('`')
        || t.contains("$(")
}

fn gate_allowlist_matches(cmd: &str, allowlist: &[String]) -> bool {
    let trimmed = cmd.trim();
    for pat in allowlist {
        if let Ok(re) = regex::Regex::new(pat) {
            if re.is_match(trimmed) {
                return true;
            }
        }
    }
    false
}

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
            params: vec![id_i.clone(), id_j.clone(), overlap.to_string()],
                            task_id: Some(id_j.clone()),
                            message: format!(
                                "Parallel tasks {} and {} have overlapping write scopes ({}); a dependency was added automatically to run them serially",
                                id_i, id_j, overlap
                            ),
                        });
                    } else {
                        issues.push(ValidationIssue {
                            severity: ValidationSeverity::Warning,
                            rule: "parallel_write_conflict".to_string(),
            params: vec![id_i.clone(), id_j.clone(), overlap.to_string()],
                            task_id: Some(id_j.clone()),
                            message: format!(
                                "Parallel tasks {} and {} have overlapping write scopes ({}); add a dependency manually or adjust the scopes",
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
                ..Default::default()
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
                ..Default::default()
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
    fn test_legacy_domains_map_to_pack_keys() {
        let cases = [
            ("database", "data-model"),
            ("devops", "infra"),
            ("security", "backend-api"),
            ("data-model", "data-model"), // already valid — no remap needed
        ];
        for (raw, expected) in cases {
            let plan = Plan {
                tasks: vec![TaskItem {
                    id: "T1".into(),
                    domain: raw.into(),
                    title: "t".into(),
                    goal: "g".into(),
                    acceptance: TaskAcceptance {
                        commands: vec!["cargo test".into()],
                        criteria: vec![],
                        ..Default::default()
                    },
                    ..Default::default()
                }],
                ..Default::default()
            };
            let res = validate_plan(plan, &PlanValidationOptions::default());
            assert_eq!(
                res.plan.tasks[0].domain, expected,
                "legacy/raw domain `{raw}` should resolve to `{expected}`"
            );
            assert!(
                crate::prompt::PACK_DOMAINS.contains(&res.plan.tasks[0].domain.as_str()),
                "resolved domain must be a pack key"
            );
        }
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
                ..Default::default()
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
                ..Default::default()
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

    #[test]
    fn test_validate_plan_serializes_nested_glob_scopes() {
        // F4.f: src/** overlaps src/server/** → auto-serialize.
        let plan = Plan {
            tasks: vec![
                TaskItem {
                    id: "T1".into(),
                    domain: "backend-api".into(),
                    title: "Broad".into(),
                    goal: "Broad".into(),
                    write_scope: vec!["src/**".into()],
                    acceptance: TaskAcceptance {
                        commands: vec!["cargo test".into()],
                        criteria: vec![],
                ..Default::default()
            },
                    ..Default::default()
                },
                TaskItem {
                    id: "T2".into(),
                    domain: "backend-api".into(),
                    title: "Narrow".into(),
                    goal: "Narrow".into(),
                    write_scope: vec!["src/server/**".into()],
                    acceptance: TaskAcceptance {
                        commands: vec!["cargo test".into()],
                        criteria: vec![],
                ..Default::default()
            },
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        let res = validate_plan(plan, &PlanValidationOptions::default());
        assert!(res.is_valid);
        assert_eq!(res.plan.tasks[1].depends_on, vec!["T1"]);
        assert!(res.issues.iter().any(|i| i.rule == "parallel_write_conflict_auto_resolved"));
    }

    #[test]
    fn test_validate_plan_does_not_overlap_sibling_prefixes() {
        // F4: src/a/** must not collide with src/ab/** (old starts_with bug).
        let plan = Plan {
            tasks: vec![
                TaskItem {
                    id: "T1".into(),
                    domain: "backend-api".into(),
                    title: "A".into(),
                    goal: "A".into(),
                    write_scope: vec!["src/a/**".into()],
                    acceptance: TaskAcceptance {
                        commands: vec!["cargo test".into()],
                        criteria: vec![],
                ..Default::default()
            },
                    ..Default::default()
                },
                TaskItem {
                    id: "T2".into(),
                    domain: "backend-api".into(),
                    title: "AB".into(),
                    goal: "AB".into(),
                    write_scope: vec!["src/ab/**".into()],
                    acceptance: TaskAcceptance {
                        commands: vec!["cargo test".into()],
                        criteria: vec![],
                ..Default::default()
            },
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        let res = validate_plan(plan, &PlanValidationOptions::default());
        assert!(res.is_valid);
        assert!(res.plan.tasks[1].depends_on.is_empty());
        assert!(!res.issues.iter().any(|i| i.rule.starts_with("parallel_write_conflict")));
    }

    #[test]
    fn test_validate_plan_unconfirmed_acceptance_command() {
        let pack: Vec<String> = serde_json::from_value(
            serde_json::from_str::<serde_json::Value>(include_str!(
                "../../../resources/knowledge/builtin.json"
            ))
            .unwrap()["gate_allowlist"]
                .clone(),
        )
        .unwrap();
        let plan = Plan {
            tasks: vec![TaskItem {
                id: "T1".into(),
                domain: "backend-api".into(),
                title: "T".into(),
                goal: "G".into(),
                acceptance: TaskAcceptance {
                    commands: vec!["make deploy".into()],
                    criteria: vec![],
                    confirmed_commands: vec![],
                },
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut opts = PlanValidationOptions::default();
        opts.gate_allowlist = pack.clone();
        let res = validate_plan(plan, &opts);
        assert!(!res.is_valid);
        assert!(res
            .issues
            .iter()
            .any(|i| i.rule == "unconfirmed_acceptance_command"));

        let plan2 = Plan {
            tasks: vec![TaskItem {
                id: "T1".into(),
                domain: "backend-api".into(),
                title: "T".into(),
                goal: "G".into(),
                acceptance: TaskAcceptance {
                    commands: vec!["make deploy".into()],
                    criteria: vec![],
                    confirmed_commands: vec!["make deploy".into()],
                },
                ..Default::default()
            }],
            ..Default::default()
        };
        let res2 = validate_plan(plan2, &opts);
        assert!(res2.is_valid);
        assert!(res2
            .issues
            .iter()
            .any(|i| i.rule == "acceptance_command_confirmed"));

        // npm script names with colons (test:miniapp, build:f) are allowlisted.
        let plan3 = Plan {
            tasks: vec![TaskItem {
                id: "T1".into(),
                domain: "testing".into(),
                title: "T".into(),
                goal: "G".into(),
                acceptance: TaskAcceptance {
                    commands: vec![
                        "npm run test:miniapp".into(),
                        "npm run build:f".into(),
                        "npm run build:b".into(),
                    ],
                    criteria: vec![],
                    confirmed_commands: vec![],
                },
                ..Default::default()
            }],
            ..Default::default()
        };
        let res3 = validate_plan(plan3, &opts);
        assert!(
            res3.is_valid,
            "colonated npm scripts should match gate_allowlist: {:?}",
            res3.issues
        );
        assert!(!res3
            .issues
            .iter()
            .any(|i| i.rule == "unconfirmed_acceptance_command"));

        // Targeted test file args after `--` (same shape as pytest/npx path args).
        let plan4 = Plan {
            tasks: vec![TaskItem {
                id: "T1".into(),
                domain: "testing".into(),
                title: "T".into(),
                goal: "G".into(),
                acceptance: TaskAcceptance {
                    commands: vec![
                        "npm run test -- src/stores/auth.test.ts src/lib/unifiedApprovals.test.ts"
                            .into(),
                        "npm run test -- src/lib/taskRunner.test.ts src/lib/taskState.test.ts src/lib/planApprove.test.ts"
                            .into(),
                    ],
                    criteria: vec![],
                    confirmed_commands: vec![],
                },
                ..Default::default()
            }],
            ..Default::default()
        };
        let res4 = validate_plan(plan4, &opts);
        assert!(
            res4.is_valid,
            "npm run test -- <files> should match gate_allowlist: {:?}",
            res4.issues
        );
        assert!(!res4
            .issues
            .iter()
            .any(|i| i.rule == "unconfirmed_acceptance_command"));
    }

    #[test]
    fn test_validate_plan_normalizes_write_scope() {
        let plan = Plan {
            tasks: vec![TaskItem {
                id: "T1".into(),
                domain: "backend-api".into(),
                title: "Task 1".into(),
                goal: "Goal 1".into(),
                write_scope: vec!["./src/users/".into(), "src\\server\\".into(), "src/main.rs".into()],
                acceptance: TaskAcceptance {
                    commands: vec!["cargo test".into()],
                    criteria: vec![],
                ..Default::default()
            },
                ..Default::default()
            }],
            ..Default::default()
        };

        let res = validate_plan(plan, &PlanValidationOptions::default());
        assert!(res.is_valid);
        assert_eq!(
            res.plan.tasks[0].write_scope,
            vec![
                "src/users/**".to_string(),
                "src/server/**".to_string(),
                "src/main.rs/**".to_string(),
            ]
        );
    }
}
