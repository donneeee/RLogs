use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::thread::{self, JoinHandle};

use crate::{CaptureError, CaptureSource, CapturedFrame};

/// A point-in-time view of the bounded capture-ingress pipeline.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BoundedCaptureIngressMetrics {
    /// Frames read from the capture adapter and accepted by the queue.
    pub source_frames: u64,
    /// Frames delivered to the ordered decoder.
    pub delivered_frames: u64,
    /// Times the queue was full and capture intake had to wait for the decoder.
    pub queue_saturations: u64,
}

#[derive(Debug, Default)]
struct SharedMetrics {
    source_frames: AtomicU64,
    delivered_frames: AtomicU64,
    queue_saturations: AtomicU64,
}

impl SharedMetrics {
    fn snapshot(&self) -> BoundedCaptureIngressMetrics {
        BoundedCaptureIngressMetrics {
            source_frames: self.source_frames.load(Ordering::Relaxed),
            delivered_frames: self.delivered_frames.load(Ordering::Relaxed),
            queue_saturations: self.queue_saturations.load(Ordering::Relaxed),
        }
    }
}

type IngressMessage<C> = Result<Option<(CapturedFrame, C)>, CaptureError>;

/// Drains a capture adapter on its own thread into a bounded ordered queue.
///
/// Protocol decoding can expand one network frame into thousands of canonical
/// events. Keeping capture intake independent lets the OS/Npcap buffer drain
/// while that ordered expansion is reduced. The queue remains bounded and
/// applies lossless backpressure when the decoder falls behind.
pub struct BoundedCaptureIngress<C> {
    receiver: Option<Receiver<IngressMessage<C>>>,
    worker: Option<JoinHandle<()>>,
    metrics: Arc<SharedMetrics>,
    finished: bool,
}

