use std::ffi::{OsStr, OsString};
use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rlogs_capture::{CaptureSource, OfflineCapture};
use rlogs_core::ResearchConnectionFile;
use rlogs_events::{RegionEvidence, RegionEvidenceKind, RegionIdentity};
use rlogs_game_bpsr::{
    GameBuild, NetworkEndpoint, OfflineRecordingConfig, OfflineRecordingLimits, ProtocolPack,
    ProtocolRuntimeConfig, RegionResolverError, ResolvedRegion, ServerRealmCatalog,
    record_offline_capture,
};

fn main() -> ExitCode {
    match run(std::env::args_os().skip(1)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("rlogs-bpsr-offline-recorder: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: impl IntoIterator<Item = OsString>) -> Result<(), Box<dyn std::error::Error>> {
    let arguments = parse_arguments(arguments)?;
    let report_path = arguments
        .report
        .clone()
        .unwrap_or_else(|| default_report_path(&arguments.output));
    ensure_available(&arguments.output, &report_path)?;

    let pack = ProtocolPack::from_json(&std::fs::read(&arguments.pack)?)?;
    let connection_file: ResearchConnectionFile =
        serde_json::from_slice(&std::fs::read(&arguments.connections)?)?;
    let target = &pack.definition().target;
    let resolved = resolve_server_realm(&arguments.pack, &connection_file)?;
    let mut region = resolved
        .as_ref()
        .map(|resolved| resolved.identity.clone())
        .unwrap_or_else(|| RegionIdentity {
            deployment_id: target.deployment_id.clone(),
            region_id: target
                .region_id
                .clone()
                .unwrap_or_else(|| target.deployment_id.clone()),
            realm_id: None,
            world_id: None,
        });
    if region.deployment_id != target.deployment_id {
        return Err("server realm catalog resolved another deployment".into());
    }
    if let Some(region_id) = &arguments.region_id {
        if region.region_id != "unknown" && &region.region_id != region_id {
            return Err(format!(
                "explicit region {region_id} conflicts with resolved region {}",
                region.region_id
            )
            .into());
        }
        region.region_id.clone_from(region_id);
    }
    let region_id = region.region_id.clone();
    let mut region_evidence = vec![RegionEvidence {
        kind: RegionEvidenceKind::ReplayManifest,
        reference: format!("offline-region:{region_id}"),
    }];
    if let Some(resolved) = resolved {
        region_evidence.extend(resolved.evidence);
    }
    let connections = connection_file.validate()?;
    let build = GameBuild {
        deployment_id: target.deployment_id.clone(),
        region_id: Some(region_id.clone()),
        channel: target.channel.clone(),
        build_id: target.build_id.clone(),
        executable_version: target.executable_version.clone(),
    };
    if !pack.matches(&build) {
        return Err(format!(
            "protocol pack {} does not match region {} and its own exact build target",
            pack.definition().pack_id,
            region_id
        )
        .into());
    }

    let source = OfflineCapture::open(&arguments.input)?;
    let adapter_name = source.metadata().source_id.clone();
    let output_partial = partial_path(&arguments.output)?;
    let report_partial = partial_path(&report_path)?;
    let output_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output_partial)?;
    let config = OfflineRecordingConfig {
        session_id: arguments.session_id.clone(),
        producer: format!("rlogs-bpsr-offline-recorder/{}", env!("CARGO_PKG_VERSION")),
        build,
        region: region.clone(),
        region_evidence,
        limits: OfflineRecordingLimits::default(),
        decoder: ProtocolRuntimeConfig::default(),
    };
    let result = record_offline_capture(
        source,
        connections,
        &pack,
        config,
        BufWriter::new(output_file),
    )?;
    let mut output = result.output;
    output.flush()?;
    output.get_ref().sync_all()?;
    drop(output);

    let report_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&report_partial)?;
    let mut report_writer = BufWriter::new(report_file);
    serde_json::to_writer_pretty(&mut report_writer, &result.report)?;
    report_writer.write_all(b"\n")?;
    report_writer.flush()?;
    report_writer.get_ref().sync_all()?;
    drop(report_writer);

    std::fs::rename(&report_partial, &report_path)?;
    std::fs::rename(&output_partial, &arguments.output)?;

    println!(
        "recorded {} canonical events from {} frames and {} framed records",
        result.report.rlog.event_count, result.report.frame_count, result.report.record_count
    );
    println!(
        "decoder: {} decoded, {} opaque, {} unrouted, {} failed",
        result.report.decoder.decoded_records,
        result.report.decoder.opaque_local_only_records,
        result.report.decoder.unrouted_records,
        result.report.decoder.decode_failed_records
    );
    println!(
        "server detection: {} privacy-safe world-entry endpoint announcement(s)",
        result.report.decoder.announced_server_records
    );
    println!(
        "clock synchronization: {} authoritative server-time record(s)",
        result.report.decoder.server_clock_records
    );
    match region.realm_id.as_deref() {
        Some(realm_id) => println!(
            "resolved server identity: deployment={}, region={}, realm={realm_id}",
            region.deployment_id, region.region_id
        ),
        None => println!(
            "resolved server identity: deployment={}, region={}, realm=unresolved",
            region.deployment_id, region.region_id
        ),
    }
    println!(
        "coverage: {} known routes, {} unknown routes, {} data gaps",
        result.report.capture.known_route_count,
        result.report.capture.unknown_route_count,
        result.report.capture.gap_count
    );
    println!("source adapter: {adapter_name}");
    println!("sealed rlog: {}", arguments.output.display());
    println!("coverage report: {}", report_path.display());
    println!("the source capture and connection evidence remain private research files");
    Ok(())
}

