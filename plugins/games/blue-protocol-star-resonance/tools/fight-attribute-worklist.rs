use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsString,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{CanonicalEvent, EntityAttributeValue, RunState, TimelineEventKind};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u16 = 2;
const VALUE_DISTRIBUTION_LIMIT: usize = 32;

#[derive(Debug)]
struct Arguments {
    catalog: PathBuf,
    rlogs: Vec<PathBuf>,
    output: PathBuf,
}

#[derive(Debug, Deserialize)]
struct Catalog {
    client_build: String,
    source: CatalogSource,
    family_suffixes: Vec<String>,
    attributes: Vec<CatalogAttribute>,
}

#[derive(Debug, Deserialize, Serialize)]
struct CatalogSource {
    table: String,
    table_hash: u64,
    row_count: u64,
    row_size: u64,
    package: String,
}

#[derive(Debug, Clone, Deserialize)]
struct CatalogAttribute {
    id: i32,
    internal_name: Option<String>,
    design_description_zh_cn: Option<String>,
    storage_type: Option<String>,
    family_ids: Vec<i32>,
    table_unit_hint: String,
}

#[derive(Debug, Serialize)]
struct Worklist {
    schema_version: u16,
    generated_by: &'static str,
    client_build: String,
    source: CatalogSource,
    policy: WorklistPolicy,
    sessions: Vec<SessionSummary>,
    coverage: Coverage,
    families: Vec<FamilyReport>,
    outside_fight_attribute_catalog: Vec<OutsideCatalogAttributeReport>,
}

#[derive(Debug, Serialize)]
struct WorklistPolicy {
    runtime_formula_authority: bool,
    formula_stage_inference_from_names: bool,
    unresolved_packet_evidence_is_hidden: bool,
    exact_value_authority: &'static str,
    summarized_value_limit: usize,
    value_semantics: &'static str,
    intended_use: &'static str,
}

#[derive(Debug, Serialize)]
struct SessionSummary {
    rlog: String,
    session_id: String,
    run_ordinals_observed: u32,
    attribute_batches: u64,
    catalog_attribute_values: u64,
    outside_catalog_attribute_values: u64,
    undecodable_catalog_values: u64,
}

#[derive(Debug, Serialize)]
struct Coverage {
    catalog_rows: usize,
    catalog_families_observed: usize,
    catalog_families_unobserved: usize,
    catalog_member_ids_observed: usize,
    catalog_member_ids_unobserved: usize,
    packet_attribute_values_mapped: u64,
    packet_attribute_values_outside_catalog: u64,
    undecodable_mapped_values: u64,
}

#[derive(Debug, Default)]
struct FamilyAccumulator {
    attribute_batches: u64,
    mapped_values: u64,
    actors: BTreeSet<i64>,
    sessions: BTreeSet<String>,
    actor_runs: BTreeSet<(String, u32, i64)>,
    update_patterns: BTreeMap<Vec<usize>, u64>,
    members: BTreeMap<usize, MemberAccumulator>,
}

#[derive(Debug, Default)]
struct MemberAccumulator {
    observations: u64,
    undecodable: u64,
    zero_values: u64,
    minimum: Option<i64>,
    maximum: Option<i64>,
    values: BTreeMap<i64, u64>,
    deltas: BTreeMap<i64, u64>,
}

#[derive(Debug, Serialize)]
struct FamilyReport {
    base_attribute_id: i32,
    internal_name: Option<String>,
    design_description_zh_cn: Option<String>,
    storage_type: Option<String>,
    table_unit_hint: String,
    formula_stage: Option<String>,
    formula_proof: Option<String>,
    observed_in_packets: bool,
    attribute_batches: u64,
    mapped_values: u64,
    actor_count: usize,
    session_count: usize,
    actor_run_count: usize,
    update_patterns: Vec<UpdatePattern>,
    members: Vec<MemberReport>,
}

#[derive(Debug, Serialize)]
struct UpdatePattern {
    member_offsets: Vec<usize>,
    member_names: Vec<String>,
    count: u64,
}

