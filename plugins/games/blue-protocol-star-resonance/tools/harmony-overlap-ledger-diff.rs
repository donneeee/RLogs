use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsString,
    fs::File,
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{
    ActorState, CanonicalEvent, EntityRef, EventProvenance, RunState, StatusEvent, StatusOrigin,
    StatusState, TimelineEventKind, UnresolvedStatusEvent, UnresolvedStatusReason,
};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 2;
const HARMONY_GRACE_EFFECT_ID: i64 = 3_003_052;
const DEFAULT_TERMINAL_LOOKBACK_MICROS: u64 = 30_000_000;
const MAX_RECENT_TERMINALS_PER_ENDPOINT: usize = 32;
const MAX_SIGNATURE_SAMPLES: usize = 8;

#[derive(Debug)]
struct Arguments {
    rlog: PathBuf,
    trusted_ledger: PathBuf,
    candidate_ledger: PathBuf,
    output: PathBuf,
    terminal_lookback_micros: u64,
    summary_only: bool,
}

#[derive(Clone, Debug, Deserialize)]
struct ReplayAuditBundle {
    schema_version: u16,
    reports: Vec<ReplayAuditReport>,
}

#[derive(Clone, Debug, Deserialize)]
struct ReplayAuditReport {
    source_path: String,
    session_id: String,
    deployment_id: String,
    client_build: String,
    protocol_pack_digest: String,
    event_count: u64,
    harmony_grace_audit_gates: BTreeMap<String, u64>,
    target_vulnerability_audit_gates: BTreeMap<String, u64>,
    emitted_contribution_ledger: Vec<LedgerRow>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct LedgerRow {
    sequence: u64,
    capture_sequence: Option<u64>,
    observed_micros: u64,
    effect_id: i64,
    provider_actor_id: u64,
    provider_entity_uuid: Option<String>,
    recipient_actor_id: u64,
    recipient_entity_uuid: Option<String>,
    affected_damage_id: Option<i64>,
    damage_source_actor_id: Option<String>,
    damage_source_entity_uuid: Option<String>,
    target_actor_id: Option<String>,
    target_entity_uuid: Option<String>,
    numerator: String,
    denominator: String,
    observed_damage: String,
}

#[derive(Clone, Debug, Serialize)]
struct InputArtifact {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema_version: u16,
    generated_by: &'static str,
    policy: Policy,
    inputs: Inputs,
    identity: Identity,
    comparison: Comparison,
    unresolved_lifecycle_models: Vec<ModelComparison>,
    discriminating_signatures: Vec<SignatureSummary>,
    row_details_included: bool,
    rows: Vec<RowSnapshot>,
}

#[derive(Debug, Serialize)]
struct Policy {
    runtime_use: &'static str,
    processing: &'static str,
    authority: &'static str,
    cast_packets: &'static str,
    unresolved_evidence: &'static str,
    duration_semantics: &'static str,
    interpretation: &'static str,
}

#[derive(Debug, Serialize)]
struct Inputs {
    rlog: InputArtifact,
    trusted_ledger: InputArtifact,
    candidate_ledger: InputArtifact,
}

#[derive(Debug, Serialize)]
struct Identity {
    session_id: String,
    deployment_id: String,
    client_build: String,
    protocol_pack_digest: String,
    rlog_event_count: u64,
    sealed: bool,
}

#[derive(Debug, Serialize)]
struct Comparison {
    effect_id: i64,
    trusted_rows: usize,
    candidate_rows: usize,
    old_included_rows: usize,
    old_suppressed_rows: usize,
    trusted_is_exact_subset_of_candidate: bool,
    candidate_rows_matched_in_rlog: usize,
    trusted_harmony_individually_emitted_rows: u64,
    candidate_harmony_individually_emitted_rows: u64,
    trusted_unresolved_status_confounder_damage_rows: u64,
    candidate_unresolved_status_confounder_damage_rows: u64,
    unmatched_candidate_sequences: Vec<u64>,
    damage_identity_mismatches: Vec<DamageIdentityMismatch>,
}

#[derive(Debug, Serialize)]
struct DamageIdentityMismatch {
    sequence: u64,
    fields: Vec<&'static str>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum RowClass {
    OldIncluded,
    OldSuppressed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum EndpointRole {
    Provider,
    RecipientDamageSource,
    RecipientOrEnemyTarget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum UnresolvedModel {
    TerminalAwareExactInstance,
    StickyNonterminalOrUnknown,
    StickyEveryObservation,
}

impl UnresolvedModel {
    const ALL: [Self; 3] = [
        Self::TerminalAwareExactInstance,
        Self::StickyNonterminalOrUnknown,
        Self::StickyEveryObservation,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::TerminalAwareExactInstance => "terminal_aware_exact_instance",
            Self::StickyNonterminalOrUnknown => "sticky_nonterminal_or_unknown",
            Self::StickyEveryObservation => "sticky_every_observation",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct ExactStatusKey {
    target_actor_id: u64,
    target_entity_uuid: i64,
    effect_id: i64,
    instance_id: Option<i64>,
    source_actor_id: Option<u64>,
    source_entity_uuid: Option<i64>,
}

#[derive(Clone, Debug)]
struct ExactStatusTrace {
    key: ExactStatusKey,
    origin: Option<StatusOrigin>,
    state: StatusState,
    stacks: Option<u32>,
    duration_millis: Option<u64>,
    level: Option<i32>,
    part_id: Option<i32>,
    count: Option<i32>,
    created_at_millis: Option<i64>,
    first_sequence: u64,
    last_sequence: u64,
    first_observed_micros: u64,
    last_observed_micros: u64,
    nominal_expiry_micros: Option<u64>,
    last_provenance: EventProvenance,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct UnresolvedKey {
    target_actor_id: u64,
    instance_id: Option<i64>,
}

#[derive(Clone, Debug)]
struct UnresolvedTrace {
    key: UnresolvedKey,
    source: Option<EntityRef>,
    target: EntityRef,
    state: Option<StatusState>,
    wire_event_type: Option<i32>,
    wire_logic_type: Option<i32>,
    reason: UnresolvedStatusReason,
    raw_payload_bytes: usize,
    raw_payload_sha256: String,
    first_sequence: u64,
    last_sequence: u64,
    first_observed_micros: u64,
    last_observed_micros: u64,
    last_provenance: EventProvenance,
}

#[derive(Clone, Debug, Serialize)]
struct ExactStatusSnapshot {
    endpoint_role: EndpointRole,
    effect_id: i64,
    instance_id: Option<i64>,
    source_actor_id: Option<String>,
    source_entity_uuid: Option<String>,
    target_actor_id: String,
    target_entity_uuid: String,
    origin: Option<StatusOrigin>,
    last_state: StatusState,
    stacks: Option<u32>,
    duration_millis: Option<u64>,
    level: Option<i32>,
    part_id: Option<i32>,
    count: Option<i32>,
    created_at_millis: Option<i64>,
    first_sequence: u64,
    last_sequence: u64,
    first_observed_micros: u64,
    last_observed_micros: u64,
    nominal_expiry_micros: Option<u64>,
    nominally_expired_before_damage: bool,
    last_provenance: EventProvenance,
}

#[derive(Clone, Debug, Serialize)]
struct UnresolvedSnapshot {
    endpoint_role: EndpointRole,
    source_actor_id: Option<String>,
    source_entity_uuid: Option<String>,
    target_actor_id: String,
    target_entity_uuid: String,
    instance_id: Option<i64>,
    last_state: Option<StatusState>,
    wire_event_type: Option<i32>,
    wire_logic_type: Option<i32>,
    reason: UnresolvedStatusReason,
    raw_payload_bytes: usize,
    raw_payload_sha256: String,
    first_sequence: u64,
    last_sequence: u64,
    first_observed_micros: u64,
    last_observed_micros: u64,
    last_provenance: EventProvenance,
}

#[derive(Clone, Debug, Serialize)]
struct RecentTerminalSnapshot {
    endpoint_role: EndpointRole,
    effect_id: i64,
    instance_id: Option<i64>,
    source_actor_id: Option<String>,
    source_entity_uuid: Option<String>,
    target_actor_id: String,
    target_entity_uuid: String,
    terminal_state: StatusState,
    sequence: u64,
    observed_micros: u64,
    provenance: EventProvenance,
}

#[derive(Debug, Serialize)]
struct RowSnapshot {
    class: RowClass,
    sequence: u64,
    capture_sequence: Option<u64>,
    observed_micros: u64,
    provider: EntityIdentity,
    recipient_damage_source: EntityIdentity,
    recipient_or_enemy_target: EntityIdentity,
    affected_damage_id: Option<i64>,
    hit_event_id: Option<i32>,
    observed_damage: String,
    contribution_numerator: String,
    contribution_denominator: String,
    active_exact_statuses: Vec<ExactStatusSnapshot>,
    recent_terminal_exact_statuses: Vec<RecentTerminalSnapshot>,
    recent_terminal_exact_statuses_truncated: bool,
    active_unresolved_by_model: BTreeMap<&'static str, Vec<UnresolvedSnapshot>>,
    blocked_by_unresolved_model: BTreeMap<&'static str, bool>,
    signature: StateSignature,
}

#[derive(Clone, Debug, Serialize)]
struct EntityIdentity {
    actor_id: String,
    entity_uuid: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize)]
struct StateSignature {
    provider_active_effect_ids: Vec<i64>,
    recipient_active_effect_ids: Vec<i64>,
    target_active_effect_ids: Vec<i64>,
    provider_nominally_expired_effect_ids: Vec<i64>,
    recipient_nominally_expired_effect_ids: Vec<i64>,
    target_nominally_expired_effect_ids: Vec<i64>,
    current_unresolved_provider: usize,
    current_unresolved_recipient: usize,
    current_unresolved_target: usize,
    sticky_any_unresolved_provider: usize,
    sticky_any_unresolved_recipient: usize,
    sticky_any_unresolved_target: usize,
}

#[derive(Debug, Default)]
struct SignatureAccumulator {
    old_included_rows: usize,
    old_suppressed_rows: usize,
    sample_sequences: Vec<u64>,
}

#[derive(Debug, Serialize)]
struct SignatureSummary {
    signature: StateSignature,
    old_included_rows: usize,
    old_suppressed_rows: usize,
    discriminates_class: Option<RowClass>,
    sample_sequences: Vec<u64>,
}

#[derive(Debug, Serialize)]
struct ModelComparison {
    model: &'static str,
    semantics: &'static str,
    allowed_rows: usize,
    blocked_rows: usize,
    true_old_included: usize,
    true_old_suppressed: usize,
    false_included: usize,
    false_suppressed: usize,
    exact_match_to_trusted_ledger: bool,
    false_included_sequences: Vec<u64>,
    false_suppressed_sequences: Vec<u64>,
}

#[derive(Default)]
struct LifecycleState {
    active_exact: BTreeMap<ExactStatusKey, ExactStatusTrace>,
    latest_exact: BTreeMap<ExactStatusKey, ExactStatusTrace>,
    unresolved: BTreeMap<UnresolvedModel, BTreeMap<UnresolvedKey, UnresolvedTrace>>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Harmony overlap ledger diff failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = arguments()?;
    let trusted_artifact = artifact(&args.trusted_ledger)?;
    let candidate_artifact = artifact(&args.candidate_ledger)?;
    let rlog_artifact = artifact(&args.rlog)?;
    let trusted = read_audit(&args.trusted_ledger)?;
    let candidate = read_audit(&args.candidate_ledger)?;
    let (trusted_report, candidate_report) = validate_reports(&trusted, &candidate)?;
    let trusted_by_sequence = ledger_by_sequence(&trusted_report.emitted_contribution_ledger)?;
    let candidate_by_sequence = ledger_by_sequence(&candidate_report.emitted_contribution_ledger)?;
    if !trusted_by_sequence
        .iter()
        .all(|(sequence, trusted)| candidate_by_sequence.get(sequence) == Some(trusted))
    {
        return Err("trusted ledger is not an exact row subset of candidate ledger".into());
    }

    let mut reader = RlogReader::new(
        BufReader::new(File::open(&args.rlog)?),
        RlogLimits::default(),
    )?;
    validate_header(&reader, trusted_report)?;
    let identity = Identity {
        session_id: reader.header().session_id.clone(),
        deployment_id: reader.header().region.identity.deployment_id.clone(),
        client_build: reader.header().region.client_build.clone(),
        protocol_pack_digest: reader.header().region.protocol_pack_digest.clone(),
        rlog_event_count: 0,
        sealed: false,
    };
    let mut state = LifecycleState::default();
    for model in UnresolvedModel::ALL {
        state.unresolved.entry(model).or_default();
    }
    let mut rows = Vec::with_capacity(candidate_by_sequence.len());
    let mut matched_sequences = BTreeSet::new();
    let mut damage_identity_mismatches = Vec::new();
    let mut event_count = 0_u64;

    while let Some(envelope) = reader.next_event()? {
        event_count = event_count.saturating_add(1);
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            continue;
        };
        match &timeline.kind {
            TimelineEventKind::RunBoundary {
                state: RunState::Entered,
                ..
            }
            | TimelineEventKind::EncounterBoundary {
                state: rlogs_events::EncounterState::Wiped,
                ..
            } => state.clear_all(),
            TimelineEventKind::Actor(actor)
                if matches!(
                    actor.state,
                    ActorState::Spawned | ActorState::Transformed | ActorState::Despawned
                ) =>
            {
                state.clear_actor(actor.actor.actor_id.0)
            }
            TimelineEventKind::Status(status) => state.observe_exact(
                envelope.sequence,
                envelope.time.observed_micros,
                &envelope.provenance,
                status,
            ),
            TimelineEventKind::UnresolvedStatus(status) => state.observe_unresolved(
                envelope.sequence,
                envelope.time.observed_micros,
                &envelope.provenance,
                status,
            ),
            TimelineEventKind::Damage(damage) => {
                let Some(ledger) = candidate_by_sequence.get(&envelope.sequence) else {
                    continue;
                };
                matched_sequences.insert(envelope.sequence);
                let mismatches = damage_identity_mismatch(ledger, damage);
                if !mismatches.is_empty() {
                    damage_identity_mismatches.push(DamageIdentityMismatch {
                        sequence: envelope.sequence,
                        fields: mismatches,
                    });
                }
                let class = if trusted_by_sequence.contains_key(&envelope.sequence) {
                    RowClass::OldIncluded
                } else {
                    RowClass::OldSuppressed
                };
                rows.push(snapshot_row(
                    class,
                    ledger,
                    damage,
                    &state,
                    args.terminal_lookback_micros,
                )?);
            }
            _ => {}
        }
    }
    let sealed = reader.summary().is_some();
    if !sealed {
        return Err(format!("{} is not a sealed canonical rlog", args.rlog.display()).into());
    }
    if event_count != trusted_report.event_count || event_count != candidate_report.event_count {
        return Err(format!(
            "rlog event count {event_count} does not match ledgers {} and {}",
            trusted_report.event_count, candidate_report.event_count
        )
        .into());
    }

    rows.sort_by_key(|row| row.sequence);
    let unresolved_lifecycle_models = compare_models(&rows);
    let discriminating_signatures = summarize_signatures(&rows);
    let unmatched_candidate_sequences = candidate_by_sequence
        .keys()
        .filter(|sequence| !matched_sequences.contains(sequence))
        .copied()
        .collect::<Vec<_>>();
    let receipt = Receipt {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-harmony-overlap-ledger-diff",
        policy: Policy {
            runtime_use: "offline_differential_evidence_only_never_runtime_attribution",
            processing: "single_pass_streaming_rlog_with_state_bounded_by_observed_lifecycles_and_223_selected_rows",
            authority: "exact_numeric_ids_rlog_build_protocol_pack_and_input_hashes",
            cast_packets: "not_required_status_and_damage_rows_use_their_own_exact_wire_identity",
            unresolved_evidence: "retained_under_multiple_explicit_lifecycle_models_never_mapped_to_an_invented_effect",
            duration_semantics: "nominal_expiry_is_derived_from_observed_time_plus_positive_duration_and_reported_but_does_not_silently_replace_terminal_lifecycle_evidence",
            interpretation: "a model match explains_historical_suppression_behavior_only_and_does_not_prove_a_damage_formula_or_runtime_credit",
        },
        inputs: Inputs {
            rlog: rlog_artifact,
            trusted_ledger: trusted_artifact,
            candidate_ledger: candidate_artifact,
        },
        identity: Identity {
            rlog_event_count: event_count,
            sealed,
            ..identity
        },
        comparison: Comparison {
            effect_id: HARMONY_GRACE_EFFECT_ID,
            trusted_rows: trusted_by_sequence.len(),
            candidate_rows: candidate_by_sequence.len(),
            old_included_rows: rows
                .iter()
                .filter(|row| row.class == RowClass::OldIncluded)
                .count(),
            old_suppressed_rows: rows
                .iter()
                .filter(|row| row.class == RowClass::OldSuppressed)
                .count(),
            trusted_is_exact_subset_of_candidate: true,
            candidate_rows_matched_in_rlog: matched_sequences.len(),
            trusted_harmony_individually_emitted_rows: gate_count(
                &trusted_report.harmony_grace_audit_gates,
                "emitted",
            ),
            candidate_harmony_individually_emitted_rows: gate_count(
                &candidate_report.harmony_grace_audit_gates,
                "emitted",
            ),
            trusted_unresolved_status_confounder_damage_rows: gate_count(
                &trusted_report.target_vulnerability_audit_gates,
                "unresolved_status_confounder",
            ),
            candidate_unresolved_status_confounder_damage_rows: gate_count(
                &candidate_report.target_vulnerability_audit_gates,
                "unresolved_status_confounder",
            ),
            unmatched_candidate_sequences,
            damage_identity_mismatches,
        },
        unresolved_lifecycle_models,
        discriminating_signatures,
        row_details_included: !args.summary_only,
        rows: if args.summary_only { Vec::new() } else { rows },
    };

    let mut writer = BufWriter::new(File::create(&args.output)?);
    serde_json::to_writer_pretty(&mut writer, &receipt)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn read_audit(path: &Path) -> Result<ReplayAuditBundle, Box<dyn std::error::Error>> {
    Ok(serde_json::from_reader(BufReader::new(File::open(path)?))?)
}

fn validate_reports<'a>(
    trusted: &'a ReplayAuditBundle,
    candidate: &'a ReplayAuditBundle,
) -> Result<(&'a ReplayAuditReport, &'a ReplayAuditReport), Box<dyn std::error::Error>> {
    if trusted.reports.len() != 1 || candidate.reports.len() != 1 {
        return Err("each replay audit must contain exactly one report".into());
    }
    let left = &trusted.reports[0];
    let right = &candidate.reports[0];
    for (name, matches) in [
        ("session_id", left.session_id == right.session_id),
        ("source_path", left.source_path == right.source_path),
        ("deployment_id", left.deployment_id == right.deployment_id),
        ("client_build", left.client_build == right.client_build),
        (
            "protocol_pack_digest",
            left.protocol_pack_digest == right.protocol_pack_digest,
        ),
        ("event_count", left.event_count == right.event_count),
    ] {
        if !matches {
            return Err(format!("replay audit {name} mismatch").into());
        }
    }
    if left.emitted_contribution_ledger.len() != 39 {
        return Err(format!(
            "trusted ledger must contain 39 rows, found {}",
            left.emitted_contribution_ledger.len()
        )
        .into());
    }
    if right.emitted_contribution_ledger.len() != 223 {
        return Err(format!(
            "candidate ledger must contain 223 rows, found {}",
            right.emitted_contribution_ledger.len()
        )
        .into());
    }
    if trusted.schema_version == 0 || candidate.schema_version == 0 {
        return Err("replay audit schema version must be positive".into());
    }
    Ok((left, right))
}

fn validate_header(
    reader: &RlogReader<BufReader<File>>,
    report: &ReplayAuditReport,
) -> Result<(), Box<dyn std::error::Error>> {
    let header = reader.header();
    for (name, matches) in [
        ("session_id", header.session_id == report.session_id),
        (
            "deployment_id",
            header.region.identity.deployment_id == report.deployment_id,
        ),
        (
            "client_build",
            header.region.client_build == report.client_build,
        ),
        (
            "protocol_pack_digest",
            header.region.protocol_pack_digest == report.protocol_pack_digest,
        ),
    ] {
        if !matches {
            return Err(format!("rlog header {name} does not match ledger report").into());
        }
    }
    Ok(())
}

fn ledger_by_sequence(rows: &[LedgerRow]) -> Result<BTreeMap<u64, &LedgerRow>, String> {
    let mut result = BTreeMap::new();
    for row in rows {
        if row.effect_id != HARMONY_GRACE_EFFECT_ID {
            return Err(format!(
                "ledger row {} has effect {}, expected {}",
                row.sequence, row.effect_id, HARMONY_GRACE_EFFECT_ID
            ));
        }
        if result.insert(row.sequence, row).is_some() {
            return Err(format!("duplicate ledger sequence {}", row.sequence));
        }
    }
    Ok(result)
}

fn gate_count(gates: &BTreeMap<String, u64>, gate: &str) -> u64 {
    gates.get(gate).copied().unwrap_or_default()
}

impl LifecycleState {
    fn clear_all(&mut self) {
        self.active_exact.clear();
        self.latest_exact.clear();
        for windows in self.unresolved.values_mut() {
            windows.clear();
        }
    }

    fn clear_actor(&mut self, actor_id: u64) {
        self.active_exact.retain(|key, _| {
            key.target_actor_id != actor_id && key.source_actor_id != Some(actor_id)
        });
        self.latest_exact.retain(|key, _| {
            key.target_actor_id != actor_id && key.source_actor_id != Some(actor_id)
        });
        for windows in self.unresolved.values_mut() {
            windows.retain(|key, trace| {
                key.target_actor_id != actor_id
                    && trace.source.map(|source| source.actor_id.0) != Some(actor_id)
            });
        }
    }

    fn observe_exact(
        &mut self,
        sequence: u64,
        observed_micros: u64,
        provenance: &EventProvenance,
        status: &StatusEvent,
    ) {
        let key = ExactStatusKey {
            target_actor_id: status.target.actor_id.0,
            target_entity_uuid: status.target.entity_uuid.0,
            effect_id: status.effect.0,
            instance_id: status.instance_id.map(|value| value.0),
            source_actor_id: status.source.map(|source| source.actor_id.0),
            source_entity_uuid: status.source.map(|source| source.entity_uuid.0),
        };
        let previous = self.latest_exact.get(&key);
        let trace = ExactStatusTrace {
            key: key.clone(),
            origin: status
                .origin
                .or_else(|| previous.and_then(|value| value.origin)),
            state: status.state,
            stacks: status
                .stacks
                .or_else(|| previous.and_then(|value| value.stacks)),
            duration_millis: status
                .duration_millis
                .or_else(|| previous.and_then(|value| value.duration_millis)),
            level: status
                .level
                .or_else(|| previous.and_then(|value| value.level)),
            part_id: status
                .part_id
                .or_else(|| previous.and_then(|value| value.part_id)),
            count: status
                .count
                .or_else(|| previous.and_then(|value| value.count)),
            created_at_millis: status
                .created_at_millis
                .or_else(|| previous.and_then(|value| value.created_at_millis)),
            first_sequence: previous.map_or(sequence, |value| value.first_sequence),
            last_sequence: sequence,
            first_observed_micros: previous
                .map_or(observed_micros, |value| value.first_observed_micros),
            last_observed_micros: observed_micros,
            nominal_expiry_micros: nominal_expiry(observed_micros, status.duration_millis)
                .or_else(|| previous.and_then(|value| value.nominal_expiry_micros)),
            last_provenance: provenance.clone(),
        };
        self.latest_exact.insert(key.clone(), trace.clone());
        match status.state {
            StatusState::Applied | StatusState::Refreshed | StatusState::Stacked => {
                self.active_exact.insert(key, trace);
            }
            StatusState::Consumed | StatusState::Removed => {
                self.active_exact.retain(|candidate, _| {
                    if candidate.target_actor_id != key.target_actor_id
                        || candidate.target_entity_uuid != key.target_entity_uuid
                        || candidate.effect_id != key.effect_id
                    {
                        return true;
                    }
                    if let Some(instance_id) = key.instance_id {
                        candidate.instance_id != Some(instance_id)
                    } else if let Some(source_actor_id) = key.source_actor_id {
                        candidate.source_actor_id != Some(source_actor_id)
                    } else {
                        false
                    }
                });
            }
        }
    }

    fn observe_unresolved(
        &mut self,
        sequence: u64,
        observed_micros: u64,
        provenance: &EventProvenance,
        status: &UnresolvedStatusEvent,
    ) {
        let key = UnresolvedKey {
            target_actor_id: status.target.actor_id.0,
            instance_id: status.instance_id.map(|value| value.0),
        };
        for model in UnresolvedModel::ALL {
            let windows = self.unresolved.entry(model).or_default();
            let previous = windows.get(&key);
            let trace = UnresolvedTrace {
                key: key.clone(),
                source: status.source,
                target: status.target,
                state: status.state,
                wire_event_type: status.wire_event_type,
                wire_logic_type: status.wire_logic_type,
                reason: status.reason,
                raw_payload_bytes: status.raw_payload.len(),
                raw_payload_sha256: hex_digest(&status.raw_payload),
                first_sequence: previous.map_or(sequence, |value| value.first_sequence),
                last_sequence: sequence,
                first_observed_micros: previous
                    .map_or(observed_micros, |value| value.first_observed_micros),
                last_observed_micros: observed_micros,
                last_provenance: provenance.clone(),
            };
            match model {
                UnresolvedModel::TerminalAwareExactInstance => match status.state {
                    Some(StatusState::Consumed | StatusState::Removed) => {
                        if key.instance_id.is_some() {
                            windows.remove(&key);
                        }
                    }
                    Some(StatusState::Applied | StatusState::Refreshed | StatusState::Stacked)
                    | None => {
                        windows.insert(key.clone(), trace);
                    }
                },
                UnresolvedModel::StickyNonterminalOrUnknown => match status.state {
                    Some(StatusState::Consumed | StatusState::Removed) => {}
                    Some(StatusState::Applied | StatusState::Refreshed | StatusState::Stacked)
                    | None => {
                        windows.insert(key.clone(), trace);
                    }
                },
                UnresolvedModel::StickyEveryObservation => {
                    windows.insert(key.clone(), trace);
                }
            }
        }
    }
}

fn snapshot_row(
    class: RowClass,
    ledger: &LedgerRow,
    damage: &rlogs_events::DamageEvent,
    state: &LifecycleState,
    terminal_lookback_micros: u64,
) -> Result<RowSnapshot, Box<dyn std::error::Error>> {
    let provider = EntityIdentity {
        actor_id: ledger.provider_actor_id.to_string(),
        entity_uuid: required_string(
            ledger.provider_entity_uuid.as_deref(),
            "provider_entity_uuid",
            ledger.sequence,
        )?,
    };
    let recipient = EntityIdentity {
        actor_id: damage.source.actor_id.0.to_string(),
        entity_uuid: damage.source.entity_uuid.0.to_string(),
    };
    let target = EntityIdentity {
        actor_id: damage.target.actor_id.0.to_string(),
        entity_uuid: damage.target.entity_uuid.0.to_string(),
    };
    let endpoints = [
        (
            EndpointRole::Provider,
            ledger.provider_actor_id,
            parse_i64(&provider.entity_uuid, "provider entity", ledger.sequence)?,
        ),
        (
            EndpointRole::RecipientDamageSource,
            damage.source.actor_id.0,
            damage.source.entity_uuid.0,
        ),
        (
            EndpointRole::RecipientOrEnemyTarget,
            damage.target.actor_id.0,
            damage.target.entity_uuid.0,
        ),
    ];
    let mut active_exact_statuses = Vec::new();
    for (role, actor_id, entity_uuid) in endpoints {
        active_exact_statuses.extend(
            state
                .active_exact
                .values()
                .filter(|trace| {
                    trace.key.target_actor_id == actor_id
                        && trace.key.target_entity_uuid == entity_uuid
                })
                .map(|trace| exact_snapshot(role, trace, ledger.observed_micros)),
        );
    }
    active_exact_statuses.sort_by_key(|status| {
        (
            status.endpoint_role,
            status.effect_id,
            status.instance_id,
            status.last_sequence,
        )
    });

    let terminal_floor = ledger
        .observed_micros
        .saturating_sub(terminal_lookback_micros);
    let mut recent_terminal_exact_statuses = Vec::new();
    for (role, actor_id, entity_uuid) in endpoints {
        recent_terminal_exact_statuses.extend(
            state
                .latest_exact
                .values()
                .filter(|trace| {
                    trace.key.target_actor_id == actor_id
                        && trace.key.target_entity_uuid == entity_uuid
                        && matches!(trace.state, StatusState::Consumed | StatusState::Removed)
                        && trace.last_observed_micros >= terminal_floor
                        && trace.last_observed_micros <= ledger.observed_micros
                })
                .map(|trace| terminal_snapshot(role, trace)),
        );
    }
    recent_terminal_exact_statuses.sort_by_key(|status| std::cmp::Reverse(status.sequence));
    let recent_terminal_exact_statuses_truncated =
        recent_terminal_exact_statuses.len() > MAX_RECENT_TERMINALS_PER_ENDPOINT * endpoints.len();
    recent_terminal_exact_statuses.truncate(MAX_RECENT_TERMINALS_PER_ENDPOINT * endpoints.len());
    recent_terminal_exact_statuses.sort_by_key(|status| status.sequence);

    let mut active_unresolved_by_model = BTreeMap::new();
    let mut blocked_by_unresolved_model = BTreeMap::new();
    for model in UnresolvedModel::ALL {
        let mut snapshots = Vec::new();
        if let Some(windows) = state.unresolved.get(&model) {
            for (role, actor_id, _) in endpoints {
                snapshots.extend(
                    windows
                        .values()
                        .filter(|trace| trace.key.target_actor_id == actor_id)
                        .map(|trace| unresolved_snapshot(role, trace)),
                );
            }
        }
        snapshots.sort_by_key(|status| {
            (
                status.endpoint_role,
                status.instance_id,
                status.last_sequence,
            )
        });
        blocked_by_unresolved_model.insert(model.name(), !snapshots.is_empty());
        active_unresolved_by_model.insert(model.name(), snapshots);
    }
    let signature = signature(&active_exact_statuses, &active_unresolved_by_model);

    Ok(RowSnapshot {
        class,
        sequence: ledger.sequence,
        capture_sequence: ledger.capture_sequence,
        observed_micros: ledger.observed_micros,
        provider,
        recipient_damage_source: recipient,
        recipient_or_enemy_target: target,
        affected_damage_id: ledger.affected_damage_id,
        hit_event_id: damage.hit_event_id,
        observed_damage: ledger.observed_damage.clone(),
        contribution_numerator: ledger.numerator.clone(),
        contribution_denominator: ledger.denominator.clone(),
        active_exact_statuses,
        recent_terminal_exact_statuses,
        recent_terminal_exact_statuses_truncated,
        active_unresolved_by_model,
        blocked_by_unresolved_model,
        signature,
    })
}

fn exact_snapshot(
    endpoint_role: EndpointRole,
    trace: &ExactStatusTrace,
    damage_micros: u64,
) -> ExactStatusSnapshot {
    ExactStatusSnapshot {
        endpoint_role,
        effect_id: trace.key.effect_id,
        instance_id: trace.key.instance_id,
        source_actor_id: trace.key.source_actor_id.map(|value| value.to_string()),
        source_entity_uuid: trace.key.source_entity_uuid.map(|value| value.to_string()),
        target_actor_id: trace.key.target_actor_id.to_string(),
        target_entity_uuid: trace.key.target_entity_uuid.to_string(),
        origin: trace.origin,
        last_state: trace.state,
        stacks: trace.stacks,
        duration_millis: trace.duration_millis,
        level: trace.level,
        part_id: trace.part_id,
        count: trace.count,
        created_at_millis: trace.created_at_millis,
        first_sequence: trace.first_sequence,
        last_sequence: trace.last_sequence,
        first_observed_micros: trace.first_observed_micros,
        last_observed_micros: trace.last_observed_micros,
        nominal_expiry_micros: trace.nominal_expiry_micros,
        nominally_expired_before_damage: trace
            .nominal_expiry_micros
            .is_some_and(|expiry| expiry <= damage_micros),
        last_provenance: trace.last_provenance.clone(),
    }
}

fn terminal_snapshot(
    endpoint_role: EndpointRole,
    trace: &ExactStatusTrace,
) -> RecentTerminalSnapshot {
    RecentTerminalSnapshot {
        endpoint_role,
        effect_id: trace.key.effect_id,
        instance_id: trace.key.instance_id,
        source_actor_id: trace.key.source_actor_id.map(|value| value.to_string()),
        source_entity_uuid: trace.key.source_entity_uuid.map(|value| value.to_string()),
        target_actor_id: trace.key.target_actor_id.to_string(),
        target_entity_uuid: trace.key.target_entity_uuid.to_string(),
        terminal_state: trace.state,
        sequence: trace.last_sequence,
        observed_micros: trace.last_observed_micros,
        provenance: trace.last_provenance.clone(),
    }
}

fn unresolved_snapshot(endpoint_role: EndpointRole, trace: &UnresolvedTrace) -> UnresolvedSnapshot {
    UnresolvedSnapshot {
        endpoint_role,
        source_actor_id: trace.source.map(|value| value.actor_id.0.to_string()),
        source_entity_uuid: trace.source.map(|value| value.entity_uuid.0.to_string()),
        target_actor_id: trace.target.actor_id.0.to_string(),
        target_entity_uuid: trace.target.entity_uuid.0.to_string(),
        instance_id: trace.key.instance_id,
        last_state: trace.state,
        wire_event_type: trace.wire_event_type,
        wire_logic_type: trace.wire_logic_type,
        reason: trace.reason,
        raw_payload_bytes: trace.raw_payload_bytes,
        raw_payload_sha256: trace.raw_payload_sha256.clone(),
        first_sequence: trace.first_sequence,
        last_sequence: trace.last_sequence,
        first_observed_micros: trace.first_observed_micros,
        last_observed_micros: trace.last_observed_micros,
        last_provenance: trace.last_provenance.clone(),
    }
}

fn signature(
    exact: &[ExactStatusSnapshot],
    unresolved: &BTreeMap<&'static str, Vec<UnresolvedSnapshot>>,
) -> StateSignature {
    let effects = |role: EndpointRole, expired: bool| {
        exact
            .iter()
            .filter(|status| {
                status.endpoint_role == role && status.nominally_expired_before_damage == expired
            })
            .map(|status| status.effect_id)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
    };
    let unresolved_count = |model: &'static str, role: EndpointRole| {
        unresolved
            .get(model)
            .into_iter()
            .flatten()
            .filter(|status| status.endpoint_role == role)
            .count()
    };
    StateSignature {
        provider_active_effect_ids: effects(EndpointRole::Provider, false),
        recipient_active_effect_ids: effects(EndpointRole::RecipientDamageSource, false),
        target_active_effect_ids: effects(EndpointRole::RecipientOrEnemyTarget, false),
        provider_nominally_expired_effect_ids: effects(EndpointRole::Provider, true),
        recipient_nominally_expired_effect_ids: effects(EndpointRole::RecipientDamageSource, true),
        target_nominally_expired_effect_ids: effects(EndpointRole::RecipientOrEnemyTarget, true),
        current_unresolved_provider: unresolved_count(
            UnresolvedModel::TerminalAwareExactInstance.name(),
            EndpointRole::Provider,
        ),
        current_unresolved_recipient: unresolved_count(
            UnresolvedModel::TerminalAwareExactInstance.name(),
            EndpointRole::RecipientDamageSource,
        ),
        current_unresolved_target: unresolved_count(
            UnresolvedModel::TerminalAwareExactInstance.name(),
            EndpointRole::RecipientOrEnemyTarget,
        ),
        sticky_any_unresolved_provider: unresolved_count(
            UnresolvedModel::StickyEveryObservation.name(),
            EndpointRole::Provider,
        ),
        sticky_any_unresolved_recipient: unresolved_count(
            UnresolvedModel::StickyEveryObservation.name(),
            EndpointRole::RecipientDamageSource,
        ),
        sticky_any_unresolved_target: unresolved_count(
            UnresolvedModel::StickyEveryObservation.name(),
            EndpointRole::RecipientOrEnemyTarget,
        ),
    }
}

fn compare_models(rows: &[RowSnapshot]) -> Vec<ModelComparison> {
    UnresolvedModel::ALL
        .into_iter()
        .map(|model| {
            let mut result = ModelComparison {
                model: model.name(),
                semantics: match model {
                    UnresolvedModel::TerminalAwareExactInstance => {
                        "active_or_unknown_opens_a_window_and_exact_instance_terminal_closes_it"
                    }
                    UnresolvedModel::StickyNonterminalOrUnknown => {
                        "active_or_unknown_opens_a_window_and_no_terminal_closes_it"
                    }
                    UnresolvedModel::StickyEveryObservation => {
                        "every_observation_including_terminal_opens_a_window_and_no_terminal_closes_it"
                    }
                },
                allowed_rows: 0,
                blocked_rows: 0,
                true_old_included: 0,
                true_old_suppressed: 0,
                false_included: 0,
                false_suppressed: 0,
                exact_match_to_trusted_ledger: false,
                false_included_sequences: Vec::new(),
                false_suppressed_sequences: Vec::new(),
            };
            for row in rows {
                let blocked = row.blocked_by_unresolved_model[model.name()];
                if blocked {
                    result.blocked_rows += 1;
                } else {
                    result.allowed_rows += 1;
                }
                match (row.class, blocked) {
                    (RowClass::OldIncluded, false) => result.true_old_included += 1,
                    (RowClass::OldSuppressed, true) => result.true_old_suppressed += 1,
                    (RowClass::OldSuppressed, false) => {
                        result.false_included += 1;
                        result.false_included_sequences.push(row.sequence);
                    }
                    (RowClass::OldIncluded, true) => {
                        result.false_suppressed += 1;
                        result.false_suppressed_sequences.push(row.sequence);
                    }
                }
            }
            result.exact_match_to_trusted_ledger =
                result.false_included == 0 && result.false_suppressed == 0;
            result
        })
        .collect()
}

fn summarize_signatures(rows: &[RowSnapshot]) -> Vec<SignatureSummary> {
    let mut summaries = BTreeMap::<StateSignature, SignatureAccumulator>::new();
    for row in rows {
        let accumulator = summaries.entry(row.signature.clone()).or_default();
        match row.class {
            RowClass::OldIncluded => accumulator.old_included_rows += 1,
            RowClass::OldSuppressed => accumulator.old_suppressed_rows += 1,
        }
        if accumulator.sample_sequences.len() < MAX_SIGNATURE_SAMPLES {
            accumulator.sample_sequences.push(row.sequence);
        }
    }
    summaries
        .into_iter()
        .map(|(signature, accumulator)| SignatureSummary {
            discriminates_class: match (
                accumulator.old_included_rows > 0,
                accumulator.old_suppressed_rows > 0,
            ) {
                (true, false) => Some(RowClass::OldIncluded),
                (false, true) => Some(RowClass::OldSuppressed),
                _ => None,
            },
            signature,
            old_included_rows: accumulator.old_included_rows,
            old_suppressed_rows: accumulator.old_suppressed_rows,
            sample_sequences: accumulator.sample_sequences,
        })
        .collect()
}

fn damage_identity_mismatch(
    row: &LedgerRow,
    damage: &rlogs_events::DamageEvent,
) -> Vec<&'static str> {
    let mut fields = Vec::new();
    if row.damage_source_actor_id.as_deref() != Some(&damage.source.actor_id.0.to_string()) {
        fields.push("damage_source_actor_id");
    }
    if row.damage_source_entity_uuid.as_deref() != Some(&damage.source.entity_uuid.0.to_string()) {
        fields.push("damage_source_entity_uuid");
    }
    if row.target_actor_id.as_deref() != Some(&damage.target.actor_id.0.to_string()) {
        fields.push("target_actor_id");
    }
    if row.target_entity_uuid.as_deref() != Some(&damage.target.entity_uuid.0.to_string()) {
        fields.push("target_entity_uuid");
    }
    if row.affected_damage_id != damage.ability.map(|ability| ability.0) {
        fields.push("affected_damage_id");
    }
    if row.observed_damage != damage.amount.to_string() {
        fields.push("observed_damage");
    }
    fields
}

fn nominal_expiry(observed_micros: u64, duration_millis: Option<u64>) -> Option<u64> {
    duration_millis
        .filter(|duration| *duration > 0)
        .map(|duration| observed_micros.saturating_add(duration.saturating_mul(1_000)))
}

fn artifact(path: &Path) -> Result<InputArtifact, Box<dyn std::error::Error>> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut hasher = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        bytes = bytes.saturating_add(count as u64);
        hasher.update(&buffer[..count]);
    }
    Ok(InputArtifact {
        path: path.display().to_string(),
        bytes,
        sha256: format!("SHA256:{:X}", hasher.finalize()),
    })
}

