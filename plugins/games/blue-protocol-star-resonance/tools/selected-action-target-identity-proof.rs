use std::{
    collections::{BTreeMap, HashMap},
    env,
    ffi::OsString,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{ActorKind, ActorState, CanonicalEvent, RunState, TimelineEventKind};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::Serialize;
use serde_json::Value;

const SCHEMA_VERSION: u16 = 2;

#[derive(Debug)]
struct Arguments {
    build: String,
    selected_actions: PathBuf,
    rlogs: Vec<PathBuf>,
    output: PathBuf,
}

#[derive(Debug, Clone)]
struct Request {
    session_id: String,
    sequence: u64,
    run_ordinal: u32,
    source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
}

#[derive(Debug, Clone)]
struct ActorEvidence {
    active: bool,
    actor_sequence: u64,
    entity_type_id: i32,
    kind: ActorKind,
    monster_id: Option<i64>,
    character_id: Option<String>,
    class_id: Option<i32>,
    specialization_id: Option<i32>,
    level: Option<u32>,
    identity_conflict: bool,
}

#[derive(Debug, Serialize)]
struct Bundle {
    schema_version: u16,
    generated_by: &'static str,
    game_build: String,
    policy: Policy,
    selection_source: String,
    summary: Summary,
    missing_action_keys: Vec<String>,
    observations: Vec<Observation>,
}

#[derive(Debug, Serialize)]
struct Policy {
    exact_session_sequence_and_target_join_only: bool,
    actor_identity_is_event_time_state: bool,
    target_allegiance_assumed: bool,
    target_endpoint_is_allegiance_neutral: bool,
    recipient_or_enemy_target_are_both_allowed: bool,
    absent_monster_or_character_identity_zero_filled: bool,
    conflicting_or_despawned_identity_is_unresolved: bool,
    numeric_monster_id_is_exact_build_lookup_key_only: bool,
    localized_name_is_runtime_key: bool,
    static_target_stats_substituted: bool,
    runtime_authority: bool,
    provider_rdps_credit_allowed: bool,
}

#[derive(Debug, Serialize)]
struct Summary {
    requested_actions: usize,
    matched_actions: usize,
    missing_actions: usize,
    observations_with_active_actor_state: usize,
    observations_with_exact_numeric_monster_id: usize,
    observations_with_exact_character_id: usize,
    observations_with_level: usize,
    observations_with_identity_conflict: usize,
    observations_with_active_source_actor_state: usize,
    observations_with_exact_source_numeric_monster_id: usize,
    observations_with_exact_source_character_id: usize,
    observations_with_source_identity_conflict: usize,
    exact_identity_kind_counts: Vec<Count>,
    target_actor_kind_counts: Vec<Count>,
    distinct_numeric_monster_ids: usize,
}

#[derive(Debug, Serialize)]
struct Count {
    value: String,
    count: usize,
}

#[derive(Debug, Serialize)]
struct Observation {
    session_id: String,
    sequence: u64,
    run_ordinal: u32,
    scene_id: Option<i32>,
    source_entity_uuid: Option<i64>,
    source_actor_observation_sequence: Option<u64>,
    source_actor_active: bool,
    source_entity_type_id: Option<i32>,
    source_actor_kind: Option<String>,
    source_numeric_monster_id: Option<i64>,
    source_character_id: Option<String>,
    source_class_id: Option<i32>,
    source_specialization_id: Option<i32>,
    source_level: Option<u32>,
    source_identity_conflict: bool,
    source_exact_identity_kind: &'static str,
    source_unresolved_reasons: Vec<&'static str>,
    target_entity_uuid: i64,
    actor_observation_sequence: Option<u64>,
    actor_active: bool,
    entity_type_id: Option<i32>,
    actor_kind: Option<String>,
    numeric_monster_id: Option<i64>,
    character_id: Option<String>,
    class_id: Option<i32>,
    specialization_id: Option<i32>,
    level: Option<u32>,
    identity_conflict: bool,
    exact_identity_kind: &'static str,
    static_target_stat_join_allowed: bool,
    unresolved_reasons: Vec<&'static str>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args(env::args_os().skip(1))?;
    let requests = read_requests(&args.selected_actions)?;
    let mut observations = BTreeMap::<(String, u64), Observation>::new();
    for path in &args.rlogs {
        read_rlog(path, &args.build, &requests, &mut observations)?;
    }
    let missing_action_keys = requests
        .keys()
        .filter(|key| !observations.contains_key(*key))
        .map(|(session, sequence)| format!("{session}:{sequence}"))
        .collect::<Vec<_>>();
    let observations = observations.into_values().collect::<Vec<_>>();
    let summary = summarize(&requests, &observations, missing_action_keys.len());
    let bundle = Bundle {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-selected-action-target-identity-proof",
        game_build: args.build,
        policy: Policy {
            exact_session_sequence_and_target_join_only: true,
            actor_identity_is_event_time_state: true,
            target_allegiance_assumed: false,
            target_endpoint_is_allegiance_neutral: true,
            recipient_or_enemy_target_are_both_allowed: true,
            absent_monster_or_character_identity_zero_filled: false,
            conflicting_or_despawned_identity_is_unresolved: true,
            numeric_monster_id_is_exact_build_lookup_key_only: true,
            localized_name_is_runtime_key: false,
            static_target_stats_substituted: false,
            runtime_authority: false,
            provider_rdps_credit_allowed: false,
        },
        selection_source: args
            .selected_actions
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or("selected-action input has no UTF-8 file name")?
            .to_owned(),
        summary,
        missing_action_keys,
        observations,
    };
    let mut writer = BufWriter::new(File::create(&args.output)?);
    serde_json::to_writer_pretty(&mut writer, &bundle)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    println!("wrote {}", args.output.display());
    Ok(())
}

fn read_rlog(
    path: &Path,
    build: &str,
    requests: &BTreeMap<(String, u64), Request>,
    observations: &mut BTreeMap<(String, u64), Observation>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
    if reader.header().region.client_build != build {
        return Err(format!(
            "{} has build {}, expected {build}",
            path.display(),
            reader.header().region.client_build
        )
        .into());
    }
    let mut actors = HashMap::<(u32, i64), ActorEvidence>::new();
    let mut run_ordinal = 0_u32;
    let mut scene_id = None;
    while let Some(envelope) = reader.next_event()? {
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            continue;
        };
        match &timeline.kind {
            TimelineEventKind::RunBoundary {
                state,
                scene_id: observed_scene_id,
                ..
            } => {
                match state {
                    RunState::Entered => run_ordinal = run_ordinal.saturating_add(1),
                    RunState::Started if run_ordinal == 0 => run_ordinal = 1,
                    _ => {}
                }
                if let Some(observed_scene_id) = observed_scene_id {
                    scene_id = Some(observed_scene_id.0);
                }
            }
            TimelineEventKind::Actor(actor) => {
                observe_actor(&mut actors, run_ordinal, envelope.sequence, actor)
            }
            TimelineEventKind::Damage(damage) => {
                let key = (envelope.session_id.clone(), envelope.sequence);
                let Some(request) = requests.get(&key) else {
                    continue;
                };
                let source = damage.source.entity_uuid.0;
                let target = damage.target.entity_uuid.0;
                let source_evidence = actors.get(&(run_ordinal, source));
                let target_evidence = actors.get(&(run_ordinal, target));
                observations.insert(
                    key,
                    observation(
                        request,
                        run_ordinal,
                        scene_id,
                        source,
                        source_evidence,
                        target,
                        target_evidence,
                    ),
                );
            }
            _ => {}
        }
    }
    Ok(())
}

