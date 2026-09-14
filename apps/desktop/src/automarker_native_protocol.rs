//! Pure authenticated protocol contract for a future native automarker adapter.
//! This module performs no I/O and grants no process or native-call capability.

#![allow(dead_code)] // Deliberately unwired while the production adapter is unavailable.

use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::{
    automarker_native_trigger::{NativeCarrierTriggerRequest, NativeMarkerTarget, marker_skill_id},
    automarker_presets::AutomarkerPoint,
};

type HmacSha256 = Hmac<Sha256>;

const MAGIC: &[u8; 4] = b"RLAM";
const VERSION: u16 = 1;
pub(crate) const MAX_FRAME_BYTES: usize = 256;
const MAX_BODY_BYTES: usize = 128;
const HEADER_BYTES: usize = 36;
const DIRECTION_KEY_DOMAIN: &[u8] = b"rlogs-automarker-ipc-direction-v1\0";

pub(crate) struct LaunchCapability([u8; 32]);

impl LaunchCapability {
    pub(crate) fn new(bytes: [u8; 32]) -> Option<Self> {
        (bytes.iter().any(|byte| *byte != 0)).then_some(Self(bytes))
    }

    fn direction_key(&self, direction: FrameDirection) -> [u8; 32] {
        let mut mac =
            HmacSha256::new_from_slice(&self.0).expect("HMAC-SHA256 accepts a 256-bit key");
        mac.update(DIRECTION_KEY_DOMAIN);
        mac.update(&[direction as u8]);
        mac.finalize().into_bytes().into()
    }
}

