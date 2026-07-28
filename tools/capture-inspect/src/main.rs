use std::{ffi::OsString, path::PathBuf, process::ExitCode};

use rlogs_capture::{
    CaptureError, CaptureFileFormat, CaptureLinkType, CaptureSourceMetadata, CapturedFrame,
    OfflineCapture, TimestampNormalization, ValidatedCapture,
};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputMode {
    Text,
    Json,
}

#[derive(Debug, PartialEq, Eq)]
struct Arguments {
    mode: OutputMode,
    path: PathBuf,
}

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
struct CaptureSummary {
    format: Option<CaptureFileFormat>,
    link_types: Vec<CaptureLinkType>,
    frame_count: u64,
    captured_bytes: u64,
    original_bytes: u64,
    truncated_frames: u64,
    backward_timestamp_frames: u64,
    unavailable_timestamp_frames: u64,
    duration_micros: u64,
}

impl CaptureSummary {
    fn observe(&mut self, frame: &CapturedFrame) {
        self.frame_count = self.frame_count.saturating_add(1);
        self.captured_bytes = self.captured_bytes.saturating_add(frame.bytes.len() as u64);
        self.original_bytes = self
            .original_bytes
            .saturating_add(u64::from(frame.original_length));
        if frame.bytes.len() < frame.original_length as usize {
            self.truncated_frames = self.truncated_frames.saturating_add(1);
        }
        match frame.timestamp_normalization {
            TimestampNormalization::Exact => {}
            TimestampNormalization::ClampedBackward => {
                self.backward_timestamp_frames = self.backward_timestamp_frames.saturating_add(1);
            }
            TimestampNormalization::Unavailable => {
                self.unavailable_timestamp_frames =
                    self.unavailable_timestamp_frames.saturating_add(1);
            }
        }
        self.duration_micros = frame.observed_micros;
    }

    fn finish(&mut self, metadata: &CaptureSourceMetadata) {
        self.format = metadata.file_format;
        self.link_types.clone_from(&metadata.link_types);
    }
}

fn main() -> ExitCode {
    match run(std::env::args_os().skip(1)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("rlogs-capture-inspect: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: impl IntoIterator<Item = OsString>) -> Result<(), String> {
    let arguments = parse_arguments(arguments)?;
    let summary = inspect_capture(&arguments.path)?;

    match arguments.mode {
        OutputMode::Text => print_text(&summary),
        OutputMode::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&summary)
                    .map_err(|error| format!("could not serialize summary: {error}"))?
            );
        }
    }

    Ok(())
}

fn inspect_capture(path: &std::path::Path) -> Result<CaptureSummary, String> {
    let source = OfflineCapture::open(path).map_err(format_capture_error)?;
    let mut source = ValidatedCapture::new(source);
    let mut summary = CaptureSummary::default();

    while let Some(frame) = source.next_frame().map_err(format_capture_error)? {
        summary.observe(&frame);
    }
    summary.finish(source.metadata());

    Ok(summary)
}

fn parse_arguments(arguments: impl IntoIterator<Item = OsString>) -> Result<Arguments, String> {
    let mut arguments = arguments.into_iter();
    let first = arguments.next().ok_or_else(usage)?;

    let (mode, path) = if first == "--json" {
        (
            OutputMode::Json,
            PathBuf::from(arguments.next().ok_or_else(usage)?),
        )
    } else {
        (OutputMode::Text, PathBuf::from(first))
    };

    if arguments.next().is_some() {
        return Err(usage());
    }

    Ok(Arguments { mode, path })
}

fn usage() -> String {
    "usage: rlogs-capture-inspect [--json] <capture.pcap|capture.pcapng>".into()
}

fn format_capture_error(error: CaptureError) -> String {
    error.to_string()
}

fn print_text(summary: &CaptureSummary) {
    println!(
        "format: {}",
        summary.format.map(format_label).unwrap_or("undetermined")
    );
    println!(
        "link types: {}",
        if summary.link_types.is_empty() {
            "none".into()
        } else {
            summary
                .link_types
                .iter()
                .map(|link_type| link_type_label(*link_type))
                .collect::<Vec<_>>()
                .join(", ")
        }
    );
    println!("frames: {}", summary.frame_count);
    println!("captured bytes: {}", summary.captured_bytes);
    println!("original bytes: {}", summary.original_bytes);
    println!("truncated frames: {}", summary.truncated_frames);
    println!("backward timestamps: {}", summary.backward_timestamp_frames);
    println!(
        "unavailable timestamps: {}",
        summary.unavailable_timestamp_frames
    );
    println!("duration: {} us", summary.duration_micros);
}

