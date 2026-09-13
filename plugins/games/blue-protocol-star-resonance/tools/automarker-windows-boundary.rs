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
        SignatureFlowCaptureConfig, SignatureFlowCaptureMetrics, TcpConnection, ValidatedCapture,
        WindowsProcessSocketOwner, WindowsRouteAwareCaptureMode, WindowsSignatureLiveCapture,
        npcap_device_name, recommend_windows_capture_adapter, windows_capture_adapters,
    };
    use rlogs_game_bpsr::{
        AUTOMARKER_REQUEST_BUILD, AutomarkerRequestXyz, BpsrFrame, BpsrFrameUpLayout,
        BpsrFramerSet, BpsrFramerSetConfig, BpsrFramingConfig, BpsrFramingEvent, FragmentKind,
        MappingProvenance, PacketDirection, ProtocolPack, classify_bpsr_tcp_prefix,
        classify_observed_automarker_tcp_prefix, decode_observed_automarker_request_into,
        supports_observed_automarker_requests, verify_offline_automarker_substitution,
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
        let process_ids = args.process_id.into_iter().collect::<Vec<_>>();
        let primary_adapter =
            resolve_primary_adapter(&adapters, &process_ids, args.interface_name.as_deref())?;
        let primary_interface = npcap_device_name(&primary_adapter);
        let mode = if args.exitlag && !args.mirror {
            WindowsRouteAwareCaptureMode::ExitLag
        } else {
            WindowsRouteAwareCaptureMode::Standard
        };
        let capture = WindowsSignatureLiveCapture::open_route_aware_prefix(
            &primary_interface,
            &process_ids,
            mode,
            args.duration_seconds,
            None,
            classify_boundary_tcp_prefix,
            SignatureFlowCaptureConfig::default(),
        )?;
        let owner = args
            .process_id
            .map(WindowsProcessSocketOwner::new)
            .transpose()?;
        let pack = current_automarker_pack()?;
        let mut capture = ValidatedCapture::new(capture);
        let mut analyzer = BoundaryAnalyzer::new(pack, owner.is_some())?;

        println!(
            "Passive diagnostic running for {} seconds. Place one marker normally; no traffic will be transmitted or modified.",
            args.duration_seconds
        );
        while let Some(frame) = capture.next_frame()? {
            let confirmed = capture.source().confirmed_connections();
            let owned = owner
                .as_ref()
                .map(WindowsProcessSocketOwner::snapshot_all_connections)
                .transpose()
                .unwrap_or_default()
                .unwrap_or_default();
            analyzer.process_frame(&frame, &confirmed, &owned)?;
        }
        let signature_filter = capture.source().metrics().clone();
        let receipt = analyzer.finish(args.capture_mode(), &signature_filter);
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

    fn classify_boundary_tcp_prefix(payload: &[u8]) -> rlogs_capture::TcpPayloadSignatureResult {
        let early = classify_bpsr_tcp_prefix(payload);
        if matches!(early, rlogs_capture::TcpPayloadSignatureResult::Match(_)) {
            return early;
        }
        classify_observed_automarker_tcp_prefix(payload)
    }

    fn resolve_primary_adapter(
        adapters: &[rlogs_capture::WindowsCaptureAdapter],
        process_ids: &[u32],
        requested: Option<&str>,
    ) -> Result<String, Box<dyn std::error::Error>> {
        if let Some(requested) = requested {
            let requested = requested.trim();
            if requested.is_empty()
                || requested.len() > 256
                || requested.chars().any(char::is_control)
            {
                return Err("interface name must be 1-256 printable characters".into());
            }
            let mut matches = adapters.iter().filter(|adapter| {
                [
                    adapter.adapter_name.as_str(),
                    adapter.friendly_name.as_str(),
                    adapter.description.as_str(),
                ]
                .into_iter()
                .any(|value| value.eq_ignore_ascii_case(requested))
                    || npcap_device_name(&adapter.adapter_name).eq_ignore_ascii_case(requested)
            });
            let Some(adapter) = matches.next() else {
                return Err("interface name did not uniquely match a local capture adapter".into());
            };
            if matches.next().is_some() {
                return Err("interface name matched more than one local capture adapter".into());
            }
            return Ok(adapter.adapter_name.clone());
        }
        recommend_windows_capture_adapter(adapters, process_ids)
            .map(|recommendation| recommendation.adapter_name)
            .ok_or_else(|| "no bounded Windows capture adapter could be selected".into())
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
        connection_ordinal: u32,
        connection_epoch_ordinal: u32,
        epoch_syn_observed: bool,
        process_ownership_evidence: ProcessOwnershipEvidence,
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
        connection_ordinals: BTreeMap<ConnectionKey, u32>,
        owned_ever: BTreeSet<ConnectionKey>,
        markers: Vec<MarkerObservation>,
        physical_payload_segments: u64,
        decoded_tcp_segments: u64,
        process_ownership_available: bool,
        bpsr_diagnostics: BpsrDiagnosticAggregate,
    }

    impl BoundaryAnalyzer {
        fn new(
            pack: ProtocolPack,
            process_ownership_available: bool,
        ) -> Result<Self, Box<dyn std::error::Error>> {
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
                connection_ordinals: BTreeMap::new(),
                owned_ever: BTreeSet::new(),
                markers: Vec::new(),
                physical_payload_segments: 0,
                decoded_tcp_segments: 0,
                process_ownership_available,
                bpsr_diagnostics: BpsrDiagnosticAggregate::default(),
            })
        }

        fn process_frame(
            &mut self,
            frame: &rlogs_capture::CapturedFrame,
            confirmed: &[TcpConnection],
            owned: &[TcpConnection],
        ) -> Result<(), Box<dyn std::error::Error>> {
            for connection in confirmed {
                let key = connection_key(*connection);
                self.confirmed.insert(key, *connection);
                if !self.connection_ordinals.contains_key(&key) {
                    let ordinal = u32::try_from(self.connection_ordinals.len())
                        .unwrap_or(u32::MAX)
                        .saturating_add(1);
                    self.connection_ordinals.insert(key, ordinal);
                }
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
                connection_ordinals,
                owned_ever,
                markers,
                physical_payload_segments,
                decoded_tcp_segments,
                process_ownership_available,
                bpsr_diagnostics,
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
                            collect_frame(
                                pack,
                                frame,
                                chunks,
                                outers,
                                epochs,
                                confirmed,
                                connection_ordinals,
                                markers,
                                *process_ownership_available,
                                bpsr_diagnostics,
                            );
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

        fn finish(
            self,
            capture_mode: CaptureMode,
            signature_filter: &SignatureFlowCaptureMetrics,
        ) -> Receipt {
            let mut surfaces = BTreeMap::<CaptureSurface, SurfaceAggregate>::new();
            for connection in self.confirmed.values().copied() {
                let aggregate = surfaces.entry(surface(connection)).or_default();
                aggregate.signature_confirmed_connections =
                    aggregate.signature_confirmed_connections.saturating_add(1);
                if self.process_ownership_available {
                    aggregate.exact_game_process_owned_connections_observed = Some(
                        aggregate
                            .exact_game_process_owned_connections_observed
                            .unwrap_or_default()
                            + usize::from(self.owned_ever.contains(&connection_key(connection))),
                    );
                    aggregate.process_ownership_scope = "exact_four_tuple_poll";
                } else {
                    aggregate.exact_game_process_owned_connections_observed = None;
                    aggregate.process_ownership_scope = "unproven_mirror_capture";
                }
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
            let game_process_ownership_proven = self.process_ownership_available
                && self.markers.iter().any(|marker| {
                    marker.process_ownership_evidence
                        == ProcessOwnershipEvidence::ExactGameProcessSocketObserved
                });
            let topology = assess_topology(&self.markers, capture_mode);
            Receipt {
                schema_version: 3,
                audit_kind: "sanitized-passive-automarker-windows-boundary",
                requested_mode: capture_mode.as_str(),
                input_scope: InputScope {
                    live_npcap_capture: true,
                    protocol_signature_gate: true,
                    process_socket_table_read: self.process_ownership_available,
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
                signature_filter: SignatureFilterAggregate::from(signature_filter),
                bpsr_diagnostics: self.bpsr_diagnostics,
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
                topology,
                interpretation: Interpretation {
                    plain_bpsr_marker_visible_on_loopback_capture_surface: loopback_marker_visible,
                    plain_bpsr_marker_visible_on_physical_or_routed_capture_surface:
                        physical_marker_visible,
                    process_ownership_is_exact_four_tuple_not_process_name_inference: true,
                    game_process_ownership_proven,
                    mirror_mode_never_infers_game_process_ownership: true,
                    wfp_and_exitlag_callout_ordering: "unproven",
                    exitlag_mode_was_operator_selected: capture_mode == CaptureMode::ExitLag,
                    exitlag_process_or_driver_state_observed: false,
                    observation_proves_inline_ordering: false,
                    observation_proves_server_acceptance: false,
                    runtime_sender_enabled: false,
                },
                privacy: Privacy::sanitized(),
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn collect_frame(
        pack: &ProtocolPack,
        frame: BpsrFrame,
        chunks: &HashMap<TcpFlowKey, Vec<ChunkSpan>>,
        outers: &mut HashMap<(TcpFlowKey, u64), OuterLayout>,
        epochs: &BTreeMap<ConnectionKey, EpochState>,
        confirmed: &BTreeMap<ConnectionKey, TcpConnection>,
        connection_ordinals: &BTreeMap<ConnectionKey, u32>,
        markers: &mut Vec<MarkerObservation>,
        process_ownership_available: bool,
        diagnostics: &mut BpsrDiagnosticAggregate,
    ) {
        diagnostics.framed_records = diagnostics.framed_records.saturating_add(1);
        match frame.direction {
            PacketDirection::ClientToServer => {
                diagnostics.client_to_server_records =
                    diagnostics.client_to_server_records.saturating_add(1);
            }
            PacketDirection::ServerToClient => {
                diagnostics.server_to_client_records =
                    diagnostics.server_to_client_records.saturating_add(1);
            }
            PacketDirection::Unknown => {
                diagnostics.unknown_direction_records =
                    diagnostics.unknown_direction_records.saturating_add(1);
            }
        }
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
        if route.key.fragment == FragmentKind::Call {
            diagnostics.client_call_records = diagnostics.client_call_records.saturating_add(1);
        }
        if route.key.direction != PacketDirection::ClientToServer
            || route.key.fragment != FragmentKind::Call
            || route.key.service_id != WORLD_SERVICE
            || route.key.method_id != USE_SLOT_METHOD
        {
            return;
        }
        diagnostics.world_use_slot_calls = diagnostics.world_use_slot_calls.saturating_add(1);
        let Some(application) = frame.application_bytes.as_deref() else {
            diagnostics.use_slot_calls_without_application = diagnostics
                .use_slot_calls_without_application
                .saturating_add(1);
            return;
        };
        diagnostics.use_slot_calls_with_application = diagnostics
            .use_slot_calls_with_application
            .saturating_add(1);
        if application.len() == 161 {
            diagnostics.use_slot_calls_with_161_byte_application = diagnostics
                .use_slot_calls_with_161_byte_application
                .saturating_add(1);
        }
        let mut scratch = Vec::new();
        let request = match decode_observed_automarker_request_into(pack, application, &mut scratch)
        {
            Ok(request) => {
                diagnostics.exact_marker_requests_decoded =
                    diagnostics.exact_marker_requests_decoded.saturating_add(1);
                request
            }
            Err(_) => {
                diagnostics.exact_marker_schema_rejections =
                    diagnostics.exact_marker_schema_rejections.saturating_add(1);
                return;
            }
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
        let connection_ordinal = connection_ordinals.get(&key).copied().unwrap_or_default();
        let application_uncompressed = !frame.compressed_on_wire;
        let outer_uncompressed = outer.is_some_and(|layout| !layout.compressed);
        let reconstructable = proof.as_ref().is_some_and(|proof| {
            proof.allowed_mutable_bytes == 16 && proof.exact_build_decode_succeeded
        });
        markers.push(MarkerObservation {
            marker_number: request.marker_number,
            surface: surface(connection),
            connection_ordinal,
            connection_epoch_ordinal: epoch.map_or(0, |state| state.ordinal),
            epoch_syn_observed: epoch.is_some_and(|state| state.syn_observed),
            process_ownership_evidence: if !process_ownership_available {
                ProcessOwnershipEvidence::UnprovenMirrorCapture
            } else if epoch.is_some_and(|state| state.process_owned_observed) {
                ProcessOwnershipEvidence::ExactGameProcessSocketObserved
            } else {
                ProcessOwnershipEvidence::NotObservedDuringPoll
            },
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

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
    #[serde(rename_all = "snake_case")]
    enum ProcessOwnershipEvidence {
        ExactGameProcessSocketObserved,
        NotObservedDuringPoll,
        UnprovenMirrorCapture,
    }

    #[derive(Debug, Serialize)]
    struct SurfaceAggregate {
        signature_confirmed_connections: usize,
        exact_game_process_owned_connections_observed: Option<usize>,
        process_ownership_scope: &'static str,
        marker_requests: usize,
    }

    impl Default for SurfaceAggregate {
        fn default() -> Self {
            Self {
                signature_confirmed_connections: 0,
                exact_game_process_owned_connections_observed: None,
                process_ownership_scope: "unproven",
                marker_requests: 0,
            }
        }
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
        signature_filter: SignatureFilterAggregate,
        bpsr_diagnostics: BpsrDiagnosticAggregate,
        filtered_tcp: FilteredTcpAggregate,
        checksum_and_offload: ChecksumAndOffload,
        topology: TopologyAssessment,
        interpretation: Interpretation,
        privacy: Privacy,
    }

    #[derive(Debug, Default, Serialize)]
    struct BpsrDiagnosticAggregate {
        framed_records: u64,
        client_to_server_records: u64,
        server_to_client_records: u64,
        unknown_direction_records: u64,
        client_call_records: u64,
        world_use_slot_calls: u64,
        use_slot_calls_without_application: u64,
        use_slot_calls_with_application: u64,
        use_slot_calls_with_161_byte_application: u64,
        exact_marker_requests_decoded: u64,
        exact_marker_schema_rejections: u64,
    }

    #[derive(Debug, Serialize)]
    struct SignatureFilterAggregate {
        ingress_frames: u64,
        emitted_frames: u64,
        unidentified_frames_discarded: u64,
        pending_limit_evictions: u64,
        signature_matches: u64,
        confirmed_connections: u64,
    }

    impl SignatureFilterAggregate {
        fn from(metrics: &SignatureFlowCaptureMetrics) -> Self {
            Self {
                ingress_frames: metrics.ingress_frames,
                emitted_frames: metrics.emitted_frames,
                unidentified_frames_discarded: metrics.unidentified_frames_discarded,
                pending_limit_evictions: metrics.pending_limit_evictions,
                signature_matches: metrics.signature_matches,
                confirmed_connections: metrics.confirmed_connections,
            }
        }
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

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
    #[serde(rename_all = "snake_case")]
    enum PlaintextTopology {
        NotObserved,
        LoopbackOnly,
        PhysicalOrRoutedOnly,
        LoopbackAndPhysicalOrRouted,
    }

    #[derive(Debug, Serialize)]
    struct TopologyAssessment {
        plaintext_bpsr: PlaintextBpsrGate,
        game_socket: GameSocketGate,
        local_proxy: LocalProxyGate,
        wfp_ordering: WfpOrderingGate,
        live_interception_activation_allowed: bool,
        blocking_reasons: Vec<&'static str>,
    }

    #[derive(Debug, Serialize)]
    struct PlaintextBpsrGate {
        topology: PlaintextTopology,
        exact_marker_requests_observed: usize,
        distinct_marker_connection_epochs: usize,
        exactly_one_candidate_connection_epoch: bool,
        every_request_has_direct_uncompressed_wire_offsets: bool,
    }

    #[derive(Debug, Serialize)]
    struct GameSocketGate {
        socket_table_was_available: bool,
        exact_owned_marker_requests: usize,
        every_marker_request_was_game_owned: bool,
        every_marker_epoch_started_with_observed_syn: bool,
        exact_owned_game_socket_epoch_proven: bool,
    }

    #[derive(Debug, Serialize)]
    struct LocalProxyGate {
        loopback_marker_requests: usize,
        game_owned_loopback_marker_requests: usize,
        loopback_requests_without_observed_game_ownership: usize,
        peer_process_ownership_was_queried: bool,
        exitlag_peer_identity_proven: bool,
        authoritative_plaintext_proxy_leg_proven: bool,
    }

    #[derive(Debug, Serialize)]
    struct WfpOrderingGate {
        passive_npcap_can_observe_callout_ordering: bool,
        interception_layer_was_exercised: bool,
        exitlag_relative_callout_ordering_proven: bool,
    }

    fn assess_topology(
        markers: &[MarkerObservation],
        capture_mode: CaptureMode,
    ) -> TopologyAssessment {
        let loopback = markers
            .iter()
            .filter(|marker| marker.surface == CaptureSurface::Loopback)
            .count();
        let physical = markers.len().saturating_sub(loopback);
        let topology = match (loopback > 0, physical > 0) {
            (false, false) => PlaintextTopology::NotObserved,
            (true, false) => PlaintextTopology::LoopbackOnly,
            (false, true) => PlaintextTopology::PhysicalOrRoutedOnly,
            (true, true) => PlaintextTopology::LoopbackAndPhysicalOrRouted,
        };
        let connection_epochs = markers
            .iter()
            .map(|marker| (marker.connection_ordinal, marker.connection_epoch_ordinal))
            .collect::<BTreeSet<_>>();
        let exactly_one_candidate_connection_epoch = connection_epochs.len() == 1;
        let every_request_has_direct_uncompressed_wire_offsets = !markers.is_empty()
            && markers
                .iter()
                .all(|marker| marker.all_16_approved_bytes_directly_locatable_on_wire);
        let exact_owned = markers
            .iter()
            .filter(|marker| {
                marker.process_ownership_evidence
                    == ProcessOwnershipEvidence::ExactGameProcessSocketObserved
            })
            .count();
        let every_marker_request_was_game_owned =
            !markers.is_empty() && exact_owned == markers.len();
        let every_marker_epoch_started_with_observed_syn =
            !markers.is_empty() && markers.iter().all(|marker| marker.epoch_syn_observed);
        let exact_owned_game_socket_epoch_proven = exactly_one_candidate_connection_epoch
            && every_marker_request_was_game_owned
            && every_marker_epoch_started_with_observed_syn;
        let game_owned_loopback = markers
            .iter()
            .filter(|marker| {
                marker.surface == CaptureSurface::Loopback
                    && marker.process_ownership_evidence
                        == ProcessOwnershipEvidence::ExactGameProcessSocketObserved
            })
            .count();

        let mut blocking_reasons = Vec::new();
        if markers.is_empty() {
            blocking_reasons.push("no_exact_marker_request_observed");
        }
        if !exactly_one_candidate_connection_epoch {
            blocking_reasons.push("candidate_plaintext_connection_epoch_not_unique");
        }
        if !every_request_has_direct_uncompressed_wire_offsets {
            blocking_reasons.push("direct_uncompressed_wire_offsets_not_proven");
        }
        if !exact_owned_game_socket_epoch_proven {
            blocking_reasons.push("exact_game_owned_syn_epoch_not_proven");
        }
        if capture_mode == CaptureMode::ExitLag {
            blocking_reasons.push("exitlag_peer_process_identity_not_observed");
            blocking_reasons.push("exitlag_authoritative_plaintext_leg_not_proven");
        }
        blocking_reasons.push("wfp_relative_callout_ordering_not_measured");
        blocking_reasons.push("live_interception_backend_not_exercised");

        TopologyAssessment {
            plaintext_bpsr: PlaintextBpsrGate {
                topology,
                exact_marker_requests_observed: markers.len(),
                distinct_marker_connection_epochs: connection_epochs.len(),
                exactly_one_candidate_connection_epoch,
                every_request_has_direct_uncompressed_wire_offsets,
            },
            game_socket: GameSocketGate {
                socket_table_was_available: capture_mode != CaptureMode::Mirror,
                exact_owned_marker_requests: exact_owned,
                every_marker_request_was_game_owned,
                every_marker_epoch_started_with_observed_syn,
                exact_owned_game_socket_epoch_proven,
            },
            local_proxy: LocalProxyGate {
                loopback_marker_requests: loopback,
                game_owned_loopback_marker_requests: game_owned_loopback,
                loopback_requests_without_observed_game_ownership: loopback
                    .saturating_sub(game_owned_loopback),
                peer_process_ownership_was_queried: false,
                exitlag_peer_identity_proven: false,
                authoritative_plaintext_proxy_leg_proven: false,
            },
            wfp_ordering: WfpOrderingGate {
                passive_npcap_can_observe_callout_ordering: false,
                interception_layer_was_exercised: false,
                exitlag_relative_callout_ordering_proven: false,
            },
            live_interception_activation_allowed: false,
            blocking_reasons,
        }
    }

    #[derive(Debug, Serialize)]
    struct Interpretation {
        plain_bpsr_marker_visible_on_loopback_capture_surface: bool,
        plain_bpsr_marker_visible_on_physical_or_routed_capture_surface: bool,
        process_ownership_is_exact_four_tuple_not_process_name_inference: bool,
        game_process_ownership_proven: bool,
        mirror_mode_never_infers_game_process_ownership: bool,
        wfp_and_exitlag_callout_ordering: &'static str,
        exitlag_mode_was_operator_selected: bool,
        exitlag_process_or_driver_state_observed: bool,
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
        mirror_mode_available: bool,
        sanitized_recognition_stage_counters: bool,
        exact_application_mutable_byte_count: usize,
        explicit_topology_gates: bool,
        exitlag_process_or_driver_observation_available: bool,
        wfp_order_observation_available: bool,
        sensitive_wire_fields_emitted: bool,
        packet_transmission_available: bool,
        packet_modification_available: bool,
    }

    fn schema_capabilities() -> SchemaCapabilities {
        SchemaCapabilities {
            schema_version: 3,
            passive_capture_only: true,
            process_owned_four_tuple_snapshot: true,
            loopback_and_physical_surface_classification: true,
            mirror_mode_available: true,
            sanitized_recognition_stage_counters: true,
            exact_application_mutable_byte_count: 16,
            explicit_topology_gates: true,
            exitlag_process_or_driver_observation_available: false,
            wfp_order_observation_available: false,
            sensitive_wire_fields_emitted: false,
            packet_transmission_available: false,
            packet_modification_available: false,
        }
    }

    struct Arguments {
        process_id: Option<u32>,
        duration_seconds: u32,
        exitlag: bool,
        mirror: bool,
        interface_name: Option<String>,
        output: PathBuf,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum CaptureMode {
        Standard,
        ExitLag,
        Mirror,
    }

    impl CaptureMode {
        const fn as_str(self) -> &'static str {
            match self {
                Self::Standard => "standard",
                Self::ExitLag => "exitlag",
                Self::Mirror => "mirror",
            }
        }
    }

    impl Arguments {
        fn parse(raw: Vec<OsString>) -> Result<Self, String> {
            let mut process_id = None;
            let mut duration = None;
            let mut output = None;
            let mut interface_name = None;
            let mut exitlag = false;
            let mut mirror = false;
            let mut private = false;
            let mut args = raw.into_iter();
            while let Some(arg) = args.next() {
                if arg == OsStr::new("--private-research") {
                    private = true;
                } else if arg == OsStr::new("--exitlag") {
                    exitlag = true;
                } else if arg == OsStr::new("--mirror") {
                    mirror = true;
                } else if arg == OsStr::new("--process-id") {
                    process_id = unique(process_id, args.next(), "--process-id")?;
                } else if arg == OsStr::new("--interface-name") {
                    interface_name = unique(interface_name, args.next(), "--interface-name")?;
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
            if mirror == process_id.is_some() || (mirror && exitlag) {
                return Err(Self::usage());
            }
            let process_id = process_id
                .map(|value| parse_u32(Some(value), "--process-id", 1, u32::MAX))
                .transpose()?;
            let duration_seconds = parse_u32(duration, "--duration-seconds", 1, 600)?;
            let interface_name = interface_name
                .map(|value| {
                    value
                        .into_string()
                        .map_err(|_| "--interface-name must be valid UTF-8".to_owned())
                })
                .transpose()?;
            Ok(Self {
                process_id,
                duration_seconds,
                exitlag,
                mirror,
                interface_name,
                output: PathBuf::from(output.ok_or_else(Self::usage)?),
            })
        }

        fn capture_mode(&self) -> CaptureMode {
            if self.mirror {
                CaptureMode::Mirror
            } else if self.exitlag {
                CaptureMode::ExitLag
            } else {
                CaptureMode::Standard
            }
        }

        fn usage() -> String {
            "usage: rlogs-bpsr-automarker-windows-boundary --private-research (--process-id <pid> [--exitlag] | --mirror) [--interface-name <local-adapter>] --duration-seconds <1-600> --output <create-only-receipt.json>".into()
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
            assert!(json.contains("\"explicit_topology_gates\":true"));
            assert!(json.contains("\"exitlag_process_or_driver_observation_available\":false"));
            assert!(json.contains("\"wfp_order_observation_available\":false"));
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
            assert_eq!(parsed.process_id, Some(42));
            assert_eq!(parsed.duration_seconds, 25);
            assert!(parsed.exitlag);
            assert!(!parsed.mirror);
        }

        #[test]
        fn mirror_arguments_require_no_process_or_exitlag_claim() {
            let parsed = Arguments::parse(vec![
                "--private-research".into(),
                "--mirror".into(),
                "--interface-name".into(),
                "Ethernet 2".into(),
                "--duration-seconds".into(),
                "30".into(),
                "--output".into(),
                "safe.json".into(),
            ])
            .unwrap();
            assert_eq!(parsed.process_id, None);
            assert_eq!(parsed.capture_mode(), CaptureMode::Mirror);
            assert_eq!(parsed.interface_name.as_deref(), Some("Ethernet 2"));
            for invalid in [
                vec![
                    "--private-research",
                    "--mirror",
                    "--process-id",
                    "42",
                    "--duration-seconds",
                    "30",
                    "--output",
                    "safe.json",
                ],
                vec![
                    "--private-research",
                    "--mirror",
                    "--exitlag",
                    "--duration-seconds",
                    "30",
                    "--output",
                    "safe.json",
                ],
            ] {
                assert!(Arguments::parse(invalid.into_iter().map(Into::into).collect()).is_err());
            }
        }

        #[test]
        fn empty_mirror_receipt_keeps_ownership_and_ordering_unproven() {
            let signature_filter = SignatureFlowCaptureMetrics::default();
            let receipt = BoundaryAnalyzer::new(current_automarker_pack().unwrap(), false)
                .unwrap()
                .finish(CaptureMode::Mirror, &signature_filter);
            let json = serde_json::to_string(&receipt).unwrap();
            assert!(json.contains("\"requested_mode\":\"mirror\""));
            assert!(json.contains("\"process_socket_table_read\":false"));
            assert!(json.contains("\"game_process_ownership_proven\":false"));
            assert!(json.contains("\"wfp_and_exitlag_callout_ordering\":\"unproven\""));
            assert!(json.contains("\"exitlag_mode_was_operator_selected\":false"));
            assert!(json.contains("\"exitlag_process_or_driver_state_observed\":false"));
            assert!(json.contains("\"live_interception_activation_allowed\":false"));
            assert!(json.contains("\"signature_filter\":"));
            assert!(json.contains("\"bpsr_diagnostics\":"));
            assert!(json.contains("\"world_use_slot_calls\":0"));
            assert!(json.contains("\"exact_marker_schema_rejections\":0"));
            assert!(!json.contains("process_id"));
            assert!(!json.contains("interface_name"));
        }

        fn marker(
            surface: CaptureSurface,
            connection_ordinal: u32,
            ownership: ProcessOwnershipEvidence,
        ) -> MarkerObservation {
            MarkerObservation {
                marker_number: 1,
                surface,
                connection_ordinal,
                connection_epoch_ordinal: 1,
                epoch_syn_observed: true,
                process_ownership_evidence: ownership,
                application_length_bytes: 197,
                application_uncompressed_on_wire: true,
                outer_frame_uncompressed_on_wire: true,
                tcp_stream_chunks_spanned: Some(1),
                exactly_one_tcp_stream_chunk: true,
                approved_application_value_bytes: 16,
                all_16_approved_application_bytes_reconstructable: true,
                all_16_approved_bytes_directly_locatable_on_wire: true,
            }
        }

        #[test]
        fn exitlag_selection_never_claims_peer_identity_or_wfp_ordering() {
            let assessment = assess_topology(
                &[marker(
                    CaptureSurface::Loopback,
                    1,
                    ProcessOwnershipEvidence::ExactGameProcessSocketObserved,
                )],
                CaptureMode::ExitLag,
            );
            assert_eq!(
                assessment.plaintext_bpsr.topology,
                PlaintextTopology::LoopbackOnly
            );
            assert!(assessment.game_socket.exact_owned_game_socket_epoch_proven);
            assert!(!assessment.local_proxy.exitlag_peer_identity_proven);
            assert!(
                !assessment
                    .local_proxy
                    .authoritative_plaintext_proxy_leg_proven
            );
            assert!(
                !assessment
                    .wfp_ordering
                    .exitlag_relative_callout_ordering_proven
            );
            assert!(!assessment.live_interception_activation_allowed);
            assert!(
                assessment
                    .blocking_reasons
                    .contains(&"exitlag_peer_process_identity_not_observed")
            );
            assert!(
                assessment
                    .blocking_reasons
                    .contains(&"wfp_relative_callout_ordering_not_measured")
            );
        }

        #[test]
        fn two_observed_plaintext_legs_are_explicitly_ambiguous() {
            let assessment = assess_topology(
                &[
                    marker(
                        CaptureSurface::Loopback,
                        1,
                        ProcessOwnershipEvidence::ExactGameProcessSocketObserved,
                    ),
                    marker(
                        CaptureSurface::PhysicalOrRouted,
                        2,
                        ProcessOwnershipEvidence::ExactGameProcessSocketObserved,
                    ),
                ],
                CaptureMode::Standard,
            );
            assert_eq!(
                assessment.plaintext_bpsr.topology,
                PlaintextTopology::LoopbackAndPhysicalOrRouted
            );
            assert_eq!(
                assessment.plaintext_bpsr.distinct_marker_connection_epochs,
                2
            );
            assert!(
                !assessment
                    .plaintext_bpsr
                    .exactly_one_candidate_connection_epoch
            );
            assert!(
                assessment
                    .blocking_reasons
                    .contains(&"candidate_plaintext_connection_epoch_not_unique")
            );
        }
    }
}

#[cfg(windows)]
fn main() {
    windows::main();
}
