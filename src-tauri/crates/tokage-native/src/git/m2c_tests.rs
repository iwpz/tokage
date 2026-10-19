//! Real Git fixtures: no CLI or Git operations are mocked.
use super::*;

struct Fixture {
    _temp: tempfile::TempDir,
    env: ShellEnv,
    repo: PathBuf,
    app: PathBuf,
}
impl Fixture {
    async fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        std::fs::create_dir_all(repo.join("packages/app")).unwrap();
        let f = Self {
            app: temp.path().join("appdata"),
            _temp: temp,
            env: ShellEnv {
                vars: std::env::vars().collect(),
            },
            repo,
        };
        for args in [
            vec!["init", "-qb", "main"],
            vec!["config", "user.name", "fixture"],
            vec!["config", "user.email", "fixture@example.invalid"],
        ] {
            f.git(&args).await;
        }
        std::fs::write(f.repo.join("packages/app/base.txt"), "base\n").unwrap();
        f.git(&["add", "."]).await;
        f.git(&["commit", "-qm", "base"]).await;
        f
    }
    async fn git(&self, args: &[&str]) -> String {
        run(&self.env, &self.repo, args, &[], 20).await.unwrap()
    }
    async fn worktree(&self, name: &str) -> Worktree {
        create_worktree(
            &self.env,
            &self.repo.join("packages/app"),
            &self.app,
            name,
            None,
        )
        .await
        .unwrap()
    }
}

#[tokio::test]
async fn dirty_checkout_subproject_and_runtime_edits_are_preserved() {
    let f = Fixture::new().await;
    std::fs::write(f.repo.join("packages/app/base.txt"), "staged\n").unwrap();
    f.git(&["add", "."]).await;
    std::fs::write(f.repo.join("packages/app/base.txt"), "unstaged\n").unwrap();
    std::fs::write(f.repo.join("untracked.dat"), [0, 255, 1]).unwrap();
    let index = f.git(&["write-tree"]).await;
    let status = f.git(&["status", "--porcelain=v1"]).await;
    let target = checkout_identity(&f.env, &f.repo).await.unwrap();
    let wt = f.worktree("m2c-dirty").await;
    assert_eq!(Path::new(&wt.cwd), Path::new(&wt.root).join("packages/app"));
    // Isolation starts at committed HEAD, never copies or stages the user's pending work.
    assert_eq!(
        std::fs::read_to_string(Path::new(&wt.cwd).join("base.txt")).unwrap(),
        "base\n"
    );
    assert!(!Path::new(&wt.root).join("untracked.dat").exists());
    std::fs::write(
        f.repo.join("packages/app/base.txt"),
        "runtime human update\n",
    )
    .unwrap();
    std::fs::write(Path::new(&wt.cwd).join("feature"), "result").unwrap();
    commit_worktree(&f.env, Path::new(&wt.root), "feature")
        .await
        .unwrap();
    let accepted = scope::snapshot(&f.env, Path::new(&wt.cwd)).await.unwrap();
    assert_eq!(
        merge_accepted_into_checkout(&f.env, &f.repo, &wt.branch, &accepted, Some(&target))
            .await
            .unwrap_err(),
        "DIRTY"
    );
    assert_eq!(f.git(&["write-tree"]).await, index);
    assert_eq!(f.git(&["status", "--porcelain=v1"]).await, status);
    assert_eq!(
        std::fs::read(f.repo.join("untracked.dat")).unwrap(),
        [0, 255, 1]
    );
    // Human edits made after acceptance cannot be discarded by the keep-branch path.
    std::fs::write(Path::new(&wt.cwd).join("feature"), "human update").unwrap();
    assert_eq!(
        remove_worktree(&f.env, Path::new(&wt.root), &wt.branch, false, true)
            .await
            .unwrap_err(),
        "DIRTY"
    );
    assert_eq!(
        std::fs::read_to_string(Path::new(&wt.cwd).join("feature")).unwrap(),
        "human update"
    );
    assert_eq!(
        std::fs::read_to_string(f.repo.join("packages/app/base.txt")).unwrap(),
        "runtime human update\n"
    );
}

