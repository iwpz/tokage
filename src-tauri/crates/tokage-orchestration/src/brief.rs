//! Task brief and report domain models, assembly, and parsing (docs/05 task-brief.md, docs/03 §4, P7.m2).

use serde::{Deserialize, Serialize};
use tokage_core::planning::{PlanContract, TaskItem};

/// Task brief header defined verbatim in docs/05/task-brief.md §1.
pub const TASK_BRIEF_FIXED_HEADER: &str = r#"# Tokage Task Brief v1

## Execution rules
You are one executor in a multi-agent pipeline. Other agents work on other tasks in parallel; you cannot see their context.
1. Strictly follow interfaces, types, and conventions in the "Contracts"; if you must deviate, do not edit contracts yourself—note it in the report.
2. Modify only files within "Write scope."
3. After finishing, run "Acceptance commands" for self-check.
4. Finally output a "Task report" in the format at the end, no more than 300 Token.
5. Write the free text of the report in the same language as the task description. Keep the section headings and the word `None` exactly as shown, in English.
"#;

/// Task report section headings. Shared by the brief footer, the report parser and
/// `TaskReport::to_markdown`, so the fixed English tokens cannot drift apart.
pub const REPORT_COMPLETED: &str = "What was completed";
pub const REPORT_DEVIATIONS: &str = "Deviations from contracts";
pub const REPORT_TIPS: &str = "Hints for downstream";
pub const REPORT_SELF_CHECK: &str = "Self-check against acceptance criteria";
pub const REPORT_DEFECTS: &str = "Defects found";
pub const REPORT_CHANGES_LOCAL: &str = "Changes (local)";
pub const REPORT_GATES_LOCAL: &str = "Acceptance gate (local)";
/// Sentinel written in a report section that has nothing to report.
pub const REPORT_NONE: &str = "None";

/// Upstream product passed into a task brief.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamArtifact {
    pub task_id: String,
    pub title: String,
    pub report: String,
    pub changed_files: Vec<String>,
}

/// Input parameters for assembling a task brief.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskBriefInput {
    pub task: TaskItem,
    pub contracts: Vec<PlanContract>,
    pub upstream_artifacts: Vec<UpstreamArtifact>,
    /// Evidence of gate failure when repairing (optional).
    pub failure_evidence: Option<String>,
}

