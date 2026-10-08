//! Global and single-task model assignment engine (docs/04 §7, §8, P6.m5).
//!
//! Handles:
//! - Global plan assignment across DAG tasks with cumulative provider utilization tracking.
//! - Parallel load penalty and context window reuse bonuses.
//! - Planning model selection (ranked strictly by p_success).
//! - Direct single-task assignment.
//! - Re-evaluation of remaining tasks upon manual reassignment.

use std::collections::{HashMap, HashSet};
use serde::{Deserialize, Serialize};

use tokage_core::planning::{Plan, TaskItem, TaskSize};
use crate::cost::{calculate_c_exec, calculate_eff_price, calculate_scarcity, TokenPriceKind};
use crate::expected_cost::{calculate_expected_cost, EvaluatedCandidate};
use crate::learning::{resolve_p_min, LearnedIndex};
use crate::merge::{EffectiveCandidate, EffectiveKnowledgePack};
use crate::pack::UsageProfileEntry;
use crate::probability::{calculate_prior_p0, get_p_min_threshold};

/// An alternative model candidate considered during assignment.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AlternativeCandidate {
    pub provider: String,
    pub model_id: String,
    pub display_name: String,
    pub p_success: f64,
    pub est_cost_usd: f64,
    pub meets_threshold: bool,
}

/// Structured assignment result for a task (docs/04 §7).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskAssignment {
    pub task_id: String,
    pub provider: String,
    pub model_id: String,
    pub display_name: String,
    pub cli_model_arg: String,
    pub p_success: f64,
    pub est_cost_usd: f64,
    pub meets_threshold: bool,
    pub reuse_attempt_id: Option<String>,
    pub explanation: String,
    pub alternatives: Vec<AlternativeCandidate>,
}

/// Complete plan allocation result containing assignments for all tasks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlanAllocationResult {
    pub assignments: Vec<TaskAssignment>,
    pub total_est_cost_usd: f64,
    pub all_meet_threshold: bool,
    pub provider_utilizations: HashMap<String, f64>,
}

