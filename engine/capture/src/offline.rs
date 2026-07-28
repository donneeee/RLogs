use std::{
    fmt,
    fs::File,
    io::{BufReader, Read},
    path::Path,
};

use pcap_parser::{
    Block, PcapBlockOwned, PcapError, create_reader,
    traits::{PcapNGPacketBlock, PcapReaderIterator},
};

use crate::{
    CaptureError, CaptureFileFormat, CaptureLinkType, CaptureSource, CaptureSourceKind,
    CaptureSourceMetadata, CapturedFrame, TimestampNormalization,
};

const INITIAL_READER_CAPACITY: usize = 1024 * 1024;
const MAX_READER_CAPACITY: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
struct InterfaceInfo {
    link_type: CaptureLinkType,
    timestamp_resolution: Option<u64>,
    timestamp_offset_seconds: i64,
}

#[derive(Debug)]
enum BlockEffect {
    PcapHeader {
        link_type: CaptureLinkType,
        nanosecond_precision: bool,
    },
    PcapNgSection,
    PcapNgInterface(InterfaceInfo),
    LegacyPacket {
        seconds: u32,
        fraction: u32,
        original_length: u32,
        bytes: Vec<u8>,
    },
    EnhancedPacket {
        interface_id: u32,
        raw_timestamp: u64,
        original_length: u32,
        bytes: Vec<u8>,
    },
    SimplePacket {
        original_length: u32,
        bytes: Vec<u8>,
    },
    Ignore,
}

enum ReaderOutcome {
    Block { offset: usize, effect: BlockEffect },
    Eof,
    Incomplete,
    BufferTooSmall,
    Error(String),
}

#[derive(Debug, Default)]
struct ReplayClock {
    first_source_nanos: Option<i64>,
    previous_source_nanos: Option<i64>,
    last_observed_micros: u64,
}

impl ReplayClock {
    fn normalize(&mut self, source_timestamp_nanos: Option<i64>) -> (u64, TimestampNormalization) {
        let Some(source_timestamp_nanos) = source_timestamp_nanos else {
            return (
                self.last_observed_micros,
                TimestampNormalization::Unavailable,
            );
        };

        let first_source_nanos = *self
            .first_source_nanos
            .get_or_insert(source_timestamp_nanos);
        let moved_backward = self
            .previous_source_nanos
            .is_some_and(|previous| source_timestamp_nanos < previous);

        let candidate_micros = source_timestamp_nanos
            .checked_sub(first_source_nanos)
            .and_then(|difference| u64::try_from(difference).ok())
            .map(|difference| difference / 1_000)
            .unwrap_or(0);

        let normalization = if moved_backward || candidate_micros < self.last_observed_micros {
            TimestampNormalization::ClampedBackward
        } else {
            TimestampNormalization::Exact
        };
        let observed_micros = candidate_micros.max(self.last_observed_micros);

        self.previous_source_nanos = Some(source_timestamp_nanos);
        self.last_observed_micros = observed_micros;

        (observed_micros, normalization)
    }
}

/// Streaming offline pcap/pcapng source.
///
/// This reader emits raw captured frames only. TCP reconstruction and game
/// protocol decoding remain downstream in the single RLogs pipeline.
pub struct OfflineCapture {
    metadata: CaptureSourceMetadata,
    reader: Box<dyn PcapReaderIterator + Send>,
    reader_capacity: usize,
    interfaces: Vec<InterfaceInfo>,
    legacy_link_type: Option<CaptureLinkType>,
    legacy_nanosecond_precision: Option<bool>,
    next_sequence: u64,
    clock: ReplayClock,
    finished: bool,
}

impl fmt::Debug for OfflineCapture {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OfflineCapture")
            .field("metadata", &self.metadata)
            .field("reader_capacity", &self.reader_capacity)
            .field("interfaces", &self.interfaces)
            .field("legacy_link_type", &self.legacy_link_type)
            .field(
                "legacy_nanosecond_precision",
                &self.legacy_nanosecond_precision,
            )
            .field("next_sequence", &self.next_sequence)
            .field("finished", &self.finished)
            .finish_non_exhaustive()
    }
}

