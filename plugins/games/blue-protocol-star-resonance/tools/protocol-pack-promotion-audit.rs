use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    error::Error,
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

use rlogs_game_bpsr::{
    DecodeCoverageSummary, FragmentKind, OfflineRecordingReport, PacketDirection, ProtocolPack,
    ProtocolPackDefinition, ProtocolPackRouteDisposition, RouteKey,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const REPORT_SCHEMA_VERSION: u16 = 6;

const WORLD_SERVICE_NAME: &str = "WorldNtf";
const WORLD_CLIENT_SERVICE_ID: u64 = 103_198_054;
const USE_SLOT_METHOD_ID: u32 = 0x3D002;

#[derive(Debug)]
struct Arguments {
    pack: PathBuf,
    reports: Vec<PathBuf>,
    report_receipts: Vec<PathBuf>,
    observability_contract: PathBuf,
    carrier_route_audit: PathBuf,
    output: PathBuf,
}

#[derive(Debug, Deserialize)]
struct CarrierRouteAuditReceipt {
    schema_version: u16,
    generated_by: String,
    game_build: String,
    protocol_pack_id: String,
    protocol_pack_digest: String,
    policy: CarrierRouteAuditPolicy,
    routes: Vec<CarrierRouteAuditRow>,
}

#[derive(Debug, Deserialize)]
struct CarrierRouteAuditPolicy {
    exact_numeric_route_identity_is_authoritative: bool,
    method_and_localized_names_are_evidence_only: bool,
    successful_decoder_return_is_not_semantic_route_proof: bool,
    discriminating_current_build_wire_witness_required: bool,
    exact_current_build_decoder_contract_required: bool,
    route_identity_proof_does_not_prove_event_coverage: bool,
}

#[derive(Debug, Deserialize)]
struct CarrierRouteAuditRow {
    route: RouteKey,
    semantic_route_identity_proven: bool,
    full_decoder_contract_proven: bool,
    event_carrier_decoder_proven: bool,
}

#[derive(Debug, Deserialize)]
struct ObservabilityContract {
    schema_version: u16,
    game_build: String,
    policy: ObservabilityContractPolicy,
    routes: Vec<ObservabilityRouteRule>,
}

#[derive(Debug, Deserialize)]
struct ObservabilityContractPolicy {
    exact_numeric_route_identity_is_authoritative: bool,
    localized_and_method_names_are_evidence_only: bool,
    packet_absence_is_not_zero: bool,
    structural_non_obligations_never_synthesize_canonical_events: bool,
    unknown_and_unresolved_canonical_events_are_preserved: bool,
}

#[derive(Debug, Deserialize)]
struct ObservabilityRouteRule {
    route: RouteKey,
    classification: String,
    packet_semantics: String,
    canonical_event_policy: String,
    reason: String,
    evidence: Vec<ObservabilityEvidence>,
}

#[derive(Debug, Deserialize)]
struct ObservabilityEvidence {
    path: String,
    fact: String,
}

#[derive(Debug, Clone)]
struct ReportEvidence {
    path: String,
    session_id: String,
    record_count: u64,
    pack_id: String,
    pack_digest: String,
    source_pack_digest: Option<String>,
    gap_count: u64,
    routes: Vec<RouteEvidence>,
}

#[derive(Debug, Deserialize)]
struct GapFreeSegmentReceipt {
    schema_version: u16,
    artifact_kind: String,
    generated_by: String,
    game_build: String,
    policy: GapFreeSegmentPolicy,
    source: FileDescriptor,
    output: FileDescriptor,
    output_capture_id: String,
    protocol_pack_digest: Option<String>,
    source_record_count: u64,
    source_sequence_start: u64,
    source_sequence_end: u64,
    selected_record_count: u64,
    selected_capture_gap_records: u64,
    source_capture_gap_records: u64,
    authority: GapFreeSegmentAuthority,
}

#[derive(Debug, Deserialize)]
struct GapFreeSegmentPolicy {
    source_journal_is_modified: bool,
    source_packet_payload_byte_arrays_are_preserved: bool,
    output_record_wrappers_are_resequenced: bool,
    selected_capture_gap_records_allowed: bool,
    source_capture_gaps_outside_segment_are_disclosed: bool,
    gaps_outside_segment_are_treated_as_zero_bytes_or_events: bool,
    output_proves_encounter_or_lifecycle_conservation: bool,
}

#[derive(Debug, Deserialize)]
struct GapFreeSegmentAuthority {
    gap_free_selected_segment_proven: bool,
    canonical_replay_conservation_proven: bool,
    runtime_promotion_allowed: bool,
    provider_rdps_credit_allowed: bool,
}

#[derive(Debug, Deserialize)]
struct FileDescriptor {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Clone)]
struct RouteEvidence {
    route: RouteKey,
    packet_count: u64,
    application_bytes: u64,
    decode: DecodeCoverageSummary,
}

#[derive(Debug, Default, Clone, Copy)]
struct AggregateRoute {
    report_count: u64,
    packet_count: u64,
    application_bytes: u64,
    decoded_records: u64,
    missing_application_payload_records: u64,
    decode_failed_records: u64,
}

