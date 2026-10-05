//! RepoDigest generator module (docs/02 §3, P5.m1).

pub mod budget;
pub mod cache;
pub mod hotspots;
pub mod manifest;
pub mod symbols;
pub mod tree;

use std::path::{Path, PathBuf};
pub use budget::{estimate_tokens, format_and_trim_digest, DEFAULT_DIGEST_MAX_TOKENS};
pub use cache::DigestCache;
pub use tokage_core::planning::RepoDigest;

/// Builder for constructing RepoDigest with customizable options.
pub struct RepoDigestBuilder {
    root: PathBuf,
    max_tokens: usize,
    cache: Option<DigestCache>,
    conventions_filenames: Vec<String>,
}

impl RepoDigestBuilder {
    pub fn new<P: Into<PathBuf>>(root: P) -> Self {
        Self {
            root: root.into(),
            max_tokens: DEFAULT_DIGEST_MAX_TOKENS,
            cache: None,
            conventions_filenames: vec![
                "CLAUDE.md".to_string(),
                "AGENTS.md".to_string(),
                "README.md".to_string(),
                "CONTRIBUTING.md".to_string(),
            ],
        }
    }

    pub fn with_max_tokens(mut self, max: usize) -> Self {
        self.max_tokens = max;
        self
    }

    pub fn with_cache(mut self, cache: DigestCache) -> Self {
        self.cache = Some(cache);
        self
    }

    /// Generates or retrieves the cached project digest.
    pub async fn build(self) -> RepoDigest {
        if let Some(ref cache) = self.cache {
            if let Some(cached) = cache.get(&self.root).await {
                return cached;
            }
        }

        let digest = generate_digest_internal(&self.root, self.max_tokens, &self.conventions_filenames).await;

        if let Some(ref cache) = self.cache {
            cache.put(&self.root, digest.clone()).await;
        }

        digest
    }
}

/// Convenience function to generate RepoDigest directly.
pub async fn generate_digest(root: &Path, max_tokens: usize) -> RepoDigest {
    RepoDigestBuilder::new(root).with_max_tokens(max_tokens).build().await
}

async fn generate_digest_internal(
    root: &Path,
    max_tokens: usize,
    conventions_filenames: &[String],
) -> RepoDigest {
    let now = chrono_now_string();

    // 1. Manifests and build commands
    let (manifests, commands) = manifest::parse_manifests(root).await;

    // Detect primary languages
    let mut primary_languages = Vec::new();
    for m in &manifests {
        let lang = match m.manifest_type.as_str() {
            "cargo" => "Rust",
            "npm" => "TypeScript / JavaScript",
            "pyproject" => "Python",
            "go" => "Go",
            other => other,
        };
        if !primary_languages.contains(&lang.to_string()) {
            primary_languages.push(lang.to_string());
        }
    }

    // 2. Directory tree
    let tree_options = tree::TreeCrawlerOptions::default();
    let dir_summary = tree::crawl_directory(root, &tree_options).await;

    // 3. Project conventions / documentation summary
    let mut conventions = String::new();
    for fname in conventions_filenames {
        let p = root.join(fname);
        if let Ok(content) = tokio::fs::read_to_string(&p).await {
            conventions.push_str(&format!("### {fname}\n"));
            let lines: Vec<&str> = content.lines().take(40).collect();
            conventions.push_str(&lines.join("\n"));
            conventions.push('\n');
            break; // Include primary instructions file to save budget
        }
    }

    // 4. Git hotspots
    let hotspots = hotspots::analyze_git_hotspots(root, 50, 10).await;

    // 5. Public symbols & signatures: prioritize hotspot files and key source files
    let mut public_symbols = Vec::new();
    let mut files_to_scan = Vec::new();

    // Add hotspot files that exist
    for h in &hotspots {
        let p = root.join(&h.path);
        if p.is_file() {
            files_to_scan.push(p);
        }
    }

    // Add other source files discovered in tree
    for file_path in &dir_summary.all_files {
        let ext = file_path.extension().and_then(|s| s.to_str()).unwrap_or("");
        if matches!(ext, "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go") && !files_to_scan.contains(file_path) {
            files_to_scan.push(file_path.clone());
        }
        if files_to_scan.len() >= 50 {
            break;
        }
    }

    for file_path in files_to_scan {
        if let Ok(rel) = file_path.strip_prefix(root) {
            let rel_str = rel.to_string_lossy().to_string();
            let syms = symbols::extract_file_symbols(&file_path, &rel_str).await;
            public_symbols.extend(syms);
        }
    }

    // 6. Token budget enforcement and Markdown formatting
    let root_str = root.to_string_lossy().to_string();
    let (markdown, estimated_tokens) = budget::format_and_trim_digest(
        &root_str,
        &primary_languages,
        &manifests,
        &commands,
        &conventions,
        &dir_summary.tree_string,
        &public_symbols,
        &hotspots,
        max_tokens,
    );

    RepoDigest {
        project_path: root_str,
        generated_at: now,
        primary_languages,
        manifests,
        commands,
        conventions_summary: conventions,
        tree_summary: dir_summary.tree_string,
        public_symbols,
        hotspots,
        estimated_tokens,
        markdown,
    }
}

fn chrono_now_string() -> String {
    // Generate ISO timestamp without pulling full chrono if needed, or simple RFC3339
    let now = std::time::SystemTime::now();
    let dur = now.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    format!("{}.{}", dur.as_secs(), dur.subsec_millis())
}
