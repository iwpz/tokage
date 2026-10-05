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

