//! Pure, process-private projection from lossless method-46 starts into the
//! Automarker confirmation router's parser snapshot.
//!
//! This module owns no capture/native handle and is intentionally not wired
//! into live processing yet. It retains malformed candidates so the existing
//! router/coordinator boundary can reject them without making absence look
//! valid.

#![allow(dead_code)]

use std::{collections::BTreeMap, net::Ipv4Addr};

use rlogs_game_bpsr::{
    AUTOMARKER_REQUEST_BUILD, AUTOMARKER_REQUEST_PACK_DIGEST,
    BPSR_COMPATIBILITY_EPOCH_DEPLOYMENT_ID, CaptureRecord, CaptureRecordKind,
    DecodedLocalMarkerStart, DecoderKind, FragmentKind, LocalMarkerStartDecodeError,
    PacketDirection, ProtocolDecodeStatus, ProtocolPack, bundled_scene_run_identities,
    decode_local_marker_start_candidates,
};

use crate::{
    automarker_confirmation_router::{
        PrivateConfirmationContext, PrivateConfirmationProvenance, PrivateFragmentKind,
        PrivateMarkerAdd, PrivatePacketDirection, PrivateParserConfirmationEvent,
        PrivateParserConfirmationSnapshot, PrivateSourceClocks,
    },
    mechanics_map::{MechanicsMapMarker, MechanicsMapSnapshot},
};

const WORLD_NOTIFICATION_SERVICE_ID: u64 = 1_664_308_034;
const WORLD_SYNC_TO_ME_DELTA_METHOD_ID: u32 = 46;
const MAX_MARKER_CANDIDATES: usize = 4_096;
const MAX_RECORD_TIME_ENTITIES: usize = 4_096;
type MarkerReplacementIdentity = (
    Option<i64>,
    Option<u8>,
    Option<i64>,
    Option<u32>,
    Option<u32>,
    Option<u32>,
);

/// Exact non-serializable session identity needed to construct a router
/// snapshot. Deployment/build/digest are checked against the same pack that
/// produced the lossless candidates and are not copied into routed events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrivateMarkerConfirmationBinding {
    pub session_key: String,
    pub deployment_id: String,
    pub game_build: String,
    pub protocol_pack_digest: String,
    pub scene_family: String,
    pub scene_id: i32,
    pub map_id: u32,
    pub local_actor_id: i64,
    pub connection_epoch: u64,
    pub capture_connection_id: u64,
    pub stream_id: u64,
    pub client_address: [u8; 4],
    pub client_port: u16,
    pub server_address: [u8; 4],
    pub server_port: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrivateMarkerConfirmationIngressError {
    UnsupportedProtocolBinding,
    RecordRouteMismatch,
    MechanicsSnapshotMismatch,
    CandidateLimitExceeded { count: usize, limit: usize },
    EntityLimitExceeded { count: usize, limit: usize },
    PostReplacementRevisionNotStrictlyNew { current: u64, replacement: u64 },
}

/// Opaque, process-private evidence carried across the mechanics-map
/// replacement boundary. The packet record is consumed by `prepare`; this
/// value retains only decoded marker fields, sanitized provenance/clocks, the
/// exact binding, and the minimum record-time mechanics evidence needed to
/// prove a replacement.
#[derive(Debug)]
pub(crate) struct PrivatePreparedMarkerConfirmation {
    binding: PrivateMarkerConfirmationBinding,
    provenance: PrivatePreparedMarkerProvenance,
    source_clocks: PrivateSourceClocks,
    record_mechanics: PrivateRecordMechanicsEvidence,
    candidates: Vec<DecodedLocalMarkerStart>,
}

#[derive(Debug, Clone, Copy)]
struct PrivatePreparedMarkerProvenance {
    capture_sequence: u64,
    connection_id: u64,
    stream_id: u64,
    stub_id: u32,
}

#[derive(Debug)]
struct PrivateRecordMechanicsEvidence {
    revision: u64,
    entities: Vec<PrivateRecordEntityEvidence>,
    markers: Vec<MechanicsMapMarker>,
}

#[derive(Debug, Clone, Copy)]
struct PrivateRecordEntityEvidence {
    actor_id: u64,
    entity_uuid: i64,
    stale: bool,
    last_observed_micros: u64,
}

