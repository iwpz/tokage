//! M1.b: auditable inputs for one native window; no compression or dispatch side effects.
use crate::InWindowContextParams;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tokage_matching::merge::EffectiveKnowledgePack;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    pub id: String,
    pub window: String,
    pub provider: String,
    pub model: String,
    pub ended_at: u64,
    #[serde(default)]
    pub completed: bool,
    pub context_tokens: Option<f64>,
    pub output_tokens: Option<f64>,
    pub api_calls: Option<u32>,
    /// Estimate from visible prompt/text/tool content, never cumulative API input usage.
    pub visible_tokens: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputRequest {
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub role: String,
    pub task_id: Option<String>,
    pub revision: Option<u64>,
    #[serde(default)]
    pub history_complete: bool,
    #[serde(default)]
    pub retention: Option<crate::compact::RetentionContext>,
    #[serde(default)]
    pub model_identity: Option<String>,
    pub provider: String,
    pub model: String,
    pub window: String,
    pub observations: Vec<Observation>,
    pub cache_aware: bool,
    pub min_saving: f64,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedInputs {
    /// Populated by the storage-backed command; pure planning has no durable dismissals.
    pub automatic_queue_suppressed: bool,
    pub params: InWindowContextParams,
    pub cooldown: crate::cooldown::Cooldown,
    pub compact_intent: Option<crate::compact::CompactIntent>,
    pub compact_unavailable_reason: Option<String>,
    pub preview: crate::ContextActionPlan,
    pub read_only: bool,
    pub sources: BTreeMap<String, String>,
    pub pack_version: String,
    pub model_id: String,
    pub total_tokens: f64,
    pub sample_count: usize,
    pub warm_now: bool,
    pub cache_ttl_remaining_seconds: f64,
}
/// Planning priors, not measurements. Deliberately conservative and centrally defined.
fn priors(provider: &str) -> (f64, u32, u32, f64, f64) {
    match provider {
        "CLAUDE_CODE" => (12_000., 4, 6, 8_000., 1_000.),
        "CODEX_CLI" => (8_000., 4, 4, 6_000., 1_000.),
        "CURSOR_CLI" => (12_000., 4, 6, 8_000., 1_000.),
        "COPILOT_CLI" => (8_000., 4, 4, 6_000., 1_000.),
        "ANTIGRAVITY_CLI" => (10_000., 4, 6, 8_000., 1_000.),
        "AIDER" => (4_000., 3, 1, 4_000., 1_000.),
        "OPENCODE" | "REASONIX" | "KIRO_CLI" => (8_000., 4, 4, 6_000., 1_000.),
        _ => (8_000., 4, 1, 6_000., 1_000.),
    }
}
fn nonnegative(n: f64) -> bool {
    n.is_finite() && n >= 0.
}
fn mean(values: impl Iterator<Item = f64>) -> Option<f64> {
    let values: Vec<_> = values.filter(|v| nonnegative(*v)).collect();
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}
/// Cache spelling shared with cachePolicy.ts; legacy `standard` remains an alias.
pub fn canonical_cache_type(mode: &str) -> &'static str {
    match mode {
        "standard" | "ephemeral" | "explicit_prefix" | "automatic_prefix" => "ephemeral",
        "disk_persistent" => "disk_persistent",
        "implicit" => "implicit",
        _ => "none",
    }
}
pub fn resolve_inputs(
    pack: &EffectiveKnowledgePack,
    request: &InputRequest,
    now: u64,
) -> Result<ResolvedInputs, String> {
    resolve_inputs_with_history(pack, request, now, &[])
}
pub fn resolve_inputs_with_history(
    pack: &EffectiveKnowledgePack,
    request: &InputRequest,
    now: u64,
    resets: &[crate::cooldown::ResetAction],
) -> Result<ResolvedInputs, String> {
    if request.observations.len() > 512
        || request.window.is_empty()
        || !request.min_saving.is_finite()
        || !(0.0..=1.0).contains(&request.min_saving)
    {
        return Err("Invalid context input request".into());
    }
    let exact: Vec<_> = pack
        .candidates
        .iter()
        .filter(|c| {
            c.provider == request.provider
                && (c.cli_model_arg == request.model || c.model_id == request.model)
        })
        .collect();
    let matches = if exact.is_empty() {
        pack.candidates
            .iter()
            .filter(|c| {
                c.provider == request.provider
                    && tokage_matching::assignment::id_matches(&request.model, &c.cli_model_arg)
            })
            .collect::<Vec<_>>()
    } else {
        exact
    };
    if matches.len() != 1 {
        return Err("Context model is unknown or ambiguous in the effective knowledge pack".into());
    }
    let candidate = matches[0];
    if candidate.context_window == 0
        || candidate.effective_context == 0
        || !nonnegative(candidate.price_per_mtok.input)
        || !nonnegative(candidate.price_per_mtok.output)
    {
        return Err("Context model limits or prices are unavailable".into());
    }
    let definition = pack.models.get(&candidate.model_id);
    let provider = pack.providers.get(&request.provider);
    let mut samples: Vec<_> = request
        .observations
        .iter()
        .filter(|s| {
            s.window == request.window
                && s.provider == request.provider
                && s.model == request.model
                && s.ended_at <= now
        })
        .collect();
    samples.sort_by_key(|s| s.ended_at);
    let mut seen = std::collections::HashSet::new();
    samples.retain(|s| seen.insert(&s.id));
    if samples.iter().any(|s| {
        !nonnegative(s.visible_tokens)
            || s.context_tokens.is_some_and(|v| !nonnegative(v))
            || s.output_tokens.is_some_and(|v| !nonnegative(v))
    }) {
        return Err("Invalid context observation".into());
    }
    let mut cooldown = crate::cooldown::replay(&samples, resets);
    if !request.history_complete {
        cooldown.known = false;
        cooldown.turns_since_last_compact = 0;
        cooldown.remaining_turns = 2;
        cooldown.source = "UNVERIFIED_OR_TRUNCATED_WINDOW_HISTORY";
    }
    let last = samples
        .last()
        .ok_or("No completed observations for this window")?;
    let (baseline, horizon, calls, growth, output) = priors(&request.provider);
    let mut sources: BTreeMap<String, String> = BTreeMap::new();
    let mut set_source = |key: &str, value: &str| {
        sources.insert(key.into(), value.into());
    };
    set_source(
        "modelIdentity",
        if request.model_identity.as_deref() == Some("CLI_OBSERVED") {
            "CLI_OBSERVED"
        } else {
            "REQUESTED_IDENTITY_UNCONFIRMED"
        },
    );
    let total = if let Some(tokens) = last.context_tokens {
        set_source("totalTokens", "CLI_CONTEXT_TOKENS");
        tokens
    } else {
        set_source(
            "totalTokens",
            "VISIBLE_CONTENT_ESTIMATE_PLUS_PROVIDER_BASELINE",
        );
        baseline + samples.iter().map(|s| s.visible_tokens).sum::<f64>()
    };
    if !nonnegative(total) {
        return Err("Context estimate is out of range".into());
    }
    // contextTokens is B+H: never add the baseline again. B/H split remains estimated.
    let baseline = baseline.min(total);
    set_source("baselineTokens", "PROVIDER_BASELINE_PRIOR_CAPPED_AT_TOTAL");
    set_source("historyTokens", "TOTAL_MINUS_ESTIMATED_BASELINE");
    let recent = &samples[samples.len().saturating_sub(8)..];
    let growth_observed =
        mean(
            recent
                .windows(2)
                .filter_map(|w| match (w[0].context_tokens, w[1].context_tokens) {
                    (Some(a), Some(b)) if b >= a => Some(b - a),
                    _ => None,
                }),
        );
    let calls_observed = mean(recent.iter().filter_map(|s| s.api_calls.map(f64::from)));
    let output_observed = mean(recent.iter().filter_map(|s| s.output_tokens));
    set_source(
        "growthPerTurn",
        if growth_observed.is_some() {
            "ROLLING_CONTEXT_DELTA_8"
        } else {
            "PROVIDER_PRIOR"
        },
    );
    set_source(
        "apiCallsPerTurn",
        if calls_observed.is_some() {
            "ROLLING_REPORTED_API_CALLS_8"
        } else {
            "PROVIDER_PRIOR"
        },
    );
    set_source(
        "outputPerTurn",
        if output_observed.is_some() {
            "ROLLING_REPORTED_OUTPUT_8"
        } else {
            "PROVIDER_PRIOR"
        },
    );
    // Completed turns cannot reveal how many turns remain. Keep this a labelled forecast.
    set_source(
        "remainingTurns",
        "PROVIDER_HORIZON_MINUS_COMPLETED_TURNS_FORECAST",
    );
    set_source("contextWindow", "EFFECTIVE_PACK_PROVIDER_MODEL");
    set_source("effectiveContext", "EFFECTIVE_PACK_PROVIDER_MODEL");
    set_source(
        "price",
        &format!(
            "EFFECTIVE_PACK:{}; NOT_ACTUAL_BILLING",
            candidate.origin_layer
        ),
    );
    let context = provider.and_then(|p| p.context.as_ref());
    let auto = context
        .and_then(|c| c.cli_auto_compact_ratio)
        .filter(|n| n.is_finite() && *n > 0. && *n <= 1.);
    let compact = context
        .and_then(|c| c.compact_output_ratio)
        .filter(|n| n.is_finite() && *n > 0. && *n <= 1.);
    set_source(
        "cliAutoCompactRatio",
        if auto.is_some() {
            "EFFECTIVE_PACK_PROVIDER"
        } else {
            "CONSERVATIVE_RATIO_PRIOR"
        },
    );
    set_source(
        "compactOutputRatio",
        if compact.is_some() {
            "EFFECTIVE_PACK_PROVIDER"
        } else {
            "COMPACT_RATIO_PRIOR"
        },
    );
    let cache = provider.and_then(|p| p.cache.as_ref());
    let by_model = cache.is_some_and(|c| c.mode == "by_model");
    // Explicit vendor metadata, never a substring guess from a CLI/model label.
    let vendor_policy = match definition.and_then(|m| m.vendor.as_deref()) {
        Some("anthropic") => ("ephemeral", 300),
        Some("openai") => ("ephemeral", 1800),
        Some("deepseek") => ("disk_persistent", 86400),
        Some("google" | "alibaba") => ("implicit", 0),
        _ => ("none", 0),
    };
    let cache_type = if !request.cache_aware {
        "none"
    } else if by_model {
        vendor_policy.0
    } else {
        canonical_cache_type(cache.map(|c| c.mode.as_str()).unwrap_or("none"))
    };
    let cache_ttl = if by_model {
        vendor_policy.1
    } else {
        cache.and_then(|c| c.ttl_seconds).unwrap_or(0)
    };
    let rates = definition.and_then(|m| m.cache_rates.as_ref());
    let read = rates
        .map(|r| r.read_multiplier)
        .or_else(|| cache.and_then(|c| c.read_multiplier))
        .filter(|n| nonnegative(*n))
        .unwrap_or(1.);
    let write = rates
        .map(|r| r.write_multiplier)
        .or_else(|| cache.and_then(|c| c.write_multiplier))
        .filter(|n| nonnegative(*n))
        .unwrap_or(1.);
    set_source(
        "cacheRates",
        if rates.is_some() {
            "EFFECTIVE_PACK_MODEL"
        } else if cache.is_some() {
            "EFFECTIVE_PACK_PROVIDER_OR_NEUTRAL"
        } else {
            "NEUTRAL_NO_CACHE_DISCOUNT"
        },
    );
    set_source(
        "cachePolicy",
        if !request.cache_aware {
            "USER_DISABLED"
        } else if by_model {
            "PACK_MODEL_VENDOR_POLICY_PRIOR"
        } else {
            "EFFECTIVE_PACK_PROVIDER_OR_NONE"
        },
    );
    set_source("turnsSinceLastCompact", cooldown.source);
    let mut p_in = candidate.price_per_mtok.input;
    let mut p_out = candidate.price_per_mtok.output;
    if let Some(pricing) = definition
        .and_then(|m| m.long_context_pricing.as_ref())
        .filter(|p| total > p.threshold as f64)
    {
        if !nonnegative(pricing.input_multiplier) || !nonnegative(pricing.output_multiplier) {
            return Err("Invalid long-context prices".into());
        }
        p_in *= pricing.input_multiplier;
        p_out *= pricing.output_multiplier;
        set_source(
            "price",
            "EFFECTIVE_PACK_LONG_CONTEXT_TIER; NOT_ACTUAL_BILLING",
        );
    }
    let params = InWindowContextParams {
        baseline_tokens: baseline,
        history_tokens: total - baseline,
        remaining_turns: horizon
            .saturating_sub(samples.len().min(u32::MAX as usize) as u32)
            .max(1),
        api_calls_per_turn: calls_observed
            .map(|v| v.ceil().min(u32::MAX as f64) as u32)
            .unwrap_or(calls),
        growth_per_turn: growth_observed.unwrap_or(growth),
        output_per_turn: output_observed.unwrap_or(output),
        compact_output_ratio: compact.unwrap_or(0.06),
        rework_phi: 0.3,
        min_saving: request.min_saving,
        cache_read_multiplier: if cache_type == "none" { 1. } else { read },
        cache_write_multiplier: if cache_type == "none" { 1. } else { write },
        cache_ttl_seconds: if cache_type == "none" {
            0.
        } else {
            cache_ttl as f64
        },
        cache_type: cache_type.into(),
        implicit_hit_reliability: 0.,
        elapsed_seconds_since_last_turn: now.saturating_sub(last.ended_at) as f64,
        effective_context: candidate.effective_context.min(candidate.context_window) as f64,
        context_window: candidate.context_window as f64,
        cli_auto_compact_ratio: auto.unwrap_or(0.92),
        turns_since_last_compact: cooldown.turns_since_last_compact,
        p_in,
        p_out,
    };
    if [
        params.growth_per_turn,
        params.output_per_turn,
        params.p_in,
        params.p_out,
    ]
    .iter()
    .any(|v| !nonnegative(*v))
    {
        return Err("Context forecast or prices are out of range".into());
    }
    for (field, group) in [
        ("pIn", "price"),
        ("pOut", "price"),
        ("cacheReadMultiplier", "cacheRates"),
        ("cacheWriteMultiplier", "cacheRates"),
        ("cacheType", "cachePolicy"),
        ("cacheTtlSeconds", "cachePolicy"),
    ] {
        let origin = sources[group].clone();
        sources.insert(field.into(), origin);
    }
    sources.insert(
        "elapsedSecondsSinceLastTurn".into(),
        "LAST_COMPLETED_TURN_TIMESTAMP".into(),
    );
    sources.insert("implicitHitReliability".into(), "UNVERIFIED_ZERO".into());
    sources.insert("minSaving".into(), "USER_SETTING".into());
    sources.insert(
        "reworkPhi".into(),
        "REWORK_PRIOR_M3_CALIBRATION_PENDING".into(),
    );
    let (warm_now, cache_ttl_remaining_seconds) = crate::is_cache_warm_now(
        params.elapsed_seconds_since_last_turn,
        params.cache_ttl_seconds,
        &params.cache_type,
        params.implicit_hit_reliability,
    );
    let mut preview = crate::plan_context_action(&params);
    if !cooldown.known {
        preview.explanation =
            "Context action history is incomplete; non-CONTINUE actions remain unavailable".into();
    }
    if [
        preview.continue_cost_usd,
        preview.compact_cost_usd,
        preview.fresh_cost_usd,
        preview.estimated_saving_percentage,
    ]
    .iter()
    .any(|v| !v.is_finite())
    {
        return Err("Context cost preview is out of range".into());
    }
    let prepared = request
        .retention
        .clone()
        .ok_or("Current retention context unavailable".to_string())
        .and_then(|retained| {
            if retained.turn_id != last.id
                || retained.session_id != request.session_id
                || retained.role != request.role
                || retained.task_id != request.task_id
                || retained.revision != request.revision
            {
                return Err("Compact source turn is stale".into());
            }
            let scope = crate::compact::CompactScope {
                session_id: request.session_id.clone(),
                turn_id: retained.turn_id.clone(),
                window: request.window.clone(),
                provider_name: request.provider.clone(),
                model: request.model.clone(),
                role: request.role.clone(),
                task_id: request.task_id.clone(),
                revision: request.revision,
            };
            crate::compact::build_compact_intent(scope, retained)
        });
    let (compact_intent, compact_unavailable_reason) = match prepared {
        Ok(intent) => (Some(intent), None),
        Err(reason) => (None, Some(reason)),
    };
    Ok(ResolvedInputs {
        automatic_queue_suppressed: false,
        preview,
        read_only: true,
        compact_intent,
        compact_unavailable_reason,
        cooldown,
        warm_now,
        cache_ttl_remaining_seconds,
        params,
        sources,
        pack_version: pack.pack_version.clone(),
        model_id: candidate.model_id.clone(),
        total_tokens: total,
        sample_count: recent.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokage_matching::{builtin_knowledge_pack, merge_layers, LayeredPacks};
    fn fixture() -> (EffectiveKnowledgePack, InputRequest) {
        let pack = merge_layers(&LayeredPacks {
            builtin: builtin_knowledge_pack().clone(),
            official: None,
            customs: vec![],
            overrides: vec![],
            learned_usage_profiles: None,
        })
        .unwrap();
        let c = pack
            .candidates
            .iter()
            .find(|c| c.provider == "CLAUDE_CODE" && c.cli_model_arg == "sonnet")
            .unwrap();
        let request = InputRequest {
            session_id: "session-fixture".into(),
            role: "planner".into(),
            task_id: None,
            revision: None,
            history_complete: true,
            retention: None,
            model_identity: None,
            provider: c.provider.clone(),
            model: c.cli_model_arg.clone(),
            window: "thread-a".into(),
            observations: vec![],
            cache_aware: true,
            min_saving: 0.15,
        };
        (pack, request)
    }
    fn sample(r: &InputRequest, id: &str, time: u64, context: Option<f64>) -> Observation {
        Observation {
            id: id.into(),
            window: r.window.clone(),
            provider: r.provider.clone(),
            model: r.model.clone(),
            ended_at: time,
            completed: true,
            context_tokens: context,
            visible_tokens: 100.,
            output_tokens: Some(10.),
            api_calls: Some(2),
        }
    }
    #[test]
    fn aider_warm_single_remaining_turn_does_not_justify_compaction() {
        // Reconstruct the documented final totals and reported output/call samples.
        // Visible text per turn was not archived; only its sum is known. Its
        // distribution does not enter the forecast when contextTokens is absent.
        let (pack, mut request) = fixture();
        request.provider = "AIDER".into();
        request.model = "deepseek/deepseek-flash".into();
        for (index, output) in [17000., 12000., 15000., 11000., 9600., 1800., 5400., 3700.].into_iter().enumerate() {
            let mut observation = sample(&request, &format!("turn-{index}"), index as u64 + 1, None);
            observation.visible_tokens = (92947. - 4000.) / 8.;
            observation.output_tokens = Some(output);
            observation.api_calls = Some(1);
            request.observations.push(observation);
        }
        let resolved = resolve_inputs(&pack, &request, 9).unwrap();
        assert_eq!(resolved.total_tokens, 92947.);
        assert_eq!(resolved.params.remaining_turns, 1);
        assert_eq!(resolved.params.api_calls_per_turn, 1);
        assert_eq!(resolved.params.growth_per_turn, 4000.);
        assert!(resolved.warm_now);
        assert!(resolved.cooldown.known);
        assert!(!resolved.preview.continue_disqualified);
        assert!(!resolved.preview.cooldown_blocked);
        assert!(resolved.preview.continue_cost_usd < resolved.preview.compact_cost_usd);
        assert!(resolved.preview.continue_cost_usd < resolved.preview.fresh_cost_usd);
        assert_eq!(resolved.preview.recommended_action, crate::ContextActionKind::Continue);
    }

    #[test]
    fn cli_total_is_not_double_counted_and_zero_is_preserved() {
        let (pack, mut r) = fixture();
        r.observations.push(sample(&r, "a", 1, Some(0.)));
        r.observations[0].api_calls = Some(0);
        r.observations[0].output_tokens = Some(0.);
        let zero = resolve_inputs(&pack, &r, 2).unwrap();
        assert_eq!(zero.params.api_calls_per_turn, 0);
        assert_eq!(zero.params.output_per_turn, 0.);
        assert_eq!(zero.total_tokens, 0.);
        assert_eq!(zero.params.baseline_tokens, 0.);
        assert_eq!(zero.params.history_tokens, 0.);
        r.observations[0].context_tokens = Some(50_000.);
        let p = resolve_inputs(&pack, &r, 2).unwrap();
        assert_eq!(p.params.baseline_tokens + p.params.history_tokens, 50_000.);
        assert_eq!(p.sources["totalTokens"], "CLI_CONTEXT_TOKENS");
        r.observations[0].context_tokens = None;
        let fallback = resolve_inputs(&pack, &r, 2).unwrap();
        assert_eq!(fallback.total_tokens, 12_100.);
        assert!(fallback.sources["totalTokens"].contains("ESTIMATE"));
    }
    #[test]
    fn exact_channel_effective_limits_prices_and_model_cache_rates_win() {
        let (mut pack, mut r) = fixture();
        r.observations.push(sample(&r, "a", 1, Some(50_000.)));
        let c = pack
            .candidates
            .iter_mut()
            .find(|c| c.provider == r.provider && c.cli_model_arg == r.model)
            .unwrap();
        c.context_window = 77_000;
        c.effective_context = 65_000;
        c.price_per_mtok.input = 17.;
        c.price_per_mtok.output = 29.;
        let id = c.model_id.clone();
        pack.models.get_mut(&id).unwrap().cache_rates = Some(tokage_matching::CacheRatesDef {
            read_multiplier: 0.2,
            write_multiplier: 1.3,
        });
        let p = resolve_inputs(&pack, &r, 2).unwrap().params;
        assert_eq!(
            (p.context_window, p.effective_context, p.p_in, p.p_out),
            (77_000., 65_000., 17., 29.)
        );
        assert_eq!(
            (p.cache_read_multiplier, p.cache_write_multiplier),
            (0.2, 1.3)
        );
        assert_eq!(p.cache_type, "ephemeral");
        assert_eq!(p.cache_ttl_seconds, 300.);
        r.model = "not-a-real-model".into();
        assert!(resolve_inputs(&pack, &r, 2).is_err());
    }
    #[test]
    fn rolling_stats_ignore_other_windows_duplicates_future_and_reset_deltas() {
        let (pack, mut r) = fixture();
        for i in 0..12 {
            let mut o = sample(&r, &i.to_string(), i + 1, Some(20_000. + i as f64 * 100.));
            o.api_calls = Some(i as u32);
            r.observations.push(o);
        }
        let mut other = sample(&r, "other", 15, Some(9_000_000.));
        other.window = "other".into();
        r.observations.push(other);
        r.observations.push(r.observations[0].clone());
        r.observations
            .push(sample(&r, "future", 1000, Some(99_000.)));
        let p = resolve_inputs(&pack, &r, 20).unwrap();
        assert_eq!(p.sample_count, 8);
        assert_eq!(p.total_tokens, 21_100.);
        assert_eq!(p.params.growth_per_turn, 100.);
        assert_eq!(p.params.api_calls_per_turn, 8);
        assert!(p.sources["remainingTurns"].contains("FORECAST"));
        r.observations = vec![
            sample(&r, "a", 1, Some(40_000.)),
            sample(&r, "b", 2, Some(10_000.)),
        ];
        assert_eq!(
            resolve_inputs(&pack, &r, 20).unwrap().sources["growthPerTurn"],
            "PROVIDER_PRIOR"
        );
    }
    #[test]
    fn long_context_tier_and_invalid_observations_do_not_invent_prices() {
        let (mut pack, mut r) = fixture();
        r.observations.push(sample(&r, "a", 1, Some(50_000.)));
        let candidate = pack
            .candidates
            .iter()
            .find(|c| c.provider == r.provider && c.cli_model_arg == r.model)
            .unwrap();
        let (id, price) = (candidate.model_id.clone(), candidate.price_per_mtok.input);
        pack.models.get_mut(&id).unwrap().long_context_pricing =
            Some(tokage_matching::pack::LongContextPricingDef {
                threshold: 40_000,
                input_multiplier: 2.,
                output_multiplier: 1.5,
            });
        let resolved = resolve_inputs(&pack, &r, 2).unwrap();
        assert_eq!(resolved.params.p_in, price * 2.);
        assert!(resolved.read_only);
        assert_eq!(resolved.params.turns_since_last_compact, 1);
        r.observations[0].visible_tokens = f64::NAN;
        assert!(resolve_inputs(&pack, &r, 2).is_err());
    }

    #[tokio::test]
    async fn accepted_history_replays_after_reopen_without_counting_queries_as_turns() {
        use tokage_storage::{ContextActionRow, Storage};
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let id = storage
            .record_context_action(ContextActionRow {
                id: None,
                attempt_id: "a".into(),
                kind: "COMPACT".into(),
                trigger: "USER".into(),
                status: "SUGGESTED".into(),
                est_costs_json: "{}".into(),
                est_saving_pct: 0.,
                actual_saving_usd: None,
                new_attempt_id: None,
                created_at: None,
                applied_at: None,
            })
            .await
            .unwrap();
        storage
            .update_context_action_status(id, "APPLIED".into(), None)
            .await
            .unwrap();
        storage
            .get_conn()
            .unwrap()
            .execute(
                "UPDATE context_actions SET applied_at='1970-01-01T00:00:01.500Z' WHERE id=?1",
                [id],
            )
            .unwrap();
        drop(storage);
        let storage = Storage::open(dir.path()).unwrap();
        let conn = storage.get_conn().unwrap();
        let rows = tokage_storage::context::list_context_reset_actions(
            &conn,
            &["a".into(), "b".into(), "c".into()],
        )
        .unwrap();
        let resets: Vec<_> = rows
            .into_iter()
            .map(|r| crate::cooldown::ResetAction {
                action_id: r.action_id,
                attempt_id: r.attempt_id,
                new_attempt_id: r.new_attempt_id,
                applied_at_ms: r.applied_at_ms.unwrap() as u64,
            })
            .collect();
        let (pack, mut request) = fixture();
        request.observations = vec![
            sample(&request, "a", 1, Some(20_000.)),
            sample(&request, "b", 2, Some(21_000.)),
        ];
        for _ in 0..3 {
            let result = resolve_inputs_with_history(&pack, &request, 4, &resets).unwrap();
            assert_eq!(result.params.turns_since_last_compact, 1);
            assert_eq!(result.preview.cooldown_remaining_turns, 1);
        }
        request
            .observations
            .push(sample(&request, "c", 3, Some(22_000.)));
        let result = resolve_inputs_with_history(&pack, &request, 4, &resets).unwrap();
        assert_eq!(result.params.turns_since_last_compact, 2);
        assert_eq!(result.cooldown.last_reset_action_id, Some(id));
        assert_eq!(
            tokage_storage::context::list_context_actions(&conn, "a")
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn incomplete_history_never_unlocks_cooldown_from_partial_completed_turns() {
        let (pack, mut request) = fixture();
        request.observations = vec![
            sample(&request, "a", 1, Some(20_000.)),
            sample(&request, "b", 2, Some(21_000.)),
        ];
        request.history_complete = false;
        let unknown = resolve_inputs(&pack, &request, 3).unwrap();
        assert!(!unknown.cooldown.known);
        assert_eq!(unknown.params.turns_since_last_compact, 0);
        request.history_complete = true;
        let complete = resolve_inputs(&pack, &request, 3).unwrap();
        assert!(complete.cooldown.known);
        assert_eq!(complete.params.turns_since_last_compact, 2);
    }

    #[test]
    fn compact_preparation_is_scoped_and_stale_retention_does_not_erase_valid_metrics() {
        let (pack, mut request) = fixture();
        request.observations = vec![sample(&request, "a", 1, Some(20_000.))];
        request.retention = Some(crate::compact::RetentionContext {
            session_id: request.session_id.clone(),
            turn_id: "a".into(),
            role: request.role.clone(),
            task_id: None,
            revision: None,
            task_goal: "Current goal".into(),
            goal_source: "RUN_REQUIREMENT".into(),
            contract_paths: vec![],
            contracts: vec![],
            pending_errors: vec!["Current error".into()],
        });
        let ready = resolve_inputs(&pack, &request, 2).unwrap();
        assert_eq!(
            ready.compact_intent.unwrap().scope.provider_name,
            request.provider
        );
        request.retention.as_mut().unwrap().turn_id = "stale".into();
        let stale = resolve_inputs(&pack, &request, 2).unwrap();
        assert!(stale.compact_intent.is_none());
        assert!(stale.compact_unavailable_reason.is_some());
        assert_eq!(stale.total_tokens, 20_000.);
    }

    #[test]
    fn accepted_main_turn_revalidates_planner_cooldown_and_regenerates_payload() {
        use crate::compact::*;
        let (pack, mut request) = fixture();
        request.observations = vec![sample(&request, "a", 1, Some(20_000.))];
        let mut resolved = resolve_inputs(&pack, &request, 2).unwrap();
        // Isolate the acceptance guard; economic planner coverage remains in in_window tests.
        resolved.cooldown.known = true;
        resolved.preview.recommended_action = crate::ContextActionKind::Compact;
        resolved.preview.cooldown_blocked = false;
        resolved.preview.cooldown_remaining_turns = 0;
        for (provider, fresh) in [("CURSOR_CLI", false), ("CODEX_CLI", true)] {
            let scope = CompactScope {
                session_id: "s".into(),
                turn_id: "a".into(),
                window: "w".into(),
                provider_name: provider.into(),
                model: "current".into(),
                role: "direct".into(),
                task_id: None,
                revision: None,
            };
            let retained = RetentionContext {
                session_id: "s".into(),
                turn_id: "a".into(),
                role: "direct".into(),
                task_id: None,
                revision: None,
                task_goal: "goal".into(),
                goal_source: "SESSION_FIRST_USER_MESSAGE".into(),
                contract_paths: vec![],
                contracts: vec![],
                pending_errors: vec![],
            };
            let intent = build_compact_intent(scope, retained).unwrap();
            resolved.compact_intent = Some(intent.clone());
            let mut expected = intent.clone();
            expected.next_turn = NextTurnIntent::SameWindowPrompt {
                prefix: "CALLER_OVERRIDE".into(),
            };
            let draft = prepare_accepted_main_turn(&resolved, &expected, " user ").unwrap();
            assert_eq!(draft.user_message, " user ");
            assert_eq!(draft.require_new_window, fresh);
            assert!(!draft.context_prefix.contains("CALLER_OVERRIDE"));
            expected.retained.task_goal = "changed".into();
            assert!(prepare_accepted_main_turn(&resolved, &expected, "user").is_err());
            resolved.cooldown.known = false;
            assert!(prepare_accepted_main_turn(&resolved, &intent, "user").is_err());
            resolved.cooldown.known = true;
            resolved.preview.cooldown_remaining_turns = 1;
            assert!(prepare_accepted_main_turn(&resolved, &intent, "user").is_err());
            resolved.preview.cooldown_remaining_turns = 0;
            resolved.preview.recommended_action = crate::ContextActionKind::Continue;
            assert!(prepare_accepted_main_turn(&resolved, &intent, "user").is_err());
            resolved.preview.recommended_action = crate::ContextActionKind::Compact;
        }
    }

    #[test]
    fn cache_disabled_unknown_mode_and_exact_ttl_boundary_are_cold() {
        let (mut pack, mut r) = fixture();
        r.observations.push(sample(&r, "a", 10, Some(0.)));
        assert!(resolve_inputs(&pack, &r, 279).unwrap().warm_now);
        assert!(!resolve_inputs(&pack, &r, 280).unwrap().warm_now);
        r.cache_aware = false;
        let off = resolve_inputs(&pack, &r, 11).unwrap();
        assert!(!off.warm_now);
        assert_eq!(off.params.cache_read_multiplier, 1.);
        r.cache_aware = true;
        pack.providers
            .get_mut(&r.provider)
            .unwrap()
            .cache
            .as_mut()
            .unwrap()
            .mode = "unknown".into();
        assert!(!resolve_inputs(&pack, &r, 11).unwrap().warm_now);
        assert_eq!(crate::is_cache_warm_now(1., 300., "none", 1.), (false, 0.));
        assert_eq!(
            crate::is_cache_warm_now(1000., 300., "disk_persistent", 1.),
            (false, 0.)
        );
        assert!(!crate::is_cache_warm_now(1., 300., "implicit", 0.).0);
    }
}
