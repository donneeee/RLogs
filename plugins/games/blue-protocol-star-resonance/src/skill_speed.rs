//! Exact, allocation-free helpers for the current-client skill-stage speed formula.
//!
//! These helpers reproduce a client calculation boundary. They do not decide
//! whether a speed change creates external rDPS, infer completed actions from
//! damage hits, or authorize a protocol route. Callers must use a loadout and
//! attribute snapshot captured for the action being evaluated.

use std::num::NonZeroI128;

use crate::state_formula::BPSR_FIXED_POINT_SCALE;

/// Skill-stage family selected by the current client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillStageSpeedFamily {
    Normal,
    Singing,
    Charge,
    Guide,
    Unaffected,
}

/// One reduced, positive rational speed multiplier.
///
/// A value of `15_000 / 10_000` means the stage runs at 1.5 times its base
/// speed. Keeping the ratio exact avoids per-event floating-point drift in
/// offline attribution and replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExactSkillSpeedRatio {
    numerator: i128,
    denominator: NonZeroI128,
}

impl ExactSkillSpeedRatio {
    pub const ONE: Self = Self {
        numerator: 1,
        denominator: NonZeroI128::new(1).expect("one is non-zero"),
    };

    pub fn new(numerator: i128, denominator: i128) -> Option<Self> {
        if numerator <= 0 || denominator <= 0 {
            return None;
        }
        let divisor = greatest_common_divisor(numerator, denominator);
        Some(Self {
            numerator: numerator.checked_div(divisor)?,
            denominator: NonZeroI128::new(denominator.checked_div(divisor)?)?,
        })
    }

    pub const fn numerator(self) -> i128 {
        self.numerator
    }

    pub const fn denominator(self) -> i128 {
        self.denominator.get()
    }
}

/// Exact elapsed duration, expressed in the caller's base-duration unit.
///
/// If a stage has base duration `D` and speed `N / Q`, its elapsed duration is
/// exactly `D * Q / N`. The rational is reduced so it can be cached and
/// compared without floating-point drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExactSkillStageDuration {
    numerator: i128,
    denominator: NonZeroI128,
}

impl ExactSkillStageDuration {
    fn from_base_duration_and_speed(
        base_duration_units: i64,
        speed: ExactSkillSpeedRatio,
    ) -> Option<Self> {
        if base_duration_units <= 0 {
            return None;
        }
        let numerator = i128::from(base_duration_units).checked_mul(speed.denominator())?;
        let denominator = speed.numerator();
        let divisor = greatest_common_divisor(numerator, denominator);
        Some(Self {
            numerator: numerator.checked_div(divisor)?,
            denominator: NonZeroI128::new(denominator.checked_div(divisor)?)?,
        })
    }

    pub const fn numerator(self) -> i128 {
        self.numerator
    }

    pub const fn denominator(self) -> i128 {
        self.denominator.get()
    }
}

/// Exact signed duration difference in the caller's base-duration unit.
///
/// For [`ExactSkillStageTimingCounterfactual::without_provider_minus_observed`],
/// a positive value is time saved by the removed provider contribution, zero
/// means the stage is unaffected, and a negative value means the contribution
/// slowed the stage. This is schedule evidence, not damage or rDPS credit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExactSkillStageDurationDelta {
    numerator: i128,
    denominator: NonZeroI128,
}

impl ExactSkillStageDurationDelta {
    fn between(left: ExactSkillStageDuration, right: ExactSkillStageDuration) -> Option<Self> {
        let numerator = left
            .numerator()
            .checked_mul(right.denominator())?
            .checked_sub(right.numerator().checked_mul(left.denominator())?)?;
        let denominator = left.denominator().checked_mul(right.denominator())?;
        let divisor = greatest_common_divisor(numerator.checked_abs()?, denominator);
        Some(Self {
            numerator: numerator.checked_div(divisor)?,
            denominator: NonZeroI128::new(denominator.checked_div(divisor)?)?,
        })
    }

    pub const fn numerator(self) -> i128 {
        self.numerator
    }

    pub const fn denominator(self) -> i128 {
        self.denominator.get()
    }

    pub const fn is_positive(self) -> bool {
        self.numerator > 0
    }
}

