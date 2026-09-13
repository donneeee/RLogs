//! Provisional, local-only projection of reference-derived in-game marker observations.
//!
//! These values deliberately never become canonical events or submission data.

use std::collections::BTreeMap;

use prost::Message;

use crate::{
    AllowedDataDomain, BpsrFrameUpLayout, CaptureRecord, CaptureRecordKind, DecoderKind,
    FragmentKind, PacketDirection, ProtocolPack, ProtocolPackRouteDisposition, RouteKey,
    bpsr_runtime_authority, game_schema_v1 as schema,
};

const WORLD_NTF: u64 = 1_664_308_034;
const MAX_MARKERS: usize = 64;

const MARKER_OBSERVER_ROUTE_CONTRACTS: &[(u32, AllowedDataDomain, DecoderKind)] = &[
    (3, AllowedDataDomain::WorldState, DecoderKind::EnterSceneV1),
    (
        4,
        AllowedDataDomain::WorldState,
        DecoderKind::NotifyLoadSceneEndV1,
    ),
    (
        6,
        AllowedDataDomain::ActorState,
        DecoderKind::SyncNearEntitiesV1,
    ),
    (45, AllowedDataDomain::Combat, DecoderKind::SyncNearDeltaV1),
    (46, AllowedDataDomain::Combat, DecoderKind::SyncToMeDeltaV1),
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocalMapMarker {
    pub passive_instance_id: i64,
    pub related_entity_uuid: Option<i64>,
    pub marker_number: u8,
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub z: Option<f32>,
    /// Capture-clock observation time for the authoritative inbound start.
    pub observed_micros: u64,
}

/// Why the current observed marker projection cannot yet be imported as a
/// preset. Protocol verification remains a separate capability gate: this
/// type only describes whether an already-observed snapshot is structurally
/// safe to hand to the local preset store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalMapMarkerSnapshotError {
    Empty,
    InvalidMarkerNumber,
    DuplicateNumber,
    MissingCoordinate,
    InvalidCoordinate,
}

#[derive(Debug, Default)]
pub struct LocalMapMarkerProjection {
    // One live value per numbered game slot. A later authoritative start for
    // the same number is a move/replacement, even when the passive instance
    // identity changes.
    markers: BTreeMap<u8, LocalMapMarker>,
}

impl LocalMapMarkerProjection {
    /// Whether this pack has compatibility-epoch authority for passive marker
    /// observation and retains the exact framing and route semantics consumed
    /// by this projection. The scoped capability contract deliberately ignores
    /// pack identity/provenance metadata changed by reviewed retargeting while
    /// failing closed if any scene or marker decoder dependency changes. It
    /// grants no authority to send, replay, or infer an outbound marker request.
    pub fn protocol_supported(pack: &ProtocolPack) -> bool {
        let target = &pack.definition().target;
        matches!(
            bpsr_runtime_authority(&target.deployment_id, &target.build_id, pack.digest(),),
            Ok(Some(_))
        ) && marker_observer_capability_matches(pack)
    }

