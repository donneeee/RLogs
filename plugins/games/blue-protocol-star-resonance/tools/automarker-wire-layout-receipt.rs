//! Offline-only sanitizer for retained automarker captures.
//!
//! This tool reads a private PCAP/PCAPNG plus its private protocol journal and
//! emits only bounded frame-layout and TCP-reassembly aggregates. It has no
//! network sender, packet mutation, process access, or raw-payload output.

use std::{
    collections::HashMap,
    env,
    ffi::{OsStr, OsString},
    fs::{File, OpenOptions},
    io::{BufReader, BufWriter, Write},
    path::PathBuf,
};

use rlogs_capture::{OfflineCapture, ValidatedCapture};
use rlogs_core::{GameConnectionFilter, ResearchConnectionFile, TransportDirection};
use rlogs_game_bpsr::{
    BpsrFrame, BpsrFrameUpLayout, BpsrFramerSet, BpsrFramerSetConfig, BpsrFramingConfig,
    BpsrFramingEvent, CaptureRecordKind, CompressionState, FragmentKind, JsonlJournalReader,
    PacketDirection, ProtocolPack, decode_observed_automarker_request_into,
    supports_observed_automarker_requests,
};
use rlogs_network::{
    NetworkDecodeEvent, NetworkDecoder, ReassemblyMetrics, TcpFlowKey, TcpReassembler,
    TcpStreamEvent,
};
use serde::Serialize;

const WORLD_SERVICE: u64 = 103_198_054;
const USE_SLOT_METHOD: u32 = 249_858;
const MAX_MARKER_REQUESTS: usize = 256;
const MAX_CHUNK_SPANS: usize = 1_000_000;

fn main() {
    if let Err(error) = run() {
        eprintln!("automarker wire-layout receipt failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = Arguments::parse(env::args_os().skip(1))?;
    if args.output.exists() {
        return Err(format!("refusing to overwrite {}", args.output.display()).into());
    }

    let pack = ProtocolPack::from_json(&std::fs::read(&args.pack)?)?;
    if !supports_observed_automarker_requests(&pack) {
        return Err("pack is not the exact reviewed automarker request identity".into());
    }
    let filter =
        serde_json::from_slice::<ResearchConnectionFile>(&std::fs::read(&args.connections)?)?
            .validate()?;
    let journal_marker_count = audit_journal(&args.journal, &pack)?;

    let mut capture = ValidatedCapture::new(OfflineCapture::open(&args.capture)?);
    let mut decode_scratch = Vec::new();
    let mut analyzer = WireAnalyzer::new(
        filter,
        pack.definition().acquisition.frame_up_layout,
        |payload| {
            decode_observed_automarker_request_into(&pack, payload, &mut decode_scratch)
                .ok()
                .map(|request| request.marker_number)
        },
    )?;
    while let Some(frame) = capture.next_frame()? {
        analyzer.process_frame(&frame)?;
    }
    let analysis = analyzer.finish();
    if analysis.markers.len() != journal_marker_count {
        return Err(format!(
            "PCAP decoded {} marker requests but journal decoded {journal_marker_count}",
            analysis.markers.len()
        )
        .into());
    }
    if analysis.markers.is_empty() {
        return Err("capture and journal contain no decoded ground-marker requests".into());
    }

    let receipt = Receipt {
        schema_version: 1,
        audit_kind: "sanitized-offline-automarker-wire-layout-receipt",
        input_scope: InputScope {
            pcap_or_pcapng_read_offline: true,
            protocol_journal_read_offline: true,
            packet_transmission_performed: false,
            packet_replay_performed: false,
            packet_modification_performed: false,
        },
        journal_marker_request_count: journal_marker_count,
        marker_requests: analysis.markers,
        filtered_tcp: FilteredTcpAggregate::from(
            analysis.physical_payload_segments,
            &analysis.metrics,
        ),
        interpretation: Interpretation {
            application_equal_length_is_not_wire_feasibility: true,
            compression_or_retransmission_requires_inline_transport_proof: true,
            server_acceptance_proven: false,
            anti_cheat_safety_proven: false,
            runtime_sender_enabled: false,
        },
        privacy: Privacy {
            contains_raw_payloads: false,
            contains_network_endpoints: false,
            contains_ports: false,
            contains_tcp_sequence_numbers: false,
            contains_rpc_call_ids: false,
            contains_session_or_action_identifiers: false,
            contains_local_absolute_paths: false,
            contains_personal_identity: false,
        },
    };

    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args.output)?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, &receipt)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    writer.get_ref().sync_all()?;
    println!("wrote sanitized offline layout receipt");
    Ok(())
}

