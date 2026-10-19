//! Opt-in desktop acceptance. Available only in debug builds with the explicit
//! `live-acceptance` feature, against a marked repository in a temporary root.
use std::{path::PathBuf, sync::OnceLock};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State, Manager};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(default)]
    pub fault: Option<String>,
    #[serde(default)]
    pub recovery: Option<Recovery>,
    #[serde(default)]
    pub scenario: Option<String>,
    #[serde(default)]
    pub request: Option<String>,
    #[serde(default)]
    pub background_probe_seconds: u32,
    pub root: PathBuf,
    pub cwd: PathBuf,
    pub provider: String,
    pub model: String,
    pub tier: Option<String>,
    #[serde(default)]
    pub engine: Option<String>,
    #[serde(default)]
    pub cli_path: Option<PathBuf>,
    #[serde(default)]
    pub data_home: Option<PathBuf>,
    #[serde(default)]
    pub version_prefix: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recovery { pub session_id: String, pub cwd: PathBuf }
#[derive(Default)]
pub struct AcceptanceState(pub Option<Config>);
static CONFIG: OnceLock<Option<Config>> = OnceLock::new();

pub fn configure(context: &mut tauri::Context<tauri::Wry>) -> Result<AcceptanceState, String> {
    let config = if cfg!(all(debug_assertions, feature = "live-acceptance")) {
        match std::env::var("TOKAGE_LIVE_DESKTOP_CONFIG") {
            Ok(path) => {
                let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
                let mut config: Config = serde_json::from_str(&text).map_err(|e| e.to_string())?;
                config.root = config.root.canonicalize().map_err(|e| e.to_string())?;
                config.cwd = config.cwd.canonicalize().map_err(|e| e.to_string())?;
                let allowed_root = [std::env::temp_dir(), PathBuf::from("/tmp")].into_iter()
                    .filter_map(|p| p.canonicalize().ok()).any(|p| config.root.parent() == Some(p.as_path()));
                if !allowed_root || !config.root.file_name().is_some_and(|s| s.to_string_lossy().starts_with("tokage-live-desktop-"))
                    || config.cwd != config.root.join("repo") || !config.cwd.join(".git").is_dir()
                    || std::fs::read_to_string(config.root.join(".tokage-live-acceptance")).ok().as_deref() != Some("Tokage live CLI acceptance\n") {
                    return Err("Desktop live acceptance requires a marked temporary fixture repository".into());
                }
                if config.scenario.as_deref().is_some_and(|s| !matches!(s, "fixed" | "natural"))
                    || (config.scenario.as_deref() == Some("natural") && (config.request.as_ref().is_none_or(|s| s.trim().is_empty()) || !matches!(config.provider.as_str(), "CLAUDE_CODE" | "OPENCODE"))) {
                    return Err("Invalid natural-language acceptance scenario".into());
                }
                if config.fault.as_deref().is_some_and(|s| s != "crash") || (config.fault.is_some() && !matches!(config.provider.as_str(), "CLAUDE_CODE" | "OPENCODE")) { return Err("Invalid fault acceptance target".into()); }
                if let Some(recovery) = &config.recovery {
                    if config.fault.as_deref() != Some("crash") || recovery.session_id.is_empty() || !recovery.cwd.canonicalize().map_err(|e| e.to_string())?.starts_with(&config.root) { return Err("Invalid recovery fixture".into()); }
                }
                if config.background_probe_seconds > 600 { return Err("Background probe exceeds acceptance limit".into()); }
                let fixed = match config.provider.as_str() {
                    "OPENCODE" | "REASONIX" => config.model == "deepseek/deepseek-flash" && config.tier.is_none(),
                    "KIRO_CLI" => config.model == "claude-haiku-4.5" && config.tier.is_none(),
                    "AIDER" => config.model == "deepseek/deepseek-flash" && config.tier.is_none(),
                    "CLAUDE_CODE" => config.model == "haiku" && config.tier.is_none(),
                    "CURSOR_CLI" => config.model == "auto" && config.tier.is_none(),
                    "COPILOT_CLI" => config.model == "auto" && config.tier.as_deref() == Some("efficiency"),
                    "ANTIGRAVITY_CLI" => config.model == "gemini-3.8-flash-low" && config.tier.is_none(),
                    _ => false,
                };
                if config.engine.as_ref().is_some_and(|e| config.provider != "KIRO_CLI" || !matches!(e.as_str(), "v2"|"v3")) { return Err("Invalid live acceptance engine".into()); }
                if let Some(path) = &config.cli_path { if !matches!(config.provider.as_str(), "OPENCODE" | "CLAUDE_CODE") || !path.is_absolute() || !path.is_file() { return Err("Invalid acceptance CLI path".into()); } }
                if let Some(path) = &config.data_home { if config.provider != "OPENCODE" || !path.canonicalize().map_err(|e|e.to_string())?.starts_with(&config.root) { return Err("Acceptance data must stay in the temporary fixture".into()); } }
                if !fixed { return Err("Unsupported live acceptance provider/model; no fallback allowed".into()); }
                context.config_mut().app.app_directories_override = Some(tauri::utils::config::AppDirectoriesOverride::Root(config.root.join("app-data").to_string_lossy().into_owned().into()));
                context.config_mut().identifier = "dev.tokage.live-acceptance".into();
                use sha2::{Digest, Sha256};
                let hash = Sha256::digest(config.root.to_string_lossy().as_bytes());
                let mut id = [0u8; 16]; id.copy_from_slice(&hash[..16]);
                for window in &mut context.config_mut().app.windows {
                    window.data_store_identifier = Some(id);
                    window.incognito = true;
                    window.title = format!("Tokage live acceptance — {}", config.provider);
                }
                Some(config)
            }
            Err(_) => None,
        }
    } else { None };
    let _ = CONFIG.set(config.clone());
    Ok(AcceptanceState(config))
}

