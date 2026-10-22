//! Deterministic DAG scheduling, durable reservations and bounded repair policy.
use crate::*;
use std::collections::{HashMap, HashSet};
use tokage_orchestration::{ConcurrencyLimits, DagScheduler, RepairStep, TaskState};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskRun {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manual: Option<crate::manual::Input>,
    pub state: TaskState,
    pub assignment: ModelAssignment,
    pub attempts: Vec<String>,
    pub repair_step: RepairStep,
    pub reservation_active: bool,
    pub reserved_usd: Option<f64>,
    pub error: Option<String>,
    pub deviation: bool,
    pub context: Option<ContextDecision>,
    pub continuation_of: Option<String>,
    pub session_allowed: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderHealth {
    pub failures: u32,
    pub melted_until: Option<u64>,
    pub reason: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Execution {
    pub sequence: u64,
    pub tasks: BTreeMap<String, TaskRun>,
    pub health: BTreeMap<String, ProviderHealth>,
    pub claimed: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Reservation {
    pub task_id: String,
    pub attempt_id: String,
    pub assignment: ModelAssignment,
    pub estimated_usd: Option<f64>,
    pub resume_turn: Option<String>,
}
impl Execution {
    pub(crate) fn new(s: &Snapshot) -> Result<Self> {
        let plan = s.plan.as_ref().ok_or(Error::Transition)?;
        Ok(Self {
            sequence: 0,
            tasks: plan
                .assignments
                .iter()
                .map(|(id, m)| {
                    (
                        id.clone(),
                        TaskRun {
                            manual: None,
                            state: TaskState::Pending,
                            assignment: m.clone(),
                            attempts: vec![],
                            repair_step: RepairStep::Initial,
                            reservation_active: false,
                            reserved_usd: None,
                            error: None,
                            deviation: false,
                            context: None,
                            continuation_of: None,
                            session_allowed: vec![],
                        },
                    )
                })
                .collect(),
            health: BTreeMap::new(),
            claimed: false,
        })
    }
}
pub fn reserve_ready(s: &mut Snapshot, now: u64) -> Result<Vec<Reservation>> {
    let mut next = s.clone();
    let reserved = reserve_inner(&mut next, now)?;
    *s = next;
    Ok(reserved)
}
fn reserve_inner(s: &mut Snapshot, now: u64) -> Result<Vec<Reservation>> {
    if s.state != RunState::Executing {
        return Err(Error::Transition);
    }
    let p = s.plan.as_ref().ok_or(Error::Transition)?;
    let claimed = s
        .execution
        .as_ref()
        .ok_or(Error::Transition)?
        .tasks
        .values()
        .filter(|t| t.reservation_active)
        .filter_map(|t| t.context.as_ref().and_then(|c| c.previous_turn.clone()))
        .collect::<HashSet<_>>();
    let contexts = s
        .execution
        .as_ref()
        .unwrap()
        .tasks
        .iter()
        .map(|(id, t)| {
            Ok((
                id.clone(),
                choose_context(s, id, &t.assignment, now, &claimed)?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let ex = s.execution.as_mut().ok_or(Error::Transition)?;
    for task in ex.tasks.values_mut() {
        if task.state == TaskState::PausedQuota
            && ex
                .health
                .get(&task.assignment.provider)
                .is_some_and(|h| h.melted_until.is_some_and(|until| until <= now))
        {
            task.state = TaskState::Pending;
        }
    }
    let states = ex
        .tasks
        .iter()
        .map(|(id, t)| {
            let state = if !t.reservation_active
                && ex
                    .health
                    .get(&t.assignment.provider)
                    .is_some_and(|h| h.melted_until.is_some_and(|until| until > now))
            {
                TaskState::PausedQuota
            } else {
                t.state
            };
            (id.clone(), state)
        })
        .collect::<HashMap<_, _>>();
    let assignments = ex
        .tasks
        .iter()
        .map(|(id, t)| {
            (
                id.clone(),
                (
                    t.assignment.provider.clone(),
                    t.assignment.requested_model.clone(),
                ),
            )
        })
        .collect::<HashMap<_, _>>();
    let pack = planning::effective_pack(&s.config)?;
    let mut provider_overrides: HashMap<String, usize> = pack
        .providers
        .iter()
        .filter_map(|(id, p)| p.concurrency_hint.map(|n| (id.clone(), n.max(1) as usize)))
        .collect();
    provider_overrides.extend(
        s.config
            .provider_concurrency
            .iter()
            .map(|(p, n)| (p.clone(), *n as usize)),
    );
    let deviations = ex
        .tasks
        .iter()
        .filter(|(_, t)| t.deviation)
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    let scheduler = DagScheduler::from_states(
        &p.plan.tasks,
        &states,
        &assignments,
        ConcurrencyLimits {
            max_global_concurrent: s.config.concurrency as usize,
            default_provider_concurrent: 2,
            provider_overrides,
        },
        &deviations,
    );
    let unknown = s.turns.values().any(|t| t.ended && t.cost_usd.is_none());
    if unknown && s.config.block_unknown_cost {
        return Err(Error::Invalid(
            "unknown actual turn cost blocks dispatch".into(),
        ));
    }
    let spent = s.turns.values().filter_map(|t| t.cost_usd).sum::<f64>();
    let mut reserved = ex
        .tasks
        .values()
        .filter(|t| t.reservation_active)
        .filter_map(|t| t.reserved_usd)
        .sum::<f64>();
    let mut result = Vec::new();
    let mut used_windows = claimed;
    let mut probing = ex
        .tasks
        .values()
        .filter(|t| t.reservation_active)
        .map(|t| t.assignment.provider.clone())
        .collect::<HashSet<_>>();
    for id in scheduler.pick().dispatchable {
        let t = ex.tasks.get_mut(&id).ok_or(Error::Transition)?;
        if t.manual.is_none()
            && ex
                .health
                .get(&t.assignment.provider)
                .is_some_and(|h| h.melted_until.is_some_and(|until| until > now))
        {
            continue;
        }
        let half_open = t.manual.is_none()
            && ex
                .health
                .get(&t.assignment.provider)
                .is_some_and(|h| h.melted_until.is_some_and(|until| until <= now));
        if half_open && !probing.insert(t.assignment.provider.clone()) {
            continue;
        }
        let estimate = if t.manual.is_some() {
            Some(0.0)
        } else {
            estimate(&p.plan, &id, &t.assignment, &s.config)?
        };
        if estimate.is_none() && s.config.block_unknown_cost {
            return Err(Error::Invalid(
                "unknown estimated cost blocks dispatch".into(),
            ));
        }
        if s.config
            .budget_usd
            .is_some_and(|budget| spent + reserved + estimate.unwrap_or(0.0) > budget)
        {
            continue;
        }
        if t.reservation_active {
            return Err(Error::Invalid("duplicate task reservation".into()));
        }
        ex.sequence = ex.sequence.checked_add(1).ok_or(Error::RevisionConflict)?;
        let attempt_id = digest(&(&s.run_id, &id, ex.sequence, &s.config_hash))?;
        let mut context = contexts.get(&id).cloned().ok_or(Error::Transition)?;
        if context
            .previous_turn
            .as_ref()
            .is_some_and(|id| !used_windows.insert(id.clone()))
        {
            context.reuse = false;
            context.reason = "window already reserved".into();
        }
        let resume_turn = if t.continuation_of.is_some() {
            t.continuation_of.take()
        } else if t.repair_step == RepairStep::InWindowRepair {
            t.attempts.last().cloned()
        } else if context.reuse {
            context.previous_turn.clone()
        } else {
            None
        };
        t.context = Some(context);
        t.attempts.push(attempt_id.clone());
        t.state = TaskState::Running;
        t.reservation_active = true;
        t.reserved_usd = estimate;
        reserved += estimate.unwrap_or(0.0);
        result.push(Reservation {
            task_id: id,
            attempt_id,
            assignment: t.assignment.clone(),
            estimated_usd: estimate,
            resume_turn,
        });
    }
    Ok(result)
}
pub fn estimate(
    plan: &tokage_core::planning::Plan,
    task_id: &str,
    m: &ModelAssignment,
    config: &EffectiveRunConfig,
) -> Result<Option<f64>> {
    if m.model_params
        .as_ref()
        .is_some_and(|p| p.fast || p.tier.is_some())
    {
        return Ok(None);
    }
    let pack = planning::effective_pack(config)?;
    let known = pack.candidates.iter().any(|c| {
        c.provider == m.provider
            && tokage_matching::assignment::id_matches(&m.requested_model, &c.cli_model_arg)
    });
    if !known {
        return Ok(None);
    }
    let mut pack = pack;
    pack.candidates
        .sort_by(|a, b| (&a.provider, &a.cli_model_arg).cmp(&(&b.provider, &b.cli_model_arg)));
    let result = tokage_matching::assignment::recompute_plan_allocation_with_overrides(
        plan,
        &pack,
        &planning::learned_index(config)?,
        &HashMap::from([(
            task_id.to_owned(),
            format!("{}:{}", m.provider, m.requested_model),
        )]),
        None,
        None,
    )
    .map_err(Error::Invalid)?;
    Ok(result
        .assignments
        .iter()
        .find(|a| a.task_id == task_id)
        .map(|a| a.est_cost_usd)
        .filter(|n| n.is_finite() && *n >= 0.0))
}
/// Failure handling is service-owned. It never marks Done and never rewrites the approved plan.
pub fn fail_task(s: &mut Snapshot, task_id: &str, error: String, now: u64) -> Result<()> {
    let p = s.plan.as_ref().ok_or(Error::Transition)?;
    let task = p
        .plan
        .tasks
        .iter()
        .find(|t| t.id == task_id)
        .ok_or(Error::Transition)?;
    let ex = s.execution.as_mut().ok_or(Error::Transition)?;
    let t = ex.tasks.get_mut(task_id).ok_or(Error::Transition)?;
    t.reservation_active = false;
    t.reserved_usd = None;
    t.error = Some(error.clone());
    let provider = serde_json::from_value(serde_json::json!(t.assignment.provider)).ok();
    let kind = tokage_native::chat::error::classify_error(provider, &error, None);
    let health = ex.health.entry(t.assignment.provider.clone()).or_default();
    let provider_failed = t
        .attempts
        .last()
        .and_then(|id| s.turns.get(id))
        .is_some_and(|r| r.status == turn::TurnStatus::Failed);
    if provider_failed {
        health.failures = health.failures.saturating_add(1);
    }
    if provider_failed
        && (matches!(
            kind,
            tokage_core::TurnErrorKind::AuthRequired | tokage_core::TurnErrorKind::RateLimited
        ) || health.failures >= 3)
    {
        health.melted_until = Some(now.saturating_add(3600));
        health.reason = Some(error.clone());
    }
    let git_pending = t
        .attempts
        .last()
        .and_then(|id| s.work.as_ref()?.attempts.get(id))
        .is_some_and(|a| a.commit_intent || a.integration_before.is_some());
    if t.manual.is_some()
        || git_pending
        || s.config.repair_policy == "manual"
        || t.deviation
        || matches!(s.state, RunState::CancelRequested | RunState::Stopping)
    {
        t.state = TaskState::Blocked;
        return Ok(());
    }
    let evidence = tokage_orchestration::FailureEvidence {
        failed_command: None,
        output_tail: error,
        out_of_scope_files: vec![],
        previous_changed_files: vec![],
    };
    let (step, decision) = tokage_orchestration::decide_repair_action_with_melted(
        task,
        t.repair_step,
        t.attempts.last().map(String::as_str).unwrap_or(""),
        &t.assignment.requested_model,
        &t.assignment.provider,
        health.melted_until.is_some_and(|until| until > now),
        &evidence,
    );
    t.repair_step = step;
    match decision {
        tokage_orchestration::RepairDecision::TryInWindowRepair { .. } => {
            let last = t.attempts.last().and_then(|id| s.turns.get(id));
            if last.is_some_and(|turn| {
                turn.ended && turn.cleanup_confirmed && turn.thread_id.is_some()
            }) {
                t.state = TaskState::Pending;
            } else {
                t.state = TaskState::Blocked;
            }
        }
        tokage_orchestration::RepairDecision::TryReassign { .. } => {
            if t.assignment.user_locked {
                t.state = TaskState::Blocked;
                return Ok(());
            }
            let mut pack = planning::effective_pack(&s.config)?;
            let available = s
                .config
                .models
                .values()
                .map(|m| tokage_matching::assignment::AvailableModel {
                    provider: m.provider.clone(),
                    cli_model_arg: m.requested_model.clone(),
                })
                .collect::<Vec<_>>();
            let melted = ex
                .health
                .iter()
                .filter(|(_, h)| h.melted_until.is_some_and(|until| until > now))
                .map(|(p, _)| p.clone())
                .collect::<HashSet<_>>();
            pack = tokage_matching::assignment::restrict_pack(&pack, Some(&available), &melted);
            pack.candidates.sort_by(|a, b| {
                (&a.provider, &a.cli_model_arg).cmp(&(&b.provider, &b.cli_model_arg))
            });
            let excluded = t
                .attempts
                .iter()
                .filter_map(|id| s.turns.get(id))
                .map(|r| format!("{}:{}", r.requested.provider, r.requested.requested_model))
                .collect::<Vec<_>>();
            let next = tokage_matching::assignment::reassign_task_excluding(
                &p.plan,
                task_id,
                &excluded,
                &pack,
                &planning::learned_index(&s.config)?,
                None,
                None,
            )
            .ok();
            if let Some(model) = next
                .and_then(|a| {
                    s.config
                        .models
                        .values()
                        .find(|m| m.provider == a.provider && m.requested_model == a.cli_model_arg)
                })
                .cloned()
            {
                t.assignment = model;
                t.state = TaskState::Pending;
                t.session_allowed.clear();
                t.continuation_of = None;
            } else {
                t.state = TaskState::Blocked;
            }
        }
        _ => t.state = TaskState::Blocked,
    }
    Ok(())
}
pub(crate) fn validate_snapshot(s: &Snapshot) -> Result<()> {
    let Some(ex) = &s.execution else {
        return Ok(());
    };
    let plan = s.plan.as_ref().ok_or(Error::Transition)?;
    if ex.tasks.len() != plan.plan.tasks.len()
        || ex.tasks.keys().any(|id| !plan.assignments.contains_key(id))
    {
        return Err(Error::Invalid("execution/plan mismatch".into()));
    }
    if ex.tasks.values().any(|t| {
        t.reserved_usd.is_some_and(|n| !n.is_finite() || n < 0.0)
            || t.reservation_active && !t.state.is_active()
    }) {
        return Err(Error::Invalid("reservation mismatch".into()));
    }
    Ok(())
}
pub(crate) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContextDecision {
    pub previous_turn: Option<String>,
    pub reuse: bool,
    pub reason: String,
    pub inputs: Option<serde_json::Value>,
}
fn choose_context(
    s: &Snapshot,
    task_id: &str,
    assignment: &ModelAssignment,
    now: u64,
    claimed: &HashSet<String>,
) -> Result<ContextDecision> {
    let fresh = |reason: &str| ContextDecision {
        previous_turn: None,
        reuse: false,
        reason: reason.into(),
        inputs: None,
    };
    if s.config.context_policy == "off" {
        return Ok(fresh("context optimization disabled"));
    }
    let p = s.plan.as_ref().ok_or(Error::Transition)?;
    let ex = s.execution.as_ref().ok_or(Error::Transition)?;
    let task = p
        .plan
        .tasks
        .iter()
        .find(|t| t.id == task_id)
        .ok_or(Error::Transition)?;
    let pack = planning::effective_pack(&s.config)?;
    let model = pack.candidates.iter().find(|c| {
        c.provider == assignment.provider
            && tokage_matching::assignment::id_matches(
                &assignment.requested_model,
                &c.cli_model_arg,
            )
    });
    let Some(model) = model else {
        return Ok(fresh("model context/price unknown"));
    };
    let provider = serde_json::from_value(serde_json::json!(assignment.provider)).ok();
    if !provider
        .and_then(tokage_native::chat::adapter::adapter)
        .is_some_and(|a| {
            a.meta()
                .caps
                .has(tokage_native::chat::adapter::Caps::RESUME)
        })
    {
        return Ok(fresh("adapter cannot resume"));
    }
    let candidate = ex
        .tasks
        .iter()
        .filter(|(_, t)| t.state == TaskState::Done)
        .filter_map(|(id, t)| {
            let prev = t.attempts.last().and_then(|id| s.turns.get(id))?;
            let old = p.plan.tasks.iter().find(|task| task.id == *id)?;
            if !t.session_allowed.is_empty()
                || prev.requested != *assignment
                || prev.thread_id.is_none()
                || !prev.cleanup_confirmed
                || claimed.contains(&prev.id)
            {
                return None;
            }
            Some((old, prev))
        })
        .max_by_key(|(_, t)| (t.finished_at.unwrap_or(0), t.id.clone()));
    let Some((old, previous)) = candidate else {
        return Ok(fresh("no idle compatible window"));
    };
    let Some(usage) = &previous.usage else {
        return Ok(fresh("previous context usage unknown"));
    };
    let cache = pack
        .providers
        .get(&assignment.provider)
        .and_then(|p| p.cache.as_ref());
    let params = tokage_context::reuse::WindowReuseParams {
        model_a: previous.requested.requested_model.clone(),
        model_b: assignment.requested_model.clone(),
        role_a: previous.role.clone(),
        role_b: "executor".into(),
        domain_a: old.domain.clone(),
        domain_b: task.domain.clone(),
        write_scope_a: old.write_scope.clone(),
        write_scope_b: task.write_scope.clone(),
        history_tokens_a: usage.context_tokens.unwrap_or(
            usage
                .input_tokens
                .saturating_add(usage.cache_read_tokens)
                .saturating_add(usage.output_tokens),
        ) as f64,
        elapsed_seconds: now.saturating_sub(previous.finished_at.unwrap_or(0)) as f64,
        cache_ttl_seconds: cache.and_then(|c| c.ttl_seconds).unwrap_or(0) as f64,
        effective_context: model.effective_context as f64,
        context_window: model.context_window as f64,
        p_in: model.price_per_mtok.input,
        p_out: model.price_per_mtok.output,
        cache_read_multiplier: cache.and_then(|c| c.read_multiplier).unwrap_or(1.0),
        cache_write_multiplier: cache.and_then(|c| c.write_multiplier).unwrap_or(1.0),
        ..Default::default()
    };
    let decision = tokage_context::reuse::decide_window_reuse(&params);
    Ok(ContextDecision {
        previous_turn: Some(previous.id.clone()),
        reuse: decision.can_reuse && s.config.context_policy == "auto",
        reason: decision.reason,
        inputs: Some(serde_json::to_value(params).map_err(|e| Error::Invalid(e.to_string()))?),
    })
}
pub(crate) fn role_budget(
    s: &Snapshot,
    role: &str,
    selected: &ModelAssignment,
) -> Result<Option<f64>> {
    if s.config.block_unknown_cost && s.turns.values().any(|t| t.ended && t.cost_usd.is_none()) {
        return Err(Error::Invalid(
            "unknown actual cost blocks next role".into(),
        ));
    }
    let task = tokage_core::planning::TaskItem {
        id: role.into(),
        domain: role.into(),
        title: role.into(),
        goal: role.into(),
        ..Default::default()
    };
    let plan = tokage_core::planning::Plan {
        tasks: vec![task],
        ..Default::default()
    };
    let estimate = estimate(&plan, role, selected, &s.config)?;
    let spent = s.turns.values().filter_map(|t| t.cost_usd).sum::<f64>();
    let reserved = s
        .turns
        .values()
        .filter(|t| !t.ended || !t.cleanup_confirmed)
        .filter_map(|t| t.reserved_usd)
        .sum::<f64>();
    if estimate.is_none() && s.config.block_unknown_cost
        || s.config
            .budget_usd
            .is_some_and(|b| spent + reserved + estimate.unwrap_or(0.0) > b)
    {
        return Err(Error::Invalid("role budget unavailable".into()));
    }
    Ok(estimate)
}
