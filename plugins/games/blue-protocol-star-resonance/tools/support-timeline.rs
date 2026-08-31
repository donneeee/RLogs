#![recursion_limit = "256"]

use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{
    CanonicalEvent, EntityAttributeEvent, EntityRef, EventEnvelope, EvidenceSource,
    PartyRosterObservation, TimelineEventKind,
};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::Serialize;
use serde_json::{Value, json};

const SUPPORT_TIMELINE_SCHEMA_VERSION: u16 = 11;
const TEAM_ATTRIBUTE_INTERPRETATION_BUILD: &str = "24687926";
// Exact names and values from the current-build Zproto.EAttrType enum in
// research/game-file-inventory/global/steam-24687926/rpc-message-surface.v2.json.
const ATTR_TEAM_ID: i32 = 194;
const ATTR_TEAM_MEMBER_NUMS: i32 = 195;

#[derive(Debug)]
struct Arguments {
    rlogs: Vec<PathBuf>,
    output: PathBuf,
    effect_filter: Option<i64>,
    include_related_damage: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ActiveEffectKey {
    source_entity_uuid: Option<i64>,
    affected_entity_uuid: i64,
    status_instance_id: Option<i64>,
}

#[derive(Debug, Default, Serialize)]
struct EventCounts {
    party_roster: u64,
    party_affiliation: u64,
    cast: u64,
    cooldown: u64,
    resource: u64,
    damage: u64,
    healing: u64,
    shield: u64,
    status: u64,
    unresolved_status: u64,
}

#[derive(Debug, Default)]
struct RelationshipIds {
    source: Option<EntityRef>,
    direct_source: Option<EntityRef>,
    target: Option<EntityRef>,
    provider: Option<EntityRef>,
    affected_entity: Option<EntityRef>,
    recipient_or_enemy_target: Option<EntityRef>,
    damage_actor: Option<EntityRef>,
    damage_target: Option<EntityRef>,
    action_id: Option<i64>,
    action_instance_id: Option<i64>,
    skill_effect_group_uuid: Option<i64>,
    hit_event_id: Option<i32>,
    owner_id: Option<i32>,
    owner_level: Option<i32>,
    owner_stage: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    type_flags: Option<i32>,
    normal_value: Option<i64>,
    lucky_value: Option<i64>,
    normal_hit: Option<bool>,
    property: Option<i32>,
    passive_uuid: Option<u32>,
    rainbow: Option<bool>,
    damage_mode: Option<i32>,
    skill_effect_total_damage: Option<i64>,
    skill_effect_group_index: Option<u32>,
    skill_effect_component_index: Option<u32>,
    skill_effect_component_count: Option<u32>,
    reported_amount: Option<i64>,
    actual_amount: Option<i64>,
    hp_loss: Option<i64>,
    shield_loss: Option<i64>,
    effect_id: Option<i64>,
    status_instance_id: Option<i64>,
    status_state: Option<&'static str>,
    unresolved_status_reason: Option<&'static str>,
    wire_status_event_type: Option<i32>,
    wire_status_logic_type: Option<i32>,
    source_type_id: Option<i32>,
    source_config_id: Option<i64>,
    status_stacks: Option<u32>,
    status_duration_millis: Option<u64>,
    status_level: Option<i32>,
    status_count: Option<i32>,
    status_created_at_millis: Option<i64>,
    party_id: Option<String>,
    reported_party_member_count: Option<u64>,
    team_id_attribute_id: Option<i32>,
    team_member_count_attribute_id: Option<i32>,
    exact_build_team_attribute_interpretation: bool,
    party_membership_authority: bool,
    relationship_endpoint_role: Option<&'static str>,
    action_identity_resolution: Option<&'static str>,
    cast_observability_scope: Option<&'static str>,
    source_resolution: &'static str,
    relationship_shape: &'static str,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("support timeline failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments()?;
    if arguments.output.exists() {
        return Err(format!(
            "refusing to overwrite existing output: {}",
            arguments.output.display()
        )
        .into());
    }
    let partial = partial_path(&arguments.output)?;
    if partial.exists() {
        return Err(format!(
            "refusing to overwrite existing partial output: {}",
            partial.display()
        )
        .into());
    }
    if let Some(parent) = arguments.output.parent() {
        fs::create_dir_all(parent)?;
    }
    let output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)?;
    let mut writer = BufWriter::new(output);
    write_json_line(
        &mut writer,
        &json!({
            "schema_version": SUPPORT_TIMELINE_SCHEMA_VERSION,
            "row_type": "manifest",
            "projection": "canonical-who-did-what-id-to-which-target-timeline",
            "topology": {
                "effect_edge": "provider -> effect/status lifecycle -> recipient or enemy target",
                "damage_edge": "recipient damage action -> recipient or enemy target",
                "source_side_join": "effect endpoint equals damage actor",
                "target_side_join": "effect endpoint equals damage target",
                "effect_endpoint_allegiance_assumed": false,
                "damage_endpoint_allegiance_assumed": false
            },
            "policy": {
                "exact_numeric_ids_and_build_are_authoritative": true,
                "localized_names_are_runtime_keys": false,
                "remote_player_cast_packets_required": false,
                "remote_player_cast_packets_treated_as_zero": false,
                "remote_player_cast_packets_synthesized": false,
                "status_rows_without_wire_source_are_preserved_with_null_source": true,
                "status_target_is_projected_as_allegiance_neutral_affected_entity": true,
                "status_target_is_projected_as_recipient_or_enemy_target": true,
                "status_affected_entity_role_requires_event_time_evidence": true,
                "affected_entity_is_assumed_friendly": false,
                "affected_entity_is_assumed_enemy": false,
                "damage_target_is_assumed_friendly": false,
                "damage_target_is_assumed_enemy": false,
                "entity_role_and_party_membership_require_event_time_evidence": true,
                "damage_actor_and_damage_target_are_preserved_without_allegiance_assumptions": true,
                "damage_endpoint_is_projected_as_recipient_or_enemy_target": true,
                "source_side_effect_relationship_requires_effect_target_equal_damage_actor": true,
                "target_side_effect_relationship_requires_effect_target_equal_damage_target": true,
                "cast_instance_ancestry_may_be_unobservable": true,
                "unknown_effects_are_preserved": true,
                "unresolved_status_lifecycles_are_combat_evidence_not_global_data_gaps": true,
                "party_roster_snapshots_and_deltas_are_preserved_without_completion_inference": true,
                "party_roster_lifecycle_route_coverage_proven": false,
                "team_attribute_interpretation_is_exact_build_gated": true,
                "team_attribute_interpretation_build": TEAM_ATTRIBUTE_INTERPRETATION_BUILD,
                "team_id_attribute_id": ATTR_TEAM_ID,
                "team_member_count_attribute_id": ATTR_TEAM_MEMBER_NUMS,
                "matching_team_attributes_alone_grant_party_membership_authority": false,
                "current_character_snapshots_substituted_into_older_runs": false,
                "relationship_projection_changes_canonical_events": false,
                "canonical_event_payload_duplicated_in_projection": false,
                "canonical_source_rlog_and_sequence_are_retained": true,
                "compact_projection_is_lossless_canonical_event_storage": false,
                "provider_credit_authorized_by_timeline_presence_alone": false,
                "packet_owner_stage_is_zero_based_stage_index_not_stage_type": true,
                "missing_packet_stage_or_level_is_preserved_as_null_not_zero": true,
                "packet_damage_stage_fields_grant_formula_authority": false
            },
            "event_kinds": ["party_roster", "party_affiliation", "cast", "cooldown", "resource", "damage", "healing", "shield", "status", "unresolved_status"],
            "projection_filter": {
                "effect_id": arguments.effect_filter,
                "include_related_damage": arguments.include_related_damage,
                "relationship_rows_only": arguments.effect_filter.is_some(),
                "canonical_source_rlogs_remain_complete": true
            },
            "rlog_count": arguments.rlogs.len()
        }),
    )?;

