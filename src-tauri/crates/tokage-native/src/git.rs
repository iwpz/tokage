//! Git for a project directory: a read-only summary (branch, upstream, working-tree state),
//! session worktrees, and file reads for the preview pane. Shells out to the user's `git`
//! with the captured login-shell environment.

pub mod scope;
pub mod rollback;
#[cfg(test)]
mod m2c_tests;
#[cfg(test)]
mod m2d_tests;
#[cfg(test)]
mod m2e_tests;

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use serde::{Serialize, Deserialize};

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
    /// Author name of `HEAD` commit when `has_commits` is true.
    pub head_author_name: Option<String>,
    /// Author email of `HEAD` commit when `has_commits` is true.
    pub head_author_email: Option<String>,
    /// Co-author identities parsed from `Co-authored-by:` trailers on `HEAD`.
    pub head_co_authors: Vec<String>,
    /// True when `HEAD` includes a `Co-authored-by: Tokage <...>` trailer.
    pub head_co_authored_by_tokage: bool,
}

/// Runs `git -C dir <args>`; stdout on success, stderr (or the reason) on failure.
pub async fn run(
    env: &ShellEnv,
    dir: &Path,
    args: &[&str],
    envs: &[(&str, &str)],
    secs: u64,
) -> Result<String, String> {
    run_bytes(env, dir, args, envs, secs).await
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

async fn git_output(env: &ShellEnv, dir: &Path, args: &[&str], envs: &[(&str, &str)], secs: u64) -> Result<std::process::Output, String> {
    let mut cmd = tokio::process::Command::new("git");
    cmd
        .arg("-C")
        .arg(dir)
        .args(args)
        .env_clear()
        .envs(&env.vars)
        // Repository identity comes from `-C`, including isolated debug fixtures.
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .envs(envs.iter().copied())
        // Never block on credential or pager prompts, and never take the index lock for reads.
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    crate::proc::isolate(&mut cmd);
    let pending=crate::proc::scoped_run_directory().map(|dir|crate::proc::PendingOperation::prepare(&dir)).transpose().map_err(|e|format!("Git process recovery required: {e}"))?;
    let child=match cmd.spawn(){Ok(child)=>child,Err(e)=>{if let Some(p)=pending{let _=p.unspawned();}return Err(format!("git not available: {e}"));}};
    if let Some(p)=&pending{p.spawned(child.id().ok_or("Git process identity missing")?).map_err(|e|e.to_string())?;}
    let group = crate::proc::Group::with_dir_cmd(&child, crate::proc::get_run_dir(), Some("git".into()));
    let output = tokio::time::timeout(Duration::from_secs(secs), child.wait_with_output()).await;
    group.kill().await;
    if let Some(p)=pending {p.wait_completed().await.map_err(|e|e.to_string())?;}
    output.map_err(|_| format!("git {} timed out after {secs}s", args.join(" ")))?
        .map_err(|e| format!("git not available: {e}"))
}

async fn run_bytes(env: &ShellEnv, dir: &Path, args: &[&str], envs: &[(&str, &str)], secs: u64) -> Result<Vec<u8>, String> {
    let out = git_output(env, dir, args, envs, secs).await?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(format!(
            "git {} exited {}: stderr: {}; stdout: {}",
            args.join(" "), out.status,
            String::from_utf8_lossy(&out.stderr).trim(),
            String::from_utf8_lossy(&out.stdout).trim(),
        ))
    }
}

async fn git(env: &ShellEnv, dir: &Path, args: &[&str]) -> Option<String> {
    run(env, dir, args, &[], 10)
        .await
        .ok()
        .map(|s| s.trim_end().to_string())
}

