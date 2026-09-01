#![allow(clippy::collapsible_if, clippy::large_enum_variant)]

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    env,
    ffi::OsString,
    fs::File,
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

use rlogs_combat::{ActorAncestryResolver, ActorOwnershipEvidence};
use rlogs_events::{
    ActorEvent, ActorKind, ActorLoadoutEvidence, ActorLoadoutSlot, ActorOwnershipUpdate,
    ActorState, CanonicalEvent, EncounterState, EntityRef, EvidenceSource, RunState, StatusEvent,
    StatusState, TimelineEventKind,
};
use rlogs_game_bpsr::character_id_from_entity_uuid;
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::Serialize;
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 5;
const DEFAULT_EXAMPLE_LIMIT: usize = 12;
const MAX_ANCESTRY_DEPTH: usize = 16;

#[derive(Debug)]
struct Arguments {
    rlogs: Vec<PathBuf>,
    effects: BTreeSet<i64>,
    output: PathBuf,
    example_limit: usize,
}

#[derive(Debug, Clone, Serialize)]
struct InputEvidence {
    path: String,
    bytes: u64,
    sha256: String,
    session_id: String,
    game_build: String,
}

#[derive(Debug, Serialize)]
struct Report {
    schema_version: u16,
    tool: &'static str,
    game_build: String,
    policy: ProofPolicy,
    selection: Selection,
    inputs: Vec<InputEvidence>,
    summary: Summary,
    effects: Vec<EffectReport>,
    resolutions: Vec<ResolutionReport>,
}

#[derive(Debug, Serialize)]
struct ProofPolicy {
    scope: &'static str,
    exact_numeric_effect_ids_authoritative: bool,
    exact_input_build_authoritative: bool,
    localized_names_are_evidence_only: bool,
    actor_kind_or_packet_proven_ancestry_required_for_player_ownership: bool,
    bpsr_player_entity_uuid_character_id_contract_applied: bool,
    explicit_and_derived_character_id_mismatches_are_rejected: bool,
    prior_exact_status_instance_player_ownership_may_flow_forward: bool,
    forward_status_instance_ownership_requires_exact_run_target_effect_instance_and_source: bool,
    conflicting_status_instance_owners_disable_inheritance: bool,
    later_attributed_combat_relation_in_same_exact_wire_packet_may_resolve_provider: bool,
    same_wire_packet_resolution_requires_exact_capture_connection_stream_and_observed_time: bool,
    player_loadout_is_reported_only_from_the_resolved_player_actor_snapshot: bool,
    loadout_tier_is_evidence_only_not_formula_authority: bool,
    future_actor_snapshots_may_backfill_prior_status_events: bool,
    unknown_and_unresolved_events_preserved: bool,
    formula_authority: bool,
    runtime_authority: bool,
    provider_rdps_credit_allowed: bool,
}

#[derive(Debug, Serialize)]
struct Selection {
    effect_ids: Vec<i64>,
    example_limit_per_resolution: usize,
}

