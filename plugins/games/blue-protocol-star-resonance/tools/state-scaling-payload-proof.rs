use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsString,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::PathBuf,
};

use prost::Message;
use rlogs_game_bpsr::{CaptureRecordKind, JsonlJournalReader};
use serde::Serialize;

const SCHEMA_VERSION: u16 = 3;
const CURRENT_HP_ATTRIBUTE_ID: i32 = 11_310;
const MAX_HP_ATTRIBUTE_ID: i32 = 11_320;
const MAX_HP_TOTAL_ATTRIBUTE_ID: i32 = 11_321;
const MAX_HP_ADD_ATTRIBUTE_ID: i32 = 11_322;
const MAX_HP_EXTRA_ADD_ATTRIBUTE_ID: i32 = 11_323;
const MAX_HP_PERCENT_ATTRIBUTE_ID: i32 = 11_324;
const MAX_HP_EXTRA_PERCENT_ATTRIBUTE_ID: i32 = 11_325;
const DAMAGE_TYPE_HEAL: i32 = 2;
const BUFF_LOGIC_ADD_BUFF: i32 = 18;
const BUFF_LOGIC_CHANGE: i32 = 19;

#[derive(Debug)]
struct Arguments {
    journal: PathBuf,
    sequences: BTreeSet<u64>,
    abilities: BTreeSet<i32>,
    output: PathBuf,
}

#[derive(Debug, Serialize)]
struct Report {
    schema_version: u16,
    generated_by: &'static str,
    journal: String,
    capture_id: String,
    requested_sequences: Vec<u64>,
    requested_abilities: Vec<i32>,
    policy: Policy,
    stack_transition_evidence: Vec<StackTransitionEvidence>,
    packets: Vec<PacketReport>,
}

#[derive(Debug, Serialize)]
struct StackTransitionEvidence {
    raw_event_type: i32,
    transitions_with_known_before_and_after: u64,
    increases: u64,
    decreases: u64,
    unchanged: u64,
    examples: Vec<StackTransitionExample>,
}

#[derive(Debug, Serialize)]
struct StackTransitionExample {
    target_entity_uuid: i64,
    buff_instance_id: i32,
    effect_id: i32,
    before_stacks: i32,
    after_stacks: i32,
}

#[derive(Debug, Default)]
struct StackTransitionAccumulator {
    transitions: u64,
    increases: u64,
    decreases: u64,
    unchanged: u64,
    examples: Vec<StackTransitionExample>,
}

#[derive(Debug, Serialize)]
struct Policy {
    packet_fields_discarded: bool,
    nonmatching_deltas_discarded_from_canonical_timeline: bool,
    descriptions_are_formula_truth: bool,
    purpose: &'static str,
}

#[derive(Debug, Serialize)]
struct PacketReport {
    record_sequence: u64,
    observed_micros: u64,
    route_service_id: Option<u64>,
    route_method_id: Option<u32>,
    application_payload_length: usize,
    matching_deltas: Vec<DeltaReport>,
    decode_error: Option<String>,
}

#[derive(Debug, Serialize)]
struct DeltaReport {
    delta_index: usize,
    target_entity_uuid: Option<i64>,
    attributes: Vec<AttributeReport>,
    hp_state: HpState,
    active_buffs_before_delta: Vec<ActiveBuffReport>,
    buff_effects_in_wire_order: Vec<BuffEffectReport>,
    active_buffs_after_delta: Vec<ActiveBuffReport>,
    effects_in_wire_order: Vec<DamageReport>,
}

#[derive(Debug, Clone, Serialize)]
struct ActiveBuffReport {
    buff_instance_id: i32,
    effect_id: i32,
    source_entity_uuid: Option<i64>,
    host_entity_uuid: Option<i64>,
    level: Option<i32>,
    stacks: Option<i32>,
    part_id: Option<i32>,
    count: Option<i32>,
    origin_source_type_id: Option<i32>,
    origin_source_config_id: Option<i32>,
}

#[derive(Debug, Serialize)]
struct BuffEffectReport {
    buff_index: usize,
    event_type: Option<i32>,
    event_kind: &'static str,
    buff_instance_id: Option<i32>,
    host_entity_uuid: Option<i64>,
    trigger_time_millis: Option<i64>,
    logic_effects_in_wire_order: Vec<BuffLogicReport>,
}