fn resolve_server_realm(
    pack_path: &Path,
    connections: &ResearchConnectionFile,
) -> Result<Option<ResolvedRegion>, Box<dyn std::error::Error>> {
    let Some(build_folder) = pack_path.parent() else {
        return Ok(None);
    };
    let Some(deployment_folder) = build_folder.parent() else {
        return Ok(None);
    };
    let catalog_path = deployment_folder.join("server-realms.json");
    if !catalog_path.is_file() {
        return Ok(None);
    }
    let catalog = ServerRealmCatalog::from_json(&std::fs::read(&catalog_path)?)?;
    let mut resolved: Option<ResolvedRegion> = None;
    for connection in &connections.connections {
        let endpoint = NetworkEndpoint {
            address: connection.server.address.to_string(),
            port: connection.server.port,
        };
        let candidate = match catalog.resolve(&endpoint) {
            Ok(candidate) => candidate,
            Err(RegionResolverError::NoMatch { .. }) => continue,
            Err(error) => return Err(error.into()),
        };
        if let Some(current) = &mut resolved {
            if current.identity != candidate.identity {
                return Err("capture connections resolve to conflicting server realms".into());
            }
            for evidence in candidate.evidence {
                if !current.evidence.contains(&evidence) {
                    current.evidence.push(evidence);
                }
            }
        } else {
            resolved = Some(candidate);
        }
    }
    Ok(resolved)
}

#[derive(Debug, PartialEq, Eq)]
struct Arguments {
    pack: PathBuf,
    connections: PathBuf,
    session_id: String,
    region_id: Option<String>,
    report: Option<PathBuf>,
    input: PathBuf,
    output: PathBuf,
}

fn parse_arguments(arguments: impl IntoIterator<Item = OsString>) -> Result<Arguments, String> {
    let mut private_research = false;
    let mut pack = None;
    let mut connections = None;
    let mut session_id = None;
    let mut region_id = None;
    let mut report = None;
    let mut positional = Vec::new();
    let mut arguments = arguments.into_iter();

    while let Some(argument) = arguments.next() {
        if argument == OsStr::new("--private-research") {
            private_research = true;
        } else if argument == OsStr::new("--pack") {
            pack = unique_value(pack, arguments.next(), "--pack")?;
        } else if argument == OsStr::new("--connections") {
            connections = unique_value(connections, arguments.next(), "--connections")?;
        } else if argument == OsStr::new("--session-id") {
            session_id = unique_value(session_id, arguments.next(), "--session-id")?;
        } else if argument == OsStr::new("--region-id") {
            region_id = unique_value(region_id, arguments.next(), "--region-id")?;
        } else if argument == OsStr::new("--report") {
            report = unique_value(report, arguments.next(), "--report")?;
        } else if argument.to_string_lossy().starts_with('-') {
            return Err(usage());
        } else {
            positional.push(PathBuf::from(argument));
        }
    }
    if !private_research || positional.len() != 2 {
        return Err(usage());
    }
    let session_id = required_identifier(session_id, "--session-id")?;
    let region_id = region_id
        .map(|value| validate_identifier(value, "--region-id"))
        .transpose()?;
    let output = positional.remove(1);
    if output.extension().and_then(OsStr::to_str) != Some("rlog") {
        return Err("output must use the .rlog extension".into());
    }

    Ok(Arguments {
        pack: pack.map(PathBuf::from).ok_or_else(usage)?,
        connections: connections.map(PathBuf::from).ok_or_else(usage)?,
        session_id,
        region_id,
        report: report.map(PathBuf::from),
        input: positional.remove(0),
        output,
    })
}

fn unique_value(
    current: Option<OsString>,
    next: Option<OsString>,
    flag: &str,
) -> Result<Option<OsString>, String> {
    if current.is_some() {
        return Err(format!("{flag} may be supplied only once"));
    }
    next.map(Some)
        .ok_or_else(|| format!("{flag} requires a value"))
}

