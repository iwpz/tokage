//! Parser and fault-tolerant repair for Planning Markdown outputs (docs/02 §7, docs/02 plan-format.md).

use regex::Regex;
use serde_yaml::Value;
use std::collections::HashSet;
use tokage_core::planning::{
    Plan, PlanContract, PlanSpec, TaskAcceptance, TaskItem, TaskSize,
};

/// Errors encountered during plan parsing.
#[derive(Debug, thiserror::Error)]
pub enum PlanParseError {
    #[error("Missing or empty Tasks section")]
    MissingTasks,
    #[error("Failed to parse YAML tasks block: {0}")]
    YamlError(#[from] serde_yaml::Error),
    #[error("Invalid plan structure: {0}")]
    InvalidStructure(String),
}

/// Options to guide parsing and fallback repair.
#[derive(Clone, Debug, Default)]
pub struct PlanParseOptions {
    /// Discovered build/test commands from project digest to fill missing acceptance commands.
    pub default_test_commands: Vec<String>,
    /// Fallback default domain if missing or invalid.
    pub default_domain: String,
}

/// Parses raw planning markdown into structured Plan, performing tolerant local repairs.
pub fn parse_plan(markdown: &str, opts: &PlanParseOptions) -> Result<Plan, PlanParseError> {
    let raw_markdown = markdown.to_string();

    // 1. Extract major top-level sections (# Spec, # Contracts, # Tasks, # Notes)
    let sections = split_markdown_sections(markdown);

    let spec_content = sections.get("spec").cloned().unwrap_or_default();
    let contracts_content = sections.get("contracts").cloned().unwrap_or_default();
    let tasks_content = sections.get("tasks").cloned().unwrap_or_default();
    let notes = sections.get("notes").cloned();

    // 2. Parse Spec details (Goal, In Scope, Out of Scope, Decisions)
    let spec = parse_spec(&spec_content);

    // 3. Parse Contracts (## C1 ..., ## C2 ...)
    let contracts = parse_contracts(&contracts_content);

    // 4. Extract and parse Tasks YAML with tolerant fallback repair
    let tasks = parse_tasks(&tasks_content, opts)?;

    Ok(Plan {
        spec,
        contracts,
        tasks,
        notes: notes.filter(|n| !n.trim().is_empty()),
        raw_markdown,
    })
}

/// Splits markdown by level 1 headings (`# Heading`).
fn split_markdown_sections(markdown: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let mut current_key: Option<String> = None;
    let mut current_buf = Vec::new();

    for line in markdown.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("# ") {
            if let Some(key) = current_key.take() {
                map.insert(key, current_buf.join("\n"));
                current_buf.clear();
            }
            let title = trimmed[2..].trim().to_lowercase();
            if title.starts_with("spec") || title.contains("设计") || title.contains("目标") {
                current_key = Some("spec".to_string());
            } else if title.starts_with("contract") || title.contains("契约") {
                current_key = Some("contracts".to_string());
            } else if title.starts_with("task") || title.contains("任务") {
                current_key = Some("tasks".to_string());
            } else if title.starts_with("note") || title.contains("说明") || title.contains("注意") {
                current_key = Some("notes".to_string());
            } else {
                current_key = Some(title);
            }
        } else if current_key.is_some() {
            current_buf.push(line);
        }
    }

    if let Some(key) = current_key {
        map.insert(key, current_buf.join("\n"));
    }

    map
}

/// Parses the `# Spec` section.
fn parse_spec(spec_text: &str) -> PlanSpec {
    let mut goal = String::new();
    let mut in_scope = Vec::new();
    let mut out_of_scope = Vec::new();
    let mut decisions = Vec::new();

    let mut current_sub = "";

    for line in spec_text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            let heading = trimmed[3..].trim().to_lowercase();
            if heading.contains("目标") || heading.contains("goal") || heading.contains("target") {
                current_sub = "goal";
            } else if heading.contains("范围") || heading.contains("scope") {
                current_sub = "scope";
            } else if heading.contains("决策") || heading.contains("decision") {
                current_sub = "decisions";
            } else {
                current_sub = "";
            }
            continue;
        }

        match current_sub {
            "goal" => {
                if !trimmed.is_empty() {
                    if !goal.is_empty() {
                        goal.push(' ');
                    }
                    goal.push_str(trimmed);
                }
            }
            "scope" => {
                if let Some(item) = trimmed.strip_prefix("- ") {
                    let item_trim = item.trim();
                    if item_trim.starts_with("包含：") || item_trim.starts_with("包含:") || item_trim.to_lowercase().starts_with("in:") {
                        let clean = item_trim.trim_start_matches("包含：").trim_start_matches("包含:").trim_start_matches("in:").trim();
                        if !clean.is_empty() { in_scope.push(clean.to_string()); }
                    } else if item_trim.starts_with("不包含：") || item_trim.starts_with("不包含:") || item_trim.to_lowercase().starts_with("out:") {
                        let clean = item_trim.trim_start_matches("不包含：").trim_start_matches("不包含:").trim_start_matches("out:").trim();
                        if !clean.is_empty() { out_of_scope.push(clean.to_string()); }
                    } else {
                        in_scope.push(item_trim.to_string());
                    }
                }
            }
            "decisions" => {
                if let Some(item) = trimmed.strip_prefix("- ") {
                    let clean = item.trim();
                    if !clean.is_empty() {
                        decisions.push(clean.to_string());
                    }
                }
            }
            _ => {}
        }
    }

    PlanSpec {
        raw: spec_text.trim().to_string(),
        goal,
        in_scope,
        out_of_scope,
        decisions,
    }
}

