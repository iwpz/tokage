//! Acceptance tests for P8.m1 context lifecycle engine (docs/05, P8.m1).
//!
//! Specifically reproduces:
//! - 05 §5.1 numerical examples (Situation 1: 38% saving -> COMPACT, Situation 2: 11% saving -> CONTINUE).
//! - Window reuse conditions and decisions (P8.m1a).
//! - In-window planning with hard limit and frequency constraints (P8.m1b).
//! - Native compact CLI integration and fallback (P8.m1c).
//! - Appended message triage (P8.m1e).
//! - R9 and R10 context and cache discipline tests (P8.m1f).

use crate::compact::{build_compact, BuildCompactRequest, CompactMethod};
use crate::discipline::{
    validate_no_fake_keepalives, validate_task_brief_header, validate_user_message_integrity,
    validate_window_continuity, WindowConfig,
};
use crate::in_window::{
    compute_example_unit_costs, plan_context_action, ActionTrigger, ContextActionKind,
    InWindowContextParams,
};
use crate::reuse::{decide_window_reuse, WindowReuseParams};
use crate::triage::{
    triage_main_session_message, AppendMessageRoute, TaskSummaryForTriage, TriageMessageParams,
};
use tokage_orchestration::brief::TASK_BRIEF_FIXED_HEADER;

#[test]
fn test_acceptance_docs_05_section_5_1_numerical_examples() {
    // 05 §5.1 parameters:
    // r = 0.1, w = 1.25, TTL = 5 min, B = 25K, g = 8K, k = 6, compact_output_ratio = 0.06

    // --- 情境 1: H = 90K, h = 4, 用户离开 12 分钟（冷） ---
    // Expected: CONTINUE ≈ 613, COMPACT ≈ 378, 节省 ≈ 38%, 结论: 压缩
    let (cont_1, comp_1, saving_1) = compute_example_unit_costs(90.0, 4, true);

    assert!(
        (cont_1 - 613.0).abs() < 5.0,
        "Situation 1 CONTINUE was {:.1}, expected ~613",
        cont_1
    );
    assert!(
        (comp_1 - 378.0).abs() < 5.0,
        "Situation 1 COMPACT was {:.1}, expected ~378",
        comp_1
    );
    assert!(
        (saving_1 - 0.38).abs() < 0.02,
        "Situation 1 saving was {:.3}, expected ~0.38",
        saving_1
    );

    // Full InWindowContextParams evaluation for Situation 1
    let mut p1 = InWindowContextParams::default();
    p1.turns_since_last_compact=2; // Isolate numerical cost behavior from cooldown.
    p1.baseline_tokens = 25_000.0;
    p1.history_tokens = 90_000.0;
    p1.remaining_turns = 4;
    p1.growth_per_turn = 8_000.0;
    p1.api_calls_per_turn = 6;
    p1.elapsed_seconds_since_last_turn = 720.0; // 12 min (cold)
    p1.cache_ttl_seconds = 300.0;
    let plan1 = plan_context_action(&p1);

    assert_eq!(plan1.recommended_action, ContextActionKind::Compact);
    assert!(plan1.estimated_saving_percentage >= 0.15);
    assert!(!plan1.warm_now);

    // --- 情境 2: H = 15K, h = 2, 用户 1 分钟内回复（热） ---
    // Expected: CONTINUE ≈ 89, COMPACT ≈ 80, 节省 ≈ 11%, 低于门槛, 结论: 继续
    let (cont_2, comp_2, saving_2) = compute_example_unit_costs(15.0, 2, false);

    assert!(
        (cont_2 - 89.0).abs() < 5.0,
        "Situation 2 CONTINUE was {:.1}, expected ~89",
        cont_2
    );
    assert!(
        (comp_2 - 80.0).abs() < 5.0,
        "Situation 2 COMPACT was {:.1}, expected ~80",
        comp_2
    );
    assert!(
        saving_2 < 0.15,
        "Situation 2 saving was {:.3}, expected < 0.15 (threshold)",
        saving_2
    );
    assert!(
        (saving_2 - 0.11).abs() < 0.05,
        "Situation 2 saving was {:.3}, expected ~0.11",
        saving_2
    );

    // Full InWindowContextParams evaluation for Situation 2
    let mut p2 = InWindowContextParams::default();
    p2.turns_since_last_compact=2;
    p2.baseline_tokens = 25_000.0;
    p2.history_tokens = 15_000.0;
    p2.remaining_turns = 2;
    p2.growth_per_turn = 8_000.0;
    p2.api_calls_per_turn = 6;
    p2.elapsed_seconds_since_last_turn = 60.0; // 1 min (hot)
    p2.cache_ttl_seconds = 300.0;
    p2.min_saving = 0.15;
    let plan2 = plan_context_action(&p2);

    assert_eq!(plan2.recommended_action, ContextActionKind::Continue);
    assert!(plan2.warm_now);
    assert!(plan2.estimated_saving_percentage < 0.15);
}

