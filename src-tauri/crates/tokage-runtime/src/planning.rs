//! Shared plan normalization, model selection and authorization. No entrypoint policy.
use crate::*;
use std::collections::{HashMap, HashSet};
use tokage_core::planning::Plan;
use tokage_matching::{assignment, learning::LearnedIndex, merge, pack};
use tokage_planning::{
    parse_plan, validate_plan, PlanParseOptions, PlanValidationOptions, ValidationIssue,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanningPolicy {
    pub official: Option<pack::KnowledgePack>,
    pub customs: Vec<(String, pack::KnowledgePack)>,
    pub overrides: Vec<merge::ModelOverride>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanRecord {
    pub plan: Plan,
    pub assignments: BTreeMap<String, ModelAssignment>,
    pub issues: Vec<ValidationIssue>,
    pub confirmation_digest: String,
}
pub fn effective_pack(config: &EffectiveRunConfig) -> Result<merge::EffectiveKnowledgePack> {
    let policy = config.planning_policy.clone().unwrap_or_default();
    let hashes = policy
        .customs
        .iter()
        .map(|(_, p)| digest(p))
        .collect::<Result<Vec<_>>>()?;
    if hashes != config.custom_pack_hashes {
        return Err(Error::Invalid(
            "custom pack hashes do not match frozen layers".into(),
        ));
    }
    merge::merge_layers(&merge::LayeredPacks {
        builtin: pack::builtin_knowledge_pack().clone(),
        official: policy.official,
        customs: policy.customs,
        overrides: policy.overrides,
        learned_usage_profiles: None,
    })
    .map_err(|e| Error::Invalid(e.to_string()))
}
pub fn editable(state: RunState) -> bool {
    matches!(
        state,
        RunState::Draft | RunState::AwaitingApproval | RunState::Approved
    )
}
fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}
/// Conservative containment proof for globs: equal globs or a literal directory prefix.
/// Arbitrary glob-language inclusion is deliberately not guessed from overlap.
fn within(child: &str, parent: &str) -> bool {
    use tokage_core::command_policy::scope::normalize_scope;
    let c = normalize_scope(child);
    let p = normalize_scope(parent);
    p == "**"
        || c == p
        || p.strip_suffix("/**").is_some_and(|base| {
            !base.contains(['*', '?', '[']) && c.starts_with(&format!("{base}/"))
        })
}
fn safe_scope(scope: &str) -> bool {
    let s = scope.replace('\\', "/");
    !s.trim().is_empty()
        && !s.starts_with('/')
        && !s.contains(':')
        && !s
            .split('/')
            .any(|p| p == ".." || p.eq_ignore_ascii_case(".git"))
}
pub fn normalize(text: &str, config: &EffectiveRunConfig) -> Result<PlanRecord> {
    let pack = effective_pack(config)?;
    let mut plan =
        parse_plan(text, &PlanParseOptions::default()).map_err(|e| invalid(e.to_string()))?;
    // Only config, submitted by a user intent, may confirm commands or lock models.
    for t in &mut plan.tasks {
        t.acceptance.confirmed_commands = t
            .acceptance
            .commands
            .iter()
            .filter(|c| config.confirmed_commands.contains(c))
            .cloned()
            .collect();
        t.assigned_model = None;
        t.model_params = None;
        if t.id.trim().is_empty()
            || t.title.trim().is_empty()
            || t.goal.trim().is_empty()
            || t.write_scope.is_empty()
            || t.acceptance.commands.is_empty()
            || t.write_scope.iter().any(|p| {
                !safe_scope(p) || !config.scope.iter().any(|s| safe_scope(s) && within(p, s))
            })
        {
            return Err(invalid(format!(
                "task {} requires identity, goal, bounded scope and acceptance commands",
                t.id
            )));
        }
    }
    // Sort before validation/automatic serialization: HashMap/topological traversal order is
    // not authority. Matching below receives this stable task ordering and frozen catalog.
    plan.tasks.sort_by(|a, b| a.id.cmp(&b.id));
    let validated = validate_plan(
        plan,
        &PlanValidationOptions {
            valid_domains: pack.domains.clone(),
            gate_allowlist: pack.gate_allowlist.clone(),
            ..Default::default()
        },
    );
    // Unconfirmed commands are editable findings, not permission granted by the model.
    if validated.issues.iter().any(|i| {
        i.severity == tokage_planning::ValidationSeverity::Error
            && i.rule != "unconfirmed_acceptance_command"
    }) {
        return Err(invalid(
            serde_json::to_string(&validated.issues).unwrap_or_default(),
        ));
    }
    let plan = validated.plan;
    let available: Vec<_> = config
        .models
        .values()
        .map(|m| assignment::AvailableModel {
            provider: m.provider.clone(),
            cli_model_arg: m.requested_model.clone(),
        })
        .collect();
    let mut restricted = assignment::restrict_pack(&pack, Some(&available), &HashSet::new());
    restricted
        .candidates
        .sort_by(|a, b| (&a.provider, &a.cli_model_arg).cmp(&(&b.provider, &b.cli_model_arg)));
    let mut overrides = HashMap::new();
    for t in &plan.tasks {
        if let Some(m) = config
            .models
            .get(&t.id)
            .or_else(|| config.models.get("executor"))
        {
            if m.user_locked {
                overrides.insert(
                    t.id.clone(),
                    format!("{}:{}", m.provider, m.requested_model),
                );
            }
        }
    }
    let allocation = assignment::recompute_plan_allocation_with_overrides(
        &plan,
        &restricted,
        &LearnedIndex::empty(),
        &overrides,
        None,
        None,
    )
    .map_err(invalid)?;
    let mut assignments = BTreeMap::new();
    for task in &plan.tasks {
        let locked = config
            .models
            .get(&task.id)
            .or_else(|| config.models.get("executor"))
            .filter(|m| m.user_locked);
        let m = if let Some(m) = locked {
            // Matching allows unknown explicit models, but their exact provider/model identity
            // must survive. Unknown price remains unknown in scheduling, never inferred zero.
            m.clone()
        } else {
            let a = allocation
                .assignments
                .iter()
                .find(|a| a.task_id == task.id)
                .ok_or_else(|| invalid("no runnable model assignment"))?;
            config
                .models
                .values()
                .find(|m| m.provider == a.provider && m.requested_model == a.cli_model_arg)
                .ok_or_else(|| invalid("assignment outside frozen catalog"))?
                .clone()
        };
        assignments.insert(task.id.clone(), m);
    }
    let confirmation_digest = digest(&(SCHEMA_VERSION, &plan, &assignments, digest(config)?))?;
    Ok(PlanRecord {
        plan,
        assignments,
        issues: validated.issues,
        confirmation_digest,
    })
}
pub fn approve(s: &Snapshot, confirmation: &str) -> Result<()> {
    let p = s.plan.as_ref().ok_or(Error::Transition)?;
    if confirmation != p.confirmation_digest {
        return Err(invalid("stale plan confirmation"));
    }
    if !s.config.trust_workspace || s.config.permission_mode == "read-only" {
        return Err(invalid(
            "explicit workspace trust and write permission required",
        ));
    }
    let pack = effective_pack(&s.config)?;
    let mut policy: tokage_core::command_policy::PackPolicy = serde_json::from_value(
        pack::builtin_knowledge_pack()
            .command_policy
            .clone()
            .ok_or_else(|| invalid("missing command policy"))?,
    )
    .map_err(|e| invalid(e.to_string()))?;
    policy
        .forbidden
        .extend(pack.command_policy_forbidden.clone());
    policy.forbidden_paths.extend(pack.forbidden_paths.clone());
    let mut context = tokage_core::command_policy::Context::new(s.config.workspace.clone());
    context.workspace_roots = vec![s.config.workspace.clone()];
    for task in &p.plan.tasks {
        for command in &task.acceptance.commands {
            if tokage_core::command_policy::Policy::new(&policy, &[])
                .classify(command, &context)
                .level
                == tokage_core::command_policy::Level::Forbidden
            {
                return Err(invalid("command forbidden by shared policy"));
            }
            if let tokage_orchestration::GateCommandPermission::Denied(reason) =
                tokage_orchestration::check_gate_command_safety(
                    command,
                    s.config.confirmed_commands.contains(command),
                    &pack.gate_allowlist,
                )
            {
                return Err(invalid(reason));
            }
        }
    }
    Ok(())
}
pub fn validate_snapshot(s: &Snapshot) -> Result<()> {
    if s.state == RunState::Planning && s.planning_attempt.is_none() {
        return Err(invalid("missing planning intent"));
    }
    if let Some(p) = &s.plan {
        let expected = normalize(&p.plan.raw_markdown, &s.config)?;
        if &expected != p {
            return Err(invalid("plan normalization/hash mismatch"));
        }
    }
    if let Some(d) = &s.approved_digest {
        approve(s, d)?;
    }
    if matches!(s.state, RunState::AwaitingApproval | RunState::Approved) && s.plan.is_none()
        || s.state == RunState::Approved && s.approved_digest.is_none()
    {
        return Err(invalid("missing plan/approval evidence"));
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanningAttempt {
    pub id: String,
    pub claimed: bool,
    pub error: Option<String>,
}
