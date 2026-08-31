#![allow(clippy::needless_range_loop)]

use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsString,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{
    ActorState, CanonicalEvent, EntityAttributeUpdateKind, EntityAttributeValue, RunState,
    TimelineEventKind,
};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::Serialize;

const SCHEMA_VERSION: u16 = 6;
const FAMILY_WIDTH: usize = 6;
const DEFAULT_EXAMPLE_LIMIT: usize = 8;

const MEMBER_SUFFIXES: [&str; FAMILY_WIDTH] = [
    "current",
    "total",
    "add",
    "extra_add",
    "percent",
    "extra_percent",
];

#[derive(Debug, Serialize)]
struct AuditBundle {
    schema_version: u16,
    generated_by: &'static str,
    build_scope: BuildScope,
    policy: AuditPolicy,
    sessions: Vec<SessionSummary>,
    families: Vec<FamilyReport>,
    cross_family_transition_selection: Option<CrossFamilyTransitionReport>,
}

#[derive(Debug, Serialize)]
struct BuildScope {
    expected_game_build: String,
    recording_build_identity_authority: bool,
    recording_build_identity_policy: &'static str,
}

#[derive(Debug, Serialize)]
struct AuditPolicy {
    runtime_use: &'static str,
    source_scope: &'static str,
    state_scope: &'static str,
    missing_members: &'static str,
    units: &'static str,
    formula_policy: &'static str,
    cross_family_transition_policy: &'static str,
    unresolved_evidence_is_hidden: bool,
}