/// Parses the `# Contracts` section by `## C...` subheadings.
fn parse_contracts(contracts_text: &str) -> Vec<PlanContract> {
    let mut contracts = Vec::new();
    let contract_header_re = Regex::new(r"^(?i)##\s*(C\d+|[A-Za-z0-9_\-]+)\s*(.*)$").unwrap();

    let mut current_id: Option<String> = None;
    let mut current_title = String::new();
    let mut current_buf = Vec::new();

    for line in contracts_text.lines() {
        let trimmed = line.trim();
        if let Some(caps) = contract_header_re.captures(trimmed) {
            if let Some(id) = current_id.take() {
                contracts.push(PlanContract {
                    id,
                    title: current_title.clone(),
                    content: current_buf.join("\n").trim().to_string(),
                });
                current_buf.clear();
            }
            current_id = Some(caps[1].to_uppercase());
            current_title = caps[2].trim().to_string();
        } else if current_id.is_some() {
            current_buf.push(line);
        }
    }

    if let Some(id) = current_id {
        contracts.push(PlanContract {
            id,
            title: current_title,
            content: current_buf.join("\n").trim().to_string(),
        });
    }

    contracts
}

/// Extracts YAML from code block and parses tasks with fallback repairs.
fn parse_tasks(tasks_text: &str, opts: &PlanParseOptions) -> Result<Vec<TaskItem>, PlanParseError> {
    // Look for ```yaml ... ``` or ``` ... ```
    let yaml_re = Regex::new(r"(?s)```(?:yaml)?\s*\n(.*?)\n```").unwrap();
    let yaml_str = if let Some(caps) = yaml_re.captures(tasks_text) {
        caps[1].to_string()
    } else {
        // Fallback: entire tasks_text might be raw YAML if code fence was omitted
        tasks_text.trim().to_string()
    };

    if yaml_str.trim().is_empty() {
        return Err(PlanParseError::MissingTasks);
    }

    // Try standard serde deserialization first
    match serde_yaml::from_str::<Vec<TaskItem>>(&yaml_str) {
        Ok(tasks) => {
            let repaired = tasks
                .into_iter()
                .enumerate()
                .map(|(idx, task)| repair_task_item(task, idx + 1, opts))
                .collect();
            Ok(repaired)
        }
        Err(_err) => {
            // Tolerant repair: parse as serde_yaml::Value and repair fields defensively
            repair_and_parse_yaml_values(&yaml_str, opts)
        }
    }
}

/// Defensive repair on a TaskItem to satisfy plan constraints.
fn repair_task_item(mut item: TaskItem, default_seq: usize, opts: &PlanParseOptions) -> TaskItem {
    if item.id.trim().is_empty() {
        item.id = format!("T{}", default_seq);
    }
    if item.domain.trim().is_empty() {
        item.domain = if !opts.default_domain.is_empty() {
            opts.default_domain.clone()
        } else {
            "general".to_string()
        };
    }
    if item.title.trim().is_empty() {
        item.title = format!("Task {}", item.id);
    }
    if item.goal.trim().is_empty() {
        item.goal = item.title.clone();
    }
    if item.difficulty == 0 || item.difficulty > 3 {
        item.difficulty = 2;
    }
    // Auto-fill acceptance commands if completely empty and project has test commands
    if item.acceptance.commands.is_empty() && !opts.default_test_commands.is_empty() {
        item.acceptance.commands = opts.default_test_commands.clone();
    }
    item
}

