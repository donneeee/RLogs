#![allow(clippy::field_reassign_with_default, clippy::type_complexity)]

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    env,
    error::Error,
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{
    CanonicalEvent, DamageEvent, EntityAttributeUpdateKind, EntityAttributeValue, EntityRef,
    EvidenceSource, TimelineEventKind,
};
use rlogs_game_bpsr::decode_known_entity_attribute_value;
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 4;
const GENERATED_BY: &str = "rlogs-bpsr-rlog-opaque-attribute-audit";
const EXAMPLE_LIMIT_PER_ATTRIBUTE: usize = 32;
const RAW_PREFIX_LIMIT: usize = 32;

#[derive(Debug)]
enum Command {
    Generate {
        build: String,
        gap_window_audit: PathBuf,
        attribute_ids: Vec<i32>,
        output: PathBuf,
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
    gap_window_effect_id: i64,
    gap_window_damage_relationship: DamageRelationship,
    attribute_ids: Vec<i32>,
    policy: AuditPolicy,
    inputs: AuditInputs,
    summary: AuditSummary,
    sessions: Vec<SessionAudit>,
    raw_value_examples: Vec<RawValueExample>,
    same_wire_damage_examples: Vec<SameWireDamageExample>,
    blockers: Vec<String>,
    content_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuditPolicy {
    sealed_rlogs_are_streamed_one_event_at_a_time: bool,
    every_data_gap_pause_and_run_boundary_resets_prior_attribute_state: bool,
    wire_adjacency_requires_exact_capture_connection_and_stream_identity: bool,
    gap_window_damage_relationship_is_explicit_and_scope_only: bool,
    retained_raw_bytes_are_redecoded_with_the_current_exact_id_allowlist: bool,
    generic_varint_interpretation_is_diagnostic_only: bool,
    protobuf_pair_collection_interpretation_is_diagnostic_only: bool,
    opaque_attributes_are_not_excluded_without_semantic_proof: bool,
    packet_absence_is_not_zero: bool,
    structurally_unobservable_remote_player_packets_are_not_acquisition_requirements: bool,
    remote_player_packet_dependency: bool,
    formula_input_semantics_proven: bool,
    damage_consequence_semantics_proven: bool,
    safe_to_exclude_from_counterfactual_matching: bool,
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct AuditSummary {
    source_rlog_count: usize,
    source_rlog_bytes: u64,
    canonical_event_count: u64,
    data_gap_count: u64,
    recorder_pause_count: u64,
    run_boundary_count: u64,
    attributes: BTreeMap<i32, AttributeAudit>,
    formula_input_semantics_proven: bool,
    damage_consequence_semantics_proven: bool,
    safe_to_exclude_from_counterfactual_matching: bool,
    formula_authority: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct SessionAudit {
    path: String,
    session_id: String,
    event_count: u64,
    data_gap_count: u64,
    recorder_pause_count: u64,
    run_boundary_count: u64,
    attributes: BTreeMap<i32, AttributeAudit>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct AttributeAudit {
    observation_count: u64,
    distinct_actor_count: usize,
    snapshot_observation_count: u64,
    delta_observation_count: u64,
    unknown_update_kind_observation_count: u64,
    empty_raw_value_count: u64,
    raw_length_counts: BTreeMap<usize, u64>,
    canonical_decoder_unresolved_count: u64,
    canonical_decoder_integer_count: u64,
    canonical_decoder_text_count: u64,
    canonical_decoder_position_count: u64,
    diagnostic_unsigned_varint_valid_count: u64,
    diagnostic_unsigned_varint_invalid_count: u64,
    diagnostic_unsigned_varint_min: Option<u64>,
    diagnostic_unsigned_varint_max: Option<u64>,
    diagnostic_signed_prior_delta_counts: BTreeMap<i64, u64>,
    diagnostic_pair_collection_valid_count: u64,
    diagnostic_pair_collection_invalid_count: u64,
    diagnostic_pair_entry_count: u64,
    diagnostic_pair_entry_min_per_observation: Option<usize>,
    diagnostic_pair_entry_max_per_observation: Option<usize>,
    diagnostic_pair_key_min: Option<u64>,
    diagnostic_pair_key_max: Option<u64>,
    diagnostic_pair_value_min: Option<u64>,
    diagnostic_pair_value_max: Option<u64>,
    diagnostic_distinct_pair_key_count: usize,
    diagnostic_pair_entries_with_session_entity_key: u64,
    diagnostic_distinct_pair_keys_matching_session_entities: usize,
    prior_value_change_count: u64,
    prior_value_repeat_count: u64,
    first_observation_after_reset_count: u64,
    same_wire_related_damage_pairs: u64,
    same_wire_attribute_before_damage_pairs: u64,
    same_wire_damage_before_attribute_pairs: u64,
    same_wire_source_role_pairs: u64,
    same_wire_direct_source_role_pairs: u64,
    same_wire_target_role_pairs: u64,
    diagnostic_value_equals_damage_amount_pairs: u64,
    diagnostic_value_equals_actual_amount_pairs: u64,
    diagnostic_value_equals_hp_loss_pairs: u64,
    diagnostic_value_equals_shield_loss_pairs: u64,
    formula_input_semantics_proven: bool,
    damage_consequence_semantics_proven: bool,
    safe_to_exclude_from_counterfactual_matching: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RawValueExample {
    rlog: String,
    session_id: String,
    segment_index: u64,
    attribute_id: i32,
    actor_id: u64,
    entity_uuid: i64,
    envelope_sequence: u64,
    observed_micros: u64,
    update_kind: EntityAttributeUpdateKind,
    raw_length: usize,
    raw_sha256: String,
    raw_hex_prefix: String,
    raw_hex_prefix_truncated: bool,
    canonical_decoder_variant: String,
    diagnostic_unsigned_varint: Option<u64>,
    diagnostic_pair_entries: Option<Vec<(u64, u64)>>,
    formula_authority: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SameWireDamageExample {
    rlog: String,
    session_id: String,
    segment_index: u64,
    capture_sequence: u64,
    connection_id: u64,
    stream_id: u64,
    attribute_id: i32,
    attribute_actor_id: u64,
    attribute_entity_uuid: i64,
    attribute_sequence: u64,
    damage_sequence: u64,
    ordering: String,
    roles: Vec<String>,
    diagnostic_unsigned_varint: Option<u64>,
    diagnostic_signed_prior_delta: Option<i64>,
    damage_amount: i64,
    actual_amount: Option<i64>,
    hp_loss: Option<i64>,
    shield_loss: Option<i64>,
    formula_authority: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct GapWindowAudit {
    schema_version: u16,
    generated_by: String,
    game_build: String,
    effect_id: i64,
    damage_relationship: DamageRelationship,
    policy: GapWindowPolicy,
    sessions: Vec<GapSession>,
}

#[derive(Debug, Clone, Deserialize)]
struct GapWindowPolicy {
    damage_relationship_is_explicit: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DamageRelationship {
    Source,
    Target,
}

#[derive(Debug, Clone, Deserialize)]
struct GapSession {
    path: String,
    bytes: u64,
    sealed_content_sha256: String,
    event_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WireKey {
    capture_sequence: u64,
    connection_id: u64,
    stream_id: u64,
}

#[derive(Debug, Clone)]
struct WireAttribute {
    segment_index: u64,
    attribute_id: i32,
    actor: EntityRef,
    sequence: u64,
    diagnostic_unsigned_varint: Option<u64>,
    diagnostic_signed_prior_delta: Option<i64>,
}

#[derive(Debug, Clone)]
struct WireDamage {
    segment_index: u64,
    sequence: u64,
    source: EntityRef,
    direct_source: Option<EntityRef>,
    target: EntityRef,
    amount: i64,
    actual_amount: Option<i64>,
    hp_loss: Option<i64>,
    shield_loss: Option<i64>,
}

#[derive(Debug, Default)]
struct ScanState {
    segment_index: u64,
    session: SessionAudit,
    selected_ids: BTreeSet<i32>,
    actors: BTreeMap<i32, BTreeSet<(u64, i64)>>,
    known_entity_uuids: BTreeSet<u64>,
    diagnostic_pair_key_counts: BTreeMap<i32, BTreeMap<u64, u64>>,
    prior_values: HashMap<(i32, u64, i64), Vec<u8>>,
    raw_example_keys: BTreeSet<(i32, String)>,
    raw_examples: Vec<RawValueExample>,
    same_wire_examples: Vec<SameWireDamageExample>,
    wire_key: Option<WireKey>,
    wire_attributes: Vec<WireAttribute>,
    wire_damages: Vec<WireDamage>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("RLOG opaque attribute audit failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    match arguments()? {
        Command::Generate {
            build,
            gap_window_audit,
            attribute_ids,
            output,
        } => generate(&build, &gap_window_audit, &attribute_ids, &output),
        Command::Verify { input } => {
            let report: AuditReport = serde_json::from_reader(BufReader::new(File::open(&input)?))?;
            verify_report(&report)?;
            verify_input_receipts(&report)?;
            println!(
                "RLOG opaque attribute audit verified for build {} IDs {:?}; safe exclusion=false.",
                report.game_build, report.attribute_ids
            );
            Ok(())
        }
    }
}

fn generate(
    build: &str,
    gap_window_audit_path: &Path,
    attribute_ids: &[i32],
    output: &Path,
) -> Result<(), Box<dyn Error>> {
    if build.is_empty()
        || !build.bytes().all(|value| value.is_ascii_digit())
        || attribute_ids.is_empty()
        || attribute_ids.iter().any(|value| *value <= 0)
        || attribute_ids.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err("build or sorted unique attribute IDs are invalid".into());
    }
    let gap_value: Value =
        serde_json::from_reader(BufReader::new(File::open(gap_window_audit_path)?))?;
    verify_embedded_digest(&gap_value, "RLOG gap-window audit")?;
    let gap: GapWindowAudit = serde_json::from_value(gap_value)?;
    if gap.schema_version != 3
        || gap.generated_by != "rlogs-bpsr-rlog-gap-window-audit"
        || gap.game_build != build
        || gap.effect_id <= 0
        || !gap.policy.damage_relationship_is_explicit
        || gap.sessions.is_empty()
    {
        return Err("gap-window audit identity does not match the requested build".into());
    }

    let mut sessions = Vec::with_capacity(gap.sessions.len());
    let mut receipts = Vec::with_capacity(gap.sessions.len());
    let mut raw_examples = Vec::new();
    let mut wire_examples = Vec::new();
    for expected in &gap.sessions {
        let path = PathBuf::from(&expected.path);
        if fs::metadata(&path)?.len() != expected.bytes {
            return Err(format!("source RLOG byte length changed: {}", path.display()).into());
        }
        let (session, mut session_raw, mut session_wire, sealed_content_sha256) =
            audit_rlog(&path, attribute_ids)?;
        if session.event_count != expected.event_count
            || sealed_content_sha256 != expected.sealed_content_sha256
        {
            return Err(format!("source RLOG seal changed: {}", path.display()).into());
        }
        receipts.push(RlogReceipt {
            path: display_path(&path),
            bytes: expected.bytes,
            sha256: sha256_file(&path)?,
            sealed_content_sha256,
            event_count: expected.event_count,
        });
        raw_examples.append(&mut session_raw);
        wire_examples.append(&mut session_wire);
        sessions.push(session);
    }
    raw_examples.sort_by(|left, right| {
        left.attribute_id
            .cmp(&right.attribute_id)
            .then_with(|| left.rlog.cmp(&right.rlog))
            .then_with(|| left.envelope_sequence.cmp(&right.envelope_sequence))
    });
    retain_examples_per_attribute(&mut raw_examples, EXAMPLE_LIMIT_PER_ATTRIBUTE, |example| {
        example.attribute_id
    });
    wire_examples.sort_by(|left, right| {
        left.attribute_id
            .cmp(&right.attribute_id)
            .then_with(|| left.rlog.cmp(&right.rlog))
            .then_with(|| left.attribute_sequence.cmp(&right.attribute_sequence))
            .then_with(|| left.damage_sequence.cmp(&right.damage_sequence))
    });
    retain_examples_per_attribute(&mut wire_examples, EXAMPLE_LIMIT_PER_ATTRIBUTE, |example| {
        example.attribute_id
    });

    let summary = summarize(&sessions, &receipts, attribute_ids);
    let mut report = AuditReport {
        schema_version: SCHEMA_VERSION,
        generated_by: GENERATED_BY.to_owned(),
        game_build: build.to_owned(),
        gap_window_effect_id: gap.effect_id,
        gap_window_damage_relationship: gap.damage_relationship,
        attribute_ids: attribute_ids.to_vec(),
        policy: AuditPolicy {
            sealed_rlogs_are_streamed_one_event_at_a_time: true,
            every_data_gap_pause_and_run_boundary_resets_prior_attribute_state: true,
            wire_adjacency_requires_exact_capture_connection_and_stream_identity: true,
            gap_window_damage_relationship_is_explicit_and_scope_only: true,
            retained_raw_bytes_are_redecoded_with_the_current_exact_id_allowlist: true,
            generic_varint_interpretation_is_diagnostic_only: true,
            protobuf_pair_collection_interpretation_is_diagnostic_only: true,
            opaque_attributes_are_not_excluded_without_semantic_proof: true,
            packet_absence_is_not_zero: true,
            structurally_unobservable_remote_player_packets_are_not_acquisition_requirements: true,
            remote_player_packet_dependency: false,
            formula_input_semantics_proven: false,
            damage_consequence_semantics_proven: false,
            safe_to_exclude_from_counterfactual_matching: false,
            formula_authority: false,
            runtime_authority: false,
            ui_display_authority: false,
            provider_rdps_credit_allowed: false,
        },
        inputs: AuditInputs {
            gap_window_audit: file_receipt(gap_window_audit_path)?,
            source_rlogs: receipts,
        },
        summary,
        sessions,
        raw_value_examples: raw_examples,
        same_wire_damage_examples: wire_examples,
        blockers: vec![
            "an exact-build semantic identity does not by itself prove that an attribute has no damage consequence".to_owned(),
            "diagnostic raw-value shapes and wire timing cannot alone prove formula-input or damage-consequence semantics".to_owned(),
            "unexcluded attributes remain exact counterfactual matching dimensions".to_owned(),
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
        "Audited {} sealed RLOGs for opaque attributes {:?}; safe exclusion=false.",
        report.summary.source_rlog_count, report.attribute_ids
    );
    println!("wrote {}", output.display());
    Ok(())
}

fn audit_rlog(
    path: &Path,
    attribute_ids: &[i32],
) -> Result<
    (
        SessionAudit,
        Vec<RawValueExample>,
        Vec<SameWireDamageExample>,
        String,
    ),
    Box<dyn Error>,
> {
    let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
    let session_id = reader.header().session_id.clone();
    let mut state = ScanState {
        selected_ids: attribute_ids.iter().copied().collect(),
        ..ScanState::default()
    };
    state.session.path = display_path(path);
    state.session.session_id = session_id.clone();
    for id in attribute_ids {
        state
            .session
            .attributes
            .insert(*id, AttributeAudit::default());
    }

    while let Some(envelope) = reader.next_event()? {
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            continue;
        };
        select_wire_group(&mut state, wire_key(&envelope.provenance));
        match &timeline.kind {
            TimelineEventKind::EntityAttributes(event) => {
                for attribute in &event.attributes {
                    if state.selected_ids.contains(&attribute.attribute_id) {
                        observe_attribute(
                            &mut state,
                            path,
                            &session_id,
                            envelope.sequence,
                            envelope.time.observed_micros,
                            event.actor,
                            event.update_kind,
                            attribute.attribute_id,
                            &attribute.raw_value,
                            attribute
                                .decoded
                                .as_ref()
                                .cloned()
                                .or_else(|| {
                                    decode_known_entity_attribute_value(
                                        attribute.attribute_id,
                                        &attribute.raw_value,
                                    )
                                })
                                .as_ref(),
                        );
                    }
                }
            }
            TimelineEventKind::Damage(damage) => {
                observe_damage(&mut state, path, &session_id, envelope.sequence, damage)
            }
            TimelineEventKind::DataGap(_) => {
                state.session.data_gap_count = state.session.data_gap_count.saturating_add(1);
                reset_state(&mut state);
            }
            TimelineEventKind::RecorderPause(_) => {
                state.session.recorder_pause_count =
                    state.session.recorder_pause_count.saturating_add(1);
                reset_state(&mut state);
            }
            TimelineEventKind::RunBoundary { .. } => {
                state.session.run_boundary_count =
                    state.session.run_boundary_count.saturating_add(1);
                reset_state(&mut state);
            }
            _ => {}
        }
    }
    let replay = reader
        .summary()
        .ok_or("sealed RLOG replay summary is missing")?;
    state.session.event_count = replay.event_count;
    for id in attribute_ids {
        let audit = state
            .session
            .attributes
            .get_mut(id)
            .expect("requested attribute audit exists");
        audit.distinct_actor_count = state.actors.get(id).map(BTreeSet::len).unwrap_or(0);
        let pair_keys = state.diagnostic_pair_key_counts.get(id);
        audit.diagnostic_distinct_pair_key_count = pair_keys.map(BTreeMap::len).unwrap_or(0);
        audit.diagnostic_pair_entries_with_session_entity_key = pair_keys
            .into_iter()
            .flat_map(|counts| counts.iter())
            .filter(|(key, _)| state.known_entity_uuids.contains(key))
            .map(|(_, count)| count)
            .sum();
        audit.diagnostic_distinct_pair_keys_matching_session_entities = pair_keys
            .into_iter()
            .flat_map(|counts| counts.keys())
            .filter(|key| state.known_entity_uuids.contains(key))
            .count();
    }
    Ok((
        state.session,
        state.raw_examples,
        state.same_wire_examples,
        replay.content_sha256.clone(),
    ))
}

#[allow(clippy::too_many_arguments)]
fn observe_attribute(
    state: &mut ScanState,
    path: &Path,
    session_id: &str,
    sequence: u64,
    observed_micros: u64,
    actor: EntityRef,
    update_kind: EntityAttributeUpdateKind,
    attribute_id: i32,
    raw: &[u8],
    decoded: Option<&EntityAttributeValue>,
) {
    let diagnostic = diagnostic_unsigned_varint(raw);
    let diagnostic_pairs = diagnostic_pair_collection(raw);
    let audit = state
        .session
        .attributes
        .get_mut(&attribute_id)
        .expect("requested attribute audit exists");
    audit.observation_count = audit.observation_count.saturating_add(1);
    match update_kind {
        EntityAttributeUpdateKind::Snapshot => {
            audit.snapshot_observation_count = audit.snapshot_observation_count.saturating_add(1)
        }
        EntityAttributeUpdateKind::Delta => {
            audit.delta_observation_count = audit.delta_observation_count.saturating_add(1)
        }
        EntityAttributeUpdateKind::Unknown => {
            audit.unknown_update_kind_observation_count = audit
                .unknown_update_kind_observation_count
                .saturating_add(1)
        }
    }
    if raw.is_empty() {
        audit.empty_raw_value_count = audit.empty_raw_value_count.saturating_add(1);
    }
    *audit.raw_length_counts.entry(raw.len()).or_default() += 1;
    match decoded {
        None => {
            audit.canonical_decoder_unresolved_count =
                audit.canonical_decoder_unresolved_count.saturating_add(1)
        }
        Some(EntityAttributeValue::Integer(_)) => {
            audit.canonical_decoder_integer_count =
                audit.canonical_decoder_integer_count.saturating_add(1)
        }
        Some(EntityAttributeValue::Text(_)) => {
            audit.canonical_decoder_text_count =
                audit.canonical_decoder_text_count.saturating_add(1)
        }
        Some(EntityAttributeValue::Position { .. }) => {
            audit.canonical_decoder_position_count =
                audit.canonical_decoder_position_count.saturating_add(1)
        }
    }
    match diagnostic {
        Some(value) => {
            audit.diagnostic_unsigned_varint_valid_count = audit
                .diagnostic_unsigned_varint_valid_count
                .saturating_add(1);
            audit.diagnostic_unsigned_varint_min = Some(
                audit
                    .diagnostic_unsigned_varint_min
                    .map_or(value, |current| current.min(value)),
            );
            audit.diagnostic_unsigned_varint_max = Some(
                audit
                    .diagnostic_unsigned_varint_max
                    .map_or(value, |current| current.max(value)),
            );
        }
        None => {
            audit.diagnostic_unsigned_varint_invalid_count = audit
                .diagnostic_unsigned_varint_invalid_count
                .saturating_add(1)
        }
    }
    match &diagnostic_pairs {
        Some(entries) => {
            audit.diagnostic_pair_collection_valid_count = audit
                .diagnostic_pair_collection_valid_count
                .saturating_add(1);
            audit.diagnostic_pair_entry_count = audit
                .diagnostic_pair_entry_count
                .saturating_add(entries.len() as u64);
            audit.diagnostic_pair_entry_min_per_observation = Some(
                audit
                    .diagnostic_pair_entry_min_per_observation
                    .map_or(entries.len(), |current| current.min(entries.len())),
            );
            audit.diagnostic_pair_entry_max_per_observation = Some(
                audit
                    .diagnostic_pair_entry_max_per_observation
                    .map_or(entries.len(), |current| current.max(entries.len())),
            );
            for (key, value) in entries {
                audit.diagnostic_pair_key_min = Some(
                    audit
                        .diagnostic_pair_key_min
                        .map_or(*key, |current| current.min(*key)),
                );
                audit.diagnostic_pair_key_max = Some(
                    audit
                        .diagnostic_pair_key_max
                        .map_or(*key, |current| current.max(*key)),
                );
                audit.diagnostic_pair_value_min = Some(
                    audit
                        .diagnostic_pair_value_min
                        .map_or(*value, |current| current.min(*value)),
                );
                audit.diagnostic_pair_value_max = Some(
                    audit
                        .diagnostic_pair_value_max
                        .map_or(*value, |current| current.max(*value)),
                );
                *state
                    .diagnostic_pair_key_counts
                    .entry(attribute_id)
                    .or_default()
                    .entry(*key)
                    .or_default() += 1;
            }
        }
        None => {
            audit.diagnostic_pair_collection_invalid_count = audit
                .diagnostic_pair_collection_invalid_count
                .saturating_add(1)
        }
    }
    let actor_key = (actor.actor_id.0, actor.entity_uuid.0);
    if let Ok(entity_uuid) = u64::try_from(actor.entity_uuid.0) {
        state.known_entity_uuids.insert(entity_uuid);
    }
    state
        .actors
        .entry(attribute_id)
        .or_default()
        .insert(actor_key);
    let previous_raw = state
        .prior_values
        .insert((attribute_id, actor_key.0, actor_key.1), raw.to_vec());
    match &previous_raw {
        None => {
            audit.first_observation_after_reset_count =
                audit.first_observation_after_reset_count.saturating_add(1)
        }
        Some(previous) if previous.as_slice() == raw => {
            audit.prior_value_repeat_count = audit.prior_value_repeat_count.saturating_add(1)
        }
        Some(_) => {
            audit.prior_value_change_count = audit.prior_value_change_count.saturating_add(1)
        }
    }
    let signed_prior_delta = previous_raw
        .as_deref()
        .and_then(diagnostic_unsigned_varint)
        .zip(diagnostic)
        .and_then(|(previous, current)| {
            i128::from(current)
                .checked_sub(i128::from(previous))
                .and_then(|delta| i64::try_from(delta).ok())
        });
    if let Some(delta) = signed_prior_delta {
        *audit
            .diagnostic_signed_prior_delta_counts
            .entry(delta)
            .or_default() += 1;
    }

    let raw_sha256 = format!("sha256:{:x}", Sha256::digest(raw));
    if state
        .raw_examples
        .iter()
        .filter(|example| example.attribute_id == attribute_id)
        .count()
        < EXAMPLE_LIMIT_PER_ATTRIBUTE
        && state
            .raw_example_keys
            .insert((attribute_id, raw_sha256.clone()))
    {
        state.raw_examples.push(RawValueExample {
            rlog: display_path(path),
            session_id: session_id.to_owned(),
            segment_index: state.segment_index,
            attribute_id,
            actor_id: actor.actor_id.0,
            entity_uuid: actor.entity_uuid.0,
            envelope_sequence: sequence,
            observed_micros,
            update_kind,
            raw_length: raw.len(),
            raw_sha256,
            raw_hex_prefix: hex_bytes(&raw[..raw.len().min(RAW_PREFIX_LIMIT)]),
            raw_hex_prefix_truncated: raw.len() > RAW_PREFIX_LIMIT,
            canonical_decoder_variant: decoded_variant(decoded).to_owned(),
            diagnostic_unsigned_varint: diagnostic,
            diagnostic_pair_entries: diagnostic_pairs.clone(),
            formula_authority: false,
        });
    }

    let occurrence = WireAttribute {
        segment_index: state.segment_index,
        attribute_id,
        actor,
        sequence,
        diagnostic_unsigned_varint: diagnostic,
        diagnostic_signed_prior_delta: signed_prior_delta,
    };
    for damage in state.wire_damages.clone() {
        observe_same_wire_relation(
            state,
            path,
            session_id,
            &occurrence,
            &damage,
            "damage_before_attribute",
        );
    }
    state.wire_attributes.push(occurrence);
}

fn observe_damage(
    state: &mut ScanState,
    path: &Path,
    session_id: &str,
    sequence: u64,
    damage: &DamageEvent,
) {
    for entity in [
        Some(damage.source),
        damage.direct_source,
        Some(damage.target),
    ]
    .into_iter()
    .flatten()
    {
        if let Ok(entity_uuid) = u64::try_from(entity.entity_uuid.0) {
            state.known_entity_uuids.insert(entity_uuid);
        }
    }
    let occurrence = WireDamage {
        segment_index: state.segment_index,
        sequence,
        source: damage.source,
        direct_source: damage.direct_source,
        target: damage.target,
        amount: damage.amount,
        actual_amount: damage.actual_amount,
        hp_loss: damage.hp_loss,
        shield_loss: damage.shield_loss,
    };
    for attribute in state.wire_attributes.clone() {
        observe_same_wire_relation(
            state,
            path,
            session_id,
            &attribute,
            &occurrence,
            "attribute_before_damage",
        );
    }
    state.wire_damages.push(occurrence);
}

fn observe_same_wire_relation(
    state: &mut ScanState,
    path: &Path,
    session_id: &str,
    attribute: &WireAttribute,
    damage: &WireDamage,
    ordering: &str,
) {
    if attribute.segment_index != damage.segment_index {
        return;
    }
    let mut roles = Vec::new();
    if attribute.actor == damage.source {
        roles.push("source");
    }
    if damage.direct_source == Some(attribute.actor) {
        roles.push("direct_source");
    }
    if attribute.actor == damage.target {
        roles.push("target");
    }
    if roles.is_empty() {
        return;
    }
    let audit = state
        .session
        .attributes
        .get_mut(&attribute.attribute_id)
        .expect("requested attribute audit exists");
    audit.same_wire_related_damage_pairs = audit.same_wire_related_damage_pairs.saturating_add(1);
    if ordering == "attribute_before_damage" {
        audit.same_wire_attribute_before_damage_pairs = audit
            .same_wire_attribute_before_damage_pairs
            .saturating_add(1);
    } else {
        audit.same_wire_damage_before_attribute_pairs = audit
            .same_wire_damage_before_attribute_pairs
            .saturating_add(1);
    }
    if roles.contains(&"source") {
        audit.same_wire_source_role_pairs = audit.same_wire_source_role_pairs.saturating_add(1);
    }
    if roles.contains(&"direct_source") {
        audit.same_wire_direct_source_role_pairs =
            audit.same_wire_direct_source_role_pairs.saturating_add(1);
    }
    if roles.contains(&"target") {
        audit.same_wire_target_role_pairs = audit.same_wire_target_role_pairs.saturating_add(1);
    }
    if let Some(value) = attribute.diagnostic_unsigned_varint {
        let signed_value = i64::try_from(value).ok();
        if signed_value == Some(damage.amount) {
            audit.diagnostic_value_equals_damage_amount_pairs = audit
                .diagnostic_value_equals_damage_amount_pairs
                .saturating_add(1);
        }
        if signed_value.is_some_and(|value| damage.actual_amount == Some(value)) {
            audit.diagnostic_value_equals_actual_amount_pairs = audit
                .diagnostic_value_equals_actual_amount_pairs
                .saturating_add(1);
        }
        if signed_value.is_some_and(|value| damage.hp_loss == Some(value)) {
            audit.diagnostic_value_equals_hp_loss_pairs = audit
                .diagnostic_value_equals_hp_loss_pairs
                .saturating_add(1);
        }
        if signed_value.is_some_and(|value| damage.shield_loss == Some(value)) {
            audit.diagnostic_value_equals_shield_loss_pairs = audit
                .diagnostic_value_equals_shield_loss_pairs
                .saturating_add(1);
        }
    }
    if state
        .same_wire_examples
        .iter()
        .filter(|example| example.attribute_id == attribute.attribute_id)
        .count()
        < EXAMPLE_LIMIT_PER_ATTRIBUTE
    {
        let key = state
            .wire_key
            .expect("wire relation requires wire identity");
        state.same_wire_examples.push(SameWireDamageExample {
            rlog: display_path(path),
            session_id: session_id.to_owned(),
            segment_index: attribute.segment_index,
            capture_sequence: key.capture_sequence,
            connection_id: key.connection_id,
            stream_id: key.stream_id,
            attribute_id: attribute.attribute_id,
            attribute_actor_id: attribute.actor.actor_id.0,
            attribute_entity_uuid: attribute.actor.entity_uuid.0,
            attribute_sequence: attribute.sequence,
            damage_sequence: damage.sequence,
            ordering: ordering.to_owned(),
            roles: roles.into_iter().map(str::to_owned).collect(),
            diagnostic_unsigned_varint: attribute.diagnostic_unsigned_varint,
            diagnostic_signed_prior_delta: attribute.diagnostic_signed_prior_delta,
            damage_amount: damage.amount,
            actual_amount: damage.actual_amount,
            hp_loss: damage.hp_loss,
            shield_loss: damage.shield_loss,
            formula_authority: false,
        });
    }
}

fn select_wire_group(state: &mut ScanState, key: Option<WireKey>) {
    if state.wire_key != key {
        state.wire_key = key;
        state.wire_attributes.clear();
        state.wire_damages.clear();
    }
}

fn reset_state(state: &mut ScanState) {
    state.segment_index = state.segment_index.saturating_add(1);
    state.prior_values.clear();
    state.wire_key = None;
    state.wire_attributes.clear();
    state.wire_damages.clear();
}

fn wire_key(provenance: &rlogs_events::EventProvenance) -> Option<WireKey> {
    match provenance.source {
        EvidenceSource::Wire {
            capture_sequence,
            connection_id,
            stream_id,
        } => Some(WireKey {
            capture_sequence,
            connection_id,
            stream_id,
        }),
        EvidenceSource::Derived { .. } | EvidenceSource::Manual { .. } => None,
    }
}

fn summarize(
    sessions: &[SessionAudit],
    receipts: &[RlogReceipt],
    attribute_ids: &[i32],
) -> AuditSummary {
    let mut attributes = attribute_ids
        .iter()
        .map(|id| (*id, AttributeAudit::default()))
        .collect::<BTreeMap<_, _>>();
    for session in sessions {
        for id in attribute_ids {
            merge_attribute_audit(
                attributes
                    .get_mut(id)
                    .expect("requested summary audit exists"),
                session
                    .attributes
                    .get(id)
                    .expect("requested session audit exists"),
            );
        }
    }
    AuditSummary {
        source_rlog_count: sessions.len(),
        source_rlog_bytes: receipts.iter().map(|receipt| receipt.bytes).sum(),
        canonical_event_count: sessions.iter().map(|session| session.event_count).sum(),
        data_gap_count: sessions.iter().map(|session| session.data_gap_count).sum(),
        recorder_pause_count: sessions
            .iter()
            .map(|session| session.recorder_pause_count)
            .sum(),
        run_boundary_count: sessions
            .iter()
            .map(|session| session.run_boundary_count)
            .sum(),
        attributes,
        formula_input_semantics_proven: false,
        damage_consequence_semantics_proven: false,
        safe_to_exclude_from_counterfactual_matching: false,
        formula_authority: false,
    }
}

fn retain_examples_per_attribute<T>(
    examples: &mut Vec<T>,
    limit: usize,
    attribute_id: impl Fn(&T) -> i32,
) {
    let mut retained = BTreeMap::<i32, usize>::new();
    examples.retain(|example| {
        let count = retained.entry(attribute_id(example)).or_default();
        if *count >= limit {
            false
        } else {
            *count += 1;
            true
        }
    });
}

fn merge_attribute_audit(target: &mut AttributeAudit, source: &AttributeAudit) {
    macro_rules! add {
        ($field:ident) => {
            target.$field = target.$field.saturating_add(source.$field)
        };
    }
    add!(observation_count);
    target.distinct_actor_count = target
        .distinct_actor_count
        .saturating_add(source.distinct_actor_count);
    add!(snapshot_observation_count);
    add!(delta_observation_count);
    add!(unknown_update_kind_observation_count);
    add!(empty_raw_value_count);
    for (length, count) in &source.raw_length_counts {
        *target.raw_length_counts.entry(*length).or_default() += count;
    }
    add!(canonical_decoder_unresolved_count);
    add!(canonical_decoder_integer_count);
    add!(canonical_decoder_text_count);
    add!(canonical_decoder_position_count);
    add!(diagnostic_unsigned_varint_valid_count);
    add!(diagnostic_unsigned_varint_invalid_count);
    if let Some(value) = source.diagnostic_unsigned_varint_min {
        target.diagnostic_unsigned_varint_min = Some(
            target
                .diagnostic_unsigned_varint_min
                .map_or(value, |current| current.min(value)),
        );
    }
    if let Some(value) = source.diagnostic_unsigned_varint_max {
        target.diagnostic_unsigned_varint_max = Some(
            target
                .diagnostic_unsigned_varint_max
                .map_or(value, |current| current.max(value)),
        );
    }
    for (delta, count) in &source.diagnostic_signed_prior_delta_counts {
        *target
            .diagnostic_signed_prior_delta_counts
            .entry(*delta)
            .or_default() += count;
    }
    add!(diagnostic_pair_collection_valid_count);
    add!(diagnostic_pair_collection_invalid_count);
    add!(diagnostic_pair_entry_count);
    merge_min(
        &mut target.diagnostic_pair_entry_min_per_observation,
        source.diagnostic_pair_entry_min_per_observation,
    );
    merge_max(
        &mut target.diagnostic_pair_entry_max_per_observation,
        source.diagnostic_pair_entry_max_per_observation,
    );
    merge_min(
        &mut target.diagnostic_pair_key_min,
        source.diagnostic_pair_key_min,
    );
    merge_max(
        &mut target.diagnostic_pair_key_max,
        source.diagnostic_pair_key_max,
    );
    merge_min(
        &mut target.diagnostic_pair_value_min,
        source.diagnostic_pair_value_min,
    );
    merge_max(
        &mut target.diagnostic_pair_value_max,
        source.diagnostic_pair_value_max,
    );
    target.diagnostic_distinct_pair_key_count = target
        .diagnostic_distinct_pair_key_count
        .saturating_add(source.diagnostic_distinct_pair_key_count);
    add!(diagnostic_pair_entries_with_session_entity_key);
    target.diagnostic_distinct_pair_keys_matching_session_entities = target
        .diagnostic_distinct_pair_keys_matching_session_entities
        .saturating_add(source.diagnostic_distinct_pair_keys_matching_session_entities);
    add!(prior_value_change_count);
    add!(prior_value_repeat_count);
    add!(first_observation_after_reset_count);
    add!(same_wire_related_damage_pairs);
    add!(same_wire_attribute_before_damage_pairs);
    add!(same_wire_damage_before_attribute_pairs);
    add!(same_wire_source_role_pairs);
    add!(same_wire_direct_source_role_pairs);
    add!(same_wire_target_role_pairs);
    add!(diagnostic_value_equals_damage_amount_pairs);
    add!(diagnostic_value_equals_actual_amount_pairs);
    add!(diagnostic_value_equals_hp_loss_pairs);
    add!(diagnostic_value_equals_shield_loss_pairs);
}

fn merge_min<T: Copy + Ord>(target: &mut Option<T>, source: Option<T>) {
    if let Some(value) = source {
        *target = Some(target.map_or(value, |current| current.min(value)));
    }
}

fn merge_max<T: Copy + Ord>(target: &mut Option<T>, source: Option<T>) {
    if let Some(value) = source {
        *target = Some(target.map_or(value, |current| current.max(value)));
    }
}

fn verify_report(report: &AuditReport) -> Result<(), Box<dyn Error>> {
    if report.schema_version != SCHEMA_VERSION
        || report.generated_by != GENERATED_BY
        || report.game_build.is_empty()
        || !report
            .game_build
            .bytes()
            .all(|value| value.is_ascii_digit())
        || report.gap_window_effect_id <= 0
        || report.attribute_ids.is_empty()
        || report.attribute_ids.iter().any(|value| *value <= 0)
        || report
            .attribute_ids
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
    {
        return Err("unsupported opaque attribute audit identity".into());
    }
    if !report.policy.sealed_rlogs_are_streamed_one_event_at_a_time
        || !report
            .policy
            .every_data_gap_pause_and_run_boundary_resets_prior_attribute_state
        || !report
            .policy
            .wire_adjacency_requires_exact_capture_connection_and_stream_identity
        || !report
            .policy
            .gap_window_damage_relationship_is_explicit_and_scope_only
        || !report
            .policy
            .retained_raw_bytes_are_redecoded_with_the_current_exact_id_allowlist
        || !report
            .policy
            .generic_varint_interpretation_is_diagnostic_only
        || !report
            .policy
            .protobuf_pair_collection_interpretation_is_diagnostic_only
        || !report
            .policy
            .opaque_attributes_are_not_excluded_without_semantic_proof
        || !report.policy.packet_absence_is_not_zero
        || !report
            .policy
            .structurally_unobservable_remote_player_packets_are_not_acquisition_requirements
        || report.policy.remote_player_packet_dependency
        || report.policy.formula_input_semantics_proven
        || report.policy.damage_consequence_semantics_proven
        || report.policy.safe_to_exclude_from_counterfactual_matching
        || report.policy.formula_authority
        || report.policy.runtime_authority
        || report.policy.ui_display_authority
        || report.policy.provider_rdps_credit_allowed
    {
        return Err("opaque attribute audit policy is unsafe".into());
    }
    if report.content_sha256 != report_digest(report)? {
        return Err("opaque attribute audit content digest mismatch".into());
    }
    let expected = summarize(
        &report.sessions,
        &report.inputs.source_rlogs,
        &report.attribute_ids,
    );
    if expected != report.summary
        || report.inputs.source_rlogs.len() != report.sessions.len()
        || report.raw_value_examples.len()
            > report.attribute_ids.len() * EXAMPLE_LIMIT_PER_ATTRIBUTE
        || report.same_wire_damage_examples.len()
            > report.attribute_ids.len() * EXAMPLE_LIMIT_PER_ATTRIBUTE
        || report
            .raw_value_examples
            .iter()
            .any(|example| example.formula_authority)
        || report
            .same_wire_damage_examples
            .iter()
            .any(|example| example.formula_authority)
        || report.summary.formula_input_semantics_proven
        || report.summary.damage_consequence_semantics_proven
        || report.summary.safe_to_exclude_from_counterfactual_matching
        || report.summary.formula_authority
    {
        return Err("opaque attribute audit totals or authority flags are inconsistent".into());
    }
    for session in &report.sessions {
        for id in &report.attribute_ids {
            verify_attribute(
                session
                    .attributes
                    .get(id)
                    .ok_or("requested attribute missing from session audit")?,
            )?;
        }
    }
    for id in &report.attribute_ids {
        verify_attribute(
            report
                .summary
                .attributes
                .get(id)
                .ok_or("requested attribute missing from summary audit")?,
        )?;
    }
    Ok(())
}

fn verify_attribute(audit: &AttributeAudit) -> Result<(), Box<dyn Error>> {
    let decoded = audit
        .canonical_decoder_unresolved_count
        .saturating_add(audit.canonical_decoder_integer_count)
        .saturating_add(audit.canonical_decoder_text_count)
        .saturating_add(audit.canonical_decoder_position_count);
    if audit.observation_count
        != audit
            .snapshot_observation_count
            .saturating_add(audit.delta_observation_count)
            .saturating_add(audit.unknown_update_kind_observation_count)
        || audit.observation_count != decoded
        || audit.observation_count
            != audit
                .diagnostic_unsigned_varint_valid_count
                .saturating_add(audit.diagnostic_unsigned_varint_invalid_count)
        || audit.observation_count
            != audit
                .diagnostic_pair_collection_valid_count
                .saturating_add(audit.diagnostic_pair_collection_invalid_count)
        || audit.same_wire_related_damage_pairs
            != audit
                .same_wire_attribute_before_damage_pairs
                .saturating_add(audit.same_wire_damage_before_attribute_pairs)
        || audit.formula_input_semantics_proven
        || audit.damage_consequence_semantics_proven
        || audit.safe_to_exclude_from_counterfactual_matching
    {
        return Err("opaque attribute counts or semantic flags are inconsistent".into());
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

fn diagnostic_pair_collection(bytes: &[u8]) -> Option<Vec<(u64, u64)>> {
    let mut offset = 0_usize;
    let mut entries = Vec::new();
    while offset < bytes.len() {
        let (outer_tag, tag_bytes) = read_varint(&bytes[offset..])?;
        offset = offset.checked_add(tag_bytes)?;
        if outer_tag != 10 {
            return None;
        }
        let (entry_length, length_bytes) = read_varint(&bytes[offset..])?;
        offset = offset.checked_add(length_bytes)?;
        let entry_length = usize::try_from(entry_length).ok()?;
        let entry_end = offset.checked_add(entry_length)?;
        let entry = bytes.get(offset..entry_end)?;
        offset = entry_end;

        let (key_tag, key_tag_bytes) = read_varint(entry)?;
        if key_tag != 8 {
            return None;
        }
        let (key, key_bytes) = read_varint(entry.get(key_tag_bytes..)?)?;
        let value_tag_offset = key_tag_bytes.checked_add(key_bytes)?;
        let (value_tag, value_tag_bytes) = read_varint(entry.get(value_tag_offset..)?)?;
        if value_tag != 16 {
            return None;
        }
        let value_offset = value_tag_offset.checked_add(value_tag_bytes)?;
        let (value, value_bytes) = read_varint(entry.get(value_offset..)?)?;
        if value_offset.checked_add(value_bytes)? != entry.len() {
            return None;
        }
        entries.push((key, value));
    }
    Some(entries)
}

fn read_varint(bytes: &[u8]) -> Option<(u64, usize)> {
    let mut value = 0_u64;
    for (index, byte) in bytes.iter().copied().take(10).enumerate() {
        if index == 9 && byte > 1 {
            return None;
        }
        value |= u64::from(byte & 0x7f) << (index * 7);
        if byte & 0x80 == 0 {
            return Some((value, index + 1));
        }
    }
    None
}

fn diagnostic_unsigned_varint(bytes: &[u8]) -> Option<u64> {
    if bytes.is_empty() {
        return Some(0);
    }
    let (value, consumed) = read_varint(bytes)?;
    (consumed == bytes.len()).then_some(value)
}

fn decoded_variant(value: Option<&EntityAttributeValue>) -> &'static str {
    match value {
        None => "unresolved",
        Some(EntityAttributeValue::Integer(_)) => "integer",
        Some(EntityAttributeValue::Text(_)) => "text",
        Some(EntityAttributeValue::Position { .. }) => "position",
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
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
    value
        .as_object_mut()
        .expect("serialized report must be an object")
        .remove("content_sha256");
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
    Ok(format!("sha256:{}", hex_bytes(&digest)))
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
        "generate" => {
            let mut attribute_ids = take_required(&mut options, "--attribute-ids")?
                .split(',')
                .map(|value| value.parse::<i32>().map_err(|_| usage()))
                .collect::<Result<Vec<_>, _>>()?;
            attribute_ids.sort_unstable();
            attribute_ids.dedup();
            Ok(Command::Generate {
                build: take_required(&mut options, "--build")?,
                gap_window_audit: PathBuf::from(take_required(&mut options, "--gap-window-audit")?),
                attribute_ids,
                output: PathBuf::from(take_required(&mut options, "--output")?),
            })
        }
        "verify" => Ok(Command::Verify {
            input: PathBuf::from(take_required(&mut options, "--input")?),
        }),
        _ => Err(usage()),
    }
}

fn take_required(options: &mut HashMap<String, String>, name: &str) -> Result<String, String> {
    options.remove(name).ok_or_else(usage)
}

fn usage() -> String {
    "usage:\n  rlogs-bpsr-rlog-opaque-attribute-audit generate --build <id> --gap-window-audit <json> --attribute-ids <comma-separated-ids> --output <json>\n  rlogs-bpsr-rlog-opaque-attribute-audit verify --input <json>".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_varint_requires_complete_canonical_encoding() {
        assert_eq!(diagnostic_unsigned_varint(&[]), Some(0));
        assert_eq!(diagnostic_unsigned_varint(&[0xac, 0x02]), Some(300));
        assert_eq!(diagnostic_unsigned_varint(&[0xac]), None);
        assert_eq!(diagnostic_unsigned_varint(&[0x01, 0x00]), None);
    }

    #[test]
    fn diagnostic_pair_collection_requires_exact_nested_shape() {
        assert_eq!(diagnostic_pair_collection(&[]), Some(Vec::new()));
        assert_eq!(
            diagnostic_pair_collection(&[0x0a, 0x06, 0x08, 0xc0, 0x80, 0x68, 0x10, 0x01]),
            Some(vec![(1_704_000, 1)])
        );
        assert_eq!(diagnostic_pair_collection(&[0x0a, 0x01, 0x08]), None);
    }

    #[test]
    fn opaque_attribute_verifier_rejects_semantic_promotion() {
        let mut audit = AttributeAudit::default();
        audit.safe_to_exclude_from_counterfactual_matching = true;
        assert!(verify_attribute(&audit).is_err());
    }
}