/// Assembles a complete Markdown task brief (docs/05 task-brief.md §1).
pub fn assemble_task_brief(input: &TaskBriefInput) -> String {
    let mut out = String::new();

    // 1. Fixed Header
    out.push_str(TASK_BRIEF_FIXED_HEADER);
    out.push('\n');

    // 2. Task Card
    out.push_str("## Task\n");
    out.push_str(&format!("- ID: {}  Domain: {}\n", input.task.id, input.task.domain));
    out.push_str(&format!("- Title: {}\n", input.task.title));
    out.push_str(&format!("- Goal: {}\n", input.task.goal));
    if !input.task.deliverables.is_empty() {
        out.push_str(&format!("- Deliverables: {}\n", input.task.deliverables.join(", ")));
    }
    if !input.task.write_scope.is_empty() {
        out.push_str(&format!("- Write scope: {}\n", input.task.write_scope.join(", ")));
    }
    if !input.task.acceptance.commands.is_empty() {
        out.push_str(&format!("- Acceptance commands: {}\n", input.task.acceptance.commands.join(" && ")));
    }
    if !input.task.acceptance.criteria.is_empty() {
        out.push_str(&format!("- Acceptance criteria: {}\n", input.task.acceptance.criteria.join("; ")));
    }
    out.push('\n');

    // 3. Contracts
    out.push_str("## Contracts\n");
    if input.contracts.is_empty() {
        out.push_str("No dedicated contract constraints.\n");
    } else {
        for c in &input.contracts {
            out.push_str(&format!("- {} {}\n", c.id, c.title));
            if !c.content.trim().is_empty() {
                out.push_str(&c.content);
                out.push('\n');
            }
        }
    }
    out.push('\n');

    // 4. Upstream Artifacts
    out.push_str("## Upstream results\n");
    if input.upstream_artifacts.is_empty() {
        out.push_str("No direct upstream task dependencies.\n");
    } else {
        for u in &input.upstream_artifacts {
            out.push_str(&format!("### {} {}\n", u.task_id, u.title));
            out.push_str(&u.report);
            out.push('\n');
            if !u.changed_files.is_empty() {
                out.push_str(&format!("Changed files: {}\n", u.changed_files.join(", ")));
            }
            out.push('\n');
        }
    }

    // 5. Failure Evidence (if repairing)
    if let Some(evidence) = &input.failure_evidence {
        out.push_str("## Failure evidence\n");
        out.push_str(evidence);
        out.push_str("\n\n");
    }

    // 6. Fixed Task Report Format Footer (R10: header above is fixed; footer may grow)
    out.push_str("## Task report format\n");
    out.push_str(&format!("### {REPORT_COMPLETED}\n"));
    out.push_str(&format!("### {REPORT_DEVIATIONS} (write `{REPORT_NONE}` if there are none)\n"));
    out.push_str(&format!("### {REPORT_TIPS}\n"));
    out.push_str(&format!("### {REPORT_SELF_CHECK}\n"));
    // F13.e: testing tasks may report implementation defects
    if input.task.domain == "testing" {
        out.push_str(&format!("### {REPORT_DEFECTS}\n"));
        out.push_str(&format!("(optional; one `- <implementation task id>: <description>` per line, `{REPORT_NONE}` if there are none)\n"));
    }

    out
}

/// Structured task report extracted from model output and complemented locally (docs/03 §4.2, docs/05 §2).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskReport {
    pub task_id: String,
    pub title: String,
    pub model: String,
    pub attempt_id: String,
    pub first_pass: bool,
    pub is_locally_generated: bool,

    // Model sections
    pub completed_work: String,
    pub contract_deviations: Option<String>,
    pub tips_for_downstream: String,
    pub self_check_criteria: Vec<String>,

    // Local extractions
    pub changed_files_stat: String,
    pub changed_files_list: Vec<String>,
    pub gate_results_summary: String,
    pub cost_usd: Option<f64>,
    /// F13.e: testing-task defect notes (`- <implTaskId>: <desc>` lines).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defects: Option<String>,
}

impl TaskReport {
    /// Formats the task report into Markdown for persistence and display.
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();
        md.push_str(&format!("## {} {} · Done\n", self.task_id, self.title));
        md.push_str(&format!(
            "Model: {}  Window: {}  First pass: {}{}\n\n",
            self.model,
            self.attempt_id,
            if self.first_pass { "yes" } else { "no" },
            if self.is_locally_generated { "  [locally generated]" } else { "" }
        ));

        md.push_str(&format!("### {REPORT_COMPLETED}\n"));
        md.push_str(&self.completed_work);
        md.push_str("\n\n");

        md.push_str(&format!("### {REPORT_DEVIATIONS}\n"));
        md.push_str(self.contract_deviations.as_deref().unwrap_or(REPORT_NONE));
        md.push_str("\n\n");

        md.push_str(&format!("### {REPORT_TIPS}\n"));
        md.push_str(&self.tips_for_downstream);
        md.push_str("\n\n");

        md.push_str(&format!("### {REPORT_SELF_CHECK}\n"));
        if self.self_check_criteria.is_empty() {
            md.push_str(&format!("{REPORT_NONE}\n"));
        } else {
            for c in &self.self_check_criteria {
                md.push_str(&format!("- {}\n", c));
            }
        }
        md.push('\n');

        if !self.changed_files_stat.trim().is_empty() {
            md.push_str(&format!("### {REPORT_CHANGES_LOCAL}\n"));
            md.push_str(&self.changed_files_stat);
            md.push_str("\n\n");
        }

