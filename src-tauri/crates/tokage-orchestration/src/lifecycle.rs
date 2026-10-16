//! Plan and Pipeline execution lifecycle engine (docs/03 §9-§10, P7.m5).
//!
//! Submodules / features:
//! - Full gate execution upon plan completion (project-level build + test).
//! - Delivery summary aggregation (models used, touched files, gate results, total cost vs estimated cost, savings).
//! - Optional review task allocation (docs/03 §9).
//! - Pipeline lifecycle controls: Pause, Resume, Cancel, Retry (P7.m5b).
//! - Breakpoint resumption after crash/restart is owned by planning `PipelineState` load
//!   (`INTERRUPTED` + user 「继续」; F7 / S5) — not by auto-READY recovery here.
//! - Task-level worktree isolation with dependency-ordered branch merges and conflict integration tasks (P7.m5d).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use tokage_core::planning::{Plan, TaskItem};
use tokage_native::env::ShellEnv;

use crate::brief::TaskReport;
use crate::gate::GateCommandResult;

/// Plan-level lifecycle status (distinct from `tokage_planning::PipelineStatus`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PlanLifecycleStatus {
    /// In draft or awaiting approval.
    Idle,
    /// Actively scheduling and executing tasks.
    Running,
    /// Paused by user or budget overrun.
    Paused,
    /// Cancelled by user.
    Cancelled,
    /// All tasks completed (or skipped) and full gate passed.
    Completed,
    /// One or more tasks blocked and awaiting human resolution.
    Blocked,
}

/// Delivery summary generated upon plan completion (docs/03 §9).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliverySummary {
    pub plan_id: String,
    pub goal: String,
    pub total_tasks: usize,
    pub completed_tasks: usize,
    pub skipped_tasks: usize,
    pub all_gates_passed: bool,
    pub full_gate_result: Option<GateCommandResult>,
    pub touched_files: Vec<String>,
    pub estimated_cost_usd: f64,
    pub actual_cost_usd: f64,
    pub cost_variance_pct: f64,
    pub review_task: Option<TaskItem>,
    pub task_models: HashMap<String, String>,
}

/// Aggregates completed plan deliverables into a delivery summary (docs/03 §9, P7.m5a).
/// `plan_id` is the `plans.id` string (F12.d); empty falls back to `"unknown"`.
pub fn generate_delivery_summary(
    plan: &Plan,
    reports: &[TaskReport],
    full_gate_result: Option<GateCommandResult>,
    estimated_cost: f64,
    create_review_task: bool,
    plan_id: Option<&str>,
) -> DeliverySummary {
    let mut touched_files_set = std::collections::HashSet::new();
    let mut actual_cost_usd = 0.0;
    let mut task_models = HashMap::new();

    for r in reports {
        for f in &r.changed_files_list {
            touched_files_set.insert(f.clone());
        }
        if let Some(c) = r.cost_usd {
            actual_cost_usd += c;
        }
        task_models.insert(r.task_id.clone(), r.model.clone());
    }

    let all_gates_passed = full_gate_result.as_ref().map(|g| g.passed).unwrap_or(true);
    let mut touched_files: Vec<String> = touched_files_set.into_iter().collect();
    touched_files.sort();

    let cost_variance_pct = if estimated_cost > 0.0 {
        ((actual_cost_usd - estimated_cost) / estimated_cost) * 100.0
    } else {
        0.0
    };

    let review_task = if create_review_task {
        Some(TaskItem {
            id: "T-review".to_string(),
            domain: "review".to_string(),
            title: format!("Review change summary: {}", plan.spec.goal),
            goal: "Read-only: check each task's changes and deviations against the contracts"
                .to_string(),
            depends_on: plan.tasks.iter().map(|t| t.id.clone()).collect(),
            contracts: plan.contracts.iter().map(|c| c.id.clone()).collect(),
            write_scope: vec![],
            deliverables: vec!["review_comments.md".to_string()],
            acceptance: tokage_core::planning::TaskAcceptance::default(),
            size: tokage_core::planning::TaskSize::S,
            difficulty: 1,
            assigned_model: None,
            model_params: None,
        })
    } else {
        None
    };

    DeliverySummary {
        plan_id: plan_id
            .filter(|s| !s.is_empty())
            .unwrap_or("unknown")
            .to_string(),
        goal: plan.spec.goal.clone(),
        total_tasks: plan.tasks.len(),
        completed_tasks: reports.len(),
        skipped_tasks: plan.tasks.len().saturating_sub(reports.len()),
        all_gates_passed,
        full_gate_result,
        touched_files,
        estimated_cost_usd: estimated_cost,
        actual_cost_usd,
        cost_variance_pct,
        review_task,
        task_models,
    }
}

