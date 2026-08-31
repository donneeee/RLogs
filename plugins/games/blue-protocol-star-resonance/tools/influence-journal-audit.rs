#![allow(clippy::field_reassign_with_default)]

use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    error::Error,
    fs::{self, File},
    io::{BufReader, BufWriter, Write},
    path::PathBuf,
};

use rlogs_events::{
    CanonicalEvent, CastEvent, CastState, DamageEvent, RegionIdentity, StatusEvent, StatusState,
    TimelineEventKind,
};
use rlogs_game_bpsr::{
    CaptureRecordKind, JsonlJournalReader, ProtocolDecodeStatus, ProtocolPack, ProtocolRuntime,
    ProtocolRuntimeConfig,
};
use serde::{Deserialize, Serialize};

const REPORT_SCHEMA_VERSION: u16 = 1;

#[derive(Debug)]
struct Arguments {
    pack: PathBuf,
    journal: PathBuf,
    relationship_overview: PathBuf,
    output: PathBuf,
}

#[derive(Debug, Deserialize)]
struct RelationshipOverview {
    schema_version: u16,
    static_game_build: String,
    policy: RelationshipPolicy,
    source_effect_edges: Vec<SourceEffectEdge>,
    source_damage_edges: Vec<SourceDamageEdge>,
}

