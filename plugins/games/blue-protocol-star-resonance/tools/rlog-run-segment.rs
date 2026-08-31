use std::{
    env,
    error::Error,
    fs::{self, File, OpenOptions},
    io::{BufReader, BufWriter, Write},
    mem,
    path::{Path, PathBuf},
};

use rlogs_events::EventProvenance;
use rlogs_game_bpsr::{SealedDungeonRunLog, SegmentedDungeonLogWriter};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde_json::json;

const REPORT_SCHEMA_VERSION: u16 = 1;

#[derive(Debug)]
struct Arguments {
    rlog: PathBuf,
    output_directory: PathBuf,
    output_report: PathBuf,
    base_session_id: String,
    expected_build: String,
    expected_protocol_pack_digest: String,
    maximum_events: u64,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("RLOG run segmentation failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let arguments = arguments()?;
    ensure_available(&arguments.output_report)?;

    let mut reader = RlogReader::new(
        BufReader::new(File::open(&arguments.rlog)?),
        RlogLimits {
            maximum_events: arguments.maximum_events,
            ..RlogLimits::default()
        },
    )?;
    let header = reader.header().clone();
    if header.region.client_build != arguments.expected_build {
        return Err(format!(
            "input build {} does not match expected build {}",
            header.region.client_build, arguments.expected_build
        )
        .into());
    }
    if header.region.protocol_pack_digest != arguments.expected_protocol_pack_digest {
        return Err(format!(
            "input protocol pack digest {} does not match expected digest {}",
            header.region.protocol_pack_digest, arguments.expected_protocol_pack_digest
        )
        .into());
    }

    let mut segmenter = SegmentedDungeonLogWriter::new(
        &arguments.output_directory,
        &arguments.base_session_id,
        format!("rlogs-bpsr-rlog-run-segment/{}", env!("CARGO_PKG_VERSION")),
    )?;
    let mut batch = Vec::new();
    let mut batch_provenance: Option<EventProvenance> = None;
    let mut segments = Vec::new();

    while let Some(envelope) = reader.next_event()? {
        if batch_provenance
            .as_ref()
            .is_some_and(|current| current != &envelope.provenance)
        {
            segments.extend(segmenter.consume_batch(mem::take(&mut batch))?);
            batch_provenance = None;
        }
        batch_provenance.get_or_insert_with(|| envelope.provenance.clone());
        batch.push(envelope);
    }
    if !batch.is_empty() {
        segments.extend(segmenter.consume_batch(batch)?);
    }
    segments.extend(segmenter.finish()?);

    let replay = reader
        .summary()
        .ok_or("sealed RLOG replay summary is missing")?;
    write_report(&arguments, &header, replay, &segments)?;
    println!(
        "segmented {} sealed canonical events into {} dungeon runs",
        replay.event_count,
        segments.len()
    );
    println!("wrote {}", arguments.output_report.display());
    for segment in &segments {
        println!("wrote {}", segment.path.display());
    }
    Ok(())
}

fn write_report(
    arguments: &Arguments,
    header: &rlogs_log_format::RlogHeader,
    replay: &rlogs_log_format::RlogReplaySummary,
    segments: &[SealedDungeonRunLog],
) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = arguments.output_report.parent() {
        fs::create_dir_all(parent)?;
    }
    let output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&arguments.output_report)?;
    let mut output = BufWriter::new(output);
    let segment_rows = segments
        .iter()
        .map(|segment| {
            json!({
                "session_id": segment.session_id,
                "path": segment.path,
                "start_reason": format!("{:?}", segment.start_reason),
                "end_reason": format!("{:?}", segment.end_reason),
                "started": {
                    "instance_id": segment.started.instance_id,
                    "observed_micros": segment.started.time.observed_micros,
                    "game_time_millis": segment.started.time.game_time_millis,
                },
                "ended": {
                    "instance_id": segment.ended.instance_id,
                    "observed_micros": segment.ended.time.observed_micros,
                    "game_time_millis": segment.ended.time.game_time_millis,
                },
                "event_count": segment.seal.event_count,
                "content_sha256": segment.seal.content_sha256,
                "completed": segment.is_completed(),
            })
        })
        .collect::<Vec<_>>();
    let report = json!({
        "schema_version": REPORT_SCHEMA_VERSION,
        "proof_scope": "packet-batched-authoritative-dungeon-run-segmentation",
        "source": {
            "path": arguments.rlog,
            "header": header,
            "sealed_replay": replay,
        },
        "requirements": {
            "expected_build": arguments.expected_build,
            "expected_protocol_pack_digest": arguments.expected_protocol_pack_digest,
            "maximum_events": arguments.maximum_events,
            "remote_player_cast_packet_required": false,
        },
        "batching": {
            "identity": "contiguous canonical events with identical full provenance",
            "reason": "preserve all companion events emitted from one captured packet before a terminal run seal",
        },
        "segment_count": segment_rows.len(),
        "segments": segment_rows,
        "authority": {
            "proves": [
                "sealed source RLOG integrity",
                "exact build and protocol-pack identity",
                "packet-batched authoritative dungeon-run boundaries",
                "sealed per-run event conservation",
            ],
            "does_not_prove": [
                "support-effect magnitude",
                "damage operation order",
                "integer rounding",
                "stacking",
                "provider rDPS credit",
            ],
        },
    });
    serde_json::to_writer_pretty(&mut output, &report)?;
    output.write_all(b"\n")?;
    output.flush()?;
    output.get_ref().sync_all()?;
    Ok(())
}

