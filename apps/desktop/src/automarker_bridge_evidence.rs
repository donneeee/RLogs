//! Process-private, fail-closed evidence retained for a future native marker driver.
//!
//! This projection is deliberately not serializable and is not returned by any
//! HTTP/UI surface.  The public observed-marker feed remains the privacy boundary.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Mutex,
};

use rlogs_game_bpsr::{
    CaptureRecord, CaptureRecordKind, CompressionState, DecoderKind, FragmentKind, LocalMapMarker,
    ObservedAutomarkerRequest, PacketDirection, ProtocolPack,
    SINGLE_MARKER_XYZ_MAX_CARRIER_AGE_MILLIS,
};

use crate::{automarker_presets::AutomarkerSceneContext, mechanics_map::MechanicsMapSnapshot};

const WORLD_NOTIFICATION_SERVICE_ID: u64 = 1_664_308_034;
const WORLD_SERVICE_ID: u64 = 103_198_054;
const WORLD_USE_SLOT_METHOD_ID: u32 = 249_858;
const EXACT_CARRIER_APPLICATION_BYTES: usize = 161;
const EXACT_EMPTY_RETURN_FRAME_BYTES: usize = 18;
const RETURN_CORRELATION_MAX_MICROS: u64 = 2_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AutomarkerBridgeSessionIdentity {
    pub capture_session_id: String,
    pub deployment_id: String,
    pub client_build: String,
    pub protocol_pack_digest: String,
    pub protocol_supported: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AutomarkerBridgeRecordProvenance {
    pub capture_sequence: u64,
    pub observed_micros: u64,
    pub wall_clock_unix_micros: Option<i64>,
    pub connection_id: u64,
    pub stream_id: u64,
    pub direction: PacketDirection,
    pub fragment: FragmentKind,
    pub service_id: u64,
    pub method_id: u32,
    pub stub_id: u32,
    pub call_id: Option<u32>,
    pub decoder: DecoderKind,
}

impl AutomarkerBridgeRecordProvenance {
    /// Build provenance only from the same exact record already accepted by
    /// `LocalMapMarkerProjection`; callers cannot synthesize a route label.
    pub(crate) fn from_decoded_marker_record(
        pack: &ProtocolPack,
        record: &CaptureRecord,
    ) -> Option<Self> {
        let CaptureRecordKind::Packet(packet) = &record.kind else {
            return None;
        };
        let routed = packet.route?;
        let key = routed.key;
        if key.direction != PacketDirection::ServerToClient
            || key.fragment != FragmentKind::Notify
            || key.service_id != WORLD_NOTIFICATION_SERVICE_ID
            || !matches!(key.method_id, 3 | 4 | 6 | 45 | 46)
        {
            return None;
        }
        let decoder = pack.decoder(&key)?;
        let expected = match key.method_id {
            3 => DecoderKind::EnterSceneV1,
            4 => DecoderKind::NotifyLoadSceneEndV1,
            6 => DecoderKind::SyncNearEntitiesV1,
            45 => DecoderKind::SyncNearDeltaV1,
            46 => DecoderKind::SyncToMeDeltaV1,
            _ => unreachable!(),
        };
        if decoder != expected {
            return None;
        }
        Some(Self {
            capture_sequence: record.sequence,
            observed_micros: record.observed_micros,
            wall_clock_unix_micros: record.wall_clock_unix_micros,
            connection_id: packet.connection_id,
            stream_id: packet.stream_id,
            direction: key.direction,
            fragment: key.fragment,
            service_id: key.service_id,
            method_id: key.method_id,
            stub_id: routed.stub_id,
            call_id: routed.call_id,
            decoder,
        })
    }

    fn from_world_use_slot_record(
        pack: &ProtocolPack,
        record: &CaptureRecord,
        direction: PacketDirection,
        fragment: FragmentKind,
    ) -> Option<Self> {
        let CaptureRecordKind::Packet(packet) = &record.kind else {
            return None;
        };
        let routed = packet.route?;
        let key = routed.key;
        let decoder_key = if fragment == FragmentKind::Return {
            rlogs_game_bpsr::RouteKey::new(
                PacketDirection::ClientToServer,
                FragmentKind::Call,
                key.service_id,
                key.method_id,
            )
        } else {
            key
        };
        if key.direction != direction
            || key.fragment != fragment
            || key.service_id != WORLD_SERVICE_ID
            || key.method_id != WORLD_USE_SLOT_METHOD_ID
            || pack.decoder(&decoder_key) != Some(DecoderKind::WorldUseSlotV1)
            || routed.call_id == Some(0)
            || routed.call_id.is_none()
            || packet.connection_id == 0
            || packet.stream_id == 0
        {
            return None;
        }
        Some(Self {
            capture_sequence: record.sequence,
            observed_micros: record.observed_micros,
            wall_clock_unix_micros: record.wall_clock_unix_micros,
            connection_id: packet.connection_id,
            stream_id: packet.stream_id,
            direction: key.direction,
            fragment: key.fragment,
            service_id: key.service_id,
            method_id: key.method_id,
            stub_id: routed.stub_id,
            call_id: routed.call_id,
            decoder: DecoderKind::WorldUseSlotV1,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AutomarkerBridgeMarkerEvidence {
    pub capture_session_id: String,
    pub deployment_id: String,
    pub client_build: String,
    pub protocol_pack_digest: String,
    pub scene_id: i32,
    pub map_id: u32,
    pub activity_family_id: String,
    pub local_actor_id: u64,
    pub feed_revision: u64,
    pub mechanics_runtime_revision: u64,
    pub marker_owner_actor_id: u64,
    pub marker_owner_entity_uuid: i64,
    pub passive_instance_identity: i64,
    pub marker_number: u8,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub provenance: AutomarkerBridgeRecordProvenance,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct AutomarkerBridgeEvidenceSnapshot {
    pub feed_revision: u64,
    pub markers: Vec<AutomarkerBridgeMarkerEvidence>,
    pub outbound_carrier: Option<AutomarkerBridgeOutboundCarrierEvidence>,
    pub correlated_return: Option<AutomarkerBridgeCorrelatedReturnEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AutomarkerBridgeOutboundCarrierEvidence {
    pub capture_session_id: String,
    pub deployment_id: String,
    pub client_build: String,
    pub protocol_pack_digest: String,
    pub scene_id: i32,
    pub map_id: u32,
    pub activity_family_id: String,
    pub mechanics_runtime_revision: u64,
    pub marker_number: u8,
    pub session_sequence: u32,
    pub application_bytes: Vec<u8>,
    pub provenance: AutomarkerBridgeRecordProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AutomarkerBridgeCorrelatedReturnEvidence {
    pub capture_session_id: String,
    pub deployment_id: String,
    pub client_build: String,
    pub protocol_pack_digest: String,
    pub scene_id: i32,
    pub map_id: u32,
    pub activity_family_id: String,
    pub mechanics_runtime_revision: u64,
    pub carrier_capture_sequence: u64,
    pub provenance: AutomarkerBridgeRecordProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AutomarkerBridgeContextIdentity {
    client_build: String,
    scene_id: i32,
    map_id: u32,
    activity_family_id: String,
}

impl From<&AutomarkerSceneContext> for AutomarkerBridgeContextIdentity {
    fn from(scene: &AutomarkerSceneContext) -> Self {
        Self {
            client_build: scene.client_build.clone(),
            scene_id: scene.scene_id,
            map_id: scene.map_id,
            activity_family_id: scene.activity_family_id.clone(),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct AutomarkerBridgeEvidenceFeed {
    state: Mutex<AutomarkerBridgeEvidenceState>,
}

#[derive(Debug, Default)]
struct AutomarkerBridgeEvidenceState {
    session: Option<AutomarkerBridgeSessionIdentity>,
    feed_revision: u64,
    context: Option<AutomarkerBridgeContextIdentity>,
    markers: BTreeMap<u8, AutomarkerBridgeMarkerEvidence>,
    outbound_carrier: Option<AutomarkerBridgeOutboundCarrierEvidence>,
    correlated_return: Option<AutomarkerBridgeCorrelatedReturnEvidence>,
}

impl AutomarkerBridgeEvidenceFeed {
    pub(crate) fn begin_session(&self, session: AutomarkerBridgeSessionIdentity) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.session = Some(session);
        state.feed_revision = state.feed_revision.wrapping_add(1);
        state.context = None;
        state.markers.clear();
        state.outbound_carrier = None;
        state.correlated_return = None;
    }

    pub(crate) fn finish_session(&self, session_id: &str) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state
            .session
            .as_ref()
            .is_some_and(|session| session.capture_session_id == session_id)
        {
            state.session = None;
            state.feed_revision = state.feed_revision.wrapping_add(1);
            state.context = None;
            state.markers.clear();
            state.outbound_carrier = None;
            state.correlated_return = None;
        }
    }

    /// Observe the current scene selector independently of marker traffic.
    /// Any actual context transition invalidates coordinates immediately.
    pub(crate) fn reconcile_context(&self, scene: Option<&AutomarkerSceneContext>) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let next = scene.map(AutomarkerBridgeContextIdentity::from);
        if state.context == next {
            return false;
        }
        state.context = next;
        Self::invalidate_locked(&mut state);
        true
    }

    pub(crate) fn invalidate(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Self::invalidate_locked(&mut state);
    }

    fn invalidate_locked(state: &mut AutomarkerBridgeEvidenceState) {
        state.feed_revision = state.feed_revision.wrapping_add(1);
        state.markers.clear();
        state.outbound_carrier = None;
        state.correlated_return = None;
    }

    pub(crate) fn expire_stale_carrier(&self, current_observed_micros: u64) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let stale = state.outbound_carrier.as_ref().is_some_and(|carrier| {
            current_observed_micros.saturating_sub(carrier.provenance.observed_micros)
                > SINGLE_MARKER_XYZ_MAX_CARRIER_AGE_MILLIS.saturating_mul(1_000)
        });
        if !stale {
            return false;
        }
        state.feed_revision = state.feed_revision.wrapping_add(1);
        state.outbound_carrier = None;
        state.correlated_return = None;
        true
    }

    pub(crate) fn observe_outbound_carrier(
        &self,
        pack: &ProtocolPack,
        record: &CaptureRecord,
        scene: Option<&AutomarkerSceneContext>,
        mechanics: &MechanicsMapSnapshot,
        scratch: &mut Vec<u8>,
    ) -> Option<ObservedAutomarkerRequest> {
        let provenance = AutomarkerBridgeRecordProvenance::from_world_use_slot_record(
            pack,
            record,
            PacketDirection::ClientToServer,
            FragmentKind::Call,
        )?;
        let CaptureRecordKind::Packet(packet) = &record.kind else {
            return None;
        };
        if packet.compression != CompressionState::NotCompressed {
            self.invalidate();
            return None;
        }
        let Some(application) = packet.payload.decode_input() else {
            self.invalidate();
            return None;
        };
        if application.len() != EXACT_CARRIER_APPLICATION_BYTES {
            self.invalidate();
            return None;
        }
        let Ok(request) =
            rlogs_game_bpsr::decode_observed_automarker_request_into(pack, application, scratch)
        else {
            self.invalidate();
            return None;
        };
        self.retain_decoded_outbound_carrier(
            scene,
            mechanics,
            provenance,
            request,
            application,
            (&pack.definition().target.build_id, pack.digest()),
        )
        .then_some(request)
    }

    fn retain_decoded_outbound_carrier(
        &self,
        scene: Option<&AutomarkerSceneContext>,
        mechanics: &MechanicsMapSnapshot,
        provenance: AutomarkerBridgeRecordProvenance,
        request: ObservedAutomarkerRequest,
        application: &[u8],
        observed_pack: (&str, &str),
    ) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(session) = state
            .session
            .clone()
            .filter(|session| session.protocol_supported)
        else {
            Self::invalidate_locked(&mut state);
            return false;
        };
        let Some(scene) = scene else {
            Self::invalidate_locked(&mut state);
            return false;
        };
        let context = AutomarkerBridgeContextIdentity::from(scene);
        if state.context.as_ref() != Some(&context)
            || mechanics.session_id.as_deref() != Some(session.capture_session_id.as_str())
            || mechanics.client_build.as_deref() != Some(session.client_build.as_str())
            || mechanics.scene_id != Some(scene.scene_id)
            || mechanics.map_id != Some(scene.map_id)
            || scene.client_build != session.client_build
            || observed_pack.0 != session.client_build
            || observed_pack.1 != session.protocol_pack_digest
            || application.len() != EXACT_CARRIER_APPLICATION_BYTES
            || provenance.direction != PacketDirection::ClientToServer
            || provenance.fragment != FragmentKind::Call
            || provenance.service_id != WORLD_SERVICE_ID
            || provenance.method_id != WORLD_USE_SLOT_METHOD_ID
            || provenance.decoder != DecoderKind::WorldUseSlotV1
        {
            Self::invalidate_locked(&mut state);
            return false;
        }
        state.feed_revision = state.feed_revision.wrapping_add(1);
        state.correlated_return = None;
        state.outbound_carrier = Some(AutomarkerBridgeOutboundCarrierEvidence {
            capture_session_id: session.capture_session_id,
            deployment_id: session.deployment_id,
            client_build: session.client_build,
            protocol_pack_digest: session.protocol_pack_digest,
            scene_id: scene.scene_id,
            map_id: scene.map_id,
            activity_family_id: scene.activity_family_id.clone(),
            mechanics_runtime_revision: mechanics.revision,
            marker_number: request.marker_number,
            session_sequence: request.session_sequence,
            application_bytes: application.to_vec(),
            provenance,
        });
        true
    }

    pub(crate) fn observe_correlated_empty_return(
        &self,
        pack: &ProtocolPack,
        record: &CaptureRecord,
        scene: Option<&AutomarkerSceneContext>,
        mechanics: &MechanicsMapSnapshot,
    ) -> bool {
        let Some(provenance) = AutomarkerBridgeRecordProvenance::from_world_use_slot_record(
            pack,
            record,
            PacketDirection::ServerToClient,
            FragmentKind::Return,
        ) else {
            return false;
        };
        let CaptureRecordKind::Packet(packet) = &record.kind else {
            return false;
        };
        let wire = &packet.payload.wire_bytes;
        if packet.compression != CompressionState::NotCompressed
            || packet.payload.decode_input() != Some(&[])
            || wire.len() != EXACT_EMPTY_RETURN_FRAME_BYTES
            || u32::from_be_bytes(wire[0..4].try_into().expect("checked exact frame length"))
                as usize
                != wire.len()
            || u16::from_be_bytes(wire[4..6].try_into().expect("checked exact frame length")) != 3
            || u32::from_be_bytes(wire[14..18].try_into().expect("checked exact frame length")) != 0
        {
            self.invalidate();
            return false;
        }

        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(session) = state
            .session
            .clone()
            .filter(|session| session.protocol_supported)
        else {
            Self::invalidate_locked(&mut state);
            return false;
        };
        let (Some(scene), Some(carrier)) = (scene, state.outbound_carrier.clone()) else {
            Self::invalidate_locked(&mut state);
            return false;
        };
        let context = AutomarkerBridgeContextIdentity::from(scene);
        if state.context.as_ref() != Some(&context)
            || mechanics.session_id.as_deref() != Some(session.capture_session_id.as_str())
            || mechanics.client_build.as_deref() != Some(session.client_build.as_str())
            || mechanics.scene_id != Some(scene.scene_id)
            || mechanics.map_id != Some(scene.map_id)
            || pack.definition().target.build_id != session.client_build
            || pack.digest() != session.protocol_pack_digest
            || carrier.capture_session_id != session.capture_session_id
            || carrier.scene_id != scene.scene_id
            || carrier.map_id != scene.map_id
            || carrier.activity_family_id != scene.activity_family_id
            || provenance.connection_id != carrier.provenance.connection_id
            || provenance.call_id != carrier.provenance.call_id
            || provenance.observed_micros < carrier.provenance.observed_micros
            || provenance
                .observed_micros
                .saturating_sub(carrier.provenance.observed_micros)
                > RETURN_CORRELATION_MAX_MICROS
        {
            Self::invalidate_locked(&mut state);
            return false;
        }
        state.feed_revision = state.feed_revision.wrapping_add(1);
        state.correlated_return = Some(AutomarkerBridgeCorrelatedReturnEvidence {
            capture_session_id: session.capture_session_id,
            deployment_id: session.deployment_id,
            client_build: session.client_build,
            protocol_pack_digest: session.protocol_pack_digest,
            scene_id: scene.scene_id,
            map_id: scene.map_id,
            activity_family_id: scene.activity_family_id.clone(),
            mechanics_runtime_revision: mechanics.revision,
            carrier_capture_sequence: carrier.provenance.capture_sequence,
            provenance,
        });
        true
    }

    /// Replace the private baseline after the public decoder changed its
    /// authoritative projection. Missing, mismatched, or unresolved identity
    /// evidence clears the baseline rather than retaining stale coordinates.
    pub(crate) fn replace_from_decoded_projection(
        &self,
        scene: Option<&AutomarkerSceneContext>,
        mechanics: &MechanicsMapSnapshot,
        provenance: AutomarkerBridgeRecordProvenance,
        markers: impl IntoIterator<Item = LocalMapMarker>,
        changed_marker_numbers: &BTreeSet<u8>,
    ) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(session) = state
            .session
            .clone()
            .filter(|session| session.protocol_supported)
        else {
            Self::invalidate_locked(&mut state);
            return false;
        };
        let Some(scene) = scene else {
            state.context = None;
            Self::invalidate_locked(&mut state);
            return false;
        };
        let Some(local_actor_id) = mechanics.local_actor_id else {
            Self::invalidate_locked(&mut state);
            return false;
        };
        if mechanics.session_id.as_deref() != Some(session.capture_session_id.as_str())
            || mechanics.client_build.as_deref() != Some(session.client_build.as_str())
            || mechanics.scene_id != Some(scene.scene_id)
            || mechanics.map_id != Some(scene.map_id)
            || scene.client_build != session.client_build
        {
            Self::invalidate_locked(&mut state);
            return false;
        }

        let next_context = AutomarkerBridgeContextIdentity::from(scene);
        if state.context.as_ref() != Some(&next_context) {
            Self::invalidate_locked(&mut state);
            return false;
        }
        let next_revision = state.feed_revision.wrapping_add(1);
        let mut next = BTreeMap::new();
        for marker in markers {
            let (Some(owner_uuid), Some(x), Some(y), Some(z)) =
                (marker.related_entity_uuid, marker.x, marker.y, marker.z)
            else {
                continue;
            };
            if !(1..=6).contains(&marker.marker_number)
                || ![x, y, z]
                    .iter()
                    .all(|value| value.is_finite() && value.abs() <= 1_000_000.0)
            {
                continue;
            }
            let Some(owner_actor_id) = mechanics
                .entities
                .iter()
                .find(|entity| entity.entity_uuid == owner_uuid)
                .map(|entity| entity.actor_id)
            else {
                continue;
            };
            // Existing markers retain the exact provenance of their own
            // authoritative start; only markers changed by this record inherit it.
            let marker_provenance = if changed_marker_numbers.contains(&marker.marker_number) {
                provenance.clone()
            } else if let Some(existing) = state
                .markers
                .get(&marker.marker_number)
                .filter(|existing| existing.passive_instance_identity == marker.passive_instance_id)
            {
                existing.provenance.clone()
            } else {
                continue;
            };
            next.insert(
                marker.marker_number,
                AutomarkerBridgeMarkerEvidence {
                    capture_session_id: session.capture_session_id.clone(),
                    deployment_id: session.deployment_id.clone(),
                    client_build: session.client_build.clone(),
                    protocol_pack_digest: session.protocol_pack_digest.clone(),
                    scene_id: scene.scene_id,
                    map_id: scene.map_id,
                    activity_family_id: scene.activity_family_id.clone(),
                    local_actor_id,
                    feed_revision: next_revision,
                    mechanics_runtime_revision: mechanics.revision,
                    marker_owner_actor_id: owner_actor_id,
                    marker_owner_entity_uuid: owner_uuid,
                    passive_instance_identity: marker.passive_instance_id,
                    marker_number: marker.marker_number,
                    x,
                    y,
                    z,
                    provenance: marker_provenance,
                },
            );
        }
        state.feed_revision = next_revision;
        let changed = state.markers != next;
        state.markers = next;
        changed
    }

    #[allow(dead_code)]
    pub(crate) fn current(&self) -> AutomarkerBridgeEvidenceSnapshot {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        AutomarkerBridgeEvidenceSnapshot {
            feed_revision: state.feed_revision,
            markers: state.markers.values().cloned().collect(),
            outbound_carrier: state.outbound_carrier.clone(),
            correlated_return: state.correlated_return.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mechanics_map::MechanicsMapEntity;
    use rlogs_game_bpsr::{
        AutomarkerRequestAttributes, AutomarkerRequestPosition, CompressionState, PacketEnvelope,
        PacketPayload, RouteKey, RoutedMessage,
    };

    fn observed_request() -> ObservedAutomarkerRequest {
        ObservedAutomarkerRequest {
            marker_number: 1,
            slot_id: 201,
            skill_uuid: 1,
            skill_id: 1101,
            skill_level: 1,
            begin_time: 1,
            target_position: AutomarkerRequestPosition {
                x: 1.0,
                y: 2.0,
                z: 3.0,
                heading_degrees: 4.0,
            },
            current_position: AutomarkerRequestPosition {
                x: 5.0,
                y: 6.0,
                z: 7.0,
                heading_degrees: 8.0,
            },
            session_sequence: 9,
            attributes: AutomarkerRequestAttributes {
                timestamp: 10,
                velocity: 0.0,
                attack_speed_pct: 0,
                cast_speed_pct: 0,
                charge_speed_pct: None,
                opaque_current_build_scalar: 0.0,
            },
        }
    }

    fn carrier_provenance(call_id: u32, observed_micros: u64) -> AutomarkerBridgeRecordProvenance {
        AutomarkerBridgeRecordProvenance {
            capture_sequence: 30,
            observed_micros,
            wall_clock_unix_micros: Some(40),
            connection_id: 50,
            stream_id: 60,
            direction: PacketDirection::ClientToServer,
            fragment: FragmentKind::Call,
            service_id: WORLD_SERVICE_ID,
            method_id: WORLD_USE_SLOT_METHOD_ID,
            stub_id: 1,
            call_id: Some(call_id),
            decoder: DecoderKind::WorldUseSlotV1,
        }
    }

    fn source_pack() -> ProtocolPack {
        ProtocolPack::from_json(include_bytes!(
            "../../../plugins/games/blue-protocol-star-resonance/protocol-packs/global/steam-24687926/pack.json"
        ))
        .unwrap()
    }

    fn return_record(call_id: u32, observed_micros: u64) -> CaptureRecord {
        let mut wire = Vec::new();
        wire.extend_from_slice(&(EXACT_EMPTY_RETURN_FRAME_BYTES as u32).to_be_bytes());
        wire.extend_from_slice(&3_u16.to_be_bytes());
        wire.extend_from_slice(&1_u32.to_be_bytes());
        wire.extend_from_slice(&call_id.to_be_bytes());
        wire.extend_from_slice(&0_u32.to_be_bytes());
        CaptureRecord {
            sequence: 31,
            observed_micros,
            wall_clock_unix_micros: Some(41),
            kind: CaptureRecordKind::Packet(PacketEnvelope {
                connection_id: 50,
                stream_id: 61,
                source: None,
                destination: None,
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
                    application_bytes: Some(Vec::new()),
                },
            }),
        }
    }

    fn transport_context(
        pack: &ProtocolPack,
    ) -> (
        AutomarkerBridgeEvidenceFeed,
        AutomarkerSceneContext,
        MechanicsMapSnapshot,
    ) {
        let feed = AutomarkerBridgeEvidenceFeed::default();
        let build = pack.definition().target.build_id.clone();
        feed.begin_session(AutomarkerBridgeSessionIdentity {
            capture_session_id: "capture-transport".into(),
            deployment_id: pack.definition().target.deployment_id.clone(),
            client_build: build.clone(),
            protocol_pack_digest: pack.digest().into(),
            protocol_supported: true,
        });
        let scene = AutomarkerSceneContext {
            client_build: build.clone(),
            scene_id: 1,
            map_id: 2,
            activity_family_id: "mech-facility".into(),
            scene_name: None,
        };
        feed.reconcile_context(Some(&scene));
        let mechanics = MechanicsMapSnapshot {
            revision: 70,
            session_id: Some("capture-transport".into()),
            client_build: Some(build),
            scene_id: Some(1),
            map_id: Some(2),
            ..MechanicsMapSnapshot::default()
        };
        (feed, scene, mechanics)
    }

    fn matching_mechanics() -> MechanicsMapSnapshot {
        let mut mechanics = MechanicsMapSnapshot {
            revision: 22,
            session_id: Some("capture-a".into()),
            client_build: Some("25247556".into()),
            scene_id: Some(1),
            map_id: Some(2),
            local_actor_id: Some(7),
            ..MechanicsMapSnapshot::default()
        };
        mechanics.entities.push(MechanicsMapEntity {
            actor_id: 9,
            entity_uuid: 90,
            kind: "player",
            display_name: None,
            monster_id: None,
            mechanic_role: None,
            x: 1.0,
            y: 2.0,
            z: 3.0,
            facing_radians: None,
            dead: false,
            stale: false,
            last_observed_micros: 10,
        });
        mechanics
    }

    fn marker_provenance() -> AutomarkerBridgeRecordProvenance {
        AutomarkerBridgeRecordProvenance {
            capture_sequence: 4,
            observed_micros: 10,
            wall_clock_unix_micros: Some(20),
            connection_id: 3,
            stream_id: 5,
            direction: PacketDirection::ServerToClient,
            fragment: FragmentKind::Notify,
            service_id: WORLD_NOTIFICATION_SERVICE_ID,
            method_id: 45,
            stub_id: 8,
            call_id: Some(13),
            decoder: DecoderKind::SyncNearDeltaV1,
        }
    }

    #[test]
    fn provenance_is_derived_from_exact_reviewed_record_route() {
        let pack = ProtocolPack::from_json(include_bytes!(
            "../../../plugins/games/blue-protocol-star-resonance/protocol-packs/global/steam-24687926/pack.json"
        ))
        .unwrap();
        let key = RouteKey::new(
            PacketDirection::ServerToClient,
            FragmentKind::Notify,
            WORLD_NOTIFICATION_SERVICE_ID,
            45,
        );
        let record = CaptureRecord {
            sequence: 7,
            observed_micros: 11,
            wall_clock_unix_micros: Some(13),
            kind: CaptureRecordKind::Packet(PacketEnvelope {
                connection_id: 17,
                stream_id: 19,
                source: None,
                destination: None,
                direction: PacketDirection::ServerToClient,
                fragment: Some(FragmentKind::Notify),
                route: Some(RoutedMessage {
                    key,
                    stub_id: 23,
                    call_id: Some(29),
                }),
                compression: CompressionState::NotCompressed,
                payload: PacketPayload {
                    wire_bytes: Vec::new(),
                    application_bytes: Some(Vec::new()),
                },
            }),
        };
        let evidence =
            AutomarkerBridgeRecordProvenance::from_decoded_marker_record(&pack, &record).unwrap();
        assert_eq!(evidence.method_id, 45);
        assert_eq!(evidence.decoder, DecoderKind::SyncNearDeltaV1);
        assert_eq!(
            (
                evidence.capture_sequence,
                evidence.connection_id,
                evidence.stream_id
            ),
            (7, 17, 19)
        );
        assert_eq!((evidence.stub_id, evidence.call_id), (23, Some(29)));
    }

    #[test]
    fn session_finish_clears_private_evidence_and_advances_revision() {
        let feed = AutomarkerBridgeEvidenceFeed::default();
        feed.begin_session(AutomarkerBridgeSessionIdentity {
            capture_session_id: "capture-a".into(),
            deployment_id: "global".into(),
            client_build: "25247556".into(),
            protocol_pack_digest: "sha256:test".into(),
            protocol_supported: true,
        });
        let before = feed.current().feed_revision;
        feed.finish_session("capture-a");
        let after = feed.current();
        assert!(after.feed_revision > before);
        assert!(after.markers.is_empty());
    }

    #[test]
    fn mismatched_capture_context_fails_closed() {
        let feed = AutomarkerBridgeEvidenceFeed::default();
        feed.begin_session(AutomarkerBridgeSessionIdentity {
            capture_session_id: "capture-a".into(),
            deployment_id: "global".into(),
            client_build: "25247556".into(),
            protocol_pack_digest: "sha256:test".into(),
            protocol_supported: true,
        });
        let scene = AutomarkerSceneContext {
            client_build: "25247556".into(),
            scene_id: 1,
            map_id: 1,
            activity_family_id: "test".into(),
            scene_name: None,
        };
        let provenance = AutomarkerBridgeRecordProvenance {
            capture_sequence: 1,
            observed_micros: 10,
            wall_clock_unix_micros: None,
            connection_id: 1,
            stream_id: 1,
            direction: PacketDirection::ServerToClient,
            fragment: FragmentKind::Notify,
            service_id: WORLD_NOTIFICATION_SERVICE_ID,
            method_id: 45,
            stub_id: 1,
            call_id: None,
            decoder: DecoderKind::SyncNearDeltaV1,
        };
        feed.reconcile_context(Some(&scene));
        let mechanics = MechanicsMapSnapshot {
            session_id: Some("capture-b".into()),
            client_build: Some("25247556".into()),
            scene_id: Some(1),
            map_id: Some(1),
            local_actor_id: Some(7),
            ..MechanicsMapSnapshot::default()
        };
        assert!(!feed.replace_from_decoded_projection(
            Some(&scene),
            &mechanics,
            provenance,
            [],
            &BTreeSet::new(),
        ));
        assert!(feed.current().markers.is_empty());
    }

    #[test]
    fn preserves_private_marker_identity_context_and_exact_provenance() {
        let feed = AutomarkerBridgeEvidenceFeed::default();
        feed.begin_session(AutomarkerBridgeSessionIdentity {
            capture_session_id: "capture-a".into(),
            deployment_id: "global".into(),
            client_build: "25247556".into(),
            protocol_pack_digest: "sha256:test".into(),
            protocol_supported: true,
        });
        let scene = AutomarkerSceneContext {
            client_build: "25247556".into(),
            scene_id: 1,
            map_id: 2,
            activity_family_id: "mech-facility".into(),
            scene_name: None,
        };
        let provenance = marker_provenance();
        feed.reconcile_context(Some(&scene));
        assert!(feed.replace_from_decoded_projection(
            Some(&scene),
            &matching_mechanics(),
            provenance.clone(),
            [LocalMapMarker {
                passive_instance_id: 1234,
                related_entity_uuid: Some(90),
                marker_number: 1,
                x: Some(11.0),
                y: Some(12.0),
                z: Some(13.0),
                observed_micros: 10,
            }],
            &BTreeSet::from([1]),
        ));
        let snapshot = feed.current();
        let marker = snapshot.markers.first().unwrap();
        assert_eq!(marker.capture_session_id, "capture-a");
        assert_eq!(marker.deployment_id, "global");
        assert_eq!(marker.client_build, "25247556");
        assert_eq!(marker.protocol_pack_digest, "sha256:test");
        assert_eq!((marker.scene_id, marker.map_id), (1, 2));
        assert_eq!(marker.activity_family_id, "mech-facility");
        assert_eq!(
            (marker.local_actor_id, marker.marker_owner_actor_id),
            (7, 9)
        );
        assert_eq!(marker.marker_owner_entity_uuid, 90);
        assert_eq!(marker.passive_instance_identity, 1234);
        assert_eq!(
            (marker.marker_number, marker.x, marker.y, marker.z),
            (1, 11.0, 12.0, 13.0)
        );
        assert_eq!(marker.feed_revision, snapshot.feed_revision);
        assert_eq!(marker.mechanics_runtime_revision, 22);
        assert_eq!(marker.provenance, provenance);
    }

    #[test]
    fn sequential_records_preserve_each_markers_own_provenance() {
        let feed = AutomarkerBridgeEvidenceFeed::default();
        feed.begin_session(AutomarkerBridgeSessionIdentity {
            capture_session_id: "capture-a".into(),
            deployment_id: "global".into(),
            client_build: "25247556".into(),
            protocol_pack_digest: "sha256:test".into(),
            protocol_supported: true,
        });
        let scene = AutomarkerSceneContext {
            client_build: "25247556".into(),
            scene_id: 1,
            map_id: 2,
            activity_family_id: "mech-facility".into(),
            scene_name: None,
        };
        let mechanics = matching_mechanics();
        feed.reconcile_context(Some(&scene));
        let first = marker_provenance();
        let mut second = first.clone();
        second.capture_sequence = 5;
        // Capture clocks need not be unique; exact record deltas, not time,
        // disambiguate the two starts.
        second.observed_micros = 10;

        let marker_one = LocalMapMarker {
            passive_instance_id: 1234,
            related_entity_uuid: Some(90),
            marker_number: 1,
            x: Some(11.0),
            y: Some(12.0),
            z: Some(13.0),
            observed_micros: 10,
        };
        let marker_two = LocalMapMarker {
            passive_instance_id: 5678,
            related_entity_uuid: Some(90),
            marker_number: 2,
            x: Some(21.0),
            y: Some(22.0),
            z: Some(23.0),
            observed_micros: 10,
        };

        assert!(feed.replace_from_decoded_projection(
            Some(&scene),
            &mechanics,
            first.clone(),
            [marker_one],
            &BTreeSet::from([1]),
        ));
        assert!(feed.replace_from_decoded_projection(
            Some(&scene),
            &mechanics,
            second.clone(),
            [marker_one, marker_two],
            &BTreeSet::from([2]),
        ));

        let snapshot = feed.current();
        assert_eq!(snapshot.markers.len(), 2);
        assert_eq!(snapshot.markers[0].marker_number, 1);
        assert_eq!(snapshot.markers[0].provenance, first);
        assert_eq!(snapshot.markers[1].marker_number, 2);
        assert_eq!(snapshot.markers[1].provenance, second);
    }

    #[test]
    fn scene_transition_without_marker_traffic_invalidates_and_advances_revision() {
        let feed = AutomarkerBridgeEvidenceFeed::default();
        feed.begin_session(AutomarkerBridgeSessionIdentity {
            capture_session_id: "capture-a".into(),
            deployment_id: "global".into(),
            client_build: "25247556".into(),
            protocol_pack_digest: "sha256:test".into(),
            protocol_supported: true,
        });
        let first_scene = AutomarkerSceneContext {
            client_build: "25247556".into(),
            scene_id: 1,
            map_id: 2,
            activity_family_id: "mech-facility".into(),
            scene_name: None,
        };
        feed.reconcile_context(Some(&first_scene));
        feed.replace_from_decoded_projection(
            Some(&first_scene),
            &matching_mechanics(),
            marker_provenance(),
            [LocalMapMarker {
                passive_instance_id: 1234,
                related_entity_uuid: Some(90),
                marker_number: 1,
                x: Some(11.0),
                y: Some(12.0),
                z: Some(13.0),
                observed_micros: 10,
            }],
            &BTreeSet::from([1]),
        );
        let before = feed.current();
        assert_eq!(before.markers.len(), 1);
        let second_scene = AutomarkerSceneContext {
            scene_id: 3,
            map_id: 4,
            activity_family_id: "sea-ringed-reef".into(),
            ..first_scene.clone()
        };
        assert!(feed.reconcile_context(Some(&second_scene)));
        let after = feed.current();
        assert!(after.markers.is_empty());
        assert!(after.feed_revision > before.feed_revision);

        // A marker update decoded before the transition but drained after it
        // cannot move the trusted context backward.
        assert!(!feed.replace_from_decoded_projection(
            Some(&first_scene),
            &matching_mechanics(),
            marker_provenance(),
            [LocalMapMarker {
                passive_instance_id: 1234,
                related_entity_uuid: Some(90),
                marker_number: 1,
                x: Some(11.0),
                y: Some(12.0),
                z: Some(13.0),
                observed_micros: 10,
            }],
            &BTreeSet::from([1]),
        ));
        assert!(feed.current().markers.is_empty());
    }

    #[test]
    fn carrier_bytes_are_exactly_bounded_and_expire() {
        let pack = source_pack();
        let (feed, scene, mechanics) = transport_context(&pack);
        assert!(!feed.retain_decoded_outbound_carrier(
            Some(&scene),
            &mechanics,
            carrier_provenance(77, 100),
            observed_request(),
            &[0; EXACT_CARRIER_APPLICATION_BYTES + 1],
            (&pack.definition().target.build_id, pack.digest()),
        ));
        assert!(feed.current().outbound_carrier.is_none());

        feed.reconcile_context(Some(&scene));
        assert!(feed.retain_decoded_outbound_carrier(
            Some(&scene),
            &mechanics,
            carrier_provenance(77, 100),
            observed_request(),
            &[0; EXACT_CARRIER_APPLICATION_BYTES],
            (&pack.definition().target.build_id, pack.digest()),
        ));
        assert_eq!(
            feed.current()
                .outbound_carrier
                .as_ref()
                .unwrap()
                .application_bytes
                .len(),
            EXACT_CARRIER_APPLICATION_BYTES
        );
        assert!(
            feed.expire_stale_carrier(100 + SINGLE_MARKER_XYZ_MAX_CARRIER_AGE_MILLIS * 1_000 + 1)
        );
        assert!(feed.current().outbound_carrier.is_none());
    }

    #[test]
    fn only_exact_correlated_successful_empty_return_is_retained() {
        let pack = source_pack();
        let (feed, scene, mechanics) = transport_context(&pack);
        assert!(feed.retain_decoded_outbound_carrier(
            Some(&scene),
            &mechanics,
            carrier_provenance(77, 100),
            observed_request(),
            &[0; EXACT_CARRIER_APPLICATION_BYTES],
            (&pack.definition().target.build_id, pack.digest()),
        ));
        assert!(!feed.observe_correlated_empty_return(
            &pack,
            &return_record(78, 200),
            Some(&scene),
            &mechanics,
        ));
        assert!(feed.current().outbound_carrier.is_none());

        feed.reconcile_context(Some(&scene));
        assert!(feed.retain_decoded_outbound_carrier(
            Some(&scene),
            &mechanics,
            carrier_provenance(77, 100),
            observed_request(),
            &[0; EXACT_CARRIER_APPLICATION_BYTES],
            (&pack.definition().target.build_id, pack.digest()),
        ));
        assert!(feed.observe_correlated_empty_return(
            &pack,
            &return_record(77, 200),
            Some(&scene),
            &mechanics,
        ));
        let snapshot = feed.current();
        assert_eq!(
            snapshot.correlated_return.unwrap().carrier_capture_sequence,
            30
        );
    }
}