#[tokio::test]
async fn accepted_merge_rejects_same_head_branch_switch_and_new_target_commit() {
    let f = Fixture::new().await;
    let target = checkout_identity(&f.env, &f.repo).await.unwrap();
    let wt = f.worktree("m2c-target").await;
    std::fs::write(Path::new(&wt.cwd).join("feature"), "result").unwrap();
    commit_worktree(&f.env, Path::new(&wt.root), "feature")
        .await
        .unwrap();
    let accepted = scope::snapshot(&f.env, Path::new(&wt.cwd)).await.unwrap();
    f.git(&["checkout", "-qb", "human-branch"]).await;
    assert!(
        merge_accepted_into_checkout(&f.env, &f.repo, &wt.branch, &accepted, Some(&target))
            .await
            .unwrap_err()
            .contains("TARGET")
    );
    assert!(!f.repo.join("packages/app/feature").exists());
    f.git(&["checkout", "main"]).await;
    f.git(&["commit", "--allow-empty", "-qm", "human commit"])
        .await;
    assert!(
        merge_accepted_into_checkout(&f.env, &f.repo, &wt.branch, &accepted, Some(&target))
            .await
            .unwrap_err()
            .contains("TARGET")
    );
    assert!(
        merge_accepted_into_checkout(&f.env, &f.repo, &wt.branch, &accepted, None)
            .await
            .unwrap_err()
            .contains("TARGET")
    );
}

#[tokio::test]
async fn patch_roundtrip_preserves_bytes_modes_renames_deletes_and_survives_cleanup() {
    let f = Fixture::new().await;
    std::fs::write(f.repo.join("delete"), "delete me").unwrap();
    std::fs::write(f.repo.join("rename"), "move me\n".repeat(30)).unwrap();
    f.git(&["add", "."]).await;
    f.git(&["commit", "-qm", "patch baseline"]).await;
    let base = f.git(&["rev-parse", "HEAD"]).await.trim().to_owned();
    let wt = f.worktree("m2c-patch").await;
    let root = Path::new(&wt.root);
    std::fs::write(root.join("binary"), [0, 255, 1, 0, 128]).unwrap();
    std::fs::write(root.join("non-utf8"), [255, 254, b'\n', 128]).unwrap();
    std::fs::write(root.join("空 格.txt"), "你好\r\nsecond\r\n").unwrap();
    std::fs::write(root.join("script"), "#!/bin/sh\necho ok\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root.join("script"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        std::os::unix::fs::symlink("空 格.txt", root.join("link")).unwrap();
    }
    std::fs::remove_file(root.join("delete")).unwrap();
    std::fs::rename(root.join("rename"), root.join("renamed")).unwrap();
    commit_worktree(&f.env, root, "all supported file types")
        .await
        .unwrap();
    let source_tree = run(&f.env, root, &["rev-parse", "HEAD^{tree}"], &[], 10)
        .await
        .unwrap();
    let source = rev_parse(&f.env, root, "HEAD").await.unwrap();
    let patch = f._temp.path().join("delivery.patch");
    export_patch_file(&f.env, &f.repo, &base, &source, &patch)
        .await
        .unwrap();
    let bytes = std::fs::read(&patch).unwrap();
    assert!(bytes
        .windows(b"GIT binary patch".len())
        .any(|s| s == b"GIT binary patch"));
    assert!(export_patch_file(&f.env, &f.repo, &base, &source, &patch)
        .await
        .is_err());
    assert_eq!(std::fs::read(&patch).unwrap(), bytes);
    remove_worktree(&f.env, root, &wt.branch, false, true)
        .await
        .unwrap();
    assert!(!root.exists());
    assert_eq!(
        rev_parse(&f.env, &f.repo, &wt.branch).await.unwrap(),
        source
    );
    f.git(&["apply", "--index", patch.to_str().unwrap()]).await;
    assert_eq!(f.git(&["write-tree"]).await, source_tree);
    assert_eq!(
        std::fs::read(f.repo.join("binary")).unwrap(),
        [0, 255, 1, 0, 128]
    );
    assert_eq!(
        std::fs::read(f.repo.join("non-utf8")).unwrap(),
        [255, 254, b'\n', 128]
    );
    if let Some(out) = std::env::var_os("TOKAGE_M2C_EVIDENCE_DIR") {
        let out = PathBuf::from(out);
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("delivery.patch"), &bytes).unwrap();
        f.git(&[
            "bundle",
            "create",
            out.join("patch-and-kept-branch.bundle").to_str().unwrap(),
            "--all",
        ])
        .await;
        std::fs::write(out.join("patch-and-kept-branch.json"), serde_json::to_vec_pretty(&serde_json::json!({
            "base": base, "source": source, "sourceTree": source_tree.trim(), "retainedBranch": wt.branch,
            "worktreeRemoved": !root.exists(), "roundTripIndexTree": f.git(&["write-tree"]).await.trim(),
        })).unwrap()).unwrap();
    }
}

