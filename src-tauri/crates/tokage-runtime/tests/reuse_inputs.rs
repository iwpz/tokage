#![cfg(unix)]
mod common;
#[path = "common/native.rs"]
mod native;
use common::*;
use native::*;
use tokage_runtime::*;
use tokage_runtime::{owned::Runtime, repository::SqliteRepository, service::NativePlanner};
#[tokio::test]
async fn scheduler_persists_actual_brief_and_refuses_unverified_aggregate_context_through_public_execution(
) {
    let (dir, cwd, mut req) = fixture();
    std::fs::write(cwd.join("other.txt"), "before\n").unwrap();
    git(&cwd, &["add", "other.txt"]);
    git(&cwd, &["commit", "-m", "second scoped file"]);
    if let Command::CreateRun { config, .. } = &mut req.command {
        config.target_identity = git(&cwd, &["rev-parse", "HEAD"]);
        config.scope.push("other.txt".into());
        config.context_policy = "auto".into();
        config.context_options = Some(ContextOptions {
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
    assert_eq!(decision.previous_turn.as_deref(), Some(t1.id.as_str()));
    assert!(decision.reason.contains("unverified"));
    let inputs = decision.inputs.as_ref().unwrap();
    assert_eq!(
        inputs["sources"]["totalTokens"],
        "VISIBLE_CONTENT_ESTIMATE_PLUS_PROVIDER_BASELINE"
    );
    assert!(inputs["historyTokensA"].as_f64().unwrap() < 100_000.);
    let raw = std::fs::read_to_string(dir.path().join("task-prompt-T2.txt")).unwrap();
    // Native role framing is added by the adapter; this saved byte estimate is the exact runtime prompt before that framing.
    assert_eq!(
        inputs["briefTokensB"].as_f64().unwrap(),
        t2.visible_prompt_bytes.unwrap() as f64 / 4.
    );
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
