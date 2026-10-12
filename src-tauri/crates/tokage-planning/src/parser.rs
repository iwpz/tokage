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
    //    (section YAML -> fenced YAML anywhere -> bare `- id:` lists -> Markdown table)
    let tasks = match parse_tasks(&tasks_content, opts) {
        Ok(t) if !t.is_empty() => t,
        _ => match extract_tasks_from_full_markdown(markdown, opts) {
            Ok(t) if !t.is_empty() => t,
            _ => match extract_tasks_from_bare_yaml_lists(markdown, opts) {
                Ok(t) if !t.is_empty() => t,
                _ => match extract_tasks_from_markdown_table(&tasks_content, opts) {
                    Ok(t) if !t.is_empty() => t,
                    _ => extract_tasks_from_markdown_table(markdown, opts)?,
                },
            },
        },
    };

    Ok(Plan {
        version: 1,
        parent_plan_id: None,
        spec,
        contracts,
        tasks,
        notes: notes.filter(|n| !n.trim().is_empty()),
        raw_markdown,
        ..Default::default()
    })
}

/// Fallback scanner to search entire markdown for YAML tasks code blocks when section extraction missed it.
fn extract_tasks_from_full_markdown(markdown: &str, opts: &PlanParseOptions) -> Result<Vec<TaskItem>, PlanParseError> {
    let yaml_re = Regex::new(r"(?s)```(?:yaml)?\s*\n(.*?)\n```").unwrap();
    for cap in yaml_re.captures_iter(markdown) {
        let block = &cap[1];
        if block.contains("- id:") || block.contains("domain:") || block.contains("acceptance:") {
            if let Ok(tasks) = parse_tasks(block, opts) {
                if !tasks.is_empty() {
                    return Ok(tasks);
                }
            }
        }
    }
    Err(PlanParseError::MissingTasks)
}

/// Fallback when the model emits bare YAML task lists under milestone/module headings
/// (e.g. `# P0 里程碑` / `# P0.M1 模块`) without a `# Tasks` section or ```yaml fence.
fn extract_tasks_from_bare_yaml_lists(
    markdown: &str,
    opts: &PlanParseOptions,
) -> Result<Vec<TaskItem>, PlanParseError> {
    let mut blocks: Vec<String> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut in_task = false;

    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("- id:") {
            if !current.is_empty() {
                blocks.push(current.join("\n"));
                current.clear();
            }
            in_task = true;
            current.push(line.to_string());
            continue;
        }

        if !in_task {
            continue;
        }

        // Milestone / module headings or other top-level prose end the current task.
        if trimmed.starts_with('#') {
            blocks.push(current.join("\n"));
            current.clear();
            in_task = false;
            continue;
        }

        // Indented YAML fields / nested lists continue the task; blank lines are kept.
        if trimmed.is_empty() || line.starts_with(' ') || line.starts_with('\t') {
            current.push(line.to_string());
            continue;
        }

        // Non-indented content that isn't a new `- id:` item ends the sequence.
        blocks.push(current.join("\n"));
        current.clear();
        in_task = false;
    }

    if !current.is_empty() {
        blocks.push(current.join("\n"));
    }

    if blocks.is_empty() {
        return Err(PlanParseError::MissingTasks);
    }

    let yaml_str = blocks.join("\n");
    parse_tasks(&yaml_str, opts)
}

/// Splits markdown by section headings (supporting `#`, `##`, etc.).
fn split_markdown_sections(markdown: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let mut current_key: Option<String> = None;
    let mut current_buf = Vec::new();

    for line in markdown.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            let hash_count = trimmed.chars().take_while(|&c| c == '#').count();
            let after_hash = trimmed[hash_count..].trim();
            let title = after_hash.to_lowercase();

            let matched_section = if title.starts_with("spec") {
                Some("spec")
            } else if title.starts_with("contract") {
                Some("contracts")
            } else if title.starts_with("task") {
                Some("tasks")
            } else if title.starts_with("note") {
                Some("notes")
            } else {
                None
            };

            if let Some(sec) = matched_section {
                let is_contract_sub = current_key.as_deref() == Some("contracts")
                    && (hash_count >= 3 || (title.starts_with('c') && title.chars().nth(1).map_or(false, |c| c.is_ascii_digit())));
                let is_spec_sub = current_key.as_deref() == Some("spec")
                    && (hash_count >= 3 || title.contains("scope") || title.contains("goal") || title.contains("decision"));

                if !is_contract_sub && !is_spec_sub {
                    if let Some(key) = current_key.take() {
                        map.insert(key, current_buf.join("\n"));
                        current_buf.clear();
                    }
                    current_key = Some(sec.to_string());
                    continue;
                }
            } else if hash_count == 1 {
                if let Some(key) = current_key.take() {
                    map.insert(key, current_buf.join("\n"));
                    current_buf.clear();
                }
                current_key = Some(title);
                continue;
            }
        }

        if current_key.is_some() {
            current_buf.push(line);
        }
    }

    if let Some(key) = current_key {
        map.insert(key, current_buf.join("\n"));
    }

    map
}

