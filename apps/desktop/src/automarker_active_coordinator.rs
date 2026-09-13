//! Production-shaped coordinator adapter for the active Automarker worker.
//!
//! The adapter is deliberately not wired into the desktop lifecycle. It owns
//! the exact filter/pack/binding, the bridge and its private router clock, and
//! a bounded control ingress. No diagnostic surface returns packet bytes,
//! tuples, session keys, or marker coordinates.

#![allow(dead_code)]

use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};

use rlogs_game_bpsr::{
    AUTOMARKER_REQUEST_BUILD, AutomarkerActiveFilterPlan, AutomarkerActivePacketRole,
    AutomarkerBridgeCommitDisposition as BridgeCommitDisposition, AutomarkerBridgeCoordinator,
    AutomarkerBridgeCoordinatorError, AutomarkerBridgePrepareDisposition,
    AutomarkerConfirmationBaseline, AutomarkerRequestXyz, OfflineAutomarkerConnectionEpochBinding,
    ProtocolPack, SINGLE_MARKER_XYZ_CANARY_ARM_TOKEN, SingleMarkerXyzCanaryConfig,
    SingleMarkerXyzCanaryContext, SingleMarkerXyzExternalSendOutcome,
    decode_observed_automarker_request_into,
};

use crate::{
    automarker_active_worker::{
        ActiveAutomarkerCancelDisposition, ActiveAutomarkerClassificationMode,
        ActiveAutomarkerCommitDisposition, ActiveAutomarkerCoordinator,
        ActiveAutomarkerDisposition, ActiveAutomarkerPacket, ActiveAutomarkerSendOutcome,
        ActiveAutomarkerTimeoutDisposition,
    },
    automarker_confirmation_adapter::{
        PrivateAutomarkerConfirmationAdapter, PrivateAutomarkerConfirmationBinding,
        PrivateAutomarkerTcpObservation,
    },
    automarker_confirmation_router::PrivateParserConfirmationSnapshot,
};

const EXACT_CARRIER_BYTES: usize = 197;
const APPLICATION_OFFSET: usize = 36;
const COMMAND_CAPACITY_MAX: usize = 256;

enum ActiveAutomarkerCommand {
    ParserSnapshot(PrivateParserConfirmationSnapshot),
    ContextInvalidated,
    Timeout,
    ProcessTerminated,
}

#[derive(Clone)]
pub(crate) struct ActiveAutomarkerControl {
    sender: SyncSender<ActiveAutomarkerCommand>,
}

impl ActiveAutomarkerControl {
    pub(crate) fn parser_snapshot(
        &self,
        snapshot: PrivateParserConfirmationSnapshot,
    ) -> Result<(), &'static str> {
        self.send(ActiveAutomarkerCommand::ParserSnapshot(snapshot))
    }

    pub(crate) fn context_invalidated(&self) -> Result<(), &'static str> {
        self.send(ActiveAutomarkerCommand::ContextInvalidated)
    }

    pub(crate) fn timeout(&self) -> Result<(), &'static str> {
        self.send(ActiveAutomarkerCommand::Timeout)
    }

    pub(crate) fn process_terminated(&self) -> Result<(), &'static str> {
        self.send(ActiveAutomarkerCommand::ProcessTerminated)
    }

    fn send(&self, command: ActiveAutomarkerCommand) -> Result<(), &'static str> {
        self.sender.try_send(command).map_err(|error| match error {
            TrySendError::Full(_) => "Automarker control queue is full",
            TrySendError::Disconnected(_) => "Automarker coordinator is no longer running",
        })
    }
}

pub(crate) struct ActiveAutomarkerCoordinatorConfig {
    pub pack: ProtocolPack,
    pub filter_plan: AutomarkerActiveFilterPlan,
    pub connection_binding: OfflineAutomarkerConnectionEpochBinding,
    pub session_key: String,
    pub carrier_capture_sequence: u64,
    pub scene_family: String,
    pub local_actor_id: i64,
    pub marker_number: u8,
    pub target_position: AutomarkerRequestXyz,
    pub baseline_runtime_revision: u64,
    pub runtime_revision: u64,
    pub observation_age_millis: u64,
    pub baseline_same_number_passive_instance_identities: Vec<i64>,
}

