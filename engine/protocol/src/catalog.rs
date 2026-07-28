use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::RouteKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MappingConfidence {
    Verified,
    Imported,
    Candidate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MappingProvenance {
    pub source: String,
    pub reference: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteDefinition {
    pub route: RouteKey,
    pub service_name: String,
    pub method_name: String,
    pub message_name: Option<String>,
    pub confidence: MappingConfidence,
    pub provenance: Vec<MappingProvenance>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RouteCatalog {
    definitions: BTreeMap<RouteKey, RouteDefinition>,
}

impl RouteCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, definition: RouteDefinition) -> Result<(), RouteCatalogError> {
        let route = definition.route;
        if self.definitions.contains_key(&route) {
            return Err(RouteCatalogError::DuplicateRoute(route));
        }
        self.definitions.insert(route, definition);
        Ok(())
    }

    pub fn get(&self, route: &RouteKey) -> Option<&RouteDefinition> {
        self.definitions.get(route)
    }

    pub fn contains(&self, route: &RouteKey) -> bool {
        self.definitions.contains_key(route)
    }

    pub fn len(&self) -> usize {
        self.definitions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.definitions.is_empty()
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RouteCatalogError {
    #[error("route is already defined: {0:?}")]
    DuplicateRoute(RouteKey),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FragmentKind, PacketDirection};

    fn definition() -> RouteDefinition {
        RouteDefinition {
            route: RouteKey::new(
                PacketDirection::ServerToClient,
                FragmentKind::Notify,
                1_664_308_034,
                3,
            ),
            service_name: "WorldNtf".into(),
            method_name: "EnterScene".into(),
            message_name: Some("EnterScene".into()),
            confidence: MappingConfidence::Imported,
            provenance: vec![MappingProvenance {
                source: "documented-research-fixture".into(),
                reference: "tests/fixtures/example-route.json".into(),
            }],
        }
    }

    #[test]
    fn duplicate_routes_are_rejected_instead_of_overwritten() {
        let mut catalog = RouteCatalog::new();
        let route = definition().route;

        assert_eq!(catalog.insert(definition()), Ok(()));
        assert_eq!(
            catalog.insert(definition()),
            Err(RouteCatalogError::DuplicateRoute(route))
        );
        assert_eq!(catalog.len(), 1);
    }
}
