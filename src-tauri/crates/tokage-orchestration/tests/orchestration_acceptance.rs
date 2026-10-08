//! Acceptance tests for P7.m1 (DAG Scheduling & State Machine)
//! and P7.m2 (Task Briefs & Reports) (docs/03, docs/05, docs/08).

use std::collections::HashMap;
use tokage_core::planning::{Plan, PlanContract, TaskAcceptance, TaskItem, TaskSize};
use tokage_orchestration::{
    assemble_task_brief, parse_or_generate_task_report, ConcurrencyLimits, DagScheduler,
    TaskBriefInput, TaskState, TASK_BRIEF_FIXED_HEADER,
};

fn sample_complex_plan() -> Plan {
    let mut plan = Plan::default();
    plan.tasks = vec![
        TaskItem {
            id: "T1".to_string(),
            domain: "backend".to_string(),
            title: "Database schema migration".to_string(),
            goal: "Setup tables".to_string(),
            depends_on: vec![],
            contracts: vec!["C1".to_string()],
            write_scope: vec!["src/db/".to_string()],
            deliverables: vec!["V1__init.sql".to_string()],
            acceptance: TaskAcceptance {
                commands: vec!["cargo test db".to_string()],
                criteria: vec!["Tables created".to_string()],
                ..Default::default()
            },
            size: TaskSize::S,
            difficulty: 1,
            assigned_model: None,
        },
        TaskItem {
            id: "T2".to_string(),
            domain: "backend".to_string(),
            title: "User service endpoints".to_string(),
            goal: "User CRUD logic".to_string(),
            depends_on: vec!["T1".to_string()],
            contracts: vec!["C1".to_string()],
            write_scope: vec!["src/users/".to_string()],
            deliverables: vec!["users.rs".to_string()],
            acceptance: TaskAcceptance {
                commands: vec!["cargo test users".to_string()],
                criteria: vec!["200 OK returned".to_string()],
                ..Default::default()
            },
            size: TaskSize::M,
            difficulty: 2,
            assigned_model: None,
        },
        TaskItem {
            id: "T3".to_string(),
            domain: "frontend".to_string(),
            title: "Frontend user dashboard".to_string(),
            goal: "User list UI".to_string(),
            depends_on: vec!["T1".to_string()],
            contracts: vec!["C1".to_string()],
            write_scope: vec!["src/web/users/".to_string()],
            deliverables: vec!["Users.tsx".to_string()],
            acceptance: TaskAcceptance {
                commands: vec!["pnpm test".to_string()],
                criteria: vec!["Render without error".to_string()],
                ..Default::default()
            },
            size: TaskSize::M,
            difficulty: 2,
            assigned_model: None,
        },
        TaskItem {
            id: "T4".to_string(),
            domain: "testing".to_string(),
            title: "Full integration test".to_string(),
            goal: "End to end integration suite".to_string(),
            depends_on: vec!["T2".to_string(), "T3".to_string()],
            contracts: vec![],
            write_scope: vec!["tests/integration/".to_string()],
            deliverables: vec!["e2e_spec.ts".to_string()],
            acceptance: TaskAcceptance {
                commands: vec!["cargo test -p e2e".to_string()],
                criteria: vec!["All pass".to_string()],
                ..Default::default()
            },
            size: TaskSize::L,
            difficulty: 2,
            assigned_model: None,
        },
    ];
    plan
}

