use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    env,
    ffi::OsString,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{CanonicalEvent, EntityAttributeValue, LifeState, RunState, TimelineEventKind};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::Serialize;
use serde_json::Value;

const CURRENT_HP_ATTRIBUTE_ID: i32 = 11_310;
const MAX_HP_ATTRIBUTE_ID: i32 = 11_320;
const SCHEMA_VERSION: u16 = 2;
const DEFAULT_EXAMPLE_LIMIT: usize = 24;

#[derive(Debug)]
struct Arguments {
    rlogs: Vec<PathBuf>,
    output: PathBuf,
    example_limit: usize,
    selected_actions: Option<PathBuf>,
}

#[derive(Debug, Clone, Default)]
struct LedgerState {
    authoritative_hp: Option<i64>,
    predicted_hp: Option<i64>,
    max_hp: Option<i64>,
    baseline_sequence: Option<u64>,
    damage_events: u64,
    damage_hp_loss: i64,
    damage_without_hp_loss: u64,
    healing_events: u64,
    healing_amount: i64,
    healing_without_max_hp: u64,
    healing_without_effective_amount: u64,
    life_deaths: u64,
    life_revives: u64,
    max_hp_changes: u64,
    invalidated: bool,
}

impl LedgerState {
    fn reset_interval(&mut self, sequence: u64, current_hp: i64, max_hp: Option<i64>) {
        self.authoritative_hp = Some(current_hp);
        self.predicted_hp = Some(current_hp);
        self.max_hp = max_hp.or(self.max_hp);
        self.baseline_sequence = Some(sequence);
        self.damage_events = 0;
        self.damage_hp_loss = 0;
        self.damage_without_hp_loss = 0;
        self.healing_events = 0;
        self.healing_amount = 0;
        self.healing_without_max_hp = 0;
        self.healing_without_effective_amount = 0;
        self.life_deaths = 0;
        self.life_revives = 0;
        self.max_hp_changes = 0;
        self.invalidated = false;
    }

    fn observe_damage(&mut self, hp_loss: Option<i64>) {
        self.damage_events = self.damage_events.saturating_add(1);
        let Some(hp_loss) = hp_loss else {
            self.damage_without_hp_loss = self.damage_without_hp_loss.saturating_add(1);
            return;
        };
        self.damage_hp_loss = self.damage_hp_loss.saturating_add(hp_loss);
        if let Some(current) = self.predicted_hp {
            self.predicted_hp = Some(current.saturating_sub(hp_loss).max(0));
        }
    }

    fn observe_healing(&mut self, effective_amount: Option<i64>, reported_amount: i64) {
        self.healing_events = self.healing_events.saturating_add(1);
        if effective_amount.is_none() {
            self.healing_without_effective_amount =
                self.healing_without_effective_amount.saturating_add(1);
        }
        let amount = effective_amount.unwrap_or(reported_amount);
        self.healing_amount = self.healing_amount.saturating_add(amount);
        let (Some(current), Some(maximum)) = (self.predicted_hp, self.max_hp) else {
            self.healing_without_max_hp = self.healing_without_max_hp.saturating_add(1);
            self.invalidated = true;
            return;
        };
        self.predicted_hp = Some(current.saturating_add(amount).min(maximum).max(0));
    }

    fn observe_life(&mut self, state: LifeState) {
        match state {
            LifeState::Died => {
                self.life_deaths = self.life_deaths.saturating_add(1);
                self.predicted_hp = Some(0);
            }
            LifeState::Revived => {
                self.life_revives = self.life_revives.saturating_add(1);
                self.invalidated = true;
                self.predicted_hp = None;
            }
        }
    }
}

#[derive(Debug, Default)]
struct AuditAccumulator {
    current_hp_snapshots: u64,
    intervals_compared: u64,
    eligible_intervals: u64,
    eligible_exact: u64,
    eligible_mismatched: u64,
    invalidated_intervals: u64,
    no_event_intervals: u64,
    damage_events: u64,
    damage_events_with_hp_loss: u64,
    damage_events_without_hp_loss: u64,
    healing_events: u64,
    life_events: u64,
    residual_counts: BTreeMap<i64, u64>,
    mismatch_examples: Vec<IntervalExample>,
    exact_examples: Vec<IntervalExample>,
}

