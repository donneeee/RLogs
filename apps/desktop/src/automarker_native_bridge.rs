//! Private desktop-owned lifecycle for the future one-marker native bridge.
//!
//! This module intentionally exposes no HTTP or plug-in surface. Its optional
//! readiness probe may load pinned dependencies and briefly open a false-filter
//! read-only handle, but it performs no active interception, packet mutation,
//! or send. It binds private parser evidence to the exact capture session and
//! scene context that a future in-process `AutomarkerBridgeCoordinator` must
//! use, and gives all future native resources one invalidation/shutdown domain.

use std::{sync::Mutex, time::Instant};

use rlogs_game_bpsr::{
    AutomarkerBridgeCoordinator, AutomarkerConfirmationBaseline, AutomarkerConfirmationContext,
    AutomarkerConfirmationTcpTuple, AutomarkerOwnedTcpConnection, AutomarkerRequestXyz,
    OfflineAutomarkerConnectionEpochBinding, ProtocolPack, SINGLE_MARKER_XYZ_CANARY_ARM_TOKEN,
    SingleMarkerXyzCanaryConfig, SingleMarkerXyzCanaryContext,
};

#[cfg(windows)]
use crate::automarker_native_readiness::{
    AutomarkerNativeReadinessEvidence, AutomarkerNativeReadinessRequest,
    AutomarkerPassiveReadinessWorker, discover_native_readiness,
};
#[cfg(windows)]
use crate::automarker_windivert_backend::WinDivertHandle;
use crate::{
    automarker_bridge_evidence::{
        AutomarkerBridgeCaptureTcpConnection, AutomarkerBridgeEvidenceSnapshot,
        AutomarkerBridgeSessionIdentity,
    },
    automarker_presets::{AutomarkerPoint, AutomarkerSceneContext},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AutomarkerBridgeReverseAckObservation {
    pub connection_epoch: u64,
    pub capture_sequence: u64,
    pub observed_micros: u64,
    pub source_address: std::net::Ipv4Addr,
    pub source_port: u16,
    pub destination_address: std::net::Ipv4Addr,
    pub destination_port: u16,
    pub ack_flag: bool,
    pub cumulative_ack: u32,
    pub syn: bool,
    pub fin: bool,
    pub rst: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NativeFlowEvidence {
    binding: OfflineAutomarkerConnectionEpochBinding,
    capture_connection: AutomarkerBridgeCaptureTcpConnection,
    syn_capture_sequence: u64,
    syn_observed_micros: u64,
    reverse_ack: Option<AutomarkerBridgeReverseAckObservation>,
}

const LIVE_PACKET_MUTATION_WIRED: bool = false;
const STOP_DRAIN_JOIN_RESOURCE_BUNDLE_WIRED: bool = false;

#[derive(Debug, Clone, PartialEq, Eq)]
struct BridgeContinuity {
    capture_session_id: String,
    deployment_id: String,
    client_build: String,
    protocol_pack_digest: String,
    scene_id: i32,
    map_id: u32,
    activity_family_id: String,
}

impl BridgeContinuity {
    fn from_session_scene(
        session: &AutomarkerBridgeSessionIdentity,
        scene: &AutomarkerSceneContext,
    ) -> Option<Self> {
        if !session.protocol_supported
            || session.capture_session_id.trim().is_empty()
            || session.deployment_id.trim().is_empty()
            || session.client_build.trim().is_empty()
            || session.protocol_pack_digest.trim().is_empty()
            || scene.client_build != session.client_build
            || scene.activity_family_id.trim().is_empty()
            || scene.scene_id <= 0
            || scene.map_id == 0
        {
            return None;
        }
        Some(Self {
            capture_session_id: session.capture_session_id.clone(),
            deployment_id: session.deployment_id.clone(),
            client_build: session.client_build.clone(),
            protocol_pack_digest: session.protocol_pack_digest.clone(),
            scene_id: scene.scene_id,
            map_id: scene.map_id,
            activity_family_id: scene.activity_family_id.clone(),
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct NativeGateState {
    exact_local_process: bool,
    exact_syn_owned_tuple_epoch: bool,
    pinned_backend: bool,
    reflect_arbitrated: bool,
    exact_build_pack_scene: bool,
    fresh_world_use_slot_carrier: bool,
    checksum_helper_ready: bool,
    authoritative_inbound_decoder_ready: bool,
}

impl NativeGateState {
    fn all_satisfied(self) -> bool {
        self.exact_local_process
            && self.exact_syn_owned_tuple_epoch
            && self.pinned_backend
            && self.reflect_arbitrated
            && self.exact_build_pack_scene
            && self.fresh_world_use_slot_carrier
            && self.checksum_helper_ready
            && self.authoritative_inbound_decoder_ready
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum LifecyclePhase {
    #[default]
    Dormant,
    Observing,
    Invalidated,
    Shutdown,
    Poisoned,
}

#[derive(Default)]
struct NativeBridgeState {
    phase: LifecyclePhase,
    generation: u64,
    session: Option<AutomarkerBridgeSessionIdentity>,
    continuity: Option<BridgeContinuity>,
    parser_evidence: Option<AutomarkerBridgeEvidenceSnapshot>,
    parser_carrier_received_at: Option<Instant>,
    protocol_pack: Option<ProtocolPack>,
    native_flow: Option<NativeFlowEvidence>,
    gates: NativeGateState,
    #[cfg(windows)]
    passive_readiness_worker: Option<AutomarkerPassiveReadinessWorker>,
    #[cfg(windows)]
    active_handle: Option<WinDivertHandle>,
    // Declared after the native handle so ordinary struct drop also closes
    // interception before discarding the coordinator's retransmission ledger.
    coordinator: Option<AutomarkerBridgeCoordinator>,
}

#[derive(Default)]
struct DetachedNativeResources {
    #[cfg(windows)]
    passive_readiness_worker: Option<AutomarkerPassiveReadinessWorker>,
    #[cfg(windows)]
    active_handle: Option<WinDivertHandle>,
    coordinator: Option<AutomarkerBridgeCoordinator>,
}

impl DetachedNativeResources {
    /// Interception must be closed before the retransmission ledger is
    /// discarded. Both drops happen only after the lifecycle mutex is free.
    fn drop_in_shutdown_order(self) {
        #[cfg(windows)]
        if let Some(worker) = self.passive_readiness_worker {
            worker.stop_drain_join();
        }
        #[cfg(windows)]
        drop(self.active_handle);
        drop(self.coordinator);
    }
}

#[derive(Default)]
pub(crate) struct AutomarkerNativeBridgeLifecycle {
    state: Mutex<NativeBridgeState>,
}

impl AutomarkerNativeBridgeLifecycle {
    #[allow(dead_code)] // Tests and pack-less fail-closed embeddings.
    pub(crate) fn begin_session(&self, session: AutomarkerBridgeSessionIdentity) {
        self.begin_session_inner(session, None);
    }

    pub(crate) fn begin_session_with_pack(
        &self,
        session: AutomarkerBridgeSessionIdentity,
        pack: ProtocolPack,
    ) {
        self.begin_session_inner(session, Some(pack));
    }

    fn begin_session_inner(
        &self,
        session: AutomarkerBridgeSessionIdentity,
        pack: Option<ProtocolPack>,
    ) {
        let Some(mut state) = self.lock_or_poison_shutdown() else {
            return;
        };
        if state.phase == LifecyclePhase::Poisoned {
            return;
        }
        let detached = Self::detach_owned_state(&mut state, LifecyclePhase::Observing, true);
        state.session = Some(session);
        state.protocol_pack = pack;
        drop(state);
        detached.drop_in_shutdown_order();
    }

    /// Bind private parser evidence to one exact session/build/pack/scene.
    /// Any mismatch invalidates the coordinator and all native gate state.
    pub(crate) fn accept_parser_evidence(
        &self,
        scene: Option<&AutomarkerSceneContext>,
        evidence: AutomarkerBridgeEvidenceSnapshot,
    ) -> bool {
        let Some(mut state) = self.lock_or_poison_shutdown() else {
            return false;
        };
        if !matches!(
            state.phase,
            LifecyclePhase::Observing | LifecyclePhase::Invalidated
        ) {
            return false;
        }
        let Some(session) = state.session.as_ref() else {
            return Self::invalidate_and_release(state);
        };
        let Some(scene) = scene else {
            return Self::invalidate_and_release(state);
        };
        let Some(continuity) = BridgeContinuity::from_session_scene(session, scene) else {
            return Self::invalidate_and_release(state);
        };
        if evidence.markers.iter().any(|marker| {
            marker.capture_session_id != continuity.capture_session_id
                || marker.deployment_id != continuity.deployment_id
                || marker.client_build != continuity.client_build
                || marker.protocol_pack_digest != continuity.protocol_pack_digest
                || marker.scene_id != continuity.scene_id
                || marker.map_id != continuity.map_id
                || marker.activity_family_id != continuity.activity_family_id
        }) {
            return Self::invalidate_and_release(state);
        }
        if evidence.outbound_carrier.as_ref().is_some_and(|carrier| {
            carrier.capture_session_id != continuity.capture_session_id
                || carrier.deployment_id != continuity.deployment_id
                || carrier.client_build != continuity.client_build
                || carrier.protocol_pack_digest != continuity.protocol_pack_digest
                || carrier.scene_id != continuity.scene_id
                || carrier.map_id != continuity.map_id
                || carrier.activity_family_id != continuity.activity_family_id
        }) {
            return Self::invalidate_and_release(state);
        }
        if evidence.correlated_return.as_ref().is_some_and(|returned| {
            returned.capture_session_id != continuity.capture_session_id
                || returned.deployment_id != continuity.deployment_id
                || returned.client_build != continuity.client_build
                || returned.protocol_pack_digest != continuity.protocol_pack_digest
                || returned.scene_id != continuity.scene_id
                || returned.map_id != continuity.map_id
                || returned.activity_family_id != continuity.activity_family_id
                || evidence.outbound_carrier.as_ref().is_none_or(|carrier| {
                    returned.carrier_capture_sequence != carrier.provenance.capture_sequence
                        || returned.provenance.call_id != carrier.provenance.call_id
                        || returned.provenance.connection_id != carrier.provenance.connection_id
                })
        }) {
            return Self::invalidate_and_release(state);
        }
        if state
            .parser_evidence
            .as_ref()
            .is_some_and(|previous| evidence.feed_revision < previous.feed_revision)
        {
            return Self::invalidate_and_release(state);
        }
        if state
            .continuity
            .as_ref()
            .is_some_and(|current| current != &continuity)
        {
            return Self::invalidate_and_release(state);
        }
        state.continuity = Some(continuity);
        let carrier_changed = state
            .parser_evidence
            .as_ref()
            .and_then(|previous| previous.outbound_carrier.as_ref())
            .map(|carrier| carrier.provenance.capture_sequence)
            != evidence
                .outbound_carrier
                .as_ref()
                .map(|carrier| carrier.provenance.capture_sequence);
        let carrier_present = evidence.outbound_carrier.is_some();
        state.parser_evidence = Some(evidence);
        if carrier_changed {
            state.parser_carrier_received_at = carrier_present.then(Instant::now);
        }
        let capture_connection = state
            .parser_evidence
            .as_ref()
            .and_then(|evidence| evidence.outbound_carrier.as_ref())
            .map(|carrier| carrier.tcp_connection);
        if state.native_flow.as_ref().is_some_and(|flow| {
            capture_connection.is_none_or(|capture| {
                capture != flow.capture_connection
                    || !binding_matches_capture(flow.binding, capture)
            })
        }) {
            return Self::invalidate_and_release(state);
        }
        if capture_connection.is_none() {
            state.native_flow = None;
            state.gates.exact_local_process = false;
            state.gates.exact_syn_owned_tuple_epoch = false;
        }
        state.phase = LifecyclePhase::Observing;
        // Parser continuity is necessary but insufficient. It cannot assert
        // process ownership, REFLECT arbitration, or packet-send readiness.
        state.gates.exact_build_pack_scene = true;
        // A parser-observed carrier is correlation evidence only. The fresh
        // carrier gate belongs to a future active game-PC interception loop
        // holding the exact packet it can synchronously return.
        state.gates.fresh_world_use_slot_carrier = false;
        true
    }

    /// Retain only an opaque SYN/process-owned binding whose exact IPv4 tuple
    /// matches the passive parser carrier. This method opens no native handle.
    #[allow(dead_code)] // Consumed by the future reviewed passive WinDivert loop.
    pub(crate) fn accept_native_flow_binding(
        &self,
        binding: OfflineAutomarkerConnectionEpochBinding,
        syn_capture_sequence: u64,
        syn_observed_micros: u64,
    ) -> bool {
        let Some(mut state) = self.lock_or_poison_shutdown() else {
            return false;
        };
        if !Self::retain_native_flow_locked(
            &mut state,
            binding,
            syn_capture_sequence,
            syn_observed_micros,
        ) {
            return Self::invalidate_and_release(state);
        }
        true
    }

    /// Retain a read-only game-PC readiness result. REFLECT is deliberately
    /// left false: the active open must arbitrate again while opening the exact
    /// tuple filter. No active handle or checksum helper is retained here.
    #[cfg(windows)]
    #[allow(dead_code)] // Called by the next private game-PC discovery worker.
    pub(crate) fn accept_native_readiness(
        &self,
        readiness: AutomarkerNativeReadinessEvidence,
        syn_capture_sequence: u64,
        syn_observed_micros: u64,
    ) -> bool {
        let Some(mut state) = self.lock_or_poison_shutdown() else {
            return false;
        };
        if !readiness.reflect_preflight_clear
            || !readiness.pinned_backend_ready
            || !readiness.checksum_helper_ready
            || !Self::retain_native_flow_locked(
                &mut state,
                readiness.binding,
                syn_capture_sequence,
                syn_observed_micros,
            )
        {
            return Self::invalidate_and_release(state);
        }
        state.gates.pinned_backend = true;
        state.gates.checksum_helper_ready = true;
        state.gates.reflect_arbitrated = false;
        true
    }

    fn retain_native_flow_locked(
        state: &mut NativeBridgeState,
        binding: OfflineAutomarkerConnectionEpochBinding,
        syn_capture_sequence: u64,
        syn_observed_micros: u64,
    ) -> bool {
        let Some(capture) = state
            .parser_evidence
            .as_ref()
            .and_then(|evidence| evidence.outbound_carrier.as_ref())
            .map(|carrier| carrier.tcp_connection)
        else {
            return false;
        };
        if state.phase != LifecyclePhase::Observing
            || state.continuity.is_none()
            || binding.connection_epoch() == 0
            || syn_capture_sequence == 0
            || !binding_matches_capture(binding, capture)
        {
            return false;
        }
        state.native_flow = Some(NativeFlowEvidence {
            binding,
            capture_connection: capture,
            syn_capture_sequence,
            syn_observed_micros,
            reverse_ack: None,
        });
        state.gates.exact_local_process = true;
        state.gates.exact_syn_owned_tuple_epoch = true;
        true
    }

    /// Run the private read-only host probe, then bind its result to the same
    /// parser carrier tuple. This never opens or retains an active filter.
    #[cfg(windows)]
    #[allow(dead_code)] // Invoked by the next lifecycle worker slice.
    pub(crate) fn discover_and_accept_native_readiness(
        &self,
        request: AutomarkerNativeReadinessRequest<'_>,
    ) -> Result<bool, String> {
        let readiness = discover_native_readiness(&request)?;
        Ok(self.accept_native_readiness(
            readiness,
            request.syn_capture_sequence,
            request.syn_observed_micros,
        ))
    }

    /// Own a passive SYN observer inside this lifecycle's shutdown domain.
    /// It does not hold, mutate, or reinject packets and cannot enable Place.
    #[cfg(windows)]
    #[allow(dead_code)] // Started by the future private game-PC host wiring.
    pub(crate) fn start_passive_readiness_worker(
        &self,
        process_id: u32,
        dependency_directory: &std::path::Path,
        connection_epoch: u64,
    ) -> Result<bool, String> {
        let worker = AutomarkerPassiveReadinessWorker::spawn(
            process_id,
            dependency_directory,
            connection_epoch,
        )?;
        let Some(mut state) = self.lock_or_poison_shutdown() else {
            drop(worker);
            return Ok(false);
        };
        if state.phase != LifecyclePhase::Observing || state.session.is_none() {
            drop(state);
            worker.stop_drain_join();
            return Ok(false);
        }
        let previous = state.passive_readiness_worker.replace(worker);
        drop(state);
        if let Some(previous) = previous {
            previous.stop_drain_join();
        }
        Ok(true)
    }

    /// Consume at most one packet-free readiness result. Exact parser
    /// continuity and the observed carrier tuple must already be present.
    #[cfg(windows)]
    #[allow(dead_code)] // Polled by the future private game-PC host wiring.
    pub(crate) fn poll_passive_readiness_worker(&self) -> bool {
        let Some(mut state) = self.lock_or_poison_shutdown() else {
            return false;
        };
        let outcome = state
            .passive_readiness_worker
            .as_ref()
            .and_then(AutomarkerPassiveReadinessWorker::try_take_result);
        let Some(outcome) = outcome else {
            return false;
        };
        let worker = state
            .passive_readiness_worker
            .take()
            .expect("observed worker result has an owning worker");
        match outcome {
            Ok(observation)
                if observation.readiness.reflect_preflight_clear
                    && observation.readiness.pinned_backend_ready
                    && observation.readiness.checksum_helper_ready
                    && Self::retain_native_flow_locked(
                        &mut state,
                        observation.readiness.binding,
                        observation.syn_ordinal,
                        observation.syn_observed_micros,
                    ) =>
            {
                state.gates.pinned_backend = true;
                state.gates.checksum_helper_ready = true;
                // A passive preflight is not authorization for a future
                // active exact-tuple open; that operation must re-arbitrate.
                state.gates.reflect_arbitrated = false;
                drop(state);
                worker.stop_drain_join();
                true
            }
            Ok(_) | Err(_) => {
                let mut detached =
                    Self::detach_owned_state(&mut state, LifecyclePhase::Invalidated, false);
                detached.passive_readiness_worker = Some(worker);
                drop(state);
                detached.drop_in_shutdown_order();
                false
            }
        }
    }

    /// Arm only the pure one-marker coordinator from the exact retained
    /// parser/native baseline. This opens no handle and cannot send.
    pub(crate) fn arm_one_marker_coordinator(&self, point: AutomarkerPoint) -> bool {
        let Some(mut state) = self.lock_or_poison_shutdown() else {
            return false;
        };
        let (Some(continuity), Some(evidence), Some(received_at), Some(flow), Some(pack)) = (
            state.continuity.as_ref(),
            state.parser_evidence.as_ref(),
            state.parser_carrier_received_at,
            state.native_flow.as_ref(),
            state.protocol_pack.as_ref(),
        ) else {
            return false;
        };
        let Some(carrier) = evidence.outbound_carrier.as_ref() else {
            return false;
        };
        let Ok(local_actor_id) = i64::try_from(carrier.local_actor_id) else {
            return false;
        };
        let age_millis = received_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        if state.phase != LifecyclePhase::Observing
            || carrier.marker_number != point.marker_number
            || carrier.mechanics_runtime_revision == 0
            || carrier.local_actor_id == 0
            || carrier.provenance.capture_sequence == 0
            || age_millis > rlogs_game_bpsr::SINGLE_MARKER_XYZ_MAX_CONTEXT_AGE_MILLIS
            || ![point.x, point.y, point.z]
                .iter()
                .all(|value| value.is_finite() && value.abs() <= 1_000_000.0)
            || pack.definition().target.build_id != continuity.client_build
            || pack.digest() != continuity.protocol_pack_digest
        {
            return false;
        }
        let connection = flow.binding.connection();
        let context = AutomarkerConfirmationContext {
            game_build: continuity.client_build.clone(),
            scene_family: continuity.activity_family_id.clone(),
            local_actor_id,
            connection_epoch: flow.binding.connection_epoch(),
            client_to_server_tuple: AutomarkerConfirmationTcpTuple {
                client_address: connection.local.address.octets(),
                client_port: connection.local.port,
                server_address: connection.remote.address.octets(),
                server_port: connection.remote.port,
            },
            runtime_revision: carrier.mechanics_runtime_revision,
            observed_micros: carrier.provenance.observed_micros,
        };
        let baseline = AutomarkerConfirmationBaseline {
            context,
            observation_ordinal: carrier.provenance.capture_sequence,
            same_number_passive_instance_identities: evidence
                .markers
                .iter()
                .filter(|marker| marker.marker_number == point.marker_number)
                .map(|marker| marker.passive_instance_identity)
                .collect(),
        };
        let config = SingleMarkerXyzCanaryConfig {
            expected_scene_family: continuity.activity_family_id.clone(),
            marker_number: point.marker_number,
            target_position: AutomarkerRequestXyz {
                x: point.x,
                y: point.y,
                z: point.z,
            },
        };
        let canary_context = SingleMarkerXyzCanaryContext {
            game_build: &continuity.client_build,
            current_scene_family: &continuity.activity_family_id,
            runtime_revision: carrier.mechanics_runtime_revision,
            observation_monotonic_millis: carrier.provenance.observed_micros / 1_000,
            observation_age_millis: age_millis,
        };
        let Ok(coordinator) = AutomarkerBridgeCoordinator::arm(
            config,
            SINGLE_MARKER_XYZ_CANARY_ARM_TOKEN,
            pack,
            flow.binding,
            canary_context,
            baseline,
        ) else {
            return false;
        };
        let previous = state.coordinator.replace(coordinator);
        // An armed pure coordinator is still waiting for a future carrier;
        // no active packet has been held by this process.
        state.gates.fresh_world_use_slot_carrier = false;
        drop(state);
        drop(previous);
        true
    }

    #[allow(dead_code)] // Awaiting an explicit bounded operator cancel request.
    pub(crate) fn cancel_armed_coordinator(&self) -> bool {
        let Some(mut state) = self.lock_or_poison_shutdown() else {
            return false;
        };
        let coordinator = state.coordinator.take();
        state.gates.fresh_world_use_slot_carrier = false;
        drop(state);
        let cancelled = coordinator.is_some();
        drop(coordinator);
        cancelled
    }

    /// Retain the latest reverse cumulative ACK only on the exact bound epoch
    /// and tuple. SYN/FIN/RST or regressed capture order invalidates the flow.
    #[allow(dead_code)] // Consumed by the future reviewed passive WinDivert loop.
    pub(crate) fn observe_native_reverse_ack(
        &self,
        observation: AutomarkerBridgeReverseAckObservation,
    ) -> bool {
        let Some(mut state) = self.lock_or_poison_shutdown() else {
            return false;
        };
        let Some(mut flow) = state.native_flow else {
            return Self::invalidate_and_release(state);
        };
        let capture = flow.capture_connection;
        let ordered = observation.capture_sequence > flow.syn_capture_sequence
            && observation.observed_micros >= flow.syn_observed_micros
            && flow.reverse_ack.is_none_or(|previous| {
                observation.capture_sequence > previous.capture_sequence
                    && observation.observed_micros >= previous.observed_micros
                    && observation
                        .cumulative_ack
                        .wrapping_sub(previous.cumulative_ack)
                        < (1_u32 << 31)
            });
        let exact_reverse = observation.source_address == capture.server_address
            && observation.source_port == capture.server_port
            && observation.destination_address == capture.client_address
            && observation.destination_port == capture.client_port;
        if state.phase != LifecyclePhase::Observing
            || observation.connection_epoch != flow.binding.connection_epoch()
            || !observation.ack_flag
            || observation.syn
            || observation.fin
            || observation.rst
            || !ordered
            || !exact_reverse
        {
            return Self::invalidate_and_release(state);
        }
        flow.reverse_ack = Some(observation);
        state.native_flow = Some(flow);
        true
    }

    #[allow(dead_code)] // Consumed by the future reviewed passive WinDivert loop.
    pub(crate) fn observe_native_connection_terminated(&self, connection_epoch: u64) -> bool {
        let Some(mut state) = self.lock_or_poison_shutdown() else {
            return false;
        };
        if state
            .native_flow
            .as_ref()
            .is_some_and(|flow| flow.binding.connection_epoch() == connection_epoch)
        {
            let detached = Self::detach_owned_state(&mut state, LifecyclePhase::Invalidated, false);
            drop(state);
            detached.drop_in_shutdown_order();
            return true;
        }
        false
    }

    pub(crate) fn reconcile_context(&self, scene: Option<&AutomarkerSceneContext>) -> bool {
        let Some(mut state) = self.lock_or_poison_shutdown() else {
            return false;
        };
        let Some(session) = state.session.as_ref() else {
            return false;
        };
        let next = scene.and_then(|scene| BridgeContinuity::from_session_scene(session, scene));
        if state.continuity == next {
            return false;
        }
        let detached = Self::detach_owned_state(&mut state, LifecyclePhase::Invalidated, false);
        drop(state);
        detached.drop_in_shutdown_order();
        true
    }

    /// A capture-owned connection-set transition invalidates every native
    /// resource and any carrier correlation from the previous tuple epoch.
    pub(crate) fn invalidate_connection_epoch(&self) {
        let Some(mut state) = self.lock_or_poison_shutdown() else {
            return;
        };
        if !matches!(
            state.phase,
            LifecyclePhase::Observing | LifecyclePhase::Invalidated
        ) {
            return;
        }
        let detached = Self::detach_owned_state(&mut state, LifecyclePhase::Invalidated, false);
        drop(state);
        detached.drop_in_shutdown_order();
    }

    /// Report whether a changed process-confirmed connection set requires a
    /// new passive epoch. A pending SYN worker owns the transition and must
    /// not be torn down merely because the ordinary capture noticed its new
    /// connection after the SYN.
    pub(crate) fn confirmed_connections_require_restart(
        &self,
        confirmed: &[rlogs_capture::TcpConnection],
    ) -> bool {
        let Some(state) = self.lock_or_poison_shutdown() else {
            return false;
        };
        #[cfg(windows)]
        if state.passive_readiness_worker.is_some() {
            return false;
        }
        let Some(flow) = state.native_flow.as_ref() else {
            return true;
        };
        let capture = flow.capture_connection;
        !confirmed.iter().any(|connection| {
            connection.client.address == std::net::IpAddr::V4(capture.client_address)
                && connection.client.port == capture.client_port
                && connection.server.address == std::net::IpAddr::V4(capture.server_address)
                && connection.server.port == capture.server_port
        })
    }

    pub(crate) fn finish_session(&self, session_id: &str) {
        let Some(mut state) = self.lock_or_poison_shutdown() else {
            return;
        };
        if state
            .session
            .as_ref()
            .is_some_and(|session| session.capture_session_id == session_id)
        {
            let detached = Self::detach_owned_state(&mut state, LifecyclePhase::Shutdown, true);
            drop(state);
            detached.drop_in_shutdown_order();
        }
    }

    /// Production placement remains disabled. Even a future all-positive gate
    /// snapshot cannot enable sending until the in-process packet loop itself
    /// is reviewed and this compile-time boundary is deliberately changed.
    pub(crate) fn placement_enabled(&self) -> bool {
        let Some(state) = self.lock_or_poison_shutdown() else {
            return false;
        };
        state.phase == LifecyclePhase::Observing
            && state.continuity.is_some()
            && state.coordinator.is_some()
            && {
                #[cfg(windows)]
                {
                    state.active_handle.is_some()
                }
                #[cfg(not(windows))]
                {
                    false
                }
            }
            && state.gates.all_satisfied()
            && LIVE_PACKET_MUTATION_WIRED
            && STOP_DRAIN_JOIN_RESOURCE_BUNDLE_WIRED
    }

    fn detach_owned_state(
        state: &mut NativeBridgeState,
        phase: LifecyclePhase,
        clear_session: bool,
    ) -> DetachedNativeResources {
        state.generation = state.generation.wrapping_add(1);
        state.phase = phase;
        if clear_session {
            state.session = None;
        }
        state.continuity = None;
        state.parser_evidence = None;
        state.parser_carrier_received_at = None;
        if clear_session {
            state.protocol_pack = None;
        }
        state.native_flow = None;
        state.gates = NativeGateState::default();
        DetachedNativeResources {
            #[cfg(windows)]
            passive_readiness_worker: state.passive_readiness_worker.take(),
            #[cfg(windows)]
            active_handle: state.active_handle.take(),
            coordinator: state.coordinator.take(),
        }
    }

    fn invalidate_and_release(mut state: std::sync::MutexGuard<'_, NativeBridgeState>) -> bool {
        let detached = Self::detach_owned_state(&mut state, LifecyclePhase::Invalidated, false);
        drop(state);
        detached.drop_in_shutdown_order();
        false
    }

    /// Mutex poison means a lifecycle mutation may have stopped halfway. Take
    /// every resource from the poisoned state, mark it terminal, unlock, then
    /// close interception before dropping the coordinator ledger.
    fn lock_or_poison_shutdown(&self) -> Option<std::sync::MutexGuard<'_, NativeBridgeState>> {
        match self.state.lock() {
            Ok(state) => Some(state),
            Err(poisoned) => {
                let mut state = poisoned.into_inner();
                let detached = Self::detach_owned_state(&mut state, LifecyclePhase::Poisoned, true);
                drop(state);
                detached.drop_in_shutdown_order();
                self.state.clear_poison();
                None
            }
        }
    }
}

fn binding_matches_capture(
    binding: OfflineAutomarkerConnectionEpochBinding,
    capture: AutomarkerBridgeCaptureTcpConnection,
) -> bool {
    let connection: AutomarkerOwnedTcpConnection = binding.connection();
    binding.process_id() != 0
        && connection.process_id == binding.process_id()
        && connection.local.address == capture.client_address
        && connection.local.port == capture.client_port
        && connection.remote.address == capture.server_address
        && connection.remote.port == capture.server_port
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automarker_bridge_evidence::{
        AutomarkerBridgeOutboundCarrierEvidence, AutomarkerBridgeRecordProvenance,
    };
    use rlogs_game_bpsr::{
        AUTOMARKER_REQUEST_BUILD, AutomarkerIpv4Endpoint, DecoderKind, FragmentKind,
        MappingProvenance, PacketDirection, bind_offline_automarker_connection_epoch,
    };
    use std::net::Ipv4Addr;
    #[cfg(windows)]
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    fn state(
        bridge: &AutomarkerNativeBridgeLifecycle,
    ) -> std::sync::MutexGuard<'_, NativeBridgeState> {
        bridge.state.lock().expect("test lifecycle mutex")
    }

    fn session() -> AutomarkerBridgeSessionIdentity {
        AutomarkerBridgeSessionIdentity {
            capture_session_id: "capture-a".into(),
            deployment_id: "global".into(),
            client_build: "25247556".into(),
            protocol_pack_digest: "sha256:exact".into(),
            protocol_supported: true,
        }
    }

    fn exact_automarker_pack() -> ProtocolPack {
        let source = ProtocolPack::from_json(include_bytes!(
            "../../../plugins/games/blue-protocol-star-resonance/protocol-packs/global/steam-24687926/pack.json"
        ))
        .unwrap();
        let source_build = source.definition().target.build_id.clone();
        let mut definition = source.definition().clone();
        definition.pack_id = format!("{}-compatibility-fallback-steam", definition.pack_id);
        definition.target.deployment_id = "global".into();
        definition.target.region_id = None;
        definition.target.channel = "steam".into();
        definition.target.build_id = AUTOMARKER_REQUEST_BUILD.into();
        definition.provenance.push(MappingProvenance {
            source: "provisional-compatibility-fallback".into(),
            reference: format!(
                "pack_build={source_build};client_deployment=global;client_channel=steam;client_build={AUTOMARKER_REQUEST_BUILD}"
            ),
        });
        ProtocolPack::build(definition).unwrap()
    }

    fn scene(family: &str) -> AutomarkerSceneContext {
        AutomarkerSceneContext {
            client_build: "25247556".into(),
            scene_id: 6525,
            map_id: 6525,
            activity_family_id: family.into(),
            scene_name: None,
        }
    }

    fn capture_connection() -> AutomarkerBridgeCaptureTcpConnection {
        AutomarkerBridgeCaptureTcpConnection {
            capture_connection_id: 50,
            client_address: Ipv4Addr::new(10, 0, 0, 2),
            client_port: 50_000,
            server_address: Ipv4Addr::new(10, 0, 0, 3),
            server_port: 443,
        }
    }

    fn parser_evidence() -> AutomarkerBridgeEvidenceSnapshot {
        AutomarkerBridgeEvidenceSnapshot {
            feed_revision: 2,
            markers: Vec::new(),
            outbound_carrier: Some(AutomarkerBridgeOutboundCarrierEvidence {
                capture_session_id: "capture-a".into(),
                deployment_id: "global".into(),
                client_build: "25247556".into(),
                protocol_pack_digest: "sha256:exact".into(),
                scene_id: 6525,
                map_id: 6525,
                activity_family_id: "mech-facility".into(),
                local_actor_id: 99,
                mechanics_runtime_revision: 7,
                marker_number: 1,
                session_sequence: 9,
                tcp_connection: capture_connection(),
                application_bytes: vec![0; 161],
                provenance: AutomarkerBridgeRecordProvenance {
                    capture_sequence: 30,
                    observed_micros: 1_000,
                    wall_clock_unix_micros: None,
                    connection_id: 50,
                    stream_id: 60,
                    direction: PacketDirection::ClientToServer,
                    fragment: FragmentKind::Call,
                    service_id: 103_198_054,
                    method_id: 249_858,
                    stub_id: 1,
                    call_id: Some(77),
                    decoder: DecoderKind::WorldUseSlotV1,
                },
            }),
            correlated_return: None,
        }
    }

    fn binding(remote_port: u16) -> OfflineAutomarkerConnectionEpochBinding {
        let connection = AutomarkerOwnedTcpConnection {
            process_id: 42,
            local: AutomarkerIpv4Endpoint {
                address: Ipv4Addr::new(10, 0, 0, 2),
                port: 50_000,
            },
            remote: AutomarkerIpv4Endpoint {
                address: Ipv4Addr::new(10, 0, 0, 3),
                port: remote_port,
            },
        };
        bind_offline_automarker_connection_epoch(42, connection, 9, true, &[connection]).unwrap()
    }

    fn reverse_ack() -> AutomarkerBridgeReverseAckObservation {
        AutomarkerBridgeReverseAckObservation {
            connection_epoch: 9,
            capture_sequence: 12,
            observed_micros: 1_200,
            source_address: Ipv4Addr::new(10, 0, 0, 3),
            source_port: 443,
            destination_address: Ipv4Addr::new(10, 0, 0, 2),
            destination_port: 50_000,
            ack_flag: true,
            cumulative_ack: 1234,
            syn: false,
            fin: false,
            rst: false,
        }
    }

    #[cfg(windows)]
    fn passive_observation()
    -> crate::automarker_native_readiness::AutomarkerPassiveReadinessObservation {
        crate::automarker_native_readiness::AutomarkerPassiveReadinessObservation {
            readiness: AutomarkerNativeReadinessEvidence {
                binding: binding(443),
                reflect_preflight_clear: true,
                pinned_backend_ready: true,
                checksum_helper_ready: true,
            },
            syn_ordinal: 10,
            syn_observed_micros: 900,
        }
    }

    #[cfg(windows)]
    fn install_completed_worker(
        bridge: &AutomarkerNativeBridgeLifecycle,
        result: Result<
            crate::automarker_native_readiness::AutomarkerPassiveReadinessObservation,
            String,
        >,
    ) -> Arc<AtomicBool> {
        let joined = Arc::new(AtomicBool::new(false));
        state(bridge).passive_readiness_worker = Some(
            AutomarkerPassiveReadinessWorker::completed_for_test(result, Arc::clone(&joined)),
        );
        joined
    }

    #[test]
    fn shutdown_clears_every_owned_domain_and_cannot_be_revived() {
        let bridge = AutomarkerNativeBridgeLifecycle::default();
        bridge.begin_session(session());
        assert!(bridge.accept_parser_evidence(
            Some(&scene("mech-facility")),
            AutomarkerBridgeEvidenceSnapshot::default(),
        ));
        let before = state(&bridge).generation;
        bridge.finish_session("capture-a");
        let snapshot = state(&bridge);
        assert_eq!(snapshot.phase, LifecyclePhase::Shutdown);
        assert!(snapshot.generation > before);
        assert!(snapshot.session.is_none());
        assert!(snapshot.continuity.is_none());
        assert!(snapshot.parser_evidence.is_none());
        assert!(snapshot.coordinator.is_none());
        drop(snapshot);
        assert!(!bridge.accept_parser_evidence(
            Some(&scene("mech-facility")),
            AutomarkerBridgeEvidenceSnapshot::default(),
        ));
    }

    #[cfg(windows)]
    #[test]
    fn session_shutdown_stops_and_joins_passive_worker() {
        let bridge = AutomarkerNativeBridgeLifecycle::default();
        bridge.begin_session(session());
        let joined = install_completed_worker(&bridge, Ok(passive_observation()));
        bridge.finish_session("capture-a");
        assert!(joined.load(Ordering::SeqCst));
        let snapshot = state(&bridge);
        assert_eq!(snapshot.phase, LifecyclePhase::Shutdown);
        assert!(snapshot.passive_readiness_worker.is_none());
    }

    #[test]
    fn context_change_invalidates_coordinator_domain() {
        let bridge = AutomarkerNativeBridgeLifecycle::default();
        bridge.begin_session(session());
        assert!(bridge.accept_parser_evidence(
            Some(&scene("mech-facility")),
            AutomarkerBridgeEvidenceSnapshot::default(),
        ));
        assert!(bridge.reconcile_context(Some(&scene("sea-ringed-reef"))));
        let snapshot = state(&bridge);
        assert_eq!(snapshot.phase, LifecyclePhase::Invalidated);
        assert!(snapshot.session.is_some());
        assert!(snapshot.continuity.is_none());
        assert!(snapshot.coordinator.is_none());
        assert_eq!(snapshot.gates, NativeGateState::default());
    }

    #[cfg(windows)]
    #[test]
    fn context_change_stops_and_joins_passive_worker() {
        let bridge = AutomarkerNativeBridgeLifecycle::default();
        bridge.begin_session(session());
        assert!(bridge.accept_parser_evidence(Some(&scene("mech-facility")), parser_evidence()));
        let joined = install_completed_worker(&bridge, Ok(passive_observation()));
        assert!(bridge.reconcile_context(Some(&scene("sea-ringed-reef"))));
        assert!(joined.load(Ordering::SeqCst));
        let snapshot = state(&bridge);
        assert_eq!(snapshot.phase, LifecyclePhase::Invalidated);
        assert!(snapshot.passive_readiness_worker.is_none());
    }

    #[test]
    fn no_send_before_all_gates_or_before_packet_loop_is_reviewed() {
        let bridge = AutomarkerNativeBridgeLifecycle::default();
        bridge.begin_session(session());
        assert!(bridge.accept_parser_evidence(
            Some(&scene("mech-facility")),
            AutomarkerBridgeEvidenceSnapshot::default(),
        ));
        assert!(!bridge.placement_enabled());
        {
            let mut snapshot = state(&bridge);
            snapshot.gates = NativeGateState {
                exact_local_process: true,
                exact_syn_owned_tuple_epoch: true,
                pinned_backend: true,
                reflect_arbitrated: true,
                exact_build_pack_scene: true,
                fresh_world_use_slot_carrier: true,
                checksum_helper_ready: true,
                authoritative_inbound_decoder_ready: true,
            };
        }
        // No coordinator and no reviewed packet loop: still impossible.
        assert!(!bridge.placement_enabled());
    }

    #[test]
    fn passive_carrier_does_not_satisfy_active_held_carrier_gate() {
        let bridge = AutomarkerNativeBridgeLifecycle::default();
        bridge.begin_session(session());
        assert!(bridge.accept_parser_evidence(Some(&scene("mech-facility")), parser_evidence(),));
        assert!(!state(&bridge).gates.fresh_world_use_slot_carrier);
    }

    #[test]
    fn exact_syn_owned_tuple_and_reverse_ack_are_retained_privately() {
        let bridge = AutomarkerNativeBridgeLifecycle::default();
        bridge.begin_session(session());
        assert!(bridge.accept_parser_evidence(Some(&scene("mech-facility")), parser_evidence(),));
        assert!(bridge.accept_native_flow_binding(binding(443), 10, 900));
        assert!(bridge.observe_native_reverse_ack(reverse_ack()));
        let snapshot = state(&bridge);
        assert!(snapshot.gates.exact_local_process);
        assert!(snapshot.gates.exact_syn_owned_tuple_epoch);
        let flow = snapshot.native_flow.as_ref().unwrap();
        assert_eq!(flow.capture_connection.capture_connection_id, 50);
        assert_eq!(flow.binding.connection_epoch(), 9);
        assert_eq!(flow.reverse_ack.unwrap().cumulative_ack, 1234);

        let mut regressed = reverse_ack();
        regressed.capture_sequence = 13;
        regressed.observed_micros = 1_300;
        regressed.cumulative_ack = 1233;
        drop(snapshot);
        assert!(!bridge.observe_native_reverse_ack(regressed));
        assert_eq!(state(&bridge).phase, LifecyclePhase::Invalidated);
    }

    #[cfg(windows)]
    #[test]
    fn read_only_readiness_never_claims_active_reflect_arbitration_or_send() {
        let bridge = AutomarkerNativeBridgeLifecycle::default();
        bridge.begin_session(session());
        assert!(bridge.accept_parser_evidence(Some(&scene("mech-facility")), parser_evidence(),));
        assert!(bridge.accept_native_readiness(
            AutomarkerNativeReadinessEvidence {
                binding: binding(443),
                reflect_preflight_clear: true,
                pinned_backend_ready: true,
                checksum_helper_ready: true,
            },
            10,
            900,
        ));
        let snapshot = state(&bridge);
        assert!(snapshot.gates.exact_local_process);
        assert!(snapshot.gates.exact_syn_owned_tuple_epoch);
        assert!(snapshot.gates.pinned_backend);
        assert!(snapshot.gates.checksum_helper_ready);
        assert!(!snapshot.gates.reflect_arbitrated);
        drop(snapshot);
        assert!(!bridge.placement_enabled());
    }

    #[test]
    fn exact_one_marker_request_arms_only_pure_coordinator_and_rearms_safely() {
        let bridge = AutomarkerNativeBridgeLifecycle::default();
        let pack = exact_automarker_pack();
        let exact_session = AutomarkerBridgeSessionIdentity {
            capture_session_id: "capture-a".into(),
            deployment_id: "global".into(),
            client_build: AUTOMARKER_REQUEST_BUILD.into(),
            protocol_pack_digest: pack.digest().into(),
            protocol_supported: true,
        };
        let exact_scene = AutomarkerSceneContext {
            client_build: AUTOMARKER_REQUEST_BUILD.into(),
            scene_id: 6525,
            map_id: 6525,
            activity_family_id: "mech-facility".into(),
            scene_name: None,
        };
        let mut evidence = parser_evidence();
        let carrier = evidence.outbound_carrier.as_mut().unwrap();
        carrier.client_build = AUTOMARKER_REQUEST_BUILD.into();
        carrier.protocol_pack_digest = pack.digest().into();
        bridge.begin_session_with_pack(exact_session, pack);
        assert!(bridge.accept_parser_evidence(Some(&exact_scene), evidence));
        assert!(bridge.accept_native_flow_binding(binding(443), 10, 900));
        let point = AutomarkerPoint {
            marker_number: 1,
            x: 101.25,
            y: -22.5,
            z: 303.75,
        };
        assert!(bridge.arm_one_marker_coordinator(point.clone()));
        assert!(state(&bridge).coordinator.is_some());
        assert!(!bridge.placement_enabled());
        assert!(bridge.arm_one_marker_coordinator(point));
        assert!(state(&bridge).coordinator.is_some());
        assert!(bridge.cancel_armed_coordinator());
        assert!(state(&bridge).coordinator.is_none());
        assert!(bridge.arm_one_marker_coordinator(AutomarkerPoint {
            marker_number: 1,
            x: 101.25,
            y: -22.5,
            z: 303.75,
        }));
        assert!(bridge.reconcile_context(Some(&scene("sea-ringed-reef"))));
        assert!(state(&bridge).coordinator.is_none());
    }

    #[cfg(windows)]
    #[test]
    fn passive_worker_result_retains_only_readiness_and_cannot_send() {
        let bridge = AutomarkerNativeBridgeLifecycle::default();
        bridge.begin_session(session());
        assert!(bridge.accept_parser_evidence(Some(&scene("mech-facility")), parser_evidence()));
        let joined = install_completed_worker(&bridge, Ok(passive_observation()));
        assert!(bridge.poll_passive_readiness_worker());
        assert!(joined.load(Ordering::SeqCst));
        let snapshot = state(&bridge);
        assert!(snapshot.passive_readiness_worker.is_none());
        assert!(snapshot.gates.exact_local_process);
        assert!(snapshot.gates.exact_syn_owned_tuple_epoch);
        assert!(snapshot.gates.pinned_backend);
        assert!(snapshot.gates.checksum_helper_ready);
        assert!(!snapshot.gates.reflect_arbitrated);
        drop(snapshot);
        assert!(!bridge.placement_enabled());
    }

    #[cfg(windows)]
    #[test]
    fn passive_worker_error_invalidates_and_joins_fail_closed() {
        let bridge = AutomarkerNativeBridgeLifecycle::default();
        bridge.begin_session(session());
        assert!(bridge.accept_parser_evidence(Some(&scene("mech-facility")), parser_evidence()));
        let joined = install_completed_worker(&bridge, Err("passive receive failed".into()));
        assert!(!bridge.poll_passive_readiness_worker());
        assert!(joined.load(Ordering::SeqCst));
        let snapshot = state(&bridge);
        assert_eq!(snapshot.phase, LifecyclePhase::Invalidated);
        assert!(snapshot.passive_readiness_worker.is_none());
        assert_eq!(snapshot.gates, NativeGateState::default());
        drop(snapshot);
        assert!(!bridge.placement_enabled());
    }

    #[test]
    fn tuple_ack_and_epoch_mismatches_fail_closed() {
        let bridge = AutomarkerNativeBridgeLifecycle::default();
        bridge.begin_session(session());
        assert!(bridge.accept_parser_evidence(Some(&scene("mech-facility")), parser_evidence(),));
        assert!(!bridge.accept_native_flow_binding(binding(444), 10, 900));
        assert_eq!(state(&bridge).phase, LifecyclePhase::Invalidated);

        bridge.begin_session(session());
        assert!(bridge.accept_parser_evidence(Some(&scene("mech-facility")), parser_evidence(),));
        assert!(bridge.accept_native_flow_binding(binding(443), 10, 900));
        let mut wrong = reverse_ack();
        wrong.connection_epoch = 10;
        assert!(!bridge.observe_native_reverse_ack(wrong));
        let snapshot = state(&bridge);
        assert_eq!(snapshot.phase, LifecyclePhase::Invalidated);
        assert!(snapshot.native_flow.is_none());
        assert_eq!(snapshot.gates, NativeGateState::default());
    }

    #[test]
    fn exact_connection_termination_invalidates_retained_flow() {
        let bridge = AutomarkerNativeBridgeLifecycle::default();
        bridge.begin_session(session());
        assert!(bridge.accept_parser_evidence(Some(&scene("mech-facility")), parser_evidence(),));
        assert!(bridge.accept_native_flow_binding(binding(443), 10, 900));
        assert!(bridge.observe_native_connection_terminated(9));
        assert_eq!(state(&bridge).phase, LifecyclePhase::Invalidated);
    }

    #[test]
    fn host_connection_epoch_change_invalidates_old_carrier_and_native_flow() {
        let bridge = AutomarkerNativeBridgeLifecycle::default();
        bridge.begin_session(session());
        assert!(bridge.accept_parser_evidence(Some(&scene("mech-facility")), parser_evidence()));
        assert!(bridge.accept_native_flow_binding(binding(443), 10, 900));
        bridge.invalidate_connection_epoch();
        let snapshot = state(&bridge);
        assert_eq!(snapshot.phase, LifecyclePhase::Invalidated);
        assert!(snapshot.parser_evidence.is_none());
        assert!(snapshot.native_flow.is_none());
        assert_eq!(snapshot.gates, NativeGateState::default());
    }

    #[test]
    fn mutex_poison_is_terminal_and_cannot_be_rearmed() {
        let bridge = AutomarkerNativeBridgeLifecycle::default();
        bridge.begin_session(session());
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = bridge.state.lock().expect("test lifecycle mutex");
            panic!("poison lifecycle mutex");
        }));

        assert!(!bridge.placement_enabled());
        let snapshot = state(&bridge);
        assert_eq!(snapshot.phase, LifecyclePhase::Poisoned);
        assert!(snapshot.session.is_none());
        assert!(snapshot.coordinator.is_none());
        drop(snapshot);

        bridge.begin_session(session());
        assert_eq!(state(&bridge).phase, LifecyclePhase::Poisoned);
    }
}