#[test]
fn test_acceptance_p7_m1_dag_scheduling_lifecycle() {
    let plan = sample_complex_plan();
    let assignments = HashMap::from([
        ("T1".to_string(), ("claude".to_string(), "claude-3-7-sonnet".to_string())),
        ("T2".to_string(), ("claude".to_string(), "claude-3-7-sonnet".to_string())),
        ("T3".to_string(), ("copilot".to_string(), "gpt-4o".to_string())),
        ("T4".to_string(), ("codex".to_string(), "o3-mini".to_string())),
    ]);

    let mut scheduler = DagScheduler::new(&plan, &assignments, ConcurrencyLimits {
        max_global_concurrent: 4,
        default_provider_concurrent: 2,
        provider_overrides: HashMap::new(),
    });

    // 1. Initial State: Only T1 has no dependencies
    assert_eq!(scheduler.tasks["T1"].state, TaskState::Ready);
    assert_eq!(scheduler.tasks["T2"].state, TaskState::Pending);
    assert_eq!(scheduler.tasks["T3"].state, TaskState::Pending);
    assert_eq!(scheduler.tasks["T4"].state, TaskState::Pending);

    // 2. Dispatch T1
    let dispatchable = scheduler.pick_next_dispatchable_tasks();
    assert_eq!(dispatchable, vec!["T1"]);

    scheduler.update_task_state("T1", TaskState::Running).unwrap();
    assert_eq!(scheduler.tasks["T1"].state, TaskState::Running);

    scheduler.update_task_state("T1", TaskState::Verifying).unwrap();
    assert_eq!(scheduler.tasks["T1"].state, TaskState::Verifying);

    // 3. T1 Completes -> T2 & T3 should become READY
    let newly_ready = scheduler.update_task_state("T1", TaskState::Done).unwrap();
    assert!(newly_ready.contains(&"T2".to_string()));
    assert!(newly_ready.contains(&"T3".to_string()));
    assert_eq!(scheduler.tasks["T2"].state, TaskState::Ready);
    assert_eq!(scheduler.tasks["T3"].state, TaskState::Ready);

    // 4. Critical Path Prioritization: T2 and T3 both lead to T4
    // T2 (cp=2), T3 (cp=2). Both can run in parallel since write_scopes don't collide
    // (src/users/ vs src/web/users/) and providers are claude & copilot
    let next_batch = scheduler.pick_next_dispatchable_tasks();
    assert_eq!(next_batch.len(), 2);
    assert!(next_batch.contains(&"T2".to_string()));
    assert!(next_batch.contains(&"T3".to_string()));

    // 5. Contract deviation on T2 pauses downstream T4 (P7.m1c)
    scheduler.handle_contract_deviation("T2");
    assert!(scheduler.tasks["T4"].contract_deviation_paused);
    assert_eq!(scheduler.tasks["T4"].deviation_source_task.as_deref(), Some("T2"));

    // 6. Complete T3 and T2 (Running -> Verifying -> Done)
    scheduler.update_task_state("T2", TaskState::Running).unwrap();
    scheduler.update_task_state("T2", TaskState::Verifying).unwrap();
    scheduler.update_task_state("T2", TaskState::Done).unwrap();

    scheduler.update_task_state("T3", TaskState::Running).unwrap();
    scheduler.update_task_state("T3", TaskState::Verifying).unwrap();
    scheduler.update_task_state("T3", TaskState::Done).unwrap();

    // T4 has dependencies met (T2 and T3 done), but is PAUSED due to contract deviation
    assert!(scheduler.tasks["T4"].contract_deviation_paused);
    let dispatch_blocked = scheduler.pick_next_dispatchable_tasks();
    assert!(!dispatch_blocked.contains(&"T4".to_string()));

    // 7. Human review unpauses deviation -> T4 can now be dispatched
    scheduler.unpause_contract_deviation("T2");
    assert!(!scheduler.tasks["T4"].contract_deviation_paused);

    let dispatch_unpaused = scheduler.pick_next_dispatchable_tasks();
    assert_eq!(dispatch_unpaused, vec!["T4"]);
}

