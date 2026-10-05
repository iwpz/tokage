//! Tokage model matching engine (docs/04).

pub mod pack;

pub use pack::{
    builtin_knowledge_pack, parse_knowledge_pack, validate_knowledge_pack, KnowledgePack,
    ModelDef, PackError, ProviderDef, ProviderModelOffering, SuccessModel, UsageProfileEntry,
    BUILTIN_KNOWLEDGE_JSON, SUPPORTED_SCHEMA_VERSION,
};
