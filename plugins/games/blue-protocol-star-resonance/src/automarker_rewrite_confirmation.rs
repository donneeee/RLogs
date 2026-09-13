//! Pure, research-only confirmation contract for one marker rewrite.
//!
//! The contract consumes already-decoded evidence. It owns no capture handle,
//! driver, socket, process, memory, UI, or send capability. In particular, a
//! positive result is retrospective evidence and is never send authorization.

use crate::AutomarkerRequestXyz;

pub const AUTOMARKER_OUTBOUND_CARRIER_METHOD_ID: u32 = 249_858;
pub const AUTOMARKER_AUTHORITATIVE_MARKER_ADD_METHOD_ID: u32 = 46;
pub const AUTOMARKER_CONFIRMATION_TIMEOUT_MILLIS: u64 = 2_000;
pub const AUTOMARKER_CONFIRMATION_TIMEOUT_MICROS: u64 =
    AUTOMARKER_CONFIRMATION_TIMEOUT_MILLIS * 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutomarkerConfirmationTcpTuple {
    pub client_address: [u8; 4],
    pub client_port: u16,
    pub server_address: [u8; 4],
    pub server_port: u16,
}

impl AutomarkerConfirmationTcpTuple {
    fn is_valid(self) -> bool {
        self.client_port != 0
            && self.server_port != 0
            && self.client_address != [0; 4]
            && self.server_address != [0; 4]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutomarkerConfirmationObservationStamp {
    /// Bridge-assigned monotonic observation order. This is ordering evidence,
    /// not a packet sequence number or permission to transmit.
    pub observation_ordinal: u64,
    pub observed_micros: u64,
    pub elapsed_since_rewrite_millis: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomarkerConfirmationContext {
    pub game_build: String,
    pub scene_family: String,
    pub local_actor_id: i64,
    pub connection_epoch: u64,
    pub client_to_server_tuple: AutomarkerConfirmationTcpTuple,
    pub runtime_revision: u64,
    pub observed_micros: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomarkerConfirmationBaseline {
    pub context: AutomarkerConfirmationContext,
    pub observation_ordinal: u64,
    /// All passive instance identities for the target marker number in the
    /// authoritative pre-rewrite snapshot. More than one is ambiguous.
    pub same_number_passive_instance_identities: Vec<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutomarkerConfirmationRewrite {
    pub method_id: u32,
    pub original_rpc_call_id: u32,
    pub mapped_tcp_sequence_start: u32,
    pub mapped_tcp_length: u32,
    pub runtime_revision: u64,
    pub observed_micros: u64,
    pub observation_ordinal: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AutomarkerConfirmationConfig {
    pub expected_game_build: String,
    pub expected_scene_family: String,
    pub marker_number: u8,
    pub target_position: AutomarkerRequestXyz,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutomarkerConfirmationTcpObservation {
    pub connection_epoch: u64,
    pub source_address: [u8; 4],
    pub source_port: u16,
    pub destination_address: [u8; 4],
    pub destination_port: u16,
    pub ack_flag: bool,
    pub cumulative_ack: u32,
    pub fin: bool,
    pub rst: bool,
    pub stamp: AutomarkerConfirmationObservationStamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutomarkerConfirmationRpcReturn {
    pub method_id: u32,
    pub original_call_id: u32,
    /// Caller assertion from the future bridge's trusted inbound decoder. It
    /// is confirmation provenance only, never permission to transmit.
    pub asserted_decoded_from_authoritative_server_stream: bool,
    pub decoded_as_success: bool,
    pub decoded_body_length: usize,
    pub stamp: AutomarkerConfirmationObservationStamp,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutomarkerConfirmationMarkerAdd {
    pub method_id: u32,
    pub marker_number: u8,
    pub marker_owner_actor_id: i64,
    pub position: AutomarkerRequestXyz,
    pub passive_instance_identity: i64,
    /// The bridge must derive this from the trusted inbound decode path. Its
    /// name records that it remains caller-supplied to this pure contract.
    pub asserted_decoded_from_authoritative_server_stream: bool,
    pub runtime_revision: u64,
    pub observed_micros: u64,
    pub stamp: AutomarkerConfirmationObservationStamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomarkerConfirmationState {
    AwaitingEvidence {
        reverse_cumulative_ack: bool,
        successful_empty_rpc_return: bool,
        new_authoritative_marker_add: bool,
    },
    Confirmed,
    Aborted(AutomarkerConfirmationError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomarkerConfirmationError {
    InvalidBaseline,
    AmbiguousBaselineMarker,
    InvalidRewrite,
    WrongRewriteMethod,
    InvalidMarkerNumber,
    InvalidTargetPosition,
    BuildChanged,
    SceneChanged,
    LocalActorChanged,
    SocketChanged,
    ConnectionEpochChanged,
    RuntimeRevisionRegressed,
    ObservationNotPostRewrite,
    StaleOrReorderedObservation,
    InconsistentObservationAge,
    TimedOut,
    FinOrRstObserved,
    NotReverseExactTupleAck,
    WrongRpcMethod,
    WrongRpcCallId,
    RpcReturnNotSuccessful,
    RpcReturnNotAuthoritative,
    RpcReturnNotEmpty,
    AmbiguousRpcReturn,
    MarkerAddNotAuthoritative,
    WrongMarkerMethod,
    WrongMarkerNumber,
    WrongMarkerOwner,
    MarkerPositionMismatch,
    MarkerInstanceNotNew,
    AmbiguousMarkerAdd,
    AlreadyTerminal,
}

/// Retrospective three-signal proof for one already-performed rewrite.
pub struct SingleMarkerRewriteConfirmation {
    config: AutomarkerConfirmationConfig,
    baseline: AutomarkerConfirmationBaseline,
    rewrite: AutomarkerConfirmationRewrite,
    state: AutomarkerConfirmationState,
    last_stamp: AutomarkerConfirmationObservationStamp,
    last_runtime_revision: u64,
    last_context_observed_micros: u64,
    reverse_ack_observed: bool,
    rpc_return_observed: bool,
    marker_add_instance: Option<i64>,
}

impl SingleMarkerRewriteConfirmation {
    pub fn begin(
        config: AutomarkerConfirmationConfig,
        baseline: AutomarkerConfirmationBaseline,
        rewrite: AutomarkerConfirmationRewrite,
    ) -> Result<Self, AutomarkerConfirmationError> {
        if !(1..=6).contains(&config.marker_number) {
            return Err(AutomarkerConfirmationError::InvalidMarkerNumber);
        }
        if !position_is_valid(config.target_position) {
            return Err(AutomarkerConfirmationError::InvalidTargetPosition);
        }
        if config.expected_game_build.trim().is_empty()
            || config.expected_scene_family.trim().is_empty()
            || baseline.context.game_build != config.expected_game_build
            || baseline.context.scene_family != config.expected_scene_family
            || baseline.context.local_actor_id == 0
            || baseline.context.connection_epoch == 0
            || baseline.context.runtime_revision == 0
            || baseline.context.observed_micros == 0
            || baseline.observation_ordinal == 0
            || !baseline.context.client_to_server_tuple.is_valid()
        {
            return Err(AutomarkerConfirmationError::InvalidBaseline);
        }
        if baseline.same_number_passive_instance_identities.len() > 1
            || baseline
                .same_number_passive_instance_identities
                .contains(&0)
        {
            return Err(AutomarkerConfirmationError::AmbiguousBaselineMarker);
        }
        if rewrite.method_id != AUTOMARKER_OUTBOUND_CARRIER_METHOD_ID {
            return Err(AutomarkerConfirmationError::WrongRewriteMethod);
        }
        if rewrite.original_rpc_call_id == 0
            || rewrite.mapped_tcp_length == 0
            || rewrite.mapped_tcp_length >= (1_u32 << 31)
            || rewrite.runtime_revision < baseline.context.runtime_revision
            || rewrite.observed_micros <= baseline.context.observed_micros
            || rewrite.observation_ordinal <= baseline.observation_ordinal
        {
            return Err(AutomarkerConfirmationError::InvalidRewrite);
        }
        let last_stamp = AutomarkerConfirmationObservationStamp {
            observation_ordinal: rewrite.observation_ordinal,
            observed_micros: rewrite.observed_micros,
            elapsed_since_rewrite_millis: 0,
        };
        Ok(Self {
            config,
            baseline,
            rewrite,
            state: AutomarkerConfirmationState::AwaitingEvidence {
                reverse_cumulative_ack: false,
                successful_empty_rpc_return: false,
                new_authoritative_marker_add: false,
            },
            last_stamp,
            last_runtime_revision: rewrite.runtime_revision,
            last_context_observed_micros: rewrite.observed_micros,
            reverse_ack_observed: false,
            rpc_return_observed: false,
            marker_add_instance: None,
        })
    }

    pub fn state(&self) -> AutomarkerConfirmationState {
        self.state
    }

    /// A runtime guard is mandatory for every accepted observation. Assertions
    /// are continuity inputs only; this method has no send decision output.
    pub fn revalidate_context(
        &mut self,
        context: &AutomarkerConfirmationContext,
    ) -> Result<(), AutomarkerConfirmationError> {
        self.require_active()?;
        self.validate_context(context)
            .or_else(|reason| self.abort(reason))?;
        if context.runtime_revision < self.last_runtime_revision
            || context.observed_micros < self.last_context_observed_micros
        {
            return self.abort(AutomarkerConfirmationError::StaleOrReorderedObservation);
        }
        Ok(())
    }

    pub fn observe_tcp(
        &mut self,
        context: &AutomarkerConfirmationContext,
        observation: AutomarkerConfirmationTcpObservation,
    ) -> Result<(), AutomarkerConfirmationError> {
        self.prepare_observation(context, observation.stamp)?;
        if observation.fin || observation.rst {
            return self.abort(AutomarkerConfirmationError::FinOrRstObserved);
        }
        if observation.connection_epoch != self.baseline.context.connection_epoch {
            return self.abort(AutomarkerConfirmationError::ConnectionEpochChanged);
        }
        let expected = self.baseline.context.client_to_server_tuple;
        if observation.source_address != expected.server_address
            || observation.source_port != expected.server_port
            || observation.destination_address != expected.client_address
            || observation.destination_port != expected.client_port
            || !observation.ack_flag
        {
            return self.abort(AutomarkerConfirmationError::NotReverseExactTupleAck);
        }
        let mapped_end = self
            .rewrite
            .mapped_tcp_sequence_start
            .wrapping_add(self.rewrite.mapped_tcp_length);
        if serial_at_or_after(observation.cumulative_ack, mapped_end) {
            self.reverse_ack_observed = true;
        }
        self.refresh_state();
        Ok(())
    }

    pub fn observe_rpc_return(
        &mut self,
        context: &AutomarkerConfirmationContext,
        observation: AutomarkerConfirmationRpcReturn,
    ) -> Result<(), AutomarkerConfirmationError> {
        self.prepare_observation(context, observation.stamp)?;
        if observation.method_id != AUTOMARKER_OUTBOUND_CARRIER_METHOD_ID {
            return self.abort(AutomarkerConfirmationError::WrongRpcMethod);
        }
        if observation.original_call_id != self.rewrite.original_rpc_call_id {
            return self.abort(AutomarkerConfirmationError::WrongRpcCallId);
        }
        if self.rpc_return_observed {
            return self.abort(AutomarkerConfirmationError::AmbiguousRpcReturn);
        }
        if !observation.asserted_decoded_from_authoritative_server_stream {
            return self.abort(AutomarkerConfirmationError::RpcReturnNotAuthoritative);
        }
        if !observation.decoded_as_success {
            return self.abort(AutomarkerConfirmationError::RpcReturnNotSuccessful);
        }
        if observation.decoded_body_length != 0 {
            return self.abort(AutomarkerConfirmationError::RpcReturnNotEmpty);
        }
        self.rpc_return_observed = true;
        self.refresh_state();
        Ok(())
    }

    pub fn observe_authoritative_marker_add(
        &mut self,
        context: &AutomarkerConfirmationContext,
        observation: AutomarkerConfirmationMarkerAdd,
    ) -> Result<(), AutomarkerConfirmationError> {
        self.prepare_observation(context, observation.stamp)?;
        if !observation.asserted_decoded_from_authoritative_server_stream {
            return self.abort(AutomarkerConfirmationError::MarkerAddNotAuthoritative);
        }
        if observation.method_id != AUTOMARKER_AUTHORITATIVE_MARKER_ADD_METHOD_ID {
            return self.abort(AutomarkerConfirmationError::WrongMarkerMethod);
        }
        if observation.marker_number != self.config.marker_number {
            return self.abort(AutomarkerConfirmationError::WrongMarkerNumber);
        }
        if observation.marker_owner_actor_id != self.baseline.context.local_actor_id {
            return self.abort(AutomarkerConfirmationError::WrongMarkerOwner);
        }
        if !same_position_bits(observation.position, self.config.target_position) {
            return self.abort(AutomarkerConfirmationError::MarkerPositionMismatch);
        }
        if observation.runtime_revision <= self.rewrite.runtime_revision
            || observation.observed_micros <= self.rewrite.observed_micros
            || observation.runtime_revision != context.runtime_revision
            || observation.observed_micros != context.observed_micros
            || observation.observed_micros != observation.stamp.observed_micros
        {
            return self.abort(AutomarkerConfirmationError::ObservationNotPostRewrite);
        }
        if observation.passive_instance_identity == 0
            || self
                .baseline
                .same_number_passive_instance_identities
                .contains(&observation.passive_instance_identity)
        {
            return self.abort(AutomarkerConfirmationError::MarkerInstanceNotNew);
        }
        if let Some(previous) = self.marker_add_instance {
            if previous != observation.passive_instance_identity {
                return self.abort(AutomarkerConfirmationError::AmbiguousMarkerAdd);
            }
            return Ok(());
        }
        self.marker_add_instance = Some(observation.passive_instance_identity);
        self.refresh_state();
        Ok(())
    }

    pub fn observe_timeout(
        &mut self,
        elapsed_since_rewrite_millis: u64,
        current_observed_micros: u64,
    ) -> Result<(), AutomarkerConfirmationError> {
        self.require_active()?;
        let Some(observed_delta_micros) =
            current_observed_micros.checked_sub(self.rewrite.observed_micros)
        else {
            return self.abort(AutomarkerConfirmationError::ObservationNotPostRewrite);
        };
        if elapsed_since_rewrite_millis >= AUTOMARKER_CONFIRMATION_TIMEOUT_MILLIS
            || observed_delta_micros >= AUTOMARKER_CONFIRMATION_TIMEOUT_MICROS
        {
            return self.abort(AutomarkerConfirmationError::TimedOut);
        }
        if elapsed_since_rewrite_millis < observed_delta_micros / 1_000 {
            return self.abort(AutomarkerConfirmationError::InconsistentObservationAge);
        }
        Ok(())
    }

    fn prepare_observation(
        &mut self,
        context: &AutomarkerConfirmationContext,
        stamp: AutomarkerConfirmationObservationStamp,
    ) -> Result<(), AutomarkerConfirmationError> {
        self.require_active()?;
        self.validate_context(context)
            .or_else(|reason| self.abort(reason))?;
        if stamp.observation_ordinal <= self.rewrite.observation_ordinal
            || stamp.observed_micros <= self.rewrite.observed_micros
        {
            return self.abort(AutomarkerConfirmationError::ObservationNotPostRewrite);
        }
        let observed_delta_micros = stamp.observed_micros - self.rewrite.observed_micros;
        if stamp.elapsed_since_rewrite_millis >= AUTOMARKER_CONFIRMATION_TIMEOUT_MILLIS
            || observed_delta_micros >= AUTOMARKER_CONFIRMATION_TIMEOUT_MICROS
        {
            return self.abort(AutomarkerConfirmationError::TimedOut);
        }
        if stamp.elapsed_since_rewrite_millis < observed_delta_micros / 1_000 {
            return self.abort(AutomarkerConfirmationError::InconsistentObservationAge);
        }
        if stamp.observation_ordinal <= self.last_stamp.observation_ordinal
            || stamp.observed_micros <= self.last_stamp.observed_micros
            || stamp.elapsed_since_rewrite_millis < self.last_stamp.elapsed_since_rewrite_millis
            || context.runtime_revision < self.last_runtime_revision
            || context.observed_micros < self.last_context_observed_micros
        {
            return self.abort(AutomarkerConfirmationError::StaleOrReorderedObservation);
        }
        self.last_stamp = stamp;
        self.last_runtime_revision = context.runtime_revision;
        self.last_context_observed_micros = context.observed_micros;
        Ok(())
    }

    fn validate_context(
        &self,
        context: &AutomarkerConfirmationContext,
    ) -> Result<(), AutomarkerConfirmationError> {
        let baseline = &self.baseline.context;
        if context.game_build != baseline.game_build {
            return Err(AutomarkerConfirmationError::BuildChanged);
        }
        if context.scene_family != baseline.scene_family {
            return Err(AutomarkerConfirmationError::SceneChanged);
        }
        if context.local_actor_id != baseline.local_actor_id {
            return Err(AutomarkerConfirmationError::LocalActorChanged);
        }
        if context.client_to_server_tuple != baseline.client_to_server_tuple {
            return Err(AutomarkerConfirmationError::SocketChanged);
        }
        if context.connection_epoch != baseline.connection_epoch {
            return Err(AutomarkerConfirmationError::ConnectionEpochChanged);
        }
        if context.runtime_revision < self.rewrite.runtime_revision
            || context.observed_micros < self.rewrite.observed_micros
        {
            return Err(AutomarkerConfirmationError::RuntimeRevisionRegressed);
        }
        Ok(())
    }

    fn require_active(&self) -> Result<(), AutomarkerConfirmationError> {
        match self.state {
            AutomarkerConfirmationState::AwaitingEvidence { .. } => Ok(()),
            _ => Err(AutomarkerConfirmationError::AlreadyTerminal),
        }
    }

    fn refresh_state(&mut self) {
        let marker = self.marker_add_instance.is_some();
        self.state = if self.reverse_ack_observed && self.rpc_return_observed && marker {
            AutomarkerConfirmationState::Confirmed
        } else {
            AutomarkerConfirmationState::AwaitingEvidence {
                reverse_cumulative_ack: self.reverse_ack_observed,
                successful_empty_rpc_return: self.rpc_return_observed,
                new_authoritative_marker_add: marker,
            }
        };
    }

    fn abort<T>(
        &mut self,
        reason: AutomarkerConfirmationError,
    ) -> Result<T, AutomarkerConfirmationError> {
        self.state = AutomarkerConfirmationState::Aborted(reason);
        Err(reason)
    }
}

fn serial_at_or_after(value: u32, target: u32) -> bool {
    value.wrapping_sub(target) < (1_u32 << 31)
}

fn position_is_valid(position: AutomarkerRequestXyz) -> bool {
    [position.x, position.y, position.z]
        .into_iter()
        .all(|value| value.is_finite() && value.abs() <= 1_000_000.0)
}

fn same_position_bits(left: AutomarkerRequestXyz, right: AutomarkerRequestXyz) -> bool {
    left.x.to_bits() == right.x.to_bits()
        && left.y.to_bits() == right.y.to_bits()
        && left.z.to_bits() == right.z.to_bits()
}

#[cfg(test)]
mod tests {
    use super::*;

    type MarkerMutation = Box<dyn Fn(&mut AutomarkerConfirmationMarkerAdd)>;
    type MarkerMutationCase = (MarkerMutation, AutomarkerConfirmationError);
    type ContextMutation = Box<dyn Fn(&mut AutomarkerConfirmationContext)>;
    type ContextMutationCase = (ContextMutation, AutomarkerConfirmationError);

    fn tuple() -> AutomarkerConfirmationTcpTuple {
        AutomarkerConfirmationTcpTuple {
            client_address: [10, 0, 0, 2],
            client_port: 49_000,
            server_address: [20, 0, 0, 3],
            server_port: 443,
        }
    }

    fn context(revision: u64, micros: u64) -> AutomarkerConfirmationContext {
        AutomarkerConfirmationContext {
            game_build: "25247556".into(),
            scene_family: "mech-facility".into(),
            local_actor_id: 88,
            connection_epoch: 9,
            client_to_server_tuple: tuple(),
            runtime_revision: revision,
            observed_micros: micros,
        }
    }

    fn config() -> AutomarkerConfirmationConfig {
        AutomarkerConfirmationConfig {
            expected_game_build: "25247556".into(),
            expected_scene_family: "mech-facility".into(),
            marker_number: 1,
            target_position: AutomarkerRequestXyz {
                x: 42.5,
                y: -7.25,
                z: 91.0,
            },
        }
    }

    fn contract() -> SingleMarkerRewriteConfirmation {
        SingleMarkerRewriteConfirmation::begin(
            config(),
            AutomarkerConfirmationBaseline {
                context: context(100, 1_000),
                observation_ordinal: 9,
                same_number_passive_instance_identities: vec![700],
            },
            AutomarkerConfirmationRewrite {
                method_id: AUTOMARKER_OUTBOUND_CARRIER_METHOD_ID,
                original_rpc_call_id: 55,
                mapped_tcp_sequence_start: 1_000,
                mapped_tcp_length: 16,
                runtime_revision: 101,
                observed_micros: 1_100,
                observation_ordinal: 10,
            },
        )
        .unwrap()
    }

    fn stamp(ordinal: u64) -> AutomarkerConfirmationObservationStamp {
        AutomarkerConfirmationObservationStamp {
            observation_ordinal: ordinal,
            observed_micros: 1_100 + ordinal,
            elapsed_since_rewrite_millis: ordinal,
        }
    }

    fn tcp(ordinal: u64, ack: u32) -> AutomarkerConfirmationTcpObservation {
        AutomarkerConfirmationTcpObservation {
            connection_epoch: 9,
            source_address: [20, 0, 0, 3],
            source_port: 443,
            destination_address: [10, 0, 0, 2],
            destination_port: 49_000,
            ack_flag: true,
            cumulative_ack: ack,
            fin: false,
            rst: false,
            stamp: stamp(ordinal),
        }
    }

    fn rpc(ordinal: u64) -> AutomarkerConfirmationRpcReturn {
        AutomarkerConfirmationRpcReturn {
            method_id: AUTOMARKER_OUTBOUND_CARRIER_METHOD_ID,
            original_call_id: 55,
            asserted_decoded_from_authoritative_server_stream: true,
            decoded_as_success: true,
            decoded_body_length: 0,
            stamp: stamp(ordinal),
        }
    }

    fn add(ordinal: u64) -> AutomarkerConfirmationMarkerAdd {
        AutomarkerConfirmationMarkerAdd {
            method_id: 46,
            marker_number: 1,
            marker_owner_actor_id: 88,
            position: config().target_position,
            passive_instance_identity: 701,
            asserted_decoded_from_authoritative_server_stream: true,
            runtime_revision: 102,
            observed_micros: 1_100 + ordinal,
            stamp: stamp(ordinal),
        }
    }

    #[test]
    fn confirms_only_after_all_three_strictly_post_rewrite_signals() {
        let mut proof = contract();
        proof
            .observe_tcp(&context(101, 1_111), tcp(11, 1_016))
            .unwrap();
        proof
            .observe_rpc_return(&context(101, 1_112), rpc(12))
            .unwrap();
        assert!(matches!(
            proof.state(),
            AutomarkerConfirmationState::AwaitingEvidence { .. }
        ));
        proof
            .observe_authoritative_marker_add(&context(102, 1_113), add(13))
            .unwrap();
        assert_eq!(proof.state(), AutomarkerConfirmationState::Confirmed);
    }

    #[test]
    fn independent_signals_may_arrive_in_a_different_valid_order() {
        let mut proof = contract();
        proof
            .observe_authoritative_marker_add(&context(102, 1_111), add(11))
            .unwrap();
        proof
            .observe_rpc_return(&context(102, 1_112), rpc(12))
            .unwrap();
        proof
            .observe_tcp(&context(102, 1_113), tcp(13, 1_016))
            .unwrap();
        assert_eq!(proof.state(), AutomarkerConfirmationState::Confirmed);
    }

    #[test]
    fn cumulative_ack_must_cover_the_entire_mapped_range() {
        let mut proof = contract();
        proof
            .observe_tcp(&context(101, 1_111), tcp(11, 1_015))
            .unwrap();
        assert_eq!(
            proof.state(),
            AutomarkerConfirmationState::AwaitingEvidence {
                reverse_cumulative_ack: false,
                successful_empty_rpc_return: false,
                new_authoritative_marker_add: false,
            }
        );
        proof
            .observe_tcp(&context(101, 1_112), tcp(12, 1_016))
            .unwrap();
        assert!(matches!(
            proof.state(),
            AutomarkerConfirmationState::AwaitingEvidence {
                reverse_cumulative_ack: true,
                ..
            }
        ));
    }

    #[test]
    fn cumulative_ack_handles_sequence_wrap() {
        let mut proof = SingleMarkerRewriteConfirmation::begin(
            config(),
            AutomarkerConfirmationBaseline {
                context: context(100, 1_000),
                observation_ordinal: 9,
                same_number_passive_instance_identities: vec![],
            },
            AutomarkerConfirmationRewrite {
                method_id: AUTOMARKER_OUTBOUND_CARRIER_METHOD_ID,
                original_rpc_call_id: 55,
                mapped_tcp_sequence_start: u32::MAX - 7,
                mapped_tcp_length: 16,
                runtime_revision: 101,
                observed_micros: 1_100,
                observation_ordinal: 10,
            },
        )
        .unwrap();
        proof.observe_tcp(&context(101, 1_111), tcp(11, 8)).unwrap();
        assert!(matches!(
            proof.state(),
            AutomarkerConfirmationState::AwaitingEvidence {
                reverse_cumulative_ack: true,
                ..
            }
        ));
    }

    #[test]
    fn wrong_tuple_and_epoch_fail_closed() {
        let mut proof = contract();
        let mut observation = tcp(11, 1_016);
        observation.source_port = 444;
        assert_eq!(
            proof.observe_tcp(&context(101, 1_111), observation),
            Err(AutomarkerConfirmationError::NotReverseExactTupleAck)
        );
        let mut proof = contract();
        let mut observation = tcp(11, 1_016);
        observation.connection_epoch = 10;
        assert_eq!(
            proof.observe_tcp(&context(101, 1_111), observation),
            Err(AutomarkerConfirmationError::ConnectionEpochChanged)
        );
    }

    #[test]
    fn fin_or_rst_aborts_before_ack_credit() {
        for rst in [false, true] {
            let mut proof = contract();
            let mut observation = tcp(11, 1_016);
            observation.fin = !rst;
            observation.rst = rst;
            assert_eq!(
                proof.observe_tcp(&context(101, 1_111), observation),
                Err(AutomarkerConfirmationError::FinOrRstObserved)
            );
        }
    }

    #[test]
    fn rpc_return_requires_exact_call_success_and_empty_body() {
        let cases = [
            (
                AUTOMARKER_OUTBOUND_CARRIER_METHOD_ID - 1,
                55,
                true,
                true,
                0,
                AutomarkerConfirmationError::WrongRpcMethod,
            ),
            (
                AUTOMARKER_OUTBOUND_CARRIER_METHOD_ID,
                54,
                true,
                true,
                0,
                AutomarkerConfirmationError::WrongRpcCallId,
            ),
            (
                AUTOMARKER_OUTBOUND_CARRIER_METHOD_ID,
                55,
                true,
                false,
                0,
                AutomarkerConfirmationError::RpcReturnNotSuccessful,
            ),
            (
                AUTOMARKER_OUTBOUND_CARRIER_METHOD_ID,
                55,
                false,
                true,
                0,
                AutomarkerConfirmationError::RpcReturnNotAuthoritative,
            ),
            (
                AUTOMARKER_OUTBOUND_CARRIER_METHOD_ID,
                55,
                true,
                true,
                1,
                AutomarkerConfirmationError::RpcReturnNotEmpty,
            ),
        ];
        for (method, call, authoritative, success, length, expected) in cases {
            let mut proof = contract();
            let mut observation = rpc(11);
            observation.method_id = method;
            observation.original_call_id = call;
            observation.asserted_decoded_from_authoritative_server_stream = authoritative;
            observation.decoded_as_success = success;
            observation.decoded_body_length = length;
            assert_eq!(
                proof.observe_rpc_return(&context(101, 1_111), observation),
                Err(expected)
            );
        }
    }

    #[test]
    fn duplicate_rpc_candidate_is_ambiguous() {
        let mut proof = contract();
        proof
            .observe_rpc_return(&context(101, 1_111), rpc(11))
            .unwrap();
        assert_eq!(
            proof.observe_rpc_return(&context(101, 1_112), rpc(12)),
            Err(AutomarkerConfirmationError::AmbiguousRpcReturn)
        );
    }

    #[test]
    fn marker_requires_authoritative_method_number_bit_exact_xyz_and_new_identity() {
        let mutations: Vec<MarkerMutationCase> = vec![
            (
                Box::new(|value| value.asserted_decoded_from_authoritative_server_stream = false),
                AutomarkerConfirmationError::MarkerAddNotAuthoritative,
            ),
            (
                Box::new(|value| value.method_id = 45),
                AutomarkerConfirmationError::WrongMarkerMethod,
            ),
            (
                Box::new(|value| value.marker_number = 2),
                AutomarkerConfirmationError::WrongMarkerNumber,
            ),
            (
                Box::new(|value| value.marker_owner_actor_id = 89),
                AutomarkerConfirmationError::WrongMarkerOwner,
            ),
            (
                Box::new(|value| value.position.x = f32::from_bits(value.position.x.to_bits() + 1)),
                AutomarkerConfirmationError::MarkerPositionMismatch,
            ),
            (
                Box::new(|value| value.passive_instance_identity = 700),
                AutomarkerConfirmationError::MarkerInstanceNotNew,
            ),
        ];
        for (mutate, expected) in mutations {
            let mut proof = contract();
            let mut observation = add(11);
            mutate(&mut observation);
            assert_eq!(
                proof.observe_authoritative_marker_add(&context(102, 1_111), observation),
                Err(expected)
            );
        }
    }

    #[test]
    fn marker_requires_strictly_new_runtime_revision_and_micros() {
        let mut proof = contract();
        let mut observation = add(11);
        observation.runtime_revision = 101;
        assert_eq!(
            proof.observe_authoritative_marker_add(&context(101, 1_111), observation),
            Err(AutomarkerConfirmationError::ObservationNotPostRewrite)
        );
        let mut proof = contract();
        let mut observation = add(11);
        observation.observed_micros = 1_100;
        observation.stamp.observed_micros = 1_100;
        assert_eq!(
            proof.observe_authoritative_marker_add(&context(102, 1_100), observation),
            Err(AutomarkerConfirmationError::ObservationNotPostRewrite)
        );
    }

    #[test]
    fn a_second_distinct_marker_instance_is_ambiguous() {
        let mut proof = contract();
        proof
            .observe_authoritative_marker_add(&context(102, 1_111), add(11))
            .unwrap();
        let mut competing = add(12);
        competing.passive_instance_identity = 702;
        competing.runtime_revision = 103;
        assert_eq!(
            proof.observe_authoritative_marker_add(&context(103, 1_112), competing),
            Err(AutomarkerConfirmationError::AmbiguousMarkerAdd)
        );
    }

    #[test]
    fn context_changes_abort_independently() {
        let mutations: Vec<ContextMutationCase> = vec![
            (
                Box::new(|value| value.game_build = "next-season".into()),
                AutomarkerConfirmationError::BuildChanged,
            ),
            (
                Box::new(|value| value.scene_family = "sea-ringed-reef".into()),
                AutomarkerConfirmationError::SceneChanged,
            ),
            (
                Box::new(|value| value.local_actor_id = 89),
                AutomarkerConfirmationError::LocalActorChanged,
            ),
            (
                Box::new(|value| value.connection_epoch = 10),
                AutomarkerConfirmationError::ConnectionEpochChanged,
            ),
            (
                Box::new(|value| value.client_to_server_tuple.client_port = 49_001),
                AutomarkerConfirmationError::SocketChanged,
            ),
        ];
        for (mutate, expected) in mutations {
            let mut proof = contract();
            let mut changed = context(101, 1_111);
            mutate(&mut changed);
            assert_eq!(proof.revalidate_context(&changed), Err(expected));
            assert_eq!(
                proof.state(),
                AutomarkerConfirmationState::Aborted(expected)
            );
        }
    }

    #[test]
    fn stale_reordered_and_timed_out_observations_abort() {
        let mut proof = contract();
        let mut observation = tcp(10, 1_016);
        observation.stamp.observed_micros = 1_101;
        assert_eq!(
            proof.observe_tcp(&context(101, 1_101), observation),
            Err(AutomarkerConfirmationError::ObservationNotPostRewrite)
        );

        let mut proof = contract();
        proof
            .observe_tcp(&context(101, 1_112), tcp(12, 1_015))
            .unwrap();
        assert_eq!(
            proof.observe_rpc_return(&context(101, 1_111), rpc(11)),
            Err(AutomarkerConfirmationError::StaleOrReorderedObservation)
        );

        let mut proof = contract();
        assert_eq!(
            proof.observe_timeout(
                AUTOMARKER_CONFIRMATION_TIMEOUT_MILLIS,
                1_100 + AUTOMARKER_CONFIRMATION_TIMEOUT_MICROS
            ),
            Err(AutomarkerConfirmationError::TimedOut)
        );

        let mut proof = contract();
        proof
            .observe_tcp(&context(103, 1_111), tcp(11, 1_015))
            .unwrap();
        assert_eq!(
            proof.observe_rpc_return(&context(102, 1_112), rpc(12)),
            Err(AutomarkerConfirmationError::StaleOrReorderedObservation)
        );
    }

    #[test]
    fn monotonic_micros_independently_enforce_timeout_and_elapsed_consistency() {
        let mut proof = contract();
        let mut observation = tcp(11, 1_016);
        observation.stamp.observed_micros = 1_100 + AUTOMARKER_CONFIRMATION_TIMEOUT_MICROS;
        observation.stamp.elapsed_since_rewrite_millis = 1;
        assert_eq!(
            proof.observe_tcp(
                &context(101, observation.stamp.observed_micros),
                observation
            ),
            Err(AutomarkerConfirmationError::TimedOut)
        );

        let mut proof = contract();
        let mut observation = tcp(11, 1_016);
        observation.stamp.observed_micros = 1_100 + 1_500_000;
        observation.stamp.elapsed_since_rewrite_millis = 0;
        assert_eq!(
            proof.observe_tcp(
                &context(101, observation.stamp.observed_micros),
                observation
            ),
            Err(AutomarkerConfirmationError::InconsistentObservationAge)
        );

        let mut proof = contract();
        assert_eq!(
            proof.observe_timeout(0, 1_099),
            Err(AutomarkerConfirmationError::ObservationNotPostRewrite)
        );
    }

    #[test]
    fn ambiguous_baseline_and_invalid_rewrite_are_rejected() {
        let baseline = AutomarkerConfirmationBaseline {
            context: context(100, 1_000),
            observation_ordinal: 9,
            same_number_passive_instance_identities: vec![700, 701],
        };
        let rewrite = AutomarkerConfirmationRewrite {
            method_id: AUTOMARKER_OUTBOUND_CARRIER_METHOD_ID,
            original_rpc_call_id: 55,
            mapped_tcp_sequence_start: 1_000,
            mapped_tcp_length: 16,
            runtime_revision: 101,
            observed_micros: 1_100,
            observation_ordinal: 10,
        };
        assert!(matches!(
            SingleMarkerRewriteConfirmation::begin(config(), baseline, rewrite),
            Err(AutomarkerConfirmationError::AmbiguousBaselineMarker)
        ));

        let mut rewrite = rewrite;
        rewrite.mapped_tcp_length = 0;
        assert!(matches!(
            SingleMarkerRewriteConfirmation::begin(
                config(),
                AutomarkerConfirmationBaseline {
                    context: context(100, 1_000),
                    observation_ordinal: 9,
                    same_number_passive_instance_identities: vec![]
                },
                rewrite
            ),
            Err(AutomarkerConfirmationError::InvalidRewrite)
        ));

        rewrite.mapped_tcp_length = 16;
        rewrite.method_id = AUTOMARKER_AUTHORITATIVE_MARKER_ADD_METHOD_ID;
        assert!(matches!(
            SingleMarkerRewriteConfirmation::begin(
                config(),
                AutomarkerConfirmationBaseline {
                    context: context(100, 1_000),
                    observation_ordinal: 9,
                    same_number_passive_instance_identities: vec![]
                },
                rewrite
            ),
            Err(AutomarkerConfirmationError::WrongRewriteMethod)
        ));
    }

    #[test]
    fn terminal_contract_cannot_accept_more_evidence() {
        let mut proof = contract();
        proof
            .observe_tcp(&context(101, 1_111), tcp(11, 1_016))
            .unwrap();
        proof
            .observe_rpc_return(&context(101, 1_112), rpc(12))
            .unwrap();
        proof
            .observe_authoritative_marker_add(&context(102, 1_113), add(13))
            .unwrap();
        assert_eq!(
            proof.revalidate_context(&context(102, 1_114)),
            Err(AutomarkerConfirmationError::AlreadyTerminal)
        );
    }
}
