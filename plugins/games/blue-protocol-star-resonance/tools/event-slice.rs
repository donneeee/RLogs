use std::{
    env,
    ffi::OsString,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::PathBuf,
};

use rlogs_events::{CanonicalEvent, TimelineEventKind};
use rlogs_log_format::{RlogLimits, RlogReader};

#[derive(Debug)]
struct Arguments {
    rlog: PathBuf,
    output: PathBuf,
    first_sequence: u64,
    last_sequence: u64,
    effect_id: Option<i64>,
    actor_id: Option<u64>,
    entity_uuid: Option<i64>,
    numeric_id_prefix: Option<String>,
    data_gap_pattern: Option<String>,
    event_kind: Option<EventKindFilter>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EventKindFilter {
    Dungeon,
    RunBoundary,
    EncounterBoundary,
    CombatBoundary,
    Actor,
    EntityAttributes,
    Life,
    Damage,
    Cast,
    Cooldown,
    DataGap,
}

impl EventKindFilter {
    fn parse(value: OsString) -> Result<Self, String> {
        match value.to_string_lossy().as_ref() {
            "dungeon" => Ok(Self::Dungeon),
            "run_boundary" => Ok(Self::RunBoundary),
            "encounter_boundary" => Ok(Self::EncounterBoundary),
            "combat_boundary" => Ok(Self::CombatBoundary),
            "actor" => Ok(Self::Actor),
            "entity_attributes" => Ok(Self::EntityAttributes),
            "life" => Ok(Self::Life),
            "damage" => Ok(Self::Damage),
            "cast" => Ok(Self::Cast),
            "cooldown" => Ok(Self::Cooldown),
            "data_gap" => Ok(Self::DataGap),
            _ => Err("--event-kind must be dungeon, run_boundary, encounter_boundary, combat_boundary, actor, entity_attributes, life, damage, cast, cooldown, or data_gap".to_owned()),
        }
    }

