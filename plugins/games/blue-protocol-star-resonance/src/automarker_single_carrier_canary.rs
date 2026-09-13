//! Fail-closed activation contract for one live XYZ-only marker substitution.
//!
//! This module owns no driver, socket, process, UI, memory, or packet-send
//! capability. A separate research executable may connect it to the reviewed
//! WinDivert boundary only after constructing the opaque SYN-scoped process
//! binding. The canary deliberately consumes one complete game-generated
//! same-number carrier and can never synthesize or duplicate a request.

use crate::{
    AUTOMARKER_REQUEST_BUILD, AUTOMARKER_REQUEST_PACK_DIGEST, AutomarkerRequestXyz,
    OfflineAutomarkerConnectionEpochBinding, OfflineAutomarkerTcpAckResult,
    OfflineAutomarkerTcpArmResult, OfflineAutomarkerTcpRewriteLedger,
    OfflineAutomarkerTcpSegmentReason, OfflineAutomarkerTcpSegmentResult, ProtocolPack,
    decode_observed_automarker_request_into, supports_observed_automarker_requests,
};

pub const SINGLE_MARKER_XYZ_CANARY_ARM_TOKEN: &str = "RLOGS_AUTOMARKER_SINGLE_FRESH_CARRIER_XYZ_V1";
pub const SINGLE_MARKER_XYZ_MAX_CARRIER_AGE_MILLIS: u64 = 500;
pub const SINGLE_MARKER_XYZ_MAX_CONTEXT_AGE_MILLIS: u64 = 500;

const EXACT_FRAME_BYTES: usize = 197;
const APPLICATION_OFFSET: usize = 36;