    pub fn markers(&self) -> impl Iterator<Item = LocalMapMarker> + '_ {
        self.markers.values().copied()
    }

    /// Returns one deterministic, fully positioned marker per game marker
    /// number. This deliberately does not claim that the provisional decoder
    /// is verified; callers must keep their independent build/protocol gate.
    pub fn preset_snapshot(&self) -> Result<Vec<LocalMapMarker>, LocalMapMarkerSnapshotError> {
        if self.markers.is_empty() {
            return Err(LocalMapMarkerSnapshotError::Empty);
        }
        let mut by_number = BTreeMap::new();
        for marker in self.markers.values().copied() {
            if !(1..=6).contains(&marker.marker_number) {
                return Err(LocalMapMarkerSnapshotError::InvalidMarkerNumber);
            }
            let (Some(x), Some(y), Some(z)) = (marker.x, marker.y, marker.z) else {
                return Err(LocalMapMarkerSnapshotError::MissingCoordinate);
            };
            if !x.is_finite()
                || !y.is_finite()
                || !z.is_finite()
                || [x, y, z].iter().any(|value| value.abs() > 1_000_000.0)
            {
                return Err(LocalMapMarkerSnapshotError::InvalidCoordinate);
            }
            if by_number.insert(marker.marker_number, marker).is_some() {
                return Err(LocalMapMarkerSnapshotError::DuplicateNumber);
            }
        }
        Ok(by_number.into_values().collect())
    }

    pub fn observe(&mut self, pack: &ProtocolPack, record: &CaptureRecord) -> bool {
        if !Self::protocol_supported(pack) {
            return false;
        }
        let CaptureRecordKind::Packet(packet) = &record.kind else {
            return false;
        };
        let Some(routed) = packet.route else {
            return false;
        };
        let key = routed.key;
        if key.direction != PacketDirection::ServerToClient
            || key.fragment != FragmentKind::Notify
            || key.service_id != WORLD_NTF
        {
            return false;
        }
        if matches!(key.method_id, 3 | 4) {
            return !self.markers.is_empty() && {
                self.markers.clear();
                true
            };
        }
        let expected = match key.method_id {
            6 => DecoderKind::SyncNearEntitiesV1,
            45 => DecoderKind::SyncNearDeltaV1,
            46 => DecoderKind::SyncToMeDeltaV1,
            _ => return false,
        };
        if pack.decoder(&key) != Some(expected) {
            return false;
        }
        let Some(payload) = packet.payload.decode_input() else {
            return false;
        };
        let deltas: Vec<(
            Option<i64>,
            Option<schema::SeqPassiveSkillInfo>,
            Option<schema::SeqPassiveSkillEndInfo>,
        )> = match key.method_id {
            6 => match schema::SyncNearEntities::decode(payload) {
                Ok(message) => message
                    .appeared
                    .into_iter()
                    .filter_map(|entity| {
                        entity
                            .passive_skill_infos
                            .map(|passives| (entity.uuid, Some(passives), None))
                    })
                    .collect(),
                Err(_) => return false,
            },
            45 => match schema::SyncNearDeltaInfo::decode(payload) {
                Ok(message) => message
                    .deltas
                    .into_iter()
                    .map(|delta| {
                        (
                            delta.uuid,
                            delta.passive_skill_infos,
                            delta.passive_skill_end_infos,
                        )
                    })
                    .collect(),
                Err(_) => return false,
            },
            46 => match schema::SyncToMeDeltaInfo::decode(payload) {
                Ok(message) => message
                    .delta
                    .and_then(|delta| delta.base_delta)
                    .into_iter()
                    .map(|delta| {
                        (
                            delta.uuid,
                            delta.passive_skill_infos,
                            delta.passive_skill_end_infos,
                        )
                    })
                    .collect(),
                Err(_) => return false,
            },
            _ => unreachable!(),
        };
        let mut changed = false;
        for (fallback, starts, ends) in deltas {
            if let Some(starts) = starts {
                let actor = starts.actor_uuid.or(fallback);
                for start in starts.passive_infos {
                    let (Some(instance), Some(skill)) = (start.uuid.map(i64::from), start.skill_id)
                    else {
                        continue;
                    };
                    let Ok(marker_number) = u8::try_from(skill - 1100) else {
                        continue;
                    };
                    if !(1..=6).contains(&marker_number) {
                        continue;
                    }
                    let position = start
                        .target_position
                        .as_deref()
                        .and_then(|bytes| schema::Position::decode(bytes).ok());
                    let marker = LocalMapMarker {
                        passive_instance_id: instance,
                        related_entity_uuid: actor,
                        marker_number,
                        x: position.as_ref().and_then(|p| p.x),
                        y: position.as_ref().and_then(|p| p.y),
                        z: position.as_ref().and_then(|p| p.z),
                        observed_micros: record.observed_micros,
                    };
                    if self.markers.contains_key(&marker_number) || self.markers.len() < MAX_MARKERS
                    {
                        changed |= self.markers.insert(marker_number, marker) != Some(marker);
                    }
                }
            }
            if let Some(ends) = ends {
                for instance in ends.passive_uuids {
                    let before = self.markers.len();
                    self.markers
                        .retain(|_, marker| marker.passive_instance_id != instance);
                    changed |= self.markers.len() != before;
                }
            }
        }
        changed
    }
}