impl OfflineCapture {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, CaptureError> {
        let file = File::open(path.as_ref()).map_err(|error| CaptureError::ReplayOpen {
            message: error.to_string(),
        })?;

        Self::from_reader("offline-pcap", "Offline capture", BufReader::new(file))
    }

    pub fn from_reader<R>(
        source_id: impl Into<String>,
        display_name: impl Into<String>,
        reader: R,
    ) -> Result<Self, CaptureError>
    where
        R: Read + Send + 'static,
    {
        let reader = create_reader(INITIAL_READER_CAPACITY, reader).map_err(|error| {
            CaptureError::InvalidReplay {
                message: safe_parser_error(error),
            }
        })?;

        Ok(Self {
            metadata: CaptureSourceMetadata {
                source_id: source_id.into(),
                display_name: display_name.into(),
                kind: CaptureSourceKind::Replay,
                link_types: Vec::new(),
                file_format: None,
            },
            reader,
            reader_capacity: INITIAL_READER_CAPACITY,
            interfaces: Vec::new(),
            legacy_link_type: None,
            legacy_nanosecond_precision: None,
            next_sequence: 1,
            clock: ReplayClock::default(),
            finished: false,
        })
    }

    fn read_outcome(&mut self) -> ReaderOutcome {
        match self.reader.next() {
            Ok((offset, block)) => ReaderOutcome::Block {
                offset,
                effect: block_effect(block),
            },
            Err(PcapError::Eof) => ReaderOutcome::Eof,
            Err(PcapError::Incomplete(_)) => ReaderOutcome::Incomplete,
            Err(PcapError::BufferTooSmall) => ReaderOutcome::BufferTooSmall,
            Err(error) => ReaderOutcome::Error(safe_parser_error(error)),
        }
    }

    fn refill(&mut self) -> Result<(), CaptureError> {
        match self.reader.refill() {
            Ok(()) => Ok(()),
            Err(PcapError::BufferTooSmall) => self.grow_and_refill(),
            Err(error) => Err(CaptureError::InvalidReplay {
                message: safe_parser_error(error),
            }),
        }
    }

    fn grow_and_refill(&mut self) -> Result<(), CaptureError> {
        if self.reader_capacity >= MAX_READER_CAPACITY {
            return Err(CaptureError::InvalidReplay {
                message: format!(
                    "capture block exceeds the {} byte safety limit",
                    MAX_READER_CAPACITY
                ),
            });
        }

        let new_capacity = self
            .reader_capacity
            .saturating_mul(2)
            .min(MAX_READER_CAPACITY);
        if !self.reader.grow(new_capacity) {
            return Err(CaptureError::InvalidReplay {
                message: format!("could not grow replay buffer to {new_capacity} bytes"),
            });
        }
        self.reader_capacity = new_capacity;

        self.reader
            .refill()
            .map_err(|error| CaptureError::InvalidReplay {
                message: safe_parser_error(error),
            })
    }

