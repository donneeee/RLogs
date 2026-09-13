//! Passive Windows boundary diagnostic for future automarker transport work.
//!
//! This executable captures only BPSR-signature-confirmed TCP flows. It never
//! persists packets and has no send, inject, block, rewrite, process-memory, or
//! input-automation capability. Its create-only receipt deliberately omits
//! endpoints, ports, TCP sequence/acknowledgement values, payloads, call IDs,
//! timestamps, local paths, and account/action/session identifiers.

#[cfg(not(windows))]
fn main() {
    eprintln!("automarker Windows boundary diagnostic is available only on Windows");
    std::process::exit(1);
}

#[cfg(windows)]
mod windows {
    use std::{
        collections::{BTreeMap, BTreeSet, HashMap},
        env,
        ffi::{OsStr, OsString},
        fs::OpenOptions,
        io::{BufWriter, Write},
        path::PathBuf,
    };

    use rlogs_capture::{
        SignatureFlowCaptureConfig, TcpConnection, ValidatedCapture, WindowsProcessSocketOwner,
        WindowsRouteAwareCaptureMode, WindowsSignatureLiveCapture, npcap_device_name,
        recommend_windows_capture_adapter, windows_capture_adapters,
    };
    use rlogs_game_bpsr::{
        AUTOMARKER_REQUEST_BUILD, AutomarkerRequestXyz, BpsrFrame, BpsrFrameUpLayout,
        BpsrFramerSet, BpsrFramerSetConfig, BpsrFramingConfig, BpsrFramingEvent, FragmentKind,
        MappingProvenance, PacketDirection, ProtocolPack, classify_bpsr_tcp_prefix,
        decode_observed_automarker_request_into, supports_observed_automarker_requests,
        verify_offline_automarker_substitution,
    };
    use rlogs_network::{
        IpEndpoint, NetworkDecodeEvent, NetworkDecoder, ReassemblyMetrics, TcpFlowKey,
        TcpReassembler, TcpStreamEvent,
    };
    use serde::Serialize;

    const WORLD_SERVICE: u64 = 103_198_054;
    const USE_SLOT_METHOD: u32 = 249_858;
    const MAX_MARKER_REQUESTS: usize = 256;
    const MAX_CHUNK_SPANS: usize = 1_000_000;

    pub fn main() {
        if let Err(error) = run() {
            eprintln!("automarker Windows boundary diagnostic failed: {error}");
            std::process::exit(1);
        }
    }

