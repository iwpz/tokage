mod chat;
mod env;
mod git;
mod install;
mod knowledge;
mod models;
mod open_in;
mod proc;
mod providers;
mod usage;

use std::collections::HashMap;

use providers::{ProviderId, ProviderInfo};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::RwLock;

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
    let list = providers::detect_each(&env, &custom_paths, |provider| {
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
    use tauri::Manager;
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
    let root = match chat::mirror_dir(&app) {
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

/// Downloads a knowledge pack from a user-supplied URL; the frontend parses and shows it.
#[tauri::command]
async fn fetch_knowledge_pack(state: State<'_, AppState>, url: String) -> Result<String, String> {
    let env = state.env().await;
    knowledge::fetch(&env, url.trim()).await
}

/// Starts one conversation turn; output streams as `chat-event` events keyed by `turn_id`.
#[tauri::command]
async fn start_chat_turn(
    app: AppHandle,
    state: State<'_, AppState>,
    turns: State<'_, chat::Turns>,
    request: chat::ChatRequest,
) -> Result<(), String> {
    if !std::path::Path::new(&request.cwd).is_dir() {
        return Err(format!("Project folder not found: {}", request.cwd));
    }
    let env = state.env().await;
    let providers = state.providers_or_scan().await;
    let id = request.turn_id.clone();
    let task = tokio::spawn({
        let app = app.clone();
        let id = id.clone();
        async move {
            chat::run_turn(app.clone(), request, providers, env).await;
            use tauri::Manager;
            app.state::<chat::Turns>().0.lock().await.remove(&id);
        }
    });
    turns.0.lock().await.insert(id, task.abort_handle());
    Ok(())
}

#[tauri::command]
async fn cancel_chat_turn(turns: State<'_, chat::Turns>, turn_id: String) -> Result<(), String> {
    if let Some(h) = turns.0.lock().await.remove(&turn_id) {
        h.abort();
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .manage(chat::Turns::default())
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
            fetch_knowledge_pack,
            start_chat_turn,
            cancel_chat_turn
        ])
        .run(tauri::generate_context!())
        .expect("error while running tokage");
}
