//! Exact integer helpers for packet-proven state-dependent mechanics.
//!
//! These functions do not decide whether a status deserves rDPS credit. They
//! only replay a formula after packet evidence has already established the
//! provider, recipient, fixed-point input, stage order, and damage relation.
//! The original canonical event must always remain intact.

use serde::Deserialize;

pub const BPSR_FIXED_POINT_SCALE: i64 = 10_000;

/// Exact-build interpretation of packet attribute 12510. The current build
/// deliberately selects `Unresolved`; the other variants let controlled
/// proof/replay exercise both retained candidates without embedding either
/// candidate as an implicit runtime fact.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum CriticalDamageFactorInterpretation {
    Unresolved,
    AdditiveBonus,
    DirectTotal,
}

impl CriticalDamageFactorInterpretation {
    /// Returns `(total factor, bonus above the normal-hit body)` in 1/10,000
    /// units. Unresolved or non-positive bonus states fail closed.
    fn factor_and_bonus(self, critical_damage_raw: i64) -> Option<(i64, i64)> {
        let (factor, bonus) = match self {
            Self::Unresolved => return None,
            Self::AdditiveBonus => (
                BPSR_FIXED_POINT_SCALE.checked_add(critical_damage_raw)?,
                critical_damage_raw,
            ),
            Self::DirectTotal => (
                critical_damage_raw,
                critical_damage_raw.checked_sub(BPSR_FIXED_POINT_SCALE)?,
            ),
        };
        (factor > BPSR_FIXED_POINT_SCALE && bonus > 0).then_some((factor, bonus))
    }

    pub const fn is_resolved(self) -> bool {
        !matches!(self, Self::Unresolved)
    }
}

/// Formula-family boundary selected from the current-build `DamageScript`
/// field in `DamageAttrTable`.
///
/// `DamageType` is a separate integer classification and must never be
/// interpreted as a string-pool pointer or used to select a formula.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PacketDamageScriptFamily {
    /// A row whose `DamageScript` is exactly `Attack` or `MAttack` and whose
    /// packet-linked coefficient is the complete offensive-stat multiplier in
    /// 1/10,000 units.
    StandardAttack,
    /// Any script whose integer semantics have not yet been independently
    /// proven, such as `AttackLucky`.
    Unsupported,
}

/// Positive fixed-point rounding stages currently considered by the offline
/// damage-formula proof tools. A rule is not eligible for the live rDPS path
/// until packet evidence reduces this to one exact stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PositiveFixedPointRounding {
    Floor,
    HalfUp,
}

/// One exact factor/base interval that reproduces both sides of an observed
/// additive fixed-point transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdditiveFixedPointPairCandidate {
    pub rounding: PositiveFixedPointRounding,
    pub inactive_factor: i64,
    pub active_factor: i64,
    pub minimum_base_value: i64,
    pub maximum_base_value: i64,
}

/// Solves an observed inactive/active damage pair without guessing the latent
/// pre-percentage base. Only the bounded factor domain is enumerated; the
/// complete base preimage is retained as an interval.
///
/// This is an offline proof helper. It is not called by packet capture or the
/// live meter reducer.
pub fn additive_fixed_point_pair_candidates(
    inactive_amount: i64,
    active_amount: i64,
    provider_raw_delta: i64,
    minimum_inactive_factor: i64,
    maximum_inactive_factor: i64,
) -> Vec<AdditiveFixedPointPairCandidate> {
    if inactive_amount < 0
        || active_amount < 0
        || provider_raw_delta <= 0
        || minimum_inactive_factor <= 0
        || maximum_inactive_factor < minimum_inactive_factor
    {
        return Vec::new();
    }

    let mut candidates = Vec::new();
    for rounding in [
        PositiveFixedPointRounding::Floor,
        PositiveFixedPointRounding::HalfUp,
    ] {
        for inactive_factor in minimum_inactive_factor..=maximum_inactive_factor {
            let Some(active_factor) = inactive_factor.checked_add(provider_raw_delta) else {
                continue;
            };
            let Some(inactive_base) =
                positive_fixed_point_preimage(inactive_amount, inactive_factor, rounding)
            else {
                continue;
            };
            let Some(active_base) =
                positive_fixed_point_preimage(active_amount, active_factor, rounding)
            else {
                continue;
            };
            let minimum_base_value = inactive_base.0.max(active_base.0);
            let maximum_base_value = inactive_base.1.min(active_base.1);
            if minimum_base_value <= maximum_base_value {
                candidates.push(AdditiveFixedPointPairCandidate {
                    rounding,
                    inactive_factor,
                    active_factor,
                    minimum_base_value,
                    maximum_base_value,
                });
            }
        }
    }
    candidates
}

/// Removes one proven additive component from the factor that produced an
/// observed positive fixed-point output. A marginal is returned only when
/// every latent base value capable of producing the observed output yields the
/// same provider-removed counterfactual. Ambiguous integer outputs are kept in
/// the canonical stream and return `None` here.
pub fn exact_additive_fixed_point_marginal_from_observed_output(
    observed_output: i64,
    current_factor: i64,
    provider_raw_delta: i64,
    rounding: PositiveFixedPointRounding,
) -> Option<i64> {
    if observed_output <= 0 || provider_raw_delta <= 0 || current_factor <= provider_raw_delta {
        return None;
    }
    let (minimum_base_value, maximum_base_value) =
        positive_fixed_point_preimage(observed_output, current_factor, rounding)?;
    let provider_removed_factor = current_factor.checked_sub(provider_raw_delta)?;
    let minimum_counterfactual =
        positive_fixed_point_output(minimum_base_value, provider_removed_factor, rounding)?;
    let maximum_counterfactual =
        positive_fixed_point_output(maximum_base_value, provider_removed_factor, rounding)?;
    if minimum_counterfactual != maximum_counterfactual {
        return None;
    }
    observed_output
        .checked_sub(minimum_counterfactual)
        .filter(|amount| *amount > 0 && *amount <= observed_output)
}

/// Returns the exact change in one fixed-point percentage stage when a proven
/// provider contribution is removed from the current raw percentage value.
pub fn fixed_point_percent_input_marginal(
    base_value: i64,
    current_raw_percent: i64,
    provider_raw_percent: i64,
) -> Option<i64> {
    if base_value < 0 || provider_raw_percent < 0 || current_raw_percent < provider_raw_percent {
        return None;
    }
    let current = fixed_point_product(base_value, current_raw_percent)?;
    let without_provider = fixed_point_product(
        base_value,
        current_raw_percent.checked_sub(provider_raw_percent)?,
    )?;
    current.checked_sub(without_provider)
}

/// Converts one positive, packet-proven provider delta through a linear
/// fixed-point relation without floating point or allocation.
///
/// This is the narrow arithmetic boundary needed for derived attributes such
/// as the build-24252055 Falconry relation
/// `delta(Light Bonus) = floor(delta(Mastery) * 60 / 100)`. The numerator and
/// denominator are supplied by a versioned, packet-proven rule; descriptions
/// and current profile snapshots are never consulted here. A zero result is
/// retained as a valid exact conversion because small deltas can disappear at
/// the game's integer boundary.
pub fn exact_positive_linear_conversion_delta(
    provider_input_delta: i64,
    numerator: i64,
    denominator: i64,
) -> Option<i64> {
    if provider_input_delta < 0 || numerator < 0 || denominator <= 0 {
        return None;
    }
    let converted = i128::from(provider_input_delta)
        .checked_mul(i128::from(numerator))?
        .checked_div(i128::from(denominator))?;
    i64::try_from(converted).ok()
}