fn hex_digest(bytes: &[u8]) -> String {
    format!("SHA256:{:X}", Sha256::digest(bytes))
}

fn required_string(value: Option<&str>, field: &str, sequence: u64) -> Result<String, String> {
    value
        .map(str::to_owned)
        .ok_or_else(|| format!("ledger sequence {sequence} is missing {field}"))
}

fn parse_i64(value: &str, field: &str, sequence: u64) -> Result<i64, String> {
    value
        .parse()
        .map_err(|_| format!("ledger sequence {sequence} has invalid {field} {value:?}"))
}

fn arguments() -> Result<Arguments, String> {
    let mut values = env::args_os().skip(1).collect::<Vec<_>>();
    let rlog = take_path(&mut values, "--rlog")?;
    let trusted_ledger = take_path(&mut values, "--trusted-ledger")?;
    let candidate_ledger = take_path(&mut values, "--candidate-ledger")?;
    let output = take_path(&mut values, "--output")?;
    let summary_only = take_switch(&mut values, "--summary-only");
    let terminal_lookback_micros = take_optional_string(&mut values, "--terminal-lookback-micros")?
        .map(|value| {
            value.parse::<u64>().map_err(|_| {
                format!("--terminal-lookback-micros must be an unsigned integer, got {value:?}")
            })
        })
        .transpose()?
        .unwrap_or(DEFAULT_TERMINAL_LOOKBACK_MICROS);
    if !values.is_empty() {
        return Err(usage());
    }
    Ok(Arguments {
        rlog,
        trusted_ledger,
        candidate_ledger,
        output,
        terminal_lookback_micros,
        summary_only,
    })
}

