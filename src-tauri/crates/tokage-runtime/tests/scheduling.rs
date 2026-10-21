mod common;
use common::*;
use tokage_orchestration::{RepairStep, TaskState};
use tokage_runtime::*;
fn executing(concurrency: u32) -> Snapshot {
    let mut req = create();
    if let Command::CreateRun { config, .. } = &mut req.command {
        config.concurrency = concurrency;
        config.target_reference = Some("refs/heads/main".into());
    }
    let (s, _) = transition(None, &req).unwrap();
    let text = format!(
        "{}\n{}",
        PLAN,
        PLAN.split("```yaml\n")
            .nth(1)
            .unwrap()
            .split("```\n")
            .next()
            .unwrap()
            .replace("T1", "T2")
            .replace("src/feature/**", "src/other/**")
    );
    // One YAML task list, independent tasks with non-overlapping scopes.
    let text = text.replace("```\n\n- id: T2", "- id: T2");
    let text = if text.ends_with("```\n") {
        text
    } else {
        format!("{text}\n```\n")
    };
    let (s, _) = transition(
        Some(&s),
        &command(&s, "import", Command::ImportPlan { text }),
    )
    .unwrap();
    let digest = s.plan.as_ref().unwrap().confirmation_digest.clone();
    let (s, _) = transition(
        Some(&s),
        &command(
            &s,
            "approve",
            Command::ApprovePlan {
                confirmation_digest: digest.clone(),
            },
        ),
    )
    .unwrap();
    transition(
        Some(&s),
        &command(
            &s,
            "execute",
            Command::StartExecution {
                confirmation_digest: digest,
            },
        ),
    )
    .unwrap()
    .0
}
#[test]
fn serial_parallel_share_dag_and_atomic_reservations() {
    for (slots, count) in [(1, 1), (2, 2)] {
        let mut s = executing(slots);
        let first = execution::reserve_ready(&mut s, 100).unwrap();
        assert_eq!(first.len(), count);
        assert!(execution::reserve_ready(&mut s, 100).unwrap().is_empty());
        assert_eq!(
            s.execution
                .as_ref()
                .unwrap()
                .tasks
                .values()
                .filter(|t| t.reservation_active)
                .count(),
            count
        );
        assert!(first
            .iter()
            .all(|r| r.assignment.user_locked && r.assignment.requested_model == "gpt-5.6-sol"));
    }
}
#[test]
fn unknown_estimate_and_actual_cost_block_without_partial_reservations() {
    let mut s = executing(2);
    s.execution
        .as_mut()
        .unwrap()
        .tasks
        .get_mut("T2")
        .unwrap()
        .assignment
        .requested_model = "unpriced-user-model".into();
    let original = s.clone();
    assert!(execution::reserve_ready(&mut s, 100).is_err());
    assert_eq!(s, original);
    let mut s = executing(2);
    let m = s.config.models["executor"].clone();
    let mut turn = turn::TurnRecord::prepared("previous".into(), "planner", None, m);
    turn.ended = true;
    turn.cleanup_confirmed = true;
    s.turns.insert("previous".into(), turn);
    let original = s.clone();
    assert!(execution::reserve_ready(&mut s, 100).is_err());
    assert_eq!(s, original);
    s.config.block_unknown_cost = false;
    assert_eq!(execution::reserve_ready(&mut s, 100).unwrap().len(), 2);
}
#[test]
fn budget_and_provider_half_open_reserve_before_dispatch() {
    let mut s = executing(2);
    s.config.budget_usd = Some(0.0);
    assert!(execution::reserve_ready(&mut s, 100).unwrap().is_empty());
    assert!(s
        .execution
        .as_ref()
        .unwrap()
        .tasks
        .values()
        .all(|t| !t.reservation_active));
    let mut s = executing(2);
    s.execution.as_mut().unwrap().health.insert(
        "CODEX_CLI".into(),
        execution::ProviderHealth {
            failures: 3,
            melted_until: Some(200),
            reason: Some("quota".into()),
        },
    );
    assert!(execution::reserve_ready(&mut s, 100).unwrap().is_empty());
    assert_eq!(execution::reserve_ready(&mut s, 200).unwrap().len(), 1);
    assert!(execution::reserve_ready(&mut s, 200).unwrap().is_empty());
}
#[test]
fn manual_failure_and_locked_bounded_repair_do_not_switch_models_or_loop() {
    let mut s = executing(1);
    let r = execution::reserve_ready(&mut s, 100).unwrap().remove(0);
    execution::fail_task(&mut s, &r.task_id, "gate failed".into(), 100).unwrap();
    assert_eq!(
        s.execution.as_ref().unwrap().tasks[&r.task_id].state,
        TaskState::Blocked
    );
    let mut s = executing(1);
    s.config.repair_policy = "bounded-ladder".into();
    let r = execution::reserve_ready(&mut s, 100).unwrap().remove(0);
    let mut t = turn::TurnRecord::prepared(
        r.attempt_id.clone(),
        "executor",
        Some(r.task_id.clone()),
        r.assignment.clone(),
    );
    t.ended = true;
    t.cleanup_confirmed = true;
    t.thread_id = Some("native-thread".into());
    t.cost_usd = Some(0.1);
    s.turns.insert(t.id.clone(), t);
    execution::fail_task(&mut s, &r.task_id, "gate failed".into(), 100).unwrap();
    assert_eq!(
        s.execution.as_ref().unwrap().tasks[&r.task_id].repair_step,
        RepairStep::InWindowRepair
    );
    assert_eq!(
        s.execution.as_ref().unwrap().tasks[&r.task_id].state,
        TaskState::Pending
    );
    let second = execution::reserve_ready(&mut s, 100).unwrap().remove(0);
    assert_eq!(second.resume_turn.as_deref(), Some(r.attempt_id.as_str()));
    execution::fail_task(&mut s, &r.task_id, "repair failed".into(), 100).unwrap();
    assert_eq!(
        s.execution.as_ref().unwrap().tasks[&r.task_id].state,
        TaskState::Blocked
    );
    assert_eq!(
        s.execution.as_ref().unwrap().tasks[&r.task_id].assignment,
        r.assignment
    );
}
#[test]
fn explicit_provider_limit_and_unknown_priority_price_remain_authoritative(){
    let mut s=executing(2);s.config.provider_concurrency.insert("CODEX_CLI".into(),1);assert_eq!(execution::reserve_ready(&mut s,100).unwrap().len(),1);
    let mut s=executing(2);s.execution.as_mut().unwrap().tasks.get_mut("T1").unwrap().assignment.model_params=Some(tokage_core::planning::ModelParams{fast:true,..Default::default()});let before=s.clone();assert!(execution::reserve_ready(&mut s,100).is_err());assert_eq!(s,before);
}
