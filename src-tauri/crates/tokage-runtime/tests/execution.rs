#![cfg(unix)]
mod common;
#[path = "common/native.rs"]
mod native;
use common::*;
use native::*;
use tokage_runtime::*;
use tokage_runtime::{owned::Runtime, repository::SqliteRepository, service::NativePlanner};
#[tokio::test]
async fn native_task_acceptance_commits_and_integrates_without_touching_target() {
    let (dir, cwd, req) = fixture();
    let original = git(&cwd, &["rev-parse", "HEAD"]);
    let repo = SqliteRepository::open(&dir.path().join("db"), "run").unwrap();
    let native = NativePlanner::new(
        tokage_native::env::ShellEnv {
            vars: std::env::vars().collect(),
        },
        &dir.path().join("native"),
    )
    .unwrap();
    let service = Runtime::new(repo, "run", native).unwrap();
    service.submit(&req).await.unwrap();
    let s = service.snapshot().await.unwrap().unwrap();
    let text = PLAN
        .replace("src/feature/**", "hello.txt")
        .replace("cargo test", "git status --porcelain");
    service
        .submit(&command(&s, "import", Command::ImportPlan { text }))
        .await
        .unwrap();
    let s = service.snapshot().await.unwrap().unwrap();
    let digest = s.plan.as_ref().unwrap().confirmation_digest.clone();
    service
        .submit(&command(
            &s,
            "approve",
            Command::ApprovePlan {
                confirmation_digest: digest.clone(),
            },
        ))
        .await
        .unwrap();
    let s = service.snapshot().await.unwrap().unwrap();
    let start = command(
        &s,
        "execute",
        Command::StartExecution {
            confirmation_digest: digest,
        },
    );
    let receipt = service.submit(&start).await.unwrap();
    let s = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            let s = service.snapshot().await.unwrap().unwrap();
            if matches!(s.state, RunState::AwaitingMerge | RunState::Blocked) {
                break s;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        s.state,
        RunState::AwaitingMerge,
        "{:?} {:?}",
        s.execution,
        service.worker_error()
    );
    assert_eq!(
        s.execution.as_ref().unwrap().tasks["T1"].state,
        tokage_orchestration::TaskState::Done
    );
    let w = s.work.as_ref().unwrap();
    let a = &w.attempts[s.execution.as_ref().unwrap().tasks["T1"]
        .attempts
        .last()
        .unwrap()];
    assert!(a.commit.is_some());
    assert!(a.integrated_head.is_some());
    assert!(a.report.as_ref().is_some_and(|r| !r.is_locally_generated));
    assert!(a
        .gates
        .iter()
        .all(|g| g.cleanup_confirmed && g.result.as_ref().unwrap().passed));
    assert_eq!(git(&cwd, &["rev-parse", "HEAD"]), original);
    assert_eq!(
        std::fs::read_to_string(cwd.join("hello.txt")).unwrap(),
        "before\n"
    );
    assert_eq!(service.submit(&start).await.unwrap(), receipt);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("calls.jsonl"))
            .unwrap()
            .lines()
            .count(),
        2
    );
}