#[derive(Debug, Serialize)]
struct PromotionAudit {
    schema_version: u16,
    build_id: String,
    protocol_pack_id: String,
    protocol_pack_digest: String,
    report_paths: Vec<String>,
    report_receipt_paths: Vec<String>,
    gap_free_segment_receipt_count: usize,
    segmented_report_evidence_does_not_prove_canonical_replay_conservation: bool,
    canonical_replay_conservation_proven_by_this_audit: bool,
    runtime_rdps_promotion_allowed_by_this_audit: bool,
    exact_world_service_id: u64,
    exact_world_call_service_id: u64,
    observed_exact_world_route_count: usize,
    migrated_decoder_route_count: usize,
    validated_migrated_decoder_route_count: usize,
    observable_migrated_decoder_route_count: usize,
    validated_observable_migrated_decoder_route_count: usize,
    structural_non_obligation_route_count: usize,
    observability_contract_path: String,
    carrier_route_audit_path: String,
    semantic_route_audit_route_count: usize,
    use_slot_method_id: u32,
    use_slot_candidate_disposition: String,
    use_slot_runtime_decoder_required: bool,
    use_slot_promotion_requirement_satisfied: bool,
    use_slot_service_ids: Vec<u64>,
    use_slot_routes: Vec<UseSlotRouteEvidence>,
    capture_gap_count: u64,
    route_audits: Vec<MigratedRouteAudit>,
    promotion_ready: bool,
    blockers: Vec<String>,
}

#[derive(Debug, Serialize)]
struct UseSlotRouteEvidence {
    service_id: u64,
    fragment: FragmentKind,
    report_count: u64,
    packet_count: u64,
    application_bytes: u64,
}