    for path in arguments.rlogs {
        project_rlog(
            &path,
            arguments.effect_filter,
            arguments.include_related_damage,
            &mut writer,
        )?;
    }
    writer.flush()?;
    writer.get_ref().sync_all()?;
    drop(writer);
    fs::rename(&partial, &arguments.output)?;
    Ok(())
}

fn project_rlog(
    path: &Path,
    effect_filter: Option<i64>,
    include_related_damage: bool,
    writer: &mut impl Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
    let session_id = reader.header().session_id.clone();
    let deployment_id = reader.header().region.identity.deployment_id.clone();
    let client_build = reader.header().region.client_build.clone();
    let protocol_pack_digest = reader.header().region.protocol_pack_digest.clone();
    write_json_line(
        writer,
        &json!({
            "schema_version": SUPPORT_TIMELINE_SCHEMA_VERSION,
            "row_type": "run_header",
            "source_path": path.display().to_string(),
            "session_id": session_id,
            "deployment_id": deployment_id,
            "client_build": client_build,
            "protocol_pack_digest": protocol_pack_digest
        }),
    )?;

    let mut canonical_events = 0_u64;
    let mut relationship_rows = 0_u64;
    let mut counts = EventCounts::default();
    let mut status_effect_counts = BTreeMap::<i64, u64>::new();
    let mut missing_status_source_rows = 0_u64;
    let mut unresolved_status_rows = 0_u64;
    let mut active_filtered_effects = BTreeSet::<ActiveEffectKey>::new();
    while let Some(envelope) = reader.next_event()? {
        canonical_events = canonical_events.saturating_add(1);
        let filtered_effect_join = match effect_filter {
            Some(effect_id) => {
                let Some(join) = filtered_relationship_join(
                    &envelope.event,
                    effect_id,
                    include_related_damage,
                    &active_filtered_effects,
                ) else {
                    continue;
                };
                join
            }
            None => None,
        };
        let Some(row) = relationship_row(
            &session_id,
            &client_build,
            &envelope,
            effect_filter,
            filtered_effect_join,
            &mut counts,
        ) else {
            continue;
        };
        if let CanonicalEvent::Timeline(timeline) = &envelope.event
            && let TimelineEventKind::Status(status) = &timeline.kind
        {
            *status_effect_counts.entry(status.effect.0).or_default() += 1;
            if status.source.is_none() {
                missing_status_source_rows = missing_status_source_rows.saturating_add(1);
            }
        }
        if let CanonicalEvent::Timeline(timeline) = &envelope.event
            && matches!(&timeline.kind, TimelineEventKind::UnresolvedStatus(_))
        {
            unresolved_status_rows = unresolved_status_rows.saturating_add(1);
        }
        write_json_line(writer, &row)?;
        relationship_rows = relationship_rows.saturating_add(1);
        if let Some(effect_id) = effect_filter {
            update_active_filtered_effects(
                &envelope.event,
                effect_id,
                &mut active_filtered_effects,
            );
        }
    }
    if reader.summary().is_none() {
        return Err(format!("{} is not a sealed canonical rlog", path.display()).into());
    }
    write_json_line(
        writer,
        &json!({
            "schema_version": SUPPORT_TIMELINE_SCHEMA_VERSION,
            "row_type": "run_summary",
            "source_path": path.display().to_string(),
            "session_id": session_id,
            "deployment_id": deployment_id,
            "client_build": client_build,
            "protocol_pack_digest": protocol_pack_digest,
            "sealed_canonical_rlog": true,
            "canonical_events": canonical_events,
            "relationship_rows": relationship_rows,
            "event_counts": counts,
            "status_effect_counts": status_effect_counts,
            "status_rows_with_unresolved_wire_source": missing_status_source_rows,
            "unresolved_status_lifecycle_rows": unresolved_status_rows,
            "filtered_effect_id": effect_filter,
            "filtered_effect_open_lifecycle_count": active_filtered_effects.len(),
            "remote_cast_rows_synthesized": 0
        }),
    )?;
    Ok(())
}