fn observe_actor(
    actors: &mut HashMap<(u32, i64), ActorEvidence>,
    run_ordinal: u32,
    sequence: u64,
    actor: &rlogs_events::ActorEvent,
) {
    let key = (run_ordinal, actor.actor.entity_uuid.0);
    if actor.state == ActorState::Despawned {
        if let Some(evidence) = actors.get_mut(&key) {
            evidence.active = false;
            evidence.actor_sequence = sequence;
        }
        return;
    }
    let monster_id = actor.monster_id.map(|value| value.0);
    let fresh = ActorEvidence {
        active: true,
        actor_sequence: sequence,
        entity_type_id: actor.entity_type_id,
        kind: actor.kind,
        monster_id,
        character_id: actor.character_id.clone(),
        class_id: actor.class_id,
        specialization_id: actor.specialization_id,
        level: actor.level,
        identity_conflict: false,
    };
    if actor.state == ActorState::Spawned || !actors.contains_key(&key) {
        actors.insert(key, fresh);
        return;
    }
    let evidence = actors.get_mut(&key).expect("actor exists");
    if evidence.entity_type_id != actor.entity_type_id || evidence.kind != actor.kind {
        evidence.identity_conflict = true;
    }
    if let (Some(previous), Some(current)) = (evidence.monster_id, monster_id) {
        if previous != current {
            evidence.identity_conflict = true;
        }
    }
    if let (Some(previous), Some(current)) = (&evidence.character_id, &actor.character_id) {
        if previous != current {
            evidence.identity_conflict = true;
        }
    }
    if let (Some(previous), Some(current)) = (evidence.class_id, actor.class_id) {
        if previous != current {
            evidence.identity_conflict = true;
        }
    }
    if let (Some(previous), Some(current)) = (evidence.specialization_id, actor.specialization_id) {
        if previous != current {
            evidence.identity_conflict = true;
        }
    }
    evidence.active = true;
    evidence.actor_sequence = sequence;
    evidence.entity_type_id = actor.entity_type_id;
    evidence.kind = actor.kind;
    if monster_id.is_some() {
        evidence.monster_id = monster_id;
    }
    if actor.character_id.is_some() {
        evidence.character_id = actor.character_id.clone();
    }
    if actor.class_id.is_some() {
        evidence.class_id = actor.class_id;
    }
    if actor.specialization_id.is_some() {
        evidence.specialization_id = actor.specialization_id;
    }
    if actor.level.is_some() {
        evidence.level = actor.level;
    }
}

