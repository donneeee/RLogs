use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsString,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::PathBuf,
};

use rlogs_events::{CanonicalEvent, StatusState, TimelineEventKind};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::Serialize;

#[derive(Debug, Clone, Copy)]
struct AttributePoint {
    observed_micros: u64,
    sequence: u64,
    value: u64,
}

#[derive(Debug)]
struct StatusPoint {
    observed_micros: u64,
    sequence: u64,
    source: Option<i64>,
    target: i64,
    state: StatusState,
}

#[derive(Debug, Default, Serialize)]
struct TransitionSummary {
    count: u64,
    before_relative_micros_min: Option<i64>,
    before_relative_micros_max: Option<i64>,
    after_relative_micros_min: Option<i64>,
    after_relative_micros_max: Option<i64>,
    examples: Vec<TransitionExample>,
}

#[derive(Debug, Serialize)]
struct TransitionExample {
    status_sequence: u64,
    status_observed_micros: u64,
    before_sequence: Option<u64>,
    before_observed_micros: Option<u64>,
    after_sequence: Option<u64>,
    after_observed_micros: Option<u64>,
}

#[derive(Debug, Serialize)]
struct Audit {
    schema_version: u16,
    policy: &'static str,
    effect_id: i64,
    window_micros: u64,
    status_event_count: usize,
    transitions: BTreeMap<String, TransitionSummary>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("status attribute correlation failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let (effect_id, attribute_ids, rlogs, output, window_micros) = arguments()?;
    let mut statuses = Vec::new();
    let mut attributes: BTreeMap<(i64, i32), Vec<AttributePoint>> = BTreeMap::new();

    for path in rlogs {
        let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
        while let Some(envelope) = reader.next_event()? {
            let CanonicalEvent::Timeline(timeline) = envelope.event else {
                continue;
            };
            match timeline.kind {
                TimelineEventKind::Status(status) if status.effect.0 == effect_id => {
                    statuses.push(StatusPoint {
                        observed_micros: envelope.time.observed_micros,
                        sequence: envelope.sequence,
                        source: status.source.map(|source| source.entity_uuid.0),
                        target: status.target.entity_uuid.0,
                        state: status.state,
                    });
                }
                TimelineEventKind::EntityAttributes(event) => {
                    for attribute in event.attributes {
                        if !attribute_ids.contains(&attribute.attribute_id) {
                            continue;
                        }
                        let Some(value) = decode_varint(&attribute.raw_value) else {
                            continue;
                        };
                        attributes
                            .entry((event.actor.entity_uuid.0, attribute.attribute_id))
                            .or_default()
                            .push(AttributePoint {
                                observed_micros: envelope.time.observed_micros,
                                sequence: envelope.sequence,
                                value,
                            });
                    }
                }
                _ => {}
            }
        }
    }

    let mut transitions: BTreeMap<String, TransitionSummary> = BTreeMap::new();
    for status in &statuses {
        for attribute_id in &attribute_ids {
            let Some(points) = attributes.get(&(status.target, *attribute_id)) else {
                continue;
            };
            let split = points.partition_point(|point| {
                (point.observed_micros, point.sequence) <= (status.observed_micros, status.sequence)
            });
            let before = split.checked_sub(1).and_then(|index| points.get(index));
            let after = points.get(split);
            let before = before.filter(|point| {
                status.observed_micros.saturating_sub(point.observed_micros) <= window_micros
            });
            let after = after.filter(|point| {
                point.observed_micros.saturating_sub(status.observed_micros) <= window_micros
            });
            if before.is_none() && after.is_none() {
                continue;
            }
            let before_value = before.map(|point| point.value);
            let after_value = after.map(|point| point.value);
            let delta = before_value
                .zip(after_value)
                .map(|(before, after)| i128::from(after) - i128::from(before));
            let key = format!(
                "{:?}|source={}|target={}|attr={attribute_id}|before={}|after={}|delta={}",
                status.state,
                status
                    .source
                    .map(|source| source.to_string())
                    .unwrap_or_else(|| "null".to_owned()),
                status.target,
                before_value
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "null".to_owned()),
                after_value
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "null".to_owned()),
                delta
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "null".to_owned()),
            );
            let summary = transitions.entry(key).or_default();
            summary.count = summary.count.saturating_add(1);
            if summary.examples.len() < 8 {
                summary.examples.push(TransitionExample {
                    status_sequence: status.sequence,
                    status_observed_micros: status.observed_micros,
                    before_sequence: before.map(|point| point.sequence),
                    before_observed_micros: before.map(|point| point.observed_micros),
                    after_sequence: after.map(|point| point.sequence),
                    after_observed_micros: after.map(|point| point.observed_micros),
                });
            }
            if let Some(point) = before {
                let relative = point.observed_micros as i128 - status.observed_micros as i128;
                observe_range(
                    &mut summary.before_relative_micros_min,
                    &mut summary.before_relative_micros_max,
                    i64::try_from(relative).unwrap_or(i64::MIN),
                );
            }
            if let Some(point) = after {
                let relative = point.observed_micros as i128 - status.observed_micros as i128;
                observe_range(
                    &mut summary.after_relative_micros_min,
                    &mut summary.after_relative_micros_max,
                    i64::try_from(relative).unwrap_or(i64::MAX),
                );
            }
        }
    }

    let audit = Audit {
        schema_version: 1,
        policy: "packet_ordered_status_attribute_adjacency_no_formula_inference",
        effect_id,
        window_micros,
        status_event_count: statuses.len(),
        transitions,
    };
    let mut writer = BufWriter::new(File::create(output)?);
    serde_json::to_writer_pretty(&mut writer, &audit)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn observe_range(minimum: &mut Option<i64>, maximum: &mut Option<i64>, value: i64) {
    *minimum = Some(minimum.map_or(value, |current| current.min(value)));
    *maximum = Some(maximum.map_or(value, |current| current.max(value)));
}