#[derive(Debug, Serialize)]
struct SessionSummary {
    rlog: String,
    session_id: String,
    run_ordinals_observed: u32,
    attribute_events: u64,
    selected_attribute_values: u64,
    undecodable_selected_values: u64,
    actor_lifecycle_state_resets: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct CrossTransitionAnchor {
    attribute_id: i32,
    delta: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct CrossFamilyMemberDelta {
    base_attribute_id: i32,
    offset: usize,
    attribute_id: i32,
    semantic_suffix: &'static str,
    delta: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct CrossFamilyMemberTransition {
    base_attribute_id: i32,
    offset: usize,
    attribute_id: i32,
    semantic_suffix: &'static str,
    before: i64,
    after: i64,
    delta: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CrossFamilyTransitionPatternKey {
    observed_family_base_ids: Vec<i32>,
    changed_members: Vec<CrossFamilyMemberDelta>,
}

#[derive(Debug, Default)]
struct CrossFamilyTransitionPatternAccumulator {
    count: u64,
    examples: Vec<CrossFamilyTransitionExample>,
}

#[derive(Debug, Default)]
struct CrossFamilyTransitionAccumulator {
    observed_batches: u64,
    complete_selected_family_batches: u64,
    incomplete_selected_family_batches: u64,
    sessions: BTreeSet<String>,
    actor_runs: BTreeSet<(String, u32, i64)>,
    patterns: BTreeMap<CrossFamilyTransitionPatternKey, CrossFamilyTransitionPatternAccumulator>,
}

#[derive(Debug, Clone, Serialize)]
struct CrossFamilyTransitionExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    sequence: u64,
    observed_micros: u64,
    actor_entity_uuid: i64,
    matched_anchors: Vec<CrossTransitionAnchor>,
    observed_family_base_ids: Vec<i32>,
    changed_members: Vec<CrossFamilyMemberDelta>,
    member_transitions: Vec<CrossFamilyMemberTransition>,
}

#[derive(Debug, Serialize)]
struct CrossFamilyTransitionPatternReport {
    observed_family_base_ids: Vec<i32>,
    all_selected_families_present: bool,
    changed_members: Vec<CrossFamilyMemberDelta>,
    count: u64,
    examples: Vec<CrossFamilyTransitionExample>,
}

#[derive(Debug, Serialize)]
struct CrossFamilyTransitionReport {
    selection_authority: &'static str,
    anchors: Vec<CrossTransitionAnchor>,
    selected_family_base_ids: Vec<i32>,
    observed_batches: u64,
    complete_selected_family_batches: u64,
    incomplete_selected_family_batches: u64,
    independent_sessions: usize,
    actor_runs: usize,
    patterns: Vec<CrossFamilyTransitionPatternReport>,
    formula_authority: bool,
    runtime_authority: bool,
    ui_display_authority: bool,
}

#[derive(Debug, Serialize)]
struct FamilyReport {
    base_attribute_id: i32,
    members: Vec<FamilyMember>,
    attribute_events: u64,
    actors: usize,
    actor_runs: usize,
    complete_packet_batches: u64,
    incomplete_packet_batches: u64,
    update_patterns: Vec<UpdatePatternReport>,
    delta_patterns: Vec<DeltaPatternReport>,
    equality_invariants: Vec<EqualityInvariant>,
    formula_checks: Vec<FormulaCheck>,
    transition_formula_checks: Vec<TransitionFormulaCheck>,
    examples: Vec<SnapshotExample>,
}

#[derive(Debug, Serialize)]
struct FamilyMember {
    offset: usize,
    attribute_id: i32,
    semantic_suffix: &'static str,
    observations: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct UpdatePatternKey {
    updated_offsets: Vec<usize>,
    equal_value_groups: Vec<Vec<usize>>,
}

#[derive(Debug, Default)]
struct UpdatePatternAccumulator {
    count: u64,
    examples: Vec<SnapshotExample>,
}

#[derive(Debug, Serialize)]
struct UpdatePatternReport {
    updated_offsets: Vec<usize>,
    updated_members: Vec<&'static str>,
    equal_value_groups: Vec<Vec<usize>>,
    count: u64,
    examples: Vec<SnapshotExample>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DeltaPatternKey {
    deltas: [Option<i64>; FAMILY_WIDTH],
}

#[derive(Debug, Default)]
struct DeltaPatternAccumulator {
    count: u64,
    examples: Vec<SnapshotExample>,
}

#[derive(Debug, Serialize)]
struct DeltaPatternReport {
    deltas: [Option<i64>; FAMILY_WIDTH],
    count: u64,
    examples: Vec<SnapshotExample>,
}

#[derive(Debug, Serialize)]
struct EqualityInvariant {
    left_offset: usize,
    left_member: &'static str,
    right_offset: usize,
    right_member: &'static str,
    evaluable_snapshots: u64,
    exact_matches: u64,
    mismatches: u64,
}

#[derive(Debug, Serialize)]
struct FormulaCheck {
    expression: &'static str,
    scale: Option<i64>,
    rounding: Option<&'static str>,
    evaluable_snapshots: u64,
    exact_matches: u64,
    mismatches: u64,
    residual_min: Option<i64>,
    residual_max: Option<i64>,
    residual_examples: Vec<i64>,
    mismatch_examples: Vec<FormulaMismatchExample>,
}

#[derive(Debug, Serialize)]
struct TransitionFormulaCheck {
    stage: &'static str,
    expression: &'static str,
    scale: i64,
    rounding: &'static str,
    evaluable_transitions: u64,
    exact_matches: u64,
    within_one_packet_unit: u64,
    mismatches_beyond_one_packet_unit: u64,
    actors: usize,
    residual_min: Option<i64>,
    residual_max: Option<i64>,
    examples: Vec<TransitionFormulaExample>,
    mismatches_beyond_one_examples: Vec<TransitionFormulaExample>,
}

#[derive(Debug, Clone, Serialize)]
struct FormulaMismatchExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    sequence: u64,
    observed_micros: u64,
    actor_entity_uuid: i64,
    packet_values: [Option<i64>; FAMILY_WIDTH],
    actual: i64,
    predicted: i64,
    residual: i64,
}

#[derive(Debug, Clone, Serialize)]
struct TransitionFormulaExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    sequence: u64,
    actor_entity_uuid: i64,
    basis: i64,
    old_rate: i64,
    new_rate: i64,
    observed_delta: i64,
    predicted_delta: i64,
    residual: i64,
}

#[derive(Debug, Clone, Serialize)]
struct SnapshotExample {
    rlog: String,
    session_id: String,
    run_ordinal: u32,
    sequence: u64,
    observed_micros: u64,
    actor_entity_uuid: i64,
    updated_offsets: Vec<usize>,
    packet_values: [Option<i64>; FAMILY_WIDTH],
    last_seen_before: [Option<i64>; FAMILY_WIDTH],
    last_seen_after: [Option<i64>; FAMILY_WIDTH],
    deltas_since_last_seen: [Option<i64>; FAMILY_WIDTH],
}

#[derive(Debug, Clone, Copy, Default)]
struct FamilyState {
    values: [Option<i64>; FAMILY_WIDTH],
}

#[derive(Debug, Default)]
struct FamilyAccumulator {
    observations: [u64; FAMILY_WIDTH],
    attribute_events: u64,
    actors: BTreeSet<i64>,
    actor_runs: BTreeSet<(String, u32, i64)>,
    complete_packet_batches: u64,
    incomplete_packet_batches: u64,
    update_patterns: BTreeMap<UpdatePatternKey, UpdatePatternAccumulator>,
    delta_patterns: BTreeMap<DeltaPatternKey, DeltaPatternAccumulator>,
    equality_counts: [[(u64, u64); FAMILY_WIDTH]; FAMILY_WIDTH],
    formula_checks: Vec<FormulaAccumulator>,
    transition_formula_checks: Vec<TransitionFormulaAccumulator>,
    examples: Vec<SnapshotExample>,
}

#[derive(Debug)]
struct FormulaAccumulator {
    expression: &'static str,
    scale: Option<i64>,
    rounding: Option<RoundingMode>,
    evaluable: u64,
    exact: u64,
    mismatches: u64,
    residual_min: Option<i64>,
    residual_max: Option<i64>,
    residual_examples: BTreeSet<i64>,
    mismatch_examples: Vec<FormulaMismatchExample>,
}

#[derive(Debug)]
struct TransitionFormulaAccumulator {
    stage: &'static str,
    expression: &'static str,
    scale: i64,
    rounding: RoundingMode,
    evaluable: u64,
    exact: u64,
    within_one: u64,
    mismatches_beyond_one: u64,
    actors: BTreeSet<i64>,
    residual_min: Option<i64>,
    residual_max: Option<i64>,
    examples: Vec<TransitionFormulaExample>,
    mismatches_beyond_one_examples: Vec<TransitionFormulaExample>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RoundingMode {
    TruncTowardZero,
    Floor,
    Ceil,
    NearestHalfAwayFromZero,
    NearestHalfToEven,
}

impl RoundingMode {
    const ALL: [Self; 5] = [
        Self::TruncTowardZero,
        Self::Floor,
        Self::Ceil,
        Self::NearestHalfAwayFromZero,
        Self::NearestHalfToEven,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::TruncTowardZero => "trunc_toward_zero",
            Self::Floor => "floor",
            Self::Ceil => "ceil",
            Self::NearestHalfAwayFromZero => "nearest_half_away_from_zero",
            Self::NearestHalfToEven => "nearest_half_to_even",
        }
    }
}

impl TransitionFormulaAccumulator {
    fn new(
        stage: &'static str,
        expression: &'static str,
        scale: i64,
        rounding: RoundingMode,
    ) -> Self {
        Self {
            stage,
            expression,
            scale,
            rounding,
            evaluable: 0,
            exact: 0,
            within_one: 0,
            mismatches_beyond_one: 0,
            actors: BTreeSet::new(),
            residual_min: None,
            residual_max: None,
            examples: Vec::new(),
            mismatches_beyond_one_examples: Vec::new(),
        }
    }

    fn observe(
        &mut self,
        source: &SnapshotExample,
        basis: i64,
        old_rate: i64,
        new_rate: i64,
        observed_delta: i64,
        example_limit: usize,
    ) {
        let Some(predicted_delta) =
            scaled_delta_with_rounding(basis, old_rate, new_rate, self.scale, self.rounding)
        else {
            return;
        };
        let residual = observed_delta.saturating_sub(predicted_delta);
        self.evaluable = self.evaluable.saturating_add(1);
        self.actors.insert(source.actor_entity_uuid);
        if residual == 0 {
            self.exact = self.exact.saturating_add(1);
        } else if residual.unsigned_abs() == 1 {
            self.within_one = self.within_one.saturating_add(1);
        } else {
            self.mismatches_beyond_one = self.mismatches_beyond_one.saturating_add(1);
        }
        self.residual_min = Some(
            self.residual_min
                .map_or(residual, |value| value.min(residual)),
        );
        self.residual_max = Some(
            self.residual_max
                .map_or(residual, |value| value.max(residual)),
        );
        let evidence = TransitionFormulaExample {
            rlog: source.rlog.clone(),
            session_id: source.session_id.clone(),
            run_ordinal: source.run_ordinal,
            sequence: source.sequence,
            actor_entity_uuid: source.actor_entity_uuid,
            basis,
            old_rate,
            new_rate,
            observed_delta,
            predicted_delta,
            residual,
        };
        if self.examples.len() < example_limit {
            self.examples.push(evidence.clone());
        }
        if residual.unsigned_abs() > 1 && self.mismatches_beyond_one_examples.len() < example_limit
        {
            self.mismatches_beyond_one_examples.push(evidence);
        }
    }
}

impl FormulaAccumulator {
    fn new(expression: &'static str, scale: Option<i64>, rounding: Option<RoundingMode>) -> Self {
        Self {
            expression,
            scale,
            rounding,
            evaluable: 0,
            exact: 0,
            mismatches: 0,
            residual_min: None,
            residual_max: None,
            residual_examples: BTreeSet::new(),
            mismatch_examples: Vec::new(),
        }
    }

    fn observe(
        &mut self,
        source: &SnapshotExample,
        actual: i64,
        predicted: Option<i64>,
        example_limit: usize,
    ) {
        let Some(predicted) = predicted else {
            return;
        };
        self.evaluable = self.evaluable.saturating_add(1);
        let residual = actual.saturating_sub(predicted);
        if residual == 0 {
            self.exact = self.exact.saturating_add(1);
        } else {
            self.mismatches = self.mismatches.saturating_add(1);
            self.residual_min = Some(
                self.residual_min
                    .map_or(residual, |value| value.min(residual)),
            );
            self.residual_max = Some(
                self.residual_max
                    .map_or(residual, |value| value.max(residual)),
            );
            if self.residual_examples.len() < example_limit {
                self.residual_examples.insert(residual);
            }
            if self.mismatch_examples.len() < example_limit {
                self.mismatch_examples.push(FormulaMismatchExample {
                    rlog: source.rlog.clone(),
                    session_id: source.session_id.clone(),
                    run_ordinal: source.run_ordinal,
                    sequence: source.sequence,
                    observed_micros: source.observed_micros,
                    actor_entity_uuid: source.actor_entity_uuid,
                    packet_values: source.packet_values,
                    actual,
                    predicted,
                    residual,
                });
            }
        }
    }
}

#[derive(Debug)]
struct Arguments {
    expected_game_build: String,
    families: BTreeSet<i32>,
    cross_transition_anchors: BTreeSet<CrossTransitionAnchor>,
    rlogs: Vec<PathBuf>,
    output: PathBuf,
    example_limit: usize,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("attribute-family formula proof failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = arguments()?;
    let selected_ids = args
        .families
        .iter()
        .flat_map(|base| (0..FAMILY_WIDTH).map(move |offset| base + offset as i32))
        .collect::<BTreeSet<_>>();
    let selected_family_base_ids = args.families.iter().copied().collect::<Vec<_>>();
    let mut accumulators = args
        .families
        .iter()
        .copied()
        .map(|base| (base, FamilyAccumulator::with_formula_checks()))
        .collect::<BTreeMap<_, _>>();
    let mut cross_family_transitions = CrossFamilyTransitionAccumulator::default();
    let mut sessions = Vec::new();

    for path in &args.rlogs {
        sessions.push(read_session(
            path,
            &selected_ids,
            &mut accumulators,
            &args.cross_transition_anchors,
            &selected_family_base_ids,
            &mut cross_family_transitions,
            args.example_limit,
        )?);
    }

    let families = accumulators
        .into_iter()
        .map(|(base, accumulator)| accumulator.finish(base))
        .collect();
    let bundle = AuditBundle {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-attribute-family-formula-proof",
        build_scope: BuildScope {
            expected_game_build: args.expected_game_build,
            recording_build_identity_authority: false,
            recording_build_identity_policy: "the expected build is caller-declared cohort scope; runtime promotion still requires an exact protocol-pack identity plus canonical replay conservation and protocol event coverage",
        },
        policy: AuditPolicy {
            runtime_use: "offline_research_only_never_loaded_by_capture_or_live_meter",
            source_scope: "each_rlog_is_read_independently_and_state_never_crosses_session_run_or_actor_lifetime",
            state_scope: "absolute_formula_checks_use_only_values_co_present_in_one entity_attribute packet; transition checks use packet-ordered before and after state for the same actor family run and lifetime and require that the packet did not update another tracked input stage",
            missing_members: "unknown_never_coerced_to_zero; absolute checks require co-present values and transition checks may reuse only a previously observed value from the same actor lifetime",
            units: "raw_signed_packet_units_no_percent_flat_or_multiplier_conversion",
            formula_policy: "candidate_expressions_are_diagnostics_only_and_require_exact_packet_and_damage_validation_before_runtime_use",
            cross_family_transition_policy: "optional exact signed attribute-delta anchors select whole same-packet batches only; co-transition patterns establish observed vectors, never causal formulas, provider ownership, opportunity attribution, or runtime/UI authority",
            unresolved_evidence_is_hidden: false,
        },
        sessions,
        families,
        cross_family_transition_selection: (!args.cross_transition_anchors.is_empty()).then(|| {
            cross_family_transitions.finish(
                args.cross_transition_anchors.into_iter().collect(),
                selected_family_base_ids,
            )
        }),
    };
    let mut writer = BufWriter::new(File::create(&args.output)?);
    serde_json::to_writer_pretty(&mut writer, &bundle)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    println!("wrote {}", args.output.display());
    Ok(())
}

impl CrossFamilyTransitionAccumulator {
    fn observe(
        &mut self,
        batch: &[(i32, SnapshotExample)],
        anchors: &BTreeSet<CrossTransitionAnchor>,
        selected_family_base_ids: &[i32],
        example_limit: usize,
    ) {
        if anchors.is_empty() || batch.is_empty() {
            return;
        }
        let observed_family_base_ids = batch.iter().map(|(base, _)| *base).collect::<Vec<_>>();
        let mut changed_members = Vec::new();
        let mut member_transitions = Vec::new();
        for (base, example) in batch {
            for (offset, delta) in example.deltas_since_last_seen.iter().copied().enumerate() {
                let Some(delta) = delta else {
                    continue;
                };
                changed_members.push(CrossFamilyMemberDelta {
                    base_attribute_id: *base,
                    offset,
                    attribute_id: *base + offset as i32,
                    semantic_suffix: MEMBER_SUFFIXES[offset],
                    delta,
                });
                let (Some(before), Some(after)) = (
                    example.last_seen_before[offset],
                    example.last_seen_after[offset],
                ) else {
                    continue;
                };
                member_transitions.push(CrossFamilyMemberTransition {
                    base_attribute_id: *base,
                    offset,
                    attribute_id: *base + offset as i32,
                    semantic_suffix: MEMBER_SUFFIXES[offset],
                    before,
                    after,
                    delta,
                });
            }
        }
        let matched_anchors = anchors
            .iter()
            .filter(|anchor| {
                changed_members.iter().any(|member| {
                    member.attribute_id == anchor.attribute_id && member.delta == anchor.delta
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        if matched_anchors.is_empty() {
            return;
        }
        let first = &batch[0].1;
        self.observed_batches = self.observed_batches.saturating_add(1);
        if selected_family_base_ids
            .iter()
            .all(|base| observed_family_base_ids.contains(base))
        {
            self.complete_selected_family_batches =
                self.complete_selected_family_batches.saturating_add(1);
        } else {
            self.incomplete_selected_family_batches =
                self.incomplete_selected_family_batches.saturating_add(1);
        }
        self.sessions.insert(first.session_id.clone());
        self.actor_runs.insert((
            first.session_id.clone(),
            first.run_ordinal,
            first.actor_entity_uuid,
        ));
        let pattern = self
            .patterns
            .entry(CrossFamilyTransitionPatternKey {
                observed_family_base_ids: observed_family_base_ids.clone(),
                changed_members: changed_members.clone(),
            })
            .or_default();
        pattern.count = pattern.count.saturating_add(1);
        if pattern.examples.len() < example_limit {
            pattern.examples.push(CrossFamilyTransitionExample {
                rlog: first.rlog.clone(),
                session_id: first.session_id.clone(),
                run_ordinal: first.run_ordinal,
                sequence: first.sequence,
                observed_micros: first.observed_micros,
                actor_entity_uuid: first.actor_entity_uuid,
                matched_anchors,
                observed_family_base_ids,
                changed_members,
                member_transitions,
            });
        }
    }

    fn finish(
        self,
        anchors: Vec<CrossTransitionAnchor>,
        selected_family_base_ids: Vec<i32>,
    ) -> CrossFamilyTransitionReport {
        CrossFamilyTransitionReport {
            selection_authority: "exact signed attribute-delta anchors select same canonical EntityAttributes packet batches; all co-transitioned selected-family members are retained and missing members remain absent rather than zero",
            anchors,
            selected_family_base_ids: selected_family_base_ids.clone(),
            observed_batches: self.observed_batches,
            complete_selected_family_batches: self.complete_selected_family_batches,
            incomplete_selected_family_batches: self.incomplete_selected_family_batches,
            independent_sessions: self.sessions.len(),
            actor_runs: self.actor_runs.len(),
            patterns: self
                .patterns
                .into_iter()
                .map(|(key, value)| CrossFamilyTransitionPatternReport {
                    all_selected_families_present: selected_family_base_ids
                        .iter()
                        .all(|base| key.observed_family_base_ids.contains(base)),
                    observed_family_base_ids: key.observed_family_base_ids,
                    changed_members: key.changed_members,
                    count: value.count,
                    examples: value.examples,
                })
                .collect(),
            formula_authority: false,
            runtime_authority: false,
            ui_display_authority: false,
        }
    }
}

impl FamilyAccumulator {
    fn with_formula_checks() -> Self {
        let mut checks = vec![
            FormulaAccumulator::new("current = total", None, None),
            FormulaAccumulator::new("current = add", None, None),
            FormulaAccumulator::new("total = add", None, None),
            FormulaAccumulator::new("current = total + extra_add", None, None),
            FormulaAccumulator::new("current = add + extra_add", None, None),
            FormulaAccumulator::new("current = total + add", None, None),
            FormulaAccumulator::new("current = total + add + extra_add", None, None),
        ];
        for scale in [100_i64, 10_000_i64] {
            for rounding in RoundingMode::ALL {
                checks.push(FormulaAccumulator::new(
                    "total = div_round(add * (scale + percent), scale)",
                    Some(scale),
                    Some(rounding),
                ));
                checks.push(FormulaAccumulator::new(
                    "current = div_round(add * (scale + percent), scale)",
                    Some(scale),
                    Some(rounding),
                ));
                checks.push(FormulaAccumulator::new(
                    "current = div_round((total + add) * (scale + percent), scale) + extra_add",
                    Some(scale),
                    Some(rounding),
                ));
                checks.push(FormulaAccumulator::new(
                    "current = div_round((total + add + extra_add) * (scale + percent + extra_percent), scale)",
                    Some(scale),
                    Some(rounding),
                ));
                checks.push(FormulaAccumulator::new(
                    "current = div_round((div_round((total + add) * (scale + percent), scale) + extra_add) * (scale + extra_percent), scale)",
                    Some(scale),
                    Some(rounding),
                ));
            }
        }
        let mut transition_formula_checks = Vec::new();
        for (stage, expression) in [
            (
                "percent_to_total",
                "delta(total) = div_round(add * new_percent, scale) - div_round(add * old_percent, scale)",
            ),
            (
                "add_to_total",
                "delta(total) = div_round(new_add * (scale + percent), scale) - div_round(old_add * (scale + percent), scale)",
            ),
            (
                "extra_percent_to_current",
                "delta(current) = div_round(total * new_extra_percent, scale) - div_round(total * old_extra_percent, scale)",
            ),
        ] {
            for scale in [100_i64, 10_000_i64] {
                for rounding in RoundingMode::ALL {
                    transition_formula_checks.push(TransitionFormulaAccumulator::new(
                        stage, expression, scale, rounding,
                    ));
                }
            }
        }
        Self {
            formula_checks: checks,
            transition_formula_checks,
            ..Self::default()
        }
    }

    fn observe(
        &mut self,
        example: SnapshotExample,
        updated_values: &[(usize, i64)],
        example_limit: usize,
    ) {
        self.attribute_events = self.attribute_events.saturating_add(1);
        self.actors.insert(example.actor_entity_uuid);
        self.actor_runs.insert((
            example.session_id.clone(),
            example.run_ordinal,
            example.actor_entity_uuid,
        ));
        for (offset, _) in updated_values {
            self.observations[*offset] = self.observations[*offset].saturating_add(1);
        }
        self.observe_equalities(&example.packet_values);
        self.observe_formulas(&example, example_limit);
        self.observe_transition_formulas(&example, example_limit);
        if complete(&example.packet_values) {
            self.complete_packet_batches = self.complete_packet_batches.saturating_add(1);
        } else {
            self.incomplete_packet_batches = self.incomplete_packet_batches.saturating_add(1);
        }

        let update_key = UpdatePatternKey {
            updated_offsets: example.updated_offsets.clone(),
            equal_value_groups: equal_value_groups(updated_values),
        };
        let update = self.update_patterns.entry(update_key).or_default();
        update.count = update.count.saturating_add(1);
        push_example(&mut update.examples, &example, example_limit);

        let delta = self
            .delta_patterns
            .entry(DeltaPatternKey {
                deltas: example.deltas_since_last_seen,
            })
            .or_default();
        delta.count = delta.count.saturating_add(1);
        push_example(&mut delta.examples, &example, example_limit);
        push_example(&mut self.examples, &example, example_limit);
    }

    fn observe_equalities(&mut self, values: &[Option<i64>; FAMILY_WIDTH]) {
        for left in 0..FAMILY_WIDTH {
            for right in (left + 1)..FAMILY_WIDTH {
                let (Some(left_value), Some(right_value)) = (values[left], values[right]) else {
                    continue;
                };
                let counts = &mut self.equality_counts[left][right];
                if left_value == right_value {
                    counts.0 = counts.0.saturating_add(1);
                } else {
                    counts.1 = counts.1.saturating_add(1);
                }
            }
        }
    }

    fn observe_formulas(&mut self, source: &SnapshotExample, example_limit: usize) {
        let [current, total, add, extra_add, percent, extra_percent] = source.packet_values;
        let mut index = 0;
        observe_optional(
            &mut self.formula_checks[index],
            source,
            current,
            total,
            example_limit,
        );
        index += 1;
        observe_optional(
            &mut self.formula_checks[index],
            source,
            current,
            add,
            example_limit,
        );
        index += 1;
        observe_optional(
            &mut self.formula_checks[index],
            source,
            total,
            add,
            example_limit,
        );
        index += 1;
        observe_optional(
            &mut self.formula_checks[index],
            source,
            current,
            total
                .zip(extra_add)
                .and_then(|(total, extra_add)| total.checked_add(extra_add)),
            example_limit,
        );
        index += 1;
        observe_optional(
            &mut self.formula_checks[index],
            source,
            current,
            add.zip(extra_add)
                .and_then(|(add, extra_add)| add.checked_add(extra_add)),
            example_limit,
        );
        index += 1;
        observe_optional(
            &mut self.formula_checks[index],
            source,
            current,
            total
                .zip(add)
                .and_then(|(total, add)| total.checked_add(add)),
            example_limit,
        );
        index += 1;
        observe_optional(
            &mut self.formula_checks[index],
            source,
            current,
            total
                .zip(add)
                .zip(extra_add)
                .and_then(|((total, add), extra_add)| {
                    total
                        .checked_add(add)
                        .and_then(|value| value.checked_add(extra_add))
                }),
            example_limit,
        );
        index += 1;
        for scale in [100_i64, 10_000_i64] {
            for rounding in RoundingMode::ALL {
                let total_from_add = add
                    .zip(percent)
                    .and_then(|(add, percent)| scaled_with_rounding(add, percent, scale, rounding));
                observe_optional(
                    &mut self.formula_checks[index],
                    source,
                    total,
                    total_from_add,
                    example_limit,
                );
                index += 1;
                observe_optional(
                    &mut self.formula_checks[index],
                    source,
                    current,
                    total_from_add,
                    example_limit,
                );
                index += 1;
                let first = total.zip(add).zip(percent).zip(extra_add).and_then(
                    |(((total, add), percent), extra_add)| {
                        total
                            .checked_add(add)
                            .and_then(|value| scaled_with_rounding(value, percent, scale, rounding))
                            .and_then(|value| value.checked_add(extra_add))
                    },
                );
                observe_optional(
                    &mut self.formula_checks[index],
                    source,
                    current,
                    first,
                    example_limit,
                );
                index += 1;
                let second = total
                    .zip(add)
                    .zip(extra_add)
                    .zip(percent)
                    .zip(extra_percent)
                    .and_then(|((((total, add), extra_add), percent), extra_percent)| {
                        total
                            .checked_add(add)
                            .and_then(|value| value.checked_add(extra_add))
                            .and_then(|value| {
                                percent.checked_add(extra_percent).and_then(|rate| {
                                    scaled_with_rounding(value, rate, scale, rounding)
                                })
                            })
                    });
                observe_optional(
                    &mut self.formula_checks[index],
                    source,
                    current,
                    second,
                    example_limit,
                );
                index += 1;
                let third = first.zip(extra_percent).and_then(|(value, extra_percent)| {
                    scaled_with_rounding(value, extra_percent, scale, rounding)
                });
                observe_optional(
                    &mut self.formula_checks[index],
                    source,
                    current,
                    third,
                    example_limit,
                );
                index += 1;
            }
        }
        debug_assert_eq!(index, self.formula_checks.len());
    }

    fn observe_transition_formulas(&mut self, example: &SnapshotExample, example_limit: usize) {
        if example.updated_offsets == [0, 1, 4] {
            if let (
                Some(before_total),
                Some(after_total),
                Some(before_add),
                Some(after_add),
                Some(old_percent),
                Some(new_percent),
            ) = (
                example.last_seen_before[1],
                example.last_seen_after[1],
                example.last_seen_before[2],
                example.last_seen_after[2],
                example.last_seen_before[4],
                example.last_seen_after[4],
            ) {
                if before_add == after_add && old_percent != new_percent {
                    let observed_delta = after_total.saturating_sub(before_total);
                    for check in &mut self.transition_formula_checks[0..10] {
                        check.observe(
                            example,
                            after_add,
                            old_percent,
                            new_percent,
                            observed_delta,
                            example_limit,
                        );
                    }
                }
            }
        }

        if example.updated_offsets == [0, 1, 2] {
            if let (
                Some(before_total),
                Some(after_total),
                Some(old_add),
                Some(new_add),
                Some(before_percent),
                Some(after_percent),
            ) = (
                example.last_seen_before[1],
                example.last_seen_after[1],
                example.last_seen_before[2],
                example.last_seen_after[2],
                example.last_seen_before[4],
                example.last_seen_after[4],
            ) {
                if old_add != new_add && before_percent == after_percent {
                    let observed_delta = after_total.saturating_sub(before_total);
                    for check in &mut self.transition_formula_checks[10..20] {
                        let Some(rate) = check.scale.checked_add(after_percent) else {
                            continue;
                        };
                        check.observe(
                            example,
                            rate,
                            old_add,
                            new_add,
                            observed_delta,
                            example_limit,
                        );
                    }
                }
            }
        }

        if example.updated_offsets == [0, 5] {
            if let (
                Some(before_current),
                Some(after_current),
                Some(before_total),
                Some(after_total),
                Some(old_extra_percent),
                Some(new_extra_percent),
            ) = (
                example.last_seen_before[0],
                example.last_seen_after[0],
                example.last_seen_before[1],
                example.last_seen_after[1],
                example.last_seen_before[5],
                example.last_seen_after[5],
            ) {
                if before_total == after_total && old_extra_percent != new_extra_percent {
                    let observed_delta = after_current.saturating_sub(before_current);
                    for check in &mut self.transition_formula_checks[20..30] {
                        check.observe(
                            example,
                            after_total,
                            old_extra_percent,
                            new_extra_percent,
                            observed_delta,
                            example_limit,
                        );
                    }
                }
            }
        }
    }

    fn finish(self, base: i32) -> FamilyReport {
        let members = (0..FAMILY_WIDTH)
            .map(|offset| FamilyMember {
                offset,
                attribute_id: base + offset as i32,
                semantic_suffix: MEMBER_SUFFIXES[offset],
                observations: self.observations[offset],
            })
            .collect();
        let update_patterns = self
            .update_patterns
            .into_iter()
            .map(|(key, value)| UpdatePatternReport {
                updated_members: key
                    .updated_offsets
                    .iter()
                    .map(|offset| MEMBER_SUFFIXES[*offset])
                    .collect(),
                updated_offsets: key.updated_offsets,
                equal_value_groups: key.equal_value_groups,
                count: value.count,
                examples: value.examples,
            })
            .collect();
        let delta_patterns = self
            .delta_patterns
            .into_iter()
            .map(|(key, value)| DeltaPatternReport {
                deltas: key.deltas,
                count: value.count,
                examples: value.examples,
            })
            .collect();
        let mut equality_invariants = Vec::new();
        for left in 0..FAMILY_WIDTH {
            for right in (left + 1)..FAMILY_WIDTH {
                let (exact_matches, mismatches) = self.equality_counts[left][right];
                equality_invariants.push(EqualityInvariant {
                    left_offset: left,
                    left_member: MEMBER_SUFFIXES[left],
                    right_offset: right,
                    right_member: MEMBER_SUFFIXES[right],
                    evaluable_snapshots: exact_matches.saturating_add(mismatches),
                    exact_matches,
                    mismatches,
                });
            }
        }
        let formula_checks = self
            .formula_checks
            .into_iter()
            .map(|check| FormulaCheck {
                expression: check.expression,
                scale: check.scale,
                rounding: check.rounding.map(RoundingMode::label),
                evaluable_snapshots: check.evaluable,
                exact_matches: check.exact,
                mismatches: check.mismatches,
                residual_min: check.residual_min,
                residual_max: check.residual_max,
                residual_examples: check.residual_examples.into_iter().collect(),
                mismatch_examples: check.mismatch_examples,
            })
            .collect();
        let transition_formula_checks = self
            .transition_formula_checks
            .into_iter()
            .map(|check| TransitionFormulaCheck {
                stage: check.stage,
                expression: check.expression,
                scale: check.scale,
                rounding: check.rounding.label(),
                evaluable_transitions: check.evaluable,
                exact_matches: check.exact,
                within_one_packet_unit: check.within_one,
                mismatches_beyond_one_packet_unit: check.mismatches_beyond_one,
                actors: check.actors.len(),
                residual_min: check.residual_min,
                residual_max: check.residual_max,
                examples: check.examples,
                mismatches_beyond_one_examples: check.mismatches_beyond_one_examples,
            })
            .collect();
        FamilyReport {
            base_attribute_id: base,
            members,
            attribute_events: self.attribute_events,
            actors: self.actors.len(),
            actor_runs: self.actor_runs.len(),
            complete_packet_batches: self.complete_packet_batches,
            incomplete_packet_batches: self.incomplete_packet_batches,
            update_patterns,
            delta_patterns,
            equality_invariants,
            formula_checks,
            transition_formula_checks,
            examples: self.examples,
        }
    }
}

fn read_session(
    path: &Path,
    selected_ids: &BTreeSet<i32>,
    accumulators: &mut BTreeMap<i32, FamilyAccumulator>,
    cross_transition_anchors: &BTreeSet<CrossTransitionAnchor>,
    selected_family_base_ids: &[i32],
    cross_family_transitions: &mut CrossFamilyTransitionAccumulator,
    example_limit: usize,
) -> Result<SessionSummary, Box<dyn std::error::Error>> {
    let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
    let mut session_id = None::<String>;
    let mut current_run_ordinal = 0_u32;
    let mut maximum_run_ordinal = 0_u32;
    let mut attribute_events = 0_u64;
    let mut selected_attribute_values = 0_u64;
    let mut undecodable_selected_values = 0_u64;
    let mut actor_lifecycle_state_resets = 0_u64;
    let mut states = BTreeMap::<(u32, i64, i32), FamilyState>::new();

    while let Some(envelope) = reader.next_event()? {
        if let Some(expected) = &session_id {
            if expected != &envelope.session_id {
                return Err(format!(
                    "{} contains multiple sessions: {expected} and {}",
                    path.display(),
                    envelope.session_id
                )
                .into());
            }
        } else {
            session_id = Some(envelope.session_id.clone());
        }
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            continue;
        };
        match &timeline.kind {
            TimelineEventKind::RunBoundary { state, .. } => match state {
                RunState::Entered => {
                    current_run_ordinal = current_run_ordinal.saturating_add(1);
                    maximum_run_ordinal = maximum_run_ordinal.max(current_run_ordinal);
                }
                RunState::Started if current_run_ordinal == 0 => {
                    current_run_ordinal = 1;
                    maximum_run_ordinal = 1;
                }
                _ => {}
            },
            TimelineEventKind::Actor(event)
                if matches!(
                    event.state,
                    ActorState::Spawned | ActorState::Transformed | ActorState::Despawned
                ) =>
            {
                let before = states.len();
                states.retain(|(run_ordinal, entity_uuid, _), _| {
                    *run_ordinal != current_run_ordinal || *entity_uuid != event.actor.entity_uuid.0
                });
                actor_lifecycle_state_resets = actor_lifecycle_state_resets
                    .saturating_add((before.saturating_sub(states.len())) as u64);
            }
            TimelineEventKind::EntityAttributes(event) => {
                attribute_events = attribute_events.saturating_add(1);
                if event.update_kind == EntityAttributeUpdateKind::Snapshot {
                    for ((run_ordinal, entity_uuid, _), state) in &mut states {
                        if *run_ordinal == current_run_ordinal
                            && *entity_uuid == event.actor.entity_uuid.0
                        {
                            state.values = [None; FAMILY_WIDTH];
                        }
                    }
                }
                let mut updates = BTreeMap::<i32, Vec<(usize, i64)>>::new();
                for attribute in &event.attributes {
                    if !selected_ids.contains(&attribute.attribute_id) {
                        continue;
                    }
                    let Some(value) = decode_attribute(attribute) else {
                        undecodable_selected_values = undecodable_selected_values.saturating_add(1);
                        continue;
                    };
                    selected_attribute_values = selected_attribute_values.saturating_add(1);
                    let Some((base, offset)) =
                        family_and_offset(attribute.attribute_id, accumulators)
                    else {
                        continue;
                    };
                    updates.entry(base).or_default().push((offset, value));
                }
                let mut batch_examples = Vec::new();
                for (base, mut family_updates) in updates {
                    family_updates.sort_unstable_by_key(|(offset, _)| *offset);
                    family_updates.dedup_by_key(|(offset, _)| *offset);
                    let state = states
                        .entry((current_run_ordinal, event.actor.entity_uuid.0, base))
                        .or_default();
                    let before = state.values;
                    let mut packet_values = [None; FAMILY_WIDTH];
                    for (offset, value) in &family_updates {
                        packet_values[*offset] = Some(*value);
                        state.values[*offset] = Some(*value);
                    }
                    let after = state.values;
                    let deltas =
                        std::array::from_fn(|offset| match (before[offset], after[offset]) {
                            (Some(before), Some(after)) if before != after => {
                                Some(after.saturating_sub(before))
                            }
                            _ => None,
                        });
                    let example = SnapshotExample {
                        rlog: path.display().to_string(),
                        session_id: envelope.session_id.clone(),
                        run_ordinal: current_run_ordinal,
                        sequence: envelope.sequence,
                        observed_micros: envelope.time.observed_micros,
                        actor_entity_uuid: event.actor.entity_uuid.0,
                        updated_offsets: family_updates.iter().map(|(offset, _)| *offset).collect(),
                        packet_values,
                        last_seen_before: before,
                        last_seen_after: after,
                        deltas_since_last_seen: deltas,
                    };
                    batch_examples.push((base, example.clone()));
                    if let Some(accumulator) = accumulators.get_mut(&base) {
                        accumulator.observe(example, &family_updates, example_limit);
                    }
                }
                cross_family_transitions.observe(
                    &batch_examples,
                    cross_transition_anchors,
                    selected_family_base_ids,
                    example_limit,
                );
            }
            _ => {}
        }
    }

    Ok(SessionSummary {
        rlog: path.display().to_string(),
        session_id: session_id.unwrap_or_else(|| "unobserved".to_owned()),
        run_ordinals_observed: maximum_run_ordinal,
        attribute_events,
        selected_attribute_values,
        undecodable_selected_values,
        actor_lifecycle_state_resets,
    })
}

fn family_and_offset(
    attribute_id: i32,
    accumulators: &BTreeMap<i32, FamilyAccumulator>,
) -> Option<(i32, usize)> {
    accumulators.keys().find_map(|base| {
        let offset = attribute_id.checked_sub(*base)?;
        (0..FAMILY_WIDTH as i32)
            .contains(&offset)
            .then_some((*base, offset as usize))
    })
}

fn equal_value_groups(values: &[(usize, i64)]) -> Vec<Vec<usize>> {
    let mut by_value = BTreeMap::<i64, Vec<usize>>::new();
    for (offset, value) in values {
        by_value.entry(*value).or_default().push(*offset);
    }
    by_value
        .into_values()
        .filter(|group| group.len() > 1)
        .collect()
}

fn divide_with_rounding(numerator: i128, denominator: i64, rounding: RoundingMode) -> Option<i64> {
    if denominator <= 0 {
        return None;
    }
    let denominator = i128::from(denominator);
    let quotient = numerator.checked_div(denominator)?;
    let remainder = numerator.checked_rem(denominator)?;
    let rounded = match rounding {
        RoundingMode::TruncTowardZero => quotient,
        RoundingMode::Floor => {
            if remainder != 0 && numerator < 0 {
                quotient.checked_sub(1)?
            } else {
                quotient
            }
        }
        RoundingMode::Ceil => {
            if remainder != 0 && numerator > 0 {
                quotient.checked_add(1)?
            } else {
                quotient
            }
        }
        RoundingMode::NearestHalfAwayFromZero | RoundingMode::NearestHalfToEven => {
            let doubled_remainder = remainder.abs().checked_mul(2)?;
            let should_adjust = if doubled_remainder > denominator {
                true
            } else if doubled_remainder < denominator {
                false
            } else {
                match rounding {
                    RoundingMode::NearestHalfAwayFromZero => true,
                    RoundingMode::NearestHalfToEven => quotient % 2 != 0,
                    _ => unreachable!(),
                }
            };
            if should_adjust {
                quotient.checked_add(if numerator < 0 { -1 } else { 1 })?
            } else {
                quotient
            }
        }
    };
    i64::try_from(rounded).ok()
}

fn scaled_with_rounding(value: i64, rate: i64, scale: i64, rounding: RoundingMode) -> Option<i64> {
    let multiplier = i128::from(scale).checked_add(i128::from(rate))?;
    let numerator = i128::from(value).checked_mul(multiplier)?;
    divide_with_rounding(numerator, scale, rounding)
}

fn scaled_component_with_rounding(
    value: i64,
    rate: i64,
    scale: i64,
    rounding: RoundingMode,
) -> Option<i64> {
    let numerator = i128::from(value).checked_mul(i128::from(rate))?;
    divide_with_rounding(numerator, scale, rounding)
}

fn scaled_delta_with_rounding(
    value: i64,
    old_rate: i64,
    new_rate: i64,
    scale: i64,
    rounding: RoundingMode,
) -> Option<i64> {
    let old = scaled_component_with_rounding(value, old_rate, scale, rounding)?;
    let new = scaled_component_with_rounding(value, new_rate, scale, rounding)?;
    new.checked_sub(old)
}

fn observe_optional(
    check: &mut FormulaAccumulator,
    source: &SnapshotExample,
    actual: Option<i64>,
    predicted: Option<i64>,
    example_limit: usize,
) {
    if let Some(actual) = actual {
        check.observe(source, actual, predicted, example_limit);
    }
}

fn complete(values: &[Option<i64>; FAMILY_WIDTH]) -> bool {
    values.iter().all(Option::is_some)
}

fn push_example(target: &mut Vec<SnapshotExample>, example: &SnapshotExample, limit: usize) {
    if target.len() < limit {
        target.push(example.clone());
    }
}

fn decode_attribute(attribute: &rlogs_events::EntityAttribute) -> Option<i64> {
    if let Some(EntityAttributeValue::Integer(value)) = attribute.decoded {
        return Some(value);
    }
    decode_varint(&attribute.raw_value).map(|value| value as i64)
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
    let output = PathBuf::from(take_value(&mut values, "--output")?);
    let example_limit = take_optional_value(&mut values, "--example-limit")
        .map(|value| parse_usize(value, "--example-limit"))
        .transpose()?
        .unwrap_or(DEFAULT_EXAMPLE_LIMIT);
    let mut families = BTreeSet::new();
    while let Some(position) = values.iter().position(|value| value == "--family") {
        if position + 1 >= values.len() {
            return Err("--family requires the base numeric attribute ID".to_owned());
        }
        let raw = values.remove(position + 1);
        values.remove(position);
        families.insert(
            raw.to_string_lossy()
                .parse::<i32>()
                .map_err(|_| "--family requires the base numeric attribute ID".to_owned())?,
        );
    }
    let mut cross_transition_anchors = BTreeSet::new();
    while let Some(position) = values
        .iter()
        .position(|value| value == "--cross-transition-anchor")
    {
        if position + 1 >= values.len() {
            return Err(
                "--cross-transition-anchor requires <attribute-id>:<signed-delta>".to_owned(),
            );
        }
        let raw = values.remove(position + 1).to_string_lossy().into_owned();
        values.remove(position);
        let Some((attribute_id, delta)) = raw.split_once(':') else {
            return Err(
                "--cross-transition-anchor requires <attribute-id>:<signed-delta>".to_owned(),
            );
        };
        cross_transition_anchors.insert(CrossTransitionAnchor {
            attribute_id: attribute_id
                .parse::<i32>()
                .map_err(|_| "--cross-transition-anchor attribute ID must be an i32".to_owned())?,
            delta: delta
                .parse::<i64>()
                .map_err(|_| "--cross-transition-anchor delta must be a signed i64".to_owned())?,
        });
    }
    let mut rlogs = Vec::new();
    while let Some(position) = values.iter().position(|value| value == "--rlog") {
        if position + 1 >= values.len() {
            return Err("--rlog requires a path".to_owned());
        }
        rlogs.push(PathBuf::from(values.remove(position + 1)));
        values.remove(position);
    }
    if families.is_empty() || rlogs.is_empty() || !values.is_empty() {
        return Err(usage());
    }
    for anchor in &cross_transition_anchors {
        if !families.iter().any(|base| {
            anchor.attribute_id >= *base && anchor.attribute_id < *base + FAMILY_WIDTH as i32
        }) {
            return Err(format!(
                "cross-transition anchor attribute {} is not in a selected family",
                anchor.attribute_id
            ));
        }
    }
    Ok(Arguments {
        expected_game_build,
        families,
        cross_transition_anchors,
        rlogs,
        output,
        example_limit,
    })
}

fn parse_usize(value: OsString, flag: &str) -> Result<usize, String> {
    value
        .to_string_lossy()
        .parse::<usize>()
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
        return None;
    }
    let value = values.remove(position + 1);
    values.remove(position);
    Some(value)
}

fn usage() -> String {
    "usage: rlogs-bpsr-attribute-family-formula-proof --expected-game-build <numeric-build> --family <base-id> [--family <base-id> ...] [--cross-transition-anchor <attribute-id>:<signed-delta> ...] --rlog <current-decoder.rlog> [--rlog <current-decoder.rlog> ...] --output <audit.json> [--example-limit <count>]".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transition_example(
        sequence: u64,
        deltas_since_last_seen: [Option<i64>; FAMILY_WIDTH],
    ) -> SnapshotExample {
        SnapshotExample {
            rlog: "fixture.rlog".to_owned(),
            session_id: "fixture".to_owned(),
            run_ordinal: 1,
            sequence,
            observed_micros: 123,
            actor_entity_uuid: 42,
            updated_offsets: deltas_since_last_seen
                .iter()
                .enumerate()
                .filter_map(|(offset, value)| value.map(|_| offset))
                .collect(),
            packet_values: [None; FAMILY_WIDTH],
            last_seen_before: [None; FAMILY_WIDTH],
            last_seen_after: [None; FAMILY_WIDTH],
            deltas_since_last_seen,
        }
    }

    #[test]
    fn cross_family_anchor_retains_the_complete_same_packet_delta_vector() {
        let anchors = BTreeSet::from([CrossTransitionAnchor {
            attribute_id: 11_033,
            delta: 4_434,
        }]);
        let batch = vec![
            (
                11_030,
                transition_example(7, [Some(4_434), None, None, Some(4_434), None, None]),
            ),
            (
                11_120,
                transition_example(7, [Some(6_651), Some(6_651), Some(6_651), None, None, None]),
            ),
        ];
        let mut accumulator = CrossFamilyTransitionAccumulator::default();
        let selected_families = vec![11_030, 11_120];
        accumulator.observe(&batch, &anchors, &selected_families, 4);
        let report = accumulator.finish(anchors.into_iter().collect(), selected_families);

        assert_eq!(report.observed_batches, 1);
        assert_eq!(report.complete_selected_family_batches, 1);
        assert_eq!(report.incomplete_selected_family_batches, 0);
        assert_eq!(report.patterns.len(), 1);
        assert!(
            report.patterns[0]
                .changed_members
                .iter()
                .any(|member| { member.attribute_id == 11_120 && member.delta == 6_651 })
        );
        assert!(!report.formula_authority);
        assert!(!report.ui_display_authority);
    }

    #[test]
    fn max_hp_extra_percent_transition_uses_ten_thousandths() {
        assert_eq!(
            scaled_delta_with_rounding(
                569_116,
                4_080,
                4_533,
                10_000,
                RoundingMode::TruncTowardZero,
            ),
            Some(25_781)
        );
        assert_eq!(
            scaled_delta_with_rounding(
                577_153,
                1_414,
                2_356,
                10_000,
                RoundingMode::TruncTowardZero,
            ),
            Some(54_368)
        );
    }

    #[test]
    fn max_hp_percent_transition_preserves_component_rounding_residual() {
        assert_eq!(
            scaled_delta_with_rounding(
                292_021,
                1_294,
                1_894,
                10_000,
                RoundingMode::TruncTowardZero,
            ),
            Some(17_521)
        );
    }

    #[test]
    fn percentage_scale_one_hundred_is_not_equivalent() {
        assert_eq!(
            scaled_delta_with_rounding(569_116, 4_080, 4_533, 100, RoundingMode::TruncTowardZero,),
            Some(2_578_096)
        );
        assert_ne!(
            scaled_delta_with_rounding(569_116, 4_080, 4_533, 100, RoundingMode::TruncTowardZero,),
            scaled_delta_with_rounding(
                569_116,
                4_080,
                4_533,
                10_000,
                RoundingMode::TruncTowardZero,
            )
        );
    }

    #[test]
    fn signed_rounding_modes_are_explicit_at_half_and_non_half_boundaries() {
        assert_eq!(
            divide_with_rounding(5, 2, RoundingMode::TruncTowardZero),
            Some(2)
        );
        assert_eq!(divide_with_rounding(-5, 2, RoundingMode::Floor), Some(-3));
        assert_eq!(divide_with_rounding(-5, 2, RoundingMode::Ceil), Some(-2));
        assert_eq!(
            divide_with_rounding(5, 2, RoundingMode::NearestHalfAwayFromZero),
            Some(3)
        );
        assert_eq!(
            divide_with_rounding(-5, 2, RoundingMode::NearestHalfAwayFromZero),
            Some(-3)
        );
        assert_eq!(
            divide_with_rounding(5, 2, RoundingMode::NearestHalfToEven),
            Some(2)
        );
        assert_eq!(
            divide_with_rounding(7, 2, RoundingMode::NearestHalfToEven),
            Some(4)
        );
    }

    #[test]
    fn attack_family_pair_distinguishes_truncation_from_nearest_rounding() {
        assert_eq!(
            scaled_with_rounding(5_959, 1_600, 10_000, RoundingMode::TruncTowardZero),
            Some(6_912)
        );
        assert_eq!(
            scaled_with_rounding(8_531, 1_600, 10_000, RoundingMode::TruncTowardZero),
            Some(9_895)
        );
        assert_eq!(
            scaled_with_rounding(8_531, 1_600, 10_000, RoundingMode::NearestHalfAwayFromZero,),
            Some(9_896)
        );
        assert_eq!(
            scaled_with_rounding(8_531, 1_600, 10_000, RoundingMode::NearestHalfToEven,),
            Some(9_896)
        );
    }
}
