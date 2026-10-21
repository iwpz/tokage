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
