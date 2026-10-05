//! Domain types shared by every Tokage crate, and the internal event bus (docs/00 §5–6).
//! No IO lives here.

pub mod auth;
pub mod bus;
pub mod command_policy;
pub mod event;
pub mod planning;
pub mod provider;
pub mod proxy;
pub mod session;

pub use auth::{AuthMode, Role};
pub use bus::{Bus, DomainEvent};
pub use event::{Change, ChatEvent, EventKind, ToolKind, TurnErrorKind, TurnUsage, UsageSource};
pub use planning::{HotspotItem, ManifestSummary, ProjectCommands, RepoDigest};
pub use provider::{ProviderId, ProviderStatus};
pub use proxy::{
    CliProxyConfig, GlobalProxyConfig, ProviderProxyConfig, ProviderProxyStatus, ProxyMode,
    ProxyProtocol, ProxyTestResult, ProxyUpstream,
};
pub use session::NativeSession;
