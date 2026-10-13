//! Turn registry: register, spawn, and cancel conversation turns (docs/fix-todo F2).
//!
//! One task per turn owns `run_turn` / `run_cli`. Cancel is cooperative via a `watch`
//! signal; `abort()` is only an 8s fallback if the task does not finish.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use tokage_core::{Bus, DomainEvent, EventKind};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::env::ShellEnv;
use crate::providers::ProviderInfo;

use super::{run_cli, run_turn, ChatRequest, Launch};

const CANCEL_ABORT_AFTER: Duration = Duration::from_secs(8);

struct TurnEntry {
    cancel_tx: watch::Sender<bool>,
    join: JoinHandle<()>,
    bus: Bus,
    ended: Arc<AtomicBool>,
}

/// Running turns keyed by `turn_id`.
#[derive(Clone, Default)]
pub struct TurnRegistry {
    turns: Arc<Mutex<HashMap<String, TurnEntry>>>,
}

impl TurnRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register and start a turn via `run_turn`. Usage listening is the caller's job.
    /// Publishes exactly one `ChatEnded` when the turn finishes (normal, failed, or cancelled).
    pub fn spawn(
        &self,
        bus: Bus,
        req: ChatRequest,
        providers: Vec<ProviderInfo>,
        env: ShellEnv,
        mirror: Option<PathBuf>,
    ) {
        let turn_id = req.turn_id.clone();
        let bus_run = bus.clone();
        self.spawn_inner(bus, turn_id.clone(), move |cancel_rx| {
            let bus_run = bus_run;
            let turn_id = turn_id;
            async move {
                let cancelled =
                    run_turn(&bus_run, req, providers, env, mirror, Some(cancel_rx)).await;
                bus_run.publish(DomainEvent::ChatEnded {
                    turn_id,
                    cancelled,
                });
            }
        });
    }

    /// Mock uses the same owner/cancellation/ended path without spawning any CLI.
    pub fn spawn_mock(&self, bus: Bus, req: ChatRequest) {
        let turn_id = req.turn_id.clone();
        let bus_run = bus.clone();
        self.spawn_inner(bus, turn_id.clone(), move |cancel| async move {
            let cancelled = super::debug::run_mock(&bus_run, &req, cancel).await;
            bus_run.publish(DomainEvent::ChatEnded { turn_id, cancelled });
        });
    }

    /// Same registry/cancel path as [`spawn`], but drives `run_cli` with a prepared
    /// `Launch` (FakeAdapter cancel tests).
    pub fn spawn_prepared(
        &self,
        bus: Bus,
        req: ChatRequest,
        exe: PathBuf,
        launch: Launch,
        env: ShellEnv,
        mirror: Option<PathBuf>,
    ) {
        let turn_id = req.turn_id.clone();
        let bus_run = bus.clone();
        self.spawn_inner(bus, turn_id.clone(), move |cancel_rx| {
            let bus_run = bus_run;
            let turn_id = turn_id;
            async move {
                let filter = super::filter::OutputFilter::new(&env);
                let emit = |event: EventKind| {
                    super::process_event_for_emit(event, &filter, |ev| {
                        bus_run.publish(DomainEvent::ChatOutput {
                            turn_id: turn_id.clone(),
                            event: ev,
                        });
                    });
                };
                let cancelled = matches!(
                    run_cli(
                        &exe,
                        &req,
                        launch,
                        &env,
                        mirror,
                        None,
                        &emit,
                        Some(cancel_rx),
                    )
                    .await,
                    Err(ref m) if m == super::TURN_CANCELLED
                );
                bus_run.publish(DomainEvent::ChatEnded {
                    turn_id,
                    cancelled,
                });
            }
        });
    }

    fn spawn_inner<F, Fut>(&self, bus: Bus, turn_id: String, run: F)
    where
        F: FnOnce(watch::Receiver<bool>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let ended = Arc::new(AtomicBool::new(false));
        let registry = self.clone();
        let ended_flag = ended.clone();
        let id = turn_id.clone();
        // Held across spawn so the task can't remove its entry before it is inserted.
        let mut running = self.turns.lock().unwrap_or_else(|e| e.into_inner());
        let join = tokio::spawn(async move {
            run(cancel_rx).await;
            ended_flag.store(true, Ordering::SeqCst);
            registry
                .turns
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&id);
        });
        running.insert(
            turn_id,
            TurnEntry {
                cancel_tx,
                join,
                bus,
                ended,
            },
        );
    }

    /// Signal cancel, wait for the turn task (and CLI process group) to finish.
    /// Aborts the task after 8s if it has not ended. Returns whether the turn was registered.
    pub async fn cancel(&self, turn_id: &str) -> bool {
        let entry = self
            .turns
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(turn_id);
        let Some(mut entry) = entry else {
            return false;
        };
        let _ = entry.cancel_tx.send(true);
        tokio::select! {
            result = &mut entry.join => {
                let _ = result;
            }
            _ = tokio::time::sleep(CANCEL_ABORT_AFTER) => {
                entry.join.abort();
                let _ = (&mut entry.join).await;
                if !entry.ended.load(Ordering::SeqCst) {
                    entry.bus.publish(DomainEvent::ChatEnded {
                        turn_id: turn_id.to_string(),
                        cancelled: true,
                    });
                }
            }
        }
        true
    }
}