/// Exact observed-versus-provider-removed timing for one skill stage.
///
/// The caller must supply two independently replayed client speed ratios: the
/// packet-observed action snapshot and the same snapshot with one proven
/// provider component removed. This helper never guesses an attribute
/// conversion or converts saved time into damage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExactSkillStageTimingCounterfactual {
    pub observed_duration: ExactSkillStageDuration,
    pub without_provider_duration: ExactSkillStageDuration,
    pub without_provider_minus_observed: ExactSkillStageDurationDelta,
}

/// Computes an exact, allocation-free timing counterfactual for one stage.
///
/// `base_duration_units` may use any integral unit used by the decoded stage
/// table (for example milliseconds or fixed client ticks), provided both
/// counterfactuals use the same unit.
pub fn exact_skill_stage_timing_counterfactual(
    base_duration_units: i64,
    observed_speed: ExactSkillSpeedRatio,
    without_provider_speed: ExactSkillSpeedRatio,
) -> Option<ExactSkillStageTimingCounterfactual> {
    let observed_duration =
        ExactSkillStageDuration::from_base_duration_and_speed(base_duration_units, observed_speed)?;
    let without_provider_duration = ExactSkillStageDuration::from_base_duration_and_speed(
        base_duration_units,
        without_provider_speed,
    )?;
    let without_provider_minus_observed =
        ExactSkillStageDurationDelta::between(without_provider_duration, observed_duration)?;

    Some(ExactSkillStageTimingCounterfactual {
        observed_duration,
        without_provider_duration,
        without_provider_minus_observed,
    })
}

/// Returns the exact conserved share of one observed, action-linked damage
/// body supplied by an external speed provider.
///
/// If the packet-observed action speed is `S` and replaying that same action
/// with one proven provider component removed gives `W`, the provider owns the
/// throughput share `(S - W) / S`. Multiplying that reduced ratio by the
/// damage already linked to the action preserves the observed damage total and
/// avoids manufacturing an unobserved extra action.
///
/// This function deliberately does not establish action ownership. Callers
/// must first prove the exact action snapshot, stage family, provider delta,
/// and action-to-damage recount parent. Equal, slower, or invalid
/// counterfactuals fail closed.
pub fn exact_external_speed_capacity_fraction(
    observed_linked_damage: i64,
    observed_speed: ExactSkillSpeedRatio,
    without_provider_speed: ExactSkillSpeedRatio,
) -> Option<(i128, i128)> {
    if observed_linked_damage <= 0 {
        return None;
    }

    let observed_cross = observed_speed
        .numerator()
        .checked_mul(without_provider_speed.denominator())?;
    let without_provider_cross = without_provider_speed
        .numerator()
        .checked_mul(observed_speed.denominator())?;
    let speed_marginal = observed_cross.checked_sub(without_provider_cross)?;
    if speed_marginal <= 0 {
        return None;
    }

    let numerator = i128::from(observed_linked_damage).checked_mul(speed_marginal)?;
    let denominator = observed_cross;
    let divisor = greatest_common_divisor(numerator, denominator);
    Some((
        numerator.checked_div(divisor)?,
        denominator.checked_div(divisor)?,
    ))
}

/// Packet-time inputs consumed by the current-client stage-speed boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkillStageSpeedInputs {
    /// Entity type `8` selects the pet-specific speed family before stage type.
    pub is_pet: bool,
    /// SkillTable.AtkSpeedSwitch for a normal stage.
    pub attack_speed_enabled: bool,
    /// Attribute 11720.
    pub attack_speed_basis_points: i64,
    /// Skill-scoped temporary attribute effect type 700.
    pub temporary_attack_speed_basis_points: i64,
    /// Attribute 11730.
    pub cast_speed_basis_points: i64,
    /// Skill-scoped temporary attribute effect type 710.
    pub temporary_cast_speed_basis_points: i64,
    /// Attribute 11740.
    pub charge_speed_basis_points: i64,
    /// Attribute 11990.
    pub pet_attack_speed_basis_points: i64,
}

impl Default for SkillStageSpeedInputs {
    fn default() -> Self {
        Self {
            is_pet: false,
            attack_speed_enabled: false,
            attack_speed_basis_points: 0,
            temporary_attack_speed_basis_points: 0,
            cast_speed_basis_points: 0,
            temporary_cast_speed_basis_points: 0,
            charge_speed_basis_points: 0,
            pet_attack_speed_basis_points: 0,
        }
    }
}