    fn apply_effect(&mut self, effect: BlockEffect) -> Result<Option<CapturedFrame>, CaptureError> {
        match effect {
            BlockEffect::PcapHeader {
                link_type,
                nanosecond_precision,
            } => {
                self.set_format(CaptureFileFormat::Pcap)?;
                self.legacy_link_type = Some(link_type);
                self.legacy_nanosecond_precision = Some(nanosecond_precision);
                self.remember_link_type(link_type);
                Ok(None)
            }
            BlockEffect::PcapNgSection => {
                self.set_format(CaptureFileFormat::PcapNg)?;
                self.interfaces.clear();
                Ok(None)
            }
            BlockEffect::PcapNgInterface(interface) => {
                self.set_format(CaptureFileFormat::PcapNg)?;
                self.remember_link_type(interface.link_type);
                self.interfaces.push(interface);
                Ok(None)
            }
            BlockEffect::LegacyPacket {
                seconds,
                fraction,
                original_length,
                bytes,
            } => {
                self.set_format(CaptureFileFormat::Pcap)?;
                let link_type =
                    self.legacy_link_type
                        .ok_or_else(|| CaptureError::InvalidReplay {
                            message: "pcap packet appeared before its global header".into(),
                        })?;
                let nanosecond_precision = self.legacy_nanosecond_precision.ok_or_else(|| {
                    CaptureError::InvalidReplay {
                        message: "pcap timestamp precision is unavailable".into(),
                    }
                })?;
                let timestamp = legacy_timestamp_nanos(seconds, fraction, nanosecond_precision)?;
                self.build_frame(None, link_type, Some(timestamp), original_length, bytes)
                    .map(Some)
            }
            BlockEffect::EnhancedPacket {
                interface_id,
                raw_timestamp,
                original_length,
                bytes,
            } => {
                self.set_format(CaptureFileFormat::PcapNg)?;
                let interface = self
                    .interfaces
                    .get(interface_id as usize)
                    .copied()
                    .ok_or_else(|| CaptureError::InvalidReplay {
                        message: format!(
                            "pcapng packet references missing interface {interface_id}"
                        ),
                    })?;
                let resolution = interface.timestamp_resolution.ok_or_else(|| {
                    CaptureError::InvalidReplay {
                        message: format!(
                            "pcapng interface {interface_id} has an invalid timestamp resolution"
                        ),
                    }
                })?;
                let timestamp = pcapng_timestamp_nanos(
                    raw_timestamp,
                    resolution,
                    interface.timestamp_offset_seconds,
                )?;
                self.build_frame(
                    Some(interface_id),
                    interface.link_type,
                    Some(timestamp),
                    original_length,
                    bytes,
                )
                .map(Some)
            }
            BlockEffect::SimplePacket {
                original_length,
                bytes,
            } => {
                self.set_format(CaptureFileFormat::PcapNg)?;
                let interface = self.interfaces.first().copied().ok_or_else(|| {
                    CaptureError::InvalidReplay {
                        message: "pcapng simple packet has no interface 0".into(),
                    }
                })?;
                self.build_frame(Some(0), interface.link_type, None, original_length, bytes)
                    .map(Some)
            }
            BlockEffect::Ignore => Ok(None),
        }
    }

    fn set_format(&mut self, format: CaptureFileFormat) -> Result<(), CaptureError> {
        if let Some(existing) = self.metadata.file_format {
            if existing != format {
                return Err(CaptureError::InvalidReplay {
                    message: "capture stream mixed pcap and pcapng blocks".into(),
                });
            }
        } else {
            self.metadata.file_format = Some(format);
        }
        Ok(())
    }

    fn remember_link_type(&mut self, link_type: CaptureLinkType) {
        if !self.metadata.link_types.contains(&link_type) {
            self.metadata.link_types.push(link_type);
        }
    }

    fn build_frame(
        &mut self,
        interface_id: Option<u32>,
        link_type: CaptureLinkType,
        source_timestamp_nanos: Option<i64>,
        original_length: u32,
        bytes: Vec<u8>,
    ) -> Result<CapturedFrame, CaptureError> {
        let captured_length =
            u32::try_from(bytes.len()).map_err(|_| CaptureError::InvalidReplay {
                message: "captured frame length does not fit in 32 bits".into(),
            })?;
        if captured_length > original_length {
            return Err(CaptureError::InvalidReplay {
                message: format!(
                    "captured frame length {captured_length} exceeds original length {original_length}"
                ),
            });
        }

        let (observed_micros, timestamp_normalization) =
            self.clock.normalize(source_timestamp_nanos);
        let frame = CapturedFrame {
            sequence: self.next_sequence,
            observed_micros,
            source_timestamp_nanos,
            timestamp_normalization,
            interface_id,
            link_type,
            original_length,
            bytes,
        };
        self.next_sequence =
            self.next_sequence
                .checked_add(1)
                .ok_or_else(|| CaptureError::InvalidReplay {
                    message: "capture contains too many frames".into(),
                })?;

        Ok(frame)
    }
}