/// Merge result when merging task-level worktree branches (P7.m5d / F11).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum WorktreeMergeOutcome {
    Success {
        merged_branch: String,
    },
    Conflict {
        #[serde(default)]
        diagnostic: String,
        conflicted_files: Vec<String>,
        integration_task: TaskItem,
    },
}

/// Merges a task's worktree branch into the integration branch, creating an integration task on conflict (P7.m5d).
pub async fn merge_task_worktree(
    env: &ShellEnv,
    project_dir: &Path,
    target_branch: &str,
    task_branch: &str,
    task_item: &TaskItem,
    expected_commit: Option<&str>,
) -> Result<WorktreeMergeOutcome, String> {
    use tokage_native::git::run;
    // Never switch an unexpected checkout or merge into an index with unrelated work.
    let branch = run(
        env,
        project_dir,
        &["symbolic-ref", "--short", "HEAD"],
        &[],
        10,
    )
    .await?;
    if branch.trim() != target_branch {
        return Err(format!(
            "Integration checkout branch mismatch: expected {target_branch}, got {}",
            branch.trim()
        ));
    }
    require_clean(env, project_dir).await?;
    let source = run(
        env,
        project_dir,
        &[
            "rev-parse",
            "--verify",
            &format!("refs/heads/{task_branch}^{{commit}}"),
        ],
        &[],
        10,
    )
    .await?;
    let source = source.trim();
    if expected_commit.is_some_and(|expected| expected != source) {
        return Err("Task branch changed after accepted commit".into());
    }
    if task_item.domain == "integration" {
        for path in &task_item.write_scope {
            let entry = run(
                env,
                project_dir,
                &["ls-tree", "--name-only", source, "--", path],
                &[],
                10,
            )
            .await?;
            if entry.is_empty() {
                continue;
            } // resolution may delete the conflicted file
            let content = run(
                env,
                project_dir,
                &["show", &format!("{source}:{path}")],
                &[],
                10,
            )
            .await?;
            if content.lines().any(|line| {
                line.starts_with("<<<<<<< ") || line.starts_with(">>>>>>> ") || line == "======="
            }) {
                return Err(format!(
                    "Unresolved conflict markers in {path}; preserving private baseline"
                ));
            }
        }
    }
    let before = run(env, project_dir, &["rev-parse", "HEAD"], &[], 10).await?;
    // A lost IPC response after merge must not create a second merge commit.
    if contains_commit(env, project_dir, source).await? {
        return Ok(WorktreeMergeOutcome::Success {
            merged_branch: target_branch.into(),
        });
    }
    let merge_res = run(
        env,
        project_dir,
        &[
            "-c",
            "user.name=Tokage",
            "-c",
            "user.email=tokage@localhost",
            "merge",
            "--no-ff",
            "-m",
            &format!("Merge task {}", task_item.id),
            source,
        ],
        &[],
        60,
    )
    .await;
    match merge_res {
        Ok(_) => {
            require_clean(env, project_dir).await?;
            if !contains_commit(env, project_dir, source).await? {
                return Err("Merge returned success without containing the task commit".into());
            }
            Ok(WorktreeMergeOutcome::Success {
                merged_branch: target_branch.into(),
            })
        }
        Err(err) => {
            let conflicted_files = conflict_files(env, project_dir)
                .await
                .map_err(|check| format!("{err}; conflict inspection failed: {check}"))?;
            if conflicted_files.is_empty() {
                return Err(format!("Merge infrastructure/validation failure (no confirmed content conflict); {err}"));
            }
            let merge_head = run(
                env,
                project_dir,
                &["rev-parse", "--verify", "MERGE_HEAD"],
                &[],
                10,
            )
            .await
            .map_err(|check| format!("{err}; MERGE_HEAD inspection failed: {check}"))?;
            if merge_head.trim() != source {
                return Err(format!(
                    "{err}; MERGE_HEAD does not match the accepted task commit"
                ));
            }
            // Abort only our confirmed conflict. Failed rollback is a blocking error too.
            run(env, project_dir, &["merge", "--abort"], &[], 30)
                .await
                .map_err(|abort| format!("{err}; merge abort failed: {abort}"))?;
            require_clean(env, project_dir)
                .await
                .map_err(|check| format!("{err}; after abort: {check}"))?;
            if run(env, project_dir, &["rev-parse", "HEAD"], &[], 10).await? != before {
                return Err(format!("{err}; integration HEAD changed during abort"));
            }

            let integration_task = TaskItem {
                id: format!("{}-integration", task_item.id),
                domain: "integration".to_string(),
                title: format!("Resolve merge conflict: {}", task_item.title),
                goal: format!(
                    "Resolve the conflict markers already prepared in this checkout when merging branch {} into {}: {:?}. Edit only these files; do not change Git history.",
                    task_branch, target_branch, conflicted_files
                ),
                depends_on: vec![task_item.id.clone()],
                contracts: task_item.contracts.clone(),
                write_scope: conflicted_files.clone(),
                deliverables: conflicted_files.clone(),
                acceptance: task_item.acceptance.clone(),
                size: tokage_core::planning::TaskSize::S,
                difficulty: 2,
                assigned_model: task_item.assigned_model.clone(),
                model_params: task_item.model_params.clone(),
            };

            Ok(WorktreeMergeOutcome::Conflict {
                diagnostic: err,
                conflicted_files,
                integration_task,
            })
        }
    }
}