fn observation(
    request: &Request,
    run_ordinal: u32,
    scene_id: Option<i32>,
    source_entity_uuid: i64,
    source_evidence: Option<&ActorEvidence>,
    target_entity_uuid: i64,
    target_evidence: Option<&ActorEvidence>,
) -> Observation {
    let mut unresolved_reasons = Vec::new();
    if request.run_ordinal != run_ordinal {
        unresolved_reasons.push("run-ordinal-mismatch");
    }
    if request.target_entity_uuid != target_entity_uuid {
        unresolved_reasons.push("target-entity-mismatch");
    }
    let active = target_evidence.is_some_and(|value| value.active);
    if target_evidence.is_none() {
        unresolved_reasons.push("event-time-actor-state-absent");
    } else if !active {
        unresolved_reasons.push("latest-actor-state-despawned");
    }
    if target_evidence.is_some_and(|value| value.identity_conflict) {
        unresolved_reasons.push("actor-identity-conflict");
    }
    let exact_identity_kind = match target_evidence {
        Some(value)
            if active
                && !value.identity_conflict
                && value.kind == ActorKind::Player
                && value.character_id.is_some() =>
        {
            "player-character-id"
        }
        Some(value) if active && !value.identity_conflict && value.monster_id.is_some() => {
            "numeric-monster-id"
        }
        _ => "unresolved",
    };
    if exact_identity_kind == "unresolved" {
        unresolved_reasons.push("exact-static-target-identity-absent");
    }
    let static_target_stat_join_allowed = exact_identity_kind == "numeric-monster-id";
    let mut source_unresolved_reasons = Vec::new();
    if request
        .source_entity_uuid
        .is_some_and(|expected| expected != source_entity_uuid)
    {
        source_unresolved_reasons.push("source-entity-mismatch");
    }
    let source_active = source_evidence.is_some_and(|value| value.active);
    if source_evidence.is_none() {
        source_unresolved_reasons.push("event-time-source-actor-state-absent");
    } else if !source_active {
        source_unresolved_reasons.push("latest-source-actor-state-despawned");
    }
    if source_evidence.is_some_and(|value| value.identity_conflict) {
        source_unresolved_reasons.push("source-actor-identity-conflict");
    }
    let source_exact_identity_kind = match source_evidence {
        Some(value)
            if source_active
                && !value.identity_conflict
                && value.kind == ActorKind::Player
                && value.character_id.is_some() =>
        {
            "player-character-id"
        }
        Some(value) if source_active && !value.identity_conflict && value.monster_id.is_some() => {
            "numeric-monster-id"
        }
        _ => "unresolved",
    };
    if source_exact_identity_kind == "unresolved" {
        source_unresolved_reasons.push("exact-static-source-identity-absent");
    }
    Observation {
        session_id: request.session_id.clone(),
        sequence: request.sequence,
        run_ordinal,
        scene_id,
        source_entity_uuid: Some(source_entity_uuid),
        source_actor_observation_sequence: source_evidence.map(|value| value.actor_sequence),
        source_actor_active: source_active,
        source_entity_type_id: source_evidence.map(|value| value.entity_type_id),
        source_actor_kind: source_evidence.map(|value| actor_kind_name(value.kind).to_owned()),
        source_numeric_monster_id: source_evidence.and_then(|value| value.monster_id),
        source_character_id: source_evidence.and_then(|value| value.character_id.clone()),
        source_class_id: source_evidence.and_then(|value| value.class_id),
        source_specialization_id: source_evidence.and_then(|value| value.specialization_id),
        source_level: source_evidence.and_then(|value| value.level),
        source_identity_conflict: source_evidence.is_some_and(|value| value.identity_conflict),
        source_exact_identity_kind,
        source_unresolved_reasons,
        target_entity_uuid,
        actor_observation_sequence: target_evidence.map(|value| value.actor_sequence),
        actor_active: active,
        entity_type_id: target_evidence.map(|value| value.entity_type_id),
        actor_kind: target_evidence.map(|value| actor_kind_name(value.kind).to_owned()),
        numeric_monster_id: target_evidence.and_then(|value| value.monster_id),
        character_id: target_evidence.and_then(|value| value.character_id.clone()),
        class_id: target_evidence.and_then(|value| value.class_id),
        specialization_id: target_evidence.and_then(|value| value.specialization_id),
        level: target_evidence.and_then(|value| value.level),
        identity_conflict: target_evidence.is_some_and(|value| value.identity_conflict),
        exact_identity_kind,
        static_target_stat_join_allowed,
        unresolved_reasons,
    }
}

