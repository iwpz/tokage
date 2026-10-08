//! Internal event bus (docs/00 §6). Engines publish what happened; the app shell and other
//! engines subscribe. Every subscriber gets every event in publish order, none are dropped:
//! a subscriber that stops reading only grows its own queue, and one that is dropped is
//! removed on the next publish.

use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;

use crate::{event::EventKind, provider::ProviderId, provider::ProviderStatus};

#[derive(Clone, Debug, PartialEq)]
pub enum DomainEvent {
    /// A detection probe finished for one CLI.
    ProviderDetected { provider: ProviderId, status: ProviderStatus },
    /// A provider's circuit breaker tripped due to rate limiting or quota exhaustion (docs/06 §6).
    ProviderMelted { provider: ProviderId, reason: String },
    /// One event of a running conversation turn (no attempt record yet; that comes with P7).
    ChatOutput { turn_id: String, event: EventKind },
    /// A conversation turn ended: done, failed or cancelled. Always the turn's last event.
    ChatEnded { turn_id: String, cancelled: bool },
    /// Local triage decided the route for a prompt or main session (docs/00 §6, docs/02 §2).
    TriageDone {
        main_id: Option<i64>,
        route: crate::planning::TriageRoute,
    },
    /// Orchestration task state changed (docs/00 §6, docs/03 §2, P7.m1).
    TaskStateChanged {
        plan_id: String,
        task_id: String,
        old_state: String,
        new_state: String,
        reason: Option<String>,
    },
    /// A new official knowledge pack update was verified and is pending application (docs/04 §10, P8.m4).
    KnowledgePackUpdateAvailable {
        pack_version: String,
        pending_path: String,
    },
}

/// Cheap to clone; clones share the same subscribers.
#[derive(Clone, Default)]
pub struct Bus {
    subscribers: Arc<Mutex<Vec<mpsc::UnboundedSender<DomainEvent>>>>,
}

impl Bus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn publish(&self, event: DomainEvent) {
        let mut subs = self.subscribers.lock().unwrap_or_else(|e| e.into_inner());
        subs.retain(|s| s.send(event.clone()).is_ok());
    }

    /// Receives every event published from now on.
    pub fn subscribe(&self) -> mpsc::UnboundedReceiver<DomainEvent> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.subscribers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(tx);
        rx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(turn: &str, t: &str) -> DomainEvent {
        DomainEvent::ChatOutput {
            turn_id: turn.into(),
            event: EventKind::Text { text: t.into() },
        }
    }

    #[tokio::test]
    async fn every_subscriber_gets_every_event_in_order() {
        let bus = Bus::new();
        let mut a = bus.subscribe();
        let mut b = bus.clone().subscribe();
        for i in 0..1000 {
            bus.publish(text("t", &i.to_string()));
        }
        for rx in [&mut a, &mut b] {
            for i in 0..1000 {
                assert_eq!(rx.recv().await, Some(text("t", &i.to_string())));
            }
        }
    }

    #[tokio::test]
    async fn late_subscriber_sees_only_later_events_and_dropped_ones_are_removed() {
        let bus = Bus::new();
        let early = bus.subscribe();
        bus.publish(text("t", "before"));
        let mut late = bus.subscribe();
        drop(early);
        bus.publish(DomainEvent::ChatEnded {
            turn_id: "t".into(),
            cancelled: false,
        });
        assert_eq!(bus.subscribers.lock().unwrap().len(), 1);
        assert_eq!(
            late.recv().await,
            Some(DomainEvent::ChatEnded {
                turn_id: "t".into(),
                cancelled: false,
            })
        );
    }
}
