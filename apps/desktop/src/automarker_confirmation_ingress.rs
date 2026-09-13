//! Pure, bounded extraction of private Automarker confirmation candidates.
//!
//! This module borrows capture bytes only long enough to inspect one Return
//! header. It owns no capture callback, coordinator, driver, socket, or
//! serialization surface.

#![allow(dead_code)]

use rlogs_game_bpsr::{
    CaptureRecord, CaptureRecordKind, CompressionState, DecoderKind, FragmentKind, PacketDirection,
    ProtocolDecodeStatus, ProtocolPack, RouteKey,
};

use crate::{
    automarker_bridge_evidence::AutomarkerBridgeOutboundCarrierEvidence,
    automarker_confirmation_router::{
        PrivateConfirmationProvenance, PrivateCorrelatedReturn, PrivateFragmentKind,
        PrivatePacketDirection, PrivateParserConfirmationEvent, PrivateSourceClocks,
    },
};

const WORLD_SERVICE_ID: u64 = 103_198_054;
const WORLD_USE_SLOT_METHOD_ID: u32 = 249_858;
const RETURN_FRAGMENT_TAG: u16 = 3;
const RETURN_HEADER_BYTES: usize = 18;
const MAX_RETURN_FRAME_BYTES: usize = 65_536;
const RETURN_CORRELATION_MAX_MICROS: u64 = 2_000_000;

/// Exact private identity of the currently active capture/pack ingress.
/// This is deliberately not serializable and must be supplied independently
/// of retained carrier evidence so a stale carrier cannot validate itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrivateConfirmationIngressIdentity {
    pub capture_session_id: String,
    pub deployment_id: String,
    pub client_build: String,
    pub protocol_pack_digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BorrowedReturnHeader {
    raw_stub_id: u32,
    raw_call_id: u32,
    raw_status: u32,
    raw_body_length: usize,
}