fn format_label(format: CaptureFileFormat) -> &'static str {
    match format {
        CaptureFileFormat::Pcap => "pcap",
        CaptureFileFormat::PcapNg => "pcapng",
        CaptureFileFormat::RlogsEvidence => "rlogs-evidence",
    }
}

fn link_type_label(link_type: CaptureLinkType) -> String {
    match link_type {
        CaptureLinkType::NullLoopback => "null-loopback".into(),
        CaptureLinkType::Ethernet => "ethernet".into(),
        CaptureLinkType::RawIp => "raw-ip".into(),
        CaptureLinkType::RawIpv4 => "raw-ipv4".into(),
        CaptureLinkType::RawIpv6 => "raw-ipv6".into(),
        CaptureLinkType::LinuxCookedV1 => "linux-cooked-v1".into(),
        CaptureLinkType::LinuxCookedV2 => "linux-cooked-v2".into(),
        CaptureLinkType::Unknown(value) => format!("unknown-{value}"),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;

    const PCAPNG_FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/capture/minimal-ethernet.pcapng.hex"
    ));
    static NEXT_TEMP_FILE_ID: AtomicU64 = AtomicU64::new(1);

    fn decode_hex_fixture(input: &str) -> Vec<u8> {
        input
            .split_whitespace()
            .map(|byte| u8::from_str_radix(byte, 16).expect("fixture byte"))
            .collect()
    }

    fn frame(
        sequence: u64,
        observed_micros: u64,
        captured_length: usize,
        original_length: u32,
        normalization: TimestampNormalization,
    ) -> CapturedFrame {
        CapturedFrame {
            sequence,
            observed_micros,
            source_timestamp_nanos: Some(observed_micros as i64 * 1_000),
            timestamp_normalization: normalization,
            interface_id: None,
            link_type: CaptureLinkType::Ethernet,
            original_length,
            bytes: vec![0; captured_length].into(),
        }
    }

    #[test]
    fn arguments_accept_text_and_json_modes() {
        assert_eq!(
            parse_arguments([OsString::from("capture.pcap")]).unwrap(),
            Arguments {
                mode: OutputMode::Text,
                path: PathBuf::from("capture.pcap"),
            }
        );
        assert_eq!(
            parse_arguments([OsString::from("--json"), OsString::from("capture.pcapng")]).unwrap(),
            Arguments {
                mode: OutputMode::Json,
                path: PathBuf::from("capture.pcapng"),
            }
        );
    }

    #[test]
    fn arguments_reject_missing_and_extra_paths() {
        assert!(parse_arguments(Vec::<OsString>::new()).is_err());
        assert!(parse_arguments([OsString::from("--json")]).is_err());
        assert!(parse_arguments([OsString::from("one.pcap"), OsString::from("two.pcap")]).is_err());
    }

    #[test]
    fn summary_tracks_truncation_and_timestamp_quality_without_payloads() {
        let mut summary = CaptureSummary::default();
        summary.observe(&frame(1, 0, 4, 4, TimestampNormalization::Exact));
        summary.observe(&frame(
            2,
            500,
            3,
            5,
            TimestampNormalization::ClampedBackward,
        ));
        summary.observe(&frame(3, 500, 2, 2, TimestampNormalization::Unavailable));
        summary.finish(&CaptureSourceMetadata {
            source_id: "fixture".into(),
            display_name: "Fixture".into(),
            kind: rlogs_capture::CaptureSourceKind::Replay,
            link_types: vec![CaptureLinkType::Ethernet],
            file_format: Some(CaptureFileFormat::PcapNg),
        });

        assert_eq!(summary.frame_count, 3);
        assert_eq!(summary.captured_bytes, 9);
        assert_eq!(summary.original_bytes, 11);
        assert_eq!(summary.truncated_frames, 1);
        assert_eq!(summary.backward_timestamp_frames, 1);
        assert_eq!(summary.unavailable_timestamp_frames, 1);
        assert_eq!(summary.duration_micros, 500);
        assert_eq!(summary.format, Some(CaptureFileFormat::PcapNg));
    }

    #[test]
    fn inspect_capture_reads_a_real_pcapng_file_end_to_end() {
        let unique = NEXT_TEMP_FILE_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "rlogs-capture-inspect-{}-{unique}.pcapng",
            std::process::id()
        ));
        fs::write(&path, decode_hex_fixture(PCAPNG_FIXTURE)).unwrap();

        let result = inspect_capture(&path);
        fs::remove_file(&path).unwrap();

        let summary = result.unwrap();
        assert_eq!(summary.format, Some(CaptureFileFormat::PcapNg));
        assert_eq!(summary.frame_count, 2);
        assert_eq!(summary.captured_bytes, 7);
        assert_eq!(summary.original_bytes, 9);
        assert_eq!(summary.truncated_frames, 1);
        assert_eq!(summary.duration_micros, 500);
    }
}
