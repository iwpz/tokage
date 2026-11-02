//! Stable, read-only action projection. Native completion, task acceptance and fees are separate claims.
use crate::*;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    pub key: String,
    pub kind: String,
    pub run_id: String,
    pub task_id: Option<String>,
    pub attempt_id: String,
    pub source_turn: Option<String>,
    pub source_thread: Option<String>,
    pub target_thread: Option<String>,
    pub window_origin: Option<String>,
    pub status: String,
    pub reason: String,
    pub occurred_at: Option<u64>,
    pub parameters: serde_json::Value,
    pub receipt_key: Option<String>,
    pub reported_usd: Option<f64>,
    pub estimated_usd: Option<f64>,
    pub settlement: String,
    pub baseline_usd: Option<f64>,
    pub forecast_delta_usd: Option<f64>,
    pub measured_saving_usd: Option<f64>,
    pub evidence_digest: String,
}
pub fn project(s: &Snapshot, receipts: &[accounting::Receipt]) -> Result<Vec<Action>> {
    let mut rows = Vec::new();
    for t in s.turns.values() {
        let receipt = receipts.iter().find(|r| r.turn_id.as_ref() == Some(&t.id));
        let accepted = s
            .work
            .as_ref()
            .and_then(|w| w.attempts.get(&t.id))
            .is_some_and(|a| a.commit.is_some() && a.integrated_head.is_some());
        let mut add = |kind: &str,
                       source: Option<String>,
                       status: String,
                       reason: String,
                       parameters: serde_json::Value,
                       baseline: Option<f64>,
                       delta: Option<f64>|
         -> Result<()> {
            rows.push(Action {
                key: format!("{}:{}:{}", s.run_id, t.id, kind),
                kind: kind.into(),
                run_id: s.run_id.clone(),
                task_id: t.task_id.clone(),
                attempt_id: t.id.clone(),
                source_thread: source
                    .as_ref()
                    .and_then(|id| s.turns.get(id))
                    .and_then(|t| t.thread_id.clone()),
                source_turn: source,
                target_thread: t.thread_id.clone(),
                window_origin: t.window_origin.clone(),
                status,
                reason,
                occurred_at: t.finished_at.or(t.created_at),
                parameters,
                receipt_key: receipt.map(|r| r.key.clone()),
                reported_usd: receipt.and_then(|r| r.reported_usd),
                estimated_usd: receipt.and_then(|r| r.estimated_usd),
                settlement: if !t.ended || !t.cleanup_confirmed {
                    "PENDING_NATIVE_CLEANUP"
                } else if receipt.is_some_and(|r| r.reported_usd.is_some()) {
                    "REPORTED_USD_NOT_VENDOR_INVOICE"
                } else if receipt.is_some_and(|r| r.estimated_usd.is_some()) {
                    "TOKEN_EQUIVALENT_ONLY"
                } else {
                    "COST_UNKNOWN"
                }
                .into(),
                baseline_usd: baseline,
                forecast_delta_usd: delta,
                measured_saving_usd: None,
                evidence_digest: digest(&(t, kind))?,
            });
            Ok(())
        };
        if let Some(d) = &t.reuse_decision {
            let source = t.resume_source_intent.clone().or(d.previous_turn.clone());
            let requested = t.resume_source_intent.is_some() || d.reuse;
            let context_fresh = t
                .context_preparation
                .as_ref()
                .is_some_and(|p| p.applies && p.require_new_window);
            let economic = d.reuse && source == d.previous_turn;
            let native_matches = requested
                && t.accounting_launch
                    .as_ref()
                    .and_then(|l| l.resume_source.as_ref())
                    == source.as_ref()
                && source
                    .as_ref()
                    .and_then(|id| s.turns.get(id))
                    .and_then(|old| old.thread_id.as_ref())
                    .is_some_and(|thread| t.thread_id.as_ref() == Some(thread))
                && t.observed_provider
                    .as_ref()
                    .is_none_or(|p| p == &t.requested.provider)
                && t.observed_model.as_ref().is_none_or(|m| {
                    tokage_matching::assignment::id_matches(m, &t.requested.requested_model)
                })
                && t.window_identity.is_some()
                && t.status == turn::TurnStatus::Completed
                && t.cleanup_confirmed;
            let status = if requested && context_fresh {
                "FRESH_CONTEXT_SELECTED"
            } else if !requested {
                "NOT_SELECTED"
            } else if !t.ended {
                "PENDING"
            } else if !native_matches {
                "FAILED_OR_FRESH_FALLBACK"
            } else if accepted {
                "TASK_ACCEPTED"
            } else {
                "NATIVE_CONFIRMED_TASK_NOT_ACCEPTED"
            };
            let costs = d
                .inputs
                .as_ref()
                .filter(|_| economic)
                .and_then(|v| v.get("costEstimates"));
            let baseline = costs.and_then(|v| v["freshUsd"].as_f64());
            let reuse = costs.and_then(|v| v["reuseUsd"].as_f64());
            add(
                "REUSE",
                source,
                status.into(),
                if requested && context_fresh {
                    "Context policy selected a fresh window; no resume was executed".into()
                } else if requested && !native_matches && t.ended {
                    t.error
                        .clone()
                        .or_else(|| s.work.as_ref()?.attempts.get(&t.id)?.error.clone())
                        .unwrap_or_else(|| {
                            "Native reuse not confirmed or dispatch opened fresh".into()
                        })
                } else if requested && !economic {
                    "Same-task continuation; cross-task economic forecast not applied".into()
                } else {
                    d.reason.clone()
                },
                serde_json::json!({"decision":d,"resumeSourceIntent":t.resume_source_intent,"launch":t.accounting_launch}),
                baseline,
                baseline.zip(reuse).map(|(a, b)| a - b),
            )?;
        }
        if let Some(p) = &t.context_preparation {
            let status = if p.status == "FAILED" || p.status == "CANCELLED" {
                p.status.clone()
            } else if !t.ended {
                "PENDING".into()
            } else if t.status != turn::TurnStatus::Completed || !t.cleanup_confirmed {
                "FAILED".into()
            } else if !p.applies {
                "NOT_APPLIED".into()
            } else if p.native_confirmed {
                "NATIVE_CONFIRMED".into()
            } else {
                "CARRIER_COMPLETED_NATIVE_UNKNOWN".into()
            };
            add(
                "CONTEXT",
                Some(p.source_turn.clone()),
                status,
                p.failure.clone().unwrap_or_else(|| {
                    p.preview["preview"]["explanation"]
                        .as_str()
                        .unwrap_or("Context decision")
                        .into()
                }),
                serde_json::to_value(p).map_err(|e| Error::Invalid(e.to_string()))?,
                p.preview["preview"]["continueCostUsd"].as_f64(),
                {
                    let preview = &p.preview["preview"];
                    let chosen = if p.require_new_window {
                        "freshCostUsd"
                    } else {
                        "compactCostUsd"
                    };
                    preview["continueCostUsd"]
                        .as_f64()
                        .zip(preview[chosen].as_f64())
                        .map(|(a, b)| a - b)
                },
            )?;
        }
    }
    for (task_id, task) in s
        .execution
        .as_ref()
        .into_iter()
        .flat_map(|e| e.tasks.iter())
    {
        for (index, p) in task.context_actions.iter().enumerate() {
            if s.turns.values().any(|t| {
                t.context_preparation.as_ref().is_some_and(|bound| {
                    bound.user_accepted
                        && bound.source_turn == p.source_turn
                        && bound.source_revision == p.source_revision
                })
            }) {
                continue;
            }
            let source = s.turns.get(&p.source_turn);
            rows.push(Action {
                key: format!("{}:queue:{}:{}", s.run_id, task_id, index),
                kind: "CONTEXT".into(),
                run_id: s.run_id.clone(),
                task_id: Some(task_id.clone()),
                attempt_id: p.target_turn.clone(),
                source_turn: Some(p.source_turn.clone()),
                source_thread: source.and_then(|t| t.thread_id.clone()),
                target_thread: None,
                window_origin: source.and_then(|t| t.window_origin.clone()),
                status: p.status.clone(),
                reason: p
                    .failure
                    .clone()
                    .unwrap_or_else(|| "Queued context action; no native turn receipt".into()),
                occurred_at: p.bound_at,
                parameters: serde_json::to_value(p).map_err(|e| Error::Invalid(e.to_string()))?,
                receipt_key: None,
                reported_usd: None,
                estimated_usd: None,
                settlement: "NO_NATIVE_RECEIPT".into(),
                baseline_usd: p.preview["preview"]["continueCostUsd"].as_f64(),
                forecast_delta_usd: None,
                measured_saving_usd: None,
                evidence_digest: digest(p)?,
            });
        }
    }
    rows.sort_by(|a, b| (&a.occurred_at, &a.key).cmp(&(&b.occurred_at, &b.key)));
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancelled_queue_without_a_native_turn_keeps_stable_unknown_fee_evidence() {
        let (mut s, _) = task_context::tests::fixture();
        s.execution
            .as_mut()
            .unwrap()
            .tasks
            .get_mut("T1")
            .unwrap()
            .context_actions
            .push(task_context::Prepared {
                source_turn: "b".into(),
                target_turn: "next:b".into(),
                source_revision: 3,
                config_hash: s.config_hash.clone(),
                plan_digest: s.approved_digest.clone().unwrap(),
                source_digest: "source".into(),
                policy: "suggest".into(),
                status: "CANCELLED".into(),
                preview: serde_json::json!({"preview":{"continueCostUsd":1.0}}),
                prefix: "retained contract".into(),
                require_new_window: true,
                applies: true,
                bound_at: None,
                native_confirmed: false,
                user_accepted: true,
                failure: None,
            });
        let p = accounting::project(&s).unwrap();
        let a = p
            .context_actions
            .iter()
            .find(|a| a.kind == "CONTEXT")
            .unwrap();
        assert_eq!(a.status, "CANCELLED");
        assert_eq!(a.settlement, "NO_NATIVE_RECEIPT");
        assert!(
            a.receipt_key.is_none()
                && a.reported_usd.is_none()
                && a.estimated_usd.is_none()
                && a.occurred_at.is_none()
        );
        assert_eq!(accounting::project(&s).unwrap(), p);
    }
    #[test]
    fn reused_native_completion_is_not_task_acceptance_or_measured_savings() {
        let (mut s, _) = task_context::tests::fixture();
        let b = s.turns.get_mut("b").unwrap();
        b.accounting_launch = Some(usage_attribution::Launch {
            resume_source: Some("a".into()),
            requested_thread: b.thread_id.clone(),
        });
        b.window_identity = Some(window_identity::WindowIdentity {
            config_hash: s.config_hash.clone(),
            plan_digest: s.approved_digest.clone().unwrap(),
            assignment: b.requested.clone(),
            role: "executor".into(),
            write_scope: vec![],
            workspace: artifacts::Workspace {
                root: "/tree".into(),
                cwd: "/tree".into(),
                branch: "source".into(),
            },
        });
        b.reuse_decision = Some(execution::ContextDecision {
            previous_turn: Some("a".into()),
            reuse: true,
            reason: "forecast".into(),
            inputs: Some(serde_json::json!({"costEstimates":{"freshUsd":1.0,"reuseUsd":2.0}})),
        });
        let p = accounting::project(&s).unwrap();
        let row = p
            .context_actions
            .iter()
            .find(|a| a.kind == "REUSE")
            .unwrap();
        assert_eq!(row.status, "NATIVE_CONFIRMED_TASK_NOT_ACCEPTED");
        assert_eq!(row.forecast_delta_usd, Some(-1.));
        assert_eq!(row.measured_saving_usd, None);
        assert_eq!(row.source_thread.as_deref(), Some("native-window"));
        let key = row.key.clone();
        s.turns.get_mut("b").unwrap().thread_id = Some("wrong-thread".into());
        let p = accounting::project(&s).unwrap();
        let row = p
            .context_actions
            .iter()
            .find(|a| a.kind == "REUSE")
            .unwrap();
        assert_eq!(row.key, key);
        assert_eq!(row.status, "FAILED_OR_FRESH_FALLBACK");
        assert_eq!(row.measured_saving_usd, None);
        assert_eq!(accounting::project(&s).unwrap(), p);
    }
}
