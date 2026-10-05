//! P6.m6 Acceptance Tests: Task outcomes, Beta posterior convergence, usage profile updates, and cost calibration (docs/04 §9).

use tokage_matching::{
    apply_user_preference_signal, update_cost_calibration,
    update_usage_profile_entry, BetaAccumulator, TaskOutcomeResult, UsageProfileEntry,
};
use tokage_storage::{LearnedStatRow, Storage, TaskOutcomeRow};

#[test]
fn test_acceptance_simulated_outcome_sequence_posterior_convergence() {
    // 1. Initial Beta accumulator with prior p0 = 0.75, kappa = 6.0
    let mut beta_acc = BetaAccumulator::new(0.75, 6.0);
    assert!((beta_acc.posterior_p() - 0.75).abs() < 1e-4);

    // 2. Feed sequence of 8 FIRST_PASS successes and 2 FIXED_IN_WINDOW repairs
    for _ in 0..8 {
        beta_acc.update(TaskOutcomeResult::FirstPass);
    }
    for _ in 0..2 {
        beta_acc.update(TaskOutcomeResult::FixedInWindow);
    }

    // Success sequence MUST pull posterior strictly upward towards 0.85+
    let p_after_success = beta_acc.posterior_p();
    assert!(p_after_success > 0.84);

    // 3. Simulated failure sequence (reassigned, blocked, user takeover)
    let mut beta_fail = BetaAccumulator::new(0.75, 6.0);
    for _ in 0..6 {
        beta_fail.update(TaskOutcomeResult::Reassigned);
    }
    for _ in 0..4 {
        beta_fail.update(TaskOutcomeResult::Blocked);
    }

    // Failure sequence MUST pull posterior strictly downward towards < 0.40
    let p_after_fail = beta_fail.posterior_p();
    assert!(p_after_fail < 0.35);
}

#[test]
fn test_acceptance_usage_profile_and_cost_calibration_learning() {
    let initial_profile = UsageProfileEntry {
        turns: 1,
        calls_per_turn: 6,
        growth_per_turn: 10_000,
        output_per_turn: 3_000,
    };

    // Actual execution had higher turns and token growth
    let learned = update_usage_profile_entry(
        &initial_profile,
        2,      // actual turns
        10,     // actual calls
        18_000, // actual growth
        5_500,  // actual output
        5,      // 5th sample
    );

    assert!(learned.calls_per_turn >= 6);
    assert!(learned.growth_per_turn > 10_000);

    // Cost calibration: actual cost was 1.25x estimated cost
    let mut calibration = 1.0;
    for _ in 0..6 {
        calibration = update_cost_calibration(calibration, 0.25, 0.20, 6);
    }
    assert!(calibration > 1.05);

    // User preference boost signal
    let mut beta = BetaAccumulator::new(0.70, 6.0);
    let p_prev = beta.posterior_p();
    apply_user_preference_signal(&mut beta, 3.0);
    assert!(beta.posterior_p() > p_prev);
}

#[tokio::test]
async fn test_acceptance_storage_task_outcomes_and_learned_stats_persistence() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let storage = Storage::open(temp_dir.path()).expect("open storage with V5 migrations");

    let outcome = TaskOutcomeRow {
        id: None,
        task_id: Some("T1".into()),
        model_id: "claude-sonnet-5-5".into(),
        provider_name: "CLAUDE_CODE".into(),
        domain: "backend-api".into(),
        difficulty: 2,
        size: "M".into(),
        result: "FIRST_PASS".into(),
        p_predicted: 0.88,
        est_cost_usd: Some(0.22),
        actual_cost_usd: Some(0.20),
        turns: Some(2),
        created_at: None,
    };

    let outcome_id = storage
        .record_task_outcome(outcome)
        .await
        .expect("record outcome");
    assert!(outcome_id > 0);

    let list = storage
        .list_task_outcomes(Some("claude-sonnet-5-5"), Some("backend-api"), 10)
        .expect("list outcomes");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].result, "FIRST_PASS");

    // Learned stats persistence
    let stat = LearnedStatRow {
        provider_name: "CLAUDE_CODE".into(),
        model_id: "claude-sonnet-5-5".into(),
        domain: "backend-api".into(),
        bucket: "d2".into(),
        metric: "success_alpha".into(),
        value: 5.8,
        samples: 1,
        updated_at: None,
    };
    storage.update_learned_stat(stat).await.expect("update learned stat");

    let model_stats = storage
        .list_learned_stats("claude-sonnet-5-5")
        .expect("list learned stats");
    assert!(!model_stats.is_empty());
    assert_eq!(model_stats[0].metric, "success_alpha");
    assert_eq!(model_stats[0].value, 5.8);
}
