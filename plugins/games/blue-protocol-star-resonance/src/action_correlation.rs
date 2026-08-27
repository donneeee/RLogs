use std::collections::{BTreeMap, VecDeque};

use rlogs_events::{CastEvent, CastState, DamageEvent};
use serde::Serialize;

use crate::{
    ClientSkillStageEndSnapshot, ClientSkillStageTriggerSnapshot, ServerSkillStageEndSnapshot,
};

pub const ACTION_CORRELATION_SCHEMA_VERSION: u16 = 1;

/// Bounded, replay-facing evidence collector for the BPSR action UUID chain.
///
/// This collector deliberately does not turn an equal integer found in
/// `SkillEffect.uuid` into an authorized action-to-damage relationship. That
/// field's semantics must first be proven by an exact-build replay. Until then
/// every equality is retained as a candidate and every missing/mismatched row
/// remains visible in the report.
#[derive(Debug)]
pub struct ActionCorrelationAudit {
    maximum_pending_actions: usize,
    pending: BTreeMap<ActionKey, PendingAction>,
    insertion_order: VecDeque<(ActionKey, u64)>,
    actions_observed: u64,
    actions_evicted: u64,
    client_stage_triggers_linked: u64,
    client_stage_triggers_unmatched: u64,
    client_stage_ends_linked: u64,
    client_stage_ends_unmatched: u64,
    server_stage_ends_linked: u64,
    server_stage_ends_unmatched: u64,
    damage_rows_observed: u64,
    damage_rows_without_packet_candidate_id: u64,
    damage_rows_with_candidate_id: u64,
    damage_candidate_id_matches: u64,
    damage_candidate_id_mismatches: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ActionKey {
    actor_id: u64,
    action_instance_id: i64,
}

#[derive(Debug, Clone, Copy)]
struct PendingAction {
    observed_micros: u64,
    base_ability_id: i64,
    target_actor_id: Option<u64>,
    target_entity_uuid: Option<i64>,
    client_stage_trigger_count: u32,
    client_stage_end_count: u32,
    server_stage_end_count: u32,
    damage_candidate_match_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActionCorrelationReport {
    pub schema_version: u16,
    pub maximum_pending_actions: usize,
    pub pending_action_count: usize,
    pub actions_observed: u64,
    pub actions_evicted: u64,
    pub client_stage_triggers_linked: u64,
    pub client_stage_triggers_unmatched: u64,
    pub client_stage_ends_linked: u64,
    pub client_stage_ends_unmatched: u64,
    pub server_stage_ends_linked: u64,
    pub server_stage_ends_unmatched: u64,
    pub damage_rows_observed: u64,
    pub damage_rows_without_packet_candidate_id: u64,
    pub damage_rows_with_candidate_id: u64,
    pub damage_candidate_id_matches: u64,
    pub damage_candidate_id_mismatches: u64,
    /// Remains zero until an exact-build replay independently proves that the
    /// candidate damage field is the same action UUID namespace.
    pub authorized_action_damage_links: u64,
    pub pending_actions: Vec<PendingActionEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PendingActionEvidence {
    pub actor_id: u64,
    pub action_instance_id: i64,
    pub observed_micros: u64,
    pub base_ability_id: i64,
    pub target_actor_id: Option<u64>,
    pub target_entity_uuid: Option<i64>,
    pub client_stage_trigger_count: u32,
    pub client_stage_end_count: u32,
    pub server_stage_end_count: u32,
    pub damage_candidate_match_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageCorrelationCandidate {
    MissingPacketCandidateId,
    CandidateIdDidNotMatch,
    CandidateIdMatchedUnproven,
}

impl ActionCorrelationAudit {
    pub fn new(maximum_pending_actions: usize) -> Self {
        assert!(maximum_pending_actions > 0);
        Self {
            maximum_pending_actions,
            pending: BTreeMap::new(),
            insertion_order: VecDeque::new(),
            actions_observed: 0,
            actions_evicted: 0,
            client_stage_triggers_linked: 0,
            client_stage_triggers_unmatched: 0,
            client_stage_ends_linked: 0,
            client_stage_ends_unmatched: 0,
            server_stage_ends_linked: 0,
            server_stage_ends_unmatched: 0,
            damage_rows_observed: 0,
            damage_rows_without_packet_candidate_id: 0,
            damage_rows_with_candidate_id: 0,
            damage_candidate_id_matches: 0,
            damage_candidate_id_mismatches: 0,
        }
    }

    pub fn observe_action(&mut self, observed_micros: u64, cast: &CastEvent) -> bool {
        if cast.state != CastState::Started {
            return false;
        }
        let Some(timing) = cast.action_timing else {
            return false;
        };
        let key = ActionKey {
            actor_id: cast.source.actor_id.0,
            action_instance_id: timing.action_instance_id,
        };
        let pending = PendingAction {
            observed_micros,
            base_ability_id: timing.base_ability.0,
            target_actor_id: cast.target.map(|target| target.actor_id.0),
            target_entity_uuid: cast.target.map(|target| target.entity_uuid.0),
            client_stage_trigger_count: 0,
            client_stage_end_count: 0,
            server_stage_end_count: 0,
            damage_candidate_match_count: 0,
        };
        self.actions_observed = self.actions_observed.saturating_add(1);
        self.pending.insert(key, pending);
        self.insertion_order.push_back((key, observed_micros));
        self.enforce_bound();
        true
    }

    pub fn observe_client_stage_trigger(
        &mut self,
        actor_id: u64,
        stage: ClientSkillStageTriggerSnapshot,
    ) -> bool {
        let linked = self
            .pending
            .get_mut(&ActionKey {
                actor_id,
                action_instance_id: i64::from(stage.skill_uuid),
            })
            .map(|action| {
                action.client_stage_trigger_count =
                    action.client_stage_trigger_count.saturating_add(1);
            })
            .is_some();
        if linked {
            self.client_stage_triggers_linked = self.client_stage_triggers_linked.saturating_add(1);
        } else {
            self.client_stage_triggers_unmatched =
                self.client_stage_triggers_unmatched.saturating_add(1);
        }
        linked
    }

    pub fn observe_client_stage_end(
        &mut self,
        actor_id: u64,
        stage: ClientSkillStageEndSnapshot,
    ) -> bool {
        let linked = self
            .pending
            .get_mut(&ActionKey {
                actor_id,
                action_instance_id: i64::from(stage.skill_uuid),
            })
            .map(|action| {
                action.client_stage_end_count = action.client_stage_end_count.saturating_add(1);
            })
            .is_some();
        if linked {
            self.client_stage_ends_linked = self.client_stage_ends_linked.saturating_add(1);
        } else {
            self.client_stage_ends_unmatched = self.client_stage_ends_unmatched.saturating_add(1);
        }
        linked
    }

    pub fn observe_server_stage_end(
        &mut self,
        actor_id: u64,
        stage: ServerSkillStageEndSnapshot,
    ) -> bool {
        let linked = self
            .pending
            .get_mut(&ActionKey {
                actor_id,
                action_instance_id: i64::from(stage.skill_uuid),
            })
            .map(|action| {
                action.server_stage_end_count = action.server_stage_end_count.saturating_add(1);
            })
            .is_some();
        if linked {
            self.server_stage_ends_linked = self.server_stage_ends_linked.saturating_add(1);
        } else {
            self.server_stage_ends_unmatched = self.server_stage_ends_unmatched.saturating_add(1);
        }
        linked
    }

    pub fn observe_damage(&mut self, damage: &DamageEvent) -> DamageCorrelationCandidate {
        self.damage_rows_observed = self.damage_rows_observed.saturating_add(1);
        let Some(candidate_id) = damage.packet.skill_effect_uuid.filter(|value| *value > 0) else {
            self.damage_rows_without_packet_candidate_id = self
                .damage_rows_without_packet_candidate_id
                .saturating_add(1);
            return DamageCorrelationCandidate::MissingPacketCandidateId;
        };
        self.damage_rows_with_candidate_id = self.damage_rows_with_candidate_id.saturating_add(1);
        let key = ActionKey {
            actor_id: damage.source.actor_id.0,
            action_instance_id: candidate_id,
        };
        if let Some(action) = self.pending.get_mut(&key) {
            action.damage_candidate_match_count =
                action.damage_candidate_match_count.saturating_add(1);
            self.damage_candidate_id_matches = self.damage_candidate_id_matches.saturating_add(1);
            DamageCorrelationCandidate::CandidateIdMatchedUnproven
        } else {
            self.damage_candidate_id_mismatches =
                self.damage_candidate_id_mismatches.saturating_add(1);
            DamageCorrelationCandidate::CandidateIdDidNotMatch
        }
    }

    pub fn report(&self) -> ActionCorrelationReport {
        ActionCorrelationReport {
            schema_version: ACTION_CORRELATION_SCHEMA_VERSION,
            maximum_pending_actions: self.maximum_pending_actions,
            pending_action_count: self.pending.len(),
            actions_observed: self.actions_observed,
            actions_evicted: self.actions_evicted,
            client_stage_triggers_linked: self.client_stage_triggers_linked,
            client_stage_triggers_unmatched: self.client_stage_triggers_unmatched,
            client_stage_ends_linked: self.client_stage_ends_linked,
            client_stage_ends_unmatched: self.client_stage_ends_unmatched,
            server_stage_ends_linked: self.server_stage_ends_linked,
            server_stage_ends_unmatched: self.server_stage_ends_unmatched,
            damage_rows_observed: self.damage_rows_observed,
            damage_rows_without_packet_candidate_id: self.damage_rows_without_packet_candidate_id,
            damage_rows_with_candidate_id: self.damage_rows_with_candidate_id,
            damage_candidate_id_matches: self.damage_candidate_id_matches,
            damage_candidate_id_mismatches: self.damage_candidate_id_mismatches,
            authorized_action_damage_links: 0,
            pending_actions: self
                .pending
                .iter()
                .map(|(key, value)| PendingActionEvidence {
                    actor_id: key.actor_id,
                    action_instance_id: key.action_instance_id,
                    observed_micros: value.observed_micros,
                    base_ability_id: value.base_ability_id,
                    target_actor_id: value.target_actor_id,
                    target_entity_uuid: value.target_entity_uuid,
                    client_stage_trigger_count: value.client_stage_trigger_count,
                    client_stage_end_count: value.client_stage_end_count,
                    server_stage_end_count: value.server_stage_end_count,
                    damage_candidate_match_count: value.damage_candidate_match_count,
                })
                .collect(),
        }
    }

    fn enforce_bound(&mut self) {
        while self.pending.len() > self.maximum_pending_actions {
            let Some((key, observed_micros)) = self.insertion_order.pop_front() else {
                break;
            };
            if self
                .pending
                .get(&key)
                .is_some_and(|pending| pending.observed_micros == observed_micros)
            {
                self.pending.remove(&key);
                self.actions_evicted = self.actions_evicted.saturating_add(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use rlogs_events::{
        AbilityId, ActionTimingSnapshot, ActorId, DamageFlags, DamagePacketDetail, EntityRef,
        EntityUuid,
    };

    use super::*;

    fn entity(actor_id: u64, entity_uuid: i64) -> EntityRef {
        EntityRef {
            actor_id: ActorId(actor_id),
            entity_uuid: EntityUuid(entity_uuid),
        }
    }

    fn action(actor_id: u64, action_instance_id: i64) -> CastEvent {
        CastEvent {
            source: entity(actor_id, actor_id as i64 + 10_000),
            ability: AbilityId(2233),
            target: Some(entity(90, 9000)),
            state: CastState::Started,
            action_timing: Some(ActionTimingSnapshot {
                action_instance_id,
                base_ability: AbilityId(2233),
                ability_level: 4,
                slot_id: 2,
                client_timestamp_raw: 100,
                begin_time_raw: 90,
                attack_speed_basis_points: 10_500,
                cast_speed_basis_points: 10_400,
                charge_speed_basis_points: 10_300,
                passive: false,
                activated_roulette: false,
                target_part_id: 0,
            }),
        }
    }

    fn damage(actor_id: u64, candidate_id: Option<i64>) -> DamageEvent {
        DamageEvent {
            source: entity(actor_id, actor_id as i64 + 10_000),
            direct_source: None,
            target: entity(90, 9000),
            ability: Some(AbilityId(2233)),
            amount: 1234,
            actual_amount: None,
            hp_loss: None,
            shield_loss: None,
            hit_event_id: None,
            damage_source: None,
            damage_type: None,
            flags: DamageFlags::default(),
            packet: DamagePacketDetail {
                skill_effect_uuid: candidate_id,
                ..DamagePacketDetail::default()
            },
        }
    }

    #[test]
    fn exact_actor_and_action_uuid_link_all_stage_messages() {
        let mut audit = ActionCorrelationAudit::new(8);
        assert!(audit.observe_action(10, &action(7, 321)));
        assert!(audit.observe_client_stage_trigger(
            7,
            ClientSkillStageTriggerSnapshot {
                trigger_type: 1,
                time: 11,
                skill_uuid: 321,
            }
        ));
        assert!(audit.observe_client_stage_end(
            7,
            ClientSkillStageEndSnapshot {
                current_stage_index: 0,
                next_stage_index: 1,
                time: 12,
                condition_id: 0,
                skill_uuid: 321,
                trigger_index: 0,
            }
        ));
        assert!(audit.observe_server_stage_end(
            7,
            ServerSkillStageEndSnapshot {
                skill_uuid: 321,
                stage_id: 1,
                new_stage_id: 2,
                condition_id: 0,
            }
        ));
        let report = audit.report();
        assert_eq!(report.client_stage_triggers_linked, 1);
        assert_eq!(report.client_stage_ends_linked, 1);
        assert_eq!(report.server_stage_ends_linked, 1);
        assert_eq!(report.pending_actions[0].server_stage_end_count, 1);
    }

    #[test]
    fn actor_mismatch_never_links_an_equal_action_uuid() {
        let mut audit = ActionCorrelationAudit::new(8);
        assert!(audit.observe_action(10, &action(7, 321)));
        assert!(!audit.observe_server_stage_end(
            8,
            ServerSkillStageEndSnapshot {
                skill_uuid: 321,
                stage_id: 1,
                new_stage_id: 2,
                condition_id: 0,
            }
        ));
        assert_eq!(audit.report().server_stage_ends_unmatched, 1);
    }

    #[test]
    fn equal_damage_candidate_is_retained_but_never_authorized() {
        let mut audit = ActionCorrelationAudit::new(8);
        assert!(audit.observe_action(10, &action(7, 321)));
        assert_eq!(
            audit.observe_damage(&damage(7, Some(321))),
            DamageCorrelationCandidate::CandidateIdMatchedUnproven
        );
        let report = audit.report();
        assert_eq!(report.damage_candidate_id_matches, 1);
        assert_eq!(report.authorized_action_damage_links, 0);
    }

    #[test]
    fn missing_and_mismatched_damage_ids_remain_visible() {
        let mut audit = ActionCorrelationAudit::new(8);
        assert!(audit.observe_action(10, &action(7, 321)));
        assert_eq!(
            audit.observe_damage(&damage(7, None)),
            DamageCorrelationCandidate::MissingPacketCandidateId
        );
        assert_eq!(
            audit.observe_damage(&damage(7, Some(999))),
            DamageCorrelationCandidate::CandidateIdDidNotMatch
        );
        let report = audit.report();
        assert_eq!(report.damage_rows_without_packet_candidate_id, 1);
        assert_eq!(report.damage_candidate_id_mismatches, 1);
    }

    #[test]
    fn pending_state_is_strictly_bounded() {
        let mut audit = ActionCorrelationAudit::new(2);
        assert!(audit.observe_action(10, &action(7, 1)));
        assert!(audit.observe_action(20, &action(7, 2)));
        assert!(audit.observe_action(30, &action(7, 3)));
        let report = audit.report();
        assert_eq!(report.pending_action_count, 2);
        assert_eq!(report.actions_evicted, 1);
        assert_eq!(report.pending_actions[0].action_instance_id, 2);
    }
}
