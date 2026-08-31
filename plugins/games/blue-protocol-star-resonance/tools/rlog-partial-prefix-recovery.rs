use std::{
    env,
    error::Error,
    fs::{self, File, OpenOptions},
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{
    CanonicalEvent, DataGapEvent, DataGapKind, EventEnvelope, EventProvenance, EventSensitivity,
    EventTime, EvidenceConfidence, EvidenceSource, TimelineEvent, TimelineEventKind,
};
use rlogs_log_format::{RlogError, RlogHeader, RlogLimits, RlogReader, RlogSeal, RlogWriter};
use serde::Serialize;
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 1;
const GENERATED_BY: &str = "rlogs-bpsr-rlog-partial-prefix-recovery";
const RULE_ID: &str = "bpsr.partial-rlog-tail-recovery.v1";

#[derive(Debug)]
struct Arguments {
    expected_build: String,
    input: PathBuf,
    output_rlog: PathBuf,
    output_receipt: PathBuf,
}

#[derive(Debug, Clone)]
struct PrefixValidation {
    header: RlogHeader,
    termination: &'static str,
    event_count: u64,
    first_observed_micros: u64,
    last_observed_micros: u64,
    last_sequence: u64,
    last_timeline_sequence: u64,
}

#[derive(Debug, Clone, Serialize)]
struct RecoveryReceipt {
    schema_version: u16,
    generated_by: &'static str,
    game_build: String,
    policy: RecoveryPolicy,
    input: FileReceipt,
    validated_prefix: PrefixReceipt,
    output: OutputReceipt,
    derived_terminal_gap: DerivedGapReceipt,
    blockers: Vec<&'static str>,
    content_sha256: String,
}

#[derive(Debug, Clone, Serialize)]
struct RecoveryPolicy {
    original_partial_rlog_is_read_only: bool,
    source_prefix_has_integrity_seal_authority: bool,
    source_prefix_events_are_schema_order_and_region_validated: bool,
    no_missing_source_event_is_synthesized: bool,
    derived_terminal_gap_is_an_exclusion_boundary: bool,
    recovered_rlog_seal_authenticates_the_transformation_not_the_original_capture: bool,
    formula_authority: bool,
    runtime_authority: bool,
    provider_rdps_credit_allowed: bool,
}

#[derive(Debug, Clone, Serialize)]
struct FileReceipt {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Clone, Serialize)]
struct PrefixReceipt {
    session_id: String,
    event_schema_version: u16,
    termination: &'static str,
    valid_prefix_event_count: u64,
    first_observed_micros: u64,
    last_observed_micros: u64,
    last_sequence: u64,
    last_timeline_sequence: u64,
}

#[derive(Debug, Clone, Serialize)]
struct OutputReceipt {
    file: FileReceipt,
    session_id: String,
    event_count: u64,
    content_sha256: String,
    integrity_seal_validated: bool,
}

