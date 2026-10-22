use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};

struct Fixture {
    _temp: tempfile::TempDir,
    repo: PathBuf,
    data: PathBuf,
    requirement: PathBuf,
    plan: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("project");
        std::fs::create_dir(&repo).unwrap();
        let data = temp.path().join("data");
        let requirement = temp.path().join("requirement.txt");
        let plan = temp.path().join("plan.md");
        std::fs::write(
            &requirement,
            "Update hello.txt while preserving unrelated files",
        )
        .unwrap();
        std::fs::write(repo.join("hello.txt"), "before\n").unwrap();
        let git = |args: &[&str]| {
            assert!(Command::new("git")
                .current_dir(&repo)
                .args(args)
                .output()
                .unwrap()
                .status
                .success())
        };
        git(&["init", "-b", "main"]);
        git(&["add", "."]);
        git(&[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "initial",
        ]);
        std::fs::write(&plan, plan_text("git status --porcelain")).unwrap();
        Self {
            _temp: temp,
            repo,
            data,
            requirement,
            plan,
        }
    }
    fn call(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_tokage"))
            .args(["--json", "--data-dir", self.data.to_str().unwrap()])
            .args(args)
            .output()
            .unwrap()
    }
    fn ok(&self, args: &[&str]) -> Value {
        let o = self.call(args);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        serde_json::from_slice(&o.stdout).unwrap()
    }
    fn create(&self) -> Value {
        self.ok(&[
            "run",
            "create",
            "--repo",
            self.repo.to_str().unwrap(),
            "--requirement-file",
            self.requirement.to_str().unwrap(),
            "--provider",
            "CODEX_CLI",
            "--model",
            "ordinary-user-model",
        ])
    }
    fn id(v: &Value) -> &str {
        v["run"]["id"].as_str().unwrap()
    }
}
fn plan_text(command: &str) -> String {
    format!("# Spec\n## Goal\nUpdate hello\n# Contracts\n# Tasks\n```yaml\n- id: T1\n  domain: general\n  title: Update greeting\n  goal: Update hello.txt\n  write_scope: [hello.txt]\n  deliverables: [hello.txt]\n  acceptance:\n    commands: ['{command}']\n    criteria: [Greeting is updated]\n```\n# Notes\nKeep unrelated files.\n")
}
fn bytes(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap()
}
#[test]
fn registration_and_exact_plan_consent_do_not_touch_repo_or_global_identity() {
    let f = Fixture::new();
    let config = bytes(&f.repo.join(".git/config"));
    let head = Command::new("git")
        .current_dir(&f.repo)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap()
        .stdout;
    let created = f.create();
    let id = Fixture::id(&created);
    assert_eq!(created["run"]["status"], "DRAFT");
    let imported = f.ok(&[
        "run",
        "import-plan",
        "--id",
        id,
        "--file",
        f.plan.to_str().unwrap(),
    ]);
    let hash = imported["planSha"].as_str().unwrap();
    assert!(!f
        .call(&[
            "run",
            "approve",
            "--id",
            id,
            "--plan-sha",
            "wrong",
            "--allow-writes",
            "--trust-workspace"
        ])
        .status
        .success());
    assert!(!f
        .call(&["run", "approve", "--id", id, "--plan-sha", hash])
        .status
        .success());
    assert!(!f.call(&["run", "plan", "--id", id]).status.success()); // No workspace trust: never dispatch.
    let approved = f.ok(&[
        "run",
        "approve",
        "--id",
        id,
        "--plan-sha",
        hash,
        "--allow-writes",
        "--trust-workspace",
        "--confirm-command",
        "git status --porcelain",
    ]);
    assert_eq!(approved["run"]["status"], "APPROVED");
    let replaced = f.ok(&[
        "run",
        "import-plan",
        "--id",
        id,
        "--file",
        f.plan.to_str().unwrap(),
    ]);
    assert_eq!(replaced["run"]["status"], "AWAITING_APPROVAL");
    assert!(replaced["run"]["approvedDigest"].is_null());
    assert!(!f.call(&["run", "execute", "--id", id]).status.success());
    assert_eq!(f.ok(&["run", "list"])["runs"].as_array().unwrap().len(), 1);
    assert!(!f
        .call(&["run", "show", "--id", "../project"])
        .status
        .success());
    assert_eq!(bytes(&f.repo.join(".git/config")), config);
    assert_eq!(
        Command::new("git")
            .current_dir(&f.repo)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
        head
    );
    assert_eq!(bytes(&f.repo.join("hello.txt")), b"before\n");
    assert!(!f.repo.join(".tokage").exists());
}
#[test]
fn dirty_repo_nested_data_empty_scopes_and_unsafe_gate_are_rejected() {
    let f = Fixture::new();
    std::fs::write(f.repo.join("untracked"), "user work").unwrap();
    assert!(!f
        .call(&[
            "run",
            "create",
            "--repo",
            f.repo.to_str().unwrap(),
            "--requirement-file",
            f.requirement.to_str().unwrap(),
            "--provider",
            "CODEX_CLI",
            "--model",
            "x"
        ])
        .status
        .success());
    std::fs::remove_file(f.repo.join("untracked")).unwrap();
    let nested = f.repo.join("data");
    let out = Command::new(env!("CARGO_BIN_EXE_tokage"))
        .args([
            "--data-dir",
            nested.to_str().unwrap(),
            "run",
            "create",
            "--repo",
            f.repo.to_str().unwrap(),
            "--requirement-file",
            f.requirement.to_str().unwrap(),
            "--provider",
            "CODEX_CLI",
            "--model",
            "x",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(!nested.exists());
    let created = f.create();
    let id = Fixture::id(&created);
    std::fs::write(
        &f.plan,
        plan_text("git status").replace("write_scope: [hello.txt]", "write_scope: []"),
    )
    .unwrap();
    assert!(!f
        .call(&[
            "run",
            "import-plan",
            "--id",
            id,
            "--file",
            f.plan.to_str().unwrap()
        ])
        .status
        .success());
    std::fs::write(&f.plan, plan_text("echo ok; echo unsafe")).unwrap();
    assert!(!f
        .call(&[
            "run",
            "import-plan",
            "--id",
            id,
            "--file",
            f.plan.to_str().unwrap()
        ])
        .status
        .success());
}
#[test]
fn consent_hash_covers_model_and_requirement_changes() {
    let f = Fixture::new();
    let created = f.create();
    let id = Fixture::id(&created);
    let imported = f.ok(&[
        "run",
        "import-plan",
        "--id",
        id,
        "--file",
        f.plan.to_str().unwrap(),
    ]);
    let sha = imported["planSha"].as_str().unwrap();
    let state_path = f._temp.path().join("config.json");
    let mut config = imported["run"]["config"].clone();
    config["models"]["executor"]["requestedModel"] = json!("changed-model");
    std::fs::write(&state_path, serde_json::to_vec(&config).unwrap()).unwrap();
    f.ok(&[
        "run",
        "update-config",
        "--id",
        id,
        "--file",
        state_path.to_str().unwrap(),
    ]);
    assert!(!f
        .call(&[
            "run",
            "approve",
            "--id",
            id,
            "--plan-sha",
            sha,
            "--allow-writes",
            "--trust-workspace"
        ])
        .status
        .success());
}

#[cfg(unix)]
impl Fixture {
    fn provider(&self, mode: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let directory = self._temp.path().join("provider");
        std::fs::create_dir_all(&directory).unwrap();
        let exe = directory.join("codex");
        std::fs::write(&exe, include_str!("fixtures/codex.py")).unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(directory.join("mode.txt"), mode).unwrap();
        std::fs::write(
            directory.join("plan.md"),
            plan_text("git status --porcelain"),
        )
        .unwrap();
        exe
    }
    fn create_with_provider(&self, exe: &Path) -> Value {
        self.ok(&[
            "run",
            "create",
            "--repo",
            self.repo.to_str().unwrap(),
            "--requirement-file",
            self.requirement.to_str().unwrap(),
            "--provider",
            "CODEX_CLI",
            "--model",
            "ordinary-user-model",
            "--provider-path",
            exe.to_str().unwrap(),
        ])
    }
    fn approve(&self, id: &str) {
        let imported = self.ok(&[
            "run",
            "import-plan",
            "--id",
            id,
            "--file",
            self.plan.to_str().unwrap(),
        ]);
        self.ok(&[
            "run",
            "approve",
            "--id",
            id,
            "--plan-sha",
            imported["planSha"].as_str().unwrap(),
            "--allow-writes",
            "--trust-workspace",
            "--confirm-command",
            "git status --porcelain",
        ]);
    }
}
#[cfg(unix)]
#[test]
fn offline_executor_uses_worktree_and_keeps_main_untouched() {
    let f = Fixture::new();
    let exe = f.provider("ok");
    let created = f.create_with_provider(&exe);
    let id = Fixture::id(&created);
    f.approve(id);
    let executed = f.ok(&["run", "execute", "--id", id]);
    assert!(executed["run"]["completedTasks"]["T1"].is_string());
    assert_eq!(bytes(&f.repo.join("hello.txt")), b"before\n");
    let integration = Path::new(executed["run"]["integration"]["cwd"].as_str().unwrap());
    assert_eq!(bytes(&integration.join("hello.txt")), b"after\n");
    let events = f.ok(&["run", "events", "--id", id]);
    let events = events["events"].as_array().unwrap();
    assert!(events.iter().any(|e| e["kind"] == "ExecutionProgress"));
    assert!(events.iter().any(|e| e["kind"] == "TurnObserved"));
    let turn = executed["run"]["turns"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap();
    let transcript = f.ok(&["run", "transcript", "--id", id, "--turn", turn]);
    assert!(transcript["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["event"]["kind"] == "done"));
    let calls = std::fs::read_to_string(exe.parent().unwrap().join("calls.jsonl")).unwrap();
    assert!(calls.contains("sandbox_mode=\\\"workspace-write\\\""));
}
#[cfg(unix)]
#[test]
fn native_failure_scope_violation_missing_done_and_deviation_never_accept_task() {
    for mode in ["nonzero", "out-of-scope", "missing-done", "deviation"] {
        let f = Fixture::new();
        let exe = f.provider(mode);
        let created = f.create_with_provider(&exe);
        let id = Fixture::id(&created);
        f.approve(id);
        let output = f.call(&["run", "execute", "--id", id]);
        assert!(!output.status.success(), "mode {mode}");
        let run = f.ok(&["run", "show", "--id", id]);
        assert_eq!(run["run"]["status"], "BLOCKED");
        let reason = run["run"]["error"].as_str().unwrap();
        match mode {
            "out-of-scope" => assert!(reason.contains("scope"), "{reason}"),
            "deviation" => assert!(reason.contains("contract deviation"), "{reason}"),
            "nonzero" => assert!(reason.contains("offline failure"), "{reason}"),
            _ => assert!(!reason.contains("outside its checkout"), "{reason}"),
        }
        assert_eq!(run["run"]["completedTasks"], json!({}));
        assert_eq!(bytes(&f.repo.join("hello.txt")), b"before\n");
    }
}

#[cfg(unix)]
#[test]
fn full_offline_flow_requires_exact_explicit_merge() {
    let f = Fixture::new();
    let exe = f.provider("ok");
    let created = f.create_with_provider(&exe);
    let id = Fixture::id(&created);
    f.approve(id);
    let executed = f.ok(&["run", "execute", "--id", id]);
    assert_eq!(executed["run"]["status"], "AWAITING_MERGE");
    assert_eq!(bytes(&f.repo.join("hello.txt")), b"before\n");
    let source = executed["run"]["accepted"]["head"].as_str().unwrap();
    let target = executed["run"]["target"]["head"].as_str().unwrap();
    assert!(!f
        .call(&["run", "merge", "--id", id, "--source", source, "--target", target])
        .status
        .success());
    assert!(!f
        .call(&[
            "run",
            "merge",
            "--id",
            id,
            "--source",
            "wrong",
            "--target",
            target,
            "--confirm"
        ])
        .status
        .success());
    let merged = f.ok(&[
        "run",
        "merge",
        "--id",
        id,
        "--source",
        source,
        "--target",
        target,
        "--confirm",
    ]);
    assert_eq!(merged["run"]["status"], "DELIVERED");
    assert_eq!(bytes(&f.repo.join("hello.txt")), b"after\n");
    let calls = std::fs::read_to_string(exe.parent().unwrap().join("calls.jsonl")).unwrap();
    assert!(calls.contains("read-only"));
}
#[cfg(unix)]
#[test]
fn failed_or_writing_review_blocks_delivery() {
    for mode in ["review-failure", "review-write"] {
        let f = Fixture::new();
        let exe = f.provider(mode);
        let created = f.create_with_provider(&exe);
        let id = Fixture::id(&created);
        f.approve(id);
        assert!(!f.call(&["run", "execute", "--id", id]).status.success());
        let run = f.ok(&["run", "show", "--id", id]);
        assert_eq!(run["run"]["status"], "BLOCKED");
        let reason = run["run"]["error"].as_str().unwrap();
        assert!(
            reason.contains(if mode == "review-failure" {
                "review did not pass"
            } else {
                "read-only"
            }),
            "{reason}"
        );
        assert!(run["run"]["accepted"].is_null());
        assert_eq!(bytes(&f.repo.join("hello.txt")), b"before\n");
    }
}
#[cfg(unix)]
#[test]
fn dirty_target_changed_target_and_changed_source_reject_stale_delivery() {
    for case in ["dirty-target", "changed-target", "changed-source"] {
        let f = Fixture::new();
        let exe = f.provider("ok");
        let created = f.create_with_provider(&exe);
        let id = Fixture::id(&created);
        f.approve(id);
        let executed = f.ok(&["run", "execute", "--id", id]);
        let source = executed["run"]["accepted"]["head"].as_str().unwrap();
        let target = executed["run"]["target"]["head"].as_str().unwrap();
        if case == "dirty-target" {
            std::fs::write(f.repo.join("user.txt"), "user work").unwrap();
        } else if case == "changed-target" {
            assert!(Command::new("git")
                .current_dir(&f.repo)
                .args([
                    "-c",
                    "user.name=Fixture",
                    "-c",
                    "user.email=fixture@example.invalid",
                    "-c",
                    "commit.gpgsign=false",
                    "commit",
                    "--allow-empty",
                    "-m",
                    "user advance"
                ])
                .output()
                .unwrap()
                .status
                .success());
        } else {
            let integration = Path::new(executed["run"]["integration"]["cwd"].as_str().unwrap());
            std::fs::write(integration.join("hello.txt"), "changed after acceptance\n").unwrap();
        }
        assert!(
            !f.call(&[
                "run",
                "merge",
                "--id",
                id,
                "--source",
                source,
                "--target",
                target,
                "--confirm"
            ])
            .status
            .success(),
            "{case}"
        );
        assert_eq!(bytes(&f.repo.join("hello.txt")), b"before\n");
    }
}

#[cfg(unix)]
#[test]
fn blocked_execution_requires_explicit_retry_and_preserves_failed_attempt() {
    let f = Fixture::new();
    let exe = f.provider("nonzero");
    let created = f.create_with_provider(&exe);
    let id = Fixture::id(&created);
    f.approve(id);
    assert!(!f.call(&["run", "execute", "--id", id]).status.success());
    assert!(!f.call(&["run", "retry", "--id", id]).status.success());
    assert!(!f.call(&["run", "execute", "--id", id]).status.success());
    f.provider("ok");
    let retry = f.ok(&["run", "retry", "--id", id, "--confirm"]);
    assert_eq!(retry["run"]["status"], "APPROVED");
    let executed = f.ok(&["run", "execute", "--id", id]);
    assert_eq!(executed["run"]["status"], "AWAITING_MERGE");
    assert_eq!(executed["run"]["sequence"], 2);
    let events = f.ok(&["run", "events", "--id", id]);
    assert!(events["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["kind"] == "ExecutionProgress"));
}
#[cfg(unix)]
#[test]
fn cancel_active_native_turn_then_retry_without_overlapping_owner() {
    use std::{
        thread,
        time::{Duration, Instant},
    };
    let f = Fixture::new();
    let exe = f.provider("hang");
    let created = f.create_with_provider(&exe);
    let id = Fixture::id(&created);
    f.approve(id);
    let mut owner = Command::new(env!("CARGO_BIN_EXE_tokage"))
        .args([
            "--json",
            "--data-dir",
            f.data.to_str().unwrap(),
            "run",
            "execute",
            "--id",
            id,
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !exe.parent().unwrap().join("calls.jsonl").exists() {
        if Instant::now() > deadline {
            owner.kill().unwrap();
            panic!("fixture never launched");
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(!f
        .call(&["run", "retry", "--id", id, "--confirm"])
        .status
        .success());
    let cancellation = f.ok(&["run", "cancel", "--id", id]);
    assert_eq!(cancellation["completionConfirmed"], false);
    loop {
        if let Some(status) = owner.try_wait().unwrap() {
            assert!(!status.success());
            break;
        }
        if Instant::now() > deadline {
            owner.kill().unwrap();
            panic!("cancel did not terminate owner");
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        f.ok(&["run", "show", "--id", id])["run"]["status"],
        "CANCELLED"
    );
    f.provider("ok");
    f.ok(&["run", "retry", "--id", id, "--confirm"]);
    assert_eq!(
        f.ok(&["run", "execute", "--id", id])["run"]["status"],
        "AWAITING_MERGE"
    );
    assert_eq!(bytes(&f.repo.join("hello.txt")), b"before\n");
}
#[cfg(unix)]
#[test]
fn explicit_planning_stream_and_lost_delivery_acknowledgement_are_recoverable() {
    let f = Fixture::new();
    let exe = f.provider("ok");
    let created = f.create_with_provider(&exe);
    let id = Fixture::id(&created);
    let planned = f.ok(&["run", "plan", "--id", id, "--trust-workspace"]);
    assert_eq!(planned["run"]["status"], "AWAITING_APPROVAL");
    f.ok(&[
        "run",
        "approve",
        "--id",
        id,
        "--plan-sha",
        planned["planSha"].as_str().unwrap(),
        "--allow-writes",
        "--trust-workspace",
        "--confirm-command",
        "git status --porcelain",
    ]);
    let accepted = f.ok(&["run", "execute", "--id", id]);
    let source = accepted["run"]["accepted"]["head"].as_str().unwrap();
    let target = accepted["run"]["target"]["head"].as_str().unwrap();
    // Offline SQL trigger simulates only the lost acknowledgement, through the real CLI.
    let db = rusqlite::Connection::open(f.data.join("runtime-v1/tokage.db")).unwrap();
    db.execute_batch("CREATE TRIGGER lose_delivery BEFORE UPDATE ON runtime_runs WHEN json_extract(NEW.snapshot,'$.state')='Delivered' BEGIN SELECT RAISE(ABORT,'lost delivery acknowledgement'); END;").unwrap();
    assert!(!f
        .call(&[
            "run",
            "merge",
            "--id",
            id,
            "--source",
            source,
            "--target",
            target,
            "--confirm"
        ])
        .status
        .success());
    let merged_head = Command::new("git")
        .current_dir(&f.repo)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap()
        .stdout;
    assert_eq!(bytes(&f.repo.join("hello.txt")), b"after\n");
    db.execute_batch("DROP TRIGGER lose_delivery").unwrap();
    assert_eq!(
        f.ok(&["run", "resume", "--id", id])["run"]["status"],
        "DELIVERED"
    );
    assert_eq!(
        Command::new("git")
            .current_dir(&f.repo)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
        merged_head
    );
}

#[test]
fn runtime_pages_and_idempotent_commands_use_one_authority_and_history_never_executes() {
    let f = Fixture::new();
    let created = f.create();
    let id = Fixture::id(&created);
    let request = f._temp.path().join("command.json");
    std::fs::write(&request,serde_json::to_vec(&json!({"schemaVersion":1,"commandId":"explicit-import","runId":id,"expectedRevision":created["run"]["revision"],"command":{"type":"ImportPlan","payload":{"text":std::fs::read_to_string(&f.plan).unwrap()}}})).unwrap()).unwrap();
    let one = f.ok(&["run", "submit", "--file", request.to_str().unwrap()]);
    let two = f.ok(&["run", "submit", "--file", request.to_str().unwrap()]);
    assert_eq!(one["receipt"], two["receipt"]);
    assert_eq!(one["snapshot"], two["snapshot"]);
    let page = f.ok(&["run", "events", "--id", id, "--after", "0", "--limit", "1"]);
    assert_eq!(page["events"].as_array().unwrap().len(), 1);
    assert_eq!(page["events"][0]["sequence"], 1);
    assert!(!f
        .call(&["run", "events", "--id", id, "--limit", "1001"])
        .status
        .success());
    assert!(!f.data.join("runs").join(id).join("run.json").exists());
    let legacy = f._temp.path().join("legacy.json");
    let backup = f._temp.path().join("backup.json");
    let original = json!({"schemaVersion":1,"id":"legacy","status":"EXECUTING","requirement":"old","repo":f.repo});
    std::fs::write(&legacy, serde_json::to_vec(&original).unwrap()).unwrap();
    std::fs::copy(&legacy, &backup).unwrap();
    let archived = f.ok(&[
        "run",
        "import-history",
        "--id",
        "legacy",
        "--file",
        legacy.to_str().unwrap(),
        "--backup",
        backup.to_str().unwrap(),
    ]);
    assert_eq!(archived["executable"], false);
    assert!(!f
        .call(&["run", "execute", "--id", "legacy"])
        .status
        .success());
    assert_eq!(f.ok(&["run", "list"])["runs"].as_array().unwrap().len(), 1);
    assert_eq!(bytes(&f.repo.join("hello.txt")), b"before\n");
}