#[tokio::test]
async fn real_conflict_aborts_only_own_merge_and_regular_git_errors_stay_regular() {
    let f = Fixture::new().await;
    let wt = f.worktree("m2c-conflict").await;
    std::fs::write(Path::new(&wt.cwd).join("base.txt"), "task\n").unwrap();
    commit_worktree(&f.env, Path::new(&wt.root), "task")
        .await
        .unwrap();
    std::fs::write(f.repo.join("packages/app/base.txt"), "human\n").unwrap();
    f.git(&["add", "."]).await;
    f.git(&["commit", "-qm", "human"]).await;
    let head = f.git(&["rev-parse", "HEAD"]).await;
    assert!(merge_branch_into_checkout(&f.env, &f.repo, &wt.branch)
        .await
        .unwrap_err()
        .starts_with("CONFLICT"));
    assert_eq!(f.git(&["rev-parse", "HEAD"]).await, head);
    assert!(is_working_tree_clean(&f.env, &f.repo).await.unwrap());
    // A real user merge remains intact, including unmerged index entries.
    assert!(run(&f.env, &f.repo, &["merge", &wt.branch], &[], 10)
        .await
        .is_err());
    let index = f.git(&["ls-files", "--stage"]).await;
    assert!(merge_branch_into_checkout(&f.env, &f.repo, &wt.branch)
        .await
        .is_err());
    assert_eq!(f.git(&["ls-files", "--stage"]).await, index);
    assert!(f.repo.join(".git/MERGE_HEAD").exists());
    f.git(&["merge", "--abort"]).await;
    // Lock errors must never be misclassified as conflicts or delete a user's lock.
    std::fs::write(f.repo.join(".git/index.lock"), "human lock").unwrap();
    let err = merge_branch_into_checkout(&f.env, &f.repo, &wt.branch)
        .await
        .unwrap_err();
    assert!(!err.starts_with("CONFLICT"), "{err}");
    assert_eq!(
        std::fs::read_to_string(f.repo.join(".git/index.lock")).unwrap(),
        "human lock"
    );
    assert_eq!(f.git(&["rev-parse", "HEAD"]).await, head);
}