#[derive(Debug, Serialize)]
struct MemberReport {
    offset: usize,
    attribute_id: i32,
    semantic_suffix: String,
    observations: u64,
    undecodable: u64,
    zero_values: u64,
    minimum: Option<i64>,
    maximum: Option<i64>,
    distinct_values: usize,
    value_distribution: ValueDistribution,
    delta_distribution: ValueDistribution,
}

#[derive(Debug, Default)]
struct OutsideCatalogAccumulator {
    observations: u64,
    actors: BTreeSet<i64>,
    decoded_integer_values: BTreeMap<i64, u64>,
    non_integer_or_undecodable: u64,
}

#[derive(Debug, Serialize)]
struct OutsideCatalogAttributeReport {
    attribute_id: i32,
    observations: u64,
    actor_count: usize,
    decoded_integer_values: ValueDistribution,
    non_integer_or_undecodable: u64,
}

#[derive(Debug, Serialize)]
struct ValueDistribution {
    observations: u64,
    distinct_values: usize,
    minimum: Option<i64>,
    maximum: Option<i64>,
    top_value_counts: Vec<ValueCount>,
    other_distinct_values: usize,
    other_observations: u64,
}

#[derive(Debug, Serialize)]
struct ValueCount {
    value: i64,
    count: u64,
}