    fn run() -> Result<(), Box<dyn std::error::Error>> {
        let raw_args = env::args_os().skip(1).collect::<Vec<_>>();
        if raw_args.as_slice() == [OsStr::new("--schema-check")] {
            println!("{}", serde_json::to_string(&schema_capabilities())?);
            return Ok(());
        }
        let args = Arguments::parse(raw_args)?;
        if args.output.exists() {
            return Err(format!("refusing to overwrite {}", args.output.display()).into());
        }

        let adapters = windows_capture_adapters()?;
        let recommendation = recommend_windows_capture_adapter(&adapters, &[args.process_id])
            .ok_or("no bounded Windows capture adapter could be selected")?;
        let primary_interface = npcap_device_name(&recommendation.adapter_name);
        let mode = if args.exitlag {
            WindowsRouteAwareCaptureMode::ExitLag
        } else {
            WindowsRouteAwareCaptureMode::Standard
        };
        let capture = WindowsSignatureLiveCapture::open_route_aware_prefix(
            &primary_interface,
            &[args.process_id],
            mode,
            args.duration_seconds,
            None,
            classify_bpsr_tcp_prefix,
            SignatureFlowCaptureConfig::default(),
        )?;
        let owner = WindowsProcessSocketOwner::new(args.process_id)?;
        let pack = current_automarker_pack()?;
        let mut capture = ValidatedCapture::new(capture);
        let mut analyzer = BoundaryAnalyzer::new(pack)?;

        println!(
            "Passive diagnostic running for {} seconds. Place one marker normally; no traffic will be transmitted or modified.",
            args.duration_seconds
        );
        while let Some(frame) = capture.next_frame()? {
            let confirmed = capture.source().confirmed_connections();
            let owned = owner.snapshot_all_connections().unwrap_or_default();
            analyzer.process_frame(&frame, &confirmed, &owned)?;
        }
        let receipt = analyzer.finish(args.exitlag);
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args.output)?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer_pretty(&mut writer, &receipt)?;
        writer.write_all(b"\n")?;
        writer.flush()?;
        writer.get_ref().sync_all()?;
        println!("Sanitized create-only boundary receipt written.");
        Ok(())
    }

    fn current_automarker_pack() -> Result<ProtocolPack, Box<dyn std::error::Error>> {
        let source = ProtocolPack::from_json(include_bytes!(
            "../protocol-packs/global/steam-24687926/pack.json"
        ))?;
        let source_build = source.definition().target.build_id.clone();
        let mut definition = source.definition().clone();
        definition.pack_id = format!("{}-compatibility-fallback-steam", definition.pack_id);
        definition.target.deployment_id = "global".into();
        definition.target.region_id = None;
        definition.target.channel = "steam".into();
        definition.target.build_id = AUTOMARKER_REQUEST_BUILD.into();
        definition.provenance.push(MappingProvenance {
            source: "provisional-compatibility-fallback".into(),
            reference: format!(
                "pack_build={source_build};client_deployment=global;client_channel=steam;client_build={AUTOMARKER_REQUEST_BUILD}"
            ),
        });
        let pack = ProtocolPack::build(definition)?;
        if !supports_observed_automarker_requests(&pack) {
            return Err(
                "derived pack is not the exact reviewed automarker request identity".into(),
            );
        }
        Ok(pack)
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
    #[serde(rename_all = "snake_case")]
    enum CaptureSurface {
        Loopback,
        PhysicalOrRouted,
    }

    fn surface(connection: TcpConnection) -> CaptureSurface {
        if connection.client.address.is_loopback() && connection.server.address.is_loopback() {
            CaptureSurface::Loopback
        } else {
            CaptureSurface::PhysicalOrRouted
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    struct ConnectionKey(IpEndpoint, IpEndpoint);

    impl ConnectionKey {
        fn new(left: IpEndpoint, right: IpEndpoint) -> Self {
            if left <= right {
                Self(left, right)
            } else {
                Self(right, left)
            }
        }
    }

    fn network_endpoint(endpoint: rlogs_capture::TcpEndpoint) -> IpEndpoint {
        IpEndpoint::new(endpoint.address, endpoint.port)
    }

    fn connection_key(connection: TcpConnection) -> ConnectionKey {
        ConnectionKey::new(
            network_endpoint(connection.client),
            network_endpoint(connection.server),
        )
    }

    #[derive(Debug, Default)]
    struct EpochState {
        ordinal: u32,
        active: bool,
        syn_observed: bool,
        process_owned_observed: bool,
    }

    #[derive(Debug, Clone, Copy)]
    struct ChunkSpan {
        start: u64,
        end: u64,
    }

    #[derive(Debug, Clone)]
    struct OuterLayout {
        compressed: bool,
        chunks_spanned: usize,
        exact_single_chunk: bool,
    }

    #[derive(Debug, Serialize)]
    struct MarkerObservation {
        marker_number: u8,
        surface: CaptureSurface,
        connection_epoch_ordinal: u32,
        epoch_syn_observed: bool,
        exact_game_process_ownership_observed: bool,
        application_length_bytes: usize,
        application_uncompressed_on_wire: bool,
        outer_frame_uncompressed_on_wire: bool,
        tcp_stream_chunks_spanned: Option<usize>,
        exactly_one_tcp_stream_chunk: bool,
        approved_application_value_bytes: usize,
        all_16_approved_application_bytes_reconstructable: bool,
        all_16_approved_bytes_directly_locatable_on_wire: bool,
    }

    struct BoundaryAnalyzer {
        pack: ProtocolPack,
        network: NetworkDecoder,
        tcp: TcpReassembler,
        framing: BpsrFramerSet,
        chunks: HashMap<TcpFlowKey, Vec<ChunkSpan>>,
        outers: HashMap<(TcpFlowKey, u64), OuterLayout>,
        epochs: BTreeMap<ConnectionKey, EpochState>,
        confirmed: BTreeMap<ConnectionKey, TcpConnection>,
        owned_ever: BTreeSet<ConnectionKey>,
        markers: Vec<MarkerObservation>,
        physical_payload_segments: u64,
        decoded_tcp_segments: u64,
    }

    impl BoundaryAnalyzer {
        fn new(pack: ProtocolPack) -> Result<Self, Box<dyn std::error::Error>> {
            Ok(Self {
                pack,
                network: NetworkDecoder::new(),
                tcp: TcpReassembler::new(),
                framing: BpsrFramerSet::try_with_config(BpsrFramerSetConfig {
                    stream: BpsrFramingConfig {
                        frame_up_layout: BpsrFrameUpLayout::NestedAfterFourBytes,
                        ..BpsrFramingConfig::default()
                    },
                    ..BpsrFramerSetConfig::default()
                })?,
                chunks: HashMap::new(),
                outers: HashMap::new(),
                epochs: BTreeMap::new(),
                confirmed: BTreeMap::new(),
                owned_ever: BTreeSet::new(),
                markers: Vec::new(),
                physical_payload_segments: 0,
                decoded_tcp_segments: 0,
            })
        }

        fn process_frame(
            &mut self,
            frame: &rlogs_capture::CapturedFrame,
            confirmed: &[TcpConnection],
            owned: &[TcpConnection],
        ) -> Result<(), Box<dyn std::error::Error>> {
            for connection in confirmed {
                self.confirmed
                    .insert(connection_key(*connection), *connection);
            }
            for connection in owned {
                self.owned_ever.insert(connection_key(*connection));
            }
            let Self {
                pack,
                network,
                tcp,
                framing,
                chunks,
                outers,
                epochs,
                confirmed,
                owned_ever,
                markers,
                physical_payload_segments,
                decoded_tcp_segments,
            } = self;
            let mut failure = None;
            network.process_frame(frame, |event| {
                let NetworkDecodeEvent::Tcp(segment) = event else {
                    return;
                };
                *decoded_tcp_segments = decoded_tcp_segments.saturating_add(1);
                let key = ConnectionKey::new(segment.flow.source, segment.flow.destination);
                let Some(connection) = confirmed.get(&key).copied() else {
                    return;
                };
                if !segment.payload.is_empty() {
                    *physical_payload_segments = physical_payload_segments.saturating_add(1);
                }
                let epoch = epochs.entry(key).or_default();
                if segment.flags.syn || !epoch.active {
                    epoch.ordinal = epoch.ordinal.saturating_add(1).max(1);
                    epoch.active = true;
                    epoch.syn_observed = segment.flags.syn;
                }
                epoch.process_owned_observed |= owned_ever.contains(&key);
                if segment.flags.fin || segment.flags.rst {
                    epoch.active = false;
                }
                let direction = if segment.flow.source == network_endpoint(connection.client) {
                    PacketDirection::ClientToServer
                } else {
                    PacketDirection::ServerToClient
                };
                tcp.process(segment, |stream_event| {
                    if let TcpStreamEvent::Chunk(chunk) = &stream_event {
                        if chunks.values().map(Vec::len).sum::<usize>() >= MAX_CHUNK_SPANS {
                            failure = Some("capture exceeds bounded TCP chunk-span limit");
                            return;
                        }
                        chunks.entry(chunk.flow).or_default().push(ChunkSpan {
                            start: chunk.stream_offset,
                            end: chunk.stream_offset.saturating_add(chunk.bytes.len() as u64),
                        });
                    }
                    framing.process(direction, stream_event, |event| {
                        if let BpsrFramingEvent::Frame(frame) = event {
                            collect_frame(pack, frame, chunks, outers, epochs, confirmed, markers);
                        }
                    });
                });
            });
            if let Some(message) = failure {
                return Err(message.into());
            }
            if self.markers.len() > MAX_MARKER_REQUESTS {
                return Err("capture exceeds bounded marker-request limit".into());
            }
            Ok(())
        }

        fn finish(self, exitlag_requested: bool) -> Receipt {
            let mut surfaces = BTreeMap::<CaptureSurface, SurfaceAggregate>::new();
            for connection in self.confirmed.values().copied() {
                let aggregate = surfaces.entry(surface(connection)).or_default();
                aggregate.signature_confirmed_connections =
                    aggregate.signature_confirmed_connections.saturating_add(1);
                aggregate.exact_game_process_owned_connections +=
                    usize::from(self.owned_ever.contains(&connection_key(connection)));
            }
            for marker in &self.markers {
                surfaces.entry(marker.surface).or_default().marker_requests += 1;
            }
            let loopback_marker_visible = surfaces
                .get(&CaptureSurface::Loopback)
                .is_some_and(|row| row.marker_requests > 0);
            let physical_marker_visible = surfaces
                .get(&CaptureSurface::PhysicalOrRouted)
                .is_some_and(|row| row.marker_requests > 0);
            Receipt {
                schema_version: 1,
                audit_kind: "sanitized-passive-automarker-windows-boundary",
                requested_mode: if exitlag_requested {
                    "exitlag"
                } else {
                    "standard"
                },
                input_scope: InputScope {
                    live_npcap_capture: true,
                    protocol_signature_gate: true,
                    process_socket_table_read: true,
                    packet_persistence_performed: false,
                    packet_transmission_performed: false,
                    packet_modification_performed: false,
                    packet_blocking_performed: false,
                    packet_reinjection_performed: false,
                    process_memory_access_performed: false,
                    input_automation_performed: false,
                },
                surfaces,
                connection_epochs_observed: self
                    .epochs
                    .values()
                    .map(|state| u64::from(state.ordinal))
                    .sum(),
                epochs_with_syn_observed: self
                    .epochs
                    .values()
                    .filter(|state| state.syn_observed)
                    .count(),
                marker_requests: self.markers,
                filtered_tcp: FilteredTcpAggregate::from(
                    self.decoded_tcp_segments,
                    self.physical_payload_segments,
                    self.tcp.metrics(),
                ),
                checksum_and_offload: ChecksumAndOffload {
                    tcp_checksum_validation_performed: false,
                    capture_point_offload_state_observable: false,
                    invalid_capture_checksum_would_distinguish_bad_wire_from_tx_offload: false,
                    future_inline_rewrite_must_recalculate_tcp_checksum: true,
                    future_inline_design_must_handle_segmentation_and_retransmission: true,
                },
                interpretation: Interpretation {
                    loopback_signature_proves_plain_bpsr_visible_before_local_proxy:
                        loopback_marker_visible,
                    physical_signature_proves_plain_bpsr_visible_on_routed_adapter:
                        physical_marker_visible,
                    process_ownership_is_exact_four_tuple_not_process_name_inference: true,
                    observation_proves_inline_ordering: false,
                    observation_proves_server_acceptance: false,
                    runtime_sender_enabled: false,
                },
                privacy: Privacy::sanitized(),
            }
        }
    }

    fn collect_frame(
        pack: &ProtocolPack,
        frame: BpsrFrame,
        chunks: &HashMap<TcpFlowKey, Vec<ChunkSpan>>,
        outers: &mut HashMap<(TcpFlowKey, u64), OuterLayout>,
        epochs: &BTreeMap<ConnectionKey, EpochState>,
        confirmed: &BTreeMap<ConnectionKey, TcpConnection>,
        markers: &mut Vec<MarkerObservation>,
    ) {
        if frame.direction != PacketDirection::ClientToServer {
            return;
        }
        if frame.fragment == FragmentKind::FrameUp && frame.nesting_depth == 0 {
            let end = frame
                .stream_offset
                .saturating_add(frame.wire_bytes.len() as u64);
            let spans = chunks
                .get(&frame.flow)
                .into_iter()
                .flatten()
                .filter(|span| span.start < end && span.end > frame.stream_offset)
                .collect::<Vec<_>>();
            let exact =
                spans.len() == 1 && spans[0].start == frame.stream_offset && spans[0].end == end;
            outers.insert(
                (frame.flow, frame.stream_offset),
                OuterLayout {
                    compressed: frame.compressed_on_wire,
                    chunks_spanned: spans.len(),
                    exact_single_chunk: exact,
                },
            );
            return;
        }
        let Some(route) = frame.route else { return };
        if route.key.direction != PacketDirection::ClientToServer
            || route.key.fragment != FragmentKind::Call
            || route.key.service_id != WORLD_SERVICE
            || route.key.method_id != USE_SLOT_METHOD
        {
            return;
        }
        let Some(application) = frame.application_bytes.as_deref() else {
            return;
        };
        let mut scratch = Vec::new();
        let Ok(request) = decode_observed_automarker_request_into(pack, application, &mut scratch)
        else {
            return;
        };
        let target = AutomarkerRequestXyz {
            x: request.target_position.x,
            y: request.target_position.y,
            z: request.target_position.z,
        };
        let proof = verify_offline_automarker_substitution(
            pack,
            application,
            request.marker_number,
            target,
        )
        .ok();
        let outer = outers.get(&(frame.flow, frame.stream_offset));
        let key = ConnectionKey::new(frame.flow.source, frame.flow.destination);
        let Some(connection) = confirmed.get(&key).copied() else {
            return;
        };
        let epoch = epochs.get(&key);
        let application_uncompressed = !frame.compressed_on_wire;
        let outer_uncompressed = outer.is_some_and(|layout| !layout.compressed);
        let reconstructable = proof.as_ref().is_some_and(|proof| {
            proof.allowed_mutable_bytes == 16 && proof.exact_build_decode_succeeded
        });
        markers.push(MarkerObservation {
            marker_number: request.marker_number,
            surface: surface(connection),
            connection_epoch_ordinal: epoch.map_or(0, |state| state.ordinal),
            epoch_syn_observed: epoch.is_some_and(|state| state.syn_observed),
            exact_game_process_ownership_observed: epoch
                .is_some_and(|state| state.process_owned_observed),
            application_length_bytes: application.len(),
            application_uncompressed_on_wire: application_uncompressed,
            outer_frame_uncompressed_on_wire: outer_uncompressed,
            tcp_stream_chunks_spanned: outer.map(|layout| layout.chunks_spanned),
            exactly_one_tcp_stream_chunk: outer.is_some_and(|layout| layout.exact_single_chunk),
            approved_application_value_bytes: proof.map_or(0, |proof| proof.allowed_mutable_bytes),
            all_16_approved_application_bytes_reconstructable: reconstructable,
            all_16_approved_bytes_directly_locatable_on_wire: reconstructable
                && application_uncompressed
                && outer_uncompressed,
        });
    }

    #[derive(Debug, Default, Serialize)]
    struct SurfaceAggregate {
        signature_confirmed_connections: usize,
        exact_game_process_owned_connections: usize,
        marker_requests: usize,
    }

    #[derive(Debug, Serialize)]
    struct Receipt {
        schema_version: u16,
        audit_kind: &'static str,
        requested_mode: &'static str,
        input_scope: InputScope,
        surfaces: BTreeMap<CaptureSurface, SurfaceAggregate>,
        connection_epochs_observed: u64,
        epochs_with_syn_observed: usize,
        marker_requests: Vec<MarkerObservation>,
        filtered_tcp: FilteredTcpAggregate,
        checksum_and_offload: ChecksumAndOffload,
        interpretation: Interpretation,
        privacy: Privacy,
    }

    #[derive(Debug, Serialize)]
    struct InputScope {
        live_npcap_capture: bool,
        protocol_signature_gate: bool,
        process_socket_table_read: bool,
        packet_persistence_performed: bool,
        packet_transmission_performed: bool,
        packet_modification_performed: bool,
        packet_blocking_performed: bool,
        packet_reinjection_performed: bool,
        process_memory_access_performed: bool,
        input_automation_performed: bool,
    }

    #[derive(Debug, Serialize)]
    struct FilteredTcpAggregate {
        decoded_tcp_segments: u64,
        physical_payload_segments: u64,
        in_order_segments: u64,
        reordered_segments: u64,
        duplicate_segments: u64,
        retransmitted_bytes: u64,
        overlap_bytes: u64,
        stream_chunks: u64,
        forced_gaps: u64,
    }

    impl FilteredTcpAggregate {
        fn from(decoded: u64, physical: u64, metrics: &ReassemblyMetrics) -> Self {
            Self {
                decoded_tcp_segments: decoded,
                physical_payload_segments: physical,
                in_order_segments: metrics.in_order_segments,
                reordered_segments: metrics.reordered_segments,
                duplicate_segments: metrics.duplicate_segments,
                retransmitted_bytes: metrics.retransmitted_bytes,
                overlap_bytes: metrics.overlap_bytes,
                stream_chunks: metrics.stream_chunks,
                forced_gaps: metrics.forced_gaps,
            }
        }
    }

    #[derive(Debug, Serialize)]
    struct ChecksumAndOffload {
        tcp_checksum_validation_performed: bool,
        capture_point_offload_state_observable: bool,
        invalid_capture_checksum_would_distinguish_bad_wire_from_tx_offload: bool,
        future_inline_rewrite_must_recalculate_tcp_checksum: bool,
        future_inline_design_must_handle_segmentation_and_retransmission: bool,
    }

    #[derive(Debug, Serialize)]
    struct Interpretation {
        loopback_signature_proves_plain_bpsr_visible_before_local_proxy: bool,
        physical_signature_proves_plain_bpsr_visible_on_routed_adapter: bool,
        process_ownership_is_exact_four_tuple_not_process_name_inference: bool,
        observation_proves_inline_ordering: bool,
        observation_proves_server_acceptance: bool,
        runtime_sender_enabled: bool,
    }

    #[derive(Debug, Serialize)]
    struct Privacy {
        contains_raw_payloads: bool,
        contains_network_endpoints: bool,
        contains_ports: bool,
        contains_tcp_sequence_or_acknowledgement_numbers: bool,
        contains_rpc_call_ids: bool,
        contains_timestamps: bool,
        contains_local_paths: bool,
        contains_account_action_or_session_identifiers: bool,
    }

    impl Privacy {
        fn sanitized() -> Self {
            Self {
                contains_raw_payloads: false,
                contains_network_endpoints: false,
                contains_ports: false,
                contains_tcp_sequence_or_acknowledgement_numbers: false,
                contains_rpc_call_ids: false,
                contains_timestamps: false,
                contains_local_paths: false,
                contains_account_action_or_session_identifiers: false,
            }
        }
    }

    #[derive(Debug, Serialize)]
    struct SchemaCapabilities {
        schema_version: u16,
        passive_capture_only: bool,
        process_owned_four_tuple_snapshot: bool,
        loopback_and_physical_surface_classification: bool,
        exact_application_mutable_byte_count: usize,
        sensitive_wire_fields_emitted: bool,
        packet_transmission_available: bool,
        packet_modification_available: bool,
    }

    fn schema_capabilities() -> SchemaCapabilities {
        SchemaCapabilities {
            schema_version: 1,
            passive_capture_only: true,
            process_owned_four_tuple_snapshot: true,
            loopback_and_physical_surface_classification: true,
            exact_application_mutable_byte_count: 16,
            sensitive_wire_fields_emitted: false,
            packet_transmission_available: false,
            packet_modification_available: false,
        }
    }

    struct Arguments {
        process_id: u32,
        duration_seconds: u32,
        exitlag: bool,
        output: PathBuf,
    }

    impl Arguments {
        fn parse(raw: Vec<OsString>) -> Result<Self, String> {
            let mut process_id = None;
            let mut duration = None;
            let mut output = None;
            let mut exitlag = false;
            let mut private = false;
            let mut args = raw.into_iter();
            while let Some(arg) = args.next() {
                if arg == OsStr::new("--private-research") {
                    private = true;
                } else if arg == OsStr::new("--exitlag") {
                    exitlag = true;
                } else if arg == OsStr::new("--process-id") {
                    process_id = unique(process_id, args.next(), "--process-id")?;
                } else if arg == OsStr::new("--duration-seconds") {
                    duration = unique(duration, args.next(), "--duration-seconds")?;
                } else if arg == OsStr::new("--output") {
                    output = unique(output, args.next(), "--output")?;
                } else {
                    return Err(Self::usage());
                }
            }
            if !private {
                return Err(Self::usage());
            }
            let process_id = parse_u32(process_id, "--process-id", 1, u32::MAX)?;
            let duration_seconds = parse_u32(duration, "--duration-seconds", 1, 600)?;
            Ok(Self {
                process_id,
                duration_seconds,
                exitlag,
                output: PathBuf::from(output.ok_or_else(Self::usage)?),
            })
        }

        fn usage() -> String {
            "usage: rlogs-bpsr-automarker-windows-boundary --private-research --process-id <pid> --duration-seconds <1-600> [--exitlag] --output <create-only-receipt.json>".into()
        }
    }

    fn unique(
        current: Option<OsString>,
        next: Option<OsString>,
        name: &str,
    ) -> Result<Option<OsString>, String> {
        if current.is_some() || next.is_none() {
            return Err(format!("{name} must be supplied exactly once"));
        }
        Ok(next)
    }

    fn parse_u32(value: Option<OsString>, name: &str, min: u32, max: u32) -> Result<u32, String> {
        let parsed = value
            .ok_or_else(Arguments::usage)?
            .to_str()
            .ok_or_else(Arguments::usage)?
            .parse::<u32>()
            .map_err(|_| Arguments::usage())?;
        if !(min..=max).contains(&parsed) {
            return Err(format!("{name} must be between {min} and {max}"));
        }
        Ok(parsed)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::net::IpAddr;

        #[test]
        fn schema_is_passive_and_sanitized() {
            let json = serde_json::to_string(&schema_capabilities()).unwrap();
            assert!(json.contains("\"passive_capture_only\":true"));
            assert!(json.contains("\"sensitive_wire_fields_emitted\":false"));
            assert!(json.contains("\"packet_transmission_available\":false"));
            assert!(json.contains("\"packet_modification_available\":false"));
            assert!(!json.contains("endpoint"));
            assert!(!json.contains("sequence_number"));
        }

        #[test]
        fn surface_classification_does_not_emit_addresses() {
            let loopback = TcpConnection::new(
                rlogs_capture::TcpEndpoint::new(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), 1),
                rlogs_capture::TcpEndpoint::new(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), 2),
            );
            let external = TcpConnection::new(
                rlogs_capture::TcpEndpoint::new(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), 1),
                rlogs_capture::TcpEndpoint::new(IpAddr::V4(std::net::Ipv4Addr::new(1, 2, 3, 4)), 2),
            );
            assert_eq!(surface(loopback), CaptureSurface::Loopback);
            assert_eq!(surface(external), CaptureSurface::PhysicalOrRouted);
            let json = serde_json::to_string(&surface(external)).unwrap();
            assert_eq!(json, "\"physical_or_routed\"");
        }

        #[test]
        fn arguments_require_explicit_private_mode() {
            assert!(Arguments::parse(vec![]).is_err());
            let parsed = Arguments::parse(vec![
                "--private-research".into(),
                "--process-id".into(),
                "42".into(),
                "--duration-seconds".into(),
                "25".into(),
                "--exitlag".into(),
                "--output".into(),
                "safe.json".into(),
            ])
            .unwrap();
            assert_eq!(parsed.process_id, 42);
            assert_eq!(parsed.duration_seconds, 25);
            assert!(parsed.exitlag);
        }
    }
}

#[cfg(windows)]
fn main() {
    windows::main();
}