#[test]
fn test_acceptance_p7_m2_brief_assembly_and_report_lifecycle() {
    let plan = sample_complex_plan();
    let t2_item = &plan.tasks[1]; // T2

    let contracts = vec![PlanContract {
        id: "C1".to_string(),
        title: "User Auth Spec".to_string(),
        content: "POST /auth/login returns JWT token".to_string(),
    }];

    // 1. T1's generated task report becomes upstream artifact for T2
    let t1_item = &plan.tasks[0];
    let t1_raw_output = r#"
### 完成内容
完成数据库表设计与迁移脚本，包含 users 与 sessions 两个数据表。

### 偏离契约之处
无

### 给下游的提示
可通过 pool 获取连接，默认用户 ID 为 uuidv4。

### 验收标准自查
- Tables created: 是
"#;

    let t1_report = parse_or_generate_task_report(
        t1_item,
        t1_raw_output,
        "claude-3-7-sonnet",
        "att-1",
        true,
        "src/db/init.sql +150 -0",
        vec!["src/db/init.sql".to_string()],
        "cargo test db (passed)",
        Some(0.08),
    );

    assert_eq!(t1_report.first_pass, true);
    assert_eq!(t1_report.is_locally_generated, false);
    assert_eq!(t1_report.contract_deviations, None);

    let t1_upstream_artifact = t1_report.to_upstream_artifact();
    assert_eq!(t1_upstream_artifact.task_id, "T1");
    assert!(t1_upstream_artifact.report.contains("完成数据库表设计"));

    // 2. Assemble Task Brief for T2
    let brief_input = TaskBriefInput {
        task: t2_item.clone(),
        contracts,
        upstream_artifacts: vec![t1_upstream_artifact],
        failure_evidence: None,
    };

    let brief = assemble_task_brief(&brief_input);

    // Verify fixed header and format
    assert!(brief.starts_with(TASK_BRIEF_FIXED_HEADER.trim_start()));
    assert!(brief.contains("## 任务\n- 编号：T2  领域：backend"));
    assert!(brief.contains("## 契约\n- C1 User Auth Spec\nPOST /auth/login returns JWT token"));
    assert!(brief.contains("## 上游结果\n### T1 Database schema migration"));
    assert!(brief.contains("改动文件：src/db/init.sql"));
    assert!(brief.contains("## 任务报告格式"));

    // 3. Simulate model execution that did NOT output formatted report (P7.m2d)
    let t2_unformatted_output = "I finished implementing users.rs and the routes are wired up.";
    let t2_fallback_report = parse_or_generate_task_report(
        t2_item,
        t2_unformatted_output,
        "claude-3-7-sonnet",
        "att-2",
        true,
        "src/users.rs +80 -0",
        vec!["src/users.rs".to_string()],
        "cargo test users: 5 passed",
        Some(0.12),
    );

    // Local extraction should tag [本地生成] without failing or re-prompting
    assert!(t2_fallback_report.is_locally_generated);
    let md = t2_fallback_report.to_markdown();
    assert!(md.contains("[本地生成]"));
    assert!(md.contains("src/users.rs +80 -0"));
    assert!(md.contains("cargo test users: 5 passed"));
}