#[derive(Debug, Default, Serialize)]
struct Summary {
    canonical_events_scanned: u64,
    actor_events_scanned: u64,
    ownership_updates_scanned: u64,
    attributed_combat_relations_scanned: u64,
    status_events_scanned: u64,
    selected_status_events: u64,
    selected_events_with_missing_source: u64,
    selected_events_with_direct_player_provider: u64,
    selected_events_with_player_owner: u64,
    selected_events_with_non_player_owner: u64,
    selected_events_with_unobserved_owner_identity: u64,
    selected_events_with_non_player_unowned_source: u64,
    selected_events_with_unobserved_source_identity: u64,
    selected_events_with_same_wire_packet_player_owner: u64,
    selected_events_with_prior_status_instance_player_owner: u64,
    selected_events_with_stable_player_character_id: u64,
    selected_events_with_player_primary_loadout_evidence: u64,
    unique_selected_source_entities: usize,
    unique_proven_player_character_ids: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum ResolutionClass {
    MissingSource,
    DirectPlayer,
    OwnedByPlayer,
    SameWirePacketOwnedByPlayer,
    PriorStatusInstancePlayer,
    OwnedByNonPlayer,
    OwnerIdentityUnobserved,
    NonPlayerUnowned,
    SourceIdentityUnobserved,
}

#[derive(Debug, Default)]
struct EffectAccumulator {
    status_events: u64,
    classes: BTreeMap<ResolutionClass, u64>,
    source_entities: BTreeSet<i64>,
    player_character_ids: BTreeSet<String>,
    player_primary_loadouts: BTreeSet<PlayerPrimaryLoadoutEvidence>,
    events_with_stable_player_character_id: u64,
    events_with_player_primary_loadout_evidence: u64,
}

#[derive(Debug, Serialize)]
struct EffectReport {
    effect_id: i64,
    status_events: u64,
    resolution_counts: BTreeMap<ResolutionClass, u64>,
    unique_source_entities: usize,
    proven_player_character_ids: Vec<String>,
    proven_player_primary_loadouts: Vec<PlayerPrimaryLoadoutEvidence>,
    player_actor_ownership_proven_for_every_sourced_event: bool,
    status_events_with_stable_player_character_id: u64,
    stable_player_character_id_proven_for_every_sourced_event: bool,
    status_events_with_player_primary_loadout_evidence: u64,
    player_primary_loadout_proven_for_every_player_owned_event: bool,
    formula_authority: bool,
    runtime_authority: bool,
}

#[derive(Debug, Clone)]
struct ActorSnapshot {
    actor: EntityRef,
    kind: ActorKind,
    entity_type_id: i32,
    character_id: Option<String>,
    character_id_source: Option<String>,
    display_name: Option<String>,
    class_id: Option<i32>,
    specialization_id: Option<i32>,
    primary_loadout: Vec<LoadoutSlotEvidence>,
    primary_loadout_evidence: ActorLoadoutEvidence,
    observed_sequence: u64,
    observed_micros: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ActorIdentityKey {
    actor_id: u64,
    entity_uuid: i64,
    kind: Option<String>,
    entity_type_id: Option<i32>,
    character_id: Option<String>,
    character_id_source: Option<String>,
    class_id: Option<i32>,
    specialization_id: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct OwnershipLink {
    child_actor_id: u64,
    child_entity_uuid: i64,
    owner_actor_id: u64,
    owner_entity_uuid: i64,
    attributed_combat_source: bool,
    confirmed_entity_attributes: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct LoadoutSlotEvidence {
    slot_id: i32,
    ability_id: Option<i64>,
    item_id: Option<i64>,
    tier: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct PlayerPrimaryLoadoutEvidence {
    character_id: String,
    actor_id: u64,
    entity_uuid: i64,
    evidence: ActorLoadoutEvidence,
    slots: Vec<LoadoutSlotEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ResolutionKey {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    effect_id: i64,
    origin_source_type_id: Option<i32>,
    origin_source_config_id: Option<i64>,
    class: ResolutionClass,
    source: Option<ActorIdentityKey>,
    resolved_owner: Option<ActorIdentityKey>,
    ownership_chain: Vec<OwnershipLink>,
    same_wire_packet_ownership_sequence: Option<u64>,
    prior_status_instance_ownership_sequence: Option<u64>,
    player_primary_loadout_evidence: Option<PlayerPrimaryLoadoutEvidence>,
}

#[derive(Debug, Default)]
struct ResolutionAccumulator {
    status_events: u64,
    state_counts: BTreeMap<&'static str, u64>,
    target_entity_uuids: BTreeSet<i64>,
    source_display_names: BTreeSet<String>,
    owner_display_names: BTreeSet<String>,
    first_sequence: Option<u64>,
    last_sequence: Option<u64>,
    first_observed_micros: Option<u64>,
    last_observed_micros: Option<u64>,
    examples: Vec<StatusExample>,
}

#[derive(Debug, Serialize)]
struct ResolutionReport {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    effect_id: i64,
    origin_source_type_id: Option<i32>,
    origin_source_config_id: Option<i64>,
    class: ResolutionClass,
    source: Option<ActorIdentityEvidence>,
    resolved_owner: Option<ActorIdentityEvidence>,
    ownership_chain: Vec<OwnershipLink>,
    same_wire_packet_ownership_sequence: Option<u64>,
    prior_status_instance_ownership_sequence: Option<u64>,
    player_primary_loadout_evidence: Option<PlayerPrimaryLoadoutEvidence>,
    source_display_name_evidence: Vec<String>,
    owner_display_name_evidence: Vec<String>,
    status_events: u64,
    status_state_counts: BTreeMap<&'static str, u64>,
    unique_target_entities: usize,
    first_sequence: u64,
    last_sequence: u64,
    first_observed_micros: u64,
    last_observed_micros: u64,
    examples: Vec<StatusExample>,
    ownership_only: bool,
    formula_authority: bool,
    runtime_authority: bool,
}

#[derive(Debug, Serialize)]
struct ActorIdentityEvidence {
    actor_id: u64,
    entity_uuid: i64,
    kind: Option<String>,
    entity_type_id: Option<i32>,
    character_id: Option<String>,
    character_id_source: Option<String>,
    class_id: Option<i32>,
    specialization_id: Option<i32>,
}

#[derive(Debug, Clone, Serialize)]
struct StatusExample {
    sequence: u64,
    observed_micros: u64,
    state: &'static str,
    source_actor_id: Option<u64>,
    source_entity_uuid: Option<i64>,
    target_actor_id: u64,
    target_entity_uuid: i64,
    instance_id: Option<i64>,
    stacks: Option<u32>,
    level: Option<i32>,
    duration_millis: Option<u64>,
    capture_sequence: Option<u64>,
    source_actor_snapshot_sequence: Option<u64>,
    source_actor_snapshot_observed_micros: Option<u64>,
    owner_actor_snapshot_sequence: Option<u64>,
    owner_actor_snapshot_observed_micros: Option<u64>,
}

#[derive(Debug, Clone)]
struct ProviderResolution {
    class: ResolutionClass,
    source: Option<ActorIdentityKey>,
    resolved_owner: Option<ActorIdentityKey>,
    ownership_chain: Vec<OwnershipLink>,
    source_display_name: Option<String>,
    owner_display_name: Option<String>,
    player_character_id: Option<String>,
    player_primary_loadout_evidence: Option<PlayerPrimaryLoadoutEvidence>,
    same_wire_packet_ownership_sequence: Option<u64>,
    prior_status_instance_ownership_sequence: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WirePacketKey {
    capture_sequence: u64,
    connection_id: u64,
    stream_id: u64,
}

#[derive(Debug, Clone, Copy)]
struct PacketAttributedRelation {
    sequence: u64,
    observed_micros: u64,
    child: EntityRef,
    owner: EntityRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct StatusInstanceKey {
    target_actor_id: u64,
    target_entity_uuid: i64,
    effect_id: i64,
    instance_id: i64,
}

#[derive(Debug, Clone)]
enum StatusInstanceOwnership {
    Proven {
        resolution: ProviderResolution,
        established_sequence: u64,
    },
    Conflicted,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("BPSR status-effect provider ownership proof failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_arguments(env::args_os().skip(1))?;
    let inputs = inspect_inputs(&args.rlogs)?;
    let game_build = inputs
        .first()
        .map(|input| input.game_build.clone())
        .ok_or("at least one rlog input is required")?;
    let mut summary = Summary::default();
    let mut effects = args
        .effects
        .iter()
        .map(|effect| (*effect, EffectAccumulator::default()))
        .collect::<BTreeMap<_, _>>();
    let mut resolutions = BTreeMap::<ResolutionKey, ResolutionAccumulator>::new();

    for (path, input) in args.rlogs.iter().zip(&inputs) {
        scan_rlog(
            path,
            input,
            &args,
            &mut summary,
            &mut effects,
            &mut resolutions,
        )?;
    }

    let unique_sources = effects
        .values()
        .flat_map(|effect| effect.source_entities.iter().copied())
        .collect::<BTreeSet<_>>();
    let unique_players = effects
        .values()
        .flat_map(|effect| effect.player_character_ids.iter().cloned())
        .collect::<BTreeSet<_>>();
    summary.unique_selected_source_entities = unique_sources.len();
    summary.unique_proven_player_character_ids = unique_players.len();

    let report = Report {
        schema_version: SCHEMA_VERSION,
        tool: "rlogs-bpsr-status-effect-provider-ownership-proof",
        game_build,
        policy: ProofPolicy {
            scope: "provider_ownership_only",
            exact_numeric_effect_ids_authoritative: true,
            exact_input_build_authoritative: true,
            localized_names_are_evidence_only: true,
            actor_kind_or_packet_proven_ancestry_required_for_player_ownership: true,
            bpsr_player_entity_uuid_character_id_contract_applied: true,
            explicit_and_derived_character_id_mismatches_are_rejected: true,
            prior_exact_status_instance_player_ownership_may_flow_forward: true,
            forward_status_instance_ownership_requires_exact_run_target_effect_instance_and_source:
                true,
            conflicting_status_instance_owners_disable_inheritance: true,
            later_attributed_combat_relation_in_same_exact_wire_packet_may_resolve_provider: true,
            same_wire_packet_resolution_requires_exact_capture_connection_stream_and_observed_time:
                true,
            player_loadout_is_reported_only_from_the_resolved_player_actor_snapshot: true,
            loadout_tier_is_evidence_only_not_formula_authority: true,
            future_actor_snapshots_may_backfill_prior_status_events: false,
            unknown_and_unresolved_events_preserved: true,
            formula_authority: false,
            runtime_authority: false,
            provider_rdps_credit_allowed: false,
        },
        selection: Selection {
            effect_ids: args.effects.iter().copied().collect(),
            example_limit_per_resolution: args.example_limit,
        },
        inputs,
        summary,
        effects: effects
            .into_iter()
            .map(|(effect_id, effect)| effect.into_report(effect_id))
            .collect(),
        resolutions: resolutions
            .into_iter()
            .map(|(key, accumulator)| accumulator.into_report(key))
            .collect(),
    };

    let mut writer = BufWriter::new(File::create(&args.output)?);
    serde_json::to_writer_pretty(&mut writer, &report)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn inspect_inputs(paths: &[PathBuf]) -> Result<Vec<InputEvidence>, Box<dyn std::error::Error>> {
    let mut expected: Option<(String, &Path)> = None;
    let mut inputs = Vec::with_capacity(paths.len());
    for path in paths {
        let reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
        let build = reader.header().region.client_build.trim();
        if build.is_empty() {
            return Err(format!(
                "{} has an empty client build in its rlog header",
                path.display()
            )
            .into());
        }
        if let Some((expected_build, expected_path)) = &expected {
            if build != expected_build {
                return Err(format!(
                    "input build mismatch: {} declares {}, while {} declares {}",
                    expected_path.display(),
                    expected_build,
                    path.display(),
                    build
                )
                .into());
            }
        } else {
            expected = Some((build.to_owned(), path));
        }
        let metadata = path.metadata()?;
        inputs.push(InputEvidence {
            path: display_path(path),
            bytes: metadata.len(),
            sha256: sha256_file(path)?,
            session_id: reader.header().session_id.clone(),
            game_build: build.to_owned(),
        });
    }
    Ok(inputs)
}

fn scan_rlog(
    path: &Path,
    input: &InputEvidence,
    args: &Arguments,
    summary: &mut Summary,
    effects: &mut BTreeMap<i64, EffectAccumulator>,
    resolutions: &mut BTreeMap<ResolutionKey, ResolutionAccumulator>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
    let mut run_ordinal = 0_u32;
    let mut actors = HashMap::<u64, ActorSnapshot>::new();
    let mut ancestry = ActorAncestryResolver::default();
    let mut status_instance_ownership =
        HashMap::<StatusInstanceKey, StatusInstanceOwnership>::new();
    let mut wire_packet_key = None;
    let mut wire_packet_events = Vec::new();

    while let Some(envelope) = reader.next_event()? {
        summary.canonical_events_scanned = summary.canonical_events_scanned.saturating_add(1);
        if is_ownership_boundary(&envelope) {
            process_wire_packet(
                path,
                input,
                args,
                &mut run_ordinal,
                &mut actors,
                &mut ancestry,
                &mut status_instance_ownership,
                summary,
                effects,
                resolutions,
                &mut wire_packet_events,
            )?;
            wire_packet_key = None;
            process_timeline_event(
                path,
                input,
                args,
                &mut run_ordinal,
                &mut actors,
                &mut ancestry,
                &mut status_instance_ownership,
                summary,
                effects,
                resolutions,
                &envelope,
                &[],
            )?;
            continue;
        }

        let Some(key) = exact_wire_packet_key(&envelope.provenance.source) else {
            process_wire_packet(
                path,
                input,
                args,
                &mut run_ordinal,
                &mut actors,
                &mut ancestry,
                &mut status_instance_ownership,
                summary,
                effects,
                resolutions,
                &mut wire_packet_events,
            )?;
            wire_packet_key = None;
            process_timeline_event(
                path,
                input,
                args,
                &mut run_ordinal,
                &mut actors,
                &mut ancestry,
                &mut status_instance_ownership,
                summary,
                effects,
                resolutions,
                &envelope,
                &[],
            )?;
            continue;
        };

        if wire_packet_key.is_some_and(|current| current != key) {
            process_wire_packet(
                path,
                input,
                args,
                &mut run_ordinal,
                &mut actors,
                &mut ancestry,
                &mut status_instance_ownership,
                summary,
                effects,
                resolutions,
                &mut wire_packet_events,
            )?;
        }
        wire_packet_key = Some(key);
        wire_packet_events.push(envelope);
    }
    process_wire_packet(
        path,
        input,
        args,
        &mut run_ordinal,
        &mut actors,
        &mut ancestry,
        &mut status_instance_ownership,
        summary,
        effects,
        resolutions,
        &mut wire_packet_events,
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn process_wire_packet(
    path: &Path,
    input: &InputEvidence,
    args: &Arguments,
    run_ordinal: &mut u32,
    actors: &mut HashMap<u64, ActorSnapshot>,
    ancestry: &mut ActorAncestryResolver,
    status_instance_ownership: &mut HashMap<StatusInstanceKey, StatusInstanceOwnership>,
    summary: &mut Summary,
    effects: &mut BTreeMap<i64, EffectAccumulator>,
    resolutions: &mut BTreeMap<ResolutionKey, ResolutionAccumulator>,
    events: &mut Vec<rlogs_events::EventEnvelope>,
) -> Result<(), Box<dyn std::error::Error>> {
    if events.is_empty() {
        return Ok(());
    }
    let attributed_relations = packet_attributed_relations(events);
    for envelope in events.drain(..) {
        process_timeline_event(
            path,
            input,
            args,
            run_ordinal,
            actors,
            ancestry,
            status_instance_ownership,
            summary,
            effects,
            resolutions,
            &envelope,
            &attributed_relations,
        )?;
    }
    Ok(())
}

fn exact_wire_packet_key(source: &EvidenceSource) -> Option<WirePacketKey> {
    match source {
        EvidenceSource::Wire {
            capture_sequence,
            connection_id,
            stream_id,
        } => Some(WirePacketKey {
            capture_sequence: *capture_sequence,
            connection_id: *connection_id,
            stream_id: *stream_id,
        }),
        EvidenceSource::Derived { .. } | EvidenceSource::Manual { .. } => None,
    }
}

fn is_ownership_boundary(envelope: &rlogs_events::EventEnvelope) -> bool {
    matches!(
        &envelope.event,
        CanonicalEvent::Timeline(timeline)
            if matches!(
                timeline.kind,
                TimelineEventKind::RunBoundary {
                    state: RunState::Entered,
                    ..
                } | TimelineEventKind::EncounterBoundary {
                    state: EncounterState::Wiped,
                    ..
                }
            )
    )
}

fn packet_attributed_relations(
    events: &[rlogs_events::EventEnvelope],
) -> Vec<PacketAttributedRelation> {
    events
        .iter()
        .filter_map(|envelope| {
            let CanonicalEvent::Timeline(timeline) = &envelope.event else {
                return None;
            };
            let (owner, direct) = match &timeline.kind {
                TimelineEventKind::Damage(damage) => (damage.source, damage.direct_source),
                TimelineEventKind::Healing(healing) => (healing.source, healing.direct_source),
                _ => return None,
            };
            let child = direct.filter(|direct| direct.actor_id != owner.actor_id)?;
            Some(PacketAttributedRelation {
                sequence: envelope.sequence,
                observed_micros: envelope.time.observed_micros,
                child,
                owner,
            })
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn process_timeline_event(
    path: &Path,
    input: &InputEvidence,
    args: &Arguments,
    run_ordinal: &mut u32,
    actors: &mut HashMap<u64, ActorSnapshot>,
    ancestry: &mut ActorAncestryResolver,
    status_instance_ownership: &mut HashMap<StatusInstanceKey, StatusInstanceOwnership>,
    summary: &mut Summary,
    effects: &mut BTreeMap<i64, EffectAccumulator>,
    resolutions: &mut BTreeMap<ResolutionKey, ResolutionAccumulator>,
    envelope: &rlogs_events::EventEnvelope,
    same_packet_relations: &[PacketAttributedRelation],
) -> Result<(), Box<dyn std::error::Error>> {
    let CanonicalEvent::Timeline(timeline) = &envelope.event else {
        return Ok(());
    };
    let observed_micros = envelope.time.observed_micros;
    match &timeline.kind {
        TimelineEventKind::RunBoundary { state, .. } => {
            match state {
                RunState::Entered => *run_ordinal = run_ordinal.saturating_add(1),
                RunState::Started if *run_ordinal == 0 => *run_ordinal = 1,
                _ => {}
            }
            if *state == RunState::Entered {
                reset_ancestry(ancestry, actors);
                status_instance_ownership.clear();
            }
        }
        TimelineEventKind::EncounterBoundary {
            state: EncounterState::Wiped,
            ..
        } => {
            reset_ancestry(ancestry, actors);
            status_instance_ownership.clear();
        }
        TimelineEventKind::Actor(actor) => {
            summary.actor_events_scanned = summary.actor_events_scanned.saturating_add(1);
            observe_actor(actors, ancestry, actor, envelope.sequence, observed_micros).map_err(
                |error| {
                    format!(
                        "{} sequence {} has invalid player character identity: {error}",
                        path.display(),
                        envelope.sequence
                    )
                },
            )?;
        }
        TimelineEventKind::EntityAttributes(attributes) => {
            ancestry.observe_entity(attributes.actor);
            match attributes.ownership.as_ref() {
                Some(ActorOwnershipUpdate::Confirmed { owner_entity_uuid }) => {
                    summary.ownership_updates_scanned =
                        summary.ownership_updates_scanned.saturating_add(1);
                    ancestry.observe_owner_entity(
                        observed_micros,
                        attributes.actor,
                        owner_entity_uuid.0,
                        ActorOwnershipEvidence::ConfirmedEntityAttributes,
                    );
                }
                Some(ActorOwnershipUpdate::Cleared) => {
                    summary.ownership_updates_scanned =
                        summary.ownership_updates_scanned.saturating_add(1);
                    ancestry.clear_owner(observed_micros, attributes.actor);
                }
                None => {}
            }
        }
        TimelineEventKind::Damage(damage) => {
            if damage
                .direct_source
                .is_some_and(|direct| direct.actor_id != damage.source.actor_id)
            {
                summary.attributed_combat_relations_scanned = summary
                    .attributed_combat_relations_scanned
                    .saturating_add(1);
            }
            ancestry.observe_damage(observed_micros, damage);
        }
        TimelineEventKind::Healing(healing) => {
            ancestry.observe_entity(healing.target);
            if healing
                .direct_source
                .is_some_and(|direct| direct.actor_id != healing.source.actor_id)
            {
                summary.attributed_combat_relations_scanned = summary
                    .attributed_combat_relations_scanned
                    .saturating_add(1);
            }
            ancestry.observe_attributed_source(
                observed_micros,
                healing.source,
                healing.direct_source,
            );
        }
        TimelineEventKind::Shield(shield) => {
            ancestry.observe_entity(shield.source);
            ancestry.observe_entity(shield.target);
        }
        TimelineEventKind::Cast(cast) => {
            ancestry.observe_entity(cast.source);
            if let Some(target) = cast.target {
                ancestry.observe_entity(target);
            }
        }
        TimelineEventKind::Cooldown(cooldown) => ancestry.observe_entity(cooldown.actor),
        TimelineEventKind::Resource(resource) => ancestry.observe_entity(resource.actor),
        TimelineEventKind::TemporaryAttributes(attributes) => {
            ancestry.observe_entity(attributes.actor)
        }
        TimelineEventKind::Life { actor, .. } => ancestry.observe_entity(*actor),
        TimelineEventKind::Position(position) => ancestry.observe_entity(position.actor),
        TimelineEventKind::UnresolvedAction(action) => {
            if let Some(container) = action.container {
                ancestry.observe_entity(container);
            }
            if let Some(target) = action.target {
                ancestry.observe_entity(target);
            }
        }
        TimelineEventKind::Status(status) => {
            summary.status_events_scanned = summary.status_events_scanned.saturating_add(1);
            ancestry.observe_entity(status.target);
            if let Some(source) = status.source {
                ancestry.observe_entity(source);
            }
            if !args.effects.contains(&status.effect.0) {
                return Ok(());
            }
            observe_selected_status(
                path,
                input,
                *run_ordinal,
                envelope,
                status,
                actors,
                ancestry,
                same_packet_relations,
                status_instance_ownership,
                args.example_limit,
                summary,
                effects,
                resolutions,
            );
        }
        TimelineEventKind::EncounterBoundary { .. }
        | TimelineEventKind::CombatBoundary { .. }
        | TimelineEventKind::RecorderPause(_)
        | TimelineEventKind::UnresolvedStatus(_)
        | TimelineEventKind::DataGap(_) => {}
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn observe_selected_status(
    path: &Path,
    input: &InputEvidence,
    run_ordinal: u32,
    envelope: &rlogs_events::EventEnvelope,
    status: &StatusEvent,
    actors: &HashMap<u64, ActorSnapshot>,
    ancestry: &ActorAncestryResolver,
    same_packet_relations: &[PacketAttributedRelation],
    status_instance_ownership: &mut HashMap<StatusInstanceKey, StatusInstanceOwnership>,
    example_limit: usize,
    summary: &mut Summary,
    effects: &mut BTreeMap<i64, EffectAccumulator>,
    resolutions: &mut BTreeMap<ResolutionKey, ResolutionAccumulator>,
) {
    summary.selected_status_events = summary.selected_status_events.saturating_add(1);
    let direct_resolution = classify_provider_with_same_packet_relations(
        status.source,
        envelope.sequence,
        envelope.time.observed_micros,
        actors,
        ancestry,
        same_packet_relations,
    );
    let resolution = resolve_status_instance_provider(
        status,
        envelope.sequence,
        direct_resolution,
        status_instance_ownership,
    );
    increment_summary_class(summary, resolution.class);

    let effect = effects.get_mut(&status.effect.0).expect("selected effect");
    effect.status_events = effect.status_events.saturating_add(1);
    let class_count = effect.classes.entry(resolution.class).or_default();
    *class_count = class_count.saturating_add(1);
    if let Some(source) = status.source {
        effect.source_entities.insert(source.entity_uuid.0);
    }
    if let Some(character_id) = &resolution.player_character_id {
        effect.player_character_ids.insert(character_id.clone());
        effect.events_with_stable_player_character_id = effect
            .events_with_stable_player_character_id
            .saturating_add(1);
        summary.selected_events_with_stable_player_character_id = summary
            .selected_events_with_stable_player_character_id
            .saturating_add(1);
    }
    if let Some(loadout) = &resolution.player_primary_loadout_evidence {
        effect.player_primary_loadouts.insert(loadout.clone());
        effect.events_with_player_primary_loadout_evidence = effect
            .events_with_player_primary_loadout_evidence
            .saturating_add(1);
        summary.selected_events_with_player_primary_loadout_evidence = summary
            .selected_events_with_player_primary_loadout_evidence
            .saturating_add(1);
    }

    let source_actor_snapshot = status
        .source
        .and_then(|source| current_actor(actors, source))
        .map(|snapshot| (snapshot.observed_sequence, snapshot.observed_micros));
    let owner_actor_snapshot = resolution.resolved_owner.as_ref().and_then(|owner| {
        actors
            .get(&owner.actor_id)
            .filter(|snapshot| snapshot.actor.entity_uuid.0 == owner.entity_uuid)
            .map(|snapshot| (snapshot.observed_sequence, snapshot.observed_micros))
    });

    let key = ResolutionKey {
        rlog: file_label(path),
        session_id: input.session_id.clone(),
        run_ordinal,
        effect_id: status.effect.0,
        origin_source_type_id: status.origin.map(|origin| origin.source_type_id),
        origin_source_config_id: status.origin.map(|origin| origin.source_config_id),
        class: resolution.class,
        source: resolution.source,
        resolved_owner: resolution.resolved_owner,
        ownership_chain: resolution.ownership_chain,
        same_wire_packet_ownership_sequence: resolution.same_wire_packet_ownership_sequence,
        prior_status_instance_ownership_sequence: resolution
            .prior_status_instance_ownership_sequence,
        player_primary_loadout_evidence: resolution.player_primary_loadout_evidence,
    };
    let accumulator = resolutions.entry(key).or_default();
    accumulator.status_events = accumulator.status_events.saturating_add(1);
    *accumulator
        .state_counts
        .entry(status_state_label(status.state))
        .or_default() += 1;
    accumulator
        .target_entity_uuids
        .insert(status.target.entity_uuid.0);
    if let Some(name) = resolution.source_display_name {
        accumulator.source_display_names.insert(name);
    }
    if let Some(name) = resolution.owner_display_name {
        accumulator.owner_display_names.insert(name);
    }
    accumulator.first_sequence.get_or_insert(envelope.sequence);
    accumulator.last_sequence = Some(envelope.sequence);
    accumulator
        .first_observed_micros
        .get_or_insert(envelope.time.observed_micros);
    accumulator.last_observed_micros = Some(envelope.time.observed_micros);
    if accumulator.examples.len() < example_limit {
        accumulator.examples.push(StatusExample {
            sequence: envelope.sequence,
            observed_micros: envelope.time.observed_micros,
            state: status_state_label(status.state),
            source_actor_id: status.source.map(|source| source.actor_id.0),
            source_entity_uuid: status.source.map(|source| source.entity_uuid.0),
            target_actor_id: status.target.actor_id.0,
            target_entity_uuid: status.target.entity_uuid.0,
            instance_id: status.instance_id.map(|instance| instance.0),
            stacks: status.stacks,
            level: status.level,
            duration_millis: status.duration_millis,
            capture_sequence: capture_sequence(&envelope.provenance.source),
            source_actor_snapshot_sequence: source_actor_snapshot.map(|value| value.0),
            source_actor_snapshot_observed_micros: source_actor_snapshot.map(|value| value.1),
            owner_actor_snapshot_sequence: owner_actor_snapshot.map(|value| value.0),
            owner_actor_snapshot_observed_micros: owner_actor_snapshot.map(|value| value.1),
        });
    }
}

fn classify_provider_with_same_packet_relations(
    source: Option<EntityRef>,
    sequence: u64,
    observed_micros: u64,
    actors: &HashMap<u64, ActorSnapshot>,
    ancestry: &ActorAncestryResolver,
    same_packet_relations: &[PacketAttributedRelation],
) -> ProviderResolution {
    let direct = classify_provider(source, observed_micros, actors, ancestry);
    if provider_resolution_is_player_owned(&direct) {
        return direct;
    }

    let eligible = same_packet_relations
        .iter()
        .copied()
        .filter(|relation| {
            relation.sequence > sequence && relation.observed_micros == observed_micros
        })
        .collect::<Vec<_>>();
    if eligible.is_empty() {
        return direct;
    }

    let mut owners_by_child = BTreeMap::<(u64, i64), BTreeSet<(u64, i64)>>::new();
    for relation in &eligible {
        owners_by_child
            .entry((relation.child.actor_id.0, relation.child.entity_uuid.0))
            .or_default()
            .insert((relation.owner.actor_id.0, relation.owner.entity_uuid.0));
    }

    let mut coobserved_ancestry = ancestry.clone();
    for relation in &eligible {
        let child = (relation.child.actor_id.0, relation.child.entity_uuid.0);
        if owners_by_child
            .get(&child)
            .is_some_and(|owners| owners.len() == 1)
        {
            coobserved_ancestry.observe_relation(
                observed_micros,
                relation.child,
                relation.owner,
                ActorOwnershipEvidence::AttributedCombatSource,
            );
        }
    }

    let mut candidate = classify_provider(source, observed_micros, actors, &coobserved_ancestry);
    if candidate.class != ResolutionClass::OwnedByPlayer {
        return direct;
    }
    let proving_sequence = eligible
        .iter()
        .filter(|relation| {
            candidate.ownership_chain.iter().any(|link| {
                link.child_actor_id == relation.child.actor_id.0
                    && link.child_entity_uuid == relation.child.entity_uuid.0
                    && link.owner_actor_id == relation.owner.actor_id.0
                    && link.owner_entity_uuid == relation.owner.entity_uuid.0
            })
        })
        .map(|relation| relation.sequence)
        .min();
    let Some(proving_sequence) = proving_sequence else {
        return direct;
    };
    candidate.class = ResolutionClass::SameWirePacketOwnedByPlayer;
    candidate.same_wire_packet_ownership_sequence = Some(proving_sequence);
    candidate
}

fn classify_provider(
    source: Option<EntityRef>,
    observed_micros: u64,
    actors: &HashMap<u64, ActorSnapshot>,
    ancestry: &ActorAncestryResolver,
) -> ProviderResolution {
    let Some(source) = source else {
        return ProviderResolution {
            class: ResolutionClass::MissingSource,
            source: None,
            resolved_owner: None,
            ownership_chain: Vec::new(),
            source_display_name: None,
            owner_display_name: None,
            player_character_id: None,
            player_primary_loadout_evidence: None,
            same_wire_packet_ownership_sequence: None,
            prior_status_instance_ownership_sequence: None,
        };
    };
    let source_snapshot = current_actor(actors, source);
    let source_identity = Some(identity_key(source, source_snapshot));
    let ownership_chain = ownership_chain(source, observed_micros, ancestry);
    let resolved = ownership_chain.last().map_or(source, |link| EntityRef {
        actor_id: rlogs_events::ActorId(link.owner_actor_id),
        entity_uuid: rlogs_events::EntityUuid(link.owner_entity_uuid),
    });
    let owner_snapshot = current_actor(actors, resolved);

    let class = if ownership_chain.is_empty() {
        match source_snapshot.map(|snapshot| snapshot.kind) {
            Some(ActorKind::Player) => ResolutionClass::DirectPlayer,
            Some(_) => ResolutionClass::NonPlayerUnowned,
            None => ResolutionClass::SourceIdentityUnobserved,
        }
    } else {
        match owner_snapshot.map(|snapshot| snapshot.kind) {
            Some(ActorKind::Player) => ResolutionClass::OwnedByPlayer,
            Some(_) => ResolutionClass::OwnedByNonPlayer,
            None => ResolutionClass::OwnerIdentityUnobserved,
        }
    };
    let player_character_id = match class {
        ResolutionClass::DirectPlayer => {
            source_snapshot.and_then(|snapshot| snapshot.character_id.clone())
        }
        ResolutionClass::OwnedByPlayer => {
            owner_snapshot.and_then(|snapshot| snapshot.character_id.clone())
        }
        _ => None,
    };
    let player_primary_loadout_evidence = match class {
        ResolutionClass::DirectPlayer => source_snapshot,
        ResolutionClass::OwnedByPlayer => owner_snapshot,
        _ => None,
    }
    .and_then(player_primary_loadout);

    ProviderResolution {
        class,
        source: source_identity,
        resolved_owner: (!ownership_chain.is_empty())
            .then(|| identity_key(resolved, owner_snapshot)),
        ownership_chain,
        source_display_name: source_snapshot.and_then(|snapshot| snapshot.display_name.clone()),
        owner_display_name: owner_snapshot.and_then(|snapshot| snapshot.display_name.clone()),
        player_character_id,
        player_primary_loadout_evidence,
        same_wire_packet_ownership_sequence: None,
        prior_status_instance_ownership_sequence: None,
    }
}

fn resolve_status_instance_provider(
    status: &StatusEvent,
    sequence: u64,
    direct: ProviderResolution,
    instances: &mut HashMap<StatusInstanceKey, StatusInstanceOwnership>,
) -> ProviderResolution {
    let Some(instance_id) = status.instance_id.map(|value| value.0) else {
        return direct;
    };
    let key = StatusInstanceKey {
        target_actor_id: status.target.actor_id.0,
        target_entity_uuid: status.target.entity_uuid.0,
        effect_id: status.effect.0,
        instance_id,
    };
    let active = matches!(
        status.state,
        StatusState::Applied | StatusState::Refreshed | StatusState::Stacked
    ) || (status.state == StatusState::Consumed
        && status.stacks.unwrap_or_default() > 0);
    let terminal = status.state == StatusState::Removed
        || (status.state == StatusState::Consumed && status.stacks.unwrap_or_default() == 0);

    let mut resolved = direct;
    if !provider_resolution_is_player_owned(&resolved) && status.state != StatusState::Applied {
        if let Some(StatusInstanceOwnership::Proven {
            resolution,
            established_sequence,
        }) = instances.get(&key)
            && status_source_matches_resolution(status.source, resolution)
        {
            resolved = resolution.clone();
            resolved.class = ResolutionClass::PriorStatusInstancePlayer;
            resolved.prior_status_instance_ownership_sequence = Some(*established_sequence);
        }
    }

    if active && provider_resolution_is_player_owned(&resolved) {
        match instances.get(&key) {
            Some(StatusInstanceOwnership::Proven { resolution, .. })
                if resolution.player_character_id != resolved.player_character_id =>
            {
                instances.insert(key, StatusInstanceOwnership::Conflicted);
            }
            Some(StatusInstanceOwnership::Conflicted) => {}
            Some(StatusInstanceOwnership::Proven { .. }) => {}
            None => {
                let mut stored = resolved.clone();
                stored.class = match stored.class {
                    ResolutionClass::SameWirePacketOwnedByPlayer
                    | ResolutionClass::PriorStatusInstancePlayer => ResolutionClass::OwnedByPlayer,
                    class => class,
                };
                stored.prior_status_instance_ownership_sequence = None;
                instances.insert(
                    key,
                    StatusInstanceOwnership::Proven {
                        resolution: stored,
                        established_sequence: sequence,
                    },
                );
            }
        }
    }
    if terminal {
        instances.remove(&key);
    }
    resolved
}

fn provider_resolution_is_player_owned(resolution: &ProviderResolution) -> bool {
    matches!(
        resolution.class,
        ResolutionClass::DirectPlayer
            | ResolutionClass::OwnedByPlayer
            | ResolutionClass::SameWirePacketOwnedByPlayer
            | ResolutionClass::PriorStatusInstancePlayer
    ) && resolution.player_character_id.is_some()
}

fn status_source_matches_resolution(
    source: Option<EntityRef>,
    resolution: &ProviderResolution,
) -> bool {
    match (source, resolution.source.as_ref()) {
        (None, _) => true,
        (Some(source), Some(expected)) => {
            source.actor_id.0 == expected.actor_id && source.entity_uuid.0 == expected.entity_uuid
        }
        (Some(_), None) => false,
    }
}

fn ownership_chain(
    source: EntityRef,
    observed_micros: u64,
    ancestry: &ActorAncestryResolver,
) -> Vec<OwnershipLink> {
    let mut chain = Vec::new();
    let mut current = source;
    let mut visited = BTreeSet::new();
    for _ in 0..MAX_ANCESTRY_DEPTH {
        if !visited.insert(current.actor_id.0) {
            break;
        }
        let Some(owner) = ancestry.direct_owner_at(current.actor_id.0, observed_micros) else {
            break;
        };
        if owner.actor_id == current.actor_id || visited.contains(&owner.actor_id.0) {
            break;
        }
        chain.push(OwnershipLink {
            child_actor_id: current.actor_id.0,
            child_entity_uuid: current.entity_uuid.0,
            owner_actor_id: owner.actor_id.0,
            owner_entity_uuid: owner.entity_uuid.0,
            attributed_combat_source: ancestry.has_direct_owner_evidence_at(
                current.actor_id.0,
                observed_micros,
                ActorOwnershipEvidence::AttributedCombatSource,
            ),
            confirmed_entity_attributes: ancestry.has_direct_owner_evidence_at(
                current.actor_id.0,
                observed_micros,
                ActorOwnershipEvidence::ConfirmedEntityAttributes,
            ),
        });
        current = owner;
    }
    chain
}

fn current_actor(actors: &HashMap<u64, ActorSnapshot>, actor: EntityRef) -> Option<&ActorSnapshot> {
    actors
        .get(&actor.actor_id.0)
        .filter(|snapshot| snapshot.actor.entity_uuid == actor.entity_uuid)
}

fn identity_key(actor: EntityRef, snapshot: Option<&ActorSnapshot>) -> ActorIdentityKey {
    ActorIdentityKey {
        actor_id: actor.actor_id.0,
        entity_uuid: actor.entity_uuid.0,
        kind: snapshot.map(|snapshot| actor_kind_label(snapshot.kind)),
        entity_type_id: snapshot.map(|snapshot| snapshot.entity_type_id),
        character_id: snapshot.and_then(|snapshot| snapshot.character_id.clone()),
        character_id_source: snapshot.and_then(|snapshot| snapshot.character_id_source.clone()),
        class_id: snapshot.and_then(|snapshot| snapshot.class_id),
        specialization_id: snapshot.and_then(|snapshot| snapshot.specialization_id),
    }
}

fn observe_actor(
    actors: &mut HashMap<u64, ActorSnapshot>,
    ancestry: &mut ActorAncestryResolver,
    event: &ActorEvent,
    sequence: u64,
    observed_micros: u64,
) -> Result<(), String> {
    let actor_id = event.actor.actor_id.0;
    if matches!(
        event.state,
        ActorState::Spawned | ActorState::Transformed | ActorState::Despawned
    ) {
        actors.remove(&actor_id);
        ancestry.clear_owner(observed_micros, event.actor);
    }
    if event.state == ActorState::Despawned {
        return Ok(());
    }
    let (character_id, character_id_source) = proven_character_id(event)?;
    ancestry.observe_entity(event.actor);
    match actors.get_mut(&actor_id) {
        Some(snapshot) if snapshot.actor.entity_uuid == event.actor.entity_uuid => {
            snapshot.kind = event.kind;
            snapshot.entity_type_id = event.entity_type_id;
            if character_id.is_some() {
                snapshot.character_id = character_id;
                snapshot.character_id_source = character_id_source;
            }
            if event.display_name.is_some() {
                snapshot.display_name = event.display_name.clone();
            }
            if event.class_id.is_some() {
                snapshot.class_id = event.class_id;
            }
            if event.specialization_id.is_some() {
                snapshot.specialization_id = event.specialization_id;
            }
            if event.loadout_observation.primary != ActorLoadoutEvidence::Unobserved {
                snapshot.primary_loadout = loadout_slots(&event.primary_loadout);
                snapshot.primary_loadout_evidence = event.loadout_observation.primary;
            }
            snapshot.observed_sequence = sequence;
            snapshot.observed_micros = observed_micros;
        }
        _ => {
            actors.insert(
                actor_id,
                ActorSnapshot {
                    actor: event.actor,
                    kind: event.kind,
                    entity_type_id: event.entity_type_id,
                    character_id,
                    character_id_source,
                    display_name: event.display_name.clone(),
                    class_id: event.class_id,
                    specialization_id: event.specialization_id,
                    primary_loadout: loadout_slots(&event.primary_loadout),
                    primary_loadout_evidence: event.loadout_observation.primary,
                    observed_sequence: sequence,
                    observed_micros,
                },
            );
        }
    }
    Ok(())
}

fn loadout_slots(slots: &[ActorLoadoutSlot]) -> Vec<LoadoutSlotEvidence> {
    slots
        .iter()
        .map(|slot| LoadoutSlotEvidence {
            slot_id: slot.slot_id,
            ability_id: slot.ability_id,
            item_id: slot.item_id,
            tier: slot.tier,
        })
        .collect()
}

fn player_primary_loadout(snapshot: &ActorSnapshot) -> Option<PlayerPrimaryLoadoutEvidence> {
    if snapshot.kind != ActorKind::Player
        || snapshot.primary_loadout_evidence == ActorLoadoutEvidence::Unobserved
    {
        return None;
    }
    Some(PlayerPrimaryLoadoutEvidence {
        character_id: snapshot.character_id.clone()?,
        actor_id: snapshot.actor.actor_id.0,
        entity_uuid: snapshot.actor.entity_uuid.0,
        evidence: snapshot.primary_loadout_evidence,
        slots: snapshot.primary_loadout.clone(),
    })
}

fn proven_character_id(event: &ActorEvent) -> Result<(Option<String>, Option<String>), String> {
    let explicit = event
        .character_id
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());
    if event.character_id.is_some() && explicit.is_none() {
        return Err("canonical actor character_id is empty".to_owned());
    }
    let derived = (event.kind == ActorKind::Player)
        .then(|| character_id_from_entity_uuid(event.actor.entity_uuid.0))
        .flatten();
    match (explicit, derived) {
        (Some(explicit), Some(derived)) if explicit != derived => Err(format!(
            "canonical actor character_id {explicit} disagrees with the BPSR player entity UUID contract {derived}"
        )),
        (Some(explicit), Some(_)) => Ok((
            Some(explicit.to_owned()),
            Some("canonical_actor_field_and_bpsr_entity_uuid_contract".to_owned()),
        )),
        (Some(explicit), None) => Ok((
            Some(explicit.to_owned()),
            Some("canonical_actor_field".to_owned()),
        )),
        (None, Some(derived)) => Ok((
            Some(derived),
            Some("bpsr_player_entity_uuid_contract".to_owned()),
        )),
        (None, None) => Ok((None, None)),
    }
}

fn reset_ancestry(ancestry: &mut ActorAncestryResolver, actors: &HashMap<u64, ActorSnapshot>) {
    ancestry.clear();
    for snapshot in actors.values() {
        ancestry.observe_entity(snapshot.actor);
    }
}

fn increment_summary_class(summary: &mut Summary, class: ResolutionClass) {
    match class {
        ResolutionClass::MissingSource => {
            summary.selected_events_with_missing_source = summary
                .selected_events_with_missing_source
                .saturating_add(1)
        }
        ResolutionClass::DirectPlayer => {
            summary.selected_events_with_direct_player_provider = summary
                .selected_events_with_direct_player_provider
                .saturating_add(1)
        }
        ResolutionClass::OwnedByPlayer => {
            summary.selected_events_with_player_owner =
                summary.selected_events_with_player_owner.saturating_add(1)
        }
        ResolutionClass::SameWirePacketOwnedByPlayer => {
            summary.selected_events_with_same_wire_packet_player_owner = summary
                .selected_events_with_same_wire_packet_player_owner
                .saturating_add(1)
        }
        ResolutionClass::PriorStatusInstancePlayer => {
            summary.selected_events_with_prior_status_instance_player_owner = summary
                .selected_events_with_prior_status_instance_player_owner
                .saturating_add(1)
        }
        ResolutionClass::OwnedByNonPlayer => {
            summary.selected_events_with_non_player_owner = summary
                .selected_events_with_non_player_owner
                .saturating_add(1)
        }
        ResolutionClass::OwnerIdentityUnobserved => {
            summary.selected_events_with_unobserved_owner_identity = summary
                .selected_events_with_unobserved_owner_identity
                .saturating_add(1)
        }
        ResolutionClass::NonPlayerUnowned => {
            summary.selected_events_with_non_player_unowned_source = summary
                .selected_events_with_non_player_unowned_source
                .saturating_add(1)
        }
        ResolutionClass::SourceIdentityUnobserved => {
            summary.selected_events_with_unobserved_source_identity = summary
                .selected_events_with_unobserved_source_identity
                .saturating_add(1)
        }
    }
}

impl EffectAccumulator {
    fn into_report(self, effect_id: i64) -> EffectReport {
        let sourced_events = self.status_events.saturating_sub(
            self.classes
                .get(&ResolutionClass::MissingSource)
                .copied()
                .unwrap_or(0),
        );
        let proven_events = self
            .classes
            .get(&ResolutionClass::DirectPlayer)
            .copied()
            .unwrap_or(0)
            .saturating_add(
                self.classes
                    .get(&ResolutionClass::OwnedByPlayer)
                    .copied()
                    .unwrap_or(0),
            )
            .saturating_add(
                self.classes
                    .get(&ResolutionClass::PriorStatusInstancePlayer)
                    .copied()
                    .unwrap_or(0),
            )
            .saturating_add(
                self.classes
                    .get(&ResolutionClass::SameWirePacketOwnedByPlayer)
                    .copied()
                    .unwrap_or(0),
            );
        EffectReport {
            effect_id,
            status_events: self.status_events,
            resolution_counts: self.classes,
            unique_source_entities: self.source_entities.len(),
            proven_player_character_ids: self.player_character_ids.into_iter().collect(),
            proven_player_primary_loadouts: self.player_primary_loadouts.into_iter().collect(),
            player_actor_ownership_proven_for_every_sourced_event: sourced_events > 0
                && proven_events == sourced_events,
            status_events_with_stable_player_character_id: self
                .events_with_stable_player_character_id,
            stable_player_character_id_proven_for_every_sourced_event: sourced_events > 0
                && self.events_with_stable_player_character_id == sourced_events,
            status_events_with_player_primary_loadout_evidence: self
                .events_with_player_primary_loadout_evidence,
            player_primary_loadout_proven_for_every_player_owned_event: proven_events > 0
                && self.events_with_player_primary_loadout_evidence == proven_events,
            formula_authority: false,
            runtime_authority: false,
        }
    }
}

impl ResolutionAccumulator {
    fn into_report(self, key: ResolutionKey) -> ResolutionReport {
        ResolutionReport {
            rlog: key.rlog,
            session_id: key.session_id,
            run_ordinal: key.run_ordinal,
            effect_id: key.effect_id,
            origin_source_type_id: key.origin_source_type_id,
            origin_source_config_id: key.origin_source_config_id,
            class: key.class,
            source: key.source.map(ActorIdentityEvidence::from),
            resolved_owner: key.resolved_owner.map(ActorIdentityEvidence::from),
            ownership_chain: key.ownership_chain,
            same_wire_packet_ownership_sequence: key.same_wire_packet_ownership_sequence,
            prior_status_instance_ownership_sequence: key.prior_status_instance_ownership_sequence,
            player_primary_loadout_evidence: key.player_primary_loadout_evidence,
            source_display_name_evidence: self.source_display_names.into_iter().collect(),
            owner_display_name_evidence: self.owner_display_names.into_iter().collect(),
            status_events: self.status_events,
            status_state_counts: self.state_counts,
            unique_target_entities: self.target_entity_uuids.len(),
            first_sequence: self.first_sequence.unwrap_or_default(),
            last_sequence: self.last_sequence.unwrap_or_default(),
            first_observed_micros: self.first_observed_micros.unwrap_or_default(),
            last_observed_micros: self.last_observed_micros.unwrap_or_default(),
            examples: self.examples,
            ownership_only: true,
            formula_authority: false,
            runtime_authority: false,
        }
    }
}

impl From<ActorIdentityKey> for ActorIdentityEvidence {
    fn from(value: ActorIdentityKey) -> Self {
        Self {
            actor_id: value.actor_id,
            entity_uuid: value.entity_uuid,
            kind: value.kind,
            entity_type_id: value.entity_type_id,
            character_id: value.character_id,
            character_id_source: value.character_id_source,
            class_id: value.class_id,
            specialization_id: value.specialization_id,
        }
    }
}

fn actor_kind_label(kind: ActorKind) -> String {
    match kind {
        ActorKind::Player => "player".to_owned(),
        ActorKind::Monster => "monster".to_owned(),
        ActorKind::Npc => "npc".to_owned(),
        ActorKind::SceneObject => "scene_object".to_owned(),
        ActorKind::Zone => "zone".to_owned(),
        ActorKind::Projectile => "projectile".to_owned(),
        ActorKind::Pet => "pet".to_owned(),
        ActorKind::TrainingDummy => "training_dummy".to_owned(),
        ActorKind::Drop => "drop".to_owned(),
        ActorKind::Field => "field".to_owned(),
        ActorKind::Trap => "trap".to_owned(),
        ActorKind::Collection => "collection".to_owned(),
        ActorKind::StaticObject => "static_object".to_owned(),
        ActorKind::Vehicle => "vehicle".to_owned(),
        ActorKind::Toy => "toy".to_owned(),
        ActorKind::Housing => "housing".to_owned(),
        ActorKind::Unknown(value) => format!("unknown:{value}"),
    }
}

fn status_state_label(state: StatusState) -> &'static str {
    match state {
        StatusState::Applied => "applied",
        StatusState::Refreshed => "refreshed",
        StatusState::Stacked => "stacked",
        StatusState::Consumed => "consumed",
        StatusState::Removed => "removed",
    }
}

fn capture_sequence(source: &EvidenceSource) -> Option<u64> {
    match source {
        EvidenceSource::Wire {
            capture_sequence, ..
        } => Some(*capture_sequence),
        EvidenceSource::Derived {
            evidence_sequences, ..
        } => evidence_sequences.first().copied(),
        EvidenceSource::Manual { .. } => None,
    }
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

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| display_path(path))
}

fn parse_arguments<I>(mut arguments: I) -> Result<Arguments, String>
where
    I: Iterator<Item = OsString>,
{
    let mut rlogs = Vec::new();
    let mut effects = BTreeSet::new();
    let mut output = None;
    let mut example_limit = DEFAULT_EXAMPLE_LIMIT;
    while let Some(flag) = arguments.next() {
        match flag.to_string_lossy().as_ref() {
            "--rlog" => rlogs.push(next_path(&mut arguments, "--rlog")?),
            "--effect" => {
                let effect = next_i64(&mut arguments, "--effect")?;
                if effect <= 0 {
                    return Err("--effect requires a positive exact numeric effect ID".to_owned());
                }
                effects.insert(effect);
            }
            "--output" => output = Some(next_path(&mut arguments, "--output")?),
            "--example-limit" => example_limit = next_usize(&mut arguments, "--example-limit")?,
            _ => return Err(usage()),
        }
    }
    if rlogs.is_empty() || effects.is_empty() || output.is_none() {
        return Err(usage());
    }
    Ok(Arguments {
        rlogs,
        effects,
        output: output.expect("validated output"),
        example_limit,
    })
}

fn next_path<I>(arguments: &mut I, flag: &str) -> Result<PathBuf, String>
where
    I: Iterator<Item = OsString>,
{
    arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| format!("missing path after {flag}\n{}", usage()))
}

fn next_i64<I>(arguments: &mut I, flag: &str) -> Result<i64, String>
where
    I: Iterator<Item = OsString>,
{
    arguments
        .next()
        .ok_or_else(|| format!("missing value after {flag}"))?
        .to_string_lossy()
        .parse()
        .map_err(|_| format!("{flag} requires an integer"))
}

fn next_usize<I>(arguments: &mut I, flag: &str) -> Result<usize, String>
where
    I: Iterator<Item = OsString>,
{
    arguments
        .next()
        .ok_or_else(|| format!("missing value after {flag}"))?
        .to_string_lossy()
        .parse()
        .map_err(|_| format!("{flag} requires a non-negative integer"))
}

fn usage() -> String {
    "usage: rlogs-bpsr-status-effect-provider-ownership-proof --rlog <exact-build.rlog> [--rlog ...] --effect <exact-numeric-effect-id> [--effect ...] --output <ownership-proof.json> [--example-limit <count>]".to_owned()
}

#[cfg(test)]
mod tests {
    use rlogs_events::{ActorId, EntityUuid};

    use super::*;

    fn entity(actor_id: u64, entity_uuid: i64) -> EntityRef {
        EntityRef {
            actor_id: ActorId(actor_id),
            entity_uuid: EntityUuid(entity_uuid),
        }
    }

    fn snapshot(actor: EntityRef, kind: ActorKind, name: Option<&str>) -> ActorSnapshot {
        ActorSnapshot {
            actor,
            kind,
            entity_type_id: 1,
            character_id: (kind == ActorKind::Player)
                .then(|| format!("character-{}", actor.actor_id.0)),
            character_id_source: (kind == ActorKind::Player).then(|| "test_fixture".to_owned()),
            display_name: name.map(str::to_owned),
            class_id: None,
            specialization_id: None,
            primary_loadout: Vec::new(),
            primary_loadout_evidence: ActorLoadoutEvidence::Unobserved,
            observed_sequence: 1,
            observed_micros: 10,
        }
    }

    fn actor_event(actor: EntityRef, kind: ActorKind, character_id: Option<&str>) -> ActorEvent {
        ActorEvent {
            actor,
            state: ActorState::Updated,
            entity_type_id: 1,
            kind,
            monster_id: None,
            character_id: character_id.map(str::to_owned),
            display_name: None,
            class_id: None,
            specialization_id: None,
            level: None,
            ability_score: None,
            weapon_item_id: None,
            weapon_breakthrough_count: None,
            seasonal_score: None,
            primary_loadout: Vec::new(),
            auxiliary_loadout: Vec::new(),
            loadout_observation: rlogs_events::ActorLoadoutObservation::default(),
        }
    }

    fn status(source: EntityRef, target: EntityRef, state: StatusState) -> StatusEvent {
        StatusEvent {
            source: Some(source),
            target,
            effect: rlogs_events::StatusEffectId(2110092),
            instance_id: Some(rlogs_events::StatusEffectInstanceId(77)),
            origin: None,
            state,
            stacks: Some(u32::from(state != StatusState::Removed)),
            duration_millis: Some(10_000),
            level: Some(1),
            part_id: None,
            count: None,
            created_at_millis: None,
        }
    }

    #[test]
    fn bpsr_player_entity_uuid_contract_fills_legacy_actor_identity() {
        let player = entity(1, 216_009_015_936);
        let event = actor_event(player, ActorKind::Player, None);
        let (character_id, source) = proven_character_id(&event).expect("exact BPSR player UID");
        assert_eq!(character_id.as_deref(), Some("3296036"));
        assert_eq!(source.as_deref(), Some("bpsr_player_entity_uuid_contract"));
    }

    #[test]
    fn explicit_character_id_mismatch_is_rejected() {
        let player = entity(1, 216_009_015_936);
        let event = actor_event(player, ActorKind::Player, Some("3296037"));
        let error = proven_character_id(&event).expect_err("mismatch must fail closed");
        assert!(error.contains("disagrees"));
    }

    #[test]
    fn direct_actor_kind_proves_player_provider() {
        let player = entity(1, 100);
        let actors = HashMap::from([(1, snapshot(player, ActorKind::Player, Some("localized")))]);
        let mut ancestry = ActorAncestryResolver::default();
        ancestry.observe_entity(player);
        let resolution = classify_provider(Some(player), 20, &actors, &ancestry);
        assert_eq!(resolution.class, ResolutionClass::DirectPlayer);
        assert_eq!(
            resolution.player_character_id.as_deref(),
            Some("character-1")
        );
    }

    #[test]
    fn resolved_player_snapshot_carries_exact_primary_loadout_tier() {
        let player = entity(1, 100);
        let mut player_snapshot = snapshot(player, ActorKind::Player, Some("provider"));
        player_snapshot.primary_loadout_evidence = ActorLoadoutEvidence::ExactSlots;
        player_snapshot.primary_loadout = vec![LoadoutSlotEvidence {
            slot_id: 7,
            ability_id: Some(3946),
            item_id: Some(3_000_045),
            tier: Some(5),
        }];
        let actors = HashMap::from([(1, player_snapshot)]);
        let mut ancestry = ActorAncestryResolver::default();
        ancestry.observe_entity(player);

        let resolution = classify_provider(Some(player), 20, &actors, &ancestry);
        let loadout = resolution
            .player_primary_loadout_evidence
            .expect("exact provider loadout");
        assert_eq!(loadout.character_id, "character-1");
        assert_eq!(loadout.evidence, ActorLoadoutEvidence::ExactSlots);
        assert_eq!(loadout.slots[0].ability_id, Some(3946));
        assert_eq!(loadout.slots[0].tier, Some(5));
    }

    #[test]
    fn exact_pet_relation_resolves_player_owner() {
        let player = entity(1, 100);
        let pet = entity(2, 200);
        let actors = HashMap::from([
            (1, snapshot(player, ActorKind::Player, Some("provider"))),
            (2, snapshot(pet, ActorKind::Pet, Some("summon"))),
        ]);
        let mut ancestry = ActorAncestryResolver::default();
        ancestry.observe_relation(
            15,
            pet,
            player,
            ActorOwnershipEvidence::ConfirmedEntityAttributes,
        );
        let resolution = classify_provider(Some(pet), 20, &actors, &ancestry);
        assert_eq!(resolution.class, ResolutionClass::OwnedByPlayer);
        assert_eq!(resolution.ownership_chain.len(), 1);
        assert!(resolution.ownership_chain[0].confirmed_entity_attributes);
    }

    #[test]
    fn display_name_alone_never_proves_player_identity() {
        let source = entity(7, 700);
        let actors = HashMap::from([(
            7,
            snapshot(source, ActorKind::Pet, Some("same name as a player")),
        )]);
        let mut ancestry = ActorAncestryResolver::default();
        ancestry.observe_entity(source);
        let resolution = classify_provider(Some(source), 20, &actors, &ancestry);
        assert_eq!(resolution.class, ResolutionClass::NonPlayerUnowned);
        assert!(resolution.player_character_id.is_none());
    }

    #[test]
    fn later_actor_snapshot_does_not_backfill_prior_status() {
        let source = entity(9, 900);
        let mut actors = HashMap::new();
        let mut ancestry = ActorAncestryResolver::default();
        ancestry.observe_entity(source);
        let before = classify_provider(Some(source), 20, &actors, &ancestry);
        assert_eq!(before.class, ResolutionClass::SourceIdentityUnobserved);

        actors.insert(9, snapshot(source, ActorKind::Player, Some("later")));
        let after = classify_provider(Some(source), 30, &actors, &ancestry);
        assert_eq!(after.class, ResolutionClass::DirectPlayer);
        assert_eq!(before.class, ResolutionClass::SourceIdentityUnobserved);
    }

    #[test]
    fn later_attributed_relation_in_same_wire_packet_resolves_projectile_provider() {
        let player = entity(1, 100);
        let projectile = entity(9, 900);
        let actors = HashMap::from([
            (1, snapshot(player, ActorKind::Player, Some("provider"))),
            (
                9,
                snapshot(projectile, ActorKind::Projectile, Some("projectile")),
            ),
        ]);
        let mut ancestry = ActorAncestryResolver::default();
        ancestry.observe_entity(player);
        ancestry.observe_entity(projectile);
        let relations = [PacketAttributedRelation {
            sequence: 11,
            observed_micros: 20,
            child: projectile,
            owner: player,
        }];

        let resolution = classify_provider_with_same_packet_relations(
            Some(projectile),
            10,
            20,
            &actors,
            &ancestry,
            &relations,
        );
        assert_eq!(
            resolution.class,
            ResolutionClass::SameWirePacketOwnedByPlayer
        );
        assert_eq!(resolution.same_wire_packet_ownership_sequence, Some(11));
        assert_eq!(
            resolution.player_character_id.as_deref(),
            Some("character-1")
        );
        assert_eq!(
            classify_provider(Some(projectile), 20, &actors, &ancestry).class,
            ResolutionClass::NonPlayerUnowned
        );
    }

    #[test]
    fn different_observed_time_does_not_enable_same_packet_resolution() {
        let player = entity(1, 100);
        let projectile = entity(9, 900);
        let actors = HashMap::from([
            (1, snapshot(player, ActorKind::Player, Some("provider"))),
            (
                9,
                snapshot(projectile, ActorKind::Projectile, Some("projectile")),
            ),
        ]);
        let relations = [PacketAttributedRelation {
            sequence: 11,
            observed_micros: 21,
            child: projectile,
            owner: player,
        }];
        let resolution = classify_provider_with_same_packet_relations(
            Some(projectile),
            10,
            20,
            &actors,
            &ActorAncestryResolver::default(),
            &relations,
        );
        assert_eq!(resolution.class, ResolutionClass::NonPlayerUnowned);
    }

    #[test]
    fn conflicting_same_packet_owners_fail_closed() {
        let first_player = entity(1, 100);
        let second_player = entity(2, 200);
        let projectile = entity(9, 900);
        let actors = HashMap::from([
            (
                1,
                snapshot(first_player, ActorKind::Player, Some("first provider")),
            ),
            (
                2,
                snapshot(second_player, ActorKind::Player, Some("second provider")),
            ),
            (
                9,
                snapshot(projectile, ActorKind::Projectile, Some("projectile")),
            ),
        ]);
        let relations = [
            PacketAttributedRelation {
                sequence: 11,
                observed_micros: 20,
                child: projectile,
                owner: first_player,
            },
            PacketAttributedRelation {
                sequence: 12,
                observed_micros: 20,
                child: projectile,
                owner: second_player,
            },
        ];
        let resolution = classify_provider_with_same_packet_relations(
            Some(projectile),
            10,
            20,
            &actors,
            &ActorAncestryResolver::default(),
            &relations,
        );
        assert_eq!(resolution.class, ResolutionClass::NonPlayerUnowned);
    }

    #[test]
    fn prior_exact_status_instance_player_owner_flows_forward_to_removal() {
        let player = entity(1, 100);
        let target = entity(2, 200);
        let actors = HashMap::from([(1, snapshot(player, ActorKind::Player, Some("provider")))]);
        let mut ancestry = ActorAncestryResolver::default();
        ancestry.observe_entity(player);
        let mut instances = HashMap::new();

        let applied = resolve_status_instance_provider(
            &status(player, target, StatusState::Applied),
            10,
            classify_provider(Some(player), 10, &actors, &ancestry),
            &mut instances,
        );
        assert_eq!(applied.class, ResolutionClass::DirectPlayer);

        let removed = resolve_status_instance_provider(
            &status(player, target, StatusState::Removed),
            20,
            classify_provider(Some(player), 20, &HashMap::new(), &ancestry),
            &mut instances,
        );
        assert_eq!(removed.class, ResolutionClass::PriorStatusInstancePlayer);
        assert_eq!(removed.player_character_id.as_deref(), Some("character-1"));
        assert_eq!(removed.prior_status_instance_ownership_sequence, Some(10));
        assert!(instances.is_empty());
    }

    #[test]
    fn later_player_resolution_does_not_flow_backward_to_application() {
        let player = entity(1, 100);
        let target = entity(2, 200);
        let mut ancestry = ActorAncestryResolver::default();
        ancestry.observe_entity(player);
        let mut instances = HashMap::new();

        let applied = resolve_status_instance_provider(
            &status(player, target, StatusState::Applied),
            10,
            classify_provider(Some(player), 10, &HashMap::new(), &ancestry),
            &mut instances,
        );
        assert_eq!(applied.class, ResolutionClass::SourceIdentityUnobserved);
        assert!(instances.is_empty());

        let actors = HashMap::from([(1, snapshot(player, ActorKind::Player, Some("later")))]);
        let removed = resolve_status_instance_provider(
            &status(player, target, StatusState::Removed),
            20,
            classify_provider(Some(player), 20, &actors, &ancestry),
            &mut instances,
        );
        assert_eq!(removed.class, ResolutionClass::DirectPlayer);
        assert_eq!(applied.class, ResolutionClass::SourceIdentityUnobserved);
    }

    #[test]
    fn conflicting_exact_status_instance_owner_disables_later_inheritance() {
        let first_player = entity(1, 100);
        let second_player = entity(3, 300);
        let target = entity(2, 200);
        let actors = HashMap::from([
            (1, snapshot(first_player, ActorKind::Player, Some("first"))),
            (
                3,
                snapshot(second_player, ActorKind::Player, Some("second")),
            ),
        ]);
        let mut ancestry = ActorAncestryResolver::default();
        ancestry.observe_entity(first_player);
        ancestry.observe_entity(second_player);
        let mut instances = HashMap::new();

        resolve_status_instance_provider(
            &status(first_player, target, StatusState::Applied),
            10,
            classify_provider(Some(first_player), 10, &actors, &ancestry),
            &mut instances,
        );
        resolve_status_instance_provider(
            &status(second_player, target, StatusState::Refreshed),
            20,
            classify_provider(Some(second_player), 20, &actors, &ancestry),
            &mut instances,
        );
        assert!(matches!(
            instances.values().next(),
            Some(StatusInstanceOwnership::Conflicted)
        ));

        let removed = resolve_status_instance_provider(
            &status(first_player, target, StatusState::Removed),
            30,
            classify_provider(Some(first_player), 30, &HashMap::new(), &ancestry),
            &mut instances,
        );
        assert_eq!(removed.class, ResolutionClass::SourceIdentityUnobserved);
    }

    #[test]
    fn parser_requires_exact_positive_effect_ids() {
        let error = parse_arguments(
            ["--rlog", "a.rlog", "--effect", "0", "--output", "out.json"]
                .into_iter()
                .map(OsString::from),
        )
        .expect_err("zero is not an effect identity");
        assert!(error.contains("positive exact numeric effect ID"));
    }
}
