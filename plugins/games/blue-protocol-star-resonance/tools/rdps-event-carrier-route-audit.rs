use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    error::Error,
    fs::{self, File},
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use rlogs_game_bpsr::{
    CaptureRecordKind, FragmentKind, JsonlJournalReader, PacketDirection, ProtocolPack,
    ProtocolPackRouteDisposition, RouteKey,
};
use serde::{Deserialize, Serialize};

const REPORT_SCHEMA_VERSION: u16 = 1;
const CONTRACT_SCHEMA_VERSION: u16 = 3;
const WORLD_SERVICE_NAME: &str = "WorldNtf";

#[derive(Debug)]
struct Arguments {
    pack: PathBuf,
    decoder_contract: PathBuf,
    native_route_proof: PathBuf,
    journals: Vec<PathBuf>,
    output: PathBuf,
}

#[derive(Debug, Deserialize)]
struct DecoderContractAudit {
    schema_version: u16,
    build_id: String,
    policy: DecoderContractPolicy,
    messages: Vec<DecoderMessageContract>,
}

#[derive(Debug, Deserialize)]
struct DecoderContractPolicy {
    generated_field_order_treated_as_protobuf_tag: bool,
    exact_native_wire_tag_required: bool,
    exact_build_packet_replay_required: bool,
}

#[derive(Debug, Deserialize)]
struct DecoderMessageContract {
    decoder_name: String,
    generated_full_name: Option<String>,
    fields: Vec<DecoderFieldContract>,
}

#[derive(Debug, Deserialize)]
struct DecoderFieldContract {
    decoder_tag: u32,
    prost_shape: String,
    generated_protobuf_tag: Option<u32>,
    tag_state: String,
}

#[derive(Debug, Deserialize)]
struct NativeRouteProof {
    game_build: String,
    server_dispatcher: NativeServerDispatcher,
}

#[derive(Debug, Deserialize)]
struct NativeServerDispatcher {
    routes: Vec<NativeRoute>,
}

#[derive(Debug, Deserialize)]
struct NativeRoute {
    name: String,
    method_id_decimal: u32,
    proof_state: String,
}

#[derive(Debug, Clone, Copy)]
struct CarrierSpec {
    method_id: u32,
    decoder: &'static str,
    wrapper: &'static str,
    child_by_wrapper_tag: &'static [(u32, &'static str)],
    discriminator: Discriminator,
}

#[derive(Debug, Clone, Copy)]
enum Discriminator {
    ChildField {
        wrapper_tag: u32,
        child_tag: u32,
        wire_type: u8,
    },
}

const ENTITY_CHILDREN: &[(u32, &str)] = &[(1, "Entity"), (2, "DisappearEntity")];
const NEAR_DELTA_CHILDREN: &[(u32, &str)] = &[(1, "AoiSyncDelta")];
const TO_ME_CHILDREN: &[(u32, &str)] = &[(1, "AoiSyncToMeDelta")];
const CARRIER_SPECS: &[CarrierSpec] = &[
    CarrierSpec {
        method_id: 6,
        decoder: "SyncNearEntitiesV1",
        wrapper: "SyncNearEntities",
        child_by_wrapper_tag: ENTITY_CHILDREN,
        discriminator: Discriminator::ChildField {
            wrapper_tag: 1,
            child_tag: 2,
            wire_type: 0,
        },
    },
    CarrierSpec {
        method_id: 45,
        decoder: "SyncNearDeltaV1",
        wrapper: "SyncNearDeltaInfo",
        child_by_wrapper_tag: NEAR_DELTA_CHILDREN,
        discriminator: Discriminator::ChildField {
            wrapper_tag: 1,
            child_tag: 2,
            wire_type: 2,
        },
    },
    CarrierSpec {
        method_id: 46,
        decoder: "SyncToMeDeltaV1",
        wrapper: "SyncToMeDeltaInfo",
        child_by_wrapper_tag: TO_ME_CHILDREN,
        discriminator: Discriminator::ChildField {
            wrapper_tag: 1,
            child_tag: 1,
            wire_type: 2,
        },
    },
];

