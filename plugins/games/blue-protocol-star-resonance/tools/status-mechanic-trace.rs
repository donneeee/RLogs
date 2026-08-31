use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsString,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::PathBuf,
};

use rlogs_events::{
    ActorEvent, CanonicalEvent, EntityRef, EventEnvelope, EventProvenance, StatusEvent,
    TimelineEventKind,
};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::Serialize;

const SCHEMA_VERSION: u16 = 2;
const DEFAULT_WINDOW_MICROS: u64 = 2_000_000;

#[derive(Debug, Serialize)]
struct TraceBundle {
    schema_version: u16,
    policy: &'static str,
    effect_ids: Vec<i64>,
    context_window_micros: u64,
    reports: Vec<TraceReport>,
}

#[derive(Debug, Serialize)]
struct TraceReport {
    session_id: String,
    actors: BTreeMap<i64, ActorEvent>,
    owner_by_direct_entity: BTreeMap<i64, i64>,
    owned_entities: Vec<i64>,
    owned_entity_event_sequences: Vec<u64>,
    context_events: Vec<ContextEvent>,
    occurrences: Vec<TraceOccurrence>,
}

#[derive(Debug, Serialize)]
struct TraceOccurrence {
    effect_id: i64,
    observed_micros: u64,
    sequence: u64,
    timeline_sequence: u64,
    provenance: EventProvenance,
    source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    status: StatusEvent,
    context: Vec<ContextReference>,
}

#[derive(Debug, Serialize)]
struct ContextEvent {
    sequence: u64,
    timeline_sequence: u64,
    observed_micros: u64,
    provenance: EventProvenance,
    kind: TimelineEventKind,
}

#[derive(Debug, Serialize)]
struct ContextReference {
    sequence: u64,
    relative_micros: i64,
    same_provenance_source: bool,
    relationship: Vec<&'static str>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("status mechanic trace failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments()?;
    let mut reports = Vec::with_capacity(arguments.rlogs.len());
    for path in &arguments.rlogs {
        reports.push(trace_file(
            path,
            &arguments.effect_ids,
            arguments.window_micros,
        )?);
    }
    let bundle = TraceBundle {
        schema_version: SCHEMA_VERSION,
        policy: "packet_exact_context_no_temporal_source_guessing",
        effect_ids: arguments.effect_ids.iter().copied().collect(),
        context_window_micros: arguments.window_micros,
        reports,
    };
    let mut writer = BufWriter::new(File::create(arguments.output)?);
    serde_json::to_writer_pretty(&mut writer, &bundle)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn trace_file(
    path: &PathBuf,
    effect_ids: &BTreeSet<i64>,
    window_micros: u64,
) -> Result<TraceReport, Box<dyn std::error::Error>> {
    let mut occurrences = Vec::new();
    let mut actors = BTreeMap::new();
    let mut owner_by_direct_entity = BTreeMap::new();
    let mut session_id = String::new();

    let mut reader = open_reader(path)?;
    while let Some(envelope) = reader.next_event()? {
        session_id.clone_from(&envelope.session_id);
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            continue;
        };
        match &timeline.kind {
            TimelineEventKind::Actor(actor) => {
                actors.insert(actor.actor.entity_uuid.0, actor.clone());
            }
            TimelineEventKind::Damage(damage) => observe_owner(
                &mut owner_by_direct_entity,
                damage.source.entity_uuid.0,
                damage.direct_source,
            ),
            TimelineEventKind::Healing(healing) => observe_owner(
                &mut owner_by_direct_entity,
                healing.source.entity_uuid.0,
                healing.direct_source,
            ),
            TimelineEventKind::Status(status) if effect_ids.contains(&status.effect.0) => {
                occurrences.push(TraceOccurrence {
                    effect_id: status.effect.0,
                    observed_micros: envelope.time.observed_micros,
                    sequence: envelope.sequence,
                    timeline_sequence: timeline.sequence,
                    provenance: envelope.provenance.clone(),
                    source_entity_uuid: status.source.map(|source| source.entity_uuid.0),
                    target_entity_uuid: status.target.entity_uuid.0,
                    status: status.clone(),
                    context: Vec::new(),
                });
            }
            _ => {}
        }
    }
    if reader.summary().is_none() {
        return Err(format!("{} is not a sealed canonical rlog", path.display()).into());
    }

