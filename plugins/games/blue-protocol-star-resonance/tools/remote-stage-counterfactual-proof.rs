use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, hash_map::DefaultHasher},
    env,
    fs::{self, File},
    hash::{Hash, Hasher},
    io::{BufRead, BufReader, BufWriter, Write},
    path::PathBuf,
    sync::OnceLock,
};

use rlogs_events::DamagePacketDetail;
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor},
};

const COHORT_SCHEMAS: [u16; 9] = [39, 40, 41, 42, 43, 44, 45, 46, 47];
const CURRENT_HP_ATTRIBUTE_ID: i32 = 11_310;
const PARTITIONS: usize = 64;
const ATTACK_SEARCH_RADIUS: i64 = 8_192;
const MAX_ATTACK: i64 = 500_000;
const DEFAULT_EFFECT_ID: i64 = 3_003_052;

fn target_effect_id() -> i64 {
    static EFFECT_ID: OnceLock<i64> = OnceLock::new();
    *EFFECT_ID.get_or_init(|| {
        env::var("RLOGS_RDPS_EFFECT_ID")
            .ok()
            .and_then(|value| value.parse::<i64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_EFFECT_ID)
    })
}

fn ignored_source_effect_ids() -> &'static BTreeSet<i64> {
    static EFFECT_IDS: OnceLock<BTreeSet<i64>> = OnceLock::new();
    EFFECT_IDS.get_or_init(|| {
        env::var("RLOGS_RDPS_IGNORED_SOURCE_EFFECT_IDS")
            .ok()
            .into_iter()
            .flat_map(|value| {
                value
                    .split(',')
                    .filter_map(|item| item.trim().parse::<i64>().ok())
                    .filter(|effect_id| *effect_id > 0)
                    .collect::<Vec<_>>()
            })
            .collect()
    })
}