#[derive(Debug, Deserialize)]
struct RelationshipPolicy {
    unresolved_evidence_hidden: bool,
    static_relationships_are_not_runtime_amounts: bool,
    matching_build_counterfactual_required_for_attributed_damage: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SourceEffectEdge {
    source_identity: String,
    effect_id: i64,
    relationship: String,
    proof_state: String,
}

#[derive(Debug, Deserialize)]
struct SourceDamageEdge {
    source_identity: String,
    affected_damage_id: i64,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ActiveStatusKey {
    target_actor_id: u64,
    effect_id: i64,
    instance_id: Option<i64>,
    provider_actor_id: Option<u64>,
}

#[derive(Clone, Debug)]
struct ActiveStatus {
    key: ActiveStatusKey,
    origin_source_type_id: Option<i32>,
    origin_source_config_id: Option<i64>,
    applied_micros: u64,
    last_observed_micros: u64,
    expires_micros: Option<u64>,
    stacks: Option<u32>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct EffectDamageWindowKey {
    placement: &'static str,
    effect_id: i64,
    provider_actor_id: Option<u64>,
    recipient_actor_id: u64,
    affected_damage_id: Option<i64>,
    packet_owner_id: Option<i32>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    attacker_actor_id: u64,
    target_actor_id: u64,
}

#[derive(Debug, Default)]
struct EffectDamageWindowAccumulator {
    first_observed_micros: u64,
    last_observed_micros: u64,
    damage_event_count: u64,
    observed_damage: i128,
    status_instance_ids: BTreeSet<i64>,
    origin_pairs: BTreeSet<(i32, i64)>,
    observed_stack_counts: BTreeSet<u32>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct CastIdentityKey {
    source_actor_id: u64,
    ability_id: i64,
    state: &'static str,
    action_instance_id: Option<i64>,
    base_ability_id: Option<i64>,
    slot_id: Option<i32>,
}

#[derive(Debug, Default)]
struct ObservationAccumulator {
    first_observed_micros: u64,
    last_observed_micros: u64,
    event_count: u64,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct OriginEffectKey {
    source_type_id: i32,
    source_config_id: i64,
    effect_id: i64,
    provider_actor_id: Option<u64>,
    recipient_actor_id: u64,
}

#[derive(Debug, Default)]
struct OriginEffectAccumulator {
    first_observed_micros: u64,
    last_observed_micros: u64,
    applied: u64,
    refreshed: u64,
    stacked: u64,
    consumed: u64,
    removed: u64,
    instance_ids: BTreeSet<i64>,
    stack_counts: BTreeSet<u32>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct DamageIdentityKey {
    affected_damage_id: Option<i64>,
    packet_owner_id: Option<i32>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    owner_level: Option<i32>,
    owner_stage: Option<i32>,
    skill_effect_uuid: Option<i64>,
}

#[derive(Debug, Default)]
struct DamageIdentityAccumulator {
    first_observed_micros: u64,
    last_observed_micros: u64,
    event_count: u64,
    observed_damage: i128,
    actual_amount: i128,
    actual_amount_count: u64,
    hp_loss: i128,
    hp_loss_count: u64,
    shield_loss: i128,
    shield_loss_count: u64,
    attacker_actor_ids: BTreeSet<u64>,
    direct_attacker_actor_ids: BTreeSet<u64>,
    target_actor_ids: BTreeSet<u64>,
}

#[derive(Debug, Default, Serialize)]
struct StatusLifecycleSummary {
    applied: u64,
    refreshed: u64,
    stacked: u64,
    consumed: u64,
    removed: u64,
    nominally_expired: u64,
    first_observed_micros: u64,
    last_observed_micros: u64,
    provider_actor_ids: BTreeSet<String>,
    recipient_actor_ids: BTreeSet<String>,
    instance_ids: BTreeSet<String>,
    origin_pairs: BTreeSet<String>,
    stack_counts: BTreeSet<u32>,
}

#[derive(Debug, Serialize)]
struct EffectDamageWindowSummary {
    placement: &'static str,
    effect_id: i64,
    provider_actor_id: Option<String>,
    recipient_actor_id: String,
    affected_damage_id: Option<i64>,
    packet_owner_id: Option<i32>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    attacker_actor_id: String,
    target_actor_id: String,
    first_observed_micros: u64,
    last_observed_micros: u64,
    damage_event_count: u64,
    observed_damage: String,
    status_instance_ids: Vec<String>,
    origin_pairs: Vec<String>,
    observed_stack_counts: Vec<u32>,
    source_candidates: Vec<SourceCandidate>,
    proof_state: &'static str,
    attributed_damage_delta: Option<String>,
    blockers: Vec<&'static str>,
}

#[derive(Clone, Debug, Serialize)]
struct SourceCandidate {
    source_identity: String,
    relationship: String,
    static_proof_state: String,
    damage_target_catalogued: bool,
}

#[derive(Debug, Serialize)]
struct CastIdentitySummary {
    source_actor_id: String,
    ability_id: i64,
    state: &'static str,
    action_instance_id: Option<String>,
    base_ability_id: Option<i64>,
    slot_id: Option<i32>,
    first_observed_micros: u64,
    last_observed_micros: u64,
    event_count: u64,
}

#[derive(Debug, Serialize)]
struct OriginEffectSummary {
    source_type_id: i32,
    source_config_id: i64,
    effect_id: i64,
    provider_actor_id: Option<String>,
    recipient_actor_id: String,
    first_observed_micros: u64,
    last_observed_micros: u64,
    applied: u64,
    refreshed: u64,
    stacked: u64,
    consumed: u64,
    removed: u64,
    instance_ids: Vec<String>,
    stack_counts: Vec<u32>,
    static_source_candidates: Vec<SourceEffectEdge>,
    proof_state: &'static str,
}

#[derive(Debug, Serialize)]
struct DamageIdentitySummary {
    affected_damage_id: Option<i64>,
    packet_owner_id: Option<i32>,
    hit_event_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    owner_level: Option<i32>,
    owner_stage: Option<i32>,
    skill_effect_uuid: Option<String>,
    first_observed_micros: u64,
    last_observed_micros: u64,
    event_count: u64,
    observed_damage: String,
    actual_amount: Option<String>,
    hp_loss: Option<String>,
    shield_loss: Option<String>,
    attacker_actor_ids: Vec<String>,
    direct_attacker_actor_ids: Vec<String>,
    target_actor_ids: Vec<String>,
    proof_state: &'static str,
}

#[derive(Debug, Serialize)]
struct AuditCoverage {
    cast_identity_count: usize,
    exact_origin_effect_edge_count: usize,
    damage_identity_count: usize,
    effect_damage_window_count: usize,
    unique_packet_effect_ids: usize,
    unique_affected_damage_ids: usize,
    origin_effect_edges_without_static_source_candidate: usize,
    effect_damage_windows_without_static_source_candidate: usize,
    effect_damage_windows_with_ambiguous_static_source_candidates: usize,
    effect_damage_windows_without_matching_source_damage_edge: usize,
    proof_queue_row_count: usize,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ProofQueueKey {
    placement: &'static str,
    effect_id: i64,
    affected_damage_id: Option<i64>,
}

#[derive(Debug, Default)]
struct ProofQueueAccumulator {
    packet_window_rows: u64,
    damage_event_count: u64,
    observed_damage: i128,
    provider_actor_ids: BTreeSet<String>,
    recipient_actor_ids: BTreeSet<String>,
    attacker_actor_ids: BTreeSet<String>,
    target_actor_ids: BTreeSet<String>,
    origin_pairs: BTreeSet<String>,
    source_candidates: BTreeSet<String>,
    catalogued_source_candidates: BTreeSet<String>,
}

#[derive(Debug, Serialize)]
struct ProofQueueEntry {
    placement: &'static str,
    effect_id: i64,
    affected_damage_id: Option<i64>,
    packet_window_rows: u64,
    damage_event_count: u64,
    observed_damage: String,
    provider_actor_ids: Vec<String>,
    recipient_actor_ids: Vec<String>,
    attacker_actor_ids: Vec<String>,
    target_actor_ids: Vec<String>,
    origin_pairs: Vec<String>,
    source_candidates: Vec<String>,
    catalogued_source_candidates: Vec<String>,
    promotion_eligible: bool,
    blockers: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct AuditReport {
    schema_version: u16,
    generated_by: &'static str,
    authority: &'static str,
    capture_id: String,
    deployment_id: String,
    region_id: Option<String>,
    channel: String,
    game_build: String,
    protocol_pack_id: String,
    protocol_pack_digest: String,
    journal_record_count: u64,
    capture_gap_count: u64,
    decoded_event_count: u64,
    decode_statuses: BTreeMap<String, u64>,
    canonical_event_kinds: BTreeMap<String, u64>,
    cast_identities: Vec<CastIdentitySummary>,
    status_lifecycles: BTreeMap<i64, StatusLifecycleSummary>,
    exact_origin_effect_edges: Vec<OriginEffectSummary>,
    damage_identities: Vec<DamageIdentitySummary>,
    effect_damage_windows: Vec<EffectDamageWindowSummary>,
    coverage: AuditCoverage,
    proof_queue: Vec<ProofQueueEntry>,
    active_statuses_at_end: u64,
    policy: AuditPolicy,
}

#[derive(Debug, Serialize)]
struct AuditPolicy {
    exact_build_required: bool,
    exact_protocol_digest_required: bool,
    retained_services: Vec<&'static str>,
    account_and_login_services_retained: bool,
    effect_damage_window_is_attribution_proof: bool,
    all_decoded_casts_inventoried: bool,
    all_exact_status_origins_inventoried: bool,
    all_decoded_damage_identities_inventoried: bool,
    proof_queue_may_promote_runtime_attribution: bool,
    unresolved_relationships_retained: bool,
    missing_amounts_default_to_zero: bool,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("influence journal audit failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let arguments = arguments()?;
    if arguments.output.exists() {
        return Err(format!("refusing to overwrite {}", arguments.output.display()).into());
    }

    let pack = ProtocolPack::from_json(&fs::read(&arguments.pack)?)?;
    let overview: RelationshipOverview =
        serde_json::from_slice(&fs::read(&arguments.relationship_overview)?)?;
    validate_overview(&overview)?;

    let mut stream = JsonlJournalReader::new(BufReader::new(File::open(&arguments.journal)?))
        .into_record_stream()?;
    let session = stream.session().clone();
    let allowed_service_ids = validate_inputs(&pack, &session, &overview)?;

    let region = RegionIdentity {
        deployment_id: session.game_build.deployment_id.clone(),
        region_id: session
            .game_build
            .region_id
            .clone()
            .unwrap_or_else(|| session.game_build.deployment_id.clone()),
        realm_id: None,
        world_id: None,
    };
    let mut runtime = ProtocolRuntime::new(
        &pack,
        session.capture_id.clone(),
        &session.game_build,
        region,
        vec![],
        ProtocolRuntimeConfig::default(),
    )?;
    let mut decode_statuses = BTreeMap::<String, u64>::new();
    let mut canonical_event_kinds = BTreeMap::<String, u64>::new();
    let mut status_lifecycles = BTreeMap::<i64, StatusLifecycleSummary>::new();
    let mut cast_identities = BTreeMap::<CastIdentityKey, ObservationAccumulator>::new();
    let mut origin_effect_edges = BTreeMap::<OriginEffectKey, OriginEffectAccumulator>::new();
    let mut damage_identities = BTreeMap::<DamageIdentityKey, DamageIdentityAccumulator>::new();
    let mut active_statuses = BTreeMap::<ActiveStatusKey, ActiveStatus>::new();
    let mut effect_damage_windows =
        BTreeMap::<EffectDamageWindowKey, EffectDamageWindowAccumulator>::new();
    let mut capture_gap_count = 0_u64;
    let mut decoded_event_count = 0_u64;
    let mut last_observed_micros = 0_u64;

    while let Some(record) = stream.next_record()? {
        last_observed_micros = record.observed_micros;
        if matches!(record.kind, CaptureRecordKind::Gap(_)) {
            capture_gap_count = capture_gap_count.saturating_add(1);
        }
        validate_research_record(&record, &allowed_service_ids)?;
        expire_statuses(
            &mut active_statuses,
            &mut status_lifecycles,
            record.observed_micros,
        );

        let batch = runtime.process(&record)?;
        *decode_statuses
            .entry(decode_status_name(batch.status).into())
            .or_default() += 1;
        decoded_event_count = decoded_event_count.saturating_add(batch.events.len() as u64);
        for envelope in batch.events {
            let timeline = match envelope.event {
                CanonicalEvent::Timeline(timeline) => timeline,
                CanonicalEvent::CharacterProfileObserved { .. } => {
                    *canonical_event_kinds
                        .entry("character_profile".into())
                        .or_default() += 1;
                    continue;
                }
                CanonicalEvent::PartyRosterObserved(_) => {
                    *canonical_event_kinds
                        .entry("party_roster_observed".into())
                        .or_default() += 1;
                    continue;
                }
                CanonicalEvent::PartyChanged { .. } => {
                    *canonical_event_kinds.entry("party".into()).or_default() += 1;
                    continue;
                }
                CanonicalEvent::WorldChanged(_) => {
                    *canonical_event_kinds.entry("world".into()).or_default() += 1;
                    continue;
                }
                CanonicalEvent::Map(_) => {
                    *canonical_event_kinds.entry("map".into()).or_default() += 1;
                    continue;
                }
                CanonicalEvent::Dungeon(_) => {
                    *canonical_event_kinds.entry("dungeon".into()).or_default() += 1;
                    continue;
                }
                CanonicalEvent::Chat(_) => {
                    *canonical_event_kinds.entry("chat".into()).or_default() += 1;
                    continue;
                }
            };
            *canonical_event_kinds
                .entry(timeline_event_name(&timeline.kind).into())
                .or_default() += 1;
            match &timeline.kind {
                TimelineEventKind::Cast(cast) => {
                    observe_cast(&mut cast_identities, timeline.time.observed_micros, cast)
                }
                TimelineEventKind::Status(status) => {
                    observe_origin_effect(
                        &mut origin_effect_edges,
                        timeline.time.observed_micros,
                        status,
                    );
                    observe_status(
                        &mut active_statuses,
                        &mut status_lifecycles,
                        timeline.time.observed_micros,
                        status,
                    )
                }
                TimelineEventKind::Damage(damage) => {
                    observe_damage_identity(
                        &mut damage_identities,
                        timeline.time.observed_micros,
                        damage,
                    );
                    observe_damage(
                        &active_statuses,
                        &mut effect_damage_windows,
                        timeline.time.observed_micros,
                        damage,
                    )
                }
                _ => {}
            }
        }
    }
    expire_statuses(
        &mut active_statuses,
        &mut status_lifecycles,
        last_observed_micros,
    );

    let source_edges_by_effect = source_edges_by_effect(&overview.source_effect_edges);
    let static_damage_edges = overview
        .source_damage_edges
        .iter()
        .map(|edge| (edge.source_identity.clone(), edge.affected_damage_id))
        .collect::<BTreeSet<_>>();
    let effect_damage_windows = finish_windows(
        effect_damage_windows,
        &source_edges_by_effect,
        &static_damage_edges,
    );
    let cast_identities = finish_casts(cast_identities);
    let exact_origin_effect_edges =
        finish_origin_effects(origin_effect_edges, &source_edges_by_effect);
    let damage_identities = finish_damage_identities(damage_identities);
    let proof_queue = build_proof_queue(&effect_damage_windows)?;
    let coverage = build_coverage(
        &cast_identities,
        &status_lifecycles,
        &exact_origin_effect_edges,
        &damage_identities,
        &effect_damage_windows,
        proof_queue.len(),
    );
    let report = AuditReport {
        schema_version: REPORT_SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-influence-journal-audit",
        authority: "research-only-cooccurrence-not-rdps-attribution",
        capture_id: session.capture_id,
        deployment_id: session.game_build.deployment_id,
        region_id: session.game_build.region_id,
        channel: session.game_build.channel,
        game_build: session.game_build.build_id,
        protocol_pack_id: pack.definition().pack_id.clone(),
        protocol_pack_digest: pack.digest().to_owned(),
        journal_record_count: stream.record_count(),
        capture_gap_count,
        decoded_event_count,
        decode_statuses,
        canonical_event_kinds,
        cast_identities,
        status_lifecycles,
        exact_origin_effect_edges,
        damage_identities,
        effect_damage_windows,
        coverage,
        proof_queue,
        active_statuses_at_end: active_statuses.len() as u64,
        policy: AuditPolicy {
            exact_build_required: true,
            exact_protocol_digest_required: true,
            retained_services: vec!["World", "WorldNtf"],
            account_and_login_services_retained: false,
            effect_damage_window_is_attribution_proof: false,
            all_decoded_casts_inventoried: true,
            all_exact_status_origins_inventoried: true,
            all_decoded_damage_identities_inventoried: true,
            proof_queue_may_promote_runtime_attribution: false,
            unresolved_relationships_retained: true,
            missing_amounts_default_to_zero: false,
        },
    };

    if let Some(parent) = arguments.output.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut output = BufWriter::new(File::create(&arguments.output)?);
    serde_json::to_writer_pretty(&mut output, &report)?;
    output.write_all(b"\n")?;
    output.flush()?;
    println!(
        "audited {} journal records into {} effect/damage windows (research only)",
        report.journal_record_count,
        report.effect_damage_windows.len()
    );
    println!("wrote {}", arguments.output.display());
    Ok(())
}

fn validate_overview(overview: &RelationshipOverview) -> Result<(), Box<dyn Error>> {
    if overview.schema_version != 4
        || overview.policy.unresolved_evidence_hidden
        || !overview.policy.static_relationships_are_not_runtime_amounts
        || !overview
            .policy
            .matching_build_counterfactual_required_for_attributed_damage
    {
        return Err("relationship overview has an unsafe or unsupported contract".into());
    }
    Ok(())
}

fn validate_inputs(
    pack: &ProtocolPack,
    session: &rlogs_game_bpsr::CaptureSession,
    overview: &RelationshipOverview,
) -> Result<BTreeSet<u64>, Box<dyn Error>> {
    if !pack.matches(&session.game_build) {
        return Err("protocol pack does not exactly match the journal build".into());
    }
    if overview.static_game_build != session.game_build.build_id {
        return Err("relationship overview does not match the journal build".into());
    }
    if session.protocol_pack_digest.as_deref() != Some(pack.digest()) {
        return Err("journal protocol digest does not exactly match the selected pack".into());
    }
    let allowed_service_ids = pack
        .definition()
        .routes
        .iter()
        .filter(|route| matches!(route.service_name.as_str(), "World" | "WorldNtf"))
        .map(|route| route.route.service_id)
        .collect::<BTreeSet<_>>();
    if allowed_service_ids.is_empty() {
        return Err("selected pack has no World/WorldNtf research services".into());
    }
    Ok(allowed_service_ids)
}

fn validate_research_record(
    record: &rlogs_game_bpsr::CaptureRecord,
    allowed_service_ids: &BTreeSet<u64>,
) -> Result<(), Box<dyn Error>> {
    let CaptureRecordKind::Packet(packet) = &record.kind else {
        return Ok(());
    };
    let Some(route) = packet.route else {
        return Err(format!(
            "research journal record {} contains an unrouted packet",
            record.sequence
        )
        .into());
    };
    if !allowed_service_ids.contains(&route.key.service_id) {
        return Err(format!(
            "research journal record {} contains disallowed service {}",
            record.sequence, route.key.service_id
        )
        .into());
    }
    Ok(())
}

fn source_edges_by_effect(edges: &[SourceEffectEdge]) -> BTreeMap<i64, Vec<SourceEffectEdge>> {
    let mut result = BTreeMap::<i64, Vec<SourceEffectEdge>>::new();
    for edge in edges {
        result.entry(edge.effect_id).or_default().push(edge.clone());
    }
    for candidates in result.values_mut() {
        candidates.sort_by(|left, right| left.source_identity.cmp(&right.source_identity));
        candidates.dedup_by(|left, right| {
            left.source_identity == right.source_identity && left.relationship == right.relationship
        });
    }
    result
}

fn observe_cast(
    casts: &mut BTreeMap<CastIdentityKey, ObservationAccumulator>,
    observed_micros: u64,
    cast: &CastEvent,
) {
    let key = CastIdentityKey {
        source_actor_id: cast.source.actor_id.0,
        ability_id: cast.ability.0,
        state: cast_state_name(cast.state),
        action_instance_id: cast
            .action_timing
            .as_ref()
            .map(|timing| timing.action_instance_id),
        base_ability_id: cast
            .action_timing
            .as_ref()
            .map(|timing| timing.base_ability.0),
        slot_id: cast.action_timing.as_ref().map(|timing| timing.slot_id),
    };
    observe_occurrence(casts.entry(key).or_default(), observed_micros);
}

fn observe_origin_effect(
    edges: &mut BTreeMap<OriginEffectKey, OriginEffectAccumulator>,
    observed_micros: u64,
    status: &StatusEvent,
) {
    let Some(origin) = status.origin else {
        return;
    };
    let key = OriginEffectKey {
        source_type_id: origin.source_type_id,
        source_config_id: origin.source_config_id,
        effect_id: status.effect.0,
        provider_actor_id: status.source.as_ref().map(|source| source.actor_id.0),
        recipient_actor_id: status.target.actor_id.0,
    };
    let accumulator = edges.entry(key).or_default();
    if accumulator.first_observed_micros == 0 {
        accumulator.first_observed_micros = observed_micros;
    }
    accumulator.last_observed_micros = observed_micros;
    match status.state {
        StatusState::Applied => accumulator.applied += 1,
        StatusState::Refreshed => accumulator.refreshed += 1,
        StatusState::Stacked => accumulator.stacked += 1,
        StatusState::Consumed => accumulator.consumed += 1,
        StatusState::Removed => accumulator.removed += 1,
    }
    if let Some(instance_id) = status.instance_id {
        accumulator.instance_ids.insert(instance_id.0);
    }
    if let Some(stacks) = status.stacks {
        accumulator.stack_counts.insert(stacks);
    }
}

fn observe_occurrence(accumulator: &mut ObservationAccumulator, observed_micros: u64) {
    if accumulator.event_count == 0 {
        accumulator.first_observed_micros = observed_micros;
    }
    accumulator.last_observed_micros = observed_micros;
    accumulator.event_count = accumulator.event_count.saturating_add(1);
}

fn observe_damage_identity(
    identities: &mut BTreeMap<DamageIdentityKey, DamageIdentityAccumulator>,
    observed_micros: u64,
    damage: &DamageEvent,
) {
    let key = DamageIdentityKey {
        affected_damage_id: damage.ability.map(|value| value.0),
        packet_owner_id: damage.packet.owner_id,
        hit_event_id: damage.hit_event_id,
        damage_source: damage.damage_source,
        damage_type: damage.damage_type,
        owner_level: damage.packet.owner_level,
        owner_stage: damage.packet.owner_stage,
        skill_effect_uuid: damage.packet.skill_effect_uuid,
    };
    let accumulator = identities.entry(key).or_default();
    if accumulator.event_count == 0 {
        accumulator.first_observed_micros = observed_micros;
    }
    accumulator.last_observed_micros = observed_micros;
    accumulator.event_count = accumulator.event_count.saturating_add(1);
    accumulator.observed_damage += i128::from(damage.amount);
    if let Some(amount) = damage.actual_amount {
        accumulator.actual_amount += i128::from(amount);
        accumulator.actual_amount_count = accumulator.actual_amount_count.saturating_add(1);
    }
    if let Some(amount) = damage.hp_loss {
        accumulator.hp_loss += i128::from(amount);
        accumulator.hp_loss_count = accumulator.hp_loss_count.saturating_add(1);
    }
    if let Some(amount) = damage.shield_loss {
        accumulator.shield_loss += i128::from(amount);
        accumulator.shield_loss_count = accumulator.shield_loss_count.saturating_add(1);
    }
    accumulator
        .attacker_actor_ids
        .insert(damage.source.actor_id.0);
    if let Some(source) = &damage.direct_source {
        accumulator
            .direct_attacker_actor_ids
            .insert(source.actor_id.0);
    }
    accumulator
        .target_actor_ids
        .insert(damage.target.actor_id.0);
}

fn finish_casts(
    casts: BTreeMap<CastIdentityKey, ObservationAccumulator>,
) -> Vec<CastIdentitySummary> {
    casts
        .into_iter()
        .map(|(key, accumulator)| CastIdentitySummary {
            source_actor_id: key.source_actor_id.to_string(),
            ability_id: key.ability_id,
            state: key.state,
            action_instance_id: key.action_instance_id.map(|value| value.to_string()),
            base_ability_id: key.base_ability_id,
            slot_id: key.slot_id,
            first_observed_micros: accumulator.first_observed_micros,
            last_observed_micros: accumulator.last_observed_micros,
            event_count: accumulator.event_count,
        })
        .collect()
}

fn finish_origin_effects(
    edges: BTreeMap<OriginEffectKey, OriginEffectAccumulator>,
    source_edges_by_effect: &BTreeMap<i64, Vec<SourceEffectEdge>>,
) -> Vec<OriginEffectSummary> {
    edges
        .into_iter()
        .map(|(key, accumulator)| OriginEffectSummary {
            source_type_id: key.source_type_id,
            source_config_id: key.source_config_id,
            effect_id: key.effect_id,
            provider_actor_id: key.provider_actor_id.map(|value| value.to_string()),
            recipient_actor_id: key.recipient_actor_id.to_string(),
            first_observed_micros: accumulator.first_observed_micros,
            last_observed_micros: accumulator.last_observed_micros,
            applied: accumulator.applied,
            refreshed: accumulator.refreshed,
            stacked: accumulator.stacked,
            consumed: accumulator.consumed,
            removed: accumulator.removed,
            instance_ids: values_to_strings(accumulator.instance_ids),
            stack_counts: accumulator.stack_counts.into_iter().collect(),
            static_source_candidates: source_edges_by_effect
                .get(&key.effect_id)
                .cloned()
                .unwrap_or_default(),
            proof_state: "exact-packet-origin-effect-edge-not-yet-formula-attribution",
        })
        .collect()
}

fn finish_damage_identities(
    identities: BTreeMap<DamageIdentityKey, DamageIdentityAccumulator>,
) -> Vec<DamageIdentitySummary> {
    identities
        .into_iter()
        .map(|(key, accumulator)| DamageIdentitySummary {
            affected_damage_id: key.affected_damage_id,
            packet_owner_id: key.packet_owner_id,
            hit_event_id: key.hit_event_id,
            damage_source: key.damage_source,
            damage_type: key.damage_type,
            owner_level: key.owner_level,
            owner_stage: key.owner_stage,
            skill_effect_uuid: key.skill_effect_uuid.map(|value| value.to_string()),
            first_observed_micros: accumulator.first_observed_micros,
            last_observed_micros: accumulator.last_observed_micros,
            event_count: accumulator.event_count,
            observed_damage: accumulator.observed_damage.to_string(),
            actual_amount: (accumulator.actual_amount_count > 0)
                .then(|| accumulator.actual_amount.to_string()),
            hp_loss: (accumulator.hp_loss_count > 0).then(|| accumulator.hp_loss.to_string()),
            shield_loss: (accumulator.shield_loss_count > 0)
                .then(|| accumulator.shield_loss.to_string()),
            attacker_actor_ids: values_to_strings(accumulator.attacker_actor_ids),
            direct_attacker_actor_ids: values_to_strings(accumulator.direct_attacker_actor_ids),
            target_actor_ids: values_to_strings(accumulator.target_actor_ids),
            proof_state: "exact-packet-damage-identity-not-yet-counterfactual-attribution",
        })
        .collect()
}

fn values_to_strings<T>(values: BTreeSet<T>) -> Vec<String>
where
    T: ToString,
{
    values.into_iter().map(|value| value.to_string()).collect()
}

fn cast_state_name(state: CastState) -> &'static str {
    match state {
        CastState::Started => "started",
        CastState::Completed => "completed",
        CastState::Interrupted => "interrupted",
        CastState::Cancelled => "cancelled",
    }
}

fn observe_status(
    active: &mut BTreeMap<ActiveStatusKey, ActiveStatus>,
    summaries: &mut BTreeMap<i64, StatusLifecycleSummary>,
    observed_micros: u64,
    status: &StatusEvent,
) {
    let summary = summaries.entry(status.effect.0).or_default();
    if summary.first_observed_micros == 0 {
        summary.first_observed_micros = observed_micros;
    }
    summary.last_observed_micros = observed_micros;
    let provider_actor_id = status.source.as_ref().map(|source| source.actor_id.0);
    if let Some(provider) = provider_actor_id {
        summary.provider_actor_ids.insert(provider.to_string());
    }
    summary
        .recipient_actor_ids
        .insert(status.target.actor_id.0.to_string());
    if let Some(instance) = status.instance_id {
        summary.instance_ids.insert(instance.0.to_string());
    }
    if let Some(origin) = status.origin {
        summary.origin_pairs.insert(format!(
            "{}:{}",
            origin.source_type_id, origin.source_config_id
        ));
    }
    if let Some(stacks) = status.stacks {
        summary.stack_counts.insert(stacks);
    }

    let key = ActiveStatusKey {
        target_actor_id: status.target.actor_id.0,
        effect_id: status.effect.0,
        instance_id: status.instance_id.map(|value| value.0),
        provider_actor_id,
    };
    match status.state {
        StatusState::Applied | StatusState::Refreshed | StatusState::Stacked => {
            match status.state {
                StatusState::Applied => summary.applied += 1,
                StatusState::Refreshed => summary.refreshed += 1,
                StatusState::Stacked => summary.stacked += 1,
                _ => unreachable!(),
            }
            let origin = status.origin;
            active
                .entry(key.clone())
                .and_modify(|current| {
                    current.last_observed_micros = observed_micros;
                    current.expires_micros =
                        nominal_expiry(observed_micros, status.duration_millis)
                            .or(current.expires_micros);
                    current.stacks = status.stacks.or(current.stacks);
                    if let Some(origin) = origin {
                        current.origin_source_type_id = Some(origin.source_type_id);
                        current.origin_source_config_id = Some(origin.source_config_id);
                    }
                })
                .or_insert(ActiveStatus {
                    key,
                    origin_source_type_id: origin.map(|value| value.source_type_id),
                    origin_source_config_id: origin.map(|value| value.source_config_id),
                    applied_micros: observed_micros,
                    last_observed_micros: observed_micros,
                    expires_micros: nominal_expiry(observed_micros, status.duration_millis),
                    stacks: status.stacks,
                });
        }
        StatusState::Consumed | StatusState::Removed => {
            match status.state {
                StatusState::Consumed => summary.consumed += 1,
                StatusState::Removed => summary.removed += 1,
                _ => unreachable!(),
            }
            active.retain(|candidate, _| {
                if candidate.target_actor_id != key.target_actor_id
                    || candidate.effect_id != key.effect_id
                {
                    return true;
                }
                if let Some(instance_id) = key.instance_id {
                    candidate.instance_id != Some(instance_id)
                } else if let Some(provider) = key.provider_actor_id {
                    candidate.provider_actor_id != Some(provider)
                } else {
                    false
                }
            });
        }
    }
}

fn nominal_expiry(observed_micros: u64, duration_millis: Option<u64>) -> Option<u64> {
    duration_millis
        .filter(|duration| *duration > 0)
        .map(|duration| observed_micros.saturating_add(duration.saturating_mul(1_000)))
}

fn expire_statuses(
    active: &mut BTreeMap<ActiveStatusKey, ActiveStatus>,
    summaries: &mut BTreeMap<i64, StatusLifecycleSummary>,
    observed_micros: u64,
) {
    let expired = active
        .iter()
        .filter_map(|(key, status)| {
            status
                .expires_micros
                .filter(|expires| observed_micros >= *expires)
                .map(|_| key.clone())
        })
        .collect::<Vec<_>>();
    for key in expired {
        active.remove(&key);
        summaries
            .entry(key.effect_id)
            .or_default()
            .nominally_expired += 1;
    }
}

fn observe_damage(
    active: &BTreeMap<ActiveStatusKey, ActiveStatus>,
    windows: &mut BTreeMap<EffectDamageWindowKey, EffectDamageWindowAccumulator>,
    observed_micros: u64,
    damage: &DamageEvent,
) {
    for status in active.values() {
        let placement = if status.key.target_actor_id == damage.source.actor_id.0 {
            "attacker_active_status"
        } else if status.key.target_actor_id == damage.target.actor_id.0 {
            "target_active_status"
        } else {
            continue;
        };
        let key = EffectDamageWindowKey {
            placement,
            effect_id: status.key.effect_id,
            provider_actor_id: status.key.provider_actor_id,
            recipient_actor_id: status.key.target_actor_id,
            affected_damage_id: damage.ability.map(|value| value.0),
            packet_owner_id: damage.packet.owner_id,
            hit_event_id: damage.hit_event_id,
            damage_source: damage.damage_source,
            damage_type: damage.damage_type,
            attacker_actor_id: damage.source.actor_id.0,
            target_actor_id: damage.target.actor_id.0,
        };
        let accumulator = windows.entry(key).or_default();
        if accumulator.damage_event_count == 0 {
            accumulator.first_observed_micros = observed_micros.max(status.applied_micros);
        }
        accumulator.last_observed_micros = observed_micros;
        accumulator.damage_event_count = accumulator.damage_event_count.saturating_add(1);
        accumulator.observed_damage += i128::from(damage.amount);
        if let Some(instance_id) = status.key.instance_id {
            accumulator.status_instance_ids.insert(instance_id);
        }
        if let (Some(source_type), Some(config_id)) =
            (status.origin_source_type_id, status.origin_source_config_id)
        {
            accumulator.origin_pairs.insert((source_type, config_id));
        }
        if let Some(stacks) = status.stacks {
            accumulator.observed_stack_counts.insert(stacks);
        }
    }
}

fn finish_windows(
    windows: BTreeMap<EffectDamageWindowKey, EffectDamageWindowAccumulator>,
    source_edges_by_effect: &BTreeMap<i64, Vec<SourceEffectEdge>>,
    static_damage_edges: &BTreeSet<(String, i64)>,
) -> Vec<EffectDamageWindowSummary> {
    windows
        .into_iter()
        .map(|(key, accumulator)| {
            let source_candidates = source_edges_by_effect
                .get(&key.effect_id)
                .into_iter()
                .flatten()
                .map(|edge| SourceCandidate {
                    source_identity: edge.source_identity.clone(),
                    relationship: edge.relationship.clone(),
                    static_proof_state: edge.proof_state.clone(),
                    damage_target_catalogued: key.affected_damage_id.is_some_and(|damage_id| {
                        static_damage_edges.contains(&(edge.source_identity.clone(), damage_id))
                    }),
                })
                .collect();
            EffectDamageWindowSummary {
                placement: key.placement,
                effect_id: key.effect_id,
                provider_actor_id: key.provider_actor_id.map(|value| value.to_string()),
                recipient_actor_id: key.recipient_actor_id.to_string(),
                affected_damage_id: key.affected_damage_id,
                packet_owner_id: key.packet_owner_id,
                hit_event_id: key.hit_event_id,
                damage_source: key.damage_source,
                damage_type: key.damage_type,
                attacker_actor_id: key.attacker_actor_id.to_string(),
                target_actor_id: key.target_actor_id.to_string(),
                first_observed_micros: accumulator.first_observed_micros,
                last_observed_micros: accumulator.last_observed_micros,
                damage_event_count: accumulator.damage_event_count,
                observed_damage: accumulator.observed_damage.to_string(),
                status_instance_ids: accumulator
                    .status_instance_ids
                    .into_iter()
                    .map(|value| value.to_string())
                    .collect(),
                origin_pairs: accumulator
                    .origin_pairs
                    .into_iter()
                    .map(|(source_type, config_id)| format!("{source_type}:{config_id}"))
                    .collect(),
                observed_stack_counts: accumulator.observed_stack_counts.into_iter().collect(),
                source_candidates,
                proof_state: "packet-cooccurrence-only-not-attribution-proof",
                attributed_damage_delta: None,
                blockers: vec![
                    "source ownership and recipient scope require exact origin review",
                    "cooccurrence does not prove the formula stage or marginal damage",
                    "provider-removed counterfactual and conservation replay are required",
                ],
            }
        })
        .collect()
}

fn build_proof_queue(
    windows: &[EffectDamageWindowSummary],
) -> Result<Vec<ProofQueueEntry>, Box<dyn Error>> {
    let mut grouped = BTreeMap::<ProofQueueKey, ProofQueueAccumulator>::new();
    for window in windows {
        let key = ProofQueueKey {
            placement: window.placement,
            effect_id: window.effect_id,
            affected_damage_id: window.affected_damage_id,
        };
        let accumulator = grouped.entry(key).or_default();
        accumulator.packet_window_rows = accumulator.packet_window_rows.saturating_add(1);
        accumulator.damage_event_count = accumulator
            .damage_event_count
            .saturating_add(window.damage_event_count);
        accumulator.observed_damage = accumulator
            .observed_damage
            .checked_add(window.observed_damage.parse::<i128>()?)
            .ok_or("proof queue observed-damage sum overflowed i128")?;
        if let Some(provider) = &window.provider_actor_id {
            accumulator.provider_actor_ids.insert(provider.clone());
        }
        accumulator
            .recipient_actor_ids
            .insert(window.recipient_actor_id.clone());
        accumulator
            .attacker_actor_ids
            .insert(window.attacker_actor_id.clone());
        accumulator
            .target_actor_ids
            .insert(window.target_actor_id.clone());
        accumulator
            .origin_pairs
            .extend(window.origin_pairs.iter().cloned());
        for candidate in &window.source_candidates {
            accumulator
                .source_candidates
                .insert(candidate.source_identity.clone());
            if candidate.damage_target_catalogued {
                accumulator
                    .catalogued_source_candidates
                    .insert(candidate.source_identity.clone());
            }
        }
    }

    Ok(grouped
        .into_iter()
        .map(|(key, accumulator)| {
            let mut blockers = Vec::new();
            if accumulator.origin_pairs.is_empty() {
                blockers.push("exact packet origin pair is absent");
            }
            match accumulator.source_candidates.len() {
                0 => blockers.push("no static source candidate matches the packet effect"),
                1 => {}
                _ => blockers.push("multiple static source candidates match the packet effect"),
            }
            if accumulator.catalogued_source_candidates.is_empty() {
                blockers.push("no matching static source-to-damage edge is catalogued");
            }
            blockers.push("provider-removed counterfactual is not proven");
            blockers.push("party-damage conservation is not proven");
            ProofQueueEntry {
                placement: key.placement,
                effect_id: key.effect_id,
                affected_damage_id: key.affected_damage_id,
                packet_window_rows: accumulator.packet_window_rows,
                damage_event_count: accumulator.damage_event_count,
                observed_damage: accumulator.observed_damage.to_string(),
                provider_actor_ids: accumulator.provider_actor_ids.into_iter().collect(),
                recipient_actor_ids: accumulator.recipient_actor_ids.into_iter().collect(),
                attacker_actor_ids: accumulator.attacker_actor_ids.into_iter().collect(),
                target_actor_ids: accumulator.target_actor_ids.into_iter().collect(),
                origin_pairs: accumulator.origin_pairs.into_iter().collect(),
                source_candidates: accumulator.source_candidates.into_iter().collect(),
                catalogued_source_candidates: accumulator
                    .catalogued_source_candidates
                    .into_iter()
                    .collect(),
                promotion_eligible: false,
                blockers,
            }
        })
        .collect())
}

fn build_coverage(
    casts: &[CastIdentitySummary],
    status_lifecycles: &BTreeMap<i64, StatusLifecycleSummary>,
    origin_effect_edges: &[OriginEffectSummary],
    damage_identities: &[DamageIdentitySummary],
    windows: &[EffectDamageWindowSummary],
    proof_queue_row_count: usize,
) -> AuditCoverage {
    let unique_affected_damage_ids = damage_identities
        .iter()
        .filter_map(|identity| identity.affected_damage_id)
        .collect::<BTreeSet<_>>()
        .len();
    AuditCoverage {
        cast_identity_count: casts.len(),
        exact_origin_effect_edge_count: origin_effect_edges.len(),
        damage_identity_count: damage_identities.len(),
        effect_damage_window_count: windows.len(),
        unique_packet_effect_ids: status_lifecycles.len(),
        unique_affected_damage_ids,
        origin_effect_edges_without_static_source_candidate: origin_effect_edges
            .iter()
            .filter(|edge| edge.static_source_candidates.is_empty())
            .count(),
        effect_damage_windows_without_static_source_candidate: windows
            .iter()
            .filter(|window| window.source_candidates.is_empty())
            .count(),
        effect_damage_windows_with_ambiguous_static_source_candidates: windows
            .iter()
            .filter(|window| window.source_candidates.len() > 1)
            .count(),
        effect_damage_windows_without_matching_source_damage_edge: windows
            .iter()
            .filter(|window| {
                !window
                    .source_candidates
                    .iter()
                    .any(|candidate| candidate.damage_target_catalogued)
            })
            .count(),
        proof_queue_row_count,
    }
}

fn decode_status_name(status: ProtocolDecodeStatus) -> &'static str {
    match status {
        ProtocolDecodeStatus::Decoded => "decoded",
        ProtocolDecodeStatus::CaptureGap => "capture_gap",
        ProtocolDecodeStatus::Unrouted => "unrouted",
        ProtocolDecodeStatus::OpaqueLocalOnly => "opaque_local_only",
        ProtocolDecodeStatus::Prohibited(_) => "prohibited",
        ProtocolDecodeStatus::MissingApplicationPayload => "missing_application_payload",
        ProtocolDecodeStatus::DecodeFailed => "decode_failed",
    }
}

fn timeline_event_name(event: &TimelineEventKind) -> &'static str {
    match event {
        TimelineEventKind::RunBoundary { .. } => "run_boundary",
        TimelineEventKind::EncounterBoundary { .. } => "encounter_boundary",
        TimelineEventKind::CombatBoundary { .. } => "combat_boundary",
        TimelineEventKind::Actor(_) => "actor",
        TimelineEventKind::EntityAttributes(_) => "entity_attributes",
        TimelineEventKind::TemporaryAttributes(_) => "temporary_attributes",
        TimelineEventKind::Cast(_) => "cast",
        TimelineEventKind::Cooldown(_) => "cooldown",
        TimelineEventKind::Resource(_) => "resource",
        TimelineEventKind::Damage(_) => "damage",
        TimelineEventKind::Healing(_) => "healing",
        TimelineEventKind::Shield(_) => "shield",
        TimelineEventKind::Life { .. } => "life",
        TimelineEventKind::Status(_) => "status",
        TimelineEventKind::UnresolvedStatus(_) => "unresolved_status",
        TimelineEventKind::UnresolvedAction(_) => "unresolved_action",
        TimelineEventKind::Position(_) => "position",
        TimelineEventKind::RecorderPause(_) => "recorder_pause",
        TimelineEventKind::DataGap(_) => "data_gap",
    }
}

fn arguments() -> Result<Arguments, Box<dyn Error>> {
    let mut pack = None;
    let mut journal = None;
    let mut relationship_overview = None;
    let mut output = None;
    let mut values = env::args_os().skip(1);
    while let Some(argument) = values.next() {
        match argument.to_string_lossy().as_ref() {
            "--pack" => pack = values.next().map(PathBuf::from),
            "--journal" => journal = values.next().map(PathBuf::from),
            "--relationship-overview" => relationship_overview = values.next().map(PathBuf::from),
            "--output" => output = values.next().map(PathBuf::from),
            unknown => return Err(format!("unknown argument {unknown}").into()),
        }
    }
    Ok(Arguments {
        pack: pack.ok_or("--pack is required")?,
        journal: journal.ok_or("--journal is required")?,
        relationship_overview: relationship_overview
            .ok_or("--relationship-overview is required")?,
        output: output.ok_or("--output is required")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rlogs_events::{
        AbilityId, ActionTimingSnapshot, ActorId, EntityRef, EntityUuid, StatusEffectId,
        StatusEffectInstanceId,
    };
    use rlogs_game_bpsr::{
        CaptureAdapter, CaptureRecord, CompressionState, FragmentKind, GameBuild, PacketDirection,
        PacketEnvelope, PacketPayload, RouteKey, RoutedMessage,
    };

    fn entity(actor_id: u64, uuid: i64) -> EntityRef {
        EntityRef {
            actor_id: ActorId(actor_id),
            entity_uuid: EntityUuid(uuid),
        }
    }

    fn overview(build: &str) -> RelationshipOverview {
        RelationshipOverview {
            schema_version: 4,
            static_game_build: build.into(),
            policy: RelationshipPolicy {
                unresolved_evidence_hidden: false,
                static_relationships_are_not_runtime_amounts: true,
                matching_build_counterfactual_required_for_attributed_damage: true,
            },
            source_effect_edges: vec![],
            source_damage_edges: vec![],
        }
    }

    fn packet_record(service_id: u64) -> CaptureRecord {
        CaptureRecord {
            sequence: 1,
            observed_micros: 1,
            wall_clock_unix_micros: None,
            kind: CaptureRecordKind::Packet(PacketEnvelope {
                connection_id: 1,
                stream_id: 1,
                source: None,
                destination: None,
                direction: PacketDirection::ServerToClient,
                fragment: Some(FragmentKind::Notify),
                route: Some(RoutedMessage {
                    key: RouteKey::new(
                        PacketDirection::ServerToClient,
                        FragmentKind::Notify,
                        service_id,
                        1,
                    ),
                    stub_id: 0,
                    call_id: None,
                }),
                compression: CompressionState::NotCompressed,
                payload: PacketPayload {
                    wire_bytes: vec![],
                    application_bytes: Some(vec![]),
                },
            }),
        }
    }

    fn window_summary(
        observed_damage: &str,
        origin_pairs: Vec<String>,
        source_candidates: Vec<SourceCandidate>,
    ) -> EffectDamageWindowSummary {
        EffectDamageWindowSummary {
            placement: "attacker_active_status",
            effect_id: 9001,
            provider_actor_id: Some("7".into()),
            recipient_actor_id: "8".into(),
            affected_damage_id: Some(777),
            packet_owner_id: Some(777),
            hit_event_id: Some(4),
            damage_source: Some(1),
            damage_type: Some(2),
            attacker_actor_id: "8".into(),
            target_actor_id: "9".into(),
            first_observed_micros: 100,
            last_observed_micros: 120,
            damage_event_count: 1,
            observed_damage: observed_damage.into(),
            status_instance_ids: vec!["44".into()],
            origin_pairs,
            observed_stack_counts: vec![3],
            source_candidates,
            proof_state: "packet-cooccurrence-only-not-attribution-proof",
            attributed_damage_delta: None,
            blockers: vec![],
        }
    }

    #[test]
    fn inputs_require_exact_build_and_protocol_digest() {
        let pack = ProtocolPack::from_json(include_bytes!(
            "../protocol-packs/global/steam-24687926/pack.json"
        ))
        .unwrap();
        let target = &pack.definition().target;
        let mut session = rlogs_game_bpsr::CaptureSession {
            format_version: 1,
            capture_id: "fixture".into(),
            started_unix_micros: None,
            game_build: GameBuild {
                deployment_id: target.deployment_id.clone(),
                region_id: target.region_id.clone(),
                channel: target.channel.clone(),
                build_id: target.build_id.clone(),
                executable_version: target.executable_version.clone(),
            },
            adapter: CaptureAdapter {
                name: "fixture".into(),
                version: None,
            },
            protocol_pack_digest: Some(pack.digest().into()),
        };
        let exact_overview = overview(&target.build_id);
        assert!(
            !validate_inputs(&pack, &session, &exact_overview)
                .unwrap()
                .is_empty()
        );

        session.game_build.build_id = "different-build".into();
        assert!(validate_inputs(&pack, &session, &exact_overview).is_err());
        session.game_build.build_id = target.build_id.clone();
        session.protocol_pack_digest = Some("sha256:different".into());
        assert!(validate_inputs(&pack, &session, &exact_overview).is_err());
    }

    #[test]
    fn research_record_gate_rejects_unrouted_and_non_world_services() {
        let allowed = BTreeSet::from([10]);
        assert!(validate_research_record(&packet_record(10), &allowed).is_ok());
        assert!(validate_research_record(&packet_record(11), &allowed).is_err());
        let mut unrouted = packet_record(10);
        let CaptureRecordKind::Packet(packet) = &mut unrouted.kind else {
            unreachable!()
        };
        packet.route = None;
        assert!(validate_research_record(&unrouted, &allowed).is_err());
    }

    #[test]
    fn status_window_retains_exact_origin_and_does_not_invent_attribution() {
        let mut active = BTreeMap::new();
        let mut lifecycles = BTreeMap::new();
        observe_status(
            &mut active,
            &mut lifecycles,
            100,
            &StatusEvent {
                source: Some(entity(7, 70)),
                target: entity(8, 80),
                effect: StatusEffectId(9001),
                instance_id: Some(StatusEffectInstanceId(44)),
                origin: Some(rlogs_events::StatusOrigin {
                    source_type_id: 2,
                    source_config_id: 123,
                }),
                state: StatusState::Applied,
                stacks: Some(3),
                duration_millis: Some(5_000),
                level: Some(1),
                part_id: None,
                count: None,
                created_at_millis: None,
            },
        );
        let mut windows = BTreeMap::new();
        observe_damage(
            &active,
            &mut windows,
            120,
            &DamageEvent {
                source: entity(8, 80),
                direct_source: None,
                target: entity(9, 90),
                ability: Some(rlogs_events::AbilityId(777)),
                amount: 10_000,
                actual_amount: None,
                hp_loss: None,
                shield_loss: None,
                hit_event_id: Some(4),
                damage_source: Some(1),
                damage_type: Some(2),
                flags: Default::default(),
                packet: Default::default(),
            },
        );
        let source_edges = BTreeMap::from([(
            9001,
            vec![SourceEffectEdge {
                source_identity: "skill:123".into(),
                effect_id: 9001,
                relationship: "applies".into(),
                proof_state: "static-table-edge".into(),
            }],
        )]);
        let static_damage_edges = BTreeSet::from([("skill:123".to_owned(), 777)]);
        let rows = finish_windows(windows, &source_edges, &static_damage_edges);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].provider_actor_id.as_deref(), Some("7"));
        assert_eq!(rows[0].recipient_actor_id, "8");
        assert_eq!(rows[0].affected_damage_id, Some(777));
        assert_eq!(rows[0].origin_pairs, ["2:123"]);
        assert_eq!(rows[0].observed_stack_counts, [3]);
        assert_eq!(rows[0].observed_damage, "10000");
        assert_eq!(rows[0].attributed_damage_delta, None);
        assert_eq!(
            rows[0].proof_state,
            "packet-cooccurrence-only-not-attribution-proof"
        );
        let queue = build_proof_queue(&rows).unwrap();
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].effect_id, 9001);
        assert_eq!(queue[0].affected_damage_id, Some(777));
        assert_eq!(queue[0].observed_damage, "10000");
        assert_eq!(queue[0].source_candidates, ["skill:123"]);
        assert_eq!(queue[0].catalogued_source_candidates, ["skill:123"]);
        assert!(!queue[0].promotion_eligible);
        assert_eq!(
            queue[0].blockers,
            [
                "provider-removed counterfactual is not proven",
                "party-damage conservation is not proven"
            ]
        );
    }

