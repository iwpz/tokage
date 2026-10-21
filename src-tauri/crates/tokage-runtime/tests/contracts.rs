use tokage_runtime::*;
fn request() -> Request {
    serde_json::from_value(serde_json::json!({"schemaVersion":1,"commandId":"create","runId":"run","expectedRevision":0,
        "command":{"type":"CreateRun","payload":{"requirement":"implement feature","config":{
            "source":"cli","policyVersion":"1","workspace":"/repo","targetIdentity":"sha",
            "scope":["src"],"models":{"executor":{"provider":"codex","requestedModel":"model","providerPath":null,"capabilities":["read"],"userLocked":true}},"commandPolicyHash":"policy-hash","trustWorkspace":false,
            "permissionMode":"read-only","concurrency":1,"budgetUsd":null,"blockUnknownCost":true,
            "timeoutSeconds":60,"repairPolicy":"manual","contextPolicy":"off","confirmedCommands":[],
            "customPackHashes":[],"coAuthor":false}}}})).unwrap()
}
#[test]
fn rejects_unknown_versions_commands_and_snapshot_writes() {
    let mut r = request();
    r.schema_version = 2;
    assert!(matches!(transition(None, &r), Err(Error::Schema)));
    assert!(
        serde_json::from_str::<Command>(r#"{"type":"SetState","payload":"Delivered"}"#).is_err()
    );
    let mut value = serde_json::to_value(request()).unwrap();
    value["override"] = true.into();
    assert!(serde_json::from_value::<Request>(value).is_err());
}
#[test]
fn transitions_fence_revision_and_never_complete_execution() {
    let mut r = request();
    let (s, e) = transition(None, &r).unwrap();
    assert_eq!(s.revision, e.sequence);
    assert_eq!(s.state, RunState::Draft);
    r.command = Command::CancelRun;
    assert!(matches!(
        transition(Some(&s), &r),
        Err(Error::RevisionConflict)
    ));
    r.expected_revision = 1;
    assert_eq!(transition(Some(&s), &r).unwrap().0.state, RunState::Stopped);
    r.command = Command::StartExecution {
        confirmation_digest: "anything".into(),
    };
    assert!(matches!(transition(Some(&s), &r), Err(Error::Unsupported)));
    let mut corrupt = s;
    corrupt.config.concurrency = 2;
    assert!(transition(Some(&corrupt), &r).is_err());
}
#[test]
fn invalid_config_cannot_create_run() {
    let mut r = request();
    if let Command::CreateRun { config, .. } = &mut r.command {
        config.concurrency = 0;
    }
    assert!(transition(None, &r).is_err());
}