/// Recognizes an `In scope` / `Out of scope` label at the start of `text` and returns the
/// mode plus the remainder after the label and its colon (ASCII `:` or full-width `：`).
fn split_scope_label(text: &str) -> Option<(&'static str, &str)> {
    const OUT: &[&str] = &["out of scope", "out-of-scope", "non-goals", "non-goal", "out"];
    const IN: &[&str] = &["in scope", "in-scope", "in"];
    for (mode, labels) in [("out", OUT), ("in", IN)] {
        for label in labels {
            let n = label.len();
            if text.len() < n || !text.is_char_boundary(n) || !text[..n].eq_ignore_ascii_case(label) {
                continue;
            }
            let rest = text[n..].trim_start().trim_start_matches('*').trim_start();
            if rest.is_empty() {
                return Some((mode, ""));
            }
            if let Some(r) = rest.strip_prefix(':').or_else(|| rest.strip_prefix('：')) {
                return Some((mode, r.trim_start_matches('*').trim()));
            }
        }
    }
    None
}

/// Parses the `# Spec` section.
fn parse_spec(spec_text: &str) -> PlanSpec {
    let mut goal = String::new();
    let mut in_scope = Vec::new();
    let mut out_of_scope = Vec::new();
    let mut decisions = Vec::new();

    let mut current_sub = "";
    let mut scope_mode = ""; // "in" or "out"

    for line in spec_text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // 1. Check for markdown headings (##, ###, ####)
        if trimmed.starts_with('#') {
            let hash_count = trimmed.chars().take_while(|&c| c == '#').count();
            if hash_count >= 2 {
                let heading_raw = trimmed[hash_count..].trim();
                let heading = heading_raw.to_lowercase();

                if heading.contains("goal") || heading.contains("target") {
                    current_sub = "goal";
                    scope_mode = "";
                } else if heading.contains("out of scope")
                    || heading.contains("out-of-scope")
                    || heading.contains("non-goal")
                {
                    current_sub = "scope";
                    scope_mode = "out";
                } else if heading.contains("in scope") || heading.contains("in-scope") {
                    current_sub = "scope";
                    scope_mode = "in";
                } else if heading.contains("scope") {
                    current_sub = "scope";
                    scope_mode = ""; // will be determined by subsequent markers or items
                } else if heading.contains("decision") {
                    current_sub = "decisions";
                    scope_mode = "";
                } else {
                    current_sub = "";
                    scope_mode = "";
                }
                continue;
            }
        }

        // 2. Inline scope labels (with or without bullet / bold), e.g. "- Out of scope:",
        // "**In scope:** login", "Out of scope: third-party OAuth".
        let mut unbulleted = trimmed;
        if let Some(rest) = unbulleted.strip_prefix("- ")
            .or_else(|| unbulleted.strip_prefix("* "))
            .or_else(|| unbulleted.strip_prefix("+ "))
        {
            unbulleted = rest.trim();
        }
        if let Some((mode, rest)) = split_scope_label(unbulleted.trim_matches('*').trim()) {
            if !rest.is_empty() {
                if mode == "out" {
                    out_of_scope.push(rest.to_string());
                } else {
                    in_scope.push(rest.to_string());
                }
            }
            current_sub = "scope";
            scope_mode = mode;
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
                // Strip list bullet prefixes: "- ", "* ", "+ ", "1. ", "2. ", etc.
                let mut content = trimmed;
                if let Some(rest) = content.strip_prefix("- ")
                    .or_else(|| content.strip_prefix("* "))
                    .or_else(|| content.strip_prefix("+ "))
                {
                    content = rest.trim();
                } else if let Some(idx) = content.find(". ") {
                    if idx < 5 && content[..idx].chars().all(|c| c.is_ascii_digit()) {
                        content = content[idx + 2..].trim();
                    }
                }

                // Item-level tags: "In scope: ...", "Out of scope: ...", "out: ..."
                if let Some((mode, rest)) = split_scope_label(content.trim_matches('*').trim()) {
                    if !rest.is_empty() {
                        if mode == "out" {
                            out_of_scope.push(rest.to_string());
                        } else {
                            in_scope.push(rest.to_string());
                        }
                    }
                } else if scope_mode == "out" {
                    out_of_scope.push(content.to_string());
                } else {
                    in_scope.push(content.to_string());
                }
            }
            "decisions" => {
                let mut content = trimmed;
                if let Some(rest) = content.strip_prefix("- ")
                    .or_else(|| content.strip_prefix("* "))
                    .or_else(|| content.strip_prefix("+ "))
                {
                    content = rest.trim();
                } else if let Some(idx) = content.find(". ") {
                    if idx < 5 && content[..idx].chars().all(|c| c.is_ascii_digit()) {
                        content = content[idx + 2..].trim();
                    }
                }
                if !content.is_empty() {
                    decisions.push(content.to_string());
                }
            }
            _ => {}
        }
    }

    if goal.is_empty() {
        // Fallback: extract the first non-heading paragraph from spec_text
        for line in spec_text.lines() {
            let t = line.trim();
            if !t.is_empty() && !t.starts_with('#') && !t.starts_with('-') && !t.starts_with('*') {
                goal = t.to_string();
                break;
            }
        }
    }
    if goal.is_empty() {
        goal = "System features and architecture plan".to_string();
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
    let contract_header_re = Regex::new(r"^(?i)#{2,4}\s*(C\d+|[A-Za-z0-9_\-]+)\s*(.*)$").unwrap();
    let alt_header_re = Regex::new(r"^(?i)#{2,4}\s*(.+)$").unwrap();

    let mut current_id: Option<String> = None;
    let mut current_title = String::new();
    let mut current_buf = Vec::new();
    let mut auto_idx = 1;

    for line in contracts_text.lines() {
        let trimmed = line.trim();
        if let Some(caps) = contract_header_re.captures(trimmed) {
            let id_candidate = caps[1].to_uppercase();
            if id_candidate.starts_with('C') && id_candidate.len() >= 2 && id_candidate[1..].chars().all(|c| c.is_ascii_digit()) {
                if let Some(id) = current_id.take() {
                    contracts.push(PlanContract {
                        id,
                        title: current_title.clone(),
                        content: current_buf.join("\n").trim().to_string(),
                    });
                    current_buf.clear();
                }
                current_id = Some(id_candidate);
                current_title = caps[2].trim().to_string();
                continue;
            }
        }

        if let Some(caps) = alt_header_re.captures(trimmed) {
            let heading = caps[1].trim();
            if heading.to_lowercase().contains("contract") {
                if let Some(id) = current_id.take() {
                    contracts.push(PlanContract {
                        id,
                        title: current_title.clone(),
                        content: current_buf.join("\n").trim().to_string(),
                    });
                    current_buf.clear();
                }
                current_id = Some(format!("C{}", auto_idx));
                auto_idx += 1;
                current_title = heading.to_string();
                continue;
            }
        }

        if current_id.is_some() {
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

    if contracts.is_empty() {
        let code_re = Regex::new(r"(?s)```[a-zA-Z0-9_-]*\s*\n(.*?)\n```").unwrap();
        for (idx, cap) in code_re.captures_iter(contracts_text).enumerate() {
            let content = cap[1].trim().to_string();
            if !content.is_empty() {
                contracts.push(PlanContract {
                    id: format!("C{}", idx + 1),
                    title: format!("Contract C{}", idx + 1),
                    content,
                });
            }
        }
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
            let write_scope = get_str_list_prop(&map, "write_scope")
                .into_iter()
                .map(|s| tokage_core::command_policy::scope::normalize_scope(&s))
                .collect();
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

/// Extracts tasks from markdown table (e.g. `| ID | Domain | Depends | Files | Acceptance |`).
fn extract_tasks_from_markdown_table(text: &str, opts: &PlanParseOptions) -> Result<Vec<TaskItem>, PlanParseError> {
    let mut tasks = Vec::new();
    let mut headers: Option<Vec<String>> = None;
    let path_re = Regex::new(r"`([^`]+)`").unwrap();

    for line in text.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') || !trimmed.ends_with('|') {
            continue;
        }

        // Split cells
        let cells: Vec<String> = trimmed[1..trimmed.len() - 1]
            .split('|')
            .map(|c| c.trim().to_string())
            .collect();

        if cells.is_empty() {
            continue;
        }

        // Check if header row
        if headers.is_none() {
            let line_lower = trimmed.to_lowercase();
            if (line_lower.contains("id") || line_lower.contains("task"))
                && (line_lower.contains("domain") || line_lower.contains("file") || line_lower.contains("scope") || line_lower.contains("acceptance") || line_lower.contains("depend"))
            {
                headers = Some(cells.into_iter().map(|c| c.to_lowercase()).collect());
            }
            continue;
        }

        // Check if separator row (e.g. |:--|:--| or |--|--)
        if cells.iter().all(|c| c.chars().all(|ch| ch == '-' || ch == ':' || ch.is_whitespace())) {
            continue;
        }

        let hdr = headers.as_ref().unwrap();
        let mut id = String::new();
        let mut domain = String::new();
        let mut title = String::new();
        let mut goal = String::new();
        let mut depends_on = Vec::new();
        let mut write_scope = Vec::new();
        let mut acceptance_criteria = Vec::new();
        let mut acceptance_commands = Vec::new();

        for (idx, cell) in cells.iter().enumerate() {
            if idx >= hdr.len() {
                break;
            }
            let col = &hdr[idx];
            if col.contains("id") {
                let clean_id = cell.replace('*', "").replace('`', "").replace('_', "").trim().to_string();
                id = clean_id;
            } else if col.contains("domain") {
                let d = cell.replace('*', "").replace('`', "").trim().to_string();
                domain = if d.contains("frontend") || d.contains("ui") {
                    "frontend-ui".to_string()
                } else if d.contains("backend") || d.contains("api") {
                    "backend-api".to_string()
                } else if d.contains("test") {
                    "testing".to_string()
                } else if d.contains("db")
                    || d.contains("data-model")
                    || d.contains("data model")
                    || d.contains("schema")
                    || d.contains("migration")
                {
                    "data-model".to_string()
                } else if d.contains("algo") || d.contains("performance") {
                    "algorithm".to_string()
                } else if d.contains("infra")
                    || d.contains("devops")
                    || d.contains("ci")
                    || d.contains("deploy")
                    || d.contains("docker")
                {
                    "infra".to_string()
                } else if d.contains("refactor") {
                    "refactor".to_string()
                } else if d.contains("integrat") {
                    "integration".to_string()
                } else if !d.is_empty() {
                    d
                } else {
                    opts.default_domain.clone()
                };
            } else if col.contains("file") || col.contains("scope") || col.contains("path") {
                for cap in path_re.captures_iter(cell) {
                    let p = cap[1].trim();
                    if !p.is_empty() {
                        write_scope.push(p.to_string());
                    }
                }
                if write_scope.is_empty() && !cell.trim().is_empty() && cell.trim() != "—" && cell.trim() != "-" {
                    for part in cell.split(&['、', ',', ';'][..]) {
                        let p = part.trim().replace('`', "");
                        if !p.is_empty() && p != "—" && p != "-" {
                            write_scope.push(p);
                        }
                    }
                }
            } else if col.contains("depend") {
                let d_str = cell.replace('*', "").replace('`', "").trim().to_string();
                if d_str != "—" && d_str != "-" && !d_str.is_empty() {
                    for part in d_str.split(&['、', ',', ';', ' '][..]) {
                        let dep = part.trim();
                        if !dep.is_empty() && dep != "—" && dep != "-" {
                            depends_on.push(dep.to_string());
                        }
                    }
                }
            } else if col.contains("acceptance") || col.contains("criteria") {
                for cap in path_re.captures_iter(cell) {
                    let cmd = cap[1].trim();
                    if cmd.contains("test") || cmd.contains("cargo") || cmd.contains("npm") || cmd.contains("dotnet") {
                        acceptance_commands.push(cmd.to_string());
                    }
                }
                let clean_text = cell.trim();
                if !clean_text.is_empty() {
                    acceptance_criteria.push(clean_text.to_string());
                }
            } else if col.contains("title") || col.contains("task") {
                title = cell.replace('*', "").trim().to_string();
            } else if col.contains("goal") || col.contains("description") {
                goal = cell.replace('*', "").trim().to_string();
            }
        }

        if id.is_empty() {
            id = format!("T{}", tasks.len() + 1);
        }

        if title.is_empty() && !goal.is_empty() {
            title = goal.clone();
        } else if goal.is_empty() && !title.is_empty() {
            goal = title.clone();
        } else if title.is_empty() && goal.is_empty() {
            title = format!("Task {}", id);
            goal = title.clone();
        }

        if domain.is_empty() {
            domain = if !opts.default_domain.is_empty() {
                opts.default_domain.clone()
            } else {
                "general".to_string()
            };
        }

        if acceptance_commands.is_empty() && !opts.default_test_commands.is_empty() {
            acceptance_commands = opts.default_test_commands.clone();
        }

        let deliverables = vec![title.clone()];
        let write_scope = write_scope
            .into_iter()
            .map(|s| tokage_core::command_policy::scope::normalize_scope(&s))
            .collect();

        tasks.push(TaskItem {
            id,
            domain,
            title,
            goal,
            depends_on,
            contracts: vec![],
            write_scope,
            deliverables,
            acceptance: TaskAcceptance {
                commands: acceptance_commands,
                criteria: acceptance_criteria,
                ..Default::default()
            },
            size: TaskSize::M,
            difficulty: 2,
            assigned_model: None,
        });
    }

    if tasks.is_empty() {
        Err(PlanParseError::MissingTasks)
    } else {
        Ok(tasks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_PLAN_MD: &str = r#"# Spec
## Goal
按规范实现用户与认证模块。

## Scope
- In scope: 登录注册 API
- Out of scope: 第三方 OAuth

## Decisions
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
## Goal
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
        assert_eq!(plan.tasks[0].write_scope, vec!["single/path/file.rs/**"]);

        assert_eq!(plan.tasks[0].acceptance.commands, vec!["cargo test"]);
        assert_eq!(plan.tasks[0].acceptance.criteria, vec!["测试通过"]);
        assert_eq!(plan.tasks[0].difficulty, 2);
    }

    #[test]
    fn test_parse_plan_with_irregular_headings_and_fallback() {
        // Output from LLM using ## instead of # and Chinese heading
        let irregular = r#"# 方案设计
## Goal
实现支付模块

# Contracts
### C1 支付接口
```json
{ "payId": "string" }
```

# Tasks
```yaml
- id: T1
  domain: payment
  title: 编写支付服务
  goal: 实现核心支付逻辑
  write_scope: ["src/pay/**"]
  acceptance:
    commands: ["npm test"]
    criteria: ["单测通过"]
```
"#;
        let opts = PlanParseOptions::default();
        let plan = parse_plan(irregular, &opts).expect("should parse plan with irregular headings");
        assert_eq!(plan.tasks.len(), 1);
        assert_eq!(plan.tasks[0].id, "T1");
        assert_eq!(plan.tasks[0].domain, "payment");
        assert_eq!(plan.contracts.len(), 1);
    }

    #[test]
    fn test_parse_plan_with_markdown_table_tasks() {
        let md_table = r#"# fish-idle 前后端性能评估与优化计划
# Spec
### 核心性能瓶颈
系统存在 N+1 查询与缺少虚拟滚动的问题。

# Contracts
### Backend Optimization Contract
```csharp
public interface IPlayerStateBatchQuery { }
```

# Tasks
| ID | Domain | Depends | Files | Acceptance |
|:--|:--|:--|:--|:--|
| **P0-1** | backend query optimization | — | `backend/Services/GuildCommissionService.cs` | 用 Select 投影替代深层 Include；`dotnet test` |
| **P0-2** | frontend virtual scrolling | P0-1 | `frontend/src/components/Leaderboard.vue` | 启用虚拟列表组件 |
"#;
        let opts = PlanParseOptions::default();
        let plan = parse_plan(md_table, &opts).expect("should parse markdown table tasks tolerantly");
        assert_eq!(plan.tasks.len(), 2);
        assert_eq!(plan.tasks[0].id, "P0-1");
        assert_eq!(plan.tasks[0].domain, "backend-api");
        assert_eq!(
            plan.tasks[0].write_scope,
            vec!["backend/Services/GuildCommissionService.cs/**"]
        );
        assert_eq!(plan.tasks[0].acceptance.commands, vec!["dotnet test"]);
        assert_eq!(plan.tasks[1].id, "P0-2");
        assert_eq!(plan.tasks[1].domain, "frontend-ui");
        assert_eq!(plan.tasks[1].depends_on, vec!["P0-1"]);
        assert!(!plan.spec.goal.is_empty());
        assert_eq!(plan.contracts.len(), 1);
    }

    #[test]
    fn test_parse_spec_out_of_scope_variations() {
        // Test case based on user's exact input format
        let md = r#"# Spec
## Goal
评估性能优化空间

## Scope
In scope:
- 前端首屏与计算优化
- 后端数据查询优化

Out of scope:
数值/平衡调整
协议字段变更，即 DTO 结构不变
更换 SignalR 协议：已在用 MessagePack 加 permessage-deflate，见 backend/Program.cs:270、backend/Infrastructure/SignalRWebSocketCompression.cs
离线追赶沙箱重构（LiveTickSandbox / CatchupSliceSandbox 整表双读，暂列 P2 观察项，不排任务）
SVG sprite 改成按需 <img>：改动面大，涉及全部图标组件
列表虚拟化
引入 Redis 等新基础设施

## Decisions
- 保持向后兼容

# Tasks
```yaml
- id: T1
  title: 优化查询
  write_scope: ["backend/**"]
  acceptance:
    commands: ["cargo test"]
```
"#;
        let opts = PlanParseOptions::default();
        let plan = parse_plan(md, &opts).expect("should parse successfully");

        assert_eq!(plan.spec.in_scope.len(), 2);
        assert_eq!(plan.spec.in_scope[0], "前端首屏与计算优化");
        assert_eq!(plan.spec.in_scope[1], "后端数据查询优化");

        assert_eq!(plan.spec.out_of_scope.len(), 7);
        assert_eq!(plan.spec.out_of_scope[0], "数值/平衡调整");
        assert_eq!(plan.spec.out_of_scope[1], "协议字段变更，即 DTO 结构不变");
        assert!(plan.spec.out_of_scope[2].starts_with("更换 SignalR 协议"));
        assert!(plan.spec.out_of_scope[3].starts_with("离线追赶沙箱重构"));
        assert!(plan.spec.out_of_scope[4].starts_with("SVG sprite 改成按需 <img>"));
        assert_eq!(plan.spec.out_of_scope[5], "列表虚拟化");
        assert_eq!(plan.spec.out_of_scope[6], "引入 Redis 等新基础设施");
    }

    #[test]
    fn test_parse_spec_out_of_scope_with_subheadings() {
        let md = r#"# Spec
## Goal
重构认证系统

### In scope
- 邮箱登录注册
- 短信验证码登录

### Out of Scope
- 微信第三方登录
- 支付宝免密支付

## Decisions
- 使用 JWT
# Tasks
```yaml
- id: T1
  title: 接口
  write_scope: ["src/**"]
  acceptance:
    commands: ["npm test"]
```
"#;
        let opts = PlanParseOptions::default();
        let plan = parse_plan(md, &opts).expect("should parse successfully");

        assert_eq!(plan.spec.in_scope, vec!["邮箱登录注册", "短信验证码登录"]);
        assert_eq!(plan.spec.out_of_scope, vec!["微信第三方登录", "支付宝免密支付"]);
        assert_eq!(plan.spec.decisions, vec!["使用 JWT"]);
    }

    #[test]
    fn test_parse_spec_out_of_scope_nested_bullets() {
        let md = r#"# Spec
## Goal
评估优化空间

## Scope
- In scope: 
  - tick 热路径索引
  - 战斗 tick 的重复加载与 N+1
  - 每次推送都序列化压缩一遍的 payload 指标
  - 配置读锁
  - 静态字典泄漏
  - 前端深度 watcher、库存分组重算、interval 泄漏
  - 静态目录数据改 shallowRef
  - MessagePack 驼峰转换的整对象复制
  - 打包分块与目录 chunk
- Out of scope: 
  - 数值/平衡调整
  - 协议字段变更，即 DTO 结构不变
  - 更换 SignalR 协议：已在用 MessagePack 加 permessage-deflate，见 `backend/Program.cs:270`、`backend/Infrastructure/SignalRWebSocketCompression.cs`
  - 离线追赶沙箱重构（LiveTickSandbox / CatchupSliceSandbox 整表双读，暂列 P2 观察项，不排任务）
  - SVG sprite 改成按需 `<img>`：改动面大，涉及全部图标组件
  - 列表虚拟化
  - 引入 Redis 等新基础设施

# Tasks
```yaml
- id: T1
  title: 优化
  write_scope: ["backend/**"]
  acceptance:
    commands: ["dotnet test"]
```
"#;
        let opts = PlanParseOptions::default();
        let plan = parse_plan(md, &opts).expect("should parse nested bullets successfully");

        assert_eq!(plan.spec.in_scope.len(), 9);
        assert_eq!(plan.spec.in_scope[0], "tick 热路径索引");
        assert_eq!(plan.spec.in_scope[8], "打包分块与目录 chunk");

        assert_eq!(plan.spec.out_of_scope.len(), 7);
        assert_eq!(plan.spec.out_of_scope[0], "数值/平衡调整");
        assert_eq!(plan.spec.out_of_scope[1], "协议字段变更，即 DTO 结构不变");
        assert!(plan.spec.out_of_scope[2].starts_with("更换 SignalR 协议"));
        assert!(plan.spec.out_of_scope[3].starts_with("离线追赶沙箱重构"));
        assert!(plan.spec.out_of_scope[4].starts_with("SVG sprite 改成按需 `<img>`"));
        assert_eq!(plan.spec.out_of_scope[5], "列表虚拟化");
        assert_eq!(plan.spec.out_of_scope[6], "引入 Redis 等新基础设施");
    }

    #[test]
    fn test_parse_bare_yaml_under_milestone_headers() {
        // Planner sometimes emits P0/P1 headings + bare YAML lists (no # Tasks / fence).
        let md = r#"# P0 里程碑：连接稳定性与恢复一致性
# P0.M1 模块：小游戏协议完整性
- id: P0.M1a
  domain: integration
  title: 完善小游戏心跳与关闭语义
  goal: 满足 C4 并消除空闲连接缺少客户端心跳的问题
  depends_on: []
  contracts: [C4, C8]
  write_scope: ["minigame/miniapp/src/services/signalr-client.js"]
  deliverables: ["兼容现有 API 的生命周期与协议处理"]
  acceptance:
    commands: ["npm run test:miniapp"]
    criteria: ["现有测试通过", "握手错误不进入 connected"]
  size: M
  difficulty: 2

- id: P0.M1b
  domain: testing
  title: 小游戏连接回归测试
  goal: 使用模拟 socket 与可控时间验证协议边界
  depends_on: [P0.M1a]
  contracts: [C4, C8]
  write_scope: ["minigame/miniapp/test/signalr-client.test.js"]
  deliverables: ["心跳、失活、握手失败测试"]
  acceptance:
    commands: ["npm run test:miniapp"]
    criteria: ["测试全部通过"]
  size: M
  difficulty: 2

# P1 里程碑：控制积压
# P1.M1 模块：前端战斗消息缓冲
- id: P1.M1a
  domain: algorithm
  title: 为战斗消息缓冲增加上限与兜底
  goal: 满足 C5
  depends_on: []
  contracts: [C5, C8]
  write_scope: ["frontend/src/utils/combatTickBuffer.ts"]
  deliverables: ["有界待处理队列"]
  acceptance:
    commands: ["npm run build:f"]
    criteria: ["构建成功"]
  size: M
  difficulty: 2
"#;
        let opts = PlanParseOptions::default();
        let plan = parse_plan(md, &opts).expect("bare YAML under milestone headers should parse");
        assert_eq!(plan.tasks.len(), 3);
        assert_eq!(plan.tasks[0].id, "P0.M1a");
        assert_eq!(plan.tasks[0].domain, "integration");
        assert_eq!(
            plan.tasks[0].write_scope,
            vec!["minigame/miniapp/src/services/signalr-client.js"]
        );
        assert_eq!(plan.tasks[1].id, "P0.M1b");
        assert_eq!(plan.tasks[1].depends_on, vec!["P0.M1a"]);
        assert_eq!(plan.tasks[2].id, "P1.M1a");
        assert_eq!(plan.tasks[2].domain, "algorithm");
    }
}