#[derive(Debug, Serialize)]
struct MigratedRouteAudit {
    route: RouteKey,
    method_name: String,
    decoder: String,
    report_count: u64,
    packet_count: u64,
    decoded_records: u64,
    missing_application_payload_records: u64,
    decode_failed_records: u64,
    wire_decode_validated: bool,
    semantic_route_audit_required: bool,
    semantic_route_identity_proven: Option<bool>,
    full_decoder_contract_proven: Option<bool>,
    validated: bool,
    coverage_requirement: String,
    promotion_requirement_satisfied: bool,
    structural_non_obligation_reason: Option<String>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("protocol-pack promotion audit failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = arguments()?;
    if args.output.exists() {
        return Err(format!("refusing to overwrite {}", args.output.display()).into());
    }
    let pack = ProtocolPack::from_json(&fs::read(&args.pack)?)?;
    let observability_contract: ObservabilityContract =
        serde_json::from_reader(BufReader::new(File::open(&args.observability_contract)?))?;
    let carrier_route_audit: CarrierRouteAuditReceipt =
        serde_json::from_reader(BufReader::new(File::open(&args.carrier_route_audit)?))?;
    let reports = args
        .reports
        .iter()
        .map(|path| read_report(path))
        .collect::<Result<Vec<_>, _>>()?;
    let report_receipts =
        validate_gap_free_segment_receipts(&pack, &reports, &args.report_receipts)?;
    let audit = audit(
        &pack,
        &reports,
        report_receipts,
        &observability_contract,
        args.observability_contract.to_string_lossy().into_owned(),
        &carrier_route_audit,
        args.carrier_route_audit.to_string_lossy().into_owned(),
    )?;
    if let Some(parent) = args.output.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut output = BufWriter::new(File::create(&args.output)?);
    serde_json::to_writer_pretty(&mut output, &audit)?;
    output.write_all(b"\n")?;
    output.flush()?;

    println!(
        "validated {}/{} observable migrated decoder routes; {} exact structural non-obligations; observed {} exact WorldNtf routes",
        audit.validated_observable_migrated_decoder_route_count,
        audit.observable_migrated_decoder_route_count,
        audit.structural_non_obligation_route_count,
        audit.observed_exact_world_route_count
    );
    if audit.use_slot_service_ids.len() == 1 {
        println!(
            "exact UseSlot namespace candidate: service={} method=0x{:X}",
            audit.use_slot_service_ids[0], audit.use_slot_method_id
        );
    } else {
        println!(
            "UseSlot namespace unresolved: {} distinct service IDs observed",
            audit.use_slot_service_ids.len()
        );
    }
    println!("promotion ready: {}", audit.promotion_ready);
    println!("wrote {}", args.output.display());
    Ok(())
}

fn read_report(path: &Path) -> Result<ReportEvidence, Box<dyn Error>> {
    let report: OfflineRecordingReport =
        serde_json::from_reader(BufReader::new(File::open(path)?))?;
    Ok(ReportEvidence {
        path: path.to_string_lossy().into_owned(),
        session_id: report.session_id,
        record_count: report.record_count,
        pack_id: report.protocol_pack_id,
        pack_digest: report.protocol_pack_digest,
        source_pack_digest: report
            .protocol_pack_transition
            .map(|transition| transition.source_protocol_pack_digest),
        gap_count: report.capture.gap_count,
        routes: report
            .routes
            .into_iter()
            .map(|route| RouteEvidence {
                route: route.route,
                packet_count: route.packet_count,
                application_bytes: route.application_bytes,
                decode: route.decode,
            })
            .collect(),
    })
}

fn validate_gap_free_segment_receipts(
    pack: &ProtocolPack,
    reports: &[ReportEvidence],
    receipt_paths: &[PathBuf],
) -> Result<Vec<String>, Box<dyn Error>> {
    if receipt_paths.is_empty() {
        return Ok(Vec::new());
    }
    if receipt_paths.len() != reports.len() {
        return Err(format!(
            "--report-receipt count {} must equal --report count {}",
            receipt_paths.len(),
            reports.len()
        )
        .into());
    }
    let definition = pack.definition();
    let mut validated = Vec::new();
    for (report, receipt_path) in reports.iter().zip(receipt_paths) {
        let receipt: GapFreeSegmentReceipt =
            serde_json::from_reader(BufReader::new(File::open(receipt_path)?))?;
        let policy = &receipt.policy;
        let authority = &receipt.authority;
        if receipt.schema_version != 2
            || receipt.artifact_kind != "gap-free-journal-segment"
            || receipt.generated_by != "tools/bpsr-protocol-journal-sealed-prefix.mjs"
            || receipt.game_build != definition.target.build_id
            || policy.source_journal_is_modified
            || !policy.source_packet_payload_byte_arrays_are_preserved
            || !policy.output_record_wrappers_are_resequenced
            || policy.selected_capture_gap_records_allowed
            || !policy.source_capture_gaps_outside_segment_are_disclosed
            || policy.gaps_outside_segment_are_treated_as_zero_bytes_or_events
            || policy.output_proves_encounter_or_lifecycle_conservation
            || !authority.gap_free_selected_segment_proven
            || authority.canonical_replay_conservation_proven
            || authority.runtime_promotion_allowed
            || authority.provider_rdps_credit_allowed
        {
            return Err(format!(
                "gap-free segment receipt {} has unsafe policy or authority",
                receipt_path.display()
            )
            .into());
        }
        if receipt.selected_record_count == 0
            || receipt.selected_capture_gap_records != 0
            || receipt.source_capture_gap_records == 0
            || receipt.source_sequence_start == 0
            || receipt.source_sequence_end < receipt.source_sequence_start
            || receipt.selected_record_count
                != receipt.source_sequence_end - receipt.source_sequence_start + 1
            || receipt.source_sequence_end > receipt.source_record_count
            || receipt.output_capture_id != report.session_id
            || receipt.selected_record_count != report.record_count
            || report.gap_count != 0
        {
            return Err(format!(
                "gap-free segment receipt {} does not bind the exact report interval",
                receipt_path.display()
            )
            .into());
        }
        let source_digest = report.source_pack_digest.as_deref().ok_or_else(|| {
            format!(
                "segmented report {} lacks a source protocol-pack transition",
                report.path
            )
        })?;
        if receipt.protocol_pack_digest.as_deref() != Some(source_digest) {
            return Err(format!(
                "gap-free segment receipt {} source pack digest does not match report transition",
                receipt_path.display()
            )
            .into());
        }
        validate_descriptor(&receipt.source)?;
        validate_descriptor(&receipt.output)?;
        validated.push(receipt_path.to_string_lossy().into_owned());
    }
    Ok(validated)
}

fn validate_descriptor(descriptor: &FileDescriptor) -> Result<(), Box<dyn Error>> {
    let path = Path::new(&descriptor.path);
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() || metadata.len() != descriptor.bytes {
        return Err(format!("descriptor size changed for {}", path.display()).into());
    }
    let mut file = File::open(path)?;
    let mut buffer = [0_u8; 64 * 1024];
    let mut hash = Sha256::new();
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    let actual = format!("{:x}", hash.finalize());
    if actual != descriptor.sha256.to_ascii_lowercase() {
        return Err(format!("descriptor hash changed for {}", path.display()).into());
    }
    Ok(())
}

fn audit(
    pack: &ProtocolPack,
    reports: &[ReportEvidence],
    report_receipt_paths: Vec<String>,
    observability_contract: &ObservabilityContract,
    observability_contract_path: String,
    carrier_route_audit: &CarrierRouteAuditReceipt,
    carrier_route_audit_path: String,
) -> Result<PromotionAudit, Box<dyn Error>> {
    if reports.is_empty() {
        return Err("at least one matching-build offline recording report is required".into());
    }
    let definition = pack.definition();
    let semantic_route_audits = validate_carrier_route_audit(pack, carrier_route_audit)?;
    let world_service_ids = definition
        .routes
        .iter()
        .filter(|route| route.service_name == WORLD_SERVICE_NAME)
        .map(|route| route.route.service_id)
        .collect::<BTreeSet<_>>();
    if world_service_ids.len() != 1 {
        return Err(format!(
            "candidate must contain exactly one exact WorldNtf service ID, found {}",
            world_service_ids.len()
        )
        .into());
    }
    let world_service_id = *world_service_ids.iter().next().expect("one service ID");

    let mut aggregate = BTreeMap::<RouteKey, AggregateRoute>::new();
    let mut gap_count = 0_u64;
    for report in reports {
        if report.pack_id != definition.pack_id || report.pack_digest != pack.digest() {
            return Err(format!(
                "report {} was not produced by candidate {} ({})",
                report.path,
                definition.pack_id,
                pack.digest()
            )
            .into());
        }
        gap_count = gap_count.saturating_add(report.gap_count);
        for route in &report.routes {
            let current = aggregate.entry(route.route).or_default();
            current.report_count = current.report_count.saturating_add(1);
            current.packet_count = current.packet_count.saturating_add(route.packet_count);
            current.application_bytes = current
                .application_bytes
                .saturating_add(route.application_bytes);
            current.decoded_records = current
                .decoded_records
                .saturating_add(route.decode.decoded_records);
            current.missing_application_payload_records = current
                .missing_application_payload_records
                .saturating_add(route.decode.missing_application_payload_records);
            current.decode_failed_records = current
                .decode_failed_records
                .saturating_add(route.decode.decode_failed_records);
        }
    }

    let structural_non_obligations =
        validate_observability_contract(definition, observability_contract, &aggregate)?;

    let observed_exact_world_route_count = definition
        .routes
        .iter()
        .filter(|route| {
            route.service_name == WORLD_SERVICE_NAME
                && aggregate
                    .get(&route.route)
                    .is_some_and(|coverage| coverage.packet_count > 0)
        })
        .count();

    let mut route_audits = Vec::new();
    for mapping in definition.routes.iter().filter(|mapping| {
        mapping.service_name == WORLD_SERVICE_NAME
            && matches!(
                mapping.disposition,
                ProtocolPackRouteDisposition::Allowed { .. }
            )
    }) {
        let coverage = aggregate.get(&mapping.route).copied().unwrap_or_default();
        let ProtocolPackRouteDisposition::Allowed { decoder, .. } = mapping.disposition else {
            unreachable!();
        };
        let wire_decode_validated = coverage.packet_count > 0
            && coverage.decoded_records == coverage.packet_count
            && coverage.missing_application_payload_records == 0
            && coverage.decode_failed_records == 0;
        let semantic_route_audit = semantic_route_audits.get(&mapping.route).copied();
        let validated = wire_decode_validated
            && semantic_route_audit.is_none_or(|route| route.event_carrier_decoder_proven);
        let structural_non_obligation = structural_non_obligations.get(&mapping.route);
        let coverage_requirement = if structural_non_obligation.is_some() {
            "structural_non_obligation"
        } else {
            "matching_build_packet_evidence"
        };
        route_audits.push(MigratedRouteAudit {
            route: mapping.route,
            method_name: mapping.method_name.clone(),
            decoder: format!("{decoder:?}"),
            report_count: coverage.report_count,
            packet_count: coverage.packet_count,
            decoded_records: coverage.decoded_records,
            missing_application_payload_records: coverage.missing_application_payload_records,
            decode_failed_records: coverage.decode_failed_records,
            wire_decode_validated,
            semantic_route_audit_required: semantic_route_audit.is_some(),
            semantic_route_identity_proven: semantic_route_audit
                .map(|route| route.semantic_route_identity_proven),
            full_decoder_contract_proven: semantic_route_audit
                .map(|route| route.full_decoder_contract_proven),
            validated,
            coverage_requirement: coverage_requirement.to_owned(),
            promotion_requirement_satisfied: validated || structural_non_obligation.is_some(),
            structural_non_obligation_reason: structural_non_obligation
                .map(|rule| rule.reason.clone()),
        });
    }
    route_audits.sort_by_key(|route| route.route);

    let use_slot_routes = aggregate
        .iter()
        .filter(|(route, coverage)| {
            route.direction == PacketDirection::ClientToServer
                && route.fragment == FragmentKind::Call
                && route.method_id == USE_SLOT_METHOD_ID
                && coverage.packet_count > 0
        })
        .map(|(route, coverage)| UseSlotRouteEvidence {
            service_id: route.service_id,
            fragment: route.fragment,
            report_count: coverage.report_count,
            packet_count: coverage.packet_count,
            application_bytes: coverage.application_bytes,
        })
        .collect::<Vec<_>>();
    let use_slot_service_ids = use_slot_routes
        .iter()
        .filter(|route| route.application_bytes > 0)
        .map(|route| route.service_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    let validated_migrated_decoder_route_count =
        route_audits.iter().filter(|route| route.validated).count();
    let observable_migrated_decoder_route_count = route_audits
        .iter()
        .filter(|route| route.coverage_requirement == "matching_build_packet_evidence")
        .count();
    let validated_observable_migrated_decoder_route_count = route_audits
        .iter()
        .filter(|route| {
            route.coverage_requirement == "matching_build_packet_evidence" && route.validated
        })
        .count();
    let mut blockers = Vec::new();
    if observed_exact_world_route_count == 0 {
        blockers.push("no exact current-build WorldNtf route was observed".to_owned());
    }
    if gap_count > 0 {
        blockers.push(format!(
            "matching-build evidence contains {gap_count} capture gaps"
        ));
    }
    for route in route_audits
        .iter()
        .filter(|route| !route.promotion_requirement_satisfied)
    {
        blockers.push(if route.packet_count == 0 {
            format!(
                "migrated decoder {} ({}) has no matching-build packet evidence",
                route.method_name, route.decoder
            )
        } else if route.wire_decode_validated && route.semantic_route_audit_required {
            format!(
                "migrated decoder {} ({}) parses observed packets but lacks complete exact-build semantic route and decoder-contract proof",
                route.method_name, route.decoder
            )
        } else {
            format!(
                "migrated decoder {} ({}) did not decode every observed packet exactly",
                route.method_name, route.decoder
            )
        });
    }
    let exact_use_slot_route = RouteKey::new(
        PacketDirection::ClientToServer,
        FragmentKind::Call,
        WORLD_CLIENT_SERVICE_ID,
        USE_SLOT_METHOD_ID,
    );
    let exact_use_slot_mapping = definition
        .routes
        .iter()
        .find(|mapping| mapping.route == exact_use_slot_route)
        .ok_or("candidate lacks the statically exact World.UseSlot route")?;
    let (use_slot_candidate_disposition, use_slot_runtime_decoder_required) =
        match exact_use_slot_mapping.disposition {
            ProtocolPackRouteDisposition::Allowed {
                decoder: rlogs_game_bpsr::DecoderKind::WorldUseSlotV1,
                ..
            } => ("allowed:world_use_slot_v1".to_owned(), true),
            ProtocolPackRouteDisposition::Allowed { .. } => {
                return Err("World.UseSlot is assigned to an unexpected decoder".into());
            }
            ProtocolPackRouteDisposition::Opaque => ("opaque".to_owned(), false),
            ProtocolPackRouteDisposition::Prohibited { .. } => {
                return Err("World.UseSlot gameplay telemetry cannot be marked prohibited".into());
            }
        };
    let exact_use_slot_coverage = aggregate
        .get(&exact_use_slot_route)
        .copied()
        .unwrap_or_default();
    let use_slot_promotion_requirement_satisfied = if use_slot_runtime_decoder_required {
        if use_slot_service_ids != [WORLD_CLIENT_SERVICE_ID] {
            blockers.push(format!(
                "UseSlot method 0x{USE_SLOT_METHOD_ID:X} observed service namespaces {:?}, expected exact World service {}",
                use_slot_service_ids, WORLD_CLIENT_SERVICE_ID
            ));
        }
        if exact_use_slot_coverage.packet_count == 0 {
            blockers.push(
                "enabled World.UseSlot decoder has no matching-build packet evidence".to_owned(),
            );
            false
        } else if exact_use_slot_coverage.decoded_records != exact_use_slot_coverage.packet_count
            || exact_use_slot_coverage.missing_application_payload_records > 0
            || exact_use_slot_coverage.decode_failed_records > 0
        {
            blockers.push(
                "enabled World.UseSlot decoder did not decode every observed packet exactly"
                    .to_owned(),
            );
            false
        } else {
            use_slot_service_ids == [WORLD_CLIENT_SERVICE_ID]
        }
    } else {
        // An opaque exact route registers no runtime decoder. Packet absence is
        // retained as unknown evidence and is never treated as zero.
        true
    };

    Ok(PromotionAudit {
        schema_version: REPORT_SCHEMA_VERSION,
        build_id: definition.target.build_id.clone(),
        protocol_pack_id: definition.pack_id.clone(),
        protocol_pack_digest: pack.digest().to_owned(),
        report_paths: reports.iter().map(|report| report.path.clone()).collect(),
        gap_free_segment_receipt_count: report_receipt_paths.len(),
        report_receipt_paths,
        segmented_report_evidence_does_not_prove_canonical_replay_conservation: true,
        canonical_replay_conservation_proven_by_this_audit: false,
        runtime_rdps_promotion_allowed_by_this_audit: false,
        exact_world_service_id: world_service_id,
        exact_world_call_service_id: WORLD_CLIENT_SERVICE_ID,
        observed_exact_world_route_count,
        migrated_decoder_route_count: route_audits.len(),
        validated_migrated_decoder_route_count,
        observable_migrated_decoder_route_count,
        validated_observable_migrated_decoder_route_count,
        structural_non_obligation_route_count: structural_non_obligations.len(),
        observability_contract_path,
        carrier_route_audit_path,
        semantic_route_audit_route_count: semantic_route_audits.len(),
        use_slot_method_id: USE_SLOT_METHOD_ID,
        use_slot_candidate_disposition,
        use_slot_runtime_decoder_required,
        use_slot_promotion_requirement_satisfied,
        use_slot_service_ids,
        use_slot_routes,
        capture_gap_count: gap_count,
        route_audits,
        promotion_ready: blockers.is_empty(),
        blockers,
    })
}

fn validate_carrier_route_audit<'a>(
    pack: &ProtocolPack,
    audit: &'a CarrierRouteAuditReceipt,
) -> Result<BTreeMap<RouteKey, &'a CarrierRouteAuditRow>, Box<dyn Error>> {
    let definition = pack.definition();
    if audit.schema_version != 1
        || audit.generated_by != "rlogs-bpsr-rdps-event-carrier-route-audit"
        || audit.game_build != definition.target.build_id
        || audit.protocol_pack_id != definition.pack_id
        || audit.protocol_pack_digest != pack.digest()
    {
        return Err(
            "carrier route audit schema, generator, pack, or exact build identity is invalid"
                .into(),
        );
    }
    let policy = &audit.policy;
    if !policy.exact_numeric_route_identity_is_authoritative
        || !policy.method_and_localized_names_are_evidence_only
        || !policy.successful_decoder_return_is_not_semantic_route_proof
        || !policy.discriminating_current_build_wire_witness_required
        || !policy.exact_current_build_decoder_contract_required
        || !policy.route_identity_proof_does_not_prove_event_coverage
    {
        return Err("carrier route audit policy is unsafe".into());
    }
    let candidate_routes = definition
        .routes
        .iter()
        .map(|route| route.route)
        .collect::<BTreeSet<_>>();
    let mut routes = BTreeMap::new();
    for row in &audit.routes {
        if !candidate_routes.contains(&row.route) {
            return Err(format!(
                "carrier route audit contains route {:?} absent from the exact candidate",
                row.route
            )
            .into());
        }
        if row.event_carrier_decoder_proven
            != (row.semantic_route_identity_proven && row.full_decoder_contract_proven)
        {
            return Err("carrier route audit contains an inconsistent proof state".into());
        }
        if routes.insert(row.route, row).is_some() {
            return Err("carrier route audit contains a duplicate exact route".into());
        }
    }
    Ok(routes)
}

