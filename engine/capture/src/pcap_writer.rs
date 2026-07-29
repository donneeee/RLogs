use std::io::{self, Write};

use thiserror::Error;

use crate::{CaptureLinkType, CapturedFrame};

const PCAP_NANOSECOND_MAGIC: u32 = 0xa1b2_3c4d;
const PCAP_VERSION_MAJOR: u16 = 2;
const PCAP_VERSION_MINOR: u16 = 4;
const DEFAULT_SNAPSHOT_LENGTH: u32 = 262_144;

#[derive(Debug)]
pub struct PcapWriter<W> {
    writer: W,
    link_type: CaptureLinkType,
    frames_written: u64,
}

impl<W: Write> PcapWriter<W> {
    pub fn new(mut writer: W, link_type: CaptureLinkType) -> Result<Self, PcapWriteError> {
        let network = link_type
            .to_pcap_link_type()
            .ok_or(PcapWriteError::UnsupportedLinkType(link_type))?;
        writer.write_all(&PCAP_NANOSECOND_MAGIC.to_le_bytes())?;
        writer.write_all(&PCAP_VERSION_MAJOR.to_le_bytes())?;
        writer.write_all(&PCAP_VERSION_MINOR.to_le_bytes())?;
        writer.write_all(&0_i32.to_le_bytes())?;
        writer.write_all(&0_u32.to_le_bytes())?;
        writer.write_all(&DEFAULT_SNAPSHOT_LENGTH.to_le_bytes())?;
        writer.write_all(&network.to_le_bytes())?;

        Ok(Self {
            writer,
            link_type,
            frames_written: 0,
        })
    }

    pub fn write_frame(&mut self, frame: &CapturedFrame) -> Result<(), PcapWriteError> {
        if frame.link_type != self.link_type {
            return Err(PcapWriteError::MixedLinkTypes {
                expected: self.link_type,
                actual: frame.link_type,
            });
        }
        let timestamp_nanos = frame
            .source_timestamp_nanos
            .ok_or(PcapWriteError::MissingTimestamp)?;
        if timestamp_nanos < 0 {
            return Err(PcapWriteError::NegativeTimestamp(timestamp_nanos));
        }
        let timestamp_nanos =
            u64::try_from(timestamp_nanos).map_err(|_| PcapWriteError::TimestampOutOfRange)?;
        let seconds = u32::try_from(timestamp_nanos / 1_000_000_000)
            .map_err(|_| PcapWriteError::TimestampOutOfRange)?;
        let nanos = u32::try_from(timestamp_nanos % 1_000_000_000)
            .map_err(|_| PcapWriteError::TimestampOutOfRange)?;
        let captured_length = u32::try_from(frame.bytes.len())
            .map_err(|_| PcapWriteError::CapturedLengthOutOfRange)?;
        if captured_length > frame.original_length {
            return Err(PcapWriteError::CapturedLengthExceedsOriginal {
                captured: captured_length,
                original: frame.original_length,
            });
        }

        self.writer.write_all(&seconds.to_le_bytes())?;
        self.writer.write_all(&nanos.to_le_bytes())?;
        self.writer.write_all(&captured_length.to_le_bytes())?;
        self.writer
            .write_all(&frame.original_length.to_le_bytes())?;
        self.writer.write_all(&frame.bytes)?;
        self.frames_written = self.frames_written.saturating_add(1);
        Ok(())
    }

    pub fn frames_written(&self) -> u64 {
        self.frames_written
    }

    pub fn flush(&mut self) -> Result<(), PcapWriteError> {
        self.writer.flush().map_err(PcapWriteError::Io)
    }

    pub fn into_inner(self) -> W {
        self.writer
    }
}

#[derive(Debug, Error)]
pub enum PcapWriteError {
    #[error("capture link type {0:?} cannot be represented in pcap")]
    UnsupportedLinkType(CaptureLinkType),

    #[error("pcap cannot mix link types: expected {expected:?}, received {actual:?}")]
    MixedLinkTypes {
        expected: CaptureLinkType,
        actual: CaptureLinkType,
    },

    #[error("captured frame has no source timestamp")]
    MissingTimestamp,

    #[error("captured frame has a negative source timestamp: {0}")]
    NegativeTimestamp(i64),

    #[error("captured frame timestamp is outside pcap range")]
    TimestampOutOfRange,

    #[error("captured frame length does not fit in pcap")]
    CapturedLengthOutOfRange,

    #[error("captured frame length {captured} exceeds its original on-wire length {original}")]
    CapturedLengthExceedsOriginal { captured: u32, original: u32 },

    #[error(transparent)]
    Io(#[from] io::Error),
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use bytes::Bytes;

    use super::*;
    use crate::{OfflineCapture, TimestampNormalization, ValidatedCapture};

    #[test]
    fn written_capture_replays_with_exact_frame_data() {
        let frame = CapturedFrame {
            sequence: 1,
            observed_micros: 0,
            source_timestamp_nanos: Some(1_750_000_000_123_456_789),
            timestamp_normalization: TimestampNormalization::Exact,
            interface_id: Some(0),
            link_type: CaptureLinkType::Ethernet,
            original_length: 4,
            bytes: Bytes::from_static(&[1, 2, 3, 4]),
        };
        let mut output = Vec::new();
        {
            let mut writer = PcapWriter::new(&mut output, CaptureLinkType::Ethernet).unwrap();
            writer.write_frame(&frame).unwrap();
            writer.flush().unwrap();
        }

        let source = OfflineCapture::from_reader("test", "test", Cursor::new(output)).unwrap();
        let mut replay = ValidatedCapture::new(source);
        let replayed = replay.next_frame().unwrap().unwrap();

        assert_eq!(replayed.sequence, 1);
        assert_eq!(
            replayed.source_timestamp_nanos,
            frame.source_timestamp_nanos
        );
        assert_eq!(replayed.original_length, 4);
        assert_eq!(replayed.bytes, frame.bytes);
        assert!(replay.next_frame().unwrap().is_none());
    }
}
