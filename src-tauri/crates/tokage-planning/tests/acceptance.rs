//! Acceptance and integration tests for RepoDigest (P5.m1).

use std::path::Path;
use tokage_planning::digest::{generate_digest, RepoDigestBuilder, DigestCache};

#[tokio::test]
async fn test_acceptance_tokage_repo_digest() {
    // Current workspace repository path
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    let digest = generate_digest(repo_root, 8000).await;

    // 1. Token budget <= 8000
    assert!(
        digest.estimated_tokens <= 8000,
        "RepoDigest tokens must be <= 8000, got {}",
        digest.estimated_tokens
    );

    // 2. Primary languages detected
    assert!(
        digest.primary_languages.contains(&"Rust".to_string()),
        "Primary languages must contain Rust"
    );

    // 3. Discovered build and test commands
    assert!(
        digest.commands.test.contains(&"cargo test".to_string()),
        "Discovered test commands must contain 'cargo test'"
    );
    assert!(
        digest.commands.build.contains(&"cargo build".to_string()),
        "Discovered build commands must contain 'cargo build'"
    );

    // 4. Manifests detected
    assert!(
        digest.manifests.iter().any(|m| m.manifest_type == "cargo"),
        "Must discover Cargo.toml manifest"
    );

    // 5. Hotspots detected (tokage repo has git history)
    assert!(
        !digest.hotspots.is_empty(),
        "Must discover git hotspots"
    );

    // 6. Tree summary contains crates
    assert!(
        digest.tree_summary.contains("crates/"),
        "Tree summary must contain crates/"
    );

    // 7. Markdown output contains key sections
    assert!(digest.markdown.contains("# Project Digest:"));
    assert!(digest.markdown.contains("## Discovered Build & Test Commands"));
    assert!(digest.markdown.contains("## Project Manifests"));
    assert!(digest.markdown.contains("## Recent Git Hotspots"));
    assert!(digest.markdown.contains("## Directory Tree"));
}

#[tokio::test]
async fn test_builder_with_cache() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    let cache = DigestCache::new();
    let builder1 = RepoDigestBuilder::new(repo_root).with_cache(cache.clone());
    let digest1 = builder1.build().await;

    let builder2 = RepoDigestBuilder::new(repo_root).with_cache(cache.clone());
    let digest2 = builder2.build().await;

    assert_eq!(digest1.generated_at, digest2.generated_at);
    assert_eq!(digest1.markdown, digest2.markdown);
}

#[test]
fn test_acceptance_plan_parsing_and_repair() {
    let standard_fixture = include_str!("fixtures/plan_standard.md");
    let malformed_fixture = include_str!("fixtures/plan_malformed.md");

    let opts = tokage_planning::PlanParseOptions {
        default_test_commands: vec!["cargo test".to_string()],
        default_domain: "general".to_string(),
    };

    // 1. Standard plan parse
    let plan = tokage_planning::parse_plan(standard_fixture, &opts)
        .expect("Standard plan should parse cleanly");
    assert_eq!(plan.spec.goal, "实现用户认证模块，包括注册、登录和 JWT 鉴权机制。");
    assert_eq!(plan.contracts.len(), 2);
    assert_eq!(plan.contracts[0].id, "C1");
    assert_eq!(plan.contracts[1].id, "C2");
    assert_eq!(plan.tasks.len(), 3);
    assert_eq!(plan.tasks[0].id, "T1");
    assert_eq!(plan.tasks[1].id, "T2");
    assert_eq!(plan.tasks[2].id, "T3");
    assert_eq!(plan.tasks[1].depends_on, vec!["T1"]);
    assert_eq!(plan.tasks[2].depends_on, vec!["T2"]);
    assert_eq!(plan.tasks[0].acceptance.commands, vec!["cargo test -p server db::users"]);

    // 2. Malformed plan parse with tolerant repair
    let repaired_plan = tokage_planning::parse_plan(malformed_fixture, &opts)
        .expect("Malformed plan should be repaired tolerantly");
    assert_eq!(repaired_plan.spec.goal, "快速修复登录会话过期的死锁问题。");
    assert_eq!(repaired_plan.contracts.len(), 1);
    assert_eq!(repaired_plan.contracts[0].id, "C1");
    assert_eq!(repaired_plan.tasks.len(), 2);

    // Auto-assigned IDs
    assert_eq!(repaired_plan.tasks[0].id, "T1");
    assert_eq!(repaired_plan.tasks[1].id, "T2");

    // Auto-repaired domain
    assert_eq!(repaired_plan.tasks[0].domain, "general");
    assert_eq!(repaired_plan.tasks[1].domain, "testing");

    // Auto-repaired acceptance command from defaults
    assert_eq!(repaired_plan.tasks[0].acceptance.commands, vec!["cargo test"]);

    // Clamped difficulty (was 99, must be clamped to 3)
    assert_eq!(repaired_plan.tasks[1].difficulty, 3);
}

