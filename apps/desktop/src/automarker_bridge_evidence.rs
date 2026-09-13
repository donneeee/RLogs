//! Process-private, fail-closed evidence retained for a future native marker driver.
//!
//! This projection is deliberately not serializable and is not returned by any
//! HTTP/UI surface.  The public observed-marker feed remains the privacy boundary.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Mutex,
};

use rlogs_game_bpsr::{
    CaptureRecord, CaptureRecordKind, DecoderKind, FragmentKind, LocalMapMarker, PacketDirection,
    ProtocolPack,
};

use crate::{automarker_presets::AutomarkerSceneContext, mechanics_map::MechanicsMapSnapshot};

const WORLD_NOTIFICATION_SERVICE_ID: u64 = 1_664_308_034;

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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mechanics_map::MechanicsMapEntity;
    use rlogs_game_bpsr::{
        CompressionState, PacketEnvelope, PacketPayload, RouteKey, RoutedMessage,
    };

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
}
