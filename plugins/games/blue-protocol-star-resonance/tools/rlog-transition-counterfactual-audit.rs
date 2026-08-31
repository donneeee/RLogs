use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    env,
    error::Error,
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{
    CanonicalEvent, DamageEvent, DamagePacketDetail, EntityAttributeUpdateKind,
    EntityAttributeValue, EntityRef, StatusEvent, StatusState, TimelineEventKind,
};
use rlogs_game_bpsr::decode_known_entity_attribute_value;
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 9;
const GENERATED_BY: &str = "rlogs-bpsr-rlog-transition-counterfactual-audit";
const DEFAULT_MAX_PAIR_GAP_MICROS: u64 = 2_000_000;
const RECENT_SAMPLES_PER_TARGET: usize = 128;
const EXAMPLE_LIMIT: usize = 32;
const CURRENT_HP_ATTRIBUTE_ID: i32 = 11_310;
const DIAGNOSTIC_EXCLUDED_ENTITY_ATTRIBUTE_IDS: [i32; 2] = [443, 474];
const POSITION_ATTRIBUTE_ID: i32 = 52;
const TARGET_POSITION_ATTRIBUTE_ID: i32 = 53;

#[derive(Debug)]
enum Command {
    Generate {
        build: String,
        gap_window_audit: PathBuf,
        effect_id: i64,
        damage_relationship: DamageRelationship,
        diagnostic_endpoint_attribute_ids: Vec<i32>,
        output: PathBuf,
        max_pair_gap_micros: u64,
    },
    Verify {
        input: PathBuf,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuditReport {
    schema_version: u16,
    generated_by: String,
    game_build: String,
    effect_id: i64,
    #[serde(default)]
    damage_relationship: DamageRelationship,
    policy: AuditPolicy,
    inputs: AuditInputs,
    search_contract: SearchContract,
    summary: AuditSummary,
    sessions: Vec<SessionAudit>,
    examples: Vec<PairExample>,
    same_context_mismatch_examples: Vec<SameContextMismatchExample>,
    blockers: Vec<String>,
    content_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuditPolicy {
    sealed_rlogs_are_streamed_one_event_at_a_time: bool,
    every_data_gap_pause_and_run_boundary_resets_all_observed_state: bool,
    only_same_segment_transition_adjacent_pairs_are_compared: bool,
    exact_numeric_ids_and_build_are_authoritative: bool,
    #[serde(default)]
    damage_relationship_is_explicit: bool,
    packet_absence_is_not_zero: bool,
    unknown_segment_baseline_statuses_are_preserved_as_unresolved: bool,
    target_current_hp_exclusion_is_diagnostic_only: bool,
    attribute_443_474_exclusion_is_diagnostic_only: bool,
    configured_endpoint_attribute_exclusion_is_diagnostic_only: bool,
    relative_spatial_relations_are_diagnostic_only: bool,
    structurally_unobservable_remote_player_packets_are_not_acquisition_requirements: bool,
    candidate_pairs_never_grant_formula_or_runtime_authority: bool,
    current_snapshots_are_never_backfilled_into_historical_segments: bool,
    formula_authority: bool,
    runtime_authority: bool,
    ui_display_authority: bool,
    provider_rdps_credit_allowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuditInputs {
    gap_window_audit: FileReceipt,
    source_rlogs: Vec<RlogReceipt>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FileReceipt {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RlogReceipt {
    path: String,
    bytes: u64,
    sha256: String,
    sealed_content_sha256: String,
    event_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SearchContract {
    max_pair_gap_micros: u64,
    recent_samples_per_target: usize,
    exact_context_dimensions: Vec<String>,
    exact_observed_state_dimensions: Vec<String>,
    excluded_output_dimensions: Vec<String>,
    strict_formula_pair_requirements_still_open: Vec<String>,
    remote_player_packet_dependency: bool,
    selected_effect_endpoint_role: String,
    diagnostic_endpoint_attribute_ids: Vec<i32>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DamageRelationship {
    Source,
    #[default]
    Target,
}

impl DamageRelationship {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "source" => Ok(Self::Source),
            "target" => Ok(Self::Target),
            _ => Err("--damage-relationship must be source or target".to_owned()),
        }
    }

    fn endpoint(self, damage: &DamageEvent) -> EntityRef {
        self.select(damage.source, damage.target)
    }

    fn select(self, source: EntityRef, target: EntityRef) -> EntityRef {
        match self {
            Self::Source => source,
            Self::Target => target,
        }
    }

    fn endpoint_role(self) -> &'static str {
        match self {
            Self::Source => "damage_actor",
            Self::Target => "damage_target",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct AuditSummary {
    source_rlog_count: usize,
    source_rlog_bytes: u64,
    canonical_event_count: u64,
    reset_boundary_count: u64,
    data_gap_count: u64,
    recorder_pause_count: u64,
    run_boundary_count: u64,
    damage_events: u64,
    damage_events_with_selected_effect_active: u64,
    damage_events_with_selected_effect_absent: u64,
    opposite_state_recent_comparisons: u64,
    same_normalized_damage_context_pairs: u64,
    same_context_and_observed_attribute_pairs: u64,
    same_context_and_nonselected_status_pairs: u64,
    same_context_pairs_with_only_target_current_hp_difference: u64,
    same_context_pairs_after_443_474_attribute_exclusion: u64,
    same_context_pairs_after_443_474_and_target_current_hp_exclusion: u64,
    same_context_pairs_after_443_474_and_target_current_hp_exclusion_with_equal_statuses: u64,
    same_context_pairs_after_configured_endpoint_443_474_and_target_current_hp_exclusion: u64,
    same_context_pairs_after_configured_endpoint_443_474_and_target_current_hp_exclusion_with_equal_statuses:
        u64,
    same_context_pairs_after_configured_endpoint_transition_and_diagnostic_exclusions_with_equal_statuses:
        u64,
    configured_endpoint_transition_pairs: u64,
    configured_endpoint_transition_action_identity_counts: BTreeMap<String, u64>,
    configured_endpoint_attribute_transition_counts: BTreeMap<i32, u64>,
    configured_endpoint_transition_source_residual_attribute_difference_counts: BTreeMap<i32, u64>,
    configured_endpoint_transition_target_residual_attribute_difference_counts: BTreeMap<i32, u64>,
    configured_endpoint_transition_source_temporary_attribute_difference_counts: BTreeMap<i32, u64>,
    configured_endpoint_transition_target_temporary_attribute_difference_counts: BTreeMap<i32, u64>,
    configured_endpoint_transition_source_status_difference_counts: BTreeMap<i64, u64>,
    configured_endpoint_transition_target_status_difference_counts: BTreeMap<i64, u64>,
    configured_endpoint_transition_residual_dimension_count_distribution: BTreeMap<usize, u64>,
    configured_endpoint_transition_pairs_with_attribute_snapshot_flag_difference: u64,
    configured_endpoint_transition_pairs_with_temporary_snapshot_flag_difference: u64,
    configured_endpoint_transition_spatial_relations: BTreeMap<String, SpatialRelationAudit>,
    minimum_residual_observed_state_dimensions_after_443_474_exclusion: Option<usize>,
    minimum_residual_observed_state_dimensions_after_443_474_and_target_current_hp_exclusion:
        Option<usize>,
    minimum_residual_observed_state_dimensions_after_configured_endpoint_443_474_and_target_current_hp_exclusion:
        Option<usize>,
    minimum_residual_observed_state_dimensions_among_configured_endpoint_transition_pairs:
        Option<usize>,
    same_context_source_attribute_difference_counts: BTreeMap<i32, u64>,
    same_context_target_attribute_difference_counts: BTreeMap<i32, u64>,
    same_context_source_temporary_attribute_difference_counts: BTreeMap<i32, u64>,
    same_context_target_temporary_attribute_difference_counts: BTreeMap<i32, u64>,
    same_context_source_status_difference_counts: BTreeMap<i64, u64>,
    same_context_target_status_difference_counts: BTreeMap<i64, u64>,
    exact_observed_input_candidate_pairs: u64,
    exact_observed_input_equal_output_pairs: u64,
    exact_observed_input_divergent_output_pairs: u64,
    candidate_pairs_with_complete_source_target_attribute_snapshots: u64,
    candidate_pairs_with_exact_selected_provider: u64,
    candidate_pairs_with_unresolved_segment_status_baseline: u64,
    target_current_hp_excluded_candidate_pairs: u64,
    target_current_hp_excluded_equal_output_pairs: u64,
    target_current_hp_excluded_divergent_output_pairs: u64,
    strict_controlled_counterfactual_pairs: u64,
    distinct_candidate_input_fingerprints: usize,
    maximum_recent_samples_retained: usize,
    exact_damage_projection_proven: bool,
    exact_operation_order_proven: bool,
    exact_integer_rounding_proven: bool,
    packet_conservation_proven: bool,
    formula_authority: bool,
    runtime_authority: bool,
    ui_display_authority: bool,
    provider_rdps_credit_allowed: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SessionAudit {
    path: String,
    session_id: String,
    event_count: u64,
    reset_boundary_count: u64,
    data_gap_count: u64,
    recorder_pause_count: u64,
    run_boundary_count: u64,
    damage_events: u64,
    damage_events_with_selected_effect_active: u64,
    damage_events_with_selected_effect_absent: u64,
    opposite_state_recent_comparisons: u64,
    same_normalized_damage_context_pairs: u64,
    same_context_and_observed_attribute_pairs: u64,
    same_context_and_nonselected_status_pairs: u64,
    same_context_pairs_with_only_target_current_hp_difference: u64,
    same_context_pairs_after_443_474_attribute_exclusion: u64,
    same_context_pairs_after_443_474_and_target_current_hp_exclusion: u64,
    same_context_pairs_after_443_474_and_target_current_hp_exclusion_with_equal_statuses: u64,
    same_context_pairs_after_configured_endpoint_443_474_and_target_current_hp_exclusion: u64,
    same_context_pairs_after_configured_endpoint_443_474_and_target_current_hp_exclusion_with_equal_statuses:
        u64,
    same_context_pairs_after_configured_endpoint_transition_and_diagnostic_exclusions_with_equal_statuses:
        u64,
    configured_endpoint_transition_pairs: u64,
    configured_endpoint_transition_action_identity_counts: BTreeMap<String, u64>,
    configured_endpoint_attribute_transition_counts: BTreeMap<i32, u64>,
    configured_endpoint_transition_source_residual_attribute_difference_counts: BTreeMap<i32, u64>,
    configured_endpoint_transition_target_residual_attribute_difference_counts: BTreeMap<i32, u64>,
    configured_endpoint_transition_source_temporary_attribute_difference_counts: BTreeMap<i32, u64>,
    configured_endpoint_transition_target_temporary_attribute_difference_counts: BTreeMap<i32, u64>,
    configured_endpoint_transition_source_status_difference_counts: BTreeMap<i64, u64>,
    configured_endpoint_transition_target_status_difference_counts: BTreeMap<i64, u64>,
    configured_endpoint_transition_residual_dimension_count_distribution: BTreeMap<usize, u64>,
    configured_endpoint_transition_pairs_with_attribute_snapshot_flag_difference: u64,
    configured_endpoint_transition_pairs_with_temporary_snapshot_flag_difference: u64,
    configured_endpoint_transition_spatial_relations: BTreeMap<String, SpatialRelationAudit>,
    minimum_residual_observed_state_dimensions_after_443_474_exclusion: Option<usize>,
    minimum_residual_observed_state_dimensions_after_443_474_and_target_current_hp_exclusion:
        Option<usize>,
    minimum_residual_observed_state_dimensions_after_configured_endpoint_443_474_and_target_current_hp_exclusion:
        Option<usize>,
    minimum_residual_observed_state_dimensions_among_configured_endpoint_transition_pairs:
        Option<usize>,
    same_context_source_attribute_difference_counts: BTreeMap<i32, u64>,
    same_context_target_attribute_difference_counts: BTreeMap<i32, u64>,
    same_context_source_temporary_attribute_difference_counts: BTreeMap<i32, u64>,
    same_context_target_temporary_attribute_difference_counts: BTreeMap<i32, u64>,
    same_context_source_status_difference_counts: BTreeMap<i64, u64>,
    same_context_target_status_difference_counts: BTreeMap<i64, u64>,
    exact_observed_input_candidate_pairs: u64,
    exact_observed_input_equal_output_pairs: u64,
    exact_observed_input_divergent_output_pairs: u64,
    candidate_pairs_with_complete_source_target_attribute_snapshots: u64,
    candidate_pairs_with_exact_selected_provider: u64,
    candidate_pairs_with_unresolved_segment_status_baseline: u64,
    target_current_hp_excluded_candidate_pairs: u64,
    target_current_hp_excluded_equal_output_pairs: u64,
    target_current_hp_excluded_divergent_output_pairs: u64,
    strict_controlled_counterfactual_pairs: u64,
    distinct_candidate_input_fingerprints: usize,
    maximum_recent_samples_retained: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct SpatialRelationAudit {
    pair_count: u64,
    prior_relation_complete_count: u64,
    sample_relation_complete_count: u64,
    both_relations_complete_count: u64,
    exact_displacement_vector_equal_count: u64,
    exact_squared_distance_equal_count: u64,
    absolute_distance_delta_le_0_000001_count: u64,
    absolute_distance_delta_le_0_0001_count: u64,
    absolute_distance_delta_le_0_01_count: u64,
    absolute_distance_delta_le_0_1_count: u64,
    absolute_distance_delta_le_1_count: u64,
    maximum_absolute_distance_delta_raw_coordinate_units: Option<f64>,
    spatial_state_safe_to_exclude_from_counterfactual_matching: bool,
    formula_authority: bool,
}

#[derive(Debug, Clone, Copy)]
struct Position3 {
    x: f64,
    y: f64,
    z: f64,
}

#[derive(Debug, Clone, Copy)]
enum PositionSlot {
    SourcePosition,
    SourceTargetPosition,
    TargetPosition,
    TargetTargetPosition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PairExample {
    comparison_mode: String,
    rlog: String,
    session_id: String,
    segment_index: u64,
    input_fingerprint_sha256: String,
    source_actor_id: u64,
    source_entity_uuid: i64,
    target_actor_id: u64,
    target_entity_uuid: i64,
    ability_id: Option<i64>,
    action_identity: String,
    present_sequence: u64,
    absent_sequence: u64,
    pair_gap_micros: u64,
    present_amount: i64,
    absent_amount: i64,
    outputs_equal: bool,
    source_target_attribute_snapshots_complete: bool,
    selected_provider_exact: bool,
    segment_status_baseline_complete: bool,
    controlled_counterfactual_pair_proven: bool,
    formula_authority: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SameContextMismatchExample {
    rlog: String,
    session_id: String,
    segment_index: u64,
    present_sequence: u64,
    absent_sequence: u64,
    pair_gap_micros: u64,
    source_actor_id: u64,
    source_entity_uuid: i64,
    target_actor_id: u64,
    target_entity_uuid: i64,
    ability_id: Option<i64>,
    action_identity: String,
    present_amount: i64,
    absent_amount: i64,
    present_normal_value: Option<i64>,
    absent_normal_value: Option<i64>,
    source_attribute_ids: Vec<i32>,
    target_attribute_ids: Vec<i32>,
    source_temporary_attribute_ids: Vec<i32>,
    target_temporary_attribute_ids: Vec<i32>,
    source_status_effect_ids: Vec<i64>,
    target_status_effect_ids: Vec<i64>,
    only_target_current_hp_differs: bool,
    residual_observed_state_dimensions_after_443_474_exclusion: usize,
    residual_observed_state_dimensions_after_443_474_and_target_current_hp_exclusion: usize,
    configured_endpoint_attribute_ids_that_differ: Vec<i32>,
    residual_observed_state_dimensions_after_configured_endpoint_443_474_and_target_current_hp_exclusion:
        usize,
    source_target_attribute_snapshots_complete: bool,
    selected_provider_exact: bool,
    segment_status_baseline_complete: bool,
    controlled_counterfactual_pair_proven: bool,
    formula_authority: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct GapWindowAudit {
    schema_version: u16,
    generated_by: String,
    game_build: String,
    effect_id: i64,
    #[serde(default)]
    damage_relationship: DamageRelationship,
    policy: GapWindowPolicy,
    sessions: Vec<GapSession>,
}

#[derive(Debug, Clone, Deserialize)]
struct GapWindowPolicy {
    #[serde(default)]
    damage_relationship_is_explicit: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct GapSession {
    path: String,
    bytes: u64,
    sealed_content_sha256: String,
    event_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
struct EntityState {
    attributes: BTreeMap<i32, Vec<u8>>,
    temporary_attributes: BTreeMap<i32, i32>,
    attribute_snapshot_after_boundary: bool,
    temporary_snapshot_after_boundary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct StatusKey {
    effect_id: i64,
    instance_id: Option<i64>,
    source_actor_id: Option<u64>,
    source_entity_uuid: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct StatusValue {
    stacks: Option<u32>,
    level: Option<i32>,
    part_id: Option<i32>,
    count: Option<i32>,
    origin_source_type_id: Option<i32>,
    origin_source_config_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
struct ControlInputs {
    session_id: String,
    segment_index: u64,
    source_actor_id: u64,
    source_entity_uuid: i64,
    direct_source_actor_id: Option<u64>,
    direct_source_entity_uuid: Option<i64>,
    target_actor_id: u64,
    target_entity_uuid: i64,
    ability_id: Option<i64>,
    action_identity: String,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    critical: Option<bool>,
    lucky: Option<bool>,
    causes_lucky: Option<bool>,
    blocked: Option<bool>,
    periodic: Option<bool>,
    packet_inputs: DamagePacketDetail,
    source_state: EntityState,
    target_state: EntityState,
    source_statuses_without_selected_effect: Vec<(StatusKey, StatusValue)>,
    target_statuses_without_selected_effect: Vec<(StatusKey, StatusValue)>,
}

#[derive(Debug, Serialize)]
struct ContextInputs<'a> {
    session_id: &'a str,
    segment_index: u64,
    source_actor_id: u64,
    source_entity_uuid: i64,
    direct_source_actor_id: Option<u64>,
    direct_source_entity_uuid: Option<i64>,
    target_actor_id: u64,
    target_entity_uuid: i64,
    ability_id: Option<i64>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    critical: Option<bool>,
    lucky: Option<bool>,
    causes_lucky: Option<bool>,
    blocked: Option<bool>,
    periodic: Option<bool>,
    packet_inputs: &'a DamagePacketDetail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DamageOutput {
    amount: i64,
    actual_amount: Option<i64>,
    hp_loss: Option<i64>,
    shield_loss: Option<i64>,
    normal_value: Option<i64>,
    lucky_value: Option<i64>,
}

#[derive(Debug, Clone)]
struct RecentDamage {
    segment_index: u64,
    sequence: u64,
    observed_micros: u64,
    selected_effect_active: bool,
    selected_provider: Option<EntityRef>,
    selected_instance_count: usize,
    input_fingerprint: [u8; 32],
    current_hp_excluded_input_fingerprint: [u8; 32],
    normalized_context_fingerprint: [u8; 32],
    observed_attribute_fingerprint: [u8; 32],
    nonselected_status_fingerprint: [u8; 32],
    source_state: EntityState,
    target_state: EntityState,
    source_statuses: Vec<(StatusKey, StatusValue)>,
    target_statuses: Vec<(StatusKey, StatusValue)>,
    source: EntityRef,
    target: EntityRef,
    ability_id: Option<i64>,
    action_identity: String,
    source_target_attribute_snapshots_complete: bool,
    output: DamageOutput,
}

#[derive(Debug, Default)]
struct ScanState {
    segment_index: u64,
    entities: HashMap<(u64, i64), EntityState>,
    statuses: HashMap<(u64, i64), BTreeMap<StatusKey, StatusValue>>,
    active_selected: HashMap<(u64, i64, i64), Option<EntityRef>>,
    recent_by_target: HashMap<(u64, i64), VecDeque<RecentDamage>>,
    candidate_fingerprints: BTreeMap<String, u64>,
    examples: Vec<PairExample>,
    mismatch_examples: Vec<SameContextMismatchExample>,
    session: SessionAudit,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("RLOG transition counterfactual audit failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    match arguments()? {
        Command::Generate {
            build,
            gap_window_audit,
            effect_id,
            damage_relationship,
            diagnostic_endpoint_attribute_ids,
            output,
            max_pair_gap_micros,
        } => generate(
            &build,
            &gap_window_audit,
            effect_id,
            damage_relationship,
            &diagnostic_endpoint_attribute_ids,
            max_pair_gap_micros,
            &output,
        ),
        Command::Verify { input } => {
            let report: AuditReport = serde_json::from_reader(BufReader::new(File::open(&input)?))?;
            verify_report(&report)?;
            verify_input_receipts(&report)?;
            println!(
                "RLOG transition counterfactual audit verified for build {} effect {}: {} exact observed-input candidates, {} strict controlled pairs; formula authority=false.",
                report.game_build,
                report.effect_id,
                report.summary.exact_observed_input_candidate_pairs,
                report.summary.strict_controlled_counterfactual_pairs,
            );
            Ok(())
        }
    }
}

fn generate(
    build: &str,
    gap_window_audit_path: &Path,
    effect_id: i64,
    damage_relationship: DamageRelationship,
    diagnostic_endpoint_attribute_ids: &[i32],
    max_pair_gap_micros: u64,
    output: &Path,
) -> Result<(), Box<dyn Error>> {
    if build.is_empty()
        || !build.bytes().all(|value| value.is_ascii_digit())
        || effect_id <= 0
        || diagnostic_endpoint_attribute_ids.is_empty()
        || diagnostic_endpoint_attribute_ids.iter().any(|id| *id <= 0)
        || max_pair_gap_micros == 0
    {
        return Err("build, effect ID, or maximum pair gap is invalid".into());
    }
    let gap_value: Value =
        serde_json::from_reader(BufReader::new(File::open(gap_window_audit_path)?))?;
    verify_embedded_digest(&gap_value, "RLOG gap-window audit")?;
    let gap_audit: GapWindowAudit = serde_json::from_value(gap_value)?;
    if gap_audit.schema_version != 3
        || gap_audit.generated_by != "rlogs-bpsr-rlog-gap-window-audit"
        || gap_audit.game_build != build
        || gap_audit.effect_id != effect_id
        || gap_audit.damage_relationship != damage_relationship
        || !gap_audit.policy.damage_relationship_is_explicit
        || gap_audit.sessions.is_empty()
    {
        return Err(
            "gap-window audit identity does not match the requested exact build/effect".into(),
        );
    }
    let gap_receipt = file_receipt(gap_window_audit_path)?;
    let mut sessions = Vec::with_capacity(gap_audit.sessions.len());
    let mut rlog_receipts = Vec::with_capacity(gap_audit.sessions.len());
    let mut examples = Vec::new();
    let mut mismatch_examples = Vec::new();
    for expected in &gap_audit.sessions {
        let path = PathBuf::from(&expected.path);
        if fs::metadata(&path)?.len() != expected.bytes {
            return Err(format!("source RLOG byte length changed: {}", path.display()).into());
        }
        let (session, mut session_examples, mut session_mismatch_examples, sealed_content_sha256) =
            audit_rlog(
                &path,
                effect_id,
                damage_relationship,
                diagnostic_endpoint_attribute_ids,
                max_pair_gap_micros,
            )?;
        if session.event_count != expected.event_count
            || sealed_content_sha256 != expected.sealed_content_sha256
        {
            return Err(format!("source RLOG seal changed: {}", path.display()).into());
        }
        rlog_receipts.push(RlogReceipt {
            path: display_path(&path),
            bytes: expected.bytes,
            sha256: sha256_file(&path)?,
            sealed_content_sha256,
            event_count: expected.event_count,
        });
        examples.append(&mut session_examples);
        mismatch_examples.append(&mut session_mismatch_examples);
        sessions.push(session);
    }
    examples.sort_by(|left, right| {
        left.rlog
            .cmp(&right.rlog)
            .then_with(|| left.absent_sequence.cmp(&right.absent_sequence))
            .then_with(|| left.present_sequence.cmp(&right.present_sequence))
    });
    examples.truncate(EXAMPLE_LIMIT);
    mismatch_examples.sort_by(|left, right| {
        left.residual_observed_state_dimensions_after_configured_endpoint_443_474_and_target_current_hp_exclusion
            .cmp(&right.residual_observed_state_dimensions_after_configured_endpoint_443_474_and_target_current_hp_exclusion)
            .then_with(|| left.rlog
            .cmp(&right.rlog)
            .then_with(|| left.absent_sequence.cmp(&right.absent_sequence)))
    });
    mismatch_examples.truncate(EXAMPLE_LIMIT);
    let summary = summarize(&sessions, &rlog_receipts);
    let mut report = AuditReport {
        schema_version: SCHEMA_VERSION,
        generated_by: GENERATED_BY.to_owned(),
        game_build: build.to_owned(),
        effect_id,
        damage_relationship,
        policy: AuditPolicy {
            sealed_rlogs_are_streamed_one_event_at_a_time: true,
            every_data_gap_pause_and_run_boundary_resets_all_observed_state: true,
            only_same_segment_transition_adjacent_pairs_are_compared: true,
            exact_numeric_ids_and_build_are_authoritative: true,
            damage_relationship_is_explicit: true,
            packet_absence_is_not_zero: true,
            unknown_segment_baseline_statuses_are_preserved_as_unresolved: true,
            target_current_hp_exclusion_is_diagnostic_only: true,
            attribute_443_474_exclusion_is_diagnostic_only: true,
            configured_endpoint_attribute_exclusion_is_diagnostic_only: true,
            relative_spatial_relations_are_diagnostic_only: true,
            structurally_unobservable_remote_player_packets_are_not_acquisition_requirements: true,
            candidate_pairs_never_grant_formula_or_runtime_authority: true,
            current_snapshots_are_never_backfilled_into_historical_segments: true,
            formula_authority: false,
            runtime_authority: false,
            ui_display_authority: false,
            provider_rdps_credit_allowed: false,
        },
        inputs: AuditInputs {
            gap_window_audit: gap_receipt,
            source_rlogs: rlog_receipts,
        },
        search_contract: SearchContract {
            max_pair_gap_micros,
            recent_samples_per_target: RECENT_SAMPLES_PER_TARGET,
            exact_context_dimensions: vec![
                "same RLOG and reset-bounded segment".to_owned(),
                "source, direct source, target, ability, hit event, damage source, and damage type"
                    .to_owned(),
                "critical, lucky, causes-lucky, blocked, and periodic flags".to_owned(),
                "all packet inputs after removing result/output fields".to_owned(),
            ],
            exact_observed_state_dimensions: vec![
                "source and target entity attribute raw bytes".to_owned(),
                "six decoded relative relations among source/target AttrPos 52 and AttrTargetPos 53; equality and tolerance counts are diagnostic only"
                    .to_owned(),
                "source and target temporary attribute values".to_owned(),
                "source and target observed status state excluding only the selected target effect"
                    .to_owned(),
            ],
            // This diagnostic mirrors the existing counterfactual frontier but
            // never changes the strict exact-input result.
            excluded_output_dimensions: vec![
                "amount, actual amount, HP loss, shield loss, normal value, lucky value".to_owned(),
                "skill-effect result UUID, total, and group index".to_owned(),
                "per-hit-part damage values".to_owned(),
            ],
            strict_formula_pair_requirements_still_open: vec![
                "complete target/source/provider status baseline at the segment start".to_owned(),
                "proof that no hidden damage-stage input changed".to_owned(),
                "exact operation order and integer rounding".to_owned(),
                "canonical party-wide conservation replay".to_owned(),
            ],
            remote_player_packet_dependency: false,
            selected_effect_endpoint_role: damage_relationship.endpoint_role().to_owned(),
            diagnostic_endpoint_attribute_ids: diagnostic_endpoint_attribute_ids.to_vec(),
        },
        summary,
        sessions,
        examples,
        same_context_mismatch_examples: mismatch_examples,
        blockers: vec![
            "segment-start status baselines remain unknown after every capture gap or run reset"
                .to_owned(),
            "exact observed-input candidates are acquisition leads, not controlled formula proof"
                .to_owned(),
            "diagnostically excluding AttrStunned 443 and AttrHateList 474 does not grant semantic or formula exclusion authority"
                .to_owned(),
            "diagnostically excluding configured endpoint attributes does not grant formula-input or operation-order authority"
                .to_owned(),
            "relative AttrPos and AttrTargetPos equality or tolerance counts are diagnostic and do not prove spatial damage equivalence"
                .to_owned(),
            "hidden damage-stage inputs, operation order, and integer rounding remain unproven"
                .to_owned(),
            "party-wide packet conservation remains unproven".to_owned(),
        ],
        content_sha256: String::new(),
    };
    report.content_sha256 = report_digest(&report)?;
    verify_report(&report)?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(output)?);
    serde_json::to_writer_pretty(&mut writer, &report)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    println!(
        "Audited {} sealed RLOGs: {} exact observed-input candidate pairs, {} divergent, {} strict controlled; formula authority=false.",
        report.summary.source_rlog_count,
        report.summary.exact_observed_input_candidate_pairs,
        report.summary.exact_observed_input_divergent_output_pairs,
        report.summary.strict_controlled_counterfactual_pairs,
    );
    println!("wrote {}", output.display());
    Ok(())
}

fn audit_rlog(
    path: &Path,
    effect_id: i64,
    damage_relationship: DamageRelationship,
    diagnostic_endpoint_attribute_ids: &[i32],
    max_pair_gap_micros: u64,
) -> Result<
    (
        SessionAudit,
        Vec<PairExample>,
        Vec<SameContextMismatchExample>,
        String,
    ),
    Box<dyn Error>,
> {
    let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
    let session_id = reader.header().session_id.clone();
    let mut state = ScanState::default();
    state.session.path = display_path(path);
    state.session.session_id = session_id.clone();
    while let Some(envelope) = reader.next_event()? {
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            continue;
        };
        match &timeline.kind {
            TimelineEventKind::DataGap(_) => {
                state.session.data_gap_count = state.session.data_gap_count.saturating_add(1);
                reset_observed_state(&mut state);
            }
            TimelineEventKind::RecorderPause(_) => {
                state.session.recorder_pause_count =
                    state.session.recorder_pause_count.saturating_add(1);
                reset_observed_state(&mut state);
            }
            TimelineEventKind::RunBoundary { .. } => {
                state.session.run_boundary_count =
                    state.session.run_boundary_count.saturating_add(1);
                reset_observed_state(&mut state);
            }
            TimelineEventKind::EntityAttributes(event) => {
                let entity = state.entities.entry(entity_key(event.actor)).or_default();
                match event.update_kind {
                    EntityAttributeUpdateKind::Snapshot => {
                        entity.attributes.clear();
                        entity.attribute_snapshot_after_boundary = true;
                    }
                    EntityAttributeUpdateKind::Unknown => {
                        entity.attribute_snapshot_after_boundary = false;
                    }
                    EntityAttributeUpdateKind::Delta => {}
                }
                for attribute in &event.attributes {
                    entity
                        .attributes
                        .insert(attribute.attribute_id, attribute.raw_value.clone());
                }
            }
            TimelineEventKind::TemporaryAttributes(event) => {
                let entity = state.entities.entry(entity_key(event.actor)).or_default();
                match event.update_kind {
                    EntityAttributeUpdateKind::Snapshot => {
                        entity.temporary_attributes.clear();
                        entity.temporary_snapshot_after_boundary = true;
                    }
                    EntityAttributeUpdateKind::Unknown => {
                        entity.temporary_snapshot_after_boundary = false;
                    }
                    EntityAttributeUpdateKind::Delta => {}
                }
                for attribute in &event.attributes {
                    entity
                        .temporary_attributes
                        .insert(attribute.id, attribute.value);
                }
            }
            TimelineEventKind::Status(status) => {
                observe_status(&mut state, status, effect_id);
            }
            TimelineEventKind::Damage(damage) => {
                observe_damage(
                    &mut state,
                    path,
                    &session_id,
                    envelope.sequence,
                    envelope.time.observed_micros,
                    damage,
                    effect_id,
                    damage_relationship,
                    diagnostic_endpoint_attribute_ids,
                    max_pair_gap_micros,
                )?;
            }
            _ => {}
        }
    }
    let replay = reader
        .summary()
        .ok_or("sealed RLOG replay summary is missing")?;
    state.session.event_count = replay.event_count;
    state.session.distinct_candidate_input_fingerprints = state.candidate_fingerprints.len();
    Ok((
        state.session,
        state.examples,
        state.mismatch_examples,
        replay.content_sha256.clone(),
    ))
}

fn reset_observed_state(state: &mut ScanState) {
    state.session.reset_boundary_count = state.session.reset_boundary_count.saturating_add(1);
    state.segment_index = state.segment_index.saturating_add(1);
    state.entities.clear();
    state.statuses.clear();
    state.active_selected.clear();
    state.recent_by_target.clear();
}

fn observe_status(state: &mut ScanState, status: &StatusEvent, effect_id: i64) {
    let key = StatusKey {
        effect_id: status.effect.0,
        instance_id: status.instance_id.map(|value| value.0),
        source_actor_id: status.source.map(|value| value.actor_id.0),
        source_entity_uuid: status.source.map(|value| value.entity_uuid.0),
    };
    let values = state.statuses.entry(entity_key(status.target)).or_default();
    match status.state {
        StatusState::Applied | StatusState::Refreshed | StatusState::Stacked => {
            values.insert(
                key.clone(),
                StatusValue {
                    stacks: status.stacks,
                    level: status.level,
                    part_id: status.part_id,
                    count: status.count,
                    origin_source_type_id: status.origin.map(|value| value.source_type_id),
                    origin_source_config_id: status.origin.map(|value| value.source_config_id),
                },
            );
        }
        StatusState::Consumed | StatusState::Removed => {
            values.remove(&key);
        }
    }
    if status.effect.0 != effect_id {
        return;
    }
    let Some(instance_id) = status.instance_id else {
        return;
    };
    let selected_key = (
        status.target.actor_id.0,
        status.target.entity_uuid.0,
        instance_id.0,
    );
    match status.state {
        StatusState::Applied | StatusState::Refreshed | StatusState::Stacked => {
            state.active_selected.insert(selected_key, status.source);
        }
        StatusState::Consumed | StatusState::Removed => {
            state.active_selected.remove(&selected_key);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn observe_damage(
    state: &mut ScanState,
    path: &Path,
    session_id: &str,
    sequence: u64,
    observed_micros: u64,
    damage: &DamageEvent,
    effect_id: i64,
    damage_relationship: DamageRelationship,
    diagnostic_endpoint_attribute_ids: &[i32],
    max_pair_gap_micros: u64,
) -> Result<(), Box<dyn Error>> {
    state.session.damage_events = state.session.damage_events.saturating_add(1);
    let selected_endpoint = damage_relationship.endpoint(damage);
    let selected = state
        .active_selected
        .iter()
        .filter(|((actor_id, entity_uuid, _), _)| {
            *actor_id == selected_endpoint.actor_id.0
                && *entity_uuid == selected_endpoint.entity_uuid.0
        })
        .map(|(_, provider)| *provider)
        .collect::<Vec<_>>();
    let selected_effect_active = !selected.is_empty();
    if selected_effect_active {
        state.session.damage_events_with_selected_effect_active = state
            .session
            .damage_events_with_selected_effect_active
            .saturating_add(1);
    } else {
        state.session.damage_events_with_selected_effect_absent = state
            .session
            .damage_events_with_selected_effect_absent
            .saturating_add(1);
    }
    let selected_provider = if selected.len() == 1 {
        selected[0]
    } else {
        None
    };
    let source_state = state
        .entities
        .get(&entity_key(damage.source))
        .cloned()
        .unwrap_or_default();
    let target_state = state
        .entities
        .get(&entity_key(damage.target))
        .cloned()
        .unwrap_or_default();
    let snapshots_complete = source_state.attribute_snapshot_after_boundary
        && target_state.attribute_snapshot_after_boundary;
    let action_identity = damage_action_identity(damage);
    let input = ControlInputs {
        session_id: session_id.to_owned(),
        segment_index: state.segment_index,
        source_actor_id: damage.source.actor_id.0,
        source_entity_uuid: damage.source.entity_uuid.0,
        direct_source_actor_id: damage.direct_source.map(|value| value.actor_id.0),
        direct_source_entity_uuid: damage.direct_source.map(|value| value.entity_uuid.0),
        target_actor_id: damage.target.actor_id.0,
        target_entity_uuid: damage.target.entity_uuid.0,
        ability_id: damage.ability.map(|value| value.0),
        action_identity: action_identity.clone(),
        hit_event_id: damage.hit_event_id,
        damage_source: damage.damage_source,
        damage_type: damage.damage_type,
        critical: damage.flags.critical,
        lucky: damage.flags.lucky,
        causes_lucky: damage.flags.causes_lucky,
        blocked: damage.flags.blocked,
        periodic: damage.flags.periodic,
        packet_inputs: normalized_packet_inputs(&damage.packet),
        source_state,
        target_state,
        source_statuses_without_selected_effect: statuses_without_effect(
            state.statuses.get(&entity_key(damage.source)),
            effect_id,
        ),
        target_statuses_without_selected_effect: statuses_without_effect(
            state.statuses.get(&entity_key(damage.target)),
            effect_id,
        ),
    };
    let fingerprint = sha256_serialized(&input)?;
    let mut current_hp_excluded_input = input.clone();
    current_hp_excluded_input
        .target_state
        .attributes
        .remove(&CURRENT_HP_ATTRIBUTE_ID);
    let current_hp_excluded_input_fingerprint = sha256_serialized(&current_hp_excluded_input)?;
    let normalized_context_fingerprint = sha256_serialized(&ContextInputs {
        session_id: &input.session_id,
        segment_index: input.segment_index,
        source_actor_id: input.source_actor_id,
        source_entity_uuid: input.source_entity_uuid,
        direct_source_actor_id: input.direct_source_actor_id,
        direct_source_entity_uuid: input.direct_source_entity_uuid,
        target_actor_id: input.target_actor_id,
        target_entity_uuid: input.target_entity_uuid,
        ability_id: input.ability_id,
        hit_event_id: input.hit_event_id,
        damage_source: input.damage_source,
        damage_type: input.damage_type,
        critical: input.critical,
        lucky: input.lucky,
        causes_lucky: input.causes_lucky,
        blocked: input.blocked,
        periodic: input.periodic,
        packet_inputs: &input.packet_inputs,
    })?;
    let observed_attribute_fingerprint =
        sha256_serialized(&(&input.source_state, &input.target_state))?;
    let nonselected_status_fingerprint = sha256_serialized(&(
        &input.source_statuses_without_selected_effect,
        &input.target_statuses_without_selected_effect,
    ))?;
    let sample = RecentDamage {
        segment_index: state.segment_index,
        sequence,
        observed_micros,
        selected_effect_active,
        selected_provider,
        selected_instance_count: selected.len(),
        input_fingerprint: fingerprint,
        current_hp_excluded_input_fingerprint,
        normalized_context_fingerprint,
        observed_attribute_fingerprint,
        nonselected_status_fingerprint,
        source_state: input.source_state.clone(),
        target_state: input.target_state.clone(),
        source_statuses: input.source_statuses_without_selected_effect.clone(),
        target_statuses: input.target_statuses_without_selected_effect.clone(),
        source: damage.source,
        target: damage.target,
        ability_id: damage.ability.map(|value| value.0),
        action_identity,
        source_target_attribute_snapshots_complete: snapshots_complete,
        output: DamageOutput {
            amount: damage.amount,
            actual_amount: damage.actual_amount,
            hp_loss: damage.hp_loss,
            shield_loss: damage.shield_loss,
            normal_value: damage.packet.normal_value,
            lucky_value: damage.packet.lucky_value,
        },
    };
    let recent = state
        .recent_by_target
        .entry(entity_key(damage.target))
        .or_default();
    while recent.front().is_some_and(|prior| {
        observed_micros.saturating_sub(prior.observed_micros) > max_pair_gap_micros
    }) {
        recent.pop_front();
    }
    for prior in recent.iter().rev() {
        if prior.selected_effect_active == sample.selected_effect_active {
            continue;
        }
        state.session.opposite_state_recent_comparisons = state
            .session
            .opposite_state_recent_comparisons
            .saturating_add(1);
        if prior.segment_index != sample.segment_index {
            continue;
        }
        let same_context =
            prior.normalized_context_fingerprint == sample.normalized_context_fingerprint;
        let same_attributes =
            prior.observed_attribute_fingerprint == sample.observed_attribute_fingerprint;
        let same_statuses =
            prior.nonselected_status_fingerprint == sample.nonselected_status_fingerprint;
        if same_context {
            state.session.same_normalized_damage_context_pairs = state
                .session
                .same_normalized_damage_context_pairs
                .saturating_add(1);
            observe_same_context_mismatch(
                &mut state.session,
                &mut state.mismatch_examples,
                path,
                session_id,
                prior,
                &sample,
                damage_relationship,
                diagnostic_endpoint_attribute_ids,
            );
            if same_attributes {
                state.session.same_context_and_observed_attribute_pairs = state
                    .session
                    .same_context_and_observed_attribute_pairs
                    .saturating_add(1);
            }
            if same_statuses {
                state.session.same_context_and_nonselected_status_pairs = state
                    .session
                    .same_context_and_nonselected_status_pairs
                    .saturating_add(1);
            }
        }
        let exact_match = prior.input_fingerprint == sample.input_fingerprint;
        let current_hp_excluded_match = prior.current_hp_excluded_input_fingerprint
            == sample.current_hp_excluded_input_fingerprint;
        if !exact_match && !current_hp_excluded_match {
            continue;
        }
        let outputs_equal = prior.output == sample.output;
        if current_hp_excluded_match {
            state.session.target_current_hp_excluded_candidate_pairs = state
                .session
                .target_current_hp_excluded_candidate_pairs
                .saturating_add(1);
            if outputs_equal {
                state.session.target_current_hp_excluded_equal_output_pairs = state
                    .session
                    .target_current_hp_excluded_equal_output_pairs
                    .saturating_add(1);
            } else {
                state
                    .session
                    .target_current_hp_excluded_divergent_output_pairs = state
                    .session
                    .target_current_hp_excluded_divergent_output_pairs
                    .saturating_add(1);
            }
        }
        let snapshots_complete = prior.source_target_attribute_snapshots_complete
            && sample.source_target_attribute_snapshots_complete;
        let present = if sample.selected_effect_active {
            &sample
        } else {
            prior
        };
        let selected_provider_exact =
            present.selected_instance_count == 1 && present.selected_provider.is_some();
        if exact_match {
            state.session.exact_observed_input_candidate_pairs = state
                .session
                .exact_observed_input_candidate_pairs
                .saturating_add(1);
            if outputs_equal {
                state.session.exact_observed_input_equal_output_pairs = state
                    .session
                    .exact_observed_input_equal_output_pairs
                    .saturating_add(1);
            } else {
                state.session.exact_observed_input_divergent_output_pairs = state
                    .session
                    .exact_observed_input_divergent_output_pairs
                    .saturating_add(1);
            }
            if snapshots_complete {
                state
                    .session
                    .candidate_pairs_with_complete_source_target_attribute_snapshots = state
                    .session
                    .candidate_pairs_with_complete_source_target_attribute_snapshots
                    .saturating_add(1);
            }
            if selected_provider_exact {
                state.session.candidate_pairs_with_exact_selected_provider = state
                    .session
                    .candidate_pairs_with_exact_selected_provider
                    .saturating_add(1);
            }
            state
                .session
                .candidate_pairs_with_unresolved_segment_status_baseline = state
                .session
                .candidate_pairs_with_unresolved_segment_status_baseline
                .saturating_add(1);
            let fingerprint_text = hex_sha256(&sample.input_fingerprint);
            *state
                .candidate_fingerprints
                .entry(fingerprint_text)
                .or_default() += 1;
        }
        if state.examples.len() < EXAMPLE_LIMIT {
            let (present, absent) = if sample.selected_effect_active {
                (&sample, prior)
            } else {
                (prior, &sample)
            };
            let (comparison_mode, fingerprint_text) = if exact_match {
                (
                    "strict_exact_observed_inputs",
                    hex_sha256(&sample.input_fingerprint),
                )
            } else {
                (
                    "target_current_hp_excluded_diagnostic",
                    hex_sha256(&sample.current_hp_excluded_input_fingerprint),
                )
            };
            state.examples.push(PairExample {
                comparison_mode: comparison_mode.to_owned(),
                rlog: display_path(path),
                session_id: session_id.to_owned(),
                segment_index: sample.segment_index,
                input_fingerprint_sha256: fingerprint_text,
                source_actor_id: sample.source.actor_id.0,
                source_entity_uuid: sample.source.entity_uuid.0,
                target_actor_id: sample.target.actor_id.0,
                target_entity_uuid: sample.target.entity_uuid.0,
                ability_id: sample.ability_id,
                action_identity: sample.action_identity.clone(),
                present_sequence: present.sequence,
                absent_sequence: absent.sequence,
                pair_gap_micros: sample.observed_micros.saturating_sub(prior.observed_micros),
                present_amount: present.output.amount,
                absent_amount: absent.output.amount,
                outputs_equal,
                source_target_attribute_snapshots_complete: snapshots_complete,
                selected_provider_exact,
                segment_status_baseline_complete: false,
                controlled_counterfactual_pair_proven: false,
                formula_authority: false,
            });
        }
    }
    recent.push_back(sample);
    while recent.len() > RECENT_SAMPLES_PER_TARGET {
        recent.pop_front();
    }
    let retained = state
        .recent_by_target
        .values()
        .map(VecDeque::len)
        .sum::<usize>();
    state.session.maximum_recent_samples_retained =
        state.session.maximum_recent_samples_retained.max(retained);
    Ok(())
}

fn observe_same_context_mismatch(
    session: &mut SessionAudit,
    examples: &mut Vec<SameContextMismatchExample>,
    path: &Path,
    session_id: &str,
    prior: &RecentDamage,
    sample: &RecentDamage,
    damage_relationship: DamageRelationship,
    diagnostic_endpoint_attribute_ids: &[i32],
) {
    let source_attributes = differing_map_keys(
        &prior.source_state.attributes,
        &sample.source_state.attributes,
    );
    let target_attributes = differing_map_keys(
        &prior.target_state.attributes,
        &sample.target_state.attributes,
    );
    let source_temporary = differing_map_keys(
        &prior.source_state.temporary_attributes,
        &sample.source_state.temporary_attributes,
    );
    let target_temporary = differing_map_keys(
        &prior.target_state.temporary_attributes,
        &sample.target_state.temporary_attributes,
    );
    let source_statuses =
        differing_status_effect_ids(&prior.source_statuses, &sample.source_statuses);
    let target_statuses =
        differing_status_effect_ids(&prior.target_statuses, &sample.target_statuses);
    let source_attributes_after_443_474 = source_attributes
        .iter()
        .copied()
        .filter(|id| !DIAGNOSTIC_EXCLUDED_ENTITY_ATTRIBUTE_IDS.contains(id))
        .collect::<Vec<_>>();
    let target_attributes_after_443_474 = target_attributes
        .iter()
        .copied()
        .filter(|id| !DIAGNOSTIC_EXCLUDED_ENTITY_ATTRIBUTE_IDS.contains(id))
        .collect::<Vec<_>>();
    let target_attributes_after_443_474_and_current_hp = target_attributes_after_443_474
        .iter()
        .copied()
        .filter(|id| *id != CURRENT_HP_ATTRIBUTE_ID)
        .collect::<Vec<_>>();
    let configured_endpoint_attribute_ids_that_differ = match damage_relationship {
        DamageRelationship::Source => &source_attributes,
        DamageRelationship::Target => &target_attributes,
    }
    .iter()
    .copied()
    .filter(|id| diagnostic_endpoint_attribute_ids.contains(id))
    .collect::<Vec<_>>();
    let source_attributes_after_configured_endpoint_443_474 = source_attributes_after_443_474
        .iter()
        .copied()
        .filter(|id| {
            damage_relationship != DamageRelationship::Source
                || !diagnostic_endpoint_attribute_ids.contains(id)
        })
        .collect::<Vec<_>>();
    let target_attributes_after_configured_endpoint_443_474_and_current_hp =
        target_attributes_after_443_474_and_current_hp
            .iter()
            .copied()
            .filter(|id| {
                damage_relationship != DamageRelationship::Target
                    || !diagnostic_endpoint_attribute_ids.contains(id)
            })
            .collect::<Vec<_>>();
    let attribute_snapshot_flag_differences = usize::from(
        prior.source_state.attribute_snapshot_after_boundary
            != sample.source_state.attribute_snapshot_after_boundary,
    ) + usize::from(
        prior.target_state.attribute_snapshot_after_boundary
            != sample.target_state.attribute_snapshot_after_boundary,
    );
    let temporary_snapshot_flag_differences = usize::from(
        prior.source_state.temporary_snapshot_after_boundary
            != sample.source_state.temporary_snapshot_after_boundary,
    ) + usize::from(
        prior.target_state.temporary_snapshot_after_boundary
            != sample.target_state.temporary_snapshot_after_boundary,
    );
    let residual_after_443_474 = source_attributes_after_443_474.len()
        + target_attributes_after_443_474.len()
        + source_temporary.len()
        + target_temporary.len()
        + source_statuses.len()
        + target_statuses.len()
        + attribute_snapshot_flag_differences
        + temporary_snapshot_flag_differences;
    let residual_after_443_474_and_current_hp = source_attributes_after_443_474.len()
        + target_attributes_after_443_474_and_current_hp.len()
        + source_temporary.len()
        + target_temporary.len()
        + source_statuses.len()
        + target_statuses.len()
        + attribute_snapshot_flag_differences
        + temporary_snapshot_flag_differences;
    let residual_after_configured_endpoint_443_474_and_current_hp =
        source_attributes_after_configured_endpoint_443_474.len()
            + target_attributes_after_configured_endpoint_443_474_and_current_hp.len()
            + source_temporary.len()
            + target_temporary.len()
            + source_statuses.len()
            + target_statuses.len()
            + attribute_snapshot_flag_differences
            + temporary_snapshot_flag_differences;
    update_minimum(
        &mut session.minimum_residual_observed_state_dimensions_after_443_474_exclusion,
        residual_after_443_474,
    );
    update_minimum(
        &mut session
            .minimum_residual_observed_state_dimensions_after_443_474_and_target_current_hp_exclusion,
        residual_after_443_474_and_current_hp,
    );
    update_minimum(
        &mut session
            .minimum_residual_observed_state_dimensions_after_configured_endpoint_443_474_and_target_current_hp_exclusion,
        residual_after_configured_endpoint_443_474_and_current_hp,
    );
    if !configured_endpoint_attribute_ids_that_differ.is_empty() {
        debug_assert_eq!(prior.action_identity, sample.action_identity);
        session.configured_endpoint_transition_pairs = session
            .configured_endpoint_transition_pairs
            .saturating_add(1);
        *session
            .configured_endpoint_transition_action_identity_counts
            .entry(sample.action_identity.clone())
            .or_default() += 1;
        observe_configured_endpoint_spatial_relations(session, prior, sample);
        increment_counts(
            &mut session.configured_endpoint_attribute_transition_counts,
            &configured_endpoint_attribute_ids_that_differ,
        );
        increment_counts(
            &mut session.configured_endpoint_transition_source_residual_attribute_difference_counts,
            &source_attributes_after_configured_endpoint_443_474,
        );
        increment_counts(
            &mut session.configured_endpoint_transition_target_residual_attribute_difference_counts,
            &target_attributes_after_configured_endpoint_443_474_and_current_hp,
        );
        increment_counts(
            &mut session
                .configured_endpoint_transition_source_temporary_attribute_difference_counts,
            &source_temporary,
        );
        increment_counts(
            &mut session
                .configured_endpoint_transition_target_temporary_attribute_difference_counts,
            &target_temporary,
        );
        increment_counts(
            &mut session.configured_endpoint_transition_source_status_difference_counts,
            &source_statuses,
        );
        increment_counts(
            &mut session.configured_endpoint_transition_target_status_difference_counts,
            &target_statuses,
        );
        *session
            .configured_endpoint_transition_residual_dimension_count_distribution
            .entry(residual_after_configured_endpoint_443_474_and_current_hp)
            .or_default() += 1;
        if attribute_snapshot_flag_differences > 0 {
            session.configured_endpoint_transition_pairs_with_attribute_snapshot_flag_difference =
                session
                    .configured_endpoint_transition_pairs_with_attribute_snapshot_flag_difference
                    .saturating_add(1);
        }
        if temporary_snapshot_flag_differences > 0 {
            session.configured_endpoint_transition_pairs_with_temporary_snapshot_flag_difference =
                session
                    .configured_endpoint_transition_pairs_with_temporary_snapshot_flag_difference
                    .saturating_add(1);
        }
        update_minimum(
            &mut session
                .minimum_residual_observed_state_dimensions_among_configured_endpoint_transition_pairs,
            residual_after_configured_endpoint_443_474_and_current_hp,
        );
    }
    let attributes_match_after_443_474 = source_attributes_after_443_474.is_empty()
        && target_attributes_after_443_474.is_empty()
        && source_temporary.is_empty()
        && target_temporary.is_empty()
        && attribute_snapshot_flag_differences == 0
        && temporary_snapshot_flag_differences == 0;
    let attributes_match_after_443_474_and_current_hp = source_attributes_after_443_474.is_empty()
        && target_attributes_after_443_474_and_current_hp.is_empty()
        && source_temporary.is_empty()
        && target_temporary.is_empty()
        && attribute_snapshot_flag_differences == 0
        && temporary_snapshot_flag_differences == 0;
    let attributes_match_after_configured_endpoint_443_474_and_current_hp =
        source_attributes_after_configured_endpoint_443_474.is_empty()
            && target_attributes_after_configured_endpoint_443_474_and_current_hp.is_empty()
            && source_temporary.is_empty()
            && target_temporary.is_empty()
            && attribute_snapshot_flag_differences == 0
            && temporary_snapshot_flag_differences == 0;
    if attributes_match_after_443_474 {
        session.same_context_pairs_after_443_474_attribute_exclusion = session
            .same_context_pairs_after_443_474_attribute_exclusion
            .saturating_add(1);
    }
    if attributes_match_after_443_474_and_current_hp {
        session.same_context_pairs_after_443_474_and_target_current_hp_exclusion = session
            .same_context_pairs_after_443_474_and_target_current_hp_exclusion
            .saturating_add(1);
        if source_statuses.is_empty() && target_statuses.is_empty() {
            session
                .same_context_pairs_after_443_474_and_target_current_hp_exclusion_with_equal_statuses =
                session
                    .same_context_pairs_after_443_474_and_target_current_hp_exclusion_with_equal_statuses
                    .saturating_add(1);
        }
    }
    if attributes_match_after_configured_endpoint_443_474_and_current_hp {
        session
            .same_context_pairs_after_configured_endpoint_443_474_and_target_current_hp_exclusion =
            session
                .same_context_pairs_after_configured_endpoint_443_474_and_target_current_hp_exclusion
                .saturating_add(1);
        if source_statuses.is_empty() && target_statuses.is_empty() {
            session
                .same_context_pairs_after_configured_endpoint_443_474_and_target_current_hp_exclusion_with_equal_statuses =
                session
                    .same_context_pairs_after_configured_endpoint_443_474_and_target_current_hp_exclusion_with_equal_statuses
                    .saturating_add(1);
            if !configured_endpoint_attribute_ids_that_differ.is_empty() {
                session
                    .same_context_pairs_after_configured_endpoint_transition_and_diagnostic_exclusions_with_equal_statuses =
                    session
                        .same_context_pairs_after_configured_endpoint_transition_and_diagnostic_exclusions_with_equal_statuses
                        .saturating_add(1);
            }
        }
    }
    increment_counts(
        &mut session.same_context_source_attribute_difference_counts,
        &source_attributes,
    );
    increment_counts(
        &mut session.same_context_target_attribute_difference_counts,
        &target_attributes,
    );
    increment_counts(
        &mut session.same_context_source_temporary_attribute_difference_counts,
        &source_temporary,
    );
    increment_counts(
        &mut session.same_context_target_temporary_attribute_difference_counts,
        &target_temporary,
    );
    increment_counts(
        &mut session.same_context_source_status_difference_counts,
        &source_statuses,
    );
    increment_counts(
        &mut session.same_context_target_status_difference_counts,
        &target_statuses,
    );
    let only_target_current_hp_differs = source_attributes.is_empty()
        && target_attributes.as_slice() == [CURRENT_HP_ATTRIBUTE_ID]
        && source_temporary.is_empty()
        && target_temporary.is_empty()
        && source_statuses.is_empty()
        && target_statuses.is_empty()
        && prior.source_state.attribute_snapshot_after_boundary
            == sample.source_state.attribute_snapshot_after_boundary
        && prior.target_state.attribute_snapshot_after_boundary
            == sample.target_state.attribute_snapshot_after_boundary
        && prior.source_state.temporary_snapshot_after_boundary
            == sample.source_state.temporary_snapshot_after_boundary
        && prior.target_state.temporary_snapshot_after_boundary
            == sample.target_state.temporary_snapshot_after_boundary;
    if only_target_current_hp_differs {
        session.same_context_pairs_with_only_target_current_hp_difference = session
            .same_context_pairs_with_only_target_current_hp_difference
            .saturating_add(1);
    }
    let (present, absent) = if sample.selected_effect_active {
        (sample, prior)
    } else {
        (prior, sample)
    };
    examples.push(SameContextMismatchExample {
        rlog: display_path(path),
        session_id: session_id.to_owned(),
        segment_index: sample.segment_index,
        present_sequence: present.sequence,
        absent_sequence: absent.sequence,
        pair_gap_micros: sample.observed_micros.saturating_sub(prior.observed_micros),
        source_actor_id: sample.source.actor_id.0,
        source_entity_uuid: sample.source.entity_uuid.0,
        target_actor_id: sample.target.actor_id.0,
        target_entity_uuid: sample.target.entity_uuid.0,
        ability_id: sample.ability_id,
        action_identity: sample.action_identity.clone(),
        present_amount: present.output.amount,
        absent_amount: absent.output.amount,
        present_normal_value: present.output.normal_value,
        absent_normal_value: absent.output.normal_value,
        source_attribute_ids: source_attributes,
        target_attribute_ids: target_attributes,
        source_temporary_attribute_ids: source_temporary,
        target_temporary_attribute_ids: target_temporary,
        source_status_effect_ids: source_statuses,
        target_status_effect_ids: target_statuses,
        only_target_current_hp_differs,
        residual_observed_state_dimensions_after_443_474_exclusion: residual_after_443_474,
        residual_observed_state_dimensions_after_443_474_and_target_current_hp_exclusion:
            residual_after_443_474_and_current_hp,
        configured_endpoint_attribute_ids_that_differ,
        residual_observed_state_dimensions_after_configured_endpoint_443_474_and_target_current_hp_exclusion:
            residual_after_configured_endpoint_443_474_and_current_hp,
        source_target_attribute_snapshots_complete: prior
            .source_target_attribute_snapshots_complete
            && sample.source_target_attribute_snapshots_complete,
        selected_provider_exact: present.selected_instance_count == 1
            && present.selected_provider.is_some(),
        segment_status_baseline_complete: false,
        controlled_counterfactual_pair_proven: false,
        formula_authority: false,
    });
}

const SPATIAL_RELATIONS: [(&str, PositionSlot, PositionSlot); 6] = [
    (
        "source_attr_pos_to_source_attr_target_pos",
        PositionSlot::SourcePosition,
        PositionSlot::SourceTargetPosition,
    ),
    (
        "target_attr_pos_to_target_attr_target_pos",
        PositionSlot::TargetPosition,
        PositionSlot::TargetTargetPosition,
    ),
    (
        "source_attr_pos_to_target_attr_pos",
        PositionSlot::SourcePosition,
        PositionSlot::TargetPosition,
    ),
    (
        "source_attr_pos_to_target_attr_target_pos",
        PositionSlot::SourcePosition,
        PositionSlot::TargetTargetPosition,
    ),
    (
        "source_attr_target_pos_to_target_attr_pos",
        PositionSlot::SourceTargetPosition,
        PositionSlot::TargetPosition,
    ),
    (
        "source_attr_target_pos_to_target_attr_target_pos",
        PositionSlot::SourceTargetPosition,
        PositionSlot::TargetTargetPosition,
    ),
];

fn observe_configured_endpoint_spatial_relations(
    session: &mut SessionAudit,
    prior: &RecentDamage,
    sample: &RecentDamage,
) {
    for (name, from, to) in SPATIAL_RELATIONS {
        let prior_vector = spatial_relation_vector(prior, from, to);
        let sample_vector = spatial_relation_vector(sample, from, to);
        let audit = session
            .configured_endpoint_transition_spatial_relations
            .entry(name.to_owned())
            .or_default();
        audit.pair_count = audit.pair_count.saturating_add(1);
        if prior_vector.is_some() {
            audit.prior_relation_complete_count =
                audit.prior_relation_complete_count.saturating_add(1);
        }
        if sample_vector.is_some() {
            audit.sample_relation_complete_count =
                audit.sample_relation_complete_count.saturating_add(1);
        }
        if let (Some(prior_vector), Some(sample_vector)) = (prior_vector, sample_vector) {
            observe_spatial_relation_comparison(audit, prior_vector, sample_vector);
        }
    }
}

fn spatial_relation_vector(
    damage: &RecentDamage,
    from: PositionSlot,
    to: PositionSlot,
) -> Option<Position3> {
    let from = spatial_position(damage, from)?;
    let to = spatial_position(damage, to)?;
    Some(Position3 {
        x: to.x - from.x,
        y: to.y - from.y,
        z: to.z - from.z,
    })
}

fn spatial_position(damage: &RecentDamage, slot: PositionSlot) -> Option<Position3> {
    let (state, attribute_id) = match slot {
        PositionSlot::SourcePosition => (&damage.source_state, POSITION_ATTRIBUTE_ID),
        PositionSlot::SourceTargetPosition => (&damage.source_state, TARGET_POSITION_ATTRIBUTE_ID),
        PositionSlot::TargetPosition => (&damage.target_state, POSITION_ATTRIBUTE_ID),
        PositionSlot::TargetTargetPosition => (&damage.target_state, TARGET_POSITION_ATTRIBUTE_ID),
    };
    let raw = state.attributes.get(&attribute_id)?;
    let EntityAttributeValue::Position { x, y, z, .. } =
        decode_known_entity_attribute_value(attribute_id, raw)?
    else {
        return None;
    };
    if !x.is_finite() || !y.is_finite() || !z.is_finite() {
        return None;
    }
    Some(Position3 {
        x: f64::from(x),
        y: f64::from(y),
        z: f64::from(z),
    })
}

fn observe_spatial_relation_comparison(
    audit: &mut SpatialRelationAudit,
    prior: Position3,
    sample: Position3,
) {
    audit.both_relations_complete_count = audit.both_relations_complete_count.saturating_add(1);
    let vector_equal = prior.x.to_bits() == sample.x.to_bits()
        && prior.y.to_bits() == sample.y.to_bits()
        && prior.z.to_bits() == sample.z.to_bits();
    if vector_equal {
        audit.exact_displacement_vector_equal_count = audit
            .exact_displacement_vector_equal_count
            .saturating_add(1);
    }
    let prior_squared = prior.x * prior.x + prior.y * prior.y + prior.z * prior.z;
    let sample_squared = sample.x * sample.x + sample.y * sample.y + sample.z * sample.z;
    if prior_squared.to_bits() == sample_squared.to_bits() {
        audit.exact_squared_distance_equal_count =
            audit.exact_squared_distance_equal_count.saturating_add(1);
    }
    let absolute_distance_delta = (prior_squared.sqrt() - sample_squared.sqrt()).abs();
    if absolute_distance_delta <= 0.000_001 {
        audit.absolute_distance_delta_le_0_000001_count = audit
            .absolute_distance_delta_le_0_000001_count
            .saturating_add(1);
    }
    if absolute_distance_delta <= 0.000_1 {
        audit.absolute_distance_delta_le_0_0001_count = audit
            .absolute_distance_delta_le_0_0001_count
            .saturating_add(1);
    }
    if absolute_distance_delta <= 0.01 {
        audit.absolute_distance_delta_le_0_01_count = audit
            .absolute_distance_delta_le_0_01_count
            .saturating_add(1);
    }
    if absolute_distance_delta <= 0.1 {
        audit.absolute_distance_delta_le_0_1_count =
            audit.absolute_distance_delta_le_0_1_count.saturating_add(1);
    }
    if absolute_distance_delta <= 1.0 {
        audit.absolute_distance_delta_le_1_count =
            audit.absolute_distance_delta_le_1_count.saturating_add(1);
    }
    audit.maximum_absolute_distance_delta_raw_coordinate_units = Some(
        audit
            .maximum_absolute_distance_delta_raw_coordinate_units
            .map_or(absolute_distance_delta, |current| {
                current.max(absolute_distance_delta)
            }),
    );
}

fn update_minimum(slot: &mut Option<usize>, value: usize) {
    *slot = Some(slot.map_or(value, |current| current.min(value)));
}

fn differing_map_keys<K, V>(left: &BTreeMap<K, V>, right: &BTreeMap<K, V>) -> Vec<K>
where
    K: Copy + Ord,
    V: PartialEq,
{
    left.keys()
        .chain(right.keys())
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|key| left.get(key) != right.get(key))
        .collect()
}

fn differing_status_effect_ids(
    left: &[(StatusKey, StatusValue)],
    right: &[(StatusKey, StatusValue)],
) -> Vec<i64> {
    let left = left.iter().cloned().collect::<BTreeMap<_, _>>();
    let right = right.iter().cloned().collect::<BTreeMap<_, _>>();
    left.keys()
        .chain(right.keys())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|key| left.get(key) != right.get(key))
        .map(|key| key.effect_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn increment_counts<K: Copy + Ord>(counts: &mut BTreeMap<K, u64>, keys: &[K]) {
    for key in keys {
        *counts.entry(*key).or_default() += 1;
    }
}

fn statuses_without_effect(
    statuses: Option<&BTreeMap<StatusKey, StatusValue>>,
    effect_id: i64,
) -> Vec<(StatusKey, StatusValue)> {
    statuses
        .into_iter()
        .flat_map(|values| values.iter())
        .filter(|(key, _)| key.effect_id != effect_id)
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn hex_sha256(digest: &[u8; 32]) -> String {
    let mut output = String::with_capacity(71);
    output.push_str("sha256:");
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

fn sha256_serialized(value: &impl Serialize) -> Result<[u8; 32], serde_json::Error> {
    Ok(Sha256::digest(serde_json::to_vec(value)?).into())
}

fn normalized_packet_inputs(packet: &DamagePacketDetail) -> DamagePacketDetail {
    let mut packet = packet.clone();
    packet.dead = None;
    packet.normal_value = None;
    packet.lucky_value = None;
    packet.skill_effect_uuid = None;
    packet.skill_effect_total_damage = None;
    packet.skill_effect_group_index = None;
    for hit_part in &mut packet.hit_parts {
        hit_part.damage_value = None;
    }
    packet
}

fn damage_action_identity(damage: &DamageEvent) -> String {
    format!(
        "ability={};hit_event={};owner={};property={};damage_source={};damage_type={};damage_mode={};normal_hit={};passive_uuid={};component_index={};component_count={}",
        option_identity(damage.ability.map(|value| value.0)),
        option_identity(damage.hit_event_id),
        option_identity(damage.packet.owner_id),
        option_identity(damage.packet.property),
        option_identity(damage.damage_source),
        option_identity(damage.damage_type),
        option_identity(damage.packet.damage_mode),
        option_identity(damage.packet.normal_hit),
        option_identity(damage.packet.passive_uuid),
        option_identity(damage.packet.skill_effect_component_index),
        option_identity(damage.packet.skill_effect_component_count),
    )
}

fn option_identity(value: Option<impl std::fmt::Display>) -> String {
    value.map_or_else(|| "none".to_owned(), |value| value.to_string())
}

fn entity_key(entity: EntityRef) -> (u64, i64) {
    (entity.actor_id.0, entity.entity_uuid.0)
}

fn summarize(sessions: &[SessionAudit], receipts: &[RlogReceipt]) -> AuditSummary {
    let mut fingerprints = BTreeMap::<String, ()>::new();
    // Session reports contain only the distinct count, so the exact union is not
    // reconstructible here. Fingerprints cannot cross RLOG/session identity in
    // the input hash, making the sum exact.
    for (index, session) in sessions.iter().enumerate() {
        for fingerprint_index in 0..session.distinct_candidate_input_fingerprints {
            fingerprints.insert(format!("{index}:{fingerprint_index}"), ());
        }
    }
    AuditSummary {
        source_rlog_count: sessions.len(),
        source_rlog_bytes: receipts.iter().map(|receipt| receipt.bytes).sum(),
        canonical_event_count: sessions.iter().map(|session| session.event_count).sum(),
        reset_boundary_count: sum(sessions, |session| session.reset_boundary_count),
        data_gap_count: sum(sessions, |session| session.data_gap_count),
        recorder_pause_count: sum(sessions, |session| session.recorder_pause_count),
        run_boundary_count: sum(sessions, |session| session.run_boundary_count),
        damage_events: sum(sessions, |session| session.damage_events),
        damage_events_with_selected_effect_active: sum(sessions, |session| {
            session.damage_events_with_selected_effect_active
        }),
        damage_events_with_selected_effect_absent: sum(sessions, |session| {
            session.damage_events_with_selected_effect_absent
        }),
        opposite_state_recent_comparisons: sum(sessions, |session| {
            session.opposite_state_recent_comparisons
        }),
        same_normalized_damage_context_pairs: sum(sessions, |session| {
            session.same_normalized_damage_context_pairs
        }),
        same_context_and_observed_attribute_pairs: sum(sessions, |session| {
            session.same_context_and_observed_attribute_pairs
        }),
        same_context_and_nonselected_status_pairs: sum(sessions, |session| {
            session.same_context_and_nonselected_status_pairs
        }),
        same_context_pairs_with_only_target_current_hp_difference: sum(sessions, |session| {
            session.same_context_pairs_with_only_target_current_hp_difference
        }),
        same_context_pairs_after_443_474_attribute_exclusion: sum(sessions, |session| {
            session.same_context_pairs_after_443_474_attribute_exclusion
        }),
        same_context_pairs_after_443_474_and_target_current_hp_exclusion: sum(
            sessions,
            |session| {
                session.same_context_pairs_after_443_474_and_target_current_hp_exclusion
            },
        ),
        same_context_pairs_after_443_474_and_target_current_hp_exclusion_with_equal_statuses:
            sum(sessions, |session| {
                session
                    .same_context_pairs_after_443_474_and_target_current_hp_exclusion_with_equal_statuses
            }),
        same_context_pairs_after_configured_endpoint_443_474_and_target_current_hp_exclusion:
            sum(sessions, |session| {
                session
                    .same_context_pairs_after_configured_endpoint_443_474_and_target_current_hp_exclusion
            }),
        same_context_pairs_after_configured_endpoint_443_474_and_target_current_hp_exclusion_with_equal_statuses:
            sum(sessions, |session| {
                session
                    .same_context_pairs_after_configured_endpoint_443_474_and_target_current_hp_exclusion_with_equal_statuses
            }),
        same_context_pairs_after_configured_endpoint_transition_and_diagnostic_exclusions_with_equal_statuses:
            sum(sessions, |session| {
                session
                    .same_context_pairs_after_configured_endpoint_transition_and_diagnostic_exclusions_with_equal_statuses
            }),
        configured_endpoint_transition_pairs: sum(sessions, |session| {
            session.configured_endpoint_transition_pairs
        }),
        configured_endpoint_transition_action_identity_counts: merge_counts(
            sessions,
            |session| &session.configured_endpoint_transition_action_identity_counts,
        ),
        configured_endpoint_attribute_transition_counts: merge_counts(sessions, |session| {
            &session.configured_endpoint_attribute_transition_counts
        }),
        configured_endpoint_transition_source_residual_attribute_difference_counts: merge_counts(
            sessions,
            |session| {
                &session.configured_endpoint_transition_source_residual_attribute_difference_counts
            },
        ),
        configured_endpoint_transition_target_residual_attribute_difference_counts: merge_counts(
            sessions,
            |session| {
                &session.configured_endpoint_transition_target_residual_attribute_difference_counts
            },
        ),
        configured_endpoint_transition_source_temporary_attribute_difference_counts: merge_counts(
            sessions,
            |session| {
                &session.configured_endpoint_transition_source_temporary_attribute_difference_counts
            },
        ),
        configured_endpoint_transition_target_temporary_attribute_difference_counts: merge_counts(
            sessions,
            |session| {
                &session.configured_endpoint_transition_target_temporary_attribute_difference_counts
            },
        ),
        configured_endpoint_transition_source_status_difference_counts: merge_counts(
            sessions,
            |session| &session.configured_endpoint_transition_source_status_difference_counts,
        ),
        configured_endpoint_transition_target_status_difference_counts: merge_counts(
            sessions,
            |session| &session.configured_endpoint_transition_target_status_difference_counts,
        ),
        configured_endpoint_transition_residual_dimension_count_distribution: merge_counts(
            sessions,
            |session| &session.configured_endpoint_transition_residual_dimension_count_distribution,
        ),
        configured_endpoint_transition_pairs_with_attribute_snapshot_flag_difference: sum(
            sessions,
            |session| {
                session
                    .configured_endpoint_transition_pairs_with_attribute_snapshot_flag_difference
            },
        ),
        configured_endpoint_transition_pairs_with_temporary_snapshot_flag_difference: sum(
            sessions,
            |session| {
                session
                    .configured_endpoint_transition_pairs_with_temporary_snapshot_flag_difference
            },
        ),
        configured_endpoint_transition_spatial_relations: merge_spatial_relation_audits(sessions),
        minimum_residual_observed_state_dimensions_after_443_474_exclusion: sessions
            .iter()
            .filter_map(|session| {
                session.minimum_residual_observed_state_dimensions_after_443_474_exclusion
            })
            .min(),
        minimum_residual_observed_state_dimensions_after_443_474_and_target_current_hp_exclusion:
            sessions
                .iter()
                .filter_map(|session| {
                    session
                        .minimum_residual_observed_state_dimensions_after_443_474_and_target_current_hp_exclusion
                })
                .min(),
        minimum_residual_observed_state_dimensions_after_configured_endpoint_443_474_and_target_current_hp_exclusion:
            sessions
                .iter()
                .filter_map(|session| {
                    session
                        .minimum_residual_observed_state_dimensions_after_configured_endpoint_443_474_and_target_current_hp_exclusion
                })
                .min(),
        minimum_residual_observed_state_dimensions_among_configured_endpoint_transition_pairs:
            sessions
                .iter()
                .filter_map(|session| {
                    session
                        .minimum_residual_observed_state_dimensions_among_configured_endpoint_transition_pairs
                })
                .min(),
        same_context_source_attribute_difference_counts: merge_counts(sessions, |session| {
            &session.same_context_source_attribute_difference_counts
        }),
        same_context_target_attribute_difference_counts: merge_counts(sessions, |session| {
            &session.same_context_target_attribute_difference_counts
        }),
        same_context_source_temporary_attribute_difference_counts: merge_counts(
            sessions,
            |session| &session.same_context_source_temporary_attribute_difference_counts,
        ),
        same_context_target_temporary_attribute_difference_counts: merge_counts(
            sessions,
            |session| &session.same_context_target_temporary_attribute_difference_counts,
        ),
        same_context_source_status_difference_counts: merge_counts(sessions, |session| {
            &session.same_context_source_status_difference_counts
        }),
        same_context_target_status_difference_counts: merge_counts(sessions, |session| {
            &session.same_context_target_status_difference_counts
        }),
        exact_observed_input_candidate_pairs: sum(sessions, |session| {
            session.exact_observed_input_candidate_pairs
        }),
        exact_observed_input_equal_output_pairs: sum(sessions, |session| {
            session.exact_observed_input_equal_output_pairs
        }),
        exact_observed_input_divergent_output_pairs: sum(sessions, |session| {
            session.exact_observed_input_divergent_output_pairs
        }),
        candidate_pairs_with_complete_source_target_attribute_snapshots: sum(sessions, |session| {
            session.candidate_pairs_with_complete_source_target_attribute_snapshots
        }),
        candidate_pairs_with_exact_selected_provider: sum(sessions, |session| {
            session.candidate_pairs_with_exact_selected_provider
        }),
        candidate_pairs_with_unresolved_segment_status_baseline: sum(sessions, |session| {
            session.candidate_pairs_with_unresolved_segment_status_baseline
        }),
        target_current_hp_excluded_candidate_pairs: sum(sessions, |session| {
            session.target_current_hp_excluded_candidate_pairs
        }),
        target_current_hp_excluded_equal_output_pairs: sum(sessions, |session| {
            session.target_current_hp_excluded_equal_output_pairs
        }),
        target_current_hp_excluded_divergent_output_pairs: sum(sessions, |session| {
            session.target_current_hp_excluded_divergent_output_pairs
        }),
        strict_controlled_counterfactual_pairs: sum(sessions, |session| {
            session.strict_controlled_counterfactual_pairs
        }),
        distinct_candidate_input_fingerprints: fingerprints.len(),
        maximum_recent_samples_retained: sessions
            .iter()
            .map(|session| session.maximum_recent_samples_retained)
            .max()
            .unwrap_or(0),
        exact_damage_projection_proven: false,
        exact_operation_order_proven: false,
        exact_integer_rounding_proven: false,
        packet_conservation_proven: false,
        formula_authority: false,
        runtime_authority: false,
        ui_display_authority: false,
        provider_rdps_credit_allowed: false,
    }
}

fn sum(sessions: &[SessionAudit], value: impl Fn(&SessionAudit) -> u64) -> u64 {
    sessions.iter().map(value).sum()
}

fn merge_counts<K: Clone + Ord>(
    sessions: &[SessionAudit],
    counts: impl Fn(&SessionAudit) -> &BTreeMap<K, u64>,
) -> BTreeMap<K, u64> {
    let mut merged = BTreeMap::new();
    for session in sessions {
        for (key, value) in counts(session) {
            *merged.entry(key.clone()).or_default() += *value;
        }
    }
    merged
}

fn merge_spatial_relation_audits(
    sessions: &[SessionAudit],
) -> BTreeMap<String, SpatialRelationAudit> {
    let mut merged = BTreeMap::<String, SpatialRelationAudit>::new();
    for session in sessions {
        for (name, audit) in &session.configured_endpoint_transition_spatial_relations {
            let target = merged.entry(name.clone()).or_default();
            target.pair_count = target.pair_count.saturating_add(audit.pair_count);
            target.prior_relation_complete_count = target
                .prior_relation_complete_count
                .saturating_add(audit.prior_relation_complete_count);
            target.sample_relation_complete_count = target
                .sample_relation_complete_count
                .saturating_add(audit.sample_relation_complete_count);
            target.both_relations_complete_count = target
                .both_relations_complete_count
                .saturating_add(audit.both_relations_complete_count);
            target.exact_displacement_vector_equal_count = target
                .exact_displacement_vector_equal_count
                .saturating_add(audit.exact_displacement_vector_equal_count);
            target.exact_squared_distance_equal_count = target
                .exact_squared_distance_equal_count
                .saturating_add(audit.exact_squared_distance_equal_count);
            target.absolute_distance_delta_le_0_000001_count = target
                .absolute_distance_delta_le_0_000001_count
                .saturating_add(audit.absolute_distance_delta_le_0_000001_count);
            target.absolute_distance_delta_le_0_0001_count = target
                .absolute_distance_delta_le_0_0001_count
                .saturating_add(audit.absolute_distance_delta_le_0_0001_count);
            target.absolute_distance_delta_le_0_01_count = target
                .absolute_distance_delta_le_0_01_count
                .saturating_add(audit.absolute_distance_delta_le_0_01_count);
            target.absolute_distance_delta_le_0_1_count = target
                .absolute_distance_delta_le_0_1_count
                .saturating_add(audit.absolute_distance_delta_le_0_1_count);
            target.absolute_distance_delta_le_1_count = target
                .absolute_distance_delta_le_1_count
                .saturating_add(audit.absolute_distance_delta_le_1_count);
            target.maximum_absolute_distance_delta_raw_coordinate_units = match (
                target.maximum_absolute_distance_delta_raw_coordinate_units,
                audit.maximum_absolute_distance_delta_raw_coordinate_units,
            ) {
                (Some(left), Some(right)) => Some(left.max(right)),
                (Some(value), None) | (None, Some(value)) => Some(value),
                (None, None) => None,
            };
            target.spatial_state_safe_to_exclude_from_counterfactual_matching = false;
            target.formula_authority = false;
        }
    }
    merged
}

fn spatial_relation_audits_are_valid(
    audits: &BTreeMap<String, SpatialRelationAudit>,
    configured_transition_pairs: u64,
) -> bool {
    if configured_transition_pairs == 0 {
        return audits.is_empty();
    }
    let expected = SPATIAL_RELATIONS
        .iter()
        .map(|(name, _, _)| *name)
        .collect::<BTreeSet<_>>();
    if audits.keys().map(String::as_str).collect::<BTreeSet<_>>() != expected {
        return false;
    }
    audits.values().all(|audit| {
        audit.pair_count == configured_transition_pairs
            && audit.prior_relation_complete_count <= audit.pair_count
            && audit.sample_relation_complete_count <= audit.pair_count
            && audit.both_relations_complete_count <= audit.prior_relation_complete_count
            && audit.both_relations_complete_count <= audit.sample_relation_complete_count
            && audit.exact_displacement_vector_equal_count
                <= audit.exact_squared_distance_equal_count
            && audit.exact_squared_distance_equal_count
                <= audit.absolute_distance_delta_le_0_000001_count
            && audit.absolute_distance_delta_le_0_000001_count
                <= audit.absolute_distance_delta_le_0_0001_count
            && audit.absolute_distance_delta_le_0_0001_count
                <= audit.absolute_distance_delta_le_0_01_count
            && audit.absolute_distance_delta_le_0_01_count
                <= audit.absolute_distance_delta_le_0_1_count
            && audit.absolute_distance_delta_le_0_1_count
                <= audit.absolute_distance_delta_le_1_count
            && audit.absolute_distance_delta_le_1_count <= audit.both_relations_complete_count
            && (audit.both_relations_complete_count == 0)
                == audit
                    .maximum_absolute_distance_delta_raw_coordinate_units
                    .is_none()
            && audit
                .maximum_absolute_distance_delta_raw_coordinate_units
                .is_none_or(|value| value.is_finite() && value >= 0.0)
            && !audit.spatial_state_safe_to_exclude_from_counterfactual_matching
            && !audit.formula_authority
    })
}

fn verify_report(report: &AuditReport) -> Result<(), Box<dyn Error>> {
    if report.schema_version != SCHEMA_VERSION
        || report.generated_by != GENERATED_BY
        || report.game_build.is_empty()
        || !report
            .game_build
            .bytes()
            .all(|value| value.is_ascii_digit())
        || report.effect_id <= 0
    {
        return Err("unsupported transition counterfactual audit identity".into());
    }
    if !report.policy.sealed_rlogs_are_streamed_one_event_at_a_time
        || !report
            .policy
            .every_data_gap_pause_and_run_boundary_resets_all_observed_state
        || !report
            .policy
            .only_same_segment_transition_adjacent_pairs_are_compared
        || !report.policy.exact_numeric_ids_and_build_are_authoritative
        || !report.policy.damage_relationship_is_explicit
        || report.search_contract.selected_effect_endpoint_role
            != report.damage_relationship.endpoint_role()
        || !report.policy.packet_absence_is_not_zero
        || !report
            .policy
            .unknown_segment_baseline_statuses_are_preserved_as_unresolved
        || !report.policy.target_current_hp_exclusion_is_diagnostic_only
        || !report.policy.attribute_443_474_exclusion_is_diagnostic_only
        || !report
            .policy
            .configured_endpoint_attribute_exclusion_is_diagnostic_only
        || !report.policy.relative_spatial_relations_are_diagnostic_only
        || !report
            .policy
            .structurally_unobservable_remote_player_packets_are_not_acquisition_requirements
        || !report
            .policy
            .candidate_pairs_never_grant_formula_or_runtime_authority
        || !report
            .policy
            .current_snapshots_are_never_backfilled_into_historical_segments
        || report.policy.formula_authority
        || report.policy.runtime_authority
        || report.policy.ui_display_authority
        || report.policy.provider_rdps_credit_allowed
        || report.search_contract.remote_player_packet_dependency
        || report
            .search_contract
            .diagnostic_endpoint_attribute_ids
            .is_empty()
        || report
            .search_contract
            .diagnostic_endpoint_attribute_ids
            .iter()
            .any(|id| *id <= 0)
        || report
            .search_contract
            .diagnostic_endpoint_attribute_ids
            .windows(2)
            .any(|ids| ids[0] >= ids[1])
    {
        return Err("transition counterfactual audit policy is unsafe".into());
    }
    if report.content_sha256 != report_digest(report)? {
        return Err("transition counterfactual audit content digest mismatch".into());
    }
    let expected = summarize(&report.sessions, &report.inputs.source_rlogs);
    let configured_transition_distribution_total = report
        .summary
        .configured_endpoint_transition_residual_dimension_count_distribution
        .values()
        .copied()
        .sum::<u64>();
    let configured_transition_distribution_minimum = report
        .summary
        .configured_endpoint_transition_residual_dimension_count_distribution
        .keys()
        .next()
        .copied();
    let configured_transition_attribute_observations = report
        .summary
        .configured_endpoint_attribute_transition_counts
        .values()
        .copied()
        .sum::<u64>();
    let configured_transition_action_observations = report
        .summary
        .configured_endpoint_transition_action_identity_counts
        .values()
        .copied()
        .sum::<u64>();
    if serde_json::to_value(&expected)? != serde_json::to_value(&report.summary)?
        || report.inputs.source_rlogs.len() != report.sessions.len()
        || report.summary.reset_boundary_count
            != report
                .summary
                .data_gap_count
                .saturating_add(report.summary.recorder_pause_count)
                .saturating_add(report.summary.run_boundary_count)
        || report.summary.damage_events
            != report
                .summary
                .damage_events_with_selected_effect_active
                .saturating_add(report.summary.damage_events_with_selected_effect_absent)
        || report.summary.exact_observed_input_candidate_pairs
            != report
                .summary
                .exact_observed_input_equal_output_pairs
                .saturating_add(report.summary.exact_observed_input_divergent_output_pairs)
        || report
            .summary
            .candidate_pairs_with_unresolved_segment_status_baseline
            != report.summary.exact_observed_input_candidate_pairs
        || report.summary.target_current_hp_excluded_candidate_pairs
            != report
                .summary
                .target_current_hp_excluded_equal_output_pairs
                .saturating_add(
                    report
                        .summary
                        .target_current_hp_excluded_divergent_output_pairs,
                )
        || report
            .summary
            .same_context_pairs_after_443_474_and_target_current_hp_exclusion_with_equal_statuses
            > report
                .summary
                .same_context_pairs_after_443_474_and_target_current_hp_exclusion
        || report
            .summary
            .same_context_pairs_after_443_474_and_target_current_hp_exclusion
            > report.summary.same_normalized_damage_context_pairs
        || report
            .summary
            .same_context_pairs_after_configured_endpoint_transition_and_diagnostic_exclusions_with_equal_statuses
            > report
                .summary
                .same_context_pairs_after_configured_endpoint_443_474_and_target_current_hp_exclusion_with_equal_statuses
        || report
            .summary
            .same_context_pairs_after_configured_endpoint_443_474_and_target_current_hp_exclusion_with_equal_statuses
            > report
                .summary
                .same_context_pairs_after_configured_endpoint_443_474_and_target_current_hp_exclusion
        || report
            .summary
            .same_context_pairs_after_configured_endpoint_443_474_and_target_current_hp_exclusion
            > report.summary.same_normalized_damage_context_pairs
        || report.summary.configured_endpoint_transition_pairs
            > report.summary.same_normalized_damage_context_pairs
        || report
            .summary
            .same_context_pairs_after_configured_endpoint_transition_and_diagnostic_exclusions_with_equal_statuses
            > report.summary.configured_endpoint_transition_pairs
        || configured_transition_distribution_total
            != report.summary.configured_endpoint_transition_pairs
        || configured_transition_action_observations
            != report.summary.configured_endpoint_transition_pairs
        || !spatial_relation_audits_are_valid(
            &report.summary.configured_endpoint_transition_spatial_relations,
            report.summary.configured_endpoint_transition_pairs,
        )
        || report
            .summary
            .configured_endpoint_transition_action_identity_counts
            .keys()
            .any(|identity| !identity.starts_with("ability="))
        || configured_transition_attribute_observations
            < report.summary.configured_endpoint_transition_pairs
        || report
            .summary
            .configured_endpoint_attribute_transition_counts
            .keys()
            .any(|id| {
                !report
                    .search_contract
                    .diagnostic_endpoint_attribute_ids
                    .contains(id)
            })
        || report
            .summary
            .configured_endpoint_transition_pairs_with_attribute_snapshot_flag_difference
            > report.summary.configured_endpoint_transition_pairs
        || report
            .summary
            .configured_endpoint_transition_pairs_with_temporary_snapshot_flag_difference
            > report.summary.configured_endpoint_transition_pairs
        || (report.summary.configured_endpoint_transition_pairs == 0
            && (report
                .summary
                .minimum_residual_observed_state_dimensions_among_configured_endpoint_transition_pairs
                .is_some()
                || configured_transition_distribution_minimum.is_some()))
        || (report.summary.configured_endpoint_transition_pairs > 0
            && (report
                .summary
                .minimum_residual_observed_state_dimensions_among_configured_endpoint_transition_pairs
                != configured_transition_distribution_minimum))
        || report
            .summary
            .configured_endpoint_transition_source_residual_attribute_difference_counts
            .keys()
            .chain(
                report
                    .summary
                    .configured_endpoint_transition_target_residual_attribute_difference_counts
                    .keys(),
            )
            .any(|id| DIAGNOSTIC_EXCLUDED_ENTITY_ATTRIBUTE_IDS.contains(id))
        || report
            .summary
            .configured_endpoint_transition_target_residual_attribute_difference_counts
            .contains_key(&CURRENT_HP_ATTRIBUTE_ID)
        || (report.damage_relationship == DamageRelationship::Source
            && report
                .summary
                .configured_endpoint_transition_source_residual_attribute_difference_counts
                .keys()
                .any(|id| {
                    report
                        .search_contract
                        .diagnostic_endpoint_attribute_ids
                        .contains(id)
                }))
        || (report.damage_relationship == DamageRelationship::Target
            && report
                .summary
                .configured_endpoint_transition_target_residual_attribute_difference_counts
                .keys()
                .any(|id| {
                    report
                        .search_contract
                        .diagnostic_endpoint_attribute_ids
                        .contains(id)
                }))
        || report
            .summary
            .same_context_pairs_after_443_474_attribute_exclusion
            > report.summary.same_normalized_damage_context_pairs
        || (report.summary.same_normalized_damage_context_pairs > 0
            && (report
                .summary
                .minimum_residual_observed_state_dimensions_after_443_474_exclusion
                .is_none()
                || report
                    .summary
                    .minimum_residual_observed_state_dimensions_after_443_474_and_target_current_hp_exclusion
                    .is_none()
                || report
                    .summary
                    .minimum_residual_observed_state_dimensions_after_configured_endpoint_443_474_and_target_current_hp_exclusion
                    .is_none()))
        || report.summary.strict_controlled_counterfactual_pairs != 0
        || report.summary.exact_damage_projection_proven
        || report.summary.exact_operation_order_proven
        || report.summary.exact_integer_rounding_proven
        || report.summary.packet_conservation_proven
        || report.summary.formula_authority
        || report.summary.runtime_authority
        || report.summary.ui_display_authority
        || report.summary.provider_rdps_credit_allowed
        || report.examples.iter().any(|example| {
            example.segment_status_baseline_complete
                || example.controlled_counterfactual_pair_proven
                || example.formula_authority
        })
        || report.same_context_mismatch_examples.len() > EXAMPLE_LIMIT
        || report.same_context_mismatch_examples.iter().any(|example| {
            example.segment_status_baseline_complete
                || example.controlled_counterfactual_pair_proven
                || example.formula_authority
        })
        || report.same_context_mismatch_examples.first().is_some_and(|example| {
            Some(
                example
                    .residual_observed_state_dimensions_after_configured_endpoint_443_474_and_target_current_hp_exclusion,
            ) != report
                .summary
                .minimum_residual_observed_state_dimensions_after_configured_endpoint_443_474_and_target_current_hp_exclusion
        })
    {
        return Err(
            "transition counterfactual audit totals or authority flags are inconsistent".into(),
        );
    }
    Ok(())
}

fn verify_input_receipts(report: &AuditReport) -> Result<(), Box<dyn Error>> {
    verify_file_receipt(&report.inputs.gap_window_audit)?;
    for receipt in &report.inputs.source_rlogs {
        let path = PathBuf::from(&receipt.path);
        if fs::metadata(&path)?.len() != receipt.bytes || sha256_file(&path)? != receipt.sha256 {
            return Err(format!("source RLOG receipt changed: {}", path.display()).into());
        }
    }
    Ok(())
}

fn verify_file_receipt(receipt: &FileReceipt) -> Result<(), Box<dyn Error>> {
    let path = PathBuf::from(&receipt.path);
    if fs::metadata(&path)?.len() != receipt.bytes || sha256_file(&path)? != receipt.sha256 {
        return Err(format!("input receipt changed: {}", path.display()).into());
    }
    Ok(())
}

fn verify_embedded_digest(value: &Value, label: &str) -> Result<(), Box<dyn Error>> {
    let recorded = value
        .get("content_sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{label} lacks content_sha256"))?;
    let mut without_hash = value.clone();
    without_hash
        .as_object_mut()
        .ok_or_else(|| format!("{label} is not an object"))?
        .remove("content_sha256");
    let expected = format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&without_hash)?)
    );
    if recorded != expected {
        return Err(format!("{label} content digest mismatch").into());
    }
    Ok(())
}

fn report_digest(report: &AuditReport) -> Result<String, serde_json::Error> {
    let mut value = serde_json::to_value(report)?;
    let object = value
        .as_object_mut()
        .expect("serialized report must be an object");
    object.remove("content_sha256");
    Ok(format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&value)?)
    ))
}

fn file_receipt(path: &Path) -> Result<FileReceipt, Box<dyn Error>> {
    Ok(FileReceipt {
        path: display_path(path),
        bytes: fs::metadata(path)?.len(),
        sha256: sha256_file(path)?,
    })
}

fn sha256_file(path: &Path) -> Result<String, Box<dyn Error>> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    let digest: [u8; 32] = digest.finalize().into();
    Ok(hex_sha256(&digest))
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn arguments() -> Result<Command, String> {
    let mut values = env::args_os().skip(1);
    let command = values
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or_else(usage)?;
    let mut options = HashMap::<String, String>::new();
    while let Some(flag) = values.next() {
        let flag = flag.into_string().map_err(|_| usage())?;
        let value = values
            .next()
            .ok_or_else(usage)?
            .into_string()
            .map_err(|_| usage())?;
        options.insert(flag, value);
    }
    match command.as_str() {
        "generate" => Ok(Command::Generate {
            build: take_required(&mut options, "--build")?,
            gap_window_audit: PathBuf::from(take_required(&mut options, "--gap-window-audit")?),
            effect_id: take_required(&mut options, "--effect-id")?
                .parse()
                .map_err(|_| usage())?,
            damage_relationship: DamageRelationship::parse(&take_required(
                &mut options,
                "--damage-relationship",
            )?)?,
            diagnostic_endpoint_attribute_ids: parse_attribute_ids(&take_required(
                &mut options,
                "--diagnostic-endpoint-attribute-ids",
            )?)?,
            output: PathBuf::from(take_required(&mut options, "--output")?),
            max_pair_gap_micros: options
                .remove("--max-pair-gap-micros")
                .map(|value| value.parse().map_err(|_| usage()))
                .transpose()?
                .unwrap_or(DEFAULT_MAX_PAIR_GAP_MICROS),
        }),
        "verify" => Ok(Command::Verify {
            input: PathBuf::from(take_required(&mut options, "--input")?),
        }),
        _ => Err(usage()),
    }
}

fn take_required(options: &mut HashMap<String, String>, name: &str) -> Result<String, String> {
    options.remove(name).ok_or_else(usage)
}

fn parse_attribute_ids(value: &str) -> Result<Vec<i32>, String> {
    let mut ids = value
        .split(',')
        .map(|part| part.trim().parse::<i32>().map_err(|_| usage()))
        .collect::<Result<Vec<_>, _>>()?;
    if ids.is_empty() || ids.iter().any(|id| *id <= 0) {
        return Err(usage());
    }
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

fn usage() -> String {
    "usage:\n  rlogs-bpsr-rlog-transition-counterfactual-audit generate --build <id> --gap-window-audit <json> --effect-id <id> --damage-relationship <source|target> --diagnostic-endpoint-attribute-ids <id,id,...> [--max-pair-gap-micros <n>] --output <json>\n  rlogs-bpsr-rlog-transition-counterfactual-audit verify --input <json>".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifier_rejects_formula_authority() {
        let mut report = AuditReport {
            schema_version: SCHEMA_VERSION,
            generated_by: GENERATED_BY.to_owned(),
            game_build: "1".to_owned(),
            effect_id: 1,
            damage_relationship: DamageRelationship::Source,
            policy: AuditPolicy {
                sealed_rlogs_are_streamed_one_event_at_a_time: true,
                every_data_gap_pause_and_run_boundary_resets_all_observed_state: true,
                only_same_segment_transition_adjacent_pairs_are_compared: true,
                exact_numeric_ids_and_build_are_authoritative: true,
                damage_relationship_is_explicit: true,
                packet_absence_is_not_zero: true,
                unknown_segment_baseline_statuses_are_preserved_as_unresolved: true,
                target_current_hp_exclusion_is_diagnostic_only: true,
                attribute_443_474_exclusion_is_diagnostic_only: true,
                configured_endpoint_attribute_exclusion_is_diagnostic_only: true,
                relative_spatial_relations_are_diagnostic_only: true,
                structurally_unobservable_remote_player_packets_are_not_acquisition_requirements:
                    true,
                candidate_pairs_never_grant_formula_or_runtime_authority: true,
                current_snapshots_are_never_backfilled_into_historical_segments: true,
                formula_authority: false,
                runtime_authority: false,
                ui_display_authority: false,
                provider_rdps_credit_allowed: false,
            },
            inputs: AuditInputs {
                gap_window_audit: FileReceipt {
                    path: "gap.json".to_owned(),
                    bytes: 1,
                    sha256: "sha256:0".to_owned(),
                },
                source_rlogs: Vec::new(),
            },
            search_contract: SearchContract {
                max_pair_gap_micros: 1,
                recent_samples_per_target: 1,
                exact_context_dimensions: vec!["context".to_owned()],
                exact_observed_state_dimensions: vec!["state".to_owned()],
                excluded_output_dimensions: vec!["output".to_owned()],
                strict_formula_pair_requirements_still_open: vec!["open".to_owned()],
                remote_player_packet_dependency: false,
                selected_effect_endpoint_role: "damage_actor".to_owned(),
                diagnostic_endpoint_attribute_ids: vec![13_100],
            },
            summary: AuditSummary::default(),
            sessions: Vec::new(),
            examples: Vec::new(),
            same_context_mismatch_examples: Vec::new(),
            blockers: vec!["not formula proof".to_owned()],
            content_sha256: String::new(),
        };
        report.policy.formula_authority = true;
        report.content_sha256 = report_digest(&report).unwrap();
        assert!(verify_report(&report).is_err());
    }

    #[test]
    fn damage_relationship_selects_source_or_target_without_allegiance_assumptions() {
        let source = EntityRef {
            actor_id: rlogs_events::ActorId(1),
            entity_uuid: rlogs_events::EntityUuid(10),
        };
        let target = EntityRef {
            actor_id: rlogs_events::ActorId(2),
            entity_uuid: rlogs_events::EntityUuid(20),
        };
        assert_eq!(DamageRelationship::Source.select(source, target), source);
        assert_eq!(DamageRelationship::Target.select(source, target), target);
        assert_eq!(DamageRelationship::Source.endpoint_role(), "damage_actor");
        assert_eq!(DamageRelationship::Target.endpoint_role(), "damage_target");
    }

    #[test]
    fn diagnostic_attribute_ids_are_positive_sorted_and_deduplicated() {
        assert_eq!(
            parse_attribute_ids("13102, 13100,13101,13100").unwrap(),
            vec![13_100, 13_101, 13_102]
        );
        assert!(parse_attribute_ids("13100,0").is_err());
        assert!(parse_attribute_ids("").is_err());
    }

    #[test]
    fn damage_action_identity_preserves_exact_numeric_packet_selectors() {
        let damage = DamageEvent {
            source: EntityRef {
                actor_id: rlogs_events::ActorId(1),
                entity_uuid: rlogs_events::EntityUuid(10),
            },
            direct_source: None,
            target: EntityRef {
                actor_id: rlogs_events::ActorId(2),
                entity_uuid: rlogs_events::EntityUuid(20),
            },
            ability: Some(rlogs_events::AbilityId(2_352)),
            amount: 1,
            actual_amount: None,
            hp_loss: None,
            shield_loss: None,
            hit_event_id: Some(4),
            damage_source: Some(2),
            damage_type: Some(3),
            flags: rlogs_events::DamageFlags::default(),
            packet: DamagePacketDetail {
                owner_id: Some(2_352),
                property: Some(5),
                normal_hit: Some(true),
                passive_uuid: Some(7),
                damage_mode: Some(6),
                skill_effect_component_index: Some(0),
                skill_effect_component_count: Some(1),
                ..DamagePacketDetail::default()
            },
        };

        assert_eq!(
            damage_action_identity(&damage),
            "ability=2352;hit_event=4;owner=2352;property=5;damage_source=2;damage_type=3;damage_mode=6;normal_hit=true;passive_uuid=7;component_index=0;component_count=1"
        );
    }

    #[test]
    fn spatial_relation_diagnostic_separates_vector_and_distance_equality() {
        let prior = Position3 {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        };
        let mut audit = SpatialRelationAudit::default();
        observe_spatial_relation_comparison(&mut audit, prior, prior);
        observe_spatial_relation_comparison(
            &mut audit,
            prior,
            Position3 {
                x: -1.0,
                y: 2.0,
                z: 3.0,
            },
        );
        assert_eq!(audit.both_relations_complete_count, 2);
        assert_eq!(audit.exact_displacement_vector_equal_count, 1);
        assert_eq!(audit.exact_squared_distance_equal_count, 2);
        assert_eq!(audit.absolute_distance_delta_le_0_000001_count, 2);
        assert!(!audit.spatial_state_safe_to_exclude_from_counterfactual_matching);
        assert!(!audit.formula_authority);
    }
}
