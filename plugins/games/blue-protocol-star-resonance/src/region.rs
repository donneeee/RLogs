use std::collections::{BTreeSet, HashSet};
use std::net::IpAddr;
use std::str::FromStr;

use rlogs_events::{RegionEvidence, RegionEvidenceKind, RegionIdentity};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::NetworkEndpoint;

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
