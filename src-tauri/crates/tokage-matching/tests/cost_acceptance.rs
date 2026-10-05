//! P6.m4 Acceptance Tests: Cost model, success probability, and expected cost calculations (docs/04 §4, §5, §6).

use std::collections::HashMap;
use tokage_matching::{
    calculate_c_exec, calculate_c_turn, calculate_eff_price, calculate_expected_cost,
    calculate_posterior_p, calculate_prior_p0, calculate_scarcity, get_p_min_threshold,
    select_best_candidate, sigmoid, EvaluatedCandidate, PricePerMtok, SuccessModel,
    TokenPriceKind, TurnCostParams, UsageProfileEntry,
};

#[test]
fn test_acceptance_cost_model_and_scarcity_multipliers() {
    // 1. Scarcity tiers based on projected utilization u
    // PAY_AS_YOU_GO is always 1.0
    assert_eq!(calculate_scarcity("PAY_AS_YOU_GO", 0.10), 1.0);
    assert_eq!(calculate_scarcity("PAY_AS_YOU_GO", 0.90), 1.0);

    // SUBSCRIPTION:
    // u < 50% -> 0.5
    assert_eq!(calculate_scarcity("SUBSCRIPTION", 0.30), 0.5);
    // 50-80% -> 1.0
    assert_eq!(calculate_scarcity("SUBSCRIPTION", 0.72), 1.0);
    // 80-95% -> 3.0
    assert_eq!(calculate_scarcity("SUBSCRIPTION", 0.88), 3.0);
    // >= 95% -> melted (infinity)
    assert!(calculate_scarcity("SUBSCRIPTION", 0.96).is_infinite());

    // 2. Effective price
    let price = PricePerMtok {
        input: 3.0,
        output: 15.0,
    };
    let scarcity_05 = calculate_scarcity("SUBSCRIPTION", 0.40);
    assert_eq!(
        calculate_eff_price(&price, TokenPriceKind::Input, scarcity_05),
        1.5
    );
    let scarcity_30 = calculate_scarcity("SUBSCRIPTION", 0.85);
    assert_eq!(
        calculate_eff_price(&price, TokenPriceKind::Output, scarcity_30),
        45.0
    );

    // 3. Single turn cost C_turn with prompt caching
    let turn = TurnCostParams {
        context_tokens: 50_000.0,
        warm: true,
        api_calls: 4,
        context_growth: 5_000.0,
        output_tokens: 2_000.0,
        p_in: 3.0,
        p_out: 15.0,
        cache_read_multiplier: 0.1,
        cache_write_multiplier: 1.25,
    };
    // Input weighted = 50k*0.1 + 3*(50k*0.1) + 5k*1.25 = 5000 + 15000 + 6250 = 26250 tokens
    // Input cost = (26250 / 1e6) * 3.0 = 0.07875 USD
    // Output cost = (2000 / 1e6) * 15.0 = 0.030 USD
    // C_turn = 0.10875 USD
    let c_turn = calculate_c_turn(&turn);
    assert!((c_turn - 0.10875).abs() < 1e-5);

    // 4. Multi-turn execution cost C_exec
    let profile = UsageProfileEntry {
        turns: 2,
        calls_per_turn: 4,
        growth_per_turn: 5_000,
        output_per_turn: 2_000,
    };
    let cache_rates = tokage_matching::pack::CacheRatesDef {
        read_multiplier: 0.1,
        write_multiplier: 1.25,
    };
    let c_exec = calculate_c_exec(
        20_000.0, // Baseline B
        3_000.0,  // Brief D
        &profile,
        true, // Baseline warm
        3.0,
        15.0,
        Some(&cache_rates),
    );
    assert!(c_exec > 0.0 && c_exec < 1.0);
}