#[derive(Debug, Clone, Serialize)]
struct DerivedGapReceipt {
    rule_id: &'static str,
    envelope_sequence: u64,
    timeline_sequence: u64,
    observed_micros: u64,
    kind: &'static str,
    detail: &'static str,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("RLOG partial-prefix recovery failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let arguments = arguments()?;
    for output in [&arguments.output_rlog, &arguments.output_receipt] {
        if output.exists() {
            return Err(format!("refusing to overwrite {}", output.display()).into());
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
    }
    if !arguments
        .input
        .file_name()
        .is_some_and(|name| name.to_string_lossy().ends_with(".partial.rlog"))
    {
        return Err("--input must name a .partial.rlog".into());
    }

    let input = file_receipt(&arguments.input)?;
    let validation = validate_prefix(&arguments.input, &arguments.expected_build)?;
    let (seal, derived_gap) = recover_prefix(
        &arguments.input,
        &arguments.output_rlog,
        &validation,
        &arguments.expected_build,
    )?;
    let input_after = file_receipt(&arguments.input)?;
    if input.bytes != input_after.bytes || input.sha256 != input_after.sha256 {
        return Err("source partial RLOG changed during recovery".into());
    }
    let replay = verify_recovered(&arguments.output_rlog)?;
    if replay.event_count != seal.event_count || replay.content_sha256 != seal.content_sha256 {
        return Err("recovered RLOG replay does not match its emitted seal".into());
    }
    let output_file = file_receipt(&arguments.output_rlog)?;

    let mut receipt = RecoveryReceipt {
        schema_version: SCHEMA_VERSION,
        generated_by: GENERATED_BY,
        game_build: arguments.expected_build,
        policy: RecoveryPolicy {
            original_partial_rlog_is_read_only: true,
            source_prefix_has_integrity_seal_authority: false,
            source_prefix_events_are_schema_order_and_region_validated: true,
            no_missing_source_event_is_synthesized: true,
            derived_terminal_gap_is_an_exclusion_boundary: true,
            recovered_rlog_seal_authenticates_the_transformation_not_the_original_capture: true,
            formula_authority: false,
            runtime_authority: false,
            provider_rdps_credit_allowed: false,
        },
        input,
        validated_prefix: PrefixReceipt {
            session_id: validation.header.session_id.clone(),
            event_schema_version: validation.header.event_schema_version,
            termination: validation.termination,
            valid_prefix_event_count: validation.event_count,
            first_observed_micros: validation.first_observed_micros,
            last_observed_micros: validation.last_observed_micros,
            last_sequence: validation.last_sequence,
            last_timeline_sequence: validation.last_timeline_sequence,
        },
        output: OutputReceipt {
            file: output_file,
            session_id: validation.header.session_id,
            event_count: seal.event_count,
            content_sha256: seal.content_sha256,
            integrity_seal_validated: true,
        },
        derived_terminal_gap: derived_gap,
        blockers: vec![
            "the source partial prefix never carried an original integrity seal",
            "the recovered seal authenticates only the deterministic prefix transformation and explicit terminal gap",
            "recovered-prefix observations remain candidate evidence and cannot independently authorize a formula or provider credit",
        ],
        content_sha256: String::new(),
    };
    receipt.content_sha256 = report_digest(&receipt)?;
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&arguments.output_receipt)?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, &receipt)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    writer.get_ref().sync_all()?;
    println!(
        "Recovered {} validated prefix events plus one explicit terminal gap into {}; source seal authority=false.",
        validation.event_count,
        arguments.output_rlog.display()
    );
    Ok(())
}

fn validate_prefix(path: &Path, expected_build: &str) -> Result<PrefixValidation, Box<dyn Error>> {
    let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
    let header = reader.header().clone();
    if header.region.client_build != expected_build {
        return Err(format!(
            "{} contains client build {}, not requested build {expected_build}",
            path.display(),
            header.region.client_build
        )
        .into());
    }
    let mut event_count = 0_u64;
    let mut first_observed_micros = None;
    let mut last_observed_micros = None;
    let mut last_sequence = None;
    let mut last_timeline_sequence = 0_u64;
    let termination = loop {
        match reader.next_event() {
            Ok(Some(envelope)) => {
                event_count = event_count.saturating_add(1);
                first_observed_micros.get_or_insert(envelope.time.observed_micros);
                last_observed_micros = Some(envelope.time.observed_micros);
                last_sequence = Some(envelope.sequence);
                if let CanonicalEvent::Timeline(timeline) = envelope.event {
                    last_timeline_sequence = timeline.sequence;
                }
            }
            Ok(None) => return Err("input unexpectedly contains a valid seal".into()),
            Err(error) => break expected_partial_termination(&error).ok_or(error)?,
        }
    };
    if event_count == 0 {
        return Err("partial RLOG contains no complete prefix events to recover".into());
    }
    Ok(PrefixValidation {
        header,
        termination,
        event_count,
        first_observed_micros: first_observed_micros.expect("non-empty prefix"),
        last_observed_micros: last_observed_micros.expect("non-empty prefix"),
        last_sequence: last_sequence.expect("non-empty prefix"),
        last_timeline_sequence,
    })
}

