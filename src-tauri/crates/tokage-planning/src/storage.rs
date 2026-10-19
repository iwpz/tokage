//! Structured Plan persistence under `<cwd>/.tokage/plans/<plan_id>/` (docs/02 §6, docs/08 §2.3, P5.m3b).
//!
//! Layout:
//! ```text
//! .tokage/plans/<plan_id>/
//!   spec.md          Target, scope, decisions
//!   tasks.yaml       Structured YAML task graph
//!   contracts/
//!     C1.md          Contract definitions
//!     C2.md
//!   plan.md          Complete combined markdown for human reading / fallback
//! ```
//!
//! Also writes `.tokage/` into `.git/info/exclude` to prevent polluting git status without forcing .gitignore.

use std::{
    fs,
    io,
    path::Path,
};

use tokage_core::planning::Plan;

/// Result of persisting a structured plan.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistPlanResult {
    pub plan_id: String,
    pub plan_dir: String,
    pub files_written: Vec<String>,
}

/// Persists a structured plan into `<root>/.tokage/plans/<plan_id>/` and updates `.git/info/exclude`.
pub fn persist_plan(root: &Path, plan_id: &str, plan: &Plan) -> io::Result<PersistPlanResult> {
    let clean_id = sanitize_plan_id(plan_id);
    let plan_dir = root.join(".tokage").join("plans").join(&clean_id);
    fs::create_dir_all(&plan_dir)?;

    let mut files_written = Vec::new();

    // 1. spec.md
    let spec_path = plan_dir.join("spec.md");
    let spec_content = render_spec_markdown(plan);
    fs::write(&spec_path, spec_content)?;
    files_written.push("spec.md".to_string());

    // 2. tasks.yaml
    let tasks_path = plan_dir.join("tasks.yaml");
    let tasks_yaml = serde_yaml::to_string(&plan.tasks)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    fs::write(&tasks_path, tasks_yaml)?;
    files_written.push("tasks.yaml".to_string());

    // 3. contracts/
    if !plan.contracts.is_empty() {
        let contracts_dir = plan_dir.join("contracts");
        fs::create_dir_all(&contracts_dir)?;
        for contract in &plan.contracts {
            let filename = format!("{}.md", sanitize_plan_id(&contract.id));
            let file_path = contracts_dir.join(&filename);
            let content = format!("# {}\n\n{}\n", contract.title, contract.content);
            fs::write(&file_path, content)?;
            files_written.push(format!("contracts/{}", filename));
        }
    }

    // 4. Archive previous version if it exists and version is increasing (docs/02 §9)
    let combined_path = plan_dir.join("plan.md");
    if combined_path.is_file() {
        if let Ok(prev_content) = fs::read_to_string(&combined_path) {
            let history_dir = plan_dir.join("history");
            let _ = fs::create_dir_all(&history_dir);
            // Deduce previous version from file if possible, or count existing
            let history_count = fs::read_dir(&history_dir)
                .map(|entries| entries.filter_map(|e| e.ok()).count())
                .unwrap_or(0);
            let prev_version_num = if plan.version > 1 {
                plan.version - 1
            } else {
                (history_count as u32) + 1
            };
            let history_file = history_dir.join(format!("{}.md", prev_version_num));
            fs::write(&history_file, prev_content)?;
            let snapshot = combined_path.with_extension("json");
            if snapshot.is_file() { fs::copy(snapshot, history_file.with_extension("json"))?; }
        }
    }

    // Write updated plan.md (Complete combined human-readable document)
    // Structured edits (including routing and adaptation tasks) are authoritative.
    let rendered_markdown = render_full_plan_markdown(plan);
    fs::write(&combined_path, &rendered_markdown)?;
    write_plan_snapshot(&combined_path, plan, &rendered_markdown, false)?;
    files_written.extend(["plan.md".to_string(), "plan.json".to_string()]);
    // Keep the user's edited proposal for recovery, but do not let it shadow a confirmed revision.
    let edited = plan_dir.join("user_edited");
    if edited.exists() {
        let history = plan_dir.join("history");
        fs::create_dir_all(&history)?;
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .map_err(io::Error::other)?.as_nanos();
        fs::rename(edited, history.join(format!("user-edited-{stamp}")))?;
    }

    // 5. Ensure .git/info/exclude includes .tokage/
    let _ = ensure_git_info_exclude(root);

    Ok(PersistPlanResult {
        plan_id: clean_id,
        plan_dir: plan_dir.to_string_lossy().to_string(),
        files_written,
    })
}

/// Persists an updated plan into `<root>/.tokage/plans/<plan_id>/user_edited/`.
pub fn persist_plan_user_edited(root: &Path, plan_id: &str, plan: &Plan) -> io::Result<PersistPlanResult> {
    let clean_id = sanitize_plan_id(plan_id);
    let user_edited_dir = root.join(".tokage").join("plans").join(&clean_id).join("user_edited");
    fs::create_dir_all(&user_edited_dir)?;

    let mut files_written = Vec::new();

    // 1. spec.md
    let spec_path = user_edited_dir.join("spec.md");
    let spec_content = render_spec_markdown(plan);
    fs::write(&spec_path, spec_content)?;
    files_written.push("user_edited/spec.md".to_string());

    // 2. tasks.yaml
    let tasks_path = user_edited_dir.join("tasks.yaml");
    let tasks_yaml = serde_yaml::to_string(&plan.tasks)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    fs::write(&tasks_path, tasks_yaml)?;
    files_written.push("user_edited/tasks.yaml".to_string());

    // 3. contracts/
    if !plan.contracts.is_empty() {
        let contracts_dir = user_edited_dir.join("contracts");
        fs::create_dir_all(&contracts_dir)?;
        for contract in &plan.contracts {
            let filename = format!("{}.md", sanitize_plan_id(&contract.id));
            let file_path = contracts_dir.join(&filename);
            let content = format!("# {}\n\n{}\n", contract.title, contract.content);
            fs::write(&file_path, content)?;
            files_written.push(format!("user_edited/contracts/{}", filename));
        }
    }

    // 4. Combined plan.md (always render from current spec & tasks so changes are reflected)
    let combined_path = user_edited_dir.join("plan.md");
    let rendered_markdown = render_full_plan_markdown(plan);
    fs::write(&combined_path, &rendered_markdown)?;
    write_plan_snapshot(&combined_path, plan, &rendered_markdown, true)?;
    files_written.extend(["user_edited/plan.md".to_string(), "user_edited/plan.json".to_string()]);

    let _ = ensure_git_info_exclude(root);

    Ok(PersistPlanResult {
        plan_id: clean_id,
        plan_dir: user_edited_dir.to_string_lossy().to_string(),
        files_written,
    })
}

// Pair structured identity with the rendered document, so hand edits to plan.md remain visible.
fn write_plan_snapshot(path: &Path, plan: &Plan, markdown: &str, edited: bool) -> io::Result<()> {
    let mut snapshot = plan.clone();
    snapshot.raw_markdown = markdown.into();
    snapshot.is_user_edited = Some(edited);
    fs::write(path.with_extension("json"), serde_json::to_vec_pretty(&snapshot).map_err(io::Error::other)?)
}

