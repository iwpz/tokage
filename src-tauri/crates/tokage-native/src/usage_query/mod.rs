//! Identity-bound account quota. No task usage events are rewritten by this service.
pub mod antigravity;
pub mod aider;
pub mod contract;
pub mod codex;
pub mod copilot;
pub mod kiro;
pub(crate) mod credentials;
pub mod http;
pub mod identity;
pub mod opencode;
pub mod scoped;
pub mod reasonix;
use crate::{env::ShellEnv, online_usage::OnlineUsagePermission};
use contract::*;
use futures::{
    future::{BoxFuture, Shared},
    FutureExt,
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokage_core::{proxy::GlobalProxyConfig, ProviderId};

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
struct Entry {
    identity: Option<String>,
    generation: u64,
    #[serde(default)]
    state_revision: u64,
    success: Option<AccountSnapshot>,
    last: Option<QueryResult>,
    failures: u32,
    next_at: i64,
    auth_blocked: bool,
    #[serde(skip)]
    source_state: Option<QueryStatus>,
    #[serde(skip)]
    source_login: Option<SourceLogin>,
    #[serde(skip)]
    verified: bool,
    #[serde(skip)]
    dispatch_acknowledged: bool,
}
impl Entry {
    fn verify(&mut self) {
        if !self.verified || self.source_state.is_some() {
            self.state_revision += 1;
        }
        self.verified = true;
        self.source_state = None;
    }
    fn project(&self, mut result: QueryResult) -> QueryResult {
        // Current binding proof belongs to the result envelope. The snapshot keeps
        // its query generation, fetch time and TTL, even after source revalidation.
        result.identity_generation = Some(self.generation);
        result.state_revision = Some(self.state_revision);
        result.latest_success = self.success.clone();
        result.source_login = self.source_login.clone().filter(|login| self.verified || matches!(login.status, SourceLoginStatus::NeedsLogin));
        if self.verified { result.identity_key = self.identity.clone(); }
        result
    }
}
#[derive(Clone)]
pub struct QueryRequest {
    pub provider: ProviderId,
    pub exe: PathBuf,
    pub env: ShellEnv,
    pub proxy: GlobalProxyConfig,
    pub automatic: bool,
    pub revision: u64,
    pub deadline: std::time::Instant,
}
type Flight = Shared<BoxFuture<'static, QueryResult>>;
struct FlightEntry {
    serial: u64,
    result: Flight,
}
/// Full revision projection survives coalesced watch events; cancellation is per provider.
#[derive(Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageRevisions {
    pub all: u64,
    pub providers: HashMap<ProviderId, u64>,
    serial: u64,
}
impl UsageRevisions {
    pub fn for_provider(&self, id: ProviderId) -> u64 { self.all.max(self.providers.get(&id).copied().unwrap_or(0)) }
}
pub struct UsageQueryService {
    permission: Arc<OnlineUsagePermission>,
    root: PathBuf,
    salt: Option<[u8; 32]>,
    routing: Mutex<(
        Option<HashMap<ProviderId, bool>>,
        Option<HashMap<ProviderId, String>>,
    )>,
    entries: Mutex<HashMap<ProviderId, Entry>>,
    flights: tokio::sync::Mutex<HashMap<(ProviderId, u64), FlightEntry>>,
    serial: std::sync::atomic::AtomicU64,
    slots: tokio::sync::Semaphore,
    changed: tokio::sync::watch::Sender<UsageRevisions>,
}
fn atomic_json(path: &Path, value: &impl serde::Serialize) -> Result<(), String> {
    use std::io::Write;
    let parent = path.parent().ok_or("Invalid usage record path")?;
    std::fs::create_dir_all(parent).map_err(|_| "Usage persistence unavailable")?;
    let mut file =
        tempfile::NamedTempFile::new_in(parent).map_err(|_| "Usage persistence unavailable")?;
    let bytes = serde_json::to_vec(value).map_err(|_| "Invalid usage record")?;
    if bytes.len() > 1024 * 1024 {
        return Err("Usage record exceeds local size limit".into());
    }
    file.write_all(&bytes)
        .and_then(|_| file.as_file().sync_all())
        .map_err(|_| "Usage persistence unavailable")?;
    file.persist(path)
        .map_err(|_| "Usage persistence unavailable")?;
    Ok(())
}
fn read_bounded(path: &Path, limit: u64) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= limit).then_some(bytes)
}
fn salt(root: &Path) -> Option<[u8; 32]> {
    let path = root.join("identity-salt");
    if path.exists() {
        return read_bounded(&path, 32)?.try_into().ok();
    }
    std::fs::create_dir_all(root).ok()?;
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).ok()?;
    use std::io::Write;
    let mut f = tempfile::NamedTempFile::new_in(root).ok()?;
    f.write_all(&bytes).ok()?;
    f.as_file().sync_all().ok()?;
    // Never overwrite another installation/process's newly created salt.
    match f.persist_noclobber(&path) {
        Ok(_) => Some(bytes),
        Err(_) => read_bounded(&path, 32)?.try_into().ok(),
    }
}
impl UsageQueryService {
    pub fn open(root: &Path, permission: Arc<OnlineUsagePermission>) -> Self {
        let entries: HashMap<ProviderId, Entry> =
            read_bounded(&root.join("snapshots-v2.json"), 1024 * 1024)
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or_default();
        let entries = entries
            .into_iter()
            .filter(|(id, e)| {
                e.generation < 9_007_199_254_740_991
                    && e.success.as_ref().map_or(true, |s| {
                        validate(s)
                            && s.scope.provider == id.as_str()
                            && e.identity.as_deref() == Some(s.identity_key.as_str())
                            && e.generation >= s.identity_generation
                    })
            })
            .map(|(id, mut entry)| { if let Some(last) = &mut entry.last { last.source_login = None; } (id,entry) })
            .collect();
        Self {
            permission,
            root: root.into(),
            salt: salt(root),
            routing: Mutex::new((None, None)),
            entries: Mutex::new(entries),
            flights: tokio::sync::Mutex::new(HashMap::new()),
            serial: std::sync::atomic::AtomicU64::new(0),
            slots: tokio::sync::Semaphore::new(2),
            changed: tokio::sync::watch::channel(UsageRevisions::default()).0,
        }
    }
    /// Local-only read. Persisted data has no current identity proof after restart.
    pub fn inspect(&self, id: ProviderId) -> QueryResult {
        if matches!(capability(id).state, "UNSUPPORTED" | "ADAPTER_PENDING" | "IDENTITY_PENDING") { return QueryResult::empty(QueryStatus::Unsupported, 0); }
        let entries = self.entries.lock().unwrap();
        let Some(e) = entries.get(&id) else {
            return QueryResult::empty(QueryStatus::NotRequested, 0);
        };
        if !e.verified {
            let mut r = QueryResult::empty(e.source_state.clone().unwrap_or(QueryStatus::Unverified), e.last.as_ref().map_or(0, |r| r.attempted_at));
            r.next_retry_at = (e.next_at > 0).then_some(e.next_at);
            r.error = e.last.as_ref().and_then(|r| r.error.clone());
            r.identity_generation = Some(e.generation);
            r.state_revision = Some(e.state_revision);
            r.latest_success = e.success.clone();
            r.from_cache = r.latest_success.is_some();
            return e.project(r);
        }
        let mut r = e.project(
            e.last.clone().unwrap_or_else(|| QueryResult::empty(QueryStatus::NotRequested, 0)),
        );
        if r.status == QueryStatus::Latest
            && r.latest_success
                .as_ref()
                .is_some_and(|s| s.valid_until <= now())
        {
            r.status = QueryStatus::Expired;
            r.from_cache = true;
        }
        r
    }
    /// Explicit per-card rescan can confirm the CLI's normal READY source without
    /// pretending unsupported subscription quotas are known (e.g. switching to API key).
    pub fn acknowledge_scanned_source(&self, id: ProviderId, ready: bool) {
        if !ready {
            return;
        }
        if let Ok(mut entries) = self.entries.lock() {
            if let Some(e) = entries.get_mut(&id) {
                e.dispatch_acknowledged = true;
                // Explicit READY rescan permits another auth attempt after re-login.
                // It does not bypass a server's Retry-After cooldown.
                if e.auth_blocked { e.auth_blocked = false; }
            }
        }
    }
    pub async fn reconcile_sources(&self, providers: &mut [crate::providers::ProviderInfo], env: &ShellEnv) {
        futures::future::join_all(providers.iter().map(|p| self.reconcile_source(p.id, p.path.as_deref().map(Path::new), env))).await;
        for provider in providers { self.apply_source_login(provider); }
    }
    pub fn apply_source_login(&self, provider: &mut crate::providers::ProviderInfo) {
        if let Some(entry) = self.entries.lock().unwrap().get(&provider.id) {
            if let Some(login) = entry.source_login.as_ref().filter(|l| entry.verified || matches!(l.status, SourceLoginStatus::NeedsLogin)) { login.apply(provider); }
        }
    }
    fn record_source_login(&self, id: ProviderId, key: &str, generation: u64, login: Option<SourceLogin>) {
        let Some(login) = login else { return; };
        if let Some(e) = self.entries.lock().unwrap().get_mut(&id) {
            if e.generation == generation && e.identity.as_deref() == Some(key) && e.verified {
                e.source_login = Some(login); e.state_revision += 1;
            }
        }
    }
    fn record_signed_out(&self, id: ProviderId, exe: &Path, generation: u64) {
        if id != ProviderId::CursorCli { return; }
        if let Some(e)=self.entries.lock().unwrap().get_mut(&id) {
            if e.generation != generation || e.verified { return; }
            e.source_login=Some(SourceLogin { path:exe.to_string_lossy().into_owned(), observed_at:now(), status:SourceLoginStatus::NeedsLogin, auth_method:None, account:None });
            e.state_revision += 1;
        }
    }
    /// A status scan revalidates an existing binding; it never fetches quota or resets cooldown.
    pub async fn reconcile_source(&self, id: ProviderId, exe: Option<&Path>, env: &ShellEnv) {
        let binding = self.entries.lock().unwrap().get(&id).map(|e| (e.identity.clone(),e.generation));
        let expected = binding.as_ref().and_then(|(key,_)| key.clone());
        let observed_generation = binding.map(|(_,generation)|generation);
        if expected.is_none() && id != ProviderId::CursorCli { return; }
        let Some(salt) = self.salt else { return; };
        let filtered = ShellEnv { vars: crate::chat::child_env::assemble(id, env, &[]) };
        let mut login = None;
        let current = if let Some(exe) = exe {
            match identity::observe(id, exe, &filtered, &salt).await {
                Ok(o) => { login=o.source_login; Some(o.identity) },
                Err(error) if Self::transient_source_error(&error) => { self.source_unavailable(id, observed_generation, error); return; }
                Err(error) => {
                    if self.invalidate_observed(id,observed_generation) {
                        if error.category==QueryError::Authentication { self.record_signed_out(id,exe,observed_generation.unwrap_or(0)+1); }
                        self.publish_invalidation(Some(id));
                    }
                    return;
                }
            }
        } else { None };
        if self.entries.lock().unwrap().get(&id).map(|e|e.generation)!=observed_generation { return; }
        if let Some(expected)=expected {
            self.reconcile_binding(id, &expected, current);
            self.record_source_login(id,&expected,observed_generation.unwrap(),login);
        } else if let Some(current)=current {
            let generation=self.bind(id,&current.key);
            self.record_source_login(id,&current.key,generation,login);
        }
    }
    fn transient_source_error(error: &QueryFailure) -> bool {
        matches!(error.category, QueryError::Timeout | QueryError::Network | QueryError::RateLimited | QueryError::Persistence | QueryError::InvalidData)
    }
    fn source_unavailable(&self, id: ProviderId, expected_generation: Option<u64>, error: QueryFailure) -> QueryResult {
        let time=now(); let mut entries=self.entries.lock().unwrap();
        if entries.get(&id).map(|e|e.generation)!=expected_generation { return QueryResult::empty(QueryStatus::IdentityChanged,time); }
        let e=entries.entry(id).or_default();
        e.verified=false; e.source_login=None; e.source_state=Some(QueryStatus::Failed); e.failures=e.failures.saturating_add(1); e.state_revision += 1;
        e.next_at=e.next_at.max(time+(30i64.saturating_mul(1i64<<e.failures.min(6))).min(1800)).max(error.retry_after.unwrap_or(0));
        let mut r=QueryResult::empty(QueryStatus::Failed,time);r.error=Some(if e.auth_blocked { QueryError::Authentication } else { error.category });r.identity_generation=Some(e.generation);
        r=e.project(r);r.from_cache=r.latest_success.is_some();r.next_retry_at=(!e.auth_blocked).then_some(e.next_at);e.last=Some(r.clone());
        let _=atomic_json(&self.root.join("snapshots-v2.json"),&*entries);r
    }
    fn reconcile_binding(&self, id: ProviderId, expected: &str, current: Option<identity::Identity>) {
        if current.as_ref().is_some_and(|i| i.key == expected) {
            if let Some(e) = self.entries.lock().unwrap().get_mut(&id) {
                if e.identity.as_deref() == Some(expected) { e.verify(); }
            }
        } else {
            let status = if current.is_some() { QueryStatus::IdentityChanged } else { QueryStatus::IdentityUnknown };
            let should_invalidate = self.entries.lock().unwrap().get(&id).is_some_and(|e| e.verified || e.source_state.as_ref() != Some(&status));
            if should_invalidate { self.invalidate_source(Some(id), status); }
        }
    }
    pub fn expected_identity_key(&self, id: ProviderId) -> Option<String> {
        self.entries.lock().ok()?.get(&id)?.identity.clone()
    }
    pub fn utilization(&self, id: ProviderId) -> Option<f64> {
        let entries = self.entries.lock().ok()?;
        let e = entries.get(&id)?;
        if !e.verified || e.auth_blocked {
            return None;
        }
        utilization(e.success.as_ref()?, now())
    }
    /// Observe submitted settings before the asynchronous DB writer can lag behind.
    pub fn update_routing(&self, key: &str, value: &serde_json::Value) -> Result<(), String> {
        if !matches!(key, "settings.providerEnabled" | "settings.providerPaths") {
            return Ok(());
        }
        let mut routing = self
            .routing
            .lock()
            .map_err(|_| "CLI configuration unavailable")?;
        let affected: Vec<ProviderId> = if key == "settings.providerEnabled" {
            let next: HashMap<ProviderId, bool> = serde_json::from_value(value.clone()).map_err(|_| "Invalid enabled CLI configuration")?;
            let previous = routing.0.as_ref().cloned().unwrap_or_default();
            let ids: std::collections::HashSet<_> = previous.keys().chain(next.keys()).copied().collect();
            let affected = ids.into_iter().filter(|id| previous.get(id).copied().unwrap_or(true) != next.get(id).copied().unwrap_or(true)).collect();
            routing.0 = Some(next); affected
        } else {
            let next: HashMap<ProviderId, String> = serde_json::from_value(value.clone()).map_err(|_| "Invalid CLI path configuration")?;
            let previous = routing.1.as_ref().cloned().unwrap_or_default();
            let ids: std::collections::HashSet<_> = previous.keys().chain(next.keys()).copied().collect();
            let affected = ids.into_iter().filter(|id| previous.get(id) != next.get(id)).collect();
            routing.1 = Some(next); affected
        };
        drop(routing);
        for id in affected { self.invalidate(Some(id)); }
        Ok(())
    }
    pub fn enabled(&self, id: ProviderId, fallback: bool) -> bool {
        self.routing.lock().ok().map_or(false, |r| {
            r.0.as_ref()
                .map_or(fallback, |m| m.get(&id).copied().unwrap_or(true))
        })
    }
    pub fn path(&self, id: ProviderId, fallback: Option<String>) -> Option<String> {
        self.routing
            .lock()
            .ok()
            .and_then(|r| r.1.as_ref().map(|m| m.get(&id).cloned()))
            .unwrap_or(fallback)
    }
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<UsageRevisions> {
        self.changed.subscribe()
    }
    pub fn invalidated_result(&self, id: ProviderId, status: QueryStatus) -> QueryResult {
        let mut result = QueryResult::empty(status, now());
        if let Some(e) = self.entries.lock().unwrap().get(&id) {
            result.source_login = e.source_login.clone().filter(|l| e.verified || matches!(l.status, SourceLoginStatus::NeedsLogin));
            result.identity_generation = Some(e.generation);
            result.state_revision = Some(e.state_revision);
            result.latest_success = e.success.clone();
            result.from_cache = result.latest_success.is_some();
        } else { result.identity_generation = Some(0); }
        result
    }
    pub fn revision(&self, id: ProviderId) -> u64 {
        self.changed.borrow().for_provider(id)
    }
    pub fn invalidate(&self, id: Option<ProviderId>) { self.invalidate_source(id, QueryStatus::Unverified); }
    fn invalidate_source(&self, id: Option<ProviderId>, status: QueryStatus) {
        self.invalidate_records(id);
        if let Some(id) = id { if let Some(e) = self.entries.lock().unwrap().get_mut(&id) { e.source_state = Some(status); } }
        self.publish_invalidation(id);
    }
    fn publish_invalidation(&self, id: Option<ProviderId>) {
        self.changed.send_modify(|v| {
            v.serial = v.serial.wrapping_add(1);
            if let Some(id) = id { v.providers.insert(id, v.serial); } else { v.all = v.serial; }
        });
    }
    async fn invalidation(&self, changed: &mut tokio::sync::watch::Receiver<UsageRevisions>, id: ProviderId, revision: u64) {
        loop {
            if changed.borrow_and_update().for_provider(id) != revision { return; }
            if changed.changed().await.is_err() { return; }
        }
    }
    fn invalidate_records(&self, id: Option<ProviderId>) {
        let mut entries = self.entries.lock().unwrap();
        for (p, e) in entries.iter_mut() {
            if id.map_or(true, |id| id == *p) {
                e.generation += 1;
                e.state_revision += 1;
                e.verified = false;
                e.source_login = None;
                e.source_state = Some(QueryStatus::Unverified);
                e.dispatch_acknowledged = false;
                // Same-source Retry-After and auth failures survive rescans/config invalidation.
            }
        }
        let _ = atomic_json(&self.root.join("snapshots-v2.json"), &*entries);
    }
    /// A delayed failed status read must not invalidate a newer account binding.
    fn invalidate_observed(&self, id: ProviderId, generation: Option<u64>) -> bool {
        let mut entries=self.entries.lock().unwrap();
        if entries.get(&id).map(|e|e.generation)!=generation { return false; }
        let e=entries.entry(id).or_default();
        e.generation+=1; e.state_revision+=1; e.verified=false; e.source_login=None;
        e.source_state=Some(QueryStatus::Unverified); e.dispatch_acknowledged=false;
        let _=atomic_json(&self.root.join("snapshots-v2.json"),&*entries); true
    }
    fn bind(&self, id: ProviderId, key: &str) -> u64 {
        let mut entries = self.entries.lock().unwrap();
        let e = entries.entry(id).or_default();
        if e.identity.as_deref() != Some(key) {
            e.generation += 1;
            e.identity = Some(key.into());
            e.success = None;
            e.last = None;
            e.next_at = 0;
            e.failures = 0;
            e.auth_blocked = false;
            e.source_login = None;
        }
        e.verify();
        e.dispatch_acknowledged = false;
        e.generation
    }
    /// Manual and automatic refresh share a single-flight future, result path and backoff.
    pub async fn refresh(self: &Arc<Self>, request: QueryRequest) -> QueryResult {
        if !self.enabled(request.provider, true) {
            return QueryResult::empty(QueryStatus::Disabled, now());
        }
        if self.permission.permit().is_err() {
            return QueryResult::empty(QueryStatus::Disabled, now());
        }
        if request.revision != self.revision(request.provider) {
            return self.invalidated_result(request.provider, QueryStatus::IdentityChanged);
        }
        let provider = request.provider;
        let service = self.clone();
        self.coalesce(provider, request.revision, async move { service.execute(request).await })
            .await
    }
    async fn coalesce(
        self: &Arc<Self>,
        provider: ProviderId,
        revision: u64,
        work: impl std::future::Future<Output = QueryResult> + Send + 'static,
    ) -> QueryResult {
        let key = (provider, revision);
        let future = {
            let mut flights = self.flights.lock().await;
            if let Some(f) = flights.get(&key) {
                f.result.clone()
            } else {
                let serial = self
                    .serial
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let (tx, rx) = tokio::sync::oneshot::channel();
                let result = async move {
                    rx.await
                        .unwrap_or_else(|_| QueryResult::empty(QueryStatus::Cancelled, now()))
                }
                .boxed()
                .shared();
                flights.insert(
                    key,
                    FlightEntry {
                        serial,
                        result: result.clone(),
                    },
                );
                let service = self.clone();
                // Work stays polled even if a window/request disappears; timeout and revoke
                // cleanup therefore still run. The map never owns a suspended CLI future.
                tokio::spawn(async move {
                    let result = std::panic::AssertUnwindSafe(work)
                        .catch_unwind()
                        .await
                        .unwrap_or_else(|_| QueryResult::empty(QueryStatus::Cancelled, now()));
                    let _ = tx.send(result);
                    let mut flights = service.flights.lock().await;
                    if flights.get(&key).is_some_and(|f| f.serial == serial) {
                        flights.remove(&key);
                    }
                });
                result
            }
        };
        future.await
    }