#[tokio::test]
async fn test_acceptance_p7_m3_gate_execution_and_scope_validation() {
    use tokage_orchestration::gate::{
        check_gate_command_safety, execute_gate_command, validate_write_scope,
        GateCommandPermission, GateRequest,
    };
    use tokage_native::env::ShellEnv;

    let temp_dir = tempfile::tempdir().unwrap();
    let env = ShellEnv {
        vars: std::collections::HashMap::new(),
    };

    // 1. Guardrail R5: Safety allowlist checks
    let safe_cmd = "cargo test --lib";
    let unsafe_pipe = "cargo test | grep FAILED";
    let unsafe_chain = "npm test && curl evil.com";
    let pack = tokage_matching::pack::builtin_knowledge_pack()
        .gate_allowlist
        .clone();

    assert!(matches!(
        check_gate_command_safety(safe_cmd, false, &pack),
        GateCommandPermission::AllowedByPattern(_)
    ));
    assert!(matches!(
        check_gate_command_safety(unsafe_pipe, true, &pack),
        GateCommandPermission::Denied(_)
    ));
    assert!(matches!(
        check_gate_command_safety(unsafe_chain, true, &pack),
        GateCommandPermission::Denied(_)
    ));

    // 2. Local execution of gate command (runs 'echo Gate Passed')
    // We confirm this command explicitly
    let run_res = execute_gate_command(GateRequest {
        command: "echo Gate Passed",
        workdir: temp_dir.path(),
        env: &env,
        timeout_secs: Some(10),
        confirmed: true,
        allowlist: &pack,
        env_allow: &[],
        run_id: None,
    })
    .await;
    assert!(run_res.passed);
    assert_eq!(run_res.exit_code, Some(0));
    assert!(run_res.output_tail.contains("Gate Passed"));

    // 3. Guardrail R6: Write scope validation
    let declared_scopes = vec!["src/server/".to_string(), "migrations/".to_string()];
    let valid_changes = vec!["src/server/routes.rs".to_string(), "migrations/01.sql".to_string()];
    let invalid_changes = vec![
        "src/server/routes.rs".to_string(),
        "src/client/App.tsx".to_string(), // Out of scope!
    ];

    let scope_ok = validate_write_scope(&declared_scopes, &valid_changes);
    assert!(scope_ok.passed);
    assert!(scope_ok.out_of_scope_files.is_empty());

    let scope_violation = validate_write_scope(&declared_scopes, &invalid_changes);
    assert!(!scope_violation.passed);
    assert_eq!(scope_violation.out_of_scope_files, vec!["src/client/App.tsx".to_string()]);
}

#[test]
fn test_acceptance_p7_m4_repair_three_step_ladder_and_defect_task() {
    use tokage_orchestration::repair::{
        decide_repair_action, generate_implementation_fix_task, FailureEvidence,
        RepairDecision, RepairStep,
    };

    let plan = sample_complex_plan();
    let backend_task = &plan.tasks[1]; // T2
    let testing_task = &plan.tasks[3]; // T4

    let failure_evidence = FailureEvidence {
        failed_command: Some("cargo test users".to_string()),
        output_tail: "test user_registration ... FAILED\nassertion `left == right` failed".to_string(),
        out_of_scope_files: vec!["src/config/secret.env".to_string()],
        previous_changed_files: vec!["src/users/users.rs".to_string()],
    };

    // --- 1. Step 1: In-window repair (warm cache) ---
    let (step1, action1) = decide_repair_action(
        backend_task,
        RepairStep::Initial,
        "att-backend-1",
        "claude-3-7-sonnet",
        "claude",
        &failure_evidence,
    );
    assert_eq!(step1, RepairStep::InWindowRepair);
    if let RepairDecision::TryInWindowRepair { attempt_id, evidence } = action1 {
        assert_eq!(attempt_id, "att-backend-1");
        assert!(evidence.contains("失败命令"));
        assert!(evidence.contains("越界改动文件"));
        assert!(evidence.contains("src/config/secret.env"));
    } else {
        panic!("Expected TryInWindowRepair");
    }

    // --- 2. Step 2: In-window repair fails -> Reassign ---
    let (step2, action2) = decide_repair_action(
        backend_task,
        RepairStep::InWindowRepair,
        "att-backend-1",
        "claude-3-7-sonnet",
        "claude",
        &failure_evidence,
    );
    assert_eq!(step2, RepairStep::Reassigned);
    if let RepairDecision::TryReassign { exclude_models, evidence } = action2 {
        assert_eq!(exclude_models, vec!["claude-3-7-sonnet".to_string()]);
        assert!(evidence.contains("test user_registration ... FAILED"));
    } else {
        panic!("Expected TryReassign");
    }

    // --- 3. Step 3: Reassignment also fails -> BLOCKED ---
    let (step3, action3) = decide_repair_action(
        backend_task,
        RepairStep::Reassigned,
        "att-backend-2",
        "o3-mini",
        "codex",
        &failure_evidence,
    );
    assert_eq!(step3, RepairStep::Blocked);
    if let RepairDecision::MarkBlocked { terminal_resume_command, reason } = action3 {
        assert_eq!(terminal_resume_command, "codex resume att-backend-2");
        assert!(reason.contains("人工干预"));
    } else {
        panic!("Expected MarkBlocked");
    }

    // --- 4. Testing task defects generate implementation fix task (P7.m4b) ---
    let fix_task = generate_implementation_fix_task(
        testing_task,
        backend_task,
        "密码未正确哈希存储，缺失 Argon2 加密",
        "e2e test verification failure",
    );

    assert_eq!(fix_task.id, "T2-fix");
    assert_eq!(fix_task.depends_on, vec!["T4".to_string()]);
    // The critical rule: write_scope stays constrained to implementation directory!
    assert_eq!(fix_task.write_scope, backend_task.write_scope);
    assert!(!fix_task.write_scope.contains(&"tests/integration/".to_string()));
    assert!(fix_task.goal.contains("缺失 Argon2 加密"));
}