fn audit_journal(path: &PathBuf, pack: &ProtocolPack) -> Result<usize, Box<dyn std::error::Error>> {
    let mut stream =
        JsonlJournalReader::new(BufReader::new(File::open(path)?)).into_record_stream()?;
    if stream.session().game_build.build_id != pack.definition().target.build_id {
        return Err("journal build does not match the exact reviewed pack".into());
    }
    if stream.session().protocol_pack_digest.as_deref() != Some(pack.digest()) {
        let valid_carry_forward = stream
            .session()
            .protocol_pack_authority
            .as_ref()
            .is_some_and(|authority| {
                authority.kind == "unverified-carry-forward-decoder-hypothesis"
                    && !authority.exact_for_captured_build
                    && !authority.runtime_authority
                    && authority.captured_build == pack.definition().target.build_id
                    && authority.source_build != authority.captured_build
            });
        if !valid_carry_forward {
            return Err(
                "journal has neither the exact pack digest nor a valid carry-forward authority"
                    .into(),
            );
        }
    }
    let mut count = 0usize;
    let mut scratch = Vec::new();
    while let Some(record) = stream.next_record()? {
        let CaptureRecordKind::Packet(packet) = record.kind else {
            continue;
        };
        let Some(route) = packet.route else {
            continue;
        };
        if is_use_slot_call(
            route.key.direction,
            route.key.fragment,
            route.key.service_id,
            route.key.method_id,
        ) && packet.payload.decode_input().is_some_and(|payload| {
            decode_observed_automarker_request_into(pack, payload, &mut scratch).is_ok()
        }) {
            count = count
                .checked_add(1)
                .ok_or("journal marker count overflow")?;
            if count > MAX_MARKER_REQUESTS {
                return Err("journal exceeds the bounded marker-request limit".into());
            }
        }
    }
    Ok(count)
}

fn is_use_slot_call(
    direction: PacketDirection,
    fragment: FragmentKind,
    service_id: u64,
    method_id: u32,
) -> bool {
    direction == PacketDirection::ClientToServer
        && fragment == FragmentKind::Call
        && service_id == WORLD_SERVICE
        && method_id == USE_SLOT_METHOD
}

#[derive(Debug, Clone, Copy)]
struct ChunkSpan {
    start: u64,
    end: u64,
}

#[derive(Debug, Clone, Serialize)]
struct OuterLayout {
    compressed_on_wire: bool,
    compression: CompressionState,
    frame_length_bytes: usize,
    nested_frame_count: Option<usize>,
    nested_frame_lengths_bytes: Option<Vec<usize>>,
    unique_tcp_stream_chunks_spanned: usize,
}

#[derive(Debug, Clone, Serialize)]
struct MarkerLayout {
    marker_number: u8,
    application_length_bytes: usize,
    nested_call_compressed_on_wire: bool,
    nested_call_compression: CompressionState,
    nested_call_frame_length_bytes: usize,
    application_offset_in_uncompressed_call_frame_bytes: usize,
    application_wire_offset_known: bool,
    outer_frame_up: Option<OuterLayout>,
}