#[derive(Debug, Clone, Copy)]
struct MemberAddress {
    base_id: i32,
    offset: usize,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("fight attribute worklist failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = arguments()?;
    let catalog: Catalog = serde_json::from_reader(BufReader::new(File::open(&args.catalog)?))?;
    let (member_addresses, member_total) = member_addresses(&catalog)?;
    let mut families = catalog
        .attributes
        .iter()
        .map(|attribute| (attribute.id, FamilyAccumulator::default()))
        .collect::<BTreeMap<_, _>>();
    let mut outside_catalog = BTreeMap::<i32, OutsideCatalogAccumulator>::new();
    let mut states = BTreeMap::<(String, u32, i64, i32), i64>::new();
    let mut sessions = Vec::new();
    let mut mapped_total = 0_u64;
    let mut outside_catalog_total = 0_u64;
    let mut undecodable_total = 0_u64;

    for rlog in &args.rlogs {
        let summary = read_session(
            rlog,
            &member_addresses,
            &mut families,
            &mut outside_catalog,
            &mut states,
        )?;
        mapped_total = mapped_total.saturating_add(summary.catalog_attribute_values);
        outside_catalog_total =
            outside_catalog_total.saturating_add(summary.outside_catalog_attribute_values);
        undecodable_total = undecodable_total.saturating_add(summary.undecodable_catalog_values);
        sessions.push(summary);
    }

    let mut observed_members = BTreeSet::new();
    let family_reports = catalog
        .attributes
        .iter()
        .map(|attribute| {
            let accumulator = families.remove(&attribute.id).unwrap_or_default();
            finish_family(
                attribute,
                accumulator,
                &catalog.family_suffixes,
                &mut observed_members,
            )
        })
        .collect::<Vec<_>>();
    let observed_families = family_reports
        .iter()
        .filter(|family| family.observed_in_packets)
        .count();
    let outside_fight_attribute_catalog = outside_catalog
        .into_iter()
        .map(
            |(attribute_id, accumulator)| OutsideCatalogAttributeReport {
                attribute_id,
                observations: accumulator.observations,
                actor_count: accumulator.actors.len(),
                decoded_integer_values: summarize_counts(
                    accumulator.decoded_integer_values,
                    VALUE_DISTRIBUTION_LIMIT,
                ),
                non_integer_or_undecodable: accumulator.non_integer_or_undecodable,
            },
        )
        .collect();

    let worklist = Worklist {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-fight-attribute-worklist",
        client_build: catalog.client_build,
        source: catalog.source,
        policy: WorklistPolicy {
            runtime_formula_authority: false,
            formula_stage_inference_from_names: false,
            unresolved_packet_evidence_is_hidden: false,
            exact_value_authority: "input .rlog canonical EntityAttributes events",
            summarized_value_limit: VALUE_DISTRIBUTION_LIMIT,
            value_semantics: "packet-exact decoded integer values; labels and units remain table evidence only",
            intended_use: "prioritize formula experiments and generate explicit proof artifacts before runtime attribution",
        },
        sessions,
        coverage: Coverage {
            catalog_rows: family_reports.len(),
            catalog_families_observed: observed_families,
            catalog_families_unobserved: family_reports.len().saturating_sub(observed_families),
            catalog_member_ids_observed: observed_members.len(),
            catalog_member_ids_unobserved: member_total.saturating_sub(observed_members.len()),
            packet_attribute_values_mapped: mapped_total,
            packet_attribute_values_outside_catalog: outside_catalog_total,
            undecodable_mapped_values: undecodable_total,
        },
        families: family_reports,
        outside_fight_attribute_catalog,
    };

    let mut writer = BufWriter::new(File::create(args.output)?);
    serde_json::to_writer_pretty(&mut writer, &worklist)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn member_addresses(catalog: &Catalog) -> Result<(BTreeMap<i32, MemberAddress>, usize), String> {
    let mut addresses = BTreeMap::new();
    for attribute in &catalog.attributes {
        let member_ids = effective_member_ids(attribute);
        for (offset, member_id) in member_ids.into_iter().enumerate() {
            let address = MemberAddress {
                base_id: attribute.id,
                offset,
            };
            if let Some(previous) = addresses.insert(member_id, address) {
                return Err(format!(
                    "catalog member ID {member_id} belongs to both {} and {}",
                    previous.base_id, attribute.id
                ));
            }
        }
    }
    let count = addresses.len();
    Ok((addresses, count))
}

fn effective_member_ids(attribute: &CatalogAttribute) -> Vec<i32> {
    if attribute.family_ids.is_empty() {
        vec![attribute.id]
    } else {
        attribute.family_ids.clone()
    }
}

fn read_session(
    path: &Path,
    addresses: &BTreeMap<i32, MemberAddress>,
    families: &mut BTreeMap<i32, FamilyAccumulator>,
    outside_catalog: &mut BTreeMap<i32, OutsideCatalogAccumulator>,
    states: &mut BTreeMap<(String, u32, i64, i32), i64>,
) -> Result<SessionSummary, Box<dyn std::error::Error>> {
    let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
    let mut session_id = None::<String>;
    let mut run_ordinal = 0_u32;
    let mut maximum_run_ordinal = 0_u32;
    let mut summary = SessionSummary {
        rlog: file_label(path),
        session_id: "unobserved".to_owned(),
        run_ordinals_observed: 0,
        attribute_batches: 0,
        catalog_attribute_values: 0,
        outside_catalog_attribute_values: 0,
        undecodable_catalog_values: 0,
    };

    while let Some(envelope) = reader.next_event()? {
        if let Some(expected) = &session_id {
            if expected != &envelope.session_id {
                return Err(format!(
                    "{} contains multiple sessions: {expected} and {}",
                    path.display(),
                    envelope.session_id
                )
                .into());
            }
        } else {
            session_id = Some(envelope.session_id.clone());
        }
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            continue;
        };
        match &timeline.kind {
            TimelineEventKind::RunBoundary { state, .. } => match state {
                RunState::Entered => {
                    run_ordinal = run_ordinal.saturating_add(1);
                    maximum_run_ordinal = maximum_run_ordinal.max(run_ordinal);
                }
                RunState::Started if run_ordinal == 0 => {
                    run_ordinal = 1;
                    maximum_run_ordinal = 1;
                }
                _ => {}
            },
            TimelineEventKind::EntityAttributes(event) => {
                summary.attribute_batches = summary.attribute_batches.saturating_add(1);
                let mut per_family_updates = BTreeMap::<i32, BTreeSet<usize>>::new();
                for attribute in &event.attributes {
                    let Some(address) = addresses.get(&attribute.attribute_id).copied() else {
                        let accumulator =
                            outside_catalog.entry(attribute.attribute_id).or_default();
                        accumulator.observations = accumulator.observations.saturating_add(1);
                        accumulator.actors.insert(event.actor.entity_uuid.0);
                        summary.outside_catalog_attribute_values =
                            summary.outside_catalog_attribute_values.saturating_add(1);
                        if let Some(value) = decode_attribute(attribute) {
                            *accumulator.decoded_integer_values.entry(value).or_default() += 1;
                        } else {
                            accumulator.non_integer_or_undecodable =
                                accumulator.non_integer_or_undecodable.saturating_add(1);
                        }
                        continue;
                    };
                    summary.catalog_attribute_values =
                        summary.catalog_attribute_values.saturating_add(1);
                    let family = families
                        .get_mut(&address.base_id)
                        .expect("catalog family accumulator exists");
                    family.mapped_values = family.mapped_values.saturating_add(1);
                    family.actors.insert(event.actor.entity_uuid.0);
                    family.sessions.insert(envelope.session_id.clone());
                    family.actor_runs.insert((
                        envelope.session_id.clone(),
                        run_ordinal,
                        event.actor.entity_uuid.0,
                    ));
                    per_family_updates
                        .entry(address.base_id)
                        .or_default()
                        .insert(address.offset);
                    let member = family.members.entry(address.offset).or_default();
                    member.observations = member.observations.saturating_add(1);
                    let Some(value) = decode_attribute(attribute) else {
                        member.undecodable = member.undecodable.saturating_add(1);
                        summary.undecodable_catalog_values =
                            summary.undecodable_catalog_values.saturating_add(1);
                        continue;
                    };
                    if value == 0 {
                        member.zero_values = member.zero_values.saturating_add(1);
                    }
                    member.minimum = Some(member.minimum.map_or(value, |old| old.min(value)));
                    member.maximum = Some(member.maximum.map_or(value, |old| old.max(value)));
                    *member.values.entry(value).or_default() += 1;
                    let state_key = (
                        envelope.session_id.clone(),
                        run_ordinal,
                        event.actor.entity_uuid.0,
                        attribute.attribute_id,
                    );
                    if let Some(previous) = states.insert(state_key, value) {
                        if previous != value {
                            *member
                                .deltas
                                .entry(value.saturating_sub(previous))
                                .or_default() += 1;
                        }
                    }
                }
                for (base_id, offsets) in per_family_updates {
                    let family = families
                        .get_mut(&base_id)
                        .expect("catalog family accumulator exists");
                    family.attribute_batches = family.attribute_batches.saturating_add(1);
                    *family
                        .update_patterns
                        .entry(offsets.into_iter().collect())
                        .or_default() += 1;
                }
            }
            _ => {}
        }
    }

    summary.session_id = session_id.unwrap_or_else(|| "unobserved".to_owned());
    summary.run_ordinals_observed = maximum_run_ordinal;
    Ok(summary)
}

fn finish_family(
    attribute: &CatalogAttribute,
    mut accumulator: FamilyAccumulator,
    suffixes: &[String],
    observed_members: &mut BTreeSet<i32>,
) -> FamilyReport {
    let member_ids = effective_member_ids(attribute);
    let members = member_ids
        .iter()
        .copied()
        .enumerate()
        .map(|(offset, attribute_id)| {
            let member = accumulator.members.remove(&offset).unwrap_or_default();
            if member.observations > 0 {
                observed_members.insert(attribute_id);
            }
            MemberReport {
                offset,
                attribute_id,
                semantic_suffix: if attribute.family_ids.is_empty() {
                    "value".to_owned()
                } else {
                    suffixes
                        .get(offset)
                        .cloned()
                        .unwrap_or_else(|| format!("member_{offset}"))
                },
                observations: member.observations,
                undecodable: member.undecodable,
                zero_values: member.zero_values,
                minimum: member.minimum,
                maximum: member.maximum,
                distinct_values: member.values.len(),
                value_distribution: summarize_counts(member.values, VALUE_DISTRIBUTION_LIMIT),
                delta_distribution: summarize_counts(member.deltas, VALUE_DISTRIBUTION_LIMIT),
            }
        })
        .collect();
    let update_patterns = accumulator
        .update_patterns
        .into_iter()
        .map(|(member_offsets, count)| UpdatePattern {
            member_names: member_offsets
                .iter()
                .map(|offset| {
                    if attribute.family_ids.is_empty() {
                        "value".to_owned()
                    } else {
                        suffixes
                            .get(*offset)
                            .cloned()
                            .unwrap_or_else(|| format!("member_{offset}"))
                    }
                })
                .collect(),
            member_offsets,
            count,
        })
        .collect();
    FamilyReport {
        base_attribute_id: attribute.id,
        internal_name: attribute.internal_name.clone(),
        design_description_zh_cn: attribute.design_description_zh_cn.clone(),
        storage_type: attribute.storage_type.clone(),
        table_unit_hint: attribute.table_unit_hint.clone(),
        formula_stage: None,
        formula_proof: None,
        observed_in_packets: accumulator.mapped_values > 0,
        attribute_batches: accumulator.attribute_batches,
        mapped_values: accumulator.mapped_values,
        actor_count: accumulator.actors.len(),
        session_count: accumulator.sessions.len(),
        actor_run_count: accumulator.actor_runs.len(),
        update_patterns,
        members,
    }
}

fn summarize_counts(counts: BTreeMap<i64, u64>, limit: usize) -> ValueDistribution {
    let observations = counts.values().copied().sum::<u64>();
    let distinct_values = counts.len();
    let minimum = counts.keys().next().copied();
    let maximum = counts.keys().next_back().copied();
    let mut ranked = counts.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|(left_value, left_count), (right_value, right_count)| {
        right_count
            .cmp(left_count)
            .then_with(|| left_value.cmp(right_value))
    });
    let retained = ranked.len().min(limit);
    let top_observations = ranked
        .iter()
        .take(retained)
        .map(|(_, count)| *count)
        .sum::<u64>();
    let top_value_counts = ranked
        .into_iter()
        .take(retained)
        .map(|(value, count)| ValueCount { value, count })
        .collect();
    ValueDistribution {
        observations,
        distinct_values,
        minimum,
        maximum,
        top_value_counts,
        other_distinct_values: distinct_values.saturating_sub(retained),
        other_observations: observations.saturating_sub(top_observations),
    }
}