impl Drop for LaunchCapability {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum FrameDirection {
    HostToAdapter = 1,
    AdapterToHost = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlaceSavedMarker {
    pub attempt_id: u64,
    pub context_generation: u64,
    pub preset_commitment: [u8; 32],
    pub target: NativeMarkerTarget,
    pub marker_skill_id: i32,
}

impl PlaceSavedMarker {
    pub(crate) fn from_trigger(
        request: NativeCarrierTriggerRequest,
        preset_commitment: [u8; 32],
    ) -> Option<Self> {
        if request.prearm_attempt_id == 0
            || request.context_generation == 0
            || preset_commitment.iter().all(|byte| *byte == 0)
            || request.target.marker_number != request.marker_number
            || marker_skill_id(request.marker_number) != Some(request.marker_skill_id)
        {
            return None;
        }
        Some(Self {
            attempt_id: request.prearm_attempt_id,
            context_generation: request.context_generation,
            preset_commitment,
            target: request.target,
            marker_skill_id: request.marker_skill_id,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostRequest {
    Place(PlaceSavedMarker),
    Cancel {
        attempt_id: u64,
        context_generation: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum TerminalOutcome {
    Succeeded = 1,
    Failed = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum CancelOutcome {
    CancelledBeforeExecution = 1,
    TooLate = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdapterReceipt {
    Queued {
        attempt_id: u64,
        context_generation: u64,
    },
    Executing {
        attempt_id: u64,
        context_generation: u64,
    },
    DispatchReturned {
        attempt_id: u64,
        context_generation: u64,
        accepted: bool,
    },
    Terminal {
        attempt_id: u64,
        context_generation: u64,
        outcome: TerminalOutcome,
    },
    CancelResult {
        attempt_id: u64,
        context_generation: u64,
        outcome: CancelOutcome,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProtocolMessage {
    Request(HostRequest),
    Receipt(AdapterReceipt),
}

pub(crate) struct AuthenticatedFrame {
    bytes: Vec<u8>,
    tag: [u8; 32],
}

impl AuthenticatedFrame {
    pub(crate) fn seal(
        capability: &LaunchCapability,
        direction: FrameDirection,
        session: [u8; 16],
        counter: u64,
        message: ProtocolMessage,
    ) -> Result<Self, ProtocolError> {
        if counter == 0 || session.iter().all(|byte| *byte == 0) {
            return Err(ProtocolError::InvalidIdentity);
        }
        validate_direction(direction, message)?;
        let bytes = encode(direction, session, counter, message)?;
        let key = capability.direction_key(direction);
        let mut mac = HmacSha256::new_from_slice(&key).expect("HMAC-SHA256 accepts a 256-bit key");
        mac.update(&bytes);
        let tag: [u8; 32] = mac.finalize().into_bytes().into();
        Ok(Self { bytes, tag })
    }

    pub(crate) fn open(
        &self,
        capability: &LaunchCapability,
        expected_direction: FrameDirection,
        expected_session: [u8; 16],
        counters: &mut ReceiveCounter,
    ) -> Result<ProtocolMessage, ProtocolError> {
        if self.bytes.len() > MAX_FRAME_BYTES {
            return Err(ProtocolError::FrameTooLarge);
        }
        let key = capability.direction_key(expected_direction);
        let mut mac = HmacSha256::new_from_slice(&key).expect("HMAC-SHA256 accepts a 256-bit key");
        mac.update(&self.bytes);
        mac.verify_slice(&self.tag)
            .map_err(|_| ProtocolError::Authentication)?;
        let decoded = decode(&self.bytes)?;
        if decoded.direction != expected_direction {
            return Err(ProtocolError::WrongDirection);
        }
        if decoded.session != expected_session {
            return Err(ProtocolError::WrongSession);
        }
        counters.accept(decoded.counter)?;
        Ok(decoded.message)
    }
}

#[derive(Debug, Default)]
pub(crate) struct ReceiveCounter(u64);

impl ReceiveCounter {
    fn accept(&mut self, counter: u64) -> Result<(), ProtocolError> {
        let expected = self.0.checked_add(1).ok_or(ProtocolError::Counter)?;
        if counter != expected {
            return Err(ProtocolError::Counter);
        }
        self.0 = counter;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttemptPhase {
    Idle,
    Placed {
        attempt_id: u64,
        context_generation: u64,
    },
    Queued {
        attempt_id: u64,
        context_generation: u64,
    },
    Executing {
        attempt_id: u64,
        context_generation: u64,
    },
    DispatchReturned {
        attempt_id: u64,
        context_generation: u64,
        accepted: bool,
    },
    CancelRequested {
        attempt_id: u64,
        context_generation: u64,
        prior: CancelPoint,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CancelPoint {
    BeforeExecution,
    Executing,
    DispatchReturned { accepted: bool },
}

#[derive(Debug, Default)]
pub(crate) struct AttemptTracker {
    active: Option<PlaceSavedMarker>,
    phase: Option<AttemptPhase>,
}

impl AttemptTracker {
    pub(crate) fn accept_place(
        &mut self,
        place: PlaceSavedMarker,
        expected: PlaceSavedMarker,
    ) -> Result<(), ProtocolError> {
        if self.active.is_some() || place != expected {
            return Err(if self.active.is_some() {
                ProtocolError::AttemptInFlight
            } else {
                ProtocolError::BindingMismatch
            });
        }
        self.active = Some(place);
        self.phase = Some(AttemptPhase::Placed {
            attempt_id: place.attempt_id,
            context_generation: place.context_generation,
        });
        Ok(())
    }

    pub(crate) fn accept_cancel(
        &mut self,
        attempt_id: u64,
        context_generation: u64,
    ) -> Result<(), ProtocolError> {
        self.require_active(attempt_id, context_generation)?;
        let prior = match self.phase {
            Some(AttemptPhase::Placed { .. }) | Some(AttemptPhase::Queued { .. }) => {
                CancelPoint::BeforeExecution
            }
            Some(AttemptPhase::Executing { .. }) => CancelPoint::Executing,
            Some(AttemptPhase::DispatchReturned { accepted, .. }) => {
                CancelPoint::DispatchReturned { accepted }
            }
            _ => return Err(ProtocolError::InvalidTransition),
        };
        self.phase = Some(AttemptPhase::CancelRequested {
            attempt_id,
            context_generation,
            prior,
        });
        Ok(())
    }

    pub(crate) fn accept_receipt(&mut self, receipt: AdapterReceipt) -> Result<(), ProtocolError> {
        let (attempt_id, context_generation) = receipt_identity(receipt);
        self.require_active(attempt_id, context_generation)?;
        let next = match (self.phase, receipt) {
            (Some(AttemptPhase::Placed { .. }), AdapterReceipt::Queued { .. }) => {
                Some(AttemptPhase::Queued {
                    attempt_id,
                    context_generation,
                })
            }
            (Some(AttemptPhase::Queued { .. }), AdapterReceipt::Executing { .. }) => {
                Some(AttemptPhase::Executing {
                    attempt_id,
                    context_generation,
                })
            }
            (
                Some(AttemptPhase::Executing { .. }),
                AdapterReceipt::DispatchReturned { accepted, .. },
            ) => Some(AttemptPhase::DispatchReturned {
                attempt_id,
                context_generation,
                accepted,
            }),
            (
                Some(AttemptPhase::DispatchReturned { accepted: true, .. }),
                AdapterReceipt::Terminal {
                    outcome: TerminalOutcome::Succeeded,
                    ..
                },
            )
            | (
                Some(AttemptPhase::DispatchReturned { .. }),
                AdapterReceipt::Terminal {
                    outcome: TerminalOutcome::Failed,
                    ..
                },
            ) => None,
            (
                Some(AttemptPhase::CancelRequested {
                    prior: CancelPoint::BeforeExecution,
                    ..
                }),
                AdapterReceipt::CancelResult {
                    outcome: CancelOutcome::CancelledBeforeExecution,
                    ..
                },
            ) => None,
            (
                Some(AttemptPhase::CancelRequested { prior, .. }),
                AdapterReceipt::CancelResult {
                    outcome: CancelOutcome::TooLate,
                    ..
                },
            ) => Some(match prior {
                CancelPoint::BeforeExecution | CancelPoint::Executing => AttemptPhase::Executing {
                    attempt_id,
                    context_generation,
                },
                CancelPoint::DispatchReturned { accepted } => AttemptPhase::DispatchReturned {
                    attempt_id,
                    context_generation,
                    accepted,
                },
            }),
            _ => return Err(ProtocolError::InvalidTransition),
        };
        self.phase = next;
        if next.is_none() {
            self.active = None;
        }
        Ok(())
    }

    pub(crate) fn phase(&self) -> AttemptPhase {
        self.phase.unwrap_or(AttemptPhase::Idle)
    }

    fn require_active(
        &self,
        attempt_id: u64,
        context_generation: u64,
    ) -> Result<(), ProtocolError> {
        match self.active {
            Some(active)
                if active.attempt_id == attempt_id
                    && active.context_generation == context_generation =>
            {
                Ok(())
            }
            _ => Err(ProtocolError::BindingMismatch),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProtocolError {
    InvalidIdentity,
    InvalidMessage,
    Authentication,
    FrameTooLarge,
    WrongDirection,
    WrongSession,
    Counter,
    BindingMismatch,
    AttemptInFlight,
    InvalidTransition,
}

struct Decoded {
    direction: FrameDirection,
    session: [u8; 16],
    counter: u64,
    message: ProtocolMessage,
}

fn validate_direction(
    direction: FrameDirection,
    message: ProtocolMessage,
) -> Result<(), ProtocolError> {
    match (direction, message) {
        (FrameDirection::HostToAdapter, ProtocolMessage::Request(_))
        | (FrameDirection::AdapterToHost, ProtocolMessage::Receipt(_)) => Ok(()),
        _ => Err(ProtocolError::WrongDirection),
    }
}

fn encode(
    direction: FrameDirection,
    session: [u8; 16],
    counter: u64,
    message: ProtocolMessage,
) -> Result<Vec<u8>, ProtocolError> {
    let mut out = Vec::with_capacity(128);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.push(direction as u8);
    let kind = match message {
        ProtocolMessage::Request(HostRequest::Place(_)) => 1,
        ProtocolMessage::Request(HostRequest::Cancel { .. }) => 2,
        ProtocolMessage::Receipt(AdapterReceipt::Queued { .. }) => 3,
        ProtocolMessage::Receipt(AdapterReceipt::Executing { .. }) => 4,
        ProtocolMessage::Receipt(AdapterReceipt::DispatchReturned { .. }) => 5,
        ProtocolMessage::Receipt(AdapterReceipt::Terminal { .. }) => 6,
        ProtocolMessage::Receipt(AdapterReceipt::CancelResult { .. }) => 7,
    };
    out.push(kind);
    let body_length_offset = out.len();
    out.extend_from_slice(&0_u16.to_le_bytes());
    out.extend_from_slice(&0_u16.to_le_bytes());
    out.extend_from_slice(&session);
    out.extend_from_slice(&counter.to_le_bytes());
    let body_start = out.len();
    match message {
        ProtocolMessage::Request(HostRequest::Place(place)) => {
            out.extend_from_slice(&place.attempt_id.to_le_bytes());
            out.extend_from_slice(&place.context_generation.to_le_bytes());
            out.extend_from_slice(&place.preset_commitment);
            out.push(place.target.marker_number);
            out.extend_from_slice(&place.marker_skill_id.to_le_bytes());
            for bits in place.target.coordinate_bits() {
                out.extend_from_slice(&bits.to_le_bytes());
            }
        }
        ProtocolMessage::Request(HostRequest::Cancel {
            attempt_id,
            context_generation,
        }) => {
            out.extend_from_slice(&attempt_id.to_le_bytes());
            out.extend_from_slice(&context_generation.to_le_bytes());
        }
        ProtocolMessage::Receipt(receipt) => {
            let (attempt_id, context_generation) = receipt_identity(receipt);
            out.extend_from_slice(&attempt_id.to_le_bytes());
            out.extend_from_slice(&context_generation.to_le_bytes());
            match receipt {
                AdapterReceipt::DispatchReturned { accepted, .. } => out.push(u8::from(accepted)),
                AdapterReceipt::Terminal { outcome, .. } => out.push(outcome as u8),
                AdapterReceipt::CancelResult { outcome, .. } => out.push(outcome as u8),
                _ => {}
            }
        }
    }
    let body_len = out.len() - body_start;
    if body_len > MAX_BODY_BYTES {
        return Err(ProtocolError::FrameTooLarge);
    }
    out[body_length_offset..body_length_offset + 2]
        .copy_from_slice(&(body_len as u16).to_le_bytes());
    (out.len() <= MAX_FRAME_BYTES)
        .then_some(out)
        .ok_or(ProtocolError::FrameTooLarge)
}

fn decode(bytes: &[u8]) -> Result<Decoded, ProtocolError> {
    let mut cursor = Cursor::new(bytes);
    if cursor.take(4)? != MAGIC || cursor.u16()? != VERSION {
        return Err(ProtocolError::InvalidMessage);
    }
    let direction = match cursor.u8()? {
        1 => FrameDirection::HostToAdapter,
        2 => FrameDirection::AdapterToHost,
        _ => return Err(ProtocolError::InvalidMessage),
    };
    let kind = cursor.u8()?;
    let body_len = usize::from(cursor.u16()?);
    if cursor.u16()? != 0 || body_len > MAX_BODY_BYTES {
        return Err(ProtocolError::InvalidMessage);
    }
    let session = cursor.array::<16>()?;
    let counter = cursor.u64()?;
    if bytes.len() != HEADER_BYTES + body_len {
        return Err(ProtocolError::InvalidMessage);
    }
    let message = match kind {
        1 => {
            let attempt_id = cursor.u64()?;
            let context_generation = cursor.u64()?;
            let preset_commitment = cursor.array::<32>()?;
            let marker_number = cursor.u8()?;
            let skill_id = cursor.i32()?;
            let point = AutomarkerPoint {
                marker_number,
                x: f32::from_bits(cursor.u32()?),
                y: f32::from_bits(cursor.u32()?),
                z: f32::from_bits(cursor.u32()?),
            };
            let target = NativeMarkerTarget::from_saved_point(&point)
                .ok_or(ProtocolError::InvalidMessage)?;
            let place = PlaceSavedMarker {
                attempt_id,
                context_generation,
                preset_commitment,
                target,
                marker_skill_id: skill_id,
            };
            if attempt_id == 0
                || context_generation == 0
                || preset_commitment.iter().all(|byte| *byte == 0)
                || marker_skill_id(marker_number) != Some(skill_id)
            {
                return Err(ProtocolError::InvalidMessage);
            }
            ProtocolMessage::Request(HostRequest::Place(place))
        }
        2 => {
            let attempt_id = cursor.u64()?;
            let context_generation = cursor.u64()?;
            if attempt_id == 0 || context_generation == 0 {
                return Err(ProtocolError::InvalidMessage);
            }
            ProtocolMessage::Request(HostRequest::Cancel {
                attempt_id,
                context_generation,
            })
        }
        3..=7 => {
            let attempt_id = cursor.u64()?;
            let context_generation = cursor.u64()?;
            if attempt_id == 0 || context_generation == 0 {
                return Err(ProtocolError::InvalidMessage);
            }
            let receipt = match kind {
                3 => AdapterReceipt::Queued {
                    attempt_id,
                    context_generation,
                },
                4 => AdapterReceipt::Executing {
                    attempt_id,
                    context_generation,
                },
                5 => AdapterReceipt::DispatchReturned {
                    attempt_id,
                    context_generation,
                    accepted: match cursor.u8()? {
                        0 => false,
                        1 => true,
                        _ => return Err(ProtocolError::InvalidMessage),
                    },
                },
                6 => AdapterReceipt::Terminal {
                    attempt_id,
                    context_generation,
                    outcome: match cursor.u8()? {
                        1 => TerminalOutcome::Succeeded,
                        2 => TerminalOutcome::Failed,
                        _ => return Err(ProtocolError::InvalidMessage),
                    },
                },
                _ => AdapterReceipt::CancelResult {
                    attempt_id,
                    context_generation,
                    outcome: match cursor.u8()? {
                        1 => CancelOutcome::CancelledBeforeExecution,
                        2 => CancelOutcome::TooLate,
                        _ => return Err(ProtocolError::InvalidMessage),
                    },
                },
            };
            ProtocolMessage::Receipt(receipt)
        }
        _ => return Err(ProtocolError::InvalidMessage),
    };
    if !cursor.finished() {
        return Err(ProtocolError::InvalidMessage);
    }
    validate_direction(direction, message)?;
    Ok(Decoded {
        direction,
        session,
        counter,
        message,
    })
}

fn receipt_identity(receipt: AdapterReceipt) -> (u64, u64) {
    match receipt {
        AdapterReceipt::Queued {
            attempt_id,
            context_generation,
        }
        | AdapterReceipt::Executing {
            attempt_id,
            context_generation,
        }
        | AdapterReceipt::DispatchReturned {
            attempt_id,
            context_generation,
            ..
        }
        | AdapterReceipt::Terminal {
            attempt_id,
            context_generation,
            ..
        }
        | AdapterReceipt::CancelResult {
            attempt_id,
            context_generation,
            ..
        } => (attempt_id, context_generation),
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }
    fn take(&mut self, count: usize) -> Result<&'a [u8], ProtocolError> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or(ProtocolError::InvalidMessage)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(ProtocolError::InvalidMessage)?;
        self.offset = end;
        Ok(value)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], ProtocolError> {
        self.take(N)?
            .try_into()
            .map_err(|_| ProtocolError::InvalidMessage)
    }
    fn u8(&mut self) -> Result<u8, ProtocolError> {
        Ok(self.array::<1>()?[0])
    }
    fn u16(&mut self) -> Result<u16, ProtocolError> {
        Ok(u16::from_le_bytes(self.array()?))
    }
    fn u32(&mut self) -> Result<u32, ProtocolError> {
        Ok(u32::from_le_bytes(self.array()?))
    }
    fn i32(&mut self) -> Result<i32, ProtocolError> {
        Ok(i32::from_le_bytes(self.array()?))
    }
    fn u64(&mut self) -> Result<u64, ProtocolError> {
        Ok(u64::from_le_bytes(self.array()?))
    }
    fn finished(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capability() -> LaunchCapability {
        LaunchCapability::new([0xA5; 32]).unwrap()
    }
    fn session() -> [u8; 16] {
        [0x5A; 16]
    }
    fn place(marker: u8) -> PlaceSavedMarker {
        let target = NativeMarkerTarget::from_saved_point(&AutomarkerPoint {
            marker_number: marker,
            x: marker as f32 * 10.0,
            y: 118.0,
            z: -(marker as f32),
        })
        .unwrap();
        PlaceSavedMarker {
            attempt_id: marker as u64,
            context_generation: 9,
            preset_commitment: [marker; 32],
            target,
            marker_skill_id: 1_100 + marker as i32,
        }
    }

    fn copy_frame(frame: &AuthenticatedFrame) -> AuthenticatedFrame {
        AuthenticatedFrame {
            bytes: frame.bytes.clone(),
            tag: frame.tag,
        }
    }

    fn resign(
        frame: &mut AuthenticatedFrame,
        capability: &LaunchCapability,
        direction: FrameDirection,
    ) {
        let key = capability.direction_key(direction);
        let mut mac = HmacSha256::new_from_slice(&key).unwrap();
        mac.update(&frame.bytes);
        frame.tag = mac.finalize().into_bytes().into();
    }

    #[test]
    fn canonical_place_and_cancel_round_trip() {
        let cap = capability();
        let mut counters = ReceiveCounter::default();
        for (counter, request) in [
            HostRequest::Place(place(1)),
            HostRequest::Cancel {
                attempt_id: 1,
                context_generation: 9,
            },
        ]
        .into_iter()
        .enumerate()
        {
            let frame = AuthenticatedFrame::seal(
                &cap,
                FrameDirection::HostToAdapter,
                session(),
                counter as u64 + 1,
                ProtocolMessage::Request(request),
            )
            .unwrap();
            assert!(frame.bytes.len() <= MAX_FRAME_BYTES);
            assert_eq!(
                frame
                    .open(
                        &cap,
                        FrameDirection::HostToAdapter,
                        session(),
                        &mut counters
                    )
                    .unwrap(),
                ProtocolMessage::Request(request)
            );
        }
    }

    #[test]
    fn tampering_replay_direction_session_and_counter_fail() {
        let cap = capability();
        let frame = AuthenticatedFrame::seal(
            &cap,
            FrameDirection::HostToAdapter,
            session(),
            1,
            ProtocolMessage::Request(HostRequest::Place(place(1))),
        )
        .unwrap();
        let mut tampered = copy_frame(&frame);
        tampered.bytes[40] ^= 1;
        assert_eq!(
            tampered.open(
                &cap,
                FrameDirection::HostToAdapter,
                session(),
                &mut ReceiveCounter::default()
            ),
            Err(ProtocolError::Authentication)
        );
        assert_eq!(
            frame.open(
                &cap,
                FrameDirection::AdapterToHost,
                session(),
                &mut ReceiveCounter::default()
            ),
            Err(ProtocolError::Authentication)
        );
        assert_eq!(
            frame.open(
                &cap,
                FrameDirection::HostToAdapter,
                [9; 16],
                &mut ReceiveCounter::default()
            ),
            Err(ProtocolError::WrongSession)
        );
        let mut counters = ReceiveCounter::default();
        frame
            .open(
                &cap,
                FrameDirection::HostToAdapter,
                session(),
                &mut counters,
            )
            .unwrap();
        assert_eq!(
            frame.open(
                &cap,
                FrameDirection::HostToAdapter,
                session(),
                &mut counters
            ),
            Err(ProtocolError::Counter)
        );
        let skipped = AuthenticatedFrame::seal(
            &cap,
            FrameDirection::HostToAdapter,
            session(),
            3,
            ProtocolMessage::Request(HostRequest::Place(place(1))),
        )
        .unwrap();
        assert_eq!(
            skipped.open(
                &cap,
                FrameDirection::HostToAdapter,
                session(),
                &mut counters
            ),
            Err(ProtocolError::Counter)
        );
    }

    #[test]
    fn wrong_key_reflection_and_malformed_frames_do_not_consume_counter() {
        let cap = capability();
        let valid = AuthenticatedFrame::seal(
            &cap,
            FrameDirection::HostToAdapter,
            session(),
            1,
            ProtocolMessage::Request(HostRequest::Place(place(1))),
        )
        .unwrap();
        let wrong = LaunchCapability::new([0x3C; 32]).unwrap();
        let mut counter = ReceiveCounter::default();
        assert_eq!(
            valid.open(
                &wrong,
                FrameDirection::HostToAdapter,
                session(),
                &mut counter
            ),
            Err(ProtocolError::Authentication)
        );

        let reflected = AuthenticatedFrame::seal(
            &cap,
            FrameDirection::AdapterToHost,
            session(),
            1,
            ProtocolMessage::Receipt(AdapterReceipt::Queued {
                attempt_id: 1,
                context_generation: 9,
            }),
        )
        .unwrap();
        assert_eq!(
            reflected.open(&cap, FrameDirection::HostToAdapter, session(), &mut counter),
            Err(ProtocolError::Authentication)
        );

        for mutation in 0..5 {
            let mut malformed = copy_frame(&valid);
            match mutation {
                0 => malformed.bytes.truncate(HEADER_BYTES - 1),
                1 => malformed.bytes.push(0),
                2 => malformed.bytes[4..6].copy_from_slice(&(VERSION + 1).to_le_bytes()),
                3 => malformed.bytes[10] = 1,
                _ => malformed.bytes[8..10]
                    .copy_from_slice(&((MAX_BODY_BYTES + 1) as u16).to_le_bytes()),
            }
            resign(&mut malformed, &cap, FrameDirection::HostToAdapter);
            assert_eq!(
                malformed.open(&cap, FrameDirection::HostToAdapter, session(), &mut counter),
                Err(ProtocolError::InvalidMessage)
            );
        }

        valid
            .open(&cap, FrameDirection::HostToAdapter, session(), &mut counter)
            .unwrap();
    }

    #[test]
    fn truncated_tag_and_frame_boundaries_fail_closed() {
        let cap = capability();
        let valid = AuthenticatedFrame::seal(
            &cap,
            FrameDirection::HostToAdapter,
            session(),
            1,
            ProtocolMessage::Request(HostRequest::Place(place(1))),
        )
        .unwrap();
        for length in [0, 1, HEADER_BYTES - 1, valid.bytes.len() - 1] {
            let mut truncated = copy_frame(&valid);
            truncated.bytes.truncate(length);
            assert_eq!(
                truncated.open(
                    &cap,
                    FrameDirection::HostToAdapter,
                    session(),
                    &mut ReceiveCounter::default()
                ),
                Err(ProtocolError::Authentication)
            );
        }
        let mut bad_tag = copy_frame(&valid);
        bad_tag.tag[31] ^= 1;
        assert_eq!(
            bad_tag.open(
                &cap,
                FrameDirection::HostToAdapter,
                session(),
                &mut ReceiveCounter::default()
            ),
            Err(ProtocolError::Authentication)
        );
        let oversized = AuthenticatedFrame {
            bytes: vec![0; MAX_FRAME_BYTES + 1],
            tag: [0; 32],
        };
        assert_eq!(
            oversized.open(
                &cap,
                FrameDirection::HostToAdapter,
                session(),
                &mut ReceiveCounter::default()
            ),
            Err(ProtocolError::FrameTooLarge)
        );
    }

    #[test]
    fn binding_skill_target_and_size_gates_fail_closed() {
        let mut tracker = AttemptTracker::default();
        assert_eq!(
            tracker.accept_place(place(2), place(1)),
            Err(ProtocolError::BindingMismatch)
        );
        tracker.accept_place(place(1), place(1)).unwrap();
        assert_eq!(
            tracker.accept_place(place(2), place(2)),
            Err(ProtocolError::AttemptInFlight)
        );

        let mut invalid = place(1);
        invalid.marker_skill_id = 1102;
        let frame = AuthenticatedFrame::seal(
            &capability(),
            FrameDirection::HostToAdapter,
            session(),
            1,
            ProtocolMessage::Request(HostRequest::Place(invalid)),
        )
        .unwrap();
        assert_eq!(
            frame.open(
                &capability(),
                FrameDirection::HostToAdapter,
                session(),
                &mut ReceiveCounter::default()
            ),
            Err(ProtocolError::InvalidMessage)
        );

        let oversized = AuthenticatedFrame {
            bytes: vec![0; MAX_FRAME_BYTES + 1],
            tag: [0; 32],
        };
        assert_eq!(
            oversized.open(
                &capability(),
                FrameDirection::HostToAdapter,
                session(),
                &mut ReceiveCounter::default()
            ),
            Err(ProtocolError::FrameTooLarge)
        );
    }

    #[test]
    fn terminal_and_cancellation_fsm_is_strict() {
        let p = place(1);
        let mut tracker = AttemptTracker::default();
        tracker.accept_place(p, p).unwrap();
        tracker
            .accept_receipt(AdapterReceipt::Queued {
                attempt_id: 1,
                context_generation: 9,
            })
            .unwrap();
        assert_eq!(
            tracker.accept_receipt(AdapterReceipt::Queued {
                attempt_id: 1,
                context_generation: 9,
            }),
            Err(ProtocolError::InvalidTransition)
        );
        tracker
            .accept_receipt(AdapterReceipt::Executing {
                attempt_id: 1,
                context_generation: 9,
            })
            .unwrap();
        tracker
            .accept_receipt(AdapterReceipt::DispatchReturned {
                attempt_id: 1,
                context_generation: 9,
                accepted: true,
            })
            .unwrap();
        tracker
            .accept_receipt(AdapterReceipt::Terminal {
                attempt_id: 1,
                context_generation: 9,
                outcome: TerminalOutcome::Succeeded,
            })
            .unwrap();
        assert_eq!(tracker.phase(), AttemptPhase::Idle);

        tracker.accept_place(p, p).unwrap();
        tracker
            .accept_receipt(AdapterReceipt::Queued {
                attempt_id: 1,
                context_generation: 9,
            })
            .unwrap();
        tracker
            .accept_receipt(AdapterReceipt::Executing {
                attempt_id: 1,
                context_generation: 9,
            })
            .unwrap();
        tracker
            .accept_receipt(AdapterReceipt::DispatchReturned {
                attempt_id: 1,
                context_generation: 9,
                accepted: false,
            })
            .unwrap();
        assert_eq!(
            tracker.accept_receipt(AdapterReceipt::Terminal {
                attempt_id: 1,
                context_generation: 9,
                outcome: TerminalOutcome::Succeeded,
            }),
            Err(ProtocolError::InvalidTransition)
        );
        tracker
            .accept_receipt(AdapterReceipt::Terminal {
                attempt_id: 1,
                context_generation: 9,
                outcome: TerminalOutcome::Failed,
            })
            .unwrap();

        tracker.accept_place(p, p).unwrap();
        tracker.accept_cancel(1, 9).unwrap();
        assert_eq!(
            tracker.accept_receipt(AdapterReceipt::Executing {
                attempt_id: 1,
                context_generation: 9
            }),
            Err(ProtocolError::InvalidTransition)
        );
        tracker
            .accept_receipt(AdapterReceipt::CancelResult {
                attempt_id: 1,
                context_generation: 9,
                outcome: CancelOutcome::CancelledBeforeExecution,
            })
            .unwrap();
        assert_eq!(tracker.phase(), AttemptPhase::Idle);
    }

    #[test]
    fn cancellation_distinguishes_pre_execution_from_too_late() {
        let p = place(1);

        let mut before = AttemptTracker::default();
        before.accept_place(p, p).unwrap();
        before.accept_cancel(1, 9).unwrap();
        assert_eq!(
            before.accept_receipt(AdapterReceipt::Terminal {
                attempt_id: 1,
                context_generation: 9,
                outcome: TerminalOutcome::Succeeded,
            }),
            Err(ProtocolError::InvalidTransition)
        );
        before
            .accept_receipt(AdapterReceipt::CancelResult {
                attempt_id: 1,
                context_generation: 9,
                outcome: CancelOutcome::CancelledBeforeExecution,
            })
            .unwrap();
        assert_eq!(before.phase(), AttemptPhase::Idle);

        let mut late = AttemptTracker::default();
        late.accept_place(p, p).unwrap();
        late.accept_receipt(AdapterReceipt::Queued {
            attempt_id: 1,
            context_generation: 9,
        })
        .unwrap();
        late.accept_receipt(AdapterReceipt::Executing {
            attempt_id: 1,
            context_generation: 9,
        })
        .unwrap();
        late.accept_cancel(1, 9).unwrap();
        assert_eq!(
            late.accept_receipt(AdapterReceipt::CancelResult {
                attempt_id: 1,
                context_generation: 9,
                outcome: CancelOutcome::CancelledBeforeExecution,
            }),
            Err(ProtocolError::InvalidTransition)
        );
        late.accept_receipt(AdapterReceipt::CancelResult {
            attempt_id: 1,
            context_generation: 9,
            outcome: CancelOutcome::TooLate,
        })
        .unwrap();
        assert_eq!(
            late.phase(),
            AttemptPhase::Executing {
                attempt_id: 1,
                context_generation: 9
            }
        );
        late.accept_receipt(AdapterReceipt::DispatchReturned {
            attempt_id: 1,
            context_generation: 9,
            accepted: true,
        })
        .unwrap();
        late.accept_receipt(AdapterReceipt::Terminal {
            attempt_id: 1,
            context_generation: 9,
            outcome: TerminalOutcome::Succeeded,
        })
        .unwrap();
        assert_eq!(late.phase(), AttemptPhase::Idle);
    }
}
