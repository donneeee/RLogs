//! Region-aware canonical events and ordered, loss-aware run timelines.

mod envelope;
mod event;
mod identity;
mod ids;
mod timeline;

pub use envelope::{CanonicalEvent, EVENT_SCHEMA_VERSION, EventEnvelope, EventTopic};
pub use event::{
    ActorEvent, ActorKind, ActorState, BoundaryReason, CastEvent, CastState, CombatState,
    DamageEvent, DamageFlags, DataGapEvent, DataGapKind, EncounterState, EventProvenance,
    EventTime, EvidenceConfidence, EvidenceSource, HealingEvent, LifeState, PositionEvent,
    RunState, ShieldEvent, StatusEvent, StatusState, TimelineEvent, TimelineEventDraft,
    TimelineEventKind,
};
pub use identity::{
    CharacterIdentity, RegionContext, RegionEvidence, RegionEvidenceKind, RegionIdentity,
};
pub use ids::{AbilityId, ActorId, SceneId, StatusEffectId};
pub use timeline::{RunTimeline, TimelineError};
