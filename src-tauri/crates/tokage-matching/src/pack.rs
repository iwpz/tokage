//! Knowledge pack schema, parsing, and validation (docs/04 knowledge-pack.md).

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const SUPPORTED_SCHEMA_VERSION: u32 = 1;
pub static BUILTIN_KNOWLEDGE_JSON: &str = include_str!("../../../resources/knowledge/builtin.json");

#[derive(Debug, Error)]
pub enum PackError {
    #[error("JSON deserialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Unsupported schema version {version}, expected {SUPPORTED_SCHEMA_VERSION}")]
    UnsupportedSchemaVersion { version: u32 },

    #[error("Validation failed: {0}")]
    Validation(String),
}

/// Top-level Knowledge Pack structure.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KnowledgePack {
    pub schema_version: u32,
    pub pack_version: String,
    #[serde(default)]
    pub generated_at: Option<String>,
    #[serde(default)]
    pub domains: Vec<String>,
    pub success_model: SuccessModel,
    #[serde(default)]
    pub providers: HashMap<String, ProviderDef>,
    #[serde(default)]
    pub models: HashMap<String, ModelDef>,
    #[serde(default)]
    pub usage_profiles: HashMap<String, HashMap<String, UsageProfileEntry>>,
    #[serde(default)]
    pub triage_rules: Vec<serde_json::Value>,
    #[serde(default)]
    pub domain_keywords: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub usage_patterns: HashMap<String, Vec<serde_json::Value>>,
    #[serde(default)]
    pub time_formats: HashMap<String, String>,
    #[serde(default)]
    pub noise_rules: Vec<serde_json::Value>,
    #[serde(default)]
    pub gate_allowlist: Vec<String>,
    #[serde(default)]
    pub command_policy: Option<serde_json::Value>,
    #[serde(default)]
    pub credential_env: Option<serde_json::Value>,
    #[serde(default)]
    pub redaction_patterns: Vec<String>,
    #[serde(default)]
    pub sandbox_profiles: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SuccessModel {
    pub a: f64,
    pub b: f64,
    pub prior_strength: f64,
    /// Requirement matrix: difficulty ("1", "2", "3") -> size ("S", "M", "L") -> required score.
    pub req: HashMap<String, HashMap<String, f64>>,
    #[serde(default = "default_planner_min_skill")]
    pub planner_min_skill: f64,
    #[serde(default)]
    pub p_min: Option<PMinConfig>,
}

