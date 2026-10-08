//! Task brief and report domain models, assembly, and parsing (docs/05 task-brief.md, docs/03 §4, P7.m2).

use serde::{Deserialize, Serialize};
use tokage_core::planning::{PlanContract, TaskItem};

/// Task brief header defined verbatim in docs/05/task-brief.md §1.
pub const TASK_BRIEF_FIXED_HEADER: &str = r#"# Tokage Task Brief v1

## 执行规则
你是多代理流水线中的一名执行者。其他代理在并行完成其他任务，你看不到它们的上下文。
1. 严格遵守「契约」中的接口、类型与约定；需要偏离时，不要自行修改契约，在报告中说明。
2. 只修改「写入范围」内的文件。
3. 完成后运行「验收命令」自查。
4. 最后输出「任务报告」，格式见末尾，不超过 300 Token。
"#;

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
    out.push_str("## 任务\n");
    out.push_str(&format!("- 编号：{}  领域：{}\n", input.task.id, input.task.domain));
    out.push_str(&format!("- 标题：{}\n", input.task.title));
    out.push_str(&format!("- 目标：{}\n", input.task.goal));
    if !input.task.deliverables.is_empty() {
        out.push_str(&format!("- 交付物：{}\n", input.task.deliverables.join(", ")));
    }
    if !input.task.write_scope.is_empty() {
        out.push_str(&format!("- 写入范围：{}\n", input.task.write_scope.join(", ")));
    }
    if !input.task.acceptance.commands.is_empty() {
        out.push_str(&format!("- 验收命令：{}\n", input.task.acceptance.commands.join(" && ")));
    }
    if !input.task.acceptance.criteria.is_empty() {
        out.push_str(&format!("- 验收标准：{}\n", input.task.acceptance.criteria.join("; ")));
    }
    out.push('\n');

    // 3. Contracts
    out.push_str("## 契约\n");
    if input.contracts.is_empty() {
        out.push_str("无专属契约约束。\n");
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
    out.push_str("## 上游结果\n");
    if input.upstream_artifacts.is_empty() {
        out.push_str("无直接上游任务依赖。\n");
    } else {
        for u in &input.upstream_artifacts {
            out.push_str(&format!("### {} {}\n", u.task_id, u.title));
            out.push_str(&u.report);
            out.push('\n');
            if !u.changed_files.is_empty() {
                out.push_str(&format!("改动文件：{}\n", u.changed_files.join(", ")));
            }
            out.push('\n');
        }
    }

    // 5. Failure Evidence (if repairing)
    if let Some(evidence) = &input.failure_evidence {
        out.push_str("## 失败证据\n");
        out.push_str(evidence);
        out.push_str("\n\n");
    }

    // 6. Fixed Task Report Format Footer (R10: header above is fixed; footer may grow)
    out.push_str("## 任务报告格式\n");
    out.push_str("### 完成内容\n");
    out.push_str("### 偏离契约之处（无则写「无」）\n");
    out.push_str("### 给下游的提示\n");
    out.push_str("### 验收标准自查\n");
    // F13.e: testing tasks may report implementation defects
    if input.task.domain == "testing" {
        out.push_str("### 发现的缺陷\n");
        out.push_str("（可选；每行 `- <实现任务编号>: <描述>`，无则写「无」）\n");
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
        md.push_str(&format!("## {} {} · 完成\n", self.task_id, self.title));
        md.push_str(&format!(
            "模型：{}  执行窗口：{}  首次通过：{}{}\n\n",
            self.model,
            self.attempt_id,
            if self.first_pass { "是" } else { "否" },
            if self.is_locally_generated { "  [本地生成]" } else { "" }
        ));

        md.push_str("### 完成内容\n");
        md.push_str(&self.completed_work);
        md.push_str("\n\n");

        md.push_str("### 偏离契约之处\n");
        md.push_str(self.contract_deviations.as_deref().unwrap_or("无"));
        md.push_str("\n\n");

        md.push_str("### 给下游的提示\n");
        md.push_str(&self.tips_for_downstream);
        md.push_str("\n\n");

        md.push_str("### 验收标准自查\n");
        if self.self_check_criteria.is_empty() {
            md.push_str("无\n");
        } else {
            for c in &self.self_check_criteria {
                md.push_str(&format!("- {}\n", c));
            }
        }
        md.push('\n');

        if !self.changed_files_stat.trim().is_empty() {
            md.push_str("### 改动（本地）\n");
            md.push_str(&self.changed_files_stat);
            md.push_str("\n\n");
        }

        if !self.gate_results_summary.trim().is_empty() {
            md.push_str("### 验收关卡（本地）\n");
            md.push_str(&self.gate_results_summary);
            md.push('\n');
        }

        md
    }

    /// Converts this report into a compact upstream artifact passed to downstream briefs (docs/05 §2).
    /// Omits full diff stats and gate traces to save token budget.
    pub fn to_upstream_artifact(&self) -> UpstreamArtifact {
        let mut report_text = String::new();
        report_text.push_str(&format!("完成：{}\n", self.completed_work.trim()));
        if let Some(dev) = &self.contract_deviations {
            if !dev.trim().is_empty() && dev.trim() != "无" {
                report_text.push_str(&format!("偏离契约：{}\n", dev.trim()));
            }
        }
        if !self.tips_for_downstream.trim().is_empty() && self.tips_for_downstream.trim() != "无" {
            report_text.push_str(&format!("提示：{}\n", self.tips_for_downstream.trim()));
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
            let t = d.trim();
            !t.is_empty() && t != "无" && t != "none" && t != "None"
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
            format!("模型完成执行（无规范报告头），最后输出尾部：\n{}", tail)
        } else {
            format!("任务已执行完成，交付物：{}", task.deliverables.join(", "))
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
            tips_for_downstream: "请直接参考改动文件列表。".to_string(),
            self_check_criteria: vec!["自动验收关卡已执行".to_string()],
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
    if t.is_empty() || t == "无" || t.eq_ignore_ascii_case("none") {
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
    // Requires at least "### 完成内容" or "完成内容"
    if !raw.contains("完成内容") {
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
        if trimmed.starts_with('#') && trimmed.contains("完成内容") {
            current_section = 1;
            continue;
        } else if trimmed.starts_with('#') && trimmed.contains("偏离契约") {
            current_section = 2;
            continue;
        } else if trimmed.starts_with('#') && trimmed.contains("给下游的提示") {
            current_section = 3;
            continue;
        } else if trimmed.starts_with('#') && trimmed.contains("验收标准自查") {
            current_section = 4;
            continue;
        } else if trimmed.starts_with('#') && trimmed.contains("发现的缺陷") {
            current_section = 5;
            continue;
        } else if trimmed.starts_with("### 改动") || trimmed.starts_with("### 验收关卡") {
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
        assert!(brief.contains("## 任务"));
        assert!(brief.contains("T2"));
        assert!(brief.contains("## 契约"));
        assert!(brief.contains("C1"));
        assert!(brief.contains("## 上游结果"));
        assert!(brief.contains("migrations/01.sql"));
        assert!(brief.contains("## 失败证据"));
        assert!(brief.contains("## 任务报告格式"));
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
        };

        let raw = r#"
### 完成内容
实现数据库连接池与用户表初始化。

### 偏离契约之处
无

### 给下游的提示
可直接通过 Repo::get_conn 获取连接。

### 验收标准自查
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
        assert!(report.to_markdown().contains("首次通过：是"));
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
        assert!(report.to_markdown().contains("[本地生成]"));
        assert!(report.completed_work.contains("Done! I modified the code."));
    }
}
