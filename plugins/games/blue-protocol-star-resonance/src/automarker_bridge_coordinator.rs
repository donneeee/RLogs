//! Pure coordinator for one Automarker carrier rewrite and its later proof.
//!
//! This module intentionally owns no driver, handle, socket, process, memory,
//! input, checksum helper, or send capability. It only composes the already
//! reviewed two-phase carrier state machine with checksum-boundary inputs and
//! the retrospective three-signal confirmation contract.

use crate::{
    AUTOMARKER_OUTBOUND_CARRIER_METHOD_ID, AutomarkerConfirmationBaseline,
    AutomarkerConfirmationConfig, AutomarkerConfirmationContext, AutomarkerConfirmationError,
    AutomarkerConfirmationMarkerAdd, AutomarkerConfirmationRewrite,
    AutomarkerConfirmationRpcReturn, AutomarkerConfirmationState,
    AutomarkerConfirmationTcpObservation, AutomarkerWinDivertAddress,
    OfflineAutomarkerConnectionEpochBinding, ProtocolPack, SingleMarkerRewriteConfirmation,
    SingleMarkerXyzCanary, SingleMarkerXyzCanaryConfig, SingleMarkerXyzCanaryContext,
    SingleMarkerXyzCanaryError, SingleMarkerXyzCanaryState, SingleMarkerXyzCarrierIdentity,
    SingleMarkerXyzCommitDisposition, SingleMarkerXyzExternalSendOutcome,
    SingleMarkerXyzSegmentDisposition,
};

