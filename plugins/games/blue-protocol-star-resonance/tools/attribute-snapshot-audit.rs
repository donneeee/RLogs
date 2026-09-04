use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsString,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{
    CanonicalEvent, EntityAttribute, EntityAttributeUpdateKind, EntityAttributeValue, RunState,
    TimelineEventKind,
};
use rlogs_game_bpsr::decode_known_entity_attribute_value;
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u16 = 2;

#[derive(Debug)]
struct Arguments {
    catalog: PathBuf,
    rlogs: Vec<PathBuf>,
    output: PathBuf,
    entities: BTreeSet<i64>,
}

#[derive(Debug, Deserialize)]
struct Catalog {
    client_build: String,
    attributes: Vec<CatalogAttribute>,
}

#[derive(Debug, Deserialize)]
struct CatalogAttribute {
    id: i32,
    internal_name: Option<String>,
}

#[derive(Debug, Default)]
struct ActorState {
    damage_events: u64,
    first_snapshot_sequence: Option<u64>,
    snapshot_values: BTreeMap<i32, Option<i64>>,
    delta_ids: BTreeSet<i32>,
}

#[derive(Debug, Serialize)]
struct AuditBundle {
    schema_version: u16,
    generated_by: &'static str,
    client_build: String,
    policy: AuditPolicy,
    sessions: Vec<SessionReport>,
}

#[derive(Debug, Serialize)]
struct AuditPolicy {
    runtime_use: &'static str,
    snapshot_semantics: &'static str,
    missing_attribute_semantics: &'static str,
    unresolved_evidence_is_hidden: bool,
}

#[derive(Debug, Serialize)]
struct SessionReport {
    rlog: String,
    session_id: String,
    snapshot_events: u64,
    delta_events: u64,
    unknown_update_events: u64,
    damage_source_actors: usize,
    damage_source_actors_with_snapshot: usize,
    damage_source_actors_without_snapshot: usize,
    snapshot_attribute_count_distribution: BTreeMap<usize, u64>,
    fight_attributes: Vec<AttributeReport>,
    actor_examples: Vec<ActorReport>,
    selected_actor_examples: Vec<ActorReport>,
}

#[derive(Debug, Serialize)]
struct AttributeReport {
    attribute_id: i32,
    internal_name: Option<String>,
    damage_source_actors: usize,
    snapshot_present: usize,
    snapshot_missing: usize,
    snapshot_explicit_zero: usize,
    snapshot_nonzero: usize,
    observed_in_later_delta: usize,
    value_examples: Vec<i64>,
}

