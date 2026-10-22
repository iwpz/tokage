//! SQLite authority; all writes use Storage's durable writer. Lease lives with this handle.
use crate::*;
use rusqlite::{params, Connection, OptionalExtension};
use std::{path::Path, sync::Arc};
use tokage_native::file_lease::FileLease;
use tokage_storage::Storage;

fn db(e: impl std::fmt::Display) -> Error {
    Error::Repository(e.to_string())
}
fn decode<T: for<'de> Deserialize<'de>>(text: &str) -> Result<T> {
    serde_json::from_str(text).map_err(db)
}
fn encode<T: Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(db)
}
fn load(conn: &Connection, id: &str) -> Result<Option<Snapshot>> {
    let row: Option<(i64, String)> = conn
        .query_row(
            "SELECT revision,snapshot FROM runtime_runs WHERE run_id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(db)?;
    row.map(|(rev,text)| {
        let s: Snapshot = decode(&text)?; s.validate()?;
        if s.run_id != id || rev < 1 || rev as u64 != s.revision { return Err(db("snapshot column mismatch")); }
        let last: Option<(i64,String)> = conn.query_row("SELECT sequence,event FROM runtime_events WHERE run_id=?1 ORDER BY sequence DESC LIMIT 1", [id], |r| Ok((r.get(0)?,r.get(1)?))).optional().map_err(db)?;
        let (seq,text) = last.ok_or_else(|| db("missing event"))?;
        let event: Event = decode(&text)?;
        if seq != rev || event.sequence != rev as u64 || event.revision != rev as u64 || event.run_id != id || event.schema_version != SCHEMA_VERSION { return Err(db("event mismatch")); }
        let count: i64 = conn.query_row("SELECT count(*) FROM runtime_events WHERE run_id=?1", [id], |r| r.get(0)).map_err(db)?;
        let consistent = match event.kind {
            EventKind::RunCreated => s.state == RunState::Draft && rev == 1,
            EventKind::DraftCancelled => s.state == RunState::Stopped,
            EventKind::PlanImported => s.state == RunState::AwaitingApproval,
            EventKind::PlanApproved => s.state == RunState::Approved,
            EventKind::PlanningRequested | EventKind::PlanningClaimed => s.state == RunState::Planning,
            EventKind::PlanningCompleted => s.state == RunState::AwaitingApproval,
            EventKind::PlanningFailed => s.state == RunState::Blocked,
            EventKind::TurnObserved | EventKind::TurnFinished | EventKind::ApprovalResolved => !s.turns.is_empty(),
            EventKind::CancelRequested => s.state==RunState::CancelRequested,
            EventKind::Stopping => s.state==RunState::Stopping,
            EventKind::Stopped => s.state==RunState::Stopped,
            EventKind::RecoveryRequested => s.state==RunState::Blocked,
            EventKind::ExecutionRequested | EventKind::ExecutionProgress => s.execution.is_some(),
            EventKind::ConfigUpdated => matches!(s.state, RunState::Draft | RunState::AwaitingApproval),
        };
        if count != rev || !consistent { return Err(db("event history/state mismatch")); }
        for (turn_id,turn) in &s.turns {
            let count:i64=conn.query_row("SELECT count(*) FROM runtime_turn_events WHERE run_id=?1 AND turn_id=?2",params![id,turn_id],|r|r.get(0)).map_err(db)?;
            if count as u64 != turn.last_sequence {return Err(db("turn transcript/projection mismatch"));}
        }
        Ok(s)
    }).transpose()
}
pub struct SqliteRepository {
    storage: Arc<Storage>,
    directory: std::path::PathBuf,
    run_id: String,
    _lease: FileLease,
}
impl SqliteRepository {
    /// Directory contains both DB and owner locks. Never unlink locks while storage is in use.
    pub fn open(directory: &Path, run_id: &str) -> Result<Self> {
        if run_id.trim().is_empty() {
            return Err(Error::Invalid("empty run ID".into()));
        }
        std::fs::create_dir_all(directory).map_err(db)?;
        let directory = directory.canonicalize().map_err(db)?;
        let database = directory.join("tokage.db");
        if std::fs::symlink_metadata(&database).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(Error::Invalid(
                "symlinked database would bypass owner namespace".into(),
            ));
        }
        let lock = directory.join(format!("runtime-{}.lock", digest(&run_id)?));
        let lease = FileLease::acquire(&lock).map_err(|e| {
            if e.kind() == std::io::ErrorKind::WouldBlock
                || (cfg!(windows) && matches!(e.raw_os_error(), Some(32 | 33)))
            {
                Error::OwnerConflict
            } else {
                db(e)
            }
        })?;
        let storage = Arc::new(Storage::open(&directory).map_err(db)?);
        let repo = Self {
            storage,
            directory,
            run_id: run_id.into(),
            _lease: lease,
        };
        repo.snapshot(run_id)?;
        Ok(repo)
    }
    /// Service-only decisions. There is deliberately no public snapshot replacement API.
    pub(crate) async fn advance<F>(&mut self, expected: u64, decide: F) -> Result<()>
    where
        F: FnOnce(&mut Snapshot) -> Result<EventKind> + Send + 'static,
    {
        let id = self.run_id.clone();
        self.storage
            .atomic(move |c| {
                let mut next = match load(c, &id) {
                    Ok(Some(s)) => s,
                    Ok(None) => return Ok(Err(Error::Transition)),
                    Err(e) => return Ok(Err(e)),
                };
                if next.revision != expected {
                    return Ok(Err(Error::RevisionConflict));
                }
                let prior_turns = next.turns.clone();
                let prior_attempts = next
                    .work
                    .as_ref()
                    .map(|w| w.attempts.clone())
                    .unwrap_or_default();
                let kind = match decide(&mut next) {
                    Ok(k) => k,
                    Err(e) => return Ok(Err(e)),
                };
                next.revision = match next
                    .revision
                    .checked_add(1)
                    .filter(|r| *r <= i64::MAX as u64)
                {
                    Some(r) => r,
                    None => return Ok(Err(Error::RevisionConflict)),
                };
                if let Err(e) = next.validate() {
                    return Ok(Err(e));
                }
                let changed = next
                    .turns
                    .iter()
                    .filter(|(id, t)| prior_turns.get(*id) != Some(*t))
                    .map(|(id, _)| id.clone())
                    .collect::<Vec<_>>();
                let changed_work = next
                    .work
                    .as_ref()
                    .map(|w| {
                        w.attempts
                            .iter()
                            .filter(|(id, a)| prior_attempts.get(*id) != Some(*a))
                            .map(|(id, _)| id.clone())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let correlation = if changed.len() == 1 {
                    changed.first().cloned()
                } else if changed_work.len() == 1 {
                    changed_work.first().cloned()
                } else {
                    None
                };
                let event = Event {
                    schema_version: SCHEMA_VERSION,
                    run_id: id.clone(),
                    sequence: next.revision,
                    revision: next.revision,
                    attempt_id: correlation.clone(),
                    turn_id: correlation,
                    kind,
                };
                let sql = |e: Error| rusqlite::Error::ToSqlConversionFailure(Box::new(e));
                c.execute(
                    "UPDATE runtime_runs SET revision=?2,snapshot=?3 WHERE run_id=?1",
                    params![id, next.revision as i64, encode(&next).map_err(sql)?],
                )?;
                c.execute(
                    "INSERT INTO runtime_events VALUES(?1,?2,?3)",
                    params![id, next.revision as i64, encode(&event).map_err(sql)?],
                )?;
                Ok(Ok(()))
            })
            .await
            .map_err(db)?
    }
    pub fn transcript(
        &self,
        id: &str,
        turn_id: &str,
        after: u64,
        limit: u32,
    ) -> Result<Vec<turn::TranscriptEvent>> {
        self.check_id(id)?;
        if limit == 0 || limit > 1000 || after > i64::MAX as u64 {
            return Err(Error::Invalid("transcript cursor".into()));
        }
        let c = self.storage.get_conn().map_err(db)?;
        let mut q=c.prepare("SELECT sequence,event FROM runtime_turn_events WHERE run_id=?1 AND turn_id=?2 AND sequence>?3 ORDER BY sequence LIMIT ?4").map_err(db)?;
        let rows = q
            .query_map(params![id, turn_id, after as i64, limit], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(db)?;
        rows.map(|r| {
            let (sequence, text) = r.map_err(db)?;
            Ok(turn::TranscriptEvent {
                turn_id: turn_id.into(),
                sequence: sequence as u64,
                event: decode(&text)?,
            })
        })
        .collect()
    }
    #[cfg(test)]
    pub(crate) async fn observe(
        &mut self,
        turn_id: &str,
        sequence: u64,
        event: tokage_core::EventKind,
    ) -> Result<()> {
        self.observe_batch(turn_id, vec![(sequence, event)]).await
    }
    pub(crate) async fn observe_batch(
        &mut self,
        turn_id: &str,
        observations: Vec<(u64, tokage_core::EventKind)>,
    ) -> Result<()> {
        let id = self.run_id.clone();
        let turn_id = turn_id.to_owned();
        if observations.is_empty()
            || observations.len() > 64
            || observations
                .iter()
                .any(|(s, _)| *s == 0 || *s > i64::MAX as u64)
        {
            return Err(Error::Invalid("turn observation batch".into()));
        }
        self.storage.atomic(move |c|{
            let mut next=match load(c,&id){Ok(Some(s))=>s,Ok(None)=>return Ok(Err(Error::Transition)),Err(e)=>return Ok(Err(e))};
            let mut rows=Vec::new();
            for (sequence,event) in observations {
                let text=match encode(&event){Ok(t)=>t,Err(e)=>return Ok(Err(e))};
                let prior:Option<String>=c.query_row("SELECT event FROM runtime_turn_events WHERE run_id=?1 AND turn_id=?2 AND sequence=?3",params![id,turn_id,sequence as i64],|r|r.get(0)).optional()?;
                if let Some(prior)=prior {if prior==text{continue;}else{return Ok(Err(Error::CommandConflict));}}
                let Some(t)=next.turns.get_mut(&turn_id) else {return Ok(Err(Error::Transition));};
                if sequence!=t.last_sequence+1 {return Ok(Err(Error::Invalid("out of order turn observation".into())));}
                let Some(revision)=next.revision.checked_add(1).filter(|r|*r<=i64::MAX as u64) else{return Ok(Err(Error::RevisionConflict));};
                if let Err(e)=t.observe(&event,revision) {return Ok(Err(e));}
                t.last_sequence=sequence;next.revision=revision;
                let ev=Event{schema_version:SCHEMA_VERSION,run_id:id.clone(),sequence:revision,revision,attempt_id:Some(turn_id.clone()),turn_id:Some(turn_id.clone()),kind:EventKind::TurnObserved};
                rows.push((sequence,text,revision,ev));
            }
            if rows.is_empty(){return Ok(Ok(()));}
            if let Err(e)=next.validate(){return Ok(Err(e));}
            let sql=|e:Error|rusqlite::Error::ToSqlConversionFailure(Box::new(e));
            c.execute("UPDATE runtime_runs SET revision=?2,snapshot=?3 WHERE run_id=?1",params![id,next.revision as i64,encode(&next).map_err(sql)?])?;
            for (sequence,text,revision,event) in rows {
                c.execute("INSERT INTO runtime_events VALUES(?1,?2,?3)",params![id,revision as i64,encode(&event).map_err(sql)?])?;
                c.execute("INSERT INTO runtime_turn_events VALUES(?1,?2,?3,?4)",params![id,turn_id,sequence as i64,text])?;
            }
            Ok(Ok(()))
        }).await.map_err(db)?
    }
    fn check_id(&self, id: &str) -> Result<()> {
        if id != self.run_id {
            return Err(Error::Invalid("repository owner/run mismatch".into()));
        }
        Ok(())
    }
    pub(crate) fn directory(&self) -> &Path {
        &self.directory
    }
    /// Returns immutable source text, after checking its archived content hash.
    pub fn history(&self, kind: &str, id: &str) -> Result<Option<String>> {
        self.check_id(id)?;
        let conn = self.storage.get_conn().map_err(db)?;
        let row: Option<(String, String)> = conn.query_row(
            "SELECT content_hash,original FROM runtime_history WHERE source_kind=?1 AND source_id=?2",
            params![kind,id], |r| Ok((r.get(0)?,r.get(1)?)),
        ).optional().map_err(db)?;
        row.map(|(hash, original)| {
            if hash != format!("{:x}", Sha256::digest(original.as_bytes())) {
                return Err(db("historical content hash mismatch"));
            }
            Ok(original)
        })
        .transpose()
    }
    /// Explicit archival import. Original input must already be durably backed up by caller.
    /// History never appears in runtime_runs and cannot be executed.
    pub async fn import_history(
        &self,
        kind: &str,
        id: &str,
        original: &str,
        backup: &Path,
    ) -> Result<String> {
        self.check_id(id)?;
        let bytes = std::fs::read(backup).map_err(db)?;
        if bytes != original.as_bytes() {
            return Err(Error::Invalid("backup differs from original".into()));
        }
        let value: serde_json::Value = serde_json::from_str(original).map_err(db)?;
        let valid = match kind {
            "cli-v1" => {
                value["schemaVersion"] == 1
                    && value["id"] == id
                    && value["status"].is_string()
                    && value["requirement"].is_string()
                    && value["repo"].is_string()
            }
            "app-pipeline-v1" => {
                value["schemaVersion"] == 1
                    && value["mainSessionId"].is_string()
                    && value["planId"].is_number()
                    && value["pipeline"].is_object()
                    && format!(
                        "{}:{}",
                        value["mainSessionId"].as_str().unwrap_or(""),
                        value["planId"]
                    ) == id
            }
            _ => false,
        };
        if !valid {
            return Err(Error::Invalid(
                "unknown legacy record format/identity".into(),
            ));
        }
        let hash = format!("{:x}", Sha256::digest(original.as_bytes()));
        let (kind, id, original, hash_copy) = (
            kind.to_owned(),
            id.to_owned(),
            original.to_owned(),
            hash.clone(),
        );
        let result = self.storage.atomic(move |c| {
            let prior: Option<String> = c.query_row("SELECT content_hash FROM runtime_history WHERE source_kind=?1 AND source_id=?2", params![kind,id],|r| r.get(0)).optional()?;
            if let Some(prior) = prior { return Ok(prior == hash_copy); }
            c.execute("INSERT INTO runtime_history VALUES(?1,?2,?3,?4)", params![kind,id,hash_copy,original])?;
            Ok(true)
        }).await.map_err(db)?;
        if !result {
            return Err(Error::CommandConflict);
        }
        Ok(hash)
    }
}
impl Repository for SqliteRepository {
    async fn submit(&mut self, request: &Request) -> Result<Receipt> {
        self.check_id(&request.run_id)?;
        if request.schema_version != SCHEMA_VERSION {
            return Err(Error::Schema);
        }
        let request = request.clone();
        let request_hash = digest(&request)?;
        self.storage.atomic(move |c| {
            // Domain rejection has no writes. SQL errors after writes roll back the savepoint.
            let prior: Option<(String,String)> = c.query_row("SELECT request_hash,receipt FROM runtime_commands WHERE command_id=?1", [&request.command_id],|r| Ok((r.get(0)?,r.get(1)?))).optional()?;
            if let Some((hash,text)) = prior {
                return Ok(if hash == request_hash { decode(&text) } else { Err(Error::CommandConflict) });
            }
            let current = match load(c,&request.run_id) { Ok(s) => s, Err(e) => return Ok(Err(e)) };
            let (next,event) = match transition(current.as_ref(),&request) { Ok(v) => v, Err(e) => return Ok(Err(e)) };
            let receipt = Receipt { run_id: request.run_id.clone(), command_id: request.command_id.clone(), revision: next.revision };
            let serialize = |v| encode(v).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)));
            let snapshot = serialize(&next)?;
            let event = encode(&event).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
            let receipt_json = encode(&receipt).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
            c.execute("INSERT INTO runtime_runs VALUES(?1,?2,?3) ON CONFLICT(run_id) DO UPDATE SET revision=excluded.revision,snapshot=excluded.snapshot",params![next.run_id,next.revision as i64,snapshot])?;
            c.execute("INSERT INTO runtime_events VALUES(?1,?2,?3)",params![next.run_id,next.revision as i64,event])?;
            c.execute("INSERT INTO runtime_commands VALUES(?1,?2,?3)",params![request.command_id,request_hash,receipt_json])?;
            Ok(Ok(receipt))
        }).await.map_err(db)?
    }
    fn snapshot(&self, id: &str) -> Result<Option<Snapshot>> {
        self.check_id(id)?;
        {
            let conn = self.storage.get_conn().map_err(db)?;
            load(&conn, id)
        }
    }
    fn events(&self, id: &str, after: u64, limit: u32) -> Result<Vec<Event>> {
        self.check_id(id)?;
        if limit == 0 || limit > 1000 || after > i64::MAX as u64 {
            return Err(Error::Invalid("event cursor/page size".into()));
        }
        let c = self.storage.get_conn().map_err(db)?;
        let mut stmt = c.prepare("SELECT sequence,event FROM runtime_events WHERE run_id=?1 AND sequence>?2 ORDER BY sequence LIMIT ?3").map_err(db)?;
        let rows = stmt
            .query_map(params![id, after as i64, limit], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(db)?;
        rows.map(|row| {
            let (seq, text) = row.map_err(db)?;
            let e: Event = decode(&text)?;
            if e.schema_version != SCHEMA_VERSION
                || e.run_id != id
                || e.sequence != seq as u64
                || e.revision != seq as u64
            {
                return Err(db("corrupt event"));
            }
            Ok(e)
        })
        .collect()
    }
}
#[cfg(test)]
mod turn_tests {
    use super::*;
    fn create() -> Request {
        let model = ModelAssignment {
            provider: "CODEX_CLI".into(),
            requested_model: "model".into(),
            provider_path: None,
            capabilities: vec![],
            user_locked: true,
            model_params: None,
        };
        let config = EffectiveRunConfig {
            source: "unit".into(),
            policy_version: "1".into(),
            workspace: "/repo".into(),
            target_identity: "head".into(),
            target_reference: None,
            scope: vec!["src".into()],
            models: BTreeMap::from([("executor".into(), model)]),
            command_policy_hash: "hash".into(),
            trust_workspace: true,
            permission_mode: "workspace-write".into(),
            concurrency: 1,
            provider_concurrency: BTreeMap::new(),
            budget_usd: None,
            block_unknown_cost: true,
            timeout_seconds: 60,
            repair_policy: "manual".into(),
            context_policy: "off".into(),
            confirmed_commands: vec![],
            custom_pack_hashes: vec![],
            co_author: false,
            planning_policy: None,
        };
        Request {
            schema_version: 1,
            command_id: "create".into(),
            run_id: "run".into(),
            expected_revision: 0,
            command: Command::CreateRun {
                requirement: "requirement".into(),
                config,
            },
        }
    }
    #[tokio::test]
    async fn transcript_duplicates_gaps_conflicts_and_transaction_failures_are_fenced() {
        let dir = tempfile::tempdir().unwrap();
        let mut repo = SqliteRepository::open(dir.path(), "run").unwrap();
        repo.submit(&create()).await.unwrap();
        let s = repo.snapshot("run").unwrap().unwrap();
        let mut req = create();
        req.command_id = "generate".into();
        req.expected_revision = s.revision;
        req.command = Command::GeneratePlan;
        repo.submit(&req).await.unwrap();
        repo.advance(2, |s| {
            let a = s.planning_attempt.as_mut().unwrap();
            a.claimed = true;
            s.turns.insert(
                a.id.clone(),
                turn::TurnRecord::prepared(
                    a.id.clone(),
                    "planner",
                    None,
                    s.config.models["executor"].clone(),
                ),
            );
            Ok(EventKind::PlanningClaimed)
        })
        .await
        .unwrap();
        let s = repo.snapshot("run").unwrap().unwrap();
        let id = s.turns.keys().next().unwrap().clone();
        let text = tokage_core::EventKind::Text {
            text: "same repeated text".into(),
        };
        assert!(repo.observe(&id, 2, text.clone()).await.is_err());
        assert_eq!(repo.snapshot("run").unwrap().unwrap().revision, 3);
        repo.observe(&id, 1, text.clone()).await.unwrap();
        repo.observe(&id, 1, text.clone()).await.unwrap();
        assert_eq!(repo.snapshot("run").unwrap().unwrap().revision, 4);
        assert!(matches!(
            repo.observe(
                &id,
                1,
                tokage_core::EventKind::Text {
                    text: "changed".into()
                }
            )
            .await,
            Err(Error::CommandConflict)
        ));
        let c = Connection::open(dir.path().join("tokage.db")).unwrap();
        c.execute_batch("CREATE TRIGGER fail_transcript BEFORE INSERT ON runtime_turn_events BEGIN SELECT RAISE(ABORT,'transcript disk failure'); END;").unwrap();
        assert!(repo.observe(&id, 2, text.clone()).await.is_err());
        assert_eq!(repo.snapshot("run").unwrap().unwrap().revision, 4);
        assert_eq!(repo.transcript("run", &id, 0, 100).unwrap().len(), 1);
        c.execute_batch("DROP TRIGGER fail_transcript").unwrap();
        repo.observe(&id, 2, text).await.unwrap();
        assert_eq!(repo.transcript("run", &id, 0, 100).unwrap().len(), 2);
        drop(repo);
        c.execute("DELETE FROM runtime_turn_events WHERE sequence=1", [])
            .unwrap();
        assert!(SqliteRepository::open(dir.path(), "run").is_err());
    }
}

/// Owner-independent, read-only queries. Opening this handle never creates or migrates data.
pub struct Reader {
    conn: Connection,
}
impl Reader {
    pub fn open(directory: &Path) -> Result<Self> {
        let path = directory.join("tokage.db");
        if std::fs::symlink_metadata(&path)
            .map_err(db)?
            .file_type()
            .is_symlink()
        {
            return Err(Error::Invalid("symlinked runtime database".into()));
        }
        let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(db)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(db)?;
        Ok(Self { conn })
    }
    pub fn snapshot(&mut self, id: &str) -> Result<Option<Snapshot>> {
        let tx = self.conn.transaction().map_err(db)?;
        let s = load(&tx, id)?;
        tx.commit().map_err(db)?;
        Ok(s)
    }
    pub fn list(&mut self) -> Result<Vec<Snapshot>> {
        let tx = self.conn.transaction().map_err(db)?;
        let ids = tx
            .prepare("SELECT run_id FROM runtime_runs ORDER BY run_id")
            .map_err(db)?
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(db)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db)?;
        let result = ids
            .iter()
            .map(|id| load(&tx, id)?.ok_or(Error::Transition))
            .collect::<Result<_>>()?;
        tx.commit().map_err(db)?;
        Ok(result)
    }
    pub fn events(&mut self, id: &str, after: u64, limit: u32) -> Result<Vec<Event>> {
        if limit == 0 || limit > 1000 || after > i64::MAX as u64 {
            return Err(Error::Invalid("event cursor/page size".into()));
        }
        let tx = self.conn.transaction().map_err(db)?;
        load(&tx, id)?.ok_or(Error::Transition)?;
        let events = tx.prepare("SELECT event FROM runtime_events WHERE run_id=?1 AND sequence>?2 ORDER BY sequence LIMIT ?3").map_err(db)?
            .query_map(params![id, after as i64, limit], |r| r.get::<_,String>(0)).map_err(db)?
            .map(|r| decode::<Event>(&r.map_err(db)?)).collect::<Result<Vec<_>>>()?;
        tx.commit().map_err(db)?;
        Ok(events)
    }
    pub fn transcript(
        &mut self,
        id: &str,
        turn: &str,
        after: u64,
        limit: u32,
    ) -> Result<Vec<turn::TranscriptEvent>> {
        if limit == 0 || limit > 1000 || after > i64::MAX as u64 {
            return Err(Error::Invalid("transcript cursor/page size".into()));
        }
        let tx = self.conn.transaction().map_err(db)?;
        let s = load(&tx, id)?.ok_or(Error::Transition)?;
        if !s.turns.contains_key(turn) {
            return Err(Error::Invalid("unknown turn".into()));
        }
        let events = tx.prepare("SELECT sequence,event FROM runtime_turn_events WHERE run_id=?1 AND turn_id=?2 AND sequence>?3 ORDER BY sequence LIMIT ?4").map_err(db)?
            .query_map(params![id,turn,after as i64,limit], |r| Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?))).map_err(db)?
            .map(|r| {let (seq,text)=r.map_err(db)?;Ok(turn::TranscriptEvent{turn_id:turn.into(),sequence:seq as u64,event:decode(&text)?})}).collect::<Result<Vec<_>>>()?;
        tx.commit().map_err(db)?;
        Ok(events)
    }
}
