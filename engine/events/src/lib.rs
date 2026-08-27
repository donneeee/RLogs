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
    PartyRosterEvent, PartyRosterMember, PartyRosterObservation,
};
pub use event::{
    ActionTimingSnapshot, ActorEvent, ActorKind, ActorLoadoutEvidence, ActorLoadoutObservation,
    ActorLoadoutSlot, ActorOwnershipUpdate, ActorState, BoundaryReason, CastEvent, CastState,
    CombatState, CooldownEvent, DamageEvent, DamageFlags, DamageHitPart, DamagePacketDetail,
    DamagePosition, DataGapEvent, DataGapKind, EncounterState, EntityAttribute,
    EntityAttributeEvent, EntityAttributeUpdateKind, EntityAttributeValue, EventProvenance,
    EventTime, EvidenceConfidence, EvidenceSource, HealingEvent, LifeState, PositionEvent,
    RecorderPauseEvent, ResourceCooldown, ResourceEvent, RunState, ShieldEvent, StatusEvent,
    StatusOrigin, StatusState, TemporaryAttribute, TemporaryAttributeEvent, TimelineEvent,
    TimelineEventDraft, TimelineEventKind, UnresolvedActionEvent, UnresolvedActionReason,
    UnresolvedStatusEvent, UnresolvedStatusReason,
};
pub use identity::{
    CharacterIdentity, RegionContext, RegionEvidence, RegionEvidenceKind, RegionIdentity,
};
pub use ids::{
    AbilityId, ActorId, DungeonId, EntityRef, EntityUuid, MapId, MonsterId, SceneId,
    StatusEffectId, StatusEffectInstanceId,
};
pub use profile::{GameProfileEvent, WorldContext};
pub use timeline::{RunTimeline, TimelineError};
pub use world::{
    ChatChannel, ChatEvent, DungeonEvent, DungeonEventKind, DungeonFlowPhase, DungeonFlowSnapshot,
    DungeonObjectiveCatalogReference, DungeonObjectiveCatalogResolution, MapEvent, MapEventKind,
};
