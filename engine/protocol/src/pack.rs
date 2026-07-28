use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    AllowedDataDomain, DecodeDisposition, DecoderKind, GameBuild, MappingConfidence,
    MappingProvenance, PrivacyPolicyError, ProhibitedDataClass, ProtocolPrivacyPolicy,
    RouteCatalog, RouteCatalogError, RouteDefinition, RouteKey,
};

pub const PROTOCOL_PACK_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolPackDefinition {
    pub schema_version: u16,
    pub pack_id: String,
    pub target: ProtocolPackTarget,
    #[serde(default)]
    pub provenance: Vec<MappingProvenance>,
    pub routes: Vec<ProtocolPackRoute>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolPackTarget {
    pub deployment_id: String,
    /// `None` means the wire schema is shared by every region in the deployment.
    pub region_id: Option<String>,
    pub channel: String,
    /// Build selection is exact. Wildcards and version ranges are intentionally
    /// unsupported because silently choosing a nearby schema corrupts logs.
    pub build_id: String,
    pub executable_version: Option<String>,
}

impl ProtocolPackTarget {
    pub fn matches(&self, build: &GameBuild) -> bool {
        self.deployment_id == build.deployment_id
            && self.channel == build.channel
            && self.build_id == build.build_id
            && self
                .region_id
                .as_ref()
                .is_none_or(|region| build.region_id.as_ref() == Some(region))
            && self
                .executable_version
                .as_ref()
                .is_none_or(|version| build.executable_version.as_ref() == Some(version))
    }

    fn specificity(&self) -> u8 {
        u8::from(self.region_id.is_some()) + u8::from(self.executable_version.is_some())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolPackRoute {
    pub route: RouteKey,
    pub service_name: String,
    pub method_name: String,
    pub message_name: Option<String>,
    pub confidence: MappingConfidence,
    #[serde(default)]
    pub provenance: Vec<MappingProvenance>,
    /// Gameplay surfaces observed or decoded on this route. This is used to
    /// audit complete coverage independently from the route's privacy domain.
    #[serde(default)]
    pub features: Vec<ProtocolFeature>,
    #[serde(flatten)]
    pub disposition: ProtocolPackRouteDisposition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolFeature {
    Scene,
    Map,
    Dungeon,
    DungeonObjective,
    EntityLifecycle,
    EntityAttributes,
    CharacterIdentity,
    CharacterProfile,
    MonsterIdentity,
    Position,
    Movement,
    Skill,
    Cooldown,
    Damage,
    Healing,
    Shield,
    Death,
    Revive,
    StatusEffect,
    Equipment,
    Profession,
    Talent,
    Party,
    PublicChat,
    PartyChat,
    GuildChat,
    SystemMessage,
    Social,
    Matchmaking,
    WorldState,
    UnknownResearch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "disposition", rename_all = "snake_case")]
pub enum ProtocolPackRouteDisposition {
    Allowed {
        domain: AllowedDataDomain,
        decoder: DecoderKind,
    },
    Prohibited {
        class: ProhibitedDataClass,
    },
    Opaque,
}

#[derive(Debug, Clone)]
pub struct ProtocolPack {
    definition: ProtocolPackDefinition,
    digest: String,
    catalog: RouteCatalog,
    privacy: ProtocolPrivacyPolicy,
    decoders: BTreeMap<RouteKey, DecoderKind>,
    mapping_indexes: BTreeMap<RouteKey, usize>,
}

impl ProtocolPack {
    pub fn build(definition: ProtocolPackDefinition) -> Result<Self, ProtocolPackError> {
        validate_definition(&definition)?;

        let encoded = serde_json::to_vec(&definition)
            .map_err(|error| ProtocolPackError::Serialization(error.to_string()))?;
        let digest = format!("sha256:{:x}", Sha256::digest(encoded));
        let mut catalog = RouteCatalog::new();
        let mut privacy = ProtocolPrivacyPolicy::new();
        let mut decoders = BTreeMap::new();
        let mut mapping_indexes = BTreeMap::new();

        for (index, mapping) in definition.routes.iter().enumerate() {
            let mut provenance = definition.provenance.clone();
            provenance.extend(mapping.provenance.clone());
            catalog.insert(RouteDefinition {
                route: mapping.route,
                service_name: mapping.service_name.clone(),
                method_name: mapping.method_name.clone(),
                message_name: mapping.message_name.clone(),
                confidence: mapping.confidence,
                provenance,
            })?;
            mapping_indexes.insert(mapping.route, index);

            match mapping.disposition {
                ProtocolPackRouteDisposition::Allowed { domain, decoder } => {
                    if decoder.domain() != domain {
                        return Err(ProtocolPackError::DecoderDomainMismatch {
                            route: mapping.route,
                            decoder,
                            decoder_domain: decoder.domain(),
                            declared_domain: domain,
                        });
                    }
                    privacy.allow_route(mapping.route, domain)?;
                    decoders.insert(mapping.route, decoder);
                }
                ProtocolPackRouteDisposition::Prohibited { class } => {
                    privacy.prohibit_route(mapping.route, class)?;
                }
                ProtocolPackRouteDisposition::Opaque => {}
            }
        }

        Ok(Self {
            definition,
            digest,
            catalog,
            privacy,
            decoders,
            mapping_indexes,
        })
    }

    pub fn from_json(json: &[u8]) -> Result<Self, ProtocolPackError> {
        let definition = serde_json::from_slice(json)
            .map_err(|error| ProtocolPackError::Serialization(error.to_string()))?;
        Self::build(definition)
    }

    pub fn definition(&self) -> &ProtocolPackDefinition {
        &self.definition
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }

    pub fn catalog(&self) -> &RouteCatalog {
        &self.catalog
    }

    pub fn privacy(&self) -> &ProtocolPrivacyPolicy {
        &self.privacy
    }

    pub fn decoder(&self, route: &RouteKey) -> Option<DecoderKind> {
        self.decoders.get(route).copied()
    }

    pub fn route(&self, route: &RouteKey) -> Option<&ProtocolPackRoute> {
        self.mapping_indexes
            .get(route)
            .map(|index| &self.definition.routes[*index])
    }

    pub fn disposition(&self, route: Option<&RouteKey>) -> DecodeDisposition {
        self.privacy.disposition(route)
    }

    pub fn matches(&self, build: &GameBuild) -> bool {
        self.definition.target.matches(build)
    }
}

#[derive(Debug, Default)]
pub struct ProtocolPackRegistry {
    packs: BTreeMap<String, ProtocolPack>,
}

impl ProtocolPackRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, pack: ProtocolPack) -> Result<(), ProtocolPackRegistryError> {
        if self.packs.contains_key(pack.digest()) {
            return Err(ProtocolPackRegistryError::DuplicateDigest(
                pack.digest().to_owned(),
            ));
        }
        self.packs.insert(pack.digest().to_owned(), pack);
        Ok(())
    }

    pub fn select(&self, build: &GameBuild) -> Result<&ProtocolPack, ProtocolPackRegistryError> {
        let mut candidates = self
            .packs
            .values()
            .filter(|pack| pack.matches(build))
            .collect::<Vec<_>>();

        let Some(best_specificity) = candidates
            .iter()
            .map(|pack| pack.definition.target.specificity())
            .max()
        else {
            return Err(ProtocolPackRegistryError::NoMatch {
                deployment_id: build.deployment_id.clone(),
                region_id: build.region_id.clone(),
                channel: build.channel.clone(),
                build_id: build.build_id.clone(),
                executable_version: build.executable_version.clone(),
            });
        };

        candidates.retain(|pack| pack.definition.target.specificity() == best_specificity);
        if candidates.len() != 1 {
            return Err(ProtocolPackRegistryError::Ambiguous {
                pack_ids: candidates
                    .iter()
                    .map(|pack| pack.definition.pack_id.clone())
                    .collect(),
            });
        }
        Ok(candidates[0])
    }

    pub fn len(&self) -> usize {
        self.packs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.packs.is_empty()
    }
}

fn validate_definition(definition: &ProtocolPackDefinition) -> Result<(), ProtocolPackError> {
    if definition.schema_version != PROTOCOL_PACK_SCHEMA_VERSION {
        return Err(ProtocolPackError::UnsupportedSchemaVersion {
            expected: PROTOCOL_PACK_SCHEMA_VERSION,
            actual: definition.schema_version,
        });
    }
    for (field, value) in [
        ("pack_id", definition.pack_id.as_str()),
        (
            "target.deployment_id",
            definition.target.deployment_id.as_str(),
        ),
        ("target.channel", definition.target.channel.as_str()),
        ("target.build_id", definition.target.build_id.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(ProtocolPackError::EmptyRequiredField(field));
        }
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum ProtocolPackError {
    #[error("unsupported protocol-pack schema version {actual}; expected {expected}")]
    UnsupportedSchemaVersion { expected: u16, actual: u16 },

    #[error("protocol-pack field {0} must not be empty")]
    EmptyRequiredField(&'static str),

    #[error("protocol-pack serialization failed: {0}")]
    Serialization(String),

    #[error(
        "decoder {decoder:?} uses {decoder_domain:?}, but route {route:?} declares {declared_domain:?}"
    )]
    DecoderDomainMismatch {
        route: RouteKey,
        decoder: DecoderKind,
        decoder_domain: AllowedDataDomain,
        declared_domain: AllowedDataDomain,
    },

    #[error(transparent)]
    Catalog(#[from] RouteCatalogError),

    #[error(transparent)]
    Privacy(#[from] PrivacyPolicyError),
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProtocolPackRegistryError {
    #[error("protocol pack with digest {0} is already registered")]
    DuplicateDigest(String),

    #[error(
        "no protocol pack matches deployment={deployment_id}, region={region_id:?}, channel={channel}, build={build_id}, executable={executable_version:?}"
    )]
    NoMatch {
        deployment_id: String,
        region_id: Option<String>,
        channel: String,
        build_id: String,
        executable_version: Option<String>,
    },

    #[error("multiple equally specific protocol packs match: {pack_ids:?}")]
    Ambiguous { pack_ids: Vec<String> },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FragmentKind, PacketDirection};

    fn route(method_id: u32) -> ProtocolPackRoute {
        ProtocolPackRoute {
            route: RouteKey::new(
                PacketDirection::ServerToClient,
                FragmentKind::Notify,
                10,
                method_id,
            ),
            service_name: "WorldNtf".into(),
            method_name: "EnterScene".into(),
            message_name: Some("EnterScene".into()),
            confidence: MappingConfidence::Verified,
            provenance: Vec::new(),
            features: vec![ProtocolFeature::Scene],
            disposition: ProtocolPackRouteDisposition::Allowed {
                domain: AllowedDataDomain::WorldState,
                decoder: DecoderKind::EnterSceneV1,
            },
        }
    }

    fn definition(pack_id: &str, region_id: Option<&str>) -> ProtocolPackDefinition {
        ProtocolPackDefinition {
            schema_version: PROTOCOL_PACK_SCHEMA_VERSION,
            pack_id: pack_id.into(),
            target: ProtocolPackTarget {
                deployment_id: "global".into(),
                region_id: region_id.map(str::to_owned),
                channel: "steam".into(),
                build_id: "build-1".into(),
                executable_version: None,
            },
            provenance: Vec::new(),
            routes: vec![route(3)],
        }
    }

    fn build(region_id: Option<&str>) -> GameBuild {
        GameBuild {
            deployment_id: "global".into(),
            region_id: region_id.map(str::to_owned),
            channel: "steam".into(),
            build_id: "build-1".into(),
            executable_version: None,
        }
    }

    #[test]
    fn digest_changes_when_route_mapping_changes() {
        let first = ProtocolPack::build(definition("one", None)).unwrap();
        let mut changed = definition("one", None);
        changed.routes[0].method_name = "Changed".into();
        let second = ProtocolPack::build(changed).unwrap();

        assert_ne!(first.digest(), second.digest());
        assert!(first.digest().starts_with("sha256:"));
    }

    #[test]
    fn exact_region_pack_wins_over_deployment_wide_pack() {
        let mut registry = ProtocolPackRegistry::new();
        registry
            .register(ProtocolPack::build(definition("wide", None)).unwrap())
            .unwrap();
        registry
            .register(ProtocolPack::build(definition("na", Some("north-america"))).unwrap())
            .unwrap();

        let selected = registry.select(&build(Some("north-america"))).unwrap();
        assert_eq!(selected.definition().pack_id, "na");
        let selected = registry.select(&build(Some("europe"))).unwrap();
        assert_eq!(selected.definition().pack_id, "wide");
    }

    #[test]
    fn nearby_build_is_never_selected() {
        let mut registry = ProtocolPackRegistry::new();
        registry
            .register(ProtocolPack::build(definition("one", None)).unwrap())
            .unwrap();
        let mut unknown = build(None);
        unknown.build_id = "build-2".into();

        assert!(matches!(
            registry.select(&unknown),
            Err(ProtocolPackRegistryError::NoMatch { .. })
        ));
    }

    #[test]
    fn equally_specific_matches_are_rejected_as_ambiguous() {
        let mut registry = ProtocolPackRegistry::new();
        registry
            .register(ProtocolPack::build(definition("one", None)).unwrap())
            .unwrap();
        registry
            .register(ProtocolPack::build(definition("two", None)).unwrap())
            .unwrap();

        assert!(matches!(
            registry.select(&build(None)),
            Err(ProtocolPackRegistryError::Ambiguous { .. })
        ));
    }

    #[test]
    fn decoder_domain_mismatch_is_rejected() {
        let mut invalid = definition("bad", None);
        invalid.routes[0].disposition = ProtocolPackRouteDisposition::Allowed {
            domain: AllowedDataDomain::Combat,
            decoder: DecoderKind::EnterSceneV1,
        };

        assert!(matches!(
            ProtocolPack::build(invalid),
            Err(ProtocolPackError::DecoderDomainMismatch { .. })
        ));
    }

    #[test]
    fn human_readable_reference_pack_is_valid_but_cannot_match_a_live_build() {
        let pack = ProtocolPack::from_json(include_bytes!(
            "../../../protocol-packs/global/reference-v1/pack.json"
        ))
        .unwrap();

        assert_eq!(
            pack.definition().pack_id,
            "global-reference-v1-not-for-live"
        );
        assert!(pack.catalog().len() >= 15);
        assert!(!pack.matches(&build(Some("north-america"))));
    }
}