pub(crate) struct ProductionActiveAutomarkerCoordinator {
    pack: ProtocolPack,
    filter_plan: AutomarkerActiveFilterPlan,
    connection_binding: OfflineAutomarkerConnectionEpochBinding,
    session_key: String,
    carrier_capture_sequence: u64,
    scene_family: String,
    local_actor_id: i64,
    marker_number: u8,
    target_position: AutomarkerRequestXyz,
    baseline_runtime_revision: u64,
    runtime_revision: u64,
    observation_age_millis: u64,
    baseline_same_number_passive_instance_identities: Vec<i64>,
    commands: Receiver<ActiveAutomarkerCommand>,
    adapter: Option<PrivateAutomarkerConfirmationAdapter>,
    pending_packet_len: Option<usize>,
    context_invalidated: bool,
    termination_requested: bool,
}

impl ProductionActiveAutomarkerCoordinator {
    pub(crate) fn create(
        config: ActiveAutomarkerCoordinatorConfig,
        command_capacity: usize,
    ) -> Result<(Self, ActiveAutomarkerControl), &'static str> {
        if command_capacity == 0 || command_capacity > COMMAND_CAPACITY_MAX {
            return Err("Automarker command capacity is outside the reviewed bound");
        }
        if config.pack.definition().target.build_id != AUTOMARKER_REQUEST_BUILD
            || config.filter_plan.connection_epoch() != config.connection_binding.connection_epoch()
            || config.session_key.trim().is_empty()
            || config.carrier_capture_sequence == 0
            || config.scene_family.trim().is_empty()
            || config.local_actor_id == 0
            || !(1..=6).contains(&config.marker_number)
            || config.baseline_runtime_revision == 0
            || config.runtime_revision < config.baseline_runtime_revision
            || config
                .baseline_same_number_passive_instance_identities
                .contains(&0)
            || config
                .baseline_same_number_passive_instance_identities
                .len()
                > 1
        {
            return Err("Automarker active coordinator configuration is invalid");
        }
        let (sender, commands) = mpsc::sync_channel(command_capacity);
        let control = ActiveAutomarkerControl { sender };
        Ok((
            Self {
                pack: config.pack,
                filter_plan: config.filter_plan,
                connection_binding: config.connection_binding,
                session_key: config.session_key,
                carrier_capture_sequence: config.carrier_capture_sequence,
                scene_family: config.scene_family,
                local_actor_id: config.local_actor_id,
                marker_number: config.marker_number,
                target_position: config.target_position,
                baseline_runtime_revision: config.baseline_runtime_revision,
                runtime_revision: config.runtime_revision,
                observation_age_millis: config.observation_age_millis,
                baseline_same_number_passive_instance_identities: config
                    .baseline_same_number_passive_instance_identities,
                commands,
                adapter: None,
                pending_packet_len: None,
                context_invalidated: false,
                termination_requested: false,
            },
            control,
        ))
    }

    fn drain_commands(&mut self) {
        loop {
            match self.commands.try_recv() {
                Ok(ActiveAutomarkerCommand::ParserSnapshot(snapshot)) => {
                    if let Some(adapter) = self.adapter.as_mut() {
                        if adapter.route_parser_snapshot(snapshot).is_err() {
                            self.context_invalidated = true;
                        }
                    }
                }
                Ok(ActiveAutomarkerCommand::ContextInvalidated) => {
                    self.context_invalidated = true;
                    if let Some(adapter) = self.adapter.as_mut() {
                        adapter.invalidate_context();
                    }
                }
                Ok(ActiveAutomarkerCommand::Timeout) => {
                    if let Some(adapter) = self.adapter.as_mut() {
                        let _ = adapter.observe_timeout();
                    }
                }
                Ok(ActiveAutomarkerCommand::ProcessTerminated) => {
                    self.termination_requested = true;
                    if let Some(adapter) = self.adapter.as_mut() {
                        let _ = adapter.observe_connection_terminated(
                            self.connection_binding.connection_epoch(),
                        );
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.termination_requested = true;
                    break;
                }
            }
        }
    }

    fn classify_existing(
        &mut self,
        packet: &ActiveAutomarkerPacket,
        tcp_sequence_start: u32,
        tcp_payload_offset: usize,
    ) -> ActiveAutomarkerDisposition {
        let Some(adapter) = self.adapter.as_mut() else {
            return ActiveAutomarkerDisposition::PassThrough;
        };
        match adapter.prepare_outbound_packet(
            self.connection_binding.connection_epoch(),
            tcp_sequence_start,
            tcp_payload_offset,
            &packet.bytes,
            packet.address,
            self.runtime_revision,
            self.observation_age_millis,
        ) {
            Ok(AutomarkerBridgePrepareDisposition::SendOriginal(original))
                if original.packet == packet.bytes && original.address == packet.address =>
            {
                ActiveAutomarkerDisposition::PassThrough
            }
            Ok(AutomarkerBridgePrepareDisposition::NeedsChecksumRepair(input))
                if input.original_packet == packet.bytes
                    && input.address == packet.address
                    && input.approved_changed_packet.len() == packet.bytes.len() =>
            {
                self.pending_packet_len = Some(packet.bytes.len());
                ActiveAutomarkerDisposition::HoldExactCarrier {
                    preparation_id: input.preparation_id,
                    approved_changed_bytes: input.approved_changed_packet,
                }
            }
            _ => ActiveAutomarkerDisposition::AbortWithoutReinject,
        }
    }

    fn arm_exact_carrier(
        &mut self,
        packet: &ActiveAutomarkerPacket,
        tcp_sequence_start: u32,
        tcp_payload_offset: usize,
        payload: &[u8],
    ) -> ActiveAutomarkerDisposition {
        let mut scratch = Vec::new();
        let request = match decode_observed_automarker_request_into(
            &self.pack,
            &payload[APPLICATION_OFFSET..],
            &mut scratch,
        ) {
            Ok(request) if request.marker_number == self.marker_number => request,
            _ => return ActiveAutomarkerDisposition::PassThrough,
        };
        let carrier_rpc_call_id = u32::from_be_bytes(payload[28..32].try_into().unwrap());
        if carrier_rpc_call_id == 0 {
            return ActiveAutomarkerDisposition::PassThrough;
        }
        let frame_sequence_start = tcp_sequence_start;
        let binding = self.connection_binding.connection();
        let private_binding = PrivateAutomarkerConfirmationBinding {
            session_key: self.session_key.clone(),
            game_build: self.pack.definition().target.build_id.clone(),
            scene_family: self.scene_family.clone(),
            local_actor_id: self.local_actor_id,
            connection_epoch: self.connection_binding.connection_epoch(),
            client_address: binding.local.address.octets(),
            client_port: binding.local.port,
            server_address: binding.remote.address.octets(),
            server_port: binding.remote.port,
            marker_number: self.marker_number,
            target_position: self.target_position,
            carrier_capture_sequence: self.carrier_capture_sequence,
            carrier_rpc_call_id,
            mapped_tcp_sequence_start: frame_sequence_start,
            mapped_tcp_length: EXACT_CARRIER_BYTES as u32,
            baseline_runtime_revision: self.baseline_runtime_revision,
            rewrite_runtime_revision: self.runtime_revision,
        };
        let mut prepared = None;
        let adapter =
            PrivateAutomarkerConfirmationAdapter::begin_prepared(private_binding, |clock| {
                let baseline = AutomarkerConfirmationBaseline {
                    context: clock.baseline_context.clone(),
                    observation_ordinal: clock.baseline_stamp.observation_ordinal,
                    same_number_passive_instance_identities: self
                        .baseline_same_number_passive_instance_identities
                        .clone(),
                };
                let config = SingleMarkerXyzCanaryConfig {
                    expected_scene_family: self.scene_family.clone(),
                    marker_number: self.marker_number,
                    target_position: self.target_position,
                };
                let baseline_context = SingleMarkerXyzCanaryContext {
                    game_build: &clock.baseline_context.game_build,
                    current_scene_family: &clock.baseline_context.scene_family,
                    runtime_revision: clock.baseline_context.runtime_revision,
                    observation_monotonic_millis: clock
                        .baseline_context
                        .observed_micros
                        .saturating_add(999)
                        / 1_000,
                    observation_age_millis: self.observation_age_millis,
                };
                let mut coordinator = AutomarkerBridgeCoordinator::arm(
                    config,
                    SINGLE_MARKER_XYZ_CANARY_ARM_TOKEN,
                    &self.pack,
                    self.connection_binding,
                    baseline_context,
                    baseline,
                )?;
                let rewrite_context = SingleMarkerXyzCanaryContext {
                    game_build: &clock.rewrite_context.game_build,
                    current_scene_family: &clock.rewrite_context.scene_family,
                    runtime_revision: clock.rewrite_context.runtime_revision,
                    observation_monotonic_millis: clock
                        .rewrite_context
                        .observed_micros
                        .saturating_add(999)
                        / 1_000,
                    observation_age_millis: self.observation_age_millis,
                };
                let identity = coordinator.observe_fresh_carrier(
                    &self.pack,
                    self.connection_binding.connection_epoch(),
                    frame_sequence_start,
                    self.observation_age_millis,
                    payload,
                    rewrite_context,
                )?;
                if identity.rpc_call_id != carrier_rpc_call_id
                    || identity.session_sequence != request.session_sequence
                {
                    return Err(AutomarkerBridgeCoordinatorError::CarrierNotObserved);
                }
                let disposition = coordinator.prepare(
                    self.connection_binding.connection_epoch(),
                    tcp_sequence_start,
                    tcp_payload_offset,
                    &packet.bytes,
                    packet.address,
                    rewrite_context,
                    &clock.rewrite_context,
                    clock.rewrite_stamp.observation_ordinal,
                );
                let AutomarkerBridgePrepareDisposition::NeedsChecksumRepair(input) = disposition
                else {
                    return Err(AutomarkerBridgeCoordinatorError::PreparationMismatch);
                };
                let preparation_id = input.preparation_id;
                prepared = Some(input);
                Ok((coordinator, preparation_id))
            });
        let adapter = match adapter {
            Ok(adapter) => adapter,
            Err(_reason) => {
                #[cfg(test)]
                eprintln!("active coordinator rejected carrier: {_reason:?}");
                return ActiveAutomarkerDisposition::PassThrough;
            }
        };
        let Some(input) = prepared else {
            return ActiveAutomarkerDisposition::AbortWithoutReinject;
        };
        if input.original_packet != packet.bytes
            || input.address != packet.address
            || input.approved_changed_packet.len() != packet.bytes.len()
        {
            return ActiveAutomarkerDisposition::AbortWithoutReinject;
        }
        self.adapter = Some(adapter);
        self.pending_packet_len = Some(packet.bytes.len());
        ActiveAutomarkerDisposition::HoldExactCarrier {
            preparation_id: input.preparation_id,
            approved_changed_bytes: input.approved_changed_packet,
        }
    }
}

