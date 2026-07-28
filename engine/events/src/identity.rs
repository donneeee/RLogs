use serde::{Deserialize, Serialize};

/// Persistent game-location identity used in character keys and leaderboards.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RegionIdentity {
    /// Product deployment such as `global` or `cn`.
    pub deployment_id: String,
    /// Authoritative region or shard identity within the deployment.
    pub region_id: String,
    pub realm_id: Option<String>,
    pub world_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegionEvidenceKind {
    ConnectionEndpoint,
    AuthoritativeMessage,
    ProtocolPack,
    ReplayManifest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegionEvidence {
    pub kind: RegionEvidenceKind,
    /// A privacy-reviewed description or stable evidence identifier.
    pub reference: String,
}

/// Runtime evidence explaining how a region and protocol pack were selected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegionContext {
    pub identity: RegionIdentity,
    pub client_build: String,
    pub protocol_pack_digest: String,
    pub evidence: Vec<RegionEvidence>,
}

/// A character key cannot collide across deployments, regions, or worlds.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CharacterIdentity {
    pub region: RegionIdentity,
    pub character_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(deployment_id: &str) -> RegionIdentity {
        RegionIdentity {
            deployment_id: deployment_id.into(),
            region_id: "east".into(),
            realm_id: None,
            world_id: Some("world-1".into()),
        }
    }

    #[test]
    fn equal_character_ids_in_different_deployments_are_distinct() {
        let global = CharacterIdentity {
            region: region("global"),
            character_id: "42".into(),
        };
        let cn = CharacterIdentity {
            region: region("cn"),
            character_id: "42".into(),
        };

        assert_ne!(global, cn);
    }

    #[test]
    fn region_evidence_has_no_user_setting_variant() {
        let evidence = RegionEvidence {
            kind: RegionEvidenceKind::AuthoritativeMessage,
            reference: "world-session-region".into(),
        };

        let json = serde_json::to_string(&evidence).unwrap();
        assert!(json.contains("authoritative_message"));
        assert!(!json.contains("account"));
    }
}
