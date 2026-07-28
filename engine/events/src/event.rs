use serde::{Deserialize, Serialize};

use crate::{AbilityId, ActorId, EventTopic, SceneId, StatusEffectId};

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
        context_id: u32,
        wire_sequence: Option<u64>,
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
    pub fn wire(context_id: u32, wire_sequence: Option<u64>) -> Self {
        Self {
            confidence: EvidenceConfidence::Exact,
            source: EvidenceSource::Wire {
                context_id,
                wire_sequence,
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
    Cast(CastEvent),
    Damage(DamageEvent),
    Healing(HealingEvent),
    Shield(ShieldEvent),
    Life {
        actor: ActorId,
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
            Self::Actor(_) | Self::Life { .. } | Self::Position(_) => EventTopic::Actor,
            Self::Cast(_)
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
    Mob,
    Npc,
    Object,
    Unknown,
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
    pub actor: ActorId,
    pub state: ActorState,
    pub kind: ActorKind,
    pub class_id: Option<i32>,
    pub level: Option<u32>,
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
    pub source: ActorId,
    pub ability: AbilityId,
    pub target: Option<ActorId>,
    pub state: CastState,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DamageFlags {
    pub critical: bool,
    pub lucky: bool,
    pub blocked: bool,
    pub periodic: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DamageEvent {
    pub source: ActorId,
    pub target: ActorId,
    pub ability: AbilityId,
    pub amount: i64,
    pub absorbed: i64,
    pub shield_break: bool,
    pub flags: DamageFlags,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HealingEvent {
    pub source: ActorId,
    pub target: ActorId,
    pub ability: AbilityId,
    pub amount: i64,
    pub overheal: i64,
    pub critical: bool,
    pub periodic: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShieldEvent {
    pub source: ActorId,
    pub target: ActorId,
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
    pub source: Option<ActorId>,
    pub target: ActorId,
    pub effect: StatusEffectId,
    pub state: StatusState,
    pub stacks: Option<u32>,
    pub duration_millis: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PositionEvent {
    pub actor: ActorId,
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
    pub context_id: Option<u32>,
    pub detail: String,
}
