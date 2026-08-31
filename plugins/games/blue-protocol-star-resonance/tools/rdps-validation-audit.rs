use std::{
    env,
    fs::{self, File},
    io::{BufReader, BufWriter, Write},
    path::PathBuf,
    time::Instant,
};

use rlogs_game_bpsr::{
    RDPS_VALIDATION_REPORT_SCHEMA_VERSION, RdpsValidationAnalyzer, RdpsValidationReport,
};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::Serialize;

#[derive(Debug)]
struct Arguments {
    manifest: PathBuf,
    output: PathBuf,
    rlogs: Vec<PathBuf>,
    aggregate_only: bool,
    compact: bool,
    closure_projection: bool,
    retain_formula_input_snapshots: bool,
}

#[derive(Debug, Serialize)]
struct ValidationAuditBundle {
    schema_version: u16,
    manifest_path: String,
    elapsed_micros: u128,
    observation_elapsed_micros: u128,
    report_construction_elapsed_micros: u128,
    serialization_elapsed_micros: u128,
    total_events: u64,
    events_per_second: f64,
    aggregate: RdpsValidationReport,
    reports: Vec<ValidationAuditEntry>,
}

#[derive(Debug, Serialize)]
struct ValidationAuditEntry {
    source_path: String,
    session_id: String,
    report: RdpsValidationReport,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("rDPS validation audit failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments()?;
    let manifest = fs::read_to_string(&arguments.manifest)?;
    let started = Instant::now();
    let observation_started = Instant::now();
    let mut total_events = 0_u64;
    let source_count = arguments.rlogs.len();
    let mut reports = if arguments.aggregate_only {
        Vec::new()
    } else {
        Vec::with_capacity(source_count)
    };
    let mut report_construction_elapsed = std::time::Duration::ZERO;
    let mut aggregate = RdpsValidationAnalyzer::from_manifest_json(&manifest)?;

    for path in &arguments.rlogs {
        aggregate.begin_session();
        let mut analyzer = if arguments.aggregate_only {
            None
        } else {
            Some(RdpsValidationAnalyzer::from_manifest_json(&manifest)?)
        };
        let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
        let session_id = reader.header().session_id.clone();
        while let Some(event) = reader.next_event()? {
            if let Some(analyzer) = &mut analyzer {
                analyzer.observe(&event);
            }
            aggregate.observe(&event);
            total_events = total_events.saturating_add(1);
        }
        if let Some(analyzer) = analyzer {
            let report_started = Instant::now();
            let report = analyzer.report();
            report_construction_elapsed += report_started.elapsed();
            reports.push(ValidationAuditEntry {
                source_path: path.display().to_string(),
                session_id,
                report,
            });
        }
    }

    let observation_elapsed = observation_started
        .elapsed()
        .saturating_sub(report_construction_elapsed);
    let report_started = Instant::now();
    let mut aggregate = aggregate.report();
    if arguments.closure_projection {
        project_for_proof_closure(&mut aggregate, arguments.retain_formula_input_snapshots);
    }
    report_construction_elapsed += report_started.elapsed();
    let events_per_second = if observation_elapsed.as_secs_f64() > 0.0 {
        total_events as f64 / observation_elapsed.as_secs_f64()
    } else {
        0.0
    };
    let mut bundle = ValidationAuditBundle {
        schema_version: RDPS_VALIDATION_REPORT_SCHEMA_VERSION,
        manifest_path: arguments.manifest.display().to_string(),
        elapsed_micros: 0,
        observation_elapsed_micros: observation_elapsed.as_micros(),
        report_construction_elapsed_micros: report_construction_elapsed.as_micros(),
        serialization_elapsed_micros: 0,
        total_events,
        events_per_second,
        aggregate,
        reports,
    };
    let serialization_started = Instant::now();
    if !arguments.aggregate_only {
        let _ = if arguments.compact {
            serde_json::to_vec(&bundle)?
        } else {
            serde_json::to_vec_pretty(&bundle)?
        };
        bundle.serialization_elapsed_micros = serialization_started.elapsed().as_micros();
    }
    bundle.elapsed_micros = started.elapsed().as_micros();
    let output = File::create(&arguments.output)?;
    let mut writer = BufWriter::new(output);
    if arguments.compact {
        serde_json::to_writer(&mut writer, &bundle)?;
    } else {
        serde_json::to_writer_pretty(&mut writer, &bundle)?;
    }
    writer.write_all(b"\n")?;
    writer.flush()?;
    let serialization_elapsed_micros = serialization_started.elapsed().as_micros();

    println!(
        "Audited {} event(s) from {} log(s) at {:.0} events/s -> {}",
        total_events,
        source_count,
        events_per_second,
        arguments.output.display(),
    );
    println!(
        "timing: observe {} us, construct reports {} us, serialize {} us",
        bundle.observation_elapsed_micros,
        bundle.report_construction_elapsed_micros,
        serialization_elapsed_micros,
    );
    for entry in &bundle.reports {
        println!(
            "{}: complete candidate coverage {}, partial {}, no evidence {}, proof promotions {}",
            entry.session_id,
            entry.report.summary.candidate_event_coverage_complete,
            entry.report.summary.partial_candidate_event_coverage,
            entry.report.summary.no_candidate_evidence,
            entry.report.summary.proof_promotions,
        );
    }
    println!(
        "aggregate: complete candidate coverage {}, partial {}, no evidence {}, proof promotions {}",
        bundle.aggregate.summary.candidate_event_coverage_complete,
        bundle.aggregate.summary.partial_candidate_event_coverage,
        bundle.aggregate.summary.no_candidate_evidence,
        bundle.aggregate.summary.proof_promotions,
    );
    Ok(())
}

fn arguments() -> Result<Arguments, String> {
    let mut manifest = None;
    let mut output = None;
    let mut rlogs = Vec::new();
    let mut aggregate_only = false;
    let mut compact = false;
    let mut closure_projection = false;
    let mut retain_formula_input_snapshots = false;
    let mut values = env::args_os().skip(1);
    while let Some(argument) = values.next() {
        match argument.to_string_lossy().as_ref() {
            "--manifest" => {
                manifest = Some(PathBuf::from(
                    values.next().ok_or("--manifest requires a path")?,
                ));
            }
            "--output" => {
                output = Some(PathBuf::from(
                    values.next().ok_or("--output requires a path")?,
                ));
            }
            "--aggregate-only" => aggregate_only = true,
            "--compact" => compact = true,
            "--closure-projection" => closure_projection = true,
            "--retain-formula-input-snapshots" => retain_formula_input_snapshots = true,
            "--help" | "-h" => {
                return Err(
                    "usage: rlogs-bpsr-rdps-validation-audit --manifest <manifest.json> --output <report.json> [--aggregate-only] [--compact] [--closure-projection] [--retain-formula-input-snapshots] <capture.rlog> [capture.rlog ...]"
                        .into(),
                );
            }
            value if value.starts_with('-') => {
                return Err(format!("unknown argument {value}"));
            }
            _ => rlogs.push(PathBuf::from(argument)),
        }
    }
    if rlogs.is_empty() {
        return Err("at least one .rlog path is required".into());
    }
    Ok(Arguments {
        manifest: manifest.ok_or("--manifest is required")?,
        output: output.ok_or("--output is required")?,
        rlogs,
        aggregate_only,
        compact,
        closure_projection,
        retain_formula_input_snapshots,
    })
}

/// Keep the semantic gates needed by the proof closure while removing only
/// repeated derived observations. The canonical `.rlog` inputs remain intact
/// and can always regenerate the full forensic report.
fn project_for_proof_closure(
    report: &mut RdpsValidationReport,
    retain_formula_input_snapshots: bool,
) {
    const REPRESENTATIVE_ROWS: usize = 2;
    const REPRESENTATIVE_IDS: usize = 16;

    for obligation in &mut report.obligations {
        obligation.formula_input_snapshot_count =
            Some(obligation.formula_input_snapshots.len() as u64);
        obligation.complete_formula_input_snapshot_count = Some(
            obligation
                .formula_input_snapshots
                .iter()
                .filter(|snapshot| snapshot.state.eq_ignore_ascii_case("complete"))
                .count() as u64,
        );
        obligation.packet_damage_row_count = Some(obligation.packet_damage_rows.len() as u64);

        if !retain_formula_input_snapshots {
            obligation
                .formula_input_snapshots
                .truncate(REPRESENTATIVE_ROWS);
        }
        obligation.packet_damage_rows.truncate(REPRESENTATIVE_ROWS);
        obligation.stack_at_damage_observations.clear();
        obligation.matched_identifiers.truncate(REPRESENTATIVE_IDS);
        obligation.selected_actor_ids.truncate(REPRESENTATIVE_IDS);
        obligation.status_instance_ids.truncate(REPRESENTATIVE_IDS);
        obligation.attribute_values.clear();
        obligation.projected_rational_totals.clear();
    }

    for effect in &mut report.dreamscope_terminal_effects {
        effect.stack_at_damage_observations.clear();
        effect.status_instance_ids.truncate(REPRESENTATIVE_IDS);
    }
}
