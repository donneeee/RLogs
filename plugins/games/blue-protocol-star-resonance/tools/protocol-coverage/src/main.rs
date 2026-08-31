use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use rlogs_game_bpsr::{
    CaptureGapKind, CompressionState, FragmentKind, JsonlJournalError, JsonlJournalReader,
    JsonlJournalSummary, PacketDirection, ProtocolFeature, ProtocolPack,
    ProtocolPackCoverageSummary,
};
use serde::Serialize;

fn main() {
    if let Err(error) = run() {
        eprintln!("protocol coverage failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments()?;
    let path = arguments.capture_path;
    let file = File::open(&path)?;
    let mut stream = JsonlJournalReader::new(BufReader::new(file)).into_record_stream()?;
    let mut truncated_tail = None;
    loop {
        match stream.next_record() {
            Ok(Some(_)) => {}
            Ok(None) => break,
            Err(JsonlJournalError::InvalidJson { line, source })
                if arguments.recover_truncated_tail
                    && source.is_eof()
                    && stream
                        .truncated_tail()
                        .is_some_and(|(tail_line, _, _)| tail_line == line) =>
            {
                truncated_tail = stream.truncated_tail();
                break;
            }
            Err(error) => return Err(error.into()),
        }
    }
    let summary = stream.summary();
    let pack = arguments
        .pack_path
        .as_ref()
        .map(|path| std::fs::read(path).map_err(Box::<dyn std::error::Error>::from))
        .transpose()?
        .map(|json| ProtocolPack::from_json(&json))
        .transpose()?;

    if arguments.json {
        print_json(&path, &summary, pack.as_ref(), truncated_tail)?;
    } else {
        print_text(&path, &summary, pack.as_ref(), truncated_tail);
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
struct Arguments {
    json: bool,
    recover_truncated_tail: bool,
    pack_path: Option<PathBuf>,
    capture_path: PathBuf,
}

fn arguments() -> Result<Arguments, String> {
    parse_arguments(std::env::args_os().skip(1))
}

fn parse_arguments(arguments: impl IntoIterator<Item = OsString>) -> Result<Arguments, String> {
    let mut json = false;
    let mut recover_truncated_tail = false;
    let mut pack_path = None;
    let mut capture_path = None;
    let mut arguments = arguments.into_iter();

    while let Some(argument) = arguments.next() {
        if argument == OsStr::new("--json") {
            json = true;
        } else if argument == OsStr::new("--recover-truncated-tail") {
            recover_truncated_tail = true;
        } else if argument == OsStr::new("--pack") {
            if pack_path.is_some() {
                return Err(usage());
            }
            pack_path = Some(PathBuf::from(arguments.next().ok_or_else(usage)?));
        } else if capture_path.is_none() {
            capture_path = Some(PathBuf::from(argument));
        } else {
            return Err(usage());
        }
    }

    Ok(Arguments {
        json,
        recover_truncated_tail,
        pack_path,
        capture_path: capture_path.ok_or_else(usage)?,
    })
}

fn usage() -> String {
    "usage: rlogs-protocol-coverage [--json] [--recover-truncated-tail] [--pack <pack.json>] <capture.jsonl>".into()
}

fn print_text(
    path: &Path,
    summary: &JsonlJournalSummary,
    pack: Option<&ProtocolPack>,
    truncated_tail: Option<(usize, usize, u64)>,
) {
    let coverage = &summary.coverage;
    println!("Capture: {}", path.display());
    println!("Capture ID: {}", summary.session.capture_id);
    println!(
        "Build: {}/{}/{}",
        summary.session.game_build.deployment_id,
        summary.session.game_build.channel,
        summary.session.game_build.build_id
    );
    println!("Records: {}", summary.record_count);
    if let Some((line, bytes, after_observed_micros)) = truncated_tail {
        println!(
            "Recovered truncated final line: line {line}, {bytes} bytes, after {after_observed_micros} observed micros"
        );
    }
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

    if let Some(pack) = pack {
        let summary = coverage.summarize_pack(pack);
        println!();
        println!("Protocol pack: {}", pack.definition().pack_id);
        println!("Protocol pack digest: {}", pack.digest());
        println!("Known packets: {}", summary.routes.known_packets);
        println!("Unknown packets: {}", summary.routes.unknown_packets);
        println!("Allowed decoder packets: {}", summary.allowed_packets);
        println!("Opaque research packets: {}", summary.opaque_packets);
        println!("Prohibited packets: {}", summary.prohibited_packets);
        println!();
        println!(
            "{:<24} {:>8} {:>10} {:>10} {:>10} {:>10}",
            "feature", "routes", "packets", "allowed", "opaque", "prohibited"
        );
        for (feature, counts) in &summary.features {
            println!(
                "{:<24} {:>8} {:>10} {:>10} {:>10} {:>10}",
                feature_name(*feature),
                counts.route_count,
                counts.packet_count,
                counts.allowed_packets,
                counts.opaque_packets,
                counts.prohibited_packets,
            );
        }
    }
}

fn print_json(
    path: &Path,
    summary: &JsonlJournalSummary,
    pack: Option<&ProtocolPack>,
    truncated_tail: Option<(usize, usize, u64)>,
) -> Result<(), serde_json::Error> {
    let coverage = &summary.coverage;
    let report = JsonReport {
        capture_path: path.display().to_string(),
        capture_id: summary.session.capture_id.clone(),
        deployment_id: summary.session.game_build.deployment_id.clone(),
        region_id: summary.session.game_build.region_id.clone(),
        channel: summary.session.game_build.channel.clone(),
        build_id: summary.session.game_build.build_id.clone(),
        record_count: summary.record_count,
        truncated_tail: truncated_tail.map(|(line, bytes, after_observed_micros)| {
            JsonTruncatedTail {
                line,
                bytes,
                after_observed_micros,
            }
        }),
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
        protocol_pack: pack.map(|pack| json_pack_report(pack, &coverage.summarize_pack(pack))),
    };

    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn json_pack_report(pack: &ProtocolPack, summary: &ProtocolPackCoverageSummary) -> JsonPackReport {
    JsonPackReport {
        pack_id: pack.definition().pack_id.clone(),
        digest: pack.digest().to_owned(),
        known_routes: summary.routes.known_routes,
        unknown_routes: summary.routes.unknown_routes,
        known_packets: summary.routes.known_packets,
        unknown_packets: summary.routes.unknown_packets,
        allowed_packets: summary.allowed_packets,
        opaque_packets: summary.opaque_packets,
        prohibited_packets: summary.prohibited_packets,
        features: summary
            .features
            .iter()
            .map(|(feature, counts)| JsonFeatureCoverage {
                feature: *feature,
                route_count: counts.route_count,
                packet_count: counts.packet_count,
                allowed_packets: counts.allowed_packets,
                opaque_packets: counts.opaque_packets,
                prohibited_packets: counts.prohibited_packets,
            })
            .collect(),
    }
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

fn feature_name(feature: ProtocolFeature) -> String {
    serde_json::to_value(feature)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("{feature:?}"))
}

#[derive(Serialize)]
struct JsonReport {
    capture_path: String,
    capture_id: String,
    deployment_id: String,
    region_id: Option<String>,
    channel: String,
    build_id: String,
    record_count: u64,
    truncated_tail: Option<JsonTruncatedTail>,
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
    protocol_pack: Option<JsonPackReport>,
}

#[derive(Serialize)]
struct JsonTruncatedTail {
    line: usize,
    bytes: usize,
    after_observed_micros: u64,
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

#[derive(Serialize)]
struct JsonPackReport {
    pack_id: String,
    digest: String,
    known_routes: u64,
    unknown_routes: u64,
    known_packets: u64,
    unknown_packets: u64,
    allowed_packets: u64,
    opaque_packets: u64,
    prohibited_packets: u64,
    features: Vec<JsonFeatureCoverage>,
}

#[derive(Serialize)]
struct JsonFeatureCoverage {
    feature: ProtocolFeature,
    route_count: u64,
    packet_count: u64,
    allowed_packets: u64,
    opaque_packets: u64,
    prohibited_packets: u64,
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
            Arguments {
                json: false,
                recover_truncated_tail: false,
                pack_path: None,
                capture_path: PathBuf::from("capture.jsonl"),
            }
        );
        assert_eq!(
            parse_arguments(args(&["--json", "capture.jsonl"])).unwrap(),
            Arguments {
                json: true,
                recover_truncated_tail: false,
                pack_path: None,
                capture_path: PathBuf::from("capture.jsonl"),
            }
        );
        assert_eq!(
            parse_arguments(args(&["capture.jsonl", "--json", "--pack", "pack.json"])).unwrap(),
            Arguments {
                json: true,
                recover_truncated_tail: false,
                pack_path: Some(PathBuf::from("pack.json")),
                capture_path: PathBuf::from("capture.jsonl"),
            }
        );
        assert!(
            parse_arguments(args(&["--recover-truncated-tail", "capture.jsonl",]))
                .unwrap()
                .recover_truncated_tail
        );
    }

    #[test]
    fn arguments_reject_missing_or_extra_paths() {
        assert!(parse_arguments(args(&[])).is_err());
        assert!(parse_arguments(args(&["one.jsonl", "two.jsonl"])).is_err());
    }
}
