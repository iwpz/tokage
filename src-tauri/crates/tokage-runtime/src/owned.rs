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
struct Inner<P: Planner> {
    repo: Mutex<SqliteRepository>,
    run_id: String,
    planner: P,
    jobs: SyncMutex<BTreeMap<String, watch::Sender<bool>>>,
    failure: SyncMutex<Option<Error>>,
}
pub struct Runtime<P: Planner> {
    inner: Arc<Inner<P>>,
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
    pub async fn submit(&self, request: &Request) -> Result<Receipt> {
        let mut repo = self.inner.repo.lock().await;
        if matches!(request.command, Command::ResumeRun)
            && !self
                .inner
                .jobs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_empty()
        {
            return Err(Error::Transition);
        }
        let receipt = repo.submit(request).await?;
        let current = repo
            .snapshot(&self.inner.run_id)?
            .ok_or(Error::Transition)?;
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
            if has_jobs {
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
            let id = attempt.clone();
            repo.advance(current.revision, move |s| {
                s.planning_attempt.as_mut().unwrap().claimed = true;
                s.turns.insert(
                    id.clone(),
                    TurnRecord::prepared(id, "planner", None, selected),
                );
                Ok(EventKind::PlanningClaimed)
            })
            .await?;
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
                    run_owned(&worker_inner, snapshot, worker_attempt, cancel_rx).await
                });
                let result = worker.await.unwrap_or_else(|_| {
                    Err(Error::Repository(
                        "turn worker terminated; native effect requires reconciliation".into(),
                    ))
                });
                if let Err(error) = result {
                    *inner.failure.lock().unwrap_or_else(|e| e.into_inner()) = Some(error);
                }
                inner
                    .jobs
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&attempt);
            });
        }
        Ok(receipt)
    }
}
async fn run_owned<P: Planner + 'static>(
    inner: &Arc<Inner<P>>,
    snapshot: Snapshot,
    attempt: String,
    cancel: watch::Receiver<bool>,
) -> Result<()> {
    let (tx, mut rx) = mpsc::channel(64);
    let control = TurnControl { events: tx, cancel };
    let mut future = Box::pin(
        inner
            .planner
            .generate_observed(&snapshot, &attempt, control),
    );
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
                            if let Some(tx)=inner.jobs.lock().unwrap_or_else(|e|e.into_inner()).get(&attempt){let _=tx.send(true);}
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
    let cancelled = matches!(
        current.state,
        RunState::CancelRequested | RunState::Stopping
    ) || matches!(outcome, Err(Error::Cancelled));
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
    let plan = if error.is_none() && !cancelled && text.as_ref() == Some(&transcript) {
        planning::normalize(&transcript, &current.config).ok()
    } else {
        None
    };
    repo.advance(current.revision, move |s| {
        let turn = s.turns.get_mut(&attempt).ok_or(Error::Transition)?;
        let status = turn.finish(cancelled, cleanup, error.map(|e| e.to_string()));
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
    .await
}
