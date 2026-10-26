//! Bounded local metadata only. Never stores credentials, identities, paths or CLI output.
use super::contract::{QueryError, QueryResult, QueryStatus};
use serde::{Deserialize, Serialize};
use std::{path::Path, sync::Mutex};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum TokenState {
    Missing,
    UnknownExpiry,
    Valid,
    Expired,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CredentialState {
    access: TokenState,
    refresh: TokenState,
    access_expires_at: Option<i64>,
    refresh_expires_at: Option<i64>,
}
impl CredentialState {
    pub fn from_oauth(oauth: &serde_json::Value, now: i64) -> Self {
        fn state(
            oauth: &serde_json::Value,
            key: &str,
            expiry: &str,
            now: i64,
        ) -> (TokenState, Option<i64>) {
            if oauth[key].as_str().is_none_or(|s| s.is_empty()) {
                return (TokenState::Missing, None);
            }
            let expires = oauth[expiry].as_i64().filter(|n| *n > 0).map(|n| n / 1000);
            (
                match expires {
                    None => TokenState::UnknownExpiry,
                    Some(t) if t <= now => TokenState::Expired,
                    _ => TokenState::Valid,
                },
                expires,
            )
        }
        let (access, access_expires_at) = state(oauth, "accessToken", "expiresAt", now);
        let (refresh, refresh_expires_at) =
            state(oauth, "refreshToken", "refreshTokenExpiresAt", now);
        Self {
            access,
            refresh,
            access_expires_at,
            refresh_expires_at,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum Phase {
    BeforeQuery,
    QueryReturned,
    AfterQuery,
    Finished,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Event {
    pub observed_at: i64,
    pub attempt: String,
    pub phase: Phase,
    pub cli_version: Option<String>,
    pub credentials: Option<CredentialState>,
    pub credentials_changed: Option<bool>,
    pub error: Option<QueryError>,
    pub status: Option<QueryStatus>,
    pub pty_stage: Option<PtyStage>,
    pub elapsed_ms: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum PtyStage {
    Prompt,
    Completion,
    Result,
}
pub(crate) fn pty_stage(message: &str) -> Option<PtyStage> {
    match message {
        "PTY execution timed out before input readiness" => Some(PtyStage::Prompt),
        "PTY execution timed out before command submission" => Some(PtyStage::Completion),
        "PTY execution timed out waiting for result" => Some(PtyStage::Result),
        _ => None,
    }
}
pub(crate) struct Journal<'a> {
    root: &'a Path,
    version: Option<String>,
    started: std::time::Instant,
    attempt: String,
}
impl<'a> Journal<'a> {
    pub fn new(root: &'a Path, version: Option<&str>) -> Self {
        // Only a numeric semantic version is admitted, never raw version command output.
        let version = version.and_then(|v| {
            let parts: Vec<_> = v.split('.').collect();
            (parts.len() == 3
                && parts.iter().all(|p| {
                    !p.is_empty() && p.len() <= 8 && p.bytes().all(|b| b.is_ascii_digit())
                }))
            .then(|| v.to_string())
        });
        Self {
            root,
            version,
            started: std::time::Instant::now(),
            attempt: format!(
                "{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ),
        }
    }
    pub fn event(
        &self,
        phase: Phase,
        credentials: Option<CredentialState>,
        credentials_changed: Option<bool>,
        error: Option<QueryError>,
        status: Option<QueryStatus>,
        pty_stage: Option<PtyStage>,
    ) {
        let event = Event {
            attempt: self.attempt.clone(),
            observed_at: super::now(),
            phase,
            cli_version: self.version.clone(),
            credentials,
            credentials_changed,
            error,
            status,
            pty_stage,
            elapsed_ms: self.started.elapsed().as_millis().min(u64::MAX as u128) as u64,
        };
        if persist(self.root, event).is_err() {
            tracing::warn!("Claude lifecycle diagnostic persistence unavailable");
        }
    }
    pub fn finish(&self, result: &QueryResult) {
        self.event(
            Phase::Finished,
            None,
            None,
            result.error.clone(),
            Some(result.status.clone()),
            None,
        );
    }
}
fn persist(root: &Path, event: Event) -> Result<(), String> {
    static LOCK: Mutex<()> = Mutex::new(());
    let _lock = LOCK.lock().map_err(|_| "Diagnostic lock unavailable")?;
    let path = root.join("claude-auth-lifecycle-v1.json");
    let mut events: Vec<Event> = super::read_bounded(&path, 262144)
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    events.push(event);
    if events.len() > 128 {
        events.drain(..events.len() - 128);
    }
    super::atomic_json(&path, &events)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_expiry_unknown_refresh_and_missing_tokens_remain_distinct() {
        let oauth = serde_json::json!({"accessToken":"SECRET_ACCESS", "refreshToken":"SECRET_REFRESH", "expiresAt":100000});
        let before = CredentialState::from_oauth(&oauth, 99);
        assert_eq!(before.access, TokenState::Valid);
        let at = CredentialState::from_oauth(&oauth, 100);
        assert_eq!(at.access, TokenState::Expired);
        assert_eq!(at.refresh, TokenState::UnknownExpiry);
        assert_eq!(
            CredentialState::from_oauth(&serde_json::json!({}), 100).access,
            TokenState::Missing
        );
        assert!(!serde_json::to_string(&at).unwrap().contains("SECRET"));
    }
    #[test]
    fn bounded_private_journal_survives_reopen_and_excludes_raw_version() {
        let root = tempfile::tempdir().unwrap();
        let journal = Journal::new(root.path(), Some("2.1.288 SECRET"));
        for _ in 0..140 {
            journal.finish(&QueryResult::empty(QueryStatus::Latest, 100));
        }
        let bytes = std::fs::read(root.path().join("claude-auth-lifecycle-v1.json")).unwrap();
        let events: Vec<Event> = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(events.len(), 128);
        assert!(events.iter().all(|e| e.cli_version.is_none()));
        Journal::new(root.path(), Some("2.1.288"))
            .finish(&QueryResult::empty(QueryStatus::Failed, 101));
        let events: Vec<Event> = serde_json::from_slice(
            &std::fs::read(root.path().join("claude-auth-lifecycle-v1.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            events.last().unwrap().cli_version.as_deref(),
            Some("2.1.288")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(root.path().join("claude-auth-lifecycle-v1.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
}