struct WireAnalyzer<F> {
    filter: GameConnectionFilter,
    network: NetworkDecoder,
    tcp: TcpReassembler,
    framing: BpsrFramerSet,
    chunks: HashMap<TcpFlowKey, Vec<ChunkSpan>>,
    outers: HashMap<(TcpFlowKey, u64), OuterLayout>,
    markers: Vec<MarkerLayout>,
    physical_payload_segments: u64,
    marker_classifier: F,
}

impl<F> WireAnalyzer<F>
where
    F: FnMut(&[u8]) -> Option<u8>,
{
    fn new(
        filter: GameConnectionFilter,
        frame_up_layout: BpsrFrameUpLayout,
        marker_classifier: F,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            filter,
            network: NetworkDecoder::new(),
            tcp: TcpReassembler::new(),
            framing: BpsrFramerSet::try_with_config(BpsrFramerSetConfig {
                stream: BpsrFramingConfig {
                    frame_up_layout,
                    ..BpsrFramingConfig::default()
                },
                ..BpsrFramerSetConfig::default()
            })?,
            chunks: HashMap::new(),
            outers: HashMap::new(),
            markers: Vec::new(),
            physical_payload_segments: 0,
            marker_classifier,
        })
    }

    fn process_frame(
        &mut self,
        frame: &rlogs_capture::CapturedFrame,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let Self {
            filter,
            network,
            tcp,
            framing,
            chunks,
            outers,
            markers,
            physical_payload_segments,
            marker_classifier,
        } = self;
        let mut error = None;
        network.process_frame(frame, |event| {
            let NetworkDecodeEvent::Tcp(segment) = event else {
                return;
            };
            let Some(identity) = filter.classify(segment.flow) else {
                return;
            };
            if !segment.payload.is_empty() {
                *physical_payload_segments = physical_payload_segments.saturating_add(1);
            }
            tcp.process(segment, |stream_event| {
                if let TcpStreamEvent::Chunk(chunk) = &stream_event {
                    if chunks.values().map(Vec::len).sum::<usize>() >= MAX_CHUNK_SPANS {
                        error = Some("capture exceeds the bounded TCP chunk-span limit");
                        return;
                    }
                    chunks.entry(chunk.flow).or_default().push(ChunkSpan {
                        start: chunk.stream_offset,
                        end: chunk.stream_offset.saturating_add(chunk.bytes.len() as u64),
                    });
                }
                let direction = match identity.direction {
                    TransportDirection::ClientToServer => PacketDirection::ClientToServer,
                    TransportDirection::ServerToClient => PacketDirection::ServerToClient,
                };
                framing.process(direction, stream_event, |event| {
                    if let BpsrFramingEvent::Frame(frame) = event {
                        collect_frame(frame, chunks, outers, markers, marker_classifier);
                    }
                });
            });
        });
        if let Some(error) = error {
            return Err(error.into());
        }
        if self.markers.len() > MAX_MARKER_REQUESTS {
            return Err("capture exceeds the bounded marker-request limit".into());
        }
        Ok(())
    }

    fn finish(self) -> CaptureAnalysis {
        CaptureAnalysis {
            markers: self.markers,
            physical_payload_segments: self.physical_payload_segments,
            metrics: self.tcp.metrics().clone(),
        }
    }
}

