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

