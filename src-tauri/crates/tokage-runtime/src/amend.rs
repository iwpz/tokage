//! Explicit amendments after all native effects have stopped. Completed work stays immutable.
use crate::*;
pub(crate) fn quiescent(s: &Snapshot) -> Result<()> {
    if !matches!(s.state, RunState::Blocked | RunState::Stopped)
        && !(s.execution.is_some()
            && matches!(s.state, RunState::AwaitingApproval | RunState::Approved))
    {
        return Err(Error::Transition);
    }
    if s.turns.values().any(|t| !t.ended || !t.cleanup_confirmed)
        || s.execution
            .as_ref()
            .is_some_and(|e| e.tasks.values().any(|t| t.reservation_active))
    {
        return Err(Error::CleanupUnknown);
    }
    if s.work.as_ref().is_some_and(|w| {
        w.attempts.values().any(|a| {
            (a.commit_intent && a.integrated_head.is_none())
                || a.gates.iter().any(|g| !g.cleanup_confirmed)
        })
    }) || s
        .delivery
        .as_ref()
        .and_then(|d| d.intent.as_ref())
        .is_some_and(|i| i.claimed && !i.completed)
    {
        return Err(Error::Invalid(
            "amendment requires Git/gate reconciliation".into(),
        ));
    }
    Ok(())
}
pub(crate) fn revise(
    s: &Snapshot,
    confirmation: &str,
    config: &EffectiveRunConfig,
    text: &str,
) -> Result<Snapshot> {
    quiescent(s)?;
    if s.plan
        .as_ref()
        .is_none_or(|p| p.confirmation_digest != confirmation)
    {
        return Err(Error::Invalid("stale amendment confirmation".into()));
    }
    config.validate()?;
    if config.workspace != s.config.workspace
        || config.target_identity != s.config.target_identity
        || config.target_reference != s.config.target_reference
        || config.scope != s.config.scope
        || config.command_policy_hash != s.config.command_policy_hash
        || config.custom_pack_hashes != s.config.custom_pack_hashes
        || config.planning_policy != s.config.planning_policy
        || config.trust_workspace != s.config.trust_workspace
        || config.permission_mode != s.config.permission_mode
        || config.co_author != s.config.co_author
        || config.gate_env_allow != s.config.gate_env_allow
    {
        return Err(Error::Invalid(
            "workspace and security policy changes require a new run".into(),
        ));
    }
    let mut next = s.clone();
    next.config = config.clone();
    next.config_hash = digest(config)?;
    next.plan = Some(planning::normalize(text, config)?);
    if let Some(old) = &s.execution {
        let plan = &next.plan.as_ref().unwrap();
        let previous = &s.plan.as_ref().unwrap().plan;
        for (id, t) in old
            .tasks
            .iter()
            .filter(|(_, t)| t.state == tokage_orchestration::TaskState::Done)
        {
            if plan.plan.tasks.iter().find(|t| t.id == *id)
                != previous.tasks.iter().find(|t| t.id == *id)
                || plan.assignments.get(id) != Some(&t.assignment)
            {
                return Err(Error::Invalid(
                    "completed tasks and their allocation are immutable; create a new run".into(),
                ));
            }
        }
        let mut ex = execution::Execution::new(&next)?;
        ex.sequence = old.sequence;
        ex.health = old.health.clone();
        for (id, t) in &mut ex.tasks {
            if let Some(prior) = old.tasks.get(id) {
                if prior.state == tokage_orchestration::TaskState::Done {
                    *t = prior.clone();
                } else {
                    t.attempts = prior.attempts.clone();
                }
            }
        }
        next.execution = Some(ex);
    }
    next.approved_digest = None;
    next.delivery = None;
    next.state = RunState::AwaitingApproval;
    Ok(next)
}