fn marker_observer_capability_matches(pack: &ProtocolPack) -> bool {
    if pack.definition().acquisition.frame_up_layout != BpsrFrameUpLayout::NestedAfterFourBytes {
        return false;
    }

    MARKER_OBSERVER_ROUTE_CONTRACTS
        .iter()
        .all(|(method_id, expected_domain, expected_decoder)| {
            let key = RouteKey::new(
                PacketDirection::ServerToClient,
                FragmentKind::Notify,
                WORLD_NTF,
                *method_id,
            );
            pack.definition()
                .routes
                .iter()
                .find(|route| route.route == key)
                .is_some_and(|route| {
                    route.disposition
                        == ProtocolPackRouteDisposition::Allowed {
                            domain: *expected_domain,
                            decoder: *expected_decoder,
                        }
                })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CaptureRecordKind, CompressionState, PacketEnvelope, PacketPayload, RouteKey, RoutedMessage,
    };

    const REVIEWED_MARKER_OBSERVER_SOURCE_PACK_DIGEST: &str =
        crate::BPSR_COMPATIBILITY_EPOCH_SOURCE_DIGEST;
    const VERIFIED_MARKER_OBSERVER_COMPATIBILITY_BUILD: &str = "25247556";
    const VERIFIED_MARKER_OBSERVER_COMPATIBILITY_PACK_DIGEST: &str =
        "sha256:480f928cca6baf19c1ebaf85e8c52f2f1852096167043260503cca6d46133a60";

    fn source_observer_pack() -> ProtocolPack {
        ProtocolPack::from_json(include_bytes!(
            "../protocol-packs/global/steam-24687926/pack.json"
        ))
        .unwrap()
    }

    fn current_observer_pack(build: &str) -> ProtocolPack {
        crate::compatibility_epoch::retarget_protocol_pack(
            &source_observer_pack(),
            "compatibility-fallback",
            "global",
            "steam",
            build,
        )
        .unwrap()
    }

    fn bootstrap_observer_pack(channel: &str) -> ProtocolPack {
        crate::compatibility_epoch::retarget_protocol_pack(
            &source_observer_pack(),
            "client-bootstrap",
            "unknown",
            channel,
            crate::BPSR_COMPATIBILITY_EPOCH_SOURCE_BUILD,
        )
        .unwrap()
    }

    fn record(method: u32, payload: Vec<u8>) -> CaptureRecord {
        CaptureRecord {
            sequence: 1,
            observed_micros: 1,
            wall_clock_unix_micros: None,
            kind: CaptureRecordKind::Packet(PacketEnvelope {
                connection_id: 1,
                stream_id: 1,
                source: None,
                destination: None,
                direction: PacketDirection::ServerToClient,
                fragment: Some(FragmentKind::Notify),
                route: Some(RoutedMessage {
                    key: RouteKey::new(
                        PacketDirection::ServerToClient,
                        FragmentKind::Notify,
                        WORLD_NTF,
                        method,
                    ),
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
    fn lifecycle_and_scene_clear_remain_local() {
        // This is the exact unresolved-region bootstrap identity observed by
        // live capture; its pack metadata differs from the reviewed source,
        // while its scoped marker capability is identical.
        let pack = bootstrap_observer_pack("unknown");
        let passive = schema::SeqPassiveSkillInfo {
            actor_uuid: Some(10),
            passive_infos: vec![schema::PassiveSkillInfo {
                uuid: Some(20),
                target_uuid: None,
                skill_id: Some(1103),
                target_position: None,
                ..Default::default()
            }],
        };
        let start = schema::SyncNearDeltaInfo {
            deltas: vec![schema::AoiSyncDelta {
                passive_skill_infos: Some(passive),
                ..Default::default()
            }],
        }
        .encode_to_vec();
        let end = schema::SyncNearDeltaInfo {
            deltas: vec![schema::AoiSyncDelta {
                passive_skill_end_infos: Some(schema::SeqPassiveSkillEndInfo {
                    actor_uuid: Some(10),
                    passive_uuids: vec![20],
                }),
                ..Default::default()
            }],
        }
        .encode_to_vec();
        let mut projection = LocalMapMarkerProjection::default();
        assert!(projection.observe(&pack, &record(45, start.clone())));
        assert_eq!(projection.markers().next().unwrap().marker_number, 3);
        assert!(!projection.observe(&pack, &record(45, start)));
        assert!(projection.observe(&pack, &record(45, end)));
        assert_eq!(projection.markers().count(), 0);
        assert!(
            projection.observe(
                &pack,
                &record(
                    45,
                    schema::SyncNearDeltaInfo {
                        deltas: vec![schema::AoiSyncDelta {
                            passive_skill_infos: Some(schema::SeqPassiveSkillInfo {
                                actor_uuid: None,
                                passive_infos: vec![schema::PassiveSkillInfo {
                                    uuid: Some(21),
                                    skill_id: Some(1101),
                                    ..Default::default()
                                }]
                            }),
                            ..Default::default()
                        }]
                    }
                    .encode_to_vec()
                )
            )
        );
        assert!(projection.observe(&pack, &record(3, vec![])));
        assert_eq!(projection.markers().count(), 0);
    }

    #[test]
    fn same_number_is_replaced_and_only_its_proven_current_end_removes_it() {
        let pack = source_observer_pack();
        let start = |instance: i32, x: f32, observed_micros: u64| {
            let payload = schema::SyncNearDeltaInfo {
                deltas: vec![schema::AoiSyncDelta {
                    passive_skill_infos: Some(schema::SeqPassiveSkillInfo {
                        actor_uuid: None,
                        passive_infos: vec![schema::PassiveSkillInfo {
                            uuid: Some(instance),
                            skill_id: Some(1101),
                            target_position: Some(
                                schema::Position {
                                    x: Some(x),
                                    y: Some(2.0),
                                    z: Some(3.0),
                                    facing_radians: None,
                                }
                                .encode_to_vec(),
                            ),
                            ..Default::default()
                        }],
                    }),
                    ..Default::default()
                }],
            }
            .encode_to_vec();
            let mut value = record(45, payload);
            value.observed_micros = observed_micros;
            value
        };
        let end = |instance: i64| {
            record(
                45,
                schema::SyncNearDeltaInfo {
                    deltas: vec![schema::AoiSyncDelta {
                        passive_skill_end_infos: Some(schema::SeqPassiveSkillEndInfo {
                            actor_uuid: None,
                            passive_uuids: vec![instance],
                        }),
                        ..Default::default()
                    }],
                }
                .encode_to_vec(),
            )
        };

        let mut projection = LocalMapMarkerProjection::default();
        assert!(projection.observe(&pack, &start(10, 1.0, 100)));
        assert!(projection.observe(&pack, &start(11, 9.0, 200)));
        let moved = projection.markers().next().unwrap();
        assert_eq!(projection.markers().count(), 1);
        assert_eq!(
            (moved.passive_instance_id, moved.x, moved.observed_micros),
            (11, Some(9.0), 200)
        );
        assert!(!projection.observe(&pack, &end(10)));
        assert_eq!(projection.markers().count(), 1);
        assert!(projection.observe(&pack, &end(11)));
        assert_eq!(projection.markers().count(), 0);
    }

    #[test]
    fn historical_pack_cannot_enable_provisional_semantics() {
        let pack = ProtocolPack::from_json(include_bytes!(
            "../protocol-packs/global/steam-24252055/pack.json"
        ))
        .unwrap();
        let mut projection = LocalMapMarkerProjection::default();
        assert!(!projection.observe(&pack, &record(45, vec![])));
        assert_eq!(projection.markers().count(), 0);
    }

    #[test]
    fn marker_observation_accepts_exact_and_authorized_compatibility_epoch_packs() {
        let source = source_observer_pack();
        assert!(LocalMapMarkerProjection::protocol_supported(&source));

        let unresolved_region_bootstrap = bootstrap_observer_pack("unknown");
        assert_eq!(
            unresolved_region_bootstrap.digest(),
            "sha256:52b0d954ed3d1179f9fd75dd2edd312cbea66d0d84168f957a47cf81ef3e86ee"
        );
        assert!(LocalMapMarkerProjection::protocol_supported(
            &unresolved_region_bootstrap
        ));
        for channel in crate::compatibility_epoch::BPSR_COMPATIBILITY_BOOTSTRAP_CHANNELS {
            assert!(LocalMapMarkerProjection::protocol_supported(
                &bootstrap_observer_pack(channel)
            ));
        }

        let current = current_observer_pack("25247556");
        assert_eq!(
            current.digest(),
            "sha256:480f928cca6baf19c1ebaf85e8c52f2f1852096167043260503cca6d46133a60"
        );
        assert!(LocalMapMarkerProjection::protocol_supported(&current));
        let later_same_epoch = current_observer_pack("26000000");
        assert!(matches!(
            crate::bpsr_runtime_authority("global", "26000000", later_same_epoch.digest()).unwrap(),
            Some(crate::BpsrRuntimeAuthority::CompatibilityEpoch { .. })
        ));
        assert!(LocalMapMarkerProjection::protocol_supported(
            &later_same_epoch
        ));

        for (method, expected) in [
            (3, DecoderKind::EnterSceneV1),
            (4, DecoderKind::NotifyLoadSceneEndV1),
            (6, DecoderKind::SyncNearEntitiesV1),
            (45, DecoderKind::SyncNearDeltaV1),
            (46, DecoderKind::SyncToMeDeltaV1),
        ] {
            let inbound = RouteKey::new(
                PacketDirection::ServerToClient,
                FragmentKind::Notify,
                WORLD_NTF,
                method,
            );
            assert_eq!(current.decoder(&inbound), Some(expected));
            for (direction, fragment) in [
                (PacketDirection::ClientToServer, FragmentKind::Notify),
                (PacketDirection::ClientToServer, FragmentKind::Call),
                (PacketDirection::ClientToServer, FragmentKind::Return),
                (PacketDirection::ServerToClient, FragmentKind::Call),
                (PacketDirection::ServerToClient, FragmentKind::Return),
            ] {
                assert_eq!(
                    current.decoder(&RouteKey::new(direction, fragment, WORLD_NTF, method)),
                    None
                );
            }
        }

        let mut wrong_definition = current.definition().clone();
        wrong_definition.pack_id.push_str("-wrong-digest");
        let wrong_digest = ProtocolPack::build(wrong_definition).unwrap();
        assert_ne!(wrong_digest.digest(), current.digest());
        assert!(marker_observer_capability_matches(&wrong_digest));
        assert!(!LocalMapMarkerProjection::protocol_supported(&wrong_digest));
    }

    #[test]
    fn marker_observer_capability_fails_closed_on_framing_or_route_drift() {
        let source = source_observer_pack();
        assert!(marker_observer_capability_matches(&source));

        let mut wrong_framing = source.definition().clone();
        wrong_framing.acquisition.frame_up_layout = BpsrFrameUpLayout::Opaque;
        assert!(!marker_observer_capability_matches(
            &ProtocolPack::build(wrong_framing).unwrap()
        ));

        for (method_id, _, _) in MARKER_OBSERVER_ROUTE_CONTRACTS {
            let key = RouteKey::new(
                PacketDirection::ServerToClient,
                FragmentKind::Notify,
                WORLD_NTF,
                *method_id,
            );
            let mut missing_route = source.definition().clone();
            let route = missing_route
                .routes
                .iter_mut()
                .find(|route| route.route == key)
                .unwrap();
            route.route.service_id += 1;
            assert!(!marker_observer_capability_matches(
                &ProtocolPack::build(missing_route).unwrap()
            ));

            let mut opaque_route = source.definition().clone();
            opaque_route
                .routes
                .iter_mut()
                .find(|route| route.route == key)
                .unwrap()
                .disposition = ProtocolPackRouteDisposition::Opaque;
            assert!(!marker_observer_capability_matches(
                &ProtocolPack::build(opaque_route).unwrap()
            ));
        }

        for (method_id, replacement) in [
            (3, DecoderKind::NotifyLoadSceneEndV1),
            (4, DecoderKind::EnterSceneV1),
            (45, DecoderKind::SyncToMeDeltaV1),
            (46, DecoderKind::SyncNearDeltaV1),
        ] {
            let key = RouteKey::new(
                PacketDirection::ServerToClient,
                FragmentKind::Notify,
                WORLD_NTF,
                method_id,
            );
            let mut wrong_decoder = source.definition().clone();
            let route = wrong_decoder
                .routes
                .iter_mut()
                .find(|route| route.route == key)
                .unwrap();
            route.disposition = ProtocolPackRouteDisposition::Allowed {
                domain: replacement.domain(),
                decoder: replacement,
            };
            assert!(!marker_observer_capability_matches(
                &ProtocolPack::build(wrong_decoder).unwrap()
            ));
        }
    }

    #[test]
    fn client_to_server_marker_shaped_payload_is_never_observed() {
        let payload = schema::SyncNearDeltaInfo {
            deltas: vec![schema::AoiSyncDelta {
                passive_skill_infos: Some(schema::SeqPassiveSkillInfo {
                    actor_uuid: None,
                    passive_infos: vec![schema::PassiveSkillInfo {
                        uuid: Some(1),
                        skill_id: Some(1101),
                        target_position: Some(
                            schema::Position {
                                x: Some(1.0),
                                y: Some(2.0),
                                z: Some(3.0),
                                facing_radians: None,
                            }
                            .encode_to_vec(),
                        ),
                        ..Default::default()
                    }],
                }),
                ..Default::default()
            }],
        }
        .encode_to_vec();
        let mut outbound = record(45, payload);
        let CaptureRecordKind::Packet(packet) = &mut outbound.kind else {
            unreachable!();
        };
        packet.direction = PacketDirection::ClientToServer;
        packet.route.as_mut().unwrap().key = RouteKey::new(
            PacketDirection::ClientToServer,
            FragmentKind::Notify,
            WORLD_NTF,
            45,
        );
        let mut projection = LocalMapMarkerProjection::default();
        assert!(!projection.observe(
            &current_observer_pack(VERIFIED_MARKER_OBSERVER_COMPATIBILITY_BUILD),
            &outbound
        ));
        assert_eq!(projection.markers().count(), 0);
    }

    #[test]
    fn current_verified_build_projects_all_six_observed_ground_points() {
        let proof: serde_json::Value = serde_json::from_str(include_str!(
            "../research/game-file-inventory/global/steam-25247556/inbound-ground-marker-observation-proof.v1.json"
        ))
        .unwrap();
        assert_eq!(
            proof["observer_gate_build"],
            VERIFIED_MARKER_OBSERVER_COMPATIBILITY_BUILD
        );
        assert_eq!(
            proof["protocol_identity"]["runtime_derivation"]["derived_pack_semantic_digest"],
            VERIFIED_MARKER_OBSERVER_COMPATIBILITY_PACK_DIGEST
        );
        assert_eq!(
            proof["protocol_identity"]["source_pack_semantic_digest"],
            REVIEWED_MARKER_OBSERVER_SOURCE_PACK_DIGEST
        );
        assert_eq!(
            proof["build_identity"]["capture_client_build_verified"],
            false
        );
        assert_eq!(
            proof["independent_direct_pcap_analysis"]["exact_signature"]["direction"],
            "server_to_client"
        );
        assert_eq!(
            proof["independent_direct_pcap_analysis"]["exact_signature"]["fragment"],
            "notify"
        );
        assert_eq!(
            proof["independent_direct_pcap_analysis"]["exact_signature"]["service_id"],
            WORLD_NTF
        );
        assert_eq!(
            proof["independent_direct_pcap_analysis"]["exact_signature"]["method_id"],
            45
        );
        assert_eq!(
            proof["independent_direct_pcap_analysis"]["exact_signature"]["marker_skill_id_path"],
            "1.8.2.6"
        );
        assert_eq!(
            proof["conclusion"]["outbound_placement_route_proven"],
            false
        );
        assert_eq!(
            proof["conclusion"]["permission_to_send_or_replay_packets"],
            false
        );
        let receipt: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/automarker/tina-m20-six-marker-points.v1.json"
        ))
        .unwrap();
        assert_eq!(
            receipt["evidenceKind"],
            "sanitized-inbound-marker-coordinate-receipt"
        );
        assert!(
            receipt["privacy"]
                .as_object()
                .unwrap()
                .values()
                .all(|value| value == false)
        );
        let observed = receipt["points"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                (
                    row["markerNumber"].as_u64().unwrap() as u8,
                    row["x"].as_f64().unwrap() as f32,
                    row["y"].as_f64().unwrap() as f32,
                    row["z"].as_f64().unwrap() as f32,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(observed.len(), 6);
        let payload = schema::SyncNearDeltaInfo {
            deltas: observed
                .iter()
                .map(|(number, x, y, z)| schema::AoiSyncDelta {
                    passive_skill_infos: Some(schema::SeqPassiveSkillInfo {
                        actor_uuid: None,
                        passive_infos: vec![schema::PassiveSkillInfo {
                            uuid: Some(i32::from(*number)),
                            target_uuid: None,
                            skill_id: Some(1100 + i32::from(*number)),
                            target_position: Some(
                                schema::Position {
                                    x: Some(*x),
                                    y: Some(*y),
                                    z: Some(*z),
                                    facing_radians: None,
                                }
                                .encode_to_vec(),
                            ),
                            ..Default::default()
                        }],
                    }),
                    ..Default::default()
                })
                .collect(),
        }
        .encode_to_vec();
        let mut projection = LocalMapMarkerProjection::default();
        assert!(projection.observe(
            &current_observer_pack(VERIFIED_MARKER_OBSERVER_COMPATIBILITY_BUILD),
            &record(45, payload)
        ));
        let snapshot = projection.preset_snapshot().unwrap();
        assert_eq!(snapshot.len(), 6);
        for (marker, (number, x, y, z)) in snapshot.iter().zip(observed) {
            assert_eq!(marker.marker_number, number);
            assert_eq!((marker.x, marker.y, marker.z), (Some(x), Some(y), Some(z)));
        }
    }

    #[test]
    fn current_build_use_slot_session_sequence_schema_is_bound_to_static_proof() {
        let proof: serde_json::Value = serde_json::from_str(include_str!(
            "../research/game-file-inventory/global/steam-25247556/use-slot-current-schema-transport-proof.v1.json"
        ))
        .unwrap();
        let field = &proof["exact_current_build_descriptor_evidence"]["field"];
        assert_eq!(proof["build_id"], "25247556");
        assert_eq!(field["number"], 5);
        assert_eq!(field["proto_name"], "sessionSequence");
        assert_eq!(field["proto_type"], "uint32");
        assert_eq!(field["wire_type"], "varint");
        assert_eq!(proof["recommendation"]["runtime_sender_enabled"], false);
        assert_eq!(proof["privacy"]["contains_cryptographic_keys"], false);
        assert_eq!(proof["privacy"]["contains_raw_payloads"], false);
        assert_eq!(proof["privacy"]["contains_network_endpoints"], false);
        assert_eq!(proof["privacy"]["contains_local_absolute_paths"], false);
        assert_eq!(proof["privacy"]["contains_personal_identity"], false);

        let outbound: serde_json::Value = serde_json::from_str(include_str!(
            "../research/game-file-inventory/global/steam-25247556/outbound-ground-marker-request-correlation-proof.v1.json"
        ))
        .unwrap();
        assert_eq!(
            outbound["exact_current_build_request_schema_proof"]["artifact"],
            "use-slot-current-schema-transport-proof.v1.json"
        );
        assert_eq!(
            outbound["exact_current_build_request_schema_proof"]["field_5"]["proto_name"],
            "sessionSequence"
        );
        assert_eq!(
            outbound["exact_current_build_request_schema_proof"]["field_5"]["proto_type"],
            "uint32"
        );
        assert!(
            outbound["request_schema"]["field_1_5"]
                .as_str()
                .unwrap()
                .contains("sessionSequence")
        );
    }

    #[test]
    fn preset_snapshot_requires_unique_complete_finite_points_and_sorts_them() {
        let marker = |instance, number, x, y, z| LocalMapMarker {
            passive_instance_id: instance,
            related_entity_uuid: None,
            marker_number: number,
            x,
            y,
            z,
            observed_micros: instance as u64,
        };
        let mut projection = LocalMapMarkerProjection::default();
        assert_eq!(
            projection.preset_snapshot(),
            Err(LocalMapMarkerSnapshotError::Empty)
        );

        projection
            .markers
            .insert(2, marker(20, 2, Some(4.0), Some(5.0), Some(6.0)));
        projection
            .markers
            .insert(1, marker(10, 1, Some(1.0), Some(2.0), Some(3.0)));
        let snapshot = projection.preset_snapshot().unwrap();
        assert_eq!(
            snapshot
                .iter()
                .map(|marker| marker.marker_number)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );

        projection
            .markers
            .insert(2, marker(30, 2, Some(7.0), Some(8.0), Some(9.0)));
        let moved = projection.preset_snapshot().unwrap();
        assert_eq!(moved[1].passive_instance_id, 30);
        projection
            .markers
            .insert(3, marker(40, 3, Some(1.0), None, Some(3.0)));
        assert_eq!(
            projection.preset_snapshot(),
            Err(LocalMapMarkerSnapshotError::MissingCoordinate)
        );
        projection
            .markers
            .insert(3, marker(40, 3, Some(f32::NAN), Some(2.0), Some(3.0)));
        assert_eq!(
            projection.preset_snapshot(),
            Err(LocalMapMarkerSnapshotError::InvalidCoordinate)
        );
        projection.markers.remove(&3);
        projection
            .markers
            .insert(7, marker(40, 7, Some(1.0), Some(2.0), Some(3.0)));
        assert_eq!(
            projection.preset_snapshot(),
            Err(LocalMapMarkerSnapshotError::InvalidMarkerNumber)
        );
    }
}
