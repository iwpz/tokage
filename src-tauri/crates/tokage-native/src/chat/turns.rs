//! Turn registry: register, spawn, and cancel conversation turns (docs/fix-todo F2).
//!
//! F2.a keeps the nested-spawn shape of the former `start_chat_turn` so aborting the
//! registered (outer) task still does not stop the inner `run_turn` that owns the CLI.
//! F2.c replaces that with cooperative cancel and a single task.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use tokage_core::{Bus, DomainEvent};
use tokio::task::AbortHandle;

use crate::env::ShellEnv;
use crate::providers::ProviderInfo;

use super::{run_turn, ChatRequest};

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

    /// Register and start a turn: one outer task awaits an inner `run_turn` task.
    /// The turn publishes exactly one `ChatEnded` when the inner task finishes.
    /// Usage listening is the caller's responsibility (subscribe until `ChatEnded`).
    pub fn spawn(
        &self,
        bus: Bus,
        req: ChatRequest,
        providers: Vec<ProviderInfo>,
        env: ShellEnv,
        mirror: Option<PathBuf>,
    ) {
        let id = req.turn_id.clone();
        let registry = self.clone();
        // Held across spawn so the task can't remove its entry before it is inserted.
        let mut running = self.turns.lock().unwrap_or_else(|e| e.into_inner());
        let task = tokio::spawn({
            let id = id.clone();
            let bus = bus.clone();
            async move {
                let run_id = id.clone();
                let run_bus = bus;
                let run_task = tokio::spawn(async move {
                    run_turn(&run_bus, req, providers, env, mirror).await;
                    run_bus.publish(DomainEvent::ChatEnded { turn_id: run_id });
                });
                let _ = run_task.await;
                registry
                    .turns
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&id);
            }
        });
        running.insert(
            id,
            TurnEntry {
                abort: task.abort_handle(),
                bus,
            },
        );
    }

    /// Abort the registered outer task and publish `ChatEnded`. Returns whether the
    /// turn was still registered. Does not wait for the CLI process group to exit (F2.c).
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
            });
            true
        } else {
            false
        }
    }
}
