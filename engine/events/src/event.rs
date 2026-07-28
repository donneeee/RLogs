use serde::{Deserialize, Serialize};

use crate::{AbilityId, EntityRef, EventTopic, MonsterId, SceneId, StatusEffectId};

/// Capture-observed time is monotonic within a log. Game time is optional
/// because not every packet carries an authoritative server timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventTime {
    pub observed_micros: u64,
    pub game_time_millis: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceConfidence {
    Exact,
    Inferred,
    Estimated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EvidenceSource {
    Wire {
        capture_sequence: u64,
        connection_id: u64,
        stream_id: u64,
    },
    Derived {
        rule_id: String,
        evidence_sequences: Vec<u64>,
    },
    Manual {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventProvenance {
    pub confidence: EvidenceConfidence,
    pub source: EvidenceSource,
}

impl EventProvenance {
    pub fn wire(capture_sequence: u64, connection_id: u64, stream_id: u64) -> Self {
        Self {
            confidence: EvidenceConfidence::Exact,
            source: EvidenceSource::Wire {
                capture_sequence,
                connection_id,
                stream_id,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineEventDraft {
    pub time: EventTime,
    pub provenance: EventProvenance,
    pub kind: TimelineEventKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineEvent {
    /// Stable within this run, beginning at one.
    pub sequence: u64,
    pub time: EventTime,
    pub provenance: EventProvenance,
    pub kind: TimelineEventKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum TimelineEventKind {
    RunBoundary {
        state: RunState,
        scene_id: Option<SceneId>,
        reason: BoundaryReason,
    },
    EncounterBoundary {
        state: EncounterState,
        encounter_id: Option<String>,
        reason: BoundaryReason,
    },
    CombatBoundary {
        state: CombatState,
        reason: BoundaryReason,
    },
    Actor(ActorEvent),
    EntityAttributes(EntityAttributeEvent),
    Cast(CastEvent),
    Cooldown(CooldownEvent),
    Damage(DamageEvent),
    Healing(HealingEvent),
    Shield(ShieldEvent),
    Life {
        actor: EntityRef,
        state: LifeState,
    },
    Status(StatusEvent),
    Position(PositionEvent),
    DataGap(DataGapEvent),
}

impl TimelineEventKind {
    pub fn topic(&self) -> EventTopic {
        match self {
            Self::RunBoundary { .. }
            | Self::EncounterBoundary { .. }
            | Self::CombatBoundary { .. } => EventTopic::Encounter,
            Self::Actor(_) | Self::EntityAttributes(_) | Self::Life { .. } | Self::Position(_) => {
                EventTopic::Actor
            }
            Self::Cast(_)
            | Self::Cooldown(_)
            | Self::Damage(_)
            | Self::Healing(_)
            | Self::Shield(_)
            | Self::Status(_) => EventTopic::Combat,
            Self::DataGap(_) => EventTopic::DataQuality,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryReason {
    AuthoritativePacket,
    SceneTransition,
    HostileAction,
    ActorLifecycle,
    Completion,
    Wipe,
    InactivityFallback,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Entered,
    Completed,
    Failed,
    Exited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EncounterState {
    Started,
    Cleared,
    Wiped,
    Ended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CombatState {
    Started,
    Ended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    Player,
    Monster,
    Npc,
    SceneObject,
    Zone,
    Projectile,
    Pet,
    TrainingDummy,
    Drop,
    Field,
    Trap,
    Collection,
    StaticObject,
    Vehicle,
    Toy,
    Housing,
    Unknown(i32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorState {
    Spawned,
    Updated,
    Transformed,
    Despawned,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActorEvent {
    pub actor: EntityRef,
    pub state: ActorState,
    /// Exact protocol enum value retained even when `kind` is normalized.
    pub entity_type_id: i32,
    pub kind: ActorKind,
    pub monster_id: Option<MonsterId>,
    pub display_name: Option<String>,
    pub class_id: Option<i32>,
    pub level: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityAttributeEvent {
    pub actor: EntityRef,
    pub attributes: Vec<EntityAttribute>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityAttribute {
    pub attribute_id: i32,
    /// Exact attribute payload after protobuf field decoding.
    pub raw_value: Vec<u8>,
    pub decoded: Option<EntityAttributeValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum EntityAttributeValue {
    Integer(i64),
    Text(String),
    Position {
        x: f32,
        y: f32,
        z: f32,
        facing_radians: Option<f32>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CastState {
    Started,
    Completed,
    Interrupted,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CastEvent {
    pub source: EntityRef,
    pub ability: AbilityId,
    pub target: Option<EntityRef>,
    pub state: CastState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CooldownEvent {
    pub actor: EntityRef,
    pub ability: AbilityId,
    pub begin_time_millis: Option<i64>,
    pub duration_millis: Option<i32>,
    pub valid_duration_millis: Option<i32>,
    pub cooldown_type: Option<i32>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DamageFlags {
    pub critical: Option<bool>,
    pub lucky: Option<bool>,
    pub blocked: Option<bool>,
    pub periodic: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DamageEvent {
    /// Owner or top-summoner attribution used for totals.
    pub source: EntityRef,
    /// Immediate wire attacker when it differs from the attributed source.
    pub direct_source: Option<EntityRef>,
    pub target: EntityRef,
    pub ability: Option<AbilityId>,
    /// Primary amount reported by the game.
    pub amount: i64,
    pub actual_amount: Option<i64>,
    pub hp_loss: Option<i64>,
    pub shield_loss: Option<i64>,
    pub hit_event_id: Option<i32>,
    pub damage_source: Option<i32>,
    pub damage_type: Option<i32>,
    pub flags: DamageFlags,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HealingEvent {
    pub source: EntityRef,
    pub direct_source: Option<EntityRef>,
    pub target: EntityRef,
    pub ability: Option<AbilityId>,
    pub amount: i64,
    pub effective_amount: Option<i64>,
    pub overheal: Option<i64>,
    pub critical: Option<bool>,
    pub periodic: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShieldEvent {
    pub source: EntityRef,
    pub target: EntityRef,
    pub ability: AbilityId,
    pub amount: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifeState {
    Died,
    Revived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusState {
    Applied,
    Refreshed,
    Stacked,
    Consumed,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatusEvent {
    pub source: Option<EntityRef>,
    pub target: EntityRef,
    pub effect: StatusEffectId,
    pub state: StatusState,
    pub stacks: Option<u32>,
    pub duration_millis: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PositionEvent {
    pub actor: EntityRef,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub facing_radians: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataGapKind {
    CaptureDrop,
    TcpGap,
    UnknownRoute,
    DecodeFailure,
    UnsupportedFragment,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataGapEvent {
    pub kind: DataGapKind,
    pub connection_id: Option<u64>,
    pub stream_id: Option<u64>,
    pub detail: String,
}
