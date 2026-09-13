//! Read-only decoding and offline copied-buffer verification of the observed
//! current-build ground-marker request.
//!
//! This boundary exists only to preserve evidence from already captured
//! `World.UseSlot` requests. It is intentionally separate from combat action
//! decoding, canonical events, run timing, and every outbound transport.

use aes::Aes128;
use cbc::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
use hmac::{Hmac, Mac};
use rlogs_capture::{
    MAX_TCP_SIGNATURE_PREFIX_BYTES, TcpPayloadDirection, TcpPayloadSignatureResult,
};
use serde::Serialize;
use sha2::Sha256;
use std::ops::Range;
use thiserror::Error;

use crate::ProtocolPack;

type Aes128CbcDecryptor = cbc::Decryptor<Aes128>;
type HmacSha256 = Hmac<Sha256>;

pub const AUTOMARKER_REQUEST_BUILD: &str = "25247556";
pub const AUTOMARKER_REQUEST_PACK_DIGEST: &str =
    "sha256:480f928cca6baf19c1ebaf85e8c52f2f1852096167043260503cca6d46133a60";

const IV_LENGTH: usize = 16;
const MAC_LENGTH: usize = 32;
const ENVELOPE_PREFIX_LENGTH: usize = IV_LENGTH + MAC_LENGTH;
const AES_BLOCK_LENGTH: usize = 16;
const MAX_ABS_MARKER_COORDINATE: f32 = 1_000_000.0;
const OBSERVED_APPLICATION_LENGTH: usize = 161;
const OBSERVED_NESTED_CALL_LENGTH: usize = 187;
const OBSERVED_FRAME_UP_LENGTH: usize = 197;
const FRAME_HEADER_LENGTH: usize = 6;
const FRAME_UP_PREFIX_LENGTH: usize = 4;
const CALL_ROUTE_HEADER_LENGTH: usize = 20;
const COMPRESSION_FLAG: u16 = 0x8000;
const FRAME_UP_FRAGMENT: u16 = 5;
const CALL_FRAGMENT: u16 = 1;
const WORLD_SERVICE_ID: u64 = 103_198_054;
const WORLD_STUB_ID: u32 = 1;
const USE_SLOT_METHOD_ID: u32 = 249_858;

/// Recognizes an exact current-build ground-marker request inside a bounded
/// contiguous TCP prefix.
///
/// This late-attach signature exists for passive mirror diagnostics that may
/// start after the normal early BPSR server signature has already passed. It
/// requires the complete uncompressed 197-byte FrameUp/Call layout and the
/// authenticated marker application body. No payload or decoded identity is
/// returned to the capture boundary.
pub fn classify_observed_automarker_tcp_prefix(payload: &[u8]) -> TcpPayloadSignatureResult {
    if payload.len() > MAX_TCP_SIGNATURE_PREFIX_BYTES {
        return TcpPayloadSignatureResult::Reject;
    }
    for start in 0..payload.len().saturating_sub(OBSERVED_FRAME_UP_LENGTH - 1) {
        let candidate = &payload[start..start + OBSERVED_FRAME_UP_LENGTH];
        if u32::from_be_bytes(candidate[0..4].try_into().expect("fixed outer length")) as usize
            != OBSERVED_FRAME_UP_LENGTH
            || u16::from_be_bytes(candidate[4..6].try_into().expect("fixed outer fragment"))
                != FRAME_UP_FRAGMENT
            || u32::from_be_bytes(candidate[10..14].try_into().expect("fixed nested length"))
                as usize
                != OBSERVED_NESTED_CALL_LENGTH
            || u16::from_be_bytes(candidate[14..16].try_into().expect("fixed nested fragment"))
                != CALL_FRAGMENT
            || u64::from_be_bytes(candidate[16..24].try_into().expect("fixed service"))
                != WORLD_SERVICE_ID
            || u32::from_be_bytes(candidate[24..28].try_into().expect("fixed stub"))
                != WORLD_STUB_ID
            || u32::from_be_bytes(candidate[32..36].try_into().expect("fixed method"))
                != USE_SLOT_METHOD_ID
        {
            continue;
        }
        let application = &candidate[36..];
        let mut scratch = Vec::new();
        if one_message(application, "Zproto.World.Types.UseSlot", 1)
            .and_then(|request| decode_request(request, &mut scratch))
            .is_ok()
        {
            return TcpPayloadSignatureResult::Match(TcpPayloadDirection::ClientToServer);
        }
    }
    if payload.len() == MAX_TCP_SIGNATURE_PREFIX_BYTES {
        TcpPayloadSignatureResult::Reject
    } else {
        TcpPayloadSignatureResult::NeedMore
    }
}

// Current observations prove continuity of these gameplay-envelope keys from
// the reviewed source build. They authenticate only the captured marker
// telemetry decoded here; their presence grants no request-generation right.
const SKILL_AES_KEY: [u8; 16] = [
    0x3d, 0x09, 0xd6, 0x69, 0x1d, 0xd9, 0x7a, 0x7c, 0xf9, 0xae, 0x12, 0x2c, 0x06, 0xef, 0x3c, 0x84,
];
const SKILL_HMAC_KEY: [u8; 32] = [
    0x24, 0x4b, 0x9a, 0xab, 0x92, 0x60, 0x5e, 0xbd, 0xf4, 0x6b, 0x7f, 0x32, 0x1a, 0x18, 0x70, 0x9a,
    0xff, 0x63, 0x3c, 0x03, 0x86, 0x30, 0xb7, 0xea, 0xc2, 0xbc, 0x95, 0xec, 0xd6, 0xa9, 0xc3, 0x36,
];

/// Four-float `Zproto.Position` carried by an observed marker request.
///
/// The fourth value is expressed in degrees in the current marker samples. A
/// distinct type prevents it from inheriting the canonical skill decoder's
/// historical `direction_radians` label.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct AutomarkerRequestPosition {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub heading_degrees: f32,
}

/// XYZ-only input accepted by the offline substitution verifier.
///
/// Heading is deliberately absent so the target heading and the complete
/// game-owned current position remain byte-identical to the carrier request.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct AutomarkerRequestXyz {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// Authenticated plaintext attached to a current-build marker request.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct AutomarkerRequestAttributes {
    pub timestamp: u64,
    pub velocity: f32,
    pub attack_speed_pct: i32,
    pub cast_speed_pct: i32,
    pub charge_speed_pct: Option<i32>,
    /// Protobuf field 6. Its meaning and native generation rule remain unknown.
    pub opaque_current_build_scalar: f32,
}

/// A decoded request observation. This type cannot encode or transmit itself.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ObservedAutomarkerRequest {
    pub marker_number: u8,
    pub slot_id: i32,
    pub skill_uuid: i32,
    pub skill_id: i32,
    pub skill_level: i32,
    pub begin_time: i64,
    pub target_position: AutomarkerRequestPosition,
    pub current_position: AutomarkerRequestPosition,
    /// `UseSlotRequest.sessionSequence` (protobuf field 5). This is observed
    /// state only; the decoder does not manufacture a session sequence.
    pub session_sequence: u32,
    pub attributes: AutomarkerRequestAttributes,
}

/// Sanitized local-player position carried by one already-observed outbound
/// `World.UseSlot` skill request. This type has no encoder or transport API.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ObservedUseSlotCurrentPosition {
    pub current_position: AutomarkerRequestXyz,
    /// Game-owned `UseSlotRequest.sessionSequence` observed on the wire.
    pub session_sequence: u32,
}

/// Result of a pure, offline substitution proof over a copied application body.
///
/// The verifier does not return the changed bytes and has no transport access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct OfflineAutomarkerSubstitutionProof {
    pub original_application_length_bytes: usize,
    pub substituted_application_length_bytes: usize,
    pub nested_call_length_bytes: usize,
    pub outer_frame_up_length_bytes: usize,
    pub allowed_mutable_bytes: usize,
    pub all_other_bytes_identical: bool,
    pub game_owned_values_identical: bool,
    pub authenticated_envelope_bytes_identical: bool,
    pub target_heading_bytes_identical: bool,
    pub current_position_bytes_identical: bool,
    pub exact_build_decode_succeeded: bool,
    pub packet_transmission_performed: bool,
}

/// Result of attempting an exact-layout substitution on a copied BPSR frame.
///
/// A rejected input always returns the original bytes byte-for-byte. This
/// type is deliberately transport-agnostic: it has no packet, socket,
/// interception, suppression, or process handle.
#[derive(Debug, Clone, PartialEq)]
pub enum OfflineAutomarkerFrameSubstitution {
    Substituted {
        frame: Vec<u8>,
        proof: OfflineAutomarkerSubstitutionProof,
    },
    OriginalUnchanged {
        frame: Vec<u8>,
        reason: OfflineAutomarkerSubstitutionError,
    },
}

impl OfflineAutomarkerFrameSubstitution {
    pub fn frame(&self) -> &[u8] {
        match self {
            Self::Substituted { frame, .. } | Self::OriginalUnchanged { frame, .. } => frame,
        }
    }

    pub fn was_substituted(&self) -> bool {
        matches!(self, Self::Substituted { .. })
    }
}

/// Why an offline TCP rewrite ledger can no longer safely rewrite its current
/// connection epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfflineAutomarkerTcpPoisonReason {
    GapObserved,
    ConflictingRetransmission,
    AmbiguousSequenceRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OfflineAutomarkerTcpArmError {
    EpochMismatch { expected: u64, actual: u64 },
    LedgerPoisoned(OfflineAutomarkerTcpPoisonReason),
    FrameRejected(OfflineAutomarkerSubstitutionError),
    OverlappingOperation,
    AmbiguousSequenceRange,
    OperationCapacityExceeded,
}