/// Replays one packet-observed BPSR attribute family and returns the exact
/// final-value marginal owned by one external provider.
///
/// Current-build packets prove the family stages as:
///
/// `total = floor(add * (10000 + percent) / 10000)`
///
/// `current = total + extra_add`
///
/// Provider components are removed together before replay, preserving the
/// cross-term between additive and percentage inputs. This does not decide
/// whether the family is a damage multiplier and never reads a newer profile
/// snapshot; callers must supply fields captured for the attributed event.
pub fn packet_attribute_family_provider_marginal(
    current_add: i64,
    current_percent: i64,
    current_extra_add: i64,
    provider_add: i64,
    provider_percent: i64,
    provider_extra_add: i64,
) -> Option<i64> {
    if current_add < 0
        || current_percent < 0
        || current_extra_add < 0
        || provider_add < 0
        || provider_percent < 0
        || provider_extra_add < 0
        || provider_add > current_add
        || provider_percent > current_percent
        || provider_extra_add > current_extra_add
    {
        return None;
    }

    let current_total = packet_attribute_family_total(current_add, current_percent)?;
    let current = current_total.checked_add(current_extra_add)?;
    let without_provider_total = packet_attribute_family_total(
        current_add.checked_sub(provider_add)?,
        current_percent.checked_sub(provider_percent)?,
    )?;
    let without_provider =
        without_provider_total.checked_add(current_extra_add.checked_sub(provider_extra_add)?)?;
    current.checked_sub(without_provider)
}

/// Replays the complete packet-observed value of an additive/percentage
/// attribute family. Callers use this to reject stale or incomplete packet
/// snapshots before projecting a provider marginal.
pub fn packet_attribute_family_value(
    current_add: i64,
    current_percent: i64,
    current_extra_add: i64,
) -> Option<i64> {
    if current_extra_add < 0 {
        return None;
    }
    packet_attribute_family_total(current_add, current_percent)?.checked_add(current_extra_add)
}

/// Returns the exact provider-owned marginal at the standard Attack
/// coefficient stage of one packet-selected damage row.
///
/// Current-build game data proves the coefficient relation and packet
/// correlation proves the selected Attack input. The floor used here remains
/// an audit candidate: retained packets and client code do not yet prove the
/// server's integer boundary or its order relative to later damage stages.
/// This is deliberately not named "damage marginal" and cannot authorize
/// runtime transfer. Unsupported scripts return `None` instead of reusing
/// standard semantics.
pub fn packet_attack_coefficient_stage_provider_marginal(
    family: PacketDamageScriptFamily,
    current_attack: i64,
    provider_attack_marginal: i64,
    coefficient_basis_points: i64,
) -> Option<i64> {
    if family != PacketDamageScriptFamily::StandardAttack
        || current_attack < 0
        || provider_attack_marginal < 0
        || provider_attack_marginal > current_attack
        || coefficient_basis_points < 0
    {
        return None;
    }

    let current = fixed_point_product(current_attack, coefficient_basis_points)?;
    let without_provider = fixed_point_product(
        current_attack.checked_sub(provider_attack_marginal)?,
        coefficient_basis_points,
    )?;
    current.checked_sub(without_provider)
}

/// Returns the exact conserved accounting share owned by an external Attack
/// provider at one packet-selected standard Attack coefficient stage.
///
/// The audit candidate stage body is
/// `floor(attack * coefficient / 10000) + fixed_parameter`. Current-build
/// evidence proves the coefficient and fixed-parameter relation, but not this
/// server rounding boundary or its downstream operation order. The provider
/// owns only the difference between the active and provider-removed candidate
/// terms; the fixed parameter remains entirely with the recipient. The share
/// is carried through observed damage as an exact rational so offline audits
/// neither floor away nor manufacture damage while later stages are rebuilt.
///
/// This is an audit-only accounting projection, not an authoritative integer
/// game counterfactual. Nonstandard scripts remain ineligible until their own
/// coefficient semantics are proven.
pub fn exact_external_attack_coefficient_stage_fraction(
    observed_damage: i64,
    family: PacketDamageScriptFamily,
    current_attack: i64,
    provider_attack_marginal: i64,
    coefficient_basis_points: i64,
    fixed_parameter: i64,
) -> Option<(i128, i128)> {
    if observed_damage <= 0 {
        return None;
    }

    exact_external_attack_ordered_stage_fraction(
        observed_damage,
        family,
        current_attack,
        current_attack,
        current_attack.checked_sub(provider_attack_marginal)?,
        coefficient_basis_points,
        fixed_parameter,
    )
}

/// Returns one exact ordered provider share while retaining the complete
/// active Attack body as the accounting denominator.
///
/// `stage_with_provider_attack` and `stage_without_provider_attack` may both
/// be lower than `active_attack`. That is required when an earlier external
/// mechanic has already been removed: the later provider owns only the
/// difference between its adjacent counterfactual stages, while every share
/// is still a fraction of the one packet-observed active damage body. This
/// prevents separately removing two Attack inputs from assigning their shared
/// integer-floor cross-term twice.
pub fn exact_external_attack_ordered_stage_fraction(
    observed_damage: i64,
    family: PacketDamageScriptFamily,
    active_attack: i64,
    stage_with_provider_attack: i64,
    stage_without_provider_attack: i64,
    coefficient_basis_points: i64,
    fixed_parameter: i64,
) -> Option<(i128, i128)> {
    if observed_damage <= 0
        || family != PacketDamageScriptFamily::StandardAttack
        || active_attack < 0
        || stage_with_provider_attack < stage_without_provider_attack
        || active_attack < stage_with_provider_attack
        || coefficient_basis_points < 0
    {
        return None;
    }

    let active_coefficient_term = fixed_point_product(active_attack, coefficient_basis_points)?;
    let active_stage_body = active_coefficient_term.checked_add(fixed_parameter)?;
    let with_provider_term =
        fixed_point_product(stage_with_provider_attack, coefficient_basis_points)?;
    let without_provider_term =
        fixed_point_product(stage_without_provider_attack, coefficient_basis_points)?;
    let coefficient_stage_marginal = with_provider_term.checked_sub(without_provider_term)?;
    if active_stage_body <= 0
        || coefficient_stage_marginal <= 0
        || coefficient_stage_marginal > active_stage_body
    {
        return None;
    }

    let numerator =
        i128::from(observed_damage).checked_mul(i128::from(coefficient_stage_marginal))?;
    reduce_positive_fraction(numerator, i128::from(active_stage_body))
}

fn packet_attribute_family_total(add: i64, percent: i64) -> Option<i64> {
    if add < 0 || percent < 0 {
        return None;
    }
    let factor = BPSR_FIXED_POINT_SCALE.checked_add(percent)?;
    let value = i128::from(add).checked_mul(i128::from(factor))?;
    i64::try_from(value.checked_div(i128::from(BPSR_FIXED_POINT_SCALE))?).ok()
}

/// Replays a packet-proven two-stage MaxHP-style percentage pipeline and
/// returns only the marginal final-state value supplied by one provider.
///
/// The packet's exact current intermediate value is accepted directly so
/// unrelated flat additions and stage-local rounding remain preserved. No
/// action, status, or residual is filtered by this calculation.
pub fn two_stage_percent_input_marginal(
    base_value: i64,
    current_raw_percent: i64,
    provider_raw_percent: i64,
    current_intermediate_value: i64,
    current_raw_extra_percent: i64,
) -> Option<i64> {
    if current_intermediate_value < 0 || current_raw_extra_percent < 0 {
        return None;
    }
    let intermediate_delta =
        fixed_point_percent_input_marginal(base_value, current_raw_percent, provider_raw_percent)?;
    let without_provider_intermediate =
        current_intermediate_value.checked_sub(intermediate_delta)?;
    if without_provider_intermediate < 0 {
        return None;
    }
    let current_extra = fixed_point_product(current_intermediate_value, current_raw_extra_percent)?;
    let without_provider_extra =
        fixed_point_product(without_provider_intermediate, current_raw_extra_percent)?;
    intermediate_delta.checked_add(current_extra.checked_sub(without_provider_extra)?)
}

