//! Pure planner for mapping a saved marker preset onto fresh game-generated
//! marker requests.
//!
//! The exact-build capture proves that every successful marker operation has
//! its own action UUID, begin time, session sequence, authenticated attribute
//! envelope, RPC call ID, and FrameUp sequence. Consequently this planner never
//! duplicates or synthesizes a request. It consumes one already-observed exact
//! carrier for each preset marker and exposes only the XYZ rewrite assignment.
//! It has no process, packet, interception, input, or transport capability.

use std::collections::HashSet;

use crate::AutomarkerRequestXyz;

const MAX_MARKERS: usize = 6;
const MAX_ABS_COORDINATE: f32 = 1_000_000.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutomarkerPresetMarker {
    pub marker_number: u8,
    pub target_position: AutomarkerRequestXyz,
}

/// Sanitized identity of one complete request emitted by the live game.
///
/// `authenticated_envelope_fingerprint` identifies the complete immutable
/// `attr_data` bytes without exposing them. The actual rewrite boundary must
/// preserve that envelope, the current position, and all other fields exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AutomarkerCarrierObservation {
    pub connection_epoch: u64,
    pub marker_number: u8,
    pub frame_up_sequence: u32,
    pub rpc_call_id: u32,
    pub skill_uuid: i32,
    pub begin_time: i64,
    pub session_sequence: u32,
    /// Exact fixed32 bits of the carrier's game-resolved current position and
    /// heading. They are evidence only and are never replaced by this plan.
    pub current_position_bits: [u32; 4],
    pub authenticated_envelope_fingerprint: [u8; 32],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutomarkerRewriteAssignment {
    pub marker_number: u8,
    pub target_position: AutomarkerRequestXyz,
    pub connection_epoch: u64,
    pub frame_up_sequence: u32,
    pub rpc_call_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomarkerPresetSequenceState {
    AwaitingCarrier {
        marker_number: u8,
    },
    AwaitingConfirmation {
        marker_number: u8,
        rpc_return_observed: bool,
        authoritative_add_observed: bool,
    },
    Complete,
    Aborted(AutomarkerPresetSequenceError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomarkerPresetSequenceError {
    EmptyPreset,
    TooManyMarkers,
    InvalidMarkerNumber,
    DuplicateMarkerNumber,
    InvalidCoordinate,
    AlreadyComplete,
    AlreadyAborted,
    CarrierWhileAwaitingConfirmation,
    WrongCarrierMarker { expected: u8, actual: u8 },
    ConnectionEpochChanged,
    ReusedGameOwnedIdentity,
    StaleGameOwnedSequence,
    WrongRpcCallId,
    WrongAuthoritativeMarker,
    AuthoritativePositionMismatch,
    NegativeRpcReturn,
    SceneOrLeadershipChanged,
    TimedOut,
}

#[derive(Debug, Clone)]
pub struct AutomarkerPresetSequence {
    markers: Vec<AutomarkerPresetMarker>,
    next_index: usize,
    connection_epoch: Option<u64>,
    active_carrier: Option<AutomarkerCarrierObservation>,
    rpc_return_observed: bool,
    authoritative_add_observed: bool,
    used_identities: HashSet<(i32, i64, u32, [u8; 32])>,
    last_frame_up_sequence: Option<u32>,
    last_rpc_call_id: Option<u32>,
    last_session_sequence: Option<u32>,
    aborted: Option<AutomarkerPresetSequenceError>,
}

impl AutomarkerPresetSequence {
    pub fn new(
        mut markers: Vec<AutomarkerPresetMarker>,
    ) -> Result<Self, AutomarkerPresetSequenceError> {
        if markers.is_empty() {
            return Err(AutomarkerPresetSequenceError::EmptyPreset);
        }
        if markers.len() > MAX_MARKERS {
            return Err(AutomarkerPresetSequenceError::TooManyMarkers);
        }
        if markers.iter().any(|marker| {
            !(1..=MAX_MARKERS as u8).contains(&marker.marker_number)
                || !coordinate_is_valid(marker.target_position)
        }) {
            return Err(
                if markers
                    .iter()
                    .any(|marker| !(1..=MAX_MARKERS as u8).contains(&marker.marker_number))
                {
                    AutomarkerPresetSequenceError::InvalidMarkerNumber
                } else {
                    AutomarkerPresetSequenceError::InvalidCoordinate
                },
            );
        }
        markers.sort_by_key(|marker| marker.marker_number);
        if markers
            .windows(2)
            .any(|pair| pair[0].marker_number == pair[1].marker_number)
        {
            return Err(AutomarkerPresetSequenceError::DuplicateMarkerNumber);
        }
        Ok(Self {
            markers,
            next_index: 0,
            connection_epoch: None,
            active_carrier: None,
            rpc_return_observed: false,
            authoritative_add_observed: false,
            used_identities: HashSet::new(),
            last_frame_up_sequence: None,
            last_rpc_call_id: None,
            last_session_sequence: None,
            aborted: None,
        })
    }

    pub fn state(&self) -> AutomarkerPresetSequenceState {
        if let Some(reason) = self.aborted {
            return AutomarkerPresetSequenceState::Aborted(reason);
        }
        if self.next_index == self.markers.len() {
            return AutomarkerPresetSequenceState::Complete;
        }
        let marker_number = self.markers[self.next_index].marker_number;
        if self.active_carrier.is_some() {
            AutomarkerPresetSequenceState::AwaitingConfirmation {
                marker_number,
                rpc_return_observed: self.rpc_return_observed,
                authoritative_add_observed: self.authoritative_add_observed,
            }
        } else {
            AutomarkerPresetSequenceState::AwaitingCarrier { marker_number }
        }
    }

    /// Consumes exactly one fresh, same-number carrier for the next marker.
    ///
    /// Same-number carriers deliberately keep slot ID, skill ID, and action UUID
    /// relationships game-generated. Cross-marker identity substitution is
    /// byte-layout feasible but server acceptance is not proven.
    pub fn observe_carrier(
        &mut self,
        carrier: AutomarkerCarrierObservation,
    ) -> Result<AutomarkerRewriteAssignment, AutomarkerPresetSequenceError> {
        self.ensure_active()?;
        if self.active_carrier.is_some() {
            return self.abort(AutomarkerPresetSequenceError::CarrierWhileAwaitingConfirmation);
        }
        let target = self.markers[self.next_index];
        if carrier.marker_number != target.marker_number {
            return self.abort(AutomarkerPresetSequenceError::WrongCarrierMarker {
                expected: target.marker_number,
                actual: carrier.marker_number,
            });
        }
        if let Some(epoch) = self.connection_epoch {
            if carrier.connection_epoch != epoch {
                return self.abort(AutomarkerPresetSequenceError::ConnectionEpochChanged);
            }
        }
        let identity = (
            carrier.skill_uuid,
            carrier.begin_time,
            carrier.session_sequence,
            carrier.authenticated_envelope_fingerprint,
        );
        if self.used_identities.contains(&identity) {
            return self.abort(AutomarkerPresetSequenceError::ReusedGameOwnedIdentity);
        }
        if !is_newer_than(self.last_frame_up_sequence, carrier.frame_up_sequence)
            || !is_newer_than(self.last_rpc_call_id, carrier.rpc_call_id)
            || !is_newer_than(self.last_session_sequence, carrier.session_sequence)
        {
            return self.abort(AutomarkerPresetSequenceError::StaleGameOwnedSequence);
        }

        self.connection_epoch = Some(carrier.connection_epoch);
        self.used_identities.insert(identity);
        self.last_frame_up_sequence = Some(carrier.frame_up_sequence);
        self.last_rpc_call_id = Some(carrier.rpc_call_id);
        self.last_session_sequence = Some(carrier.session_sequence);
        self.active_carrier = Some(carrier);
        self.rpc_return_observed = false;
        self.authoritative_add_observed = false;
        Ok(AutomarkerRewriteAssignment {
            marker_number: target.marker_number,
            target_position: target.target_position,
            connection_epoch: carrier.connection_epoch,
            frame_up_sequence: carrier.frame_up_sequence,
            rpc_call_id: carrier.rpc_call_id,
        })
    }

    pub fn observe_rpc_return(
        &mut self,
        rpc_call_id: u32,
        successful_empty_return: bool,
    ) -> Result<(), AutomarkerPresetSequenceError> {
        self.ensure_active()?;
        let Some(carrier) = self.active_carrier else {
            return self.abort(AutomarkerPresetSequenceError::WrongRpcCallId);
        };
        if carrier.rpc_call_id != rpc_call_id {
            return self.abort(AutomarkerPresetSequenceError::WrongRpcCallId);
        }
        if !successful_empty_return {
            return self.abort(AutomarkerPresetSequenceError::NegativeRpcReturn);
        }
        self.rpc_return_observed = true;
        self.advance_if_confirmed();
        Ok(())
    }

    pub fn observe_authoritative_add(
        &mut self,
        marker_number: u8,
        position: AutomarkerRequestXyz,
    ) -> Result<(), AutomarkerPresetSequenceError> {
        self.ensure_active()?;
        if self.active_carrier.is_none() {
            return self.abort(AutomarkerPresetSequenceError::WrongAuthoritativeMarker);
        }
        let target = self.markers[self.next_index];
        if marker_number != target.marker_number {
            return self.abort(AutomarkerPresetSequenceError::WrongAuthoritativeMarker);
        }
        if !same_position_bits(position, target.target_position) {
            return self.abort(AutomarkerPresetSequenceError::AuthoritativePositionMismatch);
        }
        self.authoritative_add_observed = true;
        self.advance_if_confirmed();
        Ok(())
    }

    pub fn abort_for_context_change(&mut self) -> Result<(), AutomarkerPresetSequenceError> {
        self.abort(AutomarkerPresetSequenceError::SceneOrLeadershipChanged)
    }

    pub fn abort_for_timeout(&mut self) -> Result<(), AutomarkerPresetSequenceError> {
        self.abort(AutomarkerPresetSequenceError::TimedOut)
    }

    fn ensure_active(&self) -> Result<(), AutomarkerPresetSequenceError> {
        if let Some(reason) = self.aborted {
            Err(reason)
        } else if self.next_index == self.markers.len() {
            Err(AutomarkerPresetSequenceError::AlreadyComplete)
        } else {
            Ok(())
        }
    }

    fn advance_if_confirmed(&mut self) {
        if self.rpc_return_observed && self.authoritative_add_observed {
            self.next_index += 1;
            self.active_carrier = None;
            self.rpc_return_observed = false;
            self.authoritative_add_observed = false;
        }
    }

    fn abort<T>(
        &mut self,
        reason: AutomarkerPresetSequenceError,
    ) -> Result<T, AutomarkerPresetSequenceError> {
        self.aborted = Some(reason);
        self.active_carrier = None;
        Err(reason)
    }
}

fn coordinate_is_valid(position: AutomarkerRequestXyz) -> bool {
    [position.x, position.y, position.z]
        .into_iter()
        .all(|value| value.is_finite() && value.abs() <= MAX_ABS_COORDINATE)
}

fn same_position_bits(left: AutomarkerRequestXyz, right: AutomarkerRequestXyz) -> bool {
    left.x.to_bits() == right.x.to_bits()
        && left.y.to_bits() == right.y.to_bits()
        && left.z.to_bits() == right.z.to_bits()
}

/// RFC-1982-style comparison used only within a single proven connection
/// epoch. Equality is stale and the exact half-space remains ambiguous/stale.
fn is_newer_than(previous: Option<u32>, candidate: u32) -> bool {
    previous.is_none_or(|previous| {
        let distance = candidate.wrapping_sub(previous);
        distance != 0 && distance < 0x8000_0000
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker(number: u8) -> AutomarkerPresetMarker {
        AutomarkerPresetMarker {
            marker_number: number,
            target_position: AutomarkerRequestXyz {
                x: f32::from(number) * 10.0,
                y: 118.0,
                z: -f32::from(number),
            },
        }
    }

    fn carrier(number: u8, generation: u32) -> AutomarkerCarrierObservation {
        AutomarkerCarrierObservation {
            connection_epoch: 7,
            marker_number: number,
            frame_up_sequence: 50 + generation,
            rpc_call_id: 2_250 + generation,
            skill_uuid: 1_610_614_693 + generation as i32,
            begin_time: 1_789_176_498_000 + i64::from(generation),
            session_sequence: 600 + generation,
            current_position_bits: [1, 2, 3, 4],
            authenticated_envelope_fingerprint: [generation as u8; 32],
        }
    }

    #[test]
    fn six_marker_preset_requires_six_fresh_same_number_carriers() {
        let mut plan = AutomarkerPresetSequence::new((1..=6).rev().map(marker).collect()).unwrap();
        for number in 1..=6 {
            assert_eq!(
                plan.state(),
                AutomarkerPresetSequenceState::AwaitingCarrier {
                    marker_number: number
                }
            );
            let assignment = plan
                .observe_carrier(carrier(number, u32::from(number)))
                .unwrap();
            assert_eq!(assignment.marker_number, number);
            assert_eq!(assignment.target_position, marker(number).target_position);

            // Neither signal alone permits reuse of the carrier for the next marker.
            plan.observe_rpc_return(2_250 + u32::from(number), true)
                .unwrap();
            assert!(matches!(
                plan.state(),
                AutomarkerPresetSequenceState::AwaitingConfirmation {
                    rpc_return_observed: true,
                    authoritative_add_observed: false,
                    ..
                }
            ));
            plan.observe_authoritative_add(number, marker(number).target_position)
                .unwrap();
        }
        assert_eq!(plan.state(), AutomarkerPresetSequenceState::Complete);
    }

    #[test]
    fn duplicated_carrier_cannot_supply_two_markers() {
        let original = carrier(1, 1);
        let mut plan = AutomarkerPresetSequence::new(vec![marker(1), marker(2)]).unwrap();
        plan.observe_carrier(original).unwrap();
        plan.observe_authoritative_add(1, marker(1).target_position)
            .unwrap();
        plan.observe_rpc_return(original.rpc_call_id, true).unwrap();

        let duplicated = AutomarkerCarrierObservation {
            marker_number: 2,
            ..original
        };
        assert_eq!(
            plan.observe_carrier(duplicated),
            Err(AutomarkerPresetSequenceError::ReusedGameOwnedIdentity)
        );
    }

    #[test]
    fn cross_marker_carrier_fails_closed_before_any_assignment() {
        let mut plan = AutomarkerPresetSequence::new(vec![marker(2)]).unwrap();
        assert_eq!(
            plan.observe_carrier(carrier(1, 1)),
            Err(AutomarkerPresetSequenceError::WrongCarrierMarker {
                expected: 2,
                actual: 1
            })
        );
        assert_eq!(
            plan.state(),
            AutomarkerPresetSequenceState::Aborted(
                AutomarkerPresetSequenceError::WrongCarrierMarker {
                    expected: 2,
                    actual: 1
                }
            )
        );
    }

    #[test]
    fn sequence_does_not_advance_without_matching_authoritative_add() {
        let mut plan = AutomarkerPresetSequence::new(vec![marker(1), marker(2)]).unwrap();
        let request = carrier(1, 1);
        plan.observe_carrier(request).unwrap();
        plan.observe_rpc_return(request.rpc_call_id, true).unwrap();
        assert_eq!(
            plan.observe_authoritative_add(
                1,
                AutomarkerRequestXyz {
                    x: 10.5,
                    ..marker(1).target_position
                }
            ),
            Err(AutomarkerPresetSequenceError::AuthoritativePositionMismatch)
        );
    }

    #[test]
    fn connection_epoch_and_monotonic_transport_state_cannot_be_reused() {
        let mut plan = AutomarkerPresetSequence::new(vec![marker(1), marker(2)]).unwrap();
        let first = carrier(1, 5);
        plan.observe_carrier(first).unwrap();
        plan.observe_rpc_return(first.rpc_call_id, true).unwrap();
        plan.observe_authoritative_add(1, marker(1).target_position)
            .unwrap();

        let stale = AutomarkerCarrierObservation {
            marker_number: 2,
            skill_uuid: first.skill_uuid + 1,
            begin_time: first.begin_time + 1,
            authenticated_envelope_fingerprint: [9; 32],
            ..first
        };
        assert_eq!(
            plan.observe_carrier(stale),
            Err(AutomarkerPresetSequenceError::StaleGameOwnedSequence)
        );

        let mut plan = AutomarkerPresetSequence::new(vec![marker(1), marker(2)]).unwrap();
        plan.observe_carrier(first).unwrap();
        plan.observe_rpc_return(first.rpc_call_id, true).unwrap();
        plan.observe_authoritative_add(1, marker(1).target_position)
            .unwrap();
        let changed_epoch = AutomarkerCarrierObservation {
            connection_epoch: 8,
            marker_number: 2,
            ..carrier(2, 6)
        };
        assert_eq!(
            plan.observe_carrier(changed_epoch),
            Err(AutomarkerPresetSequenceError::ConnectionEpochChanged)
        );
    }

    #[test]
    fn malformed_presets_are_rejected() {
        assert!(matches!(
            AutomarkerPresetSequence::new(vec![]),
            Err(AutomarkerPresetSequenceError::EmptyPreset)
        ));
        assert!(matches!(
            AutomarkerPresetSequence::new(vec![marker(1), marker(1)]),
            Err(AutomarkerPresetSequenceError::DuplicateMarkerNumber)
        ));
        let mut invalid = marker(1);
        invalid.target_position.x = f32::NAN;
        assert!(matches!(
            AutomarkerPresetSequence::new(vec![invalid]),
            Err(AutomarkerPresetSequenceError::InvalidCoordinate)
        ));
    }
}