fn collect_frame(
    frame: BpsrFrame,
    chunks: &HashMap<TcpFlowKey, Vec<ChunkSpan>>,
    outers: &mut HashMap<(TcpFlowKey, u64), OuterLayout>,
    markers: &mut Vec<MarkerLayout>,
    marker_classifier: &mut impl FnMut(&[u8]) -> Option<u8>,
) {
    if frame.direction != PacketDirection::ClientToServer {
        return;
    }
    if frame.fragment == FragmentKind::FrameUp && frame.nesting_depth == 0 {
        let frame_end = frame
            .stream_offset
            .saturating_add(frame.wire_bytes.len() as u64);
        let unique_tcp_stream_chunks_spanned = chunks
            .get(&frame.flow)
            .into_iter()
            .flatten()
            .filter(|span| span.start < frame_end && span.end > frame.stream_offset)
            .count();
        let nested_lengths = frame
            .application_bytes
            .as_deref()
            .and_then(nested_frame_lengths);
        outers.insert(
            (frame.flow, frame.stream_offset),
            OuterLayout {
                compressed_on_wire: frame.compressed_on_wire,
                compression: frame.compression,
                frame_length_bytes: frame.wire_bytes.len(),
                nested_frame_count: nested_lengths.as_ref().map(Vec::len),
                nested_frame_lengths_bytes: nested_lengths,
                unique_tcp_stream_chunks_spanned,
            },
        );
        return;
    }
    let Some(route) = frame.route else {
        return;
    };
    if !is_use_slot_call(
        route.key.direction,
        route.key.fragment,
        route.key.service_id,
        route.key.method_id,
    ) {
        return;
    }
    let Some(application) = frame.application_bytes.as_deref() else {
        return;
    };
    let Some(marker_number) = marker_classifier(application) else {
        return;
    };
    markers.push(MarkerLayout {
        marker_number,
        application_length_bytes: application.len(),
        nested_call_compressed_on_wire: frame.compressed_on_wire,
        nested_call_compression: frame.compression,
        nested_call_frame_length_bytes: frame.wire_bytes.len(),
        application_offset_in_uncompressed_call_frame_bytes: 26,
        application_wire_offset_known: !frame.compressed_on_wire,
        outer_frame_up: outers.get(&(frame.flow, frame.stream_offset)).cloned(),
    });
}

fn nested_frame_lengths(bytes: &[u8]) -> Option<Vec<usize>> {
    let mut result = Vec::new();
    let mut offset = 0usize;
    while offset < bytes.len() {
        let header = bytes.get(offset..offset.checked_add(6)?)?;
        let length = u32::from_be_bytes(header[..4].try_into().ok()?) as usize;
        if length < 6 || offset.checked_add(length)? > bytes.len() {
            return None;
        }
        result.push(length);
        offset += length;
    }
    Some(result)
}

struct CaptureAnalysis {
    markers: Vec<MarkerLayout>,
    physical_payload_segments: u64,
    metrics: ReassemblyMetrics,
}

#[derive(Serialize)]
struct Receipt {
    schema_version: u16,
    audit_kind: &'static str,
    input_scope: InputScope,
    journal_marker_request_count: usize,
    marker_requests: Vec<MarkerLayout>,
    filtered_tcp: FilteredTcpAggregate,
    interpretation: Interpretation,
    privacy: Privacy,
}

#[derive(Serialize)]
struct InputScope {
    pcap_or_pcapng_read_offline: bool,
    protocol_journal_read_offline: bool,
    packet_transmission_performed: bool,
    packet_replay_performed: bool,
    packet_modification_performed: bool,
}

#[derive(Serialize)]
struct FilteredTcpAggregate {
    physical_payload_segments: u64,
    segments_seen: u64,
    payload_bytes_seen: u64,
    in_order_segments: u64,
    reordered_segments: u64,
    duplicate_segments: u64,
    retransmitted_bytes: u64,
    overlap_bytes: u64,
    stream_chunks: u64,
    stream_bytes: u64,
    forced_gaps: u64,
}

impl FilteredTcpAggregate {
    fn from(physical_payload_segments: u64, metrics: &ReassemblyMetrics) -> Self {
        Self {
            physical_payload_segments,
            segments_seen: metrics.segments_seen,
            payload_bytes_seen: metrics.payload_bytes_seen,
            in_order_segments: metrics.in_order_segments,
            reordered_segments: metrics.reordered_segments,
            duplicate_segments: metrics.duplicate_segments,
            retransmitted_bytes: metrics.retransmitted_bytes,
            overlap_bytes: metrics.overlap_bytes,
            stream_chunks: metrics.stream_chunks,
            stream_bytes: metrics.stream_bytes,
            forced_gaps: metrics.forced_gaps,
        }
    }
}