#[test]
fn test_acceptance_planning_prompt_render() {
    let _repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    let digest_md = "# Tokage Digest\nCargo workspace";
    let prompt = tokage_planning::render_planning_prompt(tokage_planning::PlanningPromptArgs {
        user_request: "支持新的 Adapter 插件体系",
        repo_digest: digest_md,
        domains: Some(&["core".to_string(), "native".to_string()]),
        output_max_tokens: Some(5000),
        custom_format_spec: None,
    });

    assert!(prompt.contains("You are a technical planner"));
    assert!(prompt.contains("5000 tokens"));
    assert!(prompt.contains("Available domains: core, native"));
    assert!(prompt.contains("# Project digest\n# Tokage Digest"));
    assert!(prompt.contains("# Request\n支持新的 Adapter 插件体系"));
    assert!(prompt.contains("Match the language of the user request"));
}

#[test]
fn test_acceptance_plan_validation_and_persistence() {
    let standard_fixture = include_str!("fixtures/plan_standard.md");
    let parse_opts = tokage_planning::PlanParseOptions {
        default_test_commands: vec!["cargo test".to_string()],
        default_domain: "general".to_string(),
    };

    let plan = tokage_planning::parse_plan(standard_fixture, &parse_opts)
        .expect("Standard fixture should parse");

    // 1. Validation
    let val_opts = tokage_planning::PlanValidationOptions {
        max_tasks: 10,
        valid_domains: vec![
            "backend-api".to_string(),
            "frontend-ui".to_string(),
            "testing".to_string(),
            "general".to_string(),
        ],
        allowed_commands: vec!["cargo test".to_string()],
        gate_allowlist: Vec::new(),
        auto_serialize_conflicts: true,
    };
    let val_res = tokage_planning::validate_plan(plan, &val_opts);
    assert!(val_res.is_valid, "Standard plan must be valid");

    // 2. Persistence under temp dir
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();

    let persist_res = tokage_planning::persist_plan(root, "acceptance-plan-1", &val_res.plan)
        .expect("Persisting plan must succeed");
    assert_eq!(persist_res.plan_id, "acceptance-plan-1");
    assert!(root.join(".tokage/plans/acceptance-plan-1/spec.md").is_file());
    assert!(root.join(".tokage/plans/acceptance-plan-1/tasks.yaml").is_file());
    assert!(root.join(".tokage/plans/acceptance-plan-1/contracts/C1.md").is_file());
    assert!(root.join(".tokage/plans/acceptance-plan-1/contracts/C2.md").is_file());
    assert!(root.join(".tokage/plans/acceptance-plan-1/plan.md").is_file());

    // 3. Loading back
    let loaded = tokage_planning::load_persisted_plan(root, "acceptance-plan-1")
        .expect("Loading persisted plan must succeed");
    assert_eq!(loaded.spec.goal, val_res.plan.spec.goal);
    assert_eq!(loaded.tasks.len(), 3);
}

#[test]
fn test_acceptance_triage_benchmark() {
    #[derive(serde::Deserialize)]
    struct Sample {
        category: String,
        prompt: String,
        expected: String,
    }

    let raw = include_str!("fixtures/triage_samples.json");
    let samples: Vec<Sample> = serde_json::from_str(raw).expect("triage_samples.json must be valid");

    let total = samples.len();
    assert!(total >= 25, "Benchmark set must contain at least 25 samples, got {}", total);

    let mut correct = 0;
    let mut failures = Vec::new();

    for sample in &samples {
        let res = tokage_planning::triage(&sample.prompt, None);
        let actual = res.route.as_str();
        if actual == sample.expected {
            correct += 1;
        } else {
            failures.push(format!(
                "Category [{}]: prompt: {:?} => expected: {}, got: {} (plan_score: {}, direct_score: {}, reason: {})",
                sample.category, sample.prompt, sample.expected, actual, res.score_plan, res.score_direct, res.reason
            ));
        }
    }

    let accuracy = (correct as f64) / (total as f64);
    if !failures.is_empty() {
        eprintln!("Triage benchmark failures (accuracy: {:.1}%):", accuracy * 100.0);
        for f in &failures {
            eprintln!("  - {}", f);
        }
    }

    assert!(
        accuracy >= 0.95,
        "Triage accuracy must reach at least 95%, got {:.1}% ({}/{} correct)",
        accuracy * 100.0,
        correct,
        total
    );
}

