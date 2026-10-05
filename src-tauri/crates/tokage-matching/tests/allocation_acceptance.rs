//! P6.m5 Acceptance Tests: Global plan allocation, planning model ranking, and review explanations (docs/04 §7, §8).

use std::collections::HashMap;
use tokage_core::planning::{Plan, PlanSpec, TaskItem, TaskSize};
use tokage_matching::{
    allocate_plan_models, assign_direct_task, builtin_knowledge_pack, merge_layers,
    recompute_plan_allocation_with_overrides, select_planning_model, LayeredPacks,
};

#[test]
fn test_acceptance_planning_model_ranked_by_p_success() {
    let builtin = builtin_knowledge_pack().clone();
    let layers = LayeredPacks {
        builtin,
        official: None,
        customs: vec![],
        overrides: vec![],
        learned_usage_profiles: None,
    };
    let pack = merge_layers(&layers).expect("pack must merge");

    // Planning models must be ranked strictly by p_success
    let planner = select_planning_model(&pack, 2).expect("Planner must be found");
    assert!(planner.p_success >= 0.80);

    let cand = pack
        .candidates
        .iter()
        .find(|c| c.model_id == planner.model_id)
        .expect("planner candidate exists");
    assert!(cand.roles.iter().any(|r| r.eq_ignore_ascii_case("planner")));
    let skill = cand.skills.get("planning").copied().unwrap_or(0.0);
    assert!(skill >= pack.success_model.planner_min_skill);
}

#[test]
fn test_acceptance_direct_task_assignment() {
    let builtin = builtin_knowledge_pack().clone();
    let layers = LayeredPacks {
        builtin,
        official: None,
        customs: vec![],
        overrides: vec![],
        learned_usage_profiles: None,
    };
    let pack = merge_layers(&layers).expect("pack must merge");

    let direct = assign_direct_task("frontend-ui", 1, Some(TaskSize::S), &pack, None)
        .expect("Direct task assigned");
    assert_eq!(direct.task_id, "DIRECT");
    assert!(direct.p_success >= 0.80);
    assert!(direct.est_cost_usd > 0.0);
    assert!(direct.explanation.contains("frontend-ui"));
    assert!(direct.explanation.contains("把握"));
}

#[test]
fn test_acceptance_global_plan_assignment_and_reassignment_recomputation() {
    let builtin = builtin_knowledge_pack().clone();
    let layers = LayeredPacks {
        builtin,
        official: None,
        customs: vec![],
        overrides: vec![],
        learned_usage_profiles: None,
    };
    let pack = merge_layers(&layers).expect("pack must merge");

    let tasks = vec![
        TaskItem {
            id: "T1".into(),
            domain: "backend-api".into(),
            title: "Database Repository".into(),
            goal: "Data access layer".into(),
            depends_on: vec![],
            contracts: vec![],
            write_scope: vec!["src/repo/**".into()],
            deliverables: vec![],
            acceptance: Default::default(),
            size: TaskSize::M,
            difficulty: 2,
            assigned_model: None,
        },
        TaskItem {
            id: "T2".into(),
            domain: "frontend-ui".into(),
            title: "Management View".into(),
            goal: "Render table view".into(),
            depends_on: vec!["T1".into()],
            contracts: vec![],
            write_scope: vec!["src/views/**".into()],
            deliverables: vec![],
            acceptance: Default::default(),
            size: TaskSize::S,
            difficulty: 1,
            assigned_model: None,
        },
        TaskItem {
            id: "T3".into(),
            domain: "testing".into(),
            title: "Integration Verification".into(),
            goal: "Verify flow end to end".into(),
            depends_on: vec!["T2".into()],
            contracts: vec![],
            write_scope: vec!["tests/**".into()],
            deliverables: vec![],
            acceptance: Default::default(),
            size: TaskSize::M,
            difficulty: 2,
            assigned_model: None,
        },
    ];

    let plan = Plan {
        version: 1,
        parent_plan_id: None,
        spec: PlanSpec::default(),
        contracts: vec![],
        tasks,
        notes: None,
        raw_markdown: String::new(),
    };

    // 1. Initial global allocation
    let initial_res = allocate_plan_models(&plan, &pack, None);
    assert_eq!(initial_res.assignments.len(), 3);
    assert!(initial_res.total_est_cost_usd > 0.0);

    // Each assignment has explanation and model details
    for assign in &initial_res.assignments {
        assert!(!assign.explanation.is_empty());
        assert!(!assign.display_name.is_empty());
        assert!(assign.p_success > 0.0);
    }

    // 2. User manually reassigns T1 to another model
    let mut manual_overrides = HashMap::new();
    manual_overrides.insert("T1".into(), "CLAUDE_CODE:claude-opus-5-5".into());

    let reassigned_res =
        recompute_plan_allocation_with_overrides(&plan, &pack, &manual_overrides, None);
    assert_eq!(reassigned_res.assignments.len(), 3);
    assert_eq!(reassigned_res.assignments[0].model_id, "claude-opus-5-5");
    assert!(reassigned_res.assignments[0].explanation.contains("用户手动指定"));

    // Remaining tasks T2 and T3 are recomputed with updated cumulative provider utilization
    assert!(reassigned_res.provider_utilizations.get("CLAUDE_CODE").unwrap() > &0.0);
}
