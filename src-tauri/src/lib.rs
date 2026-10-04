mod knowledge;
mod open_in;
mod policy;

use std::{collections::HashMap, path::PathBuf};

use tauri::{AppHandle, Emitter, Manager, State};
use tokage_core::{Bus, ChatEvent, DomainEvent};
use tokage_native::{
    chat, context, env, git,
    providers::{self, ProviderId, ProviderInfo},
};
use tokage_storage as storage;
use tokio::sync::RwLock;

/// Frontend event carrying one turn's output (`ChatEvent`).
const CHAT_EVENT: &str = "chat-event";

#[derive(Default)]
pub struct AppState {
    shell_env: RwLock<Option<env::ShellEnv>>,
    providers: RwLock<Vec<ProviderInfo>>,
    /// Held for the duration of a scan so concurrent callers wait instead of re-scanning.
    scan: tokio::sync::Mutex<()>,
}

impl AppState {
    async fn env(&self) -> env::ShellEnv {
        if let Some(e) = self.shell_env.read().await.clone() {
            return e;
        }
        let e = env::capture().await;
        *self.shell_env.write().await = Some(e.clone());
        e
    }

    /// Detection results, waiting for an in-flight scan (or running one) if none exist yet.
    async fn providers_or_scan(&self) -> Vec<ProviderInfo> {
        if !self.providers.read().await.is_empty() {
            return self.providers.read().await.clone();
        }
        let _guard = self.scan.lock().await;
        if self.providers.read().await.is_empty() {
            let env = self.env().await;
            *self.providers.write().await = providers::detect_all(&env, &HashMap::new()).await;
        }
        self.providers.read().await.clone()
    }
}

/// One provider's fresh detection result, emitted as soon as it finishes.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderDetected<'a> {
    scan_id: &'a str,
    provider: &'a ProviderInfo,
}

/// Full rescan. Each provider is pushed as a `provider-detected` event when its probe
/// finishes; the complete list is also returned once all are done.
#[tauri::command]
async fn refresh_providers(
    app: AppHandle,
    state: State<'_, AppState>,
    scan_id: String,
    custom_paths: HashMap<ProviderId, String>,
) -> Result<Vec<ProviderInfo>, String> {
    let _guard = state.scan.lock().await;
    // Rescan always re-captures the login shell env so newly installed CLIs
    // and freshly exported API keys are picked up without a manual reload.
    let env = env::capture().await;
    *state.shell_env.write().await = Some(env.clone());
    let bus = app.state::<Bus>();
    let list = providers::detect_each(&env, &custom_paths, |provider| {
        bus.publish(DomainEvent::ProviderDetected {
            provider: provider.id,
            status: provider.status,
        });
        let _ = app.emit(
            "provider-detected",
            ProviderDetected {
                scan_id: &scan_id,
                provider,
            },
        );
    })
    .await;
    *state.providers.write().await = list.clone();
    Ok(list)
}

/// Rescans one CLI only (the per-card refresh button) and patches it into the cached list.
#[tauri::command]
async fn refresh_provider(
    state: State<'_, AppState>,
    id: ProviderId,
    custom_path: Option<String>,
) -> Result<ProviderInfo, String> {
    // Re-capture the shell env so a CLI installed since the last scan is found.
    let env = env::capture().await;
    *state.shell_env.write().await = Some(env.clone());
    let info = providers::detect_one(&env, id, custom_path.as_deref())
        .await
        .ok_or_else(|| format!("Unknown provider: {id:?}"))?;
    if let Some(slot) = state
        .providers
        .write()
        .await
        .iter_mut()
        .find(|p| p.id == id)
    {
        *slot = info.clone();
    }
    Ok(info)
}

#[tauri::command]
async fn reload_shell_env(state: State<'_, AppState>) -> Result<(), String> {
    *state.shell_env.write().await = Some(env::capture().await);
    Ok(())
}

#[tauri::command]
async fn git_info(state: State<'_, AppState>, path: String) -> Result<git::GitInfo, String> {
    let env = state.env().await;
    Ok(git::info(&env, std::path::Path::new(&path)).await)
}

/// Creates a linked worktree on a new `tokage/<name>` branch for one session, under the
/// app's data folder, so its edits stay off the user's checkout.
#[tauri::command]
async fn create_session_worktree(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    name: String,
) -> Result<git::Worktree, String> {
    let base = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let env = state.env().await;
    git::create_worktree(&env, std::path::Path::new(&path), &base, &name).await
}

/// Removes a session's worktree and its `tokage/` branch. Errors with `DIRTY` when it has
/// uncommitted changes and `force` is false.
#[tauri::command]
async fn remove_session_worktree(
    state: State<'_, AppState>,
    root: String,
    branch: String,
    force: bool,
) -> Result<(), String> {
    let env = state.env().await;
    git::remove_worktree(&env, std::path::Path::new(&root), &branch, force).await
}

