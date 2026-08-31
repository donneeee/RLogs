#![allow(clippy::type_complexity)]

use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    fs::{self, File},
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{CanonicalEvent, EntityAttributeUpdateKind, StatusState, TimelineEventKind};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 1;
const MAX_EXAMPLES: usize = 32;
const MAX_WATCHED_EXAMPLES: usize = 64;

#[derive(Debug)]
struct Arguments {
    effect_id: i64,
    temp_attr_table: PathBuf,
    output: PathBuf,
    rlogs: Vec<PathBuf>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct TempAttributeDefinition {
    #[serde(rename = "Id")]
    id: i32,
    #[serde(rename = "Name", default)]
    name: String,
    #[serde(rename = "Desc", default)]
    description: String,
    #[serde(rename = "AttrType")]
    attribute_type: i32,
    #[serde(rename = "LogicType")]
    logic_type: i32,
    #[serde(rename = "AttrParams", default)]
    attribute_parameters: Vec<i64>,
    #[serde(rename = "LowerLimit")]
    lower_limit: i64,
    #[serde(rename = "UpperLimit")]
    upper_limit: i64,
    #[serde(rename = "IsSyncClient")]
    syncs_to_client: bool,
    #[serde(rename = "AttrDesc", default)]
    attribute_description: String,
    #[serde(rename = "AttrIcon", default)]
    attribute_icon: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct EffectInterval {
    start_micros: u64,
    end_micros: u64,
}

#[derive(Debug, Clone, Serialize)]
struct AttributeExample {
    rlog: String,
    session_id: String,
    envelope_sequence: u64,
    timeline_sequence: u64,
    observed_micros: u64,
    game_time_millis: Option<i64>,
    actor_entity_uuid: i64,
    update_kind: &'static str,
    value: i32,
    watched_effect_active: bool,
}

#[derive(Debug, Default)]
struct AttributeAccumulator {
    value_count: u64,
    unknown_update_kind_value_count: u64,
    snapshot_value_count: u64,
    delta_value_count: u64,
    watched_effect_value_count: u64,
    actor_entity_uuids: BTreeSet<i64>,
    distinct_values: BTreeSet<i32>,
    examples: Vec<AttributeExample>,
    watched_examples: Vec<AttributeExample>,
}

#[derive(Debug, Serialize)]
struct AttributeReport {
    attribute_id: i32,
    current_build_definition: Option<TempAttributeDefinition>,
    value_count: u64,
    unknown_update_kind_value_count: u64,
    snapshot_value_count: u64,
    delta_value_count: u64,
    watched_effect_value_count: u64,
    actor_entity_uuids: Vec<i64>,
    distinct_values: Vec<i32>,
    examples: Vec<AttributeExample>,
    watched_effect_examples: Vec<AttributeExample>,
}

#[derive(Debug, Serialize)]
struct SessionReport {
    rlog: String,
    session_id: String,
    temporary_attribute_event_count: u64,
    temporary_attribute_value_count: u64,
    watched_effect_status_event_count: u64,
    watched_effect_interval_count: usize,
    values_inside_watched_effect: u64,
}

#[derive(Debug, Serialize)]
struct AuditBundle {
    schema_version: u16,
    generated_by: &'static str,
    policy: AuditPolicy,
    watched_effect_id: i64,
    current_build_table: TableEvidence,
    totals: Totals,
    sessions: Vec<SessionReport>,
    attributes: Vec<AttributeReport>,
}

#[derive(Debug, Serialize)]
struct AuditPolicy {
    runtime_formula_authority: bool,
    unresolved_evidence_hidden: bool,
    wire_values_scaled_or_reinterpreted: bool,
    status_window_rule: &'static str,
    promotion_requirement: &'static str,
}

#[derive(Debug, Serialize)]
struct TableEvidence {
    path: String,
    sha256: String,
    row_count: usize,
}

#[derive(Debug, Default, Serialize)]
struct Totals {
    temporary_attribute_events: u64,
    temporary_attribute_values: u64,
    values_inside_watched_effect: u64,
    distinct_attribute_ids: usize,
    current_build_resolved_attribute_ids: usize,
    current_build_unresolved_attribute_ids: usize,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("temporary attribute audit failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments()?;
    let table_bytes = fs::read(&arguments.temp_attr_table)?;
    let table_sha256 = format!("sha256:{:x}", Sha256::digest(&table_bytes));
    let table_by_key =
        serde_json::from_slice::<BTreeMap<String, TempAttributeDefinition>>(&table_bytes)?;
    let definitions = table_by_key
        .into_values()
        .map(|row| (row.id, row))
        .collect::<BTreeMap<_, _>>();

    let mut accumulators = BTreeMap::<i32, AttributeAccumulator>::new();
    let mut sessions = Vec::new();
    for rlog in &arguments.rlogs {
        sessions.push(read_session(rlog, arguments.effect_id, &mut accumulators)?);
    }

    let mut totals = Totals::default();
    let attributes = accumulators
        .into_iter()
        .map(|(attribute_id, accumulator)| {
            totals.temporary_attribute_values += accumulator.value_count;
            totals.values_inside_watched_effect += accumulator.watched_effect_value_count;
            let definition = definitions.get(&attribute_id).cloned();
            if definition.is_some() {
                totals.current_build_resolved_attribute_ids += 1;
            } else {
                totals.current_build_unresolved_attribute_ids += 1;
            }
            AttributeReport {
                attribute_id,
                current_build_definition: definition,
                value_count: accumulator.value_count,
                unknown_update_kind_value_count: accumulator.unknown_update_kind_value_count,
                snapshot_value_count: accumulator.snapshot_value_count,
                delta_value_count: accumulator.delta_value_count,
                watched_effect_value_count: accumulator.watched_effect_value_count,
                actor_entity_uuids: accumulator.actor_entity_uuids.into_iter().collect(),
                distinct_values: accumulator.distinct_values.into_iter().collect(),
                examples: accumulator.examples,
                watched_effect_examples: accumulator.watched_examples,
            }
        })
        .collect::<Vec<_>>();
    totals.temporary_attribute_events = sessions
        .iter()
        .map(|session| session.temporary_attribute_event_count)
        .sum();
    totals.distinct_attribute_ids = attributes.len();

    let bundle = AuditBundle {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-temp-attribute-audit",
        policy: AuditPolicy {
            runtime_formula_authority: false,
            unresolved_evidence_hidden: false,
            wire_values_scaled_or_reinterpreted: false,
            status_window_rule: "Applied, refreshed, and stacked status observations contribute exact observed-time intervals only when duration_millis is present; overlapping intervals are merged per target.",
            promotion_requirement: "Prove each raw temporary attribute's fixed-point unit, selector semantics, and cooldown equation from packet-observed transitions before enabling provider-attributed action-opportunity rDPS.",
        },
        watched_effect_id: arguments.effect_id,
        current_build_table: TableEvidence {
            path: arguments.temp_attr_table.display().to_string(),
            sha256: table_sha256,
            row_count: definitions.len(),
        },
        totals,
        sessions,
        attributes,
    };
    let mut writer = BufWriter::new(File::create(arguments.output)?);
    serde_json::to_writer_pretty(&mut writer, &bundle)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn read_session(
    rlog: &Path,
    effect_id: i64,
    accumulators: &mut BTreeMap<i32, AttributeAccumulator>,
) -> Result<SessionReport, Box<dyn std::error::Error>> {
    let (session_id, watched_status_events, intervals) = collect_effect_intervals(rlog, effect_id)?;
    let mut reader = RlogReader::new(BufReader::new(File::open(rlog)?), RlogLimits::default())?;
    let mut report = SessionReport {
        rlog: rlog.display().to_string(),
        session_id,
        temporary_attribute_event_count: 0,
        temporary_attribute_value_count: 0,
        watched_effect_status_event_count: watched_status_events,
        watched_effect_interval_count: intervals.values().map(Vec::len).sum(),
        values_inside_watched_effect: 0,
    };

    while let Some(envelope) = reader.next_event()? {
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            continue;
        };
        let TimelineEventKind::TemporaryAttributes(event) = &timeline.kind else {
            continue;
        };
        report.temporary_attribute_event_count += 1;
        let actor_entity_uuid = event.actor.entity_uuid.0;
        let watched_effect_active = intervals
            .get(&actor_entity_uuid)
            .is_some_and(|ranges| interval_contains(ranges, timeline.time.observed_micros));
        let update_kind = match event.update_kind {
            EntityAttributeUpdateKind::Unknown => "unknown",
            EntityAttributeUpdateKind::Snapshot => "snapshot",
            EntityAttributeUpdateKind::Delta => "delta",
        };
        for attribute in &event.attributes {
            report.temporary_attribute_value_count += 1;
            report.values_inside_watched_effect += u64::from(watched_effect_active);
            let accumulator = accumulators.entry(attribute.id).or_default();
            accumulator.value_count += 1;
            match event.update_kind {
                EntityAttributeUpdateKind::Unknown => {
                    accumulator.unknown_update_kind_value_count += 1
                }
                EntityAttributeUpdateKind::Snapshot => accumulator.snapshot_value_count += 1,
                EntityAttributeUpdateKind::Delta => accumulator.delta_value_count += 1,
            }
            accumulator.watched_effect_value_count += u64::from(watched_effect_active);
            accumulator.actor_entity_uuids.insert(actor_entity_uuid);
            accumulator.distinct_values.insert(attribute.value);
            let example = AttributeExample {
                rlog: rlog.display().to_string(),
                session_id: envelope.session_id.clone(),
                envelope_sequence: envelope.sequence,
                timeline_sequence: timeline.sequence,
                observed_micros: timeline.time.observed_micros,
                game_time_millis: timeline.time.game_time_millis,
                actor_entity_uuid,
                update_kind,
                value: attribute.value,
                watched_effect_active,
            };
            if accumulator.examples.len() < MAX_EXAMPLES {
                accumulator.examples.push(example.clone());
            }
            if watched_effect_active && accumulator.watched_examples.len() < MAX_WATCHED_EXAMPLES {
                accumulator.watched_examples.push(example);
            }
        }
    }
    Ok(report)
}

fn collect_effect_intervals(
    rlog: &Path,
    effect_id: i64,
) -> Result<(String, u64, BTreeMap<i64, Vec<EffectInterval>>), Box<dyn std::error::Error>> {
    let mut reader = RlogReader::new(BufReader::new(File::open(rlog)?), RlogLimits::default())?;
    let mut session_id = String::new();
    let mut status_event_count = 0_u64;
    let mut raw_intervals = BTreeMap::<i64, Vec<EffectInterval>>::new();
    while let Some(envelope) = reader.next_event()? {
        session_id = envelope.session_id.clone();
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            continue;
        };
        let TimelineEventKind::Status(status) = &timeline.kind else {
            continue;
        };
        if status.effect.0 != effect_id {
            continue;
        }
        status_event_count += 1;
        if !matches!(
            status.state,
            StatusState::Applied | StatusState::Refreshed | StatusState::Stacked
        ) {
            continue;
        }
        let Some(duration_millis) = status.duration_millis else {
            continue;
        };
        let duration_micros = duration_millis.saturating_mul(1_000);
        let start_micros = timeline.time.observed_micros;
        let end_micros = start_micros.saturating_add(duration_micros);
        raw_intervals
            .entry(status.target.entity_uuid.0)
            .or_default()
            .push(EffectInterval {
                start_micros,
                end_micros,
            });
    }
    let intervals = raw_intervals
        .into_iter()
        .map(|(actor, ranges)| (actor, merge_intervals(ranges)))
        .collect();
    Ok((session_id, status_event_count, intervals))
}

fn merge_intervals(mut intervals: Vec<EffectInterval>) -> Vec<EffectInterval> {
    intervals.sort_unstable();
    let mut merged = Vec::<EffectInterval>::new();
    for interval in intervals {
        if let Some(last) = merged.last_mut()
            && interval.start_micros <= last.end_micros
        {
            last.end_micros = last.end_micros.max(interval.end_micros);
            continue;
        }
        merged.push(interval);
    }
    merged
}

fn interval_contains(intervals: &[EffectInterval], observed_micros: u64) -> bool {
    intervals.iter().any(|interval| {
        observed_micros >= interval.start_micros && observed_micros <= interval.end_micros
    })
}

fn arguments() -> Result<Arguments, String> {
    let mut values = env::args_os().skip(1).collect::<Vec<_>>();
    let effect_id = take_option(&mut values, "--effect")?
        .to_string_lossy()
        .parse::<i64>()
        .map_err(|_| "--effect requires an integer".to_owned())?;
    let temp_attr_table = PathBuf::from(take_option(&mut values, "--temp-attr-table")?);
    let output = PathBuf::from(take_option(&mut values, "--output")?);
    if values.is_empty() {
        return Err(usage());
    }
    Ok(Arguments {
        effect_id,
        temp_attr_table,
        output,
        rlogs: values.into_iter().map(PathBuf::from).collect(),
    })
}

fn take_option(
    values: &mut Vec<std::ffi::OsString>,
    option: &str,
) -> Result<std::ffi::OsString, String> {
    let position = values
        .iter()
        .position(|value| value == option)
        .ok_or_else(usage)?;
    if position + 1 >= values.len() {
        return Err(format!("{option} requires a value"));
    }
    let value = values.remove(position + 1);
    values.remove(position);
    Ok(value)
}

fn usage() -> String {
    "usage: rlogs-bpsr-temp-attribute-audit --effect <status-effect-id> --temp-attr-table <TempAttrTable.json> --output <proof.json> <sealed.rlog>...".to_owned()
}

#[cfg(test)]
mod tests {
    use super::{EffectInterval, interval_contains, merge_intervals};

    #[test]
    fn overlapping_status_observations_merge_without_extending_uncovered_time() {
        let merged = merge_intervals(vec![
            EffectInterval {
                start_micros: 100,
                end_micros: 200,
            },
            EffectInterval {
                start_micros: 150,
                end_micros: 300,
            },
            EffectInterval {
                start_micros: 400,
                end_micros: 500,
            },
        ]);
        assert_eq!(
            merged,
            vec![
                EffectInterval {
                    start_micros: 100,
                    end_micros: 300,
                },
                EffectInterval {
                    start_micros: 400,
                    end_micros: 500,
                },
            ]
        );
        assert!(interval_contains(&merged, 250));
        assert!(!interval_contains(&merged, 350));
    }
}