async fn conflict_files(env: &ShellEnv, dir: &Path) -> Result<Vec<String>, String> {
    let paths = tokage_native::git::run(
        env,
        dir,
        &["diff", "--name-only", "--diff-filter=U", "-z"],
        &[],
        10,
    )
    .await?;
    Ok(paths
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .collect())
}

async fn require_clean(env: &ShellEnv, dir: &Path) -> Result<(), String> {
    let status = tokage_native::git::run(
        env,
        dir,
        &["status", "--porcelain=v1", "--untracked-files=all"],
        &[],
        10,
    )
    .await?;
    let merge_path = tokage_native::git::run(
        env,
        dir,
        &["rev-parse", "--git-path", "MERGE_HEAD"],
        &[],
        10,
    )
    .await?;
    let merge_path = dir.join(merge_path.trim());
    if !status.is_empty() || merge_path.try_exists().map_err(|e| e.to_string())? {
        return Err(
            "Integration checkout has pending changes or an unfinished merge; preserving it".into(),
        );
    }
    Ok(())
}

/// `merge-base` read failures are never interpreted as an absent ancestor.
async fn contains_commit(env: &ShellEnv, dir: &Path, commit: &str) -> Result<bool, String> {
    let base = tokage_native::git::run(env, dir, &["merge-base", "HEAD", commit], &[], 10).await?;
    Ok(base.trim() == commit)
}

