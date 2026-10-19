//! Turn registry: register, spawn, and cancel conversation turns (docs/fix-todo F2).
//!
//! One task per turn owns `run_turn` / `run_cli`. Cancel is cooperative via a `watch`
//! signal; `abort()` is only an 8s fallback if the task does not finish.

use std::{
    collections::{HashMap, HashSet},
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
    pending: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    cancelled: Arc<Mutex<HashSet<String>>>,
}

pub struct PendingTurn { registry: TurnRegistry, id: String, cancelled: Arc<AtomicBool> }
impl PendingTurn { pub fn cancelled(&self) -> bool { self.cancelled.load(Ordering::SeqCst) } }
impl Drop for PendingTurn {
    fn drop(&mut self) { self.registry.pending.lock().unwrap_or_else(|e| e.into_inner()).remove(&self.id); }
}

impl TurnRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn has_active(&self) -> bool {
        let pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        let running = self.turns.lock().unwrap_or_else(|e| e.into_inner());
        !pending.is_empty() || !running.is_empty()
    }

    /// Covers asynchronous detection/routing, including cancellation before registration.
    pub fn reserve(&self, id: &str) -> Result<PendingTurn, String> {
        let cancelled_ids = self.cancelled.lock().unwrap_or_else(|e| e.into_inner());
        if cancelled_ids.contains(id) { return Err("Turn was cancelled before submission".into()); }
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if pending.contains_key(id) { return Err("Turn submission already pending".into()); }
        let cancelled = Arc::new(AtomicBool::new(false));
        pending.insert(id.to_string(), cancelled.clone());
        Ok(PendingTurn { registry: self.clone(), id: id.to_string(), cancelled })
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
    ) -> bool {
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
        })
    }

    /// Mock uses the same owner/cancellation/ended path without spawning any CLI.
    pub fn spawn_mock(&self, bus: Bus, req: ChatRequest) -> bool {
        let turn_id = req.turn_id.clone();
        let bus_run = bus.clone();
        self.spawn_inner(bus, turn_id.clone(), move |cancel| async move {
            let cancelled = super::debug::run_mock(&bus_run, &req, cancel).await;
            bus_run.publish(DomainEvent::ChatEnded { turn_id, cancelled });
        })
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
    ) -> bool {
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
        })
    }

    fn spawn_inner<F, Fut>(&self, bus: Bus, turn_id: String, run: F) -> bool
    where
        F: FnOnce(watch::Receiver<bool>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let ended = Arc::new(AtomicBool::new(false));
        let registry = self.clone();
        let ended_flag = ended.clone();
        let id = turn_id.clone();
        // Same lock order as cancel/reserve; cancel cannot miss the handoff to running.
        let cancelled_ids = self.cancelled.lock().unwrap_or_else(|e| e.into_inner());
        if cancelled_ids.contains(&turn_id) { return false; }
        let _pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        // Held across spawn so the task can't remove its entry before it is inserted.
        let mut running = self.turns.lock().unwrap_or_else(|e| e.into_inner());
        if running.contains_key(&turn_id) { return false; }
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
        true
    }

    /// Signal cancel, wait for the turn task (and CLI process group) to finish.
    /// Aborts the task after 8s if it has not ended. Returns whether the turn was registered.
    pub async fn cancel(&self, turn_id: &str) -> bool {
        self.cancelled.lock().unwrap_or_else(|e| e.into_inner()).insert(turn_id.to_string());
        let was_pending = {
            let pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(flag) = pending.get(turn_id) { flag.store(true, Ordering::SeqCst); true } else { false }
        };
        let entry = self
            .turns
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(turn_id);
        let Some(mut entry) = entry else {
            return was_pending;
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

#[cfg(test)]
mod recovery_tests {
    use super::*;
    #[tokio::test]
    async fn m2e_cancel_fences_pending_and_late_native_launches() {
        let registry = TurnRegistry::new();
        let pending = registry.reserve("pending").unwrap();
        assert!(registry.cancel("pending").await);
        assert!(pending.cancelled());
        drop(pending);
        assert!(!registry.has_active());
        assert!(registry.reserve("pending").is_err());
        assert!(!registry.cancel("late").await);
        assert!(registry.reserve("late").is_err());
        let mut request = super::super::testing::request(tokage_core::ProviderId::ClaudeCode);
        request.turn_id = "late".into();
        assert!(!registry.spawn_mock(Bus::new(), request));
        assert!(!registry.has_active());
    }

    #[test]
    fn m2e_pending_native_submission_is_not_reported_idle() {
        let registry = TurnRegistry::new();
        let pending = registry.reserve("one").unwrap();
        assert!(registry.has_active());
        assert!(registry.reserve("one").is_err());
        drop(pending);
        assert!(!registry.has_active());
    }
}
