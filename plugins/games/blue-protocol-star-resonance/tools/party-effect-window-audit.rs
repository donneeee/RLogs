use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    fs::{self, File},
    io::{BufRead, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{
    ActorEvent, ActorKind, ActorState, CanonicalEvent, DamageEvent, EncounterState,
    EntityAttributeEvent, EntityRef, PartyRosterEvent, PartyRosterObservation, RunState,
    StatusEvent, StatusState, TimelineEventKind,
};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 8;
const MIN_COHORT_RECEIPT_SCHEMA_VERSION: u16 = 18;
const MAX_COHORT_RECEIPT_SCHEMA_VERSION: u16 = 19;
const AUDIT_THREAD_STACK_BYTES: usize = 8 * 1024 * 1024;
const MAX_DAMAGE_ACTION_SAMPLES_PER_EDGE: usize = 4;
const TEAM_ATTRIBUTE_INTERPRETATION_BUILD: &str = "24687926";
const FIGHT_SOURCE_ENUM_BUILD: &str = "24687926";
// Exact names and values from the current-build Zproto.EAttrType enum in
// research/game-file-inventory/global/steam-24687926/rpc-message-surface.v2.json.
const ATTR_TEAM_ID: i32 = 194;
const ATTR_TEAM_MEMBER_NUMS: i32 = 195;

#[derive(Debug)]
struct Arguments {
    cohort_receipt: PathBuf,
    party_closure: PathBuf,
    output: PathBuf,
}

#[derive(Debug, Deserialize)]
struct CohortReceipt {
    schema_version: u16,
    generated_by: String,
    deployment_id: String,
    game_build: String,
    protocol_pack_digests: Vec<String>,
    rlogs: Vec<RlogReceipt>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RlogReceipt {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Clone, Debug, Default, Serialize)]
struct EffectSpec {
    effect_id: i64,
    exact_static_edge: bool,
    reviewed_candidate_edge: bool,
    source_skill_ids: BTreeSet<i64>,
    source_entry_ids: BTreeSet<i64>,
    support_categories: BTreeSet<String>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct WindowKey {
    effect_id: i64,
    affected_entity_actor_id: u64,
    instance_id: Option<i64>,
}

#[derive(Clone, Debug, Default, Serialize)]
struct DamageRelation {
    event_count: u64,
    amount: i128,
    ability_ids: BTreeSet<i64>,
    damage_source_actor_ids: BTreeSet<String>,
    damage_target_actor_ids: BTreeSet<String>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum DamageActionRole {
    EffectTargetIsDamageActor,
    EffectTargetIsDamageTarget,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct DamageActionKey {
    role: DamageActionRole,
    damage_source_actor_id: u64,
    damage_source_entity_uuid: i64,
    direct_damage_source_actor_id: Option<u64>,
    direct_damage_source_entity_uuid: Option<i64>,
    ability_id: Option<i64>,
    damage_target_actor_id: u64,
    damage_target_entity_uuid: i64,
}

#[derive(Clone, Debug, Serialize)]
struct DamageActionSample {
    sequence: u64,
    observed_micros: u64,
    amount: i64,
    actual_amount: Option<i64>,
    hit_event_id: Option<i32>,
    skill_effect_uuid: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct DamageActionEdge {
    role: DamageActionRole,
    damage_source_actor_id: String,
    damage_source_entity_uuid: String,
    direct_damage_source_actor_id: Option<String>,
    direct_damage_source_entity_uuid: Option<String>,
    ability_id: Option<i64>,
    damage_target_actor_id: String,
    damage_target_entity_uuid: String,
    event_count: u64,
    amount: i128,
    first_sequence: u64,
    last_sequence: u64,
    first_observed_micros: u64,
    last_observed_micros: u64,
    samples: Vec<DamageActionSample>,
    causal_attribution_authorized: bool,
    provider_rdps_credit_authorized: bool,
}

#[derive(Clone, Debug)]
struct ActorSnapshot {
    actor: EntityRef,
    kind: ActorKind,
    character_id: Option<String>,
    class_id: Option<i32>,
}

#[derive(Clone, Copy, Debug, Default)]
struct PartyScopeEvidence {
    both_in_observed_party_roster: bool,
    matching_last_observed_team_id: Option<u64>,
    mismatching_last_observed_team_ids: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
struct IdentityEvidence {
    source_status_events: u64,
    source_player_identity_events: u64,
    source_non_player_identity_events: u64,
    source_identity_unresolved_events: u64,
    affected_entity_status_events: u64,
    affected_entity_player_identity_events: u64,
    affected_entity_non_player_identity_events: u64,
    affected_entity_identity_unresolved_events: u64,
    self_source_affected_status_events: u64,
    external_source_affected_status_events: u64,
    external_status_events_with_both_player_identities: u64,
    external_status_events_with_unresolved_identity: u64,
    external_status_events_with_both_in_observed_party_roster: u64,
    external_status_events_with_roster_evidence_but_lifecycle_coverage_open: u64,
    external_status_events_with_matching_last_observed_team_id: u64,
    external_status_events_with_mismatching_last_observed_team_ids: u64,
    external_status_events_with_unresolved_last_observed_team_id: u64,
    external_status_events_with_team_id_evidence_but_protocol_coverage_open: u64,
    matching_last_observed_team_ids: BTreeSet<String>,
    party_membership_proven_status_events: u64,
    party_membership_unproven_status_events: u64,
    source_actor_kinds: BTreeSet<String>,
    affected_entity_actor_kinds: BTreeSet<String>,
    source_character_ids: BTreeSet<String>,
    affected_entity_character_ids: BTreeSet<String>,
    source_class_ids: BTreeSet<i32>,
    affected_entity_class_ids: BTreeSet<i32>,
}

#[derive(Clone, Debug, Serialize)]
struct WindowRecord {
    session_id: String,
    effect_id: i64,
    exact_static_edge: bool,
    reviewed_candidate_edge: bool,
    affected_entity_actor_id: String,
    affected_entity_uuid: String,
    effect_target_actor_id: String,
    effect_target_entity_uuid: String,
    instance_id: Option<String>,
    source_actor_ids: BTreeSet<String>,
    source_entity_uuids: BTreeSet<String>,
    missing_source_observed: bool,
    provider_conflict_observed: bool,
    origin_pairs: BTreeSet<(i32, i64)>,
    levels: BTreeSet<i32>,
    reported_duration_millis: BTreeSet<u64>,
    reported_stacks: BTreeSet<u32>,
    reported_counts: BTreeSet<i32>,
    start_sequence: u64,
    end_sequence: Option<u64>,
    start_observed_micros: u64,
    end_observed_micros: Option<u64>,
    close_reason: Option<String>,
    lifecycle_counts: BTreeMap<String, u64>,
    orphan_lifecycle_start: bool,
    identity_evidence: IdentityEvidence,
    affected_entity_damage_actions: DamageRelation,
    damage_actions_targeting_affected_entity: DamageRelation,
    damage_action_edges: Vec<DamageActionEdge>,
    #[serde(skip)]
    damage_action_edge_accumulator: BTreeMap<DamageActionKey, DamageActionEdge>,
    provider_rdps_credit_authorized: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct ObservedOriginEdge {
    source_type_id: i32,
    source_kind: &'static str,
    source_enum_name: Option<&'static str>,
    source_config_id: i64,
    child_effect_id: i64,
    observation_count: u64,
    exact_current_build_enum_identity: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
struct EffectAggregate {
    effect_id: i64,
    exact_static_edge: bool,
    reviewed_candidate_edge: bool,
    source_skill_ids: BTreeSet<i64>,
    source_entry_ids: BTreeSet<i64>,
    support_categories: BTreeSet<String>,
    status_events: u64,
    status_events_with_source: u64,
    status_events_without_source: u64,
    unique_source_actor_ids: BTreeSet<String>,
    unique_affected_entity_actor_ids: BTreeSet<String>,
    unique_origin_pairs: BTreeSet<(i32, i64)>,
    observed_origin_edges: Vec<ObservedOriginEdge>,
    #[serde(skip)]
    origin_observation_counts: BTreeMap<(i32, i64), u64>,
    reported_duration_millis: BTreeSet<u64>,
    reported_status_levels: BTreeSet<i32>,
    reported_stacks: BTreeSet<u32>,
    reported_counts: BTreeSet<i32>,
    lifecycle_counts: BTreeMap<String, u64>,
    windows_closed: u64,
    windows_open_at_log_end: u64,
    orphan_lifecycle_windows: u64,
    identity_evidence: IdentityEvidence,
    affected_entity_damage_actions: DamageRelation,
    damage_actions_targeting_affected_entity: DamageRelation,
    provider_rdps_credit_authorized: bool,
}

#[derive(Debug, Serialize)]
struct AuditReport {
    schema_version: u16,
    generated_by: &'static str,
    deployment_id: String,
    game_build: String,
    protocol_pack_digests: Vec<String>,
    policy: Policy,
    inputs: Inputs,
    summary: Summary,
    effects: Vec<EffectAggregate>,
    windows: Vec<WindowRecord>,
}

#[derive(Clone, Debug, Serialize)]
struct TeamAttributeObservationExample {
    session_id: String,
    sequence: u64,
    observed_micros: u64,
    actor_id: String,
    entity_uuid: String,
    raw_value: Vec<u8>,
}

#[derive(Debug, Serialize)]
struct Policy {
    exact_numeric_effect_ids_and_build_are_authoritative: bool,
    localized_names_are_runtime_keys: bool,
    remote_player_cast_packets_required: bool,
    remote_player_cast_packets_treated_as_zero: bool,
    remote_player_cast_packets_synthesized: bool,
    status_rows_without_provider_are_preserved: bool,
    actor_identity_is_event_time_canonical_evidence_only: bool,
    player_identity_is_party_membership_authority: bool,
    explicit_party_roster_evidence_consumed: bool,
    party_roster_lifecycle_route_coverage_proven: bool,
    exact_build_team_id_attribute_evidence_consumed: bool,
    team_attribute_interpretation_build: &'static str,
    team_id_attribute_id: i32,
    team_member_count_attribute_id: i32,
    team_attribute_protocol_event_coverage_proven: bool,
    matching_last_observed_team_ids_grant_party_membership_authority: bool,
    fight_source_enum_build: &'static str,
    fight_source_type_identity_exact_build_gated: bool,
    packet_origin_edges_are_skill_to_buff_edges: bool,
    packet_origin_edges_are_provider_ownership_authority: bool,
    packet_origin_edges_are_formula_authority: bool,
    damage_links_preserve_affected_entity_as_actor_and_as_target: bool,
    affected_entity_is_assumed_friendly: bool,
    affected_entity_is_assumed_enemy: bool,
    status_source_to_effect_target_lifecycle_is_preserved: bool,
    status_source_is_provider_ownership_authority: bool,
    effect_target_role_is_allegiance_neutral: bool,
    effect_target_damage_actor_and_damage_target_edges_are_separate: bool,
    damage_action_edges_preserve_actor_ability_and_target: bool,
    damage_action_edges_are_causal_or_formula_authority: bool,
    current_character_snapshots_substituted_into_older_runs: bool,
    timeline_presence_is_formula_authority: bool,
    provider_rdps_credit_authorized: bool,
    runtime_promotion_allowed: bool,
    ui_display_allowed: bool,
}

#[derive(Debug, Serialize)]
struct Inputs {
    cohort_receipt_path: String,
    cohort_receipt_bytes: u64,
    cohort_receipt_sha256: String,
    party_closure_path: String,
    party_closure_bytes: u64,
    party_closure_sha256: String,
    rlogs: Vec<RlogReceipt>,
}

#[derive(Debug, Default, Serialize)]
struct Summary {
    rlogs_verified: u64,
    canonical_events: u64,
    damage_events: u64,
    cast_events_observed: u64,
    remote_cast_rows_synthesized: u64,
    party_roster_full_snapshot_events: u64,
    party_roster_members_observed_events: u64,
    party_roster_member_left_events: u64,
    party_roster_dissolved_events: u64,
    team_id_attribute_events: u64,
    team_id_attribute_positive_values: u64,
    team_id_attribute_clear_values: u64,
    team_id_attribute_malformed_values: u64,
    team_id_attribute_malformed_examples: Vec<TeamAttributeObservationExample>,
    team_member_count_attribute_events: u64,
    party_effects_in_frontier: u64,
    party_effects_observed: u64,
    party_status_events: u64,
    party_status_events_without_source: u64,
    windows: u64,
    windows_with_affected_entity_damage_actions: u64,
    windows_with_damage_actions_targeting_affected_entity: u64,
    window_damage_action_edges: u64,
    window_damage_action_actor_edges: u64,
    window_damage_action_target_edges: u64,
    provider_rdps_credit_authorized_effects: u64,
}

#[derive(Debug)]
struct Tracker {
    specs: BTreeMap<i64, EffectSpec>,
    effects: BTreeMap<i64, EffectAggregate>,
    actors: BTreeMap<(u64, i64), ActorSnapshot>,
    observed_party_roster_character_ids: BTreeSet<String>,
    observed_party_roster_has_full_snapshot: bool,
    last_observed_team_ids: BTreeMap<(u64, i64), u64>,
    game_build: String,
    active: BTreeMap<WindowKey, WindowRecord>,
    windows: Vec<WindowRecord>,
    summary: Summary,
    session_id: String,
}

fn main() {
    let outcome = match std::thread::Builder::new()
        .name("party-effect-window-audit".to_owned())
        .stack_size(AUDIT_THREAD_STACK_BYTES)
        .spawn(|| run().map_err(|error| error.to_string()))
    {
        Ok(worker) => match worker.join() {
            Ok(outcome) => outcome,
            Err(_) => Err("party effect window audit worker panicked".to_owned()),
        },
        Err(error) => Err(error.to_string()),
    };
    if let Err(error) = outcome {
        eprintln!("party effect window audit failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments()?;
    let receipt = read_cohort_receipt_prefix(&arguments.cohort_receipt)?;
    validate_cohort_receipt_identity(&receipt)?;
    let party_value: Value =
        serde_json::from_reader(BufReader::new(File::open(&arguments.party_closure)?))?;
    let party_build = party_value
        .get("game_build")
        .and_then(Value::as_str)
        .ok_or("party closure is missing game_build")?;
    if party_build != receipt.game_build {
        return Err(format!(
            "party closure build {party_build} does not match cohort build {}",
            receipt.game_build
        )
        .into());
    }
    validate_party_policy(&party_value)?;
    let specs = party_effect_specs(&party_value)?;
    eprintln!(
        "loaded {} party-effect frontier IDs and {} exact rlog receipts",
        specs.len(),
        receipt.rlogs.len()
    );
    if specs.is_empty() {
        return Err("party closure contains no rDPS-relevant effect IDs".into());
    }

    let mut tracker = Tracker::new(specs, &receipt.game_build);
    for (index, rlog) in receipt.rlogs.iter().enumerate() {
        eprintln!(
            "verifying and streaming rlog {}/{}: {}",
            index + 1,
            receipt.rlogs.len(),
            rlog.path
        );
        verify_receipt_file(rlog)?;
        tracker.observe_rlog(
            rlog,
            &receipt.deployment_id,
            &receipt.game_build,
            &receipt.protocol_pack_digests,
        )?;
    }
    tracker.finish_all("cohort_end", u64::MAX, u64::MAX);

    let game_build = tracker.game_build.clone();
    let effects = tracker
        .effects
        .into_values()
        .map(|effect| finalize_effect_aggregate(effect, &game_build))
        .collect::<Vec<_>>();
    tracker.summary.party_effects_observed = effects
        .iter()
        .filter(|effect| effect.status_events > 0)
        .count() as u64;
    tracker.summary.party_status_events = effects.iter().map(|effect| effect.status_events).sum();
    tracker.summary.party_status_events_without_source = effects
        .iter()
        .map(|effect| effect.status_events_without_source)
        .sum();
    tracker.summary.windows = tracker.windows.len() as u64;
    tracker.summary.windows_with_affected_entity_damage_actions = tracker
        .windows
        .iter()
        .filter(|window| window.affected_entity_damage_actions.event_count > 0)
        .count() as u64;
    tracker
        .summary
        .windows_with_damage_actions_targeting_affected_entity = tracker
        .windows
        .iter()
        .filter(|window| window.damage_actions_targeting_affected_entity.event_count > 0)
        .count() as u64;

    let report = AuditReport {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-party-effect-window-audit",
        deployment_id: receipt.deployment_id,
        game_build: receipt.game_build,
        protocol_pack_digests: receipt.protocol_pack_digests,
        policy: Policy {
            exact_numeric_effect_ids_and_build_are_authoritative: true,
            localized_names_are_runtime_keys: false,
            remote_player_cast_packets_required: false,
            remote_player_cast_packets_treated_as_zero: false,
            remote_player_cast_packets_synthesized: false,
            status_rows_without_provider_are_preserved: true,
            actor_identity_is_event_time_canonical_evidence_only: true,
            player_identity_is_party_membership_authority: false,
            explicit_party_roster_evidence_consumed: true,
            party_roster_lifecycle_route_coverage_proven: false,
            exact_build_team_id_attribute_evidence_consumed: true,
            team_attribute_interpretation_build: TEAM_ATTRIBUTE_INTERPRETATION_BUILD,
            team_id_attribute_id: ATTR_TEAM_ID,
            team_member_count_attribute_id: ATTR_TEAM_MEMBER_NUMS,
            team_attribute_protocol_event_coverage_proven: false,
            matching_last_observed_team_ids_grant_party_membership_authority: false,
            fight_source_enum_build: FIGHT_SOURCE_ENUM_BUILD,
            fight_source_type_identity_exact_build_gated: true,
            packet_origin_edges_are_skill_to_buff_edges: false,
            packet_origin_edges_are_provider_ownership_authority: false,
            packet_origin_edges_are_formula_authority: false,
            damage_links_preserve_affected_entity_as_actor_and_as_target: true,
            affected_entity_is_assumed_friendly: false,
            affected_entity_is_assumed_enemy: false,
            status_source_to_effect_target_lifecycle_is_preserved: true,
            status_source_is_provider_ownership_authority: false,
            effect_target_role_is_allegiance_neutral: true,
            effect_target_damage_actor_and_damage_target_edges_are_separate: true,
            damage_action_edges_preserve_actor_ability_and_target: true,
            damage_action_edges_are_causal_or_formula_authority: false,
            current_character_snapshots_substituted_into_older_runs: false,
            timeline_presence_is_formula_authority: false,
            provider_rdps_credit_authorized: false,
            runtime_promotion_allowed: false,
            ui_display_allowed: false,
        },
        inputs: Inputs {
            cohort_receipt_path: arguments.cohort_receipt.display().to_string(),
            cohort_receipt_bytes: fs::metadata(&arguments.cohort_receipt)?.len(),
            cohort_receipt_sha256: sha256_file(&arguments.cohort_receipt)?,
            party_closure_path: arguments.party_closure.display().to_string(),
            party_closure_bytes: fs::metadata(&arguments.party_closure)?.len(),
            party_closure_sha256: sha256_file(&arguments.party_closure)?,
            rlogs: receipt.rlogs,
        },
        summary: tracker.summary,
        effects,
        windows: tracker.windows,
    };
    let mut writer = BufWriter::new(File::create(arguments.output)?);
    serde_json::to_writer_pretty(&mut writer, &report)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn validate_cohort_receipt_identity(
    receipt: &CohortReceipt,
) -> Result<(), Box<dyn std::error::Error>> {
    if !(MIN_COHORT_RECEIPT_SCHEMA_VERSION..=MAX_COHORT_RECEIPT_SCHEMA_VERSION)
        .contains(&receipt.schema_version)
        || receipt.generated_by != "rlogs-bpsr-inspiration-proc-attribution-proof"
    {
        return Err(format!(
            "cohort receipt identity is not a reviewed schema-{MIN_COHORT_RECEIPT_SCHEMA_VERSION}..={MAX_COHORT_RECEIPT_SCHEMA_VERSION} proof"
        )
        .into());
    }
    Ok(())
}

impl Tracker {
    fn new(specs: BTreeMap<i64, EffectSpec>, game_build: impl Into<String>) -> Self {
        let effects = specs
            .iter()
            .map(|(&effect_id, spec)| {
                (
                    effect_id,
                    EffectAggregate {
                        effect_id,
                        exact_static_edge: spec.exact_static_edge,
                        reviewed_candidate_edge: spec.reviewed_candidate_edge,
                        source_skill_ids: spec.source_skill_ids.clone(),
                        source_entry_ids: spec.source_entry_ids.clone(),
                        support_categories: spec.support_categories.clone(),
                        ..Default::default()
                    },
                )
            })
            .collect();
        let party_effects_in_frontier = specs.len() as u64;
        Self {
            specs,
            effects,
            actors: BTreeMap::new(),
            observed_party_roster_character_ids: BTreeSet::new(),
            observed_party_roster_has_full_snapshot: false,
            last_observed_team_ids: BTreeMap::new(),
            game_build: game_build.into(),
            active: BTreeMap::new(),
            windows: Vec::new(),
            summary: Summary {
                party_effects_in_frontier,
                ..Default::default()
            },
            session_id: String::new(),
        }
    }

    fn observe_rlog(
        &mut self,
        receipt: &RlogReceipt,
        expected_deployment: &str,
        expected_build: &str,
        allowed_protocol_digests: &[String],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let path = Path::new(&receipt.path);
        let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
        let header = reader.header();
        if header.region.identity.deployment_id != expected_deployment
            || header.region.client_build != expected_build
            || !allowed_protocol_digests.contains(&header.region.protocol_pack_digest)
        {
            return Err(format!("exact-build identity mismatch for {}", path.display()).into());
        }
        self.session_id = header.session_id.clone();
        self.actors.clear();
        self.observed_party_roster_character_ids.clear();
        self.observed_party_roster_has_full_snapshot = false;
        self.last_observed_team_ids.clear();
        while let Some(envelope) = reader.next_event()? {
            self.summary.canonical_events = self.summary.canonical_events.saturating_add(1);
            let timeline = match &envelope.event {
                CanonicalEvent::PartyRosterObserved(event) => {
                    self.observe_party_roster(event);
                    continue;
                }
                CanonicalEvent::Timeline(timeline) => timeline,
                _ => continue,
            };
            match &timeline.kind {
                TimelineEventKind::Actor(actor) => self.observe_actor(actor),
                TimelineEventKind::EntityAttributes(attributes) => self.observe_entity_attributes(
                    envelope.sequence,
                    envelope.time.observed_micros,
                    attributes,
                ),
                TimelineEventKind::Status(status) if self.specs.contains_key(&status.effect.0) => {
                    self.observe_status(envelope.sequence, envelope.time.observed_micros, status)
                }
                TimelineEventKind::Damage(damage) => {
                    self.summary.damage_events = self.summary.damage_events.saturating_add(1);
                    self.observe_damage(envelope.sequence, envelope.time.observed_micros, damage);
                }
                TimelineEventKind::Cast(_) => {
                    self.summary.cast_events_observed =
                        self.summary.cast_events_observed.saturating_add(1)
                }
                TimelineEventKind::EncounterBoundary { state, .. }
                    if matches!(
                        state,
                        EncounterState::Cleared | EncounterState::Wiped | EncounterState::Ended
                    ) =>
                {
                    self.finish_all(
                        "encounter_boundary",
                        envelope.sequence,
                        envelope.time.observed_micros,
                    );
                }
                TimelineEventKind::RunBoundary { state, .. }
                    if matches!(
                        state,
                        RunState::Completed | RunState::Failed | RunState::Exited
                    ) =>
                {
                    self.finish_all(
                        "run_boundary",
                        envelope.sequence,
                        envelope.time.observed_micros,
                    );
                }
                _ => {}
            }
        }
        if reader.summary().is_none() {
            return Err(format!("{} is not a sealed canonical rlog", path.display()).into());
        }
        self.finish_all("log_end", u64::MAX, u64::MAX);
        self.actors.clear();
        self.observed_party_roster_character_ids.clear();
        self.observed_party_roster_has_full_snapshot = false;
        self.last_observed_team_ids.clear();
        self.summary.rlogs_verified = self.summary.rlogs_verified.saturating_add(1);
        Ok(())
    }

    fn observe_actor(&mut self, actor: &ActorEvent) {
        let key = (actor.actor.actor_id.0, actor.actor.entity_uuid.0);
        if actor.state == ActorState::Despawned {
            self.actors.remove(&key);
            self.last_observed_team_ids.remove(&key);
            return;
        }
        self.actors.insert(
            key,
            ActorSnapshot {
                actor: actor.actor,
                kind: actor.kind,
                character_id: actor.character_id.clone(),
                class_id: actor.class_id,
            },
        );
    }

    fn observe_entity_attributes(
        &mut self,
        sequence: u64,
        observed_micros: u64,
        event: &EntityAttributeEvent,
    ) {
        if self.game_build != TEAM_ATTRIBUTE_INTERPRETATION_BUILD {
            return;
        }
        let key = (event.actor.actor_id.0, event.actor.entity_uuid.0);
        for attribute in &event.attributes {
            match attribute.attribute_id {
                ATTR_TEAM_ID => {
                    self.summary.team_id_attribute_events =
                        self.summary.team_id_attribute_events.saturating_add(1);
                    match decode_unsigned_varint_exact(&attribute.raw_value) {
                        Some(value) if value > 0 => {
                            self.summary.team_id_attribute_positive_values = self
                                .summary
                                .team_id_attribute_positive_values
                                .saturating_add(1);
                            self.last_observed_team_ids.insert(key, value);
                        }
                        Some(0) => {
                            self.summary.team_id_attribute_clear_values = self
                                .summary
                                .team_id_attribute_clear_values
                                .saturating_add(1);
                            self.last_observed_team_ids.remove(&key);
                        }
                        None => {
                            self.summary.team_id_attribute_malformed_values = self
                                .summary
                                .team_id_attribute_malformed_values
                                .saturating_add(1);
                            if self.summary.team_id_attribute_malformed_examples.len() < 16 {
                                self.summary.team_id_attribute_malformed_examples.push(
                                    TeamAttributeObservationExample {
                                        session_id: self.session_id.clone(),
                                        sequence,
                                        observed_micros,
                                        actor_id: event.actor.actor_id.0.to_string(),
                                        entity_uuid: event.actor.entity_uuid.0.to_string(),
                                        raw_value: attribute.raw_value.clone(),
                                    },
                                );
                            }
                            self.last_observed_team_ids.remove(&key);
                        }
                        Some(_) => unreachable!("positive and zero cover unsigned values"),
                    }
                }
                ATTR_TEAM_MEMBER_NUMS => {
                    self.summary.team_member_count_attribute_events = self
                        .summary
                        .team_member_count_attribute_events
                        .saturating_add(1);
                }
                _ => {}
            }
        }
    }

    fn observe_party_roster(&mut self, event: &PartyRosterEvent) {
        match &event.observation {
            PartyRosterObservation::FullSnapshot { members, .. } => {
                self.summary.party_roster_full_snapshot_events = self
                    .summary
                    .party_roster_full_snapshot_events
                    .saturating_add(1);
                self.observed_party_roster_character_ids = members
                    .iter()
                    .map(|member| member.character.character_id.clone())
                    .collect();
                self.observed_party_roster_has_full_snapshot = true;
            }
            PartyRosterObservation::MembersObserved { members } => {
                self.summary.party_roster_members_observed_events = self
                    .summary
                    .party_roster_members_observed_events
                    .saturating_add(1);
                self.observed_party_roster_character_ids.extend(
                    members
                        .iter()
                        .map(|member| member.character.character_id.clone()),
                );
            }
            PartyRosterObservation::MemberLeft { member, .. } => {
                self.summary.party_roster_member_left_events = self
                    .summary
                    .party_roster_member_left_events
                    .saturating_add(1);
                self.observed_party_roster_character_ids
                    .remove(&member.character_id);
                // Until the exact-build pack proves complete leave/dissolve
                // route coverage, a leave invalidates persisted completeness.
                self.observed_party_roster_has_full_snapshot = false;
            }
            PartyRosterObservation::Dissolved => {
                self.summary.party_roster_dissolved_events =
                    self.summary.party_roster_dissolved_events.saturating_add(1);
                self.observed_party_roster_character_ids.clear();
                self.observed_party_roster_has_full_snapshot = false;
            }
        }
    }

    fn observe_status(&mut self, sequence: u64, observed_micros: u64, status: &StatusEvent) {
        let effect_id = status.effect.0;
        let state = status_state_name(status.state).to_owned();
        let source_snapshot = status
            .source
            .and_then(|source| self.actor_snapshot(source))
            .cloned();
        let affected_snapshot = self.actor_snapshot(status.target).cloned();
        let party_scope =
            self.party_scope_evidence(source_snapshot.as_ref(), affected_snapshot.as_ref());
        let aggregate = self
            .effects
            .get_mut(&effect_id)
            .expect("known party effect");
        aggregate.status_events = aggregate.status_events.saturating_add(1);
        *aggregate.lifecycle_counts.entry(state.clone()).or_default() += 1;
        aggregate
            .unique_affected_entity_actor_ids
            .insert(status.target.actor_id.0.to_string());
        if let Some(source) = status.source {
            aggregate.status_events_with_source =
                aggregate.status_events_with_source.saturating_add(1);
            aggregate
                .unique_source_actor_ids
                .insert(source.actor_id.0.to_string());
        } else {
            aggregate.status_events_without_source =
                aggregate.status_events_without_source.saturating_add(1);
        }
        if let Some(origin) = status.origin {
            aggregate
                .unique_origin_pairs
                .insert((origin.source_type_id, origin.source_config_id));
            *aggregate
                .origin_observation_counts
                .entry((origin.source_type_id, origin.source_config_id))
                .or_default() += 1;
        }
        if let Some(duration_millis) = status.duration_millis {
            aggregate.reported_duration_millis.insert(duration_millis);
        }
        if let Some(level) = status.level {
            aggregate.reported_status_levels.insert(level);
        }
        if let Some(stacks) = status.stacks {
            aggregate.reported_stacks.insert(stacks);
        }
        if let Some(count) = status.count {
            aggregate.reported_counts.insert(count);
        }
        observe_identity_evidence(
            &mut aggregate.identity_evidence,
            status,
            source_snapshot.as_ref(),
            affected_snapshot.as_ref(),
            party_scope,
        );
        let key = WindowKey {
            effect_id,
            affected_entity_actor_id: status.target.actor_id.0,
            instance_id: status.instance_id.map(|id| id.0),
        };
        match status.state {
            StatusState::Applied => {
                if let Some(window) = self.active.remove(&key) {
                    self.close_window(window, sequence, observed_micros, "replaced_by_apply");
                }
                let mut window = self.new_window(sequence, observed_micros, status, false);
                observe_status_identity(
                    &mut window,
                    status,
                    source_snapshot.as_ref(),
                    affected_snapshot.as_ref(),
                    party_scope,
                );
                *window.lifecycle_counts.entry(state).or_default() += 1;
                self.active.insert(key, window);
            }
            StatusState::Refreshed | StatusState::Stacked => {
                let window = self.active.entry(key).or_insert_with(|| {
                    let mut window = new_window_from_parts(
                        &self.session_id,
                        self.specs.get(&effect_id).expect("known party effect"),
                        sequence,
                        observed_micros,
                        status,
                        true,
                    );
                    window.orphan_lifecycle_start = true;
                    window
                });
                observe_status_identity(
                    window,
                    status,
                    source_snapshot.as_ref(),
                    affected_snapshot.as_ref(),
                    party_scope,
                );
                *window.lifecycle_counts.entry(state).or_default() += 1;
            }
            StatusState::Consumed | StatusState::Removed => {
                let mut window = self
                    .active
                    .remove(&key)
                    .unwrap_or_else(|| self.new_window(sequence, observed_micros, status, true));
                observe_status_identity(
                    &mut window,
                    status,
                    source_snapshot.as_ref(),
                    affected_snapshot.as_ref(),
                    party_scope,
                );
                *window.lifecycle_counts.entry(state).or_default() += 1;
                self.close_window(
                    window,
                    sequence,
                    observed_micros,
                    status_state_name(status.state),
                );
            }
        }
    }

    fn actor_snapshot(&self, actor: EntityRef) -> Option<&ActorSnapshot> {
        self.actors
            .get(&(actor.actor_id.0, actor.entity_uuid.0))
            .filter(|snapshot| snapshot.actor == actor)
    }

    fn party_scope_evidence(
        &self,
        source: Option<&ActorSnapshot>,
        affected: Option<&ActorSnapshot>,
    ) -> PartyScopeEvidence {
        let both_in_observed_party_roster = if self.observed_party_roster_has_full_snapshot {
            source
                .and_then(|actor| actor.character_id.as_ref())
                .zip(affected.and_then(|actor| actor.character_id.as_ref()))
                .is_some_and(|(source_character_id, affected_character_id)| {
                    self.observed_party_roster_character_ids
                        .contains(source_character_id)
                        && self
                            .observed_party_roster_character_ids
                            .contains(affected_character_id)
                })
        } else {
            false
        };
        let player_pair = source.zip(affected).filter(|(source, affected)| {
            source.kind == ActorKind::Player && affected.kind == ActorKind::Player
        });
        let team_pair = player_pair.and_then(|(source, affected)| {
            self.last_observed_team_ids
                .get(&(source.actor.actor_id.0, source.actor.entity_uuid.0))
                .copied()
                .zip(
                    self.last_observed_team_ids
                        .get(&(affected.actor.actor_id.0, affected.actor.entity_uuid.0))
                        .copied(),
                )
        });
        PartyScopeEvidence {
            both_in_observed_party_roster,
            matching_last_observed_team_id: team_pair
                .filter(|(source_team_id, affected_team_id)| source_team_id == affected_team_id)
                .map(|(team_id, _)| team_id),
            mismatching_last_observed_team_ids: team_pair.is_some_and(
                |(source_team_id, affected_team_id)| source_team_id != affected_team_id,
            ),
        }
    }

    fn new_window(
        &self,
        sequence: u64,
        observed_micros: u64,
        status: &StatusEvent,
        orphan: bool,
    ) -> WindowRecord {
        new_window_from_parts(
            &self.session_id,
            self.specs
                .get(&status.effect.0)
                .expect("known party effect"),
            sequence,
            observed_micros,
            status,
            orphan,
        )
    }

    fn observe_damage(&mut self, sequence: u64, observed_micros: u64, damage: &DamageEvent) {
        let amount = i128::from(damage.amount.max(0));
        for window in self.active.values_mut() {
            if window.affected_entity_actor_id == damage.source.actor_id.0.to_string() {
                observe_damage_relation(&mut window.affected_entity_damage_actions, damage, amount);
                observe_damage_action_edge(
                    &mut window.damage_action_edge_accumulator,
                    DamageActionRole::EffectTargetIsDamageActor,
                    sequence,
                    observed_micros,
                    damage,
                    amount,
                );
            }
            if window.affected_entity_actor_id == damage.target.actor_id.0.to_string() {
                observe_damage_relation(
                    &mut window.damage_actions_targeting_affected_entity,
                    damage,
                    amount,
                );
                observe_damage_action_edge(
                    &mut window.damage_action_edge_accumulator,
                    DamageActionRole::EffectTargetIsDamageTarget,
                    sequence,
                    observed_micros,
                    damage,
                    amount,
                );
            }
        }
    }

    fn finish_all(&mut self, reason: &str, sequence: u64, observed_micros: u64) {
        let active = std::mem::take(&mut self.active);
        for (_, window) in active {
            self.close_window(window, sequence, observed_micros, reason);
        }
    }

    fn close_window(
        &mut self,
        mut window: WindowRecord,
        sequence: u64,
        observed_micros: u64,
        reason: &str,
    ) {
        window.damage_action_edges = std::mem::take(&mut window.damage_action_edge_accumulator)
            .into_values()
            .collect();
        self.summary.window_damage_action_edges = self
            .summary
            .window_damage_action_edges
            .saturating_add(window.damage_action_edges.len() as u64);
        self.summary.window_damage_action_actor_edges = self
            .summary
            .window_damage_action_actor_edges
            .saturating_add(
                window
                    .damage_action_edges
                    .iter()
                    .filter(|edge| edge.role == DamageActionRole::EffectTargetIsDamageActor)
                    .count() as u64,
            );
        self.summary.window_damage_action_target_edges = self
            .summary
            .window_damage_action_target_edges
            .saturating_add(
                window
                    .damage_action_edges
                    .iter()
                    .filter(|edge| edge.role == DamageActionRole::EffectTargetIsDamageTarget)
                    .count() as u64,
            );
        if sequence == u64::MAX {
            window.close_reason = Some(reason.to_owned());
            self.effects
                .get_mut(&window.effect_id)
                .expect("known party effect")
                .windows_open_at_log_end += 1;
        } else {
            window.end_sequence = Some(sequence);
            window.end_observed_micros = Some(observed_micros);
            window.close_reason = Some(reason.to_owned());
            self.effects
                .get_mut(&window.effect_id)
                .expect("known party effect")
                .windows_closed += 1;
        }
        let aggregate = self
            .effects
            .get_mut(&window.effect_id)
            .expect("known party effect");
        if window.orphan_lifecycle_start {
            aggregate.orphan_lifecycle_windows += 1;
        }
        merge_damage_relation(
            &mut aggregate.affected_entity_damage_actions,
            &window.affected_entity_damage_actions,
        );
        merge_damage_relation(
            &mut aggregate.damage_actions_targeting_affected_entity,
            &window.damage_actions_targeting_affected_entity,
        );
        self.windows.push(window);
    }
}

fn new_window_from_parts(
    session_id: &str,
    spec: &EffectSpec,
    sequence: u64,
    observed_micros: u64,
    status: &StatusEvent,
    orphan: bool,
) -> WindowRecord {
    WindowRecord {
        session_id: session_id.to_owned(),
        effect_id: status.effect.0,
        exact_static_edge: spec.exact_static_edge,
        reviewed_candidate_edge: spec.reviewed_candidate_edge,
        affected_entity_actor_id: status.target.actor_id.0.to_string(),
        affected_entity_uuid: status.target.entity_uuid.0.to_string(),
        effect_target_actor_id: status.target.actor_id.0.to_string(),
        effect_target_entity_uuid: status.target.entity_uuid.0.to_string(),
        instance_id: status.instance_id.map(|id| id.0.to_string()),
        source_actor_ids: BTreeSet::new(),
        source_entity_uuids: BTreeSet::new(),
        missing_source_observed: status.source.is_none(),
        provider_conflict_observed: false,
        origin_pairs: BTreeSet::new(),
        levels: BTreeSet::new(),
        reported_duration_millis: BTreeSet::new(),
        reported_stacks: BTreeSet::new(),
        reported_counts: BTreeSet::new(),
        start_sequence: sequence,
        end_sequence: None,
        start_observed_micros: observed_micros,
        end_observed_micros: None,
        close_reason: None,
        lifecycle_counts: BTreeMap::new(),
        orphan_lifecycle_start: orphan,
        identity_evidence: IdentityEvidence::default(),
        affected_entity_damage_actions: DamageRelation::default(),
        damage_actions_targeting_affected_entity: DamageRelation::default(),
        damage_action_edges: Vec::new(),
        damage_action_edge_accumulator: BTreeMap::new(),
        provider_rdps_credit_authorized: false,
    }
}

fn observed_origin_edge(
    game_build: &str,
    source_type_id: i32,
    source_config_id: i64,
    child_effect_id: i64,
    observation_count: u64,
) -> ObservedOriginEdge {
    let (source_kind, source_enum_name) = if game_build == FIGHT_SOURCE_ENUM_BUILD {
        fight_source_identity(source_type_id)
    } else {
        ("unresolved", None)
    };
    ObservedOriginEdge {
        source_type_id,
        source_kind,
        source_enum_name,
        source_config_id,
        child_effect_id,
        observation_count,
        exact_current_build_enum_identity: source_enum_name.is_some(),
    }
}

fn finalize_effect_aggregate(mut effect: EffectAggregate, game_build: &str) -> EffectAggregate {
    effect.observed_origin_edges = effect
        .origin_observation_counts
        .iter()
        .map(
            |(&(source_type_id, source_config_id), &observation_count)| {
                observed_origin_edge(
                    game_build,
                    source_type_id,
                    source_config_id,
                    effect.effect_id,
                    observation_count,
                )
            },
        )
        .collect();
    effect
}

// Exact names and values from the current-build Zproto.EFightSource enum in
// research/game-file-inventory/global/steam-24687926/rpc-message-surface.v2.json.
fn fight_source_identity(source_type_id: i32) -> (&'static str, Option<&'static str>) {
    match source_type_id {
        0 => ("skill", Some("EFightSourceSkill")),
        1 => ("buff", Some("EFightSourceBuff")),
        2 => ("bullet", Some("EFightSourceBullet")),
        4 => ("task", Some("EFightSourceTask")),
        6 => ("talent", Some("EFightSourceTalent")),
        7 => ("season-medal", Some("EFightSourceSeasonMedal")),
        8 => ("union-effect", Some("EFightSourceUnionEffect")),
        9 => ("mod", Some("EFightSourceMod")),
        10 => ("equip", Some("EFightSourceEquip")),
        11 => ("equip-slot-refine", Some("EFightSourceEquipSlotRefine")),
        12 => ("vehicle", Some("EFightSourceVehicle")),
        13 => ("season-talent", Some("EFightSourceSeasonTalent")),
        14 => ("fantasy-atlas", Some("EFightSourceFantasyAtlas")),
        1000 => ("scene-begin", Some("EFightSourceSceneBegin")),
        1001 => ("scene", Some("EFightSourceScene")),
        1002 => ("affix", Some("EFightSourceAffix")),
        10000 => ("other", Some("EFightSourceOther")),
        _ => ("unresolved", None),
    }
}

fn observe_status_identity(
    window: &mut WindowRecord,
    status: &StatusEvent,
    source_snapshot: Option<&ActorSnapshot>,
    affected_snapshot: Option<&ActorSnapshot>,
    party_scope: PartyScopeEvidence,
) {
    if let Some(source) = status.source {
        window
            .source_actor_ids
            .insert(source.actor_id.0.to_string());
        window
            .source_entity_uuids
            .insert(source.entity_uuid.0.to_string());
        if window.source_actor_ids.len() > 1 || window.source_entity_uuids.len() > 1 {
            window.provider_conflict_observed = true;
        }
    } else {
        window.missing_source_observed = true;
    }
    if let Some(origin) = status.origin {
        window
            .origin_pairs
            .insert((origin.source_type_id, origin.source_config_id));
    }
    if let Some(level) = status.level {
        window.levels.insert(level);
    }
    if let Some(duration_millis) = status.duration_millis {
        window.reported_duration_millis.insert(duration_millis);
    }
    if let Some(stacks) = status.stacks {
        window.reported_stacks.insert(stacks);
    }
    if let Some(count) = status.count {
        window.reported_counts.insert(count);
    }
    observe_identity_evidence(
        &mut window.identity_evidence,
        status,
        source_snapshot,
        affected_snapshot,
        party_scope,
    );
}

fn observe_identity_evidence(
    evidence: &mut IdentityEvidence,
    status: &StatusEvent,
    source_snapshot: Option<&ActorSnapshot>,
    affected_snapshot: Option<&ActorSnapshot>,
    party_scope: PartyScopeEvidence,
) {
    evidence.affected_entity_status_events =
        evidence.affected_entity_status_events.saturating_add(1);
    observe_actor_snapshot(
        affected_snapshot,
        &mut evidence.affected_entity_player_identity_events,
        &mut evidence.affected_entity_non_player_identity_events,
        &mut evidence.affected_entity_identity_unresolved_events,
        &mut evidence.affected_entity_actor_kinds,
        &mut evidence.affected_entity_character_ids,
        &mut evidence.affected_entity_class_ids,
    );
    let Some(source) = status.source else {
        return;
    };
    evidence.source_status_events = evidence.source_status_events.saturating_add(1);
    observe_actor_snapshot(
        source_snapshot,
        &mut evidence.source_player_identity_events,
        &mut evidence.source_non_player_identity_events,
        &mut evidence.source_identity_unresolved_events,
        &mut evidence.source_actor_kinds,
        &mut evidence.source_character_ids,
        &mut evidence.source_class_ids,
    );
    if source == status.target {
        evidence.self_source_affected_status_events = evidence
            .self_source_affected_status_events
            .saturating_add(1);
        return;
    }
    evidence.external_source_affected_status_events = evidence
        .external_source_affected_status_events
        .saturating_add(1);
    if source_snapshot.is_some_and(|snapshot| snapshot.kind == ActorKind::Player)
        && affected_snapshot.is_some_and(|snapshot| snapshot.kind == ActorKind::Player)
    {
        evidence.external_status_events_with_both_player_identities = evidence
            .external_status_events_with_both_player_identities
            .saturating_add(1);
    } else if source_snapshot.is_none() || affected_snapshot.is_none() {
        evidence.external_status_events_with_unresolved_identity = evidence
            .external_status_events_with_unresolved_identity
            .saturating_add(1);
    }
    if party_scope.both_in_observed_party_roster {
        evidence.external_status_events_with_both_in_observed_party_roster = evidence
            .external_status_events_with_both_in_observed_party_roster
            .saturating_add(1);
        // The event stream now preserves exact roster observations, but the
        // current-build protocol pack and its leave/dissolve route coverage are
        // still absent. Retain the containment evidence without upgrading it
        // to authoritative party membership.
        evidence.external_status_events_with_roster_evidence_but_lifecycle_coverage_open = evidence
            .external_status_events_with_roster_evidence_but_lifecycle_coverage_open
            .saturating_add(1);
    }
    if let Some(team_id) = party_scope.matching_last_observed_team_id {
        evidence.external_status_events_with_matching_last_observed_team_id = evidence
            .external_status_events_with_matching_last_observed_team_id
            .saturating_add(1);
        evidence
            .matching_last_observed_team_ids
            .insert(team_id.to_string());
        // The exact-build attribute meaning is proven by the native enum and
        // the raw value is replayable, but the current-build protocol event
        // coverage gate is still open. Treat this as scope evidence only.
        evidence.external_status_events_with_team_id_evidence_but_protocol_coverage_open = evidence
            .external_status_events_with_team_id_evidence_but_protocol_coverage_open
            .saturating_add(1);
    } else if party_scope.mismatching_last_observed_team_ids {
        evidence.external_status_events_with_mismatching_last_observed_team_ids = evidence
            .external_status_events_with_mismatching_last_observed_team_ids
            .saturating_add(1);
    } else {
        evidence.external_status_events_with_unresolved_last_observed_team_id = evidence
            .external_status_events_with_unresolved_last_observed_team_id
            .saturating_add(1);
    }
    evidence.party_membership_unproven_status_events = evidence
        .party_membership_unproven_status_events
        .saturating_add(1);
}

#[allow(clippy::too_many_arguments)]
fn observe_actor_snapshot(
    snapshot: Option<&ActorSnapshot>,
    player_events: &mut u64,
    non_player_events: &mut u64,
    unresolved_events: &mut u64,
    kinds: &mut BTreeSet<String>,
    character_ids: &mut BTreeSet<String>,
    class_ids: &mut BTreeSet<i32>,
) {
    let Some(snapshot) = snapshot else {
        *unresolved_events = unresolved_events.saturating_add(1);
        return;
    };
    kinds.insert(actor_kind_name(snapshot.kind));
    if snapshot.kind == ActorKind::Player {
        *player_events = player_events.saturating_add(1);
    } else {
        *non_player_events = non_player_events.saturating_add(1);
    }
    if let Some(character_id) = &snapshot.character_id {
        character_ids.insert(character_id.clone());
    }
    if let Some(class_id) = snapshot.class_id {
        class_ids.insert(class_id);
    }
}

fn decode_unsigned_varint_exact(bytes: &[u8]) -> Option<u64> {
    // Attr scalar payloads are stored inside a bytes field. The game's
    // canonical decoder proves that an omitted scalar zero is represented by
    // an empty payload, so empty is an exact clear rather than malformed.
    if bytes.is_empty() {
        return Some(0);
    }
    if bytes.len() > 10 {
        return None;
    }
    let mut value = 0_u64;
    for (index, byte) in bytes.iter().copied().enumerate() {
        if index == 9 && byte > 1 {
            return None;
        }
        value |= u64::from(byte & 0x7f) << (index * 7);
        if byte & 0x80 == 0 {
            if index + 1 != bytes.len() || unsigned_varint_len(value) != bytes.len() {
                return None;
            }
            return Some(value);
        }
    }
    None
}

fn unsigned_varint_len(mut value: u64) -> usize {
    let mut length = 1;
    while value >= 0x80 {
        value >>= 7;
        length += 1;
    }
    length
}

fn actor_kind_name(kind: ActorKind) -> String {
    match kind {
        ActorKind::Unknown(value) => format!("unknown:{value}"),
        other => format!("{other:?}").to_ascii_lowercase(),
    }
}

fn observe_damage_relation(relation: &mut DamageRelation, damage: &DamageEvent, amount: i128) {
    relation.event_count = relation.event_count.saturating_add(1);
    relation.amount = relation.amount.saturating_add(amount);
    if let Some(ability) = damage.ability {
        relation.ability_ids.insert(ability.0);
    }
    relation
        .damage_source_actor_ids
        .insert(damage.source.actor_id.0.to_string());
    relation
        .damage_target_actor_ids
        .insert(damage.target.actor_id.0.to_string());
}

fn observe_damage_action_edge(
    edges: &mut BTreeMap<DamageActionKey, DamageActionEdge>,
    role: DamageActionRole,
    sequence: u64,
    observed_micros: u64,
    damage: &DamageEvent,
    amount: i128,
) {
    let key = DamageActionKey {
        role,
        damage_source_actor_id: damage.source.actor_id.0,
        damage_source_entity_uuid: damage.source.entity_uuid.0,
        direct_damage_source_actor_id: damage.direct_source.map(|source| source.actor_id.0),
        direct_damage_source_entity_uuid: damage.direct_source.map(|source| source.entity_uuid.0),
        ability_id: damage.ability.map(|ability| ability.0),
        damage_target_actor_id: damage.target.actor_id.0,
        damage_target_entity_uuid: damage.target.entity_uuid.0,
    };
    let edge = edges.entry(key).or_insert_with(|| DamageActionEdge {
        role,
        damage_source_actor_id: damage.source.actor_id.0.to_string(),
        damage_source_entity_uuid: damage.source.entity_uuid.0.to_string(),
        direct_damage_source_actor_id: damage
            .direct_source
            .map(|source| source.actor_id.0.to_string()),
        direct_damage_source_entity_uuid: damage
            .direct_source
            .map(|source| source.entity_uuid.0.to_string()),
        ability_id: damage.ability.map(|ability| ability.0),
        damage_target_actor_id: damage.target.actor_id.0.to_string(),
        damage_target_entity_uuid: damage.target.entity_uuid.0.to_string(),
        event_count: 0,
        amount: 0,
        first_sequence: sequence,
        last_sequence: sequence,
        first_observed_micros: observed_micros,
        last_observed_micros: observed_micros,
        samples: Vec::new(),
        causal_attribution_authorized: false,
        provider_rdps_credit_authorized: false,
    });
    edge.event_count = edge.event_count.saturating_add(1);
    edge.amount = edge.amount.saturating_add(amount);
    edge.last_sequence = sequence;
    edge.last_observed_micros = observed_micros;
    if edge.samples.len() < MAX_DAMAGE_ACTION_SAMPLES_PER_EDGE {
        edge.samples.push(DamageActionSample {
            sequence,
            observed_micros,
            amount: damage.amount,
            actual_amount: damage.actual_amount,
            hit_event_id: damage.hit_event_id,
            skill_effect_uuid: damage
                .packet
                .skill_effect_uuid
                .map(|value| value.to_string()),
        });
    }
}

fn merge_damage_relation(target: &mut DamageRelation, source: &DamageRelation) {
    target.event_count = target.event_count.saturating_add(source.event_count);
    target.amount = target.amount.saturating_add(source.amount);
    target
        .ability_ids
        .extend(source.ability_ids.iter().copied());
    target
        .damage_source_actor_ids
        .extend(source.damage_source_actor_ids.iter().cloned());
    target
        .damage_target_actor_ids
        .extend(source.damage_target_actor_ids.iter().cloned());
}

fn status_state_name(state: StatusState) -> &'static str {
    match state {
        StatusState::Applied => "applied",
        StatusState::Refreshed => "refreshed",
        StatusState::Stacked => "stacked",
        StatusState::Consumed => "consumed",
        StatusState::Removed => "removed",
    }
}

fn validate_party_policy(value: &Value) -> Result<(), Box<dyn std::error::Error>> {
    let policy = value
        .get("policy")
        .ok_or("party closure is missing policy")?;
    let required_true = [
        "exact_numeric_skill_effect_buff_ids_and_build_are_authoritative",
        "unresolved_skill_to_buff_edges_preserved",
    ];
    let required_false = [
        "remote_player_cast_packets_required",
        "remote_player_cast_packets_treated_as_zero",
        "remote_player_cast_packets_synthesized",
        "provider_rdps_credit_allowed",
        "runtime_promotion_allowed",
        "ui_rdps_display_allowed",
    ];
    if required_true
        .iter()
        .any(|key| policy.get(key).and_then(Value::as_bool) != Some(true))
        || required_false
            .iter()
            .any(|key| policy.get(key).and_then(Value::as_bool) != Some(false))
    {
        return Err("party closure policy is not fail-closed".into());
    }
    Ok(())
}

fn party_effect_specs(value: &Value) -> Result<BTreeMap<i64, EffectSpec>, String> {
    let mut specs = BTreeMap::<i64, EffectSpec>::new();
    for skill in value
        .get("skill_candidates")
        .and_then(Value::as_array)
        .ok_or("party closure is missing skill_candidates")?
    {
        if skill
            .get("rdps_relevant_candidate")
            .and_then(Value::as_bool)
            != Some(true)
        {
            continue;
        }
        let skill_id = required_i64(skill, "skill_id")?;
        let categories = string_set(skill.get("support_categories"));
        for edge in array(skill.get("exact_skill_to_buff_edges")) {
            let effect_id = required_i64(edge, "buff_id")?;
            add_spec(
                &mut specs,
                effect_id,
                Some(skill_id),
                None,
                true,
                false,
                &categories,
            );
        }
        for edge in array(skill.get("reviewed_candidate_skill_to_buff_links")) {
            let effect_id = required_i64(edge, "buff_id")?;
            add_spec(
                &mut specs,
                effect_id,
                Some(skill_id),
                None,
                false,
                true,
                &categories,
            );
        }
    }
    for entry in value
        .get("rogue_party_entry_candidates")
        .and_then(Value::as_array)
        .ok_or("party closure is missing rogue_party_entry_candidates")?
    {
        if entry
            .get("rdps_relevant_candidate")
            .and_then(Value::as_bool)
            != Some(true)
        {
            continue;
        }
        let entry_id = required_i64(entry, "entry_id")?;
        let categories = string_set(entry.get("support_categories"));
        let root = required_i64(entry, "exact_root_buff_id")?;
        add_spec(
            &mut specs,
            root,
            None,
            Some(entry_id),
            true,
            false,
            &categories,
        );
        for child in array(entry.get("candidate_child_buff_family")) {
            let effect_id = required_i64(child, "buff_id")?;
            add_spec(
                &mut specs,
                effect_id,
                None,
                Some(entry_id),
                false,
                true,
                &categories,
            );
        }
    }
    Ok(specs)
}

fn add_spec(
    specs: &mut BTreeMap<i64, EffectSpec>,
    effect_id: i64,
    skill_id: Option<i64>,
    entry_id: Option<i64>,
    exact: bool,
    candidate: bool,
    categories: &BTreeSet<String>,
) {
    let spec = specs.entry(effect_id).or_insert_with(|| EffectSpec {
        effect_id,
        ..Default::default()
    });
    spec.exact_static_edge |= exact;
    spec.reviewed_candidate_edge |= candidate;
    if let Some(skill_id) = skill_id {
        spec.source_skill_ids.insert(skill_id);
    }
    if let Some(entry_id) = entry_id {
        spec.source_entry_ids.insert(entry_id);
    }
    spec.support_categories.extend(categories.iter().cloned());
}

fn array(value: Option<&Value>) -> &[Value] {
    value.and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

fn string_set(value: Option<&Value>) -> BTreeSet<String> {
    array(value)
        .iter()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect()
}

fn required_i64(value: &Value, key: &str) -> Result<i64, String> {
    value
        .get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("missing integer {key}"))
}

fn verify_receipt_file(receipt: &RlogReceipt) -> Result<(), Box<dyn std::error::Error>> {
    let path = Path::new(&receipt.path);
    let bytes = fs::metadata(path)?.len();
    if bytes != receipt.bytes {
        return Err(format!("byte length changed for {}", path.display()).into());
    }
    let hash = sha256_file(path)?;
    if !hash.eq_ignore_ascii_case(&receipt.sha256) {
        return Err(format!("SHA-256 changed for {}", path.display()).into());
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, std::io::Error> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn read_cohort_receipt_prefix(path: &Path) -> Result<CohortReceipt, Box<dyn std::error::Error>> {
    let reader = BufReader::new(File::open(path)?);
    let mut lines = reader.lines();
    let mut schema_version = None;
    let mut generated_by = None;
    let mut deployment_id = None;
    let mut game_build = None;
    let mut protocol_pack_digests = None;
    let mut rlogs = None;
    while let Some(line) = lines.next() {
        let line = line?;
        if schema_version.is_none() {
            schema_version = property_value::<u16>(&line, "schema_version")?;
        }
        if generated_by.is_none() {
            generated_by = property_value::<String>(&line, "generated_by")?;
        }
        if deployment_id.is_none() {
            deployment_id = property_value::<String>(&line, "deployment_id")?;
        }
        if game_build.is_none() {
            game_build = property_value::<String>(&line, "game_build")?;
        }
        if protocol_pack_digests.is_none()
            && let Some(initial) = property_fragment(&line, "protocol_pack_digests")
        {
            let array = collect_array(initial, &mut lines)?;
            protocol_pack_digests = Some(serde_json::from_str(&array)?);
        }
        if rlogs.is_none()
            && let Some(initial) = property_fragment(&line, "rlogs")
        {
            let array = collect_array(initial, &mut lines)?;
            rlogs = Some(serde_json::from_str(&array)?);
        }
        if schema_version.is_some()
            && generated_by.is_some()
            && deployment_id.is_some()
            && game_build.is_some()
            && protocol_pack_digests.is_some()
            && rlogs.is_some()
        {
            break;
        }
    }
    Ok(CohortReceipt {
        schema_version: schema_version.ok_or("cohort receipt is missing schema_version")?,
        generated_by: generated_by.ok_or("cohort receipt is missing generated_by")?,
        deployment_id: deployment_id.ok_or("cohort receipt is missing deployment_id")?,
        game_build: game_build.ok_or("cohort receipt is missing game_build")?,
        protocol_pack_digests: protocol_pack_digests
            .ok_or("cohort receipt is missing protocol_pack_digests")?,
        rlogs: rlogs.ok_or("cohort receipt is missing rlogs")?,
    })
}

fn property_value<T: for<'de> Deserialize<'de>>(
    line: &str,
    key: &str,
) -> Result<Option<T>, serde_json::Error> {
    let Some(fragment) = property_fragment(line, key) else {
        return Ok(None);
    };
    let fragment = fragment.trim_end_matches(',').trim();
    serde_json::from_str(fragment).map(Some)
}

fn property_fragment<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let trimmed = line.trim_start();
    let prefix = format!("\"{key}\":");
    trimmed.strip_prefix(&prefix).map(str::trim_start)
}

fn collect_array(
    initial: &str,
    lines: &mut impl Iterator<Item = Result<String, std::io::Error>>,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut text = initial.trim().to_owned();
    let mut depth = json_array_depth(&text);
    if depth <= 0 {
        return Err("JSON array property did not begin with an array".into());
    }
    while depth > 0 {
        let line = lines
            .next()
            .ok_or("JSON array ended before its closing bracket")??;
        text.push('\n');
        text.push_str(line.trim());
        depth = json_array_depth(&text);
    }
    while text.ends_with(',') || text.ends_with(char::is_whitespace) {
        text.pop();
    }
    Ok(text)
}

fn json_array_depth(value: &str) -> i64 {
    let mut depth = 0_i64;
    let mut in_string = false;
    let mut escaped = false;
    for character in value.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            '[' => depth += 1,
            ']' => depth -= 1,
            _ => {}
        }
    }
    depth
}

fn arguments() -> Result<Arguments, String> {
    let mut args = env::args().skip(1).collect::<Vec<_>>();
    let cohort_receipt = take(&mut args, "--cohort-receipt")?;
    let party_closure = take(&mut args, "--party-closure")?;
    let output = take(&mut args, "--output")?;
    if !args.is_empty() {
        return Err(usage());
    }
    Ok(Arguments {
        cohort_receipt: cohort_receipt.into(),
        party_closure: party_closure.into(),
        output: output.into(),
    })
}

fn take(args: &mut Vec<String>, flag: &str) -> Result<String, String> {
    let position = args
        .iter()
        .position(|value| value == flag)
        .ok_or_else(usage)?;
    args.remove(position);
    if position >= args.len() {
        return Err(format!("{flag} requires a value"));
    }
    Ok(args.remove(position))
}

fn usage() -> String {
    "usage: rlogs-bpsr-party-effect-window-audit --cohort-receipt <proof.json> --party-closure <party.json> --output <audit.json>".into()
}

#[cfg(test)]
mod tests {
    use rlogs_events::{
        AbilityId, ActorId, DamageFlags, DamagePacketDetail, EntityRef, EntityUuid, StatusEffectId,
        StatusEffectInstanceId, StatusOrigin,
    };
    use serde_json::json;

    use super::*;

    fn entity(actor_id: u64, entity_uuid: i64) -> EntityRef {
        EntityRef {
            actor_id: ActorId(actor_id),
            entity_uuid: EntityUuid(entity_uuid),
        }
    }

    fn party_fixture() -> Value {
        json!({
            "skill_candidates": [{
                "skill_id": 1410,
                "rdps_relevant_candidate": true,
                "support_categories": ["party-action-opportunity"],
                "exact_skill_to_buff_edges": [],
                "reviewed_candidate_skill_to_buff_links": [{"buff_id": 31602}]
            }],
            "rogue_party_entry_candidates": [{
                "entry_id": 103,
                "exact_root_buff_id": 998542,
                "rdps_relevant_candidate": true,
                "support_categories": ["party-offensive-stat"],
                "candidate_child_buff_family": [{"buff_id": 998543}]
            }]
        })
    }

    fn status(state: StatusState, source: Option<EntityRef>) -> StatusEvent {
        StatusEvent {
            source,
            target: entity(2, 200),
            effect: StatusEffectId(31_602),
            instance_id: Some(StatusEffectInstanceId(7)),
            origin: Some(StatusOrigin {
                source_type_id: 1,
                source_config_id: 31_601,
            }),
            state,
            stacks: Some(1),
            duration_millis: Some(10_000),
            level: Some(2),
            part_id: None,
            count: None,
            created_at_millis: None,
        }
    }

    fn damage(source: EntityRef, target: EntityRef) -> DamageEvent {
        DamageEvent {
            source,
            direct_source: None,
            target,
            ability: Some(AbilityId(55)),
            amount: 100,
            actual_amount: None,
            hp_loss: None,
            shield_loss: None,
            hit_event_id: None,
            damage_source: None,
            damage_type: None,
            flags: DamageFlags::default(),
            packet: DamagePacketDetail::default(),
        }
    }

    fn actor(entity: EntityRef, character_id: &str, class_id: i32) -> ActorEvent {
        ActorEvent {
            actor: entity,
            state: ActorState::Spawned,
            entity_type_id: 10,
            kind: ActorKind::Player,
            monster_id: None,
            character_id: Some(character_id.to_owned()),
            display_name: None,
            class_id: Some(class_id),
            specialization_id: None,
            level: None,
            ability_score: None,
            weapon_item_id: None,
            weapon_breakthrough_count: None,
            seasonal_score: None,
            primary_loadout: Vec::new(),
            auxiliary_loadout: Vec::new(),
            loadout_observation: Default::default(),
        }
    }

    fn team_attributes(actor: EntityRef, raw_team_id: Vec<u8>) -> EntityAttributeEvent {
        EntityAttributeEvent {
            actor,
            update_kind: rlogs_events::EntityAttributeUpdateKind::Snapshot,
            ownership: None,
            attributes: vec![rlogs_events::EntityAttribute {
                attribute_id: ATTR_TEAM_ID,
                raw_value: raw_team_id,
                decoded: None,
            }],
        }
    }

    fn cohort_receipt(schema_version: u16, generated_by: &str) -> CohortReceipt {
        CohortReceipt {
            schema_version,
            generated_by: generated_by.to_owned(),
            deployment_id: "global".to_owned(),
            game_build: TEAM_ATTRIBUTE_INTERPRETATION_BUILD.to_owned(),
            protocol_pack_digests: vec!["sha256:test".to_owned()],
            rlogs: Vec::new(),
        }
    }

    #[test]
    fn accepts_only_reviewed_inspiration_cohort_receipt_schemas() {
        for schema_version in [18, 19] {
            validate_cohort_receipt_identity(&cohort_receipt(
                schema_version,
                "rlogs-bpsr-inspiration-proc-attribution-proof",
            ))
            .expect("reviewed receipt schema");
        }
        for schema_version in [17, 20] {
            assert!(
                validate_cohort_receipt_identity(&cohort_receipt(
                    schema_version,
                    "rlogs-bpsr-inspiration-proc-attribution-proof",
                ))
                .is_err()
            );
        }
        assert!(
            validate_cohort_receipt_identity(&cohort_receipt(19, "unreviewed-generator")).is_err()
        );
    }

    #[test]
    fn extracts_exact_and_candidate_party_effects() {
        let specs = party_effect_specs(&party_fixture()).expect("specs");
        assert_eq!(specs.len(), 3);
        assert!(specs[&31_602].reviewed_candidate_edge);
        assert!(specs[&998_542].exact_static_edge);
        assert!(specs[&998_543].reviewed_candidate_edge);
    }

    #[test]
    fn exact_build_buff_origin_is_retained_as_an_effect_family_edge() {
        let mut tracker = Tracker::new(
            party_effect_specs(&party_fixture()).unwrap(),
            FIGHT_SOURCE_ENUM_BUILD,
        );
        tracker.session_id = "s".into();
        tracker.observe_status(1, 10, &status(StatusState::Applied, Some(entity(1, 100))));
        let effect = finalize_effect_aggregate(
            tracker.effects.remove(&31_602).expect("effect"),
            FIGHT_SOURCE_ENUM_BUILD,
        );
        assert_eq!(
            effect.observed_origin_edges,
            vec![ObservedOriginEdge {
                source_type_id: 1,
                source_kind: "buff",
                source_enum_name: Some("EFightSourceBuff"),
                source_config_id: 31_601,
                child_effect_id: 31_602,
                observation_count: 1,
                exact_current_build_enum_identity: true,
            }]
        );
        assert!(!effect.provider_rdps_credit_authorized);
    }

    #[test]
    fn unknown_fight_source_type_remains_unresolved() {
        assert_eq!(
            observed_origin_edge(FIGHT_SOURCE_ENUM_BUILD, 3, 77, 88, 1),
            ObservedOriginEdge {
                source_type_id: 3,
                source_kind: "unresolved",
                source_enum_name: None,
                source_config_id: 77,
                child_effect_id: 88,
                observation_count: 1,
                exact_current_build_enum_identity: false,
            }
        );
        assert_eq!(
            observed_origin_edge("older-build", 1, 31_601, 31_602, 1).source_kind,
            "unresolved"
        );
    }

    #[test]
    fn links_affected_entity_actions_and_actions_targeting_it_without_assuming_allegiance() {
        let mut tracker = Tracker::new(
            party_effect_specs(&party_fixture()).unwrap(),
            TEAM_ATTRIBUTE_INTERPRETATION_BUILD,
        );
        tracker.session_id = "s".into();
        tracker.observe_status(1, 10, &status(StatusState::Applied, Some(entity(1, 100))));
        tracker.observe_damage(2, 20, &damage(entity(2, 200), entity(9, 900)));
        tracker.observe_damage(3, 30, &damage(entity(9, 900), entity(2, 200)));
        tracker.observe_status(4, 40, &status(StatusState::Removed, None));
        assert_eq!(tracker.windows.len(), 1);
        let window = &tracker.windows[0];
        assert_eq!(window.affected_entity_damage_actions.event_count, 1);
        assert_eq!(
            window.damage_actions_targeting_affected_entity.event_count,
            1
        );
        assert_eq!(window.effect_target_actor_id, "2");
        assert_eq!(window.damage_action_edges.len(), 2);
        assert_eq!(
            window.damage_action_edges[0].role,
            DamageActionRole::EffectTargetIsDamageActor
        );
        assert_eq!(window.damage_action_edges[0].ability_id, Some(55));
        assert_eq!(window.damage_action_edges[0].damage_target_actor_id, "9");
        assert_eq!(
            window.damage_action_edges[1].role,
            DamageActionRole::EffectTargetIsDamageTarget
        );
        assert_eq!(window.damage_action_edges[1].damage_source_actor_id, "9");
        assert_eq!(tracker.summary.window_damage_action_edges, 2);
        assert!(window.missing_source_observed);
        assert!(!window.provider_rdps_credit_authorized);
    }

    #[test]
    fn orphan_remove_is_retained_instead_of_dropped() {
        let mut tracker = Tracker::new(
            party_effect_specs(&party_fixture()).unwrap(),
            TEAM_ATTRIBUTE_INTERPRETATION_BUILD,
        );
        tracker.session_id = "s".into();
        tracker.observe_status(1, 10, &status(StatusState::Removed, None));
        assert_eq!(tracker.windows.len(), 1);
        assert!(tracker.windows[0].orphan_lifecycle_start);
        assert_eq!(tracker.effects[&31_602].orphan_lifecycle_windows, 1);
    }

    #[test]
    fn player_identity_does_not_invent_party_membership() {
        let mut tracker = Tracker::new(
            party_effect_specs(&party_fixture()).unwrap(),
            TEAM_ATTRIBUTE_INTERPRETATION_BUILD,
        );
        tracker.session_id = "s".into();
        tracker.observe_actor(&actor(entity(1, 100), "1000", 14));
        tracker.observe_actor(&actor(entity(2, 200), "2000", 11));
        tracker.observe_status(1, 10, &status(StatusState::Applied, Some(entity(1, 100))));
        let identity = &tracker.effects[&31_602].identity_evidence;
        assert_eq!(identity.external_source_affected_status_events, 1);
        assert_eq!(
            identity.external_status_events_with_both_player_identities,
            1
        );
        assert_eq!(identity.party_membership_proven_status_events, 0);
        assert_eq!(identity.party_membership_unproven_status_events, 1);
        assert_eq!(
            identity.source_character_ids,
            BTreeSet::from(["1000".into()])
        );
        assert_eq!(
            identity.affected_entity_character_ids,
            BTreeSet::from(["2000".into()])
        );
    }

    #[test]
    fn matching_exact_build_team_ids_are_retained_without_granting_membership_authority() {
        let mut tracker = Tracker::new(
            party_effect_specs(&party_fixture()).unwrap(),
            TEAM_ATTRIBUTE_INTERPRETATION_BUILD,
        );
        tracker.session_id = "s".into();
        tracker.observe_actor(&actor(entity(1, 100), "1000", 14));
        tracker.observe_actor(&actor(entity(2, 200), "2000", 11));
        tracker.observe_entity_attributes(
            1,
            1,
            &team_attributes(entity(1, 100), vec![135, 213, 167, 192, 1]),
        );
        tracker.observe_entity_attributes(
            2,
            2,
            &team_attributes(entity(2, 200), vec![135, 213, 167, 192, 1]),
        );
        tracker.observe_status(1, 10, &status(StatusState::Applied, Some(entity(1, 100))));

        let identity = &tracker.effects[&31_602].identity_evidence;
        assert_eq!(
            identity.external_status_events_with_matching_last_observed_team_id,
            1
        );
        assert_eq!(
            identity.external_status_events_with_team_id_evidence_but_protocol_coverage_open,
            1
        );
        assert_eq!(
            identity.matching_last_observed_team_ids,
            BTreeSet::from(["403303047".into()])
        );
        assert_eq!(identity.party_membership_proven_status_events, 0);
        assert_eq!(identity.party_membership_unproven_status_events, 1);
        assert_eq!(tracker.summary.team_id_attribute_events, 2);
        assert_eq!(tracker.summary.team_id_attribute_positive_values, 2);
    }

    #[test]
    fn team_id_clear_and_mismatch_fail_closed() {
        let mut tracker = Tracker::new(
            party_effect_specs(&party_fixture()).unwrap(),
            TEAM_ATTRIBUTE_INTERPRETATION_BUILD,
        );
        tracker.session_id = "s".into();
        tracker.observe_actor(&actor(entity(1, 100), "1000", 14));
        tracker.observe_actor(&actor(entity(2, 200), "2000", 11));
        tracker.observe_entity_attributes(1, 1, &team_attributes(entity(1, 100), vec![77]));
        tracker.observe_entity_attributes(2, 2, &team_attributes(entity(2, 200), vec![78]));
        tracker.observe_status(1, 10, &status(StatusState::Applied, Some(entity(1, 100))));
        assert_eq!(
            tracker.effects[&31_602]
                .identity_evidence
                .external_status_events_with_mismatching_last_observed_team_ids,
            1
        );

        tracker.observe_entity_attributes(3, 3, &team_attributes(entity(1, 100), vec![0]));
        tracker.observe_status(2, 20, &status(StatusState::Refreshed, Some(entity(1, 100))));
        let identity = &tracker.effects[&31_602].identity_evidence;
        assert_eq!(
            identity.external_status_events_with_unresolved_last_observed_team_id,
            1
        );
        assert_eq!(identity.party_membership_proven_status_events, 0);
        assert_eq!(identity.party_membership_unproven_status_events, 2);
        assert_eq!(tracker.summary.team_id_attribute_clear_values, 1);
    }

    #[test]
    fn explicit_roster_containment_is_retained_but_lifecycle_gate_stays_closed() {
        let mut tracker = Tracker::new(
            party_effect_specs(&party_fixture()).unwrap(),
            TEAM_ATTRIBUTE_INTERPRETATION_BUILD,
        );
        tracker.session_id = "s".into();
        tracker.observe_actor(&actor(entity(1, 100), "1000", 14));
        tracker.observe_actor(&actor(entity(2, 200), "2000", 11));
        let member = |character_id: &str| rlogs_events::PartyRosterMember {
            character: rlogs_events::CharacterIdentity {
                region: rlogs_events::RegionIdentity {
                    deployment_id: "global".into(),
                    region_id: "north-america".into(),
                    realm_id: None,
                    world_id: None,
                },
                character_id: character_id.into(),
            },
            enter_time: Some(10),
            online_status: Some(1),
            scene_id: Some(1_631),
            group_id: Some(2),
        };
        tracker.observe_party_roster(&PartyRosterEvent {
            observation: PartyRosterObservation::FullSnapshot {
                party_id: Some("77".into()),
                members: vec![member("1000"), member("2000")],
            },
        });
        tracker.observe_status(1, 10, &status(StatusState::Applied, Some(entity(1, 100))));

        let identity = &tracker.effects[&31_602].identity_evidence;
        assert_eq!(
            identity.external_status_events_with_both_in_observed_party_roster,
            1
        );
        assert_eq!(
            identity.external_status_events_with_roster_evidence_but_lifecycle_coverage_open,
            1
        );
        assert_eq!(identity.party_membership_proven_status_events, 0);
        assert_eq!(identity.party_membership_unproven_status_events, 1);

        tracker.observe_party_roster(&PartyRosterEvent {
            observation: PartyRosterObservation::MemberLeft {
                member: member("1000").character,
                leave_type: Some(2),
            },
        });
        assert!(!tracker.observed_party_roster_has_full_snapshot);
    }

    #[test]
    fn malformed_team_id_keeps_a_bounded_raw_example() {
        let mut tracker = Tracker::new(
            party_effect_specs(&party_fixture()).unwrap(),
            TEAM_ATTRIBUTE_INTERPRETATION_BUILD,
        );
        tracker.session_id = "s".into();
        tracker.observe_entity_attributes(9, 90, &team_attributes(entity(2, 200), vec![255; 10]));
        assert_eq!(tracker.summary.team_id_attribute_malformed_values, 1);
        assert_eq!(
            tracker.summary.team_id_attribute_malformed_examples.len(),
            1
        );
        let example = &tracker.summary.team_id_attribute_malformed_examples[0];
        assert_eq!(example.session_id, "s");
        assert_eq!(example.sequence, 9);
        assert_eq!(example.observed_micros, 90);
        assert_eq!(example.raw_value, vec![255; 10]);
    }

    #[test]
    fn array_depth_ignores_brackets_inside_strings() {
        assert_eq!(json_array_depth(r#"[{"path":"C:/[x]/a.rlog"}]"#), 0);
        assert_eq!(json_array_depth("[\n{\"a\":1}"), 1);
    }
}