#[derive(Debug, Clone, PartialEq)]
pub struct SingleMarkerXyzCanaryConfig {
    pub expected_scene_family: String,
    pub marker_number: u8,
    pub target_position: AutomarkerRequestXyz,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SingleMarkerXyzCanaryContext<'a> {
    pub game_build: &'a str,
    pub current_scene_family: &'a str,
    pub runtime_revision: u64,
    /// Monotonic bridge clock sampled with `runtime_revision`. This is pinned
    /// only when the first complete modified packet send is committed.
    pub observation_monotonic_millis: u64,
    pub observation_age_millis: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SingleMarkerXyzCanaryState {
    DryRun,
    AwaitingFreshCarrier,
    AwaitingRewrite,
    AwaitingExternalSend,
    AwaitingAcknowledgement {
        transport_ack_observed: bool,
        successful_rpc_return_observed: bool,
        authoritative_self_add_observed: bool,
    },
    Succeeded,
    Aborted(SingleMarkerXyzCanaryError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SingleMarkerXyzCanaryError {
    InvalidConsent,
    UnsupportedBuildOrPack,
    RuntimeBuildMismatch,
    StaleRuntimeContext,
    MissingSceneFamily,
    SceneFamilyMismatch,
    InvalidMarkerNumber,
    InvalidTargetCoordinate,
    CarrierTooOld,
    CarrierFrameNotExact,
    CarrierDecodeRejected,
    WrongCarrierMarker,
    CarrierAlreadyConsumed,
    RewriteArmRejected,
    RewriteRejected,
    RewriteDidNotChangeApprovedBytes,
    PreparedRewriteMismatch,
    PreSendPreparationFailed,
    IndeterminateModifiedSend,
    ConfirmationBeforeRewrite,
    WrongRpcCallId,
    NegativeRpcReturn,
    WrongAuthoritativeMarker,
    AuthoritativePositionMismatch,
    StaleConfirmation,
    AuthoritativeAddNotNew,
    ConnectionEpochChanged,
    SceneChanged,
    TimedOut,
    ConnectionTerminated,
}

/// An explicit send/no-send decision for the live bridge. Once any changed
/// byte has been sent, an ambiguous overlap may never degrade to sending the
/// original bytes: that would create two conflicting versions of one TCP
/// sequence range. The bridge must close the divert handle and force the game
/// connection to reconnect on `AbortWithoutReinject`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SingleMarkerXyzSegmentDisposition {
    SendOriginal(Vec<u8>),
    /// This is only a rewritten TCP payload. It is not authorized for direct
    /// transmission. The Windows bridge must install it into the held packet,
    /// run the pinned `WinDivertHelperCalcChecksums` on that packet and its
    /// mutable 80-byte address, verify the helper/flags, then send the packet.
    PreparedRewriteNeedsPacketChecksumRepair(SingleMarkerXyzPreparedRewrite),
    AbortWithoutReinject(SingleMarkerXyzCanaryError),
}

/// Owned two-phase handoff to the Windows bridge. The payload may be copied
/// into the held packet and checksum-repaired, but the canary remains
/// uncommitted until `commit_prepared_rewrite` receives proof that the entire
/// packet was sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SingleMarkerXyzPreparedRewrite {
    pub preparation_id: u64,
    pub original_payload: Vec<u8>,
    pub rewritten_payload: Vec<u8>,
    pub expected_packet_send_len: usize,
    pub changed_bytes: usize,
    pub first_modified_emission: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SingleMarkerXyzExternalSendOutcome {
    /// WinDivert reported success and supplied this exact sent byte count.
    Complete { bytes_sent: usize },
    /// A false return has ambiguous delivery semantics for this boundary.
    Failed,
    /// A short successful return is also indeterminate.
    Short { bytes_sent: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SingleMarkerXyzCommitDisposition {
    Committed,
    AbortWithoutReinject(SingleMarkerXyzCanaryError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SingleMarkerXyzCommittedRewriteStamp {
    pub runtime_revision: u64,
    pub monotonic_millis: u64,
}

#[derive(Debug, Clone)]
struct PendingRewrite {
    preparation: SingleMarkerXyzPreparedRewrite,
    stamp: SingleMarkerXyzCommittedRewriteStamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SingleMarkerXyzCarrierIdentity {
    pub frame_up_sequence: u32,
    pub rpc_call_id: u32,
    pub session_sequence: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SingleMarkerXyzInterceptedSegment<'a> {
    pub connection_epoch: u64,
    pub tcp_sequence_start: u32,
    pub tcp_syn: bool,
    pub frame_payload_offset: usize,
    /// Monotonic age measured by the intercepting bridge when it still owns
    /// the held packet. This must never be replaced by a constant zero.
    pub observed_age_millis: u64,
    /// Full held NETWORK-layer packet size expected from the external send.
    pub held_packet_len: usize,
    pub payload: &'a [u8],
}

/// One-shot state machine shared by tests and the future research-only live
/// bridge. The opaque binding can only be produced by exact PID + tuple + SYN
/// epoch validation in `bind_offline_automarker_connection_epoch`.
pub struct SingleMarkerXyzCanary {
    config: SingleMarkerXyzCanaryConfig,
    binding: Option<OfflineAutomarkerConnectionEpochBinding>,
    ledger: Option<OfflineAutomarkerTcpRewriteLedger>,
    state: SingleMarkerXyzCanaryState,
    carrier: Option<SingleMarkerXyzCarrierIdentity>,
    rewrite_obligation_active: bool,
    modified_send_committed: bool,
    transport_ack_observed: bool,
    rpc_return_observed: bool,
    authoritative_add_observed: bool,
    committed_rewrite_stamp: Option<SingleMarkerXyzCommittedRewriteStamp>,
    pending_rewrite: Option<PendingRewrite>,
    next_preparation_id: u64,
}

impl SingleMarkerXyzCanary {
    /// The default constructor is incapable of arming or changing bytes.
    pub fn dry_run(config: SingleMarkerXyzCanaryConfig) -> Self {
        Self {
            config,
            binding: None,
            ledger: None,
            state: SingleMarkerXyzCanaryState::DryRun,
            carrier: None,
            rewrite_obligation_active: false,
            modified_send_committed: false,
            transport_ack_observed: false,
            rpc_return_observed: false,
            authoritative_add_observed: false,
            committed_rewrite_stamp: None,
            pending_rewrite: None,
            next_preparation_id: 1,
        }
    }

    pub fn arm(
        config: SingleMarkerXyzCanaryConfig,
        literal_consent: &str,
        pack: &ProtocolPack,
        binding: OfflineAutomarkerConnectionEpochBinding,
        context: SingleMarkerXyzCanaryContext<'_>,
    ) -> Result<Self, SingleMarkerXyzCanaryError> {
        if literal_consent != SINGLE_MARKER_XYZ_CANARY_ARM_TOKEN {
            return Err(SingleMarkerXyzCanaryError::InvalidConsent);
        }
        if pack.definition().target.build_id != AUTOMARKER_REQUEST_BUILD
            || pack.digest() != AUTOMARKER_REQUEST_PACK_DIGEST
            || !supports_observed_automarker_requests(pack)
        {
            return Err(SingleMarkerXyzCanaryError::UnsupportedBuildOrPack);
        }
        validate_context(&config, context)?;
        if !(1..=6).contains(&config.marker_number) {
            return Err(SingleMarkerXyzCanaryError::InvalidMarkerNumber);
        }
        if !coordinate_is_finite(config.target_position) {
            return Err(SingleMarkerXyzCanaryError::InvalidTargetCoordinate);
        }
        let epoch = binding.connection_epoch();
        Ok(Self {
            config,
            binding: Some(binding),
            ledger: Some(OfflineAutomarkerTcpRewriteLedger::new(epoch)),
            state: SingleMarkerXyzCanaryState::AwaitingFreshCarrier,
            carrier: None,
            rewrite_obligation_active: false,
            modified_send_committed: false,
            transport_ack_observed: false,
            rpc_return_observed: false,
            authoritative_add_observed: false,
            committed_rewrite_stamp: None,
            pending_rewrite: None,
            next_preparation_id: 1,
        })
    }

    pub fn state(&self) -> SingleMarkerXyzCanaryState {
        self.state
    }

    pub fn connection_epoch(&self) -> Option<u64> {
        self.binding.map(|binding| binding.connection_epoch())
    }

    /// Validates and arms one complete, uncompressed, same-number carrier.
    /// `frame_sequence_start` is the TCP sequence of byte zero of the 197-byte
    /// outer FrameUp, not merely the containing segment's sequence.
    pub fn observe_fresh_carrier(
        &mut self,
        pack: &ProtocolPack,
        connection_epoch: u64,
        frame_sequence_start: u32,
        carrier_age_millis: u64,
        frame: &[u8],
        context: SingleMarkerXyzCanaryContext<'_>,
    ) -> Result<SingleMarkerXyzCarrierIdentity, SingleMarkerXyzCanaryError> {
        self.require_state(SingleMarkerXyzCanaryState::AwaitingFreshCarrier)?;
        self.require_context_and_epoch(connection_epoch, context)?;
        if carrier_age_millis > SINGLE_MARKER_XYZ_MAX_CARRIER_AGE_MILLIS {
            return self.abort(SingleMarkerXyzCanaryError::CarrierTooOld);
        }
        if frame.len() != EXACT_FRAME_BYTES {
            return self.abort(SingleMarkerXyzCanaryError::CarrierFrameNotExact);
        }
        let Some(application) = frame.get(APPLICATION_OFFSET..) else {
            return self.abort(SingleMarkerXyzCanaryError::CarrierFrameNotExact);
        };
        let mut scratch = Vec::new();
        let request = decode_observed_automarker_request_into(pack, application, &mut scratch)
            .map_err(|_| SingleMarkerXyzCanaryError::CarrierDecodeRejected)
            .or_else(|reason| self.abort(reason))?;
        if request.marker_number != self.config.marker_number {
            return self.abort(SingleMarkerXyzCanaryError::WrongCarrierMarker);
        }
        let frame_up_sequence = u32::from_be_bytes(
            frame[6..10]
                .try_into()
                .map_err(|_| SingleMarkerXyzCanaryError::CarrierFrameNotExact)?,
        );
        let rpc_call_id = u32::from_be_bytes(
            frame[28..32]
                .try_into()
                .map_err(|_| SingleMarkerXyzCanaryError::CarrierFrameNotExact)?,
        );
        let identity = SingleMarkerXyzCarrierIdentity {
            frame_up_sequence,
            rpc_call_id,
            session_sequence: request.session_sequence,
        };
        let ledger = self.ledger.as_mut().expect("armed canary owns ledger");
        match ledger.arm_frame(
            connection_epoch,
            frame_sequence_start,
            pack,
            frame,
            self.config.marker_number,
            self.config.target_position,
        ) {
            OfflineAutomarkerTcpArmResult::Armed { proof, .. }
                if proof.allowed_mutable_bytes == 16
                    && proof.all_other_bytes_identical
                    && proof.game_owned_values_identical => {}
            OfflineAutomarkerTcpArmResult::Armed { .. }
            | OfflineAutomarkerTcpArmResult::OriginalUnchanged { .. } => {
                return self.abort(SingleMarkerXyzCanaryError::RewriteArmRejected);
            }
        }
        self.carrier = Some(identity);
        self.state = SingleMarkerXyzCanaryState::AwaitingRewrite;
        Ok(identity)
    }

    /// Phase one of the outbound boundary. This computes a deterministic
    /// replacement but does not claim it was sent and does not advance the
    /// confirmation clock. The bridge must either cancel before attempting a
    /// send or report the exact result through `commit_prepared_rewrite`.
    pub fn prepare_outbound_segment(
        &mut self,
        connection_epoch: u64,
        sequence_start: u32,
        payload: &[u8],
        expected_packet_send_len: usize,
        context: SingleMarkerXyzCanaryContext<'_>,
    ) -> SingleMarkerXyzSegmentDisposition {
        if self.pending_rewrite.is_some() {
            self.abort_in_place(SingleMarkerXyzCanaryError::PreparedRewriteMismatch);
            return SingleMarkerXyzSegmentDisposition::AbortWithoutReinject(
                SingleMarkerXyzCanaryError::PreparedRewriteMismatch,
            );
        }
        if !matches!(
            self.state,
            SingleMarkerXyzCanaryState::AwaitingRewrite
                | SingleMarkerXyzCanaryState::AwaitingAcknowledgement { .. }
                | SingleMarkerXyzCanaryState::Aborted(_)
        ) {
            return SingleMarkerXyzSegmentDisposition::SendOriginal(payload.to_vec());
        }
        if self.connection_epoch() != Some(connection_epoch) {
            self.abort_in_place(SingleMarkerXyzCanaryError::ConnectionEpochChanged);
            return if self.rewrite_obligation_active {
                SingleMarkerXyzSegmentDisposition::AbortWithoutReinject(
                    SingleMarkerXyzCanaryError::ConnectionEpochChanged,
                )
            } else {
                SingleMarkerXyzSegmentDisposition::SendOriginal(payload.to_vec())
            };
        }
        // A logical abort after a committed modified send is terminal for the
        // one-shot outcome, but cannot erase the TCP rewrite obligation.
        // Matching retransmissions must still receive identical bytes until
        // the operation is ACKed or the connection terminates.
        if validate_context(&self.config, context).is_err() {
            self.abort_in_place(SingleMarkerXyzCanaryError::SceneChanged);
            if !self.rewrite_obligation_active {
                return SingleMarkerXyzSegmentDisposition::SendOriginal(payload.to_vec());
            }
        }
        let Some(ledger) = self.ledger.as_mut() else {
            return SingleMarkerXyzSegmentDisposition::SendOriginal(payload.to_vec());
        };
        let result = ledger.rewrite_segment(connection_epoch, sequence_start, payload);
        match result {
            OfflineAutomarkerTcpSegmentResult::Rewritten { changed_bytes, .. }
                if changed_bytes > 0 =>
            {
                let preparation = SingleMarkerXyzPreparedRewrite {
                    preparation_id: self.next_preparation_id,
                    original_payload: payload.to_vec(),
                    rewritten_payload: result.payload().to_vec(),
                    expected_packet_send_len,
                    changed_bytes,
                    first_modified_emission: !self.rewrite_obligation_active,
                };
                self.next_preparation_id = self.next_preparation_id.wrapping_add(1).max(1);
                self.pending_rewrite = Some(PendingRewrite {
                    preparation: preparation.clone(),
                    stamp: SingleMarkerXyzCommittedRewriteStamp {
                        runtime_revision: context.runtime_revision,
                        monotonic_millis: context.observation_monotonic_millis,
                    },
                });
                if !matches!(self.state, SingleMarkerXyzCanaryState::Aborted(_)) {
                    self.state = SingleMarkerXyzCanaryState::AwaitingExternalSend;
                }
                SingleMarkerXyzSegmentDisposition::PreparedRewriteNeedsPacketChecksumRepair(
                    preparation,
                )
            }
            // An overlap can cover only immutable bytes in the registered
            // frame. No output byte changed, so checksum repair is forbidden.
            OfflineAutomarkerTcpSegmentResult::Rewritten { payload, .. } => {
                SingleMarkerXyzSegmentDisposition::SendOriginal(payload)
            }
            OfflineAutomarkerTcpSegmentResult::OriginalUnchanged { payload, reason }
                if !self.rewrite_obligation_active =>
            {
                self.abort_in_place(SingleMarkerXyzCanaryError::RewriteRejected);
                let _ = reason;
                SingleMarkerXyzSegmentDisposition::SendOriginal(payload)
            }
            OfflineAutomarkerTcpSegmentResult::OriginalUnchanged { payload, reason } => {
                if matches!(reason, OfflineAutomarkerTcpSegmentReason::NoActiveOverlap) {
                    SingleMarkerXyzSegmentDisposition::SendOriginal(payload)
                } else {
                    self.abort_in_place(SingleMarkerXyzCanaryError::RewriteRejected);
                    SingleMarkerXyzSegmentDisposition::AbortWithoutReinject(
                        SingleMarkerXyzCanaryError::RewriteRejected,
                    )
                }
            }
        }
    }

    /// Abandons a prepared rewrite before checksum repair or any send attempt.
    /// Because no modified bytes may have left the host, this is the only
    /// failure path that authorizes reinjection of the exact held original.
    pub fn cancel_prepared_rewrite_before_send(
        &mut self,
        preparation_id: u64,
    ) -> SingleMarkerXyzSegmentDisposition {
        let Some(pending) = self.pending_rewrite.take() else {
            self.abort_in_place(SingleMarkerXyzCanaryError::PreparedRewriteMismatch);
            return SingleMarkerXyzSegmentDisposition::AbortWithoutReinject(
                SingleMarkerXyzCanaryError::PreparedRewriteMismatch,
            );
        };
        if pending.preparation.preparation_id != preparation_id {
            self.pending_rewrite = Some(pending);
            self.abort_in_place(SingleMarkerXyzCanaryError::PreparedRewriteMismatch);
            return SingleMarkerXyzSegmentDisposition::AbortWithoutReinject(
                SingleMarkerXyzCanaryError::PreparedRewriteMismatch,
            );
        }
        let original = pending.preparation.original_payload;
        self.abort_in_place(SingleMarkerXyzCanaryError::PreSendPreparationFailed);
        if self.rewrite_obligation_active {
            SingleMarkerXyzSegmentDisposition::AbortWithoutReinject(
                SingleMarkerXyzCanaryError::PreSendPreparationFailed,
            )
        } else {
            SingleMarkerXyzSegmentDisposition::SendOriginal(original)
        }
    }

    /// Phase two of the outbound boundary. Only an exact full-packet send
    /// commits the first rewrite stamp. False or short sends are indeterminate:
    /// the original overlap is never authorized afterward.
    pub fn commit_prepared_rewrite(
        &mut self,
        preparation_id: u64,
        outcome: SingleMarkerXyzExternalSendOutcome,
    ) -> SingleMarkerXyzCommitDisposition {
        let Some(pending) = self.pending_rewrite.take() else {
            self.abort_in_place(SingleMarkerXyzCanaryError::PreparedRewriteMismatch);
            return SingleMarkerXyzCommitDisposition::AbortWithoutReinject(
                SingleMarkerXyzCanaryError::PreparedRewriteMismatch,
            );
        };
        if pending.preparation.preparation_id != preparation_id {
            self.pending_rewrite = Some(pending);
            self.abort_in_place(SingleMarkerXyzCanaryError::PreparedRewriteMismatch);
            return SingleMarkerXyzCommitDisposition::AbortWithoutReinject(
                SingleMarkerXyzCanaryError::PreparedRewriteMismatch,
            );
        }
        let complete = matches!(
            outcome,
            SingleMarkerXyzExternalSendOutcome::Complete { bytes_sent }
                if bytes_sent == pending.preparation.expected_packet_send_len
        );
        if !complete {
            // The bridge attempted a modified send. A false or short result
            // cannot prove that no replacement byte reached the stack.
            self.rewrite_obligation_active = true;
            self.abort_in_place(SingleMarkerXyzCanaryError::IndeterminateModifiedSend);
            return SingleMarkerXyzCommitDisposition::AbortWithoutReinject(
                SingleMarkerXyzCanaryError::IndeterminateModifiedSend,
            );
        }
        self.rewrite_obligation_active = true;
        if !self.modified_send_committed {
            self.modified_send_committed = true;
            self.committed_rewrite_stamp = Some(pending.stamp);
        }
        self.refresh_confirmation_state();
        SingleMarkerXyzCommitDisposition::Committed
    }

    pub fn committed_rewrite_stamp(&self) -> Option<SingleMarkerXyzCommittedRewriteStamp> {
        self.committed_rewrite_stamp
    }

    /// First-canary entry point: the exact 197-byte frame must be wholly
    /// present in the packet currently held by the divert boundary. Split
    /// frames and SYN-with-payload are rejected before any mutation is armed.
    #[must_use = "a prepared rewrite must be externally sent and committed or explicitly cancelled"]
    pub fn intercept_complete_carrier_segment(
        &mut self,
        pack: &ProtocolPack,
        segment: SingleMarkerXyzInterceptedSegment<'_>,
        context: SingleMarkerXyzCanaryContext<'_>,
    ) -> Result<SingleMarkerXyzSegmentDisposition, SingleMarkerXyzCanaryError> {
        if segment.tcp_syn {
            return self.abort(SingleMarkerXyzCanaryError::CarrierFrameNotExact);
        }
        let frame_end = segment
            .frame_payload_offset
            .checked_add(EXACT_FRAME_BYTES)
            .ok_or(SingleMarkerXyzCanaryError::CarrierFrameNotExact)
            .or_else(|reason| self.abort(reason))?;
        let frame = segment
            .payload
            .get(segment.frame_payload_offset..frame_end)
            .ok_or(SingleMarkerXyzCanaryError::CarrierFrameNotExact)
            .or_else(|reason| self.abort(reason))?;
        let frame_sequence_start = segment
            .tcp_sequence_start
            .wrapping_add(segment.frame_payload_offset as u32);
        self.observe_fresh_carrier(
            pack,
            segment.connection_epoch,
            frame_sequence_start,
            segment.observed_age_millis,
            frame,
            context,
        )?;
        Ok(self.prepare_outbound_segment(
            segment.connection_epoch,
            segment.tcp_sequence_start,
            segment.payload,
            segment.held_packet_len,
            context,
        ))
    }

    pub fn observe_cumulative_ack(
        &mut self,
        connection_epoch: u64,
        cumulative_ack: u32,
        context: SingleMarkerXyzCanaryContext<'_>,
    ) -> OfflineAutomarkerTcpAckResult {
        if self.connection_epoch() != Some(connection_epoch) || self.ledger.is_none() {
            self.abort_in_place(SingleMarkerXyzCanaryError::ConnectionEpochChanged);
            return OfflineAutomarkerTcpAckResult {
                retired_operations: 0,
                active_operations: 0,
                ledger_poisoned: true,
            };
        }
        if validate_context(&self.config, context).is_err() {
            self.abort_in_place(SingleMarkerXyzCanaryError::SceneChanged);
            if !self.rewrite_obligation_active {
                return OfflineAutomarkerTcpAckResult {
                    retired_operations: 0,
                    active_operations: 0,
                    ledger_poisoned: true,
                };
            }
        }
        let result = self
            .ledger
            .as_mut()
            .expect("ACK observation requires an armed canary")
            .observe_cumulative_ack(connection_epoch, cumulative_ack);
        if self.connection_epoch() != Some(connection_epoch) || result.ledger_poisoned {
            self.abort_in_place(SingleMarkerXyzCanaryError::ConnectionEpochChanged);
        } else if self.modified_send_committed && result.retired_operations == 1 {
            self.transport_ack_observed = true;
            self.refresh_confirmation_state();
        }
        result
    }

    /// FIN/RST (or an externally proven connection teardown) ends every
    /// retransmission obligation for this epoch. It never turns an incomplete
    /// canary into success.
    pub fn observe_connection_terminated(&mut self, connection_epoch: u64) {
        if self.connection_epoch() != Some(connection_epoch) {
            self.abort_in_place(SingleMarkerXyzCanaryError::ConnectionEpochChanged);
            return;
        }
        self.ledger = None;
        self.pending_rewrite = None;
        if !matches!(self.state, SingleMarkerXyzCanaryState::Succeeded) {
            self.abort_in_place(SingleMarkerXyzCanaryError::ConnectionTerminated);
        }
    }

    pub fn observe_rpc_return(
        &mut self,
        call_id: u32,
        successful_empty_return: bool,
        observation_runtime_revision: u64,
    ) -> Result<(), SingleMarkerXyzCanaryError> {
        self.require_confirmation_phase()?;
        if observation_runtime_revision
            <= self
                .committed_rewrite_stamp
                .map(|stamp| stamp.runtime_revision)
                .unwrap_or(u64::MAX)
        {
            return self.abort(SingleMarkerXyzCanaryError::StaleConfirmation);
        }
        if self.carrier.map(|carrier| carrier.rpc_call_id) != Some(call_id) {
            return self.abort(SingleMarkerXyzCanaryError::WrongRpcCallId);
        }
        if !successful_empty_return {
            return self.abort(SingleMarkerXyzCanaryError::NegativeRpcReturn);
        }
        self.rpc_return_observed = true;
        self.refresh_confirmation_state();
        Ok(())
    }

    pub fn observe_authoritative_self_add(
        &mut self,
        marker_number: u8,
        position: AutomarkerRequestXyz,
        observation_runtime_revision: u64,
        new_instance_assertion: bool,
    ) -> Result<(), SingleMarkerXyzCanaryError> {
        self.require_confirmation_phase()?;
        if observation_runtime_revision
            <= self
                .committed_rewrite_stamp
                .map(|stamp| stamp.runtime_revision)
                .unwrap_or(u64::MAX)
        {
            return self.abort(SingleMarkerXyzCanaryError::StaleConfirmation);
        }
        // This is explicitly only a bridge assertion. Activation remains
        // blocked until the bridge derives it by comparing fresh authoritative
        // passive-instance evidence against the pre-rewrite snapshot.
        if !new_instance_assertion {
            return self.abort(SingleMarkerXyzCanaryError::AuthoritativeAddNotNew);
        }
        if marker_number != self.config.marker_number {
            return self.abort(SingleMarkerXyzCanaryError::WrongAuthoritativeMarker);
        }
        if !same_position_bits(position, self.config.target_position) {
            return self.abort(SingleMarkerXyzCanaryError::AuthoritativePositionMismatch);
        }
        self.authoritative_add_observed = true;
        self.refresh_confirmation_state();
        Ok(())
    }

    pub fn revalidate_context(
        &mut self,
        connection_epoch: u64,
        context: SingleMarkerXyzCanaryContext<'_>,
    ) -> Result<(), SingleMarkerXyzCanaryError> {
        self.require_context_and_epoch(connection_epoch, context)
    }

    pub fn abort_for_timeout(&mut self) -> Result<(), SingleMarkerXyzCanaryError> {
        self.abort(SingleMarkerXyzCanaryError::TimedOut)
    }

    fn require_confirmation_phase(&mut self) -> Result<(), SingleMarkerXyzCanaryError> {
        if !self.modified_send_committed {
            return self.abort(SingleMarkerXyzCanaryError::ConfirmationBeforeRewrite);
        }
        if let SingleMarkerXyzCanaryState::Aborted(reason) = self.state {
            return Err(reason);
        }
        Ok(())
    }

    fn require_context_and_epoch(
        &mut self,
        connection_epoch: u64,
        context: SingleMarkerXyzCanaryContext<'_>,
    ) -> Result<(), SingleMarkerXyzCanaryError> {
        if self.connection_epoch() != Some(connection_epoch) {
            return self.abort(SingleMarkerXyzCanaryError::ConnectionEpochChanged);
        }
        if validate_context(&self.config, context).is_err() {
            return self.abort(SingleMarkerXyzCanaryError::SceneChanged);
        }
        Ok(())
    }

    fn require_state(
        &mut self,
        expected: SingleMarkerXyzCanaryState,
    ) -> Result<(), SingleMarkerXyzCanaryError> {
        if self.state == expected {
            Ok(())
        } else if let SingleMarkerXyzCanaryState::Aborted(reason) = self.state {
            Err(reason)
        } else {
            self.abort(SingleMarkerXyzCanaryError::CarrierAlreadyConsumed)
        }
    }

    fn refresh_confirmation_state(&mut self) {
        // Aborted is terminal. The retained ledger may still produce the same
        // replacement for an already-sent range, but retransmission or ACK
        // handling can never revive, re-arm, or succeed the canary.
        if matches!(self.state, SingleMarkerXyzCanaryState::Aborted(_)) {
            return;
        }
        if self.modified_send_committed
            && self.transport_ack_observed
            && self.rpc_return_observed
            && self.authoritative_add_observed
        {
            self.state = SingleMarkerXyzCanaryState::Succeeded;
        } else if self.modified_send_committed {
            self.state = SingleMarkerXyzCanaryState::AwaitingAcknowledgement {
                transport_ack_observed: self.transport_ack_observed,
                successful_rpc_return_observed: self.rpc_return_observed,
                authoritative_self_add_observed: self.authoritative_add_observed,
            };
        }
    }

    fn abort<T>(
        &mut self,
        reason: SingleMarkerXyzCanaryError,
    ) -> Result<T, SingleMarkerXyzCanaryError> {
        self.abort_in_place(reason);
        Err(reason)
    }

    fn abort_in_place(&mut self, reason: SingleMarkerXyzCanaryError) {
        if matches!(self.state, SingleMarkerXyzCanaryState::Aborted(_)) {
            return;
        }
        self.state = SingleMarkerXyzCanaryState::Aborted(reason);
        // Retain the mapping after a modified send so matching retransmissions
        // can still receive the same replacement bytes. A poisoned/conflicting
        // overlap is returned as AbortWithoutReinject and requires reconnect.
        if !self.rewrite_obligation_active {
            self.ledger = None;
            self.pending_rewrite = None;
        }
    }
}

fn validate_context(
    config: &SingleMarkerXyzCanaryConfig,
    context: SingleMarkerXyzCanaryContext<'_>,
) -> Result<(), SingleMarkerXyzCanaryError> {
    if context.game_build != AUTOMARKER_REQUEST_BUILD {
        return Err(SingleMarkerXyzCanaryError::RuntimeBuildMismatch);
    }
    if context.runtime_revision == 0
        || context.observation_age_millis > SINGLE_MARKER_XYZ_MAX_CONTEXT_AGE_MILLIS
    {
        return Err(SingleMarkerXyzCanaryError::StaleRuntimeContext);
    }
    if config.expected_scene_family.trim().is_empty()
        || context.current_scene_family.trim().is_empty()
    {
        return Err(SingleMarkerXyzCanaryError::MissingSceneFamily);
    }
    if config.expected_scene_family != context.current_scene_family {
        return Err(SingleMarkerXyzCanaryError::SceneFamilyMismatch);
    }
    Ok(())
}

fn coordinate_is_finite(position: AutomarkerRequestXyz) -> bool {
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
    use crate::{
        AutomarkerIpv4Endpoint, AutomarkerOwnedTcpConnection, MappingProvenance, ProtocolPack,
        bind_offline_automarker_connection_epoch,
    };
    use std::net::Ipv4Addr;

    fn config() -> SingleMarkerXyzCanaryConfig {
        SingleMarkerXyzCanaryConfig {
            expected_scene_family: "mech-facility".into(),
            marker_number: 1,
            target_position: AutomarkerRequestXyz {
                x: 10.0,
                y: 20.0,
                z: 30.0,
            },
        }
    }

    fn context(family: &str) -> SingleMarkerXyzCanaryContext<'_> {
        context_at(family, 100, 1_000)
    }

    fn context_at<'a>(
        family: &'a str,
        runtime_revision: u64,
        observation_monotonic_millis: u64,
    ) -> SingleMarkerXyzCanaryContext<'a> {
        SingleMarkerXyzCanaryContext {
            game_build: AUTOMARKER_REQUEST_BUILD,
            current_scene_family: family,
            runtime_revision,
            observation_monotonic_millis,
            observation_age_millis: 0,
        }
    }

    fn current_pack() -> ProtocolPack {
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

    fn binding() -> OfflineAutomarkerConnectionEpochBinding {
        let connection = AutomarkerOwnedTcpConnection {
            process_id: 42,
            local: AutomarkerIpv4Endpoint {
                address: Ipv4Addr::new(10, 0, 0, 2),
                port: 50_000,
            },
            remote: AutomarkerIpv4Endpoint {
                address: Ipv4Addr::new(10, 0, 0, 3),
                port: 443,
            },
        };
        bind_offline_automarker_connection_epoch(42, connection, 9, true, &[connection]).unwrap()
    }

    fn armed() -> SingleMarkerXyzCanary {
        SingleMarkerXyzCanary::arm(
            config(),
            SINGLE_MARKER_XYZ_CANARY_ARM_TOKEN,
            &current_pack(),
            binding(),
            context("mech-facility"),
        )
        .unwrap()
    }

    fn armed_with_carrier(sequence_start: u32) -> (SingleMarkerXyzCanary, Vec<u8>) {
        let frame = crate::automarker_request::tests_support::synthetic_frame_for_adapter();
        assert_eq!(frame.len(), EXACT_FRAME_BYTES);
        let mut canary = armed();
        canary
            .observe_fresh_carrier(
                &current_pack(),
                9,
                sequence_start,
                0,
                &frame,
                context("mech-facility"),
            )
            .unwrap();
        (canary, frame)
    }

    fn prepare_full(
        canary: &mut SingleMarkerXyzCanary,
        sequence_start: u32,
        frame: &[u8],
        context: SingleMarkerXyzCanaryContext<'_>,
    ) -> SingleMarkerXyzPreparedRewrite {
        match canary.prepare_outbound_segment(9, sequence_start, frame, 40 + frame.len(), context) {
            SingleMarkerXyzSegmentDisposition::PreparedRewriteNeedsPacketChecksumRepair(
                prepared,
            ) => prepared,
            other => panic!("expected prepared rewrite, got {other:?}"),
        }
    }

    fn awaiting_confirmation() -> SingleMarkerXyzCanary {
        let mut canary = armed();
        canary.state = SingleMarkerXyzCanaryState::AwaitingAcknowledgement {
            transport_ack_observed: false,
            successful_rpc_return_observed: false,
            authoritative_self_add_observed: false,
        };
        canary.carrier = Some(SingleMarkerXyzCarrierIdentity {
            frame_up_sequence: 10,
            rpc_call_id: 20,
            session_sequence: 30,
        });
        canary.rewrite_obligation_active = true;
        canary.modified_send_committed = true;
        canary.committed_rewrite_stamp = Some(SingleMarkerXyzCommittedRewriteStamp {
            runtime_revision: 100,
            monotonic_millis: 1_000,
        });
        canary
    }

    #[test]
    fn default_is_inert_dry_run() {
        let canary = SingleMarkerXyzCanary::dry_run(config());
        assert_eq!(canary.state(), SingleMarkerXyzCanaryState::DryRun);
        assert_eq!(canary.connection_epoch(), None);
    }

    #[test]
    fn arm_requires_literal_exact_pack_and_scene() {
        assert_eq!(
            SingleMarkerXyzCanary::arm(
                config(),
                "almost",
                &current_pack(),
                binding(),
                context("mech-facility"),
            )
            .err(),
            Some(SingleMarkerXyzCanaryError::InvalidConsent)
        );
        assert_eq!(
            SingleMarkerXyzCanary::arm(
                config(),
                SINGLE_MARKER_XYZ_CANARY_ARM_TOKEN,
                &current_pack(),
                binding(),
                context("tina"),
            )
            .err(),
            Some(SingleMarkerXyzCanaryError::SceneFamilyMismatch)
        );
        assert_eq!(
            armed().state(),
            SingleMarkerXyzCanaryState::AwaitingFreshCarrier
        );

        let mut stale = context("mech-facility");
        stale.observation_age_millis = SINGLE_MARKER_XYZ_MAX_CONTEXT_AGE_MILLIS + 1;
        assert_eq!(
            SingleMarkerXyzCanary::arm(
                config(),
                SINGLE_MARKER_XYZ_CANARY_ARM_TOKEN,
                &current_pack(),
                binding(),
                stale,
            )
            .err(),
            Some(SingleMarkerXyzCanaryError::StaleRuntimeContext)
        );
    }

    #[test]
    fn success_requires_both_independent_server_confirmations() {
        let mut canary = awaiting_confirmation();
        canary.observe_rpc_return(20, true, 101).unwrap();
        assert_eq!(
            canary.state(),
            SingleMarkerXyzCanaryState::AwaitingAcknowledgement {
                transport_ack_observed: false,
                successful_rpc_return_observed: true,
                authoritative_self_add_observed: false,
            }
        );
        canary
            .observe_authoritative_self_add(1, config().target_position, 102, true)
            .unwrap();
        assert!(matches!(
            canary.state(),
            SingleMarkerXyzCanaryState::AwaitingAcknowledgement {
                successful_rpc_return_observed: true,
                authoritative_self_add_observed: true,
                ..
            }
        ));
        canary.transport_ack_observed = true;
        canary.refresh_confirmation_state();
        assert_eq!(canary.state(), SingleMarkerXyzCanaryState::Succeeded);
    }

    #[test]
    fn mismatched_confirmation_and_context_change_abort() {
        let mut wrong_return = awaiting_confirmation();
        assert_eq!(
            wrong_return.observe_rpc_return(21, true, 101),
            Err(SingleMarkerXyzCanaryError::WrongRpcCallId)
        );
        assert_eq!(
            wrong_return.state(),
            SingleMarkerXyzCanaryState::Aborted(SingleMarkerXyzCanaryError::WrongRpcCallId)
        );

        let mut scene_change = armed();
        assert_eq!(
            scene_change.revalidate_context(9, context("tina")),
            Err(SingleMarkerXyzCanaryError::SceneChanged)
        );

        let mut stale_confirmation = awaiting_confirmation();
        assert_eq!(
            stale_confirmation.observe_rpc_return(20, true, 100),
            Err(SingleMarkerXyzCanaryError::StaleConfirmation)
        );
    }

    #[test]
    fn target_coordinates_require_only_finite_supported_values() {
        assert!(coordinate_is_finite(AutomarkerRequestXyz {
            x: 900_000.0,
            y: -900_000.0,
            z: 800_000.0,
        }));
        assert!(!coordinate_is_finite(AutomarkerRequestXyz {
            x: f32::NAN,
            y: 0.0,
            z: 0.0,
        }));
    }

    #[test]
    fn intercept_entry_point_rejects_a_stale_held_carrier_before_decode() {
        let mut canary = armed();
        let payload = [0_u8; EXACT_FRAME_BYTES];
        let result = canary.intercept_complete_carrier_segment(
            &current_pack(),
            SingleMarkerXyzInterceptedSegment {
                connection_epoch: 9,
                tcp_sequence_start: 500,
                tcp_syn: false,
                frame_payload_offset: 0,
                observed_age_millis: SINGLE_MARKER_XYZ_MAX_CARRIER_AGE_MILLIS + 1,
                held_packet_len: 40 + payload.len(),
                payload: &payload,
            },
            context("mech-facility"),
        );
        assert_eq!(result, Err(SingleMarkerXyzCanaryError::CarrierTooOld));
        assert_eq!(
            canary.state(),
            SingleMarkerXyzCanaryState::Aborted(SingleMarkerXyzCanaryError::CarrierTooOld)
        );
    }

    #[test]
    fn aborted_state_is_terminal_during_retransmission_and_ack_bookkeeping() {
        let mut canary = awaiting_confirmation();
        canary.abort_in_place(SingleMarkerXyzCanaryError::TimedOut);
        canary.transport_ack_observed = true;
        canary.rpc_return_observed = true;
        canary.authoritative_add_observed = true;
        canary.refresh_confirmation_state();
        assert_eq!(
            canary.state(),
            SingleMarkerXyzCanaryState::Aborted(SingleMarkerXyzCanaryError::TimedOut)
        );

        let result = canary.observe_cumulative_ack(9, u32::MAX, context("mech-facility"));
        assert_eq!(result.retired_operations, 0);
        assert_eq!(
            canary.state(),
            SingleMarkerXyzCanaryState::Aborted(SingleMarkerXyzCanaryError::TimedOut)
        );
    }

    #[test]
    fn preparation_does_not_commit_and_pre_send_failure_returns_exact_original() {
        let sequence = 1_000;
        let (mut canary, frame) = armed_with_carrier(sequence);
        let prepared = prepare_full(
            &mut canary,
            sequence,
            &frame,
            context_at("mech-facility", 101, 1_100),
        );
        assert_eq!(
            canary.state(),
            SingleMarkerXyzCanaryState::AwaitingExternalSend
        );
        assert_eq!(canary.committed_rewrite_stamp(), None);
        assert_ne!(prepared.rewritten_payload, frame);
        assert_eq!(prepared.original_payload, frame);
        assert_eq!(
            canary.cancel_prepared_rewrite_before_send(prepared.preparation_id),
            SingleMarkerXyzSegmentDisposition::SendOriginal(frame.clone())
        );
        assert_eq!(
            canary.state(),
            SingleMarkerXyzCanaryState::Aborted(
                SingleMarkerXyzCanaryError::PreSendPreparationFailed
            )
        );
        assert!(canary.ledger.is_none());
    }

    #[test]
    fn only_a_full_successful_send_commits_and_pins_the_first_stamp() {
        let sequence = 2_000;
        let (mut canary, frame) = armed_with_carrier(sequence);
        let first_context = context_at("mech-facility", 101, 1_100);
        let prepared = prepare_full(&mut canary, sequence, &frame, first_context);
        assert_eq!(
            canary.commit_prepared_rewrite(
                prepared.preparation_id,
                SingleMarkerXyzExternalSendOutcome::Complete {
                    bytes_sent: prepared.expected_packet_send_len,
                },
            ),
            SingleMarkerXyzCommitDisposition::Committed
        );
        let first_stamp = SingleMarkerXyzCommittedRewriteStamp {
            runtime_revision: 101,
            monotonic_millis: 1_100,
        };
        assert_eq!(canary.committed_rewrite_stamp(), Some(first_stamp));

        let retransmission = prepare_full(
            &mut canary,
            sequence,
            &frame,
            context_at("mech-facility", 222, 9_999),
        );
        assert!(!retransmission.first_modified_emission);
        assert_eq!(
            canary.commit_prepared_rewrite(
                retransmission.preparation_id,
                SingleMarkerXyzExternalSendOutcome::Complete {
                    bytes_sent: retransmission.expected_packet_send_len,
                },
            ),
            SingleMarkerXyzCommitDisposition::Committed
        );
        assert_eq!(canary.committed_rewrite_stamp(), Some(first_stamp));
    }

    #[test]
    fn false_and_short_sends_are_indeterminate_and_never_authorize_original_overlap() {
        for outcome in [
            SingleMarkerXyzExternalSendOutcome::Failed,
            SingleMarkerXyzExternalSendOutcome::Short { bytes_sent: 12 },
            SingleMarkerXyzExternalSendOutcome::Complete { bytes_sent: 12 },
        ] {
            let sequence = 3_000;
            let (mut canary, frame) = armed_with_carrier(sequence);
            let prepared = prepare_full(
                &mut canary,
                sequence,
                &frame,
                context_at("mech-facility", 101, 1_100),
            );
            assert_eq!(
                canary.commit_prepared_rewrite(prepared.preparation_id, outcome),
                SingleMarkerXyzCommitDisposition::AbortWithoutReinject(
                    SingleMarkerXyzCanaryError::IndeterminateModifiedSend
                )
            );
            assert_eq!(canary.committed_rewrite_stamp(), None);
            let retransmission = match canary.prepare_outbound_segment(
                9,
                sequence,
                &frame,
                40 + frame.len(),
                context("mech-facility"),
            ) {
                SingleMarkerXyzSegmentDisposition::PreparedRewriteNeedsPacketChecksumRepair(
                    prepared,
                ) => prepared,
                other => panic!("indeterminate send lost rewrite obligation: {other:?}"),
            };
            assert_eq!(
                canary.cancel_prepared_rewrite_before_send(retransmission.preparation_id),
                SingleMarkerXyzSegmentDisposition::AbortWithoutReinject(
                    SingleMarkerXyzCanaryError::PreSendPreparationFailed
                )
            );
        }
    }

    #[test]
    fn zero_changed_overlap_passes_exact_original_without_checksum_repair() {
        let sequence = 4_000;
        let (mut canary, frame) = armed_with_carrier(sequence);
        assert_eq!(
            canary
                .prepare_outbound_segment(9, sequence, &frame[..1], 41, context("mech-facility"),),
            SingleMarkerXyzSegmentDisposition::SendOriginal(frame[..1].to_vec())
        );
        assert_eq!(canary.state(), SingleMarkerXyzCanaryState::AwaitingRewrite);
        assert_eq!(canary.committed_rewrite_stamp(), None);
    }

    #[test]
    fn terminal_abort_keeps_deterministic_retransmission_mapping_until_ack() {
        let sequence = 5_000;
        let (mut canary, frame) = armed_with_carrier(sequence);
        let prepared = prepare_full(
            &mut canary,
            sequence,
            &frame,
            context_at("mech-facility", 101, 1_100),
        );
        assert_eq!(
            canary.commit_prepared_rewrite(
                prepared.preparation_id,
                SingleMarkerXyzExternalSendOutcome::Complete {
                    bytes_sent: prepared.expected_packet_send_len,
                },
            ),
            SingleMarkerXyzCommitDisposition::Committed
        );
        assert_eq!(
            canary.abort_for_timeout(),
            Err(SingleMarkerXyzCanaryError::TimedOut)
        );

        let retransmission = prepare_full(
            &mut canary,
            sequence,
            &frame,
            context_at("wrong-family", 999, 99_999),
        );
        assert_eq!(retransmission.rewritten_payload, prepared.rewritten_payload);
        assert_eq!(
            canary.commit_prepared_rewrite(
                retransmission.preparation_id,
                SingleMarkerXyzExternalSendOutcome::Complete {
                    bytes_sent: retransmission.expected_packet_send_len,
                },
            ),
            SingleMarkerXyzCommitDisposition::Committed
        );
        assert_eq!(
            canary.state(),
            SingleMarkerXyzCanaryState::Aborted(SingleMarkerXyzCanaryError::TimedOut)
        );
        assert_eq!(
            canary.prepare_outbound_segment(
                9,
                sequence.wrapping_add(1_000),
                b"ordinary",
                48,
                context("mech-facility"),
            ),
            SingleMarkerXyzSegmentDisposition::SendOriginal(b"ordinary".to_vec())
        );

        let ack = canary.observe_cumulative_ack(
            9,
            sequence.wrapping_add(EXACT_FRAME_BYTES as u32),
            context("mech-facility"),
        );
        assert_eq!(ack.retired_operations, 1);
        assert_eq!(ack.active_operations, 0);
        assert_eq!(
            canary.prepare_outbound_segment(
                9,
                sequence,
                &frame,
                40 + frame.len(),
                context("mech-facility"),
            ),
            SingleMarkerXyzSegmentDisposition::SendOriginal(frame)
        );
    }

    #[test]
    fn connection_termination_discards_the_retained_mapping() {
        let sequence = 6_000;
        let (mut canary, frame) = armed_with_carrier(sequence);
        let prepared = prepare_full(
            &mut canary,
            sequence,
            &frame,
            context_at("mech-facility", 101, 1_100),
        );
        assert_eq!(
            canary.commit_prepared_rewrite(
                prepared.preparation_id,
                SingleMarkerXyzExternalSendOutcome::Complete {
                    bytes_sent: prepared.expected_packet_send_len,
                },
            ),
            SingleMarkerXyzCommitDisposition::Committed
        );
        canary.observe_connection_terminated(9);
        assert!(canary.ledger.is_none());
        assert_eq!(
            canary.state(),
            SingleMarkerXyzCanaryState::Aborted(SingleMarkerXyzCanaryError::ConnectionTerminated)
        );
        assert_eq!(
            canary.prepare_outbound_segment(
                9,
                sequence,
                &frame,
                40 + frame.len(),
                context("mech-facility"),
            ),
            SingleMarkerXyzSegmentDisposition::SendOriginal(frame)
        );
    }
}