#[derive(Debug, Serialize)]
struct AuditBundle {
    schema_version: u16,
    generated_by: &'static str,
    policy: AuditPolicy,
    sessions: Vec<SessionReport>,
    aggregate: Coverage,
    selected_action_hp_context: Option<SelectedActionReport>,
}

#[derive(Debug, Serialize)]
struct AuditPolicy {
    runtime_authority: bool,
    current_hp_attribute_id: i32,
    max_hp_attribute_id: i32,
    damage_transition: &'static str,
    healing_transition_under_test: &'static str,
    life_transition: &'static str,
    eligibility: &'static str,
    selected_action_formula_eligibility: &'static str,
    unresolved_intervals_hidden: bool,
}

#[derive(Debug, Serialize)]
struct SessionReport {
    rlog: String,
    session_id: String,
    coverage: Coverage,
}

#[derive(Debug, Clone, Default, Serialize)]
struct Coverage {
    current_hp_snapshots: u64,
    intervals_compared: u64,
    eligible_intervals: u64,
    eligible_exact: u64,
    eligible_mismatched: u64,
    invalidated_intervals: u64,
    no_event_intervals: u64,
    damage_events: u64,
    damage_events_with_hp_loss: u64,
    damage_events_without_hp_loss: u64,
    healing_events: u64,
    life_events: u64,
    exact_rate_basis_points: Option<u64>,
    residual_counts: Vec<ResidualCount>,
    mismatch_examples: Vec<IntervalExample>,
    exact_examples: Vec<IntervalExample>,
}

#[derive(Debug, Clone, Serialize)]
struct ResidualCount {
    residual: i64,
    intervals: u64,
}

#[derive(Debug, Clone, Serialize)]
struct IntervalExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    entity_uuid: i64,
    baseline_sequence: u64,
    observed_sequence: u64,
    baseline_hp: i64,
    predicted_hp: Option<i64>,
    observed_hp: i64,
    residual: Option<i64>,
    max_hp: Option<i64>,
    damage_events: u64,
    damage_hp_loss: i64,
    damage_without_hp_loss: u64,
    healing_events: u64,
    healing_amount: i64,
    healing_without_max_hp: u64,
    healing_without_effective_amount: u64,
    life_deaths: u64,
    life_revives: u64,
    max_hp_changes: u64,
    eligible: bool,
}

#[derive(Debug, Clone)]
struct SelectedActionRequest {
    session_id: String,
    sequence: u64,
    run_ordinal: u32,
    target_entity_uuid: i64,
}

#[derive(Debug, Serialize)]
struct SelectedActionReport {
    source_path: String,
    requested_actions: usize,
    matched_actions: usize,
    missing_action_keys: Vec<String>,
    observations: Vec<SelectedActionObservation>,
}

