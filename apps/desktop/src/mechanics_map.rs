use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Condvar, Mutex},
    time::{Duration, Instant},
};

use rlogs_events::{
    ActorKind, ActorState, CanonicalEvent, CastState, EntityRef, EventEnvelope, LifeState,
    MapEventKind, PartyRosterObservation, StatusState, TimelineEventKind,
};
use serde::{Deserialize, Serialize};

pub const MECHANICS_MAP_SCHEMA_VERSION: u16 = 1;
const ENTITY_STALE_AFTER_MICROS: u64 = 5_000_000;
const CAST_STALE_AFTER_MICROS: u64 = 8_000_000;
const MAX_ENTITIES: usize = 192;
const MAX_MECHANICS: usize = 96;
const MINIMAP_WORLD_RADIUS: f32 = 140.0;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MechanicsMapSnapshot {
    pub schema_version: u16,
    pub revision: u64,
    pub session_id: Option<String>,
    pub client_build: Option<String>,
    pub scene_id: Option<i32>,
    pub map_id: Option<u32>,
    pub scene_name: Option<String>,
    pub map_model: &'static str,
    pub world_radius: f32,
    pub map_origin_x: Option<f32>,
    pub map_origin_z: Option<f32>,
    pub map_span_x: Option<f32>,
    pub map_span_z: Option<f32>,
    pub background_asset_url: Option<String>,
    pub local_actor_id: Option<u64>,
    pub local_position_observed: bool,
    pub encounter_pack: Option<&'static str>,
    pub encounter_pack_reviewed: bool,
    pub entities: Vec<MechanicsMapEntity>,
    pub mechanics: Vec<MechanicsMapSignal>,
    pub markers: Vec<MechanicsMapMarker>,
    pub data_gap: Option<String>,
    pub last_event_sequence: Option<u64>,
    pub last_observed_micros: Option<u64>,
}