    async fn execute(self: &Arc<Self>, request: QueryRequest) -> QueryResult {
        let time = now();
        let Some(salt) = self.salt else {
            let mut r = QueryResult::empty(QueryStatus::Failed, time);
            r.error = Some(QueryError::Persistence);
            return r;
        };
        let permit = match self.permission.permit() {
            Ok(p) => p,
            Err(_) => return QueryResult::empty(QueryStatus::Disabled, time),
        };
        if matches!(
            capability(request.provider).state,
            "UNSUPPORTED" | "ADAPTER_PENDING" | "IDENTITY_PENDING"
        ) {
            return QueryResult::empty(QueryStatus::Unsupported, time);
        }
        let mut changed = self.changed.subscribe();
        if request.revision != changed.borrow().for_provider(request.provider) {
            return self.invalidated_result(request.provider, QueryStatus::IdentityChanged);
        }
        let work = async {
            let _slot = self
                .slots
                .acquire()
                .await
                .map_err(|_| "Query cancelled".to_string())?;
            let proxy =
                crate::proxy::prepare_turn_proxy(&request.proxy, request.provider, &request.env)
                    .await
                    .map_err(|_| "Query network preparation failed".to_string())?;
            let query_env = ShellEnv {
                vars: crate::chat::child_env::assemble_with_proxy(
                    request.provider,
                    &request.env,
                    &[],
                    Some(&proxy),
                ),
            };
            let lookup_generation=self.entries.lock().unwrap().get(&request.provider).map(|e|e.generation);
            let observation = match identity::observe(request.provider, &request.exe, &query_env, &salt).await {
                Ok(observation) => observation,
                Err(error) if Self::transient_source_error(&error) => return Ok(self.source_unavailable(request.provider,lookup_generation,error)),
                Err(error) => {
                    if !self.invalidate_observed(request.provider, lookup_generation) { return Ok(self.invalidated_result(request.provider,QueryStatus::IdentityChanged)); }
                    if error.category == QueryError::Authentication { self.record_signed_out(request.provider, &request.exe,lookup_generation.unwrap_or(0)+1); }
                    return Ok(self.invalidated_result(request.provider, QueryStatus::IdentityUnknown));
                }
            };
            let identity = observation.identity;
            let previous = self
                .entries
                .lock()
                .unwrap()
                .get(&request.provider)
                .and_then(|e| e.identity.clone());
            if request.automatic && previous.as_ref().is_some_and(|k| k != &identity.key) {
                self.invalidate_records(Some(request.provider));
                let mut r = self.invalidated_result(request.provider, QueryStatus::IdentityChanged);
                r.identity_key = Some(identity.key);
                return Ok(r);
            }
            let generation = self.bind(request.provider, &identity.key);
            self.record_source_login(request.provider, &identity.key, generation, observation.source_login);
            if let Some(r) = self.cached_or_backoff(request.provider, request.automatic, now()) {
                return Ok(r);
            }
            let queried = if request.provider == ProviderId::Opencode {
                opencode::query(&permit, &request.exe, &query_env, &salt, &identity, &proxy).await
            } else if request.provider == ProviderId::Reasonix {
                reasonix::query(&permit, &query_env, &salt, &identity, &proxy).await
            } else if request.provider == ProviderId::Aider {
                aider::query(&permit, &request.exe, &query_env, &salt, &identity, &proxy).await
            } else if request.provider == ProviderId::CodexCli {
                codex::query(&permit, &request.exe, &query_env, &salt, &identity, &proxy).await
            } else if request.provider == ProviderId::KiroCli {
                kiro::query(&permit, &request.exe, &query_env, &proxy).await
            } else if request.provider == ProviderId::CopilotCli {
                copilot::query(&request.exe, &query_env).await
            } else {
                crate::chat::quota::query_with_permit(
                    &permit,
                    request.provider,
                    &request.exe,
                    &request.env,
                    Some(&proxy),
                )
                .await
                .map(bridge_cli)
                .map_err(|e| QueryFailure::cli(&e))
            };
            // Re-capture ordinary shell environment and CLI status; no shared credentials are modified.
            let current_env = crate::env::capture().await;
            let current_env = ShellEnv {
                vars: crate::chat::child_env::assemble_with_proxy(
                    request.provider,
                    &current_env,
                    &[],
                    Some(&proxy),
                ),
            };
            let mut after_login = None;
            let mut signed_out = false;
            let after = match identity::observe(request.provider, &request.exe, &current_env, &salt).await {
                Ok(o) => { after_login=o.source_login; Some(o.identity) },
                Err(error) if Self::transient_source_error(&error) => return Ok(self.source_unavailable(request.provider,Some(generation),error)),
                Err(error) => { signed_out=error.category==QueryError::Authentication; None },
            };
            if after.as_ref() != Some(&identity) {
                if !self.invalidate_observed(request.provider, Some(generation)) { return Ok(self.invalidated_result(request.provider,QueryStatus::IdentityChanged)); }
                if signed_out { self.record_signed_out(request.provider, &request.exe,generation+1); }
                let mut r = self.invalidated_result(
                    request.provider,
                    if after.is_some() {
                        QueryStatus::IdentityChanged
                    } else {
                        QueryStatus::IdentityUnknown
                    },
                );
                r.identity_key = after.map(|i| i.key);
                return Ok(r);
            }
            // A simultaneous status scan may have temporarily failed to read this source.
            // This query's successful post-check is fresh proof for this unchanged generation.
            if let Some(e)=self.entries.lock().unwrap().get_mut(&request.provider) {
                if e.generation==generation && e.identity.as_deref()==Some(&identity.key) { e.verify(); }
            }
            self.record_source_login(request.provider, &identity.key, generation, after_login);
            permit.publish(|| {
                self.finish_current(request.provider, &identity.key, generation, queried)
            })?
        };
        match permit.run(async {
            tokio::select! {
                result=tokio::time::timeout_at(request.deadline.into(),work)=>match result {
                    Ok(r)=>r,
                    Err(_)=>{
                        let binding=self.entries.lock().unwrap().get(&request.provider).and_then(|e|e.identity.clone().map(|k|(k,e.generation)));
                        if let Some((key,generation))=binding {permit.publish(||self.finish_current(request.provider,&key,generation,Err(QueryFailure{category:QueryError::Timeout,retry_after:None})))?}
                        else {let mut r=QueryResult::empty(QueryStatus::Failed,now());r.error=Some(QueryError::Timeout);Ok(r)}
                    }
                },
                _=self.invalidation(&mut changed, request.provider, request.revision)=>Ok(self.invalidated_result(request.provider,QueryStatus::IdentityChanged)),
            }
        }).await {
            Ok(r)=>r,
            Err(_)=>{let mut r=QueryResult::empty(QueryStatus::Cancelled,time);r.error=Some(QueryError::Cancelled);r}
        }
    }
    fn cached_or_backoff(&self, id: ProviderId, automatic: bool, time: i64) -> Option<QueryResult> {
        let entries = self.entries.lock().unwrap();
        let e = entries.get(&id)?;
        if e.auth_blocked || e.next_at > time {
            let mut r = e
                .last
                .clone()
                .unwrap_or_else(|| QueryResult::empty(QueryStatus::Backoff, time));
            r.status = QueryStatus::Backoff;
            r.latest_success = e.success.clone();
            r.identity_key = e.identity.clone();
            r.identity_generation = Some(e.generation);
            r.state_revision = Some(e.state_revision);
            r.from_cache = r.latest_success.is_some();
            r.next_retry_at = (!e.auth_blocked).then_some(e.next_at);
            return Some(e.project(r));
        }
        if automatic && e.last.as_ref().is_some_and(|r| r.status == QueryStatus::Latest) && e.success.as_ref().is_some_and(|s| s.valid_until > time) {
            let mut r = e.last.clone()?;
            r.from_cache = true;
            return Some(e.project(r));
        }
        None
    }
    /// TTL and Retry-After spacing start at observation completion, not before a slow CLI launch.
    fn finish_current(&self, id: ProviderId, key: &str, generation: u64, result: Result<QueryOutput, QueryFailure>) -> Result<QueryResult, String> {
        self.finish(id, key, generation, result, now())
    }
    fn finish(
        &self,
        id: ProviderId,
        key: &str,
        generation: u64,
        queried: Result<QueryOutput, QueryFailure>,
        time: i64,
    ) -> Result<QueryResult, String> {
        let mut entries = self.entries.lock().unwrap();
        let e = entries.entry(id).or_default();
        let previous = e.clone();
        if !e.verified || e.identity.as_deref() != Some(key) || e.generation != generation {
            let mut r = QueryResult::empty(QueryStatus::IdentityChanged, now());
            r.identity_generation = Some(e.generation);
            return Ok(r);
        }
        let mut result = QueryResult::empty(QueryStatus::Latest, time);
        e.state_revision += 1;
        result.state_revision = Some(e.state_revision);
        result.identity_key = Some(key.into());
        result.identity_generation = Some(generation);
        let queried = queried.and_then(|output| {
            let snapshot = normalize(id, key, generation, output, time);
            if validate(&snapshot) {
                Ok(snapshot)
            } else {
                Err(QueryFailure {
                    category: QueryError::InvalidData,
                    retry_after: None,
                })
            }
        });
        match queried {
            Ok(mut snapshot) => {
                snapshot.valid_until = time
                    + if utilization(&snapshot, time).is_some_and(|u| u >= 0.8) {
                        180
                    } else {
                        600
                    };
                e.success = Some(snapshot);
                e.failures = 0;
                e.next_at = time + 15;
                e.auth_blocked = false;
            }
            Err(error) => {
                e.failures = e.failures.saturating_add(1);
                e.auth_blocked = error.category == QueryError::Authentication;
                e.next_at = (time + (30i64.saturating_mul(1i64 << e.failures.min(6))).min(1800))
                    .max(error.retry_after.unwrap_or(0));
                result.status = QueryStatus::Failed;
                result.error = Some(error.category);
            }
        }
        result.latest_success = e.success.clone();
        result.from_cache = result.status != QueryStatus::Latest && result.latest_success.is_some();
        result.next_retry_at = Some(e.next_at);
        result = e.project(result);
        e.last = Some(result.clone());
        if atomic_json(&self.root.join("snapshots-v2.json"), &*entries).is_err() {
            result.status = QueryStatus::Failed;
            result.error = Some(QueryError::Persistence);
            result.latest_success = previous.success.clone();
            result.from_cache = result.latest_success.is_some();
            let mut previous = previous;
            previous.state_revision = result.state_revision.unwrap();
            previous.last = Some(result.clone());
            entries.insert(id, previous);
        }
        Ok(result)
    }
}
fn bridge_cli(output: crate::chat::quota::CliQueryOutput) -> QueryOutput {
    if let Some(scoped) = output.scoped { return scoped; }
    let mut counts = std::collections::HashMap::new();
    let windows = output
        .snapshot
        .as_ref()
        .map(|s| {
            s.windows
                .iter()
                .map(|w| {
                    let occurrence = counts.entry(w.window_minutes).or_insert(0usize);
                    let id = format!("legacy-period-{}-{}", w.window_minutes, *occurrence);
                    *occurrence += 1;
                    QuotaWindow {
                        id,
                        scope: "unclassified".into(),
                        period: "legacy".into(),
                        window_minutes: (w.window_minutes > 0).then_some(w.window_minutes),
                        used_percent: Some(w.used_percent),
                        resets_at: w.resets_at,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let quantities = output
        .balance
        .as_ref()
        .and_then(|s| parse_balance(s))
        .into_iter()
        .collect();
    QueryOutput {
        available: None,
        source: QuerySource::Cli,
        windows,
        quantities,
        display: output.snapshot,
    }
}
fn normalize(
    id: ProviderId,
    key: &str,
    generation: u64,
    output: QueryOutput,
    time: i64,
) -> AccountSnapshot {
    // Legacy windows lack scope evidence; displayable, never trusted for provider-wide soft melt.
    AccountSnapshot {
        available: output.available,
        schema: 2,
        scope: AccountScope {
            provider: id.as_str().into(),
            backend: match id {
                ProviderId::ClaudeCode => "anthropic",
                ProviderId::CodexCli => "openai",
                ProviderId::CursorCli => "cursor",
                ProviderId::CopilotCli => "github",
                ProviderId::AntigravityCli => "google",
                ProviderId::Reasonix => "deepseek",
                ProviderId::Opencode | ProviderId::Aider => "deepseek",
                ProviderId::KiroCli => "kiro",
                _ => "unknown",
            }
            .into(),
            endpoint: matches!(id, ProviderId::Opencode | ProviderId::Reasonix | ProviderId::Aider).then(|| "https://api.deepseek.com".into()),
            account_label: None,
            organization: None,
            workspace: None,
        },
        identity_key: key.into(),
        identity_generation: generation,
        fetched_at: time,
        valid_until: time + 600,
        source: match output.source {
            QuerySource::Cli => "CLI",
            QuerySource::Http => "HTTP",
        }
        .into(),
        windows: output.windows,
        quantities: output.quantities,
        display: output.display,
    }
}
fn parse_balance(s: &str) -> Option<Quantity> {
    let text = s.trim();
    let (currency, amount) = if let Some(s) = text.strip_prefix('¥') {
        ("CNY", s)
    } else if let Some(s) = text.strip_prefix('$') {
        ("USD", s)
    } else if let Some(s) = text.strip_prefix('€') {
        ("EUR", s)
    } else if let Some((amount, unit)) = text
        .rsplit_once(' ')
        .filter(|(_, unit)| unit.len() == 3 && unit.bytes().all(|b| b.is_ascii_uppercase()))
    {
        (unit, amount)
    } else {
        return None;
    };
    let amount = amount.trim();
    let parts: Vec<_> = amount.split('.').collect();
    if parts.len() > 2 {
        return None;
    }
    let groups: Vec<_> = parts[0].split(',').collect();
    if groups.len() > 1
        && (groups[0].is_empty() || groups[0].len() > 3 || groups[1..].iter().any(|g| g.len() != 3))
    {
        return None;
    }
    let amount = amount.replace(',', "");
    decimal(&amount).then(|| Quantity {
        id: format!("wallet-{currency}"),
        unit: "currency".into(),
        currency: Some(currency.into()),
        used: None,
        limit: None,
        remaining: Some(amount),
    })
}

static DISPATCH_GUARD: std::sync::OnceLock<std::sync::Weak<UsageQueryService>> =
    std::sync::OnceLock::new();
pub fn install_dispatch_guard(service: &Arc<UsageQueryService>) {
    let _ = DISPATCH_GUARD.set(Arc::downgrade(service));
}
/// Called before spawning a normal turn. Running turns retain their original environment.
pub async fn guard_dispatch(
    provider: ProviderId,
    exe: &Path,
    env: &ShellEnv,
    proxy: Option<&crate::proxy::PreparedTurnProxy>,
) -> Result<(), String> {
    let Some(service) = DISPATCH_GUARD.get().and_then(|s| s.upgrade()) else {
        return Ok(());
    };
    if !service.enabled(provider, true) {
        return Err("CLI is disabled; dispatch paused".into());
    }
    // A named backend wallet does not attest the source of an arbitrary CLI turn.
    if !contract::binds_dispatch(provider) {
        return Ok(());
    }
    let expected = {
        let entries = service
            .entries
            .lock()
            .map_err(|_| "Usage identity unavailable")?;
        let Some(e) = entries.get(&provider) else {
            return Ok(());
        };

        if e.dispatch_acknowledged {
            return Ok(());
        }
        e.identity.clone()
    };
    let Some(expected) = expected else {
        return Ok(());
    };
    let salt = service.salt.ok_or("Usage identity unavailable")?;
    let fresh = crate::env::capture().await;
    let fresh = ShellEnv {
        vars: crate::chat::child_env::assemble_with_proxy(provider, &fresh, &[], proxy),
    };
    let actual = ShellEnv {
        vars: crate::chat::child_env::assemble_with_proxy(provider, env, &[], proxy),
    };
    let (current, actual) = tokio::join!(
        identity::resolve(provider, exe, &fresh, &salt),
        identity::resolve(provider, exe, &actual, &salt)
    );
    if current.as_ref().map(|i| &i.key) != Some(&expected)
        || actual.as_ref().map(|i| &i.key) != Some(&expected)
    {
        service.invalidate(Some(provider));
        return Err(
            "CLI identity changed or is unknown; check the official CLI login, then rescan and confirm that CLI in Settings before dispatch".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::{UsageSnapshot, UsageSource, UsageWindow};
    fn fixture() -> (tempfile::TempDir, Arc<UsageQueryService>) {
        let dir = tempfile::tempdir().unwrap();
        let permission = Arc::new(OnlineUsagePermission::open(&dir.path().join("consent")));
        let service = Arc::new(UsageQueryService::open(
            &dir.path().join("usage"),
            permission,
        ));
        (dir, service)
    }
    fn output(percent: f64) -> QueryOutput {
        bridge_cli(crate::chat::quota::CliQueryOutput {
            scoped: None,
            snapshot: Some(UsageSnapshot {
                windows: vec![UsageWindow {
                    window_minutes: 300,
                    used_percent: percent,
                    resets_at: None,
                }],
                details: None,
                observed_at: Some("synthetic-source-time".into()),
                source: UsageSource::CliOutput,
            }),
            balance: None,
        })
    }
    #[tokio::test]
    async fn cursor_login_projection_requires_current_status_proof_not_quota_history() {
        let (dir,s)=fixture();let id=ProviderId::CursorCli;let exe="/fixture/cursor";
        let mut provider=crate::providers::detect_one(&ShellEnv{vars:Default::default()},id,Some("/must-not-exist/cursor")).await.unwrap();
        provider.path=Some(exe.into());provider.status=crate::providers::ProviderStatus::UnknownAuth;
        let generation=s.bind(id,"account");s.finish(id,"account",generation,Ok(output(33.0)),now()).unwrap();
        s.apply_source_login(&mut provider);assert_eq!(provider.status,crate::providers::ProviderStatus::UnknownAuth,"quota data never establishes login");
        let proof=SourceLogin{path:exe.into(),observed_at:now(),status:SourceLoginStatus::Ready,auth_method:Some("person@example.invalid".into()),account:Some(crate::usage::Account{billing:crate::usage::Billing::Subscription,official:true,endpoint:None,plan:Some("Pro+".into()),balance:None,usage:None})};
        s.record_source_login(id,"account",generation,Some(proof.clone()));s.apply_source_login(&mut provider);
        assert_eq!(provider.status,crate::providers::ProviderStatus::Ready);assert!(s.inspect(id).source_login.is_some());
        s.finish(id,"account",generation,Ok(output(33.0)),now()).unwrap();
        let restarted=UsageQueryService::open(&dir.path().join("usage"),s.permission.clone());
        assert!(restarted.inspect(id).source_login.is_none(),"persisted status proof is not valid after restart");
        s.invalidate(Some(id));let newer=s.entries.lock().unwrap()[&id].generation;
        s.record_source_login(id,"account",generation,Some(proof.clone()));assert!(s.inspect(id).source_login.is_none());
        assert!(!s.invalidate_observed(id,Some(generation)),"late read cannot invalidate a new generation");
        s.bind(id,"account");s.record_source_login(id,"account",newer,Some(proof));
        s.source_unavailable(id,Some(newer),QueryFailure{category:QueryError::Timeout,retry_after:None});
        let failed=s.inspect(id);assert_eq!(failed.status,QueryStatus::Failed);assert_eq!(failed.error,Some(QueryError::Timeout));assert!(failed.latest_success.is_some());assert!(failed.source_login.is_none());
        assert!(s.invalidate_observed(id,Some(newer)));s.record_signed_out(id,Path::new(exe),newer+1);s.apply_source_login(&mut provider);
        assert_eq!(provider.status,crate::providers::ProviderStatus::NeedsLogin);
        s.bind(id,"different");let current=s.entries.lock().unwrap()[&id].generation;s.record_signed_out(id,Path::new(exe),newer+1);
        assert_eq!(s.entries.lock().unwrap()[&id].generation,current);assert!(s.inspect(id).source_login.is_none());
    }
    #[test]
    fn failure_preserves_success_time_and_never_serializes_error_text() {
        let (_dir, s) = fixture();
        let id = ProviderId::CursorCli;
        let generation = s.bind(id, "opaque");
        let success = s
            .finish(id, "opaque", generation, Ok(output(1.0)), 10)
            .unwrap();
        assert_eq!(success.latest_success.unwrap().fetched_at, 10);
        let failure = s
            .finish(
                id,
                "opaque",
                generation,
                Err(QueryFailure::cli("401 SECRET_SHOULD_NOT_LEAVE_NATIVE")),
                30,
            )
            .unwrap();
        assert_eq!(failure.status, QueryStatus::Failed);
        assert!(failure.from_cache);
        assert_eq!(failure.latest_success.unwrap().fetched_at, 10);
        assert!(s
            .cached_or_backoff(id, false, 10000)
            .unwrap()
            .next_retry_at
            .is_none());
        assert!(!serde_json::to_string(&s.inspect(id))
            .unwrap()
            .contains("SECRET_SHOULD_NOT_LEAVE_NATIVE"));
        let new_generation = s.bind(id, "different-account");
        assert!(new_generation > generation);
        assert!(s.cached_or_backoff(id, false, 10000).is_none());
        assert!(s.inspect(id).latest_success.is_none());
    }
    #[tokio::test]
    async fn invalidation_cancels_only_the_selected_provider_and_keeps_all_event_versions() {
        let (_dir, s) = fixture();
        let a = ProviderId::CursorCli; let b = ProviderId::CopilotCli;
        let mut events = s.subscribe();
        let mut b_events = s.subscribe();
        let b_revision = s.revision(b);
        s.invalidate(Some(a));
        assert!(s.revision(a) > 0); assert_eq!(s.revision(b), b_revision);
        assert!(tokio::time::timeout(std::time::Duration::from_millis(20), s.invalidation(&mut b_events, b, b_revision)).await.is_err());
        s.invalidate(Some(b));
        s.invalidation(&mut b_events, b, b_revision).await;
        let projection = events.borrow_and_update().clone();
        assert!(projection.for_provider(a)>0 && projection.for_provider(b)>0);
        s.invalidate(None);
        assert!(s.revision(a)>projection.for_provider(a)); assert!(s.revision(b)>projection.for_provider(b));
    }
    #[test]
    fn scans_revalidate_same_source_without_erasing_history_or_failed_attempts() {
        let (dir,s)=fixture();let id=ProviderId::CursorCli;
        let generation=s.bind(id,"account");s.finish(id,"account",generation,Ok(output(25.0)),10).unwrap();
        let restarted=UsageQueryService::open(&dir.path().join("usage"),s.permission.clone());
        assert_eq!(restarted.inspect(id).status,QueryStatus::Unverified);
        restarted.reconcile_binding(id,"account",Some(identity::Identity{key:"account".into()}));
        assert_eq!(restarted.inspect(id).status,QueryStatus::Expired);
        assert_eq!(restarted.revision(id),0);
        restarted.reconcile_binding(id,"account",Some(identity::Identity{key:"other".into()}));
        assert_eq!(restarted.inspect(id).status,QueryStatus::IdentityChanged);
        assert!(restarted.inspect(id).latest_success.is_some());
        let version=restarted.revision(id);
        restarted.reconcile_binding(id,"account",Some(identity::Identity{key:"other".into()}));
        assert_eq!(restarted.revision(id),version);
    }
    #[test]
    fn revalidated_cache_has_current_binding_and_state_but_original_fetch_time() {
        let (_dir,s)=fixture(); let id=ProviderId::CursorCli; let time=now();
        let generation=s.bind(id,"account");
        let original=s.finish(id,"account",generation,Ok(output(25.0)),time).unwrap();
        s.reconcile_binding(id,"account",None);
        let unknown=s.inspect(id); assert_eq!(unknown.status,QueryStatus::IdentityUnknown);
        s.reconcile_binding(id,"account",Some(identity::Identity{key:"account".into()}));
        let restored=s.inspect(id);
        assert_eq!(restored.status,QueryStatus::Latest);
        assert_eq!(restored.identity_generation,unknown.identity_generation);
        assert!(restored.state_revision>unknown.state_revision);
        assert_eq!(restored.attempted_at,original.attempted_at);
        for projected in [restored,s.cached_or_backoff(id,true,time+16).unwrap()] {
            assert!(projected.identity_generation>original.identity_generation);
            let snapshot=projected.latest_success.unwrap();
            assert_eq!(snapshot.fetched_at,time); assert_eq!(snapshot.valid_until,time+600);
            assert_eq!(snapshot.identity_generation,generation,"snapshot retains its original query provenance");
        }
        s.reconcile_binding(id,"account",Some(identity::Identity{key:"different".into()}));
        assert_eq!(s.inspect(id).status,QueryStatus::IdentityChanged);
        s.bind(id,"different"); assert!(s.inspect(id).latest_success.is_none());
    }
    #[test]
    fn slow_attempts_receive_a_full_cooldown_and_success_ttl_after_completion() {
        let (_dir,s)=fixture();let id=ProviderId::KiroCli;let generation=s.bind(id,"account");
        let completed=now();
        let r=s.finish_current(id,"account",generation,Err(QueryFailure{category:QueryError::Timeout,retry_after:None})).unwrap();
        assert!(r.attempted_at>=completed);assert!(r.next_retry_at.unwrap()>=completed+60);
        let r=s.finish_current(id,"account",generation,Ok(output(10.0))).unwrap();
        let snapshot=r.latest_success.unwrap();assert!(snapshot.fetched_at>=completed);assert!(snapshot.valid_until>=completed+600);
    }
    #[test]
    fn transient_source_read_failure_is_not_a_changed_account_or_a_cleared_wallet() {
        let (_dir,s)=fixture();let id=ProviderId::Opencode;let b=ProviderId::CopilotCli;
        let generation=s.bind(id,"account");s.finish(id,"account",generation,Ok(output(20.0)),now()).unwrap();
        let other=s.bind(b,"other");s.finish(b,"other",other,Ok(output(30.0)),now()).unwrap();
        let r=s.source_unavailable(id,Some(generation),QueryFailure{category:QueryError::Timeout,retry_after:None});
        assert_eq!(r.status,QueryStatus::Failed);assert_eq!(r.error,Some(QueryError::Timeout));assert!(r.latest_success.is_some());
        assert_eq!(s.expected_identity_key(id).as_deref(),Some("account"));assert_eq!(s.revision(id),0);
        assert_eq!(s.inspect(b).status,QueryStatus::Latest);assert!(s.utilization(id).is_none());
        s.bind(id,"account");assert!(s.cached_or_backoff(id,true,r.next_retry_at.unwrap()+1).is_none(),"failed source reads must be retried after cooldown, not returned as fresh cache");
        let newer=s.bind(id,"new-account");
        let late=s.source_unavailable(id,Some(generation),QueryFailure{category:QueryError::Network,retry_after:None});
        assert_eq!(late.status,QueryStatus::IdentityChanged);assert_eq!(s.expected_identity_key(id).as_deref(),Some("new-account"));
        s.finish(id,"new-account",newer,Err(QueryFailure{category:QueryError::Authentication,retry_after:None}),now()).unwrap();
        let blocked=s.source_unavailable(id,Some(newer),QueryFailure{category:QueryError::Timeout,retry_after:None});
        assert_eq!(blocked.error,Some(QueryError::Authentication));assert!(blocked.next_retry_at.is_none());

    }
    #[test]
    fn unchanged_routing_does_not_invalidate_or_clear_other_provider_backoff() {
        let (_dir, s) = fixture(); let a=ProviderId::CursorCli; let b=ProviderId::CopilotCli;
        let generation=s.bind(b,"account");
        s.finish(b,"account",generation,Err(QueryFailure{category:QueryError::RateLimited,retry_after:Some(10000)}),30).unwrap();
        s.update_routing("settings.providerEnabled",&serde_json::json!({"CURSOR_CLI":false})).unwrap();
        assert_eq!(s.revision(b),0); let version=s.revision(a);
        s.update_routing("settings.providerEnabled",&serde_json::json!({"CURSOR_CLI":false})).unwrap();
        assert_eq!(s.revision(a),version);
        s.invalidate(Some(b));s.bind(b,"account");s.acknowledge_scanned_source(b,true);
        assert_eq!(s.cached_or_backoff(b,false,500).unwrap().next_retry_at,Some(10000));
    }
    #[test]
    fn empty_and_unsupported_sources_are_not_failed_login_checks() {
        let (_dir,s)=fixture();
        assert_eq!(s.inspect(ProviderId::CursorCli).status,QueryStatus::NotRequested);
        assert_eq!(s.inspect(ProviderId::CodexCli).status,QueryStatus::NotRequested);
        assert_eq!(s.inspect(ProviderId::GrokCli).status,QueryStatus::Unsupported);
    }
    #[test]
    fn retry_after_applies_to_manual_and_automatic_and_old_generation_is_rejected() {
        let (_dir, s) = fixture();
        let id = ProviderId::ClaudeCode;
        let generation = s.bind(id, "account");
        s.finish(id, "account", generation, Ok(output(80.0)), 10)
            .unwrap();
        assert_eq!(
            s.cached_or_backoff(id, false, 20).unwrap().status,
            QueryStatus::Backoff
        );
        let failure = QueryFailure {
            category: QueryError::RateLimited,
            retry_after: Some(10000),
        };
        s.finish(id, "account", generation, Err(failure), 30)
            .unwrap();
        for automatic in [false, true] {
            assert_eq!(
                s.cached_or_backoff(id, automatic, 500)
                    .unwrap()
                    .next_retry_at,
                Some(10000)
            );
        }
        s.invalidate(Some(id));
        assert_eq!(
            s.finish(id, "account", generation, Ok(output(0.0)), 31)
                .unwrap()
                .status,
            QueryStatus::IdentityChanged
        );
        assert_eq!(s.inspect(id).status, QueryStatus::Unverified);
        assert!(s.inspect(id).latest_success.is_some());
        assert!(s.utilization(id).is_none());
    }
    #[test]
    fn restart_and_old_unbound_cache_never_become_current() {
        let (dir, s) = fixture();
        let id = ProviderId::CursorCli;
        let generation = s.bind(id, "bound");
        s.finish(id, "bound", generation, Ok(output(100.0)), now())
            .unwrap();
        let reopened = UsageQueryService::open(&dir.path().join("usage"), s.permission.clone());
        assert_eq!(reopened.inspect(id).status, QueryStatus::Unverified);
        assert_eq!(reopened.utilization(id), None);
        assert_eq!(reopened.salt, s.salt);
        std::fs::write(
            dir.path().join("usage/snapshots-v2.json"),
            r#"{"windows":[{"usedPercent":100}]}"#,
        )
        .unwrap();
        let legacy = UsageQueryService::open(&dir.path().join("usage"), s.permission.clone());
        assert!(legacy.inspect(id).latest_success.is_none());
    }
    #[test]
    fn persistence_failure_keeps_prior_success_and_scope_is_required_for_scheduling() {
        let (dir, s) = fixture();
        let id = ProviderId::CursorCli;
        let generation = s.bind(id, "current");
        s.finish(id, "current", generation, Ok(output(1.0)), now())
            .unwrap();
        assert!(
            s.utilization(id).is_none(),
            "legacy windows must not drive provider quota protection"
        );
        {
            let mut entries = s.entries.lock().unwrap();
            entries
                .get_mut(&id)
                .unwrap()
                .success
                .as_mut()
                .unwrap()
                .windows[0]
                .scope = "account".into();
        }
        assert_eq!(s.utilization(id), Some(0.01));
        std::fs::remove_dir_all(dir.path().join("usage")).unwrap();
        std::fs::write(dir.path().join("usage"), "blocked").unwrap();
        let r = s
            .finish(id, "current", generation, Ok(output(100.0)), now() + 1)
            .unwrap();
        assert_eq!(r.error, Some(QueryError::Persistence));
        assert_eq!(r.latest_success.unwrap().windows[0].used_percent, Some(1.0));
    }
    #[tokio::test]
    async fn concurrent_requests_share_one_active_execution_and_survive_caller_drop() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let (_dir, s) = fixture();
        let count = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(tokio::sync::Notify::new());
        let mut callers = Vec::new();
        for _ in 0..8 {
            let service = s.clone();
            let count = count.clone();
            let release = release.clone();
            callers.push(tokio::spawn(async move {
                service
                    .coalesce(ProviderId::CursorCli, 0, async move {
                        count.fetch_add(1, Ordering::SeqCst);
                        release.notified().await;
                        QueryResult::empty(QueryStatus::Latest, 10)
                    })
                    .await
            }));
        }
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while count.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert_eq!(count.load(Ordering::SeqCst), 1);
        callers.remove(0).abort();
        release.notify_waiters();
        for caller in callers {
            assert_eq!(caller.await.unwrap().status, QueryStatus::Latest);
        }
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while !s.flights.lock().await.is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
    #[test]
    fn malformed_percent_and_unknown_balance_do_not_replace_valid_data() {
        let (_dir, s) = fixture();
        let id = ProviderId::CursorCli;
        let generation = s.bind(id, "current");
        s.finish(id, "current", generation, Ok(output(29.0)), 10)
            .unwrap();
        let invalid = s
            .finish(id, "current", generation, Ok(output(f64::NAN)), 20)
            .unwrap();
        assert_eq!(invalid.error, Some(QueryError::InvalidData));
        assert_eq!(invalid.latest_success.unwrap().fetched_at, 10);
        assert_eq!(
            parse_balance("¥ 1,000.0001").unwrap().remaining.as_deref(),
            Some("1000.0001")
        );
        assert_eq!(
            parse_balance("20.50 ABC").unwrap().currency.as_deref(),
            Some("ABC")
        );
        assert_eq!(
            parse_balance("€0.00").unwrap().currency.as_deref(),
            Some("EUR")
        );
        assert!(parse_balance("¥1,2").is_none());
        assert!(parse_balance("29.55").is_none());
    }
    #[tokio::test]
    async fn last_caller_disappearing_does_not_suspend_revoke_cleanup() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let (_dir, s) = fixture();
        s.permission.set(true).unwrap();
        let started = Arc::new(AtomicBool::new(false));
        let ended = Arc::new(AtomicBool::new(false));
        let service = s.clone();
        let begin = started.clone();
        let end = ended.clone();
        let permission = s.permission.clone();
        let caller = tokio::spawn(async move {
            service
                .coalesce(ProviderId::CursorCli, 0, async move {
                    let permit = permission.permit().unwrap();
                    begin.store(true, Ordering::SeqCst);
                    let result = permit
                        .run(std::future::pending::<Result<(), String>>())
                        .await;
                    assert!(result.is_err());
                    end.store(true, Ordering::SeqCst);
                    QueryResult::empty(QueryStatus::Cancelled, now())
                })
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while !started.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        caller.abort();
        s.permission.set(false).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while !ended.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
    #[test]
    fn explicit_ready_scan_confirms_dispatch_without_granting_quota_or_changing_history() {
        let (_dir, s) = fixture();
        let id = ProviderId::CursorCli;
        let generation = s.bind(id, "account-a");
        s.finish(id, "account-a", generation, Ok(output(99.0)), now())
            .unwrap();
        s.invalidate(Some(id));
        s.acknowledge_scanned_source(id, false);
        assert!(
            !s.entries
                .lock()
                .unwrap()
                .get(&id)
                .unwrap()
                .dispatch_acknowledged
        );
        s.acknowledge_scanned_source(id, true);
        assert!(
            s.entries
                .lock()
                .unwrap()
                .get(&id)
                .unwrap()
                .dispatch_acknowledged
        );
        assert_eq!(s.expected_identity_key(id).as_deref(), Some("account-a"));
        assert_eq!(s.inspect(id).status, QueryStatus::Unverified);
        assert!(s.utilization(id).is_none());
        assert_eq!(
            s.inspect(id).latest_success.unwrap().windows[0].used_percent,
            Some(99.0)
        );
    }
    #[test]
    fn http_adapter_output_uses_common_identity_cache_without_losing_currency_precision() {
        let (_dir, s) = fixture();
        let id = ProviderId::Opencode;
        let generation = s.bind(id, "official-deepseek-source");
        let quantities = [("USD", "10000000000000000.0001"), ("CNY", "20.50")]
            .into_iter()
            .map(|(currency, amount)| Quantity {
                id: format!("balance-{currency}"),
                unit: "currency".into(),
                currency: Some(currency.into()),
                used: None,
                limit: None,
                remaining: Some(amount.into()),
            })
            .collect();
        let output = QueryOutput {
            available: Some(false),
            source: QuerySource::Http,
            windows: vec![],
            quantities,
            display: None,
        };
        let result = s
            .finish(id, "official-deepseek-source", generation, Ok(output), 10)
            .unwrap();
        let snapshot = result.latest_success.unwrap();
        assert_eq!(snapshot.source, "HTTP");
        assert_eq!(snapshot.scope.backend, "deepseek");
        assert_eq!(snapshot.available, Some(false));
        assert_eq!(snapshot.scope.endpoint.as_deref(), Some("https://api.deepseek.com"));
        let mut legacy = serde_json::to_value(&snapshot).unwrap();
        legacy.as_object_mut().unwrap().remove("available");
        assert_eq!(serde_json::from_value::<AccountSnapshot>(legacy).unwrap().available, None);
        assert_eq!(
            snapshot.quantities[0].remaining.as_deref(),
            Some("10000000000000000.0001")
        );
        assert_eq!(snapshot.quantities[1].currency.as_deref(), Some("CNY"));
        assert!(
            s.utilization(id).is_none(),
            "a wallet without a denominator is not a percentage"
        );
    }
    #[test]
    fn submitted_configuration_invalidates_before_db_write_and_restored_salt_cannot_mix_installations(
    ) {
        let (dir, s) = fixture();
        let id = ProviderId::CursorCli;
        let revision = s.revision(id);
        s.update_routing(
            "settings.providerEnabled",
            &serde_json::json!({"CURSOR_CLI":false}),
        )
        .unwrap();
        assert!(!s.enabled(id, true));
        assert!(s.revision(id) > revision);
        s.update_routing(
            "settings.providerPaths",
            &serde_json::json!({"CURSOR_CLI":"/new/agent"}),
        )
        .unwrap();
        assert_eq!(
            s.path(id, Some("/old/agent".into())),
            Some("/new/agent".into())
        );
        std::fs::write(dir.path().join("usage/identity-salt"), b"damaged").unwrap();
        assert!(
            UsageQueryService::open(&dir.path().join("usage"), s.permission.clone())
                .salt
                .is_none()
        );
        let (_other_dir, other) = fixture();
        assert_ne!(s.salt, other.salt);
    }
}