#[derive(Debug, Serialize)]
struct ActorReport {
    run_ordinal: u32,
    entity_uuid: i64,
    damage_events: u64,
    first_snapshot_sequence: Option<u64>,
    snapshot_attribute_count: usize,
    fight_attribute_count: usize,
    missing_fight_attribute_count: usize,
    later_delta_fight_attribute_count: usize,
    snapshot_fight_values: BTreeMap<i32, i64>,
    undecoded_snapshot_fight_attribute_ids: Vec<i32>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("attribute snapshot audit failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = arguments()?;
    let catalog: Catalog = serde_json::from_reader(BufReader::new(File::open(&args.catalog)?))?;
    let catalog_names = catalog
        .attributes
        .into_iter()
        .map(|attribute| (attribute.id, attribute.internal_name))
        .collect::<BTreeMap<_, _>>();
    let fight_attribute_ids = catalog_names.keys().copied().collect::<BTreeSet<_>>();
    let sessions = args
        .rlogs
        .iter()
        .map(|path| read_session(path, &fight_attribute_ids, &catalog_names, &args.entities))
        .collect::<Result<Vec<_>, _>>()?;
    let bundle = AuditBundle {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-attribute-snapshot-audit",
        client_build: catalog.client_build,
        policy: AuditPolicy {
            runtime_use: "offline_research_only_never_loaded_by_capture_or_live_meter",
            snapshot_semantics: "snapshot means the attribute collection arrived on EnterScene or SyncNearEntities actor appearance; delta means SyncNearDelta or SyncToMeDelta",
            missing_attribute_semantics: "missing remains unknown until packet or client semantics prove that an appearance snapshot materializes omitted fight attributes as zero",
            unresolved_evidence_is_hidden: false,
        },
        sessions,
    };
    let mut writer = BufWriter::new(File::create(args.output)?);
    serde_json::to_writer_pretty(&mut writer, &bundle)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn read_session(
    path: &Path,
    fight_attribute_ids: &BTreeSet<i32>,
    catalog_names: &BTreeMap<i32, Option<String>>,
    selected_entities: &BTreeSet<i64>,
) -> Result<SessionReport, Box<dyn std::error::Error>> {
    let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
    let mut session_id = None::<String>;
    let mut run_ordinal = 0_u32;
    let mut actors = BTreeMap::<(u32, i64), ActorState>::new();
    let mut snapshot_events = 0_u64;
    let mut delta_events = 0_u64;
    let mut unknown_update_events = 0_u64;

    while let Some(envelope) = reader.next_event()? {
        session_id.get_or_insert_with(|| envelope.session_id.clone());
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            continue;
        };
        match &timeline.kind {
            TimelineEventKind::RunBoundary { state, .. } => match state {
                RunState::Entered => run_ordinal = run_ordinal.saturating_add(1),
                RunState::Started if run_ordinal == 0 => run_ordinal = 1,
                _ => {}
            },
            TimelineEventKind::EntityAttributes(event) => {
                let state = actors
                    .entry((run_ordinal, event.actor.entity_uuid.0))
                    .or_default();
                match event.update_kind {
                    EntityAttributeUpdateKind::Snapshot => {
                        snapshot_events = snapshot_events.saturating_add(1);
                        if state.first_snapshot_sequence.is_none() {
                            state.first_snapshot_sequence = Some(envelope.sequence);
                            state.snapshot_values = event
                                .attributes
                                .iter()
                                .map(|attribute| {
                                    (attribute.attribute_id, decode_attribute(attribute))
                                })
                                .collect();
                        }
                    }
                    EntityAttributeUpdateKind::Delta => {
                        delta_events = delta_events.saturating_add(1);
                        state.delta_ids.extend(
                            event
                                .attributes
                                .iter()
                                .map(|attribute| attribute.attribute_id),
                        );
                    }
                    EntityAttributeUpdateKind::Unknown => {
                        unknown_update_events = unknown_update_events.saturating_add(1)
                    }
                }
            }
            TimelineEventKind::Damage(damage) => {
                let state = actors
                    .entry((run_ordinal, damage.source.entity_uuid.0))
                    .or_default();
                state.damage_events = state.damage_events.saturating_add(1);
            }
            _ => {}
        }
    }

    let damaged_actors = actors
        .iter()
        .filter(|(_, state)| state.damage_events > 0)
        .collect::<Vec<_>>();
    let mut distribution = BTreeMap::<usize, u64>::new();
    for (_, state) in &damaged_actors {
        if state.first_snapshot_sequence.is_some() {
            *distribution.entry(state.snapshot_values.len()).or_default() += 1;
        }
    }
    let fight_attributes = catalog_names
        .iter()
        .map(|(attribute_id, internal_name)| {
            let mut present = 0_usize;
            let mut missing = 0_usize;
            let mut explicit_zero = 0_usize;
            let mut nonzero = 0_usize;
            let mut later_delta = 0_usize;
            let mut values = BTreeSet::new();
            for (_, state) in &damaged_actors {
                if state.delta_ids.contains(attribute_id) {
                    later_delta += 1;
                }
                match state.snapshot_values.get(attribute_id) {
                    Some(value) => {
                        present += 1;
                        if let Some(value) = value {
                            values.insert(*value);
                            if *value == 0 {
                                explicit_zero += 1;
                            } else {
                                nonzero += 1;
                            }
                        }
                    }
                    None => missing += 1,
                }
            }
            AttributeReport {
                attribute_id: *attribute_id,
                internal_name: internal_name.clone(),
                damage_source_actors: damaged_actors.len(),
                snapshot_present: present,
                snapshot_missing: missing,
                snapshot_explicit_zero: explicit_zero,
                snapshot_nonzero: nonzero,
                observed_in_later_delta: later_delta,
                value_examples: values.into_iter().take(12).collect(),
            }
        })
        .collect();
    let actor_report = |(run_ordinal, entity_uuid): &(u32, i64), state: &ActorState| {
        let fight_attribute_count = state
            .snapshot_values
            .keys()
            .filter(|id| fight_attribute_ids.contains(id))
            .count();
        ActorReport {
            run_ordinal: *run_ordinal,
            entity_uuid: *entity_uuid,
            damage_events: state.damage_events,
            first_snapshot_sequence: state.first_snapshot_sequence,
            snapshot_attribute_count: state.snapshot_values.len(),
            fight_attribute_count,
            missing_fight_attribute_count: fight_attribute_ids
                .len()
                .saturating_sub(fight_attribute_count),
            later_delta_fight_attribute_count: state
                .delta_ids
                .iter()
                .filter(|id| fight_attribute_ids.contains(id))
                .count(),
            snapshot_fight_values: state
                .snapshot_values
                .iter()
                .filter_map(|(id, value)| {
                    fight_attribute_ids
                        .contains(id)
                        .then(|| value.map(|value| (*id, value)))
                        .flatten()
                })
                .collect(),
            undecoded_snapshot_fight_attribute_ids: state
                .snapshot_values
                .iter()
                .filter_map(|(id, value)| {
                    (fight_attribute_ids.contains(id) && value.is_none()).then_some(*id)
                })
                .collect(),
        }
    };
    let mut actor_examples = damaged_actors
        .iter()
        .map(|(key, state)| actor_report(key, state))
        .collect::<Vec<_>>();
    actor_examples.sort_by_key(|actor| {
        (
            std::cmp::Reverse(actor.snapshot_attribute_count),
            std::cmp::Reverse(actor.damage_events),
            actor.entity_uuid,
        )
    });
    actor_examples.truncate(32);
    let mut selected_actor_examples = actors
        .iter()
        .filter(|((_, entity_uuid), _)| selected_entities.contains(entity_uuid))
        .map(|(key, state)| actor_report(key, state))
        .collect::<Vec<_>>();
    selected_actor_examples.sort_by_key(|actor| (actor.run_ordinal, actor.entity_uuid));
    let with_snapshot = damaged_actors
        .iter()
        .filter(|(_, state)| state.first_snapshot_sequence.is_some())
        .count();
    Ok(SessionReport {
        rlog: path.display().to_string(),
        session_id: session_id.unwrap_or_default(),
        snapshot_events,
        delta_events,
        unknown_update_events,
        damage_source_actors: damaged_actors.len(),
        damage_source_actors_with_snapshot: with_snapshot,
        damage_source_actors_without_snapshot: damaged_actors.len().saturating_sub(with_snapshot),
        snapshot_attribute_count_distribution: distribution,
        fight_attributes,
        actor_examples,
        selected_actor_examples,
    })
}

fn decode_attribute(attribute: &EntityAttribute) -> Option<i64> {
    let decoded = attribute.decoded.clone().or_else(|| {
        decode_known_entity_attribute_value(attribute.attribute_id, &attribute.raw_value)
    });
    match decoded {
        Some(EntityAttributeValue::Integer(value)) => Some(value),
        Some(_) => None,
        None => decode_varint(&attribute.raw_value).and_then(|value| i64::try_from(value).ok()),
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
            return Some(value);
        }
    }
    None
}