#[derive(Serialize)]
struct Interpretation {
    application_equal_length_is_not_wire_feasibility: bool,
    compression_or_retransmission_requires_inline_transport_proof: bool,
    server_acceptance_proven: bool,
    anti_cheat_safety_proven: bool,
    runtime_sender_enabled: bool,
}

#[derive(Serialize)]
struct Privacy {
    contains_raw_payloads: bool,
    contains_network_endpoints: bool,
    contains_ports: bool,
    contains_tcp_sequence_numbers: bool,
    contains_rpc_call_ids: bool,
    contains_session_or_action_identifiers: bool,
    contains_local_absolute_paths: bool,
    contains_personal_identity: bool,
}

#[derive(Debug)]
struct Arguments {
    pack: PathBuf,
    connections: PathBuf,
    journal: PathBuf,
    capture: PathBuf,
    output: PathBuf,
}

impl Arguments {
    fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Self, String> {
        let mut pack = None;
        let mut connections = None;
        let mut journal = None;
        let mut output = None;
        let mut private_research = false;
        let mut positional = Vec::new();
        let mut arguments = arguments.into_iter();
        while let Some(argument) = arguments.next() {
            if argument == OsStr::new("--private-research") {
                private_research = true;
            } else if argument == OsStr::new("--pack") {
                pack = unique(pack, arguments.next(), "--pack")?;
            } else if argument == OsStr::new("--connections") {
                connections = unique(connections, arguments.next(), "--connections")?;
            } else if argument == OsStr::new("--journal") {
                journal = unique(journal, arguments.next(), "--journal")?;
            } else if argument == OsStr::new("--output") {
                output = unique(output, arguments.next(), "--output")?;
            } else if argument.to_string_lossy().starts_with('-') {
                return Err(Self::usage());
            } else {
                positional.push(PathBuf::from(argument));
            }
        }
        if !private_research || positional.len() != 1 {
            return Err(Self::usage());
        }
        Ok(Self {
            pack: pack.map(PathBuf::from).ok_or_else(Self::usage)?,
            connections: connections.map(PathBuf::from).ok_or_else(Self::usage)?,
            journal: journal.map(PathBuf::from).ok_or_else(Self::usage)?,
            capture: positional.remove(0),
            output: output.map(PathBuf::from).ok_or_else(Self::usage)?,
        })
    }

    fn usage() -> String {
        "usage: rlogs-bpsr-automarker-wire-layout-receipt --private-research --pack <pack.json> --connections <connections.json> --journal <protocol.jsonl> --output <receipt.json> <capture.pcap|capture.pcapng>".into()
    }
}

