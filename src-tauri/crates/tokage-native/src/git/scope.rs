//! Read-only execution baselines. Fail closed on incomplete or unstable Git/file reads.
use crate::env::ShellEnv;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Read, path::Path};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ScopeFile {
    pub disk: String,
    pub index: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ScopeSnapshot {
    pub root: String,
    pub workdir: String,
    pub prefix: String,
    pub head: String,
    pub clean: bool,
    pub files: BTreeMap<String, ScopeFile>,
}

// Unlike general UI Git reads, warnings about unreadable paths are fatal here.
async fn strict_read(
    env: &ShellEnv,
    dir: &Path,
    args: &[&str],
    _: &[(&str, &str)],
    secs: u64,
) -> Result<String, String> {
    let command = tokio::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env_clear()
        .envs(&env.vars)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(std::time::Duration::from_secs(secs), command)
        .await
        .map_err(|_| "Scope Git read timed out".to_string())?
        .map_err(|e| e.to_string())?;
    if !output.status.success() || !output.stderr.is_empty() {
        return Err(format!(
            "Scope Git read failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    String::from_utf8(output.stdout).map_err(|_| "Non-UTF8 Git output cannot be verified".into())
}

pub async fn snapshot(env: &ShellEnv, workdir: &Path) -> Result<ScopeSnapshot, String> {
    let root = strict_read(env, workdir, &["rev-parse", "--show-toplevel"], &[], 20).await?;
    let root = Path::new(root.trim_end())
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let workdir = workdir.canonicalize().map_err(|e| e.to_string())?;
    let prefix = workdir
        .strip_prefix(&root)
        .map_err(|_| "Scope directory outside repository")?
        .to_str()
        .ok_or("Non-UTF8 scope directory")?
        .replace('\\', "/");
    let head = strict_read(env, &root, &["rev-parse", "--verify", "HEAD"], &[], 20)
        .await?
        .trim()
        .to_string();
    let status = strict_read(
        env,
        &root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        &[],
        30,
    )
    .await?;
    let flags = strict_read(env, &root, &["ls-files", "-v", "-z"], &[], 30).await?;
    if flags
        .split('\0')
        .filter(|s| !s.is_empty())
        .any(|s| !s.starts_with("H "))
    {
        return Err("Unmerged, sparse or assume-unchanged index cannot be verified".into());
    }
    let index = strict_read(env, &root, &["ls-files", "--stage", "-z"], &[], 30).await?;
    let untracked = strict_read(
        env,
        &root,
        &["ls-files", "--others", "--exclude-standard", "-z"],
        &[],
        30,
    )
    .await?;
    if index.contains('\u{fffd}') || untracked.contains('\u{fffd}') {
        return Err("Non-UTF8 Git path cannot be verified".into());
    }
    let root_text = root.to_str().ok_or("Non-UTF8 repository")?.to_string();
    let workdir_text = workdir
        .to_str()
        .ok_or("Non-UTF8 working directory")?
        .to_string();
    let untracked_check = untracked.clone();
    let read_root = root.clone();
    let read_index = index.clone();
    let files =
        tokio::task::spawn_blocking(move || -> Result<BTreeMap<String, ScopeFile>, String> {
            let mut entries = BTreeMap::new();
            for entry in read_index.split('\0').filter(|s| !s.is_empty()) {
                let (info, path) = entry.split_once('\t').ok_or("Malformed Git index")?;
                let fields: Vec<_> = info.split(' ').collect();
                if fields.len() != 3 || fields[2] != "0" {
                    return Err("Unmerged Git index cannot be verified".into());
                }
                if fields[0] == "160000" {
                    return Err("Submodule scope requires separate verification".into());
                }
                entries.insert(path.to_string(), info.to_string());
            }
            for path in untracked.split('\0').filter(|s| !s.is_empty()) {
                entries.entry(path.into()).or_default();
            }
            if entries.len() > 50_000 {
                return Err("Scope snapshot exceeds 50000 files".into());
            }
            let mut total = 0u64;
            let mut result = BTreeMap::new();
            for (path, index) in entries {
                if Path::new(&path).is_absolute()
                    || Path::new(&path)
                        .components()
                        .any(|c| matches!(c, std::path::Component::ParentDir))
                {
                    return Err("Unsafe Git path".into());
                }
                let file = read_root.join(&path);
                let disk = match std::fs::symlink_metadata(&file) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => "missing".into(),
                    Err(e) => return Err(format!("Cannot inspect {path}: {e}")),
                    Ok(meta) => {
                        if let Some(parent) = file.parent() {
                            if !parent
                                .canonicalize()
                                .map_err(|e| e.to_string())?
                                .starts_with(&read_root)
                            {
                                return Err(format!("Parent escapes repository: {path}"));
                            }
                        }
                        if meta.file_type().is_symlink() {
                            let target = std::fs::read_link(&file).map_err(|e| e.to_string())?;
                            let resolved = file
                                .canonicalize()
                                .map_err(|e| format!("Cannot verify symlink {path}: {e}"))?;
                            if !resolved.starts_with(&read_root) {
                                return Err(format!("Symlink escapes repository: {path}"));
                            }
                            format!("link:{}", target.to_str().ok_or("Non-UTF8 symlink")?)
                        } else if meta.is_file() {
                            let mut input = std::fs::File::open(&file)
                                .map_err(|e| format!("Cannot read {path}: {e}"))?;
                            let mut hash = Sha256::new();
                            let mut buf = [0u8; 65536];
                            loop {
                                let n = input
                                    .read(&mut buf)
                                    .map_err(|e| format!("Cannot read {path}: {e}"))?;
                                if n == 0 {
                                    break;
                                }
                                total += n as u64;
                                if total > 512 * 1024 * 1024 {
                                    return Err("Scope snapshot exceeds 512 MiB".into());
                                }
                                hash.update(&buf[..n]);
                            }
                            #[cfg(unix)]
                            let mode = {
                                use std::os::unix::fs::PermissionsExt;
                                meta.permissions().mode() & 0o111
                            };
                            #[cfg(not(unix))]
                            let mode = meta.permissions().readonly() as u32;
                            format!("file:{mode}:{:x}", hash.finalize())
                        } else {
                            return Err(format!("Unsupported file type: {path}"));
                        }
                    }
                };
                result.insert(path, ScopeFile { disk, index });
            }
            Ok(result)
        })
        .await
        .map_err(|e| e.to_string())??;
    let after_index = strict_read(env, &root, &["ls-files", "--stage", "-z"], &[], 30).await?;
    let after_head = strict_read(env, &root, &["rev-parse", "--verify", "HEAD"], &[], 20).await?;
    let after_untracked = strict_read(
        env,
        &root,
        &["ls-files", "--others", "--exclude-standard", "-z"],
        &[],
        30,
    )
    .await?;
    let after_status = strict_read(
        env,
        &root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        &[],
        30,
    )
    .await?;
    if after_untracked != untracked_check
        || after_status != status
        || after_index != index
        || after_head.trim() != head
    {
        return Err("Git changed during scope snapshot".into());
    }
    Ok(ScopeSnapshot {
        root: root_text,
        workdir: workdir_text,
        prefix,
        head,
        clean: status.is_empty(),
        files,
    })
}

/// Git clean filters must not silently change the contents that passed the gates.
async fn verify_staged_contents(
    env: &ShellEnv,
    root: &Path,
    expected: &ScopeSnapshot,
    staged: &ScopeSnapshot,
) -> Result<(), String> {
    let paths = strict_read(
        env,
        root,
        &[
            "diff",
            "--cached",
            "--name-only",
            "--no-renames",
            "-z",
            &expected.head,
        ],
        &[],
        30,
    )
    .await?;
    let mut total = 0u64;
    for path in paths.split('\0').filter(|p| !p.is_empty()) {
        let accepted = expected
            .files
            .get(path)
            .ok_or_else(|| format!("Unaccepted staged path: {path}"))?;
        let Some(file) = staged.files.get(path).filter(|f| !f.index.is_empty()) else {
            if accepted.disk != "missing" {
                return Err(format!("Unexpected staged deletion: {path}"));
            }
            continue;
        };
        if file.disk == "missing" {
            return Err(format!("Index contains an absent file: {path}"));
        }
        let fields: Vec<_> = file.index.split(' ').collect();
        if fields.len() != 3 {
            return Err("Malformed staged entry".into());
        }
        let size = strict_read(env, root, &["cat-file", "-s", fields[1]], &[], 20)
            .await?
            .trim()
            .parse::<u64>()
            .map_err(|e| e.to_string())?;
        total = total
            .checked_add(size)
            .ok_or("Staged content size overflow")?;
        if total > 512 * 1024 * 1024 {
            return Err("Staged content exceeds 512 MiB".into());
        }
        let command = tokio::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["cat-file", "blob", fields[1]])
            .env_clear()
            .envs(&env.vars)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_COMMON_DIR")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .output();
        let output = tokio::time::timeout(std::time::Duration::from_secs(30), command)
            .await
            .map_err(|_| "Staged blob read timed out")?
            .map_err(|e| e.to_string())?;
        if !output.status.success() || !output.stderr.is_empty() {
            return Err("Cannot verify staged blob".into());
        }
        let matches = if let Some(link) = accepted.disk.strip_prefix("link:") {
            fields[0] == "120000" && output.stdout == link.as_bytes()
        } else if let Some(file) = accepted.disk.strip_prefix("file:") {
            let (mode, hash) = file
                .split_once(':')
                .ok_or("Malformed accepted file fingerprint")?;
            let executable = mode.parse::<u32>().map_err(|e| e.to_string())? & 0o100 != 0;
            let expected_mode = if cfg!(unix) && executable {
                "100755"
            } else {
                "100644"
            };
            fields[0] == expected_mode && format!("{:x}", Sha256::digest(&output.stdout)) == hash
        } else {
            false
        };
        if !matches {
            return Err(format!(
                "Staging transformed accepted file contents or mode: {path}"
            ));
        }
    }
    Ok(())
}

/// The commit path consumes precisely the disk tree accepted after the gates.
pub async fn commit_verified(
    env: &ShellEnv,
    root: &Path,
    message: &str,
    expected: &ScopeSnapshot,
) -> Result<bool, String> {
    if root.canonicalize().map_err(|e| e.to_string())? != Path::new(&expected.root) {
        return Err("Scope repository mismatch".into());
    }
    let current = snapshot(env, Path::new(&expected.workdir)).await?;
    let disk_tree = |s: &ScopeSnapshot| {
        s.files
            .iter()
            .filter(|(_, f)| f.disk != "missing")
            .map(|(p, f)| (p.clone(), f.disk.clone()))
            .collect::<BTreeMap<_, _>>()
    };
    if disk_tree(&current) != disk_tree(expected) {
        return Err("Files changed after scope acceptance".into());
    }
    if current.head != expected.head {
        // Reconcile a commit whose response/state save was lost. Only the exact
        // accepted tree, clean checkout and single original parent may be reused.
        let parents =
            super::run(env, root, &["show", "-s", "--format=%P", "HEAD"], &[], 10).await?;
        let subject =
            super::run(env, root, &["show", "-s", "--format=%B", "HEAD"], &[], 10).await?;
        if !current.clean || parents.trim() != expected.head || subject.trim() != message {
            return Err(
                "Cannot reconcile task commit with accepted scope and original parent".into(),
            );
        }
        verify_staged_contents(env, root, expected, &current).await?;
        return Ok(false);
    }
    if &current != expected {
        // An interrupted add may change the index, but never erase unrelated staged contents.
        verify_staged_contents(env, root, expected, &current).await?;
    }
    super::run(env, root, &["add", "-A"], &[], 30).await?;
    let staged = snapshot(env, Path::new(&expected.workdir)).await?;
    if staged.head != expected.head
        || staged.files.len() != expected.files.len()
        || staged
            .files
            .iter()
            .any(|(p, f)| expected.files.get(p).map(|old| &old.disk) != Some(&f.disk))
    {
        // Staging removes deleted entries, so compare the effective disk trees as well below.
        let disk_tree = |s: &ScopeSnapshot| {
            s.files
                .iter()
                .filter(|(_, f)| f.disk != "missing")
                .map(|(p, f)| (p.clone(), f.disk.clone()))
                .collect::<BTreeMap<_, _>>()
        };
        if staged.head != expected.head || disk_tree(&staged) != disk_tree(expected) {
            return Err("Files changed while staging accepted scope".into());
        }
    }
    verify_staged_contents(env, root, expected, &staged).await?;
    // Avoid running another add after the check.
    if super::run(env, root, &["diff", "--cached", "--name-only"], &[], 20)
        .await?
        .is_empty()
    {
        return Ok(false);
    }
    let accepted_tree = super::run(env, root, &["write-tree"], &[], 20).await?;
    super::run(
        env,
        root,
        &[
            "-c",
            "user.name=Tokage",
            "-c",
            "user.email=tokage@localhost",
            "commit",
            "-m",
            message,
        ],
        &[],
        60,
    )
    .await?;
    let committed_tree = super::run(env, root, &["rev-parse", "HEAD^{tree}"], &[], 20).await?;
    if committed_tree != accepted_tree {
        return Err("Commit hooks changed the accepted tree; refusing delivery".into());
    }
    let after = snapshot(env, Path::new(&expected.workdir)).await?;
    let disk_tree = |s: &ScopeSnapshot| {
        s.files
            .iter()
            .filter(|(_, f)| f.disk != "missing")
            .map(|(p, f)| (p.clone(), f.disk.clone()))
            .collect::<BTreeMap<_, _>>()
    };
    if disk_tree(&after) != disk_tree(expected) {
        return Err("Files changed during commit; refusing delivery".into());
    }
    Ok(true)
}

/// A synthetic merge-marker baseline is private bookkeeping, not a delivery.
/// Do not run project hooks on deliberately unresolved markers. Real task commits
/// and integration merges still run hooks and propagate their failures.
pub async fn commit_private_conflict_baseline(env: &ShellEnv, root: &Path) -> Result<(), String> {
    super::run(env, root, &["add", "-A"], &[], 30).await?;
    let accepted = snapshot(env, root).await?;
    let mut verification = accepted.clone();
    let deletions = super::run(
        env,
        root,
        &[
            "diff",
            "--cached",
            "--name-only",
            "--diff-filter=D",
            "-z",
            "HEAD",
        ],
        &[],
        10,
    )
    .await?;
    for path in deletions.split('\0').filter(|p| !p.is_empty()) {
        verification.files.insert(
            path.into(),
            ScopeFile {
                disk: "missing".into(),
                index: String::new(),
            },
        );
    }
    verify_staged_contents(env, root, &verification, &accepted).await?;
    let tree = super::run(env, root, &["write-tree"], &[], 10).await?;
    let hooks = tempfile::tempdir().map_err(|e| e.to_string())?;
    let hooks_config = format!("core.hooksPath={}", hooks.path().display());
    super::run(
        env,
        root,
        &[
            "-c",
            "user.name=Tokage",
            "-c",
            "user.email=tokage@localhost",
            "-c",
            &hooks_config,
            "-c",
            "commit.gpgSign=false",
            "commit",
            "-m",
            "Tokage private conflict-resolution baseline",
        ],
        &[],
        60,
    )
    .await?;
    if super::run(env, root, &["rev-parse", "HEAD^{tree}"], &[], 10).await? != tree {
        return Err("Private conflict baseline tree changed".into());
    }
    let after = snapshot(env, root).await?;
    if !after.clean || after.files != accepted.files {
        return Err("Private conflict baseline files changed during commit".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn env() -> ShellEnv {
        ShellEnv {
            vars: std::env::vars().collect(),
        }
    }
    async fn repo() -> (tempfile::TempDir, ShellEnv) {
        let dir = tempfile::tempdir().unwrap();
        let env = env();
        super::super::run(&env, dir.path(), &["init"], &[], 10)
            .await
            .unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/a"), "original").unwrap();
        std::fs::write(dir.path().join("src/b"), "delete me").unwrap();
        super::super::commit_worktree(&env, dir.path(), "initial")
            .await
            .unwrap();
        (dir, env)
    }
    fn changed(a: &ScopeSnapshot, b: &ScopeSnapshot) -> Vec<String> {
        let mut paths: std::collections::BTreeSet<_> =
            a.files.keys().chain(b.files.keys()).cloned().collect();
        paths.retain(|p| a.files.get(p) != b.files.get(p));
        paths.into_iter().collect()
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn m1d_private_baseline_handles_deletions_and_only_skips_private_hooks() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, env) = repo().await;
        let hook = dir.path().join(".git/hooks/pre-commit");
        std::fs::write(&hook, "#!/bin/sh\necho task-hook-refused >&2\nexit 5\n").unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(
            dir.path().join("src/a"),
            "<<<<<<< HEAD\nintegration\n=======\ntask\n>>>>>>> source\n",
        )
        .unwrap();
        std::fs::remove_file(dir.path().join("src/b")).unwrap();
        commit_private_conflict_baseline(&env, dir.path())
            .await
            .unwrap();
        assert!(snapshot(&env, dir.path()).await.unwrap().clean);
        std::fs::write(dir.path().join("src/a"), "resolved").unwrap();
        let expected = snapshot(&env, dir.path()).await.unwrap();
        assert!(
            commit_verified(&env, dir.path(), "real delivery", &expected)
                .await
                .unwrap_err()
                .contains("task-hook-refused")
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn m1d_git_runner_retains_exit_output_and_timeout_reason() {
        use std::os::unix::fs::PermissionsExt;
        let tools = tempfile::tempdir().unwrap();
        let fake = tools.path().join("git");
        std::fs::write(
            &fake,
            "#!/bin/sh\necho useful-stdout\necho permission-denied >&2\nexit 9\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut env = env();
        env.vars
            .insert("PATH".into(), tools.path().to_string_lossy().into_owned());
        let error = super::super::run(&env, tools.path(), &["merge", "source"], &[], 10)
            .await
            .unwrap_err();
        assert!(
            error.contains("git merge source")
                && error.contains('9')
                && error.contains("useful-stdout")
                && error.contains("permission-denied"),
            "{error}"
        );
        std::fs::write(&fake, "#!/bin/sh\nexec /bin/sleep 1\n").unwrap();
        let timeout = super::super::run(&env, tools.path(), &["merge", "source"], &[], 0)
            .await
            .unwrap_err();
        assert!(
            timeout.contains("git merge source timed out after 0s"),
            "{timeout}"
        );
    }

    #[tokio::test]
    async fn m1d_reconcile_staging_and_committed_tree_after_lost_response() {
        let (dir, env) = repo().await;
        std::fs::write(dir.path().join("src/a"), "accepted").unwrap();
        std::fs::remove_file(dir.path().join("src/b")).unwrap();
        let accepted = snapshot(&env, dir.path()).await.unwrap();
        // Crash after add, before commit.
        super::super::run(&env, dir.path(), &["add", "-A"], &[], 10)
            .await
            .unwrap();
        assert!(commit_verified(&env, dir.path(), "delivery", &accepted)
            .await
            .unwrap());
        let committed = super::super::rev_parse(&env, dir.path(), "HEAD")
            .await
            .unwrap();
        // Crash after commit, before saving its result.
        assert!(!commit_verified(&env, dir.path(), "delivery", &accepted)
            .await
            .unwrap());
        assert_eq!(
            committed,
            super::super::rev_parse(&env, dir.path(), "HEAD")
                .await
                .unwrap()
        );
        assert!(
            commit_verified(&env, dir.path(), "different task", &accepted)
                .await
                .is_err()
        );
        std::fs::write(dir.path().join("src/a"), "external edit").unwrap();
        assert!(commit_verified(&env, dir.path(), "delivery", &accepted)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn disk_index_untracked_deleted_and_renamed_paths_are_authoritative() {
        let (dir, env) = repo().await;
        let root = dir.path();
        std::fs::write(root.join("src/a"), "user dirty before run").unwrap();
        let base = snapshot(&env, root).await.unwrap();
        assert!(!base.clean);
        let index = super::super::run(&env, root, &["ls-files", "--stage", "-z"], &[], 10)
            .await
            .unwrap();
        std::fs::write(root.join("src/a"), "same dirty path, new agent content").unwrap();
        std::fs::rename(root.join("src/b"), root.join("outside renamed\nfile")).unwrap();
        std::fs::write(root.join("untracked"), "new").unwrap();
        let after = snapshot(&env, root).await.unwrap();
        assert_eq!(
            changed(&base, &after),
            vec!["outside renamed\nfile", "src/a", "src/b", "untracked"]
        );
        assert_eq!(
            super::super::run(&env, root, &["ls-files", "--stage", "-z"], &[], 10)
                .await
                .unwrap(),
            index,
            "snapshot must not stage files"
        );
        super::super::run(&env, root, &["add", "src/a"], &[], 10)
            .await
            .unwrap();
        let staged = snapshot(&env, root).await.unwrap();
        assert_ne!(staged.files["src/a"].index, after.files["src/a"].index);
        assert_eq!(staged.files["src/a"].disk, after.files["src/a"].disk);
        let sub = snapshot(&env, &root.join("src")).await.unwrap();
        assert_eq!(sub.prefix, "src");
        assert_eq!(sub.root, after.root);
    }
    #[tokio::test]
    async fn missing_repository_unborn_head_and_submodule_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        let env = env();
        assert!(snapshot(&env, dir.path()).await.is_err());
        super::super::run(&env, dir.path(), &["init"], &[], 10)
            .await
            .unwrap();
        assert!(snapshot(&env, dir.path()).await.is_err());
        let (dir, env) = repo().await;
        let blob = super::super::run(&env, dir.path(), &["rev-parse", "HEAD:src/a"], &[], 10)
            .await
            .unwrap();
        super::super::run(
            &env,
            dir.path(),
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!(
                    "160000,{},submodule",
                    super::super::run(&env, dir.path(), &["rev-parse", "HEAD"], &[], 10)
                        .await
                        .unwrap()
                        .trim()
                ),
            ],
            &[],
            10,
        )
        .await
        .unwrap();
        assert!(snapshot(&env, dir.path())
            .await
            .unwrap_err()
            .contains("Submodule"));
        assert!(!blob.trim().is_empty());
    }
    #[tokio::test]
    async fn hidden_index_flags_and_unreadable_file_types_fail_closed() {
        let (dir, env) = repo().await;
        let root = dir.path();
        super::super::run(
            &env,
            root,
            &["update-index", "--assume-unchanged", "src/a"],
            &[],
            10,
        )
        .await
        .unwrap();
        assert!(snapshot(&env, root)
            .await
            .unwrap_err()
            .contains("assume-unchanged"));
        super::super::run(
            &env,
            root,
            &["update-index", "--no-assume-unchanged", "src/a"],
            &[],
            10,
        )
        .await
        .unwrap();
        std::fs::remove_file(root.join("src/a")).unwrap();
        std::fs::create_dir(root.join("src/a")).unwrap();
        assert!(snapshot(&env, root)
            .await
            .unwrap_err()
            .contains("Unsupported file type"));
    }
    #[tokio::test]
    async fn verified_commit_rejects_stale_evidence_and_commits_accepted_deletions() {
        let (dir, env) = repo().await;
        let root = dir.path();
        std::fs::write(root.join("src/a"), "accepted").unwrap();
        let accepted = snapshot(&env, root).await.unwrap();
        std::fs::write(root.join("outside"), "late gate output").unwrap();
        assert!(commit_verified(&env, root, "must refuse", &accepted)
            .await
            .unwrap_err()
            .contains("changed"));
        assert_eq!(
            super::super::run(&env, root, &["rev-parse", "HEAD"], &[], 10)
                .await
                .unwrap()
                .trim(),
            accepted.head
        );
        std::fs::remove_file(root.join("outside")).unwrap();
        std::fs::remove_file(root.join("src/b")).unwrap();
        std::fs::write(root.join("src/new"), "new accepted").unwrap();
        let accepted = snapshot(&env, root).await.unwrap();
        assert!(commit_verified(&env, root, "accepted", &accepted)
            .await
            .unwrap());
        assert!(snapshot(&env, root).await.unwrap().clean);
        assert!(
            super::super::run(&env, root, &["cat-file", "-e", "HEAD:src/b"], &[], 10)
                .await
                .is_err()
        );
        assert_eq!(
            super::super::run(&env, root, &["show", "HEAD:src/new"], &[], 10)
                .await
                .unwrap(),
            "new accepted"
        );
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn clean_filters_cannot_replace_gate_accepted_contents() {
        let (dir, env) = repo().await;
        let root = dir.path();
        std::fs::write(root.join(".gitattributes"), "src/a filter=replace\n").unwrap();
        super::super::commit_worktree(&env, root, "attributes")
            .await
            .unwrap();
        super::super::run(
            &env,
            root,
            &["config", "filter.replace.clean", "printf transformed"],
            &[],
            10,
        )
        .await
        .unwrap();
        std::fs::write(root.join("src/a"), "accepted by gates").unwrap();
        let accepted = snapshot(&env, root).await.unwrap();
        assert!(commit_verified(&env, root, "must refuse", &accepted)
            .await
            .unwrap_err()
            .contains("Staging transformed"));
        assert_eq!(
            super::super::run(&env, root, &["rev-parse", "HEAD"], &[], 10)
                .await
                .unwrap()
                .trim(),
            accepted.head
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symlinks_modes_and_commit_hook_tree_changes_cannot_hide() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let (dir, env) = repo().await;
        let root = dir.path();
        let before = snapshot(&env, root).await.unwrap();
        std::fs::set_permissions(root.join("src/a"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        symlink("../src/a", root.join("src/link")).unwrap();
        let after = snapshot(&env, root).await.unwrap();
        assert_eq!(changed(&before, &after), vec!["src/a", "src/link"]);
        let hook = root.join(".git/hooks/pre-commit");
        std::fs::write(
            &hook,
            "#!/bin/sh\nprintf unsafe > outside-hook\ngit add outside-hook\n",
        )
        .unwrap();
        std::fs::set_permissions(hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(commit_verified(&env, root, "hook changed tree", &after)
            .await
            .unwrap_err()
            .contains("hooks changed"));
        let external = tempfile::tempdir().unwrap();
        std::fs::write(external.path().join("outside"), "outside").unwrap();
        symlink(external.path().join("outside"), root.join("src/external")).unwrap();
        assert!(snapshot(&env, root)
            .await
            .unwrap_err()
            .contains("Symlink escapes"));
    }
}
