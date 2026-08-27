//! Platform-neutral packet capture and replay contracts.

mod dumpcap;
mod offline;
mod pcap_writer;
mod process_filter;
#[cfg(windows)]
mod recording;
#[cfg(windows)]
mod windows;

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use dumpcap::{DumpcapLiveConfig, LiveCaptureStopHandle};
pub use offline::OfflineCapture;
pub use pcap_writer::{PcapWriteError, PcapWriter};
pub use process_filter::{
    OwnedProcessCapture, OwnedProcessCaptureConfig, OwnedProcessCaptureConfigError,
    OwnedProcessCaptureMetrics, ProcessSocketOwner, TcpConnection, TcpEndpoint,
};
#[cfg(windows)]
pub use recording::{
    OwnedCaptureRecordingError, OwnedCaptureRecordingResult, record_owned_capture_to_files,
};
#[cfg(windows)]
pub use windows::{
    WindowsCaptureAdapter, WindowsCaptureAdapterRecommendation,
    WindowsCaptureAdapterRecommendationSource, WindowsOwnedDumpcapCapture,
    WindowsProcessSocketOwner, recommend_windows_capture_adapter, windows_capture_adapters,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureSourceKind {
    Live,
    Replay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureLinkType {
    NullLoopback,
    Ethernet,
    RawIp,
    RawIpv4,
    RawIpv6,
    LinuxCookedV1,
    LinuxCookedV2,
    Unknown(i32),
}

impl CaptureLinkType {
    pub fn from_pcap_link_type(link_type: i32) -> Self {
        match link_type {
            0 | 108 => Self::NullLoopback,
            1 => Self::Ethernet,
            101 => Self::RawIp,
            113 => Self::LinuxCookedV1,
            228 => Self::RawIpv4,
            229 => Self::RawIpv6,
            276 => Self::LinuxCookedV2,
            other => Self::Unknown(other),
        }
    }

    pub const fn to_pcap_link_type(self) -> Option<i32> {
        match self {
            Self::NullLoopback => Some(0),
            Self::Ethernet => Some(1),
            Self::RawIp => Some(101),
            Self::LinuxCookedV1 => Some(113),
            Self::RawIpv4 => Some(228),
            Self::RawIpv6 => Some(229),
            Self::LinuxCookedV2 => Some(276),
            Self::Unknown(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureFileFormat {
    Pcap,
    PcapNg,
    RlogsEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureSourceMetadata {
    pub source_id: String,
    pub display_name: String,
    pub kind: CaptureSourceKind,
    /// Filled as interfaces are discovered. Pcapng may contain more than one.
    pub link_types: Vec<CaptureLinkType>,
    pub file_format: Option<CaptureFileFormat>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimestampNormalization {
    Exact,
    ClampedBackward,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapturedFrame {
    /// Monotonic sequence assigned by the adapter, beginning at one.
    pub sequence: u64,
    /// Monotonic time relative to the start of this capture.
    pub observed_micros: u64,
    /// Original wall-clock timestamp retained independently from replay timing.
    pub source_timestamp_nanos: Option<i64>,
    pub timestamp_normalization: TimestampNormalization,
    /// Pcapng interface index. Legacy pcap has one implicit interface.
    pub interface_id: Option<u32>,
    pub link_type: CaptureLinkType,
    /// The on-wire size before capture snap-length truncation.
    pub original_length: u32,
    /// Shared immutable frame storage. Capture adapters perform the only
    /// required ownership copy; downstream layers take O(1) slices.
    pub bytes: Bytes,
}

pub trait CaptureSource: Send {
    fn metadata(&self) -> &CaptureSourceMetadata;

    /// Returns `Ok(None)` only after a replay ends or a live source shuts down.
    fn next_frame(&mut self) -> Result<Option<CapturedFrame>, CaptureError>;
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CaptureError {
    #[error("capture adapter {adapter} failed: {message}")]
    Adapter { adapter: String, message: String },

    #[error("could not open offline capture: {message}")]
    ReplayOpen { message: String },

    #[error("invalid offline capture: {message}")]
    InvalidReplay { message: String },

    #[error("capture adapter emitted invalid sequence: expected {expected}, received {actual}")]
    InvalidSequence { expected: u64, actual: u64 },

    #[error("capture time moved backward from {previous_micros}us to {next_micros}us")]
    TimeMovedBackward {
        previous_micros: u64,
        next_micros: u64,
    },
}

/// Enforces ordering before frames enter reconstruction or protocol decoding.
#[derive(Debug)]
pub struct ValidatedCapture<S> {
    source: S,
    next_sequence: u64,
    previous_micros: Option<u64>,
}

impl<S: CaptureSource> ValidatedCapture<S> {
    pub fn new(source: S) -> Self {
        Self {
            source,
            next_sequence: 1,
            previous_micros: None,
        }
    }

    pub fn metadata(&self) -> &CaptureSourceMetadata {
        self.source.metadata()
    }

    pub fn next_frame(&mut self) -> Result<Option<CapturedFrame>, CaptureError> {
        let Some(frame) = self.source.next_frame()? else {
            return Ok(None);
        };

        if frame.sequence != self.next_sequence {
            return Err(CaptureError::InvalidSequence {
                expected: self.next_sequence,
                actual: frame.sequence,
            });
        }

        if let Some(previous_micros) = self.previous_micros {
            if frame.observed_micros < previous_micros {
                return Err(CaptureError::TimeMovedBackward {
                    previous_micros,
                    next_micros: frame.observed_micros,
                });
            }
        }

        self.next_sequence += 1;
        self.previous_micros = Some(frame.observed_micros);
        Ok(Some(frame))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    struct FixtureCapture {
        metadata: CaptureSourceMetadata,
        frames: VecDeque<CapturedFrame>,
    }

    impl CaptureSource for FixtureCapture {
        fn metadata(&self) -> &CaptureSourceMetadata {
            &self.metadata
        }

        fn next_frame(&mut self) -> Result<Option<CapturedFrame>, CaptureError> {
            Ok(self.frames.pop_front())
        }
    }

    fn frame(sequence: u64, observed_micros: u64) -> CapturedFrame {
        CapturedFrame {
            sequence,
            observed_micros,
            source_timestamp_nanos: Some(observed_micros as i64 * 1_000),
            timestamp_normalization: TimestampNormalization::Exact,
            interface_id: None,
            link_type: CaptureLinkType::Ethernet,
            original_length: 3,
            bytes: Bytes::from_static(&[1, 2, 3]),
        }
    }

    fn fixture(frames: Vec<CapturedFrame>) -> FixtureCapture {
        FixtureCapture {
            metadata: CaptureSourceMetadata {
                source_id: "fixture".into(),
                display_name: "test fixture".into(),
                kind: CaptureSourceKind::Replay,
                link_types: vec![CaptureLinkType::Ethernet],
                file_format: None,
            },
            frames: frames.into(),
        }
    }

    #[test]
    fn live_and_replay_sources_share_one_validated_boundary() {
        let mut capture = ValidatedCapture::new(fixture(vec![frame(1, 10), frame(2, 10)]));

        assert_eq!(capture.next_frame().unwrap(), Some(frame(1, 10)));
        assert_eq!(capture.next_frame().unwrap(), Some(frame(2, 10)));
        assert_eq!(capture.next_frame().unwrap(), None);
    }

    #[test]
    fn a_missing_frame_is_reported_before_protocol_processing() {
        let mut capture = ValidatedCapture::new(fixture(vec![frame(2, 10)]));

        assert_eq!(
            capture.next_frame(),
            Err(CaptureError::InvalidSequence {
                expected: 1,
                actual: 2,
            })
        );
    }

    #[test]
    fn capture_time_cannot_move_backward() {
        let mut capture = ValidatedCapture::new(fixture(vec![frame(1, 20), frame(2, 19)]));
        capture.next_frame().unwrap();

        assert_eq!(
            capture.next_frame(),
            Err(CaptureError::TimeMovedBackward {
                previous_micros: 20,
                next_micros: 19,
            })
        );
    }
}