/// Reads a text file inside a project (or session worktree) for the preview pane, or a
/// copy Tokage keeps of a file a turn wrote outside the project.
#[tauri::command]
async fn read_project_file(app: AppHandle, root: String, path: String) -> Result<git::FileText, String> {
    let root = match mirror_dir(&app) {
        Some(m) if std::path::Path::new(&path).starts_with(&m) => m,
        _ => std::path::PathBuf::from(root),
    };
    tokio::task::spawn_blocking(move || git::read_text(&root, &path))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn list_open_targets(state: State<'_, AppState>) -> Result<Vec<open_in::OpenTarget>, String> {
    let env = state.env().await;
    Ok(open_in::available(&env))
}

#[tauri::command]
async fn open_path_in(
    state: State<'_, AppState>,
    path: String,
    target: open_in::OpenTarget,
) -> Result<(), String> {
    let env = state.env().await;
    open_in::open(&env, std::path::Path::new(&path), target).await
}

#[tauri::command]
async fn open_external_url(state: State<'_, AppState>, url: String) -> Result<(), String> {
    let env = state.env().await;
    open_in::open_url(&env, &url).await
}

const REPO_URL: &str = "https://github.com/iwpz/tokage";

/// Build provenance for Settings → About (docs/11 §7.1). Values are fixed at compile time
/// by build.rs; a modified copy can fake them, which the About page says.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct BuildInfo {
    version: &'static str,
    /// Full commit hash, or None when built outside a git checkout.
    commit: Option<&'static str>,
    /// "official" (release pipeline) or "local".
    kind: &'static str,
    repo_url: &'static str,
    /// Tree URL for this exact source: the release tag, else the commit.
    source_url: String,
}

#[tauri::command]
fn build_info() -> BuildInfo {
    let version = env!("CARGO_PKG_VERSION");
    let commit = Some(env!("TOKAGE_COMMIT")).filter(|c| !c.is_empty());
    let kind = env!("TOKAGE_BUILD_KIND");
    let rev = match (kind, commit) {
        ("official", _) => format!("v{version}"),
        (_, Some(c)) => c.to_string(),
        _ => "master".to_string(),
    };
    BuildInfo {
        version,
        commit,
        kind,
        repo_url: REPO_URL,
        source_url: format!("{REPO_URL}/tree/{rev}"),
    }
}

/// Downloads a knowledge pack from a user-supplied URL; the frontend parses and shows it.
#[tauri::command]
async fn fetch_knowledge_pack(state: State<'_, AppState>, url: String) -> Result<String, String> {
    let env = state.env().await;
    knowledge::fetch(&env, url.trim()).await
}

/// Tokage's folder for copies of files a turn wrote outside its working directory.
fn mirror_dir(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("files"))
}

/// Saves a Plan turn's reply as the session's plan under `<cwd>/.tokage/plans/` (P1.m6b).
#[tauri::command]
async fn save_session_plan(
    cwd: String,
    session_id: String,
    title: String,
    text: String,
) -> Result<context::SavedPlan, String> {
    tokio::task::spawn_blocking(move || context::save_plan(std::path::Path::new(&cwd), &session_id, &title, &text))
        .await
        .map_err(|e| e.to_string())?
}

/// Whether `.tokage/` exists yet in a folder.
#[tauri::command]
fn tokage_dir_exists(cwd: String) -> bool {
    context::exists(std::path::Path::new(&cwd))
}

/// Adds `.tokage/` to the project's `.gitignore`, after the user agreed (P1.m6e).
#[tauri::command]
fn ignore_tokage_dir(path: String) -> Result<(), String> {
    context::add_to_gitignore(std::path::Path::new(&path)).map_err(|e| e.to_string())
}

/// Running turns, so the UI can stop one. Aborting drops the child and its process group.
#[derive(Default)]
struct Turns(tokio::sync::Mutex<HashMap<String, tokio::task::AbortHandle>>);

/// Starts one conversation turn; output streams as `chat-event` events keyed by `turn_id`.
#[tauri::command]
async fn start_chat_turn(
    app: AppHandle,
    state: State<'_, AppState>,
    turns: State<'_, Turns>,
    bus: State<'_, Bus>,
    request: chat::ChatRequest,
) -> Result<(), String> {
    if !std::path::Path::new(&request.cwd).is_dir() {
        return Err(format!("Project folder not found: {}", request.cwd));
    }
    let env = state.env().await;
    let providers = state.providers_or_scan().await;
    let id = request.turn_id.clone();
    let bus = bus.inner().clone();
    let mirror = mirror_dir(&app);
    // Held across spawn so the task can't remove its entry before it is inserted.
    let mut running = turns.0.lock().await;
    let task = tokio::spawn({
        let app = app.clone();
        let id = id.clone();
        async move {
            chat::run_turn(&bus, request, providers, env, mirror).await;
            app.state::<Turns>().0.lock().await.remove(&id);
            bus.publish(DomainEvent::ChatEnded { turn_id: id });
        }
    });
    running.insert(id, task.abort_handle());
    Ok(())
}