/// Allocates the optimal model for the PLANNING role (docs/04 §5).
///
/// Planning candidates must have `planning` skill >= planner_min_skill (default: 5.0)
/// and have "planner" in roles. Candidates are ranked strictly by p_success (not cost),
/// because planning output is short and its quality determines the entire plan.
pub fn select_planning_model(
    pack: &EffectiveKnowledgePack,
    difficulty: u8,
) -> Option<EvaluatedCandidate> {
    let min_skill = pack.success_model.planner_min_skill;
    let mut candidates: Vec<EvaluatedCandidate> = Vec::new();

    for cand in &pack.candidates {
        if !cand.roles.iter().any(|r| r.eq_ignore_ascii_case("planner")) {
            continue;
        }

        let skill = cand.skills.get("planning").copied().unwrap_or(0.0);
        if skill < min_skill {
            continue;
        }

        let p_prior = calculate_prior_p0(skill, difficulty, "L", &pack.success_model);
        let p_min = get_p_min_threshold(difficulty, &pack.success_model);

        candidates.push(EvaluatedCandidate {
            provider: cand.provider.clone(),
            model_id: cand.model_id.clone(),
            display_name: cand.display_name.clone(),
            cli_model_arg: cand.cli_model_arg.clone(),
            billing: cand.billing.clone(),
            p_success: p_prior,
            c_exec: 0.10, // Nominal planning cost
            expected_cost: 0.10,
            meets_threshold: p_prior >= p_min,
            origin_layer: cand.origin_layer.clone(),
        });
    }

    // Rank strictly by p_success descending
    candidates.sort_by(|a, b| {
        b.p_success
            .partial_cmp(&a.p_success)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    candidates.into_iter().next()
}

/// Assigns a model for a single DIRECT task (docs/04 §8).
///
/// Uses domain and difficulty from triage, defaulting size to S.
pub fn assign_direct_task(
    domain: &str,
    difficulty: u8,
    size: Option<TaskSize>,
    pack: &EffectiveKnowledgePack,
    learned: &LearnedIndex,
    provider_utilization: Option<&HashMap<String, f64>>,
    confidence_threshold: Option<f64>,
) -> Option<TaskAssignment> {
    let sz = size.unwrap_or(TaskSize::S);
    let task = TaskItem {
        id: "DIRECT".to_string(),
        domain: domain.to_string(),
        title: "Direct Task".to_string(),
        goal: String::new(),
        depends_on: vec![],
        contracts: vec![],
        write_scope: vec![],
        deliverables: vec![],
        acceptance: Default::default(),
        size: sz,
        difficulty,
        assigned_model: None,
    };

    let empty_map = HashMap::new();
    let current_utils = provider_utilization.unwrap_or(&empty_map);
    let mut running_utils = current_utils.clone();
    let prev_assignments = HashMap::new();

    assign_single_task_internal(
        &task,
        pack,
        learned,
        &mut running_utils,
        &prev_assignments,
        confidence_threshold,
    )
}

/// Global allocation algorithm across all tasks in a Plan (docs/04 §7).
///
/// 1. Orders tasks in topological order.
/// 2. For each task, evaluates all candidate models:
///    - Calculates p_success using domain skill and task (difficulty, size) + learned posterior.
///    - Updates provider scarcity based on cumulative projected utilization.
///    - Evaluates window reuse bonuses if immediate predecessor has same model and related scope.
///    - Calculates C_exec and expected cost E(m, t).
/// 3. Filters candidates by p_min; picks argmin E(m, t) (or max p if none qualify).
/// 4. Generates human-readable explanation and alternatives list.
pub fn allocate_plan_models(
    plan: &Plan,
    pack: &EffectiveKnowledgePack,
    learned: &LearnedIndex,
    initial_provider_utils: Option<&HashMap<String, f64>>,
    confidence_threshold: Option<f64>,
) -> PlanAllocationResult {
    let mut provider_utils = initial_provider_utils.cloned().unwrap_or_default();
    let mut assignments: Vec<TaskAssignment> = Vec::new();
    let mut total_cost = 0.0;
    let mut all_meet = true;

    // Order tasks by topological order
    let ordered_tasks = order_tasks_topologically(&plan.tasks);
    let mut assigned_map: HashMap<String, TaskAssignment> = HashMap::new();

    for task in ordered_tasks {
        if let Some(assignment) = assign_single_task_internal(
            task,
            pack,
            learned,
            &mut provider_utils,
            &assigned_map,
            confidence_threshold,
        ) {
            if !assignment.meets_threshold {
                all_meet = false;
            }
            total_cost += assignment.est_cost_usd;
            assigned_map.insert(task.id.clone(), assignment.clone());
            assignments.push(assignment);
        }
    }

    PlanAllocationResult {
        assignments,
        total_est_cost_usd: (total_cost * 100.0).round() / 100.0,
        all_meet_threshold: all_meet,
        provider_utilizations: provider_utils,
    }
}

/// Re-calculates assignments for remaining tasks after a manual user change (docs/04 §7, P6.m5b).
pub fn recompute_plan_allocation_with_overrides(
    plan: &Plan,
    pack: &EffectiveKnowledgePack,
    learned: &LearnedIndex,
    manual_assignments: &HashMap<String, String>, // taskId -> "PROVIDER:model"
    initial_provider_utils: Option<&HashMap<String, f64>>,
    confidence_threshold: Option<f64>,
) -> Result<PlanAllocationResult, String> {
    let mut provider_utils = initial_provider_utils.cloned().unwrap_or_default();
    let mut assignments: Vec<TaskAssignment> = Vec::new();
    let mut total_cost = 0.0;
    let mut all_meet = true;

    let ordered_tasks = order_tasks_topologically(&plan.tasks);
    let mut assigned_map: HashMap<String, TaskAssignment> = HashMap::new();

    for task in ordered_tasks {
        let auto_assignment = assign_single_task_internal(
            task,
            pack,
            learned,
            &mut provider_utils,
            &assigned_map,
            confidence_threshold,
        );

        if let Some(target_override) = manual_assignments.get(&task.id).filter(|s| !s.trim().is_empty()) {
            // Check if the user selected the same model as auto recommendation
            let is_same_as_auto = if let Some(ref auto) = auto_assignment {
                let spec1 = format!("{}:{}", auto.provider, auto.model_id);
                target_override == &spec1 || target_override == &auto.model_id || target_override == &auto.cli_model_arg
            } else {
                false
            };

            let assignment = if is_same_as_auto {
                auto_assignment.unwrap()
            } else {
                create_manual_assignment(
                    task,
                    target_override,
                    pack,
                    learned,
                    &mut provider_utils,
                    &assigned_map,
                    confidence_threshold,
                )
                .ok_or_else(|| format!("找不到该模型: {target_override}"))?
            };

            if !assignment.meets_threshold {
                all_meet = false;
            }
            total_cost += assignment.est_cost_usd;
            assigned_map.insert(task.id.clone(), assignment.clone());
            assignments.push(assignment);
        } else if let Some(assignment) = auto_assignment {
            if !assignment.meets_threshold {
                all_meet = false;
            }
            total_cost += assignment.est_cost_usd;
            assigned_map.insert(task.id.clone(), assignment.clone());
            assignments.push(assignment);
        }
    }

    Ok(PlanAllocationResult {
        assignments,
        total_est_cost_usd: (total_cost * 100.0).round() / 100.0,
        all_meet_threshold: all_meet,
        provider_utilizations: provider_utils,
    })
}

fn assign_single_task_internal(
    task: &TaskItem,
    pack: &EffectiveKnowledgePack,
    learned: &LearnedIndex,
    provider_utils: &mut HashMap<String, f64>,
    prev_assignments: &HashMap<String, TaskAssignment>,
    confidence_threshold: Option<f64>,
) -> Option<TaskAssignment> {
    let size_str = match task.size {
        TaskSize::S => "S",
        TaskSize::M => "M",
        TaskSize::L => "L",
    };

    let p_min = resolve_p_min(task.difficulty, &pack.success_model, confidence_threshold);

    // Profile lookup
    let default_profile = UsageProfileEntry {
        turns: match task.size {
            TaskSize::S => 1,
            TaskSize::M => 2,
            TaskSize::L => 3,
        },
        calls_per_turn: match task.size {
            TaskSize::S => 8,
            TaskSize::M => 14,
            TaskSize::L => 20,
        },
        growth_per_turn: match task.size {
            TaskSize::S => 10_000,
            TaskSize::M => 18_000,
            TaskSize::L => 25_000,
        },
        output_per_turn: match task.size {
            TaskSize::S => 3_000,
            TaskSize::M => 6_000,
            TaskSize::L => 9_000,
        },
    };

    let profile = pack
        .usage_profiles
        .get(&task.domain)
        .and_then(|sizes| sizes.get(size_str))
        .unwrap_or(&default_profile);

    // Evaluate all candidates
    let mut evaluated: Vec<(EffectiveCandidate, EvaluatedCandidate, bool)> = Vec::new();

    for cand in &pack.candidates {
        // Skill in task domain
        let skill = cand
            .skills
            .get(&task.domain)
            .copied()
            .or_else(|| cand.skills.get("general").copied())
            .unwrap_or(2.5);

        let p_success = learned.posterior_p(
            skill,
            task.difficulty,
            size_str,
            &pack.success_model,
            &cand.provider,
            &cand.model_id,
            &task.domain,
        );

        // Current cumulative provider utilization
        let current_u = provider_utils.get(&cand.provider).copied().unwrap_or(0.0);
        let scarcity = calculate_scarcity(&cand.billing, current_u);
        if scarcity.is_infinite() {
            continue; // Provider is melted / over limit
        }

        let p_in = calculate_eff_price(&cand.price_per_mtok, TokenPriceKind::Input, scarcity);
        let p_out = calculate_eff_price(&cand.price_per_mtok, TokenPriceKind::Output, scarcity);

        // Window reuse check: if any upstream dependency was executed by same provider + model
        let mut can_reuse_window = false;
        let mut _reuse_attempt_id: Option<String> = None;
        for dep_id in &task.depends_on {
            if let Some(prev) = prev_assignments.get(dep_id) {
                if prev.provider == cand.provider && prev.model_id == cand.model_id {
                    can_reuse_window = true;
                    _reuse_attempt_id = Some(format!("attempt-reuse-{}", dep_id));
                    break;
                }
            }
        }

        // Cache rates from model or provider
        let cache_rates = pack
            .models
            .get(&cand.model_id)
            .and_then(|m| m.cache_rates.as_ref());

        // Baseline tokens
        let baseline_tokens = if can_reuse_window {
            // Reusing window: baseline is warmer, initial overhead is reduced
            8_000.0
        } else {
            15_000.0
        };

        let brief_tokens = 2_500.0;
        let c_exec = calculate_c_exec(
            baseline_tokens,
            brief_tokens,
            profile,
            can_reuse_window,
            p_in,
            p_out,
            cache_rates,
        );

        let eval_cand = EvaluatedCandidate {
            provider: cand.provider.clone(),
            model_id: cand.model_id.clone(),
            display_name: cand.display_name.clone(),
            cli_model_arg: cand.cli_model_arg.clone(),
            billing: cand.billing.clone(),
            p_success,
            c_exec,
            expected_cost: c_exec, // Updated below after runner-up known
            meets_threshold: p_success >= p_min,
            origin_layer: cand.origin_layer.clone(),
        };

        evaluated.push((cand.clone(), eval_cand, can_reuse_window));
    }

    if evaluated.is_empty() {
        return None;
    }

    // Sort by c_exec to find runner-up costs
    evaluated.sort_by(|a, b| {
        a.1.c_exec
            .partial_cmp(&b.1.c_exec)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let best_c_exec = evaluated.first().map(|e| e.1.c_exec).unwrap_or(0.2);
    let second_c_exec = evaluated.get(1).map(|e| e.1.c_exec).unwrap_or(best_c_exec * 1.5);
    let bucket = format!("d{}", task.difficulty);

    // Update expected cost E(m, t) using learned fix_rate when available
    for (_, eval, _) in &mut evaluated {
        let runner_up = if (eval.c_exec - best_c_exec).abs() < 1e-4 {
            second_c_exec
        } else {
            best_c_exec
        };
        let q = learned.lookup_fix_rate(
            &eval.provider,
            &eval.model_id,
            &task.domain,
            &bucket,
        );
        eval.expected_cost = calculate_expected_cost(eval.c_exec, eval.p_success, q, Some(runner_up));
    }

    // Filter by threshold
    let mut qualified: Vec<_> = evaluated
        .iter()
        .filter(|(_, eval, _)| eval.meets_threshold)
        .cloned()
        .collect();

    let (chosen_cand, chosen_eval, reused) = if !qualified.is_empty() {
        // Pick minimum expected cost
        qualified.sort_by(|a, b| {
            a.1.expected_cost
                .partial_cmp(&b.1.expected_cost)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        qualified.remove(0)
    } else {
        // Fallback: pick highest p_success
        evaluated.sort_by(|a, b| {
            b.1.p_success
                .partial_cmp(&a.1.p_success)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        evaluated.remove(0)
    };

    // Update provider projected utilization
    // Each M sized task adds roughly ~0.04 to subscription usage
    let util_delta = match task.size {
        TaskSize::S => 0.02,
        TaskSize::M => 0.04,
        TaskSize::L => 0.07,
    };
    *provider_utils.entry(chosen_cand.provider.clone()).or_insert(0.0) += util_delta;
    let new_util = provider_utils.get(&chosen_cand.provider).copied().unwrap_or(0.0);

    // Build alternatives list
    let alternatives: Vec<AlternativeCandidate> = evaluated
        .iter()
        .take(3)
        .map(|(_, eval, _)| AlternativeCandidate {
            provider: eval.provider.clone(),
            model_id: eval.model_id.clone(),
            display_name: eval.display_name.clone(),
            p_success: (eval.p_success * 100.0).round() / 100.0,
            est_cost_usd: (eval.expected_cost * 100.0).round() / 100.0,
            meets_threshold: eval.meets_threshold,
        })
        .collect();

    // Build explanation (docs/04 §7 format)
    let cost_rounded = (chosen_eval.expected_cost * 10_000.0).round() / 10_000.0;
    let p_pct = (chosen_eval.p_success * 100.0).round() as u32;
    let _util_pct = (new_util * 100.0).round() as u32;

    let explanation = if !chosen_eval.meets_threshold {
        format!(
            "{} · 难度 {} · 规模 {}。无模型达到把握门槛 ({}%)，选择把握最高者 {} (把握 {}%, 预估 ${:.2})。",
            task.domain,
            task.difficulty,
            size_str,
            (p_min * 100.0) as u32,
            chosen_eval.display_name,
            p_pct,
            cost_rounded
        )
    } else if let Some(alt) = alternatives.first() {
        format!(
            "{} · 难度 {} · 规模 {}。{} 一次通过把握 {:.2}，预估 ${:.2}；{} 把握 {:.2}，预估 ${:.2}。选择 {}。",
            task.domain,
            task.difficulty,
            size_str,
            chosen_eval.display_name,
            chosen_eval.p_success,
            cost_rounded,
            alt.display_name,
            alt.p_success,
            alt.est_cost_usd,
            chosen_eval.display_name
        )
    } else {
        format!(
            "{} · 难度 {} · 规模 {}。{} 一次通过把握 {:.2}，预估 ${:.2}。选择 {}。",
            task.domain,
            task.difficulty,
            size_str,
            chosen_eval.display_name,
            chosen_eval.p_success,
            cost_rounded,
            chosen_eval.display_name
        )
    };

    Some(TaskAssignment {
        task_id: task.id.clone(),
        provider: chosen_cand.provider,
        model_id: chosen_cand.model_id,
        display_name: chosen_cand.display_name,
        cli_model_arg: chosen_cand.cli_model_arg,
        p_success: (chosen_eval.p_success * 100.0).round() / 100.0,
        est_cost_usd: cost_rounded,
        meets_threshold: chosen_eval.meets_threshold,
        reuse_attempt_id: if reused {
            Some(format!("attempt-reuse-{}", task.id))
        } else {
            None
        },
        explanation,
        alternatives,
    })
}

fn create_manual_assignment(
    task: &TaskItem,
    target_spec: &str, // "PROVIDER:model" or "model"
    pack: &EffectiveKnowledgePack,
    learned: &LearnedIndex,
    provider_utils: &mut HashMap<String, f64>,
    prev_assignments: &HashMap<String, TaskAssignment>,
    confidence_threshold: Option<f64>,
) -> Option<TaskAssignment> {
    let size_str = match task.size {
        TaskSize::S => "S",
        TaskSize::M => "M",
        TaskSize::L => "L",
    };

    let p_min = resolve_p_min(task.difficulty, &pack.success_model, confidence_threshold);

    // Parse target: provider:model_id or model_id or cli_model_arg
    let (target_provider, target_model) = if let Some((p, m)) = target_spec.split_once(':') {
        (Some(p), m)
    } else {
        (None, target_spec)
    };

    let cand = pack.candidates.iter().find(|c| {
        let provider_match = match target_provider {
            Some(tp) => c.provider.eq_ignore_ascii_case(tp),
            None => true,
        };
        if !provider_match {
            return false;
        }
        c.model_id == target_model
            || c.cli_model_arg == target_model
            || c.model_id.eq_ignore_ascii_case(target_model)
            || c.cli_model_arg.eq_ignore_ascii_case(target_model)
            || c.model_id.contains(target_model)
    })?;

    let skill = cand
        .skills
        .get(&task.domain)
        .copied()
        .or_else(|| cand.skills.get("general").copied())
        .unwrap_or(3.0);
    let p_success = learned.posterior_p(
        skill,
        task.difficulty,
        size_str,
        &pack.success_model,
        &cand.provider,
        &cand.model_id,
        &task.domain,
    );

    let default_profile = UsageProfileEntry {
        turns: 2,
        calls_per_turn: 14,
        growth_per_turn: 18_000,
        output_per_turn: 6_000,
    };
    let profile = pack
        .usage_profiles
        .get(&task.domain)
        .and_then(|sizes| sizes.get(size_str))
        .unwrap_or(&default_profile);

    let current_u = provider_utils.get(&cand.provider).copied().unwrap_or(0.0);
    let scarcity = calculate_scarcity(&cand.billing, current_u);
    let p_in = calculate_eff_price(&cand.price_per_mtok, TokenPriceKind::Input, scarcity);
    let p_out = calculate_eff_price(&cand.price_per_mtok, TokenPriceKind::Output, scarcity);
    let cache_rates = pack
        .models
        .get(&cand.model_id)
        .and_then(|m| m.cache_rates.as_ref());
    let c_exec = calculate_c_exec(15_000.0, 2_500.0, profile, false, p_in, p_out, cache_rates);
    let bucket = format!("d{}", task.difficulty);
    let q = learned.lookup_fix_rate(&cand.provider, &cand.model_id, &task.domain, &bucket);
    let est = calculate_expected_cost(c_exec, p_success, q, None);

    // Alternatives: other pack candidates by posterior (exclude chosen)
    let mut alt_evals: Vec<AlternativeCandidate> = pack
        .candidates
        .iter()
        .filter(|c| c.model_id != cand.model_id || c.provider != cand.provider)
        .map(|c| {
            let sk = c
                .skills
                .get(&task.domain)
                .copied()
                .or_else(|| c.skills.get("general").copied())
                .unwrap_or(2.5);
            let p = learned.posterior_p(
                sk,
                task.difficulty,
                size_str,
                &pack.success_model,
                &c.provider,
                &c.model_id,
                &task.domain,
            );
            AlternativeCandidate {
                provider: c.provider.clone(),
                model_id: c.model_id.clone(),
                display_name: c.display_name.clone(),
                p_success: (p * 100.0).round() / 100.0,
                est_cost_usd: 0.0,
                meets_threshold: p >= p_min,
            }
        })
        .collect();
    alt_evals.sort_by(|a, b| {
        b.p_success
            .partial_cmp(&a.p_success)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let alternatives: Vec<_> = alt_evals.into_iter().take(3).collect();
    let _ = prev_assignments; // reserved for reuse hints

    let util_delta = 0.04;
    *provider_utils.entry(cand.provider.clone()).or_insert(0.0) += util_delta;

    Some(TaskAssignment {
        task_id: task.id.clone(),
        provider: cand.provider.clone(),
        model_id: cand.model_id.clone(),
        display_name: cand.display_name.clone(),
        cli_model_arg: cand.cli_model_arg.clone(),
        p_success: (p_success * 100.0).round() / 100.0,
        est_cost_usd: (est * 10_000.0).round() / 10_000.0,
        meets_threshold: p_success >= p_min,
        reuse_attempt_id: None,
        explanation: format!("用户手动指定模型 {} 执行。", cand.display_name),
        alternatives,
    })
}

/// Topological sort of tasks respecting depends_on.
fn order_tasks_topologically(tasks: &[TaskItem]) -> Vec<&TaskItem> {
    let mut result = Vec::new();
    let mut visited = HashSet::new();

    fn visit<'a>(
        t: &'a TaskItem,
        all: &'a [TaskItem],
        visited: &mut HashSet<String>,
        result: &mut Vec<&'a TaskItem>,
    ) {
        if visited.contains(&t.id) {
            return;
        }
        for dep_id in &t.depends_on {
            if let Some(dep_task) = all.iter().find(|item| &item.id == dep_id) {
                visit(dep_task, all, visited, result);
            }
        }
        visited.insert(t.id.clone());
        result.push(t);
    }

    for task in tasks {
        visit(task, tasks, &mut visited, &mut result);
    }

    result
}

/// Allocates optimal models for a plan, strictly skipping any melted providers (docs/06 §6.2, P8.m2c).
pub fn allocate_plan_models_with_melted(
    plan: &Plan,
    pack: &EffectiveKnowledgePack,
    learned: &LearnedIndex,
    initial_provider_utils: Option<&HashMap<String, f64>>,
    melted_providers: &HashSet<String>,
    confidence_threshold: Option<f64>,
) -> PlanAllocationResult {
    let mut provider_utils = initial_provider_utils.cloned().unwrap_or_default();
    let mut assignments: Vec<TaskAssignment> = Vec::new();
    let mut total_cost = 0.0;
    let mut all_meet = true;

    let ordered_tasks = order_tasks_topologically(&plan.tasks);
    let mut assigned_map: HashMap<String, TaskAssignment> = HashMap::new();

    for task in ordered_tasks {
        if let Some(assignment) = assign_single_task_internal_with_melted(
            task,
            pack,
            learned,
            &mut provider_utils,
            &assigned_map,
            melted_providers,
            confidence_threshold,
        ) {
            if !assignment.meets_threshold {
                all_meet = false;
            }
            total_cost += assignment.est_cost_usd;
            assigned_map.insert(task.id.clone(), assignment.clone());
            assignments.push(assignment);
        }
    }

    PlanAllocationResult {
        assignments,
        total_est_cost_usd: (total_cost * 100.0).round() / 100.0,
        all_meet_threshold: all_meet,
        provider_utilizations: provider_utils,
    }
}

fn candidate_model_excluded(cand: &EffectiveCandidate, exclude_models: &[String]) -> bool {
    exclude_models.iter().any(|ex| {
        let ex = ex.trim();
        if ex.is_empty() {
            return false;
        }
        ex == cand.model_id
            || ex == cand.cli_model_arg
            || ex.eq_ignore_ascii_case(&format!("{}:{}", cand.provider, cand.cli_model_arg))
            || ex.eq_ignore_ascii_case(&format!("{}:{}", cand.provider, cand.model_id))
    })
}

/// Reassign a single task excluding listed models (F13.d). Returns `Err` when no candidate remains.
pub fn reassign_task_excluding(
    plan: &Plan,
    task_id: &str,
    exclude_models: &[String],
    pack: &EffectiveKnowledgePack,
    learned: &LearnedIndex,
    initial_provider_utils: Option<&HashMap<String, f64>>,
    confidence_threshold: Option<f64>,
) -> Result<TaskAssignment, String> {
    let task = plan
        .tasks
        .iter()
        .find(|t| t.id == task_id)
        .ok_or_else(|| format!("task `{task_id}` not found"))?;
    let mut filtered = pack.clone();
    filtered
        .candidates
        .retain(|c| !candidate_model_excluded(c, exclude_models));
    if filtered.candidates.is_empty() {
        return Err(format!(
            "no qualified candidate for `{task_id}` after excluding {exclude_models:?}"
        ));
    }
    let mut utils = initial_provider_utils.cloned().unwrap_or_default();
    let prev = HashMap::new();
    assign_single_task_internal(
        task,
        &filtered,
        learned,
        &mut utils,
        &prev,
        confidence_threshold,
    )
    .ok_or_else(|| {
        format!("no qualified candidate for `{task_id}` after excluding {exclude_models:?}")
    })
}

/// Automatically reassigns unstarted / pending tasks when a provider melts (docs/06 §6.2, P8.m2c).
pub fn reassign_unstarted_tasks(
    plan: &Plan,
    current_assignments: &[TaskAssignment],
    completed_or_running_task_ids: &HashSet<String>,
    melted_providers: &HashSet<String>,
    pack: &EffectiveKnowledgePack,
    learned: &LearnedIndex,
    initial_provider_utils: Option<&HashMap<String, f64>>,
    confidence_threshold: Option<f64>,
) -> PlanAllocationResult {
    let mut provider_utils = initial_provider_utils.cloned().unwrap_or_default();
    let mut assignments: Vec<TaskAssignment> = Vec::new();
    let mut total_cost = 0.0;
    let mut all_meet = true;

    let ordered_tasks = order_tasks_topologically(&plan.tasks);
    let mut assigned_map: HashMap<String, TaskAssignment> = HashMap::new();

    for task in ordered_tasks {
        if completed_or_running_task_ids.contains(&task.id) {
            // Keep existing assignment for already running/completed tasks
            if let Some(existing) = current_assignments.iter().find(|a| a.task_id == task.id) {
                if !existing.meets_threshold {
                    all_meet = false;
                }
                total_cost += existing.est_cost_usd;
                assigned_map.insert(task.id.clone(), existing.clone());
                assignments.push(existing.clone());
            }
        } else {
            // Reassign unstarted task, filtering out melted providers
            if let Some(assignment) = assign_single_task_internal_with_melted(
                task,
                pack,
                learned,
                &mut provider_utils,
                &assigned_map,
                melted_providers,
                confidence_threshold,
            ) {
                if !assignment.meets_threshold {
                    all_meet = false;
                }
                total_cost += assignment.est_cost_usd;
                assigned_map.insert(task.id.clone(), assignment.clone());
                assignments.push(assignment);
            }
        }
    }

    PlanAllocationResult {
        assignments,
        total_est_cost_usd: (total_cost * 100.0).round() / 100.0,
        all_meet_threshold: all_meet,
        provider_utilizations: provider_utils,
    }
}

fn assign_single_task_internal_with_melted(
    task: &TaskItem,
    pack: &EffectiveKnowledgePack,
    learned: &LearnedIndex,
    provider_utils: &mut HashMap<String, f64>,
    prev_assignments: &HashMap<String, TaskAssignment>,
    melted_providers: &HashSet<String>,
    confidence_threshold: Option<f64>,
) -> Option<TaskAssignment> {
    let mut filtered = pack.clone();
    filtered
        .candidates
        .retain(|c| !melted_providers.contains(&c.provider));
    assign_single_task_internal(
        task,
        &filtered,
        learned,
        provider_utils,
        prev_assignments,
        confidence_threshold,
    )
}

/// Determines the reference model for savings calculations (docs/06 §5.2, P8.m3a).
///
/// If `user_override` is specified and exists in candidates, uses that.
/// Otherwise, selects the candidate with planner role and highest planning skill;
/// defaults to "claude-3-7-sonnet" or the first candidate.
pub fn determine_reference_model(
    pack: &EffectiveKnowledgePack,
    user_override: Option<&str>,
) -> String {
    if let Some(override_model) = user_override {
        let clean = override_model.trim();
        if !clean.is_empty() {
            if pack.candidates.iter().any(|c| c.model_id == clean || c.display_name == clean) {
                return clean.to_string();
            }
        }
    }

    if let Some(top_planner) = select_planning_model(pack, 2) {
        return top_planner.model_id;
    }

    pack.candidates
        .first()
        .map(|c| c.model_id.clone())
        .unwrap_or_else(|| "claude-3-7-sonnet".to_string())
}

/// Calculates the counterfactual baseline cost for a task executed by the reference model (docs/06 §5.2).
pub fn calculate_task_counterfactual_cost(
    task: &TaskItem,
    reference_model_id: &str,
    pack: &EffectiveKnowledgePack,
) -> f64 {
    let size_str = match task.size {
        TaskSize::S => "S",
        TaskSize::M => "M",
        TaskSize::L => "L",
    };

    let default_profile = UsageProfileEntry {
        turns: match task.size {
            TaskSize::S => 1,
            TaskSize::M => 2,
            TaskSize::L => 3,
        },
        calls_per_turn: match task.size {
            TaskSize::S => 8,
            TaskSize::M => 14,
            TaskSize::L => 20,
        },
        growth_per_turn: match task.size {
            TaskSize::S => 10_000,
            TaskSize::M => 18_000,
            TaskSize::L => 25_000,
        },
        output_per_turn: match task.size {
            TaskSize::S => 3_000,
            TaskSize::M => 6_000,
            TaskSize::L => 9_000,
        },
    };

    let profile = pack
        .usage_profiles
        .get(&task.domain)
        .and_then(|sizes| sizes.get(size_str))
        .unwrap_or(&default_profile);

    let cand = pack
        .candidates
        .iter()
        .find(|c| c.model_id == reference_model_id)
        .or_else(|| pack.candidates.first());
    let (p_in, p_out) = match cand {
        Some(c) => (c.price_per_mtok.input, c.price_per_mtok.output),
        None => (3.0, 15.0),
    };
    let cache_rates = pack
        .models
        .get(reference_model_id)
        .and_then(|m| m.cache_rates.as_ref());

    calculate_c_exec(2000.0, 1500.0, profile, false, p_in, p_out, cache_rates)
}

/// Calculates the counterfactual baseline and savings for all tasks in a plan when executed by the reference model.
pub fn calculate_plan_assignment_savings(
    tasks: &[TaskItem],
    actual_plan_cost: f64,
    reference_model_id: &str,
    pack: &EffectiveKnowledgePack,
) -> (f64, f64, f64) {
    let baseline: f64 = tasks
        .iter()
        .map(|t| calculate_task_counterfactual_cost(t, reference_model_id, pack))
        .sum();
    let saved = (baseline - actual_plan_cost).max(0.0);
    (baseline, actual_plan_cost, saved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LearnedIndex;
    use tokage_core::planning::{Plan, PlanSpec, TaskItem, TaskSize};
    use crate::pack::builtin_knowledge_pack;
    use crate::merge::{merge_layers, LayeredPacks};

    fn get_test_effective_pack() -> EffectiveKnowledgePack {
        let builtin = builtin_knowledge_pack().clone();
        let layers = LayeredPacks {
            builtin,
            official: None,
            customs: vec![],
            overrides: vec![],
            learned_usage_profiles: None,
        };
        merge_layers(&layers).expect("builtin pack must merge cleanly")
    }

    #[test]
    fn test_select_planning_model_ranks_by_p_success() {
        let pack = get_test_effective_pack();
        let planner = select_planning_model(&pack, 2).expect("Planner model must be found");

        // Planner candidate must have planner role and planning skill >= 5.0
        let cand = pack
            .candidates
            .iter()
            .find(|c| c.model_id == planner.model_id)
            .expect("Planner exists");
        assert!(cand.roles.iter().any(|r| r.eq_ignore_ascii_case("planner")));
        assert!(planner.p_success >= 0.80);
    }

    #[test]
    fn test_direct_single_task_assignment() {
        let pack = get_test_effective_pack();
        let assignment = assign_direct_task("frontend-ui", 1, Some(TaskSize::S), &pack, &LearnedIndex::empty(), None, None)
            .expect("Direct task assigned");

        assert_eq!(assignment.task_id, "DIRECT");
        assert!(assignment.p_success >= 0.80);
        assert!(assignment.est_cost_usd > 0.0);
        assert!(!assignment.explanation.is_empty());
    }

    #[test]
    fn test_global_plan_allocation_and_explanation() {
        let pack = get_test_effective_pack();

        let tasks = vec![
            TaskItem {
                id: "T1".into(),
                domain: "backend-api".into(),
                title: "User Auth".into(),
                goal: "Implement token auth".into(),
                depends_on: vec![],
                contracts: vec![],
                write_scope: vec!["src/auth/**".into()],
                deliverables: vec![],
                acceptance: Default::default(),
                size: TaskSize::M,
                difficulty: 2,
                assigned_model: None,
            },
            TaskItem {
                id: "T2".into(),
                domain: "frontend-ui".into(),
                title: "Login Screen".into(),
                goal: "Build login modal".into(),
                depends_on: vec!["T1".into()],
                contracts: vec![],
                write_scope: vec!["src/ui/**".into()],
                deliverables: vec![],
                acceptance: Default::default(),
                size: TaskSize::S,
                difficulty: 1,
                assigned_model: None,
            },
        ];

        let plan = Plan {
            version: 1,
            parent_plan_id: None,
            spec: PlanSpec::default(),
            contracts: vec![],
            tasks,
            notes: None,
            raw_markdown: String::new(),
            is_user_edited: Some(false),
        };

        let result = allocate_plan_models(&plan, &pack, &LearnedIndex::empty(), None, None);
        assert_eq!(result.assignments.len(), 2);
        assert!(result.total_est_cost_usd > 0.0);

        // Explanations must be non-empty and formatted
        for assign in &result.assignments {
            assert!(assign.explanation.contains("把握"));
            assert!(!assign.display_name.is_empty());
        }
    }

    #[test]
    fn test_reassignment_recomputes_rest_of_plan() {
        let pack = get_test_effective_pack();

        let tasks = vec![
            TaskItem {
                id: "T1".into(),
                domain: "backend-api".into(),
                title: "Backend Service".into(),
                goal: "Service logic".into(),
                depends_on: vec![],
                contracts: vec![],
                write_scope: vec![],
                deliverables: vec![],
                acceptance: Default::default(),
                size: TaskSize::L,
                difficulty: 3,
                assigned_model: None,
            },
            TaskItem {
                id: "T2".into(),
                domain: "testing".into(),
                title: "Unit Tests".into(),
                goal: "Test cases".into(),
                depends_on: vec!["T1".into()],
                contracts: vec![],
                write_scope: vec![],
                deliverables: vec![],
                acceptance: Default::default(),
                size: TaskSize::M,
                difficulty: 2,
                assigned_model: None,
            },
        ];

        let plan = Plan {
            version: 1,
            parent_plan_id: None,
            spec: PlanSpec::default(),
            contracts: vec![],
            tasks,
            notes: None,
            raw_markdown: String::new(),
            is_user_edited: Some(false),
        };

        let mut manual_overrides = HashMap::new();
        manual_overrides.insert("T1".into(), "CLAUDE_CODE:claude-opus-5-5".into());

        let result = recompute_plan_allocation_with_overrides(&plan, &pack, &LearnedIndex::empty(), &manual_overrides, None, None).unwrap();
        assert_eq!(result.assignments[0].model_id, "claude-opus-5-5");
        // Utilization should have increased for CLAUDE_CODE
        assert!(result.provider_utilizations.get("CLAUDE_CODE").unwrap() > &0.0);
    }

    #[test]
    fn test_manual_assignment_opus_high_confidence() {
        let pack = get_test_effective_pack();

        let tasks = vec![TaskItem {
            id: "T1".into(),
            domain: "database".into(), // non-standard domain, falls back to general
            title: "Database Migration".into(),
            goal: "Run migrations".into(),
            depends_on: vec![],
            contracts: vec![],
            write_scope: vec![],
            deliverables: vec![],
            acceptance: Default::default(),
            size: TaskSize::L,
            difficulty: 3,
            assigned_model: None,
        }];

        let plan = Plan {
            version: 1,
            parent_plan_id: None,
            spec: PlanSpec::default(),
            contracts: vec![],
            tasks,
            notes: None,
            raw_markdown: String::new(),
            is_user_edited: Some(false),
        };

        let mut manual_overrides = HashMap::new();
        // User selects opus via CLI alias "CLAUDE_CODE:opus"
        manual_overrides.insert("T1".into(), "CLAUDE_CODE:opus".into());

        let result = recompute_plan_allocation_with_overrides(&plan, &pack, &LearnedIndex::empty(), &manual_overrides, None, None).unwrap();
        assert_eq!(result.assignments[0].model_id, "claude-opus-5-5");
        // Prior confidence for Opus on diff 3 L should be >= 0.85, not 25%!
        assert!(result.assignments[0].p_success >= 0.85, "p_success was {}", result.assignments[0].p_success);
        assert!(result.assignments[0].meets_threshold);
    }

    #[test]
    fn test_reference_model_and_plan_savings() {
        let pack = get_test_effective_pack();
        let ref_model = determine_reference_model(&pack, None);
        assert!(!ref_model.is_empty());

        let tasks = vec![
            TaskItem {
                id: "T1".into(),
                domain: "architecture".into(),
                title: "Arch design".into(),
                goal: "Core architecture".into(),
                depends_on: vec![],
                contracts: vec![],
                write_scope: vec![],
                deliverables: vec![],
                acceptance: Default::default(),
                size: TaskSize::L,
                difficulty: 3,
                assigned_model: None,
            },
            TaskItem {
                id: "T2".into(),
                domain: "testing".into(),
                title: "Unit tests".into(),
                goal: "Tests".into(),
                depends_on: vec!["T1".into()],
                contracts: vec![],
                write_scope: vec![],
                deliverables: vec![],
                acceptance: Default::default(),
                size: TaskSize::S,
                difficulty: 1,
                assigned_model: None,
            },
        ];

        let (baseline, actual, saved) = calculate_plan_assignment_savings(&tasks, 0.40, &ref_model, &pack);
        assert!(baseline > 0.0, "Baseline should be positive");
        assert_eq!(actual, 0.40);
        assert!(saved >= 0.0);
    }

    #[test]
    fn f13d_reassign_task_excluding_skips_excluded_and_errors_when_empty() {
        let pack = get_test_effective_pack();
        let plan = Plan {
            version: 1,
            parent_plan_id: None,
            spec: PlanSpec {
                raw: "".into(),
                goal: "g".into(),
                in_scope: vec![],
                out_of_scope: vec![],
                decisions: vec![],
            },
            contracts: vec![],
            tasks: vec![TaskItem {
                id: "T1".into(),
                domain: "backend".into(),
                title: "t".into(),
                goal: "g".into(),
                depends_on: vec![],
                contracts: vec![],
                write_scope: vec![],
                deliverables: vec![],
                acceptance: Default::default(),
                size: TaskSize::S,
                difficulty: 1,
                assigned_model: None,
            }],
            notes: None,
            raw_markdown: "".into(),
            is_user_edited: None,
        };
        let first = reassign_task_excluding(&plan, "T1", &[], &pack, &LearnedIndex::empty(), None, None).unwrap();
        let exclude = vec![
            format!("{}:{}", first.provider, first.cli_model_arg),
            first.model_id.clone(),
        ];
        let second = reassign_task_excluding(&plan, "T1", &exclude, &pack, &LearnedIndex::empty(), None, None);
        if let Ok(a) = &second {
            assert_ne!(a.model_id, first.model_id);
        }
        let all: Vec<String> = pack
            .candidates
            .iter()
            .map(|c| format!("{}:{}", c.provider, c.cli_model_arg))
            .collect();
        assert!(reassign_task_excluding(&plan, "T1", &all, &pack, &LearnedIndex::empty(), None, None).is_err());
    }

    #[test]
    fn f14a_customs_and_overrides_change_allocation() {
        let builtin = builtin_knowledge_pack().clone();
        let base = get_test_effective_pack();
        let task = TaskItem {
            id: "T1".into(),
            domain: "testing".into(),
            title: "t".into(),
            goal: "g".into(),
            depends_on: vec![],
            contracts: vec![],
            write_scope: vec![],
            deliverables: vec![],
            acceptance: Default::default(),
            size: TaskSize::S,
            difficulty: 1,
            assigned_model: None,
        };
        let plan = Plan {
            version: 1,
            parent_plan_id: None,
            spec: PlanSpec::default(),
            contracts: vec![],
            tasks: vec![task],
            notes: None,
            raw_markdown: String::new(),
            is_user_edited: Some(false),
        };
        let before = allocate_plan_models(&plan, &base, &LearnedIndex::empty(), None, None);
        let before_id = before.assignments[0].model_id.clone();

        // Downgrade the winner's testing skill via override so another candidate is chosen
        let layers = LayeredPacks {
            builtin,
            official: None,
            customs: vec![],
            overrides: vec![crate::merge::ModelOverride {
                provider_name: "*".into(),
                model_id: before_id.clone(),
                patch: serde_json::json!({ "skills": { "testing": 1.0 } }),
            }],
            learned_usage_profiles: None,
        };
        let with_ov = merge_layers(&layers).expect("merge");
        assert!(
            with_ov
                .models
                .get(&before_id)
                .and_then(|m| m.skills.get("testing"))
                .copied()
                .unwrap_or(5.0)
                <= 1.0 + f64::EPSILON
        );
        let after = allocate_plan_models(&plan, &with_ov, &LearnedIndex::empty(), None, None);
        assert!(
            !after.assignments.is_empty(),
            "allocation must still produce a pick"
        );
        // With multiple candidates, the downgraded winner should lose; if alone, skill patch still applied above.
        if base.candidates.iter().filter(|c| c.model_id != before_id).count() > 0 {
            assert_ne!(
                after.assignments[0].model_id, before_id,
                "override should change allocation when alternatives exist"
            );
        }
    }

    #[test]
    fn f14e_manual_unknown_model_returns_none() {
        let pack = get_test_effective_pack();
        let task = TaskItem {
            id: "T1".into(),
            domain: "backend".into(),
            title: "t".into(),
            goal: "g".into(),
            depends_on: vec![],
            contracts: vec![],
            write_scope: vec![],
            deliverables: vec![],
            acceptance: Default::default(),
            size: TaskSize::S,
            difficulty: 1,
            assigned_model: None,
        };
        let mut utils = HashMap::new();
        let prev = HashMap::new();
        let none = create_manual_assignment(
            &task,
            "UNKNOWN_PROVIDER:not-a-real-model",
            &pack,
            &LearnedIndex::empty(),
            &mut utils,
            &prev,
            None,
        );
        assert!(none.is_none());
    }
}
