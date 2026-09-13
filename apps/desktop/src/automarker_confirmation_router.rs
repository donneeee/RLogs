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
    collections::BTreeMap,
    time::{Duration, Instant},
};

const WORLD_NOTIFICATION_SERVICE_ID: u64 = 1_664_308_034;
const WORLD_SERVICE_ID: u64 = 103_198_054;
const WORLD_USE_SLOT_METHOD_ID: u32 = 249_858;
const WORLD_SYNC_TO_ME_DELTA_METHOD_ID: u32 = 46;
/// A session containing more Automarker confirmation candidates than this is
/// anomalous. Failing closed bounds private replay memory without evicting a
/// key that could later make a replay look new.
const MAX_SESSION_PROVENANCE_KEYS: usize = 4_096;

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
    /// Stable position inside one decoded capture record. This, rather than a
    /// source timestamp, orders multiple confirmation projections in a record.
    pub record_event_index: u32,
    pub connection_id: u64,
    pub stream_id: u64,
    pub direction: PrivatePacketDirection,
    pub fragment: PrivateFragmentKind,
    pub service_id: u64,
    pub method_id: u32,
    pub stub_id: u32,
    pub call_id: Option<u32>,
}

/// Diagnostic source clocks are retained on the private input only. They are
/// deliberately absent from provenance identity, sorting, and routed output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PrivateSourceClocks {
    pub observed_micros: u64,
    pub wall_clock_unix_micros: Option<i64>,
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
    pub source_clocks: PrivateSourceClocks,
    pub carrier_capture_sequence: u64,
    pub asserted_authoritative_server_decode: bool,
    pub decoded_as_success: bool,
    pub decoded_body_length: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PrivateMarkerAdd {
    pub provenance: PrivateConfirmationProvenance,
    pub source_clocks: PrivateSourceClocks,
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

    #[cfg(test)]
    fn provenance_mut(&mut self) -> &mut PrivateConfirmationProvenance {
        match self {
            Self::CorrelatedReturn(event) => &mut event.provenance,
            Self::MarkerAdd(event) => &mut event.provenance,
        }
    }

    fn source_kind_order(&self) -> u8 {
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
pub(crate) struct OwnedRpcReturnCandidate {
    pub method_id: u32,
    pub original_call_id: u32,
    pub asserted_authoritative_server_decode: bool,
    pub decoded_as_success: bool,
    pub decoded_body_length: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OwnedMarkerAddCandidate {
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
    RpcReturnCandidate(OwnedRpcReturnCandidate),
    MarkerAddCandidate(OwnedMarkerAddCandidate),
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
    LateUnseenEvidence,
    ConflictingProvenance,
    SessionEvidenceLimitExceeded,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CaptureOrderKey {
    capture_sequence: u64,
    record_event_index: u32,
    source_kind_order: u8,
    direction: PrivatePacketDirection,
    fragment: PrivateFragmentKind,
    service_id: u64,
    method_id: u32,
    stub_id: u32,
    call_id: Option<u32>,
    connection_id: u64,
    stream_id: u64,
}

/// One router instance is one capture session. Construct a new instance on
/// every session transition; its deduplication and clock domains never carry
/// across sessions.
pub(crate) struct AutomarkerConfirmationRouter {
    session_key: String,
    origin: Instant,
    last_ordinal: u64,
    last_micros: u64,
    capture_frontier: Option<CaptureOrderKey>,
    seen_events: BTreeMap<PrivateConfirmationProvenance, PrivateParserConfirmationEvent>,
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
            origin,
            last_ordinal: 0,
            last_micros: 0,
            capture_frontier: None,
            seen_events: BTreeMap::new(),
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

        // Only clone private state. Nothing below becomes authoritative until
        // the entire batch (including every clock allocation) succeeds.
        let mut next_seen = self.seen_events.clone();
        let mut next_frontier = self.capture_frontier.clone();
        let mut next_ordinal = self.last_ordinal;
        let mut next_micros = self.last_micros;

        // Decoder collection order is not proof order. Capture identity,
        // record-local event index, source kind, and stable route provenance
        // form a total tie-break independent of source clocks.
        snapshot.events.sort_by_key(capture_order_key);
        let mut routed = Vec::new();
        for event in snapshot.events {
            if !self.is_related_target_route(&event) {
                continue;
            }
            let provenance = event.provenance().clone();
            let event_for_history = event.clone();
            if let Some(previous) = next_seen.get(&provenance) {
                if same_event_payload(previous, &event) {
                    continue;
                }
                return Err(ConfirmationRouterError::ConflictingProvenance);
            }
            let order_key = capture_order_key(&event);
            if next_frontier
                .as_ref()
                .is_some_and(|frontier| order_key <= *frontier)
            {
                return Err(ConfirmationRouterError::LateUnseenEvidence);
            }
            if next_seen.len() >= MAX_SESSION_PROVENANCE_KEYS {
                return Err(ConfirmationRouterError::SessionEvidenceLimitExceeded);
            }
            let kind = match event {
                PrivateParserConfirmationEvent::CorrelatedReturn(event) => {
                    // Correlation is a router admission invariant. Once it is
                    // present, every target-route failure field is forwarded
                    // so the coordinator—not this router—fails closed.
                    if event.carrier_capture_sequence == 0 {
                        continue;
                    }
                    OwnedConfirmationEventKind::RpcReturnCandidate(OwnedRpcReturnCandidate {
                        method_id: event.provenance.method_id,
                        original_call_id: event.provenance.call_id.unwrap_or(0),
                        asserted_authoritative_server_decode: event
                            .asserted_authoritative_server_decode,
                        decoded_as_success: event.decoded_as_success,
                        decoded_body_length: event.decoded_body_length,
                    })
                }
                PrivateParserConfirmationEvent::MarkerAdd(event) => {
                    // Do not pre-validate method, marker number, owner, XYZ,
                    // instance freshness, revision, or authoritative assertion.
                    OwnedConfirmationEventKind::MarkerAddCandidate(OwnedMarkerAddCandidate {
                        method_id: event.provenance.method_id,
                        marker_number: event.marker_number,
                        marker_owner_actor_id: event.marker_owner_actor_id,
                        position: event.position,
                        passive_instance_identity: event.passive_instance_identity,
                        asserted_authoritative_server_decode: event
                            .asserted_authoritative_server_decode,
                        runtime_revision: event.runtime_revision,
                        observed_micros: 0,
                    })
                }
            };
            let stamp = next_stamp(
                self.origin,
                observed_at,
                &mut next_ordinal,
                &mut next_micros,
            )?;
            let kind = match kind {
                OwnedConfirmationEventKind::MarkerAddCandidate(mut marker) => {
                    marker.observed_micros = stamp.observed_micros;
                    OwnedConfirmationEventKind::MarkerAddCandidate(marker)
                }
                other => other,
            };
            let context = owned_context(&snapshot.context, stamp.observed_micros);
            routed.push(OwnedConfirmationEvent {
                context,
                stamp,
                kind,
            });
            next_seen.insert(provenance, event_for_history);
            next_frontier = Some(order_key);
        }
        self.seen_events = next_seen;
        self.capture_frontier = next_frontier;
        self.last_ordinal = next_ordinal;
        self.last_micros = next_micros;
        Ok(routed)
    }

    fn is_related_target_route(&self, event: &PrivateParserConfirmationEvent) -> bool {
        let provenance = event.provenance();
        match event {
            PrivateParserConfirmationEvent::CorrelatedReturn(event) => {
                provenance.direction == PrivatePacketDirection::ServerToClient
                    && provenance.fragment == PrivateFragmentKind::Return
                    && provenance.service_id == WORLD_SERVICE_ID
                    && event.carrier_capture_sequence != 0
            }
            PrivateParserConfirmationEvent::MarkerAdd(_) => {
                provenance.direction == PrivatePacketDirection::ServerToClient
                    && provenance.fragment == PrivateFragmentKind::Notify
                    && provenance.service_id == WORLD_NOTIFICATION_SERVICE_ID
            }
        }
    }

    fn stamp_at(
        &mut self,
        observed_at: Instant,
    ) -> Result<OwnedConfirmationStamp, ConfirmationRouterError> {
        next_stamp(
            self.origin,
            observed_at,
            &mut self.last_ordinal,
            &mut self.last_micros,
        )
    }
}

fn next_stamp(
    origin: Instant,
    observed_at: Instant,
    last_ordinal: &mut u64,
    last_micros: &mut u64,
) -> Result<OwnedConfirmationStamp, ConfirmationRouterError> {
    let elapsed = observed_at
        .checked_duration_since(origin)
        .ok_or(ConfirmationRouterError::ClockBeforeSession)?;
    let elapsed_micros = duration_micros_saturating(elapsed);
    let observation_ordinal = last_ordinal
        .checked_add(1)
        .ok_or(ConfirmationRouterError::ClockExhausted)?;
    let Some(incremented_micros) = last_micros.checked_add(1) else {
        return Err(ConfirmationRouterError::ClockExhausted);
    };
    let observed_micros = elapsed_micros.max(incremented_micros).max(1);
    *last_ordinal = observation_ordinal;
    *last_micros = observed_micros;
    Ok(OwnedConfirmationStamp {
        observation_ordinal,
        observed_micros,
    })
}

fn duration_micros_saturating(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

fn capture_order_key(event: &PrivateParserConfirmationEvent) -> CaptureOrderKey {
    let provenance = event.provenance();
    CaptureOrderKey {
        capture_sequence: provenance.capture_sequence,
        record_event_index: provenance.record_event_index,
        source_kind_order: event.source_kind_order(),
        direction: provenance.direction,
        fragment: provenance.fragment,
        service_id: provenance.service_id,
        method_id: provenance.method_id,
        stub_id: provenance.stub_id,
        call_id: provenance.call_id,
        connection_id: provenance.connection_id,
        stream_id: provenance.stream_id,
    }
}

/// Parser clock metadata may legitimately differ between two projections of
/// the same immutable record. It therefore participates in neither identity
/// nor conflict detection.
fn same_event_payload(
    left: &PrivateParserConfirmationEvent,
    right: &PrivateParserConfirmationEvent,
) -> bool {
    match (left, right) {
        (
            PrivateParserConfirmationEvent::CorrelatedReturn(left),
            PrivateParserConfirmationEvent::CorrelatedReturn(right),
        ) => {
            left.provenance == right.provenance
                && left.carrier_capture_sequence == right.carrier_capture_sequence
                && left.asserted_authoritative_server_decode
                    == right.asserted_authoritative_server_decode
                && left.decoded_as_success == right.decoded_as_success
                && left.decoded_body_length == right.decoded_body_length
        }
        (
            PrivateParserConfirmationEvent::MarkerAdd(left),
            PrivateParserConfirmationEvent::MarkerAdd(right),
        ) => {
            left.provenance == right.provenance
                && left.asserted_authoritative_server_decode
                    == right.asserted_authoritative_server_decode
                && left.marker_number == right.marker_number
                && left.marker_owner_actor_id == right.marker_owner_actor_id
                && left.position.x.to_bits() == right.position.x.to_bits()
                && left.position.y.to_bits() == right.position.y.to_bits()
                && left.position.z.to_bits() == right.position.z.to_bits()
                && left.passive_instance_identity == right.passive_instance_identity
                && left.runtime_revision == right.runtime_revision
        }
        _ => false,
    }
}

fn owned_context(
    source: &PrivateConfirmationContext,
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
        runtime_revision: source.runtime_revision,
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
        record_event_index: u32,
        fragment: PrivateFragmentKind,
        service_id: u64,
        method_id: u32,
        call_id: Option<u32>,
    ) -> PrivateConfirmationProvenance {
        PrivateConfirmationProvenance {
            capture_sequence: sequence,
            record_event_index,
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
                0,
                PrivateFragmentKind::Return,
                WORLD_SERVICE_ID,
                WORLD_USE_SLOT_METHOD_ID,
                Some(91),
            ),
            source_clocks: PrivateSourceClocks {
                observed_micros: source_clock,
                wall_clock_unix_micros: Some(1_800_000_000_000_000),
            },
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
                0,
                PrivateFragmentKind::Notify,
                WORLD_NOTIFICATION_SERVICE_ID,
                WORLD_SYNC_TO_ME_DELTA_METHOD_ID,
                None,
            ),
            source_clocks: PrivateSourceClocks {
                observed_micros: source_clock,
                wall_clock_unix_micros: Some(1_800_000_000_000_000),
            },
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
            OwnedConfirmationEventKind::RpcReturnCandidate(OwnedRpcReturnCandidate {
                method_id: WORLD_USE_SLOT_METHOD_ID,
                original_call_id: 91,
                asserted_authoritative_server_decode: true,
                decoded_as_success: true,
                decoded_body_length: 0
            })
        )));
    }

    #[test]
    fn source_clock_variation_does_not_make_replay_new() {
        let origin = Instant::now();
        let mut router =
            AutomarkerConfirmationRouter::begin_at("private-session".into(), 1, origin).unwrap();
        let first = successful_return(10, 5);
        let mut second = first.clone();
        let PrivateParserConfirmationEvent::CorrelatedReturn(second_return) = &mut second else {
            unreachable!();
        };
        second_return.source_clocks.wall_clock_unix_micros = Some(1_800_000_000_000_001);
        second_return.source_clocks.observed_micros = u64::MAX;
        let routed = router
            .route_snapshot_at(
                snapshot(vec![first, second]),
                origin + Duration::from_millis(1),
            )
            .unwrap();
        assert_eq!(routed.len(), 1);
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
            OwnedConfirmationEventKind::RpcReturnCandidate(_)
        ));
        assert!(matches!(
            routed[1].kind,
            OwnedConfirmationEventKind::MarkerAddCandidate(_)
        ));
        assert!(routed[0].stamp.observation_ordinal < routed[1].stamp.observation_ordinal);
    }

    #[test]
    fn capture_sequence_ties_use_record_index_then_source_kind_not_clocks() {
        let origin = Instant::now();
        let mut router =
            AutomarkerConfirmationRouter::begin_at("private-session".into(), 1, origin).unwrap();
        let mut returned = successful_return(10, u64::MAX);
        let mut added = marker(10, 1, 0);
        // The same record index deliberately exercises the stable source-kind
        // tie. Input arrives in the opposite order.
        returned.provenance_mut().record_event_index = 4;
        added.provenance_mut().record_event_index = 4;
        let routed = router
            .route_snapshot_at(
                snapshot(vec![added, returned]),
                origin + Duration::from_micros(10),
            )
            .unwrap();
        assert!(matches!(
            routed[0].kind,
            OwnedConfirmationEventKind::RpcReturnCandidate(_)
        ));
        assert!(matches!(
            routed[1].kind,
            OwnedConfirmationEventKind::MarkerAddCandidate(_)
        ));
    }

    #[test]
    fn unseen_evidence_behind_session_capture_frontier_fails_closed() {
        let origin = Instant::now();
        let mut router =
            AutomarkerConfirmationRouter::begin_at("private-session".into(), 1, origin).unwrap();
        assert_eq!(
            router
                .route_snapshot_at(
                    snapshot(vec![successful_return(20, 1)]),
                    origin + Duration::from_micros(1),
                )
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            router.route_snapshot_at(
                snapshot(vec![successful_return(10, 2)]),
                origin + Duration::from_micros(2),
            ),
            Err(ConfirmationRouterError::LateUnseenEvidence)
        );
        // An exact replay at the frontier remains an ordinary no-op.
        assert!(
            router
                .route_snapshot_at(
                    snapshot(vec![successful_return(20, 999)]),
                    origin + Duration::from_micros(3),
                )
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn unseen_lower_record_index_at_capture_tie_fails_closed() {
        let origin = Instant::now();
        let mut router =
            AutomarkerConfirmationRouter::begin_at("private-session".into(), 1, origin).unwrap();
        let mut later = successful_return(10, 1);
        later.provenance_mut().record_event_index = 2;
        router
            .route_snapshot_at(snapshot(vec![later]), origin + Duration::from_micros(1))
            .unwrap();
        let mut earlier = marker(10, 1, 2);
        earlier.provenance_mut().record_event_index = 1;
        assert_eq!(
            router.route_snapshot_at(snapshot(vec![earlier]), origin + Duration::from_micros(2),),
            Err(ConfirmationRouterError::LateUnseenEvidence)
        );
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
        let OwnedConfirmationEventKind::MarkerAddCandidate(add) = &routed[1].kind else {
            panic!("expected marker candidate");
        };
        assert_eq!(add.observed_micros, 51);
        assert_eq!(routed[1].context.runtime_revision, 11);
        assert_eq!(add.runtime_revision, 12);
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
        candidate.provenance.method_id = 45;
        candidate.marker_number = 2;
        candidate.marker_owner_actor_id = 999;
        candidate.position.x = f32::NAN;
        candidate.passive_instance_identity = 0;
        let routed = router
            .route_snapshot_at(
                snapshot(vec![PrivateParserConfirmationEvent::MarkerAdd(candidate)]),
                origin + Duration::from_micros(5),
            )
            .unwrap();
        let OwnedConfirmationEventKind::MarkerAddCandidate(candidate) = &routed[0].kind else {
            panic!("expected marker candidate");
        };
        assert!(!candidate.asserted_authoritative_server_decode);
        assert_eq!(candidate.method_id, 45);
        assert_eq!(candidate.marker_number, 2);
        assert_eq!(candidate.marker_owner_actor_id, 999);
        assert!(candidate.position.x.is_nan());
        assert_eq!(candidate.passive_instance_identity, 0);
    }

    #[test]
    fn only_truly_unrelated_routes_are_ignored() {
        let origin = Instant::now();
        let mut router =
            AutomarkerConfirmationRouter::begin_at("private-session".into(), 1, origin).unwrap();
        let mut wrong_direction = marker(10, 1, 1);
        match &mut wrong_direction {
            PrivateParserConfirmationEvent::MarkerAdd(event) => {
                event.provenance.direction = PrivatePacketDirection::ClientToServer;
            }
            _ => unreachable!(),
        }
        let mut wrong_service = marker(11, 1, 2);
        match &mut wrong_service {
            PrivateParserConfirmationEvent::MarkerAdd(event) => event.provenance.service_id = 7,
            _ => unreachable!(),
        }
        assert!(
            router
                .route_snapshot_at(
                    snapshot(vec![wrong_direction, wrong_service]),
                    origin + Duration::from_micros(5),
                )
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn target_return_failure_fields_are_forwarded_but_uncorrelated_is_ignored() {
        let origin = Instant::now();
        let mut router =
            AutomarkerConfirmationRouter::begin_at("private-session".into(), 1, origin).unwrap();
        let mut events = Vec::new();
        for sequence in 1..=6 {
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
                5 => event.provenance.call_id = Some(999),
                6 => event.provenance.method_id = 777,
                _ => unreachable!(),
            }
            events.push(PrivateParserConfirmationEvent::CorrelatedReturn(event));
        }
        let routed = router
            .route_snapshot_at(snapshot(events), origin + Duration::from_micros(5))
            .unwrap();
        assert_eq!(routed.len(), 5);
        let returns = routed
            .iter()
            .map(|event| match &event.kind {
                OwnedConfirmationEventKind::RpcReturnCandidate(candidate) => candidate,
                _ => panic!("expected Return candidate"),
            })
            .collect::<Vec<_>>();
        assert!(!returns[0].asserted_authoritative_server_decode);
        assert!(!returns[1].decoded_as_success);
        assert_eq!(returns[2].decoded_body_length, 1);
        assert_eq!(returns[3].original_call_id, 999);
        assert_eq!(returns[4].method_id, 777);
    }

    #[test]
    fn marker_and_context_runtime_revisions_remain_independent() {
        let origin = Instant::now();
        let mut router =
            AutomarkerConfirmationRouter::begin_at("private-session".into(), 1, origin).unwrap();
        let routed = router
            .route_snapshot_at(
                snapshot(vec![marker(10, 1, 1)]),
                origin + Duration::from_micros(1),
            )
            .unwrap();
        let OwnedConfirmationEventKind::MarkerAddCandidate(marker) = &routed[0].kind else {
            panic!("expected marker candidate");
        };
        assert_eq!(routed[0].context.runtime_revision, 11);
        assert_eq!(marker.runtime_revision, 12);
    }

    #[test]
    fn batch_is_atomic_when_a_later_stamp_exhausts_the_clock() {
        let origin = Instant::now();
        let mut router =
            AutomarkerConfirmationRouter::begin_at("private-session".into(), 1, origin).unwrap();
        router.last_ordinal = u64::MAX - 1;
        let before_seen = router.seen_events.clone();
        let before_frontier = router.capture_frontier.clone();
        let before_ordinal = router.last_ordinal;
        let before_micros = router.last_micros;
        assert_eq!(
            router.route_snapshot_at(
                snapshot(vec![successful_return(10, 1), marker(11, 1, 2)]),
                origin + Duration::from_micros(5),
            ),
            Err(ConfirmationRouterError::ClockExhausted)
        );
        assert_eq!(router.seen_events, before_seen);
        assert_eq!(router.capture_frontier, before_frontier);
        assert_eq!(router.last_ordinal, before_ordinal);
        assert_eq!(router.last_micros, before_micros);
    }

    #[test]
    fn conflicting_payload_for_seen_provenance_fails_closed_transactionally() {
        let origin = Instant::now();
        let mut router =
            AutomarkerConfirmationRouter::begin_at("private-session".into(), 1, origin).unwrap();
        let original = marker(10, 1, 1);
        router
            .route_snapshot_at(
                snapshot(vec![original.clone()]),
                origin + Duration::from_micros(1),
            )
            .unwrap();
        let mut conflicting = original;
        let PrivateParserConfirmationEvent::MarkerAdd(conflicting_marker) = &mut conflicting else {
            unreachable!();
        };
        conflicting_marker.marker_owner_actor_id = 999;
        let before_seen = router.seen_events.clone();
        let before_frontier = router.capture_frontier.clone();
        assert_eq!(
            router.route_snapshot_at(
                snapshot(vec![conflicting]),
                origin + Duration::from_micros(2),
            ),
            Err(ConfirmationRouterError::ConflictingProvenance)
        );
        assert_eq!(router.seen_events, before_seen);
        assert_eq!(router.capture_frontier, before_frontier);
    }

    #[test]
    fn hard_session_evidence_limit_fails_without_eviction() {
        let origin = Instant::now();
        let mut router =
            AutomarkerConfirmationRouter::begin_at("private-session".into(), 1, origin).unwrap();
        for sequence in 1..=MAX_SESSION_PROVENANCE_KEYS as u64 {
            let event = successful_return(sequence, 1);
            router.seen_events.insert(event.provenance().clone(), event);
        }
        let before_seen = router.seen_events.clone();
        assert_eq!(
            router.route_snapshot_at(
                snapshot(vec![successful_return(
                    MAX_SESSION_PROVENANCE_KEYS as u64 + 1,
                    2,
                )]),
                origin + Duration::from_micros(2),
            ),
            Err(ConfirmationRouterError::SessionEvidenceLimitExceeded)
        );
        assert_eq!(router.seen_events, before_seen);
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