#[tokio::test]
async fn parallel_native_tasks_use_separate_worktrees_and_one_scheduler() {
    let (dir, cwd, mut req) = fixture();
    std::fs::write(cwd.join("other.txt"), "before\n").unwrap();
    git(&cwd, &["add", "other.txt"]);
    git(
        &cwd,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-m",
            "second file",
        ],
    );
    if let Command::CreateRun { config, .. } = &mut req.command {
        config.target_identity = git(&cwd, &["rev-parse", "HEAD"]);
        config.scope.push("other.txt".into());
        config.concurrency = 2;
    }
    std::fs::write(dir.path().join("mode.txt"), "parallel").unwrap();
    let repo = SqliteRepository::open(&dir.path().join("db"), "run").unwrap();
    let native = NativePlanner::new(
        tokage_native::env::ShellEnv {
            vars: std::env::vars().collect(),
        },
        &dir.path().join("native"),
    )
    .unwrap();
    let service = Runtime::new(repo, "run", native).unwrap();
    service.submit(&req).await.unwrap();
    let task1 = PLAN
        .replace("src/feature/**", "hello.txt")
        .replace("cargo test", "git status --porcelain");
    let second = task1
        .split("```yaml\n")
        .nth(1)
        .unwrap()
        .split("```\n")
        .next()
        .unwrap()
        .replace("T1", "T2")
        .replace("hello.txt", "other.txt");
    let text = task1.replace("```\n", &format!("{second}```\n"));
    let s = service.snapshot().await.unwrap().unwrap();
    service
        .submit(&command(&s, "import", Command::ImportPlan { text }))
        .await
        .unwrap();
    let s = service.snapshot().await.unwrap().unwrap();
    let digest = s.plan.as_ref().unwrap().confirmation_digest.clone();
    service
        .submit(&command(
            &s,
            "approve",
            Command::ApprovePlan {
                confirmation_digest: digest.clone(),
            },
        ))
        .await
        .unwrap();
    let s = service.snapshot().await.unwrap().unwrap();
    service
        .submit(&command(
            &s,
            "execute",
            Command::StartExecution {
                confirmation_digest: digest,
            },
        ))
        .await
        .unwrap();
    let s = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            let s = service.snapshot().await.unwrap().unwrap();
            if matches!(s.state, RunState::AwaitingMerge | RunState::Blocked) {
                break s;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        s.state,
        RunState::AwaitingMerge,
        "{:?} {:?}",
        s.execution,
        service.worker_error()
    );
    let events = std::fs::read_to_string(dir.path().join("timeline.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(events[0]["kind"], "start");
    assert_eq!(events[1]["kind"], "start");
    assert_ne!(events[0]["cwd"], events[1]["cwd"]);
    assert_eq!(
        s.execution
            .as_ref()
            .unwrap()
            .tasks
            .values()
            .filter(|t| t.state == tokage_orchestration::TaskState::Done)
            .count(),
        2
    );
    assert_eq!(
        std::fs::read_to_string(cwd.join("hello.txt")).unwrap(),
        "before\n"
    );
    assert_eq!(
        std::fs::read_to_string(cwd.join("other.txt")).unwrap(),
        "before\n"
    );
}

#[tokio::test]
async fn in_window_repair_reuses_native_thread_and_has_a_bound() {
    let gate = "python3 -c \"assert open('hello.txt').read().strip() == 'after'\"";
    let (dir, _cwd, service, start) = prepared("repair-once", gate, true).await;
    service.submit(&start).await.unwrap();
    let s = settled(&service).await;
    assert_eq!(s.state, RunState::AwaitingMerge, "{:?}", s.execution);
    assert_eq!(s.execution.as_ref().unwrap().tasks["T1"].attempts.len(), 2);
    let calls = std::fs::read_to_string(dir.path().join("calls.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0]["cwd"], calls[1]["cwd"]);
    assert!(calls[1]["args"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a == "resume"));
    let (dir, _cwd, service, start) = prepared("normal", "false", true).await;
    service.submit(&start).await.unwrap();
    let s = settled(&service).await;
    assert_eq!(s.state, RunState::Blocked);
    assert_eq!(s.execution.as_ref().unwrap().tasks["T1"].attempts.len(), 2);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("calls.jsonl"))
            .unwrap()
            .lines()
            .count(),
        2
    );
}

#[tokio::test]
async fn missing_cost_after_completed_task_retains_run_error_in_durable_snapshot() {
    let (dir, _cwd, mut req) = fixture();
    if let Command::CreateRun { config, .. } = &mut req.command {
        config.block_unknown_cost = true;
    }
    let repo = SqliteRepository::open(&dir.path().join("db"), "run").unwrap();
    let native = NativePlanner::new(
        tokage_native::env::ShellEnv {
            vars: std::env::vars().collect(),
        },
        &dir.path().join("native"),
    )
    .unwrap();
    let service = Runtime::new(repo, "run", native).unwrap();
    service.submit(&req).await.unwrap();
    let s = service.snapshot().await.unwrap().unwrap();
    let text = PLAN
        .replace("src/feature/**", "hello.txt")
        .replace("cargo test", "git status --porcelain");
    service
        .submit(&command(&s, "import", Command::ImportPlan { text }))
        .await
        .unwrap();
    let s = service.snapshot().await.unwrap().unwrap();
    let digest = s.plan.as_ref().unwrap().confirmation_digest.clone();
    service
        .submit(&command(
            &s,
            "approve",
            Command::ApprovePlan {
                confirmation_digest: digest.clone(),
            },
        ))
        .await
        .unwrap();
    let s = service.snapshot().await.unwrap().unwrap();
    let start = command(
        &s,
        "execute",
        Command::StartExecution {
            confirmation_digest: digest,
        },
    );
    service.submit(&start).await.unwrap();
    let s = settled(&service).await;
    assert_eq!(s.state, RunState::Blocked);
    assert_eq!(
        s.execution.as_ref().unwrap().tasks["T1"].state,
        tokage_orchestration::TaskState::Done
    );
    assert!(s
        .run_error
        .as_deref()
        .unwrap()
        .contains("unknown actual turn cost"));
    assert!(s.delivery.is_none());
    let connection = rusqlite::Connection::open(dir.path().join("db/tokage.db")).unwrap();
    let stored: String = connection
        .query_row(
            "SELECT snapshot FROM runtime_runs WHERE run_id = 'run'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let replay: Snapshot = serde_json::from_str(&stored).unwrap();
    assert_eq!(replay.run_error, s.run_error);
    let (retry, _) = transition(
        Some(&s),
        &command(
            &s,
            "prepare-after-cost-block",
            Command::PrepareRetry {
                confirmation_digest: s.plan.as_ref().unwrap().confirmation_digest.clone(),
            },
        ),
    )
    .unwrap();
    assert_eq!(retry.state, RunState::Approved);
    assert!(retry.run_error.is_none());
    let mut old = serde_json::to_value(&s).unwrap();
    old.as_object_mut().unwrap().remove("runError");
    assert!(serde_json::from_value::<Snapshot>(old)
        .unwrap()
        .run_error
        .is_none());
}
