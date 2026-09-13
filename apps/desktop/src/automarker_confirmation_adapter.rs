//! Process-private bridge between parser confirmation evidence and the pure
//! Automarker coordinator.
//!
//! This adapter owns no socket, driver, capture handle, or serialization
//! surface. One instance is permanently bound to one already-prepared canary
//! and one capture session. The router is the sole allocator for baseline,
//! rewrite, parser-event, and TCP-observation timestamps.

#![allow(dead_code)]

use rlogs_game_bpsr::{
    AutomarkerBridgeCoordinator, AutomarkerBridgeCoordinatorError, AutomarkerBridgeObservation,
    AutomarkerBridgeState, AutomarkerConfirmationContext, AutomarkerConfirmationMarkerAdd,
    AutomarkerConfirmationObservationStamp, AutomarkerConfirmationRpcReturn,
    AutomarkerConfirmationState, AutomarkerConfirmationTcpObservation,
    AutomarkerConfirmationTcpTuple, AutomarkerRequestXyz, SingleMarkerXyzCanaryContext,
};

use crate::automarker_confirmation_router::{
    AutomarkerConfirmationRouter, ConfirmationRouterError, OwnedConfirmationContext,
    OwnedConfirmationEvent, OwnedConfirmationEventKind, OwnedConfirmationStamp,
    PrivateParserConfirmationEvent, PrivateParserConfirmationSnapshot,
};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PrivateAutomarkerConfirmationBinding {
    pub session_key: String,
    pub game_build: String,
    pub scene_family: String,
    pub local_actor_id: i64,
    pub connection_epoch: u64,
    pub client_address: [u8; 4],
    pub client_port: u16,
    pub server_address: [u8; 4],
    pub server_port: u16,
    pub marker_number: u8,
    pub target_position: AutomarkerRequestXyz,
    pub carrier_capture_sequence: u64,
    pub carrier_rpc_call_id: u32,
    pub mapped_tcp_sequence_start: u32,
    pub mapped_tcp_length: u32,
    pub baseline_runtime_revision: u64,
    pub rewrite_runtime_revision: u64,
}

impl PrivateAutomarkerConfirmationBinding {
    fn valid(&self) -> bool {
        !self.session_key.trim().is_empty()
            && !self.game_build.trim().is_empty()
            && !self.scene_family.trim().is_empty()
            && self.local_actor_id != 0
            && self.connection_epoch != 0
            && self.client_address != [0; 4]
            && self.server_address != [0; 4]
            && self.client_port != 0
            && self.server_port != 0
            && (1..=6).contains(&self.marker_number)
            && self.target_position.x.is_finite()
            && self.target_position.y.is_finite()
            && self.target_position.z.is_finite()
            && self.carrier_capture_sequence != 0
            && self.carrier_rpc_call_id != 0
            && self.mapped_tcp_length != 0
            && self.baseline_runtime_revision != 0
            && self.rewrite_runtime_revision >= self.baseline_runtime_revision
    }

