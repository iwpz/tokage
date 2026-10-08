mod dashboard;
mod knowledge;
mod open_in;
mod policy;

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

/// Queries a provider's official quota via zero-token slash command in PTY (P4.m3d) or returns cached snapshot.
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

    let slash_cmd = match provider {
        ProviderId::ClaudeCode => Some("/usage"),
        ProviderId::CodexCli => Some("/status"),
        ProviderId::AntigravityCli => Some("/usage"),
        ProviderId::CopilotCli => Some("/usage"),
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
) -> Result<(), String> {
    let p = std::path::PathBuf::from(cwd);
    tokio::task::spawn_blocking(move || tokage_planning::save_pipeline_state(&p, &state))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Loads the execution pipeline state from `<cwd>/.tokage/execution/<sessionId>.json` (m2b/m2c).
#[tauri::command]
async fn load_pipeline_state_cmd(
    cwd: String,
    session_id: String,
) -> Result<Option<tokage_planning::PipelineState>, String> {
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
) -> Result<tokage_matching::PlanAllocationResult, String> {
    let builtin = tokage_matching::builtin_knowledge_pack().clone();
    let layers = tokage_matching::LayeredPacks {
        builtin,
        official: None,
        customs: vec![],
        overrides: vec![],
        learned_usage_profiles: None,
    };
    let pack = tokage_matching::merge_layers(&layers).map_err(|e| e.to_string())?;

    let mut melted_providers = std::collections::HashSet::new();
    if let Ok(storage) = db.get() {
        if let Ok(health_list) = storage.list_provider_health() {
            for h in health_list {
                if h.status == "MELTED" {
                    melted_providers.insert(h.provider_name);
                }
            }
        }
    }

    if let Some(overrides) = manual_overrides {
        Ok(tokage_matching::recompute_plan_allocation_with_overrides(
            &plan,
            &pack,
            &overrides,
            initial_provider_utils.as_ref(),
        ))
    } else if !melted_providers.is_empty() {
        Ok(tokage_matching::allocate_plan_models_with_melted(
            &plan,
            &pack,
            initial_provider_utils.as_ref(),
            &melted_providers,
        ))
    } else {
        Ok(tokage_matching::allocate_plan_models(
            &plan,
            &pack,
            initial_provider_utils.as_ref(),
        ))
    }
}

/// Assigns a model for a single DIRECT task (docs/04 §8, P6.m5c).
#[tauri::command]
async fn assign_direct_task(
    domain: String,
    difficulty: u8,
    size: Option<tokage_core::planning::TaskSize>,
    initial_provider_utils: Option<std::collections::HashMap<String, f64>>,
) -> Result<Option<tokage_matching::TaskAssignment>, String> {
    let builtin = tokage_matching::builtin_knowledge_pack().clone();
    let layers = tokage_matching::LayeredPacks {
        builtin,
        official: None,
        customs: vec![],
        overrides: vec![],
        learned_usage_profiles: None,
    };
    let pack = tokage_matching::merge_layers(&layers).map_err(|e| e.to_string())?;

    Ok(tokage_matching::assign_direct_task(
        &domain,
        difficulty,
        size,
        &pack,
        initial_provider_utils.as_ref(),
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
    Ok(tokage_orchestration::assemble_task_brief(&input))
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

/// Retrieves all task reports for a task (docs/08 schema.md §4, P7.m2b).
#[tauri::command]
async fn get_task_reports(
    db: State<'_, storage::StorageState>,
    task_id: String,
) -> Result<Vec<storage::TaskReportRow>, String> {
    db.get()?.get_task_reports(&task_id)
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
) -> Result<tokage_orchestration::GateCommandResult, String> {
    let env = state.env().await;
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
fn cancel_gate_command_cmd(run_id: String) -> bool {
    tokage_orchestration::cancel_gate_command(&run_id)
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

/// Reverts out-of-scope files via git checkout (docs/09 §4, P7.m3c).
#[tauri::command]
async fn revert_out_of_scope_files_cmd(
    state: State<'_, AppState>,
    workdir: String,
    files: Vec<String>,
) -> Result<String, String> {
    let env = state.env().await;
    let path = std::path::PathBuf::from(&workdir);
    tokage_orchestration::revert_out_of_scope_files(&path, &env, &files).await
}

/// Pasteable terminal resume one-liner for takeover (docs/03 §8, F10 / A.15).
/// Returns `null` when thread id is missing or the CLI cannot resume by id (e.g. Aider).
#[tauri::command]
async fn terminal_resume_command(
    provider: ProviderId,
    thread_id: Option<String>,
    cwd: String,
) -> Result<Option<String>, String> {
    Ok(tokage_native::chat::adapter::terminal_resume_command_line(
        provider,
        thread_id.as_deref(),
        &cwd,
    ))
}

/// Computes repair action according to 3-step ladder (docs/03 §8, P7.m4a).
#[tauri::command]
async fn decide_repair_action_cmd(
    task: tokage_core::planning::TaskItem,
    current_step: tokage_orchestration::RepairStep,
    current_attempt_id: String,
    current_model: String,
    provider: String,
    evidence: tokage_orchestration::FailureEvidence,
) -> Result<(tokage_orchestration::RepairStep, tokage_orchestration::RepairDecision), String> {
    Ok(tokage_orchestration::decide_repair_action(
        &task,
        current_step,
        &current_attempt_id,
        &current_model,
        &provider,
        &evidence,
    ))
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
    plan_id: Option<String>,
) -> Result<Vec<storage::GateRunRow>, String> {
    db.get()?.list_gate_runs(task_id.as_deref(), plan_id.as_deref())
}

/// Generates completed delivery summary with optional review task (docs/03 §9, P7.m5a).
#[tauri::command]
async fn generate_delivery_summary_cmd(
    plan: tokage_core::planning::Plan,
    reports: Vec<tokage_orchestration::TaskReport>,
    full_gate_result: Option<tokage_orchestration::GateCommandResult>,
    estimated_cost: f64,
    create_review_task: bool,
) -> Result<tokage_orchestration::DeliverySummary, String> {
    Ok(tokage_orchestration::generate_delivery_summary(
        &plan,
        &reports,
        full_gate_result,
        estimated_cost,
        create_review_task,
    ))
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

/// Updates main session status machine (docs/07 §3, P7.m6b).
#[tauri::command]
async fn update_main_session_status_cmd(
    db: State<'_, storage::StorageState>,
    session_id: String,
    status: String,
) -> Result<(), String> {
    db.get()?.update_main_session_status(session_id, status).await
}



/// Records a completed task execution outcome and updates learning accumulators (docs/04 §9, P6.m6).
#[tauri::command]
async fn record_task_outcome(
    db: State<'_, storage::StorageState>,
    outcome: storage::TaskOutcomeRow,
) -> Result<i64, String> {
    let storage = db.get()?;
    let id = storage.record_task_outcome(outcome.clone()).await?;

    // Update Beta accumulator for (model_id, domain)
    if let Some(res) = tokage_matching::TaskOutcomeResult::from_str(&outcome.result) {
        let current_alpha = storage
            .list_learned_stats(&outcome.model_id)?
            .into_iter()
            .find(|s| s.domain == outcome.domain && s.metric == "success_alpha")
            .map(|s| s.value)
            .unwrap_or(4.8);

        let current_beta = storage
            .list_learned_stats(&outcome.model_id)?
            .into_iter()
            .find(|s| s.domain == outcome.domain && s.metric == "success_beta")
            .map(|s| s.value)
            .unwrap_or(1.2);

        let mut acc = tokage_matching::BetaAccumulator {
            alpha: current_alpha,
            beta: current_beta,
            samples: 0,
        };
        acc.update(res);

        let _ = storage
            .update_learned_stat(storage::LearnedStatRow {
                provider_name: outcome.provider_name.clone(),
                model_id: outcome.model_id.clone(),
                domain: outcome.domain.clone(),
                bucket: format!("d{}", outcome.difficulty),
                metric: "success_alpha".into(),
                value: acc.alpha,
                samples: acc.samples,
                updated_at: None,
            })
            .await;

        let _ = storage
            .update_learned_stat(storage::LearnedStatRow {
                provider_name: outcome.provider_name.clone(),
                model_id: outcome.model_id.clone(),
                domain: outcome.domain.clone(),
                bucket: format!("d{}", outcome.difficulty),
                metric: "success_beta".into(),
                value: acc.beta,
                samples: acc.samples,
                updated_at: None,
            })
            .await;
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

/// Returns the bundled built-in knowledge pack JSON (offline available, P6.m1b).
#[tauri::command]
fn get_builtin_knowledge_pack() -> String {
    tokage_matching::BUILTIN_KNOWLEDGE_JSON.to_string()
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
) -> Result<(), String> {
    if !std::path::Path::new(&request.cwd).is_dir() {
        return Err(format!("Project folder not found: {}", request.cwd));
    }
    let mut request = request;
    if request.proxy.is_none() {
        request.proxy = state.proxy_config.read().await.clone();
    }
    let env = state.env().await;
    let providers = state.providers_or_scan().await;
    let bus = bus.inner().clone();
    let mirror = mirror_dir(&app);

    // Usage listener: independent of the turn task so cancel can later target the CLI owner.
    let mut rx = bus.subscribe();
    let req = request.clone();
    tokio::spawn(async move {
        while let Some(bus_ev) = rx.recv().await {
            match bus_ev {
                DomainEvent::ChatOutput { turn_id, event } if turn_id == req.turn_id => {
                    if let Some(usage) = event.usage() {
                        let attempt_id = req.session_id.as_ref().map(|s| {
                            format!("{s}|{}|{}", req.provider.as_str(), req.model)
                        });
                        let role = req.effective_role().as_str();
                        let phase = match req.effective_role() {
                            tokage_core::Role::Planner => "PLANNING",
                            tokage_core::Role::Reviewer => "REVIEW",
                            tokage_core::Role::Tester => "TEST",
                            tokage_core::Role::Executor => {
                                if req.mode == chat::TurnMode::Plan {
                                    "PLANNING"
                                } else {
                                    "DIRECT"
                                }
                            }
                        };
                        let row = storage::UsageEventRow {
                            id: None,
                            attempt_id,
                            task_id: None,
                            plan_id: None,
                            provider_name: req.provider.as_str().to_string(),
                            model_id: Some(req.model.clone()),
                            role: Some(role.to_string()),
                            domain: None,
                            turn_index: None,
                            input_tokens: usage.input_tokens as i64,
                            cache_read_tokens: usage.cache_read_tokens as i64,
                            cache_write_tokens: usage.cache_write_tokens as i64,
                            output_tokens: usage.output_tokens as i64,
                            api_calls: usage.api_calls.map(|n| n as i64),
                            context_tokens: usage.context_tokens.map(|n| n as i64),
                            gap_seconds: None,
                            cost_usd: usage.cost_usd,
                            equiv_cost_usd: None,
                            phase: phase.to_string(),
                            source: usage.source.as_str().to_string(),
                            created_at: None,
                        };
                        if let Ok(storage) = app.state::<storage::StorageState>().get() {
                            let _ = storage.record_usage(row).await;
                        }
                    }
                }
                DomainEvent::ChatEnded { turn_id, .. } if turn_id == req.turn_id => break,
                _ => {}
            }
        }
    });

    turns.spawn(bus, request, providers, env, mirror);
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
async fn list_usage_events(
    db: State<'_, storage::StorageState>,
    session_id: Option<String>,
    plan_id: Option<i64>,
) -> Result<Vec<storage::UsageEventRow>, String> {
    db.get()?.usage_events(session_id.as_deref(), plan_id)
}

#[tauri::command]
async fn record_usage_event(
    db: State<'_, storage::StorageState>,
    event: storage::UsageEventRow,
) -> Result<i64, String> {
    db.get()?.record_usage(event).await
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
async fn get_provider_proxy_status(
    state: State<'_, AppState>,
    provider: ProviderId,
) -> Result<tokage_core::proxy::ProviderProxyStatus, String> {
    let env = state.env().await;
    Ok(tokage_native::proxy::inspect_provider_proxy(provider, &env))
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

/// Evaluates candidate in-window context actions (CONTINUE / COMPACT / FRESH) (docs/05 §5, P8.m1b).
#[tauri::command]
async fn plan_in_window_context_cmd(
    params: tokage_context::InWindowContextParams,
) -> Result<tokage_context::ContextActionPlan, String> {
    Ok(tokage_context::plan_context_action(&params))
}

/// Applies a context action (docs/05 §7, P8.m1d).
#[tauri::command]
async fn apply_context_action_cmd(
    db: State<'_, storage::StorageState>,
    action_id: i64,
    new_attempt_id: Option<String>,
) -> Result<(), String> {
    let storage = db.get()?;
    storage
        .update_context_action_status(action_id, "APPLIED".to_string(), new_attempt_id)
        .await
}

/// Dismisses a suggested context action (docs/05 §7, P8.m1d).
#[tauri::command]
async fn dismiss_context_action_cmd(
    db: State<'_, storage::StorageState>,
    action_id: i64,
) -> Result<(), String> {
    let storage = db.get()?;
    storage
        .update_context_action_status(action_id, "DISMISSED".to_string(), None)
        .await
}

/// Evaluates window reuse for task handover (docs/05 §4, P8.m1a).
#[tauri::command]
async fn evaluate_window_reuse_cmd(
    params: tokage_context::WindowReuseParams,
) -> Result<tokage_context::WindowReuseDecision, String> {
    Ok(tokage_context::decide_window_reuse(&params))
}

/// Routes an appended user message in the main session or composer (docs/05 §6, P8.m1e).
#[tauri::command]
async fn triage_main_session_message_cmd(
    params: tokage_context::TriageMessageParams,
) -> Result<tokage_context::AppendMessageRoute, String> {
    Ok(tokage_context::triage_main_session_message(&params))
}

/// Builds compaction payload with native slash command or fallback prompt (docs/05 §5, P8.m1c).
#[tauri::command]
async fn build_compact_cmd(
    req: tokage_context::BuildCompactRequest,
) -> Result<tokage_context::CompactExecutionPayload, String> {
    Ok(tokage_context::build_compact(&req))
}

/// Records a context action in SQLite (docs/08 schema.md §4, P8.m1f).
#[tauri::command]
async fn record_context_action_cmd(
    db: State<'_, storage::StorageState>,
    action: storage::ContextActionRow,
) -> Result<i64, String> {
    let storage = db.get()?;
    storage.record_context_action(action).await
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
    storage.trip_provider_melt(provider_name.clone(), r, melted_until).await?;

    let bus = app.state::<Bus>();
    if let Ok(pid) = serde_json::from_value::<ProviderId>(serde_json::Value::String(provider_name.clone())) {
        bus.publish(DomainEvent::ProviderMelted {
            provider: pid,
            reason: reason.clone(),
        });
    }
    let _ = app.emit("provider-melted", serde_json::json!({
        "provider": provider_name,
        "reason": reason
    }));
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
    storage.recover_provider_health(provider_name.clone(), new_status).await?;
    let _ = app.emit("provider-recovered", serde_json::json!({ "provider": provider_name }));
    Ok(())
}

/// Records a provider execution failure and checks HEALTH threshold (3 consecutive fails, docs/06 §6.1, P8.m2d).
#[tauri::command]
async fn record_provider_failure_cmd(
    app: AppHandle,
    db: State<'_, storage::StorageState>,
    provider_name: String,
) -> Result<i64, String> {
    let storage = db.get()?;
    let fails = storage.record_provider_failure(provider_name.clone()).await?;
    if fails >= 3 {
        // HEALTH melting triggered (docs/06 §6.1, docs/09)
        storage.trip_provider_melt(provider_name.clone(), storage::MeltReason::Health, None).await?;
        let _ = app.emit("provider-melted", serde_json::json!({
            "provider": provider_name,
            "reason": "HEALTH"
        }));
    }
    Ok(fails)
}

/// Resets consecutive failures on successful execution (P8.m2d).
#[tauri::command]
async fn reset_provider_failures_cmd(
    db: State<'_, storage::StorageState>,
    provider_name: String,
) -> Result<(), String> {
    let storage = db.get()?;
    storage.reset_provider_failures(provider_name).await
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
    plan: tokage_core::planning::Plan,
    current_assignments: Vec<tokage_matching::TaskAssignment>,
    completed_or_running_task_ids: Vec<String>,
    melted_providers: Vec<String>,
) -> Result<tokage_matching::PlanAllocationResult, String> {
    let builtin = tokage_matching::builtin_knowledge_pack().clone();
    let layers = tokage_matching::LayeredPacks {
        builtin,
        official: None,
        customs: vec![],
        overrides: vec![],
        learned_usage_profiles: None,
    };
    let pack = tokage_matching::merge_layers(&layers).map_err(|e| e.to_string())?;

    let completed_set: std::collections::HashSet<String> = completed_or_running_task_ids.into_iter().collect();
    let melted_set: std::collections::HashSet<String> = melted_providers.into_iter().collect();

    Ok(tokage_matching::reassign_unstarted_tasks(
        &plan,
        &current_assignments,
        &completed_set,
        &melted_set,
        &pack,
        None,
    ))
}

/// Checks plan budget and execution overrun against 50% limit and budget redline (docs/06 §5.1, docs/09 R12, P8.m2e).
#[tauri::command]
async fn check_plan_budget_overrun_cmd(
    est_cost_usd: f64,
    current_actual_cost_usd: f64,
    plan_budget_usd: Option<f64>,
) -> Result<serde_json::Value, String> {
    let overrun_ratio = if est_cost_usd > 0.0 {
        (current_actual_cost_usd - est_cost_usd) / est_cost_usd
    } else {
        0.0
    };

    // Exceeds 50% estimate: pause warning
    let is_50pct_overrun = est_cost_usd > 0.0 && current_actual_cost_usd > (est_cost_usd * 1.50);

    // Hard redline (R12): exceed user-configured plan budget ceiling
    let is_redline_exceeded = match plan_budget_usd {
        Some(b) if b > 0.0 => current_actual_cost_usd >= b,
        _ => false,
    };

    let should_pause = is_50pct_overrun || is_redline_exceeded;

    Ok(serde_json::json!({
        "estCostUsd": est_cost_usd,
        "actualCostUsd": current_actual_cost_usd,
        "overrunRatio": overrun_ratio,
        "is50PctOverrun": is_50pct_overrun,
        "isRedlineExceeded": is_redline_exceeded,
        "shouldPauseQueue": should_pause,
        "message": if is_redline_exceeded {
            format!("已触发预算硬红线 (R12)：实际花费 ${:.2} 已超出预算上限 ${:.2}，编排队列自动暂停。", current_actual_cost_usd, plan_budget_usd.unwrap_or(0.0))
        } else if is_50pct_overrun {
            format!("实际花费 ${:.2} 已超出计划预估 ${:.2} 的 50%，编排队列自动暂停提示。", current_actual_cost_usd, est_cost_usd)
        } else {
            "预算处于正常可控区间。".to_string()
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
    dashboard::settle_plan_savings(&conn, plan_id, user_override.as_deref()).map_err(|e| e.to_string())
}

/// Lists recorded savings events (P8.m3a).
#[tauri::command]
async fn list_savings_events_cmd(
    range: Option<String>,
    storage_state: State<'_, storage::StorageState>,
) -> Result<Vec<storage::SavingsEventRow>, String> {
    let db = storage_state.get()?;
    db.list_savings_events(range)
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

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .manage(chat::TurnRegistry::default())
        .manage(Bus::new())
        .setup(|app| {
            forward_to_frontend(app.handle().clone(), &app.state::<Bus>());
            let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
            let run_dir = app_data.join("run");
            proc::set_run_dir(&run_dir);
            let reaped = proc::cleanup_leftovers(&run_dir);
            if !reaped.is_empty() {
                tracing::info!("Reaped {} leftover processes from previous session", reaped.len());
            }
            let db = storage::Storage::open(&app_data);
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
            get_builtin_knowledge_pack,
            check_knowledge_pack_update,
            get_pending_knowledge_update,
            apply_knowledge_pack_update,
            start_chat_turn,
            cancel_chat_turn,
            save_session_plan,
            tokage_dir_exists,
            ignore_tokage_dir,
            remove_tokage_dir,
            storage_load,
            storage_write,
            storage_import,
            get_command_policy,
            update_command_rules,
            classify_command,
            authorize_command,
            list_approvals,
            resolve_approval,
            list_usage_events,
            record_usage_event,
            get_proxy_config,
            save_proxy_config,
            get_system_proxy,
            test_proxy_connection,
            get_provider_proxy_status,
            get_all_provider_proxy_statuses,
            list_native_sessions,
            query_provider_quota,
            get_repo_digest,
            render_planning_prompt,
            parse_planning_output,
            validate_plan,
            persist_plan,
            persist_plan_user_edited,
            reset_plan_to_default,
            load_plan,
            save_pipeline_state_cmd,
            load_pipeline_state_cmd,
            compute_plan_diff,
            apply_plan_diff,
            assemble_replan_prompt,
            generate_adaptation_tasks,
            triage_request,
            allocate_plan_models,
            assign_direct_task,
            pick_dispatchable_tasks,
            assemble_task_brief_cmd,
            parse_or_generate_task_report_cmd,
            record_task_report,
            get_task_reports,
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
            update_main_session_status_cmd,
            record_task_outcome,
            list_task_outcomes,
            list_learned_stats,
            plan_in_window_context_cmd,
            apply_context_action_cmd,
            dismiss_context_action_cmd,
            evaluate_window_reuse_cmd,
            triage_main_session_message_cmd,
            build_compact_cmd,
            record_context_action_cmd,
            list_context_actions_cmd,
            settle_context_savings_cmd,
            trip_provider_melt_cmd,
            recover_provider_health_cmd,
            record_provider_failure_cmd,
            reset_provider_failures_cmd,
            list_provider_health_cmd,
            get_provider_utilization_now_cmd,
            reassign_unstarted_tasks_cmd,
            check_plan_budget_overrun_cmd,
            get_usage_dashboard,
            settle_plan_savings_cmd,
            list_savings_events_cmd,
            record_savings_event_cmd,
            get_chat_logs_cmd,
            search_chat_logs_cmd,
            cleanup_retention_records_cmd,
            export_project_data_cmd,
            clear_project_records_cmd
        ])


        .build(tauri::generate_context!())
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
