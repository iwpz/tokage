//! Transport-independent direct execution. Intent and events are durable before
//! inference/output; an abandoned intent is never automatically replayed.
use crate::{
    chat::{self, ChatRequest, TurnMode},
    env::ShellEnv,
    file_lease::FileLease,
    providers::ProviderInfo,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};
use tokage_core::{Bus, DomainEvent, EventKind};
use tokio::sync::watch;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn key(s: &str) -> String {
    format!("{:x}", Sha256::digest(s.as_bytes()))
}
fn private_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(err)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(err)?;
    }
    Ok(())
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Used by all native write-capable turns, including pipeline executors.
/// Canonical paths make aliases contend; linked worktrees remain independent.
pub fn workspace_lease(cwd: &str) -> Result<FileLease, String> {
    let cwd = fs::canonicalize(cwd).map_err(err)?;
    #[cfg(unix)]
    let namespace = format!("tokage-workspace-leases-{}", unsafe { libc::geteuid() });
    #[cfg(not(unix))]
    let namespace = "tokage-workspace-leases";
    let root = std::env::temp_dir().join(namespace);
    private_dir(&root)?;
    FileLease::acquire(&root.join(key(&cwd.to_string_lossy())))
        .map_err(|_| "Another agent is using this working directory; wait for it to finish".into())
}