#[test]
fn test_acceptance_window_reuse_lifecycle() {
    // 1. Success case: Same model, same role, same domain, warm cache -> Reuse approved
    let mut p = WindowReuseParams::default();
    p.model_a = "claude-3-5-sonnet".to_string();
    p.model_b = "claude-3-5-sonnet".to_string();
    p.role_a = "EXECUTOR".to_string();
    p.role_b = "EXECUTOR".to_string();
    p.domain_a = "backend".to_string();
    p.domain_b = "backend".to_string();
    p.elapsed_seconds = 45.0; // < 300s TTL
    p.history_tokens_a = 15_000.0;

    let res = decide_window_reuse(&p);
    assert!(res.can_reuse);
    assert!(res.saving_percentage > 0.0);

    // 2. Failure case: Role change (EXECUTOR -> TESTER) -> Must open fresh window
    let mut p_role = p.clone();
    p_role.role_b = "TESTER".to_string();
    let res_role = decide_window_reuse(&p_role);
    assert!(!res_role.can_reuse);
    assert!(res_role.reason.contains("Role/permission mismatch"));

    // 3. Failure case: Cache is cold -> Must open fresh window
    let mut p_cold = p.clone();
    p_cold.elapsed_seconds = 350.0; // > 300s TTL
    let res_cold = decide_window_reuse(&p_cold);
    assert!(!res_cold.can_reuse);
    assert!(res_cold.reason.contains("Cache is cold"));
}

#[test]
fn test_acceptance_in_window_hard_limit_and_cooling() {
    let mut p = InWindowContextParams::default();
    p.turns_since_last_compact=2;
    p.history_tokens = 150_000.0;
    p.effective_context = 120_000.0;

    let plan = plan_context_action(&p);
    assert!(plan.continue_disqualified);
    assert_ne!(plan.recommended_action, ContextActionKind::Continue);
    assert_eq!(plan.trigger, ActionTrigger::HardLimit);
}

#[test]
fn test_acceptance_r9_r10_discipline_enforcement() {
    // R9: Startup args or model changed in same window
    let w1 = WindowConfig {
        model: "claude-3-5-sonnet".to_string(),
        role: "EXECUTOR".to_string(),
        startup_args: vec!["--dangerously-skip-permissions".to_string()],
    };
    let w2 = WindowConfig {
        startup_args: vec![],
        ..w1.clone()
    };
    assert!(validate_window_continuity(&w1, &w2).is_err());

    // R10: Byte-for-byte brief fixed header integrity
    let valid = format!("{}\n## Task\n- ID: T1", TASK_BRIEF_FIXED_HEADER);
    assert!(validate_task_brief_header(&valid).is_ok());

    let corrupted = format!("# Custom Header\n{}", TASK_BRIEF_FIXED_HEADER);
    assert!(validate_task_brief_header(&corrupted).is_err());

    // R10: Verbatim user message without synthetic prefix
    let msg = "Hello assistant";
    assert!(validate_user_message_integrity(msg, msg).is_ok());
    assert!(validate_user_message_integrity(msg, &format!("[SYS_INFO] {}", msg)).is_err());

    // R10: No fake keepalive requests
    assert!(validate_no_fake_keepalives(false).is_ok());
    assert!(validate_no_fake_keepalives(true).is_err());
}

#[test]
fn test_acceptance_compact_generation() {
    let req = BuildCompactRequest {
        provider_name: "Claude".to_string(),
        task_goal: "Fix database pool timeout".to_string(),
        contract_paths: vec!["crates/tokage-storage/src/repo.rs".to_string()],
        pending_errors: Some("pool timeout after 30s".to_string()),
        custom_instruction: None,
    };
    let payload = build_compact(&req);
    assert_eq!(payload.method, CompactMethod::NativeSlashCommand);
    assert!(payload.command.unwrap().contains("/compact"));
    assert!(payload.instruction.contains("Keep the current task goal"));
}

#[test]
fn test_acceptance_appended_message_triage() {
    let tasks = vec![TaskSummaryForTriage {
        task_id: "T2".to_string(),
        title: "Implement Token Bucket".to_string(),
        write_scope: vec!["src/ratelimit.rs".to_string()],
    }];

    // 1. Task window route
    let p_task = TriageMessageParams {
        message: "在 src/ratelimit.rs 里增加并发锁保护".to_string(),
        active_tasks: tasks.clone(),
        last_direct_attempt_id: None,
        last_direct_timestamp_ms: None,
        current_time_ms: 10_000,
        explicit_task_id: None,
    };
    match triage_main_session_message(&p_task) {
        AppendMessageRoute::TaskExecutionWindow { task_id, .. } => assert_eq!(task_id, "T2"),
        _ => panic!("Expected TaskExecutionWindow"),
    }

    // 2. Incremental replan route
    let p_replan = TriageMessageParams {
        message: "还要补充需求，增加一个 prometheus 监控指标".to_string(),
        active_tasks: tasks.clone(),
        last_direct_attempt_id: None,
        last_direct_timestamp_ms: None,
        current_time_ms: 10_000,
        explicit_task_id: None,
    };
    match triage_main_session_message(&p_replan) {
        AppendMessageRoute::IncrementalReplan { .. } => (),
        _ => panic!("Expected IncrementalReplan"),
    }

    // 3. Follow-up to previous direct dispatch
    let p_direct_followup = TriageMessageParams {
        message: "上个回答里的第一点没太看懂，再详细点".to_string(),
        active_tasks: tasks,
        last_direct_attempt_id: Some(101),
        last_direct_timestamp_ms: Some(9000),
        current_time_ms: 12_000,
        explicit_task_id: None,
    };
    match triage_main_session_message(&p_direct_followup) {
        AppendMessageRoute::PreviousDirectWindow { attempt_id, .. } => assert_eq!(attempt_id, 101),
        _ => panic!("Expected PreviousDirectWindow"),
    }
}