impl ActiveAutomarkerCoordinator for ProductionActiveAutomarkerCoordinator {
    fn classify(
        &mut self,
        packet: &ActiveAutomarkerPacket,
        mode: ActiveAutomarkerClassificationMode,
    ) -> ActiveAutomarkerDisposition {
        self.drain_commands();
        let inspection = match self.filter_plan.inspect(&packet.bytes) {
            Ok(inspection) => inspection,
            Err(_) => return ActiveAutomarkerDisposition::PassThrough,
        };
        if inspection.role == AutomarkerActivePacketRole::InboundRetirementPassThrough {
            if let Some(adapter) = self.adapter.as_mut() {
                let transport = inspection.transport;
                let _ = adapter.observe_tcp(PrivateAutomarkerTcpObservation {
                    connection_epoch: self.connection_binding.connection_epoch(),
                    source_address: transport.source.address.octets(),
                    source_port: transport.source.port,
                    destination_address: transport.destination.address.octets(),
                    destination_port: transport.destination.port,
                    ack_flag: transport.ack,
                    cumulative_ack: transport.acknowledgement,
                    fin: transport.fin,
                    rst: transport.rst,
                    runtime_revision: self.runtime_revision,
                });
            }
            return if inspection.transport.rst {
                ActiveAutomarkerDisposition::ConnectionTerminatedAfterPassThrough
            } else if inspection.transport.fin {
                ActiveAutomarkerDisposition::FinObservedPassThrough
            } else {
                ActiveAutomarkerDisposition::PassThrough
            };
        }

        if self.adapter.is_some() {
            return self.classify_existing(
                packet,
                inspection.transport.sequence,
                inspection.transport.payload_offset_bytes,
            );
        }
        if mode == ActiveAutomarkerClassificationMode::DrainExistingOnly
            || self.context_invalidated
            || self.termination_requested
        {
            return ActiveAutomarkerDisposition::PassThrough;
        }
        let payload = &packet.bytes[inspection.transport.payload_offset_bytes..];
        if payload.len() != EXACT_CARRIER_BYTES
            || tcp_syn(&packet.bytes, inspection.transport.payload_offset_bytes)
        {
            return ActiveAutomarkerDisposition::PassThrough;
        }
        // A complete retained frame has both exact declared lengths and no
        // compression bit at either layer. These cheap checks prevent the
        // deep carrier decoder from ever accepting split, concatenated, or
        // compressed input.
        if u32::from_be_bytes(payload[0..4].try_into().unwrap()) as usize != EXACT_CARRIER_BYTES
            || u16::from_be_bytes(payload[4..6].try_into().unwrap()) & 0x8000 != 0
            || u32::from_be_bytes(payload[10..14].try_into().unwrap()) as usize
                != EXACT_CARRIER_BYTES - 10
            || u16::from_be_bytes(payload[14..16].try_into().unwrap()) & 0x8000 != 0
        {
            return ActiveAutomarkerDisposition::PassThrough;
        }
        self.arm_exact_carrier(
            packet,
            inspection.transport.sequence,
            inspection.transport.payload_offset_bytes,
            payload,
        )
    }

