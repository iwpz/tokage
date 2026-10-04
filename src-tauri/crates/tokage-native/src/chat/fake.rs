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
    /// Dump the environment the CLI was started with, as plain text.
    PrintEnv,
    Exit(i32),
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
        let mut lines = vec![];
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
                Step::PrintEnv => "env".into(),
                Step::Exit(code) => format!("exit {code}"),
            };
            lines.push(line);
        }
        lines.join("\n")
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
                cost_usd: None,
                premium_requests: None,
            }),
            "fail" => out.fail(str_of("message")),
            _ => {}
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
    use crate::{chat::run_cli, env::ShellEnv};

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
        let mut req = crate::chat::testing::request(fake.provider);
        req.cwd = cwd.display().to_string();
        let events = Mutex::new(vec![]);
        let emit = |k: EventKind| events.lock().unwrap().push(k);
        let launch = fake.launch(&req, "p".into());
        let result = run_cli(&FakeAdapter::exe(), fake.provider, launch, env, &req.cwd, None, &emit).await;
        Run {
            result,
            events: events.into_inner().unwrap(),
        }
    }

    fn pid(file: &Path) -> i32 {
        std::fs::read_to_string(file).unwrap().trim().parse().unwrap()
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
        assert_eq!(texts(&r.events), "noise noise noise\n".repeat(3).trim_end());
    }

    /// A flood ends normally with memory bounded by the plain-text cap.
    #[tokio::test]
    async fn flood_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let env = env(dir.path(), &[]);
        let fake = FakeAdapter::new(ProviderId::CodexCli, vec![Step::Flood { lines: 200_000 }]);
        let r = run(&fake, &env, dir.path()).await;
        assert!(r.result.is_ok());
        assert!(texts(&r.events).len() <= super::super::PLAIN_MAX + 8);
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
        let r = tokio::time::timeout(Duration::from_millis(800), run(&fake, &env, dir.path())).await;
        assert!(r.is_err(), "a hung CLI never finishes on its own");
        assert!(gone(pid(&pidfile)).await, "hung CLI survived the stop");
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
        let r = run_cli(&FakeAdapter::exe(), fake.provider, launch, &env, &req.cwd, None, &emit).await;
        assert!(r.is_ok(), "{r:?}");
        let out = texts(&events.into_inner().unwrap());
        assert!(out.contains("ANTHROPIC_API_KEY=sk-ant-own"), "{out}");
        assert!(!out.contains("sk-openai-other") && !out.contains("ghp-other"), "{out}");
        assert!(!out.contains("CLAUDECODE="), "{out}");
    }
}