#[derive(Debug, Serialize)]
struct SelectedActionObservation {
    session_id: String,
    sequence: u64,
    run_ordinal: u32,
    target_entity_uuid: i64,
    baseline_sequence: Option<u64>,
    authoritative_current_hp: Option<i64>,
    max_hp: Option<i64>,
    candidate_pre_hit_current_hp: Option<i64>,
    predicted_pre_hit_current_hp: Option<i64>,
    damage_events_since_snapshot: u64,
    damage_hp_loss_since_snapshot: i64,
    damage_events_without_hp_loss: u64,
    healing_events_since_snapshot: u64,
    healing_effective_amount_since_snapshot: i64,
    healing_events_without_effective_amount: u64,
    healing_events_without_max_hp: u64,
    life_deaths_since_snapshot: u64,
    life_revives_since_snapshot: u64,
    max_hp_changes_since_snapshot: u64,
    interval_closure_sequence: Option<u64>,
    interval_closure_residual: Option<i64>,
    interval_closure_exact: bool,
    formula_context_eligible: bool,
    reconstruction_available: bool,
    unresolved_reasons: Vec<&'static str>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args(env::args_os().skip(1))?;
    let selected_requests = args
        .selected_actions
        .as_deref()
        .map(read_selected_action_requests)
        .transpose()?
        .unwrap_or_default();
    let mut selected_observations = BTreeMap::<(String, u64), SelectedActionObservation>::new();
    let mut aggregate = AuditAccumulator::default();
    let mut sessions = Vec::new();
    for path in &args.rlogs {
        let (session, accumulator) = read_session(
            path,
            args.example_limit,
            &selected_requests,
            &mut selected_observations,
        )?;
        merge_accumulator(&mut aggregate, &accumulator, args.example_limit);
        sessions.push(SessionReport {
            rlog: file_label(path),
            session_id: session,
            coverage: coverage(accumulator),
        });
    }
    for observation in selected_observations.values_mut() {
        if observation.interval_closure_sequence.is_none() {
            observation
                .unresolved_reasons
                .push("no-subsequent-authoritative-current-hp-closure");
        } else if !observation.interval_closure_exact {
            observation
                .unresolved_reasons
                .push("snapshot-interval-transition-model-mismatch");
        }
    }
    let selected_action_hp_context = args.selected_actions.as_ref().map(|source_path| {
        let requested_keys = selected_requests.keys().cloned().collect::<BTreeSet<_>>();
        let matched_keys = selected_observations
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        let missing_action_keys = requested_keys
            .difference(&matched_keys)
            .map(|(session_id, sequence)| format!("{session_id}:{sequence}"))
            .collect::<Vec<_>>();
        SelectedActionReport {
            source_path: source_path.display().to_string(),
            requested_actions: selected_requests.len(),
            matched_actions: selected_observations.len(),
            missing_action_keys,
            observations: selected_observations.into_values().collect(),
        }
    });
    let bundle = AuditBundle {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-hp-state-ledger-proof",
        policy: AuditPolicy {
            runtime_authority: false,
            current_hp_attribute_id: CURRENT_HP_ATTRIBUTE_ID,
            max_hp_attribute_id: MAX_HP_ATTRIBUTE_ID,
            damage_transition: "subtract packet hp_loss after the hit; missing hp_loss makes strict reconstruction unavailable",
            healing_transition_under_test: "add canonical effective healing and cap at the latest authoritative MaxHP; a missing effective amount makes strict reconstruction unavailable",
            life_transition: "death sets zero; revive invalidates the interval until the next authoritative CurrentHP snapshot",
            eligibility: "an interval is eligible only when it has a prediction and no missing hp_loss, missing effective healing amount, MaxHP change, revive, or healing event lacking MaxHP",
            selected_action_formula_eligibility: "a selected pre-hit HP context remains unavailable unless the complete containing snapshot interval closes with zero residual under the strict transition model",
            unresolved_intervals_hidden: false,
        },
        sessions,
        aggregate: coverage(aggregate),
        selected_action_hp_context,
    };
    let mut writer = BufWriter::new(File::create(&args.output)?);
    serde_json::to_writer_pretty(&mut writer, &bundle)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    println!("wrote {}", args.output.display());
    Ok(())
}