#[derive(Debug, Default, Clone, Copy)]
struct RouteObservation {
    journal_count: u64,
    packet_count: u64,
    application_bytes: u64,
    missing_application_payloads: u64,
    malformed_payloads: u64,
    wire_contract_mismatches: u64,
    discriminating_witnesses: u64,
    unknown_field_observations: u64,
}

#[derive(Debug, Serialize)]
struct AuditReport {
    schema_version: u16,
    generated_by: &'static str,
    game_build: String,
    protocol_pack_id: String,
    protocol_pack_digest: String,
    decoder_contract_path: String,
    native_route_proof_path: String,
    journal_paths: Vec<String>,
    policy: AuditPolicy,
    summary: AuditSummary,
    routes: Vec<RouteAudit>,
    blockers: Vec<String>,
}

#[derive(Debug, Serialize)]
struct AuditPolicy {
    exact_numeric_route_identity_is_authoritative: bool,
    method_and_localized_names_are_evidence_only: bool,
    successful_decoder_return_is_not_semantic_route_proof: bool,
    discriminating_current_build_wire_witness_required: bool,
    exact_current_build_decoder_contract_required: bool,
    unknown_wire_fields_are_counted_not_discarded: bool,
    capture_gaps_are_not_reclassified_by_this_audit: bool,
    route_identity_proof_does_not_prove_event_coverage: bool,
}

#[derive(Debug, Serialize)]
struct AuditSummary {
    journals: usize,
    capture_gaps: u64,
    malformed_or_truncated_journals: u64,
    carrier_routes: usize,
    semantically_proven_routes: usize,
    full_decoder_contract_proven_routes: usize,
    native_dispatch_name_conflicts: usize,
    event_coverage_proven: bool,
}

#[derive(Debug, Serialize)]
struct RouteAudit {
    route: RouteKey,
    candidate_method_name: String,
    candidate_decoder: String,
    expected_wrapper_message: String,
    native_dispatch_name_at_numeric_id: Option<String>,
    native_dispatch_name_matches_candidate: bool,
    native_dispatch_name_is_runtime_authority: bool,
    journal_count: u64,
    packet_count: u64,
    application_bytes: u64,
    missing_application_payloads: u64,
    malformed_payloads: u64,
    wire_contract_mismatches: u64,
    discriminating_witnesses: u64,
    unknown_field_observations: u64,
    exact_route_identity_contract_proven: bool,
    full_decoder_contract_proven: bool,
    semantic_route_identity_proven: bool,
    event_carrier_decoder_proven: bool,
    event_coverage_proven: bool,
    open_obligations: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
struct WireField<'a> {
    tag: u32,
    wire_type: u8,
    bytes: &'a [u8],
}