    let provider_owners = occurrences
        .iter()
        .filter_map(|occurrence| occurrence.source_entity_uuid)
        .map(|source| {
            owner_by_direct_entity
                .get(&source)
                .copied()
                .unwrap_or(source)
        })
        .collect::<BTreeSet<_>>();
    occurrences.sort_by_key(|occurrence| (occurrence.observed_micros, occurrence.sequence));
    let owned_entities = owner_by_direct_entity
        .iter()
        .filter_map(|(direct, owner)| provider_owners.contains(owner).then_some(*direct))
        .collect::<BTreeSet<_>>();

    let mut context_events = BTreeMap::new();
    let mut owned_entity_event_sequences = BTreeSet::new();
    let mut reader = open_reader(path)?;
    while let Some(envelope) = reader.next_event()? {
        let Some(entities) = timeline_entities(&envelope) else {
            continue;
        };
        let observed_micros = envelope.time.observed_micros;
        if entities
            .iter()
            .any(|entity| owned_entities.contains(entity))
        {
            owned_entity_event_sequences.insert(envelope.sequence);
            let CanonicalEvent::Timeline(timeline) = &envelope.event else {
                unreachable!("timeline_entities only returns timeline events");
            };
            context_events
                .entry(envelope.sequence)
                .or_insert_with(|| ContextEvent {
                    sequence: envelope.sequence,
                    timeline_sequence: timeline.sequence,
                    observed_micros,
                    provenance: envelope.provenance.clone(),
                    kind: timeline.kind.clone(),
                });
        }
        let window_start = observed_micros.saturating_sub(window_micros);
        let window_end = observed_micros.saturating_add(window_micros);
        let first_nearby =
            occurrences.partition_point(|occurrence| occurrence.observed_micros < window_start);
        let after_nearby =
            occurrences.partition_point(|occurrence| occurrence.observed_micros <= window_end);
        for occurrence in &mut occurrences[first_nearby..after_nearby] {
            if envelope.sequence == occurrence.sequence {
                continue;
            }
            let provider = occurrence.source_entity_uuid;
            let provider_owner = provider.and_then(|source| {
                owner_by_direct_entity
                    .get(&source)
                    .copied()
                    .or(Some(source))
            });
            let relationships = relationships(
                &entities,
                provider,
                provider_owner,
                occurrence.target_entity_uuid,
                &owner_by_direct_entity,
            );
            if relationships.is_empty() {
                continue;
            }
            occurrence.context.push(ContextReference {
                sequence: envelope.sequence,
                relative_micros: signed_delta(observed_micros, occurrence.observed_micros),
                same_provenance_source: envelope.provenance.source == occurrence.provenance.source,
                relationship: relationships,
            });
            let CanonicalEvent::Timeline(timeline) = &envelope.event else {
                unreachable!("timeline_entities only returns timeline events");
            };
            context_events
                .entry(envelope.sequence)
                .or_insert_with(|| ContextEvent {
                    sequence: envelope.sequence,
                    timeline_sequence: timeline.sequence,
                    observed_micros,
                    provenance: envelope.provenance.clone(),
                    kind: timeline.kind.clone(),
                });
        }
    }
    if reader.summary().is_none() {
        return Err(format!("{} is not a sealed canonical rlog", path.display()).into());
    }
    for occurrence in &mut occurrences {
        occurrence.context.sort_by_key(|event| event.sequence);
    }
    occurrences.sort_by_key(|occurrence| occurrence.sequence);