/// Like `run`, but treats exit codes 0 and 1 as success (git diff returns 1 when changes exist
/// with `--exit-code` / `--no-index`).
async fn run_diff(
    env: &ShellEnv,
    dir: &Path,
    args: &[&str],
    secs: u64,
) -> Result<String, String> {
    let out = git_output(env, dir, args, &[], secs).await?;
    let code = out.status.code().unwrap_or(-1);
    if code == 0 || code == 1 {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
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
    let (status, stat, head_meta) = tokio::join!(
        git(env, dir, &["status", "--porcelain=v2", "--branch", "--untracked-files=normal"]),
        git(env, dir, &["diff", "--shortstat", "HEAD"]),
        git(env, dir, &["log", "-1", "--format=%an%n%ae%n%B", "HEAD"]),
    );
    let mut i = parse_status(status.as_deref().unwrap_or(""));
    (i.insertions, i.deletions) = parse_shortstat(stat.as_deref().unwrap_or(""));
    if let Some(meta) = head_meta.as_deref() {
        let mut lines = meta.lines();
        i.head_author_name = lines.next().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
        i.head_author_email = lines.next().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
        for line in lines {
            let trimmed = line.trim();
            if trimmed.len() >= 15 && trimmed[..15].eq_ignore_ascii_case("co-authored-by:") {
                let ident = trimmed[15..].trim();
                if !ident.is_empty() {
                    let is_tokage = ident.to_ascii_lowercase().starts_with("tokage ")
                        || ident.to_ascii_lowercase().starts_with("tokage-app")
                        || ident.eq_ignore_ascii_case("tokage")
                        || ident.to_ascii_lowercase().contains(&format!("<{TOKAGE_CO_AUTHOR_EMAIL}>"))
                        || ident.to_ascii_lowercase().contains("<tokage@localhost>");
                    if is_tokage {
                        i.head_co_authored_by_tokage = true;
                    }
                    i.head_co_authors.push(ident.to_string());
                }
            }
        }
    }
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

#[derive(Clone, Debug, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Worktree {
    /// Worktree checkout root.
    pub root: String,
    /// Where turns run: the project's own subdirectory inside the worktree.
    pub cwd: String,
    pub branch: String,
}

/// Creates `tokage/<name>` from `base_ref` (default `HEAD`) in a new linked worktree under
/// `base`, so a session's edits stay off the user's checkout. Uncommitted changes in the
/// project are not carried over (F11: `base_ref` lets task worktrees start from the
/// integration branch).
pub async fn create_worktree(
    env: &ShellEnv,
    project: &Path,
    base: &Path,
    name: &str,
    base_ref: Option<&str>,
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
    let start_ref = base_ref.unwrap_or("HEAD");
    run(env, &top, &["rev-parse", "--verify", "--quiet", start_ref], &[], 10)
        .await
        .map_err(|_| {
            if start_ref == "HEAD" {
                "This repository has no commits yet. A worktree needs at least one commit."
                    .to_string()
            } else {
                format!("Unknown base_ref: {start_ref}")
            }
        })?;

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
    run(
        env,
        &top,
        &["worktree", "add", "-b", &branch, &path, start_ref],
        &[],
        60,
    )
    .await
    .map_err(|e| format!("git worktree add failed: {e}"))?;
    let cwd = if prefix.is_empty() {
        dir.clone()
    } else {
        dir.join(&prefix)
    };
    Ok(Worktree {
        root: path,
        cwd: cwd.to_string_lossy().into_owned(),
        branch,
    })
}

pub const TOKAGE_CO_AUTHOR_NAME: &str = "Tokage";
pub const TOKAGE_CO_AUTHOR_EMAIL: &str = "345094078+tokage-app[bot]@users.noreply.github.com";
pub const TOKAGE_CO_AUTHOR_TRAILER: &str =
    "Co-authored-by: Tokage <345094078+tokage-app[bot]@users.noreply.github.com>";

/// Formats a commit message, optionally appending the Tokage `Co-authored-by` trailer.
pub fn format_commit_message(message: &str, co_author: bool) -> String {
    let trimmed = message.trim_end();
    if !co_author {
        return trimmed.to_string();
    }
    if trimmed
        .lines()
        .any(|line| line.trim().eq_ignore_ascii_case(TOKAGE_CO_AUTHOR_TRAILER))
    {
        return trimmed.to_string();
    }
    format!("{trimmed}\n\n{TOKAGE_CO_AUTHOR_TRAILER}")
}

/// Resolves the original author identity for `dir` so Tokage commits are always authored
/// by the original author (and never solely by Tokage).
/// Order: git config (`user.name`/`user.email`), environment variables, `HEAD` commit author,
/// and finally system username fallback.
pub async fn resolve_author_identity(env: &ShellEnv, dir: &Path) -> (String, String) {
    let is_tokage_name = |s: &str| {
        let lower = s.trim().to_ascii_lowercase();
        lower == "tokage"
            || lower.starts_with("tokage ")
            || lower.starts_with("tokage-")
            || lower.starts_with("tokage_")
    };
    let is_tokage_email = |s: &str| {
        let lower = s.trim().to_ascii_lowercase();
        lower == TOKAGE_CO_AUTHOR_EMAIL.to_ascii_lowercase()
            || lower == "tokage@localhost"
            || lower.ends_with("@tokage.invalid")
            || lower.starts_with("tokage@")
            || lower.contains("+tokage-app[bot]@")
    };
    let cfg_name = git(env, dir, &["config", "--get", "user.name"])
        .await
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && !is_tokage_name(s));
    let cfg_email = git(env, dir, &["config", "--get", "user.email"])
        .await
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && !is_tokage_email(s));

    let global_name = if cfg_name.is_none() {
        git(env, dir, &["config", "--global", "--get", "user.name"])
            .await
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && !is_tokage_name(s))
    } else {
        None
    };
    let global_email = if cfg_email.is_none() {
        git(env, dir, &["config", "--global", "--get", "user.email"])
            .await
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && !is_tokage_email(s))
    } else {
        None
    };

    let env_name = env
        .vars
        .get("GIT_AUTHOR_NAME")
        .or_else(|| env.vars.get("GIT_COMMITTER_NAME"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && !is_tokage_name(s));
    let env_email = env
        .vars
        .get("GIT_AUTHOR_EMAIL")
        .or_else(|| env.vars.get("GIT_COMMITTER_EMAIL"))
        .or_else(|| env.vars.get("EMAIL"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && !is_tokage_email(s));

    let mut name = cfg_name.or(global_name).or(env_name);
    let mut email = cfg_email.or(global_email).or(env_email);

    if name.is_none() || email.is_none() {
        if let Some(head_log) = git(env, dir, &["log", "-n", "20", "--format=%an%x09%ae", "HEAD"]).await {
            for line in head_log.lines() {
                let Some((an, ae)) = line.split_once('\t') else {
                    continue;
                };
                let an = an.trim();
                let ae = ae.trim();
                if name.is_none() && !an.is_empty() && !is_tokage_name(an) {
                    name = Some(an.to_owned());
                }
                if email.is_none() && !ae.is_empty() && !is_tokage_email(ae) {
                    email = Some(ae.to_owned());
                }
                if name.is_some() && email.is_some() {
                    break;
                }
            }
        }
    }

    let final_name = name
        .or_else(|| {
            env.vars
                .get("USER")
                .or_else(|| env.vars.get("USERNAME"))
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty() && !is_tokage_name(s))
        })
        .unwrap_or_else(|| "Developer".to_string());
    let final_email = email.unwrap_or_else(|| {
        let slug: String = final_name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(*c, '.' | '_' | '-'))
            .collect();
        let slug = if slug.is_empty() { "developer" } else { &slug };
        format!("{slug}@localhost")
    });
    (final_name, final_email)
}

/// Resolve a revision to a full commit SHA.
pub async fn rev_parse(env: &ShellEnv, dir: &Path, rev: &str) -> Result<String, String> {
    let out = run(env, dir, &["rev-parse", rev], &[], 10).await?;
    Ok(out.trim().to_string())
}

/// Stage all changes and commit under the original author's identity (F11). Returns `false`
/// when there is nothing to commit.
pub async fn commit_worktree(env: &ShellEnv, root: &Path, message: &str) -> Result<bool, String> {
    commit_worktree_with_options(env, root, message, false).await
}

pub async fn commit_worktree_with_options(
    env: &ShellEnv,
    root: &Path,
    message: &str,
    co_author: bool,
) -> Result<bool, String> {
    run(env, root, &["add", "-A"], &[], 30)
        .await
        .map_err(|e| format!("git add failed: {e}"))?;
    let status = run(env, root, &["status", "--porcelain"], &[], 20).await?;
    if status.trim().is_empty() {
        return Ok(false);
    }
    let (author_name, author_email) = resolve_author_identity(env, root).await;
    let user_name_cfg = format!("user.name={author_name}");
    let user_email_cfg = format!("user.email={author_email}");
    let full_msg = format_commit_message(message, co_author);
    run(
        env,
        root,
        &[
            "-c",
            &user_name_cfg,
            "-c",
            &user_email_cfg,
            "commit",
            "-m",
            &full_msg,
        ],
        &[],
        60,
    )
    .await
    .map_err(|e| format!("git commit failed: {e}"))?;
    Ok(true)
}