    #[test]
    fn proof_queue_groups_exact_totals_and_keeps_every_ambiguity_blocker() {
        let candidates = vec![
            SourceCandidate {
                source_identity: "skill:123".into(),
                relationship: "candidate".into(),
                static_proof_state: "static-only".into(),
                damage_target_catalogued: false,
            },
            SourceCandidate {
                source_identity: "talent:456".into(),
                relationship: "candidate".into(),
                static_proof_state: "static-only".into(),
                damage_target_catalogued: false,
            },
        ];
        let windows = vec![
            window_summary("10000", vec![], candidates.clone()),
            window_summary("25000", vec![], candidates),
        ];
        let queue = build_proof_queue(&windows).unwrap();
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].packet_window_rows, 2);
        assert_eq!(queue[0].damage_event_count, 2);
        assert_eq!(queue[0].observed_damage, "35000");
        assert_eq!(queue[0].source_candidates, ["skill:123", "talent:456"]);
        assert!(queue[0].catalogued_source_candidates.is_empty());
        assert_eq!(
            queue[0].blockers,
            [
                "exact packet origin pair is absent",
                "multiple static source candidates match the packet effect",
                "no matching static source-to-damage edge is catalogued",
                "provider-removed counterfactual is not proven",
                "party-damage conservation is not proven"
            ]
        );
    }

