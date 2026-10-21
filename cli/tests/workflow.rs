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
    fn id<'a>(v: &'a Value) -> &'a str {
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
    let state_path = f.data.join("runs").join(id).join("run.json");
    let mut state: Value = serde_json::from_slice(&bytes(&state_path)).unwrap();
    state["model"] = json!("changed-model");
    std::fs::write(&state_path, serde_json::to_vec(&state).unwrap()).unwrap();
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
