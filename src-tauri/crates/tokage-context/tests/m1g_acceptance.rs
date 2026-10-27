use serde::Deserialize;
use tokage_context::{plan_context_action, ContextActionKind, InWindowContextParams, WindowReuseParams};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlannerCase {
    name: String,
    params: InWindowContextParams,
    expected_action: ContextActionKind,
    expected_warm: bool,
}

#[test]
fn missing_fields_keep_documented_defaults_and_cooldown() {
    let params: InWindowContextParams = serde_json::from_str(r#"{"historyTokens":90000}"#).unwrap();
    let defaults = InWindowContextParams::default();
    assert_eq!(params.history_tokens, 90_000.0);
    assert_eq!(params.baseline_tokens, defaults.baseline_tokens);
    assert_eq!(params.remaining_turns, defaults.remaining_turns);
    assert_eq!(params.turns_since_last_compact, 0);
    assert!(plan_context_action(&params).cooldown_blocked);
    let reuse: WindowReuseParams = serde_json::from_str(r#"{"historyTokensA":12345}"#).unwrap();
    assert_eq!(reuse.history_tokens_a, 12_345.0);
    assert_eq!(reuse.min_saving, WindowReuseParams::default().min_saving);
}

#[test]
fn planner_decisions_follow_cold_and_warm_fixtures() {
    let cases: Vec<PlannerCase> = serde_json::from_str(include_str!("fixtures/m1g-planner.json")).unwrap();
    assert_eq!(cases.len(), 2);
    for case in cases {
        let result = plan_context_action(&case.params);
        assert_eq!(result.recommended_action, case.expected_action, "{}", case.name);
        assert_eq!(result.warm_now, case.expected_warm, "{}", case.name);
        assert!(!result.cooldown_blocked, "{}", case.name);
        if case.expected_action == ContextActionKind::Compact {
            assert!(result.estimated_saving_percentage >= case.params.min_saving, "{}", case.name);
        } else {
            assert!(result.estimated_saving_percentage < case.params.min_saving, "{}", case.name);
        }
    }
}