/// Replays the current-client speed multiplier for a non-singing skill stage.
///
/// Singing stages additionally require their total and first-stage durations;
/// use [`singing_stage_speed`] for those.
pub fn skill_stage_speed(
    family: SkillStageSpeedFamily,
    inputs: SkillStageSpeedInputs,
) -> Option<ExactSkillSpeedRatio> {
    if inputs.is_pet {
        return additive_basis_point_speed(inputs.pet_attack_speed_basis_points, 0);
    }

    match family {
        SkillStageSpeedFamily::Normal if inputs.attack_speed_enabled => additive_basis_point_speed(
            inputs.attack_speed_basis_points,
            inputs.temporary_attack_speed_basis_points,
        ),
        SkillStageSpeedFamily::Normal | SkillStageSpeedFamily::Unaffected => {
            Some(ExactSkillSpeedRatio::ONE)
        }
        SkillStageSpeedFamily::Charge => {
            additive_basis_point_speed(inputs.charge_speed_basis_points, 0)
        }
        SkillStageSpeedFamily::Guide => additive_basis_point_speed(
            inputs.cast_speed_basis_points,
            inputs.temporary_cast_speed_basis_points,
        ),
        SkillStageSpeedFamily::Singing => None,
    }
}

/// Replays the current-client singing-stage speed boundary.
///
/// The client first forms `reduce = 1 + cast_speed / 10000 + temp / 10000`,
/// then compares `total_duration / reduce` with `first_duration`. If the
/// adjusted duration is not shorter, the stage stays at 1x. Otherwise the
/// multiplier is `first_duration * reduce / total_duration`.
pub fn singing_stage_speed(
    inputs: SkillStageSpeedInputs,
    total_duration: i64,
    first_duration: i64,
) -> Option<ExactSkillSpeedRatio> {
    if inputs.is_pet {
        return additive_basis_point_speed(inputs.pet_attack_speed_basis_points, 0);
    }
    if total_duration < 0 || first_duration < 0 {
        return None;
    }
    if total_duration == 0 || first_duration == 0 {
        return Some(ExactSkillSpeedRatio::ONE);
    }

    let scale = i128::from(BPSR_FIXED_POINT_SCALE);
    let reduction_numerator = additive_speed_numerator(
        inputs.cast_speed_basis_points,
        inputs.temporary_cast_speed_basis_points,
    )?;
    let scaled_total_duration = i128::from(total_duration).checked_mul(scale)?;
    let scaled_first_duration = i128::from(first_duration).checked_mul(reduction_numerator)?;

    if scaled_total_duration >= scaled_first_duration {
        Some(ExactSkillSpeedRatio::ONE)
    } else {
        ExactSkillSpeedRatio::new(scaled_first_duration, scaled_total_duration)
    }
}

fn additive_basis_point_speed(
    primary_basis_points: i64,
    temporary_basis_points: i64,
) -> Option<ExactSkillSpeedRatio> {
    ExactSkillSpeedRatio::new(
        additive_speed_numerator(primary_basis_points, temporary_basis_points)?,
        i128::from(BPSR_FIXED_POINT_SCALE),
    )
}

fn additive_speed_numerator(
    primary_basis_points: i64,
    temporary_basis_points: i64,
) -> Option<i128> {
    let value = i128::from(BPSR_FIXED_POINT_SCALE)
        .checked_add(i128::from(primary_basis_points))?
        .checked_add(i128::from(temporary_basis_points))?;
    (value > 0).then_some(value)
}

