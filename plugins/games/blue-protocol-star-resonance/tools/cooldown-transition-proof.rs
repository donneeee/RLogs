use std::{
    collections::{BTreeMap, HashMap},
    env,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{CanonicalEvent, CooldownEvent, StatusState, TimelineEventKind};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::Serialize;

const SCHEMA_VERSION: u16 = 1;
const DEFAULT_BOUNDARY_MARGIN_MILLIS: u64 = 5_000;

#[derive(Debug)]
struct Arguments {
    effect_id: i64,
    boundary_margin_millis: u64,
    output: PathBuf,
    rlogs: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum ProgressField {
    ValidDurationMillis,
    ValidCooldownTimeMillis,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct RawCooldownSample {
    envelope_sequence: u64,
    timeline_sequence: u64,
    observed_micros: u64,
    game_time_millis: Option<i64>,
    actor_entity_uuid: i64,
    ability_id: i64,
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

impl RawCooldownSample {
    fn from_event(
        envelope_sequence: u64,
        timeline_sequence: u64,
        observed_micros: u64,
        game_time_millis: Option<i64>,
        event: &CooldownEvent,
    ) -> Self {
        Self {
            envelope_sequence,
            timeline_sequence,
            observed_micros,
            game_time_millis,
            actor_entity_uuid: event.actor.entity_uuid.0,
            ability_id: event.ability.0,
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

    fn progress(&self) -> Option<(ProgressField, i64)> {
        self.valid_duration_millis
            .map(|value| (ProgressField::ValidDurationMillis, i64::from(value)))
            .or_else(|| {
                self.valid_cooldown_time_millis
                    .map(|value| (ProgressField::ValidCooldownTimeMillis, i64::from(value)))
            })
    }
}

#[derive(Debug, Clone)]
struct EffectBoundary {
    source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    instance_id: Option<i64>,
    start_envelope_sequence: u64,
    start_timeline_sequence: u64,
    start_observed_micros: u64,
    start_game_time_millis: Option<i64>,
    duration_millis: Option<u64>,
    expected_end_observed_micros: Option<u64>,
    observed_end_envelope_sequence: Option<u64>,
    observed_end_timeline_sequence: Option<u64>,
    observed_end_micros: Option<u64>,
    observed_end_game_time_millis: Option<i64>,
    end_state: Option<&'static str>,
}

impl EffectBoundary {
    fn end_micros(&self) -> u64 {
        self.observed_end_micros
            .or(self.expected_end_observed_micros)
            .unwrap_or(self.start_observed_micros)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum TransitionPhase {
    Before,
    StartBoundary,
    Inside,
    EndBoundary,
    After,
}

#[derive(Debug, Clone, Serialize)]
struct ProgressTransition {
    phase: TransitionPhase,
    progress_field: ProgressField,
    from: RawCooldownSample,
    to: RawCooldownSample,
    observed_delta_micros: u64,
    game_time_delta_millis: Option<i64>,
    progress_delta_raw: i64,
    progress_per_game_millisecond: Option<f64>,
    begin_time_unchanged: bool,
    duration_unchanged: bool,
    cooldown_type_unchanged: bool,
    progress_did_not_rewind: bool,
    usable_for_continuous_slope_proof: bool,
    progress_inside_positive_duration_at_both_samples: bool,
    completed_at_to_sample: bool,
    usable_for_active_cooldown_slope_proof: bool,
}

#[derive(Debug, Serialize)]
struct AbilityTransitionReport {
    ability_id: i64,
    transitions: Vec<ProgressTransition>,
}

#[derive(Debug, Serialize)]
struct BoundaryReport {
    source_entity_uuid: Option<i64>,
    target_entity_uuid: i64,
    instance_id: Option<i64>,
    start_envelope_sequence: u64,
    start_timeline_sequence: u64,
    start_observed_micros: u64,
    start_game_time_millis: Option<i64>,
    duration_millis: Option<u64>,
    expected_end_observed_micros: Option<u64>,
    observed_end_envelope_sequence: Option<u64>,
    observed_end_timeline_sequence: Option<u64>,
    observed_end_micros: Option<u64>,
    observed_end_game_time_millis: Option<i64>,
    end_state: Option<&'static str>,
    ability_reports: Vec<AbilityTransitionReport>,
}

#[derive(Debug, Serialize)]
struct SessionReport {
    rlog: String,
    session_id: String,
    watched_effect_start_observation_count: usize,
    watched_effect_matched_end_observation_count: usize,
    watched_effect_interval_count: usize,
    cooldown_event_count: u64,
    cooldown_event_deduplicated_count: u64,
    boundaries: Vec<BoundaryReport>,
}

#[derive(Debug, Serialize)]
struct AuditBundle {
    schema_version: u16,
    generated_by: &'static str,
    policy: AuditPolicy,
    watched_effect_id: i64,
    boundary_margin_millis: u64,
    sessions: Vec<SessionReport>,
}

#[derive(Debug, Serialize)]
struct AuditPolicy {
    runtime_formula_authority: bool,
    unresolved_evidence_hidden: bool,
    wire_values_scaled_or_reinterpreted: bool,
    duplicate_rule: &'static str,
    progress_rule: &'static str,
    promotion_requirement: &'static str,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("cooldown transition proof failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments()?;
    let sessions = arguments
        .rlogs
        .iter()
        .map(|rlog| read_session(rlog, arguments.effect_id, arguments.boundary_margin_millis))
        .collect::<Result<Vec<_>, _>>()?;

    let bundle = AuditBundle {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-cooldown-transition-proof",
        policy: AuditPolicy {
            runtime_formula_authority: false,
            unresolved_evidence_hidden: false,
            wire_values_scaled_or_reinterpreted: false,
            duplicate_rule: "Only byte-equivalent canonical cooldown observations for the same actor, ability, observed timestamp, and raw fields are deduplicated; distinct wire values are retained.",
            progress_rule: "valid_duration_millis is compared as a raw progress field when present; otherwise valid_cooldown_time_millis is compared. Distinct progress-field streams are never crossed. Continuous slope proof requires unchanged begin time, duration, and cooldown type with no progress rewind. Active-cooldown slope proof additionally requires both raw progress observations to be strictly below the same positive duration; a transition reaching or passing duration is retained as a censored completion observation and cannot prove a slope. All rejected transitions remain in the output. No unit conversion or cooldown formula is assumed.",
            promotion_requirement: "Repeated before, inside, and after transitions must prove the exact cooldown equation and provider tier, and the resulting action opportunity must conserve before this mechanic may affect rDPS.",
        },
        watched_effect_id: arguments.effect_id,
        boundary_margin_millis: arguments.boundary_margin_millis,
        sessions,
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
    boundary_margin_millis: u64,
) -> Result<SessionReport, Box<dyn std::error::Error>> {
    let mut reader = RlogReader::new(BufReader::new(File::open(rlog)?), RlogLimits::default())?;
    let mut session_id = String::new();
    let mut boundaries = Vec::<EffectBoundary>::new();
    let mut open_boundaries = HashMap::<(i64, Option<i64>), Vec<usize>>::new();
    let mut cooldown_event_count = 0_u64;
    let mut cooldowns = BTreeMap::<(i64, i64), Vec<RawCooldownSample>>::new();

    while let Some(envelope) = reader.next_event()? {
        session_id = envelope.session_id.clone();
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            continue;
        };
        match &timeline.kind {
            TimelineEventKind::Status(status) if status.effect.0 == effect_id => {
                let key = (
                    status.target.entity_uuid.0,
                    status.instance_id.map(|value| value.0),
                );
                match status.state {
                    StatusState::Applied | StatusState::Refreshed | StatusState::Stacked => {
                        let duration_millis = status.duration_millis;
                        let expected_end_observed_micros = duration_millis.map(|duration| {
                            timeline
                                .time
                                .observed_micros
                                .saturating_add(duration.saturating_mul(1_000))
                        });
                        let index = boundaries.len();
                        boundaries.push(EffectBoundary {
                            source_entity_uuid: status
                                .source
                                .as_ref()
                                .map(|source| source.entity_uuid.0),
                            target_entity_uuid: status.target.entity_uuid.0,
                            instance_id: status.instance_id.map(|value| value.0),
                            start_envelope_sequence: envelope.sequence,
                            start_timeline_sequence: timeline.sequence,
                            start_observed_micros: timeline.time.observed_micros,
                            start_game_time_millis: timeline.time.game_time_millis,
                            duration_millis,
                            expected_end_observed_micros,
                            observed_end_envelope_sequence: None,
                            observed_end_timeline_sequence: None,
                            observed_end_micros: None,
                            observed_end_game_time_millis: None,
                            end_state: None,
                        });
                        open_boundaries.entry(key).or_default().push(index);
                    }
                    StatusState::Consumed | StatusState::Removed => {
                        if let Some(index) = open_boundaries
                            .get_mut(&key)
                            .and_then(|indices| indices.pop())
                        {
                            let boundary = &mut boundaries[index];
                            boundary.observed_end_envelope_sequence = Some(envelope.sequence);
                            boundary.observed_end_timeline_sequence = Some(timeline.sequence);
                            boundary.observed_end_micros = Some(timeline.time.observed_micros);
                            boundary.observed_end_game_time_millis = timeline.time.game_time_millis;
                            boundary.end_state = Some(match status.state {
                                StatusState::Consumed => "consumed",
                                StatusState::Removed => "removed",
                                _ => unreachable!(),
                            });
                        }
                    }
                }
            }
            TimelineEventKind::Cooldown(cooldown) => {
                cooldown_event_count += 1;
                let sample = RawCooldownSample::from_event(
                    envelope.sequence,
                    timeline.sequence,
                    timeline.time.observed_micros,
                    timeline.time.game_time_millis,
                    cooldown,
                );
                cooldowns
                    .entry((sample.actor_entity_uuid, sample.ability_id))
                    .or_default()
                    .push(sample);
            }
            _ => {}
        }
    }

    let mut deduplicated_count = 0_u64;
    for samples in cooldowns.values_mut() {
        samples.sort_by_key(|sample| (sample.observed_micros, sample.envelope_sequence));
        samples.dedup_by(|right, left| {
            right.observed_micros == left.observed_micros
                && right.actor_entity_uuid == left.actor_entity_uuid
                && right.ability_id == left.ability_id
                && right.begin_time_millis == left.begin_time_millis
                && right.duration_millis == left.duration_millis
                && right.valid_duration_millis == left.valid_duration_millis
                && right.cooldown_type == left.cooldown_type
                && right.profession_hold_begin_time_millis == left.profession_hold_begin_time_millis
                && right.charge_count == left.charge_count
                && right.valid_cooldown_time_millis == left.valid_cooldown_time_millis
                && right.sub_cooldown_ratio_raw == left.sub_cooldown_ratio_raw
                && right.sub_cooldown_fixed_raw == left.sub_cooldown_fixed_raw
                && right.accelerate_cooldown_ratio_raw == left.accelerate_cooldown_ratio_raw
        });
        deduplicated_count += samples.len() as u64;
    }

    let start_observation_count = boundaries.len();
    let matched_end_observation_count = boundaries
        .iter()
        .filter(|boundary| boundary.observed_end_micros.is_some())
        .count();
    let boundaries = merge_boundaries(boundaries);
    let boundary_reports = boundaries
        .into_iter()
        .map(|boundary| build_boundary_report(boundary, boundary_margin_millis, &cooldowns))
        .collect::<Vec<_>>();

    Ok(SessionReport {
        rlog: rlog.display().to_string(),
        session_id,
        watched_effect_start_observation_count: start_observation_count,
        watched_effect_matched_end_observation_count: matched_end_observation_count,
        watched_effect_interval_count: boundary_reports.len(),
        cooldown_event_count,
        cooldown_event_deduplicated_count: deduplicated_count,
        boundaries: boundary_reports,
    })
}

fn merge_boundaries(mut boundaries: Vec<EffectBoundary>) -> Vec<EffectBoundary> {
    boundaries.sort_by_key(|boundary| {
        (
            boundary.target_entity_uuid,
            boundary.start_observed_micros,
            boundary.start_envelope_sequence,
        )
    });
    let mut merged = Vec::<EffectBoundary>::new();
    for boundary in boundaries {
        if let Some(last) = merged.last_mut()
            && last.target_entity_uuid == boundary.target_entity_uuid
            && boundary.start_observed_micros <= last.end_micros()
        {
            if last.source_entity_uuid != boundary.source_entity_uuid {
                last.source_entity_uuid = None;
            }
            if last.instance_id != boundary.instance_id {
                last.instance_id = None;
            }
            last.expected_end_observed_micros = last
                .expected_end_observed_micros
                .into_iter()
                .chain(boundary.expected_end_observed_micros)
                .max();
            if boundary.observed_end_micros > last.observed_end_micros {
                last.observed_end_envelope_sequence = boundary.observed_end_envelope_sequence;
                last.observed_end_timeline_sequence = boundary.observed_end_timeline_sequence;
                last.observed_end_micros = boundary.observed_end_micros;
                last.observed_end_game_time_millis = boundary.observed_end_game_time_millis;
                last.end_state = boundary.end_state;
            }
            let merged_end = last.end_micros();
            last.duration_millis = Some(
                merged_end
                    .saturating_sub(last.start_observed_micros)
                    .saturating_div(1_000),
            );
            continue;
        }
        merged.push(boundary);
    }
    merged
}

fn build_boundary_report(
    boundary: EffectBoundary,
    boundary_margin_millis: u64,
    cooldowns: &BTreeMap<(i64, i64), Vec<RawCooldownSample>>,
) -> BoundaryReport {
    let margin_micros = boundary_margin_millis.saturating_mul(1_000);
    let window_start = boundary.start_observed_micros.saturating_sub(margin_micros);
    let window_end = boundary.end_micros().saturating_add(margin_micros);
    let mut ability_reports = Vec::new();

    for ((actor_entity_uuid, ability_id), samples) in cooldowns {
        if *actor_entity_uuid != boundary.target_entity_uuid {
            continue;
        }
        let mut relevant_by_progress_field = BTreeMap::<ProgressField, Vec<_>>::new();
        for sample in samples.iter().filter(|sample| {
            sample.observed_micros >= window_start && sample.observed_micros <= window_end
        }) {
            if let Some((progress_field, _)) = sample.progress() {
                relevant_by_progress_field
                    .entry(progress_field)
                    .or_default()
                    .push(sample.clone());
            }
        }
        let transitions = relevant_by_progress_field
            .into_values()
            .flat_map(|samples| {
                samples
                    .windows(2)
                    .filter_map(|pair| transition(&boundary, &pair[0], &pair[1]))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        if !transitions.is_empty() {
            ability_reports.push(AbilityTransitionReport {
                ability_id: *ability_id,
                transitions,
            });
        }
    }

    BoundaryReport {
        source_entity_uuid: boundary.source_entity_uuid,
        target_entity_uuid: boundary.target_entity_uuid,
        instance_id: boundary.instance_id,
        start_envelope_sequence: boundary.start_envelope_sequence,
        start_timeline_sequence: boundary.start_timeline_sequence,
        start_observed_micros: boundary.start_observed_micros,
        start_game_time_millis: boundary.start_game_time_millis,
        duration_millis: boundary.duration_millis,
        expected_end_observed_micros: boundary.expected_end_observed_micros,
        observed_end_envelope_sequence: boundary.observed_end_envelope_sequence,
        observed_end_timeline_sequence: boundary.observed_end_timeline_sequence,
        observed_end_micros: boundary.observed_end_micros,
        observed_end_game_time_millis: boundary.observed_end_game_time_millis,
        end_state: boundary.end_state,
        ability_reports,
    }
}

fn transition(
    boundary: &EffectBoundary,
    from: &RawCooldownSample,
    to: &RawCooldownSample,
) -> Option<ProgressTransition> {
    let (from_field, from_progress) = from.progress()?;
    let (to_field, to_progress) = to.progress()?;
    if from_field != to_field || to.observed_micros <= from.observed_micros {
        return None;
    }
    let end = boundary.end_micros();
    let phase = if to.observed_micros <= boundary.start_observed_micros {
        TransitionPhase::Before
    } else if from.observed_micros < boundary.start_observed_micros {
        TransitionPhase::StartBoundary
    } else if to.observed_micros <= end {
        TransitionPhase::Inside
    } else if from.observed_micros < end {
        TransitionPhase::EndBoundary
    } else {
        TransitionPhase::After
    };
    let game_time_delta_millis = from
        .game_time_millis
        .zip(to.game_time_millis)
        .map(|(from, to)| to.saturating_sub(from));
    let progress_delta_raw = to_progress.saturating_sub(from_progress);
    let progress_per_game_millisecond = game_time_delta_millis
        .filter(|delta| *delta > 0)
        .map(|delta| progress_delta_raw as f64 / delta as f64);
    let begin_time_unchanged = from.begin_time_millis == to.begin_time_millis;
    let duration_unchanged = from.duration_millis == to.duration_millis;
    let cooldown_type_unchanged = from.cooldown_type == to.cooldown_type;
    let progress_did_not_rewind = progress_delta_raw >= 0;
    let usable_for_continuous_slope_proof = begin_time_unchanged
        && duration_unchanged
        && cooldown_type_unchanged
        && progress_did_not_rewind
        && game_time_delta_millis.is_some_and(|delta| delta > 0);
    let progress_inside_positive_duration_at_both_samples =
        match (from.duration_millis, to.duration_millis) {
            (Some(from_duration), Some(to_duration)) if from_duration > 0 && to_duration > 0 => {
                from_progress >= 0
                    && from_progress < i64::from(from_duration)
                    && to_progress >= 0
                    && to_progress < i64::from(to_duration)
            }
            _ => false,
        };
    let completed_at_to_sample = to
        .duration_millis
        .is_some_and(|duration| duration > 0 && to_progress >= i64::from(duration));
    let usable_for_active_cooldown_slope_proof = usable_for_continuous_slope_proof
        && progress_inside_positive_duration_at_both_samples
        && !completed_at_to_sample;

    Some(ProgressTransition {
        phase,
        progress_field: from_field,
        from: from.clone(),
        to: to.clone(),
        observed_delta_micros: to.observed_micros.saturating_sub(from.observed_micros),
        game_time_delta_millis,
        progress_delta_raw,
        progress_per_game_millisecond,
        begin_time_unchanged,
        duration_unchanged,
        cooldown_type_unchanged,
        progress_did_not_rewind,
        usable_for_continuous_slope_proof,
        progress_inside_positive_duration_at_both_samples,
        completed_at_to_sample,
        usable_for_active_cooldown_slope_proof,
    })
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

    let boundary_margin_millis = if let Some(position) = values
        .iter()
        .position(|value| value == "--boundary-margin-ms")
    {
        if position + 1 >= values.len() {
            return Err("--boundary-margin-ms requires an unsigned integer".to_owned());
        }
        let parsed = values
            .remove(position + 1)
            .to_string_lossy()
            .parse::<u64>()
            .map_err(|_| "--boundary-margin-ms requires an unsigned integer".to_owned())?;
        values.remove(position);
        parsed
    } else {
        DEFAULT_BOUNDARY_MARGIN_MILLIS
    };

    if values.is_empty() {
        return Err(usage());
    }
    Ok(Arguments {
        effect_id,
        boundary_margin_millis,
        output,
        rlogs: values.into_iter().map(PathBuf::from).collect(),
    })
}

fn usage() -> String {
    "usage: rlogs-bpsr-cooldown-transition-proof --effect <status-effect-id> --output <proof.json> [--boundary-margin-ms <ms>] <sealed.rlog>...".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(observed_micros: u64, game_time_millis: i64, progress: i32) -> RawCooldownSample {
        RawCooldownSample {
            envelope_sequence: observed_micros,
            timeline_sequence: observed_micros,
            observed_micros,
            game_time_millis: Some(game_time_millis),
            actor_entity_uuid: 7,
            ability_id: 42,
            begin_time_millis: Some(1),
            duration_millis: Some(30_000),
            valid_duration_millis: Some(progress),
            cooldown_type: Some(1),
            profession_hold_begin_time_millis: None,
            charge_count: None,
            valid_cooldown_time_millis: None,
            sub_cooldown_ratio_raw: None,
            sub_cooldown_fixed_raw: None,
            accelerate_cooldown_ratio_raw: None,
        }
    }

    fn boundary() -> EffectBoundary {
        EffectBoundary {
            source_entity_uuid: Some(9),
            target_entity_uuid: 7,
            instance_id: Some(1),
            start_envelope_sequence: 2,
            start_timeline_sequence: 2,
            start_observed_micros: 2_000_000,
            start_game_time_millis: Some(2_000),
            duration_millis: Some(2_000),
            expected_end_observed_micros: Some(4_000_000),
            observed_end_envelope_sequence: None,
            observed_end_timeline_sequence: None,
            observed_end_micros: None,
            observed_end_game_time_millis: None,
            end_state: None,
        }
    }

    #[test]
    fn transitions_are_partitioned_around_exact_boundaries() {
        let boundary = boundary();
        assert_eq!(
            transition(
                &boundary,
                &sample(1_000_000, 1_000, 100),
                &sample(1_500_000, 1_500, 600)
            )
            .unwrap()
            .phase,
            TransitionPhase::Before
        );
        assert_eq!(
            transition(
                &boundary,
                &sample(1_500_000, 1_500, 600),
                &sample(2_500_000, 2_500, 2_100)
            )
            .unwrap()
            .phase,
            TransitionPhase::StartBoundary
        );
        let inside = transition(
            &boundary,
            &sample(2_500_000, 2_500, 2_100),
            &sample(3_500_000, 3_500, 3_600),
        )
        .unwrap();
        assert_eq!(inside.phase, TransitionPhase::Inside);
        assert_eq!(inside.progress_delta_raw, 1_500);
        assert_eq!(inside.progress_per_game_millisecond, Some(1.5));
        assert!(inside.usable_for_continuous_slope_proof);
        assert!(inside.usable_for_active_cooldown_slope_proof);
        assert_eq!(
            transition(
                &boundary,
                &sample(3_500_000, 3_500, 3_600),
                &sample(4_500_000, 4_500, 4_600)
            )
            .unwrap()
            .phase,
            TransitionPhase::EndBoundary
        );
    }

    #[test]
    fn overlapping_status_observations_become_one_effect_interval() {
        let first = boundary();
        let mut refresh = boundary();
        refresh.start_observed_micros = 3_000_000;
        refresh.expected_end_observed_micros = Some(5_000_000);
        refresh.start_envelope_sequence = 3;
        let merged = merge_boundaries(vec![refresh, first]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].start_observed_micros, 2_000_000);
        assert_eq!(merged[0].expected_end_observed_micros, Some(5_000_000));
        assert_eq!(merged[0].duration_millis, Some(3_000));
    }

    #[test]
    fn server_completion_snap_is_retained_but_censored_from_slope_proof() {
        let transition = transition(
            &boundary(),
            &sample(2_500_000, 2_500, 2_100),
            &sample(3_500_000, 3_500, 30_000),
        )
        .unwrap();

        assert_eq!(transition.progress_delta_raw, 27_900);
        assert!(transition.usable_for_continuous_slope_proof);
        assert!(transition.completed_at_to_sample);
        assert!(!transition.progress_inside_positive_duration_at_both_samples);
        assert!(!transition.usable_for_active_cooldown_slope_proof);
    }
}
