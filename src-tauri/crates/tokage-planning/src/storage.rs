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
            let _ = fs::write(history_file, prev_content);
        }
    }

    // Write updated plan.md (Complete combined human-readable document)
    let rendered_markdown = if plan.raw_markdown.trim().is_empty() {
        render_full_plan_markdown(plan)
    } else {
        plan.raw_markdown.clone()
    };
    fs::write(&combined_path, &rendered_markdown)?;
    files_written.push("plan.md".to_string());

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
    files_written.push("user_edited/plan.md".to_string());

    let _ = ensure_git_info_exclude(root);

    Ok(PersistPlanResult {
        plan_id: clean_id,
        plan_dir: user_edited_dir.to_string_lossy().to_string(),
        files_written,
    })
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
                let raw = fs::read_to_string(&user_plan_md)?;
                let opts = crate::parser::PlanParseOptions::default();
                if let Ok(mut plan) = crate::parser::parse_plan(&raw, &opts) {
                    plan.is_user_edited = Some(true);
                    return Ok(plan);
                }
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
        let raw = fs::read_to_string(&target_path)?;
        let opts = crate::parser::PlanParseOptions::default();
        if let Ok(mut plan) = crate::parser::parse_plan(&raw, &opts) {
            if let Some(v) = version {
                plan.version = v;
            }
            plan.is_user_edited = Some(false);
            return Ok(plan);
        }
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

/// Detailed runtime state of a task in the execution pipeline.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineTaskInfo {
    pub id: String,
    pub title: String,
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assigned_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<u64>,
}

/// Gate verification result recorded for a task.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineGateResult {
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
    pub session_id: String,
    pub status: PipelineStatus,
    pub tasks: Vec<PipelineTaskInfo>,
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

/// Saves the execution pipeline state to `<root>/.tokage/execution/<sessionId>.json` using atomic write.
pub fn save_pipeline_state(root: &Path, state: &PipelineState) -> io::Result<()> {
    let clean_id = sanitize_plan_id(&state.session_id);
    let exec_dir = root.join(".tokage").join("execution");
    fs::create_dir_all(&exec_dir)?;

    let target_file = exec_dir.join(format!("{}.json", clean_id));
    let tmp_file = exec_dir.join(format!("{}.tmp.{}", clean_id, std::process::id()));

    let json_bytes = serde_json::to_vec_pretty(state)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    // Atomic write: write to tmp file then rename
    fs::write(&tmp_file, &json_bytes)?;
    fs::rename(&tmp_file, &target_file)?;

    let _ = ensure_git_info_exclude(root);
    Ok(())
}

/// Loads the execution pipeline state from `<root>/.tokage/execution/<sessionId>.json`.
/// Handles cold startup or crash recovery: if the process unexpectedly died while tasks were RUNNING or VERIFYING,
/// marks those dangling tasks as INTERRUPTED so users can review or resume them.
pub fn load_pipeline_state(root: &Path, session_id: &str) -> io::Result<Option<PipelineState>> {
    let clean_id = sanitize_plan_id(session_id);
    let file_path = root.join(".tokage").join("execution").join(format!("{}.json", clean_id));
    if !file_path.is_file() {
        return Ok(None);
    }

    let content = fs::read_to_string(&file_path)?;
    let mut state: PipelineState = serde_json::from_str(&content)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let mut modified = false;
    // Cold start recovery check:
    // If the pipeline is recorded as RUNNING when cold loaded from disk, check for interrupted tasks
    if state.status == PipelineStatus::Running {
        for t in &mut state.tasks {
            if t.state == "RUNNING" || t.state == "VERIFYING" {
                t.state = "INTERRUPTED".to_string();
                modified = true;
            }
        }
        for (task_id, st) in &mut state.task_states {
            if st == "RUNNING" || st == "VERIFYING" {
                *st = "INTERRUPTED".to_string();
                if let Some(logs) = state.task_logs.get_mut(task_id) {
                    logs.push("[系统] 检测到先前执行异常中断（应用关闭或崩溃），任务已自动置为 INTERRUPTED，支持从断点恢复或重新执行。".to_string());
                }
                modified = true;
            }
        }
        state.status = PipelineStatus::Interrupted;
    }

    if modified {
        let _ = save_pipeline_state(root, &state);
    }

    Ok(Some(state))
}

/// Ensures `.tokage/` is added to `.git/info/exclude` (P5.m3b).
pub fn ensure_git_info_exclude(root: &Path) -> io::Result<()> {
    let exclude_path = root.join(".git").join("info").join("exclude");
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
        out.push_str("## 目标\n");
        out.push_str(&plan.spec.goal);
        out.push_str("\n\n");
    }
    if !plan.spec.in_scope.is_empty() || !plan.spec.out_of_scope.is_empty() {
        out.push_str("## 范围\n");
        for item in &plan.spec.in_scope {
            out.push_str(&format!("- 包含：{}\n", item));
        }
        for item in &plan.spec.out_of_scope {
            out.push_str(&format!("- 不包含：{}\n", item));
        }
        out.push('\n');
    }
    if !plan.spec.decisions.is_empty() {
        out.push_str("## 关键决策\n");
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
            session_id: "session-abc-123".to_string(),
            status: PipelineStatus::Running,
            tasks: vec![
                PipelineTaskInfo {
                    id: "T1".to_string(),
                    title: "初始化项目".to_string(),
                    state: "DONE".to_string(),
                    assigned_model: Some("claude-3-7-sonnet".to_string()),
                    turn_id: Some("turn-1".to_string()),
                    started_at: Some(1000),
                    ended_at: Some(2000),
                },
                PipelineTaskInfo {
                    id: "T2".to_string(),
                    title: "编写功能并验证".to_string(),
                    state: "RUNNING".to_string(),
                    assigned_model: Some("codex-cli".to_string()),
                    turn_id: Some("turn-2".to_string()),
                    started_at: Some(2050),
                    ended_at: None,
                },
            ],
            task_states,
            task_logs,
            gate_results: vec![
                PipelineGateResult {
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
        assert!(loaded.task_logs.get("T2").unwrap().iter().any(|l| l.contains("INTERRUPTED")));
    }
}
