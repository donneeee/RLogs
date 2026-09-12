//! Pure, fail-closed coordinate planning for a future automarker canary.
//!
//! Positions are game-world `[x, y, z]` coordinates in one caller-certified
//! frame. The player origin, saved target, and every indicator observation must
//! share that frame; this planner never transforms coordinates. Range checks
//! use Euclidean 3D distance, which is deliberately conservative on slopes.
//! A foreground loss or lifecycle, root, or scene-context change invalidates
//! the session; callers must discard it and construct a new calibrated session.
//!
//! This module has no OS, process, packet, input, click, confirmation, timing,
//! or activation authority. It may only return one bounded integer relative-
//! mouse delta. The caller must report the exact emitted delta and command ID
//! before another proposal can be issued.

use std::collections::VecDeque;

pub const HARD_PLAYER_TARGET_LIMIT: f64 = 18.0;
pub const HARD_INDICATOR_TARGET_LIMIT: f64 = 36.0;
const RANGE_EPSILON: f64 = 1.0e-6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CertifiedMouseDelta {
    /// None is reserved for caller-owned symmetric calibration moves.
    pub planner_command_id: Option<u32>,
    /// Exact integer delta that the caller certifies was emitted.
    pub delta: [i32; 2],
    /// True only when no competing/user mouse input was observed.
    pub exclusive_input_ownership: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SettledIndicatorObservation {
    pub position: [f64; 3],
    /// Motion applied after the preceding observation; the first sample is None.
    pub applied_mouse_delta: Option<CertifiedMouseDelta>,
    pub lifecycle_generation: u64,
    pub root_generation: u64,
    pub context_identity: u64,
    pub settled: bool,
    pub foreground: bool,
    pub context_continuous: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CoordinatePlannerConfig {
    pub normalized_fit_damping: f64,
    pub normalized_solve_damping: f64,
    pub maximum_mouse_step: f64,
    pub maximum_mouse_axis_step: i32,
    pub minimum_excitation_energy: f64,
    pub maximum_condition_number: f64,
    pub minimum_predicted_improvement: f64,
    pub maximum_tangent_residual_fraction: f64,
    pub absolute_model_tolerance: f64,
    pub relative_model_tolerance: f64,
    pub arrival_tolerance: f64,
    pub maximum_transitions: usize,
    pub maximum_commands: u32,
    pub maximum_cumulative_motion: f64,
    pub minimum_improvement_ratio: f64,
    pub poor_ratio_limit: u8,
    pub trust_shrink_factor: f64,
    pub trust_growth_factor: f64,
    pub monotonic_regression_tolerance: f64,
    pub best_distance_regression_tolerance: f64,
    pub no_improvement_limit: u8,
    pub cycle_position_tolerance: f64,
}

impl Default for CoordinatePlannerConfig {
    fn default() -> Self {
        Self {
            normalized_fit_damping: 1.0e-6,
            normalized_solve_damping: 1.0e-4,
            maximum_mouse_step: 8.0,
            maximum_mouse_axis_step: 8,
            minimum_excitation_energy: 1.0e-3,
            maximum_condition_number: 5_000.0,
            minimum_predicted_improvement: 0.005,
            maximum_tangent_residual_fraction: 0.98,
            absolute_model_tolerance: 0.035,
            relative_model_tolerance: 0.15,
            // Indicator observations have a realistic 5–10 cm noise floor.
            arrival_tolerance: 0.075,
            maximum_transitions: 20,
            maximum_commands: 24,
            maximum_cumulative_motion: 120.0,
            minimum_improvement_ratio: 0.20,
            poor_ratio_limit: 2,
            trust_shrink_factor: 0.5,
            trust_growth_factor: 1.15,
            monotonic_regression_tolerance: 0.05,
            best_distance_regression_tolerance: 0.15,
            no_improvement_limit: 3,
            cycle_position_tolerance: 0.04,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlannerAbort {
    InvalidConfiguration,
    NonFiniteInput,
    ZeroSavedTargetSentinel,
    TargetOutsidePlayerRange,
    IndicatorTargetOutsidePlanningRange,
    InsufficientObservations,
    UnsettledSample,
    ForegroundLost,
    InputOwnershipLost,
    ContextDiscontinuity,
    CalibrationNotSymmetricOrthogonal,
    InsufficientExcitation,
    IllConditionedGeometry,
    TargetLocallyUnreachable,
    ModelInconsistent,
    ResolutionLimited,
    CommandReceiptMismatch,
    CommandBudgetExhausted,
    CumulativeMotionBudgetExhausted,
    NoImprovement,
    Diverging,
    CycleDetected,
    RecalibrationRequired,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PlannerDecision {
    Arrived { distance: f64 },
    Move(CoordinateMoveProposal),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CoordinateMoveProposal {
    pub command_id: u32,
    /// The only actionable datum; deliberately no click or confirmation field.
    pub relative_mouse_delta: [i32; 2],
    pub current_distance: f64,
    pub predicted_distance: f64,
    pub condition_number: f64,
    pub trust_region_radius: f64,
    pub trust_region_clamped: bool,
}

#[derive(Clone, Copy, Debug)]
struct PendingCommand {
    command_id: u32,
    delta: [i32; 2],
    position_before: [f64; 3],
    distance_before: f64,
    predicted_improvement: f64,
}

#[derive(Debug)]
pub struct CoordinatePlannerSession {
    config: CoordinatePlannerConfig,
    lifecycle_generation: u64,
    root_generation: u64,
    context_identity: u64,
    next_command_id: u32,
    command_count: u32,
    cumulative_motion: f64,
    trust_radius: f64,
    pending: Option<PendingCommand>,
    best_distance: Option<f64>,
    previous_distance: Option<f64>,
    poor_ratio_count: u8,
    no_improvement_count: u8,
    completed_positions: VecDeque<[f64; 3]>,
}

impl CoordinatePlannerSession {
    pub fn new(
        config: CoordinatePlannerConfig,
        lifecycle_generation: u64,
        root_generation: u64,
        context_identity: u64,
    ) -> Result<Self, PlannerAbort> {
        validate_config(config)?;
        Ok(Self {
            config,
            lifecycle_generation,
            root_generation,
            context_identity,
            next_command_id: 1,
            command_count: 0,
            cumulative_motion: 0.0,
            trust_radius: config.maximum_mouse_step,
            pending: None,
            best_distance: None,
            previous_distance: None,
            poor_ratio_count: 0,
            no_improvement_count: 0,
            completed_positions: VecDeque::with_capacity(8),
        })
    }

    pub fn command_count(&self) -> u32 {
        self.command_count
    }

    pub fn cumulative_motion(&self) -> f64 {
        self.cumulative_motion
    }

    pub fn plan(
        &mut self,
        observations: &[SettledIndicatorObservation],
        saved_target: [f64; 3],
        player_origin: [f64; 3],
        live_max_distance: f64,
    ) -> Result<PlannerDecision, PlannerAbort> {
        validate_inputs(
            observations,
            saved_target,
            player_origin,
            live_max_distance,
            self.lifecycle_generation,
            self.root_generation,
            self.context_identity,
        )?;
        require_symmetric_orthogonal_calibration(observations)?;

        let current = observations
            .last()
            .expect("validated observations")
            .position;
        let current_distance = norm3(subtract3(saved_target, current));
        let player_distance = norm3(subtract3(saved_target, player_origin));
        if player_distance > live_max_distance.min(HARD_PLAYER_TARGET_LIMIT) + RANGE_EPSILON {
            return Err(PlannerAbort::TargetOutsidePlayerRange);
        }
        if current_distance > HARD_INDICATOR_TARGET_LIMIT + RANGE_EPSILON {
            return Err(PlannerAbort::IndicatorTargetOutsidePlanningRange);
        }

        self.evaluate_pending(observations, current_distance)?;
        self.guard_progress(current, current_distance)?;
        if current_distance <= self.config.arrival_tolerance {
            return Ok(PlannerDecision::Arrived {
                distance: current_distance,
            });
        }
        if self.command_count >= self.config.maximum_commands {
            return Err(PlannerAbort::CommandBudgetExhausted);
        }

        let fit_observations = if self.poor_ratio_count > 0 {
            // Once a response underperforms, do not contaminate the model with
            // any canary response until a fresh symmetric calibration occurs.
            &observations[..5]
        } else {
            observations
        };
        let start = fit_observations
            .len()
            .saturating_sub(self.config.maximum_transitions + 1);
        let model = fit_normalized_jacobian(&fit_observations[start..], self.config)?;
        let error = subtract3(saved_target, current);
        // Reachability uses an undamped tangent projection. Solve damping cannot
        // alter the reachable/unreachable classification.
        let tangent_delta =
            solve_normal(model.jacobian, error, 0.0).ok_or(PlannerAbort::IllConditionedGeometry)?;
        let tangent_residual = norm3(subtract3(
            error,
            multiply_jacobian(model.jacobian, tangent_delta),
        ));
        if tangent_residual / current_distance > self.config.maximum_tangent_residual_fraction {
            return Err(PlannerAbort::TargetLocallyUnreachable);
        }

        let gram = jacobian_gram(model.jacobian);
        let solve_scale = ((gram[0][0] + gram[1][1]) * 0.5).max(f64::EPSILON);
        let continuous = solve_normal(
            model.jacobian,
            error,
            self.config.normalized_solve_damping * solve_scale,
        )
        .ok_or(PlannerAbort::IllConditionedGeometry)?;
        let bounded = bound_delta(
            continuous,
            self.trust_radius,
            self.config.maximum_mouse_axis_step,
        );
        let discrete = best_feasible_integer_delta(
            bounded.delta,
            model.jacobian,
            error,
            self.trust_radius,
            self.config.maximum_mouse_axis_step,
        )
        .ok_or(PlannerAbort::ResolutionLimited)?;
        let discrete_norm = norm2_i32(discrete);
        if discrete == [0, 0]
            || discrete_norm > self.trust_radius + RANGE_EPSILON
            || discrete
                .into_iter()
                .any(|value| value.abs() > self.config.maximum_mouse_axis_step)
        {
            return Err(PlannerAbort::ResolutionLimited);
        }
        let predicted_distance = norm3(subtract3(
            error,
            multiply_jacobian(
                model.jacobian,
                [f64::from(discrete[0]), f64::from(discrete[1])],
            ),
        ));
        let predicted_improvement = current_distance - predicted_distance;
        if !predicted_distance.is_finite()
            || predicted_improvement < self.config.minimum_predicted_improvement
        {
            return Err(PlannerAbort::ResolutionLimited);
        }
        if self.cumulative_motion + discrete_norm
            > self.config.maximum_cumulative_motion + RANGE_EPSILON
        {
            return Err(PlannerAbort::CumulativeMotionBudgetExhausted);
        }

        let command_id = self.next_command_id;
        self.next_command_id = self.next_command_id.saturating_add(1);
        self.command_count += 1;
        self.cumulative_motion += discrete_norm;
        self.pending = Some(PendingCommand {
            command_id,
            delta: discrete,
            position_before: current,
            distance_before: current_distance,
            predicted_improvement,
        });
        Ok(PlannerDecision::Move(CoordinateMoveProposal {
            command_id,
            relative_mouse_delta: discrete,
            current_distance,
            predicted_distance,
            condition_number: model.condition_number,
            trust_region_radius: self.trust_radius,
            trust_region_clamped: bounded.clamped
                || squared_distance2(continuous, discrete.map(f64::from)) > 1.0e-18,
        }))
    }

    fn evaluate_pending(
        &mut self,
        observations: &[SettledIndicatorObservation],
        current_distance: f64,
    ) -> Result<(), PlannerAbort> {
        let Some(pending) = self.pending.take() else {
            return Ok(());
        };
        let prior = observations
            .get(observations.len().saturating_sub(2))
            .ok_or(PlannerAbort::CommandReceiptMismatch)?
            .position;
        let receipt = observations.last().unwrap().applied_mouse_delta;
        if squared_distance3(prior, pending.position_before) > 1.0e-10
            || receipt.map(|value| value.planner_command_id) != Some(Some(pending.command_id))
            || receipt.map(|value| value.delta) != Some(pending.delta)
        {
            return Err(PlannerAbort::CommandReceiptMismatch);
        }

        let actual_improvement = pending.distance_before - current_distance;
        if actual_improvement < -self.config.monotonic_regression_tolerance {
            return Err(PlannerAbort::Diverging);
        }
        let ratio = actual_improvement / pending.predicted_improvement.max(f64::EPSILON);
        if ratio < self.config.minimum_improvement_ratio {
            self.poor_ratio_count = self.poor_ratio_count.saturating_add(1);
            self.trust_radius = (self.trust_radius * self.config.trust_shrink_factor).max(1.0);
            if self.poor_ratio_count >= self.config.poor_ratio_limit {
                return Err(PlannerAbort::RecalibrationRequired);
            }
        } else {
            self.poor_ratio_count = 0;
            self.trust_radius = (self.trust_radius * self.config.trust_growth_factor)
                .min(self.config.maximum_mouse_step);
        }
        if actual_improvement <= self.config.minimum_predicted_improvement {
            self.no_improvement_count = self.no_improvement_count.saturating_add(1);
            if self.no_improvement_count >= self.config.no_improvement_limit {
                return Err(PlannerAbort::NoImprovement);
            }
        } else {
            self.no_improvement_count = 0;
        }
        Ok(())
    }

    fn guard_progress(
        &mut self,
        current: [f64; 3],
        current_distance: f64,
    ) -> Result<(), PlannerAbort> {
        if self.previous_distance.is_some_and(|previous| {
            current_distance > previous + self.config.monotonic_regression_tolerance
        }) || self.best_distance.is_some_and(|best| {
            current_distance > best + self.config.best_distance_regression_tolerance
        }) {
            return Err(PlannerAbort::Diverging);
        }
        if self.completed_positions.iter().any(|position| {
            norm3(subtract3(current, *position)) <= self.config.cycle_position_tolerance
        }) && self.best_distance.is_some_and(|best| {
            current_distance >= best - self.config.minimum_predicted_improvement
        }) {
            return Err(PlannerAbort::CycleDetected);
        }
        self.previous_distance = Some(current_distance);
        self.best_distance = Some(
            self.best_distance
                .map_or(current_distance, |best| best.min(current_distance)),
        );
        self.completed_positions.push_back(current);
        while self.completed_positions.len() > 8 {
            self.completed_positions.pop_front();
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct LocalModel {
    jacobian: [[f64; 2]; 3],
    condition_number: f64,
}

fn validate_inputs(
    observations: &[SettledIndicatorObservation],
    target: [f64; 3],
    player: [f64; 3],
    live_max_distance: f64,
    lifecycle: u64,
    root: u64,
    context: u64,
) -> Result<(), PlannerAbort> {
    if !finite3(target)
        || !finite3(player)
        || !live_max_distance.is_finite()
        || live_max_distance <= 0.0
        || observations.iter().any(|sample| !finite3(sample.position))
    {
        return Err(PlannerAbort::NonFiniteInput);
    }
    if target == [0.0; 3] {
        return Err(PlannerAbort::ZeroSavedTargetSentinel);
    }
    if observations.len() < 5
        || observations[0].applied_mouse_delta.is_some()
        || observations[1..]
            .iter()
            .any(|sample| sample.applied_mouse_delta.is_none())
    {
        return Err(PlannerAbort::InsufficientObservations);
    }
    for sample in observations {
        if !sample.settled {
            return Err(PlannerAbort::UnsettledSample);
        }
        if !sample.foreground {
            return Err(PlannerAbort::ForegroundLost);
        }
        if !sample.context_continuous
            || sample.lifecycle_generation != lifecycle
            || sample.root_generation != root
            || sample.context_identity != context
        {
            return Err(PlannerAbort::ContextDiscontinuity);
        }
        if sample
            .applied_mouse_delta
            .is_some_and(|delta| !delta.exclusive_input_ownership)
        {
            return Err(PlannerAbort::InputOwnershipLost);
        }
    }
    Ok(())
}

fn require_symmetric_orthogonal_calibration(
    observations: &[SettledIndicatorObservation],
) -> Result<(), PlannerAbort> {
    let calibration = observations[1..5]
        .iter()
        .map(|sample| sample.applied_mouse_delta.unwrap())
        .collect::<Vec<_>>();
    if calibration
        .iter()
        .any(|sample| sample.planner_command_id.is_some())
    {
        return Err(PlannerAbort::CalibrationNotSymmetricOrthogonal);
    }
    let vectors = calibration
        .iter()
        .map(|sample| sample.delta)
        .collect::<Vec<_>>();
    let mut pairs = Vec::new();
    let mut used = [false; 4];
    for index in 0..4 {
        if used[index] {
            continue;
        }
        let Some(opposite) = (index + 1..4).find(|candidate| {
            !used[*candidate]
                && vectors[*candidate][0] == -vectors[index][0]
                && vectors[*candidate][1] == -vectors[index][1]
        }) else {
            return Err(PlannerAbort::CalibrationNotSymmetricOrthogonal);
        };
        used[index] = true;
        used[opposite] = true;
        pairs.push(vectors[index]);
    }
    let dot = i64::from(pairs[0][0]) * i64::from(pairs[1][0])
        + i64::from(pairs[0][1]) * i64::from(pairs[1][1]);
    if pairs.len() != 2 || pairs.contains(&[0, 0]) || dot != 0 {
        return Err(PlannerAbort::CalibrationNotSymmetricOrthogonal);
    }
    Ok(())
}

fn fit_normalized_jacobian(
    observations: &[SettledIndicatorObservation],
    config: CoordinatePlannerConfig,
) -> Result<LocalModel, PlannerAbort> {
    let transitions = observations.len() - 1;
    let input_scale = (observations[1..]
        .iter()
        .map(|sample| {
            let delta = sample.applied_mouse_delta.unwrap().delta;
            f64::from(delta[0]).powi(2) + f64::from(delta[1]).powi(2)
        })
        .sum::<f64>()
        / transitions as f64)
        .sqrt();
    let output_energy = observations
        .windows(2)
        .map(|pair| {
            let change = subtract3(pair[1].position, pair[0].position);
            dot3(change, change)
        })
        .sum::<f64>();
    let output_scale = (output_energy / transitions as f64).sqrt();
    if input_scale <= f64::EPSILON || output_scale <= f64::EPSILON {
        return Err(PlannerAbort::InsufficientExcitation);
    }

    let mut gram = [[0.0; 2]; 2];
    let mut cross = [[0.0; 2]; 3];
    for pair in observations.windows(2) {
        let raw = pair[1].applied_mouse_delta.unwrap().delta;
        let input = [
            f64::from(raw[0]) / input_scale,
            f64::from(raw[1]) / input_scale,
        ];
        let change =
            subtract3(pair[1].position, pair[0].position).map(|value| value / output_scale);
        gram[0][0] += input[0] * input[0];
        gram[0][1] += input[0] * input[1];
        gram[1][0] += input[1] * input[0];
        gram[1][1] += input[1] * input[1];
        for axis in 0..3 {
            cross[axis][0] += change[axis] * input[0];
            cross[axis][1] += change[axis] * input[1];
        }
    }
    let (input_minimum, input_maximum) = symmetric_eigenvalues(gram);
    if input_minimum < config.minimum_excitation_energy {
        return Err(PlannerAbort::InsufficientExcitation);
    }
    let input_condition = input_maximum / input_minimum;
    if !input_condition.is_finite() || input_condition > config.maximum_condition_number {
        return Err(PlannerAbort::IllConditionedGeometry);
    }
    let inverse = inverse2([
        [gram[0][0] + config.normalized_fit_damping, gram[0][1]],
        [gram[1][0], gram[1][1] + config.normalized_fit_damping],
    ])
    .ok_or(PlannerAbort::IllConditionedGeometry)?;
    let scale = output_scale / input_scale;
    let mut jacobian = [[0.0; 2]; 3];
    for axis in 0..3 {
        jacobian[axis][0] =
            (cross[axis][0] * inverse[0][0] + cross[axis][1] * inverse[1][0]) * scale;
        jacobian[axis][1] =
            (cross[axis][0] * inverse[0][1] + cross[axis][1] * inverse[1][1]) * scale;
    }
    let (output_minimum, output_maximum) = symmetric_eigenvalues(jacobian_gram(jacobian));
    if output_minimum <= f64::EPSILON {
        return Err(PlannerAbort::IllConditionedGeometry);
    }
    let output_condition = output_maximum / output_minimum;
    if !output_condition.is_finite() || output_condition > config.maximum_condition_number {
        return Err(PlannerAbort::IllConditionedGeometry);
    }
    let residual_rms = (observations
        .windows(2)
        .map(|pair| {
            let raw = pair[1].applied_mouse_delta.unwrap().delta;
            let actual = subtract3(pair[1].position, pair[0].position);
            let predicted = multiply_jacobian(jacobian, [f64::from(raw[0]), f64::from(raw[1])]);
            let residual = subtract3(actual, predicted);
            dot3(residual, residual)
        })
        .sum::<f64>()
        / transitions as f64)
        .sqrt();
    if residual_rms
        > config.absolute_model_tolerance + config.relative_model_tolerance * output_scale
    {
        return Err(PlannerAbort::ModelInconsistent);
    }
    Ok(LocalModel {
        jacobian,
        condition_number: input_condition.max(output_condition),
    })
}

#[derive(Clone, Copy)]
struct BoundedDelta {
    delta: [f64; 2],
    clamped: bool,
}

fn best_feasible_integer_delta(
    bounded: [f64; 2],
    jacobian: [[f64; 2]; 3],
    error: [f64; 3],
    radius: f64,
    axis_limit: i32,
) -> Option<[i32; 2]> {
    let axis_candidates = |value: f64| {
        [
            value.floor() as i32,
            value.ceil() as i32,
            value.round() as i32,
            value.trunc() as i32,
            0,
        ]
    };
    let xs = axis_candidates(bounded[0]);
    let ys = axis_candidates(bounded[1]);
    let mut best: Option<([i32; 2], f64)> = None;
    for x in xs {
        for y in ys {
            let candidate = [x, y];
            if candidate == [0, 0]
                || x.abs() > axis_limit
                || y.abs() > axis_limit
                || norm2_i32(candidate) > radius + RANGE_EPSILON
            {
                continue;
            }
            let residual = subtract3(error, multiply_jacobian(jacobian, candidate.map(f64::from)));
            let score = dot3(residual, residual);
            if best.is_none_or(|(_, best_score)| score < best_score) {
                best = Some((candidate, score));
            }
        }
    }
    best.map(|(candidate, _)| candidate)
}

fn bound_delta(value: [f64; 2], radius: f64, axis_limit: i32) -> BoundedDelta {
    let mut delta =
        value.map(|component| component.clamp(-f64::from(axis_limit), f64::from(axis_limit)));
    let length = norm2(delta);
    if length > radius {
        let scale = radius / length;
        delta[0] *= scale;
        delta[1] *= scale;
    }
    BoundedDelta {
        delta,
        clamped: squared_distance2(delta, value) > 1.0e-18,
    }
}

fn solve_normal(jacobian: [[f64; 2]; 3], error: [f64; 3], damping: f64) -> Option<[f64; 2]> {
    let mut normal = jacobian_gram(jacobian);
    normal[0][0] += damping;
    normal[1][1] += damping;
    let inverse = inverse2(normal)?;
    let rhs: [f64; 2] = [
        jacobian
            .iter()
            .zip(error)
            .map(|(row, value)| row[0] * value)
            .sum(),
        jacobian
            .iter()
            .zip(error)
            .map(|(row, value)| row[1] * value)
            .sum(),
    ];
    Some([
        inverse[0][0] * rhs[0] + inverse[0][1] * rhs[1],
        inverse[1][0] * rhs[0] + inverse[1][1] * rhs[1],
    ])
}

fn jacobian_gram(jacobian: [[f64; 2]; 3]) -> [[f64; 2]; 2] {
    [
        [
            jacobian.iter().map(|row| row[0] * row[0]).sum(),
            jacobian.iter().map(|row| row[0] * row[1]).sum(),
        ],
        [
            jacobian.iter().map(|row| row[1] * row[0]).sum(),
            jacobian.iter().map(|row| row[1] * row[1]).sum(),
        ],
    ]
}

fn validate_config(config: CoordinatePlannerConfig) -> Result<(), PlannerAbort> {
    let positive = [
        config.normalized_fit_damping,
        config.normalized_solve_damping,
        config.maximum_mouse_step,
        config.minimum_excitation_energy,
        config.maximum_condition_number,
        config.minimum_predicted_improvement,
        config.absolute_model_tolerance,
        config.maximum_cumulative_motion,
        config.monotonic_regression_tolerance,
        config.best_distance_regression_tolerance,
        config.cycle_position_tolerance,
    ]
    .into_iter()
    .all(|value| value.is_finite() && value > 0.0);
    if !positive
        || config.maximum_mouse_axis_step < 1
        || config.maximum_transitions < 4
        || config.maximum_commands == 0
        || config.poor_ratio_limit == 0
        || config.no_improvement_limit == 0
        || !config.relative_model_tolerance.is_finite()
        || config.relative_model_tolerance < 0.0
        || !(0.0..=1.0).contains(&config.minimum_improvement_ratio)
        || !(0.0..1.0).contains(&config.maximum_tangent_residual_fraction)
        || !(0.0..1.0).contains(&config.trust_shrink_factor)
        || !config.trust_growth_factor.is_finite()
        || config.trust_growth_factor < 1.0
        || !(0.05..=0.10).contains(&config.arrival_tolerance)
    {
        return Err(PlannerAbort::InvalidConfiguration);
    }
    Ok(())
}

fn inverse2(value: [[f64; 2]; 2]) -> Option<[[f64; 2]; 2]> {
    let determinant = value[0][0] * value[1][1] - value[0][1] * value[1][0];
    if !determinant.is_finite() || determinant.abs() <= f64::EPSILON {
        return None;
    }
    Some([
        [value[1][1] / determinant, -value[0][1] / determinant],
        [-value[1][0] / determinant, value[0][0] / determinant],
    ])
}

fn symmetric_eigenvalues(value: [[f64; 2]; 2]) -> (f64, f64) {
    let trace = value[0][0] + value[1][1];
    let discriminant = ((value[0][0] - value[1][1]).powi(2) + 4.0 * value[0][1] * value[1][0])
        .max(0.0)
        .sqrt();
    ((trace - discriminant) * 0.5, (trace + discriminant) * 0.5)
}

fn multiply_jacobian(jacobian: [[f64; 2]; 3], input: [f64; 2]) -> [f64; 3] {
    [
        jacobian[0][0] * input[0] + jacobian[0][1] * input[1],
        jacobian[1][0] * input[0] + jacobian[1][1] * input[1],
        jacobian[2][0] * input[0] + jacobian[2][1] * input[1],
    ]
}

fn subtract3(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [left[0] - right[0], left[1] - right[1], left[2] - right[2]]
}

fn dot3(left: [f64; 3], right: [f64; 3]) -> f64 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}

fn norm2(value: [f64; 2]) -> f64 {
    (value[0] * value[0] + value[1] * value[1]).sqrt()
}

fn norm2_i32(value: [i32; 2]) -> f64 {
    norm2(value.map(f64::from))
}

fn norm3(value: [f64; 3]) -> f64 {
    dot3(value, value).sqrt()
}

fn squared_distance2(left: [f64; 2], right: [f64; 2]) -> f64 {
    (left[0] - right[0]).powi(2) + (left[1] - right[1]).powi(2)
}

fn squared_distance3(left: [f64; 3], right: [f64; 3]) -> f64 {
    dot3(subtract3(left, right), subtract3(left, right))
}

fn finite3(value: [f64; 3]) -> bool {
    value.into_iter().all(f64::is_finite)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIFE: u64 = 7;
    const ROOT: u64 = 11;
    const CONTEXT: u64 = 13;
    const FLAT: [[f64; 2]; 3] = [[0.40, 0.05], [0.0, 0.0], [-0.10, 0.30]];
    const SLOPED: [[f64; 2]; 3] = [[0.35, 0.02], [0.08, -0.06], [-0.04, 0.28]];

    fn sample(
        position: [f64; 3],
        delta: Option<CertifiedMouseDelta>,
    ) -> SettledIndicatorObservation {
        SettledIndicatorObservation {
            position,
            applied_mouse_delta: delta,
            lifecycle_generation: LIFE,
            root_generation: ROOT,
            context_identity: CONTEXT,
            settled: true,
            foreground: true,
            context_continuous: true,
        }
    }

    fn calibration(jacobian: [[f64; 2]; 3], scale: i32) -> Vec<SettledIndicatorObservation> {
        let inputs = [[scale, 0], [-scale, 0], [0, scale], [0, -scale]];
        let mut position = [1.0, 0.5, 1.0];
        let mut observations = vec![sample(position, None)];
        for input in inputs {
            let change = multiply_jacobian(jacobian, input.map(f64::from));
            position = [
                position[0] + change[0],
                position[1] + change[1],
                position[2] + change[2],
            ];
            observations.push(sample(
                position,
                Some(CertifiedMouseDelta {
                    planner_command_id: None,
                    delta: input,
                    exclusive_input_ownership: true,
                }),
            ));
        }
        observations
    }

    fn session(config: CoordinatePlannerConfig) -> CoordinatePlannerSession {
        CoordinatePlannerSession::new(config, LIFE, ROOT, CONTEXT).unwrap()
    }

    fn proposal(decision: PlannerDecision) -> CoordinateMoveProposal {
        match decision {
            PlannerDecision::Move(value) => value,
            PlannerDecision::Arrived { .. } => panic!("expected a move"),
        }
    }

    fn apply(
        observations: &mut Vec<SettledIndicatorObservation>,
        jacobian: [[f64; 2]; 3],
        proposal: CoordinateMoveProposal,
        effectiveness: f64,
    ) {
        let input = proposal.relative_mouse_delta;
        let change = multiply_jacobian(
            jacobian,
            input.map(|value| f64::from(value) * effectiveness),
        );
        let previous = observations.last().unwrap().position;
        observations.push(sample(
            [
                previous[0] + change[0],
                previous[1] + change[1],
                previous[2] + change[2],
            ],
            Some(CertifiedMouseDelta {
                planner_command_id: Some(proposal.command_id),
                delta: input,
                exclusive_input_ownership: true,
            }),
        ));
    }

    #[test]
    fn converges_on_flat_and_sloped_ground_with_integer_commands() {
        for jacobian in [FLAT, SLOPED] {
            let exact_change = multiply_jacobian(jacobian, [5.0, -4.0]);
            let target = [
                1.0 + exact_change[0],
                0.5 + exact_change[1],
                1.0 + exact_change[2],
            ];
            let mut observations = calibration(jacobian, 4);
            let mut planner = session(CoordinatePlannerConfig::default());
            for _ in 0..20 {
                match planner.plan(&observations, target, target, 18.0).unwrap() {
                    PlannerDecision::Arrived { .. } => break,
                    PlannerDecision::Move(value) => apply(&mut observations, jacobian, value, 1.0),
                }
            }
            assert!(norm3(subtract3(target, observations.last().unwrap().position)) <= 0.10);
        }
    }

    #[test]
    fn enforces_zero_sentinel_player_epsilon_and_indicator_limits() {
        let observations = calibration(FLAT, 4);
        assert_eq!(
            session(CoordinatePlannerConfig::default()).plan(
                &observations,
                [0.0; 3],
                [0.0; 3],
                18.0
            ),
            Err(PlannerAbort::ZeroSavedTargetSentinel)
        );
        assert!(
            session(CoordinatePlannerConfig::default())
                .plan(
                    &observations,
                    [18.0 + RANGE_EPSILON * 0.5, 0.5, 1.0],
                    [0.0, 0.5, 1.0],
                    18.0
                )
                .is_ok()
        );
        assert_eq!(
            session(CoordinatePlannerConfig::default()).plan(
                &observations,
                [18.0 + RANGE_EPSILON * 2.0, 0.5, 1.0],
                [0.0, 0.5, 1.0],
                18.0
            ),
            Err(PlannerAbort::TargetOutsidePlayerRange)
        );
        let far = calibration(FLAT, 4)
            .into_iter()
            .map(|mut value| {
                value.position[0] -= 36.1;
                value
            })
            .collect::<Vec<_>>();
        assert_eq!(
            session(CoordinatePlannerConfig::default()).plan(
                &far,
                [1.0, 0.5, 1.0],
                [1.0, 0.5, 1.0],
                18.0
            ),
            Err(PlannerAbort::IndicatorTargetOutsidePlanningRange)
        );
    }

    #[test]
    fn requires_symmetric_orthogonal_full_rank_calibration() {
        let target = [3.0, 0.5, 2.0];
        let mut asymmetric = calibration(FLAT, 4);
        asymmetric[2].applied_mouse_delta.as_mut().unwrap().delta = [-3, 0];
        assert_eq!(
            session(CoordinatePlannerConfig::default()).plan(&asymmetric, target, target, 18.0),
            Err(PlannerAbort::CalibrationNotSymmetricOrthogonal)
        );
        let rank_one = [[0.4, 0.8], [0.0, 0.0], [0.1, 0.2]];
        assert_eq!(
            session(CoordinatePlannerConfig::default()).plan(
                &calibration(rank_one, 4),
                target,
                target,
                18.0
            ),
            Err(PlannerAbort::IllConditionedGeometry)
        );
    }

    #[test]
    fn unreachable_normal_component_is_independent_of_solve_damping() {
        let observations = calibration(FLAT, 4);
        let target = [1.0, 1.5, 1.0];
        for damping in [1.0e-8, 1.0e-1] {
            let config = CoordinatePlannerConfig {
                normalized_solve_damping: damping,
                ..CoordinatePlannerConfig::default()
            };
            assert_eq!(
                session(config).plan(&observations, target, target, 18.0),
                Err(PlannerAbort::TargetLocallyUnreachable)
            );
        }
    }

    #[test]
    fn arrival_uses_the_certified_seven_and_a_half_centimeter_noise_floor() {
        let observations = calibration(FLAT, 4);
        let target = [1.06, 0.5, 1.0];
        assert!(matches!(
            session(CoordinatePlannerConfig::default())
                .plan(&observations, target, target, 18.0)
                .unwrap(),
            PlannerDecision::Arrived { distance } if distance <= 0.075
        ));
    }

    #[test]
    fn rejects_foreground_input_context_lifecycle_and_root_discontinuity() {
        let target = [3.0, 0.5, 2.0];
        let cases = [
            (|sample: &mut SettledIndicatorObservation| sample.foreground = false)
                as fn(&mut SettledIndicatorObservation),
            |sample| {
                sample
                    .applied_mouse_delta
                    .as_mut()
                    .unwrap()
                    .exclusive_input_ownership = false
            },
            |sample| sample.context_identity += 1,
            |sample| sample.lifecycle_generation += 1,
            |sample| sample.root_generation += 1,
            |sample| sample.settled = false,
        ];
        let expected = [
            PlannerAbort::ForegroundLost,
            PlannerAbort::InputOwnershipLost,
            PlannerAbort::ContextDiscontinuity,
            PlannerAbort::ContextDiscontinuity,
            PlannerAbort::ContextDiscontinuity,
            PlannerAbort::UnsettledSample,
        ];
        for (mutate, expected) in cases.into_iter().zip(expected) {
            let mut observations = calibration(FLAT, 4);
            mutate(&mut observations[2]);
            assert_eq!(
                session(CoordinatePlannerConfig::default()).plan(
                    &observations,
                    target,
                    target,
                    18.0
                ),
                Err(expected)
            );
        }
    }

    #[test]
    fn rejects_non_finite_coordinates_and_live_range_narrower_than_hard_cap() {
        let target = [3.0, 0.5, 2.0];
        let mut observations = calibration(FLAT, 4);
        observations[2].position[0] = f64::NAN;
        assert_eq!(
            session(CoordinatePlannerConfig::default()).plan(&observations, target, target, 18.0),
            Err(PlannerAbort::NonFiniteInput)
        );
        assert_eq!(
            session(CoordinatePlannerConfig::default()).plan(
                &calibration(FLAT, 4),
                [6.1, 0.5, 1.0],
                [1.0, 0.5, 1.0],
                5.0
            ),
            Err(PlannerAbort::TargetOutsidePlayerRange)
        );
    }

    #[test]
    fn exact_integer_receipt_is_mandatory_and_no_confirmation_authority_exists() {
        let target = [4.0, 0.5, 2.0];
        let mut observations = calibration(FLAT, 4);
        let mut planner = session(CoordinatePlannerConfig::default());
        let movement = proposal(planner.plan(&observations, target, target, 18.0).unwrap());
        let _: [i32; 2] = movement.relative_mouse_delta;
        apply(&mut observations, FLAT, movement, 1.0);
        observations
            .last_mut()
            .unwrap()
            .applied_mouse_delta
            .as_mut()
            .unwrap()
            .delta[0] += 1;
        assert_eq!(
            planner.plan(&observations, target, target, 18.0),
            Err(PlannerAbort::CommandReceiptMismatch)
        );
        // Compile-time shape contract: proposal exposes motion only; there is
        // no click or confirmation API to invoke.
    }

    #[test]
    fn quantization_fails_closed_and_post_quantization_bounds_hold() {
        let tiny = [[0.20, 0.0], [0.0, 0.0], [0.0, 0.20]];
        let target = [1.06, 0.5, 1.06];
        assert_eq!(
            session(CoordinatePlannerConfig::default()).plan(
                &calibration(tiny, 4),
                target,
                target,
                18.0
            ),
            Err(PlannerAbort::ResolutionLimited)
        );
        let config = CoordinatePlannerConfig {
            maximum_mouse_step: 2.1,
            maximum_mouse_axis_step: 2,
            ..CoordinatePlannerConfig::default()
        };
        let movement = proposal(
            session(config)
                .plan(
                    &calibration(FLAT, 4),
                    [8.0, 0.5, 5.0],
                    [8.0, 0.5, 5.0],
                    18.0,
                )
                .unwrap(),
        );
        assert!(norm2_i32(movement.relative_mouse_delta) <= 2.1 + RANGE_EPSILON);
        assert!(
            movement
                .relative_mouse_delta
                .into_iter()
                .all(|value| value.abs() <= 2)
        );
    }

    #[test]
    fn normalized_fit_is_scale_invariant_and_low_sensitivity_is_reachable() {
        let target = [3.0, 0.5, 2.0];
        let first = proposal(
            session(CoordinatePlannerConfig::default())
                .plan(&calibration(FLAT, 2), target, target, 18.0)
                .unwrap(),
        );
        let second = proposal(
            session(CoordinatePlannerConfig::default())
                .plan(&calibration(FLAT, 8), target, target, 18.0)
                .unwrap(),
        );
        assert_eq!(first.relative_mouse_delta, second.relative_mouse_delta);
        let low = [[0.015, 0.0], [0.0, 0.0], [0.0, 0.012]];
        assert!(
            session(CoordinatePlannerConfig::default())
                .plan(
                    &calibration(low, 20),
                    [1.3, 0.5, 1.24],
                    [1.3, 0.5, 1.24],
                    18.0
                )
                .is_ok()
        );
    }

    #[test]
    fn bounded_noise_passes_but_large_residual_is_model_inconsistent() {
        let mut noisy = calibration(SLOPED, 5);
        for (index, value) in noisy.iter_mut().enumerate().skip(1) {
            let sign = if index % 2 == 0 { 1.0 } else { -1.0 };
            value.position[0] += sign * 0.012;
            value.position[2] -= sign * 0.009;
        }
        let target = [3.0, 0.8, 2.0];
        assert!(
            session(CoordinatePlannerConfig::default())
                .plan(&noisy, target, target, 18.0)
                .is_ok()
        );
        noisy[4].position[0] += 3.0;
        assert_eq!(
            session(CoordinatePlannerConfig::default()).plan(&noisy, target, target, 18.0),
            Err(PlannerAbort::ModelInconsistent)
        );
    }

    #[test]
    fn poor_ratio_shrinks_trust_then_requires_recalibration() {
        let config = CoordinatePlannerConfig {
            monotonic_regression_tolerance: 1.0,
            ..CoordinatePlannerConfig::default()
        };
        let target = [6.0, 0.5, 2.0];
        let mut observations = calibration(FLAT, 4);
        let mut planner = session(config);
        let first = proposal(planner.plan(&observations, target, target, 18.0).unwrap());
        apply(&mut observations, FLAT, first, 0.05);
        let second = proposal(planner.plan(&observations, target, target, 18.0).unwrap());
        assert!(second.trust_region_radius < config.maximum_mouse_step);
        apply(&mut observations, FLAT, second, 0.05);
        assert_eq!(
            planner.plan(&observations, target, target, 18.0),
            Err(PlannerAbort::RecalibrationRequired)
        );
    }

    #[test]
    fn command_and_cumulative_motion_budgets_fail_closed() {
        let target = [8.0, 0.5, 4.0];
        let mut observations = calibration(FLAT, 4);
        let config = CoordinatePlannerConfig {
            maximum_commands: 1,
            ..CoordinatePlannerConfig::default()
        };
        let mut planner = session(config);
        let first = proposal(planner.plan(&observations, target, target, 18.0).unwrap());
        apply(&mut observations, FLAT, first, 1.0);
        assert_eq!(
            planner.plan(&observations, target, target, 18.0),
            Err(PlannerAbort::CommandBudgetExhausted)
        );
        let config = CoordinatePlannerConfig {
            maximum_cumulative_motion: 1.0,
            ..CoordinatePlannerConfig::default()
        };
        assert_eq!(
            session(config).plan(&calibration(FLAT, 4), target, target, 18.0),
            Err(PlannerAbort::CumulativeMotionBudgetExhausted)
        );
    }

    #[test]
    fn monotonic_guard_aborts_divergence() {
        let target = [5.0, 0.5, 2.0];
        let mut observations = calibration(FLAT, 4);
        let mut planner = session(CoordinatePlannerConfig::default());
        let first = proposal(planner.plan(&observations, target, target, 18.0).unwrap());
        apply(&mut observations, FLAT, first, -1.0);
        assert_eq!(
            planner.plan(&observations, target, target, 18.0),
            Err(PlannerAbort::Diverging)
        );
    }

    #[test]
    fn best_distance_guard_catches_cumulative_regression() {
        let config = CoordinatePlannerConfig {
            monotonic_regression_tolerance: 0.20,
            best_distance_regression_tolerance: 0.10,
            poor_ratio_limit: 100,
            ..CoordinatePlannerConfig::default()
        };
        let target = [5.0, 0.5, 2.0];
        let mut observations = calibration(FLAT, 4);
        let mut planner = session(config);
        let movement = proposal(planner.plan(&observations, target, target, 18.0).unwrap());
        let current = observations.last().unwrap().position;
        let away = subtract3(current, target);
        let scale = 0.12 / norm3(away);
        observations.push(sample(
            [
                current[0] + away[0] * scale,
                current[1] + away[1] * scale,
                current[2] + away[2] * scale,
            ],
            Some(CertifiedMouseDelta {
                planner_command_id: Some(movement.command_id),
                delta: movement.relative_mouse_delta,
                exclusive_input_ownership: true,
            }),
        ));
        assert_eq!(
            planner.plan(&observations, target, target, 18.0),
            Err(PlannerAbort::Diverging)
        );
    }

    #[test]
    fn cycle_and_repeated_no_improvement_abort_separately() {
        let target = [5.0, 0.5, 2.0];
        let mut observations = calibration(FLAT, 4);
        let mut cycle_planner = session(CoordinatePlannerConfig::default());
        let movement = proposal(
            cycle_planner
                .plan(&observations, target, target, 18.0)
                .unwrap(),
        );
        apply(&mut observations, FLAT, movement, 0.0);
        assert_eq!(
            cycle_planner.plan(&observations, target, target, 18.0),
            Err(PlannerAbort::CycleDetected)
        );

        let config = CoordinatePlannerConfig {
            poor_ratio_limit: 100,
            no_improvement_limit: 2,
            cycle_position_tolerance: 1.0e-9,
            ..CoordinatePlannerConfig::default()
        };
        let mut observations = calibration(FLAT, 4);
        let mut no_progress_planner = session(config);
        for attempt in 0..2 {
            let movement = proposal(
                no_progress_planner
                    .plan(&observations, target, target, 18.0)
                    .unwrap(),
            );
            apply(&mut observations, FLAT, movement, 0.001);
            if attempt == 1 {
                assert_eq!(
                    no_progress_planner.plan(&observations, target, target, 18.0),
                    Err(PlannerAbort::NoImprovement)
                );
            }
        }
    }
}
