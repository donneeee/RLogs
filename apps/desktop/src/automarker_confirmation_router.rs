//! Pure, process-private routing for decoded Automarker confirmation evidence.
//!
//! The router intentionally owns no capture handle, socket, native driver,
//! coordinator, or serialization surface. It converts immutable parser
//! snapshots into owned intermediate events. A later bridge integration may
//! translate those events into `AutomarkerBridgeCoordinator` observations.

// This module is intentionally unregistered until the live bridge adapter is
// reviewed. Its integration-test path still compiles and exercises every
// contract below.
#![allow(dead_code)]

use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

const WORLD_NOTIFICATION_SERVICE_ID: u64 = 1_664_308_034;
const WORLD_SERVICE_ID: u64 = 103_198_054;
const WORLD_USE_SLOT_METHOD_ID: u32 = 249_858;
const WORLD_SYNC_TO_ME_DELTA_METHOD_ID: u32 = 46;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PrivatePacketDirection {
    ClientToServer,
    ServerToClient,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PrivateFragmentKind {
    Call,
    Return,
    Notify,
}

/// Exact immutable parser provenance. It remains inside this module's
/// process-private API and is deliberately not serializable.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct PrivateConfirmationProvenance {
    pub capture_sequence: u64,
    pub source_observed_micros: u64,
    pub source_wall_clock_unix_micros: Option<i64>,
    pub connection_id: u64,
    pub stream_id: u64,
    pub direction: PrivatePacketDirection,
    pub fragment: PrivateFragmentKind,
    pub service_id: u64,
    pub method_id: u32,
    pub stub_id: u32,
    pub call_id: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrivateConfirmationContext {
    pub game_build: String,
    pub scene_family: String,
    pub local_actor_id: i64,
    pub connection_epoch: u64,
    pub client_address: [u8; 4],
    pub client_port: u16,
    pub server_address: [u8; 4],
    pub server_port: u16,
    pub runtime_revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PrivateMarkerPosition {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrivateCorrelatedReturn {
    pub provenance: PrivateConfirmationProvenance,
    pub carrier_capture_sequence: u64,
    pub asserted_authoritative_server_decode: bool,
    pub decoded_as_success: bool,
    pub decoded_body_length: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PrivateMarkerAdd {
    pub provenance: PrivateConfirmationProvenance,
    pub asserted_authoritative_server_decode: bool,
    pub marker_number: u8,
    pub marker_owner_actor_id: i64,
    pub position: PrivateMarkerPosition,
    pub passive_instance_identity: i64,
    pub runtime_revision: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PrivateParserConfirmationEvent {
    CorrelatedReturn(PrivateCorrelatedReturn),
    MarkerAdd(PrivateMarkerAdd),
}

impl PrivateParserConfirmationEvent {
    fn provenance(&self) -> &PrivateConfirmationProvenance {
        match self {
            Self::CorrelatedReturn(event) => &event.provenance,
            Self::MarkerAdd(event) => &event.provenance,
        }
    }

    fn tie_breaker(&self) -> u8 {
        match self {
            Self::CorrelatedReturn(_) => 0,
            Self::MarkerAdd(_) => 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PrivateParserConfirmationSnapshot {
    /// Opaque session identity checked but never copied into routed output.
    pub session_key: String,
    pub context: PrivateConfirmationContext,
    pub events: Vec<PrivateParserConfirmationEvent>,
}

/// Router-owned ordering/timing domain. Source capture clocks never enter this
/// stamp, so unrelated capture and parser clock origins cannot reorder proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OwnedConfirmationStamp {
    pub observation_ordinal: u64,
    pub observed_micros: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnedConfirmationContext {
    pub game_build: String,
    pub scene_family: String,
    pub local_actor_id: i64,
    pub connection_epoch: u64,
    pub client_address: [u8; 4],
    pub client_port: u16,
    pub server_address: [u8; 4],
    pub server_port: u16,
    pub runtime_revision: u64,
    pub observed_micros: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnedSuccessfulRpcReturn {
    pub original_call_id: u32,
    pub decoded_body_length: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OwnedAuthoritativeMarkerAddCandidate {
    pub method_id: u32,
    pub marker_number: u8,
    pub marker_owner_actor_id: i64,
    pub position: PrivateMarkerPosition,
    pub passive_instance_identity: i64,
    pub asserted_authoritative_server_decode: bool,
    pub runtime_revision: u64,
    pub observed_micros: u64,
}

/// Safe owned intermediate event. It contains everything a later adapter
/// needs for the coordinator, but no connection ids, stream ids, capture
/// sequences, wall-clock values, session token, or raw bytes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum OwnedConfirmationEventKind {
    SuccessfulRpcReturn(OwnedSuccessfulRpcReturn),
    AuthoritativeMarkerAddCandidate(OwnedAuthoritativeMarkerAddCandidate),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OwnedConfirmationEvent {
    pub context: OwnedConfirmationContext,
    pub stamp: OwnedConfirmationStamp,
    pub kind: OwnedConfirmationEventKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfirmationRouterError {
    SessionChanged,
    ClockBeforeSession,
    ClockExhausted,
}

/// One router instance is one capture session. Construct a new instance on
/// every session transition; its deduplication and clock domains never carry
/// across sessions.
pub(crate) struct AutomarkerConfirmationRouter {
    session_key: String,
    target_marker_number: u8,
    origin: Instant,
    last_ordinal: u64,
    last_micros: u64,
    seen_provenance: BTreeSet<PrivateConfirmationProvenance>,
}

impl AutomarkerConfirmationRouter {
    pub(crate) fn begin(session_key: String, target_marker_number: u8) -> Option<Self> {
        Self::begin_at(session_key, target_marker_number, Instant::now())
    }

    fn begin_at(session_key: String, target_marker_number: u8, origin: Instant) -> Option<Self> {
        if session_key.trim().is_empty() || !(1..=6).contains(&target_marker_number) {
            return None;
        }
        Some(Self {
            session_key,
            target_marker_number,
            origin,
            last_ordinal: 0,
            last_micros: 0,
            seen_provenance: BTreeSet::new(),
        })
    }

    /// Allocate a stamp for baseline/rewrite integration from this same
    /// session clock. Routing uses the identical allocator.
    pub(crate) fn stamp_now(&mut self) -> Result<OwnedConfirmationStamp, ConfirmationRouterError> {
        self.stamp_at(Instant::now())
    }

    pub(crate) fn route_snapshot(
        &mut self,
        snapshot: PrivateParserConfirmationSnapshot,
    ) -> Result<Vec<OwnedConfirmationEvent>, ConfirmationRouterError> {
        self.route_snapshot_at(snapshot, Instant::now())
    }

    fn route_snapshot_at(
        &mut self,
        mut snapshot: PrivateParserConfirmationSnapshot,
        observed_at: Instant,
    ) -> Result<Vec<OwnedConfirmationEvent>, ConfirmationRouterError> {
        if snapshot.session_key != self.session_key {
            return Err(ConfirmationRouterError::SessionChanged);
        }

        // A parser snapshot may aggregate projections in decoder order. Proof
        // order is instead the immutable record capture sequence. The full
        // provenance is a deterministic tie-breaker, never an output field.
        snapshot.events.sort_by(|left, right| {
            left.provenance()
                .capture_sequence
                .cmp(&right.provenance().capture_sequence)
                .then_with(|| left.provenance().cmp(right.provenance()))
                .then_with(|| left.tie_breaker().cmp(&right.tie_breaker()))
        });

        let mut routed = Vec::new();
        for event in snapshot.events {
            let provenance = event.provenance().clone();
            if !self.is_relevant(&event) || !self.seen_provenance.insert(provenance) {
                continue;
            }
            let kind = match event {
                PrivateParserConfirmationEvent::CorrelatedReturn(event) => {
                    // Only an authoritative, successful, empty Return already
                    // correlated to a concrete carrier can become proof input.
                    if !event.asserted_authoritative_server_decode
                        || !event.decoded_as_success
                        || event.decoded_body_length != 0
                        || event.carrier_capture_sequence == 0
                    {
                        continue;
                    }
                    let Some(call_id) = event.provenance.call_id.filter(|id| *id != 0) else {
                        continue;
                    };
                    OwnedConfirmationEventKind::SuccessfulRpcReturn(OwnedSuccessfulRpcReturn {
                        original_call_id: call_id,
                        decoded_body_length: event.decoded_body_length,
                    })
                }
                PrivateParserConfirmationEvent::MarkerAdd(event) => {
                    // Do not pre-validate owner, XYZ, instance freshness, or
                    // authoritative assertion here. Forward the exact target-
                    // number candidate so the coordinator can fail closed.
                    OwnedConfirmationEventKind::AuthoritativeMarkerAddCandidate(
                        OwnedAuthoritativeMarkerAddCandidate {
                            method_id: event.provenance.method_id,
                            marker_number: event.marker_number,
                            marker_owner_actor_id: event.marker_owner_actor_id,
                            position: event.position,
                            passive_instance_identity: event.passive_instance_identity,
                            asserted_authoritative_server_decode: event
                                .asserted_authoritative_server_decode,
                            runtime_revision: event.runtime_revision,
                            observed_micros: 0,
                        },
                    )
                }
            };
            let stamp = self.stamp_at(observed_at)?;
            let (context_runtime_revision, kind) = match kind {
                OwnedConfirmationEventKind::AuthoritativeMarkerAddCandidate(mut marker) => {
                    marker.observed_micros = stamp.observed_micros;
                    (
                        marker.runtime_revision,
                        OwnedConfirmationEventKind::AuthoritativeMarkerAddCandidate(marker),
                    )
                }
                other => (snapshot.context.runtime_revision, other),
            };
            let context = owned_context(
                &snapshot.context,
                context_runtime_revision,
                stamp.observed_micros,
            );
            routed.push(OwnedConfirmationEvent {
                context,
                stamp,
                kind,
            });
        }
        Ok(routed)
    }

    fn is_relevant(&self, event: &PrivateParserConfirmationEvent) -> bool {
        let provenance = event.provenance();
        match event {
            PrivateParserConfirmationEvent::CorrelatedReturn(_) => {
                provenance.direction == PrivatePacketDirection::ServerToClient
                    && provenance.fragment == PrivateFragmentKind::Return
                    && provenance.service_id == WORLD_SERVICE_ID
                    && provenance.method_id == WORLD_USE_SLOT_METHOD_ID
            }
            PrivateParserConfirmationEvent::MarkerAdd(marker) => {
                provenance.direction == PrivatePacketDirection::ServerToClient
                    && provenance.fragment == PrivateFragmentKind::Notify
                    && provenance.service_id == WORLD_NOTIFICATION_SERVICE_ID
                    && provenance.method_id == WORLD_SYNC_TO_ME_DELTA_METHOD_ID
                    && marker.marker_number == self.target_marker_number
            }
        }
    }

    fn stamp_at(
        &mut self,
        observed_at: Instant,
    ) -> Result<OwnedConfirmationStamp, ConfirmationRouterError> {
        let elapsed = observed_at
            .checked_duration_since(self.origin)
            .ok_or(ConfirmationRouterError::ClockBeforeSession)?;
        let elapsed_micros = duration_micros_saturating(elapsed);
        let observation_ordinal = self
            .last_ordinal
            .checked_add(1)
            .ok_or(ConfirmationRouterError::ClockExhausted)?;
        let observed_micros = elapsed_micros
            .max(self.last_micros.saturating_add(1))
            .max(1);
        if observed_micros == u64::MAX && self.last_micros == u64::MAX {
            return Err(ConfirmationRouterError::ClockExhausted);
        }
        self.last_ordinal = observation_ordinal;
        self.last_micros = observed_micros;
        Ok(OwnedConfirmationStamp {
            observation_ordinal,
            observed_micros,
        })
    }
}

fn duration_micros_saturating(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

fn owned_context(
    source: &PrivateConfirmationContext,
    runtime_revision: u64,
    observed_micros: u64,
) -> OwnedConfirmationContext {
    OwnedConfirmationContext {
        game_build: source.game_build.clone(),
        scene_family: source.scene_family.clone(),
        local_actor_id: source.local_actor_id,
        connection_epoch: source.connection_epoch,
        client_address: source.client_address,
        client_port: source.client_port,
        server_address: source.server_address,
        server_port: source.server_port,
        runtime_revision,
        observed_micros,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> PrivateConfirmationContext {
        PrivateConfirmationContext {
            game_build: "25247556".into(),
            scene_family: "mech-facility".into(),
            local_actor_id: 44,
            connection_epoch: 7,
            client_address: [10, 0, 0, 2],
            client_port: 50_000,
            server_address: [10, 0, 0, 3],
            server_port: 443,
            runtime_revision: 11,
        }
    }

    fn provenance(
        sequence: u64,
        fragment: PrivateFragmentKind,
        service_id: u64,
        method_id: u32,
        call_id: Option<u32>,
        source_clock: u64,
    ) -> PrivateConfirmationProvenance {
        PrivateConfirmationProvenance {
            capture_sequence: sequence,
            source_observed_micros: source_clock,
            source_wall_clock_unix_micros: Some(1_800_000_000_000_000),
            connection_id: 17,
            stream_id: 3,
            direction: PrivatePacketDirection::ServerToClient,
            fragment,
            service_id,
            method_id,
            stub_id: 99,
            call_id,
        }
    }

    fn successful_return(sequence: u64, source_clock: u64) -> PrivateParserConfirmationEvent {
        PrivateParserConfirmationEvent::CorrelatedReturn(PrivateCorrelatedReturn {
            provenance: provenance(
                sequence,
                PrivateFragmentKind::Return,
                WORLD_SERVICE_ID,
                WORLD_USE_SLOT_METHOD_ID,
                Some(91),
                source_clock,
            ),
            carrier_capture_sequence: 8,
            asserted_authoritative_server_decode: true,
            decoded_as_success: true,
            decoded_body_length: 0,
        })
    }

    fn marker(sequence: u64, number: u8, source_clock: u64) -> PrivateParserConfirmationEvent {
        PrivateParserConfirmationEvent::MarkerAdd(PrivateMarkerAdd {
            provenance: provenance(
                sequence,
                PrivateFragmentKind::Notify,
                WORLD_NOTIFICATION_SERVICE_ID,
                WORLD_SYNC_TO_ME_DELTA_METHOD_ID,
                None,
                source_clock,
            ),
            asserted_authoritative_server_decode: true,
            marker_number: number,
            marker_owner_actor_id: 44,
            position: PrivateMarkerPosition {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            },
            passive_instance_identity: 123,
            runtime_revision: 12,
        })
    }

    fn snapshot(events: Vec<PrivateParserConfirmationEvent>) -> PrivateParserConfirmationSnapshot {
        PrivateParserConfirmationSnapshot {
            session_key: "private-session".into(),
            context: context(),
            events,
        }
    }

    #[test]
    fn replayed_snapshot_is_deduplicated_by_exact_provenance() {
        let origin = Instant::now();
        let mut router =
            AutomarkerConfirmationRouter::begin_at("private-session".into(), 1, origin).unwrap();
        let event = successful_return(10, 999);
        assert_eq!(
            router
                .route_snapshot_at(
                    snapshot(vec![event.clone()]),
                    origin + Duration::from_millis(1)
                )
                .unwrap()
                .len(),
            1
        );
        assert!(
            router
                .route_snapshot_at(snapshot(vec![event]), origin + Duration::from_millis(2))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn semantically_duplicate_return_with_distinct_provenance_is_forwarded() {
        let origin = Instant::now();
        let mut router =
            AutomarkerConfirmationRouter::begin_at("private-session".into(), 1, origin).unwrap();
        let routed = router
            .route_snapshot_at(
                snapshot(vec![successful_return(10, 5), successful_return(11, 5)]),
                origin + Duration::from_millis(1),
            )
            .unwrap();
        assert_eq!(routed.len(), 2);
        assert!(routed.iter().all(|event| matches!(
            event.kind,
            OwnedConfirmationEventKind::SuccessfulRpcReturn(OwnedSuccessfulRpcReturn {
                original_call_id: 91,
                decoded_body_length: 0
            })
        )));
    }

    #[test]
    fn provenance_deduplication_is_exact_not_capture_sequence_only() {
        let origin = Instant::now();
        let mut router =
            AutomarkerConfirmationRouter::begin_at("private-session".into(), 1, origin).unwrap();
        let first = successful_return(10, 5);
        let mut second = first.clone();
        let PrivateParserConfirmationEvent::CorrelatedReturn(second_return) = &mut second else {
            unreachable!();
        };
        second_return.provenance.source_wall_clock_unix_micros = Some(1_800_000_000_000_001);
        let routed = router
            .route_snapshot_at(
                snapshot(vec![first, second]),
                origin + Duration::from_millis(1),
            )
            .unwrap();
        assert_eq!(routed.len(), 2);
    }

    #[test]
    fn newly_seen_mixed_events_are_sorted_by_capture_sequence() {
        let origin = Instant::now();
        let mut router =
            AutomarkerConfirmationRouter::begin_at("private-session".into(), 1, origin).unwrap();
        let routed = router
            .route_snapshot_at(
                snapshot(vec![marker(20, 1, 1), successful_return(10, 2)]),
                origin + Duration::from_micros(10),
            )
            .unwrap();
        assert!(matches!(
            routed[0].kind,
            OwnedConfirmationEventKind::SuccessfulRpcReturn(_)
        ));
        assert!(matches!(
            routed[1].kind,
            OwnedConfirmationEventKind::AuthoritativeMarkerAddCandidate(_)
        ));
        assert!(routed[0].stamp.observation_ordinal < routed[1].stamp.observation_ordinal);
    }

    #[test]
    fn source_clocks_do_not_control_router_clock() {
        let origin = Instant::now();
        let mut router =
            AutomarkerConfirmationRouter::begin_at("private-session".into(), 1, origin).unwrap();
        let routed = router
            .route_snapshot_at(
                snapshot(vec![successful_return(10, u64::MAX), marker(11, 1, 1)]),
                origin + Duration::from_micros(50),
            )
            .unwrap();
        assert_eq!(routed[0].stamp.observed_micros, 50);
        assert_eq!(routed[1].stamp.observed_micros, 51);
        assert_eq!(routed[0].context.observed_micros, 50);
        let OwnedConfirmationEventKind::AuthoritativeMarkerAddCandidate(add) = &routed[1].kind
        else {
            panic!("expected marker candidate");
        };
        assert_eq!(add.observed_micros, 51);
        assert_eq!(routed[1].context.runtime_revision, add.runtime_revision);
    }

    #[test]
    fn stamps_are_strictly_monotonic_even_at_the_same_instant() {
        let origin = Instant::now();
        let mut router =
            AutomarkerConfirmationRouter::begin_at("private-session".into(), 1, origin).unwrap();
        let first = router.stamp_at(origin).unwrap();
        let second = router.stamp_at(origin).unwrap();
        let third = router.stamp_at(origin + Duration::from_micros(1)).unwrap();
        assert_eq!(first.observation_ordinal, 1);
        assert_eq!(first.observed_micros, 1);
        assert_eq!(second.observation_ordinal, 2);
        assert_eq!(second.observed_micros, 2);
        assert_eq!(third.observation_ordinal, 3);
        assert_eq!(third.observed_micros, 3);
    }

    #[test]
    fn target_marker_candidates_preserve_mismatches_for_fail_closed_coordinator() {
        let origin = Instant::now();
        let mut router =
            AutomarkerConfirmationRouter::begin_at("private-session".into(), 1, origin).unwrap();
        let mut candidate = match marker(10, 1, 4) {
            PrivateParserConfirmationEvent::MarkerAdd(candidate) => candidate,
            _ => unreachable!(),
        };
        candidate.asserted_authoritative_server_decode = false;
        candidate.marker_owner_actor_id = 999;
        candidate.position.x = f32::NAN;
        candidate.passive_instance_identity = 0;
        let routed = router
            .route_snapshot_at(
                snapshot(vec![PrivateParserConfirmationEvent::MarkerAdd(candidate)]),
                origin + Duration::from_micros(5),
            )
            .unwrap();
        let OwnedConfirmationEventKind::AuthoritativeMarkerAddCandidate(candidate) =
            &routed[0].kind
        else {
            panic!("expected marker candidate");
        };
        assert!(!candidate.asserted_authoritative_server_decode);
        assert_eq!(candidate.marker_owner_actor_id, 999);
        assert!(candidate.position.x.is_nan());
        assert_eq!(candidate.passive_instance_identity, 0);
    }

    #[test]
    fn unrelated_marker_numbers_and_routes_are_ignored() {
        let origin = Instant::now();
        let mut router =
            AutomarkerConfirmationRouter::begin_at("private-session".into(), 1, origin).unwrap();
        let mut wrong_route = marker(10, 1, 1);
        match &mut wrong_route {
            PrivateParserConfirmationEvent::MarkerAdd(event) => {
                event.provenance.method_id = 45;
            }
            _ => unreachable!(),
        }
        assert!(
            router
                .route_snapshot_at(
                    snapshot(vec![wrong_route, marker(11, 2, 2)]),
                    origin + Duration::from_micros(5),
                )
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn unsuccessful_uncorrelated_or_nonempty_returns_are_ignored() {
        let origin = Instant::now();
        let mut router =
            AutomarkerConfirmationRouter::begin_at("private-session".into(), 1, origin).unwrap();
        let mut events = Vec::new();
        for sequence in 1..=4 {
            let PrivateParserConfirmationEvent::CorrelatedReturn(mut event) =
                successful_return(sequence, sequence)
            else {
                unreachable!();
            };
            match sequence {
                1 => event.carrier_capture_sequence = 0,
                2 => event.asserted_authoritative_server_decode = false,
                3 => event.decoded_as_success = false,
                4 => event.decoded_body_length = 1,
                _ => unreachable!(),
            }
            events.push(PrivateParserConfirmationEvent::CorrelatedReturn(event));
        }
        assert!(
            router
                .route_snapshot_at(snapshot(events), origin + Duration::from_micros(5))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn session_mismatch_fails_without_consuming_evidence() {
        let origin = Instant::now();
        let mut router =
            AutomarkerConfirmationRouter::begin_at("private-session".into(), 1, origin).unwrap();
        let event = successful_return(10, 1);
        let mut wrong = snapshot(vec![event.clone()]);
        wrong.session_key = "other-session".into();
        assert_eq!(
            router.route_snapshot_at(wrong, origin + Duration::from_micros(1)),
            Err(ConfirmationRouterError::SessionChanged)
        );
        assert_eq!(
            router
                .route_snapshot_at(snapshot(vec![event]), origin + Duration::from_micros(2))
                .unwrap()
                .len(),
            1
        );
    }
}
