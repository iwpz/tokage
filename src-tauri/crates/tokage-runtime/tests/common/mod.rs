use tokage_runtime::*;
pub fn create() -> Request {
    serde_json::from_value(serde_json::json!({"schemaVersion":1,"commandId":"create","runId":"run","expectedRevision":0,
        "command":{"type":"CreateRun","payload":{"requirement":"implement feature","config":{
            "source":"cli","policyVersion":"1","workspace":"/repo","targetIdentity":"sha",
            "scope":["src"],"models":{"executor":{"provider":"CODEX_CLI","requestedModel":"gpt-5.6-sol","providerPath":null,"capabilities":["STRUCTURED_OUTPUT","READ_ONLY_MODE"],"userLocked":true}},"commandPolicyHash":"policy-hash","trustWorkspace":true,
            "permissionMode":"workspace-write","concurrency":2,"budgetUsd":10.0,"blockUnknownCost":true,
            "timeoutSeconds":60,"repairPolicy":"manual","contextPolicy":"off","confirmedCommands":[],
            "customPackHashes":[],"coAuthor":false}}}})).unwrap()
}
pub const PLAN: &str = r#"# Spec
## Goals
Implement feature
# Tasks
```yaml
- id: T1
  domain: backend-api
  title: Feature
  goal: Implement feature
  write_scope: [src/feature/**]
  assigned_model: bad-model
  acceptance:
    commands: [cargo test]
    criteria: [feature works]
```
"#;
pub fn command(s: &Snapshot, id: &str, command: Command) -> Request {
    Request {
        schema_version: SCHEMA_VERSION,
        command_id: id.into(),
        run_id: s.run_id.clone(),
        expected_revision: s.revision,
        command,
    }
}