fn load_plan_document(path: &Path, version: Option<u32>, edited: bool) -> io::Result<Plan> {
    let raw = fs::read_to_string(path)?;
    let snapshot = fs::read(path.with_extension("json")).ok()
        .and_then(|bytes| serde_json::from_slice::<Plan>(&bytes).ok());
    let mut plan = if let Some(saved) = snapshot.as_ref().filter(|p| p.raw_markdown == raw) {
        saved.clone()
    } else {
        crate::parser::parse_plan(&raw, &crate::parser::PlanParseOptions::default()).map_err(io::Error::other)?
    };
    plan.version = version.or_else(|| snapshot.map(|p| p.version)).unwrap_or(plan.version);
    plan.is_user_edited = Some(edited);
    Ok(plan)
}

/// Deletes the `user_edited` directory under `<root>/.tokage/plans/<plan_id>/` and returns the restored original plan.
pub fn reset_plan_to_default(root: &Path, plan_id: &str) -> io::Result<Plan> {
    let clean_id = sanitize_plan_id(plan_id);
    let plan_dir = root.join(".tokage").join("plans").join(&clean_id);
    let user_edited_dir = plan_dir.join("user_edited");
    if user_edited_dir.exists() {
        let _ = fs::remove_dir_all(&user_edited_dir);
    }
    load_persisted_plan_version(root, plan_id, None)
}

/// Loads a structured plan from `<root>/.tokage/plans/<plan_id>/`.
pub fn load_persisted_plan(root: &Path, plan_id: &str) -> io::Result<Plan> {
    load_persisted_plan_version(root, plan_id, None)
}

/// Loads a specific version of a plan (or current if None).
/// If version is None and a `user_edited` version exists, defaults to loading `user_edited` with `is_user_edited: Some(true)`.
pub fn load_persisted_plan_version(root: &Path, plan_id: &str, version: Option<u32>) -> io::Result<Plan> {
    let clean_id = sanitize_plan_id(plan_id);
    let plan_dir = root.join(".tokage").join("plans").join(&clean_id);
    if !plan_dir.is_dir() {
        // Fallback: check if single-file plan exists from save_session_plan (.tokage/plans/<clean_id>.md)
        let single_file = root.join(".tokage").join("plans").join(format!("{}.md", clean_id));
        if single_file.is_file() {
            let raw = fs::read_to_string(&single_file)?;
            let opts = crate::parser::PlanParseOptions::default();
            if let Ok(mut plan) = crate::parser::parse_plan(&raw, &opts) {
                if let Some(v) = version {
                    plan.version = v;
                }
                return Ok(plan);
            }
        }
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Plan directory or file not found: {}", plan_dir.display()),
        ));
    }

    // If no explicit historical version is requested, check if user_edited version exists
    if version.is_none() {
        let user_edited_dir = plan_dir.join("user_edited");
        if user_edited_dir.is_dir() {
            let user_plan_md = user_edited_dir.join("plan.md");
            if user_plan_md.is_file() {
                return load_plan_document(&user_plan_md, None, true);
            }
            let user_tasks_yaml = user_edited_dir.join("tasks.yaml");
            if user_tasks_yaml.is_file() {
                if let Ok(content) = fs::read_to_string(&user_tasks_yaml) {
                    if let Ok(tasks) = serde_yaml::from_str::<Vec<tokage_core::planning::TaskItem>>(&content) {
                        return Ok(Plan {
                            version: 1,
                            tasks,
                            is_user_edited: Some(true),
                            ..Default::default()
                        });
                    }
                }
            }
        }
    }

    let target_path = if let Some(v) = version {
        let history_file = plan_dir.join("history").join(format!("{}.md", v));
        if history_file.is_file() {
            history_file
        } else {
            plan_dir.join("plan.md")
        }
    } else {
        plan_dir.join("plan.md")
    };

    if target_path.is_file() {
        return load_plan_document(&target_path, version, false);
    }

    // Fallback: reconstruct from separate files
    let tasks_path = plan_dir.join("tasks.yaml");
    let tasks = if tasks_path.is_file() {
        let content = fs::read_to_string(&tasks_path)?;
        serde_yaml::from_str(&content)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
    } else {
        Vec::new()
    };

    Ok(Plan {
        version: version.unwrap_or(1),
        tasks,
        raw_markdown: String::new(),
        is_user_edited: Some(false),
        ..Default::default()
    })
}

/// Execution status of a pipeline session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PipelineStatus {
    Running,
    Paused,
    Done,
    Interrupted,
}

/// Integration worktree metadata (F11; optional in F7 schema).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationInfo {
    pub branch: String,
    pub root: String,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub base_commit: Option<String>,
    #[serde(default)]
    pub target: Option<tokage_native::git::CheckoutIdentity>,
}

/// Per-task worktree metadata (F11; optional in F7 schema).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskWorktree {
    pub root: String,
    pub cwd: String,
    pub branch: String,
    pub base_commit: String,
}

fn default_schema_version() -> u32 {
    1
}

/// Detailed runtime state of a task in the execution pipeline.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineTaskInfo {
    pub id: String,
    pub title: String,
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assigned_model: Option<String>,
    /// Effort / fast / tier flags that go with `assigned_model`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_params: Option<tokage_core::planning::ModelParams>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<u64>,
    /// CLI provider id (F7.b).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Latest CLI thread id from `thread` events (F6.a / F7.b).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    /// Task working directory (worktree or shared).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<TaskWorktree>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repair_step: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_commands: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub approvals: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}

/// Gate verification result recorded for a task.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineGateResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    pub task_id: String,
    pub command: String,
    pub passed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    #[serde(default)]
    pub output_tail: String,
}

/// Complete pipeline execution state schema persisted under `.tokage/execution/<sessionId>.json`.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineState {
    /// 2 for new writes; missing field deserializes as 1 (v1 files).
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    /// Monotonic snapshot sequence; zero is reserved for legacy files.
    #[serde(default)]
    pub persistence_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery: Option<serde_json::Value>,
    #[serde(default)]
    pub task_attempt_counts: std::collections::HashMap<String, u32>,
    pub session_id: String,
    pub status: PipelineStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_db_id: Option<i64>,
    #[serde(default)]
    pub task_db_ids: std::collections::HashMap<String, i64>,
    #[serde(default)]
    pub task_report_plan_ids: std::collections::HashMap<String, i64>,
    #[serde(default)]
    pub plan_definition: Option<String>,
    #[serde(default)]
    pub completion_receipts: std::collections::HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub recovery_tasks: Vec<String>,
    #[serde(default)]
    pub git_deliveries: std::collections::HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub task_failure_evidence: std::collections::HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub scope_baselines: std::collections::HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub scope_checks: std::collections::HashMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integration: Option<IntegrationInfo>,
    pub tasks: Vec<PipelineTaskInfo>,
    /// Full task definitions (v2). Empty on v1 files.
    #[serde(default)]
    pub task_items: Vec<tokage_core::planning::TaskItem>,
    #[serde(default)]
    pub contracts: Vec<tokage_core::planning::PlanContract>,
    /// taskId → UpstreamArtifact as JSON (avoid orchestration crate cycle).
    #[serde(default)]
    pub upstream_artifacts: std::collections::HashMap<String, serde_json::Value>,
    /// taskId → TaskAssignment snapshot as JSON.
    #[serde(default)]
    pub assignments: std::collections::HashMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deviation_sources: Vec<String>,
    #[serde(default)]
    pub task_states: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub task_logs: std::collections::HashMap<String, Vec<String>>,
    #[serde(default)]
    pub gate_results: Vec<PipelineGateResult>,
    pub started_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<u64>,
}

