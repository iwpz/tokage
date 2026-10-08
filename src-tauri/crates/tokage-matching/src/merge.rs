//! Layered merging for knowledge packs (docs/04 knowledge-pack.md §6).
//!
//! Merging precedence: builtin -> official -> custom (in order) -> override -> learned -> effective.
//! Security keys (command_policy.forbidden, forbidden_paths, credential_env, sandbox_profiles)
//! can only be tightened, NEVER loosened or removed.

use std::collections::{HashMap, HashSet};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::pack::{
    KnowledgePack, ModelDef, PricePerMtok, ProviderDef, ProviderModelOffering, SuccessModel,
    UsageProfileEntry,
};

#[derive(Debug, Error)]
pub enum MergeError {
    #[error("Security violation: {0}")]
    SecurityViolation(String),

    #[error("Validation error: {0}")]
    Validation(String),
}

/// Identifies a configuration layer.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LayerOrigin {
    Builtin,
    Official,
    Custom(String),
    Override,
    Learned,
}

impl std::fmt::Display for LayerOrigin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LayerOrigin::Builtin => write!(f, "builtin"),
            LayerOrigin::Official => write!(f, "official"),
            LayerOrigin::Custom(name) => write!(f, "custom:{}", name),
            LayerOrigin::Override => write!(f, "override"),
            LayerOrigin::Learned => write!(f, "learned"),
        }
    }
}

/// A model override record from SQLite `model_overrides` table.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelOverride {
    pub provider_name: String,
    pub model_id: String,
    pub patch: serde_json::Value,
}

/// Input container with all layers to be merged.
#[derive(Debug, Clone)]
pub struct LayeredPacks {
    pub builtin: KnowledgePack,
    pub official: Option<KnowledgePack>,
    pub customs: Vec<(String, KnowledgePack)>,
    pub overrides: Vec<ModelOverride>,
    pub learned_usage_profiles: Option<HashMap<String, HashMap<String, UsageProfileEntry>>>,
}

/// Fully expanded candidate representing a `(provider, model)` pair.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EffectiveCandidate {
    pub provider: String,
    pub model_id: String,
    pub display_name: String,
    pub cli_model_arg: String,
    pub billing: String,
    pub roles: Vec<String>,
    pub skills: HashMap<String, f64>,
    pub price_per_mtok: PricePerMtok,
    pub context_window: u64,
    pub effective_context: u64,
    pub credit_multiplier: Option<f64>,
    pub verified: bool,
    pub speed: Option<f64>,
    pub tags: Vec<String>,
    pub origin_layer: String,
}

/// Effective merged knowledge pack view consumed by the matching engine.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EffectiveKnowledgePack {
    pub pack_version: String,
    pub domains: Vec<String>,
    pub success_model: SuccessModel,
    pub models: HashMap<String, ModelDef>,
    pub providers: HashMap<String, ProviderDef>,
    pub candidates: Vec<EffectiveCandidate>,
    pub usage_profiles: HashMap<String, HashMap<String, UsageProfileEntry>>,
    pub forbidden_paths: Vec<String>,
    pub command_policy_forbidden: Vec<String>,
    pub credential_env: Option<serde_json::Value>,
    pub sandbox_profiles: HashMap<String, serde_json::Value>,
    pub triage_rules: Vec<serde_json::Value>,
    pub domain_keywords: HashMap<String, Vec<String>>,
    pub usage_patterns: HashMap<String, Vec<serde_json::Value>>,
    pub time_formats: HashMap<String, String>,
    pub noise_rules: Vec<serde_json::Value>,
    pub gate_allowlist: Vec<String>,
    pub redaction_patterns: Vec<String>,
    /// Tracks the provenance layer for each model.
    pub model_origins: HashMap<String, String>,
}