#[tokio::test]
async fn parallel_outputs_enter_only_integration_and_downstream_reads_both_before_delivery() {
    let f = Fixture::new().await;
    let target = checkout_identity(&f.env, &f.repo).await.unwrap();
    let integration = f.worktree("m2c-integration").await;
    let project = f.repo.join("packages/app");
    let (a, b) = tokio::join!(
        create_worktree(&f.env, &project, &f.app, "m2c-A", Some(&integration.branch)),
        create_worktree(&f.env, &project, &f.app, "m2c-B", Some(&integration.branch))
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    std::fs::write(Path::new(&a.cwd).join("a"), "A").unwrap();
    std::fs::write(Path::new(&b.cwd).join("b"), "B").unwrap();
    let (ca, cb) = tokio::join!(
        commit_worktree(&f.env, Path::new(&a.root), "A"),
        commit_worktree(&f.env, Path::new(&b.root), "B")
    );
    assert!(ca.unwrap());
    assert!(cb.unwrap());
    merge_branch_into_checkout(&f.env, Path::new(&integration.root), &a.branch)
        .await
        .unwrap();
    merge_branch_into_checkout(&f.env, Path::new(&integration.root), &b.branch)
        .await
        .unwrap();
    assert!(!f.repo.join("packages/app/a").exists());
    assert!(!f.repo.join("packages/app/b").exists());
    let downstream = create_worktree(
        &f.env,
        &f.repo.join("packages/app"),
        &f.app,
        "m2c-join",
        Some(&integration.branch),
    )
    .await
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(Path::new(&downstream.cwd).join("a")).unwrap(),
        "A"
    );
    assert_eq!(
        std::fs::read_to_string(Path::new(&downstream.cwd).join("b")).unwrap(),
        "B"
    );
    let accepted = scope::snapshot(&f.env, Path::new(&integration.cwd))
        .await
        .unwrap();
    merge_accepted_into_checkout(
        &f.env,
        &f.repo,
        &integration.branch,
        &accepted,
        Some(&target),
    )
    .await
    .unwrap();
    assert_eq!(f.git(&["rev-parse", "HEAD^2"]).await.trim(), accepted.head);
    assert_eq!(
        std::fs::read_to_string(f.repo.join("packages/app/a")).unwrap(),
        "A"
    );
    assert_eq!(
        std::fs::read_to_string(f.repo.join("packages/app/b")).unwrap(),
        "B"
    );
    if let Some(out) = std::env::var_os("TOKAGE_M2C_EVIDENCE_DIR") {
        let out = PathBuf::from(out);
        std::fs::create_dir_all(&out).unwrap();
        f.git(&[
            "bundle",
            "create",
            out.join("merged-delivery.bundle").to_str().unwrap(),
            "--all",
        ])
        .await;
        std::fs::write(
            out.join("merged-delivery.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "initialTarget": target, "acceptedSource": accepted.head,
                "deliveredHead": f.git(&["rev-parse", "HEAD"]).await.trim(),
                "deliveredSecondParent": f.git(&["rev-parse", "HEAD^2"]).await.trim(),
                "clean": is_working_tree_clean(&f.env, &f.repo).await.unwrap(),
                "integrationBranch": integration.branch,
            }))
            .unwrap(),
        )
        .unwrap();
    }
}

#[tokio::test]
async fn cleanup_refuses_a_checkout_switched_to_a_different_branch() {
    let f = Fixture::new().await;
    let wt = f.worktree("m2c-cleanup").await;
    run(
        &f.env,
        Path::new(&wt.root),
        &["checkout", "-qb", "human-branch"],
        &[],
        10,
    )
    .await
    .unwrap();
    let err = remove_worktree(&f.env, Path::new(&wt.root), &wt.branch, false, true)
        .await
        .unwrap_err();
    assert!(err.contains("branch changed"));
    assert!(Path::new(&wt.root).exists());
    assert_eq!(
        checkout_identity(&f.env, Path::new(&wt.root))
            .await
            .unwrap()
            .reference,
        "refs/heads/human-branch"
    );
}

#[tokio::test]
async fn cleanup_refuses_new_human_commits_even_when_checkout_is_clean() {
    let f = Fixture::new().await;
    let wt = f.worktree("m2c-new-commit").await;
    let accepted = rev_parse(&f.env, Path::new(&wt.root), "HEAD")
        .await
        .unwrap();
    std::fs::write(Path::new(&wt.cwd).join("human"), "new human commit").unwrap();
    commit_worktree(&f.env, Path::new(&wt.root), "human")
        .await
        .unwrap();
    assert!(is_working_tree_clean(&f.env, Path::new(&wt.root))
        .await
        .unwrap());
    assert!(remove_worktree_if_head(
        &f.env,
        Path::new(&wt.root),
        &wt.branch,
        false,
        false,
        Some(&accepted)
    )
    .await
    .unwrap_err()
    .contains("HEAD changed"));
    assert!(Path::new(&wt.root).exists());
    assert_eq!(
        std::fs::read_to_string(Path::new(&wt.cwd).join("human")).unwrap(),
        "new human commit"
    );
}