    #[test]
    fn cast_inventory_retains_action_instance_base_ability_and_slot() {
        let mut casts = BTreeMap::new();
        observe_cast(
            &mut casts,
            900,
            &CastEvent {
                source: entity(7, 70),
                ability: AbilityId(2203291),
                target: Some(entity(8, 80)),
                state: CastState::Started,
                action_timing: Some(ActionTimingSnapshot {
                    action_instance_id: 123456789,
                    base_ability: AbilityId(2233),
                    ability_level: 4,
                    slot_id: 2,
                    client_timestamp_raw: 50,
                    begin_time_raw: 40,
                    attack_speed_basis_points: 10_000,
                    cast_speed_basis_points: 10_000,
                    charge_speed_basis_points: 10_000,
                    passive: false,
                    activated_roulette: false,
                    target_part_id: 0,
                }),
            },
        );
        let rows = finish_casts(casts);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].source_actor_id, "7");
        assert_eq!(rows[0].ability_id, 2203291);
        assert_eq!(rows[0].state, "started");
        assert_eq!(rows[0].action_instance_id.as_deref(), Some("123456789"));
        assert_eq!(rows[0].base_ability_id, Some(2233));
        assert_eq!(rows[0].slot_id, Some(2));
    }

    #[test]
    fn exact_origin_effect_inventory_retains_packet_edge_and_static_candidates() {
        let mut edges = BTreeMap::new();
        observe_origin_effect(
            &mut edges,
            100,
            &StatusEvent {
                source: Some(entity(7, 70)),
                target: entity(8, 80),
                effect: StatusEffectId(9001),
                instance_id: Some(StatusEffectInstanceId(44)),
                origin: Some(rlogs_events::StatusOrigin {
                    source_type_id: 2,
                    source_config_id: 123,
                }),
                state: StatusState::Applied,
                stacks: Some(3),
                duration_millis: Some(5_000),
                level: Some(1),
                part_id: None,
                count: None,
                created_at_millis: None,
            },
        );
        let source_edges = BTreeMap::from([(
            9001,
            vec![SourceEffectEdge {
                source_identity: "skill:123".into(),
                effect_id: 9001,
                relationship: "applies".into(),
                proof_state: "static-table-edge".into(),
            }],
        )]);
        let rows = finish_origin_effects(edges, &source_edges);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].source_type_id, 2);
        assert_eq!(rows[0].source_config_id, 123);
        assert_eq!(rows[0].effect_id, 9001);
        assert_eq!(rows[0].provider_actor_id.as_deref(), Some("7"));
        assert_eq!(rows[0].recipient_actor_id, "8");
        assert_eq!(rows[0].instance_ids, ["44"]);
        assert_eq!(rows[0].stack_counts, [3]);
        assert_eq!(rows[0].static_source_candidates.len(), 1);
        assert_eq!(
            rows[0].proof_state,
            "exact-packet-origin-effect-edge-not-yet-formula-attribution"
        );
    }

    #[test]
    fn damage_inventory_keeps_damage_without_any_active_status() {
        let mut identities = BTreeMap::new();
        let mut packet = rlogs_events::DamagePacketDetail::default();
        packet.owner_id = Some(777);
        packet.owner_level = Some(60);
        packet.owner_stage = Some(5);
        packet.skill_effect_uuid = Some(987654321);
        observe_damage_identity(
            &mut identities,
            120,
            &DamageEvent {
                source: entity(8, 80),
                direct_source: Some(entity(7, 70)),
                target: entity(9, 90),
                ability: Some(AbilityId(777)),
                amount: 10_000,
                actual_amount: Some(9_000),
                hp_loss: Some(8_000),
                shield_loss: Some(1_000),
                hit_event_id: Some(4),
                damage_source: Some(1),
                damage_type: Some(2),
                flags: Default::default(),
                packet,
            },
        );
        let rows = finish_damage_identities(identities);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].affected_damage_id, Some(777));
        assert_eq!(rows[0].observed_damage, "10000");
        assert_eq!(rows[0].actual_amount.as_deref(), Some("9000"));
        assert_eq!(rows[0].hp_loss.as_deref(), Some("8000"));
        assert_eq!(rows[0].shield_loss.as_deref(), Some("1000"));
        assert_eq!(rows[0].attacker_actor_ids, ["8"]);
        assert_eq!(rows[0].direct_attacker_actor_ids, ["7"]);
        assert_eq!(rows[0].target_actor_ids, ["9"]);
        assert_eq!(
            rows[0].proof_state,
            "exact-packet-damage-identity-not-yet-counterfactual-attribution"
        );
    }

    #[test]
    fn removal_without_instance_closes_all_matching_unresolved_instances() {
        let mut active = BTreeMap::new();
        let mut lifecycles = BTreeMap::new();
        for instance in [1, 2] {
            observe_status(
                &mut active,
                &mut lifecycles,
                100,
                &StatusEvent {
                    source: Some(entity(7, 70)),
                    target: entity(8, 80),
                    effect: StatusEffectId(9001),
                    instance_id: Some(StatusEffectInstanceId(instance)),
                    origin: None,
                    state: StatusState::Applied,
                    stacks: None,
                    duration_millis: None,
                    level: None,
                    part_id: None,
                    count: None,
                    created_at_millis: None,
                },
            );
        }
        assert_eq!(active.len(), 2);
        observe_status(
            &mut active,
            &mut lifecycles,
            200,
            &StatusEvent {
                source: None,
                target: entity(8, 80),
                effect: StatusEffectId(9001),
                instance_id: None,
                origin: None,
                state: StatusState::Removed,
                stacks: None,
                duration_millis: None,
                level: None,
                part_id: None,
                count: None,
                created_at_millis: None,
            },
        );
        assert!(active.is_empty());
        assert_eq!(lifecycles[&9001].removed, 1);
    }

    #[test]
    fn packet_duration_closes_window_without_inventing_late_cooccurrence() {
        let mut active = BTreeMap::new();
        let mut lifecycles = BTreeMap::new();
        observe_status(
            &mut active,
            &mut lifecycles,
            100,
            &StatusEvent {
                source: Some(entity(7, 70)),
                target: entity(8, 80),
                effect: StatusEffectId(9001),
                instance_id: Some(StatusEffectInstanceId(1)),
                origin: None,
                state: StatusState::Applied,
                stacks: None,
                duration_millis: Some(5_000),
                level: None,
                part_id: None,
                count: None,
                created_at_millis: None,
            },
        );
        expire_statuses(&mut active, &mut lifecycles, 5_000_100);
        assert!(active.is_empty());
        assert_eq!(lifecycles[&9001].nominally_expired, 1);
    }
}