/// Converts a proven marginal state contribution into the corresponding
/// marginal damage for a linear state-scaled action. Constant action offsets
/// cancel from the counterfactual and are intentionally not accepted here.
pub fn linear_state_scaled_damage_marginal(
    state_multiplier: i64,
    marginal_state_value: i64,
) -> Option<i64> {
    if state_multiplier < 0 || marginal_state_value < 0 {
        return None;
    }
    let value = i128::from(state_multiplier).checked_mul(i128::from(marginal_state_value))?;
    i64::try_from(value).ok()
}

/// Returns the exact conserved share of a critical-only damage row caused by
/// one packet-proven external critical-chance component.
///
/// `interpretation` converts attribute 12510 into the total critical factor
/// and its bonus above the normal-hit body. Only that bonus is an
/// occurrence-dependent marginal; the normal-hit body remains with the
/// recipient. An unresolved interpretation emits no result.
pub fn exact_external_critical_chance_fraction(
    observed_damage: i64,
    current_critical_chance_raw: i64,
    provider_critical_chance_raw_delta: i64,
    critical_damage_raw: i64,
    interpretation: CriticalDamageFactorInterpretation,
) -> Option<(i128, i128)> {
    if observed_damage <= 0
        || current_critical_chance_raw <= 0
        || provider_critical_chance_raw_delta <= 0
        || provider_critical_chance_raw_delta > current_critical_chance_raw
    {
        return None;
    }
    let (critical_factor, critical_bonus) = interpretation.factor_and_bonus(critical_damage_raw)?;
    let numerator = i128::from(observed_damage)
        .checked_mul(i128::from(critical_bonus))?
        .checked_mul(i128::from(provider_critical_chance_raw_delta))?;
    let denominator =
        i128::from(critical_factor).checked_mul(i128::from(current_critical_chance_raw))?;
    reduce_positive_fraction(numerator, denominator)
}

/// Returns the exact conserved provider share of a non-critical Lucky row.
///
/// A Lucky packet row is an occurrence supplied by Lucky chance rather than a
/// normal-hit body with an additive bonus. Therefore the complete observed row
/// is apportioned by `provider_delta / current_chance`. Critical-plus-Lucky
/// rows are intentionally outside this function; their stage order remains a
/// separate proof obligation.
pub fn exact_external_lucky_chance_fraction(
    observed_damage: i64,
    current_lucky_chance_raw: i64,
    provider_lucky_chance_raw_delta: i64,
) -> Option<(i128, i128)> {
    if observed_damage <= 0
        || current_lucky_chance_raw <= 0
        || provider_lucky_chance_raw_delta <= 0
        || provider_lucky_chance_raw_delta > current_lucky_chance_raw
    {
        return None;
    }
    let numerator =
        i128::from(observed_damage).checked_mul(i128::from(provider_lucky_chance_raw_delta))?;
    reduce_positive_fraction(numerator, i128::from(current_lucky_chance_raw))
}

/// Returns one exact, non-overlapping provider share for a non-critical Lucky
/// row when the same provider increases both its occurrence chance and its
/// complete Lucky-DMG multiplier.
///
/// For current chance `C`, provider chance `dC`, current multiplier `M`, and
/// provider multiplier `dM`, the active expected Lucky component is `C*M` and
/// the provider-removed component is `(C-dC)*(M-dM)`. Subtracting those
/// composites before applying the observed packet value assigns their shared
/// cross-term once. Recipient-owned talent, passive, and Imagine terms remain
/// inside both multiplier states and are therefore preserved.
pub fn exact_external_lucky_chance_and_damage_fraction(
    observed_damage: i64,
    current_lucky_chance_raw: i64,
    provider_lucky_chance_raw_delta: i64,
    current_lucky_damage_raw: i64,
    provider_lucky_damage_raw_delta: i64,
) -> Option<(i128, i128)> {
    if observed_damage <= 0
        || current_lucky_chance_raw <= 0
        || provider_lucky_chance_raw_delta <= 0
        || provider_lucky_chance_raw_delta > current_lucky_chance_raw
        || current_lucky_damage_raw <= 0
        || provider_lucky_damage_raw_delta <= 0
        || provider_lucky_damage_raw_delta >= current_lucky_damage_raw
    {
        return None;
    }
    let active_composite =
        i128::from(current_lucky_chance_raw).checked_mul(i128::from(current_lucky_damage_raw))?;
    let provider_removed_composite =
        i128::from(current_lucky_chance_raw.checked_sub(provider_lucky_chance_raw_delta)?)
            .checked_mul(i128::from(
                current_lucky_damage_raw.checked_sub(provider_lucky_damage_raw_delta)?,
            ))?;
    let provider_marginal = active_composite.checked_sub(provider_removed_composite)?;
    if provider_marginal <= 0 {
        return None;
    }
    let numerator = i128::from(observed_damage).checked_mul(provider_marginal)?;
    reduce_positive_fraction(numerator, active_composite)
}

/// Returns one exact, non-overlapping provider share for a packet Lucky
/// component which also received the critical outcome flag.
///
/// Current-build packets prove this is a dedicated `lucky_value` component,
/// not a combined normal-plus-Lucky total. The provider owns the complete
/// Lucky component on occurrences caused by its Lucky-chance delta. On the
/// remaining Lucky occurrences, it owns only the critical bonus caused by its
/// critical-chance delta. Combining both terms before reduction assigns their
/// intersection exactly once and conserves the packet-observed component.
#[allow(clippy::too_many_arguments)]
pub fn exact_external_combined_critical_lucky_chance_fraction(
    observed_lucky_component_damage: i64,
    current_critical_chance_raw: i64,
    provider_critical_chance_raw_delta: i64,
    current_lucky_chance_raw: i64,
    provider_lucky_chance_raw_delta: i64,
    critical_damage_raw: i64,
    interpretation: CriticalDamageFactorInterpretation,
) -> Option<(i128, i128)> {
    if observed_lucky_component_damage <= 0
        || current_critical_chance_raw <= 0
        || provider_critical_chance_raw_delta <= 0
        || provider_critical_chance_raw_delta > current_critical_chance_raw
        || current_lucky_chance_raw <= 0
        || provider_lucky_chance_raw_delta <= 0
        || provider_lucky_chance_raw_delta > current_lucky_chance_raw
    {
        return None;
    }
    let (critical_factor, critical_bonus) = interpretation.factor_and_bonus(critical_damage_raw)?;
    let lucky_occurrence_term = i128::from(provider_lucky_chance_raw_delta)
        .checked_mul(i128::from(current_critical_chance_raw))?
        .checked_mul(i128::from(critical_factor))?;
    let critical_bonus_term = i128::from(critical_bonus)
        .checked_mul(i128::from(provider_critical_chance_raw_delta))?
        .checked_mul(i128::from(
            current_lucky_chance_raw.checked_sub(provider_lucky_chance_raw_delta)?,
        ))?;
    let numerator = i128::from(observed_lucky_component_damage)
        .checked_mul(lucky_occurrence_term.checked_add(critical_bonus_term)?)?;
    let denominator = i128::from(current_lucky_chance_raw)
        .checked_mul(i128::from(current_critical_chance_raw))?
        .checked_mul(i128::from(critical_factor))?;
    reduce_positive_fraction(numerator, denominator)
}

