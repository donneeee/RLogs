//! Presentation metadata for equipped weapon configurations.
//!
//! The packet stream carries the equipped item ID for every observed party
//! member. The exact client `ItemTable` row selects the equipment-inventory
//! badge. This deliberately does not follow `WeaponSkinId`: cosmetic weapon
//! skins are a separate system and do not identify the equipped item. A
//! detailed local profile may additionally carry the per-instance
//! breakthrough count.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeaponPresentation {
    pub item_id: i64,
    pub english_name: &'static str,
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
    english_name: &'static str,
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
        english_name: record.english_name,
        icon: record.icon,
        base_level: record.levels.first().copied(),
        max_level: record.levels.last().copied(),
        level_progression: record.levels,
        badge_kind: record.badge_kind,
    })
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
                .all(|record| !record.english_name.is_empty() && !record.icon.is_empty())
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
        assert_eq!(weapon.english_name, "Celestial Pyre - Silent");
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
        assert_eq!(weapon.english_name, "Lucy (empty-handed)");
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
        assert_eq!(hand_cannon.english_name, "Thunder Strike");
        assert_eq!(hand_cannon.base_level, Some(10));

        let ceremonial_staff = weapon_presentation(2_000_108).unwrap();
        assert_eq!(ceremonial_staff.english_name, "Dark Spirit's Prayer");
        assert_eq!(ceremonial_staff.base_level, Some(10));

        // The current ItemTable intentionally assigns these entries the same
        // inventory badge. Their WeaponSkinTable art is not equipment identity.
        assert_eq!(hand_cannon.icon, ceremonial_staff.icon);
        assert_eq!(
            hand_cannon.icon,
            "icons/weapons/items/c_equip_icon_samurai01.png"
        );
    }
}