impl Default for MechanicsMapSnapshot {
    fn default() -> Self {
        Self {
            schema_version: MECHANICS_MAP_SCHEMA_VERSION,
            revision: 0,
            session_id: None,
            client_build: None,
            scene_id: None,
            map_id: None,
            scene_name: None,
            map_model: "player_relative_radar",
            world_radius: MINIMAP_WORLD_RADIUS,
            map_origin_x: None,
            map_origin_z: None,
            map_span_x: None,
            map_span_z: None,
            background_asset_url: None,
            local_actor_id: None,
            local_position_observed: false,
            encounter_pack: None,
            encounter_pack_reviewed: false,
            entities: Vec::new(),
            mechanics: Vec::new(),
            markers: Vec::new(),
            data_gap: None,
            last_event_sequence: None,
            last_observed_micros: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MechanicsMapEntity {
    pub actor_id: u64,
    pub entity_uuid: i64,
    pub kind: &'static str,
    pub display_name: Option<String>,
    pub monster_id: Option<i64>,
    pub mechanic_role: Option<&'static str>,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub facing_radians: Option<f32>,
    pub dead: bool,
    pub stale: bool,
    pub last_observed_micros: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MechanicsMapSignal {
    pub effect_id: i64,
    pub mechanic_kind: Option<&'static str>,
    pub presentation_name: Option<String>,
    pub instance_id: Option<i64>,
    pub target_actor_id: u64,
    pub source_actor_id: Option<u64>,
    pub stacks: Option<u32>,
    pub duration_millis: Option<u64>,
    pub origin_x: Option<f32>,
    pub origin_z: Option<f32>,
    pub facing_radians: Option<f32>,
    pub applied_at_micros: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MechanicsMapMarker {
    pub marker_id: Option<i64>,
    pub related_actor_id: Option<u64>,
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub z: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MechanicsMapUpdate {
    pub schema_version: u16,
    pub revision: u64,
    pub snapshot: MechanicsMapSnapshot,
}

#[derive(Debug, Deserialize)]
pub struct MechanicsMapWaitRequest {
    pub after_revision: u64,
    #[serde(default = "default_wait_millis")]
    pub timeout_millis: u64,
}

#[derive(Debug, Default)]
pub struct MechanicsMapFeed {
    snapshot: Mutex<MechanicsMapSnapshot>,
    changed: Condvar,
}

impl MechanicsMapFeed {
    pub fn publish(&self, mut snapshot: MechanicsMapSnapshot) {
        let mut current = self
            .snapshot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *current == snapshot {
            return;
        }
        if snapshot.revision <= current.revision {
            snapshot.revision = current.revision.saturating_add(1);
        }
        *current = snapshot;
        self.changed.notify_all();
    }

    pub fn reset(&self) {
        self.publish(MechanicsMapSnapshot::default());
    }

    pub fn current(&self) -> MechanicsMapUpdate {
        let snapshot = self
            .snapshot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        MechanicsMapUpdate {
            schema_version: MECHANICS_MAP_SCHEMA_VERSION,
            revision: snapshot.revision,
            snapshot,
        }
    }

    pub fn wait_after(&self, after_revision: u64, timeout: Duration) -> MechanicsMapUpdate {
        let deadline = Instant::now() + timeout;
        let mut snapshot = self
            .snapshot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while snapshot.revision <= after_revision {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            snapshot = match self.changed.wait_timeout(snapshot, remaining) {
                Ok((snapshot, _)) => snapshot,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
        MechanicsMapUpdate {
            schema_version: MECHANICS_MAP_SCHEMA_VERSION,
            revision: snapshot.revision,
            snapshot: snapshot.clone(),
        }
    }
}

fn default_wait_millis() -> u64 {
    30_000
}

#[derive(Debug, Clone)]
struct EntityState {
    actor: EntityRef,
    kind: ActorKind,
    character_id: Option<String>,
    display_name: Option<String>,
    monster_id: Option<i64>,
    position: Option<(f32, f32, f32)>,
    facing_radians: Option<f32>,
    dead: bool,
    last_observed_micros: u64,
}

#[derive(Debug, Clone)]
struct SignalState {
    effect_id: i64,
    instance_id: Option<i64>,
    target: EntityRef,
    source: Option<EntityRef>,
    stacks: Option<u32>,
    duration_millis: Option<u64>,
    origin_x: Option<f32>,
    origin_z: Option<f32>,
    facing_radians: Option<f32>,
    applied_at_micros: u64,
}

#[derive(Debug, Default)]
pub struct MechanicsMapProjector {
    revision: u64,
    session_id: Option<String>,
    client_build: Option<String>,
    scene_id: Option<i32>,
    map_id: Option<u32>,
    local_character_id: Option<String>,
    party_character_ids: BTreeSet<String>,
    entities: BTreeMap<u64, EntityState>,
    signals: BTreeMap<(u64, i64), SignalState>,
    markers: BTreeMap<Option<i64>, MechanicsMapMarker>,
    data_gap: Option<String>,
    last_event_sequence: Option<u64>,
    last_observed_micros: Option<u64>,
}

impl MechanicsMapProjector {
    pub fn reset(&mut self, session_id: impl Into<String>, client_build: impl Into<String>) {
        let next_revision = self.revision.saturating_add(1);
        *self = Self {
            revision: next_revision,
            session_id: Some(session_id.into()),
            client_build: Some(client_build.into()),
            ..Self::default()
        };
    }

    pub fn observe(&mut self, envelope: &EventEnvelope) -> bool {
        if self.session_id.as_deref() != Some(envelope.session_id.as_str()) {
            self.reset(&envelope.session_id, &envelope.region.client_build);
        }
        self.last_event_sequence = Some(envelope.sequence);
        self.last_observed_micros = Some(envelope.time.observed_micros);
        let mut changed = false;
        match &envelope.event {
            CanonicalEvent::WorldChanged(world) => {
                let next_scene = world.scene_id.map(|scene| scene.0);
                if self.scene_id != next_scene {
                    self.scene_id = next_scene;
                    self.entities.clear();
                    self.signals.clear();
                    self.markers.clear();
                    self.data_gap = None;
                    changed = true;
                }
                if self.map_id != world.map_id {
                    self.map_id = world.map_id;
                    changed = true;
                }
            }
            CanonicalEvent::CharacterProfileObserved { profile } => {
                if self.local_character_id.as_deref() != Some(&profile.character.character_id) {
                    self.local_character_id = Some(profile.character.character_id.clone());
                    changed = true;
                }
            }
            CanonicalEvent::PartyChanged { members } => {
                let next = members
                    .iter()
                    .map(|member| member.character_id.clone())
                    .collect();
                changed |= self.replace_party(next);
            }
            CanonicalEvent::PartyRosterObserved(roster) => match &roster.observation {
                PartyRosterObservation::FullSnapshot { members, .. } => {
                    let next = members
                        .iter()
                        .map(|member| member.character.character_id.clone())
                        .collect();
                    changed |= self.replace_party(next);
                }
                PartyRosterObservation::MembersObserved { members } => {
                    for member in members {
                        changed |= self
                            .party_character_ids
                            .insert(member.character.character_id.clone());
                    }
                }
                PartyRosterObservation::MemberLeft { member, .. } => {
                    changed |= self.party_character_ids.remove(&member.character_id);
                }
                PartyRosterObservation::Dissolved => {
                    changed |= !self.party_character_ids.is_empty();
                    self.party_character_ids.clear();
                }
            },
            CanonicalEvent::Map(event) => match event.kind {
                MapEventKind::MarkerRemoved => {
                    changed |= self.markers.remove(&event.marker_id).is_some();
                }
                MapEventKind::MarkerAdded
                | MapEventKind::MarkerUpdated
                | MapEventKind::ObjectiveUpdated => {
                    let marker = MechanicsMapMarker {
                        marker_id: event.marker_id,
                        related_actor_id: event.related_entity.map(|entity| entity.actor_id.0),
                        x: event.x,
                        y: event.y,
                        z: event.z,
                    };
                    changed |= self.markers.get(&event.marker_id) != Some(&marker);
                    self.markers.insert(event.marker_id, marker);
                }
                MapEventKind::Entered | MapEventKind::Exited => {}
            },
            CanonicalEvent::Timeline(timeline) => match &timeline.kind {
                TimelineEventKind::Actor(event) => {
                    if event.state == ActorState::Despawned {
                        changed |= self.entities.remove(&event.actor.actor_id.0).is_some();
                        self.signals
                            .retain(|_, signal| signal.target.actor_id != event.actor.actor_id);
                    } else {
                        let entry =
                            self.entities
                                .entry(event.actor.actor_id.0)
                                .or_insert(EntityState {
                                    actor: event.actor,
                                    kind: event.kind,
                                    character_id: event.character_id.clone(),
                                    display_name: event.display_name.clone(),
                                    monster_id: event.monster_id.map(|id| id.0),
                                    position: None,
                                    facing_radians: None,
                                    dead: false,
                                    last_observed_micros: envelope.time.observed_micros,
                                });
                        entry.actor = event.actor;
                        entry.kind = event.kind;
                        entry.character_id = event
                            .character_id
                            .clone()
                            .or_else(|| entry.character_id.clone());
                        entry.display_name = event
                            .display_name
                            .clone()
                            .or_else(|| entry.display_name.clone());
                        entry.monster_id = event.monster_id.map(|id| id.0).or(entry.monster_id);
                        entry.last_observed_micros = envelope.time.observed_micros;
                        changed = true;
                    }
                }
                TimelineEventKind::Position(position) => {
                    if let Some(entity) = self.entities.get_mut(&position.actor.actor_id.0) {
                        entity.position = Some((position.x, position.y, position.z));
                        entity.facing_radians = position.facing_radians;
                        entity.last_observed_micros = envelope.time.observed_micros;
                    } else {
                        self.entities.insert(
                            position.actor.actor_id.0,
                            EntityState {
                                actor: position.actor,
                                kind: ActorKind::Unknown(-1),
                                character_id: None,
                                display_name: None,
                                monster_id: None,
                                position: Some((position.x, position.y, position.z)),
                                facing_radians: position.facing_radians,
                                dead: false,
                                last_observed_micros: envelope.time.observed_micros,
                            },
                        );
                    }
                    changed = true;
                }
                TimelineEventKind::Life { actor, state } => {
                    if let Some(entity) = self.entities.get_mut(&actor.actor_id.0) {
                        let dead = *state == LifeState::Died;
                        changed |= entity.dead != dead;
                        entity.dead = dead;
                        entity.last_observed_micros = envelope.time.observed_micros;
                    }
                }
                TimelineEventKind::Status(status)
                    if is_reviewed_mechanic_effect(
                        self.client_build.as_deref(),
                        self.scene_id,
                        status.effect.0,
                    ) =>
                {
                    let key = (
                        status.target.actor_id.0,
                        status.instance_id.map_or(status.effect.0, |id| id.0),
                    );
                    if status.state == StatusState::Removed || status.state == StatusState::Consumed
                    {
                        changed |= self.signals.remove(&key).is_some();
                    } else {
                        self.signals.insert(
                            key,
                            SignalState {
                                effect_id: status.effect.0,
                                instance_id: status.instance_id.map(|id| id.0),
                                target: status.target,
                                source: status.source,
                                stacks: status.stacks,
                                duration_millis: status.duration_millis,
                                origin_x: None,
                                origin_z: None,
                                facing_radians: None,
                                applied_at_micros: envelope.time.observed_micros,
                            },
                        );
                        changed = true;
                    }
                }
                TimelineEventKind::Cast(cast)
                    if cast.state == CastState::Started
                        && is_reviewed_mechanic_cast(
                            self.client_build.as_deref(),
                            self.scene_id,
                            cast.ability.0,
                        ) =>
                {
                    // Targetless arena casts remain useful mechanic evidence. Anchor those
                    // signals to their caster so the map can project packet-observed facing.
                    let target = cast.target.unwrap_or(cast.source);
                    let (origin_x, origin_z, facing_radians) = self
                        .entities
                        .get(&cast.source.actor_id.0)
                        .and_then(|entity| {
                            entity
                                .position
                                .map(|(x, _, z)| (Some(x), Some(z), entity.facing_radians))
                        })
                        .unwrap_or((None, None, None));
                    self.signals.insert(
                        (target.actor_id.0, -cast.ability.0),
                        SignalState {
                            effect_id: -cast.ability.0,
                            instance_id: None,
                            target,
                            source: Some(cast.source),
                            stacks: None,
                            duration_millis: Some(10_000),
                            origin_x,
                            origin_z,
                            facing_radians,
                            applied_at_micros: envelope.time.observed_micros,
                        },
                    );
                    changed = true;
                }
                TimelineEventKind::DataGap(gap) => {
                    self.data_gap = Some(format!("{:?}: {}", gap.kind, gap.detail));
                    changed = true;
                }
                _ => {}
            },
            _ => {}
        }
        if changed {
            self.enforce_bounds();
            self.revision = self.revision.saturating_add(1);
        }
        changed
    }

    pub fn snapshot(&self) -> MechanicsMapSnapshot {
        let now = self.last_observed_micros.unwrap_or_default();
        let local_actor_id = self.local_actor_id();
        let local_position_observed = local_actor_id
            .and_then(|actor_id| self.entities.get(&actor_id))
            .and_then(|entity| entity.position)
            .is_some();
        let mut entities = self
            .entities
            .values()
            .filter_map(|entity| {
                let (x, y, z) = entity.position?;
                Some(MechanicsMapEntity {
                    actor_id: entity.actor.actor_id.0,
                    entity_uuid: entity.actor.entity_uuid.0,
                    kind: self.entity_kind(entity, local_actor_id),
                    display_name: entity.display_name.clone(),
                    monster_id: entity.monster_id,
                    mechanic_role: reviewed_mechanic_entity_role(
                        self.client_build.as_deref(),
                        self.scene_id,
                        entity.monster_id,
                    ),
                    x,
                    y,
                    z,
                    facing_radians: entity.facing_radians,
                    dead: entity.dead,
                    stale: now.saturating_sub(entity.last_observed_micros)
                        > ENTITY_STALE_AFTER_MICROS,
                    last_observed_micros: entity.last_observed_micros,
                })
            })
            .collect::<Vec<_>>();
        entities.sort_by_key(|entity| {
            (
                entity.kind != "local",
                entity.kind != "boss",
                entity.actor_id,
            )
        });
        entities.truncate(MAX_ENTITIES);
        let mut mechanics = self
            .signals
            .values()
            .filter(|signal| {
                let age = now.saturating_sub(signal.applied_at_micros);
                if signal.effect_id < 0 {
                    return age <= CAST_STALE_AFTER_MICROS;
                }
                signal
                    .duration_millis
                    .filter(|duration| *duration > 0)
                    .is_none_or(|duration| age <= duration.saturating_mul(1_000))
            })
            .map(|signal| MechanicsMapSignal {
                effect_id: signal.effect_id,
                mechanic_kind: reviewed_mechanic_signal_kind(
                    self.client_build.as_deref(),
                    self.scene_id,
                    signal.effect_id,
                ),
                presentation_name: if signal.effect_id < 0 {
                    rlogs_game_bpsr::localized_combat_action_name(-signal.effect_id, "en-US")
                        .ok()
                        .flatten()
                        .map(str::to_owned)
                } else {
                    rlogs_game_bpsr::localized_status_effect_name(signal.effect_id, "en-US")
                        .ok()
                        .flatten()
                        .map(str::to_owned)
                },
                instance_id: signal.instance_id,
                target_actor_id: signal.target.actor_id.0,
                source_actor_id: signal.source.map(|source| source.actor_id.0),
                stacks: signal.stacks,
                duration_millis: signal.duration_millis,
                origin_x: signal.origin_x,
                origin_z: signal.origin_z,
                facing_radians: signal.facing_radians,
                applied_at_micros: signal.applied_at_micros,
            })
            .collect::<Vec<_>>();
        mechanics.sort_by_key(|signal| (signal.target_actor_id, signal.effect_id));
        mechanics.truncate(MAX_MECHANICS);
        let pack = encounter_pack(self.client_build.as_deref(), self.scene_id);
        let scene_map = scene_map_spec(self.client_build.as_deref(), self.scene_id);
        MechanicsMapSnapshot {
            schema_version: MECHANICS_MAP_SCHEMA_VERSION,
            revision: self.revision,
            session_id: self.session_id.clone(),
            client_build: self.client_build.clone(),
            scene_id: self.scene_id,
            map_id: self.map_id,
            scene_name: self.scene_id.and_then(|scene_id| {
                rlogs_game_bpsr::localized_scene_name(i64::from(scene_id), "en-US")
                    .ok()
                    .flatten()
                    .map(str::to_owned)
            }),
            map_model: if scene_map.is_some() {
                "absolute_scene_map"
            } else {
                "player_relative_radar"
            },
            world_radius: MINIMAP_WORLD_RADIUS,
            map_origin_x: scene_map.map(|spec| spec.origin_x),
            map_origin_z: scene_map.map(|spec| spec.origin_z),
            map_span_x: scene_map.map(|spec| spec.span_x),
            map_span_z: scene_map.map(|spec| spec.span_z),
            background_asset_url: match (self.client_build.as_ref(), scene_map) {
                (Some(build), Some(spec)) => {
                    Some(format!("/local-game-assets/{build}/{}", spec.asset_file))
                }
                (Some(build), None) => {
                    Some(format!("/local-game-assets/{build}/dungeon_map_bg.png"))
                }
                (None, _) => None,
            },
            local_actor_id,
            local_position_observed,
            encounter_pack: pack,
            encounter_pack_reviewed: pack.is_some(),
            entities,
            mechanics,
            markers: self.markers.values().take(64).cloned().collect(),
            data_gap: self.data_gap.clone(),
            last_event_sequence: self.last_event_sequence,
            last_observed_micros: self.last_observed_micros,
        }
    }

    fn replace_party(&mut self, next: BTreeSet<String>) -> bool {
        if self.party_character_ids == next {
            return false;
        }
        self.party_character_ids = next;
        true
    }

    fn local_actor_id(&self) -> Option<u64> {
        let local = self.local_character_id.as_deref()?;
        self.entities
            .values()
            .find(|entity| entity.character_id.as_deref() == Some(local))
            .map(|entity| entity.actor.actor_id.0)
    }

    fn entity_kind(&self, entity: &EntityState, local_actor_id: Option<u64>) -> &'static str {
        if Some(entity.actor.actor_id.0) == local_actor_id {
            return "local";
        }
        if entity
            .character_id
            .as_ref()
            .is_some_and(|id| self.party_character_ids.contains(id))
        {
            return "party";
        }
        if entity
            .monster_id
            .is_some_and(|id| rlogs_game_bpsr::is_boss_monster(id).unwrap_or(false))
        {
            return "boss";
        }
        match entity.kind {
            ActorKind::Player => "player",
            ActorKind::Monster | ActorKind::TrainingDummy => "monster",
            ActorKind::Pet => "pet",
            ActorKind::Npc => "npc",
            _ => "object",
        }
    }

    fn enforce_bounds(&mut self) {
        if self.entities.len() > MAX_ENTITIES {
            let mut oldest = self
                .entities
                .iter()
                .map(|(actor_id, entity)| (*actor_id, entity.last_observed_micros))
                .collect::<Vec<_>>();
            oldest.sort_by_key(|(_, observed)| *observed);
            for (actor_id, _) in oldest.into_iter().take(self.entities.len() - MAX_ENTITIES) {
                self.entities.remove(&actor_id);
                self.signals
                    .retain(|_, signal| signal.target.actor_id.0 != actor_id);
            }
        }
        while self.signals.len() > MAX_MECHANICS {
            let Some(oldest) = self
                .signals
                .iter()
                .min_by_key(|(_, signal)| signal.applied_at_micros)
                .map(|(key, _)| *key)
            else {
                break;
            };
            self.signals.remove(&oldest);
        }
        while self.markers.len() > 64 {
            let Some(key) = self.markers.keys().next().copied() else {
                break;
            };
            self.markers.remove(&key);
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct SceneMapSpec {
    asset_file: &'static str,
    origin_x: f32,
    origin_z: f32,
    span_x: f32,
    span_z: f32,
}

fn scene_map_spec(build: Option<&str>, scene_id: Option<i32>) -> Option<SceneMapSpec> {
    if build != Some("global/steam-24687926") {
        return None;
    }
    match scene_id? {
        // Exact texture: dng_main_1001_tina. The paired game-owned region_data
        // stores the lower-left world origin and 800 x 800 span.
        1631..=1633 => Some(SceneMapSpec {
            asset_file: "scene-1631-tina-mindrealm.png",
            origin_x: -640.0,
            origin_z: -523.0,
            span_x: 800.0,
            span_z: 800.0,
        }),
        // Exact texture: dng_branch_6501_godvault. The paired game-owned
        // region_data stores the lower-left world origin and 450 x 450 span.
        6513..=6515 => Some(SceneMapSpec {
            asset_file: "scene-6513-cursed-tomb.png",
            origin_x: -149.0,
            origin_z: -377.0,
            span_x: 450.0,
            span_z: 450.0,
        }),
        _ => None,
    }
}

fn encounter_pack(client_build: Option<&str>, scene_id: Option<i32>) -> Option<&'static str> {
    if client_build != Some("global/steam-24687926") {
        return None;
    }
    match scene_id? {
        6513..=6515 => Some("Cursed Tomb"),
        1150..=1152 => Some("Void Towering Ruin"),
        13021..=13023 => Some("Season 3 raid"),
        6563..=6565 => Some("Coral Sea"),
        1631..=1633 => Some("Tina encounter"),
        6615 => Some("Wasteland encounter"),
        _ => None,
    }
}

fn is_reviewed_mechanic_effect(
    client_build: Option<&str>,
    scene_id: Option<i32>,
    effect_id: i64,
) -> bool {
    if client_build != Some("global/steam-24687926") {
        return false;
    }
    let ids: &[i64] = match scene_id {
        Some(6513..=6515) => &[
            884101, 884102, 884103, 884106, 884122, 884129, 884141, 884162, 884163, 884168, 884169,
            884170,
        ],
        Some(1150..=1152) => &[821076],
        Some(13021..=13023) => &[
            829104, 829105, 829106, 829115, 829116, 829214, 829215, 829217, 829226, 829227, 829228,
            829245, 829304, 829305, 829306, 829307, 829308, 829309, 829314, 829316, 829318, 829323,
            829324, 829326, 829327, 829328, 829329, 829330, 829331, 829332, 829372, 829373, 829374,
        ],
        Some(6563..=6565) => &[
            883707, 883708, 883709, 883710, 883714, 883601, 883602, 883603, 883605, 883631, 522602,
            883633, 883634,
        ],
        Some(1631..=1633) => &[510571, 841519, 841509],
        Some(6615) => &[
            884609, 884610, 884614, 884615, 884616, 884641, 884659, 884660, 884661, 884664,
        ],
        _ => &[],
    };
    ids.contains(&effect_id)
}

fn reviewed_mechanic_entity_role(
    client_build: Option<&str>,
    scene_id: Option<i32>,
    monster_id: Option<i64>,
) -> Option<&'static str> {
    if client_build != Some("global/steam-24687926") || !matches!(scene_id, Some(6513..=6515)) {
        return None;
    }
    match monster_id? {
        33901 => Some("boss"),
        33904 | 33905 => Some("tower"),
        33908 | 33921 => Some("left_clone"),
        33909 | 33922 => Some("right_clone"),
        _ => None,
    }
}

fn reviewed_mechanic_signal_kind(
    client_build: Option<&str>,
    scene_id: Option<i32>,
    effect_id: i64,
) -> Option<&'static str> {
    if client_build != Some("global/steam-24687926") || !matches!(scene_id, Some(6513..=6515)) {
        return None;
    }
    match effect_id {
        884101 | 884106 | 884122 => Some("tower_activating"),
        884102 => Some("tower_blue_complete"),
        884103 => Some("tower_gold_complete"),
        884129 => Some("energy_pillar"),
        884141 => Some("energy_pillar_short"),
        884162 => Some("charge_target_left"),
        884163 => Some("charge_target_right"),
        884168 => Some("charge_target_random"),
        884169 => Some("puzzle_piece_one"),
        884170 => Some("puzzle_piece_two"),
        -3390117 | -3390123 => Some("clone_charge_left"),
        -3390118 | -3390124 => Some("clone_charge_right"),
        _ => None,
    }
}

fn is_reviewed_mechanic_cast(
    client_build: Option<&str>,
    scene_id: Option<i32>,
    ability_id: i64,
) -> bool {
    if client_build != Some("global/steam-24687926") {
        return false;
    }
    let ids: &[i64] = match scene_id {
        Some(6513..=6515) => &[3390117, 3390118, 3390123, 3390124],
        _ => &[],
    };
    ids.contains(&ability_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rlogs_events::{
        ActorEvent, ActorId, ActorLoadoutObservation, CharacterIdentity, EntityUuid,
        EventProvenance, EventSensitivity, EventTime, EvidenceConfidence, EvidenceSource,
        GameProfileEvent, RegionContext, RegionIdentity, SceneId, TimelineEvent,
    };

    fn envelope(sequence: u64, event: CanonicalEvent) -> EventEnvelope {
        EventEnvelope {
            schema_version: rlogs_events::EVENT_SCHEMA_VERSION,
            session_id: "session".into(),
            sequence,
            region: RegionContext {
                identity: RegionIdentity {
                    deployment_id: "global".into(),
                    region_id: "north-america".into(),
                    realm_id: None,
                    world_id: None,
                },
                client_build: "global/steam-24687926".into(),
                protocol_pack_digest: "digest".into(),
                evidence: vec![],
            },
            time: EventTime {
                observed_micros: sequence * 1_000,
                game_time_millis: None,
            },
            provenance: EventProvenance {
                confidence: EvidenceConfidence::Exact,
                source: EvidenceSource::Wire {
                    capture_sequence: sequence,
                    connection_id: 1,
                    stream_id: 1,
                },
            },
            sensitivity: EventSensitivity::PublicGameplay,
            event,
        }
    }

    fn entity(actor_id: u64, uuid: i64) -> EntityRef {
        EntityRef {
            actor_id: ActorId(actor_id),
            entity_uuid: EntityUuid(uuid),
        }
    }

    #[test]
    fn joins_local_identity_and_clears_scene_scoped_state() {
        let mut projector = MechanicsMapProjector::default();
        let identity = CharacterIdentity {
            region: RegionIdentity {
                deployment_id: "global".into(),
                region_id: "north-america".into(),
                realm_id: None,
                world_id: None,
            },
            character_id: "42".into(),
        };
        projector.observe(&envelope(
            1,
            CanonicalEvent::CharacterProfileObserved {
                profile: Box::new(GameProfileEvent {
                    game_plugin_id: "game.rlogs.blue-protocol-star-resonance".into(),
                    payload_schema_id: "test".into(),
                    payload_schema_version: 1,
                    character: identity,
                    payload: serde_json::json!({}),
                }),
            },
        ));
        projector.observe(&envelope(
            2,
            CanonicalEvent::Timeline(TimelineEvent {
                sequence: 1,
                time: EventTime {
                    observed_micros: 2_000,
                    game_time_millis: None,
                },
                provenance: EventProvenance {
                    confidence: EvidenceConfidence::Exact,
                    source: EvidenceSource::Wire {
                        capture_sequence: 2,
                        connection_id: 1,
                        stream_id: 1,
                    },
                },
                kind: TimelineEventKind::Actor(ActorEvent {
                    actor: entity(7, 42 << 16),
                    state: ActorState::Spawned,
                    entity_type_id: 1,
                    kind: ActorKind::Player,
                    monster_id: None,
                    character_id: Some("42".into()),
                    display_name: Some("Local".into()),
                    class_id: None,
                    specialization_id: None,
                    level: None,
                    ability_score: None,
                    weapon_item_id: None,
                    weapon_breakthrough_count: None,
                    seasonal_score: None,
                    primary_loadout: vec![],
                    auxiliary_loadout: vec![],
                    loadout_observation: ActorLoadoutObservation::default(),
                }),
            }),
        ));
        projector.observe(&envelope(
            3,
            CanonicalEvent::Timeline(TimelineEvent {
                sequence: 2,
                time: EventTime {
                    observed_micros: 3_000,
                    game_time_millis: None,
                },
                provenance: EventProvenance {
                    confidence: EvidenceConfidence::Exact,
                    source: EvidenceSource::Wire {
                        capture_sequence: 3,
                        connection_id: 1,
                        stream_id: 1,
                    },
                },
                kind: TimelineEventKind::Position(rlogs_events::PositionEvent {
                    actor: entity(7, 42 << 16),
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                    facing_radians: Some(1.0),
                }),
            }),
        ));
        assert_eq!(projector.snapshot().local_actor_id, Some(7));
        projector.observe(&envelope(
            4,
            CanonicalEvent::WorldChanged(rlogs_events::WorldContext {
                scene_id: Some(SceneId(6615)),
                map_id: Some(6615),
                line_id: None,
                scene_instance_id: None,
                dungeon_instance_id: None,
            }),
        ));
        assert!(projector.snapshot().entities.is_empty());
        assert_eq!(
            projector.snapshot().encounter_pack,
            Some("Wasteland encounter")
        );
    }

    #[test]
    fn mechanic_effects_are_scene_scoped_and_fail_closed() {
        let build = Some("global/steam-24687926");
        assert!(is_reviewed_mechanic_effect(build, Some(6615), 884609));
        assert!(!is_reviewed_mechanic_effect(build, Some(6615), 821076));
        assert!(!is_reviewed_mechanic_effect(build, Some(999_999), 884609));
        assert!(!is_reviewed_mechanic_effect(build, None, 884609));
        assert!(!is_reviewed_mechanic_effect(
            Some("global/steam-newer"),
            Some(6615),
            884609,
        ));
    }

    #[test]
    fn full_scene_map_is_exact_build_and_scene_scoped() {
        let tina = scene_map_spec(Some("global/steam-24687926"), Some(1632))
            .expect("reviewed Tina Mindrealm map");
        assert_eq!(tina.asset_file, "scene-1631-tina-mindrealm.png");
        assert_eq!((tina.origin_x, tina.origin_z), (-640.0, -523.0));
        assert_eq!((tina.span_x, tina.span_z), (800.0, 800.0));

        let map = scene_map_spec(Some("global/steam-24687926"), Some(6513))
            .expect("reviewed Cursed Tomb map");
        assert_eq!(map.asset_file, "scene-6513-cursed-tomb.png");
        assert_eq!((map.origin_x, map.origin_z), (-149.0, -377.0));
        assert_eq!((map.span_x, map.span_z), (450.0, 450.0));
        assert!(scene_map_spec(Some("global/steam-newer"), Some(6513)).is_none());
        assert!(scene_map_spec(Some("global/steam-24687926"), Some(6615)).is_none());
    }

    #[test]
    fn mechanic_casts_are_exact_build_and_scene_scoped() {
        assert!(is_reviewed_mechanic_cast(
            Some("global/steam-24687926"),
            Some(6513),
            3390117,
        ));
        assert!(!is_reviewed_mechanic_cast(
            Some("global/steam-24687926"),
            Some(6513),
            1701,
        ));
        assert!(!is_reviewed_mechanic_cast(
            Some("global/steam-newer"),
            Some(6513),
            3390117,
        ));
    }

    #[test]
    fn cursed_tomb_semantics_are_exact_build_and_scene_scoped() {
        let build = Some("global/steam-24687926");
        assert_eq!(
            reviewed_mechanic_entity_role(build, Some(6513), Some(33904)),
            Some("tower")
        );
        assert_eq!(
            reviewed_mechanic_entity_role(build, Some(6513), Some(33922)),
            Some("right_clone")
        );
        assert_eq!(
            reviewed_mechanic_signal_kind(build, Some(6513), 884102),
            Some("tower_blue_complete")
        );
        assert_eq!(
            reviewed_mechanic_signal_kind(build, Some(6513), -3390117),
            Some("clone_charge_left")
        );
        assert_eq!(
            reviewed_mechanic_signal_kind(Some("global/steam-newer"), Some(6513), 884102),
            None
        );
        assert_eq!(
            reviewed_mechanic_entity_role(build, Some(6615), Some(33904)),
            None
        );
    }

    #[test]
    fn feed_keeps_revisions_monotonic_across_reset() {
        let feed = MechanicsMapFeed::default();
        feed.publish(MechanicsMapSnapshot {
            revision: 8,
            scene_id: Some(6615),
            ..MechanicsMapSnapshot::default()
        });
        feed.reset();
        let update = feed.current();
        assert_eq!(update.revision, 9);
        assert_eq!(update.snapshot.scene_id, None);
    }
}
