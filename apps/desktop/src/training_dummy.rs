use std::collections::{HashMap, HashSet};

use rlogs_events::{
    ActorId, ActorKind, CanonicalEvent, EventEnvelope, StatusEffectId, StatusState,
    TimelineEventKind,
};
use serde::Serialize;

pub const GUILD_HALL_SCENE_ID: i32 = 12_000;
pub const TRAINING_DURATION_MICROS: u64 = 180_000_000;
const ELITE_DUMMY_MONSTER_IDS: [i64; 2] = [115, 122];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrainingDummyPhase {
    #[default]
    Idle,
    Armed,
    Running,
    Finished,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainingDummyState {
    pub phase: TrainingDummyPhase,
    pub duration_micros: u64,
    pub scene_id: Option<i32>,
    pub player_actor_id: Option<String>,
    pub player_character_id: Option<String>,
    pub player_name: Option<String>,
    pub class_id: Option<i32>,
    pub specialization_id: Option<i32>,
    pub observed_party_size: Option<usize>,
    pub target_actor_id: Option<String>,
    pub target_monster_id: Option<i64>,
    pub started_micros: Option<u64>,
    pub ended_micros: Option<u64>,
    pub elapsed_micros: u64,
    pub remaining_micros: u64,
    pub total_damage: i64,
    pub dps: f64,
    pub valid: bool,
    pub invalid_reason: Option<String>,
}

impl TrainingDummyState {
    fn idle(scene_id: Option<i32>) -> Self {
        Self {
            phase: TrainingDummyPhase::Idle,
            duration_micros: TRAINING_DURATION_MICROS,
            scene_id,
            remaining_micros: TRAINING_DURATION_MICROS,
            ..Self::default()
        }
    }

    fn armed(scene_id: Option<i32>) -> Self {
        Self {
            phase: TrainingDummyPhase::Armed,
            duration_micros: TRAINING_DURATION_MICROS,
            scene_id,
            remaining_micros: TRAINING_DURATION_MICROS,
            valid: true,
            ..Self::default()
        }
    }
}

impl Default for TrainingDummyState {
    fn default() -> Self {
        Self {
            phase: TrainingDummyPhase::Idle,
            duration_micros: TRAINING_DURATION_MICROS,
            scene_id: None,
            player_actor_id: None,
            player_character_id: None,
            player_name: None,
            class_id: None,
            specialization_id: None,
            observed_party_size: None,
            target_actor_id: None,
            target_monster_id: None,
            started_micros: None,
            ended_micros: None,
            elapsed_micros: 0,
            remaining_micros: TRAINING_DURATION_MICROS,
            total_damage: 0,
            dps: 0.0,
            valid: false,
            invalid_reason: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TrainingDummyObservation {
    pub reset_meter: bool,
    pub accept_damage: bool,
    pub changed: bool,
}

#[derive(Debug, Clone)]
struct ActorEvidence {
    kind: ActorKind,
    monster_id: Option<i64>,
    character_id: Option<String>,
    display_name: Option<String>,
    class_id: Option<i32>,
    specialization_id: Option<i32>,
}

#[derive(Debug)]
pub struct TrainingDummyController {
    state: TrainingDummyState,
    actors: HashMap<ActorId, ActorEvidence>,
    active_statuses: HashSet<(ActorId, ActorId, StatusEffectId)>,
    observed_party_size: Option<usize>,
}

impl Default for TrainingDummyController {
    fn default() -> Self {
        Self {
            state: TrainingDummyState::idle(None),
            actors: HashMap::new(),
            active_statuses: HashSet::new(),
            observed_party_size: None,
        }
    }
}

impl TrainingDummyController {
    pub fn state(&self) -> TrainingDummyState {
        self.state.clone()
    }

    pub fn arm(&mut self) {
        self.state = TrainingDummyState::armed(self.state.scene_id);
        self.state.observed_party_size = self.observed_party_size;
        self.active_statuses.clear();
        if self.observed_party_size.is_some_and(|size| size > 1) {
            self.invalidate("party_not_solo", 0);
        }
    }

    pub fn disarm(&mut self) {
        self.state = TrainingDummyState::idle(self.state.scene_id);
        self.active_statuses.clear();
    }

    pub fn observe(&mut self, event: &EventEnvelope) -> TrainingDummyObservation {
        let mut observation = TrainingDummyObservation {
            accept_damage: self.state.phase == TrainingDummyPhase::Idle,
            ..TrainingDummyObservation::default()
        };

        if let CanonicalEvent::WorldChanged(world) = &event.event {
            let scene_id = world.scene_id.map(|scene| scene.0);
            if self.state.scene_id != scene_id {
                self.state.scene_id = scene_id;
                observation.changed = true;
            }
            if self.state.phase != TrainingDummyPhase::Idle && scene_id != Some(GUILD_HALL_SCENE_ID)
            {
                self.invalidate("left_guild_hall", event.time.observed_micros);
                observation.changed = true;
            }
            return observation;
        }

        let CanonicalEvent::Timeline(timeline) = &event.event else {
            let party_size = match &event.event {
                CanonicalEvent::PartyRosterObserved(roster) => match &roster.observation {
                    rlogs_events::PartyRosterObservation::FullSnapshot { members, .. } => {
                        Some(members.len())
                    }
                    rlogs_events::PartyRosterObservation::Dissolved => Some(0),
                    _ => None,
                },
                CanonicalEvent::PartyChanged { members } => Some(members.len()),
                _ => None,
            };
            if let Some(party_size) = party_size {
                self.observed_party_size = Some(party_size);
                self.state.observed_party_size = Some(party_size);
                observation.changed = true;
                if party_size > 1
                    && matches!(
                        self.state.phase,
                        TrainingDummyPhase::Armed | TrainingDummyPhase::Running
                    )
                {
                    self.invalidate("party_not_solo", event.time.observed_micros);
                }
            }
            return observation;
        };
        match &timeline.kind {
            TimelineEventKind::Actor(actor) => {
                self.actors.insert(
                    actor.actor.actor_id,
                    ActorEvidence {
                        kind: actor.kind,
                        monster_id: actor.monster_id.map(|id| id.0),
                        character_id: actor.character_id.clone(),
                        display_name: actor.display_name.clone(),
                        class_id: actor.class_id,
                        specialization_id: actor.specialization_id,
                    },
                );
            }
            TimelineEventKind::Status(status) => {
                if let Some(source) = status.source {
                    let key = (source.actor_id, status.target.actor_id, status.effect);
                    match status.state {
                        StatusState::Applied | StatusState::Refreshed | StatusState::Stacked => {
                            self.active_statuses.insert(key);
                        }
                        StatusState::Consumed | StatusState::Removed => {
                            self.active_statuses.remove(&key);
                        }
                    }
                    let external_player_effect = self.state.phase == TrainingDummyPhase::Running
                        && self.actor_is_player(source.actor_id)
                        && Some(source.actor_id.0.to_string()) != self.state.player_actor_id
                        && (Some(status.target.actor_id.0.to_string())
                            == self.state.player_actor_id
                            || Some(status.target.actor_id.0.to_string())
                                == self.state.target_actor_id);
                    if external_player_effect
                        && matches!(
                            status.state,
                            StatusState::Applied | StatusState::Refreshed | StatusState::Stacked
                        )
                    {
                        self.invalidate("external_player_effect", event.time.observed_micros);
                        observation.changed = true;
                    }
                }
            }
            TimelineEventKind::Damage(damage) if damage.amount > 0 => {
                observation.accept_damage = false;
                match self.state.phase {
                    TrainingDummyPhase::Idle => observation.accept_damage = true,
                    TrainingDummyPhase::Armed => {
                        if self.state.scene_id != Some(GUILD_HALL_SCENE_ID) {
                            return observation;
                        }
                        let Some(target) = self.actors.get(&damage.target.actor_id) else {
                            return observation;
                        };
                        if !matches!(target.kind, ActorKind::Monster | ActorKind::TrainingDummy)
                            || !target
                                .monster_id
                                .is_some_and(|id| ELITE_DUMMY_MONSTER_IDS.contains(&id))
                            || !self.actor_is_player(damage.source.actor_id)
                        {
                            return observation;
                        }
                        let monster_id = target.monster_id.expect("validated dummy monster id");
                        self.start(
                            damage.source.actor_id,
                            damage.target.actor_id,
                            monster_id,
                            event.time.observed_micros,
                        );
                        observation.reset_meter = true;
                        observation.accept_damage = self.state.phase == TrainingDummyPhase::Running;
                        observation.changed = true;
                        self.add_damage(damage.amount);
                    }
                    TrainingDummyPhase::Running => {
                        let now = event.time.observed_micros;
                        if self.deadline_reached(now) {
                            self.finish();
                            observation.changed = true;
                        } else if Some(damage.source.actor_id.0.to_string())
                            != self.state.player_actor_id
                        {
                            self.invalidate("multiple_players", now);
                            observation.changed = true;
                        } else if Some(damage.target.actor_id.0.to_string())
                            != self.state.target_actor_id
                        {
                            self.invalidate("multiple_targets", now);
                            observation.changed = true;
                        } else {
                            observation.accept_damage = true;
                            observation.changed = true;
                            self.add_damage(damage.amount);
                            self.advance(now);
                        }
                    }
                    TrainingDummyPhase::Finished | TrainingDummyPhase::Invalid => {}
                }
            }
            _ => {
                if self.state.phase == TrainingDummyPhase::Running
                    && self.deadline_reached(event.time.observed_micros)
                {
                    self.finish();
                    observation.changed = true;
                }
            }
        }
        observation
    }

    fn actor_is_player(&self, actor_id: ActorId) -> bool {
        self.actors
            .get(&actor_id)
            .is_some_and(|actor| actor.kind == ActorKind::Player)
    }

    fn start(&mut self, player: ActorId, target: ActorId, monster_id: i64, now: u64) {
        let player_evidence = self.actors.get(&player).cloned();
        self.state.phase = TrainingDummyPhase::Running;
        self.state.player_actor_id = Some(player.0.to_string());
        self.state.player_character_id = player_evidence
            .as_ref()
            .and_then(|actor| actor.character_id.clone());
        self.state.player_name = player_evidence
            .as_ref()
            .and_then(|actor| actor.display_name.clone());
        self.state.class_id = player_evidence.as_ref().and_then(|actor| actor.class_id);
        self.state.specialization_id = player_evidence
            .as_ref()
            .and_then(|actor| actor.specialization_id);
        self.state.target_actor_id = Some(target.0.to_string());
        self.state.target_monster_id = Some(monster_id);
        self.state.started_micros = Some(now);
        self.state.ended_micros = None;
        self.state.elapsed_micros = 0;
        self.state.remaining_micros = TRAINING_DURATION_MICROS;
        self.state.total_damage = 0;
        self.state.dps = 0.0;
        self.state.valid = true;
        self.state.invalid_reason = None;

        let externally_buffed = self.active_statuses.iter().any(|(source, recipient, _)| {
            (*recipient == player || *recipient == target)
                && *source != player
                && self.actor_is_player(*source)
        });
        if externally_buffed {
            self.invalidate("external_player_effect", now);
        }
    }

    fn deadline_reached(&self, now: u64) -> bool {
        self.state
            .started_micros
            .is_some_and(|started| now.saturating_sub(started) >= TRAINING_DURATION_MICROS)
    }

    fn advance(&mut self, now: u64) {
        let Some(started) = self.state.started_micros else {
            return;
        };
        self.state.elapsed_micros = now.saturating_sub(started).min(TRAINING_DURATION_MICROS);
        self.state.remaining_micros =
            TRAINING_DURATION_MICROS.saturating_sub(self.state.elapsed_micros);
    }

    fn add_damage(&mut self, amount: i64) {
        self.state.total_damage = self.state.total_damage.saturating_add(amount.max(0));
    }

    fn finish(&mut self) {
        let Some(started) = self.state.started_micros else {
            return;
        };
        self.state.phase = TrainingDummyPhase::Finished;
        self.state.ended_micros = Some(started.saturating_add(TRAINING_DURATION_MICROS));
        self.state.elapsed_micros = TRAINING_DURATION_MICROS;
        self.state.remaining_micros = 0;
        self.state.dps = self.state.total_damage as f64 / 180.0;
        self.state.valid = true;
    }

    fn invalidate(&mut self, reason: &str, now: u64) {
        self.advance(now);
        self.state.phase = TrainingDummyPhase::Invalid;
        self.state.ended_micros = Some(now);
        self.state.valid = false;
        self.state.invalid_reason = Some(reason.to_owned());
        self.state.dps = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rlogs_events::{
        ActorEvent, ActorLoadoutObservation, ActorState, CanonicalEventDraft,
        CanonicalEventDraftKind, DamageEvent, DamageFlags, EntityRef, EntityUuid,
        EventEnvelopeFactory, EventProvenance, EventSensitivity, EventTime, MonsterId,
        RegionContext, RegionIdentity, SceneId, StatusEvent, WorldContext,
    };

    fn entity(id: u64) -> EntityRef {
        EntityRef {
            actor_id: ActorId(id),
            entity_uuid: EntityUuid(id as i64),
        }
    }

    fn factory() -> EventEnvelopeFactory {
        EventEnvelopeFactory::new(
            "training",
            RegionContext {
                identity: RegionIdentity {
                    deployment_id: "global".into(),
                    region_id: "north-america".into(),
                    realm_id: Some("asteria".into()),
                    world_id: Some("asteria".into()),
                },
                client_build: "fixture".into(),
                protocol_pack_digest: "sha256:fixture".into(),
                evidence: Vec::new(),
            },
        )
    }

    fn emit(
        factory: &mut EventEnvelopeFactory,
        micros: u64,
        kind: CanonicalEventDraftKind,
    ) -> EventEnvelope {
        factory
            .emit(CanonicalEventDraft {
                time: EventTime {
                    observed_micros: micros,
                    game_time_millis: None,
                },
                provenance: EventProvenance::wire(1, 1, 1),
                sensitivity: EventSensitivity::PublicGameplay,
                kind,
            })
            .unwrap()
    }

    fn actor(id: u64, kind: ActorKind, monster_id: Option<i64>) -> CanonicalEventDraftKind {
        CanonicalEventDraftKind::Timeline(TimelineEventKind::Actor(ActorEvent {
            actor: entity(id),
            state: ActorState::Spawned,
            entity_type_id: 0,
            kind,
            monster_id: monster_id.map(MonsterId),
            character_id: (kind == ActorKind::Player).then(|| id.to_string()),
            display_name: (kind == ActorKind::Player).then(|| format!("Player {id}")),
            class_id: (kind == ActorKind::Player).then_some(4),
            specialization_id: (kind == ActorKind::Player).then_some(41),
            level: None,
            ability_score: None,
            weapon_item_id: None,
            weapon_breakthrough_count: None,
            seasonal_score: None,
            primary_loadout: Vec::new(),
            auxiliary_loadout: Vec::new(),
            loadout_observation: ActorLoadoutObservation::default(),
        }))
    }

    fn damage(source: u64, target: u64, amount: i64) -> CanonicalEventDraftKind {
        CanonicalEventDraftKind::Timeline(TimelineEventKind::Damage(DamageEvent {
            source: entity(source),
            direct_source: None,
            target: entity(target),
            ability: None,
            amount,
            actual_amount: None,
            hp_loss: None,
            shield_loss: None,
            hit_event_id: None,
            damage_source: None,
            damage_type: None,
            flags: DamageFlags::default(),
            packet: Default::default(),
        }))
    }

    #[test]
    fn exact_three_minute_window_excludes_damage_at_the_deadline() {
        let mut factory = factory();
        let mut controller = TrainingDummyController::default();
        controller.observe(&emit(
            &mut factory,
            1,
            CanonicalEventDraftKind::WorldChanged(WorldContext {
                scene_id: Some(SceneId(GUILD_HALL_SCENE_ID)),
                map_id: None,
                line_id: None,
                scene_instance_id: None,
                dungeon_instance_id: None,
            }),
        ));
        controller.observe(&emit(&mut factory, 2, actor(1, ActorKind::Player, None)));
        controller.observe(&emit(
            &mut factory,
            3,
            actor(2, ActorKind::TrainingDummy, Some(115)),
        ));
        controller.arm();

        let first = controller.observe(&emit(&mut factory, 10, damage(1, 2, 1_000)));
        assert!(first.reset_meter && first.accept_damage);
        let before = controller.observe(&emit(
            &mut factory,
            TRAINING_DURATION_MICROS + 9,
            damage(1, 2, 500),
        ));
        assert!(before.accept_damage);
        let deadline = controller.observe(&emit(
            &mut factory,
            TRAINING_DURATION_MICROS + 10,
            damage(1, 2, 9_999),
        ));
        assert!(!deadline.accept_damage);
        let state = controller.state();
        assert_eq!(state.phase, TrainingDummyPhase::Finished);
        assert_eq!(state.total_damage, 1_500);
        assert_eq!(state.dps, 1_500.0 / 180.0);
    }

    #[test]
    fn another_player_or_external_player_effect_invalidates_the_result() {
        let mut factory = factory();
        let mut controller = TrainingDummyController::default();
        for event in [
            emit(
                &mut factory,
                1,
                CanonicalEventDraftKind::WorldChanged(WorldContext {
                    scene_id: Some(SceneId(GUILD_HALL_SCENE_ID)),
                    map_id: None,
                    line_id: None,
                    scene_instance_id: None,
                    dungeon_instance_id: None,
                }),
            ),
            emit(&mut factory, 2, actor(1, ActorKind::Player, None)),
            emit(
                &mut factory,
                3,
                actor(2, ActorKind::TrainingDummy, Some(122)),
            ),
            emit(&mut factory, 4, actor(3, ActorKind::Player, None)),
        ] {
            controller.observe(&event);
        }
        controller.arm();
        controller.observe(&emit(&mut factory, 10, damage(1, 2, 1_000)));
        controller.observe(&emit(
            &mut factory,
            11,
            CanonicalEventDraftKind::Timeline(TimelineEventKind::Status(StatusEvent {
                source: Some(entity(3)),
                target: entity(1),
                effect: StatusEffectId(99),
                instance_id: None,
                origin: None,
                state: StatusState::Applied,
                stacks: Some(1),
                duration_millis: Some(10_000),
                level: None,
                part_id: None,
                count: None,
                created_at_millis: None,
            })),
        ));
        assert_eq!(controller.state().phase, TrainingDummyPhase::Invalid);
        assert_eq!(
            controller.state().invalid_reason.as_deref(),
            Some("external_player_effect")
        );
    }
}
