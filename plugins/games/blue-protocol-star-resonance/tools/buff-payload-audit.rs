use std::{
    collections::BTreeSet,
    ffi::{OsStr, OsString},
    fs::File,
    io::{BufReader, BufWriter},
    path::PathBuf,
};

use prost::Message;
use rlogs_game_bpsr::{CaptureRecordKind, JsonlJournalError, JsonlJournalReader};
use serde::Serialize;

const BUFF_LOGIC_ADD_BUFF: i32 = 18;
const BUFF_LOGIC_CHANGE: i32 = 19;
const MAXIMUM_RECURSION_DEPTH: usize = 6;
const MAXIMUM_FIELDS_PER_MESSAGE: usize = 16_384;
const MAXIMUM_INLINE_BYTES: usize = 96;

fn main() {
    if let Err(error) = run() {
        eprintln!("buff payload audit failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments()?;
    let mut stream = JsonlJournalReader::new(BufReader::new(File::open(&arguments.journal)?))
        .into_record_stream()?;
    let session = stream.session().clone();
    let mut packets = Vec::new();
    let mut truncated_final_record = None;

    loop {
        let record = match stream.next_record() {
            Ok(Some(record)) => record,
            Ok(None) => break,
            Err(JsonlJournalError::InvalidJson { line, source })
                if source.is_eof()
                    && stream
                        .truncated_tail()
                        .is_some_and(|(tail_line, _, _)| tail_line == line) =>
            {
                let (_, bytes, after_observed_micros) =
                    stream.truncated_tail().expect("tail was checked above");
                truncated_final_record = Some(TruncatedTailAudit {
                    line,
                    bytes,
                    after_observed_micros,
                });
                break;
            }
            Err(error) => return Err(error.into()),
        };
        if arguments
            .sequence
            .is_some_and(|sequence| record.sequence != sequence)
        {
            continue;
        }
        let CaptureRecordKind::Packet(packet) = &record.kind else {
            continue;
        };
        let Some(payload) = packet.payload.decode_input() else {
            continue;
        };
        let route = packet.route.map(|route| route.key);
        let decoded = decode_buff_effects(
            payload,
            arguments.target_entity_uuid,
            &arguments.buff_instances,
            &arguments.effect_ids,
        );
        if !arguments.effect_ids.is_empty() && !decoded.has_matching_effects() {
            continue;
        }
        packets.push(PacketAudit {
            record_sequence: record.sequence,
            observed_micros: record.observed_micros,
            route: route.map(|route| RouteAudit {
                direction: format!("{:?}", route.direction),
                fragment: format!("{:?}", route.fragment),
                service_id: route.service_id,
                method_id: route.method_id,
            }),
            application_payload_length: payload.len(),
            application_payload_fields: parse_message(payload, 0),
            decoded,
        });
    }

    let report = AuditReport {
        schema_version: 2,
        journal: arguments.journal.display().to_string(),
        capture_id: session.capture_id,
        game_build: session.game_build.build_id,
        requested_record_sequence: arguments.sequence,
        requested_target_entity_uuid: arguments.target_entity_uuid,
        requested_buff_instances: arguments.buff_instances.into_iter().collect(),
        requested_effect_ids: arguments.effect_ids.into_iter().collect(),
        truncated_final_record,
        packets,
        interpretation_policy: InterpretationPolicy {
            raw_hp_shield_or_resource_evidence_discarded: false,
            unknown_protobuf_fields_discarded: false,
            table_or_description_values_are_formula_truth: false,
            absent_numeric_buff_payload_means_irrelevant: false,
        },
    };
    serde_json::to_writer_pretty(BufWriter::new(File::create(&arguments.output)?), &report)?;
    println!(
        "wrote {} packet audit(s) to {}",
        report.packets.len(),
        arguments.output.display()
    );
    Ok(())
}

#[derive(Debug)]
struct Arguments {
    journal: PathBuf,
    output: PathBuf,
    sequence: Option<u64>,
    target_entity_uuid: Option<i64>,
    buff_instances: BTreeSet<i32>,
    effect_ids: BTreeSet<i32>,
}

fn arguments() -> Result<Arguments, String> {
    let mut values = std::env::args_os().skip(1);
    let mut journal = None;
    let mut output = None;
    let mut sequence = None;
    let mut target_entity_uuid = None;
    let mut buff_instances = BTreeSet::new();
    let mut effect_ids = BTreeSet::new();
    while let Some(argument) = values.next() {
        match argument.to_string_lossy().as_ref() {
            "--journal" => journal = Some(PathBuf::from(required(&mut values, "--journal")?)),
            "--output" => output = Some(PathBuf::from(required(&mut values, "--output")?)),
            "--sequence" => {
                sequence = Some(parse_u64(
                    required(&mut values, "--sequence")?,
                    "--sequence",
                )?)
            }
            "--target-entity" => {
                let value = required(&mut values, "--target-entity")?;
                target_entity_uuid =
                    Some(value.to_string_lossy().parse::<i64>().map_err(|_| {
                        format!("invalid --target-entity {}", value.to_string_lossy())
                    })?);
            }
            "--buff-instance" => {
                let value = required(&mut values, "--buff-instance")?;
                let parsed = value
                    .to_string_lossy()
                    .parse::<i32>()
                    .map_err(|_| format!("invalid --buff-instance {}", value.to_string_lossy()))?;
                buff_instances.insert(parsed);
            }
            "--effect" => {
                let value = required(&mut values, "--effect")?;
                let parsed = value
                    .to_string_lossy()
                    .parse::<i32>()
                    .map_err(|_| format!("invalid --effect {}", value.to_string_lossy()))?;
                effect_ids.insert(parsed);
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    if sequence.is_none() && effect_ids.is_empty() {
        return Err(
            "missing --sequence (or provide at least one --effect to scan the journal)".to_owned(),
        );
    }
    Ok(Arguments {
        journal: journal.ok_or("missing --journal")?,
        output: output.ok_or("missing --output")?,
        sequence,
        target_entity_uuid,
        buff_instances,
        effect_ids,
    })
}

fn required(values: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<OsString, String> {
    values
        .next()
        .ok_or_else(|| format!("missing value for {flag}"))
}

fn parse_u64(value: impl AsRef<OsStr>, flag: &str) -> Result<u64, String> {
    value
        .as_ref()
        .to_string_lossy()
        .parse::<u64>()
        .map_err(|_| format!("invalid {flag}"))
}

#[derive(Debug, Serialize)]
struct AuditReport {
    schema_version: u16,
    journal: String,
    capture_id: String,
    game_build: String,
    requested_record_sequence: Option<u64>,
    requested_target_entity_uuid: Option<i64>,
    requested_buff_instances: Vec<i32>,
    requested_effect_ids: Vec<i32>,
    truncated_final_record: Option<TruncatedTailAudit>,
    packets: Vec<PacketAudit>,
    interpretation_policy: InterpretationPolicy,
}

#[derive(Debug, Serialize)]
struct TruncatedTailAudit {
    line: usize,
    bytes: usize,
    after_observed_micros: u64,
}

#[derive(Debug, Serialize)]
struct InterpretationPolicy {
    raw_hp_shield_or_resource_evidence_discarded: bool,
    unknown_protobuf_fields_discarded: bool,
    table_or_description_values_are_formula_truth: bool,
    absent_numeric_buff_payload_means_irrelevant: bool,
}

#[derive(Debug, Serialize)]
struct PacketAudit {
    record_sequence: u64,
    observed_micros: u64,
    route: Option<RouteAudit>,
    application_payload_length: usize,
    application_payload_fields: WireParse,
    decoded: DecodedPacket,
}

#[derive(Debug, Serialize)]
struct RouteAudit {
    direction: String,
    fragment: String,
    service_id: u64,
    method_id: u32,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum DecodedPacket {
    SyncToMeDelta {
        target_entity_uuid: Option<i64>,
        matching_effects: Vec<BuffEffectAudit>,
        all_buff_instance_ids: Vec<i32>,
    },
    SyncNearDelta {
        matching_effects: Vec<BuffEffectAudit>,
        all_buff_instance_ids: Vec<i32>,
    },
    UnsupportedRoute,
    DecodeFailure {
        detail: String,
    },
}

impl DecodedPacket {
    fn has_matching_effects(&self) -> bool {
        match self {
            Self::SyncToMeDelta {
                matching_effects, ..
            }
            | Self::SyncNearDelta {
                matching_effects, ..
            } => !matching_effects.is_empty(),
            Self::UnsupportedRoute | Self::DecodeFailure { .. } => false,
        }
    }
}

fn decode_buff_effects(
    payload: &[u8],
    target_entity_uuid: Option<i64>,
    requested_instances: &BTreeSet<i32>,
    requested_effects: &BTreeSet<i32>,
) -> DecodedPacket {
    if let Ok(message) = SyncToMeDeltaInfo::decode(payload) {
        if let Some(base_delta) = message.delta.and_then(|delta| delta.base_delta) {
            return decoded_effects_for_delta(
                base_delta,
                target_entity_uuid,
                requested_instances,
                requested_effects,
                true,
            );
        }
    }
    if let Ok(message) = SyncNearDeltaInfo::decode(payload) {
        let mut matching_effects = Vec::new();
        let mut all = BTreeSet::new();
        for delta in message.deltas {
            collect_delta_effects(
                delta,
                target_entity_uuid,
                requested_instances,
                requested_effects,
                &mut matching_effects,
                &mut all,
            );
        }
        return DecodedPacket::SyncNearDelta {
            matching_effects,
            all_buff_instance_ids: all.into_iter().collect(),
        };
    }
    DecodedPacket::DecodeFailure {
        detail: "payload did not decode as a current-build SyncToMeDeltaInfo or SyncNearDeltaInfo"
            .to_owned(),
    }
}

fn decoded_effects_for_delta(
    delta: AoiSyncDelta,
    requested_target_entity_uuid: Option<i64>,
    requested_instances: &BTreeSet<i32>,
    requested_effects: &BTreeSet<i32>,
    to_me: bool,
) -> DecodedPacket {
    let target_entity_uuid = delta.uuid;
    let mut matching_effects = Vec::new();
    let mut all = BTreeSet::new();
    collect_delta_effects(
        delta,
        requested_target_entity_uuid,
        requested_instances,
        requested_effects,
        &mut matching_effects,
        &mut all,
    );
    if to_me {
        DecodedPacket::SyncToMeDelta {
            target_entity_uuid,
            matching_effects,
            all_buff_instance_ids: all.into_iter().collect(),
        }
    } else {
        DecodedPacket::UnsupportedRoute
    }
}

fn collect_delta_effects(
    delta: AoiSyncDelta,
    requested_target_entity_uuid: Option<i64>,
    requested_instances: &BTreeSet<i32>,
    requested_effects: &BTreeSet<i32>,
    matching: &mut Vec<BuffEffectAudit>,
    all: &mut BTreeSet<i32>,
) {
    let Some(raw) = delta.buff_effect else {
        return;
    };
    let Ok(sync) = BuffEffectSync::decode(raw.as_slice()) else {
        return;
    };
    let target_entity_uuid = sync.uuid.or(delta.uuid);
    if requested_target_entity_uuid.is_some() && requested_target_entity_uuid != target_entity_uuid
    {
        return;
    }
    for effect in sync.buff_effects {
        let Some(instance_id) = effect.buff_uuid else {
            continue;
        };
        all.insert(instance_id);
        if !requested_instances.is_empty() && !requested_instances.contains(&instance_id) {
            continue;
        }
        let logic_effects = effect
            .logic_effects
            .into_iter()
            .map(|logic| logic_audit(logic, 0))
            .collect::<Vec<_>>();
        if !requested_effects.is_empty()
            && !logic_effects
                .iter()
                .any(|logic| logic.contains_effect(requested_effects))
        {
            continue;
        }
        matching.push(BuffEffectAudit {
            target_entity_uuid,
            event_type: effect.event_type,
            buff_instance_id: instance_id,
            host_entity_uuid: effect.host_uuid,
            trigger_time: effect.trigger_time,
            logic_effects,
        });
    }
}

fn logic_audit(logic: BuffEffectLogicInfo, depth: usize) -> BuffLogicAudit {
    let raw = logic.raw_data.unwrap_or_default();
    let decoded = match logic.effect_type {
        Some(BUFF_LOGIC_ADD_BUFF) => match BuffInfo::decode(raw.as_slice()) {
            Ok(info) => {
                let nested_logic_effect_count = info.logic_effects.len();
                let (nested_logic_effects, nested_logic_effects_omitted) =
                    if depth < MAXIMUM_RECURSION_DEPTH {
                        (
                            info.logic_effects
                                .into_iter()
                                .map(|nested| logic_audit(nested, depth + 1))
                                .collect(),
                            0,
                        )
                    } else {
                        (Vec::new(), nested_logic_effect_count)
                    };
                LogicDecode::AddBuff {
                    buff_instance_id: info.buff_uuid,
                    effect_id: info.base_id,
                    level: info.level,
                    host_entity_uuid: info.host_uuid,
                    table_entity_uuid: info.table_uuid,
                    source_entity_uuid: info.fire_uuid,
                    stacks: info.layer,
                    part_id: info.part_id,
                    count: info.count,
                    duration_millis: info.duration,
                    create_time_millis: info.create_time,
                    skin_id: info.skin_id,
                    origin_source_type_id: info
                        .fight_source_info
                        .as_ref()
                        .and_then(|source| source.source_type_id),
                    origin_source_config_id: info
                        .fight_source_info
                        .as_ref()
                        .and_then(|source| source.source_config_id),
                    nested_logic_effects,
                    nested_logic_effects_omitted,
                }
            }
            Err(error) => LogicDecode::Failure {
                detail: error.to_string(),
            },
        },
        Some(BUFF_LOGIC_CHANGE) => match BuffChange::decode(raw.as_slice()) {
            Ok(change) => LogicDecode::BuffChange {
                stacks: change.layer,
                duration_millis: change.duration,
                create_time_millis: change.create_time,
            },
            Err(error) => LogicDecode::Failure {
                detail: error.to_string(),
            },
        },
        _ => LogicDecode::NotInterpreted,
    };
    BuffLogicAudit {
        effect_type: logic.effect_type,
        is_loop: logic.is_loop,
        raw_length: raw.len(),
        raw_hex_prefix: hex_prefix(&raw),
        raw_fields: parse_message(&raw, 0),
        decoded,
    }
}

#[derive(Debug, Serialize)]
struct BuffEffectAudit {
    target_entity_uuid: Option<i64>,
    event_type: Option<i32>,
    buff_instance_id: i32,
    host_entity_uuid: Option<i64>,
    trigger_time: Option<i64>,
    logic_effects: Vec<BuffLogicAudit>,
}

#[derive(Debug, Serialize)]
struct BuffLogicAudit {
    effect_type: Option<i32>,
    is_loop: Option<bool>,
    raw_length: usize,
    raw_hex_prefix: String,
    raw_fields: WireParse,
    decoded: LogicDecode,
}

impl BuffLogicAudit {
    fn contains_effect(&self, requested_effects: &BTreeSet<i32>) -> bool {
        match &self.decoded {
            LogicDecode::AddBuff {
                effect_id,
                nested_logic_effects,
                ..
            } => {
                effect_id.is_some_and(|effect_id| requested_effects.contains(&effect_id))
                    || nested_logic_effects
                        .iter()
                        .any(|logic| logic.contains_effect(requested_effects))
            }
            LogicDecode::BuffChange { .. }
            | LogicDecode::NotInterpreted
            | LogicDecode::Failure { .. } => false,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum LogicDecode {
    AddBuff {
        buff_instance_id: Option<i32>,
        effect_id: Option<i32>,
        level: Option<i32>,
        host_entity_uuid: Option<i64>,
        table_entity_uuid: Option<i32>,
        source_entity_uuid: Option<i64>,
        stacks: Option<i32>,
        part_id: Option<i32>,
        count: Option<i32>,
        duration_millis: Option<i32>,
        create_time_millis: Option<i64>,
        skin_id: Option<i32>,
        origin_source_type_id: Option<i32>,
        origin_source_config_id: Option<i32>,
        nested_logic_effects: Vec<BuffLogicAudit>,
        nested_logic_effects_omitted: usize,
    },
    BuffChange {
        stacks: Option<i32>,
        duration_millis: Option<i32>,
        create_time_millis: Option<i64>,
    },
    NotInterpreted,
    Failure {
        detail: String,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum WireParse {
    Complete { fields: Vec<WireField> },
    Invalid { offset: usize, detail: String },
    LimitExceeded,
}

#[derive(Debug, Serialize)]
struct WireField {
    field_number: u32,
    wire_type: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    unsigned_varint: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    signed_twos_complement: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fixed32: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fixed64: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bytes_length: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bytes_hex_prefix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    nested: Option<Box<WireParse>>,
}

fn parse_message(bytes: &[u8], depth: usize) -> WireParse {
    if depth > MAXIMUM_RECURSION_DEPTH {
        return WireParse::LimitExceeded;
    }
    let mut offset = 0usize;
    let mut fields = Vec::new();
    while offset < bytes.len() {
        if fields.len() >= MAXIMUM_FIELDS_PER_MESSAGE {
            return WireParse::LimitExceeded;
        }
        let field_offset = offset;
        let Some(key) = read_varint(bytes, &mut offset) else {
            return WireParse::Invalid {
                offset: field_offset,
                detail: "truncated field key".to_owned(),
            };
        };
        let field_number = (key >> 3) as u32;
        let wire_type = (key & 7) as u8;
        if field_number == 0 {
            return WireParse::Invalid {
                offset: field_offset,
                detail: "field number zero".to_owned(),
            };
        }
        let mut field = WireField {
            field_number,
            wire_type,
            unsigned_varint: None,
            signed_twos_complement: None,
            fixed32: None,
            fixed64: None,
            bytes_length: None,
            bytes_hex_prefix: None,
            nested: None,
        };
        match wire_type {
            0 => {
                let Some(value) = read_varint(bytes, &mut offset) else {
                    return WireParse::Invalid {
                        offset,
                        detail: "truncated varint".to_owned(),
                    };
                };
                field.unsigned_varint = Some(value);
                field.signed_twos_complement = Some(value as i64);
            }
            1 => {
                let Some(raw) = take(bytes, &mut offset, 8) else {
                    return WireParse::Invalid {
                        offset,
                        detail: "truncated fixed64".to_owned(),
                    };
                };
                field.fixed64 = Some(u64::from_le_bytes(raw.try_into().expect("eight bytes")));
            }
            2 => {
                let Some(length) =
                    read_varint(bytes, &mut offset).and_then(|value| usize::try_from(value).ok())
                else {
                    return WireParse::Invalid {
                        offset,
                        detail: "invalid length-delimited size".to_owned(),
                    };
                };
                let Some(raw) = take(bytes, &mut offset, length) else {
                    return WireParse::Invalid {
                        offset,
                        detail: "truncated length-delimited value".to_owned(),
                    };
                };
                field.bytes_length = Some(length);
                field.bytes_hex_prefix = Some(hex_prefix(raw));
                if !raw.is_empty() {
                    let nested = parse_message(raw, depth + 1);
                    if matches!(nested, WireParse::Complete { .. }) {
                        field.nested = Some(Box::new(nested));
                    }
                }
            }
            5 => {
                let Some(raw) = take(bytes, &mut offset, 4) else {
                    return WireParse::Invalid {
                        offset,
                        detail: "truncated fixed32".to_owned(),
                    };
                };
                field.fixed32 = Some(u32::from_le_bytes(raw.try_into().expect("four bytes")));
            }
            _ => {
                return WireParse::Invalid {
                    offset: field_offset,
                    detail: format!("unsupported wire type {wire_type}"),
                };
            }
        }
        fields.push(field);
    }
    WireParse::Complete { fields }
}

fn read_varint(bytes: &[u8], offset: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    for shift in (0..70).step_by(7) {
        let byte = *bytes.get(*offset)?;
        *offset = (*offset).saturating_add(1);
        if shift == 63 && byte > 1 {
            return None;
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

fn take<'a>(bytes: &'a [u8], offset: &mut usize, length: usize) -> Option<&'a [u8]> {
    let end = offset.checked_add(length)?;
    let value = bytes.get(*offset..end)?;
    *offset = end;
    Some(value)
}

fn hex_prefix(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().min(MAXIMUM_INLINE_BYTES) * 2);
    for byte in bytes.iter().take(MAXIMUM_INLINE_BYTES) {
        use std::fmt::Write;
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
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
struct SyncNearDeltaInfo {
    #[prost(message, repeated, tag = "1")]
    deltas: Vec<AoiSyncDelta>,
}

#[derive(Clone, PartialEq, Message)]
struct AoiSyncDelta {
    #[prost(int64, optional, tag = "1")]
    uuid: Option<i64>,
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
    #[prost(int32, optional, tag = "5")]
    table_uuid: Option<i32>,
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
    #[prost(message, repeated, tag = "13")]
    logic_effects: Vec<BuffEffectLogicInfo>,
    #[prost(int32, optional, tag = "14")]
    skin_id: Option<i32>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retains_unknown_varint_and_length_delimited_fields() {
        let bytes = [0x08, 0x05, 0x22, 0x02, 0x08, 0x07, 0x28, 0x96, 0x01];
        let WireParse::Complete { fields } = parse_message(&bytes, 0) else {
            panic!("expected complete parse");
        };
        assert_eq!(
            fields
                .iter()
                .map(|field| field.field_number)
                .collect::<Vec<_>>(),
            [1, 4, 5]
        );
        assert_eq!(fields[0].unsigned_varint, Some(5));
        assert!(fields[1].nested.is_some());
        assert_eq!(fields[2].unsigned_varint, Some(150));
    }

    #[test]
    fn rejects_truncated_fields_without_panicking() {
        assert!(matches!(
            parse_message(&[0x0a, 0x03, 0x01], 0),
            WireParse::Invalid { .. }
        ));
    }

    #[test]
    fn target_filter_prevents_cross_entity_instance_collisions() {
        let mut raw = Vec::new();
        BuffEffectSync {
            uuid: Some(22),
            buff_effects: vec![BuffEffect {
                event_type: Some(6),
                buff_uuid: Some(983),
                host_uuid: Some(22),
                trigger_time: Some(100),
                logic_effects: Vec::new(),
            }],
        }
        .encode(&mut raw)
        .expect("test protobuf should encode");
        let delta = AoiSyncDelta {
            uuid: Some(22),
            buff_effect: Some(raw),
        };
        let requested = BTreeSet::from([983]);
        let requested_effects = BTreeSet::new();
        let mut matching = Vec::new();
        let mut all = BTreeSet::new();

        collect_delta_effects(
            delta,
            Some(11),
            &requested,
            &requested_effects,
            &mut matching,
            &mut all,
        );

        assert!(matching.is_empty());
        assert!(all.is_empty());
    }

    #[test]
    fn effect_filter_matches_nested_add_buff_without_guessing_instance_id() {
        let nested = BuffEffectLogicInfo {
            effect_type: Some(BUFF_LOGIC_ADD_BUFF),
            raw_data: Some(
                BuffInfo {
                    buff_uuid: Some(901),
                    base_id: Some(3_003_052),
                    level: Some(1),
                    host_uuid: Some(22),
                    table_uuid: None,
                    create_time: Some(100),
                    fire_uuid: Some(11),
                    layer: Some(1),
                    part_id: None,
                    count: Some(-1),
                    duration: Some(8_000),
                    fight_source_info: None,
                    logic_effects: Vec::new(),
                    skin_id: None,
                }
                .encode_to_vec(),
            ),
            is_loop: Some(false),
        };
        let outer = BuffEffectLogicInfo {
            effect_type: Some(BUFF_LOGIC_ADD_BUFF),
            raw_data: Some(
                BuffInfo {
                    buff_uuid: Some(900),
                    base_id: Some(3_003_050),
                    level: Some(1),
                    host_uuid: Some(22),
                    table_uuid: None,
                    create_time: Some(100),
                    fire_uuid: Some(11),
                    layer: Some(1),
                    part_id: None,
                    count: Some(-1),
                    duration: Some(8_000),
                    fight_source_info: None,
                    logic_effects: vec![nested],
                    skin_id: None,
                }
                .encode_to_vec(),
            ),
            is_loop: Some(false),
        };
        let audit = logic_audit(outer, 0);

        assert!(audit.contains_effect(&BTreeSet::from([3_003_052])));
        assert!(!audit.contains_effect(&BTreeSet::from([3_003_012])));
    }
}
