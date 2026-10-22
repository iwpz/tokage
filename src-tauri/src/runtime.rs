//! Tauri is a transport; the runtime map belongs to the application, never a WebView listener.
use std::{collections::HashMap, path::PathBuf};
use tauri::{AppHandle, Manager, State};
use tokage_runtime::{
    host::{self, NativeRuntime},
    repository::Reader,
    turn::TranscriptEvent,
    Event, Receipt, Request, Snapshot,
};
#[derive(Default)]
pub struct RuntimeState {
    owners: tokio::sync::Mutex<HashMap<String, NativeRuntime>>,
}
fn directory(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("runtime-v1"))
}
#[tauri::command]
pub async fn runtime_submit(
    app: AppHandle,
    state: State<'_, RuntimeState>,
    native: State<'_, crate::AppState>,
    request: Request,
) -> Result<Receipt, String> {
    let dir = directory(&app)?;
    // No implicit ResumeRun on reconnect or app restart. Commands alone can acquire ownership.
    let runtime = {
        let mut owners = state.owners.lock().await;
        if let Some(rt) = owners.get(&request.run_id) {
            rt.clone()
        } else {
            let rt =
                host::open(&dir, &request.run_id, native.env().await).map_err(|e| e.to_string())?;
            owners.insert(request.run_id.clone(), rt.clone());
            rt
        }
    };
    runtime.submit(&request).await.map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn runtime_snapshot(app: AppHandle, run_id: String) -> Result<Option<Snapshot>, String> {
    let dir = directory(&app)?;
    if !dir.join("tokage.db").exists() {
        return Ok(None);
    }
    Reader::open(&dir)
        .and_then(|mut r| r.snapshot(&run_id))
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn runtime_list(app: AppHandle) -> Result<Vec<Snapshot>, String> {
    let dir = directory(&app)?;
    if !dir.join("tokage.db").exists() {
        return Ok(vec![]);
    }
    Reader::open(&dir)
        .and_then(|mut r| r.list())
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn runtime_events(
    app: AppHandle,
    run_id: String,
    after: u64,
    limit: u32,
) -> Result<Vec<Event>, String> {
    Reader::open(&directory(&app)?)
        .and_then(|mut r| r.events(&run_id, after, limit))
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn runtime_transcript(
    app: AppHandle,
    run_id: String,
    turn_id: String,
    after: u64,
    limit: u32,
) -> Result<Vec<TranscriptEvent>, String> {
    Reader::open(&directory(&app)?)
        .and_then(|mut r| r.transcript(&run_id, &turn_id, after, limit))
        .map_err(|e| e.to_string())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DraftOptions {
    workspace: String,
    models: std::collections::BTreeMap<String, tokage_runtime::ModelAssignment>,
    trust_workspace: bool,
    permission_mode: String,
    concurrency: u32,
    provider_concurrency: std::collections::BTreeMap<String, u32>,
    budget_usd: Option<f64>,
    block_unknown_cost: bool,
    timeout_seconds: u64,
    repair_policy: String,
    context_policy: String,
    co_author: bool,
    gate_env_allow: Vec<String>,
}
/// Snapshot normal desktop settings and knowledge inputs. This is a query, never consent.
#[tauri::command]
pub async fn runtime_config(
    native: State<'_, crate::AppState>,
    db: State<'_, tokage_storage::StorageState>,
    options: DraftOptions,
) -> Result<tokage_runtime::EffectiveRunConfig, String> {
    use tokage_runtime::{
        digest,
        planning::{LearnedObservation, PlanningPolicy},
    };
    let cwd = PathBuf::from(&options.workspace)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let target = tokage_native::git::checkout_identity(&native.env().await, &cwd).await?;
    let policy = {
        let conn = db.get()?.read_conn()?;
        let overrides = tokage_storage::knowledge::list_overrides(&conn)
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|r| {
                Ok(tokage_matching::ModelOverride {
                    provider_name: r.provider_name,
                    model_id: r.model_id,
                    patch: serde_json::from_str(&r.patch_json).map_err(|e| e.to_string())?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let customs = tokage_storage::knowledge::list_custom_packs(&conn)
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter(|r| r.enabled)
            .map(|r| {
                Ok((
                    r.name,
                    tokage_matching::parse_knowledge_pack(&r.pack_json)
                        .map_err(|e| e.to_string())?,
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut learned: std::collections::BTreeMap<
            (String, String, String, String),
            LearnedObservation,
        > = Default::default();
        for r in
            tokage_storage::learning::list_all_learned_stats(&conn).map_err(|e| e.to_string())?
        {
            let row = learned
                .entry((
                    r.provider_name.clone(),
                    r.model_id.clone(),
                    r.domain.clone(),
                    r.bucket.clone(),
                ))
                .or_insert(LearnedObservation {
                    provider: r.provider_name,
                    model: r.model_id,
                    domain: r.domain,
                    bucket: r.bucket,
                    alpha: 1.0,
                    beta: 1.0,
                    samples: 0,
                    fix_rate: None,
                });
            match r.metric.as_str() {
                "success_alpha" => row.alpha = r.value,
                "success_beta" => row.beta = r.value,
                "fix_rate" => row.fix_rate = Some(r.value),
                _ => {}
            }
            row.samples = row.samples.max(r.samples);
        }
        PlanningPolicy {
            official: None,
            customs,
            overrides,
            learned: learned.into_values().collect(),
            learned_usage_profiles: None,
        }
    };
    let config = tokage_runtime::EffectiveRunConfig {
        source: "app-settings-v1".into(),
        policy_version: "runtime-v1".into(),
        workspace: cwd.to_string_lossy().into(),
        target_identity: target.head,
        target_reference: Some(target.reference),
        scope: vec!["**".into()],
        models: options.models,
        command_policy_hash: digest(&(
            &tokage_matching::builtin_knowledge_pack().command_policy,
            &policy,
        ))
        .map_err(|e| e.to_string())?,
        trust_workspace: options.trust_workspace,
        permission_mode: options.permission_mode,
        concurrency: options.concurrency,
        provider_concurrency: options.provider_concurrency,
        budget_usd: options.budget_usd,
        block_unknown_cost: options.block_unknown_cost,
        timeout_seconds: options.timeout_seconds,
        repair_policy: options.repair_policy,
        context_policy: options.context_policy,
        gate_env_allow: options.gate_env_allow,
        confirmed_commands: vec![],
        custom_pack_hashes: policy
            .customs
            .iter()
            .map(|(_, p)| digest(p))
            .collect::<tokage_runtime::Result<Vec<_>>>()
            .map_err(|e| e.to_string())?,
        co_author: options.co_author,
        planning_policy: Some(policy),
    };
    config.validate().map_err(|e| e.to_string())?;
    Ok(config)
}

pub fn monitor(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let dir = match directory(&app) {
                Ok(d) => d,
                Err(_) => return,
            };
            let owners: Vec<_> = app
                .state::<RuntimeState>()
                .owners
                .lock()
                .await
                .values()
                .cloned()
                .collect();
            for runtime in owners {
                let _ = host::poll_cancellation(&runtime, &dir).await;
            }
            tokio::time::sleep(std::time::Duration::from_millis(350)).await;
        }
    });
}
#[tauri::command]
pub async fn runtime_worker_error(
    state: State<'_, RuntimeState>,
    run_id: String,
) -> Result<Option<String>, String> {
    Ok(state
        .owners
        .lock()
        .await
        .get(&run_id)
        .and_then(|r| r.worker_error())
        .map(|e| e.to_string()))
}
#[tauri::command]
pub async fn runtime_inspect_task(
    app: AppHandle,
    state: State<'_, RuntimeState>,
    native: State<'_, crate::AppState>,
    run_id: String,
    task_id: String,
) -> Result<tokage_runtime::manual::Inspection, String> {
    let runtime = {
        let mut owners = state.owners.lock().await;
        if let Some(r) = owners.get(&run_id) {
            r.clone()
        } else {
            let r = host::open(&directory(&app)?, &run_id, native.env().await)
                .map_err(|e| e.to_string())?;
            owners.insert(run_id.clone(), r.clone());
            r
        }
    };
    runtime
        .inspect_task(&task_id)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub fn runtime_render_plan(plan: tokage_core::planning::Plan) -> String {
    tokage_planning::storage::render_full_plan_markdown(&plan)
}