fn summarize(
    requests: &BTreeMap<(String, u64), Request>,
    observations: &[Observation],
    missing: usize,
) -> Summary {
    let mut identity_counts = BTreeMap::<String, usize>::new();
    let mut kind_counts = BTreeMap::<String, usize>::new();
    let mut monster_ids = std::collections::BTreeSet::new();
    for row in observations {
        *identity_counts
            .entry(row.exact_identity_kind.to_owned())
            .or_default() += 1;
        *kind_counts
            .entry(
                row.actor_kind
                    .clone()
                    .unwrap_or_else(|| "unresolved".to_owned()),
            )
            .or_default() += 1;
        if let Some(value) = row.numeric_monster_id {
            monster_ids.insert(value);
        }
    }
    Summary {
        requested_actions: requests.len(),
        matched_actions: observations.len(),
        missing_actions: missing,
        observations_with_active_actor_state: observations
            .iter()
            .filter(|row| row.actor_active)
            .count(),
        observations_with_exact_numeric_monster_id: observations
            .iter()
            .filter(|row| row.exact_identity_kind == "numeric-monster-id")
            .count(),
        observations_with_exact_character_id: observations
            .iter()
            .filter(|row| row.exact_identity_kind == "player-character-id")
            .count(),
        observations_with_level: observations
            .iter()
            .filter(|row| row.level.is_some())
            .count(),
        observations_with_identity_conflict: observations
            .iter()
            .filter(|row| row.identity_conflict)
            .count(),
        observations_with_active_source_actor_state: observations
            .iter()
            .filter(|row| row.source_actor_active)
            .count(),
        observations_with_exact_source_numeric_monster_id: observations
            .iter()
            .filter(|row| row.source_exact_identity_kind == "numeric-monster-id")
            .count(),
        observations_with_exact_source_character_id: observations
            .iter()
            .filter(|row| row.source_exact_identity_kind == "player-character-id")
            .count(),
        observations_with_source_identity_conflict: observations
            .iter()
            .filter(|row| row.source_identity_conflict)
            .count(),
        exact_identity_kind_counts: counts(identity_counts),
        target_actor_kind_counts: counts(kind_counts),
        distinct_numeric_monster_ids: monster_ids.len(),
    }
}

