#![allow(dead_code)]
use crate::common::{command, create, PLAN};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command as Process,
};
use tokage_runtime::{owned::Runtime, repository::SqliteRepository, service::NativePlanner, *};
pub fn git(cwd: &Path, args: &[&str]) -> String {
    let o = Process::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().into()
}
pub fn fixture() -> (tempfile::TempDir, PathBuf, Request) {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().join("repo");
    std::fs::create_dir(&cwd).unwrap();
    git(&cwd, &["init"]);
    std::fs::write(cwd.join("hello.txt"), "before\n").unwrap();
    git(&cwd, &["add", "hello.txt"]);
    git(
        &cwd,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-m",
            "fixture",
        ],
    );
    let provider = dir.path().join("codex");
    std::fs::write(&provider, include_str!("../fixtures/codex-runtime.py")).unwrap();
    std::fs::set_permissions(&provider, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(dir.path().join("mode.txt"), "normal").unwrap();
    let mut req = create();
    if let Command::CreateRun { config, .. } = &mut req.command {
        config.workspace = cwd.to_string_lossy().into();
        config.target_identity = git(&cwd, &["rev-parse", "HEAD"]);
        config.target_reference = Some(git(&cwd, &["rev-parse", "--symbolic-full-name", "HEAD"]));
        config.permission_mode = "full-auto".into();
        config.scope = vec!["hello.txt".into()];
        config.block_unknown_cost = false;
        config.confirmed_commands = vec!["git status --porcelain".into()];
        config.models.get_mut("executor").unwrap().provider_path =
            Some(provider.to_string_lossy().into());
    }
    (dir, cwd, req)
}
pub async fn prepared(
    mode: &str,
    gate: &str,
    repair: bool,
) -> (tempfile::TempDir, PathBuf, Runtime<NativePlanner>, Request) {
    let (dir, cwd, mut req) = fixture();
    std::fs::write(dir.path().join("mode.txt"), mode).unwrap();
    if let Command::CreateRun { config, .. } = &mut req.command {
        config.confirmed_commands = vec![gate.into()];
        if repair {
            config.repair_policy = "bounded-ladder".into();
        }
    }
    if mode == "phase-approval" {
        if let Command::CreateRun { requirement, .. } = &mut req.command {
            *requirement = "Plan first; do not execute before approval. Preserve frozen tests.".into();
        }
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
    let text = PLAN.replace("src/feature/**", "hello.txt").replace(
        "commands: [cargo test]",
        &format!("commands: [{}]", serde_json::to_string(gate).unwrap()),
    );
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
    let start = command(
        &s,
        "execute",
        Command::StartExecution {
            confirmation_digest: digest,
        },
    );
    (dir, cwd, service, start)
}
pub async fn settled(service: &Runtime<NativePlanner>) -> Snapshot {
    let result = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            let s = service.snapshot().await.unwrap().unwrap();
            if matches!(
                s.state,
                RunState::AwaitingMerge
                    | RunState::Blocked
                    | RunState::Delivered
                    | RunState::AwaitingReview
            ) {
                return s;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await;
    match result {
        Ok(s) => s,
        Err(_) => {
            let s = service.snapshot().await.unwrap().unwrap();
            panic!(
                "state {:?} revision {} execution {:?} turns {:?} delivery {:?} worker {:?}",
                s.state,
                s.revision,
                s.execution,
                s.turns,
                s.delivery,
                service.worker_error()
            );
        }
    }
}