fn decode_varint(bytes: &[u8]) -> Option<u64> {
    if bytes.is_empty() {
        return Some(0);
    }
    let mut value = 0_u64;
    for (index, byte) in bytes.iter().copied().take(10).enumerate() {
        if index == 9 && byte > 1 {
            return None;
        }
        value |= u64::from(byte & 0x7f) << (index * 7);
        if byte & 0x80 == 0 && index + 1 == bytes.len() {
            return Some(value);
        }
        if byte & 0x80 == 0 {
            return None;
        }
    }
    None
}

fn arguments() -> Result<(i64, BTreeSet<i32>, Vec<PathBuf>, PathBuf, u64), String> {
    let mut values = env::args_os().skip(1).collect::<Vec<_>>();
    let effect_id = take_value(&mut values, "--effect")?
        .to_string_lossy()
        .parse::<i64>()
        .map_err(|_| "--effect requires a numeric status effect ID".to_owned())?;
    let output = PathBuf::from(take_value(&mut values, "--output")?);
    let window_micros = match take_optional_value(&mut values, "--window-micros")? {
        Some(value) => value
            .to_string_lossy()
            .parse::<u64>()
            .map_err(|_| "--window-micros requires a non-negative integer".to_owned())?,
        None => 250_000,
    };
    let mut attributes = BTreeSet::new();
    while let Some(position) = values.iter().position(|value| value == "--attribute") {
        if position + 1 >= values.len() {
            return Err("--attribute requires a numeric attribute ID".to_owned());
        }
        let raw = values.remove(position + 1);
        values.remove(position);
        attributes.insert(
            raw.to_string_lossy()
                .parse::<i32>()
                .map_err(|_| "--attribute requires a numeric attribute ID".to_owned())?,
        );
    }
    let mut rlogs = Vec::new();
    while let Some(position) = values.iter().position(|value| value == "--rlog") {
        if position + 1 >= values.len() {
            return Err("--rlog requires a path".to_owned());
        }
        rlogs.push(PathBuf::from(values.remove(position + 1)));
        values.remove(position);
    }
    if attributes.is_empty() || rlogs.is_empty() || !values.is_empty() {
        return Err(usage());
    }
    Ok((effect_id, attributes, rlogs, output, window_micros))
}

fn take_value(values: &mut Vec<OsString>, flag: &str) -> Result<OsString, String> {
    take_optional_value(values, flag)?.ok_or_else(usage)
}

fn take_optional_value(values: &mut Vec<OsString>, flag: &str) -> Result<Option<OsString>, String> {
    let Some(position) = values.iter().position(|value| value == flag) else {
        return Ok(None);
    };
    if position + 1 >= values.len() {
        return Err(format!("{flag} requires a value"));
    }
    let value = values.remove(position + 1);
    values.remove(position);
    Ok(Some(value))
}

fn usage() -> String {
    "usage: rlogs-bpsr-status-attribute-correlation --effect <id> --attribute <id> [--attribute <id> ...] --rlog <sealed.rlog> [--rlog <sealed.rlog> ...] --output <audit.json> [--window-micros <micros>]".to_owned()
}
