//! Resume authorization is compared with the actual launch snapshot, never reconstructed from today's settings.
use crate::*;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WindowIdentity {
    pub config_hash: String,
    pub plan_digest: String,
    pub assignment: ModelAssignment,
    pub role: String,
    pub write_scope: Vec<String>,
    pub workspace: artifacts::Workspace,
}
pub(crate) fn capture(
    s: &Snapshot,
    task: &str,
    assignment: &ModelAssignment,
    workspace: &artifacts::Workspace,
) -> Result<WindowIdentity> {
    let task = s
        .plan
        .as_ref()
        .and_then(|p| p.plan.tasks.iter().find(|t| t.id == task))
        .ok_or(Error::Transition)?;
    Ok(WindowIdentity {
        config_hash: s.config_hash.clone(),
        plan_digest: s.approved_digest.clone().ok_or(Error::Transition)?,
        assignment: assignment.clone(),
        role: "executor".into(),
        write_scope: task.write_scope.clone(),
        workspace: workspace.clone(),
    })
}
pub(crate) fn key(t: &turn::TurnRecord) -> Option<String> {
    let thread = t.thread_id.as_ref().filter(|id| !id.trim().is_empty())?;
    digest(&(&t.requested.provider, &t.requested.provider_path, thread)).ok()
}
pub(crate) fn resources(s: &Snapshot, t: &turn::TurnRecord) -> Vec<String> {
    let mut keys = key(t).into_iter().collect::<Vec<_>>();
    if let Some(w) = s
        .work
        .as_ref()
        .and_then(|w| w.attempts.get(&t.id))
        .and_then(|a| a.workspace.as_ref())
    {
        if let Ok(k) = digest(&("workspace", &w.root, &w.cwd)) {
            keys.push(k);
        }
    }
    keys
}
fn overlaps(s: &Snapshot, a: &turn::TurnRecord, b: &turn::TurnRecord) -> bool {
    let keys = resources(s, a);
    resources(s, b).iter().any(|key| keys.contains(key))
}
pub(crate) fn compatible(
    s: &Snapshot,
    source: &turn::TurnRecord,
    task: &str,
    assignment: &ModelAssignment,
) -> bool {
    let Some(identity) = source.window_identity.as_ref() else {
        return false;
    };
    let Some(workspace) = s
        .work
        .as_ref()
        .and_then(|w| w.attempts.get(&source.id))
        .and_then(|a| a.workspace.as_ref())
    else {
        return false;
    };
    if !s.config.trust_workspace
        || source.role != "executor"
        || source.requested != *assignment
        || !source.ended
        || !source.cleanup_confirmed
        || key(source).is_none()
        || source
            .observed_provider
            .as_ref()
            .is_some_and(|p| p != &assignment.provider)
        || source.observed_model.as_ref().is_some_and(|m| {
            !tokage_matching::assignment::id_matches(m, &assignment.requested_model)
        })
    {
        return false;
    }
    capture(s, task, assignment, workspace).is_ok_and(|expected| identity == &expected)
}
fn newer_alias(s: &Snapshot, source: &turn::TurnRecord, other: &turn::TurnRecord) -> bool {
    if source.task_id == other.task_id {
        if let Some(attempts) = source
            .task_id
            .as_ref()
            .and_then(|id| s.execution.as_ref()?.tasks.get(id))
            .map(|t| &t.attempts)
        {
            if let (Some(a), Some(b)) = (
                attempts.iter().position(|id| id == &source.id),
                attempts.iter().position(|id| id == &other.id),
            ) {
                return b > a;
            }
        }
        return other.finished_at > source.finished_at;
    }
    // Second-resolution timestamps cannot order aliases from different tasks at a tie.
    other.finished_at >= source.finished_at
}
/// All aliases of a native thread share ownership, including a completed source borrowed by an active task.
pub(crate) fn busy(s: &Snapshot, source: &turn::TurnRecord, target: &str) -> bool {
    if key(source).is_none() {
        return true;
    }
    s.turns.values().any(|t| {
        t.id != source.id
            && overlaps(s, source, t)
            && (!t.ended || !t.cleanup_confirmed || newer_alias(s, source, t))
    }) || s.execution.as_ref().is_some_and(|e| {
        e.tasks.iter().any(|(id, t)| {
            id != target
                && t.state.is_active()
                && (t
                    .attempts
                    .iter()
                    .any(|id| s.turns.get(id).is_some_and(|t| overlaps(s, source, t)))
                    || t.context
                        .as_ref()
                        .filter(|c| c.reuse)
                        .and_then(|c| c.previous_turn.as_ref())
                        .and_then(|id| s.turns.get(id))
                        .is_some_and(|t| overlaps(s, source, t)))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Snapshot {
        let (mut s, _) = task_context::tests::fixture();
        let workspace = artifacts::Workspace {
            root: "/owned/task".into(),
            cwd: "/owned/task".into(),
            branch: "tokage/source".into(),
        };
        for id in ["a", "b"] {
            let identity = capture(&s, "T1", &s.turns[id].requested, &workspace).unwrap();
            s.turns.get_mut(id).unwrap().window_identity = Some(identity);
            s.work.as_mut().unwrap().attempts.insert(
                id.into(),
                artifacts::AttemptEvidence {
                    workspace: Some(workspace.clone()),
                    ..Default::default()
                },
            );
        }
        s
    }
    fn compatible_source(s: &Snapshot) -> bool {
        compatible(s, &s.turns["b"], "T1", &s.turns["b"].requested)
    }
    #[test]
    fn frozen_authorization_workspace_role_and_observed_identity_are_all_required() {
        let s = fixture();
        assert!(compatible_source(&s));
        for change in 0..12 {
            let mut changed = s.clone();
            match change {
                0 => changed.turns.get_mut("b").unwrap().window_identity = None,
                1 => changed.config_hash = "new permission/startup/config".into(),
                2 => changed.approved_digest = Some("new plan authorization".into()),
                3 => {
                    changed
                        .work
                        .as_mut()
                        .unwrap()
                        .attempts
                        .get_mut("b")
                        .unwrap()
                        .workspace
                        .as_mut()
                        .unwrap()
                        .cwd = "/other".into()
                }
                4 => {
                    changed
                        .work
                        .as_mut()
                        .unwrap()
                        .attempts
                        .get_mut("b")
                        .unwrap()
                        .workspace
                        .as_mut()
                        .unwrap()
                        .branch = "other".into()
                }
                5 => changed.config.trust_workspace = false,
                6 => changed.turns.get_mut("b").unwrap().role = "reviewer".into(),
                7 => {
                    changed.turns.get_mut("b").unwrap().observed_model =
                        Some("unrelated-model".into())
                }
                8 => changed.turns.get_mut("b").unwrap().observed_provider = Some("OTHER".into()),
                9 => changed.turns.get_mut("b").unwrap().cleanup_confirmed = false,
                10 => {
                    changed.plan.as_mut().unwrap().plan.tasks[0].write_scope =
                        vec!["other/**".into()]
                }
                _ => changed.turns.get_mut("b").unwrap().thread_id = Some(" ".into()),
            };
            assert!(!compatible_source(&changed), "mutation {change}");
        }
    }
    #[test]
    fn same_thread_aliases_cannot_escape_active_or_uncertain_ownership() {
        let mut s = fixture();
        assert!(!busy(&s, &s.turns["b"], "T1"));
        let mut alias = s.turns["a"].clone();
        alias.id = "alias".into();
        alias.ended = false;
        s.turns.insert(alias.id.clone(), alias);
        assert!(busy(&s, &s.turns["b"], "T1"));
        s.turns.get_mut("alias").unwrap().ended = true;
        s.turns.get_mut("alias").unwrap().cleanup_confirmed = false;
        assert!(busy(&s, &s.turns["b"], "T1"));
        s.turns.get_mut("alias").unwrap().cleanup_confirmed = true;
        s.turns.get_mut("alias").unwrap().finished_at = Some(21);
        assert!(busy(&s, &s.turns["b"], "T1"));
        s.turns.get_mut("alias").unwrap().finished_at = Some(10);
        let mut borrower = s.execution.as_ref().unwrap().tasks["T1"].clone();
        borrower.state = tokage_orchestration::TaskState::Running;
        borrower.attempts = vec!["alias".into()];
        s.execution
            .as_mut()
            .unwrap()
            .tasks
            .insert("T2".into(), borrower);
        assert!(busy(&s, &s.turns["b"], "T1"));
        s.turns.get_mut("alias").unwrap().thread_id = Some("different-native-thread".into());
        let workspace = s.work.as_ref().unwrap().attempts["b"].clone();
        s.work
            .as_mut()
            .unwrap()
            .attempts
            .insert("alias".into(), workspace);
        assert!(busy(&s, &s.turns["b"], "T1"));
    }
    #[test]
    fn provider_path_and_model_parameters_are_startup_identity() {
        let s = fixture();
        let mut assignment = s.turns["b"].requested.clone();
        assignment.provider_path = Some("/different-cli".into());
        assert!(!compatible(&s, &s.turns["b"], "T1", &assignment));
        let mut assignment = s.turns["b"].requested.clone();
        assignment.model_params = Some(tokage_core::planning::ModelParams {
            effort: Some("high".into()),
            fast: false,
            tier: None,
        });
        assert!(!compatible(&s, &s.turns["b"], "T1", &assignment));
    }
}
