use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsString,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{
    CanonicalEvent, EntityAttributeUpdateKind, EntityAttributeValue, EntityRef, RunState,
    TimelineEventKind,
};
use rlogs_game_bpsr::decode_known_entity_attribute_value;
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u16 = 1;
const SAMPLE_LIMIT: usize = 32;

#[derive(Debug)]
struct Arguments {
    catalog: PathBuf,
    rlogs: Vec<PathBuf>,
    output: PathBuf,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum AttributeLane {
    Entity,
    Temporary,
}

#[derive(Debug, Clone)]
struct Observation {
    sequence: u64,
    observed_micros: u64,
    run_ordinal: u32,
    actor: EntityRef,
    lane: AttributeLane,
    update_kind: EntityAttributeUpdateKind,
    attribute_id: i32,
    value: Option<i64>,
}

#[derive(Debug, Clone, Copy)]
struct DamageObservation {
    sequence: u64,
    observed_micros: u64,
    run_ordinal: u32,
    actor: EntityRef,
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
    identity_policy: &'static str,
    ordering_policy: &'static str,
    unresolved_evidence_is_hidden: bool,
}

#[derive(Debug, Serialize)]
struct SessionReport {
    rlog: String,
    session_id: String,
    selected_attribute_ids: Vec<i32>,
    attribute_observations: usize,
    damage_source_actors: usize,
    exact_actor_before_damage: usize,
    same_entity_alias_before_damage: usize,
    observed_only_after_damage: usize,
    undecoded_before_damage: usize,
    never_observed: usize,
    actor_formula_states: Vec<ActorFormulaState>,
    alias_groups: Vec<AliasGroup>,
    observation_samples: Vec<ObservationSample>,
}

#[derive(Debug, Serialize)]
struct ActorFormulaState {
    run_ordinal: u32,
    actor_id: u64,
    entity_uuid: i64,
    damage_events: u64,
    first_damage_sequence: u64,
    first_damage_observed_micros: u64,
    attributes: Vec<AttributeState>,
}

#[derive(Debug, Serialize)]
struct AttributeState {
    attribute_id: i32,
    internal_name: Option<String>,
    state: &'static str,
    lane: Option<AttributeLane>,
    value: Option<i64>,
    observation_sequence: Option<u64>,
    observation_actor_id: Option<u64>,
    observation_entity_uuid: Option<i64>,
}

#[derive(Debug, Serialize)]
struct AliasGroup {
    run_ordinal: u32,
    entity_uuid: i64,
    actor_ids: Vec<u64>,
    selected_attribute_ids: Vec<i32>,
    damage_actor_ids: Vec<u64>,
}

#[derive(Debug, Serialize)]
struct ObservationSample {
    sequence: u64,
    observed_micros: u64,
    run_ordinal: u32,
    actor_id: u64,
    entity_uuid: i64,
    lane: AttributeLane,
    update_kind: EntityAttributeUpdateKind,
    attribute_id: i32,
    value: Option<i64>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("formula input state audit failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = arguments()?;
    let catalog: Catalog = serde_json::from_reader(BufReader::new(File::open(&args.catalog)?))?;
    let attributes = catalog
        .attributes
        .into_iter()
        .map(|attribute| (attribute.id, attribute.internal_name))
        .collect::<BTreeMap<_, _>>();
    let sessions = args
        .rlogs
        .iter()
        .map(|path| read_session(path, &attributes))
        .collect::<Result<Vec<_>, _>>()?;
    let bundle = AuditBundle {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-formula-input-state-audit",
        client_build: catalog.client_build,
        policy: AuditPolicy {
            runtime_use: "offline_research_only_never_loaded_by_capture_history_or_live_meter",
            identity_policy: "exact_actor_id_is_primary; same_entity_uuid_other_actor_id_is_reported_as_alias_evidence_but_never_silently_substituted",
            ordering_policy: "only_attribute_values_observed_before_the_first_damage_event_can_satisfy_a_formula_input_snapshot",
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
    attributes: &BTreeMap<i32, Option<String>>,
) -> Result<SessionReport, Box<dyn std::error::Error>> {
    let selected_ids = attributes.keys().copied().collect::<BTreeSet<_>>();
    let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
    let mut session_id = String::new();
    let mut run_ordinal = 0_u32;
    let mut observations = Vec::<Observation>::new();
    let mut damage = Vec::<DamageObservation>::new();

    while let Some(envelope) = reader.next_event()? {
        if session_id.is_empty() {
            session_id = envelope.session_id.clone();
        }
        let CanonicalEvent::Timeline(timeline) = envelope.event else {
            continue;
        };
        match timeline.kind {
            TimelineEventKind::RunBoundary { state, .. } => match state {
                RunState::Entered => run_ordinal = run_ordinal.saturating_add(1),
                RunState::Started if run_ordinal == 0 => run_ordinal = 1,
                _ => {}
            },
            TimelineEventKind::EntityAttributes(event) => {
                for attribute in event.attributes {
                    if !selected_ids.contains(&attribute.attribute_id) {
                        continue;
                    }
                    let decoded = attribute.decoded.or_else(|| {
                        decode_known_entity_attribute_value(
                            attribute.attribute_id,
                            &attribute.raw_value,
                        )
                    });
                    let value = match decoded {
                        Some(EntityAttributeValue::Integer(value)) => Some(value),
                        _ => None,
                    };
                    observations.push(Observation {
                        sequence: envelope.sequence,
                        observed_micros: envelope.time.observed_micros,
                        run_ordinal,
                        actor: event.actor,
                        lane: AttributeLane::Entity,
                        update_kind: event.update_kind,
                        attribute_id: attribute.attribute_id,
                        value,
                    });
                }
            }
            TimelineEventKind::TemporaryAttributes(event) => {
                for attribute in event.attributes {
                    if !selected_ids.contains(&attribute.id) {
                        continue;
                    }
                    observations.push(Observation {
                        sequence: envelope.sequence,
                        observed_micros: envelope.time.observed_micros,
                        run_ordinal,
                        actor: event.actor,
                        lane: AttributeLane::Temporary,
                        update_kind: event.update_kind,
                        attribute_id: attribute.id,
                        value: Some(i64::from(attribute.value)),
                    });
                }
            }
            TimelineEventKind::Damage(event) => damage.push(DamageObservation {
                sequence: envelope.sequence,
                observed_micros: envelope.time.observed_micros,
                run_ordinal,
                actor: event.source,
            }),
            _ => {}
        }
    }

    let mut damage_actors = BTreeMap::<(u32, u64, i64), (u64, u64, u64)>::new();
    for event in damage {
        let state = damage_actors
            .entry((
                event.run_ordinal,
                event.actor.actor_id.0,
                event.actor.entity_uuid.0,
            ))
            .or_insert((0, event.sequence, event.observed_micros));
        state.0 = state.0.saturating_add(1);
        if event.sequence < state.1 {
            state.1 = event.sequence;
            state.2 = event.observed_micros;
        }
    }

    let mut exact_actor_before_damage = 0_usize;
    let mut same_entity_alias_before_damage = 0_usize;
    let mut observed_only_after_damage = 0_usize;
    let mut undecoded_before_damage = 0_usize;
    let mut never_observed = 0_usize;
    let mut actor_formula_states = Vec::new();

    for ((run, actor_id, entity_uuid), (damage_events, first_damage, first_micros)) in
        &damage_actors
    {
        let mut states = Vec::new();
        for (attribute_id, internal_name) in attributes {
            let same_actor_before = latest_observation(
                &observations,
                *run,
                *attribute_id,
                *first_damage,
                |observation| observation.actor.actor_id.0 == *actor_id,
            );
            let same_entity_before = latest_observation(
                &observations,
                *run,
                *attribute_id,
                *first_damage,
                |observation| observation.actor.entity_uuid.0 == *entity_uuid,
            );
            let same_entity_after = observations
                .iter()
                .filter(|observation| {
                    observation.run_ordinal == *run
                        && observation.attribute_id == *attribute_id
                        && observation.actor.entity_uuid.0 == *entity_uuid
                        && observation.sequence >= *first_damage
                })
                .min_by_key(|observation| observation.sequence);

            let (state, observation) = if let Some(observation) =
                same_actor_before.filter(|observation| observation.value.is_some())
            {
                exact_actor_before_damage += 1;
                ("exact_actor_before_damage", Some(observation))
            } else if let Some(observation) =
                same_entity_before.filter(|observation| observation.value.is_some())
            {
                same_entity_alias_before_damage += 1;
                ("same_entity_alias_before_damage", Some(observation))
            } else if same_actor_before.is_some() || same_entity_before.is_some() {
                undecoded_before_damage += 1;
                (
                    "undecoded_before_damage",
                    same_actor_before.or(same_entity_before),
                )
            } else if let Some(observation) = same_entity_after {
                observed_only_after_damage += 1;
                ("observed_only_after_damage", Some(observation))
            } else {
                never_observed += 1;
                ("never_observed", None)
            };
            states.push(AttributeState {
                attribute_id: *attribute_id,
                internal_name: internal_name.clone(),
                state,
                lane: observation.map(|observation| observation.lane),
                value: observation.and_then(|observation| observation.value),
                observation_sequence: observation.map(|observation| observation.sequence),
                observation_actor_id: observation.map(|observation| observation.actor.actor_id.0),
                observation_entity_uuid: observation
                    .map(|observation| observation.actor.entity_uuid.0),
            });
        }
        actor_formula_states.push(ActorFormulaState {
            run_ordinal: *run,
            actor_id: *actor_id,
            entity_uuid: *entity_uuid,
            damage_events: *damage_events,
            first_damage_sequence: *first_damage,
            first_damage_observed_micros: *first_micros,
            attributes: states,
        });
    }

    let mut grouped_actor_ids = BTreeMap::<(u32, i64), BTreeSet<u64>>::new();
    let mut grouped_attribute_ids = BTreeMap::<(u32, i64), BTreeSet<i32>>::new();
    for observation in &observations {
        grouped_actor_ids
            .entry((observation.run_ordinal, observation.actor.entity_uuid.0))
            .or_default()
            .insert(observation.actor.actor_id.0);
        grouped_attribute_ids
            .entry((observation.run_ordinal, observation.actor.entity_uuid.0))
            .or_default()
            .insert(observation.attribute_id);
    }
    let alias_groups = grouped_actor_ids
        .into_iter()
        .filter(|(_, actor_ids)| actor_ids.len() > 1)
        .map(|((run, entity_uuid), actor_ids)| AliasGroup {
            run_ordinal: run,
            entity_uuid,
            actor_ids: actor_ids.into_iter().collect(),
            selected_attribute_ids: grouped_attribute_ids
                .remove(&(run, entity_uuid))
                .unwrap_or_default()
                .into_iter()
                .collect(),
            damage_actor_ids: damage_actors
                .keys()
                .filter_map(|(damage_run, actor_id, damage_entity_uuid)| {
                    (*damage_run == run && *damage_entity_uuid == entity_uuid).then_some(*actor_id)
                })
                .collect(),
        })
        .collect();
    let observation_samples = observations
        .iter()
        .take(SAMPLE_LIMIT)
        .map(|observation| ObservationSample {
            sequence: observation.sequence,
            observed_micros: observation.observed_micros,
            run_ordinal: observation.run_ordinal,
            actor_id: observation.actor.actor_id.0,
            entity_uuid: observation.actor.entity_uuid.0,
            lane: observation.lane,
            update_kind: observation.update_kind,
            attribute_id: observation.attribute_id,
            value: observation.value,
        })
        .collect();

    Ok(SessionReport {
        rlog: path.display().to_string(),
        session_id,
        selected_attribute_ids: selected_ids.into_iter().collect(),
        attribute_observations: observations.len(),
        damage_source_actors: damage_actors.len(),
        exact_actor_before_damage,
        same_entity_alias_before_damage,
        observed_only_after_damage,
        undecoded_before_damage,
        never_observed,
        actor_formula_states,
        alias_groups,
        observation_samples,
    })
}

fn latest_observation<F>(
    observations: &[Observation],
    run_ordinal: u32,
    attribute_id: i32,
    before_sequence: u64,
    predicate: F,
) -> Option<&Observation>
where
    F: Fn(&Observation) -> bool,
{
    observations
        .iter()
        .filter(|observation| {
            observation.run_ordinal == run_ordinal
                && observation.attribute_id == attribute_id
                && observation.sequence < before_sequence
                && predicate(observation)
        })
        .max_by_key(|observation| observation.sequence)
}

fn arguments() -> Result<Arguments, String> {
    arguments_from(env::args_os().skip(1).collect())
}

fn arguments_from(mut values: Vec<OsString>) -> Result<Arguments, String> {
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
    if rlogs.is_empty() {
        return Err("at least one --rlog is required".to_owned());
    }
    if !values.is_empty() {
        return Err(format!(
            "unexpected argument: {}",
            values[0].to_string_lossy()
        ));
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
        .ok_or_else(|| format!("missing {flag}"))?;
    if position + 1 >= values.len() {
        return Err(format!("{flag} requires a value"));
    }
    let value = values.remove(position + 1);
    values.remove(position);
    Ok(value)
}