fn arguments() -> Result<Arguments, Box<dyn Error>> {
    let mut rlog = None;
    let mut output_directory = None;
    let mut output_report = None;
    let mut base_session_id = None;
    let mut expected_build = None;
    let mut expected_protocol_pack_digest = None;
    let mut maximum_events = None;
    let mut args = env::args_os().skip(1);
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("{} requires a value", flag.to_string_lossy()))?;
        match flag.to_string_lossy().as_ref() {
            "--rlog" => rlog = Some(PathBuf::from(value)),
            "--output-directory" => output_directory = Some(PathBuf::from(value)),
            "--output-report" => output_report = Some(PathBuf::from(value)),
            "--base-session-id" => base_session_id = Some(value.to_string_lossy().into_owned()),
            "--expected-build" => expected_build = Some(value.to_string_lossy().into_owned()),
            "--expected-protocol-pack-digest" => {
                expected_protocol_pack_digest = Some(value.to_string_lossy().into_owned())
            }
            "--maximum-events" => {
                let value = value.to_string_lossy();
                let parsed = value.parse::<u64>().map_err(|_| {
                    format!("--maximum-events must be a positive integer, got {value}")
                })?;
                if parsed == 0 {
                    return Err("--maximum-events must be greater than zero".into());
                }
                maximum_events = Some(parsed);
            }
            unknown => return Err(format!("unknown argument {unknown}").into()),
        }
    }
    Ok(Arguments {
        rlog: rlog.ok_or("--rlog is required")?,
        output_directory: output_directory.ok_or("--output-directory is required")?,
        output_report: output_report.ok_or("--output-report is required")?,
        base_session_id: base_session_id.ok_or("--base-session-id is required")?,
        expected_build: expected_build.ok_or("--expected-build is required")?,
        expected_protocol_pack_digest: expected_protocol_pack_digest
            .ok_or("--expected-protocol-pack-digest is required")?,
        maximum_events: maximum_events.unwrap_or(RlogLimits::default().maximum_events),
    })
}

fn ensure_available(path: &Path) -> Result<(), Box<dyn Error>> {
    if path.exists() {
        return Err(format!("refusing to overwrite {}", path.display()).into());
    }
    Ok(())
}