fn unique(
    current: Option<OsString>,
    value: Option<OsString>,
    name: &str,
) -> Result<Option<OsString>, String> {
    if current.is_some() || value.is_none() {
        return Err(format!("{name} must be supplied exactly once"));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use bytes::Bytes;
    use etherparse::{PacketBuilder, TcpHeader};
    use rlogs_capture::{CaptureLinkType, CapturedFrame, TimestampNormalization};
    use rlogs_core::GameConnection;
    use rlogs_network::IpEndpoint;

    use super::*;

    #[test]
    fn synthetic_split_and_retransmitted_frame_emits_only_sanitized_aggregates() {
        let client = endpoint(1, 31_000);
        let server = endpoint(2, 32_000);
        let mut nested_payload = Vec::new();
        nested_payload.extend_from_slice(&WORLD_SERVICE.to_be_bytes());
        nested_payload.extend_from_slice(&1_u32.to_be_bytes());
        nested_payload.extend_from_slice(&77_u32.to_be_bytes());
        nested_payload.extend_from_slice(&USE_SLOT_METHOD.to_be_bytes());
        nested_payload.extend_from_slice(&[0_u8; 161]);
        let nested = bpsr_frame(1, &nested_payload);
        let mut outer_payload = 56_u32.to_be_bytes().to_vec();
        outer_payload.extend_from_slice(&nested);
        let outer = bpsr_frame(5, &outer_payload);
        assert_eq!(outer.len(), 197);

        let split = 100usize;
        let frames = [
            tcp_frame(1, client, server, 100, &outer[..split]),
            tcp_frame(2, client, server, 100 + split as u32, &outer[split..]),
            tcp_frame(3, client, server, 100, &outer[..split]),
        ];
        let filter =
            GameConnectionFilter::try_new(vec![GameConnection { client, server }]).unwrap();
        let mut analyzer =
            WireAnalyzer::new(filter, BpsrFrameUpLayout::NestedAfterFourBytes, |body| {
                (body.len() == 161).then_some(1)
            })
            .unwrap();
        for frame in &frames {
            analyzer.process_frame(frame).unwrap();
        }
        let analysis = analyzer.finish();

        assert_eq!(analysis.markers.len(), 1);
        let marker = &analysis.markers[0];
        assert_eq!(marker.application_length_bytes, 161);
        assert_eq!(marker.nested_call_frame_length_bytes, 187);
        let outer = marker.outer_frame_up.as_ref().unwrap();
        assert_eq!(outer.frame_length_bytes, 197);
        assert_eq!(outer.nested_frame_count, Some(1));
        assert_eq!(outer.nested_frame_lengths_bytes, Some(vec![187]));
        assert_eq!(outer.unique_tcp_stream_chunks_spanned, 2);
        assert_eq!(analysis.metrics.duplicate_segments, 1);
        assert_eq!(analysis.metrics.retransmitted_bytes, 100);

        let aggregate =
            FilteredTcpAggregate::from(analysis.physical_payload_segments, &analysis.metrics);
        let json = serde_json::to_string(&aggregate).unwrap();
        assert!(!json.contains("10.0.0"));
        assert!(!json.contains("31000"));
        assert!(!json.contains("249858"));
        assert!(!json.contains("77"));
    }

    #[test]
    fn nested_frame_length_parser_rejects_truncation() {
        let valid = bpsr_frame(4, b"abc");
        assert_eq!(nested_frame_lengths(&valid), Some(vec![9]));
        assert_eq!(nested_frame_lengths(&valid[..8]), None);
    }

    fn endpoint(last: u8, port: u16) -> IpEndpoint {
        IpEndpoint::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, last)), port)
    }

    fn bpsr_frame(fragment: u16, payload: &[u8]) -> Vec<u8> {
        let length = 6 + payload.len();
        let mut bytes = Vec::with_capacity(length);
        bytes.extend_from_slice(&(length as u32).to_be_bytes());
        bytes.extend_from_slice(&fragment.to_be_bytes());
        bytes.extend_from_slice(payload);
        bytes
    }

    fn tcp_frame(
        sequence: u64,
        source: IpEndpoint,
        destination: IpEndpoint,
        tcp_sequence: u32,
        payload: &[u8],
    ) -> CapturedFrame {
        let tcp = TcpHeader::new(source.port, destination.port, tcp_sequence, 16_384);
        let builder = PacketBuilder::ipv4(
            match source.address {
                IpAddr::V4(address) => address.octets(),
                IpAddr::V6(_) => unreachable!(),
            },
            match destination.address {
                IpAddr::V4(address) => address.octets(),
                IpAddr::V6(_) => unreachable!(),
            },
            64,
        )
        .tcp_header(tcp);
        let mut packet = Vec::with_capacity(builder.size(payload.len()));
        builder.write(&mut packet, payload).unwrap();
        CapturedFrame {
            sequence,
            observed_micros: sequence,
            source_timestamp_nanos: Some(sequence as i64 * 1_000),
            timestamp_normalization: TimestampNormalization::Exact,
            interface_id: Some(0),
            link_type: CaptureLinkType::RawIpv4,
            original_length: packet.len() as u32,
            bytes: Bytes::from(packet),
        }
    }
}
