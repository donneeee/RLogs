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
    reviewed_automarker_active_filter_plan,
};

#[cfg(windows)]
use crate::automarker_active_worker::ActiveAutomarkerBackend;
#[cfg(windows)]
use crate::automarker_native_readiness::{
    AutomarkerNativeFailureCategory, AutomarkerNativeReadinessEvidence,
    AutomarkerNativeReadinessRequest, AutomarkerPassiveReadinessStatus,
    AutomarkerPassiveReadinessWorker, discover_native_readiness, sanitized_failure_category,
};
#[cfg(windows)]
use crate::automarker_windivert_backend::WinDivertHandle;
use crate::{
    automarker_active_coordinator::{
        ActiveAutomarkerControl, ActiveAutomarkerCoordinatorConfig,
        ProductionActiveAutomarkerCoordinator,
    },
    automarker_active_worker::{
        ActiveAutomarkerBoundProcess, ActiveAutomarkerFatalOwnership,
        ActiveAutomarkerFatalRecovery, ActiveAutomarkerFatalRecoveryStatus,
        ActiveAutomarkerWorkerBundle,
    },
    automarker_bridge_evidence::{
        AutomarkerBridgeCaptureTcpConnection, AutomarkerBridgeEvidenceSnapshot,
        AutomarkerBridgeSessionIdentity,
    },
    automarker_confirmation_router::PrivateParserConfirmationSnapshot,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct AutomarkerNativeOperatorStatus {
    pub observer_ready: bool,
    pub syn_candidate_observed: bool,
    pub bpsr_tuple_confirmed: bool,
    pub marker_carrier_observed: bool,
    pub return_confirmed: bool,
    pub active_placement_enabled: bool,
    pub failure_category: Option<&'static str>,
}

const LIVE_PACKET_MUTATION_WIRED: bool = false;
const STOP_DRAIN_JOIN_RESOURCE_BUNDLE_WIRED: bool = false;
const ACTIVE_COMMAND_CAPACITY: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutomarkerActiveLifetimeArbitration {
    /// The exact active handle owns a continuously sampled REFLECT monitor;
    /// loss is latched by the backend and forces the worker into drain-only.
    Maintained,
}

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
    /// An active worker or its fatal completion still owns interception. No
    /// session transition or placement may occur until explicit recovery.
    OwnershipRecoveryRequired,
    Poisoned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutomarkerActiveOwnershipStatus {
    Idle,
    WorkerRunning,
    RecoveryRequired {
        bound_process: Option<ActiveAutomarkerBoundProcess>,
    },
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
    active_worker: Option<ActiveAutomarkerWorkerBundle>,
    active_control: Option<ActiveAutomarkerControl>,
    #[cfg(windows)]
    active_receive_control:
        Option<crate::automarker_active_windivert::ActiveAutomarkerReceiveControl>,
    fatal_ownership: Option<ActiveAutomarkerFatalOwnership>,
    shutdown_requested_while_active: bool,
    #[cfg(windows)]
    native_failure: Option<AutomarkerNativeFailureCategory>,
    #[cfg(windows)]
    passive_readiness_worker: Option<AutomarkerPassiveReadinessWorker>,
    #[cfg(windows)]
    pending_passive_readiness: Option<PendingPassiveReadiness>,
    #[cfg(windows)]
    active_handle: Option<WinDivertHandle>,
    // Declared after the native handle so ordinary struct drop also closes
    // interception before discarding the coordinator's retransmission ledger.
    coordinator: Option<AutomarkerBridgeCoordinator>,
}

#[cfg(windows)]
struct PendingPassiveReadiness {
    observation: crate::automarker_native_readiness::AutomarkerPassiveReadinessObservation,
    received_at: Instant,
    generation: u64,
    capture_session_id: String,
    process_id: u32,
    connection_epoch: u64,
    capture_tuple_confirmed: bool,
}

#[cfg(windows)]
const PASSIVE_READINESS_CANDIDATE_TTL: std::time::Duration = std::time::Duration::from_secs(2);

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
    pub(crate) fn active_lifetime_arbitration(&self) -> AutomarkerActiveLifetimeArbitration {
        AutomarkerActiveLifetimeArbitration::Maintained
    }
    /// Transfer the sole active worker owner into the bridge lifecycle. This
    /// is private until the activation review deliberately wires construction.
    #[allow(dead_code)]
    pub(crate) fn retain_active_worker(
        &self,
        worker: ActiveAutomarkerWorkerBundle,
    ) -> Result<(), ActiveAutomarkerWorkerBundle> {
        let Some(mut state) = self.lock_or_poison_shutdown() else {
            return Err(worker);
        };
        if state.active_worker.is_some()
            || state.fatal_ownership.is_some()
            || !matches!(state.phase, LifecyclePhase::Observing)
        {
            return Err(worker);
        }
        state.active_worker = Some(worker);
        Ok(())
    }

    /// Deliver one already source-ordered parser snapshot to the active
    /// coordinator. The bounded channel is process-private; saturation is a
    /// lifecycle failure rather than permission to drop confirmation proof.
    pub(crate) fn accept_confirmation_snapshot(
        &self,
        snapshot: PrivateParserConfirmationSnapshot,
    ) -> bool {
        let Some(mut state) = self.lock_or_poison_shutdown() else {
            return false;
        };
        Self::collect_finished_worker_locked(&mut state);
        let Some(control) = state.active_control.as_ref() else {
            return false;
        };
        if control.parser_snapshot(snapshot).is_err() {
            let _ = control.context_invalidated();
            Self::request_active_stop_locked(&mut state, false);
            return false;
        }
        #[cfg(windows)]
        if let Some(receive_control) = state.active_receive_control.as_ref() {
            let _ = receive_control.wake();
        }
        true
    }

    /// Construct the reviewed production coordinator and active Windows
    /// backend for exactly one retained session/scene/pack/tuple. This is a
    /// private developer-canary seam and is intentionally not called by the
    /// public activation route.
    #[cfg(windows)]
    #[allow(dead_code)]
    pub(crate) fn arm_private_one_marker_canary(
        &self,
        point: AutomarkerPoint,
        dependency_directory: &std::path::Path,
    ) -> Result<bool, String> {
        if self.active_lifetime_arbitration() != AutomarkerActiveLifetimeArbitration::Maintained {
            return Ok(false);
        }
        let Some(mut state) = self.lock_or_poison_shutdown() else {
            return Ok(false);
        };
        Self::collect_finished_worker_locked(&mut state);
        let Some(config) = Self::active_coordinator_config_locked(&state, &point) else {
            return Ok(false);
        };
        if state.active_worker.is_some() || state.fatal_ownership.is_some() {
            return Ok(false);
        }
        let generation = state.generation;
        let session_key = config.session_key.clone();
        let binding = config.connection_binding;
        let passive = state.passive_readiness_worker.take();
        drop(state);
        if let Some(passive) = passive {
            passive.stop_drain_join();
        }

        let (coordinator, control) =
            ProductionActiveAutomarkerCoordinator::create(config, ACTIVE_COMMAND_CAPACITY)
                .map_err(str::to_owned)?;
        let (mut backend, receive_control) =
            crate::automarker_active_windivert::WindowsActiveAutomarkerBackend::open(
                dependency_directory,
                binding,
            )?;
        if !backend.active_lifetime_arbitration_healthy() {
            backend.close_interception()?;
            return Ok(false);
        }
        let mut worker = ActiveAutomarkerWorkerBundle::spawn(backend, coordinator)?;

        let Some(mut state) = self.lock_or_poison_shutdown() else {
            let _ = receive_control.wake();
            let _ = worker.stop_drain_join();
            return Ok(false);
        };
        let still_exact = state.generation == generation
            && state.phase == LifecyclePhase::Observing
            && state
                .session
                .as_ref()
                .is_some_and(|session| session.capture_session_id == session_key)
            && state
                .native_flow
                .as_ref()
                .is_some_and(|flow| flow.binding == binding)
            && state.active_worker.is_none()
            && state.fatal_ownership.is_none();
        if !still_exact {
            drop(state);
            let _ = receive_control.wake();
            let _ = worker.stop_drain_join();
            return Ok(false);
        }
        drop(state.coordinator.take());
        state.active_control = Some(control);
        state.active_receive_control = Some(receive_control);
        state.active_worker = Some(worker);
        state.gates.reflect_arbitrated = true;
        state.gates.authoritative_inbound_decoder_ready = true;
        Ok(true)
    }

    fn active_coordinator_config_locked(
        state: &NativeBridgeState,
        point: &AutomarkerPoint,
    ) -> Option<ActiveAutomarkerCoordinatorConfig> {
        let continuity = state.continuity.as_ref()?;
        let evidence = state.parser_evidence.as_ref()?;
        let received_at = state.parser_carrier_received_at?;
        let flow = state.native_flow.as_ref()?;
        let pack = state.protocol_pack.as_ref()?;
        let carrier = evidence.outbound_carrier.as_ref()?;
        let local_actor_id = i64::try_from(carrier.local_actor_id).ok()?;
        let observation_age_millis =
            received_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        if state.phase != LifecyclePhase::Observing
            || !state.gates.exact_local_process
            || !state.gates.exact_syn_owned_tuple_epoch
            || !state.gates.pinned_backend
            || !state.gates.checksum_helper_ready
            || carrier.marker_number != point.marker_number
            || carrier.mechanics_runtime_revision == 0
            || carrier.provenance.capture_sequence == 0
            || carrier.tcp_connection.capture_connection_id == 0
            || observation_age_millis > rlogs_game_bpsr::SINGLE_MARKER_XYZ_MAX_CONTEXT_AGE_MILLIS
            || ![point.x, point.y, point.z]
                .iter()
                .all(|axis| axis.is_finite() && axis.abs() <= 1_000_000.0)
            || pack.definition().target.build_id != continuity.client_build
            || pack.digest() != continuity.protocol_pack_digest
            || !binding_matches_capture(flow.binding, carrier.tcp_connection)
        {
            return None;
        }
        Some(ActiveAutomarkerCoordinatorConfig {
            pack: pack.clone(),
            filter_plan: reviewed_automarker_active_filter_plan(flow.binding),
            connection_binding: flow.binding,
            session_key: continuity.capture_session_id.clone(),
            carrier_capture_sequence: carrier.provenance.capture_sequence,
            scene_family: continuity.activity_family_id.clone(),
            local_actor_id,
            marker_number: point.marker_number,
            target_position: AutomarkerRequestXyz {
                x: point.x,
                y: point.y,
                z: point.z,
            },
            baseline_runtime_revision: carrier.mechanics_runtime_revision,
            runtime_revision: carrier.mechanics_runtime_revision,
            observation_age_millis,
            baseline_same_number_passive_instance_identities: evidence
                .markers
                .iter()
                .filter(|marker| marker.marker_number == point.marker_number)
                .map(|marker| marker.passive_instance_identity)
                .collect(),
        })
    }

    /// Poll a terminal worker without blocking. Every fatal completion is
    /// moved into persistent lifecycle ownership before this method returns.
    #[allow(dead_code)]
    pub(crate) fn poll_active_ownership(&self) -> AutomarkerActiveOwnershipStatus {
        let Some(mut state) = self.lock_or_poison_shutdown() else {
            return AutomarkerActiveOwnershipStatus::RecoveryRequired {
                bound_process: None,
            };
        };
        Self::collect_finished_worker_locked(&mut state);
        if state.active_worker.is_none()
            && state.fatal_ownership.is_none()
            && state.shutdown_requested_while_active
        {
            state.shutdown_requested_while_active = false;
            let detached = Self::detach_owned_state(&mut state, LifecyclePhase::Shutdown, true);
            drop(state);
            detached.drop_in_shutdown_order();
            AutomarkerActiveOwnershipStatus::Idle
        } else if state.active_worker.is_none()
            && state.fatal_ownership.is_none()
            && state.phase == LifecyclePhase::OwnershipRecoveryRequired
        {
            let detached = Self::detach_owned_state(&mut state, LifecyclePhase::Invalidated, false);
            drop(state);
            detached.drop_in_shutdown_order();
            AutomarkerActiveOwnershipStatus::Idle
        } else {
            Self::active_ownership_status_locked(&state)
        }
    }

    /// Advance retained fatal ownership with one bounded, typed recovery
    /// observation. Activation remains closed until recovery releases it.
    #[allow(dead_code)]
    pub(crate) fn recover_active_ownership(
        &self,
        recovery: ActiveAutomarkerFatalRecovery,
    ) -> Result<AutomarkerActiveOwnershipStatus, String> {
        let Some(mut state) = self.lock_or_poison_shutdown() else {
            return Err("Automarker native lifecycle is poisoned".to_owned());
        };
        Self::collect_finished_worker_locked(&mut state);
        let Some(fatal) = state.fatal_ownership.as_mut() else {
            return Err("no retained Automarker fatal ownership is available".to_owned());
        };
        if fatal.recover(recovery)? == ActiveAutomarkerFatalRecoveryStatus::Released {
            let released = state.fatal_ownership.take();
            drop(released);
            if state.shutdown_requested_while_active {
                state.shutdown_requested_while_active = false;
                let detached = Self::detach_owned_state(&mut state, LifecyclePhase::Shutdown, true);
                drop(state);
                detached.drop_in_shutdown_order();
                return Ok(AutomarkerActiveOwnershipStatus::Idle);
            }
            state.phase = LifecyclePhase::Invalidated;
        }
        Ok(Self::active_ownership_status_locked(&state))
    }

    fn collect_finished_worker_locked(state: &mut NativeBridgeState) {
        let Some(worker) = state.active_worker.as_mut() else {
            return;
        };
        let completion = match worker.try_join_finished() {
            Ok(Some(completion)) => completion,
            Ok(None) => return,
            Err(_) => {
                state.active_worker = None;
                state.phase = LifecyclePhase::Poisoned;
                return;
            }
        };
        state.active_worker = None;
        state.active_control = None;
        #[cfg(windows)]
        {
            state.active_receive_control = None;
        }
        if let Some(fatal) = completion.fatal_ownership {
            state.fatal_ownership = Some(fatal);
            state.phase = LifecyclePhase::OwnershipRecoveryRequired;
        }
    }

    fn request_active_stop_locked(state: &mut NativeBridgeState, process_terminated: bool) {
        Self::collect_finished_worker_locked(state);
        if let Some(control) = state.active_control.as_ref() {
            let _ = if process_terminated {
                control.process_terminated()
            } else {
                control.context_invalidated()
            };
        }
        #[cfg(windows)]
        if let Some(receive_control) = state.active_receive_control.as_ref() {
            let _ = receive_control.wake();
        }
        if let Some(mut worker) = state.active_worker.take() {
            match worker.stop_drain_join() {
                Ok(completion) => {
                    state.active_control = None;
                    #[cfg(windows)]
                    {
                        state.active_receive_control = None;
                    }
                    if let Some(fatal) = completion.fatal_ownership {
                        state.fatal_ownership = Some(fatal);
                    }
                }
                Err(_) if worker.is_finished() => {
                    state.active_worker = Some(worker);
                    Self::collect_finished_worker_locked(state);
                }
                Err(_) => state.active_worker = Some(worker),
            }
        }
        Self::collect_finished_worker_locked(state);
        if state.active_worker.is_some() || state.fatal_ownership.is_some() {
            state.phase = LifecyclePhase::OwnershipRecoveryRequired;
        }
    }

    fn active_ownership_status_locked(
        state: &NativeBridgeState,
    ) -> AutomarkerActiveOwnershipStatus {
        if let Some(fatal) = state.fatal_ownership.as_ref() {
            AutomarkerActiveOwnershipStatus::RecoveryRequired {
                bound_process: fatal.retained_bound_process(),
            }
        } else if state.active_worker.is_some() {
            AutomarkerActiveOwnershipStatus::WorkerRunning
        } else {
            AutomarkerActiveOwnershipStatus::Idle
        }
    }

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
        Self::collect_finished_worker_locked(&mut state);
        if state.active_worker.is_some() || state.fatal_ownership.is_some() {
            Self::request_active_stop_locked(&mut state, false);
            if state.active_worker.is_some() || state.fatal_ownership.is_some() {
                state.phase = LifecyclePhase::OwnershipRecoveryRequired;
                return;
            }
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
            let detached = Self::clear_scene_bound_preserving_passive(&mut state);
            drop(state);
            detached.drop_in_shutdown_order();
            return false;
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
        if let (Some(mut flow), Some(capture)) = (state.native_flow, capture_connection) {
            if !binding_matches_capture(flow.binding, capture)
                || (flow.capture_connection.capture_connection_id != 0
                    && flow.capture_connection.capture_connection_id
                        != capture.capture_connection_id)
            {
                return Self::invalidate_and_release(state);
            }
            // SignatureFlowCapture proves the tuple before the framed parser
            // knows its connection id. Bind that id only when the exact later
            // carrier arrives; the absence of a carrier must not erase the
            // already-proven process-owned connection epoch.
            flow.capture_connection = capture;
            state.native_flow = Some(flow);
        }
        state.phase = LifecyclePhase::Observing;
        // Parser continuity is necessary but insufficient. It cannot assert
        // process ownership, REFLECT arbitration, or packet-send readiness.
        state.gates.exact_build_pack_scene = true;
        // A parser-observed carrier is correlation evidence only. The fresh
        // carrier gate belongs to a future active game-PC interception loop
        // holding the exact packet it can synchronously return.
        state.gates.fresh_world_use_slot_carrier = false;
        #[cfg(windows)]
        Self::try_promote_pending_readiness_locked(&mut state);
        if carrier_changed
            && let Some((capture_sequence, Some(rpc_call_id))) = state
                .parser_evidence
                .as_ref()
                .and_then(|evidence| evidence.outbound_carrier.as_ref())
                .map(|carrier| {
                    (
                        carrier.provenance.capture_sequence,
                        carrier.provenance.call_id,
                    )
                })
            && state.active_control.as_ref().is_some_and(|control| {
                control
                    .parser_carrier(capture_sequence, rpc_call_id)
                    .is_err()
            })
        {
            Self::request_active_stop_locked(&mut state, false);
            return false;
        }
        #[cfg(windows)]
        if carrier_changed && let Some(receive_control) = state.active_receive_control.as_ref() {
            let _ = receive_control.wake();
        }
        true
    }

    /// Retain only an opaque SYN/process-owned binding. If a parser carrier is
    /// already present its IPv4 tuple must match; otherwise the later carrier
    /// performs that independent correlation. This method opens no handle.
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
        let parser_capture = state
            .parser_evidence
            .as_ref()
            .and_then(|evidence| evidence.outbound_carrier.as_ref())
            .map(|carrier| carrier.tcp_connection);
        if state.phase != LifecyclePhase::Observing
            || state.session.is_none()
            || binding.connection_epoch() == 0
            || syn_capture_sequence == 0
            || parser_capture.is_some_and(|capture| !binding_matches_capture(binding, capture))
        {
            return false;
        }
        let connection = binding.connection();
        let capture = parser_capture.unwrap_or(AutomarkerBridgeCaptureTcpConnection {
            capture_connection_id: 0,
            client_address: connection.local.address,
            client_port: connection.local.port,
            server_address: connection.remote.address,
            server_port: connection.remote.port,
        });
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
        let worker = match AutomarkerPassiveReadinessWorker::spawn(
            process_id,
            dependency_directory,
            connection_epoch,
        ) {
            Ok(worker) => worker,
            Err(error) => {
                if let Some(mut state) = self.lock_or_poison_shutdown() {
                    state.native_failure = Some(sanitized_failure_category(&error));
                }
                return Err(error);
            }
        };
        let Some(mut state) = self.lock_or_poison_shutdown() else {
            drop(worker);
            return Ok(false);
        };
        if !matches!(
            state.phase,
            LifecyclePhase::Observing | LifecyclePhase::Invalidated
        ) || state.session.is_none()
        {
            drop(state);
            worker.stop_drain_join();
            return Ok(false);
        }
        state.phase = LifecyclePhase::Observing;
        let previous = state.passive_readiness_worker.replace(worker);
        state.native_failure = None;
        drop(state);
        if let Some(previous) = previous {
            previous.stop_drain_join();
        }
        Ok(true)
    }

    #[cfg(windows)]
    pub(crate) fn passive_readiness_worker_needed(&self) -> bool {
        let Some(state) = self.lock_or_poison_shutdown() else {
            return false;
        };
        matches!(
            state.phase,
            LifecyclePhase::Observing | LifecyclePhase::Invalidated
        ) && state.session.is_some()
            && state.active_worker.is_none()
            && state.fatal_ownership.is_none()
            && state.passive_readiness_worker.is_none()
            && state.pending_passive_readiness.is_none()
            && state.native_flow.is_none()
            && state.native_failure.is_none()
    }

    /// Consume at most one packet-free readiness result. It remains a bounded
    /// candidate until SignatureFlowCapture independently confirms its tuple;
    /// no marker carrier is required for that connection-level promotion.
    #[cfg(windows)]
    #[allow(dead_code)] // Polled by the future private game-PC host wiring.
    pub(crate) fn poll_passive_readiness_worker(&self) -> bool {
        let Some(mut state) = self.lock_or_poison_shutdown() else {
            return false;
        };
        if state
            .pending_passive_readiness
            .as_ref()
            .is_some_and(|pending| pending.received_at.elapsed() > PASSIVE_READINESS_CANDIDATE_TTL)
        {
            state.pending_passive_readiness = None;
        }
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
                    && observation.readiness.checksum_helper_ready =>
            {
                // This is only a bounded process-owned SYN candidate. The
                // ordinary parser capture must independently confirm its BPSR
                // tuple before it may satisfy any native readiness gate.
                state.pending_passive_readiness = Some(PendingPassiveReadiness {
                    observation,
                    received_at: Instant::now(),
                    generation: state.generation,
                    capture_session_id: state
                        .session
                        .as_ref()
                        .map(|session| session.capture_session_id.clone())
                        .unwrap_or_default(),
                    process_id: observation.readiness.binding.process_id(),
                    connection_epoch: observation.readiness.binding.connection_epoch(),
                    capture_tuple_confirmed: false,
                });
                state.native_failure = None;
                drop(state);
                worker.stop_drain_join();
                true
            }
            Ok(_) => {
                state.native_failure = Some(AutomarkerNativeFailureCategory::Internal);
                let mut detached =
                    Self::detach_owned_state(&mut state, LifecyclePhase::Invalidated, false);
                state.native_failure = Some(AutomarkerNativeFailureCategory::Internal);
                detached.passive_readiness_worker = Some(worker);
                drop(state);
                detached.drop_in_shutdown_order();
                false
            }
            Err(error) => {
                let category = sanitized_failure_category(&error);
                let mut detached =
                    Self::detach_owned_state(&mut state, LifecyclePhase::Invalidated, false);
                state.native_failure = Some(category);
                detached.passive_readiness_worker = Some(worker);
                drop(state);
                detached.drop_in_shutdown_order();
                false
            }
        }
    }

    pub(crate) fn sanitized_operator_status(&self) -> AutomarkerNativeOperatorStatus {
        let Some(state) = self.lock_or_poison_shutdown() else {
            return AutomarkerNativeOperatorStatus {
                failure_category: Some("internal"),
                ..AutomarkerNativeOperatorStatus::default()
            };
        };
        #[cfg(windows)]
        let worker_status = state
            .passive_readiness_worker
            .as_ref()
            .map(AutomarkerPassiveReadinessWorker::sanitized_status);
        #[cfg(not(windows))]
        let worker_status: Option<()> = None;
        #[cfg(windows)]
        let worker_readiness_proven = matches!(
            worker_status,
            Some(AutomarkerPassiveReadinessStatus::NativeReadinessProven)
        );
        #[cfg(not(windows))]
        let worker_readiness_proven = false;
        let retained_readiness_proven = state.gates.exact_local_process
            && state.gates.exact_syn_owned_tuple_epoch
            && state.gates.pinned_backend
            && state.gates.checksum_helper_ready;
        // A worker milestone is only an uncorrelated candidate. Never expose
        // it as readiness until SignatureFlowCapture and the parser carrier
        // have both matched the exact tuple.
        let native_readiness_proven = retained_readiness_proven;
        let marker_carrier_present = state
            .parser_evidence
            .as_ref()
            .is_some_and(|evidence| evidence.outbound_carrier.is_some());
        #[cfg(windows)]
        let failure = state.native_failure.or(match worker_status {
            Some(AutomarkerPassiveReadinessStatus::Failed(category)) => Some(category),
            _ => None,
        });
        #[cfg(windows)]
        let failure_present = failure.is_some();
        #[cfg(not(windows))]
        let failure_category = None;
        #[cfg(windows)]
        let failure_category = failure.map(AutomarkerNativeFailureCategory::as_str);
        #[cfg(windows)]
        let observer_ready = !failure_present
            && (state.passive_readiness_worker.is_some()
                || state.pending_passive_readiness.is_some()
                || native_readiness_proven);
        #[cfg(not(windows))]
        let observer_ready = false;
        #[cfg(windows)]
        let syn_candidate_observed = worker_readiness_proven
            || state.pending_passive_readiness.is_some()
            || native_readiness_proven;
        #[cfg(not(windows))]
        let syn_candidate_observed = native_readiness_proven;
        #[cfg(windows)]
        let bpsr_tuple_confirmed = state
            .pending_passive_readiness
            .as_ref()
            .is_some_and(|pending| pending.capture_tuple_confirmed)
            || native_readiness_proven;
        #[cfg(not(windows))]
        let bpsr_tuple_confirmed = native_readiness_proven;
        AutomarkerNativeOperatorStatus {
            // These are independent, sanitized milestones rather than an
            // overloaded state label. The UI can therefore show precisely
            // how far the passive proof progressed without exposing a PID,
            // socket tuple, connection epoch, packet bytes, or call identity.
            observer_ready,
            syn_candidate_observed,
            bpsr_tuple_confirmed,
            marker_carrier_observed: marker_carrier_present,
            return_confirmed: state
                .parser_evidence
                .as_ref()
                .is_some_and(|evidence| evidence.correlated_return.is_some()),
            active_placement_enabled: Self::placement_enabled_locked(&state),
            failure_category,
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
        Self::request_active_stop_locked(&mut state, false);
        if state.active_worker.is_some() || state.fatal_ownership.is_some() {
            return true;
        }
        let detached = Self::clear_scene_bound_preserving_passive(&mut state);
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
        Self::request_active_stop_locked(&mut state, false);
        if state.active_worker.is_some() || state.fatal_ownership.is_some() {
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
        let Some(mut state) = self.lock_or_poison_shutdown() else {
            return false;
        };
        #[cfg(windows)]
        if state.passive_readiness_worker.is_some() {
            return false;
        }
        #[cfg(windows)]
        if let Some(pending) = state.pending_passive_readiness.as_mut() {
            if pending.received_at.elapsed() > PASSIVE_READINESS_CANDIDATE_TTL {
                state.pending_passive_readiness = None;
                return true;
            }
            let connection = pending.observation.readiness.binding.connection();
            let matched = confirmed.iter().any(|candidate| {
                candidate.client.address == std::net::IpAddr::V4(connection.local.address)
                    && candidate.client.port == connection.local.port
                    && candidate.server.address == std::net::IpAddr::V4(connection.remote.address)
                    && candidate.server.port == connection.remote.port
            });
            if matched {
                pending.capture_tuple_confirmed = true;
                Self::try_promote_pending_readiness_locked(&mut state);
                return false;
            }
            state.pending_passive_readiness = None;
            return true;
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
            Self::collect_finished_worker_locked(&mut state);
            if state.active_worker.is_some() {
                state.shutdown_requested_while_active = true;
                Self::request_active_stop_locked(&mut state, false);
                if state.active_worker.is_some() || state.fatal_ownership.is_some() {
                    state.phase = LifecyclePhase::OwnershipRecoveryRequired;
                    return;
                }
            }
            if state.fatal_ownership.is_some() {
                state.shutdown_requested_while_active = true;
                state.phase = LifecyclePhase::OwnershipRecoveryRequired;
                return;
            }
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
        if self.active_lifetime_arbitration() != AutomarkerActiveLifetimeArbitration::Maintained {
            return false;
        }
        Self::placement_enabled_locked(&state)
    }

    fn placement_enabled_locked(state: &NativeBridgeState) -> bool {
        state.phase == LifecyclePhase::Observing
            && state.continuity.is_some()
            && state.coordinator.is_some()
            && state.active_worker.is_none()
            && state.fatal_ownership.is_none()
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
        Self::collect_finished_worker_locked(state);
        if state.active_worker.is_some() || state.fatal_ownership.is_some() {
            state.phase = LifecyclePhase::OwnershipRecoveryRequired;
            state.shutdown_requested_while_active |= phase == LifecyclePhase::Shutdown;
            return DetachedNativeResources::default();
        }
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
        state.active_control = None;
        #[cfg(windows)]
        {
            state.active_receive_control = None;
        }
        #[cfg(windows)]
        {
            state.native_failure = None;
            state.pending_passive_readiness = None;
        }
        DetachedNativeResources {
            #[cfg(windows)]
            passive_readiness_worker: state.passive_readiness_worker.take(),
            #[cfg(windows)]
            active_handle: state.active_handle.take(),
            coordinator: state.coordinator.take(),
        }
    }

    /// Scene presentation may arrive after the process-owned SYN. Clear only
    /// scene-bound correlation while retaining the one bounded passive worker
    /// or candidate inside the same capture-session generation.
    #[cfg(windows)]
    fn clear_scene_bound_preserving_passive(
        state: &mut NativeBridgeState,
    ) -> DetachedNativeResources {
        let worker = state.passive_readiness_worker.take();
        let pending = state.pending_passive_readiness.take();
        let native_flow = state.native_flow.take();
        let native_connection_gates = NativeGateState {
            exact_local_process: state.gates.exact_local_process,
            exact_syn_owned_tuple_epoch: state.gates.exact_syn_owned_tuple_epoch,
            pinned_backend: state.gates.pinned_backend,
            checksum_helper_ready: state.gates.checksum_helper_ready,
            ..NativeGateState::default()
        };
        let mut detached = Self::detach_owned_state(state, LifecyclePhase::Observing, false);
        state.passive_readiness_worker = worker;
        state.pending_passive_readiness = pending.map(|mut pending| {
            pending.generation = state.generation;
            pending
        });
        state.native_flow = native_flow;
        state.gates = native_connection_gates;
        detached.passive_readiness_worker = None;
        detached
    }

    #[cfg(not(windows))]
    fn clear_scene_bound_preserving_passive(
        state: &mut NativeBridgeState,
    ) -> DetachedNativeResources {
        Self::detach_owned_state(state, LifecyclePhase::Observing, false)
    }

    #[cfg(windows)]
    fn try_promote_pending_readiness_locked(state: &mut NativeBridgeState) -> bool {
        let Some(pending) = state.pending_passive_readiness.as_ref() else {
            return false;
        };
        let Some(session) = state.session.as_ref() else {
            return false;
        };
        if pending.received_at.elapsed() > PASSIVE_READINESS_CANDIDATE_TTL
            || pending.generation != state.generation
            || pending.capture_session_id != session.capture_session_id
            || pending.process_id != pending.observation.readiness.binding.process_id()
            || pending.connection_epoch != pending.observation.readiness.binding.connection_epoch()
            || !pending.capture_tuple_confirmed
        {
            return false;
        }
        let observation = pending.observation;
        if !Self::retain_native_flow_locked(
            state,
            observation.readiness.binding,
            observation.syn_ordinal,
            observation.syn_observed_micros,
        ) {
            return false;
        }
        state.pending_passive_readiness = None;
        state.gates.pinned_backend = true;
        state.gates.checksum_helper_ready = true;
        state.gates.reflect_arbitrated = false;
        state.native_failure = None;
        true
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

impl Drop for AutomarkerNativeBridgeLifecycle {
    fn drop(&mut self) {
        let state = match self.state.get_mut() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        Self::collect_finished_worker_locked(state);
        if state.fatal_ownership.is_some() {
            eprintln!("fatal: Automarker lifecycle exited with retained interception ownership");
            std::process::abort();
        }
        let Some(mut worker) = state.active_worker.take() else {
            return;
        };
        match worker.stop_drain_join() {
            Ok(completion) if completion.fatal_ownership.is_none() => {}
            Ok(_) | Err(_) => {
                eprintln!(
                    "fatal: Automarker lifecycle could not drain active interception during teardown"
                );
                std::process::abort();
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
    use crate::automarker_active_worker::{
        ActiveAutomarkerBackend, ActiveAutomarkerCancelDisposition,
        ActiveAutomarkerClassificationMode, ActiveAutomarkerCommitDisposition,
        ActiveAutomarkerCoordinator, ActiveAutomarkerDisposition, ActiveAutomarkerPacket,
        ActiveAutomarkerSendOutcome, ActiveAutomarkerTimeoutDisposition, ActiveAutomarkerWake,
    };
    use crate::automarker_bridge_evidence::{
        AutomarkerBridgeOutboundCarrierEvidence, AutomarkerBridgeRecordProvenance,
    };
    use rlogs_game_bpsr::{
        AUTOMARKER_REQUEST_BUILD, AutomarkerIpv4Endpoint, DecoderKind, FragmentKind,
        MappingProvenance, PacketDirection, bind_offline_automarker_connection_epoch,
    };
    #[cfg(windows)]
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::{
        collections::VecDeque,
        net::Ipv4Addr,
        sync::{Arc, Mutex as TestMutex},
        thread,
        time::Duration,
    };

    fn state(
        bridge: &AutomarkerNativeBridgeLifecycle,
    ) -> std::sync::MutexGuard<'_, NativeBridgeState> {
        bridge.state.lock().expect("test lifecycle mutex")
    }

    #[derive(Clone, Default)]
    struct ActiveLifecycleHarness(Arc<TestMutex<VecDeque<Result<ActiveAutomarkerWake, String>>>>);

    impl ActiveLifecycleHarness {
        fn push(&self, wake: ActiveAutomarkerWake) {
            self.0.lock().unwrap().push_back(Ok(wake));
        }
    }

    struct ActiveLifecycleBackend(ActiveLifecycleHarness);

    impl ActiveAutomarkerBackend for ActiveLifecycleBackend {
        fn receive(&mut self) -> Result<ActiveAutomarkerWake, String> {
            self.0
                .0
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Ok(ActiveAutomarkerWake::Timeout))
        }

        fn prepare_modified_send(
            &mut self,
            _original: &ActiveAutomarkerPacket,
            _approved_changed_bytes: &[u8],
        ) -> Result<rlogs_game_bpsr::AutomarkerPacketSendPreparation, String> {
            Err("not used by lifecycle ownership test".to_owned())
        }

        fn send(&mut self, packet: &ActiveAutomarkerPacket) -> Result<usize, String> {
            Ok(packet.bytes.len())
        }

        fn close_interception(&mut self) -> Result<(), String> {
            Ok(())
        }
    }

    struct ActiveLifecycleCoordinator {
        obligation: bool,
    }

    impl ActiveAutomarkerCoordinator for ActiveLifecycleCoordinator {
        fn classify(
            &mut self,
            packet: &ActiveAutomarkerPacket,
            _mode: ActiveAutomarkerClassificationMode,
        ) -> ActiveAutomarkerDisposition {
            if packet.bytes == b"exact-rst" {
                self.obligation = false;
                ActiveAutomarkerDisposition::ConnectionTerminatedAfterPassThrough
            } else {
                ActiveAutomarkerDisposition::PassThrough
            }
        }

        fn cancel_after_checksum_failure(
            &mut self,
            _preparation_id: u64,
        ) -> ActiveAutomarkerCancelDisposition {
            ActiveAutomarkerCancelDisposition::AbortWithoutReinject
        }

        fn commit_send(
            &mut self,
            _preparation_id: u64,
            _outcome: ActiveAutomarkerSendOutcome,
        ) -> ActiveAutomarkerCommitDisposition {
            ActiveAutomarkerCommitDisposition::AbortWithoutReinject
        }

        fn record_modified_send_may_begin(
            &mut self,
            _preparation_id: u64,
        ) -> ActiveAutomarkerCommitDisposition {
            ActiveAutomarkerCommitDisposition::AbortWithoutReinject
        }

        fn observe_timeout(&mut self) -> ActiveAutomarkerTimeoutDisposition {
            ActiveAutomarkerTimeoutDisposition::Continue
        }

        fn rewrite_obligation_active(&self) -> bool {
            self.obligation
        }

        fn drain_unsent_originals(&mut self) -> Vec<ActiveAutomarkerPacket> {
            Vec::new()
        }

        fn discard_retransmission_ledger(&mut self) {}

        fn retained_bound_process(&self) -> Option<ActiveAutomarkerBoundProcess> {
            Some(ActiveAutomarkerBoundProcess {
                process_id: 42,
                connection_epoch: 9,
            })
        }

        fn observe_bound_process_terminated(
            &mut self,
            proof: ActiveAutomarkerBoundProcess,
        ) -> bool {
            if self.retained_bound_process() != Some(proof) {
                return false;
            }
            self.obligation = false;
            true
        }
    }

    fn active_lifecycle_packet(bytes: &[u8]) -> ActiveAutomarkerPacket {
        ActiveAutomarkerPacket {
            bytes: bytes.to_vec(),
            address: rlogs_game_bpsr::AutomarkerWinDivertAddress::from_opaque_bytes([0; 80]),
        }
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

    #[cfg(windows)]
    fn confirmed_connection() -> rlogs_capture::TcpConnection {
        rlogs_capture::TcpConnection::new(
            rlogs_capture::TcpEndpoint::new(
                std::net::IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)),
                50_000,
            ),
            rlogs_capture::TcpEndpoint::new(std::net::IpAddr::V4(Ipv4Addr::new(10, 0, 0, 3)), 443),
        )
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

    #[test]
    fn session_finish_retains_fatal_worker_until_exact_rst_is_drained() {
        let bridge = AutomarkerNativeBridgeLifecycle::default();
        bridge.begin_session(session());
        let harness = ActiveLifecycleHarness::default();
        harness.push(ActiveAutomarkerWake::EndOfStream);
        let worker = ActiveAutomarkerWorkerBundle::spawn(
            ActiveLifecycleBackend(harness.clone()),
            ActiveLifecycleCoordinator { obligation: true },
        )
        .unwrap();
        if let Err(worker) = bridge.retain_active_worker(worker) {
            // Preserve the owner even on a broken test setup so the safety
            // Drop path cannot mask the assertion with a process abort.
            std::mem::forget(worker);
            panic!("test lifecycle rejected its first active worker");
        }

        bridge.finish_session("capture-a");
        let started = Instant::now();
        let retained = loop {
            let status = bridge.poll_active_ownership();
            if matches!(
                status,
                AutomarkerActiveOwnershipStatus::RecoveryRequired { .. }
            ) {
                break status;
            }
            assert!(started.elapsed() < Duration::from_secs(1));
            thread::yield_now();
        };
        assert_eq!(
            retained,
            AutomarkerActiveOwnershipStatus::RecoveryRequired {
                bound_process: Some(ActiveAutomarkerBoundProcess {
                    process_id: 42,
                    connection_epoch: 9,
                }),
            }
        );
        {
            let snapshot = state(&bridge);
            assert_eq!(snapshot.phase, LifecyclePhase::OwnershipRecoveryRequired);
            assert!(snapshot.session.is_some());
            assert!(snapshot.fatal_ownership.is_some());
        }
        assert!(!bridge.placement_enabled());

        harness.push(ActiveAutomarkerWake::Packet(active_lifecycle_packet(
            b"exact-rst",
        )));
        assert_eq!(
            bridge
                .recover_active_ownership(ActiveAutomarkerFatalRecovery::DrainOneReceive)
                .unwrap(),
            AutomarkerActiveOwnershipStatus::Idle
        );
        let snapshot = state(&bridge);
        assert_eq!(snapshot.phase, LifecyclePhase::Shutdown);
        assert!(snapshot.session.is_none());
        assert!(snapshot.active_worker.is_none());
        assert!(snapshot.fatal_ownership.is_none());
    }

    #[test]
    fn session_finish_stops_drains_and_joins_clean_active_worker() {
        let bridge = AutomarkerNativeBridgeLifecycle::default();
        bridge.begin_session(session());
        let worker = ActiveAutomarkerWorkerBundle::spawn(
            ActiveLifecycleBackend(ActiveLifecycleHarness::default()),
            ActiveLifecycleCoordinator { obligation: false },
        )
        .unwrap();
        bridge
            .retain_active_worker(worker)
            .unwrap_or_else(|worker| {
                std::mem::forget(worker);
                panic!("test lifecycle rejected its clean worker")
            });

        bridge.finish_session("capture-a");
        let snapshot = state(&bridge);
        assert_eq!(snapshot.phase, LifecyclePhase::Shutdown);
        assert!(snapshot.session.is_none());
        assert!(snapshot.active_worker.is_none());
        assert!(snapshot.fatal_ownership.is_none());
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
        assert_eq!(snapshot.phase, LifecyclePhase::Observing);
        assert!(snapshot.session.is_some());
        assert!(snapshot.continuity.is_none());
        assert!(snapshot.coordinator.is_none());
        assert_eq!(snapshot.gates, NativeGateState::default());
    }

    #[cfg(windows)]
    #[test]
    fn context_change_preserves_pre_correlation_passive_worker() {
        let bridge = AutomarkerNativeBridgeLifecycle::default();
        bridge.begin_session(session());
        assert!(bridge.accept_parser_evidence(Some(&scene("mech-facility")), parser_evidence()));
        let joined = install_completed_worker(&bridge, Ok(passive_observation()));
        assert!(bridge.reconcile_context(Some(&scene("sea-ringed-reef"))));
        let snapshot = state(&bridge);
        assert_eq!(snapshot.phase, LifecyclePhase::Observing);
        assert!(snapshot.passive_readiness_worker.is_some());
        drop(snapshot);
        bridge.finish_session("capture-a");
        assert!(joined.load(Ordering::SeqCst));
    }

    #[test]
    fn no_send_before_all_gates_or_before_packet_loop_is_reviewed() {
        let bridge = AutomarkerNativeBridgeLifecycle::default();
        assert_eq!(
            bridge.active_lifetime_arbitration(),
            AutomarkerActiveLifetimeArbitration::Maintained
        );
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
        {
            let mut snapshot = state(&bridge);
            snapshot.gates.pinned_backend = true;
            snapshot.gates.checksum_helper_ready = true;
            let config = AutomarkerNativeBridgeLifecycle::active_coordinator_config_locked(
                &snapshot, &point,
            )
            .expect("exact private canary configuration");
            assert_eq!(config.session_key, "capture-a");
            assert_eq!(config.connection_binding, binding(443));
            assert_eq!(config.carrier_capture_sequence, 30);
            assert_eq!(config.marker_number, 1);
            assert_eq!(config.target_position.x.to_bits(), point.x.to_bits());
        }
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
        let joined = install_completed_worker(&bridge, Ok(passive_observation()));
        assert!(bridge.poll_passive_readiness_worker());
        assert!(joined.load(Ordering::SeqCst));
        let candidate_status = bridge.sanitized_operator_status();
        assert!(candidate_status.observer_ready);
        assert!(candidate_status.syn_candidate_observed);
        assert!(!candidate_status.bpsr_tuple_confirmed);
        assert!(!candidate_status.marker_carrier_observed);
        assert!(!bridge.confirmed_connections_require_restart(&[confirmed_connection()]));
        assert!(bridge.sanitized_operator_status().bpsr_tuple_confirmed);
        assert!(state(&bridge).parser_evidence.is_none());
        assert!(bridge.accept_parser_evidence(Some(&scene("mech-facility")), parser_evidence()));
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
    fn tuple_promoted_readiness_survives_scene_discovery_until_later_carrier() {
        let bridge = AutomarkerNativeBridgeLifecycle::default();
        bridge.begin_session(session());
        let joined = install_completed_worker(&bridge, Ok(passive_observation()));
        assert!(bridge.poll_passive_readiness_worker());
        assert!(joined.load(Ordering::SeqCst));
        assert!(!bridge.confirmed_connections_require_restart(&[confirmed_connection()]));
        assert!(bridge.sanitized_operator_status().bpsr_tuple_confirmed);

        // Scene presentation and carrier framing are independent and may be
        // delayed arbitrarily relative to the connection SYN.
        assert!(bridge.reconcile_context(Some(&scene("mech-facility"))));
        assert!(bridge.sanitized_operator_status().bpsr_tuple_confirmed);
        assert!(bridge.accept_parser_evidence(
            Some(&scene("mech-facility")),
            AutomarkerBridgeEvidenceSnapshot::default(),
        ));
        let before_carrier = bridge.sanitized_operator_status();
        assert!(before_carrier.bpsr_tuple_confirmed);
        assert!(!before_carrier.marker_carrier_observed);

        assert!(bridge.accept_parser_evidence(Some(&scene("mech-facility")), parser_evidence()));
        let snapshot = state(&bridge);
        assert_eq!(
            snapshot
                .native_flow
                .as_ref()
                .unwrap()
                .capture_connection
                .capture_connection_id,
            50
        );
        assert!(snapshot.gates.exact_syn_owned_tuple_epoch);
        assert!(!snapshot.gates.fresh_world_use_slot_carrier);
    }

    #[cfg(windows)]
    #[test]
    fn unrelated_process_owned_syn_candidate_is_rejected_before_parser_correlation() {
        let bridge = AutomarkerNativeBridgeLifecycle::default();
        bridge.begin_session(session());
        let joined = install_completed_worker(&bridge, Ok(passive_observation()));
        assert!(bridge.poll_passive_readiness_worker());
        assert!(joined.load(Ordering::SeqCst));

        let unrelated = rlogs_capture::TcpConnection::new(
            rlogs_capture::TcpEndpoint::new(
                std::net::IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)),
                50_001,
            ),
            rlogs_capture::TcpEndpoint::new(std::net::IpAddr::V4(Ipv4Addr::new(10, 0, 0, 4)), 443),
        );
        assert!(bridge.confirmed_connections_require_restart(&[unrelated]));
        let snapshot = state(&bridge);
        assert!(snapshot.pending_passive_readiness.is_none());
        assert!(snapshot.native_flow.is_none());
        assert_eq!(snapshot.gates, NativeGateState::default());
        drop(snapshot);
        assert!(!bridge.placement_enabled());
    }

    #[cfg(windows)]
    #[test]
    fn operator_status_reports_readiness_before_consuming_the_worker_result() {
        let bridge = AutomarkerNativeBridgeLifecycle::default();
        bridge.begin_session(session());
        assert!(bridge.accept_parser_evidence(
            Some(&scene("mech-facility")),
            AutomarkerBridgeEvidenceSnapshot::default(),
        ));
        let joined = install_completed_worker(&bridge, Ok(passive_observation()));
        let status = bridge.sanitized_operator_status();
        assert!(status.observer_ready);
        assert!(status.syn_candidate_observed);
        assert!(!status.bpsr_tuple_confirmed);
        assert!(!status.marker_carrier_observed);
        assert!(!status.return_confirmed);
        assert!(!status.active_placement_enabled);
        assert_eq!(status.failure_category, None);
        bridge.finish_session("capture-a");
        assert!(joined.load(Ordering::SeqCst));
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
        let status = bridge.sanitized_operator_status();
        assert_eq!(status.failure_category, Some("internal"));
        assert!(!status.active_placement_enabled);
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