pub fn session_lease(data: &Path, session: &str) -> Result<FileLease, String> {
    private_dir(data)?;
    FileLease::acquire(&data.join(format!("session-owner-{}.lock", key(session))))
        .map_err(|_| "This session already has an execution owner".to_string())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Record {
    pub schema_version: u32,
    pub turn_id: String,
    pub session_id: String,
    pub cwd: String,
    pub request: serde_json::Value,
    pub status: String,
    pub created_at: u64,
    pub finished_at: Option<u64>,
    pub error: Option<String>,
    pub process_directory: Option<PathBuf>,
}
fn record_path(root: &Path, id: &str) -> PathBuf {
    root.join(format!("{}.json", key(id)))
}
fn save(root: &Path, record: &Record) -> Result<(), String> {
    let mut temp = tempfile::NamedTempFile::new_in(root).map_err(err)?;
    temp.write_all(&serde_json::to_vec(record).map_err(err)?)
        .map_err(err)?;
    temp.as_file().sync_all().map_err(err)?;
    temp.persist(record_path(root, &record.turn_id))
        .map_err(err)?;
    #[cfg(unix)]
    File::open(root).and_then(|f| f.sync_all()).map_err(err)?;
    Ok(())
}
pub fn list(root: &Path, session: Option<&str>) -> Result<Vec<Record>, String> {
    if !root.exists() {
        return Ok(vec![]);
    }
    let mut records = vec![];
    for entry in fs::read_dir(root).map_err(err)? {
        let path = entry.map_err(err)?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let record: Record = serde_json::from_slice(&fs::read(&path).map_err(err)?).map_err(err)?;
        if record.schema_version != 1 || record_path(root, &record.turn_id) != path {
            return Err("Unknown direct execution schema or identity mismatch".into());
        }
        if session.is_none_or(|s| record.session_id == s) {
            records.push(record);
        }
    }
    records.sort_by(|a, b| (a.created_at, &a.turn_id).cmp(&(b.created_at, &b.turn_id)));
    Ok(records)
}
pub fn events(root: &Path, id: &str) -> Result<Vec<EventKind>, String> {
    let path = root.join(format!("{}.jsonl", key(id)));
    let text = fs::read_to_string(path).map_err(err)?;
    text.lines()
        .map(|line| serde_json::from_str(line).map_err(err))
        .collect()
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryTurn {
    pub record: Record,
    pub events: Vec<EventKind>,
    pub partial_tail: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoverySession {
    pub session_id: String,
    pub active: bool,
    pub tip: Option<String>,
    pub turns: Vec<RecoveryTurn>,
}

fn ordered(mut remaining: Vec<Record>) -> Result<Vec<Record>, String> {
    if remaining.iter().any(|r| {
        r.request
            .get("directTip")
            .and_then(|v| v.as_str())
            .is_some_and(|tip| !remaining.iter().any(|p| p.turn_id == tip))
    }) {
        return Err("Direct history is missing a predecessor".into());
    }
    let mut result = vec![];
    while !remaining.is_empty() {
        let index = remaining
            .iter()
            .position(|candidate| {
                candidate
                    .request
                    .get("directTip")
                    .and_then(|v| v.as_str())
                    .is_none_or(|tip| !remaining.iter().any(|r| r.turn_id == tip))
            })
            .ok_or("Cyclic direct turn history")?;
        result.push(remaining.remove(index));
    }
    Ok(result)
}

/// Read a stable closed-session snapshot. Active owners are reported without
/// returning a transcript that could be mistaken for a recoverable closed turn.
pub fn recovery(root: &Path, session: &str) -> Result<RecoverySession, String> {
    let mut snapshot = RecoverySession {
        session_id: session.into(),
        active: false,
        tip: None,
        turns: vec![],
    };
    if !root.exists() {
        return Ok(snapshot);
    }
    let data = root.parent().ok_or("Invalid direct data directory")?;
    let _lease =
        match FileLease::acquire(&data.join(format!("session-owner-{}.lock", key(session)))) {
            Ok(lease) => lease,
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || (cfg!(windows) && matches!(error.raw_os_error(), Some(32 | 33))) =>
            {
                snapshot.active = true;
                return Ok(snapshot);
            }
            Err(error) => return Err(err(error)),
        };
    let records = ordered(list(root, Some(session))?)?;
    // Turn IDs are random and timestamps have second resolution. Persisted UI
    // tip links establish ordering when several turns start in the same second.
    for record in records {
        let path = root.join(format!("{}.jsonl", key(&record.turn_id)));
        let bytes = fs::read(path).map_err(err)?;
        let partial_tail = !bytes.is_empty() && !bytes.ends_with(b"\n");
        let complete = if partial_tail {
            bytes
                .iter()
                .rposition(|b| *b == b'\n')
                .map_or(&[][..], |i| &bytes[..=i])
        } else {
            bytes.as_slice()
        };
        let text = std::str::from_utf8(complete).map_err(err)?;
        let events = text
            .lines()
            .map(|line| serde_json::from_str(line).map_err(err))
            .collect::<Result<Vec<EventKind>, String>>()?;
        if partial_tail && record.finished_at.is_some() {
            return Err("Closed direct turn has a truncated journal".into());
        }
        snapshot.tip = Some(record.turn_id.clone());
        snapshot.turns.push(RecoveryTurn {
            record,
            events,
            partial_tail,
        });
    }
    Ok(snapshot)
}

pub struct Execution {
    root: PathBuf,
    record: Record,
    journal: File,
    _session: FileLease,
}
impl Execution {
    pub fn prepare(root: &Path, req: &ChatRequest) -> Result<Self, String> {
        if req.mode != TurnMode::Direct || req.task_code.is_some() || req.plan_id.is_some() {
            return Err("Direct execution cannot own plan tasks".into());
        }
        let session = req
            .session_id
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or("Direct execution requires a session")?;
        let session_lease = session_lease(
            root.parent().ok_or("Invalid direct data directory")?,
            session,
        )?;
        let runtime = root
            .parent()
            .ok_or("Invalid direct data directory")?
            .join("runtime-v1/tokage.db");
        if runtime.exists() {
            let db = rusqlite::Connection::open_with_flags(
                runtime,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .map_err(err)?;
            let owned: bool = db
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM runtime_runs WHERE run_id=?1)",
                    [session],
                    |r| r.get(0),
                )
                .map_err(err)?;
            if owned {
                return Err(
                    "This session belongs to plan runtime; use a separate direct session".into(),
                );
            }
        }
        private_dir(root)?;
        let cwd = fs::canonicalize(&req.cwd)
            .map_err(err)?
            .to_string_lossy()
            .into_owned();
        let request = serde_json::to_value(req).map_err(err)?;
        let previous = list(root, None)?;
        if req.conversation.is_some() || req.direct_tip.is_some() {
            let same = ordered(
                previous
                    .iter()
                    .filter(|r| r.session_id == session)
                    .cloned()
                    .collect(),
            )?;
            let tip = same.last();
            if req.direct_tip.as_deref() != tip.map(|r| r.turn_id.as_str()) {
                return Err("Direct conversation is behind its durable history; restore it before continuing".into());
            }
        }
        for old in previous {
            if old.turn_id == req.turn_id {
                return Err(if old.request != request {
                    "Turn identity conflicts with an earlier request"
                } else {
                    "Turn already submitted; inspect its durable record instead of replaying it"
                }
                .into());
            }
            if old.finished_at.is_none() && (old.session_id == session || old.cwd == cwd) {
                return Err("Earlier execution is active or unresolved; inspect and reconcile it before sending again".into());
            }
        }
        let journal = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join(format!("{}.jsonl", key(&req.turn_id))))
            .map_err(err)?;
        let record = Record {
            schema_version: 1,
            turn_id: req.turn_id.clone(),
            session_id: session.into(),
            cwd,
            request,
            status: "prepared".into(),
            created_at: now(),
            finished_at: None,
            error: None,
            process_directory: req
                .process_registry_dir
                .clone()
                .or_else(|| Some(root.join("processes").join(key(&req.turn_id)))),
        };
        save(root, &record)?;
        Ok(Self {
            root: root.into(),
            record,
            journal,
            _session: session_lease,
        })
    }
    fn append(&mut self, event: &EventKind) -> Result<(), String> {
        self.journal
            .write_all(&serde_json::to_vec(event).map_err(err)?)
            .map_err(err)?;
        self.journal.write_all(b"\n").map_err(err)?;
        self.journal.sync_data().map_err(err)
    }
    pub async fn run(
        mut self,
        bus: &Bus,
        mut req: ChatRequest,
        providers: Vec<ProviderInfo>,
        env: ShellEnv,
        mirror: Option<PathBuf>,
        cancel: Option<watch::Receiver<bool>>,
    ) {
        let id = self.record.turn_id.clone();
        if serde_json::to_value(&req).ok().as_ref() != Some(&self.record.request) {
            self.record.status = "failed".into();
            self.record.error = Some("Direct execution request changed after preparation".into());
            self.record.finished_at = Some(now());
            let _ = save(&self.root, &self.record);
            bus.publish(DomainEvent::ChatOutput {
                turn_id: id.clone(),
                event: EventKind::Error {
                    message: "Direct execution request changed after preparation".into(),
                },
            });
            bus.publish(DomainEvent::ChatEnded {
                turn_id: id,
                cancelled: false,
            });
            return;
        }
        req.process_registry_dir = self.record.process_directory.clone();
        req.direct_execution = true;
        self.record.status = "running".into();
        if let Err(error) = save(&self.root, &self.record) {
            bus.publish(DomainEvent::ChatOutput {
                turn_id: id.clone(),
                event: EventKind::Error { message: error },
            });
            bus.publish(DomainEvent::ChatEnded {
                turn_id: id,
                cancelled: false,
            });
            return;
        }
        let local = Bus::new();
        let mut rx = local.subscribe();
        let (stop, stopped) = watch::channel(false);
        let process_directory = req
            .process_registry_dir
            .clone()
            .unwrap_or_else(|| self.root.join("processes").join(key(&id)));
        let runner = async {
            let relay = async {
                if let Some(mut external) = cancel {
                    loop {
                        if *external.borrow() {
                            let _ = stop.send(true);
                            break;
                        }
                        if external.changed().await.is_err() {
                            break;
                        }
                    }
                }
                std::future::pending::<()>().await;
            };
            let cancelled = tokio::select! {
                result = crate::proc::in_run_directory(process_directory,chat::run_turn(&local,req,providers,env,mirror,Some(stopped))) => result,
                _ = relay => unreachable!(),
            };
            local.publish(DomainEvent::ChatEnded {
                turn_id: id.clone(),
                cancelled,
            });
        };
        let recorder = async {
            let mut completion = None;
            let mut awaiting_approval = false;
            let mut failed = None;
            let mut persistence_error = None;
            while let Some(event) = rx.recv().await {
                match event {
                    DomainEvent::ChatOutput { event, .. } => {
                        if matches!(event, EventKind::ApprovalRequest { .. }) {
                            awaiting_approval = true;
                        }
                        if persistence_error.is_none() {
                            if let Err(error) = self.append(&event) {
                                persistence_error = Some(error);
                                let _ = stop.send(true);
                            }
                        }
                        if let EventKind::Error { message } = &event {
                            failed = Some(message.clone());
                            // Keep Stop available until the native process has
                            // actually exited; an error frame is not cleanup.
                            continue;
                        }
                        if matches!(event, EventKind::Done { .. }) {
                            completion = Some(event);
                        } else if persistence_error.is_none() {
                            bus.publish(DomainEvent::ChatOutput {
                                turn_id: id.clone(),
                                event,
                            });
                        }
                    }
                    DomainEvent::ChatEnded { cancelled, .. } => {
                        let cleanup_error = self
                            .record
                            .process_directory
                            .as_deref()
                            .ok_or_else(|| "Process registry unavailable".to_string())
                            .and_then(|dir| {
                                crate::recovery::reconcile_pending_operations(dir).map_err(err)?;
                                if crate::recovery::runtime_idle(Some(dir)).map_err(err)? {
                                    Ok(())
                                } else {
                                    Err("Native process cleanup is unconfirmed".into())
                                }
                            })
                            .err();
                        self.record.status = if persistence_error.is_some() {
                            "unknown"
                        } else if cleanup_error.is_some() {
                            "unknown"
                        } else if cancelled {
                            "stopped"
                        } else if failed.is_some() {
                            "failed"
                        } else if awaiting_approval {
                            "awaiting_approval"
                        } else if completion.is_none() {
                            "failed"
                        } else {
                            "completed"
                        }
                        .into();
                        let clean = matches!(
                            self.record.status.as_str(),
                            "completed" | "awaiting_approval"
                        );
                        let cleanup_confirmed = cleanup_error.is_none();
                        self.record.error = persistence_error.clone().or(cleanup_error).or(failed);
                        let terminal_error = self.record.error.clone().unwrap_or_else(|| {
                            "Native execution ended without a completion receipt".into()
                        });
                        self.record.finished_at = cleanup_confirmed.then(now);
                        if let Err(error) = save(&self.root, &self.record) {
                            persistence_error = Some(error);
                        }
                        // Release the session owner before a clean completion drains the queue.
                        drop(self);
                        if let Some(error) = persistence_error {
                            bus.publish(DomainEvent::ChatOutput{turn_id:id.clone(),event:EventKind::Error{message:format!("Direct execution persistence failed; outcome requires inspection: {error}")}});
                        } else if clean {
                            if let Some(event) = completion {
                                bus.publish(DomainEvent::ChatOutput {
                                    turn_id: id.clone(),
                                    event,
                                });
                            }
                        } else if !cancelled || !cleanup_confirmed {
                            bus.publish(DomainEvent::ChatOutput {
                                turn_id: id.clone(),
                                event: EventKind::Error {
                                    message: terminal_error,
                                },
                            });
                        }
                        bus.publish(DomainEvent::ChatEnded {
                            turn_id: id.clone(),
                            cancelled,
                        });
                        break;
                    }
                    other => bus.publish(other),
                }
            }
        };
        tokio::join!(runner, recorder);
    }
}

/// Explicit acknowledgment, only after both the owner lease and process registry
/// establish quiescence. It records an unknown result, never success or replay.
pub fn reconcile(root: &Path, id: &str) -> Result<Record, String> {
    let mut record = list(root, None)?
        .into_iter()
        .find(|r| r.turn_id == id)
        .ok_or("Direct turn not found")?;
    let _lease = session_lease(
        root.parent().ok_or("Invalid direct data directory")?,
        &record.session_id,
    )?;
    if record.finished_at.is_some() {
        return Ok(record);
    }
    if !cfg!(unix) {
        return Err(
            "Interrupted direct execution cannot be automatically reconciled on this platform"
                .into(),
        );
    }
    let directory = record
        .process_directory
        .as_deref()
        .ok_or("Process registry unavailable")?;
    crate::recovery::reconcile_pending_operations(directory).map_err(err)?;
    if !crate::recovery::runtime_idle(record.process_directory.as_deref()).map_err(err)? {
        return Err(
            "A recorded native process may still be active; stop it before reconciliation".into(),
        );
    }
    record.status = "unknown".into();
    record.error = Some(
        "Interrupted execution acknowledged; inspect workspace changes before retrying".into(),
    );
    record.finished_at = Some(now());
    save(root, &record)?;
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(cwd: &Path, id: &str, session: &str) -> ChatRequest {
        serde_json::from_value(serde_json::json!({"turnId":id,"sessionId":session,"cwd":cwd,"prompt":"do work","mode":"direct","provider":"CLAUDE_CODE","model":"test"})).unwrap()
    }

    #[test]
    fn recovery_freezes_closed_transcripts_and_discloses_incomplete_tail() {
        let data = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let root = data.path().join("direct-v1");
        let owner = Execution::prepare(&root, &request(cwd.path(), "turn", "s")).unwrap();
        assert!(recovery(&root, "s").unwrap().active);
        drop(owner);
        let path = root.join(format!("{}.jsonl", key("turn")));
        let bytes =
            b"{\"kind\":\"text\",\"text\":\"saved\"}\n{\"kind\":\"text\",\"text\":\"partial";
        fs::write(&path, bytes).unwrap();
        let snapshot = recovery(&root, "s").unwrap();
        assert!(!snapshot.active);
        assert!(snapshot.turns[0].partial_tail);
        assert_eq!(snapshot.turns[0].events.len(), 1);
        assert!(snapshot.turns[0].record.finished_at.is_none());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        let mut record = snapshot.turns[0].record.clone();
        record.status = "completed".into();
        record.finished_at = Some(now());
        save(&root, &record).unwrap();
        assert!(recovery(&root, "s").unwrap_err().contains("truncated"));
        fs::write(path, b"not-json\n").unwrap();
        assert!(recovery(&root, "s").is_err());
    }

    #[test]
    fn tip_links_order_fast_turns_and_reject_stale_continuation() {
        let data = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let root = data.path().join("direct-v1");
        let owner = Execution::prepare(&root, &request(cwd.path(), "z-first", "s")).unwrap();
        drop(owner);
        let mut first = list(&root, Some("s")).unwrap().pop().unwrap();
        first.status = "completed".into();
        first.finished_at = Some(now());
        save(&root, &first).unwrap();
        let mut next = request(cwd.path(), "a-second", "s");
        next.conversation = Some(serde_json::json!({"userText":"next"}));
        assert!(Execution::prepare(&root, &next)
            .err()
            .unwrap()
            .contains("behind"));
        next.direct_tip = Some("z-first".into());
        let owner = Execution::prepare(&root, &next).unwrap();
        drop(owner);
        let snapshot = recovery(&root, "s").unwrap();
        assert_eq!(
            snapshot
                .turns
                .iter()
                .map(|t| t.record.turn_id.as_str())
                .collect::<Vec<_>>(),
            vec!["z-first", "a-second"]
        );
        assert_eq!(snapshot.tip.as_deref(), Some("a-second"));
    }
    #[test]
    fn duplicate_conflicting_and_abandoned_requests_never_relaunch() {
        let root = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let req = request(cwd.path(), "one", "s");
        let owner = Execution::prepare(&root.path().join("direct-v1"), &req).unwrap();
        assert!(Execution::prepare(
            &root.path().join("direct-v1"),
            &request(cwd.path(), "two", "s")
        )
        .is_err());
        drop(owner);
        assert!(Execution::prepare(&root.path().join("direct-v1"), &req).is_err());
        let mut changed = req.clone();
        changed.prompt = "different".into();
        assert!(Execution::prepare(&root.path().join("direct-v1"), &changed)
            .err()
            .unwrap()
            .contains("conflicts"));
        assert!(Execution::prepare(
            &root.path().join("direct-v1"),
            &request(cwd.path(), "two", "other")
        )
        .is_err());
        assert_eq!(
            list(&root.path().join("direct-v1"), Some("s")).unwrap()[0].status,
            "prepared"
        );
    }
    #[test]
    fn workspace_aliases_contend_and_different_directories_do_not() {
        let cwd = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let _owner = workspace_lease(cwd.path().to_str().unwrap()).unwrap();
        assert!(workspace_lease(&format!("{}/.", cwd.path().display())).is_err());
        assert!(workspace_lease(other.path().to_str().unwrap()).is_ok());
    }
    #[test]
    fn explicit_recovery_keeps_result_unknown_and_requires_idle_processes() {
        let root = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let processes = tempfile::tempdir().unwrap();
        let mut req = request(cwd.path(), "one", "s");
        req.process_registry_dir = Some(processes.path().into());
        let owner = Execution::prepare(&root.path().join("direct-v1"), &req).unwrap();
        assert!(reconcile(&root.path().join("direct-v1"), "one").is_err());
        drop(owner);
        fs::write(processes.path().join("pending-gap.json"), "null").unwrap();
        assert!(reconcile(&root.path().join("direct-v1"), "one").is_err());
        fs::remove_file(processes.path().join("pending-gap.json")).unwrap();
        fs::write(processes.path().join("children.json"), "broken").unwrap();
        assert!(reconcile(&root.path().join("direct-v1"), "one").is_err());
        fs::write(processes.path().join("children.json"), "[]").unwrap();
        assert_eq!(
            reconcile(&root.path().join("direct-v1"), "one")
                .unwrap()
                .status,
            "unknown"
        );
        assert!(Execution::prepare(
            &root.path().join("direct-v1"),
            &request(cwd.path(), "two", "s")
        )
        .is_ok());
    }

    #[tokio::test]
    async fn failed_native_launch_is_durable_and_does_not_emit_success() {
        let root = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let req = request(cwd.path(), "one", "s");
        let execution = Execution::prepare(&root.path().join("direct-v1"), &req).unwrap();
        let bus = Bus::new();
        let mut rx = bus.subscribe();
        execution
            .run(
                &bus,
                req,
                vec![],
                ShellEnv {
                    vars: Default::default(),
                },
                None,
                None,
            )
            .await;
        let records = list(&root.path().join("direct-v1"), Some("s")).unwrap();
        assert_eq!(records[0].status, "failed");
        assert!(records[0].finished_at.is_some());
        assert!(events(&root.path().join("direct-v1"), "one")
            .unwrap()
            .iter()
            .any(|e| matches!(e, EventKind::Error { .. })));
        while let Ok(event) = rx.try_recv() {
            assert!(!matches!(
                event,
                DomainEvent::ChatOutput {
                    event: EventKind::Done { .. },
                    ..
                }
            ));
        }
    }

    #[tokio::test]
    async fn prepared_request_cannot_be_swapped_before_execution() {
        let root = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let mut req = request(cwd.path(), "original", "s");
        let execution = Execution::prepare(&root.path().join("direct-v1"), &req).unwrap();
        req.prompt = "changed work".into();
        req.turn_id = "replacement".into();
        let bus = Bus::new();
        let mut rx = bus.subscribe();
        execution
            .run(
                &bus,
                req,
                vec![],
                ShellEnv {
                    vars: Default::default(),
                },
                None,
                None,
            )
            .await;
        let record = list(&root.path().join("direct-v1"), Some("s"))
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(record.status, "failed");
        assert_eq!(record.request["prompt"], "do work");
        assert!(record.error.unwrap().contains("changed after preparation"));
        assert!(
            matches!(rx.recv().await,Some(DomainEvent::ChatOutput{turn_id,event:EventKind::Error{..}}) if turn_id=="original")
        );
    }
}