impl CaptureSource for OfflineCapture {
    fn metadata(&self) -> &CaptureSourceMetadata {
        &self.metadata
    }

    fn next_frame(&mut self) -> Result<Option<CapturedFrame>, CaptureError> {
        if self.finished {
            return Ok(None);
        }

        loop {
            match self.read_outcome() {
                ReaderOutcome::Block { offset, effect } => {
                    self.reader.consume(offset);
                    if let Some(frame) = self.apply_effect(effect)? {
                        return Ok(Some(frame));
                    }
                }
                ReaderOutcome::Eof => {
                    self.finished = true;
                    return Ok(None);
                }
                ReaderOutcome::Incomplete => {
                    if self.reader.reader_exhausted() {
                        return Err(CaptureError::InvalidReplay {
                            message: "capture ended inside an incomplete block".into(),
                        });
                    }
                    self.refill()?;
                }
                ReaderOutcome::BufferTooSmall => self.grow_and_refill()?,
                ReaderOutcome::Error(message) => {
                    return Err(CaptureError::InvalidReplay { message });
                }
            }
        }
    }
}

fn block_effect(block: PcapBlockOwned<'_>) -> BlockEffect {
    match block {
        PcapBlockOwned::LegacyHeader(header) => BlockEffect::PcapHeader {
            link_type: CaptureLinkType::from_pcap_link_type(header.network.0),
            nanosecond_precision: header.is_nanosecond_precision(),
        },
        PcapBlockOwned::Legacy(packet) => BlockEffect::LegacyPacket {
            seconds: packet.ts_sec,
            fraction: packet.ts_usec,
            original_length: packet.origlen,
            bytes: packet.data.to_vec(),
        },
        PcapBlockOwned::NG(Block::SectionHeader(_)) => BlockEffect::PcapNgSection,
        PcapBlockOwned::NG(Block::InterfaceDescription(interface)) => {
            BlockEffect::PcapNgInterface(InterfaceInfo {
                link_type: CaptureLinkType::from_pcap_link_type(interface.linktype.0),
                timestamp_resolution: interface.ts_resolution(),
                timestamp_offset_seconds: interface.ts_offset(),
            })
        }
        PcapBlockOwned::NG(Block::EnhancedPacket(packet)) => BlockEffect::EnhancedPacket {
            interface_id: packet.if_id,
            raw_timestamp: (u64::from(packet.ts_high) << 32) | u64::from(packet.ts_low),
            original_length: packet.origlen,
            bytes: packet.packet_data().to_vec(),
        },
        PcapBlockOwned::NG(Block::SimplePacket(packet)) => BlockEffect::SimplePacket {
            original_length: packet.origlen,
            bytes: packet.packet_data().to_vec(),
        },
        // Metadata, name resolution, custom blocks, and decryption-secret
        // blocks are deliberately not exposed as captured frames.
        PcapBlockOwned::NG(_) => BlockEffect::Ignore,
    }
}

fn legacy_timestamp_nanos(
    seconds: u32,
    fraction: u32,
    nanosecond_precision: bool,
) -> Result<i64, CaptureError> {
    let fraction_nanos = if nanosecond_precision {
        if fraction >= 1_000_000_000 {
            return Err(CaptureError::InvalidReplay {
                message: format!("pcap nanosecond fraction is out of range: {fraction}"),
            });
        }
        u64::from(fraction)
    } else {
        if fraction >= 1_000_000 {
            return Err(CaptureError::InvalidReplay {
                message: format!("pcap microsecond fraction is out of range: {fraction}"),
            });
        }
        u64::from(fraction) * 1_000
    };
    let timestamp = u64::from(seconds) * 1_000_000_000 + fraction_nanos;

    i64::try_from(timestamp).map_err(|_| CaptureError::InvalidReplay {
        message: "pcap timestamp is outside the supported range".into(),
    })
}

