use rusqlite::Connection;
use tokage_runtime::{repository::SqliteRepository, *};
fn request() -> Request {
    serde_json::from_value(serde_json::json!({"schemaVersion":1,"commandId":"create","runId":"run","expectedRevision":0,
        "command":{"type":"CreateRun","payload":{"requirement":"implement feature","config":{
            "source":"cli","policyVersion":"1","workspace":"/repo","targetIdentity":"sha",
            "scope":["src"],"models":{"executor":{"provider":"codex","requestedModel":"model","providerPath":null,"capabilities":["read"],"userLocked":true}},"commandPolicyHash":"policy-hash","trustWorkspace":false,
            "permissionMode":"read-only","concurrency":1,"budgetUsd":null,"blockUnknownCost":true,
            "timeoutSeconds":60,"repairPolicy":"manual","contextPolicy":"off","confirmedCommands":[],
            "customPackHashes":[],"coAuthor":false}}}})).unwrap()
}
#[tokio::test]
async fn durable_replay_idempotency_revision_and_owner() {
    let dir = tempfile::tempdir().unwrap();
    let mut repo = SqliteRepository::open(dir.path(), "run").unwrap();
    assert!(matches!(
        SqliteRepository::open(dir.path(), "run"),
        Err(Error::OwnerConflict)
    ));
    let mut req = request();
    let receipt = repo.submit(&req).await.unwrap();
    assert_eq!(repo.submit(&req).await.unwrap(), receipt);
    req.command = Command::CancelRun;
    assert!(matches!(
        repo.submit(&req).await,
        Err(Error::CommandConflict)
    ));
    req.command_id = "cancel".into();
    assert!(matches!(
        repo.submit(&req).await,
        Err(Error::RevisionConflict)
    ));
    req.expected_revision = 1;
    repo.submit(&req).await.unwrap();
    assert_eq!(repo.events("run", 0, 1).unwrap().len(), 1);
    assert_eq!(
        repo.events("run", 1, 10).unwrap()[0].kind,
        EventKind::DraftCancelled
    );
    // Old request retries return original durable result, even after subsequent transitions.
    assert_eq!(repo.submit(&request()).await.unwrap(), receipt);
    drop(repo);
    let repo = SqliteRepository::open(dir.path(), "run").unwrap();
    assert_eq!(repo.snapshot("run").unwrap().unwrap().revision, 2);
    assert_eq!(repo.events("run", 0, 10).unwrap().len(), 2);
}
#[tokio::test]
async fn failing_event_insert_rolls_back_snapshot_and_command_then_can_retry() {
    let dir = tempfile::tempdir().unwrap();
    let mut repo = SqliteRepository::open(dir.path(), "run").unwrap();
    let c = Connection::open(dir.path().join("tokage.db")).unwrap();
    c.execute_batch("CREATE TRIGGER reject_event BEFORE INSERT ON runtime_events BEGIN SELECT RAISE(ABORT,'disk failure fixture'); END;").unwrap();
    assert!(repo.submit(&request()).await.is_err());
    assert!(repo.snapshot("run").unwrap().is_none());
    assert_eq!(
        c.query_row("SELECT count(*) FROM runtime_commands", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    c.execute_batch("DROP TRIGGER reject_event").unwrap();
    repo.submit(&request()).await.unwrap();
    drop(repo);
    c.execute("UPDATE runtime_runs SET snapshot='broken'", [])
        .unwrap();
    assert!(SqliteRepository::open(dir.path(), "run").is_err());
}
#[tokio::test]
async fn legacy_fixtures_are_backed_up_idempotent_and_never_executable() {
    let dir = tempfile::tempdir().unwrap();
    let repo = SqliteRepository::open(dir.path(), "run").unwrap();
    let original = include_str!("fixtures/cli-v1.json");
    let backup = dir.path().join("input-backup.json");
    assert!(repo
        .import_history("cli-v1", "run", original, &backup)
        .await
        .is_err());
    std::fs::write(&backup, original).unwrap();
    let hash = repo
        .import_history("cli-v1", "run", original, &backup)
        .await
        .unwrap();
    assert_eq!(
        repo.import_history("cli-v1", "run", original, &backup)
            .await
            .unwrap(),
        hash
    );
    assert_eq!(repo.history("cli-v1", "run").unwrap().unwrap(), original);
    assert!(repo.snapshot("run").unwrap().is_none());
    assert!(repo.events("run", 0, 10).unwrap().is_empty());
    let changed = original.replace("historical request", "changed request");
    std::fs::write(&backup, &changed).unwrap();
    assert!(matches!(
        repo.import_history("cli-v1", "run", &changed, &backup)
            .await,
        Err(Error::CommandConflict)
    ));
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), changed);
    let app = include_str!("fixtures/app-pipeline-v1.json");
    let app_backup = dir.path().join("app-backup.json");
    std::fs::write(&app_backup, app).unwrap();
    let app_repo = SqliteRepository::open(dir.path(), "session:7").unwrap();
    app_repo
        .import_history("app-pipeline-v1", "session:7", app, &app_backup)
        .await
        .unwrap();
    assert!(app_repo.snapshot("session:7").unwrap().is_none());
}
#[test]
fn unknown_database_version_is_not_opened() {
    let dir = tempfile::tempdir().unwrap();
    drop(SqliteRepository::open(dir.path(), "run").unwrap());
    let c = Connection::open(dir.path().join("tokage.db")).unwrap();
    c.execute(
        "UPDATE refinery_schema_history SET version=999 WHERE version=14",
        [],
    )
    .unwrap();
    assert!(SqliteRepository::open(dir.path(), "run").is_err());
}
#[test]
fn migration_failure_keeps_consistent_backup_and_restores_in_new_directory() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tokage.db");
    let mut c = Connection::open(&path).unwrap();
    // Build the actual pre-runtime schema, not a fabricated version marker.
    let mut migrations = Vec::new();
    for item in std::fs::read_dir("../tokage-storage/migrations").unwrap() {
        let path = item.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap();
        if name[1..name.find("__").unwrap()].parse::<u32>().unwrap() > 13 {
            continue;
        }
        migrations.push((name.to_owned(), std::fs::read_to_string(path).unwrap()));
    }
    migrations.sort_by_key(|(name, _)| name[1..name.find("__").unwrap()].parse::<u32>().unwrap());
    let migrations: Vec<_> = migrations
        .into_iter()
        .map(|(name, sql)| {
            refinery::Migration::unapplied(&name.trim_end_matches(".sql"), &sql).unwrap()
        })
        .collect();
    refinery::Runner::new(&migrations).run(&mut c).unwrap();
    c.execute_batch("CREATE TABLE runtime_runs(conflicting_legacy_column TEXT);")
        .unwrap();
    assert!(SqliteRepository::open(dir.path(), "run").is_err());
    let backup = std::fs::read_dir(dir.path().join("backups"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let backup_conn = Connection::open(&backup).unwrap();
    assert_eq!(
        backup_conn
            .query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    assert_eq!(
        backup_conn
            .query_row(
                "SELECT max(version) FROM refinery_schema_history",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        13
    );
    let restored = tempfile::tempdir().unwrap();
    std::fs::copy(&backup, restored.path().join("tokage.db")).unwrap();
    // An operator resolves the incompatible table only in the restored copy.
    let recovery = Connection::open(restored.path().join("tokage.db")).unwrap();
    recovery
        .execute_batch("ALTER TABLE runtime_runs RENAME TO preserved_conflict;")
        .unwrap();
    let repo = SqliteRepository::open(restored.path(), "run").unwrap();
    assert!(repo.snapshot("run").unwrap().is_none());
    assert_eq!(
        c.query_row(
            "SELECT max(version) FROM refinery_schema_history",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        13
    );
}
#[tokio::test]
async fn parseable_but_inconsistent_history_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let mut repo = SqliteRepository::open(dir.path(), "run").unwrap();
    repo.submit(&request()).await.unwrap();
    let mut s = repo.snapshot("run").unwrap().unwrap();
    s.state = RunState::Delivered;
    drop(repo);
    let c = Connection::open(dir.path().join("tokage.db")).unwrap();
    c.execute(
        "UPDATE runtime_runs SET snapshot=?1",
        [serde_json::to_string(&s).unwrap()],
    )
    .unwrap();
    assert!(SqliteRepository::open(dir.path(), "run").is_err());
}