        if !self.gate_results_summary.trim().is_empty() {
            md.push_str(&format!("### {REPORT_GATES_LOCAL}\n"));
            md.push_str(&self.gate_results_summary);
            md.push('\n');
        }

        md
    }

    /// Converts this report into a compact upstream artifact passed to downstream briefs (docs/05 §2).
    /// Omits full diff stats and gate traces to save token budget.
    pub fn to_upstream_artifact(&self) -> UpstreamArtifact {
        let mut report_text = String::new();
        report_text.push_str(&format!("Completed: {}\n", self.completed_work.trim()));
        if let Some(dev) = &self.contract_deviations {
            if !dev.trim().is_empty() && !is_none_token(dev) {
                report_text.push_str(&format!("Deviations from contracts: {}\n", dev.trim()));
            }
        }
        if !self.tips_for_downstream.trim().is_empty() && !is_none_token(&self.tips_for_downstream) {
            report_text.push_str(&format!("Hints: {}\n", self.tips_for_downstream.trim()));
        }

        UpstreamArtifact {
            task_id: self.task_id.clone(),
            title: self.title.clone(),
            report: report_text,
            changed_files: self.changed_files_list.clone(),
        }
    }
}

/// Parses task report from model output, or generates a local fallback if missing / malformed (docs/03 §4.2, P7.m2d).
pub fn parse_or_generate_task_report(
    task: &TaskItem,
    raw_output: &str,
    model: &str,
    attempt_id: &str,
    first_pass: bool,
    changed_files_stat: &str,
    changed_files_list: Vec<String>,
    gate_results_summary: &str,
    cost_usd: Option<f64>,
) -> TaskReport {
    if let Some(parsed) = try_parse_model_report(raw_output) {
        let has_deviations = parsed.contract_deviations.as_ref().map(|d| {
            !d.trim().is_empty() && !is_none_token(d)
        }).unwrap_or(false);

        TaskReport {
            task_id: task.id.clone(),
            title: task.title.clone(),
            model: model.to_string(),
            attempt_id: attempt_id.to_string(),
            first_pass,
            is_locally_generated: false,
            completed_work: parsed.completed_work,
            contract_deviations: if has_deviations { parsed.contract_deviations } else { None },
            tips_for_downstream: parsed.tips_for_downstream,
            self_check_criteria: parsed.self_check_criteria,
            changed_files_stat: changed_files_stat.to_string(),
            changed_files_list,
            gate_results_summary: gate_results_summary.to_string(),
            cost_usd,
            defects: normalize_defects(parsed.defects),
        }
    } else {
        // Fallback: Generate locally without issuing additional LLM turn (P7.m2d)
        let fallback_work = if !raw_output.trim().is_empty() {
            let tail = extract_output_tail(raw_output, 300);
            format!("The model finished without a well-formed report; tail of its output:\n{}", tail)
        } else {
            format!("Task executed; deliverables: {}", task.deliverables.join(", "))
        };

        TaskReport {
            task_id: task.id.clone(),
            title: task.title.clone(),
            model: model.to_string(),
            attempt_id: attempt_id.to_string(),
            first_pass,
            is_locally_generated: true,
            completed_work: fallback_work,
            contract_deviations: None,
            tips_for_downstream: "See the changed-files list.".to_string(),
            self_check_criteria: vec!["Automated verification gate executed".to_string()],
            changed_files_stat: changed_files_stat.to_string(),
            changed_files_list,
            gate_results_summary: gate_results_summary.to_string(),
            cost_usd,
            defects: None,
        }
    }
}

fn normalize_defects(raw: Option<String>) -> Option<String> {
    let d = raw?;
    let t = d.trim();
    if t.is_empty() || is_none_token(t) {
        None
    } else {
        Some(t.to_string())
    }
}

struct ParsedSections {
    completed_work: String,
    contract_deviations: Option<String>,
    tips_for_downstream: String,
    self_check_criteria: Vec<String>,
    defects: Option<String>,
}