// All writers (including recovery) share this lock. The critical section includes
// checking the on-disk revision and replacing the file, so late IPC requests
// cannot overwrite a newer snapshot. Serializing across sessions is deliberate:
// pipeline files are small and this also avoids an unbounded lock registry.
static PIPELINE_WRITE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
static PIPELINE_TMP_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Atomic replacement with file sync and, on Unix, directory sync. A failed save
/// must be treated as uncertain (rename may have succeeded before directory sync).
fn pipeline_lease(root: &Path) -> io::Result<tokage_native::file_lease::FileLease> {
    let directory = root.join(".tokage/execution");
    fs::create_dir_all(&directory)?;
    tokage_native::file_lease::FileLease::acquire(&directory.join(".pipeline.lock"))
        .map_err(|e| io::Error::new(e.kind(), format!("Pipeline is owned by another writer: {e}")))
}

pub fn save_pipeline_state(root: &Path, state: &PipelineState) -> io::Result<()> {
    save_pipeline_state_cas(root, state, None)
}

pub fn save_pipeline_state_cas(root: &Path, state: &PipelineState, expected_revision: Option<u64>) -> io::Result<()> {
    let _guard = PIPELINE_WRITE_LOCK.lock().map_err(|_| io::Error::other("pipeline writer poisoned"))?;
    let _lease = pipeline_lease(root)?;
    save_pipeline_state_locked(root, state, expected_revision)
}

