use std::collections::{HashMap, HashSet};

use rlogs_events::{
    ActorId, ActorKind, ActorOwnershipUpdate, CanonicalEvent, EntityUuid, EventEnvelope,
    EventSensitivity, StatusEffectId, StatusState, TimelineEventKind,
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
    pub season_id: Option<i64>,
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
            season_id: None,
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
    actor_by_entity_uuid: HashMap<EntityUuid, ActorId>,
    owner_entity_by_actor: HashMap<ActorId, EntityUuid>,
    active_statuses: HashSet<(ActorId, ActorId, StatusEffectId)>,
    local_character_id: Option<String>,
    local_season_id: Option<i64>,
    observed_party_size: Option<usize>,
    locked_player: Option<ActorId>,
    locked_target: Option<ActorId>,
}

impl Default for TrainingDummyController {
    fn default() -> Self {
        Self {
            state: TrainingDummyState::idle(None),
            actors: HashMap::new(),
            actor_by_entity_uuid: HashMap::new(),
            owner_entity_by_actor: HashMap::new(),
            active_statuses: HashSet::new(),
            local_character_id: None,
            local_season_id: None,
            observed_party_size: None,
            locked_player: None,
            locked_target: None,
        }
    }
}

impl TrainingDummyController {
    pub fn state(&self) -> TrainingDummyState {
        self.state.clone()
    }

    pub fn arm(&mut self) {
        self.state = TrainingDummyState::armed(self.state.scene_id);
        self.state.season_id = self.local_season_id;
        self.state.observed_party_size = self.observed_party_size;
        self.state.season_id = self.local_season_id;
        self.locked_player = None;
        self.locked_target = None;
        if self.observed_party_size.is_some_and(|size| size > 1) {
            self.invalidate("party_not_solo", 0);
        }
    }