fn validate_observability_contract<'a>(
    definition: &ProtocolPackDefinition,
    contract: &'a ObservabilityContract,
    aggregate: &BTreeMap<RouteKey, AggregateRoute>,
) -> Result<BTreeMap<RouteKey, &'a ObservabilityRouteRule>, Box<dyn Error>> {
    if contract.schema_version != 1 || contract.game_build != definition.target.build_id {
        return Err("observability contract schema or exact build identity is invalid".into());
    }
    let policy = &contract.policy;
    if !policy.exact_numeric_route_identity_is_authoritative
        || !policy.localized_and_method_names_are_evidence_only
        || !policy.packet_absence_is_not_zero
        || !policy.structural_non_obligations_never_synthesize_canonical_events
        || !policy.unknown_and_unresolved_canonical_events_are_preserved
    {
        return Err("observability contract policy is unsafe".into());
    }
    let candidate_routes = definition
        .routes
        .iter()
        .map(|route| route.route)
        .collect::<BTreeSet<_>>();
    let mut routes = BTreeMap::new();
    for rule in &contract.routes {
        if rule.classification != "structurally_unobservable_from_local_client"
            || rule.packet_semantics != "absence_is_not_zero"
            || rule.canonical_event_policy != "preserve_unknown_do_not_synthesize"
            || rule.reason.trim().is_empty()
            || rule.evidence.is_empty()
            || rule
                .evidence
                .iter()
                .any(|evidence| evidence.path.trim().is_empty() || evidence.fact.trim().is_empty())
        {
            return Err(
                "observability contract contains an unsafe structural non-obligation".into(),
            );
        }
        if !candidate_routes.contains(&rule.route) {
            return Err(format!(
                "observability contract route {:?} is absent from the exact candidate",
                rule.route
            )
            .into());
        }
        if aggregate
            .get(&rule.route)
            .is_some_and(|coverage| coverage.packet_count > 0)
        {
            return Err(format!(
                "observability contract route {:?} was observed; the structural non-obligation is stale",
                rule.route
            )
            .into());
        }
        if routes.insert(rule.route, rule).is_some() {
            return Err("observability contract contains a duplicate exact route".into());
        }
    }
    Ok(routes)
}