fn read_session(
    path: &Path,
    example_limit: usize,
    selected_requests: &BTreeMap<(String, u64), SelectedActionRequest>,
    selected_observations: &mut BTreeMap<(String, u64), SelectedActionObservation>,
) -> Result<(String, AuditAccumulator), Box<dyn std::error::Error>> {
    let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
    let mut states = HashMap::<(u32, i64), LedgerState>::new();
    let mut pending_selected = HashMap::<(u32, i64), Vec<(String, u64)>>::new();
    let mut accumulator = AuditAccumulator::default();
    let mut session_id = None::<String>;
    let mut run_ordinal = 0_u32;
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
                let mut current_hp = None;
                let mut max_hp = None;
                for attribute in &event.attributes {
                    let Some(value) = decode_attribute(attribute) else {
                        continue;
                    };
                    match attribute.attribute_id {
                        CURRENT_HP_ATTRIBUTE_ID => current_hp = Some(value),
                        MAX_HP_ATTRIBUTE_ID => max_hp = Some(value),
                        _ => {}
                    }
                }
                let key = (run_ordinal, event.actor.entity_uuid.0);
                let state = states.entry(key).or_default();
                if let Some(maximum) = max_hp {
                    if state.max_hp.is_some_and(|previous| previous != maximum)
                        && state.baseline_sequence.is_some()
                    {
                        state.max_hp_changes = state.max_hp_changes.saturating_add(1);
                        state.invalidated = true;
                    }
                    state.max_hp = Some(maximum);
                }
                if let Some(observed_hp) = current_hp {
                    accumulator.current_hp_snapshots =
                        accumulator.current_hp_snapshots.saturating_add(1);
                    if let (Some(baseline_sequence), Some(baseline_hp)) =
                        (state.baseline_sequence, state.authoritative_hp)
                    {
                        accumulator.intervals_compared =
                            accumulator.intervals_compared.saturating_add(1);
                        let event_count = state
                            .damage_events
                            .saturating_add(state.healing_events)
                            .saturating_add(state.life_deaths)
                            .saturating_add(state.life_revives)
                            .saturating_add(state.max_hp_changes);
                        if event_count == 0 {
                            accumulator.no_event_intervals =
                                accumulator.no_event_intervals.saturating_add(1);
                        }
                        let eligible = interval_is_strictly_eligible(state);
                        let residual = state
                            .predicted_hp
                            .map(|predicted| observed_hp.saturating_sub(predicted));
                        close_pending_selected_interval(
                            &mut pending_selected,
                            selected_observations,
                            key,
                            envelope.sequence,
                            eligible,
                            residual,
                        );
                        let example = IntervalExample {
                            rlog: file_label(path),
                            session_id: envelope.session_id.clone(),
                            run_ordinal,
                            entity_uuid: event.actor.entity_uuid.0,
                            baseline_sequence,
                            observed_sequence: envelope.sequence,
                            baseline_hp,
                            predicted_hp: state.predicted_hp,
                            observed_hp,
                            residual,
                            max_hp: state.max_hp,
                            damage_events: state.damage_events,
                            damage_hp_loss: state.damage_hp_loss,
                            damage_without_hp_loss: state.damage_without_hp_loss,
                            healing_events: state.healing_events,
                            healing_amount: state.healing_amount,
                            healing_without_max_hp: state.healing_without_max_hp,
                            healing_without_effective_amount: state
                                .healing_without_effective_amount,
                            life_deaths: state.life_deaths,
                            life_revives: state.life_revives,
                            max_hp_changes: state.max_hp_changes,
                            eligible,
                        };
                        if eligible {
                            accumulator.eligible_intervals =
                                accumulator.eligible_intervals.saturating_add(1);
                            let residual = residual.expect("eligible interval has a prediction");
                            *accumulator.residual_counts.entry(residual).or_default() += 1;
                            if residual == 0 {
                                accumulator.eligible_exact =
                                    accumulator.eligible_exact.saturating_add(1);
                                if event_count > 0
                                    && accumulator.exact_examples.len() < example_limit
                                {
                                    accumulator.exact_examples.push(example);
                                }
                            } else {
                                accumulator.eligible_mismatched =
                                    accumulator.eligible_mismatched.saturating_add(1);
                                if accumulator.mismatch_examples.len() < example_limit {
                                    accumulator.mismatch_examples.push(example);
                                }
                            }
                        } else {
                            accumulator.invalidated_intervals =
                                accumulator.invalidated_intervals.saturating_add(1);
                            if accumulator.mismatch_examples.len() < example_limit {
                                accumulator.mismatch_examples.push(example);
                            }
                        }
                    }
                    state.reset_interval(envelope.sequence, observed_hp, max_hp);
                }
            }
            TimelineEventKind::Damage(damage) => {
                accumulator.damage_events = accumulator.damage_events.saturating_add(1);
                if damage.hp_loss.is_some() {
                    accumulator.damage_events_with_hp_loss =
                        accumulator.damage_events_with_hp_loss.saturating_add(1);
                } else {
                    accumulator.damage_events_without_hp_loss =
                        accumulator.damage_events_without_hp_loss.saturating_add(1);
                }
                let key = (envelope.session_id.clone(), envelope.sequence);
                if let Some(request) = selected_requests.get(&key) {
                    let observed_target = damage.target.entity_uuid.0;
                    let state = states
                        .get(&(run_ordinal, observed_target))
                        .cloned()
                        .unwrap_or_default();
                    selected_observations.insert(
                        key.clone(),
                        selected_action_observation(request, run_ordinal, observed_target, &state),
                    );
                    pending_selected
                        .entry((run_ordinal, observed_target))
                        .or_default()
                        .push(key);
                }
                states
                    .entry((run_ordinal, damage.target.entity_uuid.0))
                    .or_default()
                    .observe_damage(damage.hp_loss);
            }
            TimelineEventKind::Healing(healing) => {
                accumulator.healing_events = accumulator.healing_events.saturating_add(1);
                states
                    .entry((run_ordinal, healing.target.entity_uuid.0))
                    .or_default()
                    .observe_healing(healing.effective_amount, healing.amount);
            }
            TimelineEventKind::Life { actor, state } => {
                accumulator.life_events = accumulator.life_events.saturating_add(1);
                states
                    .entry((run_ordinal, actor.entity_uuid.0))
                    .or_default()
                    .observe_life(*state);
            }
            _ => {}
        }
    }
    Ok((
        session_id.unwrap_or_else(|| "unobserved".to_owned()),
        accumulator,
    ))
}

