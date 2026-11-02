#![cfg(unix)]
mod common;
#[path = "common/native.rs"]
mod native;
use common::*;
use native::*;
use tokage_runtime::*;
use tokage_runtime::{owned::Runtime, repository::SqliteRepository, service::NativePlanner};
#[tokio::test]
async fn scheduler_preserves_launch_identity_and_opens_fresh_for_different_scopes() {
    let (dir, cwd, mut req) = fixture();
    std::fs::write(cwd.join("other.txt"), "before\n").unwrap();
    git(&cwd, &["add", "other.txt"]);
    git(&cwd, &["commit", "-m", "second scoped file"]);
    if let Command::CreateRun { config, .. } = &mut req.command {
        config.target_identity = git(&cwd, &["rev-parse", "HEAD"]);
        config.scope.push("other.txt".into());
        config.context_policy = "auto".into();
        config.context_options = Some(ContextOptions {
            tuning: None,
            cache_aware: true,
            min_saving: 0.,
        });
        config.concurrency = 1;
    }
    std::fs::write(dir.path().join("mode.txt"), "reuse-window").unwrap();
    let repo = SqliteRepository::open(&dir.path().join("db"), "run").unwrap();
    let planner = NativePlanner::new(
        tokage_native::env::ShellEnv {
            vars: std::env::vars().collect(),
        },
        &dir.path().join("native"),
    )
    .unwrap();
    let service = Runtime::new(repo, "run", planner).unwrap();
    service.submit(&req).await.unwrap();
    let text = PLAN
        .replace("src/feature/**", "hello.txt")
        .replace("cargo test", "git status --porcelain");
    let second = text
        .split("```yaml\n")
        .nth(1)
        .unwrap()
        .split("```\n")
        .next()
        .unwrap()
        .replace("T1", "T2")
        .replace("hello.txt", "other.txt")
        .replace("  domain:", "  depends_on: [T1]\n  domain:");
    let text = text.replace("```\n", &format!("{second}```\n"));
    let s = service.snapshot().await.unwrap().unwrap();
    service
        .submit(&command(&s, "import", Command::ImportPlan { text }))
        .await
        .unwrap();
    let s = service.snapshot().await.unwrap().unwrap();
    service
        .submit(&command(
            &s,
            "approve",
            Command::ApprovePlan {
                confirmation_digest: s.plan.as_ref().unwrap().confirmation_digest.clone(),
            },
        ))
        .await
        .unwrap();
    let s = service.snapshot().await.unwrap().unwrap();
    service
        .submit(&command(
            &s,
            "start",
            Command::StartExecution {
                confirmation_digest: s.approved_digest.clone().unwrap(),
            },
        ))
        .await
        .unwrap();
    let s = settled(&service).await;
    assert_eq!(s.state, RunState::AwaitingMerge, "{:?}", s.run_error);
    let tasks = &s.execution.as_ref().unwrap().tasks;
    let t1 = &s.turns[tasks["T1"].attempts.last().unwrap()];
    let t2 = &s.turns[tasks["T2"].attempts.last().unwrap()];
    assert_eq!(t1.usage.as_ref().unwrap().input_tokens, 10_000_000);
    assert_eq!(t1.usage.as_ref().unwrap().context_tokens, None);
    assert_eq!(t1.usage.as_ref().unwrap().api_calls, None);
    assert_eq!(t1.context_metrics_version, Some(2));
    let decision = tasks["T2"].context.as_ref().unwrap();
    assert!(!decision.reuse);
    assert!(decision.previous_turn.is_none());
    assert!(decision.reason.contains("compatible"));
    assert_eq!(decision.inputs.as_ref().unwrap()["rejectedCandidates"][0]["reason"],"LAUNCH_IDENTITY_OR_SCOPE_CHANGED");
    let identity = t1.window_identity.as_ref().unwrap();
    assert_eq!(identity.config_hash, s.config_hash);
    assert_eq!(
        identity.plan_digest,
        s.approved_digest.as_ref().unwrap().as_str()
    );
    assert_eq!(
        identity.workspace,
        s.work.as_ref().unwrap().attempts[&t1.id]
            .workspace
            .clone()
            .unwrap()
    );
    let raw = std::fs::read_to_string(dir.path().join("task-prompt-T2.txt")).unwrap();
    // Native role framing is added by the adapter; this saved byte estimate is the exact runtime prompt before that framing.
    assert!(t2.visible_prompt_bytes.unwrap() > 0);
    assert!(raw.contains("Hints for downstream"));
    let calls = std::fs::read_to_string(dir.path().join("calls.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 3);
    assert!(!calls[1]["args"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == "resume"));
    assert_ne!(calls[0]["cwd"], calls[1]["cwd"]);
    let reread = repository::Reader::open(&dir.path().join("db"))
        .unwrap()
        .snapshot("run")
        .unwrap()
        .unwrap();
    assert_eq!(
        reread.execution.unwrap().tasks["T2"].context,
        tasks["T2"].context
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
async fn source_branch_change_starts_fresh_and_keeps_original_artifacts() {
    let (_dir, _cwd, service, start) = prepared("gate-failure", "grep -q '^after$' hello.txt", false).await;
    service.submit(&start).await.unwrap();
    let s = settled(&service).await;
    assert_eq!(s.state, RunState::Blocked);
    while service.busy() {
        tokio::task::yield_now().await;
    }
    let source_id = s.execution.as_ref().unwrap().tasks["T1"]
        .attempts
        .last()
        .unwrap()
        .clone();
    let source = s.work.as_ref().unwrap().attempts[&source_id]
        .workspace
        .clone()
        .unwrap();
    git(
        std::path::Path::new(&source.cwd),
        &["checkout", "-b", "changed-source-branch"],
    );
    service
        .submit(&command(
            &s,
            "follow",
            Command::AppendTaskMessage {
                task_id: "T1".into(),
                confirmation_digest: s.approved_digest.clone().unwrap(),
                message: "Repair within the unchanged task contract".into(),
            },
        ))
        .await
        .unwrap();
    let after = settled(&service).await;
    let tasks = &after.execution.as_ref().unwrap().tasks["T1"];
    assert_eq!(tasks.attempts.len(), 2);
    let next = tasks.attempts.last().unwrap();
    let workspace = after.work.as_ref().unwrap().attempts[next]
        .workspace
        .as_ref()
        .unwrap();
    assert_ne!(workspace.cwd, source.cwd);
    assert!(after.turns[next].window_identity.is_some());
    assert_eq!(
        std::fs::read_to_string(std::path::Path::new(&source.cwd).join("hello.txt")).unwrap(),
        "wrong\n"
    );
    let calls = std::fs::read_to_string(_dir.path().join("calls.jsonl")).unwrap();
    let calls = calls
        .lines()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .collect::<Vec<_>>();
    assert!(!calls.last().unwrap()["args"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == "resume"));
}

#[tokio::test]
async fn resumed_native_thread_change_cannot_pass_acceptance() {
    let (_dir, cwd, service, start) = prepared("resume-thread-changed", "grep -q '^after$' hello.txt", true).await;
    service.submit(&start).await.unwrap();
    let after = settled(&service).await;
    assert_eq!(after.state, RunState::Blocked);
    let task = &after.execution.as_ref().unwrap().tasks["T1"];
    assert_eq!(task.attempts.len(), 2);
    assert!(task
        .error
        .as_ref()
        .unwrap()
        .contains("native resume thread identity changed"));
    let attempt = &after.work.as_ref().unwrap().attempts[task.attempts.last().unwrap()];
    assert!(attempt.commit.is_none());
    assert!(attempt.gates.is_empty());
    let turn=&after.turns[task.attempts.last().unwrap()];
    assert!(turn.window_identity.is_none());
    assert!(turn.window_origin.is_none());
    assert_eq!(
        std::fs::read_to_string(cwd.join("hello.txt")).unwrap(),
        "before\n"
    );
}
