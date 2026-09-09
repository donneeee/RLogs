use std::{collections::BTreeMap, env, fs::File, io::BufReader, path::PathBuf};

use prost::Message;
use rlogs_game_bpsr::{
    CaptureRecord, CaptureRecordKind, DecodeDisposition, DecoderKind, FragmentKind,
    JsonlJournalReader, PacketDirection, ProtocolPack, ProtocolPackRouteDisposition, RouteKey,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

const WORLD_NTF: u64 = 1_664_308_034;
const MARKER_FIRST: i32 = 1101;
const MARKER_LAST: i32 = 1106;
const MAX_ACTIVE_MARKERS: usize = 16_384;
const MAX_EVIDENCE: usize = 100_000;

fn main() {
    if let Err(error) = run() {
        eprintln!("marker journal audit failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = Arguments::parse()?;
    if !args.private_research {
        return Err(
            "pass --private-research to acknowledge local sensitive journal handling".into(),
        );
    }
    let pack = ProtocolPack::from_json(&std::fs::read(&args.pack)?)?;
    let mut stream =
        JsonlJournalReader::new(BufReader::new(File::open(&args.journal)?)).into_record_stream()?;
    if !pack.matches(&stream.session().game_build) {
        return Err(format!(
            "protocol pack does not match journal game build {}",
            stream.session().game_build.build_id
        )
        .into());
    }
    if stream.session().protocol_pack_digest.as_deref() != Some(pack.digest()) {
        return Err(
            "journal protocol pack digest does not match the selected exact-build pack".into(),
        );
    }
    validate_routes(&pack)?;

    let capture_id = stream.session().capture_id.clone();
    let game_build = stream.session().game_build.build_id.clone();
    let mut state = BTreeMap::new();
    let mut evidence = Vec::new();
    let mut inspected_packet_counts = BTreeMap::new();
    let mut first_inspected_wall_clock_unix_micros = None;
    let mut last_inspected_wall_clock_unix_micros = None;
    while let Some(record) = stream.next_record()? {
        if let CaptureRecordKind::Packet(packet) = &record.kind {
            if let Some(routed) = packet.route {
                if matches!(routed.key.method_id, 6 | 45 | 46)
                    && routed.key == route(routed.key.method_id)
                {
                    *inspected_packet_counts
                        .entry(route_label(routed.key.method_id))
                        .or_insert(0_u64) += 1;
                    if let Some(timestamp) = record.wall_clock_unix_micros {
                        first_inspected_wall_clock_unix_micros.get_or_insert(timestamp);
                        last_inspected_wall_clock_unix_micros = Some(timestamp);
                    }
                }
            }
        }
        audit_record(&record, &capture_id, &mut state, &mut evidence)?;
    }
    let report = Report {
        schema_version: 1,
        capture_id,
        game_build,
        inspected_routes: [route_label(6), route_label(45), route_label(46)],
        inspected_packet_counts,
        first_inspected_wall_clock_unix_micros,
        last_inspected_wall_clock_unix_micros,
        evidence,
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn validate_routes(pack: &ProtocolPack) -> Result<(), Box<dyn std::error::Error>> {
    for (method, decoder) in [
        (6, DecoderKind::SyncNearEntitiesV1),
        (45, DecoderKind::SyncNearDeltaV1),
        (46, DecoderKind::SyncToMeDeltaV1),
    ] {
        let key = route(method);
        if pack.decoder(&key) != Some(decoder)
            || !matches!(
                pack.disposition(Some(&key)),
                DecodeDisposition::Allowed { .. }
            )
            || !matches!(
                pack.route(&key).map(|r| &r.disposition),
                Some(ProtocolPackRouteDisposition::Allowed { .. })
            )
        {
            return Err(format!(
                "selected pack does not allow the expected decoder for {}",
                route_label(method)
            )
            .into());
        }
    }
    Ok(())
}

fn audit_record(
    record: &CaptureRecord,
    capture_id: &str,
    state: &mut BTreeMap<i64, MarkerState>,
    output: &mut Vec<MarkerEvidence>,
) -> Result<(), AuditError> {
    let CaptureRecordKind::Packet(packet) = &record.kind else {
        return Ok(());
    };
    let Some(routed) = packet.route else {
        return Ok(());
    };
    let key = routed.key;
    if !matches!(key.method_id, 6 | 45 | 46) || key != route(key.method_id) {
        return Ok(());
    }
    let Some(payload) = packet.payload.decode_input() else {
        return Ok(());
    };
    let mut deltas = Vec::new();
    match key.method_id {
        6 => {
            for entity in SyncNearEntities::decode(payload)?.appeared {
                if let Some(info) = entity.passive_skill_infos {
                    deltas.push(info.into_delta());
                }
            }
        }
        45 => deltas.extend(SyncNearDeltaInfo::decode(payload)?.deltas),
        46 => {
            if let Some(delta) = SyncToMeDeltaInfo::decode(payload)?
                .delta
                .and_then(|d| d.base_delta)
            {
                deltas.push(delta);
            }
        }
        _ => unreachable!(),
    }
    for delta in deltas {
        if let Some(infos) = delta.passive_skill_infos {
            let actor = infos.actor_uuid;
            for info in infos.passive_infos {
                let Some(skill_id @ MARKER_FIRST..=MARKER_LAST) = info.skill_id else {
                    continue;
                };
                let Some(instance) = info.uuid.map(i64::from) else {
                    continue;
                };
                let marker = MarkerState {
                    skill_id,
                    actor_entity_uuid: actor,
                    target_entity_uuid: info.target_uuid,
                    target_position: decode_position(info.target_position.as_deref()),
                };
                let lifecycle = if state.contains_key(&instance) {
                    Lifecycle::Update
                } else {
                    Lifecycle::Add
                };
                if !state.contains_key(&instance) && state.len() >= MAX_ACTIVE_MARKERS {
                    return Err(AuditError::ActiveMarkerLimit);
                }
                state.insert(instance, marker.clone());
                push_evidence(
                    output,
                    MarkerEvidence::new(
                        record,
                        key.method_id,
                        instance,
                        lifecycle,
                        marker,
                        capture_id,
                    ),
                )?;
            }
        }
        if let Some(ended) = delta.passive_skill_end_infos {
            for instance in ended.passive_uuids {
                if let Some(marker) = state.remove(&instance) {
                    push_evidence(
                        output,
                        MarkerEvidence::new(
                            record,
                            key.method_id,
                            instance,
                            Lifecycle::End,
                            marker,
                            capture_id,
                        ),
                    )?;
                }
            }
        }
    }
    Ok(())
}

fn push_evidence(
    output: &mut Vec<MarkerEvidence>,
    evidence: MarkerEvidence,
) -> Result<(), AuditError> {
    if output.len() >= MAX_EVIDENCE {
        return Err(AuditError::EvidenceLimit);
    }
    output.push(evidence);
    Ok(())
}

#[derive(Debug, thiserror::Error)]
enum AuditError {
    #[error(transparent)]
    Decode(#[from] prost::DecodeError),
    #[error("active marker state exceeded the fail-closed limit")]
    ActiveMarkerLimit,
    #[error("marker evidence exceeded the fail-closed limit")]
    EvidenceLimit,
}

fn decode_position(bytes: Option<&[u8]>) -> Option<PositionAudit> {
    let position = Position::decode(bytes?).ok()?;
    Some(PositionAudit {
        x: position.x,
        y: position.y,
        z: position.z,
    })
}

fn pseudonym(capture_id: &str, namespace: &str, value: i64) -> String {
    let digest = Sha256::digest(format!("{capture_id}\0{namespace}\0{value}").as_bytes());
    let short = u64::from_be_bytes(digest[..8].try_into().expect("SHA-256 prefix"));
    format!("{namespace}_{short:016x}")
}
fn route(method_id: u32) -> RouteKey {
    RouteKey::new(
        PacketDirection::ServerToClient,
        FragmentKind::Notify,
        WORLD_NTF,
        method_id,
    )
}
fn route_label(method_id: u32) -> String {
    format!("server_to_client/notify/{WORLD_NTF}/{method_id}")
}

#[derive(Debug)]
struct Arguments {
    journal: PathBuf,
    pack: PathBuf,
    private_research: bool,
}
impl Arguments {
    fn parse() -> Result<Self, Box<dyn std::error::Error>> {
        let mut journal = None;
        let mut pack = None;
        let mut private_research = false;
        let mut args = env::args_os().skip(1);
        while let Some(arg) = args.next() {
            match arg.to_string_lossy().as_ref() {
                "--journal" => journal = args.next().map(PathBuf::from),
                "--pack" => pack = args.next().map(PathBuf::from),
                "--private-research" => private_research = true,
                other => return Err(format!("unknown argument {other}").into()),
            }
        }
        Ok(Self {
            journal: journal.ok_or("missing --journal")?,
            pack: pack.ok_or("missing --pack")?,
            private_research,
        })
    }
}

#[derive(Debug, Serialize)]
struct Report {
    schema_version: u16,
    capture_id: String,
    game_build: String,
    inspected_routes: [String; 3],
    inspected_packet_counts: BTreeMap<String, u64>,
    first_inspected_wall_clock_unix_micros: Option<i64>,
    last_inspected_wall_clock_unix_micros: Option<i64>,
    evidence: Vec<MarkerEvidence>,
}
#[derive(Debug, Clone, PartialEq)]
struct MarkerState {
    skill_id: i32,
    actor_entity_uuid: Option<i64>,
    target_entity_uuid: Option<i64>,
    target_position: Option<PositionAudit>,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
struct PositionAudit {
    x: Option<f32>,
    y: Option<f32>,
    z: Option<f32>,
}
#[derive(Debug, Serialize)]
struct MarkerEvidence {
    sequence: u64,
    observed_micros: u64,
    wall_clock_unix_micros: Option<i64>,
    route: String,
    lifecycle: Lifecycle,
    marker_number: i32,
    passive_skill_id: i32,
    passive_instance_id: String,
    actor_entity_uuid: Option<String>,
    target_entity_uuid: Option<String>,
    target_position: Option<PositionAudit>,
}
impl MarkerEvidence {
    fn new(
        record: &CaptureRecord,
        method: u32,
        instance: i64,
        lifecycle: Lifecycle,
        marker: MarkerState,
        capture_id: &str,
    ) -> Self {
        Self {
            sequence: record.sequence,
            observed_micros: record.observed_micros,
            wall_clock_unix_micros: record.wall_clock_unix_micros,
            route: route_label(method),
            lifecycle,
            marker_number: marker.skill_id - 1100,
            passive_skill_id: marker.skill_id,
            passive_instance_id: pseudonym(capture_id, "passive", instance),
            actor_entity_uuid: marker
                .actor_entity_uuid
                .map(|value| pseudonym(capture_id, "entity", value)),
            target_entity_uuid: marker
                .target_entity_uuid
                .map(|value| pseudonym(capture_id, "entity", value)),
            target_position: marker.target_position,
        }
    }
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum Lifecycle {
    Add,
    Update,
    End,
}

#[derive(Clone, PartialEq, Message)]
struct Position {
    #[prost(float, optional, tag = "1")]
    x: Option<f32>,
    #[prost(float, optional, tag = "2")]
    y: Option<f32>,
    #[prost(float, optional, tag = "3")]
    z: Option<f32>,
}
#[derive(Clone, PartialEq, Message)]
struct PassiveSkillInfo {
    #[prost(int32, optional, tag = "1")]
    uuid: Option<i32>,
    #[prost(int64, optional, tag = "2")]
    target_uuid: Option<i64>,
    #[prost(int32, optional, tag = "6")]
    skill_id: Option<i32>,
    #[prost(bytes = "vec", optional, tag = "9")]
    target_position: Option<Vec<u8>>,
}
#[derive(Clone, PartialEq, Message)]
struct SeqPassiveSkillInfo {
    #[prost(int64, optional, tag = "1")]
    actor_uuid: Option<i64>,
    #[prost(message, repeated, tag = "2")]
    passive_infos: Vec<PassiveSkillInfo>,
}
impl SeqPassiveSkillInfo {
    fn into_delta(self) -> AoiSyncDelta {
        AoiSyncDelta {
            passive_skill_infos: Some(self),
            passive_skill_end_infos: None,
        }
    }
}
#[derive(Clone, PartialEq, Message)]
struct SeqPassiveSkillEndInfo {
    #[prost(int64, optional, tag = "1")]
    actor_uuid: Option<i64>,
    #[prost(int64, repeated, packed = "true", tag = "2")]
    passive_uuids: Vec<i64>,
}
#[derive(Clone, PartialEq, Message)]
struct Entity {
    #[prost(int64, optional, tag = "1")]
    uuid: Option<i64>,
    #[prost(message, optional, tag = "6")]
    passive_skill_infos: Option<SeqPassiveSkillInfo>,
}
#[derive(Clone, PartialEq, Message)]
struct SyncNearEntities {
    #[prost(message, repeated, tag = "1")]
    appeared: Vec<Entity>,
}
#[derive(Clone, PartialEq, Message)]
struct AoiSyncDelta {
    #[prost(message, optional, tag = "8")]
    passive_skill_infos: Option<SeqPassiveSkillInfo>,
    #[prost(message, optional, tag = "9")]
    passive_skill_end_infos: Option<SeqPassiveSkillEndInfo>,
}
#[derive(Clone, PartialEq, Message)]
struct SyncNearDeltaInfo {
    #[prost(message, repeated, tag = "1")]
    deltas: Vec<AoiSyncDelta>,
}
#[derive(Clone, PartialEq, Message)]
struct AoiSyncToMeDelta {
    #[prost(message, optional, tag = "1")]
    base_delta: Option<AoiSyncDelta>,
}
#[derive(Clone, PartialEq, Message)]
struct SyncToMeDeltaInfo {
    #[prost(message, optional, tag = "1")]
    delta: Option<AoiSyncToMeDelta>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rlogs_game_bpsr::{CompressionState, PacketEnvelope, PacketPayload, RoutedMessage};

    fn record(method: u32, payload: Vec<u8>) -> CaptureRecord {
        CaptureRecord {
            sequence: 7,
            observed_micros: 9,
            wall_clock_unix_micros: Some(11),
            kind: CaptureRecordKind::Packet(PacketEnvelope {
                connection_id: 1,
                stream_id: 1,
                source: None,
                destination: None,
                direction: PacketDirection::ServerToClient,
                fragment: Some(FragmentKind::Notify),
                route: Some(RoutedMessage {
                    key: route(method),
                    stub_id: 0,
                    call_id: None,
                }),
                compression: CompressionState::NotCompressed,
                payload: PacketPayload {
                    wire_bytes: vec![],
                    application_bytes: Some(payload),
                },
            }),
        }
    }
    #[test]
    fn filters_non_marker_passives_and_emits_add_update_end() {
        let info = |skill_id| PassiveSkillInfo {
            uuid: Some(22),
            target_uuid: Some(33),
            skill_id: Some(skill_id),
            target_position: None,
        };
        let start = SyncNearDeltaInfo {
            deltas: vec![AoiSyncDelta {
                passive_skill_infos: Some(SeqPassiveSkillInfo {
                    actor_uuid: Some(44),
                    passive_infos: vec![info(1101), info(999)],
                }),
                passive_skill_end_infos: None,
            }],
        }
        .encode_to_vec();
        let end = SyncNearDeltaInfo {
            deltas: vec![AoiSyncDelta {
                passive_skill_infos: None,
                passive_skill_end_infos: Some(SeqPassiveSkillEndInfo {
                    actor_uuid: Some(44),
                    passive_uuids: vec![22],
                }),
            }],
        }
        .encode_to_vec();
        let mut state = BTreeMap::new();
        let mut out = vec![];
        audit_record(
            &record(45, start.clone()),
            "capture-a",
            &mut state,
            &mut out,
        )
        .unwrap();
        audit_record(&record(45, start), "capture-a", &mut state, &mut out).unwrap();
        audit_record(&record(45, end), "capture-a", &mut state, &mut out).unwrap();
        assert_eq!(out.len(), 3);
        assert!(matches!(out[0].lifecycle, Lifecycle::Add));
        assert!(matches!(out[1].lifecycle, Lifecycle::Update));
        assert!(matches!(out[2].lifecycle, Lifecycle::End));
    }
    #[test]
    fn ignores_disallowed_routes_before_decoding_payload() {
        let mut state = BTreeMap::new();
        let mut out = vec![];
        audit_record(&record(44, vec![0xff]), "capture-a", &mut state, &mut out).unwrap();
        assert!(out.is_empty());
    }
    #[test]
    fn decodes_snapshot_and_to_me_routes() {
        let passive = || SeqPassiveSkillInfo {
            actor_uuid: Some(8),
            passive_infos: vec![PassiveSkillInfo {
                uuid: Some(9),
                target_uuid: None,
                skill_id: Some(1106),
                target_position: None,
            }],
        };
        let snapshot = SyncNearEntities {
            appeared: vec![Entity {
                uuid: Some(8),
                passive_skill_infos: Some(passive()),
            }],
        }
        .encode_to_vec();
        let to_me = SyncToMeDeltaInfo {
            delta: Some(AoiSyncToMeDelta {
                base_delta: Some(passive().into_delta()),
            }),
        }
        .encode_to_vec();
        let mut state = BTreeMap::new();
        let mut out = vec![];
        audit_record(&record(6, snapshot), "capture-a", &mut state, &mut out).unwrap();
        audit_record(&record(46, to_me), "capture-a", &mut state, &mut out).unwrap();
        assert_eq!(out.len(), 2);
        assert!(matches!(out[0].lifecycle, Lifecycle::Add));
        assert!(matches!(out[1].lifecycle, Lifecycle::Update));
    }

    #[test]
    fn identifiers_are_capture_local_and_evidence_is_bounded() {
        assert_eq!(pseudonym("a", "entity", 7), pseudonym("a", "entity", 7));
        assert_ne!(pseudonym("a", "entity", 7), pseudonym("b", "entity", 7));
        assert_ne!(pseudonym("a", "entity", 7), "7");
        let mut output = Vec::with_capacity(MAX_EVIDENCE);
        for _ in 0..MAX_EVIDENCE {
            output.push(MarkerEvidence::new(
                &record(45, vec![]),
                45,
                1,
                Lifecycle::Add,
                MarkerState {
                    skill_id: 1101,
                    actor_entity_uuid: None,
                    target_entity_uuid: None,
                    target_position: None,
                },
                "capture-a",
            ));
        }
        let extra = MarkerEvidence::new(
            &record(45, vec![]),
            45,
            1,
            Lifecycle::Add,
            MarkerState {
                skill_id: 1101,
                actor_entity_uuid: None,
                target_entity_uuid: None,
                target_position: None,
            },
            "capture-a",
        );
        assert!(matches!(
            push_evidence(&mut output, extra),
            Err(AuditError::EvidenceLimit)
        ));

        let marker = MarkerState {
            skill_id: 1101,
            actor_entity_uuid: None,
            target_entity_uuid: None,
            target_position: None,
        };
        let mut full_state = (0..MAX_ACTIVE_MARKERS as i64)
            .map(|instance| (instance, marker.clone()))
            .collect::<BTreeMap<_, _>>();
        let payload = SyncNearDeltaInfo {
            deltas: vec![AoiSyncDelta {
                passive_skill_infos: Some(SeqPassiveSkillInfo {
                    actor_uuid: None,
                    passive_infos: vec![PassiveSkillInfo {
                        uuid: Some(MAX_ACTIVE_MARKERS as i32 + 1),
                        target_uuid: None,
                        skill_id: Some(1101),
                        target_position: None,
                    }],
                }),
                passive_skill_end_infos: None,
            }],
        }
        .encode_to_vec();
        assert!(matches!(
            audit_record(
                &record(45, payload),
                "capture-a",
                &mut full_state,
                &mut vec![]
            ),
            Err(AuditError::ActiveMarkerLimit)
        ));
    }
}