/// Returns the exact conserved provider share of a critical damage multiplier.
///
/// `interpretation` converts attribute 12510 into the exact current factor.
/// Attribution is the reduced rational share of the packet-observed final
/// damage, so it does not claim an unobserved server integer counterfactual.
/// Provider credit and recipient subtraction use the same fraction and remain
/// exactly conserved. Callers must gate the factor interpretation and provider
/// delta through build-specific evidence before using this accounting model.
pub fn exact_external_critical_damage_fraction(
    observed_damage: i64,
    current_critical_damage_raw: i64,
    provider_critical_damage_raw_delta: i64,
    interpretation: CriticalDamageFactorInterpretation,
) -> Option<(i128, i128)> {
    let (current_factor, current_bonus) =
        interpretation.factor_and_bonus(current_critical_damage_raw)?;
    if provider_critical_damage_raw_delta > current_bonus {
        return None;
    }
    exact_external_fixed_point_multiplier_share_fraction(
        observed_damage,
        current_factor,
        provider_critical_damage_raw_delta,
    )
}

/// Returns the exact conserved share of a critical row supplied by one
/// external provider that contributes both critical chance and critical
/// damage.
///
/// Removing the two components independently would count their shared
/// cross-term twice. For current critical chance `C`, provider chance `dC`,
/// current normalized critical bonus `B`, and provider damage `dB`, the one
/// provider's combined critical-bonus marginal is:
///
/// `C*B - (C-dC)*(B-dB) = dC*B + (C-dC)*dB`.
///
/// The observed critical row contains the fixed normal-hit body plus `B`, so
/// the conserved fraction is divided by `C * (10000 + B)`. This function is
/// intentionally limited to packet rows already proven to be critical; it
/// does not manufacture expected damage for non-critical rows.
pub fn exact_external_critical_chance_and_damage_fraction(
    observed_damage: i64,
    current_critical_chance_raw: i64,
    provider_critical_chance_raw_delta: i64,
    current_critical_damage_raw: i64,
    provider_critical_damage_raw_delta: i64,
    interpretation: CriticalDamageFactorInterpretation,
) -> Option<(i128, i128)> {
    if observed_damage <= 0
        || current_critical_chance_raw <= 0
        || provider_critical_chance_raw_delta <= 0
        || provider_critical_chance_raw_delta > current_critical_chance_raw
        || provider_critical_damage_raw_delta <= 0
    {
        return None;
    }

    let (current_critical_factor, current_critical_bonus) =
        interpretation.factor_and_bonus(current_critical_damage_raw)?;
    if provider_critical_damage_raw_delta > current_critical_bonus {
        return None;
    }

    let remaining_critical_chance =
        current_critical_chance_raw.checked_sub(provider_critical_chance_raw_delta)?;
    let combined_marginal = i128::from(provider_critical_chance_raw_delta)
        .checked_mul(i128::from(current_critical_bonus))?
        .checked_add(
            i128::from(remaining_critical_chance)
                .checked_mul(i128::from(provider_critical_damage_raw_delta))?,
        )?;
    let numerator = i128::from(observed_damage).checked_mul(combined_marginal)?;
    let denominator =
        i128::from(current_critical_chance_raw).checked_mul(i128::from(current_critical_factor))?;
    reduce_positive_fraction(numerator, denominator)
}

/// Returns the exact conserved provider share of the packet-observed External
/// Damage bonus (attribute 11840).
///
/// The raw value is basis points above the fixed normal-damage body, so the
/// stage factor is `(10000 + current_external_damage_raw) / 10000`. The caller
/// must have already proven that `observed_damage` is the output of this exact
/// floor stage for the current client build. This primitive deliberately does
/// not establish stage ordering and therefore does not, by itself, make an
/// Inspiration vector eligible for live transfer.
pub fn exact_external_damage_bonus_fraction(
    observed_damage: i64,
    current_external_damage_raw: i64,
    provider_external_damage_raw_delta: i64,
) -> Option<(i128, i128)> {
    if current_external_damage_raw < 0
        || provider_external_damage_raw_delta > current_external_damage_raw
    {
        return None;
    }
    let current_factor = BPSR_FIXED_POINT_SCALE.checked_add(current_external_damage_raw)?;
    exact_external_fixed_point_multiplier_fraction(
        observed_damage,
        current_factor,
        provider_external_damage_raw_delta,
    )
}

/// Returns one exact, non-overlapping accounting share for an external
/// provider that contributes both Attack and External Damage.
///
/// Removing the two stages independently would assign their shared
/// multiplicative cross-term twice. For the packet-selected standard Attack
/// body `A`, its provider-removed body `A0`, the current External Damage factor
/// `E`, and its provider-removed factor `E0`, the provider owns exactly
/// `A*E - A0*E0` of the active `A*E` composite. The observed packet damage is
/// carried through that ratio as a reduced rational, so the provider and
/// recipient shares conserve the wire value without per-hit rounding loss.
///
/// This is deliberately an accounting projection rather than permission to
/// enable a partially reconstructed support effect. The caller must still
/// prove the packet row's script family, coefficient, fixed parameter, active
/// attributes, stage order, and every other provider component before a full
/// effect such as Inspiration becomes eligible for live rDPS transfer.
#[allow(clippy::too_many_arguments)]
pub fn exact_external_attack_and_damage_bonus_fraction(
    observed_damage: i64,
    family: PacketDamageScriptFamily,
    active_attack: i64,
    provider_attack_marginal: i64,
    coefficient_basis_points: i64,
    fixed_parameter: i64,
    current_external_damage_raw: i64,
    provider_external_damage_raw_delta: i64,
) -> Option<(i128, i128)> {
    if observed_damage <= 0
        || family != PacketDamageScriptFamily::StandardAttack
        || active_attack < 0
        || provider_attack_marginal < 0
        || provider_attack_marginal > active_attack
        || coefficient_basis_points < 0
        || current_external_damage_raw < 0
        || provider_external_damage_raw_delta < 0
        || provider_external_damage_raw_delta > current_external_damage_raw
        || (provider_attack_marginal == 0 && provider_external_damage_raw_delta == 0)
    {
        return None;
    }

    let without_provider_attack = active_attack.checked_sub(provider_attack_marginal)?;
    let active_attack_body = fixed_point_product(active_attack, coefficient_basis_points)?
        .checked_add(fixed_parameter)?;
    let without_provider_attack_body =
        fixed_point_product(without_provider_attack, coefficient_basis_points)?
            .checked_add(fixed_parameter)?;
    let active_external_factor = BPSR_FIXED_POINT_SCALE.checked_add(current_external_damage_raw)?;
    let without_provider_external_factor =
        active_external_factor.checked_sub(provider_external_damage_raw_delta)?;
    if active_attack_body <= 0
        || without_provider_attack_body < 0
        || without_provider_attack_body > active_attack_body
        || active_external_factor <= 0
        || without_provider_external_factor <= 0
    {
        return None;
    }

    exact_external_composite_damage_fraction(
        observed_damage,
        active_attack_body,
        without_provider_attack_body,
        &[((active_external_factor), provider_external_damage_raw_delta)],
    )
}