fn relationship_row(
    session_id: &str,
    client_build: &str,
    envelope: &EventEnvelope,
    filtered_effect_id: Option<i64>,
    filtered_effect_join: Option<&'static str>,
    counts: &mut EventCounts,
) -> Option<Value> {
    if let CanonicalEvent::PartyRosterObserved(event) = &envelope.event {
        counts.party_roster = counts.party_roster.saturating_add(1);
        let (lifecycle, party_id, members, leave_type, relationship_shape) =
            match &event.observation {
                PartyRosterObservation::FullSnapshot { party_id, members } => (
                    "full_snapshot",
                    party_id.clone(),
                    members
                        .iter()
                        .map(|member| member.character.character_id.clone())
                        .collect::<Vec<_>>(),
                    None,
                    "party-to-full-roster-snapshot",
                ),
                PartyRosterObservation::MembersObserved { members } => (
                    "members_observed",
                    None,
                    members
                        .iter()
                        .map(|member| member.character.character_id.clone())
                        .collect::<Vec<_>>(),
                    None,
                    "party-to-partial-member-observation",
                ),
                PartyRosterObservation::MemberLeft { member, leave_type } => (
                    "member_left",
                    None,
                    vec![member.character_id.clone()],
                    *leave_type,
                    "party-member-to-leave-observation",
                ),
                PartyRosterObservation::Dissolved => (
                    "dissolved",
                    None,
                    Vec::new(),
                    None,
                    "party-to-dissolve-observation",
                ),
            };
        return Some(json!({
            "schema_version": SUPPORT_TIMELINE_SCHEMA_VERSION,
            "row_type": "relationship",
            "session_id": session_id,
            "sequence": envelope.sequence,
            "capture_sequence": capture_sequence(&envelope.provenance.source),
            "observed_micros": envelope.time.observed_micros,
            "game_time_millis": envelope.time.game_time_millis,
            "event_kind": "party_roster",
            "party_roster_lifecycle": lifecycle,
            "party_id": party_id,
            "member_character_ids": members,
            "leave_type": leave_type,
            "source_resolution": "exact-canonical-party-roster-observation",
            "relationship_shape": relationship_shape,
            "canonical_source_rlog_sequence": envelope.sequence,
            "canonical_event_payload_retained_in_source_rlog": true
        }));
    }
    let CanonicalEvent::Timeline(timeline) = &envelope.event else {
        return None;
    };
    let (event_kind, ids) = relationship_ids(client_build, &timeline.kind, counts)?;
    Some(json!({
        "schema_version": SUPPORT_TIMELINE_SCHEMA_VERSION,
        "row_type": "relationship",
        "session_id": session_id,
        "sequence": envelope.sequence,
        "capture_sequence": capture_sequence(&envelope.provenance.source),
        "observed_micros": envelope.time.observed_micros,
        "game_time_millis": envelope.time.game_time_millis,
        "event_kind": event_kind,
        "source_actor_id": actor_id(ids.source),
        "source_entity_uuid": entity_uuid(ids.source),
        "direct_source_actor_id": actor_id(ids.direct_source),
        "direct_source_entity_uuid": entity_uuid(ids.direct_source),
        "target_actor_id": actor_id(ids.target),
        "target_entity_uuid": entity_uuid(ids.target),
        "provider_actor_id": actor_id(ids.provider),
        "provider_entity_uuid": entity_uuid(ids.provider),
        "affected_entity_actor_id": actor_id(ids.affected_entity),
        "affected_entity_uuid": entity_uuid(ids.affected_entity),
        "recipient_or_enemy_target_actor_id": actor_id(ids.recipient_or_enemy_target),
        "recipient_or_enemy_target_entity_uuid": entity_uuid(ids.recipient_or_enemy_target),
        "damage_actor_id": actor_id(ids.damage_actor),
        "damage_actor_entity_uuid": entity_uuid(ids.damage_actor),
        "damage_target_actor_id": actor_id(ids.damage_target),
        "damage_target_entity_uuid": entity_uuid(ids.damage_target),
        "action_id": ids.action_id,
        "action_instance_id": ids.action_instance_id,
        "skill_effect_group_uuid": ids.skill_effect_group_uuid.map(|value| value.to_string()),
        "hit_event_id": ids.hit_event_id,
        "owner_id": ids.owner_id,
        "owner_level": ids.owner_level,
        "owner_stage": ids.owner_stage,
        "damage_source": ids.damage_source,
        "damage_type": ids.damage_type,
        "type_flags": ids.type_flags,
        "normal_value": ids.normal_value,
        "lucky_value": ids.lucky_value,
        "normal_hit": ids.normal_hit,
        "property": ids.property,
        "passive_uuid": ids.passive_uuid,
        "rainbow": ids.rainbow,
        "damage_mode": ids.damage_mode,
        "skill_effect_total_damage": ids.skill_effect_total_damage,
        "skill_effect_group_index": ids.skill_effect_group_index,
        "skill_effect_component_index": ids.skill_effect_component_index,
        "skill_effect_component_count": ids.skill_effect_component_count,
        "reported_amount": ids.reported_amount,
        "actual_amount": ids.actual_amount,
        "hp_loss": ids.hp_loss,
        "shield_loss": ids.shield_loss,
        "effect_id": ids.effect_id,
        "status_instance_id": ids.status_instance_id,
        "status_state": ids.status_state,
        "unresolved_status_reason": ids.unresolved_status_reason,
        "wire_status_event_type": ids.wire_status_event_type,
        "wire_status_logic_type": ids.wire_status_logic_type,
        "source_type_id": ids.source_type_id,
        "source_config_id": ids.source_config_id,
        "status_stacks": ids.status_stacks,
        "status_duration_millis": ids.status_duration_millis,
        "status_level": ids.status_level,
        "status_count": ids.status_count,
        "status_created_at_millis": ids.status_created_at_millis,
        "party_id": ids.party_id,
        "reported_party_member_count": ids.reported_party_member_count,
        "team_id_attribute_id": ids.team_id_attribute_id,
        "team_member_count_attribute_id": ids.team_member_count_attribute_id,
        "exact_build_team_attribute_interpretation": ids.exact_build_team_attribute_interpretation,
        "party_membership_authority": ids.party_membership_authority,
        "relationship_endpoint_role": ids.relationship_endpoint_role,
        "action_identity_resolution": ids.action_identity_resolution,
        "cast_observability_scope": ids.cast_observability_scope,
        "source_resolution": ids.source_resolution,
        "relationship_shape": ids.relationship_shape,
        "filtered_effect_id": filtered_effect_id,
        "filtered_effect_join": filtered_effect_join,
        "canonical_source_rlog_sequence": envelope.sequence,
        "canonical_event_payload_retained_in_source_rlog": true
    }))
}

fn filtered_relationship_join(
    event: &CanonicalEvent,
    effect_id: i64,
    include_related_damage: bool,
    active_effects: &BTreeSet<ActiveEffectKey>,
) -> Option<Option<&'static str>> {
    let CanonicalEvent::Timeline(timeline) = event else {
        return None;
    };
    filtered_timeline_relationship_join(
        &timeline.kind,
        effect_id,
        include_related_damage,
        active_effects,
    )
}

fn filtered_timeline_relationship_join(
    kind: &TimelineEventKind,
    effect_id: i64,
    include_related_damage: bool,
    active_effects: &BTreeSet<ActiveEffectKey>,
) -> Option<Option<&'static str>> {
    match kind {
        TimelineEventKind::Status(status) if status.effect.0 == effect_id => Some(None),
        TimelineEventKind::Damage(damage) if include_related_damage => {
            let source_side = active_effects
                .iter()
                .any(|active| active.affected_entity_uuid == damage.source.entity_uuid.0);
            let target_side = active_effects
                .iter()
                .any(|active| active.affected_entity_uuid == damage.target.entity_uuid.0);
            match (source_side, target_side) {
                (true, true) => Some(Some("source-and-target-side")),
                (true, false) => Some(Some("source-side-effect-endpoint-equals-damage-actor")),
                (false, true) => Some(Some("target-side-effect-endpoint-equals-damage-target")),
                (false, false) => None,
            }
        }
        _ => None,
    }
}

