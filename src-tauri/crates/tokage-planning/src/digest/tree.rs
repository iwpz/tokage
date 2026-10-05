//! Directory tree crawler respecting .gitignore and folding large directories (P5.m1a).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Options for crawling directory trees.
pub struct TreeCrawlerOptions {
    pub max_depth: usize,
    pub max_entries_per_dir: usize,
    pub ignored_dirs: HashSet<String>,
}

impl Default for TreeCrawlerOptions {
    fn default() -> Self {
        let mut ignored = HashSet::new();
        ignored.insert(".git".to_string());
        ignored.insert(".tokage".to_string());
        ignored.insert("node_modules".to_string());
        ignored.insert("target".to_string());
        ignored.insert("dist".to_string());
        ignored.insert("build".to_string());
        ignored.insert(".next".to_string());
        ignored.insert(".turbo".to_string());
        ignored.insert("__pycache__".to_string());
        ignored.insert(".venv".to_string());
        ignored.insert("venv".to_string());
        ignored.insert(".idea".to_string());
        ignored.insert(".vscode".to_string());
        Self {
            max_depth: 4,
            max_entries_per_dir: 15,
            ignored_dirs: ignored,
        }
    }
}

/// Discovered directory tree summary and flat file list.
pub struct DirectorySummary {
    pub tree_string: String,
    pub all_files: Vec<PathBuf>,
    pub file_count: usize,
    pub dir_count: usize,
}

/// Crawls the project directory tree, honors .gitignore, and formats a folded ASCII tree.
pub async fn crawl_directory(root: &Path, options: &TreeCrawlerOptions) -> DirectorySummary {
    let gitignore_patterns = load_gitignore(root).await;
    let mut all_files = Vec::new();
    let mut lines = Vec::new();
    let mut file_count = 0;
    let mut dir_count = 0;

    lines.push(format!("{}/", root.file_name().and_then(|n| n.to_str()).unwrap_or(".")));

    crawl_recursive(
        root,
        root,
        0,
        "",
        options,
        &gitignore_patterns,
        &mut lines,
        &mut all_files,
        &mut file_count,
        &mut dir_count,
    ).await;

    DirectorySummary {
        tree_string: lines.join("\n"),
        all_files,
        file_count,
        dir_count,
    }
}

async fn load_gitignore(root: &Path) -> Vec<String> {
    let gitignore_path = root.join(".gitignore");
    let mut patterns = Vec::new();
    if let Ok(content) = tokio::fs::read_to_string(&gitignore_path).await {
        for line in content.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                patterns.push(trimmed.trim_end_matches('/').to_string());
            }
        }
    }
    patterns
}

fn is_ignored(name: &str, rel_path_str: &str, options: &TreeCrawlerOptions, gitignore_patterns: &[String]) -> bool {
    if options.ignored_dirs.contains(name) {
        return true;
    }
    for pat in gitignore_patterns {
        if pat.starts_with('*') {
            let suffix = pat.trim_start_matches('*');
            if name.ends_with(suffix) || rel_path_str.ends_with(suffix) {
                return true;
            }
        } else if name == pat || rel_path_str == pat || rel_path_str.starts_with(&format!("{pat}/")) || name.ends_with(pat) {
            return true;
        }
    }
    false
}

fn crawl_recursive<'a>(
    current: &'a Path,
    root: &'a Path,
    depth: usize,
    prefix: &'a str,
    options: &'a TreeCrawlerOptions,
    gitignore: &'a [String],
    lines: &'a mut Vec<String>,
    all_files: &'a mut Vec<PathBuf>,
    file_count: &'a mut usize,
    dir_count: &'a mut usize,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
    Box::pin(async move {
        if depth >= options.max_depth {
            return;
        }

        let mut read_dir = match tokio::fs::read_dir(current).await {
            Ok(rd) => rd,
            Err(_) => return,
        };

        let mut dirs = Vec::new();
        let mut files = Vec::new();

        while let Ok(Some(entry)) = read_dir.next_entry().await {
            let file_name = entry.file_name().to_string_lossy().to_string();
            let path = entry.path();
            let rel_path = path.strip_prefix(root).unwrap_or(&path);
            let rel_str = rel_path.to_string_lossy();

            if is_ignored(&file_name, &rel_str, options, gitignore) {
                continue;
            }

            if let Ok(file_type) = entry.file_type().await {
                if file_type.is_dir() {
                    dirs.push((file_name, path));
                } else if file_type.is_file() {
                    files.push((file_name, path));
                }
            }
        }

        dirs.sort_by(|a, b| a.0.cmp(&b.0));
        files.sort_by(|a, b| a.0.cmp(&b.0));

        *dir_count += dirs.len();
        *file_count += files.len();

        let total_items = dirs.len() + files.len();
        let fold_excess = total_items > options.max_entries_per_dir;
        let max_show = if fold_excess { options.max_entries_per_dir } else { total_items };

        let mut shown = 0;

        // Output directories first
        for (name, path) in dirs {
            if shown >= max_show {
                break;
            }
            shown += 1;
            let is_last = shown == total_items;
            let branch = if is_last { "└── " } else { "├── " };
            let next_prefix = if is_last { format!("{prefix}    ") } else { format!("{prefix}│   ") };

            lines.push(format!("{prefix}{branch}{name}/"));

            crawl_recursive(
                &path,
                root,
                depth + 1,
                &next_prefix,
                options,
                gitignore,
                lines,
                all_files,
                file_count,
                dir_count,
            ).await;
        }

        // Output files
        for (name, path) in files {
            all_files.push(path.clone());
            if shown >= max_show {
                continue;
            }
            shown += 1;
            let is_last = shown == total_items;
            let branch = if is_last { "└── " } else { "├── " };
            lines.push(format!("{prefix}{branch}{name}"));
        }

        if fold_excess {
            let remaining = total_items - max_show;
            lines.push(format!("{prefix}└── ... and {remaining} more items"));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_crawl_directory_ignores_and_folds() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        // Create directory structure
        tokio::fs::create_dir_all(root.join("src/nested")).await.unwrap();
        tokio::fs::create_dir_all(root.join("target")).await.unwrap();
        tokio::fs::create_dir_all(root.join(".git")).await.unwrap();

        tokio::fs::write(root.join(".gitignore"), "target\n*.log").await.unwrap();
        tokio::fs::write(root.join("Cargo.toml"), "[package]").await.unwrap();
        tokio::fs::write(root.join("src/main.rs"), "fn main() {}").await.unwrap();
        tokio::fs::write(root.join("src/nested/mod.rs"), "pub mod foo;").await.unwrap();
        tokio::fs::write(root.join("app.log"), "error").await.unwrap();

        let options = TreeCrawlerOptions::default();
        let summary = crawl_directory(root, &options).await;

        assert!(summary.tree_string.contains("Cargo.toml"));
        assert!(summary.tree_string.contains("src/"));
        assert!(!summary.tree_string.contains("target/"));
        assert!(!summary.tree_string.contains(".git/"));
        assert!(!summary.tree_string.contains("app.log"));
    }
}