fn default_planner_min_skill() -> f64 {
    5.0
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PMinConfig {
    pub default: f64,
    pub difficulty_3: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderDef {
    pub billing: String,
    #[serde(default)]
    pub cache: Option<ProviderCacheDef>,
    #[serde(default)]
    pub context: Option<ProviderContextDef>,
    #[serde(default)]
    pub concurrency_hint: Option<u32>,
    #[serde(default)]
    pub env_allow: Option<Vec<String>>,
    #[serde(default)]
    pub proxy: Option<serde_json::Value>,
    #[serde(default)]
    pub quota: Option<serde_json::Value>,
    #[serde(default)]
    pub models: Vec<ProviderModelOffering>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderCacheDef {
    pub mode: String,
    #[serde(default)]
    pub ttl_seconds: Option<u64>,
    #[serde(default)]
    pub read_multiplier: Option<f64>,
    #[serde(default)]
    pub write_multiplier: Option<f64>,
    #[serde(default)]
    pub shared_baseline: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderContextDef {
    #[serde(default)]
    pub cli_auto_compact_ratio: Option<f64>,
    #[serde(default)]
    pub compact: Option<String>,
    #[serde(default)]
    pub compact_output_ratio: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderModelOffering {
    pub model: String,
    pub cli_model_arg: String,
    #[serde(default)]
    pub context_window: Option<u64>,
    #[serde(default)]
    pub effective_context: Option<u64>,
    #[serde(default)]
    pub credit_multiplier: Option<f64>,
    #[serde(default = "default_true")]
    pub cli_model_arg_verified: bool,
    #[serde(default)]
    pub skills: Option<HashMap<String, f64>>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelDef {
    pub display_name: String,
    #[serde(default)]
    pub vendor: Option<String>,
    #[serde(default)]
    pub roles: Vec<String>,
    #[serde(default)]
    pub skills: HashMap<String, f64>,
    pub price_per_mtok: PricePerMtok,
    pub context_window: u64,
    pub effective_context: u64,
    #[serde(default)]
    pub speed: Option<f64>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub released: Option<String>,
    #[serde(default)]
    pub max_output: Option<u64>,
    #[serde(default)]
    pub cache_rates: Option<CacheRatesDef>,
    #[serde(default)]
    pub long_context_pricing: Option<LongContextPricingDef>,
    #[serde(default)]
    pub price_schedule: Option<serde_json::Value>,
    #[serde(default)]
    pub benchmarks: Option<HashMap<String, f64>>,
    #[serde(default)]
    pub price_estimated: Option<bool>,
    #[serde(default)]
    pub skills_estimated: Option<bool>,
    #[serde(default)]
    pub deprecated: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PricePerMtok {
    pub input: f64,
    pub output: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CacheRatesDef {
    pub read_multiplier: f64,
    pub write_multiplier: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LongContextPricingDef {
    pub threshold: u64,
    pub input_multiplier: f64,
    pub output_multiplier: f64,
}

fn deserialize_flexible_u64<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Num {
        Int(u64),
        Float(f64),
    }
    match Num::deserialize(deserializer)? {
        Num::Int(i) => Ok(i),
        Num::Float(f) => Ok(f.round() as u64),
    }
}

fn deserialize_flexible_u32<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Num {
        Int(u32),
        Float(f64),
    }
    match Num::deserialize(deserializer)? {
        Num::Int(i) => Ok(i),
        Num::Float(f) => Ok(f.round() as u32),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UsageProfileEntry {
    #[serde(deserialize_with = "deserialize_flexible_u32")]
    pub turns: u32,
    #[serde(deserialize_with = "deserialize_flexible_u32")]
    pub calls_per_turn: u32,
    #[serde(deserialize_with = "deserialize_flexible_u64")]
    pub growth_per_turn: u64,
    #[serde(deserialize_with = "deserialize_flexible_u64")]
    pub output_per_turn: u64,
}

/// Parses and validates a knowledge pack JSON string.
pub fn parse_knowledge_pack(json_str: &str) -> Result<KnowledgePack, PackError> {
    let pack: KnowledgePack = serde_json::from_str(json_str)?;
    validate_knowledge_pack(&pack)?;
    Ok(pack)
}

/// Validates structural integrity, score ranges, and cross-references of a knowledge pack.
pub fn validate_knowledge_pack(pack: &KnowledgePack) -> Result<(), PackError> {
    if pack.schema_version != SUPPORTED_SCHEMA_VERSION {
        return Err(PackError::UnsupportedSchemaVersion {
            version: pack.schema_version,
        });
    }

    if pack.pack_version.trim().is_empty() {
        return Err(PackError::Validation("pack_version cannot be empty".into()));
    }

    // Validate models
    for (model_id, model) in &pack.models {
        if model.context_window == 0 {
            return Err(PackError::Validation(format!(
                "model {model_id} context_window must be > 0"
            )));
        }
        if model.effective_context > model.context_window {
            return Err(PackError::Validation(format!(
                "model {model_id} effective_context ({}) > context_window ({})",
                model.effective_context, model.context_window
            )));
        }
        if model.price_per_mtok.input < 0.0 || model.price_per_mtok.output < 0.0 {
            return Err(PackError::Validation(format!(
                "model {model_id} price_per_mtok must be non-negative"
            )));
        }
        for (domain, &score) in &model.skills {
            if !(1.0..=5.0).contains(&score) {
                return Err(PackError::Validation(format!(
                    "model {model_id} domain {domain} skill {score} outside [1, 5]"
                )));
            }
        }
    }

    // Validate providers and their model references
    for (provider_id, provider) in &pack.providers {
        let mut seen = HashSet::new();
        for offering in &provider.models {
            if !pack.models.contains_key(&offering.model) {
                return Err(PackError::Validation(format!(
                    "provider {provider_id} references unknown model '{}'",
                    offering.model
                )));
            }
            if !seen.insert(&offering.model) {
                return Err(PackError::Validation(format!(
                    "provider {provider_id} contains duplicate offering for model '{}'",
                    offering.model
                )));
            }
            let base_model = &pack.models[&offering.model];
            if let Some(ctx) = offering.context_window {
                if ctx > base_model.context_window {
                    return Err(PackError::Validation(format!(
                        "provider {provider_id} offering '{}' context_window ({ctx}) exceeds base model ({})",
                        offering.model, base_model.context_window
                    )));
                }
            }
            if let Some(eff) = offering.effective_context {
                let max_ctx = offering.context_window.unwrap_or(base_model.context_window);
                if eff > max_ctx {
                    return Err(PackError::Validation(format!(
                        "provider {provider_id} offering '{}' effective_context ({eff}) exceeds context_window ({max_ctx})",
                        offering.model
                    )));
                }
            }
            if let Some(ref skills) = offering.skills {
                for (domain, &score) in skills {
                    if !(1.0..=5.0).contains(&score) {
                        return Err(PackError::Validation(format!(
                            "provider {provider_id} offering '{}' skill for {domain} ({score}) outside [1, 5]",
                            offering.model
                        )));
                    }
                }
            }
        }
    }

    // Validate success_model requirements
    for (diff, sizes) in &pack.success_model.req {
        for (sz, &req_score) in sizes {
            if req_score <= 0.0 {
                return Err(PackError::Validation(format!(
                    "success_model.req[{diff}][{sz}] must be > 0"
                )));
            }
        }
    }

    Ok(())
}

/// Returns the singleton parsed builtin knowledge pack.
pub fn builtin_knowledge_pack() -> &'static KnowledgePack {
    static BUILTIN: OnceLock<KnowledgePack> = OnceLock::new();
    BUILTIN.get_or_init(|| {
        parse_knowledge_pack(BUILTIN_KNOWLEDGE_JSON)
            .expect("builtin.json must parse and validate cleanly")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_pack_parses_and_validates() {
        let pack = builtin_knowledge_pack();
        assert_eq!(pack.schema_version, 1);
        assert!(!pack.pack_version.is_empty());
        assert!(!pack.domains.is_empty());
        assert!(!pack.models.is_empty());
        assert!(!pack.providers.is_empty());
        assert!(!pack.usage_profiles.is_empty());
        assert!(pack.success_model.req.contains_key("1"));
        assert!(pack.success_model.req.contains_key("2"));
        assert!(pack.success_model.req.contains_key("3"));
    }

    #[test]
    fn test_invalid_schema_version_rejected() {
        let bad_json = r#"{
            "schema_version": 99,
            "pack_version": "test",
            "success_model": { "a": 1.6, "b": 1.2, "prior_strength": 6, "req": {} }
        }"#;
        let res = parse_knowledge_pack(bad_json);
        assert!(res.is_err());
        match res.unwrap_err() {
            PackError::UnsupportedSchemaVersion { version } => assert_eq!(version, 99),
            other => panic!("expected UnsupportedSchemaVersion, got {:?}", other),
        }
    }

    #[test]
    fn test_invalid_skill_score_rejected() {
        let mut pack = builtin_knowledge_pack().clone();
        pack.models.get_mut("claude-opus-5-5").unwrap().skills.insert("planning".into(), 6.5);
        let res = validate_knowledge_pack(&pack);
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("outside [1, 5]"));
    }

    #[test]
    fn test_provider_unknown_model_rejected() {
        let mut pack = builtin_knowledge_pack().clone();
        pack.providers.get_mut("CLAUDE_CODE").unwrap().models.push(ProviderModelOffering {
            model: "non-existent-model".into(),
            cli_model_arg: "non-existent".into(),
            context_window: None,
            effective_context: None,
            credit_multiplier: None,
            cli_model_arg_verified: false,
            skills: None,
        });
        let res = validate_knowledge_pack(&pack);
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("unknown model"));
    }
}