fn take_path(values: &mut Vec<OsString>, name: &str) -> Result<PathBuf, String> {
    take_optional_os(values, name)?
        .map(PathBuf::from)
        .ok_or_else(usage)
}

fn take_optional_string(values: &mut Vec<OsString>, name: &str) -> Result<Option<String>, String> {
    take_optional_os(values, name)?
        .map(|value| {
            value
                .into_string()
                .map_err(|_| format!("{name} must be valid Unicode"))
        })
        .transpose()
}

fn take_optional_os(values: &mut Vec<OsString>, name: &str) -> Result<Option<OsString>, String> {
    let Some(position) = values.iter().position(|value| value == name) else {
        return Ok(None);
    };
    if position + 1 >= values.len() {
        return Err(format!("{name} requires a value"));
    }
    values.remove(position);
    Ok(Some(values.remove(position)))
}

fn take_switch(values: &mut Vec<OsString>, name: &str) -> bool {
    if let Some(position) = values.iter().position(|value| value == name) {
        values.remove(position);
        true
    } else {
        false
    }
}

fn usage() -> String {
    "usage: rlogs-bpsr-harmony-overlap-ledger-diff --rlog <sealed.rlog> --trusted-ledger <39-row-audit.json> --candidate-ledger <223-row-audit.json> --output <receipt.json> [--terminal-lookback-micros <micros>] [--summary-only]".into()
}

