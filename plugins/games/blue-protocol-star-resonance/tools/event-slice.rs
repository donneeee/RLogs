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
    data_gap_pattern: Option<String>,
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
            arguments.effect_id,
            arguments.actor_id,
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
    effect_id: Option<i64>,
    actor_id: Option<u64>,
    data_gap_pattern: Option<&str>,
) -> bool {
    if let Some(pattern) = data_gap_pattern {
        let CanonicalEvent::Timeline(timeline) = event else {
            return false;
        };
        return matches!(
            &timeline.kind,
            TimelineEventKind::DataGap(gap) if gap.detail.contains(pattern)
        );
    }
    if effect_id.is_none() && actor_id.is_none() {
        return true;
    }
    let CanonicalEvent::Timeline(timeline) = event else {
        return false;
    };
    match &timeline.kind {
        TimelineEventKind::Actor(actor) => {
            effect_id.is_none()
                && actor_id.is_none_or(|actor_id| actor.actor.actor_id.0 == actor_id)
        }
        TimelineEventKind::Status(status) => {
            effect_id.is_none_or(|effect_id| status.effect.0 == effect_id)
                && actor_id.is_none_or(|actor_id| {
                    status.target.actor_id.0 == actor_id
                        || status
                            .source
                            .is_some_and(|source| source.actor_id.0 == actor_id)
                })
        }
        TimelineEventKind::Damage(damage) => {
            effect_id.is_none()
                && actor_id.is_none_or(|actor_id| {
                    damage.source.actor_id.0 == actor_id || damage.target.actor_id.0 == actor_id
                })
        }
        TimelineEventKind::Healing(healing) => {
            effect_id.is_none()
                && actor_id.is_none_or(|actor_id| {
                    healing.source.actor_id.0 == actor_id || healing.target.actor_id.0 == actor_id
                })
        }
        TimelineEventKind::EntityAttributes(attributes) => {
            effect_id.is_none()
                && actor_id.is_none_or(|actor_id| attributes.actor.actor_id.0 == actor_id)
        }
        TimelineEventKind::TemporaryAttributes(attributes) => {
            effect_id.is_none()
                && actor_id.is_none_or(|actor_id| attributes.actor.actor_id.0 == actor_id)
        }
        _ => false,
    }
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
    let data_gap_pattern = take_optional_value(&mut values, "--data-gap-pattern")
        .map(|value| value.to_string_lossy().into_owned());
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
        data_gap_pattern,
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
    "usage: rlogs-bpsr-event-slice --rlog <sealed.rlog> --output <events.jsonl> --first <sequence> --last <sequence> [--effect-id <id>] [--actor-id <id>] [--data-gap-pattern <text>]".to_owned()
}
