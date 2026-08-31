use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsString,
    fs::{self, File},
    io::{BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use rlogs_core::ResearchConnectionFile;
use rlogs_events::{
    ActorState, CanonicalEvent, EntityAttributeUpdateKind, StatusState, TimelineEventKind,
};
use rlogs_game_bpsr::{
    CaptureRecordKind, JsonlJournalReader, OfflineRecordingReport, ProtocolPack,
};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 1;
const GENERATED_BY: &str = "rlogs-bpsr-controlled-formula-capture-receipt";
const CURRENT_HP_ATTRIBUTE_ID: i32 = 11_310;
const EFFECT_CONTEXT_LEAD_MICROS: u64 = 2_000_000;
const SUPPORTED_EFFECT_IDS: [i64; 4] = [2_203_031, 2_205_031, 3_003_012, 3_003_411];

fn required_attribute_ids() -> BTreeSet<i32> {
    [
        // Strength, Intelligence, and Dexterity base/current/percent lanes.
        11_010, 11_011, 11_014, 11_020, 11_021, 11_024, 11_030, 11_031, 11_034,
        // Current HP, attack base/current/percent, season strength/weakness, and Mastery.
        11_310, 11_330, 11_331, 11_332, 11_440, 11_450, 11_940,
    ]
    .into_iter()
    .chain(12_690..=12_695)
    .chain(12_700..=12_705)
    .collect()
}

#[derive(Debug)]
struct Arguments {
    build: String,
    effects: BTreeSet<i64>,
    pack: PathBuf,
    capture: PathBuf,
    connections: PathBuf,
    journal: PathBuf,
    rlog: PathBuf,
    coverage: PathBuf,
    output: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct FileReceipt {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ReceiptInputs {
    protocol_pack: FileReceipt,
    raw_capture: FileReceipt,
    exact_connections: FileReceipt,
    raw_protocol_journal: FileReceipt,
    sealed_canonical_rlog: FileReceipt,
    replay_coverage: FileReceipt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct IdentityReceipt {
    protocol_pack_id: String,
    protocol_pack_digest: String,
    journal_capture_id: String,
    canonical_session_id: String,
    connection_count: usize,
    journal_build_matches: bool,
    canonical_build_matches: bool,
    pack_digest_matches_journal_and_canonical: bool,
    coverage_matches_pack_and_canonical_seal: bool,
    coverage_source_matches_journal_capture: bool,
    coverage_session_matches_canonical_session: bool,
    coverage_capture_counters_match_journal: bool,
    raw_capture_and_connections_names_match_capture_id: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct JournalReceipt {
    record_count: u64,
    packet_count: u64,
    gap_count: u64,
    wire_bytes: u64,
    application_bytes: u64,
    packets_with_empty_wire_payload: u64,
    packets_without_application_payload: u64,
    first_observed_micros: Option<u64>,
    last_observed_micros: Option<u64>,
    ends_with_newline: bool,
    strict_zero_gap_closed_file: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct EffectReceipt {
    effect_id: i64,
    status_events: u64,
    applied_events: u64,
    applied_events_with_provider: u64,
    events_without_instance_id: u64,
    duplicate_applied_instances: u64,
    terminal_events: u64,
    unmatched_terminal_events: u64,
    exact_closed_windows: u64,
    unclosed_windows: u64,
    windows_targeting_packet_proven_local_actor: u64,
    recipient_outgoing_damage_events_in_windows: u64,
    target_incoming_damage_events_in_windows: u64,
    distinct_target_damage_sources_in_windows: usize,
    external_target_damage_sources_in_windows: usize,
    linked_local_actor_entity_uuids: Vec<String>,
    linked_local_actor_entity_uuids_with_complete_context: Vec<String>,
    linked_actor_window_contexts: Vec<EffectActorWindowContextReceipt>,
    exact_provider_status_lifecycle_preserved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct EffectActorWindowContextReceipt {
    target_entity_uuid: String,
    status_instance_id: String,
    actor_entity_uuid: String,
    applied_sequence: u64,
    applied_observed_micros: u64,
    terminal_sequence: u64,
    terminal_observed_micros: u64,
    first_relevant_damage_sequence: u64,
    first_relevant_damage_observed_micros: u64,
    latest_complete_snapshot_sequence_before_application: Option<u64>,
    latest_complete_snapshot_observed_micros_before_application: Option<u64>,
    action_timing_events_in_bounded_lifecycle_through_first_damage: u64,
    cooldown_events_in_bounded_lifecycle_through_first_damage: u64,
    resource_events_in_bounded_lifecycle_through_first_damage: u64,
    bounded_context_lead_micros: u64,
    complete_temporal_context: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct LocalAttributeReceipt {
    entity_uuid: String,
    snapshot_attribute_ids: Vec<i32>,
    missing_required_snapshot_attribute_ids: Vec<i32>,
    cooldown_events: u64,
    resource_events: u64,
    client_action_timing_events: u64,
    complete_run_aggregate_context: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CanonicalOperandReceipt {
    event_count: u64,
    content_sha256: String,
    data_gap_events: u64,
    unresolved_status_events: u64,
    unresolved_action_events: u64,
    status_events: u64,
    entity_attribute_snapshot_events: u64,
    entity_attribute_delta_events: u64,
    temporary_attribute_snapshot_events: u64,
    temporary_attribute_delta_events: u64,
    packet_proven_local_actor_entity_uuids: Vec<String>,
    local_attribute_snapshots: Vec<LocalAttributeReceipt>,
    local_temporary_attribute_ids: Vec<i32>,
    one_local_actor_has_every_required_attribute_snapshot: bool,
    required_attribute_ids: Vec<i32>,
    cooldown_events_for_local_actors: u64,
    resource_events_for_local_actors: u64,
    client_action_timing_events: u64,
    damage_events: u64,
    damage_events_with_ability_id: u64,
    damage_events_with_hit_event_id: u64,
    damage_events_with_critical_and_lucky_flags: u64,
    damage_events_with_action_instance_candidate: u64,
    damage_targets: usize,
    damage_targets_spawned_before_first_damage: usize,
    damage_targets_with_current_hp_evidence: usize,
    status_events_on_damage_targets: u64,
    effects: Vec<EffectReceipt>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ReadinessReceipt {
    exact_input_identity_bound: bool,
    zero_gap_closed_interval: bool,
    raw_payload_bytes_preserved: bool,
    canonical_rlog_seal_valid: bool,
    canonical_formula_operand_schema_complete: bool,
    controlled_capture_observed_every_required_operand: bool,
    operation_order_and_integer_rounding_proven: bool,
    formula_authority: bool,
    runtime_promotion_allowed: bool,
    provider_rdps_credit_allowed: bool,
    missing_obligations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PolicyReceipt {
    numeric_effect_ids_and_build_are_authoritative: bool,
    raw_capture_and_unknown_records_are_retained: bool,
    current_character_snapshot_substitution_allowed: bool,
    structurally_absent_remote_cast_packets_required: bool,
    acquisition_receipt_alone_promotes_formula: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ControlledCaptureReceipt {
    schema_version: u16,
    generated_by: String,
    game_build: String,
    effect_ids: Vec<i64>,
    inputs: ReceiptInputs,
    identity: IdentityReceipt,
    journal: JournalReceipt,
    canonical: CanonicalOperandReceipt,
    policy: PolicyReceipt,
    readiness: ReadinessReceipt,
    content_sha256: String,
}

#[derive(Debug, Default)]
struct EffectWindow {
    provider_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    instance_id: i64,
    applied: TimedObservation,
    terminal: Option<TimedObservation>,
    recipient_outgoing_damage_events: u64,
    target_incoming_damage_events: u64,
    target_damage_sources: BTreeSet<i64>,
    target_damage_context_actors: BTreeSet<i64>,
    relevant_damage_by_actor: BTreeMap<i64, TimedObservation>,
}

#[derive(Debug, Default)]
struct EffectAccumulator {
    receipt: EffectReceipt,
    active: BTreeMap<(i64, i64), EffectWindow>,
    closed_targets: BTreeSet<i64>,
    all_target_damage_sources: BTreeSet<i64>,
    all_target_damage_context_actors: BTreeSet<i64>,
    external_target_damage_sources: BTreeSet<i64>,
    closed_windows: Vec<EffectWindow>,
}

#[derive(Debug, Clone, Copy, Default)]
struct TimedObservation {
    sequence: u64,
    observed_micros: u64,
}

#[derive(Debug, Clone)]
struct AttributeSnapshotObservation {
    at: TimedObservation,
    attribute_ids: Vec<i32>,
}

#[derive(Debug, Default)]
struct CanonicalAccumulator {
    data_gap_events: u64,
    unresolved_status_events: u64,
    unresolved_action_events: u64,
    status_events: u64,
    entity_attribute_snapshot_events: u64,
    entity_attribute_delta_events: u64,
    resource_actor_entity_uuids: BTreeSet<i64>,
    action_timing_actor_entity_uuids: BTreeSet<i64>,
    snapshot_attributes: BTreeMap<i64, BTreeSet<i32>>,
    snapshot_attribute_observations: BTreeMap<i64, Vec<AttributeSnapshotObservation>>,
    observed_attributes: BTreeMap<i64, BTreeSet<i32>>,
    temporary_attributes: BTreeMap<i64, BTreeSet<i32>>,
    temporary_attribute_snapshot_events: u64,
    temporary_attribute_delta_events: u64,
    cooldown_events_by_actor: BTreeMap<i64, u64>,
    cooldown_observations_by_actor: BTreeMap<i64, Vec<TimedObservation>>,
    resource_events_by_actor: BTreeMap<i64, u64>,
    resource_observations_by_actor: BTreeMap<i64, Vec<TimedObservation>>,
    action_timing_events_by_actor: BTreeMap<i64, u64>,
    action_timing_observations_by_actor: BTreeMap<i64, Vec<TimedObservation>>,
    client_action_timing_events: u64,
    damage_events: u64,
    damage_events_with_ability_id: u64,
    damage_events_with_hit_event_id: u64,
    damage_events_with_critical_and_lucky_flags: u64,
    damage_events_with_action_instance_candidate: u64,
    spawned_at: BTreeMap<i64, u64>,
    first_damage_to_target: BTreeMap<i64, u64>,
    damage_targets: BTreeSet<i64>,
    status_events_on_damage_targets_by_target: BTreeMap<i64, u64>,
    effect_accumulators: BTreeMap<i64, EffectAccumulator>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("controlled formula capture receipt failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = arguments()?;
    if args.output.exists() {
        return Err(format!("refusing to overwrite {}", args.output.display()).into());
    }
    let mut receipt = generate(&args)?;
    receipt.content_sha256 = content_sha256(&receipt)?;
    validate_receipt(&receipt)?;
    if let Some(parent) = args.output.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut output = File::create(&args.output)?;
    serde_json::to_writer_pretty(&mut output, &receipt)?;
    output.write_all(b"\n")?;
    output.sync_all()?;
    println!(
        "wrote controlled capture receipt for effects {:?}; operands_complete={}",
        receipt.effect_ids,
        receipt
            .readiness
            .controlled_capture_observed_every_required_operand
    );
    for obligation in &receipt.readiness.missing_obligations {
        println!("missing: {obligation}");
    }
    Ok(())
}

fn generate(args: &Arguments) -> Result<ControlledCaptureReceipt, Box<dyn std::error::Error>> {
    let pack_bytes = fs::read(&args.pack)?;
    let pack = ProtocolPack::from_json(&pack_bytes)?;
    if pack.definition().target.build_id != args.build {
        return Err("protocol pack target does not match --build".into());
    }
    let connections: ResearchConnectionFile =
        serde_json::from_reader(BufReader::new(File::open(&args.connections)?))?;
    let connection_count = connections.clone().validate()?.connection_count();
    let coverage: OfflineRecordingReport =
        serde_json::from_reader(BufReader::new(File::open(&args.coverage)?))?;
    let (journal, journal_session) = scan_journal(&args.journal)?;
    let (canonical, header) = scan_rlog(&args.rlog, &args.effects)?;

    let coverage_source_matches_journal_capture =
        coverage.source.source_id == journal_session.capture_id;
    let coverage_session_matches_canonical_session = coverage.session_id == header.session_id;
    let coverage_capture_counters_match_journal = coverage.record_count == journal.record_count
        && coverage.capture.packet_count == journal.packet_count
        && coverage.capture.gap_count == journal.gap_count
        && coverage.capture.wire_bytes == journal.wire_bytes
        && coverage.capture.application_bytes == journal.application_bytes;
    let raw_capture_and_connections_names_match_capture_id = capture_input_names_match_id(
        &args.capture,
        &args.connections,
        &journal_session.capture_id,
    );

    let identity = IdentityReceipt {
        protocol_pack_id: pack.definition().pack_id.clone(),
        protocol_pack_digest: pack.digest().to_owned(),
        journal_capture_id: journal_session.capture_id,
        canonical_session_id: header.session_id,
        connection_count,
        journal_build_matches: journal_session.game_build.build_id == args.build,
        canonical_build_matches: header.region.client_build == args.build,
        pack_digest_matches_journal_and_canonical: journal_session.protocol_pack_digest.as_deref()
            == Some(pack.digest())
            && header.region.protocol_pack_digest == pack.digest(),
        coverage_matches_pack_and_canonical_seal: coverage.protocol_pack_id
            == pack.definition().pack_id
            && coverage.protocol_pack_digest == pack.digest()
            && coverage.rlog.content_sha256 == canonical.content_sha256
            && coverage.rlog.event_count == canonical.event_count
            && coverage.record_count == journal.record_count,
        coverage_source_matches_journal_capture,
        coverage_session_matches_canonical_session,
        coverage_capture_counters_match_journal,
        raw_capture_and_connections_names_match_capture_id,
    };

    let inputs = ReceiptInputs {
        protocol_pack: file_receipt(&args.pack)?,
        raw_capture: file_receipt(&args.capture)?,
        exact_connections: file_receipt(&args.connections)?,
        raw_protocol_journal: file_receipt(&args.journal)?,
        sealed_canonical_rlog: file_receipt(&args.rlog)?,
        replay_coverage: file_receipt(&args.coverage)?,
    };

    let exact_identity = exact_input_identity_bound(&identity, &inputs);
    let zero_gap = journal.strict_zero_gap_closed_file
        && coverage.capture.gap_count == 0
        && coverage.decoder.capture_gap_records == 0
        && canonical.data_gap_events == 0;
    let raw_bytes = journal.packet_count > 0
        && journal.wire_bytes > 0
        && journal.packets_with_empty_wire_payload == 0;
    let schema_complete = canonical_schema_preserves_required_operands();
    let mut missing = missing_obligations(
        &canonical,
        &args.effects,
        exact_identity,
        zero_gap,
        raw_bytes,
        coverage.decoder.decode_failed_records,
        coverage.decoder.missing_application_payload_records,
    );
    if inputs.raw_capture.bytes == 0 {
        missing.push("bound raw capture file is empty".into());
    }
    if connection_count == 0 {
        missing.push("bound exact-connection manifest contains no captured connections".into());
    }
    if !identity.coverage_source_matches_journal_capture {
        missing.push("coverage source ID does not match the protocol journal capture ID".into());
    }
    if !identity.coverage_session_matches_canonical_session {
        missing.push("coverage session ID does not match the sealed RLOG session ID".into());
    }
    if !identity.coverage_capture_counters_match_journal {
        missing.push(
            "coverage record/packet/gap/wire/application counters do not match the journal".into(),
        );
    }
    if !identity.raw_capture_and_connections_names_match_capture_id {
        missing.push(
            "raw capture derivation is unproven: capture and connection filenames do not bind to the journal capture ID"
                .into(),
        );
    }
    missing.sort();
    missing.dedup();
    let observed_all = missing.is_empty();

    Ok(ControlledCaptureReceipt {
        schema_version: SCHEMA_VERSION,
        generated_by: GENERATED_BY.to_owned(),
        game_build: args.build.clone(),
        effect_ids: args.effects.iter().copied().collect(),
        inputs,
        identity,
        journal,
        canonical,
        policy: PolicyReceipt {
            numeric_effect_ids_and_build_are_authoritative: true,
            raw_capture_and_unknown_records_are_retained: true,
            current_character_snapshot_substitution_allowed: false,
            structurally_absent_remote_cast_packets_required: false,
            acquisition_receipt_alone_promotes_formula: false,
        },
        readiness: ReadinessReceipt {
            exact_input_identity_bound: exact_identity,
            zero_gap_closed_interval: zero_gap,
            raw_payload_bytes_preserved: raw_bytes,
            canonical_rlog_seal_valid: true,
            canonical_formula_operand_schema_complete: schema_complete,
            controlled_capture_observed_every_required_operand: observed_all,
            operation_order_and_integer_rounding_proven: false,
            formula_authority: false,
            runtime_promotion_allowed: false,
            provider_rdps_credit_allowed: false,
            missing_obligations: missing,
        },
        content_sha256: String::new(),
    })
}

fn scan_journal(
    path: &Path,
) -> Result<(JournalReceipt, rlogs_game_bpsr::CaptureSession), Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let mut stream = JsonlJournalReader::new(BufReader::new(file)).into_record_stream()?;
    let session = stream.session().clone();
    let mut packet_count = 0_u64;
    let mut gap_count = 0_u64;
    let mut wire_bytes = 0_u64;
    let mut application_bytes = 0_u64;
    let mut empty_wire = 0_u64;
    let mut missing_application = 0_u64;
    let mut first_observed = None;
    let mut last_observed = None;
    while let Some(record) = stream.next_record()? {
        first_observed.get_or_insert(record.observed_micros);
        last_observed = Some(record.observed_micros);
        match record.kind {
            CaptureRecordKind::Packet(packet) => {
                packet_count = packet_count.saturating_add(1);
                wire_bytes = wire_bytes.saturating_add(packet.payload.wire_bytes.len() as u64);
                if packet.payload.wire_bytes.is_empty() {
                    empty_wire = empty_wire.saturating_add(1);
                }
                if let Some(bytes) = packet.payload.application_bytes {
                    application_bytes = application_bytes.saturating_add(bytes.len() as u64);
                } else {
                    missing_application = missing_application.saturating_add(1);
                }
            }
            CaptureRecordKind::Gap(_) => gap_count = gap_count.saturating_add(1),
        }
    }
    let record_count = stream.record_count();
    let ends_with_newline = file_ends_with_newline(path)?;
    Ok((
        JournalReceipt {
            record_count,
            packet_count,
            gap_count,
            wire_bytes,
            application_bytes,
            packets_with_empty_wire_payload: empty_wire,
            packets_without_application_payload: missing_application,
            first_observed_micros: first_observed,
            last_observed_micros: last_observed,
            ends_with_newline,
            strict_zero_gap_closed_file: record_count > 0
                && packet_count > 0
                && gap_count == 0
                && ends_with_newline,
        },
        session,
    ))
}

fn scan_rlog(
    path: &Path,
    effects: &BTreeSet<i64>,
) -> Result<(CanonicalOperandReceipt, rlogs_log_format::RlogHeader), Box<dyn std::error::Error>> {
    let limits = RlogLimits {
        maximum_line_bytes: 128 * 1024 * 1024,
        maximum_events: 20_000_000,
        maximum_block_bytes: 128 * 1024 * 1024,
        maximum_block_events: 65_536,
    };
    let mut reader = RlogReader::new(BufReader::new(File::open(path)?), limits)?;
    let header = reader.header().clone();
    let mut state = CanonicalAccumulator::default();
    for effect in effects {
        state.effect_accumulators.insert(
            *effect,
            EffectAccumulator {
                receipt: EffectReceipt {
                    effect_id: *effect,
                    ..EffectReceipt::default()
                },
                ..EffectAccumulator::default()
            },
        );
    }
    while let Some(envelope) = reader.next_event()? {
        observe_canonical(&mut state, envelope.sequence, &envelope.event);
    }
    let summary = reader
        .summary()
        .cloned()
        .ok_or("sealed RLOG replay produced no integrity summary")?;
    Ok((finish_canonical(state, summary), header))
}

fn observe_canonical(state: &mut CanonicalAccumulator, sequence: u64, event: &CanonicalEvent) {
    let CanonicalEvent::Timeline(timeline) = event else {
        return;
    };
    let at = TimedObservation {
        sequence,
        observed_micros: timeline.time.observed_micros,
    };
    match &timeline.kind {
        TimelineEventKind::Actor(actor) if actor.state == ActorState::Spawned => {
            state
                .spawned_at
                .entry(actor.actor.entity_uuid.0)
                .or_insert(sequence);
        }
        TimelineEventKind::EntityAttributes(event) => {
            state
                .observed_attributes
                .entry(event.actor.entity_uuid.0)
                .or_default()
                .extend(event.attributes.iter().map(|entry| entry.attribute_id));
            match event.update_kind {
                EntityAttributeUpdateKind::Snapshot => {
                    state.entity_attribute_snapshot_events =
                        state.entity_attribute_snapshot_events.saturating_add(1);
                    state
                        .snapshot_attributes
                        .entry(event.actor.entity_uuid.0)
                        .or_default()
                        .extend(event.attributes.iter().map(|entry| entry.attribute_id));
                    state
                        .snapshot_attribute_observations
                        .entry(event.actor.entity_uuid.0)
                        .or_default()
                        .push(AttributeSnapshotObservation {
                            at,
                            attribute_ids: event
                                .attributes
                                .iter()
                                .map(|entry| entry.attribute_id)
                                .collect(),
                        });
                }
                EntityAttributeUpdateKind::Delta => {
                    state.entity_attribute_delta_events =
                        state.entity_attribute_delta_events.saturating_add(1);
                }
                EntityAttributeUpdateKind::Unknown => {}
            }
        }
        TimelineEventKind::TemporaryAttributes(event) => {
            state
                .temporary_attributes
                .entry(event.actor.entity_uuid.0)
                .or_default()
                .extend(event.attributes.iter().map(|entry| entry.id));
            match event.update_kind {
                EntityAttributeUpdateKind::Snapshot => {
                    state.temporary_attribute_snapshot_events =
                        state.temporary_attribute_snapshot_events.saturating_add(1);
                }
                EntityAttributeUpdateKind::Delta => {
                    state.temporary_attribute_delta_events =
                        state.temporary_attribute_delta_events.saturating_add(1);
                }
                EntityAttributeUpdateKind::Unknown => {}
            }
        }
        TimelineEventKind::Cast(event) => {
            if event.action_timing.is_some() {
                state.client_action_timing_events =
                    state.client_action_timing_events.saturating_add(1);
                state
                    .action_timing_actor_entity_uuids
                    .insert(event.source.entity_uuid.0);
                *state
                    .action_timing_events_by_actor
                    .entry(event.source.entity_uuid.0)
                    .or_default() += 1;
                state
                    .action_timing_observations_by_actor
                    .entry(event.source.entity_uuid.0)
                    .or_default()
                    .push(at);
            }
        }
        TimelineEventKind::Cooldown(event) => {
            *state
                .cooldown_events_by_actor
                .entry(event.actor.entity_uuid.0)
                .or_default() += 1;
            state
                .cooldown_observations_by_actor
                .entry(event.actor.entity_uuid.0)
                .or_default()
                .push(at);
        }
        TimelineEventKind::Resource(event) => {
            state
                .resource_actor_entity_uuids
                .insert(event.actor.entity_uuid.0);
            *state
                .resource_events_by_actor
                .entry(event.actor.entity_uuid.0)
                .or_default() += 1;
            state
                .resource_observations_by_actor
                .entry(event.actor.entity_uuid.0)
                .or_default()
                .push(at);
        }
        TimelineEventKind::Damage(event) => {
            state.damage_events = state.damage_events.saturating_add(1);
            state.damage_targets.insert(event.target.entity_uuid.0);
            state
                .first_damage_to_target
                .entry(event.target.entity_uuid.0)
                .or_insert(sequence);
            if event.ability.is_some() {
                state.damage_events_with_ability_id =
                    state.damage_events_with_ability_id.saturating_add(1);
            }
            if event.hit_event_id.is_some() {
                state.damage_events_with_hit_event_id =
                    state.damage_events_with_hit_event_id.saturating_add(1);
            }
            if event.flags.critical.is_some() && event.flags.lucky.is_some() {
                state.damage_events_with_critical_and_lucky_flags = state
                    .damage_events_with_critical_and_lucky_flags
                    .saturating_add(1);
            }
            if event.packet.skill_effect_uuid.is_some() {
                state.damage_events_with_action_instance_candidate = state
                    .damage_events_with_action_instance_candidate
                    .saturating_add(1);
            }
            for accumulator in state.effect_accumulators.values_mut() {
                for window in accumulator.active.values_mut() {
                    if event.source.entity_uuid.0 == window.target_entity_uuid
                        || event
                            .direct_source
                            .is_some_and(|source| source.entity_uuid.0 == window.target_entity_uuid)
                    {
                        window.recipient_outgoing_damage_events =
                            window.recipient_outgoing_damage_events.saturating_add(1);
                        window
                            .relevant_damage_by_actor
                            .entry(window.target_entity_uuid)
                            .or_insert(at);
                    }
                    if event.target.entity_uuid.0 == window.target_entity_uuid {
                        window.target_incoming_damage_events =
                            window.target_incoming_damage_events.saturating_add(1);
                        window
                            .target_damage_sources
                            .insert(event.source.entity_uuid.0);
                        window
                            .target_damage_context_actors
                            .insert(event.source.entity_uuid.0);
                        window
                            .relevant_damage_by_actor
                            .entry(event.source.entity_uuid.0)
                            .or_insert(at);
                        if let Some(direct_source) = event.direct_source {
                            window
                                .target_damage_context_actors
                                .insert(direct_source.entity_uuid.0);
                            window
                                .relevant_damage_by_actor
                                .entry(direct_source.entity_uuid.0)
                                .or_insert(at);
                        }
                    }
                }
            }
        }
        TimelineEventKind::Status(event) => {
            state.status_events = state.status_events.saturating_add(1);
            *state
                .status_events_on_damage_targets_by_target
                .entry(event.target.entity_uuid.0)
                .or_default() += 1;
            let Some(accumulator) = state.effect_accumulators.get_mut(&event.effect.0) else {
                return;
            };
            accumulator.receipt.status_events = accumulator.receipt.status_events.saturating_add(1);
            let Some(instance) = event.instance_id.map(|value| value.0) else {
                accumulator.receipt.events_without_instance_id = accumulator
                    .receipt
                    .events_without_instance_id
                    .saturating_add(1);
                return;
            };
            let key = (event.target.entity_uuid.0, instance);
            match event.state {
                StatusState::Applied => {
                    accumulator.receipt.applied_events =
                        accumulator.receipt.applied_events.saturating_add(1);
                    if event.source.is_some() {
                        accumulator.receipt.applied_events_with_provider = accumulator
                            .receipt
                            .applied_events_with_provider
                            .saturating_add(1);
                    }
                    if accumulator
                        .active
                        .insert(
                            key,
                            EffectWindow {
                                provider_entity_uuid: event.source.map(|value| value.entity_uuid.0),
                                target_entity_uuid: event.target.entity_uuid.0,
                                instance_id: instance,
                                applied: at,
                                ..EffectWindow::default()
                            },
                        )
                        .is_some()
                    {
                        accumulator.receipt.duplicate_applied_instances = accumulator
                            .receipt
                            .duplicate_applied_instances
                            .saturating_add(1);
                    }
                }
                StatusState::Removed => {
                    accumulator.receipt.terminal_events =
                        accumulator.receipt.terminal_events.saturating_add(1);
                    if let Some(window) = accumulator.active.remove(&key) {
                        close_window(accumulator, window, at);
                    } else {
                        accumulator.receipt.unmatched_terminal_events = accumulator
                            .receipt
                            .unmatched_terminal_events
                            .saturating_add(1);
                    }
                }
                StatusState::Consumed if event.stacks.unwrap_or_default() == 0 => {
                    accumulator.receipt.terminal_events =
                        accumulator.receipt.terminal_events.saturating_add(1);
                    if let Some(window) = accumulator.active.remove(&key) {
                        close_window(accumulator, window, at);
                    } else {
                        accumulator.receipt.unmatched_terminal_events = accumulator
                            .receipt
                            .unmatched_terminal_events
                            .saturating_add(1);
                    }
                }
                StatusState::Refreshed | StatusState::Stacked | StatusState::Consumed => {}
            }
        }
        TimelineEventKind::UnresolvedStatus(_) => {
            state.unresolved_status_events = state.unresolved_status_events.saturating_add(1)
        }
        TimelineEventKind::UnresolvedAction(_) => {
            state.unresolved_action_events = state.unresolved_action_events.saturating_add(1)
        }
        TimelineEventKind::DataGap(_) => {
            state.data_gap_events = state.data_gap_events.saturating_add(1)
        }
        _ => {}
    }
}

fn close_window(
    accumulator: &mut EffectAccumulator,
    mut window: EffectWindow,
    terminal: TimedObservation,
) {
    window.terminal = Some(terminal);
    accumulator.receipt.exact_closed_windows =
        accumulator.receipt.exact_closed_windows.saturating_add(1);
    accumulator
        .receipt
        .recipient_outgoing_damage_events_in_windows = accumulator
        .receipt
        .recipient_outgoing_damage_events_in_windows
        .saturating_add(window.recipient_outgoing_damage_events);
    accumulator.receipt.target_incoming_damage_events_in_windows = accumulator
        .receipt
        .target_incoming_damage_events_in_windows
        .saturating_add(window.target_incoming_damage_events);
    accumulator.closed_targets.insert(window.target_entity_uuid);
    accumulator
        .all_target_damage_sources
        .extend(window.target_damage_sources.iter().copied());
    accumulator
        .all_target_damage_context_actors
        .extend(window.target_damage_context_actors.iter().copied());
    accumulator.external_target_damage_sources.extend(
        window
            .target_damage_sources
            .iter()
            .copied()
            .filter(|source| Some(*source) != window.provider_entity_uuid),
    );
    accumulator.closed_windows.push(window);
}

fn complete_snapshot_before(
    observations: Option<&[AttributeSnapshotObservation]>,
    required: &BTreeSet<i32>,
    before: TimedObservation,
) -> Option<TimedObservation> {
    let mut observed = BTreeSet::new();
    let mut completed_at = None;
    for observation in observations.unwrap_or_default() {
        if observation.at.sequence >= before.sequence
            || observation.at.observed_micros > before.observed_micros
        {
            break;
        }
        observed.extend(observation.attribute_ids.iter().copied());
        if required.is_subset(&observed) {
            completed_at = Some(observation.at);
        }
    }
    completed_at
}

fn bounded_observation_count(
    observations: Option<&[TimedObservation]>,
    applied: TimedObservation,
    upper_bound: TimedObservation,
) -> u64 {
    let lower_micros = applied
        .observed_micros
        .saturating_sub(EFFECT_CONTEXT_LEAD_MICROS);
    observations
        .unwrap_or_default()
        .iter()
        .filter(|observation| {
            observation.sequence <= upper_bound.sequence
                && observation.observed_micros >= lower_micros
                && observation.observed_micros <= upper_bound.observed_micros
        })
        .count() as u64
}

fn finish_canonical(
    mut state: CanonicalAccumulator,
    summary: rlogs_log_format::RlogReplaySummary,
) -> CanonicalOperandReceipt {
    let local_actors = state
        .resource_actor_entity_uuids
        .union(&state.action_timing_actor_entity_uuids)
        .copied()
        .collect::<BTreeSet<_>>();
    let required = required_attribute_ids();
    let local_attribute_snapshots = local_actors
        .iter()
        .map(|actor| {
            let observed = state
                .snapshot_attributes
                .get(actor)
                .cloned()
                .unwrap_or_default();
            let missing_required_snapshot_attribute_ids =
                required.difference(&observed).copied().collect::<Vec<_>>();
            let cooldown_events = state
                .cooldown_events_by_actor
                .get(actor)
                .copied()
                .unwrap_or(0);
            let resource_events = state
                .resource_events_by_actor
                .get(actor)
                .copied()
                .unwrap_or(0);
            let client_action_timing_events = state
                .action_timing_events_by_actor
                .get(actor)
                .copied()
                .unwrap_or(0);
            LocalAttributeReceipt {
                entity_uuid: actor.to_string(),
                snapshot_attribute_ids: observed.iter().copied().collect(),
                complete_run_aggregate_context: missing_required_snapshot_attribute_ids.is_empty()
                    && cooldown_events > 0
                    && resource_events > 0
                    && client_action_timing_events > 0,
                missing_required_snapshot_attribute_ids,
                cooldown_events,
                resource_events,
                client_action_timing_events,
            }
        })
        .collect::<Vec<_>>();
    let all_attributes = local_attribute_snapshots
        .iter()
        .any(|entry| entry.missing_required_snapshot_attribute_ids.is_empty());
    let local_cooldowns = local_actors
        .iter()
        .map(|actor| {
            state
                .cooldown_events_by_actor
                .get(actor)
                .copied()
                .unwrap_or(0)
        })
        .sum();
    let local_resources = local_actors
        .iter()
        .map(|actor| {
            state
                .resource_events_by_actor
                .get(actor)
                .copied()
                .unwrap_or(0)
        })
        .sum();
    let local_temporary_attribute_ids = local_actors
        .iter()
        .flat_map(|actor| {
            state
                .temporary_attributes
                .get(actor)
                .into_iter()
                .flat_map(|values| values.iter().copied())
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let spawned_targets = state
        .first_damage_to_target
        .iter()
        .filter(|(target, damage_sequence)| {
            state
                .spawned_at
                .get(target)
                .is_some_and(|spawn_sequence| spawn_sequence < *damage_sequence)
        })
        .count();
    let targets_with_current_hp = state
        .damage_targets
        .iter()
        .filter(|target| {
            state
                .observed_attributes
                .get(target)
                .is_some_and(|attributes| attributes.contains(&CURRENT_HP_ATTRIBUTE_ID))
        })
        .count();
    let status_events_on_targets = state
        .damage_targets
        .iter()
        .map(|target| {
            state
                .status_events_on_damage_targets_by_target
                .get(target)
                .copied()
                .unwrap_or(0)
        })
        .sum();
    let mut effect_receipts = Vec::new();
    let snapshot_observations = &state.snapshot_attribute_observations;
    let action_observations = &state.action_timing_observations_by_actor;
    let cooldown_observations = &state.cooldown_observations_by_actor;
    let resource_observations = &state.resource_observations_by_actor;
    for accumulator in state.effect_accumulators.values_mut() {
        accumulator.receipt.unclosed_windows = accumulator.active.len() as u64;
        accumulator
            .receipt
            .windows_targeting_packet_proven_local_actor = accumulator
            .closed_targets
            .intersection(&local_actors)
            .count() as u64;
        accumulator
            .receipt
            .distinct_target_damage_sources_in_windows =
            accumulator.all_target_damage_sources.len();
        accumulator
            .receipt
            .external_target_damage_sources_in_windows =
            accumulator.external_target_damage_sources.len();
        let mut window_contexts = Vec::new();
        for window in &accumulator.closed_windows {
            let Some(terminal) = window.terminal else {
                continue;
            };
            let candidate_actors = if accumulator.receipt.effect_id == 3_003_411 {
                if window.recipient_outgoing_damage_events > 0 {
                    BTreeSet::from([window.target_entity_uuid])
                } else {
                    BTreeSet::new()
                }
            } else {
                window
                    .target_damage_context_actors
                    .iter()
                    .copied()
                    .collect()
            };
            for actor in candidate_actors.intersection(&local_actors) {
                let Some(first_damage) = window.relevant_damage_by_actor.get(actor).copied() else {
                    continue;
                };
                let complete_snapshot = complete_snapshot_before(
                    snapshot_observations.get(actor).map(Vec::as_slice),
                    &required,
                    window.applied,
                );
                let action_count = bounded_observation_count(
                    action_observations.get(actor).map(Vec::as_slice),
                    window.applied,
                    first_damage,
                );
                let cooldown_count = bounded_observation_count(
                    cooldown_observations.get(actor).map(Vec::as_slice),
                    window.applied,
                    first_damage,
                );
                let resource_count = bounded_observation_count(
                    resource_observations.get(actor).map(Vec::as_slice),
                    window.applied,
                    first_damage,
                );
                window_contexts.push(EffectActorWindowContextReceipt {
                    target_entity_uuid: window.target_entity_uuid.to_string(),
                    status_instance_id: window.instance_id.to_string(),
                    actor_entity_uuid: actor.to_string(),
                    applied_sequence: window.applied.sequence,
                    applied_observed_micros: window.applied.observed_micros,
                    terminal_sequence: terminal.sequence,
                    terminal_observed_micros: terminal.observed_micros,
                    first_relevant_damage_sequence: first_damage.sequence,
                    first_relevant_damage_observed_micros: first_damage.observed_micros,
                    latest_complete_snapshot_sequence_before_application: complete_snapshot
                        .map(|value| value.sequence),
                    latest_complete_snapshot_observed_micros_before_application: complete_snapshot
                        .map(|value| value.observed_micros),
                    action_timing_events_in_bounded_lifecycle_through_first_damage: action_count,
                    cooldown_events_in_bounded_lifecycle_through_first_damage: cooldown_count,
                    resource_events_in_bounded_lifecycle_through_first_damage: resource_count,
                    bounded_context_lead_micros: EFFECT_CONTEXT_LEAD_MICROS,
                    complete_temporal_context: complete_snapshot.is_some()
                        && action_count > 0
                        && cooldown_count > 0
                        && resource_count > 0,
                });
            }
        }
        window_contexts.sort_by_key(|entry| {
            (
                entry.applied_sequence,
                entry.target_entity_uuid.clone(),
                entry.actor_entity_uuid.clone(),
            )
        });
        let linked_local_actors = window_contexts
            .iter()
            .map(|entry| entry.actor_entity_uuid.clone())
            .collect::<BTreeSet<_>>();
        accumulator.receipt.linked_local_actor_entity_uuids =
            linked_local_actors.iter().cloned().collect();
        accumulator
            .receipt
            .linked_local_actor_entity_uuids_with_complete_context = window_contexts
            .iter()
            .filter(|entry| entry.complete_temporal_context)
            .map(|entry| entry.actor_entity_uuid.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        accumulator.receipt.linked_actor_window_contexts = window_contexts;
        accumulator
            .receipt
            .exact_provider_status_lifecycle_preserved =
            accumulator.receipt.applied_events_with_provider > 0
                && accumulator.receipt.exact_closed_windows > 0
                && accumulator.receipt.events_without_instance_id == 0
                && accumulator.receipt.duplicate_applied_instances == 0
                && accumulator.receipt.unmatched_terminal_events == 0
                && accumulator.receipt.unclosed_windows == 0;
        effect_receipts.push(accumulator.receipt.clone());
    }
    effect_receipts.sort_by_key(|entry| entry.effect_id);
    CanonicalOperandReceipt {
        event_count: summary.event_count,
        content_sha256: summary.content_sha256,
        data_gap_events: state.data_gap_events,
        unresolved_status_events: state.unresolved_status_events,
        unresolved_action_events: state.unresolved_action_events,
        status_events: state.status_events,
        entity_attribute_snapshot_events: state.entity_attribute_snapshot_events,
        entity_attribute_delta_events: state.entity_attribute_delta_events,
        temporary_attribute_snapshot_events: state.temporary_attribute_snapshot_events,
        temporary_attribute_delta_events: state.temporary_attribute_delta_events,
        packet_proven_local_actor_entity_uuids: local_actors
            .iter()
            .map(ToString::to_string)
            .collect(),
        local_attribute_snapshots,
        local_temporary_attribute_ids,
        one_local_actor_has_every_required_attribute_snapshot: all_attributes,
        required_attribute_ids: required.into_iter().collect(),
        cooldown_events_for_local_actors: local_cooldowns,
        resource_events_for_local_actors: local_resources,
        client_action_timing_events: state.client_action_timing_events,
        damage_events: state.damage_events,
        damage_events_with_ability_id: state.damage_events_with_ability_id,
        damage_events_with_hit_event_id: state.damage_events_with_hit_event_id,
        damage_events_with_critical_and_lucky_flags: state
            .damage_events_with_critical_and_lucky_flags,
        damage_events_with_action_instance_candidate: state
            .damage_events_with_action_instance_candidate,
        damage_targets: state.damage_targets.len(),
        damage_targets_spawned_before_first_damage: spawned_targets,
        damage_targets_with_current_hp_evidence: targets_with_current_hp,
        status_events_on_damage_targets: status_events_on_targets,
        effects: effect_receipts,
    }
}

fn missing_obligations(
    canonical: &CanonicalOperandReceipt,
    effects: &BTreeSet<i64>,
    exact_identity: bool,
    zero_gap: bool,
    raw_bytes: bool,
    decode_failures: u64,
    missing_application_payloads: u64,
) -> Vec<String> {
    let mut missing = Vec::new();
    if !exact_identity {
        missing.push(
            "exact build, pack, journal, coverage, and RLOG identities do not all match".into(),
        );
    }
    if !zero_gap {
        missing
            .push("capture/journal/canonical interval is not strictly zero-gap and closed".into());
    }
    if !raw_bytes {
        missing.push("one or more packet records lack original wire payload bytes".into());
    }
    if decode_failures > 0 {
        missing.push(format!(
            "{decode_failures} exact-pack records failed canonical decoding"
        ));
    }
    if missing_application_payloads > 0 {
        missing.push(format!(
            "{missing_application_payloads} records lack a decoded application payload"
        ));
    }
    if canonical.packet_proven_local_actor_entity_uuids.is_empty() {
        missing.push(
            "no local actor was proven by client action timing or local resource state".into(),
        );
    }
    if !canonical.one_local_actor_has_every_required_attribute_snapshot {
        missing.push(
            "no packet-proven local actor has a complete 11440/11450, 12690..12695/12700..12705, main-stat, Mastery, attack, and CurrentHP snapshot"
                .into(),
        );
    }
    if canonical.cooldown_events_for_local_actors == 0 {
        missing.push("no exact local cooldown state was observed".into());
    }
    if canonical.resource_events_for_local_actors == 0 {
        missing.push("no exact local resource state was observed".into());
    }
    if canonical.client_action_timing_events == 0 {
        missing.push("no exact client action instance/timing snapshot was observed".into());
    }
    if canonical.damage_events == 0
        || canonical.damage_events_with_ability_id != canonical.damage_events
        || canonical.damage_events_with_hit_event_id != canonical.damage_events
        || canonical.damage_events_with_critical_and_lucky_flags != canonical.damage_events
        || canonical.damage_events_with_action_instance_candidate != canonical.damage_events
    {
        missing.push(
            "not every damage event preserves ability, hit, critical/lucky, and action-instance candidate identity"
                .into(),
        );
    }
    if canonical.damage_targets == 0
        || canonical.damage_targets_spawned_before_first_damage != canonical.damage_targets
    {
        missing.push("not every damage target has capture-start actor state before damage".into());
    }
    if canonical.damage_targets_with_current_hp_evidence != canonical.damage_targets {
        missing.push("not every damage target has packet-observed CurrentHP 11310 state".into());
    }
    if canonical.status_events_on_damage_targets == 0 {
        missing.push("no target status lifecycle was observed on a damage target".into());
    }
    if canonical.unresolved_status_events > 0 {
        missing.push(format!(
            "{} unresolved status events remain in the controlled interval",
            canonical.unresolved_status_events
        ));
    }
    if canonical.unresolved_action_events > 0 {
        missing.push(format!(
            "{} unresolved action events remain in the controlled interval",
            canonical.unresolved_action_events
        ));
    }
    for effect_id in effects {
        let effect = canonical
            .effects
            .iter()
            .find(|entry| entry.effect_id == *effect_id);
        let Some(effect) = effect else {
            missing.push(format!("effect {effect_id} has no receipt row"));
            continue;
        };
        if !effect.exact_provider_status_lifecycle_preserved {
            missing.push(format!(
                "effect {effect_id} lacks one fully closed provider-owned instance lifecycle"
            ));
        }
        if effect.linked_local_actor_entity_uuids.is_empty() {
            missing.push(format!(
                "effect {effect_id} has no packet-proven local recipient/attacker linked to its exact lifecycle"
            ));
        } else if effect
            .linked_local_actor_entity_uuids_with_complete_context
            .is_empty()
        {
            missing.push(format!(
                "effect {effect_id}'s linked local recipient/attacker lacks a complete pre-application attribute snapshot and same-actor action-timing, cooldown, and resource observations in the exact bounded lifecycle"
            ));
        }
        match *effect_id {
            3_003_411 => {
                if effect.windows_targeting_packet_proven_local_actor == 0
                    || effect.recipient_outgoing_damage_events_in_windows == 0
                {
                    missing.push(
                        "Endless Mind 3003411 lacks a local-recipient window with recipient outgoing damage"
                            .into(),
                    );
                }
            }
            3_003_012 | 2_203_031 | 2_205_031 => {
                if effect.target_incoming_damage_events_in_windows == 0 {
                    missing.push(format!(
                        "target effect {effect_id} lacks incoming damage inside its exact lifecycle"
                    ));
                }
                if effect.external_target_damage_sources_in_windows == 0 {
                    missing.push(format!(
                        "target effect {effect_id} lacks an external attacker distinct from its provider"
                    ));
                }
                if matches!(*effect_id, 2_203_031 | 2_205_031)
                    && effect.distinct_target_damage_sources_in_windows < 2
                {
                    missing.push(format!(
                        "Wounding Curse {effect_id} lacks two distinct attackers inside one exact target lifecycle"
                    ));
                }
            }
            _ => {}
        }
    }
    missing
}

fn canonical_schema_preserves_required_operands() -> bool {
    // Compile-time use of these canonical fields is intentional. A schema change that removes
    // any operand breaks this tool instead of silently weakening the acquisition receipt.
    let _ = CURRENT_HP_ATTRIBUTE_ID;
    true
}

fn exact_input_identity_bound(identity: &IdentityReceipt, inputs: &ReceiptInputs) -> bool {
    inputs.raw_capture.bytes > 0
        && identity.connection_count > 0
        && identity.journal_build_matches
        && identity.canonical_build_matches
        && identity.pack_digest_matches_journal_and_canonical
        && identity.coverage_matches_pack_and_canonical_seal
        && identity.coverage_source_matches_journal_capture
        && identity.coverage_session_matches_canonical_session
        && identity.coverage_capture_counters_match_journal
        && identity.raw_capture_and_connections_names_match_capture_id
}

fn validate_receipt(receipt: &ControlledCaptureReceipt) -> Result<(), String> {
    if receipt.schema_version != SCHEMA_VERSION
        || receipt.generated_by != GENERATED_BY
        || receipt.effect_ids.is_empty()
        || receipt
            .effect_ids
            .iter()
            .any(|id| !SUPPORTED_EFFECT_IDS.contains(id))
        || receipt.content_sha256 != content_sha256(receipt).map_err(|error| error.to_string())?
        || !receipt
            .policy
            .numeric_effect_ids_and_build_are_authoritative
        || !receipt.policy.raw_capture_and_unknown_records_are_retained
        || receipt
            .policy
            .current_character_snapshot_substitution_allowed
        || receipt
            .policy
            .structurally_absent_remote_cast_packets_required
        || receipt.policy.acquisition_receipt_alone_promotes_formula
        || receipt.readiness.exact_input_identity_bound
            != exact_input_identity_bound(&receipt.identity, &receipt.inputs)
        || receipt
            .readiness
            .operation_order_and_integer_rounding_proven
        || receipt.readiness.formula_authority
        || receipt.readiness.runtime_promotion_allowed
        || receipt.readiness.provider_rdps_credit_allowed
    {
        return Err("controlled capture receipt violates its fail-closed schema".into());
    }
    if receipt
        .readiness
        .controlled_capture_observed_every_required_operand
        != receipt.readiness.missing_obligations.is_empty()
    {
        return Err("controlled capture operand readiness does not match its blockers".into());
    }
    Ok(())
}

fn content_sha256(receipt: &ControlledCaptureReceipt) -> Result<String, serde_json::Error> {
    let mut canonical = receipt.clone();
    canonical.content_sha256.clear();
    Ok(format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&canonical)?)
    ))
}

fn file_receipt(path: &Path) -> Result<FileReceipt, Box<dyn std::error::Error>> {
    let absolute = fs::canonicalize(path)?;
    let mut input = File::open(&absolute)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    let mut bytes = 0_u64;
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        bytes = bytes.saturating_add(read as u64);
    }
    Ok(FileReceipt {
        path: absolute.display().to_string().replace('\\', "/"),
        bytes,
        sha256: format!("sha256:{:x}", hasher.finalize()),
    })
}

fn capture_input_names_match_id(capture: &Path, connections: &Path, capture_id: &str) -> bool {
    let capture_name_matches = capture
        .file_stem()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value == capture_id);
    let expected_connections = format!("{capture_id}.connections.json");
    let connection_name_matches = connections
        .file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value == expected_connections);
    capture_name_matches && connection_name_matches
}

fn file_ends_with_newline(path: &Path) -> Result<bool, std::io::Error> {
    let mut file = File::open(path)?;
    if file.metadata()?.len() == 0 {
        return Ok(false);
    }
    file.seek(SeekFrom::End(-1))?;
    let mut byte = [0_u8; 1];
    file.read_exact(&mut byte)?;
    Ok(byte[0] == b'\n')
}

fn arguments() -> Result<Arguments, String> {
    arguments_from(env::args_os().skip(1).collect())
}

fn arguments_from(mut values: Vec<OsString>) -> Result<Arguments, String> {
    let build = text(take_value(&mut values, "--build")?, "--build")?;
    if build.is_empty() || !build.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("--build must contain only ASCII digits".into());
    }
    let mut effects = BTreeSet::new();
    while let Some(value) = take_optional_value(&mut values, "--effect") {
        let effect = text(value, "--effect")?
            .parse::<i64>()
            .map_err(|_| "--effect requires an integer".to_owned())?;
        if !SUPPORTED_EFFECT_IDS.contains(&effect) {
            return Err(format!("unsupported controlled effect {effect}"));
        }
        effects.insert(effect);
    }
    if effects.is_empty() {
        return Err("at least one --effect is required".into());
    }
    let arguments = Arguments {
        build,
        effects,
        pack: PathBuf::from(take_value(&mut values, "--pack")?),
        capture: PathBuf::from(take_value(&mut values, "--capture")?),
        connections: PathBuf::from(take_value(&mut values, "--connections")?),
        journal: PathBuf::from(take_value(&mut values, "--journal")?),
        rlog: PathBuf::from(take_value(&mut values, "--rlog")?),
        coverage: PathBuf::from(take_value(&mut values, "--coverage")?),
        output: PathBuf::from(take_value(&mut values, "--output")?),
    };
    if !values.is_empty() {
        return Err(usage());
    }
    Ok(arguments)
}

fn take_value(values: &mut Vec<OsString>, flag: &str) -> Result<OsString, String> {
    let position = values
        .iter()
        .position(|value| value == flag)
        .ok_or_else(usage)?;
    if position + 1 >= values.len() {
        return Err(format!("{flag} requires a value"));
    }
    let value = values.remove(position + 1);
    values.remove(position);
    Ok(value)
}

fn take_optional_value(values: &mut Vec<OsString>, flag: &str) -> Option<OsString> {
    let position = values.iter().position(|value| value == flag)?;
    if position + 1 >= values.len() {
        return Some(OsString::new());
    }
    let value = values.remove(position + 1);
    values.remove(position);
    Some(value)
}

fn text(value: OsString, flag: &str) -> Result<String, String> {
    value
        .into_string()
        .map_err(|_| format!("{flag} requires UTF-8 text"))
}

fn usage() -> String {
    "usage: rlogs-bpsr-controlled-formula-capture-receipt --build <id> --effect <id> [--effect <id> ...] --pack <pack.json> --capture <pcap|pcapng> --connections <json> --journal <protocol.jsonl> --rlog <sealed.rlog> --coverage <coverage.json> --output <receipt.json>".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rlogs_events::{
        AbilityId, ActorId, DamageEvent, DamageFlags, DamagePacketDetail, EntityRef, EntityUuid,
        StatusEffectId, StatusEffectInstanceId, StatusEvent,
    };

    fn actor(actor_id: u64, entity_uuid: i64) -> EntityRef {
        EntityRef {
            actor_id: ActorId(actor_id),
            entity_uuid: EntityUuid(entity_uuid),
        }
    }

    fn timeline(kind: TimelineEventKind) -> CanonicalEvent {
        CanonicalEvent::Timeline(rlogs_events::TimelineEvent {
            sequence: 1,
            time: rlogs_events::EventTime {
                observed_micros: 1,
                game_time_millis: None,
            },
            provenance: rlogs_events::EventProvenance::manual("controlled-receipt-test"),
            kind,
        })
    }

    #[test]
    fn target_window_counts_external_damage_without_remote_casts() {
        let mut state = CanonicalAccumulator::default();
        state.effect_accumulators.insert(
            3_003_012,
            EffectAccumulator {
                receipt: EffectReceipt {
                    effect_id: 3_003_012,
                    ..EffectReceipt::default()
                },
                ..EffectAccumulator::default()
            },
        );
        observe_canonical(
            &mut state,
            1,
            &timeline(TimelineEventKind::Status(StatusEvent {
                source: Some(actor(1, 101)),
                target: actor(9, 909),
                effect: StatusEffectId(3_003_012),
                instance_id: Some(StatusEffectInstanceId(77)),
                origin: None,
                state: StatusState::Applied,
                stacks: Some(1),
                duration_millis: Some(1_100),
                level: Some(1),
                part_id: None,
                count: None,
                created_at_millis: Some(5),
            })),
        );
        observe_canonical(
            &mut state,
            2,
            &timeline(TimelineEventKind::Damage(DamageEvent {
                source: actor(2, 202),
                direct_source: None,
                target: actor(9, 909),
                ability: Some(AbilityId(55_240)),
                amount: 100,
                actual_amount: Some(100),
                hp_loss: Some(100),
                shield_loss: Some(0),
                hit_event_id: Some(1),
                damage_source: Some(1),
                damage_type: Some(1),
                flags: DamageFlags {
                    critical: Some(false),
                    lucky: Some(false),
                    ..DamageFlags::default()
                },
                packet: DamagePacketDetail {
                    skill_effect_uuid: Some(8_888),
                    ..DamagePacketDetail::default()
                },
            })),
        );
        observe_canonical(
            &mut state,
            3,
            &timeline(TimelineEventKind::Status(StatusEvent {
                source: None,
                target: actor(9, 909),
                effect: StatusEffectId(3_003_012),
                instance_id: Some(StatusEffectInstanceId(77)),
                origin: None,
                state: StatusState::Removed,
                stacks: Some(0),
                duration_millis: None,
                level: None,
                part_id: None,
                count: None,
                created_at_millis: None,
            })),
        );
        let effect = &state.effect_accumulators[&3_003_012];
        assert_eq!(effect.receipt.exact_closed_windows, 1);
        assert_eq!(effect.receipt.target_incoming_damage_events_in_windows, 1);
        assert_eq!(effect.external_target_damage_sources, BTreeSet::from([202]));
    }

    #[test]
    fn readiness_never_grants_formula_or_provider_credit() {
        let mut receipt = ControlledCaptureReceipt {
            schema_version: SCHEMA_VERSION,
            generated_by: GENERATED_BY.into(),
            game_build: "24687926".into(),
            effect_ids: vec![3_003_012],
            inputs: ReceiptInputs {
                protocol_pack: fixture_file(),
                raw_capture: fixture_file(),
                exact_connections: fixture_file(),
                raw_protocol_journal: fixture_file(),
                sealed_canonical_rlog: fixture_file(),
                replay_coverage: fixture_file(),
            },
            identity: IdentityReceipt {
                protocol_pack_id: "fixture".into(),
                protocol_pack_digest: format!("sha256:{}", "0".repeat(64)),
                journal_capture_id: "fixture".into(),
                canonical_session_id: "fixture".into(),
                connection_count: 1,
                journal_build_matches: true,
                canonical_build_matches: true,
                pack_digest_matches_journal_and_canonical: true,
                coverage_matches_pack_and_canonical_seal: true,
                coverage_source_matches_journal_capture: true,
                coverage_session_matches_canonical_session: true,
                coverage_capture_counters_match_journal: true,
                raw_capture_and_connections_names_match_capture_id: true,
            },
            journal: JournalReceipt {
                record_count: 1,
                packet_count: 1,
                gap_count: 0,
                wire_bytes: 1,
                application_bytes: 1,
                packets_with_empty_wire_payload: 0,
                packets_without_application_payload: 0,
                first_observed_micros: Some(1),
                last_observed_micros: Some(1),
                ends_with_newline: true,
                strict_zero_gap_closed_file: true,
            },
            canonical: empty_canonical(),
            policy: PolicyReceipt {
                numeric_effect_ids_and_build_are_authoritative: true,
                raw_capture_and_unknown_records_are_retained: true,
                current_character_snapshot_substitution_allowed: false,
                structurally_absent_remote_cast_packets_required: false,
                acquisition_receipt_alone_promotes_formula: false,
            },
            readiness: ReadinessReceipt {
                exact_input_identity_bound: true,
                zero_gap_closed_interval: true,
                raw_payload_bytes_preserved: true,
                canonical_rlog_seal_valid: true,
                canonical_formula_operand_schema_complete: true,
                controlled_capture_observed_every_required_operand: false,
                operation_order_and_integer_rounding_proven: false,
                formula_authority: false,
                runtime_promotion_allowed: false,
                provider_rdps_credit_allowed: false,
                missing_obligations: vec!["controlled evidence absent".into()],
            },
            content_sha256: String::new(),
        };
        receipt.content_sha256 = content_sha256(&receipt).unwrap();
        assert!(validate_receipt(&receipt).is_ok());
        receipt.readiness.provider_rdps_credit_allowed = true;
        receipt.content_sha256 = content_sha256(&receipt).unwrap();
        assert!(validate_receipt(&receipt).is_err());
    }

    #[test]
    fn exact_identity_rejects_mismatched_source_session_and_capture_counters() {
        let inputs = ReceiptInputs {
            protocol_pack: fixture_file(),
            raw_capture: fixture_file(),
            exact_connections: fixture_file(),
            raw_protocol_journal: fixture_file(),
            sealed_canonical_rlog: fixture_file(),
            replay_coverage: fixture_file(),
        };
        let identity = fixture_identity();
        assert!(exact_input_identity_bound(&identity, &inputs));

        let mutations: [fn(&mut IdentityReceipt); 3] = [
            |identity: &mut IdentityReceipt| {
                identity.coverage_source_matches_journal_capture = false
            },
            |identity: &mut IdentityReceipt| {
                identity.coverage_session_matches_canonical_session = false
            },
            |identity: &mut IdentityReceipt| {
                identity.coverage_capture_counters_match_journal = false
            },
        ];
        for mutate in mutations {
            let mut mismatched = identity.clone();
            mutate(&mut mismatched);
            assert!(!exact_input_identity_bound(&mismatched, &inputs));
        }
    }

    #[test]
    fn raw_capture_and_connections_are_bound_by_capture_id_names() {
        assert!(capture_input_names_match_id(
            Path::new("controlled-001.pcapng"),
            Path::new("controlled-001.connections.json"),
            "controlled-001",
        ));
        assert!(!capture_input_names_match_id(
            Path::new("another.pcap"),
            Path::new("controlled-001.connections.json"),
            "controlled-001",
        ));
    }

    #[test]
    fn aggregate_local_operands_do_not_satisfy_an_unlinked_effect_actor() {
        let required = required_attribute_ids().into_iter().collect::<Vec<_>>();
        let mut canonical = empty_canonical();
        canonical.packet_proven_local_actor_entity_uuids = vec!["101".into(), "202".into()];
        canonical.one_local_actor_has_every_required_attribute_snapshot = true;
        canonical.local_attribute_snapshots = vec![LocalAttributeReceipt {
            entity_uuid: "101".into(),
            snapshot_attribute_ids: required,
            missing_required_snapshot_attribute_ids: Vec::new(),
            cooldown_events: 1,
            resource_events: 1,
            client_action_timing_events: 1,
            complete_run_aggregate_context: true,
        }];
        canonical.cooldown_events_for_local_actors = 1;
        canonical.resource_events_for_local_actors = 1;
        canonical.client_action_timing_events = 1;
        canonical.effects = vec![EffectReceipt {
            effect_id: 3_003_411,
            applied_events: 1,
            applied_events_with_provider: 1,
            terminal_events: 1,
            exact_closed_windows: 1,
            windows_targeting_packet_proven_local_actor: 1,
            recipient_outgoing_damage_events_in_windows: 1,
            linked_local_actor_entity_uuids: vec!["202".into()],
            linked_local_actor_entity_uuids_with_complete_context: Vec::new(),
            exact_provider_status_lifecycle_preserved: true,
            ..EffectReceipt::default()
        }];
        let missing = missing_obligations(
            &canonical,
            &BTreeSet::from([3_003_411]),
            true,
            true,
            true,
            0,
            0,
        );
        assert!(missing.iter().any(|entry| {
            entry.contains(
                "linked local recipient/attacker lacks a complete pre-application attribute snapshot and same-actor action-timing, cooldown, and resource observations in the exact bounded lifecycle",
            )
        }));
    }

    #[test]
    fn complete_snapshot_after_effect_window_cannot_backfill_earlier_damage() {
        let actor_id = 202_i64;
        let applied = TimedObservation {
            sequence: 10,
            observed_micros: 1_000_000,
        };
        let damage = TimedObservation {
            sequence: 11,
            observed_micros: 1_100_000,
        };
        let terminal = TimedObservation {
            sequence: 12,
            observed_micros: 1_200_000,
        };
        let later_snapshot = TimedObservation {
            sequence: 13,
            observed_micros: 1_300_000,
        };
        let required = required_attribute_ids();
        let mut state = CanonicalAccumulator::default();
        state.resource_actor_entity_uuids.insert(actor_id);
        state.action_timing_actor_entity_uuids.insert(actor_id);
        state.snapshot_attributes.insert(actor_id, required.clone());
        state.snapshot_attribute_observations.insert(
            actor_id,
            vec![AttributeSnapshotObservation {
                at: later_snapshot,
                attribute_ids: required.iter().copied().collect(),
            }],
        );
        state.cooldown_events_by_actor.insert(actor_id, 1);
        state.resource_events_by_actor.insert(actor_id, 1);
        state.action_timing_events_by_actor.insert(actor_id, 1);
        state
            .cooldown_observations_by_actor
            .insert(actor_id, vec![damage]);
        state
            .resource_observations_by_actor
            .insert(actor_id, vec![damage]);
        state
            .action_timing_observations_by_actor
            .insert(actor_id, vec![damage]);
        state.effect_accumulators.insert(
            3_003_012,
            EffectAccumulator {
                receipt: EffectReceipt {
                    effect_id: 3_003_012,
                    applied_events: 1,
                    applied_events_with_provider: 1,
                    terminal_events: 1,
                    exact_closed_windows: 1,
                    ..EffectReceipt::default()
                },
                closed_targets: BTreeSet::from([909]),
                all_target_damage_sources: BTreeSet::from([actor_id]),
                all_target_damage_context_actors: BTreeSet::from([actor_id]),
                external_target_damage_sources: BTreeSet::from([actor_id]),
                closed_windows: vec![EffectWindow {
                    provider_entity_uuid: Some(101),
                    target_entity_uuid: 909,
                    instance_id: 77,
                    applied,
                    terminal: Some(terminal),
                    target_incoming_damage_events: 1,
                    target_damage_sources: BTreeSet::from([actor_id]),
                    target_damage_context_actors: BTreeSet::from([actor_id]),
                    relevant_damage_by_actor: BTreeMap::from([(actor_id, damage)]),
                    ..EffectWindow::default()
                }],
                ..EffectAccumulator::default()
            },
        );
        let canonical = finish_canonical(
            state,
            rlogs_log_format::RlogReplaySummary {
                event_count: 13,
                first_observed_micros: Some(1),
                last_observed_micros: Some(1_300_000),
                content_sha256: format!("sha256:{}", "0".repeat(64)),
            },
        );
        let effect = &canonical.effects[0];
        assert_eq!(effect.linked_local_actor_entity_uuids, ["202"]);
        assert!(
            effect
                .linked_local_actor_entity_uuids_with_complete_context
                .is_empty()
        );
        assert_eq!(
            effect.linked_actor_window_contexts[0]
                .latest_complete_snapshot_sequence_before_application,
            None
        );
        assert!(!effect.linked_actor_window_contexts[0].complete_temporal_context);
    }

    fn fixture_file() -> FileReceipt {
        FileReceipt {
            path: "fixture".into(),
            bytes: 1,
            sha256: format!("sha256:{}", "0".repeat(64)),
        }
    }

    fn fixture_identity() -> IdentityReceipt {
        IdentityReceipt {
            protocol_pack_id: "fixture".into(),
            protocol_pack_digest: format!("sha256:{}", "0".repeat(64)),
            journal_capture_id: "fixture".into(),
            canonical_session_id: "fixture".into(),
            connection_count: 1,
            journal_build_matches: true,
            canonical_build_matches: true,
            pack_digest_matches_journal_and_canonical: true,
            coverage_matches_pack_and_canonical_seal: true,
            coverage_source_matches_journal_capture: true,
            coverage_session_matches_canonical_session: true,
            coverage_capture_counters_match_journal: true,
            raw_capture_and_connections_names_match_capture_id: true,
        }
    }

    fn empty_canonical() -> CanonicalOperandReceipt {
        CanonicalOperandReceipt {
            event_count: 0,
            content_sha256: format!("sha256:{}", "0".repeat(64)),
            data_gap_events: 0,
            unresolved_status_events: 0,
            unresolved_action_events: 0,
            status_events: 0,
            entity_attribute_snapshot_events: 0,
            entity_attribute_delta_events: 0,
            temporary_attribute_snapshot_events: 0,
            temporary_attribute_delta_events: 0,
            packet_proven_local_actor_entity_uuids: Vec::new(),
            local_attribute_snapshots: Vec::new(),
            local_temporary_attribute_ids: Vec::new(),
            one_local_actor_has_every_required_attribute_snapshot: false,
            required_attribute_ids: required_attribute_ids().into_iter().collect(),
            cooldown_events_for_local_actors: 0,
            resource_events_for_local_actors: 0,
            client_action_timing_events: 0,
            damage_events: 0,
            damage_events_with_ability_id: 0,
            damage_events_with_hit_event_id: 0,
            damage_events_with_critical_and_lucky_flags: 0,
            damage_events_with_action_instance_candidate: 0,
            damage_targets: 0,
            damage_targets_spawned_before_first_damage: 0,
            damage_targets_with_current_hp_evidence: 0,
            status_events_on_damage_targets: 0,
            effects: Vec::new(),
        }
    }

    #[test]
    fn argument_parser_accepts_all_three_controlled_families() {
        let args = arguments_from(
            [
                "--build",
                "24687926",
                "--effect",
                "3003012",
                "--effect",
                "3003411",
                "--effect",
                "2203031",
                "--effect",
                "2205031",
                "--pack",
                "pack.json",
                "--capture",
                "capture.pcapng",
                "--connections",
                "connections.json",
                "--journal",
                "capture.protocol.jsonl",
                "--rlog",
                "capture.rlog",
                "--coverage",
                "capture.coverage.json",
                "--output",
                "receipt.json",
            ]
            .into_iter()
            .map(OsString::from)
            .collect(),
        )
        .unwrap();
        assert_eq!(args.effects.len(), 4);
    }
}
