pub mod cost;
pub mod expected_cost;
pub mod merge;
pub mod pack;
pub mod probability;

pub use cost::{
    calculate_c_exec, calculate_c_turn, calculate_eff_price, calculate_scarcity,
    TokenPriceKind, TurnCostParams,
};
pub use expected_cost::{
    calculate_expected_cost, select_best_candidate, EvaluatedCandidate,
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
