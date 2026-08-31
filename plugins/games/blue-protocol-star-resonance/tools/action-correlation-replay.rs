use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    error::Error,
    fs::{self, File},
    io::{BufReader, BufWriter, Write},
    path::PathBuf,
};

use rlogs_events::{CanonicalEvent, RegionIdentity, TimelineEventKind};
use rlogs_game_bpsr::{
    ActionCorrelationAudit, ActionCorrelationReport, BPSR_USE_SKILL_ATTR_BUILD, CaptureRecord,
    CaptureRecordKind, DecoderKind, FragmentKind, JsonlJournalReader, PacketDirection,
    ProtocolDecodeStatus, ProtocolJournal, ProtocolPack, ProtocolPackRouteDisposition,
    ProtocolRuntime, ProtocolRuntimeConfig, RouteKey, decode_client_skill_stage_end,
    decode_client_skill_stage_trigger, decode_server_skill_stage_end,
    decode_world_use_slot_skill_action_into,
};
use serde::Serialize;

const REPORT_SCHEMA_VERSION: u16 = 2;
const WORLD_SERVICE_ID: u64 = 103_198_054;
const USE_SLOT_METHOD_ID: u32 = 0x3D002;
const CLIENT_STAGE_END_METHOD_ID: u32 = 0x300A;
const SERVER_STAGE_END_SERVICE_ID: u64 = 1_664_308_034;
const SERVER_STAGE_END_METHOD_ID: u32 = 0x3006;
const MAXIMUM_PENDING_ACTIONS: usize = 65_536;

#[derive(Debug)]
struct Arguments {
    pack: PathBuf,
    journal: PathBuf,
    output: PathBuf,
}

#[derive(Debug, Default, Clone, Copy)]
struct RouteCounts {
    packet_count: u64,
    valid_payload_count: u64,
    action_uuid_match_count: u64,
    decode_failure_count: u64,
}

#[derive(Debug, Serialize)]
struct RouteEvidence {
    route: RouteKey,
    packet_count: u64,
    valid_payload_count: u64,
    action_uuid_match_count: u64,
    decode_failure_count: u64,
}

#[derive(Debug)]
struct Discovery {
    action_instance_ids: BTreeSet<i32>,
    use_slot: BTreeMap<RouteKey, RouteCounts>,
    trigger: BTreeMap<RouteKey, RouteCounts>,
    client_end: BTreeMap<RouteKey, RouteCounts>,
    server_end: BTreeMap<RouteKey, RouteCounts>,
    exact_client_service_id: Option<u64>,
    exact_trigger_method_id: Option<u32>,
}

#[derive(Debug, Serialize)]
struct DiscoveryReport {
    exact_client_service_id: Option<u64>,
    exact_trigger_method_id: Option<u32>,
    exact_client_stage_end_method_id: u32,
    exact_server_stage_end_route: RouteKey,
    distinct_action_instance_ids: usize,
    use_slot_routes: Vec<RouteEvidence>,
    client_stage_trigger_routes: Vec<RouteEvidence>,
    client_stage_end_routes: Vec<RouteEvidence>,
    server_stage_end_routes: Vec<RouteEvidence>,
}

#[derive(Debug, Serialize)]
struct ReplayReport {
    schema_version: u16,
    proof_scope: &'static str,
    journal_path: String,
    capture_id: String,
    game_build: String,
    record_count: usize,
    capture_gap_count: u64,
    candidate_pack_id: String,
    candidate_pack_digest: String,
    runtime_pack_digest: Option<String>,
    discovery: DiscoveryReport,
    runtime_decode_statuses: BTreeMap<String, u64>,
    stages_without_local_actor: u64,
    action_correlation: ActionCorrelationReport,
    ready_for_manual_damage_namespace_review: bool,
    blockers: Vec<String>,
    policy: ReplayPolicy,
}