/// Decode and bind one exact inbound method-46 record before the mechanics
/// map applies that record. No packet bytes or `CaptureRecord` survive this
/// boundary.
pub(crate) fn prepare_marker_confirmation(
    pack: &ProtocolPack,
    record: &CaptureRecord,
    decode_status: ProtocolDecodeStatus,
    record_mechanics: &MechanicsMapSnapshot,
    binding: &PrivateMarkerConfirmationBinding,
) -> Result<PrivatePreparedMarkerConfirmation, PrivateMarkerConfirmationIngressError> {
    validate_protocol_binding(pack, binding)?;
    let (packet, routed) = match &record.kind {
        CaptureRecordKind::Packet(packet) => {
            let Some(routed) = packet.route else {
                return Err(PrivateMarkerConfirmationIngressError::RecordRouteMismatch);
            };
            (packet, routed)
        }
        _ => return Err(PrivateMarkerConfirmationIngressError::RecordRouteMismatch),
    };
    let route = routed.key;
    let exact_source = packet.source.as_ref().is_some_and(|source| {
        source.port == binding.server_port
            && source.address.parse::<Ipv4Addr>().ok()
                == Some(Ipv4Addr::from(binding.server_address))
    });
    let exact_destination = packet.destination.as_ref().is_some_and(|destination| {
        destination.port == binding.client_port
            && destination.address.parse::<Ipv4Addr>().ok()
                == Some(Ipv4Addr::from(binding.client_address))
    });
    if packet.connection_id != binding.capture_connection_id
        || packet.stream_id != binding.stream_id
        || !exact_source
        || !exact_destination
        || packet.direction != PacketDirection::ServerToClient
        || packet.fragment != Some(FragmentKind::Notify)
        || route.direction != PacketDirection::ServerToClient
        || route.fragment != FragmentKind::Notify
        || route.service_id != WORLD_NOTIFICATION_SERVICE_ID
        || route.method_id != WORLD_SYNC_TO_ME_DELTA_METHOD_ID
        || routed.call_id.is_some()
        || pack.decoder(&route) != Some(DecoderKind::SyncToMeDeltaV1)
    {
        return Err(PrivateMarkerConfirmationIngressError::RecordRouteMismatch);
    }
    if record_mechanics.entities.len() > MAX_RECORD_TIME_ENTITIES {
        return Err(PrivateMarkerConfirmationIngressError::EntityLimitExceeded {
            count: record_mechanics.entities.len(),
            limit: MAX_RECORD_TIME_ENTITIES,
        });
    }
    validate_mechanics_binding(record_mechanics, binding)?;

    let candidates =
        decode_local_marker_start_candidates(pack, record, decode_status).map_err(|error| {
            match error {
                LocalMarkerStartDecodeError::UnsupportedProtocol => {
                    PrivateMarkerConfirmationIngressError::UnsupportedProtocolBinding
                }
                LocalMarkerStartDecodeError::EventLimitExceeded { count, limit } => {
                    PrivateMarkerConfirmationIngressError::CandidateLimitExceeded { count, limit }
                }
            }
        })?;

    Ok(PrivatePreparedMarkerConfirmation {
        binding: binding.clone(),
        provenance: PrivatePreparedMarkerProvenance {
            capture_sequence: record.sequence,
            connection_id: packet.connection_id,
            stream_id: packet.stream_id,
            stub_id: routed.stub_id,
        },
        source_clocks: PrivateSourceClocks {
            observed_micros: record.observed_micros,
            wall_clock_unix_micros: record.wall_clock_unix_micros,
        },
        record_mechanics: PrivateRecordMechanicsEvidence {
            revision: record_mechanics.revision,
            entities: record_mechanics
                .entities
                .iter()
                .map(|entity| PrivateRecordEntityEvidence {
                    actor_id: entity.actor_id,
                    entity_uuid: entity.entity_uuid,
                    stale: entity.stale,
                    last_observed_micros: entity.last_observed_micros,
                })
                .collect(),
            markers: record_mechanics.markers.clone(),
        },
        candidates,
    })
}

/// Consume prepared packet evidence after the mechanics map has applied the
/// same record. Only an exact, strictly newer marker replacement can upgrade a
/// decoded candidate to authoritative confirmation evidence.
pub(crate) fn complete_marker_confirmation(
    prepared: PrivatePreparedMarkerConfirmation,
    post_replacement_mechanics: &MechanicsMapSnapshot,
) -> Result<PrivateParserConfirmationSnapshot, PrivateMarkerConfirmationIngressError> {
    let PrivatePreparedMarkerConfirmation {
        binding,
        provenance: prepared_provenance,
        source_clocks,
        record_mechanics,
        candidates,
    } = prepared;

    if post_replacement_mechanics.entities.len() > MAX_RECORD_TIME_ENTITIES {
        return Err(PrivateMarkerConfirmationIngressError::EntityLimitExceeded {
            count: post_replacement_mechanics.entities.len(),
            limit: MAX_RECORD_TIME_ENTITIES,
        });
    }
    validate_mechanics_binding(post_replacement_mechanics, &binding)?;

    project_prepared_marker_confirmation(
        binding,
        prepared_provenance,
        source_clocks,
        record_mechanics,
        candidates,
        post_replacement_mechanics,
    )
}

/// Preserve every decoded start as one private marker event. Structurally
/// exact candidates receive the strictly newer post-replacement mechanics
/// revision. Every invalid candidate keeps the unchanged record-time revision
/// so it cannot impersonate fresh authoritative replacement evidence.
pub(crate) fn project_marker_confirmation_snapshot(
    pack: &ProtocolPack,
    record: &CaptureRecord,
    decode_status: ProtocolDecodeStatus,
    record_mechanics: &MechanicsMapSnapshot,
    post_replacement_mechanics: &MechanicsMapSnapshot,
    binding: &PrivateMarkerConfirmationBinding,
) -> Result<PrivateParserConfirmationSnapshot, PrivateMarkerConfirmationIngressError> {
    let prepared =
        prepare_marker_confirmation(pack, record, decode_status, record_mechanics, binding)?;
    complete_marker_confirmation(prepared, post_replacement_mechanics)
}

