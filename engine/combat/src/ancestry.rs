//! Time-aware ownership attribution for summons, pets, projectiles, and other
//! transient combat actors.
//!
//! Games remain responsible for proving a relationship. Consumers use this
//! resolver so live, history, submissions, TPS, and rDPS do not each invent a
//! different child-to-owner policy.

use std::collections::{BTreeMap, BTreeSet};

use rlogs_events::{ActorId, DamageEvent, EntityRef, EntityUuid};

const MAX_ANCESTRY_DEPTH: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorOwnershipEvidence {
    /// `DamageInfo.top_summoner_uuid` (or the equivalent game-owned field)
    /// differed from the immediate attacker.
    AttributedCombatSource,
    /// A game plug-in observed a complete, internally consistent owner
    /// attribute set on the child actor.
    ConfirmedEntityAttributes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OwnershipObservation {
    observed_micros: u64,
    owner_actor_id: Option<u64>,
    owner_entity_uuid: Option<i64>,
    evidence: ActorOwnershipEvidence,
}

#[derive(Debug, Clone, Default)]
pub struct ActorAncestryResolver {
    actor_by_entity_uuid: BTreeMap<i64, u64>,
    entity_uuid_by_actor: BTreeMap<u64, i64>,
    ownership_by_child: BTreeMap<u64, Vec<OwnershipObservation>>,
}

impl ActorAncestryResolver {
    pub fn observe_entity(&mut self, actor: EntityRef) {
        if let Some(previous_entity_uuid) = self
            .entity_uuid_by_actor
            .insert(actor.actor_id.0, actor.entity_uuid.0)
            && previous_entity_uuid != actor.entity_uuid.0
            && self.actor_by_entity_uuid.get(&previous_entity_uuid) == Some(&actor.actor_id.0)
        {
            self.actor_by_entity_uuid.remove(&previous_entity_uuid);
        }
        if let Some(previous_actor_id) = self
            .actor_by_entity_uuid
            .insert(actor.entity_uuid.0, actor.actor_id.0)
            && previous_actor_id != actor.actor_id.0
            && self.entity_uuid_by_actor.get(&previous_actor_id) == Some(&actor.entity_uuid.0)
        {
            self.entity_uuid_by_actor.remove(&previous_actor_id);
        }
    }

    pub fn observe_damage(&mut self, observed_micros: u64, damage: &DamageEvent) {
        self.observe_entity(damage.target);
        self.observe_attributed_source(observed_micros, damage.source, damage.direct_source);
    }

    /// Records the common `source` / `direct_source` contract used by damage,
    /// healing, and any later canonical combat-result family. `source` is the
    /// already attributed top owner; `direct_source` is retained for exact
    /// child drill-down and for establishing ancestry.
    pub fn observe_attributed_source(
        &mut self,
        observed_micros: u64,
        source: EntityRef,
        direct_source: Option<EntityRef>,
    ) {
        self.observe_entity(source);
        let Some(direct) = direct_source else {
            return;
        };
        self.observe_entity(direct);
        if direct.actor_id != source.actor_id {
            self.observe_relation(
                observed_micros,
                direct,
                source,
                ActorOwnershipEvidence::AttributedCombatSource,
            );
        }
    }

    pub fn observe_relation(
        &mut self,
        observed_micros: u64,
        child: EntityRef,
        owner: EntityRef,
        evidence: ActorOwnershipEvidence,
    ) {
        self.observe_entity(child);
        self.observe_entity(owner);
        if child.actor_id == owner.actor_id || child.entity_uuid == owner.entity_uuid {
            return;
        }
        self.push_observation(
            child.actor_id.0,
            OwnershipObservation {
                observed_micros,
                owner_actor_id: Some(owner.actor_id.0),
                owner_entity_uuid: Some(owner.entity_uuid.0),
                evidence,
            },
        );
    }

    pub fn observe_owner_entity(
        &mut self,
        observed_micros: u64,
        child: EntityRef,
        owner_entity_uuid: i64,
        evidence: ActorOwnershipEvidence,
    ) {
        self.observe_entity(child);
        if owner_entity_uuid <= 0 || owner_entity_uuid == child.entity_uuid.0 {
            return;
        }
        self.push_observation(
            child.actor_id.0,
            OwnershipObservation {
                observed_micros,
                owner_actor_id: self.actor_by_entity_uuid.get(&owner_entity_uuid).copied(),
                owner_entity_uuid: Some(owner_entity_uuid),
                evidence,
            },
        );
    }

    /// Ends the previous ownership interval without discarding earlier history.
    pub fn clear_owner(&mut self, observed_micros: u64, child: EntityRef) {
        self.observe_entity(child);
        self.push_observation(
            child.actor_id.0,
            OwnershipObservation {
                observed_micros,
                owner_actor_id: None,
                owner_entity_uuid: None,
                evidence: ActorOwnershipEvidence::ConfirmedEntityAttributes,
            },
        );
    }

    /// Closes every ownership relation active at a packet-proven lifecycle
    /// boundary while retaining the completed intervals for historical facts.
    ///
    /// Short actor IDs can be reused after a wipe or scene transition. Without
    /// an explicit boundary, a newly spawned entity could inherit a pet,
    /// summon, or projectile owner observed during the previous attempt.
    pub fn end_active_ownership_intervals(&mut self, observed_micros: u64) {
        let active_children = self
            .ownership_by_child
            .iter()
            .filter_map(|(child_actor_id, observations)| {
                observations
                    .iter()
                    .rev()
                    .find(|observation| observation.observed_micros <= observed_micros)
                    .filter(|observation| {
                        observation.owner_actor_id.is_some()
                            || observation.owner_entity_uuid.is_some()
                    })
                    .map(|_| *child_actor_id)
            })
            .collect::<Vec<_>>();

        for child_actor_id in active_children {
            self.push_observation(
                child_actor_id,
                OwnershipObservation {
                    observed_micros,
                    owner_actor_id: None,
                    owner_entity_uuid: None,
                    evidence: ActorOwnershipEvidence::ConfirmedEntityAttributes,
                },
            );
        }
    }

    pub fn resolve_actor_id_at(&self, actor_id: u64, observed_micros: u64) -> u64 {
        let mut current = actor_id;
        let mut visited = BTreeSet::new();
        for _ in 0..MAX_ANCESTRY_DEPTH {
            if !visited.insert(current) {
                break;
            }
            let Some(parent) = self.parent_actor_at(current, observed_micros) else {
                break;
            };
            current = parent;
        }
        current
    }

    pub fn resolve_entity_at(&self, actor: EntityRef, observed_micros: u64) -> EntityRef {
        let owner_actor_id = self.resolve_actor_id_at(actor.actor_id.0, observed_micros);
        if owner_actor_id == actor.actor_id.0 {
            return actor;
        }
        EntityRef {
            actor_id: ActorId(owner_actor_id),
            entity_uuid: EntityUuid(
                self.entity_uuid_by_actor
                    .get(&owner_actor_id)
                    .copied()
                    .unwrap_or(actor.entity_uuid.0),
            ),
        }
    }

    pub fn actor_for_entity(&self, entity_uuid: i64) -> Option<u64> {
        self.actor_by_entity_uuid.get(&entity_uuid).copied()
    }

    pub fn entity_for_actor(&self, actor_id: u64) -> Option<i64> {
        self.entity_uuid_by_actor.get(&actor_id).copied()
    }

    /// Returns the immediate, packet-proven parent rather than walking to the
    /// top owner. This is used by game-specific formula projectors that need
    /// to validate the exact proxy which supplied an effect.
    pub fn direct_owner_at(&self, child_actor_id: u64, observed_micros: u64) -> Option<EntityRef> {
        let observation = self.observation_at(child_actor_id, observed_micros)?;
        let owner_actor_id = observation.owner_actor_id.or_else(|| {
            observation
                .owner_entity_uuid
                .and_then(|uuid| self.actor_by_entity_uuid.get(&uuid).copied())
        })?;
        let owner_entity_uuid = observation
            .owner_entity_uuid
            .or_else(|| self.entity_uuid_by_actor.get(&owner_actor_id).copied())?;
        Some(EntityRef {
            actor_id: ActorId(owner_actor_id),
            entity_uuid: EntityUuid(owner_entity_uuid),
        })
    }

    pub fn has_direct_owner_evidence_at(
        &self,
        child_actor_id: u64,
        observed_micros: u64,
        evidence: ActorOwnershipEvidence,
    ) -> bool {
        self.observation_at(child_actor_id, observed_micros)
            .is_some_and(|observation| {
                observation.evidence == evidence
                    && (observation.owner_actor_id.is_some()
                        || observation.owner_entity_uuid.is_some())
            })
    }

    pub fn clear(&mut self) {
        self.actor_by_entity_uuid.clear();
        self.entity_uuid_by_actor.clear();
        self.ownership_by_child.clear();
    }

    fn parent_actor_at(&self, child_actor_id: u64, observed_micros: u64) -> Option<u64> {
        let observation = self.observation_at(child_actor_id, observed_micros)?;
        observation.owner_actor_id.or_else(|| {
            observation
                .owner_entity_uuid
                .and_then(|uuid| self.actor_by_entity_uuid.get(&uuid).copied())
        })
    }

    fn observation_at(
        &self,
        child_actor_id: u64,
        observed_micros: u64,
    ) -> Option<&OwnershipObservation> {
        self.ownership_by_child
            .get(&child_actor_id)?
            .iter()
            .rev()
            .find(|observation| observation.observed_micros <= observed_micros)
    }

    fn push_observation(&mut self, child_actor_id: u64, observation: OwnershipObservation) {
        let observations = self.ownership_by_child.entry(child_actor_id).or_default();
        if observations.last().is_some_and(|previous| {
            previous.owner_actor_id == observation.owner_actor_id
                && previous.owner_entity_uuid == observation.owner_entity_uuid
                && previous.evidence == observation.evidence
        }) {
            return;
        }
        observations.push(observation);
    }
}

#[cfg(test)]
mod tests {
    use rlogs_events::{AbilityId, DamageFlags, DamagePacketDetail};

    use super::*;

    fn entity(actor_id: u64, entity_uuid: i64) -> EntityRef {
        EntityRef {
            actor_id: ActorId(actor_id),
            entity_uuid: EntityUuid(entity_uuid),
        }
    }

    #[test]
    fn resolves_multi_level_ownership_without_losing_direct_identity() {
        let mut resolver = ActorAncestryResolver::default();
        resolver.observe_relation(
            10,
            entity(3, 300),
            entity(2, 200),
            ActorOwnershipEvidence::AttributedCombatSource,
        );
        resolver.observe_relation(
            10,
            entity(2, 200),
            entity(1, 100),
            ActorOwnershipEvidence::ConfirmedEntityAttributes,
        );
        assert_eq!(
            resolver.resolve_entity_at(entity(3, 300), 10),
            entity(1, 100)
        );
        assert_eq!(entity(3, 300).entity_uuid.0, 300);
    }

    #[test]
    fn ownership_is_time_aware_and_can_be_cleared() {
        let mut resolver = ActorAncestryResolver::default();
        resolver.observe_relation(
            10,
            entity(2, 200),
            entity(1, 100),
            ActorOwnershipEvidence::AttributedCombatSource,
        );
        resolver.clear_owner(20, entity(2, 200));
        assert_eq!(resolver.resolve_actor_id_at(2, 19), 1);
        assert_eq!(resolver.resolve_actor_id_at(2, 20), 2);
    }

    #[test]
    fn damage_relation_uses_attributed_source_as_owner() {
        let mut resolver = ActorAncestryResolver::default();
        let damage = DamageEvent {
            source: entity(1, 100),
            direct_source: Some(entity(2, 200)),
            target: entity(3, 300),
            ability: Some(AbilityId(7)),
            amount: 10,
            actual_amount: Some(10),
            hp_loss: Some(10),
            shield_loss: None,
            hit_event_id: None,
            damage_source: None,
            damage_type: None,
            flags: DamageFlags::default(),
            packet: DamagePacketDetail::default(),
        };
        resolver.observe_damage(5, &damage);
        assert_eq!(resolver.resolve_actor_id_at(2, 5), 1);
    }

    #[test]
    fn owner_entity_evidence_resolves_when_owner_identity_arrives_later() {
        let mut resolver = ActorAncestryResolver::default();
        resolver.observe_owner_entity(
            5,
            entity(2, 200),
            100,
            ActorOwnershipEvidence::ConfirmedEntityAttributes,
        );
        assert_eq!(resolver.resolve_actor_id_at(2, 5), 2);

        resolver.observe_entity(entity(1, 100));
        assert_eq!(resolver.resolve_actor_id_at(2, 5), 1);
        assert_eq!(resolver.direct_owner_at(2, 5), Some(entity(1, 100)));
        assert!(resolver.has_direct_owner_evidence_at(
            2,
            5,
            ActorOwnershipEvidence::ConfirmedEntityAttributes,
        ));
    }

    #[test]
    fn lifecycle_boundary_closes_active_owners_without_rewriting_history() {
        let mut resolver = ActorAncestryResolver::default();
        resolver.observe_relation(
            10,
            entity(2, 200),
            entity(1, 100),
            ActorOwnershipEvidence::ConfirmedEntityAttributes,
        );
        resolver.observe_relation(
            12,
            entity(4, 400),
            entity(3, 300),
            ActorOwnershipEvidence::AttributedCombatSource,
        );

        resolver.end_active_ownership_intervals(20);

        assert_eq!(
            resolver.resolve_entity_at(entity(2, 200), 19),
            entity(1, 100)
        );
        assert_eq!(
            resolver.resolve_entity_at(entity(4, 400), 19),
            entity(3, 300)
        );
        assert_eq!(
            resolver.resolve_entity_at(entity(2, 200), 20),
            entity(2, 200)
        );
        assert_eq!(
            resolver.resolve_entity_at(entity(4, 400), 20),
            entity(4, 400)
        );

        resolver.observe_relation(
            30,
            entity(2, 250),
            entity(5, 500),
            ActorOwnershipEvidence::ConfirmedEntityAttributes,
        );
        assert_eq!(
            resolver.resolve_entity_at(entity(2, 250), 30),
            entity(5, 500)
        );
        assert_eq!(
            resolver.resolve_entity_at(entity(2, 200), 19),
            entity(1, 100)
        );
    }
}
