use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    env,
    error::Error,
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{
    CanonicalEvent, EntityRef, StatusEffectInstanceId, StatusState, TimelineEventKind,
};
use rlogs_log_format::{RlogError, RlogLimits, RlogReader};
use serde::Serialize;
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 3;
const GENERATED_BY: &str = "rlogs-bpsr-rlog-partial-prefix-audit";

#[derive(Debug)]
struct Arguments {
    expected_build: String,
    expected_protocol_pack_digest: Option<String>,
    effect_ids: BTreeSet<i64>,
    damage_relationship: DamageRelationship,
    rlogs: Vec<PathBuf>,
    output: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
struct AuditReport {
    schema_version: u16,
    generated_by: &'static str,
    expected_game_build: String,
    expected_protocol_pack_digest: Option<String>,
    selected_effect_ids: BTreeSet<i64>,
    damage_relationship: DamageRelationship,
    policy: AuditPolicy,
    summary: AuditSummary,
    inputs: Vec<InputReport>,
    blockers: Vec<&'static str>,
    content_sha256: String,
}

#[derive(Debug, Clone, Serialize)]
struct AuditPolicy {
    original_partial_rlogs_are_read_only: bool,
    valid_prefix_events_are_streamed_one_at_a_time: bool,
    valid_prefix_events_are_schema_order_and_region_validated: bool,
    protocol_pack_digest_is_retained_per_input: bool,
    exact_protocol_pack_digest_is_required_when_supplied: bool,
    damage_relationship_is_explicit: bool,
    missing_or_truncated_tail_is_an_exclusion_boundary: bool,
    open_status_lifecycles_at_tail_are_never_complete_windows: bool,
    partial_prefix_has_integrity_seal_authority: bool,
    packet_absence_is_zero: bool,
    formula_authority: bool,
    runtime_authority: bool,
    provider_rdps_credit_allowed: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
struct AuditSummary {
    input_count: usize,
    exact_build_input_count: usize,
    exact_protocol_pack_digest_input_count: usize,
    protocol_pack_digests: BTreeMap<String, usize>,
    input_bytes: u64,
    valid_prefix_event_count: u64,
    timeline_event_count: u64,
    damage_event_count: u64,
    status_event_count: u64,
    unresolved_status_event_count: u64,
    data_gap_count: u64,
    recorder_pause_count: u64,
    selected_effect_status_event_count: u64,
    selected_effect_applied_count: u64,
    selected_effect_terminal_count: u64,
    selected_effect_complete_prefix_lifecycle_count: u64,
    selected_effect_open_at_partial_tail_count: u64,
    selected_effect_damage_events_while_endpoint_active: u64,
    record_boundary_missing_seal_count: usize,
    truncated_record_tail_count: usize,
    unexpected_sealed_input_count: usize,
    exact_build_prefix_evidence_found: bool,
    exact_protocol_pack_prefix_evidence_found: bool,
    selected_effect_prefix_evidence_found: bool,
    controlled_counterfactual_pair_proven: bool,
    exact_operation_order_proven: bool,
    exact_integer_rounding_proven: bool,
    formula_authority: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
struct EffectCounts {
    total: u64,
    applied: u64,
    refreshed: u64,
    stacked: u64,
    consumed: u64,
    removed: u64,
}

#[derive(Debug, Clone, Serialize)]
struct InputReport {
    path: String,
    bytes: u64,
    sha256: String,
    session_id: String,
    game_build: String,
    protocol_pack_digest: String,
    exact_protocol_pack_digest: bool,
    event_schema_version: u16,
    termination: &'static str,
    integrity_seal_validated: bool,
    valid_prefix_event_count: u64,
    timeline_event_count: u64,
    first_observed_micros: Option<u64>,
    last_observed_micros: Option<u64>,
    damage_event_count: u64,
    damage_ability_counts: BTreeMap<String, u64>,
    status_event_count: u64,
    status_effect_counts: BTreeMap<i64, EffectCounts>,
    unresolved_status_event_count: u64,
    data_gap_count: u64,
    recorder_pause_count: u64,
    selected_effect_status_event_count: u64,
    selected_effect_applied_count: u64,
    selected_effect_terminal_count: u64,
    selected_effect_complete_prefix_lifecycle_count: u64,
    selected_effect_duplicate_application_count: u64,
    selected_effect_unmatched_terminal_count: u64,
    selected_effect_open_at_partial_tail_count: u64,
    selected_effect_damage_events_while_endpoint_active: u64,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum DamageRelationship {
    Source,
    Target,
}

impl DamageRelationship {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "source" => Ok(Self::Source),
            "target" => Ok(Self::Target),
            _ => Err("--damage-relationship must be source or target".to_owned()),
        }
    }