#[cfg(test)]
mod tests {
    use super::{LifecycleState, UnresolvedModel, compare_models, nominal_expiry};
    use rlogs_events::{
        ActorId, EntityRef, EntityUuid, EventProvenance, StatusEffectInstanceId, StatusState,
        UnresolvedStatusEvent, UnresolvedStatusReason,
    };

    #[test]
    fn positive_duration_has_saturating_nominal_expiry() {
        assert_eq!(nominal_expiry(10, Some(25)), Some(25_010));
        assert_eq!(nominal_expiry(10, Some(0)), None);
        assert_eq!(nominal_expiry(10, None), None);
    }

    #[test]
    fn lifecycle_state_initializes_each_explicit_model() {
        let mut state = LifecycleState::default();
        for model in UnresolvedModel::ALL {
            state.unresolved.entry(model).or_default();
        }
        assert_eq!(state.unresolved.len(), UnresolvedModel::ALL.len());
    }

    #[test]
    fn empty_model_comparison_is_exact_for_every_model() {
        let comparisons = compare_models(&[]);
        assert_eq!(comparisons.len(), UnresolvedModel::ALL.len());
        assert!(
            comparisons
                .iter()
                .all(|comparison| comparison.exact_match_to_trusted_ledger)
        );
    }

    #[test]
    fn terminal_only_unresolved_row_is_not_active_under_terminal_aware_model() {
        let mut state = initialized_state();
        state.observe_unresolved(
            10,
            20,
            &EventProvenance::wire(30, 40, 50),
            &unresolved(Some(StatusState::Removed)),
        );
        assert!(state.unresolved[&UnresolvedModel::TerminalAwareExactInstance].is_empty());
        assert!(state.unresolved[&UnresolvedModel::StickyNonterminalOrUnknown].is_empty());
        assert_eq!(
            state.unresolved[&UnresolvedModel::StickyEveryObservation].len(),
            1
        );
    }