    fn cancel_after_checksum_failure(
        &mut self,
        preparation_id: u64,
    ) -> ActiveAutomarkerCancelDisposition {
        let Some(adapter) = self.adapter.as_mut() else {
            return ActiveAutomarkerCancelDisposition::AbortWithoutReinject;
        };
        let disposition = match adapter.cancel_prepared_send(preparation_id) {
            Ok(AutomarkerBridgePrepareDisposition::SendOriginal(_)) => {
                ActiveAutomarkerCancelDisposition::ReinjectHeldOriginal
            }
            _ => ActiveAutomarkerCancelDisposition::AbortWithoutReinject,
        };
        self.pending_packet_len = None;
        disposition
    }

    fn record_modified_send_may_begin(
        &mut self,
        preparation_id: u64,
    ) -> ActiveAutomarkerCommitDisposition {
        let Some(adapter) = self.adapter.as_mut() else {
            return ActiveAutomarkerCommitDisposition::AbortWithoutReinject;
        };
        match adapter.record_modified_send_may_begin(preparation_id) {
            Ok(BridgeCommitDisposition::Committed) => ActiveAutomarkerCommitDisposition::Committed,
            _ => ActiveAutomarkerCommitDisposition::AbortWithoutReinject,
        }
    }

    fn commit_send(
        &mut self,
        preparation_id: u64,
        outcome: ActiveAutomarkerSendOutcome,
    ) -> ActiveAutomarkerCommitDisposition {
        let pending_packet_len = self.pending_packet_len.take().unwrap_or(0);
        let Some(adapter) = self.adapter.as_mut() else {
            return ActiveAutomarkerCommitDisposition::AbortWithoutReinject;
        };
        let outcome = match outcome {
            ActiveAutomarkerSendOutcome::Complete => SingleMarkerXyzExternalSendOutcome::Complete {
                bytes_sent: pending_packet_len,
            },
            ActiveAutomarkerSendOutcome::FailedOrIndeterminate => {
                SingleMarkerXyzExternalSendOutcome::Failed
            }
        };
        match adapter.commit_modified_send(preparation_id, outcome) {
            Ok(BridgeCommitDisposition::Committed) => ActiveAutomarkerCommitDisposition::Committed,
            Ok(BridgeCommitDisposition::CommittedButConfirmationAborted(_)) => {
                ActiveAutomarkerCommitDisposition::CommittedButConfirmationAborted
            }
            _ => ActiveAutomarkerCommitDisposition::AbortWithoutReinject,
        }
    }

