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

    assert!(prompt.contains("你是技术规划者"));
    assert!(prompt.contains("5000 Token"));
    assert!(prompt.contains("可用领域：core, native"));
    assert!(prompt.contains("# 项目摘要\n# Tokage Digest"));
    assert!(prompt.contains("# 需求\n支持新的 Adapter 插件体系"));
}