#[derive(Debug, Serialize)]
struct ReplayPolicy {
    timestamp_proximity_links_allowed: bool,
    old_build_evidence_allowed: bool,
    unresolved_rows_retained: bool,
    automatic_damage_namespace_authorization: bool,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("action-correlation replay failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = arguments()?;
    if args.output.exists() {
        return Err(format!("refusing to overwrite {}", args.output.display()).into());
    }
    let candidate = ProtocolPack::from_json(&fs::read(&args.pack)?)?;
    let journal = JsonlJournalReader::new(BufReader::new(File::open(&args.journal)?)).read()?;
    let report = analyze(
        &candidate,
        &journal,
        args.journal.to_string_lossy().into_owned(),
    )?;
    if let Some(parent) = args.output.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut output = BufWriter::new(File::create(&args.output)?);
    serde_json::to_writer_pretty(&mut output, &report)?;
    output.write_all(b"\n")?;
    output.flush()?;
    println!(
        "observed {} exact local actions; linked trigger/end/server-end = {}/{}/{}",
        report.action_correlation.actions_observed,
        report.action_correlation.client_stage_triggers_linked,
        report.action_correlation.client_stage_ends_linked,
        report.action_correlation.server_stage_ends_linked,
    );
    println!(
        "damage candidate matches/mismatches = {}/{}; ready for manual review: {}",
        report.action_correlation.damage_candidate_id_matches,
        report.action_correlation.damage_candidate_id_mismatches,
        report.ready_for_manual_damage_namespace_review,
    );
    println!("wrote {}", args.output.display());
    Ok(())
}

fn analyze(
    candidate: &ProtocolPack,
    journal: &ProtocolJournal,
    journal_path: String,
) -> Result<ReplayReport, Box<dyn Error>> {
    journal.validate()?;
    let session = journal.session();
    if session.game_build.build_id != BPSR_USE_SKILL_ATTR_BUILD {
        return Err(format!(
            "journal build {} is not exact required build {}",
            session.game_build.build_id, BPSR_USE_SKILL_ATTR_BUILD
        )
        .into());
    }
    if !candidate.matches(&session.game_build) {
        return Err("candidate pack does not exactly match the journal build".into());
    }
    validate_static_use_slot_route(candidate)?;

    let discovery = discover(journal.records());
    let capture_gap_count = journal
        .records()
        .iter()
        .filter(|record| matches!(record.kind, CaptureRecordKind::Gap(_)))
        .count() as u64;
    let mut blockers = discovery_blockers(&discovery, capture_gap_count);
    let mut runtime_decode_statuses = BTreeMap::new();
    let mut stages_without_local_actor = 0_u64;
    let mut audit = ActionCorrelationAudit::new(MAXIMUM_PENDING_ACTIONS);
    let mut runtime_pack_digest = None;

    if discovery.exact_client_service_id == Some(WORLD_SERVICE_ID) {
        runtime_pack_digest = Some(candidate.digest().to_owned());
        let region = RegionIdentity {
            deployment_id: session.game_build.deployment_id.clone(),
            region_id: session
                .game_build
                .region_id
                .clone()
                .unwrap_or_else(|| "unresolved".to_owned()),
            realm_id: None,
            world_id: None,
        };
        let mut runtime = ProtocolRuntime::new(
            candidate,
            session.capture_id.clone(),
            &session.game_build,
            region,
            vec![],
            ProtocolRuntimeConfig::default(),
        )?;
        let mut local_actor_id = None;
        for record in journal.records() {
            let batch = runtime.process(record)?;
            *runtime_decode_statuses
                .entry(status_name(batch.status).to_owned())
                .or_default() += 1;
            for envelope in &batch.events {
                let CanonicalEvent::Timeline(timeline) = &envelope.event else {
                    continue;
                };
                match &timeline.kind {
                    TimelineEventKind::Cast(cast) if cast.action_timing.is_some() => {
                        local_actor_id.get_or_insert(cast.source.actor_id.0);
                        audit.observe_action(envelope.time.observed_micros, cast);
                    }
                    TimelineEventKind::Damage(damage) => {
                        audit.observe_damage(damage);
                    }
                    _ => {}
                }
            }

            let CaptureRecordKind::Packet(packet) = &record.kind else {
                continue;
            };
            let (Some(route), Some(payload)) = (packet.route, packet.payload.decode_input()) else {
                continue;
            };
            let Some(actor_id) = local_actor_id else {
                if is_stage_route(route.key, &discovery) {
                    stages_without_local_actor = stages_without_local_actor.saturating_add(1);
                }
                continue;
            };
            if route.key.direction == PacketDirection::ClientToServer
                && route.key.fragment == FragmentKind::Call
                && route.key.service_id == WORLD_SERVICE_ID
                && Some(route.key.method_id) == discovery.exact_trigger_method_id
            {
                if let Ok(stage) = decode_client_skill_stage_trigger(payload) {
                    audit.observe_client_stage_trigger(actor_id, stage);
                }
            } else if route.key
                == RouteKey::new(
                    PacketDirection::ClientToServer,
                    FragmentKind::Call,
                    WORLD_SERVICE_ID,
                    CLIENT_STAGE_END_METHOD_ID,
                )
            {
                if let Ok(stage) = decode_client_skill_stage_end(payload) {
                    audit.observe_client_stage_end(actor_id, stage);
                }
            } else if route.key == exact_server_stage_end_route() {
                if let Ok(stage) = decode_server_skill_stage_end(payload) {
                    audit.observe_server_stage_end(actor_id, stage);
                }
            }
        }
    }

    let action_correlation = audit.report();
    if action_correlation.actions_observed == 0 {
        blockers.push("no canonical local UseSlot action was emitted by the replay runtime".into());
    }
    if action_correlation.client_stage_triggers_linked == 0 {
        blockers.push("no client stage trigger linked by exact actor and action UUID".into());
    }
    if action_correlation.client_stage_ends_linked == 0 {
        blockers.push("no client stage end linked by exact actor and action UUID".into());
    }
    if action_correlation.server_stage_ends_linked == 0 {
        blockers.push("no server stage end linked by exact actor and action UUID".into());
    }
    if action_correlation.damage_candidate_id_matches == 0 {
        blockers.push("no damage packet candidate ID matched an exact actor/action UUID".into());
    }
    if action_correlation.damage_candidate_id_mismatches > 0 {
        blockers.push(format!(
            "{} damage candidate IDs did not match an exact actor/action UUID",
            action_correlation.damage_candidate_id_mismatches
        ));
    }
    if stages_without_local_actor > 0 {
        blockers.push(format!(
            "{stages_without_local_actor} stage packets occurred before the local actor was resolved"
        ));
    }
    for status in ["decode_failed", "missing_application_payload"] {
        if let Some(count) = runtime_decode_statuses
            .get(status)
            .copied()
            .filter(|count| *count > 0)
        {
            blockers.push(format!(
                "runtime retained {count} {status} packet records during exact-build replay"
            ));
        }
    }
    blockers.sort();
    blockers.dedup();

    Ok(ReplayReport {
        schema_version: REPORT_SCHEMA_VERSION,
        proof_scope: "exact_current_build_actor_action_instance_chain_without_proximity_links",
        journal_path,
        capture_id: session.capture_id.clone(),
        game_build: session.game_build.build_id.clone(),
        record_count: journal.records().len(),
        capture_gap_count,
        candidate_pack_id: candidate.definition().pack_id.clone(),
        candidate_pack_digest: candidate.digest().to_owned(),
        runtime_pack_digest,
        discovery: discovery_report(&discovery),
        runtime_decode_statuses,
        stages_without_local_actor,
        ready_for_manual_damage_namespace_review: blockers.is_empty(),
        action_correlation,
        blockers,
        policy: ReplayPolicy {
            timestamp_proximity_links_allowed: false,
            old_build_evidence_allowed: false,
            unresolved_rows_retained: true,
            automatic_damage_namespace_authorization: false,
        },
    })
}

fn discover(records: &[CaptureRecord]) -> Discovery {
    let mut use_slot = BTreeMap::<RouteKey, RouteCounts>::new();
    let mut action_instance_ids = BTreeSet::new();
    let mut scratch = Vec::new();
    for record in records {
        let Some((route, payload)) = routed_payload(record) else {
            continue;
        };
        if route.direction != PacketDirection::ClientToServer
            || route.fragment != FragmentKind::Call
            || route.method_id != USE_SLOT_METHOD_ID
        {
            continue;
        }
        let counts = use_slot.entry(route).or_default();
        counts.packet_count = counts.packet_count.saturating_add(1);
        match decode_world_use_slot_skill_action_into(
            BPSR_USE_SKILL_ATTR_BUILD,
            payload,
            &mut scratch,
        ) {
            Ok(Some(action)) if action.param.skill_uuid > 0 => {
                counts.valid_payload_count = counts.valid_payload_count.saturating_add(1);
                counts.action_uuid_match_count = counts.action_uuid_match_count.saturating_add(1);
                action_instance_ids.insert(action.param.skill_uuid);
            }
            Ok(None) => {}
            Ok(Some(_)) | Err(_) => {
                counts.decode_failure_count = counts.decode_failure_count.saturating_add(1);
            }
        }
    }
    let services = use_slot
        .iter()
        .filter(|(_, counts)| counts.valid_payload_count > 0)
        .map(|(route, _)| route.service_id)
        .collect::<BTreeSet<_>>();
    let exact_client_service_id = (services.len() == 1).then(|| *services.iter().next().unwrap());

    let mut trigger = BTreeMap::<RouteKey, RouteCounts>::new();
    let mut client_end = BTreeMap::<RouteKey, RouteCounts>::new();
    let mut server_end = BTreeMap::<RouteKey, RouteCounts>::new();
    if let Some(service_id) = exact_client_service_id {
        for record in records {
            let Some((route, payload)) = routed_payload(record) else {
                continue;
            };
            if route
                == RouteKey::new(
                    PacketDirection::ClientToServer,
                    FragmentKind::Call,
                    service_id,
                    CLIENT_STAGE_END_METHOD_ID,
                )
            {
                let counts = client_end.entry(route).or_default();
                counts.packet_count = counts.packet_count.saturating_add(1);
                match decode_client_skill_stage_end(payload) {
                    Ok(stage) if stage.skill_uuid > 0 => {
                        counts.valid_payload_count = counts.valid_payload_count.saturating_add(1);
                        if action_instance_ids.contains(&stage.skill_uuid) {
                            counts.action_uuid_match_count =
                                counts.action_uuid_match_count.saturating_add(1);
                        }
                    }
                    _ => {
                        counts.decode_failure_count = counts.decode_failure_count.saturating_add(1)
                    }
                }
            } else if route.direction == PacketDirection::ClientToServer
                && route.fragment == FragmentKind::Call
                && route.service_id == service_id
                && route.method_id != USE_SLOT_METHOD_ID
                && let Ok(stage) = decode_client_skill_stage_trigger(payload)
                && stage.skill_uuid > 0
                && known_trigger_type(stage.trigger_type)
            {
                let counts = trigger.entry(route).or_default();
                counts.packet_count = counts.packet_count.saturating_add(1);
                counts.valid_payload_count = counts.valid_payload_count.saturating_add(1);
                if action_instance_ids.contains(&stage.skill_uuid) {
                    counts.action_uuid_match_count =
                        counts.action_uuid_match_count.saturating_add(1);
                }
            }
            if route == exact_server_stage_end_route() {
                let counts = server_end.entry(route).or_default();
                counts.packet_count = counts.packet_count.saturating_add(1);
                match decode_server_skill_stage_end(payload) {
                    Ok(stage) if stage.skill_uuid > 0 => {
                        counts.valid_payload_count = counts.valid_payload_count.saturating_add(1);
                        if action_instance_ids.contains(&stage.skill_uuid) {
                            counts.action_uuid_match_count =
                                counts.action_uuid_match_count.saturating_add(1);
                        }
                    }
                    _ => {
                        counts.decode_failure_count = counts.decode_failure_count.saturating_add(1)
                    }
                }
            }
        }
    }
    let trigger_methods = trigger
        .iter()
        .filter(|(_, counts)| counts.action_uuid_match_count > 0)
        .map(|(route, _)| route.method_id)
        .collect::<BTreeSet<_>>();
    let exact_trigger_method_id =
        (trigger_methods.len() == 1).then(|| *trigger_methods.iter().next().unwrap());
    if let (Some(service_id), Some(method_id)) = (exact_client_service_id, exact_trigger_method_id)
    {
        let exact_route = RouteKey::new(
            PacketDirection::ClientToServer,
            FragmentKind::Call,
            service_id,
            method_id,
        );
        let counts = audit_exact_trigger_route(records, exact_route, &action_instance_ids);
        trigger.clear();
        trigger.insert(exact_route, counts);
    }
    Discovery {
        action_instance_ids,
        use_slot,
        trigger,
        client_end,
        server_end,
        exact_client_service_id,
        exact_trigger_method_id,
    }
}

fn audit_exact_trigger_route(
    records: &[CaptureRecord],
    exact_route: RouteKey,
    action_instance_ids: &BTreeSet<i32>,
) -> RouteCounts {
    let mut counts = RouteCounts::default();
    for record in records {
        let Some((route, payload)) = routed_payload(record) else {
            continue;
        };
        if route != exact_route {
            continue;
        }
        counts.packet_count = counts.packet_count.saturating_add(1);
        match decode_client_skill_stage_trigger(payload) {
            Ok(stage) if stage.skill_uuid > 0 && known_trigger_type(stage.trigger_type) => {
                counts.valid_payload_count = counts.valid_payload_count.saturating_add(1);
                if action_instance_ids.contains(&stage.skill_uuid) {
                    counts.action_uuid_match_count =
                        counts.action_uuid_match_count.saturating_add(1);
                }
            }
            _ => counts.decode_failure_count = counts.decode_failure_count.saturating_add(1),
        }
    }
    counts
}

fn validate_static_use_slot_route(candidate: &ProtocolPack) -> Result<(), Box<dyn Error>> {
    let route = RouteKey::new(
        PacketDirection::ClientToServer,
        FragmentKind::Call,
        WORLD_SERVICE_ID,
        USE_SLOT_METHOD_ID,
    );
    let mapping = candidate
        .definition()
        .routes
        .iter()
        .find(|mapping| mapping.route == route)
        .ok_or("candidate pack lacks the statically exact World.UseSlot route")?;
    if mapping.service_name != "World" || mapping.method_name != "UseSlot" {
        return Err("candidate World.UseSlot route has inconsistent names".into());
    }
    if !matches!(
        mapping.disposition,
        ProtocolPackRouteDisposition::Allowed {
            decoder: DecoderKind::WorldUseSlotV1,
            ..
        }
    ) {
        return Err("candidate World.UseSlot route lacks its strict decoder".into());
    }
    Ok(())
}

fn discovery_blockers(discovery: &Discovery, gap_count: u64) -> Vec<String> {
    let mut blockers = Vec::new();
    if gap_count > 0 {
        blockers.push(format!(
            "matching-build replay contains {gap_count} capture gaps"
        ));
    }
    match discovery.exact_client_service_id {
        None => blockers
            .push("World.UseSlot did not converge on one strictly decoded client service".into()),
        Some(service_id) if service_id != WORLD_SERVICE_ID => blockers.push(format!(
            "World.UseSlot appeared on service {service_id}, not statically proven World service {WORLD_SERVICE_ID}"
        )),
        Some(_) => {}
    }
    if discovery.exact_trigger_method_id.is_none() {
        blockers.push(
            "SyncSkillStageTrigger did not converge on one strict same-service action-UUID route"
                .into(),
        );
    }
    if discovery
        .client_end
        .values()
        .map(|counts| counts.action_uuid_match_count)
        .sum::<u64>()
        == 0
    {
        blockers.push("ClientStageEnd 0x300A had no exact UseSlot action-UUID match".into());
    }
    if discovery
        .server_end
        .values()
        .map(|counts| counts.action_uuid_match_count)
        .sum::<u64>()
        == 0
    {
        blockers
            .push("SyncServerSkillStageEnd 0x3006 had no exact UseSlot action-UUID match".into());
    }
    for (name, routes) in [
        ("World.UseSlot", &discovery.use_slot),
        ("SyncSkillStageTrigger", &discovery.trigger),
        ("ClientStageEnd", &discovery.client_end),
        ("SyncServerSkillStageEnd", &discovery.server_end),
    ] {
        let failures = routes
            .values()
            .map(|counts| counts.decode_failure_count)
            .sum::<u64>();
        if failures > 0 {
            blockers.push(format!(
                "{name} retained {failures} strict payload decode failures"
            ));
        }
    }
    blockers
}

fn discovery_report(discovery: &Discovery) -> DiscoveryReport {
    DiscoveryReport {
        exact_client_service_id: discovery.exact_client_service_id,
        exact_trigger_method_id: discovery.exact_trigger_method_id,
        exact_client_stage_end_method_id: CLIENT_STAGE_END_METHOD_ID,
        exact_server_stage_end_route: exact_server_stage_end_route(),
        distinct_action_instance_ids: discovery.action_instance_ids.len(),
        use_slot_routes: route_evidence(&discovery.use_slot),
        client_stage_trigger_routes: route_evidence(&discovery.trigger),
        client_stage_end_routes: route_evidence(&discovery.client_end),
        server_stage_end_routes: route_evidence(&discovery.server_end),
    }
}

fn route_evidence(routes: &BTreeMap<RouteKey, RouteCounts>) -> Vec<RouteEvidence> {
    routes
        .iter()
        .map(|(route, counts)| RouteEvidence {
            route: *route,
            packet_count: counts.packet_count,
            valid_payload_count: counts.valid_payload_count,
            action_uuid_match_count: counts.action_uuid_match_count,
            decode_failure_count: counts.decode_failure_count,
        })
        .collect()
}

fn routed_payload(record: &CaptureRecord) -> Option<(RouteKey, &[u8])> {
    let CaptureRecordKind::Packet(packet) = &record.kind else {
        return None;
    };
    Some((packet.route?.key, packet.payload.decode_input()?))
}

fn exact_server_stage_end_route() -> RouteKey {
    RouteKey::new(
        PacketDirection::ServerToClient,
        FragmentKind::Notify,
        SERVER_STAGE_END_SERVICE_ID,
        SERVER_STAGE_END_METHOD_ID,
    )
}

fn known_trigger_type(value: i32) -> bool {
    matches!(
        value,
        0 | 1
            | 2
            | 101
            | 102
            | 103
            | 104
            | 105
            | 106
            | 107
            | 108
            | 109
            | 110
            | 111
            | 10_000
            | 10_001
            | 10_002
    )
}

fn is_stage_route(route: RouteKey, discovery: &Discovery) -> bool {
    route == exact_server_stage_end_route()
        || discovery.exact_client_service_id.is_some_and(|service_id| {
            route.direction == PacketDirection::ClientToServer
                && route.fragment == FragmentKind::Call
                && route.service_id == service_id
                && (route.method_id == CLIENT_STAGE_END_METHOD_ID
                    || Some(route.method_id) == discovery.exact_trigger_method_id)
        })
}

fn status_name(status: ProtocolDecodeStatus) -> &'static str {
    match status {
        ProtocolDecodeStatus::Decoded => "decoded",
        ProtocolDecodeStatus::CaptureGap => "capture_gap",
        ProtocolDecodeStatus::Unrouted => "unrouted",
        ProtocolDecodeStatus::OpaqueLocalOnly => "opaque_local_only",
        ProtocolDecodeStatus::Prohibited(_) => "prohibited",
        ProtocolDecodeStatus::MissingApplicationPayload => "missing_application_payload",
        ProtocolDecodeStatus::DecodeFailed => "decode_failed",
    }
}