/// Merges all knowledge pack layers into a single effective pack.
pub fn merge_layers(input: &LayeredPacks) -> Result<EffectiveKnowledgePack, MergeError> {
    // 1. Base is builtin, or official if present.
    let base_layer = input
        .official
        .as_ref()
        .unwrap_or(&input.builtin);
    let base_origin = if input.official.is_some() {
        LayerOrigin::Official
    } else {
        LayerOrigin::Builtin
    };

    let mut pack_version = base_layer.pack_version.clone();
    let mut domains = base_layer.domains.clone();
    let mut success_model = base_layer.success_model.clone();
    let mut models = base_layer.models.clone();
    let mut providers = base_layer.providers.clone();
    let mut usage_profiles = base_layer.usage_profiles.clone();
    let triage_rules = base_layer.triage_rules.clone();
    let domain_keywords = base_layer.domain_keywords.clone();
    let usage_patterns = base_layer.usage_patterns.clone();
    let time_formats = base_layer.time_formats.clone();
    let noise_rules = base_layer.noise_rules.clone();
    let gate_allowlist = base_layer.gate_allowlist.clone();
    let mut redaction_patterns = base_layer.redaction_patterns.clone();
    let credential_env = base_layer.credential_env.clone();
    let mut sandbox_profiles = base_layer.sandbox_profiles.clone();

    // Security baseline sets
    let mut forbidden_paths_set: HashSet<String> = extract_forbidden_paths(base_layer);
    let mut command_forbidden_set: HashSet<String> = extract_command_forbidden(base_layer);

    let mut model_origins: HashMap<String, String> = models
        .keys()
        .map(|k| (k.clone(), base_origin.to_string()))
        .collect();

    // 2. Apply Custom packs in sequence
    for (name, custom) in &input.customs {
        let origin = LayerOrigin::Custom(name.clone());
        pack_version = format!("{}+{}", pack_version, name);

        // Merge domains
        for d in &custom.domains {
            if !domains.contains(d) {
                domains.push(d.clone());
            }
        }

        // Merge success model (if provided with non-empty req)
        if !custom.success_model.req.is_empty() {
            success_model = custom.success_model.clone();
        }

        // Merge models
        for (mid, custom_model) in &custom.models {
            models.insert(mid.clone(), custom_model.clone());
            model_origins.insert(mid.clone(), origin.to_string());
        }

        // Merge providers
        for (pid, custom_prov) in &custom.providers {
            if let Some(existing_prov) = providers.get_mut(pid) {
                // Update offerings
                for custom_offering in &custom_prov.models {
                    if let Some(pos) = existing_prov
                        .models
                        .iter()
                        .position(|o| o.model == custom_offering.model)
                    {
                        existing_prov.models[pos] = custom_offering.clone();
                    } else {
                        existing_prov.models.push(custom_offering.clone());
                    }
                }
                if let Some(ref c) = custom_prov.cache {
                    existing_prov.cache = Some(c.clone());
                }
                if let Some(ref ctx) = custom_prov.context {
                    existing_prov.context = Some(ctx.clone());
                }
            } else {
                providers.insert(pid.clone(), custom_prov.clone());
            }
        }

        // Merge usage profiles
        for (d, sizes) in &custom.usage_profiles {
            let entry = usage_profiles.entry(d.clone()).or_default();
            for (sz, prof) in sizes {
                entry.insert(sz.clone(), prof.clone());
            }
        }

        // Security key tightening:
        // Any forbidden paths defined in custom can ONLY add to the baseline
        let custom_forbidden_paths = extract_forbidden_paths(custom);
        forbidden_paths_set.extend(custom_forbidden_paths);

        // Any command_policy.forbidden in custom can ONLY add to the baseline
        let custom_cmd_forbidden = extract_command_forbidden(custom);
        command_forbidden_set.extend(custom_cmd_forbidden);

        // Redaction patterns can only append
        for pat in &custom.redaction_patterns {
            if !redaction_patterns.contains(pat) {
                redaction_patterns.push(pat.clone());
            }
        }

        // Merge sandbox profiles: add new ones or tighten existing deny rules
        for (cli, profile) in &custom.sandbox_profiles {
            sandbox_profiles.insert(cli.clone(), profile.clone());
        }
    }

    // 3. Apply Overrides
    for ov in &input.overrides {
        let origin = LayerOrigin::Override;
        if ov.provider_name == "*" {
            // Overrides model directly
            if let Some(model) = models.get_mut(&ov.model_id) {
                apply_model_patch(model, &ov.patch);
                model_origins.insert(ov.model_id.clone(), origin.to_string());
            }
        } else {
            // Overrides provider offering
            if let Some(prov) = providers.get_mut(&ov.provider_name) {
                if let Some(offering) = prov.models.iter_mut().find(|o| o.model == ov.model_id) {
                    apply_offering_patch(offering, &ov.patch);
                }
            }
        }
    }

    // 4. Apply Learned stats
    if let Some(ref learned_profiles) = input.learned_usage_profiles {
        for (d, sizes) in learned_profiles {
            let entry = usage_profiles.entry(d.clone()).or_default();
            for (sz, prof) in sizes {
                entry.insert(sz.clone(), prof.clone());
            }
        }
    }

    // Expand candidates
    let mut candidates = Vec::new();
    for (pid, prov) in &providers {
        for offering in &prov.models {
            if let Some(base_model) = models.get(&offering.model) {
                // Skills: base model skills overridden by offering skills_patch
                let mut skills = base_model.skills.clone();
                if let Some(ref patch) = offering.skills {
                    for (d, &score) in patch {
                        skills.insert(d.clone(), score);
                    }
                }

                let context_window = offering
                    .context_window
                    .unwrap_or(base_model.context_window)
                    .min(base_model.context_window);

                let effective_context = offering
                    .effective_context
                    .unwrap_or(base_model.effective_context)
                    .min(base_model.effective_context);

                let origin = model_origins
                    .get(&offering.model)
                    .cloned()
                    .unwrap_or_else(|| "builtin".into());

                candidates.push(EffectiveCandidate {
                    provider: pid.clone(),
                    model_id: offering.model.clone(),
                    display_name: base_model.display_name.clone(),
                    cli_model_arg: offering.cli_model_arg.clone(),
                    billing: prov.billing.clone(),
                    roles: base_model.roles.clone(),
                    skills,
                    price_per_mtok: base_model.price_per_mtok.clone(),
                    context_window,
                    effective_context,
                    credit_multiplier: offering.credit_multiplier,
                    verified: offering.cli_model_arg_verified,
                    speed: base_model.speed,
                    tags: base_model.tags.clone(),
                    origin_layer: origin,
                });
            }
        }
    }

    let mut forbidden_paths: Vec<String> = forbidden_paths_set.into_iter().collect();
    forbidden_paths.sort();
    let mut command_policy_forbidden: Vec<String> = command_forbidden_set.into_iter().collect();
    command_policy_forbidden.sort();

    Ok(EffectiveKnowledgePack {
        pack_version,
        domains,
        success_model,
        models,
        providers,
        candidates,
        usage_profiles,
        forbidden_paths,
        command_policy_forbidden,
        credential_env,
        sandbox_profiles,
        triage_rules,
        domain_keywords,
        usage_patterns,
        time_formats,
        noise_rules,
        gate_allowlist,
        redaction_patterns,
        model_origins,
    })
}

