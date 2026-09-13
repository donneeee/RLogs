use std::{
    collections::VecDeque,
    sync::{Arc, Condvar, Mutex},
    time::Instant,
};

use thiserror::Error;

use crate::{
    CaptureError, CaptureLinkType, CaptureSource, CaptureSourceKind, CaptureSourceMetadata,
    CapturedFrame,
};

pub(crate) const MAX_MULTI_SOURCE_FAN_IN_SOURCES: usize = 8;
pub(crate) const MAX_MULTI_SOURCE_FAN_IN_QUEUE_FRAMES: usize = 8_192;
pub(crate) const MAX_MULTI_SOURCE_FAN_IN_QUEUE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct MultiSourceFanInMetrics {
    pub registered_sources: u64,
    pub active_sources: usize,
    pub completed_sources: u64,
    pub failed_sources: u64,
    pub accepted_frames: u64,
    pub delivered_frames: u64,
    pub queue_full_rejections: u64,
    pub byte_full_rejections: u64,
    pub oversized_frame_rejections: u64,
    pub queued_bytes: usize,
    pub peak_queued_frames: usize,
    pub peak_queued_bytes: usize,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MultiSourceRegisterError {
    #[error("multi-source capture has already stopped")]
    Stopped,
    #[error("multi-source capture already has its maximum number of active sources")]
    MaximumSources,
    #[error("multi-source capture source identity space is exhausted")]
    SourceIdentityExhausted,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub(crate) enum MultiSourcePushError {
    #[error("multi-source capture aggregate queue is full")]
    QueueFull(CapturedFrame),
    #[error("multi-source capture aggregate byte queue is full")]
    QueueBytesFull(CapturedFrame),
    #[error("frame exceeds the multi-source capture aggregate byte limit")]
    FrameTooLarge(CapturedFrame),
    #[error("multi-source capture has stopped")]
    Stopped(CapturedFrame),
    #[error("multi-source capture frame sequence space is exhausted")]
    SequenceExhausted(CapturedFrame),
}

#[derive(Debug)]
struct FanInState {
    queue: VecDeque<CapturedFrame>,
    stopped: bool,
    terminated: bool,
    next_sequence: u64,
    next_source_id: u32,
    last_observed_micros: u64,
    registration_open: bool,
    #[cfg(test)]
    waiting_consumers: usize,
    metrics: MultiSourceFanInMetrics,
}

#[derive(Debug)]
struct SharedFanIn {
    state: Mutex<FanInState>,
    changed: Condvar,
    started: Instant,
    queue_capacity: usize,
    max_queue_bytes: usize,
    max_sources: usize,
    leased_registration: bool,
}

/// Private raw-frame fan-in intended to sit beneath one shared signature
/// filter. It never spawns workers and has one aggregate queue, so callers may
/// attach one external reader worker per source without multiplying buffers.
#[derive(Debug)]
pub(crate) struct MultiSourceFanIn {
    shared: Arc<SharedFanIn>,
    metadata: CaptureSourceMetadata,
}

impl MultiSourceFanIn {
    #[allow(
        dead_code,
        reason = "legacy static construction remains available while Windows uses the leased lifecycle"
    )]
    pub(crate) fn new(
        max_sources: usize,
        queue_capacity: usize,
        max_queue_bytes: usize,
        link_types: Vec<CaptureLinkType>,
    ) -> Result<Self, CaptureError> {
        Self::build(
            max_sources,
            queue_capacity,
            max_queue_bytes,
            link_types,
            false,
        )
    }

    pub(crate) fn new_with_registration_lease(
        max_sources: usize,
        queue_capacity: usize,
        max_queue_bytes: usize,
        link_types: Vec<CaptureLinkType>,
    ) -> Result<(Self, MultiSourceRegistrationLease), CaptureError> {
        let source = Self::build(
            max_sources,
            queue_capacity,
            max_queue_bytes,
            link_types,
            true,
        )?;
        let lease = MultiSourceRegistrationLease {
            shared: Arc::clone(&source.shared),
            closed: false,
        };
        Ok((source, lease))
    }

    fn build(
        max_sources: usize,
        queue_capacity: usize,
        max_queue_bytes: usize,
        link_types: Vec<CaptureLinkType>,
        leased_registration: bool,
    ) -> Result<Self, CaptureError> {
        if max_sources == 0 {
            return Err(configuration_error(
                "source limit must be greater than zero",
            ));
        }
        if max_sources > MAX_MULTI_SOURCE_FAN_IN_SOURCES {
            return Err(configuration_error(format!(
                "source limit exceeds the hard maximum of {MAX_MULTI_SOURCE_FAN_IN_SOURCES}"
            )));
        }
        if queue_capacity == 0 {
            return Err(configuration_error(
                "aggregate queue capacity must be greater than zero",
            ));
        }
        if queue_capacity > MAX_MULTI_SOURCE_FAN_IN_QUEUE_FRAMES {
            return Err(configuration_error(format!(
                "aggregate queue capacity exceeds the hard maximum of {MAX_MULTI_SOURCE_FAN_IN_QUEUE_FRAMES} frames"
            )));
        }
        if max_queue_bytes == 0 {
            return Err(configuration_error(
                "aggregate queue byte limit must be greater than zero",
            ));
        }
        if max_queue_bytes > MAX_MULTI_SOURCE_FAN_IN_QUEUE_BYTES {
            return Err(configuration_error(format!(
                "aggregate queue byte limit exceeds the hard maximum of {MAX_MULTI_SOURCE_FAN_IN_QUEUE_BYTES} bytes"
            )));
        }
        Ok(Self {
            shared: Arc::new(SharedFanIn {
                state: Mutex::new(FanInState {
                    // Grow only as bounded frames arrive; configuration alone
                    // must not trigger a large eager allocation.
                    queue: VecDeque::new(),
                    stopped: false,
                    terminated: false,
                    next_sequence: 1,
                    next_source_id: 0,
                    last_observed_micros: 0,
                    registration_open: leased_registration,
                    #[cfg(test)]
                    waiting_consumers: 0,
                    metrics: MultiSourceFanInMetrics::default(),
                }),
                changed: Condvar::new(),
                started: Instant::now(),
                queue_capacity,
                max_queue_bytes,
                max_sources,
                leased_registration,
            }),
            metadata: CaptureSourceMetadata {
                source_id: "private-multi-source-fan-in".into(),
                display_name: "Private bounded multi-source capture fan-in".into(),
                kind: CaptureSourceKind::Live,
                link_types,
                file_format: None,
            },
        })
    }

    #[allow(
        dead_code,
        reason = "legacy static registration remains available beside the coordinator lease"
    )]
    pub(crate) fn register_source(&self) -> Result<MultiSourceIngress, MultiSourceRegisterError> {
        register_source(&self.shared)
    }

    pub(crate) fn stop_handle(&self) -> MultiSourceFanInStopHandle {
        MultiSourceFanInStopHandle {
            shared: Arc::clone(&self.shared),
        }
    }

    pub(crate) fn metrics(&self) -> MultiSourceFanInMetrics {
        self.shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .metrics
    }
}

