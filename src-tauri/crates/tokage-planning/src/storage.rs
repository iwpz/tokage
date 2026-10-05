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

/// Loads a structured plan from `<root>/.tokage/plans/<plan_id>/`.
pub fn load_persisted_plan(root: &Path, plan_id: &str) -> io::Result<Plan> {
    load_persisted_plan_version(root, plan_id, None)
}

/// Loads a specific version of a plan (or current if None).
pub fn load_persisted_plan_version(root: &Path, plan_id: &str, version: Option<u32>) -> io::Result<Plan> {
    let clean_id = sanitize_plan_id(plan_id);
    let plan_dir = root.join(".tokage").join("plans").join(&clean_id);
    if !plan_dir.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Plan directory not found: {}", plan_dir.display()),
        ));
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
        ..Default::default()
    })
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
}