fn arguments() -> Result<Arguments, Box<dyn Error>> {
    let mut pack = None;
    let mut reports = Vec::new();
    let mut report_receipts = Vec::new();
    let mut observability_contract = None;
    let mut carrier_route_audit = None;
    let mut output = None;
    let mut args = env::args().skip(1);
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        match flag.as_str() {
            "--pack" => set_once(&mut pack, PathBuf::from(value), "--pack")?,
            "--report" => reports.push(PathBuf::from(value)),
            "--report-receipt" => report_receipts.push(PathBuf::from(value)),
            "--observability-contract" => set_once(
                &mut observability_contract,
                PathBuf::from(value),
                "--observability-contract",
            )?,
            "--carrier-route-audit" => set_once(
                &mut carrier_route_audit,
                PathBuf::from(value),
                "--carrier-route-audit",
            )?,
            "--output" => set_once(&mut output, PathBuf::from(value), "--output")?,
            _ => return Err(format!("unknown argument {flag}").into()),
        }
    }
    if reports.is_empty() {
        return Err("at least one --report is required".into());
    }
    Ok(Arguments {
        pack: pack.ok_or("missing --pack")?,
        reports,
        report_receipts,
        observability_contract: observability_contract.ok_or("missing --observability-contract")?,
        carrier_route_audit: carrier_route_audit.ok_or("missing --carrier-route-audit")?,
        output: output.ok_or("missing --output")?,
    })
}