#[derive(Debug)]
struct Arguments {
    cohort: PathBuf,
    identity_cohort: Option<PathBuf>,
    catalog: PathBuf,
    attack_effect_ledger: PathBuf,
    output: PathBuf,
    work_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ActorLookupKey {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Attribute {
    attribute_id: i32,
    value: i64,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Status {
    effect_id: i64,
    source_entity_uuid: Option<i64>,
    stacks: Option<u32>,
    level: Option<i32>,
    origin_source_type_id: Option<i32>,
    origin_source_config_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
struct ActorIdentity {
    class_id: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct Sample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    sequence: u64,
    observed_micros: u64,
    wire_capture_sequence: Option<u64>,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    source_actor_identity: Option<ActorIdentity>,
    ability_id: i64,
    hit_event_id: Option<i32>,
    normal_value: Option<i64>,
    critical: Option<bool>,
    lucky: Option<bool>,
    damage_source: Option<i32>,
    packet: DamagePacketDetail,
    source_attribute_state_id: u32,
    target_attribute_state_id: u32,
    source_status_state_id: u32,
    target_status_state_id: u32,
}

#[derive(Debug, Deserialize)]
struct Catalog {
    rules: Vec<StageRule>,
}

#[derive(Debug, Deserialize)]
struct AttackEffectLedger {
    #[serde(alias = "obligations", alias = "formula_replay_candidates")]
    candidates: Vec<AttackEffectCandidate>,
}

#[derive(Debug, Deserialize)]
struct AttackEffectCandidate {
    #[serde(default)]
    formula_term_ids: Vec<String>,
    #[serde(default)]
    effect_ids: Vec<i64>,
}

#[derive(Debug, Clone, Deserialize)]
struct StageRule {
    ability_id: i64,
    hit_event_id: i32,
    #[serde(default)]
    damage_source: Option<i32>,
    damage_script: String,
    coefficient_basis_points_by_stage: Vec<i64>,
    fixed_parameter_by_level: Vec<i64>,
}

impl StageRule {
    fn select(&self, packet: &DamagePacketDetail) -> Option<(i64, i64, bool)> {
        if self.damage_script != "Attack" && self.damage_script != "MAttack" {
            return None;
        }
        let stage = usize::try_from(packet.owner_stage.unwrap_or_default()).ok()?;
        let coefficient = if self.coefficient_basis_points_by_stage.len() == 1 {
            self.coefficient_basis_points_by_stage[0]
        } else {
            *self.coefficient_basis_points_by_stage.get(stage)?
        };
        let fixed = if self.fixed_parameter_by_level.is_empty() {
            0
        } else {
            let level = usize::try_from(packet.owner_level?).ok()?.checked_sub(1)?;
            *self.fixed_parameter_by_level.get(level)?
        };
        (coefficient > 0).then_some((coefficient, fixed, self.damage_script == "MAttack"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CompactSample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    sequence: u64,
    observed_micros: u64,
    wire_capture_sequence: Option<u64>,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    class_id: i32,
    ability_id: i64,
    magical_attack: bool,
    hit_event_id: i32,
    critical: Option<bool>,
    lucky: Option<bool>,
    property: Option<i32>,
    damage_mode: Option<i32>,
    source_state_hash: u64,
    source_damage_state_hash: u64,
    target_state_hash: u64,
    source_status_hash: u64,
    source_attack_status_hash: u64,
    source_formula_status_hash: u64,
    target_status_hash: u64,
    harmony_active: bool,
    harmony_provider: Option<i64>,
    coefficient: i64,
    fixed: i64,
    output: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct WireKey {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    class_id: i32,
    ability_id: i64,
    magical_attack: bool,
    critical: Option<bool>,
    lucky: Option<bool>,
    property: Option<i32>,
    damage_mode: Option<i32>,
    source_state_hash: u64,
    target_state_hash: u64,
    source_status_hash: u64,
    target_status_hash: u64,
    harmony_active: bool,
    harmony_provider: Option<i64>,
}

#[derive(Debug, Clone, Copy)]
struct AttackFactorCandidate {
    attack: i64,
    factor_min: i64,
    factor_max: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PairKey {
    rlog: String,
    session_id: String,
    source_entity_uuid: i64,
    class_id: i32,
    ability_id: i64,
    magical_attack: bool,
    critical: Option<bool>,
    lucky: Option<bool>,
    property: Option<i32>,
    damage_mode: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct DirectPairKey {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    class_id: i32,
    ability_id: i64,
    magical_attack: bool,
    hit_event_id: i32,
    critical: Option<bool>,
    lucky: Option<bool>,
    property: Option<i32>,
    damage_mode: Option<i32>,
    source_damage_state_hash: u64,
    source_status_hash: u64,
    source_attack_status_hash: u64,
    source_formula_status_hash: u64,
    target_state_hash: u64,
    target_status_hash: u64,
    coefficient: i64,
    fixed: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DirectTransitionKey {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    class_id: i32,
    ability_id: i64,
    hit_event_id: i32,
    source_entity_uuid: i64,
    provider_entity_uuid: Option<i64>,
    inactive_observed_micros: u64,
    active_observed_micros: u64,
    source_damage_state_hash: u64,
    source_attack_status_hash: u64,
    source_formula_status_hash: u64,
    inactive_source_state_hash: u64,
    active_source_state_hash: u64,
    inactive_source_status_hash: u64,
    active_source_status_hash: u64,
    coefficient: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ActorStateKey {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    class_id: i32,
    magical_attack: bool,
    source_status_hash: u64,
    source_attack_status_hash: u64,
    harmony_active: bool,
    harmony_provider: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ActorStatePairKey {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    class_id: i32,
    magical_attack: bool,
    source_attack_status_hash: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CoarseActorStateKey {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    class_id: i32,
    magical_attack: bool,
    harmony_active: bool,
    harmony_provider: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CoarseActorStatePairKey {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    class_id: i32,
    magical_attack: bool,
}

#[derive(Debug, Clone)]
struct ActorStateSolution {
    key: ActorStateKey,
    attack: i64,
    supporting_groups: usize,
    status_hashes: Vec<u64>,
    attribute_hashes: Vec<u64>,
    ability_ids: Vec<i64>,
    target_entity_uuids: Vec<i64>,
    observed_micros: u64,
}

#[derive(Debug, Clone)]
struct CoarseActorStateSolution {
    key: CoarseActorStateKey,
    attack: i64,
    supporting_groups: usize,
    status_variants: usize,
    attribute_variants: usize,
    ability_ids: Vec<i64>,
    target_entity_uuids: Vec<i64>,
    status_hashes: Vec<u64>,
    attribute_hashes: Vec<u64>,
    observed_micros: u64,
}

#[derive(Debug, Serialize)]
struct PairExample {
    evidence_kind: &'static str,
    class_id: i32,
    ability_id: i64,
    source_entity_uuid: i64,
    provider_entity_uuid: Option<i64>,
    inactive_attack: i64,
    active_attack: i64,
    attack_delta: i64,
    shared_factor_min: i64,
    shared_factor_max: i64,
    inactive_observed_micros: u64,
    active_observed_micros: u64,
}

#[derive(Debug, Serialize)]
struct DirectPairExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    class_id: i32,
    ability_id: i64,
    hit_event_id: i32,
    source_entity_uuid: i64,
    target_entity_uuid: i64,
    provider_entity_uuid: Option<i64>,
    inactive_output: i64,
    active_output: i64,
    output_delta: i64,
    source_damage_state_hash: u64,
    source_attack_status_hash: u64,
    source_formula_status_hash: u64,
    target_state_hash: u64,
    target_status_hash: u64,
    inactive_source_state_hash: u64,
    active_source_state_hash: u64,
    inactive_source_status_hash: u64,
    active_source_status_hash: u64,
    inactive_source_attributes: Vec<Attribute>,
    active_source_attributes: Vec<Attribute>,
    inactive_source_statuses: Vec<Status>,
    active_source_statuses: Vec<Status>,
    inactive_observed_micros: u64,
    active_observed_micros: u64,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct RatioBound {
    numerator: i64,
    denominator: i64,
}

#[derive(Debug, Serialize)]
struct ProportionalAnchorExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    class_id: i32,
    ability_id: i64,
    hit_event_id: i32,
    source_entity_uuid: i64,
    provider_entity_uuid: Option<i64>,
    inactive_observed_micros: u64,
    active_observed_micros: u64,
    observed_rows: usize,
    distinct_targets: usize,
    distinct_output_pairs: usize,
    observed_active_damage: i64,
    observed_inactive_damage: i64,
    observed_exact_delta: i64,
    full_source_attributes_equal: bool,
    full_non_harmony_source_statuses_equal: bool,
    inactive_source_state_hash: u64,
    active_source_state_hash: u64,
    inactive_source_status_hash: u64,
    active_source_status_hash: u64,
    inactive_source_statuses: Vec<Status>,
    active_source_statuses: Vec<Status>,
    counterfactual_ratio_lower: RatioBound,
    counterfactual_ratio_upper: RatioBound,
    provider_share_lower: RatioBound,
    provider_share_upper: RatioBound,
}

#[derive(Debug, Serialize)]
struct CoarsePairExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    class_id: i32,
    magical_attack: bool,
    source_entity_uuid: i64,
    provider_entity_uuid: Option<i64>,
    inactive_attack: i64,
    active_attack: i64,
    attack_delta: i64,
    inactive_supporting_groups: usize,
    active_supporting_groups: usize,
    inactive_status_variants: usize,
    active_status_variants: usize,
    inactive_attribute_variants: usize,
    active_attribute_variants: usize,
    inactive_ability_ids: Vec<i64>,
    active_ability_ids: Vec<i64>,
    inactive_target_entity_uuids: Vec<i64>,
    active_target_entity_uuids: Vec<i64>,
    inactive_status_hashes: Vec<u64>,
    active_status_hashes: Vec<u64>,
    inactive_attribute_hashes: Vec<u64>,
    active_attribute_hashes: Vec<u64>,
    inactive_observed_micros: u64,
    active_observed_micros: u64,
}

#[derive(Debug, Serialize)]
struct CrossStatusPairExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    class_id: i32,
    magical_attack: bool,
    source_entity_uuid: i64,
    provider_entity_uuid: Option<i64>,
    inactive_attack: i64,
    active_attack: i64,
    attack_delta: i64,
    inactive_supporting_groups: usize,
    active_supporting_groups: usize,
    inactive_attack_status_hash: u64,
    active_attack_status_hash: u64,
    inactive_status_hashes: Vec<u64>,
    active_status_hashes: Vec<u64>,
    inactive_statuses: Vec<Status>,
    active_statuses: Vec<Status>,
    inactive_attribute_hashes: Vec<u64>,
    active_attribute_hashes: Vec<u64>,
    inactive_attribute_states: Vec<Vec<Attribute>>,
    active_attribute_states: Vec<Vec<Attribute>>,
    inactive_ability_ids: Vec<i64>,
    active_ability_ids: Vec<i64>,
    inactive_target_entity_uuids: Vec<i64>,
    active_target_entity_uuids: Vec<i64>,
    inactive_observed_micros: u64,
    active_observed_micros: u64,
}

#[derive(Debug, Serialize)]
struct Report {
    schema_version: u16,
    generated_by: &'static str,
    effect_id: i64,
    ignored_source_effect_ids: Vec<i64>,
    attack_affecting_effect_ids: Vec<i64>,
    formula_affecting_effect_ids: Vec<i64>,
    policy: BTreeMap<&'static str, &'static str>,
    identity_actor_mappings: usize,
    identity_actor_conflicts_rejected: usize,
    scanned_samples: u64,
    selected_stage_samples: u64,
    selected_samples_by_class_ability: BTreeMap<String, u64>,
    multi_stage_wire_groups: u64,
    multi_stage_groups_by_class_ability: BTreeMap<String, u64>,
    candidate_solved_wire_groups: u64,
    candidate_solved_groups_by_class_ability: BTreeMap<String, u64>,
    active_solved_wire_groups: u64,
    inactive_solved_wire_groups: u64,
    active_solved_groups_by_class_ability: BTreeMap<String, u64>,
    inactive_solved_groups_by_class_ability: BTreeMap<String, u64>,
    exact_active_inactive_pairs: u64,
    ambiguous_active_inactive_pairs: u64,
    positive_delta_pairs: u64,
    delta_histogram_by_class: BTreeMap<i32, BTreeMap<i64, u64>>,
    uniquely_solved_actor_states: u64,
    exact_actor_state_active_inactive_pairs: u64,
    positive_actor_state_delta_pairs: u64,
    actor_state_delta_histogram_by_class: BTreeMap<i32, BTreeMap<i64, u64>>,
    cross_status_exact_actor_state_pairs: u64,
    positive_cross_status_exact_actor_state_pairs: u64,
    cross_status_exact_actor_delta_histogram_by_class: BTreeMap<i32, BTreeMap<i64, u64>>,
    cross_status_exact_examples: Vec<CrossStatusPairExample>,
    consensus_solved_actor_states: u64,
    consensus_actor_state_active_inactive_pairs: u64,
    positive_consensus_actor_state_delta_pairs: u64,
    consensus_actor_state_delta_histogram_by_class: BTreeMap<i32, BTreeMap<i64, u64>>,
    coarse_consensus_solved_actor_states: u64,
    coarse_consensus_actor_state_active_inactive_pairs: u64,
    positive_coarse_consensus_actor_state_delta_pairs: u64,
    coarse_consensus_actor_state_delta_histogram_by_class: BTreeMap<i32, BTreeMap<i64, u64>>,
    coarse_consensus_examples: Vec<CoarsePairExample>,
    direct_formula_context_pairs: u64,
    positive_direct_formula_context_pairs: u64,
    direct_formula_context_delta_histogram_by_class: BTreeMap<i32, BTreeMap<i64, u64>>,
    direct_formula_context_examples: Vec<DirectPairExample>,
    proportional_zero_fixed_anchors: Vec<ProportionalAnchorExample>,
    examples: Vec<PairExample>,
    formula_authority: bool,
    runtime_authority: bool,
    provider_rdps_credit_allowed: bool,
}

struct CohortSeed<'a> {
    rules: &'a HashMap<(i64, i32, Option<i32>), StageRule>,
    rules_by_ability: &'a HashMap<i64, Vec<StageRule>>,
    actor_classes: &'a HashMap<ActorLookupKey, i32>,
    attack_effect_ids: &'a HashSet<i64>,
    formula_effect_ids: &'a HashSet<i64>,
    source_attribute_states: &'a mut HashMap<u64, Vec<Attribute>>,
    source_status_states: &'a mut HashMap<u64, Vec<Status>>,
    writers: &'a mut [BufWriter<File>],
    scanned: &'a mut u64,
    selected: &'a mut u64,
    selected_by_class_ability: &'a mut BTreeMap<String, u64>,
}

impl<'de> DeserializeSeed<'de> for CohortSeed<'_> {
    type Value = ();
    fn deserialize<D>(self, deserializer: D) -> Result<(), D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(CohortVisitor { seed: self })
    }
}

struct CohortVisitor<'a> {
    seed: CohortSeed<'a>,
}

impl<'de> Visitor<'de> for CohortVisitor<'_> {
    type Value = ();
    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("formula cohort")
    }
    fn visit_map<A>(mut self, mut map: A) -> Result<(), A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut schema = None;
        let mut attributes: Option<Vec<Vec<Attribute>>> = None;
        let mut statuses: Option<Vec<Vec<Status>>> = None;
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "schema_version" => schema = Some(map.next_value::<u16>()?),
                "attribute_states" => attributes = Some(map.next_value()?),
                "status_states" => statuses = Some(map.next_value()?),
                "samples" => {
                    if !COHORT_SCHEMAS.contains(&schema.unwrap_or_default()) {
                        return Err(serde::de::Error::custom("unsupported cohort schema"));
                    }
                    let attributes = attributes
                        .as_ref()
                        .ok_or_else(|| serde::de::Error::missing_field("attribute_states"))?;
                    let statuses = statuses
                        .as_ref()
                        .ok_or_else(|| serde::de::Error::missing_field("status_states"))?;
                    map.next_value_seed(SamplesSeed {
                        seed: &mut self.seed,
                        attributes,
                        statuses,
                    })?;
                }
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(())
    }
}

struct SamplesSeed<'a, 'b> {
    seed: &'a mut CohortSeed<'b>,
    attributes: &'a [Vec<Attribute>],
    statuses: &'a [Vec<Status>],
}

struct IdentityCohortSeed<'a> {
    actor_classes: &'a mut HashMap<ActorLookupKey, i32>,
    conflicts: &'a mut HashSet<ActorLookupKey>,
}

impl<'de> DeserializeSeed<'de> for IdentityCohortSeed<'_> {
    type Value = ();
    fn deserialize<D>(self, deserializer: D) -> Result<(), D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(IdentityCohortVisitor { seed: self })
    }
}

struct IdentityCohortVisitor<'a> {
    seed: IdentityCohortSeed<'a>,
}

impl<'de> Visitor<'de> for IdentityCohortVisitor<'_> {
    type Value = ();
    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("formula cohort identity source")
    }
    fn visit_map<A>(self, mut map: A) -> Result<(), A::Error>
    where
        A: MapAccess<'de>,
    {
        while let Some(key) = map.next_key::<String>()? {
            if key == "samples" {
                map.next_value_seed(IdentitySamplesSeed {
                    actor_classes: &mut *self.seed.actor_classes,
                    conflicts: &mut *self.seed.conflicts,
                })?;
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }
        Ok(())
    }
}