fn try_parse_model_report(raw: &str) -> Option<ParsedSections> {
    // Requires at least a "### Completed" heading.
    if !raw.lines().any(|l| is_heading_for(l, REPORT_COMPLETED)) {
        return None;
    }

    let mut completed_work = String::new();
    let mut contract_deviations = None;
    let mut tips_for_downstream = String::new();
    let mut self_check_criteria = Vec::new();
    let mut defects = None;

    let mut current_section = 0; // 1: completed, 2: deviations, 3: tips, 4: criteria, 5: defects

    for line in raw.lines() {
        let trimmed = line.trim();
        if is_heading_for(trimmed, REPORT_COMPLETED) {
            current_section = 1;
            continue;
        } else if is_heading_for(trimmed, REPORT_DEVIATIONS) {
            current_section = 2;
            continue;
        } else if is_heading_for(trimmed, REPORT_TIPS) {
            current_section = 3;
            continue;
        } else if is_heading_for(trimmed, REPORT_SELF_CHECK) {
            current_section = 4;
            continue;
        } else if is_heading_for(trimmed, REPORT_DEFECTS) {
            current_section = 5;
            continue;
        } else if is_heading_for(trimmed, REPORT_CHANGES_LOCAL) || is_heading_for(trimmed, REPORT_GATES_LOCAL) {
            break;
        }

        match current_section {
            1 => {
                if !completed_work.is_empty() {
                    completed_work.push('\n');
                }
                completed_work.push_str(line);
            }
            2 => {
                let entry = contract_deviations.get_or_insert_with(String::new);
                if !entry.is_empty() {
                    entry.push('\n');
                }
                entry.push_str(line);
            }
            3 => {
                if !tips_for_downstream.is_empty() {
                    tips_for_downstream.push('\n');
                }
                tips_for_downstream.push_str(line);
            }
            4 => {
                if !trimmed.is_empty() {
                    let cleaned = trimmed.trim_start_matches('-').trim_start_matches('*').trim();
                    if !cleaned.is_empty() {
                        self_check_criteria.push(cleaned.to_string());
                    }
                }
            }
            5 => {
                let entry = defects.get_or_insert_with(String::new);
                if !entry.is_empty() {
                    entry.push('\n');
                }
                entry.push_str(line);
            }
            _ => {}
        }
    }

    if completed_work.trim().is_empty() {
        return None;
    }

    Some(ParsedSections {
        completed_work: completed_work.trim().to_string(),
        contract_deviations: contract_deviations.map(|s| s.trim().to_string()),
        tips_for_downstream: tips_for_downstream.trim().to_string(),
        self_check_criteria,
        defects: defects.map(|s| s.trim().to_string()),
    })
}

/// True when `line` is a Markdown heading whose text starts with `title` (case-insensitive).
fn is_heading_for(line: &str, title: &str) -> bool {
    let t = line.trim();
    if !t.starts_with('#') {
        return false;
    }
    t.trim_start_matches('#').trim().to_lowercase().starts_with(&title.to_lowercase())
}

/// True when a report section only holds the `none` sentinel.
pub fn is_none_token(text: &str) -> bool {
    text.trim().trim_matches(|c: char| c == '`' || c == '.' || c == '。').eq_ignore_ascii_case(REPORT_NONE)
}

