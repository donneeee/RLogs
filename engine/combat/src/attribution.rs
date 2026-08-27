use std::{
    cmp::Reverse,
    collections::{BTreeMap, BinaryHeap, HashMap, HashSet},
};

use num_bigint::BigInt;
use num_integer::Integer;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use rlogs_events::EventEnvelope;

const BASIS_POINTS_DENOMINATOR: f64 = 10_000.0;
const MAXIMUM_RULES: usize = 4_096;
const MAXIMUM_BASIS_POINTS: u32 = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DamageContributionKind {
    /// A status on the attacking player multiplies that player's damage.
    DirectDamageAmplification,
    /// A status on the attacked entity multiplies damage received by it.
    TargetVulnerability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DamageContributionStacking {
    /// One active application of this exact effect is effective. A newer
    /// provider replaces an older provider on the same target.
    Fixed,
    /// The reviewed magnitude is multiplied by the exact observed stack count.
    StackScaled { maximum_stacks: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DamageContributionRule {
    pub effect_id: i64,
    pub kind: DamageContributionKind,
    pub magnitude_basis_points: u32,
    pub stacking: DamageContributionStacking,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContributionStatusState {
    Applied,
    Refreshed,
    Stacked,
    Consumed,
    Removed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContributionStatusEvent {
    pub observed_micros: u64,
    pub source_actor_id: Option<u64>,
    pub target_actor_id: u64,
    pub effect_id: i64,
    pub instance_id: Option<i64>,
    pub state: ContributionStatusState,
    pub stacks: Option<u32>,
    pub duration_millis: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContributionDamageEvent {
    pub observed_micros: u64,
    pub source_actor_id: u64,
    pub target_actor_id: u64,
    pub amount: i64,
    /// Status lifecycles are always advanced, but callers can exclude damage
    /// outside a selected history interval without constructing another event
    /// stream.
    pub included: bool,
}

/// One exact marginal damage transfer produced by a game-specific formula
/// projector. The original damage event remains canonical and is still
/// observed separately; this record only moves the proven external portion
/// from the recipient's rDPS to the provider's rDPS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExactDamageContributionEvent {
    pub observed_micros: u64,
    pub effect_id: i64,
    pub provider_actor_id: u64,
    pub recipient_actor_id: u64,
    pub amount: i64,
    /// The complete packet-observed damage amount used to validate that the
    /// marginal transfer cannot exceed its source event.
    pub observed_damage: i64,
    pub included: bool,
}

/// One exact rational marginal damage transfer. Some packet-observed fixed-
/// point formulas have an exact proportional attribution even when the wire
/// omits enough pre-rounding state to select one integer counterfactual. The
/// fraction is retained here instead of guessing, flooring, or dropping the
/// event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExactRationalDamageContributionEvent {
    pub observed_micros: u64,
    pub effect_id: i64,
    pub provider_actor_id: u64,
    pub recipient_actor_id: u64,
    pub numerator: i128,
    pub denominator: i128,
    pub observed_damage: i64,
    pub included: bool,
}

/// Game-owned, stateful formula projection used by the generic combat meter.
///
/// Implementations must retain uncertain events by producing no transfer; the
/// canonical event stream is never filtered through this interface.
pub trait ExactDamageContributionProjector: std::fmt::Debug + Send {
    fn enabled(&self) -> bool;
    /// Stable identity of the exact formula inputs and projector algorithm.
    /// History consumers use this to detect derived rDPS that must be replayed
    /// after a formula-pack or calculation change. Projectors without a
    /// versioned formula contract remain uncacheable.
    fn formula_identity(&self) -> Option<&str> {
        None
    }
    /// Human-readable machine status for presentation surfaces. Projectors
    /// may report an out-of-date formula pack without disabling capture,
    /// canonical events, or the rest of the combat reducer.
    fn status(&self) -> String {
        if self.enabled() {
            "partial_packet_proven_rules".into()
        } else {
            "pending_reviewed_effect_rules".into()
        }
    }
    fn reset(&mut self);
    fn observe(
        &mut self,
        envelope: &EventEnvelope,
        output: &mut Vec<ExactDamageContributionEvent>,
        rational_output: &mut Vec<ExactRationalDamageContributionEvent>,
    );
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorDamageContribution {
    pub raw_damage: i64,
    pub contribution_given: i64,
    pub contribution_received: i64,
    pub rdps_damage: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectDamageContribution {
    pub effect_id: i64,
    pub provider_actor_id: u64,
    pub recipient_actor_id: u64,
    pub amount: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RationalEffectDamageContribution {
    pub effect_id: i64,
    pub provider_actor_id: u64,
    pub recipient_actor_id: u64,
    /// Reduced exact numerator. Strings keep the audit lossless across JSON
    /// and JavaScript's safe-integer boundary.
    pub numerator: String,
    pub denominator: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DamageContributionSummary {
    pub actors: BTreeMap<u64, ActorDamageContribution>,
    pub effects: Vec<EffectDamageContribution>,
    /// Exact terms retained for every rational formula bucket. Integer actor
    /// totals are projected only after an exact checked sum per effect,
    /// provider, and recipient; these terms remain the authoritative audit
    /// representation.
    pub rational_effects: Vec<RationalEffectDamageContribution>,
    /// Backward-compatible diagnostic for exact rational buckets that could
    /// not be projected. The current accumulator uses unbounded integers, so a
    /// valid retained bucket does not overflow while summing denominators.
    #[serde(default)]
    pub rational_projection_overflow_count: u64,
    pub damage_event_count: u64,
    pub attributed_damage_event_count: u64,
    pub attributed_bonus_damage: i64,
    pub missing_source_status_count: u64,
}

impl DamageContributionSummary {
    pub fn raw_damage_total(&self) -> i128 {
        self.actors
            .values()
            .map(|actor| i128::from(actor.raw_damage))
            .sum()
    }

    pub fn rdps_damage_total(&self) -> i128 {
        self.actors
            .values()
            .map(|actor| i128::from(actor.rdps_damage))
            .sum()
    }

    pub fn is_conserved(&self) -> bool {
        self.raw_damage_total() == self.rdps_damage_total()
            && self
                .actors
                .values()
                .map(|actor| i128::from(actor.contribution_given))
                .sum::<i128>()
                == self
                    .actors
                    .values()
                    .map(|actor| i128::from(actor.contribution_received))
                    .sum::<i128>()
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DamageContributionRuleError {
    #[error("rDPS rule count {0} exceeds the safety limit")]
    TooManyRules(usize),
    #[error("rDPS effect ID {0} is not positive")]
    InvalidEffectId(i64),
    #[error("rDPS effect ID {0} is duplicated")]
    DuplicateEffectId(i64),
    #[error("rDPS effect {effect_id} has invalid magnitude {magnitude_basis_points} basis points")]
    InvalidMagnitude {
        effect_id: i64,
        magnitude_basis_points: u32,
    },
    #[error("rDPS effect {0} has a zero maximum stack count")]
    ZeroMaximumStacks(i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct StatusWindowKey {
    effect_id: i64,
    instance_id: Option<i64>,
    source_actor_id: u64,
    target_actor_id: u64,
}

#[derive(Debug, Clone, Copy)]
struct ActiveStatusWindow {
    generation: u64,
    stacks: u32,
}

#[derive(Debug, Clone, Copy)]
struct Contributor {
    effect_id: i64,
    provider_actor_id: u64,
    multiplier: f64,
}

#[derive(Debug, Clone)]
pub struct DamageContributionReducer {
    rules: HashMap<i64, DamageContributionRule>,
    eligible_providers: HashSet<u64>,
    active: HashMap<StatusWindowKey, ActiveStatusWindow>,
    outgoing_by_actor: HashMap<u64, HashSet<StatusWindowKey>>,
    incoming_by_target: HashMap<u64, HashSet<StatusWindowKey>>,
    expirations: BinaryHeap<Reverse<(u64, u64, StatusWindowKey)>>,
    next_generation: u64,
    actors: BTreeMap<u64, ActorDamageContribution>,
    effects: BTreeMap<(i64, u64, u64), i64>,
    rational_effects: BTreeMap<(i64, u64, u64, i128), i128>,
    damage_event_count: u64,
    attributed_damage_event_count: u64,
    attributed_bonus_damage: i64,
    missing_source_status_count: u64,
}

impl Default for DamageContributionReducer {
    fn default() -> Self {
        Self::new(std::iter::empty()).expect("an empty rDPS rule set is valid")
    }
}

impl DamageContributionReducer {
    pub fn new(
        rules: impl IntoIterator<Item = DamageContributionRule>,
    ) -> Result<Self, DamageContributionRuleError> {
        let rules = rules.into_iter().collect::<Vec<_>>();
        validate_rules(&rules)?;
        Ok(Self {
            rules: rules
                .into_iter()
                .map(|rule| (rule.effect_id, rule))
                .collect(),
            eligible_providers: HashSet::new(),
            active: HashMap::new(),
            outgoing_by_actor: HashMap::new(),
            incoming_by_target: HashMap::new(),
            expirations: BinaryHeap::new(),
            next_generation: 1,
            actors: BTreeMap::new(),
            effects: BTreeMap::new(),
            rational_effects: BTreeMap::new(),
            damage_event_count: 0,
            attributed_damage_event_count: 0,
            attributed_bonus_damage: 0,
            missing_source_status_count: 0,
        })
    }

    pub fn set_provider_eligible(&mut self, actor_id: u64, eligible: bool) {
        if eligible {
            self.eligible_providers.insert(actor_id);
        } else {
            self.eligible_providers.remove(&actor_id);
        }
    }

    /// Joins a temporary runtime actor into a packet-proven stable actor.
    /// This is intentionally game-neutral: the game decoder decides when two
    /// actors are the same character, while this reducer only preserves the
    /// already-observed attribution state across that identity join.
    pub fn remap_actor(&mut self, from_actor_id: u64, to_actor_id: u64) {
        if from_actor_id == to_actor_id {
            return;
        }
        if self.eligible_providers.remove(&from_actor_id) {
            self.eligible_providers.insert(to_actor_id);
        }
        if let Some(from) = self.actors.remove(&from_actor_id) {
            let to = self.actors.entry(to_actor_id).or_default();
            to.raw_damage = to.raw_damage.saturating_add(from.raw_damage);
            to.contribution_given = to
                .contribution_given
                .saturating_add(from.contribution_given);
            to.contribution_received = to
                .contribution_received
                .saturating_add(from.contribution_received);
            to.rdps_damage = to.rdps_damage.saturating_add(from.rdps_damage);
        }

        let remap_key = |key: StatusWindowKey| StatusWindowKey {
            source_actor_id: if key.source_actor_id == from_actor_id {
                to_actor_id
            } else {
                key.source_actor_id
            },
            target_actor_id: if key.target_actor_id == from_actor_id {
                to_actor_id
            } else {
                key.target_actor_id
            },
            ..key
        };
        let mut active = HashMap::with_capacity(self.active.len());
        for (key, window) in self.active.drain() {
            let key = remap_key(key);
            active
                .entry(key)
                .and_modify(|existing: &mut ActiveStatusWindow| {
                    if window.generation > existing.generation {
                        *existing = window;
                    } else if window.generation == existing.generation {
                        existing.stacks = existing.stacks.max(window.stacks);
                    }
                })
                .or_insert(window);
        }
        self.active = active;

        let expirations = self
            .expirations
            .drain()
            .map(|Reverse((expires_at, generation, key))| {
                Reverse((expires_at, generation, remap_key(key)))
            })
            .collect();
        self.expirations = expirations;
        self.outgoing_by_actor.clear();
        self.incoming_by_target.clear();
        for key in self.active.keys().copied() {
            match self.rules.get(&key.effect_id).map(|rule| rule.kind) {
                Some(DamageContributionKind::DirectDamageAmplification) => {
                    self.outgoing_by_actor
                        .entry(key.target_actor_id)
                        .or_default()
                        .insert(key);
                }
                Some(DamageContributionKind::TargetVulnerability) => {
                    self.incoming_by_target
                        .entry(key.target_actor_id)
                        .or_default()
                        .insert(key);
                }
                None => {}
            }
        }

        let mut effects = BTreeMap::new();
        for ((effect_id, provider, recipient), amount) in std::mem::take(&mut self.effects) {
            let provider = if provider == from_actor_id {
                to_actor_id
            } else {
                provider
            };
            let recipient = if recipient == from_actor_id {
                to_actor_id
            } else {
                recipient
            };
            effects
                .entry((effect_id, provider, recipient))
                .and_modify(|total: &mut i64| *total = total.saturating_add(amount))
                .or_insert(amount);
        }
        self.effects = effects;
        let mut rational_effects = BTreeMap::new();
        for ((effect_id, provider, recipient, denominator), numerator) in
            std::mem::take(&mut self.rational_effects)
        {
            let provider = if provider == from_actor_id {
                to_actor_id
            } else {
                provider
            };
            let recipient = if recipient == from_actor_id {
                to_actor_id
            } else {
                recipient
            };
            rational_effects
                .entry((effect_id, provider, recipient, denominator))
                .and_modify(|total: &mut i128| *total = total.saturating_add(numerator))
                .or_insert(numerator);
        }
        self.rational_effects = rational_effects;
    }

    pub fn reset_statuses(&mut self) {
        self.active.clear();
        self.outgoing_by_actor.clear();
        self.incoming_by_target.clear();
        self.expirations.clear();
    }

    pub fn observe_status(&mut self, event: ContributionStatusEvent) {
        self.expire_at(event.observed_micros);
        let Some(rule) = self.rules.get(&event.effect_id).cloned() else {
            return;
        };
        if matches!(
            event.state,
            ContributionStatusState::Removed | ContributionStatusState::Consumed
        ) {
            self.remove_matching_status(event);
            return;
        }
        let Some(source_actor_id) = event.source_actor_id else {
            self.missing_source_status_count = self.missing_source_status_count.saturating_add(1);
            return;
        };

        if rule.stacking == DamageContributionStacking::Fixed {
            let replacements = self
                .active
                .keys()
                .filter(|key| {
                    key.effect_id == event.effect_id
                        && key.target_actor_id == event.target_actor_id
                        && **key
                            != (StatusWindowKey {
                                effect_id: event.effect_id,
                                instance_id: event.instance_id,
                                source_actor_id,
                                target_actor_id: event.target_actor_id,
                            })
                })
                .copied()
                .collect::<Vec<_>>();
            for key in replacements {
                self.remove_window(key);
            }
        }

        let key = StatusWindowKey {
            effect_id: event.effect_id,
            instance_id: event.instance_id,
            source_actor_id,
            target_actor_id: event.target_actor_id,
        };
        let generation = self.next_generation;
        self.next_generation = self.next_generation.saturating_add(1);
        self.active.insert(
            key,
            ActiveStatusWindow {
                generation,
                stacks: event.stacks.unwrap_or(1).max(1),
            },
        );
        match rule.kind {
            DamageContributionKind::DirectDamageAmplification => {
                self.outgoing_by_actor
                    .entry(event.target_actor_id)
                    .or_default()
                    .insert(key);
            }
            DamageContributionKind::TargetVulnerability => {
                self.incoming_by_target
                    .entry(event.target_actor_id)
                    .or_default()
                    .insert(key);
            }
        }
        if let Some(duration_millis) = event.duration_millis {
            let expires_at = event
                .observed_micros
                .saturating_add(duration_millis.saturating_mul(1_000));
            self.expirations
                .push(Reverse((expires_at, generation, key)));
        }
    }

    pub fn observe_damage(&mut self, event: ContributionDamageEvent) {
        self.expire_at(event.observed_micros);
        if !event.included || event.amount <= 0 {
            return;
        }
        self.damage_event_count = self.damage_event_count.saturating_add(1);
        let source = self.actors.entry(event.source_actor_id).or_default();
        source.raw_damage = source.raw_damage.saturating_add(event.amount);
        source.rdps_damage = source.rdps_damage.saturating_add(event.amount);

        let mut contributors = Vec::new();
        self.collect_contributors(
            self.outgoing_by_actor.get(&event.source_actor_id),
            event.source_actor_id,
            &mut contributors,
        );
        self.collect_contributors(
            self.incoming_by_target.get(&event.target_actor_id),
            event.source_actor_id,
            &mut contributors,
        );
        if contributors.is_empty() {
            return;
        }

        let allocations = allocate_contributions(event.amount, &contributors);
        let total = allocations.iter().map(|(_, amount)| *amount).sum::<i64>();
        if total <= 0 {
            return;
        }
        self.attributed_damage_event_count = self.attributed_damage_event_count.saturating_add(1);
        self.attributed_bonus_damage = self.attributed_bonus_damage.saturating_add(total);
        let recipient = self.actors.entry(event.source_actor_id).or_default();
        recipient.contribution_received = recipient.contribution_received.saturating_add(total);
        recipient.rdps_damage = recipient.rdps_damage.saturating_sub(total);

        for (contributor, amount) in allocations {
            if amount == 0 {
                continue;
            }
            let provider = self
                .actors
                .entry(contributor.provider_actor_id)
                .or_default();
            provider.contribution_given = provider.contribution_given.saturating_add(amount);
            provider.rdps_damage = provider.rdps_damage.saturating_add(amount);
            let effect = self
                .effects
                .entry((
                    contributor.effect_id,
                    contributor.provider_actor_id,
                    event.source_actor_id,
                ))
                .or_default();
            *effect = effect.saturating_add(amount);
        }
    }

    /// Applies a game-specific exact counterfactual after its formula has been
    /// proven outside the generic reducer. Invalid, self-supplied, excluded,
    /// or oversized transfers are rejected without changing the raw event.
    pub fn observe_exact_contribution(&mut self, event: ExactDamageContributionEvent) -> bool {
        if !event.included
            || event.effect_id <= 0
            || event.amount <= 0
            || event.observed_damage <= 0
            || event.amount > event.observed_damage
            || event.provider_actor_id == event.recipient_actor_id
        {
            return false;
        }

        self.attributed_damage_event_count = self.attributed_damage_event_count.saturating_add(1);
        self.attributed_bonus_damage = self.attributed_bonus_damage.saturating_add(event.amount);

        let recipient = self.actors.entry(event.recipient_actor_id).or_default();
        recipient.contribution_received =
            recipient.contribution_received.saturating_add(event.amount);
        recipient.rdps_damage = recipient.rdps_damage.saturating_sub(event.amount);

        let provider = self.actors.entry(event.provider_actor_id).or_default();
        provider.contribution_given = provider.contribution_given.saturating_add(event.amount);
        provider.rdps_damage = provider.rdps_damage.saturating_add(event.amount);

        let effect = self
            .effects
            .entry((
                event.effect_id,
                event.provider_actor_id,
                event.recipient_actor_id,
            ))
            .or_default();
        *effect = effect.saturating_add(event.amount);
        true
    }

    /// Retains an exact fraction and defers its integer compatibility
    /// projection until summary time. Terms sharing a denominator are folded
    /// together, bounding live memory by observed formula states rather than
    /// hit count.
    pub fn observe_exact_rational_contribution(
        &mut self,
        event: ExactRationalDamageContributionEvent,
    ) -> bool {
        if !event.included
            || event.effect_id <= 0
            || event.numerator <= 0
            || event.denominator <= 0
            || event.observed_damage <= 0
            || event.numerator > i128::from(event.observed_damage).saturating_mul(event.denominator)
            || event.provider_actor_id == event.recipient_actor_id
        {
            return false;
        }
        let divisor = greatest_common_divisor(event.numerator, event.denominator);
        let numerator = event.numerator / divisor;
        let denominator = event.denominator / divisor;
        let bucket = self
            .rational_effects
            .entry((
                event.effect_id,
                event.provider_actor_id,
                event.recipient_actor_id,
                denominator,
            ))
            .or_default();
        *bucket = bucket.saturating_add(numerator);
        self.attributed_damage_event_count = self.attributed_damage_event_count.saturating_add(1);
        true
    }

    pub fn summary(&self) -> DamageContributionSummary {
        let mut actors = self.actors.clone();
        let mut effects = self.effects.clone();
        let mut rational_totals = BTreeMap::<(i64, u64, u64), CheckedRationalAccumulator>::new();
        let rational_effects = self
            .rational_effects
            .iter()
            .map(
                |((effect_id, provider_actor_id, recipient_actor_id, denominator), numerator)| {
                    let divisor = greatest_common_divisor(*numerator, *denominator);
                    let reduced_numerator = *numerator / divisor;
                    let reduced_denominator = *denominator / divisor;
                    let key = (*effect_id, *provider_actor_id, *recipient_actor_id);
                    rational_totals
                        .entry(key)
                        .or_default()
                        .add(reduced_numerator, reduced_denominator);
                    RationalEffectDamageContribution {
                        effect_id: *effect_id,
                        provider_actor_id: *provider_actor_id,
                        recipient_actor_id: *recipient_actor_id,
                        numerator: reduced_numerator.to_string(),
                        denominator: reduced_denominator.to_string(),
                    }
                },
            )
            .collect::<Vec<_>>();
        let mut rational_total = 0_i64;
        let rational_projection_overflow_count = 0_u64;
        for ((effect_id, provider_actor_id, recipient_actor_id), total) in rational_totals {
            let (numerator, denominator) = total.exact();
            let amount = i64::try_from(round_half_up_big_ratio(&numerator, &denominator))
                .unwrap_or(i64::MAX);
            if amount <= 0 {
                continue;
            }
            rational_total = rational_total.saturating_add(amount);
            let recipient = actors.entry(recipient_actor_id).or_default();
            recipient.contribution_received =
                recipient.contribution_received.saturating_add(amount);
            recipient.rdps_damage = recipient.rdps_damage.saturating_sub(amount);
            let provider = actors.entry(provider_actor_id).or_default();
            provider.contribution_given = provider.contribution_given.saturating_add(amount);
            provider.rdps_damage = provider.rdps_damage.saturating_add(amount);
            effects
                .entry((effect_id, provider_actor_id, recipient_actor_id))
                .and_modify(|total| *total = total.saturating_add(amount))
                .or_insert(amount);
        }
        DamageContributionSummary {
            actors,
            effects: effects
                .iter()
                .map(
                    |((effect_id, provider_actor_id, recipient_actor_id), amount)| {
                        EffectDamageContribution {
                            effect_id: *effect_id,
                            provider_actor_id: *provider_actor_id,
                            recipient_actor_id: *recipient_actor_id,
                            amount: *amount,
                        }
                    },
                )
                .collect(),
            rational_effects,
            rational_projection_overflow_count,
            damage_event_count: self.damage_event_count,
            attributed_damage_event_count: self.attributed_damage_event_count,
            attributed_bonus_damage: self.attributed_bonus_damage.saturating_add(rational_total),
            missing_source_status_count: self.missing_source_status_count,
        }
    }

    fn collect_contributors(
        &self,
        keys: Option<&HashSet<StatusWindowKey>>,
        damage_source_actor_id: u64,
        output: &mut Vec<Contributor>,
    ) {
        let Some(keys) = keys else {
            return;
        };
        for key in keys {
            if key.source_actor_id == damage_source_actor_id
                || !self.eligible_providers.contains(&key.source_actor_id)
            {
                continue;
            }
            let Some(window) = self.active.get(key) else {
                continue;
            };
            let Some(rule) = self.rules.get(&key.effect_id) else {
                continue;
            };
            let stacks = match rule.stacking {
                DamageContributionStacking::Fixed => 1,
                DamageContributionStacking::StackScaled { maximum_stacks } => {
                    window.stacks.min(maximum_stacks)
                }
            };
            let multiplier = f64::from(rule.magnitude_basis_points) * f64::from(stacks)
                / BASIS_POINTS_DENOMINATOR;
            if multiplier > 0.0 {
                output.push(Contributor {
                    effect_id: key.effect_id,
                    provider_actor_id: key.source_actor_id,
                    multiplier,
                });
            }
        }
    }

    fn expire_at(&mut self, observed_micros: u64) {
        while let Some(Reverse((expires_at, generation, key))) = self.expirations.peek().copied() {
            if expires_at > observed_micros {
                break;
            }
            self.expirations.pop();
            if self
                .active
                .get(&key)
                .is_some_and(|active| active.generation == generation)
            {
                self.remove_window(key);
            }
        }
    }

    fn remove_matching_status(&mut self, event: ContributionStatusEvent) {
        let matches = self
            .active
            .keys()
            .filter(|key| {
                key.effect_id == event.effect_id
                    && key.target_actor_id == event.target_actor_id
                    && event
                        .instance_id
                        .is_none_or(|instance_id| key.instance_id == Some(instance_id))
                    && event
                        .source_actor_id
                        .is_none_or(|source_actor_id| key.source_actor_id == source_actor_id)
            })
            .copied()
            .collect::<Vec<_>>();
        for key in matches {
            self.remove_window(key);
        }
    }

    fn remove_window(&mut self, key: StatusWindowKey) {
        if self.active.remove(&key).is_none() {
            return;
        }
        if let Some(keys) = self.outgoing_by_actor.get_mut(&key.target_actor_id) {
            keys.remove(&key);
            if keys.is_empty() {
                self.outgoing_by_actor.remove(&key.target_actor_id);
            }
        }
        if let Some(keys) = self.incoming_by_target.get_mut(&key.target_actor_id) {
            keys.remove(&key);
            if keys.is_empty() {
                self.incoming_by_target.remove(&key.target_actor_id);
            }
        }
    }
}

fn greatest_common_divisor(mut left: i128, mut right: i128) -> i128 {
    left = left.abs();
    right = right.abs();
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left.max(1)
}

#[derive(Debug, Clone)]
struct CheckedRationalAccumulator {
    numerator: BigInt,
    denominator: BigInt,
}

impl Default for CheckedRationalAccumulator {
    fn default() -> Self {
        Self {
            numerator: BigInt::from(0),
            denominator: BigInt::from(1),
        }
    }
}

impl CheckedRationalAccumulator {
    fn add(&mut self, numerator: i128, denominator: i128) {
        debug_assert!(numerator >= 0 && denominator > 0);
        let numerator = BigInt::from(numerator);
        let denominator = BigInt::from(denominator);
        let shared = self.denominator.gcd(&denominator);
        let left_factor = &denominator / &shared;
        let right_factor = &self.denominator / &shared;
        let next_numerator = &self.numerator * &left_factor + numerator * right_factor;
        let next_denominator = &self.denominator * left_factor;
        let divisor = next_numerator.gcd(&next_denominator);
        self.numerator = next_numerator / &divisor;
        self.denominator = next_denominator / divisor;
    }

    fn exact(self) -> (BigInt, BigInt) {
        (self.numerator, self.denominator)
    }
}

fn round_half_up_big_ratio(numerator: &BigInt, denominator: &BigInt) -> BigInt {
    debug_assert!(numerator >= &BigInt::from(0) && denominator > &BigInt::from(0));
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    let half_up_threshold = denominator / 2 + denominator % 2;
    if remainder >= half_up_threshold {
        quotient + 1
    } else {
        quotient
    }
}

fn validate_rules(rules: &[DamageContributionRule]) -> Result<(), DamageContributionRuleError> {
    if rules.len() > MAXIMUM_RULES {
        return Err(DamageContributionRuleError::TooManyRules(rules.len()));
    }
    let mut effect_ids = HashSet::new();
    for rule in rules {
        if rule.effect_id <= 0 {
            return Err(DamageContributionRuleError::InvalidEffectId(rule.effect_id));
        }
        if !effect_ids.insert(rule.effect_id) {
            return Err(DamageContributionRuleError::DuplicateEffectId(
                rule.effect_id,
            ));
        }
        if rule.magnitude_basis_points == 0 || rule.magnitude_basis_points > MAXIMUM_BASIS_POINTS {
            return Err(DamageContributionRuleError::InvalidMagnitude {
                effect_id: rule.effect_id,
                magnitude_basis_points: rule.magnitude_basis_points,
            });
        }
        if matches!(
            rule.stacking,
            DamageContributionStacking::StackScaled { maximum_stacks: 0 }
        ) {
            return Err(DamageContributionRuleError::ZeroMaximumStacks(
                rule.effect_id,
            ));
        }
    }
    Ok(())
}

fn allocate_contributions(
    observed_damage: i64,
    contributors: &[Contributor],
) -> Vec<(Contributor, i64)> {
    let combined_multiplier = contributors.iter().fold(1.0, |combined, contributor| {
        combined * (1.0 + contributor.multiplier)
    });
    let base_damage = (observed_damage as f64 / combined_multiplier).round() as i64;
    let bonus_damage = observed_damage.saturating_sub(base_damage).max(0);
    if bonus_damage == 0 {
        return contributors
            .iter()
            .copied()
            .map(|contributor| (contributor, 0))
            .collect();
    }

    let mut product = vec![1.0];
    for contributor in contributors {
        product.push(0.0);
        for degree in (1..product.len()).rev() {
            product[degree] += product[degree - 1] * contributor.multiplier;
        }
    }
    let base = observed_damage as f64 / combined_multiplier;
    let weights = contributors
        .iter()
        .map(|contributor| {
            let mut quotient = vec![0.0; contributors.len()];
            quotient[0] = product[0];
            for degree in 1..contributors.len() {
                quotient[degree] = product[degree] - contributor.multiplier * quotient[degree - 1];
            }
            let integral = quotient
                .iter()
                .enumerate()
                .map(|(degree, coefficient)| coefficient / (degree as f64 + 1.0))
                .sum::<f64>();
            base * contributor.multiplier * integral
        })
        .collect::<Vec<_>>();
    let weight_total = weights.iter().sum::<f64>();
    let mut allocated = Vec::with_capacity(contributors.len());
    let mut floor_total = 0_i64;
    for (index, weight) in weights.iter().enumerate() {
        let ideal = bonus_damage as f64 * *weight / weight_total;
        let floor = ideal.floor() as i64;
        floor_total = floor_total.saturating_add(floor);
        allocated.push((index, floor, ideal - floor as f64));
    }
    allocated.sort_by(|left, right| {
        right
            .2
            .total_cmp(&left.2)
            .then_with(|| left.0.cmp(&right.0))
    });
    for allocation in allocated
        .iter_mut()
        .take(bonus_damage.saturating_sub(floor_total) as usize)
    {
        allocation.1 = allocation.1.saturating_add(1);
    }
    allocated.sort_by_key(|allocation| allocation.0);
    allocated
        .into_iter()
        .map(|(index, amount, _)| (contributors[index], amount))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(effect_id: i64, basis_points: u32) -> DamageContributionRule {
        DamageContributionRule {
            effect_id,
            kind: DamageContributionKind::DirectDamageAmplification,
            magnitude_basis_points: basis_points,
            stacking: DamageContributionStacking::Fixed,
        }
    }

    fn apply(
        reducer: &mut DamageContributionReducer,
        time: u64,
        effect_id: i64,
        provider: u64,
        target: u64,
        duration_millis: Option<u64>,
    ) {
        reducer.observe_status(ContributionStatusEvent {
            observed_micros: time,
            source_actor_id: Some(provider),
            target_actor_id: target,
            effect_id,
            instance_id: Some(effect_id),
            state: ContributionStatusState::Applied,
            stacks: Some(1),
            duration_millis,
        });
    }

    fn damage(
        reducer: &mut DamageContributionReducer,
        time: u64,
        source: u64,
        target: u64,
        amount: i64,
    ) {
        reducer.observe_damage(ContributionDamageEvent {
            observed_micros: time,
            source_actor_id: source,
            target_actor_id: target,
            amount,
            included: true,
        });
    }

    #[test]
    fn transfers_one_exact_multiplier_and_conserves_party_damage() {
        let mut reducer = DamageContributionReducer::new([rule(10, 1_000)]).unwrap();
        reducer.set_provider_eligible(1, true);
        reducer.set_provider_eligible(2, true);
        apply(&mut reducer, 1, 10, 1, 2, None);
        damage(&mut reducer, 2, 2, 99, 1_100);
        let summary = reducer.summary();
        assert_eq!(summary.actors[&1].contribution_given, 100);
        assert_eq!(summary.actors[&2].contribution_received, 100);
        assert_eq!(summary.actors[&1].rdps_damage, 100);
        assert_eq!(summary.actors[&2].rdps_damage, 1_000);
        assert!(summary.is_conserved());
    }

    #[test]
    fn simultaneous_multipliers_use_symmetric_shapley_allocation() {
        let mut reducer =
            DamageContributionReducer::new([rule(10, 1_000), rule(20, 1_000)]).unwrap();
        for actor in 1..=3 {
            reducer.set_provider_eligible(actor, true);
        }
        apply(&mut reducer, 1, 10, 1, 3, None);
        apply(&mut reducer, 1, 20, 2, 3, None);
        damage(&mut reducer, 2, 3, 99, 1_210);
        let summary = reducer.summary();
        assert_eq!(summary.actors[&1].contribution_given, 105);
        assert_eq!(summary.actors[&2].contribution_given, 105);
        assert_eq!(summary.actors[&3].rdps_damage, 1_000);
        assert!(summary.is_conserved());
    }

    #[test]
    fn self_buffs_and_ineligible_sources_never_transfer_damage() {
        let mut reducer = DamageContributionReducer::new([rule(10, 1_000)]).unwrap();
        reducer.set_provider_eligible(2, true);
        apply(&mut reducer, 1, 10, 2, 2, None);
        damage(&mut reducer, 2, 2, 99, 1_100);
        assert_eq!(reducer.summary().attributed_bonus_damage, 0);

        reducer.reset_statuses();
        apply(&mut reducer, 3, 10, 1, 2, None);
        damage(&mut reducer, 4, 2, 99, 1_100);
        assert_eq!(reducer.summary().attributed_bonus_damage, 0);
    }

    #[test]
    fn vulnerability_uses_damage_target_and_expires_without_scanning_rules() {
        let mut vulnerability = rule(30, 1_000);
        vulnerability.kind = DamageContributionKind::TargetVulnerability;
        let mut reducer = DamageContributionReducer::new([vulnerability]).unwrap();
        reducer.set_provider_eligible(1, true);
        apply(&mut reducer, 1, 30, 1, 99, Some(10));
        damage(&mut reducer, 5_000, 2, 99, 1_100);
        damage(&mut reducer, 11_001, 2, 99, 1_100);
        let summary = reducer.summary();
        assert_eq!(summary.actors[&1].contribution_given, 100);
        assert_eq!(summary.actors[&2].raw_damage, 2_200);
        assert_eq!(summary.actors[&2].rdps_damage, 2_100);
        assert!(summary.is_conserved());
    }

    #[test]
    fn remove_without_source_closes_the_matching_instance() {
        let mut reducer = DamageContributionReducer::new([rule(10, 1_000)]).unwrap();
        reducer.set_provider_eligible(1, true);
        apply(&mut reducer, 1, 10, 1, 2, None);
        reducer.observe_status(ContributionStatusEvent {
            observed_micros: 2,
            source_actor_id: None,
            target_actor_id: 2,
            effect_id: 10,
            instance_id: Some(10),
            state: ContributionStatusState::Removed,
            stacks: None,
            duration_millis: None,
        });
        damage(&mut reducer, 3, 2, 99, 1_100);
        assert_eq!(reducer.summary().attributed_bonus_damage, 0);
    }

    #[test]
    fn exact_state_scaled_contribution_preserves_raw_damage_and_party_total() {
        let mut reducer = DamageContributionReducer::default();
        damage(&mut reducer, 2, 4, 99, 2_737_001);
        assert!(
            reducer.observe_exact_contribution(ExactDamageContributionEvent {
                observed_micros: 2,
                effect_id: 2_404_261,
                provider_actor_id: 2,
                recipient_actor_id: 4,
                amount: 107_757,
                observed_damage: 2_737_001,
                included: true,
            })
        );

        let summary = reducer.summary();
        assert_eq!(summary.actors[&4].raw_damage, 2_737_001);
        assert_eq!(summary.actors[&4].contribution_received, 107_757);
        assert_eq!(summary.actors[&4].rdps_damage, 2_629_244);
        assert_eq!(summary.actors[&2].contribution_given, 107_757);
        assert_eq!(summary.actors[&2].rdps_damage, 107_757);
        assert!(summary.is_conserved());
    }

    #[test]
    fn exact_state_scaled_contribution_rejects_self_and_oversized_transfers() {
        let mut reducer = DamageContributionReducer::default();
        damage(&mut reducer, 2, 4, 99, 100);
        for event in [
            ExactDamageContributionEvent {
                observed_micros: 2,
                effect_id: 1,
                provider_actor_id: 4,
                recipient_actor_id: 4,
                amount: 10,
                observed_damage: 100,
                included: true,
            },
            ExactDamageContributionEvent {
                observed_micros: 2,
                effect_id: 1,
                provider_actor_id: 2,
                recipient_actor_id: 4,
                amount: 101,
                observed_damage: 100,
                included: true,
            },
        ] {
            assert!(!reducer.observe_exact_contribution(event));
        }
        let summary = reducer.summary();
        assert_eq!(summary.actors[&4].rdps_damage, 100);
        assert_eq!(summary.attributed_bonus_damage, 0);
        assert!(summary.is_conserved());
    }

    #[test]
    fn exact_rational_contributions_are_retained_folded_and_conserved() {
        let mut reducer = DamageContributionReducer::default();
        damage(&mut reducer, 2, 4, 99, 100);
        for numerator in [1_i128, 2] {
            assert!(reducer.observe_exact_rational_contribution(
                ExactRationalDamageContributionEvent {
                    observed_micros: 2,
                    effect_id: 2_302_121,
                    provider_actor_id: 2,
                    recipient_actor_id: 4,
                    numerator,
                    denominator: 3,
                    observed_damage: 100,
                    included: true,
                },
            ));
        }

        let summary = reducer.summary();
        assert_eq!(summary.rational_effects.len(), 1);
        assert_eq!(summary.rational_effects[0].numerator, "1");
        assert_eq!(summary.rational_effects[0].denominator, "1");
        assert_eq!(summary.attributed_damage_event_count, 2);
        assert_eq!(summary.attributed_bonus_damage, 1);
        assert_eq!(summary.actors[&2].contribution_given, 1);
        assert_eq!(summary.actors[&4].contribution_received, 1);
        assert!(summary.is_conserved());
    }

    #[test]
    fn exact_rational_projection_rounds_the_combined_fraction_only_once() {
        let mut reducer = DamageContributionReducer::default();
        damage(&mut reducer, 2, 4, 99, 100);
        for (numerator, denominator) in [
            (999_999_999_i128, 4_000_000_000_i128),
            (1_000_000_000_i128, 4_000_000_001_i128),
        ] {
            assert!(reducer.observe_exact_rational_contribution(
                ExactRationalDamageContributionEvent {
                    observed_micros: 2,
                    effect_id: 2_302_121,
                    provider_actor_id: 2,
                    recipient_actor_id: 4,
                    numerator,
                    denominator,
                    observed_damage: 100,
                    included: true,
                },
            ));
        }

        // Each denominator bucket rounds to 0.25 at nine decimals, but their
        // exact sum is still below 0.5 and therefore transfers no integer.
        let summary = reducer.summary();
        assert_eq!(summary.rational_effects.len(), 2);
        assert_eq!(summary.rational_projection_overflow_count, 0);
        assert_eq!(summary.attributed_bonus_damage, 0);
        assert!(!summary.actors.contains_key(&2));
        assert_eq!(summary.actors[&4].rdps_damage, 100);
        assert!(summary.is_conserved());
    }

    #[test]
    fn rational_projection_uses_unbounded_exact_sum_before_rounding() {
        let mut reducer = DamageContributionReducer::default();
        damage(&mut reducer, 2, 4, 99, 100);
        for denominator in [i128::MAX, i128::MAX - 2] {
            assert!(reducer.observe_exact_rational_contribution(
                ExactRationalDamageContributionEvent {
                    observed_micros: 2,
                    effect_id: 2_302_121,
                    provider_actor_id: 2,
                    recipient_actor_id: 4,
                    numerator: denominator / 2,
                    denominator,
                    observed_damage: 100,
                    included: true,
                },
            ));
        }

        let summary = reducer.summary();
        assert_eq!(summary.rational_effects.len(), 2);
        assert_eq!(summary.rational_projection_overflow_count, 0);
        assert_eq!(summary.attributed_bonus_damage, 1);
        assert_eq!(summary.actors[&2].contribution_given, 1);
        assert_eq!(summary.actors[&4].rdps_damage, 99);
        assert!(summary.is_conserved());
    }

    #[test]
    fn half_up_rounding_does_not_overflow_at_i128_max() {
        assert_eq!(
            round_half_up_big_ratio(&BigInt::from(i128::MAX), &BigInt::from(i128::MAX)),
            BigInt::from(1)
        );
    }

    #[test]
    fn exact_rational_contribution_rejects_self_oversized_and_excluded_terms() {
        let mut reducer = DamageContributionReducer::default();
        for event in [
            ExactRationalDamageContributionEvent {
                observed_micros: 1,
                effect_id: 1,
                provider_actor_id: 4,
                recipient_actor_id: 4,
                numerator: 1,
                denominator: 2,
                observed_damage: 100,
                included: true,
            },
            ExactRationalDamageContributionEvent {
                observed_micros: 1,
                effect_id: 1,
                provider_actor_id: 2,
                recipient_actor_id: 4,
                numerator: 101,
                denominator: 1,
                observed_damage: 100,
                included: true,
            },
            ExactRationalDamageContributionEvent {
                observed_micros: 1,
                effect_id: 1,
                provider_actor_id: 2,
                recipient_actor_id: 4,
                numerator: 1,
                denominator: 2,
                observed_damage: 100,
                included: false,
            },
        ] {
            assert!(!reducer.observe_exact_rational_contribution(event));
        }
        assert!(reducer.summary().rational_effects.is_empty());
    }

    #[test]
    fn validates_rules_before_entering_the_hot_path() {
        assert_eq!(
            DamageContributionReducer::new([rule(10, 0)]).unwrap_err(),
            DamageContributionRuleError::InvalidMagnitude {
                effect_id: 10,
                magnitude_basis_points: 0,
            }
        );
        assert_eq!(
            DamageContributionReducer::new([rule(10, 100), rule(10, 200)]).unwrap_err(),
            DamageContributionRuleError::DuplicateEffectId(10)
        );
    }
}
