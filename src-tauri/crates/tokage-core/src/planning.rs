//! Domain models for planning engine: repo digest, triage, and plan structures (docs/02).

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// Build, test, check and lint commands discovered in the project (docs/02 §3, P5.m1b).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectCommands {
    pub build: Vec<String>,
    pub test: Vec<String>,
    pub check: Vec<String>,
    pub lint: Vec<String>,
}

impl ProjectCommands {
    /// Returns all unique discovered commands as a flat list.
    pub fn all_commands(&self) -> Vec<String> {
        let mut list = Vec::new();
        for cmd in self.build.iter().chain(&self.test).chain(&self.check).chain(&self.lint) {
            if !list.contains(cmd) {
                list.push(cmd.clone());
            }
        }
        list
    }
}

/// Discovered manifest file summary (docs/02 §3, P5.m1a).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManifestSummary {
    pub manifest_type: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_manager: Option<String>,
    pub dependencies: Vec<String>,
    pub dev_dependencies: Vec<String>,
    pub scripts: HashMap<String, String>,
}

/// Git hotspot item representing a frequently changed file (docs/02 §3, P5.m1c).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HotspotItem {
    pub path: String,
    pub commit_count: u32,
}

/// Complete project digest (RepoDigest) generated locally with zero tokens (docs/02 §3, P5.m1).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RepoDigest {
    pub project_path: String,
    pub generated_at: String,
    pub primary_languages: Vec<String>,
    pub manifests: Vec<ManifestSummary>,
    pub commands: ProjectCommands,
    pub conventions_summary: String,
    pub tree_summary: String,
    pub public_symbols: Vec<String>,
    pub hotspots: Vec<HotspotItem>,
    pub estimated_tokens: usize,
    /// Rendered Markdown text trimmed to <= 8K tokens for prompt injection.
    pub markdown: String,
}

/// Task sizing for cost and time estimation (docs/02 plan-format.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum TaskSize {
    S,
    M,
    L,
}

impl Default for TaskSize {
    fn default() -> Self {
        Self::M
    }
}

/// Acceptance commands and criteria for a task (docs/02 plan-format.md).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskAcceptance {
    #[serde(default)]
    pub commands: Vec<String>,
    #[serde(default)]
    pub criteria: Vec<String>,
}

/// A structured task item in the Tasks YAML block (docs/02 plan-format.md).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskItem {
    pub id: String,
    pub domain: String,
    pub title: String,
    pub goal: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub contracts: Vec<String>,
    #[serde(default)]
    pub write_scope: Vec<String>,
    #[serde(default)]
    pub deliverables: Vec<String>,
    #[serde(default)]
    pub acceptance: TaskAcceptance,
    #[serde(default)]
    pub size: TaskSize,
    #[serde(default = "default_difficulty")]
    pub difficulty: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assigned_model: Option<String>,
}

fn default_difficulty() -> u8 {
    2
}

/// Structured plan specification: target, scope, decisions (docs/02 plan-format.md).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanSpec {
    pub raw: String,
    pub goal: String,
    pub in_scope: Vec<String>,
    pub out_of_scope: Vec<String>,
    pub decisions: Vec<String>,
}

/// A contract section defined in the plan (docs/02 plan-format.md).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanContract {
    pub id: String,
    pub title: String,
    pub content: String,
}

/// A parsed, structured Plan adhering to docs/02/plan-format.md.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub spec: PlanSpec,
    pub contracts: Vec<PlanContract>,
    pub tasks: Vec<TaskItem>,
    pub notes: Option<String>,
    pub raw_markdown: String,
}

/// Routing decision made by local triage (docs/02 §2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum TriageRoute {
    Direct,
    Plan,
}

impl TriageRoute {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Direct => "DIRECT",
            Self::Plan => "PLAN",
        }
    }
}

/// A matched rule in local triage explaining why a route was favored (docs/02 §2).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriageRuleMatch {
    pub rule_id: String,
    pub toward: TriageRoute,
    pub weight: f64,
    pub reason: String,
}

/// Complete output from local triage (docs/02 §2, P5.m4).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriageResult {
    pub route: TriageRoute,
    pub confidence: f64,
    pub detected_domains: Vec<String>,
    pub matched_rules: Vec<TriageRuleMatch>,
    pub estimated_files: Vec<String>,
    pub score_plan: f64,
    pub score_direct: f64,
    pub reason: String,
}

