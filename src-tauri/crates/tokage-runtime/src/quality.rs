//! Read-only task quality projection. Never publishes completion or updates execution state.
use crate::{execution::TaskRun, turn::TurnStatus, *};
use tokage_orchestration::TaskState;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    pub key: String,
    pub run_id: String,
    pub task_id: String,
    pub revision: u64,
    pub run_state: RunState,
    pub provider: String,
    pub model: String,
    pub model_basis: String,
    pub domain: String,
    pub difficulty: u8,
    pub outcome: String,
    pub learning_eligible: bool,
    pub attempt_ids: Vec<String>,
    pub observed_at: Option<u64>,
    pub evidence_digest: String,
}
impl Observation {
    pub fn resolved(&self) -> bool {
        matches!(
            self.outcome.as_str(),
            "FIRST_PASS"
                | "FIXED_IN_WINDOW"
                | "REASSIGNED"
                | "RETRIED"
                | "USER_TAKEOVER"
                | "ABANDONED"
                | "REMOVED"
        )
    }
}
fn accepted(s: &Snapshot, task: &TaskRun) -> bool {
    if task.state != TaskState::Done || crate::artifacts::validate_snapshot(s).is_err() {
        return false;
    }
    task.attempts.last().is_some_and(|id| {
        s.work
            .as_ref()
            .and_then(|w| w.attempts.get(id))
            .is_some_and(|a| {
                a.manual_receipt.is_some()
                    || s.turns.get(id).is_some_and(|t| {
                        t.status == TurnStatus::Completed
                            && t.ended
                            && t.cleanup_confirmed
                            && t.error.is_none()
                    })
            })
    })
}
pub fn observations(s: &Snapshot) -> Result<Vec<Observation>> {
    let (Some(plan), Some(ex)) = (&s.plan, &s.execution) else {
        return Ok(vec![]);
    };
    let mut rows = Vec::new();
    for definition in &plan.plan.tasks {
        let Some(task) = ex.tasks.get(&definition.id) else {
            continue;
        };
        if task.attempts.is_empty() {
            continue;
        }
        let mut earlier = s
            .turns
            .values()
            .filter(|t| {
                t.role == "executor"
                    && t.task_id.as_deref() == Some(&definition.id)
                    && !task.attempts.contains(&t.id)
            })
            .collect::<Vec<_>>();
        earlier.sort_by_key(|t| (t.created_at.unwrap_or(0), t.id.clone()));
        let incomplete_history = !earlier.is_empty();
        let attempt_ids = earlier
            .iter()
            .map(|t| t.id.clone())
            .chain(task.attempts.iter().cloned())
            .collect::<Vec<_>>();
        let turns = attempt_ids
            .iter()
            .filter_map(|id| s.turns.get(id))
            .collect::<Vec<_>>();
        let first = turns.first().copied();
        let last_evidence = task
            .attempts
            .last()
            .and_then(|id| s.work.as_ref()?.attempts.get(id));
        let manual = task.attempts.iter().any(|id| {
            s.work
                .as_ref()
                .and_then(|w| w.attempts.get(id))
                .is_some_and(|a| a.manual_receipt.is_some())
        });
        let valid_turns = turns
            .iter()
            .all(|t| t.task_id.as_deref() == Some(&definition.id) && t.role == "executor");
        let ambiguous_approval = turns.iter().any(|t| t.approval.is_some());
        let outcome = if !valid_turns || first.is_none() || ambiguous_approval {
            "UNKNOWN"
        } else if accepted(s, task) && turns.iter().all(|t| t.ended && t.cleanup_confirmed) {
            if incomplete_history {
                "RETRIED"
            } else if manual {
                "USER_TAKEOVER"
            } else if turns.len() != task.attempts.len() {
                "UNKNOWN"
            } else if turns.len() == 1 {
                "FIRST_PASS"
            } else if turns
                .iter()
                .any(|t| t.requested != first.unwrap().requested)
            {
                "REASSIGNED"
            } else if first.unwrap().thread_id.is_some()
                && turns
                    .iter()
                    .all(|t| t.thread_id == first.unwrap().thread_id)
            {
                "FIXED_IN_WINDOW"
            } else {
                "RETRIED"
            }
        } else if task.state == TaskState::Done {
            "UNKNOWN"
        } else if s.state == RunState::Stopped
            && turns.iter().all(|t| t.ended && t.cleanup_confirmed)
        {
            "ABANDONED"
        } else if task.state == TaskState::Blocked {
            "BLOCKED"
        } else {
            "IN_PROGRESS"
        };
        let provider = first
            .map(|t| {
                t.observed_provider
                    .as_ref()
                    .unwrap_or(&t.requested.provider)
            })
            .cloned()
            .unwrap_or_default();
        let model = first
            .map(|t| {
                t.observed_model
                    .as_ref()
                    .unwrap_or(&t.requested.requested_model)
            })
            .cloned()
            .unwrap_or_default();
        rows.push(Observation {
            key: digest(&(s.run_id.as_str(), definition.id.as_str()))?,
            run_id: s.run_id.clone(),
            task_id: definition.id.clone(),
            revision: s.revision,
            run_state: s.state,
            provider,
            model,
            model_basis: if incomplete_history {
                "HISTORICAL_INCOMPLETE"
            } else if first.is_some_and(|t| t.observed_model.is_some()) {
                "CLI_OBSERVED"
            } else {
                "REQUESTED"
            }
            .into(),
            domain: definition.domain.clone(),
            difficulty: definition.difficulty,
            outcome: outcome.into(),
            learning_eligible: !incomplete_history
                && (matches!(
                    outcome,
                    "FIRST_PASS" | "FIXED_IN_WINDOW" | "REASSIGNED" | "RETRIED" | "USER_TAKEOVER"
                ) || outcome == "ABANDONED"
                    && turns.iter().any(|t| t.status == TurnStatus::Failed)),
            attempt_ids,
            observed_at: if task.state == TaskState::Done {
                last_evidence.and_then(|a| a.accepted_at)
            } else if outcome == "ABANDONED" {
                None
            } else {
                turns.iter().filter_map(|t| t.finished_at).max()
            },
            evidence_digest: digest(&(task, last_evidence, &turns))?,
        });
    }
    // Amendments may withdraw an unaccepted task. Keep its model attempts visible;
    // absent original task metadata/timing is not a fabricated quality label.
    let mut withdrawn = BTreeMap::<String, Vec<&turn::TurnRecord>>::new();
    for t in s.turns.values().filter(|t| t.role == "executor") {
        if let Some(id) = &t.task_id {
            if !plan.plan.tasks.iter().any(|d| &d.id == id) {
                withdrawn.entry(id.clone()).or_default().push(t);
            }
        }
    }
    for (task_id, mut turns) in withdrawn {
        turns.sort_by_key(|t| (t.created_at.unwrap_or(0), t.id.clone()));
        let first = turns[0];
        rows.push(Observation {
            key: digest(&(s.run_id.as_str(), task_id.as_str()))?,
            run_id: s.run_id.clone(),
            task_id,
            revision: s.revision,
            run_state: s.state,
            provider: first.requested.provider.clone(),
            model: first.requested.requested_model.clone(),
            model_basis: "HISTORICAL_INCOMPLETE".into(),
            domain: "unknown".into(),
            difficulty: 0,
            outcome: if turns.iter().all(|t| t.ended && t.cleanup_confirmed) {
                "REMOVED"
            } else {
                "UNKNOWN"
            }
            .into(),
            learning_eligible: false,
            attempt_ids: turns.iter().map(|t| t.id.clone()).collect(),
            observed_at: None,
            evidence_digest: digest(&turns)?,
        });
    }
    Ok(rows)
}
/// One Bernoulli first-pass observation per resolved task, attributed to the original
/// executor. Repair, replacement, and human work never become a new first-pass success.
pub fn learned(rows: &[Observation]) -> Vec<planning::LearnedObservation> {
    let mut totals = BTreeMap::<(String, String, String, String), (f64, f64, u32)>::new();
    let mut latest = BTreeMap::<&str, &Observation>::new();
    for r in rows {
        let prior = latest.entry(&r.key).or_insert(r);
        if r.revision > prior.revision {
            *prior = r;
        }
    }
    for r in latest
        .values()
        .filter(|r| r.learning_eligible && !r.provider.is_empty() && !r.model.is_empty())
    {
        for provider in [&r.provider[..], "*"] {
            let value = totals
                .entry((
                    provider.into(),
                    r.model.clone(),
                    r.domain.clone(),
                    format!("d{}", r.difficulty),
                ))
                .or_default();
            if r.outcome == "FIRST_PASS" {
                value.0 += 1.0;
            } else {
                value.1 += 1.0;
            }
            value.2 += 1;
        }
    }
    totals
        .into_iter()
        .map(
            |((provider, model, domain, bucket), (alpha, beta, samples))| {
                planning::LearnedObservation {
                    provider,
                    model,
                    domain,
                    bucket,
                    alpha,
                    beta,
                    samples,
                    fix_rate: None,
                }
            },
        )
        .collect()
}

/// Only the explicitly selected runtime directory is read. No migration, owner or
/// history import is performed, and corrupt/unknown data is not silently skipped.
pub fn read(directory: &std::path::Path) -> Result<Vec<Observation>> {
    let path = directory.join("tokage.db");
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(Error::Repository(e.to_string())),
        Ok(_) => {}
    }
    let mut rows = Vec::new();
    for snapshot in crate::repository::Reader::open(directory)?.list()? {
        rows.extend(observations(&snapshot)?);
    }
    rows.sort_by(|a, b| a.key.cmp(&b.key));
    rows.dedup_by(|a, b| a.key == b.key);
    Ok(rows)
}
