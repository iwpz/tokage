//! Real installed CLIs against a loopback synthetic model, never a vendor API.
use super::*;
use std::sync::Arc;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::Notify,
};

async fn server(mode: &'static str, entered: Arc<Notify>) -> (u16, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        let mut connections = tokio::task::JoinSet::new();
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            let entered = entered.clone();
            connections.spawn(async move {
                let mut data = vec![];
                let mut buf = [0; 8192];
                let (offset, length) = loop {
                    let Ok(n) = stream.read(&mut buf).await else {
                        return;
                    };
                    if n == 0 {
                        return;
                    }
                    data.extend_from_slice(&buf[..n]);
                    if data.len() > 2 * 1024 * 1024 {
                        return;
                    }
                    if let Some(offset) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&data[..offset]);
                        let length = headers
                            .lines()
                            .find_map(|l| {
                                l.split_once(':')
                                    .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                                    .and_then(|(_, v)| v.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        break (offset + 4, length);
                    }
                };
                while data.len() < offset + length {
                    let Ok(n) = stream.read(&mut buf).await else {
                        return;
                    };
                    if n == 0 {
                        return;
                    }
                    data.extend_from_slice(&buf[..n]);
                }
                let request: serde_json::Value =
                    serde_json::from_slice(&data[offset..offset + length]).unwrap_or_default();
                entered.notify_one();
                if mode == "idle" || mode == "cancel" {
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    return;
                }
                let _=stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n").await;
                let tool_result = request["messages"]
                    .as_array()
                    .is_some_and(|rows| rows.iter().any(|m| m["role"] == "tool"));
                let tool = mode == "denial"
                    && !tool_result
                    && request["tools"].as_array().is_some_and(|a| !a.is_empty());
                let delta = if tool {
                    serde_json::json!({"role":"assistant","tool_calls":[{"index":0,"id":"policy-test","type":"function","function":{"name":"bash","arguments":"{\"command\":\"touch denied-marker\",\"description\":\"policy probe\"}"}}]})
                } else {
                    serde_json::json!({"role":"assistant","content":"TOKAGE_POLICY_OK"})
                };
                let chunk = |delta: serde_json::Value, finish: serde_json::Value| {
                    format!(
                        "data: {}\n\n",
                        serde_json::json!({"id":"fixture","object":"chat.completion.chunk","created":1,"model":"deepseek-flash","choices":[{"index":0,"delta":delta,"finish_reason":finish}],"usage":{"prompt_tokens":12,"completion_tokens":5,"total_tokens":17}})
                    )
                };
                if mode == "hard" {
                    loop {
                        if stream
                            .write_all(
                                chunk(serde_json::json!({"content":"."}), serde_json::Value::Null)
                                    .as_bytes(),
                            )
                            .await
                            .is_err()
                        {
                            return;
                        }
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
                let _ = stream
                    .write_all(chunk(delta, serde_json::Value::Null).as_bytes())
                    .await;
                let _ = stream
                    .write_all(
                        chunk(
                            serde_json::json!({}),
                            serde_json::json!(if tool { "tool_calls" } else { "stop" }),
                        )
                        .as_bytes(),
                    )
                    .await;
                let _ = stream.write_all(b"data: [DONE]\n\n").await;
            });
        }
    });
    (port, task)
}