/// Result of arming an offline rewrite operation. A failure always carries an
/// unchanged owned copy of the candidate frame.
#[derive(Debug, Clone, PartialEq)]
pub enum OfflineAutomarkerTcpArmResult {
    Armed {
        sequence_start: u32,
        frame_length_bytes: usize,
        proof: OfflineAutomarkerSubstitutionProof,
    },
    OriginalUnchanged {
        frame: Vec<u8>,
        reason: OfflineAutomarkerTcpArmError,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OfflineAutomarkerTcpSegmentReason {
    EpochMismatch { expected: u64, actual: u64 },
    LedgerPoisoned(OfflineAutomarkerTcpPoisonReason),
    NoActiveOverlap,
    ConflictingRetransmission,
    AmbiguousSequenceRange,
}

/// Pure offline result for one copied TCP payload segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OfflineAutomarkerTcpSegmentResult {
    Rewritten {
        payload: Vec<u8>,
        overlapped_bytes: usize,
        changed_bytes: usize,
        operations_touched: usize,
    },
    OriginalUnchanged {
        payload: Vec<u8>,
        reason: OfflineAutomarkerTcpSegmentReason,
    },
}

impl OfflineAutomarkerTcpSegmentResult {
    pub fn payload(&self) -> &[u8] {
        match self {
            Self::Rewritten { payload, .. } | Self::OriginalUnchanged { payload, .. } => payload,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OfflineAutomarkerTcpAckResult {
    pub retired_operations: usize,
    pub active_operations: usize,
    pub ledger_poisoned: bool,
}

#[derive(Debug, Clone)]
struct OfflineAutomarkerTcpOperation {
    sequence_start: u32,
    original: Vec<u8>,
    replacement: Vec<u8>,
}

/// Pure, connection-local TCP sequence-range rewrite ledger.
///
/// The ledger has no network or process capabilities. Callers supply copied
/// bytes plus an explicit connection epoch. It supports segmentation,
/// coalescing, reordering, retransmission, overlap, and RFC-style 32-bit
/// sequence wrap as long as compared ranges stay within one serial half-space.
/// A gap, conflicting retransmission, or ambiguous half-space comparison
/// poisons the epoch and makes all subsequent payloads pass through unchanged
/// until `reset_connection_epoch` is called.
#[derive(Debug, Clone)]
pub struct OfflineAutomarkerTcpRewriteLedger {
    connection_epoch: u64,
    operations: Vec<OfflineAutomarkerTcpOperation>,
    poison: Option<OfflineAutomarkerTcpPoisonReason>,
}

impl OfflineAutomarkerTcpRewriteLedger {
    const MAX_ACTIVE_OPERATIONS: usize = 64;

    pub fn new(connection_epoch: u64) -> Self {
        Self {
            connection_epoch,
            operations: Vec::new(),
            poison: None,
        }
    }

    pub fn connection_epoch(&self) -> u64 {
        self.connection_epoch
    }

    pub fn active_operations(&self) -> usize {
        self.operations.len()
    }

    pub fn poison_reason(&self) -> Option<OfflineAutomarkerTcpPoisonReason> {
        self.poison
    }

    pub fn reset_connection_epoch(&mut self, connection_epoch: u64) {
        self.connection_epoch = connection_epoch;
        self.operations.clear();
        self.poison = None;
    }

    /// Validates and records one exact-length frame transformation without
    /// touching any TCP payload. The frame copy is returned unchanged if the
    /// operation cannot be armed.
    pub fn arm_frame(
        &mut self,
        connection_epoch: u64,
        sequence_start: u32,
        pack: &ProtocolPack,
        original_frame: &[u8],
        replacement_marker: u8,
        target_position: AutomarkerRequestXyz,
    ) -> OfflineAutomarkerTcpArmResult {
        let reject = |reason| OfflineAutomarkerTcpArmResult::OriginalUnchanged {
            frame: original_frame.to_vec(),
            reason,
        };
        if connection_epoch != self.connection_epoch {
            return reject(OfflineAutomarkerTcpArmError::EpochMismatch {
                expected: self.connection_epoch,
                actual: connection_epoch,
            });
        }
        if let Some(reason) = self.poison {
            return reject(OfflineAutomarkerTcpArmError::LedgerPoisoned(reason));
        }
        if self.operations.len() >= Self::MAX_ACTIVE_OPERATIONS {
            return reject(OfflineAutomarkerTcpArmError::OperationCapacityExceeded);
        }

        let (replacement, proof) = match substitute_offline_automarker_frame(
            pack,
            original_frame,
            replacement_marker,
            target_position,
        ) {
            OfflineAutomarkerFrameSubstitution::Substituted { frame, proof } => (frame, proof),
            OfflineAutomarkerFrameSubstitution::OriginalUnchanged { reason, .. } => {
                return reject(OfflineAutomarkerTcpArmError::FrameRejected(reason));
            }
        };

        for operation in &self.operations {
            match tcp_overlap(
                operation.sequence_start,
                operation.original.len(),
                sequence_start,
                original_frame.len(),
            ) {
                Ok(Some(_)) => return reject(OfflineAutomarkerTcpArmError::OverlappingOperation),
                Ok(None) => {}
                Err(()) => {
                    return reject(OfflineAutomarkerTcpArmError::AmbiguousSequenceRange);
                }
            }
        }
        self.operations.push(OfflineAutomarkerTcpOperation {
            sequence_start,
            original: original_frame.to_vec(),
            replacement,
        });
        OfflineAutomarkerTcpArmResult::Armed {
            sequence_start,
            frame_length_bytes: original_frame.len(),
            proof,
        }
    }

    /// Rewrites every byte overlapping an armed operation in a copied segment.
    /// All overlaps are validated against the original carrier bytes before
    /// any output byte is changed, making conflicting input transactionally
    /// fail open.
    pub fn rewrite_segment(
        &mut self,
        connection_epoch: u64,
        sequence_start: u32,
        payload: &[u8],
    ) -> OfflineAutomarkerTcpSegmentResult {
        let unchanged = |reason| OfflineAutomarkerTcpSegmentResult::OriginalUnchanged {
            payload: payload.to_vec(),
            reason,
        };
        if connection_epoch != self.connection_epoch {
            return unchanged(OfflineAutomarkerTcpSegmentReason::EpochMismatch {
                expected: self.connection_epoch,
                actual: connection_epoch,
            });
        }
        if let Some(reason) = self.poison {
            return unchanged(OfflineAutomarkerTcpSegmentReason::LedgerPoisoned(reason));
        }
        if payload.len() >= TCP_SERIAL_HALF_SPACE as usize {
            self.poison(OfflineAutomarkerTcpPoisonReason::AmbiguousSequenceRange);
            return unchanged(OfflineAutomarkerTcpSegmentReason::AmbiguousSequenceRange);
        }

        let mut overlaps = Vec::new();
        for (operation_index, operation) in self.operations.iter().enumerate() {
            match tcp_overlap(
                operation.sequence_start,
                operation.original.len(),
                sequence_start,
                payload.len(),
            ) {
                Ok(Some(overlap)) => overlaps.push((operation_index, overlap)),
                Ok(None) => {}
                Err(()) => {
                    self.poison(OfflineAutomarkerTcpPoisonReason::AmbiguousSequenceRange);
                    return unchanged(OfflineAutomarkerTcpSegmentReason::AmbiguousSequenceRange);
                }
            }
        }
        if overlaps.is_empty() {
            return unchanged(OfflineAutomarkerTcpSegmentReason::NoActiveOverlap);
        }

        for (operation_index, overlap) in &overlaps {
            let operation = &self.operations[*operation_index];
            if payload[overlap.candidate.clone()] != operation.original[overlap.operation.clone()] {
                self.poison(OfflineAutomarkerTcpPoisonReason::ConflictingRetransmission);
                return unchanged(OfflineAutomarkerTcpSegmentReason::ConflictingRetransmission);
            }
        }

        let mut rewritten = payload.to_vec();
        let mut overlapped_bytes = 0;
        let mut changed_bytes = 0;
        for (operation_index, overlap) in &overlaps {
            let replacement =
                &self.operations[*operation_index].replacement[overlap.operation.clone()];
            let output = &mut rewritten[overlap.candidate.clone()];
            overlapped_bytes += output.len();
            changed_bytes += output
                .iter()
                .zip(replacement)
                .filter(|(before, after)| before != after)
                .count();
            output.copy_from_slice(replacement);
        }
        OfflineAutomarkerTcpSegmentResult::Rewritten {
            payload: rewritten,
            overlapped_bytes,
            changed_bytes,
            operations_touched: overlaps.len(),
        }
    }

    /// Marks a known TCP stream gap. Only a gap overlapping an active rewrite
    /// range poisons the epoch; unrelated gaps cannot alter an operation.
    pub fn observe_gap(
        &mut self,
        connection_epoch: u64,
        sequence_start: u32,
        length: usize,
    ) -> bool {
        if connection_epoch != self.connection_epoch || self.poison.is_some() {
            return false;
        }
        if length >= TCP_SERIAL_HALF_SPACE as usize {
            self.poison(OfflineAutomarkerTcpPoisonReason::AmbiguousSequenceRange);
            return true;
        }
        for operation in &self.operations {
            match tcp_overlap(
                operation.sequence_start,
                operation.original.len(),
                sequence_start,
                length,
            ) {
                Ok(Some(_)) => {
                    self.poison(OfflineAutomarkerTcpPoisonReason::GapObserved);
                    return true;
                }
                Ok(None) => {}
                Err(()) => {
                    self.poison(OfflineAutomarkerTcpPoisonReason::AmbiguousSequenceRange);
                    return true;
                }
            }
        }
        false
    }

    /// Retires operations covered by a cumulative TCP ACK. ACKs behind an
    /// operation do not retire it; the exactly-half-space case poisons the
    /// epoch because RFC serial ordering is undefined there.
    pub fn observe_cumulative_ack(
        &mut self,
        connection_epoch: u64,
        cumulative_ack: u32,
    ) -> OfflineAutomarkerTcpAckResult {
        if connection_epoch != self.connection_epoch || self.poison.is_some() {
            return self.ack_result(0);
        }
        let before = self.operations.len();
        let mut ambiguous = false;
        self.operations.retain(|operation| {
            let distance = cumulative_ack.wrapping_sub(operation.sequence_start);
            if distance == TCP_SERIAL_HALF_SPACE {
                ambiguous = true;
                true
            } else if distance < TCP_SERIAL_HALF_SPACE {
                (distance as usize) < operation.original.len()
            } else {
                true
            }
        });
        if ambiguous {
            self.poison(OfflineAutomarkerTcpPoisonReason::AmbiguousSequenceRange);
            return self.ack_result(0);
        }
        self.ack_result(before - self.operations.len())
    }

    fn poison(&mut self, reason: OfflineAutomarkerTcpPoisonReason) {
        self.operations.clear();
        self.poison = Some(reason);
    }

    fn ack_result(&self, retired_operations: usize) -> OfflineAutomarkerTcpAckResult {
        OfflineAutomarkerTcpAckResult {
            retired_operations,
            active_operations: self.operations.len(),
            ledger_poisoned: self.poison.is_some(),
        }
    }
}

const TCP_SERIAL_HALF_SPACE: u32 = 0x8000_0000;

#[derive(Debug, Clone)]
struct TcpOverlap {
    operation: Range<usize>,
    candidate: Range<usize>,
}

fn tcp_overlap(
    operation_start: u32,
    operation_length: usize,
    candidate_start: u32,
    candidate_length: usize,
) -> Result<Option<TcpOverlap>, ()> {
    if operation_length >= TCP_SERIAL_HALF_SPACE as usize
        || candidate_length >= TCP_SERIAL_HALF_SPACE as usize
    {
        return Err(());
    }
    let raw_delta = candidate_start.wrapping_sub(operation_start);
    if raw_delta == TCP_SERIAL_HALF_SPACE {
        return Err(());
    }
    let candidate_relative_start = if raw_delta < TCP_SERIAL_HALF_SPACE {
        i64::from(raw_delta)
    } else {
        i64::from(raw_delta) - (1_i64 << 32)
    };
    let candidate_relative_end = candidate_relative_start + candidate_length as i64;
    if candidate_relative_end > i64::from(TCP_SERIAL_HALF_SPACE) {
        return Err(());
    }
    let overlap_start = candidate_relative_start.max(0);
    let overlap_end = candidate_relative_end.min(operation_length as i64);
    if overlap_start >= overlap_end {
        return Ok(None);
    }
    Ok(Some(TcpOverlap {
        operation: overlap_start as usize..overlap_end as usize,
        candidate: (overlap_start - candidate_relative_start) as usize
            ..(overlap_end - candidate_relative_start) as usize,
    }))
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum OfflineAutomarkerSubstitutionError {
    #[error(transparent)]
    Decode(#[from] AutomarkerRequestDecodeError),
    #[error("offline substitution proof requires a 161-byte application body, got {actual}")]
    UnexpectedApplicationLength { actual: usize },
    #[error("replacement marker must be between 1 and 6")]
    InvalidReplacementMarker,
    #[error("observed protobuf field width is not stable for the requested replacement")]
    UnstableFieldWidth,
    #[error("offline substitution changed a byte outside the approved field spans")]
    UnexpectedByteChange,
    #[error("offline substitution did not decode to the requested marker and positions")]
    SubstitutedDecodeMismatch,
    #[error("{layer} frame requires exactly {expected} bytes, got {actual}")]
    UnexpectedFrameLength {
        layer: &'static str,
        expected: usize,
        actual: usize,
    },
    #[error("{layer} frame declares {declared} bytes but contains {actual}")]
    DeclaredFrameLengthMismatch {
        layer: &'static str,
        declared: usize,
        actual: usize,
    },
    #[error("{layer} frame is compressed; only the retained uncompressed layout is supported")]
    CompressedFrame { layer: &'static str },
    #[error("{layer} has fragment {actual}, expected {expected}")]
    UnexpectedFragment {
        layer: &'static str,
        expected: u16,
        actual: u16,
    },
    #[error(
        "nested call route is not exact World.UseSlot (service {service_id}, stub {stub_id}, method {method_id})"
    )]
    UnexpectedRoute {
        service_id: u64,
        stub_id: u32,
        method_id: u32,
    },
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AutomarkerRequestDecodeError {
    #[error(
        "automarker request requires exact build {AUTOMARKER_REQUEST_BUILD} and reviewed pack digest"
    )]
    UnsupportedProtocolIdentity,
    #[error("{message} ended in the middle of a field")]
    Truncated { message: &'static str },
    #[error("{message} protobuf varint overflows 64 bits")]
    VarintOverflow { message: &'static str },
    #[error("{message} protobuf tag is invalid")]
    InvalidTag { message: &'static str },
    #[error("{message} field {field} uses wire type {observed}, expected {expected}")]
    WrongWireType {
        message: &'static str,
        field: u32,
        observed: u8,
        expected: u8,
    },
    #[error("{message} field {field} appears more than once")]
    DuplicateField { message: &'static str, field: u32 },
    #[error("{message} contains unsupported field {field}")]
    UnknownField { message: &'static str, field: u32 },
    #[error("{message} is missing required field {field}")]
    MissingField { message: &'static str, field: u32 },
    #[error(
        "marker slot {slot_id} and skill {skill_id} are outside or disagree with the observed 1-through-6 mapping"
    )]
    InvalidMarkerIdentity { slot_id: i32, skill_id: i32 },
    #[error("marker request has skill UUID {skill_uuid}, outside the observed action namespace")]
    InvalidMarkerSkillUuid { skill_uuid: i32 },
    #[error("marker position field {field} is non-finite or outside its verified domain")]
    InvalidPosition { field: u32 },
    #[error("marker request field {field} has unsupported value {value}")]
    UnsupportedValue { field: u32, value: i64 },
    #[error("marker attribute envelope length {actual} is invalid")]
    InvalidEnvelopeLength { actual: usize },
    #[error("marker attribute HMAC-SHA256 verification failed")]
    MacMismatch,
    #[error("marker attribute AES-CBC PKCS#7 padding is invalid")]
    InvalidPadding,
    #[error("{message} int32 field {field} is outside the protobuf int32 domain")]
    Int32Overflow { message: &'static str, field: u32 },
}

pub fn supports_observed_automarker_requests(pack: &ProtocolPack) -> bool {
    pack.definition().target.build_id == AUTOMARKER_REQUEST_BUILD
        && pack.digest() == AUTOMARKER_REQUEST_PACK_DIGEST
}

/// Decodes one already-observed current-build marker request.
///
/// This API has no encoder and no transport access. Unknown fields and any
/// non-marker `UseSlot` identity fail closed.
pub fn decode_observed_automarker_request_into(
    pack: &ProtocolPack,
    payload: &[u8],
    scratch: &mut Vec<u8>,
) -> Result<ObservedAutomarkerRequest, AutomarkerRequestDecodeError> {
    if !supports_observed_automarker_requests(pack) {
        return Err(AutomarkerRequestDecodeError::UnsupportedProtocolIdentity);
    }
    let request = one_message(payload, "Zproto.World.Types.UseSlot", 1)?;
    decode_request(request, scratch)
}

/// Decodes only the current local-player XYZ and game-owned session sequence
/// from an already-observed exact-build `World.UseSlot` skill request.
///
/// The request route, exact build/pack identity, authenticated gameplay
/// envelope, required action identity, and finite coordinate domain are all
/// checked. No request bytes, action identity, cryptographic material, encoder,
/// or transport capability cross this boundary.
pub fn decode_observed_use_slot_current_position_into(
    pack: &ProtocolPack,
    payload: &[u8],
    scratch: &mut Vec<u8>,
) -> Result<ObservedUseSlotCurrentPosition, AutomarkerRequestDecodeError> {
    if !supports_observed_automarker_requests(pack) {
        return Err(AutomarkerRequestDecodeError::UnsupportedProtocolIdentity);
    }
    let request = one_message(payload, "Zproto.World.Types.UseSlot", 1)?;
    decode_use_slot_current_position(request, scratch)
}

/// Verifies a marker/position substitution entirely offline in a copied
/// 161-byte application body.
///
/// Only the paired slot/skill varint value bytes and target XYZ `fixed32` value
/// bytes are writable. Target heading, the complete current position, tags,
/// length prefixes, game-owned IDs, timing/session values, and the authenticated
/// envelope must remain identical.
pub fn verify_offline_automarker_substitution(
    pack: &ProtocolPack,
    original: &[u8],
    replacement_marker: u8,
    target_position: AutomarkerRequestXyz,
) -> Result<OfflineAutomarkerSubstitutionProof, OfflineAutomarkerSubstitutionError> {
    substitute_application(pack, original, replacement_marker, target_position)
        .map(|(_, proof)| proof)
}

/// Purely substitutes one retained-layout, uncompressed BPSR `FrameUp` copy.
///
/// The accepted byte layout is exactly:
/// `[FrameUp header][sequence:u32][Call header][World route][UseSlot body]`.
/// Both frame lengths, both compression flags, both fragment kinds, the exact
/// World service/stub/method route, the strict protobuf schema, and the HMAC +
/// AES-CBC attribute envelope are validated before any copied output is
/// returned as substituted. Any mismatch returns `OriginalUnchanged` with a
/// byte-for-byte copy of the input.
pub fn substitute_offline_automarker_frame(
    pack: &ProtocolPack,
    original: &[u8],
    replacement_marker: u8,
    target_position: AutomarkerRequestXyz,
) -> OfflineAutomarkerFrameSubstitution {
    match try_substitute_offline_automarker_frame(
        pack,
        original,
        replacement_marker,
        target_position,
    ) {
        Ok((frame, proof)) => OfflineAutomarkerFrameSubstitution::Substituted { frame, proof },
        Err(reason) => OfflineAutomarkerFrameSubstitution::OriginalUnchanged {
            frame: original.to_vec(),
            reason,
        },
    }
}

fn try_substitute_offline_automarker_frame(
    pack: &ProtocolPack,
    original: &[u8],
    replacement_marker: u8,
    target_position: AutomarkerRequestXyz,
) -> Result<(Vec<u8>, OfflineAutomarkerSubstitutionProof), OfflineAutomarkerSubstitutionError> {
    validate_exact_frame(
        original,
        "outer FrameUp",
        OBSERVED_FRAME_UP_LENGTH,
        FRAME_UP_FRAGMENT,
    )?;

    let nested_start = FRAME_HEADER_LENGTH + FRAME_UP_PREFIX_LENGTH;
    let nested = &original[nested_start..];
    validate_exact_frame(
        nested,
        "nested Call",
        OBSERVED_NESTED_CALL_LENGTH,
        CALL_FRAGMENT,
    )?;

    let route = &nested[FRAME_HEADER_LENGTH..FRAME_HEADER_LENGTH + CALL_ROUTE_HEADER_LENGTH];
    let service_id = u64::from_be_bytes(route[0..8].try_into().unwrap());
    let stub_id = u32::from_be_bytes(route[8..12].try_into().unwrap());
    // route[12..16] is the game-owned call ID. It is intentionally accepted
    // as opaque session state and remains outside the mutable allowlist.
    let method_id = u32::from_be_bytes(route[16..20].try_into().unwrap());
    if (service_id, stub_id, method_id) != (WORLD_SERVICE_ID, WORLD_STUB_ID, USE_SLOT_METHOD_ID) {
        return Err(OfflineAutomarkerSubstitutionError::UnexpectedRoute {
            service_id,
            stub_id,
            method_id,
        });
    }

    let application_start = nested_start + FRAME_HEADER_LENGTH + CALL_ROUTE_HEADER_LENGTH;
    let application = &original[application_start..];
    let (substituted_application, proof) =
        substitute_application(pack, application, replacement_marker, target_position)?;

    let mut substituted = original.to_vec();
    substituted[application_start..].copy_from_slice(&substituted_application);
    if substituted.len() != original.len()
        || original[..application_start] != substituted[..application_start]
    {
        return Err(OfflineAutomarkerSubstitutionError::UnexpectedByteChange);
    }
    Ok((substituted, proof))
}

fn validate_exact_frame(
    raw: &[u8],
    layer: &'static str,
    expected_length: usize,
    expected_fragment: u16,
) -> Result<(), OfflineAutomarkerSubstitutionError> {
    if raw.len() != expected_length {
        return Err(OfflineAutomarkerSubstitutionError::UnexpectedFrameLength {
            layer,
            expected: expected_length,
            actual: raw.len(),
        });
    }
    let declared = u32::from_be_bytes(raw[0..4].try_into().unwrap()) as usize;
    if declared != raw.len() {
        return Err(
            OfflineAutomarkerSubstitutionError::DeclaredFrameLengthMismatch {
                layer,
                declared,
                actual: raw.len(),
            },
        );
    }
    let raw_fragment = u16::from_be_bytes(raw[4..6].try_into().unwrap());
    if raw_fragment & COMPRESSION_FLAG != 0 {
        return Err(OfflineAutomarkerSubstitutionError::CompressedFrame { layer });
    }
    if raw_fragment != expected_fragment {
        return Err(OfflineAutomarkerSubstitutionError::UnexpectedFragment {
            layer,
            expected: expected_fragment,
            actual: raw_fragment,
        });
    }
    Ok(())
}

fn substitute_application(
    pack: &ProtocolPack,
    original: &[u8],
    replacement_marker: u8,
    target_position: AutomarkerRequestXyz,
) -> Result<(Vec<u8>, OfflineAutomarkerSubstitutionProof), OfflineAutomarkerSubstitutionError> {
    if original.len() != OBSERVED_APPLICATION_LENGTH {
        return Err(
            OfflineAutomarkerSubstitutionError::UnexpectedApplicationLength {
                actual: original.len(),
            },
        );
    }
    if !(1..=6).contains(&replacement_marker) {
        return Err(OfflineAutomarkerSubstitutionError::InvalidReplacementMarker);
    }

    let mut scratch = Vec::new();
    let original_decoded = decode_observed_automarker_request_into(pack, original, &mut scratch)?;
    let spans = substitution_spans(original)?;
    let mut substituted = original.to_vec();
    write_same_width_varint(
        &mut substituted[spans.slot_id.clone()],
        200 + u64::from(replacement_marker),
    )?;
    write_same_width_varint(
        &mut substituted[spans.skill_id.clone()],
        1100 + u64::from(replacement_marker),
    )?;
    for (range, value) in spans
        .target_position
        .iter()
        .take(3)
        .zip(xyz_values(target_position))
    {
        substituted[range.clone()].copy_from_slice(&value.to_bits().to_le_bytes());
    }

    let mut mutable = vec![false; original.len()];
    for range in spans.mutable_ranges() {
        mutable[range].fill(true);
    }
    if original
        .iter()
        .zip(&substituted)
        .zip(&mutable)
        .any(|((&before, &after), &allowed)| before != after && !allowed)
    {
        return Err(OfflineAutomarkerSubstitutionError::UnexpectedByteChange);
    }

    let substituted_decoded =
        decode_observed_automarker_request_into(pack, &substituted, &mut scratch)?;
    if substituted_decoded.marker_number != replacement_marker
        || position_xyz(substituted_decoded.target_position) != target_position
        || substituted_decoded.target_position.heading_degrees
            != original_decoded.target_position.heading_degrees
        || substituted_decoded.current_position != original_decoded.current_position
    {
        return Err(OfflineAutomarkerSubstitutionError::SubstitutedDecodeMismatch);
    }
    let game_owned_values_identical = original_decoded.skill_uuid == substituted_decoded.skill_uuid
        && original_decoded.skill_level == substituted_decoded.skill_level
        && original_decoded.begin_time == substituted_decoded.begin_time
        && original_decoded.session_sequence == substituted_decoded.session_sequence
        && original_decoded.attributes == substituted_decoded.attributes;
    let target_heading_bytes_identical =
        original[spans.target_position[3].clone()] == substituted[spans.target_position[3].clone()];
    let current_position_bytes_identical = spans
        .current_position
        .iter()
        .all(|range| original[range.clone()] == substituted[range.clone()]);
    if !game_owned_values_identical
        || !target_heading_bytes_identical
        || !current_position_bytes_identical
        || original[spans.authenticated_envelope.clone()]
            != substituted[spans.authenticated_envelope.clone()]
    {
        return Err(OfflineAutomarkerSubstitutionError::UnexpectedByteChange);
    }

    let (nested_call_length_bytes, outer_frame_up_length_bytes) =
        synthetic_uncompressed_observed_lengths(&substituted);
    let proof = OfflineAutomarkerSubstitutionProof {
        original_application_length_bytes: original.len(),
        substituted_application_length_bytes: substituted.len(),
        nested_call_length_bytes,
        outer_frame_up_length_bytes,
        allowed_mutable_bytes: mutable.into_iter().filter(|allowed| *allowed).count(),
        all_other_bytes_identical: true,
        game_owned_values_identical,
        authenticated_envelope_bytes_identical: true,
        target_heading_bytes_identical,
        current_position_bytes_identical,
        exact_build_decode_succeeded: true,
        packet_transmission_performed: false,
    };
    Ok((substituted, proof))
}

#[derive(Debug)]
struct SubstitutionSpans {
    slot_id: Range<usize>,
    skill_id: Range<usize>,
    target_position: [Range<usize>; 4],
    current_position: [Range<usize>; 4],
    authenticated_envelope: Range<usize>,
}

impl SubstitutionSpans {
    fn mutable_ranges(&self) -> impl Iterator<Item = Range<usize>> + '_ {
        std::iter::once(self.slot_id.clone())
            .chain(std::iter::once(self.skill_id.clone()))
            .chain(self.target_position.iter().take(3).cloned())
    }
}

fn substitution_spans(
    application: &[u8],
) -> Result<SubstitutionSpans, AutomarkerRequestDecodeError> {
    let request = field_value_range(application, 1, 2, "Zproto.World.Types.UseSlot")?;
    let request_bytes = &application[request.clone()];
    let slot_id = absolute(
        &request,
        field_value_range(request_bytes, 1, 0, "Zproto.UseSlotRequest")?,
    );
    let param = field_value_range(request_bytes, 3, 2, "Zproto.UseSlotRequest")?;
    let envelope = field_value_range(request_bytes, 4, 2, "Zproto.UseSlotRequest")?;
    let param_bytes = &request_bytes[param.clone()];
    let skill_id = absolute(
        &request,
        absolute(
            &param,
            field_value_range(param_bytes, 2, 0, "Zproto.UseSkillParam")?,
        ),
    );
    let target = field_value_range(param_bytes, 6, 2, "Zproto.UseSkillParam")?;
    let current = field_value_range(param_bytes, 7, 2, "Zproto.UseSkillParam")?;
    Ok(SubstitutionSpans {
        slot_id,
        skill_id,
        target_position: position_spans(application, absolute(&request, absolute(&param, target)))?,
        current_position: position_spans(
            application,
            absolute(&request, absolute(&param, current)),
        )?,
        authenticated_envelope: absolute(&request, envelope),
    })
}

fn position_spans(
    application: &[u8],
    position: Range<usize>,
) -> Result<[Range<usize>; 4], AutomarkerRequestDecodeError> {
    let bytes = &application[position.clone()];
    Ok([
        absolute(
            &position,
            field_value_range(bytes, 1, 5, "Zproto.Position")?,
        ),
        absolute(
            &position,
            field_value_range(bytes, 2, 5, "Zproto.Position")?,
        ),
        absolute(
            &position,
            field_value_range(bytes, 3, 5, "Zproto.Position")?,
        ),
        absolute(
            &position,
            field_value_range(bytes, 4, 5, "Zproto.Position")?,
        ),
    ])
}

fn field_value_range(
    raw: &[u8],
    expected_field: u32,
    expected_wire: u8,
    message: &'static str,
) -> Result<Range<usize>, AutomarkerRequestDecodeError> {
    let mut cursor = 0;
    while cursor < raw.len() {
        let (field, wire) = tag(raw, &mut cursor, message)?;
        let value_range = match wire {
            0 => {
                let start = cursor;
                let _ = varint(raw, &mut cursor, message)?;
                start..cursor
            }
            2 => {
                let value = bytes(raw, &mut cursor, message)?;
                let start = value.as_ptr() as usize - raw.as_ptr() as usize;
                start..start + value.len()
            }
            5 => {
                let start = cursor;
                let _ = fixed32(raw, &mut cursor, message)?;
                start..cursor
            }
            _ => {
                return Err(AutomarkerRequestDecodeError::WrongWireType {
                    message,
                    field,
                    observed: wire,
                    expected: expected_wire,
                });
            }
        };
        if field == expected_field {
            require_wire(message, field, wire, expected_wire)?;
            return Ok(value_range);
        }
    }
    Err(AutomarkerRequestDecodeError::MissingField {
        message,
        field: expected_field,
    })
}

fn absolute(parent: &Range<usize>, child: Range<usize>) -> Range<usize> {
    parent.start + child.start..parent.start + child.end
}

fn write_same_width_varint(
    output: &mut [u8],
    mut value: u64,
) -> Result<(), OfflineAutomarkerSubstitutionError> {
    for (index, byte) in output.iter_mut().enumerate() {
        let remaining = value >> 7;
        *byte = (value as u8 & 0x7f) | (u8::from(remaining != 0) * 0x80);
        value = remaining;
        if value == 0 {
            return if index + 1 == output.len() {
                Ok(())
            } else {
                Err(OfflineAutomarkerSubstitutionError::UnstableFieldWidth)
            };
        }
    }
    Err(OfflineAutomarkerSubstitutionError::UnstableFieldWidth)
}

fn xyz_values(position: AutomarkerRequestXyz) -> [f32; 3] {
    [position.x, position.y, position.z]
}

fn position_xyz(position: AutomarkerRequestPosition) -> AutomarkerRequestXyz {
    AutomarkerRequestXyz {
        x: position.x,
        y: position.y,
        z: position.z,
    }
}

fn synthetic_uncompressed_observed_lengths(application: &[u8]) -> (usize, usize) {
    let mut nested = Vec::with_capacity(6 + 20 + application.len());
    nested.extend_from_slice(&(6_u32 + 20 + application.len() as u32).to_be_bytes());
    nested.extend_from_slice(&1_u16.to_be_bytes());
    nested.extend_from_slice(&[0_u8; 20]);
    nested.extend_from_slice(application);
    let mut outer = Vec::with_capacity(6 + 4 + nested.len());
    outer.extend_from_slice(&(6_u32 + 4 + nested.len() as u32).to_be_bytes());
    outer.extend_from_slice(&5_u16.to_be_bytes());
    outer.extend_from_slice(&[0_u8; 4]);
    outer.extend_from_slice(&nested);
    (nested.len(), outer.len())
}

fn decode_request(
    raw: &[u8],
    scratch: &mut Vec<u8>,
) -> Result<ObservedAutomarkerRequest, AutomarkerRequestDecodeError> {
    const MESSAGE: &str = "Zproto.UseSlotRequest";
    let mut cursor = 0;
    let mut slot_id = None;
    let mut use_type = None;
    let mut extra_data = None;
    let mut attr_data = None;
    let mut session_sequence = None;
    while cursor < raw.len() {
        let (field, wire) = tag(raw, &mut cursor, MESSAGE)?;
        match field {
            1 | 2 | 5 => {
                require_wire(MESSAGE, field, wire, 0)?;
                let value = varint(raw, &mut cursor, MESSAGE)?;
                match field {
                    1 => set_once(&mut slot_id, as_i32(value, MESSAGE, field)?, MESSAGE, field)?,
                    2 => set_once(
                        &mut use_type,
                        as_i32(value, MESSAGE, field)?,
                        MESSAGE,
                        field,
                    )?,
                    5 => set_once(
                        &mut session_sequence,
                        u32::try_from(value).map_err(|_| {
                            AutomarkerRequestDecodeError::UnsupportedValue {
                                field,
                                value: value as i64,
                            }
                        })?,
                        MESSAGE,
                        field,
                    )?,
                    _ => unreachable!(),
                }
            }
            3 | 4 => {
                require_wire(MESSAGE, field, wire, 2)?;
                let value = bytes(raw, &mut cursor, MESSAGE)?;
                if field == 3 {
                    set_once(&mut extra_data, value, MESSAGE, field)?;
                } else {
                    set_once(&mut attr_data, value, MESSAGE, field)?;
                }
            }
            _ => {
                return Err(AutomarkerRequestDecodeError::UnknownField {
                    message: MESSAGE,
                    field,
                });
            }
        }
    }
    let slot_id = required(slot_id, MESSAGE, 1)?;
    let use_type = required(use_type, MESSAGE, 2)?;
    if use_type != 1 {
        return Err(AutomarkerRequestDecodeError::UnsupportedValue {
            field: 2,
            value: i64::from(use_type),
        });
    }
    let param = decode_param(required(extra_data, MESSAGE, 3)?)?;
    let marker_number = marker_number(slot_id, param.skill_id)?;
    // The action UUID is allocated independently of the marker number. All
    // current-build UseSlot observations use the 0x6....... action namespace;
    // never freeze the six sequential values from one capture as marker IDs.
    if (param.skill_uuid as u32) & 0xf000_0000 != 0x6000_0000 {
        return Err(AutomarkerRequestDecodeError::InvalidMarkerSkillUuid {
            skill_uuid: param.skill_uuid,
        });
    }
    let attributes = decode_attributes(required(attr_data, MESSAGE, 4)?, scratch)?;
    if param.begin_time < 0 || attributes.timestamp != param.begin_time as u64 {
        return Err(AutomarkerRequestDecodeError::UnsupportedValue {
            field: 4,
            value: param.begin_time,
        });
    }
    Ok(ObservedAutomarkerRequest {
        marker_number,
        slot_id,
        skill_uuid: param.skill_uuid,
        skill_id: param.skill_id,
        skill_level: param.skill_level,
        begin_time: param.begin_time,
        target_position: param.target_position,
        current_position: param.current_position,
        session_sequence: required(session_sequence, MESSAGE, 5)?,
        attributes,
    })
}

fn decode_use_slot_current_position(
    raw: &[u8],
    scratch: &mut Vec<u8>,
) -> Result<ObservedUseSlotCurrentPosition, AutomarkerRequestDecodeError> {
    const MESSAGE: &str = "Zproto.UseSlotRequest";
    let mut cursor = 0;
    let mut slot_id = None;
    let mut use_type = None;
    let mut extra_data = None;
    let mut attr_data = None;
    let mut session_sequence = None;
    while cursor < raw.len() {
        let (field, wire) = tag(raw, &mut cursor, MESSAGE)?;
        match field {
            1 | 2 | 5 => {
                require_wire(MESSAGE, field, wire, 0)?;
                let value = varint(raw, &mut cursor, MESSAGE)?;
                match field {
                    1 => set_once(&mut slot_id, as_i32(value, MESSAGE, field)?, MESSAGE, field)?,
                    2 => set_once(
                        &mut use_type,
                        as_i32(value, MESSAGE, field)?,
                        MESSAGE,
                        field,
                    )?,
                    5 => set_once(
                        &mut session_sequence,
                        u32::try_from(value).map_err(|_| {
                            AutomarkerRequestDecodeError::UnsupportedValue {
                                field,
                                value: value as i64,
                            }
                        })?,
                        MESSAGE,
                        field,
                    )?,
                    _ => unreachable!(),
                }
            }
            3 | 4 => {
                require_wire(MESSAGE, field, wire, 2)?;
                let value = bytes(raw, &mut cursor, MESSAGE)?;
                if field == 3 {
                    set_once(&mut extra_data, value, MESSAGE, field)?;
                } else {
                    set_once(&mut attr_data, value, MESSAGE, field)?;
                }
            }
            _ => {
                return Err(AutomarkerRequestDecodeError::UnknownField {
                    message: MESSAGE,
                    field,
                });
            }
        }
    }
    let slot_id = required(slot_id, MESSAGE, 1)?;
    if slot_id <= 0 {
        return Err(AutomarkerRequestDecodeError::UnsupportedValue {
            field: 1,
            value: i64::from(slot_id),
        });
    }
    let use_type = required(use_type, MESSAGE, 2)?;
    if use_type != 1 {
        return Err(AutomarkerRequestDecodeError::UnsupportedValue {
            field: 2,
            value: i64::from(use_type),
        });
    }
    let (begin_time, current_position) =
        decode_use_slot_position_param(required(extra_data, MESSAGE, 3)?)?;
    let attributes = decode_attributes(required(attr_data, MESSAGE, 4)?, scratch)?;
    if begin_time < 0 || attributes.timestamp != begin_time as u64 {
        return Err(AutomarkerRequestDecodeError::UnsupportedValue {
            field: 4,
            value: begin_time,
        });
    }
    Ok(ObservedUseSlotCurrentPosition {
        current_position: position_xyz(current_position),
        session_sequence: required(session_sequence, MESSAGE, 5)?,
    })
}

fn decode_use_slot_position_param(
    raw: &[u8],
) -> Result<(i64, AutomarkerRequestPosition), AutomarkerRequestDecodeError> {
    const MESSAGE: &str = "Zproto.UseSkillParam";
    let mut cursor = 0;
    let mut skill_uuid = None;
    let mut skill_id = None;
    let mut begin_time = None;
    let mut current_position = None;
    let mut seen = [false; 12];
    while cursor < raw.len() {
        let (field, wire) = tag(raw, &mut cursor, MESSAGE)?;
        if !(1..=11).contains(&field) {
            return Err(AutomarkerRequestDecodeError::UnknownField {
                message: MESSAGE,
                field,
            });
        }
        if seen[field as usize] {
            return Err(AutomarkerRequestDecodeError::DuplicateField {
                message: MESSAGE,
                field,
            });
        }
        seen[field as usize] = true;
        match field {
            1 | 2 | 3 | 4 | 5 | 8 | 10 | 11 => {
                require_wire(MESSAGE, field, wire, 0)?;
                let value = varint(raw, &mut cursor, MESSAGE)?;
                match field {
                    1 => skill_uuid = Some(as_i32(value, MESSAGE, field)?),
                    2 => skill_id = Some(as_i32(value, MESSAGE, field)?),
                    4 => begin_time = Some(value as i64),
                    _ => {}
                }
            }
            6 | 7 | 9 => {
                require_wire(MESSAGE, field, wire, 2)?;
                let value = decode_position_with_proto_defaults(bytes(raw, &mut cursor, MESSAGE)?)?;
                if field == 7 {
                    current_position = Some(value);
                }
            }
            _ => unreachable!(),
        }
    }
    let uuid = required(skill_uuid, MESSAGE, 1)?;
    if uuid <= 0 {
        return Err(AutomarkerRequestDecodeError::UnsupportedValue {
            field: 1,
            value: i64::from(uuid),
        });
    }
    let skill = required(skill_id, MESSAGE, 2)?;
    if skill <= 0 {
        return Err(AutomarkerRequestDecodeError::UnsupportedValue {
            field: 2,
            value: i64::from(skill),
        });
    }
    Ok((
        required(begin_time, MESSAGE, 4)?,
        required(current_position, MESSAGE, 7)?,
    ))
}

fn decode_position_with_proto_defaults(
    raw: &[u8],
) -> Result<AutomarkerRequestPosition, AutomarkerRequestDecodeError> {
    const MESSAGE: &str = "Zproto.Position";
    let mut cursor = 0;
    let mut values = [None; 4];
    while cursor < raw.len() {
        let (field, wire) = tag(raw, &mut cursor, MESSAGE)?;
        if !(1..=4).contains(&field) {
            return Err(AutomarkerRequestDecodeError::UnknownField {
                message: MESSAGE,
                field,
            });
        }
        require_wire(MESSAGE, field, wire, 5)?;
        set_once(
            &mut values[field as usize - 1],
            fixed32(raw, &mut cursor, MESSAGE)?,
            MESSAGE,
            field,
        )?;
    }
    let position = AutomarkerRequestPosition {
        x: values[0].unwrap_or_default(),
        y: values[1].unwrap_or_default(),
        z: values[2].unwrap_or_default(),
        heading_degrees: values[3].unwrap_or_default(),
    };
    for (field, coordinate) in [(1, position.x), (2, position.y), (3, position.z)] {
        if !coordinate.is_finite() || coordinate.abs() > MAX_ABS_MARKER_COORDINATE {
            return Err(AutomarkerRequestDecodeError::InvalidPosition { field });
        }
    }
    if !position.heading_degrees.is_finite() {
        return Err(AutomarkerRequestDecodeError::InvalidPosition { field: 4 });
    }
    Ok(position)
}

#[derive(Clone, Copy)]
struct MarkerParam {
    skill_uuid: i32,
    skill_id: i32,
    skill_level: i32,
    begin_time: i64,
    target_position: AutomarkerRequestPosition,
    current_position: AutomarkerRequestPosition,
}

fn decode_param(raw: &[u8]) -> Result<MarkerParam, AutomarkerRequestDecodeError> {
    const MESSAGE: &str = "Zproto.UseSkillParam";
    let mut cursor = 0;
    let (mut uuid, mut skill, mut level, mut begin) = (None, None, None, None);
    let (mut target, mut current) = (None, None);
    let (mut passive, mut roulette) = (None, None);
    while cursor < raw.len() {
        let (field, wire) = tag(raw, &mut cursor, MESSAGE)?;
        match field {
            1..=3 => {
                require_wire(MESSAGE, field, wire, 0)?;
                let value = as_i32(varint(raw, &mut cursor, MESSAGE)?, MESSAGE, field)?;
                match field {
                    1 => set_once(&mut uuid, value, MESSAGE, field)?,
                    2 => set_once(&mut skill, value, MESSAGE, field)?,
                    3 => set_once(&mut level, value, MESSAGE, field)?,
                    _ => unreachable!(),
                }
            }
            4 => {
                require_wire(MESSAGE, field, wire, 0)?;
                set_once(
                    &mut begin,
                    varint(raw, &mut cursor, MESSAGE)? as i64,
                    MESSAGE,
                    field,
                )?;
            }
            6 | 7 => {
                require_wire(MESSAGE, field, wire, 2)?;
                let value = decode_position(bytes(raw, &mut cursor, MESSAGE)?)?;
                if field == 6 {
                    set_once(&mut target, value, MESSAGE, field)?;
                } else {
                    set_once(&mut current, value, MESSAGE, field)?;
                }
            }
            10 | 11 => {
                require_wire(MESSAGE, field, wire, 0)?;
                let value = varint(raw, &mut cursor, MESSAGE)?;
                if value > 1 {
                    return Err(AutomarkerRequestDecodeError::UnsupportedValue {
                        field,
                        value: value as i64,
                    });
                }
                if field == 10 {
                    set_once(&mut passive, value == 1, MESSAGE, field)?;
                } else {
                    set_once(&mut roulette, value == 1, MESSAGE, field)?;
                }
            }
            // Captured marker requests omit target UUID, target part, and target-part position.
            _ => {
                return Err(AutomarkerRequestDecodeError::UnknownField {
                    message: MESSAGE,
                    field,
                });
            }
        }
    }
    let level = required(level, MESSAGE, 3)?;
    if level != 1 {
        return Err(AutomarkerRequestDecodeError::UnsupportedValue {
            field: 3,
            value: i64::from(level),
        });
    }
    for (field, value) in [
        (10, required(passive, MESSAGE, 10)?),
        (11, required(roulette, MESSAGE, 11)?),
    ] {
        if !value {
            return Err(AutomarkerRequestDecodeError::UnsupportedValue { field, value: 0 });
        }
    }
    Ok(MarkerParam {
        skill_uuid: required(uuid, MESSAGE, 1)?,
        skill_id: required(skill, MESSAGE, 2)?,
        skill_level: level,
        begin_time: required(begin, MESSAGE, 4)?,
        target_position: required(target, MESSAGE, 6)?,
        current_position: required(current, MESSAGE, 7)?,
    })
}

fn marker_number(slot_id: i32, skill_id: i32) -> Result<u8, AutomarkerRequestDecodeError> {
    let from_slot = slot_id.checked_sub(200);
    let from_skill = skill_id.checked_sub(1100);
    if from_slot != from_skill || !matches!(from_slot, Some(1..=6)) {
        return Err(AutomarkerRequestDecodeError::InvalidMarkerIdentity { slot_id, skill_id });
    }
    Ok(from_slot.unwrap() as u8)
}

fn decode_position(raw: &[u8]) -> Result<AutomarkerRequestPosition, AutomarkerRequestDecodeError> {
    const MESSAGE: &str = "Zproto.Position";
    let mut cursor = 0;
    let mut values = [None; 4];
    while cursor < raw.len() {
        let (field, wire) = tag(raw, &mut cursor, MESSAGE)?;
        if !(1..=4).contains(&field) {
            return Err(AutomarkerRequestDecodeError::UnknownField {
                message: MESSAGE,
                field,
            });
        }
        require_wire(MESSAGE, field, wire, 5)?;
        set_once(
            &mut values[field as usize - 1],
            fixed32(raw, &mut cursor, MESSAGE)?,
            MESSAGE,
            field,
        )?;
    }
    let position = AutomarkerRequestPosition {
        x: required(values[0], MESSAGE, 1)?,
        y: required(values[1], MESSAGE, 2)?,
        z: required(values[2], MESSAGE, 3)?,
        heading_degrees: required(values[3], MESSAGE, 4)?,
    };
    for (field, coordinate) in [(1, position.x), (2, position.y), (3, position.z)] {
        if !coordinate.is_finite() || coordinate.abs() > MAX_ABS_MARKER_COORDINATE {
            return Err(AutomarkerRequestDecodeError::InvalidPosition { field });
        }
    }
    if !position.heading_degrees.is_finite() || !(0.0..360.0).contains(&position.heading_degrees) {
        return Err(AutomarkerRequestDecodeError::InvalidPosition { field: 4 });
    }
    Ok(position)
}

fn decode_attributes(
    envelope: &[u8],
    scratch: &mut Vec<u8>,
) -> Result<AutomarkerRequestAttributes, AutomarkerRequestDecodeError> {
    if envelope.len() < ENVELOPE_PREFIX_LENGTH + AES_BLOCK_LENGTH
        || (envelope.len() - ENVELOPE_PREFIX_LENGTH) % AES_BLOCK_LENGTH != 0
    {
        return Err(AutomarkerRequestDecodeError::InvalidEnvelopeLength {
            actual: envelope.len(),
        });
    }
    let (iv, rest) = envelope.split_at(IV_LENGTH);
    let (expected_mac, ciphertext) = rest.split_at(MAC_LENGTH);
    let mut mac = <HmacSha256 as Mac>::new_from_slice(&SKILL_HMAC_KEY).unwrap();
    mac.update(iv);
    mac.update(ciphertext);
    mac.verify_slice(expected_mac)
        .map_err(|_| AutomarkerRequestDecodeError::MacMismatch)?;
    scratch.clear();
    scratch.extend_from_slice(ciphertext);
    let length = match Aes128CbcDecryptor::new_from_slices(&SKILL_AES_KEY, iv)
        .unwrap()
        .decrypt_padded_mut::<Pkcs7>(scratch)
    {
        Ok(plaintext) => plaintext.len(),
        Err(_) => {
            scratch.fill(0);
            scratch.clear();
            return Err(AutomarkerRequestDecodeError::InvalidPadding);
        }
    };
    let result = decode_attribute_plaintext(&scratch[..length]);
    scratch.fill(0);
    scratch.clear();
    result
}

fn decode_attribute_plaintext(
    raw: &[u8],
) -> Result<AutomarkerRequestAttributes, AutomarkerRequestDecodeError> {
    const MESSAGE: &str = "Zproto.UseSkillAttrPlaintext";
    let mut cursor = 0;
    let (mut timestamp, mut velocity, mut attack, mut cast, mut charge, mut opaque) =
        (None, None, None, None, None, None);
    while cursor < raw.len() {
        let (field, wire) = tag(raw, &mut cursor, MESSAGE)?;
        match field {
            1 | 3 | 4 | 5 => {
                require_wire(MESSAGE, field, wire, 0)?;
                let value = varint(raw, &mut cursor, MESSAGE)?;
                match field {
                    1 => set_once(&mut timestamp, value, MESSAGE, field)?,
                    3 => set_once(&mut attack, as_i32(value, MESSAGE, field)?, MESSAGE, field)?,
                    4 => set_once(&mut cast, as_i32(value, MESSAGE, field)?, MESSAGE, field)?,
                    5 => set_once(&mut charge, as_i32(value, MESSAGE, field)?, MESSAGE, field)?,
                    _ => unreachable!(),
                }
            }
            2 | 6 => {
                require_wire(MESSAGE, field, wire, 5)?;
                let value = fixed32(raw, &mut cursor, MESSAGE)?;
                if field == 2 {
                    set_once(&mut velocity, value, MESSAGE, field)?;
                } else {
                    set_once(&mut opaque, value, MESSAGE, field)?;
                }
            }
            _ => {
                return Err(AutomarkerRequestDecodeError::UnknownField {
                    message: MESSAGE,
                    field,
                });
            }
        }
    }
    let opaque_current_build_scalar = required(opaque, MESSAGE, 6)?;
    if !opaque_current_build_scalar.is_finite() || opaque_current_build_scalar != 1.0 {
        return Err(AutomarkerRequestDecodeError::UnsupportedValue {
            field: 6,
            value: i64::from(opaque_current_build_scalar.to_bits()),
        });
    }
    Ok(AutomarkerRequestAttributes {
        timestamp: required(timestamp, MESSAGE, 1)?,
        velocity: required(velocity, MESSAGE, 2)?,
        attack_speed_pct: required(attack, MESSAGE, 3)?,
        cast_speed_pct: required(cast, MESSAGE, 4)?,
        charge_speed_pct: charge,
        opaque_current_build_scalar,
    })
}

fn one_message<'a>(
    raw: &'a [u8],
    message: &'static str,
    expected_field: u32,
) -> Result<&'a [u8], AutomarkerRequestDecodeError> {
    let mut cursor = 0;
    let mut value = None;
    while cursor < raw.len() {
        let (field, wire) = tag(raw, &mut cursor, message)?;
        if field != expected_field {
            return Err(AutomarkerRequestDecodeError::UnknownField { message, field });
        }
        require_wire(message, field, wire, 2)?;
        set_once(
            &mut value,
            bytes(raw, &mut cursor, message)?,
            message,
            field,
        )?;
    }
    required(value, message, expected_field)
}

fn tag(
    raw: &[u8],
    cursor: &mut usize,
    message: &'static str,
) -> Result<(u32, u8), AutomarkerRequestDecodeError> {
    let value = varint(raw, cursor, message)?;
    let field = u32::try_from(value >> 3)
        .map_err(|_| AutomarkerRequestDecodeError::InvalidTag { message })?;
    if field == 0 {
        return Err(AutomarkerRequestDecodeError::InvalidTag { message });
    }
    Ok((field, (value & 7) as u8))
}

fn varint(
    raw: &[u8],
    cursor: &mut usize,
    message: &'static str,
) -> Result<u64, AutomarkerRequestDecodeError> {
    let mut value = 0_u64;
    for shift in (0..70).step_by(7) {
        let byte = *raw
            .get(*cursor)
            .ok_or(AutomarkerRequestDecodeError::Truncated { message })?;
        *cursor += 1;
        if shift == 63 && byte > 1 {
            return Err(AutomarkerRequestDecodeError::VarintOverflow { message });
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(AutomarkerRequestDecodeError::VarintOverflow { message })
}

fn bytes<'a>(
    raw: &'a [u8],
    cursor: &mut usize,
    message: &'static str,
) -> Result<&'a [u8], AutomarkerRequestDecodeError> {
    let length = usize::try_from(varint(raw, cursor, message)?)
        .map_err(|_| AutomarkerRequestDecodeError::Truncated { message })?;
    let end = cursor
        .checked_add(length)
        .filter(|end| *end <= raw.len())
        .ok_or(AutomarkerRequestDecodeError::Truncated { message })?;
    let result = &raw[*cursor..end];
    *cursor = end;
    Ok(result)
}

fn fixed32(
    raw: &[u8],
    cursor: &mut usize,
    message: &'static str,
) -> Result<f32, AutomarkerRequestDecodeError> {
    let end = cursor
        .checked_add(4)
        .filter(|end| *end <= raw.len())
        .ok_or(AutomarkerRequestDecodeError::Truncated { message })?;
    let value = f32::from_bits(u32::from_le_bytes(raw[*cursor..end].try_into().unwrap()));
    *cursor = end;
    Ok(value)
}

fn require_wire(
    message: &'static str,
    field: u32,
    observed: u8,
    expected: u8,
) -> Result<(), AutomarkerRequestDecodeError> {
    if observed == expected {
        Ok(())
    } else {
        Err(AutomarkerRequestDecodeError::WrongWireType {
            message,
            field,
            observed,
            expected,
        })
    }
}

fn required<T>(
    value: Option<T>,
    message: &'static str,
    field: u32,
) -> Result<T, AutomarkerRequestDecodeError> {
    value.ok_or(AutomarkerRequestDecodeError::MissingField { message, field })
}

fn set_once<T>(
    slot: &mut Option<T>,
    value: T,
    message: &'static str,
    field: u32,
) -> Result<(), AutomarkerRequestDecodeError> {
    if slot.is_some() {
        Err(AutomarkerRequestDecodeError::DuplicateField { message, field })
    } else {
        *slot = Some(value);
        Ok(())
    }
}

fn as_i32(
    value: u64,
    message: &'static str,
    field: u32,
) -> Result<i32, AutomarkerRequestDecodeError> {
    if value <= i32::MAX as u64 || value >= u64::MAX - i32::MAX as u64 {
        Ok(value as i32)
    } else {
        Err(AutomarkerRequestDecodeError::Int32Overflow { message, field })
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    use super::*;
    use cbc::Encryptor;
    use cbc::cipher::{BlockEncryptMut, block_padding::Pkcs7};

    type Aes128CbcEncryptor = Encryptor<Aes128>;

    fn push_varint(out: &mut Vec<u8>, mut value: u64) {
        while value >= 0x80 {
            out.push((value as u8) | 0x80);
            value >>= 7;
        }
        out.push(value as u8);
    }

    fn push_v(out: &mut Vec<u8>, field: u8, value: u64) {
        out.push(field << 3);
        push_varint(out, value);
    }

    fn push_f(out: &mut Vec<u8>, field: u8, value: f32) {
        out.push((field << 3) | 5);
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn push_b(out: &mut Vec<u8>, field: u8, value: &[u8]) {
        out.push((field << 3) | 2);
        push_varint(out, value.len() as u64);
        out.extend_from_slice(value);
    }

    fn position(values: [f32; 4]) -> Vec<u8> {
        let mut out = Vec::new();
        for (index, value) in values.into_iter().enumerate() {
            push_f(&mut out, index as u8 + 1, value);
        }
        out
    }

    fn envelope(timestamp: u64) -> Vec<u8> {
        let mut plaintext = Vec::new();
        push_v(&mut plaintext, 1, timestamp);
        push_f(&mut plaintext, 2, 4.0);
        push_v(&mut plaintext, 3, 4493);
        push_v(&mut plaintext, 4, 4168);
        push_f(&mut plaintext, 6, 1.0);
        let iv = [0x5a; 16];
        let mut buffer = plaintext.clone();
        let original_len = buffer.len();
        buffer.resize(original_len + AES_BLOCK_LENGTH, 0);
        let ciphertext = Aes128CbcEncryptor::new_from_slices(&SKILL_AES_KEY, &iv)
            .unwrap()
            .encrypt_padded_mut::<Pkcs7>(&mut buffer, original_len)
            .unwrap();
        let mut mac = <HmacSha256 as Mac>::new_from_slice(&SKILL_HMAC_KEY).unwrap();
        mac.update(&iv);
        mac.update(ciphertext);
        let mut out = iv.to_vec();
        out.extend_from_slice(&mac.finalize().into_bytes());
        out.extend_from_slice(ciphertext);
        out
    }

    pub(crate) fn synthetic_frame_for_adapter() -> Vec<u8> {
        let marker = 1_u8;
        let begin = 1_789_176_498_286_u64;
        let mut param = Vec::new();
        push_v(&mut param, 1, 1_610_614_694);
        push_v(&mut param, 2, 1100 + u64::from(marker));
        push_v(&mut param, 3, 1);
        push_v(&mut param, 4, begin);
        push_b(&mut param, 6, &position([1.0, 2.0, 3.0, 4.0]));
        push_b(&mut param, 7, &position([5.0, 6.0, 7.0, 8.0]));
        push_v(&mut param, 10, 1);
        push_v(&mut param, 11, 1);
        let mut inner = Vec::new();
        push_v(&mut inner, 1, 200 + u64::from(marker));
        push_v(&mut inner, 2, 1);
        push_b(&mut inner, 3, &param);
        push_b(&mut inner, 4, &envelope(begin));
        push_v(&mut inner, 5, 607);
        let mut application = Vec::new();
        push_b(&mut application, 1, &inner);
        assert_eq!(application.len(), OBSERVED_APPLICATION_LENGTH);

        let mut nested = Vec::with_capacity(OBSERVED_NESTED_CALL_LENGTH);
        nested.extend_from_slice(&(OBSERVED_NESTED_CALL_LENGTH as u32).to_be_bytes());
        nested.extend_from_slice(&CALL_FRAGMENT.to_be_bytes());
        nested.extend_from_slice(&WORLD_SERVICE_ID.to_be_bytes());
        nested.extend_from_slice(&WORLD_STUB_ID.to_be_bytes());
        nested.extend_from_slice(&0x1234_5678_u32.to_be_bytes());
        nested.extend_from_slice(&USE_SLOT_METHOD_ID.to_be_bytes());
        nested.extend_from_slice(&application);
        assert_eq!(nested.len(), OBSERVED_NESTED_CALL_LENGTH);
        let mut outer = Vec::with_capacity(OBSERVED_FRAME_UP_LENGTH);
        outer.extend_from_slice(&(OBSERVED_FRAME_UP_LENGTH as u32).to_be_bytes());
        outer.extend_from_slice(&FRAME_UP_FRAGMENT.to_be_bytes());
        outer.extend_from_slice(&0x9abc_def0_u32.to_be_bytes());
        outer.extend_from_slice(&nested);
        assert_eq!(outer.len(), OBSERVED_FRAME_UP_LENGTH);
        outer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cbc::Encryptor;
    use cbc::cipher::{BlockEncryptMut, block_padding::Pkcs7};

    type Aes128CbcEncryptor = Encryptor<Aes128>;

    fn source_pack() -> ProtocolPack {
        ProtocolPack::from_json(include_bytes!(
            "../protocol-packs/global/steam-24687926/pack.json"
        ))
        .unwrap()
    }

    fn current_pack(build: &str) -> ProtocolPack {
        crate::compatibility_epoch::retarget_protocol_pack(
            &source_pack(),
            "compatibility-fallback",
            "global",
            "steam",
            build,
        )
        .unwrap()
    }

    fn push_varint(out: &mut Vec<u8>, mut value: u64) {
        while value >= 0x80 {
            out.push((value as u8) | 0x80);
            value >>= 7;
        }
        out.push(value as u8);
    }

    fn push_v(out: &mut Vec<u8>, field: u8, value: u64) {
        out.push(field << 3);
        push_varint(out, value);
    }

    fn push_f(out: &mut Vec<u8>, field: u8, value: f32) {
        out.push((field << 3) | 5);
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn push_b(out: &mut Vec<u8>, field: u8, value: &[u8]) {
        out.push((field << 3) | 2);
        push_varint(out, value.len() as u64);
        out.extend_from_slice(value);
    }

    fn position(value: [f32; 4]) -> Vec<u8> {
        let mut out = vec![];
        for (index, value) in value.into_iter().enumerate() {
            push_f(&mut out, index as u8 + 1, value);
        }
        out
    }

    fn envelope(timestamp: u64, opaque: f32) -> Vec<u8> {
        let mut plaintext = vec![];
        push_v(&mut plaintext, 1, timestamp);
        push_f(&mut plaintext, 2, 4.0);
        push_v(&mut plaintext, 3, 4493);
        push_v(&mut plaintext, 4, 4168);
        push_f(&mut plaintext, 6, opaque);
        let iv = [0x5a; 16];
        let mut buffer = plaintext.clone();
        let original = buffer.len();
        buffer.resize(original + 16, 0);
        let ciphertext = Aes128CbcEncryptor::new_from_slices(&SKILL_AES_KEY, &iv)
            .unwrap()
            .encrypt_padded_mut::<Pkcs7>(&mut buffer, original)
            .unwrap();
        let mut mac = <HmacSha256 as Mac>::new_from_slice(&SKILL_HMAC_KEY).unwrap();
        mac.update(&iv);
        mac.update(ciphertext);
        let mut out = iv.to_vec();
        out.extend_from_slice(&mac.finalize().into_bytes());
        out.extend_from_slice(ciphertext);
        out
    }

    fn request(
        marker: u8,
        target: [f32; 4],
        current: [f32; 4],
        begin: u64,
        counter: u64,
    ) -> Vec<u8> {
        request_with(
            marker,
            target,
            current,
            begin,
            counter,
            1_610_614_693 + u64::from(marker),
            1.0,
        )
    }

    fn request_with(
        marker: u8,
        target: [f32; 4],
        current: [f32; 4],
        begin: u64,
        counter: u64,
        skill_uuid: u64,
        current_build_scalar: f32,
    ) -> Vec<u8> {
        let mut param = vec![];
        push_v(&mut param, 1, skill_uuid);
        push_v(&mut param, 2, 1100 + u64::from(marker));
        push_v(&mut param, 3, 1);
        push_v(&mut param, 4, begin);
        push_b(&mut param, 6, &position(target));
        push_b(&mut param, 7, &position(current));
        push_v(&mut param, 10, 1);
        push_v(&mut param, 11, 1);
        let mut inner = vec![];
        push_v(&mut inner, 1, 200 + u64::from(marker));
        push_v(&mut inner, 2, 1);
        push_b(&mut inner, 3, &param);
        push_b(&mut inner, 4, &envelope(begin, current_build_scalar));
        push_v(&mut inner, 5, counter);
        let mut outer = vec![];
        push_b(&mut outer, 1, &inner);
        outer
    }

    fn observed_frame(application: &[u8]) -> Vec<u8> {
        let mut nested = Vec::with_capacity(OBSERVED_NESTED_CALL_LENGTH);
        nested.extend_from_slice(&(OBSERVED_NESTED_CALL_LENGTH as u32).to_be_bytes());
        nested.extend_from_slice(&CALL_FRAGMENT.to_be_bytes());
        nested.extend_from_slice(&WORLD_SERVICE_ID.to_be_bytes());
        nested.extend_from_slice(&WORLD_STUB_ID.to_be_bytes());
        nested.extend_from_slice(&0x1234_5678_u32.to_be_bytes());
        nested.extend_from_slice(&USE_SLOT_METHOD_ID.to_be_bytes());
        nested.extend_from_slice(application);
        assert_eq!(nested.len(), OBSERVED_NESTED_CALL_LENGTH);

        let mut outer = Vec::with_capacity(OBSERVED_FRAME_UP_LENGTH);
        outer.extend_from_slice(&(OBSERVED_FRAME_UP_LENGTH as u32).to_be_bytes());
        outer.extend_from_slice(&FRAME_UP_FRAGMENT.to_be_bytes());
        outer.extend_from_slice(&0x9abc_def0_u32.to_be_bytes());
        outer.extend_from_slice(&nested);
        assert_eq!(outer.len(), OBSERVED_FRAME_UP_LENGTH);
        outer
    }

    fn assert_original_unchanged(
        outcome: OfflineAutomarkerFrameSubstitution,
        original: &[u8],
        expected: impl FnOnce(&OfflineAutomarkerSubstitutionError) -> bool,
    ) {
        match outcome {
            OfflineAutomarkerFrameSubstitution::OriginalUnchanged { frame, reason } => {
                assert_eq!(frame, original);
                assert!(expected(&reason), "unexpected rejection: {reason:?}");
            }
            OfflineAutomarkerFrameSubstitution::Substituted { .. } => {
                panic!("invalid frame was substituted")
            }
        }
    }

    #[test]
    fn exact_gate_rejects_neighbor_build_and_wrong_digest() {
        let current = current_pack(AUTOMARKER_REQUEST_BUILD);
        assert_eq!(current.digest(), AUTOMARKER_REQUEST_PACK_DIGEST);
        assert!(supports_observed_automarker_requests(&current));
        assert!(!supports_observed_automarker_requests(&current_pack(
            "25247557"
        )));
        let mut definition = current.definition().clone();
        definition.pack_id.push_str("-different");
        assert!(!supports_observed_automarker_requests(
            &ProtocolPack::build(definition).unwrap()
        ));
    }

    #[test]
    fn late_attach_signature_finds_only_an_authenticated_complete_marker_frame() {
        let application = request(
            1,
            [250.35721, 118.0, -64.2384, 250.49268],
            [250.44351, 118.02, -61.48509, 250.49268],
            1_789_176_498_286,
            607,
        );
        let frame = observed_frame(&application);
        let mut prefix = b"unrelated earlier stream bytes".to_vec();
        prefix.extend_from_slice(&frame);
        assert_eq!(
            classify_observed_automarker_tcp_prefix(&prefix),
            TcpPayloadSignatureResult::Match(TcpPayloadDirection::ClientToServer)
        );
        assert_eq!(
            classify_observed_automarker_tcp_prefix(&prefix[..prefix.len() - 1]),
            TcpPayloadSignatureResult::NeedMore
        );

        let envelope = substitution_spans(&application)
            .unwrap()
            .authenticated_envelope;
        let mut unauthenticated = frame;
        unauthenticated[36 + envelope.start + IV_LENGTH] ^= 1;
        assert_eq!(
            classify_observed_automarker_tcp_prefix(&unauthenticated),
            TcpPayloadSignatureResult::NeedMore
        );
        unauthenticated.resize(MAX_TCP_SIGNATURE_PREFIX_BYTES, 0);
        assert_eq!(
            classify_observed_automarker_tcp_prefix(&unauthenticated),
            TcpPayloadSignatureResult::Reject
        );
    }

    #[test]
    fn decodes_all_six_observed_marker_identities_and_positions() {
        let samples = [
            (
                1,
                [250.35721, 118.0, -64.2384, 250.49268],
                [250.44351, 118.02, -61.48509, 250.49268],
                1_789_176_498_286,
                607,
            ),
            (
                2,
                [246.87148, 118.0, -64.09218, 270.9394],
                [246.95949, 118.02, -61.450314, 270.9394],
                1_789_176_505_357,
                696,
            ),
            (
                3,
                [242.48721, 118.12358, -63.51843, 271.98438],
                [242.60005, 118.02, -61.302887, 271.98438],
                1_789_176_510_696,
                762,
            ),
            (
                4,
                [238.84279, 118.0, -63.759544, 271.9853],
                [238.9814, 118.02, -61.174927, 271.9853],
                1_789_176_515_414,
                822,
            ),
            (
                5,
                [235.98499, 118.03505, -63.687, 273.0794],
                [235.28726, 118.02, -60.974487, 273.0794],
                1_789_176_517_633,
                850,
            ),
            (
                6,
                [232.51979, 118.01601, -63.925617, 273.08215],
                [232.34154, 118.02, -60.81542, 273.08215],
                1_789_176_520_665,
                887,
            ),
        ];
        let pack = current_pack(AUTOMARKER_REQUEST_BUILD);
        let mut scratch = vec![];
        for (marker, target, current, begin, counter) in samples {
            let decoded = decode_observed_automarker_request_into(
                &pack,
                &request(marker, target, current, begin, counter),
                &mut scratch,
            )
            .unwrap();
            assert_eq!(decoded.marker_number, marker);
            assert_eq!(decoded.slot_id, 200 + i32::from(marker));
            assert_eq!(decoded.skill_id, 1100 + i32::from(marker));
            assert_eq!(decoded.skill_uuid, 1_610_614_693 + i32::from(marker));
            assert_eq!(
                decoded.target_position,
                AutomarkerRequestPosition {
                    x: target[0],
                    y: target[1],
                    z: target[2],
                    heading_degrees: target[3]
                }
            );
            assert_eq!(
                decoded.current_position,
                AutomarkerRequestPosition {
                    x: current[0],
                    y: current[1],
                    z: current[2],
                    heading_degrees: current[3]
                }
            );
            assert_eq!(decoded.session_sequence, counter as u32);
            assert_eq!(decoded.attributes.timestamp, begin);
            assert_eq!(decoded.attributes.opaque_current_build_scalar, 1.0);
            assert_eq!(decoded.attributes.charge_speed_pct, None);
            assert!(scratch.is_empty());
        }
    }

    #[test]
    fn position_only_decoder_projects_sanitized_current_xyz_and_sequence() {
        let pack = current_pack(AUTOMARKER_REQUEST_BUILD);
        let payload = request(
            1,
            [250.35721, 118.0, -64.2384, 250.49268],
            [250.44351, 118.02, -61.48509, 250.49268],
            1_789_176_498_286,
            607,
        );
        let mut scratch = Vec::new();
        let observed =
            decode_observed_use_slot_current_position_into(&pack, &payload, &mut scratch).unwrap();
        assert_eq!(
            observed,
            ObservedUseSlotCurrentPosition {
                current_position: AutomarkerRequestXyz {
                    x: 250.44351,
                    y: 118.02,
                    z: -61.48509,
                },
                session_sequence: 607,
            }
        );
        assert!(scratch.is_empty());
    }

    #[test]
    fn position_only_decoder_keeps_exact_identity_and_authentication_gates() {
        let payload = request(
            1,
            [1.0, 2.0, 3.0, 4.0],
            [5.0, 6.0, 7.0, 8.0],
            1_789_176_498_286,
            607,
        );
        let mut scratch = Vec::new();
        assert!(matches!(
            decode_observed_use_slot_current_position_into(
                &current_pack("25247557"),
                &payload,
                &mut scratch,
            ),
            Err(AutomarkerRequestDecodeError::UnsupportedProtocolIdentity)
        ));

        let mut tampered = payload;
        let iv = tampered
            .windows(16)
            .position(|window| window == [0x5a; 16])
            .unwrap();
        tampered[iv] ^= 1;
        assert!(matches!(
            decode_observed_use_slot_current_position_into(
                &current_pack(AUTOMARKER_REQUEST_BUILD),
                &tampered,
                &mut scratch,
            ),
            Err(AutomarkerRequestDecodeError::MacMismatch)
        ));
    }

    #[test]
    fn observed_marker_and_fixed32_changes_preserve_only_the_protobuf_length() {
        let samples = [
            (1, 1_789_176_498_286, 607),
            (2, 1_789_176_505_357, 696),
            (3, 1_789_176_510_696, 762),
            (4, 1_789_176_515_414, 822),
            (5, 1_789_176_517_633, 850),
            (6, 1_789_176_520_665, 887),
        ];
        for (marker, begin, counter) in samples {
            let original = request(
                marker,
                [250.35721, 118.0, -64.2384, 250.49268],
                [250.44351, 118.02, -61.48509, 250.49268],
                begin,
                counter,
            );
            let changed_fixed32 = request(
                marker,
                [-999_999.0, 0.125, 999_999.0, 359.999],
                [-1.0, -2.0, -3.0, 0.0],
                begin,
                counter,
            );
            assert_eq!(original.len(), 161);
            assert_eq!(changed_fixed32.len(), 161);
        }

        // This proves only protobuf width stability. It deliberately says
        // nothing about zstd size, TCP layout, server acceptance, or sending.
        let proof: serde_json::Value = serde_json::from_str(include_str!(
            "../research/game-file-inventory/global/steam-25247556/ground-marker-packet-substitution-feasibility.v1.json"
        ))
        .unwrap();
        assert_eq!(
            proof["protobuf_wire_layout"]["derived_observed_lengths"]["world_types_use_slot_bytes"],
            161
        );
        assert_eq!(
            proof["length_preservation"]["complete_wire_length_preservation_proven_for_observed_request"],
            true
        );
        assert_eq!(
            proof["length_preservation"]["complete_wire_length_preservation_all_requests_proven"],
            false
        );
        assert_eq!(proof["conclusion"]["runtime_sender_enabled"], false);
        assert_eq!(
            proof["conclusion"]["permission_to_replay_inject_or_rewrite"],
            false
        );
    }

    #[test]
    fn offline_target_xyz_substitution_preserves_heading_and_current_position() {
        let pack = current_pack(AUTOMARKER_REQUEST_BUILD);
        let original = request(
            1,
            [250.35721, 118.0, -64.2384, 250.49268],
            [250.44351, 118.02, -61.48509, 250.49268],
            1_789_176_498_286,
            607,
        );
        let target = AutomarkerRequestXyz {
            x: -999_999.0,
            y: 0.125,
            z: 999_999.0,
        };

        // The caller has no heading or current-position input. Exercise every
        // valid paired marker identity while those carrier values stay fixed.
        for replacement_marker in 1..=6 {
            let proof = verify_offline_automarker_substitution(
                &pack,
                &original,
                replacement_marker,
                target,
            )
            .unwrap();

            assert_eq!(proof.original_application_length_bytes, 161);
            assert_eq!(proof.substituted_application_length_bytes, 161);
            assert_eq!(proof.nested_call_length_bytes, 187);
            assert_eq!(proof.outer_frame_up_length_bytes, 197);
            assert_eq!(proof.allowed_mutable_bytes, 16);
            assert!(proof.all_other_bytes_identical);
            assert!(proof.game_owned_values_identical);
            assert!(proof.authenticated_envelope_bytes_identical);
            assert!(proof.target_heading_bytes_identical);
            assert!(proof.current_position_bytes_identical);
            assert!(proof.exact_build_decode_succeeded);
            assert!(!proof.packet_transmission_performed);
        }
    }

    #[test]
    fn offline_substitution_verifier_keeps_exact_build_and_length_gates() {
        let current = current_pack(AUTOMARKER_REQUEST_BUILD);
        let neighbor = current_pack("25247557");
        let original = request(
            1,
            [1.0, 2.0, 3.0, 4.0],
            [5.0, 6.0, 7.0, 8.0],
            1_789_176_498_286,
            607,
        );
        let target = AutomarkerRequestXyz {
            x: 9.0,
            y: 10.0,
            z: 11.0,
        };

        assert!(matches!(
            verify_offline_automarker_substitution(&neighbor, &original, 2, target),
            Err(OfflineAutomarkerSubstitutionError::Decode(
                AutomarkerRequestDecodeError::UnsupportedProtocolIdentity
            ))
        ));
        assert!(matches!(
            verify_offline_automarker_substitution(&current, &original[..160], 2, target),
            Err(OfflineAutomarkerSubstitutionError::UnexpectedApplicationLength { actual: 160 })
        ));
        assert!(matches!(
            verify_offline_automarker_substitution(&current, &original, 7, target),
            Err(OfflineAutomarkerSubstitutionError::InvalidReplacementMarker)
        ));
    }

    #[test]
    fn offline_frame_core_substitutes_all_six_markers_with_exact_byte_allowlist() {
        let pack = current_pack(AUTOMARKER_REQUEST_BUILD);
        let application_start = FRAME_HEADER_LENGTH
            + FRAME_UP_PREFIX_LENGTH
            + FRAME_HEADER_LENGTH
            + CALL_ROUTE_HEADER_LENGTH;
        let target = AutomarkerRequestXyz {
            x: -321.25,
            y: 42.5,
            z: 765.125,
        };

        for replacement_marker in 1..=6 {
            let carrier_marker = replacement_marker % 6 + 1;
            let application = request(
                carrier_marker,
                [250.35721, 118.0, -64.2384, 250.49268],
                [250.44351, 118.02, -61.48509, 250.49268],
                1_789_176_498_286 + u64::from(replacement_marker),
                607 + u64::from(replacement_marker),
            );
            let original = observed_frame(&application);
            let local_spans = substitution_spans(&application).unwrap();
            let allowed: Vec<usize> = local_spans
                .mutable_ranges()
                .flat_map(|range| {
                    (application_start + range.start)..(application_start + range.end)
                })
                .collect();

            let outcome =
                substitute_offline_automarker_frame(&pack, &original, replacement_marker, target);
            let (substituted, proof) = match outcome {
                OfflineAutomarkerFrameSubstitution::Substituted { frame, proof } => (frame, proof),
                OfflineAutomarkerFrameSubstitution::OriginalUnchanged { reason, .. } => {
                    panic!("valid marker {replacement_marker} was rejected: {reason:?}")
                }
            };
            assert_eq!(substituted.len(), original.len());
            assert_eq!(
                &substituted[..application_start],
                &original[..application_start]
            );
            assert_eq!(proof.allowed_mutable_bytes, 16);
            assert!(!proof.packet_transmission_performed);

            let changed: Vec<usize> = original
                .iter()
                .zip(&substituted)
                .enumerate()
                .filter_map(|(index, (before, after))| (before != after).then_some(index))
                .collect();
            assert!(!changed.is_empty());
            assert!(changed.iter().all(|index| allowed.contains(index)));
            assert!(allowed.iter().all(|index| {
                let local = index - application_start;
                local_spans.slot_id.contains(&local)
                    || local_spans.skill_id.contains(&local)
                    || local_spans
                        .target_position
                        .iter()
                        .take(3)
                        .any(|range| range.contains(&local))
            }));

            let decoded = decode_observed_automarker_request_into(
                &pack,
                &substituted[application_start..],
                &mut Vec::new(),
            )
            .unwrap();
            assert_eq!(decoded.marker_number, replacement_marker);
            assert_eq!(position_xyz(decoded.target_position), target);
            assert_eq!(
                position_xyz(decoded.current_position),
                AutomarkerRequestXyz {
                    x: 250.44351,
                    y: 118.02,
                    z: -61.48509,
                }
            );
            assert_eq!(decoded.target_position.heading_degrees, 250.49268);
        }
    }

    #[test]
    fn offline_frame_core_rejects_malformed_compressed_wrong_route_and_wrong_auth_unchanged() {
        let pack = current_pack(AUTOMARKER_REQUEST_BUILD);
        let application = request(
            1,
            [1.0, 2.0, 3.0, 4.0],
            [5.0, 6.0, 7.0, 8.0],
            1_789_176_498_286,
            607,
        );
        let original = observed_frame(&application);
        let target = AutomarkerRequestXyz {
            x: 9.0,
            y: 10.0,
            z: 11.0,
        };

        let truncated = &original[..original.len() - 1];
        assert_original_unchanged(
            substitute_offline_automarker_frame(&pack, truncated, 2, target),
            truncated,
            |reason| {
                matches!(
                    reason,
                    OfflineAutomarkerSubstitutionError::UnexpectedFrameLength {
                        layer: "outer FrameUp",
                        ..
                    }
                )
            },
        );

        let mut wrong_declared_length = original.clone();
        wrong_declared_length[..4].copy_from_slice(&196_u32.to_be_bytes());
        assert_original_unchanged(
            substitute_offline_automarker_frame(&pack, &wrong_declared_length, 2, target),
            &wrong_declared_length,
            |reason| {
                matches!(
                    reason,
                    OfflineAutomarkerSubstitutionError::DeclaredFrameLengthMismatch {
                        layer: "outer FrameUp",
                        ..
                    }
                )
            },
        );

        for (offset, layer) in [(4, "outer FrameUp"), (10 + 4, "nested Call")] {
            let mut compressed = original.clone();
            let fragment = u16::from_be_bytes(compressed[offset..offset + 2].try_into().unwrap());
            compressed[offset..offset + 2]
                .copy_from_slice(&(fragment | COMPRESSION_FLAG).to_be_bytes());
            assert_original_unchanged(
                substitute_offline_automarker_frame(&pack, &compressed, 2, target),
                &compressed,
                |reason| {
                    matches!(
                        reason,
                        OfflineAutomarkerSubstitutionError::CompressedFrame { layer: actual }
                            if *actual == layer
                    )
                },
            );
        }

        let mut wrong_fragment = original.clone();
        wrong_fragment[14..16].copy_from_slice(&2_u16.to_be_bytes());
        assert_original_unchanged(
            substitute_offline_automarker_frame(&pack, &wrong_fragment, 2, target),
            &wrong_fragment,
            |reason| {
                matches!(
                    reason,
                    OfflineAutomarkerSubstitutionError::UnexpectedFragment {
                        layer: "nested Call",
                        ..
                    }
                )
            },
        );

        let mut wrong_route = original.clone();
        let route_start = 10 + FRAME_HEADER_LENGTH;
        wrong_route[route_start..route_start + 8]
            .copy_from_slice(&(WORLD_SERVICE_ID + 1).to_be_bytes());
        assert_original_unchanged(
            substitute_offline_automarker_frame(&pack, &wrong_route, 2, target),
            &wrong_route,
            |reason| {
                matches!(
                    reason,
                    OfflineAutomarkerSubstitutionError::UnexpectedRoute { .. }
                )
            },
        );

        let application_start = 10 + FRAME_HEADER_LENGTH + CALL_ROUTE_HEADER_LENGTH;
        let spans = substitution_spans(&application).unwrap();
        let mut wrong_auth = original.clone();
        wrong_auth[application_start + spans.authenticated_envelope.start + IV_LENGTH] ^= 1;
        assert_original_unchanged(
            substitute_offline_automarker_frame(&pack, &wrong_auth, 2, target),
            &wrong_auth,
            |reason| {
                matches!(
                    reason,
                    OfflineAutomarkerSubstitutionError::Decode(
                        AutomarkerRequestDecodeError::MacMismatch
                    )
                )
            },
        );
    }

    #[test]
    fn offline_frame_core_keeps_protocol_identity_and_replacement_domain_fail_closed() {
        let current = current_pack(AUTOMARKER_REQUEST_BUILD);
        let neighbor = current_pack("25247557");
        let original = observed_frame(&request(
            1,
            [1.0, 2.0, 3.0, 4.0],
            [5.0, 6.0, 7.0, 8.0],
            1_789_176_498_286,
            607,
        ));
        let target = AutomarkerRequestXyz {
            x: 9.0,
            y: 10.0,
            z: 11.0,
        };

        assert_original_unchanged(
            substitute_offline_automarker_frame(&neighbor, &original, 2, target),
            &original,
            |reason| {
                matches!(
                    reason,
                    OfflineAutomarkerSubstitutionError::Decode(
                        AutomarkerRequestDecodeError::UnsupportedProtocolIdentity
                    )
                )
            },
        );
        assert_original_unchanged(
            substitute_offline_automarker_frame(&current, &original, 0, target),
            &original,
            |reason| {
                matches!(
                    reason,
                    OfflineAutomarkerSubstitutionError::InvalidReplacementMarker
                )
            },
        );
        assert_original_unchanged(
            substitute_offline_automarker_frame(
                &current,
                &original,
                2,
                AutomarkerRequestXyz {
                    x: f32::NAN,
                    y: 10.0,
                    z: 11.0,
                },
            ),
            &original,
            |reason| {
                matches!(
                    reason,
                    OfflineAutomarkerSubstitutionError::Decode(
                        AutomarkerRequestDecodeError::InvalidPosition { field: 1 }
                    )
                )
            },
        );
    }

    fn replacement_frame(
        pack: &ProtocolPack,
        original: &[u8],
        marker: u8,
        target: AutomarkerRequestXyz,
    ) -> Vec<u8> {
        match substitute_offline_automarker_frame(pack, original, marker, target) {
            OfflineAutomarkerFrameSubstitution::Substituted { frame, .. } => frame,
            OfflineAutomarkerFrameSubstitution::OriginalUnchanged { reason, .. } => {
                panic!("test carrier rejected: {reason:?}")
            }
        }
    }

    fn assert_segment_rewritten(
        outcome: OfflineAutomarkerTcpSegmentResult,
        expected: &[u8],
    ) -> (usize, usize) {
        match outcome {
            OfflineAutomarkerTcpSegmentResult::Rewritten {
                payload,
                overlapped_bytes,
                operations_touched,
                ..
            } => {
                assert_eq!(payload, expected);
                (overlapped_bytes, operations_touched)
            }
            OfflineAutomarkerTcpSegmentResult::OriginalUnchanged { reason, .. } => {
                panic!("expected rewritten segment, got {reason:?}")
            }
        }
    }

    #[test]
    fn tcp_ledger_rewrites_segmented_coalesced_out_of_order_and_retransmitted_bytes() {
        let pack = current_pack(AUTOMARKER_REQUEST_BUILD);
        let original = observed_frame(&request(
            1,
            [1.0, 2.0, 3.0, 4.0],
            [5.0, 6.0, 7.0, 8.0],
            1_789_176_498_286,
            607,
        ));
        let target = AutomarkerRequestXyz {
            x: -100.25,
            y: 200.5,
            z: -300.75,
        };
        let replacement = replacement_frame(&pack, &original, 6, target);
        let epoch = 41;
        let base = 10_000_u32;
        let mut ledger = OfflineAutomarkerTcpRewriteLedger::new(epoch);
        assert!(matches!(
            ledger.arm_frame(epoch, base, &pack, &original, 6, target),
            OfflineAutomarkerTcpArmResult::Armed {
                frame_length_bytes: OBSERVED_FRAME_UP_LENGTH,
                ..
            }
        ));

        // Coalesced bytes on both sides are conserved while the whole frame is
        // rewritten at its exact offset.
        let mut coalesced_original = vec![0xa5; 13];
        coalesced_original.extend_from_slice(&original);
        coalesced_original.extend_from_slice(&[0x5a; 17]);
        let mut coalesced_expected = vec![0xa5; 13];
        coalesced_expected.extend_from_slice(&replacement);
        coalesced_expected.extend_from_slice(&[0x5a; 17]);
        let (overlap, touched) = assert_segment_rewritten(
            ledger.rewrite_segment(epoch, base.wrapping_sub(13), &coalesced_original),
            &coalesced_expected,
        );
        assert_eq!(overlap, original.len());
        assert_eq!(touched, 1);

        // Arbitrary splits can arrive out of order. Every segment is derived
        // from the same immutable replacement, so retransmission is identical.
        let ranges = [0..1, 1..5, 5..36, 36..91, 91..196, 196..197];
        for index in [4, 1, 5, 0, 3, 2] {
            let range = ranges[index].clone();
            let outcome = ledger.rewrite_segment(
                epoch,
                base.wrapping_add(range.start as u32),
                &original[range.clone()],
            );
            assert_segment_rewritten(outcome, &replacement[range]);
        }
        for range in [20..120, 73..173, 20..120] {
            let outcome = ledger.rewrite_segment(
                epoch,
                base.wrapping_add(range.start as u32),
                &original[range.clone()],
            );
            assert_segment_rewritten(outcome, &replacement[range]);
        }

        // Exhaust every internal two-segment boundary in both arrival orders,
        // then every one-byte retransmission offset.
        for split in 1..original.len() {
            for range in [split..original.len(), 0..split] {
                assert_segment_rewritten(
                    ledger.rewrite_segment(
                        epoch,
                        base.wrapping_add(range.start as u32),
                        &original[range.clone()],
                    ),
                    &replacement[range],
                );
            }
        }
        for offset in 0..original.len() {
            assert_segment_rewritten(
                ledger.rewrite_segment(
                    epoch,
                    base.wrapping_add(offset as u32),
                    &original[offset..offset + 1],
                ),
                &replacement[offset..offset + 1],
            );
        }
    }

    #[test]
    fn tcp_ledger_rewrites_multiple_operations_and_conserves_every_nonoperation_byte() {
        let pack = current_pack(AUTOMARKER_REQUEST_BUILD);
        let first = observed_frame(&request(
            1,
            [1.0, 2.0, 3.0, 4.0],
            [5.0, 6.0, 7.0, 8.0],
            1_789_176_498_286,
            607,
        ));
        let second = observed_frame(&request(
            2,
            [11.0, 12.0, 13.0, 14.0],
            [15.0, 16.0, 17.0, 18.0],
            1_789_176_505_357,
            696,
        ));
        let first_target = AutomarkerRequestXyz {
            x: 101.0,
            y: 102.0,
            z: 103.0,
        };
        let second_target = AutomarkerRequestXyz {
            x: 201.0,
            y: 202.0,
            z: 203.0,
        };
        let first_replacement = replacement_frame(&pack, &first, 5, first_target);
        let second_replacement = replacement_frame(&pack, &second, 6, second_target);
        let epoch = 7;
        let first_base = 50_000_u32;
        let second_base = first_base.wrapping_add(300);
        let mut ledger = OfflineAutomarkerTcpRewriteLedger::new(epoch);
        assert!(matches!(
            ledger.arm_frame(epoch, first_base, &pack, &first, 5, first_target),
            OfflineAutomarkerTcpArmResult::Armed { .. }
        ));
        assert!(matches!(
            ledger.arm_frame(epoch, second_base, &pack, &second, 6, second_target),
            OfflineAutomarkerTcpArmResult::Armed { .. }
        ));

        let mut carrier = vec![0xcc; 25];
        carrier.extend_from_slice(&first);
        carrier.extend_from_slice(&[0xdd; 103]);
        carrier.extend_from_slice(&second);
        carrier.extend_from_slice(&[0xee; 31]);
        let mut expected = vec![0xcc; 25];
        expected.extend_from_slice(&first_replacement);
        expected.extend_from_slice(&[0xdd; 103]);
        expected.extend_from_slice(&second_replacement);
        expected.extend_from_slice(&[0xee; 31]);
        let (overlap, touched) = assert_segment_rewritten(
            ledger.rewrite_segment(epoch, first_base.wrapping_sub(25), &carrier),
            &expected,
        );
        assert_eq!(overlap, first.len() + second.len());
        assert_eq!(touched, 2);
        assert_eq!(&expected[..25], &carrier[..25]);
        assert_eq!(
            &expected[25 + first.len()..25 + first.len() + 103],
            &[0xdd; 103]
        );
        assert_eq!(&expected[expected.len() - 31..], &[0xee; 31]);

        let rejected = ledger.arm_frame(
            epoch,
            first_base.wrapping_add(100),
            &pack,
            &first,
            4,
            first_target,
        );
        assert!(matches!(
            rejected,
            OfflineAutomarkerTcpArmResult::OriginalUnchanged {
                frame,
                reason: OfflineAutomarkerTcpArmError::OverlappingOperation,
            } if frame == first
        ));

        let first_ack =
            ledger.observe_cumulative_ack(epoch, first_base.wrapping_add(first.len() as u32));
        assert_eq!(first_ack.retired_operations, 1);
        assert_eq!(first_ack.active_operations, 1);
        let second_ack =
            ledger.observe_cumulative_ack(epoch, second_base.wrapping_add(second.len() as u32));
        assert_eq!(second_ack.retired_operations, 1);
        assert_eq!(second_ack.active_operations, 0);
    }

    #[test]
    fn tcp_ledger_handles_sequence_wrap_and_retires_only_on_complete_cumulative_ack() {
        let pack = current_pack(AUTOMARKER_REQUEST_BUILD);
        let original = observed_frame(&request(
            3,
            [1.0, 2.0, 3.0, 4.0],
            [5.0, 6.0, 7.0, 8.0],
            1_789_176_510_696,
            762,
        ));
        let target = AutomarkerRequestXyz {
            x: -1.25,
            y: -2.5,
            z: -3.75,
        };
        let replacement = replacement_frame(&pack, &original, 4, target);
        let epoch = 99;
        let base = u32::MAX - 80;
        let mut ledger = OfflineAutomarkerTcpRewriteLedger::new(epoch);
        assert!(matches!(
            ledger.arm_frame(epoch, base, &pack, &original, 4, target),
            OfflineAutomarkerTcpArmResult::Armed { .. }
        ));

        for range in [0..81, 81..150, 150..197] {
            assert_segment_rewritten(
                ledger.rewrite_segment(
                    epoch,
                    base.wrapping_add(range.start as u32),
                    &original[range.clone()],
                ),
                &replacement[range],
            );
        }
        assert_eq!(ledger.active_operations(), 1);
        let behind = ledger.observe_cumulative_ack(epoch, base.wrapping_sub(1));
        assert_eq!(behind.retired_operations, 0);
        assert_eq!(behind.active_operations, 1);
        let partial = ledger.observe_cumulative_ack(epoch, base.wrapping_add(196));
        assert_eq!(partial.retired_operations, 0);
        assert_eq!(partial.active_operations, 1);
        let complete = ledger.observe_cumulative_ack(epoch, base.wrapping_add(197));
        assert_eq!(complete.retired_operations, 1);
        assert_eq!(complete.active_operations, 0);

        let after_ack = ledger.rewrite_segment(epoch, base, &original);
        assert!(matches!(
            after_ack,
            OfflineAutomarkerTcpSegmentResult::OriginalUnchanged {
                payload,
                reason: OfflineAutomarkerTcpSegmentReason::NoActiveOverlap,
            } if payload == original
        ));
    }

    #[test]
    fn tcp_ledger_fails_open_on_conflict_gap_ambiguity_and_epoch_change() {
        let pack = current_pack(AUTOMARKER_REQUEST_BUILD);
        let original = observed_frame(&request(
            1,
            [1.0, 2.0, 3.0, 4.0],
            [5.0, 6.0, 7.0, 8.0],
            1_789_176_498_286,
            607,
        ));
        let target = AutomarkerRequestXyz {
            x: 9.0,
            y: 10.0,
            z: 11.0,
        };
        let epoch = 123;
        let base = 77_000_u32;
        let mut ledger = OfflineAutomarkerTcpRewriteLedger::new(epoch);
        assert!(matches!(
            ledger.arm_frame(epoch, base, &pack, &original, 2, target),
            OfflineAutomarkerTcpArmResult::Armed { .. }
        ));

        let wrong_epoch = ledger.rewrite_segment(epoch + 1, base, &original);
        assert!(matches!(
            wrong_epoch,
            OfflineAutomarkerTcpSegmentResult::OriginalUnchanged {
                payload,
                reason: OfflineAutomarkerTcpSegmentReason::EpochMismatch { .. },
            } if payload == original
        ));
        assert_eq!(ledger.active_operations(), 1);

        // A gap outside the frame is irrelevant; one crossing the frame makes
        // the entire epoch unavailable until an explicit connection reset.
        assert!(!ledger.observe_gap(epoch, base.wrapping_add(500), 10));
        assert!(ledger.observe_gap(epoch, base.wrapping_add(40), 5));
        assert_eq!(
            ledger.poison_reason(),
            Some(OfflineAutomarkerTcpPoisonReason::GapObserved)
        );
        let after_gap = ledger.rewrite_segment(epoch, base, &original);
        assert!(matches!(
            after_gap,
            OfflineAutomarkerTcpSegmentResult::OriginalUnchanged {
                payload,
                reason: OfflineAutomarkerTcpSegmentReason::LedgerPoisoned(
                    OfflineAutomarkerTcpPoisonReason::GapObserved
                ),
            } if payload == original
        ));

        ledger.reset_connection_epoch(epoch + 1);
        assert_eq!(ledger.active_operations(), 0);
        assert_eq!(ledger.poison_reason(), None);
        assert!(matches!(
            ledger.arm_frame(epoch + 1, base, &pack, &original, 2, target),
            OfflineAutomarkerTcpArmResult::Armed { .. }
        ));
        let mut conflict = original[30..90].to_vec();
        conflict[7] ^= 0xff;
        let conflict_sequence = base.wrapping_add(30);
        let conflict_outcome = ledger.rewrite_segment(epoch + 1, conflict_sequence, &conflict);
        assert!(matches!(
            conflict_outcome,
            OfflineAutomarkerTcpSegmentResult::OriginalUnchanged {
                payload,
                reason: OfflineAutomarkerTcpSegmentReason::ConflictingRetransmission,
            } if payload == conflict
        ));
        assert_eq!(
            ledger.poison_reason(),
            Some(OfflineAutomarkerTcpPoisonReason::ConflictingRetransmission)
        );

        ledger.reset_connection_epoch(epoch + 2);
        assert!(matches!(
            ledger.arm_frame(epoch + 2, base, &pack, &original, 2, target),
            OfflineAutomarkerTcpArmResult::Armed { .. }
        ));
        let ambiguous = ledger.rewrite_segment(
            epoch + 2,
            base.wrapping_add(TCP_SERIAL_HALF_SPACE),
            &[1, 2, 3],
        );
        assert!(matches!(
            ambiguous,
            OfflineAutomarkerTcpSegmentResult::OriginalUnchanged {
                payload,
                reason: OfflineAutomarkerTcpSegmentReason::AmbiguousSequenceRange,
            } if payload == [1, 2, 3]
        ));
        assert_eq!(
            ledger.poison_reason(),
            Some(OfflineAutomarkerTcpPoisonReason::AmbiguousSequenceRange)
        );

        ledger.reset_connection_epoch(epoch + 3);
        assert!(matches!(
            ledger.arm_frame(epoch + 3, base, &pack, &original, 2, target),
            OfflineAutomarkerTcpArmResult::Armed { .. }
        ));
        let near_half_space = base.wrapping_add(TCP_SERIAL_HALF_SPACE).wrapping_sub(100);
        let ambiguous_arm =
            ledger.arm_frame(epoch + 3, near_half_space, &pack, &original, 3, target);
        assert!(matches!(
            ambiguous_arm,
            OfflineAutomarkerTcpArmResult::OriginalUnchanged {
                frame,
                reason: OfflineAutomarkerTcpArmError::AmbiguousSequenceRange,
            } if frame == original
        ));
    }

    #[test]
    fn tcp_ledger_proof_keeps_live_transport_disabled() {
        let proof: serde_json::Value = serde_json::from_str(include_str!(
            "../research/game-file-inventory/global/steam-25247556/ground-marker-packet-substitution-feasibility.v1.json"
        ))
        .unwrap();
        let ledger = &proof["offline_tcp_sequence_rewrite_ledger"];
        assert_eq!(ledger["implemented"], true);
        assert_eq!(ledger["network_or_process_capability"], false);
        assert_eq!(ledger["live_activation_available"], false);
        assert_eq!(proof["conclusion"]["runtime_sender_enabled"], false);
        assert_eq!(
            proof["conclusion"]["permission_to_replay_inject_or_rewrite"],
            false
        );
    }

    #[test]
    fn mismatched_marker_identity_and_tampered_envelope_fail_closed() {
        let pack = current_pack(AUTOMARKER_REQUEST_BUILD);
        let mut payload = request(1, [1.0, 2.0, 3.0, 4.0], [5.0, 6.0, 7.0, 8.0], 99, 10);
        // Slot 201 is encoded as c9 01; make it slot 202 while skill remains 1101.
        let slot = payload
            .windows(2)
            .position(|window| window == [0xc9, 0x01])
            .unwrap();
        payload[slot] = 0xca;
        assert!(matches!(
            decode_observed_automarker_request_into(&pack, &payload, &mut vec![]),
            Err(AutomarkerRequestDecodeError::InvalidMarkerIdentity { .. })
        ));

        let mut tampered = request(1, [1.0, 2.0, 3.0, 4.0], [5.0, 6.0, 7.0, 8.0], 99, 10);
        let iv = tampered
            .windows(16)
            .position(|window| window == [0x5a; 16])
            .unwrap();
        tampered[iv] ^= 1;
        assert!(matches!(
            decode_observed_automarker_request_into(&pack, &tampered, &mut vec![]),
            Err(AutomarkerRequestDecodeError::MacMismatch)
        ));
    }

    #[test]
    fn rejects_unverified_marker_skill_uuid_namespace() {
        let pack = current_pack(AUTOMARKER_REQUEST_BUILD);
        let payload = request_with(
            1,
            [1.0, 2.0, 3.0, 4.0],
            [5.0, 6.0, 7.0, 8.0],
            99,
            10,
            0x5000_0001,
            1.0,
        );
        assert!(matches!(
            decode_observed_automarker_request_into(&pack, &payload, &mut vec![]),
            Err(AutomarkerRequestDecodeError::InvalidMarkerSkillUuid {
                skill_uuid: 0x5000_0001
            })
        ));
    }

    #[test]
    fn accepts_another_session_generated_action_uuid() {
        let pack = current_pack(AUTOMARKER_REQUEST_BUILD);
        let payload = request_with(
            4,
            [1.0, 2.0, 3.0, 4.0],
            [5.0, 6.0, 7.0, 8.0],
            99,
            10,
            0x6abc_def0,
            1.0,
        );

        let decoded =
            decode_observed_automarker_request_into(&pack, &payload, &mut vec![]).unwrap();

        assert_eq!(decoded.marker_number, 4);
        assert_eq!(decoded.skill_uuid as u32, 0x6abc_def0);
    }

    #[test]
    fn rejects_nonfinite_or_out_of_bounds_positions_and_headings() {
        let pack = current_pack(AUTOMARKER_REQUEST_BUILD);
        for (target, expected_field) in [
            ([f32::NAN, 2.0, 3.0, 4.0], 1),
            ([MAX_ABS_MARKER_COORDINATE + 1.0, 2.0, 3.0, 4.0], 1),
            ([1.0, 2.0, 3.0, f32::NAN], 4),
            ([1.0, 2.0, 3.0, 360.0], 4),
        ] {
            let payload = request(1, target, [5.0, 6.0, 7.0, 8.0], 99, 10);
            assert_eq!(
                decode_observed_automarker_request_into(&pack, &payload, &mut vec![]),
                Err(AutomarkerRequestDecodeError::InvalidPosition {
                    field: expected_field
                })
            );
        }
    }

    #[test]
    fn rejects_nonfinite_or_nonunit_authenticated_current_build_scalar() {
        let pack = current_pack(AUTOMARKER_REQUEST_BUILD);
        for scalar in [f32::NAN, 0.5] {
            let payload = request_with(
                1,
                [1.0, 2.0, 3.0, 4.0],
                [5.0, 6.0, 7.0, 8.0],
                99,
                10,
                1_610_614_694,
                scalar,
            );
            assert!(matches!(
                decode_observed_automarker_request_into(&pack, &payload, &mut vec![]),
                Err(AutomarkerRequestDecodeError::UnsupportedValue { field: 6, .. })
            ));
        }
    }

    #[test]
    fn rejects_negative_begin_time_before_timestamp_comparison() {
        let pack = current_pack(AUTOMARKER_REQUEST_BUILD);
        let payload = request(1, [1.0, 2.0, 3.0, 4.0], [5.0, 6.0, 7.0, 8.0], u64::MAX, 10);
        assert_eq!(
            decode_observed_automarker_request_into(&pack, &payload, &mut vec![]),
            Err(AutomarkerRequestDecodeError::UnsupportedValue {
                field: 4,
                value: -1
            })
        );
    }
}
