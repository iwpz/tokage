//! Native environment (docs/01): login shell env, CLI detection, headless adapters, process
//! groups, git. Talks to the app only through return values and the `tokage_core::Bus`.

pub mod chat;
pub mod context;
pub mod env;
pub mod git;
pub mod install;
pub mod models;
pub mod proc;
pub mod providers;
pub mod proxy;
pub mod sandbox;
pub mod session;
pub mod usage;
