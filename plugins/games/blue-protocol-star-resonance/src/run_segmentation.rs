//! Packet-authoritative dungeon recording boundaries.
//!
//! This state machine belongs to the BPSR game integration rather than Core:
//! Core keeps network ingress available, while the game plug-in decides which
//! decoded events are safe and relevant to persist as one dungeon run.

use std::collections::BTreeMap;

use rlogs_events::{
    BoundaryReason, CanonicalEvent, DungeonEvent, DungeonEventKind, EventEnvelope, EventTime,
    RunState, TimelineEvent, TimelineEventKind,
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
    SceneDeparted,
    ReplacedByEntry,
    CaptureEnded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DungeonSegmentBoundary {
    pub instance_id: Option<String>,
    pub time: EventTime,
}

/// Recorded envelopes dominate this stream and remain inline to avoid a heap
/// allocation on every retained event.
#[allow(clippy::large_enum_variant)]
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

#[derive(Debug, Clone, PartialEq)]
struct ActiveDungeonSegment {
    instance_id: Option<String>,
    scene_id: Option<i32>,
    last_time: EventTime,
    completion_pending: bool,
    inferred_completion_source: Option<EventEnvelope>,
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
    pending_profiles: BTreeMap<String, EventEnvelope>,
    current_world_scene_id: Option<i32>,
}

const MAX_PENDING_PROFILE_CONTEXTS: usize = 64;

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
            self.remember_profile_context(&event);
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
                .or_else(|| exits_active_scene.then_some(DungeonSegmentEndReason::SceneDeparted));
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
                        inferred_completion_source: None,
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
                    } else if dungeon.kind == DungeonEventKind::ObjectiveUpdated
                        && is_field_of_forgotten_illusions_boss_objective(
                            active.scene_id,
                            dungeon_objective_id(dungeon),
                        )
                    {
                        if dungeon.objective_complete == Some(true) {
                            active.inferred_completion_source = Some(event.clone());
                        } else if dungeon.objective_complete == Some(false) {
                            // In Gauntlet mode the next boss objective follows
                            // in a later packet. That successor cancels the
                            // prior boss's pending run completion while leaving
                            // its encounter clear.
                            active.inferred_completion_source = None;
                        }
                    }
                }
                let terminal_reason = terminal.map(|reason| {
                    if self.active.as_ref().is_some_and(|active| {
                        active.completion_pending || active.inferred_completion_source.is_some()
                    }) {
                        DungeonSegmentEndReason::Completed
                    } else {
                        reason
                    }
                });
                if terminal_reason == Some(DungeonSegmentEndReason::Completed)
                    && let Some(source) = self
                        .active
                        .as_mut()
                        .and_then(|active| active.inferred_completion_source.take())
                {
                    // The objective packet proves completion, while the
                    // terminal packet supplies the monotonic timestamp at
                    // which the segment can safely be sealed. Keeping the
                    // boundary before the scene/exit event makes reducers
                    // close this run as completed rather than departed.
                    actions.push(DungeonSegmentAction::Record(inferred_completion_boundary(
                        source, event_time,
                    )));
                }
                actions.push(DungeonSegmentAction::Record(event));
                if let Some(reason) = terminal_reason {
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

    fn remember_profile_context(&mut self, event: &EventEnvelope) {
        let CanonicalEvent::CharacterProfileObserved { profile } = &event.event else {
            return;
        };
        let character_id = profile.character.character_id.clone();
        if character_id.is_empty() {
            return;
        }
        if self.pending_profiles.len() >= MAX_PENDING_PROFILE_CONTEXTS
            && !self.pending_profiles.contains_key(&character_id)
            && let Some(oldest_character_id) = self
                .pending_profiles
                .iter()
                .min_by_key(|(_, envelope)| envelope.sequence)
                .map(|(character_id, _)| character_id.clone())
        {
            self.pending_profiles.remove(&oldest_character_id);
        }
        self.pending_profiles.insert(character_id, event.clone());
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
        context.extend(self.pending_profiles.values().cloned());
        context.sort_unstable_by_key(|event| event.sequence);
        context
    }
}