    fn matches(self, endpoint: EntityRef, damage: &rlogs_events::DamageEvent) -> bool {
        match self {
            Self::Source => endpoint == damage.source,
            Self::Target => endpoint == damage.target,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ActiveKey {
    target: EntityRef,
    instance_id: StatusEffectInstanceId,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("RLOG partial-prefix audit failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let arguments = arguments()?;
    if arguments.output.exists() {
        return Err(format!("refusing to overwrite {}", arguments.output.display()).into());
    }

    let mut inputs = arguments
        .rlogs
        .iter()
        .map(|path| scan_rlog(path, &arguments))
        .collect::<Result<Vec<_>, _>>()?;
    inputs.sort_by(|left, right| left.path.cmp(&right.path));
    let summary = summarize(&inputs, arguments.expected_protocol_pack_digest.as_deref());
    let protocol_pack_mismatch = arguments.expected_protocol_pack_digest.is_some()
        && summary.exact_protocol_pack_digest_input_count != summary.input_count;
    let mut report = AuditReport {
        schema_version: SCHEMA_VERSION,
        generated_by: GENERATED_BY,
        expected_game_build: arguments.expected_build,
        expected_protocol_pack_digest: arguments.expected_protocol_pack_digest,
        selected_effect_ids: arguments.effect_ids,
        damage_relationship: arguments.damage_relationship,
        policy: AuditPolicy {
            original_partial_rlogs_are_read_only: true,
            valid_prefix_events_are_streamed_one_at_a_time: true,
            valid_prefix_events_are_schema_order_and_region_validated: true,
            protocol_pack_digest_is_retained_per_input: true,
            exact_protocol_pack_digest_is_required_when_supplied: true,
            damage_relationship_is_explicit: true,
            missing_or_truncated_tail_is_an_exclusion_boundary: true,
            open_status_lifecycles_at_tail_are_never_complete_windows: true,
            partial_prefix_has_integrity_seal_authority: false,
            packet_absence_is_zero: false,
            formula_authority: false,
            runtime_authority: false,
            provider_rdps_credit_allowed: false,
        },
        summary,
        inputs,
        blockers: {
            let mut blockers = vec![
                "partial RLOG prefixes do not have an authenticated integrity seal",
                "a missing or truncated tail prevents any lifecycle crossing the tail from becoming a complete window",
                "prefix observations do not prove an otherwise-identical effect-present/effect-absent damage pair",
                "damage-stage operation order and integer rounding remain unproven",
            ];
            if protocol_pack_mismatch {
                blockers.push(
                    "one or more partial prefixes do not match the required protocol-pack digest",
                );
            }
            blockers
        },
        content_sha256: String::new(),
    };
    report.content_sha256 = report_digest(&report)?;

    if let Some(parent) = arguments.output.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(&arguments.output)?);
    serde_json::to_writer_pretty(&mut writer, &report)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    println!(
        "Audited {} partial RLOG prefixes ({} valid events): {} selected-effect events, {} complete prefix lifecycles, {} open at truncated tails; formula authority=false.",
        report.summary.input_count,
        report.summary.valid_prefix_event_count,
        report.summary.selected_effect_status_event_count,
        report
            .summary
            .selected_effect_complete_prefix_lifecycle_count,
        report.summary.selected_effect_open_at_partial_tail_count,
    );
    Ok(())
}

fn scan_rlog(path: &Path, arguments: &Arguments) -> Result<InputReport, Box<dyn Error>> {
    if !path
        .file_name()
        .is_some_and(|name| name.to_string_lossy().ends_with(".partial.rlog"))
    {
        return Err(format!("{} is not a .partial.rlog", path.display()).into());
    }
    let bytes = fs::metadata(path)?.len();
    let sha256 = sha256_file(path)?;
    let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
    let header = reader.header().clone();
    if header.region.client_build != arguments.expected_build {
        return Err(format!(
            "{} contains client build {}, not requested build {}",
            path.display(),
            header.region.client_build,
            arguments.expected_build
        )
        .into());
    }

    let mut report = InputReport {
        path: display_path(path),
        bytes,
        sha256,
        session_id: header.session_id,
        game_build: header.region.client_build,
        exact_protocol_pack_digest: arguments
            .expected_protocol_pack_digest
            .as_deref()
            .is_none_or(|expected| digest_matches(&header.region.protocol_pack_digest, expected)),
        protocol_pack_digest: header.region.protocol_pack_digest,
        event_schema_version: header.event_schema_version,
        termination: "unknown",
        integrity_seal_validated: false,
        valid_prefix_event_count: 0,
        timeline_event_count: 0,
        first_observed_micros: None,
        last_observed_micros: None,
        damage_event_count: 0,
        damage_ability_counts: BTreeMap::new(),
        status_event_count: 0,
        status_effect_counts: BTreeMap::new(),
        unresolved_status_event_count: 0,
        data_gap_count: 0,
        recorder_pause_count: 0,
        selected_effect_status_event_count: 0,
        selected_effect_applied_count: 0,
        selected_effect_terminal_count: 0,
        selected_effect_complete_prefix_lifecycle_count: 0,
        selected_effect_duplicate_application_count: 0,
        selected_effect_unmatched_terminal_count: 0,
        selected_effect_open_at_partial_tail_count: 0,
        selected_effect_damage_events_while_endpoint_active: 0,
    };
    let mut active = HashMap::<ActiveKey, ()>::new();

    loop {
        match reader.next_event() {
            Ok(Some(envelope)) => {
                report.valid_prefix_event_count = report.valid_prefix_event_count.saturating_add(1);
                report
                    .first_observed_micros
                    .get_or_insert(envelope.time.observed_micros);
                report.last_observed_micros = Some(envelope.time.observed_micros);
                let CanonicalEvent::Timeline(timeline) = envelope.event else {
                    continue;
                };
                report.timeline_event_count = report.timeline_event_count.saturating_add(1);
                match timeline.kind {
                    TimelineEventKind::Damage(damage) => {
                        report.damage_event_count = report.damage_event_count.saturating_add(1);
                        let ability = damage
                            .ability
                            .map(|value| value.0.to_string())
                            .unwrap_or_else(|| "unresolved".to_owned());
                        *report.damage_ability_counts.entry(ability).or_default() += 1;
                        if active
                            .keys()
                            .any(|key| arguments.damage_relationship.matches(key.target, &damage))
                        {
                            report.selected_effect_damage_events_while_endpoint_active = report
                                .selected_effect_damage_events_while_endpoint_active
                                .saturating_add(1);
                        }
                    }
                    TimelineEventKind::Status(status) => {
                        report.status_event_count = report.status_event_count.saturating_add(1);
                        record_status_count(
                            report
                                .status_effect_counts
                                .entry(status.effect.0)
                                .or_default(),
                            status.state,
                        );
                        if !arguments.effect_ids.contains(&status.effect.0) {
                            continue;
                        }
                        report.selected_effect_status_event_count =
                            report.selected_effect_status_event_count.saturating_add(1);
                        let Some(instance_id) = status.instance_id else {
                            continue;
                        };
                        let key = ActiveKey {
                            target: status.target,
                            instance_id,
                        };
                        match status.state {
                            StatusState::Applied => {
                                report.selected_effect_applied_count =
                                    report.selected_effect_applied_count.saturating_add(1);
                                if active.insert(key, ()).is_some() {
                                    report.selected_effect_duplicate_application_count = report
                                        .selected_effect_duplicate_application_count
                                        .saturating_add(1);
                                }
                            }
                            StatusState::Consumed | StatusState::Removed => {
                                report.selected_effect_terminal_count =
                                    report.selected_effect_terminal_count.saturating_add(1);
                                if active.remove(&key).is_some() {
                                    report.selected_effect_complete_prefix_lifecycle_count = report
                                        .selected_effect_complete_prefix_lifecycle_count
                                        .saturating_add(1);
                                } else {
                                    report.selected_effect_unmatched_terminal_count = report
                                        .selected_effect_unmatched_terminal_count
                                        .saturating_add(1);
                                }
                            }
                            StatusState::Refreshed | StatusState::Stacked => {}
                        }
                    }
                    TimelineEventKind::UnresolvedStatus(_) => {
                        report.unresolved_status_event_count =
                            report.unresolved_status_event_count.saturating_add(1);
                    }
                    TimelineEventKind::DataGap(_) => {
                        report.data_gap_count = report.data_gap_count.saturating_add(1);
                        active.clear();
                    }
                    TimelineEventKind::RecorderPause(_) => {
                        report.recorder_pause_count = report.recorder_pause_count.saturating_add(1);
                        active.clear();
                    }
                    TimelineEventKind::RunBoundary { .. } => active.clear(),
                    _ => {}
                }
            }
            Ok(None) => {
                report.termination = "unexpected_valid_seal";
                report.integrity_seal_validated = true;
                break;
            }
            Err(error) => {
                report.termination = expected_partial_termination(&error).ok_or_else(|| {
                    format!(
                        "{} failed before its expected tail: {error}",
                        path.display()
                    )
                })?;
                break;
            }
        }
    }
    report.selected_effect_open_at_partial_tail_count = active.len() as u64;
    Ok(report)
}

fn expected_partial_termination(error: &RlogError) -> Option<&'static str> {
    match error {
        RlogError::MissingSeal => Some("missing_seal_at_record_boundary"),
        RlogError::TruncatedCompactRecord { .. } => Some("truncated_compact_record_tail"),
        RlogError::Io(source) if source.kind() == std::io::ErrorKind::UnexpectedEof => {
            Some("truncated_compact_block_tail")
        }
        _ => None,
    }
}

fn record_status_count(counts: &mut EffectCounts, state: StatusState) {
    counts.total = counts.total.saturating_add(1);
    let count = match state {
        StatusState::Applied => &mut counts.applied,
        StatusState::Refreshed => &mut counts.refreshed,
        StatusState::Stacked => &mut counts.stacked,
        StatusState::Consumed => &mut counts.consumed,
        StatusState::Removed => &mut counts.removed,
    };
    *count = count.saturating_add(1);
}

fn summarize(inputs: &[InputReport], expected_protocol_pack_digest: Option<&str>) -> AuditSummary {
    let mut summary = AuditSummary {
        input_count: inputs.len(),
        exact_build_input_count: inputs.len(),
        ..AuditSummary::default()
    };
    for input in inputs {
        *summary
            .protocol_pack_digests
            .entry(input.protocol_pack_digest.clone())
            .or_default() += 1;
        summary.exact_protocol_pack_digest_input_count +=
            usize::from(input.exact_protocol_pack_digest);
        summary.input_bytes = summary.input_bytes.saturating_add(input.bytes);
        summary.valid_prefix_event_count = summary
            .valid_prefix_event_count
            .saturating_add(input.valid_prefix_event_count);
        summary.timeline_event_count = summary
            .timeline_event_count
            .saturating_add(input.timeline_event_count);
        summary.damage_event_count = summary
            .damage_event_count
            .saturating_add(input.damage_event_count);
        summary.status_event_count = summary
            .status_event_count
            .saturating_add(input.status_event_count);
        summary.unresolved_status_event_count = summary
            .unresolved_status_event_count
            .saturating_add(input.unresolved_status_event_count);
        summary.data_gap_count = summary.data_gap_count.saturating_add(input.data_gap_count);
        summary.recorder_pause_count = summary
            .recorder_pause_count
            .saturating_add(input.recorder_pause_count);
        summary.selected_effect_status_event_count = summary
            .selected_effect_status_event_count
            .saturating_add(input.selected_effect_status_event_count);
        summary.selected_effect_applied_count = summary
            .selected_effect_applied_count
            .saturating_add(input.selected_effect_applied_count);
        summary.selected_effect_terminal_count = summary
            .selected_effect_terminal_count
            .saturating_add(input.selected_effect_terminal_count);
        summary.selected_effect_complete_prefix_lifecycle_count = summary
            .selected_effect_complete_prefix_lifecycle_count
            .saturating_add(input.selected_effect_complete_prefix_lifecycle_count);
        summary.selected_effect_open_at_partial_tail_count = summary
            .selected_effect_open_at_partial_tail_count
            .saturating_add(input.selected_effect_open_at_partial_tail_count);
        summary.selected_effect_damage_events_while_endpoint_active = summary
            .selected_effect_damage_events_while_endpoint_active
            .saturating_add(input.selected_effect_damage_events_while_endpoint_active);
        match input.termination {
            "missing_seal_at_record_boundary" => summary.record_boundary_missing_seal_count += 1,
            "unexpected_valid_seal" => summary.unexpected_sealed_input_count += 1,
            _ => summary.truncated_record_tail_count += 1,
        }
    }
    summary.exact_build_prefix_evidence_found = summary.valid_prefix_event_count > 0;
    summary.exact_protocol_pack_prefix_evidence_found = expected_protocol_pack_digest.is_none()
        || inputs
            .iter()
            .any(|input| input.exact_protocol_pack_digest && input.valid_prefix_event_count > 0);
    summary.selected_effect_prefix_evidence_found = summary.selected_effect_status_event_count > 0;
    summary
}

fn arguments() -> Result<Arguments, Box<dyn Error>> {
    let mut values = env::args_os().skip(1).collect::<Vec<_>>();
    let mut expected_build = None;
    let mut expected_protocol_pack_digest = None;
    let mut effect_ids = BTreeSet::new();
    let mut damage_relationship = None;
    let mut rlogs = Vec::new();
    let mut output = None;
    while !values.is_empty() {
        let option = values.remove(0).into_string().map_err(|_| usage())?;
        let mut value = || -> Result<String, Box<dyn Error>> {
            if values.is_empty() {
                return Err(format!("{option} requires a value").into());
            }
            values.remove(0).into_string().map_err(|_| usage().into())
        };
        match option.as_str() {
            "--expected-build" => expected_build = Some(value()?),
            "--expected-protocol-pack-digest" => {
                expected_protocol_pack_digest = Some(value()?);
            }
            "--effect-id" => {
                effect_ids.insert(value()?.parse()?);
            }
            "--damage-relationship" => {
                damage_relationship = Some(DamageRelationship::parse(&value()?)?);
            }
            "--rlog" => {
                rlogs.push(PathBuf::from(value()?));
            }
            "--output" => {
                output = Some(PathBuf::from(value()?));
            }
            _ => return Err(usage().into()),
        };
    }
    let expected_build = expected_build.ok_or_else(usage)?;
    if expected_build.is_empty() || !expected_build.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("--expected-build must contain only ASCII digits".into());
    }
    if expected_protocol_pack_digest
        .as_deref()
        .is_some_and(|digest| !valid_sha256_digest(digest))
    {
        return Err(
            "--expected-protocol-pack-digest must be sha256: followed by 64 hexadecimal digits"
                .into(),
        );
    }
    if effect_ids.is_empty() || rlogs.is_empty() {
        return Err(usage().into());
    }
    rlogs.sort();
    rlogs.dedup();
    Ok(Arguments {
        expected_build,
        expected_protocol_pack_digest,
        effect_ids,
        damage_relationship: damage_relationship.ok_or_else(usage)?,
        rlogs,
        output: output.ok_or_else(usage)?,
    })
}

fn usage() -> String {
    "usage: rlogs-bpsr-rlog-partial-prefix-audit --expected-build <id> [--expected-protocol-pack-digest <sha256:...>] --effect-id <id> [--effect-id <id> ...] --damage-relationship <source|target> --rlog <partial.rlog> [--rlog <partial.rlog> ...] --output <audit.json>".to_owned()
}

fn valid_sha256_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn digest_matches(actual: &str, expected: &str) -> bool {
    actual.eq_ignore_ascii_case(expected)
}

fn report_digest(report: &AuditReport) -> Result<String, serde_json::Error> {
    let mut copy = report.clone();
    copy.content_sha256.clear();
    serde_json::to_vec(&copy).map(|bytes| hex_digest(&bytes))
}

fn sha256_file(path: &Path) -> Result<String, Box<dyn Error>> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:X}", hasher.finalize()))
}

fn hex_digest(bytes: &[u8]) -> String {
    format!("{:X}", Sha256::digest(bytes))
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_seal_is_an_expected_partial_tail() {
        assert_eq!(
            expected_partial_termination(&RlogError::MissingSeal),
            Some("missing_seal_at_record_boundary")
        );
    }

    #[test]
    fn semantic_replay_errors_are_not_accepted_as_partial_tails() {
        assert_eq!(
            expected_partial_termination(&RlogError::SequenceMismatch {
                expected: 2,
                actual: 3,
            }),
            None
        );
    }

    #[test]
    fn protocol_pack_digest_validation_is_exact_and_case_insensitive() {
        let lower = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let upper = "sha256:0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF";
        assert!(valid_sha256_digest(lower));
        assert!(valid_sha256_digest(upper));
        assert!(digest_matches(lower, upper));
        assert!(!valid_sha256_digest("sha256:1234"));
        assert!(!valid_sha256_digest(
            "md5:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        ));
    }
}
