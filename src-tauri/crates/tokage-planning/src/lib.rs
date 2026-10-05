//! Tokage planning engine: repo digest, triage, and plan management (docs/02).

pub mod digest;

pub use digest::{generate_digest, RepoDigestBuilder};