#[tauri::command]
async fn cancel_chat_turn(
    turns: State<'_, Turns>,
    bus: State<'_, Bus>,
    turn_id: String,
) -> Result<(), String> {
    if let Some(h) = turns.0.lock().await.remove(&turn_id) {
        h.abort();
        bus.publish(DomainEvent::ChatEnded { turn_id });
    }
    Ok(())
}

/// Forwards bus events the frontend listens to. Runs for the app's lifetime.
fn forward_to_frontend(app: AppHandle, bus: &Bus) {
    let mut rx = bus.subscribe();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let DomainEvent::ChatOutput { turn_id, event } = event {
                let _ = app.emit(CHAT_EVENT, ChatEvent { turn_id, kind: event });
            }
        }
    });
}

/// Everything the stores restore at startup.
#[tauri::command]
async fn storage_load(db: State<'_, storage::StorageState>) -> Result<storage::Snapshot, String> {
    db.get()?.load()
}

/// Queues store changes for the batched writer.
#[tauri::command]
async fn storage_write(db: State<'_, storage::StorageState>, batch: storage::WriteBatch) -> Result<(), String> {
    db.get()?.write(batch)
}

/// One-time import of the localStorage data (P1.m3d). Resolves once committed.
#[tauri::command]
async fn storage_import(db: State<'_, storage::StorageState>, batch: storage::WriteBatch) -> Result<(), String> {
    db.get()?.import(batch).await
}

/// The effective command lists (pack plus custom pack) and local rules (docs/11 §3).
#[tauri::command]
async fn get_command_policy(db: State<'_, storage::StorageState>) -> Result<policy::CommandPolicyOut, String> {
    policy::get(db.get()?)
}

/// Adds, changes or deletes one local rule; returns all rules. Rules that would allow
/// something the pack forbids are refused.
#[tauri::command]
async fn update_command_rules(
    db: State<'_, storage::StorageState>,
    change: policy::RuleChange,
) -> Result<Vec<tokage_core::command_policy::Rule>, String> {
    policy::update(db.get()?, change).await
}

/// Classifies a command line against the lists (rule preview in settings; approvals later).
#[tauri::command]
async fn classify_command(
    app: AppHandle,
    state: State<'_, AppState>,
    db: State<'_, storage::StorageState>,
    request: policy::ClassifyRequest,
) -> Result<tokage_core::command_policy::Verdict, String> {
    let home = state.env().await.vars.get("HOME").cloned();
    let app_data = app.path().app_data_dir().ok().map(|p| p.to_string_lossy().into_owned());
    policy::classify(db.get()?, request, home, app_data)
}

/// What the authorization mode does with a command or edit (settings preview; approvals later).
/// `full_auto` applies only to CLIs that run inside Tokage's OS sandbox.
#[tauri::command]
async fn authorize_command(
    app: AppHandle,
    state: State<'_, AppState>,
    db: State<'_, storage::StorageState>,
    request: policy::AuthorizeRequest,
) -> Result<policy::Authorization, String> {
    let home = state.env().await.vars.get("HOME").cloned();
    let app_data = app.path().app_data_dir().ok().map(|p| p.to_string_lossy().into_owned());
    let sandboxed = request
        .target
        .provider
        .as_deref()
        .and_then(|p| serde_json::from_value::<ProviderId>(serde_json::Value::String(p.into())).ok())
        .and_then(chat::adapter::adapter)
        .is_some_and(|a| a.meta().caps.has(chat::adapter::Caps::OS_SANDBOX));
    policy::authorize(db.get()?, request, sandboxed, home, app_data)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .manage(Turns::default())
        .manage(Bus::new())
        .setup(|app| {
            forward_to_frontend(app.handle().clone(), &app.state::<Bus>());
            let db = app
                .path()
                .app_data_dir()
                .map_err(|e| e.to_string())
                .and_then(|dir| storage::Storage::open(&dir));
            if let Err(e) = &db {
                tracing::error!("database: {e}");
            }
            app.manage(storage::StorageState(db));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            refresh_providers,
            refresh_provider,
            reload_shell_env,
            git_info,
            create_session_worktree,
            remove_session_worktree,
            read_project_file,
            list_open_targets,
            open_path_in,
            open_external_url,
            build_info,
            fetch_knowledge_pack,
            start_chat_turn,
            cancel_chat_turn,
            save_session_plan,
            tokage_dir_exists,
            ignore_tokage_dir,
            storage_load,
            storage_write,
            storage_import,
            get_command_policy,
            update_command_rules,
            classify_command,
            authorize_command
        ])
        .build(tauri::generate_context!())
        .expect("error while building tokage")
        .run(|app, event| {
            // Commit queued writes before the process goes.
            if let tauri::RunEvent::Exit = event {
                if let Ok(db) = app.state::<storage::StorageState>().get() {
                    db.flush_blocking();
                }
            }
        });
}
