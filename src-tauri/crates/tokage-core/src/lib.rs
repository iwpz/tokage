//! Domain types shared by every Tokage crate, and the internal event bus (docs/00 §5–6).
//! No IO lives here.

pub mod auth;
pub mod bus;
pub mod command_policy;
pub mod event;
pub mod provider;

pub use bus::{Bus, DomainEvent};
pub use event::{Change, ChatEvent, EventKind, ToolKind};
pub use provider::{ProviderId, ProviderStatus};