fn extract_forbidden_paths(pack: &KnowledgePack) -> HashSet<String> {
    let mut set = HashSet::new();
    if let Some(ref cp) = pack.command_policy {
        if let Some(paths) = cp.get("forbidden_paths").and_then(|v| v.as_array()) {
            for p in paths {
                if let Some(s) = p.as_str() {
                    set.insert(s.to_string());
                }
            }
        }
    }
    set
}

fn extract_command_forbidden(pack: &KnowledgePack) -> HashSet<String> {
    let mut set = HashSet::new();
    if let Some(ref cp) = pack.command_policy {
        if let Some(rules) = cp.get("forbidden").and_then(|v| v.as_array()) {
            for r in rules {
                if let Some(s) = r.as_str() {
                    set.insert(s.to_string());
                }
            }
        }
    }
    set
}

fn apply_model_patch(model: &mut ModelDef, patch: &serde_json::Value) {
    if let Some(obj) = patch.as_object() {
        if let Some(name) = obj.get("display_name").and_then(|v| v.as_str()) {
            model.display_name = name.to_string();
        }
        if let Some(skills_obj) = obj.get("skills").and_then(|v| v.as_object()) {
            for (k, v) in skills_obj {
                if let Some(n) = v.as_f64() {
                    model.skills.insert(k.clone(), n.clamp(1.0, 5.0));
                }
            }
        }
        if let Some(p) = obj.get("price_per_mtok").and_then(|v| v.as_object()) {
            if let (Some(i), Some(o)) = (p.get("input").and_then(|v| v.as_f64()), p.get("output").and_then(|v| v.as_f64())) {
                model.price_per_mtok.input = i;
                model.price_per_mtok.output = o;
            }
        }
    }
}

