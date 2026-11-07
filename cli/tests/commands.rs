use std::process::Command;
fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_tokage"))
        .args(args)
        .output()
        .unwrap()
}
#[test]
fn help_and_parse_errors_do_not_create_data_or_call_models() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("not-created");
    for command in [
        vec!["--help"],
        vec!["--version"],
        vec!["nonsense"],
        vec!["run", "create", "--unknown"],
    ] {
        let mut args = vec!["--json", "--data-dir", data.to_str().unwrap()];
        args.extend(command);
        let output = cli(&args);
        assert!(
            serde_json::from_slice::<serde_json::Value>(if output.status.success() {
                &output.stdout
            } else {
                &output.stderr
            })
            .is_ok()
        );
        assert!(!data.exists());
    }
    assert!(!cli(&["--data-dir"]).status.success());
    assert!(!cli(&["providers", "list", "--unknown"]).status.success());
    assert!(!cli(&["providers", "list", "--json", "--json"])
        .status
        .success());
}
#[test]
fn invalid_plan_has_failing_exit_status() {
    let temp = tempfile::tempdir().unwrap();
    let plan = temp.path().join("plan.md");
    std::fs::write(&plan, "# Tasks\ninvalid").unwrap();
    assert!(
        !cli(&["plan", "validate", "--file", plan.to_str().unwrap()])
            .status
            .success()
    );
}

#[test]
fn direct_transport_requires_explicit_consent_and_reads_empty_history_without_inference() {
    let temp=tempfile::tempdir().unwrap();
    let data=temp.path().join("data");
    let data=data.to_str().unwrap();
    let output=cli(&["--json","--data-dir",data,"direct","send","--request","missing.json"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--trust-workspace"));
    assert!(!std::path::Path::new(data).exists());
    let output=cli(&["--json","--data-dir",data,"direct","history","--session","new"]);
    assert!(output.status.success());
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),serde_json::json!([]));
    assert!(!std::path::Path::new(data).exists());
    let file=temp.path().join("request.json");
    std::fs::write(&file,r#"{"turnId":"t","sessionId":"s","cwd":"/","prompt":"work","mode":"direct","provider":"CODEX_CLI","model":"test","authMode":"full_auto"}"#).unwrap();
    let output=cli(&["--json","--data-dir",data,"direct","send","--request",file.to_str().unwrap(),"--trust-workspace"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--full-auto"));
    assert!(!std::path::Path::new(data).exists());
}

#[cfg(unix)]
#[test]
fn direct_public_commands_persist_offline_protocol_turns_and_refuse_replay() {
    use std::os::unix::fs::PermissionsExt;
    let temp=tempfile::tempdir().unwrap();
    let workspace=temp.path().join("workspace");std::fs::create_dir(&workspace).unwrap();
    let fixture=temp.path().join("codex-fixture");
    std::fs::write(&fixture,r#"#!/bin/sh
case "$1" in
  --version) printf 'codex-cli 0.162.0\n'; exit 0;;
  login) printf 'Logged in using ChatGPT\n'; exit 0;;
  exec) cat >/dev/null;;
  *) exit 0;;
esac
printf '%s\n' '{"type":"thread.started","thread_id":"fixture-thread"}'
printf '%s\n' '{"type":"item.completed","item":{"id":"answer","type":"agent_message","text":"Offline protocol reply"}}'
printf '%s\n' '{"type":"turn.completed","usage":{"input_tokens":10,"output_tokens":5}}'
"#).unwrap();
    std::fs::set_permissions(&fixture,std::fs::Permissions::from_mode(0o755)).unwrap();
    let data=temp.path().join("data");let data=data.to_str().unwrap();
    let file=temp.path().join("request.json");
    let request=serde_json::json!({"turnId":"one","sessionId":"session","cwd":workspace,"prompt":"Read this workspace","mode":"direct","provider":"CODEX_CLI","model":"fixture-model"});
    std::fs::write(&file,request.to_string()).unwrap();
    let args=["--json","--data-dir",data,"direct","send","--request",file.to_str().unwrap(),"--trust-workspace","--provider-path",fixture.to_str().unwrap()];
    let output=cli(&args);
    assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));
    let value:serde_json::Value=serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["turn"]["status"],"completed","{value}");
    assert_eq!(value["turn"]["request"]["model"],"fixture-model");
    assert!(value["events"].as_array().unwrap().iter().any(|e|e["kind"]=="done"));
    let repeated=cli(&args);
    assert!(!repeated.status.success());
    assert!(String::from_utf8_lossy(&repeated.stderr).contains("already submitted"));
    let history=cli(&["--json","--data-dir",data,"direct","history","--session","session"]);
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&history.stdout).unwrap().as_array().unwrap().len(),1);
    let events=cli(&["--json","--data-dir",data,"direct","events","--turn","one"]);
    let events:serde_json::Value=serde_json::from_slice(&events.stdout).unwrap();
    assert!(events.as_array().unwrap().iter().any(|e|e["kind"]=="text"&&e["text"]=="Offline protocol reply"));

    // A native permission denial can close the process without a Done frame.
    // Preserve the pending user decision instead of inventing completion/error.
    std::fs::write(&fixture,r#"#!/bin/sh
case "$1" in
  --version) printf 'codex-cli 0.162.0\n'; exit 0;;
  login) printf 'Logged in using ChatGPT\n'; exit 0;;
  exec) cat >/dev/null;;
  *) exit 0;;