#[test]
fn test_acceptance_p7_m5_and_m6_complete_closed_loop() {
    use tokage_orchestration::lifecycle::{generate_delivery_summary, recover_interrupted_tasks};
    use tokage_orchestration::tree::{
        derive_session_title_from_spec, AttemptNode, AttemptRole, MainSessionStatus,
        PlanNode, SessionTreeNode, TaskNode,
    };
    use tokage_orchestration::brief::TaskReport;
    use tokage_orchestration::gate::GateCommandResult;

    let plan = sample_complex_plan();

    // 1. Session Title derivation from Spec
    let session_title = derive_session_title_from_spec(&plan.spec);
    assert!(!session_title.is_empty());

    // 2. Breakpoint Recovery upon App Restart (P7.m5c)
    let mut scheduler = DagScheduler::new(&plan, &HashMap::new(), ConcurrencyLimits::default());
    scheduler.tasks.get_mut("T1").unwrap().state = TaskState::Running;
    let recovered = recover_interrupted_tasks(&mut scheduler);
    assert_eq!(recovered, vec!["T1".to_string()]);
    assert_eq!(scheduler.tasks["T1"].state, TaskState::Ready);

    // 3. Simulated Reports for all tasks to test Full Gate and Delivery Summary (P7.m5a)
    let reports = vec![
        TaskReport {
            task_id: "T1".to_string(),
            title: "Database schema migration".to_string(),
            model: "claude-3-7-sonnet".to_string(),
            attempt_id: "att-1".to_string(),
            first_pass: true,
            is_locally_generated: false,
            completed_work: "Done".to_string(),
            contract_deviations: None,
            tips_for_downstream: "Ready".to_string(),
            self_check_criteria: vec![],
            changed_files_stat: "schema.sql +50".to_string(),
            changed_files_list: vec!["src/db/V1__init.sql".to_string()],
            gate_results_summary: "passed".to_string(),
            cost_usd: Some(0.12),
        },
        TaskReport {
            task_id: "T2".to_string(),
            title: "User service endpoints".to_string(),
            model: "claude-3-7-sonnet".to_string(),
            attempt_id: "att-2".to_string(),
            first_pass: false,
            is_locally_generated: false,
            completed_work: "Done".to_string(),
            contract_deviations: None,
            tips_for_downstream: "Ready".to_string(),
            self_check_criteria: vec![],
            changed_files_stat: "users.rs +120".to_string(),
            changed_files_list: vec!["src/users/users.rs".to_string()],
            gate_results_summary: "passed".to_string(),
            cost_usd: Some(0.18),
        },
    ];

    let full_gate_result = GateCommandResult {
        command: "cargo test --workspace".to_string(),
        passed: true,
        exit_code: Some(0),
        duration_ms: 1540,
        output_tail: "test result: ok. All tests passed".to_string(),
        error_message: None,
    };

    let summary = generate_delivery_summary(&plan, &reports, Some(full_gate_result), 0.50, true);
    assert_eq!(summary.completed_tasks, 2);
    assert_eq!(summary.all_gates_passed, true);
    assert!(summary.touched_files.contains(&"src/db/V1__init.sql".to_string()));
    assert!(summary.touched_files.contains(&"src/users/users.rs".to_string()));
    assert_eq!(summary.actual_cost_usd, 0.30);
    assert!(summary.review_task.is_some());
    assert_eq!(summary.review_task.unwrap().domain, "review");

    // 4. Session Tree Node Construction (P7.m6a)
    let session_tree = SessionTreeNode {
        session_id: "sess-closed-loop".to_string(),
        project_id: "proj-1".to_string(),
        title: session_title,
        status: MainSessionStatus::Completed,
        plans: vec![PlanNode {
            plan_id: "plan-1".to_string(),
            version: 1,
            goal: plan.spec.goal.clone(),
            tasks: vec![
                TaskNode {
                    id: "T1".to_string(),
                    title: "Database schema migration".to_string(),
                    domain: "backend".to_string(),
                    state: TaskState::Done,
                    assigned_model: Some("claude-3-7-sonnet".to_string()),
                    attempts: vec![AttemptNode {
                        id: "att-1".to_string(),
                        task_id: Some("T1".to_string()),
                        role: AttemptRole::Executor,
                        provider_name: "claude".to_string(),
                        model_id: "claude-3-7-sonnet".to_string(),
                        predecessor_id: None,
                        reused_from_id: None,
                        status: "DONE".to_string(),
                        turn_count: 2,
                    }],
                },
                TaskNode {
                    id: "T2".to_string(),
                    title: "User service endpoints".to_string(),
                    domain: "backend".to_string(),
                    state: TaskState::Done,
                    assigned_model: Some("claude-3-7-sonnet".to_string()),
                    attempts: vec![
                        AttemptNode {
                            id: "att-2-old".to_string(),
                            task_id: Some("T2".to_string()),
                            role: AttemptRole::Executor,
                            provider_name: "claude".to_string(),
                            model_id: "claude-3-7-sonnet".to_string(),
                            predecessor_id: None,
                            reused_from_id: None,
                            status: "FAILED".to_string(),
                            turn_count: 1,
                        },
                        AttemptNode {
                            id: "att-2".to_string(),
                            task_id: Some("T2".to_string()),
                            role: AttemptRole::Executor,
                            provider_name: "claude".to_string(),
                            model_id: "claude-3-7-sonnet".to_string(),
                            predecessor_id: Some("att-2-old".to_string()),
                            reused_from_id: None,
                            status: "DONE".to_string(),
                            turn_count: 2,
                        },
                    ],
                },
            ],
        }],
        mounted_external_sessions: vec!["ext-codex-1".to_string()],
    };

    assert_eq!(session_tree.status, MainSessionStatus::Completed);
    assert_eq!(session_tree.plans[0].tasks[1].attempts[1].predecessor_id, Some("att-2-old".to_string()));
    assert_eq!(session_tree.mounted_external_sessions[0], "ext-codex-1");
}

