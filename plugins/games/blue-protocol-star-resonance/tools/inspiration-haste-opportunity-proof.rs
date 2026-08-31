use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    env,
    ffi::OsString,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{
    CanonicalEvent, CastState, EntityAttributeValue, RunState, StatusState, TimelineEventKind,
};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::Serialize;
use serde_json::Value;

const SCHEMA_VERSION: u16 = 3;
const DEFAULT_STATUS_EFFECT_ID: i64 = 2_202_041;
const ATTACK_SPEED_ATTRIBUTE_ID: i32 = 11_720;
const CAST_SPEED_ATTRIBUTE_ID: i32 = 11_730;
const CHARGE_SPEED_ATTRIBUTE_ID: i32 = 11_740;
const HASTE_ATTRIBUTE_ID: i32 = 11_930;
const DEFAULT_MAX_INTERVAL_MILLIS: u64 = 15_000;

#[derive(Debug)]
struct Arguments {
    expected_game_build: String,
    effect_id: i64,
    skill_table: PathBuf,
    rlogs: Vec<PathBuf>,
    output: PathBuf,
    max_interval_micros: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
struct SkillSpeedSignal {
    direct_skill_row: bool,
    attack_speed_switch: bool,
    nonzero_sing_or_guide_time: bool,
}

#[derive(Debug, Clone)]
struct StatusWindow {
    provider_entity_uuid: Option<i64>,
    instance_id: Option<i64>,
}

#[derive(Debug, Clone)]
struct CastSnapshot {
    observed_micros: u64,
    provider_scope: ProviderScope,
    speed_attributes: SpeedAttributes,
}

#[derive(Debug, Clone, Default, Serialize)]
struct ProviderScope {
    active_window_count: usize,
    known_external_provider_entity_uuids: BTreeSet<i64>,
    unknown_provider_window_count: usize,
    self_provider_window_count: usize,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
struct SpeedAttributes {
    attack_speed: Option<i64>,
    cast_speed: Option<i64>,
    charge_speed: Option<i64>,
    haste: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum IntervalScope {
    ExternalWindow,
    NoExternalWindow,
    TransitionOverlap,
    UnresolvedProviderOrSelfWindow,
}

#[derive(Debug, Default)]
struct IntervalAccumulator {
    count: u64,
    total_micros: u128,
    minimum_micros: Option<u64>,
    maximum_micros: Option<u64>,
    millisecond_histogram: BTreeMap<u64, u64>,
    examples: Vec<IntervalExample>,
}

#[derive(Debug, Clone, Serialize)]
struct IntervalExample {
    session_id: String,
    run_ordinal: u32,
    source_entity_uuid: i64,
    ability_id: i64,
    previous_observed_micros: u64,
    current_observed_micros: u64,
    interval_micros: u64,
    external_provider_entity_uuids: Vec<i64>,
    previous_speed_attributes: SpeedAttributes,
    current_speed_attributes: SpeedAttributes,
    previous_provider_scope: ProviderScope,
    current_provider_scope: ProviderScope,
}

#[derive(Debug, Default)]
struct AbilityAccumulator {
    sources: BTreeSet<i64>,
    external: IntervalAccumulator,
    absent: IntervalAccumulator,
    transition: IntervalAccumulator,
    unresolved: IntervalAccumulator,
}

#[derive(Debug, Serialize)]
struct AuditBundle {
    schema_version: u16,
    generated_by: &'static str,
    build_scope: BuildScope,
    policy: AuditPolicy,
    effect_id: i64,
    max_interval_millis: u64,
    summary: AuditSummary,
    sessions: Vec<SessionReport>,
    abilities: Vec<AbilityReport>,
}

#[derive(Debug, Serialize)]
struct AuditSummary {
    sessions: usize,
    status_windows_started: u64,
    unknown_provider_windows_started: u64,
    self_provider_windows_started: u64,
    cast_start_events: u64,
    retained_intervals: u64,
    abilities_with_intervals: usize,
    external_intervals: u64,
    no_external_intervals: u64,
    transition_overlap_intervals: u64,
    unresolved_provider_or_self_intervals: u64,
    local_cast_start_coverage_observed: bool,
    opportunity_proof_eligible: bool,
    provider_rdps_credit_allowed: bool,
    zero_cast_interpretation: &'static str,
}

#[derive(Debug, Serialize)]
struct BuildScope {
    expected_game_build: String,
    recording_build_identity_authority: bool,
    recording_build_identity_policy: &'static str,
}

#[derive(Debug, Serialize)]
struct AuditPolicy {
    runtime_formula_authority: bool,
    current_build_static_metadata_is_runtime_authority: bool,
    unresolved_evidence_hidden: bool,
    remote_player_packets_required: bool,
    missing_provider_is_no_external_status: bool,
    no_external_status_requirement: &'static str,
    interval_classification: &'static str,
    promotion_requirement: &'static str,
}

#[derive(Debug, Serialize)]
struct SessionReport {
    rlog: String,
    session_id: String,
    run_ordinals_observed: u32,
    status_windows_started: u64,
    unknown_provider_windows_started: u64,
    self_provider_windows_started: u64,
    status_windows_ended: u64,
    cast_events: u64,
    retained_intervals: u64,
    rejected_long_or_reversed_intervals: u64,
}

#[derive(Debug, Serialize)]
struct AbilityReport {
    ability_id: i64,
    skill_speed_signal: SkillSpeedSignal,
    source_entity_uuids: Vec<i64>,
    external_status: IntervalReport,
    no_external_status: IntervalReport,
    transition_overlap: IntervalReport,
    unresolved_provider_or_self_status: IntervalReport,
}

#[derive(Debug, Serialize)]
struct IntervalReport {
    count: u64,
    minimum_millis: Option<f64>,
    maximum_millis: Option<f64>,
    mean_millis: Option<f64>,
    most_common_millisecond_buckets: Vec<HistogramBucket>,
    examples: Vec<IntervalExample>,
}

#[derive(Debug, Serialize)]
struct HistogramBucket {
    interval_millis: u64,
    count: u64,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Haste opportunity proof failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = arguments()?;
    let skill_signals = load_skill_signals(&args.skill_table)?;
    let mut accumulators = BTreeMap::<i64, AbilityAccumulator>::new();
    let mut sessions = Vec::new();
    for rlog in &args.rlogs {
        sessions.push(read_session(
            rlog,
            args.effect_id,
            args.max_interval_micros,
            &mut accumulators,
        )?);
    }

