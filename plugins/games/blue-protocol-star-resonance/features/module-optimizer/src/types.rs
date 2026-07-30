use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModulePartInput {
    pub part_id: i32,
    #[serde(default, alias = "value")]
    pub initial_link_points: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModuleCandidate {
    pub instance_id: String,
    pub config_id: i32,
    #[serde(default)]
    pub quality: Option<i32>,
    #[serde(default)]
    pub parts: Vec<ModulePartInput>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    #[default]
    Auto,
    Exact,
    Beam,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OptimizeRequest {
    pub modules: Vec<ModuleCandidate>,
    #[serde(default)]
    pub target_attributes: Vec<i32>,
    #[serde(default)]
    pub exclude_attributes: Vec<i32>,
    #[serde(default)]
    pub min_attr_requirements: BTreeMap<i32, i32>,
    #[serde(default = "default_combination_size")]
    pub combination_size: usize,
    #[serde(default = "default_max_solutions")]
    pub max_solutions: usize,
    #[serde(default)]
    pub search_mode: SearchMode,
    #[serde(default = "default_exact_combination_limit")]
    pub exact_combination_limit: u64,
    #[serde(default = "default_beam_width")]
    pub beam_width: usize,
    #[serde(default = "default_minimum_parts")]
    pub minimum_parts: usize,
    #[serde(default)]
    pub minimum_module_total: Option<i32>,
    #[serde(default = "default_require_target_match")]
    pub require_target_match: bool,
}

impl Default for OptimizeRequest {
    fn default() -> Self {
        Self {
            modules: Vec::new(),
            target_attributes: Vec::new(),
            exclude_attributes: Vec::new(),
            min_attr_requirements: BTreeMap::new(),
            combination_size: default_combination_size(),
            max_solutions: default_max_solutions(),
            search_mode: SearchMode::Auto,
            exact_combination_limit: default_exact_combination_limit(),
            beam_width: default_beam_width(),
            minimum_parts: default_minimum_parts(),
            minimum_module_total: None,
            require_target_match: default_require_target_match(),
        }
    }
}

const fn default_combination_size() -> usize {
    4
}

const fn default_max_solutions() -> usize {
    10
}

const fn default_exact_combination_limit() -> u64 {
    500_000
}

const fn default_beam_width() -> usize {
    512
}

const fn default_minimum_parts() -> usize {
    2
}

const fn default_require_target_match() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OptimizeResponse {
    pub scoring_revision: String,
    pub catalog_revision: String,
    pub solutions: Vec<ModuleSolution>,
    pub search: SearchSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModuleSolution {
    pub instance_ids: Vec<String>,
    pub modules: Vec<ModuleCandidate>,
    pub score: i32,
    pub breakdown: ScoreBreakdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScoreBreakdown {
    pub threshold_power: i32,
    pub total_link_points: i32,
    pub total_link_power: i32,
    pub attributes: Vec<AttributeScore>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AttributeScore {
    pub attribute_id: i32,
    pub total: i32,
    pub reached_threshold: Option<i32>,
    pub base_power: i32,
    pub multiplier: i32,
    pub applied_power: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchSummary {
    pub requested_mode: SearchMode,
    pub used_mode: SearchMode,
    pub exact: bool,
    pub input_module_count: usize,
    pub candidate_module_count: usize,
    pub excluded_module_count: usize,
    pub total_combinations: u64,
    pub evaluated_states: u64,
    pub combination_size: usize,
    pub beam_width: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OptimizerCatalog {
    pub game_id: String,
    pub catalog_revision: String,
    pub scoring_revision: String,
    pub client_builds: Vec<String>,
    pub attributes: Vec<AttributeCatalogEntry>,
    pub combination_sizes: Vec<usize>,
    pub default_max_solutions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AttributeCatalogEntry {
    pub id: i32,
    pub name: String,
    pub icon: Option<String>,
    pub thresholds: Vec<i32>,
    pub fight_values: Vec<i32>,
}

#[derive(Debug, Error)]
pub enum OptimizerError {
    #[error("could not read optimizer catalog file {path}: {source}")]
    ReadCatalog {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not decode optimizer catalog file {path}: {source}")]
    DecodeCatalog {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("invalid optimizer catalog: {0}")]
    InvalidCatalog(String),
    #[error("invalid optimizer request: {0}")]
    InvalidRequest(String),
}