struct IdentitySamplesSeed<'a> {
    actor_classes: &'a mut HashMap<ActorLookupKey, i32>,
    conflicts: &'a mut HashSet<ActorLookupKey>,
}

impl<'de> DeserializeSeed<'de> for IdentitySamplesSeed<'_> {
    type Value = ();
    fn deserialize<D>(self, deserializer: D) -> Result<(), D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for IdentitySamplesSeed<'_> {
    type Value = ();
    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("damage samples with actor identities")
    }
    fn visit_seq<A>(self, mut seq: A) -> Result<(), A::Error>
    where
        A: SeqAccess<'de>,
    {
        while let Some(sample) = seq.next_element::<Sample>()? {
            let Some(class_id) = sample
                .source_actor_identity
                .as_ref()
                .and_then(|identity| identity.class_id)
            else {
                continue;
            };
            let key = ActorLookupKey {
                rlog: sample.rlog,
                session_id: sample.session_id,
                run_ordinal: sample.run_ordinal,
                source_entity_uuid: sample.source_entity_uuid,
            };
            if self.conflicts.contains(&key) {
                continue;
            }
            match self.actor_classes.get(&key) {
                Some(existing) if *existing != class_id => {
                    self.actor_classes.remove(&key);
                    self.conflicts.insert(key);
                }
                Some(_) => {}
                None => {
                    self.actor_classes.insert(key, class_id);
                }
            }
        }
        Ok(())
    }
}

impl<'de> DeserializeSeed<'de> for SamplesSeed<'_, '_> {
    type Value = ();
    fn deserialize<D>(self, deserializer: D) -> Result<(), D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for SamplesSeed<'_, '_> {
    type Value = ();
    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("damage samples")
    }
    fn visit_seq<A>(self, mut seq: A) -> Result<(), A::Error>
    where
        A: SeqAccess<'de>,
    {
        while let Some(sample) = seq.next_element::<Sample>()? {
            *self.seed.scanned += 1;
            let class_id = sample
                .source_actor_identity
                .as_ref()
                .and_then(|v| v.class_id)
                .or_else(|| {
                    self.seed
                        .actor_classes
                        .get(&ActorLookupKey {
                            rlog: sample.rlog.clone(),
                            session_id: sample.session_id.clone(),
                            run_ordinal: sample.run_ordinal,
                            source_entity_uuid: sample.source_entity_uuid,
                        })
                        .copied()
                });
            let Some(class_id) = class_id else {
                continue;
            };
            let selected = if let Some(hit) = sample.hit_event_id {
                self.seed
                    .rules
                    .get(&(sample.ability_id, hit, sample.damage_source))
                    .or_else(|| self.seed.rules.get(&(sample.ability_id, hit, None)))
                    .and_then(|rule| rule.select(&sample.packet))
                    .map(|stage| (hit, stage))
            } else {
                let stages = self
                    .seed
                    .rules_by_ability
                    .get(&sample.ability_id)
                    .into_iter()
                    .flatten()
                    .filter(|rule| {
                        rule.damage_source.is_none() || rule.damage_source == sample.damage_source
                    })
                    .filter_map(|rule| rule.select(&sample.packet))
                    .collect::<BTreeSet<_>>();
                (stages.len() == 1).then(|| (0, *stages.first().unwrap()))
            };
            let Some((hit, (coefficient, fixed, magical_attack))) = selected else {
                continue;
            };
            let Some(output) = sample.normal_value.filter(|value| *value > 0) else {
                continue;
            };
            let source_status = self
                .statuses
                .get(sample.source_status_state_id as usize)
                .ok_or_else(|| serde::de::Error::custom("missing source status state"))?;
            let harmony = source_status
                .iter()
                .find(|status| status.effect_id == target_effect_id());
            let source_state_hash =
                state_hash(self.attributes, sample.source_attribute_state_id, true)?;
            let source_damage_state_hash =
                damage_state_hash(self.attributes, sample.source_attribute_state_id)?;
            self.seed
                .source_attribute_states
                .entry(source_state_hash)
                .or_insert_with(|| {
                    self.attributes[sample.source_attribute_state_id as usize]
                        .iter()
                        .filter(|value| value.attribute_id != CURRENT_HP_ATTRIBUTE_ID)
                        .copied()
                        .collect()
                });
            let source_status_hash = source_status_hash(source_status, target_effect_id());
            let source_attack_status_hash = attack_status_hash(
                source_status,
                self.seed.attack_effect_ids,
                target_effect_id(),
            );
            let source_formula_status_hash = attack_status_hash(
                source_status,
                self.seed.formula_effect_ids,
                target_effect_id(),
            );
            self.seed
                .source_status_states
                .entry(source_status_hash)
                .or_insert_with(|| {
                    source_status
                        .iter()
                        .filter(|value| {
                            value.effect_id != target_effect_id()
                                && !ignored_source_effect_ids().contains(&value.effect_id)
                        })
                        .copied()
                        .collect()
                });
            let compact = CompactSample {
                rlog: sample.rlog,
                session_id: sample.session_id,
                run_ordinal: sample.run_ordinal,
                sequence: sample.sequence,
                observed_micros: sample.observed_micros,
                wire_capture_sequence: sample.wire_capture_sequence,
                source_entity_uuid: sample.source_entity_uuid,
                target_entity_uuid: sample.target_entity_uuid,
                class_id,
                ability_id: sample.ability_id,
                magical_attack,
                hit_event_id: hit,
                critical: sample.critical,
                lucky: sample.lucky,
                property: sample.packet.property,
                damage_mode: sample.packet.damage_mode,
                source_state_hash,
                source_damage_state_hash,
                target_state_hash: state_hash(
                    self.attributes,
                    sample.target_attribute_state_id,
                    true,
                )?,
                source_status_hash,
                source_attack_status_hash,
                source_formula_status_hash,
                target_status_hash: status_hash(
                    self.statuses
                        .get(sample.target_status_state_id as usize)
                        .ok_or_else(|| serde::de::Error::custom("missing target status state"))?,
                    target_effect_id(),
                ),
                harmony_active: harmony.is_some(),
                harmony_provider: harmony.and_then(|status| status.source_entity_uuid),
                coefficient,
                fixed,
                output,
            };
            let index = partition(&compact, self.seed.writers.len());
            serde_json::to_writer(&mut self.seed.writers[index], &compact)
                .map_err(serde::de::Error::custom)?;
            self.seed.writers[index]
                .write_all(b"\n")
                .map_err(serde::de::Error::custom)?;
            *self.seed.selected += 1;
            *self
                .seed
                .selected_by_class_ability
                .entry(format!("{class_id}:{}", sample.ability_id))
                .or_default() += 1;
        }
        Ok(())
    }
}

fn state_hash<E: serde::de::Error>(
    states: &[Vec<Attribute>],
    id: u32,
    exclude_hp: bool,
) -> Result<u64, E> {
    let state = states
        .get(id as usize)
        .ok_or_else(|| E::custom("missing attribute state"))?;
    let mut hasher = DefaultHasher::new();
    for value in state
        .iter()
        .filter(|value| !exclude_hp || value.attribute_id != CURRENT_HP_ATTRIBUTE_ID)
    {
        value.hash(&mut hasher);
    }
    Ok(hasher.finish())
}

fn damage_state_hash<E: serde::de::Error>(states: &[Vec<Attribute>], id: u32) -> Result<u64, E> {
    let state = states
        .get(id as usize)
        .ok_or_else(|| E::custom("missing attribute state"))?;
    let mut hasher = DefaultHasher::new();
    for value in state.iter().filter(|value| {
        !matches!(
            value.attribute_id,
            11_310..=11_325 | 11_440 | 11_450 | 11_720 | 11_730
        )
    }) {
        value.hash(&mut hasher);
    }
    Ok(hasher.finish())
}

fn status_hash(state: &[Status], excluded_effect: i64) -> u64 {
    let mut hasher = DefaultHasher::new();
    for value in state
        .iter()
        .filter(|value| value.effect_id != excluded_effect)
    {
        value.hash(&mut hasher);
    }
    hasher.finish()
}

fn source_status_hash(state: &[Status], excluded_effect: i64) -> u64 {
    let ignored = ignored_source_effect_ids();
    let mut hasher = DefaultHasher::new();
    for value in state
        .iter()
        .filter(|value| value.effect_id != excluded_effect && !ignored.contains(&value.effect_id))
    {
        value.hash(&mut hasher);
    }
    hasher.finish()
}

fn attack_status_hash(
    state: &[Status],
    attack_effect_ids: &HashSet<i64>,
    excluded_effect: i64,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    for value in state.iter().filter(|value| {
        value.effect_id != excluded_effect && attack_effect_ids.contains(&value.effect_id)
    }) {
        value.hash(&mut hasher);
    }
    hasher.finish()
}