/// Prepare a *persisted* conflict worktree. Commit the conflict markers as a private
/// baseline so normal scope verification can measure only the resolver's edits.
/// The task's branch remains retained until integration succeeds.
pub async fn prepare_conflict_worktree(
    env: &ShellEnv,
    dir: &Path,
    task_branch: &str,
    expected_commit: Option<&str>,
    expected_conflicts: &[String],
) -> Result<(), String> {
    use tokage_native::git::run;
    let source = run(
        env,
        dir,
        &[
            "rev-parse",
            "--verify",
            &format!("refs/heads/{task_branch}^{{commit}}"),
        ],
        &[],
        10,
    )
    .await?;
    let source = source.trim();
    if expected_commit.is_some_and(|commit| commit != source) {
        return Err("Conflict source branch changed after accepted commit".into());
    }
    if contains_commit(env, dir, source).await? {
        return require_clean(env, dir).await;
    }
    let pending = run(env, dir, &["rev-parse", "--verify", "MERGE_HEAD"], &[], 10).await;
    if pending.as_deref().map(str::trim) != Ok(source) {
        require_clean(env, dir).await?;
        if let Err(err) = run(
            env,
            dir,
            &[
                "-c",
                "user.name=Tokage",
                "-c",
                "user.email=tokage@localhost",
                "merge",
                "--no-commit",
                "--no-ff",
                source,
            ],
            &[],
            60,
        )
        .await
        {
            if conflict_files(env, dir).await?.is_empty()
                || run(env, dir, &["rev-parse", "--verify", "MERGE_HEAD"], &[], 10)
                    .await?
                    .trim()
                    != source
            {
                return Err(format!(
                    "Conflict worktree preparation failed; preserving {}: {err}",
                    dir.display()
                ));
            }
        }
    }
    let actual = conflict_files(env, dir).await?;
    if actual.iter().any(|path| !expected_conflicts.contains(path)) {
        return Err(format!(
            "New conflict paths outside the persisted resolver scope: {actual:?}; preserving {}",
            dir.display()
        ));
    }
    tokage_native::git::scope::commit_private_conflict_baseline(env, dir).await?;
    require_clean(env, dir).await?;
    if !contains_commit(env, dir, source).await? {
        return Err("Conflict baseline lost task ancestry".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokage_core::planning::{TaskAcceptance, TaskSize};

    async fn git_repo() -> (tempfile::TempDir, ShellEnv) {
        let dir = tempfile::tempdir().unwrap();
        let env = ShellEnv {
            vars: std::env::vars().collect(),
        };
        tokage_native::git::run(&env, dir.path(), &["init", "-b", "integration"], &[], 10)
            .await
            .unwrap();
        std::fs::write(dir.path().join("a"), "original\n").unwrap();
        tokage_native::git::commit_worktree(&env, dir.path(), "initial")
            .await
            .unwrap();
        tokage_native::git::run(&env, dir.path(), &["checkout", "-b", "task"], &[], 10)
            .await
            .unwrap();
        std::fs::write(dir.path().join("a"), "task content\n").unwrap();
        tokage_native::git::commit_worktree(&env, dir.path(), "task")
            .await
            .unwrap();
        tokage_native::git::run(&env, dir.path(), &["checkout", "integration"], &[], 10)
            .await
            .unwrap();
        (dir, env)
    }
    async fn merge(
        env: &ShellEnv,
        dir: &Path,
        branch: &str,
    ) -> Result<WorktreeMergeOutcome, String> {
        merge_task_worktree(
            env,
            dir,
            "integration",
            branch,
            &TaskItem {
                id: "T1".into(),
                assigned_model: Some("mock:resolver".into()),
                ..Default::default()
            },
            None,
        )
        .await
    }

    #[tokio::test]
    async fn m1d_merge_reconciles_lost_response_without_duplicate_commit() {
        let (dir, env) = git_repo().await;
        assert!(matches!(
            merge(&env, dir.path(), "task").await.unwrap(),
            WorktreeMergeOutcome::Success { .. }
        ));
        let head = tokage_native::git::rev_parse(&env, dir.path(), "HEAD")
            .await
            .unwrap();
        assert!(matches!(
            merge(&env, dir.path(), "task").await.unwrap(),
            WorktreeMergeOutcome::Success { .. }
        ));
        assert_eq!(
            head,
            tokage_native::git::rev_parse(&env, dir.path(), "HEAD")
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn m1d_missing_branch_lock_and_wrong_identity_are_not_conflicts() {
        let (dir, env) = git_repo().await;
        let before = tokage_native::git::rev_parse(&env, dir.path(), "HEAD")
            .await
            .unwrap();
        let missing = merge(&env, dir.path(), "missing").await.unwrap_err();
        assert!(missing.contains("exited"), "{missing}");
        std::fs::write(dir.path().join(".git/index.lock"), "lock").unwrap();
        let locked = merge(&env, dir.path(), "task").await.unwrap_err();
        assert!(
            locked.contains("index.lock") && locked.contains("exited"),
            "{locked}"
        );
        std::fs::remove_file(dir.path().join(".git/index.lock")).unwrap();
        assert_eq!(
            before,
            tokage_native::git::rev_parse(&env, dir.path(), "HEAD")
                .await
                .unwrap()
        );
        assert!(merge_task_worktree(
            &env,
            dir.path(),
            "integration",
            "task",
            &TaskItem::default(),
            Some(&before)
        )
        .await
        .unwrap_err()
        .contains("changed"));
        assert!(merge_task_worktree(
            &env,
            dir.path(),
            "wrong",
            "task",
            &TaskItem::default(),
            None
        )
        .await
        .unwrap_err()
        .contains("mismatch"));
        std::fs::write(dir.path().join("untracked"), "user content").unwrap();
        assert!(merge(&env, dir.path(), "task")
            .await
            .unwrap_err()
            .contains("pending"));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("untracked")).unwrap(),
            "user content"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn m1d_hook_rejection_preserves_error_and_pending_merge() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, env) = git_repo().await;
        let hook = dir.path().join(".git/hooks/pre-merge-commit");
        std::fs::write(&hook, "#!/bin/sh\necho merge-hook-refused >&2\nexit 23\n").unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        let error = merge(&env, dir.path(), "task").await.unwrap_err();
        assert!(
            error.contains("merge-hook-refused") && error.contains("exited"),
            "{error}"
        );
        assert!(dir.path().join(".git/MERGE_HEAD").exists());
        assert!(merge(&env, dir.path(), "task")
            .await
            .unwrap_err()
            .contains("pending"));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a")).unwrap(),
            "task content\n"
        );
    }

    #[tokio::test]
    async fn m1d_real_conflict_prepares_clean_resolvable_baseline_and_retains_source() {
        let (dir, env) = git_repo().await;
        std::fs::write(dir.path().join("a"), "integration content\n").unwrap();
        tokage_native::git::commit_worktree(&env, dir.path(), "integration edit")
            .await
            .unwrap();
        let before = tokage_native::git::rev_parse(&env, dir.path(), "HEAD")
            .await
            .unwrap();
        let WorktreeMergeOutcome::Conflict {
            diagnostic,
            conflicted_files,
            integration_task,
        } = merge(&env, dir.path(), "task").await.unwrap()
        else {
            panic!("expected conflict")
        };
        assert!(diagnostic.contains("exited") && diagnostic.contains("CONFLICT"));
        assert_eq!(conflicted_files, vec!["a"]);
        assert_eq!(integration_task.write_scope, vec!["a"]);
        assert_eq!(
            integration_task.assigned_model.as_deref(),
            Some("mock:resolver")
        );
        assert_eq!(
            before,
            tokage_native::git::rev_parse(&env, dir.path(), "HEAD")
                .await
                .unwrap()
        );
        assert!(!dir.path().join(".git/MERGE_HEAD").exists());
        let managed = tempfile::tempdir().unwrap();
        let wt = tokage_native::git::create_worktree(
            &env,
            dir.path(),
            managed.path(),
            "resolver",
            Some("integration"),
        )
        .await
        .unwrap();
        assert!(prepare_conflict_worktree(
            &env,
            Path::new(&wt.root),
            "task",
            Some("wrong-source"),
            &["a".into()]
        )
        .await
        .unwrap_err()
        .contains("changed"));
        assert!(
            prepare_conflict_worktree(&env, Path::new(&wt.root), "task", None, &[])
                .await
                .unwrap_err()
                .contains("New conflict paths")
        );
        prepare_conflict_worktree(&env, Path::new(&wt.root), "task", None, &["a".into()])
            .await
            .unwrap();
        assert!(
            tokage_native::git::scope::snapshot(&env, Path::new(&wt.cwd))
                .await
                .unwrap()
                .clean
        );
        let baseline = tokage_native::git::rev_parse(&env, Path::new(&wt.root), "HEAD")
            .await
            .unwrap();
        // Preparation replay is idempotent after a lost IPC response.
        prepare_conflict_worktree(&env, Path::new(&wt.root), "task", None, &["a".into()])
            .await
            .unwrap();
        assert_eq!(
            baseline,
            tokage_native::git::rev_parse(&env, Path::new(&wt.root), "HEAD")
                .await
                .unwrap()
        );
        assert!(merge_task_worktree(
            &env,
            dir.path(),
            "integration",
            &wt.branch,
            &integration_task,
            Some(&baseline)
        )
        .await
        .unwrap_err()
        .contains("Unresolved conflict markers"));
        std::fs::write(Path::new(&wt.root).join("a"), "resolved together\n").unwrap();
        tokage_native::git::commit_worktree(&env, Path::new(&wt.root), "resolve")
            .await
            .unwrap();
        assert!(matches!(
            merge_task_worktree(
                &env,
                dir.path(),
                "integration",
                &wt.branch,
                &integration_task,
                None
            )
            .await
            .unwrap(),
            WorktreeMergeOutcome::Success { .. }
        ));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a")).unwrap(),
            "resolved together\n"
        );
        assert!(tokage_native::git::rev_parse(&env, dir.path(), "task")
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn m1d_preparation_does_not_ignore_missing_source_or_dirty_checkout() {
        let (dir, env) = git_repo().await;
        assert!(
            prepare_conflict_worktree(&env, dir.path(), "missing", None, &[])
                .await
                .unwrap_err()
                .contains("exited")
        );
        std::fs::write(dir.path().join("user"), "keep").unwrap();
        assert!(
            prepare_conflict_worktree(&env, dir.path(), "task", None, &[])
                .await
                .unwrap_err()
                .contains("pending")
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("user")).unwrap(),
            "keep"
        );
    }

    #[test]
    fn test_generate_delivery_summary_and_review_task() {
        let mut plan = Plan::default();
        plan.spec.goal = "用户中心微服务改造".to_string();
        plan.tasks = vec![TaskItem {
            id: "T1".to_string(),
            domain: "backend".to_string(),
            title: "DB 表".to_string(),
            goal: "".to_string(),
            depends_on: vec![],
            contracts: vec![],
            write_scope: vec!["src/db/".to_string()],
            deliverables: vec![],
            acceptance: TaskAcceptance::default(),
            size: TaskSize::S,
            difficulty: 1,
            assigned_model: None,
            model_params: None,
        }];

        let reports = vec![TaskReport {
            task_id: "T1".to_string(),
            title: "DB 表".to_string(),
            model: "claude-3-7-sonnet".to_string(),
            attempt_id: "att-1".to_string(),
            first_pass: true,
            is_locally_generated: false,
            completed_work: "Done".to_string(),
            contract_deviations: None,
            tips_for_downstream: "".to_string(),
            self_check_criteria: vec![],
            changed_files_stat: "schema.sql +10".to_string(),
            changed_files_list: vec!["schema.sql".to_string()],
            gate_results_summary: "passed".to_string(),
            cost_usd: Some(0.15),
            defects: None,
        }];

        let summary = generate_delivery_summary(&plan, &reports, None, 0.20, true, Some("42"));
        assert_eq!(summary.total_tasks, 1);
        assert_eq!(summary.completed_tasks, 1);
        assert_eq!(summary.touched_files, vec!["schema.sql".to_string()]);
        assert_eq!(summary.actual_cost_usd, 0.15);
        assert!(summary.review_task.is_some());
        assert_eq!(summary.review_task.unwrap().domain, "review");
    }

    #[test]
    fn f11b_worktree_merge_outcome_serde_camel_case_fields() {
        let outcome = WorktreeMergeOutcome::Conflict {
            diagnostic: "content conflict".into(),
            conflicted_files: vec!["a.rs".into()],
            integration_task: TaskItem {
                id: "T1-integration".into(),
                ..TaskItem::default()
            },
        };
        let v = serde_json::to_value(&outcome).unwrap();
        let obj = v.get("conflict").expect("conflict tag");
        assert!(obj.get("conflictedFiles").is_some());
        assert!(obj.get("integrationTask").is_some());
        assert!(obj.get("conflicted_files").is_none());
    }
}
