//! Git hotspot analyzer extracting frequently changed files from recent git commits (P5.m1c).

use std::collections::HashMap;
use std::path::Path;
use tokage_core::planning::HotspotItem;

/// Analyzes git commit history (up to 50 commits) to find frequently modified files.
pub async fn analyze_git_hotspots(root: &Path, max_commits: usize, top_n: usize) -> Vec<HotspotItem> {
    let output = match tokio::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("log")
        .arg(format!("-n{max_commits}"))
        .arg("--name-only")
        .arg("--format=")
        .output()
        .await
    {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).to_string(),
        _ => return Vec::new(),
    };

    let mut counts: HashMap<String, u32> = HashMap::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        *counts.entry(trimmed.to_string()).or_insert(0) += 1;
    }

    let mut list: Vec<HotspotItem> = counts
        .into_iter()
        .map(|(path, commit_count)| HotspotItem { path, commit_count })
        .collect();

    list.sort_by(|a, b| b.commit_count.cmp(&a.commit_count).then_with(|| a.path.cmp(&b.path)));
    list.truncate(top_n);
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_analyze_git_hotspots_in_git_repo() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        // Initialize git repo and make commits
        let _ = tokio::process::Command::new("git").arg("-C").arg(root).arg("init").output().await;
        let _ = tokio::process::Command::new("git").arg("-C").arg(root).args(["config", "user.name", "test"]).output().await;
        let _ = tokio::process::Command::new("git").arg("-C").arg(root).args(["config", "user.email", "test@example.com"]).output().await;

        tokio::fs::write(root.join("hot.txt"), "1").await.unwrap();
        tokio::fs::write(root.join("cold.txt"), "1").await.unwrap();
        let _ = tokio::process::Command::new("git").arg("-C").arg(root).args(["add", "."]).output().await;
        let _ = tokio::process::Command::new("git").arg("-C").arg(root).args(["commit", "-m", "init"]).output().await;

        tokio::fs::write(root.join("hot.txt"), "2").await.unwrap();
        let _ = tokio::process::Command::new("git").arg("-C").arg(root).args(["add", "hot.txt"]).output().await;
        let _ = tokio::process::Command::new("git").arg("-C").arg(root).args(["commit", "-m", "update hot"]).output().await;

        let hotspots = analyze_git_hotspots(root, 50, 5).await;
        assert!(!hotspots.is_empty());
        assert_eq!(hotspots[0].path, "hot.txt");
        assert_eq!(hotspots[0].commit_count, 2);
    }
}