#[derive(Debug, Serialize)]
struct BuffLogicReport {
    logic_index: usize,
    effect_type: Option<i32>,
    is_loop: Option<bool>,
    decoded: BuffLogicDecode,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum BuffLogicDecode {
    AddBuff {
        buff_instance_id: Option<i32>,
        effect_id: Option<i32>,
        level: Option<i32>,
        host_entity_uuid: Option<i64>,
        source_entity_uuid: Option<i64>,
        stacks: Option<i32>,
        part_id: Option<i32>,
        count: Option<i32>,
        duration_millis: Option<i32>,
        create_time_millis: Option<i64>,
        origin_source_type_id: Option<i32>,
        origin_source_config_id: Option<i32>,
    },
    Change {
        stacks: Option<i32>,
        duration_millis: Option<i32>,
        create_time_millis: Option<i64>,
    },
    DecodeFailure {
        raw_hex: String,
        detail: String,
    },
    NotInterpreted {
        raw_hex: String,
    },
}

#[derive(Debug, Default, Serialize)]
struct HpState {
    current_hp: Option<i64>,
    max_hp_final: Option<i64>,
    max_hp_total: Option<i64>,
    max_hp_add: Option<i64>,
    max_hp_extra_add: Option<i64>,
    max_hp_percent: Option<i64>,
    max_hp_extra_percent: Option<i64>,
}

#[derive(Debug, Serialize)]
struct AttributeReport {
    attribute_id: Option<i32>,
    decoded_unsigned_varint: Option<u64>,
    decoded_i64: Option<i64>,
    raw_hex: String,
}

#[derive(Debug, Serialize)]
struct DamageReport {
    damage_index: usize,
    requested_ability: bool,
    event_kind: &'static str,
    owner_id: Option<i32>,
    normal_value: Option<i64>,
    lucky_value: Option<i64>,
    selected_amount: i64,
    actual_value: Option<i64>,
    hp_loss: Option<i64>,
    shield_loss: Option<i64>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    type_flags: Option<i32>,
    critical: Option<bool>,
    attacker_entity_uuid: Option<i64>,
    top_summoner_entity_uuid: Option<i64>,
    owner_level: Option<i32>,
    owner_stage: Option<i32>,
    hit_event_id: Option<i32>,
    passive_uuid: Option<u32>,
    damage_mode: Option<i32>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("BPSR state-scaling payload proof failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = parse_arguments(env::args_os().skip(1))?;
    let journal =
        JsonlJournalReader::new(BufReader::new(File::open(&arguments.journal)?)).read()?;
    let mut packets = Vec::new();
    let mut active_buffs = BTreeMap::<(i64, i32), ActiveBuffReport>::new();
    let mut stack_transition_evidence = BTreeMap::<i32, StackTransitionAccumulator>::new();
    for record in journal.records() {
        let CaptureRecordKind::Packet(packet) = &record.kind else {
            continue;
        };
        let Some(payload) = packet.payload.decode_input() else {
            if arguments.sequences.contains(&record.sequence) {
                packets.push(PacketReport {
                    record_sequence: record.sequence,
                    observed_micros: record.observed_micros,
                    route_service_id: packet.route.map(|route| route.key.service_id),
                    route_method_id: packet.route.map(|route| route.key.method_id),
                    application_payload_length: 0,
                    matching_deltas: Vec::new(),
                    decode_error: Some("application payload is absent".to_owned()),
                });
            }
            continue;
        };
        let route = packet.route.map(|route| route.key);
        let decoded = match route.map(|route| route.method_id) {
            Some(45) => SyncNearDeltaInfo::decode(payload).map(|message| message.deltas),
            Some(46) => SyncToMeDeltaInfo::decode(payload).map(|message| {
                message
                    .delta
                    .and_then(|delta| delta.base_delta)
                    .into_iter()
                    .collect()
            }),
            method => {
                if arguments.sequences.contains(&record.sequence) {
                    packets.push(PacketReport {
                        record_sequence: record.sequence,
                        observed_micros: record.observed_micros,
                        route_service_id: route.map(|route| route.service_id),
                        route_method_id: method,
                        application_payload_length: payload.len(),
                        matching_deltas: Vec::new(),
                        decode_error: Some(
                            "route is not SyncNearDeltaInfo or SyncToMeDeltaInfo".to_owned(),
                        ),
                    });
                }
                continue;
            }
        };
        match decoded {
            Ok(deltas) => {
                let matching_deltas = deltas
                    .into_iter()
                    .enumerate()
                    .filter_map(|(index, delta)| {
                        delta_report(
                            index,
                            delta,
                            &arguments.abilities,
                            &mut active_buffs,
                            &mut stack_transition_evidence,
                        )
                    })
                    .collect();
                if arguments.sequences.contains(&record.sequence) {
                    packets.push(PacketReport {
                        record_sequence: record.sequence,
                        observed_micros: record.observed_micros,
                        route_service_id: route.map(|route| route.service_id),
                        route_method_id: route.map(|route| route.method_id),
                        application_payload_length: payload.len(),
                        matching_deltas,
                        decode_error: None,
                    });
                }
            }
            Err(error) => {
                if arguments.sequences.contains(&record.sequence) {
                    packets.push(PacketReport {
                        record_sequence: record.sequence,
                        observed_micros: record.observed_micros,
                        route_service_id: route.map(|route| route.service_id),
                        route_method_id: route.map(|route| route.method_id),
                        application_payload_length: payload.len(),
                        matching_deltas: Vec::new(),
                        decode_error: Some(error.to_string()),
                    });
                }
            }
        }
    }
    let report = Report {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-state-scaling-payload-proof",
        journal: arguments.journal.to_string_lossy().replace('\\', "/"),
        capture_id: journal.session().capture_id.clone(),
        requested_sequences: arguments.sequences.into_iter().collect(),
        requested_abilities: arguments.abilities.into_iter().collect(),
        policy: Policy {
            packet_fields_discarded: false,
            nonmatching_deltas_discarded_from_canonical_timeline: false,
            descriptions_are_formula_truth: false,
            purpose: "offline exact-payload proof only; runtime meter work remains on canonical events",
        },
        stack_transition_evidence: stack_transition_evidence
            .into_iter()
            .map(|(raw_event_type, evidence)| StackTransitionEvidence {
                raw_event_type,
                transitions_with_known_before_and_after: evidence.transitions,
                increases: evidence.increases,
                decreases: evidence.decreases,
                unchanged: evidence.unchanged,
                examples: evidence.examples,
            })
            .collect(),
        packets,
    };
    let mut writer = BufWriter::new(File::create(arguments.output)?);
    serde_json::to_writer_pretty(&mut writer, &report)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn delta_report(
    index: usize,
    delta: AoiSyncDelta,
    abilities: &BTreeSet<i32>,
    active_buffs: &mut BTreeMap<(i64, i32), ActiveBuffReport>,
    stack_transition_evidence: &mut BTreeMap<i32, StackTransitionAccumulator>,
) -> Option<DeltaReport> {
    let target_entity_uuid = delta.uuid;
    let selected = delta.skill_effects.as_ref().is_some_and(|effect| {
        effect
            .damage
            .iter()
            .any(|damage| damage.owner_id.is_some_and(|id| abilities.contains(&id)))
    });
    let active_buffs_before_delta = target_entity_uuid
        .map(|target| active_buff_snapshot(active_buffs, target))
        .unwrap_or_default();
    let buff_effects_in_wire_order = decode_buff_effect_reports(delta.buff_effect.as_deref());
    if let Some(target) = target_entity_uuid {
        apply_buff_effects(
            active_buffs,
            stack_transition_evidence,
            target,
            &buff_effects_in_wire_order,
        );
    }
    let active_buffs_after_delta = target_entity_uuid
        .map(|target| active_buff_snapshot(active_buffs, target))
        .unwrap_or_default();
    if !selected {
        return None;
    }
    let effects_in_wire_order = delta
        .skill_effects
        .as_ref()
        .into_iter()
        .flat_map(|effect| effect.damage.iter())
        .enumerate()
        .map(|(damage_index, damage)| DamageReport {
            damage_index,
            requested_ability: damage.owner_id.is_some_and(|id| abilities.contains(&id)),
            event_kind: if damage.damage_type == Some(DAMAGE_TYPE_HEAL) {
                "healing"
            } else {
                "damage"
            },
            owner_id: damage.owner_id,
            normal_value: damage.value,
            lucky_value: damage.lucky_value,
            selected_amount: damage.value.or(damage.lucky_value).unwrap_or_default(),
            actual_value: damage.actual_value,
            hp_loss: damage.hp_loss,
            shield_loss: damage.shield_loss,
            damage_source: damage.damage_source,
            damage_type: damage.damage_type,
            type_flags: damage.type_flags,
            critical: damage.critical,
            attacker_entity_uuid: damage.attacker_uuid,
            top_summoner_entity_uuid: damage.top_summoner_uuid,
            owner_level: damage.owner_level,
            owner_stage: damage.owner_stage,
            hit_event_id: damage.hit_event_id,
            passive_uuid: damage.passive_uuid,
            damage_mode: damage.damage_mode,
        })
        .collect::<Vec<_>>();
    let attributes = delta
        .attributes
        .as_ref()
        .map(|attributes| {
            attributes
                .attributes
                .iter()
                .map(|attribute| {
                    let raw = attribute.raw_data.as_deref().unwrap_or_default();
                    let unsigned = decode_varint(raw);
                    AttributeReport {
                        attribute_id: attribute.id,
                        decoded_unsigned_varint: unsigned,
                        decoded_i64: unsigned.and_then(|value| i64::try_from(value).ok()),
                        raw_hex: hex(raw),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let hp_state = delta.attributes.as_ref().map(hp_state).unwrap_or_default();
    Some(DeltaReport {
        delta_index: index,
        target_entity_uuid,
        attributes,
        hp_state,
        active_buffs_before_delta,
        buff_effects_in_wire_order,
        active_buffs_after_delta,
        effects_in_wire_order,
    })
}

fn decode_buff_effect_reports(raw: Option<&[u8]>) -> Vec<BuffEffectReport> {
    raw.and_then(|raw| BuffEffectSync::decode(raw).ok())
        .map(|sync| {
            sync.buff_effects
                .into_iter()
                .enumerate()
                .map(|(buff_index, effect)| BuffEffectReport {
                    buff_index,
                    event_type: effect.event_type,
                    event_kind: match effect.event_type {
                        Some(1) => "apply_or_refresh",
                        Some(2) => "remove",
                        Some(5) => "layer_change_type_5",
                        Some(6) => "layer_change_type_6",
                        _ => "unclassified",
                    },
                    buff_instance_id: effect.buff_uuid,
                    host_entity_uuid: effect.host_uuid,
                    trigger_time_millis: effect.trigger_time,
                    logic_effects_in_wire_order: effect
                        .logic_effects
                        .into_iter()
                        .enumerate()
                        .map(|(logic_index, logic)| BuffLogicReport {
                            logic_index,
                            effect_type: logic.effect_type,
                            is_loop: logic.is_loop,
                            decoded: decode_buff_logic(&logic),
                        })
                        .collect(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn active_buff_snapshot(
    active_buffs: &BTreeMap<(i64, i32), ActiveBuffReport>,
    target_entity_uuid: i64,
) -> Vec<ActiveBuffReport> {
    active_buffs
        .range((target_entity_uuid, i32::MIN)..=(target_entity_uuid, i32::MAX))
        .map(|(_, buff)| buff.clone())
        .collect()
}

fn apply_buff_effects(
    active_buffs: &mut BTreeMap<(i64, i32), ActiveBuffReport>,
    stack_transition_evidence: &mut BTreeMap<i32, StackTransitionAccumulator>,
    target_entity_uuid: i64,
    effects: &[BuffEffectReport],
) {
    for effect in effects {
        let Some(instance_id) = effect.buff_instance_id else {
            continue;
        };
        let key = (target_entity_uuid, instance_id);
        match effect.event_type {
            Some(1) => {
                if let Some(BuffLogicDecode::AddBuff {
                    effect_id: Some(effect_id),
                    source_entity_uuid,
                    host_entity_uuid,
                    level,
                    stacks,
                    part_id,
                    count,
                    origin_source_type_id,
                    origin_source_config_id,
                    ..
                }) = effect
                    .logic_effects_in_wire_order
                    .iter()
                    .map(|logic| &logic.decoded)
                    .find(|logic| matches!(logic, BuffLogicDecode::AddBuff { .. }))
                {
                    active_buffs.insert(
                        key,
                        ActiveBuffReport {
                            buff_instance_id: instance_id,
                            effect_id: *effect_id,
                            source_entity_uuid: *source_entity_uuid,
                            host_entity_uuid: *host_entity_uuid,
                            level: *level,
                            stacks: *stacks,
                            part_id: *part_id,
                            count: *count,
                            origin_source_type_id: *origin_source_type_id,
                            origin_source_config_id: *origin_source_config_id,
                        },
                    );
                }
            }
            Some(2) => {
                active_buffs.remove(&key);
            }
            Some(5) | Some(6) => {
                if let Some(active) = active_buffs.get_mut(&key)
                    && let Some(BuffLogicDecode::Change { stacks, .. }) = effect
                        .logic_effects_in_wire_order
                        .iter()
                        .map(|logic| &logic.decoded)
                        .find(|logic| matches!(logic, BuffLogicDecode::Change { .. }))
                {
                    if let (Some(before), Some(after), Some(event_type)) =
                        (active.stacks, *stacks, effect.event_type)
                    {
                        let evidence = stack_transition_evidence.entry(event_type).or_default();
                        evidence.transitions = evidence.transitions.saturating_add(1);
                        match after.cmp(&before) {
                            std::cmp::Ordering::Greater => {
                                evidence.increases = evidence.increases.saturating_add(1)
                            }
                            std::cmp::Ordering::Less => {
                                evidence.decreases = evidence.decreases.saturating_add(1)
                            }
                            std::cmp::Ordering::Equal => {
                                evidence.unchanged = evidence.unchanged.saturating_add(1)
                            }
                        }
                        if evidence.examples.len() < 20 {
                            evidence.examples.push(StackTransitionExample {
                                target_entity_uuid,
                                buff_instance_id: instance_id,
                                effect_id: active.effect_id,
                                before_stacks: before,
                                after_stacks: after,
                            });
                        }
                    }
                    active.stacks = *stacks;
                }
            }
            _ => {}
        }
    }
}

fn decode_buff_logic(logic: &BuffEffectLogicInfo) -> BuffLogicDecode {
    let raw = logic.raw_data.as_deref().unwrap_or_default();
    match logic.effect_type {
        Some(BUFF_LOGIC_ADD_BUFF) => match BuffInfo::decode(raw) {
            Ok(info) => BuffLogicDecode::AddBuff {
                buff_instance_id: info.buff_uuid,
                effect_id: info.base_id,
                level: info.level,
                host_entity_uuid: info.host_uuid,
                source_entity_uuid: info.fire_uuid,
                stacks: info.layer,
                part_id: info.part_id,
                count: info.count,
                duration_millis: info.duration,
                create_time_millis: info.create_time,
                origin_source_type_id: info
                    .fight_source_info
                    .as_ref()
                    .and_then(|source| source.source_type_id),
                origin_source_config_id: info
                    .fight_source_info
                    .as_ref()
                    .and_then(|source| source.source_config_id),
            },
            Err(error) => BuffLogicDecode::DecodeFailure {
                raw_hex: hex(raw),
                detail: error.to_string(),
            },
        },
        Some(BUFF_LOGIC_CHANGE) => match BuffChange::decode(raw) {
            Ok(change) => BuffLogicDecode::Change {
                stacks: change.layer,
                duration_millis: change.duration,
                create_time_millis: change.create_time,
            },
            Err(error) => BuffLogicDecode::DecodeFailure {
                raw_hex: hex(raw),
                detail: error.to_string(),
            },
        },
        _ => BuffLogicDecode::NotInterpreted { raw_hex: hex(raw) },
    }
}

fn hp_state(attributes: &AttrCollection) -> HpState {
    let value = |id| {
        attributes
            .attributes
            .iter()
            .find(|attribute| attribute.id == Some(id))
            .and_then(|attribute| attribute.raw_data.as_deref())
            .and_then(decode_varint)
            .and_then(|value| i64::try_from(value).ok())
    };
    HpState {
        current_hp: value(CURRENT_HP_ATTRIBUTE_ID),
        max_hp_final: value(MAX_HP_ATTRIBUTE_ID),
        max_hp_total: value(MAX_HP_TOTAL_ATTRIBUTE_ID),
        max_hp_add: value(MAX_HP_ADD_ATTRIBUTE_ID),
        max_hp_extra_add: value(MAX_HP_EXTRA_ADD_ATTRIBUTE_ID),
        max_hp_percent: value(MAX_HP_PERCENT_ATTRIBUTE_ID),
        max_hp_extra_percent: value(MAX_HP_EXTRA_PERCENT_ATTRIBUTE_ID),
    }
}

fn decode_varint(bytes: &[u8]) -> Option<u64> {
    if bytes.is_empty() {
        return Some(0);
    }
    let mut value = 0_u64;
    for (index, byte) in bytes.iter().copied().take(10).enumerate() {
        if index == 9 && byte > 1 {
            return None;
        }
        value |= u64::from(byte & 0x7f) << (index * 7);
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn parse_arguments(values: impl Iterator<Item = OsString>) -> Result<Arguments, String> {
    let mut values = values.collect::<Vec<_>>();
    let mut journal = None;
    let mut sequences = BTreeSet::new();
    let mut abilities = BTreeSet::new();
    let mut output = None;
    while !values.is_empty() {
        let flag = values.remove(0).to_string_lossy().into_owned();
        match flag.as_str() {
            "--journal" => journal = Some(PathBuf::from(take_value(&mut values, "--journal")?)),
            "--sequence" => {
                sequences.insert(parse_value(
                    take_value(&mut values, "--sequence")?,
                    "--sequence",
                )?);
            }
            "--ability" => {
                abilities.insert(parse_value(
                    take_value(&mut values, "--ability")?,
                    "--ability",
                )?);
            }
            "--output" => output = Some(PathBuf::from(take_value(&mut values, "--output")?)),
            _ => return Err(usage()),
        }
    }
    if sequences.is_empty() || abilities.is_empty() {
        return Err(usage());
    }
    Ok(Arguments {
        journal: journal.ok_or_else(usage)?,
        sequences,
        abilities,
        output: output.ok_or_else(usage)?,
    })
}

fn take_value(values: &mut Vec<OsString>, flag: &str) -> Result<OsString, String> {
    if values.is_empty() {
        return Err(format!("missing value after {flag}"));
    }
    Ok(values.remove(0))
}

fn parse_value<T: std::str::FromStr>(value: OsString, flag: &str) -> Result<T, String> {
    value
        .to_string_lossy()
        .parse()
        .map_err(|_| format!("{flag} requires an integer"))
}

fn usage() -> String {
    "usage: rlogs-bpsr-state-scaling-payload-proof --journal <private.jsonl> --sequence <capture-sequence> [--sequence ...] --ability <owner-id> [--ability ...] --output <proof.json>".to_owned()
}

#[derive(Clone, PartialEq, Message)]
struct SyncNearDeltaInfo {
    #[prost(message, repeated, tag = "1")]
    deltas: Vec<AoiSyncDelta>,
}

#[derive(Clone, PartialEq, Message)]
struct SyncToMeDeltaInfo {
    #[prost(message, optional, tag = "1")]
    delta: Option<AoiSyncToMeDelta>,
}

#[derive(Clone, PartialEq, Message)]
struct AoiSyncToMeDelta {
    #[prost(message, optional, tag = "1")]
    base_delta: Option<AoiSyncDelta>,
}

#[derive(Clone, PartialEq, Message)]
struct AoiSyncDelta {
    #[prost(int64, optional, tag = "1")]
    uuid: Option<i64>,
    #[prost(message, optional, tag = "2")]
    attributes: Option<AttrCollection>,
    #[prost(message, optional, tag = "7")]
    skill_effects: Option<SkillEffect>,
    #[prost(bytes = "vec", optional, tag = "10")]
    buff_effect: Option<Vec<u8>>,
}

#[derive(Clone, PartialEq, Message)]
struct BuffEffectSync {
    #[prost(int64, optional, tag = "1")]
    uuid: Option<i64>,
    #[prost(message, repeated, tag = "2")]
    buff_effects: Vec<BuffEffect>,
}

#[derive(Clone, PartialEq, Message)]
struct BuffEffect {
    #[prost(int32, optional, tag = "1")]
    event_type: Option<i32>,
    #[prost(int32, optional, tag = "2")]
    buff_uuid: Option<i32>,
    #[prost(int64, optional, tag = "3")]
    host_uuid: Option<i64>,
    #[prost(int64, optional, tag = "4")]
    trigger_time: Option<i64>,
    #[prost(message, repeated, tag = "5")]
    logic_effects: Vec<BuffEffectLogicInfo>,
}

#[derive(Clone, PartialEq, Message)]
struct BuffEffectLogicInfo {
    #[prost(int32, optional, tag = "1")]
    effect_type: Option<i32>,
    #[prost(bytes = "vec", optional, tag = "2")]
    raw_data: Option<Vec<u8>>,
    #[prost(bool, optional, tag = "3")]
    is_loop: Option<bool>,
}

#[derive(Clone, PartialEq, Message)]
struct BuffInfo {
    #[prost(int32, optional, tag = "1")]
    buff_uuid: Option<i32>,
    #[prost(int32, optional, tag = "2")]
    base_id: Option<i32>,
    #[prost(int32, optional, tag = "3")]
    level: Option<i32>,
    #[prost(int64, optional, tag = "4")]
    host_uuid: Option<i64>,
    #[prost(int64, optional, tag = "6")]
    create_time: Option<i64>,
    #[prost(int64, optional, tag = "7")]
    fire_uuid: Option<i64>,
    #[prost(int32, optional, tag = "8")]
    layer: Option<i32>,
    #[prost(int32, optional, tag = "9")]
    part_id: Option<i32>,
    #[prost(int32, optional, tag = "10")]
    count: Option<i32>,
    #[prost(int32, optional, tag = "11")]
    duration: Option<i32>,
    #[prost(message, optional, tag = "12")]
    fight_source_info: Option<FightSourceInfo>,
}

#[derive(Clone, Copy, PartialEq, Message)]
struct FightSourceInfo {
    #[prost(int32, optional, tag = "1")]
    source_type_id: Option<i32>,
    #[prost(int32, optional, tag = "2")]
    source_config_id: Option<i32>,
}

#[derive(Clone, Copy, PartialEq, Message)]
struct BuffChange {
    #[prost(int32, optional, tag = "1")]
    layer: Option<i32>,
    #[prost(int32, optional, tag = "2")]
    duration: Option<i32>,
    #[prost(int64, optional, tag = "3")]
    create_time: Option<i64>,
}

#[derive(Clone, PartialEq, Message)]
struct AttrCollection {
    #[prost(int64, optional, tag = "1")]
    uuid: Option<i64>,
    #[prost(message, repeated, tag = "2")]
    attributes: Vec<Attr>,
}

#[derive(Clone, PartialEq, Message)]
struct Attr {
    #[prost(int32, optional, tag = "1")]
    id: Option<i32>,
    #[prost(bytes = "vec", optional, tag = "2")]
    raw_data: Option<Vec<u8>>,
}

#[derive(Clone, PartialEq, Message)]
struct SkillEffect {
    #[prost(int64, optional, tag = "1")]
    uuid: Option<i64>,
    #[prost(message, repeated, tag = "2")]
    damage: Vec<DamageInfo>,
    #[prost(int64, optional, tag = "3")]
    total_damage: Option<i64>,
}

#[derive(Clone, PartialEq, Message)]
struct DamageInfo {
    #[prost(int32, optional, tag = "1")]
    damage_source: Option<i32>,
    #[prost(bool, optional, tag = "2")]
    missed: Option<bool>,
    #[prost(bool, optional, tag = "3")]
    critical: Option<bool>,
    #[prost(int32, optional, tag = "4")]
    damage_type: Option<i32>,
    #[prost(int32, optional, tag = "5")]
    type_flags: Option<i32>,
    #[prost(int64, optional, tag = "6")]
    value: Option<i64>,
    #[prost(int64, optional, tag = "7")]
    actual_value: Option<i64>,
    #[prost(int64, optional, tag = "8")]
    lucky_value: Option<i64>,
    #[prost(int64, optional, tag = "9")]
    hp_loss: Option<i64>,
    #[prost(int64, optional, tag = "10")]
    shield_loss: Option<i64>,
    #[prost(int64, optional, tag = "11")]
    attacker_uuid: Option<i64>,
    #[prost(int32, optional, tag = "12")]
    owner_id: Option<i32>,
    #[prost(int32, optional, tag = "13")]
    owner_level: Option<i32>,
    #[prost(int32, optional, tag = "14")]
    owner_stage: Option<i32>,
    #[prost(int32, optional, tag = "15")]
    hit_event_id: Option<i32>,
    #[prost(bool, optional, tag = "16")]
    normal: Option<bool>,
    #[prost(bool, optional, tag = "17")]
    dead: Option<bool>,
    #[prost(int32, optional, tag = "18")]
    property: Option<i32>,
    #[prost(int64, optional, tag = "21")]
    top_summoner_uuid: Option<i64>,
    #[prost(uint32, optional, tag = "23")]
    passive_uuid: Option<u32>,
    #[prost(bool, optional, tag = "24")]
    rainbow: Option<bool>,
    #[prost(int32, optional, tag = "25")]
    damage_mode: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_packet_varints_without_interpreting_units() {
        assert_eq!(decode_varint(&[0x96, 0x01]), Some(150));
        assert_eq!(decode_varint(&[0xff; 10]), None);
    }

    #[test]
    fn keeps_only_deltas_with_requested_packet_owner_ids() {
        let delta = AoiSyncDelta {
            uuid: Some(22),
            attributes: Some(AttrCollection {
                uuid: Some(22),
                attributes: vec![Attr {
                    id: Some(MAX_HP_ATTRIBUTE_ID),
                    raw_data: Some(vec![0x96, 0x01]),
                }],
            }),
            skill_effects: Some(SkillEffect {
                uuid: Some(22),
                damage: vec![DamageInfo {
                    damage_type: Some(DAMAGE_TYPE_HEAL),
                    value: Some(3),
                    owner_id: Some(3059210),
                    ..DamageInfo::default()
                }],
                total_damage: None,
            }),
            buff_effect: None,
        };
        let report = delta_report(
            7,
            delta,
            &BTreeSet::from([3059210]),
            &mut BTreeMap::new(),
            &mut BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(report.delta_index, 7);
        assert_eq!(report.hp_state.max_hp_final, Some(150));
        assert!(report.effects_in_wire_order[0].requested_ability);
        assert_eq!(report.effects_in_wire_order[0].event_kind, "healing");
        assert_eq!(report.effects_in_wire_order[0].selected_amount, 3);
    }

    #[test]
    fn retains_every_effect_inside_a_selected_delta_in_wire_order() {
        let delta = AoiSyncDelta {
            uuid: Some(22),
            attributes: None,
            skill_effects: Some(SkillEffect {
                uuid: Some(22),
                damage: vec![
                    DamageInfo {
                        damage_type: Some(1),
                        value: Some(17),
                        owner_id: Some(999),
                        ..DamageInfo::default()
                    },
                    DamageInfo {
                        damage_type: Some(DAMAGE_TYPE_HEAL),
                        value: Some(3),
                        owner_id: Some(3059210),
                        ..DamageInfo::default()
                    },
                ],
                total_damage: None,
            }),
            buff_effect: None,
        };
        let report = delta_report(
            0,
            delta,
            &BTreeSet::from([3059210]),
            &mut BTreeMap::new(),
            &mut BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(report.effects_in_wire_order.len(), 2);
        assert!(!report.effects_in_wire_order[0].requested_ability);
        assert_eq!(report.effects_in_wire_order[0].owner_id, Some(999));
        assert!(report.effects_in_wire_order[1].requested_ability);
    }
}
