//! Pure, process-private projection from lossless method-46 starts into the
//! Automarker confirmation router's parser snapshot.
//!
//! This module owns no capture/native handle and is intentionally not wired
//! into live processing yet. It retains malformed candidates so the existing
//! router/coordinator boundary can reject them without making absence look
//! valid.

#![allow(dead_code)]

use std::net::Ipv4Addr;

use rlogs_game_bpsr::{
    AUTOMARKER_REQUEST_BUILD, AUTOMARKER_REQUEST_PACK_DIGEST,
    BPSR_COMPATIBILITY_EPOCH_DEPLOYMENT_ID, CaptureRecord, CaptureRecordKind,
    DecodedLocalMarkerStart, DecoderKind, FragmentKind, PacketDirection, ProtocolPack,
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

/// Preserve every decoded start as one private marker event. Structurally
/// exact candidates receive the strictly newer post-replacement mechanics
/// revision. Every invalid candidate keeps the unchanged record-time revision
/// so it cannot impersonate fresh authoritative replacement evidence.
pub(crate) fn project_marker_confirmation_snapshot(
    pack: &ProtocolPack,
    record: &CaptureRecord,
    candidates: &[DecodedLocalMarkerStart],
    record_mechanics: &MechanicsMapSnapshot,
    post_replacement_mechanics: &MechanicsMapSnapshot,
    binding: &PrivateMarkerConfirmationBinding,
) -> Result<PrivateParserConfirmationSnapshot, PrivateMarkerConfirmationIngressError> {
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
    if candidates.len() > MAX_MARKER_CANDIDATES {
        return Err(
            PrivateMarkerConfirmationIngressError::CandidateLimitExceeded {
                count: candidates.len(),
                limit: MAX_MARKER_CANDIDATES,
            },
        );
    }
    if record_mechanics.entities.len() > MAX_RECORD_TIME_ENTITIES {
        return Err(PrivateMarkerConfirmationIngressError::EntityLimitExceeded {
            count: record_mechanics.entities.len(),
            limit: MAX_RECORD_TIME_ENTITIES,
        });
    }
    validate_mechanics_binding(record_mechanics, binding)?;
    validate_mechanics_binding(post_replacement_mechanics, binding)?;

    let projections = candidates
        .iter()
        .map(|candidate| {
            let owner_actor_id = candidate
                .owner_entity_uuid
                .and_then(|owner| unique_record_time_actor_id(record_mechanics, owner));
            let exact = owner_actor_id.is_some_and(|owner_actor_id| {
                exact_marker_candidate(candidate, Some(owner_actor_id))
                    && marker_replacement_proven(
                        candidate,
                        owner_actor_id,
                        record.observed_micros,
                        record_mechanics,
                        post_replacement_mechanics,
                    )
            });
            (candidate, owner_actor_id, exact)
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
        capture_sequence: record.sequence,
        record_event_index,
        connection_id: packet.connection_id,
        stream_id: packet.stream_id,
        direction: PrivatePacketDirection::ServerToClient,
        fragment: PrivateFragmentKind::Notify,
        route_resolved: true,
        service_id: route.service_id,
        method_id: route.method_id,
        stub_id: routed.stub_id,
        call_id: routed.call_id,
    };
    let source_clocks = PrivateSourceClocks {
        observed_micros: record.observed_micros,
        wall_clock_unix_micros: record.wall_clock_unix_micros,
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

fn unique_record_time_actor_id(mechanics: &MechanicsMapSnapshot, owner: i64) -> Option<i64> {
    let mut matching = mechanics
        .entities
        .iter()
        .filter(|entity| entity.entity_uuid == owner);
    let entity = matching.next()?;
    if matching.next().is_some() {
        return None;
    }
    i64::try_from(entity.actor_id)
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
    record_mechanics: &MechanicsMapSnapshot,
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
    !record_mechanics.markers.iter().any(|marker| {
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
    marker.marker_number == Some(marker_number)
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rlogs_game_bpsr::{
        CompressionState, LiveProtocolPackKind, LiveProtocolPackSelection, NetworkEndpoint,
        PacketEnvelope, PacketPayload, RouteKey, RoutedMessage,
    };

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

    fn record() -> CaptureRecord {
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
                    application_bytes: Some(Vec::new()),
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
            marker_id: None,
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
    fn valid_duplicates_are_preserved_and_use_strictly_post_replacement_revision() {
        let pack = pack();
        let snapshot = project_marker_confirmation_snapshot(
            &pack,
            &record(),
            &[candidate(0), candidate(1)],
            &mechanics(20),
            &replacement_mechanics(21),
            &binding(&pack),
        )
        .unwrap();
        assert_eq!(snapshot.events.len(), 2);
        assert_eq!(marker(&snapshot.events[0]).provenance.record_event_index, 0);
        assert_eq!(marker(&snapshot.events[1]).provenance.record_event_index, 1);
        assert_eq!(marker(&snapshot.events[0]).marker_owner_actor_id, Some(7));
        assert_eq!(marker(&snapshot.events[0]).runtime_revision, 21);
        assert!(marker(&snapshot.events[0]).asserted_authoritative_server_decode);
        assert_eq!(snapshot.context.runtime_revision, 21);
    }

    #[test]
    fn mixed_batch_never_lends_replacement_authority_to_an_invalid_candidate() {
        let pack = pack();
        let mut invalid = candidate(1);
        invalid.z = Some(f32::NAN);
        let snapshot = project_marker_confirmation_snapshot(
            &pack,
            &record(),
            &[candidate(0), invalid],
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
        let project = |current: &MechanicsMapSnapshot, replacement: &MechanicsMapSnapshot| {
            project_marker_confirmation_snapshot(
                &pack,
                &record(),
                std::slice::from_ref(&exact),
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
        let mut missing_owner = candidate(5);
        missing_owner.owner_entity_uuid = None;
        let mut missing_instance = candidate(6);
        missing_instance.passive_instance_identity = None;
        let mut unresolved = candidate(7);
        unresolved.owner_entity_uuid = Some(999);
        let candidates = [
            absent,
            malformed,
            partial,
            nonfinite,
            missing_number,
            missing_owner,
            missing_instance,
            unresolved,
        ];
        let snapshot = project_marker_confirmation_snapshot(
            &pack,
            &record(),
            &candidates,
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
        assert_eq!(marker(&snapshot.events[5]).marker_owner_entity_uuid, None);
        assert_eq!(marker(&snapshot.events[5]).marker_owner_actor_id, None);
        assert_eq!(marker(&snapshot.events[6]).passive_instance_identity, None);
        assert_eq!(
            marker(&snapshot.events[7]).marker_owner_entity_uuid,
            Some(999)
        );
        assert_eq!(marker(&snapshot.events[7]).marker_owner_actor_id, None);
        assert_eq!(snapshot.context.runtime_revision, 20);
    }

    #[test]
    fn stale_or_pre_replacement_revision_rejects_exact_candidate_but_not_invalid_candidate() {
        let pack = pack();
        for replacement in [19, 20] {
            assert_eq!(
                project_marker_confirmation_snapshot(
                    &pack,
                    &record(),
                    &[candidate(0)],
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
            &record(),
            &[invalid],
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
                &record(),
                &[],
                &current,
                &replacement,
                &wrong,
            ),
            Err(PrivateMarkerConfirmationIngressError::UnsupportedProtocolBinding)
        );

        let mut wrong_route = record();
        let CaptureRecordKind::Packet(packet) = &mut wrong_route.kind else {
            unreachable!();
        };
        packet.fragment = Some(FragmentKind::Return);
        assert_eq!(
            project_marker_confirmation_snapshot(
                &pack,
                &wrong_route,
                &[],
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
                    &[],
                    &current,
                    &replacement,
                    &binding(&pack),
                ),
                Err(PrivateMarkerConfirmationIngressError::RecordRouteMismatch)
            );
        };
        let mut wrong_connection = record();
        let CaptureRecordKind::Packet(packet) = &mut wrong_connection.kind else {
            unreachable!();
        };
        packet.connection_id += 1;
        assert_record_context_rejected(&wrong_connection);

        let mut wrong_stream = record();
        let CaptureRecordKind::Packet(packet) = &mut wrong_stream.kind else {
            unreachable!();
        };
        packet.stream_id += 1;
        assert_record_context_rejected(&wrong_stream);

        let mut wrong_source = record();
        let CaptureRecordKind::Packet(packet) = &mut wrong_source.kind else {
            unreachable!();
        };
        packet.source.as_mut().unwrap().port += 1;
        assert_record_context_rejected(&wrong_source);

        let mut wrong_destination = record();
        let CaptureRecordKind::Packet(packet) = &mut wrong_destination.kind else {
            unreachable!();
        };
        packet.destination.as_mut().unwrap().address = "10.0.0.4".to_owned();
        assert_record_context_rejected(&wrong_destination);

        assert_eq!(
            project_marker_confirmation_snapshot(
                &pack,
                &record(),
                &vec![candidate(0); MAX_MARKER_CANDIDATES + 1],
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
                &record(),
                &[],
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
            &record(),
            &[candidate(0)],
            &current,
            &replacement,
            &binding(&pack),
        )
        .unwrap();
        assert_eq!(marker(&snapshot.events[0]).marker_owner_actor_id, None);
        assert_eq!(marker(&snapshot.events[0]).runtime_revision, 20);
    }
}