fn coverage(accumulator: AuditAccumulator) -> Coverage {
    let exact_rate_basis_points = (accumulator.eligible_intervals > 0).then(|| {
        accumulator
            .eligible_exact
            .saturating_mul(10_000)
            .saturating_add(accumulator.eligible_intervals / 2)
            / accumulator.eligible_intervals
    });
    Coverage {
        current_hp_snapshots: accumulator.current_hp_snapshots,
        intervals_compared: accumulator.intervals_compared,
        eligible_intervals: accumulator.eligible_intervals,
        eligible_exact: accumulator.eligible_exact,
        eligible_mismatched: accumulator.eligible_mismatched,
        invalidated_intervals: accumulator.invalidated_intervals,
        no_event_intervals: accumulator.no_event_intervals,
        damage_events: accumulator.damage_events,
        damage_events_with_hp_loss: accumulator.damage_events_with_hp_loss,
        damage_events_without_hp_loss: accumulator.damage_events_without_hp_loss,
        healing_events: accumulator.healing_events,
        life_events: accumulator.life_events,
        exact_rate_basis_points,
        residual_counts: accumulator
            .residual_counts
            .into_iter()
            .map(|(residual, intervals)| ResidualCount {
                residual,
                intervals,
            })
            .collect(),
        mismatch_examples: accumulator.mismatch_examples,
        exact_examples: accumulator.exact_examples,
    }
}

fn merge_accumulator(
    target: &mut AuditAccumulator,
    source: &AuditAccumulator,
    example_limit: usize,
) {
    target.current_hp_snapshots = target
        .current_hp_snapshots
        .saturating_add(source.current_hp_snapshots);
    target.intervals_compared = target
        .intervals_compared
        .saturating_add(source.intervals_compared);
    target.eligible_intervals = target
        .eligible_intervals
        .saturating_add(source.eligible_intervals);
    target.eligible_exact = target.eligible_exact.saturating_add(source.eligible_exact);
    target.eligible_mismatched = target
        .eligible_mismatched
        .saturating_add(source.eligible_mismatched);
    target.invalidated_intervals = target
        .invalidated_intervals
        .saturating_add(source.invalidated_intervals);
    target.no_event_intervals = target
        .no_event_intervals
        .saturating_add(source.no_event_intervals);
    target.damage_events = target.damage_events.saturating_add(source.damage_events);
    target.damage_events_with_hp_loss = target
        .damage_events_with_hp_loss
        .saturating_add(source.damage_events_with_hp_loss);
    target.damage_events_without_hp_loss = target
        .damage_events_without_hp_loss
        .saturating_add(source.damage_events_without_hp_loss);
    target.healing_events = target.healing_events.saturating_add(source.healing_events);
    target.life_events = target.life_events.saturating_add(source.life_events);
    for (residual, intervals) in &source.residual_counts {
        *target.residual_counts.entry(*residual).or_default() += intervals;
    }
    append_examples(
        &mut target.mismatch_examples,
        &source.mismatch_examples,
        example_limit,
    );
    append_examples(
        &mut target.exact_examples,
        &source.exact_examples,
        example_limit,
    );
}

fn append_examples(target: &mut Vec<IntervalExample>, source: &[IntervalExample], limit: usize) {
    for example in source {
        if target.len() >= limit {
            break;
        }
        target.push(example.clone());
    }
}