fn decode_attribute(attribute: &rlogs_events::EntityAttribute) -> Option<i64> {
    match &attribute.decoded {
        Some(EntityAttributeValue::Integer(value)) => Some(*value),
        Some(EntityAttributeValue::Text(_)) | Some(EntityAttributeValue::Position { .. }) => None,
        None => decode_varint(&attribute.raw_value).map(|value| value as i64),
    }
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
        if byte & 0x80 == 0 {
            return (index + 1 == bytes.len()).then_some(value);
        }
    }
    None
}

fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn arguments() -> Result<Arguments, String> {
    let mut values = env::args_os().skip(1).collect::<Vec<_>>();
    let catalog = PathBuf::from(take_value(&mut values, "--catalog")?);
    let output = PathBuf::from(take_value(&mut values, "--output")?);
    let mut rlogs = Vec::new();
    while let Some(position) = values.iter().position(|value| value == "--rlog") {
        if position + 1 >= values.len() {
            return Err("--rlog requires a path".to_owned());
        }
        rlogs.push(PathBuf::from(values.remove(position + 1)));
        values.remove(position);
    }
    if rlogs.is_empty() || !values.is_empty() {
        return Err(usage());
    }
    Ok(Arguments {
        catalog,
        rlogs,
        output,
    })
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
    "usage: rlogs-bpsr-fight-attribute-worklist --catalog <fight-attributes.json> --rlog <current-decoder.rlog> [--rlog <current-decoder.rlog> ...] --output <worklist.json>".to_owned()
}