/// Returns one conserved provider share across the standard Attack/MAttack
/// body and any packet-proven multiplicative factors which apply to the same
/// damage event.
///
/// This is the stage-aware counterpart to
/// [`exact_external_composite_damage_fraction`]. It reconstructs only the
/// exact table-selected attack body, removes the packet-proven provider Attack
/// marginal, and delegates all later factors to the generic conserved
/// accounting primitive. The server's unexposed per-hit variance remains in
/// `observed_damage` and therefore cancels from the provider fraction; no
/// hypothetical rounded server result is manufactured.
pub fn exact_external_attack_and_factors_fraction(
    observed_damage: i64,
    family: PacketDamageScriptFamily,
    active_attack: i64,
    provider_attack_marginal: i64,
    coefficient_basis_points: i64,
    fixed_parameter: i64,
    multiplicative_factors: &[(i64, i64)],
) -> Option<(i128, i128)> {
    if observed_damage <= 0
        || family != PacketDamageScriptFamily::StandardAttack
        || active_attack < 0
        || provider_attack_marginal < 0
        || provider_attack_marginal > active_attack
        || coefficient_basis_points < 0
    {
        return None;
    }
    let provider_removed_attack = active_attack.checked_sub(provider_attack_marginal)?;
    let active_body = fixed_point_product(active_attack, coefficient_basis_points)?
        .checked_add(fixed_parameter)?;
    let provider_removed_body =
        fixed_point_product(provider_removed_attack, coefficient_basis_points)?
            .checked_add(fixed_parameter)?;
    exact_external_composite_damage_fraction(
        observed_damage,
        active_body,
        provider_removed_body,
        multiplicative_factors,
    )
}

/// Returns one conserved provider share across a base damage body and any
/// number of packet-proven multiplicative factors.
///
/// Each tuple is `(current_factor, provider_delta)`. The function removes all
/// provider deltas together, then takes the difference between the active and
/// provider-removed composites. This assigns every interaction term exactly
/// once, unlike summing independently calculated stage marginals. A borrowed
/// slice keeps the live path allocation-free; checked integer arithmetic and
/// reduced rationals avoid floating point and fail closed on invalid evidence
/// or overflow.
///
/// This function is an accounting primitive. Callers must independently prove
/// that every supplied factor applies to the packet event and that the base
/// bodies use the authoritative calculation snapshot. It never promotes a
/// game-table description into runtime formula authority.
pub fn exact_external_composite_damage_fraction(
    observed_damage: i64,
    active_base_body: i64,
    provider_removed_base_body: i64,
    multiplicative_factors: &[(i64, i64)],
) -> Option<(i128, i128)> {
    if observed_damage <= 0
        || active_base_body <= 0
        || provider_removed_base_body < 0
        || provider_removed_base_body > active_base_body
    {
        return None;
    }

    let mut active_composite = i128::from(active_base_body);
    let mut provider_removed_composite = i128::from(provider_removed_base_body);
    let mut provider_changes_any_stage = active_base_body != provider_removed_base_body;
    for &(current_factor, provider_delta) in multiplicative_factors {
        if current_factor <= 0 || provider_delta < 0 || provider_delta > current_factor {
            return None;
        }
        provider_changes_any_stage |= provider_delta != 0;
        active_composite = active_composite.checked_mul(i128::from(current_factor))?;
        provider_removed_composite = provider_removed_composite
            .checked_mul(i128::from(current_factor.checked_sub(provider_delta)?))?;
    }
    if !provider_changes_any_stage {
        return None;
    }

    let provider_composite_marginal = active_composite.checked_sub(provider_removed_composite)?;
    if provider_composite_marginal <= 0 {
        return None;
    }

    // Cancel before multiplying by the observed packet value. This widens the
    // safe exact range without changing the rational result.
    let stage_divisor = greatest_common_divisor(provider_composite_marginal, active_composite);
    let reduced_marginal = provider_composite_marginal / stage_divisor;
    let mut reduced_denominator = active_composite / stage_divisor;
    let observed_divisor =
        greatest_common_divisor(i128::from(observed_damage), reduced_denominator);
    let reduced_observed = i128::from(observed_damage) / observed_divisor;
    reduced_denominator /= observed_divisor;
    let numerator = reduced_observed.checked_mul(reduced_marginal)?;
    Some((numerator, reduced_denominator))
}

/// Returns the exact conserved provider share of a Lucky damage multiplier.
///
/// Packet evidence proves that attribute 12530 is the complete Lucky-row
/// factor rather than an additive bonus over the normal-hit body. As with the
/// critical stage, outputs outside the exact integer floor image are retained
/// without attribution.
pub fn exact_external_lucky_damage_fraction(
    observed_damage: i64,
    current_lucky_damage_raw: i64,
    provider_lucky_damage_raw_delta: i64,
) -> Option<(i128, i128)> {
    exact_external_fixed_point_multiplier_fraction(
        observed_damage,
        current_lucky_damage_raw,
        provider_lucky_damage_raw_delta,
    )
}

fn exact_external_fixed_point_multiplier_fraction(
    observed_damage: i64,
    current_factor: i64,
    provider_raw_delta: i64,
) -> Option<(i128, i128)> {
    if observed_damage <= 0
        || current_factor <= 0
        || provider_raw_delta <= 0
        || provider_raw_delta >= current_factor
    {
        return None;
    }
    positive_fixed_point_preimage(
        observed_damage,
        current_factor,
        PositiveFixedPointRounding::Floor,
    )?;
    let numerator = i128::from(observed_damage).checked_mul(i128::from(provider_raw_delta))?;
    reduce_positive_fraction(numerator, i128::from(current_factor))
}

fn exact_external_fixed_point_multiplier_share_fraction(
    observed_damage: i64,
    current_factor: i64,
    provider_raw_delta: i64,
) -> Option<(i128, i128)> {
    if observed_damage <= 0
        || current_factor <= 0
        || provider_raw_delta <= 0
        || provider_raw_delta >= current_factor
    {
        return None;
    }
    let numerator = i128::from(observed_damage).checked_mul(i128::from(provider_raw_delta))?;
    reduce_positive_fraction(numerator, i128::from(current_factor))
}

fn reduce_positive_fraction(numerator: i128, denominator: i128) -> Option<(i128, i128)> {
    if numerator <= 0 || denominator <= 0 {
        return None;
    }
    let divisor = greatest_common_divisor(numerator, denominator);
    Some((numerator / divisor, denominator / divisor))
}

fn greatest_common_divisor(mut left: i128, mut right: i128) -> i128 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left.abs().max(1)
}

fn fixed_point_product(value: i64, raw_percent: i64) -> Option<i64> {
    let product = i128::from(value).checked_mul(i128::from(raw_percent))?;
    let scaled = product.checked_div(i128::from(BPSR_FIXED_POINT_SCALE))?;
    i64::try_from(scaled).ok()
}

fn positive_fixed_point_preimage(
    output: i64,
    factor: i64,
    rounding: PositiveFixedPointRounding,
) -> Option<(i64, i64)> {
    if output < 0 || factor <= 0 {
        return None;
    }
    let scale = i128::from(BPSR_FIXED_POINT_SCALE);
    let bias = match rounding {
        PositiveFixedPointRounding::Floor => 0,
        PositiveFixedPointRounding::HalfUp => scale / 2,
    };
    let factor = i128::from(factor);
    let lower_numerator = i128::from(output).checked_mul(scale)?.checked_sub(bias)?;
    let upper_numerator = i128::from(output)
        .checked_add(1)?
        .checked_mul(scale)?
        .checked_sub(bias)?;
    let lower = if lower_numerator <= 0 {
        0
    } else {
        ceil_div_positive(lower_numerator, factor)?
    };
    let upper = ceil_div_positive(upper_numerator, factor)?.checked_sub(1)?;
    if lower > upper {
        return None;
    }
    Some((i64::try_from(lower).ok()?, i64::try_from(upper).ok()?))
}

