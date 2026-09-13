//! Pure, crate-private supervisor for a future no-menu native preset load.
//!
//! Each source-unbound child interceptor is installed before the game-owned
//! trigger is invoked. The child then owns carrier binding, rewriting,
//! retransmission, and confirmation. The supervisor advances only after ACK,
//! RPC Return, authoritative MarkerAdd, and transport-obligation retirement.
//! This module opens no handle, sends no packet, invokes no UI/input operation,
//! and reads no player position.

#![allow(dead_code)] // Unwired until a real game-owned trigger is proven.

use crate::{
    automarker_active_coordinator::{ActiveAutomarkerCanaryPhase, ActiveAutomarkerCanaryProgress},
    automarker_native_trigger::{
        NativeAutomarkerCarrierTrigger, NativeCarrierTriggerReceipt, NativeCarrierTriggerRequest,
    },
    automarker_presets::AutomarkerPoint,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativeSequenceContext {
    pub context_generation: u64,
    pub connection_epoch: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NativeSourceUnboundPrearmRequest {
    pub attempt_id: u64,
    pub context_generation: u64,
    pub connection_epoch: u64,
    pub requested_micros: u64,
    pub target: AutomarkerPoint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeSourceUnboundPrearmReceipt {
    Armed {
        attempt_id: u64,
        installed_micros: u64,
    },
    Unavailable,
    Rejected,
}

/// Installs the existing interceptor without requiring a prior carrier.
/// `Armed` means interception is live; it does not mean anything was sent.
pub(crate) trait NativeAutomarkerSourceUnboundPrearm {
    fn prearm_source_unbound(
        &mut self,
        request: NativeSourceUnboundPrearmRequest,
    ) -> NativeSourceUnboundPrearmReceipt;

    fn cancel(&mut self, _attempt_id: u64) {}
}

#[derive(Debug, Default)]
pub(crate) struct UnavailableNativeSourceUnboundPrearm;

impl NativeAutomarkerSourceUnboundPrearm for UnavailableNativeSourceUnboundPrearm {
    fn prearm_source_unbound(
        &mut self,
        _request: NativeSourceUnboundPrearmRequest,
    ) -> NativeSourceUnboundPrearmReceipt {
        NativeSourceUnboundPrearmReceipt::Unavailable
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NativeChildCanaryObservation {
    pub attempt_id: u64,
    pub progress: ActiveAutomarkerCanaryProgress,
    /// Independent worker fact: no deterministic rewrite/retransmission
    /// obligation remains retained for this attempt.
    pub transport_obligation_retired: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativePresetSequenceState {
    ReadyToPrearm { marker_number: u8 },
    PrearmInFlight { marker_number: u8, attempt_id: u64 },
    ChildPrearmed { marker_number: u8, attempt_id: u64 },
    TriggerInFlight { marker_number: u8, attempt_id: u64 },
    AwaitingChildCompletion { marker_number: u8, attempt_id: u64 },
    Complete,
    Cancelled,
    Failed(NativePresetSequenceError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativePresetSequenceError {
    EmptyPlan,
    InvalidPlan,
    NotReady,
    AttemptIdExhausted,
    PrearmReceiptMismatch,
    PrearmUnavailable,
    PrearmRejected,
    TriggerReceiptMismatch,
    TriggerUnavailable,
    TriggerRejected,
    ContextChanged,
    UnexpectedChildProgress,
    ChildAttemptMismatch,
    ChildFailed,
    ChildProtocolViolation,
    AlreadyTerminal,
}

pub(crate) struct NativeAutomarkerPresetSequence<P, T> {
    prearm: P,
    trigger: T,
    context: NativeSequenceContext,
    plan: Vec<AutomarkerPoint>,
    next_index: usize,
    state: NativePresetSequenceState,
    next_attempt_id: u64,
}

impl<P: NativeAutomarkerSourceUnboundPrearm, T: NativeAutomarkerCarrierTrigger>
    NativeAutomarkerPresetSequence<P, T>
{
    pub(crate) fn new(
        prearm: P,
        trigger: T,
        mut plan: Vec<AutomarkerPoint>,
        context: NativeSequenceContext,
    ) -> Result<Self, NativePresetSequenceError> {
        if plan.is_empty() {
            return Err(NativePresetSequenceError::EmptyPlan);
        }
        if context.context_generation == 0
            || context.connection_epoch == 0
            || plan.len() > 6
            || plan.iter().any(|point| {
                !(1..=6).contains(&point.marker_number)
                    || ![point.x, point.y, point.z]
                        .into_iter()
                        .all(|axis| axis.is_finite() && axis.abs() <= 1_000_000.0)
            })
        {
            return Err(NativePresetSequenceError::InvalidPlan);
        }
        plan.sort_by_key(|point| point.marker_number);
        if plan
            .windows(2)
            .any(|pair| pair[0].marker_number == pair[1].marker_number)
        {
            return Err(NativePresetSequenceError::InvalidPlan);
        }
        let marker_number = plan[0].marker_number;
        Ok(Self {
            prearm,
            trigger,
            context,
            plan,
            next_index: 0,
            state: NativePresetSequenceState::ReadyToPrearm { marker_number },
            next_attempt_id: 1,
        })
    }

    pub(crate) fn state(&self) -> NativePresetSequenceState {
        self.state
    }

    /// Synchronously establishes source-unbound interception first, then asks
    /// the game to emit the carrier. No trigger call is possible on prearm
    /// failure or a mismatched receipt.
    pub(crate) fn prearm_then_request_next_trigger(
        &mut self,
        context: NativeSequenceContext,
        now_micros: u64,
    ) -> Result<NativeCarrierTriggerReceipt, NativePresetSequenceError> {
        self.require_context(context)?;
        let NativePresetSequenceState::ReadyToPrearm { marker_number } = self.state else {
            return Err(if self.terminal() {
                NativePresetSequenceError::AlreadyTerminal
            } else {
                NativePresetSequenceError::NotReady
            });
        };
        let attempt_id = self.next_attempt_id;
        let Some(next_attempt_id) = self.next_attempt_id.checked_add(1) else {
            self.state =
                NativePresetSequenceState::Failed(NativePresetSequenceError::AttemptIdExhausted);
            return Err(NativePresetSequenceError::AttemptIdExhausted);
        };
        self.next_attempt_id = next_attempt_id;
        self.state = NativePresetSequenceState::PrearmInFlight {
            marker_number,
            attempt_id,
        };
        let receipt = self
            .prearm
            .prearm_source_unbound(NativeSourceUnboundPrearmRequest {
                attempt_id,
                context_generation: context.context_generation,
                connection_epoch: context.connection_epoch,
                requested_micros: now_micros,
                target: self.plan[self.next_index].clone(),
            });
        let installed_micros = match receipt {
            NativeSourceUnboundPrearmReceipt::Armed {
                attempt_id: received_attempt,
                installed_micros,
            } if received_attempt == attempt_id && installed_micros >= now_micros => {
                installed_micros
            }
            NativeSourceUnboundPrearmReceipt::Unavailable => {
                return self.fail(NativePresetSequenceError::PrearmUnavailable, false);
            }
            NativeSourceUnboundPrearmReceipt::Rejected => {
                return self.fail(NativePresetSequenceError::PrearmRejected, false);
            }
            NativeSourceUnboundPrearmReceipt::Armed { .. } => {
                return self.fail(NativePresetSequenceError::PrearmReceiptMismatch, true);
            }
        };
        self.state = NativePresetSequenceState::ChildPrearmed {
            marker_number,
            attempt_id,
        };

        // The trigger boundary is reached only after the prearm receipt above.
        self.state = NativePresetSequenceState::TriggerInFlight {
            marker_number,
            attempt_id,
        };
        let trigger_receipt =
            self.trigger
                .request_game_owned_carrier_after_prearm(NativeCarrierTriggerRequest {
                    prearm_attempt_id: attempt_id,
                    marker_number,
                    context_generation: context.context_generation,
                    requested_micros: installed_micros,
                });
        match trigger_receipt {
            NativeCarrierTriggerReceipt::Issued {
                attempt_id: received_attempt,
                issued_micros,
            } if received_attempt == attempt_id && issued_micros >= installed_micros => {
                self.state = NativePresetSequenceState::AwaitingChildCompletion {
                    marker_number,
                    attempt_id,
                };
                Ok(trigger_receipt)
            }
            NativeCarrierTriggerReceipt::Unavailable => {
                self.fail(NativePresetSequenceError::TriggerUnavailable, true)
            }
            NativeCarrierTriggerReceipt::Rejected => {
                self.fail(NativePresetSequenceError::TriggerRejected, true)
            }
            NativeCarrierTriggerReceipt::Issued { .. } => {
                self.fail(NativePresetSequenceError::TriggerReceiptMismatch, true)
            }
        }
    }

    pub(crate) fn observe_child_progress(
        &mut self,
        context: NativeSequenceContext,
        observation: NativeChildCanaryObservation,
    ) -> Result<bool, NativePresetSequenceError> {
        self.require_context(context)?;
        let NativePresetSequenceState::AwaitingChildCompletion { attempt_id, .. } = self.state
        else {
            return Err(if self.terminal() {
                NativePresetSequenceError::AlreadyTerminal
            } else {
                NativePresetSequenceError::UnexpectedChildProgress
            });
        };
        if observation.attempt_id != attempt_id {
            return self.fail(NativePresetSequenceError::ChildAttemptMismatch, true);
        }
        let progress = observation.progress;
        if progress.phase == ActiveAutomarkerCanaryPhase::Failed {
            return self.fail(NativePresetSequenceError::ChildFailed, false);
        }
        if progress.phase != ActiveAutomarkerCanaryPhase::Succeeded {
            return Ok(false);
        }
        if !progress.transport_ack_confirmed
            || !progress.rpc_return_confirmed
            || !progress.authoritative_marker_confirmed
            || !observation.transport_obligation_retired
            || progress.failure_category.is_some()
        {
            return self.fail(NativePresetSequenceError::ChildProtocolViolation, true);
        }

        self.next_index += 1;
        self.state = if self.next_index == self.plan.len() {
            NativePresetSequenceState::Complete
        } else {
            NativePresetSequenceState::ReadyToPrearm {
                marker_number: self.plan[self.next_index].marker_number,
            }
        };
        Ok(true)
    }

    pub(crate) fn cancel(&mut self) {
        if self.terminal() {
            return;
        }
        if let Some(attempt_id) = self.active_attempt_id() {
            self.prearm.cancel(attempt_id);
            self.trigger.cancel(attempt_id);
        }
        self.state = NativePresetSequenceState::Cancelled;
    }

    fn require_context(
        &mut self,
        context: NativeSequenceContext,
    ) -> Result<(), NativePresetSequenceError> {
        if context != self.context {
            return self.fail(NativePresetSequenceError::ContextChanged, true);
        }
        Ok(())
    }

    fn active_attempt_id(&self) -> Option<u64> {
        match self.state {
            NativePresetSequenceState::PrearmInFlight { attempt_id, .. }
            | NativePresetSequenceState::ChildPrearmed { attempt_id, .. }
            | NativePresetSequenceState::TriggerInFlight { attempt_id, .. }
            | NativePresetSequenceState::AwaitingChildCompletion { attempt_id, .. } => {
                Some(attempt_id)
            }
            _ => None,
        }
    }

    fn terminal(&self) -> bool {
        matches!(
            self.state,
            NativePresetSequenceState::Complete
                | NativePresetSequenceState::Cancelled
                | NativePresetSequenceState::Failed(_)
        )
    }

    fn fail<R>(
        &mut self,
        reason: NativePresetSequenceError,
        cancel_active: bool,
    ) -> Result<R, NativePresetSequenceError> {
        if !self.terminal() {
            if cancel_active && let Some(attempt_id) = self.active_attempt_id() {
                self.prearm.cancel(attempt_id);
                self.trigger.cancel(attempt_id);
            }
            self.state = NativePresetSequenceState::Failed(reason);
        }
        Err(reason)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use crate::{
        automarker_active_coordinator::ActiveAutomarkerCanaryFailureCategory,
        automarker_native_trigger::UnavailableNativeCarrierTrigger,
    };

    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum TraceEvent {
        Prearmed(u64, u8),
        Triggered(u64, u8),
        PrearmCancelled(u64),
        TriggerCancelled(u64),
    }

    #[derive(Clone)]
    struct ScriptedPrearm {
        trace: Arc<Mutex<Vec<TraceEvent>>>,
        receipt: NativeSourceUnboundPrearmReceipt,
    }

    impl NativeAutomarkerSourceUnboundPrearm for ScriptedPrearm {
        fn prearm_source_unbound(
            &mut self,
            request: NativeSourceUnboundPrearmRequest,
        ) -> NativeSourceUnboundPrearmReceipt {
            self.trace.lock().unwrap().push(TraceEvent::Prearmed(
                request.attempt_id,
                request.target.marker_number,
            ));
            match self.receipt {
                NativeSourceUnboundPrearmReceipt::Armed {
                    installed_micros, ..
                } => NativeSourceUnboundPrearmReceipt::Armed {
                    attempt_id: request.attempt_id,
                    installed_micros: installed_micros.max(request.requested_micros),
                },
                receipt => receipt,
            }
        }

        fn cancel(&mut self, attempt_id: u64) {
            self.trace
                .lock()
                .unwrap()
                .push(TraceEvent::PrearmCancelled(attempt_id));
        }
    }

    #[derive(Clone)]
    struct ScriptedTrigger {
        trace: Arc<Mutex<Vec<TraceEvent>>>,
        receipt: NativeCarrierTriggerReceipt,
    }

    impl NativeAutomarkerCarrierTrigger for ScriptedTrigger {
        fn request_game_owned_carrier_after_prearm(
            &mut self,
            request: NativeCarrierTriggerRequest,
        ) -> NativeCarrierTriggerReceipt {
            let mut trace = self.trace.lock().unwrap();
            assert_eq!(
                trace.last(),
                Some(&TraceEvent::Prearmed(
                    request.prearm_attempt_id,
                    request.marker_number
                ))
            );
            trace.push(TraceEvent::Triggered(
                request.prearm_attempt_id,
                request.marker_number,
            ));
            match self.receipt {
                NativeCarrierTriggerReceipt::Issued { issued_micros, .. } => {
                    NativeCarrierTriggerReceipt::Issued {
                        attempt_id: request.prearm_attempt_id,
                        issued_micros: issued_micros.max(request.requested_micros),
                    }
                }
                receipt => receipt,
            }
        }

        fn cancel(&mut self, attempt_id: u64) {
            self.trace
                .lock()
                .unwrap()
                .push(TraceEvent::TriggerCancelled(attempt_id));
        }
    }

    fn context() -> NativeSequenceContext {
        NativeSequenceContext {
            context_generation: 9,
            connection_epoch: 7,
        }
    }

    fn point(marker_number: u8) -> AutomarkerPoint {
        AutomarkerPoint {
            marker_number,
            x: f32::from(marker_number) * 10.0,
            y: 118.0,
            z: -f32::from(marker_number),
        }
    }

    fn adapters() -> (ScriptedPrearm, ScriptedTrigger, Arc<Mutex<Vec<TraceEvent>>>) {
        let trace = Arc::new(Mutex::new(Vec::new()));
        (
            ScriptedPrearm {
                trace: Arc::clone(&trace),
                receipt: NativeSourceUnboundPrearmReceipt::Armed {
                    attempt_id: 0,
                    installed_micros: 0,
                },
            },
            ScriptedTrigger {
                trace: Arc::clone(&trace),
                receipt: NativeCarrierTriggerReceipt::Issued {
                    attempt_id: 0,
                    issued_micros: 0,
                },
            },
            trace,
        )
    }

    fn progress(phase: ActiveAutomarkerCanaryPhase) -> ActiveAutomarkerCanaryProgress {
        ActiveAutomarkerCanaryProgress {
            phase,
            transport_ack_confirmed: true,
            rpc_return_confirmed: true,
            authoritative_marker_confirmed: true,
            failure_category: None,
        }
    }

    fn success(attempt_id: u64) -> NativeChildCanaryObservation {
        NativeChildCanaryObservation {
            attempt_id,
            progress: progress(ActiveAutomarkerCanaryPhase::Succeeded),
            transport_obligation_retired: true,
        }
    }

    #[test]
    fn six_markers_prearm_before_each_trigger_and_advance_only_on_full_success() {
        let (prearm, trigger, trace) = adapters();
        let mut sequence = NativeAutomarkerPresetSequence::new(
            prearm,
            trigger,
            (1..=6).rev().map(point).collect(),
            context(),
        )
        .unwrap();
        for marker_number in 1..=6 {
            sequence
                .prearm_then_request_next_trigger(context(), u64::from(marker_number) * 100)
                .unwrap();
            assert_eq!(
                sequence.state(),
                NativePresetSequenceState::AwaitingChildCompletion {
                    marker_number,
                    attempt_id: u64::from(marker_number)
                }
            );
            assert!(
                !sequence
                    .observe_child_progress(
                        context(),
                        NativeChildCanaryObservation {
                            attempt_id: u64::from(marker_number),
                            progress: progress(ActiveAutomarkerCanaryPhase::AwaitingConfirmation),
                            transport_obligation_retired: false,
                        }
                    )
                    .unwrap()
            );
            assert!(
                sequence
                    .observe_child_progress(context(), success(u64::from(marker_number)))
                    .unwrap()
            );
        }
        assert_eq!(sequence.state(), NativePresetSequenceState::Complete);
        assert_eq!(
            *trace.lock().unwrap(),
            (1..=6)
                .flat_map(|marker| [
                    TraceEvent::Prearmed(u64::from(marker), marker),
                    TraceEvent::Triggered(u64::from(marker), marker)
                ])
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn prearm_failure_never_invokes_trigger_and_trigger_failure_disarms_child() {
        let (mut prearm, trigger, trace) = adapters();
        prearm.receipt = NativeSourceUnboundPrearmReceipt::Unavailable;
        let mut sequence =
            NativeAutomarkerPresetSequence::new(prearm, trigger, vec![point(1)], context())
                .unwrap();
        assert_eq!(
            sequence.prearm_then_request_next_trigger(context(), 10),
            Err(NativePresetSequenceError::PrearmUnavailable)
        );
        assert_eq!(*trace.lock().unwrap(), vec![TraceEvent::Prearmed(1, 1)]);

        let (prearm, mut trigger, trace) = adapters();
        trigger.receipt = NativeCarrierTriggerReceipt::Unavailable;
        let mut sequence =
            NativeAutomarkerPresetSequence::new(prearm, trigger, vec![point(1)], context())
                .unwrap();
        assert_eq!(
            sequence.prearm_then_request_next_trigger(context(), 10),
            Err(NativePresetSequenceError::TriggerUnavailable)
        );
        assert_eq!(
            *trace.lock().unwrap(),
            vec![
                TraceEvent::Prearmed(1, 1),
                TraceEvent::Triggered(1, 1),
                TraceEvent::PrearmCancelled(1),
                TraceEvent::TriggerCancelled(1)
            ]
        );
    }

    #[test]
    fn one_child_must_finish_before_the_next_prearm() {
        let (prearm, trigger, trace) = adapters();
        let mut sequence = NativeAutomarkerPresetSequence::new(
            prearm,
            trigger,
            vec![point(1), point(2)],
            context(),
        )
        .unwrap();
        sequence
            .prearm_then_request_next_trigger(context(), 10)
            .unwrap();
        assert_eq!(
            sequence.prearm_then_request_next_trigger(context(), 11),
            Err(NativePresetSequenceError::NotReady)
        );
        assert_eq!(trace.lock().unwrap().len(), 2);
        assert!(
            !sequence
                .observe_child_progress(
                    context(),
                    NativeChildCanaryObservation {
                        attempt_id: 1,
                        progress: progress(ActiveAutomarkerCanaryPhase::AwaitingConfirmation),
                        transport_obligation_retired: false,
                    },
                )
                .unwrap()
        );
        assert_eq!(trace.lock().unwrap().len(), 2);
    }

    #[test]
    fn mismatched_prearm_receipt_fails_before_trigger() {
        let (_, trigger, trace) = adapters();
        // This adapter normally normalizes the ID, so use a deliberately bad
        // implementation to prove the supervisor validates the boundary.
        struct BadPrearm(Arc<Mutex<Vec<TraceEvent>>>);
        impl NativeAutomarkerSourceUnboundPrearm for BadPrearm {
            fn prearm_source_unbound(
                &mut self,
                request: NativeSourceUnboundPrearmRequest,
            ) -> NativeSourceUnboundPrearmReceipt {
                self.0.lock().unwrap().push(TraceEvent::Prearmed(
                    request.attempt_id,
                    request.target.marker_number,
                ));
                NativeSourceUnboundPrearmReceipt::Armed {
                    attempt_id: request.attempt_id + 1,
                    installed_micros: request.requested_micros,
                }
            }
        }
        let mut sequence = NativeAutomarkerPresetSequence::new(
            BadPrearm(Arc::clone(&trace)),
            trigger,
            vec![point(1)],
            context(),
        )
        .unwrap();
        assert_eq!(
            sequence.prearm_then_request_next_trigger(context(), 10),
            Err(NativePresetSequenceError::PrearmReceiptMismatch)
        );
        assert!(
            !trace
                .lock()
                .unwrap()
                .iter()
                .any(|event| matches!(event, TraceEvent::Triggered(..)))
        );
    }

    #[test]
    fn every_success_fact_and_retirement_are_mandatory() {
        for missing in 0..4 {
            let (prearm, trigger, _) = adapters();
            let mut sequence = NativeAutomarkerPresetSequence::new(
                prearm,
                trigger,
                vec![point(1), point(2)],
                context(),
            )
            .unwrap();
            sequence
                .prearm_then_request_next_trigger(context(), 10)
                .unwrap();
            let mut observation = success(1);
            match missing {
                0 => observation.progress.transport_ack_confirmed = false,
                1 => observation.progress.rpc_return_confirmed = false,
                2 => observation.progress.authoritative_marker_confirmed = false,
                _ => observation.transport_obligation_retired = false,
            }
            assert_eq!(
                sequence.observe_child_progress(context(), observation),
                Err(NativePresetSequenceError::ChildProtocolViolation)
            );
        }
    }

    #[test]
    fn stale_attempt_failure_context_drift_and_cancel_are_terminal() {
        let (prearm, trigger, _) = adapters();
        let mut sequence = NativeAutomarkerPresetSequence::new(
            prearm,
            trigger,
            vec![point(1), point(2)],
            context(),
        )
        .unwrap();
        sequence
            .prearm_then_request_next_trigger(context(), 10)
            .unwrap();
        sequence
            .observe_child_progress(context(), success(1))
            .unwrap();
        sequence
            .prearm_then_request_next_trigger(context(), 20)
            .unwrap();
        assert_eq!(
            sequence.observe_child_progress(context(), success(1)),
            Err(NativePresetSequenceError::ChildAttemptMismatch)
        );

        let (prearm, trigger, _) = adapters();
        let mut failed =
            NativeAutomarkerPresetSequence::new(prearm, trigger, vec![point(1)], context())
                .unwrap();
        failed
            .prearm_then_request_next_trigger(context(), 10)
            .unwrap();
        assert_eq!(
            failed.observe_child_progress(
                context(),
                NativeChildCanaryObservation {
                    attempt_id: 1,
                    progress: ActiveAutomarkerCanaryProgress {
                        phase: ActiveAutomarkerCanaryPhase::Failed,
                        failure_category: Some(ActiveAutomarkerCanaryFailureCategory::Transport),
                        ..ActiveAutomarkerCanaryProgress::default()
                    },
                    transport_obligation_retired: true,
                }
            ),
            Err(NativePresetSequenceError::ChildFailed)
        );

        let (prearm, trigger, trace) = adapters();
        let mut cancelled =
            NativeAutomarkerPresetSequence::new(prearm, trigger, vec![point(1)], context())
                .unwrap();
        cancelled
            .prearm_then_request_next_trigger(context(), 20)
            .unwrap();
        cancelled.cancel();
        assert_eq!(cancelled.state(), NativePresetSequenceState::Cancelled);
        assert!(
            trace
                .lock()
                .unwrap()
                .contains(&TraceEvent::PrearmCancelled(1))
        );

        let (prearm, trigger, _) = adapters();
        let mut drifted =
            NativeAutomarkerPresetSequence::new(prearm, trigger, vec![point(1)], context())
                .unwrap();
        drifted
            .prearm_then_request_next_trigger(context(), 30)
            .unwrap();
        assert_eq!(
            drifted.observe_child_progress(
                NativeSequenceContext {
                    context_generation: 10,
                    ..context()
                },
                success(1)
            ),
            Err(NativePresetSequenceError::ContextChanged)
        );
    }

    #[test]
    fn trigger_contract_has_no_ui_input_or_player_position_dependency() {
        let debug = format!(
            "{:?}",
            NativeCarrierTriggerRequest {
                prearm_attempt_id: 1,
                marker_number: 1,
                context_generation: 9,
                requested_micros: 10
            }
        )
        .to_ascii_lowercase();
        for forbidden in [
            " x:", " y:", " z:", "player", "position", "input", "menu", "packet", "address",
            "port", "process", "pid", "call_id", "session",
        ] {
            assert!(!debug.contains(forbidden));
        }
    }

    #[test]
    fn unavailable_production_placeholders_fail_closed() {
        let mut sequence = NativeAutomarkerPresetSequence::new(
            UnavailableNativeSourceUnboundPrearm,
            UnavailableNativeCarrierTrigger,
            vec![point(1)],
            context(),
        )
        .unwrap();
        assert_eq!(
            sequence.prearm_then_request_next_trigger(context(), 10),
            Err(NativePresetSequenceError::PrearmUnavailable)
        );
        assert_eq!(
            sequence.state(),
            NativePresetSequenceState::Failed(NativePresetSequenceError::PrearmUnavailable)
        );
    }
}