/// Use the already signed-in subscription, never an exported test gateway/key.
pub fn filter_subscription_env(env: &mut tokage_native::env::ShellEnv) {
    let Some(config) = CONFIG.get().and_then(Option::as_ref) else { return };
    match config.provider.as_str() {
        "OPENCODE" => { if let Some(path)=&config.data_home { env.vars.insert("XDG_DATA_HOME".into(), path.to_string_lossy().into_owned()); } },
        "KIRO_CLI" => { env.vars.insert("TOKAGE_KIRO_ENGINE".into(), config.engine.clone().unwrap_or_else(|| "v2".into())); },
        "CLAUDE_CODE" => env.vars.retain(|key, _| !key.starts_with("ANTHROPIC_") && ![
            "CLAUDE_CODE_OAUTH_TOKEN", "CLAUDE_CODE_OAUTH_TOKEN_FILE_DESCRIPTOR",
            "CLAUDE_CODE_API_KEY_HELPER_TTL_MS", "CLAUDE_CODE_USE_BEDROCK",
            "CLAUDE_CODE_USE_VERTEX", "CLAUDE_CODE_USE_FOUNDRY",
        ].contains(&key.as_str())),
        "CURSOR_CLI" => { env.vars.remove("CURSOR_API_KEY"); env.vars.remove("CURSOR_API_ENDPOINT"); },
        "COPILOT_CLI" => env.vars.retain(|k, _| !k.starts_with("COPILOT_PROVIDER_") && !["COPILOT_GITHUB_TOKEN", "GH_TOKEN", "GITHUB_TOKEN"].contains(&k.as_str())),
        "ANTIGRAVITY_CLI" => { for key in ["GEMINI_API_KEY", "GOOGLE_API_KEY", "GOOGLE_GEMINI_BASE_URL"] { env.vars.remove(key); } },
        _ => {},
    }
}

#[tauri::command]
pub fn get_live_acceptance_config(state: State<'_, AcceptanceState>) -> Option<Config> { state.0.clone() }

#[tauri::command]
pub fn report_live_acceptance(app: AppHandle, state: State<'_, AcceptanceState>, payload: serde_json::Value) -> Result<(), String> {
    use std::io::Write;
    let config = state.0.as_ref().ok_or("Desktop live acceptance is disabled")?;
    let bytes = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;
    if bytes.len() > 2 * 1024 * 1024 { return Err("Acceptance report too large".into()); }
    let mut log = std::fs::OpenOptions::new().append(true).create(true).open(config.root.join("progress.jsonl")).map_err(|e| e.to_string())?;
    log.write_all(&bytes).and_then(|_| log.write_all(b"\n")).map_err(|e| e.to_string())?;
    match payload.get("step").and_then(serde_json::Value::as_str) {
        Some("background-probe-start") if config.background_probe_seconds > 0 => {
            app.get_webview_window("main").ok_or("Missing acceptance window")?.hide().map_err(|e| e.to_string())?;
        }
        Some("background-probe-passed") if config.background_probe_seconds > 0 => {
            app.get_webview_window("main").ok_or("Missing acceptance window")?.show().map_err(|e| e.to_string())?;
        }
        _ => {}
    }
    let status = payload.get("status").and_then(serde_json::Value::as_str);
    if matches!(status, Some("passed" | "failed")) {
        std::fs::write(config.root.join("result.json"), &bytes).map_err(|e| e.to_string())?;
        app.exit(if status == Some("passed") { 0 } else { 1 });
    }
    Ok(())
}

/// External deterministic review, available only inside the marked acceptance fixture.
#[tauri::command]
pub fn get_live_acceptance_review(state: State<'_, AcceptanceState>) -> Result<Option<serde_json::Value>, String> {
    let config = state.0.as_ref().ok_or("Desktop live acceptance is disabled")?;
    match std::fs::read(config.root.join("independent-review.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}
