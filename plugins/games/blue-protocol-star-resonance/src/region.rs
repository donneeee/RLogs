use std::collections::{BTreeSet, HashSet};
use std::net::IpAddr;
use std::str::FromStr;

use rlogs_events::{RegionEvidence, RegionEvidenceKind, RegionIdentity};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::NetworkEndpoint;

pub const SERVER_REALM_CATALOG_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerRealmCatalogDefinition {
    pub schema_version: u16,
    pub deployment_id: String,
    pub realms: Vec<ServerRealmDefinition>,
    #[serde(default)]
    pub endpoint_rules: Vec<RegionEndpointRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerRealmDefinition {
    pub realm_id: String,
    pub display_name: String,
    /// Geographic region, when verified independently from the realm name.
    pub region_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegionEndpointRule {
    pub rule_id: String,
    pub identity: RegionIdentity,
    /// IPv4 or IPv6 CIDR. Exact endpoints use `/32` or `/128`.
    pub cidr: String,
    /// Empty means every port on this endpoint group.
    #[serde(default)]
    pub ports: Vec<u16>,
}

#[derive(Debug, Clone)]
struct CompiledRegionRule {
    source: RegionEndpointRule,
    network: IpNetwork,
}

#[derive(Debug, Clone)]
pub struct RegionResolver {
    rules: Vec<CompiledRegionRule>,
}

#[derive(Debug, Clone)]
pub struct ServerRealmCatalog {
    definition: ServerRealmCatalogDefinition,
    resolver: RegionResolver,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRegion {
    pub identity: RegionIdentity,
    pub evidence: Vec<RegionEvidence>,
}

impl RegionResolver {
    pub fn build(rules: Vec<RegionEndpointRule>) -> Result<Self, RegionResolverError> {
        let mut ids = BTreeSet::new();
        let mut compiled = Vec::with_capacity(rules.len());
        for mut rule in rules {
            if rule.rule_id.trim().is_empty() {
                return Err(RegionResolverError::EmptyRuleId);
            }
            if !ids.insert(rule.rule_id.clone()) {
                return Err(RegionResolverError::DuplicateRuleId(rule.rule_id));
            }
            rule.ports.sort_unstable();
            rule.ports.dedup();
            let network = IpNetwork::from_str(&rule.cidr)
                .map_err(|_| RegionResolverError::InvalidCidr(rule.cidr.clone()))?;
            compiled.push(CompiledRegionRule {
                source: rule,
                network,
            });
        }
        compiled.sort_by(|left, right| left.source.rule_id.cmp(&right.source.rule_id));
        Ok(Self { rules: compiled })
    }

    pub fn resolve(
        &self,
        endpoint: &NetworkEndpoint,
    ) -> Result<ResolvedRegion, RegionResolverError> {
        let address = IpAddr::from_str(&endpoint.address)
            .map_err(|_| RegionResolverError::InvalidEndpoint(endpoint.address.clone()))?;
        let mut candidates = self
            .rules
            .iter()
            .filter(|rule| {
                rule.network.contains(address)
                    && (rule.source.ports.is_empty()
                        || rule.source.ports.binary_search(&endpoint.port).is_ok())
            })
            .collect::<Vec<_>>();
        let Some(specificity) = candidates
            .iter()
            .map(|rule| {
                (
                    rule.network.prefix_length(),
                    u8::from(!rule.source.ports.is_empty()),
                )
            })
            .max()
        else {
            return Err(RegionResolverError::NoMatch {
                address: endpoint.address.clone(),
                port: endpoint.port,
            });
        };
        candidates.retain(|rule| {
            (
                rule.network.prefix_length(),
                u8::from(!rule.source.ports.is_empty()),
            ) == specificity
        });

        let identities = candidates
            .iter()
            .map(|rule| rule.source.identity.clone())
            .collect::<HashSet<_>>();
        if identities.len() != 1 {
            return Err(RegionResolverError::Ambiguous {
                rule_ids: candidates
                    .iter()
                    .map(|rule| rule.source.rule_id.clone())
                    .collect(),
            });
        }
        let identity = identities.into_iter().next().expect("one identity");
        let evidence = candidates
            .iter()
            .map(|rule| RegionEvidence {
                kind: RegionEvidenceKind::ConnectionEndpoint,
                reference: rule.source.rule_id.clone(),
            })
            .collect();
        Ok(ResolvedRegion { identity, evidence })
    }
}

impl ServerRealmCatalog {
    pub fn build(
        definition: ServerRealmCatalogDefinition,
    ) -> Result<Self, ServerRealmCatalogError> {
        if definition.schema_version != SERVER_REALM_CATALOG_SCHEMA_VERSION {
            return Err(ServerRealmCatalogError::UnsupportedSchemaVersion(
                definition.schema_version,
            ));
        }
        validate_identifier(&definition.deployment_id)
            .map_err(|_| ServerRealmCatalogError::InvalidDeploymentId)?;

        let mut realm_ids = BTreeSet::new();
        for realm in &definition.realms {
            validate_identifier(&realm.realm_id)
                .map_err(|_| ServerRealmCatalogError::InvalidRealmId(realm.realm_id.clone()))?;
            if !realm_ids.insert(realm.realm_id.clone()) {
                return Err(ServerRealmCatalogError::DuplicateRealmId(
                    realm.realm_id.clone(),
                ));
            }
            if realm.display_name.trim().is_empty() {
                return Err(ServerRealmCatalogError::EmptyDisplayName(
                    realm.realm_id.clone(),
                ));
            }
            if let Some(region_id) = &realm.region_id {
                validate_identifier(region_id).map_err(|_| {
                    ServerRealmCatalogError::InvalidRegionId {
                        realm_id: realm.realm_id.clone(),
                        region_id: region_id.clone(),
                    }
                })?;
            }
        }

        for rule in &definition.endpoint_rules {
            if rule.identity.deployment_id != definition.deployment_id {
                return Err(ServerRealmCatalogError::DeploymentMismatch(
                    rule.rule_id.clone(),
                ));
            }
            let realm_id = rule
                .identity
                .realm_id
                .as_ref()
                .ok_or_else(|| ServerRealmCatalogError::MissingRealmId(rule.rule_id.clone()))?;
            let realm = definition
                .realms
                .iter()
                .find(|realm| &realm.realm_id == realm_id)
                .ok_or_else(|| ServerRealmCatalogError::UnknownRealmId {
                    rule_id: rule.rule_id.clone(),
                    realm_id: realm_id.clone(),
                })?;
            let expected_region_id = realm.region_id.as_deref().unwrap_or("unknown");
            if rule.identity.region_id != expected_region_id {
                return Err(ServerRealmCatalogError::RegionMismatch {
                    rule_id: rule.rule_id.clone(),
                    expected: expected_region_id.to_owned(),
                    actual: rule.identity.region_id.clone(),
                });
            }
        }

        let resolver = RegionResolver::build(definition.endpoint_rules.clone())?;
        Ok(Self {
            definition,
            resolver,
        })
    }

    pub fn from_json(json: &[u8]) -> Result<Self, ServerRealmCatalogError> {
        let definition = serde_json::from_slice(json)
            .map_err(|error| ServerRealmCatalogError::Serialization(error.to_string()))?;
        Self::build(definition)
    }

    pub fn definition(&self) -> &ServerRealmCatalogDefinition {
        &self.definition
    }

    pub fn resolve(
        &self,
        endpoint: &NetworkEndpoint,
    ) -> Result<ResolvedRegion, RegionResolverError> {
        self.resolver.resolve(endpoint)
    }
}

fn validate_identifier(value: &str) -> Result<(), ()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(());
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum IpNetwork {
    V4 { network: u32, prefix: u8 },
    V6 { network: u128, prefix: u8 },
}

impl IpNetwork {
    fn contains(self, address: IpAddr) -> bool {
        match (self, address) {
            (Self::V4 { network, prefix }, IpAddr::V4(address)) => {
                let value = u32::from(address);
                value & ipv4_mask(prefix) == network
            }
            (Self::V6 { network, prefix }, IpAddr::V6(address)) => {
                let value = u128::from(address);
                value & ipv6_mask(prefix) == network
            }
            _ => false,
        }
    }

    const fn prefix_length(self) -> u8 {
        match self {
            Self::V4 { prefix, .. } | Self::V6 { prefix, .. } => prefix,
        }
    }
}

impl FromStr for IpNetwork {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (address, prefix) = value.split_once('/').ok_or(())?;
        let address = IpAddr::from_str(address).map_err(|_| ())?;
        let prefix = u8::from_str(prefix).map_err(|_| ())?;
        match address {
            IpAddr::V4(address) if prefix <= 32 => {
                let mask = ipv4_mask(prefix);
                Ok(Self::V4 {
                    network: u32::from(address) & mask,
                    prefix,
                })
            }
            IpAddr::V6(address) if prefix <= 128 => {
                let mask = ipv6_mask(prefix);
                Ok(Self::V6 {
                    network: u128::from(address) & mask,
                    prefix,
                })
            }
            _ => Err(()),
        }
    }
}

const fn ipv4_mask(prefix: u8) -> u32 {
    if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    }
}

const fn ipv6_mask(prefix: u8) -> u128 {
    if prefix == 0 {
        0
    } else {
        u128::MAX << (128 - prefix)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RegionResolverError {
    #[error("region rule ID must not be empty")]
    EmptyRuleId,

    #[error("duplicate region rule ID {0}")]
    DuplicateRuleId(String),

    #[error("invalid region CIDR {0}")]
    InvalidCidr(String),

    #[error("invalid endpoint address {0}")]
    InvalidEndpoint(String),

    #[error("no region rule matches {address}:{port}")]
    NoMatch { address: String, port: u16 },

    #[error("equally specific region rules disagree: {rule_ids:?}")]
    Ambiguous { rule_ids: Vec<String> },
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ServerRealmCatalogError {
    #[error("unsupported server realm catalog schema version {0}")]
    UnsupportedSchemaVersion(u16),

    #[error("server realm catalog deployment ID is invalid")]
    InvalidDeploymentId,

    #[error("server realm ID is invalid: {0}")]
    InvalidRealmId(String),

    #[error("duplicate server realm ID {0}")]
    DuplicateRealmId(String),

    #[error("server realm {0} has an empty display name")]
    EmptyDisplayName(String),

    #[error("server realm {realm_id} has invalid region ID {region_id}")]
    InvalidRegionId { realm_id: String, region_id: String },

    #[error("endpoint rule {0} belongs to another deployment")]
    DeploymentMismatch(String),

    #[error("endpoint rule {0} does not identify a realm")]
    MissingRealmId(String),

    #[error("endpoint rule {rule_id} identifies unknown realm {realm_id}")]
    UnknownRealmId { rule_id: String, realm_id: String },

    #[error(
        "endpoint rule {rule_id} uses region {actual}, but its realm is cataloged as {expected}"
    )]
    RegionMismatch {
        rule_id: String,
        expected: String,
        actual: String,
    },

    #[error("could not parse server realm catalog: {0}")]
    Serialization(String),

    #[error(transparent)]
    Resolver(#[from] RegionResolverError),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(region_id: &str) -> RegionIdentity {
        RegionIdentity {
            deployment_id: "global".into(),
            region_id: region_id.into(),
            realm_id: None,
            world_id: None,
        }
    }

    fn endpoint(address: &str, port: u16) -> NetworkEndpoint {
        NetworkEndpoint {
            address: address.into(),
            port,
        }
    }

    #[test]
    fn global_catalog_resolves_only_the_exact_user_confirmed_asteria_endpoint() {
        let catalog = ServerRealmCatalog::from_json(include_bytes!(
            "../protocol-packs/global/server-realms.json"
        ))
        .unwrap();
        let definition = catalog.definition();

        assert_eq!(definition.deployment_id, "global");
        assert_eq!(
            definition
                .realms
                .iter()
                .map(|realm| realm.realm_id.as_str())
                .collect::<Vec<_>>(),
            vec!["asteria", "bahamar"]
        );
        assert!(
            definition
                .realms
                .iter()
                .all(|realm| realm.region_id.is_none())
        );
        assert_eq!(definition.endpoint_rules.len(), 1);
        let resolved = catalog
            .resolve(&endpoint("43.174.232.118", 10_099))
            .unwrap();
        assert_eq!(resolved.identity.region_id, "unknown");
        assert_eq!(resolved.identity.realm_id.as_deref(), Some("asteria"));
        assert_eq!(
            resolved.evidence[0].reference,
            "global-asteria-world-load-001"
        );
        assert!(matches!(
            catalog.resolve(&endpoint("43.174.232.118", 10_098)),
            Err(RegionResolverError::NoMatch { .. })
        ));
    }

    #[test]
    fn exact_port_rule_wins_and_emits_privacy_safe_evidence() {
        let resolver = RegionResolver::build(vec![
            RegionEndpointRule {
                rule_id: "global-default".into(),
                identity: identity("unknown"),
                cidr: "203.0.113.0/24".into(),
                ports: Vec::new(),
            },
            RegionEndpointRule {
                rule_id: "global-na-game".into(),
                identity: identity("north-america"),
                cidr: "203.0.113.0/24".into(),
                ports: vec![443],
            },
        ])
        .unwrap();

        let resolved = resolver.resolve(&endpoint("203.0.113.5", 443)).unwrap();
        assert_eq!(resolved.identity.region_id, "north-america");
        assert_eq!(resolved.evidence[0].reference, "global-na-game");
        assert!(!resolved.evidence[0].reference.contains("203.0.113.5"));
    }

    #[test]
    fn ipv6_rules_are_supported() {
        let resolver = RegionResolver::build(vec![RegionEndpointRule {
            rule_id: "global-eu-v6".into(),
            identity: identity("europe"),
            cidr: "2001:db8:1234::/48".into(),
            ports: Vec::new(),
        }])
        .unwrap();

        assert_eq!(
            resolver
                .resolve(&endpoint("2001:db8:1234::99", 4000))
                .unwrap()
                .identity
                .region_id,
            "europe"
        );
    }

    #[test]
    fn equally_specific_disagreement_is_not_guessed() {
        let resolver = RegionResolver::build(vec![
            RegionEndpointRule {
                rule_id: "one".into(),
                identity: identity("north-america"),
                cidr: "198.51.100.0/24".into(),
                ports: Vec::new(),
            },
            RegionEndpointRule {
                rule_id: "two".into(),
                identity: identity("europe"),
                cidr: "198.51.100.0/24".into(),
                ports: Vec::new(),
            },
        ])
        .unwrap();

        assert!(matches!(
            resolver.resolve(&endpoint("198.51.100.7", 1234)),
            Err(RegionResolverError::Ambiguous { .. })
        ));
    }
}
