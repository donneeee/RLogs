//! Production-shaped coordinator adapter for the active Automarker worker.
//!
//! The adapter is deliberately not wired into the desktop lifecycle. It owns
//! the exact filter/pack/binding, the bridge and its private router clock, and
//! a bounded control ingress. No diagnostic surface returns packet bytes,
//! tuples, session keys, or marker coordinates.

#![allow(dead_code)]

use std::{
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    },
    time::Instant,
};

use rlogs_game_bpsr::{
    AUTOMARKER_REQUEST_BUILD, AutomarkerActiveFilterPlan, AutomarkerActivePacketRole,
    AutomarkerBridgeCommitDisposition as BridgeCommitDisposition, AutomarkerBridgeCoordinator,
    AutomarkerBridgeCoordinatorError, AutomarkerBridgePrepareDisposition,
    AutomarkerConfirmationBaseline, AutomarkerConfirmationState, AutomarkerRequestXyz,
    OfflineAutomarkerConnectionEpochBinding, ProtocolPack, SINGLE_MARKER_XYZ_CANARY_ARM_TOKEN,
    SingleMarkerXyzCanaryConfig, SingleMarkerXyzCanaryContext, SingleMarkerXyzExternalSendOutcome,
    decode_observed_automarker_request_into,
};

use crate::{
    automarker_active_worker::{
        ActiveAutomarkerBoundProcess, ActiveAutomarkerCancelDisposition,
        ActiveAutomarkerClassificationMode, ActiveAutomarkerCommitDisposition,
        ActiveAutomarkerCoordinator, ActiveAutomarkerDisposition, ActiveAutomarkerPacket,
        ActiveAutomarkerSendOutcome, ActiveAutomarkerTimeoutDisposition,
    },
    automarker_confirmation_adapter::{
        PrivateAutomarkerConfirmationAdapter, PrivateAutomarkerConfirmationAdapterState,
        PrivateAutomarkerConfirmationBinding, PrivateAutomarkerTcpObservation,
    },
    automarker_confirmation_router::PrivateParserConfirmationSnapshot,
};

const EXACT_CARRIER_BYTES: usize = 197;
const APPLICATION_OFFSET: usize = 36;
const COMMAND_CAPACITY_MAX: usize = 256;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ActiveAutomarkerCanaryPhase {
    #[default]
    Idle,
    Armed,
    CarrierIntercepted,
    ModifiedSendCommitted,
    AwaitingConfirmation,
    Succeeded,
    Failed,
}