/// Extract one correlated Return candidate without copying or retaining raw
/// bytes. `None` means the record is malformed, unrelated, stale, or outside
/// the exact retained carrier context.
pub(crate) fn extract_private_return_candidate(
    identity: &PrivateConfirmationIngressIdentity,
    pack: &ProtocolPack,
    record: &CaptureRecord,
    decode_status: ProtocolDecodeStatus,
    carrier: &AutomarkerBridgeOutboundCarrierEvidence,
) -> Option<PrivateParserConfirmationEvent> {
    if !carrier_matches_ingress(identity, pack, carrier) {
        return None;
    }
    let CaptureRecordKind::Packet(packet) = &record.kind else {
        return None;
    };
    if packet.direction != PacketDirection::ServerToClient
        || packet.fragment != Some(FragmentKind::Return)
        || packet.compression != CompressionState::NotCompressed
        || packet.connection_id == 0
        || packet.stream_id == 0
        || packet.stream_id != carrier.provenance.stream_id
        || !carrier.tcp_connection.matches_reverse_packet(packet)
        || record.sequence <= carrier.provenance.capture_sequence
        || record.observed_micros <= carrier.provenance.observed_micros
        || record
            .observed_micros
            .checked_sub(carrier.provenance.observed_micros)?
            > RETURN_CORRELATION_MAX_MICROS
    {
        return None;
    }

    let header = parse_return_header(&packet.payload.wire_bytes)?;
    let decoded_body = packet.payload.application_bytes.as_deref();
    if decoded_body.is_some_and(|body| body.len() != header.raw_body_length) {
        return None;
    }

    let resolved_route = packet.route.filter(|routed| {
        routed.key.direction == packet.direction
            && routed.key.fragment == FragmentKind::Return
            && routed.stub_id == header.raw_stub_id
            && routed.call_id == Some(header.raw_call_id)
            && decode_status == ProtocolDecodeStatus::Decoded
            && pack
                .decoder(&RouteKey::new(
                    PacketDirection::ClientToServer,
                    FragmentKind::Call,
                    routed.key.service_id,
                    routed.key.method_id,
                ))
                .is_some()
    });
    let route_resolved = resolved_route.is_some();
    let target_route = resolved_route.is_some_and(|routed| {
        routed.key.service_id == WORLD_SERVICE_ID
            && routed.key.method_id == WORLD_USE_SLOT_METHOD_ID
            && pack.decoder(&RouteKey::new(
                PacketDirection::ClientToServer,
                FragmentKind::Call,
                routed.key.service_id,
                routed.key.method_id,
            )) == Some(DecoderKind::WorldUseSlotV1)
    });
    let carrier_call_id = carrier.provenance.call_id.filter(|call_id| *call_id != 0)?;
    let raw_call_matches_carrier = header.raw_call_id == carrier_call_id;
    if !raw_call_matches_carrier && !target_route {
        return None;
    }

    let (service_id, method_id, stub_id) = resolved_route
        .map(|routed| (routed.key.service_id, routed.key.method_id, routed.stub_id))
        .unwrap_or((0, 0, 0));
    let asserted_authoritative_server_decode = target_route
        && raw_call_matches_carrier
        && decode_status == ProtocolDecodeStatus::Decoded
        && decoded_body.is_some();

    Some(PrivateParserConfirmationEvent::CorrelatedReturn(
        PrivateCorrelatedReturn {
            provenance: PrivateConfirmationProvenance {
                capture_sequence: record.sequence,
                record_event_index: 0,
                connection_id: packet.connection_id,
                stream_id: packet.stream_id,
                direction: PrivatePacketDirection::ServerToClient,
                fragment: PrivateFragmentKind::Return,
                route_resolved,
                service_id,
                method_id,
                stub_id,
                call_id: Some(header.raw_call_id),
            },
            source_clocks: PrivateSourceClocks {
                observed_micros: record.observed_micros,
                wall_clock_unix_micros: record.wall_clock_unix_micros,
            },
            carrier_capture_sequence: carrier.provenance.capture_sequence,
            raw_stub_id: header.raw_stub_id,
            raw_status: header.raw_status,
            asserted_authoritative_server_decode,
            decoded_as_success: header.raw_status == 0,
            decoded_body_present: decoded_body.is_some(),
            decoded_body_length: decoded_body.map_or(0, <[u8]>::len),
        },
    ))
}

fn carrier_matches_ingress(
    identity: &PrivateConfirmationIngressIdentity,
    pack: &ProtocolPack,
    carrier: &AutomarkerBridgeOutboundCarrierEvidence,
) -> bool {
    !identity.capture_session_id.is_empty()
        && identity.capture_session_id == carrier.capture_session_id
        && identity.deployment_id == carrier.deployment_id
        && identity.client_build == carrier.client_build
        && identity.protocol_pack_digest == carrier.protocol_pack_digest
        && identity.deployment_id == pack.definition().target.deployment_id
        && identity.client_build == pack.definition().target.build_id
        && identity.protocol_pack_digest == pack.digest()
        && carrier.provenance.direction == PacketDirection::ClientToServer
        && carrier.provenance.fragment == FragmentKind::Call
        && carrier.provenance.service_id == WORLD_SERVICE_ID
        && carrier.provenance.method_id == WORLD_USE_SLOT_METHOD_ID
        && carrier.provenance.decoder == DecoderKind::WorldUseSlotV1
        && carrier.provenance.connection_id == carrier.tcp_connection.capture_connection_id
        && carrier.provenance.stream_id != 0
}