    #[test]
    fn exact_terminal_closes_current_window_but_not_sticky_models() {
        let mut state = initialized_state();
        state.observe_unresolved(
            10,
            20,
            &EventProvenance::wire(30, 40, 50),
            &unresolved(Some(StatusState::Applied)),
        );
        state.observe_unresolved(
            11,
            21,
            &EventProvenance::wire(31, 40, 50),
            &unresolved(Some(StatusState::Removed)),
        );
        assert!(state.unresolved[&UnresolvedModel::TerminalAwareExactInstance].is_empty());
        assert_eq!(
            state.unresolved[&UnresolvedModel::StickyNonterminalOrUnknown].len(),
            1
        );
        assert_eq!(
            state.unresolved[&UnresolvedModel::StickyEveryObservation].len(),
            1
        );
    }

    fn initialized_state() -> LifecycleState {
        let mut state = LifecycleState::default();
        for model in UnresolvedModel::ALL {
            state.unresolved.entry(model).or_default();
        }
        state
    }

    fn unresolved(state: Option<StatusState>) -> UnresolvedStatusEvent {
        UnresolvedStatusEvent {
            source: None,
            target: EntityRef {
                actor_id: ActorId(7),
                entity_uuid: EntityUuid(70),
            },
            instance_id: Some(StatusEffectInstanceId(700)),
            state,
            wire_event_type: Some(3),
            wire_logic_type: Some(4),
            reason: UnresolvedStatusReason::MissingEffectId,
            raw_payload: vec![1, 2, 3],
        }
    }
}
