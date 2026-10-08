pub mod assignment;
pub mod cost;
pub mod expected_cost;
pub mod learning;
pub mod merge;
pub mod pack;
pub mod probability;

pub use assignment::{
    allocate_plan_models, allocate_plan_models_with_melted, assign_direct_task,
    calculate_plan_assignment_savings, calculate_task_counterfactual_cost,
    determine_reference_model, reassign_task_excluding, reassign_unstarted_tasks,
    recompute_plan_allocation_with_overrides, select_planning_model, AlternativeCandidate,
    PlanAllocationResult, TaskAssignment,
};
pub use cost::{
    calculate_c_exec, calculate_c_turn, calculate_eff_price, calculate_scarcity,
    TokenPriceKind, TurnCostParams,
};
pub use expected_cost::{
    calculate_expected_cost, select_best_candidate, EvaluatedCandidate,
};
pub use learning::{
    apply_user_preference_signal, resolve_p_min, update_cost_calibration, update_metric_ema,
    update_usage_profile_entry, BetaAccumulator, LearnedIndex, TaskOutcomeResult,
};
pub use merge::{
    merge_layers, EffectiveCandidate, EffectiveKnowledgePack, LayerOrigin, LayeredPacks,
    MergeError, ModelOverride,
};
pub use pack::{
    builtin_knowledge_pack, parse_knowledge_pack, validate_knowledge_pack, CacheRatesDef,
    KnowledgePack, ModelDef, PackError, PricePerMtok, ProviderDef, ProviderModelOffering,
    SuccessModel, UsageProfileEntry, BUILTIN_KNOWLEDGE_JSON, SUPPORTED_SCHEMA_VERSION,
};
pub use probability::{
    calculate_posterior_p, calculate_prior_p0, get_p_min_threshold, sigmoid,
};