fn project_prepared_marker_confirmation(
    binding: PrivateMarkerConfirmationBinding,
    prepared_provenance: PrivatePreparedMarkerProvenance,
    source_clocks: PrivateSourceClocks,
    record_mechanics: PrivateRecordMechanicsEvidence,
    candidates: Vec<DecodedLocalMarkerStart>,
    post_replacement_mechanics: &MechanicsMapSnapshot,
) -> Result<PrivateParserConfirmationSnapshot, PrivateMarkerConfirmationIngressError> {
    let projections = candidates
        .iter()
        .map(|candidate| {
            let owner_actor_id = candidate.owner_entity_uuid.and_then(|owner| {
                unique_record_actor_id(
                    &record_mechanics.entities,
                    owner,
                    Some(source_clocks.observed_micros),
                )
            });
            let post_owner_matches = candidate.owner_entity_uuid.is_some_and(|owner| {
                unique_snapshot_actor_id(post_replacement_mechanics, owner, None) == owner_actor_id
            });
            let exact = post_owner_matches
                && owner_actor_id.is_some_and(|owner_actor_id| {
                    exact_marker_candidate(candidate, Some(owner_actor_id))
                        && marker_replacement_proven(
                            candidate,
                            owner_actor_id,
                            source_clocks.observed_micros,
                            &record_mechanics.markers,
                            post_replacement_mechanics,
                        )
                });
            (candidate, owner_actor_id, exact)
        })
        .collect::<Vec<_>>();
    let replacement_identity_counts = projections.iter().filter(|(_, _, exact)| *exact).fold(
        BTreeMap::new(),
        |mut counts, (candidate, _, _)| {
            *counts
                .entry(replacement_identity(candidate))
                .or_insert(0_usize) += 1;
            counts
        },
    );
    let projections = projections
        .iter()
        .map(|(candidate, owner_actor_id, exact)| {
            let uniquely_attributed = *exact
                && replacement_identity_counts.get(&replacement_identity(candidate)) == Some(&1);
            (*candidate, *owner_actor_id, uniquely_attributed)
        })
        .collect::<Vec<_>>();
    let has_exact_candidate = projections.iter().any(|(_, _, exact)| *exact);
    if has_exact_candidate && post_replacement_mechanics.revision <= record_mechanics.revision {
        return Err(
            PrivateMarkerConfirmationIngressError::PostReplacementRevisionNotStrictlyNew {
                current: record_mechanics.revision,
                replacement: post_replacement_mechanics.revision,
            },
        );
    }

    let provenance = |record_event_index| PrivateConfirmationProvenance {
        capture_sequence: prepared_provenance.capture_sequence,
        record_event_index,
        connection_id: prepared_provenance.connection_id,
        stream_id: prepared_provenance.stream_id,
        direction: PrivatePacketDirection::ServerToClient,
        fragment: PrivateFragmentKind::Notify,
        route_resolved: true,
        service_id: WORLD_NOTIFICATION_SERVICE_ID,
        method_id: WORLD_SYNC_TO_ME_DELTA_METHOD_ID,
        stub_id: prepared_provenance.stub_id,
        call_id: None,
    };
    let events = projections
        .into_iter()
        .map(|(candidate, owner_actor_id, exact)| {
            PrivateParserConfirmationEvent::MarkerAdd(PrivateMarkerAdd {
                provenance: provenance(candidate.record_event_index),
                source_clocks,
                asserted_authoritative_server_decode: candidate.asserted_authoritative_decode
                    && exact,
                raw_skill_id: candidate.raw_skill_id,
                derived_marker_number: candidate.derived_marker_number,
                marker_owner_actor_id: owner_actor_id,
                marker_owner_entity_uuid: candidate.owner_entity_uuid,
                passive_instance_identity: candidate.passive_instance_identity,
                target_position_present: candidate.target_position_present,
                target_position_decode_valid: candidate.target_position_decode_valid,
                x: candidate.x,
                y: candidate.y,
                z: candidate.z,
                runtime_revision: if exact {
                    post_replacement_mechanics.revision
                } else {
                    record_mechanics.revision
                },
            })
        })
        .collect();

    Ok(PrivateParserConfirmationSnapshot {
        session_key: binding.session_key.clone(),
        context: PrivateConfirmationContext {
            game_build: binding.game_build.clone(),
            scene_family: binding.scene_family.clone(),
            local_actor_id: binding.local_actor_id,
            connection_epoch: binding.connection_epoch,
            client_address: binding.client_address,
            client_port: binding.client_port,
            server_address: binding.server_address,
            server_port: binding.server_port,
            runtime_revision: if has_exact_candidate {
                post_replacement_mechanics.revision
            } else {
                record_mechanics.revision
            },
        },
        events,
    })
}

fn validate_protocol_binding(
    pack: &ProtocolPack,
    binding: &PrivateMarkerConfirmationBinding,
) -> Result<(), PrivateMarkerConfirmationIngressError> {
    let target = &pack.definition().target;
    let exact_scene_family = bundled_scene_run_identities()
        .ok()
        .and_then(|identities| identities.get(&binding.scene_id).cloned())
        .and_then(|identity| identity.activity_family_id);
    if binding.session_key.is_empty()
        || binding.scene_family.is_empty()
        || binding.local_actor_id <= 0
        || binding.connection_epoch == 0
        || binding.capture_connection_id == 0
        || binding.stream_id == 0
        || binding.client_port == 0
        || binding.server_port == 0
        || binding.deployment_id != BPSR_COMPATIBILITY_EPOCH_DEPLOYMENT_ID
        || binding.game_build != AUTOMARKER_REQUEST_BUILD
        || binding.protocol_pack_digest != AUTOMARKER_REQUEST_PACK_DIGEST
        || target.deployment_id != binding.deployment_id
        || target.build_id != binding.game_build
        || pack.digest() != binding.protocol_pack_digest
        || exact_scene_family.as_deref() != Some(binding.scene_family.as_str())
    {
        return Err(PrivateMarkerConfirmationIngressError::UnsupportedProtocolBinding);
    }
    Ok(())
}

fn validate_mechanics_binding(
    mechanics: &MechanicsMapSnapshot,
    binding: &PrivateMarkerConfirmationBinding,
) -> Result<(), PrivateMarkerConfirmationIngressError> {
    let local_actor_id = mechanics
        .local_actor_id
        .and_then(|id| i64::try_from(id).ok());
    if mechanics.session_id.as_deref() != Some(binding.session_key.as_str())
        || mechanics.client_build.as_deref() != Some(binding.game_build.as_str())
        || mechanics.scene_id != Some(binding.scene_id)
        || mechanics.map_id != Some(binding.map_id)
        || local_actor_id != Some(binding.local_actor_id)
    {
        return Err(PrivateMarkerConfirmationIngressError::MechanicsSnapshotMismatch);
    }
    Ok(())
}

fn unique_record_actor_id(
    entities: &[PrivateRecordEntityEvidence],
    owner: i64,
    no_later_than_micros: Option<u64>,
) -> Option<i64> {
    unique_actor_id(
        entities.iter().map(|entity| {
            (
                entity.actor_id,
                entity.entity_uuid,
                entity.stale,
                entity.last_observed_micros,
            )
        }),
        owner,
        no_later_than_micros,
    )
}

fn unique_snapshot_actor_id(
    mechanics: &MechanicsMapSnapshot,
    owner: i64,
    no_later_than_micros: Option<u64>,
) -> Option<i64> {
    unique_actor_id(
        mechanics.entities.iter().map(|entity| {
            (
                entity.actor_id,
                entity.entity_uuid,
                entity.stale,
                entity.last_observed_micros,
            )
        }),
        owner,
        no_later_than_micros,
    )
}

