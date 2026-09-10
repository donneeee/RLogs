//! Presentation metadata for equipped weapon configurations.
//!
//! The packet stream carries the equipped item ID for every observed party
//! member. The exact client `ItemTable` row selects the equipment-inventory
//! badge. This deliberately does not follow `WeaponSkinId`: cosmetic weapon
//! skins are a separate system and do not identify the equipped item. A
//! detailed local profile may additionally carry the per-instance
//! breakthrough count.

use std::sync::OnceLock;

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeaponPresentation {
    pub item_id: i64,
    /// Exact inventory badge referenced by this equipped item's current
    /// ItemTable row. Multiple item IDs may intentionally share one address.
    pub icon: &'static str,
    pub base_level: Option<u32>,
    pub max_level: Option<u32>,
    /// Base level followed by every reviewed breakthrough result level.
    pub level_progression: &'static [u32],
    pub badge_kind: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeaponLevelPresentation {
    pub exact: Option<u32>,
    pub minimum: u32,
    pub maximum: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WeaponPresentationRecord {
    item_id: i64,
    icon: &'static str,
    levels: &'static [u32],
    badge_kind: &'static str,
}

mod generated {
    include!("generated/weapon_presentation_data.rs");
}

pub fn weapon_presentation(item_id: i64) -> Option<WeaponPresentation> {
    let index = generated::WEAPON_PRESENTATION_RECORDS
        .binary_search_by_key(&item_id, |record| record.item_id)
        .ok()?;
    let record = generated::WEAPON_PRESENTATION_RECORDS[index];
    Some(WeaponPresentation {
        item_id: record.item_id,
        icon: record.icon,
        base_level: record.levels.first().copied(),
        max_level: record.levels.last().copied(),
        level_progression: record.levels,
        badge_kind: record.badge_kind,
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WeaponLocalizationCatalog {
    schema_version: u16,
    locale: String,
    deployment_id: String,
    client_build: String,
    protocol_pack_digest: String,
    source_item_table_sha256: String,
    weapons: Vec<(i64, String)>,
}

static WEAPON_NAMES: OnceLock<Result<WeaponLocalizationCatalog, String>> = OnceLock::new();

fn weapon_names() -> Result<&'static WeaponLocalizationCatalog, String> {
    WEAPON_NAMES
        .get_or_init(|| {
            let catalog: WeaponLocalizationCatalog = serde_json::from_str(include_str!(
                "../game-data/runtime/localization/en-US/weapon-names.v1.json"
            ))
            .map_err(|error| format!("bundled BPSR weapon localization is invalid: {error}"))?;
            if catalog.schema_version != 1
                || catalog.locale != "en-US"
                || catalog.deployment_id.trim().is_empty()
                || catalog.client_build.trim().is_empty()
                || catalog.protocol_pack_digest.len() != 71
                || catalog.source_item_table_sha256
                    != "a5807d7b028ab5fa90e76fb519aea77493c79637576471b2e458efddf1846f99"
                || catalog.weapons.len() != 722
                || catalog
                    .weapons
                    .windows(2)
                    .any(|pair| pair[0].0 >= pair[1].0)
                || catalog
                    .weapons
                    .iter()
                    .zip(generated::WEAPON_PRESENTATION_RECORDS)
                    .any(|((localized_id, _), presentation)| *localized_id != presentation.item_id)
                || catalog
                    .weapons
                    .iter()
                    .any(|(id, name)| *id <= 0 || name.trim().is_empty())
            {
                return Err("bundled BPSR weapon localization has an unsupported shape".into());
            }
            Ok(catalog)
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Resolves the official current-build English weapon name for an equipped
/// item. The exact Global English `ItemTable` label is the fallback for every
/// requested desktop locale until independently extracted locale bundles are
/// shipped. Unknown runtime identities and item IDs remain unresolved.
pub fn localized_weapon_name_for_identity(
    deployment_id: &str,
    client_build: &str,
    protocol_pack_digest: &str,
    item_id: i64,
    _locale: &str,
) -> Result<Option<&'static str>, String> {
    if !crate::bundled_localization_supports_identity(
        deployment_id,
        client_build,
        protocol_pack_digest,
    )? {
        return Ok(None);
    }
    let catalog = weapon_names()?;
    if deployment_id != catalog.deployment_id
        || client_build != catalog.client_build
        || protocol_pack_digest != catalog.protocol_pack_digest
    {
        return Ok(None);
    }
    Ok(catalog
        .weapons
        .binary_search_by_key(&item_id, |(id, _)| *id)
        .ok()
        .map(|index| catalog.weapons[index].1.as_str()))
}

pub fn weapon_level_presentation(
    item_id: i64,
    breakthrough_count: Option<u32>,
) -> Option<WeaponLevelPresentation> {
    let weapon = weapon_presentation(item_id)?;
    let minimum = weapon.base_level?;
    let maximum = weapon.max_level?;
    let exact = match weapon.level_progression {
        [fixed] => Some(*fixed),
        progression => breakthrough_count
            .map(|count| progression[(count as usize).min(progression.len().saturating_sub(1))]),
    };
    Some(WeaponLevelPresentation {
        exact,
        minimum,
        maximum,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_build_catalog_is_complete_sorted_and_unique() {
        assert_eq!(generated::WEAPON_PRESENTATION_RECORDS.len(), 722);
        assert!(
            generated::WEAPON_PRESENTATION_RECORDS
                .windows(2)
                .all(|pair| pair[0].item_id < pair[1].item_id)
        );
        assert!(
            generated::WEAPON_PRESENTATION_RECORDS
                .iter()
                .all(|record| !record.icon.is_empty())
        );
    }

    #[test]
    fn derives_exact_far_sea_levels_only_from_instance_breakthrough_evidence() {
        assert_eq!(
            weapon_level_presentation(2_000_617, Some(0)).unwrap().exact,
            Some(100)
        );
        assert_eq!(
            weapon_level_presentation(2_000_617, Some(1)).unwrap().exact,
            Some(140)
        );
        assert_eq!(
            weapon_level_presentation(2_000_617, Some(3)).unwrap().exact,
            Some(180)
        );
        assert_eq!(
            weapon_level_presentation(2_000_631, Some(0)).unwrap().exact,
            Some(220)
        );
        assert_eq!(
            weapon_level_presentation(2_000_631, Some(3)).unwrap().exact,
            Some(280)
        );
    }

    #[test]
    fn fixed_level_weapons_do_not_require_instance_breakthrough_evidence() {
        let weapon = weapon_presentation(2_000_551).unwrap();
        assert_eq!(weapon.icon, "icons/weapons/items/ch_wp_guitar_02_01.png");
        assert_eq!(
            weapon_level_presentation(2_000_551, None),
            Some(WeaponLevelPresentation {
                exact: Some(80),
                minimum: 80,
                maximum: 80,
            })
        );
    }

    #[test]
    fn missing_instance_evidence_remains_a_truthful_range() {
        assert_eq!(
            weapon_level_presentation(2_000_631, None),
            Some(WeaponLevelPresentation {
                exact: None,
                minimum: 220,
                maximum: 280,
            })
        );
        assert_eq!(weapon_level_presentation(9, Some(3)), None);
    }

    #[test]
    fn npc_empty_handed_items_are_mapped_without_inventing_a_level() {
        let weapon = weapon_presentation(2_000_112).unwrap();
        assert_eq!(
            weapon.icon,
            "icons/weapons/items/c_equip_icon_samurai01.png"
        );
        assert_eq!(weapon_level_presentation(2_000_112, None), None);
    }

    #[test]
    fn item_presentation_uses_exact_equipped_weapon_art_not_class_icons() {
        assert_eq!(
            weapon_presentation(2_000_631).unwrap().icon,
            "icons/weapons/items/ch_wp_rodri_06_01.png"
        );
        assert_eq!(
            weapon_presentation(2_000_633).unwrap().icon,
            "icons/weapons/items/ch_wp_guitar_06_01.png"
        );
    }

    #[test]
    fn current_new_weapon_items_are_mapped_without_following_cosmetic_skins() {
        let hand_cannon = weapon_presentation(2_000_106).unwrap();
        assert_eq!(hand_cannon.base_level, Some(10));

        let ceremonial_staff = weapon_presentation(2_000_108).unwrap();
        assert_eq!(ceremonial_staff.base_level, Some(10));

        // The current ItemTable intentionally assigns these entries the same
        // inventory badge. Their WeaponSkinTable art is not equipment identity.
        assert_eq!(hand_cannon.icon, ceremonial_staff.icon);
        assert_eq!(
            hand_cannon.icon,
            "icons/weapons/items/c_equip_icon_samurai01.png"
        );
    }

    #[test]
    fn weapon_names_require_the_exact_runtime_identity() {
        const DIGEST: &str =
            "sha256:4372050d9d549808b229b16de315080f9bac427efe9602dabd9b93c4502dbbae";
        assert_eq!(
            localized_weapon_name_for_identity("global", "24687926", DIGEST, 2_000_631, "en-US")
                .unwrap(),
            Some("Ember - Gaze of the Far Sea")
        );
        assert_eq!(
            localized_weapon_name_for_identity("global", "24687926", DIGEST, 2_000_631, "fr-FR")
                .unwrap(),
            Some("Ember - Gaze of the Far Sea")
        );
        assert_eq!(
            localized_weapon_name_for_identity(
                "global",
                "24687926",
                DIGEST,
                2_000_631,
                "unsupported",
            )
            .unwrap(),
            Some("Ember - Gaze of the Far Sea")
        );
        for (deployment, build, digest) in [
            ("cn", "24687926", DIGEST),
            ("global", "24687927", DIGEST),
            ("global", "24687926", "sha256:wrong"),
        ] {
            assert_eq!(
                localized_weapon_name_for_identity(deployment, build, digest, 2_000_631, "fr-FR")
                    .unwrap(),
                None
            );
        }
        assert_eq!(
            localized_weapon_name_for_identity("global", "24687926", DIGEST, 9, "en-US").unwrap(),
            None
        );
    }
}