fn arguments() -> Result<Arguments, Box<dyn Error>> {
    let mut pack = None;
    let mut journal = None;
    let mut output = None;
    let mut args = env::args().skip(1);
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        match flag.as_str() {
            "--pack" => set_once(&mut pack, PathBuf::from(value), "--pack")?,
            "--journal" => set_once(&mut journal, PathBuf::from(value), "--journal")?,
            "--output" => set_once(&mut output, PathBuf::from(value), "--output")?,
            _ => return Err(format!("unknown argument {flag}").into()),
        }
    }
    Ok(Arguments {
        pack: pack.ok_or("missing --pack")?,
        journal: journal.ok_or("missing --journal")?,
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
    use rlogs_game_bpsr::{
        CaptureAdapter, CaptureSession, CompressionState, GameBuild, PacketEnvelope, PacketPayload,
        ProtocolPackDefinition, ProtocolPackTarget, RoutedMessage,
    };

    use super::*;

    fn packet(sequence: u64, route: RouteKey, payload: Vec<u8>) -> CaptureRecord {
        CaptureRecord {
            sequence,
            observed_micros: sequence * 1_000,
            wall_clock_unix_micros: None,
            kind: CaptureRecordKind::Packet(PacketEnvelope {
                connection_id: 1,
                stream_id: 1,
                source: None,
                destination: None,
                direction: route.direction,
                fragment: Some(route.fragment),
                route: Some(RoutedMessage {
                    key: route,
                    stub_id: 0,
                    call_id: Some(sequence as u32),
                }),
                compression: CompressionState::NotCompressed,
                payload: PacketPayload {
                    wire_bytes: payload.clone(),
                    application_bytes: Some(payload),
                },
            }),
        }
    }

    #[test]
    fn strict_trigger_discovery_requires_an_observed_action_uuid() {
        let service = 42;
        let route = RouteKey::new(
            PacketDirection::ClientToServer,
            FragmentKind::Call,
            service,
            0x3009,
        );
        let records = vec![packet(1, route, vec![0x08, 0x65, 0x10, 0x01, 0x18, 0x2A])];
        let mut actions = BTreeSet::new();
        actions.insert(42);
        let mut trigger = BTreeMap::<RouteKey, RouteCounts>::new();
        for record in &records {
            let (observed_route, payload) = routed_payload(record).unwrap();
            let stage = decode_client_skill_stage_trigger(payload).unwrap();
            if stage.skill_uuid > 0 && known_trigger_type(stage.trigger_type) {
                let counts = trigger.entry(observed_route).or_default();
                counts.valid_payload_count += 1;
                if actions.contains(&stage.skill_uuid) {
                    counts.action_uuid_match_count += 1;
                }
            }
        }
        assert_eq!(trigger[&route].action_uuid_match_count, 1);
    }

    #[test]
    fn invalid_trigger_type_is_not_a_route_candidate() {
        let raw = [0x08, 0x03, 0x10, 0x01, 0x18, 0x2A];
        let stage = decode_client_skill_stage_trigger(&raw).unwrap();
        assert!(!known_trigger_type(stage.trigger_type));
    }

    #[test]
    fn every_packet_on_the_discovered_trigger_route_must_decode() {
        let route = RouteKey::new(
            PacketDirection::ClientToServer,
            FragmentKind::Call,
            42,
            0x3009,
        );
        let records = vec![
            packet(1, route, vec![0x08, 0x65, 0x10, 0x01, 0x18, 0x2A]),
            packet(2, route, vec![0x08, 0x65, 0x10]),
        ];
        let counts = audit_exact_trigger_route(&records, route, &BTreeSet::from([42]));

        assert_eq!(counts.packet_count, 2);
        assert_eq!(counts.valid_payload_count, 1);
        assert_eq!(counts.action_uuid_match_count, 1);
        assert_eq!(counts.decode_failure_count, 1);
    }

    #[test]
    fn old_build_journals_are_rejected_before_replay() {
        let candidate = ProtocolPack::build(ProtocolPackDefinition {
            schema_version: 1,
            pack_id: "current-build-candidate".into(),
            target: ProtocolPackTarget {
                deployment_id: "global".into(),
                region_id: None,
                channel: "steam".into(),
                build_id: BPSR_USE_SKILL_ATTR_BUILD.into(),
                executable_version: None,
            },
            acquisition: Default::default(),
            provenance: vec![],
            routes: vec![],
        })
        .unwrap();
        let journal = ProtocolJournal::new(CaptureSession {
            format_version: 1,
            capture_id: "old-replay".into(),
            started_unix_micros: None,
            game_build: GameBuild {
                deployment_id: "global".into(),
                region_id: None,
                channel: "steam".into(),
                build_id: "24252055".into(),
                executable_version: None,
            },
            adapter: CaptureAdapter {
                name: "fixture".into(),
                version: None,
            },
            protocol_pack_digest: None,
        });

        let error = analyze(&candidate, &journal, "fixture.jsonl".into()).unwrap_err();
        assert!(error.to_string().contains("not exact required build"));
    }
}