fn arguments() -> Result<Arguments, String> {
    let mut values = env::args_os().skip(1).collect::<Vec<_>>();
    let catalog = PathBuf::from(take_value(&mut values, "--catalog")?);
    let output = PathBuf::from(take_value(&mut values, "--output")?);
    let mut rlogs = Vec::new();
    while let Some(position) = values.iter().position(|value| value == "--rlog") {
        if position + 1 >= values.len() {
            return Err("--rlog requires a value".to_owned());
        }
        rlogs.push(PathBuf::from(values.remove(position + 1)));
        values.remove(position);
    }
    let mut entities = BTreeSet::new();
    while let Some(position) = values.iter().position(|value| value == "--entity") {
        if position + 1 >= values.len() {
            return Err("--entity requires a value".to_owned());
        }
        let raw = values.remove(position + 1);
        values.remove(position);
        let raw = raw
            .to_str()
            .ok_or_else(|| "--entity must be valid UTF-8".to_owned())?;
        entities.insert(
            raw.parse::<i64>()
                .map_err(|_| format!("invalid --entity value: {raw}"))?,
        );
    }
    if rlogs.is_empty() || !values.is_empty() {
        return Err(usage());
    }
    Ok(Arguments {
        catalog,
        rlogs,
        output,
        entities,
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
    "usage: rlogs-bpsr-attribute-snapshot-audit --catalog <fight-attributes.json> --rlog <sealed.rlog> [--rlog <sealed.rlog> ...] --output <report.json> [--entity <entity-uuid> ...]".to_owned()
}