const EXACT_CARRIER_BYTES: u32 = 197;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomarkerBridgeCoordinatorError {
    InvalidBaseline,
    InvalidPacketLayout,
    CarrierNotObserved,
    PreparationMismatch,
    Canary(SingleMarkerXyzCanaryError),
    Confirmation(AutomarkerConfirmationError),
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::*;
    use crate::{
        AUTOMARKER_REQUEST_BUILD, AutomarkerConfirmationObservationStamp,
        AutomarkerConfirmationTcpTuple, AutomarkerIpv4Endpoint, AutomarkerOwnedTcpConnection,
        MappingProvenance, SINGLE_MARKER_XYZ_CANARY_ARM_TOKEN,
        bind_offline_automarker_connection_epoch,
    };

    const EPOCH: u64 = 9;
    const FRAME_SEQUENCE: u32 = 1_000;

    fn pack() -> ProtocolPack {
        let source = ProtocolPack::from_json(include_bytes!(
            "../protocol-packs/global/steam-24687926/pack.json"
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

    fn connection() -> AutomarkerOwnedTcpConnection {
        AutomarkerOwnedTcpConnection {
            process_id: 42,
            local: AutomarkerIpv4Endpoint {
                address: Ipv4Addr::new(10, 0, 0, 2),
                port: 50_000,
            },
            remote: AutomarkerIpv4Endpoint {
                address: Ipv4Addr::new(10, 0, 0, 3),
                port: 443,
            },
        }
    }

    fn canary_context(revision: u64) -> SingleMarkerXyzCanaryContext<'static> {
        SingleMarkerXyzCanaryContext {
            game_build: AUTOMARKER_REQUEST_BUILD,
            current_scene_family: "mech-facility",
            runtime_revision: revision,
            observation_monotonic_millis: 1_100,
            observation_age_millis: 0,
        }
    }

    fn confirmation_context(revision: u64, micros: u64) -> AutomarkerConfirmationContext {
        AutomarkerConfirmationContext {
            game_build: AUTOMARKER_REQUEST_BUILD.into(),
            scene_family: "mech-facility".into(),
            local_actor_id: 77,
            connection_epoch: EPOCH,
            client_to_server_tuple: AutomarkerConfirmationTcpTuple {
                client_address: [10, 0, 0, 2],
                client_port: 50_000,
                server_address: [10, 0, 0, 3],
                server_port: 443,
            },
            runtime_revision: revision,
            observed_micros: micros,
        }
    }

    fn coordinator() -> (AutomarkerBridgeCoordinator, ProtocolPack, Vec<u8>) {
        let pack = pack();
        let connection = connection();
        let binding =
            bind_offline_automarker_connection_epoch(42, connection, EPOCH, true, &[connection])
                .unwrap();
        let baseline = AutomarkerConfirmationBaseline {
            context: confirmation_context(100, 1_000_000),
            observation_ordinal: 1,
            same_number_passive_instance_identities: vec![70],
        };
        let mut coordinator = AutomarkerBridgeCoordinator::arm(
            SingleMarkerXyzCanaryConfig {
                expected_scene_family: "mech-facility".into(),
                marker_number: 1,
                target_position: crate::AutomarkerRequestXyz {
                    x: 101.25,
                    y: -22.5,
                    z: 303.75,
                },
            },
            SINGLE_MARKER_XYZ_CANARY_ARM_TOKEN,
            &pack,
            binding,
            canary_context(100),
            baseline,
        )
        .unwrap();
        let frame = crate::automarker_request::tests_support::synthetic_frame_for_adapter();
        coordinator
            .observe_fresh_carrier(&pack, EPOCH, FRAME_SEQUENCE, 0, &frame, canary_context(101))
            .unwrap();
        (coordinator, pack, frame)
    }

    fn address() -> AutomarkerWinDivertAddress {
        let mut bytes = [0_u8; 80];
        bytes[8..12].copy_from_slice(&(1_u32 << 17).to_ne_bytes());
        AutomarkerWinDivertAddress::from_opaque_bytes(bytes)
    }

    fn packet(frame: &[u8]) -> Vec<u8> {
        let mut packet = vec![0_u8; 40];
        packet.extend_from_slice(frame);
        packet
    }

    fn prepare(
        coordinator: &mut AutomarkerBridgeCoordinator,
        frame: &[u8],
    ) -> (AutomarkerBridgeChecksumInput, Vec<u8>) {
        let packet = packet(frame);
        let prepared = coordinator.prepare(
            EPOCH,
            FRAME_SEQUENCE,
            40,
            &packet,
            address(),
            canary_context(101),
            &confirmation_context(101, 1_100_000),
            2,
        );
        let AutomarkerBridgePrepareDisposition::NeedsChecksumRepair(input) = prepared else {
            panic!("expected checksum input, got {prepared:?}");
        };
        (input, packet)
    }

    fn stamp(ordinal: u64, micros: u64) -> AutomarkerConfirmationObservationStamp {
        AutomarkerConfirmationObservationStamp {
            observation_ordinal: ordinal,
            observed_micros: micros,
            elapsed_since_rewrite_millis: (micros - 1_100_000) / 1_000,
        }
    }

    #[test]
    fn cancel_before_send_returns_the_stored_exact_original() {
        let (mut coordinator, _, frame) = coordinator();
        let (input, packet) = prepare(&mut coordinator, &frame);
        assert_ne!(input.approved_changed_packet, packet);
        assert_eq!(input.original_packet, packet);
        assert_eq!(
            coordinator.cancel(input.preparation_id),
            AutomarkerBridgePrepareDisposition::SendOriginal(AutomarkerBridgeOriginalPacket {
                packet,
                address: address(),
            })
        );
        assert!(!coordinator.state().tcp_rewrite_obligation_active);
    }

    #[test]
    fn indeterminate_send_retains_rewrite_obligation_and_never_starts_confirmation() {
        let (mut coordinator, _, frame) = coordinator();
        let (input, _) = prepare(&mut coordinator, &frame);
        assert_eq!(
            coordinator.commit(
                input.preparation_id,
                SingleMarkerXyzExternalSendOutcome::Failed
            ),
            AutomarkerBridgeCommitDisposition::AbortWithoutReinject(
                AutomarkerBridgeCoordinatorError::Canary(
                    SingleMarkerXyzCanaryError::IndeterminateModifiedSend
                )
            )
        );
        let state = coordinator.state();
        assert!(state.tcp_rewrite_obligation_active);
        assert_eq!(state.confirmation, None);

        let retransmission = coordinator.prepare(
            EPOCH,
            FRAME_SEQUENCE,
            40,
            &packet(&frame),
            address(),
            canary_context(102),
            &confirmation_context(102, 1_200_000),
            3,
        );
        assert!(matches!(
            retransmission,
            AutomarkerBridgePrepareDisposition::NeedsChecksumRepair(_)
        ));
    }

    #[test]
    fn complete_send_begins_retrospective_confirmation_but_does_not_clear_transport() {
        let (mut coordinator, _, frame) = coordinator();
        let (input, packet) = prepare(&mut coordinator, &frame);
        assert_eq!(
            coordinator.commit(
                input.preparation_id,
                SingleMarkerXyzExternalSendOutcome::Complete {
                    bytes_sent: packet.len(),
                },
            ),
            AutomarkerBridgeCommitDisposition::Committed
        );
        assert_eq!(
            coordinator.state().confirmation,
            Some(AutomarkerConfirmationState::AwaitingEvidence {
                reverse_cumulative_ack: false,
                successful_empty_rpc_return: false,
                new_authoritative_marker_add: false,
            })
        );
        assert!(coordinator.state().tcp_rewrite_obligation_active);
    }

    #[test]
    fn all_three_post_send_signals_confirm_and_ack_retires_transport_mapping() {
        let (mut coordinator, _, frame) = coordinator();
        let carrier_call_id = coordinator.carrier.unwrap().rpc_call_id;
        let (input, packet) = prepare(&mut coordinator, &frame);
        coordinator.commit(
            input.preparation_id,
            SingleMarkerXyzExternalSendOutcome::Complete {
                bytes_sent: packet.len(),
            },
        );
        let context = confirmation_context(102, 1_111_000);
        coordinator
            .observe(AutomarkerBridgeObservation::RpcReturn {
                context: &context,
                observation: AutomarkerConfirmationRpcReturn {
                    original_call_id: carrier_call_id,
                    asserted_decoded_from_authoritative_server_stream: true,
                    decoded_as_success: true,
                    decoded_body_length: 0,
                    stamp: stamp(3, 1_111_000),
                },
            })
            .unwrap();
        let context = confirmation_context(103, 1_112_000);
        coordinator
            .observe(AutomarkerBridgeObservation::AuthoritativeMarkerAdd {
                context: &context,
                observation: AutomarkerConfirmationMarkerAdd {
                    method_id: crate::AUTOMARKER_AUTHORITATIVE_MARKER_ADD_METHOD_ID,
                    marker_number: 1,
                    marker_owner_actor_id: 77,
                    position: coordinator.config.target_position,
                    passive_instance_identity: 71,
                    asserted_decoded_from_authoritative_server_stream: true,
                    runtime_revision: 103,
                    observed_micros: 1_112_000,
                    stamp: stamp(4, 1_112_000),
                },
            })
            .unwrap();
        let context = confirmation_context(103, 1_113_000);
        let state = coordinator
            .observe(AutomarkerBridgeObservation::Tcp {
                canary_context: canary_context(103),
                confirmation_context: &context,
                observation: AutomarkerConfirmationTcpObservation {
                    connection_epoch: EPOCH,
                    source_address: [10, 0, 0, 3],
                    source_port: 443,
                    destination_address: [10, 0, 0, 2],
                    destination_port: 50_000,
                    ack_flag: true,
                    cumulative_ack: FRAME_SEQUENCE + EXACT_CARRIER_BYTES,
                    fin: false,
                    rst: false,
                    stamp: stamp(5, 1_113_000),
                },
            })
            .unwrap();
        assert_eq!(
            state.confirmation,
            Some(AutomarkerConfirmationState::Confirmed)
        );
        assert!(!state.tcp_rewrite_obligation_active);
    }

    #[test]
    fn confirmation_failure_cannot_clear_rewrite_obligation() {
        let (mut coordinator, _, frame) = coordinator();
        let (input, packet) = prepare(&mut coordinator, &frame);
        coordinator.commit(
            input.preparation_id,
            SingleMarkerXyzExternalSendOutcome::Complete {
                bytes_sent: packet.len(),
            },
        );
        let context = confirmation_context(102, 1_111_000);
        assert_eq!(
            coordinator.observe(AutomarkerBridgeObservation::RpcReturn {
                context: &context,
                observation: AutomarkerConfirmationRpcReturn {
                    original_call_id: 999,
                    asserted_decoded_from_authoritative_server_stream: true,
                    decoded_as_success: true,
                    decoded_body_length: 0,
                    stamp: stamp(3, 1_111_000),
                },
            }),
            Err(AutomarkerBridgeCoordinatorError::Confirmation(
                AutomarkerConfirmationError::WrongRpcCallId
            ))
        );
        assert!(coordinator.state().tcp_rewrite_obligation_active);
    }

    #[test]
    fn mismatched_commit_or_connection_teardown_never_releases_original_bytes() {
        let (mut coordinator, _, frame) = coordinator();
        let (input, _) = prepare(&mut coordinator, &frame);
        assert_eq!(
            coordinator.commit(
                input.preparation_id + 1,
                SingleMarkerXyzExternalSendOutcome::Failed,
            ),
            AutomarkerBridgeCommitDisposition::AbortWithoutReinject(
                AutomarkerBridgeCoordinatorError::Canary(
                    SingleMarkerXyzCanaryError::PreparedRewriteMismatch
                )
            )
        );
        assert!(coordinator.state().tcp_rewrite_obligation_active);
        assert_eq!(
            coordinator.commit(
                input.preparation_id,
                SingleMarkerXyzExternalSendOutcome::Complete { bytes_sent: 237 },
            ),
            AutomarkerBridgeCommitDisposition::AbortWithoutReinject(
                AutomarkerBridgeCoordinatorError::Canary(
                    SingleMarkerXyzCanaryError::PreparedRewriteMismatch
                )
            )
        );
        assert_eq!(coordinator.state().confirmation, None);
        coordinator
            .observe(AutomarkerBridgeObservation::ConnectionTerminated {
                connection_epoch: EPOCH + 1,
            })
            .unwrap();
        assert!(coordinator.state().tcp_rewrite_obligation_active);
    }

    #[test]
    fn prepare_rejects_a_confirmation_context_unrelated_to_the_canary_context() {
        let (mut coordinator, _, frame) = coordinator();
        let packet = packet(&frame);
        let mut unrelated = confirmation_context(101, 1_100_000);
        unrelated.local_actor_id = 999;
        assert_eq!(
            coordinator.prepare(
                EPOCH,
                FRAME_SEQUENCE,
                40,
                &packet,
                address(),
                canary_context(101),
                &unrelated,
                2,
            ),
            AutomarkerBridgePrepareDisposition::SendOriginal(AutomarkerBridgeOriginalPacket {
                packet: packet.clone(),
                address: address(),
            })
        );
        assert!(!coordinator.state().tcp_rewrite_obligation_active);

        assert!(matches!(
            coordinator.prepare(
                EPOCH,
                FRAME_SEQUENCE,
                40,
                &packet,
                address(),
                canary_context(101),
                &confirmation_context(101, 1_100_000),
                2,
            ),
            AutomarkerBridgePrepareDisposition::NeedsChecksumRepair(_)
        ));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutomarkerBridgeState {
    pub canary: SingleMarkerXyzCanaryState,
    pub confirmation: Option<AutomarkerConfirmationState>,
    pub coordinator_error: Option<AutomarkerBridgeCoordinatorError>,
    /// True means an overlapping original byte range must not be reinjected.
    /// Confirmation success or failure never clears this transport property.
    pub tcp_rewrite_obligation_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomarkerBridgeOriginalPacket {
    pub packet: Vec<u8>,
    pub address: AutomarkerWinDivertAddress,
}

/// Pure input for `prepare_automarker_ipv4_tcp_packet`. This is not itself a
/// send authorization; only that checksum boundary can produce
/// `AutomarkerPacketSendPreparation::ModifiedAuthorized`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomarkerBridgeChecksumInput {
    pub preparation_id: u64,
    pub original_packet: Vec<u8>,
    pub approved_changed_packet: Vec<u8>,
    pub address: AutomarkerWinDivertAddress,
}

impl AutomarkerBridgeChecksumInput {
    pub fn changed_packet_for_checksum(&self) -> &[u8] {
        &self.approved_changed_packet
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomarkerBridgePrepareDisposition {
    SendOriginal(AutomarkerBridgeOriginalPacket),
    NeedsChecksumRepair(AutomarkerBridgeChecksumInput),
    AbortWithoutReinject(AutomarkerBridgeCoordinatorError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomarkerBridgeCommitDisposition {
    Committed,
    CommittedButConfirmationAborted(AutomarkerConfirmationError),
    AbortWithoutReinject(AutomarkerBridgeCoordinatorError),
}

#[derive(Debug, Clone)]
struct PendingPacket {
    preparation_id: u64,
    frame_sequence_start: u32,
    rewrite_observation_ordinal: u64,
    rewrite_observed_micros: u64,
    first_modified_emission: bool,
    original_packet: Vec<u8>,
    address: AutomarkerWinDivertAddress,
}

pub enum AutomarkerBridgeObservation<'a> {
    Tcp {
        canary_context: SingleMarkerXyzCanaryContext<'a>,
        confirmation_context: &'a AutomarkerConfirmationContext,
        observation: AutomarkerConfirmationTcpObservation,
    },
    RpcReturn {
        context: &'a AutomarkerConfirmationContext,
        observation: AutomarkerConfirmationRpcReturn,
    },
    AuthoritativeMarkerAdd {
        context: &'a AutomarkerConfirmationContext,
        observation: AutomarkerConfirmationMarkerAdd,
    },
    Timeout {
        elapsed_since_rewrite_millis: u64,
        current_observed_micros: u64,
    },
    ConnectionTerminated {
        connection_epoch: u64,
    },
}

/// Deterministic, no-I/O lifecycle for exactly one marker carrier.
pub struct AutomarkerBridgeCoordinator {
    config: SingleMarkerXyzCanaryConfig,
    baseline: AutomarkerConfirmationBaseline,
    canary: SingleMarkerXyzCanary,
    carrier: Option<SingleMarkerXyzCarrierIdentity>,
    frame_sequence_start: Option<u32>,
    pending_packet: Option<PendingPacket>,
    confirmation: Option<SingleMarkerRewriteConfirmation>,
    confirmation_terminal_error: Option<AutomarkerConfirmationError>,
    coordinator_terminal_error: Option<AutomarkerBridgeCoordinatorError>,
    tcp_rewrite_obligation_active: bool,
}

impl AutomarkerBridgeCoordinator {
    pub fn arm(
        config: SingleMarkerXyzCanaryConfig,
        literal_consent: &str,
        pack: &ProtocolPack,
        binding: OfflineAutomarkerConnectionEpochBinding,
        canary_context: SingleMarkerXyzCanaryContext<'_>,
        baseline: AutomarkerConfirmationBaseline,
    ) -> Result<Self, AutomarkerBridgeCoordinatorError> {
        let binding_connection = binding.connection();
        let tuple = baseline.context.client_to_server_tuple;
        if baseline.context.game_build != canary_context.game_build
            || baseline.context.scene_family != canary_context.current_scene_family
            || baseline.context.local_actor_id == 0
            || baseline.context.connection_epoch != binding.connection_epoch()
            || baseline.context.runtime_revision == 0
            || baseline.context.observed_micros == 0
            || baseline.observation_ordinal == 0
            || baseline.same_number_passive_instance_identities.len() > 1
            || baseline
                .same_number_passive_instance_identities
                .contains(&0)
            || baseline.context.runtime_revision != canary_context.runtime_revision
            || tuple.client_address != binding_connection.local.address.octets()
            || tuple.client_port != binding_connection.local.port
            || tuple.server_address != binding_connection.remote.address.octets()
            || tuple.server_port != binding_connection.remote.port
        {
            return Err(AutomarkerBridgeCoordinatorError::InvalidBaseline);
        }
        let canary = SingleMarkerXyzCanary::arm(
            config.clone(),
            literal_consent,
            pack,
            binding,
            canary_context,
        )
        .map_err(AutomarkerBridgeCoordinatorError::Canary)?;
        Ok(Self {
            config,
            baseline,
            canary,
            carrier: None,
            frame_sequence_start: None,
            pending_packet: None,
            confirmation: None,
            confirmation_terminal_error: None,
            coordinator_terminal_error: None,
            tcp_rewrite_obligation_active: false,
        })
    }

    pub fn state(&self) -> AutomarkerBridgeState {
        AutomarkerBridgeState {
            canary: self.canary.state(),
            confirmation: self
                .confirmation
                .as_ref()
                .map(SingleMarkerRewriteConfirmation::state)
                .or_else(|| {
                    self.confirmation_terminal_error
                        .map(AutomarkerConfirmationState::Aborted)
                }),
            coordinator_error: self.coordinator_terminal_error,
            tcp_rewrite_obligation_active: self.tcp_rewrite_obligation_active,
        }
    }

    pub fn observe_fresh_carrier(
        &mut self,
        pack: &ProtocolPack,
        connection_epoch: u64,
        frame_sequence_start: u32,
        carrier_age_millis: u64,
        frame: &[u8],
        context: SingleMarkerXyzCanaryContext<'_>,
    ) -> Result<SingleMarkerXyzCarrierIdentity, AutomarkerBridgeCoordinatorError> {
        let identity = self
            .canary
            .observe_fresh_carrier(
                pack,
                connection_epoch,
                frame_sequence_start,
                carrier_age_millis,
                frame,
                context,
            )
            .map_err(AutomarkerBridgeCoordinatorError::Canary)?;
        self.carrier = Some(identity);
        self.frame_sequence_start = Some(frame_sequence_start);
        Ok(identity)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        connection_epoch: u64,
        tcp_sequence_start: u32,
        tcp_payload_offset: usize,
        original_packet: &[u8],
        address: AutomarkerWinDivertAddress,
        context: SingleMarkerXyzCanaryContext<'_>,
        rewrite_context: &AutomarkerConfirmationContext,
        rewrite_observation_ordinal: u64,
    ) -> AutomarkerBridgePrepareDisposition {
        if let Some(reason) = self.coordinator_terminal_error {
            return AutomarkerBridgePrepareDisposition::AbortWithoutReinject(reason);
        }
        if self.pending_packet.is_some() || self.carrier.is_none() {
            return AutomarkerBridgePrepareDisposition::AbortWithoutReinject(
                if self.carrier.is_none() {
                    AutomarkerBridgeCoordinatorError::CarrierNotObserved
                } else {
                    AutomarkerBridgeCoordinatorError::PreparationMismatch
                },
            );
        }
        let baseline = &self.baseline.context;
        if rewrite_observation_ordinal <= self.baseline.observation_ordinal
            || rewrite_context.observed_micros <= baseline.observed_micros
            || rewrite_context.game_build != baseline.game_build
            || rewrite_context.scene_family != baseline.scene_family
            || rewrite_context.local_actor_id != baseline.local_actor_id
            || rewrite_context.connection_epoch != baseline.connection_epoch
            || rewrite_context.client_to_server_tuple != baseline.client_to_server_tuple
            || rewrite_context.runtime_revision < baseline.runtime_revision
            || rewrite_context.game_build != context.game_build
            || rewrite_context.scene_family != context.current_scene_family
            || rewrite_context.connection_epoch != connection_epoch
            || rewrite_context.runtime_revision != context.runtime_revision
        {
            return AutomarkerBridgePrepareDisposition::SendOriginal(
                AutomarkerBridgeOriginalPacket {
                    packet: original_packet.to_vec(),
                    address,
                },
            );
        }
        let Some(payload) = original_packet.get(tcp_payload_offset..) else {
            return AutomarkerBridgePrepareDisposition::AbortWithoutReinject(
                AutomarkerBridgeCoordinatorError::InvalidPacketLayout,
            );
        };
        match self.canary.prepare_outbound_segment(
            connection_epoch,
            tcp_sequence_start,
            payload,
            original_packet.len(),
            context,
        ) {
            SingleMarkerXyzSegmentDisposition::SendOriginal(returned_payload) => {
                if returned_payload != payload {
                    return AutomarkerBridgePrepareDisposition::AbortWithoutReinject(
                        AutomarkerBridgeCoordinatorError::InvalidPacketLayout,
                    );
                }
                AutomarkerBridgePrepareDisposition::SendOriginal(AutomarkerBridgeOriginalPacket {
                    packet: original_packet.to_vec(),
                    address,
                })
            }
            SingleMarkerXyzSegmentDisposition::PreparedRewriteNeedsPacketChecksumRepair(
                prepared,
            ) => {
                if prepared.original_payload != payload
                    || prepared.rewritten_payload.len() != payload.len()
                    || prepared.expected_packet_send_len != original_packet.len()
                {
                    return AutomarkerBridgePrepareDisposition::AbortWithoutReinject(
                        AutomarkerBridgeCoordinatorError::InvalidPacketLayout,
                    );
                }
                let mut approved_changed_packet = original_packet.to_vec();
                approved_changed_packet[tcp_payload_offset..]
                    .copy_from_slice(&prepared.rewritten_payload);
                self.pending_packet = Some(PendingPacket {
                    preparation_id: prepared.preparation_id,
                    frame_sequence_start: self
                        .frame_sequence_start
                        .expect("observed carrier records its sequence"),
                    rewrite_observation_ordinal,
                    rewrite_observed_micros: rewrite_context.observed_micros,
                    first_modified_emission: prepared.first_modified_emission,
                    original_packet: original_packet.to_vec(),
                    address,
                });
                AutomarkerBridgePrepareDisposition::NeedsChecksumRepair(
                    AutomarkerBridgeChecksumInput {
                        preparation_id: prepared.preparation_id,
                        original_packet: original_packet.to_vec(),
                        approved_changed_packet,
                        address,
                    },
                )
            }
            SingleMarkerXyzSegmentDisposition::AbortWithoutReinject(reason) => {
                AutomarkerBridgePrepareDisposition::AbortWithoutReinject(
                    AutomarkerBridgeCoordinatorError::Canary(reason),
                )
            }
        }
    }

    pub fn cancel(&mut self, preparation_id: u64) -> AutomarkerBridgePrepareDisposition {
        if let Some(reason) = self.coordinator_terminal_error {
            return AutomarkerBridgePrepareDisposition::AbortWithoutReinject(reason);
        }
        let Some(pending) = self.pending_packet.as_ref() else {
            return AutomarkerBridgePrepareDisposition::AbortWithoutReinject(
                AutomarkerBridgeCoordinatorError::PreparationMismatch,
            );
        };
        if pending.preparation_id != preparation_id {
            return AutomarkerBridgePrepareDisposition::AbortWithoutReinject(
                AutomarkerBridgeCoordinatorError::PreparationMismatch,
            );
        }
        let pending = self.pending_packet.take().expect("checked pending packet");
        match self
            .canary
            .cancel_prepared_rewrite_before_send(preparation_id)
        {
            SingleMarkerXyzSegmentDisposition::SendOriginal(_) => {
                AutomarkerBridgePrepareDisposition::SendOriginal(AutomarkerBridgeOriginalPacket {
                    packet: pending.original_packet,
                    address: pending.address,
                })
            }
            SingleMarkerXyzSegmentDisposition::AbortWithoutReinject(reason) => {
                AutomarkerBridgePrepareDisposition::AbortWithoutReinject(
                    AutomarkerBridgeCoordinatorError::Canary(reason),
                )
            }
            SingleMarkerXyzSegmentDisposition::PreparedRewriteNeedsPacketChecksumRepair(_) => {
                AutomarkerBridgePrepareDisposition::AbortWithoutReinject(
                    AutomarkerBridgeCoordinatorError::PreparationMismatch,
                )
            }
        }
    }

    pub fn commit(
        &mut self,
        preparation_id: u64,
        outcome: SingleMarkerXyzExternalSendOutcome,
    ) -> AutomarkerBridgeCommitDisposition {
        if let Some(reason) = self.coordinator_terminal_error {
            return AutomarkerBridgeCommitDisposition::AbortWithoutReinject(reason);
        }
        let Some(pending) = self.pending_packet.as_ref() else {
            return AutomarkerBridgeCommitDisposition::AbortWithoutReinject(
                AutomarkerBridgeCoordinatorError::PreparationMismatch,
            );
        };
        if pending.preparation_id != preparation_id {
            // `commit` means a modified send was attempted. A mismatched
            // receipt is indeterminate and cannot release original bytes.
            self.tcp_rewrite_obligation_active = true;
            let disposition = self.canary.commit_prepared_rewrite(preparation_id, outcome);
            self.pending_packet = None;
            let reason = match disposition {
                SingleMarkerXyzCommitDisposition::AbortWithoutReinject(reason) => {
                    AutomarkerBridgeCoordinatorError::Canary(reason)
                }
                SingleMarkerXyzCommitDisposition::Committed => {
                    AutomarkerBridgeCoordinatorError::PreparationMismatch
                }
            };
            self.coordinator_terminal_error = Some(reason);
            return AutomarkerBridgeCommitDisposition::AbortWithoutReinject(reason);
        }
        let pending = self.pending_packet.take().expect("checked pending packet");
        match self.canary.commit_prepared_rewrite(preparation_id, outcome) {
            SingleMarkerXyzCommitDisposition::AbortWithoutReinject(reason) => {
                self.tcp_rewrite_obligation_active = true;
                AutomarkerBridgeCommitDisposition::AbortWithoutReinject(
                    AutomarkerBridgeCoordinatorError::Canary(reason),
                )
            }
            SingleMarkerXyzCommitDisposition::Committed => {
                self.tcp_rewrite_obligation_active = true;
                if pending.first_modified_emission && self.confirmation.is_none() {
                    let carrier = self.carrier.expect("committed rewrite has a carrier");
                    let rewrite = AutomarkerConfirmationRewrite {
                        method_id: AUTOMARKER_OUTBOUND_CARRIER_METHOD_ID,
                        original_rpc_call_id: carrier.rpc_call_id,
                        mapped_tcp_sequence_start: pending.frame_sequence_start,
                        mapped_tcp_length: EXACT_CARRIER_BYTES,
                        runtime_revision: self
                            .canary
                            .committed_rewrite_stamp()
                            .expect("complete send commits a stamp")
                            .runtime_revision,
                        observed_micros: pending.rewrite_observed_micros,
                        observation_ordinal: pending.rewrite_observation_ordinal,
                    };
                    let confirmation_config = AutomarkerConfirmationConfig {
                        expected_game_build: self.baseline.context.game_build.clone(),
                        expected_scene_family: self.config.expected_scene_family.clone(),
                        marker_number: self.config.marker_number,
                        target_position: self.config.target_position,
                    };
                    match SingleMarkerRewriteConfirmation::begin(
                        confirmation_config,
                        self.baseline.clone(),
                        rewrite,
                    ) {
                        Ok(confirmation) => self.confirmation = Some(confirmation),
                        Err(reason) => {
                            self.confirmation_terminal_error = Some(reason);
                            return AutomarkerBridgeCommitDisposition::CommittedButConfirmationAborted(
                                reason,
                            );
                        }
                    }
                }
                AutomarkerBridgeCommitDisposition::Committed
            }
        }
    }

    pub fn observe(
        &mut self,
        event: AutomarkerBridgeObservation<'_>,
    ) -> Result<AutomarkerBridgeState, AutomarkerBridgeCoordinatorError> {
        match event {
            AutomarkerBridgeObservation::Tcp {
                canary_context,
                confirmation_context,
                observation,
            } => {
                let exact_reverse_tuple_epoch = {
                    let tuple = self.baseline.context.client_to_server_tuple;
                    observation.connection_epoch == self.baseline.context.connection_epoch
                        && observation.source_address == tuple.server_address
                        && observation.source_port == tuple.server_port
                        && observation.destination_address == tuple.client_address
                        && observation.destination_port == tuple.client_port
                };
                if (observation.fin || observation.rst) && exact_reverse_tuple_epoch {
                    self.canary
                        .observe_connection_terminated(observation.connection_epoch);
                    self.tcp_rewrite_obligation_active = false;
                } else if exact_reverse_tuple_epoch && observation.ack_flag {
                    let ack = self.canary.observe_cumulative_ack(
                        observation.connection_epoch,
                        observation.cumulative_ack,
                        canary_context,
                    );
                    if ack.retired_operations > 0 {
                        self.tcp_rewrite_obligation_active = false;
                    }
                }
                self.observe_confirmation(|confirmation| {
                    confirmation.observe_tcp(confirmation_context, observation)
                })?;
            }
            AutomarkerBridgeObservation::RpcReturn {
                context,
                observation,
            } => self.observe_confirmation(|confirmation| {
                confirmation.observe_rpc_return(context, observation)
            })?,
            AutomarkerBridgeObservation::AuthoritativeMarkerAdd {
                context,
                observation,
            } => self.observe_confirmation(|confirmation| {
                confirmation.observe_authoritative_marker_add(context, observation)
            })?,
            AutomarkerBridgeObservation::Timeout {
                elapsed_since_rewrite_millis,
                current_observed_micros,
            } => self.observe_confirmation(|confirmation| {
                confirmation.observe_timeout(elapsed_since_rewrite_millis, current_observed_micros)
            })?,
            AutomarkerBridgeObservation::ConnectionTerminated { connection_epoch } => {
                let exact_epoch = self.canary.connection_epoch() == Some(connection_epoch);
                self.canary.observe_connection_terminated(connection_epoch);
                if exact_epoch {
                    self.tcp_rewrite_obligation_active = false;
                }
            }
        }
        Ok(self.state())
    }

    fn observe_confirmation(
        &mut self,
        observe: impl FnOnce(
            &mut SingleMarkerRewriteConfirmation,
        ) -> Result<(), AutomarkerConfirmationError>,
    ) -> Result<(), AutomarkerBridgeCoordinatorError> {
        let Some(confirmation) = self.confirmation.as_mut() else {
            return Err(self
                .confirmation_terminal_error
                .map(AutomarkerBridgeCoordinatorError::Confirmation)
                .unwrap_or(AutomarkerBridgeCoordinatorError::PreparationMismatch));
        };
        observe(confirmation).map_err(AutomarkerBridgeCoordinatorError::Confirmation)
    }
}