impl<C: Send + 'static> BoundedCaptureIngress<C> {
    pub fn spawn<S, F>(
        mut source: S,
        queue_capacity: usize,
        mut frame_context: F,
    ) -> Result<Self, CaptureError>
    where
        S: CaptureSource + 'static,
        F: FnMut(&S) -> C + Send + 'static,
    {
        if queue_capacity == 0 {
            return Err(CaptureError::Adapter {
                adapter: "bounded-capture-ingress".into(),
                message: "queue capacity must be greater than zero".into(),
            });
        }

        let (sender, receiver) = sync_channel(queue_capacity);
        let metrics = Arc::new(SharedMetrics::default());
        let worker_metrics = Arc::clone(&metrics);
        let worker = thread::Builder::new()
            .name("rlogs-capture-ingress".into())
            .spawn(move || {
                loop {
                    let message = match source.next_frame() {
                        Ok(Some(frame)) => {
                            let context = frame_context(&source);
                            Ok(Some((frame, context)))
                        }
                        Ok(None) => Ok(None),
                        Err(error) => Err(error),
                    };
                    let terminal = !matches!(message, Ok(Some(_)));
                    if send_bounded(&sender, message, &worker_metrics).is_err() {
                        break;
                    }
                    if terminal {
                        break;
                    }
                }
            })
            .map_err(|error| CaptureError::Adapter {
                adapter: "bounded-capture-ingress".into(),
                message: format!("could not start capture intake worker: {error}"),
            })?;

        Ok(Self {
            receiver: Some(receiver),
            worker: Some(worker),
            metrics,
            finished: false,
        })
    }

    pub fn next_frame(&mut self) -> Result<Option<(CapturedFrame, C)>, CaptureError> {
        if self.finished {
            return Ok(None);
        }
        let receiver = self
            .receiver
            .as_ref()
            .expect("active capture ingress retains its receiver");
        match receiver.recv() {
            Ok(Ok(Some((frame, context)))) => {
                self.metrics
                    .delivered_frames
                    .fetch_add(1, Ordering::Relaxed);
                Ok(Some((frame, context)))
            }
            Ok(Ok(None)) => {
                self.finish_worker();
                Ok(None)
            }
            Ok(Err(error)) => {
                self.finish_worker();
                Err(error)
            }
            Err(_) => {
                self.finish_worker();
                Err(CaptureError::Adapter {
                    adapter: "bounded-capture-ingress".into(),
                    message: "capture intake worker stopped without a terminal result".into(),
                })
            }
        }
    }

    pub fn metrics(&self) -> BoundedCaptureIngressMetrics {
        self.metrics.snapshot()
    }

    fn finish_worker(&mut self) {
        self.finished = true;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl<C> Drop for BoundedCaptureIngress<C> {
    fn drop(&mut self) {
        // Dropping the receiver releases a producer blocked by a full queue.
        // A live adapter may still be inside its short read timeout, so do not
        // synchronously join it from an error/unwind path.
        self.receiver.take();
    }
}

fn send_bounded<C>(
    sender: &SyncSender<IngressMessage<C>>,
    message: IngressMessage<C>,
    metrics: &SharedMetrics,
) -> Result<(), ()> {
    let is_frame = matches!(message, Ok(Some(_)));
    match sender.try_send(message) {
        Ok(()) => {
            if is_frame {
                metrics.source_frames.fetch_add(1, Ordering::Relaxed);
            }
            Ok(())
        }
        Err(TrySendError::Full(message)) => {
            metrics.queue_saturations.fetch_add(1, Ordering::Relaxed);
            sender.send(message).map_err(|_| ())?;
            if is_frame {
                metrics.source_frames.fetch_add(1, Ordering::Relaxed);
            }
            Ok(())
        }
        Err(TrySendError::Disconnected(_)) => Err(()),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use bytes::Bytes;

    use super::*;
    use crate::{
        CaptureLinkType, CaptureSourceKind, CaptureSourceMetadata, TimestampNormalization,
    };

    struct FixtureCapture {
        metadata: CaptureSourceMetadata,
        next_sequence: u64,
        final_sequence: u64,
        reads: Arc<AtomicUsize>,
        fail_at: Option<u64>,
    }

    impl FixtureCapture {
        fn new(final_sequence: u64, reads: Arc<AtomicUsize>) -> Self {
            Self {
                metadata: CaptureSourceMetadata {
                    source_id: "fixture".into(),
                    display_name: "Fixture".into(),
                    kind: CaptureSourceKind::Replay,
                    link_types: vec![CaptureLinkType::Ethernet],
                    file_format: None,
                },
                next_sequence: 1,
                final_sequence,
                reads,
                fail_at: None,
            }
        }
    }

    impl CaptureSource for FixtureCapture {
        fn metadata(&self) -> &CaptureSourceMetadata {
            &self.metadata
        }

        fn next_frame(&mut self) -> Result<Option<CapturedFrame>, CaptureError> {
            if self.fail_at == Some(self.next_sequence) {
                return Err(CaptureError::Adapter {
                    adapter: "fixture".into(),
                    message: "intentional failure".into(),
                });
            }
            if self.next_sequence > self.final_sequence {
                return Ok(None);
            }
            let sequence = self.next_sequence;
            self.next_sequence += 1;
            self.reads.fetch_add(1, Ordering::SeqCst);
            Ok(Some(CapturedFrame {
                sequence,
                observed_micros: sequence,
                source_timestamp_nanos: None,
                timestamp_normalization: TimestampNormalization::Exact,
                interface_id: Some(0),
                link_type: CaptureLinkType::Ethernet,
                original_length: 1,
                bytes: Bytes::from_static(&[0]),
            }))
        }
    }

    #[test]
    fn capture_intake_runs_ahead_only_to_the_hard_queue_bound() {
        let reads = Arc::new(AtomicUsize::new(0));
        let source = FixtureCapture::new(12, Arc::clone(&reads));
        let mut ingress = BoundedCaptureIngress::spawn(source, 4, |source| source.next_sequence)
            .expect("spawn ingress");

        let deadline = Instant::now() + Duration::from_secs(2);
        while reads.load(Ordering::SeqCst) < 5 && Instant::now() < deadline {
            thread::yield_now();
        }
        assert_eq!(reads.load(Ordering::SeqCst), 5);

        let mut sequences = Vec::new();
        let mut contexts = Vec::new();
        while let Some((frame, context)) = ingress.next_frame().expect("receive frame") {
            sequences.push(frame.sequence);
            contexts.push(context);
        }
        assert_eq!(sequences, (1..=12).collect::<Vec<_>>());
        assert_eq!(contexts, (2..=13).collect::<Vec<_>>());
        let metrics = ingress.metrics();
        assert_eq!(metrics.source_frames, 12);
        assert_eq!(metrics.delivered_frames, 12);
        assert!(metrics.queue_saturations >= 1);
    }

    #[test]
    fn capture_adapter_errors_cross_the_ingress_boundary() {
        let reads = Arc::new(AtomicUsize::new(0));
        let mut source = FixtureCapture::new(3, reads);
        source.fail_at = Some(2);
        let mut ingress = BoundedCaptureIngress::spawn(source, 2, |_| ()).expect("spawn ingress");

        assert_eq!(
            ingress
                .next_frame()
                .expect("first frame")
                .unwrap()
                .0
                .sequence,
            1
        );
        let error = ingress.next_frame().expect_err("source failure");
        assert!(error.to_string().contains("intentional failure"));
        assert!(
            ingress
                .next_frame()
                .expect("finished after error")
                .is_none()
        );
    }
}