fn dungeon_objective_id(dungeon: &DungeonEvent) -> Option<i64> {
    dungeon
        .objective_id
        .or_else(|| dungeon.objective_map_key.map(i64::from))
}

fn is_field_of_forgotten_illusions_boss_objective(
    scene_id: Option<i32>,
    objective_id: Option<i64>,
) -> bool {
    matches!(scene_id, Some(13_021..=13_023)) && matches!(objective_id, Some(1_302_101..=1_302_104))
}

fn inferred_completion_boundary(
    mut source: EventEnvelope,
    terminal_time: EventTime,
) -> EventEnvelope {
    source.time = terminal_time;
    source.event = CanonicalEvent::Timeline(TimelineEvent {
        sequence: 0,
        time: terminal_time,
        provenance: source.provenance.clone(),
        kind: TimelineEventKind::RunBoundary {
            state: RunState::Completed,
            scene_id: None,
            reason: BoundaryReason::AuthoritativePacket,
        },
    });
    source
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
        BoundaryReason, CanonicalEventDraft, CanonicalEventDraftKind, CharacterIdentity,
        DungeonEvent, DungeonEventKind, EventEnvelopeFactory, EventProvenance, EventSensitivity,
        GameProfileEvent, RegionContext, RegionIdentity, RunState, SceneId, TimelineEventKind,
        WorldContext,
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

    fn objective(
        factory: &mut EventEnvelopeFactory,
        sequence: u64,
        objective_id: i64,
        complete: bool,
    ) -> EventEnvelope {
        let mut event = dungeon(
            factory,
            sequence,
            DungeonEventKind::ObjectiveUpdated,
            "run-1",
        );
        let CanonicalEvent::Dungeon(dungeon) = &mut event.event else {
            unreachable!("helper always creates a dungeon event")
        };
        dungeon.objective_map_key = i32::try_from(objective_id).ok();
        dungeon.objective_id = Some(objective_id);
        dungeon.objective_complete = Some(complete);
        event
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

    fn profile(
        factory: &mut EventEnvelopeFactory,
        sequence: u64,
        character_id: &str,
    ) -> EventEnvelope {
        factory
            .emit(CanonicalEventDraft {
                time: EventTime {
                    observed_micros: sequence * 1_000,
                    game_time_millis: Some(sequence as i64),
                },
                provenance: EventProvenance::wire(sequence, 1664308034, 21),
                sensitivity: EventSensitivity::PersonalGameplay,
                kind: CanonicalEventDraftKind::CharacterProfileObserved {
                    profile: Box::new(GameProfileEvent {
                        game_plugin_id: "blue-protocol-star-resonance".into(),
                        payload_schema_id: "bpsr-character-profile".into(),
                        payload_schema_version: 1,
                        character: CharacterIdentity {
                            region: factory.region().identity.clone(),
                            character_id: character_id.into(),
                        },
                        payload: serde_json::json!({"season_cultivation": [{"season_id": 3}]}),
                    }),
                },
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
    fn carries_latest_pre_entry_profile_into_each_persisted_run_without_synthesis() {
        let mut factory = factory();
        let mut segmenter = DungeonRunSegmenter::default();
        let observed_profile = profile(&mut factory, 1, "3296036");
        let original_time = observed_profile.time;
        let original_provenance = observed_profile.provenance.clone();
        assert!(segmenter.observe_batch([observed_profile]).is_empty());

        let entry = dungeon(&mut factory, 2, DungeonEventKind::Entered, "run-1");
        let actions = segmenter.observe_batch([entry]);
        let carried = actions
            .iter()
            .find_map(|action| match action {
                DungeonSegmentAction::Record(envelope)
                    if matches!(
                        envelope.event,
                        CanonicalEvent::CharacterProfileObserved { .. }
                    ) =>
                {
                    Some(envelope)
                }
                _ => None,
            })
            .expect("carried profile context");
        assert_eq!(carried.time, original_time);
        assert_eq!(carried.provenance, original_provenance);

        let failed = dungeon(&mut factory, 3, DungeonEventKind::Failed, "run-1");
        assert!(
            segmenter
                .observe_batch([failed])
                .iter()
                .any(|action| matches!(action, DungeonSegmentAction::Seal { .. }))
        );
        let next_entry = dungeon(&mut factory, 4, DungeonEventKind::Entered, "run-2");
        let next_actions = segmenter.observe_batch([next_entry]);
        assert!(next_actions.iter().any(|action| matches!(
            action,
            DungeonSegmentAction::Record(EventEnvelope {
                event: CanonicalEvent::CharacterProfileObserved { .. },
                ..
            })
        )));
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
    fn nightmare_boss_objective_completes_when_the_run_departs_without_a_successor() {
        let mut factory = factory();
        let mut segmenter = DungeonRunSegmenter::default();
        segmenter.observe_batch([world(&mut factory, 1, 13_023)]);
        segmenter.observe_batch([dungeon(&mut factory, 2, DungeonEventKind::Entered, "run-1")]);

        let objective_actions =
            segmenter.observe_batch([objective(&mut factory, 3, 1_302_101, true)]);
        assert_eq!(objective_actions.len(), 1);
        assert!(segmenter.is_recording());

        let post_completion = profile(&mut factory, 4, "1000001");
        let post_completion_time = post_completion.time;
        assert_eq!(segmenter.observe_batch([post_completion]).len(), 1);

        let departure = world(&mut factory, 5, 8);
        let departure_time = departure.time;
        let actions = segmenter.observe_batch([departure]);

        assert!(matches!(
            actions.as_slice(),
            [
                DungeonSegmentAction::Record(EventEnvelope {
                    event: CanonicalEvent::Timeline(TimelineEvent {
                        kind: TimelineEventKind::RunBoundary {
                            state: RunState::Completed,
                            reason: BoundaryReason::AuthoritativePacket,
                            ..
                        },
                        ..
                    }),
                    ..
                }),
                DungeonSegmentAction::Record(EventEnvelope {
                    event: CanonicalEvent::WorldChanged(_),
                    ..
                }),
                DungeonSegmentAction::Seal {
                    reason: DungeonSegmentEndReason::Completed,
                    ..
                }
            ]
        ));
        let DungeonSegmentAction::Record(boundary) = &actions[0] else {
            unreachable!("the first action is the inferred completion boundary")
        };
        assert!(boundary.time.observed_micros >= post_completion_time.observed_micros);
        assert_eq!(boundary.time, departure_time);
        assert!(!segmenter.is_recording());
    }

    #[test]
    fn gauntlet_successor_objective_keeps_the_same_run_open() {
        let mut factory = factory();
        let mut segmenter = DungeonRunSegmenter::default();
        segmenter.observe_batch([world(&mut factory, 1, 13_023)]);
        segmenter.observe_batch([dungeon(&mut factory, 2, DungeonEventKind::Entered, "run-1")]);

        let completed_origin = objective(&mut factory, 3, 1_302_101, true);
        let opened_continuation = objective(&mut factory, 4, 1_302_103, false);
        let actions = segmenter.observe_batch([completed_origin, opened_continuation]);

        assert_eq!(
            actions
                .iter()
                .filter(|action| matches!(action, DungeonSegmentAction::Record(_)))
                .count(),
            2
        );
        assert!(
            !actions
                .iter()
                .any(|action| matches!(action, DungeonSegmentAction::Seal { .. }))
        );
        assert!(segmenter.is_recording());
    }

    #[test]
    fn lobby_selection_objective_never_completes_a_raid() {
        let mut factory = factory();
        let mut segmenter = DungeonRunSegmenter::default();
        segmenter.observe_batch([world(&mut factory, 1, 13_023)]);
        segmenter.observe_batch([dungeon(&mut factory, 2, DungeonEventKind::Entered, "run-1")]);

        let actions = segmenter.observe_batch([objective(&mut factory, 3, 1_301_101, true)]);
        assert!(
            !actions
                .iter()
                .any(|action| matches!(action, DungeonSegmentAction::Seal { .. }))
        );
        assert!(segmenter.is_recording());
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
    fn leaving_the_dungeon_scene_seals_without_claiming_an_explicit_exit() {
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
                    reason: DungeonSegmentEndReason::SceneDeparted,
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