#[test]
fn test_acceptance_sigmoid_prior_and_beta_posterior() {
    let mut req = HashMap::new();
    let mut d1 = HashMap::new();
    d1.insert("S".into(), 2.0);
    d1.insert("M".into(), 2.5);
    d1.insert("L".into(), 3.0);
    req.insert("1".into(), d1);

    let mut d2 = HashMap::new();
    d2.insert("S".into(), 3.0);
    d2.insert("M".into(), 3.5);
    d2.insert("L".into(), 4.0);
    req.insert("2".into(), d2);

    let mut d3 = HashMap::new();
    d3.insert("S".into(), 4.0);
    d3.insert("M".into(), 4.3);
    d3.insert("L".into(), 4.5);
    req.insert("3".into(), d3);

    let success_model = SuccessModel {
        a: 1.6,
        b: 1.2,
        prior_strength: 6.0,
        req,
        planner_min_skill: 5.0,
        p_min: None,
    };

    // Case 1: Matching requirement exactly: skill = 3.5 on difficulty 2, size M (req = 3.5)
    // z = 1.6 * 0 + 1.2 = 1.2 -> sigmoid(1.2) = 0.7685
    let p0 = calculate_prior_p0(3.5, 2, "M", &success_model);
    assert!((p0 - sigmoid(1.2)).abs() < 1e-4);

    // Case 2: High skill model: skill = 4.5 on difficulty 2, size M (req = 3.5)
    // z = 1.6 * 1.0 + 1.2 = 2.8 -> sigmoid(2.8) = 0.9426
    let p0_high = calculate_prior_p0(4.5, 2, "M", &success_model);
    assert!((p0_high - sigmoid(2.8)).abs() < 1e-4);
    assert!(p0_high > 0.90);

    // Case 3: Posterior update with Beta distribution
    // Initial posterior matches prior when observations = 0
    let p_initial = calculate_posterior_p(p0, 6.0, 0.0, 0.0);
    assert_eq!(p_initial, p0);

    // After 8 first-pass successes (1.0 each) and 1 in-window fix (0.5), successes = 8.5, failures = 0
    let p_learned_success = calculate_posterior_p(p0, 6.0, 8.5, 0.0);
    assert!(p_learned_success > p0);
    assert!(p_learned_success > 0.85);

    // Conversely, multiple failures pull probability below threshold
    let p_learned_fail = calculate_posterior_p(p0, 6.0, 1.0, 8.0);
    assert!(p_learned_fail < p0);
    assert!(p_learned_fail < 0.50);
}

#[test]
fn test_acceptance_expected_cost_and_threshold_filtering() {
    let success_model = SuccessModel {
        a: 1.6,
        b: 1.2,
        prior_strength: 6.0,
        req: HashMap::new(),
        planner_min_skill: 5.0,
        p_min: None,
    };

    let p_min = get_p_min_threshold(2, &success_model); // 0.80
    assert_eq!(p_min, 0.80);

    // Scenario from docs/04 §2 & §5:
    // Candidate A (Cursor Composer): p = 0.88, c_exec = 0.18, runner_up = 0.32
    // Candidate B (Claude Sonnet): p = 0.91, c_exec = 0.30, runner_up = 0.18
    // Candidate C (Cheap Model): p = 0.65 (below threshold 0.80), c_exec = 0.05
    let c_cand_a = calculate_expected_cost(0.18, 0.88, 0.5, Some(0.30));
    let c_cand_b = calculate_expected_cost(0.30, 0.91, 0.5, Some(0.18));
    let c_cand_c = calculate_expected_cost(0.05, 0.65, 0.5, Some(0.30));

    let candidates = vec![
        EvaluatedCandidate {
            provider: "CURSOR".into(),
            model_id: "cursor-composer".into(),
            display_name: "Cursor Composer".into(),
            cli_model_arg: "composer".into(),
            billing: "SUBSCRIPTION".into(),
            p_success: 0.88,
            c_exec: 0.18,
            expected_cost: c_cand_a,
            meets_threshold: true,
            origin_layer: "builtin".into(),
        },
        EvaluatedCandidate {
            provider: "CLAUDE_CODE".into(),
            model_id: "claude-sonnet-5-5".into(),
            display_name: "Claude Sonnet 5.5".into(),
            cli_model_arg: "sonnet".into(),
            billing: "SUBSCRIPTION".into(),
            p_success: 0.91,
            c_exec: 0.30,
            expected_cost: c_cand_b,
            meets_threshold: true,
            origin_layer: "builtin".into(),
        },
        EvaluatedCandidate {
            provider: "OPENCODE".into(),
            model_id: "opencode-cheap".into(),
            display_name: "Cheap Model".into(),
            cli_model_arg: "cheap".into(),
            billing: "PAY_AS_YOU_GO".into(),
            p_success: 0.65,
            c_exec: 0.05,
            expected_cost: c_cand_c,
            meets_threshold: false,
            origin_layer: "builtin".into(),
        },
    ];

    // Filter threshold: Candidate C is excluded because p = 0.65 < 0.80.
    // Between A (E ~= 0.21) and B (E ~= 0.33), A has lower expected cost.
    let best = select_best_candidate(candidates, p_min).expect("Must select qualified candidate");
    assert_eq!(best.model_id, "cursor-composer");
    assert!(best.p_success >= p_min);
}