impl CaptureSource for MultiSourceFanIn {
    fn metadata(&self) -> &CaptureSourceMetadata {
        &self.metadata
    }

    fn next_frame(&mut self) -> Result<Option<CapturedFrame>, CaptureError> {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if state.terminated {
                return Ok(None);
            }
            if let Some(frame) = state.queue.pop_front() {
                state.metrics.queued_bytes =
                    state.metrics.queued_bytes.saturating_sub(frame.bytes.len());
                state.metrics.delivered_frames = state.metrics.delivered_frames.saturating_add(1);
                self.shared.changed.notify_all();
                return Ok(Some(frame));
            }
            if state.stopped {
                state.terminated = true;
                return Ok(None);
            }
            let registration_closed = !self.shared.leased_registration || !state.registration_open;
            if registration_closed
                && state.metrics.active_sources == 0
                && (state.metrics.registered_sources > 0 || self.shared.leased_registration)
            {
                state.terminated = true;
                if state.metrics.registered_sources == 0 {
                    return Err(CaptureError::Adapter {
                        adapter: "private-multi-source-fan-in".into(),
                        message: "capture source registration closed without any sources".into(),
                    });
                }
                if state.metrics.failed_sources == state.metrics.registered_sources {
                    return Err(CaptureError::Adapter {
                        adapter: "private-multi-source-fan-in".into(),
                        message: format!(
                            "all {} registered capture sources failed",
                            state.metrics.failed_sources
                        ),
                    });
                }
                return Ok(None);
            }
            #[cfg(test)]
            {
                state.waiting_consumers += 1;
                self.shared.changed.notify_all();
            }
            state = self
                .shared
                .changed
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            #[cfg(test)]
            {
                state.waiting_consumers = state.waiting_consumers.saturating_sub(1);
            }
        }
    }
}

