//! Immutable per-turn accounting projection; reported USD, price estimates and
//! native credits are independent quantities. Reading never settles a bill.
use crate::*;
use tokage_core::{TurnUsage, UsageSource};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
    pub key: String,
    pub origin: String,
    pub run_id: Option<String>,
    pub turn_id: Option<String>,
    pub task_id: Option<String>,
    pub provider: String,
    pub model: String,
    pub model_basis: String,
    pub phase: String,
    pub status: String,
    pub observed_at: Option<u64>,
    pub reported_usd: Option<f64>,
    pub estimated_usd: Option<f64>,
    pub estimate_basis: String,
    /// Raw CLI receipt; never aggregate this without checking attributed_usage.
    pub usage: Option<TurnUsage>,
    #[serde(default)]
    pub attributed_usage: Option<TurnUsage>,
    #[serde(default)]
    pub attribution_basis: String,
    #[serde(default)]
    pub baseline_turn: Option<String>,
    pub source: String,
    pub price_basis: Option<PriceBasis>,
    pub evidence_digest: String,
    pub duplicate_sources: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HumanEffort {
    pub run_id: String,
    pub task_id: Option<String>,
    pub attempt_id: String,
    pub command_id: String,
    pub reported_seconds: Option<u64>,
    pub recorded_at: Option<u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Projection {
    pub receipts: Vec<Receipt>,
    pub human_effort: Vec<HumanEffort>,
    pub context_actions: Vec<crate::context_audit::Action>,
}
fn finite(value: Option<f64>) -> Option<f64> {
    value.filter(|v| v.is_finite() && *v >= 0.)
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PriceBasis {
    pub pack_version: String,
    pub config_hash: String,
    pub provider: String,
    pub model: String,
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub calibration_factor: Option<f64>,
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub calibration_key: Option<String>,
    pub input: f64,
    pub output: f64,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
    pub long_context: Option<tokage_matching::pack::LongContextPricingDef>,
}
pub(crate) fn phase(
    turn: &turn::TurnRecord,
    previous: Option<&turn::TurnRecord>,
    has_history: bool,
) -> &'static str {
    match turn.role.as_str() {
        "planner" => "PLANNING",
        "reviewer" => "REVIEW",
        "executor" | "tester" => match previous {
            Some(old)
                if old.requested.provider != turn.requested.provider
                    || old.requested.requested_model != turn.requested.requested_model =>
            {
                "REASSIGNMENT"
            }
            Some(_) => "REPAIR",
            None if has_history => "EXECUTION_HISTORY_UNKNOWN",
            None => "EXECUTION",
        },
        _ => "OTHER",
    }
}
pub fn price_basis(
    config: &EffectiveRunConfig,
    selected: &ModelAssignment,
) -> Result<Option<PriceBasis>> {
    if selected.model_params.as_ref().is_some_and(|p|p.fast||p.tier.is_some()) {return Ok(None);}
    let pack = planning::effective_pack(config)?;
    let Some(c) = pack.candidates.iter().find(|c| {
        c.provider == selected.provider
            && tokage_matching::assignment::id_matches(&selected.requested_model, &c.cli_model_arg)
    }) else {
        return Ok(None);
    };
    let definition = pack.models.get(&c.model_id);
    if definition.is_some_and(|d| d.price_schedule.is_some()) {
        return Ok(None);
    }
    let rates = definition.and_then(|m| m.cache_rates.as_ref());
    let cache = pack
        .providers
        .get(&selected.provider)
        .and_then(|p| p.cache.as_ref());
    Ok(Some(PriceBasis {
        pack_version: pack.pack_version.clone(),
        config_hash: digest(config)?,
        provider: selected.provider.clone(),
        model: selected.requested_model.clone(),
        calibration_key:Some(crate::calibration::price_key(&pack,c)?),
        calibration_factor:Some(config.planning_policy.as_ref().and_then(|p|p.calibration.iter().find(|v|crate::calibration::price_key(&pack,c).ok().as_ref()==Some(&v.price_key))).map(|v|v.factor).unwrap_or(1.)),
        input: c.price_per_mtok.input,
        output: c.price_per_mtok.output,
        cache_read: rates
            .map(|r| r.read_multiplier)
            .or_else(|| cache.and_then(|c| c.read_multiplier)),
        cache_write: rates
            .map(|r| r.write_multiplier)
            .or_else(|| cache.and_then(|c| c.write_multiplier)),
        long_context: definition.and_then(|d| d.long_context_pricing.clone()),
    }))
}
pub(crate) fn estimate(
    price: Option<&PriceBasis>,
    provider: &str,
    model: &str,
    usage: &TurnUsage,
) -> Result<(Option<f64>, String)> {
    if usage.source == UsageSource::Estimated && finite(usage.cost_usd).is_some() {
        return Ok((finite(usage.cost_usd), "CLI_ESTIMATE_NOT_BILLING".into()));
    }
    if usage.input_tokens == 0
        && usage.cache_read_tokens == 0
        && usage.cache_write_tokens == 0
        && usage.output_tokens == 0
    {
        return Ok((None, "NO_TOKEN_PRICE_BASIS".into()));
    }
    let Some(price) = price else {
        return Ok((None, "PRICE_NOT_FROZEN_OR_UNKNOWN".into()));
    };
    if price.provider != provider || !tokage_matching::assignment::id_matches(model, &price.model) {
        return Ok((None, "OBSERVED_PRICE_IDENTITY_CHANGED".into()));
    }
    let (mut pin, mut pout) = (price.input, price.output);
    let mut basis = "FROZEN_PACK_TOKEN_EQUIVALENT_NOT_BILLING";
    if let Some(long) = &price.long_context {
        if usage.api_calls != Some(1) || usage.context_tokens.is_none() {
            return Ok((None, "MULTI_CALL_OR_CONTEXT_PRICE_TIER_UNKNOWN".into()));
        }
        if usage.context_tokens.unwrap() > long.threshold {
            pin *= long.input_multiplier;
            pout *= long.output_multiplier;
            basis = "FROZEN_PACK_LONG_CONTEXT_EQUIVALENT_NOT_BILLING";
        }
    }
    if usage.cache_read_tokens > 0 && price.cache_read.is_none()
        || usage.cache_write_tokens > 0 && price.cache_write.is_none()
    {
        return Ok((None, "CACHE_PRICE_UNKNOWN".into()));
    }
    let amount = (usage.input_tokens as f64 * pin
        + usage.cache_read_tokens as f64 * pin * price.cache_read.unwrap_or(1.)
        + usage.cache_write_tokens as f64 * pin * price.cache_write.unwrap_or(1.)
        + usage.output_tokens as f64 * pout)
        / 1_000_000.;
    Ok((finite(Some(amount)), basis.into()))
}
pub fn project(s: &Snapshot) -> Result<Projection> {
    let mut receipts = Vec::new();
    for t in s.turns.values() {
        if t.status == turn::TurnStatus::Prepared && !t.ended && t.usage.is_none() {
            continue;
        }
        let provider = t
            .observed_provider
            .as_ref()
            .unwrap_or(&t.requested.provider);
        let model = t
            .observed_model
            .as_ref()
            .unwrap_or(&t.requested.requested_model);
        let repair = t
            .task_id
            .as_ref()
            .and_then(|id| s.execution.as_ref()?.tasks.get(id))
            .is_some_and(|task| {
                task.attempts
                    .iter()
                    .position(|id| id == &t.id)
                    .is_some_and(|i| i > 0)
            });
        let phase = match t.role.as_str() {
            "planner" => "PLANNING",
            "reviewer" => "REVIEW",
            "executor" | "tester" if repair => "REPAIR",
            "executor" | "tester" => "EXECUTION_HISTORY_UNKNOWN",
            _ => "OTHER",
        };
        let attributed = crate::usage_attribution::project(s,t);
        let (estimated_usd, estimate_basis) = match &attributed.usage {
            Some(u) => estimate(t.price_basis.as_ref(), provider, model, u)?,
            None => (None, attributed.basis.clone()),
        };
        receipts.push(Receipt {
            key: format!("turn:{}", t.id),
            origin: "RUNTIME".into(),
            run_id: Some(s.run_id.clone()),
            turn_id: Some(t.id.clone()),
            task_id: t.task_id.clone(),
            provider: provider.clone(),
            model: model.clone(),
            model_basis: if t.observed_model.is_some() {
                "CLI_OBSERVED"
            } else {
                "REQUESTED"
            }
            .into(),
            phase: t.accounting_phase.clone().unwrap_or_else(|| phase.into()),
            status: format!("{:?}", t.status),
            observed_at: t.finished_at.or(t.created_at),
            reported_usd: if t
                .usage
                .as_ref()
                .is_some_and(|u| u.source != UsageSource::Estimated)
            {
                finite(t.cost_usd)
            } else {
                None
            },
            estimated_usd,
            estimate_basis,
            usage: t.usage.clone(),
            attributed_usage: attributed.usage,
            attribution_basis: attributed.basis,
            baseline_turn: attributed.source_turn,
            price_basis: t.price_basis.clone(),
            source: t
                .usage
                .as_ref()
                .map(|u| u.source.as_str())
                .unwrap_or("UNKNOWN")
                .into(),
            evidence_digest: digest(&(s.run_id.as_str(), t))?,
            duplicate_sources: vec![],
        });
    }
    let human_effort = s
        .work
        .as_ref()
        .into_iter()
        .flat_map(|w| w.attempts.iter())
        .filter_map(|(id, a)| {
            a.manual_receipt.as_ref().map(|r| HumanEffort {
                run_id: s.run_id.clone(),
                task_id: s.execution.as_ref().and_then(|e| {
                    e.tasks
                        .iter()
                        .find(|(_, t)| t.attempts.contains(id))
                        .map(|(id, _)| id.clone())
                }),
                attempt_id: id.clone(),
                command_id: r.command_id.clone(),
                reported_seconds: r.effort_seconds,
                recorded_at: r.recorded_at,
            })
        })
        .collect();
    let context_actions=crate::context_audit::project(s,&receipts)?;
    Ok(Projection {receipts,human_effort,context_actions})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn phase_follows_immediate_predecessor_not_any_historical_model() {
        let model = ModelAssignment {
            provider: "CODEX_CLI".into(),
            requested_model: "a".into(),
            provider_path: None,
            capabilities: vec![],
            user_locked: true,
            model_params: None,
        };
        let original =
            turn::TurnRecord::prepared("one".into(), "executor", Some("T1".into()), model);
        let mut reassigned = original.clone();
        reassigned.requested.requested_model = "b".into();
        assert_eq!(phase(&original, None, false), "EXECUTION");
        assert_eq!(phase(&reassigned, Some(&original), true), "REASSIGNMENT");
        assert_eq!(phase(&reassigned, Some(&reassigned), true), "REPAIR");
        assert_eq!(phase(&original, Some(&reassigned), true), "REASSIGNMENT");
        assert_eq!(phase(&original, None, true), "EXECUTION_HISTORY_UNKNOWN");
    }
    #[test]
    fn price_tiers_and_cache_cannot_be_guessed_from_aggregate_usage() {
        let mut p = PriceBasis {
            calibration_key:None,calibration_factor:None,
            pack_version: "v".into(),
            config_hash: "hash".into(),
            provider: "p".into(),
            model: "m".into(),
            input: 2.,
            output: 8.,
            cache_read: Some(0.1),
            cache_write: None,
            long_context: None,
        };
        let mut u = TurnUsage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            cache_read_tokens: 1_000_000,
            cache_write_tokens: 0,
            api_calls: None,
            context_tokens: None,
            cost_usd: None,
            premium_requests: None,
            credits: None,
            source: UsageSource::Structured,
        };
        assert_eq!(estimate(Some(&p), "p", "m", &u).unwrap().0, Some(10.2));
        u.cache_write_tokens = 1;
        assert_eq!(estimate(Some(&p), "p", "m", &u).unwrap().0, None);
        u.cache_write_tokens = 0;
        p.long_context = Some(tokage_matching::pack::LongContextPricingDef {
            threshold: 100,
            input_multiplier: 2.,
            output_multiplier: 3.,
        });
        assert_eq!(estimate(Some(&p), "p", "m", &u).unwrap().0, None);
        u.api_calls = Some(1);
        u.context_tokens = Some(101);
        assert_eq!(estimate(Some(&p), "p", "m", &u).unwrap().0, Some(28.4));
        u.api_calls = Some(2);
        assert_eq!(estimate(Some(&p), "p", "m", &u).unwrap().0, None);
        assert_eq!(estimate(Some(&p), "other", "m", &u).unwrap().0, None);
    }
}