    let abilities = accumulators
        .into_iter()
        .map(|(ability_id, accumulator)| AbilityReport {
            ability_id,
            skill_speed_signal: skill_signals.get(&ability_id).cloned().unwrap_or_default(),
            source_entity_uuids: accumulator.sources.into_iter().collect(),
            external_status: accumulator.external.into_report(),
            no_external_status: accumulator.absent.into_report(),
            transition_overlap: accumulator.transition.into_report(),
            unresolved_provider_or_self_status: accumulator.unresolved.into_report(),
        })
        .collect::<Vec<_>>();

    let cast_start_events = sessions.iter().map(|row| row.cast_events).sum::<u64>();
    let external_intervals = abilities
        .iter()
        .map(|row| row.external_status.count)
        .sum::<u64>();
    let no_external_intervals = abilities
        .iter()
        .map(|row| row.no_external_status.count)
        .sum::<u64>();
    let transition_overlap_intervals = abilities
        .iter()
        .map(|row| row.transition_overlap.count)
        .sum::<u64>();
    let unresolved_provider_or_self_intervals = abilities
        .iter()
        .map(|row| row.unresolved_provider_or_self_status.count)
        .sum::<u64>();
    let summary = AuditSummary {
        sessions: sessions.len(),
        status_windows_started: sessions.iter().map(|row| row.status_windows_started).sum(),
        unknown_provider_windows_started: sessions
            .iter()
            .map(|row| row.unknown_provider_windows_started)
            .sum(),
        self_provider_windows_started: sessions
            .iter()
            .map(|row| row.self_provider_windows_started)
            .sum(),
        cast_start_events,
        retained_intervals: sessions.iter().map(|row| row.retained_intervals).sum(),
        abilities_with_intervals: abilities.len(),
        external_intervals,
        no_external_intervals,
        transition_overlap_intervals,
        unresolved_provider_or_self_intervals,
        local_cast_start_coverage_observed: cast_start_events > 0,
        opportunity_proof_eligible: cast_start_events > 0 && external_intervals > 0,
        provider_rdps_credit_allowed: false,
        zero_cast_interpretation: "unobserved action-start coverage; never zero actions, zero opportunity, or mechanic disproof",
    };

