use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    env,
    ffi::OsString,
    fs::{self, File},
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{
    ActorEvent, ActorKind, CanonicalEvent, DamageEvent, EntityAttribute, EntityAttributeUpdateKind,
    EntityAttributeValue, RunState, StatusOrigin, StatusState, TimelineEventKind,
};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 3;
const WATCHLIST_SCHEMA_VERSION: u16 = 3;

#[derive(Debug)]
struct Arguments {
    rlog: PathBuf,
    output: PathBuf,
    sequences: BTreeSet<u64>,
    watchlist: Option<LedgerWatchlist>,
    watchlist_source: Option<InputArtifact>,
}

#[derive(Debug, Serialize)]
struct AuditBundle {
    schema_version: u16,
    generated_by: &'static str,
    rlog: String,
    expected_game_build: Option<String>,
    watchlist_source: Option<InputArtifact>,
    selected_watchlist_effect_ids: Vec<i64>,
    policy: AuditPolicy,
    requested_sequences: Vec<u64>,
    matched_damage_events: Vec<DamageStateSnapshot>,
    unmatched_sequences: Vec<u64>,
}

#[derive(Debug, Serialize)]
struct AuditPolicy {
    runtime_use: &'static str,
    attribute_scope: &'static str,
    snapshot_scope: &'static str,
    status_scope: &'static str,
    unresolved_values_hidden: bool,
    packet_state_is_formula_authority: bool,
    automatic_selection: &'static str,
}

#[derive(Debug, Serialize)]
struct DamageStateSnapshot {
    sequence: u64,
    observed_micros: u64,
    game_time_millis: Option<i64>,
    run_ordinal: u32,
    selection: DamageSelection,
    damage: DamageEvent,
    source: EntityStateSnapshot,
    direct_source: Option<EntityStateSnapshot>,
    target: EntityStateSnapshot,
}

#[derive(Debug, Serialize)]
struct DamageSelection {
    manual_sequence: bool,
    active_watchlist_effect_ids: Vec<i64>,
}

#[derive(Debug, Clone, Deserialize)]
struct LedgerWatchlist {
    schema_version: u16,
    deployment_id: String,
    game_build: String,
    selected_effect_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize)]
struct InputArtifact {
    file: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct EntityStateSnapshot {
    entity_uuid: i64,
    identity: Option<ActorIdentitySnapshot>,
    latest_snapshot_sequence: Option<u64>,
    latest_snapshot_attribute_count: Option<usize>,
    attributes: Vec<AttributeSnapshot>,
    active_statuses: Vec<ActiveStatus>,
}

#[derive(Debug, Clone, Serialize)]
struct ActorIdentitySnapshot {
    last_observed_sequence: u64,
    entity_type_id: i32,
    kind: ActorKind,
    monster_id: Option<i64>,
    display_name: Option<String>,
    class_id: Option<i32>,
    specialization_id: Option<i32>,
    level: Option<u32>,
    ability_score: Option<i64>,
    weapon_item_id: Option<i64>,
    seasonal_score: Option<i64>,
}

impl ActorIdentitySnapshot {
    fn from_event(sequence: u64, event: &ActorEvent) -> Self {
        Self {
            last_observed_sequence: sequence,
            entity_type_id: event.entity_type_id,
            kind: event.kind,
            monster_id: event.monster_id.map(|value| value.0),
            display_name: event.display_name.clone(),
            class_id: event.class_id,
            specialization_id: event.specialization_id,
            level: event.level,
            ability_score: event.ability_score,
            weapon_item_id: event.weapon_item_id,
            seasonal_score: event.seasonal_score,
        }
    }