/// Tolerant YAML Value parser for distorted/malformed planning YAML blocks.
fn repair_and_parse_yaml_values(yaml_str: &str, opts: &PlanParseOptions) -> Result<Vec<TaskItem>, PlanParseError> {
    // Clean potential markdown artifact lines or unescaped colons
    let cleaned_yaml = preprocess_malformed_yaml(yaml_str);

    let val: Value = serde_yaml::from_str(&cleaned_yaml)
        .map_err(|e| PlanParseError::InvalidStructure(format!("YAML deserialization failed: {}", e)))?;

    let seq = match val {
        Value::Sequence(s) => s,
        Value::Mapping(m) => {
            // If the model wrapped tasks in a root key: { tasks: [...] }
            if let Some(Value::Sequence(s)) = m.get(&Value::String("tasks".to_string())) {
                s.clone()
            } else {
                return Err(PlanParseError::InvalidStructure("Expected YAML sequence of tasks".to_string()));
            }
        }
        _ => return Err(PlanParseError::InvalidStructure("Invalid YAML root value".to_string())),
    };

    let mut tasks = Vec::new();
    let mut seen_ids = HashSet::new();

    for (idx, item_val) in seq.into_iter().enumerate() {
        if let Value::Mapping(map) = item_val {
            let id = get_str_prop(&map, "id").unwrap_or_else(|| format!("T{}", idx + 1));
            let unique_id = if seen_ids.contains(&id) {
                format!("{}-{}", id, idx + 1)
            } else {
                id.clone()
            };
            seen_ids.insert(unique_id.clone());

            let domain = get_str_prop(&map, "domain")
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| {
                    if !opts.default_domain.trim().is_empty() {
                        opts.default_domain.clone()
                    } else {
                        "general".to_string()
                    }
                });
            let title = get_str_prop(&map, "title").filter(|s| !s.trim().is_empty()).unwrap_or_else(|| format!("Task {}", unique_id));
            let goal = get_str_prop(&map, "goal").filter(|s| !s.trim().is_empty()).unwrap_or_else(|| title.clone());


            let depends_on = get_str_list_prop(&map, "depends_on");
            let contracts = get_str_list_prop(&map, "contracts");
            let write_scope = get_str_list_prop(&map, "write_scope");
            let deliverables = get_str_list_prop(&map, "deliverables");

            // Acceptance parsing
            let mut acceptance = TaskAcceptance::default();
            if let Some(acc_val) = map.get(&Value::String("acceptance".to_string())) {
                if let Value::Mapping(acc_map) = acc_val {
                    acceptance.commands = get_str_list_prop(acc_map, "commands");
                    acceptance.criteria = get_str_list_prop(acc_map, "criteria");
                }
            }
            if acceptance.commands.is_empty() && !opts.default_test_commands.is_empty() {
                acceptance.commands = opts.default_test_commands.clone();
            }

            let size = match get_str_prop(&map, "size").as_deref().map(|s| s.to_uppercase()).as_deref() {
                Some("S") => TaskSize::S,
                Some("L") => TaskSize::L,
                _ => TaskSize::M,
            };

            let difficulty = map.get(&Value::String("difficulty".to_string()))
                .and_then(|v| v.as_u64())
                .map(|n| n.clamp(1, 3) as u8)
                .unwrap_or(2);

            let assigned_model = map.get(&Value::String("assigned_model".to_string()))
                .or_else(|| map.get(&Value::String("assignedModel".to_string())))
                .or_else(|| map.get(&Value::String("model".to_string())))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            tasks.push(TaskItem {
                id: unique_id,
                domain,
                title,
                goal,
                depends_on,
                contracts,
                write_scope,
                deliverables,
                acceptance,
                size,
                difficulty,
                assigned_model,
            });
        }
    }

    if tasks.is_empty() {
        return Err(PlanParseError::MissingTasks);
    }

    Ok(tasks)
}