fn counts(values: BTreeMap<String, usize>) -> Vec<Count> {
    values
        .into_iter()
        .map(|(value, count)| Count { value, count })
        .collect()
}

fn actor_kind_name(kind: ActorKind) -> &'static str {
    match kind {
        ActorKind::Player => "player",
        ActorKind::Monster => "monster",
        ActorKind::Npc => "npc",
        ActorKind::SceneObject => "scene-object",
        ActorKind::Zone => "zone",
        ActorKind::Projectile => "projectile",
        ActorKind::Pet => "pet",
        ActorKind::TrainingDummy => "training-dummy",
        ActorKind::Drop => "drop",
        ActorKind::Field => "field",
        ActorKind::Trap => "trap",
        ActorKind::Collection => "collection",
        ActorKind::StaticObject => "static-object",
        ActorKind::Vehicle => "vehicle",
        ActorKind::Toy => "toy",
        ActorKind::Housing => "housing",
        ActorKind::Unknown(_) => "unknown",
    }
}

fn read_requests(
    path: &Path,
) -> Result<BTreeMap<(String, u64), Request>, Box<dyn std::error::Error>> {
    let value: Value = serde_json::from_reader(BufReader::new(File::open(path)?))?;
    let mut requests = BTreeMap::new();
    if let Some(rows) = value.get("observations").and_then(Value::as_array) {
        for row in rows {
            let sequence = required_u64(row, "sequence")?;
            insert_request(&mut requests, request_from_row(row, sequence)?)?;
        }
    } else if let Some(pairs) = value.get("exact_pairs").and_then(Value::as_array) {
        for pair in pairs {
            for sequence_field in ["absent_sequences", "present_sequences"] {
                let sequences = pair
                    .get(sequence_field)
                    .and_then(Value::as_array)
                    .ok_or_else(|| format!("exact pair must contain {sequence_field}"))?;
                for sequence in sequences {
                    let sequence = sequence
                        .as_u64()
                        .ok_or_else(|| format!("{sequence_field} must contain integers"))?;
                    insert_request(&mut requests, request_from_row(pair, sequence)?)?;
                }
            }
        }
    } else {
        return Err("selection input must contain observations or exact_pairs".into());
    }
    if requests.is_empty() {
        return Err("selection input contains no selected actions".into());
    }
    Ok(requests)
}

fn request_from_row(row: &Value, sequence: u64) -> Result<Request, String> {
    Ok(Request {
        session_id: required_str(row, "session_id")?.to_owned(),
        sequence,
        run_ordinal: u32::try_from(required_u64(row, "run_ordinal")?)
            .map_err(|_| "run_ordinal exceeds u32".to_owned())?,
        source_entity_uuid: row.get("source_entity_uuid").and_then(Value::as_i64),
        target_entity_uuid: required_i64(row, "target_entity_uuid")?,
    })
}

fn insert_request(
    requests: &mut BTreeMap<(String, u64), Request>,
    request: Request,
) -> Result<(), String> {
    let key = (request.session_id.clone(), request.sequence);
    if requests.insert(key, request).is_some() {
        return Err("duplicate selected session/sequence".to_owned());
    }
    Ok(())
}

fn required_str<'a>(value: &'a Value, name: &str) -> Result<&'a str, String> {
    value
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing {name}"))
}

fn required_u64(value: &Value, name: &str) -> Result<u64, String> {
    value
        .get(name)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("missing {name}"))
}

fn required_i64(value: &Value, name: &str) -> Result<i64, String> {
    value
        .get(name)
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("missing {name}"))
}