    fn context(
        &self,
        runtime_revision: u64,
        observed_micros: u64,
    ) -> AutomarkerConfirmationContext {
        AutomarkerConfirmationContext {
            game_build: self.game_build.clone(),
            scene_family: self.scene_family.clone(),
            local_actor_id: self.local_actor_id,
            connection_epoch: self.connection_epoch,
            client_to_server_tuple: AutomarkerConfirmationTcpTuple {
                client_address: self.client_address,
                client_port: self.client_port,
                server_address: self.server_address,
                server_port: self.server_port,
            },
            runtime_revision,
            observed_micros,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrivateAutomarkerCoordinatorClockInputs {
    pub baseline_context: AutomarkerConfirmationContext,
    pub baseline_stamp: OwnedConfirmationStamp,
    pub rewrite_context: AutomarkerConfirmationContext,
    pub rewrite_stamp: OwnedConfirmationStamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PrivateAutomarkerTcpObservation {
    pub connection_epoch: u64,
    pub source_address: [u8; 4],
    pub source_port: u16,
    pub destination_address: [u8; 4],
    pub destination_port: u16,
    pub ack_flag: bool,
    pub cumulative_ack: u32,
    pub fin: bool,
    pub rst: bool,
    pub runtime_revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrivateAutomarkerConfirmationAdapterState {
    Active,
    ConfirmationFailedAwaitingTransportRetirement,
    Complete,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrivateAutomarkerConfirmationAdapterError {
    InvalidBinding,
    CoordinatorNotAwaitingConfirmation,
    CoordinatorBindingMismatch,
    Router(ConfirmationRouterError),
    SessionOrContextChanged,
    FilteredOrReplayedBatch,
    ObservationOrderChanged,
    EventAfterTerminal,
    Coordinator(AutomarkerBridgeCoordinatorError),
}

/// Owns the only router clock and the exact coordinator consuming its stamps.
pub(crate) struct PrivateAutomarkerConfirmationAdapter {
    router: AutomarkerConfirmationRouter,
    coordinator: AutomarkerBridgeCoordinator,
    binding: PrivateAutomarkerConfirmationBinding,
    rewrite_stamp: OwnedConfirmationStamp,
    last_stamp: OwnedConfirmationStamp,
    state: PrivateAutomarkerConfirmationAdapterState,
}

impl PrivateAutomarkerConfirmationAdapter {
    /// The builder receives baseline/rewrite values allocated by the router.
    /// It must use them while preparing the real coordinator. The returned
    /// coordinator is accepted only if its rewrite has started confirmation
    /// and retains the transport obligation.
    pub(crate) fn begin(
        binding: PrivateAutomarkerConfirmationBinding,
        prepare: impl FnOnce(
            &PrivateAutomarkerCoordinatorClockInputs,
        )
            -> Result<AutomarkerBridgeCoordinator, AutomarkerBridgeCoordinatorError>,
    ) -> Result<Self, PrivateAutomarkerConfirmationAdapterError> {
        if !binding.valid() {
            return Err(PrivateAutomarkerConfirmationAdapterError::InvalidBinding);
        }
        let mut router = AutomarkerConfirmationRouter::begin_after_carrier(
            binding.session_key.clone(),
            binding.marker_number,
            binding.carrier_capture_sequence,
        )
        .ok_or(PrivateAutomarkerConfirmationAdapterError::InvalidBinding)?;
        let baseline_stamp = router
            .stamp_now()
            .map_err(PrivateAutomarkerConfirmationAdapterError::Router)?;
        let rewrite_stamp = router
            .stamp_now()
            .map_err(PrivateAutomarkerConfirmationAdapterError::Router)?;
        let inputs = PrivateAutomarkerCoordinatorClockInputs {
            baseline_context: binding.context(
                binding.baseline_runtime_revision,
                baseline_stamp.observed_micros,
            ),
            baseline_stamp,
            rewrite_context: binding.context(
                binding.rewrite_runtime_revision,
                rewrite_stamp.observed_micros,
            ),
            rewrite_stamp,
        };
        let coordinator =
            prepare(&inputs).map_err(PrivateAutomarkerConfirmationAdapterError::Coordinator)?;
        let coordinator_state = coordinator.state();
        if !matches!(
            coordinator_state.confirmation,
            Some(AutomarkerConfirmationState::AwaitingEvidence { .. })
        ) || !coordinator_state.tcp_rewrite_obligation_active
            || coordinator_state.coordinator_error.is_some()
        {
            return Err(
                PrivateAutomarkerConfirmationAdapterError::CoordinatorNotAwaitingConfirmation,
            );
        }
        let Some(coordinator_binding) = coordinator.confirmation_binding() else {
            return Err(PrivateAutomarkerConfirmationAdapterError::CoordinatorBindingMismatch);
        };
        if coordinator_binding.marker_number != binding.marker_number
            || !same_position_bits(coordinator_binding.target_position, binding.target_position)
            || coordinator_binding.baseline_context != inputs.baseline_context
            || coordinator_binding.baseline_observation_ordinal
                != inputs.baseline_stamp.observation_ordinal
            || coordinator_binding.rewrite_context != inputs.rewrite_context
            || coordinator_binding.rewrite_observation_ordinal
                != inputs.rewrite_stamp.observation_ordinal
            || coordinator_binding.original_rpc_call_id != binding.carrier_rpc_call_id
            || coordinator_binding.mapped_tcp_sequence_start != binding.mapped_tcp_sequence_start
            || coordinator_binding.mapped_tcp_length != binding.mapped_tcp_length
        {
            return Err(PrivateAutomarkerConfirmationAdapterError::CoordinatorBindingMismatch);
        }
        Ok(Self {
            router,
            coordinator,
            binding,
            rewrite_stamp,
            last_stamp: rewrite_stamp,
            state: PrivateAutomarkerConfirmationAdapterState::Active,
        })
    }

    pub(crate) fn state(&self) -> PrivateAutomarkerConfirmationAdapterState {
        self.state
    }

    pub(crate) fn coordinator_state(&self) -> AutomarkerBridgeState {
        self.coordinator.state()
    }

    pub(crate) fn route_parser_snapshot(
        &mut self,
        snapshot: PrivateParserConfirmationSnapshot,
    ) -> Result<AutomarkerBridgeState, PrivateAutomarkerConfirmationAdapterError> {
        self.require_active()?;
        if !self.snapshot_context_matches(&snapshot) {
            return self.fail(PrivateAutomarkerConfirmationAdapterError::SessionOrContextChanged);
        }
        if snapshot.events.iter().any(|event| match event {
            PrivateParserConfirmationEvent::CorrelatedReturn(event) => {
                event.carrier_capture_sequence != self.binding.carrier_capture_sequence
                    || event.provenance.call_id != Some(self.binding.carrier_rpc_call_id)
            }
            PrivateParserConfirmationEvent::MarkerAdd(_) => false,
        }) {
            return self.fail(PrivateAutomarkerConfirmationAdapterError::SessionOrContextChanged);
        }
        let expected_count = snapshot.events.len();
        let routed = match self.router.route_snapshot(snapshot) {
            Ok(routed) => routed,
            Err(reason) => {
                return self.fail(PrivateAutomarkerConfirmationAdapterError::Router(reason));
            }
        };
        // The router intentionally filters unrelated evidence and silently
        // deduplicates exact replay. Neither behavior is acceptable once a
        // batch has been explicitly handed to this single-canary adapter.
        if routed.len() != expected_count {
            return self.fail(PrivateAutomarkerConfirmationAdapterError::FilteredOrReplayedBatch);
        }
        for event in routed {
            self.observe_owned_event(event)?;
        }
        Ok(self.coordinator.state())
    }

    pub(crate) fn observe_tcp(
        &mut self,
        observation: PrivateAutomarkerTcpObservation,
    ) -> Result<AutomarkerBridgeState, PrivateAutomarkerConfirmationAdapterError> {
        self.require_lifecycle_open()?;
        if observation.connection_epoch != self.binding.connection_epoch
            || observation.source_address != self.binding.server_address
            || observation.source_port != self.binding.server_port
            || observation.destination_address != self.binding.client_address
            || observation.destination_port != self.binding.client_port
        {
            return self.fail(PrivateAutomarkerConfirmationAdapterError::SessionOrContextChanged);
        }
        let stamp = match self.router.stamp_now() {
            Ok(stamp) => stamp,
            Err(reason) => {
                return self.fail(PrivateAutomarkerConfirmationAdapterError::Router(reason));
            }
        };
        self.validate_stamp(stamp)?;
        let confirmation_stamp = self.confirmation_stamp(stamp)?;
        let context = self
            .binding
            .context(observation.runtime_revision, stamp.observed_micros);
        let canary_context = SingleMarkerXyzCanaryContext {
            game_build: &self.binding.game_build,
            current_scene_family: &self.binding.scene_family,
            runtime_revision: observation.runtime_revision,
            observation_monotonic_millis: stamp.observed_micros.saturating_add(999) / 1_000,
            observation_age_millis: 0,
        };
        let result = self.coordinator.observe(AutomarkerBridgeObservation::Tcp {
            canary_context,
            confirmation_context: &context,
            observation: AutomarkerConfirmationTcpObservation {
                connection_epoch: observation.connection_epoch,
                source_address: observation.source_address,
                source_port: observation.source_port,
                destination_address: observation.destination_address,
                destination_port: observation.destination_port,
                ack_flag: observation.ack_flag,
                cumulative_ack: observation.cumulative_ack,
                fin: observation.fin,
                rst: observation.rst,
                stamp: confirmation_stamp,
            },
        });
        self.finish_observation(result)
    }

    pub(crate) fn observe_timeout(
        &mut self,
    ) -> Result<AutomarkerBridgeState, PrivateAutomarkerConfirmationAdapterError> {
        self.require_lifecycle_open()?;
        let stamp = match self.router.stamp_now() {
            Ok(stamp) => stamp,
            Err(reason) => {
                return self.fail(PrivateAutomarkerConfirmationAdapterError::Router(reason));
            }
        };
        self.validate_stamp(stamp)?;
        let elapsed_since_rewrite_millis = stamp
            .observed_micros
            .checked_sub(self.rewrite_stamp.observed_micros)
            .ok_or(PrivateAutomarkerConfirmationAdapterError::ObservationOrderChanged)?
            / 1_000;
        let result = self
            .coordinator
            .observe(AutomarkerBridgeObservation::Timeout {
                elapsed_since_rewrite_millis,
                current_observed_micros: stamp.observed_micros,
            });
        self.finish_observation(result)
    }

    pub(crate) fn observe_connection_terminated(
        &mut self,
        connection_epoch: u64,
    ) -> Result<AutomarkerBridgeState, PrivateAutomarkerConfirmationAdapterError> {
        self.require_lifecycle_open()?;
        if connection_epoch != self.binding.connection_epoch {
            return self.fail(PrivateAutomarkerConfirmationAdapterError::SessionOrContextChanged);
        }
        let stamp = match self.router.stamp_now() {
            Ok(stamp) => stamp,
            Err(reason) => {
                return self.fail(PrivateAutomarkerConfirmationAdapterError::Router(reason));
            }
        };
        self.validate_stamp(stamp)?;
        self.state = PrivateAutomarkerConfirmationAdapterState::
            ConfirmationFailedAwaitingTransportRetirement;
        let result = self
            .coordinator
            .observe(AutomarkerBridgeObservation::ConnectionTerminated { connection_epoch });
        self.finish_observation(result)
    }

    fn observe_owned_event(
        &mut self,
        event: OwnedConfirmationEvent,
    ) -> Result<(), PrivateAutomarkerConfirmationAdapterError> {
        if !self.owned_context_matches(&event.context) {
            return self.fail(PrivateAutomarkerConfirmationAdapterError::SessionOrContextChanged);
        }
        self.validate_stamp(event.stamp)?;
        let stamp = self.confirmation_stamp(event.stamp)?;
        let context = self.binding.context(
            event.context.runtime_revision,
            event.context.observed_micros,
        );
        let result = match event.kind {
            OwnedConfirmationEventKind::RpcReturnCandidate(candidate) => {
                if !candidate.route_resolved {
                    // Preserve the malformed route as a coordinator-rejected
                    // zero method rather than silently dropping it.
                }
                self.coordinator
                    .observe(AutomarkerBridgeObservation::RpcReturn {
                        context: &context,
                        observation: AutomarkerConfirmationRpcReturn {
                            method_id: candidate.method_id,
                            original_call_id: candidate.original_call_id,
                            asserted_decoded_from_authoritative_server_stream: candidate
                                .asserted_authoritative_server_decode,
                            decoded_as_success: candidate.decoded_as_success,
                            decoded_body_length: candidate.decoded_body_length,
                            stamp,
                        },
                    })
            }
            OwnedConfirmationEventKind::MarkerAddCandidate(candidate) => {
                if candidate.runtime_revision != event.context.runtime_revision
                    || candidate.observed_micros != event.context.observed_micros
                    || candidate.marker_number != self.binding.marker_number
                    || candidate.marker_owner_actor_id != self.binding.local_actor_id
                    || !same_position_bits(
                        AutomarkerRequestXyz {
                            x: candidate.position.x,
                            y: candidate.position.y,
                            z: candidate.position.z,
                        },
                        self.binding.target_position,
                    )
                {
                    return self
                        .fail(PrivateAutomarkerConfirmationAdapterError::SessionOrContextChanged);
                }
                self.coordinator
                    .observe(AutomarkerBridgeObservation::AuthoritativeMarkerAdd {
                        context: &context,
                        observation: AutomarkerConfirmationMarkerAdd {
                            method_id: candidate.method_id,
                            marker_number: candidate.marker_number,
                            marker_owner_actor_id: candidate.marker_owner_actor_id,
                            position: AutomarkerRequestXyz {
                                x: candidate.position.x,
                                y: candidate.position.y,
                                z: candidate.position.z,
                            },
                            passive_instance_identity: candidate.passive_instance_identity,
                            asserted_decoded_from_authoritative_server_stream: candidate
                                .asserted_authoritative_server_decode,
                            runtime_revision: candidate.runtime_revision,
                            observed_micros: candidate.observed_micros,
                            stamp,
                        },
                    })
            }
        };
        self.finish_observation(result).map(|_| ())
    }

    fn snapshot_context_matches(&self, snapshot: &PrivateParserConfirmationSnapshot) -> bool {
        snapshot.session_key == self.binding.session_key
            && snapshot.context.game_build == self.binding.game_build
            && snapshot.context.scene_family == self.binding.scene_family
            && snapshot.context.local_actor_id == self.binding.local_actor_id
            && snapshot.context.connection_epoch == self.binding.connection_epoch
            && snapshot.context.client_address == self.binding.client_address
            && snapshot.context.client_port == self.binding.client_port
            && snapshot.context.server_address == self.binding.server_address
            && snapshot.context.server_port == self.binding.server_port
            && snapshot.context.runtime_revision >= self.binding.rewrite_runtime_revision
    }

    fn owned_context_matches(&self, context: &OwnedConfirmationContext) -> bool {
        context.game_build == self.binding.game_build
            && context.scene_family == self.binding.scene_family
            && context.local_actor_id == self.binding.local_actor_id
            && context.connection_epoch == self.binding.connection_epoch
            && context.client_address == self.binding.client_address
            && context.client_port == self.binding.client_port
            && context.server_address == self.binding.server_address
            && context.server_port == self.binding.server_port
            && context.runtime_revision >= self.binding.rewrite_runtime_revision
            && context.observed_micros > self.rewrite_stamp.observed_micros
    }

    fn validate_stamp(
        &mut self,
        stamp: OwnedConfirmationStamp,
    ) -> Result<(), PrivateAutomarkerConfirmationAdapterError> {
        if stamp.observation_ordinal <= self.last_stamp.observation_ordinal
            || stamp.observed_micros <= self.last_stamp.observed_micros
        {
            return self.fail(PrivateAutomarkerConfirmationAdapterError::ObservationOrderChanged);
        }
        self.last_stamp = stamp;
        Ok(())
    }

    fn confirmation_stamp(
        &mut self,
        stamp: OwnedConfirmationStamp,
    ) -> Result<AutomarkerConfirmationObservationStamp, PrivateAutomarkerConfirmationAdapterError>
    {
        let Some(elapsed_micros) = stamp
            .observed_micros
            .checked_sub(self.rewrite_stamp.observed_micros)
        else {
            return self.fail(PrivateAutomarkerConfirmationAdapterError::ObservationOrderChanged);
        };
        Ok(AutomarkerConfirmationObservationStamp {
            observation_ordinal: stamp.observation_ordinal,
            observed_micros: stamp.observed_micros,
            elapsed_since_rewrite_millis: elapsed_micros / 1_000,
        })
    }

    fn finish_observation(
        &mut self,
        result: Result<AutomarkerBridgeState, AutomarkerBridgeCoordinatorError>,
    ) -> Result<AutomarkerBridgeState, PrivateAutomarkerConfirmationAdapterError> {
        let state = match result {
            Ok(state) => state,
            Err(reason) => {
                let obligation_active = self.coordinator.state().tcp_rewrite_obligation_active;
                self.state = if obligation_active {
                    PrivateAutomarkerConfirmationAdapterState::
                        ConfirmationFailedAwaitingTransportRetirement
                } else {
                    PrivateAutomarkerConfirmationAdapterState::Failed
                };
                return self.fail(PrivateAutomarkerConfirmationAdapterError::Coordinator(
                    reason,
                ));
            }
        };
        let failure_is_sticky = self.state
            == PrivateAutomarkerConfirmationAdapterState::
                ConfirmationFailedAwaitingTransportRetirement;
        if !failure_is_sticky
            && matches!(
                state.confirmation,
                Some(AutomarkerConfirmationState::Confirmed)
            )
            && !state.tcp_rewrite_obligation_active
        {
            self.state = PrivateAutomarkerConfirmationAdapterState::Complete;
        } else if !state.tcp_rewrite_obligation_active
            && (failure_is_sticky
                || matches!(
                    state.confirmation,
                    Some(AutomarkerConfirmationState::Aborted(_))
                )
                || state.coordinator_error.is_some())
        {
            self.state = PrivateAutomarkerConfirmationAdapterState::Failed;
        } else if failure_is_sticky
            || matches!(
                state.confirmation,
                Some(AutomarkerConfirmationState::Aborted(_))
            )
            || state.coordinator_error.is_some()
        {
            self.state = PrivateAutomarkerConfirmationAdapterState::
                ConfirmationFailedAwaitingTransportRetirement;
        }
        Ok(state)
    }

    fn require_active(&self) -> Result<(), PrivateAutomarkerConfirmationAdapterError> {
        if self.state == PrivateAutomarkerConfirmationAdapterState::Active {
            Ok(())
        } else {
            Err(PrivateAutomarkerConfirmationAdapterError::EventAfterTerminal)
        }
    }

    fn require_lifecycle_open(&self) -> Result<(), PrivateAutomarkerConfirmationAdapterError> {
        if matches!(
            self.state,
            PrivateAutomarkerConfirmationAdapterState::Active
                | PrivateAutomarkerConfirmationAdapterState::
                    ConfirmationFailedAwaitingTransportRetirement
        ) {
            Ok(())
        } else {
            Err(PrivateAutomarkerConfirmationAdapterError::EventAfterTerminal)
        }
    }

    fn fail<T>(
        &mut self,
        reason: PrivateAutomarkerConfirmationAdapterError,
    ) -> Result<T, PrivateAutomarkerConfirmationAdapterError> {
        self.state = if self.coordinator.state().tcp_rewrite_obligation_active {
            PrivateAutomarkerConfirmationAdapterState::ConfirmationFailedAwaitingTransportRetirement
        } else {
            PrivateAutomarkerConfirmationAdapterState::Failed
        };
        Err(reason)
    }
}

fn same_position_bits(left: AutomarkerRequestXyz, right: AutomarkerRequestXyz) -> bool {
    left.x.to_bits() == right.x.to_bits()
        && left.y.to_bits() == right.y.to_bits()
        && left.z.to_bits() == right.z.to_bits()
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use rlogs_game_bpsr::{
        AUTOMARKER_REQUEST_BUILD, AutomarkerBridgeCommitDisposition,
        AutomarkerBridgePrepareDisposition, AutomarkerConfirmationBaseline, AutomarkerIpv4Endpoint,
        AutomarkerOwnedTcpConnection, AutomarkerWinDivertAddress, MappingProvenance, ProtocolPack,
        SINGLE_MARKER_XYZ_CANARY_ARM_TOKEN, SingleMarkerXyzCanaryConfig,
        SingleMarkerXyzExternalSendOutcome, bind_offline_automarker_connection_epoch,
    };

    use super::*;
    use crate::automarker_confirmation_router::{
        PrivateConfirmationContext, PrivateConfirmationProvenance, PrivateCorrelatedReturn,
        PrivateFragmentKind, PrivateMarkerAdd, PrivatePacketDirection,
        PrivateParserConfirmationEvent, PrivateSourceClocks,
    };

    const FRAME_SEQUENCE: u32 = 1_000;
    const FRAME_LENGTH: u32 = 197;
    const CARRIER_CAPTURE_SEQUENCE: u64 = 4;
    const CALL_ID: u32 = 0x1234_5678;
    const TARGET: AutomarkerRequestXyz = AutomarkerRequestXyz {
        x: 101.25,
        y: -22.5,
        z: 303.75,
    };
    const SYNTHETIC_FRAME_HEX: &str = concat!(
        "000000c500059abcdef0000000bb0001000000000626ad6600000001123456780003d002",
        "0a9e0108c90110011a4208a68f80800610cd08180120ee90eb99893432140d0000803f",
        "15000000401d0000404025000080403a140d0000a040150000c0401d0000e040250000",
        "00415001580122505a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a12aa4caa2ac8fdad32",
        "5e04f99bf00b0e4f41c1a16cc947339a988bdfdff23a17e44700729e452212f9a7468",
        "773c78838cbab163f32a51e9b458738f925635c7628df04"
    );

    fn binding() -> PrivateAutomarkerConfirmationBinding {
        PrivateAutomarkerConfirmationBinding {
            session_key: "capture-session-1".into(),
            game_build: AUTOMARKER_REQUEST_BUILD.into(),
            scene_family: "mech-facility".into(),
            local_actor_id: 77,
            connection_epoch: 9,
            client_address: [10, 0, 0, 2],
            client_port: 50_000,
            server_address: [10, 0, 0, 3],
            server_port: 443,
            marker_number: 1,
            target_position: TARGET,
            carrier_capture_sequence: CARRIER_CAPTURE_SEQUENCE,
            carrier_rpc_call_id: CALL_ID,
            mapped_tcp_sequence_start: FRAME_SEQUENCE,
            mapped_tcp_length: FRAME_LENGTH,
            baseline_runtime_revision: 100,
            rewrite_runtime_revision: 101,
        }
    }

    fn pack() -> ProtocolPack {
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

    fn frame() -> Vec<u8> {
        SYNTHETIC_FRAME_HEX
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }

    fn address() -> AutomarkerWinDivertAddress {
        let mut bytes = [0_u8; 80];
        bytes[8..12].copy_from_slice(&(1_u32 << 17).to_ne_bytes());
        AutomarkerWinDivertAddress::from_opaque_bytes(bytes)
    }

    fn canary_context<'a>(
        binding: &'a PrivateAutomarkerConfirmationBinding,
        revision: u64,
        micros: u64,
    ) -> SingleMarkerXyzCanaryContext<'a> {
        SingleMarkerXyzCanaryContext {
            game_build: &binding.game_build,
            current_scene_family: &binding.scene_family,
            runtime_revision: revision,
            observation_monotonic_millis: micros.saturating_add(999) / 1_000,
            observation_age_millis: 0,
        }
    }

    fn adapter() -> PrivateAutomarkerConfirmationAdapter {
        adapter_claiming(binding()).unwrap()
    }

    fn adapter_claiming(
        claimed_binding: PrivateAutomarkerConfirmationBinding,
    ) -> Result<PrivateAutomarkerConfirmationAdapter, PrivateAutomarkerConfirmationAdapterError>
    {
        let builder_binding = binding();
        PrivateAutomarkerConfirmationAdapter::begin(claimed_binding, move |clock| {
            let pack = pack();
            let connection = AutomarkerOwnedTcpConnection {
                process_id: 42,
                local: AutomarkerIpv4Endpoint {
                    address: Ipv4Addr::from(builder_binding.client_address),
                    port: builder_binding.client_port,
                },
                remote: AutomarkerIpv4Endpoint {
                    address: Ipv4Addr::from(builder_binding.server_address),
                    port: builder_binding.server_port,
                },
            };
            let epoch = bind_offline_automarker_connection_epoch(
                42,
                connection,
                builder_binding.connection_epoch,
                true,
                &[connection],
            )
            .unwrap();
            let mut coordinator = AutomarkerBridgeCoordinator::arm(
                SingleMarkerXyzCanaryConfig {
                    expected_scene_family: builder_binding.scene_family.clone(),
                    marker_number: builder_binding.marker_number,
                    target_position: builder_binding.target_position,
                },
                SINGLE_MARKER_XYZ_CANARY_ARM_TOKEN,
                &pack,
                epoch,
                canary_context(
                    &builder_binding,
                    builder_binding.baseline_runtime_revision,
                    clock.baseline_stamp.observed_micros,
                ),
                AutomarkerConfirmationBaseline {
                    context: clock.baseline_context.clone(),
                    observation_ordinal: clock.baseline_stamp.observation_ordinal,
                    same_number_passive_instance_identities: vec![70],
                },
            )?;
            let frame = frame();
            coordinator.observe_fresh_carrier(
                &pack,
                builder_binding.connection_epoch,
                FRAME_SEQUENCE,
                0,
                &frame,
                canary_context(
                    &builder_binding,
                    builder_binding.rewrite_runtime_revision,
                    clock.rewrite_stamp.observed_micros,
                ),
            )?;
            let mut packet = vec![0_u8; 40];
            packet.extend_from_slice(&frame);
            let preparation = coordinator.prepare(
                builder_binding.connection_epoch,
                FRAME_SEQUENCE,
                40,
                &packet,
                address(),
                canary_context(
                    &builder_binding,
                    builder_binding.rewrite_runtime_revision,
                    clock.rewrite_stamp.observed_micros,
                ),
                &clock.rewrite_context,
                clock.rewrite_stamp.observation_ordinal,
            );
            let AutomarkerBridgePrepareDisposition::NeedsChecksumRepair(preparation) = preparation
            else {
                panic!("expected prepared rewrite");
            };
            assert_eq!(
                coordinator.record_modified_send_may_begin(preparation.preparation_id),
                AutomarkerBridgeCommitDisposition::Committed
            );
            assert_eq!(
                coordinator.commit(
                    preparation.preparation_id,
                    SingleMarkerXyzExternalSendOutcome::Complete {
                        bytes_sent: packet.len(),
                    },
                ),
                AutomarkerBridgeCommitDisposition::Committed
            );
            Ok(coordinator)
        })
    }

    fn context(revision: u64) -> PrivateConfirmationContext {
        let binding = binding();
        PrivateConfirmationContext {
            game_build: binding.game_build,
            scene_family: binding.scene_family,
            local_actor_id: binding.local_actor_id,
            connection_epoch: binding.connection_epoch,
            client_address: binding.client_address,
            client_port: binding.client_port,
            server_address: binding.server_address,
            server_port: binding.server_port,
            runtime_revision: revision,
        }
    }

    fn provenance(
        capture_sequence: u64,
        fragment: PrivateFragmentKind,
        service_id: u64,
        method_id: u32,
        call_id: Option<u32>,
    ) -> PrivateConfirmationProvenance {
        PrivateConfirmationProvenance {
            capture_sequence,
            record_event_index: 0,
            connection_id: 31,
            stream_id: 7,
            direction: PrivatePacketDirection::ServerToClient,
            fragment,
            route_resolved: true,
            service_id,
            method_id,
            stub_id: 1,
            call_id,
        }
    }

    fn return_event(sequence: u64) -> PrivateParserConfirmationEvent {
        PrivateParserConfirmationEvent::CorrelatedReturn(PrivateCorrelatedReturn {
            provenance: provenance(
                sequence,
                PrivateFragmentKind::Return,
                103_198_054,
                249_858,
                Some(CALL_ID),
            ),
            source_clocks: PrivateSourceClocks {
                observed_micros: 999_999_999,
                wall_clock_unix_micros: Some(7),
            },
            carrier_capture_sequence: CARRIER_CAPTURE_SEQUENCE,
            raw_stub_id: 1,
            raw_status: 0,
            asserted_authoritative_server_decode: true,
            decoded_as_success: true,
            decoded_body_present: true,
            decoded_body_length: 0,
        })
    }

    fn marker_event(sequence: u64) -> PrivateParserConfirmationEvent {
        PrivateParserConfirmationEvent::MarkerAdd(PrivateMarkerAdd {
            provenance: provenance(
                sequence,
                PrivateFragmentKind::Notify,
                1_664_308_034,
                46,
                None,
            ),
            source_clocks: PrivateSourceClocks {
                observed_micros: 1,
                wall_clock_unix_micros: None,
            },
            asserted_authoritative_server_decode: true,
            raw_skill_id: Some(1_101),
            derived_marker_number: Some(1),
            marker_owner_actor_id: Some(77),
            marker_owner_entity_uuid: Some(8_888),
            passive_instance_identity: Some(71),
            target_position_present: true,
            target_position_decode_valid: true,
            x: Some(TARGET.x),
            y: Some(TARGET.y),
            z: Some(TARGET.z),
            runtime_revision: 103,
        })
    }

    fn snapshot(
        revision: u64,
        events: Vec<PrivateParserConfirmationEvent>,
    ) -> PrivateParserConfirmationSnapshot {
        PrivateParserConfirmationSnapshot {
            session_key: binding().session_key,
            context: context(revision),
            events,
        }
    }

    fn ack(revision: u64) -> PrivateAutomarkerTcpObservation {
        let binding = binding();
        PrivateAutomarkerTcpObservation {
            connection_epoch: binding.connection_epoch,
            source_address: binding.server_address,
            source_port: binding.server_port,
            destination_address: binding.client_address,
            destination_port: binding.client_port,
            ack_flag: true,
            cumulative_ack: FRAME_SEQUENCE + FRAME_LENGTH,
            fin: false,
            rst: false,
            runtime_revision: revision,
        }
    }

    #[test]
    fn success_requires_return_marker_and_transport_ack_from_one_router_clock() {
        let mut adapter = adapter();
        let rewrite_stamp = adapter.rewrite_stamp;
        adapter
            .route_parser_snapshot(snapshot(102, vec![return_event(10)]))
            .unwrap();
        assert_eq!(
            adapter.state(),
            PrivateAutomarkerConfirmationAdapterState::Active
        );
        adapter
            .route_parser_snapshot(snapshot(103, vec![marker_event(11)]))
            .unwrap();
        assert_eq!(
            adapter.state(),
            PrivateAutomarkerConfirmationAdapterState::Active
        );
        let before_ack = adapter.last_stamp;
        let state = adapter.observe_tcp(ack(103)).unwrap();
        assert_eq!(
            adapter.state(),
            PrivateAutomarkerConfirmationAdapterState::Complete
        );
        assert_eq!(
            state.confirmation,
            Some(AutomarkerConfirmationState::Confirmed)
        );
        assert!(!state.tcp_rewrite_obligation_active);
        assert!(before_ack.observation_ordinal > rewrite_stamp.observation_ordinal);
        assert!(adapter.last_stamp.observation_ordinal > before_ack.observation_ordinal);
        assert!(adapter.last_stamp.observed_micros > before_ack.observed_micros);
        assert_eq!(
            adapter.observe_tcp(ack(103)),
            Err(PrivateAutomarkerConfirmationAdapterError::EventAfterTerminal)
        );
    }

    #[test]
    fn prepared_coordinator_must_match_claimed_marker_carrier_and_transport_binding() {
        for case in 0..5 {
            let mut claimed = binding();
            match case {
                0 => claimed.marker_number = 2,
                1 => claimed.target_position.x = f32::from_bits(TARGET.x.to_bits() + 1),
                2 => claimed.carrier_rpc_call_id += 1,
                3 => claimed.mapped_tcp_sequence_start += 1,
                4 => claimed.mapped_tcp_length += 1,
                _ => unreachable!(),
            }
            assert!(matches!(
                adapter_claiming(claimed),
                Err(PrivateAutomarkerConfirmationAdapterError::CoordinatorBindingMismatch)
            ));
        }
    }

    #[test]
    fn every_malformed_return_field_aborts_fail_closed() {
        for case in 0..7 {
            let mut adapter = adapter();
            let mut event = return_event(10);
            let PrivateParserConfirmationEvent::CorrelatedReturn(value) = &mut event else {
                unreachable!();
            };
            match case {
                0 => value.provenance.method_id = 1,
                1 => value.provenance.call_id = Some(CALL_ID + 1),
                2 => value.asserted_authoritative_server_decode = false,
                3 => value.decoded_as_success = false,
                4 => value.decoded_body_length = 1,
                5 => value.decoded_body_present = false,
                6 => value.raw_status = 1,
                _ => unreachable!(),
            }
            assert!(
                adapter
                    .route_parser_snapshot(snapshot(102, vec![event]))
                    .is_err()
            );
            assert_eq!(
                adapter.state(),
                PrivateAutomarkerConfirmationAdapterState::
                    ConfirmationFailedAwaitingTransportRetirement
            );
            assert!(adapter.observe_connection_terminated(9).is_ok());
            assert_eq!(
                adapter.state(),
                PrivateAutomarkerConfirmationAdapterState::Failed
            );
        }
    }

    #[test]
    fn every_malformed_marker_field_aborts_fail_closed() {
        for case in 0..10 {
            let mut adapter = adapter();
            adapter
                .route_parser_snapshot(snapshot(102, vec![return_event(10)]))
                .unwrap();
            let mut event = marker_event(11);
            let PrivateParserConfirmationEvent::MarkerAdd(value) = &mut event else {
                unreachable!();
            };
            match case {
                0 => value.provenance.method_id = 1,
                1 => value.derived_marker_number = None,
                2 => value.marker_owner_actor_id = Some(78),
                3 => value.marker_owner_entity_uuid = None,
                4 => value.x = Some(TARGET.x + 1.0),
                5 => value.y = Some(f32::NAN),
                6 => value.z = None,
                7 => value.passive_instance_identity = Some(70),
                8 => value.asserted_authoritative_server_decode = false,
                9 => value.runtime_revision = 101,
                _ => unreachable!(),
            }
            assert!(
                adapter
                    .route_parser_snapshot(snapshot(103, vec![event]))
                    .is_err()
            );
            assert_eq!(
                adapter.state(),
                PrivateAutomarkerConfirmationAdapterState::
                    ConfirmationFailedAwaitingTransportRetirement
            );
            assert!(adapter.observe_connection_terminated(9).is_ok());
            assert_eq!(
                adapter.state(),
                PrivateAutomarkerConfirmationAdapterState::Failed
            );
        }
    }

    #[test]
    fn context_changes_replay_and_late_batches_are_terminal() {
        let mut changed = adapter();
        let mut wrong_session = snapshot(102, vec![return_event(10)]);
        wrong_session.session_key = "other".into();
        assert_eq!(
            changed.route_parser_snapshot(wrong_session),
            Err(PrivateAutomarkerConfirmationAdapterError::SessionOrContextChanged)
        );

        for case in 0..7 {
            let mut changed = adapter();
            let mut wrong = snapshot(102, vec![return_event(10)]);
            match case {
                0 => wrong.context.game_build = "other-build".into(),
                1 => wrong.context.scene_family = "other-scene".into(),
                2 => wrong.context.local_actor_id += 1,
                3 => wrong.context.connection_epoch += 1,
                4 => wrong.context.client_port += 1,
                5 => wrong.context.server_address = [10, 0, 0, 9],
                6 => wrong.context.runtime_revision = 100,
                _ => unreachable!(),
            }
            assert_eq!(
                changed.route_parser_snapshot(wrong),
                Err(PrivateAutomarkerConfirmationAdapterError::SessionOrContextChanged)
            );
        }

        let mut replay = adapter();
        let first = snapshot(102, vec![return_event(10)]);
        replay.route_parser_snapshot(first.clone()).unwrap();
        assert_eq!(
            replay.route_parser_snapshot(first),
            Err(PrivateAutomarkerConfirmationAdapterError::FilteredOrReplayedBatch)
        );

        let mut late = adapter();
        late.route_parser_snapshot(snapshot(103, vec![marker_event(11)]))
            .unwrap();
        assert_eq!(
            late.route_parser_snapshot(snapshot(102, vec![return_event(10)])),
            Err(PrivateAutomarkerConfirmationAdapterError::Router(
                ConfirmationRouterError::LateUnseenEvidence
            ))
        );

        let mut conflict = adapter();
        conflict
            .route_parser_snapshot(snapshot(102, vec![return_event(10)]))
            .unwrap();
        let mut conflicting_event = return_event(10);
        let PrivateParserConfirmationEvent::CorrelatedReturn(value) = &mut conflicting_event else {
            unreachable!();
        };
        value.source_clocks.observed_micros += 1;
        value.raw_status = 1;
        assert_eq!(
            conflict.route_parser_snapshot(snapshot(102, vec![conflicting_event])),
            Err(PrivateAutomarkerConfirmationAdapterError::Router(
                ConfirmationRouterError::ConflictingProvenance
            ))
        );
    }

    #[test]
    fn carrier_frontier_rejects_delayed_pre_rewrite_evidence_and_still_retires_transport() {
        for mut event in [return_event(CARRIER_CAPTURE_SEQUENCE), marker_event(3)] {
            let mut adapter = adapter();
            match &mut event {
                PrivateParserConfirmationEvent::CorrelatedReturn(event) => {
                    event.provenance.capture_sequence = CARRIER_CAPTURE_SEQUENCE;
                }
                PrivateParserConfirmationEvent::MarkerAdd(event) => {
                    event.provenance.capture_sequence = CARRIER_CAPTURE_SEQUENCE - 1;
                }
            }
            assert_eq!(
                adapter.route_parser_snapshot(snapshot(103, vec![event])),
                Err(PrivateAutomarkerConfirmationAdapterError::Router(
                    ConfirmationRouterError::EvidenceAtOrBeforeCarrier
                ))
            );
            assert_eq!(
                adapter.state(),
                PrivateAutomarkerConfirmationAdapterState::
                    ConfirmationFailedAwaitingTransportRetirement
            );
            assert!(adapter.observe_connection_terminated(9).is_ok());
            assert_eq!(
                adapter.state(),
                PrivateAutomarkerConfirmationAdapterState::Failed
            );
        }
    }

    #[test]
    fn exact_ack_with_stale_mechanics_context_is_forwarded_for_transport_retirement() {
        let mut adapter = adapter();
        let mut malformed = return_event(10);
        let PrivateParserConfirmationEvent::CorrelatedReturn(event) = &mut malformed else {
            unreachable!();
        };
        event.decoded_as_success = false;
        assert!(
            adapter
                .route_parser_snapshot(snapshot(102, vec![malformed]))
                .is_err()
        );
        let result = adapter.observe_tcp(ack(100));
        assert!(result.is_err());
        assert!(!adapter.coordinator_state().tcp_rewrite_obligation_active);
        assert_eq!(
            adapter.state(),
            PrivateAutomarkerConfirmationAdapterState::Failed
        );
    }

    #[test]
    fn exact_rst_retires_transport_after_parser_confirmation_failure() {
        let mut adapter = adapter();
        let mut wrong_context = snapshot(102, vec![return_event(10)]);
        wrong_context.context.scene_family = "other".into();
        assert!(adapter.route_parser_snapshot(wrong_context).is_err());
        let mut rst = ack(0);
        rst.ack_flag = false;
        rst.cumulative_ack = 0;
        rst.rst = true;
        assert!(adapter.observe_tcp(rst).is_err());
        assert!(!adapter.coordinator_state().tcp_rewrite_obligation_active);
        assert_eq!(
            adapter.state(),
            PrivateAutomarkerConfirmationAdapterState::Failed
        );
    }

    #[test]
    fn timeout_blocks_later_parser_confirmation_but_connection_close_retires_transport() {
        let mut adapter = adapter();
        std::thread::sleep(std::time::Duration::from_millis(2_010));
        assert!(adapter.observe_timeout().is_err());
        assert_eq!(
            adapter.state(),
            PrivateAutomarkerConfirmationAdapterState::
                ConfirmationFailedAwaitingTransportRetirement
        );
        assert_eq!(
            adapter.route_parser_snapshot(snapshot(102, vec![return_event(10)])),
            Err(PrivateAutomarkerConfirmationAdapterError::EventAfterTerminal)
        );
        assert!(adapter.observe_connection_terminated(9).is_ok());
        assert_eq!(
            adapter.state(),
            PrivateAutomarkerConfirmationAdapterState::Failed
        );
    }
}
