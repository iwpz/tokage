//! Auditable forecasts for the shared scheduler, never bill or actual-saving evidence.
use crate::*;
use tokage_context::reuse::{WindowReuseParams, RELEVANT_HISTORY_CAP_PRIOR};
use tokage_matching::{merge::EffectiveKnowledgePack, pack::UsageProfileEntry};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Resolved {
    #[serde(flatten)]
    pub params: WindowReuseParams,
    pub sources: BTreeMap<String, String>,
    pub source_turn_ids: Vec<String>,
    pub pack_version: String,
    pub config_hash: String,
    pub context_options: ContextOptions,
    pub history_complete: bool,
    pub estimate_basis: &'static str,
    pub unavailable_reason: Option<String>,
}
pub(crate) fn resolve(
    s: &Snapshot,
    task_id: &str,
    old: &tokage_core::planning::TaskItem,
    previous: &turn::TurnRecord,
    pack: &EffectiveKnowledgePack,
    now: u64,
) -> Result<Resolved> {
    let target = s
        .plan
        .as_ref()
        .and_then(|p| p.plan.tasks.iter().find(|t| t.id == task_id))
        .ok_or(Error::Transition)?;
    let assigned = s
        .execution
        .as_ref()
        .and_then(|e| e.tasks.get(task_id))
        .ok_or(Error::Transition)?;
    if assigned
        .assignment
        .model_params
        .as_ref()
        .is_some_and(|p| p.fast || p.tier.is_some())
    {
        return Err(Error::Invalid(
            "Selected fast/tier pricing is unverified".into(),
        ));
    }
    let request = task_context::inputs(s, &previous.id, &old.id, now)?;
    let input =
        tokage_context::inputs::resolve_inputs(pack, &request, now).map_err(Error::Invalid)?;
    let exact = pack
        .candidates
        .iter()
        .filter(|c| {
            c.provider == assigned.assignment.provider
                && (c.cli_model_arg == assigned.assignment.requested_model
                    || c.model_id == assigned.assignment.requested_model)
        })
        .collect::<Vec<_>>();
    let candidates = if exact.is_empty() {
        pack.candidates
            .iter()
            .filter(|c| {
                c.provider == assigned.assignment.provider
                    && tokage_matching::assignment::id_matches(
                        &assigned.assignment.requested_model,
                        &c.cli_model_arg,
                    )
            })
            .collect::<Vec<_>>()
    } else {
        exact
    };
    if candidates.len() != 1 {
        return Err(Error::Invalid(
            "Unknown or ambiguous provider/model prices".into(),
        ));
    }
    let candidate = candidates[0];
    let definition = pack.models.get(&candidate.model_id);
    if definition.is_some_and(|m| m.price_schedule.is_some()) {
        return Err(Error::Invalid(
            "Dynamic price schedule is unverified".into(),
        ));
    }
    let (_, horizon, calls, growth, output) =
        tokage_context::inputs::priors(&assigned.assignment.provider);
    let size = match target.size {
        tokage_core::planning::TaskSize::S => "S",
        tokage_core::planning::TaskSize::M => "M",
        tokage_core::planning::TaskSize::L => "L",
    };
    let fallback = UsageProfileEntry {
        turns: horizon,
        calls_per_turn: calls,
        growth_per_turn: growth as u64,
        output_per_turn: output as u64,
    };
    let profile = pack
        .usage_profiles
        .get(&target.domain)
        .and_then(|sizes| sizes.get(size))
        .unwrap_or(&fallback);
    let profile_origin = if pack
        .usage_profiles
        .get(&target.domain)
        .and_then(|sizes| sizes.get(size))
        .is_some()
    {
        "FROZEN_EFFECTIVE_PACK_DOMAIN_SIZE_PROFILE_FORECAST"
    } else {
        "PROVIDER_PRIOR_FORECAST"
    };
    let mut sources = input.sources;
    let mut forecast = |key: &str, observed: f64, prior: f64| {
        if sources.get(key).is_some_and(|v| v == "PROVIDER_PRIOR")
            || old.domain != target.domain
            || old.size != target.size
        {
            sources.insert(key.into(), profile_origin.into());
            prior
        } else {
            observed
        }
    };
    let growth = forecast(
        "growthPerTurn",
        input.params.growth_per_turn,
        profile.growth_per_turn as f64,
    );
    let calls = forecast(
        "apiCallsPerTurn",
        input.params.api_calls_per_turn as f64,
        profile.calls_per_turn as f64,
    )
    .max(1.) as u32;
    let output = forecast(
        "outputPerTurn",
        input.params.output_per_turn,
        profile.output_per_turn as f64,
    );
    sources.insert("expectedTurnsB".into(), profile_origin.into());
    sources.insert(
        "briefTokensB".into(),
        "EXACT_EXECUTION_PROMPT_UTF8_BYTES_DIV_4_ESTIMATE; CLI_FRAMING_IN_BASELINE_PRIOR".into(),
    );
    sources.insert(
        "historyTokensA".into(),
        "WINDOW_TOTAL_MINUS_CAPPED_ESTIMATED_BASELINE; NOT_CUMULATIVE_INPUT".into(),
    );
    sources.insert(
        "relevantHistoryTokensA".into(),
        "BOUNDED_RELEVANCE_PRIOR; NOT_SEMANTIC_MEASUREMENT".into(),
    );
    sources.insert(
        "reworkPhi".into(),
        "REWORK_PRIOR_CALIBRATION_PENDING".into(),
    );
    let options = s.config.context_options.clone().unwrap_or_default();
    let option_origin = if s.config.context_options.is_some() {
        "FROZEN_RUN_CONTEXT_OPTIONS"
    } else {
        "LEGACY_CONFIG_DEFAULT"
    };
    sources.insert("minSaving".into(), option_origin.into());
    sources.insert("cacheAware".into(), option_origin.into());
    sources.insert(
        "price".into(),
        format!(
            "EFFECTIVE_PACK:{}; PER_ALTERNATIVE_CONTEXT_TIER; NOT_ACTUAL_BILLING",
            candidate.origin_layer
        ),
    );
    sources.insert(
        "pIn".into(),
        "EFFECTIVE_PACK_BASE_PRICE; NOT_ACTUAL_BILLING".into(),
    );
    sources.insert(
        "pOut".into(),
        "EFFECTIVE_PACK_BASE_PRICE; NOT_ACTUAL_BILLING".into(),
    );
    sources.insert(
        "priceQuality".into(),
        if definition.and_then(|m| m.price_estimated) == Some(true) {
            "MODEL_PRICE_ESTIMATE"
        } else {
            "PACK_DECLARED_PRICE_NOT_INVOICE"
        }
        .into(),
    );
    let params = WindowReuseParams {
        model_a: previous.requested.requested_model.clone(),
        model_b: assigned.assignment.requested_model.clone(),
        role_a: previous.role.clone(),
        role_b: "executor".into(),
        domain_a: old.domain.clone(),
        domain_b: target.domain.clone(),
        write_scope_a: old.write_scope.clone(),
        write_scope_b: target.write_scope.clone(),
        baseline_tokens: input.params.baseline_tokens,
        history_tokens_a: input.params.history_tokens,
        relevant_history_tokens_a: Some(
            input.params.history_tokens.min(RELEVANT_HISTORY_CAP_PRIOR),
        ),
        brief_tokens_b: task_prompt::build(s, task_id)?.len() as f64 / 4.,
        expected_turns_b: profile.turns,
        api_calls_per_turn: calls,
        growth_per_turn: growth,
        output_per_turn: output,
        elapsed_seconds: input.params.elapsed_seconds_since_last_turn,
        cache_ttl_seconds: input.params.cache_ttl_seconds,
        effective_context: input.params.effective_context,
        context_window: input.params.context_window,
        cli_auto_compact_ratio: input.params.cli_auto_compact_ratio,
        rework_phi: tokage_context::reuse::REWORK_PHI_PRIOR,
        min_saving: options.min_saving,
        p_in: candidate.price_per_mtok.input,
        p_out: candidate.price_per_mtok.output,
        cache_read_multiplier: input.params.cache_read_multiplier,
        cache_write_multiplier: input.params.cache_write_multiplier,
        long_context_pricing: definition.and_then(|m| m.long_context_pricing.clone()),
    };
    let known_context = previous.context_metrics_version == Some(2)
        && previous.usage.as_ref().is_some_and(|u| {
            u.source != tokage_core::UsageSource::Estimated && u.context_tokens.is_some()
        });
    let unavailable_reason = if !known_context {
        Some("Native context usage unverified; estimates cannot authorize reuse".into())
    } else if !request.history_complete {
        Some("Window history incomplete; fresh window required".into())
    } else if !options.cache_aware {
        Some("Frozen context settings disable cache-aware reuse".into())
    } else if matches!(input.params.cache_type.as_str(), "implicit" | "none") {
        Some("Cache warmth unavailable for this provider/model policy".into())
    } else {
        None
    };
    Ok(Resolved {
        params,
        sources,
        source_turn_ids: request.observations.iter().map(|o| o.id.clone()).collect(),
        pack_version: pack.pack_version.clone(),
        config_hash: s.config_hash.clone(),
        context_options: options,
        history_complete: request.history_complete,
        estimate_basis: "TOKEN_EQUIVALENT_FORECAST_NOT_BILLING_OR_MEASURED_SAVINGS",
        unavailable_reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Snapshot {
        let (mut s, _) = task_context::tests::fixture();
        let plan = &mut s.plan.as_mut().unwrap().plan;
        plan.tasks[0].write_scope = vec!["src/one/**".into()];
        let mut next = plan.tasks[0].clone();
        next.id = "T2".into();
        next.title = "Next task".into();
        next.goal = "Preserve the next task contract".into();
        next.write_scope = vec!["src/two/**".into()];
        next.depends_on = vec!["T1".into()];
        plan.tasks.push(next);
        s.config.context_options = Some(ContextOptions {
            cache_aware: true,
            min_saving: 0.,
        });
        freeze(&mut s);
        s.execution = Some(execution::Execution::new(&s).unwrap());
        let t = s.execution.as_mut().unwrap().tasks.get_mut("T1").unwrap();
        t.state = tokage_orchestration::TaskState::Done;
        t.attempts = vec!["a".into(), "b".into()];
        s.turns
            .get_mut("a")
            .unwrap()
            .usage
            .as_mut()
            .unwrap()
            .context_tokens = Some(10_000);
        s.turns
            .get_mut("b")
            .unwrap()
            .usage
            .as_mut()
            .unwrap()
            .context_tokens = Some(20_000);
        s.state = RunState::Executing;
        s
    }
    fn freeze(s: &mut Snapshot) {
        s.config_hash = digest(&s.config).unwrap();
        let text =
            tokage_planning::storage::render_full_plan_markdown(&s.plan.as_ref().unwrap().plan);
        s.plan = Some(planning::normalize(&text, &s.config).unwrap());
        s.approved_digest = Some(s.plan.as_ref().unwrap().confirmation_digest.clone());
    }
    fn resolved(s: &Snapshot, pack: &EffectiveKnowledgePack, now: u64) -> Resolved {
        resolve(
            s,
            "T2",
            &s.plan.as_ref().unwrap().plan.tasks[0],
            &s.turns["b"],
            pack,
            now,
        )
        .unwrap()
    }
    #[test]
    fn scheduler_uses_window_observations_exact_prompt_and_frozen_choices() {
        let mut s = fixture();
        let pack = planning::effective_pack(&s.config).unwrap();
        let r = resolved(&s, &pack, 30);
        assert_eq!(
            r.params.baseline_tokens + r.params.history_tokens_a,
            20_000.
        );
        assert_eq!(
            r.params.brief_tokens_b,
            task_prompt::build(&s, "T2").unwrap().len() as f64 / 4.
        );
        assert_eq!(r.params.growth_per_turn, 10_000.);
        assert_eq!(r.params.api_calls_per_turn, 1);
        assert!(r.sources["outputPerTurn"].contains("PROFILE_FORECAST"));
        assert!(r.sources["growthPerTurn"].contains("ROLLING"));
        assert_eq!(r.sources["minSaving"], "FROZEN_RUN_CONTEXT_OPTIONS");
        assert_eq!(r.source_turn_ids, vec!["a", "b"]);
        assert!(r.unavailable_reason.is_none());
        let reservations = execution::reserve_ready(&mut s, 30).unwrap();
        assert_eq!(reservations.len(), 1);
        assert_eq!(reservations[0].task_id, "T2");
        assert_eq!(
            reservations[0].resume_turn.as_deref(),
            Some("b"),
            "{:?}",
            s.execution
        );
        let stored = s.execution.as_ref().unwrap().tasks["T2"]
            .context
            .as_ref()
            .unwrap()
            .inputs
            .as_ref()
            .unwrap();
        assert_eq!(stored["briefTokensB"], r.params.brief_tokens_b);
        assert_eq!(stored["configHash"], s.config_hash);
        assert!(stored["costEstimates"].is_object());
    }
    #[test]
    fn aggregate_or_estimated_usage_never_becomes_context_and_zero_stays_zero() {
        let mut s = fixture();
        let pack = planning::effective_pack(&s.config).unwrap();
        for t in s.turns.values_mut() {
            t.visible_prompt_bytes = Some(4000);
            let u = t.usage.as_mut().unwrap();
            u.input_tokens = 99_000_000;
            u.cache_read_tokens = 90_000_000;
            u.context_tokens = None;
        }
        let r = resolved(&s, &pack, 30);
        assert!(r
            .unavailable_reason
            .as_ref()
            .unwrap()
            .contains("unverified"));
        assert!(r.params.history_tokens_a < 100_000.);
        let reserved = execution::reserve_ready(&mut s, 30).unwrap();
        assert!(reserved[0].resume_turn.is_none());
        let mut legacy = fixture();
        legacy.turns.get_mut("b").unwrap().context_metrics_version = None;
        assert!(resolved(&legacy, &pack, 30).unavailable_reason.is_some());
        let mut s = fixture();
        let u = s.turns.get_mut("b").unwrap().usage.as_mut().unwrap();
        u.context_tokens = Some(0);
        let r = resolved(&s, &pack, 30);
        assert_eq!(r.params.baseline_tokens + r.params.history_tokens_a, 0.);
        let u = s.turns.get_mut("b").unwrap().usage.as_mut().unwrap();
        u.context_tokens = Some(99_000_000);
        u.source = tokage_core::UsageSource::Estimated;
        let r = resolved(&s, &pack, 30);
        assert!(r.unavailable_reason.is_some());
        assert!(r.params.history_tokens_a < 100_000.);
    }
    #[test]
    fn prices_windows_cache_age_and_brief_can_change_the_decision() {
        let s = fixture();
        let pack = planning::effective_pack(&s.config).unwrap();
        let r = resolved(&s, &pack, 30);
        assert!(tokage_context::decide_window_reuse(&r.params).can_reuse);
        let mut small = pack.clone();
        for c in &mut small.candidates {
            c.effective_context = 15_000;
        }
        assert!(!tokage_context::decide_window_reuse(&resolved(&s, &small, 30).params).can_reuse);
        let cold = resolved(&s, &pack, 1_000_000);
        assert!(!tokage_context::decide_window_reuse(&cold.params).can_reuse);
        let mut more = s.clone();
        more.plan.as_mut().unwrap().plan.tasks[1].goal = "large supplied contract ".repeat(50_000);
        freeze(&mut more);
        assert!(!tokage_context::decide_window_reuse(&resolved(&more, &pack, 30).params).can_reuse);
        let mut threshold = s.clone();
        threshold
            .config
            .context_options
            .as_mut()
            .unwrap()
            .min_saving = 0.1;
        freeze(&mut threshold);
        let low = resolved(&threshold, &pack, 30);
        assert!(tokage_context::decide_window_reuse(&low.params).can_reuse);
        let mut expensive = pack.clone();
        for c in &mut expensive.candidates {
            c.price_per_mtok.output = 1_000_000.;
        }
        let high = resolved(&threshold, &expensive, 30);
        assert!(!tokage_context::decide_window_reuse(&high.params).can_reuse);
        let mut disabled = s.clone();
        disabled
            .config
            .context_options
            .as_mut()
            .unwrap()
            .cache_aware = false;
        freeze(&mut disabled);
        assert!(resolved(&disabled, &pack, 30)
            .unavailable_reason
            .unwrap()
            .contains("disable"));
        let mut no_cache = pack.clone();
        no_cache
            .providers
            .get_mut("CODEX_CLI")
            .unwrap()
            .cache
            .as_mut()
            .unwrap()
            .mode = "none".into();
        assert!(resolved(&s, &no_cache, 30).unavailable_reason.is_some());
    }
    #[test]
    fn missing_measurements_use_domain_size_profile_and_legacy_config_is_labelled() {
        let mut s = fixture();
        for t in s.turns.values_mut() {
            let u = t.usage.as_mut().unwrap();
            u.api_calls = None;
            u.context_tokens = Some(20_000);
            u.source = tokage_core::UsageSource::Estimated;
        }
        // A single verified context sample cannot supply a growth delta or API count.
        s.turns.get_mut("b").unwrap().usage.as_mut().unwrap().source =
            tokage_core::UsageSource::Structured;
        s.config.context_options = None;
        freeze(&mut s);
        let mut pack = planning::effective_pack(&s.config).unwrap();
        let target = &s.plan.as_ref().unwrap().plan.tasks[1];
        let size = match target.size {
            tokage_core::planning::TaskSize::S => "S",
            tokage_core::planning::TaskSize::M => "M",
            _ => "L",
        };
        pack.usage_profiles.get_mut(&target.domain).unwrap().insert(
            size.into(),
            UsageProfileEntry {
                turns: 3,
                calls_per_turn: 7,
                growth_per_turn: 3210,
                output_per_turn: 456,
            },
        );
        let r = resolved(&s, &pack, 30);
        assert_eq!(r.params.growth_per_turn, 3210.);
        assert_eq!(r.params.api_calls_per_turn, 7);
        assert_eq!(r.params.expected_turns_b, 3);
        assert!(r.sources["growthPerTurn"].contains("PROFILE_FORECAST"));
        assert_eq!(r.sources["minSaving"], "LEGACY_CONFIG_DEFAULT");
        let json = serde_json::to_value(&s.config).unwrap();
        assert!(json.get("contextOptions").is_none());
        let decoded: EffectiveRunConfig = serde_json::from_value(json).unwrap();
        assert_eq!(digest(&decoded).unwrap(), s.config_hash);
        s.config.context_options = Some(ContextOptions {
            cache_aware: true,
            min_saving: f64::NAN,
        });
        assert!(s.config.validate().is_err());
    }
}

#[cfg(test)]
mod unavailable_tests {
    use super::*;
    #[test]
    fn unpriced_variants_and_unknown_history_cannot_become_a_priced_reuse() {
        let (mut s, _) = task_context::tests::fixture();
        let pack = planning::effective_pack(&s.config).unwrap();
        let task = &s.plan.as_ref().unwrap().plan.tasks[0];
        s.execution
            .as_mut()
            .unwrap()
            .tasks
            .get_mut("T1")
            .unwrap()
            .assignment
            .model_params = Some(tokage_core::planning::ModelParams {
            effort: None,
            fast: true,
            tier: None,
        });
        assert!(resolve(&s, "T1", task, &s.turns["b"], &pack, 30).is_err());
        s.execution
            .as_mut()
            .unwrap()
            .tasks
            .get_mut("T1")
            .unwrap()
            .assignment
            .model_params = None;
        let mut scheduled = pack.clone();
        for m in scheduled.models.values_mut() {
            m.price_schedule = Some(serde_json::json!({"unknown":true}));
        }
        assert!(resolve(&s, "T1", task, &s.turns["b"], &scheduled, 30).is_err());
        let mut source = s.turns["b"].clone();
        source.window_origin = None;
        s.turns.insert("b".into(), source.clone());
        assert!(resolve(&s, "T1", task, &source, &pack, 30)
            .unwrap()
            .unavailable_reason
            .unwrap()
            .contains("incomplete"));
    }
}