fn extract_output_tail(output: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = output.lines().collect();
    if lines.len() <= max_lines {
        output.to_string()
    } else {
        lines[lines.len() - max_lines..].join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokage_core::planning::{TaskAcceptance, TaskSize};

    #[test]
    fn test_assemble_task_brief_contains_all_sections() {
        let task = TaskItem {
            id: "T2".to_string(),
            domain: "backend".to_string(),
            title: "User Routes".to_string(),
            goal: "Implement user endpoints".to_string(),
            depends_on: vec!["T1".to_string()],
            contracts: vec!["C1".to_string()],
            write_scope: vec!["src/users/".to_string()],
            deliverables: vec!["routes.rs".to_string()],
            acceptance: TaskAcceptance {
                commands: vec!["cargo test users".to_string()],
                criteria: vec!["Status 200 returned".to_string()],
                ..Default::default()
            },
            size: TaskSize::S,
            difficulty: 1,
            assigned_model: None,
            model_params: None,
        };

        let contracts = vec![PlanContract {
            id: "C1".to_string(),
            title: "API contract".to_string(),
            content: "GET /users -> 200".to_string(),
        }];

        let upstream = vec![UpstreamArtifact {
            task_id: "T1".to_string(),
            title: "DB migration".to_string(),
            report: "完成：数据库表结构建立".to_string(),
            changed_files: vec!["migrations/01.sql".to_string()],
        }];

        let brief = assemble_task_brief(&TaskBriefInput {
            task,
            contracts,
            upstream_artifacts: upstream,
            failure_evidence: Some("Previous attempt test failed".to_string()),
        });

        assert!(brief.contains(TASK_BRIEF_FIXED_HEADER));
        assert!(brief.contains("## Task"));
        assert!(brief.contains("T2"));
        assert!(brief.contains("## Contracts"));
        assert!(brief.contains("C1"));
        assert!(brief.contains("## Upstream results"));
        assert!(brief.contains("migrations/01.sql"));
        assert!(brief.contains("## Failure evidence"));
        assert!(brief.contains("## Task report format"));
    }

    #[test]
    fn test_parse_model_report_success() {
        let task = TaskItem {
            id: "T1".to_string(),
            domain: "backend".to_string(),
            title: "Setup DB".to_string(),
            goal: "".to_string(),
            depends_on: vec![],
            contracts: vec![],
            write_scope: vec![],
            deliverables: vec![],
            acceptance: TaskAcceptance::default(),
            size: TaskSize::S,
            difficulty: 1,
            assigned_model: None,
            model_params: None,
        };

        let raw = r#"
### What was completed
实现数据库连接池与用户表初始化。

### Deviations from contracts
None

### Hints for downstream
可直接通过 Repo::get_conn 获取连接。

### Self-check against acceptance criteria
- 用户表存在: 是
- 索引已建好: 是
"#;

        let report = parse_or_generate_task_report(
            &task,
            raw,
            "claude-3-7-sonnet",
            "att-123",
            true,
            "src/db/mod.rs +40 -0",
            vec!["src/db/mod.rs".to_string()],
            "cargo test pass",
            Some(0.04),
        );

        assert!(!report.is_locally_generated);
        assert!(report.completed_work.contains("实现数据库连接池"));
        assert_eq!(report.contract_deviations, None);
        assert!(report.tips_for_downstream.contains("Repo::get_conn"));
        assert_eq!(report.self_check_criteria.len(), 2);
        assert!(report.to_markdown().contains("First pass: yes"));
    }

    #[test]
    fn test_fallback_generates_local_report_on_missing_format() {
        let task = TaskItem {
            id: "T1".to_string(),
            domain: "backend".to_string(),
            title: "Setup DB".to_string(),
            goal: "".to_string(),
            depends_on: vec![],
            contracts: vec![],
            write_scope: vec![],
            deliverables: vec!["db.rs".to_string()],
            acceptance: TaskAcceptance::default(),
            size: TaskSize::S,
            difficulty: 1,
            assigned_model: None,
            model_params: None,
        };

        let raw = "Done! I modified the code.";

        let report = parse_or_generate_task_report(
            &task,
            raw,
            "claude-3-7-sonnet",
            "att-123",
            true,
            "src/db.rs +10 -2",
            vec!["src/db.rs".to_string()],
            "tests passed",
            Some(0.02),
        );

        assert!(report.is_locally_generated);
        assert!(report.to_markdown().contains("[locally generated]"));
        assert!(report.completed_work.contains("Done! I modified the code."));
    }
}