#[tokio::test]
#[ignore = "requires installed OpenCode 1.x and 2.x; local model only, no upstream credits"]
async fn installed_versions_cancel_timeouts_and_deny_shell() {
    let captured = crate::env::capture().await;
    for variable in ["TOKAGE_OPENCODE_V1_BIN", "TOKAGE_OPENCODE_V2_BIN"] {
        let exe = PathBuf::from(std::env::var(variable).expect("set both CLI binary paths"));
        assert!(exe.is_file(), "{}", exe.display());
        let version = tokio::process::Command::new(&exe)
            .arg("--version")
            .output()
            .await
            .unwrap();
        let expected = if variable.contains("V1") { "1." } else { "2." };
        assert!(
            version.status.success()
                && String::from_utf8_lossy(&version.stdout)
                    .split_whitespace()
                    .any(|part| part.trim_start_matches('v').starts_with(expected)),
            "wrong CLI major for {variable}"
        );
        for mode in ["cancel", "idle", "hard", "denial"] {
            let dir = tempfile::tempdir().unwrap();
            let home = dir.path().join("home");
            let app_data = dir.path().join("app-data");
            let work = app_data.join("worktrees/repo/active");
            let source = dir.path().join("source");
            std::fs::create_dir_all(&home).unwrap();
            std::fs::create_dir_all(&app_data).unwrap();
            for args in [
                vec!["init", source.to_str().unwrap()],
                vec![
                    "-C",
                    source.to_str().unwrap(),
                    "-c",
                    "user.name=Fixture",
                    "-c",
                    "user.email=fixture@example.invalid",
                    "-c",
                    "commit.gpgsign=false",
                    "commit",
                    "--allow-empty",
                    "-m",
                    "fixture",
                ],
                vec![
                    "-C",
                    source.to_str().unwrap(),
                    "worktree",
                    "add",
                    work.to_str().unwrap(),
                ],
            ] {
                assert!(std::process::Command::new("git")
                    .args(args)
                    .output()
                    .unwrap()
                    .status
                    .success());
            }
            let entered = Arc::new(Notify::new());
            let (port, server) = server(mode, entered.clone()).await;
            let mut env = ShellEnv {
                vars: [
                    ("PATH".into(), captured.path().unwrap().into()),
                    ("HOME".into(), home.display().to_string()),
                    ("PWD".into(), work.display().to_string()),
                ]
                .into(),
            };
            for (key, path) in [
                ("XDG_DATA_HOME", "data"),
                ("XDG_CONFIG_HOME", "config"),
                ("XDG_CACHE_HOME", "cache"),
                ("XDG_STATE_HOME", "state"),
            ] {
                env.vars
                    .insert(key.into(), home.join(path).display().to_string());
            }
            env.vars.insert("OPENCODE_CONFIG_CONTENT".into(),serde_json::json!({"provider":{"deepseek":{"options":{"baseURL":format!("http://127.0.0.1:{port}/v1"),"apiKey":"local-fixture-only"}}}}).to_string());
            let mut req = testing::request(ProviderId::Opencode);
            req.cwd = work.display().to_string();
            req.model = "deepseek/deepseek-flash".into();
            req.role = Some(Role::Executor);
            req.timeout_tier = Some(TimeoutTier::Agent);
            req.idle_timeout_secs = Some(if mode == "idle" { 5 } else { 15 });
            req.hard_timeout_secs = Some(if mode == "hard" { 10 } else { 45 });
            req.prompt = "Use bash to touch denied-marker then reply TOKAGE_POLICY_OK".into();
            let events = std::sync::Mutex::new(vec![]);
            let emit = |e| events.lock().unwrap().push(e);
            let (tx, rx) = tokio::sync::watch::channel(false);
            let result = {
                let run = run_cli(
                    &exe,
                    &req,
                    launch(&req).unwrap(),
                    &env,
                    Some(app_data.join("files")),
                    None,
                    &emit,
                    Some(rx),
                );
                let trigger = async {
                    if mode == "cancel" {
                        let _ =
                            tokio::time::timeout(Duration::from_secs(40), entered.notified()).await;
                        let _ = tx.send(true);
                    }
                };
                tokio::pin!(run);
                tokio::select! { result = &mut run => result, _ = trigger => run.await }
            };
            server.abort();
            let events = events.into_inner().unwrap();
            if mode == "denial" {
                assert!(
                    result.is_ok(),
                    "{variable} {mode}: {result:?}; events={events:?}"
                );
                assert!(
                    events.iter().any(|e| matches!(
                        e,
                        EventKind::ToolEnd {
                            ok: Some(false),
                            ..
                        }
                    )),
                    "no denied tool: {events:?}"
                );
            } else {
                let error = result.unwrap_err();
                assert!(
                    if mode == "cancel" {
                        error == TURN_CANCELLED
                    } else {
                        error.contains(&format!("{mode} timeout"))
                    },
                    "{variable} {mode}: {error}"
                );
                assert!(!events.iter().any(|e| matches!(e, EventKind::Done { .. })));
            }
            assert!(!work.join("denied-marker").exists());
            println!("PASS {variable} {mode}: real CLI / production supervisor / local model");
        }
    }
}