fn update_active_filtered_effects(
    event: &CanonicalEvent,
    effect_id: i64,
    active_effects: &mut BTreeSet<ActiveEffectKey>,
) {
    let CanonicalEvent::Timeline(timeline) = event else {
        return;
    };
    update_active_filtered_timeline_effects(&timeline.kind, effect_id, active_effects);
}

fn update_active_filtered_timeline_effects(
    kind: &TimelineEventKind,
    effect_id: i64,
    active_effects: &mut BTreeSet<ActiveEffectKey>,
) {
    let TimelineEventKind::Status(status) = kind else {
        return;
    };
    if status.effect.0 != effect_id {
        return;
    }
    let key = ActiveEffectKey {
        source_entity_uuid: status.source.map(|source| source.entity_uuid.0),
        affected_entity_uuid: status.target.entity_uuid.0,
        status_instance_id: status.instance_id.map(|instance| instance.0),
    };
    match status.state {
        rlogs_events::StatusState::Applied => {
            active_effects.insert(key);
        }
        rlogs_events::StatusState::Removed | rlogs_events::StatusState::Consumed => {
            active_effects.remove(&key);
        }
        rlogs_events::StatusState::Refreshed | rlogs_events::StatusState::Stacked => {}
    }
}

fn relationship_ids(
    client_build: &str,
    kind: &TimelineEventKind,
    counts: &mut EventCounts,
) -> Option<(&'static str, RelationshipIds)> {
    match kind {
        TimelineEventKind::EntityAttributes(event) => {
            party_affiliation_relationship(client_build, event, counts)
        }
        TimelineEventKind::Cast(event) => {
            counts.cast = counts.cast.saturating_add(1);
            Some((
                "cast",
                RelationshipIds {
                    source: Some(event.source),
                    target: event.target,
                    action_id: Some(event.ability.0),
                    action_instance_id: event.action_timing.map(|timing| timing.action_instance_id),
                    recipient_or_enemy_target: event.target,
                    relationship_endpoint_role: event
                        .target
                        .map(|_| "recipient-or-enemy-target-unresolved"),
                    action_identity_resolution: Some(if event.action_timing.is_some() {
                        "exact-local-action-instance"
                    } else {
                        "ability-only-action-instance-unobserved"
                    }),
                    cast_observability_scope: Some(if event.action_timing.is_some() {
                        "local-client-use-slot"
                    } else {
                        "local-or-server-cast-without-action-instance"
                    }),
                    source_resolution: "wire-source-present",
                    relationship_shape: "source-to-action-to-optional-recipient-or-enemy-target",
                    ..Default::default()
                },
            ))
        }
        TimelineEventKind::Cooldown(event) => {
            counts.cooldown = counts.cooldown.saturating_add(1);
            Some((
                "cooldown",
                RelationshipIds {
                    source: Some(event.actor),
                    action_id: Some(event.ability.0),
                    source_resolution: "wire-source-present",
                    relationship_shape: "source-to-action",
                    ..Default::default()
                },
            ))
        }
        TimelineEventKind::Resource(event) => {
            counts.resource = counts.resource.saturating_add(1);
            Some((
                "resource",
                RelationshipIds {
                    source: Some(event.actor),
                    source_resolution: "wire-source-present",
                    relationship_shape: "source-resource-state",
                    ..Default::default()
                },
            ))
        }
        TimelineEventKind::Damage(event) => {
            counts.damage = counts.damage.saturating_add(1);
            Some((
                "damage",
                RelationshipIds {
                    source: Some(event.source),
                    direct_source: event.direct_source,
                    target: Some(event.target),
                    damage_actor: Some(event.source),
                    damage_target: Some(event.target),
                    recipient_or_enemy_target: Some(event.target),
                    action_id: event.ability.map(|id| id.0),
                    skill_effect_group_uuid: event.packet.skill_effect_uuid,
                    hit_event_id: event.hit_event_id,
                    owner_id: event.packet.owner_id,
                    owner_level: event.packet.owner_level,
                    owner_stage: event.packet.owner_stage,
                    damage_source: event.damage_source,
                    damage_type: event.damage_type,
                    type_flags: event.packet.type_flags,
                    normal_value: event.packet.normal_value,
                    lucky_value: event.packet.lucky_value,
                    normal_hit: event.packet.normal_hit,
                    property: event.packet.property,
                    passive_uuid: event.packet.passive_uuid,
                    rainbow: event.packet.rainbow,
                    damage_mode: event.packet.damage_mode,
                    skill_effect_total_damage: event.packet.skill_effect_total_damage,
                    skill_effect_group_index: event.packet.skill_effect_group_index,
                    skill_effect_component_index: event.packet.skill_effect_component_index,
                    skill_effect_component_count: event.packet.skill_effect_component_count,
                    reported_amount: Some(event.amount),
                    actual_amount: event.actual_amount,
                    hp_loss: event.hp_loss,
                    shield_loss: event.shield_loss,
                    relationship_endpoint_role: Some("recipient-or-enemy-target-unresolved"),
                    action_identity_resolution: Some(if event.ability.is_some() {
                        "server-reported-damage-ability-action-instance-unobserved"
                    } else {
                        "damage-action-identity-unresolved"
                    }),
                    cast_observability_scope: Some("not-required-for-damage-action-row"),
                    source_resolution: "canonical-owner-or-top-summoner",
                    relationship_shape: "recipient-damage-action-to-recipient-or-enemy-target",
                    ..Default::default()
                },
            ))
        }
        TimelineEventKind::Healing(event) => {
            counts.healing = counts.healing.saturating_add(1);
            Some((
                "healing",
                RelationshipIds {
                    source: Some(event.source),
                    direct_source: event.direct_source,
                    target: Some(event.target),
                    action_id: event.ability.map(|id| id.0),
                    skill_effect_group_uuid: event.packet.skill_effect_uuid,
                    hit_event_id: event.hit_event_id,
                    reported_amount: Some(event.amount),
                    actual_amount: event.actual_amount,
                    hp_loss: event.hp_loss,
                    shield_loss: event.shield_loss,
                    source_resolution: "canonical-owner-or-top-summoner",
                    relationship_shape: "source-to-healing-action-to-target",
                    ..Default::default()
                },
            ))
        }
        TimelineEventKind::Shield(event) => {
            counts.shield = counts.shield.saturating_add(1);
            Some((
                "shield",
                RelationshipIds {
                    source: Some(event.source),
                    target: Some(event.target),
                    action_id: Some(event.ability.0),
                    source_resolution: "wire-source-present",
                    relationship_shape: "source-to-shield-action-to-target",
                    ..Default::default()
                },
            ))
        }
        TimelineEventKind::Status(event) => {
            counts.status = counts.status.saturating_add(1);
            Some((
                "status",
                RelationshipIds {
                    source: event.source,
                    provider: event.source,
                    affected_entity: Some(event.target),
                    recipient_or_enemy_target: Some(event.target),
                    effect_id: Some(event.effect.0),
                    status_instance_id: event.instance_id.map(|instance| instance.0),
                    status_state: Some(status_state_label(event.state)),
                    source_type_id: event.origin.map(|origin| origin.source_type_id),
                    source_config_id: event.origin.map(|origin| origin.source_config_id),
                    status_stacks: event.stacks,
                    status_duration_millis: event.duration_millis,
                    status_level: event.level,
                    status_count: event.count,
                    status_created_at_millis: event.created_at_millis,
                    source_resolution: if event.source.is_some() {
                        "wire-source-present"
                    } else {
                        "unresolved-missing-wire-source"
                    },
                    relationship_endpoint_role: Some("recipient-or-enemy-target-unresolved"),
                    relationship_shape: "provider-to-effect-lifecycle-to-recipient-or-enemy-target",
                    ..Default::default()
                },
            ))
        }
        TimelineEventKind::UnresolvedStatus(event) => {
            counts.unresolved_status = counts.unresolved_status.saturating_add(1);
            Some((
                "unresolved_status",
                RelationshipIds {
                    source: event.source,
                    provider: event.source,
                    affected_entity: Some(event.target),
                    recipient_or_enemy_target: Some(event.target),
                    effect_id: None,
                    status_instance_id: event.instance_id.map(|instance| instance.0),
                    status_state: event.state.map(status_state_label),
                    unresolved_status_reason: Some(unresolved_status_reason_label(event.reason)),
                    wire_status_event_type: event.wire_event_type,
                    wire_status_logic_type: event.wire_logic_type,
                    source_resolution: if event.source.is_some() {
                        "wire-source-present-effect-identity-unresolved"
                    } else {
                        "unresolved-effect-identity-and-wire-source"
                    },
                    relationship_endpoint_role: Some("recipient-or-enemy-target-unresolved"),
                    relationship_shape: "provider-to-unresolved-effect-lifecycle-to-recipient-or-enemy-target",
                    ..Default::default()
                },
            ))
        }
        _ => None,
    }
}