fn required_identifier(value: Option<OsString>, flag: &str) -> Result<String, String> {
    validate_identifier(value.ok_or_else(usage)?, flag)
}

fn validate_identifier(value: OsString, flag: &str) -> Result<String, String> {
    let value = value
        .into_string()
        .map_err(|_| format!("{flag} must be valid UTF-8"))?;
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(format!(
            "{flag} must use 1-128 ASCII letters, digits, '.', '_', or '-'"
        ));
    }
    Ok(value)
}

fn ensure_available(output: &Path, report: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if output == report {
        return Err("rlog output and coverage report must use different paths".into());
    }
    for path in [
        output,
        report,
        &partial_path(output)?,
        &partial_path(report)?,
    ] {
        if path.exists() {
            return Err(format!("refusing to overwrite existing file {}", path.display()).into());
        }
    }
    Ok(())
}

fn default_report_path(output: &Path) -> PathBuf {
    let stem = output
        .file_stem()
        .and_then(OsStr::to_str)
        .unwrap_or("recording");
    output.with_file_name(format!("{stem}.coverage.json"))
}

fn partial_path(output: &Path) -> Result<PathBuf, String> {
    let name = output
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or("output must have a valid UTF-8 filename")?;
    Ok(output.with_file_name(format!(".{name}.partial")))
}