fn unique_actor_id(
    entities: impl Iterator<Item = (u64, i64, bool, u64)>,
    owner: i64,
    no_later_than_micros: Option<u64>,
) -> Option<i64> {
    let mut matching = entities.filter(|(_, entity_uuid, stale, last_observed_micros)| {
        *entity_uuid == owner
            && !*stale
            && no_later_than_micros.is_none_or(|maximum| *last_observed_micros <= maximum)
    });
    let (actor_id, _, _, _) = matching.next()?;
    if matching.next().is_some() {
        return None;
    }
    i64::try_from(actor_id)
        .ok()
        .filter(|actor_id| *actor_id > 0)
}

fn exact_marker_candidate(
    candidate: &DecodedLocalMarkerStart,
    owner_actor_id: Option<i64>,
) -> bool {
    let exact_number = matches!(
        (candidate.raw_skill_id, candidate.derived_marker_number),
        (Some(raw), Some(number))
            if (1..=6).contains(&number) && raw == 1_100 + i32::from(number)
    );
    candidate.asserted_authoritative_decode
        && exact_number
        && candidate.owner_entity_uuid.is_some()
        && owner_actor_id.is_some()
        && candidate
            .passive_instance_identity
            .is_some_and(|identity| identity != 0)
        && candidate.target_position_present
        && candidate.target_position_decode_valid
        && [candidate.x, candidate.y, candidate.z]
            .into_iter()
            .all(|axis| axis.is_some_and(f32::is_finite))
}

fn marker_replacement_proven(
    candidate: &DecodedLocalMarkerStart,
    owner_actor_id: i64,
    record_observed_micros: u64,
    record_markers: &[MechanicsMapMarker],
    post_replacement_mechanics: &MechanicsMapSnapshot,
) -> bool {
    let mut post_matches = post_replacement_mechanics.markers.iter().filter(|marker| {
        marker_matches_candidate(marker, candidate, owner_actor_id, record_observed_micros)
    });
    let Some(_) = post_matches.next() else {
        return false;
    };
    if post_matches.next().is_some() {
        return false;
    }
    !record_markers.iter().any(|marker| {
        marker_matches_candidate(marker, candidate, owner_actor_id, record_observed_micros)
    })
}

fn marker_matches_candidate(
    marker: &MechanicsMapMarker,
    candidate: &DecodedLocalMarkerStart,
    owner_actor_id: i64,
    record_observed_micros: u64,
) -> bool {
    let (Some(marker_number), Some(x), Some(y), Some(z), Some(acknowledgment)) = (
        candidate.derived_marker_number,
        candidate.x,
        candidate.y,
        candidate.z,
        marker.acknowledgment.as_ref(),
    ) else {
        return false;
    };
    let Ok(owner_actor_id) = u64::try_from(owner_actor_id) else {
        return false;
    };
    marker.marker_id == candidate.passive_instance_identity
        && marker.marker_number == Some(marker_number)
        && marker.related_actor_id == Some(owner_actor_id)
        && marker
            .x
            .is_some_and(|actual| actual.to_bits() == x.to_bits())
        && marker
            .y
            .is_some_and(|actual| actual.to_bits() == y.to_bits())
        && marker
            .z
            .is_some_and(|actual| actual.to_bits() == z.to_bits())
        && acknowledgment.marker_number == marker_number
        && acknowledgment.slot_id == 200 + i32::from(marker_number)
        && Some(acknowledgment.skill_id) == candidate.raw_skill_id
        && acknowledgment.observed_micros == record_observed_micros
}