fn status_state_label(state: rlogs_events::StatusState) -> &'static str {
    match state {
        rlogs_events::StatusState::Applied => "applied",
        rlogs_events::StatusState::Refreshed => "refreshed",
        rlogs_events::StatusState::Stacked => "stacked",
        rlogs_events::StatusState::Consumed => "consumed",
        rlogs_events::StatusState::Removed => "removed",
    }
}

fn unresolved_status_reason_label(reason: rlogs_events::UnresolvedStatusReason) -> &'static str {
    match reason {
        rlogs_events::UnresolvedStatusReason::MissingInstanceId => "missing_instance_id",
        rlogs_events::UnresolvedStatusReason::MissingEffectId => "missing_effect_id",
        rlogs_events::UnresolvedStatusReason::MissingActiveEffectMapping => {
            "missing_active_effect_mapping"
        }
        rlogs_events::UnresolvedStatusReason::PayloadDecodeFailed => "payload_decode_failed",
        rlogs_events::UnresolvedStatusReason::AmbiguousTransition => "ambiguous_transition",
    }
}

fn party_affiliation_relationship(
    client_build: &str,
    event: &EntityAttributeEvent,
    counts: &mut EventCounts,
) -> Option<(&'static str, RelationshipIds)> {
    let team_id_attribute = event
        .attributes
        .iter()
        .find(|attribute| attribute.attribute_id == ATTR_TEAM_ID);
    let member_count_attribute = event
        .attributes
        .iter()
        .find(|attribute| attribute.attribute_id == ATTR_TEAM_MEMBER_NUMS);
    if team_id_attribute.is_none() && member_count_attribute.is_none() {
        return None;
    }

    counts.party_affiliation = counts.party_affiliation.saturating_add(1);
    let exact_build = client_build == TEAM_ATTRIBUTE_INTERPRETATION_BUILD;
    let raw_team_id =
        team_id_attribute.and_then(|attribute| decode_unsigned_varint_exact(&attribute.raw_value));
    let raw_member_count = member_count_attribute
        .and_then(|attribute| decode_unsigned_varint_exact(&attribute.raw_value));
    let positive_team_id = exact_build
        .then_some(raw_team_id)
        .flatten()
        .filter(|value| *value > 0);
    let cleared = exact_build && raw_team_id == Some(0);
    let (source_resolution, relationship_shape) = if positive_team_id.is_some() {
        (
            "exact-build-raw-team-id-attribute",
            "entity-to-event-time-party-affiliation-observation",
        )
    } else if cleared {
        (
            "exact-build-raw-team-id-attribute",
            "entity-to-party-affiliation-clear-observation",
        )
    } else if exact_build {
        (
            "unresolved-malformed-or-missing-exact-build-team-id-value",
            "entity-to-unresolved-party-affiliation-observation",
        )
    } else {
        (
            "unresolved-unversioned-build-team-attribute",
            "entity-to-unresolved-party-affiliation-observation",
        )
    };
    Some((
        "party_affiliation",
        RelationshipIds {
            source: Some(event.actor),
            target: Some(event.actor),
            party_id: positive_team_id.map(|value| value.to_string()),
            reported_party_member_count: exact_build.then_some(raw_member_count).flatten(),
            team_id_attribute_id: team_id_attribute.map(|_| ATTR_TEAM_ID),
            team_member_count_attribute_id: member_count_attribute.map(|_| ATTR_TEAM_MEMBER_NUMS),
            exact_build_team_attribute_interpretation: exact_build,
            party_membership_authority: false,
            source_resolution,
            relationship_shape,
            ..Default::default()
        },
    ))
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

fn actor_id(entity: Option<EntityRef>) -> Option<String> {
    entity.map(|entity| entity.actor_id.0.to_string())
}

fn entity_uuid(entity: Option<EntityRef>) -> Option<String> {
    entity.map(|entity| entity.entity_uuid.0.to_string())
}

fn capture_sequence(source: &EvidenceSource) -> Option<u64> {
    match source {
        EvidenceSource::Wire {
            capture_sequence, ..
        } => Some(*capture_sequence),
        _ => None,
    }
}

fn write_json_line(writer: &mut impl Write, value: &impl Serialize) -> std::io::Result<()> {
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")
}

fn arguments() -> Result<Arguments, String> {
    let mut values = env::args_os().skip(1).collect::<Vec<_>>();
    let output = take_value(&mut values, "--output")?;
    let effect_filter = take_optional_value(&mut values, "--effect-id")?
        .map(|value| {
            value
                .to_string_lossy()
                .parse::<i64>()
                .map_err(|_| "--effect-id requires a numeric effect ID".to_owned())
        })
        .transpose()?;
    let include_related_damage = take_flag(&mut values, "--include-related-damage");
    if include_related_damage && effect_filter.is_none() {
        return Err("--include-related-damage requires --effect-id".into());
    }
    let mut rlogs = BTreeSet::new();
    while let Some(position) = values.iter().position(|value| value == "--rlog") {
        values.remove(position);
        if position >= values.len() {
            return Err("--rlog requires a value".into());
        }
        rlogs.insert(PathBuf::from(values.remove(position)));
    }
    while let Some(position) = values.iter().position(|value| value == "--rlog-dir") {
        values.remove(position);
        if position >= values.len() {
            return Err("--rlog-dir requires a value".into());
        }
        collect_rlogs(&PathBuf::from(values.remove(position)), &mut rlogs)?;
    }
    if rlogs.is_empty() || !values.is_empty() {
        return Err(usage());
    }
    Ok(Arguments {
        rlogs: rlogs.into_iter().collect(),
        output: PathBuf::from(output),
        effect_filter,
        include_related_damage,
    })
}

fn collect_rlogs(directory: &Path, output: &mut BTreeSet<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(directory).map_err(|error| {
        format!(
            "cannot read rlog directory {}: {error}",
            directory.display()
        )
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        if file_type.is_dir() {
            collect_rlogs(&path, output)?;
        } else if file_type.is_file() && is_sealed_rlog_candidate(&path) {
            output.insert(path);
        }
    }
    Ok(())
}

fn is_sealed_rlog_candidate(path: &Path) -> bool {
    path.extension().and_then(|value| value.to_str()) == Some("rlog")
        && !path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.ends_with(".partial.rlog"))
}

fn take_value(values: &mut Vec<OsString>, flag: &str) -> Result<OsString, String> {
    take_optional_value(values, flag)?.ok_or_else(usage)
}

fn take_optional_value(values: &mut Vec<OsString>, flag: &str) -> Result<Option<OsString>, String> {
    let Some(position) = values.iter().position(|value| value == flag) else {
        return Ok(None);
    };
    values.remove(position);
    if position >= values.len() {
        return Err(format!("{flag} requires a value"));
    }
    Ok(Some(values.remove(position)))
}

fn take_flag(values: &mut Vec<OsString>, flag: &str) -> bool {
    let Some(position) = values.iter().position(|value| value == flag) else {
        return false;
    };
    values.remove(position);
    true
}

fn partial_path(output: &Path) -> Result<PathBuf, String> {
    let file_name = output
        .file_name()
        .ok_or_else(|| format!("output has no file name: {}", output.display()))?;
    let mut partial_name = file_name.to_os_string();
    partial_name.push(".partial");
    Ok(output.with_file_name(partial_name))
}

fn usage() -> String {
    "usage: rlogs-bpsr-support-timeline (--rlog <sealed.rlog> | --rlog-dir <directory>)... --output <timeline.jsonl> [--effect-id <id> [--include-related-damage]]".into()
}

#[cfg(test)]
mod tests {
    use rlogs_events::{
        AbilityId, ActorId, DamageEvent, DamageFlags, DamagePacketDetail, EntityUuid,
        StatusEffectId, StatusEffectInstanceId, StatusEvent, StatusOrigin, StatusState,
        UnresolvedStatusEvent, UnresolvedStatusReason,
    };

    use super::*;

    fn entity(actor_id: u64, entity_uuid: i64) -> EntityRef {
        EntityRef {
            actor_id: ActorId(actor_id),
            entity_uuid: EntityUuid(entity_uuid),
        }
    }

    #[test]
    fn status_without_source_remains_explicitly_unresolved() {
        let mut counts = EventCounts::default();
        let event = TimelineEventKind::Status(StatusEvent {
            source: None,
            target: entity(2, 200),
            effect: StatusEffectId(31_602),
            instance_id: None,
            origin: Some(StatusOrigin {
                source_type_id: 1,
                source_config_id: 1_410,
            }),
            state: StatusState::Applied,
            stacks: Some(1),
            duration_millis: Some(10_000),
            level: Some(2),
            part_id: None,
            count: None,
            created_at_millis: None,
        });
        let (kind, ids) =
            relationship_ids(TEAM_ATTRIBUTE_INTERPRETATION_BUILD, &event, &mut counts)
                .expect("status row");
        assert_eq!(kind, "status");
        assert_eq!(ids.source, None);
        assert_eq!(ids.provider, None);
        assert_eq!(ids.target, None);
        assert_eq!(ids.affected_entity, Some(entity(2, 200)));
        assert_eq!(ids.recipient_or_enemy_target, Some(entity(2, 200)));
        assert_eq!(ids.effect_id, Some(31_602));
        assert_eq!(ids.status_instance_id, None);
        assert_eq!(ids.status_state, Some("applied"));
        assert_eq!(ids.source_config_id, Some(1_410));
        assert_eq!(ids.source_resolution, "unresolved-missing-wire-source");
        assert_eq!(
            ids.relationship_shape,
            "provider-to-effect-lifecycle-to-recipient-or-enemy-target"
        );
        assert_eq!(
            ids.relationship_endpoint_role,
            Some("recipient-or-enemy-target-unresolved")
        );
        assert_eq!(counts.status, 1);
    }

    #[test]
    fn unresolved_effect_lifecycle_stays_in_allegiance_neutral_relationship_timeline() {
        let mut counts = EventCounts::default();
        let event = TimelineEventKind::UnresolvedStatus(UnresolvedStatusEvent {
            source: Some(entity(1, 100)),
            target: entity(2, 200),
            instance_id: Some(StatusEffectInstanceId(999)),
            state: Some(StatusState::Removed),
            wire_event_type: Some(2),
            wire_logic_type: None,
            reason: UnresolvedStatusReason::MissingActiveEffectMapping,
            raw_payload: vec![],
        });
        let (kind, ids) =
            relationship_ids(TEAM_ATTRIBUTE_INTERPRETATION_BUILD, &event, &mut counts)
                .expect("unresolved status row");
        assert_eq!(kind, "unresolved_status");
        assert_eq!(ids.provider, Some(entity(1, 100)));
        assert_eq!(ids.affected_entity, Some(entity(2, 200)));
        assert_eq!(ids.recipient_or_enemy_target, Some(entity(2, 200)));
        assert_eq!(ids.effect_id, None);
        assert_eq!(ids.status_instance_id, Some(999));
        assert_eq!(ids.status_state, Some("removed"));
        assert_eq!(
            ids.unresolved_status_reason,
            Some("missing_active_effect_mapping")
        );
        assert_eq!(ids.wire_status_event_type, Some(2));
        assert_eq!(ids.wire_status_logic_type, None);
        assert_eq!(
            ids.relationship_shape,
            "provider-to-unresolved-effect-lifecycle-to-recipient-or-enemy-target"
        );
        assert_eq!(counts.unresolved_status, 1);
    }

    #[test]
    fn damage_preserves_owner_direct_source_action_and_target() {
        let mut counts = EventCounts::default();
        let event = TimelineEventKind::Damage(DamageEvent {
            source: entity(1, 100),
            direct_source: Some(entity(9, 900)),
            target: entity(2, 200),
            ability: Some(AbilityId(55_001)),
            amount: 123,
            actual_amount: Some(123),
            hp_loss: Some(123),
            shield_loss: Some(0),
            hit_event_id: Some(4),
            damage_source: None,
            damage_type: None,
            flags: DamageFlags::default(),
            packet: DamagePacketDetail {
                owner_id: Some(55_001),
                owner_level: Some(30),
                owner_stage: Some(4),
                type_flags: Some(1),
                normal_value: Some(123),
                lucky_value: Some(0),
                property: Some(6),
                skill_effect_group_index: Some(2),
                skill_effect_component_index: Some(1),
                skill_effect_component_count: Some(3),
                ..Default::default()
            },
        });
        let (kind, ids) =
            relationship_ids(TEAM_ATTRIBUTE_INTERPRETATION_BUILD, &event, &mut counts)
                .expect("damage row");
        assert_eq!(kind, "damage");
        assert_eq!(ids.source, Some(entity(1, 100)));
        assert_eq!(ids.direct_source, Some(entity(9, 900)));
        assert_eq!(ids.target, Some(entity(2, 200)));
        assert_eq!(ids.damage_actor, Some(entity(1, 100)));
        assert_eq!(ids.damage_target, Some(entity(2, 200)));
        assert_eq!(ids.recipient_or_enemy_target, Some(entity(2, 200)));
        assert_eq!(ids.affected_entity, None);
        assert_eq!(ids.action_id, Some(55_001));
        assert_eq!(ids.owner_id, Some(55_001));
        assert_eq!(ids.owner_level, Some(30));
        assert_eq!(ids.owner_stage, Some(4));
        assert_eq!(ids.type_flags, Some(1));
        assert_eq!(ids.normal_value, Some(123));
        assert_eq!(ids.lucky_value, Some(0));
        assert_eq!(ids.property, Some(6));
        assert_eq!(ids.skill_effect_group_index, Some(2));
        assert_eq!(ids.skill_effect_component_index, Some(1));
        assert_eq!(ids.skill_effect_component_count, Some(3));
        assert_eq!(
            ids.action_identity_resolution,
            Some("server-reported-damage-ability-action-instance-unobserved")
        );
        assert_eq!(
            ids.cast_observability_scope,
            Some("not-required-for-damage-action-row")
        );
        assert_eq!(
            ids.relationship_shape,
            "recipient-damage-action-to-recipient-or-enemy-target"
        );
        assert_eq!(counts.damage, 1);
    }

    #[test]
    fn affected_entity_can_later_act_on_another_player_like_entity() {
        let provider = entity(1, 100);
        let affected_entity = entity(2, 200);
        let damage_target = entity(3, 300);
        let mut counts = EventCounts::default();

        let status = TimelineEventKind::Status(StatusEvent {
            source: Some(provider),
            target: affected_entity,
            effect: StatusEffectId(31_602),
            instance_id: None,
            origin: None,
            state: StatusState::Applied,
            stacks: Some(1),
            duration_millis: Some(10_000),
            level: Some(2),
            part_id: None,
            count: None,
            created_at_millis: None,
        });
        let (_, status_ids) =
            relationship_ids(TEAM_ATTRIBUTE_INTERPRETATION_BUILD, &status, &mut counts)
                .expect("status row");

        let damage = TimelineEventKind::Damage(DamageEvent {
            source: affected_entity,
            direct_source: None,
            target: damage_target,
            ability: Some(AbilityId(55_001)),
            amount: 123,
            actual_amount: Some(123),
            hp_loss: Some(123),
            shield_loss: Some(0),
            hit_event_id: Some(4),
            damage_source: None,
            damage_type: None,
            flags: DamageFlags::default(),
            packet: DamagePacketDetail::default(),
        });
        let (_, damage_ids) =
            relationship_ids(TEAM_ATTRIBUTE_INTERPRETATION_BUILD, &damage, &mut counts)
                .expect("damage row");

        assert_eq!(status_ids.provider, Some(provider));
        assert_eq!(status_ids.affected_entity, Some(affected_entity));
        assert_eq!(status_ids.recipient_or_enemy_target, Some(affected_entity));
        assert_eq!(damage_ids.damage_actor, status_ids.affected_entity);
        assert_eq!(damage_ids.damage_target, Some(damage_target));
        assert_eq!(damage_ids.recipient_or_enemy_target, Some(damage_target));
        assert_ne!(damage_ids.damage_target, Some(provider));
        assert_eq!(counts.status, 1);
        assert_eq!(counts.damage, 1);
    }

    #[test]
    fn filtered_effect_projection_keeps_both_allegiance_neutral_join_directions() {
        let provider = entity(1, 100);
        let affected_entity = entity(2, 200);
        let other_entity = entity(3, 300);
        let applied = TimelineEventKind::Status(StatusEvent {
            source: Some(provider),
            target: affected_entity,
            effect: StatusEffectId(2_110_125),
            instance_id: Some(StatusEffectInstanceId(42)),
            origin: None,
            state: StatusState::Applied,
            stacks: Some(1),
            duration_millis: Some(10_000),
            level: Some(1),
            part_id: None,
            count: None,
            created_at_millis: None,
        });
        let mut active = BTreeSet::new();
        assert_eq!(
            filtered_timeline_relationship_join(&applied, 2_110_125, true, &active),
            Some(None)
        );
        update_active_filtered_timeline_effects(&applied, 2_110_125, &mut active);

        let source_side = TimelineEventKind::Damage(DamageEvent {
            source: affected_entity,
            direct_source: None,
            target: other_entity,
            ability: Some(AbilityId(55_001)),
            amount: 123,
            actual_amount: Some(123),
            hp_loss: Some(123),
            shield_loss: Some(0),
            hit_event_id: None,
            damage_source: None,
            damage_type: None,
            flags: DamageFlags::default(),
            packet: DamagePacketDetail::default(),
        });
        assert_eq!(
            filtered_timeline_relationship_join(&source_side, 2_110_125, true, &active),
            Some(Some("source-side-effect-endpoint-equals-damage-actor"))
        );

        let target_side = TimelineEventKind::Damage(DamageEvent {
            source: other_entity,
            direct_source: None,
            target: affected_entity,
            ability: Some(AbilityId(55_002)),
            amount: 456,
            actual_amount: Some(456),
            hp_loss: Some(456),
            shield_loss: Some(0),
            hit_event_id: None,
            damage_source: None,
            damage_type: None,
            flags: DamageFlags::default(),
            packet: DamagePacketDetail::default(),
        });
        assert_eq!(
            filtered_timeline_relationship_join(&target_side, 2_110_125, true, &active),
            Some(Some("target-side-effect-endpoint-equals-damage-target"))
        );
    }

    #[test]
    fn effect_endpoint_can_be_the_damage_target_without_assuming_it_is_an_enemy() {
        let provider = entity(1, 100);
        let effect_endpoint = entity(2, 200);
        let damage_actor = entity(3, 300);
        let mut counts = EventCounts::default();

        let status = TimelineEventKind::Status(StatusEvent {
            source: Some(provider),
            target: effect_endpoint,
            effect: StatusEffectId(31_603),
            instance_id: Some(StatusEffectInstanceId(998)),
            origin: None,
            state: StatusState::Applied,
            stacks: Some(1),
            duration_millis: Some(10_000),
            level: Some(2),
            part_id: None,
            count: None,
            created_at_millis: None,
        });
        let (_, status_ids) =
            relationship_ids(TEAM_ATTRIBUTE_INTERPRETATION_BUILD, &status, &mut counts)
                .expect("status row");

        let damage = TimelineEventKind::Damage(DamageEvent {
            source: damage_actor,
            direct_source: None,
            target: effect_endpoint,
            ability: Some(AbilityId(55_002)),
            amount: 456,
            actual_amount: Some(456),
            hp_loss: Some(456),
            shield_loss: Some(0),
            hit_event_id: Some(5),
            damage_source: None,
            damage_type: None,
            flags: DamageFlags::default(),
            packet: DamagePacketDetail::default(),
        });
        let (_, damage_ids) =
            relationship_ids(TEAM_ATTRIBUTE_INTERPRETATION_BUILD, &damage, &mut counts)
                .expect("damage row");

        assert_eq!(status_ids.recipient_or_enemy_target, Some(effect_endpoint));
        assert_eq!(
            damage_ids.damage_target,
            status_ids.recipient_or_enemy_target
        );
        assert_eq!(damage_ids.damage_actor, Some(damage_actor));
        assert_eq!(
            status_ids.relationship_endpoint_role,
            Some("recipient-or-enemy-target-unresolved")
        );
        assert_eq!(
            damage_ids.relationship_endpoint_role,
            Some("recipient-or-enemy-target-unresolved")
        );
        assert_eq!(counts.status, 1);
        assert_eq!(counts.damage, 1);
    }

    #[test]
    fn non_relationship_events_do_not_fabricate_casts() {
        let mut counts = EventCounts::default();
        let event = TimelineEventKind::Life {
            actor: entity(1, 100),
            state: rlogs_events::LifeState::Died,
        };
        assert!(
            relationship_ids(TEAM_ATTRIBUTE_INTERPRETATION_BUILD, &event, &mut counts).is_none()
        );
        assert_eq!(counts.cast, 0);
    }

    #[test]
    fn exact_build_team_attributes_become_allegiance_neutral_affiliation_rows() {
        let mut counts = EventCounts::default();
        let event = TimelineEventKind::EntityAttributes(EntityAttributeEvent {
            actor: entity(2, 200),
            update_kind: rlogs_events::EntityAttributeUpdateKind::Snapshot,
            ownership: None,
            attributes: vec![
                rlogs_events::EntityAttribute {
                    attribute_id: ATTR_TEAM_ID,
                    raw_value: vec![135, 213, 167, 192, 1],
                    decoded: None,
                },
                rlogs_events::EntityAttribute {
                    attribute_id: ATTR_TEAM_MEMBER_NUMS,
                    raw_value: vec![5],
                    decoded: None,
                },
            ],
        });
        let (kind, ids) =
            relationship_ids(TEAM_ATTRIBUTE_INTERPRETATION_BUILD, &event, &mut counts)
                .expect("party affiliation row");
        assert_eq!(kind, "party_affiliation");
        assert_eq!(ids.source, Some(entity(2, 200)));
        assert_eq!(ids.target, Some(entity(2, 200)));
        assert_eq!(ids.party_id.as_deref(), Some("403303047"));
        assert_eq!(ids.reported_party_member_count, Some(5));
        assert!(ids.exact_build_team_attribute_interpretation);
        assert!(!ids.party_membership_authority);
        assert_eq!(counts.party_affiliation, 1);
    }

    #[test]
    fn another_build_preserves_team_attribute_without_interpreting_it() {
        let mut counts = EventCounts::default();
        let event = TimelineEventKind::EntityAttributes(EntityAttributeEvent {
            actor: entity(2, 200),
            update_kind: rlogs_events::EntityAttributeUpdateKind::Delta,
            ownership: None,
            attributes: vec![rlogs_events::EntityAttribute {
                attribute_id: ATTR_TEAM_ID,
                raw_value: vec![77],
                decoded: None,
            }],
        });
        let (_, ids) = relationship_ids("different-build", &event, &mut counts)
            .expect("unresolved attribute row");
        assert_eq!(ids.party_id, None);
        assert!(!ids.exact_build_team_attribute_interpretation);
        assert_eq!(
            ids.source_resolution,
            "unresolved-unversioned-build-team-attribute"
        );
        assert!(!ids.party_membership_authority);
    }

    #[test]
    fn varint_decoder_rejects_trailing_overlong_and_overflow_values() {
        assert_eq!(
            decode_unsigned_varint_exact(&[135, 213, 167, 192, 1]),
            Some(403_303_047)
        );
        assert_eq!(decode_unsigned_varint_exact(&[0]), Some(0));
        assert_eq!(decode_unsigned_varint_exact(&[]), Some(0));
        assert_eq!(decode_unsigned_varint_exact(&[1, 0]), None);
        assert_eq!(decode_unsigned_varint_exact(&[128, 0]), None);
        assert_eq!(decode_unsigned_varint_exact(&[255; 10]), None);
    }

    #[test]
    fn rlog_filter_rejects_partial_files() {
        assert!(is_sealed_rlog_candidate(Path::new("run.rlog")));
        assert!(!is_sealed_rlog_candidate(Path::new("run.partial.rlog")));
        assert!(!is_sealed_rlog_candidate(Path::new("run.jsonl")));
    }
}