    fn observe_timeout(&mut self) -> ActiveAutomarkerTimeoutDisposition {
        self.drain_commands();
        if let Some(adapter) = self.adapter.as_mut() {
            let _ = adapter.observe_timeout();
        }
        if self.termination_requested && !self.rewrite_obligation_active() {
            ActiveAutomarkerTimeoutDisposition::Terminate
        } else {
            ActiveAutomarkerTimeoutDisposition::Continue
        }
    }

    fn rewrite_obligation_active(&self) -> bool {
        self.adapter
            .as_ref()
            .is_some_and(|adapter| adapter.coordinator_state().tcp_rewrite_obligation_active)
    }

    fn drain_unsent_originals(&mut self) -> Vec<ActiveAutomarkerPacket> {
        Vec::new()
    }

    fn discard_retransmission_ledger(&mut self) {
        if !self.rewrite_obligation_active() {
            self.adapter = None;
        }
    }
}

fn tcp_syn(packet: &[u8], payload_offset: usize) -> bool {
    let ip_header_length = usize::from(packet[0] & 0x0f) * 4;
    ip_header_length + 14 <= payload_offset && packet[ip_header_length + 13] & 0x02 != 0
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use rlogs_game_bpsr::{
        AutomarkerIpv4Endpoint, AutomarkerOwnedTcpConnection, AutomarkerWinDivertAddress,
        MappingProvenance, bind_offline_automarker_connection_epoch,
        reviewed_automarker_active_filter_plan,
    };

    use super::*;

    const FRAME_SEQUENCE: u32 = 1_000;
    const FRAME_HEX: &str = concat!(
        "000000c500059abcdef0000000bb0001000000000626ad6600000001123456780003d002",
        "0a9e0108c90110011a4208a68f80800610cd08180120ee90eb99893432140d0000803f",
        "15000000401d0000404025000080403a140d0000a040150000c0401d0000e040250000",
        "00415001580122505a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a12aa4caa2ac8fdad32",
        "5e04f99bf00b0e4f41c1a16cc947339a988bdfdff23a17e44700729e452212f9a7468",
        "773c78838cbab163f32a51e9b458738f925635c7628df04"
    );

    fn connection() -> AutomarkerOwnedTcpConnection {
        AutomarkerOwnedTcpConnection {
            process_id: 42,
            local: AutomarkerIpv4Endpoint {
                address: Ipv4Addr::new(10, 0, 0, 2),
                port: 50_000,
            },
            remote: AutomarkerIpv4Endpoint {
                address: Ipv4Addr::new(203, 0, 113, 7),
                port: 44_321,
            },
        }
    }

    fn binding() -> OfflineAutomarkerConnectionEpochBinding {
        bind_offline_automarker_connection_epoch(42, connection(), 7, true, &[connection()])
            .unwrap()
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
        FRAME_HEX
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }

    fn packet(sequence: u32, flags: u8, payload: &[u8], reverse: bool) -> ActiveAutomarkerPacket {
        let connection = connection();
        let (source, destination) = if reverse {
            (connection.remote, connection.local)
        } else {
            (connection.local, connection.remote)
        };
        let mut bytes = vec![0_u8; 40 + payload.len()];
        bytes[0] = 0x45;
        let length = bytes.len() as u16;
        bytes[2..4].copy_from_slice(&length.to_be_bytes());
        bytes[6..8].copy_from_slice(&0x4000_u16.to_be_bytes());
        bytes[8] = 64;
        bytes[9] = 6;
        bytes[12..16].copy_from_slice(&source.address.octets());
        bytes[16..20].copy_from_slice(&destination.address.octets());
        bytes[20..22].copy_from_slice(&source.port.to_be_bytes());
        bytes[22..24].copy_from_slice(&destination.port.to_be_bytes());
        if reverse {
            bytes[28..32].copy_from_slice(&sequence.to_be_bytes());
        } else {
            bytes[24..28].copy_from_slice(&sequence.to_be_bytes());
        }
        bytes[32] = 5 << 4;
        bytes[33] = flags;
        bytes[40..].copy_from_slice(payload);
        ActiveAutomarkerPacket {
            bytes,
            address: AutomarkerWinDivertAddress::from_opaque_bytes([0; 80]),
        }
    }

    fn coordinator() -> (
        ProductionActiveAutomarkerCoordinator,
        ActiveAutomarkerControl,
    ) {
        let binding = binding();
        ProductionActiveAutomarkerCoordinator::create(
            ActiveAutomarkerCoordinatorConfig {
                pack: pack(),
                filter_plan: reviewed_automarker_active_filter_plan(binding),
                connection_binding: binding,
                session_key: "private-session".into(),
                carrier_capture_sequence: 10,
                scene_family: "mech-facility".into(),
                local_actor_id: 77,
                marker_number: 1,
                target_position: AutomarkerRequestXyz {
                    x: 101.25,
                    y: -22.5,
                    z: 303.75,
                },
                baseline_runtime_revision: 100,
                runtime_revision: 101,
                observation_age_millis: 0,
                baseline_same_number_passive_instance_identities: vec![88],
            },
            4,
        )
        .unwrap()
    }

    #[test]
    fn exact_carrier_is_held_then_committed_without_early_send_claim() {
        let (mut coordinator, _control) = coordinator();
        let carrier = packet(FRAME_SEQUENCE, 0x18, &frame(), false);
        let inspection = coordinator.filter_plan.inspect(&carrier.bytes).unwrap();
        assert_eq!(
            inspection.transport.payload_length_bytes,
            EXACT_CARRIER_BYTES
        );
        let mut scratch = Vec::new();
        assert_eq!(
            decode_observed_automarker_request_into(
                &coordinator.pack,
                &carrier.bytes[inspection.transport.payload_offset_bytes + APPLICATION_OFFSET..],
                &mut scratch,
            )
            .unwrap()
            .marker_number,
            1
        );
        let ActiveAutomarkerDisposition::HoldExactCarrier {
            preparation_id,
            approved_changed_bytes,
        } = coordinator.classify(
            &carrier,
            ActiveAutomarkerClassificationMode::AuthorizeNewCarrier,
        )
        else {
            panic!("exact carrier was not held")
        };
        assert_ne!(approved_changed_bytes, carrier.bytes);
        assert!(!coordinator.rewrite_obligation_active());
        assert_eq!(
            coordinator.record_modified_send_may_begin(preparation_id),
            ActiveAutomarkerCommitDisposition::Committed
        );
        assert!(coordinator.rewrite_obligation_active());
        assert_eq!(
            coordinator.commit_send(preparation_id, ActiveAutomarkerSendOutcome::Complete),
            ActiveAutomarkerCommitDisposition::Committed
        );
        let ack = packet(FRAME_SEQUENCE + EXACT_CARRIER_BYTES as u32, 0x10, &[], true);
        assert_eq!(
            coordinator.classify(&ack, ActiveAutomarkerClassificationMode::DrainExistingOnly),
            ActiveAutomarkerDisposition::PassThrough
        );
        assert!(!coordinator.rewrite_obligation_active());
    }

    #[test]
    fn split_trailing_compressed_and_syn_carriers_are_never_held() {
        let candidates = [
            packet(FRAME_SEQUENCE, 0x18, &frame()[..100], false),
            {
                let mut trailing = frame();
                trailing.push(0);
                packet(FRAME_SEQUENCE, 0x18, &trailing, false)
            },
            {
                let mut compressed = frame();
                compressed[4] |= 0x80;
                packet(FRAME_SEQUENCE, 0x18, &compressed, false)
            },
            packet(FRAME_SEQUENCE, 0x1a, &frame(), false),
        ];
        for candidate in candidates {
            let (mut coordinator, _control) = coordinator();
            assert_eq!(
                coordinator.classify(
                    &candidate,
                    ActiveAutomarkerClassificationMode::AuthorizeNewCarrier
                ),
                ActiveAutomarkerDisposition::PassThrough
            );
            assert!(!coordinator.rewrite_obligation_active());
        }
    }

    #[test]
    fn bounded_control_queue_fails_closed_and_termination_forbids_carrier() {
        let binding = binding();
        let (mut coordinator, control) = ProductionActiveAutomarkerCoordinator::create(
            ActiveAutomarkerCoordinatorConfig {
                pack: pack(),
                filter_plan: reviewed_automarker_active_filter_plan(binding),
                connection_binding: binding,
                session_key: "private-session".into(),
                carrier_capture_sequence: 10,
                scene_family: "mech-facility".into(),
                local_actor_id: 77,
                marker_number: 1,
                target_position: AutomarkerRequestXyz {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                },
                baseline_runtime_revision: 100,
                runtime_revision: 101,
                observation_age_millis: 0,
                baseline_same_number_passive_instance_identities: Vec::new(),
            },
            1,
        )
        .unwrap();
        control.process_terminated().unwrap();
        assert_eq!(control.timeout(), Err("Automarker control queue is full"));
        assert_eq!(
            coordinator.classify(
                &packet(FRAME_SEQUENCE, 0x18, &frame(), false),
                ActiveAutomarkerClassificationMode::AuthorizeNewCarrier,
            ),
            ActiveAutomarkerDisposition::PassThrough
        );
        assert_eq!(
            coordinator.observe_timeout(),
            ActiveAutomarkerTimeoutDisposition::Terminate
        );
    }
}