fn get_str_prop(map: &serde_yaml::Mapping, key: &str) -> Option<String> {
    map.get(&Value::String(key.to_string()))
        .and_then(|v| match v {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
}

fn get_str_list_prop(map: &serde_yaml::Mapping, key: &str) -> Vec<String> {
    match map.get(&Value::String(key.to_string())) {
        Some(Value::Sequence(seq)) => seq
            .iter()
            .filter_map(|v| match v {
                Value::String(s) => Some(s.clone()),
                Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
            .collect(),
        Some(Value::String(s)) => {
            if s.trim().is_empty() {
                vec![]
            } else {
                vec![s.clone()]
            }
        }
        _ => vec![],
    }
}

/// Pre-processes malformed YAML strings (e.g. invalid tab indentations or trailing characters).
fn preprocess_malformed_yaml(raw: &str) -> String {
    raw.lines()
        .map(|line| {
            // Replace leading tabs with 2 spaces
            let tab_count = line.chars().take_while(|c| *c == '\t').count();
            if tab_count > 0 {
                let rest = &line[tab_count..];
                format!("{}{}", "  ".repeat(tab_count), rest)
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_PLAN_MD: &str = r#"# Spec
## 目标
按规范实现用户与认证模块。

## 范围
- 包含：登录注册 API
- 不包含：第三方 OAuth

## 关键决策
- JWT 方案：使用 HS256 无状态 Token

# Contracts
## C1 用户 API
```ts
type User = { id: string; email: string };
```

## C2 路由表
- /api/auth/login
- /api/auth/register

# Tasks
```yaml
- id: T1
  domain: backend-api
  title: 用户 API
  goal: 按 C1 实现用户查询与创建接口
  depends_on: []
  contracts: [C1]
  write_scope: ["src/server/users/**"]
  deliverables: ["GET/POST 路由", "数据访问层"]
  acceptance:
    commands: ["cargo test -p server users"]
    criteria: ["C1 中的状态码全部覆盖"]
  size: M
  difficulty: 2

- id: T2
  domain: frontend-ui
  title: 用户资料页
  goal: 按 C1 的 User 类型实现资料页与创建表单
  depends_on: []
  contracts: [C1, C2]
  write_scope: ["web/src/pages/users/**"]
  deliverables: ["资料页", "创建表单"]
  acceptance:
    commands: ["pnpm -C web typecheck"]
    criteria: ["表单可键盘操作"]
  size: M
  difficulty: 2
```

# Notes
请优先考虑错误处理边界。
"#;

    #[test]
    fn test_parse_standard_plan() {
        let opts = PlanParseOptions::default();
        let plan = parse_plan(SAMPLE_PLAN_MD, &opts).expect("should parse successfully");

        assert_eq!(plan.spec.goal, "按规范实现用户与认证模块。");
        assert_eq!(plan.spec.in_scope, vec!["登录注册 API"]);
        assert_eq!(plan.spec.out_of_scope, vec!["第三方 OAuth"]);
        assert_eq!(plan.spec.decisions, vec!["JWT 方案：使用 HS256 无状态 Token"]);

        assert_eq!(plan.contracts.len(), 2);
        assert_eq!(plan.contracts[0].id, "C1");
        assert_eq!(plan.contracts[0].title, "用户 API");
        assert!(plan.contracts[0].content.contains("type User = { id: string"));

        assert_eq!(plan.tasks.len(), 2);
        assert_eq!(plan.tasks[0].id, "T1");
        assert_eq!(plan.tasks[0].domain, "backend-api");
        assert_eq!(plan.tasks[0].acceptance.commands, vec!["cargo test -p server users"]);
        assert_eq!(plan.tasks[0].size, TaskSize::M);
        assert_eq!(plan.tasks[0].difficulty, 2);

        assert_eq!(plan.tasks[1].id, "T2");
        assert_eq!(plan.tasks[1].contracts, vec!["C1", "C2"]);
        assert_eq!(plan.notes.as_deref(), Some("请优先考虑错误处理边界。"));
    }

    #[test]
    fn test_tolerant_repair_of_malformed_yaml() {
        // Missing fields, tab indentation, wrapped in tasks:
        let malformed = r#"# Spec
## 目标
修复缺陷

# Tasks
```yaml
tasks:
	- id: 
	  title: "缺少 id 与 domain"
	  write_scope: "single/path/file.rs"
	  acceptance:
	    criteria: ["测试通过"]
```
"#;
        let opts = PlanParseOptions {
            default_test_commands: vec!["cargo test".to_string()],
            default_domain: "backend-api".to_string(),
        };

        let plan = parse_plan(malformed, &opts).expect("tolerant repair should succeed");
        assert_eq!(plan.tasks.len(), 1);
        assert_eq!(plan.tasks[0].id, "T1");
        assert_eq!(plan.tasks[0].domain, "backend-api");
        assert_eq!(plan.tasks[0].write_scope, vec!["single/path/file.rs"]);

        assert_eq!(plan.tasks[0].acceptance.commands, vec!["cargo test"]);
        assert_eq!(plan.tasks[0].acceptance.criteria, vec!["测试通过"]);
        assert_eq!(plan.tasks[0].difficulty, 2);
    }
}