esac
printf '%s\n' '{"type":"thread.started","thread_id":"approval-thread"}'
printf '%s\n' '{"type":"item.started","item":{"type":"command_execution","id":"command","command":"touch approved.txt"}}'
printf '%s\n' '{"type":"item.completed","item":{"type":"command_execution","id":"command","command":"touch approved.txt","exit_code":1,"aggregated_output":"Operation not permitted (read-only filesystem)"}}'
"#).unwrap();
    let mut approval=request.clone();approval["turnId"]=serde_json::json!("approval");approval["authMode"]=serde_json::json!("ask_always");
    std::fs::write(&file,approval.to_string()).unwrap();
    let output=cli(&args);
    assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));
    let value:serde_json::Value=serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["turn"]["status"],"awaiting_approval","{value}");
    assert!(value["events"].as_array().unwrap().iter().any(|e|e["kind"]=="approvalRequest"));
    assert!(!value["events"].as_array().unwrap().iter().any(|e|e["kind"]=="done"||e["kind"]=="error"));
    assert!(!workspace.join("approved.txt").exists());
}

#[cfg(unix)]
#[test]
fn direct_ctrl_c_stops_the_offline_process_before_returning() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration,Instant};
    let temp=tempfile::tempdir().unwrap();
    let workspace=temp.path().join("workspace");std::fs::create_dir(&workspace).unwrap();
    let fixture=temp.path().join("codex-fixture");
    std::fs::write(&fixture,r#"#!/bin/sh
case "$1" in
  --version) printf 'codex-cli 0.162.0\n'; exit 0;;
  login) printf 'Logged in using ChatGPT\n'; exit 0;;
  exec) cat >/dev/null;;
  *) exit 0;;
esac
printf '%s\n' '{"type":"thread.started","thread_id":"fixture-thread"}'
sleep 30
"#).unwrap();
    std::fs::set_permissions(&fixture,std::fs::Permissions::from_mode(0o755)).unwrap();
    let data=temp.path().join("data");
    let file=temp.path().join("request.json");
    std::fs::write(&file,serde_json::json!({"turnId":"stop","sessionId":"session","cwd":workspace,"prompt":"Wait","mode":"direct","provider":"CODEX_CLI","model":"fixture-model"}).to_string()).unwrap();
    let child=Command::new(env!("CARGO_BIN_EXE_tokage")).args(["--json","--data-dir",data.to_str().unwrap(),"direct","send","--request",file.to_str().unwrap(),"--trust-workspace","--provider-path",fixture.to_str().unwrap()]).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
    let deadline=Instant::now()+Duration::from_secs(10);
    while tokage_native::direct::events(&data.join("direct-v1"),"stop").unwrap_or_default().is_empty(){
        if Instant::now()>deadline{let _=Command::new("/bin/kill").args(["-TERM",&child.id().to_string()]).status();panic!("offline child did not start");}
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(Command::new("/bin/kill").args(["-INT",&child.id().to_string()]).status().unwrap().success());
    let output=child.wait_with_output().unwrap();
    assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));
    let value:serde_json::Value=serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["turn"]["status"],"stopped","{value}");
    let record=tokage_native::direct::list(&data.join("direct-v1"),Some("session")).unwrap().pop().unwrap();
    assert!(tokage_native::recovery::runtime_idle(record.process_directory.as_deref()).unwrap());
}
