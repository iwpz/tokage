#[test]
fn real_opencode_invoice_plan_preserves_parallel_graph_and_contracts() {
    let raw = include_str!("fixtures/m2b-opencode-plan.md");
    let plan = tokage_planning::parse_plan(raw, &Default::default()).expect("real planner reply parses");
    assert_eq!(plan.tasks.len(), 3);
    assert!(!plan.spec.goal.is_empty());
    assert_eq!(plan.contracts.len(), 4);
    assert_eq!(plan.tasks[2].depends_on, ["P0.M1a", "P0.M1b"]);
    assert!(plan.tasks[2].goal.contains("\"Total: \""));
    assert_eq!(plan.tasks[0].acceptance.commands, ["npm run test:pricing"]);
    assert_eq!(plan.tasks[1].acceptance.commands, ["npm run test:format"]);
    assert_eq!(plan.tasks[2].write_scope, ["invoice.mjs"]);
    assert!(plan.tasks[2].acceptance.commands.contains(&"npm test".to_string()));
    let with_model = raw.replace("  domain: algorithm", "  assigned_model: CLAUDE_CODE:haiku\n  model_params: {effort: low, fast: true}\n  domain: algorithm");
    let routed = tokage_planning::parse_plan(&with_model, &Default::default()).unwrap();
    assert_eq!(routed.tasks[0].assigned_model.as_deref(), Some("CLAUDE_CODE:haiku"));
    let params = routed.tasks[0].model_params.as_ref().expect("syntax repair preserves model parameters");
    assert_eq!(params.effort.as_deref(), Some("low"));
    assert!(params.fast);
}

#[test]
fn prose_repair_preserves_other_block_scalar_content() {
    let raw = include_str!("fixtures/m2b-opencode-plan.md");
    let original = "  goal: Implement subtotal(items) per C1 summing priceCents*quantity with empty->0 and RangeError on negative/non-integer values.";
    let raw = raw.replace(original, "  goal: |\n    goal: literal: constraint\n    keep as written");
    // The integration goal still needs colon repair; the first goal is a literal
    // block and its contents must not be mistaken for YAML properties.
    let plan = tokage_planning::parse_plan(&raw, &Default::default()).unwrap();
    assert_eq!(plan.tasks[0].goal, "goal: literal: constraint\nkeep as written\n");
    assert_eq!(plan.tasks[0].write_scope, ["pricing.mjs"]);
    assert_eq!(plan.tasks[2].acceptance.commands, ["npm run test:integration", "npm test"]);
}