fn usage() -> String {
    "usage: rlogs-bpsr-offline-recorder --private-research --pack <pack.json> --connections <connections.json> --session-id <id> [--region-id <region>] [--report <coverage.json>] <capture.pcap|capture.pcapng> <output.rlog>".into()
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Cursor};
    use std::net::{IpAddr, Ipv4Addr};

    use bytes::Bytes;
    use etherparse::{PacketBuilder, TcpHeader};
    use rlogs_capture::{CaptureLinkType, CapturedFrame, PcapWriter, TimestampNormalization};
    use rlogs_core::{GameConnection, GameConnectionFilter};
    use rlogs_events::{CanonicalEvent, EventTopic};
    use rlogs_game_bpsr::FragmentKind;
    use rlogs_log_format::{RlogLimits, RlogReader};
    use rlogs_network::IpEndpoint;

    use super::*;

    fn os(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    fn endpoint(last: u8, port: u16) -> IpEndpoint {
        IpEndpoint::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, last)), port)
    }

    #[test]
    fn user_confirmed_world_load_connection_resolves_to_asteria() {
        let pack_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../protocol-packs/global/steam-24252055/pack.json");
        let connections = ResearchConnectionFile {
            schema_version: 1,
            connections: vec![GameConnection {
                client: endpoint(1, 57_576),
                server: IpEndpoint::new("43.174.232.118".parse().unwrap(), 10_099),
            }],
        };

        let resolved = resolve_server_realm(&pack_path, &connections)
            .unwrap()
            .expect("exact Asteria endpoint");
        assert_eq!(resolved.identity.deployment_id, "global");
        assert_eq!(resolved.identity.region_id, "unknown");
        assert_eq!(resolved.identity.realm_id.as_deref(), Some("asteria"));

        let wrong_port = ResearchConnectionFile {
            schema_version: 1,
            connections: vec![GameConnection {
                client: endpoint(1, 57_575),
                server: IpEndpoint::new("43.174.232.118".parse().unwrap(), 5_003),
            }],
        };
        assert!(
            resolve_server_realm(&pack_path, &wrong_port)
                .unwrap()
                .is_none()
        );
    }

    fn fixture_pcap(client: IpEndpoint, server: IpEndpoint) -> Vec<u8> {
        let protobuf = [0x0a, 0x00];
        let length = 6 + 16 + protobuf.len();
        let mut bpsr = Vec::with_capacity(length);
        bpsr.extend_from_slice(&(length as u32).to_be_bytes());
        bpsr.extend_from_slice(&FragmentKind::Notify.wire_id().to_be_bytes());
        bpsr.extend_from_slice(&1_664_308_034_u64.to_be_bytes());
        bpsr.extend_from_slice(&1_u32.to_be_bytes());
        bpsr.extend_from_slice(&3_u32.to_be_bytes());
        bpsr.extend_from_slice(&protobuf);

        let tcp = TcpHeader::new(server.port, client.port, 100, 16_384);
        let IpAddr::V4(server_address) = server.address else {
            unreachable!();
        };
        let IpAddr::V4(client_address) = client.address else {
            unreachable!();
        };
        let builder = PacketBuilder::ipv4(server_address.octets(), client_address.octets(), 64)
            .tcp_header(tcp);
        let mut packet = Vec::with_capacity(builder.size(bpsr.len()));
        builder.write(&mut packet, &bpsr).unwrap();
        let frame = CapturedFrame {
            sequence: 1,
            observed_micros: 0,
            source_timestamp_nanos: Some(1_000_000),
            timestamp_normalization: TimestampNormalization::Exact,
            interface_id: None,
            link_type: CaptureLinkType::RawIpv4,
            original_length: packet.len() as u32,
            bytes: Bytes::from(packet),
        };
        let mut writer = PcapWriter::new(Vec::new(), CaptureLinkType::RawIpv4).unwrap();
        writer.write_frame(&frame).unwrap();
        writer.finish_for_test()
    }

    trait FinishPcapForTest {
        fn finish_for_test(self) -> Vec<u8>;
    }

    impl FinishPcapForTest for PcapWriter<Vec<u8>> {
        fn finish_for_test(mut self) -> Vec<u8> {
            self.flush().unwrap();
            self.into_inner()
        }
    }

    #[test]
    fn arguments_require_private_exact_inputs() {
        let parsed = parse_arguments(os(&[
            "--private-research",
            "--pack",
            "pack.json",
            "--connections",
            "connections.json",
            "--session-id",
            "fixture-1",
            "fixture.pcap",
            "fixture.rlog",
        ]))
        .unwrap();
        assert_eq!(parsed.session_id, "fixture-1");
        assert_eq!(parsed.region_id, None);
        assert_eq!(
            default_report_path(&parsed.output),
            PathBuf::from("fixture.coverage.json")
        );
        assert!(
            parse_arguments(os(&[
                "--pack",
                "pack.json",
                "--connections",
                "connections.json",
                "--session-id",
                "fixture-1",
                "fixture.pcap",
                "fixture.rlog",
            ]))
            .is_err()
        );
    }

    #[test]
    fn pcap_reconstructs_decodes_and_seals_without_raw_payloads() {
        let client = endpoint(1, 31_000);
        let server = endpoint(2, 32_000);
        let pcap = fixture_pcap(client, server);
        let source =
            OfflineCapture::from_reader("fixture-pcap", "Fixture pcap", Cursor::new(pcap)).unwrap();
        let connections =
            GameConnectionFilter::try_new(vec![GameConnection { client, server }]).unwrap();
        let pack = ProtocolPack::from_json(include_bytes!(
            "../../../protocol-packs/global/reference-v1/pack.json"
        ))
        .unwrap();
        let target = &pack.definition().target;
        let result = record_offline_capture(
            source,
            connections,
            &pack,
            OfflineRecordingConfig {
                session_id: "fixture-session".into(),
                producer: "unit-test".into(),
                build: GameBuild {
                    deployment_id: target.deployment_id.clone(),
                    region_id: Some("north-america".into()),
                    channel: target.channel.clone(),
                    build_id: target.build_id.clone(),
                    executable_version: target.executable_version.clone(),
                },
                region: RegionIdentity {
                    deployment_id: "global".into(),
                    region_id: "north-america".into(),
                    realm_id: None,
                    world_id: None,
                },
                region_evidence: vec![RegionEvidence {
                    kind: RegionEvidenceKind::ReplayManifest,
                    reference: "unit-test".into(),
                }],
                limits: OfflineRecordingLimits::default(),
                decoder: ProtocolRuntimeConfig::default(),
            },
            Vec::new(),
        )
        .unwrap();

        assert_eq!(result.report.frame_count, 1);
        assert_eq!(result.report.record_count, 1);
        assert_eq!(result.report.capture.known_route_count, 1);
        assert_eq!(result.report.decoder.decoded_records, 1);
        assert_eq!(result.report.rlog.event_count, 2);
        assert_eq!(
            result.report.event_topics,
            vec![
                rlogs_game_bpsr::EventTopicCoverage {
                    topic: EventTopic::Encounter,
                    event_count: 1,
                },
                rlogs_game_bpsr::EventTopicCoverage {
                    topic: EventTopic::World,
                    event_count: 1,
                },
            ]
        );
        let json = serde_json::to_string(&result.report).unwrap();
        assert!(!json.contains("\"payload\""));
        assert!(!json.contains("\"source\":{\"address\""));
        assert!(!json.contains("\"destination\":{\"address\""));
        assert!(!json.contains("10.0.0."));

        let mut topics = Vec::new();
        let summary = RlogReader::new(
            BufReader::new(Cursor::new(result.output)),
            RlogLimits::default(),
        )
        .unwrap()
        .replay(|event| {
            topics.push(event.event.topic());
            assert!(matches!(
                event.event,
                CanonicalEvent::WorldChanged(_) | CanonicalEvent::Timeline(_)
            ));
            Ok(())
        })
        .unwrap();
        assert_eq!(summary.event_count, 2);
        assert_eq!(topics, vec![EventTopic::World, EventTopic::Encounter]);
    }
}