fn statuses_for_hashes(hashes: &[u64], states: &HashMap<u64, Vec<Status>>) -> Vec<Status> {
    hashes
        .iter()
        .filter_map(|hash| states.get(hash))
        .flatten()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn partition(sample: &CompactSample, count: usize) -> usize {
    let mut hasher = DefaultHasher::new();
    sample.rlog.hash(&mut hasher);
    sample.source_entity_uuid.hash(&mut hasher);
    sample.magical_attack.hash(&mut hasher);
    hasher.finish() as usize & (count - 1)
}

fn wire_key(row: &CompactSample) -> WireKey {
    WireKey {
        rlog: row.rlog.clone(),
        session_id: row.session_id.clone(),
        run_ordinal: row.run_ordinal,
        source_entity_uuid: row.source_entity_uuid,
        target_entity_uuid: row.target_entity_uuid,
        class_id: row.class_id,
        ability_id: row.ability_id,
        magical_attack: row.magical_attack,
        critical: row.critical,
        lucky: row.lucky,
        property: row.property,
        damage_mode: row.damage_mode,
        source_state_hash: row.source_state_hash,
        target_state_hash: row.target_state_hash,
        source_status_hash: row.source_status_hash,
        target_status_hash: row.target_status_hash,
        harmony_active: row.harmony_active,
        harmony_provider: row.harmony_provider,
    }
}

fn solve_group(rows: &[CompactSample]) -> Option<Vec<AttackFactorCandidate>> {
    let stages = rows
        .iter()
        .map(|r| (r.coefficient, r.fixed))
        .collect::<BTreeSet<_>>();
    if stages.len() < 2 {
        return None;
    }
    let pair = rows
        .iter()
        .enumerate()
        .flat_map(|(i, a)| rows.iter().skip(i + 1).map(move |b| (a, b)))
        .find(|(a, b)| (a.coefficient, a.fixed) != (b.coefficient, b.fixed))?;
    let numerator = i128::from(10_000_i64)
        * (i128::from(pair.1.output) * i128::from(pair.0.fixed)
            - i128::from(pair.0.output) * i128::from(pair.1.fixed));
    let denominator = i128::from(pair.0.output) * i128::from(pair.1.coefficient)
        - i128::from(pair.1.output) * i128::from(pair.0.coefficient);
    if denominator == 0 {
        return None;
    }
    let center = i64::try_from(numerator / denominator).ok()?;
    let start = center.saturating_sub(ATTACK_SEARCH_RADIUS).max(1);
    let end = center.saturating_add(ATTACK_SEARCH_RADIUS).min(MAX_ATTACK);
    let mut solutions = Vec::new();
    for attack in start..=end {
        let mut low = 1_i64;
        let mut high = i64::MAX;
        let mut valid = true;
        for row in rows {
            let body = attack
                .checked_mul(row.coefficient)?
                .checked_div(10_000)?
                .checked_add(row.fixed)?;
            if body <= 0 {
                valid = false;
                break;
            }
            let lo = ceil_div(row.output.checked_mul(10_000)?, body);
            let hi = (row
                .output
                .checked_add(1)?
                .checked_mul(10_000)?
                .checked_sub(1)?)
            .checked_div(body)?;
            low = low.max(lo);
            high = high.min(hi);
            if low > high {
                valid = false;
                break;
            }
        }
        if valid {
            solutions.push(AttackFactorCandidate {
                attack,
                factor_min: low,
                factor_max: high,
            });
        }
    }
    (!solutions.is_empty()).then_some(solutions)
}

fn ceil_div(value: i64, divisor: i64) -> i64 {
    value / divisor + i64::from(value % divisor != 0)
}

fn pair_key(row: &CompactSample) -> PairKey {
    PairKey {
        rlog: row.rlog.clone(),
        session_id: row.session_id.clone(),
        source_entity_uuid: row.source_entity_uuid,
        class_id: row.class_id,
        ability_id: row.ability_id,
        magical_attack: row.magical_attack,
        critical: row.critical,
        lucky: row.lucky,
        property: row.property,
        damage_mode: row.damage_mode,
    }
}

fn direct_pair_key(row: &CompactSample) -> DirectPairKey {
    DirectPairKey {
        rlog: row.rlog.clone(),
        session_id: row.session_id.clone(),
        run_ordinal: row.run_ordinal,
        source_entity_uuid: row.source_entity_uuid,
        target_entity_uuid: row.target_entity_uuid,
        class_id: row.class_id,
        ability_id: row.ability_id,
        magical_attack: row.magical_attack,
        hit_event_id: row.hit_event_id,
        critical: row.critical,
        lucky: row.lucky,
        property: row.property,
        damage_mode: row.damage_mode,
        source_damage_state_hash: row.source_damage_state_hash,
        source_status_hash: row.source_status_hash,
        source_attack_status_hash: row.source_attack_status_hash,
        source_formula_status_hash: row.source_formula_status_hash,
        target_state_hash: row.target_state_hash,
        target_status_hash: row.target_status_hash,
        coefficient: row.coefficient,
        fixed: row.fixed,
    }
}

fn direct_transition_key(inactive: &CompactSample, active: &CompactSample) -> DirectTransitionKey {
    DirectTransitionKey {
        rlog: active.rlog.clone(),
        session_id: active.session_id.clone(),
        run_ordinal: active.run_ordinal,
        class_id: active.class_id,
        ability_id: active.ability_id,
        hit_event_id: active.hit_event_id,
        source_entity_uuid: active.source_entity_uuid,
        provider_entity_uuid: active.harmony_provider,
        inactive_observed_micros: inactive.observed_micros,
        active_observed_micros: active.observed_micros,
        source_damage_state_hash: active.source_damage_state_hash,
        source_attack_status_hash: active.source_attack_status_hash,
        source_formula_status_hash: active.source_formula_status_hash,
        inactive_source_state_hash: inactive.source_state_hash,
        active_source_state_hash: active.source_state_hash,
        inactive_source_status_hash: inactive.source_status_hash,
        active_source_status_hash: active.source_status_hash,
        coefficient: active.coefficient,
    }
}

fn ratio_is_less(left: RatioBound, right: RatioBound) -> bool {
    i128::from(left.numerator) * i128::from(right.denominator)
        < i128::from(right.numerator) * i128::from(left.denominator)
}

fn ratio_is_less_or_equal(left: RatioBound, right: RatioBound) -> bool {
    !ratio_is_less(right, left)
}

fn actor_state_key(row: &CompactSample) -> ActorStateKey {
    ActorStateKey {
        rlog: row.rlog.clone(),
        session_id: row.session_id.clone(),
        run_ordinal: row.run_ordinal,
        source_entity_uuid: row.source_entity_uuid,
        class_id: row.class_id,
        magical_attack: row.magical_attack,
        source_status_hash: row.source_status_hash,
        source_attack_status_hash: row.source_attack_status_hash,
        harmony_active: row.harmony_active,
        harmony_provider: row.harmony_provider,
    }
}

fn actor_state_pair_key(key: &ActorStateKey) -> ActorStatePairKey {
    ActorStatePairKey {
        rlog: key.rlog.clone(),
        session_id: key.session_id.clone(),
        run_ordinal: key.run_ordinal,
        source_entity_uuid: key.source_entity_uuid,
        class_id: key.class_id,
        magical_attack: key.magical_attack,
        source_attack_status_hash: key.source_attack_status_hash,
    }
}

fn coarse_actor_state_key(row: &CompactSample) -> CoarseActorStateKey {
    CoarseActorStateKey {
        rlog: row.rlog.clone(),
        session_id: row.session_id.clone(),
        run_ordinal: row.run_ordinal,
        source_entity_uuid: row.source_entity_uuid,
        class_id: row.class_id,
        magical_attack: row.magical_attack,
        harmony_active: row.harmony_active,
        harmony_provider: row.harmony_provider,
    }
}

fn coarse_actor_state_pair_key(key: &CoarseActorStateKey) -> CoarseActorStatePairKey {
    CoarseActorStatePairKey {
        rlog: key.rlog.clone(),
        session_id: key.session_id.clone(),
        run_ordinal: key.run_ordinal,
        source_entity_uuid: key.source_entity_uuid,
        class_id: key.class_id,
        magical_attack: key.magical_attack,
    }
}

fn cross_status_pair_key(key: &ActorStateKey) -> CoarseActorStatePairKey {
    CoarseActorStatePairKey {
        rlog: key.rlog.clone(),
        session_id: key.session_id.clone(),
        run_ordinal: key.run_ordinal,
        source_entity_uuid: key.source_entity_uuid,
        class_id: key.class_id,
        magical_attack: key.magical_attack,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args()?;
    fs::create_dir_all(&args.work_dir)?;
    let catalog: Catalog = serde_json::from_reader(BufReader::new(File::open(&args.catalog)?))?;
    let attack_effect_ledger: AttackEffectLedger =
        serde_json::from_reader(BufReader::new(File::open(&args.attack_effect_ledger)?))?;
    let mut attack_effect_ids = HashSet::new();
    let mut formula_effect_ids = HashSet::new();
    for candidate in attack_effect_ledger.candidates {
        if !candidate.formula_term_ids.is_empty() {
            formula_effect_ids.extend(
                candidate
                    .effect_ids
                    .iter()
                    .copied()
                    .filter(|effect_id| *effect_id != target_effect_id()),
            );
        }
        if candidate
            .formula_term_ids
            .iter()
            .any(|term| term == "primaryAttack")
        {
            attack_effect_ids.extend(
                candidate
                    .effect_ids
                    .into_iter()
                    .filter(|effect_id| *effect_id != target_effect_id()),
            );
        }
    }
    if attack_effect_ids.is_empty() {
        return Err("attack-effect ledger selected no exact effect IDs".into());
    }
    if formula_effect_ids.is_empty() {
        return Err("attack-effect ledger selected no formula effect IDs".into());
    }
    let mut rules = HashMap::new();
    let mut rules_by_ability = HashMap::<i64, Vec<StageRule>>::new();
    for rule in catalog.rules {
        rules_by_ability
            .entry(rule.ability_id)
            .or_default()
            .push(rule.clone());
        rules.insert(
            (rule.ability_id, rule.hit_event_id, rule.damage_source),
            rule,
        );
    }
    let mut actor_classes = HashMap::new();
    let mut actor_class_conflicts = HashSet::new();
    if let Some(identity_cohort) = &args.identity_cohort {
        let mut de =
            serde_json::Deserializer::from_reader(BufReader::new(File::open(identity_cohort)?));
        IdentityCohortSeed {
            actor_classes: &mut actor_classes,
            conflicts: &mut actor_class_conflicts,
        }
        .deserialize(&mut de)?;
    }
    let paths = (0..PARTITIONS)
        .map(|i| args.work_dir.join(format!("remote-stage-{i:02}.ndjson")))
        .collect::<Vec<_>>();
    let mut writers = paths
        .iter()
        .map(|p| File::create(p).map(BufWriter::new))
        .collect::<Result<Vec<_>, _>>()?;
    let mut scanned = 0;
    let mut selected = 0;
    let mut selected_by_class_ability = BTreeMap::new();
    let mut source_attribute_states = HashMap::new();
    let mut source_status_states = HashMap::new();
    let mut de = serde_json::Deserializer::from_reader(BufReader::new(File::open(&args.cohort)?));
    CohortSeed {
        rules: &rules,
        rules_by_ability: &rules_by_ability,
        actor_classes: &actor_classes,
        attack_effect_ids: &attack_effect_ids,
        formula_effect_ids: &formula_effect_ids,
        source_attribute_states: &mut source_attribute_states,
        source_status_states: &mut source_status_states,
        writers: &mut writers,
        scanned: &mut scanned,
        selected: &mut selected,
        selected_by_class_ability: &mut selected_by_class_ability,
    }
    .deserialize(&mut de)?;
    for writer in &mut writers {
        writer.flush()?;
    }
    drop(writers);
    let mut multi = 0_u64;
    let mut multi_by_class_ability = BTreeMap::<String, u64>::new();
    let mut solved_count = 0_u64;
    let mut solved_by_class_ability = BTreeMap::<String, u64>::new();
    let mut active_solved = 0_u64;
    let mut inactive_solved = 0_u64;
    let mut active_solved_by_class_ability = BTreeMap::<String, u64>::new();
    let mut inactive_solved_by_class_ability = BTreeMap::<String, u64>::new();
    let mut pair_count = 0_u64;
    let mut ambiguous_pair_count = 0_u64;
    let mut positive = 0_u64;
    let mut histogram = BTreeMap::<i32, BTreeMap<i64, u64>>::new();
    let mut uniquely_solved_actor_states = 0_u64;
    let mut actor_state_pair_count = 0_u64;
    let mut positive_actor_state_pairs = 0_u64;
    let mut actor_state_histogram = BTreeMap::<i32, BTreeMap<i64, u64>>::new();
    let mut cross_status_exact_actor_state_pair_count = 0_u64;
    let mut positive_cross_status_exact_actor_state_pairs = 0_u64;
    let mut cross_status_exact_actor_histogram = BTreeMap::<i32, BTreeMap<i64, u64>>::new();
    let mut cross_status_exact_examples = Vec::new();
    let mut consensus_solved_actor_states = 0_u64;
    let mut consensus_actor_state_pair_count = 0_u64;
    let mut positive_consensus_actor_state_pairs = 0_u64;
    let mut consensus_actor_state_histogram = BTreeMap::<i32, BTreeMap<i64, u64>>::new();
    let mut coarse_consensus_solved_actor_states = 0_u64;
    let mut coarse_consensus_actor_state_pair_count = 0_u64;
    let mut positive_coarse_consensus_actor_state_pairs = 0_u64;
    let mut coarse_consensus_actor_state_histogram = BTreeMap::<i32, BTreeMap<i64, u64>>::new();
    let mut coarse_consensus_examples = Vec::new();
    let mut direct_formula_context_pairs = 0_u64;
    let mut positive_direct_formula_context_pairs = 0_u64;
    let mut direct_formula_context_histogram = BTreeMap::<i32, BTreeMap<i64, u64>>::new();
    let mut direct_formula_context_examples = Vec::new();
    let mut proportional_groups = BTreeMap::<DirectTransitionKey, Vec<(i64, i64, i64)>>::new();
    let mut examples = Vec::new();
    for path in &paths {
        let mut groups = BTreeMap::<WireKey, Vec<CompactSample>>::new();
        let mut direct_pairs =
            BTreeMap::<DirectPairKey, (Vec<CompactSample>, Vec<CompactSample>)>::new();
        for line in BufReader::new(File::open(path)?).lines() {
            let row: CompactSample = serde_json::from_str(&line?)?;
            let direct_entry = direct_pairs.entry(direct_pair_key(&row)).or_default();
            if row.harmony_active {
                direct_entry.1.push(row.clone());
            } else {
                direct_entry.0.push(row.clone());
            }
            groups.entry(wire_key(&row)).or_default().push(row);
        }
        for (_key, (inactive, active)) in direct_pairs {
            for active_row in active {
                let Some(inactive_row) = inactive
                    .iter()
                    .min_by_key(|row| row.observed_micros.abs_diff(active_row.observed_micros))
                else {
                    continue;
                };
                if inactive_row
                    .observed_micros
                    .abs_diff(active_row.observed_micros)
                    > 30_000_000
                {
                    continue;
                }
                direct_formula_context_pairs += 1;
                let delta = active_row.output - inactive_row.output;
                if delta > 0 {
                    positive_direct_formula_context_pairs += 1;
                    if active_row.fixed == 0 {
                        proportional_groups
                            .entry(direct_transition_key(inactive_row, &active_row))
                            .or_default()
                            .push((
                                active_row.output,
                                inactive_row.output,
                                active_row.target_entity_uuid,
                            ));
                    }
                }
                *direct_formula_context_histogram
                    .entry(active_row.class_id)
                    .or_default()
                    .entry(delta)
                    .or_default() += 1;
                if direct_formula_context_examples.len() < 256 {
                    direct_formula_context_examples.push(DirectPairExample {
                        rlog: active_row.rlog.clone(),
                        session_id: active_row.session_id.clone(),
                        run_ordinal: active_row.run_ordinal,
                        class_id: active_row.class_id,
                        ability_id: active_row.ability_id,
                        hit_event_id: active_row.hit_event_id,
                        source_entity_uuid: active_row.source_entity_uuid,
                        target_entity_uuid: active_row.target_entity_uuid,
                        provider_entity_uuid: active_row.harmony_provider,
                        inactive_output: inactive_row.output,
                        active_output: active_row.output,
                        output_delta: delta,
                        source_damage_state_hash: active_row.source_damage_state_hash,
                        source_attack_status_hash: active_row.source_attack_status_hash,
                        source_formula_status_hash: active_row.source_formula_status_hash,
                        target_state_hash: active_row.target_state_hash,
                        target_status_hash: active_row.target_status_hash,
                        inactive_source_state_hash: inactive_row.source_state_hash,
                        active_source_state_hash: active_row.source_state_hash,
                        inactive_source_status_hash: inactive_row.source_status_hash,
                        active_source_status_hash: active_row.source_status_hash,
                        inactive_source_attributes: source_attribute_states
                            .get(&inactive_row.source_state_hash)
                            .cloned()
                            .unwrap_or_default(),
                        active_source_attributes: source_attribute_states
                            .get(&active_row.source_state_hash)
                            .cloned()
                            .unwrap_or_default(),
                        inactive_source_statuses: source_status_states
                            .get(&inactive_row.source_status_hash)
                            .cloned()
                            .unwrap_or_default(),
                        active_source_statuses: source_status_states
                            .get(&active_row.source_status_hash)
                            .cloned()
                            .unwrap_or_default(),
                        inactive_observed_micros: inactive_row.observed_micros,
                        active_observed_micros: active_row.observed_micros,
                    });
                }
            }
        }
        for rows in groups.values() {
            if rows
                .iter()
                .map(|r| (r.coefficient, r.fixed))
                .collect::<BTreeSet<_>>()
                .len()
                >= 2
            {
                multi += 1;
                let representative = &rows[0];
                *multi_by_class_ability
                    .entry(format!(
                        "{}:{}",
                        representative.class_id, representative.ability_id
                    ))
                    .or_default() += 1;
            }
        }
        let mut paired = BTreeMap::<
            PairKey,
            (
                Vec<(CompactSample, Vec<AttackFactorCandidate>)>,
                Vec<(CompactSample, Vec<AttackFactorCandidate>)>,
            ),
        >::new();
        let mut actor_states =
            BTreeMap::<ActorStateKey, Vec<(CompactSample, Vec<AttackFactorCandidate>)>>::new();
        let mut coarse_actor_states =
            BTreeMap::<CoarseActorStateKey, Vec<(CompactSample, Vec<AttackFactorCandidate>)>>::new(
            );
        for (_key, rows) in groups {
            if rows
                .iter()
                .map(|r| (r.coefficient, r.fixed))
                .collect::<BTreeSet<_>>()
                .len()
                < 2
            {
                continue;
            }
            let Some(candidates) = solve_group(&rows) else {
                continue;
            };
            solved_count += 1;
            let representative = rows[0].clone();
            *solved_by_class_ability
                .entry(format!(
                    "{}:{}",
                    representative.class_id, representative.ability_id
                ))
                .or_default() += 1;
            actor_states
                .entry(actor_state_key(&representative))
                .or_default()
                .push((representative.clone(), candidates.clone()));
            coarse_actor_states
                .entry(coarse_actor_state_key(&representative))
                .or_default()
                .push((representative.clone(), candidates.clone()));
            let entry = paired.entry(pair_key(&representative)).or_default();
            if representative.harmony_active {
                active_solved += 1;
                *active_solved_by_class_ability
                    .entry(format!(
                        "{}:{}",
                        representative.class_id, representative.ability_id
                    ))
                    .or_default() += 1;
                entry.1.push((representative, candidates));
            } else {
                inactive_solved += 1;
                *inactive_solved_by_class_ability
                    .entry(format!(
                        "{}:{}",
                        representative.class_id, representative.ability_id
                    ))
                    .or_default() += 1;
                entry.0.push((representative, candidates));
            }
        }
        for (_key, (inactive, active)) in paired {
            for (active_row, active_candidates) in &active {
                let Some((inactive_row, inactive_candidates)) =
                    inactive.iter().min_by_key(|(row, _)| {
                        row.observed_micros.abs_diff(active_row.observed_micros)
                    })
                else {
                    continue;
                };
                let mut deltas = BTreeSet::new();
                let mut witness = None;
                'outer: for active_candidate in active_candidates {
                    for inactive_candidate in inactive_candidates {
                        let lo = inactive_candidate
                            .factor_min
                            .max(active_candidate.factor_min);
                        let hi = inactive_candidate
                            .factor_max
                            .min(active_candidate.factor_max);
                        if lo <= hi {
                            let delta = active_candidate.attack - inactive_candidate.attack;
                            deltas.insert(delta);
                            witness.get_or_insert((*inactive_candidate, *active_candidate, lo, hi));
                            if deltas.len() > 1 {
                                break 'outer;
                            }
                        }
                    }
                }
                if deltas.is_empty() {
                    continue;
                }
                if deltas.len() != 1 {
                    ambiguous_pair_count += 1;
                    continue;
                }
                pair_count += 1;
                let delta = *deltas.first().unwrap();
                if delta > 0 {
                    positive += 1;
                }
                *histogram
                    .entry(active_row.class_id)
                    .or_default()
                    .entry(delta)
                    .or_default() += 1;
                if examples.len() < 32 {
                    let (inactive_candidate, active_candidate, lo, hi) = witness.unwrap();
                    examples.push(PairExample {
                        evidence_kind: "shared_downstream_factor",
                        class_id: active_row.class_id,
                        ability_id: active_row.ability_id,
                        source_entity_uuid: active_row.source_entity_uuid,
                        provider_entity_uuid: active_row.harmony_provider,
                        inactive_attack: inactive_candidate.attack,
                        active_attack: active_candidate.attack,
                        attack_delta: delta,
                        shared_factor_min: lo,
                        shared_factor_max: hi,
                        inactive_observed_micros: inactive_row.observed_micros,
                        active_observed_micros: active_row.observed_micros,
                    });
                }
            }
        }
        let mut solved_states =
            BTreeMap::<ActorStatePairKey, (Vec<ActorStateSolution>, Vec<ActorStateSolution>)>::new(
            );
        let mut cross_status_exact_states = BTreeMap::<
            CoarseActorStatePairKey,
            (Vec<ActorStateSolution>, Vec<ActorStateSolution>),
        >::new();
        let mut consensus_states =
            BTreeMap::<ActorStatePairKey, (Vec<ActorStateSolution>, Vec<ActorStateSolution>)>::new(
            );
        for (key, groups) in actor_states {
            if groups.len() < 2 {
                continue;
            }
            let mut counts = BTreeMap::<i64, usize>::new();
            for (_row, candidates) in &groups {
                for attack in candidates
                    .iter()
                    .map(|candidate| candidate.attack)
                    .collect::<BTreeSet<_>>()
                {
                    *counts.entry(attack).or_default() += 1;
                }
            }
            let attribute_hashes = groups
                .iter()
                .map(|(row, _)| row.source_state_hash)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let status_hashes = groups
                .iter()
                .map(|(row, _)| row.source_status_hash)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let ability_ids = groups
                .iter()
                .map(|(row, _)| row.ability_id)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let target_entity_uuids = groups
                .iter()
                .map(|(row, _)| row.target_entity_uuid)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let shared = counts
                .iter()
                .filter_map(|(attack, count)| (*count == groups.len()).then_some(*attack))
                .collect::<Vec<_>>();
            if shared.len() == 1 {
                uniquely_solved_actor_states += 1;
                let solution = ActorStateSolution {
                    key: key.clone(),
                    attack: shared[0],
                    supporting_groups: groups.len(),
                    status_hashes: status_hashes.clone(),
                    attribute_hashes: attribute_hashes.clone(),
                    ability_ids: ability_ids.clone(),
                    target_entity_uuids: target_entity_uuids.clone(),
                    observed_micros: groups[0].0.observed_micros,
                };
                let cross_entry = cross_status_exact_states
                    .entry(cross_status_pair_key(&key))
                    .or_default();
                if key.harmony_active {
                    cross_entry.1.push(solution.clone());
                } else {
                    cross_entry.0.push(solution.clone());
                }
                let entry = solved_states.entry(actor_state_pair_key(&key)).or_default();
                if key.harmony_active {
                    entry.1.push(solution);
                } else {
                    entry.0.push(solution);
                }
            }
            let max_support = counts.values().copied().max().unwrap_or_default();
            let consensus = counts
                .iter()
                .filter_map(|(attack, count)| (*count == max_support).then_some(*attack))
                .collect::<Vec<_>>();
            if max_support >= 2 && consensus.len() == 1 {
                consensus_solved_actor_states += 1;
                let solution = ActorStateSolution {
                    key: key.clone(),
                    attack: consensus[0],
                    supporting_groups: max_support,
                    status_hashes,
                    attribute_hashes,
                    ability_ids,
                    target_entity_uuids,
                    observed_micros: groups[0].0.observed_micros,
                };
                let entry = consensus_states
                    .entry(actor_state_pair_key(&key))
                    .or_default();
                if key.harmony_active {
                    entry.1.push(solution);
                } else {
                    entry.0.push(solution);
                }
            }
        }
        for (_key, (inactive, active)) in solved_states {
            for active_state in active {
                let Some(inactive_state) = inactive.iter().min_by_key(|state| {
                    state.observed_micros.abs_diff(active_state.observed_micros)
                }) else {
                    continue;
                };
                actor_state_pair_count += 1;
                let delta = active_state.attack - inactive_state.attack;
                if delta > 0 {
                    positive_actor_state_pairs += 1;
                }
                *actor_state_histogram
                    .entry(active_state.key.class_id)
                    .or_default()
                    .entry(delta)
                    .or_default() += 1;
                if examples.len() < 32 {
                    examples.push(PairExample {
                        evidence_kind: "exact_actor_state_intersection",
                        class_id: active_state.key.class_id,
                        ability_id: 0,
                        source_entity_uuid: active_state.key.source_entity_uuid,
                        provider_entity_uuid: active_state.key.harmony_provider,
                        inactive_attack: inactive_state.attack,
                        active_attack: active_state.attack,
                        attack_delta: delta,
                        shared_factor_min: inactive_state.supporting_groups as i64,
                        shared_factor_max: active_state.supporting_groups as i64,
                        inactive_observed_micros: inactive_state.observed_micros,
                        active_observed_micros: active_state.observed_micros,
                    });
                }
            }
        }
        for (_key, (inactive, active)) in cross_status_exact_states {
            for active_state in active {
                let Some(inactive_state) = inactive.iter().min_by_key(|state| {
                    state.observed_micros.abs_diff(active_state.observed_micros)
                }) else {
                    continue;
                };
                cross_status_exact_actor_state_pair_count += 1;
                let delta = active_state.attack - inactive_state.attack;
                if delta > 0 {
                    positive_cross_status_exact_actor_state_pairs += 1;
                }
                *cross_status_exact_actor_histogram
                    .entry(active_state.key.class_id)
                    .or_default()
                    .entry(delta)
                    .or_default() += 1;
                if cross_status_exact_examples.len() < 128 {
                    cross_status_exact_examples.push(CrossStatusPairExample {
                        rlog: active_state.key.rlog.clone(),
                        session_id: active_state.key.session_id.clone(),
                        run_ordinal: active_state.key.run_ordinal,
                        class_id: active_state.key.class_id,
                        magical_attack: active_state.key.magical_attack,
                        source_entity_uuid: active_state.key.source_entity_uuid,
                        provider_entity_uuid: active_state.key.harmony_provider,
                        inactive_attack: inactive_state.attack,
                        active_attack: active_state.attack,
                        attack_delta: delta,
                        inactive_supporting_groups: inactive_state.supporting_groups,
                        active_supporting_groups: active_state.supporting_groups,
                        inactive_attack_status_hash: inactive_state.key.source_attack_status_hash,
                        active_attack_status_hash: active_state.key.source_attack_status_hash,
                        inactive_status_hashes: inactive_state.status_hashes.clone(),
                        active_status_hashes: active_state.status_hashes.clone(),
                        inactive_statuses: statuses_for_hashes(
                            &inactive_state.status_hashes,
                            &source_status_states,
                        ),
                        active_statuses: statuses_for_hashes(
                            &active_state.status_hashes,
                            &source_status_states,
                        ),
                        inactive_attribute_hashes: inactive_state.attribute_hashes.clone(),
                        active_attribute_hashes: active_state.attribute_hashes.clone(),
                        inactive_attribute_states: inactive_state
                            .attribute_hashes
                            .iter()
                            .filter_map(|hash| source_attribute_states.get(hash).cloned())
                            .collect(),
                        active_attribute_states: active_state
                            .attribute_hashes
                            .iter()
                            .filter_map(|hash| source_attribute_states.get(hash).cloned())
                            .collect(),
                        inactive_ability_ids: inactive_state.ability_ids.clone(),
                        active_ability_ids: active_state.ability_ids.clone(),
                        inactive_target_entity_uuids: inactive_state.target_entity_uuids.clone(),
                        active_target_entity_uuids: active_state.target_entity_uuids.clone(),
                        inactive_observed_micros: inactive_state.observed_micros,
                        active_observed_micros: active_state.observed_micros,
                    });
                }
                if examples.len() < 96 {
                    examples.push(PairExample {
                        evidence_kind: "exact_actor_state_cross_status_nearest",
                        class_id: active_state.key.class_id,
                        ability_id: 0,
                        source_entity_uuid: active_state.key.source_entity_uuid,
                        provider_entity_uuid: active_state.key.harmony_provider,
                        inactive_attack: inactive_state.attack,
                        active_attack: active_state.attack,
                        attack_delta: delta,
                        shared_factor_min: inactive_state.supporting_groups as i64,
                        shared_factor_max: active_state.supporting_groups as i64,
                        inactive_observed_micros: inactive_state.observed_micros,
                        active_observed_micros: active_state.observed_micros,
                    });
                }
            }
        }
        for (_key, (inactive, active)) in consensus_states {
            for active_state in active {
                let Some(inactive_state) = inactive.iter().min_by_key(|state| {
                    state.observed_micros.abs_diff(active_state.observed_micros)
                }) else {
                    continue;
                };
                consensus_actor_state_pair_count += 1;
                let delta = active_state.attack - inactive_state.attack;
                if delta > 0 {
                    positive_consensus_actor_state_pairs += 1;
                }
                *consensus_actor_state_histogram
                    .entry(active_state.key.class_id)
                    .or_default()
                    .entry(delta)
                    .or_default() += 1;
                if examples.len() < 64 {
                    examples.push(PairExample {
                        evidence_kind: "unique_repeated_support_consensus",
                        class_id: active_state.key.class_id,
                        ability_id: 0,
                        source_entity_uuid: active_state.key.source_entity_uuid,
                        provider_entity_uuid: active_state.key.harmony_provider,
                        inactive_attack: inactive_state.attack,
                        active_attack: active_state.attack,
                        attack_delta: delta,
                        shared_factor_min: inactive_state.supporting_groups as i64,
                        shared_factor_max: active_state.supporting_groups as i64,
                        inactive_observed_micros: inactive_state.observed_micros,
                        active_observed_micros: active_state.observed_micros,
                    });
                }
            }
        }
        let mut coarse_consensus_states = BTreeMap::<
            CoarseActorStatePairKey,
            (Vec<CoarseActorStateSolution>, Vec<CoarseActorStateSolution>),
        >::new();
        for (key, groups) in coarse_actor_states {
            if groups.len() < 2 {
                continue;
            }
            let mut counts = BTreeMap::<i64, usize>::new();
            for (_row, candidates) in &groups {
                for attack in candidates
                    .iter()
                    .map(|candidate| candidate.attack)
                    .collect::<BTreeSet<_>>()
                {
                    *counts.entry(attack).or_default() += 1;
                }
            }
            let max_support = counts.values().copied().max().unwrap_or_default();
            let consensus = counts
                .iter()
                .filter_map(|(attack, count)| (*count == max_support).then_some(*attack))
                .collect::<Vec<_>>();
            if max_support < 2 || consensus.len() != 1 {
                continue;
            }
            let supporting_rows = groups
                .iter()
                .filter_map(|(row, candidates)| {
                    candidates
                        .iter()
                        .any(|candidate| candidate.attack == consensus[0])
                        .then_some(row)
                })
                .collect::<Vec<_>>();
            coarse_consensus_solved_actor_states += 1;
            let solution = CoarseActorStateSolution {
                key: key.clone(),
                attack: consensus[0],
                supporting_groups: max_support,
                status_variants: supporting_rows
                    .iter()
                    .map(|row| row.source_status_hash)
                    .collect::<BTreeSet<_>>()
                    .len(),
                attribute_variants: supporting_rows
                    .iter()
                    .map(|row| row.source_state_hash)
                    .collect::<BTreeSet<_>>()
                    .len(),
                ability_ids: supporting_rows
                    .iter()
                    .map(|row| row.ability_id)
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect(),
                target_entity_uuids: supporting_rows
                    .iter()
                    .map(|row| row.target_entity_uuid)
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect(),
                status_hashes: supporting_rows
                    .iter()
                    .map(|row| row.source_status_hash)
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect(),
                attribute_hashes: supporting_rows
                    .iter()
                    .map(|row| row.source_state_hash)
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect(),
                observed_micros: groups[0].0.observed_micros,
            };
            let entry = coarse_consensus_states
                .entry(coarse_actor_state_pair_key(&key))
                .or_default();
            if key.harmony_active {
                entry.1.push(solution);
            } else {
                entry.0.push(solution);
            }
        }
        for (_key, (inactive, active)) in coarse_consensus_states {
            for active_state in active {
                let Some(inactive_state) = inactive.iter().min_by_key(|state| {
                    state.observed_micros.abs_diff(active_state.observed_micros)
                }) else {
                    continue;
                };
                coarse_consensus_actor_state_pair_count += 1;
                let delta = active_state.attack - inactive_state.attack;
                if delta > 0 {
                    positive_coarse_consensus_actor_state_pairs += 1;
                }
                *coarse_consensus_actor_state_histogram
                    .entry(active_state.key.class_id)
                    .or_default()
                    .entry(delta)
                    .or_default() += 1;
                if coarse_consensus_examples.len() < 64 {
                    coarse_consensus_examples.push(CoarsePairExample {
                        rlog: active_state.key.rlog.clone(),
                        session_id: active_state.key.session_id.clone(),
                        run_ordinal: active_state.key.run_ordinal,
                        class_id: active_state.key.class_id,
                        magical_attack: active_state.key.magical_attack,
                        source_entity_uuid: active_state.key.source_entity_uuid,
                        provider_entity_uuid: active_state.key.harmony_provider,
                        inactive_attack: inactive_state.attack,
                        active_attack: active_state.attack,
                        attack_delta: delta,
                        inactive_supporting_groups: inactive_state.supporting_groups,
                        active_supporting_groups: active_state.supporting_groups,
                        inactive_status_variants: inactive_state.status_variants,
                        active_status_variants: active_state.status_variants,
                        inactive_attribute_variants: inactive_state.attribute_variants,
                        active_attribute_variants: active_state.attribute_variants,
                        inactive_ability_ids: inactive_state.ability_ids.clone(),
                        active_ability_ids: active_state.ability_ids.clone(),
                        inactive_target_entity_uuids: inactive_state.target_entity_uuids.clone(),
                        active_target_entity_uuids: active_state.target_entity_uuids.clone(),
                        inactive_status_hashes: inactive_state.status_hashes.clone(),
                        active_status_hashes: active_state.status_hashes.clone(),
                        inactive_attribute_hashes: inactive_state.attribute_hashes.clone(),
                        active_attribute_hashes: active_state.attribute_hashes.clone(),
                        inactive_observed_micros: inactive_state.observed_micros,
                        active_observed_micros: active_state.observed_micros,
                    });
                }
            }
        }
    }
    let mut proportional_zero_fixed_anchors = Vec::new();
    for (key, rows) in proportional_groups {
        let distinct_targets = rows
            .iter()
            .map(|(_, _, target)| *target)
            .collect::<BTreeSet<_>>()
            .len();
        let distinct_output_pairs = rows
            .iter()
            .map(|(active, inactive, _)| (*active, *inactive))
            .collect::<BTreeSet<_>>()
            .len();
        if distinct_targets < 2 || distinct_output_pairs < 2 {
            continue;
        }
        let mut lower = None::<RatioBound>;
        let mut upper = None::<RatioBound>;
        for (active, inactive, _) in &rows {
            let candidate_lower = RatioBound {
                numerator: *inactive,
                denominator: active + 1,
            };
            let candidate_upper = RatioBound {
                numerator: inactive + 1,
                denominator: *active,
            };
            if lower.is_none_or(|current| ratio_is_less(current, candidate_lower)) {
                lower = Some(candidate_lower);
            }
            if upper.is_none_or(|current| ratio_is_less(candidate_upper, current)) {
                upper = Some(candidate_upper);
            }
        }
        let (Some(lower), Some(upper)) = (lower, upper) else {
            continue;
        };
        if !ratio_is_less_or_equal(lower, upper) {
            continue;
        }
        let active_total = rows
            .iter()
            .try_fold(0_i64, |sum, (active, _, _)| sum.checked_add(*active))
            .ok_or("proportional anchor active damage overflow")?;
        let inactive_total = rows
            .iter()
            .try_fold(0_i64, |sum, (_, inactive, _)| sum.checked_add(*inactive))
            .ok_or("proportional anchor inactive damage overflow")?;
        proportional_zero_fixed_anchors.push(ProportionalAnchorExample {
            rlog: key.rlog,
            session_id: key.session_id,
            run_ordinal: key.run_ordinal,
            class_id: key.class_id,
            ability_id: key.ability_id,
            hit_event_id: key.hit_event_id,
            source_entity_uuid: key.source_entity_uuid,
            provider_entity_uuid: key.provider_entity_uuid,
            inactive_observed_micros: key.inactive_observed_micros,
            active_observed_micros: key.active_observed_micros,
            observed_rows: rows.len(),
            distinct_targets,
            distinct_output_pairs,
            observed_active_damage: active_total,
            observed_inactive_damage: inactive_total,
            observed_exact_delta: active_total - inactive_total,
            full_source_attributes_equal: key.inactive_source_state_hash
                == key.active_source_state_hash,
            full_non_harmony_source_statuses_equal: key.inactive_source_status_hash
                == key.active_source_status_hash,
            inactive_source_state_hash: key.inactive_source_state_hash,
            active_source_state_hash: key.active_source_state_hash,
            inactive_source_status_hash: key.inactive_source_status_hash,
            active_source_status_hash: key.active_source_status_hash,
            inactive_source_statuses: source_status_states
                .get(&key.inactive_source_status_hash)
                .cloned()
                .unwrap_or_default(),
            active_source_statuses: source_status_states
                .get(&key.active_source_status_hash)
                .cloned()
                .unwrap_or_default(),
            counterfactual_ratio_lower: lower,
            counterfactual_ratio_upper: upper,
            provider_share_lower: RatioBound {
                numerator: upper.denominator - upper.numerator,
                denominator: upper.denominator,
            },
            provider_share_upper: RatioBound {
                numerator: lower.denominator - lower.numerator,
                denominator: lower.denominator,
            },
        });
    }
    let mut attack_affecting_effect_ids = attack_effect_ids.iter().copied().collect::<Vec<_>>();
    attack_affecting_effect_ids.sort_unstable();
    let mut formula_affecting_effect_ids = formula_effect_ids.iter().copied().collect::<Vec<_>>();
    formula_affecting_effect_ids.sort_unstable();
    let ignored_source_effect_ids = ignored_source_effect_ids()
        .iter()
        .copied()
        .collect::<Vec<_>>();
    let report = Report {
        schema_version: 3,
        generated_by: "rlogs-bpsr-remote-stage-counterfactual-proof",
        effect_id: target_effect_id(),
        ignored_source_effect_ids,
        attack_affecting_effect_ids,
        formula_affecting_effect_ids,
        policy: BTreeMap::from([
            (
                "source_stats",
                "never substituted from local or current profiles",
            ),
            (
                "identity_bridge",
                "class identity may be bridged only from the same rlog, session, run, and source entity in the supplied newer-schema cohort; conflicts are rejected",
            ),
            (
                "missing_hit_bridge",
                "a missing hit event is accepted only when every matching exact-build ability rule at the packet stage and level collapses to one identical coefficient, fixed term, and Attack lane",
            ),
            (
                "stage_solve",
                "each hidden-Attack candidate is solved only from distinct coefficient/fixed stages of one exact ability; different abilities may intersect only after their independent solves",
            ),
            (
                "pairing",
                "same run, actor, class, and Attack lane; direct pairs require every non-selected and non-explicitly-ignored source status plus the complete formula context to match; ignored IDs must be independently proven damage-neutral and are recorded in the report",
            ),
            (
                "attack_effect_selection",
                "exact effect IDs from current-build formula-magnitude-gap candidates whose formula_term_ids contain primaryAttack; the selected effect is excluded from its own confounder fingerprint",
            ),
            (
                "authority",
                "diagnostic only until replicated exact deltas and source-attribute/target-status differences close operation order, stacking, and conservation",
            ),
        ]),
        identity_actor_mappings: actor_classes.len(),
        identity_actor_conflicts_rejected: actor_class_conflicts.len(),
        scanned_samples: scanned,
        selected_stage_samples: selected,
        selected_samples_by_class_ability: selected_by_class_ability,
        multi_stage_wire_groups: multi,
        multi_stage_groups_by_class_ability: multi_by_class_ability,
        candidate_solved_wire_groups: solved_count,
        candidate_solved_groups_by_class_ability: solved_by_class_ability,
        active_solved_wire_groups: active_solved,
        inactive_solved_wire_groups: inactive_solved,
        active_solved_groups_by_class_ability: active_solved_by_class_ability,
        inactive_solved_groups_by_class_ability: inactive_solved_by_class_ability,
        exact_active_inactive_pairs: pair_count,
        ambiguous_active_inactive_pairs: ambiguous_pair_count,
        positive_delta_pairs: positive,
        delta_histogram_by_class: histogram,
        uniquely_solved_actor_states,
        exact_actor_state_active_inactive_pairs: actor_state_pair_count,
        positive_actor_state_delta_pairs: positive_actor_state_pairs,
        actor_state_delta_histogram_by_class: actor_state_histogram,
        cross_status_exact_actor_state_pairs: cross_status_exact_actor_state_pair_count,
        positive_cross_status_exact_actor_state_pairs,
        cross_status_exact_actor_delta_histogram_by_class: cross_status_exact_actor_histogram,
        cross_status_exact_examples,
        consensus_solved_actor_states,
        consensus_actor_state_active_inactive_pairs: consensus_actor_state_pair_count,
        positive_consensus_actor_state_delta_pairs: positive_consensus_actor_state_pairs,
        consensus_actor_state_delta_histogram_by_class: consensus_actor_state_histogram,
        coarse_consensus_solved_actor_states,
        coarse_consensus_actor_state_active_inactive_pairs: coarse_consensus_actor_state_pair_count,
        positive_coarse_consensus_actor_state_delta_pairs:
            positive_coarse_consensus_actor_state_pairs,
        coarse_consensus_actor_state_delta_histogram_by_class:
            coarse_consensus_actor_state_histogram,
        coarse_consensus_examples,
        direct_formula_context_pairs,
        positive_direct_formula_context_pairs,
        direct_formula_context_delta_histogram_by_class: direct_formula_context_histogram,
        direct_formula_context_examples,
        proportional_zero_fixed_anchors,
        examples,
        formula_authority: false,
        runtime_authority: false,
        provider_rdps_credit_allowed: false,
    };
    serde_json::to_writer_pretty(BufWriter::new(File::create(&args.output)?), &report)?;
    for path in paths {
        let _ = fs::remove_file(path);
    }
    let _ = fs::remove_dir(&args.work_dir);
    Ok(())
}

fn parse_args() -> Result<Arguments, String> {
    let mut values = env::args().skip(1);
    let mut cohort = None;
    let mut identity_cohort = None;
    let mut catalog = None;
    let mut attack_effect_ledger = None;
    let mut output = None;
    let mut work_dir = None;
    while let Some(key) = values.next() {
        let value = values
            .next()
            .ok_or_else(|| format!("missing value for {key}"))?;
        match key.as_str() {
            "--cohort" => cohort = Some(value.into()),
            "--identity-cohort" => identity_cohort = Some(value.into()),
            "--catalog" => catalog = Some(value.into()),
            "--attack-effect-ledger" => attack_effect_ledger = Some(value.into()),
            "--output" => output = Some(value.into()),
            "--work-dir" => work_dir = Some(value.into()),
            _ => return Err(format!("unknown argument {key}")),
        }
    }
    Ok(Arguments {
        cohort: cohort.ok_or("missing --cohort")?,
        identity_cohort,
        catalog: catalog.ok_or("missing --catalog")?,
        attack_effect_ledger: attack_effect_ledger.ok_or("missing --attack-effect-ledger")?,
        output: output.ok_or("missing --output")?,
        work_dir: work_dir.ok_or("missing --work-dir")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_selection_uses_packet_stage_and_level() {
        let rule = StageRule {
            ability_id: 1,
            hit_event_id: 2,
            damage_source: None,
            damage_script: "MAttack".to_owned(),
            coefficient_basis_points_by_stage: vec![7_000, 8_000],
            fixed_parameter_by_level: vec![10, 20],
        };
        let packet = DamagePacketDetail {
            owner_stage: Some(1),
            owner_level: Some(2),
            ..DamagePacketDetail::default()
        };
        assert_eq!(rule.select(&packet), Some((8_000, 20, true)));
    }

    #[test]
    fn factor_interval_ceiling_is_exact_for_positive_values() {
        assert_eq!(ceil_div(10, 5), 2);
        assert_eq!(ceil_div(11, 5), 3);
    }

    #[test]
    fn attack_status_fingerprint_uses_only_exact_selected_effect_ids() {
        let selected = HashSet::from([2_110_115]);
        let status = |effect_id| Status {
            effect_id,
            source_entity_uuid: Some(7),
            stacks: Some(1),
            level: Some(1),
            origin_source_type_id: Some(1),
            origin_source_config_id: Some(effect_id),
        };
        let baseline = attack_status_hash(&[status(2_110_115)], &selected, target_effect_id());
        assert_eq!(
            baseline,
            attack_status_hash(
                &[
                    status(2_110_115),
                    status(27_016),
                    status(target_effect_id())
                ],
                &selected,
                target_effect_id(),
            )
        );
        assert_ne!(
            baseline,
            attack_status_hash(
                &[status(2_110_115), status(2_110_115)],
                &selected,
                target_effect_id()
            )
        );
    }

    #[test]
    fn damage_state_fingerprint_ignores_hp_score_and_speed_not_attack() {
        let states = vec![
            vec![
                Attribute {
                    attribute_id: 11_320,
                    value: 100,
                },
                Attribute {
                    attribute_id: 11_720,
                    value: 200,
                },
                Attribute {
                    attribute_id: 11_340,
                    value: 300,
                },
            ],
            vec![
                Attribute {
                    attribute_id: 11_320,
                    value: 101,
                },
                Attribute {
                    attribute_id: 11_720,
                    value: 201,
                },
                Attribute {
                    attribute_id: 11_340,
                    value: 300,
                },
            ],
            vec![
                Attribute {
                    attribute_id: 11_320,
                    value: 101,
                },
                Attribute {
                    attribute_id: 11_720,
                    value: 201,
                },
                Attribute {
                    attribute_id: 11_340,
                    value: 301,
                },
            ],
        ];
        let first = damage_state_hash::<serde::de::value::Error>(&states, 0).unwrap();
        assert_eq!(
            first,
            damage_state_hash::<serde::de::value::Error>(&states, 1).unwrap()
        );
        assert_ne!(
            first,
            damage_state_hash::<serde::de::value::Error>(&states, 2).unwrap()
        );
    }
}
