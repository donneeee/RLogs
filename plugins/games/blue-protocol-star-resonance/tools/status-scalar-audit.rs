use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsString,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::PathBuf,
};

use rlogs_events::{CanonicalEvent, TimelineEventKind};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::Serialize;

#[derive(Debug, Default, Serialize)]
struct EffectSummary {
    event_count: u64,
    levels: BTreeMap<String, u64>,
    part_ids: BTreeMap<String, u64>,
    counts: BTreeMap<String, u64>,
    created_at_present: u64,
    created_at_missing: u64,
    origin_types: BTreeMap<String, u64>,
    origin_configs: BTreeMap<String, u64>,
    states: BTreeMap<String, u64>,
    source_target_pairs: BTreeMap<String, u64>,
}

#[derive(Debug, Serialize)]
struct Audit {
    schema_version: u16,
    policy: &'static str,
    effects: BTreeMap<i64, EffectSummary>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("status scalar audit failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let (effect_ids, rlogs, output) = arguments()?;
    let mut effects = effect_ids
        .iter()
        .copied()
        .map(|effect| (effect, EffectSummary::default()))
        .collect::<BTreeMap<_, _>>();

    for path in rlogs {
        let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
        while let Some(envelope) = reader.next_event()? {
            let CanonicalEvent::Timeline(timeline) = envelope.event else {
                continue;
            };
            let TimelineEventKind::Status(status) = timeline.kind else {
                continue;
            };
            let effect_id = status.effect.0;
            let Some(summary) = effects.get_mut(&effect_id) else {
                continue;
            };

            summary.event_count = summary.event_count.saturating_add(1);
            increment(&mut summary.levels, option_i32(status.level));
            increment(&mut summary.part_ids, option_i32(status.part_id));
            increment(&mut summary.counts, option_i32(status.count));
            if status.created_at_millis.is_some() {
                summary.created_at_present = summary.created_at_present.saturating_add(1);
            } else {
                summary.created_at_missing = summary.created_at_missing.saturating_add(1);
            }
            increment(
                &mut summary.origin_types,
                status
                    .origin
                    .map(|origin| origin.source_type_id.to_string())
                    .unwrap_or_else(|| "null".to_owned()),
            );
            increment(
                &mut summary.origin_configs,
                status
                    .origin
                    .map(|origin| origin.source_config_id.to_string())
                    .unwrap_or_else(|| "null".to_owned()),
            );
            increment(&mut summary.states, format!("{:?}", status.state));
            let source = status
                .source
                .map(|source| source.entity_uuid.0.to_string())
                .unwrap_or_else(|| "null".to_owned());
            increment(
                &mut summary.source_target_pairs,
                format!("{source}->{}", status.target.entity_uuid.0),
            );
        }
    }

    let audit = Audit {
        schema_version: 1,
        policy: "packet_exact_status_scalars_no_inference",
        effects,
    };
    let mut writer = BufWriter::new(File::create(output)?);
    serde_json::to_writer_pretty(&mut writer, &audit)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn increment(values: &mut BTreeMap<String, u64>, key: String) {
    *values.entry(key).or_default() += 1;
}

fn option_i32(value: Option<i32>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_owned())
}

fn arguments() -> Result<(BTreeSet<i64>, Vec<PathBuf>, PathBuf), String> {
    let mut values = env::args_os().skip(1).collect::<Vec<_>>();
    let output = PathBuf::from(take_value(&mut values, "--output")?);
    let mut effects = BTreeSet::new();
    while let Some(position) = values.iter().position(|value| value == "--effect") {
        if position + 1 >= values.len() {
            return Err("--effect requires a numeric status effect ID".to_owned());
        }
        let raw = values.remove(position + 1);
        values.remove(position);
        let parsed = raw
            .to_string_lossy()
            .parse::<i64>()
            .map_err(|_| "--effect requires a numeric status effect ID".to_owned())?;
        effects.insert(parsed);
    }
    let mut rlogs = Vec::new();
    while let Some(position) = values.iter().position(|value| value == "--rlog") {
        if position + 1 >= values.len() {
            return Err("--rlog requires a path".to_owned());
        }
        rlogs.push(PathBuf::from(values.remove(position + 1)));
        values.remove(position);
    }
    if effects.is_empty() || rlogs.is_empty() || !values.is_empty() {
        return Err(usage());
    }
    Ok((effects, rlogs, output))
}

fn take_value(values: &mut Vec<OsString>, flag: &str) -> Result<OsString, String> {
    let position = values
        .iter()
        .position(|value| value == flag)
        .ok_or_else(usage)?;
    if position + 1 >= values.len() {
        return Err(format!("{flag} requires a value"));
    }
    let value = values.remove(position + 1);
    values.remove(position);
    Ok(value)
}

fn usage() -> String {
    "usage: rlogs-bpsr-status-scalar-audit --effect <id> [--effect <id> ...] --rlog <sealed.rlog> [--rlog <sealed.rlog> ...] --output <audit.json>".to_owned()
}
