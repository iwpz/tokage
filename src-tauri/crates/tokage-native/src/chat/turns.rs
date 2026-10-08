//! Turn registry: register, spawn, and cancel conversation turns (docs/fix-todo F2).
//!
//! F2.a/F2.b keep the nested-spawn shape of the former `start_chat_turn` so aborting the
//! registered (outer) task still does not stop the inner task that owns the CLI.
//! F2.c replaces that with cooperative cancel and a single task.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use tokage_core::{Bus, DomainEvent, EventKind};
use tokio::task::AbortHandle;

use crate::env::ShellEnv;
use crate::providers::ProviderInfo;

use super::{run_cli, run_turn, ChatRequest, Launch};

struct TurnEntry {
    abort: AbortHandle,
    bus: Bus,
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
        self.spawn_inner(bus, turn_id.clone(), async move {
            run_turn(&bus_run, req, providers, env, mirror).await;
            bus_run.publish(DomainEvent::ChatEnded {
                turn_id,
                cancelled: false,
            });
        });
    }

    /// Same registry/cancel path as [`spawn`], but drives `run_cli` with a prepared
    /// `Launch` (FakeAdapter cancel reproduction tests).
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
        self.spawn_inner(bus, turn_id.clone(), async move {
            let filter = super::filter::OutputFilter::new(&env);
            let emit = |event: EventKind| {
                super::process_event_for_emit(event, &filter, |ev| {
                    bus_run.publish(DomainEvent::ChatOutput {
                        turn_id: turn_id.clone(),
                        event: ev,
                    });
                });
            };
            let _ = run_cli(&exe, &req, launch, &env, mirror, None, &emit, None).await;
            bus_run.publish(DomainEvent::ChatEnded {
                turn_id,
                cancelled: false,
            });
        });
    }

    fn spawn_inner(
        &self,
        bus: Bus,
        turn_id: String,
        run: impl std::future::Future<Output = ()> + Send + 'static,
    ) {
        let registry = self.clone();
        // Held across spawn so the task can't remove its entry before it is inserted.
        let mut running = self.turns.lock().unwrap_or_else(|e| e.into_inner());
        let task = tokio::spawn({
            let id = turn_id.clone();
            async move {
                // Nested spawn preserves the F2 bug: aborting this outer task drops the
                // JoinHandle without aborting `run`, so the CLI keeps running.
                let run_task = tokio::spawn(run);
                let _ = run_task.await;
                registry
                    .turns
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&id);
            }
        });
        running.insert(
            turn_id,
            TurnEntry {
                abort: task.abort_handle(),
                bus,
            },
        );
    }

    /// Abort the registered outer task and publish `ChatEnded { cancelled: true }`.
    /// Returns whether the turn was still registered. Does not wait for the CLI to exit (F2.c).
    pub async fn cancel(&self, turn_id: &str) -> bool {
        let entry = self
            .turns
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(turn_id);
        if let Some(entry) = entry {
            entry.abort.abort();
            entry.bus.publish(DomainEvent::ChatEnded {
                turn_id: turn_id.to_string(),
                cancelled: true,
            });
            true
        } else {
            false
        }
    }
}
