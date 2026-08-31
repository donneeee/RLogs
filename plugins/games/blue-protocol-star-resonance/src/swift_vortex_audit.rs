use std::collections::{BTreeMap, BTreeSet, HashMap};

use rlogs_events::{
    CanonicalEvent, EntityAttribute, EntityAttributeUpdateKind, EntityAttributeValue,
    EventEnvelope, StatusEvent, StatusState, TimelineEventKind,
};
use serde::{Deserialize, Serialize};

use crate::{
    decode_known_entity_attribute_value, state_damage_contribution_formula_target_matches,
};

pub const SWIFT_VORTEX_EFFECT_ID: i64 = 2_110_060;
pub const SWIFT_VORTEX_EXPECTED_DURATION_MILLIS: u64 = 10_000;
pub const SWIFT_VORTEX_AUDIT_SCHEMA_VERSION: u16 = 1;

const HASTE_ATTRIBUTE_ID: i32 = 11_930;
const NORMAL_ACTION_SPEED_ATTRIBUTE_ID: i32 = 11_720;
const GUIDE_ACTION_SPEED_ATTRIBUTE_ID: i32 = 11_730;
const MAXIMUM_RECEIPTS: usize = 256;
const MINIMUM_CONSENSUS_RECEIPTS: usize = 4;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwiftVortexAppliedMagnitude {
    pub haste_basis_points: i64,
    pub normal_action_speed_basis_points: i64,
    pub guide_action_speed_basis_points: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwiftVortexMagnitudeReceipt {
    pub provider_actor_id: u64,
    pub provider_entity_uuid: i64,
    pub recipient_actor_id: u64,
    pub recipient_entity_uuid: i64,
    pub status_instance_id: i64,
    pub application_event_sequence: u64,
    pub application_attribute_event_sequence: u64,
    pub removal_event_sequence: u64,
    pub removal_attribute_event_sequence: u64,
    pub applied: SwiftVortexAppliedMagnitude,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwiftVortexCandidateAuditReport {
    pub schema_version: u16,
    pub effect_id: i64,
    pub candidate_status_event_count: u64,
    pub exact_application_transition_count: u64,
    pub exact_paired_receipt_count: usize,
    pub distinct_provider_entity_count: usize,
    pub distinct_recipient_entity_count: usize,
    pub incomplete_application_count: usize,
    pub incomplete_removal_count: usize,
    pub identity_mismatch_event_count: u64,
    pub blockers: BTreeMap<String, u64>,
    pub magnitude_consensus: Option<SwiftVortexAppliedMagnitude>,
    /// This gate means the sealed observations are numerous and internally
    /// consistent enough for formula review. It never enables attribution.
    pub magnitude_gate_satisfied: bool,
    pub production_attribution_enabled: bool,
    pub receipts: Vec<SwiftVortexMagnitudeReceipt>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SpeedState {
    haste: Option<i64>,
    normal: Option<i64>,
    guide: Option<i64>,
}

impl SpeedState {
    fn complete(self) -> Option<[i64; 3]> {
        Some([self.haste?, self.normal?, self.guide?])
    }

    fn apply(&mut self, attributes: &[EntityAttribute]) -> [bool; 3] {
        let mut touched = [false; 3];
        for attribute in attributes {
            let Some(value) = integer_attribute(attribute) else {
                continue;
            };
            match attribute.attribute_id {
                HASTE_ATTRIBUTE_ID => {
                    self.haste = Some(value);
                    touched[0] = true;
                }
                NORMAL_ACTION_SPEED_ATTRIBUTE_ID => {
                    self.normal = Some(value);
                    touched[1] = true;
                }
                GUIDE_ACTION_SPEED_ATTRIBUTE_ID => {
                    self.guide = Some(value);
                    touched[2] = true;
                }
                _ => {}
            }
        }
        touched
    }
}

#[derive(Debug, Clone)]
struct PendingApplication {
    provider_actor_id: u64,
    provider_entity_uuid: i64,
    recipient_actor_id: u64,
    recipient_entity_uuid: i64,
    instance_id: i64,
    event_sequence: u64,
    baseline: [i64; 3],
    confounded: bool,
}

#[derive(Debug, Clone)]
struct ActiveCandidate {
    pending: PendingApplication,
    attribute_event_sequence: u64,
    magnitude: SwiftVortexAppliedMagnitude,
}

#[derive(Debug, Clone)]
struct PendingRemoval {
    active: ActiveCandidate,
    removal_event_sequence: u64,
    baseline: [i64; 3],
    confounded: bool,
}

#[derive(Debug, Default)]
pub struct SwiftVortexCandidateAuditAnalyzer {
    states: HashMap<i64, SpeedState>,
    pending_applications: HashMap<i64, PendingApplication>,
    active: HashMap<(i64, i64), ActiveCandidate>,
    pending_removals: HashMap<i64, PendingRemoval>,
    candidate_status_event_count: u64,
    exact_application_transition_count: u64,
    identity_mismatch_event_count: u64,
    blockers: BTreeMap<String, u64>,
    receipts: Vec<SwiftVortexMagnitudeReceipt>,
}

impl SwiftVortexCandidateAuditAnalyzer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn observe(&mut self, envelope: &EventEnvelope) {
        let identity_matches = state_damage_contribution_formula_target_matches(
            &envelope.region.identity.deployment_id,
            &envelope.region.client_build,
            &envelope.region.protocol_pack_digest,
        )
        .unwrap_or(false);
        if !identity_matches {
            self.identity_mismatch_event_count =
                self.identity_mismatch_event_count.saturating_add(1);
            return;
        }
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            return;
        };
        match &timeline.kind {
            TimelineEventKind::Status(status) => self.observe_status(envelope.sequence, status),
            TimelineEventKind::UnresolvedStatus(status) => {
                self.mark_target_confounded(status.target.entity_uuid.0)
            }
            TimelineEventKind::EntityAttributes(attributes) => self.observe_attributes(
                envelope.sequence,
                attributes.actor.actor_id.0,
                attributes.actor.entity_uuid.0,
                attributes.update_kind,
                &attributes.attributes,
            ),
            TimelineEventKind::RunBoundary { .. }
            | TimelineEventKind::EncounterBoundary { .. }
            | TimelineEventKind::DataGap(_) => self.clear_unfinished("lifecycle_boundary"),
            _ => {}
        }
    }

    pub fn report(&self) -> SwiftVortexCandidateAuditReport {
        let magnitude_consensus = consensus(&self.receipts);
        let providers = self
            .receipts
            .iter()
            .map(|receipt| receipt.provider_entity_uuid)
            .collect::<BTreeSet<_>>();
        let recipients = self
            .receipts
            .iter()
            .map(|receipt| receipt.recipient_entity_uuid)
            .collect::<BTreeSet<_>>();
        let magnitude_gate_satisfied = self.receipts.len() >= MINIMUM_CONSENSUS_RECEIPTS
            && providers.len() >= 2
            && recipients.len() >= 2
            && magnitude_consensus.is_some();
        SwiftVortexCandidateAuditReport {
            schema_version: SWIFT_VORTEX_AUDIT_SCHEMA_VERSION,
            effect_id: SWIFT_VORTEX_EFFECT_ID,
            candidate_status_event_count: self.candidate_status_event_count,
            exact_application_transition_count: self.exact_application_transition_count,
            exact_paired_receipt_count: self.receipts.len(),
            distinct_provider_entity_count: providers.len(),
            distinct_recipient_entity_count: recipients.len(),
            incomplete_application_count: self.pending_applications.len(),
            incomplete_removal_count: self.pending_removals.len(),
            identity_mismatch_event_count: self.identity_mismatch_event_count,
            blockers: self.blockers.clone(),
            magnitude_consensus,
            magnitude_gate_satisfied,
            production_attribution_enabled: false,
            receipts: self.receipts.clone(),
        }
    }

    fn observe_status(&mut self, sequence: u64, status: &StatusEvent) {
        let recipient_entity_uuid = status.target.entity_uuid.0;
        if status.effect.0 != SWIFT_VORTEX_EFFECT_ID {
            self.mark_target_confounded(recipient_entity_uuid);
            return;
        }
        self.candidate_status_event_count = self.candidate_status_event_count.saturating_add(1);
        match status.state {
            StatusState::Applied | StatusState::Stacked => {
                let (Some(provider), Some(instance_id)) =
                    (status.source, status.instance_id.map(|value| value.0))
                else {
                    self.block("application_identity_missing");
                    return;
                };
                if status.stacks != Some(1)
                    || status.duration_millis != Some(SWIFT_VORTEX_EXPECTED_DURATION_MILLIS)
                    || provider.entity_uuid.0 == 0
                    || recipient_entity_uuid == 0
                {
                    self.block("application_shape_mismatch");
                    return;
                }
                let Some(baseline) = self
                    .states
                    .get(&recipient_entity_uuid)
                    .copied()
                    .and_then(SpeedState::complete)
                else {
                    self.block("application_baseline_missing");
                    return;
                };
                if self
                    .pending_applications
                    .contains_key(&recipient_entity_uuid)
                    || self.pending_removals.contains_key(&recipient_entity_uuid)
                    || self
                        .active
                        .keys()
                        .any(|(target, _)| *target == recipient_entity_uuid)
                {
                    self.block("overlapping_swift_vortex_lifecycle");
                    self.pending_applications.remove(&recipient_entity_uuid);
                    return;
                }
                self.pending_applications.insert(
                    recipient_entity_uuid,
                    PendingApplication {
                        provider_actor_id: provider.actor_id.0,
                        provider_entity_uuid: provider.entity_uuid.0,
                        recipient_actor_id: status.target.actor_id.0,
                        recipient_entity_uuid,
                        instance_id,
                        event_sequence: sequence,
                        baseline,
                        confounded: false,
                    },
                );
            }
            StatusState::Refreshed => {
                if !self.active.contains_key(&(
                    recipient_entity_uuid,
                    status.instance_id.map(|value| value.0).unwrap_or_default(),
                )) {
                    self.block("refresh_without_validated_application");
                }
            }
            StatusState::Removed | StatusState::Consumed => {
                let Some(instance_id) = status.instance_id.map(|value| value.0) else {
                    self.block("removal_instance_missing");
                    return;
                };
                let Some(active) = self.active.remove(&(recipient_entity_uuid, instance_id)) else {
                    self.block("removal_without_validated_application");
                    return;
                };
                if status.source.is_some_and(|provider| {
                    provider.entity_uuid.0 != active.pending.provider_entity_uuid
                }) {
                    self.block("removal_provider_mismatch");
                    return;
                }
                let Some(baseline) = self
                    .states
                    .get(&recipient_entity_uuid)
                    .copied()
                    .and_then(SpeedState::complete)
                else {
                    self.block("removal_baseline_missing");
                    return;
                };
                self.pending_removals.insert(
                    recipient_entity_uuid,
                    PendingRemoval {
                        active,
                        removal_event_sequence: sequence,
                        baseline,
                        confounded: false,
                    },
                );
            }
        }
    }

    fn observe_attributes(
        &mut self,
        sequence: u64,
        actor_id: u64,
        entity_uuid: i64,
        update_kind: EntityAttributeUpdateKind,
        attributes: &[EntityAttribute],
    ) {
        if entity_uuid == 0 {
            return;
        }
        let mut next = if update_kind == EntityAttributeUpdateKind::Snapshot {
            SpeedState::default()
        } else {
            self.states.get(&entity_uuid).copied().unwrap_or_default()
        };
        let touched = next.apply(attributes);
        self.states.insert(entity_uuid, next);

        if let Some(pending) = self.pending_applications.remove(&entity_uuid) {
            if pending.confounded {
                self.block("application_intervening_status");
                return;
            }
            if update_kind != EntityAttributeUpdateKind::Delta || touched != [true, true, true] {
                self.block("application_transition_incomplete");
                return;
            }
            if actor_id != pending.recipient_actor_id {
                self.block("application_recipient_actor_mismatch");
                return;
            }
            let Some(current) = next.complete() else {
                self.block("application_transition_incomplete");
                return;
            };
            let Some(magnitude) = positive_magnitude(pending.baseline, current) else {
                self.block("application_transition_not_positive");
                return;
            };
            self.exact_application_transition_count =
                self.exact_application_transition_count.saturating_add(1);
            self.active.insert(
                (entity_uuid, pending.instance_id),
                ActiveCandidate {
                    pending,
                    attribute_event_sequence: sequence,
                    magnitude,
                },
            );
            return;
        }

        if let Some(pending) = self.pending_removals.remove(&entity_uuid) {
            if pending.confounded {
                self.block("removal_intervening_status");
                return;
            }
            if update_kind != EntityAttributeUpdateKind::Delta || touched != [true, true, true] {
                self.block("removal_transition_incomplete");
                return;
            }
            let Some(current) = next.complete() else {
                self.block("removal_transition_incomplete");
                return;
            };
            if !removal_matches(pending.baseline, current, pending.active.magnitude) {
                self.block("removal_transition_asymmetric");
                return;
            }
            if self.receipts.len() >= MAXIMUM_RECEIPTS {
                self.block("receipt_limit_reached");
                return;
            }
            self.receipts.push(SwiftVortexMagnitudeReceipt {
                provider_actor_id: pending.active.pending.provider_actor_id,
                provider_entity_uuid: pending.active.pending.provider_entity_uuid,
                recipient_actor_id: pending.active.pending.recipient_actor_id,
                recipient_entity_uuid: pending.active.pending.recipient_entity_uuid,
                status_instance_id: pending.active.pending.instance_id,
                application_event_sequence: pending.active.pending.event_sequence,
                application_attribute_event_sequence: pending.active.attribute_event_sequence,
                removal_event_sequence: pending.removal_event_sequence,
                removal_attribute_event_sequence: sequence,
                applied: pending.active.magnitude,
            });
        }
    }

    fn mark_target_confounded(&mut self, target_entity_uuid: i64) {
        if let Some(pending) = self.pending_applications.get_mut(&target_entity_uuid) {
            pending.confounded = true;
        }
        if let Some(pending) = self.pending_removals.get_mut(&target_entity_uuid) {
            pending.confounded = true;
        }
    }

    fn clear_unfinished(&mut self, blocker: &str) {
        let count =
            self.pending_applications.len() + self.pending_removals.len() + self.active.len();
        if count > 0 {
            *self.blockers.entry(blocker.to_owned()).or_default() += count as u64;
        }
        self.pending_applications.clear();
        self.pending_removals.clear();
        self.active.clear();
    }

    fn block(&mut self, blocker: &str) {
        *self.blockers.entry(blocker.to_owned()).or_default() += 1;
    }
}

fn integer_attribute(attribute: &EntityAttribute) -> Option<i64> {
    if let Some(EntityAttributeValue::Integer(value)) = attribute.decoded {
        return Some(value);
    }
    match decode_known_entity_attribute_value(attribute.attribute_id, &attribute.raw_value) {
        Some(EntityAttributeValue::Integer(value)) => Some(value),
        _ => None,
    }
}

fn positive_magnitude(
    previous: [i64; 3],
    current: [i64; 3],
) -> Option<SwiftVortexAppliedMagnitude> {
    let haste = current[0].checked_sub(previous[0])?;
    let normal = current[1].checked_sub(previous[1])?;
    let guide = current[2].checked_sub(previous[2])?;
    (haste > 0 && normal >= 0 && guide >= 0 && (normal > 0 || guide > 0)).then_some(
        SwiftVortexAppliedMagnitude {
            haste_basis_points: haste,
            normal_action_speed_basis_points: normal,
            guide_action_speed_basis_points: guide,
        },
    )
}

fn removal_matches(
    previous: [i64; 3],
    current: [i64; 3],
    magnitude: SwiftVortexAppliedMagnitude,
) -> bool {
    [
        previous[0].checked_sub(current[0]),
        previous[1].checked_sub(current[1]),
        previous[2].checked_sub(current[2]),
    ] == [
        Some(magnitude.haste_basis_points),
        Some(magnitude.normal_action_speed_basis_points),
        Some(magnitude.guide_action_speed_basis_points),
    ]
}

fn consensus(receipts: &[SwiftVortexMagnitudeReceipt]) -> Option<SwiftVortexAppliedMagnitude> {
    let first = receipts.first()?.applied;
    receipts
        .iter()
        .all(|receipt| receipt.applied == first)
        .then_some(first)
}

#[cfg(test)]
mod tests {
    use rlogs_events::{
        ActorId, EntityAttributeEvent, EntityRef, EntityUuid, EventProvenance, EventSensitivity,
        EventTime, RegionContext, RegionIdentity, StatusEffectId, StatusEffectInstanceId,
        TimelineEvent,
    };

    use super::*;
    use crate::{
        state_damage_contribution_deployment_id, state_damage_contribution_game_build,
        state_damage_contribution_protocol_pack_digest,
    };

    fn entity(actor_id: u64, entity_uuid: i64) -> EntityRef {
        EntityRef {
            actor_id: ActorId(actor_id),
            entity_uuid: EntityUuid(entity_uuid),
        }
    }

    fn envelope(sequence: u64, kind: TimelineEventKind) -> EventEnvelope {
        let time = EventTime {
            observed_micros: sequence * 1_000,
            game_time_millis: Some(sequence as i64),
        };
        let provenance = EventProvenance::wire(sequence, 1, 1);
        EventEnvelope {
            schema_version: rlogs_events::EVENT_SCHEMA_VERSION,
            session_id: "swift-vortex-audit-test".into(),
            sequence,
            region: RegionContext {
                identity: RegionIdentity {
                    deployment_id: state_damage_contribution_deployment_id().unwrap().into(),
                    region_id: "global".into(),
                    realm_id: None,
                    world_id: None,
                },
                client_build: state_damage_contribution_game_build().unwrap().into(),
                protocol_pack_digest: state_damage_contribution_protocol_pack_digest()
                    .unwrap()
                    .into(),
                evidence: vec![],
            },
            time,
            provenance: provenance.clone(),
            sensitivity: EventSensitivity::PersonalGameplay,
            event: CanonicalEvent::Timeline(TimelineEvent {
                sequence,
                time,
                provenance,
                kind,
            }),
        }
    }

    fn attributes(sequence: u64, actor: EntityRef, values: [i64; 3]) -> EventEnvelope {
        envelope(
            sequence,
            TimelineEventKind::EntityAttributes(EntityAttributeEvent {
                actor,
                update_kind: if sequence == 1 {
                    EntityAttributeUpdateKind::Snapshot
                } else {
                    EntityAttributeUpdateKind::Delta
                },
                ownership: None,
                attributes: [
                    (HASTE_ATTRIBUTE_ID, values[0]),
                    (NORMAL_ACTION_SPEED_ATTRIBUTE_ID, values[1]),
                    (GUIDE_ACTION_SPEED_ATTRIBUTE_ID, values[2]),
                ]
                .into_iter()
                .map(|(attribute_id, value)| EntityAttribute {
                    attribute_id,
                    raw_value: vec![],
                    decoded: Some(EntityAttributeValue::Integer(value)),
                })
                .collect(),
            }),
        )
    }

    fn status(
        sequence: u64,
        provider: EntityRef,
        recipient: EntityRef,
        state: StatusState,
    ) -> EventEnvelope {
        envelope(
            sequence,
            TimelineEventKind::Status(StatusEvent {
                source: Some(provider),
                target: recipient,
                effect: StatusEffectId(SWIFT_VORTEX_EFFECT_ID),
                instance_id: Some(StatusEffectInstanceId(700)),
                origin: None,
                state,
                stacks: Some(1),
                duration_millis: Some(SWIFT_VORTEX_EXPECTED_DURATION_MILLIS),
                level: Some(60),
                part_id: None,
                count: Some(1),
                created_at_millis: None,
            }),
        )
    }

    #[test]
    fn paired_application_and_removal_produce_exact_audit_receipt_only() {
        let provider = entity(2, 20);
        let recipient = entity(4, 40);
        let mut analyzer = SwiftVortexCandidateAuditAnalyzer::new();
        for event in [
            attributes(1, recipient, [2_000, 1_000, 1_500]),
            status(2, provider, recipient, StatusState::Applied),
            attributes(3, recipient, [2_500, 1_300, 2_100]),
            status(4, provider, recipient, StatusState::Removed),
            attributes(5, recipient, [2_000, 1_000, 1_500]),
        ] {
            analyzer.observe(&event);
        }
        let report = analyzer.report();
        assert_eq!(report.exact_application_transition_count, 1);
        assert_eq!(report.exact_paired_receipt_count, 1);
        assert_eq!(
            report.magnitude_consensus,
            Some(SwiftVortexAppliedMagnitude {
                haste_basis_points: 500,
                normal_action_speed_basis_points: 300,
                guide_action_speed_basis_points: 600,
            })
        );
        assert!(!report.magnitude_gate_satisfied);
        assert!(!report.production_attribution_enabled);
    }

    #[test]
    fn intervening_status_rejects_causal_transition() {
        let provider = entity(2, 20);
        let recipient = entity(4, 40);
        let mut analyzer = SwiftVortexCandidateAuditAnalyzer::new();
        analyzer.observe(&attributes(1, recipient, [2_000, 1_000, 1_500]));
        analyzer.observe(&status(2, provider, recipient, StatusState::Applied));
        let mut confounder = status(3, provider, recipient, StatusState::Applied);
        let CanonicalEvent::Timeline(timeline) = &mut confounder.event else {
            unreachable!();
        };
        let TimelineEventKind::Status(status) = &mut timeline.kind else {
            unreachable!();
        };
        status.effect = StatusEffectId(99_999);
        analyzer.observe(&confounder);
        analyzer.observe(&attributes(4, recipient, [2_500, 1_300, 2_100]));
        let report = analyzer.report();
        assert_eq!(report.exact_paired_receipt_count, 0);
        assert_eq!(
            report.blockers.get("application_intervening_status"),
            Some(&1)
        );
    }
}