fn read_selected_action_requests(
    path: &Path,
) -> Result<BTreeMap<(String, u64), SelectedActionRequest>, Box<dyn std::error::Error>> {
    let value: Value = serde_json::from_reader(BufReader::new(File::open(path)?))?;
    let observations = value
        .get("observations")
        .and_then(Value::as_array)
        .ok_or("selected-action input must contain an observations array")?;
    let mut requests = BTreeMap::new();
    for observation in observations {
        let session_id = observation
            .get("session_id")
            .and_then(Value::as_str)
            .ok_or("selected observation is missing session_id")?
            .to_owned();
        let sequence = observation
            .get("sequence")
            .and_then(Value::as_u64)
            .ok_or("selected observation is missing sequence")?;
        let run_ordinal = observation
            .get("run_ordinal")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or("selected observation is missing a valid run_ordinal")?;
        let target_entity_uuid = observation
            .get("target_entity_uuid")
            .and_then(Value::as_i64)
            .ok_or("selected observation is missing target_entity_uuid")?;
        let request = SelectedActionRequest {
            session_id: session_id.clone(),
            sequence,
            run_ordinal,
            target_entity_uuid,
        };
        if requests.insert((session_id, sequence), request).is_some() {
            return Err("selected-action input contains a duplicate session/sequence key".into());
        }
    }
    Ok(requests)
}

fn selected_action_observation(
    request: &SelectedActionRequest,
    observed_run_ordinal: u32,
    observed_target_entity_uuid: i64,
    state: &LedgerState,
) -> SelectedActionObservation {
    let mut unresolved_reasons = Vec::new();
    if request.run_ordinal != observed_run_ordinal {
        unresolved_reasons.push("run-ordinal-mismatch");
    }
    if request.target_entity_uuid != observed_target_entity_uuid {
        unresolved_reasons.push("target-entity-mismatch");
    }
    if state.baseline_sequence.is_none() || state.authoritative_hp.is_none() {
        unresolved_reasons.push("authoritative-current-hp-absent");
    }
    if state.max_hp.is_none() {
        unresolved_reasons.push("authoritative-max-hp-absent");
    }
    if state.damage_without_hp_loss != 0 {
        unresolved_reasons.push("intervening-damage-without-hp-loss");
    }
    if state.healing_without_effective_amount != 0 {
        unresolved_reasons.push("intervening-healing-without-effective-amount");
    }
    if state.healing_without_max_hp != 0 {
        unresolved_reasons.push("intervening-healing-without-max-hp");
    }
    if state.life_deaths != 0 || state.life_revives != 0 {
        unresolved_reasons.push("intervening-life-transition");
    }
    if state.max_hp_changes != 0 {
        unresolved_reasons.push("intervening-max-hp-change");
    }
    if state.invalidated {
        unresolved_reasons.push("ledger-interval-invalidated");
    }
    let predicted = state.predicted_hp;
    if predicted.is_none() {
        unresolved_reasons.push("predicted-current-hp-absent");
    }
    if let (Some(current), Some(maximum)) = (predicted, state.max_hp) {
        if maximum <= 0 || current < 0 || current > maximum {
            unresolved_reasons.push("predicted-current-hp-out-of-range");
        }
    }
    let candidate_reconstruction_available = unresolved_reasons.is_empty();
    SelectedActionObservation {
        session_id: request.session_id.clone(),
        sequence: request.sequence,
        run_ordinal: observed_run_ordinal,
        target_entity_uuid: observed_target_entity_uuid,
        baseline_sequence: state.baseline_sequence,
        authoritative_current_hp: state.authoritative_hp,
        max_hp: state.max_hp,
        candidate_pre_hit_current_hp: candidate_reconstruction_available
            .then_some(predicted)
            .flatten(),
        predicted_pre_hit_current_hp: None,
        damage_events_since_snapshot: state.damage_events,
        damage_hp_loss_since_snapshot: state.damage_hp_loss,
        damage_events_without_hp_loss: state.damage_without_hp_loss,
        healing_events_since_snapshot: state.healing_events,
        healing_effective_amount_since_snapshot: state.healing_amount,
        healing_events_without_effective_amount: state.healing_without_effective_amount,
        healing_events_without_max_hp: state.healing_without_max_hp,
        life_deaths_since_snapshot: state.life_deaths,
        life_revives_since_snapshot: state.life_revives,
        max_hp_changes_since_snapshot: state.max_hp_changes,
        interval_closure_sequence: None,
        interval_closure_residual: None,
        interval_closure_exact: false,
        formula_context_eligible: false,
        reconstruction_available: false,
        unresolved_reasons,
    }
}

fn interval_is_strictly_eligible(state: &LedgerState) -> bool {
    !state.invalidated
        && state.predicted_hp.is_some()
        && state.damage_without_hp_loss == 0
        && state.healing_without_effective_amount == 0
}