    let mut relevant_entities = BTreeSet::new();
    for occurrence in &occurrences {
        relevant_entities.insert(occurrence.target_entity_uuid);
        if let Some(source) = occurrence.source_entity_uuid {
            relevant_entities.insert(source);
            if let Some(owner) = owner_by_direct_entity.get(&source) {
                relevant_entities.insert(*owner);
            }
        }
    }
    relevant_entities.extend(owned_entities.iter().copied());
    owner_by_direct_entity.retain(|direct, owner| {
        relevant_entities.contains(direct) || relevant_entities.contains(owner)
    });
    actors.retain(|entity, _| relevant_entities.contains(entity));

    Ok(TraceReport {
        session_id,
        actors,
        owner_by_direct_entity,
        owned_entities: owned_entities.into_iter().collect(),
        owned_entity_event_sequences: owned_entity_event_sequences.into_iter().collect(),
        context_events: context_events.into_values().collect(),
        occurrences,
    })
}

fn open_reader(path: &PathBuf) -> Result<RlogReader<BufReader<File>>, Box<dyn std::error::Error>> {
    Ok(RlogReader::new(
        BufReader::new(File::open(path)?),
        RlogLimits::default(),
    )?)
}

fn observe_owner(owners: &mut BTreeMap<i64, i64>, owner: i64, direct_source: Option<EntityRef>) {
    if let Some(direct) = direct_source
        .map(|source| source.entity_uuid.0)
        .filter(|direct| *direct != owner)
    {
        owners.entry(direct).or_insert(owner);
    }
}

fn timeline_entities(envelope: &EventEnvelope) -> Option<BTreeSet<i64>> {
    let CanonicalEvent::Timeline(timeline) = &envelope.event else {
        return None;
    };
    let mut entities = BTreeSet::new();
    match &timeline.kind {
        TimelineEventKind::Actor(_) => return None,
        TimelineEventKind::EntityAttributes(event) => insert(&mut entities, event.actor),
        TimelineEventKind::TemporaryAttributes(event) => insert(&mut entities, event.actor),
        TimelineEventKind::Cast(event) => {
            insert(&mut entities, event.source);
            if let Some(target) = event.target {
                insert(&mut entities, target);
            }
        }
        TimelineEventKind::Cooldown(event) => insert(&mut entities, event.actor),
        TimelineEventKind::Resource(event) => insert(&mut entities, event.actor),
        TimelineEventKind::Damage(event) => {
            insert(&mut entities, event.source);
            if let Some(direct) = event.direct_source {
                insert(&mut entities, direct);
            }
            insert(&mut entities, event.target);
        }
        TimelineEventKind::Healing(event) => {
            insert(&mut entities, event.source);
            if let Some(direct) = event.direct_source {
                insert(&mut entities, direct);
            }
            insert(&mut entities, event.target);
        }
        TimelineEventKind::Shield(event) => {
            insert(&mut entities, event.source);
            insert(&mut entities, event.target);
        }
        TimelineEventKind::Life { actor, .. } => insert(&mut entities, *actor),
        TimelineEventKind::Status(event) => {
            if let Some(source) = event.source {
                insert(&mut entities, source);
            }
            insert(&mut entities, event.target);
        }
        TimelineEventKind::UnresolvedAction(event) => {
            if let Some(container) = event.container {
                insert(&mut entities, container);
            }
            if let Some(target) = event.target {
                insert(&mut entities, target);
            }
        }
        TimelineEventKind::Position(_)
        | TimelineEventKind::RunBoundary { .. }
        | TimelineEventKind::EncounterBoundary { .. }
        | TimelineEventKind::CombatBoundary { .. }
        | TimelineEventKind::RecorderPause(_)
        | TimelineEventKind::UnresolvedStatus(_)
        | TimelineEventKind::DataGap(_) => return None,
    }
    Some(entities)
}