fn recover_prefix(
    input: &Path,
    output: &Path,
    validation: &PrefixValidation,
    expected_build: &str,
) -> Result<(RlogSeal, DerivedGapReceipt), Box<dyn Error>> {
    let mut reader = RlogReader::new(BufReader::new(File::open(input)?), RlogLimits::default())?;
    if reader.header().region.client_build != expected_build {
        return Err("source build changed between validation and recovery".into());
    }
    let mut output_header = validation.header.clone();
    output_header.producer = format!("{}; {GENERATED_BY}", output_header.producer);
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    let mut writer = RlogWriter::new(BufWriter::new(file), output_header)?;
    let mut copied = 0_u64;
    loop {
        match reader.next_event() {
            Ok(Some(envelope)) => {
                writer.push(&envelope)?;
                copied = copied.saturating_add(1);
            }
            Ok(None) => return Err("source unexpectedly became sealed during recovery".into()),
            Err(error) => {
                let termination = expected_partial_termination(&error).ok_or(error)?;
                if termination != validation.termination {
                    return Err("source partial termination changed during recovery".into());
                }
                break;
            }
        }
    }
    if copied != validation.event_count {
        return Err("source partial event count changed during recovery".into());
    }

    let envelope_sequence = validation
        .last_sequence
        .checked_add(1)
        .ok_or("canonical sequence space exhausted")?;
    let timeline_sequence = validation
        .last_timeline_sequence
        .checked_add(1)
        .ok_or("timeline sequence space exhausted")?;
    let time = EventTime {
        observed_micros: validation.last_observed_micros,
        game_time_millis: None,
    };
    let provenance = EventProvenance {
        confidence: EvidenceConfidence::Exact,
        source: EvidenceSource::Derived {
            rule_id: RULE_ID.to_owned(),
            evidence_sequences: vec![validation.last_sequence],
        },
    };
    let detail = "derived recovery boundary: source partial RLOG ended without an integrity seal; no missing events were synthesized";
    let gap = EventEnvelope {
        schema_version: validation.header.event_schema_version,
        session_id: validation.header.session_id.clone(),
        sequence: envelope_sequence,
        region: validation.header.region.clone(),
        time,
        provenance: provenance.clone(),
        sensitivity: EventSensitivity::PublicGameplay,
        event: CanonicalEvent::Timeline(TimelineEvent {
            sequence: timeline_sequence,
            time,
            provenance,
            kind: TimelineEventKind::DataGap(DataGapEvent {
                kind: DataGapKind::CaptureDrop,
                connection_id: None,
                stream_id: None,
                detail: detail.to_owned(),
            }),
        }),
    };
    writer.push(&gap)?;
    let (mut output, seal) = writer.finish_with_seal()?;
    output.flush()?;
    output.get_ref().sync_all()?;
    Ok((
        seal,
        DerivedGapReceipt {
            rule_id: RULE_ID,
            envelope_sequence,
            timeline_sequence,
            observed_micros: validation.last_observed_micros,
            kind: "capture_drop",
            detail,
        },
    ))
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

fn verify_recovered(path: &Path) -> Result<rlogs_log_format::RlogReplaySummary, RlogError> {
    RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?.replay(|_| Ok(()))
}

fn arguments() -> Result<Arguments, Box<dyn Error>> {
    let mut values = env::args_os().skip(1);
    let mut expected_build = None;
    let mut input = None;
    let mut output_rlog = None;
    let mut output_receipt = None;
    while let Some(flag) = values.next() {
        let flag = flag.into_string().map_err(|_| usage())?;
        let value = values.next().ok_or_else(usage)?;
        match flag.as_str() {
            "--expected-build" => expected_build = Some(value.into_string().map_err(|_| usage())?),
            "--input" => input = Some(PathBuf::from(value)),
            "--output-rlog" => output_rlog = Some(PathBuf::from(value)),
            "--output-receipt" => output_receipt = Some(PathBuf::from(value)),
            _ => return Err(usage().into()),
        }
    }
    let expected_build = expected_build.ok_or_else(usage)?;
    if expected_build.is_empty() || !expected_build.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("--expected-build must contain only ASCII digits".into());
    }
    Ok(Arguments {
        expected_build,
        input: input.ok_or_else(usage)?,
        output_rlog: output_rlog.ok_or_else(usage)?,
        output_receipt: output_receipt.ok_or_else(usage)?,
    })
}

fn usage() -> String {
    "usage: rlogs-bpsr-rlog-partial-prefix-recovery --expected-build <id> --input <partial.rlog> --output-rlog <derived.rlog> --output-receipt <receipt.json>".to_owned()
}

fn file_receipt(path: &Path) -> Result<FileReceipt, Box<dyn Error>> {
    Ok(FileReceipt {
        path: display_path(path),
        bytes: fs::metadata(path)?.len(),
        sha256: sha256_file(path)?,
    })
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

fn report_digest(report: &RecoveryReceipt) -> Result<String, serde_json::Error> {
    let mut copy = report.clone();
    copy.content_sha256.clear();
    serde_json::to_vec(&copy).map(|bytes| format!("{:X}", Sha256::digest(bytes)))
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_expected_tail_failures_are_recoverable() {
        assert_eq!(
            expected_partial_termination(&RlogError::MissingSeal),
            Some("missing_seal_at_record_boundary")
        );
        assert_eq!(
            expected_partial_termination(&RlogError::TimelineSequenceMismatch {
                expected: 4,
                actual: 5,
            }),
            None
        );
    }
}