fn set_once<T>(slot: &mut Option<T>, value: T, flag: &str) -> Result<(), Box<dyn Error>> {
    if slot.replace(value).is_some() {
        return Err(format!("{flag} may only be supplied once").into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rlogs_game_bpsr::{
        AllowedDataDomain, DecoderKind, MappingConfidence, ProtocolPackRoute, ProtocolPackTarget,
    };

    const WORLD: u64 = 1_664_308_034;

    fn pack() -> ProtocolPack {
        pack_with_use_slot(true)
    }

    fn pack_with_use_slot(runtime_decoder_enabled: bool) -> ProtocolPack {
        ProtocolPack::build(ProtocolPackDefinition {
            schema_version: 1,
            pack_id: "current-candidate".to_owned(),
            target: ProtocolPackTarget {
                deployment_id: "global".to_owned(),
                region_id: None,
                channel: "steam".to_owned(),
                build_id: "24609362".to_owned(),
                executable_version: None,
            },
            acquisition: Default::default(),
            provenance: vec![],
            routes: vec![
                ProtocolPackRoute {
                    route: RouteKey::new(
                        PacketDirection::ServerToClient,
                        FragmentKind::Notify,
                        WORLD,
                        68,
                    ),
                    service_name: WORLD_SERVICE_NAME.to_owned(),
                    method_name: "SyncClientUseSkill".to_owned(),
                    message_name: None,
                    confidence: MappingConfidence::Candidate,
                    provenance: vec![],
                    features: vec![],
                    disposition: ProtocolPackRouteDisposition::Allowed {
                        domain: AllowedDataDomain::Combat,
                        decoder: DecoderKind::SyncClientUseSkillV1,
                    },
                },
                ProtocolPackRoute {
                    route: RouteKey::new(
                        PacketDirection::ClientToServer,
                        FragmentKind::Call,
                        WORLD_CLIENT_SERVICE_ID,
                        USE_SLOT_METHOD_ID,
                    ),
                    service_name: "World".to_owned(),
                    method_name: "UseSlot".to_owned(),
                    message_name: Some("Zproto.World.Types.UseSlot".to_owned()),
                    confidence: MappingConfidence::Candidate,
                    provenance: vec![],
                    features: vec![],
                    disposition: if runtime_decoder_enabled {
                        ProtocolPackRouteDisposition::Allowed {
                            domain: AllowedDataDomain::Combat,
                            decoder: DecoderKind::WorldUseSlotV1,
                        }
                    } else {
                        ProtocolPackRouteDisposition::Opaque
                    },
                },
            ],
        })
        .unwrap()
    }

    fn route(route: RouteKey, packet_count: u64, decoded_records: u64) -> RouteEvidence {
        RouteEvidence {
            route,
            packet_count,
            application_bytes: packet_count.saturating_mul(8),
            decode: DecodeCoverageSummary {
                decoded_records,
                ..DecodeCoverageSummary::default()
            },
        }
    }

    fn report(pack: &ProtocolPack, routes: Vec<RouteEvidence>) -> ReportEvidence {
        ReportEvidence {
            path: "fixture.json".to_owned(),
            session_id: "fixture-session".to_owned(),
            record_count: 1,
            pack_id: pack.definition().pack_id.clone(),
            pack_digest: pack.digest().to_owned(),
            source_pack_digest: None,
            gap_count: 0,
            routes,
        }
    }

    fn observability_contract(routes: Vec<ObservabilityRouteRule>) -> ObservabilityContract {
        ObservabilityContract {
            schema_version: 1,
            game_build: "24609362".to_owned(),
            policy: ObservabilityContractPolicy {
                exact_numeric_route_identity_is_authoritative: true,
                localized_and_method_names_are_evidence_only: true,
                packet_absence_is_not_zero: true,
                structural_non_obligations_never_synthesize_canonical_events: true,
                unknown_and_unresolved_canonical_events_are_preserved: true,
            },
            routes,
        }
    }

    fn carrier_route_audit(pack: &ProtocolPack) -> CarrierRouteAuditReceipt {
        CarrierRouteAuditReceipt {
            schema_version: 1,
            generated_by: "rlogs-bpsr-rdps-event-carrier-route-audit".to_owned(),
            game_build: pack.definition().target.build_id.clone(),
            protocol_pack_id: pack.definition().pack_id.clone(),
            protocol_pack_digest: pack.digest().to_owned(),
            policy: CarrierRouteAuditPolicy {
                exact_numeric_route_identity_is_authoritative: true,
                method_and_localized_names_are_evidence_only: true,
                successful_decoder_return_is_not_semantic_route_proof: true,
                discriminating_current_build_wire_witness_required: true,
                exact_current_build_decoder_contract_required: true,
                route_identity_proof_does_not_prove_event_coverage: true,
            },
            routes: vec![],
        }
    }

    #[test]
    fn exact_matching_build_evidence_unlocks_promotion() {
        let pack = pack();
        let report = report(
            &pack,
            vec![
                route(
                    RouteKey::new(
                        PacketDirection::ServerToClient,
                        FragmentKind::Notify,
                        WORLD,
                        68,
                    ),
                    4,
                    4,
                ),
                route(
                    RouteKey::new(
                        PacketDirection::ClientToServer,
                        FragmentKind::Call,
                        WORLD_CLIENT_SERVICE_ID,
                        USE_SLOT_METHOD_ID,
                    ),
                    2,
                    2,
                ),
            ],
        );
        let audit = audit(
            &pack,
            &[report],
            vec![],
            &observability_contract(vec![]),
            "contract.json".to_owned(),
            &carrier_route_audit(&pack),
            "carrier.json".to_owned(),
        )
        .unwrap();
        assert!(audit.promotion_ready);
        assert_eq!(audit.use_slot_service_ids, [WORLD_CLIENT_SERVICE_ID]);
    }

    #[test]
    fn successful_decode_without_full_semantic_contract_stays_blocked() {
        let pack = pack();
        let exact_route = RouteKey::new(
            PacketDirection::ServerToClient,
            FragmentKind::Notify,
            WORLD,
            68,
        );
        let report = report(
            &pack,
            vec![
                route(exact_route, 4, 4),
                route(
                    RouteKey::new(
                        PacketDirection::ClientToServer,
                        FragmentKind::Call,
                        WORLD_CLIENT_SERVICE_ID,
                        USE_SLOT_METHOD_ID,
                    ),
                    2,
                    2,
                ),
            ],
        );
        let mut carrier = carrier_route_audit(&pack);
        carrier.routes.push(CarrierRouteAuditRow {
            route: exact_route,
            semantic_route_identity_proven: true,
            full_decoder_contract_proven: false,
            event_carrier_decoder_proven: false,
        });
        let audit = audit(
            &pack,
            &[report],
            vec![],
            &observability_contract(vec![]),
            "contract.json".to_owned(),
            &carrier,
            "carrier.json".to_owned(),
        )
        .unwrap();
        assert!(!audit.promotion_ready);
        assert!(audit.route_audits[0].wire_decode_validated);
        assert!(!audit.route_audits[0].validated);
        assert!(
            audit
                .blockers
                .iter()
                .any(|blocker| blocker.contains("parses observed packets"))
        );
    }

    #[test]
    fn missing_decoder_and_ambiguous_use_slot_evidence_remain_blockers() {
        let pack = pack();
        let report = report(
            &pack,
            vec![
                route(
                    RouteKey::new(
                        PacketDirection::ClientToServer,
                        FragmentKind::Call,
                        42,
                        USE_SLOT_METHOD_ID,
                    ),
                    1,
                    0,
                ),
                route(
                    RouteKey::new(
                        PacketDirection::ClientToServer,
                        FragmentKind::Call,
                        43,
                        USE_SLOT_METHOD_ID,
                    ),
                    1,
                    0,
                ),
            ],
        );
        let audit = audit(
            &pack,
            &[report],
            vec![],
            &observability_contract(vec![]),
            "contract.json".to_owned(),
            &carrier_route_audit(&pack),
            "carrier.json".to_owned(),
        )
        .unwrap();
        assert!(!audit.promotion_ready);
        assert_eq!(audit.use_slot_service_ids, [42, 43]);
        assert!(
            audit
                .blockers
                .iter()
                .any(|blocker| blocker.contains("no matching-build packet evidence"))
        );
    }

    #[test]
    fn opaque_use_slot_route_does_not_require_packet_acquisition() {
        let pack = pack_with_use_slot(false);
        let report = report(
            &pack,
            vec![route(
                RouteKey::new(
                    PacketDirection::ServerToClient,
                    FragmentKind::Notify,
                    WORLD,
                    68,
                ),
                4,
                4,
            )],
        );
        let audit = audit(
            &pack,
            &[report],
            vec![],
            &observability_contract(vec![]),
            "contract.json".to_owned(),
            &carrier_route_audit(&pack),
            "carrier.json".to_owned(),
        )
        .unwrap();

        assert!(audit.promotion_ready);
        assert_eq!(audit.use_slot_candidate_disposition, "opaque");
        assert!(!audit.use_slot_runtime_decoder_required);
        assert!(audit.use_slot_promotion_requirement_satisfied);
        assert!(audit.use_slot_service_ids.is_empty());
    }

    #[test]
    fn structurally_unobservable_route_is_not_a_packet_acquisition_blocker() {
        let pack = pack();
        let exact_route = RouteKey::new(
            PacketDirection::ServerToClient,
            FragmentKind::Notify,
            WORLD,
            68,
        );
        let report = report(
            &pack,
            vec![route(
                RouteKey::new(
                    PacketDirection::ClientToServer,
                    FragmentKind::Call,
                    WORLD_CLIENT_SERVICE_ID,
                    USE_SLOT_METHOD_ID,
                ),
                2,
                2,
            )],
        );
        let contract = observability_contract(vec![ObservabilityRouteRule {
            route: exact_route,
            classification: "structurally_unobservable_from_local_client".to_owned(),
            packet_semantics: "absence_is_not_zero".to_owned(),
            canonical_event_policy: "preserve_unknown_do_not_synthesize".to_owned(),
            reason: "fixture structural boundary".to_owned(),
            evidence: vec![ObservabilityEvidence {
                path: "fixture.json".to_owned(),
                fact: "no route packets are exposed to the local client".to_owned(),
            }],
        }]);
        let audit = audit(
            &pack,
            &[report],
            vec![],
            &contract,
            "contract.json".to_owned(),
            &carrier_route_audit(&pack),
            "carrier.json".to_owned(),
        )
        .unwrap();
        assert_eq!(audit.structural_non_obligation_route_count, 1);
        assert_eq!(audit.observable_migrated_decoder_route_count, 0);
        assert_eq!(audit.validated_observable_migrated_decoder_route_count, 0);
        assert!(audit.route_audits[0].promotion_requirement_satisfied);
        assert!(!audit.route_audits[0].validated);
        assert!(
            !audit
                .blockers
                .iter()
                .any(|blocker| blocker.contains("no matching-build packet evidence"))
        );
    }

    #[test]
    fn observed_structural_non_obligation_is_rejected_as_stale() {
        let pack = pack();
        let exact_route = RouteKey::new(
            PacketDirection::ServerToClient,
            FragmentKind::Notify,
            WORLD,
            68,
        );
        let report = report(&pack, vec![route(exact_route, 1, 1)]);
        let contract = observability_contract(vec![ObservabilityRouteRule {
            route: exact_route,
            classification: "structurally_unobservable_from_local_client".to_owned(),
            packet_semantics: "absence_is_not_zero".to_owned(),
            canonical_event_policy: "preserve_unknown_do_not_synthesize".to_owned(),
            reason: "fixture structural boundary".to_owned(),
            evidence: vec![ObservabilityEvidence {
                path: "fixture.json".to_owned(),
                fact: "no route packets are exposed to the local client".to_owned(),
            }],
        }]);
        assert!(
            audit(
                &pack,
                &[report],
                vec![],
                &contract,
                "contract.json".to_owned(),
                &carrier_route_audit(&pack),
                "carrier.json".to_owned(),
            )
            .is_err()
        );
    }
}