fn insert(entities: &mut BTreeSet<i64>, entity: EntityRef) {
    entities.insert(entity.entity_uuid.0);
}

fn relationships(
    event_entities: &BTreeSet<i64>,
    provider: Option<i64>,
    provider_owner: Option<i64>,
    target: i64,
    owner_by_direct_entity: &BTreeMap<i64, i64>,
) -> Vec<&'static str> {
    let mut result = Vec::new();
    if provider.is_some_and(|value| event_entities.contains(&value)) {
        result.push("provider");
    }
    if provider_owner.is_some_and(|value| event_entities.contains(&value)) {
        result.push("provider_owner");
    }
    if event_entities.contains(&target) {
        result.push("recipient");
    }
    if event_entities.iter().any(|entity| {
        owner_by_direct_entity
            .get(entity)
            .is_some_and(|owner| Some(*owner) == provider_owner)
    }) {
        result.push("provider_owned_entity");
    }
    result.sort_unstable();
    result.dedup();
    result
}

fn signed_delta(actual: u64, origin: u64) -> i64 {
    if actual >= origin {
        i64::try_from(actual - origin).unwrap_or(i64::MAX)
    } else {
        -i64::try_from(origin - actual).unwrap_or(i64::MAX)
    }
}

#[derive(Debug)]
struct Arguments {
    rlogs: Vec<PathBuf>,
    output: PathBuf,
    effect_ids: BTreeSet<i64>,
    window_micros: u64,
}

fn arguments() -> Result<Arguments, String> {
    let mut values = env::args_os().skip(1).collect::<Vec<_>>();
    if values
        .iter()
        .any(|value| value == "--help" || value == "-h")
    {
        return Err(usage());
    }
    let output = PathBuf::from(take_value(&mut values, "--output")?);
    let window_micros = take_optional_value(&mut values, "--window-micros")
        .map(|value| {
            value
                .to_string_lossy()
                .parse::<u64>()
                .map_err(|_| "--window-micros must be an unsigned integer".to_owned())
        })
        .transpose()?
        .unwrap_or(DEFAULT_WINDOW_MICROS);
    let mut effect_ids = BTreeSet::new();
    while let Some(position) = values.iter().position(|value| value == "--effect") {
        if position + 1 >= values.len() {
            return Err("--effect requires a numeric status effect ID".into());
        }
        values.remove(position);
        let value = values.remove(position);
        let effect_id = value
            .to_string_lossy()
            .parse::<i64>()
            .map_err(|_| "--effect requires a numeric status effect ID".to_owned())?;
        effect_ids.insert(effect_id);
    }
    let mut rlogs = Vec::new();
    while let Some(position) = values.iter().position(|value| value == "--rlog") {
        if position + 1 >= values.len() {
            return Err("--rlog requires a path".into());
        }
        values.remove(position);
        rlogs.push(PathBuf::from(values.remove(position)));
    }
    if effect_ids.is_empty() || rlogs.is_empty() || !values.is_empty() {
        return Err(usage());
    }
    Ok(Arguments {
        rlogs,
        output,
        effect_ids,
        window_micros,
    })
}

fn take_value(values: &mut Vec<OsString>, flag: &str) -> Result<OsString, String> {
    take_optional_value(values, flag).ok_or_else(|| format!("missing {flag}\n{}", usage()))
}

fn take_optional_value(values: &mut Vec<OsString>, flag: &str) -> Option<OsString> {
    let position = values.iter().position(|value| value == flag)?;
    if position + 1 >= values.len() {
        return None;
    }
    values.remove(position);
    Some(values.remove(position))
}

fn usage() -> String {
    "usage: rlogs-bpsr-status-mechanic-trace --effect <id> [--effect <id> ...] --rlog <sealed.rlog> [--rlog <sealed.rlog> ...] --output <trace.json> [--window-micros <micros>]".into()
}