fn close_pending_selected_interval(
    pending: &mut HashMap<(u32, i64), Vec<(String, u64)>>,
    observations: &mut BTreeMap<(String, u64), SelectedActionObservation>,
    actor_key: (u32, i64),
    closure_sequence: u64,
    interval_eligible: bool,
    residual: Option<i64>,
) {
    let Some(keys) = pending.remove(&actor_key) else {
        return;
    };
    let exact = interval_eligible && residual == Some(0);
    for key in keys {
        let Some(observation) = observations.get_mut(&key) else {
            continue;
        };
        observation.interval_closure_sequence = Some(closure_sequence);
        observation.interval_closure_residual = residual;
        observation.interval_closure_exact = exact;
        observation.formula_context_eligible = exact && observation.unresolved_reasons.is_empty();
        observation.reconstruction_available = observation.formula_context_eligible;
        if observation.formula_context_eligible {
            observation.predicted_pre_hit_current_hp = observation.candidate_pre_hit_current_hp;
        }
    }
}

fn decode_attribute(attribute: &rlogs_events::EntityAttribute) -> Option<i64> {
    match attribute.decoded.as_ref() {
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

fn parse_args<I>(arguments: I) -> Result<Arguments, String>
where
    I: IntoIterator<Item = OsString>,
{
    let mut rlogs = Vec::new();
    let mut output = None;
    let mut example_limit = DEFAULT_EXAMPLE_LIMIT;
    let mut selected_actions = None;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        match argument.to_string_lossy().as_ref() {
            "--rlog" => rlogs.push(PathBuf::from(next_value(&mut arguments, "--rlog")?)),
            "--output" => output = Some(PathBuf::from(next_value(&mut arguments, "--output")?)),
            "--selected-actions" => {
                selected_actions = Some(PathBuf::from(next_value(
                    &mut arguments,
                    "--selected-actions",
                )?))
            }
            "--example-limit" => {
                example_limit = next_value(&mut arguments, "--example-limit")?
                    .to_string_lossy()
                    .parse::<usize>()
                    .map_err(|error| format!("invalid --example-limit: {error}"))?;
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    if rlogs.is_empty() {
        return Err("at least one --rlog is required".to_owned());
    }
    Ok(Arguments {
        rlogs,
        output: output.ok_or_else(|| "--output is required".to_owned())?,
        example_limit,
        selected_actions,
    })
}

fn next_value<I>(arguments: &mut I, name: &str) -> Result<OsString, String>
where
    I: Iterator<Item = OsString>,
{
    arguments
        .next()
        .ok_or_else(|| format!("{name} requires a value"))
}

fn file_label(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unlabeled.rlog")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_damage_or_effective_healing_fails_strict_eligibility() {
        let mut state = LedgerState::default();
        state.reset_interval(1, 1_000, Some(2_000));
        assert!(interval_is_strictly_eligible(&state));
        state.observe_damage(None);
        assert!(!interval_is_strictly_eligible(&state));

        state.reset_interval(2, 1_000, Some(2_000));
        state.observe_healing(None, 100);
        assert!(!interval_is_strictly_eligible(&state));
    }

    #[test]
    fn selected_hp_is_exposed_only_after_exact_interval_closure() {
        let request = SelectedActionRequest {
            session_id: "session".to_owned(),
            sequence: 20,
            run_ordinal: 1,
            target_entity_uuid: 30,
        };
        let mut state = LedgerState::default();
        state.reset_interval(10, 1_000, Some(2_000));
        state.observe_damage(Some(100));
        let observation = selected_action_observation(&request, 1, 30, &state);
        assert_eq!(observation.candidate_pre_hit_current_hp, Some(900));
        assert_eq!(observation.predicted_pre_hit_current_hp, None);

        let key = ("session".to_owned(), 20);
        let mut pending = HashMap::from([((1, 30), vec![key.clone()])]);
        let mut observations = BTreeMap::from([(key.clone(), observation)]);
        close_pending_selected_interval(
            &mut pending,
            &mut observations,
            (1, 30),
            40,
            true,
            Some(0),
        );
        let closed = observations.get(&key).expect("selected observation");
        assert!(closed.formula_context_eligible);
        assert_eq!(closed.predicted_pre_hit_current_hp, Some(900));
    }
}
