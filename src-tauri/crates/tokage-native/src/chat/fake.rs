//! `FakeAdapter` (docs/01 §10): a scripted stand-in for a CLI, for tests of everything above
//! the parsers. It runs `/bin/sh` through the same `run_cli` path as a real turn (process
//! group, trimmed environment, file tracking) and can replay a recorded sample or misbehave:
//! hang, flood, leave grandchildren, write outside the workspace, read credential paths,
//! print its environment.
//!
//! Besides a sample's own lines, the script can print `{"fake": …}` lines that the fake's
//! parser maps straight to events; everything else goes to the impersonated CLI's parser.

use std::path::{Path, PathBuf};

use super::{
    adapter::{self, AdapterMeta, ProviderAdapter},
    s, ChatRequest, EventKind, FileTouch, Launch, Out, Parser, ToolKind,
};
use crate::providers::ProviderId;

pub enum Step {
    /// Print a recorded sample from tests/fixtures, as the CLI would.
    Replay(&'static str),
    /// Print one fake-protocol line: `{"fake": "thread" | "text" | "tool" | "toolEnd" |
    /// "file" | "done" | "fail", …}`.
    Emit(serde_json::Value),
    DelayMs(u64),
    /// Write the shell's pid to the file, then never finish.
    Hang { pidfile: PathBuf },
    /// `lines` lines of non-JSON noise as fast as possible.
    Flood { lines: usize },
    /// Start a background process that outlives the CLI; its pid goes to the file.
    Grandchild { pidfile: PathBuf },
    /// Write `text` to `path` (anywhere, inside the workspace or not) and report the edit.
    Write { path: PathBuf, text: String },
    /// Read a file the CLI shouldn't touch (e.g. a credential store) via a shell tool step.
    Read { path: PathBuf },
    /// Probe reading a path and report whether the OS kernel allowed or blocked it.
    ProbeRead { path: PathBuf },
    /// Probe executing a command and report whether it succeeded or was blocked by sandbox.
    ProbeCommand { cmd: String, id: String },
    /// Dump the environment the CLI was started with, as plain text.
    PrintEnv,
    Exit(i32),
    Denial { tool_id: String, command: String, reason: String },
    WritePid(PathBuf),
}

pub struct FakeAdapter {
    /// CLI this fake stands in for: its meta, child environment and sample parser.
    pub provider: ProviderId,
    pub steps: Vec<Step>,
}

impl FakeAdapter {
    pub fn new(provider: ProviderId, steps: Vec<Step>) -> Self {
        Self { provider, steps }
    }

    pub fn exe() -> PathBuf {
        PathBuf::from("/bin/sh")
    }

    fn script(&self) -> String {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        // Every fake launch advertises piped prompt input. Consume it before
        // replaying/returning, rather than racing the supervisor's stdin write.
        let mut lines = vec!["cat >/dev/null".to_owned()];
        for step in &self.steps {
            let line = match step {
                Step::Replay(name) => format!("cat {}", q(&fixtures.join(name).display().to_string())),
                Step::Emit(v) => format!("echo {}", q(&v.to_string())),
                Step::DelayMs(ms) => format!("sleep {}", *ms as f64 / 1000.0),
                Step::Hang { pidfile } => format!("echo $$ > {}; sleep 3600", path(pidfile)),
                Step::Flood { lines } => format!("yes 'noise noise noise' | head -n {lines}"),
                Step::Grandchild { pidfile } => {
                    format!("sleep 300 >/dev/null 2>&1 & echo $! > {}", path(pidfile))
                }
                Step::Write { path: p, text } => {
                    let ev = serde_json::json!({"fake": "file", "path": p});
                    format!("printf %s {} > {}; echo {}", q(text), path(p), q(&ev.to_string()))
                }
                Step::Read { path: p } => {
                    let cmd = format!("cat {}", path(p));
                    let start = serde_json::json!({"fake": "tool", "id": "read", "cmd": cmd});
                    let end = serde_json::json!({"fake": "toolEnd", "id": "read"});
                    format!("echo {}; {cmd} >/dev/null 2>&1; echo {}", q(&start.to_string()), q(&end.to_string()))
                }
                Step::ProbeRead { path: p } => {
                    let cmd = format!("cat {}", path(p));
                    let start = serde_json::json!({"fake": "tool", "id": "probe_read", "cmd": cmd});
                    let allowed = serde_json::json!({"fake": "text", "text": "READ_ALLOWED"});
                    let blocked = serde_json::json!({"fake": "text", "text": "READ_BLOCKED"});
                    format!(
                        "echo {}; if {cmd} >/dev/null 2>&1; then echo {}; else echo {}; fi",
                        q(&start.to_string()),
                        q(&allowed.to_string()),
                        q(&blocked.to_string()),
                    )
                }
                Step::ProbeCommand { cmd, id } => {
                    let start = serde_json::json!({"fake": "tool", "id": id, "cmd": cmd});
                    let ok_ev = serde_json::json!({"fake": "text", "text": format!("{id}:SUCCESS")});
                    let fail_ev = serde_json::json!({"fake": "text", "text": format!("{id}:BLOCKED")});
                    format!(
                        "echo {}; if {cmd} >/dev/null 2>&1; then echo {}; else echo {}; fi",
                        q(&start.to_string()),
                        q(&ok_ev.to_string()),
                        q(&fail_ev.to_string()),
                    )
                }
                Step::PrintEnv => "env".into(),
                Step::Exit(code) => format!("exit {code}"),
                Step::Denial { tool_id, command, reason } => {
                    let ev = serde_json::json!({"fake": "denial", "tool_id": tool_id, "command": command, "reason": reason});
                    format!("echo {}", q(&ev.to_string()))
                }
                Step::WritePid(p) => format!("echo $$ > {}", path(p)),
            };
            lines.push(line);
        }
        lines.join("\n")
    }
}

/// Result of executing a FakeAdapter scripted turn.
#[derive(Debug)]
pub struct FakeRun {
    pub result: Result<(), String>,
    pub events: Vec<EventKind>,
}

/// Run a scripted FakeAdapter turn. Useful for cross-crate acceptance and integration testing.
pub async fn run_fake_adapter(
    fake: &FakeAdapter,
    env: &crate::env::ShellEnv,
    req: &ChatRequest,
) -> FakeRun {
    let events = std::sync::Mutex::new(vec![]);
    let filter = crate::chat::filter::OutputFilter::new(env);
    let emit = |k: EventKind| {
        crate::chat::process_event_for_emit(k, &filter, |ev| {
            events.lock().unwrap().push(ev);
        });
    };
    let launch = fake.launch(req, "p".into());
    let result =
        super::run_cli(&FakeAdapter::exe(), req, launch, env, None, None, &emit, None).await;
    FakeRun {
        result,
        events: events.into_inner().unwrap(),
    }
}

impl ProviderAdapter for FakeAdapter {
    fn meta(&self) -> &'static AdapterMeta {
        adapter::adapter(self.provider)
            .expect("FakeAdapter impersonates a CLI with an adapter")
            .meta()
    }

    fn launch(&self, req: &ChatRequest, prompt: String) -> Launch {
        let inner = adapter::adapter(self.provider).map(|a| a.launch(req, String::new()).parser);
        Launch {
            args: vec!["-c".into(), self.script()],
            stdin: Some(prompt),
            env_remove: vec![],
            parser: Box::new(FakeParser { inner }),
        }
    }
}

struct FakeParser {
    inner: Option<Box<dyn Parser>>,
}

impl Parser for FakeParser {
    fn line(&mut self, v: &serde_json::Value, out: &mut Out) {
        let Some(kind) = s(v, "fake") else {
            if let Some(p) = self.inner.as_mut() {
                p.line(v, out);
            }
            return;
        };
        let str_of = |k: &str| s(v, k).unwrap_or_default();
        match kind.as_str() {
            "thread" => out.emit(EventKind::Thread { id: str_of("id") }),
            "text" => {
                out.begin_text();
                out.text(&str_of("text"));
            }
            "tool" => out.tool_start(&str_of("id"), ToolKind::Command, str_of("cmd"), None),
            "toolEnd" => out.tool_end(&str_of("id"), Some(true), s(v, "output")),
            "file" => out.file(FileTouch {
                tool_id: None,
                path: str_of("path"),
                before: None,
                after: None,
                deleted: false,
            }),
            "done" => out.emit(EventKind::Done {
                input_tokens: 0,
                output_tokens: 0,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                api_calls: None,
                context_tokens: None,
                cost_usd: None,
                premium_requests: None, credits: None,
                source: tokage_core::UsageSource::Structured,
            }),
            "fail" => out.fail(str_of("message")),
            "denial" => out.denial(
                Some(str_of("tool_id")),
                str_of("command"),
                str_of("reason"),
            ),
            "quota" => out.quota(
                str_of("raw"),
                s(v, "reset_at"),
                v.get("retrying").and_then(|r| r.as_bool()).unwrap_or(false),
            ),
            _ => {}
        }
    }