impl ActiveAutomarkerCanaryPhase {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Armed => "armed",
            Self::CarrierIntercepted => "carrier_intercepted",
            Self::ModifiedSendCommitted => "modified_send_committed",
            Self::AwaitingConfirmation => "awaiting_confirmation",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }

    pub(crate) fn terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed)
    }

    fn rank(self) -> u8 {
        match self {
            Self::Idle => 0,
            Self::Armed => 1,
            Self::CarrierIntercepted => 2,
            Self::ModifiedSendCommitted => 3,
            Self::AwaitingConfirmation => 4,
            Self::Succeeded | Self::Failed => 5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActiveAutomarkerCanaryFailureCategory {
    Carrier,
    Transport,
    Confirmation,
    Timeout,
    Connection,
    Lifecycle,
    Internal,
}

impl ActiveAutomarkerCanaryFailureCategory {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Carrier => "carrier",
            Self::Transport => "transport",
            Self::Confirmation => "confirmation",
            Self::Timeout => "timeout",
            Self::Connection => "connection",
            Self::Lifecycle => "lifecycle",
            Self::Internal => "internal",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ActiveAutomarkerCanaryProgress {
    pub phase: ActiveAutomarkerCanaryPhase,
    pub transport_ack_confirmed: bool,
    pub rpc_return_confirmed: bool,
    pub authoritative_marker_confirmed: bool,
    pub failure_category: Option<ActiveAutomarkerCanaryFailureCategory>,
}

#[derive(Clone, Default)]
pub(crate) struct ActiveAutomarkerCanaryProgressSink(Arc<Mutex<ActiveAutomarkerCanaryProgress>>);

impl ActiveAutomarkerCanaryProgressSink {
    pub(crate) fn snapshot(&self) -> ActiveAutomarkerCanaryProgress {
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn update(&self, update: impl FnOnce(&mut ActiveAutomarkerCanaryProgress)) {
        update(
            &mut self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
    }

    pub(crate) fn armed(&self) {
        self.update(|progress| {
            if progress.phase == ActiveAutomarkerCanaryPhase::Idle {
                progress.phase = ActiveAutomarkerCanaryPhase::Armed;
            }
        });
    }

    fn carrier_intercepted(&self) {
        self.advance(ActiveAutomarkerCanaryPhase::CarrierIntercepted);
    }

    fn modified_send_committed_awaiting_confirmation(&self) {
        // Commit and the resulting AwaitingEvidence state are one externally
        // visible milestone. ModifiedSendCommitted remains an accepted schema
        // phase for bounded compatibility, but a successful coordinator never
        // leaves status parked between those two facts.
        self.advance(ActiveAutomarkerCanaryPhase::AwaitingConfirmation);
    }

    fn advance(&self, phase: ActiveAutomarkerCanaryPhase) {
        self.update(|progress| {
            if !progress.phase.terminal() && phase.rank() > progress.phase.rank() {
                progress.phase = phase;
            }
        });
    }

    pub(crate) fn fail(&self, category: ActiveAutomarkerCanaryFailureCategory) {
        self.update(|progress| {
            if !progress.phase.terminal() {
                progress.phase = ActiveAutomarkerCanaryPhase::Failed;
                progress.failure_category = Some(category);
            }
        });
    }

    fn sync_adapter(&self, adapter: &PrivateAutomarkerConfirmationAdapter) {
        let state = adapter.coordinator_state();
        self.update(|progress| {
            if progress.phase.terminal() {
                return;
            }
            if let Some(confirmation) = state.confirmation {
                match confirmation {
                    AutomarkerConfirmationState::AwaitingEvidence {
                        reverse_cumulative_ack,
                        successful_empty_rpc_return,
                        new_authoritative_marker_add,
                    } => {
                        progress.transport_ack_confirmed |= reverse_cumulative_ack;
                        progress.rpc_return_confirmed |= successful_empty_rpc_return;
                        progress.authoritative_marker_confirmed |= new_authoritative_marker_add;
                        if ActiveAutomarkerCanaryPhase::AwaitingConfirmation.rank()
                            > progress.phase.rank()
                        {
                            progress.phase = ActiveAutomarkerCanaryPhase::AwaitingConfirmation;
                        }
                    }
                    AutomarkerConfirmationState::Confirmed => {
                        progress.transport_ack_confirmed = true;
                        progress.rpc_return_confirmed = true;
                        progress.authoritative_marker_confirmed = true;
                        progress.phase = if state.tcp_rewrite_obligation_active {
                            ActiveAutomarkerCanaryPhase::AwaitingConfirmation
                        } else {
                            ActiveAutomarkerCanaryPhase::Succeeded
                        };
                    }
                    AutomarkerConfirmationState::Aborted(_) => {
                        progress.phase = ActiveAutomarkerCanaryPhase::Failed;
                        progress.failure_category =
                            Some(ActiveAutomarkerCanaryFailureCategory::Confirmation);
                    }
                }
            }
            if state.coordinator_error.is_some()
                || matches!(
                    adapter.state(),
                    PrivateAutomarkerConfirmationAdapterState::Failed
                        | PrivateAutomarkerConfirmationAdapterState::ConfirmationFailedAwaitingTransportRetirement
                )
            {
                progress.phase = ActiveAutomarkerCanaryPhase::Failed;
                progress.failure_category
                    .get_or_insert(ActiveAutomarkerCanaryFailureCategory::Confirmation);
            }
        });
    }

    pub(crate) fn terminal(&self) -> bool {
        self.snapshot().phase.terminal()
    }
}

enum ActiveAutomarkerCommand {
    ParserCarrier {
        capture_sequence: u64,
        rpc_call_id: u32,
    },
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
    pub(crate) fn parser_carrier(
        &self,
        capture_sequence: u64,
        rpc_call_id: u32,
    ) -> Result<(), &'static str> {
        self.send(ActiveAutomarkerCommand::ParserCarrier {
            capture_sequence,
            rpc_call_id,
        })
    }

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
    context_age_started_at: Instant,
    baseline_same_number_passive_instance_identities: Vec<i64>,
    commands: Receiver<ActiveAutomarkerCommand>,
    adapter: Option<PrivateAutomarkerConfirmationAdapter>,
    pending_packet_len: Option<usize>,
    pending_parser_carrier: Option<(u64, u32)>,
    context_invalidated: bool,
    termination_requested: bool,
    progress: ActiveAutomarkerCanaryProgressSink,
}

impl ProductionActiveAutomarkerCoordinator {
    pub(crate) fn create(
        config: ActiveAutomarkerCoordinatorConfig,
        command_capacity: usize,
    ) -> Result<
        (
            Self,
            ActiveAutomarkerControl,
            ActiveAutomarkerCanaryProgressSink,
        ),
        &'static str,
    > {
        if command_capacity == 0 || command_capacity > COMMAND_CAPACITY_MAX {
            return Err("Automarker command capacity is outside the reviewed bound");
        }
        if config.pack.definition().target.build_id != AUTOMARKER_REQUEST_BUILD
            || config.filter_plan.connection_epoch() != config.connection_binding.connection_epoch()
            || config.session_key.trim().is_empty()
            || config.carrier_capture_sequence == 0
            || config.scene_family.trim().is_empty()
            || config.local_actor_id == 0
            || config.marker_number != 1
            || config.baseline_runtime_revision == 0
            || config.runtime_revision < config.baseline_runtime_revision
            || config.observation_age_millis
                > rlogs_game_bpsr::SINGLE_MARKER_XYZ_MAX_CONTEXT_AGE_MILLIS
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
        let progress = ActiveAutomarkerCanaryProgressSink::default();
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
                context_age_started_at: Instant::now(),
                baseline_same_number_passive_instance_identities: config
                    .baseline_same_number_passive_instance_identities,
                commands,
                adapter: None,
                pending_packet_len: None,
                pending_parser_carrier: None,
                context_invalidated: false,
                termination_requested: false,
                progress: progress.clone(),
            },
            control,
            progress,
        ))
    }

    fn drain_commands(&mut self) {
        loop {
            match self.commands.try_recv() {
                Ok(ActiveAutomarkerCommand::ParserCarrier {
                    capture_sequence,
                    rpc_call_id,
                }) => {
                    if let Some(adapter) = self.adapter.as_mut() {
                        if adapter
                            .bind_source_carrier(capture_sequence, rpc_call_id)
                            .is_err()
                        {
                            self.context_invalidated = true;
                        } else {
                            self.carrier_capture_sequence = capture_sequence;
                        }
                    } else if capture_sequence > self.carrier_capture_sequence
                        && rpc_call_id != 0
                        && self.pending_parser_carrier.is_none()
                    {
                        self.pending_parser_carrier = Some((capture_sequence, rpc_call_id));
                    } else {
                        self.context_invalidated = true;
                    }
                }
                Ok(ActiveAutomarkerCommand::ParserSnapshot(snapshot)) => {
                    self.runtime_revision =
                        self.runtime_revision.max(snapshot.context.runtime_revision);
                    if let Some(adapter) = self.adapter.as_mut() {
                        if adapter.route_parser_snapshot(snapshot).is_err() {
                            self.context_invalidated = true;
                            self.progress
                                .fail(ActiveAutomarkerCanaryFailureCategory::Confirmation);
                        } else {
                            self.progress.sync_adapter(adapter);
                        }
                    }
                }
                Ok(ActiveAutomarkerCommand::ContextInvalidated) => {
                    self.context_invalidated = true;
                    self.progress
                        .fail(ActiveAutomarkerCanaryFailureCategory::Lifecycle);
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
                    self.progress
                        .fail(ActiveAutomarkerCanaryFailureCategory::Connection);
                    if let Some(adapter) = self.adapter.as_mut() {
                        let _ = adapter.observe_connection_terminated(
                            self.connection_binding.connection_epoch(),
                        );
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.termination_requested = true;
                    self.progress
                        .fail(ActiveAutomarkerCanaryFailureCategory::Lifecycle);
                    break;
                }
            }
        }
    }

    fn current_context_age_millis(&self) -> u64 {
        self.observation_age_millis.saturating_add(
            self.context_age_started_at
                .elapsed()
                .as_millis()
                .min(u128::from(u64::MAX)) as u64,
        )
    }

    fn classify_existing(
        &mut self,
        packet: &ActiveAutomarkerPacket,
        tcp_sequence_start: u32,
        tcp_payload_offset: usize,
    ) -> ActiveAutomarkerDisposition {
        let context_age_millis = self.current_context_age_millis();
        let Some(adapter) = self.adapter.as_mut() else {
            return ActiveAutomarkerDisposition::PassThrough;
        };
        if matches!(
            adapter.state(),
            PrivateAutomarkerConfirmationAdapterState::Complete
                | PrivateAutomarkerConfirmationAdapterState::Failed
        ) && !adapter.coordinator_state().tcp_rewrite_obligation_active
        {
            return ActiveAutomarkerDisposition::PassThrough;
        }
        match adapter.prepare_outbound_packet(
            self.connection_binding.connection_epoch(),
            tcp_sequence_start,
            tcp_payload_offset,
            &packet.bytes,
            packet.address,
            self.runtime_revision,
            context_age_millis,
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
        let context_age_millis = self.current_context_age_millis();
        if context_age_millis > rlogs_game_bpsr::SINGLE_MARKER_XYZ_MAX_CONTEXT_AGE_MILLIS {
            self.termination_requested = true;
            return ActiveAutomarkerDisposition::PassThrough;
        }
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
        let carrier_capture_sequence = match self.pending_parser_carrier.take() {
            Some((capture_sequence, rpc_call_id)) if rpc_call_id == carrier_rpc_call_id => {
                self.carrier_capture_sequence = capture_sequence;
                capture_sequence
            }
            Some(_) => {
                self.context_invalidated = true;
                return ActiveAutomarkerDisposition::PassThrough;
            }
            None => self.carrier_capture_sequence,
        };
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
            carrier_capture_sequence,
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
                    observation_age_millis: context_age_millis,
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
                    observation_age_millis: context_age_millis,
                };
                let identity = coordinator.observe_fresh_carrier(
                    &self.pack,
                    self.connection_binding.connection_epoch(),
                    frame_sequence_start,
                    context_age_millis,
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
                self.progress
                    .fail(ActiveAutomarkerCanaryFailureCategory::Carrier);
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
        self.progress.carrier_intercepted();
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
                self.progress.sync_adapter(adapter);
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
                self.progress
                    .fail(ActiveAutomarkerCanaryFailureCategory::Transport);
                ActiveAutomarkerCancelDisposition::ReinjectHeldOriginal
            }
            _ => {
                self.progress
                    .fail(ActiveAutomarkerCanaryFailureCategory::Transport);
                ActiveAutomarkerCancelDisposition::AbortWithoutReinject
            }
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
            _ => {
                self.progress
                    .fail(ActiveAutomarkerCanaryFailureCategory::Transport);
                ActiveAutomarkerCommitDisposition::AbortWithoutReinject
            }
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
            Ok(BridgeCommitDisposition::Committed) => {
                self.progress
                    .modified_send_committed_awaiting_confirmation();
                ActiveAutomarkerCommitDisposition::Committed
            }
            Ok(BridgeCommitDisposition::CommittedButConfirmationAborted(_)) => {
                self.progress
                    .fail(ActiveAutomarkerCanaryFailureCategory::Confirmation);
                ActiveAutomarkerCommitDisposition::CommittedButConfirmationAborted
            }
            _ => {
                self.progress
                    .fail(ActiveAutomarkerCanaryFailureCategory::Transport);
                ActiveAutomarkerCommitDisposition::AbortWithoutReinject
            }
        }
    }

    fn observe_timeout(&mut self) -> ActiveAutomarkerTimeoutDisposition {
        self.drain_commands();
        if self.current_context_age_millis()
            > rlogs_game_bpsr::SINGLE_MARKER_XYZ_MAX_CONTEXT_AGE_MILLIS
        {
            self.termination_requested = true;
            self.progress
                .fail(ActiveAutomarkerCanaryFailureCategory::Timeout);
        }
        if let Some(adapter) = self.adapter.as_mut() {
            let _ = adapter.observe_timeout();
            self.progress.sync_adapter(adapter);
        }
        let terminal_without_obligation = self.adapter.as_ref().is_some_and(|adapter| {
            matches!(
                adapter.state(),
                PrivateAutomarkerConfirmationAdapterState::Complete
                    | PrivateAutomarkerConfirmationAdapterState::Failed
            ) && !adapter.coordinator_state().tcp_rewrite_obligation_active
        });
        if (self.termination_requested || terminal_without_obligation)
            && !self.rewrite_obligation_active()
        {
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

    fn retained_bound_process(&self) -> Option<ActiveAutomarkerBoundProcess> {
        self.rewrite_obligation_active()
            .then_some(ActiveAutomarkerBoundProcess {
                process_id: self.connection_binding.process_id(),
                connection_epoch: self.connection_binding.connection_epoch(),
            })
    }

    fn observe_bound_process_terminated(&mut self, proof: ActiveAutomarkerBoundProcess) -> bool {
        if proof.process_id != self.connection_binding.process_id()
            || proof.connection_epoch != self.connection_binding.connection_epoch()
        {
            return false;
        }
        let Some(adapter) = self.adapter.as_mut() else {
            return false;
        };
        let _ = adapter.observe_connection_terminated(proof.connection_epoch);
        self.progress.sync_adapter(adapter);
        if !self.progress.terminal() {
            self.progress
                .fail(ActiveAutomarkerCanaryFailureCategory::Connection);
        }
        !adapter.coordinator_state().tcp_rewrite_obligation_active
    }
}

fn tcp_syn(packet: &[u8], payload_offset: usize) -> bool {
    let ip_header_length = usize::from(packet[0] & 0x0f) * 4;
    ip_header_length + 14 <= payload_offset && packet[ip_header_length + 13] & 0x02 != 0
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::{Duration, Instant};

    use rlogs_game_bpsr::{
        AutomarkerIpv4Endpoint, AutomarkerOwnedTcpConnection, AutomarkerWinDivertAddress,
        MappingProvenance, bind_offline_automarker_connection_epoch,
        reviewed_automarker_active_filter_plan,
    };

    use super::*;
    use crate::automarker_active_worker::{
        ActiveAutomarkerBackend, ActiveAutomarkerWake, ActiveAutomarkerWorkerBundle,
        ActiveAutomarkerWorkerExit,
    };
    use crate::automarker_confirmation_router::{
        PrivateConfirmationContext, PrivateConfirmationProvenance, PrivateCorrelatedReturn,
        PrivateFragmentKind, PrivateMarkerAdd, PrivatePacketDirection,
        PrivateParserConfirmationEvent, PrivateSourceClocks,
    };

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
        let (coordinator, control, _progress) = coordinator_with_progress();
        (coordinator, control)
    }

    fn coordinator_with_progress() -> (
        ProductionActiveAutomarkerCoordinator,
        ActiveAutomarkerControl,
        ActiveAutomarkerCanaryProgressSink,
    ) {
        ProductionActiveAutomarkerCoordinator::create(coordinator_config(1, 0), 4).unwrap()
    }

    fn coordinator_config(
        marker_number: u8,
        observation_age_millis: u64,
    ) -> ActiveAutomarkerCoordinatorConfig {
        let binding = binding();
        ActiveAutomarkerCoordinatorConfig {
            pack: pack(),
            filter_plan: reviewed_automarker_active_filter_plan(binding),
            connection_binding: binding,
            session_key: "private-session".into(),
            carrier_capture_sequence: 10,
            scene_family: "mech-facility".into(),
            local_actor_id: 77,
            marker_number,
            target_position: AutomarkerRequestXyz {
                x: 101.25,
                y: -22.5,
                z: 303.75,
            },
            baseline_runtime_revision: 100,
            runtime_revision: 101,
            observation_age_millis,
            baseline_same_number_passive_instance_identities: vec![88],
        }
    }

    fn confirmation_snapshot() -> PrivateParserConfirmationSnapshot {
        let context = PrivateConfirmationContext {
            game_build: AUTOMARKER_REQUEST_BUILD.into(),
            scene_family: "mech-facility".into(),
            local_actor_id: 77,
            connection_epoch: 7,
            client_address: connection().local.address.octets(),
            client_port: connection().local.port,
            server_address: connection().remote.address.octets(),
            server_port: connection().remote.port,
            runtime_revision: 102,
        };
        let provenance = |capture_sequence, fragment, service_id, method_id, call_id| {
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
        };
        PrivateParserConfirmationSnapshot {
            session_key: "private-session".into(),
            context,
            events: vec![
                PrivateParserConfirmationEvent::CorrelatedReturn(PrivateCorrelatedReturn {
                    provenance: provenance(
                        11,
                        PrivateFragmentKind::Return,
                        103_198_054,
                        249_858,
                        Some(0x1234_5678),
                    ),
                    source_clocks: PrivateSourceClocks {
                        observed_micros: 1,
                        wall_clock_unix_micros: None,
                    },
                    carrier_capture_sequence: 10,
                    raw_stub_id: 1,
                    raw_status: 0,
                    asserted_authoritative_server_decode: true,
                    decoded_as_success: true,
                    decoded_body_present: true,
                    decoded_body_length: 0,
                }),
                PrivateParserConfirmationEvent::MarkerAdd(PrivateMarkerAdd {
                    provenance: provenance(
                        12,
                        PrivateFragmentKind::Notify,
                        1_664_308_034,
                        46,
                        None,
                    ),
                    source_clocks: PrivateSourceClocks {
                        observed_micros: 2,
                        wall_clock_unix_micros: None,
                    },
                    asserted_authoritative_server_decode: true,
                    raw_skill_id: Some(1_101),
                    derived_marker_number: Some(1),
                    marker_owner_actor_id: Some(77),
                    marker_owner_entity_uuid: Some(8_888),
                    passive_instance_identity: Some(89),
                    target_position_present: true,
                    target_position_decode_valid: true,
                    x: Some(101.25),
                    y: Some(-22.5),
                    z: Some(303.75),
                    runtime_revision: 102,
                }),
            ],
        }
    }

    fn commit_carrier(
        coordinator: &mut ProductionActiveAutomarkerCoordinator,
        outcome: ActiveAutomarkerSendOutcome,
    ) {
        let carrier = packet(FRAME_SEQUENCE, 0x18, &frame(), false);
        let ActiveAutomarkerDisposition::HoldExactCarrier { preparation_id, .. } = coordinator
            .classify(
                &carrier,
                ActiveAutomarkerClassificationMode::AuthorizeNewCarrier,
            )
        else {
            panic!("exact carrier was not held")
        };
        assert_eq!(
            coordinator.record_modified_send_may_begin(preparation_id),
            ActiveAutomarkerCommitDisposition::Committed
        );
        let _ = coordinator.commit_send(preparation_id, outcome);
    }

    #[derive(Default)]
    struct WorkerHarnessState {
        wakes: std::collections::VecDeque<ActiveAutomarkerWake>,
        prepared_modified: Option<Vec<u8>>,
        modified_sent: usize,
        closed: bool,
    }

    #[derive(Clone, Default)]
    struct WorkerHarness(Arc<(Mutex<WorkerHarnessState>, Condvar)>);

    impl WorkerHarness {
        fn push(&self, wake: ActiveAutomarkerWake) {
            let (lock, ready) = &*self.0;
            lock.lock().unwrap().wakes.push_back(wake);
            ready.notify_all();
        }

        fn wait_until(&self, predicate: impl Fn(&WorkerHarnessState) -> bool) {
            let (lock, ready) = &*self.0;
            let deadline = Instant::now() + Duration::from_secs(2);
            let mut state = lock.lock().unwrap();
            while !predicate(&state) {
                let remaining = deadline.saturating_duration_since(Instant::now());
                assert!(!remaining.is_zero(), "active worker test timed out");
                state = ready.wait_timeout(state, remaining).unwrap().0;
            }
        }
    }

    struct WorkerBackend(WorkerHarness);

    impl ActiveAutomarkerBackend for WorkerBackend {
        fn shutdown_receive(&mut self) -> Result<(), String> {
            let (lock, ready) = &*self.0.0;
            lock.lock()
                .unwrap()
                .wakes
                .push_back(ActiveAutomarkerWake::EndOfStream);
            ready.notify_all();
            Ok(())
        }

        fn receive(&mut self) -> Result<ActiveAutomarkerWake, String> {
            let (lock, ready) = &*self.0.0;
            let mut state = lock.lock().unwrap();
            if state.wakes.is_empty() {
                state = ready
                    .wait_timeout(state, Duration::from_millis(20))
                    .unwrap()
                    .0;
            }
            Ok(state
                .wakes
                .pop_front()
                .unwrap_or(ActiveAutomarkerWake::Timeout))
        }

        fn prepare_modified_send(
            &mut self,
            original: &ActiveAutomarkerPacket,
            approved_changed_bytes: &[u8],
        ) -> Result<rlogs_game_bpsr::AutomarkerPacketSendPreparation, String> {
            let mut address = original.address.into_opaque_bytes();
            let mut flags = u32::from_ne_bytes(address[8..12].try_into().unwrap());
            flags |= (1 << 21) | (1 << 22);
            address[8..12].copy_from_slice(&flags.to_ne_bytes());
            self.0.0.0.lock().unwrap().prepared_modified = Some(approved_changed_bytes.to_vec());
            Ok(
                rlogs_game_bpsr::AutomarkerPacketSendPreparation::ModifiedAuthorized {
                    packet: approved_changed_bytes.to_vec(),
                    address: rlogs_game_bpsr::AutomarkerWinDivertAddress::from_opaque_bytes(
                        address,
                    ),
                },
            )
        }

        fn send(&mut self, packet: &ActiveAutomarkerPacket) -> Result<usize, String> {
            let (lock, ready) = &*self.0.0;
            let mut state = lock.lock().unwrap();
            if state.prepared_modified.as_deref() == Some(packet.bytes.as_slice()) {
                state.modified_sent += 1;
            }
            ready.notify_all();
            Ok(packet.bytes.len())
        }

        fn close_interception(&mut self) -> Result<(), String> {
            let (lock, ready) = &*self.0.0;
            lock.lock().unwrap().closed = true;
            ready.notify_all();
            Ok(())
        }
    }

    #[test]
    fn worker_runs_one_marker_confirmation_lifecycle_and_closes_on_completion() {
        let harness = WorkerHarness::default();
        let (coordinator, control) = coordinator();
        let bundle =
            ActiveAutomarkerWorkerBundle::spawn(WorkerBackend(harness.clone()), coordinator)
                .unwrap();

        harness.push(ActiveAutomarkerWake::Packet(packet(
            FRAME_SEQUENCE,
            0x18,
            &frame(),
            false,
        )));
        harness.wait_until(|state| state.modified_sent == 1);
        control.parser_carrier(11, 0x1234_5678).unwrap();
        let mut confirmation = confirmation_snapshot();
        if let PrivateParserConfirmationEvent::CorrelatedReturn(returned) =
            &mut confirmation.events[0]
        {
            returned.provenance.capture_sequence = 12;
            returned.carrier_capture_sequence = 11;
        }
        if let PrivateParserConfirmationEvent::MarkerAdd(marker) = &mut confirmation.events[1] {
            marker.provenance.capture_sequence = 13;
        }
        control.parser_snapshot(confirmation).unwrap();
        harness.push(ActiveAutomarkerWake::Packet(packet(
            FRAME_SEQUENCE + EXACT_CARRIER_BYTES as u32,
            0x10,
            &[],
            true,
        )));

        let completion = bundle.join().unwrap();
        assert_eq!(completion.exit, ActiveAutomarkerWorkerExit::Timeout);
        assert_eq!(completion.modified_sent, 1);
        assert!(completion.fatal_ownership.is_none());
        harness.wait_until(|state| state.closed);
    }

    #[test]
    fn sanitized_progress_tracks_only_active_rewrite_and_three_signal_confirmation() {
        let (mut coordinator, control, progress) = coordinator_with_progress();
        assert_eq!(progress.snapshot().phase, ActiveAutomarkerCanaryPhase::Idle);
        progress.armed();
        assert_eq!(
            progress.snapshot().phase,
            ActiveAutomarkerCanaryPhase::Armed
        );

        let carrier = packet(FRAME_SEQUENCE, 0x18, &frame(), false);
        let ActiveAutomarkerDisposition::HoldExactCarrier { preparation_id, .. } = coordinator
            .classify(
                &carrier,
                ActiveAutomarkerClassificationMode::AuthorizeNewCarrier,
            )
        else {
            panic!("exact carrier was not held")
        };
        assert_eq!(
            progress.snapshot().phase,
            ActiveAutomarkerCanaryPhase::CarrierIntercepted
        );
        assert_eq!(
            coordinator.record_modified_send_may_begin(preparation_id),
            ActiveAutomarkerCommitDisposition::Committed
        );
        assert_eq!(
            coordinator.commit_send(preparation_id, ActiveAutomarkerSendOutcome::Complete),
            ActiveAutomarkerCommitDisposition::Committed
        );
        assert_eq!(
            progress.snapshot().phase,
            ActiveAutomarkerCanaryPhase::AwaitingConfirmation
        );

        control.parser_snapshot(confirmation_snapshot()).unwrap();
        assert_eq!(
            coordinator.observe_timeout(),
            ActiveAutomarkerTimeoutDisposition::Continue
        );
        let awaiting = progress.snapshot();
        assert_eq!(
            awaiting.phase,
            ActiveAutomarkerCanaryPhase::AwaitingConfirmation
        );
        assert!(!awaiting.transport_ack_confirmed);
        assert!(awaiting.rpc_return_confirmed);
        assert!(awaiting.authoritative_marker_confirmed);

        let ack = packet(FRAME_SEQUENCE + EXACT_CARRIER_BYTES as u32, 0x10, &[], true);
        assert_eq!(
            coordinator.classify(&ack, ActiveAutomarkerClassificationMode::DrainExistingOnly),
            ActiveAutomarkerDisposition::PassThrough
        );
        let succeeded = progress.snapshot();
        assert_eq!(succeeded.phase, ActiveAutomarkerCanaryPhase::Succeeded);
        assert!(succeeded.transport_ack_confirmed);
        assert!(succeeded.rpc_return_confirmed);
        assert!(succeeded.authoritative_marker_confirmed);
        assert_eq!(succeeded.failure_category, None);
        progress.fail(ActiveAutomarkerCanaryFailureCategory::Internal);
        assert_eq!(progress.snapshot(), succeeded);
    }

    #[test]
    fn indeterminate_send_is_a_bounded_transport_failure() {
        let (mut coordinator, _control, progress) = coordinator_with_progress();
        commit_carrier(
            &mut coordinator,
            ActiveAutomarkerSendOutcome::FailedOrIndeterminate,
        );
        assert_eq!(
            progress.snapshot(),
            ActiveAutomarkerCanaryProgress {
                phase: ActiveAutomarkerCanaryPhase::Failed,
                failure_category: Some(ActiveAutomarkerCanaryFailureCategory::Transport),
                ..ActiveAutomarkerCanaryProgress::default()
            }
        );
        progress.fail(ActiveAutomarkerCanaryFailureCategory::Internal);
        assert_eq!(
            progress.snapshot().failure_category,
            Some(ActiveAutomarkerCanaryFailureCategory::Transport)
        );
    }

    #[test]
    fn marker_two_and_already_stale_context_are_rejected_at_construction() {
        assert!(
            ProductionActiveAutomarkerCoordinator::create(coordinator_config(2, 0), 4).is_err()
        );
        assert!(
            ProductionActiveAutomarkerCoordinator::create(
                coordinator_config(
                    1,
                    rlogs_game_bpsr::SINGLE_MARKER_XYZ_MAX_CONTEXT_AGE_MILLIS + 1,
                ),
                4,
            )
            .is_err()
        );
    }

    #[test]
    fn hours_late_carrier_passes_unchanged_and_worker_terminates_without_a_carrier() {
        let harness = WorkerHarness::default();
        let (mut coordinator, _control) = coordinator();
        coordinator.context_age_started_at = Instant::now()
            .checked_sub(Duration::from_secs(60 * 60))
            .unwrap();
        assert_eq!(
            coordinator.classify(
                &packet(FRAME_SEQUENCE, 0x18, &frame(), false),
                ActiveAutomarkerClassificationMode::AuthorizeNewCarrier,
            ),
            ActiveAutomarkerDisposition::PassThrough
        );
        let completion =
            ActiveAutomarkerWorkerBundle::spawn(WorkerBackend(harness.clone()), coordinator)
                .unwrap()
                .join()
                .unwrap();
        assert_eq!(completion.exit, ActiveAutomarkerWorkerExit::Timeout);
        assert_eq!(completion.modified_sent, 0);
        harness.wait_until(|state| state.closed);
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
        let (mut coordinator, control, _progress) = ProductionActiveAutomarkerCoordinator::create(
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

    #[test]
    fn packets_after_confirmed_terminal_state_pass_through_and_worker_completes() {
        let (mut coordinator, control) = coordinator();
        commit_carrier(&mut coordinator, ActiveAutomarkerSendOutcome::Complete);
        control.parser_snapshot(confirmation_snapshot()).unwrap();
        let ack = packet(FRAME_SEQUENCE + EXACT_CARRIER_BYTES as u32, 0x10, &[], true);
        assert_eq!(
            coordinator.classify(&ack, ActiveAutomarkerClassificationMode::DrainExistingOnly),
            ActiveAutomarkerDisposition::PassThrough
        );
        assert_eq!(
            coordinator.adapter.as_ref().unwrap().state(),
            PrivateAutomarkerConfirmationAdapterState::Complete
        );
        let next_outbound = packet(9_000, 0x18, &[1, 2, 3], false);
        assert_eq!(
            coordinator.classify(
                &next_outbound,
                ActiveAutomarkerClassificationMode::AuthorizeNewCarrier
            ),
            ActiveAutomarkerDisposition::PassThrough
        );
        assert_eq!(
            coordinator.classify(&ack, ActiveAutomarkerClassificationMode::DrainExistingOnly),
            ActiveAutomarkerDisposition::PassThrough
        );
        assert_eq!(
            coordinator.observe_timeout(),
            ActiveAutomarkerTimeoutDisposition::Terminate
        );
    }

    #[test]
    fn packets_after_failed_send_retirement_pass_through_and_worker_completes() {
        let (mut coordinator, _control) = coordinator();
        commit_carrier(
            &mut coordinator,
            ActiveAutomarkerSendOutcome::FailedOrIndeterminate,
        );
        assert!(coordinator.rewrite_obligation_active());
        assert_eq!(
            coordinator.retained_bound_process(),
            Some(ActiveAutomarkerBoundProcess {
                process_id: 42,
                connection_epoch: 7,
            })
        );
        assert!(
            !coordinator.observe_bound_process_terminated(ActiveAutomarkerBoundProcess {
                process_id: 43,
                connection_epoch: 7,
            })
        );
        let ack = packet(FRAME_SEQUENCE + EXACT_CARRIER_BYTES as u32, 0x10, &[], true);
        assert_eq!(
            coordinator.classify(&ack, ActiveAutomarkerClassificationMode::DrainExistingOnly),
            ActiveAutomarkerDisposition::PassThrough
        );
        assert!(!coordinator.rewrite_obligation_active());
        assert_eq!(
            coordinator.adapter.as_ref().unwrap().state(),
            PrivateAutomarkerConfirmationAdapterState::Failed
        );
        assert_eq!(
            coordinator.classify(
                &packet(9_000, 0x18, &[1, 2, 3], false),
                ActiveAutomarkerClassificationMode::AuthorizeNewCarrier,
            ),
            ActiveAutomarkerDisposition::PassThrough
        );
        assert_eq!(
            coordinator.classify(&ack, ActiveAutomarkerClassificationMode::DrainExistingOnly),
            ActiveAutomarkerDisposition::PassThrough
        );
        assert_eq!(
            coordinator.observe_timeout(),
            ActiveAutomarkerTimeoutDisposition::Terminate
        );
    }
}