fn save_pipeline_state_locked(root: &Path, state: &PipelineState, expected_revision: Option<u64>) -> io::Result<()> {
    use std::io::Write;
    use std::sync::atomic::Ordering;
    let clean_id = sanitize_plan_id(&state.session_id);
    let exec_dir = root.join(".tokage").join("execution");
    fs::create_dir_all(&exec_dir)?;
    let target_file = exec_dir.join(format!("{}.json", clean_id));
    match fs::read(&target_file) {
        Ok(bytes) => {
            let current: PipelineState = serde_json::from_slice(&bytes)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            if let Some(expected) = expected_revision {
                let identical_retry = current.persistence_revision == state.persistence_revision
                    && serde_json::to_value(&current).map_err(io::Error::other)? == serde_json::to_value(state).map_err(io::Error::other)?;
                if current.persistence_revision != expected && !identical_retry {
                    return Err(io::Error::other("Pipeline base revision changed; reload before writing"));
                }
            }
            if current.session_id != state.session_id { return Err(io::Error::other("Pipeline session identity mismatch")); }
            if current.persistence_revision > state.persistence_revision {
                return Err(io::Error::other("stale pipeline snapshot rejected"));
            }
            if current.persistence_revision != 0
                && current.persistence_revision == state.persistence_revision
            {
                if serde_json::to_value(&current).map_err(io::Error::other)?
                    != serde_json::to_value(state).map_err(io::Error::other)?
                {
                    return Err(io::Error::other(
                        "pipeline revision already used by another snapshot",
                    ));
                }
                // Retry the replacement and sync after an uncertain acknowledgement.
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            if expected_revision.unwrap_or(0) != 0 { return Err(io::Error::other("Pipeline disappeared since last acknowledgement")); }
        }
        Err(e) => return Err(e),
    }
    let (tmp_file, mut file) = loop {
        let sequence = PIPELINE_TMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = exec_dir.join(format!(
            "{}.tmp.{}.{}",
            clean_id,
            std::process::id(),
            sequence
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => break (path, file),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    };
    let result: io::Result<()> = (|| {
        let json_bytes = serde_json::to_vec_pretty(state).map_err(io::Error::other)?;
        file.write_all(&json_bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp_file, &target_file)?;
        #[cfg(unix)]
        {
            fs::File::open(&exec_dir)?.sync_all()?;
            fs::File::open(root.join(".tokage"))?.sync_all()?;
            fs::File::open(root)?.sync_all()?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp_file);
    }
    result?;
    let _ = ensure_git_info_exclude(root);
    Ok(())
}

fn is_in_flight_task_state(state: &str) -> bool {
    matches!(state, "RUNNING" | "VERIFYING" | "REPAIRING")
}

/// Loads the execution pipeline state from `<root>/.tokage/execution/<sessionId>.json`.
/// Cold start / crash recovery: in-flight tasks (`RUNNING` / `VERIFYING` / `REPAIRING`)
/// become `INTERRUPTED` and the pipeline status becomes `INTERRUPTED` (F7.b / S5).
pub fn load_pipeline_state(root: &Path, session_id: &str) -> io::Result<Option<PipelineState>> {
    let clean_id = sanitize_plan_id(session_id);
    let file_path = root.join(".tokage").join("execution").join(format!("{}.json", clean_id));
    if !file_path.is_file() {
        return Ok(None);
    }

    let _guard = PIPELINE_WRITE_LOCK.lock().map_err(|_| io::Error::other("pipeline writer poisoned"))?;
    let _lease = pipeline_lease(root)?;
    let content = fs::read_to_string(&file_path)?;
    let mut state: PipelineState = serde_json::from_str(&content)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    if state.session_id != session_id { return Err(io::Error::other("Pipeline session identity mismatch")); }
    let mut modified = false;
    // Cold start recovery: rewrite in-flight tasks regardless of pipeline status,
    // but only flip pipeline status when it was still RUNNING.
    let was_running = state.status == PipelineStatus::Running;
    for t in &mut state.tasks {
        if is_in_flight_task_state(&t.state) {
            t.state = "INTERRUPTED".to_string();
            modified = true;
        }
    }
    for (task_id, st) in &mut state.task_states {
        if is_in_flight_task_state(st) {
            *st = "INTERRUPTED".to_string();
            if let Some(logs) = state.task_logs.get_mut(task_id) {
                // Encoded i18n message (see src/lib/logMsg.ts) so the UI translates it at render time.
                logs.push("\u{1}i18n:{\"k\":\"taskLog.interrupted\"}".to_string());
            }
            modified = true;
        }
    }
    if was_running && modified {
        state.status = PipelineStatus::Interrupted;
    }

    for (id, status) in &state.task_states {
        if status == "INTERRUPTED" && !state.recovery_tasks.contains(id) {
            state.recovery_tasks.push(id.clone());
            modified = true;
        }
    }
    if modified {
        state.persistence_revision = state.persistence_revision.checked_add(1)
            .ok_or_else(|| io::Error::other("pipeline revision exhausted"))?;
        save_pipeline_state_locked(root, &state, None)?;
    }

    Ok(Some(state))
}

/// Ensures `.tokage/` is added to `.git/info/exclude` (P5.m3b).
pub fn ensure_git_info_exclude(root: &Path) -> io::Result<()> {
    let resolved = std::process::Command::new("git").arg("-C").arg(root)
        .args(["rev-parse", "--path-format=absolute", "--git-path", "info/exclude"])
        .env_remove("GIT_DIR").env_remove("GIT_WORK_TREE").env_remove("GIT_COMMON_DIR")
        .output().ok().filter(|out| out.status.success());
    let exclude_path = resolved.map(|out| std::path::PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()))
        .unwrap_or_else(|| root.join(".git").join("info").join("exclude"));
    if !exclude_path.parent().map(|p| p.is_dir()).unwrap_or(false) {
        return Ok(());
    }

    let pattern = ".tokage/";
    let existing = fs::read_to_string(&exclude_path).unwrap_or_default();
    if !existing.lines().any(|l| l.trim() == pattern) {
        let mut new_content = existing;
        if !new_content.ends_with('\n') && !new_content.is_empty() {
            new_content.push('\n');
        }
        new_content.push_str(pattern);
        new_content.push('\n');
        fs::write(&exclude_path, new_content)?;
    }
    Ok(())
}

fn sanitize_plan_id(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    if cleaned.is_empty() {
        "plan".to_string()
    } else {
        cleaned
    }
}

/// Renders the complete, human-readable combined Markdown document according to docs/02/plan-format.md.
pub fn render_full_plan_markdown(plan: &Plan) -> String {
    let mut out = String::new();

    // 1. Spec
    out.push_str(&render_spec_markdown(plan));

    // 2. Contracts
    if !plan.contracts.is_empty() {
        out.push_str("# Contracts\n\n");
        for contract in &plan.contracts {
            out.push_str(&format!("## {} {}\n", contract.id, contract.title));
            out.push_str(&contract.content);
            out.push_str("\n\n");
        }
    }

    // 3. Tasks
    out.push_str("# Tasks\n```yaml\n");
    if let Ok(tasks_yaml) = serde_yaml::to_string(&plan.tasks) {
        out.push_str(tasks_yaml.trim());
        out.push('\n');
    }
    out.push_str("```\n\n");

    // 4. Notes
    if let Some(notes) = &plan.notes {
        if !notes.trim().is_empty() {
            out.push_str("# Notes\n");
            out.push_str(notes.trim());
            out.push('\n');
        }
    }

    out
}

/// Renders the `# Spec` Markdown section.
pub fn render_spec_markdown(plan: &Plan) -> String {
    let mut out = String::new();
    out.push_str("# Spec\n\n");
    if !plan.spec.goal.is_empty() {
        out.push_str("## Goals\n");
        out.push_str(&plan.spec.goal);
        out.push_str("\n\n");
    }
    if !plan.spec.in_scope.is_empty() || !plan.spec.out_of_scope.is_empty() {
        out.push_str("## Scope\n");
        for item in &plan.spec.in_scope {
            out.push_str(&format!("- In scope: {}\n", item));
        }
        for item in &plan.spec.out_of_scope {
            out.push_str(&format!("- Out of scope: {}\n", item));
        }
        out.push('\n');
    }
    if !plan.spec.decisions.is_empty() {
        out.push_str("## Key decisions\n");
        for item in &plan.spec.decisions {
            out.push_str(&format!("- {}\n", item));
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokage_core::planning::{PlanContract, PlanSpec, TaskItem};

    #[test]
    fn m2e_writer_process() {
        let Some(root) = std::env::var_os("TOKAGE_M2E_STORAGE_ROOT") else { return; };
        let root = Path::new(&root);
        let actor = std::env::var("TOKAGE_M2E_ACTOR").unwrap();
        fs::write(root.join(format!("{actor}.ready")), "ready").unwrap();
        while !root.join("start").exists() { std::thread::sleep(std::time::Duration::from_millis(5)); }
        let mut state = persistence_fixture(2);
        state.task_logs.insert("T1".into(), vec![actor.clone()]);
        fs::write(root.join(format!("{actor}.result")), if save_pipeline_state(root, &state).is_ok() { "ok" } else { "rejected" }).unwrap();
    }

    #[test]
    fn m2e_cross_process_revision_conflict_and_recovery_receipts() {
        let dir = tempdir().unwrap();
        save_pipeline_state(dir.path(), &persistence_fixture(1)).unwrap();
        let mut children = vec![];
        for actor in ["a", "b"] {
            children.push(std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "storage::tests::m2e_writer_process"])
                .env("TOKAGE_M2E_STORAGE_ROOT", dir.path()).env("TOKAGE_M2E_ACTOR", actor)
                .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap());
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !dir.path().join("a.ready").exists() || !dir.path().join("b.ready").exists() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        fs::write(dir.path().join("start"), "start").unwrap();
        for child in &mut children { assert!(child.wait().unwrap().success()); }
        let results: Vec<_> = ["a", "b"].iter().map(|a| fs::read_to_string(dir.path().join(format!("{a}.result"))).unwrap()).collect();
        assert_eq!(results.iter().filter(|r| *r == "ok").count(), 1);
        let mut state = load_pipeline_state(dir.path(), "reliability").unwrap().unwrap();
        let mut stale = state.clone(); stale.persistence_revision = 100;
        assert!(save_pipeline_state_cas(dir.path(), &stale, Some(1)).is_err());
        assert!(save_pipeline_state_cas(dir.path(), &state, Some(1)).is_ok()); // Lost ACK of identical snapshot.
        state.persistence_revision += 1;
        state.task_states.insert("T1".into(), "VERIFYING".into());
        state.completion_receipts.insert("T1".into(), serde_json::json!({"turnId":"once", "rawOutput":"receipt"}));
        state.delivery = Some(serde_json::json!({"intent":{"kind":"export", "patchPath":"pinned.patch"}}));
        save_pipeline_state(dir.path(), &state).unwrap();
        let recovered = load_pipeline_state(dir.path(), "reliability").unwrap().unwrap();
        assert!(recovered.recovery_tasks.contains(&"T1".to_string()));
        assert_eq!(recovered.completion_receipts, state.completion_receipts);
        assert_eq!(recovered.delivery, state.delivery);
        let twice = load_pipeline_state(dir.path(), "reliability").unwrap().unwrap();
        assert_eq!(twice.persistence_revision, recovered.persistence_revision);
    }

    #[test]
    fn m1f_acceptance_evidence_and_all_gate_outcomes_survive_roundtrip() {
        let mut state = persistence_fixture(1);
        state.delivery = Some(serde_json::json!({
            "phase": "ready", "acceptance": "failed", "fullGatePassed": false,
            "gateResults": [{"status": "passed"}, {"status": "error"}, {"status": "not_run"}],
            "evidence": null
        }));
        state.gate_results.push(PipelineGateResult {
            task_id: "T1".into(), command: "check".into(), passed: false,
            status: Some("cancelled".into()), exit_code: None, duration_ms: 0, output_tail: "cancelled".into(),
        });
        let temp = tempdir().unwrap();
        save_pipeline_state(temp.path(), &state).unwrap();
        let restored = load_pipeline_state(temp.path(), &state.session_id).unwrap().unwrap();
        assert_eq!(restored.delivery, state.delivery);
        assert_eq!(restored.gate_results[0].status.as_deref(), Some("cancelled"));
    }

    fn persistence_fixture(revision: u64) -> PipelineState {
        serde_json::from_value(serde_json::json!({
            "schemaVersion": 2, "persistenceRevision": revision,
            "delivery": {"phase": "merged"}, "taskAttemptCounts": {"T1": 2},
            "sessionId": "reliability", "status": "DONE", "tasks": [],
            "taskStates": {"T1": "DONE"}, "taskLogs": {},
            "gateResults": [], "startedAt": 1
        }))
        .unwrap()
    }

    #[test]
    fn pipeline_concurrent_writes_and_stale_revisions() {
        let dir = tempdir().unwrap();
        std::thread::scope(|scope| {
            for _ in 0..16 {
                let root = dir.path();
                scope.spawn(move || {
                    for _ in 0..10 {
                        save_pipeline_state(root, &persistence_fixture(0)).unwrap();
                    }
                });
            }
        });
        save_pipeline_state(dir.path(), &persistence_fixture(20)).unwrap();
        assert!(save_pipeline_state(dir.path(), &persistence_fixture(19)).is_err());
        save_pipeline_state(dir.path(), &persistence_fixture(20)).unwrap();
        let mut reused = persistence_fixture(20);
        reused.status = PipelineStatus::Paused;
        assert!(save_pipeline_state(dir.path(), &reused).is_err());
        let loaded = load_pipeline_state(dir.path(), "reliability")
            .unwrap()
            .unwrap();
        assert_eq!(loaded.persistence_revision, 20);
        assert_eq!(loaded.task_attempt_counts.get("T1"), Some(&2));
        assert_eq!(loaded.delivery.as_ref().unwrap()["phase"], "merged");
        assert_eq!(loaded.status, PipelineStatus::Done);
    }

    #[test]
    fn pipeline_write_failure_and_orphan_retry() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        save_pipeline_state(root, &persistence_fixture(1)).unwrap();
        let exec = root.join(".tokage/execution");
        let target = exec.join("reliability.json");
        let backup = exec.join("previous.json");
        fs::rename(&target, &backup).unwrap();
        fs::create_dir(&target).unwrap();
        assert!(save_pipeline_state(root, &persistence_fixture(2)).is_err());
        fs::remove_dir(&target).unwrap();
        fs::rename(&backup, &target).unwrap();
        assert_eq!(
            load_pipeline_state(root, "reliability")
                .unwrap()
                .unwrap()
                .persistence_revision,
            1
        );
        fs::write(exec.join("reliability.tmp.orphan"), b"partial JSON").unwrap();
        save_pipeline_state(root, &persistence_fixture(2)).unwrap();
        assert_eq!(
            load_pipeline_state(root, "reliability")
                .unwrap()
                .unwrap()
                .persistence_revision,
            2
        );
    }

    #[test]
    fn pipeline_crash_writer_child() {
        let Some(root) = std::env::var_os("TOKAGE_TEST_CRASH_WRITE_ROOT") else {
            return;
        };
        let mut state = persistence_fixture(2);
        state
            .task_logs
            .insert("T1".into(), vec!["x".repeat(64 * 1024 * 1024)]);
        save_pipeline_state(Path::new(&root), &state).unwrap();
    }

    #[test]
    fn pipeline_process_exit_during_save_recovers_complete_snapshot() {
        use std::time::{Duration, Instant};
        let dir = tempdir().unwrap();
        save_pipeline_state(dir.path(), &persistence_fixture(1)).unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "storage::tests::pipeline_crash_writer_child",
                "--nocapture",
            ])
            .env("TOKAGE_TEST_CRASH_WRITE_ROOT", dir.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut saw_temporary = false;
        while Instant::now() < deadline {
            saw_temporary = fs::read_dir(dir.path().join(".tokage/execution"))
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| entry.file_name().to_string_lossy().contains(".tmp."));
            if saw_temporary || child.try_wait().unwrap().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let _ = child.kill();
        child.wait().unwrap();
        assert!(
            saw_temporary,
            "child did not reach the temporary-file write"
        );
        let recovered = load_pipeline_state(dir.path(), "reliability")
            .unwrap()
            .unwrap();
        assert!(matches!(recovered.persistence_revision, 1 | 2));
        assert_eq!(recovered.task_states.get("T1").unwrap(), "DONE");
        save_pipeline_state(dir.path(), &persistence_fixture(3)).unwrap();
    }

    #[test]
    fn test_persist_and_load_plan() {
        let temp = tempdir().unwrap();
        let root = temp.path();

        let plan = Plan {
            version: 1,
            parent_plan_id: None,
            spec: PlanSpec {
                raw: "".into(),
                goal: "测试目标".into(),
                in_scope: vec!["模块 A".into()],
                out_of_scope: vec!["模块 B".into()],
                decisions: vec!["采用无状态方案".into()],
            },
            contracts: vec![PlanContract {
                id: "C1".into(),
                title: "用户 API".into(),
                content: "type User = { id: string };".into(),
            }],
            tasks: vec![TaskItem {
                id: "T1".into(),
                domain: "backend-api".into(),
                title: "编写 API".into(),
                goal: "目标 API".into(),
                ..Default::default()
            }],
            notes: Some("补充说明".into()),
            raw_markdown: "# Spec\n## 目标\n测试目标\n\n# Contracts\n## C1 用户 API\n```ts\ntype User = { id: string };\n```\n\n# Tasks\n```yaml\n- id: T1\n  domain: backend-api\n  title: 编写 API\n  goal: 目标 API\n```\n".into(),
            ..Default::default()
        };

        let res = persist_plan(root, "plan-123", &plan).expect("should persist plan");
        assert_eq!(res.plan_id, "plan-123");
        assert!(res.files_written.contains(&"spec.md".to_string()));
        assert!(res.files_written.contains(&"tasks.yaml".to_string()));
        assert!(res.files_written.contains(&"contracts/C1.md".to_string()));
        assert!(res.files_written.contains(&"plan.md".to_string()));

        let loaded = load_persisted_plan(root, "plan-123").expect("should load plan");
        assert_eq!(loaded.spec.goal, "测试目标");
        assert_eq!(loaded.contracts.len(), 1);
        assert_eq!(loaded.tasks.len(), 1);
        assert_eq!(loaded.tasks[0].id, "T1");
    }

    #[test]
    fn m2c_plan_revision_survives_stale_markdown_user_edits_and_reload() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        let mut plan = Plan { version: 1, spec: PlanSpec { goal: "initial".into(), ..Default::default() },
            tasks: vec![TaskItem { id: "T1".into(), goal: "initial".into(), ..Default::default() }], ..Default::default() };
        persist_plan(root, "revision", &plan).unwrap();
        plan = load_persisted_plan(root, "revision").unwrap();
        let old_markdown = plan.raw_markdown.clone();
        plan.tasks[0].goal = "human edit".into();
        persist_plan_user_edited(root, "revision", &plan).unwrap();
        assert_eq!(load_persisted_plan(root, "revision").unwrap().tasks[0].goal, "human edit");
        plan.version = 2;
        plan.parent_plan_id = Some("1".into());
        plan.tasks[0].goal = "latest requirement".into();
        plan.tasks[0].assigned_model = Some("OPENCODE:chosen".into());
        plan.raw_markdown = old_markdown; // stale raw model response must never override structured edits.
        persist_plan(root, "revision", &plan).unwrap();
        let loaded = load_persisted_plan(root, "revision").unwrap();
        assert_eq!(loaded.version, 2);
        assert_eq!(loaded.parent_plan_id.as_deref(), Some("1"));
        assert_eq!(loaded.tasks, plan.tasks);
        assert_eq!(loaded.spec, plan.spec);
        assert_eq!(loaded.is_user_edited, Some(false));
        let history = root.join(".tokage/plans/revision/history");
        assert!(fs::read_dir(&history).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().starts_with("user-edited-")));
        assert_eq!(load_persisted_plan_version(root, "revision", Some(1)).unwrap().tasks[0].goal, "initial");
        // Manual edits to the human-readable document still take precedence over its snapshot.
        let path = root.join(".tokage/plans/revision/plan.md");
        fs::write(&path, fs::read_to_string(&path).unwrap().replace("latest requirement", "manual change")).unwrap();
        let manual = load_persisted_plan(root, "revision").unwrap();
        assert_eq!(manual.tasks[0].goal, "manual change");
        assert_eq!(manual.version, 2);
    }

    #[test]
    fn m2c_legacy_integration_metadata_does_not_invent_a_patch_baseline() {
        let old: IntegrationInfo = serde_json::from_str(r#"{"root":"/old","branch":"tokage/old"}"#).unwrap();
        assert!(old.base_commit.is_none());
        assert!(old.target.is_none());
        assert!(old.cwd.is_none());
    }

    #[test]
    fn m2c_git_exclude_works_for_subdirectories_and_linked_worktrees() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("repo");
        fs::create_dir_all(root.join("packages/app")).unwrap();
        let git = |cwd: &Path, args: &[&str]| {
            let out = std::process::Command::new("git").arg("-C").arg(cwd).args(args).output().unwrap();
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
            String::from_utf8_lossy(&out.stdout).to_string()
        };
        git(&root, &["init", "-q"]);
        git(&root, &["-c", "user.name=fixture", "-c", "user.email=t@t", "commit", "--allow-empty", "-qm", "base"]);
        ensure_git_info_exclude(&root.join("packages/app")).unwrap();
        fs::create_dir_all(root.join("packages/app/.tokage")).unwrap();
        fs::write(root.join("packages/app/.tokage/state"), "metadata").unwrap();
        assert!(git(&root, &["status", "--porcelain"]).trim().is_empty());
        let linked = temp.path().join("linked");
        git(&root, &["worktree", "add", "-b", "tokage/exclude", linked.to_str().unwrap()]);
        ensure_git_info_exclude(&linked).unwrap();
        fs::create_dir_all(linked.join(".tokage")).unwrap();
        fs::write(linked.join(".tokage/state"), "metadata").unwrap();
        assert!(git(&linked, &["status", "--porcelain"]).trim().is_empty());
    }

    #[test]
    fn test_git_info_exclude_updated() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        let git_info = root.join(".git").join("info");
        fs::create_dir_all(&git_info).unwrap();

        ensure_git_info_exclude(root).unwrap();

        let content = fs::read_to_string(git_info.join("exclude")).unwrap();
        assert!(content.contains(".tokage/"));

        // Idempotency
        ensure_git_info_exclude(root).unwrap();
        let count = fs::read_to_string(git_info.join("exclude"))
            .unwrap()
            .lines()
            .filter(|l| l.trim() == ".tokage/")
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_plan_versioning_and_history_archiving() {
        let temp = tempdir().unwrap();
        let root = temp.path();

        let plan_v1 = Plan {
            version: 1,
            parent_plan_id: None,
            spec: PlanSpec {
                raw: "".into(),
                goal: "初始版本目标".into(),
                in_scope: vec!["A".into()],
                out_of_scope: vec![],
                decisions: vec![],
            },
            contracts: vec![],
            tasks: vec![TaskItem {
                id: "T1".into(),
                title: "任务 1".into(),
                ..Default::default()
            }],
            notes: None,
            raw_markdown: "".into(),
            ..Default::default()
        };

        // 1. Save version 1
        persist_plan(root, "p-100", &plan_v1).unwrap();

        // 2. Update to version 2
        let mut plan_v2 = plan_v1.clone();
        plan_v2.version = 2;
        plan_v2.spec.goal = "修订版本目标".into();
        persist_plan(root, "p-100", &plan_v2).unwrap();

        // Check current plan is version 2
        let current = load_persisted_plan(root, "p-100").unwrap();
        assert_eq!(current.spec.goal, "修订版本目标");

        // Check history archived version 1
        let history_file = root.join(".tokage/plans/p-100/history/1.md");
        assert!(history_file.is_file());

        let loaded_v1 = load_persisted_plan_version(root, "p-100", Some(1)).unwrap();
        assert_eq!(loaded_v1.spec.goal, "初始版本目标");
        assert_eq!(loaded_v1.version, 1);
    }

    #[test]
    fn test_user_edited_plan_lifecycle() {
        let temp = tempdir().unwrap();
        let root = temp.path();

        let initial_plan = Plan {
            version: 1,
            parent_plan_id: None,
            spec: PlanSpec {
                raw: "".into(),
                goal: "初始设计稿目标".into(),
                in_scope: vec!["原有功能".into()],
                out_of_scope: vec![],
                decisions: vec![],
            },
            contracts: vec![],
            tasks: vec![TaskItem {
                id: "T1".into(),
                title: "任务 1".into(),
                assigned_model: None,
                model_params: None,
                ..Default::default()
            }],
            notes: None,
            raw_markdown: "".into(),
            is_user_edited: Some(false),
        };

        // 1. Initial persistence
        persist_plan(root, "plan-user-test", &initial_plan).unwrap();
        let loaded_initial = load_persisted_plan(root, "plan-user-test").unwrap();
        assert_eq!(loaded_initial.spec.goal, "初始设计稿目标");
        assert_eq!(loaded_initial.is_user_edited, Some(false));

        // 2. User edits plan (spec edited & model manually chosen)
        let mut user_plan = loaded_initial.clone();
        user_plan.spec.goal = "用户修改后的目标".into();
        user_plan.tasks[0].assigned_model = Some("CLAUDE_CODE:opus".into());
        persist_plan_user_edited(root, "plan-user-test", &user_plan).unwrap();

        // 3. Loading defaults to user_edited version
        let loaded_edited = load_persisted_plan(root, "plan-user-test").unwrap();
        assert_eq!(loaded_edited.spec.goal, "用户修改后的目标");
        assert_eq!(loaded_edited.tasks[0].assigned_model, Some("CLAUDE_CODE:opus".into()));
        assert_eq!(loaded_edited.is_user_edited, Some(true));

        // 4. Reset to default removes user_edited and restores initial version
        let restored = reset_plan_to_default(root, "plan-user-test").unwrap();
        assert_eq!(restored.spec.goal, "初始设计稿目标");
        assert_eq!(restored.tasks[0].assigned_model, None);
        assert_eq!(restored.is_user_edited, Some(false));
    }

    #[test]
    fn test_save_and_load_pipeline_state() {
        let temp = tempdir().unwrap();
        let root = temp.path();

        let mut task_states = std::collections::HashMap::new();
        task_states.insert("T1".to_string(), "DONE".to_string());
        task_states.insert("T2".to_string(), "RUNNING".to_string());

        let mut task_logs = std::collections::HashMap::new();
        task_logs.insert("T1".to_string(), vec!["[日志] 完成代码写入".to_string()]);
        task_logs.insert("T2".to_string(), vec!["[日志] 正在执行测试...".to_string()]);

        let state = PipelineState {
            schema_version: 2,
            persistence_revision: 0,
            delivery: None,
            task_attempt_counts: Default::default(),
            session_id: "session-abc-123".to_string(),
            status: PipelineStatus::Running,
            plan_db_id: None,
            task_db_ids: Default::default(),
            plan_definition: Some("plan-definition".into()),
            task_report_plan_ids: Default::default(),
            completion_receipts: Default::default(),
            recovery_tasks: Default::default(),
            git_deliveries: Default::default(),
            task_failure_evidence: Default::default(),
            scope_baselines: Default::default(),
            scope_checks: Default::default(),
            base_cwd: Some("/tmp/repo".into()),
            integration: None,
            tasks: vec![
                PipelineTaskInfo {
                    id: "T1".to_string(),
                    title: "初始化项目".to_string(),
                    state: "DONE".to_string(),
                    assigned_model: Some("claude-3-7-sonnet".to_string()),
                    model_params: None,
                    turn_id: Some("turn-1".to_string()),
                    started_at: Some(1000),
                    ended_at: Some(2000),
                    ..Default::default()
                },
                PipelineTaskInfo {
                    id: "T2".to_string(),
                    title: "编写功能并验证".to_string(),
                    state: "RUNNING".to_string(),
                    assigned_model: Some("codex-cli".to_string()),
                    model_params: None,
                    turn_id: Some("turn-2".to_string()),
                    started_at: Some(2050),
                    ended_at: None,
                    thread_id: Some("cli-thread-2".into()),
                    cwd: Some("/tmp/repo/.tokage/wt-t2".into()),
                    ..Default::default()
                },
            ],
            task_items: vec![],
            contracts: vec![],
            upstream_artifacts: Default::default(),
            assignments: Default::default(),
            deviation_sources: vec![],
            task_states,
            task_logs,
            gate_results: vec![
                PipelineGateResult {
                    status: None,
                    task_id: "T1".to_string(),
                    command: "cargo check".to_string(),
                    passed: true,
                    exit_code: Some(0),
                    duration_ms: 120,
                    output_tail: "Finished dev target".to_string(),
                },
            ],
            started_at: 1000,
            updated_at: Some(2050),
        };

        save_pipeline_state(root, &state).expect("save pipeline state");

        // Verify JSON file exists
        let json_file = root.join(".tokage/execution/session-abc-123.json");
        assert!(json_file.is_file());

        // Cold load: since state was RUNNING, cold load recovers T2 from RUNNING -> INTERRUPTED
        let loaded = load_pipeline_state(root, "session-abc-123")
            .expect("load pipeline state")
            .expect("some state");

        assert_eq!(loaded.session_id, "session-abc-123");
        assert_eq!(loaded.status, PipelineStatus::Interrupted);
        assert_eq!(loaded.tasks[0].state, "DONE");
        assert_eq!(loaded.tasks[1].state, "INTERRUPTED");
        assert_eq!(loaded.task_states.get("T2"), Some(&"INTERRUPTED".to_string()));
        assert_eq!(loaded.gate_results.len(), 1);
        assert!(loaded.task_logs.get("T2").unwrap().iter().any(|l| l.contains("taskLog.interrupted")));
        assert_eq!(loaded.tasks[1].thread_id.as_deref(), Some("cli-thread-2"));
        assert_eq!(loaded.tasks[1].cwd.as_deref(), Some("/tmp/repo/.tokage/wt-t2"));
    }

    /// F7.b: v1 JSON (no schemaVersion / taskItems) loads with schema_version == 1.
    #[test]
    fn test_pipeline_v1_fixture_loads_as_schema_1() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        let exec_dir = root.join(".tokage").join("execution");
        fs::create_dir_all(&exec_dir).unwrap();
        let v1 = r#"{
          "sessionId": "v1-session",
          "status": "PAUSED",
          "tasks": [{ "id": "T1", "title": "Old", "state": "DONE" }],
          "taskStates": { "T1": "DONE" },
          "taskLogs": { "T1": ["ok"] },
          "gateResults": [],
          "startedAt": 42
        }"#;
        fs::write(exec_dir.join("v1-session.json"), v1).unwrap();

        let loaded = load_pipeline_state(root, "v1-session")
            .unwrap()
            .expect("state");
        assert_eq!(loaded.schema_version, 1);
        assert!(loaded.task_items.is_empty());
        assert_eq!(loaded.status, PipelineStatus::Paused);
        assert_eq!(loaded.tasks[0].id, "T1");
    }

    /// F7.b: v2 round-trip keeps taskItems, contracts, artifacts, per-task fields.
    #[test]
    fn test_pipeline_v2_round_trip_preserves_fields() {
        use tokage_core::planning::{PlanContract, TaskAcceptance, TaskItem, TaskSize};

        let temp = tempdir().unwrap();
        let root = temp.path();

        let mut upstream = std::collections::HashMap::new();
        upstream.insert(
            "T1".into(),
            serde_json::json!({
                "taskId": "T1",
                "title": "初始化",
                "report": "done",
                "changedFiles": ["src/a.ts"]
            }),
        );

        let state = PipelineState {
            schema_version: 2,
            persistence_revision: 0,
            delivery: None,
            task_attempt_counts: Default::default(),
            session_id: "v2-session".into(),
            status: PipelineStatus::Paused,
            plan_db_id: Some(12),
            task_db_ids: std::collections::HashMap::from([("T2".into(), 120)]),
            plan_definition: Some("plan-definition".into()),
            task_report_plan_ids: std::collections::HashMap::from([("T1".into(), 11)]),
            completion_receipts: Default::default(),
            recovery_tasks: Default::default(),
            git_deliveries: std::collections::HashMap::from([("T2".into(), serde_json::json!({"phase":"merged","sourceCommit":"task-sha","worktree":{"root":"/wt/task","branch":"tokage/task"},"scope":{"snapshot":{"head":"baseline"}}}))]),
            task_failure_evidence: std::collections::HashMap::from([("T2".into(), serde_json::json!({"turnId":"attempt-2","workdir":"/wt/task","outOfScopeFiles":["outside"]}))]),
            scope_baselines: std::collections::HashMap::from([("T2".into(), serde_json::json!({"head":"baseline","files":{"src/a":{"disk":"hash","index":"blob"}}}))]),
            scope_checks: std::collections::HashMap::from([("T2".into(), serde_json::json!({"changedFiles":["src/a"],"checkedAt":123}))]),
            base_cwd: Some("/proj".into()),
            integration: Some(IntegrationInfo {
                branch: "tokage/int".into(),
                root: "/wt/int".into(),
                cwd: Some("/wt/int/packages/app".into()),
                base_commit: Some("baseline-sha".into()),
                target: Some(tokage_native::git::CheckoutIdentity { head: "baseline-sha".into(), reference: "refs/heads/main".into() })
            }),
            tasks: vec![PipelineTaskInfo {
                id: "T2".into(),
                title: "中间任务".into(),
                state: "READY".into(),
                assigned_model: Some("CLAUDE_CODE:sonnet".into()),
                model_params: None,
                provider: Some("CLAUDE_CODE".into()),
                thread_id: Some("thr-2".into()),
                cwd: Some("/proj/wt-t2".into()),
                worktree: Some(TaskWorktree {
                    root: "/proj/wt-t2".into(),
                    cwd: "/proj/wt-t2".into(),
                    branch: "tokage/t2".into(),
                    base_commit: "abc".into(),
                }),
                allowed_commands: vec!["npm test".into()],
                approvals: vec![serde_json::json!({"id": "apr_1", "command": "rm -rf build"})],
                cost_usd: Some(0.12),
                ..Default::default()
            }],
            task_items: vec![TaskItem {
                id: "T2".into(),
                domain: "backend".into(),
                title: "中间任务".into(),
                goal: "实现逻辑".into(),
                depends_on: vec!["T1".into()],
                contracts: vec!["C1".into()],
                write_scope: vec!["src/b.ts".into()],
                deliverables: vec!["src/b.ts".into()],
                acceptance: TaskAcceptance {
                    commands: vec!["npm test".into()],
                    criteria: vec!["通过".into()],
                    confirmed_commands: vec![],
                },
                size: TaskSize::M,
                difficulty: 2,
                assigned_model: Some("CLAUDE_CODE:sonnet".into()),
                model_params: None,
            }],
            contracts: vec![PlanContract {
                id: "C1".into(),
                title: "契约".into(),
                content: "必须".into(),
            }],
            upstream_artifacts: upstream,
            assignments: Default::default(),
            deviation_sources: vec!["T1".into()],
            task_states: {
                let mut m = std::collections::HashMap::new();
                m.insert("T2".into(), "READY".into());
                m
            },
            task_logs: Default::default(),
            gate_results: vec![],
            started_at: 100,
            updated_at: Some(200),
        };

        save_pipeline_state(root, &state).unwrap();
        let loaded = load_pipeline_state(root, "v2-session").unwrap().unwrap();
        assert_eq!(loaded.schema_version, 2);
        assert_eq!(loaded.plan_db_id, Some(12));
        assert_eq!(loaded.task_db_ids.get("T2"), Some(&120));
        assert_eq!(loaded.task_report_plan_ids.get("T1"), Some(&11));
        assert_eq!(loaded.git_deliveries["T2"]["phase"], "merged");
        assert_eq!(loaded.task_failure_evidence["T2"]["turnId"], "attempt-2");
        assert_eq!(loaded.git_deliveries["T2"]["sourceCommit"], "task-sha");
        assert_eq!(loaded.scope_baselines["T2"]["head"], "baseline");
        assert_eq!(loaded.scope_checks["T2"]["changedFiles"][0], "src/a");
        assert_eq!(loaded.base_cwd.as_deref(), Some("/proj"));
        assert_eq!(loaded.integration, state.integration);
        assert_eq!(loaded.task_items.len(), 1);
        assert_eq!(loaded.task_items[0].depends_on, vec!["T1".to_string()]);
        assert_eq!(loaded.task_items[0].goal, "实现逻辑");
        assert_eq!(loaded.contracts.len(), 1);
        assert!(loaded.upstream_artifacts.contains_key("T1"));
        assert_eq!(loaded.deviation_sources, vec!["T1".to_string()]);
        assert_eq!(loaded.tasks[0].thread_id.as_deref(), Some("thr-2"));
        assert_eq!(loaded.tasks[0].cwd.as_deref(), Some("/proj/wt-t2"));
        assert_eq!(loaded.tasks[0].allowed_commands, vec!["npm test".to_string()]);
        assert_eq!(loaded.tasks[0].cost_usd, Some(0.12));
        assert_eq!(loaded.status, PipelineStatus::Paused);
    }

    /// F7.b: REPAIRING in-flight tasks become INTERRUPTED on load.
    #[test]
    fn test_pipeline_repairing_marked_interrupted_on_load() {
        let temp = tempdir().unwrap();
        let root = temp.path();

        let mut task_states = std::collections::HashMap::new();
        task_states.insert("T1".into(), "REPAIRING".into());
        let mut task_logs = std::collections::HashMap::new();
        task_logs.insert("T1".into(), vec!["[修复] 进行中".into()]);

        let state = PipelineState {
            schema_version: 2,
            persistence_revision: 0,
            delivery: None,
            task_attempt_counts: Default::default(),
            session_id: "repair-session".into(),
            status: PipelineStatus::Running,
            plan_db_id: None,
            task_db_ids: Default::default(),
            plan_definition: Some("plan-definition".into()),
            task_report_plan_ids: Default::default(),
            completion_receipts: Default::default(),
            recovery_tasks: Default::default(),
            git_deliveries: Default::default(),
            task_failure_evidence: Default::default(),
            scope_baselines: Default::default(),
            scope_checks: Default::default(),
            base_cwd: None,
            integration: None,
            tasks: vec![PipelineTaskInfo {
                id: "T1".into(),
                title: "修复中".into(),
                state: "REPAIRING".into(),
                ..Default::default()
            }],
            task_items: vec![],
            contracts: vec![],
            upstream_artifacts: Default::default(),
            assignments: Default::default(),
            deviation_sources: vec![],
            task_states,
            task_logs,
            gate_results: vec![],
            started_at: 1,
            updated_at: None,
        };
        save_pipeline_state(root, &state).unwrap();
        let loaded = load_pipeline_state(root, "repair-session").unwrap().unwrap();
        assert_eq!(loaded.status, PipelineStatus::Interrupted);
        assert_eq!(loaded.tasks[0].state, "INTERRUPTED");
        assert_eq!(loaded.task_states.get("T1"), Some(&"INTERRUPTED".to_string()));
    }
}