const fn greatest_common_divisor(mut left: i128, mut right: i128) -> i128 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ratio(numerator: i128, denominator: i128) -> ExactSkillSpeedRatio {
        ExactSkillSpeedRatio::new(numerator, denominator).unwrap()
    }

    #[test]
    fn normal_speed_requires_the_skill_attack_speed_switch() {
        let inputs = SkillStageSpeedInputs {
            attack_speed_basis_points: 382,
            temporary_attack_speed_basis_points: 230,
            ..Default::default()
        };
        assert_eq!(
            skill_stage_speed(SkillStageSpeedFamily::Normal, inputs),
            Some(ExactSkillSpeedRatio::ONE)
        );
        assert_eq!(
            skill_stage_speed(
                SkillStageSpeedFamily::Normal,
                SkillStageSpeedInputs {
                    attack_speed_enabled: true,
                    ..inputs
                }
            ),
            Some(ratio(10_612, 10_000))
        );
    }

    #[test]
    fn stage_families_select_only_their_native_attributes() {
        let inputs = SkillStageSpeedInputs {
            attack_speed_enabled: true,
            attack_speed_basis_points: 100,
            temporary_attack_speed_basis_points: 20,
            cast_speed_basis_points: 200,
            temporary_cast_speed_basis_points: 30,
            charge_speed_basis_points: 300,
            pet_attack_speed_basis_points: 400,
            ..Default::default()
        };
        assert_eq!(
            skill_stage_speed(SkillStageSpeedFamily::Normal, inputs),
            Some(ratio(10_120, 10_000))
        );
        assert_eq!(
            skill_stage_speed(SkillStageSpeedFamily::Guide, inputs),
            Some(ratio(10_230, 10_000))
        );
        assert_eq!(
            skill_stage_speed(SkillStageSpeedFamily::Charge, inputs),
            Some(ratio(10_300, 10_000))
        );
        assert_eq!(
            skill_stage_speed(
                SkillStageSpeedFamily::Normal,
                SkillStageSpeedInputs {
                    is_pet: true,
                    ..inputs
                }
            ),
            Some(ratio(10_400, 10_000))
        );
    }

    #[test]
    fn singing_speed_preserves_the_current_client_boundary() {
        let inputs = SkillStageSpeedInputs {
            cast_speed_basis_points: 2_000,
            temporary_cast_speed_basis_points: 500,
            ..Default::default()
        };
        assert_eq!(
            singing_stage_speed(inputs, 1_500, 1_000),
            Some(ExactSkillSpeedRatio::ONE)
        );
        assert_eq!(
            singing_stage_speed(inputs, 1_000, 1_000),
            Some(ratio(12_500, 10_000))
        );
    }

    #[test]
    fn invalid_or_incomplete_inputs_fail_closed() {
        assert_eq!(
            skill_stage_speed(
                SkillStageSpeedFamily::Charge,
                SkillStageSpeedInputs {
                    charge_speed_basis_points: -10_000,
                    ..Default::default()
                }
            ),
            None
        );
        assert_eq!(
            skill_stage_speed(
                SkillStageSpeedFamily::Singing,
                SkillStageSpeedInputs::default()
            ),
            None
        );
        assert_eq!(
            singing_stage_speed(SkillStageSpeedInputs::default(), -1, 100),
            None
        );
    }

    #[test]
    fn exact_timing_counterfactual_preserves_a_230_basis_point_provider_delta() {
        let observed = skill_stage_speed(
            SkillStageSpeedFamily::Normal,
            SkillStageSpeedInputs {
                attack_speed_enabled: true,
                attack_speed_basis_points: 612,
                ..Default::default()
            },
        )
        .unwrap();
        let without_provider = skill_stage_speed(
            SkillStageSpeedFamily::Normal,
            SkillStageSpeedInputs {
                attack_speed_enabled: true,
                attack_speed_basis_points: 382,
                ..Default::default()
            },
        )
        .unwrap();
        let comparison =
            exact_skill_stage_timing_counterfactual(10_000, observed, without_provider).unwrap();

        assert_eq!(comparison.observed_duration.numerator(), 25_000_000);
        assert_eq!(comparison.observed_duration.denominator(), 2_653);
        assert_eq!(comparison.without_provider_duration.numerator(), 50_000_000);
        assert_eq!(comparison.without_provider_duration.denominator(), 5_191);
        assert_eq!(
            comparison.without_provider_minus_observed.numerator(),
            2_875_000_000
        );
        assert_eq!(
            comparison.without_provider_minus_observed.denominator(),
            13_771_723
        );
        assert!(comparison.without_provider_minus_observed.is_positive());
    }

    #[test]
    fn exact_timing_counterfactual_preserves_a_382_basis_point_cast_delta() {
        let observed = skill_stage_speed(
            SkillStageSpeedFamily::Guide,
            SkillStageSpeedInputs {
                cast_speed_basis_points: 382,
                ..Default::default()
            },
        )
        .unwrap();
        let comparison =
            exact_skill_stage_timing_counterfactual(10_000, observed, ExactSkillSpeedRatio::ONE)
                .unwrap();

        assert_eq!(
            comparison.without_provider_minus_observed.numerator(),
            1_910_000
        );
        assert_eq!(
            comparison.without_provider_minus_observed.denominator(),
            5_191
        );
    }

    #[test]
    fn exact_speed_capacity_fraction_preserves_a_230_basis_point_attack_delta() {
        let observed = skill_stage_speed(
            SkillStageSpeedFamily::Normal,
            SkillStageSpeedInputs {
                attack_speed_enabled: true,
                attack_speed_basis_points: 612,
                ..Default::default()
            },
        )
        .unwrap();
        let without_provider = skill_stage_speed(
            SkillStageSpeedFamily::Normal,
            SkillStageSpeedInputs {
                attack_speed_enabled: true,
                attack_speed_basis_points: 382,
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(
            exact_external_speed_capacity_fraction(5_306_000, observed, without_provider),
            Some((115_000, 1))
        );
    }

    #[test]
    fn exact_speed_capacity_fraction_preserves_a_382_basis_point_cast_delta() {
        let observed = skill_stage_speed(
            SkillStageSpeedFamily::Guide,
            SkillStageSpeedInputs {
                cast_speed_basis_points: 382,
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(
            exact_external_speed_capacity_fraction(5_191_000, observed, ExactSkillSpeedRatio::ONE),
            Some((191_000, 1))
        );
    }

    #[test]
    fn speed_capacity_fraction_fails_closed_without_a_positive_external_delta() {
        assert_eq!(
            exact_external_speed_capacity_fraction(
                1_000,
                ExactSkillSpeedRatio::ONE,
                ExactSkillSpeedRatio::ONE
            ),
            None
        );
        assert_eq!(
            exact_external_speed_capacity_fraction(
                1_000,
                ExactSkillSpeedRatio::ONE,
                ExactSkillSpeedRatio::new(10_382, 10_000).unwrap()
            ),
            None
        );
        assert_eq!(
            exact_external_speed_capacity_fraction(
                0,
                ExactSkillSpeedRatio::new(10_382, 10_000).unwrap(),
                ExactSkillSpeedRatio::ONE
            ),
            None
        );
    }

    #[test]
    fn unaffected_stage_has_exactly_zero_saved_time() {
        let comparison = exact_skill_stage_timing_counterfactual(
            10_000,
            ExactSkillSpeedRatio::ONE,
            ExactSkillSpeedRatio::ONE,
        )
        .unwrap();
        assert_eq!(comparison.observed_duration.numerator(), 10_000);
        assert_eq!(comparison.observed_duration.denominator(), 1);
        assert_eq!(comparison.without_provider_minus_observed.numerator(), 0);
        assert_eq!(comparison.without_provider_minus_observed.denominator(), 1);
        assert!(!comparison.without_provider_minus_observed.is_positive());
    }

    #[test]
    fn singing_counterfactual_replays_both_client_boundaries() {
        let observed = singing_stage_speed(
            SkillStageSpeedInputs {
                cast_speed_basis_points: 2_000,
                ..Default::default()
            },
            1_000,
            1_000,
        )
        .unwrap();
        let without_provider =
            singing_stage_speed(SkillStageSpeedInputs::default(), 1_000, 1_000).unwrap();
        let comparison =
            exact_skill_stage_timing_counterfactual(1_000, observed, without_provider).unwrap();

        assert_eq!(comparison.observed_duration.numerator(), 2_500);
        assert_eq!(comparison.observed_duration.denominator(), 3);
        assert_eq!(comparison.without_provider_minus_observed.numerator(), 500);
        assert_eq!(comparison.without_provider_minus_observed.denominator(), 3);
    }

    #[test]
    fn timing_counterfactual_rejects_non_positive_duration() {
        assert_eq!(
            exact_skill_stage_timing_counterfactual(
                0,
                ExactSkillSpeedRatio::ONE,
                ExactSkillSpeedRatio::ONE,
            ),
            None
        );
    }
}
