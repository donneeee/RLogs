//! Pure coordinate-convergence planning for a future automarker canary.
//!
//! This module deliberately has no process, input, packet, window, or timing
//! access. It consumes observations supplied by a caller and returns at most a
//! relative mouse-motion proposal. Applying that proposal is outside this
//! module's authority.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IndicatorObservation {
    /// The indicator position observed after `applied_mouse_delta`.
    pub position: [f64; 3],
    /// Relative mouse motion applied after the preceding observation.
    /// The first observation must use `None`; later observations use `Some`.
    pub applied_mouse_delta: Option<[f64; 2]>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CoordinatePlannerConfig {
    pub damping: f64,
    pub maximum_target_distance: f64,
    pub maximum_mouse_step: f64,
    pub maximum_mouse_axis_step: f64,
    pub minimum_excitation_energy: f64,
    pub maximum_condition_number: f64,
    pub minimum_predicted_improvement: f64,
    pub maximum_unreachable_fraction: f64,
    pub absolute_model_tolerance: f64,
    pub relative_model_tolerance: f64,
    pub no_improvement_patience: usize,
    pub minimum_observed_improvement: f64,
    pub arrival_tolerance: f64,
    pub maximum_transitions: usize,
}

impl Default for CoordinatePlannerConfig {
    fn default() -> Self {
        Self {
            damping: 1.0e-4,
            maximum_target_distance: 100.0,
            maximum_mouse_step: 12.0,
            maximum_mouse_axis_step: 10.0,
            minimum_excitation_energy: 1.0e-6,
            maximum_condition_number: 10_000.0,
            minimum_predicted_improvement: 1.0e-5,
            maximum_unreachable_fraction: 0.98,
            absolute_model_tolerance: 0.04,
            relative_model_tolerance: 0.30,
            no_improvement_patience: 4,
            minimum_observed_improvement: 1.0e-3,
            arrival_tolerance: 1.0e-3,
            maximum_transitions: 16,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlannerAbort {
    InvalidConfiguration,
    NonFiniteInput,
    InsufficientObservations,
    InsufficientExcitation,
    IllConditionedGeometry,
    TargetOutOfRange,
    TargetLocallyUnreachable,
    ModelInconsistent,
    UserInterference,
    NoImprovement,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PlannerDecision {
    Arrived { distance: f64 },
    Move(CoordinateMoveProposal),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CoordinateMoveProposal {
    pub relative_mouse_delta: [f64; 2],
    pub current_distance: f64,
    pub predicted_distance: f64,
    pub condition_number: f64,
    pub trust_region_clamped: bool,
}

/// Estimates a local 3x2 indicator-position Jacobian and proposes one bounded
/// relative mouse move. The function has no side effects and fails closed when
/// its local model is not trustworthy.
pub fn plan_coordinate_move(
    observations: &[IndicatorObservation],
    target: [f64; 3],
    config: CoordinatePlannerConfig,
) -> Result<PlannerDecision, PlannerAbort> {
    validate_config(config)?;
    if !finite3(target)
        || observations.iter().any(|sample| {
            !finite3(sample.position)
                || sample
                    .applied_mouse_delta
                    .is_some_and(|delta| !finite2(delta))
        })
    {
        return Err(PlannerAbort::NonFiniteInput);
    }
    if observations.len() < 3 || observations[0].applied_mouse_delta.is_some() {
        return Err(PlannerAbort::InsufficientObservations);
    }
    if observations[1..]
        .iter()
        .any(|sample| sample.applied_mouse_delta.is_none())
    {
        return Err(PlannerAbort::InsufficientObservations);
    }

    let current = observations.last().expect("length checked").position;
    let error = subtract3(target, current);
    let current_distance = norm3(error);
    if current_distance <= config.arrival_tolerance {
        return Ok(PlannerDecision::Arrived {
            distance: current_distance,
        });
    }
    if current_distance > config.maximum_target_distance {
        return Err(PlannerAbort::TargetOutOfRange);
    }

    detect_no_improvement(observations, target, config)?;

    let start = observations
        .len()
        .saturating_sub(config.maximum_transitions + 1);
    let window = &observations[start..];
    detect_latest_interference(window, config)?;
    let model = fit_jacobian(window, config)?;

    let mut mouse_delta = solve_damped(model.jacobian, error, config.damping)
        .ok_or(PlannerAbort::IllConditionedGeometry)?;
    let unconstrained = mouse_delta;
    for value in &mut mouse_delta {
        *value = value.clamp(
            -config.maximum_mouse_axis_step,
            config.maximum_mouse_axis_step,
        );
    }
    let length = norm2(mouse_delta);
    if length > config.maximum_mouse_step {
        let scale = config.maximum_mouse_step / length;
        mouse_delta[0] *= scale;
        mouse_delta[1] *= scale;
    }
    let trust_region_clamped = squared_distance2(mouse_delta, unconstrained) > 1.0e-18;

    let predicted_change = multiply_jacobian(model.jacobian, mouse_delta);
    let predicted_distance = norm3(subtract3(error, predicted_change));
    let full_change = multiply_jacobian(model.jacobian, unconstrained);
    let full_residual = norm3(subtract3(error, full_change));
    if full_residual / current_distance > config.maximum_unreachable_fraction {
        return Err(PlannerAbort::TargetLocallyUnreachable);
    }
    if !predicted_distance.is_finite()
        || current_distance - predicted_distance < config.minimum_predicted_improvement
    {
        return Err(PlannerAbort::TargetLocallyUnreachable);
    }

    Ok(PlannerDecision::Move(CoordinateMoveProposal {
        relative_mouse_delta: mouse_delta,
        current_distance,
        predicted_distance,
        condition_number: model.condition_number,
        trust_region_clamped,
    }))
}

#[derive(Clone, Copy)]
struct LocalModel {
    jacobian: [[f64; 2]; 3],
    condition_number: f64,
}

fn fit_jacobian(
    observations: &[IndicatorObservation],
    config: CoordinatePlannerConfig,
) -> Result<LocalModel, PlannerAbort> {
    let mut gram = [[0.0; 2]; 2];
    let mut cross = [[0.0; 2]; 3];
    let mut observed_energy = 0.0;
    for pair in observations.windows(2) {
        let delta = pair[1]
            .applied_mouse_delta
            .ok_or(PlannerAbort::InsufficientObservations)?;
        let change = subtract3(pair[1].position, pair[0].position);
        gram[0][0] += delta[0] * delta[0];
        gram[0][1] += delta[0] * delta[1];
        gram[1][0] += delta[1] * delta[0];
        gram[1][1] += delta[1] * delta[1];
        observed_energy += dot3(change, change);
        for axis in 0..3 {
            cross[axis][0] += change[axis] * delta[0];
            cross[axis][1] += change[axis] * delta[1];
        }
    }
    let (minimum_eigenvalue, maximum_eigenvalue) = symmetric_eigenvalues(gram);
    if minimum_eigenvalue < config.minimum_excitation_energy || observed_energy <= f64::EPSILON {
        return Err(PlannerAbort::InsufficientExcitation);
    }
    let condition_number = maximum_eigenvalue / minimum_eigenvalue;
    if !condition_number.is_finite() || condition_number > config.maximum_condition_number {
        return Err(PlannerAbort::IllConditionedGeometry);
    }

    let regularized = [
        [gram[0][0] + config.damping, gram[0][1]],
        [gram[1][0], gram[1][1] + config.damping],
    ];
    let inverse = inverse2(regularized).ok_or(PlannerAbort::IllConditionedGeometry)?;
    let mut jacobian = [[0.0; 2]; 3];
    for axis in 0..3 {
        jacobian[axis][0] = cross[axis][0] * inverse[0][0] + cross[axis][1] * inverse[1][0];
        jacobian[axis][1] = cross[axis][0] * inverse[0][1] + cross[axis][1] * inverse[1][1];
    }
    let output_gram = [
        [
            jacobian.iter().map(|row| row[0] * row[0]).sum(),
            jacobian.iter().map(|row| row[0] * row[1]).sum(),
        ],
        [
            jacobian.iter().map(|row| row[1] * row[0]).sum(),
            jacobian.iter().map(|row| row[1] * row[1]).sum(),
        ],
    ];
    let (output_minimum, output_maximum) = symmetric_eigenvalues(output_gram);
    if output_minimum <= f64::EPSILON {
        return Err(PlannerAbort::IllConditionedGeometry);
    }
    let output_condition = output_maximum / output_minimum;
    if !output_condition.is_finite() || output_condition > config.maximum_condition_number {
        return Err(PlannerAbort::IllConditionedGeometry);
    }

    let mut residual_energy = 0.0;
    let transitions = observations.len() - 1;
    for pair in observations.windows(2) {
        let delta = pair[1].applied_mouse_delta.expect("validated above");
        let actual = subtract3(pair[1].position, pair[0].position);
        let residual = subtract3(actual, multiply_jacobian(jacobian, delta));
        residual_energy += dot3(residual, residual);
    }
    let residual_rms = (residual_energy / transitions as f64).sqrt();
    let motion_rms = (observed_energy / transitions as f64).sqrt();
    if residual_rms > config.absolute_model_tolerance + config.relative_model_tolerance * motion_rms
    {
        return Err(PlannerAbort::ModelInconsistent);
    }

    Ok(LocalModel {
        jacobian,
        condition_number: condition_number.max(output_condition),
    })
}

fn detect_latest_interference(
    observations: &[IndicatorObservation],
    config: CoordinatePlannerConfig,
) -> Result<(), PlannerAbort> {
    if observations.len() < 5 {
        return Ok(());
    }
    let prior = fit_jacobian(&observations[..observations.len() - 1], config)?;
    let pair = &observations[observations.len() - 2..];
    let input = pair[1]
        .applied_mouse_delta
        .ok_or(PlannerAbort::InsufficientObservations)?;
    let actual = subtract3(pair[1].position, pair[0].position);
    let predicted = multiply_jacobian(prior.jacobian, input);
    let residual = norm3(subtract3(actual, predicted));
    let tolerance = config.absolute_model_tolerance
        + config.relative_model_tolerance * norm3(predicted).max(config.absolute_model_tolerance);
    if residual > tolerance {
        return Err(PlannerAbort::UserInterference);
    }
    Ok(())
}

fn detect_no_improvement(
    observations: &[IndicatorObservation],
    target: [f64; 3],
    config: CoordinatePlannerConfig,
) -> Result<(), PlannerAbort> {
    let patience = config.no_improvement_patience;
    if patience == 0 || observations.len() <= patience {
        return Ok(());
    }
    let recent = &observations[observations.len() - patience - 1..];
    let starting_distance = norm3(subtract3(target, recent[0].position));
    let best_later_distance = recent[1..]
        .iter()
        .map(|sample| norm3(subtract3(target, sample.position)))
        .fold(f64::INFINITY, f64::min);
    if starting_distance - best_later_distance < config.minimum_observed_improvement {
        return Err(PlannerAbort::NoImprovement);
    }
    Ok(())
}

fn solve_damped(jacobian: [[f64; 2]; 3], error: [f64; 3], damping: f64) -> Option<[f64; 2]> {
    let mut normal = [[0.0; 2]; 2];
    let mut rhs = [0.0; 2];
    for axis in 0..3 {
        normal[0][0] += jacobian[axis][0] * jacobian[axis][0];
        normal[0][1] += jacobian[axis][0] * jacobian[axis][1];
        normal[1][0] += jacobian[axis][1] * jacobian[axis][0];
        normal[1][1] += jacobian[axis][1] * jacobian[axis][1];
        rhs[0] += jacobian[axis][0] * error[axis];
        rhs[1] += jacobian[axis][1] * error[axis];
    }
    normal[0][0] += damping;
    normal[1][1] += damping;
    let inverse = inverse2(normal)?;
    Some([
        inverse[0][0] * rhs[0] + inverse[0][1] * rhs[1],
        inverse[1][0] * rhs[0] + inverse[1][1] * rhs[1],
    ])
}

fn validate_config(config: CoordinatePlannerConfig) -> Result<(), PlannerAbort> {
    let finite_positive = [
        config.damping,
        config.maximum_target_distance,
        config.maximum_mouse_step,
        config.maximum_mouse_axis_step,
        config.minimum_excitation_energy,
        config.maximum_condition_number,
        config.minimum_predicted_improvement,
        config.absolute_model_tolerance,
        config.minimum_observed_improvement,
        config.arrival_tolerance,
    ]
    .into_iter()
    .all(|value| value.is_finite() && value > 0.0);
    if !finite_positive
        || !config.relative_model_tolerance.is_finite()
        || config.relative_model_tolerance < 0.0
        || !config.maximum_unreachable_fraction.is_finite()
        || !(0.0..1.0).contains(&config.maximum_unreachable_fraction)
        || config.maximum_transitions < 2
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

fn norm3(value: [f64; 3]) -> f64 {
    dot3(value, value).sqrt()
}

fn squared_distance2(left: [f64; 2], right: [f64; 2]) -> f64 {
    (left[0] - right[0]).powi(2) + (left[1] - right[1]).powi(2)
}

fn finite2(value: [f64; 2]) -> bool {
    value.into_iter().all(f64::is_finite)
}

fn finite3(value: [f64; 3]) -> bool {
    value.into_iter().all(f64::is_finite)
}

#[cfg(test)]
mod tests {
    use super::*;

    const JACOBIAN: [[f64; 2]; 3] = [[0.8, 0.1], [0.0, 0.0], [-0.2, 0.6]];

    fn calibration(jacobian: [[f64; 2]; 3]) -> Vec<IndicatorObservation> {
        let inputs = [[1.0, 0.0], [0.0, 1.0], [-0.75, 0.4], [0.35, -0.8]];
        let mut position = [0.0; 3];
        let mut observations = vec![IndicatorObservation {
            position,
            applied_mouse_delta: None,
        }];
        for input in inputs {
            let change = multiply_jacobian(jacobian, input);
            for axis in 0..3 {
                position[axis] += change[axis];
            }
            observations.push(IndicatorObservation {
                position,
                applied_mouse_delta: Some(input),
            });
        }
        observations
    }

    fn permissive_no_progress() -> CoordinatePlannerConfig {
        CoordinatePlannerConfig {
            no_improvement_patience: 100,
            ..CoordinatePlannerConfig::default()
        }
    }

    #[test]
    fn converges_deterministically_on_a_reachable_saved_position() {
        let target = [5.0, 0.0, -3.0];
        let config = CoordinatePlannerConfig {
            maximum_mouse_step: 2.0,
            maximum_mouse_axis_step: 2.0,
            no_improvement_patience: 100,
            ..CoordinatePlannerConfig::default()
        };
        let mut observations = calibration(JACOBIAN);
        for _ in 0..12 {
            match plan_coordinate_move(&observations, target, config).unwrap() {
                PlannerDecision::Arrived { .. } => break,
                PlannerDecision::Move(proposal) => {
                    let mut position = observations.last().unwrap().position;
                    let change = multiply_jacobian(JACOBIAN, proposal.relative_mouse_delta);
                    for axis in 0..3 {
                        position[axis] += change[axis];
                    }
                    observations.push(IndicatorObservation {
                        position,
                        applied_mouse_delta: Some(proposal.relative_mouse_delta),
                    });
                }
            }
        }
        assert!(norm3(subtract3(target, observations.last().unwrap().position)) < 0.002);
    }

    #[test]
    fn rejects_out_of_range_and_locally_unreachable_targets() {
        let observations = calibration(JACOBIAN);
        let config = permissive_no_progress();
        assert_eq!(
            plan_coordinate_move(&observations, [200.0, 0.0, 0.0], config),
            Err(PlannerAbort::TargetOutOfRange)
        );
        assert_eq!(
            plan_coordinate_move(&observations, [0.0, 5.0, 0.0], config),
            Err(PlannerAbort::TargetLocallyUnreachable)
        );
    }

    #[test]
    fn supports_vertical_motion_but_rejects_degenerate_geometry() {
        let vertical = [[0.7, 0.0], [0.0, 0.5], [0.0, 0.0]];
        let decision = plan_coordinate_move(
            &calibration(vertical),
            [2.0, 3.0, 0.0],
            permissive_no_progress(),
        )
        .unwrap();
        assert!(matches!(decision, PlannerDecision::Move(_)));

        let degenerate = [[1.0, 2.0], [0.0, 0.0], [0.0, 0.0]];
        assert_eq!(
            plan_coordinate_move(
                &calibration(degenerate),
                [2.0, 0.0, 0.0],
                permissive_no_progress(),
            ),
            Err(PlannerAbort::IllConditionedGeometry)
        );
    }

    #[test]
    fn tolerates_bounded_measurement_noise() {
        let mut observations = calibration(JACOBIAN);
        for (index, sample) in observations.iter_mut().enumerate().skip(1) {
            let sign = if index % 2 == 0 { 1.0 } else { -1.0 };
            sample.position[0] += sign * 0.004;
            sample.position[2] -= sign * 0.003;
        }
        let proposal =
            match plan_coordinate_move(&observations, [3.0, 0.0, 2.0], permissive_no_progress())
                .unwrap()
            {
                PlannerDecision::Move(proposal) => proposal,
                PlannerDecision::Arrived { .. } => panic!("fixture should require movement"),
            };
        assert!(proposal.predicted_distance < proposal.current_distance);
    }

    #[test]
    fn clamps_every_proposal_to_the_trust_region() {
        let config = CoordinatePlannerConfig {
            maximum_target_distance: 1_000.0,
            maximum_mouse_step: 1.0,
            maximum_mouse_axis_step: 0.8,
            no_improvement_patience: 100,
            ..CoordinatePlannerConfig::default()
        };
        let proposal = match plan_coordinate_move(&calibration(JACOBIAN), [50.0, 0.0, 40.0], config)
            .unwrap()
        {
            PlannerDecision::Move(proposal) => proposal,
            PlannerDecision::Arrived { .. } => panic!("fixture should require movement"),
        };
        assert!(proposal.trust_region_clamped);
        assert!(norm2(proposal.relative_mouse_delta) <= 1.0 + 1.0e-12);
        assert!(
            proposal
                .relative_mouse_delta
                .into_iter()
                .all(|value| value.abs() <= 0.8)
        );
    }

    #[test]
    fn aborts_on_non_finite_input_user_interference_and_no_improvement() {
        let mut non_finite = calibration(JACOBIAN);
        non_finite[2].position[0] = f64::NAN;
        assert_eq!(
            plan_coordinate_move(&non_finite, [1.0, 0.0, 1.0], permissive_no_progress()),
            Err(PlannerAbort::NonFiniteInput)
        );

        let mut interference = calibration(JACOBIAN);
        let last = interference.last_mut().unwrap();
        last.position[0] += 3.0;
        last.position[2] -= 2.0;
        assert_eq!(
            plan_coordinate_move(&interference, [5.0, 0.0, 5.0], permissive_no_progress()),
            Err(PlannerAbort::UserInterference)
        );

        let stationary = vec![
            IndicatorObservation {
                position: [0.0, 0.0, 0.0],
                applied_mouse_delta: None,
            },
            IndicatorObservation {
                position: [1.0, 0.0, 0.0],
                applied_mouse_delta: Some([1.0, 0.0]),
            },
            IndicatorObservation {
                position: [1.0, 0.0, 1.0],
                applied_mouse_delta: Some([0.0, 1.0]),
            },
            IndicatorObservation {
                position: [1.0, 0.0, 1.0],
                applied_mouse_delta: Some([0.2, 0.0]),
            },
            IndicatorObservation {
                position: [1.0, 0.0, 1.0],
                applied_mouse_delta: Some([0.0, 0.2]),
            },
        ];
        let config = CoordinatePlannerConfig {
            no_improvement_patience: 2,
            ..CoordinatePlannerConfig::default()
        };
        assert_eq!(
            plan_coordinate_move(&stationary, [4.0, 0.0, 4.0], config),
            Err(PlannerAbort::NoImprovement)
        );
    }
}
