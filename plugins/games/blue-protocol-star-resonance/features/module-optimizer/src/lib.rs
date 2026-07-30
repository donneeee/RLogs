//! Portable BPSR module optimization for native services and future WASM use.
//!
//! The behavior is derived from the AGPL-3.0 module optimizer in
//! `fudiyangjin/resonance-logs-cn` 0.2.0 at
//! `ccdeef23c7806be5072f95a9e80b103794af3544`.

mod catalog;
mod scoring;
mod search;
mod types;

pub use catalog::{load_catalog_from_install_root, load_catalog_from_path};
pub use scoring::ScoringRules;
pub use search::{optimize, score_modules};
pub use types::{
    AttributeCatalogEntry, AttributeScore, ModuleCandidate, ModulePartInput, ModuleSolution,
    OptimizeRequest, OptimizeResponse, OptimizerCatalog, OptimizerError, ScoreBreakdown,
    SearchMode, SearchSummary,
};

/// The upstream behavior pinned for compatibility and attribution.
pub const CN_OPTIMIZER_REVISION: &str =
    "fudiyangjin/resonance-logs-cn@ccdeef23c7806be5072f95a9e80b103794af3544";