    pub fn disarm(&mut self) {
        self.state = TrainingDummyState::idle(self.state.scene_id);
        self.locked_player = None;
        self.locked_target = None;
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
                self.actors.clear();
                self.actor_by_entity_uuid.clear();
                self.owner_entity_by_actor.clear();
                self.active_statuses.clear();
                observation.changed = true;
            }
            if matches!(
                self.state.phase,
                TrainingDummyPhase::Armed | TrainingDummyPhase::Running
            ) && scene_id != Some(GUILD_HALL_SCENE_ID)
            {
                self.invalidate("left_guild_hall", event.time.observed_micros);
                observation.changed = true;
            }
            return observation;
        }

        let CanonicalEvent::Timeline(timeline) = &event.event else {
            let party_size = match &event.event {
                CanonicalEvent::CharacterProfileObserved { profile }
                    if event.sensitivity == EventSensitivity::PersonalGameplay =>
                {
                    if self.local_character_id.as_deref()
                        != Some(profile.character.character_id.as_str())
                    {
                        self.local_character_id = Some(profile.character.character_id.clone());
                        self.local_season_id = None;
                        observation.changed = true;
                    }
                    if let Some(season_id) = (profile.game_plugin_id == crate::BPSR_GAME_PLUGIN_ID
                        && profile.payload_schema_id == crate::BPSR_PROFILE_SCHEMA_ID
                        && profile.payload_schema_version == crate::BPSR_PROFILE_SCHEMA_VERSION)
                        .then(|| profile.payload.get("season")?.get("season_id")?.as_i64())
                        .flatten()
                        .filter(|season_id| *season_id > 0)
                        && self.local_season_id != Some(season_id)
                    {
                        self.local_season_id = Some(season_id);
                        self.state.season_id = Some(season_id);
                        observation.changed = true;
                    }
                    None
                }
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
                self.actor_by_entity_uuid
                    .insert(actor.actor.entity_uuid, actor.actor.actor_id);
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
                if self.invalidate_if_external_effect(event.time.observed_micros) {
                    observation.changed = true;
                }
            }
            TimelineEventKind::EntityAttributes(attributes) => {
                if let Some(ownership) = attributes.ownership {
                    match ownership {
                        ActorOwnershipUpdate::Confirmed { owner_entity_uuid } => {
                            self.owner_entity_by_actor
                                .insert(attributes.actor.actor_id, owner_entity_uuid);
                        }
                        ActorOwnershipUpdate::Cleared => {
                            self.owner_entity_by_actor
                                .remove(&attributes.actor.actor_id);
                        }
                    }
                    if self.invalidate_if_external_effect(event.time.observed_micros) {
                        observation.changed = true;
                    }
                }
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
                        && self.locked_player.is_some_and(|player| {
                            self.player_owner(source.actor_id)
                                .is_some_and(|owner| owner != player)
                        })
                        && (Some(status.target.actor_id) == self.locked_player
                            || Some(status.target.actor_id) == self.locked_target);
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
                            || self.local_player_owner(damage.source.actor_id).is_none()
                        {
                            return observation;
                        }
                        let monster_id = target.monster_id.expect("validated dummy monster id");
                        self.start(
                            self.local_player_owner(damage.source.actor_id)
                                .expect("validated player owner"),
                            damage.target.actor_id,
                            monster_id,
                            event.time.observed_micros,
                        );
                        observation.reset_meter = true;
                        observation.accept_damage = self.state.phase == TrainingDummyPhase::Running;
                        observation.changed = true;
                        if observation.accept_damage {
                            self.add_damage(damage.amount);
                        }
                    }
                    TrainingDummyPhase::Running => {
                        let now = event.time.observed_micros;
                        if self.deadline_reached(now) {
                            self.finish();
                            observation.changed = true;
                        } else if self.player_owner(damage.source.actor_id) == self.locked_player
                            && Some(damage.target.actor_id) == self.locked_target
                        {
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

    fn player_owner(&self, actor_id: ActorId) -> Option<ActorId> {
        let mut current = actor_id;
        for _ in 0..8 {
            let actor = self.actors.get(&current)?;
            if actor.kind == ActorKind::Player {
                return Some(current);
            }
            let owner_uuid = self.owner_entity_by_actor.get(&current)?;
            let owner = *self.actor_by_entity_uuid.get(owner_uuid)?;
            if owner == current {
                return None;
            }
            current = owner;
        }
        None
    }

    fn local_player_owner(&self, actor_id: ActorId) -> Option<ActorId> {
        let owner = self.player_owner(actor_id)?;
        let character_id = self.actors.get(&owner)?.character_id.as_deref()?;
        (Some(character_id) == self.local_character_id.as_deref()).then_some(owner)
    }

    fn start(&mut self, player: ActorId, target: ActorId, monster_id: i64, now: u64) {
        let player_evidence = self.actors.get(&player).cloned();
        self.locked_player = Some(player);
        self.locked_target = Some(target);
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
        self.state.season_id = self.local_season_id;
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

        if self.has_external_effect(player, target) {
            self.invalidate("external_player_effect", now);
        }
    }

    fn has_external_effect(&self, player: ActorId, target: ActorId) -> bool {
        self.active_statuses.iter().any(|(source, recipient, _)| {
            (*recipient == player || *recipient == target)
                && self
                    .player_owner(*source)
                    .is_some_and(|owner| owner != player)
        })
    }

    fn invalidate_if_external_effect(&mut self, now: u64) -> bool {
        if self.state.phase != TrainingDummyPhase::Running {
            return false;
        }
        let (Some(player), Some(target)) = (self.locked_player, self.locked_target) else {
            return false;
        };
        if !self.has_external_effect(player, target) {
            return false;
        }
        self.invalidate("external_player_effect", now);
        true
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
        CanonicalEventDraftKind, CharacterIdentity, DamageEvent, DamageFlags, EntityAttributeEvent,
        EntityAttributeUpdateKind, EntityRef, EntityUuid, EventEnvelopeFactory, EventProvenance,
        EventSensitivity, EventTime, GameProfileEvent, MonsterId, RegionContext, RegionIdentity,
        SceneId, StatusEvent, WorldContext,
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

    fn emit_local_profile(
        factory: &mut EventEnvelopeFactory,
        micros: u64,
        character_id: &str,
    ) -> EventEnvelope {
        factory
            .emit(CanonicalEventDraft {
                time: EventTime {
                    observed_micros: micros,
                    game_time_millis: None,
                },
                provenance: EventProvenance::wire(1, 1, 1),
                sensitivity: EventSensitivity::PersonalGameplay,
                kind: CanonicalEventDraftKind::CharacterProfileObserved {
                    profile: Box::new(GameProfileEvent {
                        game_plugin_id: crate::BPSR_GAME_PLUGIN_ID.into(),
                        payload_schema_id: crate::BPSR_PROFILE_SCHEMA_ID.into(),
                        payload_schema_version: crate::BPSR_PROFILE_SCHEMA_VERSION,
                        character: CharacterIdentity {
                            character_id: character_id.into(),
                            region: RegionIdentity {
                                deployment_id: "global".into(),
                                region_id: "north-america".into(),
                                realm_id: Some("asteria".into()),
                                world_id: Some("asteria".into()),
                            },
                        },
                        payload: serde_json::json!({"season": {"season_id": 3}}),
                    }),
                },
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

    fn ownership(actor: u64, owner: u64) -> CanonicalEventDraftKind {
        CanonicalEventDraftKind::Timeline(TimelineEventKind::EntityAttributes(
            EntityAttributeEvent {
                actor: entity(actor),
                update_kind: EntityAttributeUpdateKind::Delta,
                ownership: Some(ActorOwnershipUpdate::Confirmed {
                    owner_entity_uuid: entity(owner).entity_uuid,
                }),
                attributes: Vec::new(),
            },
        ))
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
        controller.observe(&emit_local_profile(&mut factory, 4, "1"));
        controller.arm();
        assert_eq!(controller.state().season_id, Some(3));

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
    fn nearby_players_cannot_open_or_enter_the_local_dummy_window() {
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
            emit(&mut factory, 3, actor(3, ActorKind::Player, None)),
            emit(
                &mut factory,
                4,
                actor(2, ActorKind::TrainingDummy, Some(115)),
            ),
        ] {
            controller.observe(&event);
        }
        controller.observe(&emit_local_profile(&mut factory, 5, "1"));
        controller.arm();

        let nearby = controller.observe(&emit(&mut factory, 6, damage(3, 2, 50_000)));
        assert!(!nearby.reset_meter);
        assert!(!nearby.accept_damage);
        assert_eq!(controller.state().phase, TrainingDummyPhase::Armed);

        let local = controller.observe(&emit(&mut factory, 7, damage(1, 2, 1_000)));
        assert!(local.reset_meter && local.accept_damage);
        assert_eq!(controller.state().phase, TrainingDummyPhase::Running);
        assert_eq!(controller.state().player_character_id.as_deref(), Some("1"));
        assert_eq!(controller.state().total_damage, 1_000);
    }

    #[test]
    fn running_window_ignores_damage_outside_the_locked_player_and_dummy_pair() {
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
            emit(&mut factory, 3, actor(3, ActorKind::Player, None)),
            emit(
                &mut factory,
                4,
                actor(2, ActorKind::TrainingDummy, Some(115)),
            ),
            emit(
                &mut factory,
                5,
                actor(4, ActorKind::TrainingDummy, Some(122)),
            ),
        ] {
            controller.observe(&event);
        }
        controller.observe(&emit_local_profile(&mut factory, 6, "1"));
        controller.arm();

        controller.observe(&emit(&mut factory, 10, damage(1, 2, 1_000)));
        let nearby_same_target = controller.observe(&emit(&mut factory, 11, damage(3, 2, 50_000)));
        let nearby_other_target = controller.observe(&emit(&mut factory, 12, damage(3, 4, 60_000)));
        let local_other_target = controller.observe(&emit(&mut factory, 13, damage(1, 4, 70_000)));
        let local_locked_target = controller.observe(&emit(&mut factory, 14, damage(1, 2, 500)));

        assert!(!nearby_same_target.accept_damage);
        assert!(!nearby_other_target.accept_damage);
        assert!(!local_other_target.accept_damage);
        assert!(local_locked_target.accept_damage);
        assert_eq!(controller.state().phase, TrainingDummyPhase::Running);
        assert_eq!(controller.state().total_damage, 1_500);
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
        controller.observe(&emit_local_profile(&mut factory, 5, "1"));
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

    #[test]
    fn external_effect_present_before_arming_invalidates_the_result() {
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
                actor(2, ActorKind::TrainingDummy, Some(115)),
            ),
            emit(&mut factory, 4, actor(3, ActorKind::Player, None)),
            emit(
                &mut factory,
                5,
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
            ),
        ] {
            controller.observe(&event);
        }

        controller.observe(&emit_local_profile(&mut factory, 6, "1"));
        controller.arm();
        let opener = controller.observe(&emit(&mut factory, 10, damage(1, 2, 1_000)));

        assert!(opener.reset_meter);
        assert!(!opener.accept_damage);
        assert_eq!(controller.state().phase, TrainingDummyPhase::Invalid);
        assert_eq!(
            controller.state().invalid_reason.as_deref(),
            Some("external_player_effect")
        );
        assert_eq!(controller.state().total_damage, 0);
    }

    #[test]
    fn own_imagine_is_internal_but_another_players_imagine_is_external() {
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
                actor(2, ActorKind::TrainingDummy, Some(115)),
            ),
            emit(&mut factory, 4, actor(3, ActorKind::Pet, None)),
            emit(&mut factory, 5, ownership(3, 1)),
            emit(&mut factory, 6, actor(4, ActorKind::Player, None)),
            emit(&mut factory, 7, actor(5, ActorKind::Pet, None)),
            emit(&mut factory, 8, ownership(5, 4)),
        ] {
            controller.observe(&event);
        }
        controller.observe(&emit_local_profile(&mut factory, 9, "1"));
        controller.arm();

        let first = controller.observe(&emit(&mut factory, 10, damage(3, 2, 1_000)));
        assert!(first.reset_meter && first.accept_damage);
        assert_eq!(controller.state().player_actor_id.as_deref(), Some("1"));
        assert_eq!(controller.state().total_damage, 1_000);

        controller.observe(&emit(
            &mut factory,
            11,
            CanonicalEventDraftKind::Timeline(TimelineEventKind::Status(StatusEvent {
                source: Some(entity(3)),
                target: entity(2),
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
        assert_eq!(controller.state().phase, TrainingDummyPhase::Running);

        controller.observe(&emit(
            &mut factory,
            12,
            CanonicalEventDraftKind::Timeline(TimelineEventKind::Status(StatusEvent {
                source: Some(entity(5)),
                target: entity(2),
                effect: StatusEffectId(100),
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

    #[test]
    fn leaving_guild_hall_does_not_destroy_a_finished_result() {
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
                actor(2, ActorKind::TrainingDummy, Some(115)),
            ),
        ] {
            controller.observe(&event);
        }
        controller.observe(&emit_local_profile(&mut factory, 4, "1"));
        controller.arm();
        controller.observe(&emit(&mut factory, 10, damage(1, 2, 1_000)));
        controller.observe(&emit(
            &mut factory,
            TRAINING_DURATION_MICROS + 10,
            damage(1, 2, 1),
        ));
        assert_eq!(controller.state().phase, TrainingDummyPhase::Finished);

        controller.observe(&emit(
            &mut factory,
            TRAINING_DURATION_MICROS + 20,
            CanonicalEventDraftKind::WorldChanged(WorldContext {
                scene_id: Some(SceneId(1)),
                map_id: None,
                line_id: None,
                scene_instance_id: None,
                dungeon_instance_id: None,
            }),
        ));
        assert_eq!(controller.state().phase, TrainingDummyPhase::Finished);
        assert!(controller.state().valid);
    }
}