/// Cached changed paths vs `base_commit` after `git add -A` (F11.h worktree R6).
pub async fn staged_diff_names(
    env: &ShellEnv,
    root: &Path,
    base_commit: &str,
) -> Result<Vec<String>, String> {
    run(env, root, &["add", "-A"], &[], 30)
        .await
        .map_err(|e| format!("git add failed: {e}"))?;
    let out = run(
        env,
        root,
        &["diff", "--cached", "--name-only", base_commit],
        &[],
        30,
    )
    .await
    .map_err(|e| format!("git diff failed: {e}"))?;
    Ok(out
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

/// Removes a Tokage worktree. When `keep_branch` is true, the `tokage/` branch is retained
/// (F11: task branches live until plan delivery). Refuses dirty checkouts unless `force`.
pub async fn remove_worktree(
    env: &ShellEnv,
    root: &Path,
    branch: &str,
    force: bool,
    keep_branch: bool,
) -> Result<(), String> {
    remove_worktree_if_head(env, root, branch, force, keep_branch, None).await
}

pub async fn remove_worktree_if_head(
    env: &ShellEnv, root: &Path, branch: &str, force: bool, keep_branch: bool,
    expected_commit: Option<&str>,
) -> Result<(), String> {
    if !branch.starts_with("tokage/") {
        return Err(format!("Not a Tokage worktree branch: {branch}"));
    }
    if !root.exists() {
        return Ok(());
    }
    let actual = checkout_identity(env, root).await?;
    if expected_commit.is_some_and(|head| head != actual.head) {
        return Err("Worktree HEAD changed; refusing cleanup".into());
    }
    if actual.reference != format!("refs/heads/{branch}") {
        return Err("Worktree branch changed; refusing cleanup".into());
    }
    let common = run(
        env,
        root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        &[],
        10,
    )
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
    if !keep_branch {
        run(env, &main, &["branch", "-D", branch], &[], 20).await.ok();
    }
    Ok(())
}

/// Whether `path` resolves under `{app_data}/worktrees/` (F11 path guard).
pub fn is_under_worktrees_dir(path: &Path, app_data: &Path) -> bool {
    let Ok(canon_path) = path.canonicalize() else {
        // Not yet created / missing — compare lexical prefix of absolutized paths.
        let abs = if path.is_absolute() {
            path.to_path_buf()
        } else {
            return false;
        };
        let base = app_data.join("worktrees");
        return abs.starts_with(&base);
    };
    let Ok(canon_base) = app_data.join("worktrees").canonicalize() else {
        return false;
    };
    canon_path.starts_with(&canon_base)
}

/// True when `git status --porcelain` is empty.
pub async fn is_working_tree_clean(env: &ShellEnv, dir: &Path) -> Result<bool, String> {
    let status = run(env, dir, &["status", "--porcelain"], &[], 20).await?;
    Ok(status.trim().is_empty())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckoutIdentity {
    pub head: String,
    pub reference: String,
}

pub async fn checkout_identity(env: &ShellEnv, dir: &Path) -> Result<CheckoutIdentity, String> {
    Ok(CheckoutIdentity {
        head: rev_parse(env, dir, "HEAD").await?,
        reference: run(env, dir, &["rev-parse", "--symbolic-full-name", "HEAD"], &[], 10).await?.trim().into(),
    })
}

/// Merge `branch` into `checkout` with `--no-ff` (F11.f delivery into user main checkout).
/// Refuses when the working tree is dirty (`Err("DIRTY")`). On conflict: `merge --abort`
/// then `Err("CONFLICT")`.
/// Validate accepted source content again at the native mutation boundary, then merge its SHA.
pub async fn merge_accepted_into_checkout(
    env: &ShellEnv, checkout: &Path, branch: &str, expected: &scope::ScopeSnapshot, target: Option<&CheckoutIdentity>,
) -> Result<(), String> {
    merge_accepted_into_checkout_with_options(env, checkout, branch, expected, target, false).await
}

pub async fn merge_accepted_into_checkout_with_options(
    env: &ShellEnv,
    checkout: &Path,
    branch: &str,
    expected: &scope::ScopeSnapshot,
    target: Option<&CheckoutIdentity>,
    co_author: bool,
) -> Result<(), String> {
    let current = scope::snapshot(env, Path::new(&expected.workdir)).await?;
    if current != *expected || !current.clean {
        return Err("Acceptance evidence is stale or source checkout is dirty".into());
    }
    let source = rev_parse(env, checkout, branch).await?;
    if source != expected.head { return Err("Acceptance source branch changed".into()); }
    let target = target.ok_or("TARGET identity missing; keep branch or export patch instead")?;
    if checkout_identity(env, checkout).await? != *target {
        return Err("TARGET branch or HEAD changed; keep branch or export patch instead".into());
    }
    merge_branch_into_checkout_with_options(env, checkout, &source, co_author).await
}

/// Reconcile a lost delivery acknowledgement using exact parents and the expected merge tree.
pub async fn reconcile_delivery_merge(env: &ShellEnv, checkout: &Path, source: &str, target: &CheckoutIdentity) -> Result<bool, String> {
    let current = checkout_identity(env, checkout).await?;
    if current.reference != target.reference || !is_working_tree_clean(env, checkout).await? {
        return Err("Delivery target branch or content changed; preserve evidence for manual reconciliation".into());
    }
    if current.head == target.head { return Ok(false); }
    let parents = run(env, checkout, &["show", "-s", "--format=%P", &current.head], &[], 10).await?;
    if parents.split_whitespace().collect::<Vec<_>>() != vec![target.head.as_str(), source] {
        return Err("Delivery target does not match the recorded merge transaction".into());
    }
    let expected = run(env, checkout, &["merge-tree", "--write-tree", &target.head, source], &[], 30).await?;
    let actual = rev_parse(env, checkout, &format!("{}^{{tree}}", current.head)).await?;
    if expected.lines().next() != Some(actual.as_str()) { return Err("Delivery merge tree differs from accepted source".into()); }
    Ok(true)
}

pub async fn merge_branch_into_checkout(
    env: &ShellEnv,
    checkout: &Path,
    branch: &str,
) -> Result<(), String> {
    merge_branch_into_checkout_with_options(env, checkout, branch, false).await
}

pub async fn merge_branch_into_checkout_with_options(
    env: &ShellEnv,
    checkout: &Path,
    branch: &str,
    co_author: bool,
) -> Result<(), String> {
    if !is_working_tree_clean(env, checkout).await? {
        return Err("DIRTY".into());
    }
    // Never abort or overwrite a merge/rebase started by the user.
    for marker in ["MERGE_HEAD", "CHERRY_PICK_HEAD", "REVERT_HEAD", "rebase-merge", "rebase-apply"] {
        let path = run(env, checkout, &["rev-parse", "--path-format=absolute", "--git-path", marker], &[], 10).await?;
        if Path::new(path.trim()).exists() { return Err(format!("Git operation already in progress: {marker}")); }
    }
    let source = rev_parse(env, checkout, branch).await?;
    let msg = format_commit_message(&format!("Merge {branch} (Tokage delivery)"), co_author);
    let (author_name, author_email) = resolve_author_identity(env, checkout).await;
    let user_name_cfg = format!("user.name={author_name}");
    let user_email_cfg = format!("user.email={author_email}");
    let merge_res = run(
        env,
        checkout,
        &[
            "-c",
            &user_name_cfg,
            "-c",
            &user_email_cfg,
            "merge",
            "--no-ff",
            "-m",
            &msg,
            branch,
        ],
        &[],
        120,
    )
    .await;
    if let Err(e) = merge_res {
        let unmerged = run(env, checkout, &["diff", "--name-only", "--diff-filter=U"], &[], 10).await?;
        let merge_head = rev_parse(env, checkout, "MERGE_HEAD").await.ok();
        if !unmerged.trim().is_empty() && merge_head.as_deref() == Some(source.as_str()) {
            run(env, checkout, &["merge", "--abort"], &[], 30).await
                .map_err(|abort| format!("CONFLICT; automatic abort failed: {abort}; original: {e}"))?;
            return Err(format!("CONFLICT: {e}"));
        }
        return Err(format!("git merge failed: {e}"));
    }
    Ok(())
}

/// `git diff <from_ref>..<to_ref>` as a unified patch string (F11.f export).
pub async fn diff_range(
    env: &ShellEnv,
    dir: &Path,
    from_ref: &str,
    to_ref: &str,
) -> Result<String, String> {
    let range = format!("{from_ref}..{to_ref}");
    run(env, dir, &["diff", &range], &[], 120)
        .await
        .map_err(|e| format!("git diff failed: {e}"))
}

/// Write `git diff <from_ref>..<to_ref>` to `out_path` (creates parent dirs).
pub async fn export_patch_file(
    env: &ShellEnv,
    dir: &Path,
    from_ref: &str,
    to_ref: &str,
    out_path: &Path,
) -> Result<(), String> {
    export_patch_file_retry(env, dir, from_ref, to_ref, out_path, false).await
}

pub async fn export_patch_file_retry(env: &ShellEnv, dir: &Path, from_ref: &str, to_ref: &str, out_path: &Path, allow_identical: bool) -> Result<(), String> {
    use std::io::Write;
    let resolve = |reference: &str| format!("{reference}^{{commit}}");
    let from = run(env, dir, &["rev-parse", "--verify", "--end-of-options", &resolve(from_ref)], &[], 10).await?;
    let to = run(env, dir, &["rev-parse", "--verify", "--end-of-options", &resolve(to_ref)], &[], 10).await?;
    let patch = run_bytes(env, dir, &["diff", "--binary", "--full-index", "--no-ext-diff", "--no-textconv", from.trim(), to.trim(), "--"], &[], 120).await?;
    if allow_identical && out_path.exists() {
        let existing = std::fs::read(out_path).map_err(|e| e.to_string())?;
        return if existing == patch { Ok(()) } else { Err("Existing delivery artifact differs; refusing overwrite".into()) };
    }
    let parent = out_path.parent().ok_or("Patch path has no parent")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    file.write_all(&patch).map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist_noclobber(out_path).map_err(|e| e.to_string())?;
    Ok(())
}

// ── Composer git bar: local branches + working-tree file list / diffs ─────────

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitBranchInfo {
    pub name: String,
    pub current: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatusFile {
    pub path: String,
    /// `added` | `modified` | `deleted` | `untracked` | `conflicted` | `renamed`
    pub change: String,
    pub insertions: u32,
    pub deletions: u32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitFileDiff {
    pub path: String,
    pub change: String,
    /// Unified diff; `None` when binary / oversized / unavailable.
    pub diff: Option<String>,
    pub insertions: u32,
    pub deletions: u32,
}

const DIFF_TEXT_MAX: usize = 1024 * 1024;

fn valid_branch_name(name: &str) -> bool {
    let name = name.trim();
    if name.is_empty() || name.len() > 200 {
        return false;
    }
    if name.starts_with('-') || name.ends_with('/') || name.contains("..") || name.contains('\\') {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '.'))
}

/// Local branches only (`refs/heads/*`).
pub async fn list_local_branches(env: &ShellEnv, dir: &Path) -> Result<Vec<GitBranchInfo>, String> {
    // One line per branch: `name\t*` (current) or `name\t` (other).
    let out = run(
        env,
        dir,
        &[
            "for-each-ref",
            "--format=%(refname:short)%09%(HEAD)",
            "refs/heads/",
        ],
        &[],
        15,
    )
    .await
    .map_err(|e| format!("git for-each-ref failed: {e}"))?;
    let mut branches = Vec::new();
    for line in out.lines() {
        let (name, head) = line.split_once('\t').unwrap_or((line, ""));
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        branches.push(GitBranchInfo {
            name: name.to_string(),
            current: head.trim() == "*",
        });
    }
    branches.sort_by(|a, b| match (b.current, a.current) {
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        _ => a.name.cmp(&b.name),
    });
    Ok(branches)
}

/// Switch to an existing local branch. Refuses when the working tree is dirty (`Err("DIRTY")`).
pub async fn checkout_branch(env: &ShellEnv, dir: &Path, branch: &str) -> Result<(), String> {
    let branch = branch.trim();
    if !valid_branch_name(branch) {
        return Err("Invalid branch name".into());
    }
    if !is_working_tree_clean(env, dir).await? {
        return Err("DIRTY".into());
    }
    run(env, dir, &["checkout", branch], &[], 60)
        .await
        .map_err(|e| format!("git checkout failed: {e}"))?;
    Ok(())
}

/// Create and switch to a new local branch. Refuses when dirty (`Err("DIRTY")`).
pub async fn create_branch(env: &ShellEnv, dir: &Path, branch: &str) -> Result<(), String> {
    let branch = branch.trim();
    if !valid_branch_name(branch) {
        return Err("Invalid branch name".into());
    }
    if !is_working_tree_clean(env, dir).await? {
        return Err("DIRTY".into());
    }
    run(env, dir, &["checkout", "-b", branch], &[], 60)
        .await
        .map_err(|e| format!("git checkout -b failed: {e}"))?;
    Ok(())
}

fn classify_xy(xy: &str) -> &'static str {
    let x = xy.chars().next().unwrap_or(' ');
    let y = xy.chars().nth(1).unwrap_or(' ');
    if x == 'U' || y == 'U' || (x == 'A' && y == 'A') || (x == 'D' && y == 'D') {
        return "conflicted";
    }
    if xy.starts_with('?') {
        return "untracked";
    }
    if x == 'A' || y == 'A' {
        return "added";
    }
    if x == 'D' || y == 'D' {
        return "deleted";
    }
    if x == 'R' || y == 'R' {
        return "renamed";
    }
    "modified"
}

fn parse_numstat_line(line: &str) -> Option<(String, u32, u32)> {
    let mut parts = line.splitn(3, '\t');
    let ins = parts.next()?;
    let del = parts.next()?;
    let path = parts.next()?.to_string();
    if ins == "-" || del == "-" {
        return Some((path, 0, 0));
    }
    Some((path, ins.parse().unwrap_or(0), del.parse().unwrap_or(0)))
}

fn count_file_lines(dir: &Path, rel: &str) -> u32 {
    let path = dir.join(rel);
    let Ok(bytes) = std::fs::read(&path) else {
        return 0;
    };
    if bytes.len() > DIFF_TEXT_MAX || bytes.iter().take(8000).any(|b| *b == 0) {
        return 0;
    }
    bytes.iter().filter(|b| **b == b'\n').count() as u32
        + if bytes.last().is_some_and(|b| *b != b'\n') {
            1
        } else {
            0
        }
}

/// Working-tree status entries with per-file +/- line counts (vs HEAD when tracked).
pub async fn status_files(env: &ShellEnv, dir: &Path) -> Result<Vec<GitStatusFile>, String> {
    let (status, numstat) = tokio::join!(
        run(
            env,
            dir,
            &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
            &[],
            20,
        ),
        run(env, dir, &["diff", "--numstat", "HEAD"], &[], 30),
    );
    let status = status.map_err(|e| format!("git status failed: {e}"))?;
    let mut counts: std::collections::HashMap<String, (u32, u32)> = std::collections::HashMap::new();
    if let Ok(ns) = numstat {
        for line in ns.lines() {
            if let Some((path, ins, del)) = parse_numstat_line(line) {
                counts.insert(path, (ins, del));
            }
        }
    }

    let mut files = Vec::new();
    let mut it = status.split('\0');
    while let Some(entry) = it.next() {
        if entry.len() < 3 {
            continue;
        }
        let xy = &entry[..2];
        let rest = entry[3..].trim_start();
        if rest.is_empty() {
            continue;
        }
        let change = classify_xy(xy).to_string();
        // Rename/copy (-z): this field is the old path; the next NUL field is the new path.
        let path = if xy.starts_with('R') || xy.starts_with('C') {
            it.next().unwrap_or(rest).to_string()
        } else {
            rest.to_string()
        };
        let (mut insertions, deletions) = counts.get(&path).copied().unwrap_or((0, 0));
        if change == "untracked" && insertions == 0 && deletions == 0 {
            insertions = count_file_lines(dir, &path);
        }
        files.push(GitStatusFile {
            path,
            change,
            insertions,
            deletions,
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

fn preview_change_kind(change: &str) -> String {
    match change {
        "added" | "untracked" => "added".into(),
        "deleted" => "deleted".into(),
        _ => "modified".into(),
    }
}

fn summarize_unified_diff(diff: &str) -> (u32, u32) {
    let (mut ins, mut del) = (0u32, 0u32);
    for line in diff.lines() {
        if line.starts_with("+++") || line.starts_with("---") || line.starts_with('\\') {
            continue;
        }
        if let Some(rest) = line.strip_prefix('+') {
            if !rest.starts_with("++") {
                ins += 1;
            }
        } else if let Some(rest) = line.strip_prefix('-') {
            if !rest.starts_with("--") {
                del += 1;
            }
        }
    }
    (ins, del)
}

/// Unified diff for one working-tree path (vs HEAD, or full-file for untracked).
pub async fn file_diff(env: &ShellEnv, dir: &Path, path: &str) -> Result<GitFileDiff, String> {
    if path.is_empty() || path.contains('\0') || Path::new(path).is_absolute() {
        return Err("Invalid path".into());
    }
    if path.split(['/', '\\']).any(|p| p == "..") {
        return Err("Invalid path".into());
    }

    let files = status_files(env, dir).await?;
    let meta = files.iter().find(|f| f.path == path);
    let change = meta
        .map(|f| f.change.clone())
        .unwrap_or_else(|| "modified".into());

    let diff_text = if change == "untracked" {
        let abs = dir.join(path);
        run_diff(
            env,
            dir,
            &[
                "diff",
                "--no-index",
                "--",
                "/dev/null",
                &abs.to_string_lossy(),
            ],
            30,
        )
        .await
        .unwrap_or_default()
    } else {
        run_diff(env, dir, &["diff", "HEAD", "--", path], 30)
            .await
            .map_err(|e| format!("git diff failed: {e}"))?
    };

    let diff = if diff_text.is_empty() || diff_text.len() > DIFF_TEXT_MAX || diff_text.contains('\0')
    {
        None
    } else {
        Some(diff_text)
    };
    let (insertions, deletions) = match (&diff, meta) {
        (Some(d), _) => summarize_unified_diff(d),
        (None, Some(m)) => (m.insertions, m.deletions),
        (None, None) => (0, 0),
    };

    Ok(GitFileDiff {
        path: path.to_string(),
        change: preview_change_kind(&change),
        diff,
        insertions,
        deletions,
    })
}

/// `git worktree prune` in the main checkout (F11.g).
pub async fn prune_worktrees(env: &ShellEnv, dir: &Path) -> Result<(), String> {
    run(env, dir, &["worktree", "prune"], &[], 60)
        .await
        .map_err(|e| format!("git worktree prune failed: {e}"))?;
    Ok(())
}

/// Delete a local branch (`git branch -D`). Ignores missing branch.
pub async fn delete_branch(env: &ShellEnv, dir: &Path, branch: &str) -> Result<(), String> {
    match run(env, dir, &["branch", "-D", branch], &[], 20).await {
        Ok(_) => Ok(()),
        Err(e) if e.contains("not found") || e.contains("unknown") => Ok(()),
        Err(e) => Err(format!("git branch -D failed: {e}")),
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrphanWorktreeEntry {
    pub path: String,
    pub branch: Option<String>,
    pub dirty: bool,
}

/// Scan `{app_data}/worktrees/` for checkout dirs (F11.g orphan panel).
/// Each immediate child of a project hash folder that contains `.git` is listed.
pub async fn list_orphan_worktrees(
    env: &ShellEnv,
    app_data: &Path,
) -> Result<Vec<OrphanWorktreeEntry>, String> {
    let base = app_data.join("worktrees");
    if !base.is_dir() {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    let projects = std::fs::read_dir(&base).map_err(|e| e.to_string())?;
    for proj in projects.flatten() {
        let proj_path = proj.path();
        if !proj_path.is_dir() {
            continue;
        }
        let entries = match std::fs::read_dir(&proj_path) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for ent in entries.flatten() {
            let path = ent.path();
            if !path.is_dir() {
                continue;
            }
            // Linked worktrees have `.git` as a file; normal repos have a dir.
            let git_marker = path.join(".git");
            if !git_marker.exists() {
                continue;
            }
            let dirty = match is_working_tree_clean(env, &path).await {
                Ok(clean) => !clean,
                Err(_) => false,
            };
            let branch = run(env, &path, &["rev-parse", "--abbrev-ref", "HEAD"], &[], 10)
                .await
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty() && s != "HEAD");
            out.push(OrphanWorktreeEntry {
                path: path.to_string_lossy().into_owned(),
                branch,
                dirty,
            });
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// F11.g startup: `git worktree prune` in each unique main checkout reachable from
/// `{app_data}/worktrees/` entries. Does not delete directories — only prunes stale
/// git registrations. Remaining dirs are listed by `list_orphan_worktrees`.
pub async fn prune_registered_worktrees(env: &ShellEnv, app_data: &Path) -> Result<usize, String> {
    let entries = list_orphan_worktrees(env, app_data).await?;
    let mut mains = std::collections::HashSet::new();
    for ent in &entries {
        let path = PathBuf::from(&ent.path);
        if let Ok(common) = run(
            env,
            &path,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            &[],
            10,
        )
        .await
        {
            if let Some(main) = PathBuf::from(common.trim()).parent().map(Path::to_path_buf) {
                mains.insert(main);
            }
        }
    }
    let mut pruned = 0usize;
    for main in mains {
        if prune_worktrees(env, &main).await.is_ok() {
            pruned += 1;
        }
    }
    Ok(pruned)
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
/// Saves verified contents and index under the private Git directory; errors are fatal.
pub async fn record_turn_snapshot(
    env: &ShellEnv,
    dir: &Path,
    turn_id: &str,
) -> Result<GitSnapshot, String> {
    let accepted = scope::snapshot(env, dir).await?;
    rollback::capture(env, dir, turn_id, &accepted).await?;
    let status_out = run(env, dir, &["status", "--porcelain=v1", "-z", "--untracked-files=all"], &[], 15).await?;
    Ok(GitSnapshot {
        head: accepted.head,
        status: status_out.split('\0').filter(|s| !s.is_empty()).map(str::to_owned).collect(),
        created_at: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e| e.to_string())?.as_secs(),
    })
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_name_rules() {
        assert!(valid_branch_name("main"));
        assert!(valid_branch_name("feature/foo-1"));
        assert!(!valid_branch_name(""));
        assert!(!valid_branch_name("-bad"));
        assert!(!valid_branch_name("a..b"));
        assert!(!valid_branch_name("has space"));
    }

    #[test]
    fn classify_status_xy() {
        assert_eq!(classify_xy("??"), "untracked");
        assert_eq!(classify_xy("A "), "added");
        assert_eq!(classify_xy(" D"), "deleted");
        assert_eq!(classify_xy("M "), "modified");
        assert_eq!(classify_xy("UU"), "conflicted");
        assert_eq!(classify_xy("R "), "renamed");
    }

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
        assert!(create_worktree(&env, &repo.join("app"), &base, "s1", None)
            .await
            .is_err());
        g(&["add", "."]);
        g(&["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", "init"]);
        let wt = create_worktree(&env, &repo.join("app"), &base, "s1", None)
            .await
            .unwrap();
        assert_eq!(wt.branch, "tokage/s1");
        assert!(Path::new(&wt.cwd).join("a.txt").is_file());
        let i = info(&env, Path::new(&wt.cwd)).await;
        assert!(i.linked_worktree);
        assert_eq!(i.branch.as_deref(), Some("tokage/s1"));

        let root = Path::new(&wt.root);
        std::fs::write(root.join("app/b.txt"), "b").unwrap();
        assert_eq!(
            remove_worktree(&env, root, "tokage/s1", false, false).await,
            Err("DIRTY".into())
        );
        remove_worktree(&env, root, "tokage/s1", true, false)
            .await
            .unwrap();
        assert!(!root.exists());
        assert!(g(&["branch", "--list", "tokage/s1"]).stdout.is_empty());
        assert!(remove_worktree(&env, root, "main", true, false)
            .await
            .is_err());
        std::fs::remove_dir_all(&base).ok();
    }

    /// F11.a: integration → T1 commit → merge → T2 sees T1; conflict; path guard.
    #[tokio::test]
    async fn f11a_integration_merge_and_conflict() {
        let env = ShellEnv {
            vars: std::env::vars().collect(),
        };
        let base = std::env::temp_dir().join(format!(
            "tokage-f11-{}",
            std::process::id()
        ));
        let repo = base.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let g = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .output()
                .unwrap()
        };
        g(&["init", "-q"]);
        g(&["checkout", "-b", "main"]);
        std::fs::write(repo.join("shared.txt"), "base\n").unwrap();
        g(&["add", "."]);
        g(&["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", "init"]);

        // ① Integration worktree from HEAD
        let integ = create_worktree(&env, &repo, &base, "abc12345-integration", None)
            .await
            .unwrap();
        assert_eq!(integ.branch, "tokage/abc12345-integration");
        let integ_root = PathBuf::from(&integ.root);

        // ② T1 from integration branch, write + commit
        let t1 = create_worktree(
            &env,
            &repo,
            &base,
            "abc12345-T1-1",
            Some(&integ.branch),
        )
        .await
        .unwrap();
        std::fs::write(Path::new(&t1.root).join("t1.txt"), "from-t1\n").unwrap();
        assert!(commit_worktree(&env, Path::new(&t1.root), "T1 work")
            .await
            .unwrap());

        // ③ Merge T1 into integration
        run(
            &env,
            &integ_root,
            &[
                "merge",
                "--no-ff",
                "-m",
                "Merge T1",
                &t1.branch,
            ],
            &[],
            60,
        )
        .await
        .unwrap();
        assert!(integ_root.join("t1.txt").is_file());

        // ④ T2 from integration sees T1 file
        let t2 = create_worktree(
            &env,
            &repo,
            &base,
            "abc12345-T2-1",
            Some(&integ.branch),
        )
        .await
        .unwrap();
        assert!(Path::new(&t2.root).join("t1.txt").is_file());

        // ⑤ Conflict: T2 and parallel branch both edit shared.txt
        std::fs::write(Path::new(&t2.root).join("shared.txt"), "t2-edit\n").unwrap();
        assert!(commit_worktree(&env, Path::new(&t2.root), "T2 edit")
            .await
            .unwrap());

        // Parallel conflict branch from pre-T2 integration tip… use another worktree from
        // integration before merging T2, editing the same file.
        // First rewind: create conflictor from integration HEAD (already has T1).
        let conflictor = create_worktree(
            &env,
            &repo,
            &base,
            "abc12345-C-1",
            Some(&integ.branch),
        )
        .await
        .unwrap();
        std::fs::write(
            Path::new(&conflictor.root).join("shared.txt"),
            "conflictor-edit\n",
        )
        .unwrap();
        assert!(commit_worktree(&env, Path::new(&conflictor.root), "conflictor")
            .await
            .unwrap());
        // Merge conflictor into integration first
        run(
            &env,
            &integ_root,
            &["merge", "--no-ff", "-m", "Merge C", &conflictor.branch],
            &[],
            60,
        )
        .await
        .unwrap();
        // Now merge T2 → conflict
        let merge_fail = run(
            &env,
            &integ_root,
            &["merge", "--no-ff", "-m", "Merge T2", &t2.branch],
            &[],
            60,
        )
        .await;
        assert!(merge_fail.is_err());
        let conflicts = run(
            &env,
            &integ_root,
            &["diff", "--name-only", "--diff-filter=U"],
            &[],
            10,
        )
        .await
        .unwrap();
        assert!(conflicts.contains("shared.txt"));
        run(&env, &integ_root, &["merge", "--abort"], &[], 10)
            .await
            .unwrap();
        // Main checkout clean
        let main_status = run(&env, &repo, &["status", "--porcelain"], &[], 10)
            .await
            .unwrap();
        assert!(main_status.trim().is_empty());

        // ⑥ Path guard: main checkout is not under worktrees/
        assert!(!is_under_worktrees_dir(&repo, &base));
        assert!(is_under_worktrees_dir(&integ_root, &base));

        // keep_branch: remove worktree dir but leave branch
        remove_worktree(&env, Path::new(&t1.root), &t1.branch, true, true)
            .await
            .unwrap();
        assert!(!Path::new(&t1.root).exists());
        let listed = g(&["branch", "--list", &t1.branch]);
        assert!(!listed.stdout.is_empty());

        std::fs::remove_dir_all(&base).ok();
    }

    #[tokio::test]
    async fn m1f_delivery_requires_exact_accepted_source_content_and_commit() {
        let env = ShellEnv { vars: std::env::vars().collect() };
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        for args in [vec!["init", "-q"], vec!["config", "user.email", "t@t"], vec!["config", "user.name", "t"]] {
            run(&env, &repo, &args, &[], 10).await.unwrap();
        }
        std::fs::write(repo.join("a"), "base").unwrap();
        run(&env, &repo, &["add", "."], &[], 10).await.unwrap();
        run(&env, &repo, &["commit", "-qm", "base"], &[], 10).await.unwrap();
        let base = rev_parse(&env, &repo, "HEAD").await.unwrap();
        let source = create_worktree(&env, &repo, temp.path(), "m1f-integration", None).await.unwrap();
        let root = Path::new(&source.root);
        std::fs::write(root.join("feature"), "accepted").unwrap();
        commit_worktree(&env, root, "feature").await.unwrap();
        let accepted = scope::snapshot(&env, root).await.unwrap();
        std::fs::write(root.join("feature"), "later human edit").unwrap();
        assert!(merge_accepted_into_checkout(&env, &repo, &source.branch, &accepted, Some(&checkout_identity(&env, &repo).await.unwrap())).await.unwrap_err().contains("stale"));
        assert_eq!(rev_parse(&env, &repo, "HEAD").await.unwrap(), base);
        assert_eq!(std::fs::read_to_string(root.join("feature")).unwrap(), "later human edit");
        commit_worktree(&env, root, "later commit").await.unwrap();
        assert!(merge_accepted_into_checkout(&env, &repo, &source.branch, &accepted, Some(&checkout_identity(&env, &repo).await.unwrap())).await.is_err());
        let fresh = scope::snapshot(&env, root).await.unwrap();
        // A branch pointing elsewhere cannot use evidence for the accepted checkout.
        run(&env, &repo, &["branch", "other-source", &base], &[], 10).await.unwrap();
        assert!(merge_accepted_into_checkout(&env, &repo, "other-source", &fresh, Some(&checkout_identity(&env, &repo).await.unwrap())).await.unwrap_err().contains("branch changed"));
        merge_accepted_into_checkout(&env, &repo, &source.branch, &fresh, Some(&checkout_identity(&env, &repo).await.unwrap())).await.unwrap();
        assert_eq!(std::fs::read_to_string(repo.join("feature")).unwrap(), "later human edit");
        assert_eq!(run(&env, &repo, &["rev-parse", "HEAD^2"], &[], 10).await.unwrap().trim(), fresh.head);
    }

    #[tokio::test]
    async fn commits_and_merges_always_use_original_author_and_optional_tokage_co_author() {
        let env = ShellEnv {
            vars: std::env::vars().collect(),
        };
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.name", "Alice Original"],
            vec!["config", "user.email", "alice@example.com"],
        ] {
            run(&env, &repo, &args, &[], 10).await.unwrap();
        }
        std::fs::write(repo.join("a.txt"), "base\n").unwrap();
        run(&env, &repo, &["add", "."], &[], 10).await.unwrap();
        run(&env, &repo, &["commit", "-qm", "base"], &[], 10).await.unwrap();

        let wt = create_worktree(&env, &repo, temp.path(), "coauthor-test", None)
            .await
            .unwrap();
        let wt_root = Path::new(&wt.root);

        // 1. Default (co_author = false): author is Alice, no Co-authored-by trailer.
        std::fs::write(wt_root.join("step1.txt"), "1\n").unwrap();
        let snap1 = scope::snapshot(&env, wt_root).await.unwrap();
        assert!(
            scope::commit_verified_with_options(&env, wt_root, "Task 1", &snap1, false)
                .await
                .unwrap()
        );
        let log1 = run(&env, wt_root, &["log", "-1", "--format=%an|%ae|%cn|%ce|%B"], &[], 10)
            .await
            .unwrap();
        assert!(
            log1.starts_with("Alice Original|alice@example.com|Alice Original|alice@example.com|Task 1"),
            "unexpected log1: {log1}"
        );
        assert!(!log1.contains(TOKAGE_CO_AUTHOR_TRAILER));

        // 2. Enabled (co_author = true): author is still Alice, message has Co-authored-by: Tokage.
        std::fs::write(wt_root.join("step2.txt"), "2\n").unwrap();
        let snap2 = scope::snapshot(&env, wt_root).await.unwrap();
        assert!(
            scope::commit_verified_with_options(&env, wt_root, "Task 2", &snap2, true)
                .await
                .unwrap()
        );
        let log2 = run(&env, wt_root, &["log", "-1", "--format=%an|%ae|%cn|%ce|%B"], &[], 10)
            .await
            .unwrap();
        assert!(
            log2.starts_with("Alice Original|alice@example.com|Alice Original|alice@example.com|Task 2"),
            "unexpected log2: {log2}"
        );
        assert!(log2.contains(TOKAGE_CO_AUTHOR_TRAILER));

        let wt_info = info(&env, wt_root).await;
        assert_eq!(wt_info.head_author_name.as_deref(), Some("Alice Original"));
        assert!(wt_info.head_co_authored_by_tokage);

        // 3. Even if local config was accidentally set to Tokage or Tokage m3a / *.invalid, fallback recovers original author.
        for (bad_name, bad_email) in [
            ("Tokage", "tokage@localhost"),
            ("Tokage m3a", "m3a@tokage.invalid"),
            ("Tokage evidence", "evidence@tokage.invalid"),
            ("Tokage Debug", "debug@tokage.invalid"),
        ] {
            run(&env, wt_root, &["config", "user.name", bad_name], &[], 10).await.unwrap();
            run(&env, wt_root, &["config", "user.email", bad_email], &[], 10).await.unwrap();
            let (resolved_name, resolved_email) = resolve_author_identity(&env, wt_root).await;
            assert!(
                !resolved_name.to_ascii_lowercase().starts_with("tokage"),
                "unexpected tokage name: {resolved_name}"
            );
            assert!(
                !resolved_email.to_ascii_lowercase().ends_with("@tokage.invalid")
                    && !resolved_email.eq_ignore_ascii_case("tokage@localhost"),
                "unexpected tokage email: {resolved_email}"
            );
        }
    }

    /// F11.f: dirty main checkout refuses delivery merge; clean merge succeeds; conflict aborts.
    #[tokio::test]
    async fn f11f_delivery_merge_dirty_and_export() {
        let env = ShellEnv {
            vars: std::env::vars().collect(),
        };
        let base = std::env::temp_dir().join(format!(
            "tokage-f11f-{}",
            std::process::id()
        ));
        let repo = base.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let g = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .output()
                .unwrap()
        };
        g(&["init", "-q"]);
        g(&["checkout", "-b", "main"]);
        std::fs::write(repo.join("a.txt"), "base\n").unwrap();
        g(&["add", "."]);
        g(&["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", "init"]);
        let start = rev_parse(&env, &repo, "HEAD").await.unwrap();

        let integ = create_worktree(&env, &repo, &base, "deliv-integration", None)
            .await
            .unwrap();
        std::fs::write(Path::new(&integ.root).join("feat.txt"), "feat\n").unwrap();
        assert!(commit_worktree(&env, Path::new(&integ.root), "feat")
            .await
            .unwrap());

        // Dirty main → DIRTY
        std::fs::write(repo.join("dirty.txt"), "x\n").unwrap();
        assert_eq!(
            merge_branch_into_checkout(&env, &repo, &integ.branch).await,
            Err("DIRTY".into())
        );
        std::fs::remove_file(repo.join("dirty.txt")).unwrap();

        // Export patch
        let patch_path = base.join("out.patch");
        export_patch_file(&env, &repo, &start, &integ.branch, &patch_path)
            .await
            .unwrap();
        let patch = std::fs::read_to_string(&patch_path).unwrap();
        assert!(patch.contains("feat.txt"));

        // Clean merge
        merge_branch_into_checkout(&env, &repo, &integ.branch)
            .await
            .unwrap();
        assert!(repo.join("feat.txt").is_file());

        std::fs::remove_dir_all(&base).ok();
    }
}

pub async fn contains_commit(env: &ShellEnv, root: &Path, branch: &str, source: &str) -> Result<bool, String> {
    let source = rev_parse(env, root, source).await?;
    let target = rev_parse(env, root, branch).await?;
    let output = git_output(env, root, &["merge-base", "--is-ancestor", &source, &target], &[], 10).await?;
    match output.status.code() { Some(0) => Ok(true), Some(1) => Ok(false), _ => Err("Git ancestry could not be verified".into()) }
}
