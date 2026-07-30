//! Region-aware canonical events and ordered, loss-aware run timelines.

mod envelope;
mod event;
mod identity;
mod ids;
mod profile;
mod timeline;
mod world;

pub use envelope::{
    CanonicalEvent, CanonicalEventDraft, CanonicalEventDraftKind, EVENT_SCHEMA_VERSION,
    EventEnvelope, EventEnvelopeFactory, EventSensitivity, EventSequenceError, EventTopic,
};
pub use event::{
    ActorEvent, ActorKind, ActorState, BoundaryReason, CastEvent, CastState, CombatState,
    CooldownEvent, DamageEvent, DamageFlags, DataGapEvent, DataGapKind, EncounterState,
    EntityAttribute, EntityAttributeEvent, EntityAttributeValue, EventProvenance, EventTime,
    EvidenceConfidence, EvidenceSource, HealingEvent, LifeState, PositionEvent, RecorderPauseEvent,
    RunState, ShieldEvent, StatusEvent, StatusState, TimelineEvent, TimelineEventDraft,
    TimelineEventKind,
};
pub use identity::{
    CharacterIdentity, RegionContext, RegionEvidence, RegionEvidenceKind, RegionIdentity,
};
pub use ids::{
    AbilityId, ActorId, DungeonId, EntityRef, EntityUuid, MapId, MonsterId, SceneId, StatusEffectId,
};
pub use profile::{GameProfileEvent, WorldContext};
pub use timeline::{RunTimeline, TimelineError};
pub use world::{
    ChatChannel, ChatEvent, DungeonEvent, DungeonEventKind, DungeonFlowPhase, DungeonFlowSnapshot,
    MapEvent, MapEventKind,
};