fn replacement_identity(candidate: &DecodedLocalMarkerStart) -> MarkerReplacementIdentity {
    (
        candidate.passive_instance_identity,
        candidate.derived_marker_number,
        candidate.owner_entity_uuid,
        candidate.x.map(f32::to_bits),
        candidate.y.map(f32::to_bits),
        candidate.z.map(f32::to_bits),
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rlogs_game_bpsr::{
        CompressionState, LiveProtocolPackKind, LiveProtocolPackSelection, NetworkEndpoint,
        PacketEnvelope, PacketPayload, RouteKey, RoutedMessage,
    };

    fn push_varint(output: &mut Vec<u8>, mut value: u64) {
        while value >= 0x80 {
            output.push((value as u8) | 0x80);
            value >>= 7;
        }
        output.push(value as u8);
    }

    fn push_varint_field(output: &mut Vec<u8>, tag: u8, value: u64) {
        push_varint(output, u64::from(tag) << 3);
        push_varint(output, value);
    }

    fn push_bytes_field(output: &mut Vec<u8>, tag: u8, bytes: &[u8]) {
        push_varint(output, (u64::from(tag) << 3) | 2);
        push_varint(output, bytes.len() as u64);
        output.extend_from_slice(bytes);
    }

    fn encoded_position(candidate: &DecodedLocalMarkerStart) -> Vec<u8> {
        let mut bytes = Vec::new();
        for (tag, axis) in [(1, candidate.x), (2, candidate.y), (3, candidate.z)] {
            if let Some(axis) = axis {
                push_varint(&mut bytes, (tag << 3) | 5);
                bytes.extend_from_slice(&axis.to_bits().to_le_bytes());
            }
        }
        bytes
    }

    fn encoded_marker_payload(candidates: &[DecodedLocalMarkerStart]) -> Vec<u8> {
        let owner_entity_uuid = candidates
            .first()
            .and_then(|candidate| candidate.owner_entity_uuid);
        assert!(
            candidates
                .iter()
                .all(|candidate| candidate.owner_entity_uuid == owner_entity_uuid),
            "one method-46 repeated field has one enclosing owner"
        );
        let mut starts = Vec::new();
        if let Some(owner) = owner_entity_uuid {
            push_varint_field(&mut starts, 1, owner as u64);
        }
        for candidate in candidates {
            let mut start = Vec::new();
            if let Some(identity) = candidate.passive_instance_identity {
                push_varint_field(&mut start, 1, identity as u64);
            }
            if let Some(skill) = candidate.raw_skill_id {
                push_varint_field(&mut start, 6, skill as u64);
            }
            if candidate.target_position_present {
                let position = if candidate.target_position_decode_valid {
                    encoded_position(candidate)
                } else {
                    vec![0xff]
                };
                push_bytes_field(&mut start, 9, &position);
            }
            push_bytes_field(&mut starts, 2, &start);
        }
        let mut base = Vec::new();
        push_bytes_field(&mut base, 8, &starts);
        let mut to_me = Vec::new();
        push_bytes_field(&mut to_me, 1, &base);
        let mut message = Vec::new();
        push_bytes_field(&mut message, 1, &to_me);
        message
    }

    use super::*;
    use crate::{
        automarker_confirmation_router::PrivateParserConfirmationEvent,
        mechanics_map::{MechanicsMapEntity, MechanicsMapMarker, MechanicsMapMarkerAcknowledgment},
    };

    fn pack() -> ProtocolPack {
        LiveProtocolPackSelection {
            path: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
                "../../plugins/games/blue-protocol-star-resonance/protocol-packs/global/steam-24687926/pack.json",
            ),
            build_id: AUTOMARKER_REQUEST_BUILD.to_owned(),
            pack_build_id: "24687926".to_owned(),
            deployment_id: BPSR_COMPATIBILITY_EPOCH_DEPLOYMENT_ID.to_owned(),
            channel: "steam".to_owned(),
            kind: LiveProtocolPackKind::CompatibilityFallback,
        }
        .load_pack()
        .unwrap()
    }

    fn binding(pack: &ProtocolPack) -> PrivateMarkerConfirmationBinding {
        PrivateMarkerConfirmationBinding {
            session_key: "capture-session".to_owned(),
            deployment_id: pack.definition().target.deployment_id.clone(),
            game_build: pack.definition().target.build_id.clone(),
            protocol_pack_digest: pack.digest().to_owned(),
            scene_family: "mech-facility".to_owned(),
            scene_id: 6_525,
            map_id: 6_525,
            local_actor_id: 7,
            connection_epoch: 9,
            capture_connection_id: 4,
            stream_id: 5,
            client_address: [10, 0, 0, 2],
            client_port: 50_000,
            server_address: [10, 0, 0, 3],
            server_port: 443,
        }
    }

    fn record(candidates: &[DecodedLocalMarkerStart]) -> CaptureRecord {
        let application_bytes = encoded_marker_payload(candidates);
        CaptureRecord {
            sequence: 100,
            observed_micros: 200,
            wall_clock_unix_micros: Some(300),
            kind: CaptureRecordKind::Packet(PacketEnvelope {
                connection_id: 4,
                stream_id: 5,
                source: Some(NetworkEndpoint {
                    address: "10.0.0.3".to_owned(),
                    port: 443,
                }),
                destination: Some(NetworkEndpoint {
                    address: "10.0.0.2".to_owned(),
                    port: 50_000,
                }),
                direction: PacketDirection::ServerToClient,
                fragment: Some(FragmentKind::Notify),
                route: Some(RoutedMessage {
                    key: RouteKey::new(
                        PacketDirection::ServerToClient,
                        FragmentKind::Notify,
                        WORLD_NOTIFICATION_SERVICE_ID,
                        WORLD_SYNC_TO_ME_DELTA_METHOD_ID,
                    ),
                    stub_id: 6,
                    call_id: None,
                }),
                compression: CompressionState::NotCompressed,
                payload: PacketPayload {
                    wire_bytes: Vec::new(),
                    application_bytes: Some(application_bytes),
                },
            }),
        }
    }

    fn mechanics(revision: u64) -> MechanicsMapSnapshot {
        MechanicsMapSnapshot {
            revision,
            session_id: Some("capture-session".to_owned()),
            client_build: Some(AUTOMARKER_REQUEST_BUILD.to_owned()),
            scene_id: Some(6_525),
            map_id: Some(6_525),
            local_actor_id: Some(7),
            entities: vec![MechanicsMapEntity {
                actor_id: 7,
                entity_uuid: 90,
                kind: "player",
                display_name: None,
                monster_id: None,
                mechanic_role: None,
                x: 0.0,
                y: 0.0,
                z: 0.0,
                facing_radians: None,
                dead: false,
                stale: false,
                last_observed_micros: 190,
            }],
            ..MechanicsMapSnapshot::default()
        }
    }

    fn replacement_mechanics(revision: u64) -> MechanicsMapSnapshot {
        let mut mechanics = mechanics(revision);
        mechanics.markers = vec![MechanicsMapMarker {
            marker_id: Some(11),
            marker_number: Some(1),
            related_actor_id: Some(7),
            x: Some(1.0),
            y: Some(2.0),
            z: Some(3.0),
            acknowledgment: Some(MechanicsMapMarkerAcknowledgment {
                marker_number: 1,
                slot_id: 201,
                skill_id: 1_101,
                observed_micros: 200,
            }),
        }];
        mechanics
    }

    fn candidate(index: u32) -> DecodedLocalMarkerStart {
        DecodedLocalMarkerStart {
            record_event_index: index,
            raw_skill_id: Some(1_101),
            derived_marker_number: Some(1),
            owner_entity_uuid: Some(90),
            passive_instance_identity: Some(11),
            target_position_present: true,
            target_position_decode_valid: true,
            x: Some(1.0),
            y: Some(2.0),
            z: Some(3.0),
            asserted_authoritative_decode: true,
        }
    }

    fn marker(event: &PrivateParserConfirmationEvent) -> &PrivateMarkerAdd {
        let PrivateParserConfirmationEvent::MarkerAdd(marker) = event else {
            panic!("expected marker event");
        };
        marker
    }

    #[test]
    fn two_stage_confirmation_matches_the_compatibility_wrapper_without_retaining_packet_bytes() {
        let pack = pack();
        let binding = binding(&pack);
        let current = mechanics(20);
        let replacement = replacement_mechanics(21);
        let mut captured_record = record(&[candidate(0)]);
        let expected = project_marker_confirmation_snapshot(
            &pack,
            &captured_record,
            ProtocolDecodeStatus::Decoded,
            &current,
            &replacement,
            &binding,
        )
        .unwrap();

        let prepared = prepare_marker_confirmation(
            &pack,
            &captured_record,
            ProtocolDecodeStatus::Decoded,
            &current,
            &binding,
        )
        .unwrap();
        let prepared_debug = format!("{prepared:?}");
        assert!(!prepared_debug.contains("CaptureRecord"));
        assert!(!prepared_debug.contains("wire_bytes"));
        assert!(!prepared_debug.contains("application_bytes"));

        // The opaque token owns only sanitized fields. Destroying the source
        // record before POST cannot change the confirmation result.
        let CaptureRecordKind::Packet(packet) = &mut captured_record.kind else {
            unreachable!();
        };
        packet.payload.application_bytes = Some(vec![0xa5; 32_768]);
        packet.payload.wire_bytes = vec![0x5a; 32_768];
        drop(captured_record);

        let actual = complete_marker_confirmation(prepared, &replacement).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn two_stage_post_rejects_wrong_revision_and_snapshot_context() {
        let pack = pack();
        let binding = binding(&pack);

        for wrong_revision in [19, 20] {
            let prepared = prepare_marker_confirmation(
                &pack,
                &record(&[candidate(0)]),
                ProtocolDecodeStatus::Decoded,
                &mechanics(20),
                &binding,
            )
            .unwrap();
            assert_eq!(
                complete_marker_confirmation(prepared, &replacement_mechanics(wrong_revision)),
                Err(
                    PrivateMarkerConfirmationIngressError::PostReplacementRevisionNotStrictlyNew {
                        current: 20,
                        replacement: wrong_revision,
                    }
                )
            );
        }

        let mutations: [fn(&mut MechanicsMapSnapshot); 3] = [
            |snapshot: &mut MechanicsMapSnapshot| snapshot.scene_id = Some(6_565),
            |snapshot: &mut MechanicsMapSnapshot| snapshot.map_id = Some(6_565),
            |snapshot: &mut MechanicsMapSnapshot| {
                snapshot.session_id = Some("different-session".to_owned())
            },
        ];
        for mutate in mutations {
            let prepared = prepare_marker_confirmation(
                &pack,
                &record(&[candidate(0)]),
                ProtocolDecodeStatus::Decoded,
                &mechanics(20),
                &binding,
            )
            .unwrap();
            let mut wrong_context = replacement_mechanics(21);
            mutate(&mut wrong_context);
            assert_eq!(
                complete_marker_confirmation(prepared, &wrong_context),
                Err(PrivateMarkerConfirmationIngressError::MechanicsSnapshotMismatch)
            );
        }
    }

    #[test]
    fn two_stage_duplicate_candidates_remain_lossless_and_non_authoritative() {
        let pack = pack();
        let binding = binding(&pack);
        let prepared = prepare_marker_confirmation(
            &pack,
            &record(&[candidate(0), candidate(1)]),
            ProtocolDecodeStatus::Decoded,
            &mechanics(20),
            &binding,
        )
        .unwrap();
        let snapshot = complete_marker_confirmation(prepared, &replacement_mechanics(21)).unwrap();

        assert_eq!(snapshot.events.len(), 2);
        assert_eq!(marker(&snapshot.events[0]).provenance.record_event_index, 0);
        assert_eq!(marker(&snapshot.events[1]).provenance.record_event_index, 1);
        assert!(snapshot.events.iter().all(|event| {
            let marker = marker(event);
            marker.runtime_revision == 20 && !marker.asserted_authoritative_server_decode
        }));
    }

    #[test]
    fn duplicate_wire_candidates_are_preserved_but_cannot_share_one_replacement_proof() {
        let pack = pack();
        let candidates = [candidate(0), candidate(1)];
        let snapshot = project_marker_confirmation_snapshot(
            &pack,
            &record(&candidates),
            ProtocolDecodeStatus::Decoded,
            &mechanics(20),
            &replacement_mechanics(21),
            &binding(&pack),
        )
        .unwrap();
        assert_eq!(snapshot.events.len(), 2);
        assert_eq!(marker(&snapshot.events[0]).provenance.record_event_index, 0);
        assert_eq!(marker(&snapshot.events[1]).provenance.record_event_index, 1);
        assert_eq!(marker(&snapshot.events[0]).marker_owner_actor_id, Some(7));
        for event in &snapshot.events {
            assert_eq!(marker(event).runtime_revision, 20);
            assert!(!marker(event).asserted_authoritative_server_decode);
        }
        assert_eq!(snapshot.context.runtime_revision, 20);
    }

    #[test]
    fn mixed_batch_never_lends_replacement_authority_to_an_invalid_candidate() {
        let pack = pack();
        let mut invalid = candidate(1);
        invalid.z = Some(f32::NAN);
        let candidates = [candidate(0), invalid];
        let snapshot = project_marker_confirmation_snapshot(
            &pack,
            &record(&candidates),
            ProtocolDecodeStatus::Decoded,
            &mechanics(20),
            &replacement_mechanics(21),
            &binding(&pack),
        )
        .unwrap();
        assert_eq!(snapshot.context.runtime_revision, 21);
        assert_eq!(marker(&snapshot.events[0]).runtime_revision, 21);
        assert!(marker(&snapshot.events[0]).asserted_authoritative_server_decode);
        assert_eq!(marker(&snapshot.events[1]).runtime_revision, 20);
        assert!(!marker(&snapshot.events[1]).asserted_authoritative_server_decode);
    }

    #[test]
    fn unrelated_revision_unchanged_marker_and_post_marker_mismatches_stay_stale() {
        let pack = pack();
        let exact = candidate(0);
        let record = record(std::slice::from_ref(&exact));
        let project = |current: &MechanicsMapSnapshot, replacement: &MechanicsMapSnapshot| {
            project_marker_confirmation_snapshot(
                &pack,
                &record,
                ProtocolDecodeStatus::Decoded,
                current,
                replacement,
                &binding(&pack),
            )
            .unwrap()
        };

        // A revision that is already far beyond any plausible rewrite stamp
        // still cannot carry authority without the specific replacement.
        let unrelated = project(&mechanics(10_000), &mechanics(10_001));
        assert_eq!(marker(&unrelated.events[0]).runtime_revision, 10_000);
        assert!(!marker(&unrelated.events[0]).asserted_authoritative_server_decode);

        let unchanged = project(&replacement_mechanics(20), &replacement_mechanics(21));
        assert_eq!(marker(&unchanged.events[0]).runtime_revision, 20);

        let mut wrong_number = replacement_mechanics(21);
        wrong_number.markers[0].marker_number = Some(2);
        let mut wrong_owner = replacement_mechanics(21);
        wrong_owner.markers[0].related_actor_id = Some(8);
        let mut wrong_instance = replacement_mechanics(21);
        wrong_instance.markers[0].marker_id = Some(12);
        let mut wrong_xyz = replacement_mechanics(21);
        wrong_xyz.markers[0].x = Some(f32::from_bits(1.0_f32.to_bits() + 1));
        let mut wrong_ack_number = replacement_mechanics(21);
        wrong_ack_number.markers[0]
            .acknowledgment
            .as_mut()
            .unwrap()
            .marker_number = 2;
        let mut wrong_ack_skill = replacement_mechanics(21);
        wrong_ack_skill.markers[0]
            .acknowledgment
            .as_mut()
            .unwrap()
            .skill_id = 1_102;
        let mut wrong_ack_clock = replacement_mechanics(21);
        wrong_ack_clock.markers[0]
            .acknowledgment
            .as_mut()
            .unwrap()
            .observed_micros = 199;
        for replacement in [
            wrong_number,
            wrong_owner,
            wrong_instance,
            wrong_xyz,
            wrong_ack_number,
            wrong_ack_skill,
            wrong_ack_clock,
        ] {
            let snapshot = project(&mechanics(20), &replacement);
            assert_eq!(marker(&snapshot.events[0]).runtime_revision, 20);
            assert!(!marker(&snapshot.events[0]).asserted_authoritative_server_decode);
            assert_eq!(snapshot.context.runtime_revision, 20);
        }
    }

    #[test]
    fn malformed_missing_partial_nonfinite_and_unresolved_fields_remain_lossless_and_stale() {
        let pack = pack();
        let mut absent = candidate(0);
        absent.target_position_present = false;
        absent.target_position_decode_valid = false;
        absent.x = None;
        absent.y = None;
        absent.z = None;
        let mut malformed = candidate(1);
        malformed.target_position_decode_valid = false;
        malformed.x = None;
        malformed.y = None;
        malformed.z = None;
        let mut partial = candidate(2);
        partial.y = None;
        let mut nonfinite = candidate(3);
        nonfinite.x = Some(f32::NAN);
        nonfinite.y = Some(f32::INFINITY);
        let mut missing_number = candidate(4);
        missing_number.raw_skill_id = None;
        missing_number.derived_marker_number = None;
        let mut missing_instance = candidate(5);
        missing_instance.passive_instance_identity = None;
        let candidates = [
            absent,
            malformed,
            partial,
            nonfinite,
            missing_number,
            missing_instance,
        ];
        let snapshot = project_marker_confirmation_snapshot(
            &pack,
            &record(&candidates),
            ProtocolDecodeStatus::Decoded,
            &mechanics(20),
            &mechanics(21),
            &binding(&pack),
        )
        .unwrap();
        assert_eq!(snapshot.events.len(), candidates.len());
        for event in &snapshot.events {
            assert_eq!(marker(event).runtime_revision, 20);
            assert!(!marker(event).asserted_authoritative_server_decode);
        }
        assert!(!marker(&snapshot.events[0]).target_position_present);
        assert!(!marker(&snapshot.events[1]).target_position_decode_valid);
        assert_eq!(marker(&snapshot.events[2]).y, None);
        assert!(marker(&snapshot.events[3]).x.unwrap().is_nan());
        assert_eq!(marker(&snapshot.events[3]).y, Some(f32::INFINITY));
        assert_eq!(marker(&snapshot.events[4]).raw_skill_id, None);
        assert_eq!(marker(&snapshot.events[4]).derived_marker_number, None);
        assert_eq!(marker(&snapshot.events[5]).passive_instance_identity, None);
        assert_eq!(snapshot.context.runtime_revision, 20);

        for owner in [None, Some(999)] {
            let mut unresolved = candidate(0);
            unresolved.owner_entity_uuid = owner;
            let snapshot = project_marker_confirmation_snapshot(
                &pack,
                &record(std::slice::from_ref(&unresolved)),
                ProtocolDecodeStatus::Decoded,
                &mechanics(20),
                &mechanics(21),
                &binding(&pack),
            )
            .unwrap();
            assert_eq!(marker(&snapshot.events[0]).marker_owner_entity_uuid, owner);
            assert_eq!(marker(&snapshot.events[0]).marker_owner_actor_id, None);
            assert!(!marker(&snapshot.events[0]).asserted_authoritative_server_decode);
        }
    }

    #[test]
    fn stale_or_pre_replacement_revision_rejects_exact_candidate_but_not_invalid_candidate() {
        let pack = pack();
        for replacement in [19, 20] {
            assert_eq!(
                project_marker_confirmation_snapshot(
                    &pack,
                    &record(&[candidate(0)]),
                    ProtocolDecodeStatus::Decoded,
                    &mechanics(20),
                    &replacement_mechanics(replacement),
                    &binding(&pack),
                ),
                Err(
                    PrivateMarkerConfirmationIngressError::PostReplacementRevisionNotStrictlyNew {
                        current: 20,
                        replacement,
                    }
                )
            );
        }

        let mut invalid = candidate(0);
        invalid.passive_instance_identity = None;
        let snapshot = project_marker_confirmation_snapshot(
            &pack,
            &record(&[invalid]),
            ProtocolDecodeStatus::Decoded,
            &mechanics(20),
            &mechanics(20),
            &binding(&pack),
        )
        .unwrap();
        assert_eq!(marker(&snapshot.events[0]).runtime_revision, 20);
    }

    #[test]
    fn binding_route_and_bounds_fail_closed() {
        let pack = pack();
        let current = mechanics(20);
        let replacement = mechanics(21);
        let mut wrong = binding(&pack);
        wrong.protocol_pack_digest.push('0');
        assert_eq!(
            project_marker_confirmation_snapshot(
                &pack,
                &record(&[]),
                ProtocolDecodeStatus::Decoded,
                &current,
                &replacement,
                &wrong,
            ),
            Err(PrivateMarkerConfirmationIngressError::UnsupportedProtocolBinding)
        );
        let mut wrong_scene_family = binding(&pack);
        wrong_scene_family.scene_family = "sea-ringed-reef".to_owned();
        assert_eq!(
            project_marker_confirmation_snapshot(
                &pack,
                &record(&[]),
                ProtocolDecodeStatus::Decoded,
                &current,
                &replacement,
                &wrong_scene_family,
            ),
            Err(PrivateMarkerConfirmationIngressError::UnsupportedProtocolBinding)
        );

        let mut wrong_route = record(&[]);
        let CaptureRecordKind::Packet(packet) = &mut wrong_route.kind else {
            unreachable!();
        };
        packet.fragment = Some(FragmentKind::Return);
        assert_eq!(
            project_marker_confirmation_snapshot(
                &pack,
                &wrong_route,
                ProtocolDecodeStatus::Decoded,
                &current,
                &replacement,
                &binding(&pack),
            ),
            Err(PrivateMarkerConfirmationIngressError::RecordRouteMismatch)
        );

        let assert_record_context_rejected = |record: &CaptureRecord| {
            assert_eq!(
                project_marker_confirmation_snapshot(
                    &pack,
                    record,
                    ProtocolDecodeStatus::Decoded,
                    &current,
                    &replacement,
                    &binding(&pack),
                ),
                Err(PrivateMarkerConfirmationIngressError::RecordRouteMismatch)
            );
        };
        let mut wrong_connection = record(&[]);
        let CaptureRecordKind::Packet(packet) = &mut wrong_connection.kind else {
            unreachable!();
        };
        packet.connection_id += 1;
        assert_record_context_rejected(&wrong_connection);

        let mut wrong_stream = record(&[]);
        let CaptureRecordKind::Packet(packet) = &mut wrong_stream.kind else {
            unreachable!();
        };
        packet.stream_id += 1;
        assert_record_context_rejected(&wrong_stream);

        let mut wrong_source = record(&[]);
        let CaptureRecordKind::Packet(packet) = &mut wrong_source.kind else {
            unreachable!();
        };
        packet.source.as_mut().unwrap().port += 1;
        assert_record_context_rejected(&wrong_source);

        let mut wrong_destination = record(&[]);
        let CaptureRecordKind::Packet(packet) = &mut wrong_destination.kind else {
            unreachable!();
        };
        packet.destination.as_mut().unwrap().address = "10.0.0.4".to_owned();
        assert_record_context_rejected(&wrong_destination);

        assert_eq!(
            project_marker_confirmation_snapshot(
                &pack,
                &record(&vec![candidate(0); MAX_MARKER_CANDIDATES + 1]),
                ProtocolDecodeStatus::Decoded,
                &current,
                &replacement,
                &binding(&pack),
            ),
            Err(
                PrivateMarkerConfirmationIngressError::CandidateLimitExceeded {
                    count: MAX_MARKER_CANDIDATES + 1,
                    limit: MAX_MARKER_CANDIDATES,
                }
            )
        );

        let mut oversized_entities = mechanics(20);
        oversized_entities.entities =
            vec![oversized_entities.entities[0].clone(); MAX_RECORD_TIME_ENTITIES + 1];
        assert_eq!(
            project_marker_confirmation_snapshot(
                &pack,
                &record(&[]),
                ProtocolDecodeStatus::Decoded,
                &oversized_entities,
                &replacement,
                &binding(&pack),
            ),
            Err(PrivateMarkerConfirmationIngressError::EntityLimitExceeded {
                count: MAX_RECORD_TIME_ENTITIES + 1,
                limit: MAX_RECORD_TIME_ENTITIES,
            })
        );
    }

    #[test]
    fn ambiguous_record_time_owner_mapping_is_never_guessed() {
        let pack = pack();
        let mut current = mechanics(20);
        current.entities.push(MechanicsMapEntity {
            actor_id: 8,
            ..current.entities[0].clone()
        });
        let mut replacement = mechanics(21);
        replacement.entities = current.entities.clone();
        let snapshot = project_marker_confirmation_snapshot(
            &pack,
            &record(&[candidate(0)]),
            ProtocolDecodeStatus::Decoded,
            &current,
            &replacement,
            &binding(&pack),
        )
        .unwrap();
        assert_eq!(marker(&snapshot.events[0]).marker_owner_actor_id, None);
        assert_eq!(marker(&snapshot.events[0]).runtime_revision, 20);
    }

    #[test]
    fn decode_status_and_record_to_post_owner_continuity_cannot_be_bypassed() {
        let pack = pack();
        let exact = candidate(0);
        let record = record(std::slice::from_ref(&exact));
        let snapshot = project_marker_confirmation_snapshot(
            &pack,
            &record,
            ProtocolDecodeStatus::CaptureGap,
            &mechanics(20),
            &replacement_mechanics(21),
            &binding(&pack),
        )
        .unwrap();
        assert!(!marker(&snapshot.events[0]).asserted_authoritative_server_decode);
        assert_eq!(marker(&snapshot.events[0]).runtime_revision, 20);

        let mut future_record_owner = mechanics(20);
        future_record_owner.entities[0].last_observed_micros = record.observed_micros + 1;
        let snapshot = project_marker_confirmation_snapshot(
            &pack,
            &record,
            ProtocolDecodeStatus::Decoded,
            &future_record_owner,
            &replacement_mechanics(21),
            &binding(&pack),
        )
        .unwrap();
        assert_eq!(marker(&snapshot.events[0]).marker_owner_actor_id, None);
        assert!(!marker(&snapshot.events[0]).asserted_authoritative_server_decode);

        let mut rebound = replacement_mechanics(21);
        rebound.entities[0].actor_id = 8;
        rebound.markers[0].related_actor_id = Some(7);
        let snapshot = project_marker_confirmation_snapshot(
            &pack,
            &record,
            ProtocolDecodeStatus::Decoded,
            &mechanics(20),
            &rebound,
            &binding(&pack),
        )
        .unwrap();
        assert_eq!(marker(&snapshot.events[0]).marker_owner_actor_id, Some(7));
        assert!(!marker(&snapshot.events[0]).asserted_authoritative_server_decode);
        assert_eq!(marker(&snapshot.events[0]).runtime_revision, 20);
    }
}
