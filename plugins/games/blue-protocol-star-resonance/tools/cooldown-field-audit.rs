use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    env,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{CanonicalEvent, CooldownEvent, StatusState, TimelineEventKind};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::Serialize;

const SCHEMA_VERSION: u16 = 1;
const MAX_EXAMPLES_PER_ABILITY: usize = 32;

#[derive(Debug)]
struct Arguments {
    effect_id: i64,
    output: PathBuf,
    rlogs: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct RawCooldownFields {
    begin_time_millis: Option<i64>,
    duration_millis: Option<i32>,
    valid_duration_millis: Option<i32>,
    cooldown_type: Option<i32>,
    profession_hold_begin_time_millis: Option<i64>,
    charge_count: Option<i32>,
    valid_cooldown_time_millis: Option<i32>,
    sub_cooldown_ratio_raw: Option<i32>,
    sub_cooldown_fixed_raw: Option<i64>,
    accelerate_cooldown_ratio_raw: Option<i32>,
}

impl From<&CooldownEvent> for RawCooldownFields {
    fn from(event: &CooldownEvent) -> Self {
        Self {
            begin_time_millis: event.begin_time_millis,
            duration_millis: event.duration_millis,
            valid_duration_millis: event.valid_duration_millis,
            cooldown_type: event.cooldown_type,
            profession_hold_begin_time_millis: event.profession_hold_begin_time_millis,
            charge_count: event.charge_count,
            valid_cooldown_time_millis: event.valid_cooldown_time_millis,
            sub_cooldown_ratio_raw: event.sub_cooldown_ratio_raw,
            sub_cooldown_fixed_raw: event.sub_cooldown_fixed_raw,
            accelerate_cooldown_ratio_raw: event.accelerate_cooldown_ratio_raw,
        }
    }
}

impl RawCooldownFields {
    fn has_rich_fields(self) -> bool {
        self.profession_hold_begin_time_millis.is_some()
            || self.charge_count.is_some()
            || self.valid_cooldown_time_millis.is_some()
            || self.sub_cooldown_ratio_raw.is_some()
            || self.sub_cooldown_fixed_raw.is_some()
            || self.accelerate_cooldown_ratio_raw.is_some()
    }
}

#[derive(Debug, Clone, Serialize)]
struct CooldownExample {
    rlog: String,
    session_id: String,
    envelope_sequence: u64,
    timeline_sequence: u64,
    observed_micros: u64,
    game_time_millis: Option<i64>,
    actor_entity_uuid: i64,
    watched_effect_active: bool,
    fields: RawCooldownFields,
}

#[derive(Debug, Default)]
struct AbilityAccumulator {
    event_count: u64,
    rich_event_count: u64,
    inside_watched_effect_count: u64,
    actor_entity_uuids: BTreeSet<i64>,
    distinct_fields: BTreeSet<RawCooldownFields>,
    examples: Vec<CooldownExample>,
}

#[derive(Debug, Serialize)]
struct AbilityReport {
    ability_id: i64,
    event_count: u64,
    rich_event_count: u64,
    inside_watched_effect_count: u64,
    actor_entity_uuids: Vec<i64>,
    distinct_fields: Vec<RawCooldownFields>,
    examples: Vec<CooldownExample>,
}

#[derive(Debug, Serialize)]
struct SessionReport {
    rlog: String,
    session_id: String,
    cooldown_event_count: u64,
    rich_cooldown_event_count: u64,
    watched_effect_apply_or_refresh_count: u64,
    watched_effect_end_count: u64,
    cooldowns_inside_watched_effect_count: u64,
}

#[derive(Debug, Serialize)]
struct AuditBundle {
    schema_version: u16,
    generated_by: &'static str,
    policy: AuditPolicy,
    watched_effect_id: i64,
    totals: Totals,
    sessions: Vec<SessionReport>,
    abilities: Vec<AbilityReport>,
}

#[derive(Debug, Serialize)]
struct AuditPolicy {
    runtime_formula_authority: bool,
    unresolved_evidence_hidden: bool,
    wire_values_scaled_or_reinterpreted: bool,
    promotion_requirement: &'static str,
}

#[derive(Debug, Default, Serialize)]
struct Totals {
    cooldown_events: u64,
    rich_cooldown_events: u64,
    cooldowns_inside_watched_effect: u64,
    abilities: usize,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("cooldown field audit failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments()?;
    let mut accumulators = BTreeMap::<i64, AbilityAccumulator>::new();
    let mut sessions = Vec::new();
    for rlog in &arguments.rlogs {
        sessions.push(read_session(rlog, arguments.effect_id, &mut accumulators)?);
    }

    let mut totals = Totals::default();
    let abilities = accumulators
        .into_iter()
        .map(|(ability_id, accumulator)| {
            totals.cooldown_events += accumulator.event_count;
            totals.rich_cooldown_events += accumulator.rich_event_count;
            totals.cooldowns_inside_watched_effect += accumulator.inside_watched_effect_count;
            AbilityReport {
                ability_id,
                event_count: accumulator.event_count,
                rich_event_count: accumulator.rich_event_count,
                inside_watched_effect_count: accumulator.inside_watched_effect_count,
                actor_entity_uuids: accumulator.actor_entity_uuids.into_iter().collect(),
                distinct_fields: accumulator.distinct_fields.into_iter().collect(),
                examples: accumulator.examples,
            }
        })
        .collect::<Vec<_>>();
    totals.abilities = abilities.len();

    let bundle = AuditBundle {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-cooldown-field-audit",
        policy: AuditPolicy {
            runtime_formula_authority: false,
            unresolved_evidence_hidden: false,
            wire_values_scaled_or_reinterpreted: false,
            promotion_requirement: "Prove each raw field's unit and cooldown equation from repeated packet-observed transitions, then conserve provider-attributed action opportunity before enabling rDPS.",
        },
        watched_effect_id: arguments.effect_id,
        totals,
        sessions,
        abilities,
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
    accumulators: &mut BTreeMap<i64, AbilityAccumulator>,
) -> Result<SessionReport, Box<dyn std::error::Error>> {
    let mut reader = RlogReader::new(BufReader::new(File::open(rlog)?), RlogLimits::default())?;
    let mut active_effects = HashMap::<i64, BTreeSet<Option<i64>>>::new();
    let mut report = SessionReport {
        rlog: rlog.display().to_string(),
        session_id: String::new(),
        cooldown_event_count: 0,
        rich_cooldown_event_count: 0,
        watched_effect_apply_or_refresh_count: 0,
        watched_effect_end_count: 0,
        cooldowns_inside_watched_effect_count: 0,
    };

    while let Some(envelope) = reader.next_event()? {
        report.session_id = envelope.session_id.clone();
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            continue;
        };
        match &timeline.kind {
            TimelineEventKind::Status(status) if status.effect.0 == effect_id => {
                let target = status.target.entity_uuid.0;
                let instance = status.instance_id.map(|id| id.0);
                match status.state {
                    StatusState::Applied | StatusState::Refreshed | StatusState::Stacked => {
                        active_effects.entry(target).or_default().insert(instance);
                        report.watched_effect_apply_or_refresh_count += 1;
                    }
                    StatusState::Consumed | StatusState::Removed => {
                        if let Some(instances) = active_effects.get_mut(&target) {
                            instances.remove(&instance);
                            if instances.is_empty() {
                                active_effects.remove(&target);
                            }
                        }
                        report.watched_effect_end_count += 1;
                    }
                }
            }
            TimelineEventKind::Cooldown(cooldown) => {
                let fields = RawCooldownFields::from(cooldown);
                let rich = fields.has_rich_fields();
                let actor_entity_uuid = cooldown.actor.entity_uuid.0;
                let watched_effect_active = active_effects
                    .get(&actor_entity_uuid)
                    .is_some_and(|instances| !instances.is_empty());
                report.cooldown_event_count += 1;
                report.rich_cooldown_event_count += u64::from(rich);
                report.cooldowns_inside_watched_effect_count += u64::from(watched_effect_active);

                let accumulator = accumulators.entry(cooldown.ability.0).or_default();
                accumulator.event_count += 1;
                accumulator.rich_event_count += u64::from(rich);
                accumulator.inside_watched_effect_count += u64::from(watched_effect_active);
                accumulator.actor_entity_uuids.insert(actor_entity_uuid);
                accumulator.distinct_fields.insert(fields);
                if accumulator.examples.len() < MAX_EXAMPLES_PER_ABILITY {
                    accumulator.examples.push(CooldownExample {
                        rlog: rlog.display().to_string(),
                        session_id: envelope.session_id.clone(),
                        envelope_sequence: envelope.sequence,
                        timeline_sequence: timeline.sequence,
                        observed_micros: timeline.time.observed_micros,
                        game_time_millis: timeline.time.game_time_millis,
                        actor_entity_uuid,
                        watched_effect_active,
                        fields,
                    });
                }
            }
            _ => {}
        }
    }
    Ok(report)
}

fn arguments() -> Result<Arguments, String> {
    let mut values = env::args_os().skip(1).collect::<Vec<_>>();
    let effect_position = values
        .iter()
        .position(|value| value == "--effect")
        .ok_or_else(usage)?;
    if effect_position + 1 >= values.len() {
        return Err("--effect requires an integer".to_owned());
    }
    let effect_id = values
        .remove(effect_position + 1)
        .to_string_lossy()
        .parse::<i64>()
        .map_err(|_| "--effect requires an integer".to_owned())?;
    values.remove(effect_position);

    let output_position = values
        .iter()
        .position(|value| value == "--output")
        .ok_or_else(usage)?;
    if output_position + 1 >= values.len() {
        return Err("--output requires a path".to_owned());
    }
    let output = PathBuf::from(values.remove(output_position + 1));
    values.remove(output_position);
    if values.is_empty() {
        return Err(usage());
    }
    Ok(Arguments {
        effect_id,
        output,
        rlogs: values.into_iter().map(PathBuf::from).collect(),
    })
}

fn usage() -> String {
    "usage: rlogs-bpsr-cooldown-field-audit --effect <status-effect-id> --output <proof.json> <sealed.rlog>...".to_owned()
}