    fn observe(&mut self, sequence: u64, event: &ActorEvent) {
        self.last_observed_sequence = sequence;
        self.entity_type_id = event.entity_type_id;
        self.kind = event.kind;
        if event.monster_id.is_some() {
            self.monster_id = event.monster_id.map(|value| value.0);
        }
        if event.display_name.is_some() {
            self.display_name = event.display_name.clone();
        }
        if event.class_id.is_some() {
            self.class_id = event.class_id;
        }
        if event.specialization_id.is_some() {
            self.specialization_id = event.specialization_id;
        }
        if event.level.is_some() {
            self.level = event.level;
        }
        if event.ability_score.is_some() {
            self.ability_score = event.ability_score;
        }
        if event.weapon_item_id.is_some() {
            self.weapon_item_id = event.weapon_item_id;
        }
        if event.seasonal_score.is_some() {
            self.seasonal_score = event.seasonal_score;
        }
    }
}

#[derive(Debug, Serialize)]
struct AttributeSnapshot {
    attribute_id: i32,
    raw_value: Vec<u8>,
    decoded: Option<EntityAttributeValue>,
    wire_varint_u64: Option<u64>,
    last_observed_sequence: u64,
    last_update_kind: EntityAttributeUpdateKind,
}

#[derive(Debug, Default)]
struct ActorAttributeState {
    latest_snapshot_sequence: Option<u64>,
    latest_snapshot_attribute_count: Option<usize>,
    values: BTreeMap<i32, TrackedAttribute>,
}

#[derive(Debug)]
struct TrackedAttribute {
    attribute: EntityAttribute,
    last_observed_sequence: u64,
    last_update_kind: EntityAttributeUpdateKind,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct StatusKey {
    effect_id: i64,
    instance_id: Option<i64>,
    source_entity_uuid: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
struct ActiveStatus {
    effect_id: i64,
    instance_id: Option<i64>,
    source_entity_uuid: Option<i64>,
    origin: Option<StatusOrigin>,
    stacks: Option<u32>,
    duration_millis: Option<u64>,
    level: Option<i32>,
    part_id: Option<i32>,
    count: Option<i32>,
    created_at_millis: Option<i64>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("damage state ledger failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = arguments()?;
    let mut reader = RlogReader::new(
        BufReader::new(File::open(&args.rlog)?),
        RlogLimits::default(),
    )?;
    let mut run_ordinal = 0_u32;
    let mut attributes = HashMap::<(u32, i64), ActorAttributeState>::new();
    let mut identities = HashMap::<(u32, i64), ActorIdentitySnapshot>::new();
    let mut statuses = HashMap::<(u32, i64), BTreeMap<StatusKey, ActiveStatus>>::new();
    let mut matched = Vec::new();
    let mut matched_sequences = BTreeSet::new();
    let selected_watchlist_effect_ids = args
        .watchlist
        .as_ref()
        .map(|watchlist| {
            watchlist
                .selected_effect_ids
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();

    while let Some(envelope) = reader.next_event()? {
        if let Some(watchlist) = &args.watchlist
            && envelope.region.client_build != watchlist.game_build
        {
            return Err(format!(
                "{} contains client build {} but the damage-state watchlist requires {}",
                args.rlog.display(),
                envelope.region.client_build,
                watchlist.game_build
            )
            .into());
        }
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            continue;
        };
        match &timeline.kind {
            TimelineEventKind::RunBoundary { state, .. } => match state {
                RunState::Entered => run_ordinal = run_ordinal.saturating_add(1),
                RunState::Started if run_ordinal == 0 => run_ordinal = 1,
                _ => {}
            },
            TimelineEventKind::Actor(event) => {
                let key = (run_ordinal, event.actor.entity_uuid.0);
                identities
                    .entry(key)
                    .and_modify(|identity| identity.observe(envelope.sequence, event))
                    .or_insert_with(|| ActorIdentitySnapshot::from_event(envelope.sequence, event));
            }
            TimelineEventKind::EntityAttributes(event) => {
                let key = (run_ordinal, event.actor.entity_uuid.0);
                let state = attributes.entry(key).or_default();
                if event.update_kind == EntityAttributeUpdateKind::Snapshot {
                    state.values.clear();
                    state.latest_snapshot_sequence = Some(envelope.sequence);
                    state.latest_snapshot_attribute_count = Some(event.attributes.len());
                }
                for attribute in &event.attributes {
                    state.values.insert(
                        attribute.attribute_id,
                        TrackedAttribute {
                            attribute: attribute.clone(),
                            last_observed_sequence: envelope.sequence,
                            last_update_kind: event.update_kind,
                        },
                    );
                }
            }
            TimelineEventKind::Status(status) => {
                let entity_key = (run_ordinal, status.target.entity_uuid.0);
                let entity_statuses = statuses.entry(entity_key).or_default();
                let key = StatusKey {
                    effect_id: status.effect.0,
                    instance_id: status.instance_id.map(|value| value.0),
                    source_entity_uuid: status.source.map(|value| value.entity_uuid.0),
                };
                match status.state {
                    StatusState::Applied | StatusState::Refreshed | StatusState::Stacked => {
                        entity_statuses.insert(
                            key.clone(),
                            ActiveStatus {
                                effect_id: key.effect_id,
                                instance_id: key.instance_id,
                                source_entity_uuid: key.source_entity_uuid,
                                origin: status.origin,
                                stacks: status.stacks,
                                duration_millis: status.duration_millis,
                                level: status.level,
                                part_id: status.part_id,
                                count: status.count,
                                created_at_millis: status.created_at_millis,
                            },
                        );
                    }
                    StatusState::Consumed | StatusState::Removed => {
                        entity_statuses.remove(&key);
                    }
                }
            }
            TimelineEventKind::Damage(damage) => {
                let manual_sequence = args.sequences.contains(&envelope.sequence);
                let active_watchlist_effect_ids = active_watchlist_effects(
                    run_ordinal,
                    damage,
                    &statuses,
                    &selected_watchlist_effect_ids,
                );
                if !manual_sequence && active_watchlist_effect_ids.is_empty() {
                    continue;
                }
                matched_sequences.insert(envelope.sequence);
                matched.push(DamageStateSnapshot {
                    sequence: envelope.sequence,
                    observed_micros: envelope.time.observed_micros,
                    game_time_millis: envelope.time.game_time_millis,
                    run_ordinal,
                    selection: DamageSelection {
                        manual_sequence,
                        active_watchlist_effect_ids,
                    },
                    damage: damage.clone(),
                    source: entity_snapshot(
                        run_ordinal,
                        damage.source.entity_uuid.0,
                        &identities,
                        &attributes,
                        &statuses,
                    ),
                    direct_source: damage.direct_source.map(|direct_source| {
                        entity_snapshot(
                            run_ordinal,
                            direct_source.entity_uuid.0,
                            &identities,
                            &attributes,
                            &statuses,
                        )
                    }),
                    target: entity_snapshot(
                        run_ordinal,
                        damage.target.entity_uuid.0,
                        &identities,
                        &attributes,
                        &statuses,
                    ),
                });
            }
            _ => {}
        }
    }

    let requested_sequences = args.sequences.iter().copied().collect::<Vec<_>>();
    let unmatched_sequences = args
        .sequences
        .difference(&matched_sequences)
        .copied()
        .collect::<Vec<_>>();
    let report = AuditBundle {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-damage-state-ledger",
        rlog: args.rlog.display().to_string(),
        expected_game_build: args
            .watchlist
            .as_ref()
            .map(|watchlist| watchlist.game_build.clone()),
        watchlist_source: args.watchlist_source,
        selected_watchlist_effect_ids: selected_watchlist_effect_ids.into_iter().collect(),
        policy: AuditPolicy {
            runtime_use: "offline_research_only_never_loaded_by_capture_or_live_meter",
            attribute_scope: "all_latest packet-observed attributes for attributed source, immediate direct source when present, and target at the exact damage envelope sequence",
            snapshot_scope: "snapshot_updates_replace_the_previous_actor_map_and_delta_or_unknown_updates_replace_only_observed_keys",
            status_scope: "all_active_packet_observed_status_instances_for_source_and_target_at_the_exact_damage_envelope_sequence",
            unresolved_values_hidden: false,
            packet_state_is_formula_authority: false,
            automatic_selection: "a damage event is selected when any build-locked watchlist status is active on its attributed source, immediate direct source, or target; manual sequence selection remains additive",
        },
        requested_sequences,
        matched_damage_events: matched,
        unmatched_sequences,
    };

    let mut writer = BufWriter::new(File::create(&args.output)?);
    serde_json::to_writer_pretty(&mut writer, &report)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn active_watchlist_effects(
    run_ordinal: u32,
    damage: &DamageEvent,
    statuses: &HashMap<(u32, i64), BTreeMap<StatusKey, ActiveStatus>>,
    selected_effect_ids: &BTreeSet<i64>,
) -> Vec<i64> {
    if selected_effect_ids.is_empty() {
        return Vec::new();
    }
    let mut entity_uuids =
        BTreeSet::from([damage.source.entity_uuid.0, damage.target.entity_uuid.0]);
    if let Some(direct_source) = damage.direct_source {
        entity_uuids.insert(direct_source.entity_uuid.0);
    }
    entity_uuids
        .into_iter()
        .filter_map(|entity_uuid| statuses.get(&(run_ordinal, entity_uuid)))
        .flat_map(|active| active.keys().map(|key| key.effect_id))
        .filter(|effect_id| selected_effect_ids.contains(effect_id))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn entity_snapshot(
    run_ordinal: u32,
    entity_uuid: i64,
    identities: &HashMap<(u32, i64), ActorIdentitySnapshot>,
    attributes: &HashMap<(u32, i64), ActorAttributeState>,
    statuses: &HashMap<(u32, i64), BTreeMap<StatusKey, ActiveStatus>>,
) -> EntityStateSnapshot {
    let attribute_state = attributes.get(&(run_ordinal, entity_uuid));
    EntityStateSnapshot {
        entity_uuid,
        identity: identities.get(&(run_ordinal, entity_uuid)).cloned(),
        latest_snapshot_sequence: attribute_state.and_then(|state| state.latest_snapshot_sequence),
        latest_snapshot_attribute_count: attribute_state
            .and_then(|state| state.latest_snapshot_attribute_count),
        attributes: attribute_state
            .map(|state| {
                state
                    .values
                    .values()
                    .map(|tracked| AttributeSnapshot {
                        attribute_id: tracked.attribute.attribute_id,
                        raw_value: tracked.attribute.raw_value.clone(),
                        decoded: tracked.attribute.decoded.clone(),
                        wire_varint_u64: decode_complete_varint(&tracked.attribute.raw_value),
                        last_observed_sequence: tracked.last_observed_sequence,
                        last_update_kind: tracked.last_update_kind,
                    })
                    .collect()
            })
            .unwrap_or_default(),
        active_statuses: statuses
            .get(&(run_ordinal, entity_uuid))
            .map(|values| values.values().cloned().collect())
            .unwrap_or_default(),
    }
}

fn decode_complete_varint(bytes: &[u8]) -> Option<u64> {
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
            return (index + 1 == bytes.len()).then_some(value);
        }
    }
    None
}

fn arguments() -> Result<Arguments, String> {
    let mut values = env::args_os().skip(1).collect::<Vec<_>>();
    let rlog = PathBuf::from(take_value(&mut values, "--rlog")?);
    let output = PathBuf::from(take_value(&mut values, "--output")?);
    let watchlist_path = take_optional_value(&mut values, "--watchlist")?.map(PathBuf::from);
    let (watchlist, watchlist_source) = watchlist_path
        .as_ref()
        .map(|path| load_watchlist(path))
        .transpose()?
        .map(|(watchlist, source)| (Some(watchlist), Some(source)))
        .unwrap_or((None, None));
    let mut sequences = BTreeSet::new();
    while let Some(position) = values.iter().position(|value| value == "--sequence") {
        if position + 1 >= values.len() {
            return Err("--sequence requires a value".to_owned());
        }
        let raw = values.remove(position + 1);
        values.remove(position);
        sequences.insert(parse_u64(raw, "--sequence")?);
    }
    if (sequences.is_empty() && watchlist.is_none()) || !values.is_empty() {
        return Err(usage());
    }
    Ok(Arguments {
        rlog,
        output,
        sequences,
        watchlist,
        watchlist_source,
    })
}

fn load_watchlist(path: &Path) -> Result<(LedgerWatchlist, InputArtifact), String> {
    let bytes = fs::read(path)
        .map_err(|error| format!("failed to read watchlist {}: {error}", path.display()))?;
    let watchlist: LedgerWatchlist = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid watchlist {}: {error}", path.display()))?;
    if watchlist.schema_version != WATCHLIST_SCHEMA_VERSION
        || watchlist.deployment_id.trim().is_empty()
        || watchlist.game_build.trim().is_empty()
        || !watchlist
            .game_build
            .bytes()
            .all(|byte| byte.is_ascii_digit())
        || watchlist.selected_effect_ids.is_empty()
        || watchlist
            .selected_effect_ids
            .iter()
            .any(|effect_id| *effect_id <= 0)
    {
        return Err(format!(
            "watchlist {} has an unsupported or incomplete shape",
            path.display()
        ));
    }
    let source = InputArtifact {
        file: path.to_string_lossy().replace('\\', "/"),
        bytes: bytes.len() as u64,
        sha256: format!("sha256:{:x}", Sha256::digest(&bytes)),
    };
    Ok((watchlist, source))
}

fn parse_u64(value: OsString, flag: &str) -> Result<u64, String> {
    value
        .to_string_lossy()
        .parse::<u64>()
        .map_err(|_| format!("{flag} requires a non-negative integer"))
}

fn take_value(values: &mut Vec<OsString>, flag: &str) -> Result<OsString, String> {
    take_optional_value(values, flag)?.ok_or_else(usage)
}

fn take_optional_value(values: &mut Vec<OsString>, flag: &str) -> Result<Option<OsString>, String> {
    let Some(position) = values.iter().position(|value| value == flag) else {
        return Ok(None);
    };
    if position + 1 >= values.len() {
        return Err(format!("{flag} requires a value"));
    }
    let value = values.remove(position + 1);
    values.remove(position);
    Ok(Some(value))
}

fn usage() -> String {
    "usage: rlogs-bpsr-damage-state-ledger --rlog <current-decoder.rlog> [--watchlist <build-locked-watchlist.json>] [--sequence <damage-envelope-sequence> ...] --output <ledger.json>".to_owned()
}

#[cfg(test)]
mod tests {
    use super::{ActiveStatus, StatusKey, active_watchlist_effects, decode_complete_varint};
    use rlogs_events::{
        ActorId, DamageEvent, DamageFlags, DamagePacketDetail, EntityRef, EntityUuid,
    };
    use std::collections::{BTreeMap, BTreeSet, HashMap};

    #[test]
    fn complete_varint_projection_requires_the_entire_raw_value() {
        assert_eq!(decode_complete_varint(&[]), Some(0));
        assert_eq!(decode_complete_varint(&[0xbf, 0x04]), Some(575));
        assert_eq!(decode_complete_varint(&[0xc1, 0x2b]), Some(5_569));
        assert_eq!(decode_complete_varint(&[0x09, b'M', b'a']), None);
        assert_eq!(decode_complete_varint(&[0x80]), None);
    }

    #[test]
    fn automatic_selection_checks_source_direct_source_and_target_without_duplicates() {
        let actor = |uuid| EntityRef {
            actor_id: ActorId(uuid as u64),
            entity_uuid: EntityUuid(uuid),
        };
        let damage = DamageEvent {
            source: actor(1),
            direct_source: Some(actor(2)),
            target: actor(3),
            ability: None,
            amount: 1,
            actual_amount: None,
            hp_loss: None,
            shield_loss: None,
            hit_event_id: None,
            damage_source: None,
            damage_type: None,
            flags: DamageFlags::default(),
            packet: DamagePacketDetail::default(),
        };
        let status = |effect_id| ActiveStatus {
            effect_id,
            instance_id: None,
            source_entity_uuid: None,
            origin: None,
            stacks: None,
            duration_millis: None,
            level: None,
            part_id: None,
            count: None,
            created_at_millis: None,
        };
        let mut statuses = HashMap::new();
        for (uuid, effect_id) in [(1, 10), (2, 20), (3, 10), (3, 30)] {
            statuses
                .entry((1, uuid))
                .or_insert_with(BTreeMap::new)
                .insert(
                    StatusKey {
                        effect_id,
                        instance_id: None,
                        source_entity_uuid: None,
                    },
                    status(effect_id),
                );
        }
        assert_eq!(
            active_watchlist_effects(1, &damage, &statuses, &BTreeSet::from([10, 20, 40])),
            vec![10, 20]
        );
    }
}
