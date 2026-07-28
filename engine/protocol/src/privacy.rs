use std::collections::BTreeMap;

use thiserror::Error;

use crate::RouteKey;

/// Product domains that may be schema-decoded after an explicit route review.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllowedDataDomain {
    Combat,
    Encounter,
    CharacterProfile,
    WorldState,
}

/// Private data classes that RLogs must never schema-decode or normalize.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProhibitedDataClass {
    PasswordOrCredential,
    AuthenticationToken,
    PrivateAccountData,
    PaymentData,
    PrivateCommunication,
}

/// Schema decoding is allowlisted. Observing a route never grants permission
/// to interpret its payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeDisposition {
    Allowed(AllowedDataDomain),
    OpaqueLocalOnly,
    Prohibited(ProhibitedDataClass),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProtocolPrivacyPolicy {
    allowed_routes: BTreeMap<RouteKey, AllowedDataDomain>,
    prohibited_routes: BTreeMap<RouteKey, ProhibitedDataClass>,
}

impl ProtocolPrivacyPolicy {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn allow_route(
        &mut self,
        route: RouteKey,
        domain: AllowedDataDomain,
    ) -> Result<(), PrivacyPolicyError> {
        if let Some(class) = self.prohibited_routes.get(&route) {
            return Err(PrivacyPolicyError::ConflictingRoute {
                route,
                existing: DecodeDisposition::Prohibited(*class),
                requested: DecodeDisposition::Allowed(domain),
            });
        }

        if let Some(existing) = self.allowed_routes.get(&route).copied() {
            return if existing == domain {
                Ok(())
            } else {
                Err(PrivacyPolicyError::ConflictingRoute {
                    route,
                    existing: DecodeDisposition::Allowed(existing),
                    requested: DecodeDisposition::Allowed(domain),
                })
            };
        }
        self.allowed_routes.insert(route, domain);
        Ok(())
    }

    pub fn prohibit_route(
        &mut self,
        route: RouteKey,
        class: ProhibitedDataClass,
    ) -> Result<(), PrivacyPolicyError> {
        if let Some(domain) = self.allowed_routes.get(&route) {
            return Err(PrivacyPolicyError::ConflictingRoute {
                route,
                existing: DecodeDisposition::Allowed(*domain),
                requested: DecodeDisposition::Prohibited(class),
            });
        }
        if let Some(existing) = self.prohibited_routes.get(&route).copied() {
            return if existing == class {
                Ok(())
            } else {
                Err(PrivacyPolicyError::ConflictingRoute {
                    route,
                    existing: DecodeDisposition::Prohibited(existing),
                    requested: DecodeDisposition::Prohibited(class),
                })
            };
        }
        self.prohibited_routes.insert(route, class);
        Ok(())
    }

    /// Unknown, unrouted, and merely observed traffic stays opaque by default.
    pub fn disposition(&self, route: Option<&RouteKey>) -> DecodeDisposition {
        let Some(route) = route else {
            return DecodeDisposition::OpaqueLocalOnly;
        };
        if let Some(class) = self.prohibited_routes.get(route) {
            return DecodeDisposition::Prohibited(*class);
        }
        if let Some(domain) = self.allowed_routes.get(route) {
            return DecodeDisposition::Allowed(*domain);
        }
        DecodeDisposition::OpaqueLocalOnly
    }

    pub fn allowed_routes(&self) -> &BTreeMap<RouteKey, AllowedDataDomain> {
        &self.allowed_routes
    }

    pub fn prohibited_routes(&self) -> &BTreeMap<RouteKey, ProhibitedDataClass> {
        &self.prohibited_routes
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PrivacyPolicyError {
    #[error(
        "route {route:?} already has privacy disposition {existing:?}; cannot assign {requested:?}"
    )]
    ConflictingRoute {
        route: RouteKey,
        existing: DecodeDisposition,
        requested: DecodeDisposition,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FragmentKind, PacketDirection};

    fn route(method_id: u32) -> RouteKey {
        RouteKey::new(
            PacketDirection::ServerToClient,
            FragmentKind::Notify,
            10,
            method_id,
        )
    }

    #[test]
    fn unknown_and_unrouted_packets_are_opaque_by_default() {
        let policy = ProtocolPrivacyPolicy::new();

        assert_eq!(
            policy.disposition(Some(&route(1))),
            DecodeDisposition::OpaqueLocalOnly
        );
        assert_eq!(policy.disposition(None), DecodeDisposition::OpaqueLocalOnly);
    }

    #[test]
    fn character_profile_routes_are_an_explicit_allowed_domain() {
        let mut policy = ProtocolPrivacyPolicy::new();
        policy
            .allow_route(route(1), AllowedDataDomain::CharacterProfile)
            .unwrap();

        assert_eq!(
            policy.disposition(Some(&route(1))),
            DecodeDisposition::Allowed(AllowedDataDomain::CharacterProfile)
        );
        assert_eq!(
            policy.disposition(Some(&route(2))),
            DecodeDisposition::OpaqueLocalOnly
        );
    }

    #[test]
    fn prohibited_auth_routes_cannot_be_promoted_to_character_data() {
        let mut policy = ProtocolPrivacyPolicy::new();
        policy
            .prohibit_route(route(1), ProhibitedDataClass::PasswordOrCredential)
            .unwrap();

        assert_eq!(
            policy.disposition(Some(&route(1))),
            DecodeDisposition::Prohibited(ProhibitedDataClass::PasswordOrCredential)
        );
        assert!(matches!(
            policy.allow_route(route(1), AllowedDataDomain::CharacterProfile),
            Err(PrivacyPolicyError::ConflictingRoute { .. })
        ));
    }

    #[test]
    fn character_routes_cannot_be_reclassified_as_account_private() {
        let mut policy = ProtocolPrivacyPolicy::new();
        policy
            .allow_route(route(1), AllowedDataDomain::CharacterProfile)
            .unwrap();

        assert!(matches!(
            policy.prohibit_route(route(1), ProhibitedDataClass::PrivateAccountData),
            Err(PrivacyPolicyError::ConflictingRoute { .. })
        ));
    }
}
