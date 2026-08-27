//! Packet-authoritative dungeon recording boundaries.
//!
//! This state machine belongs to the BPSR game integration rather than Core:
//! Core keeps network ingress available, while the game plug-in decides which
//! decoded events are safe and relevant to persist as one dungeon run.

use rlogs_events::{
    CanonicalEvent, DungeonEvent, DungeonEventKind, EventEnvelope, EventTime, RunState,
    TimelineEventKind,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DungeonSegmentStartReason {
    Entered,
    StartedFallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DungeonSegmentEndReason {
    Completed,
    Failed,
    Exited,
    ReplacedByEntry,
    CaptureEnded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DungeonSegmentBoundary {
    pub instance_id: Option<String>,
    pub time: EventTime,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DungeonSegmentAction {
    Open {
        reason: DungeonSegmentStartReason,
        boundary: DungeonSegmentBoundary,
    },
    Record(EventEnvelope),
    Seal {
        reason: DungeonSegmentEndReason,
        boundary: DungeonSegmentBoundary,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveDungeonSegment {
    instance_id: Option<String>,
    scene_id: Option<i32>,
    last_time: EventTime,
    completion_pending: bool,
}

/// Converts canonical BPSR dungeon events into persistence actions.
///
/// Events observed before a dungeon entry are intentionally ignored. An
/// `Entered` packet is the preferred opening boundary; `Started` is a safe
/// fallback for captures that attach after entry. A successful completion is
/// sealed only after the whole decode batch has been recorded, retaining the
/// companion `RunBoundary::Completed` emitted by the same packet.
#[derive(Debug, Default)]
pub struct DungeonRunSegmenter {
    active: Option<ActiveDungeonSegment>,
    pending_world: Option<EventEnvelope>,
    pending_dungeon_identity: Option<EventEnvelope>,
    pending_scene_entry: Option<EventEnvelope>,
    pending_party: Option<EventEnvelope>,
    current_world_scene_id: Option<i32>,
}

impl DungeonRunSegmenter {
    pub fn is_recording(&self) -> bool {
        self.active.is_some()
    }

    pub fn observe_batch(
        &mut self,
        events: impl IntoIterator<Item = EventEnvelope>,
    ) -> Vec<DungeonSegmentAction> {
        let mut actions = Vec::new();
        for event in events {
            let event_time = event.time;
            let next_world_scene_id = match &event.event {
                CanonicalEvent::WorldChanged(world) => world.scene_id.map(|scene| scene.0),
                _ => None,
            };
            let exits_active_scene = self
                .active
                .as_ref()
                .and_then(|active| active.scene_id)
                .zip(next_world_scene_id)
                .is_some_and(|(active_scene_id, next_scene_id)| active_scene_id != next_scene_id);
            if matches!(event.event, CanonicalEvent::WorldChanged(_)) {
                self.current_world_scene_id = next_world_scene_id;
            }
            let terminal = terminal_boundary(&event)
                .or_else(|| exits_active_scene.then_some(DungeonSegmentEndReason::Exited));
            let departed_world = exits_active_scene.then(|| event.clone());
            let dungeon = match &event.event {
                CanonicalEvent::Dungeon(dungeon) => Some(dungeon),
                _ => None,
            };

            if let Some((reason, instance_id)) = dungeon.and_then(opening_boundary) {
                let replaces_active = self.active.as_ref().is_some_and(|active| {
                    distinct_instances(active.instance_id.as_deref(), instance_id.as_deref())
                });
                if replaces_active {
                    self.seal_active(
                        DungeonSegmentEndReason::ReplacedByEntry,
                        event.time,
                        &mut actions,
                    );
                }
                if self.active.is_none() {
                    self.active = Some(ActiveDungeonSegment {
                        instance_id: instance_id.clone(),
                        scene_id: self.current_world_scene_id,
                        last_time: event.time,
                        completion_pending: false,
                    });
                    actions.push(DungeonSegmentAction::Open {
                        reason,
                        boundary: DungeonSegmentBoundary {
                            instance_id,
                            time: event.time,
                        },
                    });
                    actions.extend(
                        self.take_entry_context()
                            .into_iter()
                            .map(DungeonSegmentAction::Record),
                    );
                }
            }

            if let Some(active) = &mut self.active {
                active.last_time = event.time;
                if let Some(dungeon) = dungeon {
                    if active.instance_id.is_none() && dungeon.instance_id.is_some() {
                        active.instance_id.clone_from(&dungeon.instance_id);
                    }
                    if dungeon.kind == DungeonEventKind::Completed {
                        active.completion_pending = true;
                    }
                }
                actions.push(DungeonSegmentAction::Record(event));
                if let Some(reason) = terminal {
                    self.seal_active(reason, event_time, &mut actions);
                }
                if let Some(world) = departed_world {
                    self.remember_entry_context(&world);
                }
            } else {
                self.remember_entry_context(&event);
            }
        }

        if self
            .active
            .as_ref()
            .is_some_and(|active| active.completion_pending)
        {
            let time = self.active.as_ref().expect("checked above").last_time;
            self.seal_active(DungeonSegmentEndReason::Completed, time, &mut actions);
        }
        actions
    }

    /// Finalizes an in-progress run when ingress ends or the game process
    /// exits. This is retained for local history but is not a completed
    /// leaderboard submission.
    pub fn finish(&mut self) -> Option<DungeonSegmentAction> {
        let active = self.active.take()?;
        Some(DungeonSegmentAction::Seal {
            reason: DungeonSegmentEndReason::CaptureEnded,
            boundary: DungeonSegmentBoundary {
                instance_id: active.instance_id,
                time: active.last_time,
            },
        })
    }

    fn seal_active(
        &mut self,
        reason: DungeonSegmentEndReason,
        time: EventTime,
        actions: &mut Vec<DungeonSegmentAction>,
    ) {
        let Some(active) = self.active.take() else {
            return;
        };
        actions.push(DungeonSegmentAction::Seal {
            reason,
            boundary: DungeonSegmentBoundary {
                instance_id: active.instance_id,
                time,
            },
        });
    }

    fn remember_entry_context(&mut self, event: &EventEnvelope) {
        match &event.event {
            CanonicalEvent::WorldChanged(_) => {
                self.pending_world = Some(event.clone());
                self.pending_dungeon_identity = None;
                self.pending_scene_entry = None;
            }
            CanonicalEvent::Dungeon(dungeon)
                if dungeon.kind == DungeonEventKind::FlowUpdated
                    && (dungeon.dungeon_id.is_some()
                        || dungeon.instance_id.is_some()
                        || dungeon.difficulty_id.is_some()) =>
            {
                // BPSR can send the authoritative difficulty/instance delta
                // immediately after the scene change but before the Ready
                // packet that opens the persisted run. Retain that one small
                // identity snapshot so the run does not lose its exact tier.
                self.pending_dungeon_identity = Some(event.clone());
            }
            CanonicalEvent::Timeline(timeline)
                if matches!(
                    timeline.kind,
                    TimelineEventKind::RunBoundary {
                        state: RunState::Entered,
                        ..
                    }
                ) =>
            {
                self.pending_scene_entry = Some(event.clone());
            }
            CanonicalEvent::PartyRosterObserved(_) | CanonicalEvent::PartyChanged { .. } => {
                self.pending_party = Some(event.clone());
            }
            _ => {}
        }
    }

    fn take_entry_context(&mut self) -> Vec<EventEnvelope> {
        let mut context = [
            self.pending_world.take(),
            self.pending_dungeon_identity.take(),
            self.pending_scene_entry.take(),
            self.pending_party.take(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        context.sort_unstable_by_key(|event| event.sequence);
        context
    }
}

fn opening_boundary(dungeon: &DungeonEvent) -> Option<(DungeonSegmentStartReason, Option<String>)> {
    let reason = match dungeon.kind {
        DungeonEventKind::Entered => DungeonSegmentStartReason::Entered,
        DungeonEventKind::Started => DungeonSegmentStartReason::StartedFallback,
        _ => return None,
    };
    Some((reason, dungeon.instance_id.clone()))
}

fn terminal_boundary(event: &EventEnvelope) -> Option<DungeonSegmentEndReason> {
    match &event.event {
        CanonicalEvent::Dungeon(dungeon) => match dungeon.kind {
            DungeonEventKind::Failed => Some(DungeonSegmentEndReason::Failed),
            DungeonEventKind::Exited => Some(DungeonSegmentEndReason::Exited),
            _ => None,
        },
        CanonicalEvent::Timeline(timeline) => match timeline.kind {
            TimelineEventKind::RunBoundary {
                state: RunState::Completed,
                ..
            } => Some(DungeonSegmentEndReason::Completed),
            TimelineEventKind::RunBoundary {
                state: RunState::Failed,
                ..
            } => Some(DungeonSegmentEndReason::Failed),
            TimelineEventKind::RunBoundary {
                state: RunState::Exited,
                ..
            } => Some(DungeonSegmentEndReason::Exited),
            _ => None,
        },
        _ => None,
    }
}

fn distinct_instances(current: Option<&str>, next: Option<&str>) -> bool {
    matches!((current, next), (Some(current), Some(next)) if current != next)
}

#[cfg(test)]
mod tests {
    use rlogs_events::{
        BoundaryReason, CanonicalEventDraft, CanonicalEventDraftKind, DungeonEvent,
        DungeonEventKind, EventEnvelopeFactory, EventProvenance, EventSensitivity, RegionContext,
        RegionIdentity, RunState, SceneId, TimelineEventKind, WorldContext,
    };

    use super::*;

    fn factory() -> EventEnvelopeFactory {
        EventEnvelopeFactory::new(
            "continuous-ingress",
            RegionContext {
                identity: RegionIdentity {
                    deployment_id: "global".into(),
                    region_id: "global".into(),
                    realm_id: None,
                    world_id: None,
                },
                client_build: "fixture".into(),
                protocol_pack_digest: "sha256:fixture".into(),
                evidence: Vec::new(),
            },
        )
    }

    fn dungeon(
        factory: &mut EventEnvelopeFactory,
        sequence: u64,
        kind: DungeonEventKind,
        instance_id: &str,
    ) -> EventEnvelope {
        factory
            .emit(CanonicalEventDraft {
                time: EventTime {
                    observed_micros: sequence * 1_000,
                    game_time_millis: Some(sequence as i64),
                },
                provenance: EventProvenance::wire(sequence, 1, 2),
                sensitivity: EventSensitivity::PublicGameplay,
                kind: CanonicalEventDraftKind::Dungeon(DungeonEvent {
                    kind,
                    dungeon_id: None,
                    instance_id: Some(instance_id.into()),
                    difficulty_id: None,
                    objective_map_key: None,
                    objective_id: None,
                    objective_value: None,
                    objective_complete: None,
                    objective_catalog: None,
                    flow: None,
                }),
            })
            .unwrap()
    }

    fn run_boundary(
        factory: &mut EventEnvelopeFactory,
        sequence: u64,
        state: RunState,
    ) -> EventEnvelope {
        factory
            .emit(CanonicalEventDraft {
                time: EventTime {
                    observed_micros: sequence * 1_000,
                    game_time_millis: Some(sequence as i64),
                },
                provenance: EventProvenance::wire(sequence, 1, 2),
                sensitivity: EventSensitivity::PublicGameplay,
                kind: CanonicalEventDraftKind::Timeline(TimelineEventKind::RunBoundary {
                    state,
                    scene_id: None,
                    reason: BoundaryReason::AuthoritativePacket,
                }),
            })
            .unwrap()
    }

    fn world(factory: &mut EventEnvelopeFactory, sequence: u64, scene_id: i32) -> EventEnvelope {
        factory
            .emit(CanonicalEventDraft {
                time: EventTime {
                    observed_micros: sequence * 1_000,
                    game_time_millis: Some(sequence as i64),
                },
                provenance: EventProvenance::wire(sequence, 1, 2),
                sensitivity: EventSensitivity::PublicGameplay,
                kind: CanonicalEventDraftKind::WorldChanged(WorldContext {
                    scene_id: Some(SceneId(scene_id)),
                    map_id: u32::try_from(scene_id).ok(),
                    line_id: None,
                    scene_instance_id: None,
                    dungeon_instance_id: None,
                }),
            })
            .unwrap()
    }

    #[test]
    fn holds_safe_scene_context_until_authoritative_dungeon_entry() {
        let mut factory = factory();
        let mut segmenter = DungeonRunSegmenter::default();
        let generic_world_entry = run_boundary(&mut factory, 1, RunState::Entered);

        assert!(segmenter.observe_batch([generic_world_entry]).is_empty());
        assert!(!segmenter.is_recording());

        let entry = dungeon(&mut factory, 2, DungeonEventKind::Entered, "run-1");
        let actions = segmenter.observe_batch([entry]);
        assert_eq!(actions.len(), 3);
        assert!(matches!(
            &actions[0],
            DungeonSegmentAction::Open {
                reason: DungeonSegmentStartReason::Entered,
                ..
            }
        ));
        assert!(matches!(
            &actions[1],
            DungeonSegmentAction::Record(EventEnvelope {
                event: CanonicalEvent::Timeline(_),
                ..
            })
        ));
        assert!(matches!(&actions[2], DungeonSegmentAction::Record(_)));
        assert!(segmenter.is_recording());
    }

    #[test]
    fn carries_pre_entry_difficulty_delta_into_the_persisted_run() {
        let mut factory = factory();
        let mut segmenter = DungeonRunSegmenter::default();
        assert!(
            segmenter
                .observe_batch([world(&mut factory, 1, 6_525)])
                .is_empty()
        );

        let mut identity = dungeon(&mut factory, 2, DungeonEventKind::FlowUpdated, "run-1");
        if let CanonicalEvent::Dungeon(dungeon) = &mut identity.event {
            dungeon.difficulty_id = Some(5);
        }
        assert!(segmenter.observe_batch([identity]).is_empty());

        let entry = dungeon(&mut factory, 3, DungeonEventKind::Entered, "run-1");
        let actions = segmenter.observe_batch([entry]);
        assert!(actions.iter().any(|action| matches!(
            action,
            DungeonSegmentAction::Record(EventEnvelope {
                event: CanonicalEvent::Dungeon(DungeonEvent {
                    kind: DungeonEventKind::FlowUpdated,
                    difficulty_id: Some(5),
                    ..
                }),
                ..
            })
        )));
    }

    #[test]
    fn successful_packet_batch_records_both_completion_events_before_sealing() {
        let mut factory = factory();
        let mut segmenter = DungeonRunSegmenter::default();
        let entry = dungeon(&mut factory, 1, DungeonEventKind::Entered, "run-1");
        segmenter.observe_batch([entry]);

        let completed = dungeon(&mut factory, 2, DungeonEventKind::Completed, "run-1");
        let boundary = run_boundary(&mut factory, 2, RunState::Completed);
        let actions = segmenter.observe_batch([completed, boundary]);

        assert_eq!(actions.len(), 3);
        assert!(matches!(actions[0], DungeonSegmentAction::Record(_)));
        assert!(matches!(actions[1], DungeonSegmentAction::Record(_)));
        assert!(matches!(
            actions[2],
            DungeonSegmentAction::Seal {
                reason: DungeonSegmentEndReason::Completed,
                ..
            }
        ));
        assert!(!segmenter.is_recording());
    }

    #[test]
    fn completion_event_alone_still_seals_at_end_of_decode_batch() {
        let mut factory = factory();
        let mut segmenter = DungeonRunSegmenter::default();
        segmenter.observe_batch([dungeon(&mut factory, 1, DungeonEventKind::Entered, "run-1")]);

        let actions = segmenter.observe_batch([dungeon(
            &mut factory,
            2,
            DungeonEventKind::Completed,
            "run-1",
        )]);

        assert!(matches!(
            actions.last(),
            Some(DungeonSegmentAction::Seal {
                reason: DungeonSegmentEndReason::Completed,
                ..
            })
        ));
        assert!(!segmenter.is_recording());
    }

    #[test]
    fn started_is_a_safe_fallback_when_capture_attaches_after_entry() {
        let mut factory = factory();
        let mut segmenter = DungeonRunSegmenter::default();
        let started = dungeon(&mut factory, 1, DungeonEventKind::Started, "run-1");
        let actions = segmenter.observe_batch([started]);

        assert!(matches!(
            actions.first(),
            Some(DungeonSegmentAction::Open {
                reason: DungeonSegmentStartReason::StartedFallback,
                ..
            })
        ));
    }

    #[test]
    fn a_new_instance_closes_an_abandoned_run_before_opening_the_next() {
        let mut factory = factory();
        let mut segmenter = DungeonRunSegmenter::default();
        segmenter.observe_batch([dungeon(&mut factory, 1, DungeonEventKind::Entered, "run-1")]);

        let actions =
            segmenter.observe_batch([dungeon(&mut factory, 2, DungeonEventKind::Entered, "run-2")]);

        assert!(matches!(
            actions.as_slice(),
            [
                DungeonSegmentAction::Seal {
                    reason: DungeonSegmentEndReason::ReplacedByEntry,
                    ..
                },
                DungeonSegmentAction::Open { .. },
                DungeonSegmentAction::Record(_)
            ]
        ));
    }

    #[test]
    fn leaving_the_dungeon_scene_seals_an_exited_run_immediately() {
        let mut factory = factory();
        let mut segmenter = DungeonRunSegmenter::default();
        assert!(
            segmenter
                .observe_batch([world(&mut factory, 1, 6_525)])
                .is_empty()
        );
        segmenter.observe_batch([dungeon(&mut factory, 2, DungeonEventKind::Entered, "run-1")]);

        let actions = segmenter.observe_batch([world(&mut factory, 3, 8)]);

        assert!(matches!(
            actions.as_slice(),
            [
                DungeonSegmentAction::Record(EventEnvelope {
                    event: CanonicalEvent::WorldChanged(_),
                    ..
                }),
                DungeonSegmentAction::Seal {
                    reason: DungeonSegmentEndReason::Exited,
                    ..
                }
            ]
        ));
        assert!(!segmenter.is_recording());

        let next =
            segmenter.observe_batch([dungeon(&mut factory, 4, DungeonEventKind::Entered, "run-2")]);
        assert!(next.iter().any(|action| matches!(
            action,
            DungeonSegmentAction::Record(EventEnvelope {
                event: CanonicalEvent::WorldChanged(WorldContext {
                    scene_id: Some(SceneId(8)),
                    ..
                }),
                ..
            })
        )));
    }

    #[test]
    fn capture_end_marks_an_open_segment_incomplete() {
        let mut factory = factory();
        let mut segmenter = DungeonRunSegmenter::default();
        segmenter.observe_batch([dungeon(&mut factory, 1, DungeonEventKind::Entered, "run-1")]);

        assert!(matches!(
            segmenter.finish(),
            Some(DungeonSegmentAction::Seal {
                reason: DungeonSegmentEndReason::CaptureEnded,
                ..
            })
        ));
    }
}