fn pcapng_timestamp_nanos(
    raw_timestamp: u64,
    resolution: u64,
    offset_seconds: i64,
) -> Result<i64, CaptureError> {
    if resolution == 0 {
        return Err(CaptureError::InvalidReplay {
            message: "pcapng timestamp resolution cannot be zero".into(),
        });
    }

    let whole_seconds = i128::from(raw_timestamp / resolution) + i128::from(offset_seconds);
    let fractional_units = i128::from(raw_timestamp % resolution);
    let timestamp =
        whole_seconds * 1_000_000_000 + fractional_units * 1_000_000_000 / i128::from(resolution);

    i64::try_from(timestamp).map_err(|_| CaptureError::InvalidReplay {
        message: "pcapng timestamp is outside the supported range".into(),
    })
}

fn safe_parser_error<I>(error: PcapError<I>) -> String {
    match error {
        PcapError::Eof => "end of file".into(),
        PcapError::BufferTooSmall => "reader buffer is too small".into(),
        PcapError::UnexpectedEof => "unexpected end of file".into(),
        PcapError::ReadError => "capture read failed".into(),
        PcapError::Incomplete(required) => {
            format!("capture block is incomplete; parser requested {required} more bytes")
        }
        PcapError::HeaderNotRecognized => "header is not recognized as pcap or pcapng".into(),
        PcapError::NomError(_, kind) | PcapError::OwnedNomError(_, kind) => {
            format!("capture parser rejected a block: {kind:?}")
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::Cursor,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;
    use crate::ValidatedCapture;

    const PCAP_FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/capture/minimal-ethernet.pcap.hex"
    ));
    const PCAPNG_FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/capture/minimal-ethernet.pcapng.hex"
    ));
    const MULTI_INTERFACE_PCAPNG_FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/capture/multi-interface.pcapng.hex"
    ));
    static NEXT_TEMP_FILE_ID: AtomicU64 = AtomicU64::new(1);

    fn decode_hex_fixture(input: &str) -> Vec<u8> {
        input
            .split_whitespace()
            .map(|byte| u8::from_str_radix(byte, 16).expect("fixture byte"))
            .collect()
    }

    fn replay(input: &str) -> (CaptureSourceMetadata, Vec<CapturedFrame>) {
        let source = OfflineCapture::from_reader(
            "fixture",
            "Fixture",
            Cursor::new(decode_hex_fixture(input)),
        )
        .unwrap();
        let mut source = ValidatedCapture::new(source);
        let mut frames = Vec::new();

        while let Some(frame) = source.next_frame().unwrap() {
            frames.push(frame);
        }

        (source.metadata().clone(), frames)
    }

    fn temporary_capture_path(extension: &str) -> std::path::PathBuf {
        let unique = NEXT_TEMP_FILE_ID.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "rlogs-offline-capture-{}-{unique}.{extension}",
            std::process::id()
        ))
    }

    #[test]
    fn legacy_pcap_streams_frames_and_preserves_truncation() {
        let (metadata, frames) = replay(PCAP_FIXTURE);

        assert_eq!(metadata.file_format, Some(CaptureFileFormat::Pcap));
        assert_eq!(metadata.link_types, vec![CaptureLinkType::Ethernet]);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].sequence, 1);
        assert_eq!(frames[0].observed_micros, 0);
        assert_eq!(frames[0].source_timestamp_nanos, Some(1_000_100_000));
        assert_eq!(frames[0].bytes, [0x10, 0x20, 0x30, 0x40]);
        assert_eq!(frames[1].observed_micros, 500);
        assert_eq!(frames[1].original_length, 5);
        assert_eq!(frames[1].bytes, [0xaa, 0xbb, 0xcc]);
        assert_eq!(frames[1].interface_id, None);
    }

    #[test]
    fn pcapng_streams_the_same_frames_with_interface_identity() {
        let (metadata, frames) = replay(PCAPNG_FIXTURE);

        assert_eq!(metadata.file_format, Some(CaptureFileFormat::PcapNg));
        assert_eq!(metadata.link_types, vec![CaptureLinkType::Ethernet]);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].source_timestamp_nanos, Some(1_000_100_000));
        assert_eq!(frames[0].bytes, [0x10, 0x20, 0x30, 0x40]);
        assert_eq!(frames[0].interface_id, Some(0));
        assert_eq!(frames[1].observed_micros, 500);
        assert_eq!(frames[1].original_length, 5);
        assert_eq!(frames[1].bytes, [0xaa, 0xbb, 0xcc]);
        assert_eq!(frames[1].interface_id, Some(0));
    }

    #[test]
    fn pcapng_resolves_each_packet_through_its_declared_interface() {
        let (metadata, frames) = replay(MULTI_INTERFACE_PCAPNG_FIXTURE);

        assert_eq!(
            metadata.link_types,
            vec![CaptureLinkType::Ethernet, CaptureLinkType::RawIp]
        );
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].interface_id, Some(1));
        assert_eq!(frames[0].link_type, CaptureLinkType::RawIp);
        assert_eq!(frames[0].bytes, [0x45, 0x00, 0x00, 0x14]);
    }

    #[test]
    fn file_open_uses_the_same_streaming_replay_path() {
        let path = temporary_capture_path("pcapng");
        fs::write(&path, decode_hex_fixture(PCAPNG_FIXTURE)).unwrap();

        let result = (|| {
            let source = OfflineCapture::open(&path)?;
            let mut source = ValidatedCapture::new(source);
            let first = source.next_frame()?.expect("first frame");
            Ok::<_, CaptureError>((first, source.metadata().clone()))
        })();
        fs::remove_file(&path).unwrap();

        let (first, metadata) = result.unwrap();
        assert_eq!(first.bytes, [0x10, 0x20, 0x30, 0x40]);
        assert_eq!(metadata.file_format, Some(CaptureFileFormat::PcapNg));
    }

    #[test]
    fn backward_source_timestamps_are_preserved_but_replay_time_is_monotonic() {
        let mut clock = ReplayClock::default();

        assert_eq!(
            clock.normalize(Some(1_000_000)),
            (0, TimestampNormalization::Exact)
        );
        assert_eq!(
            clock.normalize(Some(2_000_000)),
            (1_000, TimestampNormalization::Exact)
        );
        assert_eq!(
            clock.normalize(Some(1_500_000)),
            (1_000, TimestampNormalization::ClampedBackward)
        );
    }

    #[test]
    fn missing_timestamps_do_not_invent_elapsed_time() {
        let mut clock = ReplayClock::default();

        assert_eq!(
            clock.normalize(None),
            (0, TimestampNormalization::Unavailable)
        );
        assert_eq!(
            clock.normalize(Some(10_000)),
            (0, TimestampNormalization::Exact)
        );
        assert_eq!(
            clock.normalize(None),
            (0, TimestampNormalization::Unavailable)
        );
    }

    #[test]
    fn timestamp_precision_is_converted_without_floating_point() {
        assert_eq!(
            legacy_timestamp_nanos(1, 100, false).unwrap(),
            1_000_100_000
        );
        assert_eq!(legacy_timestamp_nanos(1, 100, true).unwrap(), 1_000_000_100);
        assert_eq!(
            pcapng_timestamp_nanos(10_001, 10_000, 2).unwrap(),
            3_000_100_000
        );
    }

    #[test]
    fn unrecognized_input_is_rejected_before_replay() {
        let error =
            OfflineCapture::from_reader("fixture", "Fixture", Cursor::new(vec![1, 2, 3, 4]))
                .unwrap_err();

        assert!(matches!(error, CaptureError::InvalidReplay { .. }));
    }
}
