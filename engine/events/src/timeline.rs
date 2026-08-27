use thiserror::Error;

use crate::{TimelineEvent, TimelineEventDraft};

/// The append-only event history for one dungeon run.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RunTimeline {
    events: Vec<TimelineEvent>,
}

impl RunTimeline {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn events(&self) -> &[TimelineEvent] {
        &self.events
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn push(&mut self, draft: TimelineEventDraft) -> Result<u64, TimelineError> {
        if let Some(previous) = self.events.last() {
            if draft.time.observed_micros < previous.time.observed_micros {
                return Err(TimelineError::ObservedTimeMovedBackward {
                    previous_micros: previous.time.observed_micros,
                    next_micros: draft.time.observed_micros,
                });
            }
        }

        let sequence = self.events.len() as u64 + 1;
        self.events.push(TimelineEvent {
            sequence,
            time: draft.time,
            provenance: draft.provenance,
            kind: draft.kind,
        });
        Ok(sequence)
    }

    pub fn validate(&self) -> Result<(), TimelineError> {
        let mut previous_time = None;

        for (index, event) in self.events.iter().enumerate() {
            let expected_sequence = index as u64 + 1;
            if event.sequence != expected_sequence {
                return Err(TimelineError::InvalidSequence {
                    expected: expected_sequence,
                    actual: event.sequence,
                });
            }

            if let Some(previous_micros) = previous_time {
                if event.time.observed_micros < previous_micros {
                    return Err(TimelineError::ObservedTimeMovedBackward {
                        previous_micros,
                        next_micros: event.time.observed_micros,
                    });
                }
            }

            previous_time = Some(event.time.observed_micros);
        }

        Ok(())
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TimelineError {
    #[error("observed event time moved backward from {previous_micros}us to {next_micros}us")]
    ObservedTimeMovedBackward {
        previous_micros: u64,
        next_micros: u64,
    },

    #[error("timeline sequence should be {expected}, but was {actual}")]
    InvalidSequence { expected: u64, actual: u64 },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AbilityId, ActorId, BoundaryReason, CombatState, DamageEvent, DamageFlags, EntityRef,
        EntityUuid, EventProvenance, EventTime, LifeState, TimelineEventKind,
    };

    fn draft(observed_micros: u64, kind: TimelineEventKind) -> TimelineEventDraft {
        TimelineEventDraft {
            time: EventTime {
                observed_micros,
                game_time_millis: None,
            },
            provenance: EventProvenance::wire(7, 1, 1),
            kind,
        }
    }

    #[test]
    fn sequences_are_stable_and_begin_at_one() {
        let mut timeline = RunTimeline::new();

        let first = timeline
            .push(draft(
                100,
                TimelineEventKind::CombatBoundary {
                    state: CombatState::Started,
                    reason: BoundaryReason::HostileAction,
                },
            ))
            .unwrap();
        let second = timeline
            .push(draft(
                100,
                TimelineEventKind::Life {
                    actor: EntityRef {
                        actor_id: ActorId(2),
                        entity_uuid: EntityUuid(200),
                    },
                    state: LifeState::Died,
                },
            ))
            .unwrap();

        assert_eq!(first, 1);
        assert_eq!(second, 2);
        assert_eq!(timeline.events()[0].sequence, 1);
        assert_eq!(timeline.events()[1].sequence, 2);
        assert_eq!(timeline.validate(), Ok(()));
    }

    #[test]
    fn backward_time_is_rejected_without_mutating_the_timeline() {
        let mut timeline = RunTimeline::new();
        timeline
            .push(draft(
                200,
                TimelineEventKind::CombatBoundary {
                    state: CombatState::Started,
                    reason: BoundaryReason::HostileAction,
                },
            ))
            .unwrap();

        let result = timeline.push(draft(
            199,
            TimelineEventKind::CombatBoundary {
                state: CombatState::Ended,
                reason: BoundaryReason::InactivityFallback,
            },
        ));

        assert_eq!(
            result,
            Err(TimelineError::ObservedTimeMovedBackward {
                previous_micros: 200,
                next_micros: 199,
            })
        );
        assert_eq!(timeline.len(), 1);
    }

    #[test]
    fn a_minimal_combat_sequence_is_representable() {
        let mut timeline = RunTimeline::new();
        let player = EntityRef {
            actor_id: ActorId(1),
            entity_uuid: EntityUuid(100),
        };
        let boss = EntityRef {
            actor_id: ActorId(2),
            entity_uuid: EntityUuid(200),
        };

        timeline
            .push(draft(
                1_000,
                TimelineEventKind::CombatBoundary {
                    state: CombatState::Started,
                    reason: BoundaryReason::HostileAction,
                },
            ))
            .unwrap();
        timeline
            .push(draft(
                1_100,
                TimelineEventKind::Damage(DamageEvent {
                    source: player,
                    direct_source: None,
                    target: boss,
                    ability: Some(AbilityId(55)),
                    amount: 12_345,
                    actual_amount: None,
                    hp_loss: Some(12_345),
                    shield_loss: None,
                    hit_event_id: None,
                    damage_source: None,
                    damage_type: None,
                    flags: DamageFlags {
                        critical: Some(true),
                        ..DamageFlags::default()
                    },
                    packet: Default::default(),
                }),
            ))
            .unwrap();
        timeline
            .push(draft(
                1_200,
                TimelineEventKind::Life {
                    actor: boss,
                    state: LifeState::Died,
                },
            ))
            .unwrap();
        timeline
            .push(draft(
                1_300,
                TimelineEventKind::CombatBoundary {
                    state: CombatState::Ended,
                    reason: BoundaryReason::ActorLifecycle,
                },
            ))
            .unwrap();

        assert_eq!(timeline.len(), 4);
        assert_eq!(timeline.validate(), Ok(()));
    }

    #[test]
    fn events_round_trip_through_json() {
        let event = TimelineEvent {
            sequence: 1,
            time: EventTime {
                observed_micros: 42,
                game_time_millis: Some(9001),
            },
            provenance: EventProvenance::wire(4, 8, 9),
            kind: TimelineEventKind::Life {
                actor: EntityRef {
                    actor_id: ActorId(3),
                    entity_uuid: EntityUuid(300),
                },
                state: LifeState::Revived,
            },
        };

        let json = serde_json::to_string(&event).unwrap();
        let decoded: TimelineEvent = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, event);
    }
}
