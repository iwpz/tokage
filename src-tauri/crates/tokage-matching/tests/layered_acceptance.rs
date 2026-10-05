//! P6.m1 Acceptance Tests: Layered merging and security key tightening (docs/04 §6).

use std::collections::HashMap;
use tokage_matching::{
    builtin_knowledge_pack, merge_layers, LayeredPacks, ModelOverride, UsageProfileEntry,
};

#[test]
fn test_acceptance_five_layer_merging_pipeline() {
    let builtin = builtin_knowledge_pack().clone();

    // Layer 2: Official update
    let mut official = builtin.clone();
    official.pack_version = "official-2026.10.15".into();
    // Official modifies claude-opus-5-5 speed
    official.models.get_mut("claude-opus-5-5").unwrap().speed = Some(4.0);

    // Layer 3: Custom pack 1 (community pack)
    let mut custom1 = builtin.clone();
    custom1.pack_version = "custom-pack-alpha".into();
    // Custom pack 1 adds a new model
    let mut custom_model = custom1.models.get("claude-haiku-4-5").unwrap().clone();
    custom_model.display_name = "Community Fine-tune 1".into();
    custom1.models.insert("community-ft-1".into(), custom_model);
    custom1.providers.get_mut("CLAUDE_CODE").unwrap().models.push(
        tokage_matching::ProviderModelOffering {
            model: "community-ft-1".into(),
            cli_model_arg: "ft-1".into(),
            context_window: Some(200_000),
            effective_context: Some(100_000),
            credit_multiplier: None,
            cli_model_arg_verified: true,
            skills: None,
        },
    );

    // Layer 3: Custom pack 2 (tightened security rules)
    let mut custom2 = builtin.clone();
    custom2.pack_version = "custom-security-tighten".into();
    custom2.command_policy = Some(serde_json::json!({
        "forbidden": ["curl * | sh", "wget * | bash"],
        "forbidden_paths": ["~/.aws/**", "~/.kube/**"]
    }));

    // Layer 4: User Override
    let overrides = vec![
        ModelOverride {
            provider_name: "*".into(),
            model_id: "claude-opus-5-5".into(),
            patch: serde_json::json!({
                "display_name": "Claude Opus 5.5 (User Tuned)",
                "skills": { "planning": 5.0, "algorithm": 5.0 }
            }),
        },
        ModelOverride {
            provider_name: "CLAUDE_CODE".into(),
            model_id: "claude-opus-5-5".into(),
            patch: serde_json::json!({
                "cli_model_arg": "opus-override"
            }),
        },
    ];

    // Layer 5: Learned usage profile
    let mut learned_profiles = HashMap::new();
    let mut planning_sizes = HashMap::new();
    planning_sizes.insert(
        "S".into(),
        UsageProfileEntry {
            turns: 1,
            calls_per_turn: 7, // Learned from experience
            growth_per_turn: 13500,
            output_per_turn: 6500,
        },
    );
    learned_profiles.insert("planning".into(), planning_sizes);

    let layers = LayeredPacks {
        builtin: builtin.clone(),
        official: Some(official),
        customs: vec![
            ("alpha".into(), custom1),
            ("sec".into(), custom2),
        ],
        overrides,
        learned_usage_profiles: Some(learned_profiles),
    };

    let effective = merge_layers(&layers).expect("Layered merge must succeed");

    // 1. Pack version reflects official base + custom pack chain
    assert!(effective.pack_version.starts_with("official-2026.10.15+alpha+sec"));

    // 2. Custom model from custom1 exists
    assert!(effective.models.contains_key("community-ft-1"));
    let ft1_cand = effective
        .candidates
        .iter()
        .find(|c| c.model_id == "community-ft-1")
        .expect("community-ft-1 candidate expanded");
    assert_eq!(ft1_cand.origin_layer, "custom:alpha");

    // 3. User override applied to claude-opus-5-5
    let opus_cand = effective
        .candidates
        .iter()
        .find(|c| c.provider == "CLAUDE_CODE" && c.model_id == "claude-opus-5-5")
        .expect("claude-opus-5-5 candidate exists");
    assert_eq!(opus_cand.display_name, "Claude Opus 5.5 (User Tuned)");
    assert_eq!(opus_cand.cli_model_arg, "opus-override");
    assert_eq!(opus_cand.origin_layer, "override");

    // 4. Learned usage profile took effect
    assert_eq!(
        effective.usage_profiles["planning"]["S"].calls_per_turn,
        7
    );

    // 5. Security key tightening: baseline forbidden rules + newly added rules
    assert!(effective.command_policy_forbidden.contains(&"curl * | sh".to_string()));
    assert!(effective.command_policy_forbidden.contains(&"wget * | bash".to_string()));
    assert!(effective.forbidden_paths.contains(&"~/.aws/**".to_string()));
    assert!(effective.forbidden_paths.contains(&"~/.kube/**".to_string()));

    // Verify all baseline forbidden commands are preserved
    assert!(effective.command_policy_forbidden.iter().any(|r| r.contains("rm -rf /")));
}

#[test]
fn test_acceptance_security_keys_cannot_be_loosened_by_any_layer() {
    let builtin = builtin_knowledge_pack().clone();

    // Adversarial custom pack attempting to wipe out forbidden commands & paths
    let mut rogue_pack = builtin.clone();
    rogue_pack.pack_version = "rogue-override".into();
    rogue_pack.command_policy = Some(serde_json::json!({
        "forbidden": [],
        "forbidden_paths": []
    }));

    let layers = LayeredPacks {
        builtin: builtin.clone(),
        official: None,
        customs: vec![("rogue".into(), rogue_pack)],
        overrides: Vec::new(),
        learned_usage_profiles: None,
    };

    let effective = merge_layers(&layers).expect("Merge succeeds with security enforcement");

    // Baseline security rules must not be emptied or reduced
    assert!(!effective.command_policy_forbidden.is_empty(), "Forbidden commands were wiped!");
    assert!(!effective.forbidden_paths.is_empty(), "Forbidden paths were wiped!");

    // Verify key security redlines are intact
    assert!(
        effective.command_policy_forbidden.iter().any(|r| r.contains("rm")),
        "rm forbidden rules must be retained"
    );
}