impl Drop for MultiSourceFanIn {
    fn drop(&mut self) {
        self.stop_handle().request_stop();
    }
}

#[derive(Debug)]
pub(crate) struct MultiSourceRegistrationLease {
    shared: Arc<SharedFanIn>,
    closed: bool,
}

impl MultiSourceRegistrationLease {
    pub(crate) fn register_source(&self) -> Result<MultiSourceIngress, MultiSourceRegisterError> {
        register_source(&self.shared)
    }

    pub(crate) fn close(mut self) {
        self.close_inner();
    }

    #[cfg(test)]
    fn wait_until_consumer_is_blocked(&self) {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while state.waiting_consumers == 0 {
            state = self
                .shared
                .changed
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    fn close_inner(&mut self) {
        if self.closed {
            return;
        }
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.closed = true;
        state.registration_open = false;
        self.shared.changed.notify_all();
    }
}

impl Drop for MultiSourceRegistrationLease {
    fn drop(&mut self) {
        self.close_inner();
    }
}

fn register_source(
    shared: &Arc<SharedFanIn>,
) -> Result<MultiSourceIngress, MultiSourceRegisterError> {
    let mut state = shared
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if state.stopped || state.terminated || (shared.leased_registration && !state.registration_open)
    {
        return Err(MultiSourceRegisterError::Stopped);
    }
    if state.metrics.active_sources >= shared.max_sources {
        return Err(MultiSourceRegisterError::MaximumSources);
    }
    let source_id = state.next_source_id;
    state.next_source_id = state
        .next_source_id
        .checked_add(1)
        .ok_or(MultiSourceRegisterError::SourceIdentityExhausted)?;
    state.metrics.registered_sources = state.metrics.registered_sources.saturating_add(1);
    state.metrics.active_sources += 1;
    Ok(MultiSourceIngress {
        shared: Arc::clone(shared),
        source_id,
        finished: false,
    })
}

#[derive(Debug)]
pub(crate) struct MultiSourceIngress {
    shared: Arc<SharedFanIn>,
    source_id: u32,
    finished: bool,
}

impl MultiSourceIngress {
    /// Attempts one insertion into the shared hard-bounded queue.
    ///
    /// Every rejection returns ownership of the frame to the caller. The
    /// caller may retain it for a bounded retry, but this core does not retain
    /// oversized frames or frames that would exceed either queue limit.
    pub(crate) fn try_push(
        &mut self,
        mut frame: CapturedFrame,
    ) -> Result<(), MultiSourcePushError> {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.finished || state.stopped || state.terminated {
            return Err(MultiSourcePushError::Stopped(frame));
        }
        let frame_bytes = frame.bytes.len();
        if frame_bytes > self.shared.max_queue_bytes {
            state.metrics.oversized_frame_rejections =
                state.metrics.oversized_frame_rejections.saturating_add(1);
            return Err(MultiSourcePushError::FrameTooLarge(frame));
        }
        if state.queue.len() >= self.shared.queue_capacity {
            state.metrics.queue_full_rejections =
                state.metrics.queue_full_rejections.saturating_add(1);
            return Err(MultiSourcePushError::QueueFull(frame));
        }
        let Some(next_queued_bytes) = state.metrics.queued_bytes.checked_add(frame_bytes) else {
            state.metrics.byte_full_rejections =
                state.metrics.byte_full_rejections.saturating_add(1);
            return Err(MultiSourcePushError::QueueBytesFull(frame));
        };
        if next_queued_bytes > self.shared.max_queue_bytes {
            state.metrics.byte_full_rejections =
                state.metrics.byte_full_rejections.saturating_add(1);
            return Err(MultiSourcePushError::QueueBytesFull(frame));
        }
        let sequence = state.next_sequence;
        let Some(next_sequence) = sequence.checked_add(1) else {
            state.stopped = true;
            self.shared.changed.notify_all();
            return Err(MultiSourcePushError::SequenceExhausted(frame));
        };
        let observed_micros = self
            .shared
            .started
            .elapsed()
            .as_micros()
            .try_into()
            .unwrap_or(u64::MAX)
            .max(state.last_observed_micros);
        frame.sequence = sequence;
        frame.observed_micros = observed_micros;
        frame.interface_id = Some(self.source_id);
        state.next_sequence = next_sequence;
        state.last_observed_micros = observed_micros;
        state.queue.push_back(frame);
        state.metrics.queued_bytes = next_queued_bytes;
        state.metrics.accepted_frames = state.metrics.accepted_frames.saturating_add(1);
        state.metrics.peak_queued_frames = state.metrics.peak_queued_frames.max(state.queue.len());
        state.metrics.peak_queued_bytes = state
            .metrics
            .peak_queued_bytes
            .max(state.metrics.queued_bytes);
        self.shared.changed.notify_one();
        Ok(())
    }

    pub(crate) fn stop_requested(&self) -> bool {
        let state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.stopped || state.terminated
    }

    pub(crate) fn finish(&mut self) {
        self.finish_with(false);
    }

    pub(crate) fn fail(&mut self) {
        self.finish_with(true);
    }

    fn finish_with(&mut self, failed: bool) {
        if self.finished {
            return;
        }
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.finished = true;
        state.metrics.active_sources = state.metrics.active_sources.saturating_sub(1);
        if failed {
            state.metrics.failed_sources = state.metrics.failed_sources.saturating_add(1);
        } else {
            state.metrics.completed_sources = state.metrics.completed_sources.saturating_add(1);
        }
        self.shared.changed.notify_all();
    }
}

impl Drop for MultiSourceIngress {
    fn drop(&mut self) {
        self.finish();
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MultiSourceFanInStopHandle {
    shared: Arc<SharedFanIn>,
}

impl MultiSourceFanInStopHandle {
    pub(crate) fn request_stop(&self) {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.stopped = true;
        state.registration_open = false;
        self.shared.changed.notify_all();
    }
}

fn configuration_error(message: impl Into<String>) -> CaptureError {
    CaptureError::Adapter {
        adapter: "private-multi-source-fan-in".into(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::mpsc, thread, time::Duration};

    use bytes::Bytes;

    use super::*;
    use crate::{CaptureLinkType, TimestampNormalization};

    fn frame(source_sequence: u64, source_observed_micros: u64, byte: u8) -> CapturedFrame {
        frame_bytes(source_sequence, source_observed_micros, vec![byte])
    }

    fn frame_bytes(
        source_sequence: u64,
        source_observed_micros: u64,
        bytes: Vec<u8>,
    ) -> CapturedFrame {
        CapturedFrame {
            sequence: source_sequence,
            observed_micros: source_observed_micros,
            source_timestamp_nanos: None,
            timestamp_normalization: TimestampNormalization::Exact,
            interface_id: None,
            link_type: CaptureLinkType::Ethernet,
            original_length: bytes.len().try_into().unwrap(),
            bytes: Bytes::from(bytes),
        }
    }

    #[test]
    fn configuration_accepts_exact_hard_ceilings_without_eager_queue_allocation() {
        let fan_in = MultiSourceFanIn::new(
            MAX_MULTI_SOURCE_FAN_IN_SOURCES,
            MAX_MULTI_SOURCE_FAN_IN_QUEUE_FRAMES,
            MAX_MULTI_SOURCE_FAN_IN_QUEUE_BYTES,
            vec![CaptureLinkType::Ethernet],
        )
        .unwrap();

        assert_eq!(fan_in.metrics().queued_bytes, 0);
        assert_eq!(fan_in.metrics().peak_queued_frames, 0);
    }

    #[test]
    fn zero_and_above_ceiling_configurations_are_rejected() {
        for result in [
            MultiSourceFanIn::new(0, 1, 1, vec![]),
            MultiSourceFanIn::new(1, 0, 1, vec![]),
            MultiSourceFanIn::new(1, 1, 0, vec![]),
            MultiSourceFanIn::new(MAX_MULTI_SOURCE_FAN_IN_SOURCES + 1, 1, 1, vec![]),
            MultiSourceFanIn::new(1, MAX_MULTI_SOURCE_FAN_IN_QUEUE_FRAMES + 1, 1, vec![]),
            MultiSourceFanIn::new(1, 1, MAX_MULTI_SOURCE_FAN_IN_QUEUE_BYTES + 1, vec![]),
        ] {
            assert!(result.is_err());
        }
    }

    #[test]
    fn exact_byte_cap_is_accepted_and_one_byte_over_is_never_retained() {
        let mut fan_in = MultiSourceFanIn::new(1, 2, 4, vec![CaptureLinkType::Ethernet]).unwrap();
        let mut source = fan_in.register_source().unwrap();
        source.try_push(frame_bytes(1, 1, vec![1; 4])).unwrap();
        assert_eq!(fan_in.metrics().queued_bytes, 4);
        assert_eq!(fan_in.next_frame().unwrap().unwrap().bytes.len(), 4);
        assert_eq!(fan_in.metrics().queued_bytes, 0);

        let error = source.try_push(frame_bytes(2, 2, vec![2; 5])).unwrap_err();
        let MultiSourcePushError::FrameTooLarge(returned) = error else {
            unreachable!();
        };
        assert_eq!(returned.bytes.len(), 5);
        assert_eq!(fan_in.metrics().queued_bytes, 0);
        assert_eq!(fan_in.metrics().oversized_frame_rejections, 1);
    }

    #[test]
    fn aggregate_byte_full_rejection_accounts_again_after_pop() {
        let mut fan_in = MultiSourceFanIn::new(2, 4, 4, vec![CaptureLinkType::Ethernet]).unwrap();
        let mut first = fan_in.register_source().unwrap();
        let mut second = fan_in.register_source().unwrap();
        first.try_push(frame_bytes(1, 1, vec![1; 3])).unwrap();
        let error = second.try_push(frame_bytes(1, 1, vec![2; 2])).unwrap_err();
        assert!(matches!(error, MultiSourcePushError::QueueBytesFull(_)));
        assert_eq!(fan_in.metrics().queued_bytes, 3);
        assert_eq!(fan_in.metrics().peak_queued_bytes, 3);
        assert_eq!(fan_in.metrics().byte_full_rejections, 1);

        assert_eq!(fan_in.next_frame().unwrap().unwrap().bytes.len(), 3);
        assert_eq!(fan_in.metrics().queued_bytes, 0);
        let MultiSourcePushError::QueueBytesFull(retry) = error else {
            unreachable!();
        };
        second.try_push(retry).unwrap();
        assert_eq!(fan_in.metrics().queued_bytes, 2);
    }

    #[test]
    fn interleaved_sources_receive_one_global_order_and_session_clock() {
        let mut fan_in =
            MultiSourceFanIn::new(2, 4, 1_024, vec![CaptureLinkType::Ethernet]).unwrap();
        let mut first = fan_in.register_source().unwrap();
        let mut second = fan_in.register_source().unwrap();
        first.try_push(frame(50, 50_000, 1)).unwrap();
        second.try_push(frame(2, 1, 2)).unwrap();

        let first_frame = fan_in.next_frame().unwrap().unwrap();
        let second_frame = fan_in.next_frame().unwrap().unwrap();
        assert_eq!((first_frame.sequence, second_frame.sequence), (1, 2));
        assert!(first_frame.observed_micros <= second_frame.observed_micros);
        assert_eq!(
            (first_frame.interface_id, second_frame.interface_id),
            (Some(0), Some(1))
        );
        assert_eq!((first_frame.bytes[0], second_frame.bytes[0]), (1, 2));
    }

    #[test]
    fn one_global_queue_applies_backpressure_to_every_source() {
        let mut fan_in =
            MultiSourceFanIn::new(2, 1, 1_024, vec![CaptureLinkType::Ethernet]).unwrap();
        let mut first = fan_in.register_source().unwrap();
        let mut second = fan_in.register_source().unwrap();
        first.try_push(frame(1, 1, 1)).unwrap();
        let rejected = second.try_push(frame(1, 1, 2)).unwrap_err();
        assert!(matches!(rejected, MultiSourcePushError::QueueFull(_)));
        assert_eq!(fan_in.metrics().peak_queued_frames, 1);
        assert_eq!(fan_in.metrics().queue_full_rejections, 1);

        assert_eq!(fan_in.next_frame().unwrap().unwrap().bytes[0], 1);
        let MultiSourcePushError::QueueFull(retry) = rejected else {
            unreachable!();
        };
        second.try_push(retry).unwrap();
        assert_eq!(fan_in.next_frame().unwrap().unwrap().bytes[0], 2);
    }

    #[test]
    fn failed_child_is_isolated_while_another_source_remains() {
        let mut fan_in =
            MultiSourceFanIn::new(2, 2, 1_024, vec![CaptureLinkType::Ethernet]).unwrap();
        let mut failed = fan_in.register_source().unwrap();
        let mut healthy = fan_in.register_source().unwrap();
        failed.fail();
        healthy.try_push(frame(1, 1, 7)).unwrap();
        healthy.finish();

        assert_eq!(fan_in.next_frame().unwrap().unwrap().bytes[0], 7);
        assert_eq!(fan_in.next_frame().unwrap(), None);
        assert_eq!(fan_in.metrics().failed_sources, 1);
        assert_eq!(fan_in.metrics().completed_sources, 1);
    }

    #[test]
    fn all_failed_children_produce_one_terminal_error_after_queued_frames() {
        let mut fan_in =
            MultiSourceFanIn::new(2, 2, 1_024, vec![CaptureLinkType::Ethernet]).unwrap();
        let mut first = fan_in.register_source().unwrap();
        let mut second = fan_in.register_source().unwrap();
        first.try_push(frame(1, 1, 9)).unwrap();
        first.fail();
        second.fail();

        assert_eq!(fan_in.next_frame().unwrap().unwrap().bytes[0], 9);
        let error = fan_in.next_frame().unwrap_err();
        assert!(
            error
                .to_string()
                .contains("all 2 registered capture sources failed")
        );
        assert_eq!(fan_in.next_frame().unwrap(), None);
    }

    #[test]
    fn registration_lease_keeps_temporary_zero_reader_gap_alive() {
        let (mut fan_in, lease) = MultiSourceFanIn::new_with_registration_lease(
            2,
            2,
            1_024,
            vec![CaptureLinkType::Ethernet],
        )
        .unwrap();
        let mut first = lease.register_source().unwrap();
        first.finish();
        let (results, received) = mpsc::sync_channel(2);
        let consumer = thread::spawn(move || {
            results.send(fan_in.next_frame()).unwrap();
            results.send(fan_in.next_frame()).unwrap();
        });

        lease.wait_until_consumer_is_blocked();
        assert!(matches!(
            received.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        let mut replacement = lease.register_source().unwrap();
        replacement.try_push(frame(1, 1, 8)).unwrap();
        replacement.finish();
        assert_eq!(
            received
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .unwrap()
                .unwrap()
                .bytes[0],
            8
        );
        lease.wait_until_consumer_is_blocked();
        assert!(matches!(
            received.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        lease.close();
        assert_eq!(
            received
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .unwrap(),
            None
        );
        consumer.join().unwrap();
    }

    #[test]
    fn closing_registration_drains_then_preserves_all_failed_error() {
        let (mut fan_in, lease) = MultiSourceFanIn::new_with_registration_lease(
            2,
            2,
            1_024,
            vec![CaptureLinkType::Ethernet],
        )
        .unwrap();
        let mut first = lease.register_source().unwrap();
        let mut second = lease.register_source().unwrap();
        first.try_push(frame(1, 1, 9)).unwrap();
        first.fail();
        second.fail();
        lease.close();

        assert_eq!(fan_in.next_frame().unwrap().unwrap().bytes[0], 9);
        assert!(
            fan_in
                .next_frame()
                .unwrap_err()
                .to_string()
                .contains("all 2 registered capture sources failed")
        );
        assert_eq!(fan_in.next_frame().unwrap(), None);
    }

    #[test]
    fn closing_unused_registration_returns_one_terminal_error() {
        let (mut fan_in, lease) = MultiSourceFanIn::new_with_registration_lease(
            1,
            1,
            1_024,
            vec![CaptureLinkType::Ethernet],
        )
        .unwrap();
        lease.close();

        assert!(
            fan_in
                .next_frame()
                .unwrap_err()
                .to_string()
                .contains("registration closed without any sources")
        );
        assert_eq!(fan_in.next_frame().unwrap(), None);
    }

    #[test]
    fn user_stop_closes_registration_and_refuses_late_reader_race() {
        let (fan_in, lease) = MultiSourceFanIn::new_with_registration_lease(
            1,
            1,
            1_024,
            vec![CaptureLinkType::Ethernet],
        )
        .unwrap();
        fan_in.stop_handle().request_stop();

        assert_eq!(
            lease.register_source().unwrap_err(),
            MultiSourceRegisterError::Stopped
        );
    }

    #[test]
    fn stop_refuses_late_sources_and_frames_without_losing_queued_work() {
        let mut fan_in =
            MultiSourceFanIn::new(2, 2, 1_024, vec![CaptureLinkType::Ethernet]).unwrap();
        let mut source = fan_in.register_source().unwrap();
        source.try_push(frame(1, 1, 4)).unwrap();
        let stop = fan_in.stop_handle();
        stop.request_stop();

        assert_eq!(
            fan_in.register_source().unwrap_err(),
            MultiSourceRegisterError::Stopped
        );
        assert!(source.stop_requested());
        assert!(matches!(
            source.try_push(frame(2, 2, 5)),
            Err(MultiSourcePushError::Stopped(_))
        ));
        assert_eq!(fan_in.next_frame().unwrap().unwrap().bytes[0], 4);
        assert_eq!(fan_in.next_frame().unwrap(), None);
    }

    #[test]
    fn source_limit_is_concurrent_and_registration_after_terminal_is_refused() {
        let mut fan_in =
            MultiSourceFanIn::new(2, 1, 1_024, vec![CaptureLinkType::Ethernet]).unwrap();
        let mut first = fan_in.register_source().unwrap();
        let mut second = fan_in.register_source().unwrap();
        assert_eq!(
            fan_in.register_source().unwrap_err(),
            MultiSourceRegisterError::MaximumSources
        );
        first.finish();
        let mut replacement = fan_in.register_source().unwrap();
        second.finish();
        replacement.finish();
        assert_eq!(fan_in.next_frame().unwrap(), None);
        assert_eq!(
            fan_in.register_source().unwrap_err(),
            MultiSourceRegisterError::Stopped
        );
    }
}
