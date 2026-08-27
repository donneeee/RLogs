use serde::{Deserialize, Serialize};

use thiserror::Error;

use crate::{
    CharacterIdentity, ChatEvent, DungeonEvent, EventProvenance, EventTime, GameProfileEvent,
    MapEvent, RegionContext, TimelineEvent, TimelineEventDraft, TimelineEventKind, WorldContext,
};

pub const EVENT_SCHEMA_VERSION: u16 = 9;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventTopic {
    Combat,
    Encounter,
    Actor,
    CharacterProfile,
    Party,
    World,
    Map,
    Dungeon,
    Chat,
    DataQuality,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventSensitivity {
    PublicGameplay,
    PersonalGameplay,
    LocalSensitive,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum CanonicalEvent {
    Timeline(TimelineEvent),
    CharacterProfileObserved {
        profile: Box<GameProfileEvent>,
    },
    /// Exact, ordered party-roster evidence. This is distinct from the legacy
    /// `PartyChanged` projection because the wire can expose a full snapshot,
    /// a partial member observation, a single leave, or a dissolve. Consumers
    /// must not reinterpret a partial observation as a complete roster.
    PartyRosterObserved(PartyRosterEvent),
    PartyChanged {
        members: Vec<CharacterIdentity>,
    },
    WorldChanged(WorldContext),
    Map(MapEvent),
    Dungeon(DungeonEvent),
    Chat(ChatEvent),
}

impl CanonicalEvent {
    pub fn topic(&self) -> EventTopic {
        match self {
            Self::Timeline(event) => event.kind.topic(),
            Self::CharacterProfileObserved { .. } => EventTopic::CharacterProfile,
            Self::PartyRosterObserved(_) => EventTopic::Party,
            Self::PartyChanged { .. } => EventTopic::Party,
            Self::WorldChanged(_) => EventTopic::World,
            Self::Map(_) => EventTopic::Map,
            Self::Dungeon(_) => EventTopic::Dungeon,
            Self::Chat(_) => EventTopic::Chat,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalEventDraft {
    pub time: EventTime,
    pub provenance: EventProvenance,
    pub sensitivity: EventSensitivity,
    pub kind: CanonicalEventDraftKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum CanonicalEventDraftKind {
    Timeline(TimelineEventKind),
    CharacterProfileObserved { profile: Box<GameProfileEvent> },
    PartyRosterObserved(PartyRosterEvent),
    PartyChanged { members: Vec<CharacterIdentity> },
    WorldChanged(WorldContext),
    Map(MapEvent),
    Dungeon(DungeonEvent),
    Chat(ChatEvent),
}

/// Privacy-reviewed party-roster evidence in the exact semantic shape exposed
/// by the decoded message. Numeric party and leave identifiers remain raw.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartyRosterEvent {
    pub observation: PartyRosterObservation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartyRosterMember {
    pub character: CharacterIdentity,
    /// Exact game-provided party entry time integer. Its epoch/unit semantics
    /// remain game-specific until separately proven.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enter_time: Option<u32>,
    /// Raw protocol values retained without assigning enum meanings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub online_status: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene_id: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum PartyRosterObservation {
    /// Complete roster supplied by the join notification.
    FullSnapshot {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        party_id: Option<String>,
        members: Vec<PartyRosterMember>,
    },
    /// Members listed by an incremental team-member notification. This proves
    /// only that the listed characters were members; it is not a full roster.
    MembersObserved { members: Vec<PartyRosterMember> },
    /// Exact character named by the leave notification. `leave_type` is the
    /// undecorated protocol integer and has no inferred meaning here.
    MemberLeft {
        member: CharacterIdentity,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        leave_type: Option<i32>,
    },
    /// Empty dissolve notification. No cause or member list is invented.
    Dissolved,
}

/// Public unit delivered to reducers, bundled plugins, and community plugins.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub schema_version: u16,
    pub session_id: String,
    /// Stable across every canonical event in this session, beginning at one.
    pub sequence: u64,
    pub region: RegionContext,
    pub time: EventTime,
    pub provenance: EventProvenance,
    pub sensitivity: EventSensitivity,
    pub event: CanonicalEvent,
}

/// Assigns public and timeline sequences after a protocol decoder emits drafts.
#[derive(Debug, Clone)]
pub struct EventEnvelopeFactory {
    session_id: String,
    region: RegionContext,
    next_sequence: u64,
    next_timeline_sequence: u64,
}

impl EventEnvelopeFactory {
    pub fn new(session_id: impl Into<String>, region: RegionContext) -> Self {
        Self {
            session_id: session_id.into(),
            region,
            next_sequence: 1,
            next_timeline_sequence: 1,
        }
    }

    pub fn emit(
        &mut self,
        draft: CanonicalEventDraft,
    ) -> Result<EventEnvelope, EventSequenceError> {
        let sequence = self.next_sequence;
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or(EventSequenceError::Exhausted)?;

        let event = match draft.kind {
            CanonicalEventDraftKind::Timeline(kind) => {
                let timeline_sequence = self.next_timeline_sequence;
                self.next_timeline_sequence = self
                    .next_timeline_sequence
                    .checked_add(1)
                    .ok_or(EventSequenceError::Exhausted)?;
                CanonicalEvent::Timeline(TimelineEvent {
                    sequence: timeline_sequence,
                    time: draft.time,
                    provenance: draft.provenance.clone(),
                    kind,
                })
            }
            CanonicalEventDraftKind::CharacterProfileObserved { profile } => {
                CanonicalEvent::CharacterProfileObserved { profile }
            }
            CanonicalEventDraftKind::PartyRosterObserved(event) => {
                CanonicalEvent::PartyRosterObserved(event)
            }
            CanonicalEventDraftKind::PartyChanged { members } => {
                CanonicalEvent::PartyChanged { members }
            }
            CanonicalEventDraftKind::WorldChanged(context) => CanonicalEvent::WorldChanged(context),
            CanonicalEventDraftKind::Map(event) => CanonicalEvent::Map(event),
            CanonicalEventDraftKind::Dungeon(event) => CanonicalEvent::Dungeon(event),
            CanonicalEventDraftKind::Chat(event) => CanonicalEvent::Chat(event),
        };

        Ok(EventEnvelope {
            schema_version: EVENT_SCHEMA_VERSION,
            session_id: self.session_id.clone(),
            sequence,
            region: self.region.clone(),
            time: draft.time,
            provenance: draft.provenance,
            sensitivity: draft.sensitivity,
            event,
        })
    }

    pub fn region(&self) -> &RegionContext {
        &self.region
    }
}

impl From<TimelineEventDraft> for CanonicalEventDraft {
    fn from(draft: TimelineEventDraft) -> Self {
        Self {
            time: draft.time,
            provenance: draft.provenance,
            sensitivity: EventSensitivity::PublicGameplay,
            kind: CanonicalEventDraftKind::Timeline(draft.kind),
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EventSequenceError {
    #[error("canonical event sequence space is exhausted")]
    Exhausted,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RegionEvidence, RegionEvidenceKind, RegionIdentity};

    #[test]
    fn every_public_event_envelope_contains_region_and_pack_evidence() {
        let region = RegionContext {
            identity: RegionIdentity {
                deployment_id: "global".into(),
                region_id: "north-america".into(),
                realm_id: None,
                world_id: Some("world-7".into()),
            },
            client_build: "example-build".into(),
            protocol_pack_digest: "sha256:example".into(),
            evidence: vec![RegionEvidence {
                kind: RegionEvidenceKind::ConnectionEndpoint,
                reference: "endpoint-group-1".into(),
            }],
        };
        let mut factory = EventEnvelopeFactory::new("capture-1", region);
        let envelope = factory
            .emit(CanonicalEventDraft {
                time: EventTime {
                    observed_micros: 12,
                    game_time_millis: None,
                },
                provenance: EventProvenance::wire(1, 2, 3),
                sensitivity: EventSensitivity::PublicGameplay,
                kind: CanonicalEventDraftKind::WorldChanged(WorldContext {
                    scene_id: Some(crate::SceneId(12)),
                    map_id: None,
                    line_id: None,
                    scene_instance_id: None,
                    dungeon_instance_id: None,
                }),
            })
            .unwrap();

        let json = serde_json::to_string(&envelope).unwrap();
        let decoded: EventEnvelope = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, envelope);
        assert_eq!(decoded.schema_version, EVENT_SCHEMA_VERSION);
        assert_eq!(decoded.sequence, 1);
        assert_eq!(decoded.event.topic(), EventTopic::World);
        assert_eq!(decoded.region.identity.deployment_id, "global");
    }

    #[test]
    fn public_and_timeline_sequences_are_assigned_independently() {
        let region = RegionContext {
            identity: RegionIdentity {
                deployment_id: "global".into(),
                region_id: "north-america".into(),
                realm_id: None,
                world_id: None,
            },
            client_build: "build".into(),
            protocol_pack_digest: "sha256:test".into(),
            evidence: Vec::new(),
        };
        let mut factory = EventEnvelopeFactory::new("capture", region);

        let world = factory
            .emit(CanonicalEventDraft {
                time: EventTime {
                    observed_micros: 1,
                    game_time_millis: None,
                },
                provenance: EventProvenance::wire(1, 2, 3),
                sensitivity: EventSensitivity::PublicGameplay,
                kind: CanonicalEventDraftKind::WorldChanged(WorldContext {
                    scene_id: None,
                    map_id: None,
                    line_id: None,
                    scene_instance_id: None,
                    dungeon_instance_id: None,
                }),
            })
            .unwrap();
        let timeline = factory
            .emit(CanonicalEventDraft {
                time: EventTime {
                    observed_micros: 2,
                    game_time_millis: None,
                },
                provenance: EventProvenance::wire(2, 2, 3),
                sensitivity: EventSensitivity::PublicGameplay,
                kind: CanonicalEventDraftKind::Timeline(crate::TimelineEventKind::CombatBoundary {
                    state: crate::CombatState::Started,
                    reason: crate::BoundaryReason::HostileAction,
                }),
            })
            .unwrap();

        assert_eq!(world.sequence, 1);
        assert_eq!(timeline.sequence, 2);
        let CanonicalEvent::Timeline(timeline) = timeline.event else {
            panic!("expected timeline event");
        };
        assert_eq!(timeline.sequence, 1);
    }
}
