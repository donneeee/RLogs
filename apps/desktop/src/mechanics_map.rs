use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Condvar, Mutex, OnceLock},
    time::{Duration, Instant},
};

use rlogs_events::{
    ActorKind, ActorOwnershipUpdate, ActorState, CanonicalEvent, CastState, DungeonEventKind,
    DungeonFlowPhase, DungeonObjectiveCatalogResolution, EncounterState, EntityAttributeUpdateKind,
    EntityAttributeValue, EntityRef, EventEnvelope, LifeState, MapEventKind,
    PartyRosterObservation, StatusState, TimelineEventKind,
};
use serde::{Deserialize, Serialize};

pub const MECHANICS_MAP_SCHEMA_VERSION: u16 = 14;
const ENTITY_STALE_AFTER_MICROS: u64 = 5_000_000;
const CAST_STALE_AFTER_MICROS: u64 = 8_000_000;
const MAX_ENTITIES: usize = 192;
const MAX_MECHANICS: usize = 96;
const MAX_TARGET_DEBUFFS: usize = 24;
const MAX_PLAYER_STATUSES: usize = 24;
const MAX_TARGET_STATUSES: usize = 4_096;
const MAX_LOCAL_COOLDOWNS: usize = 48;
const MAX_RESOURCE_VALUES: usize = 32;
const MAX_PARTY_FRAMES: usize = 39;
const MAX_DUNGEON_OBJECTIVES: usize = 32;
const MINIMAP_WORLD_RADIUS: f32 = 140.0;
// Exact build-locked `EAttrType` IDs decoded by the BPSR integration.
const ATTR_TARGET_ID: i32 = 0x1e;
const ATTR_BREAKING_STAGE: i32 = 455;
const ATTR_CURRENT_HP: i32 = 11310;
const ATTR_MAX_HP_FINAL: i32 = 11320;
const ATTR_SHIELD_LIST: i32 = 60050;

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
    pub map_layout: Option<&'static str>,
    pub world_radius: f32,
    pub map_origin_x: Option<f32>,
    pub map_origin_z: Option<f32>,
    pub map_span_x: Option<f32>,
    pub map_span_z: Option<f32>,
    pub background_asset_url: Option<String>,
    pub local_actor_id: Option<u64>,
    pub local_position_observed: bool,
    /// Packet-observed state for the local character. This deliberately uses
    /// the same entity-attribute lifecycle as the target frame; absent packet
    /// values remain unavailable instead of being filled from a profile.
    pub player: Option<PlayerFrameSnapshot>,
    /// Packet-rostered party members that currently have a joined actor.
    /// Missing actors remain absent instead of borrowing combat rows.
    pub party: Vec<PlayerFrameSnapshot>,
    /// Latest packet-observed skill cooldown state for the local character.
    /// These are HUD controls only: they do not synthesize input or infer a
    /// cooldown from damage hits.
    pub action_controls: Vec<ActionControlSnapshot>,
    /// Exact local-player class resources from the parallel resource arrays.
    /// A pair is published only when both its current and maximum IDs were
    /// observed; mismatched arrays are never zipped or guessed.
    pub resources: Vec<ResourceHudSnapshot>,
    /// Exact packet-backed dungeon flow and objectives for an independently
    /// movable HUD tracker. Raw values remain raw when their unit or semantic
    /// meaning has not been proven for this build.
    pub dungeon: Option<DungeonHudSnapshot>,
    pub encounter_pack: Option<&'static str>,
    pub encounter_pack_reviewed: bool,
    /// Packet-selected target for the local character. `None` means the
    /// protocol has not selected a target (or the target is not present in the
    /// current scene), never an inferred damage recipient.
    pub target: Option<TargetFrameSnapshot>,
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
            map_layout: None,
            world_radius: MINIMAP_WORLD_RADIUS,
            map_origin_x: None,
            map_origin_z: None,
            map_span_x: None,
            map_span_z: None,
            background_asset_url: None,
            local_actor_id: None,
            local_position_observed: false,
            player: None,
            party: Vec::new(),
            action_controls: Vec::new(),
            resources: Vec::new(),
            dungeon: None,
            encounter_pack: None,
            encounter_pack_reviewed: false,
            target: None,
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
    pub marker_number: Option<u8>,
    pub related_actor_id: Option<u64>,
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub z: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResourceHudSnapshot {
    pub kind: &'static str,
    pub label: &'static str,
    pub current_id: u32,
    pub max_id: u32,
    pub current: u32,
    pub max: u32,
    pub percent: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TargetFrameSnapshot {
    pub actor_id: u64,
    pub entity_uuid: i64,
    pub display_name: Option<String>,
    pub monster_id: Option<i64>,
    pub current_hp: Option<i64>,
    pub max_hp: Option<i64>,
    pub hp_percent: Option<f64>,
    pub current_shield: Option<i64>,
    pub max_shield: Option<i64>,
    pub shield_percent: Option<f64>,
    /// Exact current-build `EBreakingStage` integer: `0 = Breaking`,
    /// `1 = BreakEnd`. Unknown future values remain numeric and are never
    /// coerced into either state.
    pub breaking_stage: Option<i64>,
    pub dead: bool,
    pub stale: bool,
    pub debuffs: Vec<TargetFrameDebuff>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlayerFrameSnapshot {
    pub actor_id: u64,
    pub entity_uuid: i64,
    pub display_name: Option<String>,
    pub current_hp: Option<i64>,
    pub max_hp: Option<i64>,
    pub hp_percent: Option<f64>,
    pub current_shield: Option<i64>,
    pub max_shield: Option<i64>,
    pub shield_percent: Option<f64>,
    pub dead: bool,
    pub stale: bool,
    /// Packet-observed local-recipient effects whose exact current-build game
    /// asset classifies them as buffs. Unknown and debuff-atlas effects are
    /// deliberately excluded instead of being guessed into the player HUD.
    pub statuses: Vec<TargetFrameDebuff>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ActionControlSnapshot {
    /// Exact `SkillCDInfo.skill_level_id` carried by the packet.
    pub skill_level_id: i64,
    /// Presentation-only base action selected from the exact ID or its
    /// reviewed SkillLevel -> Skill relation.
    pub presentation_ability_id: Option<i64>,
    pub presentation_name: Option<String>,
    pub icon_asset_path: Option<String>,
    pub duration_millis: Option<i32>,
    pub remaining_millis: Option<u64>,
    pub cooldown_type: Option<i32>,
    pub charge_count: Option<i32>,
    pub observed_at_micros: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TargetFrameDebuff {
    pub effect_id: i64,
    pub instance_id: Option<i64>,
    pub presentation_name: Option<String>,
    pub icon_asset_path: Option<String>,
    pub source_actor_id: Option<u64>,
    pub source_display_name: Option<String>,
    /// True only when the packet-proven source is the local player or an
    /// ownership descendant such as that player's Battle Imagine.
    pub owned_by_local_player: bool,
    pub stacks: Option<u32>,
    pub duration_millis: Option<u64>,
    pub remaining_millis: Option<u64>,
    pub applied_at_micros: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DungeonHudSnapshot {
    pub dungeon_id: Option<i64>,
    pub instance_id: Option<String>,
    pub difficulty_id: Option<i32>,
    pub state: &'static str,
    pub flow_phase: Option<&'static str>,
    pub flow_state_id: Option<i32>,
    pub result_id: Option<i32>,
    pub attempt_number: u32,
    pub retry_count: u32,
    pub encounter_state: Option<&'static str>,
    /// Time since the authoritative encounter start. This freezes at a wipe,
    /// clear, or end boundary instead of continuing while the boss is dead.
    pub attempt_elapsed_micros: u64,
    pub attempt_running: bool,
    pub objectives: Vec<DungeonObjectiveSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DungeonObjectiveSnapshot {
    pub objective_id: i64,
    pub objective_map_key: Option<i32>,
    pub value: Option<i64>,
    pub complete: Option<bool>,
    pub presentation_name: Option<String>,
    pub required_count: Option<i64>,
    pub catalog_resolution: &'static str,
    pub activity_target_key: Option<String>,
    pub scene_event_keys: Vec<String>,
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
    class_id: Option<i32>,
    specialization_id: Option<i32>,
    owner_entity_uuid: Option<i64>,
    current_hp: Option<i64>,
    max_hp: Option<i64>,
    current_shield: Option<i64>,
    max_shield: Option<i64>,
    breaking_stage: Option<i64>,
    position: Option<(f32, f32, f32)>,
    facing_radians: Option<f32>,
    dead: bool,
    last_observed_micros: u64,
}

#[derive(Debug, Clone)]
struct TargetStatusState {
    effect_id: i64,
    instance_id: Option<i64>,
    target: EntityRef,
    source: Option<EntityRef>,
    stacks: Option<u32>,
    duration_millis: Option<u64>,
    applied_at_micros: u64,
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

#[derive(Debug, Clone)]
struct CooldownState {
    skill_level_id: i64,
    duration_millis: Option<i32>,
    cooldown_type: Option<i32>,
    charge_count: Option<i32>,
    observed_at_micros: u64,
}

#[derive(Debug, Clone)]
struct DungeonHudState {
    dungeon_id: Option<i64>,
    instance_id: Option<String>,
    difficulty_id: Option<i32>,
    state: DungeonEventKind,
    flow: Option<rlogs_events::DungeonFlowSnapshot>,
    attempt_number: u32,
    retry_count: u32,
    encounter_state: Option<EncounterState>,
    attempt_started_micros: Option<u64>,
    attempt_elapsed_micros: u64,
    objectives: BTreeMap<i64, DungeonObjectiveSnapshot>,
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
    attack_targets: BTreeMap<u64, i64>,
    target_statuses: BTreeMap<(u64, i64), TargetStatusState>,
    cooldowns: BTreeMap<(u64, i64), CooldownState>,
    resource_values: BTreeMap<(u64, u32), u32>,
    dungeon: Option<DungeonHudState>,
    signals: BTreeMap<(u64, i64), SignalState>,
    markers: BTreeMap<Option<i64>, MechanicsMapMarker>,
    local_markers: BTreeMap<i64, MechanicsMapMarker>,
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
        if self.client_build.as_deref() != Some(envelope.region.client_build.as_str()) {
            self.client_build = Some(envelope.region.client_build.clone());
            // A build transition changes every build-scoped presentation gate.
            // Preserve packet positions, but discard old mechanic identities so
            // an unreviewed update can never inherit guidance from its baseline.
            self.signals.clear();
            self.local_markers.clear();
            changed = true;
        }
        match &envelope.event {
            CanonicalEvent::WorldChanged(world) => {
                let next_scene = world.scene_id.map(|scene| scene.0);
                if self.scene_id != next_scene {
                    self.scene_id = next_scene;
                    self.entities.clear();
                    self.attack_targets.clear();
                    self.target_statuses.clear();
                    self.resource_values.clear();
                    self.signals.clear();
                    self.markers.clear();
                    self.local_markers.clear();
                    self.dungeon = None;
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
            CanonicalEvent::Dungeon(event) => {
                if event.kind == DungeonEventKind::Exited {
                    changed |= self.dungeon.take().is_some();
                } else {
                    let dungeon_id = event.dungeon_id.map(|id| id.0);
                    let replace = self.dungeon.as_ref().is_none_or(|current| {
                        event.kind == DungeonEventKind::Entered
                            && (current.dungeon_id != dungeon_id
                                || current.instance_id != event.instance_id)
                    });
                    if replace {
                        self.dungeon = Some(DungeonHudState {
                            dungeon_id,
                            instance_id: event.instance_id.clone(),
                            difficulty_id: event.difficulty_id,
                            state: event.kind,
                            flow: event.flow.clone(),
                            attempt_number: 0,
                            retry_count: 0,
                            encounter_state: None,
                            attempt_started_micros: None,
                            attempt_elapsed_micros: 0,
                            objectives: BTreeMap::new(),
                        });
                        changed = true;
                    }
                    let state = self.dungeon.get_or_insert_with(|| DungeonHudState {
                        dungeon_id,
                        instance_id: event.instance_id.clone(),
                        difficulty_id: event.difficulty_id,
                        state: event.kind,
                        flow: event.flow.clone(),
                        attempt_number: 0,
                        retry_count: 0,
                        encounter_state: None,
                        attempt_started_micros: None,
                        attempt_elapsed_micros: 0,
                        objectives: BTreeMap::new(),
                    });
                    let before = state.clone();
                    state.dungeon_id = dungeon_id.or(state.dungeon_id);
                    state.instance_id = event
                        .instance_id
                        .clone()
                        .or_else(|| state.instance_id.clone());
                    state.difficulty_id = event.difficulty_id.or(state.difficulty_id);
                    state.state = event.kind;
                    if event.flow.is_some() {
                        state.flow = event.flow.clone();
                    }
                    if let Some(objective_id) = event.objective_id {
                        if event.kind == DungeonEventKind::ObjectiveRemoved {
                            state.objectives.remove(&objective_id);
                        } else if event.kind == DungeonEventKind::ObjectiveUpdated {
                            let catalog = event.objective_catalog.as_ref();
                            let presentation = self.client_build.as_deref().and_then(|build| {
                                rlogs_game_bpsr::bundled_dungeon_objective_presentation(
                                    build,
                                    objective_id,
                                    "en-US",
                                )
                                .ok()
                                .flatten()
                            });
                            state.objectives.insert(
                                objective_id,
                                DungeonObjectiveSnapshot {
                                    objective_id,
                                    objective_map_key: event.objective_map_key,
                                    value: event.objective_value,
                                    complete: event.objective_complete,
                                    presentation_name: presentation
                                        .as_ref()
                                        .and_then(|value| value.name.clone()),
                                    required_count: catalog
                                        .and_then(|value| value.required_count)
                                        .or_else(|| {
                                            presentation.as_ref().map(|value| value.required_count)
                                        }),
                                    catalog_resolution: objective_resolution(
                                        catalog.map(|value| value.resolution),
                                    ),
                                    activity_target_key: catalog
                                        .and_then(|value| value.activity_target_key.clone())
                                        .or_else(|| {
                                            presentation
                                                .as_ref()
                                                .map(|value| value.stable_key.clone())
                                        }),
                                    scene_event_keys: catalog
                                        .map(|value| value.scene_event_keys.clone())
                                        .unwrap_or_default(),
                                },
                            );
                        }
                    }
                    changed |= !dungeon_hud_state_equal(&before, state);
                }
            }
            CanonicalEvent::Map(event) => match event.kind {
                MapEventKind::MarkerRemoved => {
                    changed |= self.markers.remove(&event.marker_id).is_some();
                }
                MapEventKind::MarkerAdded
                | MapEventKind::MarkerUpdated
                | MapEventKind::ObjectiveUpdated => {
                    let marker = MechanicsMapMarker {
                        marker_id: event.marker_id,
                        marker_number: None,
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
                TimelineEventKind::EncounterBoundary { state: next, .. } => {
                    if let Some(dungeon) = self.dungeon.as_mut() {
                        let before = dungeon.clone();
                        observe_dungeon_encounter_boundary(
                            dungeon,
                            *next,
                            envelope.time.observed_micros,
                        );
                        changed |= !dungeon_hud_state_equal(&before, dungeon);
                    }
                }
                TimelineEventKind::Actor(event) => {
                    if event.state == ActorState::Despawned {
                        changed |= self.entities.remove(&event.actor.actor_id.0).is_some();
                        changed |= self
                            .attack_targets
                            .remove(&event.actor.actor_id.0)
                            .is_some();
                        let status_count = self.target_statuses.len();
                        self.target_statuses.retain(|_, status| {
                            status.target.actor_id != event.actor.actor_id
                                && status.source.map(|source| source.actor_id)
                                    != Some(event.actor.actor_id)
                        });
                        changed |= status_count != self.target_statuses.len();
                        let target_uuid = event.actor.entity_uuid.0;
                        let target_count = self.attack_targets.len();
                        self.attack_targets
                            .retain(|_, selected_uuid| *selected_uuid != target_uuid);
                        changed |= target_count != self.attack_targets.len();
                        self.signals
                            .retain(|_, signal| signal.target.actor_id != event.actor.actor_id);
                        self.cooldowns
                            .retain(|(actor_id, _), _| *actor_id != event.actor.actor_id.0);
                        self.resource_values
                            .retain(|(actor_id, _), _| *actor_id != event.actor.actor_id.0);
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
                                    class_id: event.class_id,
                                    specialization_id: event.specialization_id,
                                    owner_entity_uuid: None,
                                    current_hp: None,
                                    max_hp: None,
                                    current_shield: None,
                                    max_shield: None,
                                    breaking_stage: None,
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
                        entry.class_id = event.class_id.or(entry.class_id);
                        entry.specialization_id =
                            event.specialization_id.or(entry.specialization_id);
                        entry.last_observed_micros = envelope.time.observed_micros;
                        changed = true;
                    }
                }
                TimelineEventKind::EntityAttributes(attributes) => {
                    let actor_id = attributes.actor.actor_id.0;
                    let entry = self.entities.entry(actor_id).or_insert(EntityState {
                        actor: attributes.actor,
                        kind: ActorKind::Unknown(-1),
                        character_id: None,
                        display_name: None,
                        monster_id: None,
                        class_id: None,
                        specialization_id: None,
                        owner_entity_uuid: None,
                        current_hp: None,
                        max_hp: None,
                        current_shield: None,
                        max_shield: None,
                        breaking_stage: None,
                        position: None,
                        facing_radians: None,
                        dead: false,
                        last_observed_micros: envelope.time.observed_micros,
                    });
                    entry.actor = attributes.actor;
                    entry.last_observed_micros = envelope.time.observed_micros;
                    if let Some(ownership) = attributes.ownership {
                        entry.owner_entity_uuid = match ownership {
                            ActorOwnershipUpdate::Confirmed { owner_entity_uuid } => {
                                Some(owner_entity_uuid.0)
                            }
                            ActorOwnershipUpdate::Cleared => None,
                        };
                    }
                    if attributes.update_kind == EntityAttributeUpdateKind::Snapshot {
                        entry.current_hp = None;
                        entry.max_hp = None;
                        entry.current_shield = None;
                        entry.max_shield = None;
                        entry.breaking_stage = None;
                    }
                    for attribute in &attributes.attributes {
                        if attribute.attribute_id == ATTR_SHIELD_LIST {
                            match rlogs_game_bpsr::decode_shield_list(&attribute.raw_value) {
                                Ok(shields) => {
                                    entry.current_shield = shields.current_value_total();
                                    entry.max_shield = shields.max_value_total();
                                }
                                Err(_) => {
                                    // An observed but malformed replacement cannot leave an
                                    // earlier value looking current. Preserve the undecoded
                                    // packet in the canonical log, but fail this projection
                                    // closed until a later valid shield list arrives.
                                    entry.current_shield = None;
                                    entry.max_shield = None;
                                }
                            }
                            continue;
                        }
                        if let Some(EntityAttributeValue::Integer(value)) = attribute.decoded {
                            match attribute.attribute_id {
                                ATTR_TARGET_ID if value > 0 => {
                                    self.attack_targets.insert(actor_id, value);
                                }
                                ATTR_TARGET_ID => {
                                    self.attack_targets.remove(&actor_id);
                                }
                                ATTR_CURRENT_HP => {
                                    entry.current_hp = Some(value);
                                }
                                ATTR_MAX_HP_FINAL => {
                                    entry.max_hp = Some(value);
                                }
                                ATTR_BREAKING_STAGE => {
                                    entry.breaking_stage = Some(value);
                                }
                                _ => {}
                            }
                        }
                    }
                    changed = true;
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
                                class_id: None,
                                specialization_id: None,
                                owner_entity_uuid: None,
                                current_hp: None,
                                max_hp: None,
                                current_shield: None,
                                max_shield: None,
                                breaking_stage: None,
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
                TimelineEventKind::Cooldown(cooldown) => {
                    let skill_level_id = cooldown.ability.0;
                    if skill_level_id > 0 {
                        let key = (cooldown.actor.actor_id.0, skill_level_id);
                        self.cooldowns.insert(
                            key,
                            CooldownState {
                                skill_level_id,
                                duration_millis: cooldown.duration_millis,
                                cooldown_type: cooldown.cooldown_type,
                                charge_count: cooldown.charge_count,
                                observed_at_micros: envelope.time.observed_micros,
                            },
                        );
                        changed = true;
                    }
                }
                TimelineEventKind::Resource(resource) => {
                    let actor_id = resource.actor.actor_id.0;
                    if resource.update_kind == EntityAttributeUpdateKind::Snapshot {
                        let before = self.resource_values.len();
                        self.resource_values
                            .retain(|(existing_actor_id, _), _| *existing_actor_id != actor_id);
                        changed |= before != self.resource_values.len();
                    }
                    // The protocol owns two parallel arrays. Pairing is exact
                    // only when their lengths agree; partial arrays remain in
                    // the canonical event and are not guessed into HUD state.
                    if resource.resource_ids.len() == resource.resource_values.len() {
                        for (&resource_id, &value) in resource
                            .resource_ids
                            .iter()
                            .zip(resource.resource_values.iter())
                            .take(MAX_RESOURCE_VALUES)
                        {
                            if !is_reviewed_hud_resource_id(resource_id) {
                                continue;
                            }
                            changed |= self.resource_values.insert((actor_id, resource_id), value)
                                != Some(value);
                        }
                    }
                }
                TimelineEventKind::Status(status) => {
                    let key = (
                        status.target.actor_id.0,
                        status.instance_id.map_or(status.effect.0, |id| id.0),
                    );
                    if status.state == StatusState::Removed || status.state == StatusState::Consumed
                    {
                        changed |= self.target_statuses.remove(&key).is_some();
                        if is_reviewed_mechanic_effect(
                            self.client_build.as_deref(),
                            self.scene_id,
                            status.effect.0,
                        ) {
                            changed |= self.signals.remove(&key).is_some();
                        }
                    } else {
                        self.target_statuses.insert(
                            key,
                            TargetStatusState {
                                effect_id: status.effect.0,
                                instance_id: status.instance_id.map(|id| id.0),
                                target: status.target,
                                source: status.source,
                                stacks: status.stacks,
                                duration_millis: status.duration_millis,
                                applied_at_micros: envelope.time.observed_micros,
                            },
                        );
                        if is_reviewed_mechanic_effect(
                            self.client_build.as_deref(),
                            self.scene_id,
                            status.effect.0,
                        ) {
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
                        }
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
        let player = local_actor_id
            .and_then(|actor_id| self.entities.get(&actor_id))
            .map(|entity| PlayerFrameSnapshot {
                actor_id: entity.actor.actor_id.0,
                entity_uuid: entity.actor.entity_uuid.0,
                display_name: entity.display_name.clone(),
                current_hp: entity.current_hp,
                max_hp: entity.max_hp,
                hp_percent: observed_percent(entity.current_hp, entity.max_hp),
                current_shield: entity.current_shield,
                max_shield: entity.max_shield,
                shield_percent: observed_percent(entity.current_shield, entity.max_shield),
                dead: entity.dead,
                stale: now.saturating_sub(entity.last_observed_micros) > ENTITY_STALE_AFTER_MICROS,
                statuses: self.presented_statuses_for_actor(
                    entity.actor.actor_id.0,
                    now,
                    "/buff/",
                    MAX_PLAYER_STATUSES,
                ),
            });
        let mut party = self
            .entities
            .values()
            .filter(|entity| {
                Some(entity.actor.actor_id.0) != local_actor_id
                    && entity
                        .character_id
                        .as_ref()
                        .is_some_and(|id| self.party_character_ids.contains(id))
            })
            .map(|entity| PlayerFrameSnapshot {
                actor_id: entity.actor.actor_id.0,
                entity_uuid: entity.actor.entity_uuid.0,
                display_name: entity.display_name.clone(),
                current_hp: entity.current_hp,
                max_hp: entity.max_hp,
                hp_percent: observed_percent(entity.current_hp, entity.max_hp),
                current_shield: entity.current_shield,
                max_shield: entity.max_shield,
                shield_percent: observed_percent(entity.current_shield, entity.max_shield),
                dead: entity.dead,
                stale: now.saturating_sub(entity.last_observed_micros) > ENTITY_STALE_AFTER_MICROS,
                statuses: Vec::new(),
            })
            .collect::<Vec<_>>();
        party.sort_by(|left, right| {
            left.display_name
                .as_deref()
                .unwrap_or_default()
                .cmp(right.display_name.as_deref().unwrap_or_default())
                .then(left.actor_id.cmp(&right.actor_id))
        });
        party.truncate(MAX_PARTY_FRAMES);
        let mut action_controls = local_actor_id
            .into_iter()
            .flat_map(|actor_id| {
                self.cooldowns
                    .range((actor_id, i64::MIN)..=(actor_id, i64::MAX))
                    .map(|(_, cooldown)| action_control_snapshot(cooldown, now))
            })
            .collect::<Vec<_>>();
        action_controls.sort_by_key(|control| control.skill_level_id);
        action_controls.truncate(MAX_LOCAL_COOLDOWNS);
        let resources = local_actor_id
            .and_then(|actor_id| {
                self.entities
                    .get(&actor_id)
                    .and_then(|entity| entity.class_id)
                    .map(|class_id| {
                        resource_hud_snapshots(class_id, actor_id, &self.resource_values)
                    })
            })
            .unwrap_or_default();
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
        let target = local_actor_id
            .and_then(|actor_id| self.attack_targets.get(&actor_id).copied())
            .and_then(|entity_uuid| {
                self.entities
                    .values()
                    .find(|entity| entity.actor.entity_uuid.0 == entity_uuid)
            })
            .map(|entity| {
                let mut debuffs = self
                    .target_statuses
                    .values()
                    .filter(|status| status.target.actor_id == entity.actor.actor_id)
                    .filter(|status| {
                        status
                            .duration_millis
                            .filter(|duration| *duration > 0)
                            .is_none_or(|duration| {
                                now.saturating_sub(status.applied_at_micros)
                                    <= duration.saturating_mul(1_000)
                            })
                    })
                    .filter_map(|status| {
                        let presentation =
                            rlogs_game_bpsr::status_effect_presentation(status.effect_id)
                                .ok()
                                .flatten()?;
                        let icon = presentation.icon.as_deref()?;
                        // The reviewed game asset path distinguishes the
                        // debuff atlas from recipient/self buff atlases. Fail
                        // closed when the game data does not classify it.
                        if !icon.replace('\\', "/").contains("/debuff/") {
                            return None;
                        }
                        Some(TargetFrameDebuff {
                            effect_id: status.effect_id,
                            instance_id: status.instance_id,
                            presentation_name: rlogs_game_bpsr::localized_status_effect_name(
                                status.effect_id,
                                "en-US",
                            )
                            .ok()
                            .flatten()
                            .map(str::to_owned)
                            .or_else(|| presentation.technical_name.clone()),
                            icon_asset_path: Some(format!(
                                "/game-assets/blue-protocol-star-resonance/shared/{icon}"
                            )),
                            source_actor_id: status.source.map(|source| source.actor_id.0),
                            source_display_name: status.source.and_then(|source| {
                                self.entities.get(&source.actor_id.0).and_then(|entity| {
                                    entity.display_name.clone().or_else(|| {
                                        entity.monster_id.and_then(|monster_id| {
                                            rlogs_game_bpsr::localized_monster_name(
                                                monster_id, "en-US",
                                            )
                                            .ok()
                                            .flatten()
                                            .map(str::to_owned)
                                        })
                                    })
                                })
                            }),
                            owned_by_local_player: status.source.is_some_and(|source| {
                                self.source_is_owned_by_local_player(
                                    source.actor_id.0,
                                    local_actor_id,
                                )
                            }),
                            stacks: status.stacks,
                            duration_millis: status.duration_millis,
                            remaining_millis: status.duration_millis.map(|duration| {
                                duration.saturating_sub(
                                    now.saturating_sub(status.applied_at_micros) / 1_000,
                                )
                            }),
                            applied_at_micros: status.applied_at_micros,
                        })
                    })
                    .collect::<Vec<_>>();
                // Local-player effects are the first visual group, including
                // effects emitted by a packet-proven owned Battle Imagine.
                // Every other observed debuff remains visible after them.
                debuffs.sort_by_key(|status| {
                    (
                        !status.owned_by_local_player,
                        status.effect_id,
                        status.instance_id,
                    )
                });
                debuffs.truncate(MAX_TARGET_DEBUFFS);
                let hp_percent = observed_percent(entity.current_hp, entity.max_hp);
                let shield_percent = observed_percent(entity.current_shield, entity.max_shield);
                TargetFrameSnapshot {
                    actor_id: entity.actor.actor_id.0,
                    entity_uuid: entity.actor.entity_uuid.0,
                    display_name: entity.display_name.clone().or_else(|| {
                        entity.monster_id.and_then(|monster_id| {
                            rlogs_game_bpsr::localized_monster_name(monster_id, "en-US")
                                .ok()
                                .flatten()
                                .map(str::to_owned)
                        })
                    }),
                    monster_id: entity.monster_id,
                    current_hp: entity.current_hp,
                    max_hp: entity.max_hp,
                    hp_percent,
                    current_shield: entity.current_shield,
                    max_shield: entity.max_shield,
                    shield_percent,
                    breaking_stage: entity.breaking_stage,
                    dead: entity.dead,
                    stale: now.saturating_sub(entity.last_observed_micros)
                        > ENTITY_STALE_AFTER_MICROS,
                    debuffs,
                }
            });
        let local_y = local_actor_id.and_then(|actor_id| {
            entities
                .iter()
                .find(|entity| entity.actor_id == actor_id)
                .map(|entity| entity.y)
        });
        let raid_arena = raid_arena_spec(self.client_build.as_deref(), self.scene_id, local_y);
        let absolute_map = scene_map.or(raid_arena);
        MechanicsMapSnapshot {
            schema_version: MECHANICS_MAP_SCHEMA_VERSION,
            revision: self.revision,
            session_id: self.session_id.clone(),
            client_build: self.client_build.clone(),
            scene_id: self.scene_id,
            map_id: self.map_id,
            scene_name: self.scene_id.and_then(|scene_id| {
                self.client_build.as_deref().and_then(|client_build| {
                    rlogs_game_bpsr::localized_scene_name_for_build(
                        "global",
                        client_build,
                        i64::from(scene_id),
                        "en-US",
                    )
                    .ok()
                    .flatten()
                    .map(str::to_owned)
                })
            }),
            map_model: if absolute_map.is_some() {
                "absolute_scene_map"
            } else {
                "player_relative_radar"
            },
            // The raid's packet height still selects its mechanic layout, but
            // positions use the full game-owned scene map transform when that
            // texture is available.
            map_layout: raid_arena
                .and_then(|spec| spec.layout)
                .or_else(|| scene_map.and_then(|spec| spec.layout)),
            world_radius: MINIMAP_WORLD_RADIUS,
            map_origin_x: absolute_map.map(|spec| spec.origin_x),
            map_origin_z: absolute_map.map(|spec| spec.origin_z),
            map_span_x: absolute_map.map(|spec| spec.span_x),
            map_span_z: absolute_map.map(|spec| spec.span_z),
            background_asset_url: match (self.client_build.as_ref(), scene_map) {
                (Some(build), Some(spec)) => spec
                    .asset_file
                    .map(|asset| format!("/local-game-assets/{build}/{asset}")),
                _ => None,
            },
            local_actor_id,
            local_position_observed,
            player,
            party,
            action_controls,
            resources,
            dungeon: self
                .dungeon
                .as_ref()
                .map(|state| dungeon_hud_snapshot(state, now)),
            encounter_pack: pack,
            encounter_pack_reviewed: pack.is_some(),
            target,
            entities,
            mechanics,
            markers: self
                .markers
                .values()
                .chain(self.local_markers.values())
                .take(64)
                .cloned()
                .collect(),
            data_gap: self.data_gap.clone(),
            last_event_sequence: self.last_event_sequence,
            last_observed_micros: self.last_observed_micros,
        }
    }

    /// Replaces the provisional inspection-only marker layer. These values are
    /// never fed back into canonical encounter or submission reducers.
    pub fn replace_local_markers(
        &mut self,
        markers: impl IntoIterator<Item = rlogs_game_bpsr::LocalMapMarker>,
    ) -> bool {
        let next = markers
            .into_iter()
            .map(|marker| {
                let related_actor_id = marker.related_entity_uuid.and_then(|uuid| {
                    self.entities
                        .values()
                        .find(|entity| entity.actor.entity_uuid.0 == uuid)
                        .map(|entity| entity.actor.actor_id.0)
                });
                (
                    marker.passive_instance_id,
                    MechanicsMapMarker {
                        marker_id: None,
                        marker_number: Some(marker.marker_number),
                        related_actor_id,
                        x: marker.x,
                        y: marker.y,
                        z: marker.z,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        if self.local_markers == next {
            return false;
        }
        self.local_markers = next;
        self.revision = self.revision.saturating_add(1);
        true
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

    fn source_is_owned_by_local_player(
        &self,
        source_actor_id: u64,
        local_actor_id: Option<u64>,
    ) -> bool {
        let Some(local_actor_id) = local_actor_id else {
            return false;
        };
        let mut current = source_actor_id;
        for _ in 0..8 {
            if current == local_actor_id {
                return true;
            }
            let Some(owner_entity_uuid) = self
                .entities
                .get(&current)
                .and_then(|entity| entity.owner_entity_uuid)
            else {
                return false;
            };
            let Some(owner) = self
                .entities
                .values()
                .find(|entity| entity.actor.entity_uuid.0 == owner_entity_uuid)
            else {
                return false;
            };
            let next = owner.actor.actor_id.0;
            if next == current {
                return false;
            }
            current = next;
        }
        false
    }

    fn presented_statuses_for_actor(
        &self,
        actor_id: u64,
        now: u64,
        required_icon_segment: &str,
        limit: usize,
    ) -> Vec<TargetFrameDebuff> {
        let mut statuses = self
            .target_statuses
            .values()
            .filter(|status| status.target.actor_id.0 == actor_id)
            .filter(|status| {
                status
                    .duration_millis
                    .filter(|duration| *duration > 0)
                    .is_none_or(|duration| {
                        now.saturating_sub(status.applied_at_micros)
                            <= duration.saturating_mul(1_000)
                    })
            })
            .filter_map(|status| {
                let presentation = rlogs_game_bpsr::status_effect_presentation(status.effect_id)
                    .ok()
                    .flatten()?;
                let icon = presentation.icon.as_deref()?;
                if !icon.replace('\\', "/").contains(required_icon_segment) {
                    return None;
                }
                Some(TargetFrameDebuff {
                    effect_id: status.effect_id,
                    instance_id: status.instance_id,
                    presentation_name: rlogs_game_bpsr::localized_status_effect_name(
                        status.effect_id,
                        "en-US",
                    )
                    .ok()
                    .flatten()
                    .map(str::to_owned)
                    .or_else(|| presentation.technical_name.clone()),
                    icon_asset_path: Some(format!(
                        "/game-assets/blue-protocol-star-resonance/shared/{icon}"
                    )),
                    source_actor_id: status.source.map(|source| source.actor_id.0),
                    source_display_name: status.source.and_then(|source| {
                        self.entities.get(&source.actor_id.0).and_then(|entity| {
                            entity.display_name.clone().or_else(|| {
                                entity.monster_id.and_then(|monster_id| {
                                    rlogs_game_bpsr::localized_monster_name(monster_id, "en-US")
                                        .ok()
                                        .flatten()
                                        .map(str::to_owned)
                                })
                            })
                        })
                    }),
                    owned_by_local_player: status.source.is_some_and(|source| {
                        self.source_is_owned_by_local_player(
                            source.actor_id.0,
                            self.local_actor_id(),
                        )
                    }),
                    stacks: status.stacks,
                    duration_millis: status.duration_millis,
                    remaining_millis: status.duration_millis.map(|duration| {
                        duration
                            .saturating_sub(now.saturating_sub(status.applied_at_micros) / 1_000)
                    }),
                    applied_at_micros: status.applied_at_micros,
                })
            })
            .collect::<Vec<_>>();
        statuses.sort_by_key(|status| (status.effect_id, status.instance_id));
        statuses.truncate(limit);
        statuses
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
                self.attack_targets.remove(&actor_id);
                self.target_statuses
                    .retain(|_, status| status.target.actor_id.0 != actor_id);
                self.signals
                    .retain(|_, signal| signal.target.actor_id.0 != actor_id);
                self.resource_values
                    .retain(|(resource_actor_id, _), _| *resource_actor_id != actor_id);
            }
        }
        while self.target_statuses.len() > MAX_TARGET_STATUSES {
            let Some(oldest) = self
                .target_statuses
                .iter()
                .min_by_key(|(_, status)| status.applied_at_micros)
                .map(|(key, _)| *key)
            else {
                break;
            };
            self.target_statuses.remove(&oldest);
        }
        while self.cooldowns.len() > MAX_LOCAL_COOLDOWNS.saturating_mul(MAX_ENTITIES) {
            let Some(oldest) = self
                .cooldowns
                .iter()
                .min_by_key(|(_, cooldown)| cooldown.observed_at_micros)
                .map(|(key, _)| *key)
            else {
                break;
            };
            self.cooldowns.remove(&oldest);
        }
        while self.resource_values.len() > MAX_RESOURCE_VALUES {
            self.resource_values.pop_first();
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

fn dungeon_hud_snapshot(state: &DungeonHudState, now_micros: u64) -> DungeonHudSnapshot {
    let attempt_elapsed_micros =
        state
            .attempt_started_micros
            .map_or(state.attempt_elapsed_micros, |started| {
                state
                    .attempt_elapsed_micros
                    .saturating_add(now_micros.saturating_sub(started))
            });
    DungeonHudSnapshot {
        dungeon_id: state.dungeon_id,
        instance_id: state.instance_id.clone(),
        difficulty_id: state.difficulty_id,
        state: dungeon_event_state(state.state),
        flow_phase: state
            .flow
            .as_ref()
            .and_then(|flow| flow.phase)
            .map(dungeon_flow_phase),
        flow_state_id: state.flow.as_ref().and_then(|flow| flow.state_id),
        result_id: state.flow.as_ref().and_then(|flow| flow.result_id),
        attempt_number: state.attempt_number,
        retry_count: state.retry_count,
        encounter_state: state.encounter_state.map(encounter_state),
        attempt_elapsed_micros,
        attempt_running: state.attempt_started_micros.is_some(),
        objectives: state
            .objectives
            .values()
            .take(MAX_DUNGEON_OBJECTIVES)
            .cloned()
            .collect(),
    }
}

fn dungeon_hud_state_equal(left: &DungeonHudState, right: &DungeonHudState) -> bool {
    left.dungeon_id == right.dungeon_id
        && left.instance_id == right.instance_id
        && left.difficulty_id == right.difficulty_id
        && left.state == right.state
        && left.flow == right.flow
        && left.attempt_number == right.attempt_number
        && left.retry_count == right.retry_count
        && left.encounter_state == right.encounter_state
        && left.attempt_started_micros == right.attempt_started_micros
        && left.attempt_elapsed_micros == right.attempt_elapsed_micros
        && left.objectives == right.objectives
}

fn observe_dungeon_encounter_boundary(
    state: &mut DungeonHudState,
    next: EncounterState,
    observed_micros: u64,
) {
    match next {
        EncounterState::Started => {
            if state.encounter_state != Some(EncounterState::Started) {
                state.attempt_number = state.attempt_number.saturating_add(1).max(1);
                state.attempt_elapsed_micros = 0;
                state.attempt_started_micros = Some(observed_micros);
            }
            state.encounter_state = Some(EncounterState::Started);
        }
        EncounterState::Cleared | EncounterState::Wiped | EncounterState::Ended => {
            if let Some(started) = state.attempt_started_micros.take() {
                state.attempt_elapsed_micros = state
                    .attempt_elapsed_micros
                    .saturating_add(observed_micros.saturating_sub(started));
                if next == EncounterState::Wiped {
                    state.retry_count = state.retry_count.saturating_add(1);
                }
            }
            state.encounter_state = Some(next);
        }
    }
}

fn encounter_state(value: EncounterState) -> &'static str {
    match value {
        EncounterState::Started => "started",
        EncounterState::Cleared => "cleared",
        EncounterState::Wiped => "wiped",
        EncounterState::Ended => "ended",
    }
}

fn dungeon_event_state(value: DungeonEventKind) -> &'static str {
    match value {
        DungeonEventKind::Entered => "entered",
        DungeonEventKind::Started => "started",
        DungeonEventKind::FlowUpdated => "flow_updated",
        DungeonEventKind::Ended => "ended",
        DungeonEventKind::ObjectiveUpdated => "objective_updated",
        DungeonEventKind::ObjectiveRemoved => "objective_removed",
        DungeonEventKind::BossEngaged => "boss_engaged",
        DungeonEventKind::BossDefeated => "boss_defeated",
        DungeonEventKind::Completed => "completed",
        DungeonEventKind::Failed => "failed",
        DungeonEventKind::Exited => "exited",
    }
}

fn dungeon_flow_phase(value: DungeonFlowPhase) -> &'static str {
    match value {
        DungeonFlowPhase::Null => "null",
        DungeonFlowPhase::Active => "active",
        DungeonFlowPhase::Ready => "ready",
        DungeonFlowPhase::Playing => "playing",
        DungeonFlowPhase::End => "end",
        DungeonFlowPhase::Settlement => "settlement",
        DungeonFlowPhase::Vote => "vote",
        DungeonFlowPhase::Unknown(_) => "unknown",
    }
}

fn objective_resolution(value: Option<DungeonObjectiveCatalogResolution>) -> &'static str {
    match value {
        Some(DungeonObjectiveCatalogResolution::ResolvedCurrentBuild) => "resolved_current_build",
        Some(DungeonObjectiveCatalogResolution::UnresolvedCurrentBuild) => {
            "unresolved_current_build"
        }
        Some(DungeonObjectiveCatalogResolution::CatalogNotConfigured) => "catalog_not_configured",
        Some(DungeonObjectiveCatalogResolution::CatalogUnavailable) => "catalog_unavailable",
        None => "not_observed",
    }
}

fn observed_percent(current: Option<i64>, maximum: Option<i64>) -> Option<f64> {
    current.zip(maximum).and_then(|(current, maximum)| {
        (maximum > 0).then(|| ((current.max(0) as f64 / maximum as f64) * 100.0).clamp(0.0, 100.0))
    })
}

fn action_control_snapshot(cooldown: &CooldownState, now_micros: u64) -> ActionControlSnapshot {
    let presentation_ability_id = [
        cooldown.skill_level_id,
        cooldown.skill_level_id.checked_div(100).unwrap_or_default(),
    ]
    .into_iter()
    .filter(|ability_id| *ability_id > 0)
    .find(|ability_id| {
        rlogs_game_bpsr::combat_action_presentation(*ability_id)
            .ok()
            .flatten()
            .is_some()
    });
    let presentation = presentation_ability_id.and_then(|ability_id| {
        rlogs_game_bpsr::combat_action_presentation(ability_id)
            .ok()
            .flatten()
    });
    let elapsed_millis = now_micros.saturating_sub(cooldown.observed_at_micros) / 1_000;
    let remaining_millis = cooldown
        .duration_millis
        .filter(|duration| *duration >= 0)
        .map(|duration| (duration as u64).saturating_sub(elapsed_millis));
    ActionControlSnapshot {
        skill_level_id: cooldown.skill_level_id,
        presentation_ability_id,
        presentation_name: presentation_ability_id.and_then(|ability_id| {
            rlogs_game_bpsr::localized_combat_action_name(ability_id, "en-US")
                .ok()
                .flatten()
                .map(str::to_owned)
        }),
        icon_asset_path: presentation.and_then(|presentation| {
            presentation
                .icon
                .as_deref()
                .map(|icon| format!("/game-assets/blue-protocol-star-resonance/shared/{icon}"))
        }),
        duration_millis: cooldown.duration_millis,
        remaining_millis,
        cooldown_type: cooldown.cooldown_type,
        charge_count: cooldown.charge_count,
        observed_at_micros: cooldown.observed_at_micros,
    }
}

#[derive(Debug, Clone, Copy)]
struct ResourceHudSpec {
    kind: &'static str,
    label: &'static str,
    current_id: u32,
    max_id: u32,
}

fn resource_hud_specs(class_id: i32) -> &'static [ResourceHudSpec] {
    match class_id {
        1 => &[
            ResourceHudSpec {
                kind: "bar",
                label: "Blade Intent",
                current_id: 12_051,
                max_id: 12_057,
            },
            ResourceHudSpec {
                kind: "charges",
                label: "Thunder Sigil",
                current_id: 12_041,
                max_id: 12_047,
            },
        ],
        2 => &[
            ResourceHudSpec {
                kind: "bar",
                label: "Energy",
                current_id: 12_001,
                max_id: 12_007,
            },
            ResourceHudSpec {
                kind: "charges",
                label: "Sharpness",
                current_id: 12_021,
                max_id: 12_027,
            },
        ],
        3 => &[
            ResourceHudSpec {
                kind: "bar",
                label: "Flame Soul",
                current_id: 13_011,
                max_id: 13_017,
            },
            ResourceHudSpec {
                kind: "charges",
                label: "Frenzy",
                current_id: 13_001,
                max_id: 13_007,
            },
        ],
        4 => &[
            ResourceHudSpec {
                kind: "bar",
                label: "Energy",
                current_id: 14_011,
                max_id: 14_017,
            },
            ResourceHudSpec {
                kind: "charges",
                label: "Sharpness",
                current_id: 14_001,
                max_id: 14_007,
            },
        ],
        5 => &[
            ResourceHudSpec {
                kind: "bar",
                label: "Energy",
                current_id: 15_001,
                max_id: 15_007,
            },
            ResourceHudSpec {
                kind: "charges",
                label: "Flower",
                current_id: 15_011,
                max_id: 15_017,
            },
        ],
        _ => &[],
    }
}

fn is_reviewed_hud_resource_id(resource_id: u32) -> bool {
    (1..=5).any(|class_id| {
        resource_hud_specs(class_id)
            .iter()
            .any(|spec| spec.current_id == resource_id || spec.max_id == resource_id)
    })
}

fn resource_hud_snapshots(
    class_id: i32,
    actor_id: u64,
    values: &BTreeMap<(u64, u32), u32>,
) -> Vec<ResourceHudSnapshot> {
    resource_hud_specs(class_id)
        .iter()
        .filter_map(|spec| {
            let current = *values.get(&(actor_id, spec.current_id))?;
            let max = *values.get(&(actor_id, spec.max_id))?;
            Some(ResourceHudSnapshot {
                kind: spec.kind,
                label: spec.label,
                current_id: spec.current_id,
                max_id: spec.max_id,
                current,
                max,
                percent: (max > 0).then(|| (f64::from(current) / f64::from(max)) * 100.0),
            })
        })
        .collect()
}

#[derive(Debug, Clone, Copy)]
struct SceneMapSpec {
    asset_file: Option<&'static str>,
    layout: Option<&'static str>,
    origin_x: f32,
    origin_z: f32,
    span_x: f32,
    span_z: f32,
}

#[derive(Debug, Deserialize)]
struct PackagedSceneMapManifest<'a> {
    schema_version: u16,
    #[serde(borrow)]
    builds: BTreeMap<&'a str, Vec<PackagedSceneMapEntry<'a>>>,
}

#[derive(Debug, Deserialize)]
struct PackagedSceneMapEntry<'a> {
    scene_ids: Vec<i32>,
    #[serde(borrow)]
    asset: &'a str,
    origin_x: f32,
    origin_z: f32,
    span_x: f32,
    span_z: f32,
}

const PACKAGED_SCENE_MAP_MANIFEST: &str =
    include_str!("../../desktop-tauri/resources/map-compiler/reviewed-map-assets.v1.json");

fn packaged_scene_maps() -> &'static PackagedSceneMapManifest<'static> {
    static MANIFEST: OnceLock<PackagedSceneMapManifest<'static>> = OnceLock::new();
    MANIFEST.get_or_init(|| {
        let manifest: PackagedSceneMapManifest<'static> =
            serde_json::from_str(PACKAGED_SCENE_MAP_MANIFEST)
                .expect("packaged reviewed-map-assets.v1.json must be valid");
        assert_eq!(
            manifest.schema_version, 1,
            "unsupported packaged map manifest"
        );
        manifest
    })
}

fn raid_arena_spec(
    build: Option<&str>,
    scene_id: Option<i32>,
    local_y: Option<f32>,
) -> Option<SceneMapSpec> {
    if build != Some("24687926") || !matches!(scene_id, Some(13021..=13023)) {
        return None;
    }
    Some(if local_y.is_some_and(|y| y >= 275.0) {
        SceneMapSpec {
            asset_file: None,
            layout: Some("raid_grid"),
            origin_x: -30.0,
            origin_z: -27.0,
            span_x: 60.0,
            span_z: 54.0,
        }
    } else {
        SceneMapSpec {
            asset_file: None,
            layout: Some("raid_ring"),
            origin_x: -55.0,
            origin_z: -55.0,
            span_x: 110.0,
            span_z: 110.0,
        }
    })
}

fn scene_map_spec(build: Option<&str>, scene_id: Option<i32>) -> Option<SceneMapSpec> {
    let scene_id = scene_id?;
    let build = build?;
    let maps = packaged_scene_maps();
    let entries = maps.builds.get(build).or_else(|| {
        let requested = build.parse::<u64>().ok()?;
        maps.builds
            .iter()
            .filter_map(|(candidate, entries)| Some((candidate.parse::<u64>().ok()?, entries)))
            .filter(|(candidate, _)| *candidate <= requested)
            .max_by_key(|(candidate, _)| *candidate)
            .map(|(_, entries)| entries)
    });
    let entry = entries?
        .iter()
        .find(|entry| entry.scene_ids.contains(&scene_id))?;
    Some(SceneMapSpec {
        asset_file: Some(entry.asset),
        layout: None,
        origin_x: entry.origin_x,
        origin_z: entry.origin_z,
        span_x: entry.span_x,
        span_z: entry.span_z,
    })
}

fn encounter_pack(client_build: Option<&str>, scene_id: Option<i32>) -> Option<&'static str> {
    if client_build != Some("24687926") {
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
    if client_build != Some("24687926") {
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
    if client_build != Some("24687926") {
        return None;
    }
    match (scene_id?, monster_id?) {
        (6513..=6515, 33901) | (1631..=1633, 33701) => Some("boss"),
        (6513..=6515, 33904 | 33905) => Some("tower"),
        (6513..=6515, 33908 | 33921) => Some("left_clone"),
        (6513..=6515, 33909 | 33922) => Some("right_clone"),
        (1150..=1152, 2106) => Some("correct_portal"),
        (1150..=1152, 2107) => Some("other_portal"),
        (1631..=1633, 300086) => Some("pizza_slow"),
        (1631..=1633, 300089) => Some("pizza_fast"),
        (6563..=6565, 4639) => Some("matrix_rune"),
        (6563..=6565, 3340219) => Some("ice_wave"),
        (6563..=6565, 3340220) => Some("water_wave"),
        (6563..=6565, 4604) => Some("ice_orb"),
        (6563..=6565, 4605) => Some("water_orb"),
        (13021..=13023, 10330051) => Some("pinball"),
        (13021..=13023, 10310062) => Some("ring_inner"),
        (13021..=13023, 10310063) => Some("ring_middle"),
        (13021..=13023, 10310064) => Some("ring_outer"),
        _ => None,
    }
}

fn reviewed_mechanic_signal_kind(
    client_build: Option<&str>,
    scene_id: Option<i32>,
    effect_id: i64,
) -> Option<&'static str> {
    if client_build != Some("24687926") {
        return None;
    }
    match (scene_id?, effect_id) {
        (6513..=6515, 884101 | 884106 | 884122) => Some("tower_activating"),
        (6513..=6515, 884102) => Some("tower_blue_complete"),
        (6513..=6515, 884103) => Some("tower_gold_complete"),
        (6513..=6515, 884129) => Some("energy_pillar"),
        (6513..=6515, 884141) => Some("energy_pillar_short"),
        (6513..=6515, 884162) => Some("charge_target_left"),
        (6513..=6515, 884163) => Some("charge_target_right"),
        (6513..=6515, 884168) => Some("charge_target_random"),
        (6513..=6515, 884169) => Some("puzzle_piece_one"),
        (6513..=6515, 884170) => Some("puzzle_piece_two"),
        (6513..=6515, -3390117 | -3390123) => Some("clone_charge_left"),
        (6513..=6515, -3390118 | -3390124) => Some("clone_charge_right"),
        (1150..=1152, 821076) => Some("sticky_bomb"),
        (1150..=1152, -111103) => Some("gravity_blast"),
        (1631..=1633, 510571) => Some("heavy_wound"),
        (1631..=1633, 841519) => Some("void_corruption_binding"),
        (1631..=1633, 841509) => Some("wudi_slash_order"),
        (6563..=6565, 883707) => Some("matrix_rune_a"),
        (6563..=6565, 883708) => Some("matrix_rune_b"),
        (6563..=6565, 883709) => Some("matrix_rune_c"),
        (6563..=6565, 883710) => Some("matrix_rune_d"),
        (6563..=6565, 883714) => Some("matrix_initializer"),
        (6563..=6565, 883601) => Some("death_sentence_target"),
        (6563..=6565, 522602) => Some("matrix_callout"),
        (6563..=6565, 883602) => Some("double_echo_ice"),
        (6563..=6565, 883603) => Some("double_echo_water"),
        (6563..=6565, 883605) => Some("dual_element_gravity"),
        (6563..=6565, 883631) => Some("ice_water_floor"),
        (6563..=6565, 883633) => Some("pizza_orange"),
        (6563..=6565, 883634) => Some("pizza_purple"),
        (6563..=6565, -3340245) => Some("pizza_indicator"),
        (13021..=13023, 829104) => Some("electromagnetic_pulse_a"),
        (13021..=13023, 829105) => Some("electromagnetic_pulse_b"),
        (13021..=13023, 829106) => Some("electromagnetic_pulse_c"),
        (13021..=13023, 829115) => Some("share"),
        (13021..=13023, 829116) => Some("mirage_share"),
        (13021..=13023, 829214) => Some("phase_edge"),
        (13021..=13023, 829215) => Some("phase_corner"),
        (13021..=13023, 829217) => Some("normal_target"),
        (13021..=13023, 829245) => Some("decay_target"),
        (13021..=13023, 829226) => Some("hit_order_one"),
        (13021..=13023, 829227) => Some("hit_order_two"),
        (13021..=13023, 829228) => Some("hit_order_three"),
        (13021..=13023, 829304) => Some("normal_share"),
        (13021..=13023, 829305) => Some("mirage_share_callout"),
        (13021..=13023, 829306) => Some("normal_decay"),
        (13021..=13023, 829307) => Some("mirage_decay"),
        (13021..=13023, 829308) => Some("normal_spread"),
        (13021..=13023, 829309) => Some("mirage_spread"),
        (13021..=13023, 829314) => Some("pinball_countdown"),
        (13021..=13023, 829316) => Some("causal_jump"),
        (13021..=13023, 829318) => Some("floor_link"),
        (13021..=13023, 829323) => Some("divine_sentence"),
        (13021..=13023, 829324) => Some("cumulative_sentence"),
        (13021..=13023, 829326) => Some("mirage_sentence"),
        (13021..=13023, 829327) => Some("return_top_left"),
        (13021..=13023, 829328) => Some("return_middle_left"),
        (13021..=13023, 829329) => Some("return_bottom_left"),
        (13021..=13023, 829330) => Some("return_top_right"),
        (13021..=13023, 829331) => Some("return_middle_right"),
        (13021..=13023, 829332) => Some("return_bottom_right"),
        (13021..=13023, 829372) => Some("return_count_one"),
        (13021..=13023, 829373) => Some("return_count_two"),
        (13021..=13023, 829374) => Some("return_count_three"),
        (13021..=13023, -10310062) => Some("ring_inner"),
        (13021..=13023, -10310063) => Some("ring_middle"),
        (13021..=13023, -10310064) => Some("ring_outer"),
        (6615, 884609) => Some("near_chain"),
        (6615, 884610) => Some("far_chain"),
        (6615, 884614) => Some("wheel_blue"),
        (6615, 884615) => Some("wheel_red"),
        (6615, 884616) => Some("wheel_doom"),
        (6615, 884641) => Some("energy_target"),
        (6615, 884659) => Some("pair_mark"),
        (6615, 884660) => Some("pair_settle"),
        (6615, 884661) => Some("pair_penalty"),
        (6615, 884664) => Some("pair_swap"),
        (6615, -470112) => Some("near_chain_cast"),
        (6615, -470113) => Some("far_chain_cast"),
        (6615, -470119) => Some("shadow_cast"),
        (6615, -470125) => Some("pair_settle_cast"),
        (6615, -470132) => Some("pair_resolve_cast"),
        _ => None,
    }
}

fn is_reviewed_mechanic_cast(
    client_build: Option<&str>,
    scene_id: Option<i32>,
    ability_id: i64,
) -> bool {
    if client_build != Some("24687926") {
        return false;
    }
    let ids: &[i64] = match scene_id {
        Some(6513..=6515) => &[3390117, 3390118, 3390123, 3390124],
        Some(1150..=1152) => &[111103],
        Some(6563..=6565) => &[3340245],
        Some(13021..=13023) => &[10310062, 10310063, 10310064],
        Some(6615) => &[470112, 470113, 470119, 470125, 470132],
        _ => &[],
    };
    ids.contains(&ability_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rlogs_events::{
        ActorEvent, ActorId, ActorLoadoutObservation, CharacterIdentity, DungeonEvent,
        DungeonFlowSnapshot, DungeonId, DungeonObjectiveCatalogReference, EntityAttribute,
        EntityAttributeEvent, EntityUuid, EventProvenance, EventSensitivity, EventTime,
        EvidenceConfidence, EvidenceSource, GameProfileEvent, RegionContext, RegionIdentity,
        ResourceEvent, SceneId, StatusEffectId, StatusEffectInstanceId, StatusEvent, TimelineEvent,
    };

    #[test]
    fn local_marker_layer_preserves_number_and_removal() {
        let mut projector = MechanicsMapProjector::default();
        assert!(
            projector.replace_local_markers([rlogs_game_bpsr::LocalMapMarker {
                passive_instance_id: 77,
                related_entity_uuid: None,
                marker_number: 4,
                x: Some(12.0),
                y: Some(0.0),
                z: Some(-8.0),
            }])
        );
        assert_eq!(projector.snapshot().markers[0].marker_number, Some(4));
        assert!(projector.replace_local_markers([]));
        assert!(projector.snapshot().markers.is_empty());
    }

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
                client_build: "24687926".into(),
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

    fn encounter_boundary(sequence: u64, state: EncounterState) -> CanonicalEvent {
        CanonicalEvent::Timeline(TimelineEvent {
            sequence,
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
            kind: TimelineEventKind::EncounterBoundary {
                state,
                encounter_id: None,
                reason: rlogs_events::BoundaryReason::AuthoritativePacket,
            },
        })
    }

    fn actor_event(actor: EntityRef, kind: ActorKind, character_id: Option<&str>) -> ActorEvent {
        ActorEvent {
            actor,
            state: ActorState::Spawned,
            entity_type_id: if kind == ActorKind::Player { 10 } else { 11 },
            kind,
            monster_id: (kind == ActorKind::Monster).then_some(rlogs_events::MonsterId(123)),
            character_id: character_id.map(str::to_owned),
            display_name: (kind == ActorKind::Player).then(|| "Local".into()),
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
        }
    }

    #[test]
    fn packet_build_transition_replaces_a_bootstrap_map_identity() {
        let mut projector = MechanicsMapProjector::default();
        projector.reset("session", "unverified");
        projector.scene_id = Some(6_565);
        let mut event = envelope(
            1,
            CanonicalEvent::Timeline(TimelineEvent {
                sequence: 1,
                time: EventTime {
                    observed_micros: 1_000,
                    game_time_millis: None,
                },
                provenance: EventProvenance {
                    confidence: EvidenceConfidence::Exact,
                    source: EvidenceSource::Wire {
                        capture_sequence: 1,
                        connection_id: 1,
                        stream_id: 1,
                    },
                },
                kind: TimelineEventKind::Actor(actor_event(
                    entity(7, 70),
                    ActorKind::Player,
                    Some("7"),
                )),
            }),
        );
        event.region.client_build = "24687927".into();

        assert!(projector.observe(&event));
        assert_eq!(
            projector.snapshot().client_build.as_deref(),
            Some("24687927")
        );
        assert_eq!(projector.snapshot().scene_name, None);
    }

    #[test]
    fn class_resources_require_exact_current_and_max_packet_ids() {
        let actor_id = 7;
        let values = BTreeMap::from([
            ((actor_id, 14_011), 72),
            ((actor_id, 14_017), 100),
            ((actor_id, 14_001), 4),
            ((actor_id, 14_007), 5),
        ]);
        let resources = resource_hud_snapshots(4, actor_id, &values);
        assert_eq!(resources.len(), 2);
        assert_eq!(resources[0].label, "Energy");
        assert_eq!(resources[0].percent, Some(72.0));
        assert_eq!(resources[1].label, "Sharpness");
        assert_eq!(resources[1].current, 4);
        assert!(is_reviewed_hud_resource_id(14_011));
        assert!(!is_reviewed_hud_resource_id(99_999));

        let incomplete = BTreeMap::from([((actor_id, 14_011), 72)]);
        assert!(resource_hud_snapshots(4, actor_id, &incomplete).is_empty());
        assert!(resource_hud_snapshots(11, actor_id, &values).is_empty());
    }

    #[test]
    fn mismatched_parallel_resource_arrays_are_never_zipped() {
        let mut projector = MechanicsMapProjector::default();
        projector.observe(&envelope(
            1,
            CanonicalEvent::Timeline(TimelineEvent {
                sequence: 1,
                time: EventTime {
                    observed_micros: 1_000,
                    game_time_millis: None,
                },
                provenance: EventProvenance::wire(1, 1, 1),
                kind: TimelineEventKind::Resource(ResourceEvent {
                    actor: entity(7, 42),
                    update_kind: EntityAttributeUpdateKind::Snapshot,
                    origin_energy_raw_bits: None,
                    resource_ids: vec![14_011, 14_017],
                    resource_values: vec![72],
                    cooldowns: vec![],
                }),
            }),
        ));
        assert!(projector.resource_values.is_empty());
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
    fn target_frame_uses_selected_target_hp_and_packet_classified_debuffs() {
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
        let local = entity(7, 42 << 16);
        let target = entity(8, 800);
        let teammate = entity(9, 900);
        for (sequence, actor) in [
            (2, actor_event(local, ActorKind::Player, Some("42"))),
            (3, actor_event(target, ActorKind::Monster, None)),
            (4, actor_event(teammate, ActorKind::Player, Some("43"))),
        ] {
            projector.observe(&envelope(
                sequence,
                CanonicalEvent::Timeline(TimelineEvent {
                    sequence,
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
                    kind: TimelineEventKind::Actor(actor),
                }),
            ));
        }
        projector.observe(&envelope(
            4,
            CanonicalEvent::Timeline(TimelineEvent {
                sequence: 4,
                time: EventTime {
                    observed_micros: 4_000,
                    game_time_millis: None,
                },
                provenance: EventProvenance {
                    confidence: EvidenceConfidence::Exact,
                    source: EvidenceSource::Wire {
                        capture_sequence: 4,
                        connection_id: 1,
                        stream_id: 1,
                    },
                },
                kind: TimelineEventKind::EntityAttributes(EntityAttributeEvent {
                    actor: local,
                    update_kind: EntityAttributeUpdateKind::Delta,
                    ownership: None,
                    attributes: vec![
                        EntityAttribute {
                            attribute_id: ATTR_TARGET_ID,
                            raw_value: vec![0xa0, 0x06],
                            decoded: Some(EntityAttributeValue::Integer(800)),
                        },
                        EntityAttribute {
                            attribute_id: ATTR_CURRENT_HP,
                            raw_value: vec![],
                            decoded: Some(EntityAttributeValue::Integer(900)),
                        },
                        EntityAttribute {
                            attribute_id: ATTR_MAX_HP_FINAL,
                            raw_value: vec![],
                            decoded: Some(EntityAttributeValue::Integer(1_000)),
                        },
                        EntityAttribute {
                            attribute_id: ATTR_SHIELD_LIST,
                            raw_value: vec![
                                10, 17, 8, 240, 1, 16, 12, 24, 144, 147, 2, 32, 190, 201, 1, 40,
                                208, 141, 19,
                            ],
                            decoded: None,
                        },
                    ],
                }),
            }),
        ));
        projector.observe(&envelope(
            5,
            CanonicalEvent::Timeline(TimelineEvent {
                sequence: 5,
                time: EventTime {
                    observed_micros: 5_000,
                    game_time_millis: None,
                },
                provenance: EventProvenance {
                    confidence: EvidenceConfidence::Exact,
                    source: EvidenceSource::Wire {
                        capture_sequence: 5,
                        connection_id: 1,
                        stream_id: 1,
                    },
                },
                kind: TimelineEventKind::EntityAttributes(EntityAttributeEvent {
                    actor: target,
                    update_kind: EntityAttributeUpdateKind::Delta,
                    ownership: None,
                    attributes: vec![
                        EntityAttribute {
                            attribute_id: ATTR_CURRENT_HP,
                            raw_value: vec![],
                            decoded: Some(EntityAttributeValue::Integer(500)),
                        },
                        EntityAttribute {
                            attribute_id: ATTR_MAX_HP_FINAL,
                            raw_value: vec![],
                            decoded: Some(EntityAttributeValue::Integer(1_000)),
                        },
                        EntityAttribute {
                            attribute_id: ATTR_SHIELD_LIST,
                            raw_value: vec![
                                10, 17, 8, 240, 1, 16, 12, 24, 144, 147, 2, 32, 190, 201, 1, 40,
                                208, 141, 19,
                            ],
                            decoded: None,
                        },
                        EntityAttribute {
                            attribute_id: ATTR_BREAKING_STAGE,
                            raw_value: vec![],
                            decoded: Some(EntityAttributeValue::Integer(0)),
                        },
                    ],
                }),
            }),
        ));
        projector.observe(&envelope(
            6,
            CanonicalEvent::Timeline(TimelineEvent {
                sequence: 6,
                time: EventTime {
                    observed_micros: 6_000,
                    game_time_millis: None,
                },
                provenance: EventProvenance {
                    confidence: EvidenceConfidence::Exact,
                    source: EvidenceSource::Wire {
                        capture_sequence: 6,
                        connection_id: 1,
                        stream_id: 1,
                    },
                },
                kind: TimelineEventKind::Status(StatusEvent {
                    source: Some(local),
                    target,
                    effect: StatusEffectId(4_501),
                    instance_id: Some(StatusEffectInstanceId(99)),
                    origin: None,
                    state: StatusState::Applied,
                    stacks: Some(2),
                    duration_millis: Some(5_000),
                    level: Some(1),
                    part_id: None,
                    count: None,
                    created_at_millis: None,
                }),
            }),
        ));

        projector.observe(&envelope(
            7,
            CanonicalEvent::Timeline(TimelineEvent {
                sequence: 7,
                time: EventTime {
                    observed_micros: 6_000,
                    game_time_millis: None,
                },
                provenance: EventProvenance {
                    confidence: EvidenceConfidence::Exact,
                    source: EvidenceSource::Wire {
                        capture_sequence: 7,
                        connection_id: 1,
                        stream_id: 1,
                    },
                },
                kind: TimelineEventKind::Status(StatusEvent {
                    source: Some(local),
                    target: local,
                    effect: StatusEffectId(21_412),
                    instance_id: Some(StatusEffectInstanceId(100)),
                    origin: None,
                    state: StatusState::Applied,
                    stacks: Some(3),
                    duration_millis: Some(8_000),
                    level: Some(1),
                    part_id: None,
                    count: None,
                    created_at_millis: None,
                }),
            }),
        ));

        projector.observe(&envelope(
            8,
            CanonicalEvent::Timeline(TimelineEvent {
                sequence: 8,
                time: EventTime {
                    observed_micros: 6_500,
                    game_time_millis: None,
                },
                provenance: EventProvenance {
                    confidence: EvidenceConfidence::Exact,
                    source: EvidenceSource::Wire {
                        capture_sequence: 8,
                        connection_id: 1,
                        stream_id: 1,
                    },
                },
                kind: TimelineEventKind::Status(StatusEvent {
                    source: Some(teammate),
                    target,
                    effect: StatusEffectId(4_501),
                    instance_id: Some(StatusEffectInstanceId(101)),
                    origin: None,
                    state: StatusState::Applied,
                    stacks: Some(1),
                    duration_millis: Some(5_000),
                    level: Some(1),
                    part_id: None,
                    count: None,
                    created_at_millis: None,
                }),
            }),
        ));

        let snapshot = projector.snapshot();
        let player = snapshot.player.expect("local player frame");
        assert_eq!(player.actor_id, 7);
        assert_eq!(player.current_hp, Some(900));
        assert_eq!(player.max_hp, Some(1_000));
        assert_eq!(player.hp_percent, Some(90.0));
        assert_eq!(player.current_shield, Some(35_216));
        assert_eq!(player.max_shield, Some(313_040));
        assert_eq!(player.statuses.len(), 1);
        assert_eq!(player.statuses[0].effect_id, 21_412);
        assert_eq!(player.statuses[0].stacks, Some(3));
        let selected = snapshot.target.expect("selected target");
        assert_eq!(selected.actor_id, 8);
        assert_eq!(selected.current_hp, Some(500));
        assert_eq!(selected.max_hp, Some(1_000));
        assert_eq!(selected.hp_percent, Some(50.0));
        assert_eq!(selected.current_shield, Some(35_216));
        assert_eq!(selected.max_shield, Some(313_040));
        assert_eq!(selected.shield_percent, Some(11.249680552006133));
        assert_eq!(selected.breaking_stage, Some(0));
        assert_eq!(selected.debuffs.len(), 2);
        assert_eq!(selected.debuffs[0].effect_id, 4_501);
        assert_eq!(selected.debuffs[0].source_actor_id, Some(7));
        assert!(selected.debuffs[0].owned_by_local_player);
        assert_eq!(
            selected.debuffs[0].source_display_name.as_deref(),
            Some("Local")
        );
        assert_eq!(selected.debuffs[0].stacks, Some(2));
        assert_eq!(selected.debuffs[0].duration_millis, Some(5_000));
        assert_eq!(selected.debuffs[0].remaining_millis, Some(4_998));
        assert_eq!(selected.debuffs[1].source_actor_id, Some(9));
        assert!(!selected.debuffs[1].owned_by_local_player);

        projector.observe(&envelope(
            7,
            CanonicalEvent::Timeline(TimelineEvent {
                sequence: 7,
                time: EventTime {
                    observed_micros: 7_000,
                    game_time_millis: None,
                },
                provenance: EventProvenance {
                    confidence: EvidenceConfidence::Exact,
                    source: EvidenceSource::Wire {
                        capture_sequence: 7,
                        connection_id: 1,
                        stream_id: 1,
                    },
                },
                kind: TimelineEventKind::EntityAttributes(EntityAttributeEvent {
                    actor: target,
                    update_kind: EntityAttributeUpdateKind::Delta,
                    ownership: None,
                    attributes: vec![EntityAttribute {
                        attribute_id: ATTR_SHIELD_LIST,
                        raw_value: vec![0x0a, 0x02, 0x08],
                        decoded: None,
                    }],
                }),
            }),
        ));
        let malformed_shield = projector
            .snapshot()
            .target
            .expect("target remains selected");
        assert_eq!(malformed_shield.current_shield, None);
        assert_eq!(malformed_shield.max_shield, None);
        assert_eq!(malformed_shield.shield_percent, None);

        projector.observe(&envelope(
            8,
            CanonicalEvent::Timeline(TimelineEvent {
                sequence: 8,
                time: EventTime {
                    observed_micros: 8_000,
                    game_time_millis: None,
                },
                provenance: EventProvenance {
                    confidence: EvidenceConfidence::Exact,
                    source: EvidenceSource::Wire {
                        capture_sequence: 8,
                        connection_id: 1,
                        stream_id: 1,
                    },
                },
                kind: TimelineEventKind::Status(StatusEvent {
                    source: Some(local),
                    target,
                    effect: StatusEffectId(4_501),
                    instance_id: Some(StatusEffectInstanceId(99)),
                    origin: None,
                    state: StatusState::Removed,
                    stacks: None,
                    duration_millis: None,
                    level: Some(1),
                    part_id: None,
                    count: None,
                    created_at_millis: None,
                }),
            }),
        ));
        let remaining = projector
            .snapshot()
            .target
            .expect("target remains selected")
            .debuffs;
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].source_actor_id, Some(9));
        assert!(!remaining[0].owned_by_local_player);

        projector.observe(&envelope(
            9,
            CanonicalEvent::Timeline(TimelineEvent {
                sequence: 9,
                time: EventTime {
                    observed_micros: 9_000,
                    game_time_millis: None,
                },
                provenance: EventProvenance {
                    confidence: EvidenceConfidence::Exact,
                    source: EvidenceSource::Wire {
                        capture_sequence: 9,
                        connection_id: 1,
                        stream_id: 1,
                    },
                },
                kind: TimelineEventKind::EntityAttributes(EntityAttributeEvent {
                    actor: local,
                    update_kind: EntityAttributeUpdateKind::Delta,
                    ownership: None,
                    attributes: vec![EntityAttribute {
                        attribute_id: ATTR_TARGET_ID,
                        raw_value: vec![],
                        decoded: Some(EntityAttributeValue::Integer(0)),
                    }],
                }),
            }),
        ));
        assert!(projector.snapshot().target.is_none());
    }

    #[test]
    fn packet_owned_imagine_debuff_is_marked_as_the_local_players() {
        let mut projector = MechanicsMapProjector::default();
        projector.observe(&envelope(
            1,
            CanonicalEvent::CharacterProfileObserved {
                profile: Box::new(GameProfileEvent {
                    game_plugin_id: "game.rlogs.blue-protocol-star-resonance".into(),
                    payload_schema_id: "test".into(),
                    payload_schema_version: 1,
                    character: CharacterIdentity {
                        region: RegionIdentity {
                            deployment_id: "global".into(),
                            region_id: "north-america".into(),
                            realm_id: None,
                            world_id: None,
                        },
                        character_id: "42".into(),
                    },
                    payload: serde_json::json!({}),
                }),
            },
        ));
        let local = entity(7, 700);
        let imagine = entity(10, 1_000);
        for (sequence, actor) in [
            (2, actor_event(local, ActorKind::Player, Some("42"))),
            (3, actor_event(imagine, ActorKind::Pet, None)),
        ] {
            projector.observe(&envelope(
                sequence,
                CanonicalEvent::Timeline(TimelineEvent {
                    sequence,
                    time: EventTime {
                        observed_micros: sequence * 1_000,
                        game_time_millis: None,
                    },
                    provenance: EventProvenance::wire(sequence, 1, 1),
                    kind: TimelineEventKind::Actor(actor),
                }),
            ));
        }
        projector.observe(&envelope(
            4,
            CanonicalEvent::Timeline(TimelineEvent {
                sequence: 4,
                time: EventTime {
                    observed_micros: 4_000,
                    game_time_millis: None,
                },
                provenance: EventProvenance::wire(4, 1, 1),
                kind: TimelineEventKind::EntityAttributes(EntityAttributeEvent {
                    actor: imagine,
                    update_kind: EntityAttributeUpdateKind::Delta,
                    ownership: Some(ActorOwnershipUpdate::Confirmed {
                        owner_entity_uuid: local.entity_uuid,
                    }),
                    attributes: Vec::new(),
                }),
            }),
        ));

        assert!(projector.source_is_owned_by_local_player(10, Some(7)));
        assert!(!projector.source_is_owned_by_local_player(10, Some(9)));
    }

    #[test]
    fn action_control_preserves_packet_cooldown_and_uses_reviewed_skill_presentation() {
        let control = action_control_snapshot(
            &CooldownState {
                skill_level_id: 12_301,
                duration_millis: Some(10_000),
                cooldown_type: Some(2),
                charge_count: Some(1),
                observed_at_micros: 2_000_000,
            },
            4_500_000,
        );

        assert_eq!(control.skill_level_id, 12_301);
        assert_eq!(control.presentation_ability_id, Some(123));
        assert_eq!(control.duration_millis, Some(10_000));
        assert_eq!(control.remaining_millis, Some(7_500));
        assert_eq!(control.cooldown_type, Some(2));
        assert_eq!(control.charge_count, Some(1));
        assert_eq!(control.observed_at_micros, 2_000_000);
    }

    #[test]
    fn party_frames_require_rostered_joined_actors_and_packet_vitals() {
        let mut projector = MechanicsMapProjector::default();
        let region = RegionIdentity {
            deployment_id: "global".into(),
            region_id: "north-america".into(),
            realm_id: None,
            world_id: None,
        };
        projector.observe(&envelope(
            1,
            CanonicalEvent::CharacterProfileObserved {
                profile: Box::new(GameProfileEvent {
                    game_plugin_id: "game.rlogs.blue-protocol-star-resonance".into(),
                    payload_schema_id: "test".into(),
                    payload_schema_version: 1,
                    character: CharacterIdentity {
                        region: region.clone(),
                        character_id: "42".into(),
                    },
                    payload: serde_json::json!({}),
                }),
            },
        ));
        projector.observe(&envelope(
            2,
            CanonicalEvent::PartyChanged {
                members: vec![CharacterIdentity {
                    region,
                    character_id: "84".into(),
                }],
            },
        ));
        let local = entity(7, 42 << 16);
        let teammate = entity(8, 84 << 16);
        projector.observe(&envelope(
            3,
            CanonicalEvent::Timeline(TimelineEvent {
                sequence: 3,
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
                kind: TimelineEventKind::Actor(actor_event(local, ActorKind::Player, Some("42"))),
            }),
        ));
        let mut teammate_actor = actor_event(teammate, ActorKind::Player, Some("84"));
        teammate_actor.display_name = Some("Teammate".into());
        projector.observe(&envelope(
            4,
            CanonicalEvent::Timeline(TimelineEvent {
                sequence: 4,
                time: EventTime {
                    observed_micros: 4_000,
                    game_time_millis: None,
                },
                provenance: EventProvenance {
                    confidence: EvidenceConfidence::Exact,
                    source: EvidenceSource::Wire {
                        capture_sequence: 4,
                        connection_id: 1,
                        stream_id: 1,
                    },
                },
                kind: TimelineEventKind::Actor(teammate_actor),
            }),
        ));
        projector.observe(&envelope(
            5,
            CanonicalEvent::Timeline(TimelineEvent {
                sequence: 5,
                time: EventTime {
                    observed_micros: 5_000,
                    game_time_millis: None,
                },
                provenance: EventProvenance {
                    confidence: EvidenceConfidence::Exact,
                    source: EvidenceSource::Wire {
                        capture_sequence: 5,
                        connection_id: 1,
                        stream_id: 1,
                    },
                },
                kind: TimelineEventKind::EntityAttributes(EntityAttributeEvent {
                    actor: teammate,
                    update_kind: EntityAttributeUpdateKind::Delta,
                    ownership: None,
                    attributes: vec![
                        EntityAttribute {
                            attribute_id: ATTR_CURRENT_HP,
                            raw_value: vec![],
                            decoded: Some(EntityAttributeValue::Integer(750)),
                        },
                        EntityAttribute {
                            attribute_id: ATTR_MAX_HP_FINAL,
                            raw_value: vec![],
                            decoded: Some(EntityAttributeValue::Integer(1_000)),
                        },
                    ],
                }),
            }),
        ));

        let snapshot = projector.snapshot();
        assert_eq!(snapshot.player.expect("local frame").actor_id, 7);
        assert_eq!(snapshot.party.len(), 1);
        assert_eq!(snapshot.party[0].display_name.as_deref(), Some("Teammate"));
        assert_eq!(snapshot.party[0].hp_percent, Some(75.0));
    }

    #[test]
    fn mechanic_effects_are_scene_scoped_and_fail_closed() {
        let build = Some("24687926");
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
        let tower =
            scene_map_spec(Some("24687926"), Some(1151)).expect("reviewed Towering Ruin map");
        assert_eq!(tower.asset_file, Some("scene-1150-towering-ruin.png"));
        assert!((tower.origin_x - -275.674).abs() < 0.001);
        assert!((tower.origin_z - -472.974).abs() < 0.001);
        assert!((tower.span_x - 297.348).abs() < 0.001);
        assert!((tower.span_z - 297.348).abs() < 0.001);

        let tina =
            scene_map_spec(Some("24687926"), Some(1632)).expect("reviewed Tina Mindrealm map");
        assert_eq!(tina.asset_file, Some("scene-1631-tina-mindrealm.png"));
        assert_eq!((tina.origin_x, tina.origin_z), (-640.0, -523.0));
        assert_eq!((tina.span_x, tina.span_z), (800.0, 800.0));

        let coral = scene_map_spec(Some("24687926"), Some(6565)).expect("reviewed Coral Sea map");
        assert_eq!(coral.asset_file, Some("scene-6563-coral-sea.png"));
        assert_eq!((coral.origin_x, coral.origin_z), (-600.0, -500.0));
        assert_eq!((coral.span_x, coral.span_z), (1000.0, 1000.0));

        let map = scene_map_spec(Some("24687926"), Some(6513)).expect("reviewed Cursed Tomb map");
        assert_eq!(map.asset_file, Some("scene-6513-cursed-tomb.png"));
        assert_eq!((map.origin_x, map.origin_z), (-149.0, -377.0));
        assert_eq!((map.span_x, map.span_z), (450.0, 450.0));
        assert!(scene_map_spec(Some("global/steam-newer"), Some(6513)).is_none());
        let raid =
            scene_map_spec(Some("24687926"), Some(13023)).expect("reviewed Season 3 raid map");
        assert_eq!(raid.asset_file, Some("scene-13021-s3-raid.png"));
        assert_eq!((raid.origin_x, raid.origin_z), (-500.0, -400.0));
        assert_eq!((raid.span_x, raid.span_z), (1000.0, 1000.0));

        let wasteland =
            scene_map_spec(Some("24687926"), Some(6615)).expect("reviewed Wasteland Court map");
        assert_eq!(wasteland.asset_file, Some("scene-6615-wasteland-court.png"));
        assert_eq!((wasteland.origin_x, wasteland.origin_z), (-180.0, -250.0));
        assert_eq!((wasteland.span_x, wasteland.span_z), (500.0, 500.0));
    }

    #[test]
    fn full_scene_map_reuses_latest_reviewed_numeric_build_after_a_client_update() {
        let map = scene_map_spec(Some("24687927"), Some(6513))
            .expect("new numeric builds reuse the latest reviewed map identity");
        assert_eq!(map.asset_file, Some("scene-6513-cursed-tomb.png"));
        assert!(scene_map_spec(Some("global/steam-24687927"), Some(6513)).is_none());
        assert!(scene_map_spec(Some("24600000"), Some(6513)).is_none());
    }

    #[test]
    fn season_three_raid_uses_packet_height_to_select_its_verified_arena() {
        let ring =
            raid_arena_spec(Some("24687926"), Some(13021), Some(150.0)).expect("raid ring arena");
        assert_eq!(ring.layout, Some("raid_ring"));
        assert_eq!((ring.origin_x, ring.origin_z), (-55.0, -55.0));
        assert_eq!((ring.span_x, ring.span_z), (110.0, 110.0));

        let grid =
            raid_arena_spec(Some("24687926"), Some(13023), Some(400.0)).expect("raid grid arena");
        assert_eq!(grid.layout, Some("raid_grid"));
        assert_eq!((grid.origin_x, grid.origin_z), (-30.0, -27.0));
        assert_eq!((grid.span_x, grid.span_z), (60.0, 54.0));

        assert!(raid_arena_spec(Some("global/steam-newer"), Some(13021), Some(150.0)).is_none());
        assert!(raid_arena_spec(Some("24687926"), Some(6615), Some(150.0)).is_none());
    }

    #[test]
    fn scene_map_specs_match_the_packaged_review_manifest() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../desktop-tauri/resources/map-compiler/reviewed-map-assets.v1.json");
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).expect("reviewed map manifest"))
                .expect("valid reviewed map manifest");
        let entries = value["builds"]["24687926"]
            .as_array()
            .expect("current build map entries");
        assert_eq!(entries.len(), 57);
        let mut reviewed_scene_ids = BTreeSet::new();
        for entry in entries {
            let asset = entry["asset"].as_str().expect("asset name");
            let scene_ids = entry["scene_ids"].as_array().expect("scene IDs");
            for scene_id in scene_ids {
                assert!(
                    reviewed_scene_ids.insert(scene_id.as_i64().expect("numeric scene ID")),
                    "a scene ID may resolve to only one reviewed map"
                );
                let spec = scene_map_spec(
                    Some("24687926"),
                    Some(scene_id.as_i64().expect("numeric scene ID") as i32),
                )
                .expect("manifest scene has a runtime map spec");
                assert_eq!(spec.asset_file, Some(asset));
                for (observed, key) in [
                    (spec.origin_x, "origin_x"),
                    (spec.origin_z, "origin_z"),
                    (spec.span_x, "span_x"),
                    (spec.span_z, "span_z"),
                ] {
                    let reviewed = entry[key].as_f64().expect("numeric transform") as f32;
                    assert!((observed - reviewed).abs() < 0.0001, "{asset} {key}");
                }
            }
        }
    }

    #[test]
    fn mechanic_casts_are_exact_build_and_scene_scoped() {
        assert!(is_reviewed_mechanic_cast(
            Some("24687926"),
            Some(6513),
            3390117,
        ));
        assert!(!is_reviewed_mechanic_cast(
            Some("24687926"),
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
        let build = Some("24687926");
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
    fn reviewed_scene_families_expose_named_roles_effects_and_casts() {
        let build = Some("24687926");
        for (scene, monster, role) in [
            (1151, 2106, "correct_portal"),
            (1632, 300089, "pizza_fast"),
            (6565, 3340219, "ice_wave"),
            (13023, 10330051, "pinball"),
        ] {
            assert_eq!(
                reviewed_mechanic_entity_role(build, Some(scene), Some(monster)),
                Some(role)
            );
        }
        for (scene, effect, kind) in [
            (1151, 821076, "sticky_bomb"),
            (1632, 841519, "void_corruption_binding"),
            (6565, 883603, "double_echo_water"),
            (13023, 829214, "phase_edge"),
            (13023, 829215, "phase_corner"),
            (13023, 829228, "hit_order_three"),
        ] {
            assert_eq!(
                reviewed_mechanic_signal_kind(build, Some(scene), effect),
                Some(kind)
            );
        }
        for (scene, ability, kind) in [
            (1151, 111103, "gravity_blast"),
            (6565, 3340245, "pizza_indicator"),
            (13023, 10310064, "ring_outer"),
        ] {
            assert!(is_reviewed_mechanic_cast(build, Some(scene), ability));
            assert_eq!(
                reviewed_mechanic_signal_kind(build, Some(scene), -ability),
                Some(kind)
            );
        }
        assert_eq!(
            reviewed_mechanic_signal_kind(build, Some(1151), 841519),
            None,
            "known IDs stay scene-scoped"
        );

        for (scene, effects) in [
            (
                6513,
                vec![
                    884101, 884102, 884103, 884106, 884122, 884129, 884141, 884162, 884163, 884168,
                    884169, 884170,
                ],
            ),
            (1151, vec![821076]),
            (1632, vec![510571, 841519, 841509]),
            (
                6565,
                vec![
                    883707, 883708, 883709, 883710, 883714, 883601, 883602, 883603, 883605, 883631,
                    522602, 883633, 883634,
                ],
            ),
            (
                13023,
                vec![
                    829104, 829105, 829106, 829115, 829116, 829214, 829215, 829217, 829226, 829227,
                    829228, 829245, 829304, 829305, 829306, 829307, 829308, 829309, 829314, 829316,
                    829318, 829323, 829324, 829326, 829327, 829328, 829329, 829330, 829331, 829332,
                    829372, 829373, 829374,
                ],
            ),
            (
                6615,
                vec![
                    884609, 884610, 884614, 884615, 884616, 884641, 884659, 884660, 884661, 884664,
                ],
            ),
        ] {
            for effect in effects {
                assert!(
                    reviewed_mechanic_signal_kind(build, Some(scene), effect).is_some(),
                    "scene {scene} effect {effect} must never render anonymously"
                );
            }
        }
    }

    #[test]
    fn void_towering_ruin_exposes_only_packet_position_annotations() {
        let build = Some("24687926");
        let scene = Some(1151);
        let map = scene_map_spec(build, scene).expect("reviewed Void Towering Ruin map");
        assert_eq!(map.asset_file, Some("scene-1150-towering-ruin.png"));
        assert_eq!(
            reviewed_mechanic_entity_role(build, scene, Some(2106)),
            Some("correct_portal")
        );
        assert_eq!(
            reviewed_mechanic_entity_role(build, scene, Some(2107)),
            Some("other_portal")
        );
        assert_eq!(
            reviewed_mechanic_signal_kind(build, scene, 821076),
            Some("sticky_bomb")
        );
        assert_eq!(
            reviewed_mechanic_signal_kind(build, scene, -111103),
            Some("gravity_blast")
        );
        assert!(is_reviewed_mechanic_cast(build, scene, 111103));
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

    #[test]
    fn projects_packet_observed_dungeon_objectives_without_inventing_targets() {
        let mut projector = MechanicsMapProjector::default();
        projector.observe(&envelope(
            1,
            CanonicalEvent::Dungeon(DungeonEvent {
                kind: DungeonEventKind::Entered,
                dungeon_id: Some(DungeonId(6513)),
                instance_id: Some("run-1".into()),
                difficulty_id: Some(6545),
                objective_map_key: None,
                objective_id: None,
                objective_value: None,
                objective_complete: None,
                objective_catalog: None,
                flow: Some(DungeonFlowSnapshot {
                    state_id: Some(3),
                    phase: Some(DungeonFlowPhase::Playing),
                    ..DungeonFlowSnapshot::default()
                }),
            }),
        ));
        projector.observe(&envelope(
            2,
            CanonicalEvent::Dungeon(DungeonEvent {
                kind: DungeonEventKind::ObjectiveUpdated,
                dungeon_id: Some(DungeonId(6513)),
                instance_id: Some("run-1".into()),
                difficulty_id: Some(6545),
                objective_map_key: Some(7),
                objective_id: Some(651103),
                objective_value: Some(275),
                objective_complete: Some(false),
                objective_catalog: Some(DungeonObjectiveCatalogReference {
                    resolution: DungeonObjectiveCatalogResolution::UnresolvedCurrentBuild,
                    activity_target_key: None,
                    localization_key: None,
                    required_count: None,
                    scene_event_keys: vec![],
                }),
                flow: None,
            }),
        ));

        let snapshot = projector.snapshot();
        let dungeon = snapshot.dungeon.expect("dungeon HUD state");
        assert_eq!(dungeon.dungeon_id, Some(6513));
        assert_eq!(dungeon.flow_phase, Some("playing"));
        assert_eq!(dungeon.objectives.len(), 1);
        assert_eq!(dungeon.objectives[0].objective_id, 651103);
        assert_eq!(dungeon.objectives[0].value, Some(275));
        assert_eq!(dungeon.objectives[0].complete, Some(false));
        assert_eq!(
            dungeon.objectives[0].catalog_resolution,
            "unresolved_current_build"
        );

        projector.observe(&envelope(
            3,
            CanonicalEvent::Dungeon(DungeonEvent {
                kind: DungeonEventKind::Exited,
                dungeon_id: Some(DungeonId(6513)),
                instance_id: Some("run-1".into()),
                difficulty_id: Some(6545),
                objective_map_key: None,
                objective_id: None,
                objective_value: None,
                objective_complete: None,
                objective_catalog: None,
                flow: None,
            }),
        ));
        assert!(projector.snapshot().dungeon.is_none());
    }

    #[test]
    fn packet_encounter_boundaries_drive_and_freeze_the_dungeon_attempt_clock() {
        let mut projector = MechanicsMapProjector::default();
        projector.observe(&envelope(
            1,
            CanonicalEvent::Dungeon(DungeonEvent {
                kind: DungeonEventKind::Entered,
                dungeon_id: Some(DungeonId(6513)),
                instance_id: Some("run-1".into()),
                difficulty_id: Some(6545),
                objective_map_key: None,
                objective_id: None,
                objective_value: None,
                objective_complete: None,
                objective_catalog: None,
                flow: None,
            }),
        ));

        projector.observe(&envelope(
            1_000,
            encounter_boundary(1_000, EncounterState::Started),
        ));
        let started = projector.snapshot().dungeon.expect("started attempt");
        assert_eq!(started.attempt_number, 1);
        assert_eq!(started.retry_count, 0);
        assert_eq!(started.encounter_state, Some("started"));
        assert!(started.attempt_running);

        projector.observe(&envelope(
            3_500,
            encounter_boundary(3_500, EncounterState::Wiped),
        ));
        let wiped = projector.snapshot().dungeon.expect("wiped attempt");
        assert_eq!(wiped.attempt_elapsed_micros, 2_500_000);
        assert_eq!(wiped.retry_count, 1);
        assert_eq!(wiped.encounter_state, Some("wiped"));
        assert!(!wiped.attempt_running);

        projector.observe(&envelope(
            5_000,
            encounter_boundary(5_000, EncounterState::Started),
        ));
        projector.observe(&envelope(
            7_000,
            encounter_boundary(7_000, EncounterState::Cleared),
        ));
        let cleared = projector.snapshot().dungeon.expect("cleared retry");
        assert_eq!(cleared.attempt_number, 2);
        assert_eq!(cleared.retry_count, 1);
        assert_eq!(cleared.attempt_elapsed_micros, 2_000_000);
        assert_eq!(cleared.encounter_state, Some("cleared"));
        assert!(!cleared.attempt_running);
    }
}
