use serde_json::{json, Value};
use std::process::Command;
#[test]
fn public_cli_obeys_the_same_wire_and_semantic_oracle_as_app_and_service() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../tests/fixtures/shared-runtime-v1.json")).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let file = temp.path().join("request.json");
    let call = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_tokage"))
            .args(["--json", "--data-dir", data.to_str().unwrap()])
            .args(args)
            .output()
            .unwrap()
    };
    let mut digest = Value::Null;
    for (index, kind) in ["CreateRun", "ImportPlan", "ConfirmPlan", "CancelRun"]
        .into_iter()
        .enumerate()
    {
        let mut command = fixture["commands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["wire"]["type"] == kind)
            .unwrap()["wire"]
            .clone();
        if kind == "ConfirmPlan" {
            command["payload"]["confirmation_digest"] = digest.clone();
        }
        let request = json!({"schemaVersion":1,"runId":"contract","commandId":kind,"expectedRevision":index,"command":command});
        std::fs::write(&file, serde_json::to_vec(&request).unwrap()).unwrap();
        let output = call(&["run", "submit", "--file", file.to_str().unwrap()]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        let s = &value["snapshot"];
        assert_eq!(s["state"], fixture["scenario"]["states"][index]);
        assert_eq!(s["revision"], index + 1);
        assert!(s["turns"].is_null() || s["turns"].as_object().unwrap().is_empty());
        for field in ["execution", "work", "delivery"] {
            assert!(s[field].is_null());
        }
        if !s["plan"].is_null() {
            digest = s["plan"]["confirmationDigest"].clone();
            assert_eq!(
                s["plan"]["assignments"]["T1"]["requestedModel"],
                fixture["scenario"]["expectedModel"]
            );
        }
        let replay = call(&["run", "submit", "--file", file.to_str().unwrap()]);
        assert!(replay.status.success());
        assert_eq!(
            serde_json::from_slice::<Value>(&replay.stdout).unwrap(),
            value
        );
        let mut stale = request.clone();
        stale["commandId"] = json!(format!("stale-{index}"));
        stale["expectedRevision"] = json!(0);
        std::fs::write(&file, serde_json::to_vec(&stale).unwrap()).unwrap();
        assert!(!call(&["run", "submit", "--file", file.to_str().unwrap()])
            .status
            .success());
    }
    let output = call(&[
        "run", "events", "--id", "contract", "--after", "0", "--limit", "10",
    ]);
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        value["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["kind"].clone())
            .collect::<Vec<_>>(),
        fixture["scenario"]["events"].as_array().unwrap().clone()
    );
    assert!(!data.join("runtime-v1.native/worktrees").exists());
}