/// P8.m2 Acceptance: FakeAdapter simulates quota exhaustion, trips circuit breaker,
/// and triggers automatic task reassignment to an alternative available model (docs/06 §6, P8.m2).
#[tokio::test]
async fn test_acceptance_fake_adapter_quota_exhaustion_and_auto_reassignment() {
    use tokage_core::planning::{Plan, PlanSpec, TaskItem, TaskSize};
    use tokage_core::{EventKind, ProviderId};
    use tokage_matching::{allocate_plan_models, builtin_knowledge_pack, merge_layers, reassign_unstarted_tasks, LayeredPacks};
    use tokage_native::chat::fake::{run_fake_adapter, FakeAdapter, Step};
    use tokage_native::env::ShellEnv;
    use tokage_orchestration::repair::{decide_repair_action_with_melted, RepairDecision, RepairStep};

    let work = tempfile::tempdir().unwrap();
    let mut vars = std::collections::HashMap::new();
    vars.insert("PATH".to_string(), "/usr/bin:/bin".to_string());
    vars.insert("HOME".to_string(), work.path().to_str().unwrap().to_string());
    let env = ShellEnv { vars };

    // 1. Simulate ClaudeCode running and hitting 429 quota exhaustion via FakeAdapter
    let fake = FakeAdapter::new(
        ProviderId::ClaudeCode,
        vec![
            Step::Emit(serde_json::json!({
                "fake": "quota",
                "raw": "Hit your 5-hour limit · resets 3pm",
                "reset_at": "2026-10-06T20:00:00Z",
                "retrying": false
            })),
            Step::Emit(serde_json::json!({
                "fake": "fail",
                "message": "Quota limit reached: 429 Too Many Requests"
            })),
        ],
    );
    let mut req = tokage_native::chat::testing::request(ProviderId::ClaudeCode);
    req.cwd = work.path().display().to_string();

    let r = run_fake_adapter(&fake, &env, &req).await;
    // Verify Quota event was observed
    let has_quota_event = r.events.iter().any(|e| matches!(e, EventKind::Quota { .. }));
    assert!(has_quota_event, "FakeAdapter must emit a Quota event on quota exhaustion: {:?}", r.events);

    // 2. Set up a plan with multiple tasks
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
            title: "Backend Task".into(),
            goal: "Implement API".into(),
            depends_on: vec![],
            contracts: vec![],
            write_scope: vec![],
            deliverables: vec![],
            acceptance: Default::default(),
            size: TaskSize::M,
            difficulty: 2,
            assigned_model: None,
        },
        TaskItem {
            id: "T2".into(),
            domain: "frontend-ui".into(),
            title: "Frontend Task".into(),
            goal: "Build UI".into(),
            depends_on: vec!["T1".into()],
            contracts: vec![],
            write_scope: vec![],
            deliverables: vec![],
            acceptance: Default::default(),
            size: TaskSize::S,
            difficulty: 1,
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
        is_user_edited: None,
    };

    // Initial allocation
    let initial_alloc = allocate_plan_models(&plan, &pack, None);
    assert_eq!(initial_alloc.assignments.len(), 2);

    // 3. Quota event trips circuit breaker -> record CLAUDE_CODE as melted
    let mut melted_providers = std::collections::HashSet::new();
    melted_providers.insert("CLAUDE_CODE".to_string());

    // 4. In-flight failure repair: when provider is melted, skip in-window retry and immediately advance to Reassign
    let evidence = tokage_orchestration::repair::FailureEvidence {
        failed_command: Some("cargo test".to_string()),
        output_tail: "429 Too Many Requests: Quota limit reached".to_string(),
        out_of_scope_files: vec![],
        previous_changed_files: vec![],
    };
    let (step, decision) = decide_repair_action_with_melted(
        &plan.tasks[0],
        RepairStep::Initial,
        "att-1",
        "claude-3-7-sonnet",
        "CLAUDE_CODE",
        true, // is_provider_melted
        &evidence,
    );
    assert_eq!(step, RepairStep::Reassigned, "Melted provider must immediately advance to Reassigned step");
    assert!(matches!(decision, RepairDecision::TryReassign { .. }), "Melted provider must trigger TryReassign decision");

    // 5. Automatic task reassignment for unstarted tasks
    let mut completed_or_running = std::collections::HashSet::new();
    completed_or_running.insert("T1".to_string()); // T1 was running/failed

    let reassigned = reassign_unstarted_tasks(
        &plan,
        &initial_alloc.assignments,
        &completed_or_running,
        &melted_providers,
        &pack,
        None,
    );

    assert_eq!(reassigned.assignments.len(), 2);
    let t2_assign = &reassigned.assignments[1];
    assert_eq!(t2_assign.task_id, "T2");
    // T2 was reassigned away from melted CLAUDE_CODE to an available provider
    assert_ne!(t2_assign.provider, "CLAUDE_CODE", "T2 must not be assigned to melted CLAUDE_CODE");
    assert!(t2_assign.p_success >= 0.70, "Alternative model must meet success threshold");
}