    let bundle = AuditBundle {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-haste-opportunity-proof",
        build_scope: BuildScope {
            expected_game_build: args.expected_game_build,
            recording_build_identity_authority: false,
            recording_build_identity_policy: "the expected build is caller-declared cohort scope; runtime promotion still requires exact protocol-pack identity and replay coverage",
        },
        policy: AuditPolicy {
            runtime_formula_authority: false,
            current_build_static_metadata_is_runtime_authority: false,
            unresolved_evidence_hidden: false,
            remote_player_packets_required: false,
            missing_provider_is_no_external_status: false,
            no_external_status_requirement: "both cast endpoints must have zero active target status windows; an active window with an absent provider is unresolved and never absence",
            interval_classification: "same actor and ability inside one run; both endpoints must have the same non-empty exact external status provider set and matching active-window counts, both must have zero active status windows, provider-set changes remain transition overlap, and missing-provider or self/mixed windows remain separately unresolved",
            promotion_requirement: "Each action must carry an authenticated matching-build attack/cast/charge-speed snapshot, select one exact downstream stage-speed family, retain the exact external provider delta, and join to one conserved action-to-damage recount parent. Credit is the provider's exact speed-capacity share of already observed linked damage; the proof must not invent extra actions or substitute hit counts for action starts. Aggregate interval correlation alone cannot enable rDPS.",
        },
        effect_id: args.effect_id,
        max_interval_millis: args.max_interval_micros / 1_000,
        summary,
        sessions,
        abilities,
    };
    let mut writer = BufWriter::new(File::create(&args.output)?);
    serde_json::to_writer_pretty(&mut writer, &bundle)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn read_session(
    rlog: &Path,
    effect_id: i64,
    max_interval_micros: u64,
    accumulators: &mut BTreeMap<i64, AbilityAccumulator>,
) -> Result<SessionReport, Box<dyn std::error::Error>> {
    let mut reader = RlogReader::new(BufReader::new(File::open(rlog)?), RlogLimits::default())?;
    let mut attributes = HashMap::<i64, BTreeMap<i32, i64>>::new();
    let mut windows = HashMap::<i64, Vec<StatusWindow>>::new();
    let mut last_casts = HashMap::<(u32, i64, i64), CastSnapshot>::new();
    let mut session_id = String::new();
    let mut run_ordinal = 0_u32;
    let mut started = 0_u64;
    let mut unknown_provider_started = 0_u64;
    let mut self_provider_started = 0_u64;
    let mut ended = 0_u64;
    let mut casts = 0_u64;
    let mut retained = 0_u64;
    let mut rejected = 0_u64;

    while let Some(envelope) = reader.next_event()? {
        session_id = envelope.session_id.clone();
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            continue;
        };
        match &timeline.kind {
            TimelineEventKind::RunBoundary { state, .. }
                if matches!(state, RunState::Entered | RunState::Started) =>
            {
                if *state == RunState::Entered || run_ordinal == 0 {
                    run_ordinal = run_ordinal.saturating_add(1);
                    last_casts.clear();
                    windows.clear();
                }
            }
            TimelineEventKind::EntityAttributes(update) => {
                let values = attributes.entry(update.actor.entity_uuid.0).or_default();
                for attribute in &update.attributes {
                    if !matches!(
                        attribute.attribute_id,
                        ATTACK_SPEED_ATTRIBUTE_ID
                            | CAST_SPEED_ATTRIBUTE_ID
                            | CHARGE_SPEED_ATTRIBUTE_ID
                            | HASTE_ATTRIBUTE_ID
                    ) {
                        continue;
                    }
                    if let Some(EntityAttributeValue::Integer(value)) = attribute.decoded {
                        values.insert(attribute.attribute_id, value);
                    }
                }
            }
            TimelineEventKind::Status(status) if status.effect.0 == effect_id => {
                let target = status.target.entity_uuid.0;
                let target_windows = windows.entry(target).or_default();
                match status.state {
                    StatusState::Applied | StatusState::Refreshed | StatusState::Stacked => {
                        if let Some(instance_id) = status.instance_id.map(|value| value.0) {
                            target_windows.retain(|window| window.instance_id != Some(instance_id));
                        }
                        target_windows.push(StatusWindow {
                            provider_entity_uuid: status.source.map(|source| source.entity_uuid.0),
                            instance_id: status.instance_id.map(|value| value.0),
                        });
                        match status.source.map(|source| source.entity_uuid.0) {
                            None => unknown_provider_started += 1,
                            Some(provider) if provider == target => self_provider_started += 1,
                            Some(_) => {}
                        }
                        started += 1;
                    }
                    StatusState::Consumed | StatusState::Removed => {
                        if let Some(instance_id) = status.instance_id.map(|value| value.0) {
                            target_windows.retain(|window| window.instance_id != Some(instance_id));
                        } else if let Some(provider) =
                            status.source.map(|source| source.entity_uuid.0)
                        {
                            target_windows
                                .retain(|window| window.provider_entity_uuid != Some(provider));
                        } else {
                            target_windows.clear();
                        }
                        ended += 1;
                    }
                }
            }
            TimelineEventKind::Cast(cast) if cast.state == CastState::Started => {
                casts += 1;
                let source = cast.source.entity_uuid.0;
                let ability = cast.ability.0;
                let provider_scope = provider_scope(source, windows.get(&source));
                let current = CastSnapshot {
                    observed_micros: envelope.time.observed_micros,
                    provider_scope,
                    speed_attributes: speed_attributes(attributes.get(&source)),
                };
                let key = (run_ordinal, source, ability);
                let previous = last_casts.insert(key, current.clone());
                let Some(previous) = previous else {
                    accumulators
                        .entry(ability)
                        .or_default()
                        .sources
                        .insert(source);
                    continue;
                };
                let Some(interval) = current
                    .observed_micros
                    .checked_sub(previous.observed_micros)
                    .filter(|interval| *interval <= max_interval_micros)
                else {
                    rejected += 1;
                    continue;
                };
                let scope = classify_interval(&previous.provider_scope, &current.provider_scope);
                let example = IntervalExample {
                    session_id: session_id.clone(),
                    run_ordinal,
                    source_entity_uuid: source,
                    ability_id: ability,
                    previous_observed_micros: previous.observed_micros,
                    current_observed_micros: current.observed_micros,
                    interval_micros: interval,
                    external_provider_entity_uuids: current
                        .provider_scope
                        .known_external_provider_entity_uuids
                        .iter()
                        .copied()
                        .collect(),
                    previous_speed_attributes: previous.speed_attributes,
                    current_speed_attributes: current.speed_attributes,
                    previous_provider_scope: previous.provider_scope.clone(),
                    current_provider_scope: current.provider_scope.clone(),
                };
                let accumulator = accumulators.entry(ability).or_default();
                accumulator.sources.insert(source);
                match scope {
                    IntervalScope::ExternalWindow => {
                        accumulator.external.observe(interval, example)
                    }
                    IntervalScope::NoExternalWindow => {
                        accumulator.absent.observe(interval, example)
                    }
                    IntervalScope::TransitionOverlap => {
                        accumulator.transition.observe(interval, example)
                    }
                    IntervalScope::UnresolvedProviderOrSelfWindow => {
                        accumulator.unresolved.observe(interval, example)
                    }
                }
                retained += 1;
            }
            _ => {}
        }
    }