fn parse_args<I>(args: I) -> Result<Arguments, String>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args = args.into_iter();
    let mut build = None;
    let mut selected_actions = None;
    let mut rlogs = Vec::new();
    let mut output = None;
    while let Some(arg) = args.next() {
        match arg.to_string_lossy().as_ref() {
            "--build" => build = Some(next(&mut args, "--build")?.to_string_lossy().into_owned()),
            "--selected-actions" => {
                selected_actions = Some(PathBuf::from(next(&mut args, "--selected-actions")?))
            }
            "--rlog" => rlogs.push(PathBuf::from(next(&mut args, "--rlog")?)),
            "--output" => output = Some(PathBuf::from(next(&mut args, "--output")?)),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    if rlogs.is_empty() {
        return Err("at least one --rlog is required".to_owned());
    }
    Ok(Arguments {
        build: build.ok_or_else(|| "--build is required".to_owned())?,
        selected_actions: selected_actions
            .ok_or_else(|| "--selected-actions is required".to_owned())?,
        rlogs,
        output: output.ok_or_else(|| "--output is required".to_owned())?,
    })
}

fn next<I>(args: &mut I, name: &str) -> Result<OsString, String>
where
    I: Iterator<Item = OsString>,
{
    args.next()
        .ok_or_else(|| format!("{name} requires a value"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverse_defense_exact_pairs_expand_to_every_present_and_absent_sequence() {
        let value = serde_json::json!({
            "exact_pairs": [{
                "session_id": "s",
                "run_ordinal": 0,
                "source_entity_uuid": 10,
                "target_entity_uuid": 20,
                "absent_sequences": [7, 8],
                "present_sequences": [9]
            }]
        });
        let mut requests = BTreeMap::new();
        for pair in value["exact_pairs"].as_array().unwrap() {
            for sequence_field in ["absent_sequences", "present_sequences"] {
                for sequence in pair[sequence_field].as_array().unwrap() {
                    let request = request_from_row(pair, sequence.as_u64().unwrap()).unwrap();
                    insert_request(&mut requests, request).unwrap();
                }
            }
        }

        assert_eq!(
            requests.keys().cloned().collect::<Vec<_>>(),
            vec![
                ("s".to_owned(), 7),
                ("s".to_owned(), 8),
                ("s".to_owned(), 9)
            ]
        );
    }

    #[test]
    fn unresolved_actor_never_allows_static_target_join() {
        let request = Request {
            session_id: "s".to_owned(),
            sequence: 2,
            run_ordinal: 1,
            source_entity_uuid: None,
            target_entity_uuid: 3,
        };
        let row = observation(&request, 1, None, 1, None, 3, None);
        assert!(!row.static_target_stat_join_allowed);
        assert_eq!(row.exact_identity_kind, "unresolved");
    }

    #[test]
    fn action_identity_keeps_monster_source_and_player_target_without_allegiance_inference() {
        let request = Request {
            session_id: "s".to_owned(),
            sequence: 9,
            run_ordinal: 2,
            source_entity_uuid: Some(100),
            target_entity_uuid: 200,
        };
        let source = ActorEvidence {
            active: true,
            actor_sequence: 7,
            entity_type_id: 9,
            kind: ActorKind::Monster,
            monster_id: Some(77_001),
            character_id: None,
            class_id: None,
            specialization_id: None,
            level: Some(60),
            identity_conflict: false,
        };
        let target = ActorEvidence {
            active: true,
            actor_sequence: 8,
            entity_type_id: 10,
            kind: ActorKind::Player,
            monster_id: None,
            character_id: None,
            class_id: Some(11),
            specialization_id: Some(117),
            level: None,
            identity_conflict: false,
        };
        let row = observation(
            &request,
            2,
            Some(6515),
            100,
            Some(&source),
            200,
            Some(&target),
        );
        assert_eq!(row.scene_id, Some(6515));
        assert_eq!(row.source_exact_identity_kind, "numeric-monster-id");
        assert_eq!(row.source_numeric_monster_id, Some(77_001));
        assert_eq!(row.actor_kind.as_deref(), Some("player"));
        assert_eq!(row.class_id, Some(11));
        assert_eq!(row.specialization_id, Some(117));
        assert_eq!(row.exact_identity_kind, "unresolved");
        assert!(!row.static_target_stat_join_allowed);
    }
}
