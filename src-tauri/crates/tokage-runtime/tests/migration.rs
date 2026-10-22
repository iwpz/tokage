mod common;
use common::create;
use tokage_runtime::{host, repository::SqliteRepository, *};
#[tokio::test]
async fn legacy_ids_never_switch_engines_and_fresh_runs_do_not_rewrite_history() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("repo");
    let legacy = workspace.join(".tokage/execution");
    std::fs::create_dir_all(&legacy).unwrap();
    let original = b"{\"schemaVersion\":99,\"status\":\"RUNNING\"}";
    std::fs::write(legacy.join("run.json"), original).unwrap();
    let database = dir.path().join("data/runtime-v1");
    let mut req = create();
    if let Command::CreateRun { config, .. } = &mut req.command {
        config.workspace = workspace.to_string_lossy().into();
    }
    let service = host::open(
        &database,
        "run",
        tokage_native::env::ShellEnv {
            vars: Default::default(),
        },
    )
    .unwrap();
    assert!(service.admit_request(&req).await.is_err());
    assert!(service.submit(&req).await.is_err());
    assert!(service.snapshot().await.unwrap().is_none());
    drop(service);
    let mut repo = SqliteRepository::open(&database, "run").unwrap();
    assert!(repo.submit(&req).await.is_err());
    drop(repo);
    req.run_id = "fresh".into();
    req.command_id = "fresh-create".into();
    let service = host::open(
        &database,
        "fresh",
        tokage_native::env::ShellEnv {
            vars: Default::default(),
        },
    )
    .unwrap();
    service.submit(&req).await.unwrap();
    assert_eq!(
        service.snapshot().await.unwrap().unwrap().state,
        RunState::Draft
    );
    let fresh = service.snapshot().await.unwrap().unwrap();
    service
        .submit(&common::command(
            &fresh,
            "ordinary-import",
            Command::ImportPlan {
                text: common::PLAN.into(),
            },
        ))
        .await
        .unwrap();
    let imported = service.snapshot().await.unwrap().unwrap();
    assert_eq!(imported.state, RunState::AwaitingApproval);
    assert!(imported.approved_digest.is_none());

    assert_eq!(std::fs::read(legacy.join("run.json")).unwrap(), original);
}
#[tokio::test]
async fn old_cli_records_and_archived_ids_remain_non_executable_even_via_raw_repository() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let old = data.join("runs/run");
    std::fs::create_dir_all(&old).unwrap();
    std::fs::write(old.join("run.json"), "unknown old record").unwrap();
    let database = data.join("runtime-v1");
    let mut repo = SqliteRepository::open(&database, "run").unwrap();
    assert!(repo.submit(&create()).await.is_err());
    assert!(repo.snapshot("run").unwrap().is_none());
    std::fs::remove_file(old.join("run.json")).unwrap();
    let original = include_str!("fixtures/cli-v1.json");
    let backup = dir.path().join("backup.json");
    std::fs::write(&backup, original).unwrap();
    repo.import_history("cli-v1", "run", original, &backup)
        .await
        .unwrap();
    assert!(repo.submit(&create()).await.is_err());
    assert!(repo.snapshot("run").unwrap().is_none());
    assert_eq!(repo.history("cli-v1", "run").unwrap().unwrap(), original);
}
#[tokio::test]
async fn app_database_legacy_plan_identity_is_reserved_without_a_pipeline_file() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let storage = tokage_storage::Storage::open(&data).unwrap();
    {
        let conn = storage.get_conn().unwrap();
        conn.execute_batch("INSERT INTO projects(id,name,local_path) VALUES('p','project','/old-project'); INSERT INTO main_sessions(id,project_id,title) VALUES('run','p','old'); INSERT INTO plans(main_session_id,goal,spec_json,status) VALUES('run','old goal','{}','RUNNING');").unwrap();
    }
    let mut repo = SqliteRepository::open(&data.join("runtime-v1"), "run").unwrap();
    assert!(repo.submit(&create()).await.is_err());
    assert!(repo.snapshot("run").unwrap().is_none());
    let conn = storage.get_conn().unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT status FROM plans WHERE main_session_id='run'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "RUNNING"
    );
}