    Ok(SessionReport {
        rlog: rlog.display().to_string(),
        session_id,
        run_ordinals_observed: run_ordinal,
        status_windows_started: started,
        unknown_provider_windows_started: unknown_provider_started,
        self_provider_windows_started: self_provider_started,
        status_windows_ended: ended,
        cast_events: casts,
        retained_intervals: retained,
        rejected_long_or_reversed_intervals: rejected,
    })
}

fn classify_interval(previous: &ProviderScope, current: &ProviderScope) -> IntervalScope {
    if previous.unknown_provider_window_count > 0
        || current.unknown_provider_window_count > 0
        || previous.self_provider_window_count > 0
        || current.self_provider_window_count > 0
    {
        IntervalScope::UnresolvedProviderOrSelfWindow
    } else if previous.active_window_count == 0 && current.active_window_count == 0 {
        IntervalScope::NoExternalWindow
    } else if previous.active_window_count == current.active_window_count
        && !previous.known_external_provider_entity_uuids.is_empty()
        && previous.known_external_provider_entity_uuids
            == current.known_external_provider_entity_uuids
    {
        IntervalScope::ExternalWindow
    } else {
        IntervalScope::TransitionOverlap
    }
}

fn provider_scope(target: i64, windows: Option<&Vec<StatusWindow>>) -> ProviderScope {
    let mut scope = ProviderScope::default();
    for window in windows.into_iter().flatten() {
        scope.active_window_count += 1;
        match window.provider_entity_uuid {
            None => scope.unknown_provider_window_count += 1,
            Some(provider) if provider == target => scope.self_provider_window_count += 1,
            Some(provider) => {
                scope.known_external_provider_entity_uuids.insert(provider);
            }
        }
    }
    scope
}

fn speed_attributes(values: Option<&BTreeMap<i32, i64>>) -> SpeedAttributes {
    let get = |id| values.and_then(|values| values.get(&id)).copied();
    SpeedAttributes {
        attack_speed: get(ATTACK_SPEED_ATTRIBUTE_ID),
        cast_speed: get(CAST_SPEED_ATTRIBUTE_ID),
        charge_speed: get(CHARGE_SPEED_ATTRIBUTE_ID),
        haste: get(HASTE_ATTRIBUTE_ID),
    }
}

impl IntervalAccumulator {
    fn observe(&mut self, interval_micros: u64, example: IntervalExample) {
        self.count += 1;
        self.total_micros += u128::from(interval_micros);
        self.minimum_micros = Some(
            self.minimum_micros
                .map_or(interval_micros, |v| v.min(interval_micros)),
        );
        self.maximum_micros = Some(
            self.maximum_micros
                .map_or(interval_micros, |v| v.max(interval_micros)),
        );
        *self
            .millisecond_histogram
            .entry(interval_micros / 1_000)
            .or_default() += 1;
        if self.examples.len() < 12 {
            self.examples.push(example);
        }
    }

