use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

pub const FIGHT_ATTRIBUTE_PRESENTATION_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FightAttributePresentation {
    pub attribute_id: i32,
    pub family_id: i32,
    pub component: String,
    pub name: String,
    pub description: Option<String>,
    pub number_type: i32,
    pub format_type: i32,
    pub icon: Option<String>,
    pub displayable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FightAttributePresentationCatalog {
    pub schema_version: u16,
    pub game_build: String,
    pub locale: String,
    pub source: String,
    pub source_sha256: String,
    pub attributes: Vec<FightAttributePresentation>,
}

static CATALOG: OnceLock<Result<FightAttributePresentationCatalog, String>> = OnceLock::new();

pub fn fight_attribute_presentation_catalog()
-> Result<&'static FightAttributePresentationCatalog, String> {
    CATALOG
        .get_or_init(|| {
            let catalog: FightAttributePresentationCatalog = serde_json::from_str(include_str!(
                "../game-data/runtime/fight-attribute-presentation.v1.json"
            ))
            .map_err(|error| {
                format!("bundled BPSR Fight Attribute presentation is invalid: {error}")
            })?;
            if catalog.schema_version != FIGHT_ATTRIBUTE_PRESENTATION_SCHEMA_VERSION
                || catalog.game_build != "24687926"
                || catalog.locale != "en-US"
                || catalog.source.trim().is_empty()
                || catalog.source_sha256.len() != 64
                || catalog.attributes.len() != 906
                || catalog
                    .attributes
                    .windows(2)
                    .any(|pair| pair[0].attribute_id >= pair[1].attribute_id)
                || catalog.attributes.iter().any(invalid_attribute)
            {
                return Err(
                    "bundled BPSR Fight Attribute presentation has an unsupported shape".into(),
                );
            }
            Ok(catalog)
        })
        .as_ref()
        .map_err(Clone::clone)
}

pub fn fight_attribute_presentation(
    attribute_id: i32,
) -> Result<Option<&'static FightAttributePresentation>, String> {
    let catalog = fight_attribute_presentation_catalog()?;
    Ok(catalog
        .attributes
        .binary_search_by_key(&attribute_id, |attribute| attribute.attribute_id)
        .ok()
        .map(|index| &catalog.attributes[index]))
}

fn invalid_attribute(attribute: &FightAttributePresentation) -> bool {
    attribute.attribute_id <= 0
        || attribute.family_id <= 0
        || !matches!(
            attribute.component.as_str(),
            "final" | "total" | "add" | "extra_add" | "percent" | "extra_percent"
        )
        || attribute.name.trim().is_empty()
        || !matches!(attribute.number_type, 0..=2)
        || !matches!(attribute.format_type, 0..=5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_exact_build_complete_and_searchable() {
        let catalog = fight_attribute_presentation_catalog().unwrap();
        assert_eq!(catalog.game_build, "24687926");
        assert_eq!(catalog.attributes.len(), 906);

        let attack = fight_attribute_presentation(11_330).unwrap().unwrap();
        assert_eq!(attack.family_id, 11_330);
        assert_eq!(attack.component, "final");
        assert_eq!(attack.name, "ATK");
        assert!(attack.displayable);
        assert!(fight_attribute_presentation(9_999).unwrap().is_none());
        assert!(fight_attribute_presentation(0).unwrap().is_none());
    }

    #[test]
    fn component_members_share_their_exact_family_identity() {
        for attribute_id in 11_330..=11_335 {
            let member = fight_attribute_presentation(attribute_id).unwrap().unwrap();
            assert_eq!(member.family_id, 11_330);
            assert_eq!(member.format_type, attribute_id % 10);
        }
    }
}