    fn matches(self, event: &CanonicalEvent) -> bool {
        match (self, event) {
            (Self::Dungeon, CanonicalEvent::Dungeon(_)) => true,
            (Self::RunBoundary, CanonicalEvent::Timeline(timeline)) => {
                matches!(timeline.kind, TimelineEventKind::RunBoundary { .. })
            }
            (Self::EncounterBoundary, CanonicalEvent::Timeline(timeline)) => {
                matches!(timeline.kind, TimelineEventKind::EncounterBoundary { .. })
            }
            (Self::CombatBoundary, CanonicalEvent::Timeline(timeline)) => {
                matches!(timeline.kind, TimelineEventKind::CombatBoundary { .. })
            }
            (Self::Actor, CanonicalEvent::Timeline(timeline)) => {
                matches!(timeline.kind, TimelineEventKind::Actor(_))
            }
            (Self::EntityAttributes, CanonicalEvent::Timeline(timeline)) => {
                matches!(timeline.kind, TimelineEventKind::EntityAttributes(_))
            }
            (Self::Life, CanonicalEvent::Timeline(timeline)) => {
                matches!(timeline.kind, TimelineEventKind::Life { .. })
            }
            (Self::Damage, CanonicalEvent::Timeline(timeline)) => {
                matches!(timeline.kind, TimelineEventKind::Damage(_))
            }
            (Self::Cast, CanonicalEvent::Timeline(timeline)) => {
                matches!(timeline.kind, TimelineEventKind::Cast(_))
            }
            (Self::Cooldown, CanonicalEvent::Timeline(timeline)) => {
                matches!(timeline.kind, TimelineEventKind::Cooldown(_))
            }
            (Self::DataGap, CanonicalEvent::Timeline(timeline)) => {
                matches!(timeline.kind, TimelineEventKind::DataGap(_))
            }
            _ => false,
        }
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("event slice failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments()?;
    let mut reader = RlogReader::new(
        BufReader::new(File::open(arguments.rlog)?),
        RlogLimits::default(),
    )?;
    let mut writer = BufWriter::new(File::create(arguments.output)?);
    while let Some(envelope) = reader.next_event()? {
        if envelope.sequence < arguments.first_sequence {
            continue;
        }
        if envelope.sequence > arguments.last_sequence {
            break;
        }
        if !matches_filters(
            &envelope.event,
            arguments.event_kind,
            arguments.effect_id,
            arguments.actor_id,
            arguments.entity_uuid,
            arguments.numeric_id_prefix.as_deref(),
            arguments.data_gap_pattern.as_deref(),
        ) {
            continue;
        }
        serde_json::to_writer(&mut writer, &envelope)?;
        writer.write_all(b"\n")?;
    }
    writer.flush()?;
    Ok(())
}

fn matches_filters(
    event: &CanonicalEvent,
    event_kind: Option<EventKindFilter>,
    effect_id: Option<i64>,
    actor_id: Option<u64>,
    entity_uuid: Option<i64>,
    numeric_id_prefix: Option<&str>,
    data_gap_pattern: Option<&str>,
) -> bool {
    if event_kind.is_some_and(|kind| !kind.matches(event)) {
        return false;
    }
    if effect_id.is_none()
        && actor_id.is_none()
        && entity_uuid.is_none()
        && numeric_id_prefix.is_none()
        && data_gap_pattern.is_none()
    {
        return true;
    }
    if let Some(pattern) = data_gap_pattern {
        let CanonicalEvent::Timeline(timeline) = event else {
            return false;
        };
        return matches!(
            &timeline.kind,
            TimelineEventKind::DataGap(gap) if gap.detail.contains(pattern)
        );
    }
    if effect_id.is_none()
        && actor_id.is_none()
        && entity_uuid.is_none()
        && numeric_id_prefix.is_none()
    {
        return true;
    }
    let CanonicalEvent::Timeline(timeline) = event else {
        return false;
    };
    match &timeline.kind {
        TimelineEventKind::Actor(actor) => {
            effect_id.is_none()
                && numeric_id_prefix.is_none()
                && actor_id.is_none_or(|actor_id| actor.actor.actor_id.0 == actor_id)
                && entity_uuid.is_none_or(|entity_uuid| actor.actor.entity_uuid.0 == entity_uuid)
        }
        TimelineEventKind::Status(status) => {
            effect_id.is_none_or(|effect_id| status.effect.0 == effect_id)
                && numeric_id_prefix.is_none_or(|prefix| {
                    id_has_prefix(status.effect.0, prefix)
                        || status
                            .origin
                            .is_some_and(|origin| id_has_prefix(origin.source_config_id, prefix))
                })
                && actor_id.is_none_or(|actor_id| {
                    status.target.actor_id.0 == actor_id
                        || status
                            .source
                            .is_some_and(|source| source.actor_id.0 == actor_id)
                })
                && entity_uuid.is_none_or(|entity_uuid| {
                    status.target.entity_uuid.0 == entity_uuid
                        || status
                            .source
                            .is_some_and(|source| source.entity_uuid.0 == entity_uuid)
                })
        }
        TimelineEventKind::Damage(damage) => {
            effect_id.is_none()
                && numeric_id_prefix.is_none_or(|prefix| {
                    damage
                        .ability
                        .is_some_and(|ability| id_has_prefix(ability.0, prefix))
                        || damage
                            .packet
                            .owner_id
                            .is_some_and(|id| id_has_prefix(i64::from(id), prefix))
                        || damage
                            .packet
                            .breakdown_ability_id
                            .is_some_and(|id| id_has_prefix(id, prefix))
                })
                && actor_id.is_none_or(|actor_id| {
                    damage.source.actor_id.0 == actor_id || damage.target.actor_id.0 == actor_id
                })
                && entity_uuid.is_none_or(|entity_uuid| {
                    damage.source.entity_uuid.0 == entity_uuid
                        || damage.target.entity_uuid.0 == entity_uuid
                })
        }
        TimelineEventKind::Healing(healing) => {
            effect_id.is_none()
                && numeric_id_prefix.is_none_or(|prefix| {
                    healing
                        .ability
                        .is_some_and(|ability| id_has_prefix(ability.0, prefix))
                        || healing
                            .packet
                            .owner_id
                            .is_some_and(|id| id_has_prefix(i64::from(id), prefix))
                        || healing
                            .packet
                            .breakdown_ability_id
                            .is_some_and(|id| id_has_prefix(id, prefix))
                })
                && actor_id.is_none_or(|actor_id| {
                    healing.source.actor_id.0 == actor_id || healing.target.actor_id.0 == actor_id
                })
                && entity_uuid.is_none_or(|entity_uuid| {
                    healing.source.entity_uuid.0 == entity_uuid
                        || healing.target.entity_uuid.0 == entity_uuid
                })
        }
        TimelineEventKind::EntityAttributes(attributes) => {
            effect_id.is_none()
                && numeric_id_prefix.is_none()
                && actor_id.is_none_or(|actor_id| attributes.actor.actor_id.0 == actor_id)
                && entity_uuid
                    .is_none_or(|entity_uuid| attributes.actor.entity_uuid.0 == entity_uuid)
        }
        TimelineEventKind::TemporaryAttributes(attributes) => {
            effect_id.is_none()
                && numeric_id_prefix.is_none()
                && actor_id.is_none_or(|actor_id| attributes.actor.actor_id.0 == actor_id)
                && entity_uuid
                    .is_none_or(|entity_uuid| attributes.actor.entity_uuid.0 == entity_uuid)
        }
        TimelineEventKind::Cast(cast) => {
            effect_id.is_none()
                && numeric_id_prefix.is_none_or(|prefix| id_has_prefix(cast.ability.0, prefix))
                && actor_id.is_none_or(|actor_id| cast.source.actor_id.0 == actor_id)
                && entity_uuid.is_none_or(|entity_uuid| cast.source.entity_uuid.0 == entity_uuid)
        }
        TimelineEventKind::Cooldown(cooldown) => {
            effect_id.is_none()
                && numeric_id_prefix.is_none_or(|prefix| id_has_prefix(cooldown.ability.0, prefix))
                && actor_id.is_none_or(|actor_id| cooldown.actor.actor_id.0 == actor_id)
                && entity_uuid.is_none_or(|entity_uuid| cooldown.actor.entity_uuid.0 == entity_uuid)
        }
        TimelineEventKind::Shield(shield) => {
            effect_id.is_none()
                && numeric_id_prefix.is_none_or(|prefix| id_has_prefix(shield.ability.0, prefix))
                && actor_id.is_none_or(|actor_id| {
                    shield.source.actor_id.0 == actor_id || shield.target.actor_id.0 == actor_id
                })
                && entity_uuid.is_none_or(|entity_uuid| {
                    shield.source.entity_uuid.0 == entity_uuid
                        || shield.target.entity_uuid.0 == entity_uuid
                })
        }
        _ => false,
    }
}

fn id_has_prefix(id: i64, prefix: &str) -> bool {
    id > 0 && id.to_string().starts_with(prefix)
}

fn arguments() -> Result<Arguments, String> {
    let mut values = env::args_os().skip(1).collect::<Vec<_>>();
    let rlog = PathBuf::from(take_value(&mut values, "--rlog")?);
    let output = PathBuf::from(take_value(&mut values, "--output")?);
    let first_sequence = parse_u64(take_value(&mut values, "--first")?, "--first")?;
    let last_sequence = parse_u64(take_value(&mut values, "--last")?, "--last")?;
    let effect_id = take_optional_value(&mut values, "--effect-id")
        .map(|value| parse_i64(value, "--effect-id"))
        .transpose()?;
    let actor_id = take_optional_value(&mut values, "--actor-id")
        .map(|value| parse_u64(value, "--actor-id"))
        .transpose()?;
    let entity_uuid = take_optional_value(&mut values, "--entity-uuid")
        .map(|value| parse_i64(value, "--entity-uuid"))
        .transpose()?;
    let numeric_id_prefix = take_optional_value(&mut values, "--numeric-id-prefix")
        .map(|value| {
            let value = value.to_string_lossy().into_owned();
            if value.is_empty() || !value.chars().all(|character| character.is_ascii_digit()) {
                Err("--numeric-id-prefix requires one or more digits".to_owned())
            } else {
                Ok(value)
            }
        })
        .transpose()?;
    let data_gap_pattern = take_optional_value(&mut values, "--data-gap-pattern")
        .map(|value| value.to_string_lossy().into_owned());
    let event_kind = take_optional_value(&mut values, "--event-kind")
        .map(EventKindFilter::parse)
        .transpose()?;
    if first_sequence > last_sequence {
        return Err("--first must be less than or equal to --last".to_owned());
    }
    if !values.is_empty() {
        return Err(usage());
    }
    Ok(Arguments {
        rlog,
        output,
        first_sequence,
        last_sequence,
        effect_id,
        actor_id,
        entity_uuid,
        numeric_id_prefix,
        data_gap_pattern,
        event_kind,
    })
}

fn parse_i64(value: OsString, flag: &str) -> Result<i64, String> {
    value
        .to_string_lossy()
        .parse::<i64>()
        .map_err(|_| format!("{flag} requires an integer"))
}

fn parse_u64(value: OsString, flag: &str) -> Result<u64, String> {
    value
        .to_string_lossy()
        .parse::<u64>()
        .map_err(|_| format!("{flag} requires a non-negative integer"))
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

fn take_optional_value(values: &mut Vec<OsString>, flag: &str) -> Option<OsString> {
    let position = values.iter().position(|value| value == flag)?;
    if position + 1 >= values.len() {
        return Some(OsString::new());
    }
    let value = values.remove(position + 1);
    values.remove(position);
    Some(value)
}

fn usage() -> String {
    "usage: rlogs-bpsr-event-slice --rlog <sealed.rlog> --output <events.jsonl> --first <sequence> --last <sequence> [--event-kind <kind>] [--effect-id <id>] [--actor-id <id>] [--entity-uuid <id>] [--numeric-id-prefix <digits>] [--data-gap-pattern <text>]".to_owned()
}