    fn into_report(self) -> IntervalReport {
        let mut buckets = self
            .millisecond_histogram
            .into_iter()
            .map(|(interval_millis, count)| HistogramBucket {
                interval_millis,
                count,
            })
            .collect::<Vec<_>>();
        buckets.sort_by_key(|bucket| (std::cmp::Reverse(bucket.count), bucket.interval_millis));
        buckets.truncate(12);
        IntervalReport {
            count: self.count,
            minimum_millis: self.minimum_micros.map(|value| value as f64 / 1_000.0),
            maximum_millis: self.maximum_micros.map(|value| value as f64 / 1_000.0),
            mean_millis: (self.count > 0)
                .then(|| self.total_micros as f64 / self.count as f64 / 1_000.0),
            most_common_millisecond_buckets: buckets,
            examples: self.examples,
        }
    }
}

fn load_skill_signals(
    path: &Path,
) -> Result<HashMap<i64, SkillSpeedSignal>, Box<dyn std::error::Error>> {
    let rows: Value = serde_json::from_reader(BufReader::new(File::open(path)?))?;
    let object = rows.as_object().ok_or("SkillTable must be a JSON object")?;
    let mut signals = HashMap::new();
    for (key, row) in object {
        let Some(id) = row
            .get("Id")
            .and_then(Value::as_i64)
            .or_else(|| key.parse().ok())
        else {
            continue;
        };
        signals.insert(
            id,
            SkillSpeedSignal {
                direct_skill_row: true,
                attack_speed_switch: row
                    .get("AtkSpeedSwitch")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                nonzero_sing_or_guide_time: contains_nonzero_number(
                    row.get("SingOrGuideTime").unwrap_or(&Value::Null),
                ),
            },
        );
    }
    Ok(signals)
}

fn contains_nonzero_number(value: &Value) -> bool {
    match value {
        Value::Number(number) => number.as_f64().is_some_and(|value| value != 0.0),
        Value::Array(values) => values.iter().any(contains_nonzero_number),
        _ => false,
    }
}

fn arguments() -> Result<Arguments, String> {
    let mut values = env::args_os().skip(1).collect::<Vec<_>>();
    let expected_game_build = take_value(&mut values, "--expected-game-build")?
        .to_string_lossy()
        .into_owned();
    if expected_game_build.is_empty()
        || !expected_game_build
            .bytes()
            .all(|byte| byte.is_ascii_digit())
    {
        return Err("--expected-game-build requires a numeric client build".to_owned());
    }
    let effect_id = take_optional_value(&mut values, "--effect")
        .map(|value| parse_i64(value, "--effect"))
        .transpose()?
        .unwrap_or(DEFAULT_STATUS_EFFECT_ID);
    let skill_table = PathBuf::from(take_value(&mut values, "--skill-table")?);
    let output = PathBuf::from(take_value(&mut values, "--output")?);
    let max_interval_millis = take_optional_value(&mut values, "--max-interval-millis")
        .map(|value| parse_u64(value, "--max-interval-millis"))
        .transpose()?
        .unwrap_or(DEFAULT_MAX_INTERVAL_MILLIS);
    let mut rlogs = Vec::new();
    while let Some(value) = take_optional_value(&mut values, "--rlog") {
        rlogs.push(PathBuf::from(value));
    }
    if rlogs.is_empty() || !values.is_empty() {
        return Err(usage());
    }
    Ok(Arguments {
        expected_game_build,
        effect_id,
        skill_table,
        rlogs,
        output,
        max_interval_micros: max_interval_millis
            .checked_mul(1_000)
            .ok_or("--max-interval-millis is too large")?,
    })
}

fn take_value(values: &mut Vec<OsString>, flag: &str) -> Result<OsString, String> {
    take_optional_value(values, flag).ok_or_else(usage)
}

fn take_optional_value(values: &mut Vec<OsString>, flag: &str) -> Option<OsString> {
    let position = values.iter().position(|value| value == flag)?;
    if position + 1 >= values.len() {
        return None;
    }
    let value = values.remove(position + 1);
    values.remove(position);
    Some(value)
}

fn parse_u64(value: OsString, flag: &str) -> Result<u64, String> {
    value
        .to_string_lossy()
        .parse::<u64>()
        .map_err(|_| format!("{flag} requires a non-negative integer"))
}

fn parse_i64(value: OsString, flag: &str) -> Result<i64, String> {
    value
        .to_string_lossy()
        .parse::<i64>()
        .map_err(|_| format!("{flag} requires an integer"))
}

fn usage() -> String {
    "usage: rlogs-bpsr-inspiration-haste-opportunity-proof --expected-game-build <numeric-build> [--effect <status-effect-id>] --skill-table <SkillTable.json> --rlog <sealed.rlog> [--rlog <sealed.rlog> ...] --output <proof.json> [--max-interval-millis <n>]".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(
        active_window_count: usize,
        providers: impl IntoIterator<Item = i64>,
        unknown_provider_window_count: usize,
        self_provider_window_count: usize,
    ) -> ProviderScope {
        ProviderScope {
            active_window_count,
            known_external_provider_entity_uuids: providers.into_iter().collect(),
            unknown_provider_window_count,
            self_provider_window_count,
        }
    }

    #[test]
    fn interval_scope_requires_the_same_exact_external_provider_set() {
        assert_eq!(
            classify_interval(&scope(0, [], 0, 0), &scope(0, [], 0, 0)),
            IntervalScope::NoExternalWindow
        );
        assert_eq!(
            classify_interval(&scope(1, [7], 0, 0), &scope(1, [7], 0, 0)),
            IntervalScope::ExternalWindow
        );
        assert_eq!(
            classify_interval(&scope(0, [], 0, 0), &scope(1, [7], 0, 0)),
            IntervalScope::TransitionOverlap
        );
        assert_eq!(
            classify_interval(&scope(1, [7], 0, 0), &scope(1, [8], 0, 0)),
            IntervalScope::TransitionOverlap
        );
        assert_eq!(
            classify_interval(&scope(1, [7], 0, 0), &scope(2, [7], 0, 0)),
            IntervalScope::TransitionOverlap
        );
    }

    #[test]
    fn absent_or_self_provider_never_becomes_no_external_status() {
        assert_eq!(
            classify_interval(&scope(1, [], 1, 0), &scope(1, [], 1, 0)),
            IntervalScope::UnresolvedProviderOrSelfWindow
        );
        assert_eq!(
            classify_interval(&scope(1, [], 0, 1), &scope(1, [], 0, 1)),
            IntervalScope::UnresolvedProviderOrSelfWindow
        );
        let windows = vec![StatusWindow {
            provider_entity_uuid: None,
            instance_id: Some(9),
        }];
        let observed = provider_scope(42, Some(&windows));
        assert_eq!(observed.active_window_count, 1);
        assert_eq!(observed.unknown_provider_window_count, 1);
        assert!(observed.known_external_provider_entity_uuids.is_empty());
    }

    #[test]
    fn nested_singing_metadata_detects_only_nonzero_values() {
        assert!(!contains_nonzero_number(&serde_json::json!([[0.0], []])));
        assert!(contains_nonzero_number(&serde_json::json!([[0.0], [1.5]])));
    }
}
