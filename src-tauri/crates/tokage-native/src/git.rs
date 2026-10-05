//! Git for a project directory: a read-only summary (branch, upstream, working-tree state),
//! session worktrees, and file reads for the preview pane. Shells out to the user's `git`
//! with the captured login-shell environment.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use serde::Serialize;

use crate::env::ShellEnv;

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitInfo {
    pub is_repo: bool,
    /// Branch name, or the short commit hash when HEAD is detached.
    pub branch: Option<String>,
    pub detached: bool,
    /// False in a freshly initialised repository (no commit yet).
    pub has_commits: bool,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    /// Entries in `git status`: staged, unstaged, untracked and conflicted paths.
    pub changes: u32,
    pub staged: u32,
    pub unstaged: u32,
    pub untracked: u32,
    pub conflicted: u32,
    /// Line counts of tracked changes against HEAD (staged and unstaged).
    pub insertions: u32,
    pub deletions: u32,
    /// The directory is a linked worktree, not the repository's main checkout.
    pub linked_worktree: bool,
    pub root: Option<String>,
}

/// Runs `git -C dir <args>`; stdout on success, stderr (or the reason) on failure.
pub async fn run(
    env: &ShellEnv,
    dir: &Path,
    args: &[&str],
    envs: &[(&str, &str)],
    secs: u64,
) -> Result<String, String> {
    let out = tokio::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env_clear()
        .envs(&env.vars)
        .envs(envs.iter().copied())
        // Never block on credential or pager prompts, and never take the index lock for reads.
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    let out = tokio::time::timeout(Duration::from_secs(secs), out)
        .await
        .map_err(|_| format!("git {} timed out", args.first().unwrap_or(&"")))?
        .map_err(|e| format!("git not available: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

async fn git(env: &ShellEnv, dir: &Path, args: &[&str]) -> Option<String> {
    run(env, dir, args, &[], 10)
        .await
        .ok()
        .map(|s| s.trim_end().to_string())
}

pub async fn info(env: &ShellEnv, dir: &Path) -> GitInfo {
    if !dir.is_dir() {
        return GitInfo::default();
    }
    let Some(rev) = git(
        env,
        dir,
        &[
            "rev-parse",
            "--is-inside-work-tree",
            "--show-toplevel",
            "--git-dir",
            "--git-common-dir",
        ],
    )
    .await
    else {
        return GitInfo::default();
    };
    let rev: Vec<&str> = rev.lines().collect();
    if rev.first() != Some(&"true") || rev.len() < 4 {
        return GitInfo::default();
    }
    let canon = |p: &str| {
        let p = Path::new(p);
        let p = if p.is_absolute() { p.to_path_buf() } else { dir.join(p) };
        p.canonicalize().unwrap_or(p)
    };
    let (status, stat) = tokio::join!(
        git(env, dir, &["status", "--porcelain=v2", "--branch", "--untracked-files=normal"]),
        git(env, dir, &["diff", "--shortstat", "HEAD"]),
    );
    let mut i = parse_status(status.as_deref().unwrap_or(""));
    (i.insertions, i.deletions) = parse_shortstat(stat.as_deref().unwrap_or(""));
    i.is_repo = true;
    i.root = Some(rev[1].to_string());
    i.linked_worktree = canon(rev[2]) != canon(rev[3]);
    i
}

/// `git status --porcelain=v2 --branch` (stable machine format).
fn parse_status(text: &str) -> GitInfo {
    let mut i = GitInfo::default();
    let mut oid = None;
    for line in text.lines() {
        if let Some(h) = line.strip_prefix("# branch.") {
            let (k, v) = h.split_once(' ').unwrap_or((h, ""));
            match k {
                "oid" => oid = (v != "(initial)").then(|| v.to_string()),
                "head" if v == "(detached)" => i.detached = true,
                "head" => i.branch = Some(v.to_string()),
                "upstream" => i.upstream = Some(v.to_string()),
                "ab" => {
                    for n in v.split_whitespace() {
                        if let Some(a) = n.strip_prefix('+') {
                            i.ahead = a.parse().unwrap_or(0);
                        } else if let Some(b) = n.strip_prefix('-') {
                            i.behind = b.parse().unwrap_or(0);
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        let mut cols = line.splitn(3, ' ');
        match (cols.next(), cols.next()) {
            (Some("1" | "2"), Some(xy)) => {
                let mut c = xy.chars();
                if c.next().is_some_and(|x| x != '.') {
                    i.staged += 1;
                }
                if c.next().is_some_and(|y| y != '.') {
                    i.unstaged += 1;
                }
                i.changes += 1;
            }
            (Some("u"), _) => {
                i.conflicted += 1;
                i.changes += 1;
            }
            (Some("?"), _) => {
                i.untracked += 1;
                i.changes += 1;
            }
            _ => {}
        }
    }
    i.has_commits = oid.is_some();
    if i.detached {
        i.branch = oid.map(|o| o.chars().take(7).collect());
    }
    i
}

/// " 3 files changed, 12 insertions(+), 4 deletions(-)"
fn parse_shortstat(text: &str) -> (u32, u32) {
    let (mut ins, mut del) = (0, 0);
    for part in text.split(',') {
        let mut w = part.split_whitespace();
        let (Some(n), Some(what)) = (w.next(), w.next()) else {
            continue;
        };
        let n = n.parse().unwrap_or(0);
        if what.starts_with("insertion") {
            ins = n;
        } else if what.starts_with("deletion") {
            del = n;
        }
    }
    (ins, del)
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Worktree {
    /// Worktree checkout root.
    pub root: String,
    /// Where turns run: the project's own subdirectory inside the worktree.
    pub cwd: String,
    pub branch: String,
}

/// Creates `tokage/<name>` from HEAD in a new linked worktree under `base`, so a session's
/// edits stay off the user's checkout. Uncommitted changes in the project are not carried over.
pub async fn create_worktree(
    env: &ShellEnv,
    project: &Path,
    base: &Path,
    name: &str,
) -> Result<Worktree, String> {
    let name: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(40)
        .collect();
    if name.is_empty() {
        return Err("invalid worktree name".into());
    }
    let rev = run(env, project, &["rev-parse", "--show-toplevel", "--show-prefix"], &[], 10)
        .await
        .map_err(|e| format!("Not a git repository: {e}"))?;
    let mut lines = rev.lines();
    let top = PathBuf::from(lines.next().unwrap_or_default().trim());
    let prefix = lines.next().unwrap_or_default().trim().to_string();
    run(env, &top, &["rev-parse", "--verify", "--quiet", "HEAD"], &[], 10)
        .await
        .map_err(|_| "This repository has no commits yet. A worktree needs at least one commit.".to_string())?;

    let repo = top
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "repo".into());
    let dir = base
        .join("worktrees")
        .join(format!("{repo}-{:08x}", fnv(&top.to_string_lossy())))
        .join(&name);
    if dir.exists() {
        return Err(format!("Worktree folder already exists: {}", dir.display()));
    }
    std::fs::create_dir_all(dir.parent().unwrap_or(base)).map_err(|e| e.to_string())?;
    let branch = format!("tokage/{name}");
    let path = dir.to_string_lossy().into_owned();
    run(env, &top, &["worktree", "add", "-b", &branch, &path, "HEAD"], &[], 60)
        .await
        .map_err(|e| format!("git worktree add failed: {e}"))?;
    let cwd = if prefix.is_empty() { dir.clone() } else { dir.join(&prefix) };
    Ok(Worktree {
        root: path,
        cwd: cwd.to_string_lossy().into_owned(),
        branch,
    })
}

/// Removes a session worktree Tokage created and deletes its `tokage/` branch. Refuses when
/// the checkout has uncommitted changes unless `force`, so archived work isn't lost silently.
/// A folder that is already gone only prunes git's record of it.
pub async fn remove_worktree(env: &ShellEnv, root: &Path, branch: &str, force: bool) -> Result<(), String> {
    if !branch.starts_with("tokage/") {
        return Err(format!("Not a Tokage worktree branch: {branch}"));
    }
    if !root.exists() {
        return Ok(());
    }
    let common = run(env, root, &["rev-parse", "--path-format=absolute", "--git-common-dir"], &[], 10)
        .await
        .map_err(|e| format!("Not a git worktree: {e}"))?;
    // The main checkout is the parent of the shared .git directory.
    let main = PathBuf::from(common.trim())
        .parent()
        .map(Path::to_path_buf)
        .ok_or("Can't locate the main checkout")?;
    if !force {
        let status = run(env, root, &["status", "--porcelain"], &[], 20).await?;
        if !status.trim().is_empty() {
            return Err("DIRTY".into());
        }
    }
    let path = root.to_string_lossy();
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(&path);
    run(env, &main, &args, &[], 60)
        .await
        .map_err(|e| format!("git worktree remove failed: {e}"))?;
    // Commits made on the branch stay reachable through reflog only; `-D` matches what the
    // user asked for (discard this session), and unmerged work was already guarded above.
    run(env, &main, &["branch", "-D", branch], &[], 20).await.ok();
    Ok(())
}

fn fnv(s: &str) -> u32 {
    s.bytes()
        .fold(0x811c_9dc5u32, |h, b| (h ^ b as u32).wrapping_mul(0x0100_0193))
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileText {
    /// `None` for binary or oversized files.
    pub text: Option<String>,
    pub binary: bool,
    pub size: u64,
}

const READ_MAX: u64 = 2 * 1024 * 1024;

/// Reads a text file for the preview pane. `path` may be relative to `root` or absolute,
/// but must resolve inside `root` (symlinks included).
pub fn read_text(root: &Path, path: &str) -> Result<FileText, String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let file = root
        .join(path)
        .canonicalize()
        .map_err(|_| format!("File not found: {path}"))?;
    if !file.starts_with(&root) {
        return Err("File is outside the project".into());
    }
    let size = std::fs::metadata(&file).map_err(|e| e.to_string())?.len();
    if size > READ_MAX {
        return Ok(FileText { text: None, binary: false, size });
    }
    let bytes = std::fs::read(&file).map_err(|e| e.to_string())?;
    if bytes.iter().take(8000).any(|b| *b == 0) {
        return Ok(FileText { text: None, binary: true, size });
    }
    Ok(FileText {
        text: Some(String::from_utf8_lossy(&bytes).into_owned()),
        binary: false,
        size,
    })
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitSnapshot {
    pub head: String,
    pub status: Vec<String>,
    pub created_at: u64,
}

/// Records a git snapshot before a turn executes in full_auto mode (P2.m6b).
/// Captures HEAD commit hash and all untracked/modified files, saved under `.tokage/snapshots/<turn_id>.json`.
pub async fn record_turn_snapshot(
    env: &ShellEnv,
    dir: &Path,
    turn_id: &str,
) -> Result<GitSnapshot, String> {
    let head = run(env, dir, &["rev-parse", "HEAD"], &[], 10)
        .await
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "HEAD".to_string());

    let status_out = run(
        env,
        dir,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        &[],
        15,
    )
    .await
    .unwrap_or_default();

    let mut status = Vec::new();
    for entry in status_out.split('\0') {
        if !entry.trim().is_empty() {
            status.push(entry.to_string());
        }
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let snapshot = GitSnapshot {
        head,
        status,
        created_at: now,
    };

    let snapshot_dir = dir.join(".tokage").join("snapshots");
    if let Ok(()) = std::fs::create_dir_all(&snapshot_dir) {
        let file = snapshot_dir.join(format!("{turn_id}.json"));
        if let Ok(json) = serde_json::to_string_pretty(&snapshot) {
            let _ = std::fs::write(file, json);
        }
    }

    Ok(snapshot)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcelain_v2() {
        let i = parse_status(
            "# branch.oid 1234567890abcdef\n# branch.head main\n# branch.upstream origin/main\n# branch.ab +2 -1\n\
             1 M. N... 100644 100644 100644 a b src/a.rs\n1 .M N... 100644 100644 100644 a b src/b.rs\n\
             1 MM N... 100644 100644 100644 a b src/c.rs\n? new.txt\nu UU N... 1 2 3 4 a b c x.rs\n",
        );
        assert_eq!(i.branch.as_deref(), Some("main"));
        assert_eq!((i.ahead, i.behind), (2, 1));
        assert_eq!((i.staged, i.unstaged, i.untracked, i.conflicted, i.changes), (2, 2, 1, 1, 5));
        assert!(i.has_commits);

        let d = parse_status("# branch.oid abcdef123456\n# branch.head (detached)\n");
        assert!(d.detached);
        assert_eq!(d.branch.as_deref(), Some("abcdef1"));
        assert!(!parse_status("# branch.oid (initial)\n# branch.head main\n").has_commits);
        assert_eq!(parse_shortstat(" 3 files changed, 12 insertions(+), 4 deletions(-)"), (12, 4));
        assert_eq!(parse_shortstat(" 1 file changed, 1 deletion(-)"), (0, 1));
    }

    #[test]
    fn read_text_stays_in_root() {
        let dir = std::env::temp_dir().join(format!("tokage-read-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub/a.txt"), "hi").unwrap();
        assert_eq!(read_text(&dir, "sub/a.txt").unwrap().text.as_deref(), Some("hi"));
        assert!(read_text(&dir.join("sub"), "../sub/../../etc/hosts").is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn worktree_for_session() {
        let env = ShellEnv {
            vars: std::env::vars().collect(),
        };
        let base = std::env::temp_dir().join(format!("tokage-wt-{}", std::process::id()));
        let repo = base.join("repo");
        std::fs::create_dir_all(repo.join("app")).unwrap();
        let g = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .output()
                .unwrap()
        };
        g(&["init", "-q"]);
        std::fs::write(repo.join("app/a.txt"), "a").unwrap();
        // No commit yet: refused with an explanation.
        assert!(create_worktree(&env, &repo.join("app"), &base, "s1").await.is_err());
        g(&["add", "."]);
        g(&["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", "init"]);
        let wt = create_worktree(&env, &repo.join("app"), &base, "s1").await.unwrap();
        assert_eq!(wt.branch, "tokage/s1");
        assert!(Path::new(&wt.cwd).join("a.txt").is_file());
        let i = info(&env, Path::new(&wt.cwd)).await;
        assert!(i.linked_worktree);
        assert_eq!(i.branch.as_deref(), Some("tokage/s1"));

        let root = Path::new(&wt.root);
        std::fs::write(root.join("app/b.txt"), "b").unwrap();
        assert_eq!(remove_worktree(&env, root, "tokage/s1", false).await, Err("DIRTY".into()));
        remove_worktree(&env, root, "tokage/s1", true).await.unwrap();
        assert!(!root.exists());
        assert!(g(&["branch", "--list", "tokage/s1"]).stdout.is_empty());
        assert!(remove_worktree(&env, root, "main", true).await.is_err());
        std::fs::remove_dir_all(&base).ok();
    }
}