fn parse_return_header(wire: &[u8]) -> Option<BorrowedReturnHeader> {
    if !(RETURN_HEADER_BYTES..=MAX_RETURN_FRAME_BYTES).contains(&wire.len()) {
        return None;
    }
    let declared_length = u32::from_be_bytes(wire.get(0..4)?.try_into().ok()?) as usize;
    let tag = u16::from_be_bytes(wire.get(4..6)?.try_into().ok()?);
    if declared_length != wire.len() || tag != RETURN_FRAGMENT_TAG {
        return None;
    }
    Some(BorrowedReturnHeader {
        raw_stub_id: u32::from_be_bytes(wire.get(6..10)?.try_into().ok()?),
        raw_call_id: u32::from_be_bytes(wire.get(10..14)?.try_into().ok()?),
        raw_status: u32::from_be_bytes(wire.get(14..18)?.try_into().ok()?),
        raw_body_length: wire.len() - RETURN_HEADER_BYTES,
    })
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use rlogs_game_bpsr::{
        CaptureRecordKind, CompressionState, NetworkEndpoint, PacketEnvelope, PacketPayload,
        RoutedMessage,
    };

    use super::*;
    use crate::{
        automarker_bridge_evidence::{
            AutomarkerBridgeCaptureTcpConnection, AutomarkerBridgeRecordProvenance,
        },
        automarker_confirmation_router::PrivateParserConfirmationEvent,
    };

    const CALL_ID: u32 = 77;

    fn pack() -> ProtocolPack {
        ProtocolPack::from_json(include_bytes!(
            "../../../plugins/games/blue-protocol-star-resonance/protocol-packs/global/steam-24687926/pack.json"
        ))
        .unwrap()
    }

    fn identity(pack: &ProtocolPack) -> PrivateConfirmationIngressIdentity {
        PrivateConfirmationIngressIdentity {
            capture_session_id: "capture-a".into(),
            deployment_id: pack.definition().target.deployment_id.clone(),
            client_build: pack.definition().target.build_id.clone(),
            protocol_pack_digest: pack.digest().into(),
        }
    }

    fn carrier(pack: &ProtocolPack) -> AutomarkerBridgeOutboundCarrierEvidence {
        AutomarkerBridgeOutboundCarrierEvidence {
            capture_session_id: "capture-a".into(),
            deployment_id: pack.definition().target.deployment_id.clone(),
            client_build: pack.definition().target.build_id.clone(),
            protocol_pack_digest: pack.digest().into(),
            scene_id: 1,
            map_id: 2,
            activity_family_id: "mech-facility".into(),
            local_actor_id: 44,
            mechanics_runtime_revision: 9,
            marker_number: 1,
            session_sequence: 123,
            tcp_connection: AutomarkerBridgeCaptureTcpConnection {
                capture_connection_id: 50,
                client_address: Ipv4Addr::new(10, 0, 0, 2),
                client_port: 50_000,
                server_address: Ipv4Addr::new(10, 0, 0, 3),
                server_port: 443,
            },
            application_bytes: vec![9; 161],
            provenance: AutomarkerBridgeRecordProvenance {
                capture_sequence: 30,
                observed_micros: 1_000,
                wall_clock_unix_micros: Some(2_000),
                connection_id: 50,
                stream_id: 60,
                direction: PacketDirection::ClientToServer,
                fragment: FragmentKind::Call,
                service_id: WORLD_SERVICE_ID,
                method_id: WORLD_USE_SLOT_METHOD_ID,
                stub_id: 1,
                call_id: Some(CALL_ID),
                decoder: DecoderKind::WorldUseSlotV1,
            },
        }
    }

    fn record(call_id: u32, status: u32, body: Option<&[u8]>) -> CaptureRecord {
        let raw_body = body.unwrap_or(&[]);
        let mut wire = Vec::with_capacity(RETURN_HEADER_BYTES + raw_body.len());
        wire.extend_from_slice(&((RETURN_HEADER_BYTES + raw_body.len()) as u32).to_be_bytes());
        wire.extend_from_slice(&RETURN_FRAGMENT_TAG.to_be_bytes());
        wire.extend_from_slice(&1_u32.to_be_bytes());
        wire.extend_from_slice(&call_id.to_be_bytes());
        wire.extend_from_slice(&status.to_be_bytes());
        wire.extend_from_slice(raw_body);
        CaptureRecord {
            sequence: 31,
            observed_micros: 1_001,
            wall_clock_unix_micros: Some(2_001),
            kind: CaptureRecordKind::Packet(PacketEnvelope {
                connection_id: 50,
                stream_id: 60,
                source: Some(NetworkEndpoint {
                    address: "10.0.0.3".into(),
                    port: 443,
                }),
                destination: Some(NetworkEndpoint {
                    address: "10.0.0.2".into(),
                    port: 50_000,
                }),
                direction: PacketDirection::ServerToClient,
                fragment: Some(FragmentKind::Return),
                route: Some(RoutedMessage {
                    key: RouteKey::new(
                        PacketDirection::ServerToClient,
                        FragmentKind::Return,
                        WORLD_SERVICE_ID,
                        WORLD_USE_SLOT_METHOD_ID,
                    ),
                    stub_id: 1,
                    call_id: Some(call_id),
                }),
                compression: CompressionState::NotCompressed,
                payload: PacketPayload {
                    wire_bytes: wire,
                    application_bytes: body.map(<[u8]>::to_vec),
                },
            }),
        }
    }

    fn extracted(
        identity: &PrivateConfirmationIngressIdentity,
        pack: &ProtocolPack,
        record: &CaptureRecord,
        status: ProtocolDecodeStatus,
        carrier: &AutomarkerBridgeOutboundCarrierEvidence,
    ) -> Option<PrivateCorrelatedReturn> {
        match extract_private_return_candidate(identity, pack, record, status, carrier)? {
            PrivateParserConfirmationEvent::CorrelatedReturn(event) => Some(event),
            PrivateParserConfirmationEvent::MarkerAdd(_) => None,
        }
    }

    #[test]
    fn exact_good_failure_nonempty_and_missing_body_are_preserved() {
        let pack = pack();
        let identity = identity(&pack);
        let carrier = carrier(&pack);

        let good = extracted(
            &identity,
            &pack,
            &record(CALL_ID, 0, Some(&[])),
            ProtocolDecodeStatus::Decoded,
            &carrier,
        )
        .unwrap();
        assert!(good.asserted_authoritative_server_decode);
        assert!(good.decoded_as_success);
        assert!(good.decoded_body_present);
        assert_eq!(good.decoded_body_length, 0);

        let failure = extracted(
            &identity,
            &pack,
            &record(CALL_ID, 9, Some(&[])),
            ProtocolDecodeStatus::Decoded,
            &carrier,
        )
        .unwrap();
        assert!(failure.asserted_authoritative_server_decode);
        assert!(!failure.decoded_as_success);
        assert_eq!(failure.raw_status, 9);

        let nonempty = extracted(
            &identity,
            &pack,
            &record(CALL_ID, 0, Some(&[1, 2, 3])),
            ProtocolDecodeStatus::Decoded,
            &carrier,
        )
        .unwrap();
        assert!(nonempty.asserted_authoritative_server_decode);
        assert_eq!(nonempty.decoded_body_length, 3);

        let missing = extracted(
            &identity,
            &pack,
            &record(CALL_ID, 0, None),
            ProtocolDecodeStatus::MissingApplicationPayload,
            &carrier,
        )
        .unwrap();
        assert!(!missing.provenance.route_resolved);
        assert!(!missing.asserted_authoritative_server_decode);
        assert!(!missing.decoded_body_present);
    }

    #[test]
    fn unresolved_and_wrong_routes_forward_only_when_raw_call_correlates() {
        let pack = pack();
        let identity = identity(&pack);
        let carrier = carrier(&pack);
        let mut unresolved = record(CALL_ID, 0, Some(&[]));
        let CaptureRecordKind::Packet(packet) = &mut unresolved.kind else {
            unreachable!();
        };
        packet.route = None;
        let event = extracted(
            &identity,
            &pack,
            &unresolved,
            ProtocolDecodeStatus::Unrouted,
            &carrier,
        )
        .unwrap();
        assert!(!event.provenance.route_resolved);
        assert_eq!(
            (event.provenance.service_id, event.provenance.method_id),
            (0, 0)
        );

        for (service_id, method_id) in [
            (7, WORLD_USE_SLOT_METHOD_ID),
            (WORLD_SERVICE_ID, WORLD_USE_SLOT_METHOD_ID + 1),
        ] {
            let mut wrong = record(CALL_ID, 0, Some(&[]));
            let CaptureRecordKind::Packet(packet) = &mut wrong.kind else {
                unreachable!();
            };
            let routed = packet.route.as_mut().unwrap();
            routed.key.service_id = service_id;
            routed.key.method_id = method_id;
            let event = extracted(
                &identity,
                &pack,
                &wrong,
                ProtocolDecodeStatus::Decoded,
                &carrier,
            )
            .unwrap();
            assert!(!event.provenance.route_resolved);
            assert!(!event.asserted_authoritative_server_decode);
        }

        let unrelated = record(CALL_ID + 1, 0, Some(&[]));
        let mut unrelated_unrouted = unrelated.clone();
        let CaptureRecordKind::Packet(packet) = &mut unrelated_unrouted.kind else {
            unreachable!();
        };
        packet.route = None;
        assert!(
            extracted(
                &identity,
                &pack,
                &unrelated_unrouted,
                ProtocolDecodeStatus::Unrouted,
                &carrier
            )
            .is_none()
        );
    }

    #[test]
    fn target_route_with_wrong_call_is_forwarded_but_not_authoritative() {
        let pack = pack();
        let identity = identity(&pack);
        let carrier = carrier(&pack);
        let event = extracted(
            &identity,
            &pack,
            &record(CALL_ID + 1, 0, Some(&[])),
            ProtocolDecodeStatus::Decoded,
            &carrier,
        )
        .unwrap();
        assert!(event.provenance.route_resolved);
        assert_eq!(event.provenance.call_id, Some(CALL_ID + 1));
        assert!(!event.asserted_authoritative_server_decode);
    }

    #[test]
    fn malformed_header_and_envelope_or_route_inconsistency_fail_closed() {
        let pack = pack();
        let identity = identity(&pack);
        let carrier = carrier(&pack);
        assert!(parse_return_header(&[0; RETURN_HEADER_BYTES - 1]).is_none());
        assert!(parse_return_header(&vec![0; MAX_RETURN_FRAME_BYTES + 1]).is_none());
        let mut malformed = record(CALL_ID, 0, Some(&[]));
        let CaptureRecordKind::Packet(packet) = &mut malformed.kind else {
            unreachable!();
        };
        packet.payload.wire_bytes[0..4].copy_from_slice(&99_u32.to_be_bytes());
        assert!(
            extracted(
                &identity,
                &pack,
                &malformed,
                ProtocolDecodeStatus::Decoded,
                &carrier
            )
            .is_none()
        );

        let mut wrong_tag = record(CALL_ID, 0, Some(&[]));
        let CaptureRecordKind::Packet(packet) = &mut wrong_tag.kind else {
            unreachable!();
        };
        packet.payload.wire_bytes[4..6].copy_from_slice(&2_u16.to_be_bytes());
        assert!(
            extracted(
                &identity,
                &pack,
                &wrong_tag,
                ProtocolDecodeStatus::Decoded,
                &carrier
            )
            .is_none()
        );

        let mut compression_mismatch = record(CALL_ID, 0, Some(&[]));
        let CaptureRecordKind::Packet(packet) = &mut compression_mismatch.kind else {
            unreachable!();
        };
        packet.compression = CompressionState::ZstdDecoded;
        assert!(
            extracted(
                &identity,
                &pack,
                &compression_mismatch,
                ProtocolDecodeStatus::Decoded,
                &carrier
            )
            .is_none()
        );

        let mut wrong_envelope = record(CALL_ID, 0, Some(&[]));
        let CaptureRecordKind::Packet(packet) = &mut wrong_envelope.kind else {
            unreachable!();
        };
        packet.direction = PacketDirection::ClientToServer;
        assert!(
            extracted(
                &identity,
                &pack,
                &wrong_envelope,
                ProtocolDecodeStatus::Decoded,
                &carrier
            )
            .is_none()
        );

        let mut route_mismatch = record(CALL_ID, 0, Some(&[]));
        let CaptureRecordKind::Packet(packet) = &mut route_mismatch.kind else {
            unreachable!();
        };
        packet.route.as_mut().unwrap().call_id = Some(CALL_ID + 1);
        let event = extracted(
            &identity,
            &pack,
            &route_mismatch,
            ProtocolDecodeStatus::Decoded,
            &carrier,
        )
        .unwrap();
        assert!(!event.provenance.route_resolved);
        assert!(!event.asserted_authoritative_server_decode);
    }

    #[test]
    fn reverse_context_window_and_all_identity_mismatches_are_rejected() {
        let pack = pack();
        let identity = identity(&pack);
        let carrier = carrier(&pack);
        let base = record(CALL_ID, 0, Some(&[]));

        for mutate in [
            |record: &mut CaptureRecord| record.sequence = 30,
            |record: &mut CaptureRecord| record.observed_micros = 1_000,
            |record: &mut CaptureRecord| record.observed_micros = 2_001_001,
            |record: &mut CaptureRecord| {
                let CaptureRecordKind::Packet(packet) = &mut record.kind else {
                    return;
                };
                packet.connection_id = 51;
            },
            |record: &mut CaptureRecord| {
                let CaptureRecordKind::Packet(packet) = &mut record.kind else {
                    return;
                };
                packet.stream_id = 61;
            },
            |record: &mut CaptureRecord| {
                let CaptureRecordKind::Packet(packet) = &mut record.kind else {
                    return;
                };
                packet.source.as_mut().unwrap().port = 444;
            },
        ] {
            let mut candidate = base.clone();
            mutate(&mut candidate);
            assert!(
                extracted(
                    &identity,
                    &pack,
                    &candidate,
                    ProtocolDecodeStatus::Decoded,
                    &carrier
                )
                .is_none()
            );
        }

        for mutate in [
            |identity: &mut PrivateConfirmationIngressIdentity| {
                identity.capture_session_id = "capture-b".into()
            },
            |identity: &mut PrivateConfirmationIngressIdentity| {
                identity.deployment_id = "cn".into()
            },
            |identity: &mut PrivateConfirmationIngressIdentity| {
                identity.client_build = "wrong".into()
            },
            |identity: &mut PrivateConfirmationIngressIdentity| {
                identity.protocol_pack_digest = "sha256:wrong".into()
            },
        ] {
            let mut candidate_identity = identity.clone();
            mutate(&mut candidate_identity);
            assert!(
                extracted(
                    &candidate_identity,
                    &pack,
                    &base,
                    ProtocolDecodeStatus::Decoded,
                    &carrier
                )
                .is_none()
            );
        }

        for mutate in [
            |carrier: &mut AutomarkerBridgeOutboundCarrierEvidence| {
                carrier.capture_session_id = "capture-b".into()
            },
            |carrier: &mut AutomarkerBridgeOutboundCarrierEvidence| {
                carrier.client_build = "wrong".into()
            },
            |carrier: &mut AutomarkerBridgeOutboundCarrierEvidence| {
                carrier.protocol_pack_digest = "sha256:wrong".into()
            },
        ] {
            let mut stale_carrier = carrier.clone();
            mutate(&mut stale_carrier);
            assert!(
                extracted(
                    &identity,
                    &pack,
                    &base,
                    ProtocolDecodeStatus::Decoded,
                    &stale_carrier
                )
                .is_none()
            );
        }
    }

    #[test]
    fn extracted_event_retains_lengths_not_raw_bytes() {
        let pack = pack();
        let identity = identity(&pack);
        let carrier = carrier(&pack);
        let record = record(CALL_ID, 0, Some(b"unique-private-return-body"));
        let event = extracted(
            &identity,
            &pack,
            &record,
            ProtocolDecodeStatus::Decoded,
            &carrier,
        )
        .unwrap();
        drop(record);
        assert_eq!(event.decoded_body_length, 26);
        assert!(!format!("{event:?}").contains("unique-private-return-body"));
    }
}