fn positive_fixed_point_output(
    base_value: i64,
    factor: i64,
    rounding: PositiveFixedPointRounding,
) -> Option<i64> {
    if base_value < 0 || factor <= 0 {
        return None;
    }
    let scale = i128::from(BPSR_FIXED_POINT_SCALE);
    let bias = match rounding {
        PositiveFixedPointRounding::Floor => 0,
        PositiveFixedPointRounding::HalfUp => scale / 2,
    };
    let output = i128::from(base_value)
        .checked_mul(i128::from(factor))?
        .checked_add(bias)?
        .checked_div(scale)?;
    i64::try_from(output).ok()
}

fn ceil_div_positive(numerator: i128, denominator: i128) -> Option<i128> {
    if numerator < 0 || denominator <= 0 {
        return None;
    }
    numerator
        .checked_add(denominator.checked_sub(1)?)?
        .checked_div(denominator)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_observed_two_stack_max_hp_provider_replays_exactly() {
        // Effect 2404261 supplied two observed +250 raw-percent increments to
        // actor 1489989468800 before Judgment Pursuit's calculation snapshot.
        let intermediate = fixed_point_percent_input_marginal(473_072, 2_700, 500);
        assert_eq!(intermediate, Some(23_654));

        let final_delta = two_stage_percent_input_marginal(473_072, 2_700, 500, 600_807, 5_185);
        assert_eq!(final_delta, Some(35_919));
        assert_eq!(
            linear_state_scaled_damage_marginal(3, final_delta.unwrap()),
            Some(107_757)
        );
    }

    #[test]
    fn current_build_fixed_point_scale_is_ten_thousand() {
        assert_eq!(
            fixed_point_percent_input_marginal(473_072, 2_450, 250),
            Some(11_827)
        );
        assert_ne!(
            fixed_point_percent_input_marginal(473_072, 2_450, 25),
            Some(11_827)
        );
    }

    #[test]
    fn packet_proven_mastery_conversion_uses_exact_integer_arithmetic() {
        assert_eq!(
            exact_positive_linear_conversion_delta(300, 60, 100),
            Some(180)
        );
        assert_eq!(exact_positive_linear_conversion_delta(1, 60, 100), Some(0));
        assert_eq!(exact_positive_linear_conversion_delta(300, 0, 100), Some(0));
    }

    #[test]
    fn linear_conversion_fails_closed_on_invalid_evidence() {
        assert_eq!(exact_positive_linear_conversion_delta(-1, 60, 100), None);
        assert_eq!(exact_positive_linear_conversion_delta(300, -1, 100), None);
        assert_eq!(exact_positive_linear_conversion_delta(300, 60, 0), None);
    }

    #[test]
    fn packet_attribute_family_replays_current_build_inspiration_transitions() {
        assert_eq!(
            packet_attribute_family_provider_marginal(1125, 1900, 0, 648, 0, 0),
            Some(771)
        );
        assert_eq!(
            packet_attribute_family_provider_marginal(5248, 1600, 0, 447, 0, 0),
            Some(518)
        );
    }

    #[test]
    fn packet_attribute_family_removes_all_provider_stages_together() {
        assert_eq!(
            packet_attribute_family_provider_marginal(1000, 2000, 360, 100, 500, 360),
            Some(525)
        );
        assert_eq!(
            packet_attribute_family_provider_marginal(100, 100, 0, 101, 0, 0),
            None
        );
    }

    #[test]
    fn standard_attack_coefficient_stage_replays_current_build_eagle_rows() {
        assert_eq!(
            packet_attack_coefficient_stage_provider_marginal(
                PacketDamageScriptFamily::StandardAttack,
                6_087,
                518,
                50_000,
            ),
            Some(2_590)
        );
        assert_eq!(
            packet_attack_coefficient_stage_provider_marginal(
                PacketDamageScriptFamily::StandardAttack,
                6_087,
                518,
                8_000,
            ),
            Some(414)
        );
    }

    #[test]
    fn external_attack_stage_share_is_exact_and_conserved() {
        assert_eq!(
            exact_external_attack_coefficient_stage_fraction(
                138_557,
                PacketDamageScriptFamily::StandardAttack,
                6_181,
                180,
                20_000,
                0,
            ),
            Some((24_940_260, 6_181))
        );

        let (numerator, denominator) = exact_external_attack_coefficient_stage_fraction(
            100_000,
            PacketDamageScriptFamily::StandardAttack,
            1_000,
            100,
            10_000,
            250,
        )
        .expect("valid standard Attack stage");
        assert_eq!((numerator, denominator), (8_000, 1));
        assert_eq!(
            numerator + (i128::from(100_000) * denominator - numerator),
            i128::from(100_000) * denominator
        );
    }

    #[test]
    fn ordered_attack_stage_shares_use_one_active_body_without_double_counting() {
        let observed_damage = 100_000;
        let harmony = exact_external_attack_ordered_stage_fraction(
            observed_damage,
            PacketDamageScriptFamily::StandardAttack,
            1_300,
            1_300,
            1_200,
            10_000,
            100,
        )
        .expect("first ordered Attack stage");
        let functional_amp = exact_external_attack_ordered_stage_fraction(
            observed_damage,
            PacketDamageScriptFamily::StandardAttack,
            1_300,
            1_200,
            1_100,
            10_000,
            100,
        )
        .expect("second ordered Attack stage");

        assert_eq!(harmony, (50_000, 7));
        assert_eq!(functional_amp, (50_000, 7));
        assert_eq!(
            harmony.0 * functional_amp.1 + functional_amp.0 * harmony.1,
            100_000 * harmony.1
        );
        assert_eq!(
            exact_external_attack_ordered_stage_fraction(
                observed_damage,
                PacketDamageScriptFamily::StandardAttack,
                1_300,
                1_100,
                1_200,
                10_000,
                100,
            ),
            None,
            "reversed adjacent stages must never be guessed"
        );
    }

    #[test]
    fn external_attack_stage_share_rejects_unproven_or_invalid_rows() {
        assert_eq!(
            exact_external_attack_coefficient_stage_fraction(
                100,
                PacketDamageScriptFamily::Unsupported,
                1_000,
                100,
                30_000,
                0,
            ),
            None
        );
        assert_eq!(
            exact_external_attack_coefficient_stage_fraction(
                100,
                PacketDamageScriptFamily::StandardAttack,
                1_000,
                0,
                10_000,
                0,
            ),
            None
        );
        assert_eq!(
            exact_external_attack_coefficient_stage_fraction(
                100,
                PacketDamageScriptFamily::StandardAttack,
                1_000,
                100,
                10_000,
                -1_000,
            ),
            None
        );
    }

    #[test]
    fn combined_external_critical_support_removes_the_cross_term_once() {
        let observed_damage = 100_000;
        let current_chance = 8_000;
        let provider_chance = 1_852;
        let current_critical_damage = 10_566;
        let provider_critical_damage = 566;
        let (numerator, denominator) = exact_external_critical_chance_and_damage_fraction(
            observed_damage,
            current_chance,
            provider_chance,
            current_critical_damage,
            provider_critical_damage,
            CriticalDamageFactorInterpretation::AdditiveBonus,
        )
        .expect("packet-proven Thunderwind vector should be representable");

        let active = i128::from(current_chance) * i128::from(current_critical_damage);
        let without_provider = i128::from(current_chance - provider_chance)
            * i128::from(current_critical_damage - provider_critical_damage);
        let current_factor = i128::from(BPSR_FIXED_POINT_SCALE + current_critical_damage);
        assert_eq!(
            numerator * i128::from(current_chance) * current_factor,
            i128::from(observed_damage) * denominator * (active - without_provider)
        );

        let chance_only = exact_external_critical_chance_fraction(
            observed_damage,
            current_chance,
            provider_chance,
            current_critical_damage,
            CriticalDamageFactorInterpretation::AdditiveBonus,
        )
        .unwrap();
        let damage_only = exact_external_critical_damage_fraction(
            observed_damage,
            current_critical_damage,
            provider_critical_damage,
            CriticalDamageFactorInterpretation::AdditiveBonus,
        )
        .unwrap();
        assert!(
            numerator * chance_only.1 * damage_only.1
                < chance_only.0 * denominator * damage_only.1
                    + damage_only.0 * denominator * chance_only.1,
            "independent removal must be larger because it double-counts the shared term"
        );
    }

    #[test]
    fn critical_damage_interpretation_is_explicit_and_candidate_arithmetic_conserves() {
        let unresolved = CriticalDamageFactorInterpretation::Unresolved;
        assert!(!unresolved.is_resolved());
        assert_eq!(
            exact_external_critical_chance_fraction(46_908, 2_777, 300, 10_128, unresolved),
            None,
        );
        assert_eq!(
            exact_external_critical_damage_fraction(46_908, 10_128, 520, unresolved),
            None,
        );

        // These raw values normalize to the same factor and bonus under the
        // two retained interpretations: 10_128 additive -> factor 20_128,
        // while 20_128 direct-total -> factor 20_128.
        let additive = CriticalDamageFactorInterpretation::AdditiveBonus;
        let direct = CriticalDamageFactorInterpretation::DirectTotal;
        assert!(additive.is_resolved() && direct.is_resolved());
        assert_eq!(
            exact_external_critical_chance_fraction(46_908, 2_777, 300, 10_128, additive),
            exact_external_critical_chance_fraction(46_908, 2_777, 300, 20_128, direct),
        );
        assert_eq!(
            exact_external_critical_damage_fraction(46_908, 10_128, 520, additive),
            exact_external_critical_damage_fraction(46_908, 20_128, 520, direct),
        );
        assert_eq!(
            exact_external_critical_chance_and_damage_fraction(
                100_000, 8_000, 1_852, 10_128, 520, additive,
            ),
            exact_external_critical_chance_and_damage_fraction(
                100_000, 8_000, 1_852, 20_128, 520, direct,
            ),
        );
        assert_eq!(
            exact_external_combined_critical_lucky_chance_fraction(
                15_000, 3_000, 300, 1_500, 300, 10_128, additive,
            ),
            exact_external_combined_critical_lucky_chance_fraction(
                15_000, 3_000, 300, 1_500, 300, 20_128, direct,
            ),
        );
    }

    #[test]
    fn special_attack_mode_cannot_reuse_standard_coefficient_semantics() {
        assert_eq!(
            packet_attack_coefficient_stage_provider_marginal(
                PacketDamageScriptFamily::Unsupported,
                6_087,
                518,
                30_000,
            ),
            None
        );
    }

    #[test]
    fn invalid_or_overflowing_counterfactuals_are_not_guessed() {
        assert_eq!(fixed_point_percent_input_marginal(100, 200, 201), None);
        assert_eq!(two_stage_percent_input_marginal(100, 200, 50, 0, 100), None);
        assert_eq!(linear_state_scaled_damage_marginal(-1, 10), None);
        assert_eq!(linear_state_scaled_damage_marginal(i64::MAX, 2), None);
        assert_eq!(
            exact_external_critical_chance_fraction(
                100,
                250,
                251,
                5_000,
                CriticalDamageFactorInterpretation::AdditiveBonus,
            ),
            None
        );
        assert_eq!(exact_external_lucky_chance_fraction(100, 300, 301), None);
    }

    #[test]
    fn inspiration_chance_components_are_exact_reduced_rationals() {
        assert_eq!(
            exact_external_critical_chance_fraction(
                12_000,
                3_000,
                300,
                5_000,
                CriticalDamageFactorInterpretation::AdditiveBonus,
            ),
            Some((400, 1))
        );
        assert_eq!(
            exact_external_lucky_chance_fraction(9_000, 1_500, 300),
            Some((1_800, 1))
        );
        assert_eq!(
            exact_external_lucky_chance_and_damage_fraction(9_000, 650, 150, 4_162, 37),
            Some((57_852_000, 27_053))
        );
        assert_eq!(
            exact_external_lucky_chance_and_damage_fraction(9_000, 650, 150, 4_502, 37),
            Some((62_442_000, 29_263)),
            "recipient-owned Lucky-DMG terms remain in both counterfactual states"
        );
        assert_eq!(
            exact_external_lucky_chance_and_damage_fraction(9_000, 650, 651, 4_162, 37),
            None
        );
        assert_eq!(
            exact_external_combined_critical_lucky_chance_fraction(
                15_000,
                3_000,
                300,
                1_500,
                300,
                5_000,
                CriticalDamageFactorInterpretation::AdditiveBonus,
            ),
            Some((3_400, 1))
        );
        assert_eq!(
            exact_external_combined_critical_lucky_chance_fraction(
                15_000,
                3_000,
                300,
                1_500,
                1_501,
                5_000,
                CriticalDamageFactorInterpretation::AdditiveBonus,
            ),
            None
        );

        let (numerator, denominator) = exact_external_critical_chance_fraction(
            46_908,
            2_777,
            300,
            10_128,
            CriticalDamageFactorInterpretation::AdditiveBonus,
        )
        .expect("valid current-build fixed-point values");
        assert_eq!(
            numerator
                .checked_mul(20_128)
                .and_then(|value| value.checked_mul(2_777)),
            i128::from(46_908)
                .checked_mul(10_128)
                .and_then(|value| value.checked_mul(300))
                .and_then(|value| value.checked_mul(denominator))
        );
    }

    #[test]
    fn external_damage_multiplier_components_are_exact_reduced_rationals() {
        assert_eq!(
            exact_external_critical_damage_fraction(
                46_908,
                10_128,
                520,
                CriticalDamageFactorInterpretation::AdditiveBonus,
            ),
            Some((762_255, 629))
        );
        assert_eq!(
            exact_external_lucky_damage_fraction(273_931, 4_540, 340),
            Some((4_656_827, 227))
        );
        assert_eq!(
            exact_external_critical_damage_fraction(
                1,
                10_128,
                520,
                CriticalDamageFactorInterpretation::AdditiveBonus,
            ),
            Some((65, 2_516)),
            "critical attribution uses the conserved packet-observed proportional share without claiming a server-floor inverse"
        );
        assert_eq!(
            exact_external_lucky_damage_fraction(100, 340, 340),
            None,
            "a provider cannot own the complete packet factor"
        );
        assert_eq!(
            exact_external_damage_bonus_fraction(10_575, 575, 105),
            Some((105, 1))
        );
        assert_eq!(
            exact_external_damage_bonus_fraction(10_701, 701, 126),
            Some((126, 1))
        );
        assert_eq!(
            exact_external_damage_bonus_fraction(10_575, 575, 576),
            None,
            "a provider cannot remove more External Damage than is present"
        );
        assert_eq!(
            exact_external_critical_damage_fraction(
                20_128,
                10_128,
                10_129,
                CriticalDamageFactorInterpretation::AdditiveBonus,
            ),
            None,
            "a provider cannot remove the fixed normal critical body"
        );
    }

    #[test]
    fn combined_attack_and_external_damage_removes_the_cross_term_once() {
        let observed_damage = 100_000;
        let combined = exact_external_attack_and_damage_bonus_fraction(
            observed_damage,
            PacketDamageScriptFamily::StandardAttack,
            1_000,
            100,
            10_000,
            250,
            575,
            105,
        )
        .expect("packet-proven Inspiration stages should be representable");

        let active_attack_body = 1_250_i128;
        let without_attack_body = 1_150_i128;
        let active_external_factor = 10_575_i128;
        let without_external_factor = 10_470_i128;
        let active_composite = active_attack_body * active_external_factor;
        let without_provider_composite = without_attack_body * without_external_factor;
        assert_eq!(
            combined.0 * active_composite,
            i128::from(observed_damage)
                * combined.1
                * (active_composite - without_provider_composite)
        );

        let attack_only = exact_external_attack_coefficient_stage_fraction(
            observed_damage,
            PacketDamageScriptFamily::StandardAttack,
            1_000,
            100,
            10_000,
            250,
        )
        .unwrap();
        let external_only =
            exact_external_damage_bonus_fraction(observed_damage, 575, 105).unwrap();
        assert!(
            combined.0 * attack_only.1 * external_only.1
                < attack_only.0 * combined.1 * external_only.1
                    + external_only.0 * combined.1 * attack_only.1,
            "independent stage removal must be larger because it double-counts the cross-term"
        );
    }

    #[test]
    fn combined_attack_and_external_damage_accepts_single_axis_limits() {
        assert_eq!(
            exact_external_attack_and_damage_bonus_fraction(
                100_000,
                PacketDamageScriptFamily::StandardAttack,
                1_000,
                100,
                10_000,
                250,
                575,
                0,
            ),
            exact_external_attack_coefficient_stage_fraction(
                100_000,
                PacketDamageScriptFamily::StandardAttack,
                1_000,
                100,
                10_000,
                250,
            )
        );
        assert_eq!(
            exact_external_attack_and_damage_bonus_fraction(
                10_575,
                PacketDamageScriptFamily::StandardAttack,
                1_000,
                0,
                10_000,
                0,
                575,
                105,
            ),
            exact_external_damage_bonus_fraction(10_575, 575, 105)
        );
        assert_eq!(
            exact_external_attack_and_damage_bonus_fraction(
                100,
                PacketDamageScriptFamily::Unsupported,
                1_000,
                100,
                10_000,
                0,
                575,
                105,
            ),
            None
        );
    }

    #[test]
    fn composite_damage_fraction_removes_three_provider_stages_together() {
        let observed_damage = 250_000;
        let active_body = 1_250_i64;
        let removed_body = 1_150_i64;
        let factors = [(10_575_i64, 105_i64), (13_038_i64, 180_i64)];
        let actual = exact_external_composite_damage_fraction(
            observed_damage,
            active_body,
            removed_body,
            &factors,
        )
        .expect("all three packet-proven stages should compose");

        let active = i128::from(active_body) * 10_575_i128 * 13_038_i128;
        let removed = i128::from(removed_body) * 10_470_i128 * 12_858_i128;
        let expected =
            reduce_positive_fraction(i128::from(observed_damage) * (active - removed), active)
                .unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn attack_and_factor_fraction_conserves_observed_rng_damage() {
        let factors = [(10_575_i64, 105_i64), (13_038_i64, 180_i64)];
        let actual = exact_external_attack_and_factors_fraction(
            107_483,
            PacketDamageScriptFamily::StandardAttack,
            9_021,
            180,
            20_000,
            1,
            &factors,
        )
        .expect("packet-proven stages should allocate the observed roll");
        let active_body = 18_043_i128;
        let removed_body = 17_683_i128;
        let active = active_body * 10_575_i128 * 13_038_i128;
        let removed = removed_body * 10_470_i128 * 12_858_i128;
        let expected = reduce_positive_fraction(107_483_i128 * (active - removed), active)
            .expect("positive conserved fraction");
        assert_eq!(actual, expected);
    }

    #[test]
    fn composite_damage_fraction_is_allocation_free_and_fail_closed() {
        assert_eq!(
            exact_external_composite_damage_fraction(10_000, 1_000, 900, &[]),
            Some((1_000, 1))
        );
        assert_eq!(
            exact_external_composite_damage_fraction(10_000, 1_000, 1_000, &[(10_500, 0)]),
            None
        );
        assert_eq!(
            exact_external_composite_damage_fraction(10_000, 1_000, 900, &[(10_000, 10_001)]),
            None
        );
    }

    #[test]
    fn strict_55228_pair_retains_all_four_exact_integer_candidates() {
        let candidates =
            additive_fixed_point_pair_candidates(84_592, 90_015, 1_000, 10_000, 30_000);
        assert_eq!(
            candidates,
            vec![
                AdditiveFixedPointPairCandidate {
                    rounding: PositiveFixedPointRounding::Floor,
                    inactive_factor: 15_598,
                    active_factor: 16_598,
                    minimum_base_value: 54_233,
                    maximum_base_value: 54_233,
                },
                AdditiveFixedPointPairCandidate {
                    rounding: PositiveFixedPointRounding::Floor,
                    inactive_factor: 15_600,
                    active_factor: 16_600,
                    minimum_base_value: 54_226,
                    maximum_base_value: 54_226,
                },
                AdditiveFixedPointPairCandidate {
                    rounding: PositiveFixedPointRounding::HalfUp,
                    inactive_factor: 15_597,
                    active_factor: 16_597,
                    minimum_base_value: 54_236,
                    maximum_base_value: 54_236,
                },
                AdditiveFixedPointPairCandidate {
                    rounding: PositiveFixedPointRounding::HalfUp,
                    inactive_factor: 15_599,
                    active_factor: 16_599,
                    minimum_base_value: 54_229,
                    maximum_base_value: 54_229,
                },
            ]
        );
    }

    #[test]
    fn current_build_damage_row_selects_one_strict_55228_pair_candidate() {
        let current_build_inactive_factors =
            [15_000, 15_600, 16_200, 18_000, 18_600, 19_200, 21_000];
        let candidates =
            additive_fixed_point_pair_candidates(84_592, 90_015, 1_000, 10_000, 30_000)
                .into_iter()
                .filter(|candidate| {
                    current_build_inactive_factors.contains(&candidate.inactive_factor)
                })
                .collect::<Vec<_>>();
        assert_eq!(
            candidates,
            vec![AdditiveFixedPointPairCandidate {
                rounding: PositiveFixedPointRounding::Floor,
                inactive_factor: 15_600,
                active_factor: 16_600,
                minimum_base_value: 54_226,
                maximum_base_value: 54_226,
            }]
        );
    }

    #[test]
    fn exact_55228_action_scoped_counterfactual_is_unambiguous() {
        assert_eq!(
            exact_additive_fixed_point_marginal_from_observed_output(
                90_015,
                16_600,
                1_000,
                PositiveFixedPointRounding::Floor,
            ),
            Some(5_423)
        );
        assert_eq!(
            exact_additive_fixed_point_marginal_from_observed_output(
                1,
                10_001,
                1,
                PositiveFixedPointRounding::Floor,
            ),
            None,
            "an output with multiple provider-removed counterfactuals stays unattributed"
        );
    }

    #[test]
    fn pair_solver_preserves_base_ranges_and_rejects_invalid_domains() {
        assert_eq!(
            additive_fixed_point_pair_candidates(0, 0, 1, 10_000, 10_000),
            vec![
                AdditiveFixedPointPairCandidate {
                    rounding: PositiveFixedPointRounding::Floor,
                    inactive_factor: 10_000,
                    active_factor: 10_001,
                    minimum_base_value: 0,
                    maximum_base_value: 0,
                },
                AdditiveFixedPointPairCandidate {
                    rounding: PositiveFixedPointRounding::HalfUp,
                    inactive_factor: 10_000,
                    active_factor: 10_001,
                    minimum_base_value: 0,
                    maximum_base_value: 0,
                },
            ]
        );
        assert!(additive_fixed_point_pair_candidates(1, 2, 0, 1, 2).is_empty());
        assert!(additive_fixed_point_pair_candidates(1, 2, 1, 2, 1).is_empty());
    }
}