    fn text_line(&mut self, line: &str, out: &mut Out) {
        if let Some(p) = self.inner.as_mut() {
            p.text_line(line, out);
        }
    }

    fn finish(&mut self, out: &mut Out) {
        if let Some(p) = self.inner.as_mut() {
            p.finish(out);
        }
    }
}

/// Single-quotes a string for `sh`. Test-only: real CLIs never go through a shell (R4).
fn q(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn path(p: &Path) -> String {
    q(&p.display().to_string())
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Mutex, time::Duration};

    use super::*;
    use tokage_core::{Bus, DomainEvent, Role};
    use crate::{chat::run_cli, chat::TurnRegistry, env::ShellEnv};

    struct Run {
        result: Result<(), String>,
        events: Vec<EventKind>,
    }

    fn env(home: &Path, extra: &[(&str, &str)]) -> ShellEnv {
        let mut vars: HashMap<String, String> = [
            ("PATH", "/usr/bin:/bin"),
            ("HOME", home.to_str().unwrap()),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        vars.extend(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())));
        ShellEnv { vars }
    }

    async fn run(fake: &FakeAdapter, env: &ShellEnv, cwd: &Path) -> Run {
        run_with_mirror(fake, env, cwd, None).await
    }

    async fn run_with_mirror(
        fake: &FakeAdapter,
        env: &ShellEnv,
        cwd: &Path,
        mirror: Option<PathBuf>,
    ) -> Run {
        let mut req = crate::chat::testing::request(fake.provider);
        req.cwd = cwd.display().to_string();
        let events = Mutex::new(vec![]);
        let filter = crate::chat::filter::OutputFilter::new(env);
        let emit = |k: EventKind| {
            crate::chat::process_event_for_emit(k, &filter, |ev| {
                events.lock().unwrap().push(ev);
            });
        };
        let launch = fake.launch(&req, "p".into());
        let result =
            run_cli(&FakeAdapter::exe(), &req, launch, env, mirror, None, &emit, None).await;
        Run {
            result,
            events: events.into_inner().unwrap(),
        }
    }

    async fn run_req(fake: &FakeAdapter, env: &ShellEnv, req: ChatRequest) -> Run {
        let events = Mutex::new(vec![]);
        let filter = crate::chat::filter::OutputFilter::new(env);
        let emit = |k: EventKind| {
            crate::chat::process_event_for_emit(k, &filter, |ev| {
                events.lock().unwrap().push(ev);
            });
        };
        let launch = fake.launch(&req, "p".into());
        let result =
            run_cli(&FakeAdapter::exe(), &req, launch, env, None, None, &emit, None).await;
        Run {
            result,
            events: events.into_inner().unwrap(),
        }
    }

    fn pid(file: &Path) -> i32 {
        for _ in 0..50 {
            if let Ok(s) = std::fs::read_to_string(file) {
                if let Ok(n) = s.trim().parse::<i32>() {
                    return n;
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        std::fs::read_to_string(file).unwrap().trim().parse().unwrap()
    }

    /// Async variant of [`pid`] — use when other tasks must progress on a current-thread runtime.
    async fn pid_async(file: &Path) -> i32 {
        for _ in 0..100 {
            if let Ok(s) = tokio::fs::read_to_string(file).await {
                if let Ok(n) = s.trim().parse::<i32>() {
                    return n;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("pidfile not ready: {}", file.display());
    }

    /// Waits for a process to disappear; `kill(pid, 0)` fails once it's gone.
    async fn gone(pid: i32) -> bool {
        for _ in 0..50 {
            // SAFETY: signal 0 only checks for existence.
            if unsafe { libc::kill(pid, 0) } != 0 {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        false
    }

    fn texts(evs: &[EventKind]) -> String {
        evs.iter()
            .filter_map(|e| match e {
                EventKind::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Recorded samples go through the whole turn pipeline, not just the parser.
    #[tokio::test]
    async fn replays_every_sample() {
        let dir = tempfile::tempdir().unwrap();
        let env = env(dir.path(), &[]);
        for a in adapter::all() {
            let m = a.meta();
            let fake = FakeAdapter::new(m.id, vec![Step::Replay(m.sample)]);
            let r = run(&fake, &env, dir.path()).await;
            assert!(r.result.is_ok(), "{:?}: {:?}", m.id, r.result);
            assert!(!texts(&r.events).is_empty(), "{:?}", m.id);
            assert!(matches!(r.events.last(), Some(EventKind::Done { .. })), "{:?}", m.id);
        }
    }

    #[tokio::test]
    async fn failure_and_exit_codes() {
        let dir = tempfile::tempdir().unwrap();
        let env = env(dir.path(), &[]);
        let fail = FakeAdapter::new(
            ProviderId::ClaudeCode,
            vec![Step::Emit(serde_json::json!({"fake": "fail", "message": "Not logged in"})), Step::Exit(1)],
        );
        assert_eq!(run(&fail, &env, dir.path()).await.result, Err("Not logged in".into()));
        let crash = FakeAdapter::new(ProviderId::ClaudeCode, vec![Step::Exit(3)]);
        let err = run(&crash, &env, dir.path()).await.result.unwrap_err();
        assert!(err.contains("exit status: 3"), "{err}");
    }

    /// Output the parser doesn't know (an untested version) still reaches the user.
    #[tokio::test]
    async fn unknown_format_falls_back_to_text() {
        let dir = tempfile::tempdir().unwrap();
        let env = env(dir.path(), &[]);
        let fake = FakeAdapter::new(
            ProviderId::CodexCli,
            vec![Step::Emit(serde_json::json!({"type": "brand.new"})), Step::Flood { lines: 3 }],
        );
        let r = run(&fake, &env, dir.path()).await;
        assert!(r.result.is_ok(), "{:?}", r.result);
        assert_eq!(texts(&r.events), "noise noise noise\n  (×3)");
    }

    /// Acceptance test (P3.m2, R1): A flood of 200,000 lines is folded by OutputFilter
    /// down into (×200000), bounding memory to tens of bytes and keeping UI completely responsive.
    #[tokio::test]
    async fn flood_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let env = env(dir.path(), &[]);
        let fake = FakeAdapter::new(ProviderId::CodexCli, vec![Step::Flood { lines: 200_000 }]);
        let r = run(&fake, &env, dir.path()).await;
        assert!(r.result.is_ok());
        let t = texts(&r.events);
        assert!(t.contains("(×200000)"), "expected duplicate folding count in: {t}");
        assert!(t.len() < 200, "expected tiny output buffer, got {} bytes: {t}", t.len());
    }

    /// Acceptance test (P3.m1, R2): A hanging CLI process is terminated by the supervision
    /// idle timeout according to its tier and multiplier without an external timeout.
    #[tokio::test]
    async fn supervise_terminates_hang_on_idle_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let env = env(dir.path(), &[]);
        let pidfile = dir.path().join("hang_idle.pid");
        let fake = FakeAdapter::new(
            ProviderId::ClaudeCode,
            vec![Step::Hang { pidfile: pidfile.clone() }],
        );
        let mut req = crate::chat::testing::request(fake.provider);
        req.cwd = dir.path().display().to_string();
        req.timeout_tier = Some(crate::chat::TimeoutTier::Quick);
        // Quick tier: idle 15s * 0.02 = 300ms idle timeout
        req.timeout_multiplier = Some(0.02);

        let r = with_clean_timeout_env(run_req(&fake, &env, req)).await;
        let err = r.result.unwrap_err();
        assert!(err.contains("idle timeout exceeded"), "expected idle timeout error, got: {err}");
        assert!(gone(pid(&pidfile)).await, "hung process was not reaped by group.kill()");
    }

    /// Acceptance test (P3.m1, R2): Continuous stream exceeding hard timeout is terminated
    /// even when idle timeout is reset by ongoing lines.
    #[tokio::test]
    async fn supervise_terminates_on_hard_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let env = env(dir.path(), &[]);
        let pidfile = dir.path().join("hang_hard.pid");
        let fake = FakeAdapter::new(
            ProviderId::ClaudeCode,
            vec![
                Step::WritePid(pidfile.clone()),
                Step::Emit(serde_json::json!({"fake": "text", "text": "start"})),
                Step::DelayMs(50),
                Step::Emit(serde_json::json!({"fake": "text", "text": "middle"})),
                Step::DelayMs(50),
                Step::Emit(serde_json::json!({"fake": "text", "text": "late"})),
                Step::DelayMs(50),
                Step::Emit(serde_json::json!({"fake": "text", "text": "very late"})),
                Step::DelayMs(50),
                Step::Emit(serde_json::json!({"fake": "text", "text": "still going"})),
                Step::DelayMs(50),
                Step::Emit(serde_json::json!({"fake": "text", "text": "almost there"})),
                Step::DelayMs(50),
                Step::Emit(serde_json::json!({"fake": "text", "text": "overflowing"})),
                Step::DelayMs(3600_000),
            ],
        );
        let mut req = crate::chat::testing::request(fake.provider);
        req.cwd = dir.path().display().to_string();
        req.timeout_tier = Some(crate::chat::TimeoutTier::Quick);
        // Keep idle longer than hard: scheduler/process startup delays must not
        // turn this hard-timeout regression into an idle-timeout test.
        let r = with_timeout_env(1500, 500, run_req(&fake, &env, req)).await;
        let err = r.result.unwrap_err();
        assert!(err.contains("hard timeout exceeded"), "expected hard timeout error, got: {err}");
        assert!(gone(pid(&pidfile)).await, "hung process was not reaped on hard timeout");
    }

    /// Acceptance test (P3.m1c, docs 09 §2.2 / docs 11 §8): When an approval request is pending,
    /// idle timeout pauses so human deliberation does not cause premature idle timeout.
    /// Hard timeout still runs (F3.b); keep hard above the delay.
    #[tokio::test]
    async fn idle_timeout_is_paused_during_approval() {
        let dir = tempfile::tempdir().unwrap();
        let env = env(dir.path(), &[]);
        let fake = FakeAdapter::new(
            ProviderId::ClaudeCode,
            vec![
                Step::Denial {
                    tool_id: "t1".into(),
                    command: "git push origin main".into(),
                    reason: "sensitive remote push".into(),
                },
                // 400ms delay: above idle (300ms) and below hard (600ms)
                Step::DelayMs(400),
                Step::Emit(serde_json::json!({"fake": "done"})),
            ],
        );
        let mut req = crate::chat::testing::request(fake.provider);
        req.cwd = dir.path().display().to_string();
        req.timeout_tier = Some(crate::chat::TimeoutTier::Quick);
        // Quick: idle 15s * 0.05 = 750ms; hard 30s * 0.05 = 1500ms — delay sits between them.
        req.timeout_multiplier = Some(0.05);

        let r = with_clean_timeout_env(run_req(&fake, &env, req)).await;
        assert!(r.result.is_ok(), "turn timed out instead of pausing idle timer: {:?}", r.result);
        assert!(r.events.iter().any(|e| matches!(e, EventKind::ApprovalRequest { .. })), "ApprovalRequest missing");
    }

    /// Isolate timeout overrides in this async task; global env mutations race other turns.
    async fn with_clean_timeout_env<R>(fut: impl std::future::Future<Output = R>) -> R {
        super::super::testing::TIMEOUT_TEST_OVERRIDES.scope((None, None), fut).await
    }

    async fn with_timeout_env<R>(idle_ms: u64, hard_ms: u64, fut: impl std::future::Future<Output = R>) -> R {
        super::super::testing::TIMEOUT_TEST_OVERRIDES.scope((Some(idle_ms), Some(hard_ms)), fut).await
    }

    /// F3.a/F3.b: Agent tier must terminate a hang once idle elapses.
    #[tokio::test]
    async fn agent_timeout_idle_terminates_hang() {
        let dir = tempfile::tempdir().unwrap();
        let env = env(dir.path(), &[]);
        let pidfile = dir.path().join("agent_idle.pid");
        let fake = FakeAdapter::new(
            ProviderId::ClaudeCode,
            vec![Step::Hang {
                pidfile: pidfile.clone(),
            }],
        );
        let mut req = crate::chat::testing::request(fake.provider);
        req.cwd = dir.path().display().to_string();
        req.timeout_tier = Some(crate::chat::TimeoutTier::Agent);
        req.timeout_multiplier = Some(0.001); // must be ignored
        req.idle_timeout_secs = Some(1);
        req.hard_timeout_secs = Some(30);

        let r = with_clean_timeout_env(async {
            tokio::time::timeout(Duration::from_secs(5), run_req(&fake, &env, req)).await
        })
        .await;
        if r.is_err() {
            if let Ok(p) = std::panic::catch_unwind(|| pid(&pidfile)) {
                unsafe {
                    libc::kill(p, libc::SIGKILL);
                }
            }
        }
        let r = r.expect("Agent idle supervision should finish within 5s");
        let err = r.result.expect_err("expected idle timeout");
        assert!(
            err.contains("idle timeout exceeded"),
            "expected idle timeout error, got: {err}"
        );
        assert!(gone(pid(&pidfile)).await, "hung process was not reaped by group.kill()");
    }

    /// F3.a/F3.b: Agent tier must hard-timeout even while stdout keeps resetting idle.
    #[tokio::test]
    async fn agent_timeout_hard_terminates_streaming() {
        let dir = tempfile::tempdir().unwrap();
        let env = env(dir.path(), &[]);
        let pidfile = dir.path().join("agent_hard.pid");
        let fake = FakeAdapter::new(
            ProviderId::ClaudeCode,
            vec![
                Step::WritePid(pidfile.clone()),
                Step::Emit(serde_json::json!({"fake": "text", "text": "start"})),
                Step::DelayMs(200),
                Step::Emit(serde_json::json!({"fake": "text", "text": "middle"})),
                Step::DelayMs(200),
                Step::Emit(serde_json::json!({"fake": "text", "text": "late"})),
                Step::DelayMs(200),
                Step::Emit(serde_json::json!({"fake": "text", "text": "still"})),
                Step::DelayMs(3_600_000),
            ],
        );
        let mut req = crate::chat::testing::request(fake.provider);
        req.cwd = dir.path().display().to_string();
        req.timeout_tier = Some(crate::chat::TimeoutTier::Agent);
        req.timeout_multiplier = Some(0.001);
        req.idle_timeout_secs = Some(30);
        req.hard_timeout_secs = Some(1);

        let r = with_clean_timeout_env(async {
            tokio::time::timeout(Duration::from_secs(5), run_req(&fake, &env, req)).await
        })
        .await;
        if r.is_err() {
            if let Ok(p) = std::panic::catch_unwind(|| pid(&pidfile)) {
                unsafe {
                    libc::kill(p, libc::SIGKILL);
                }
            }
        }
        let r = r.expect("Agent hard supervision should finish within 5s");
        let err = r.result.expect_err("expected hard timeout");
        assert!(
            err.contains("hard timeout exceeded"),
            "expected hard timeout error, got: {err}"
        );
        assert!(gone(pid(&pidfile)).await, "hung process was not reaped on hard timeout");
    }

    /// F3.a/F3.b: during approval, idle pauses but hard still fires; idle alone must not fire.
    #[tokio::test]
    async fn agent_timeout_hard_fires_during_approval_idle_pauses() {
        let dir = tempfile::tempdir().unwrap();
        let env = env(dir.path(), &[]);
        // ① hard fires after Denial while idle is paused (ms overrides via env, under lock)
        let fake_hard = FakeAdapter::new(
            ProviderId::ClaudeCode,
            vec![
                Step::Denial {
                    tool_id: "t1".into(),
                    command: "git push origin main".into(),
                    reason: "sensitive remote push".into(),
                },
                Step::DelayMs(500),
            ],
        );
        let mut req_hard = crate::chat::testing::request(fake_hard.provider);
        req_hard.cwd = dir.path().display().to_string();
        req_hard.timeout_tier = Some(crate::chat::TimeoutTier::Agent);
        req_hard.timeout_multiplier = Some(0.001);

        let r_hard = with_timeout_env(50, 200, async {
            tokio::time::timeout(Duration::from_secs(3), run_req(&fake_hard, &env, req_hard)).await
        })
        .await;
        let r_hard = r_hard.expect("Agent hard-during-approval should finish within 3s");
        let err = r_hard.result.expect_err("expected hard timeout during approval");
        assert!(
            err.contains("hard timeout exceeded"),
            "expected hard timeout during approval, got: {err}"
        );
        assert!(
            r_hard
                .events
                .iter()
                .any(|e| matches!(e, EventKind::ApprovalRequest { .. })),
            "ApprovalRequest missing before hard timeout"
        );

        // ② idle alone must not fire while approval is pending (delay between idle and hard)
        let fake_idle = FakeAdapter::new(
            ProviderId::ClaudeCode,
            vec![
                Step::Denial {
                    tool_id: "t2".into(),
                    command: "git push origin main".into(),
                    reason: "sensitive remote push".into(),
                },
                Step::DelayMs(150),
                Step::Emit(serde_json::json!({"fake": "done"})),
            ],
        );
        let mut req_idle = crate::chat::testing::request(fake_idle.provider);
        req_idle.cwd = dir.path().display().to_string();
        req_idle.timeout_tier = Some(crate::chat::TimeoutTier::Agent);
        req_idle.timeout_multiplier = Some(0.001);

        let r_idle = with_timeout_env(50, 500, run_req(&fake_idle, &env, req_idle)).await;
        assert!(
            r_idle.result.is_ok(),
            "idle should pause during approval, got: {:?}",
            r_idle.result
        );
        assert!(
            r_idle
                .events
                .iter()
                .any(|e| matches!(e, EventKind::ApprovalRequest { .. })),
            "ApprovalRequest missing"
        );
    }

    /// Agent ignores `timeout_multiplier`; long delays under request secs complete.
    #[tokio::test]
    async fn agent_timeout_ignores_multiplier() {
        let dir = tempfile::tempdir().unwrap();
        let env = env(dir.path(), &[]);
        let fake = FakeAdapter::new(
            ProviderId::ClaudeCode,
            vec![
                Step::Emit(serde_json::json!({"fake": "text", "text": "start"})),
                Step::DelayMs(350),
                Step::Emit(serde_json::json!({"fake": "text", "text": "after long think"})),
                Step::Emit(serde_json::json!({"fake": "done"})),
            ],
        );
        let mut req = crate::chat::testing::request(fake.provider);
        req.cwd = dir.path().display().to_string();
        req.timeout_tier = Some(crate::chat::TimeoutTier::Agent);
        // Would be ~immediate timeout if multiplied against Quick; Agent must ignore it.
        req.timeout_multiplier = Some(0.001);
        req.idle_timeout_secs = Some(30);
        req.hard_timeout_secs = Some(60);

        // Hold the env lock so parallel Agent tests cannot leave TOKAGE_* overrides set.
        let r = with_timeout_env(30_000, 60_000, run_req(&fake, &env, req)).await;
        assert!(r.result.is_ok(), "Agent unexpectedly timed out under multiplier: {:?}", r.result);
        assert!(texts(&r.events).contains("after long think"), "expected completion message after delay");
    }

    /// Stopping a hung turn (dropping its future, as abort does) kills the CLI.
    #[tokio::test]
    async fn hang_is_killed_on_stop() {
        let dir = tempfile::tempdir().unwrap();
        let env = env(dir.path(), &[]);
        let pidfile = dir.path().join("hang.pid");
        let fake = FakeAdapter::new(
            ProviderId::ClaudeCode,
            vec![Step::Emit(serde_json::json!({"fake": "text", "text": "working"})), Step::Hang { pidfile: pidfile.clone() }],
        );
        let r = tokio::time::timeout(Duration::from_millis(1500), run(&fake, &env, dir.path())).await;
        assert!(r.is_err(), "a hung CLI never finishes on its own");
        assert!(gone(pid(&pidfile)).await, "hung CLI survived the stop");
    }

    /// F2.b: cancel via TurnRegistry must reap the hung CLI process group and publish
    /// exactly one `ChatEnded { cancelled: true }` with no further ChatOutput.
    #[tokio::test]
    async fn cancel_kills_hang_via_turn_registry() {
        let dir = tempfile::tempdir().unwrap();
        let env = env(dir.path(), &[]);
        let pidfile = dir.path().join("hang.pid");
        let fake = FakeAdapter::new(
            ProviderId::ClaudeCode,
            vec![
                Step::Emit(serde_json::json!({"fake": "text", "text": "working"})),
                Step::Hang {
                    pidfile: pidfile.clone(),
                },
            ],
        );
        let mut req = crate::chat::testing::request(fake.provider);
        req.cwd = dir.path().display().to_string();
        req.turn_id = "cancel-hang".into();
        req.timeout_tier = Some(crate::chat::TimeoutTier::Agent);
        req.idle_timeout_secs = Some(3_600);
        req.hard_timeout_secs = Some(7_200);
        let launch = fake.launch(&req, "p".into());

        let bus = Bus::new();
        let mut rx = bus.subscribe();
        let registry = TurnRegistry::new();
        registry.spawn_prepared(
            bus.clone(),
            req.clone(),
            FakeAdapter::exe(),
            launch,
            env,
            None,
        );

        let child = pid_async(&pidfile).await;
        assert!(
            registry.cancel(&req.turn_id).await,
            "hang turn should still be registered"
        );

        let mut ended = Vec::new();
        let mut output_after_end = 0usize;
        let mut turn_health_events = 0usize;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_millis(50), rx.recv()).await {
                Ok(Some(DomainEvent::ChatEnded {
                    turn_id,
                    cancelled,
                })) if turn_id == req.turn_id => {
                    ended.push(cancelled);
                }
                Ok(Some(DomainEvent::ChatOutput { turn_id, .. }))
                    if turn_id == req.turn_id && !ended.is_empty() =>
                {
                    output_after_end += 1;
                }
                Ok(Some(DomainEvent::TurnFailed { .. }))
                | Ok(Some(DomainEvent::TurnSucceeded { .. })) => {
                    turn_health_events += 1;
                }
                Ok(Some(_)) => {}
                Ok(None) | Err(_) => {
                    if !ended.is_empty() {
                        break;
                    }
                }
            }
            if ended.len() == 1 && output_after_end == 0 {
                // Keep draining briefly for stray output / duplicate ChatEnded.
                tokio::time::sleep(Duration::from_millis(100)).await;
                while let Ok(ev) = rx.try_recv() {
                    match ev {
                        DomainEvent::ChatEnded {
                            turn_id,
                            cancelled,
                        } if turn_id == req.turn_id => ended.push(cancelled),
                        DomainEvent::ChatOutput { turn_id, .. }
                            if turn_id == req.turn_id && !ended.is_empty() =>
                        {
                            output_after_end += 1;
                        }
                        DomainEvent::TurnFailed { .. } | DomainEvent::TurnSucceeded { .. } => {
                            turn_health_events += 1;
                        }
                        _ => {}
                    }
                }
                break;
            }
        }

        let reaped = gone(child).await;
        if !reaped {
            // SAFETY: test cleanup so a failed cancel does not leave sleep 3600 behind.
            unsafe {
                libc::kill(child, libc::SIGKILL);
            }
        }
        assert!(
            reaped,
            "cancel left hung CLI (pid {child}) alive — nested spawn abort bug"
        );
        assert_eq!(ended, vec![true], "expected exactly one ChatEnded(cancelled=true)");
        assert_eq!(output_after_end, 0, "no ChatOutput after ChatEnded");
        // F15.a⑥: cancelled turns must not publish TurnFailed / TurnSucceeded.
        assert_eq!(turn_health_events, 0, "cancel must not count toward provider health");
    }

    /// F2.d: Denial then process exit without `done` still publishes ChatEnded
    /// (after ApprovalRequest) so the frontend can treat “ended with no terminal event”.
    #[tokio::test]
    async fn chat_ended_after_approval_without_done() {
        let dir = tempfile::tempdir().unwrap();
        let env = env(dir.path(), &[]);
        let fake = FakeAdapter::new(
            ProviderId::ClaudeCode,
            vec![
                Step::Denial {
                    tool_id: "call_1".into(),
                    command: "rm -rf build".into(),
                    reason: "disallowed_tool".into(),
                },
                Step::Exit(0),
            ],
        );
        let mut req = crate::chat::testing::request(fake.provider);
        req.cwd = dir.path().display().to_string();
        req.turn_id = "approval-exit".into();
        let launch = fake.launch(&req, "p".into());

        let bus = Bus::new();
        let mut rx = bus.subscribe();
        let registry = TurnRegistry::new();
        registry.spawn_prepared(bus, req.clone(), FakeAdapter::exe(), launch, env, None);

        let mut saw_approval = false;
        let mut ended = None;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
                Ok(Some(DomainEvent::ChatOutput {
                    turn_id,
                    event: EventKind::ApprovalRequest { .. },
                })) if turn_id == req.turn_id => {
                    saw_approval = true;
                }
                Ok(Some(DomainEvent::ChatEnded {
                    turn_id,
                    cancelled,
                })) if turn_id == req.turn_id => {
                    ended = Some(cancelled);
                    break;
                }
                Ok(Some(_)) => {}
                Ok(None) | Err(_) => {}
            }
        }
        assert!(saw_approval, "expected ApprovalRequest before end");
        assert_eq!(ended, Some(false), "expected ChatEnded(cancelled=false) after exit");
    }

    /// F2.b: a turn that finishes on its own publishes ChatEnded with cancelled=false.
    #[tokio::test]
    async fn cancel_not_set_on_normal_turn_end() {
        let dir = tempfile::tempdir().unwrap();
        let env = env(dir.path(), &[]);
        let fake = FakeAdapter::new(
            ProviderId::ClaudeCode,
            vec![
                Step::Emit(serde_json::json!({"fake": "text", "text": "hi"})),
                Step::Emit(serde_json::json!({"fake": "done"})),
            ],
        );
        let mut req = crate::chat::testing::request(fake.provider);
        req.cwd = dir.path().display().to_string();
        req.turn_id = "normal-end".into();
        let launch = fake.launch(&req, "p".into());

        let bus = Bus::new();
        let mut rx = bus.subscribe();
        let registry = TurnRegistry::new();
        registry.spawn_prepared(bus, req.clone(), FakeAdapter::exe(), launch, env, None);

        let mut cancelled = None;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
                Ok(Some(DomainEvent::ChatEnded {
                    turn_id,
                    cancelled: c,
                })) if turn_id == req.turn_id => {
                    cancelled = Some(c);
                    break;
                }
                Ok(Some(_)) => {}
                Ok(None) | Err(_) => {}
            }
        }
        assert_eq!(cancelled, Some(false));
    }

    /// Background processes a CLI leaves behind are ended with its process group.
    #[tokio::test]
    async fn grandchildren_are_reaped() {
        let dir = tempfile::tempdir().unwrap();
        let env = env(dir.path(), &[]);
        let pidfile = dir.path().join("gc.pid");
        let fake = FakeAdapter::new(
            ProviderId::ClaudeCode,
            vec![
                Step::Grandchild { pidfile: pidfile.clone() },
                Step::DelayMs(100),
                Step::Emit(serde_json::json!({"fake": "done"})),
            ],
        );
        let r = run(&fake, &env, dir.path()).await;
        assert!(r.result.is_ok(), "{:?}", r.result);
        assert!(gone(pid(&pidfile)).await, "grandchild outlived the turn");
    }

    /// Acceptance test (P3.m5, R3): FakeAdapter grandchild process left behind
    /// after a simulated Tokage crash is reaped when Tokage restarts and runs `proc::cleanup_leftovers`.
    #[tokio::test]
    async fn grandchildren_cleaned_up_after_crash_restart() {
        let work_dir = tempfile::tempdir().unwrap();
        let app_dir = tempfile::tempdir().unwrap();
        let run_dir = app_dir.path().join("run");
        let mirror = Some(app_dir.path().join("files"));
        let env = env(work_dir.path(), &[]);
        let pidfile = work_dir.path().join("gc.pid");

        // Isolate crash simulation to this specific test directory
        crate::proc::simulate_crash_for_dir(&run_dir, true);

        let fake = FakeAdapter::new(
            ProviderId::ClaudeCode,
            vec![
                Step::Grandchild { pidfile: pidfile.clone() },
                Step::DelayMs(10_000), // Keep turn active
            ],
        );

        let cwd = work_dir.path().to_path_buf();
        let env_clone = env.clone();
        let task = tokio::spawn(async move {
            run_with_mirror(&fake, &env_clone, &cwd, mirror).await
        });

        // Wait until grandchild is running using async sleep to yield to background task
        let mut gc_pid = None;
        for _ in 0..100 {
            if let Ok(s) = std::fs::read_to_string(&pidfile) {
                if let Ok(n) = s.trim().parse::<i32>() {
                    gc_pid = Some(n);
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let gc_pid = gc_pid.expect("grandchild pid must be written to pidfile");
        assert_eq!(unsafe { libc::kill(gc_pid, 0) }, 0, "grandchild must be running");

        // Verify that children.json was written and contains active child
        let children_file = run_dir.join("children.json");
        assert!(children_file.exists(), "children.json must exist while turn is running");

        // Abort the running task, simulating immediate process crash/termination
        // Because crash simulation is active for this directory, Group::drop does not kill or unregister.
        task.abort();
        let _ = task.await;

        // Restore normal mode for this directory
        crate::proc::simulate_crash_for_dir(&run_dir, false);

        // Grandchild is still running in the background!
        assert_eq!(unsafe { libc::kill(gc_pid, 0) }, 0, "grandchild survived the crash");
        assert!(children_file.exists(), "children.json remains on disk after crash");

        // Tokage restarts: startup logic calls cleanup_leftovers(&run_dir)
        let cleaned = crate::proc::cleanup_leftovers(&run_dir);
        assert!(!cleaned.is_empty(), "cleanup_leftovers must reap lingering process groups");

        // Verify grandchild is now terminated
        assert!(gone(gc_pid).await, "grandchild was terminated on Tokage restart");
        assert!(!children_file.exists(), "children.json must be removed after cleanup");
    }

    /// A write outside the workspace is reported with its absolute path, so it can be
    /// checked against the task's write scope (docs/09 §4).
    #[tokio::test]
    async fn outside_write_is_reported() {
        let work = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let env = env(work.path(), &[]);
        let target = outside.path().join("escape.txt");
        let fake = FakeAdapter::new(
            ProviderId::ClaudeCode,
            vec![
                Step::Write { path: target.clone(), text: "x\n".into() },
                Step::Emit(serde_json::json!({"fake": "done"})),
            ],
        );
        let r = run(&fake, &env, work.path()).await;
        assert!(r.result.is_ok(), "{:?}", r.result);
        let abs = r
            .events
            .iter()
            .find_map(|e| match e {
                EventKind::FileChange { abs_path, .. } => Some(PathBuf::from(abs_path)),
                _ => None,
            })
            .expect("file change event");
        let work = work.path().canonicalize().unwrap();
        assert!(!abs.starts_with(&work), "{abs:?}");
        assert_eq!(abs, target.canonicalize().unwrap());
    }

    /// Reading a credential path shows up as a tool step the user (and later guards) can see.
    #[tokio::test]
    async fn credential_read_is_visible() {
        let dir = tempfile::tempdir().unwrap();
        let env = env(dir.path(), &[]);
        let creds = dir.path().join(".aws/credentials");
        std::fs::create_dir_all(creds.parent().unwrap()).unwrap();
        std::fs::write(&creds, "[default]\naws_secret_access_key = SECRET\n").unwrap();
        let fake = FakeAdapter::new(
            ProviderId::ClaudeCode,
            vec![Step::Read { path: creds.clone() }, Step::Emit(serde_json::json!({"fake": "done"}))],
        );
        let r = run(&fake, &env, dir.path()).await;
        assert!(r.events.iter().any(
            |e| matches!(e, EventKind::ToolStart { title, .. } if title.contains(".aws/credentials"))
        ));
        let dump = format!("{:?}", r.events);
        assert!(!dump.contains("SECRET"), "file content must not reach events");
    }

    /// R16 end to end: a CLI's process sees its own key, never another vendor's.
    #[tokio::test]
    async fn printed_env_holds_only_own_keys() {
        let dir = tempfile::tempdir().unwrap();
        let env = env(
            dir.path(),
            &[
                ("ANTHROPIC_API_KEY", "sk-ant-own"),
                ("OPENAI_API_KEY", "sk-openai-other"),
                ("GITHUB_TOKEN", "ghp-other"),
                ("CLAUDECODE", "1"),
            ],
        );
        let fake = FakeAdapter::new(ProviderId::ClaudeCode, vec![Step::PrintEnv]);
        let mut req = crate::chat::testing::request(ProviderId::ClaudeCode);
        req.cwd = dir.path().display().to_string();
        // The real launch's host-session removals apply to the fake too.
        let mut launch = fake.launch(&req, "p".into());
        launch.env_remove = adapter::adapter(ProviderId::ClaudeCode)
            .unwrap()
            .launch(&req, String::new())
            .env_remove;
        let events = Mutex::new(vec![]);
        let emit = |k: EventKind| events.lock().unwrap().push(k);
        let r = run_cli(&FakeAdapter::exe(), &req, launch, &env, None, None, &emit, None).await;
        assert!(r.is_ok(), "{r:?}");
        let out = texts(&events.into_inner().unwrap());
        assert!(out.contains("ANTHROPIC_API_KEY=[REDACTED]"), "{out}");
        assert!(!out.contains("OPENAI_API_KEY") && !out.contains("GITHUB_TOKEN"), "{out}");
        assert!(!out.contains("CLAUDECODE="), "{out}");
    }

    /// P2.m3: A denied command or tool emits an ApprovalRequest and does not crash the turn.
    #[tokio::test]
    async fn denial_emits_approval_request() {
        let dir = tempfile::tempdir().unwrap();
        let env = env(dir.path(), &[]);
        let fake = FakeAdapter::new(
            ProviderId::ClaudeCode,
            vec![
                Step::Denial {
                    tool_id: "call_1".into(),
                    command: "rm -rf build".into(),
                    reason: "disallowed_tool".into(),
                },
            ],
        );
        let r = run(&fake, &env, dir.path()).await;
        assert!(r.result.is_ok(), "turn with approval request should not return Err");
        let approval = r.events.iter().find(|e| matches!(e, EventKind::ApprovalRequest { .. }));
        assert!(approval.is_some(), "expected ApprovalRequest in events");
        if let Some(EventKind::ApprovalRequest { command, tool_id, .. }) = approval {
            assert_eq!(command, "rm -rf build");
            assert_eq!(tool_id.as_deref(), Some("call_1"));
        }
    }

    /// Acceptance test (R13): When a forbidden command (e.g. `sudo rm -rf /`, credential dump)
    /// is denied/intercepted by the CLI, Tokage classifies it as forbidden and rejects it
    /// with an Error event. Forbidden commands NEVER produce an ApprovalRequest.
    #[tokio::test]
    async fn forbidden_command_denial_is_rejected_without_approval_request() {
        let dir = tempfile::tempdir().unwrap();
        let env = env(dir.path(), &[]);
        let fake = FakeAdapter::new(
            ProviderId::ClaudeCode,
            vec![
                Step::Denial {
                    tool_id: "call_forbidden".into(),
                    command: "sudo rm -rf /".into(),
                    reason: "disallowed_tool".into(),
                },
            ],
        );
        let r = run(&fake, &env, dir.path()).await;
        assert!(r.result.is_err(), "forbidden command should fail the turn");

        // 1. ApprovalRequest must NOT be emitted for a forbidden command
        let approval = r.events.iter().find(|e| matches!(e, EventKind::ApprovalRequest { .. }));
        assert!(approval.is_none(), "forbidden command should never produce ApprovalRequest, got: {:?}", approval);

        // 2. An Error event rejecting the forbidden redline must be emitted
        let error_ev = r.events.iter().find(|e| matches!(e, EventKind::Error { .. }));
        assert!(error_ev.is_some(), "expected Error event for forbidden command, got: {:?}", r.events);
        if let Some(EventKind::Error { message }) = error_ev {
            assert!(message.contains("Forbidden redline command rejected (R13)"));
            assert!(message.contains("sudo rm -rf /"));
        }
    }

    /// Acceptance test (P2.m4, P2.m7, R14, R15): Under OS sandbox (macOS / Linux), FakeAdapter's attempts
    /// to read credential redline paths (~/.ssh, ~/.aws, other CLI's credentials) are intercepted
    /// at the OS level (Operation not permitted), while reading allowed workspace files succeeds.
    #[tokio::test]
    async fn sandbox_intercepts_credential_redline_reads() {
        let home_dir = tempfile::tempdir().unwrap();
        let work_dir = tempfile::tempdir().unwrap();
        let env = env(home_dir.path(), &[]);

        // 1. Create allowed file in workspace
        let work_file = work_dir.path().join("code.rs");
        std::fs::write(&work_file, "fn main() {}\n").unwrap();

        // 2. Create forbidden credential files under HOME
        let ssh_key = home_dir.path().join(".ssh/id_rsa");
        std::fs::create_dir_all(ssh_key.parent().unwrap()).unwrap();
        std::fs::write(&ssh_key, "SUPER_SECRET_SSH_KEY\n").unwrap();

        let aws_cred = home_dir.path().join(".aws/credentials");
        std::fs::create_dir_all(aws_cred.parent().unwrap()).unwrap();
        std::fs::write(&aws_cred, "aws_secret_access_key = AWS_SECRET\n").unwrap();

        let codex_auth = home_dir.path().join(".codex/auth.json");
        std::fs::create_dir_all(codex_auth.parent().unwrap()).unwrap();
        std::fs::write(&codex_auth, "{\"token\": \"CODEX_SECRET\"}\n").unwrap();

        // Impersonate Claude Code (which has Caps::OS_SANDBOX)
        let fake = FakeAdapter::new(
            ProviderId::ClaudeCode,
            vec![
                Step::ProbeRead { path: work_file },
                Step::ProbeRead { path: ssh_key },
                Step::ProbeRead { path: aws_cred },
                Step::ProbeRead { path: codex_auth },
                Step::Emit(serde_json::json!({"fake": "done"})),
            ],
        );

        let r = run(&fake, &env, work_dir.path()).await;
        assert!(r.result.is_ok(), "{:?}", r.result);

        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            let output = texts(&r.events);
            assert!(
                output.contains("READ_ALLOWED"),
                "workspace file should be readable under sandbox, got: {output}"
            );
            assert!(
                output.contains("READ_BLOCKED"),
                "credential redlines should be blocked by sandbox, got: {output}"
            );
            let blocked_count = output.matches("READ_BLOCKED").count();
            assert_eq!(
                blocked_count, 3,
                "all 3 credential redline paths must be blocked by sandbox: {output}"
            );
        }

        // Verify secrets never leaked into events dump
        let dump = format!("{:?}", r.events);
        assert!(!dump.contains("SUPER_SECRET_SSH_KEY"));
        assert!(!dump.contains("AWS_SECRET"));
        assert!(!dump.contains("CODEX_SECRET"));
    }

    /// P2.m4e verification: CodexCli has Caps::OWN_SANDBOX and is NOT wrapped in outer sandbox-exec.
    #[test]
    fn codex_uses_own_sandbox_not_os_sandbox() {
        use crate::chat::adapter::Caps;

        let codex = adapter::adapter(ProviderId::CodexCli).unwrap().meta();
        assert!(codex.caps.has(Caps::OWN_SANDBOX));
        assert!(!codex.caps.has(Caps::OS_SANDBOX));

        let claude = adapter::adapter(ProviderId::ClaudeCode).unwrap().meta();
        assert!(claude.caps.has(Caps::OS_SANDBOX));
        assert!(!claude.caps.has(Caps::OWN_SANDBOX));
    }

    /// Acceptance test (P2.m5d, R16, R17): FakeAdapter printing its environment has all known
    /// credentials and regex-matched API keys redacted to [REDACTED]. Plaintext keys never leak into events.
    #[tokio::test]
    async fn print_env_redaction_prevents_secret_leak() {
        let dir = tempfile::tempdir().unwrap();
        let env = env(
            dir.path(),
            &[
                ("ANTHROPIC_API_KEY", "sk-ant-api03-abcdefghijklmnopqrstuvwxyz1234567890"),
                ("CUSTOM_SECRET_KEY", "custom_secret_value_12345678"),
            ],
        );
        let fake = FakeAdapter::new(ProviderId::ClaudeCode, vec![Step::PrintEnv]);
        let r = run(&fake, &env, dir.path()).await;
        assert!(r.result.is_ok(), "{:?}", r.result);

        let out = texts(&r.events);
        assert!(!out.contains("sk-ant-api03-abcdefghijklmnopqrstuvwxyz1234567890"), "raw API key leaked in: {out}");
        assert!(!out.contains("custom_secret_value_12345678"), "custom secret leaked in: {out}");
        assert!(out.contains("[REDACTED]"), "expected [REDACTED] in output, got: {out}");

        let dump = format!("{:?}", r.events);
        assert!(!dump.contains("sk-ant-api03-abcdefghijklmnopqrstuvwxyz1234567890"));
        assert!(!dump.contains("custom_secret_value_12345678"));
    }

    /// Acceptance test (P2.m5d, R17): Reading key files or emitting tool outputs with secrets
    /// are completely redacted in events.
    #[tokio::test]
    async fn key_output_and_reads_are_redacted() {
        let dir = tempfile::tempdir().unwrap();
        let env = env(dir.path(), &[]);
        let fake = FakeAdapter::new(
            ProviderId::ClaudeCode,
            vec![
                Step::Emit(serde_json::json!({
                    "fake": "text",
                    "text": "Found leaked GitHub PAT: ghp_123456789012345678901234567890123456 in file"
                })),
                Step::Emit(serde_json::json!({
                    "fake": "toolEnd",
                    "id": "t1",
                    "output": "AWS key is AKIAIOSFODNN7EXAMPLE and OpenAI is sk-proj-12345678901234567890"
                })),
                Step::Emit(serde_json::json!({"fake": "done"})),
            ],
        );
        let r = run(&fake, &env, dir.path()).await;
        assert!(r.result.is_ok(), "{:?}", r.result);

        let dump = format!("{:?}", r.events);
        assert!(!dump.contains("ghp_123456789012345678901234567890123456"));
        assert!(!dump.contains("AKIAIOSFODNN7EXAMPLE"));
        assert!(!dump.contains("sk-proj-12345678901234567890"));
        assert!(dump.contains("[REDACTED]"));
    }

    /// Acceptance test (P2.m5e, R17): When diff introduces an API key into workspace files,
    /// a Warning event is triggered and the diff payload is redacted.
    #[tokio::test]
    async fn diff_with_secret_triggers_warning_and_redaction() {
        let dir = tempfile::tempdir().unwrap();
        let env = env(dir.path(), &[]);

        // Initialize git repo so Files tracking can compute unified diff against HEAD
        let g = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(args)
                .output()
                .unwrap();
        };
        g(&["init", "-q"]);
        std::fs::write(dir.path().join("secrets.js"), "// initial\n").unwrap();
        g(&["add", "."]);
        g(&["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", "init"]);

        let target = dir.path().join("secrets.js");
        let fake = FakeAdapter::new(
            ProviderId::ClaudeCode,
            vec![
                Step::Write {
                    path: target.clone(),
                    text: "const apiKey = 'sk-ant-api03-abcdefghijklmnopqrstuvwxyz1234567890';\n".into(),
                },
                Step::Emit(serde_json::json!({"fake": "done"})),
            ],
        );
        let r = run(&fake, &env, dir.path()).await;
        assert!(r.result.is_ok(), "{:?}", r.result);

        // 1. Verify warning event was emitted
        let warning = r.events.iter().find(|e| matches!(e, EventKind::Warning { .. }));
        assert!(warning.is_some(), "expected Warning event for key in diff, found: {:?}", r.events);
        if let Some(EventKind::Warning { message }) = warning {
            assert!(message.contains("sensitive credential or API key"));
        }

        // 2. Verify FileChange diff is redacted
        let file_change = r.events.iter().find(|e| matches!(e, EventKind::FileChange { .. }));
        assert!(file_change.is_some(), "expected FileChange event");
        if let Some(EventKind::FileChange { diff: Some(diff), .. }) = file_change {
            assert!(diff.contains("[REDACTED]"), "diff should be redacted: {diff}");
            assert!(!diff.contains("sk-ant-api03-abcdefghijklmnopqrstuvwxyz1234567890"), "diff must not leak raw key");
        }

        // 3. No plaintext secret in whole events dump
        let dump = format!("{:?}", r.events);
        assert!(!dump.contains("sk-ant-api03-abcdefghijklmnopqrstuvwxyz1234567890"));
    }

    /// Acceptance test (P2.m6, R14, R15): In full_auto mode with OS sandbox active,
    /// a command attempting to `rm` or write to a path outside the workspace is intercepted
    /// at the OS kernel level, leaving the outside file intact, while workspace file removal succeeds.
    #[tokio::test]
    async fn full_auto_rm_outside_workspace_is_blocked_by_sandbox() {
        let home_dir = tempfile::tempdir().unwrap();
        let work_dir = tempfile::tempdir().unwrap();
        let env = env(home_dir.path(), &[]);

        // 1. Create a file inside workspace
        let inside_file = work_dir.path().join("inside.txt");
        std::fs::write(&inside_file, "inside content\n").unwrap();

        // 2. Create a victim file outside workspace (in /var/tmp, which is neither worktree nor temp_dir)
        let outside_dir = tempfile::Builder::new()
            .prefix("tokage-victim-")
            .tempdir_in("/var/tmp")
            .unwrap();
        let outside_file = outside_dir.path().join("victim.txt");
        std::fs::write(&outside_file, "do not delete\n").unwrap();

        let cmd_inside = format!("rm {}", path(&inside_file));
        let cmd_outside = format!("rm {}", path(&outside_file));

        let fake = FakeAdapter::new(
            ProviderId::ClaudeCode,
            vec![
                Step::ProbeCommand {
                    cmd: cmd_inside,
                    id: "inside_rm".into(),
                },
                Step::ProbeCommand {
                    cmd: cmd_outside,
                    id: "outside_rm".into(),
                },
                Step::Emit(serde_json::json!({"fake": "done"})),
            ],
        );

        let r = run(&fake, &env, work_dir.path()).await;
        assert!(r.result.is_ok(), "{:?}", r.result);

        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            let output = texts(&r.events);
            assert!(
                output.contains("inside_rm:SUCCESS"),
                "workspace file removal should succeed: {output}"
            );
            assert!(
                output.contains("outside_rm:BLOCKED"),
                "outside workspace removal should be blocked by sandbox: {output}"
            );
            assert!(
                !inside_file.exists(),
                "inside file should have been deleted"
            );
            assert!(
                outside_file.exists(),
                "outside file must NOT be deleted by sandbox-blocked command"
            );
            assert_eq!(
                std::fs::read_to_string(&outside_file).unwrap(),
                "do not delete\n"
            );
        }
    }

    /// Acceptance test (P2.m6b, P2.m6c): Full-auto mode enforces that:
    /// - The CLI must support OS sandbox
    /// - The turn must run in a linked git worktree
    #[tokio::test]
    async fn full_auto_enforces_worktree_and_sandbox() {
        use tokage_core::auth::AuthMode;
        use tokage_core::bus::Bus;

        let repo_dir = tempfile::tempdir().unwrap();
        let env = env(repo_dir.path(), &[]);

        // Initialize git repo (main checkout, NOT a linked worktree)
        let g = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(repo_dir.path())
                .args(args)
                .output()
                .unwrap();
        };
        g(&["init", "-q"]);
        std::fs::write(repo_dir.path().join("file.txt"), "hello").unwrap();
        g(&["add", "."]);
        g(&["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", "init"]);

        let bus = Bus::new();
        let mut rx = bus.subscribe();

        // 1. Non-sandboxed CLI in full_auto -> Error
        let mut req_nosandbox = crate::chat::testing::request(ProviderId::CodexCli);
        req_nosandbox.cwd = repo_dir.path().display().to_string();
        req_nosandbox.auth_mode = Some(AuthMode::FullAuto);
        req_nosandbox.trusted = vec![ProviderId::CodexCli];

        let mut p_info = crate::providers::detect_one(&env, ProviderId::CodexCli, Some("/bin/sh"))
            .await
            .expect("Codex info");
        p_info.status = crate::providers::ProviderStatus::Ready;

        crate::chat::run_turn(&bus, req_nosandbox, vec![p_info.clone()], env.clone(), None, None)
            .await;

        let mut error_msg = None;
        while let Ok(ev) = rx.try_recv() {
            if let tokage_core::bus::DomainEvent::ChatOutput { event: EventKind::Error { message }, .. } = ev {
                error_msg = Some(message);
                break;
            }
        }
        assert!(error_msg.is_some(), "expected Error event for non-sandboxed CLI in full_auto");
        assert!(error_msg.unwrap().contains("OS sandboxed CLI"));

        // 2. Sandboxed CLI in non-worktree git repo -> Error
        let mut req_claude = crate::chat::testing::request(ProviderId::ClaudeCode);
        req_claude.cwd = repo_dir.path().display().to_string();
        req_claude.auth_mode = Some(AuthMode::FullAuto);
        req_claude.trusted = vec![ProviderId::ClaudeCode];

        let mut p_claude = crate::providers::detect_one(&env, ProviderId::ClaudeCode, Some("/bin/sh"))
            .await
            .expect("Claude info");
        p_claude.status = crate::providers::ProviderStatus::Ready;

        crate::chat::run_turn(&bus, req_claude, vec![p_claude], env.clone(), None, None).await;

        let mut wt_error = None;
        while let Ok(ev) = rx.try_recv() {
            if let tokage_core::bus::DomainEvent::ChatOutput { event: EventKind::Error { message }, .. } = ev {
                wt_error = Some(message);
                break;
            }
        }
        assert!(wt_error.is_some(), "expected Error event for non-worktree directory in full_auto");
        assert!(wt_error.unwrap().contains("git worktree"));
    }

    /// Acceptance test (P3.m4b, R7): A PLANNER window attempting to write a file is immediately
    /// terminated and produces a security warning and error event.
    #[tokio::test]
    async fn planner_window_file_write_is_terminated_r7() {
        let work = tempfile::tempdir().unwrap();
        let env = env(work.path(), &[]);
        let target = work.path().join("modified_by_planner.txt");
        let fake = FakeAdapter::new(
            ProviderId::ClaudeCode,
            vec![
                Step::Write { path: target.clone(), text: "forbidden write in planner".into() },
                Step::Emit(serde_json::json!({"fake": "done"})),
            ],
        );
        let mut req = crate::chat::testing::request(ProviderId::ClaudeCode);
        req.cwd = work.path().display().to_string();
        req.role = Some(Role::Planner);

        let r = run_req(&fake, &env, req).await;
        assert!(r.result.is_err(), "expected error result when planner attempts to write");
        let err = r.result.unwrap_err();
        assert!(err.contains("(R7)"), "expected R7 reference in error: {err}");

        let has_warning = r.events.iter().any(|e| match e {
            EventKind::Warning { message } => message.contains("(R7)") && message.contains("PLANNER"),
            _ => false,
        });
        assert!(has_warning, "expected Warning event citing R7 and PLANNER in: {:?}", r.events);

        let has_error = r.events.iter().any(|e| match e {
            EventKind::Error { message } => message.contains("(R7)") && message.contains("PLANNER"),
            _ => false,
        });
        assert!(has_error, "expected Error event citing R7 and PLANNER in: {:?}", r.events);
    }

    /// Acceptance test (P3.m4b, R7): A REVIEWER window is also strictly read-only and terminates
    /// immediately upon attempting any file modification.
    #[tokio::test]
    async fn reviewer_window_file_write_is_terminated_r7() {
        let work = tempfile::tempdir().unwrap();
        let env = env(work.path(), &[]);
        let target = work.path().join("modified_by_reviewer.txt");
        let fake = FakeAdapter::new(
            ProviderId::CodexCli,
            vec![
                Step::Write { path: target.clone(), text: "forbidden write in reviewer".into() },
                Step::Emit(serde_json::json!({"fake": "done"})),
            ],
        );
        let mut req = crate::chat::testing::request(ProviderId::CodexCli);
        req.cwd = work.path().display().to_string();
        req.role = Some(Role::Reviewer);

        let r = run_req(&fake, &env, req).await;
        assert!(r.result.is_err(), "expected error result when reviewer attempts to write");
        let err = r.result.unwrap_err();
        let has_warning = r.events.iter().any(|e| match e {
            EventKind::Warning { message } => message.contains("(R7)") && message.contains("REVIEWER"),
            _ => false,
        });
        assert!(has_warning, "expected Warning event citing R7 and REVIEWER in: {:?}", r.events);
        assert!(err.contains("(R7)") || has_warning, "expected R7 reference in error or warning: {err}");
    }

    /// External disk modification during a PLANNER turn is caught by
    /// files.finish at the end of the turn; it emits a warning and fails the read-only turn.
    #[tokio::test]
    async fn planner_window_disk_change_fails_without_done() {
        let work = tempfile::tempdir().unwrap();
        // Initialize git repo so git status detection in files.finish can detect changes
        let env = env(work.path(), &[]);
        let _ = tokio::process::Command::new("git")
            .args(["init"])
            .current_dir(work.path())
            .output()
            .await;
        let _ = tokio::process::Command::new("git")
            .args(["config", "user.name", "Tester"])
            .current_dir(work.path())
            .output()
            .await;
        let _ = tokio::process::Command::new("git")
            .args(["config", "user.email", "tester@test.com"])
            .current_dir(work.path())
            .output()
            .await;

        let silent_file = work.path().join("silent_edit.txt");
        let fake = FakeAdapter::new(
            ProviderId::ClaudeCode,
            vec![
                // Direct shell write without emitting a `{"fake": "file"}` event
                Step::Emit(serde_json::json!({"fake": "text", "text": "planning..."})),
                Step::ProbeCommand {
                    cmd: format!("touch {}", silent_file.display()),
                    id: "silent_touch".into(),
                },
                Step::Emit(serde_json::json!({"fake": "done"})),
            ],
        );
        let mut req = crate::chat::testing::request(ProviderId::ClaudeCode);
        req.cwd = work.path().display().to_string();
        req.role = Some(Role::Planner);

        let r = run_req(&fake, &env, req).await;
        assert!(r.result.is_err(), "read-only disk changes must fail the turn");
        assert!(!r.events.iter().any(|e| matches!(e, EventKind::Done { .. })));

        let has_warning = r.events.iter().any(|e| match e {
            EventKind::Warning { message } => {
                message.contains("external file modification") && message.contains("PLANNER")
            }
            _ => false,
        });
        assert!(has_warning, "expected Warning event citing external file modification in: {:?}", r.events);

        let has_error = r.events.iter().any(|e| matches!(e, EventKind::Error { .. }));
        assert!(!has_error, "expected no Error event for external disk modification: {:?}", r.events);
    }

    /// P3.m4a: EXECUTOR within write_scope succeeds without warnings.
    #[tokio::test]
    async fn executor_writes_within_write_scope_allowed() {
        let work = tempfile::tempdir().unwrap();
        let env = env(work.path(), &[]);
        let src_dir = work.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        let target = src_dir.join("main.rs");

        let fake = FakeAdapter::new(
            ProviderId::ClaudeCode,
            vec![
                Step::Write { path: target.clone(), text: "fn main() {}".into() },
                Step::Emit(serde_json::json!({"fake": "done"})),
            ],
        );
        let mut req = crate::chat::testing::request(ProviderId::ClaudeCode);
        req.cwd = work.path().display().to_string();
        req.role = Some(Role::Executor);
        req.write_scope = vec!["src/**".into()];

        let r = run_req(&fake, &env, req).await;
        assert!(r.result.is_ok(), "{:?}", r.result);

        let has_scope_warning = r.events.iter().any(|e| match e {
            EventKind::Warning { message } => message.contains("outside write_scope"),
            _ => false,
        });
        assert!(!has_scope_warning, "should not emit outside_write_scope warning for in-scope write");
    }

    /// P3.m4a: EXECUTOR write outside write_scope triggers a warning.
    #[tokio::test]
    async fn executor_writes_outside_write_scope_warns() {
        let work = tempfile::tempdir().unwrap();
        let env = env(work.path(), &[]);
        let target = work.path().join("README.md");

        let fake = FakeAdapter::new(
            ProviderId::ClaudeCode,
            vec![
                Step::Write { path: target.clone(), text: "# New Readme".into() },
                Step::Emit(serde_json::json!({"fake": "done"})),
            ],
        );
        let mut req = crate::chat::testing::request(ProviderId::ClaudeCode);
        req.cwd = work.path().display().to_string();
        req.role = Some(Role::Executor);
        req.write_scope = vec!["src/**".into()];

        let r = run_req(&fake, &env, req).await;
        assert!(r.result.is_ok(), "{:?}", r.result);

        let has_scope_warning = r.events.iter().any(|e| match e {
            EventKind::Warning { message } => message.contains("outside write_scope"),
            _ => false,
        });
        assert!(has_scope_warning, "expected outside write_scope warning in: {:?}", r.events);
    }

    /// P3.m4a: TESTER role denies sensitive commands directly without asking for approval.
    #[tokio::test]
    async fn tester_role_sensitive_command_denied() {
        let work = tempfile::tempdir().unwrap();
        let env = env(work.path(), &[]);
        let fake = FakeAdapter::new(
            ProviderId::ClaudeCode,
            vec![
                Step::Denial {
                    tool_id: "t_test".into(),
                    command: "rm -rf node_modules".into(),
                    reason: "disallowed".into(),
                },
                Step::Emit(serde_json::json!({"fake": "done"})),
            ],
        );
        let mut req = crate::chat::testing::request(ProviderId::ClaudeCode);
        req.cwd = work.path().display().to_string();
        req.role = Some(Role::Tester);

        let r = run_req(&fake, &env, req).await;
        assert!(r.result.is_err(), "{:?}", r.result);
        assert!(!r.events.iter().any(|e| matches!(e, EventKind::Done { .. })));

        let has_approval = r.events.iter().any(|e| matches!(e, EventKind::ApprovalRequest { .. }));
        assert!(!has_approval, "Tester role should not ask for approval on sensitive commands");

        let has_error = r.events.iter().any(|e| match e {
            EventKind::Error { message } => message.contains("tester_role"),
            _ => false,
        });
        assert!(has_error, "expected Error event citing tester_role denial in: {:?}", r.events);
    }

    /// P3.m7a/P3.m7d: Quota events from JSON streams are emitted and update Account.usage.
    #[tokio::test]
    async fn quota_event_is_emitted_and_updates_account_usage() {
        let work = tempfile::tempdir().unwrap();
        let env = env(work.path(), &[]);
        let fake = FakeAdapter::new(
            ProviderId::ClaudeCode,
            vec![
                Step::Emit(serde_json::json!({
                    "fake": "quota",
                    "raw": "Hit your 5-hour limit · resets 3pm",
                    "reset_at": "2026-10-05T20:00:00Z",
                    "retrying": false
                })),
                Step::Emit(serde_json::json!({"fake": "done"})),
            ],
        );
        let mut req = crate::chat::testing::request(ProviderId::ClaudeCode);
        req.cwd = work.path().display().to_string();

        let r = run_req(&fake, &env, req).await;
        assert!(r.result.is_ok(), "{:?}", r.result);

        let quota_ev = r.events.iter().find_map(|e| match e {
            EventKind::Quota { raw, reset_at, retrying } => Some((raw, reset_at, retrying)),
            _ => None,
        });
        assert!(quota_ev.is_some(), "Expected Quota event in: {:?}", r.events);
        let (raw, reset, retrying) = quota_ev.unwrap();
        assert!(raw.contains("5-hour limit"));
        assert_eq!(reset.as_deref(), Some("2026-10-05T20:00:00Z"));
        assert!(!retrying);

        let cached = crate::usage::get_cached_usage(ProviderId::ClaudeCode);
        assert!(cached.is_some(), "Expected cached usage for ClaudeCode");
        let u = cached.unwrap();
        assert_eq!(u.source, crate::usage::UsageSource::CliOutput);
    }

    /// P3.m7a: Stderr tail with rate limit / 429 triggers fallback Quota event.
    #[tokio::test]
    async fn stderr_quota_fallback_triggers_quota_event() {
        let work = tempfile::tempdir().unwrap();
        let env = env(work.path(), &[]);
        // A fake script that outputs an error to stderr and exits with failure
        let fake = FakeAdapter::new(
            ProviderId::CodexCli,
            vec![
                Step::Emit(serde_json::json!({
                    "fake": "fail",
                    "message": "Codex error: 429 Too Many Requests, try again in 15m"
                })),
            ],
        );
        let mut req = crate::chat::testing::request(ProviderId::CodexCli);
        req.cwd = work.path().display().to_string();

        let r = run_req(&fake, &env, req).await;
        assert!(r.result.is_err());

        let quota_ev = r.events.iter().find(|e| matches!(e, EventKind::Quota { .. }));
        assert!(quota_ev.is_some(), "Expected fallback Quota event in: {:?}; result: {:?}", r.events, r.result);
    }
}