fn apply_offering_patch(offering: &mut ProviderModelOffering, patch: &serde_json::Value) {
    if let Some(obj) = patch.as_object() {
        if let Some(arg) = obj.get("cli_model_arg").and_then(|v| v.as_str()) {
            offering.cli_model_arg = arg.to_string();
        }
        if let Some(skills_obj) = obj.get("skills").and_then(|v| v.as_object()) {
            let mut s = offering.skills.clone().unwrap_or_default();
            for (k, v) in skills_obj {
                if let Some(n) = v.as_f64() {
                    s.insert(k.clone(), n.clamp(1.0, 5.0));
                }
            }
            offering.skills = Some(s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::builtin_knowledge_pack;

    #[test]
    fn test_merge_builtin_only() {
        let builtin = builtin_knowledge_pack().clone();
        let layers = LayeredPacks {
            builtin,
            official: None,
            customs: Vec::new(),
            overrides: Vec::new(),
            learned_usage_profiles: None,
        };

        let effective = merge_layers(&layers).unwrap();
        assert_eq!(effective.pack_version, "builtin-0.0.2");
        assert!(!effective.candidates.is_empty());
        assert!(!effective.command_policy_forbidden.is_empty());

        // Check candidate properties
        let opus_claude = effective
            .candidates
            .iter()
            .find(|c| c.provider == "CLAUDE_CODE" && c.model_id == "claude-opus-5-5")
            .unwrap();
        assert_eq!(opus_claude.display_name, "Claude Opus 5.5");
        assert_eq!(opus_claude.origin_layer, "builtin");
    }

    #[test]
    fn test_security_keys_cannot_be_loosened() {
        let builtin = builtin_knowledge_pack().clone();
        let baseline_forbidden = extract_command_forbidden(&builtin);
        assert!(!baseline_forbidden.is_empty());

        // A custom pack that defines empty forbidden rules or tries to omit them
        let mut empty_custom = builtin.clone();
        empty_custom.command_policy = Some(serde_json::json!({
            "forbidden": [],
            "forbidden_paths": []
        }));

        let layers = LayeredPacks {
            builtin: builtin.clone(),
            official: None,
            customs: vec![("empty-pack".into(), empty_custom)],
            overrides: Vec::new(),
            learned_usage_profiles: None,
        };

        let effective = merge_layers(&layers).unwrap();
        // Baseline forbidden commands MUST still be present
        for cmd in &baseline_forbidden {
            assert!(
                effective.command_policy_forbidden.contains(cmd),
                "Baseline forbidden rule {} was lost!",
                cmd
            );
        }
    }

    #[test]
    fn test_security_keys_can_be_tightened() {
        let builtin = builtin_knowledge_pack().clone();

        let mut custom = builtin.clone();
        custom.command_policy = Some(serde_json::json!({
            "forbidden": ["custom_dangerous_command *"],
            "forbidden_paths": ["~/.super_secret/**"]
        }));

        let layers = LayeredPacks {
            builtin,
            official: None,
            customs: vec![("tighten-pack".into(), custom)],
            overrides: Vec::new(),
            learned_usage_profiles: None,
        };

        let effective = merge_layers(&layers).unwrap();
        assert!(effective.command_policy_forbidden.contains(&"custom_dangerous_command *".to_string()));
        assert!(effective.forbidden_paths.contains(&"~/.super_secret/**".to_string()));
    }

    #[test]
    fn test_override_and_origin_tracking() {
        let builtin = builtin_knowledge_pack().clone();

        let layers = LayeredPacks {
            builtin,
            official: None,
            customs: Vec::new(),
            overrides: vec![ModelOverride {
                provider_name: "*".into(),
                model_id: "claude-opus-5-5".into(),
                patch: serde_json::json!({
                    "skills": { "testing": 5.0 },
                    "display_name": "Claude Opus 5.5 (Customized)"
                }),
            }],
            learned_usage_profiles: None,
        };

        let effective = merge_layers(&layers).unwrap();
        let opus = effective.models.get("claude-opus-5-5").unwrap();
        assert_eq!(opus.display_name, "Claude Opus 5.5 (Customized)");
        assert_eq!(opus.skills.get("testing"), Some(&5.0));

        let opus_candidate = effective
            .candidates
            .iter()
            .find(|c| c.model_id == "claude-opus-5-5")
            .unwrap();
        assert_eq!(opus_candidate.display_name, "Claude Opus 5.5 (Customized)");
        assert_eq!(opus_candidate.origin_layer, "override");
    }
}
