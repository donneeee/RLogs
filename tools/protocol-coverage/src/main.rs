use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use rlogs_protocol::{
    CaptureGapKind, CompressionState, FragmentKind, JsonlJournalReader, JsonlJournalSummary,
    PacketDirection,
};
use serde::Serialize;

fn main() {
    if let Err(error) = run() {
        eprintln!("protocol coverage failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let (json, path) = arguments()?;
    let file = File::open(&path)?;
    let summary = JsonlJournalReader::new(BufReader::new(file)).summarize()?;

    if json {
        print_json(&path, &summary)?;
    } else {
        print_text(&path, &summary);
    }
    Ok(())
}

fn arguments() -> Result<(bool, PathBuf), String> {
    parse_arguments(std::env::args_os().skip(1))
}

fn parse_arguments(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<(bool, PathBuf), String> {
    let mut json = false;
    let mut path = None;

    for argument in arguments {
        if argument == OsStr::new("--json") {
            json = true;
        } else if path.is_none() {
            path = Some(PathBuf::from(argument));
        } else {
            return Err(usage());
        }
    }

    path.map(|path| (json, path)).ok_or_else(usage)
}

fn usage() -> String {
    "usage: rlogs-protocol-coverage [--json] <capture.jsonl>".into()
}

fn print_text(path: &Path, summary: &JsonlJournalSummary) {
    let coverage = &summary.coverage;
    println!("Capture: {}", path.display());
    println!("Capture ID: {}", summary.session.capture_id);
    println!(
        "Build: {}/{}/{}",
        summary.session.game_build.region,
        summary.session.game_build.channel,
        summary.session.game_build.build_id
    );
    println!("Records: {}", summary.record_count);
    println!("Packets: {}", coverage.packet_count);
    println!("Gaps: {}", coverage.gap_count);
    println!("Unrouted packets: {}", coverage.unrouted_packet_count);
    println!(
        "Unclassified fragment packets: {}",
        coverage.unclassified_fragment_packet_count
    );
    println!("Wire bytes: {}", coverage.wire_bytes);
    println!("Application bytes: {}", coverage.application_bytes);
    println!("Observed fragments: {}", coverage.fragments().len());
    println!("Observed routes: {}", coverage.routes().len());
    println!();
    println!("Compression:");
    for (compression, count) in coverage.compression() {
        println!("  {:<18} {:>10}", compression_name(*compression), count);
    }
    println!("Gaps by kind:");
    if coverage.gaps().is_empty() {
        println!("  none");
    } else {
        for (kind, count) in coverage.gaps() {
            println!("  {:<24} {:>10}", gap_name(*kind), count);
        }
    }
    println!();
    println!(
        "{:<17} {:<16} {:>10} {:>12} {:>12}",
        "direction", "fragment", "packets", "wire bytes", "app bytes"
    );
    for ((direction, fragment), counts) in coverage.fragments() {
        println!(
            "{:<17} {:<16} {:>10} {:>12} {:>12}",
            direction_name(*direction),
            fragment_name(*fragment),
            counts.packet_count,
            counts.wire_bytes,
            counts.application_bytes,
        );
    }
    println!();
    println!(
        "{:<17} {:<12} {:<20} {:<18} {:>10} {:>12} {:>12}",
        "direction", "fragment", "service", "method", "packets", "wire bytes", "app bytes"
    );

    for (route, counts) in coverage.routes() {
        println!(
            "{:<17} {:<12} {:<20} {:<18} {:>10} {:>12} {:>12}",
            direction_name(route.direction),
            fragment_name(route.fragment),
            format!("{} (0x{:x})", route.service_id, route.service_id),
            format!("{} (0x{:x})", route.method_id, route.method_id),
            counts.packet_count,
            counts.wire_bytes,
            counts.application_bytes,
        );
    }
}

fn print_json(path: &Path, summary: &JsonlJournalSummary) -> Result<(), serde_json::Error> {
    let coverage = &summary.coverage;
    let report = JsonReport {
        capture_path: path.display().to_string(),
        capture_id: summary.session.capture_id.clone(),
        region: summary.session.game_build.region.clone(),
        channel: summary.session.game_build.channel.clone(),
        build_id: summary.session.game_build.build_id.clone(),
        record_count: summary.record_count,
        packet_count: coverage.packet_count,
        gap_count: coverage.gap_count,
        unrouted_packet_count: coverage.unrouted_packet_count,
        unclassified_fragment_packet_count: coverage.unclassified_fragment_packet_count,
        wire_bytes: coverage.wire_bytes,
        application_bytes: coverage.application_bytes,
        compression: coverage
            .compression()
            .iter()
            .map(|(state, count)| JsonCompression {
                state: *state,
                packet_count: *count,
            })
            .collect(),
        gaps: coverage
            .gaps()
            .iter()
            .map(|(kind, count)| JsonGap {
                kind: *kind,
                count: *count,
            })
            .collect(),
        fragments: coverage
            .fragments()
            .iter()
            .map(|((direction, fragment), counts)| JsonFragment {
                direction: *direction,
                fragment: *fragment,
                packet_count: counts.packet_count,
                wire_bytes: counts.wire_bytes,
                application_bytes: counts.application_bytes,
                first_sequence: counts.first_sequence,
                last_sequence: counts.last_sequence,
            })
            .collect(),
        routes: coverage
            .routes()
            .iter()
            .map(|(route, counts)| JsonRoute {
                direction: route.direction,
                fragment: route.fragment,
                service_id: route.service_id,
                method_id: route.method_id,
                packet_count: counts.packet_count,
                wire_bytes: counts.wire_bytes,
                application_bytes: counts.application_bytes,
                first_sequence: counts.first_sequence,
                last_sequence: counts.last_sequence,
            })
            .collect(),
    };

    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn direction_name(direction: PacketDirection) -> &'static str {
    match direction {
        PacketDirection::ClientToServer => "client_to_server",
        PacketDirection::ServerToClient => "server_to_client",
        PacketDirection::Unknown => "unknown",
    }
}

fn fragment_name(fragment: FragmentKind) -> String {
    match fragment {
        FragmentKind::Call => "call".into(),
        FragmentKind::Notify => "notify".into(),
        FragmentKind::Return => "return".into(),
        FragmentKind::Echo => "echo".into(),
        FragmentKind::FrameUp => "frame_up".into(),
        FragmentKind::FrameDown => "frame_down".into(),
        FragmentKind::Unknown(wire_id) => format!("unknown_{wire_id}"),
    }
}

fn compression_name(compression: CompressionState) -> &'static str {
    match compression {
        CompressionState::NotCompressed => "not_compressed",
        CompressionState::ZstdDecoded => "zstd_decoded",
        CompressionState::ZstdFailed => "zstd_failed",
        CompressionState::Unknown => "unknown",
    }
}

fn gap_name(kind: CaptureGapKind) -> &'static str {
    match kind {
        CaptureGapKind::AdapterDrop => "adapter_drop",
        CaptureGapKind::QueueDrop => "queue_drop",
        CaptureGapKind::TcpGap => "tcp_gap",
        CaptureGapKind::MalformedFrame => "malformed_frame",
        CaptureGapKind::DecompressionFailure => "decompression_failure",
        CaptureGapKind::UnsupportedFragment => "unsupported_fragment",
        CaptureGapKind::UnsupportedTransport => "unsupported_transport",
    }
}

#[derive(Serialize)]
struct JsonReport {
    capture_path: String,
    capture_id: String,
    region: String,
    channel: String,
    build_id: String,
    record_count: u64,
    packet_count: u64,
    gap_count: u64,
    unrouted_packet_count: u64,
    unclassified_fragment_packet_count: u64,
    wire_bytes: u64,
    application_bytes: u64,
    compression: Vec<JsonCompression>,
    gaps: Vec<JsonGap>,
    fragments: Vec<JsonFragment>,
    routes: Vec<JsonRoute>,
}

#[derive(Serialize)]
struct JsonCompression {
    state: CompressionState,
    packet_count: u64,
}

#[derive(Serialize)]
struct JsonGap {
    kind: CaptureGapKind,
    count: u64,
}

#[derive(Serialize)]
struct JsonFragment {
    direction: PacketDirection,
    fragment: FragmentKind,
    packet_count: u64,
    wire_bytes: u64,
    application_bytes: u64,
    first_sequence: u64,
    last_sequence: u64,
}

#[derive(Serialize)]
struct JsonRoute {
    direction: PacketDirection,
    fragment: FragmentKind,
    service_id: u64,
    method_id: u32,
    packet_count: u64,
    wire_bytes: u64,
    application_bytes: u64,
    first_sequence: u64,
    last_sequence: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> impl Iterator<Item = OsString> {
        values
            .iter()
            .map(OsString::from)
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn arguments_accept_text_and_json_modes() {
        assert_eq!(
            parse_arguments(args(&["capture.jsonl"])).unwrap(),
            (false, PathBuf::from("capture.jsonl"))
        );
        assert_eq!(
            parse_arguments(args(&["--json", "capture.jsonl"])).unwrap(),
            (true, PathBuf::from("capture.jsonl"))
        );
        assert_eq!(
            parse_arguments(args(&["capture.jsonl", "--json"])).unwrap(),
            (true, PathBuf::from("capture.jsonl"))
        );
    }

    #[test]
    fn arguments_reject_missing_or_extra_paths() {
        assert!(parse_arguments(args(&[])).is_err());
        assert!(parse_arguments(args(&["one.jsonl", "two.jsonl"])).is_err());
    }
}
