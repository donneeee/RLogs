use std::{
    collections::{BTreeMap, BTreeSet, HashMap, hash_map::DefaultHasher},
    env,
    fs::{self, File},
    hash::{Hash, Hasher},
    io::{BufRead, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

use serde::{
    Deserialize, Deserializer, Serialize,
    de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor},
};
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 2;
const REQUIRED_FORMULA_COHORT_SCHEMA_VERSIONS: [u16; 3] = [39, 40, 41];
const CURRENT_HP: i32 = 11_310;
const CRITICAL_DAMAGE: i32 = 12_510;
const SOURCE_CANDIDATE_STAGE_ATTRIBUTES: [(i32, &str); 15] = [
    (11_840, "external-damage-derived"),
    (11_880, "suppression-damage"),
    (11_940, "mastery"),
    (11_950, "versatility-source-alias"),
    (12_510, "critical-damage"),
    (12_550, "physical-amplification"),
    (12_590, "near-distance-damage"),
    (12_610, "far-distance-damage"),
    (12_630, "boss-damage"),
    (12_670, "general-damage"),
    (12_690, "season-damage"),
    (12_710, "dedicated-multiplicative"),
    (12_730, "pet-damage"),
    (13_100, "all-element-damage"),
    (13_170, "property-7-element-damage"),
];
const TARGET_CANDIDATE_STAGE_ATTRIBUTES: [(i32, &str); 10] = [
    (11_850, "external-damage-reduction"),
    (11_890, "suppression-damage-reduction"),
    (12_520, "critical-damage-resistance"),
    (12_560, "physical-damage-reduction"),
    (12_640, "boss-damage-resistance"),
    (12_680, "general-damage-reduction"),
    (12_700, "season-damage-reduction"),
    (12_760, "level-damage-reduction"),
    (13_200, "all-element-resistance"),
    (13_270, "property-7-element-resistance"),
];
const MIN_PARTITIONS: usize = 16;
const MAX_PARTITIONS: usize = 4_096;
const RAW_PARTITION_MEMORY_DIVISOR: u64 = 128;

#[derive(Debug)]
struct Arguments {
    cohort: PathBuf,
    output: PathBuf,
    target_identity_worklist_output: Option<PathBuf>,
    target_identity_proof: Option<PathBuf>,
    target_status_relaxed_diagnostic_output: Option<PathBuf>,
    cross_capture_target_config_diagnostic_output: Option<PathBuf>,
    selected_ability_diagnostic_output: Option<PathBuf>,
    selected_ability_ids: BTreeSet<i64>,
    selected_hit_event_id: Option<i32>,
    selected_coefficient_basis_points: Option<i64>,
    diagnostic_effect_id: Option<i64>,
    example_limit: usize,
    memory_limit_mib: usize,
}

#[derive(Debug)]
struct PartitionedCohort {
    game_build: Option<String>,
    source_inputs: Vec<String>,
    input_bytes: u64,
    input_sha256: String,
    attribute_states: Vec<Vec<Attribute>>,
    status_states: Vec<Vec<Status>>,
    sample_count: usize,
    partition_paths: Vec<PathBuf>,
    diagnostic_partition_paths: Vec<PathBuf>,
    cross_capture_partition_paths: Vec<PathBuf>,
    work_dir: PathBuf,
    largest_partition_bytes: u64,
    target_identity_worklist: Vec<TargetIdentityRequest>,
    target_identity_enrichment: Option<TargetIdentityEnrichmentReport>,
}

impl Drop for PartitionedCohort {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.work_dir);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
struct Attribute {
    attribute_id: i32,
    value: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
struct Status {
    effect_id: i64,
    source_entity_uuid: i64,
    stacks: i64,
    level: i64,
    origin_source_type_id: Option<i64>,
    origin_source_config_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
struct ActorIdentity {
    entity_type_id: i32,
    monster_id: Option<i64>,
    character_id: Option<String>,
    class_id: Option<i32>,
    specialization_id: Option<i32>,
    level: Option<u32>,
}

type TargetIdentityKey = (String, u32, u64, i64);

#[derive(Debug, Clone, Serialize)]
struct TargetIdentityRequest {
    session_id: String,
    run_ordinal: u32,
    sequence: u64,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    ability_id: i64,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    packet_property: Option<i32>,
    amount: i64,
    normal_value: Option<i64>,
    source_attribute_state_id: usize,
    target_attribute_state_id: usize,
    source_status_state_id: usize,
    target_status_state_id: usize,
    observed_mitigation_attribute_ids: BTreeSet<i32>,
    observed_mitigation_attributes: Vec<Attribute>,
}

#[derive(Debug)]
struct TargetIdentityEnrichment {
    source: PathBuf,
    bytes: u64,
    sha256: String,
    game_build: String,
    declared_observations: usize,
    target_identities: BTreeMap<TargetIdentityKey, ActorIdentity>,
    source_identities: BTreeMap<TargetIdentityKey, ActorIdentity>,
    scene_ids: BTreeMap<TargetIdentityKey, i32>,
}

#[derive(Debug, Clone, Serialize)]
struct TargetIdentityEnrichmentReport {
    path: String,
    bytes: u64,
    sha256: String,
    game_build: String,
    declared_action_observations: usize,
    declared_event_time_target_actor_observations: usize,
    unresolved_action_observations: usize,
    declared_event_time_source_actor_observations: usize,
    declared_event_time_scene_observations: usize,
    exact_formula_cohort_sample_joins: usize,
    exact_formula_cohort_source_actor_joins: usize,
    exact_formula_cohort_scene_joins: usize,
    declared_identities_without_formula_cohort_sample: usize,
    formula_cohort_identity_conflicts: usize,
    policy: &'static str,
    formula_authority: bool,
    runtime_authority: bool,
    provider_rdps_credit_allowed: bool,
}

#[derive(Debug, Deserialize, Serialize)]
struct Sample {
    session_id: String,
    run_ordinal: u32,
    sequence: u64,
    #[serde(default)]
    scene_id: Option<i32>,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    #[serde(default)]
    source_actor_identity: Option<ActorIdentity>,
    #[serde(default)]
    direct_source_actor_identity: Option<ActorIdentity>,
    #[serde(default)]
    target_actor_identity: Option<ActorIdentity>,
    ability_id: i64,
    passive_uuid: Option<i64>,
    hit_event_id: Option<i32>,
    amount: i64,
    normal_value: Option<i64>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    critical: Option<bool>,
    lucky: Option<bool>,
    packet: Packet,
    source_attribute_state_id: usize,
    target_attribute_state_id: usize,
    source_status_state_id: usize,
    target_status_state_id: usize,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct Packet {
    #[serde(default)]
    attacker_uuid: Option<i64>,
    #[serde(default)]
    top_summoner_uuid: Option<i64>,
    #[serde(default)]
    owner_id: Option<i32>,
    type_flags: Option<i32>,
    owner_level: Option<i32>,
    owner_stage: Option<i32>,
    normal_hit: Option<bool>,
    property: Option<i32>,
    passive_uuid: Option<u32>,
    rainbow: Option<bool>,
    damage_mode: Option<i32>,
    skill_effect_uuid: Option<i64>,
    skill_effect_group_index: Option<u32>,
    skill_effect_component_index: Option<u32>,
    skill_effect_component_count: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct CalculationContext {
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    critical: Option<bool>,
    lucky: Option<bool>,
    type_flags: Option<i32>,
    normal_hit: Option<bool>,
    property: Option<i32>,
    has_sample_passive_uuid: bool,
    has_packet_passive_uuid: bool,
    rainbow: Option<bool>,
    damage_mode: Option<i32>,
    has_skill_effect_uuid: bool,
    skill_effect_group_index: Option<u32>,
    skill_effect_component_index: Option<u32>,
    skill_effect_component_count: Option<u32>,
    direct_source_differs_from_source: bool,
}

impl From<&Sample> for CalculationContext {
    fn from(sample: &Sample) -> Self {
        Self {
            damage_source: sample.damage_source,
            damage_type: sample.damage_type,
            critical: sample.critical,
            lucky: sample.lucky,
            type_flags: sample.packet.type_flags,
            normal_hit: sample.packet.normal_hit,
            property: sample.packet.property,
            has_sample_passive_uuid: sample.passive_uuid.is_some(),
            has_packet_passive_uuid: sample.packet.passive_uuid.is_some(),
            rainbow: sample.packet.rainbow,
            damage_mode: sample.packet.damage_mode,
            has_skill_effect_uuid: sample.packet.skill_effect_uuid.is_some(),
            skill_effect_group_index: sample.packet.skill_effect_group_index,
            skill_effect_component_index: sample.packet.skill_effect_component_index,
            skill_effect_component_count: sample.packet.skill_effect_component_count,
            direct_source_differs_from_source: sample
                .direct_source_entity_uuid
                .is_some_and(|direct| direct != sample.source_entity_uuid),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct OwnerStageContext {
    owner_level: Option<i32>,
    owner_stage: Option<i32>,
}

impl From<&Sample> for OwnerStageContext {
    fn from(sample: &Sample) -> Self {
        Self {
            owner_level: sample.packet.owner_level,
            owner_stage: sample.packet.owner_stage,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct CompleteRetainedStateContext {
    calculation_context: CalculationContext,
    source_attribute_state_id: usize,
    target_attribute_state_id: usize,
    source_status_state_id: usize,
    target_status_state_id: usize,
}

#[derive(Debug, Default, Serialize)]
struct FactorCompatibilityCounters {
    samples_with_positive_base_and_output: u64,
    samples_with_integer_factor_interval: u64,
    samples_without_integer_factor_interval: u64,
    samples_with_unique_integer_factor: u64,
}

#[derive(Debug, Default, Serialize)]
struct CriticalStageCandidateCounters {
    samples_with_positive_staged_base_and_output: u64,
    samples_with_integer_other_factor_interval: u64,
    samples_without_integer_other_factor_interval: u64,
    samples_with_unique_integer_other_factor: u64,
}

#[derive(Debug, Default)]
struct AttributeStageCoverage {
    samples_present: u64,
    samples_nonzero: u64,
    samples_present_with_integer_factor_interval: u64,
    samples_present_without_integer_factor_interval: u64,
    values: BTreeMap<i64, FactorCompatibilityCounters>,
}

#[derive(Debug, Serialize)]
struct SourceStageOrderObservation {
    session_id: String,
    run_ordinal: u32,
    sequence: u64,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    source_attribute_state_id: usize,
    target_attribute_state_id: usize,
    source_status_state_id: usize,
    target_status_state_id: usize,
    calculation_context: CalculationContext,
    owner_stage_context: OwnerStageContext,
    base: i128,
    output: i128,
    raw_values_by_attribute_id: BTreeMap<i32, i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Identity {
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    direct_source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    ability_id: i64,
    passive_uuid: Option<i64>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    critical: Option<bool>,
    lucky: Option<bool>,
    type_flags: Option<i32>,
    owner_level: Option<i32>,
    owner_stage: Option<i32>,
    normal_hit: Option<bool>,
    property: Option<i32>,
    packet_passive_uuid: Option<u32>,
    rainbow: Option<bool>,
    damage_mode: Option<i32>,
    skill_effect_uuid: Option<i64>,
    skill_effect_group_index: Option<u32>,
    skill_effect_component_index: Option<u32>,
    skill_effect_component_count: Option<u32>,
    raw_attacker_uuid: Option<i64>,
    raw_top_summoner_uuid: Option<i64>,
    raw_owner_id: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CrossCaptureCalculationIdentity {
    scene_id: i32,
    source_actor_identity: ActorIdentity,
    direct_source_actor_identity: Option<ActorIdentity>,
    target_actor_identity: ActorIdentity,
    ability_id: i64,
    passive_uuid: Option<i64>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    critical: Option<bool>,
    lucky: Option<bool>,
    type_flags: Option<i32>,
    owner_level: Option<i32>,
    owner_stage: Option<i32>,
    normal_hit: Option<bool>,
    property: Option<i32>,
    packet_passive_uuid: Option<u32>,
    rainbow: Option<bool>,
    damage_mode: Option<i32>,
    skill_effect_uuid: Option<i64>,
    skill_effect_group_index: Option<u32>,
    skill_effect_component_index: Option<u32>,
    skill_effect_component_count: Option<u32>,
}

#[derive(Debug, Clone)]
struct Axis {
    name: &'static str,
    current_id: i32,
    family: Vec<i32>,
    required_property: Option<i32>,
    candidates: Vec<Model>,
}

#[derive(Debug, Clone)]
struct Model {
    name: &'static str,
    constant: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
struct Outcome {
    amount: i64,
    normal_value: Option<i64>,
}

#[derive(Debug, Default)]
struct Bucket {
    sequences: BTreeSet<u64>,
    outcomes: BTreeMap<Outcome, u64>,
}

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
struct Counters {
    samples_with_axis: u64,
    controlled_groups: u64,
    distinct_axis_pairs: u64,
    deterministic_pairs: u64,
    equal_output_pairs: u64,
    divergent_output_pairs: u64,
    nondeterministic_pairs: u64,
}

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
struct ModelCounters {
    exact_pairs: u64,
    rejected_pairs: u64,
}

#[derive(Debug, Serialize)]
struct Example {
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    ability_id: i64,
    property: Option<i32>,
    left_raw: i64,
    right_raw: i64,
    left_outcome: Outcome,
    right_outcome: Outcome,
    left_sequences: Vec<u64>,
    right_sequences: Vec<u64>,
    compatible_base_minimum: Option<String>,
    compatible_base_maximum: Option<String>,
}

#[derive(Debug, Serialize)]
struct ModelResult {
    constant: i64,
    counters: ModelCounters,
    exact_examples: Vec<Example>,
    rejected_examples: Vec<Example>,
}

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
struct TargetStatusRelaxedCounters {
    samples_with_axis: u64,
    groups_with_multiple_target_status_or_axis_variants: u64,
    same_axis_status_pairs: u64,
    same_axis_deterministic_pairs: u64,
    same_axis_equal_output_pairs: u64,
    same_axis_divergent_output_pairs: u64,
    same_axis_nondeterministic_pairs: u64,
    same_axis_pairs_with_selected_effect_in_status_delta: u64,
    same_axis_pairs_with_only_selected_effect_in_status_delta: u64,
    distinct_axis_pairs: u64,
    deterministic_pairs: u64,
    equal_output_pairs: u64,
    divergent_output_pairs: u64,
    nondeterministic_pairs: u64,
    pairs_with_selected_effect_in_status_delta: u64,
    pairs_with_only_selected_effect_in_status_delta: u64,
}

#[derive(Debug, Serialize)]
struct TargetStatusRelaxedExample {
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    ability_id: i64,
    property: Option<i32>,
    left_raw: i64,
    right_raw: i64,
    left_outcome: Outcome,
    right_outcome: Outcome,
    left_target_status_state_id: usize,
    right_target_status_state_id: usize,
    left_only_statuses: Vec<Status>,
    right_only_statuses: Vec<Status>,
    selected_effect_is_the_only_status_delta: bool,
    left_sequences: Vec<u64>,
    right_sequences: Vec<u64>,
}

#[derive(Debug, Serialize)]
struct TargetStatusRelaxedAxisResult {
    current_attribute_id: i32,
    family_attribute_ids: Vec<i32>,
    required_packet_property: Option<i32>,
    counters: TargetStatusRelaxedCounters,
    same_axis_status_examples: Vec<TargetStatusRelaxedExample>,
    selected_effect_same_axis_examples: Vec<TargetStatusRelaxedExample>,
    near_pair_examples: Vec<TargetStatusRelaxedExample>,
    selected_effect_examples: Vec<TargetStatusRelaxedExample>,
}

#[derive(Debug, Serialize)]
struct AxisResult {
    current_attribute_id: i32,
    family_attribute_ids: Vec<i32>,
    required_packet_property: Option<i32>,
    counters: Counters,
    models: BTreeMap<String, ModelResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
struct CrossCaptureObservation {
    session_id: String,
    run_ordinal: u32,
    sequence: u64,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
}

#[derive(Debug, Default)]
struct CrossCaptureBucket {
    observations: BTreeSet<CrossCaptureObservation>,
    outcomes: BTreeMap<Outcome, u64>,
}

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
struct CrossCaptureCounters {
    samples_with_axis: u64,
    samples_with_packet_observed_target_actor_identity: u64,
    samples_with_stable_target_actor_id: u64,
    samples_with_cross_capture_actor_shape_context: u64,
    groups_with_multiple_axis_states: u64,
    distinct_axis_pairs: u64,
    pairs_with_cross_capture_witness: u64,
    deterministic_cross_capture_pairs: u64,
    equal_output_cross_capture_pairs: u64,
    divergent_output_cross_capture_pairs: u64,
    nondeterministic_cross_capture_pairs: u64,
}

#[derive(Debug, Serialize)]
struct CrossCaptureExample {
    target_actor_identity: ActorIdentity,
    ability_id: i64,
    property: Option<i32>,
    left_raw: i64,
    right_raw: i64,
    left_outcome: Outcome,
    right_outcome: Outcome,
    left_observation: CrossCaptureObservation,
    right_observation: CrossCaptureObservation,
    compatible_base_minimum: Option<String>,
    compatible_base_maximum: Option<String>,
}

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
struct CrossCaptureModelCounters {
    exact_pairs: u64,
    rejected_pairs: u64,
}

#[derive(Debug, Serialize)]
struct CrossCaptureModelResult {
    constant: i64,
    counters: CrossCaptureModelCounters,
    exact_examples: Vec<CrossCaptureExample>,
    rejected_examples: Vec<CrossCaptureExample>,
}

#[derive(Debug, Serialize)]
struct CrossCaptureAxisResult {
    current_attribute_id: i32,
    family_attribute_ids: Vec<i32>,
    required_packet_property: Option<i32>,
    counters: CrossCaptureCounters,
    models: BTreeMap<String, CrossCaptureModelResult>,
}

fn load_target_identity_enrichment(
    path: &Path,
) -> Result<TargetIdentityEnrichment, Box<dyn std::error::Error>> {
    let bytes = fs::metadata(path)?.len();
    let sha256 = sha256_file(path)?;
    let proof: serde_json::Value = serde_json::from_reader(BufReader::new(File::open(path)?))?;
    let game_build = proof
        .get("game_build")
        .and_then(serde_json::Value::as_str)
        .ok_or("target identity proof omits game_build")?
        .to_owned();
    let summary = proof
        .get("summary")
        .ok_or("target identity proof omits summary")?;
    let observations = proof
        .get("observations")
        .and_then(serde_json::Value::as_array)
        .ok_or("target identity proof omits observations")?;
    let policy = proof
        .get("policy")
        .ok_or("target identity proof omits policy")?;
    let schema_version = proof
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or("target identity proof omits schema_version")?;
    let is_selected_schema_1 = schema_version == 1;
    let is_neutral_schema_2 = schema_version == 2;
    let requested_actions = summary
        .get("requested_actions")
        .and_then(serde_json::Value::as_u64)
        .ok_or("target identity proof omits requested_actions")?;
    if (!is_selected_schema_1 && !is_neutral_schema_2)
        || proof
            .get("generated_by")
            .and_then(serde_json::Value::as_str)
            != Some("rlogs-bpsr-selected-action-target-identity-proof")
        || (is_selected_schema_1 && requested_actions != 2_406)
        || requested_actions != observations.len() as u64
        || summary
            .get("matched_actions")
            .and_then(serde_json::Value::as_u64)
            != Some(requested_actions)
        || summary
            .get("missing_actions")
            .and_then(serde_json::Value::as_u64)
            != Some(0)
        || summary
            .get("observations_with_identity_conflict")
            .and_then(serde_json::Value::as_u64)
            != Some(0)
        || policy
            .get("exact_session_sequence_and_target_join_only")
            .and_then(serde_json::Value::as_bool)
            != Some(true)
        || policy
            .get("actor_identity_is_event_time_state")
            .and_then(serde_json::Value::as_bool)
            != Some(true)
        || policy
            .get("target_allegiance_assumed")
            .and_then(serde_json::Value::as_bool)
            != Some(false)
        || (is_neutral_schema_2
            && (policy
                .get("target_endpoint_is_allegiance_neutral")
                .and_then(serde_json::Value::as_bool)
                != Some(true)
                || policy
                    .get("recipient_or_enemy_target_are_both_allowed")
                    .and_then(serde_json::Value::as_bool)
                    != Some(true)))
        || policy
            .get("static_target_stats_substituted")
            .and_then(serde_json::Value::as_bool)
            != Some(false)
        || policy
            .get("runtime_authority")
            .and_then(serde_json::Value::as_bool)
            != Some(false)
        || policy
            .get("provider_rdps_credit_allowed")
            .and_then(serde_json::Value::as_bool)
            != Some(false)
    {
        return Err("target identity proof is unsafe, incomplete, or not the exact selected-action schema 1 receipt".into());
    }
    let mut target_identities = BTreeMap::new();
    let mut source_identities = BTreeMap::new();
    let mut scene_ids = BTreeMap::new();
    for row in observations {
        let session_id = row
            .get("session_id")
            .and_then(serde_json::Value::as_str)
            .ok_or("target identity observation omits session_id")?
            .to_owned();
        let run_ordinal = u32::try_from(
            row.get("run_ordinal")
                .and_then(serde_json::Value::as_u64)
                .ok_or("target identity observation omits run_ordinal")?,
        )?;
        let sequence = row
            .get("sequence")
            .and_then(serde_json::Value::as_u64)
            .ok_or("target identity observation omits sequence")?;
        let target_entity_uuid = row
            .get("target_entity_uuid")
            .and_then(serde_json::Value::as_i64)
            .ok_or("target identity observation omits target_entity_uuid")?;
        let actor_active = row
            .get("actor_active")
            .and_then(serde_json::Value::as_bool)
            .ok_or("target identity observation omits actor_active")?;
        let identity_conflict = row
            .get("identity_conflict")
            .and_then(serde_json::Value::as_bool)
            .ok_or("target identity observation omits identity_conflict")?;
        if identity_conflict {
            return Err(
                "target identity observation has conflicting event-time actor identity".into(),
            );
        }
        if !actor_active {
            if is_selected_schema_1 {
                return Err("selected-action target identity observation is inactive".into());
            }
            continue;
        }
        let entity_type_id = i32::try_from(
            row.get("entity_type_id")
                .and_then(serde_json::Value::as_i64)
                .ok_or("active target identity observation omits entity_type_id")?,
        )?;
        let monster_id = row
            .get("numeric_monster_id")
            .and_then(serde_json::Value::as_i64);
        let character_id = row
            .get("character_id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        let class_id = row
            .get("class_id")
            .and_then(serde_json::Value::as_i64)
            .map(i32::try_from)
            .transpose()?;
        let specialization_id = row
            .get("specialization_id")
            .and_then(serde_json::Value::as_i64)
            .map(i32::try_from)
            .transpose()?;
        let level = row
            .get("level")
            .and_then(serde_json::Value::as_u64)
            .map(u32::try_from)
            .transpose()?;
        if is_selected_schema_1 && monster_id.is_none() {
            return Err(
                "selected-action target identity observation lacks an exact numeric build identity"
                    .into(),
            );
        }
        let key = (session_id, run_ordinal, sequence, target_entity_uuid);
        if target_identities
            .insert(
                key.clone(),
                ActorIdentity {
                    entity_type_id,
                    monster_id,
                    character_id,
                    class_id,
                    specialization_id,
                    level,
                },
            )
            .is_some()
        {
            return Err("target identity proof contains a duplicate exact action key".into());
        }
        if is_neutral_schema_2 {
            if row
                .get("source_unresolved_reasons")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|reasons| {
                    reasons
                        .iter()
                        .any(|reason| reason.as_str() == Some("source-entity-mismatch"))
                })
            {
                return Err(
                    "action identity observation source does not match the requested damage actor"
                        .into(),
                );
            }
            if let Some(scene_id) = row.get("scene_id").and_then(serde_json::Value::as_i64) {
                scene_ids.insert(key.clone(), i32::try_from(scene_id)?);
            }
            let source_active = row
                .get("source_actor_active")
                .and_then(serde_json::Value::as_bool)
                .ok_or("schema 2 action identity observation omits source_actor_active")?;
            let source_conflict = row
                .get("source_identity_conflict")
                .and_then(serde_json::Value::as_bool)
                .ok_or("schema 2 action identity observation omits source_identity_conflict")?;
            if source_conflict {
                return Err(
                    "action identity observation has conflicting event-time source actor identity"
                        .into(),
                );
            }
            if source_active {
                let source_identity = ActorIdentity {
                    entity_type_id: i32::try_from(
                        row.get("source_entity_type_id")
                            .and_then(serde_json::Value::as_i64)
                            .ok_or("active source actor observation omits entity_type_id")?,
                    )?,
                    monster_id: row
                        .get("source_numeric_monster_id")
                        .and_then(serde_json::Value::as_i64),
                    character_id: row
                        .get("source_character_id")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned),
                    class_id: row
                        .get("source_class_id")
                        .and_then(serde_json::Value::as_i64)
                        .map(i32::try_from)
                        .transpose()?,
                    specialization_id: row
                        .get("source_specialization_id")
                        .and_then(serde_json::Value::as_i64)
                        .map(i32::try_from)
                        .transpose()?,
                    level: row
                        .get("source_level")
                        .and_then(serde_json::Value::as_u64)
                        .map(u32::try_from)
                        .transpose()?,
                };
                source_identities.insert(key, source_identity);
            }
        }
    }
    Ok(TargetIdentityEnrichment {
        source: path.to_path_buf(),
        bytes,
        sha256,
        game_build,
        declared_observations: observations.len(),
        target_identities,
        source_identities,
        scene_ids,
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args()?;
    let target_identity_enrichment = args
        .target_identity_proof
        .as_deref()
        .map(load_target_identity_enrichment)
        .transpose()?;
    let cohort = load_partitioned_cohort(&args, target_identity_enrichment)?;
    validate_state_references(&cohort)?;
    if let Some(path) = args.target_identity_worklist_output.as_deref() {
        write_target_identity_worklist(path, &args, &cohort)?;
    }
    let axes = axes();
    let results = audit_partitioned_cohort(&cohort, &axes, args.example_limit)?;
    let target_status_relaxed_diagnostic = match (
        args.target_status_relaxed_diagnostic_output.as_ref(),
        args.diagnostic_effect_id,
    ) {
        (Some(_), Some(effect_id)) => Some(audit_target_status_relaxed_partitions(
            &cohort,
            &axes,
            effect_id,
            args.example_limit,
        )?),
        (None, None) => None,
        _ => return Err("diagnostic output and effect ID must be supplied together".into()),
    };
    let cross_capture_target_config_diagnostic = args
        .cross_capture_target_config_diagnostic_output
        .as_ref()
        .map(|_| audit_cross_capture_target_config_partitions(&cohort, &axes, args.example_limit))
        .transpose()?;
    let selected_ability_diagnostic = args
        .selected_ability_diagnostic_output
        .as_ref()
        .map(|_| {
            audit_selected_ability_partitions(
                &cohort,
                &axes,
                &args.selected_ability_ids,
                args.selected_hit_event_id
                    .expect("selected diagnostic arguments are validated together"),
                args.selected_coefficient_basis_points
                    .expect("selected diagnostic arguments are validated together"),
                args.example_limit,
            )
        })
        .transpose()?;
    let measured_peak_working_set_bytes = peak_working_set_bytes();
    let configured_memory_limit_bytes = u64::try_from(args.memory_limit_mib)
        .unwrap_or(u64::MAX)
        .saturating_mul(1024 * 1024);
    if measured_peak_working_set_bytes.is_some_and(|peak| peak > configured_memory_limit_bytes) {
        return Err(format!(
            "measured peak working set {} bytes exceeded configured --memory-limit-mib {}",
            measured_peak_working_set_bytes.unwrap_or(0),
            args.memory_limit_mib,
        )
        .into());
    }
    let output = serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "generated_by": "rlogs-bpsr-target-mitigation-transform-proof",
        "game_build": cohort.game_build.clone(),
        "policy": {
            "runtime_authority": false,
            "formula_authority": false,
            "unresolved_evidence_is_hidden": false,
            "calculation_control": "packet calculation identity, source and target entity, formula row, outcome flags, property, owner level/stage, and skill-effect component identity are exact",
            "state_control": "complete source attributes and statuses are exact except volatile CurrentHP; complete target statuses are exact; complete target attributes are exact except volatile CurrentHP and the audited six-member mitigation family",
            "model": "floor(nonnegative_base * constant / (constant + target_axis_raw))",
            "promotion_rule": "a model is eligible only when at least one divergent deterministic controlled pair is exact and zero deterministic controlled pairs reject it; absent pairs and equal-output-only pairs are not formula proof",
            "disk_partitions_preserve_exact_group_semantics": true,
            "cross_capture_pairing_allowed": false
        },
        "processing": {
            "memory_limit_mib": args.memory_limit_mib,
            "partition_count": cohort.partition_paths.len(),
            "largest_partition_bytes": cohort.largest_partition_bytes,
            "sample_count": cohort.sample_count,
            "measured_peak_working_set_bytes": measured_peak_working_set_bytes,
            "measured_peak_working_set_mib": measured_peak_working_set_bytes
                .map(|bytes| bytes as f64 / (1024.0 * 1024.0)),
            "measured_peak_within_configured_limit": measured_peak_working_set_bytes
                .map(|peak| peak <= configured_memory_limit_bytes),
            "partition_key": "packet calculation identity, both status-state IDs, source attributes excluding CurrentHP, and target attributes excluding CurrentHP plus every audited mitigation family; exact per-axis grouping is reapplied inside each partition"
        },
        "input": {
            "path": args.cohort.display().to_string(),
            "bytes": cohort.input_bytes,
            "sha256": cohort.input_sha256.clone(),
            "source_inputs": cohort.source_inputs.clone()
        },
        "target_identity_enrichment": cohort.target_identity_enrichment.clone(),
        "axes": results,
    });
    if let Some(parent) = args.output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(&args.output)?);
    serde_json::to_writer_pretty(&mut writer, &output)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    println!("wrote {}", args.output.display());
    if let (Some(output), Some(effect_id), Some(diagnostic)) = (
        args.target_status_relaxed_diagnostic_output.as_ref(),
        args.diagnostic_effect_id,
        target_status_relaxed_diagnostic,
    ) {
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let report = serde_json::json!({
            "schema_version": 3,
            "generated_by": "rlogs-bpsr-target-mitigation-transform-proof:target-status-relaxed-diagnostic",
            "game_build": cohort.game_build.clone(),
            "selected_effect_id": effect_id,
            "policy": {
                "exact_numeric_effect_ids_and_build_are_authoritative": true,
                "same_capture_only": true,
                "cross_capture_pairing_allowed": false,
                "only_target_status_state_is_relaxed": true,
                "complete_target_status_row_deltas_are_preserved": true,
                "same_axis_status_variants_are_audited_for_exact_outcome_invariance": true,
                "same_axis_equal_outcomes_are_local_invariance_evidence_not_global_zero_effect_proof": true,
                "near_pair_is_not_controlled_counterfactual_proof": true,
                "formula_authority": false,
                "runtime_authority": false,
                "provider_rdps_credit_allowed": false
            },
            "processing": {
                "memory_limit_mib": args.memory_limit_mib,
                "partition_count": cohort.diagnostic_partition_paths.len(),
                "sample_count": cohort.sample_count,
                "measured_peak_working_set_bytes": measured_peak_working_set_bytes,
                "measured_peak_working_set_mib": measured_peak_working_set_bytes
                    .map(|bytes| bytes as f64 / (1024.0 * 1024.0)),
                "measured_peak_within_configured_limit": measured_peak_working_set_bytes
                    .map(|peak| peak <= configured_memory_limit_bytes),
                "partition_key": "exact packet calculation identity, source status state, source attributes except CurrentHP, and target attributes except CurrentHP plus all audited mitigation families; only target status state is omitted for the diagnostic"
            },
            "input": {
                "path": args.cohort.display().to_string(),
                "bytes": cohort.input_bytes,
                "sha256": cohort.input_sha256.clone(),
                "source_inputs": cohort.source_inputs.clone()
            },
            "target_identity_enrichment": cohort.target_identity_enrichment.clone(),
            "axes": diagnostic,
            "authority": {
                "exact_target_mitigation_formula_proven": false,
                "exact_operation_order_and_integer_rounding_proven": false,
                "packet_conservation_proven": false,
                "formula_authority": false,
                "runtime_authority": false,
                "provider_rdps_credit_allowed": false
            }
        });
        let mut diagnostic_writer = BufWriter::new(File::create(output)?);
        serde_json::to_writer_pretty(&mut diagnostic_writer, &report)?;
        diagnostic_writer.write_all(b"\n")?;
        diagnostic_writer.flush()?;
        println!("wrote {}", output.display());
    }
    if let (Some(output), Some(diagnostic)) = (
        args.cross_capture_target_config_diagnostic_output.as_ref(),
        cross_capture_target_config_diagnostic,
    ) {
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let report = serde_json::json!({
            "schema_version": 3,
            "generated_by": "rlogs-bpsr-target-mitigation-transform-proof:cross-capture-target-config-diagnostic",
            "game_build": cohort.game_build.clone(),
            "policy": {
                "exact_numeric_build_is_authoritative": true,
                "local_or_offline_evidence_only": true,
                "remote_player_only_packets_are_required": false,
                "packet_observed_target_actor_shape_is_required": true,
                "exact_source_actor_stable_character_or_monster_id_is_required": true,
                "exact_run_scene_id_is_required": true,
                "stable_target_character_or_monster_id_is_not_required_for_diagnostic_grouping": true,
                "missing_stable_target_actor_id_blocks_formula_promotion": true,
                "complete_retained_source_and_target_status_state_ids_are_exact": true,
                "complete_retained_source_attributes_except_current_hp_are_exact": true,
                "complete_retained_target_attributes_except_current_hp_and_the_audited_axis_are_exact": true,
                "ephemeral_session_run_and_entity_ids_are_relaxed": true,
                "actor_identity_is_the_most_recent_packet_observed_actor_event_not_a_current_character_snapshot": true,
                "cross_capture_witness_is_diagnostic_not_controlled_counterfactual_proof": true,
                "formula_authority": false,
                "runtime_authority": false,
                "provider_rdps_credit_allowed": false
            },
            "processing": {
                "memory_limit_mib": args.memory_limit_mib,
                "partition_count": cohort.cross_capture_partition_paths.len(),
                "sample_count": cohort.sample_count,
                "measured_peak_working_set_bytes": measured_peak_working_set_bytes,
                "measured_peak_working_set_mib": measured_peak_working_set_bytes
                    .map(|bytes| bytes as f64 / (1024.0 * 1024.0)),
                "measured_peak_within_configured_limit": measured_peak_working_set_bytes
                    .map(|peak| peak <= configured_memory_limit_bytes),
                "partition_key": "packet calculation fields without ephemeral UUIDs; exact run scene; stable source actor config plus locally packet-observed direct-source and target actor shape; both exact status-state IDs; source attributes except CurrentHP; target attributes except CurrentHP plus all audited mitigation families"
            },
            "input": {
                "path": args.cohort.display().to_string(),
                "bytes": cohort.input_bytes,
                "sha256": cohort.input_sha256.clone(),
                "source_inputs": cohort.source_inputs.clone()
            },
            "target_identity_enrichment": cohort.target_identity_enrichment.clone(),
            "axes": diagnostic,
            "authority": {
                "exact_target_mitigation_formula_proven": false,
                "exact_operation_order_and_integer_rounding_proven": false,
                "packet_conservation_proven": false,
                "formula_authority": false,
                "runtime_authority": false,
                "provider_rdps_credit_allowed": false
            }
        });
        let mut diagnostic_writer = BufWriter::new(File::create(output)?);
        serde_json::to_writer_pretty(&mut diagnostic_writer, &report)?;
        diagnostic_writer.write_all(b"\n")?;
        diagnostic_writer.flush()?;
        println!("wrote {}", output.display());
    }
    if let (
        Some(output),
        Some((selected_sample_count, samples_by_ability, diagnostic, post_base_factor)),
    ) = (
        args.selected_ability_diagnostic_output.as_ref(),
        selected_ability_diagnostic,
    ) {
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let report = serde_json::json!({
            "schema_version": 2,
            "generated_by": "rlogs-bpsr-target-mitigation-transform-proof:selected-ability-diagnostic",
            "game_build": cohort.game_build.clone(),
            "selection": {
                "ability_ids": args.selected_ability_ids,
                "hit_event_id": args.selected_hit_event_id,
                "coefficient_basis_points": args.selected_coefficient_basis_points,
                "selected_sample_count": selected_sample_count,
                "samples_by_ability_id": samples_by_ability
            },
            "policy": {
                "exact_numeric_ability_ids_hit_event_id_and_build_are_authoritative": true,
                "local_or_offline_evidence_only": true,
                "remote_player_only_packets_are_required": false,
                "remote_player_only_packets_are_treated_as_zero": false,
                "remote_player_only_packets_are_synthesized": false,
                "same_capture_only": true,
                "cross_capture_pairing_allowed": false,
                "retained_source_and_target_status_state_ids_are_exact": true,
                "retained_source_attributes_except_current_hp_are_exact": true,
                "retained_target_attributes_except_current_hp_and_the_audited_axis_are_exact": true,
                "packet_unobservability_does_not_establish_a_complete_status_baseline": true,
                "absent_controlled_pairs_are_not_formula_proof": true,
                "formula_authority": false,
                "runtime_authority": false,
                "provider_rdps_credit_allowed": false
            },
            "processing": {
                "memory_limit_mib": args.memory_limit_mib,
                "partition_count": cohort.partition_paths.len(),
                "full_cohort_sample_count": cohort.sample_count,
                "measured_peak_working_set_bytes": measured_peak_working_set_bytes,
                "measured_peak_working_set_mib": measured_peak_working_set_bytes
                    .map(|bytes| bytes as f64 / (1024.0 * 1024.0)),
                "measured_peak_within_configured_limit": measured_peak_working_set_bytes
                    .map(|peak| peak <= configured_memory_limit_bytes),
                "exact_control_key": "same session/run, source and target entity, packet calculation identity, both retained status-state IDs, source attributes except CurrentHP, and target attributes except CurrentHP plus the audited mitigation family"
            },
            "input": {
                "path": args.cohort.display().to_string(),
                "bytes": cohort.input_bytes,
                "sha256": cohort.input_sha256.clone(),
                "source_inputs": cohort.source_inputs.clone()
            },
            "target_identity_enrichment": cohort.target_identity_enrichment.clone(),
            "axes": diagnostic,
            "post_base_integer_factor_diagnostic": post_base_factor,
            "authority": {
                "exact_target_mitigation_formula_proven": false,
                "exact_operation_order_and_integer_rounding_proven": false,
                "complete_status_baseline_proven": false,
                "packet_conservation_proven": false,
                "formula_authority": false,
                "runtime_authority": false,
                "provider_rdps_credit_allowed": false
            }
        });
        let mut diagnostic_writer = BufWriter::new(File::create(output)?);
        serde_json::to_writer_pretty(&mut diagnostic_writer, &report)?;
        diagnostic_writer.write_all(b"\n")?;
        diagnostic_writer.flush()?;
        println!("wrote {}", output.display());
    }
    Ok(())
}

fn write_target_identity_worklist(
    path: &Path,
    args: &Arguments,
    cohort: &PartitionedCohort,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut attribute_counts = BTreeMap::<i32, u64>::new();
    for request in &cohort.target_identity_worklist {
        for attribute_id in &request.observed_mitigation_attribute_ids {
            *attribute_counts.entry(*attribute_id).or_default() += 1;
        }
    }
    let output = serde_json::json!({
        "schema_version": 1,
        "generated_by": "rlogs-bpsr-target-mitigation-transform-proof:target-identity-worklist",
        "game_build": cohort.game_build.clone(),
        "policy": {
            "exact_session_run_sequence_and_target_keys_only": true,
            "target_allegiance_assumed": false,
            "recipient_or_enemy_target_are_both_allowed": true,
            "remote_player_only_packets_are_required": false,
            "remote_player_only_packets_are_zero_filled": false,
            "current_actor_snapshots_are_substituted": false,
            "unresolved_actor_identity_is_preserved": true,
            "formula_authority": false,
            "runtime_authority": false,
            "provider_rdps_credit_allowed": false
        },
        "input": {
            "path": args.cohort.display().to_string(),
            "bytes": cohort.input_bytes,
            "sha256": cohort.input_sha256.clone(),
            "source_inputs": cohort.source_inputs.clone()
        },
        "summary": {
            "requested_actions": cohort.target_identity_worklist.len(),
            "observed_mitigation_attribute_counts": attribute_counts
        },
        "observations": &cohort.target_identity_worklist
    });
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(path)?);
    serde_json::to_writer_pretty(&mut writer, &output)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    println!("wrote {}", path.display());
    Ok(())
}

struct CohortSeed {
    partition_count: usize,
    work_dir: PathBuf,
    target_status_relaxed_diagnostic: bool,
    cross_capture_target_config_diagnostic: bool,
    target_identity_enrichment: Option<TargetIdentityEnrichment>,
}

impl<'de> DeserializeSeed<'de> for CohortSeed {
    type Value = PartitionedCohort;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(CohortVisitor {
            partition_count: self.partition_count,
            work_dir: self.work_dir,
            target_status_relaxed_diagnostic: self.target_status_relaxed_diagnostic,
            cross_capture_target_config_diagnostic: self.cross_capture_target_config_diagnostic,
            target_identity_enrichment: self.target_identity_enrichment,
        })
    }
}

struct CohortVisitor {
    partition_count: usize,
    work_dir: PathBuf,
    target_status_relaxed_diagnostic: bool,
    cross_capture_target_config_diagnostic: bool,
    target_identity_enrichment: Option<TargetIdentityEnrichment>,
}

impl<'de> Visitor<'de> for CohortVisitor {
    type Value = PartitionedCohort;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a formula cohort object")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let partition_paths = (0..self.partition_count)
            .map(|index| self.work_dir.join(format!("partition-{index:04}.ndjson")))
            .collect::<Vec<_>>();
        let mut writers = partition_paths
            .iter()
            .map(|path| {
                File::create(path)
                    .map(BufWriter::new)
                    .map_err(serde::de::Error::custom)
            })
            .collect::<Result<Vec<_>, A::Error>>()?;
        let diagnostic_partition_paths = if self.target_status_relaxed_diagnostic {
            (0..self.partition_count)
                .map(|index| {
                    self.work_dir
                        .join(format!("diagnostic-partition-{index:04}.ndjson"))
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let mut diagnostic_writers = diagnostic_partition_paths
            .iter()
            .map(|path| {
                File::create(path)
                    .map(BufWriter::new)
                    .map_err(serde::de::Error::custom)
            })
            .collect::<Result<Vec<_>, A::Error>>()?;
        let cross_capture_partition_paths = if self.cross_capture_target_config_diagnostic {
            (0..self.partition_count)
                .map(|index| {
                    self.work_dir
                        .join(format!("cross-capture-partition-{index:04}.ndjson"))
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let mut cross_capture_writers = cross_capture_partition_paths
            .iter()
            .map(|path| {
                File::create(path)
                    .map(BufWriter::new)
                    .map_err(serde::de::Error::custom)
            })
            .collect::<Result<Vec<_>, A::Error>>()?;
        let mut schema_version = None;
        let mut game_build = None;
        let mut source_inputs = Vec::new();
        let mut attribute_states: Option<Vec<Vec<Attribute>>> = None;
        let mut status_states: Option<Vec<Vec<Status>>> = None;
        let mut sample_count = 0usize;
        let mut saw_samples = false;
        let mut matched_target_identity_keys = BTreeSet::<TargetIdentityKey>::new();
        let mut matched_source_identity_keys = BTreeSet::<TargetIdentityKey>::new();
        let mut matched_scene_keys = BTreeSet::<TargetIdentityKey>::new();
        let mut target_identity_requests =
            BTreeMap::<TargetIdentityKey, TargetIdentityRequest>::new();
        let target_exclusions = all_mitigation_attribute_ids();

        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "schema_version" => schema_version = Some(map.next_value::<u16>()?),
                "game_build" => game_build = map.next_value()?,
                "inputs" => source_inputs = map.next_value()?,
                "attribute_states" => attribute_states = Some(map.next_value()?),
                "status_states" => {
                    status_states = Some(map.next_value()?);
                }
                "samples" => {
                    if saw_samples {
                        return Err(serde::de::Error::duplicate_field("samples"));
                    }
                    let attributes = attribute_states.as_ref().ok_or_else(|| {
                        serde::de::Error::custom(
                            "attribute_states must precede samples for bounded-memory analysis",
                        )
                    })?;
                    let statuses = status_states.as_ref().ok_or_else(|| {
                        serde::de::Error::custom(
                            "status_states must precede samples for bounded-memory analysis",
                        )
                    })?;
                    saw_samples = true;
                    map.next_value_seed(SamplesSeed {
                        writers: &mut writers,
                        diagnostic_writers: &mut diagnostic_writers,
                        cross_capture_writers: &mut cross_capture_writers,
                        attribute_states: attributes,
                        status_state_count: statuses.len(),
                        target_exclusions: &target_exclusions,
                        sample_count: &mut sample_count,
                        target_identity_enrichment: self.target_identity_enrichment.as_ref(),
                        matched_target_identity_keys: &mut matched_target_identity_keys,
                        matched_source_identity_keys: &mut matched_source_identity_keys,
                        matched_scene_keys: &mut matched_scene_keys,
                        target_identity_requests: &mut target_identity_requests,
                    })?;
                }
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        for writer in &mut writers {
            writer.flush().map_err(serde::de::Error::custom)?;
        }
        for writer in &mut diagnostic_writers {
            writer.flush().map_err(serde::de::Error::custom)?;
        }
        for writer in &mut cross_capture_writers {
            writer.flush().map_err(serde::de::Error::custom)?;
        }
        drop(writers);
        drop(diagnostic_writers);
        drop(cross_capture_writers);
        if !saw_samples {
            return Err(serde::de::Error::missing_field("samples"));
        }
        if !REQUIRED_FORMULA_COHORT_SCHEMA_VERSIONS.contains(&schema_version.unwrap_or(0)) {
            return Err(serde::de::Error::custom(format!(
                "formula cohort schema must be one of {:?}",
                REQUIRED_FORMULA_COHORT_SCHEMA_VERSIONS,
            )));
        }
        let attribute_states =
            attribute_states.ok_or_else(|| serde::de::Error::missing_field("attribute_states"))?;
        let status_states =
            status_states.ok_or_else(|| serde::de::Error::missing_field("status_states"))?;
        let largest_partition_bytes = partition_paths
            .iter()
            .chain(diagnostic_partition_paths.iter())
            .chain(cross_capture_partition_paths.iter())
            .map(|path| fs::metadata(path).map(|metadata| metadata.len()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(serde::de::Error::custom)?
            .into_iter()
            .max()
            .unwrap_or(0);
        if self
            .target_identity_enrichment
            .as_ref()
            .is_some_and(|enrichment| game_build.as_deref() != Some(enrichment.game_build.as_str()))
        {
            return Err(serde::de::Error::custom(
                "target identity proof build does not match formula cohort build",
            ));
        }
        let target_identity_enrichment = self.target_identity_enrichment.map(|enrichment| {
            let declared_observations = enrichment.declared_observations;
            let declared = enrichment.target_identities.len();
            let declared_sources = enrichment.source_identities.len();
            let declared_scenes = enrichment.scene_ids.len();
            let matched = matched_target_identity_keys.len();
            TargetIdentityEnrichmentReport {
                path: enrichment.source.display().to_string(),
                bytes: enrichment.bytes,
                sha256: enrichment.sha256,
                game_build: enrichment.game_build,
                declared_action_observations: declared_observations,
                declared_event_time_target_actor_observations: declared,
                unresolved_action_observations: declared_observations.saturating_sub(declared),
                declared_event_time_source_actor_observations: declared_sources,
                declared_event_time_scene_observations: declared_scenes,
                exact_formula_cohort_sample_joins: matched,
                exact_formula_cohort_source_actor_joins: matched_source_identity_keys.len(),
                exact_formula_cohort_scene_joins: matched_scene_keys.len(),
                declared_identities_without_formula_cohort_sample: declared.saturating_sub(matched),
                formula_cohort_identity_conflicts: 0,
                policy: "exact session, run, canonical sequence, damage actor, target actor, and scene join only; event-time actor identities fill only the matching formula-cohort action and never supply static or current-profile stats",
                formula_authority: false,
                runtime_authority: false,
                provider_rdps_credit_allowed: false,
            }
        });
        Ok(PartitionedCohort {
            game_build,
            source_inputs,
            input_bytes: 0,
            input_sha256: String::new(),
            attribute_states,
            status_states,
            sample_count,
            partition_paths,
            diagnostic_partition_paths,
            cross_capture_partition_paths,
            work_dir: self.work_dir,
            largest_partition_bytes,
            target_identity_worklist: target_identity_requests.into_values().collect(),
            target_identity_enrichment,
        })
    }
}

struct SamplesSeed<'a> {
    writers: &'a mut [BufWriter<File>],
    diagnostic_writers: &'a mut [BufWriter<File>],
    cross_capture_writers: &'a mut [BufWriter<File>],
    attribute_states: &'a [Vec<Attribute>],
    status_state_count: usize,
    target_exclusions: &'a BTreeSet<i32>,
    sample_count: &'a mut usize,
    target_identity_enrichment: Option<&'a TargetIdentityEnrichment>,
    matched_target_identity_keys: &'a mut BTreeSet<TargetIdentityKey>,
    matched_source_identity_keys: &'a mut BTreeSet<TargetIdentityKey>,
    matched_scene_keys: &'a mut BTreeSet<TargetIdentityKey>,
    target_identity_requests: &'a mut BTreeMap<TargetIdentityKey, TargetIdentityRequest>,
}

impl<'de> DeserializeSeed<'de> for SamplesSeed<'_> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for SamplesSeed<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("an array of damage samples")
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        while let Some(mut sample) = sequence.next_element::<Sample>()? {
            if sample.source_attribute_state_id >= self.attribute_states.len()
                || sample.target_attribute_state_id >= self.attribute_states.len()
                || sample.source_status_state_id >= self.status_state_count
                || sample.target_status_state_id >= self.status_state_count
            {
                return Err(serde::de::Error::custom(format!(
                    "sample sequence {} references a missing interned state",
                    sample.sequence,
                )));
            }
            if let Some(enrichment) = self.target_identity_enrichment {
                let key = (
                    sample.session_id.clone(),
                    sample.run_ordinal,
                    sample.sequence,
                    sample.target_entity_uuid,
                );
                if let Some(identity) = enrichment.target_identities.get(&key) {
                    if !self.matched_target_identity_keys.insert(key.clone()) {
                        return Err(serde::de::Error::custom(format!(
                            "formula cohort contains duplicate sample identity for sequence {}",
                            sample.sequence,
                        )));
                    }
                    if let Some(existing) = sample.target_actor_identity.as_ref() {
                        let conflicts = existing.entity_type_id != identity.entity_type_id
                            || (existing.monster_id.is_some()
                                && existing.monster_id != identity.monster_id)
                            || (existing.character_id.is_some()
                                && existing.character_id != identity.character_id)
                            || (existing.level.is_some()
                                && identity.level.is_some()
                                && existing.level != identity.level);
                        if conflicts {
                            return Err(serde::de::Error::custom(format!(
                                "event-time target identity conflicts with formula cohort sequence {}",
                                sample.sequence,
                            )));
                        }
                    } else {
                        sample.target_actor_identity = Some(identity.clone());
                    }
                }
                if let Some(identity) = enrichment.source_identities.get(&key) {
                    if !self.matched_source_identity_keys.insert(key.clone()) {
                        return Err(serde::de::Error::custom(format!(
                            "formula cohort contains duplicate source identity for sequence {}",
                            sample.sequence,
                        )));
                    }
                    if let Some(existing) = sample.source_actor_identity.as_ref() {
                        let conflicts = existing.entity_type_id != identity.entity_type_id
                            || (existing.monster_id.is_some()
                                && existing.monster_id != identity.monster_id)
                            || (existing.character_id.is_some()
                                && existing.character_id != identity.character_id)
                            || (existing.level.is_some()
                                && identity.level.is_some()
                                && existing.level != identity.level);
                        if conflicts {
                            return Err(serde::de::Error::custom(format!(
                                "event-time source identity conflicts with formula cohort sequence {}",
                                sample.sequence,
                            )));
                        }
                    } else {
                        sample.source_actor_identity = Some(identity.clone());
                    }
                }
                if let Some(scene_id) = enrichment.scene_ids.get(&key) {
                    if !self.matched_scene_keys.insert(key) {
                        return Err(serde::de::Error::custom(format!(
                            "formula cohort contains duplicate scene identity for sequence {}",
                            sample.sequence,
                        )));
                    }
                    if sample
                        .scene_id
                        .is_some_and(|existing| existing != *scene_id)
                    {
                        return Err(serde::de::Error::custom(format!(
                            "event-time scene identity conflicts with formula cohort sequence {}",
                            sample.sequence,
                        )));
                    }
                    sample.scene_id = Some(*scene_id);
                }
            }
            let observed_mitigation_attribute_ids = observed_mitigation_attribute_ids(
                &self.attribute_states[sample.target_attribute_state_id],
                self.target_exclusions,
            );
            if !observed_mitigation_attribute_ids.is_empty() {
                let observed_mitigation_attributes = self.attribute_states
                    [sample.target_attribute_state_id]
                    .iter()
                    .filter(|attribute| {
                        observed_mitigation_attribute_ids.contains(&attribute.attribute_id)
                    })
                    .cloned()
                    .collect();
                let key = (
                    sample.session_id.clone(),
                    sample.run_ordinal,
                    sample.sequence,
                    sample.target_entity_uuid,
                );
                let request = TargetIdentityRequest {
                    session_id: sample.session_id.clone(),
                    run_ordinal: sample.run_ordinal,
                    sequence: sample.sequence,
                    source_entity_uuid: sample.source_entity_uuid,
                    direct_source_entity_uuid: sample.direct_source_entity_uuid,
                    target_entity_uuid: sample.target_entity_uuid,
                    ability_id: sample.ability_id,
                    hit_event_id: sample.hit_event_id,
                    damage_source: sample.damage_source,
                    damage_type: sample.damage_type,
                    packet_property: sample.packet.property,
                    amount: sample.amount,
                    normal_value: sample.normal_value,
                    source_attribute_state_id: sample.source_attribute_state_id,
                    target_attribute_state_id: sample.target_attribute_state_id,
                    source_status_state_id: sample.source_status_state_id,
                    target_status_state_id: sample.target_status_state_id,
                    observed_mitigation_attribute_ids,
                    observed_mitigation_attributes,
                };
                if let Some(previous) = self.target_identity_requests.insert(key, request) {
                    return Err(serde::de::Error::custom(format!(
                        "formula cohort contains duplicate mitigation target action key {}:{}",
                        previous.session_id, previous.sequence,
                    )));
                }
            }
            let partition_index = partition_index(
                &sample,
                self.attribute_states,
                self.target_exclusions,
                self.writers.len(),
            );
            serde_json::to_writer(&mut self.writers[partition_index], &sample)
                .map_err(serde::de::Error::custom)?;
            self.writers[partition_index]
                .write_all(b"\n")
                .map_err(serde::de::Error::custom)?;
            if !self.diagnostic_writers.is_empty() {
                let diagnostic_index = target_status_relaxed_partition_index(
                    &sample,
                    self.attribute_states,
                    self.target_exclusions,
                    self.diagnostic_writers.len(),
                );
                serde_json::to_writer(&mut self.diagnostic_writers[diagnostic_index], &sample)
                    .map_err(serde::de::Error::custom)?;
                self.diagnostic_writers[diagnostic_index]
                    .write_all(b"\n")
                    .map_err(serde::de::Error::custom)?;
            }
            if !self.cross_capture_writers.is_empty() {
                let cross_capture_index = cross_capture_target_config_partition_index(
                    &sample,
                    self.attribute_states,
                    self.target_exclusions,
                    self.cross_capture_writers.len(),
                );
                serde_json::to_writer(
                    &mut self.cross_capture_writers[cross_capture_index],
                    &sample,
                )
                .map_err(serde::de::Error::custom)?;
                self.cross_capture_writers[cross_capture_index]
                    .write_all(b"\n")
                    .map_err(serde::de::Error::custom)?;
            }
            *self.sample_count = self.sample_count.saturating_add(1);
        }
        Ok(())
    }
}

fn load_partitioned_cohort(
    args: &Arguments,
    target_identity_enrichment: Option<TargetIdentityEnrichment>,
) -> Result<PartitionedCohort, Box<dyn std::error::Error>> {
    let input_bytes = fs::metadata(&args.cohort)?.len();
    let memory_bytes = u64::try_from(args.memory_limit_mib)
        .unwrap_or(u64::MAX)
        .saturating_mul(1024 * 1024);
    let raw_partition_target = (memory_bytes / RAW_PARTITION_MEMORY_DIVISOR).max(1);
    let estimated_partitions = input_bytes
        .saturating_add(raw_partition_target - 1)
        .checked_div(raw_partition_target)
        .unwrap_or(u64::MAX);
    let partition_count = usize::try_from(estimated_partitions)
        .unwrap_or(MAX_PARTITIONS)
        .clamp(MIN_PARTITIONS, MAX_PARTITIONS)
        .next_power_of_two();
    let output_parent = args.output.parent().unwrap_or_else(|| Path::new("."));
    let output_name = args
        .output
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("target-mitigation-proof");
    let work_dir = output_parent.join(format!(".{output_name}.partitions-{}", std::process::id(),));
    fs::create_dir_all(output_parent)?;
    fs::create_dir(&work_dir).map_err(|error| {
        format!(
            "cannot create isolated partition directory {}: {error}",
            work_dir.display(),
        )
    })?;
    let result = CohortSeed {
        partition_count,
        work_dir: work_dir.clone(),
        target_status_relaxed_diagnostic: args.target_status_relaxed_diagnostic_output.is_some(),
        cross_capture_target_config_diagnostic: args
            .cross_capture_target_config_diagnostic_output
            .is_some(),
        target_identity_enrichment,
    }
    .deserialize(&mut serde_json::Deserializer::from_reader(BufReader::new(
        File::open(&args.cohort)?,
    )));
    let mut cohort = match result {
        Ok(cohort) => cohort,
        Err(error) => {
            let _ = fs::remove_dir_all(&work_dir);
            return Err(error.into());
        }
    };
    cohort.input_bytes = input_bytes;
    cohort.input_sha256 = sha256_file(&args.cohort)?;
    if cohort.largest_partition_bytes > raw_partition_target.saturating_mul(2) {
        return Err(format!(
            "largest raw partition is {} bytes, exceeding the conservative {} MiB memory plan; rerun with a lower --memory-limit-mib to increase the partition count",
            cohort.largest_partition_bytes,
            args.memory_limit_mib,
        )
        .into());
    }
    Ok(cohort)
}

fn sha256_file(path: &Path) -> Result<String, std::io::Error> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

#[cfg(windows)]
fn peak_working_set_bytes() -> Option<u64> {
    use windows_sys::Win32::System::{
        ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
        Threading::GetCurrentProcess,
    };

    let mut counters = unsafe { std::mem::zeroed::<PROCESS_MEMORY_COUNTERS>() };
    counters.cb = u32::try_from(std::mem::size_of::<PROCESS_MEMORY_COUNTERS>()).ok()?;
    let result = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) };
    (result != 0).then_some(u64::try_from(counters.PeakWorkingSetSize).unwrap_or(u64::MAX))
}

#[cfg(not(windows))]
fn peak_working_set_bytes() -> Option<u64> {
    None
}

fn validate_state_references(cohort: &PartitionedCohort) -> Result<(), String> {
    if cohort.attribute_states.is_empty() || cohort.status_states.is_empty() {
        return Err(
            "formula cohort does not contain interned attribute and status states".to_owned(),
        );
    }
    Ok(())
}

fn all_mitigation_attribute_ids() -> BTreeSet<i32> {
    axes()
        .into_iter()
        .flat_map(|axis| axis.family)
        .chain([CURRENT_HP])
        .collect()
}

fn observed_mitigation_attribute_ids(
    attributes: &[Attribute],
    target_exclusions: &BTreeSet<i32>,
) -> BTreeSet<i32> {
    attributes
        .iter()
        .filter(|attribute| {
            attribute.attribute_id != CURRENT_HP
                && target_exclusions.contains(&attribute.attribute_id)
        })
        .map(|attribute| attribute.attribute_id)
        .collect()
}

fn partition_index(
    sample: &Sample,
    attribute_states: &[Vec<Attribute>],
    target_exclusions: &BTreeSet<i32>,
    partition_count: usize,
) -> usize {
    let mut hasher = DefaultHasher::new();
    identity(sample).hash(&mut hasher);
    sample.source_status_state_id.hash(&mut hasher);
    sample.target_status_state_id.hash(&mut hasher);
    hash_filtered_attributes(
        &attribute_states[sample.source_attribute_state_id],
        &[CURRENT_HP].into_iter().collect(),
        &mut hasher,
    );
    hash_filtered_attributes(
        &attribute_states[sample.target_attribute_state_id],
        target_exclusions,
        &mut hasher,
    );
    usize::try_from(hasher.finish()).unwrap_or(0) % partition_count
}

fn target_status_relaxed_partition_index(
    sample: &Sample,
    attribute_states: &[Vec<Attribute>],
    target_exclusions: &BTreeSet<i32>,
    partition_count: usize,
) -> usize {
    let mut hasher = DefaultHasher::new();
    identity(sample).hash(&mut hasher);
    sample.source_status_state_id.hash(&mut hasher);
    hash_filtered_attributes(
        &attribute_states[sample.source_attribute_state_id],
        &[CURRENT_HP].into_iter().collect(),
        &mut hasher,
    );
    hash_filtered_attributes(
        &attribute_states[sample.target_attribute_state_id],
        target_exclusions,
        &mut hasher,
    );
    usize::try_from(hasher.finish()).unwrap_or(0) % partition_count
}

fn cross_capture_target_config_partition_index(
    sample: &Sample,
    attribute_states: &[Vec<Attribute>],
    target_exclusions: &BTreeSet<i32>,
    partition_count: usize,
) -> usize {
    let mut hasher = DefaultHasher::new();
    cross_capture_calculation_identity(sample).hash(&mut hasher);
    sample.source_status_state_id.hash(&mut hasher);
    sample.target_status_state_id.hash(&mut hasher);
    hash_filtered_attributes(
        &attribute_states[sample.source_attribute_state_id],
        &[CURRENT_HP].into_iter().collect(),
        &mut hasher,
    );
    hash_filtered_attributes(
        &attribute_states[sample.target_attribute_state_id],
        target_exclusions,
        &mut hasher,
    );
    usize::try_from(hasher.finish()).unwrap_or(0) % partition_count
}

fn hash_filtered_attributes(
    attributes: &[Attribute],
    excluded: &BTreeSet<i32>,
    hasher: &mut DefaultHasher,
) {
    attributes
        .iter()
        .filter(|row| !excluded.contains(&row.attribute_id))
        .count()
        .hash(hasher);
    for row in attributes
        .iter()
        .filter(|row| !excluded.contains(&row.attribute_id))
    {
        row.hash(hasher);
    }
}

fn axes() -> Vec<Axis> {
    let mut axes = vec![
        axis("physical_defense", 11_350, 22_000, 6_500, None),
        axis("magic_defense", 11_360, 22_000, 6_500, None),
        axis("refined_defense", 11_420, 9_980, 6_500, None),
        axis("general_element_defense", 13_200, 11_000, 11_000, None),
    ];
    let names = [
        "fire", "water", "wood", "electric", "wind", "rock", "light", "dark",
    ];
    for (offset, name) in names.into_iter().enumerate() {
        axes.push(axis(
            Box::leak(format!("{name}_element_defense").into_boxed_str()),
            13_210 + i32::try_from(offset).unwrap_or(0) * 10,
            11_000,
            11_000,
            Some(i32::try_from(offset + 1).unwrap_or(0)),
        ));
    }
    axes
}

fn axis(
    name: &'static str,
    current_id: i32,
    transformed: i64,
    simple: i64,
    required_property: Option<i32>,
) -> Axis {
    let mut candidates = vec![Model {
        name: "transformed_curve",
        constant: transformed,
    }];
    if simple != transformed {
        candidates.push(Model {
            name: "runtime_simple_curve",
            constant: simple,
        });
    }
    Axis {
        name,
        current_id,
        family: (current_id..=current_id + 5).collect(),
        required_property,
        candidates,
    }
}

fn audit_partitioned_cohort(
    cohort: &PartitionedCohort,
    axes: &[Axis],
    limit: usize,
) -> Result<BTreeMap<String, AxisResult>, Box<dyn std::error::Error>> {
    let mut results = axes
        .iter()
        .map(|axis| (axis.name.to_owned(), empty_axis_result(axis)))
        .collect::<BTreeMap<_, _>>();
    for partition_path in &cohort.partition_paths {
        let mut samples = Vec::new();
        for line in BufReader::new(File::open(partition_path)?).lines() {
            let line = line?;
            if !line.is_empty() {
                samples.push(serde_json::from_str::<Sample>(&line)?);
            }
        }
        for axis in axes {
            let partial = audit_axis_samples(&cohort.attribute_states, &samples, axis, limit);
            merge_axis_result(
                results.get_mut(axis.name).expect("axis result exists"),
                partial,
                limit,
            );
        }
    }
    Ok(results)
}

type SelectedAbilityDiagnostic = (
    u64,
    BTreeMap<i64, u64>,
    BTreeMap<String, AxisResult>,
    serde_json::Value,
);

fn audit_selected_ability_partitions(
    cohort: &PartitionedCohort,
    axes: &[Axis],
    selected_ability_ids: &BTreeSet<i64>,
    selected_hit_event_id: i32,
    coefficient_basis_points: i64,
    limit: usize,
) -> Result<SelectedAbilityDiagnostic, Box<dyn std::error::Error>> {
    let mut selected_sample_count = 0u64;
    let mut samples_by_ability = selected_ability_ids
        .iter()
        .map(|ability_id| (*ability_id, 0u64))
        .collect::<BTreeMap<_, _>>();
    let mut results = axes
        .iter()
        .map(|axis| (axis.name.to_owned(), empty_axis_result(axis)))
        .collect::<BTreeMap<_, _>>();
    let mut samples_with_source_attack = 0u64;
    let mut samples_with_positive_base_and_output = 0u64;
    let mut samples_with_factor_interval = 0u64;
    let mut samples_without_factor_interval = 0u64;
    let mut samples_with_unique_factor = 0u64;
    let mut normal_value_matches_amount = 0u64;
    let mut factor_intervals_by_attack = BTreeMap::<i64, BTreeMap<(i128, i128), u64>>::new();
    let mut compatibility_by_calculation_context =
        BTreeMap::<CalculationContext, FactorCompatibilityCounters>::new();
    let mut compatibility_by_owner_stage =
        BTreeMap::<OwnerStageContext, FactorCompatibilityCounters>::new();
    let mut compatibility_by_complete_retained_state =
        BTreeMap::<CompleteRetainedStateContext, FactorCompatibilityCounters>::new();
    let mut compatibility_by_target_actor_identity =
        BTreeMap::<ActorIdentity, FactorCompatibilityCounters>::new();
    let mut eligible_samples_without_target_actor_identity = 0u64;
    let mut source_stage_attribute_coverage = SOURCE_CANDIDATE_STAGE_ATTRIBUTES
        .into_iter()
        .map(|(attribute_id, _)| (attribute_id, AttributeStageCoverage::default()))
        .collect::<BTreeMap<_, _>>();
    let mut target_stage_attribute_coverage = TARGET_CANDIDATE_STAGE_ATTRIBUTES
        .into_iter()
        .map(|(attribute_id, _)| (attribute_id, AttributeStageCoverage::default()))
        .collect::<BTreeMap<_, _>>();
    let mut critical_true_samples_with_source_attack = 0u64;
    let mut critical_true_samples_with_critical_damage = 0u64;
    let mut critical_true_samples_without_critical_damage = 0u64;
    let mut critical_stage_candidates = [
        ("additive_bonus_floor", true, false),
        ("additive_bonus_half_up", true, true),
        ("direct_total_floor", false, false),
        ("direct_total_half_up", false, true),
    ]
    .into_iter()
    .map(|(name, additive, half_up)| {
        (
            name,
            additive,
            half_up,
            CriticalStageCandidateCounters::default(),
        )
    })
    .collect::<Vec<_>>();
    let mut critical_stage_after_unknown_factor_candidates = [
        ("additive_bonus_floor", true, false),
        ("additive_bonus_half_up", true, true),
        ("direct_total_floor", false, false),
        ("direct_total_half_up", false, true),
    ]
    .into_iter()
    .map(|(name, additive, half_up)| {
        (
            name,
            additive,
            half_up,
            CriticalStageCandidateCounters::default(),
        )
    })
    .collect::<Vec<_>>();
    let mut source_stage_order_observations = Vec::new();
    let mut factor_examples = Vec::new();
    for partition_path in &cohort.partition_paths {
        let mut samples = Vec::new();
        for line in BufReader::new(File::open(partition_path)?).lines() {
            let line = line?;
            if line.is_empty() {
                continue;
            }
            let sample = serde_json::from_str::<Sample>(&line)?;
            if sample_matches_selected_ability(&sample, selected_ability_ids, selected_hit_event_id)
            {
                selected_sample_count += 1;
                *samples_by_ability
                    .get_mut(&sample.ability_id)
                    .expect("selected ability counter exists") += 1;
                if sample.normal_value == Some(sample.amount) {
                    normal_value_matches_amount += 1;
                }
                if let Some(attack) = attribute_value(
                    &cohort.attribute_states[sample.source_attribute_state_id],
                    11_330,
                ) {
                    samples_with_source_attack += 1;
                    let base = i128::from(attack)
                        .checked_mul(i128::from(coefficient_basis_points))
                        .and_then(|value| value.checked_div(10_000));
                    let output = sample.normal_value.map(i128::from);
                    if sample.critical == Some(true) {
                        critical_true_samples_with_source_attack += 1;
                        if let Some(critical_damage) = attribute_value(
                            &cohort.attribute_states[sample.source_attribute_state_id],
                            CRITICAL_DAMAGE,
                        ) {
                            critical_true_samples_with_critical_damage += 1;
                            for (_, additive, half_up, counters) in &mut critical_stage_candidates {
                                let factor = if *additive {
                                    i128::from(critical_damage).checked_add(10_000)
                                } else {
                                    Some(i128::from(critical_damage))
                                };
                                let staged_base = base.zip(factor).and_then(|(value, factor)| {
                                    apply_fixed_point_stage(value, factor, *half_up)
                                });
                                if staged_base.is_some_and(|value| value > 0)
                                    && output.is_some_and(|value| value > 0)
                                {
                                    counters.samples_with_positive_staged_base_and_output += 1;
                                    let interval = integer_factor_interval(
                                        output.expect("positive output was checked"),
                                        staged_base.expect("positive staged base was checked"),
                                    );
                                    if let Some((minimum, maximum)) = interval {
                                        counters.samples_with_integer_other_factor_interval += 1;
                                        if minimum == maximum {
                                            counters.samples_with_unique_integer_other_factor += 1;
                                        }
                                    } else {
                                        counters.samples_without_integer_other_factor_interval += 1;
                                    }
                                }
                            }
                            for (_, additive, half_up, counters) in
                                &mut critical_stage_after_unknown_factor_candidates
                            {
                                let factor = if *additive {
                                    i128::from(critical_damage).checked_add(10_000)
                                } else {
                                    Some(i128::from(critical_damage))
                                };
                                let intermediate_range =
                                    output.zip(factor).and_then(|(output, factor)| {
                                        fixed_point_stage_preimage(output, factor, *half_up)
                                    });
                                let interval = base.zip(intermediate_range).and_then(
                                    |(base, (minimum, maximum))| {
                                        integer_factor_interval_for_output_range(
                                            minimum, maximum, base,
                                        )
                                    },
                                );
                                if base.is_some_and(|value| value > 0)
                                    && output.is_some_and(|value| value > 0)
                                    && intermediate_range.is_some()
                                {
                                    counters.samples_with_positive_staged_base_and_output += 1;
                                    if let Some((minimum, maximum)) = interval {
                                        counters.samples_with_integer_other_factor_interval += 1;
                                        if minimum == maximum {
                                            counters.samples_with_unique_integer_other_factor += 1;
                                        }
                                    } else {
                                        counters.samples_without_integer_other_factor_interval += 1;
                                    }
                                }
                            }
                            let source_attributes =
                                &cohort.attribute_states[sample.source_attribute_state_id];
                            let source_stage_values = [
                                Some(critical_damage),
                                attribute_value(source_attributes, 11_940),
                                attribute_value(source_attributes, 12_550),
                                attribute_value(source_attributes, 13_170),
                            ];
                            if source_stage_values.iter().all(Option::is_some)
                                && base.is_some_and(|value| value > 0)
                                && output.is_some_and(|value| value > 0)
                            {
                                let mut raw_values_by_attribute_id =
                                    SOURCE_CANDIDATE_STAGE_ATTRIBUTES
                                        .iter()
                                        .filter_map(|(attribute_id, _)| {
                                            attribute_value(source_attributes, *attribute_id)
                                                .map(|value| (*attribute_id, value))
                                        })
                                        .collect::<BTreeMap<_, _>>();
                                raw_values_by_attribute_id.insert(12_510, critical_damage);
                                source_stage_order_observations.push(SourceStageOrderObservation {
                                    session_id: sample.session_id.clone(),
                                    run_ordinal: sample.run_ordinal,
                                    sequence: sample.sequence,
                                    source_entity_uuid: sample.source_entity_uuid,
                                    target_entity_uuid: sample.target_entity_uuid,
                                    source_attribute_state_id: sample.source_attribute_state_id,
                                    target_attribute_state_id: sample.target_attribute_state_id,
                                    source_status_state_id: sample.source_status_state_id,
                                    target_status_state_id: sample.target_status_state_id,
                                    calculation_context: CalculationContext::from(&sample),
                                    owner_stage_context: OwnerStageContext::from(&sample),
                                    base: base.expect("positive base was checked"),
                                    output: output.expect("positive output was checked"),
                                    raw_values_by_attribute_id,
                                });
                            }
                        } else {
                            critical_true_samples_without_critical_damage += 1;
                        }
                    }
                    if base.is_some_and(|value| value > 0) && output.is_some_and(|value| value > 0)
                    {
                        samples_with_positive_base_and_output += 1;
                        let interval = integer_factor_interval(
                            output.expect("positive output was checked"),
                            base.expect("positive base was checked"),
                        );
                        let calculation_context = CalculationContext::from(&sample);
                        let owner_stage_context = OwnerStageContext::from(&sample);
                        let complete_retained_state_context = CompleteRetainedStateContext {
                            calculation_context: calculation_context.clone(),
                            source_attribute_state_id: sample.source_attribute_state_id,
                            target_attribute_state_id: sample.target_attribute_state_id,
                            source_status_state_id: sample.source_status_state_id,
                            target_status_state_id: sample.target_status_state_id,
                        };
                        let target_actor_identity = sample.target_actor_identity.clone();
                        for counters in [
                            compatibility_by_calculation_context
                                .entry(calculation_context)
                                .or_default(),
                            compatibility_by_owner_stage
                                .entry(owner_stage_context)
                                .or_default(),
                            compatibility_by_complete_retained_state
                                .entry(complete_retained_state_context)
                                .or_default(),
                        ] {
                            counters.samples_with_positive_base_and_output += 1;
                            if interval.is_some() {
                                counters.samples_with_integer_factor_interval += 1;
                            } else {
                                counters.samples_without_integer_factor_interval += 1;
                            }
                            if interval.is_some_and(|(minimum, maximum)| minimum == maximum) {
                                counters.samples_with_unique_integer_factor += 1;
                            }
                        }
                        if let Some(target_actor_identity) = target_actor_identity {
                            let counters = compatibility_by_target_actor_identity
                                .entry(target_actor_identity)
                                .or_default();
                            counters.samples_with_positive_base_and_output += 1;
                            if interval.is_some() {
                                counters.samples_with_integer_factor_interval += 1;
                            } else {
                                counters.samples_without_integer_factor_interval += 1;
                            }
                            if interval.is_some_and(|(minimum, maximum)| minimum == maximum) {
                                counters.samples_with_unique_integer_factor += 1;
                            }
                        } else {
                            eligible_samples_without_target_actor_identity += 1;
                        }
                        observe_attribute_stage_coverage(
                            &cohort.attribute_states[sample.source_attribute_state_id],
                            interval,
                            &mut source_stage_attribute_coverage,
                        );
                        observe_attribute_stage_coverage(
                            &cohort.attribute_states[sample.target_attribute_state_id],
                            interval,
                            &mut target_stage_attribute_coverage,
                        );
                        if let Some((minimum, maximum)) = interval {
                            samples_with_factor_interval += 1;
                            if minimum == maximum {
                                samples_with_unique_factor += 1;
                            }
                            *factor_intervals_by_attack
                                .entry(attack)
                                .or_default()
                                .entry((minimum, maximum))
                                .or_default() += 1;
                            if factor_examples.len() < limit {
                                factor_examples.push(serde_json::json!({
                                    "session_id": sample.session_id,
                                    "run_ordinal": sample.run_ordinal,
                                    "sequence": sample.sequence,
                                    "source_entity_uuid": sample.source_entity_uuid,
                                    "target_entity_uuid": sample.target_entity_uuid,
                                    "source_attack": attack,
                                    "coefficient_base": base,
                                    "normal_value": output,
                                    "factor_minimum_basis_points": minimum,
                                    "factor_maximum_basis_points": maximum,
                                    "critical": sample.critical,
                                    "lucky": sample.lucky,
                                    "property": sample.packet.property,
                                }));
                            }
                        } else {
                            samples_without_factor_interval += 1;
                        }
                    }
                }
                samples.push(sample);
            }
        }
        for axis in axes {
            let partial = audit_axis_samples(&cohort.attribute_states, &samples, axis, limit);
            merge_axis_result(
                results.get_mut(axis.name).expect("axis result exists"),
                partial,
                limit,
            );
        }
    }
    let interval_rows = factor_intervals_by_attack
        .into_iter()
        .map(|(attack, intervals)| {
            serde_json::json!({
                "source_attack": attack,
                "samples": intervals.values().sum::<u64>(),
                "intervals": intervals.into_iter().map(|((minimum, maximum), samples)| {
                    serde_json::json!({
                        "minimum_basis_points": minimum,
                        "maximum_basis_points": maximum,
                        "samples": samples,
                    })
                }).collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    let mut complete_state_summary = serde_json::Map::new();
    let mut complete_state_rows = 0u64;
    let mut mixed_complete_state_rows = 0u64;
    let mut only_compatible_complete_state_rows = 0u64;
    let mut only_rejected_complete_state_rows = 0u64;
    let mut samples_in_mixed_complete_state_rows = 0u64;
    let mut mixed_complete_state_examples = Vec::new();
    for (context, counters) in &compatibility_by_complete_retained_state {
        complete_state_rows += 1;
        let compatible = counters.samples_with_integer_factor_interval > 0;
        let rejected = counters.samples_without_integer_factor_interval > 0;
        match (compatible, rejected) {
            (true, true) => {
                mixed_complete_state_rows += 1;
                samples_in_mixed_complete_state_rows +=
                    counters.samples_with_positive_base_and_output;
                if mixed_complete_state_examples.len() < limit {
                    mixed_complete_state_examples.push(serde_json::json!({
                        "context": context,
                        "counters": counters,
                    }));
                }
            }
            (true, false) => only_compatible_complete_state_rows += 1,
            (false, true) => only_rejected_complete_state_rows += 1,
            (false, false) => {}
        }
    }
    complete_state_summary.insert(
        "authority".to_owned(),
        serde_json::json!("offline_diagnostic_only_not_formula_or_runtime_authority"),
    );
    complete_state_summary.insert(
        "grouping_key".to_owned(),
        serde_json::json!("complete observed calculation context plus the exact interned source/target attribute-state IDs and source/target status-state IDs retained at the event"),
    );
    complete_state_summary.insert(
        "complete_retained_state_rows".to_owned(),
        serde_json::json!(complete_state_rows),
    );
    complete_state_summary.insert(
        "mixed_compatible_and_rejected_rows".to_owned(),
        serde_json::json!(mixed_complete_state_rows),
    );
    complete_state_summary.insert(
        "only_compatible_rows".to_owned(),
        serde_json::json!(only_compatible_complete_state_rows),
    );
    complete_state_summary.insert(
        "only_rejected_rows".to_owned(),
        serde_json::json!(only_rejected_complete_state_rows),
    );
    complete_state_summary.insert(
        "samples_in_mixed_compatible_and_rejected_rows".to_owned(),
        serde_json::json!(samples_in_mixed_complete_state_rows),
    );
    complete_state_summary.insert(
        "mixed_examples".to_owned(),
        serde_json::json!(mixed_complete_state_examples),
    );
    complete_state_summary.insert("formula_authority".to_owned(), serde_json::json!(false));
    complete_state_summary.insert("runtime_authority".to_owned(), serde_json::json!(false));
    complete_state_summary.insert(
        "provider_rdps_credit_allowed".to_owned(),
        serde_json::json!(false),
    );
    let critical_stage_diagnostic = serde_json::json!({
        "authority": "offline_candidate_stage_diagnostic_only_not_formula_or_runtime_authority",
        "critical_damage_attribute_id": CRITICAL_DAMAGE,
        "critical_damage_attribute_sync_scope": "local-player-only-not-area-of-interest",
        "missing_remote_critical_damage_is_not_zero": true,
        "stage_positions_tested": [
            "critical stage after the exact Attack coefficient base and before one unresolved nonnegative integer factor",
            "one unresolved nonnegative integer factor after the exact Attack coefficient base and before the critical stage"
        ],
        "critical_true_samples_with_source_attack": critical_true_samples_with_source_attack,
        "critical_true_samples_with_critical_damage": critical_true_samples_with_critical_damage,
        "critical_true_samples_without_critical_damage": critical_true_samples_without_critical_damage,
        "critical_stage_before_unknown_factor_candidates": critical_stage_candidates
            .into_iter()
            .map(|(name, additive, half_up, counters)| serde_json::json!({
                "name": name,
                "critical_factor_expression": if additive {
                    "10000 + source_attribute_12510"
                } else {
                    "source_attribute_12510"
                },
                "rounding": if half_up { "nearest_half_up" } else { "floor" },
                "counters": counters,
                "formula_authority": false,
                "runtime_authority": false,
                "provider_rdps_credit_allowed": false,
            }))
            .collect::<Vec<_>>(),
        "critical_stage_after_unknown_factor_candidates": critical_stage_after_unknown_factor_candidates
            .into_iter()
            .map(|(name, additive, half_up, counters)| serde_json::json!({
                "name": name,
                "critical_factor_expression": if additive {
                    "10000 + source_attribute_12510"
                } else {
                    "source_attribute_12510"
                },
                "rounding": if half_up { "nearest_half_up" } else { "floor" },
                "unknown_factor_rounding": "floor",
                "counters": counters,
                "formula_authority": false,
                "runtime_authority": false,
                "provider_rdps_credit_allowed": false,
            }))
            .collect::<Vec<_>>(),
        "formula_authority": false,
        "runtime_authority": false,
        "provider_rdps_credit_allowed": false,
    });
    let source_stage_attribute_coverage = stage_attribute_coverage_report(
        SOURCE_CANDIDATE_STAGE_ATTRIBUTES,
        source_stage_attribute_coverage,
        samples_with_positive_base_and_output,
    );
    let target_stage_attribute_coverage = stage_attribute_coverage_report(
        TARGET_CANDIDATE_STAGE_ATTRIBUTES,
        target_stage_attribute_coverage,
        samples_with_positive_base_and_output,
    );
    let source_stage_order_observation_count = source_stage_order_observations.len();
    let source_stage_order_diagnostic = serde_json::json!({
        "authority": "offline_exact_numeric_observation_input_only_not_formula_or_runtime_authority",
        "required_second_stage_evaluator": "tools/bpsr-source-stage-order-proof.mjs",
        "known_stage_attribute_ids": [12510, 11940, 12550, 13170],
        "retained_candidate_stage_attribute_ids": SOURCE_CANDIDATE_STAGE_ATTRIBUTES
            .iter()
            .map(|(attribute_id, _)| *attribute_id)
            .collect::<Vec<_>>(),
        "missing_candidate_stage_attributes_are_omitted_not_zero": true,
        "source_and_target_attribute_and_status_state_ids_are_retained_not_expanded_or_zero_filled": true,
        "packet_calculation_and_owner_stage_context_are_retained_without_inference": true,
        "observation_count": source_stage_order_observation_count,
        "observations": source_stage_order_observations,
        "formula_authority": false,
        "runtime_authority": false,
        "provider_rdps_credit_allowed": false,
    });
    let post_base_factor = serde_json::json!({
        "authority": "offline_diagnostic_only_not_formula_or_runtime_authority",
        "model": "observed_normal_value = floor(floor(source_Attack * selected_coefficient / 10000) * one_nonnegative_integer_factor / 10000)",
        "source_attack_attribute_id": 11330,
        "coefficient_basis_points": coefficient_basis_points,
        "selected_samples": selected_sample_count,
        "samples_with_source_attack": samples_with_source_attack,
        "samples_with_positive_base_and_output": samples_with_positive_base_and_output,
        "samples_with_integer_factor_interval": samples_with_factor_interval,
        "samples_without_integer_factor_interval": samples_without_factor_interval,
        "samples_with_unique_integer_factor": samples_with_unique_factor,
        "samples_where_normal_value_matches_amount": normal_value_matches_amount,
        "factor_intervals_by_source_attack": interval_rows,
        "factor_compatibility_by_calculation_context": compatibility_by_calculation_context
            .into_iter()
            .map(|(context, counters)| serde_json::json!({
                "context": context,
                "counters": counters,
            }))
            .collect::<Vec<_>>(),
        "factor_compatibility_by_owner_stage": compatibility_by_owner_stage
            .into_iter()
            .map(|(context, counters)| serde_json::json!({
                "context": context,
                "counters": counters,
            }))
            .collect::<Vec<_>>(),
        "factor_compatibility_by_complete_retained_state": complete_state_summary,
        "factor_compatibility_by_target_actor_identity": {
            "eligible_samples_without_target_actor_identity": eligible_samples_without_target_actor_identity,
            "rows": compatibility_by_target_actor_identity
                .into_iter()
                .map(|(target_actor_identity, counters)| serde_json::json!({
                    "target_actor_identity": target_actor_identity,
                    "counters": counters,
                }))
                .collect::<Vec<_>>(),
            "formula_authority": false,
            "runtime_authority": false,
            "provider_rdps_credit_allowed": false,
        },
        "critical_damage_stage_diagnostic": critical_stage_diagnostic,
        "source_candidate_stage_attribute_coverage": source_stage_attribute_coverage,
        "target_candidate_stage_attribute_coverage": target_stage_attribute_coverage,
        "source_stage_order_diagnostic": source_stage_order_diagnostic,
        "examples": factor_examples,
        "formula_authority": false,
        "runtime_authority": false,
        "provider_rdps_credit_allowed": false,
    });
    Ok((
        selected_sample_count,
        samples_by_ability,
        results,
        post_base_factor,
    ))
}

fn integer_factor_interval(output: i128, base: i128) -> Option<(i128, i128)> {
    if output < 0 || base <= 0 {
        return None;
    }
    let minimum = ceil_div(output.checked_mul(10_000)?, base)?;
    let maximum = ceil_div(output.checked_add(1)?.checked_mul(10_000)?, base)?.checked_sub(1)?;
    (minimum <= maximum).then_some((minimum, maximum))
}

fn observe_attribute_stage_coverage(
    attributes: &[Attribute],
    interval: Option<(i128, i128)>,
    coverage: &mut BTreeMap<i32, AttributeStageCoverage>,
) {
    for attribute in attributes {
        let Some(axis) = coverage.get_mut(&attribute.attribute_id) else {
            continue;
        };
        axis.samples_present += 1;
        if attribute.value != 0 {
            axis.samples_nonzero += 1;
        }
        if interval.is_some() {
            axis.samples_present_with_integer_factor_interval += 1;
        } else {
            axis.samples_present_without_integer_factor_interval += 1;
        }
        let value = axis.values.entry(attribute.value).or_default();
        value.samples_with_positive_base_and_output += 1;
        if interval.is_some() {
            value.samples_with_integer_factor_interval += 1;
        } else {
            value.samples_without_integer_factor_interval += 1;
        }
        if interval.is_some_and(|(minimum, maximum)| minimum == maximum) {
            value.samples_with_unique_integer_factor += 1;
        }
    }
}

fn stage_attribute_coverage_report<const N: usize>(
    attributes: [(i32, &str); N],
    mut coverage: BTreeMap<i32, AttributeStageCoverage>,
    eligible_samples: u64,
) -> serde_json::Value {
    serde_json::json!({
        "authority": "offline_numeric_attribute_coverage_only_not_stage_applicability_or_formula_authority",
        "localized_or_enum_semantic_labels_are_evidence_only": true,
        "absent_attributes_are_not_zero": true,
        "eligible_samples": eligible_samples,
        "attributes": attributes.into_iter().map(|(attribute_id, semantic_label_evidence)| {
            let axis = coverage.remove(&attribute_id).unwrap_or_default();
            serde_json::json!({
                "attribute_id": attribute_id,
                "semantic_label_evidence": semantic_label_evidence,
                "samples_present": axis.samples_present,
                "samples_missing": eligible_samples.saturating_sub(axis.samples_present),
                "samples_nonzero": axis.samples_nonzero,
                "samples_present_with_integer_factor_interval": axis.samples_present_with_integer_factor_interval,
                "samples_present_without_integer_factor_interval": axis.samples_present_without_integer_factor_interval,
                "distinct_values": axis.values.len(),
                "values": axis.values.into_iter().map(|(value, counters)| serde_json::json!({
                    "value": value,
                    "counters": counters,
                })).collect::<Vec<_>>(),
            })
        }).collect::<Vec<_>>(),
        "formula_authority": false,
        "runtime_authority": false,
        "provider_rdps_credit_allowed": false,
    })
}

fn apply_fixed_point_stage(value: i128, factor: i128, half_up: bool) -> Option<i128> {
    if value < 0 || factor < 0 {
        return None;
    }
    let numerator = value.checked_mul(factor)?;
    if half_up {
        numerator.checked_add(5_000)?.checked_div(10_000)
    } else {
        numerator.checked_div(10_000)
    }
}

fn fixed_point_stage_preimage(output: i128, factor: i128, half_up: bool) -> Option<(i128, i128)> {
    if output < 0 || factor <= 0 {
        return None;
    }
    let rounding_offset = if half_up { 5_000 } else { 0 };
    let minimum_numerator = output
        .checked_mul(10_000)?
        .checked_sub(rounding_offset)?
        .max(0);
    let maximum_exclusive_numerator = output
        .checked_add(1)?
        .checked_mul(10_000)?
        .checked_sub(rounding_offset)?;
    let minimum = ceil_div(minimum_numerator, factor)?;
    let maximum = ceil_div(maximum_exclusive_numerator, factor)?.checked_sub(1)?;
    (minimum <= maximum).then_some((minimum, maximum))
}

fn integer_factor_interval_for_output_range(
    minimum_output: i128,
    maximum_output: i128,
    base: i128,
) -> Option<(i128, i128)> {
    if minimum_output < 0 || maximum_output < minimum_output || base <= 0 {
        return None;
    }
    let minimum_factor = ceil_div(minimum_output.checked_mul(10_000)?, base)?;
    let maximum_factor =
        ceil_div(maximum_output.checked_add(1)?.checked_mul(10_000)?, base)?.checked_sub(1)?;
    (minimum_factor <= maximum_factor).then_some((minimum_factor, maximum_factor))
}

fn sample_matches_selected_ability(
    sample: &Sample,
    selected_ability_ids: &BTreeSet<i64>,
    selected_hit_event_id: i32,
) -> bool {
    selected_ability_ids.contains(&sample.ability_id)
        && sample.hit_event_id == Some(selected_hit_event_id)
}

fn audit_target_status_relaxed_partitions(
    cohort: &PartitionedCohort,
    axes: &[Axis],
    selected_effect_id: i64,
    limit: usize,
) -> Result<BTreeMap<String, TargetStatusRelaxedAxisResult>, Box<dyn std::error::Error>> {
    if cohort.diagnostic_partition_paths.is_empty() {
        return Err("target-status-relaxed diagnostic partitions were not produced".into());
    }
    let mut results = axes
        .iter()
        .map(|axis| {
            (
                axis.name.to_owned(),
                empty_target_status_relaxed_axis_result(axis),
            )
        })
        .collect::<BTreeMap<_, _>>();
    for partition_path in &cohort.diagnostic_partition_paths {
        let mut samples = Vec::new();
        for line in BufReader::new(File::open(partition_path)?).lines() {
            let line = line?;
            if !line.is_empty() {
                samples.push(serde_json::from_str::<Sample>(&line)?);
            }
        }
        for axis in axes {
            let partial = audit_target_status_relaxed_axis_samples(
                &cohort.attribute_states,
                &cohort.status_states,
                &samples,
                axis,
                selected_effect_id,
                limit,
            );
            merge_target_status_relaxed_axis_result(
                results.get_mut(axis.name).expect("diagnostic axis exists"),
                partial,
                limit,
            );
        }
    }
    Ok(results)
}

fn audit_cross_capture_target_config_partitions(
    cohort: &PartitionedCohort,
    axes: &[Axis],
    limit: usize,
) -> Result<BTreeMap<String, CrossCaptureAxisResult>, Box<dyn std::error::Error>> {
    if cohort.cross_capture_partition_paths.is_empty() {
        return Err("cross-capture target-config diagnostic partitions were not produced".into());
    }
    let mut results = axes
        .iter()
        .map(|axis| (axis.name.to_owned(), empty_cross_capture_axis_result(axis)))
        .collect::<BTreeMap<_, _>>();
    for partition_path in &cohort.cross_capture_partition_paths {
        let mut samples = Vec::new();
        for line in BufReader::new(File::open(partition_path)?).lines() {
            let line = line?;
            if !line.is_empty() {
                samples.push(serde_json::from_str::<Sample>(&line)?);
            }
        }
        for axis in axes {
            let partial = audit_cross_capture_target_config_axis_samples(
                &cohort.attribute_states,
                &samples,
                axis,
                limit,
            );
            merge_cross_capture_axis_result(
                results
                    .get_mut(axis.name)
                    .expect("cross-capture axis exists"),
                partial,
                limit,
            );
        }
    }
    Ok(results)
}

fn empty_cross_capture_axis_result(axis: &Axis) -> CrossCaptureAxisResult {
    CrossCaptureAxisResult {
        current_attribute_id: axis.current_id,
        family_attribute_ids: axis.family.clone(),
        required_packet_property: axis.required_property,
        counters: CrossCaptureCounters::default(),
        models: axis
            .candidates
            .iter()
            .map(|model| {
                (
                    model.name.to_owned(),
                    CrossCaptureModelResult {
                        constant: model.constant,
                        counters: CrossCaptureModelCounters::default(),
                        exact_examples: Vec::new(),
                        rejected_examples: Vec::new(),
                    },
                )
            })
            .collect(),
    }
}

fn merge_cross_capture_axis_result(
    target: &mut CrossCaptureAxisResult,
    source: CrossCaptureAxisResult,
    limit: usize,
) {
    assert_eq!(target.current_attribute_id, source.current_attribute_id);
    assert_eq!(target.family_attribute_ids, source.family_attribute_ids);
    assert_eq!(
        target.required_packet_property,
        source.required_packet_property
    );
    target.counters.samples_with_axis += source.counters.samples_with_axis;
    target
        .counters
        .samples_with_packet_observed_target_actor_identity += source
        .counters
        .samples_with_packet_observed_target_actor_identity;
    target.counters.samples_with_stable_target_actor_id +=
        source.counters.samples_with_stable_target_actor_id;
    target
        .counters
        .samples_with_cross_capture_actor_shape_context += source
        .counters
        .samples_with_cross_capture_actor_shape_context;
    target.counters.groups_with_multiple_axis_states +=
        source.counters.groups_with_multiple_axis_states;
    target.counters.distinct_axis_pairs += source.counters.distinct_axis_pairs;
    target.counters.pairs_with_cross_capture_witness +=
        source.counters.pairs_with_cross_capture_witness;
    target.counters.deterministic_cross_capture_pairs +=
        source.counters.deterministic_cross_capture_pairs;
    target.counters.equal_output_cross_capture_pairs +=
        source.counters.equal_output_cross_capture_pairs;
    target.counters.divergent_output_cross_capture_pairs +=
        source.counters.divergent_output_cross_capture_pairs;
    target.counters.nondeterministic_cross_capture_pairs +=
        source.counters.nondeterministic_cross_capture_pairs;
    for (name, mut source_model) in source.models {
        let target_model = target.models.get_mut(&name).expect("model exists");
        assert_eq!(target_model.constant, source_model.constant);
        target_model.counters.exact_pairs += source_model.counters.exact_pairs;
        target_model.counters.rejected_pairs += source_model.counters.rejected_pairs;
        let exact_remaining = limit.saturating_sub(target_model.exact_examples.len());
        target_model.exact_examples.extend(
            source_model
                .exact_examples
                .drain(..exact_remaining.min(source_model.exact_examples.len())),
        );
        let rejected_remaining = limit.saturating_sub(target_model.rejected_examples.len());
        target_model.rejected_examples.extend(
            source_model
                .rejected_examples
                .drain(..rejected_remaining.min(source_model.rejected_examples.len())),
        );
    }
}

fn audit_cross_capture_target_config_axis_samples(
    attribute_states: &[Vec<Attribute>],
    samples: &[Sample],
    axis: &Axis,
    limit: usize,
) -> CrossCaptureAxisResult {
    type Key = (
        CrossCaptureCalculationIdentity,
        usize,
        usize,
        Vec<Attribute>,
        Vec<Attribute>,
    );
    let family = axis.family.iter().copied().collect::<BTreeSet<_>>();
    let mut groups: HashMap<Key, BTreeMap<Vec<Attribute>, CrossCaptureBucket>> = HashMap::new();
    let mut result = empty_cross_capture_axis_result(axis);
    for sample in samples {
        if axis.required_property.is_some() && sample.packet.property != axis.required_property {
            continue;
        }
        let source = &attribute_states[sample.source_attribute_state_id];
        let target = &attribute_states[sample.target_attribute_state_id];
        let axis_state = target
            .iter()
            .filter(|row| family.contains(&row.attribute_id))
            .cloned()
            .collect::<Vec<_>>();
        if attribute_value(&axis_state, axis.current_id).is_none() {
            continue;
        }
        result.counters.samples_with_axis += 1;
        let Some(target_actor_identity) = sample.target_actor_identity.as_ref() else {
            continue;
        };
        result
            .counters
            .samples_with_packet_observed_target_actor_identity += 1;
        if target_actor_identity.monster_id.is_some()
            || target_actor_identity.character_id.is_some()
        {
            result.counters.samples_with_stable_target_actor_id += 1;
        }
        let Some(calculation_identity) = cross_capture_calculation_identity(sample) else {
            continue;
        };
        result
            .counters
            .samples_with_cross_capture_actor_shape_context += 1;
        let key = (
            calculation_identity,
            sample.source_status_state_id,
            sample.target_status_state_id,
            without(source, &[CURRENT_HP].into_iter().collect()),
            without(
                target,
                &family
                    .union(&[CURRENT_HP].into_iter().collect())
                    .copied()
                    .collect(),
            ),
        );
        let bucket = groups
            .entry(key)
            .or_default()
            .entry(axis_state)
            .or_default();
        bucket.observations.insert(CrossCaptureObservation {
            session_id: sample.session_id.clone(),
            run_ordinal: sample.run_ordinal,
            sequence: sample.sequence,
            source_entity_uuid: sample.source_entity_uuid,
            target_entity_uuid: sample.target_entity_uuid,
        });
        *bucket
            .outcomes
            .entry(Outcome {
                amount: sample.amount,
                normal_value: sample.normal_value,
            })
            .or_default() += 1;
    }

    for ((identity, _, _, _, _), states) in groups {
        if states.len() < 2 {
            continue;
        }
        result.counters.groups_with_multiple_axis_states += 1;
        let states = states.into_iter().collect::<Vec<_>>();
        for left_index in 0..states.len() {
            for right_index in left_index + 1..states.len() {
                let (left_state, left) = &states[left_index];
                let (right_state, right) = &states[right_index];
                let (Some(left_raw), Some(right_raw)) = (
                    attribute_value(left_state, axis.current_id),
                    attribute_value(right_state, axis.current_id),
                ) else {
                    continue;
                };
                if left_raw == right_raw {
                    continue;
                }
                result.counters.distinct_axis_pairs += 1;
                let Some((left_observation, right_observation)) =
                    cross_capture_observation_pair(left, right)
                else {
                    continue;
                };
                result.counters.pairs_with_cross_capture_witness += 1;
                let left_outcomes = left.outcomes.keys().cloned().collect::<Vec<_>>();
                let right_outcomes = right.outcomes.keys().cloned().collect::<Vec<_>>();
                if left_outcomes.len() != 1 || right_outcomes.len() != 1 {
                    result.counters.nondeterministic_cross_capture_pairs += 1;
                    continue;
                }
                result.counters.deterministic_cross_capture_pairs += 1;
                if left_outcomes == right_outcomes {
                    result.counters.equal_output_cross_capture_pairs += 1;
                } else {
                    result.counters.divergent_output_cross_capture_pairs += 1;
                }
                for model in &axis.candidates {
                    let compatible = shared_base_interval(
                        left_outcomes[0].amount,
                        left_raw,
                        right_outcomes[0].amount,
                        right_raw,
                        model.constant,
                    );
                    let example = CrossCaptureExample {
                        target_actor_identity: identity.target_actor_identity.clone(),
                        ability_id: identity.ability_id,
                        property: identity.property,
                        left_raw,
                        right_raw,
                        left_outcome: left_outcomes[0].clone(),
                        right_outcome: right_outcomes[0].clone(),
                        left_observation: left_observation.clone(),
                        right_observation: right_observation.clone(),
                        compatible_base_minimum: compatible.map(|value| value.0.to_string()),
                        compatible_base_maximum: compatible.map(|value| value.1.to_string()),
                    };
                    let model_result = result.models.get_mut(model.name).expect("model exists");
                    if compatible.is_some() {
                        model_result.counters.exact_pairs += 1;
                        if model_result.exact_examples.len() < limit {
                            model_result.exact_examples.push(example);
                        }
                    } else {
                        model_result.counters.rejected_pairs += 1;
                        if model_result.rejected_examples.len() < limit {
                            model_result.rejected_examples.push(example);
                        }
                    }
                }
            }
        }
    }
    result
}

fn cross_capture_observation_pair<'a>(
    left: &'a CrossCaptureBucket,
    right: &'a CrossCaptureBucket,
) -> Option<(&'a CrossCaptureObservation, &'a CrossCaptureObservation)> {
    left.observations.iter().find_map(|left_observation| {
        right
            .observations
            .iter()
            .find(|right_observation| left_observation.session_id != right_observation.session_id)
            .map(|right_observation| (left_observation, right_observation))
    })
}

fn empty_target_status_relaxed_axis_result(axis: &Axis) -> TargetStatusRelaxedAxisResult {
    TargetStatusRelaxedAxisResult {
        current_attribute_id: axis.current_id,
        family_attribute_ids: axis.family.clone(),
        required_packet_property: axis.required_property,
        counters: TargetStatusRelaxedCounters::default(),
        same_axis_status_examples: Vec::new(),
        selected_effect_same_axis_examples: Vec::new(),
        near_pair_examples: Vec::new(),
        selected_effect_examples: Vec::new(),
    }
}

fn merge_target_status_relaxed_axis_result(
    target: &mut TargetStatusRelaxedAxisResult,
    mut source: TargetStatusRelaxedAxisResult,
    limit: usize,
) {
    assert_eq!(target.current_attribute_id, source.current_attribute_id);
    assert_eq!(target.family_attribute_ids, source.family_attribute_ids);
    assert_eq!(
        target.required_packet_property,
        source.required_packet_property
    );
    target.counters.samples_with_axis += source.counters.samples_with_axis;
    target
        .counters
        .groups_with_multiple_target_status_or_axis_variants += source
        .counters
        .groups_with_multiple_target_status_or_axis_variants;
    target.counters.same_axis_status_pairs += source.counters.same_axis_status_pairs;
    target.counters.same_axis_deterministic_pairs += source.counters.same_axis_deterministic_pairs;
    target.counters.same_axis_equal_output_pairs += source.counters.same_axis_equal_output_pairs;
    target.counters.same_axis_divergent_output_pairs +=
        source.counters.same_axis_divergent_output_pairs;
    target.counters.same_axis_nondeterministic_pairs +=
        source.counters.same_axis_nondeterministic_pairs;
    target
        .counters
        .same_axis_pairs_with_selected_effect_in_status_delta += source
        .counters
        .same_axis_pairs_with_selected_effect_in_status_delta;
    target
        .counters
        .same_axis_pairs_with_only_selected_effect_in_status_delta += source
        .counters
        .same_axis_pairs_with_only_selected_effect_in_status_delta;
    target.counters.distinct_axis_pairs += source.counters.distinct_axis_pairs;
    target.counters.deterministic_pairs += source.counters.deterministic_pairs;
    target.counters.equal_output_pairs += source.counters.equal_output_pairs;
    target.counters.divergent_output_pairs += source.counters.divergent_output_pairs;
    target.counters.nondeterministic_pairs += source.counters.nondeterministic_pairs;
    target.counters.pairs_with_selected_effect_in_status_delta +=
        source.counters.pairs_with_selected_effect_in_status_delta;
    target
        .counters
        .pairs_with_only_selected_effect_in_status_delta += source
        .counters
        .pairs_with_only_selected_effect_in_status_delta;
    let same_axis_remaining = limit.saturating_sub(target.same_axis_status_examples.len());
    target.same_axis_status_examples.extend(
        source
            .same_axis_status_examples
            .drain(..same_axis_remaining.min(source.same_axis_status_examples.len())),
    );
    let selected_same_axis_remaining =
        limit.saturating_sub(target.selected_effect_same_axis_examples.len());
    target.selected_effect_same_axis_examples.extend(
        source.selected_effect_same_axis_examples.drain(
            ..selected_same_axis_remaining.min(source.selected_effect_same_axis_examples.len()),
        ),
    );
    let remaining = limit.saturating_sub(target.selected_effect_examples.len());
    target.selected_effect_examples.extend(
        source
            .selected_effect_examples
            .drain(..remaining.min(source.selected_effect_examples.len())),
    );
    let near_remaining = limit.saturating_sub(target.near_pair_examples.len());
    target.near_pair_examples.extend(
        source
            .near_pair_examples
            .drain(..near_remaining.min(source.near_pair_examples.len())),
    );
}

fn audit_target_status_relaxed_axis_samples(
    attribute_states: &[Vec<Attribute>],
    status_states: &[Vec<Status>],
    samples: &[Sample],
    axis: &Axis,
    selected_effect_id: i64,
    limit: usize,
) -> TargetStatusRelaxedAxisResult {
    type Key = (Identity, usize, Vec<Attribute>, Vec<Attribute>);
    type Variant = (usize, Vec<Attribute>);
    let family = axis.family.iter().copied().collect::<BTreeSet<_>>();
    let mut groups: HashMap<Key, BTreeMap<Variant, Bucket>> = HashMap::new();
    let mut counters = TargetStatusRelaxedCounters::default();
    let mut same_axis_status_examples = Vec::new();
    let mut selected_effect_same_axis_examples = Vec::new();
    let mut near_pair_examples = Vec::new();
    let mut examples = Vec::new();
    for sample in samples {
        if axis.required_property.is_some() && sample.packet.property != axis.required_property {
            continue;
        }
        let source = &attribute_states[sample.source_attribute_state_id];
        let target = &attribute_states[sample.target_attribute_state_id];
        let axis_state = target
            .iter()
            .filter(|row| family.contains(&row.attribute_id))
            .cloned()
            .collect::<Vec<_>>();
        if attribute_value(&axis_state, axis.current_id).is_none() {
            continue;
        }
        counters.samples_with_axis += 1;
        let key = (
            identity(sample),
            sample.source_status_state_id,
            without(source, &[CURRENT_HP].into_iter().collect()),
            without(
                target,
                &family
                    .union(&[CURRENT_HP].into_iter().collect())
                    .copied()
                    .collect(),
            ),
        );
        let bucket = groups
            .entry(key)
            .or_default()
            .entry((sample.target_status_state_id, axis_state))
            .or_default();
        bucket.sequences.insert(sample.sequence);
        *bucket
            .outcomes
            .entry(Outcome {
                amount: sample.amount,
                normal_value: sample.normal_value,
            })
            .or_default() += 1;
    }
    for ((identity, _, _, _), variants) in groups {
        if variants.len() < 2 {
            continue;
        }
        counters.groups_with_multiple_target_status_or_axis_variants += 1;
        let variants = variants.into_iter().collect::<Vec<_>>();
        for left_index in 0..variants.len() {
            for right_index in left_index + 1..variants.len() {
                let ((left_status_id, left_axis), left) = &variants[left_index];
                let ((right_status_id, right_axis), right) = &variants[right_index];
                let (Some(left_raw), Some(right_raw)) = (
                    attribute_value(left_axis, axis.current_id),
                    attribute_value(right_axis, axis.current_id),
                ) else {
                    continue;
                };
                let left_outcomes = left.outcomes.keys().cloned().collect::<Vec<_>>();
                let right_outcomes = right.outcomes.keys().cloned().collect::<Vec<_>>();
                let (left_only, right_only) = status_delta(
                    &status_states[*left_status_id],
                    &status_states[*right_status_id],
                );
                let selected_effect_present = left_only
                    .iter()
                    .chain(&right_only)
                    .any(|status| status.effect_id == selected_effect_id);
                let selected_effect_only = selected_effect_present
                    && left_only
                        .iter()
                        .chain(&right_only)
                        .all(|status| status.effect_id == selected_effect_id);
                if left_raw == right_raw {
                    if left_axis != right_axis || left_only.is_empty() && right_only.is_empty() {
                        continue;
                    }
                    counters.same_axis_status_pairs += 1;
                    if left_outcomes.len() != 1 || right_outcomes.len() != 1 {
                        counters.same_axis_nondeterministic_pairs += 1;
                        continue;
                    }
                    counters.same_axis_deterministic_pairs += 1;
                    if left_outcomes == right_outcomes {
                        counters.same_axis_equal_output_pairs += 1;
                    } else {
                        counters.same_axis_divergent_output_pairs += 1;
                    }
                    if same_axis_status_examples.len() < limit {
                        same_axis_status_examples.push(target_status_relaxed_example(
                            &identity,
                            left_raw,
                            right_raw,
                            &left_outcomes[0],
                            &right_outcomes[0],
                            *left_status_id,
                            *right_status_id,
                            &left_only,
                            &right_only,
                            selected_effect_only,
                            left,
                            right,
                        ));
                    }
                    if selected_effect_present {
                        counters.same_axis_pairs_with_selected_effect_in_status_delta += 1;
                        if selected_effect_only {
                            counters.same_axis_pairs_with_only_selected_effect_in_status_delta += 1;
                        }
                        if selected_effect_same_axis_examples.len() < limit {
                            selected_effect_same_axis_examples.push(target_status_relaxed_example(
                                &identity,
                                left_raw,
                                right_raw,
                                &left_outcomes[0],
                                &right_outcomes[0],
                                *left_status_id,
                                *right_status_id,
                                &left_only,
                                &right_only,
                                selected_effect_only,
                                left,
                                right,
                            ));
                        }
                    }
                    continue;
                }
                counters.distinct_axis_pairs += 1;
                if left_outcomes.len() != 1 || right_outcomes.len() != 1 {
                    counters.nondeterministic_pairs += 1;
                    continue;
                }
                counters.deterministic_pairs += 1;
                if left_outcomes == right_outcomes {
                    counters.equal_output_pairs += 1;
                } else {
                    counters.divergent_output_pairs += 1;
                }
                if near_pair_examples.len() < limit {
                    near_pair_examples.push(target_status_relaxed_example(
                        &identity,
                        left_raw,
                        right_raw,
                        &left_outcomes[0],
                        &right_outcomes[0],
                        *left_status_id,
                        *right_status_id,
                        &left_only,
                        &right_only,
                        selected_effect_only,
                        left,
                        right,
                    ));
                }
                if !selected_effect_present {
                    continue;
                }
                counters.pairs_with_selected_effect_in_status_delta += 1;
                if selected_effect_only {
                    counters.pairs_with_only_selected_effect_in_status_delta += 1;
                }
                if examples.len() < limit {
                    examples.push(target_status_relaxed_example(
                        &identity,
                        left_raw,
                        right_raw,
                        &left_outcomes[0],
                        &right_outcomes[0],
                        *left_status_id,
                        *right_status_id,
                        &left_only,
                        &right_only,
                        selected_effect_only,
                        left,
                        right,
                    ));
                }
            }
        }
    }
    TargetStatusRelaxedAxisResult {
        current_attribute_id: axis.current_id,
        family_attribute_ids: axis.family.clone(),
        required_packet_property: axis.required_property,
        counters,
        same_axis_status_examples,
        selected_effect_same_axis_examples,
        near_pair_examples,
        selected_effect_examples: examples,
    }
}

#[allow(clippy::too_many_arguments)]
fn target_status_relaxed_example(
    identity: &Identity,
    left_raw: i64,
    right_raw: i64,
    left_outcome: &Outcome,
    right_outcome: &Outcome,
    left_status_id: usize,
    right_status_id: usize,
    left_only: &[Status],
    right_only: &[Status],
    selected_effect_only: bool,
    left: &Bucket,
    right: &Bucket,
) -> TargetStatusRelaxedExample {
    TargetStatusRelaxedExample {
        session_id: identity.session_id.clone(),
        run_ordinal: identity.run_ordinal,
        source_entity_uuid: identity.source_entity_uuid,
        target_entity_uuid: identity.target_entity_uuid,
        ability_id: identity.ability_id,
        property: identity.property,
        left_raw,
        right_raw,
        left_outcome: left_outcome.clone(),
        right_outcome: right_outcome.clone(),
        left_target_status_state_id: left_status_id,
        right_target_status_state_id: right_status_id,
        left_only_statuses: left_only.to_vec(),
        right_only_statuses: right_only.to_vec(),
        selected_effect_is_the_only_status_delta: selected_effect_only,
        left_sequences: left.sequences.iter().copied().collect(),
        right_sequences: right.sequences.iter().copied().collect(),
    }
}

fn status_delta(left: &[Status], right: &[Status]) -> (Vec<Status>, Vec<Status>) {
    let left = left.iter().cloned().collect::<BTreeSet<_>>();
    let right = right.iter().cloned().collect::<BTreeSet<_>>();
    (
        left.difference(&right).cloned().collect(),
        right.difference(&left).cloned().collect(),
    )
}

fn empty_axis_result(axis: &Axis) -> AxisResult {
    AxisResult {
        current_attribute_id: axis.current_id,
        family_attribute_ids: axis.family.clone(),
        required_packet_property: axis.required_property,
        counters: Counters::default(),
        models: axis
            .candidates
            .iter()
            .map(|model| {
                (
                    model.name.to_owned(),
                    ModelResult {
                        constant: model.constant,
                        counters: ModelCounters::default(),
                        exact_examples: Vec::new(),
                        rejected_examples: Vec::new(),
                    },
                )
            })
            .collect(),
    }
}

fn merge_axis_result(target: &mut AxisResult, source: AxisResult, limit: usize) {
    assert_eq!(target.current_attribute_id, source.current_attribute_id);
    assert_eq!(target.family_attribute_ids, source.family_attribute_ids);
    assert_eq!(
        target.required_packet_property,
        source.required_packet_property
    );
    target.counters.samples_with_axis += source.counters.samples_with_axis;
    target.counters.controlled_groups += source.counters.controlled_groups;
    target.counters.distinct_axis_pairs += source.counters.distinct_axis_pairs;
    target.counters.deterministic_pairs += source.counters.deterministic_pairs;
    target.counters.equal_output_pairs += source.counters.equal_output_pairs;
    target.counters.divergent_output_pairs += source.counters.divergent_output_pairs;
    target.counters.nondeterministic_pairs += source.counters.nondeterministic_pairs;
    for (name, mut source_model) in source.models {
        let target_model = target.models.get_mut(&name).expect("model result exists");
        assert_eq!(target_model.constant, source_model.constant);
        target_model.counters.exact_pairs += source_model.counters.exact_pairs;
        target_model.counters.rejected_pairs += source_model.counters.rejected_pairs;
        let exact_remaining = limit.saturating_sub(target_model.exact_examples.len());
        target_model.exact_examples.extend(
            source_model
                .exact_examples
                .drain(..exact_remaining.min(source_model.exact_examples.len())),
        );
        let rejected_remaining = limit.saturating_sub(target_model.rejected_examples.len());
        target_model.rejected_examples.extend(
            source_model
                .rejected_examples
                .drain(..rejected_remaining.min(source_model.rejected_examples.len())),
        );
    }
}

fn audit_axis_samples(
    attribute_states: &[Vec<Attribute>],
    samples: &[Sample],
    axis: &Axis,
    limit: usize,
) -> AxisResult {
    type Key = (Identity, usize, usize, Vec<Attribute>, Vec<Attribute>);
    let family = axis.family.iter().copied().collect::<BTreeSet<_>>();
    let mut groups: HashMap<Key, BTreeMap<Vec<Attribute>, Bucket>> = HashMap::new();
    let mut counters = Counters::default();
    for sample in samples {
        if axis.required_property.is_some() && sample.packet.property != axis.required_property {
            continue;
        }
        let source = &attribute_states[sample.source_attribute_state_id];
        let target = &attribute_states[sample.target_attribute_state_id];
        let axis_state = target
            .iter()
            .filter(|row| family.contains(&row.attribute_id))
            .cloned()
            .collect::<Vec<_>>();
        if attribute_value(&axis_state, axis.current_id).is_none() {
            continue;
        }
        counters.samples_with_axis += 1;
        let key = (
            identity(sample),
            sample.source_status_state_id,
            sample.target_status_state_id,
            without(source, &[CURRENT_HP].into_iter().collect()),
            without(
                target,
                &family
                    .union(&[CURRENT_HP].into_iter().collect())
                    .copied()
                    .collect(),
            ),
        );
        let bucket = groups
            .entry(key)
            .or_default()
            .entry(axis_state)
            .or_default();
        bucket.sequences.insert(sample.sequence);
        *bucket
            .outcomes
            .entry(Outcome {
                amount: sample.amount,
                normal_value: sample.normal_value,
            })
            .or_default() += 1;
    }

    let mut models = axis
        .candidates
        .iter()
        .map(|model| {
            (
                model.name.to_owned(),
                ModelResult {
                    constant: model.constant,
                    counters: ModelCounters::default(),
                    exact_examples: Vec::new(),
                    rejected_examples: Vec::new(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();

    for ((identity, _, _, _, _), states) in groups {
        if states.len() < 2 {
            continue;
        }
        counters.controlled_groups += 1;
        let states = states.into_iter().collect::<Vec<_>>();
        for left_index in 0..states.len() {
            for right_index in left_index + 1..states.len() {
                let (left_state, left) = &states[left_index];
                let (right_state, right) = &states[right_index];
                let (Some(left_raw), Some(right_raw)) = (
                    attribute_value(left_state, axis.current_id),
                    attribute_value(right_state, axis.current_id),
                ) else {
                    continue;
                };
                if left_raw == right_raw {
                    continue;
                }
                counters.distinct_axis_pairs += 1;
                let left_outcomes = left.outcomes.keys().cloned().collect::<Vec<_>>();
                let right_outcomes = right.outcomes.keys().cloned().collect::<Vec<_>>();
                if left_outcomes.len() != 1 || right_outcomes.len() != 1 {
                    counters.nondeterministic_pairs += 1;
                    continue;
                }
                counters.deterministic_pairs += 1;
                if left_outcomes == right_outcomes {
                    counters.equal_output_pairs += 1;
                } else {
                    counters.divergent_output_pairs += 1;
                }
                for model in &axis.candidates {
                    let compatible = shared_base_interval(
                        left_outcomes[0].amount,
                        left_raw,
                        right_outcomes[0].amount,
                        right_raw,
                        model.constant,
                    );
                    let result = models.get_mut(model.name).expect("model exists");
                    let example = Example {
                        session_id: identity.session_id.clone(),
                        run_ordinal: identity.run_ordinal,
                        source_entity_uuid: identity.source_entity_uuid,
                        target_entity_uuid: identity.target_entity_uuid,
                        ability_id: identity.ability_id,
                        property: identity.property,
                        left_raw,
                        right_raw,
                        left_outcome: left_outcomes[0].clone(),
                        right_outcome: right_outcomes[0].clone(),
                        left_sequences: left.sequences.iter().copied().collect(),
                        right_sequences: right.sequences.iter().copied().collect(),
                        compatible_base_minimum: compatible.map(|v| v.0.to_string()),
                        compatible_base_maximum: compatible.map(|v| v.1.to_string()),
                    };
                    if compatible.is_some() {
                        result.counters.exact_pairs += 1;
                        if result.exact_examples.len() < limit {
                            result.exact_examples.push(example);
                        }
                    } else {
                        result.counters.rejected_pairs += 1;
                        if result.rejected_examples.len() < limit {
                            result.rejected_examples.push(example);
                        }
                    }
                }
            }
        }
    }
    AxisResult {
        current_attribute_id: axis.current_id,
        family_attribute_ids: axis.family.clone(),
        required_packet_property: axis.required_property,
        counters,
        models,
    }
}

fn identity(sample: &Sample) -> Identity {
    Identity {
        session_id: sample.session_id.clone(),
        run_ordinal: sample.run_ordinal,
        source_entity_uuid: sample.source_entity_uuid,
        direct_source_entity_uuid: sample.direct_source_entity_uuid,
        target_entity_uuid: sample.target_entity_uuid,
        ability_id: sample.ability_id,
        passive_uuid: sample.passive_uuid,
        hit_event_id: sample.hit_event_id,
        damage_source: sample.damage_source,
        damage_type: sample.damage_type,
        critical: sample.critical,
        lucky: sample.lucky,
        type_flags: sample.packet.type_flags,
        owner_level: sample.packet.owner_level,
        owner_stage: sample.packet.owner_stage,
        normal_hit: sample.packet.normal_hit,
        property: sample.packet.property,
        packet_passive_uuid: sample.packet.passive_uuid,
        rainbow: sample.packet.rainbow,
        damage_mode: sample.packet.damage_mode,
        skill_effect_uuid: sample.packet.skill_effect_uuid,
        skill_effect_group_index: sample.packet.skill_effect_group_index,
        skill_effect_component_index: sample.packet.skill_effect_component_index,
        skill_effect_component_count: sample.packet.skill_effect_component_count,
        raw_attacker_uuid: sample.packet.attacker_uuid,
        raw_top_summoner_uuid: sample.packet.top_summoner_uuid,
        raw_owner_id: sample.packet.owner_id,
    }
}

fn cross_capture_calculation_identity(sample: &Sample) -> Option<CrossCaptureCalculationIdentity> {
    let source_actor_identity = sample.source_actor_identity.clone()?;
    if source_actor_identity.character_id.is_none() && source_actor_identity.monster_id.is_none() {
        return None;
    }
    let target_actor_identity = sample.target_actor_identity.clone()?;
    Some(CrossCaptureCalculationIdentity {
        scene_id: sample.scene_id?,
        source_actor_identity,
        direct_source_actor_identity: sample.direct_source_actor_identity.clone(),
        target_actor_identity,
        ability_id: sample.ability_id,
        passive_uuid: sample.passive_uuid,
        hit_event_id: sample.hit_event_id,
        damage_source: sample.damage_source,
        damage_type: sample.damage_type,
        critical: sample.critical,
        lucky: sample.lucky,
        type_flags: sample.packet.type_flags,
        owner_level: sample.packet.owner_level,
        owner_stage: sample.packet.owner_stage,
        normal_hit: sample.packet.normal_hit,
        property: sample.packet.property,
        packet_passive_uuid: sample.packet.passive_uuid,
        rainbow: sample.packet.rainbow,
        damage_mode: sample.packet.damage_mode,
        skill_effect_uuid: sample.packet.skill_effect_uuid,
        skill_effect_group_index: sample.packet.skill_effect_group_index,
        skill_effect_component_index: sample.packet.skill_effect_component_index,
        skill_effect_component_count: sample.packet.skill_effect_component_count,
    })
}

fn attribute_value(attributes: &[Attribute], id: i32) -> Option<i64> {
    attributes
        .iter()
        .find(|row| row.attribute_id == id)
        .map(|row| row.value)
}

fn without(attributes: &[Attribute], excluded: &BTreeSet<i32>) -> Vec<Attribute> {
    attributes
        .iter()
        .filter(|row| !excluded.contains(&row.attribute_id))
        .cloned()
        .collect()
}

fn shared_base_interval(
    left_output: i64,
    left_raw: i64,
    right_output: i64,
    right_raw: i64,
    constant: i64,
) -> Option<(i128, i128)> {
    let left = base_preimage(left_output, left_raw, constant)?;
    let right = base_preimage(right_output, right_raw, constant)?;
    let minimum = left.0.max(right.0);
    let maximum = left.1.min(right.1);
    (minimum <= maximum).then_some((minimum, maximum))
}

fn base_preimage(output: i64, raw: i64, constant: i64) -> Option<(i128, i128)> {
    if output < 0 || raw < 0 || constant <= 0 {
        return None;
    }
    let denominator = i128::from(constant.checked_add(raw)?);
    let constant = i128::from(constant);
    let output = i128::from(output);
    let minimum = ceil_div(output.checked_mul(denominator)?, constant)?;
    let maximum =
        ceil_div(output.checked_add(1)?.checked_mul(denominator)?, constant)?.checked_sub(1)?;
    (minimum <= maximum).then_some((minimum, maximum))
}

fn ceil_div(numerator: i128, denominator: i128) -> Option<i128> {
    if numerator < 0 || denominator <= 0 {
        return None;
    }
    numerator
        .checked_add(denominator.checked_sub(1)?)?
        .checked_div(denominator)
}

fn parse_args() -> Result<Arguments, String> {
    let mut cohort = None;
    let mut output = None;
    let mut target_identity_worklist_output = None;
    let mut target_identity_proof = None;
    let mut target_status_relaxed_diagnostic_output = None;
    let mut cross_capture_target_config_diagnostic_output = None;
    let mut selected_ability_diagnostic_output = None;
    let mut selected_ability_ids = BTreeSet::new();
    let mut selected_hit_event_id = None;
    let mut selected_coefficient_basis_points = None;
    let mut diagnostic_effect_id = None;
    let mut example_limit = 8usize;
    let mut memory_limit_mib = 512usize;
    let mut args = env::args().skip(1);
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        match flag.as_str() {
            "--cohort" => cohort = Some(PathBuf::from(value)),
            "--output" => output = Some(PathBuf::from(value)),
            "--target-identity-worklist-output" => {
                target_identity_worklist_output = Some(PathBuf::from(value))
            }
            "--target-identity-proof" => target_identity_proof = Some(PathBuf::from(value)),
            "--target-status-relaxed-diagnostic-output" => {
                target_status_relaxed_diagnostic_output = Some(PathBuf::from(value))
            }
            "--cross-capture-target-config-diagnostic-output" => {
                cross_capture_target_config_diagnostic_output = Some(PathBuf::from(value))
            }
            "--selected-ability-diagnostic-output" => {
                selected_ability_diagnostic_output = Some(PathBuf::from(value))
            }
            "--selected-ability-ids" => {
                selected_ability_ids = parse_positive_i64_set(&value, "--selected-ability-ids")?
            }
            "--selected-hit-event-id" => {
                let parsed = value
                    .parse::<i32>()
                    .map_err(|_| "invalid --selected-hit-event-id".to_owned())?;
                if parsed <= 0 {
                    return Err("--selected-hit-event-id must be positive".to_owned());
                }
                selected_hit_event_id = Some(parsed)
            }
            "--selected-coefficient-basis-points" => {
                let parsed = value
                    .parse::<i64>()
                    .map_err(|_| "invalid --selected-coefficient-basis-points".to_owned())?;
                if parsed <= 0 {
                    return Err("--selected-coefficient-basis-points must be positive".to_owned());
                }
                selected_coefficient_basis_points = Some(parsed);
            }
            "--diagnostic-effect-id" => {
                let parsed = value
                    .parse::<i64>()
                    .map_err(|_| "invalid --diagnostic-effect-id".to_owned())?;
                if parsed <= 0 {
                    return Err("--diagnostic-effect-id must be positive".to_owned());
                }
                diagnostic_effect_id = Some(parsed);
            }
            "--example-limit" => {
                example_limit = value
                    .parse()
                    .map_err(|_| "invalid --example-limit".to_owned())?
            }
            "--memory-limit-mib" => {
                memory_limit_mib = value
                    .parse()
                    .map_err(|_| "invalid --memory-limit-mib".to_owned())?;
                if memory_limit_mib < 64 {
                    return Err("--memory-limit-mib must be at least 64".to_owned());
                }
            }
            _ => return Err(format!("unknown argument {flag}")),
        }
    }
    if target_status_relaxed_diagnostic_output.is_some() != diagnostic_effect_id.is_some() {
        return Err(
            "--target-status-relaxed-diagnostic-output and --diagnostic-effect-id must be supplied together"
                .to_owned(),
        );
    }
    if target_identity_proof.is_some() && cross_capture_target_config_diagnostic_output.is_none() {
        return Err(
            "--target-identity-proof requires --cross-capture-target-config-diagnostic-output"
                .to_owned(),
        );
    }
    let selected_argument_count = usize::from(selected_ability_diagnostic_output.is_some())
        + usize::from(!selected_ability_ids.is_empty())
        + usize::from(selected_hit_event_id.is_some())
        + usize::from(selected_coefficient_basis_points.is_some());
    if selected_argument_count != 0 && selected_argument_count != 4 {
        return Err(
            "--selected-ability-diagnostic-output, --selected-ability-ids, --selected-hit-event-id, and --selected-coefficient-basis-points must be supplied together"
                .to_owned(),
        );
    }
    Ok(Arguments {
        cohort: cohort.ok_or_else(|| "missing --cohort".to_owned())?,
        output: output.ok_or_else(|| "missing --output".to_owned())?,
        target_identity_worklist_output,
        target_identity_proof,
        target_status_relaxed_diagnostic_output,
        cross_capture_target_config_diagnostic_output,
        selected_ability_diagnostic_output,
        selected_ability_ids,
        selected_hit_event_id,
        selected_coefficient_basis_points,
        diagnostic_effect_id,
        example_limit,
        memory_limit_mib,
    })
}

fn parse_positive_i64_set(value: &str, flag: &str) -> Result<BTreeSet<i64>, String> {
    if value.is_empty() {
        return Err(format!("{flag} must not be empty"));
    }
    value
        .split(',')
        .map(|part| {
            let parsed = part
                .parse::<i64>()
                .map_err(|_| format!("invalid {flag} value {part}"))?;
            if parsed <= 0 {
                return Err(format!("{flag} values must be positive"));
            }
            Ok(parsed)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mitigation_identity_worklist_ignores_current_hp_only_states() {
        let exclusions = all_mitigation_attribute_ids();
        assert!(
            observed_mitigation_attribute_ids(
                &[Attribute {
                    attribute_id: CURRENT_HP,
                    value: 1_000,
                }],
                &exclusions,
            )
            .is_empty()
        );
        assert_eq!(
            observed_mitigation_attribute_ids(
                &[
                    Attribute {
                        attribute_id: CURRENT_HP,
                        value: 1_000,
                    },
                    Attribute {
                        attribute_id: 11_350,
                        value: 5_907,
                    },
                ],
                &exclusions,
            ),
            [11_350].into_iter().collect(),
        );
    }

    #[test]
    fn partition_key_preserves_exact_axis_groups_without_relaxing_other_mitigation_axes() {
        let attribute_states = vec![
            attributes(&[(10_000, 1), (CURRENT_HP, 1_000)]),
            attributes(&[(10_000, 1), (CURRENT_HP, 900)]),
            attributes(&[
                (CURRENT_HP, 10_000),
                (11_350, 100),
                (11_360, 50),
                (11_440, 7),
            ]),
            attributes(&[
                (CURRENT_HP, 9_000),
                (11_350, 200),
                (11_360, 50),
                (11_440, 7),
            ]),
            attributes(&[
                (CURRENT_HP, 8_000),
                (11_350, 300),
                (11_360, 999),
                (11_440, 7),
            ]),
        ];
        let samples = vec![
            sample(1, 1_000, 0, 2),
            sample(2, 900, 1, 3),
            sample(3, 800, 0, 4),
        ];
        let axis = axes()
            .into_iter()
            .find(|axis| axis.name == "physical_defense")
            .expect("physical axis");
        let full = audit_axis_samples(&attribute_states, &samples, &axis, 8);
        assert_eq!(full.counters.samples_with_axis, 3);
        assert_eq!(full.counters.controlled_groups, 1);
        assert_eq!(full.counters.distinct_axis_pairs, 1);
        assert_eq!(full.counters.deterministic_pairs, 1);
        assert_eq!(full.counters.divergent_output_pairs, 1);

        let exclusions = all_mitigation_attribute_ids();
        let partition_count = 4;
        let indexes = samples
            .iter()
            .map(|sample| partition_index(sample, &attribute_states, &exclusions, partition_count))
            .collect::<Vec<_>>();
        assert!(indexes.windows(2).all(|pair| pair[0] == pair[1]));

        let mut merged = empty_axis_result(&axis);
        for partition in 0..partition_count {
            let selected = samples
                .iter()
                .zip(&indexes)
                .filter(|(_, index)| **index == partition)
                .map(|(sample, _)| sample_fixture_clone(sample))
                .collect::<Vec<_>>();
            merge_axis_result(
                &mut merged,
                audit_axis_samples(&attribute_states, &selected, &axis, 8),
                8,
            );
        }
        assert_eq!(merged.counters, full.counters);
        for (name, full_model) in full.models {
            assert_eq!(merged.models[&name].counters, full_model.counters);
        }
    }

    #[test]
    fn model_preimage_contract_remains_integer_exact() {
        assert_eq!(base_preimage(50, 100, 100), Some((100, 101)));
        assert_eq!(
            shared_base_interval(50, 100, 40, 150, 100),
            Some((100, 101))
        );
        assert_eq!(shared_base_interval(50, 100, 41, 150, 100), None);
    }

    #[test]
    fn selected_action_factor_interval_inverts_the_integer_floor_exactly() {
        assert_eq!(
            integer_factor_interval(91_808, 12_702),
            Some((72_279, 72_279))
        );
        assert_eq!(
            integer_factor_interval(150_221, 16_144),
            Some((93_051, 93_051))
        );
        assert_eq!(integer_factor_interval(1, 0), None);
    }

    #[test]
    fn critical_stage_candidates_keep_floor_and_half_up_distinct() {
        assert_eq!(apply_fixed_point_stage(3, 5_000, false), Some(1));
        assert_eq!(apply_fixed_point_stage(3, 5_000, true), Some(2));
        assert_eq!(apply_fixed_point_stage(12_702, 15_000, false), Some(19_053));
        assert_eq!(apply_fixed_point_stage(-1, 15_000, false), None);
        assert_eq!(fixed_point_stage_preimage(4, 15_000, false), Some((3, 3)));
        assert_eq!(fixed_point_stage_preimage(5, 15_000, true), Some((3, 3)));
        assert_eq!(
            integer_factor_interval_for_output_range(3, 3, 3),
            Some((10_000, 13_333))
        );
    }

    #[test]
    fn selected_ability_filter_requires_exact_numeric_ability_and_hit_ids() {
        let selected = parse_positive_i64_set("2031102,2031105,2031111", "fixture")
            .expect("valid selected ability IDs");
        let mut candidate = sample(1, 1_000, 0, 0);
        candidate.ability_id = 2_031_105;
        candidate.hit_event_id = Some(3);
        assert!(sample_matches_selected_ability(&candidate, &selected, 3));
        candidate.hit_event_id = Some(2);
        assert!(!sample_matches_selected_ability(&candidate, &selected, 3));
        candidate.hit_event_id = Some(3);
        candidate.ability_id = 2_031_106;
        assert!(!sample_matches_selected_ability(&candidate, &selected, 3));
    }

    #[test]
    fn target_status_relaxed_diagnostic_preserves_the_exact_selected_effect_delta() {
        let attribute_states = vec![
            attributes(&[(10_000, 1), (CURRENT_HP, 1_000)]),
            attributes(&[(CURRENT_HP, 10_000), (11_350, 100), (11_360, 50)]),
            attributes(&[(CURRENT_HP, 9_000), (11_350, 200), (11_360, 50)]),
        ];
        let mut left = sample(1, 1_000, 0, 1);
        left.target_status_state_id = 0;
        let mut right = sample(2, 900, 0, 2);
        right.target_status_state_id = 1;
        let samples = vec![left, right];
        let status_states = vec![Vec::new(), vec![status(2_110_092)]];
        let axis = axes()
            .into_iter()
            .find(|axis| axis.name == "physical_defense")
            .expect("physical axis");
        let result = audit_target_status_relaxed_axis_samples(
            &attribute_states,
            &status_states,
            &samples,
            &axis,
            2_110_092,
            8,
        );
        assert_eq!(result.counters.distinct_axis_pairs, 1);
        assert_eq!(result.counters.deterministic_pairs, 1);
        assert_eq!(result.counters.divergent_output_pairs, 1);
        assert_eq!(
            result.counters.pairs_with_selected_effect_in_status_delta,
            1
        );
        assert_eq!(
            result
                .counters
                .pairs_with_only_selected_effect_in_status_delta,
            1
        );
        assert_eq!(result.selected_effect_examples.len(), 1);
        assert_eq!(result.near_pair_examples.len(), 1);
        assert!(result.selected_effect_examples[0].selected_effect_is_the_only_status_delta);

        let exclusions = all_mitigation_attribute_ids();
        let indexes = samples
            .iter()
            .map(|sample| {
                target_status_relaxed_partition_index(sample, &attribute_states, &exclusions, 16)
            })
            .collect::<Vec<_>>();
        assert_eq!(indexes[0], indexes[1]);
    }

    #[test]
    fn target_status_diagnostic_preserves_same_axis_equal_outcome_invariance_witness() {
        let attribute_states = vec![
            attributes(&[(10_000, 1), (CURRENT_HP, 1_000)]),
            attributes(&[(CURRENT_HP, 10_000), (11_350, 100), (11_360, 50)]),
        ];
        let mut left = sample(1, 1_000, 0, 1);
        left.target_status_state_id = 0;
        let mut right = sample(2, 1_000, 0, 1);
        right.target_status_state_id = 1;
        let status_states = vec![Vec::new(), vec![status(2_110_092)]];
        let axis = axes()
            .into_iter()
            .find(|axis| axis.name == "physical_defense")
            .expect("physical axis");
        let result = audit_target_status_relaxed_axis_samples(
            &attribute_states,
            &status_states,
            &[left, right],
            &axis,
            2_110_092,
            8,
        );
        assert_eq!(result.counters.same_axis_status_pairs, 1);
        assert_eq!(result.counters.same_axis_deterministic_pairs, 1);
        assert_eq!(result.counters.same_axis_equal_output_pairs, 1);
        assert_eq!(result.counters.same_axis_divergent_output_pairs, 0);
        assert_eq!(
            result
                .counters
                .same_axis_pairs_with_only_selected_effect_in_status_delta,
            1
        );
        assert_eq!(result.same_axis_status_examples.len(), 1);
        assert_eq!(result.selected_effect_same_axis_examples.len(), 1);
        assert_eq!(result.counters.distinct_axis_pairs, 0);
    }

    #[test]
    fn cross_capture_target_config_diagnostic_requires_a_cross_session_witness() {
        let attribute_states = vec![
            attributes(&[(10_000, 1), (CURRENT_HP, 1_000)]),
            attributes(&[(10, 80_017), (CURRENT_HP, 10_000), (11_350, 5_907)]),
            attributes(&[(10, 80_017), (CURRENT_HP, 9_000), (11_350, 5_370)]),
        ];
        let mut left = sample(1, 84_356, 0, 1);
        left.session_id = "left-capture".to_owned();
        left.scene_id = Some(12023);
        left.source_actor_identity = Some(player_actor("character-1"));
        left.target_actor_identity = Some(monster_actor(80_017));
        let mut right = sample(2, 86_011, 0, 2);
        right.session_id = "right-capture".to_owned();
        right.scene_id = Some(12023);
        right.source_actor_identity = Some(player_actor("character-1"));
        right.target_actor_identity = Some(monster_actor(80_017));
        let samples = vec![left, right];
        let axis = axes()
            .into_iter()
            .find(|axis| axis.name == "physical_defense")
            .expect("physical axis");
        let result =
            audit_cross_capture_target_config_axis_samples(&attribute_states, &samples, &axis, 8);
        assert_eq!(
            result
                .counters
                .samples_with_packet_observed_target_actor_identity,
            2
        );
        assert_eq!(result.counters.pairs_with_cross_capture_witness, 1);
        assert_eq!(result.counters.deterministic_cross_capture_pairs, 1);
        assert_eq!(result.counters.divergent_output_cross_capture_pairs, 1);
        assert_eq!(result.models["transformed_curve"].counters.exact_pairs, 1);
        assert_eq!(
            result.models["runtime_simple_curve"]
                .counters
                .rejected_pairs,
            1
        );

        let exclusions = all_mitigation_attribute_ids();
        let indexes = samples
            .iter()
            .map(|sample| {
                cross_capture_target_config_partition_index(
                    sample,
                    &attribute_states,
                    &exclusions,
                    16,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(indexes[0], indexes[1]);

        let mut same_capture = sample_fixture_clone(&samples[1]);
        same_capture.session_id = samples[0].session_id.clone();
        let same_capture_result = audit_cross_capture_target_config_axis_samples(
            &attribute_states,
            &[sample_fixture_clone(&samples[0]), same_capture],
            &axis,
            8,
        );
        assert_eq!(
            same_capture_result
                .counters
                .pairs_with_cross_capture_witness,
            0
        );
    }

    fn attributes(values: &[(i32, i64)]) -> Vec<Attribute> {
        values
            .iter()
            .map(|(attribute_id, value)| Attribute {
                attribute_id: *attribute_id,
                value: *value,
            })
            .collect()
    }

    fn status(effect_id: i64) -> Status {
        Status {
            effect_id,
            source_entity_uuid: 10,
            stacks: 1,
            level: 1,
            origin_source_type_id: None,
            origin_source_config_id: None,
        }
    }

    fn sample(sequence: u64, amount: i64, source_state: usize, target_state: usize) -> Sample {
        Sample {
            session_id: "fixture".to_owned(),
            run_ordinal: 1,
            sequence,
            scene_id: None,
            source_entity_uuid: 10,
            direct_source_entity_uuid: None,
            target_entity_uuid: 20,
            source_actor_identity: None,
            direct_source_actor_identity: None,
            target_actor_identity: None,
            ability_id: 30,
            passive_uuid: None,
            hit_event_id: Some(1),
            amount,
            normal_value: Some(amount),
            damage_source: Some(1),
            damage_type: Some(1),
            critical: Some(false),
            lucky: Some(false),
            packet: Packet {
                owner_level: Some(1),
                normal_hit: Some(true),
                property: Some(1),
                ..Packet::default()
            },
            source_attribute_state_id: source_state,
            target_attribute_state_id: target_state,
            source_status_state_id: 0,
            target_status_state_id: 0,
        }
    }

    fn sample_fixture_clone(sample: &Sample) -> Sample {
        serde_json::from_value(serde_json::to_value(sample).expect("serialize sample"))
            .expect("deserialize sample")
    }

    fn player_actor(character_id: &str) -> ActorIdentity {
        ActorIdentity {
            entity_type_id: 1,
            monster_id: None,
            character_id: Some(character_id.to_owned()),
            class_id: Some(14),
            specialization_id: Some(1),
            level: Some(60),
        }
    }

    fn monster_actor(monster_id: i64) -> ActorIdentity {
        ActorIdentity {
            entity_type_id: 2,
            monster_id: Some(monster_id),
            character_id: None,
            class_id: None,
            specialization_id: None,
            level: Some(100),
        }
    }
}