#[test]
fn test_acceptance_incremental_replanning_lifecycle() {
    // End-to-end acceptance:
    // 1. Initial Plan parsed, validated, and persisted as v1
    // 2. Task 1 completed, Task 2 completed
    // 3. User requests scope change that modifies contract C1 and adds requirement
    // 4. Adaptation task is generated for completed Task 1
    // 5. Replan prompt is assembled within token limit across multiple replans
    // 6. Plan diff is computed and applied: plan becomes v2, parent is v1, history is archived
    let initial_markdown = r#"# Spec
## 目标
实现认证模块

## 范围
- 包含：登录注册
- 不包含：第三方登录

# Contracts
## C1 认证接口
```ts
type LoginReq = { username: string; passwordHash: string };
```

# Tasks
```yaml
- id: T1
  domain: backend-api
  title: 用户登录接口
  goal: 实现 C1
  depends_on: []
  contracts: [C1]
  write_scope: ["src/auth/**"]
  acceptance:
    commands: ["cargo test auth"]
  size: M
  difficulty: 2

- id: T2
  domain: frontend-ui
  title: 登录表单
  goal: 对接 C1
  depends_on: [T1]
  contracts: [C1]
  write_scope: ["web/src/auth/**"]
  acceptance:
    commands: ["pnpm test"]
  size: S
  difficulty: 1
```
"#;

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();

    // 1. Parse & Persist v1
    let plan_v1 = tokage_planning::parse_plan(initial_markdown, &tokage_planning::PlanParseOptions::default())
        .expect("Initial plan must parse");
    tokage_planning::persist_plan(root, "acceptance-replan", &plan_v1).unwrap();

    // 2. Simulate task execution reports
    let reports = vec![
        tokage_planning::TaskExecutionReport {
            task_id: "T1".into(),
            status: tokage_planning::ReplanTaskStatus::Done,
            completed_summary: "POST /login 已实现并集成 argon2 密码校验".into(),
            full_report: Some("POST /login 接口通过全部集成测试".into()),
            modified_files: vec!["src/auth/login.rs".into()],
        },
        tokage_planning::TaskExecutionReport {
            task_id: "T2".into(),
            status: tokage_planning::ReplanTaskStatus::Done,
            completed_summary: "登录表单与状态管理已就绪".into(),
            full_report: Some("登录表单 UI 与端到端单测已就绪".into()),
            modified_files: vec!["web/src/auth/LoginForm.tsx".into()],
        },
    ];

    // 3. Requirement change: C1 adds 2FA token
    let changed_contracts = vec!["C1".to_string()];
    let adaptation_tasks = tokage_planning::generate_adaptation_tasks(&plan_v1, &changed_contracts, &reports);
    assert_eq!(adaptation_tasks.len(), 2, "Both completed T1 and T2 touch C1, so both need adaptation tasks");
    assert!(adaptation_tasks.iter().any(|t| t.title.contains("用户登录接口")));
    assert!(adaptation_tasks.iter().any(|t| t.title.contains("登录表单")));

    // 4. Assemble replanning prompt under 16K budget
    let replan_prompt = tokage_planning::assemble_replan_prompt(tokage_planning::ReplanInputArgs {
        current_plan: &plan_v1,
        change_description: "用户登录需要支持两步验证 (2FA)",
        task_reports: &reports,
        affected_contract_ids: Some(&changed_contracts),
        domains: None,
        replan_input_max_tokens: Some(tokage_planning::DEFAULT_REPLAN_INPUT_MAX_TOKENS),
    });
    assert!(replan_prompt.contains("incremental replan"));
    assert!(replan_prompt.contains("两步验证 (2FA)"));
    assert!(replan_prompt.contains("POST /login 接口通过全部集成测试"));

    // 5. Compute diff and apply onto v2
    let mut proposed_plan = plan_v1.clone();
    proposed_plan.version = 2;
    proposed_plan.spec.goal = "实现支持 2FA 的认证模块".into();
    proposed_plan.contracts[0].content = "type LoginReq = { username: string; passwordHash: string; otp?: string };".into();
    proposed_plan.tasks.extend(adaptation_tasks);

    let diff = tokage_planning::compute_plan_diff(&plan_v1, &proposed_plan);
    assert_eq!(diff.from_version, 1);
    assert_eq!(diff.to_version, 2);
    assert!(diff.contracts.iter().any(|c| c.id == "C1" && c.action == tokage_planning::DiffAction::Modified));
    assert_eq!(diff.tasks.iter().filter(|t| t.action == tokage_planning::DiffAction::Added).count(), 2);

    let plan_v2 = tokage_planning::apply_plan_diff(&plan_v1, &diff, &proposed_plan, Some("acceptance-replan".into()));
    assert_eq!(plan_v2.version, 2);
    assert_eq!(plan_v2.tasks.len(), 4);

    // 6. Persist v2 and verify history archive of v1
    tokage_planning::persist_plan(root, "acceptance-replan", &plan_v2).unwrap();

    let archived_v1 = tokage_planning::load_persisted_plan_version(root, "acceptance-replan", Some(1)).unwrap();
    assert_eq!(archived_v1.version, 1);
    assert_eq!(archived_v1.spec.goal, "实现认证模块");

    let current_v2 = tokage_planning::load_persisted_plan(root, "acceptance-replan").unwrap();
    assert_eq!(current_v2.spec.goal, "实现支持 2FA 的认证模块");
    assert_eq!(current_v2.tasks.len(), 4);
}