fn main() {
    if let Err(error) = run() {
        eprintln!("rDPS event carrier route audit failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = arguments()?;
    if args.output.exists() {
        return Err(format!("refusing to overwrite {}", args.output.display()).into());
    }
    let pack = ProtocolPack::from_json(&fs::read(&args.pack)?)?;
    let contract: DecoderContractAudit = read_json(&args.decoder_contract)?;
    let native: NativeRouteProof = read_json(&args.native_route_proof)?;
    let report = audit(&pack, &contract, &native, &args)?;
    if let Some(parent) = args.output.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut output = BufWriter::new(File::create(&args.output)?);
    serde_json::to_writer_pretty(&mut output, &report)?;
    output.write_all(b"\n")?;
    output.flush()?;
    println!(
        "proved {}/{} exact-build rDPS carrier routes; capture gaps remain {}",
        report.summary.semantically_proven_routes,
        report.summary.carrier_routes,
        report.summary.capture_gaps
    );
    println!("wrote {}", args.output.display());
    Ok(())
}

fn audit(
    pack: &ProtocolPack,
    contract: &DecoderContractAudit,
    native: &NativeRouteProof,
    args: &Arguments,
) -> Result<AuditReport, Box<dyn Error>> {
    let definition = pack.definition();
    validate_contract_header(contract, &definition.target.build_id)?;
    if native.game_build != definition.target.build_id {
        return Err("native route proof and candidate build identities disagree".into());
    }
    let contracts = contract
        .messages
        .iter()
        .map(|message| (message.decoder_name.as_str(), message))
        .collect::<BTreeMap<_, _>>();
    let native_routes = native
        .server_dispatcher
        .routes
        .iter()
        .map(|route| {
            if route.proof_state != "exact_native_dispatch_and_literal_plaintext" {
                return Err(format!("native route {} is not exact evidence", route.name));
            }
            Ok((route.method_id_decimal, route.name.as_str()))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    let world_service_ids = definition
        .routes
        .iter()
        .filter(|route| route.service_name == WORLD_SERVICE_NAME)
        .map(|route| route.route.service_id)
        .collect::<BTreeSet<_>>();
    if world_service_ids.len() != 1 {
        return Err("candidate must contain exactly one WorldNtf service ID".into());
    }
    let world_service_id = *world_service_ids.iter().next().expect("one service ID");

    let mut observations = BTreeMap::<u32, RouteObservation>::new();
    let mut capture_gaps = 0_u64;
    let mut malformed_or_truncated_journals = 0_u64;
    for journal_path in &args.journals {
        let file = File::open(journal_path)?;
        let mut stream = JsonlJournalReader::new(BufReader::new(file)).into_record_stream()?;
        if stream.session().game_build.build_id != definition.target.build_id {
            return Err(format!(
                "journal {} is build {}, expected {}",
                journal_path.display(),
                stream.session().game_build.build_id,
                definition.target.build_id
            )
            .into());
        }
        let mut seen = BTreeSet::new();
        loop {
            match stream.next_record() {
                Ok(Some(record)) => match record.kind {
                    CaptureRecordKind::Gap(_) => capture_gaps = capture_gaps.saturating_add(1),
                    CaptureRecordKind::Packet(packet) => {
                        let Some(route) = packet.route.map(|route| route.key) else {
                            continue;
                        };
                        let Some(spec) = CARRIER_SPECS.iter().find(|spec| {
                            route.direction == PacketDirection::ServerToClient
                                && route.fragment == FragmentKind::Notify
                                && route.service_id == world_service_id
                                && route.method_id == spec.method_id
                        }) else {
                            continue;
                        };
                        let current = observations.entry(spec.method_id).or_default();
                        current.packet_count = current.packet_count.saturating_add(1);
                        seen.insert(spec.method_id);
                        let Some(payload) = packet.payload.application_bytes else {
                            current.missing_application_payloads =
                                current.missing_application_payloads.saturating_add(1);
                            continue;
                        };
                        current.application_bytes = current
                            .application_bytes
                            .saturating_add(payload.len() as u64);
                        match inspect_payload(spec, &payload, &contracts) {
                            Ok(inspection) => {
                                current.wire_contract_mismatches = current
                                    .wire_contract_mismatches
                                    .saturating_add(inspection.contract_mismatches);
                                current.discriminating_witnesses = current
                                    .discriminating_witnesses
                                    .saturating_add(inspection.discriminating_witnesses);
                                current.unknown_field_observations = current
                                    .unknown_field_observations
                                    .saturating_add(inspection.unknown_fields);
                            }
                            Err(_) => {
                                current.malformed_payloads =
                                    current.malformed_payloads.saturating_add(1);
                            }
                        }
                    }
                },
                Ok(None) => break,
                Err(error) if stream.truncated_tail().is_some() => {
                    let _ = error;
                    malformed_or_truncated_journals =
                        malformed_or_truncated_journals.saturating_add(1);
                    break;
                }
                Err(error) => return Err(error.into()),
            }
        }
        for method_id in seen {
            observations.entry(method_id).or_default().journal_count = observations
                .get(&method_id)
                .copied()
                .unwrap_or_default()
                .journal_count
                .saturating_add(1);
        }
    }

    let mut routes = Vec::new();
    let mut native_conflicts = 0_usize;
    for spec in CARRIER_SPECS {
        let key = RouteKey::new(
            PacketDirection::ServerToClient,
            FragmentKind::Notify,
            world_service_id,
            spec.method_id,
        );
        let mapping = definition
            .routes
            .iter()
            .find(|route| route.route == key)
            .ok_or_else(|| format!("candidate lacks carrier route {key:?}"))?;
        let ProtocolPackRouteDisposition::Allowed { decoder, .. } = mapping.disposition else {
            return Err(format!("carrier route {key:?} is not allowed by the candidate").into());
        };
        if format!("{decoder:?}") != spec.decoder {
            return Err(
                format!("carrier route {key:?} uses unexpected decoder {decoder:?}").into(),
            );
        }
        let identity_contract = route_identity_contract_proven(spec, &contracts);
        let full_decoder_contract = full_decoder_contract_proven(spec, &contracts);
        let observation = observations
            .get(&spec.method_id)
            .copied()
            .unwrap_or_default();
        let native_name = native_routes.get(&spec.method_id).copied();
        let native_matches = native_name == Some(mapping.method_name.as_str());
        if native_name.is_some() && !native_matches {
            native_conflicts += 1;
        }
        let mut route_blockers = Vec::new();
        if !identity_contract {
            route_blockers.push(
                "the exact current-build wrapper and discriminating child contracts are incomplete"
                    .to_owned(),
            );
        }
        if observation.packet_count == 0 {
            route_blockers
                .push("no exact-build packet was observed at this numeric route".to_owned());
        }
        if observation.missing_application_payloads > 0 {
            route_blockers.push(format!(
                "{} packets lacked application payloads",
                observation.missing_application_payloads
            ));
        }
        if observation.malformed_payloads > 0 {
            route_blockers.push(format!(
                "{} packet payloads were malformed",
                observation.malformed_payloads
            ));
        }
        if observation.wire_contract_mismatches > 0 {
            route_blockers.push(format!(
                "{} observed fields contradicted the exact wire contract",
                observation.wire_contract_mismatches
            ));
        }
        if observation.discriminating_witnesses == 0 {
            route_blockers.push(
                "no packet contained a wire-type witness that discriminates this message family"
                    .to_owned(),
            );
        }
        let semantic_route_identity_proven = route_blockers.is_empty();
        let event_carrier_decoder_proven = semantic_route_identity_proven && full_decoder_contract;
        if !full_decoder_contract {
            route_blockers.push(
                "at least one decoded child message lacks an exact current-build wire contract"
                    .to_owned(),
            );
        }
        routes.push(RouteAudit {
            route: key,
            candidate_method_name: mapping.method_name.clone(),
            candidate_decoder: spec.decoder.to_owned(),
            expected_wrapper_message: spec.wrapper.to_owned(),
            native_dispatch_name_at_numeric_id: native_name.map(str::to_owned),
            native_dispatch_name_matches_candidate: native_matches,
            native_dispatch_name_is_runtime_authority: false,
            journal_count: observation.journal_count,
            packet_count: observation.packet_count,
            application_bytes: observation.application_bytes,
            missing_application_payloads: observation.missing_application_payloads,
            malformed_payloads: observation.malformed_payloads,
            wire_contract_mismatches: observation.wire_contract_mismatches,
            discriminating_witnesses: observation.discriminating_witnesses,
            unknown_field_observations: observation.unknown_field_observations,
            exact_route_identity_contract_proven: identity_contract,
            full_decoder_contract_proven: full_decoder_contract,
            semantic_route_identity_proven,
            event_carrier_decoder_proven,
            event_coverage_proven: false,
            open_obligations: route_blockers,
        });
    }
    let semantically_proven_routes = routes
        .iter()
        .filter(|route| route.semantic_route_identity_proven)
        .count();
    let full_decoder_contract_proven_routes = routes
        .iter()
        .filter(|route| route.event_carrier_decoder_proven)
        .count();
    let mut blockers = Vec::new();
    if semantically_proven_routes != CARRIER_SPECS.len() {
        blockers.push(format!(
            "only {semantically_proven_routes}/{} rDPS carrier route identities are semantically proven",
            CARRIER_SPECS.len()
        ));
    }
    blockers.push(
        "event coverage remains open: this specimen audit neither excludes capture gaps nor proves every required event carrier was observed"
            .to_owned(),
    );
    Ok(AuditReport {
        schema_version: REPORT_SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-rdps-event-carrier-route-audit",
        game_build: definition.target.build_id.clone(),
        protocol_pack_id: definition.pack_id.clone(),
        protocol_pack_digest: pack.digest().to_owned(),
        decoder_contract_path: args.decoder_contract.to_string_lossy().into_owned(),
        native_route_proof_path: args.native_route_proof.to_string_lossy().into_owned(),
        journal_paths: args
            .journals
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect(),
        policy: AuditPolicy {
            exact_numeric_route_identity_is_authoritative: true,
            method_and_localized_names_are_evidence_only: true,
            successful_decoder_return_is_not_semantic_route_proof: true,
            discriminating_current_build_wire_witness_required: true,
            exact_current_build_decoder_contract_required: true,
            unknown_wire_fields_are_counted_not_discarded: true,
            capture_gaps_are_not_reclassified_by_this_audit: true,
            route_identity_proof_does_not_prove_event_coverage: true,
        },
        summary: AuditSummary {
            journals: args.journals.len(),
            capture_gaps,
            malformed_or_truncated_journals,
            carrier_routes: CARRIER_SPECS.len(),
            semantically_proven_routes,
            full_decoder_contract_proven_routes,
            native_dispatch_name_conflicts: native_conflicts,
            event_coverage_proven: false,
        },
        routes,
        blockers,
    })
}

fn validate_contract_header(
    contract: &DecoderContractAudit,
    build_id: &str,
) -> Result<(), Box<dyn Error>> {
    if contract.schema_version != CONTRACT_SCHEMA_VERSION || contract.build_id != build_id {
        return Err("decoder contract schema or exact build identity is invalid".into());
    }
    if contract
        .policy
        .generated_field_order_treated_as_protobuf_tag
        || !contract.policy.exact_native_wire_tag_required
        || !contract.policy.exact_build_packet_replay_required
    {
        return Err("decoder contract policy is unsafe".into());
    }
    Ok(())
}

fn route_identity_contract_proven(
    spec: &CarrierSpec,
    contracts: &BTreeMap<&str, &DecoderMessageContract>,
) -> bool {
    let Discriminator::ChildField { wrapper_tag, .. } = spec.discriminator;
    let Some((_, discriminating_child)) = spec
        .child_by_wrapper_tag
        .iter()
        .find(|(tag, _)| *tag == wrapper_tag)
    else {
        return false;
    };
    exact_message_contract(spec.wrapper, contracts)
        && exact_message_contract(discriminating_child, contracts)
}

fn full_decoder_contract_proven(
    spec: &CarrierSpec,
    contracts: &BTreeMap<&str, &DecoderMessageContract>,
) -> bool {
    std::iter::once(spec.wrapper)
        .chain(spec.child_by_wrapper_tag.iter().map(|(_, child)| *child))
        .chain((spec.wrapper == "SyncToMeDeltaInfo").then_some("AoiSyncDelta"))
        .all(|name| exact_message_contract(name, contracts))
}

fn exact_message_contract(name: &str, contracts: &BTreeMap<&str, &DecoderMessageContract>) -> bool {
    contracts.get(name).is_some_and(|message| {
        message.generated_full_name.is_some()
            && !message.fields.is_empty()
            && message.fields.iter().all(|field| {
                field.generated_protobuf_tag == Some(field.decoder_tag)
                    && field.tag_state == "exact_native_tag_match"
                    && expected_wire_types(&field.prost_shape).is_some()
            })
    })
}

#[derive(Debug, Default)]
struct PayloadInspection {
    contract_mismatches: u64,
    discriminating_witnesses: u64,
    unknown_fields: u64,
}

fn inspect_payload(
    spec: &CarrierSpec,
    payload: &[u8],
    contracts: &BTreeMap<&str, &DecoderMessageContract>,
) -> Result<PayloadInspection, String> {
    let wrapper_fields = parse_fields(payload)?;
    let mut inspection = inspect_message(spec.wrapper, &wrapper_fields, contracts)?;
    for field in &wrapper_fields {
        let Some((_, child_name)) = spec
            .child_by_wrapper_tag
            .iter()
            .find(|(tag, _)| *tag == field.tag)
        else {
            continue;
        };
        if field.wire_type != 2 {
            continue;
        }
        let child_fields = parse_fields(field.bytes)?;
        let child = inspect_message(child_name, &child_fields, contracts)?;
        inspection.contract_mismatches = inspection
            .contract_mismatches
            .saturating_add(child.contract_mismatches);
        inspection.unknown_fields = inspection
            .unknown_fields
            .saturating_add(child.unknown_fields);
        if discriminator_matches(spec.discriminator, field.tag, &child_fields) {
            inspection.discriminating_witnesses =
                inspection.discriminating_witnesses.saturating_add(1);
        }
        if *child_name == "AoiSyncToMeDelta" {
            for base in child_fields
                .iter()
                .filter(|nested| nested.tag == 1 && nested.wire_type == 2)
            {
                let base_fields = parse_fields(base.bytes)?;
                let base_inspection = inspect_message("AoiSyncDelta", &base_fields, contracts)?;
                inspection.contract_mismatches = inspection
                    .contract_mismatches
                    .saturating_add(base_inspection.contract_mismatches);
                inspection.unknown_fields = inspection
                    .unknown_fields
                    .saturating_add(base_inspection.unknown_fields);
            }
        }
    }
    Ok(inspection)
}

fn inspect_message(
    name: &str,
    fields: &[WireField<'_>],
    contracts: &BTreeMap<&str, &DecoderMessageContract>,
) -> Result<PayloadInspection, String> {
    let contract = contracts
        .get(name)
        .ok_or_else(|| format!("decoder contract lacks {name}"))?;
    let expected = contract
        .fields
        .iter()
        .map(|field| {
            expected_wire_types(&field.prost_shape)
                .map(|wire| (field.decoder_tag, wire))
                .ok_or_else(|| format!("unsupported prost shape {}", field.prost_shape))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let mut inspection = PayloadInspection::default();
    for field in fields {
        match expected.get(&field.tag) {
            Some(wire_types) if !wire_types.contains(&field.wire_type) => {
                inspection.contract_mismatches = inspection.contract_mismatches.saturating_add(1);
            }
            None => {
                inspection.unknown_fields = inspection.unknown_fields.saturating_add(1);
            }
            _ => {}
        }
    }
    Ok(inspection)
}

fn discriminator_matches(
    discriminator: Discriminator,
    wrapper_tag: u32,
    child_fields: &[WireField<'_>],
) -> bool {
    match discriminator {
        Discriminator::ChildField {
            wrapper_tag: expected_wrapper_tag,
            child_tag,
            wire_type,
        } => {
            wrapper_tag == expected_wrapper_tag
                && child_fields
                    .iter()
                    .any(|field| field.tag == child_tag && field.wire_type == wire_type)
        }
    }
}

fn expected_wire_types(shape: &str) -> Option<Vec<u8>> {
    let scalar = shape.split(',').next()?.trim();
    let mut result = match scalar {
        "double" | "fixed64" | "sfixed64" => vec![1],
        "float" | "fixed32" | "sfixed32" => vec![5],
        "string" | "bytes = \"vec\"" | "message" => vec![2],
        "int32" | "int64" | "uint32" | "uint64" | "sint32" | "sint64" | "bool" | "enumeration" => {
            vec![0]
        }
        _ => return None,
    };
    if shape.contains("repeated") && shape.contains("packed = \"true\"") && !result.contains(&2) {
        result.push(2);
    }
    Some(result)
}

fn parse_fields(mut bytes: &[u8]) -> Result<Vec<WireField<'_>>, String> {
    let mut fields = Vec::new();
    while !bytes.is_empty() {
        let (key, key_len) = decode_varint(bytes)?;
        bytes = &bytes[key_len..];
        let tag = u32::try_from(key >> 3).map_err(|_| "protobuf tag overflow")?;
        let wire_type = (key & 7) as u8;
        if tag == 0 {
            return Err("protobuf tag zero".to_owned());
        }
        let value = match wire_type {
            0 => {
                let (_, len) = decode_varint(bytes)?;
                let value = &bytes[..len];
                bytes = &bytes[len..];
                value
            }
            1 => take_fixed(&mut bytes, 8)?,
            2 => {
                let (len, prefix) = decode_varint(bytes)?;
                bytes = &bytes[prefix..];
                let len = usize::try_from(len).map_err(|_| "length-delimited field overflow")?;
                take_fixed(&mut bytes, len)?
            }
            5 => take_fixed(&mut bytes, 4)?,
            _ => return Err(format!("unsupported protobuf wire type {wire_type}")),
        };
        fields.push(WireField {
            tag,
            wire_type,
            bytes: value,
        });
    }
    Ok(fields)
}

fn take_fixed<'a>(bytes: &mut &'a [u8], len: usize) -> Result<&'a [u8], String> {
    if bytes.len() < len {
        return Err("truncated protobuf field".to_owned());
    }
    let (head, tail) = bytes.split_at(len);
    *bytes = tail;
    Ok(head)
}

fn decode_varint(bytes: &[u8]) -> Result<(u64, usize), String> {
    let mut value = 0_u64;
    for (index, byte) in bytes.iter().copied().take(10).enumerate() {
        if index == 9 && byte > 1 {
            return Err("protobuf varint overflow".to_owned());
        }
        value |= u64::from(byte & 0x7f) << (index * 7);
        if byte & 0x80 == 0 {
            return Ok((value, index + 1));
        }
    }
    Err("truncated protobuf varint".to_owned())
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, Box<dyn Error>> {
    Ok(serde_json::from_reader(BufReader::new(File::open(path)?))?)
}

fn arguments() -> Result<Arguments, Box<dyn Error>> {
    let mut pack = None;
    let mut decoder_contract = None;
    let mut native_route_proof = None;
    let mut journals = Vec::new();
    let mut output = None;
    let mut args = env::args().skip(1);
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        match flag.as_str() {
            "--pack" => set_once(&mut pack, PathBuf::from(value), "--pack")?,
            "--decoder-contract" => set_once(
                &mut decoder_contract,
                PathBuf::from(value),
                "--decoder-contract",
            )?,
            "--native-route-proof" => set_once(
                &mut native_route_proof,
                PathBuf::from(value),
                "--native-route-proof",
            )?,
            "--journal" => journals.push(PathBuf::from(value)),
            "--output" => set_once(&mut output, PathBuf::from(value), "--output")?,
            _ => return Err(format!("unknown argument {flag}").into()),
        }
    }
    if journals.is_empty() {
        return Err("at least one --journal is required".into());
    }
    Ok(Arguments {
        pack: pack.ok_or("missing --pack")?,
        decoder_contract: decoder_contract.ok_or("missing --decoder-contract")?,
        native_route_proof: native_route_proof.ok_or("missing --native-route-proof")?,
        journals,
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

    #[test]
    fn protobuf_parser_retains_wire_types_and_nested_bytes() {
        let fields = parse_fields(&[0x08, 0x96, 0x01, 0x12, 0x02, 0x08, 0x01]).unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!((fields[0].tag, fields[0].wire_type), (1, 0));
        assert_eq!((fields[1].tag, fields[1].wire_type), (2, 2));
        assert_eq!(fields[1].bytes, &[0x08, 0x01]);
    }

    #[test]
    fn malformed_and_unsupported_wire_data_fail_closed() {
        assert!(parse_fields(&[0x0a, 0x02, 0x01]).is_err());
        assert!(parse_fields(&[0x0b]).is_err());
        assert!(parse_fields(&[0x00]).is_err());
    }

    #[test]
    fn packed_numeric_contract_accepts_both_legal_encodings() {
        assert_eq!(
            expected_wire_types("int64, repeated, packed = \"true\""),
            Some(vec![0, 2])
        );
    }

    #[test]
    fn route_discriminators_are_message_shape_specific() {
        let entity = parse_fields(&[0x08, 0x01, 0x10, 0x02]).unwrap();
        let delta = parse_fields(&[0x08, 0x01, 0x12, 0x00]).unwrap();
        assert!(discriminator_matches(
            CARRIER_SPECS[0].discriminator,
            1,
            &entity
        ));
        assert!(!discriminator_matches(
            CARRIER_SPECS[0].discriminator,
            1,
            &delta
        ));
        assert!(discriminator_matches(
            CARRIER_SPECS[1].discriminator,
            1,
            &delta
        ));
    }
}
