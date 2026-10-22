mod dashboard;
mod debug_provider;
mod health_sub;
mod knowledge;
mod open_in;
mod policy;
mod retention_sub;
mod runtime;

use std::{collections::HashMap, path::PathBuf};

use tauri::{AppHandle, Emitter, Manager, State};
use tokage_core::{Bus, ChatEvent, DomainEvent};
use tokage_native::{
    chat, context, env, git, proc,
    providers::{self, ProviderId, ProviderInfo},
    usage,
};
use tokage_planning::digest::{self as planning_digest, DigestCache, RepoDigest};
use tokage_storage as storage;
use tokio::sync::RwLock;

/// Frontend event carrying one turn's output (`ChatEvent`).
const CHAT_EVENT: &str = "chat-event";
/// Frontend event when a turn ends (normal, failed, or cancelled).
const CHAT_ENDED: &str = "chat-ended";

/// F12.b: `equiv_cost_usd = (input·p_in + cache_read·p_in·r + cache_write·p_in·w + output·p_out) / 1e6`.
/// Returns `(cost, is_subscription)` when a matching candidate exists.
fn compute_token_cost_usd(
    pack: &tokage_matching::EffectiveKnowledgePack,
    provider: &str,
    model: &str,
    input: i64,
    cache_read: i64,
    cache_write: i64,
    output: i64,
) -> Option<(f64, bool)> {
    let cand = pack.candidates.iter().find(|c| {
        c.provider.eq_ignore_ascii_case(provider)
            && (c.model_id == model || c.cli_model_arg == model)
    })?;
    let p_in = cand.price_per_mtok.input;
    let p_out = cand.price_per_mtok.output;
    let (r, w) = pack
        .models
        .get(&cand.model_id)
        .and_then(|m| m.cache_rates.as_ref())
        .map(|cr| (cr.read_multiplier, cr.write_multiplier))
        .unwrap_or((1.0, 1.0));
    let cost = (input as f64 * p_in
        + cache_read as f64 * p_in * r
        + cache_write as f64 * p_in * w
        + output as f64 * p_out)
        / 1_000_000.0;
    let is_sub = cand.billing.eq_ignore_ascii_case("SUBSCRIPTION");
    Some((cost, is_sub))
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatEndedPayload {
    turn_id: String,
    cancelled: bool,
}

#[derive(Default)]
pub struct AppState {
    shell_env: RwLock<Option<env::ShellEnv>>,
    providers: RwLock<Vec<ProviderInfo>>,
    proxy_config: RwLock<Option<tokage_core::proxy::GlobalProxyConfig>>,
    digest_cache: DigestCache,
    /// Held for the duration of a scan so concurrent callers wait instead of re-scanning.
    scan: tokio::sync::Mutex<()>,
}

async fn capture_shell_env() -> env::ShellEnv {
    env::capture().await
}

impl AppState {
    async fn env(&self) -> env::ShellEnv {
        if let Some(e) = self.shell_env.read().await.clone() {
            return e;
        }
        let e = capture_shell_env().await;
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

/// Returns simulated catalog entries without calling any vendor CLI.
#[tauri::command]
async fn get_debug_provider_catalog(db: State<'_, storage::StorageState>) -> Result<Vec<serde_json::Value>, String> {
    let conn = db.get()?.read_conn()?;
    Ok(debug_provider::catalog(&dashboard::get_effective_pack(&conn)))
}

#[tauri::command]
async fn prepare_debug_workspace(app: AppHandle, debug: State<'_, debug_provider::DebugState>, path: String) -> Result<String, String> {
    let _guard = debug.workspace_lock.lock().await;
    let base = debug_provider::workspace_base(&app)?;
    tauri::async_runtime::spawn_blocking(move || debug_provider::prepare_workspace(&base, &path)).await.map_err(|e| e.to_string())?
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
    let env = capture_shell_env().await;
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
    let env = capture_shell_env().await;
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

/// Explicit API-key validation; one small Gemini model turn, initiated by the user.
#[tauri::command]
async fn verify_antigravity_api_key(state: State<'_, AppState>) -> Result<providers::ProviderInfo, String> {
    let env = capture_shell_env().await;
    *state.shell_env.write().await = Some(env.clone());
    let providers = state.providers_or_scan().await;
    let exe = providers.iter().find(|p| p.id == ProviderId::AntigravityCli)
        .and_then(|p| p.path.as_deref()).ok_or("Antigravity CLI is not installed")?;
    providers::verify_antigravity_key(std::path::Path::new(exe), &env).await?;
    let info = providers::detect_one(&env, ProviderId::AntigravityCli, Some(exe)).await.ok_or("Provider not found")?;
    if let Some(slot) = state.providers.write().await.iter_mut().find(|p| p.id == ProviderId::AntigravityCli) { *slot = info.clone(); }
    Ok(info)
}

#[tauri::command]
async fn verify_reasonix_api_key(state: State<'_, AppState>) -> Result<providers::ProviderInfo, String> {
    let env = capture_shell_env().await;
    *state.shell_env.write().await = Some(env.clone());
    let providers = state.providers_or_scan().await;
    let exe = providers.iter().find(|p| p.id == ProviderId::Reasonix)
        .and_then(|p| p.path.as_deref()).ok_or("Reasonix CLI is not installed")?;
    providers::verify_reasonix_key(std::path::Path::new(exe), &env).await?;
    let info = providers::detect_one(&env, ProviderId::Reasonix, Some(exe)).await.ok_or("Provider not found")?;
    if let Some(slot) = state.providers.write().await.iter_mut().find(|p| p.id == ProviderId::Reasonix) { *slot = info.clone(); }
    Ok(info)
}

/// Queries official quota with a zero-token CLI command or cached snapshot.
#[tauri::command]
async fn query_provider_quota(
    state: State<'_, AppState>,
    provider: ProviderId,
) -> Result<Option<usage::UsageSnapshot>, String> {
    let env = state.env().await;
    let providers = state.providers_or_scan().await;
    let Some(info) = providers.iter().find(|p| p.id == provider) else {
        return Ok(None);
    };
    let Some(exe_str) = &info.path else {
        return Ok(None);
    };
    let exe = std::path::Path::new(exe_str);

    if provider == ProviderId::AntigravityCli {
        let snap = chat::quota::query_antigravity_quota(exe, &env).await?;
        usage::record_cli_output_usage(provider, snap.clone());
        return Ok(Some(snap));
    }

    if provider == ProviderId::CopilotCli && info.account.as_ref().is_some_and(|a| !a.official) {
        return Ok(None);
    }

    let slash_cmd = match provider {
        ProviderId::ClaudeCode => Some("/usage"),
        ProviderId::CodexCli => Some("/status"),
        ProviderId::AntigravityCli => Some("/usage"),
        ProviderId::CopilotCli => Some("/usage"),
        ProviderId::Reasonix => Some("/status"),
        _ => None,
    };

    if let Some(cmd) = slash_cmd {
        let cwd = std::env::temp_dir();
        match chat::quota::execute_slash_quota(provider, exe, &env, &cwd, cmd).await {
            Ok(snap) => {
                usage::record_cli_output_usage(provider, snap.clone());
                Ok(Some(snap))
            }
            Err(e) => {
                tracing::warn!("Failed to query slash quota for {provider}: {e}");
                Ok(usage::get_cached_usage(provider))
            }
        }
    } else {
        Ok(usage::get_cached_usage(provider))
    }
}

/// Generates or retrieves cached RepoDigest for planning engine (docs/02 §3, P5.m1).
#[tauri::command]
async fn get_repo_digest(
    state: State<'_, AppState>,
    path: String,
    max_tokens: Option<usize>,
) -> Result<RepoDigest, String> {
    let p = std::path::PathBuf::from(path);
    if !p.exists() {
        return Err("Path does not exist".to_string());
    }
    let builder = planning_digest::RepoDigestBuilder::new(&p)
        .with_max_tokens(max_tokens.unwrap_or(planning_digest::DEFAULT_DIGEST_MAX_TOKENS))
        .with_cache(state.digest_cache.clone());
    Ok(builder.build().await)
}

/// Renders complete planning prompt combining system template, digest, user request and format spec (P5.m2a).
#[tauri::command]
async fn render_planning_prompt(
    user_request: String,
    repo_digest: String,
    domains: Option<Vec<String>>,
    output_max_tokens: Option<usize>,
    custom_format_spec: Option<String>,
) -> Result<String, String> {
    let rendered = tokage_planning::render_planning_prompt(tokage_planning::PlanningPromptArgs {
        user_request: &user_request,
        repo_digest: &repo_digest,
        domains: domains.as_deref(),
        output_max_tokens,
        custom_format_spec: custom_format_spec.as_deref(),
    });
    Ok(rendered)
}

/// F16.b: format-repair prompt from `PLAN_FORMAT_SPEC` (single source of truth).
#[tauri::command]
async fn render_plan_repair_prompt(raw_text: String) -> Result<String, String> {
    Ok(tokage_planning::render_plan_repair_prompt(&raw_text))
}

/// Parses raw markdown output from planning model into structured Plan with tolerant repair (P5.m2b).
#[tauri::command]
async fn parse_planning_output(
    markdown: String,
    default_test_commands: Option<Vec<String>>,
    default_domain: Option<String>,
) -> Result<tokage_core::planning::Plan, String> {
    let opts = tokage_planning::PlanParseOptions {
        default_test_commands: default_test_commands.unwrap_or_default(),
        default_domain: default_domain.unwrap_or_else(|| "general".to_string()),
    };
    tokage_planning::parse_plan(&markdown, &opts).map_err(|e| e.to_string())
}

/// Validates structured plan and checks DAG cycles, domain mapping, write scope conflicts (P5.m3a).
#[tauri::command]
async fn validate_plan(
    db: State<'_, storage::StorageState>,
    plan: tokage_core::planning::Plan,
    max_tasks: Option<usize>,
    valid_domains: Option<Vec<String>>,
    allowed_commands: Option<Vec<String>>,
    auto_serialize_conflicts: Option<bool>,
) -> Result<tokage_planning::ValidationResult, String> {
    let mut opts = tokage_planning::PlanValidationOptions::default();
    if let Some(m) = max_tasks {
        opts.max_tasks = m;
    }
    if let Some(d) = valid_domains {
        opts.valid_domains = d;
    }
    if let Some(c) = allowed_commands {
        opts.allowed_commands = c;
    }
    if let Some(a) = auto_serialize_conflicts {
        opts.auto_serialize_conflicts = a;
    }
    // F5.c: load gate_allowlist from effective pack (frontend need not pass it).
    opts.gate_allowlist = db
        .get()
        .ok()
        .and_then(|storage| storage.get_conn().ok())
        .map(|conn| dashboard::get_effective_pack(&conn).gate_allowlist)
        .unwrap_or_else(|| {
            tokage_matching::pack::builtin_knowledge_pack()
                .gate_allowlist
                .clone()
        });
    Ok(tokage_planning::validate_plan(plan, &opts))
}

/// Persists structured plan into `<cwd>/.tokage/plans/<plan_id>/` and updates `.git/info/exclude` (P5.m3b).
#[tauri::command]
async fn persist_plan(
    cwd: String,
    plan_id: String,
    plan: tokage_core::planning::Plan,
) -> Result<tokage_planning::PersistPlanResult, String> {
    let p = std::path::PathBuf::from(cwd);
    tokio::task::spawn_blocking(move || tokage_planning::persist_plan(&p, &plan_id, &plan))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Persists user-edited plan into `<cwd>/.tokage/plans/<plan_id>/user_edited/`.
#[tauri::command]
async fn persist_plan_user_edited(
    cwd: String,
    plan_id: String,
    plan: tokage_core::planning::Plan,
) -> Result<tokage_planning::PersistPlanResult, String> {
    let p = std::path::PathBuf::from(cwd);
    tokio::task::spawn_blocking(move || tokage_planning::persist_plan_user_edited(&p, &plan_id, &plan))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Resets plan by deleting `<cwd>/.tokage/plans/<plan_id>/user_edited/` and returns original plan.
#[tauri::command]
async fn reset_plan_to_default(
    cwd: String,
    plan_id: String,
) -> Result<tokage_core::planning::Plan, String> {
    let p = std::path::PathBuf::from(cwd);
    tokio::task::spawn_blocking(move || tokage_planning::reset_plan_to_default(&p, &plan_id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Loads structured plan from `<cwd>/.tokage/plans/<plan_id>/` (P5.m3b).
#[tauri::command]
async fn load_plan(
    cwd: String,
    plan_id: String,
    version: Option<u32>,
) -> Result<tokage_core::planning::Plan, String> {
    let p = std::path::PathBuf::from(cwd);
    tokio::task::spawn_blocking(move || tokage_planning::load_persisted_plan_version(&p, &plan_id, version))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Persists the execution pipeline state to `<cwd>/.tokage/execution/<sessionId>.json` (m2b).
#[tauri::command]
async fn save_pipeline_state_cmd(
    cwd: String,
    state: tokage_planning::PipelineState,
    expected_revision: Option<u64>,
) -> Result<(), String> {
    let p = std::path::PathBuf::from(cwd);
    tokio::task::spawn_blocking(move || tokage_planning::storage::save_pipeline_state_cas(&p, &state, expected_revision))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Process and native-session evidence is separate from the persisted UI state.
#[tauri::command]
async fn task_recovery_evidence(app: AppHandle, turns: State<'_, chat::TurnRegistry>, db: State<'_, storage::StorageState>, turn_id: String,
    cwd: String, provider: Option<String>, thread_id: Option<String>) -> Result<serde_json::Value, String> {
    let runtime_idle = !turns.has_active() && tokage_native::recovery::runtime_idle(proc::get_run_dir().as_deref()).map_err(|e| e.to_string())?;
    let receipts = app.path().app_data_dir().map_err(|e| e.to_string())?.join("turn-receipts");
    let launch_recorded = tokage_native::recovery::verify_launch(&receipts, &turn_id, &cwd).map_err(|e| e.to_string())?;
    if runtime_idle && launch_recorded { db.get()?.record_recovery_usage_gap(turn_id.clone()).await?; }
    let native = provider.as_deref().and_then(|id| serde_json::from_value::<ProviderId>(serde_json::Value::String(id.to_string())).ok())
        .filter(|id| matches!(id, ProviderId::ClaudeCode | ProviderId::CodexCli | ProviderId::CursorCli | ProviderId::CopilotCli | ProviderId::Reasonix | ProviderId::AntigravityCli | ProviderId::KiroCli))
        .map(|id| tokage_native::session::read_native_sessions(id, Some(std::path::Path::new(&cwd)))
            .iter().any(|session| Some(&session.id) == thread_id.as_ref()));
    Ok(serde_json::json!({ "runtimeIdle": runtime_idle, "launchRecorded": launch_recorded, "nativeSessionPresent": native }))
}

/// Loads the execution pipeline state from `<cwd>/.tokage/execution/<sessionId>.json` (m2b/m2c).
#[tauri::command]
async fn load_pipeline_state_cmd(
    turns: State<'_, chat::TurnRegistry>,
    cwd: String,
    session_id: String,
) -> Result<Option<tokage_planning::PipelineState>, String> {
    if turns.has_active() || !tokage_native::recovery::runtime_idle(proc::get_run_dir().as_deref()).map_err(|e| e.to_string())? {
        return Err("Native processes are active; refusing to rewrite a live pipeline as interrupted".into());
    }
    let p = std::path::PathBuf::from(cwd);
    tokio::task::spawn_blocking(move || tokage_planning::load_pipeline_state(&p, &session_id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Computes the semantic diff between two plans (P5.m6a).
#[tauri::command]
async fn compute_plan_diff(
    current_plan: tokage_core::planning::Plan,
    proposed_plan: tokage_core::planning::Plan,
) -> Result<tokage_planning::PlanDiff, String> {
    Ok(tokage_planning::compute_plan_diff(&current_plan, &proposed_plan))
}

/// Applies a plan diff and returns the new consolidated plan (P5.m6a).
#[tauri::command]
async fn apply_plan_diff(
    current_plan: tokage_core::planning::Plan,
    diff: tokage_planning::PlanDiff,
    proposed_plan: tokage_core::planning::Plan,
    parent_plan_id: Option<String>,
) -> Result<tokage_core::planning::Plan, String> {
    Ok(tokage_planning::apply_plan_diff(&current_plan, &diff, &proposed_plan, parent_plan_id))
}

/// Assembles the prompt input for incremental replanning with budget trimming (docs/02 §9, P5.m6c).
#[tauri::command]
async fn assemble_replan_prompt(
    current_plan: tokage_core::planning::Plan,
    change_description: String,
    task_reports: Vec<tokage_planning::TaskExecutionReport>,
    affected_contract_ids: Option<Vec<String>>,
    domains: Option<Vec<String>>,
    replan_input_max_tokens: Option<usize>,
) -> Result<String, String> {
    let affected_slice = affected_contract_ids.as_deref();
    let domains_slice = domains.as_deref();
    Ok(tokage_planning::assemble_replan_prompt(tokage_planning::ReplanInputArgs {
        current_plan: &current_plan,
        change_description: &change_description,
        task_reports: &task_reports,
        affected_contract_ids: affected_slice,
        domains: domains_slice,
        replan_input_max_tokens,
    }))
}

/// Generates adaptation tasks for completed tasks affected by contract modifications (docs/02 §9, P5.m6b).
#[tauri::command]
async fn generate_adaptation_tasks(
    current_plan: tokage_core::planning::Plan,
    changed_contract_ids: Vec<String>,
    task_reports: Vec<tokage_planning::TaskExecutionReport>,
) -> Result<Vec<tokage_core::planning::TaskItem>, String> {
    Ok(tokage_planning::generate_adaptation_tasks(
        &current_plan,
        &changed_contract_ids,
        &task_reports,
    ))
}

/// Evaluates prompt and project context with zero-token local triage (docs/02 §2, P5.m4).
#[tauri::command]
async fn triage_request(
    state: State<'_, AppState>,
    bus: State<'_, Bus>,
    prompt: String,
    project_path: Option<String>,
) -> Result<tokage_core::planning::TriageResult, String> {
    let digest = if let Some(path) = project_path {
        let p = std::path::PathBuf::from(path);
        if p.exists() {
            let builder = planning_digest::RepoDigestBuilder::new(&p)
                .with_max_tokens(planning_digest::DEFAULT_DIGEST_MAX_TOKENS)
                .with_cache(state.digest_cache.clone());
            Some(builder.build().await)
        } else {
            None
        }
    } else {
        None
    };

    let result = tokage_planning::triage(&prompt, digest.as_ref());
    bus.publish(DomainEvent::TriageDone {
        main_id: None,
        route: result.route,
    });
    Ok(result)
}

/// Allocates optimal models for all tasks in a Plan with cumulative scarcity tracking and circuit breaker awareness (docs/04 §7, docs/06 §6, P8.m2c).
#[tauri::command]
async fn allocate_plan_models(
    db: State<'_, storage::StorageState>,
    plan: tokage_core::planning::Plan,
    manual_overrides: Option<std::collections::HashMap<String, String>>,
    initial_provider_utils: Option<std::collections::HashMap<String, f64>>,
    available_models: Option<Vec<tokage_matching::AvailableModel>>,
) -> Result<tokage_matching::PlanAllocationResult, String> {
    let storage = db.get()?;
    let layers = load_layers(&storage)?;
    let pack = tokage_matching::merge_layers(&layers).map_err(|e| e.to_string())?;
    let learned = build_learned_index(&storage);
    let confidence = settings_confidence_threshold(&storage);

    // MELTED is excluded from allocation; HALF_OPEN stays eligible for a probe slot (F15.d).
    let mut melted_providers = std::collections::HashSet::new();
    if let Ok(health_list) = storage.list_provider_health() {
        for h in health_list {
            if h.status == "MELTED" {
                melted_providers.insert(h.provider_name);
            }
        }
    }
    // Only what can run right now (live catalog, enabled, trusted, not blocked) and not melted
    // is a candidate, on every path including manual overrides.
    let pack = tokage_matching::restrict_pack(&pack, available_models.as_deref(), &melted_providers);

    if let Some(overrides) = manual_overrides {
        tokage_matching::recompute_plan_allocation_with_overrides(
            &plan,
            &pack,
            &learned,
            &overrides,
            initial_provider_utils.as_ref(),
            confidence,
        )
    } else {
        Ok(tokage_matching::allocate_plan_models(
            &plan,
            &pack,
            &learned,
            initial_provider_utils.as_ref(),
            confidence,
        ))
    }
}

/// Assigns a model for a single DIRECT task (docs/04 §8, P6.m5c).
#[tauri::command]
async fn assign_direct_task(
    db: State<'_, storage::StorageState>,
    domain: String,
    difficulty: u8,
    size: Option<tokage_core::planning::TaskSize>,
    initial_provider_utils: Option<std::collections::HashMap<String, f64>>,
    available_models: Option<Vec<tokage_matching::AvailableModel>>,
) -> Result<Option<tokage_matching::TaskAssignment>, String> {
    let storage = db.get()?;
    let layers = load_layers(&storage)?;
    let pack = tokage_matching::merge_layers(&layers).map_err(|e| e.to_string())?;
    let pack = tokage_matching::restrict_pack(&pack, available_models.as_deref(), &Default::default());
    let learned = build_learned_index(&storage);
    let confidence = settings_confidence_threshold(&storage);

    Ok(tokage_matching::assign_direct_task(
        &domain,
        difficulty,
        size,
        &pack,
        &learned,
        initial_provider_utils.as_ref(),
        confidence,
    ))
}

/// F16.d: counterfactual baseline cost for a DIRECT turn vs a reference model.
#[tauri::command]
async fn calculate_direct_counterfactual_cost(
    db: State<'_, storage::StorageState>,
    domain: String,
    difficulty: u8,
    size: Option<tokage_core::planning::TaskSize>,
    reference_model: Option<String>,
) -> Result<f64, String> {
    let storage = db.get()?;
    let layers = load_layers(&storage)?;
    let pack = tokage_matching::merge_layers(&layers).map_err(|e| e.to_string())?;
    let task = tokage_core::planning::TaskItem {
        id: "DIRECT".into(),
        domain,
        title: "Direct dispatch".into(),
        goal: String::new(),
        depends_on: vec![],
        contracts: vec![],
        write_scope: vec![],
        deliverables: vec![],
        acceptance: tokage_core::planning::TaskAcceptance::default(),
        size: size.unwrap_or(tokage_core::planning::TaskSize::M),
        difficulty,
        assigned_model: None,
        model_params: None,
    };
    let ref_id = reference_model.unwrap_or_else(|| {
        tokage_matching::determine_reference_model(&pack, None)
    });
    Ok(tokage_matching::calculate_task_counterfactual_cost(
        &task, &ref_id, &pack,
    ))
}

/// Pick dispatchable / promotable tasks from current pipeline snapshot (docs/03 §3, F9 / D1-A).
/// Provider concurrency limits come from the effective pack's `concurrency_hint` (default 2).
#[tauri::command]
async fn pick_dispatchable_tasks(
    db: State<'_, storage::StorageState>,
    input: tokage_orchestration::PickDispatchInput,
) -> Result<tokage_orchestration::PickDispatchOutput, String> {
    let mut provider_overrides = std::collections::HashMap::new();
    if let Ok(storage) = db.get() {
        if let Ok(conn) = storage.get_conn() {
            let pack = dashboard::get_effective_pack(&conn);
            for (id, def) in &pack.providers {
                if let Some(hint) = def.concurrency_hint {
                    provider_overrides.insert(id.clone(), hint as usize);
                }
            }
        }
        // HALF_OPEN providers are limited to a single probe concurrent (F15.d / 06 §6.3).
        if let Ok(health_list) = storage.list_provider_health() {
            for h in health_list {
                if h.status == "HALF_OPEN" {
                    provider_overrides.insert(h.provider_name, 1);
                }
            }
        }
    }
    let limits = tokage_orchestration::ConcurrencyLimits {
        max_global_concurrent: input.max_global_concurrent.max(1),
        default_provider_concurrent: 2,
        provider_overrides,
    };
    let scheduler = tokage_orchestration::DagScheduler::from_states(
        &input.tasks,
        &input.task_states,
        &input.assignments,
        limits,
        &input.deviation_sources,
    );
    Ok(scheduler.pick())
}

/// Assembles a task brief with fixed execution rules and upstream artifacts (docs/05, P7.m2a).
#[tauri::command]
async fn assemble_task_brief_cmd(
    input: tokage_orchestration::TaskBriefInput,
) -> Result<String, String> {
    let brief = tokage_orchestration::assemble_task_brief(&input);
    // R10 choke point: reject drifted fixed headers before they leave the app.
    tokage_context::validate_task_brief_header(&brief).map_err(|e| e.to_string())?;
    Ok(brief)
}

/// Parses or generates a task report locally (docs/03 §4.2, P7.m2b, P7.m2d).
#[tauri::command]
async fn parse_or_generate_task_report_cmd(
    task: tokage_core::planning::TaskItem,
    raw_output: String,
    model: String,
    attempt_id: String,
    first_pass: bool,
    changed_files_stat: String,
    changed_files_list: Vec<String>,
    gate_results_summary: String,
    cost_usd: Option<f64>,
) -> Result<tokage_orchestration::TaskReport, String> {
    Ok(tokage_orchestration::parse_or_generate_task_report(
        &task,
        &raw_output,
        &model,
        &attempt_id,
        first_pass,
        &changed_files_stat,
        changed_files_list,
        &gate_results_summary,
        cost_usd,
    ))
}

/// Records a task report into SQLite (docs/08 schema.md §4, P7.m2b).
#[tauri::command]
async fn record_task_report(
    db: State<'_, storage::StorageState>,
    report: storage::TaskReportRow,
) -> Result<i64, String> {
    db.get()?.record_task_report(report).await
}

#[tauri::command]
async fn ensure_execution_task(
    db: State<'_, storage::StorageState>, session_id: String, plan_id: i64,
    task: tokage_core::planning::TaskItem,
) -> Result<i64, String> {
    db.get()?.ensure_execution_task(session_id, plan_id, task).await
}

/// Retrieves all task reports for a task (docs/08 schema.md §4, P7.m2b).
#[tauri::command]
async fn get_task_reports(
    db: State<'_, storage::StorageState>,
    session_id: String,
    plan_record_id: i64,
    task_id: String,
) -> Result<Vec<storage::TaskReportRow>, String> {
    db.get()?.get_task_reports(&session_id, plan_record_id, &task_id)
}

/// Executes an automated acceptance gate command locally (docs/03 §5, P7.m3a).
#[tauri::command]
async fn execute_gate_command_cmd(
    state: State<'_, AppState>,
    db: State<'_, storage::StorageState>,
    command: String,
    workdir: String,
    timeout_secs: Option<u64>,
    confirmed: bool,
    env_allow: Option<Vec<String>>,
    run_id: Option<String>,
    debug: State<'_, debug_provider::DebugState>,
    debug_config: Option<chat::debug::DebugConfig>,
) -> Result<tokage_orchestration::GateCommandResult, String> {
    let path = std::path::PathBuf::from(&workdir);
    let allowlist = db
        .get()
        .ok()
        .and_then(|storage| storage.get_conn().ok())
        .map(|conn| dashboard::get_effective_pack(&conn).gate_allowlist)
        .unwrap_or_else(|| {
            tokage_matching::pack::builtin_knowledge_pack()
                .gate_allowlist
                .clone()
        });
    if chat::debug::is_mock_workspace(&path) {
        if let tokage_orchestration::GateCommandPermission::Denied(reason) = tokage_orchestration::check_gate_command_safety(&command, confirmed, &allowlist) {
            return Ok(tokage_orchestration::GateCommandResult { command, passed: false, exit_code: None, duration_ms: 0, output_tail: format!("Security rejection: {reason}"), error_message: Some(reason) });
        }
    }
    if let Some(result) = debug.gate(command.clone(), &workdir, run_id.as_deref(), debug_config) { return Ok(result); }
    let env = state.env().await;
    let env_allow = env_allow.unwrap_or_default();
    Ok(tokage_orchestration::execute_gate_command(
        tokage_orchestration::GateRequest {
            command: &command,
            workdir: &path,
            env: &env,
            timeout_secs,
            confirmed,
            allowlist: &allowlist,
            env_allow: &env_allow,
            run_id: run_id.as_deref(),
        },
    )
    .await)
}

/// Cancels an in-flight gate command by run id (F5.e).
#[tauri::command]
async fn cancel_gate_command_cmd(run_id: String) -> Result<bool, String> {
    tokage_orchestration::gate::cancel_gate_command_and_wait(&run_id).await
}

/// Validates whether changed files fall within declared write_scope (docs/09 §4, P7.m3c).
#[tauri::command]
async fn validate_write_scope_cmd(
    declared_scopes: Vec<String>,
    changed_files: Vec<String>,
) -> Result<tokage_orchestration::ScopeValidationResult, String> {
    Ok(tokage_orchestration::validate_write_scope(
        &declared_scopes,
        &changed_files,
    ))
}

/// Restores selected files from an immutable turn content backup.
#[tauri::command]
async fn revert_out_of_scope_files_cmd(
    state: State<'_, AppState>,
    workdir: String,
    files: Vec<String>,
    turn_id: String,
) -> Result<String, String> {
    let env = state.env().await;
    let path = std::path::PathBuf::from(&workdir);
    tokage_orchestration::revert_out_of_scope_files(&path, &env, &files, &turn_id).await
}

/// Pasteable terminal resume one-liner for takeover (docs/03 §8, F10 / A.15).
/// Returns `null` when thread id is missing or the CLI cannot resume by id (e.g. Aider).
#[tauri::command]
async fn terminal_resume_command(
    provider: ProviderId,
    thread_id: Option<String>,
    cwd: String,
) -> Result<Option<String>, String> {
    if let Some(id) = thread_id.as_deref().filter(|id| id.starts_with("tokage-debug:")) {
        return Ok(chat::debug::terminal_target(id).and_then(|(target, native_id)| {
            chat::adapter::terminal_resume_command_line(target, Some(&native_id), &cwd)
        }));
    }
    Ok(tokage_native::chat::adapter::terminal_resume_command_line(
        provider,
        thread_id.as_deref(),
        &cwd,
    ))
}

/// Computes repair action according to 3-step ladder (docs/03 §8, P7.m4a / F13.c).
#[tauri::command]
async fn decide_repair_action_cmd(
    task: tokage_core::planning::TaskItem,
    current_step: tokage_orchestration::RepairStep,
    current_attempt_id: String,
    current_model: String,
    provider: String,
    evidence: tokage_orchestration::FailureEvidence,
    is_provider_melted: Option<bool>,
) -> Result<(tokage_orchestration::RepairStep, tokage_orchestration::RepairDecision), String> {
    if is_provider_melted.unwrap_or(false) {
        Ok(tokage_orchestration::decide_repair_action_with_melted(
            &task,
            current_step,
            &current_attempt_id,
            &current_model,
            &provider,
            true,
            &evidence,
        ))
    } else {
        Ok(tokage_orchestration::decide_repair_action(
            &task,
            current_step,
            &current_attempt_id,
            &current_model,
            &provider,
            &evidence,
        ))
    }
}

/// Generates implementation fix task from testing defects (docs/03 §7, P7.m4b).
#[tauri::command]
async fn generate_implementation_fix_task_cmd(
    test_task: tokage_core::planning::TaskItem,
    implementation_task: tokage_core::planning::TaskItem,
    defect_description: String,
    test_failure_evidence: String,
) -> Result<tokage_core::planning::TaskItem, String> {
    Ok(tokage_orchestration::generate_implementation_fix_task(
        &test_task,
        &implementation_task,
        &defect_description,
        &test_failure_evidence,
    ))
}

/// Records a gate execution run in database (docs/08 schema.md §4, P7.m3a).
#[tauri::command]
async fn record_gate_run(
    db: State<'_, storage::StorageState>,
    run: storage::GateRunRow,
) -> Result<i64, String> {
    db.get()?.record_gate_run(run).await
}

/// Lists gate execution runs from database.
#[tauri::command]
async fn list_gate_runs(
    db: State<'_, storage::StorageState>,
    task_id: Option<String>,
    session_id: String,
    plan_record_id: i64,
) -> Result<Vec<storage::GateRunRow>, String> {
    db.get()?.list_gate_runs(&session_id, plan_record_id, task_id.as_deref())
}

/// Generates completed delivery summary with optional review task (docs/03 §9, P7.m5a).
#[tauri::command]
async fn generate_delivery_summary_cmd(
    plan: tokage_core::planning::Plan,
    reports: Vec<tokage_orchestration::TaskReport>,
    full_gate_result: Option<tokage_orchestration::GateCommandResult>,
    estimated_cost: f64,
    create_review_task: bool,
    plan_id: Option<String>,
) -> Result<tokage_orchestration::DeliverySummary, String> {
    Ok(tokage_orchestration::generate_delivery_summary(
        &plan,
        &reports,
        full_gate_result,
        estimated_cost,
        create_review_task,
        plan_id.as_deref(),
    ))
}

/// F12.c: write plans/contracts/tasks/edges on plan approval (idempotent).
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecordPlanApprovedInput {
    session_id: String,
    plan: tokage_core::planning::Plan,
    assignments: Vec<tokage_matching::TaskAssignment>,
    est_cost_usd: f64,
}

#[tauri::command]
async fn record_plan_approved(
    db: State<'_, storage::StorageState>,
    input: RecordPlanApprovedInput,
) -> Result<storage::RecordPlanApprovedResult, String> {
    let pairs: Vec<(String, f64)> = input
        .assignments
        .iter()
        .map(|a| (a.task_id.clone(), a.est_cost_usd))
        .collect();
    db.get()?
        .record_plan_approved(input.session_id, input.plan, pairs, input.est_cost_usd)
        .await
}

/// Derives a clean main session title from PlanSpec (docs/07 §6, P7.m6g).
#[tauri::command]
async fn derive_session_title_from_spec_cmd(
    spec: tokage_core::planning::PlanSpec,
) -> Result<String, String> {
    Ok(tokage_orchestration::derive_session_title_from_spec(&spec))
}

/// Records or updates an execution attempt node (docs/07 §2, P7.m6a).
#[tauri::command]
async fn upsert_attempt_cmd(
    db: State<'_, storage::StorageState>,
    att: storage::AttemptRow,
) -> Result<(), String> {
    db.get()?.upsert_attempt(att).await
}

/// Lists attempts for a session or project (docs/07 §2, P7.m6a).
#[tauri::command]
async fn list_attempts_cmd(
    db: State<'_, storage::StorageState>,
    session_id: Option<String>,
    project_id: Option<String>,
) -> Result<Vec<storage::AttemptRow>, String> {
    if let Some(pid) = project_id.filter(|p| !p.is_empty()) {
        db.get()?.list_project_attempts(&pid)
    } else {
        let sid = session_id.unwrap_or_default();
        db.get()?.list_attempts(&sid)
    }
}

/// Build `LearnedIndex` from all learned_stats rows (F14).
fn build_learned_index(storage: &storage::Storage) -> tokage_matching::LearnedIndex {
    let mut idx = tokage_matching::LearnedIndex::empty();
    let Ok(rows) = storage.list_all_learned_stats() else {
        return idx;
    };
    // Pair alpha/beta by key; also collect fix_rate.
    let mut alphas: std::collections::HashMap<(String, String, String, String), (f64, u32)> =
        std::collections::HashMap::new();
    let mut betas: std::collections::HashMap<(String, String, String, String), f64> =
        std::collections::HashMap::new();
    for r in rows {
        let key = (
            r.provider_name.clone(),
            r.model_id.clone(),
            r.domain.clone(),
            r.bucket.clone(),
        );
        match r.metric.as_str() {
            "success_alpha" => {
                alphas.insert(key, (r.value, r.samples));
            }
            "success_beta" => {
                betas.insert(key, r.value);
            }
            "fix_rate" => {
                idx.insert_fix_rate(&r.provider_name, &r.model_id, &r.domain, &r.bucket, r.value);
            }
            _ => {}
        }
    }
    for (key, (alpha, samples)) in alphas {
        let beta = betas.get(&key).copied().unwrap_or(0.0);
        idx.insert_success(&key.0, &key.1, &key.2, &key.3, alpha, beta, samples);
    }
    idx
}

/// Shared layered pack loader (F14.d): builtin + enabled customs + overrides + learned usage profiles.
fn load_layers(storage: &storage::Storage) -> Result<tokage_matching::LayeredPacks, String> {
    let builtin = tokage_matching::builtin_knowledge_pack().clone();
    let conn = storage.read_conn()?;

    let mut customs = Vec::new();
    if let Ok(rows) = tokage_storage::knowledge::list_custom_packs(&conn) {
        for row in rows.into_iter().filter(|r| r.enabled) {
            match tokage_matching::parse_knowledge_pack(&row.pack_json) {
                Ok(pack) => customs.push((row.name, pack)),
                Err(e) => {
                    tracing::warn!(
                        name = %row.name,
                        error = %e,
                        "skipping custom pack that failed to parse"
                    );
                }
            }
        }
    }

    let overrides = tokage_storage::knowledge::list_overrides(&conn)
        .unwrap_or_default()
        .into_iter()
        .map(|r| tokage_matching::ModelOverride {
            provider_name: r.provider_name,
            model_id: r.model_id,
            patch: serde_json::from_str(&r.patch_json).unwrap_or_default(),
        })
        .collect();

    // Learned usage profiles: metrics turns / calls_per_turn / growth_per_turn / output_per_turn
    // keyed by domain → size bucket (S/M/L). Built from learned_stats where bucket is size.
    let mut learned_usage: std::collections::HashMap<
        String,
        std::collections::HashMap<String, tokage_matching::UsageProfileEntry>,
    > = std::collections::HashMap::new();
    if let Ok(stats) = storage.list_all_learned_stats() {
        // Collect partial profiles then assemble
        #[derive(Default)]
        struct Partial {
            turns: Option<f64>,
            calls: Option<f64>,
            growth: Option<f64>,
            output: Option<f64>,
        }
        let mut partials: std::collections::HashMap<(String, String), Partial> =
            std::collections::HashMap::new();
        for s in stats {
            if !matches!(s.bucket.as_str(), "S" | "M" | "L") {
                continue;
            }
            let entry = partials
                .entry((s.domain.clone(), s.bucket.clone()))
                .or_default();
            match s.metric.as_str() {
                "turns" => entry.turns = Some(s.value),
                "calls_per_turn" => entry.calls = Some(s.value),
                "growth_per_turn" => entry.growth = Some(s.value),
                "output_per_turn" => entry.output = Some(s.value),
                _ => {}
            }
        }
        for ((domain, size), p) in partials {
            if let (Some(t), Some(c), Some(g), Some(o)) = (p.turns, p.calls, p.growth, p.output) {
                learned_usage.entry(domain).or_default().insert(
                    size,
                    tokage_matching::UsageProfileEntry {
                        turns: t.round().max(1.0) as u32,
                        calls_per_turn: c.round().max(1.0) as u32,
                        growth_per_turn: g.round().max(500.0) as u64,
                        output_per_turn: o.round().max(200.0) as u64,
                    },
                );
            }
        }
    }

    Ok(tokage_matching::LayeredPacks {
        builtin,
        official: None,
        customs,
        overrides,
        learned_usage_profiles: if learned_usage.is_empty() {
            None
        } else {
            Some(learned_usage)
        },
    })
}

fn settings_confidence_threshold(storage: &storage::Storage) -> Option<f64> {
    let Ok(conn) = storage.read_conn() else {
        return None;
    };
    let raw: String = conn
        .query_row(
            "SELECT value_json FROM settings WHERE key = 'settings.confidenceThreshold'",
            [],
            |r| r.get(0),
        )
        .ok()?;
    serde_json::from_str::<f64>(&raw)
        .ok()
        .or_else(|| serde_json::from_str::<serde_json::Value>(&raw).ok().and_then(|v| v.as_f64()))
}

/// F14.c: derive usage-profile samples from attempt usage_events and EMA-update learned_stats.
async fn learn_usage_from_attempt(
    storage: &storage::Storage,
    outcome: &storage::TaskOutcomeRow,
    attempt_id: &str,
) {
    let Ok(events) = storage.usage_events_by_attempt(attempt_id) else {
        return;
    };
    if events.is_empty() {
        return;
    }

    let turns = events.len().max(1) as u32;
    let total_calls: u64 = events
        .iter()
        .map(|e| e.api_calls.unwrap_or(1).max(1) as u64)
        .sum();
    let calls_per_turn = ((total_calls as f64) / turns as f64).round().max(1.0) as u32;
    let total_output: u64 = events.iter().map(|e| e.output_tokens.max(0) as u64).sum();
    let output_per_turn = ((total_output as f64) / turns as f64).round().max(200.0) as u64;

    let ctxs: Vec<i64> = events
        .iter()
        .filter_map(|e| e.context_tokens)
        .collect();
    let growth_per_turn = if ctxs.len() >= 2 {
        let mut diffs = Vec::new();
        for w in ctxs.windows(2) {
            diffs.push((w[1] - w[0]).max(0) as f64);
        }
        let mean = diffs.iter().sum::<f64>() / diffs.len() as f64;
        mean.round().max(500.0) as u64
    } else {
        15_000
    };

    let size_bucket = outcome.size.to_uppercase();
    let size_bucket = match size_bucket.as_str() {
        "S" | "M" | "L" => size_bucket,
        _ => "M".to_string(),
    };

    let pack = tokage_matching::builtin_knowledge_pack();
    let prior = pack
        .usage_profiles
        .get(&outcome.domain)
        .and_then(|sizes| sizes.get(&size_bucket))
        .cloned()
        .unwrap_or(tokage_matching::UsageProfileEntry {
            turns: 2,
            calls_per_turn: 10,
            growth_per_turn: 15_000,
            output_per_turn: 5_000,
        });

    // samples from any of the four metrics (shared)
    let samples0 = storage
        .get_learned_stat("*", "*", &outcome.domain, &size_bucket, "turns")
        .ok()
        .flatten()
        .map(|s| s.samples)
        .unwrap_or(0);
    let next_samples = samples0 + 1;

    let current = tokage_matching::UsageProfileEntry {
        turns: storage
            .get_learned_stat("*", "*", &outcome.domain, &size_bucket, "turns")
            .ok()
            .flatten()
            .map(|s| s.value.round().max(1.0) as u32)
            .unwrap_or(prior.turns),
        calls_per_turn: storage
            .get_learned_stat("*", "*", &outcome.domain, &size_bucket, "calls_per_turn")
            .ok()
            .flatten()
            .map(|s| s.value.round().max(1.0) as u32)
            .unwrap_or(prior.calls_per_turn),
        growth_per_turn: storage
            .get_learned_stat("*", "*", &outcome.domain, &size_bucket, "growth_per_turn")
            .ok()
            .flatten()
            .map(|s| s.value.round().max(500.0) as u64)
            .unwrap_or(prior.growth_per_turn),
        output_per_turn: storage
            .get_learned_stat("*", "*", &outcome.domain, &size_bucket, "output_per_turn")
            .ok()
            .flatten()
            .map(|s| s.value.round().max(200.0) as u64)
            .unwrap_or(prior.output_per_turn),
    };

    let updated = tokage_matching::update_usage_profile_entry(
        &current,
        turns,
        calls_per_turn,
        growth_per_turn,
        output_per_turn,
        next_samples,
    );

    for (metric, value) in [
        ("turns", updated.turns as f64),
        ("calls_per_turn", updated.calls_per_turn as f64),
        ("growth_per_turn", updated.growth_per_turn as f64),
        ("output_per_turn", updated.output_per_turn as f64),
    ] {
        let _ = storage
            .update_learned_stat(storage::LearnedStatRow {
                provider_name: "*".into(),
                model_id: "*".into(),
                domain: outcome.domain.clone(),
                bucket: size_bucket.clone(),
                metric: metric.into(),
                value,
                samples: next_samples,
                updated_at: None,
            })
            .await;
    }

    // cost_calibration: (provider, model, '*', '*')
    let actual = outcome
        .actual_cost_usd
        .or_else(|| {
            let sum: f64 = events
                .iter()
                .map(|e| e.cost_usd.or(e.equiv_cost_usd).unwrap_or(0.0))
                .sum();
            if sum > 0.0 {
                Some(sum)
            } else {
                None
            }
        });
    if let (Some(actual), Some(est)) = (actual, outcome.est_cost_usd) {
        if est > 0.0 && actual > 0.0 {
            let prev = storage
                .get_learned_stat(
                    &outcome.provider_name,
                    &outcome.model_id,
                    "*",
                    "*",
                    "cost_calibration",
                )
                .ok()
                .flatten()
                .map(|s| (s.value, s.samples))
                .unwrap_or((1.0, 0));
            let next_s = prev.1 + 1;
            let next_cal =
                tokage_matching::update_cost_calibration(prev.0, actual, est, next_s);
            let _ = storage
                .update_learned_stat(storage::LearnedStatRow {
                    provider_name: outcome.provider_name.clone(),
                    model_id: outcome.model_id.clone(),
                    domain: "*".into(),
                    bucket: "*".into(),
                    metric: "cost_calibration".into(),
                    value: next_cal,
                    samples: next_s,
                    updated_at: None,
                })
                .await;
        }
    }
}

/// Records a completed task execution outcome and updates learning accumulators (docs/04 §9, F14.b).
/// Observation weights start at 0; dual rows (provider-level + model-level `*`).
/// Optional `attempt_id` triggers F14.c usage-profile / cost-calibration learning.
#[tauri::command]
async fn record_task_outcome(
    db: State<'_, storage::StorageState>,
    outcome: storage::TaskOutcomeRow,
    attempt_id: Option<String>,
) -> Result<i64, String> {
    let storage = db.get()?;
    if let Some(aid) = attempt_id.as_deref() {
        let conn = storage.read_conn()?;
        let is_debug = conn.query_row("SELECT debug_output IS NOT NULL FROM attempts WHERE id = ?1", [aid], |r| r.get::<_, bool>(0)).unwrap_or(false);
        if is_debug { return Ok(0); }
    }
    let id = storage.record_task_outcome(outcome.clone()).await?;

    if let Some(aid) = attempt_id.as_deref().filter(|s| !s.is_empty()) {
        learn_usage_from_attempt(&storage, &outcome, aid).await;
    }

    if let Some(res) = tokage_matching::TaskOutcomeResult::from_str(&outcome.result) {
        let bucket = format!("d{}", outcome.difficulty);
        let providers = [
            outcome.provider_name.as_str(),
            "*", // model-level aggregate
        ];
        for provider in providers {
            let alpha0 = storage
                .get_learned_stat(
                    provider,
                    &outcome.model_id,
                    &outcome.domain,
                    &bucket,
                    "success_alpha",
                )?
                .map(|s| s.value)
                .unwrap_or(0.0);
            let beta0 = storage
                .get_learned_stat(
                    provider,
                    &outcome.model_id,
                    &outcome.domain,
                    &bucket,
                    "success_beta",
                )?
                .map(|s| s.value)
                .unwrap_or(0.0);
            let samples0 = storage
                .get_learned_stat(
                    provider,
                    &outcome.model_id,
                    &outcome.domain,
                    &bucket,
                    "success_alpha",
                )?
                .map(|s| s.samples)
                .unwrap_or(0);

            let mut acc = tokage_matching::BetaAccumulator {
                alpha: alpha0,
                beta: beta0,
                samples: samples0,
            };
            acc.update(res);

            let _ = storage
                .update_learned_stat(storage::LearnedStatRow {
                    provider_name: provider.to_string(),
                    model_id: outcome.model_id.clone(),
                    domain: outcome.domain.clone(),
                    bucket: bucket.clone(),
                    metric: "success_alpha".into(),
                    value: acc.alpha,
                    samples: acc.samples,
                    updated_at: None,
                })
                .await;
            let _ = storage
                .update_learned_stat(storage::LearnedStatRow {
                    provider_name: provider.to_string(),
                    model_id: outcome.model_id.clone(),
                    domain: outcome.domain.clone(),
                    bucket: bucket.clone(),
                    metric: "success_beta".into(),
                    value: acc.beta,
                    samples: acc.samples,
                    updated_at: None,
                })
                .await;

            // fix_rate q: FIXED_IN_WINDOW → toward 1; REASSIGNED → toward 0
            let fix_sample = match res {
                tokage_matching::TaskOutcomeResult::FixedInWindow => Some(1.0),
                tokage_matching::TaskOutcomeResult::Reassigned => Some(0.0),
                _ => None,
            };
            if let Some(sample) = fix_sample {
                let prev = storage
                    .get_learned_stat(
                        provider,
                        &outcome.model_id,
                        &outcome.domain,
                        &bucket,
                        "fix_rate",
                    )?
                    .map(|s| (s.value, s.samples))
                    .unwrap_or((0.5, 0));
                let next_samples = prev.1 + 1;
                let next_q =
                    tokage_matching::update_metric_ema(prev.0, sample, next_samples).clamp(0.0, 1.0);
                let _ = storage
                    .update_learned_stat(storage::LearnedStatRow {
                        provider_name: provider.to_string(),
                        model_id: outcome.model_id.clone(),
                        domain: outcome.domain.clone(),
                        bucket: bucket.clone(),
                        metric: "fix_rate".into(),
                        value: next_q,
                        samples: next_samples,
                        updated_at: None,
                    })
                    .await;
            }
        }
    }

    Ok(id)
}

/// Lists recorded task outcomes for dashboard and metrics (P6.m6).
#[tauri::command]
async fn list_task_outcomes(
    db: State<'_, storage::StorageState>,
    model_id: Option<String>,
    domain: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<storage::TaskOutcomeRow>, String> {
    db.get()?
        .list_task_outcomes(model_id.as_deref(), domain.as_deref(), limit.unwrap_or(50))
}

/// Lists learned model statistics across domains (P6.m6).
#[tauri::command]
async fn list_learned_stats(
    db: State<'_, storage::StorageState>,
    model_id: String,
) -> Result<Vec<storage::LearnedStatRow>, String> {
    db.get()?.list_learned_stats(&model_id)
}

/// F14.f: upsert a user model override.
#[tauri::command]
async fn upsert_model_override(
    db: State<'_, storage::StorageState>,
    provider_name: String,
    model_id: String,
    patch_json: String,
) -> Result<(), String> {
    db.get()?
        .upsert_model_override(provider_name, model_id, patch_json)
        .await
}

/// F14.f: list all model overrides.
#[tauri::command]
async fn list_model_overrides(
    db: State<'_, storage::StorageState>,
) -> Result<Vec<storage::ModelOverrideRow>, String> {
    db.get()?.list_model_overrides()
}

/// F14.f: delete a model override.
#[tauri::command]
async fn delete_model_override(
    db: State<'_, storage::StorageState>,
    provider_name: String,
    model_id: String,
) -> Result<(), String> {
    db.get()?
        .delete_model_override(provider_name, model_id)
        .await
}

/// F14.f: replace all custom packs (ordered).
#[tauri::command]
async fn replace_custom_packs_cmd(
    db: State<'_, storage::StorageState>,
    packs: Vec<serde_json::Value>,
) -> Result<(), String> {
    let storage = db.get()?;
    let parsed: Vec<(String, Option<String>, String, String, bool)> = packs
        .into_iter()
        .map(|v| {
            let name = v
                .get("name")
                .and_then(|x| x.as_str())
                .unwrap_or("custom")
                .to_string();
            let url = v
                .get("url")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string());
            let pack_json = v
                .get("pack")
                .map(|p| p.to_string())
                .unwrap_or_else(|| "{}".into());
            let warnings = v
                .get("warnings")
                .cloned()
                .unwrap_or_else(|| serde_json::json!([]))
                .to_string();
            let enabled = v
                .get("enabled")
                .and_then(|x| x.as_bool())
                .unwrap_or(true);
            (name, url, pack_json, warnings, enabled)
        })
        .collect();
    storage.replace_custom_packs(parsed).await
}

/// F14.f: list custom packs from DB.
#[tauri::command]
async fn list_custom_packs_cmd(
    db: State<'_, storage::StorageState>,
) -> Result<Vec<serde_json::Value>, String> {
    let storage = db.get()?;
    let conn = storage.read_conn()?;
    let rows = tokage_storage::knowledge::list_custom_packs(&conn).map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "name": r.name,
                "pack": serde_json::from_str::<serde_json::Value>(&r.pack_json).unwrap_or_default(),
                "position": r.position,
                "enabled": r.enabled,
            })
        })
        .collect())
}



#[tauri::command]
async fn reload_shell_env(state: State<'_, AppState>) -> Result<(), String> {
    *state.shell_env.write().await = Some(capture_shell_env().await);
    Ok(())
}

#[tauri::command]
async fn git_info(state: State<'_, AppState>, path: String) -> Result<git::GitInfo, String> {
    let env = state.env().await;
    Ok(git::info(&env, std::path::Path::new(&path)).await)
}

#[tauri::command]
async fn git_list_branches(
    state: State<'_, AppState>,
    path: String,
) -> Result<Vec<git::GitBranchInfo>, String> {
    let env = state.env().await;
    git::list_local_branches(&env, std::path::Path::new(&path)).await
}

#[tauri::command]
async fn git_checkout_branch(
    state: State<'_, AppState>,
    path: String,
    branch: String,
) -> Result<(), String> {
    let env = state.env().await;
    git::checkout_branch(&env, std::path::Path::new(&path), &branch).await
}

#[tauri::command]
async fn git_create_branch(
    state: State<'_, AppState>,
    path: String,
    branch: String,
) -> Result<(), String> {
    let env = state.env().await;
    git::create_branch(&env, std::path::Path::new(&path), &branch).await
}

#[tauri::command]
async fn git_status_files(
    state: State<'_, AppState>,
    path: String,
) -> Result<Vec<git::GitStatusFile>, String> {
    let env = state.env().await;
    git::status_files(&env, std::path::Path::new(&path)).await
}

#[tauri::command]
async fn git_file_diff(
    state: State<'_, AppState>,
    path: String,
    file: String,
) -> Result<git::GitFileDiff, String> {
    let env = state.env().await;
    git::file_diff(&env, std::path::Path::new(&path), &file).await
}

/// Creates a linked worktree on a new `tokage/<name>` branch under the app data folder.
/// `base_ref` defaults to HEAD (F11: pass the integration branch for task worktrees).
#[tauri::command]
async fn create_session_worktree(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    name: String,
    base_ref: Option<String>,
) -> Result<git::Worktree, String> {
    let base = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let env = state.env().await;
    git::create_worktree(
        &env,
        std::path::Path::new(&path),
        &base,
        &name,
        base_ref.as_deref(),
    )
    .await
}

/// Removes a session's worktree. Errors with `DIRTY` when it has uncommitted changes and
/// `force` is false. `keep_branch` retains the `tokage/` branch (F11 task cleanup).
#[tauri::command]
async fn remove_session_worktree(
    state: State<'_, AppState>,
    root: String,
    branch: String,
    force: bool,
    keep_branch: Option<bool>,
) -> Result<(), String> {
    let env = state.env().await;
    git::remove_worktree(
        &env,
        std::path::Path::new(&root),
        &branch,
        force,
        keep_branch.unwrap_or(false),
    )
    .await
}

/// Stage + commit in a task worktree under the original author's identity (F11). Returns false if nothing to commit.
#[tauri::command]
async fn commit_task_worktree(
    expected_scope: git::scope::ScopeSnapshot,
    state: State<'_, AppState>,
    root: String,
    message: String,
    co_author: Option<bool>,
) -> Result<bool, String> {
    let env = state.env().await;
    git::scope::commit_verified_with_options(
        &env,
        std::path::Path::new(&root),
        &message,
        &expected_scope,
        co_author.unwrap_or(false),
    )
    .await
}

/// Merge a task branch into the integration worktree (F11). Refuses paths outside app worktrees/.
#[tauri::command]
async fn merge_task_worktree_cmd(
    app: AppHandle,
    state: State<'_, AppState>,
    integration_root: String,
    integration_branch: String,
    task_branch: String,
    expected_commit: String,
    task: tokage_core::planning::TaskItem,
    co_author: Option<bool>,
) -> Result<tokage_orchestration::WorktreeMergeOutcome, String> {
    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let integ = std::path::PathBuf::from(&integration_root);
    if !git::is_under_worktrees_dir(&integ, &app_data) {
        return Err(
            "merge_task_worktree_cmd refuses paths outside the app worktrees directory".into(),
        );
    }
    let env = state.env().await;
    tokage_orchestration::merge_task_worktree_with_options(
        &env,
        &integ,
        &integration_branch,
        &task_branch,
        &task,
        Some(&expected_commit),
        co_author.unwrap_or(false),
    )
    .await
}

/// Prepare an integration-task worktree: create from integration branch, then
/// A confirmed content conflict becomes a private marker baseline; other failures propagate.
#[tauri::command]
async fn prepare_integration_worktree(
    app: AppHandle,
    state: State<'_, AppState>,
    project_path: String,
    name: String,
    integration_branch: String,
    task_branch: String,
    expected_commit: String,
    expected_conflicts: Vec<String>,
    existing_worktree: Option<git::Worktree>,
) -> Result<git::Worktree, String> {
    let base = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let env = state.env().await;
    let wt = if let Some(wt) = existing_worktree {
        if !git::is_under_worktrees_dir(std::path::Path::new(&wt.root), &base) {
            return Err("Conflict worktree is outside the managed worktree directory".into());
        }
        let branch = git::run(
            &env, std::path::Path::new(&wt.root),
            &["symbolic-ref", "--short", "HEAD"], &[], 10,
        ).await?;
        if branch.trim() != wt.branch {
            return Err("Conflict worktree branch mismatch".into());
        }
        wt
    } else {
        git::create_worktree(
            &env, std::path::Path::new(&project_path), &base, &name, Some(&integration_branch),
        ).await?
    };
    tokage_orchestration::prepare_conflict_worktree(
        &env, std::path::Path::new(&wt.root), &task_branch,
        Some(&expected_commit), &expected_conflicts,
    )
    .await.map_err(|e| format!("{e}; retained worktree: {} ({})", wt.root, wt.branch))?;
    Ok(wt)
}

/// Remove a task worktree; `keep_branch` defaults to true for F11 task cleanup.
#[tauri::command]
async fn remove_task_worktree(
    state: State<'_, AppState>,
    root: String,
    branch: String,
    keep_branch: Option<bool>,
    force: Option<bool>,
    expected_commit: Option<String>,
) -> Result<(), String> {
    let env = state.env().await;
    git::remove_worktree_if_head(
        &env,
        std::path::Path::new(&root),
        &branch,
        force.unwrap_or(false),
        keep_branch.unwrap_or(true),
        expected_commit.as_deref(),
    )
    .await
}

#[tauri::command]
async fn git_checkout_identity(state: State<'_, AppState>, path: String) -> Result<git::CheckoutIdentity, String> {
    git::checkout_identity(&state.env().await, std::path::Path::new(&path)).await
}

/// Resolve HEAD (or any rev) in a worktree to a full SHA (F11 baseCommit).
#[tauri::command]
async fn git_rev_parse(
    state: State<'_, AppState>,
    path: String,
    rev: Option<String>,
) -> Result<String, String> {
    let env = state.env().await;
    git::rev_parse(
        &env,
        std::path::Path::new(&path),
        rev.as_deref().unwrap_or("HEAD"),
    )
    .await
}

#[tauri::command]
async fn git_capture_rollback(state: State<'_, AppState>, workdir: String, turn_id: String, expected: git::scope::ScopeSnapshot) -> Result<(), String> {
    git::rollback::capture(&state.env().await, std::path::Path::new(&workdir), &turn_id, &expected).await
}

#[tauri::command]
async fn git_rollback_changes(state: State<'_, AppState>, workdir: String, turn_id: String) -> Result<Vec<String>, String> {
    git::rollback::changes(&state.env().await, std::path::Path::new(&workdir), &turn_id).await
}

#[tauri::command]
async fn git_scope_snapshot(state: State<'_, AppState>, workdir: String) -> Result<git::scope::ScopeSnapshot, String> {
    git::scope::snapshot(&state.env().await, std::path::Path::new(&workdir)).await
}

/// F11.f: merge integration branch into the user's main checkout (DIRTY / CONFLICT codes).
#[tauri::command]
async fn merge_integration_to_checkout(
    state: State<'_, AppState>,
    project_path: String,
    branch: String,
    expected: git::scope::ScopeSnapshot,
    target: Option<git::CheckoutIdentity>,
    co_author: Option<bool>,
) -> Result<(), String> {
    let env = state.env().await;
    git::merge_accepted_into_checkout_with_options(
        &env,
        std::path::Path::new(&project_path),
        &branch,
        &expected,
        target.as_ref(),
        co_author.unwrap_or(false),
    )
    .await
}

/// F11.f: write `git diff <fromRef>..<toRef>` to `outPath`.
#[tauri::command]
async fn git_contains_commit(state: State<'_, AppState>, path: String, branch: String, source: String) -> Result<bool, String> {
    git::contains_commit(&state.env().await, std::path::Path::new(&path), &branch, &source).await
}

#[tauri::command]
async fn reconcile_integration_delivery(state: State<'_, AppState>, path: String, source: String, target: git::CheckoutIdentity) -> Result<bool, String> {
    git::reconcile_delivery_merge(&state.env().await, std::path::Path::new(&path), &source, &target).await
}

#[tauri::command]
async fn export_integration_patch(
    state: State<'_, AppState>,
    project_path: String,
    from_ref: String,
    to_ref: String,
    out_path: String,
    allow_identical: Option<bool>,
) -> Result<(), String> {
    let env = state.env().await;
    git::export_patch_file_retry(
        &env,
        std::path::Path::new(&project_path),
        &from_ref,
        &to_ref,
        std::path::Path::new(&out_path),
        allow_identical.unwrap_or(false),
    )
    .await
}

/// F11.g: `git worktree prune` for a project checkout.
#[tauri::command]
async fn prune_project_worktrees(
    state: State<'_, AppState>,
    project_path: String,
) -> Result<(), String> {
    let env = state.env().await;
    git::prune_worktrees(&env, std::path::Path::new(&project_path)).await
}

/// F11.g: list directories under `{app_data}/worktrees/` that look like checkouts.
#[tauri::command]
async fn list_orphan_worktrees(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<git::OrphanWorktreeEntry>, String> {
    let env = state.env().await;
    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    git::list_orphan_worktrees(&env, &app_data).await
}

/// F11.g: on startup, prune stale worktree registrations for known main checkouts.
#[tauri::command]
async fn reconcile_worktrees_on_startup(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<usize, String> {
    let env = state.env().await;
    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    git::prune_registered_worktrees(&env, &app_data).await
}

/// F11.g: force-remove an orphan Tokage worktree (must be under app worktrees/).
#[tauri::command]
async fn remove_orphan_worktree(
    app: AppHandle,
    state: State<'_, AppState>,
    root: String,
    branch: Option<String>,
) -> Result<(), String> {
    let env = state.env().await;
    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let path = std::path::PathBuf::from(&root);
    if !git::is_under_worktrees_dir(&path, &app_data) {
        return Err("remove_orphan_worktree refuses paths outside the app worktrees directory".into());
    }
    let br = match branch {
        Some(b) if !b.is_empty() => b,
        _ => {
            git::run(
                &env,
                &path,
                &["rev-parse", "--abbrev-ref", "HEAD"],
                &[],
                10,
            )
            .await
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
        }
    };
    if br.starts_with("tokage/") {
        git::remove_worktree(&env, &path, &br, true, false).await
    } else {
        // Unknown / detached: prune after best-effort remove.
        let _ = git::run(
            &env,
            &path,
            &["worktree", "remove", "--force", &root],
            &[],
            60,
        )
        .await;
        if path.exists() {
            tokio::fs::remove_dir_all(&path)
                .await
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
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

/// Build provenance for Settings → About (docs/11 §7.1, P8.m6e). Values are fixed at compile time
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
    /// SHA-256 release digest or reference (P8.m6e).
    release_sha256: &'static str,
    /// macOS notarization Apple Developer Team ID (P8.m6e).
    macos_team_id: &'static str,
    /// Windows Authenticode certificate subject (P8.m6e).
    windows_signer: &'static str,
    /// Whether the official knowledge-pack update channel is enabled (F8 / D6).
    knowledge_update_enabled: bool,
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
        release_sha256: "https://github.com/iwpz/tokage/releases/latest/download/SHA256SUMS",
        macos_team_id: "Developer ID Application: Tokage OpenSource Project (TEAM_ID_TOKAGE_PROD)",
        windows_signer: "CN=Tokage OpenSource Project, O=Tokage Team, L=Tokyo, C=JP",
        knowledge_update_enabled: knowledge::OFFICIAL_MINISIGN_PUBKEY.is_some(),
    }
}

/// Downloads a knowledge pack from a user-supplied URL; the frontend parses and shows it.
#[tauri::command]
async fn fetch_knowledge_pack(state: State<'_, AppState>, url: String) -> Result<String, String> {
    let env = state.env().await;
    let proxy_cfg = state.proxy_config.read().await.clone();
    knowledge::fetch(&env, url.trim(), proxy_cfg.as_ref()).await
}

/// Checks the official release server for an updated knowledge pack, verifies minisign signature & checksum,
/// and stages to `pending.json` if available (docs/04 §10, P8.m4a, P8.m4b).
/// `metadata_url` is required; there is no default remote (F8 / D6).
#[tauri::command]
async fn check_knowledge_pack_update(
    app: AppHandle,
    state: State<'_, AppState>,
    metadata_url: Option<String>,
    current_pack_version: String,
) -> Result<Option<knowledge::StagedUpdateInfo>, String> {
    if knowledge::OFFICIAL_MINISIGN_PUBKEY.is_none() {
        return Err(knowledge::ERR_UPDATE_KEY_UNSET.to_string());
    }
    let meta_url = metadata_url
        .filter(|u| !u.trim().is_empty())
        .ok_or_else(|| "metadata_url is required".to_string())?;

    let env = state.env().await;
    let proxy_cfg = state.proxy_config.read().await.clone();
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app_data_dir: {e}"))?;

    let res = knowledge::check_and_stage_update(
        &env,
        &meta_url,
        &current_pack_version,
        &app_data_dir,
        proxy_cfg.as_ref(),
        None,
    )
    .await?;

    if let Some(ref update) = res {
        let bus = app.state::<Bus>();
        bus.publish(tokage_core::DomainEvent::KnowledgePackUpdateAvailable {
            pack_version: update.pack_version.clone(),
            pending_path: update.pending_path.clone(),
        });
        let _ = app.emit("knowledge-pack-update-available", update);
    }

    Ok(res)
}

/// Gets currently staged pending knowledge pack update info if pending.json exists.
#[tauri::command]
fn get_pending_knowledge_update(app: AppHandle) -> Result<Option<knowledge::StagedUpdateInfo>, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app_data_dir: {e}"))?;
    let pending_path = app_data_dir.join("knowledge").join("pending.json");
    if !pending_path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&pending_path)
        .map_err(|e| format!("Failed to read pending.json: {e}"))?;
    let pack = tokage_matching::parse_knowledge_pack(&content)
        .map_err(|e| format!("Invalid pending pack: {e}"))?;

    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let hash_hex = format!("{:x}", hasher.finalize());

    Ok(Some(knowledge::StagedUpdateInfo {
        pack_version: pack.pack_version,
        pending_path: pending_path.to_string_lossy().to_string(),
        sha256: hash_hex,
    }))
}

/// Atomically applies the staged pending knowledge pack into SQLite (P8.m4a).
#[tauri::command]
async fn apply_knowledge_pack_update(
    app: AppHandle,
    db: State<'_, storage::StorageState>,
) -> Result<tokage_matching::KnowledgePack, String> {
    let storage = db.get()?;
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app_data_dir: {e}"))?;

    let pack = knowledge::apply_staged_update(storage, &app_data_dir).await?;
    let _ = app.emit("knowledge-pack-applied", &pack.pack_version);
    Ok(pack)
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

/// Whether a path is an existing directory (resume checks a task's saved cwd).
#[tauri::command]
fn path_is_dir(path: String) -> bool {
    std::path::Path::new(&path).is_dir()
}

/// Adds `.tokage/` to the project's `.gitignore`, after the user agreed (P1.m6e).
#[tauri::command]
fn ignore_tokage_dir(path: String) -> Result<(), String> {
    context::add_to_gitignore(std::path::Path::new(&path)).map_err(|e| e.to_string())
}

/// Removes `.tokage/` directory inside a project path when removing a project (P8.m5b).
#[tauri::command]
fn remove_tokage_dir(path: String) -> Result<(), String> {
    let dir = std::path::Path::new(&path).join(".tokage");
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("Failed to remove .tokage directory: {e}"))?;
    }
    Ok(())
}

/// Starts one conversation turn; output streams as `chat-event` events keyed by `turn_id`.
/// Usage recording is an independent bus subscriber that ends on `ChatEnded` (F2.a).
#[tauri::command]
async fn start_chat_turn(
    app: AppHandle,
    state: State<'_, AppState>,
    turns: State<'_, chat::TurnRegistry>,
    bus: State<'_, Bus>,
    request: chat::ChatRequest,
    debug_state: State<'_, debug_provider::DebugState>,
) -> Result<(), String> {
    if !std::path::Path::new(&request.cwd).is_dir() {
        return Err(format!("Project folder not found: {}", request.cwd));
    }
    let _pending = turns.reserve(&request.turn_id)?;
    let receipts = app.path().app_data_dir().map_err(|e| e.to_string())?.join("turn-receipts");
    tokage_native::recovery::claim_launch(&receipts, &request.turn_id, &serde_json::json!({
        "turnId": request.turn_id, "sessionId": request.session_id, "taskCode": request.task_code,
        "provider": request.provider, "model": request.model, "cwd": request.cwd,
    })).map_err(|e| e.to_string())?;
    let mut request = request;
    chat::demo::enforce_route(&mut request)?;
    if let Some(config) = request.debug.clone().filter(|c| c.output == chat::debug::DebugOutput::Mock) {
        if !chat::debug::is_mock_workspace(std::path::Path::new(&request.cwd)) { return Err("Debug Provider: use the isolated mock workspace".into()); }
        debug_state.remember_request(&request, config);
        if !turns.spawn_mock(bus.inner().clone(), request) { return Err("Turn launch was cancelled".into()); }
        return Ok(());
    }
    // Strict surrogate routing occurs before usage attribution and normal CLI execution.
    let providers = state.providers_or_scan().await;
    chat::debug::route(&mut request, &providers)?;
    if request.debug.as_ref().is_some_and(|c| c.output != chat::debug::DebugOutput::Off) {
        if app.state::<storage::StorageState>().get().ok().and_then(|db| db.get_provider_health(request.provider.as_str()).ok().flatten()).is_some_and(|row| row.status == "MELTED") {
            return Err(format!("Debug Provider: configured target {} is unavailable; recover its health in CLI management.", request.provider));
        }
    }
    if request.proxy.is_none() {
        request.proxy = state.proxy_config.read().await.clone();
    }
    let env = state.env().await;
    let bus = bus.inner().clone();
    let mirror = mirror_dir(&app);

    if _pending.cancelled() { return Err("Turn cancelled before native launch".into()); }

    // Usage listener: independent of the turn task so cancel can later target the CLI owner.
    let mut rx = bus.subscribe();
    let req = request.clone();
    let usage_listener = tokio::spawn(async move {
        while let Some(bus_ev) = rx.recv().await {
            match bus_ev {
                DomainEvent::ChatOutput { turn_id, event } if turn_id == req.turn_id => {
                    if let Some(usage) = event.usage() {
                        let attempt_id = req.effective_attempt_id();
                        let role = req.effective_role().as_str();
                        let phase = req.effective_phase();
                        let mut cost_usd = usage.cost_usd;
                        let mut equiv_cost_usd = None;
                        if let Ok(storage) = app.state::<storage::StorageState>().get() {
                            // Ensure attempt stub so usage_events.attempt_id FK sticks (F12).
                            if let (Some(aid), Some(sid)) = (&attempt_id, &req.session_id) {
                                let stub = storage::AttemptRow {
                                    id: aid.clone(),
                                    main_session_id: sid.clone(),
                                    task_id: req.task_id,
                                    role: role.to_string(),
                                    provider_name: req.provider.as_str().to_string(),
                                    model_id: req.model.clone(),
                                    cli_window_identity: None,
                                    reused_from_id: None,
                                    predecessor_id: None,
                                    exec_mode: "HEADLESS".into(),
                                    debug_output: req.debug.as_ref().filter(|c| c.output != chat::debug::DebugOutput::Off).map(|c| c.output.name().to_string()),
                                    requested_provider: req.debug_requested.as_ref().map(|(p, _)| p.as_str().to_string()),
                                    requested_model: req.debug_requested.as_ref().map(|(_, m)| m.clone()),
                                    workdir: req.cwd.clone(),
                                    status: "RUNNING".into(),
                                    baseline_tokens: 0,
                                    context_tokens: 0,
                                    token_source: "ESTIMATED".into(),
                                    turn_count: 0,
                                    last_turn_ended_at: None,
                                    touched_files_json: "[]".into(),
                                    created_at: None,
                                    ended_at: None,
                                };
                                if let Err(e) = storage.upsert_attempt(stub).await {
                                    tracing::warn!(
                                        turn_id = %req.turn_id,
                                        error = %e,
                                        "upsert attempt stub for usage failed"
                                    );
                                }
                            }
                            if usage.credits.is_none() {
                                if let Ok(conn) = storage.read_conn() {
                                    let pack = dashboard::get_effective_pack(&conn);
                                    if let Some((computed, is_sub)) = compute_token_cost_usd(
                                        &pack,
                                        req.provider.as_str(),
                                        &req.model,
                                        usage.input_tokens as i64,
                                        usage.cache_read_tokens as i64,
                                        usage.cache_write_tokens as i64,
                                        usage.output_tokens as i64,
                                    ) {
                                        if is_sub {
                                            equiv_cost_usd = Some(computed);
                                        } else if cost_usd.is_none() {
                                            cost_usd = Some(computed);
                                        }
                                    }
                                }
                            }
                        }
                        let row = storage::UsageEventRow {
                            id: None,
                            attempt_id,
                            task_id: req.task_id,
                            plan_id: req.plan_id,
                            provider_name: req.provider.as_str().to_string(),
                            model_id: Some(req.model.clone()),
                            role: Some(role.to_string()),
                            domain: req.domain.clone(),
                            turn_index: None,
                            input_tokens: usage.input_tokens as i64,
                            cache_read_tokens: usage.cache_read_tokens as i64,
                            cache_write_tokens: usage.cache_write_tokens as i64,
                            output_tokens: usage.output_tokens as i64,
                            api_calls: usage.api_calls.map(|n| n as i64),
                            context_tokens: usage.context_tokens.map(|n| n as i64),
                            gap_seconds: None,
                            cost_usd,
                            equiv_cost_usd,
                            credits: usage.credits,
                            phase: phase.to_string(),
                            source: usage.source.as_str().to_string(),
                            created_at: None,
                        };
                        if let Ok(storage) = app.state::<storage::StorageState>().get() {
                            if let Err(e) = storage.record_turn_usage(req.turn_id.clone(), row).await {
                                tracing::warn!(
                                    turn_id = %req.turn_id,
                                    error = %e,
                                    "record_usage failed"
                                );
                            }
                        }
                    }
                }
                DomainEvent::ChatEnded { turn_id, .. } if turn_id == req.turn_id => break,
                _ => {}
            }
        }
    });

    if !turns.spawn(bus, request, providers, env, mirror) {
        usage_listener.abort();
        return Err("Turn cancelled or already active during native launch".into());
    }
    Ok(())
}

#[tauri::command]
async fn cancel_chat_turn(
    turns: State<'_, chat::TurnRegistry>,
    turn_id: String,
) -> Result<(), String> {
    let _ = turns.cancel(&turn_id).await;
    Ok(())
}

/// Forwards bus events the frontend listens to. Runs for the app's lifetime.
fn forward_to_frontend(app: AppHandle, bus: &Bus) {
    let mut rx = bus.subscribe();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                DomainEvent::ChatOutput { turn_id, event } => {
                    let _ = app.emit(CHAT_EVENT, ChatEvent { turn_id, kind: event });
                }
                DomainEvent::ChatEnded {
                    turn_id,
                    cancelled,
                } => {
                    let _ = app.emit(
                        CHAT_ENDED,
                        ChatEndedPayload {
                            turn_id,
                            cancelled,
                        },
                    );
                }
                _ => {}
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

/// Retrieves chat logs for an execution window after an ID (P8.m5a).
#[tauri::command]
async fn get_chat_logs_cmd(
    db: State<'_, storage::StorageState>,
    attempt_id: String,
    after_id: Option<i64>,
    limit: Option<i64>,
) -> Result<Vec<storage::ChatLogRow>, String> {
    db.get()?.get_chat_logs(&attempt_id, after_id, limit)
}

/// Full-text searches chat logs with FTS5 trigram (P8.m5a).
#[tauri::command]
async fn search_chat_logs_cmd(
    db: State<'_, storage::StorageState>,
    query: String,
    session_id: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<storage::ChatLogRow>, String> {
    db.get()?.search_chat_logs(&query, session_id.as_deref(), limit)
}

/// Triggers idle retention cleanup (chat logs > 90d, events > 365d) + incremental vacuum (P8.m5a).
#[tauri::command]
async fn cleanup_retention_records_cmd(
    db: State<'_, storage::StorageState>,
) -> Result<(usize, usize), String> {
    db.get()?.cleanup_retention_records().await
}

/// Exports all sessions and chat logs for a project to JSON (P8.m5b).
#[tauri::command]
async fn export_project_data_cmd(
    db: State<'_, storage::StorageState>,
    project_id: String,
) -> Result<serde_json::Value, String> {
    db.get()?.export_project_data(&project_id)
}

/// Clears database records for a project (P8.m5b).
#[tauri::command]
async fn clear_project_records_cmd(
    db: State<'_, storage::StorageState>,
    project_id: String,
) -> Result<(usize, usize), String> {
    db.get()?.clear_project_records(project_id).await
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

/// What the authorization mode does with a command or edit (settings preview; approvals later).
/// `full_auto` requires a supported outer or native workspace sandbox.
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
        .is_some_and(|a| a.meta().caps.supports_full_auto());
    policy::authorize(db.get()?, request, sandboxed, home, app_data)
}

#[tauri::command]
async fn list_approvals(
    db: State<'_, storage::StorageState>,
    session_id: Option<String>,
) -> Result<Vec<storage::approvals::ApprovalRecord>, String> {
    db.get()?.approvals(session_id.as_deref())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct ResolveApprovalRequest {
    id: String,
    session_id: Option<String>,
    turn_id: Option<String>,
    attempt_id: Option<String>,
    tool_id: Option<String>,
    command: String,
    decision: tokage_core::auth::UserDecision,
    pattern: Option<String>,
    project_id: Option<String>,
    mode: Option<String>,
}

#[tauri::command]
async fn resolve_approval(
    db: State<'_, storage::StorageState>,
    bus: State<'_, Bus>,
    request: ResolveApprovalRequest,
) -> Result<(), String> {
    let rec = storage::approvals::ApprovalRecord {
        id: None,
        attempt_id: request.attempt_id.clone(),
        session_id: request.session_id.clone(),
        turn_id: request.turn_id.clone(),
        command: request.command.clone(),
        level: "sensitive".into(),
        reason: None,
        decision: request.decision.as_str().to_string(),
        mode: request.mode.clone().unwrap_or_else(|| "ask_sensitive".into()),
        decided_at: None,
    };
    db.get()?.record_approval(rec).await?;

    if request.decision == tokage_core::auth::UserDecision::AllowProject {
        let pattern = request.pattern.clone().unwrap_or_else(|| request.command.clone());
        let _ = policy::update(
            db.get()?,
            policy::RuleChange::Upsert {
                project_id: request.project_id.clone(),
                provider: None,
                pattern,
                level: tokage_core::command_policy::Level::Project,
            },
        )
        .await?;
    }

    bus.publish(DomainEvent::ChatOutput {
        turn_id: request.turn_id.clone().unwrap_or_default(),
        event: tokage_core::EventKind::ApprovalResolved {
            id: request.id.clone(),
            decision: request.decision.as_str().to_string(),
        },
    });

    Ok(())
}

#[tauri::command]
async fn get_proxy_config(
    state: State<'_, AppState>,
    db: State<'_, storage::StorageState>,
) -> Result<tokage_core::proxy::GlobalProxyConfig, String> {
    if let Some(cfg) = state.proxy_config.read().await.clone() {
        return Ok(cfg);
    }
    if let Ok(storage) = db.get() {
        if let Ok(snapshot) = storage.load() {
            if let Some(val) = snapshot
                .settings
                .get("settings.proxy_config")
                .or_else(|| snapshot.settings.get("proxy_config"))
            {
                if let Ok(cfg) = serde_json::from_value::<tokage_core::proxy::GlobalProxyConfig>(val.clone()) {
                    *state.proxy_config.write().await = Some(cfg.clone());
                    return Ok(cfg);
                }
            }
        }
    }
    let default_cfg = tokage_core::proxy::GlobalProxyConfig::default();
    *state.proxy_config.write().await = Some(default_cfg.clone());
    Ok(default_cfg)
}

#[tauri::command]
async fn save_proxy_config(
    state: State<'_, AppState>,
    db: State<'_, storage::StorageState>,
    config: tokage_core::proxy::GlobalProxyConfig,
) -> Result<(), String> {
    if let Some(ref up) = config.upstream {
        if let (Some(ref u), Some(ref p)) = (&up.username, &up.password) {
            let _ = tokage_native::proxy::store_proxy_credential(u, p);
        }
    }
    for (_, cli_cfg) in &config.per_cli {
        if let Some(ref up) = cli_cfg.upstream {
            if let (Some(ref u), Some(ref p)) = (&up.username, &up.password) {
                let _ = tokage_native::proxy::store_proxy_credential(u, p);
            }
        }
    }

    *state.proxy_config.write().await = Some(config.clone());

    if let Ok(storage) = db.get() {
        if let Ok(val) = serde_json::to_value(&config) {
            let batch = storage::WriteBatch {
                settings: vec![storage::SettingRow {
                    key: "settings.proxy_config".into(),
                    value: val,
                }],
                ..Default::default()
            };
            let _ = storage.write(batch);
        }
    }
    Ok(())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SystemProxyResponse {
    upstream: Option<tokage_core::proxy::ProxyUpstream>,
    no_proxy: Option<String>,
}

#[tauri::command]
async fn get_system_proxy() -> Result<SystemProxyResponse, String> {
    let upstream = tokage_native::proxy::detect_system_proxy();
    let no_proxy = tokage_native::proxy::detect_system_no_proxy();
    Ok(SystemProxyResponse { upstream, no_proxy })
}

#[tauri::command]
async fn test_proxy_connection(
    provider: Option<ProviderId>,
    endpoint: Option<String>,
    config: Option<tokage_core::proxy::GlobalProxyConfig>,
) -> Result<tokage_core::proxy::ProxyTestResult, String> {
    let target = endpoint
        .as_deref()
        .or_else(|| provider.map(tokage_native::proxy::default_endpoint_for));
    let upstream = config.as_ref().and_then(|c| {
        provider
            .and_then(|p| c.resolved_upstream_for(p))
            .or_else(|| c.upstream.as_ref())
    });
    let no_proxy = config.as_ref().map(|c| c.no_proxy.as_str());
    let ca_file = config.as_ref().and_then(|c| c.ca_file.as_deref());

    let res = tokage_native::proxy::test_proxy_connection(target, upstream, no_proxy, ca_file).await;
    Ok(res)
}

#[tauri::command]
async fn get_all_provider_proxy_statuses(
    state: State<'_, AppState>,
) -> Result<Vec<tokage_core::proxy::ProviderProxyStatus>, String> {
    let env = state.env().await;
    let featured_ids = [
        ProviderId::ClaudeCode,
        ProviderId::CodexCli,
        ProviderId::CursorCli,
        ProviderId::CopilotCli,
        ProviderId::AntigravityCli,
        ProviderId::Aider,
        ProviderId::Opencode,
    ];
    let mut statuses = Vec::new();
    for id in featured_ids {
        statuses.push(tokage_native::proxy::inspect_provider_proxy(id, &env));
    }
    Ok(statuses)
}

#[tauri::command]
async fn list_native_sessions(
    provider: Option<ProviderId>,
    project_path: Option<String>,
) -> Result<Vec<tokage_core::NativeSession>, String> {
    let p_path = project_path.map(std::path::PathBuf::from);
    tokio::task::spawn_blocking(move || {
        let p_ref = p_path.as_deref();
        if let Some(pid) = provider {
            tokage_native::session::read_native_sessions(pid, p_ref)
        } else {
            tokage_native::session::read_all_native_sessions(p_ref)
        }
    })
    .await
    .map_err(|e| e.to_string())
}

/// Evaluates window reuse for task handover (docs/05 §4, P8.m1a).
#[tauri::command]
async fn evaluate_window_reuse_cmd(
    params: tokage_context::WindowReuseParams,
) -> Result<tokage_context::WindowReuseDecision, String> {
    Ok(tokage_context::decide_window_reuse(&params))
}

/// R10: task brief fixed header must be byte-stable (docs/05 §8).
#[tauri::command]
fn validate_task_brief_header_cmd(brief: String) -> Result<(), String> {
    tokage_context::validate_task_brief_header(&brief).map_err(|e| e.to_string())
}

/// R9: same window must not change model / role / startup args (docs/05 §8).
#[tauri::command]
fn validate_window_continuity_cmd(
    initial: tokage_context::WindowConfig,
    current: tokage_context::WindowConfig,
) -> Result<(), String> {
    tokage_context::validate_window_continuity(&initial, &current).map_err(|e| e.to_string())
}

/// R10: user message transmitted verbatim (docs/05 §8).
#[tauri::command]
fn validate_user_message_integrity_cmd(
    raw_user_input: String,
    transmitted_message: String,
) -> Result<(), String> {
    tokage_context::validate_user_message_integrity(&raw_user_input, &transmitted_message)
        .map_err(|e| e.to_string())
}

/// Routes an appended user message in the main session or composer (docs/05 §6, P8.m1e).
#[tauri::command]
async fn triage_main_session_message_cmd(
    params: tokage_context::TriageMessageParams,
) -> Result<tokage_context::AppendMessageRoute, String> {
    Ok(tokage_context::triage_main_session_message(&params))
}

/// Lists context actions for an attempt.
#[tauri::command]
async fn list_context_actions_cmd(
    db: State<'_, storage::StorageState>,
    attempt_id: String,
) -> Result<Vec<storage::ContextActionRow>, String> {
    let storage = db.get()?;
    storage.list_context_actions(&attempt_id)
}

/// Settles actual savings for a context action after execution completes.
#[tauri::command]
async fn settle_context_savings_cmd(
    db: State<'_, storage::StorageState>,
    action_id: i64,
    baseline_continue_cost: f64,
    actual_turn_cost: f64,
) -> Result<f64, String> {
    let storage = db.get()?;
    let saving = tokage_context::settle_actual_savings(baseline_continue_cost, actual_turn_cost);
    storage.settle_context_action_savings(action_id, saving).await?;
    Ok(saving)
}

/// Trips a provider's circuit breaker and emits DomainEvent (docs/06 §6.1, P8.m2c).
#[tauri::command]
async fn trip_provider_melt_cmd(
    app: AppHandle,
    db: State<'_, storage::StorageState>,
    provider_name: String,
    reason: String,
    melted_until: Option<String>,
) -> Result<(), String> {
    let storage = db.get()?;
    let r = storage::MeltReason::from_str(&reason).unwrap_or(storage::MeltReason::QuotaHard);
    storage
        .trip_provider_melt(provider_name.clone(), r, melted_until.clone())
        .await?;

    let bus = app.state::<Bus>();
    if let Ok(pid) =
        serde_json::from_value::<ProviderId>(serde_json::Value::String(provider_name.clone()))
    {
        bus.publish(DomainEvent::ProviderMelted {
            provider: pid,
            reason: reason.clone(),
        });
    }
    let _ = app.emit(
        "provider-melted",
        health_sub::ProviderMeltedPayload {
            provider: provider_name,
            reason,
            until: melted_until,
        },
    );
    Ok(())
}

/// Recovers a melted provider back to HEALTHY / READY (docs/06 §6.3, P8.m2c).
#[tauri::command]
async fn recover_provider_health_cmd(
    app: AppHandle,
    db: State<'_, storage::StorageState>,
    provider_name: String,
    new_status: Option<String>,
) -> Result<(), String> {
    let storage = db.get()?;
    storage
        .recover_provider_health(provider_name.clone(), new_status)
        .await?;
    let _ = app.emit(
        "provider-recovered",
        health_sub::ProviderRecoveredPayload {
            provider: provider_name,
            reason: None,
            until: None,
        },
    );
    Ok(())
}

/// Lists all provider health rows (P8.m2c).
#[tauri::command]
async fn list_provider_health_cmd(
    db: State<'_, storage::StorageState>,
) -> Result<Vec<storage::ProviderHealthRow>, String> {
    let storage = db.get()?;
    storage.list_provider_health()
}

/// Computes local projected utilization u_now and calibrates quota equivalent cost (docs/06 §4.2, P8.m2a).
#[tauri::command]
async fn get_provider_utilization_now_cmd(
    provider: ProviderId,
    equiv_cost_since_last: f64,
) -> Result<serde_json::Value, String> {
    let (u_proj, source) = usage::project_utilization_now(provider, equiv_cost_since_last);
    let equiv_cost = usage::get_quota_equiv_cost(provider);
    Ok(serde_json::json!({
        "provider": provider.as_str(),
        "utilizationPct": (u_proj * 1000.0).round() / 10.0,
        "source": source,
        "quotaEquivCostUsd": equiv_cost
    }))
}

/// Reassigns unstarted tasks in a plan when a provider melts (docs/06 §6.2, P8.m2c).
#[tauri::command]
async fn reassign_unstarted_tasks_cmd(
    db: State<'_, storage::StorageState>,
    plan: tokage_core::planning::Plan,
    current_assignments: Vec<tokage_matching::TaskAssignment>,
    completed_or_running_task_ids: Vec<String>,
    melted_providers: Vec<String>,
    available_models: Option<Vec<tokage_matching::AvailableModel>>,
) -> Result<tokage_matching::PlanAllocationResult, String> {
    let storage = db.get()?;
    let layers = load_layers(&storage)?;
    let pack = tokage_matching::merge_layers(&layers).map_err(|e| e.to_string())?;
    let pack = tokage_matching::restrict_pack(&pack, available_models.as_deref(), &Default::default());
    let learned = build_learned_index(&storage);
    let confidence = settings_confidence_threshold(&storage);

    let completed_set: std::collections::HashSet<String> =
        completed_or_running_task_ids.into_iter().collect();
    let melted_set: std::collections::HashSet<String> = melted_providers.into_iter().collect();

    Ok(tokage_matching::reassign_unstarted_tasks(
        &plan,
        &current_assignments,
        &completed_set,
        &melted_set,
        &pack,
        &learned,
        None,
        confidence,
    ))
}

/// F13.d: reassign one task excluding listed models (no silent fallback to first candidate).
#[tauri::command]
async fn reassign_task(
    db: State<'_, storage::StorageState>,
    plan: tokage_core::planning::Plan,
    task_id: String,
    exclude_models: Vec<String>,
    available_models: Option<Vec<tokage_matching::AvailableModel>>,
) -> Result<tokage_matching::TaskAssignment, String> {
    let storage = db.get()?;
    let layers = load_layers(&storage)?;
    let pack = tokage_matching::merge_layers(&layers).map_err(|e| e.to_string())?;
    let pack = tokage_matching::restrict_pack(&pack, available_models.as_deref(), &Default::default());
    let learned = build_learned_index(&storage);
    let confidence = settings_confidence_threshold(&storage);
    tokage_matching::reassign_task_excluding(
        &plan,
        &task_id,
        &exclude_models,
        &pack,
        &learned,
        None,
        confidence,
    )
}

/// F13.f: sum usage by phase for a plan (`COALESCE(cost_usd, equiv_cost_usd)`).
#[tauri::command]
async fn plan_cost_summary(
    db: State<'_, storage::StorageState>,
    plan_id: i64,
) -> Result<serde_json::Value, String> {
    let storage = db.get()?;
    let conn = storage.read_conn()?;
    storage::budget::plan_cost_summary(&conn, plan_id)
        .map_err(|e| e.to_string())
}

/// Checks plan budget redlines (docs/06 §5.1, docs/09 R12, F13.f).
/// Hard budget always pauses; 50% overrun / repair>30% of estimate pause only when `pause_on_overrun`.
#[tauri::command]
async fn check_plan_budget_overrun_cmd(
    est_cost_usd: f64,
    current_actual_cost_usd: f64,
    plan_budget_usd: Option<f64>,
    repair_cost_usd: Option<f64>,
    pause_on_overrun: Option<bool>,
) -> Result<serde_json::Value, String> {
    let pause_soft = pause_on_overrun.unwrap_or(true);
    let repair = repair_cost_usd.unwrap_or(0.0);
    let overrun_ratio = if est_cost_usd > 0.0 {
        (current_actual_cost_usd - est_cost_usd) / est_cost_usd
    } else {
        0.0
    };

    let is_50pct_overrun = est_cost_usd > 0.0 && current_actual_cost_usd > (est_cost_usd * 1.50);
    let is_repair_overrun = est_cost_usd > 0.0 && repair > (est_cost_usd * 0.30);
    let is_redline_exceeded = match plan_budget_usd {
        Some(b) if b > 0.0 => current_actual_cost_usd >= b,
        _ => false,
    };

    let should_pause = is_redline_exceeded
        || (pause_soft && (is_50pct_overrun || is_repair_overrun));

    Ok(serde_json::json!({
        "estCostUsd": est_cost_usd,
        "actualCostUsd": current_actual_cost_usd,
        "repairCostUsd": repair,
        "overrunRatio": overrun_ratio,
        "is50PctOverrun": is_50pct_overrun,
        "isRepairOverrun": is_repair_overrun,
        "isRedlineExceeded": is_redline_exceeded,
        "shouldPauseQueue": should_pause,
        "message": if is_redline_exceeded {
            format!("Budget hard limit (R12) hit: actual spend ${:.2} exceeds the budget cap ${:.2}; the orchestration queue is paused automatically.", current_actual_cost_usd, plan_budget_usd.unwrap_or(0.0))
        } else if is_repair_overrun {
            format!("Repair cost ${:.2} exceeds 30% of the estimate ${:.2}.", repair, est_cost_usd)
        } else if is_50pct_overrun {
            format!("Actual spend ${:.2} exceeds the plan estimate ${:.2} by 50%.", current_actual_cost_usd, est_cost_usd)
        } else {
            "Budget is within the normal range.".to_string()
        }
    }))
}

/// Queries the complete usage and quality dashboard data (docs/06 §7, P8.m3b).
#[tauri::command]
async fn get_usage_dashboard(
    range: Option<String>,
    storage_state: State<'_, storage::StorageState>,
) -> Result<dashboard::UsageDashboard, String> {
    let db = storage_state.get()?;
    let conn = db.get_conn()?;
    dashboard::build_usage_dashboard(&conn, range.as_deref()).map_err(|e| e.to_string())
}

/// Settles plan savings upon completion against reference model baseline (docs/06 §5.2, P8.m3a).
#[tauri::command]
async fn settle_plan_savings_cmd(
    plan_id: i64,
    user_override: Option<String>,
    storage_state: State<'_, storage::StorageState>,
) -> Result<Option<i64>, String> {
    let db = storage_state.get()?;
    let conn = db.get_conn()?;
    let is_debug = conn.query_row("SELECT EXISTS(SELECT 1 FROM attempts a JOIN tasks t ON t.id = a.task_id WHERE t.plan_id = ?1 AND a.debug_output IS NOT NULL)", [plan_id], |r| r.get::<_, bool>(0)).map_err(|e| e.to_string())?;
    if is_debug { return Ok(None); }
    dashboard::settle_plan_savings(&conn, plan_id, user_override.as_deref()).map_err(|e| e.to_string())
}

/// Manually records a savings event.
#[tauri::command]
async fn record_savings_event_cmd(
    session_id: Option<String>,
    plan_id: Option<i64>,
    kind: String,
    ref_id: Option<String>,
    baseline_usd: f64,
    actual_usd: Option<f64>,
    detail_json: Option<String>,
    storage_state: State<'_, storage::StorageState>,
) -> Result<i64, String> {
    let db = storage_state.get()?;
    let event = storage::SavingsEventRow {
        id: None,
        main_session_id: session_id,
        plan_id,
        kind,
        ref_id,
        baseline_usd,
        actual_usd,
        settled: actual_usd.is_some(),
        detail_json: detail_json.unwrap_or_else(|| "{}".to_string()),
        created_at: None,
    };
    db.record_savings_event(event).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let context = tauri::generate_context!();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .manage(runtime::RuntimeState::default())
        .manage(debug_provider::DebugState::default())
        .manage(chat::TurnRegistry::default())
        .manage(Bus::new())
        .setup(|app| {
            forward_to_frontend(app.handle().clone(), &app.state::<Bus>());
            let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
            let run_dir = app_data.join("run");
            std::fs::create_dir_all(&run_dir)?;
            // A second app must not reap the first app's live processes at startup.
            let lease = tokage_native::file_lease::FileLease::acquire(&run_dir.join(".owner.lock"))?;
            app.manage(lease);
            proc::set_run_dir(&run_dir);
            let reaped = proc::cleanup_leftovers(&run_dir);
            if !tokage_native::recovery::runtime_idle(Some(&run_dir))? {
                return Err(std::io::Error::other("Startup process cleanup is unconfirmed; inspect the retained registry").into());
            }
            if !reaped.is_empty() {
                tracing::info!("Reaped {} leftover processes from previous session", reaped.len());
            }
            let db = storage::Storage::open(&app_data);
            if let Err(e) = &db {
                tracing::error!("database: {e}");
            }
            app.manage(storage::StorageState(db));
            runtime::monitor(app.handle().clone());
            // F15: health subscriber + recovery loop (needs StorageState managed first).
            health_sub::spawn_health_subscriber(app.handle().clone(), &app.state::<Bus>());
            health_sub::spawn_recovery_loop(app.handle().clone());
            // F17.b: retention cleanup — first run ~60s after open, then every 24h.
            retention_sub::spawn_retention_loop(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            runtime::runtime_worker_error, runtime::runtime_config, runtime::runtime_submit, runtime::runtime_snapshot, runtime::runtime_list, runtime::runtime_events, runtime::runtime_transcript,
            refresh_providers,
            refresh_provider,
            reload_shell_env,
            git_info,
            git_list_branches,
            git_checkout_branch,
            git_create_branch,
            git_status_files,
            git_file_diff,
            create_session_worktree,
            commit_task_worktree,
            merge_task_worktree_cmd,
            prepare_integration_worktree,
            remove_task_worktree,
            git_rev_parse,
            git_checkout_identity,
            git_scope_snapshot,
            git_capture_rollback,
            git_rollback_changes,
            merge_integration_to_checkout,
            export_integration_patch,
            reconcile_integration_delivery,
            git_contains_commit,
            prune_project_worktrees,
            list_orphan_worktrees,
            reconcile_worktrees_on_startup,
            remove_orphan_worktree,
            remove_session_worktree,
            read_project_file,
            list_open_targets,
            open_path_in,
            open_external_url,
            build_info,
            fetch_knowledge_pack,
            check_knowledge_pack_update,
            get_pending_knowledge_update,
            apply_knowledge_pack_update,
            get_debug_provider_catalog,
            prepare_debug_workspace,
            start_chat_turn,
            cancel_chat_turn,
            save_session_plan,
            tokage_dir_exists,
            path_is_dir,
            ignore_tokage_dir,
            remove_tokage_dir,
            storage_load,
            storage_write,
            storage_import,
            get_command_policy,
            update_command_rules,
            authorize_command,
            list_approvals,
            resolve_approval,
            get_proxy_config,
            save_proxy_config,
            get_system_proxy,
            test_proxy_connection,
            get_all_provider_proxy_statuses,
            list_native_sessions,
            query_provider_quota,
            verify_antigravity_api_key,
            verify_reasonix_api_key,
            get_repo_digest,
            render_planning_prompt,
            render_plan_repair_prompt,
            parse_planning_output,
            validate_plan,
            persist_plan,
            persist_plan_user_edited,
            reset_plan_to_default,
            load_plan,
            save_pipeline_state_cmd,
            load_pipeline_state_cmd,
            task_recovery_evidence,
            compute_plan_diff,
            apply_plan_diff,
            assemble_replan_prompt,
            generate_adaptation_tasks,
            triage_request,
            allocate_plan_models,
            assign_direct_task,
            calculate_direct_counterfactual_cost,
            record_plan_approved,
            pick_dispatchable_tasks,
            assemble_task_brief_cmd,
            parse_or_generate_task_report_cmd,
            record_task_report,
            get_task_reports,
            ensure_execution_task,
            execute_gate_command_cmd,
            cancel_gate_command_cmd,
            validate_write_scope_cmd,
            revert_out_of_scope_files_cmd,
            terminal_resume_command,
            decide_repair_action_cmd,
            generate_implementation_fix_task_cmd,
            record_gate_run,
            list_gate_runs,
            generate_delivery_summary_cmd,
            derive_session_title_from_spec_cmd,
            upsert_attempt_cmd,
            list_attempts_cmd,
            record_task_outcome,
            list_task_outcomes,
            list_learned_stats,
            upsert_model_override,
            list_model_overrides,
            delete_model_override,
            replace_custom_packs_cmd,
            list_custom_packs_cmd,
            evaluate_window_reuse_cmd,
            validate_task_brief_header_cmd,
            validate_window_continuity_cmd,
            validate_user_message_integrity_cmd,
            triage_main_session_message_cmd,
            list_context_actions_cmd,
            settle_context_savings_cmd,
            trip_provider_melt_cmd,
            recover_provider_health_cmd,
            list_provider_health_cmd,
            get_provider_utilization_now_cmd,
            reassign_unstarted_tasks_cmd,
            reassign_task,
            plan_cost_summary,
            check_plan_budget_overrun_cmd,
            get_usage_dashboard,
            settle_plan_savings_cmd,
            record_savings_event_cmd,
            get_chat_logs_cmd,
            search_chat_logs_cmd,
            cleanup_retention_records_cmd,
            export_project_data_cmd,
            clear_project_records_cmd
        ])


        .build(context)
        .expect("error while building tokage")
        .run(|app, event| {
            // Commit queued writes before the process goes.
            if let tauri::RunEvent::Exit = event {
                if let Ok(db) = app.state::<storage::StorageState>().get() {
                    db.flush_blocking();
                }
                if let Ok(app_data) = app.path().app_data_dir() {
                    let run_dir = app_data.join("run");
                    proc::cleanup_leftovers(&run_dir);
                }
            }
        });
}
