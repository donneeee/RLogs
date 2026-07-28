use serde::{Deserialize, Serialize};

use crate::{CharacterIdentity, RegionContext, TimelineEvent};

pub const EVENT_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventTopic {
    Combat,
    Encounter,
    Actor,
    CharacterProfile,
    Party,
    World,
    DataQuality,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum CanonicalEvent {
    Timeline(TimelineEvent),
    CharacterProfileChanged {
        character: CharacterIdentity,
        revision: u64,
    },
    PartyChanged {
        members: Vec<CharacterIdentity>,
    },
    WorldChanged {
        scene_id: Option<i32>,
        map_id: Option<String>,
    },
}

impl CanonicalEvent {
    pub fn topic(&self) -> EventTopic {
        match self {
            Self::Timeline(event) => event.kind.topic(),
            Self::CharacterProfileChanged { .. } => EventTopic::CharacterProfile,
            Self::PartyChanged { .. } => EventTopic::Party,
            Self::WorldChanged { .. } => EventTopic::World,
        }
    }
}

/// Public unit delivered to reducers, bundled plugins, and community plugins.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub schema_version: u16,
    pub session_id: String,
    pub region: RegionContext,
    pub event: CanonicalEvent,
}

impl EventEnvelope {
    pub fn new(
        session_id: impl Into<String>,
        region: RegionContext,
        event: CanonicalEvent,
    ) -> Self {
        Self {
            schema_version: EVENT_SCHEMA_VERSION,
            session_id: session_id.into(),
            region,
            event,
        }
    }
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
        let envelope = EventEnvelope::new(
            "capture-1",
            region,
            CanonicalEvent::WorldChanged {
                scene_id: Some(12),
                map_id: None,
            },
        );

        let json = serde_json::to_string(&envelope).unwrap();
        let decoded: EventEnvelope = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, envelope);
        assert_eq!(decoded.schema_version, EVENT_SCHEMA_VERSION);
        assert_eq!(decoded.event.topic(), EventTopic::World);
        assert_eq!(decoded.region.identity.deployment_id, "global");
    }
}
