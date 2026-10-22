//! Run owner outlives view handles. Commands are durable before worker launch/cancellation.
use crate::{
    repository::SqliteRepository,
    service::{Planner, TurnControl},
    turn::{TurnRecord, TurnStatus},
    *,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex as SyncMutex},
};
use tokio::sync::{mpsc, watch, Mutex};
pub(crate) struct Inner<P: Planner> {
    pub(crate) repo: Mutex<SqliteRepository>,
    pub(crate) run_id: String,
    pub(crate) planner: P,
    pub(crate) jobs: SyncMutex<BTreeMap<String, watch::Sender<bool>>>,
    pub(crate) failure: SyncMutex<Option<Error>>,
}
pub struct Runtime<P: Planner> {
    pub(crate) inner: Arc<Inner<P>>,
}
impl<P: Planner> Clone for Runtime<P> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}
impl<P: Planner + 'static> Runtime<P> {
    pub fn new(repo: SqliteRepository, run_id: &str, planner: P) -> Result<Self> {
        repo.snapshot(run_id)?;
        Ok(Self {
            inner: Arc::new(Inner {
                repo: Mutex::new(repo),
                run_id: run_id.into(),
                planner,
                jobs: SyncMutex::new(BTreeMap::new()),
                failure: SyncMutex::new(None),
            }),
        })
    }
    pub fn run_id(&self) -> &str {
        &self.inner.run_id
    }
    pub fn busy(&self) -> bool {
        !self
            .inner
            .jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
    }
    pub async fn snapshot(&self) -> Result<Option<Snapshot>> {
        self.inner.repo.lock().await.snapshot(&self.inner.run_id)
    }
    pub async fn events(&self, after: u64, limit: u32) -> Result<Vec<Event>> {
        self.inner
            .repo
            .lock()
            .await
            .events(&self.inner.run_id, after, limit)
    }
    pub async fn transcript(
        &self,
        turn_id: &str,
        after: u64,
        limit: u32,
    ) -> Result<Vec<turn::TranscriptEvent>> {
        self.inner
            .repo
            .lock()
            .await
            .transcript(&self.inner.run_id, turn_id, after, limit)
    }
    /// A storage/worker error is visible independently of a snapshot whose last write failed.
    pub fn worker_error(&self) -> Option<Error> {
        self.inner
            .failure
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    /// Factory admission is read-only and runs under this handle's OS owner lease.
    pub async fn admit_request(&self, request: &Request) -> Result<()> {
        let repo = self.inner.repo.lock().await;
        if repo.snapshot(&self.inner.run_id)?.is_none() {
            crate::transition(None, request)?;
            crate::migration::admit_new(&repo, request)?;
        }
        Ok(())
    }
    /// The owner applies a scoped external cancel intent atomically against its current revision.
    /// Persist first, then use normal idempotent submission to signal workers and confirm cleanup.
    pub(crate) async fn apply_cancellation(
        &self,
        config_hash: &str,
        minimum_revision: u64,
    ) -> Result<bool> {
        let request = {
            let mut repo = self.inner.repo.lock().await;
            let snapshot = repo
                .snapshot(&self.inner.run_id)?
                .ok_or(Error::Transition)?;
            if snapshot.config_hash != config_hash || snapshot.revision < minimum_revision {
                return Ok(false);
            }
            let request = crate::host::request(&snapshot, Command::CancelRun);
            repo.submit(&request).await?;
            request
        };
        self.submit(&request).await?;
        Ok(true)
    }
    pub async fn submit(&self, request: &Request) -> Result<Receipt> {
        let mut repo = self.inner.repo.lock().await;
        let before_state = repo.snapshot(&self.inner.run_id)?.map(|s| s.state);
        let before_revision = repo.snapshot(&self.inner.run_id)?.map_or(0, |s| s.revision);
        if matches!(request.command, Command::ResumeRun)
            && before_revision == request.expected_revision
            && !self
                .inner
                .jobs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_empty()
        {
            return Err(Error::Transition);
        }
        if matches!(request.command, Command::StartExecution { .. }) {
            let s = repo
                .snapshot(&self.inner.run_id)?
                .ok_or(Error::Transition)?;
            if s.state == RunState::Approved && s.revision == request.expected_revision {
                crate::workflow::preflight(&self.inner, &s).await?;
            }
        }
        let receipt = repo.submit(request).await?;
        let current = repo
            .snapshot(&self.inner.run_id)?
            .ok_or(Error::Transition)?;
        if matches!(request.command, Command::ResumeRun)
            && current.revision == receipt.revision
            && self.inner.planner.native_backend().is_some()
        {
            if !self
                .inner
                .jobs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_empty()
            {
                return Ok(receipt);
            }
            let key = format!("recovery-{}", crate::digest(&request.command_id)?);
            let (tx, _) = watch::channel(false);
            self.inner
                .jobs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(key.clone(), tx);
            drop(repo);
            let native = self
                .inner
                .planner
                .native_backend()
                .ok_or(Error::Unsupported)?;
            let directory = native
                .directory
                .join("git-processes")
                .join(crate::digest(&self.inner.run_id)?)
                .join("recovery");
            let result = tokage_native::proc::in_run_directory(
                directory,
                crate::recovery::recover(&self.inner),
            )
            .await;
            self.inner
                .jobs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&key);
            result?;
            *self.inner.failure.lock().unwrap_or_else(|e| e.into_inner()) = None;
            return Ok(receipt);
        }
        if current.state == RunState::CancelRequested {
            let known_stopped = current
                .turns
                .values()
                .all(|t| t.ended && t.cleanup_confirmed)
                && current
                    .planning_attempt
                    .as_ref()
                    .is_none_or(|a| !a.claimed || current.turns.contains_key(&a.id));
            let has_jobs = !self
                .inner
                .jobs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_empty();
            if known_stopped
                && matches!(
                    before_state,
                    Some(
                        RunState::AwaitingMerge
                            | RunState::AwaitingReview
                            | RunState::AwaitingTurnApproval
                    )
                )
            {
                repo.advance(current.revision, |s| {
                    s.state = RunState::Stopped;
                    Ok(EventKind::Stopped)
                })
                .await?;
            } else if has_jobs {
                repo.advance(current.revision, |s| {
                    s.state = RunState::Stopping;
                    Ok(EventKind::Stopping)
                })
                .await?;
                for tx in self
                    .inner
                    .jobs
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .values()
                {
                    let _ = tx.send(true);
                }
            } else if known_stopped {
                repo.advance(current.revision, |s| {
                    s.state = RunState::Stopped;
                    Ok(EventKind::Stopped)
                })
                .await?;
            }
            // No live worker does not prove a recovered process has stopped.
            return Ok(receipt);
        }
        if current.state == RunState::Planning
            && current
                .planning_attempt
                .as_ref()
                .is_some_and(|a| !a.claimed)
        {
            let attempt = current.planning_attempt.as_ref().unwrap().id.clone();
            let selected = current
                .config
                .models
                .get("planner")
                .or_else(|| current.config.models.get("executor"))
                .ok_or(Error::Unsupported)?
                .clone();
            let reservation = match crate::execution::role_budget(&current, "planning", &selected) {
                Ok(r) => r,
                Err(error) => {
                    repo.advance(current.revision, move |s| {
                        s.state = RunState::Blocked;
                        s.planning_attempt.as_mut().unwrap().error = Some(error.to_string());
                        Ok(EventKind::PlanningFailed)
                    })
                    .await?;
                    return Ok(receipt);
                }
            };
            let id = attempt.clone();
            repo.advance(current.revision, move |s| {
                s.planning_attempt.as_mut().unwrap().claimed = true;
                let mut turn = TurnRecord::prepared(id.clone(), "planner", None, selected);
                turn.reserved_usd = reservation;
                s.turns.insert(id, turn);
                Ok(EventKind::PlanningClaimed)
            })
            .await?;
            *self.inner.failure.lock().unwrap_or_else(|e| e.into_inner()) = None;
            let snapshot = repo.snapshot(&self.inner.run_id)?.unwrap();
            let (cancel_tx, cancel_rx) = watch::channel(false);
            self.inner
                .jobs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(attempt.clone(), cancel_tx);
            let inner = self.inner.clone();
            tokio::spawn(async move {
                let worker_inner = inner.clone();
                let worker_attempt = attempt.clone();
                let worker = tokio::spawn(async move {
                    run_owned(&worker_inner, snapshot, worker_attempt, cancel_rx, None).await
                });
                let result = worker.await.unwrap_or_else(|_| {
                    Err(Error::Repository(
                        "turn worker terminated; native effect requires reconciliation".into(),
                    ))
                });
                if let Err(error) = result {
                    remember(&inner, error);
                }
                inner
                    .jobs
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&attempt);
            });
        }
        if current.state == RunState::Executing
            && current.execution.as_ref().is_some_and(|e| !e.claimed)
        {
            repo.advance(current.revision, |s| {
                s.execution.as_mut().unwrap().claimed = true;
                Ok(EventKind::ExecutionProgress)
            })
            .await?;
            *self.inner.failure.lock().unwrap_or_else(|e| e.into_inner()) = None;
            let id = format!("execution-{}", current.revision);
            let (tx, rx) = watch::channel(false);
            self.inner
                .jobs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(id.clone(), tx);
            let inner = self.inner.clone();
            tokio::spawn(async move {
                let worker_inner = inner.clone();
                let worker = tokio::spawn(async move {
                    let dir = worker_inner
                        .planner
                        .native_backend()
                        .ok_or(Error::Unsupported)?
                        .directory
                        .join("git-processes")
                        .join(crate::digest(&worker_inner.run_id)?)
                        .join("root");
                    tokage_native::proc::in_run_directory(
                        dir,
                        crate::workflow::execute(worker_inner, rx),
                    )
                    .await
                });
                let result = worker.await.unwrap_or_else(|_| {
                    Err(Error::Repository(
                        "execution worker terminated; effects require reconciliation".into(),
                    ))
                });
                if let Err(e) = result {
                    remember(&inner, e);
                }
                inner
                    .jobs
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&id);
            });
        }
        if current.state == RunState::Reviewing
            && current.delivery.as_ref().is_some_and(|d| !d.claimed)
        {
            repo.advance(current.revision, |s| {
                s.delivery.as_mut().unwrap().claimed = true;
                Ok(EventKind::ExecutionProgress)
            })
            .await?;
            *self.inner.failure.lock().unwrap_or_else(|e| e.into_inner()) = None;
            let id = current.delivery.as_ref().unwrap().review_id.clone();
            let (tx, rx) = watch::channel(false);
            self.inner
                .jobs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(id.clone(), tx);
            let inner = self.inner.clone();
            tokio::spawn(async move {
                let directory = inner.planner.native_backend().map(|n| {
                    n.directory
                        .join("git-processes")
                        .join(crate::digest(&inner.run_id).unwrap_or_default())
                        .join(&id)
                });
                let result = if let Some(directory) = directory {
                    tokage_native::proc::in_run_directory(
                        directory,
                        crate::delivery::review(inner.clone(), rx),
                    )
                    .await
                } else {
                    Err(Error::Unsupported)
                };
                if let Err(e) = result {
                    remember(&inner, e);
                }
                inner
                    .jobs
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&id);
            });
        }
        if current.state == RunState::Delivering
            && current
                .delivery
                .as_ref()
                .and_then(|d| d.intent.as_ref())
                .is_some_and(|i| !i.claimed)
        {
            repo.advance(current.revision, |s| {
                s.delivery
                    .as_mut()
                    .unwrap()
                    .intent
                    .as_mut()
                    .unwrap()
                    .claimed = true;
                Ok(EventKind::ExecutionProgress)
            })
            .await?;
            let id = current
                .delivery
                .as_ref()
                .unwrap()
                .intent
                .as_ref()
                .unwrap()
                .command_id
                .clone();
            let (tx, _) = watch::channel(false);
            self.inner
                .jobs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(id.clone(), tx);
            let inner = self.inner.clone();
            tokio::spawn(async move {
                let directory = inner.planner.native_backend().map(|n| {
                    n.directory
                        .join("git-processes")
                        .join(crate::digest(&inner.run_id).unwrap_or_default())
                        .join(format!(
                            "delivery-{}",
                            crate::digest(&id).unwrap_or_default()
                        ))
                });
                let result = if let Some(directory) = directory {
                    tokage_native::proc::in_run_directory(
                        directory,
                        crate::delivery::deliver(inner.clone()),
                    )
                    .await
                } else {
                    Err(Error::Unsupported)
                };
                if let Err(e) = result {
                    remember(&inner, e);
                }
                inner
                    .jobs
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&id);
            });
        }
        Ok(receipt)
    }
}
pub(crate) async fn run_owned<P: Planner + 'static>(
    inner: &Arc<Inner<P>>,
    snapshot: Snapshot,
    attempt: String,
    cancel: watch::Receiver<bool>,
    spec: Option<crate::service::TurnSpec>,
) -> Result<String> {
    let (tx, mut rx) = mpsc::channel(64);
    let control = TurnControl {
        events: tx,
        cancel,
        owner: Some(inner.clone()),
    };
    let role = snapshot
        .turns
        .get(&attempt)
        .map(|t| t.role.as_str())
        .ok_or(Error::Transition)?;
    let mut future: std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<String>> + Send + '_>,
    > = if let Some(spec) = spec {
        Box::pin(
            inner
                .planner
                .native_backend()
                .ok_or(Error::Unsupported)?
                .run_native(&snapshot, &attempt, spec, Some(control)),
        )
    } else {
        Box::pin(
            inner
                .planner
                .generate_observed(&snapshot, &attempt, control),
        )
    };
    let mut sequence = 0u64;
    let mut observer_error = None;
    let outcome = loop {
        tokio::select! {
            result=&mut future=>break result,
            incoming=rx.recv()=>match incoming {
                Some(event)=>{
                    sequence+=1;
                    let mut batch=vec![(sequence,event)];
                    while batch.len()<64 {match rx.try_recv(){Ok(event)=>{sequence+=1;batch.push((sequence,event));},Err(_)=>break}}
                    if observer_error.is_none() {
                        if let Err(e)=inner.repo.lock().await.observe_batch(&attempt,batch).await {
                            observer_error=Some(e);
                            for tx in inner.jobs.lock().unwrap_or_else(|e|e.into_inner()).values(){let _=tx.send(true);}
                        }
                    }
                }
                None=>break (&mut future).await,
            }
        }
    };
    drop(future);
    while let Some(event) = rx.recv().await {
        sequence += 1;
        let mut batch = vec![(sequence, event)];
        while batch.len() < 64 {
            match rx.try_recv() {
                Ok(event) => {
                    sequence += 1;
                    batch.push((sequence, event));
                }
                Err(_) => break,
            }
        }
        if observer_error.is_none() {
            if let Err(e) = inner.repo.lock().await.observe_batch(&attempt, batch).await {
                observer_error = Some(e);
            }
        }
    }
    let mut repo = inner.repo.lock().await;
    let current = repo.snapshot(&inner.run_id)?.ok_or(Error::Transition)?;
    let cleanup = !matches!(outcome, Err(Error::CleanupUnknown));
    if let Some(error) = &observer_error {
        remember(inner, error.clone());
    }
    let cancelled = matches!(
        current.state,
        RunState::CancelRequested | RunState::Stopping
    ) || matches!(outcome, Err(Error::Cancelled)) && observer_error.is_none();
    let text = match &outcome {
        Ok(text) => Some(text.clone()),
        Err(_) => None,
    };
    let error = observer_error.or_else(|| match &outcome {
        Err(Error::AwaitingApproval | Error::Cancelled) => None,
        Err(e) => Some(e.clone()),
        _ => None,
    });
    // Parse from persisted transcript, not an uncorrelated return value.
    let mut transcript = String::new();
    let mut after = 0;
    loop {
        let page = repo.transcript(&inner.run_id, &attempt, after, 1000)?;
        if page.is_empty() {
            break;
        }
        for e in page {
            after = e.sequence;
            if let tokage_core::EventKind::Text { text } = e.event {
                transcript.push_str(&text);
            }
        }
    }
    let plan =
        if role == "planner" && error.is_none() && !cancelled && text.as_ref() == Some(&transcript)
        {
            planning::normalize(&transcript, &current.config).ok()
        } else {
            None
        };
    let planning_role = role == "planner";
    let reply = transcript.clone();
    let attempt_for_result = attempt.clone();
    repo.advance(current.revision, move |s| {
        let turn = s.turns.get_mut(&attempt).ok_or(Error::Transition)?;
        let status = turn.finish(
            cancelled,
            cleanup,
            error.map(|e| e.to_string()),
            crate::execution::now(),
        );
        if !planning_role {
            if status == TurnStatus::Completed {
                if let Some(ex) = s.execution.as_mut() {
                    ex.health
                        .insert(turn.requested.provider.clone(), Default::default());
                }
            }
            return Ok(EventKind::TurnFinished);
        }
        s.state = match status {
            TurnStatus::Stopped => {
                s.approved_digest = None;
                RunState::Stopped
            }
            TurnStatus::AwaitingApproval => RunState::AwaitingTurnApproval,
            TurnStatus::Completed => {
                if let Some(plan) = plan {
                    s.plan = Some(plan);
                    RunState::AwaitingApproval
                } else {
                    turn.error = Some("planner text/validation evidence mismatch".into());
                    turn.status = TurnStatus::Failed;
                    RunState::Blocked
                }
            }
            _ => RunState::Blocked,
        };
        Ok(EventKind::TurnFinished)
    })
    .await?;
    let final_state = repo.snapshot(&inner.run_id)?.ok_or(Error::Transition)?;
    let turn = final_state
        .turns
        .get(&attempt_for_result)
        .ok_or(Error::Transition)?;
    match turn.status {
        TurnStatus::Completed => Ok(reply),
        TurnStatus::AwaitingApproval => Err(Error::AwaitingApproval),
        TurnStatus::Stopped => Err(Error::Cancelled),
        TurnStatus::Unknown => Err(Error::CleanupUnknown),
        _ => Err(Error::Invalid(
            turn.error
                .clone()
                .unwrap_or_else(|| "native turn failed".into()),
        )),
    }
}

fn remember<P: Planner>(inner: &Inner<P>, error: Error) {
    if matches!(error, Error::Repository(_) | Error::CleanupUnknown) {
        *inner.failure.lock().unwrap_or_else(|e| e.into_inner()) = Some(error);
    }
}
