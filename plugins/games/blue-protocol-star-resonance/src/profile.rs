use std::collections::BTreeMap;

use rlogs_events::{CharacterIdentity, GameProfileEvent};
use serde::{Deserialize, Serialize};

use crate::{BPSR_GAME_PLUGIN_ID, BPSR_PROFILE_SCHEMA_ID, BPSR_PROFILE_SCHEMA_VERSION};

/// A partial, privacy-reviewed Blue Protocol character update.
///
/// `None` means the packet did not carry the field. Empty collections are
/// represented as `Some(Vec::new())`, so projections can distinguish an
/// authoritative clear from missing evidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CharacterProfilePatch {
    pub character: CharacterIdentity,
    pub display_name: Option<String>,
    pub display_id: Option<String>,
    pub server_id: Option<String>,
    pub class_id: Option<i32>,
    pub specialization_id: Option<i32>,
    pub level: Option<u32>,
    pub progression: Option<CharacterProgression>,
    pub combat_power: Option<i64>,
    #[serde(default)]
    pub combat_power_breakdown: Option<CombatPowerBreakdown>,
    /// Latest authoritative local-player fight-attribute snapshot.
    ///
    /// Only integer values belonging to the reviewed character-stat family
    /// are retained. Missing IDs stay unknown; an omitted value is never
    /// interpreted as zero. Live combat deltas are published through the
    /// Overlay runtime and do not overwrite this profile baseline.
    #[serde(default)]
    pub combat_stats: Option<CharacterCombatStatsProfile>,
    pub season_strength: Option<i64>,
    #[serde(default)]
    pub master_score: Option<i64>,
    pub season: Option<SeasonProfile>,
    pub appearance: Option<CharacterAppearance>,
    pub equipment: Option<Vec<EquipmentItem>>,
    /// Exact entries from `CharSerialize.equip.suit_info_dict`.
    ///
    /// The map key is retained without assigning an unproven set-family meaning.
    /// The BPSR plug-in resolves it against exact-build/runtime evidence separately.
    #[serde(default)]
    pub equipment_suit_entries: Option<Vec<EquipmentSuitEntryProfile>>,
    #[serde(default)]
    pub modules: Option<ModuleProfile>,
    pub owned_imagines: Option<Vec<ImagineOwnership>>,
    #[serde(default)]
    pub battle_imagine_skills: Option<Vec<BattleImagineSkill>>,
    /// Exact equipped action-bar bindings published by the current game snapshot.
    ///
    /// The game plug-in owns all packet decoding and later classifies these
    /// bindings into primary and auxiliary loadout slots. Consumers such as
    /// Combat History only receive the normalized result.
    #[serde(default)]
    pub equipped_action_slots: Option<Vec<EquippedActionSlot>>,
    pub active_skills: Option<Vec<SkillLevel>>,
    pub talents: Option<Vec<TalentLevel>>,
    #[serde(default)]
    pub talent_progress: Option<TalentProgressProfile>,
    pub combat_professions: Option<Vec<CombatProfessionProfile>>,
    pub life_professions: Option<Vec<LifeProfessionProfile>>,
    pub cosmetics: Option<Vec<CosmeticOwnership>>,
    pub collection_summary: Option<CollectionSummary>,
    #[serde(default)]
    pub activity_progress: Option<ActivityProgress>,
    #[serde(default)]
    pub season_medals: Option<SeasonMedalProfile>,
    #[serde(default)]
    pub season_cultivation: Option<Vec<SeasonCultivationProfile>>,
    #[serde(default)]
    pub reputations: Option<Vec<ReputationProgress>>,
    #[serde(default)]
    pub current_profession_project_id: Option<i32>,
    pub social_display: Option<SocialDisplay>,
}

pub const CHARACTER_COMBAT_STATS_SCHEMA_VERSION: u16 = 1;

/// Privacy-reviewed raw character-sheet values for the local character.
///
/// Values come from authoritative entity snapshots plus a current-build,
/// complete local `SyncToMeDeltaInfo.base_delta` character-sheet refresh.
/// Ordinary combat deltas and computed values are never persisted here. Keys
/// are exact BPSR fight-attribute component IDs. The final decimal digit
/// identifies the game-defined component lane (`0` through `5`); presentation
/// catalogs own localized names, units, grouping, and component labels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharacterCombatStatsProfile {
    pub schema_version: u16,
    #[serde(default)]
    pub snapshot_values: BTreeMap<i32, i64>,
}

impl CharacterCombatStatsProfile {
    pub fn new(snapshot_values: BTreeMap<i32, i64>) -> Self {
        Self {
            schema_version: CHARACTER_COMBAT_STATS_SCHEMA_VERSION,
            snapshot_values,
        }
    }
}

impl CharacterProfilePatch {
    pub fn into_game_event(self) -> Result<GameProfileEvent, serde_json::Error> {
        let character = self.character.clone();
        Ok(GameProfileEvent {
            game_plugin_id: BPSR_GAME_PLUGIN_ID.into(),
            payload_schema_id: BPSR_PROFILE_SCHEMA_ID.into(),
            payload_schema_version: BPSR_PROFILE_SCHEMA_VERSION,
            character,
            payload: serde_json::to_value(self)?,
        })
    }

    pub fn from_game_event(event: &GameProfileEvent) -> Result<Self, ProfileEventError> {
        if event.game_plugin_id != BPSR_GAME_PLUGIN_ID {
            return Err(ProfileEventError::WrongGamePlugin);
        }
        if event.payload_schema_id != BPSR_PROFILE_SCHEMA_ID
            || event.payload_schema_version != BPSR_PROFILE_SCHEMA_VERSION
        {
            return Err(ProfileEventError::UnsupportedSchema);
        }
        let profile: Self = serde_json::from_value(event.payload.clone())
            .map_err(ProfileEventError::InvalidBody)?;
        if profile.character != event.character {
            return Err(ProfileEventError::CharacterMismatch);
        }
        Ok(profile)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProfileEventError {
    #[error("profile event belongs to a different game plug-in")]
    WrongGamePlugin,

    #[error("unsupported Blue Protocol profile payload schema")]
    UnsupportedSchema,

    #[error("invalid Blue Protocol profile payload: {0}")]
    InvalidBody(serde_json::Error),

    #[error("profile payload character does not match the event carrier")]
    CharacterMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharacterProgression {
    pub current_experience: Option<i64>,
    pub previous_season_max_level: Option<u32>,
}

/// Character-facing appearance and game-reviewed profile images.
///
/// Account platform metadata and picture-verification internals are not part
/// of this contract. The decoder accepts only bounded HTTPS image URLs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CharacterAppearance {
    pub gender_id: Option<i32>,
    pub body_size_id: Option<i32>,
    pub height: Option<f32>,
    #[serde(default)]
    pub voice_id: Option<i32>,
    pub face_options: BTreeMap<i32, i32>,
    pub color_options: BTreeMap<i32, RgbColor>,
    pub avatar_id: Option<i32>,
    #[serde(default)]
    pub profile_image_url: Option<String>,
    #[serde(default)]
    pub half_body_image_url: Option<String>,
    pub business_card_style_id: Option<i32>,
    pub avatar_frame_id: Option<i32>,
    #[serde(default)]
    pub unlocked_profile_image_ids: Vec<i64>,
    #[serde(default)]
    pub unlocked_face_item_ids: Vec<i64>,
    #[serde(default)]
    pub unlocked_voice_ids: Vec<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RgbColor {
    pub red: i32,
    pub green: i32,
    pub blue: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EquipmentItem {
    pub slot_id: i32,
    pub item_id: i64,
    pub instance_id: Option<String>,
    pub level: Option<u32>,
    pub quality: Option<i32>,
    pub refinement_level: Option<u32>,
    #[serde(default)]
    pub refinement_failed_count: Option<u32>,
    #[serde(default)]
    pub attributes: Option<EquipmentAttributeProfile>,
    pub enchantment_ids: Vec<i64>,
    #[serde(default)]
    pub enchantments: Vec<EquipmentEnchantmentProfile>,
    pub set_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EquipmentAttributeProfile {
    pub base: BTreeMap<i32, i64>,
    pub basic: BTreeMap<i32, i64>,
    pub advanced: BTreeMap<i32, i64>,
    pub recast: BTreeMap<i32, i64>,
    pub rare_quality: BTreeMap<i32, i64>,
    pub perfection_value: Option<i32>,
    pub perfection_level: Option<i32>,
    pub max_perfection_value: Option<i32>,
    pub recast_count: Option<i32>,
    pub total_recast_count: Option<i32>,
    pub breakthrough_count: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EquipmentEnchantmentProfile {
    pub enchantment_id: i64,
    pub level: Option<u32>,
    pub enchantment_type: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EquipmentSuitEntryProfile {
    pub map_key: i32,
    pub attribute_type: Option<i32>,
    pub attributes: BTreeMap<i32, i32>,
}

/// Module inventory and equipped-slot state needed by profile displays and
/// deterministic optimizers.
///
/// Only package 5 (the game's module package) is projected. Instance IDs remain
/// strings so browsers never lose precision when joining inventory entries to
/// equipped slots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleProfile {
    pub equipped_slots: BTreeMap<i32, String>,
    pub inventory: Vec<ModuleItemProfile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleItemProfile {
    pub instance_id: String,
    pub config_id: i32,
    pub count: Option<i64>,
    pub quality: Option<i32>,
    pub load_flag: Option<i32>,
    pub module_type: Option<i32>,
    pub level: Option<u32>,
    pub parts: Vec<ModulePartProfile>,
    pub upgrade_records: Vec<ModuleUpgradeRecord>,
    pub success_rate: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModulePartProfile {
    pub part_id: i32,
    pub initial_link_points: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleUpgradeRecord {
    pub part_id: i32,
    pub succeeded: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImagineOwnership {
    pub imagine_id: i64,
    pub level: Option<u32>,
    pub breakthrough_level: Option<u32>,
    pub equipped_slot: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeasonProfile {
    pub season_id: Option<i64>,
    pub level: Option<u32>,
    #[serde(default)]
    pub experience: Option<i64>,
    pub power: Option<i64>,
    pub strength: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CombatPowerBreakdown {
    pub total: Option<i64>,
    pub components: Vec<CombatPowerComponent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CombatPowerComponent {
    pub function_type_id: i32,
    pub total_points: Option<i64>,
    pub points: Option<i64>,
    pub subcomponents: Vec<CombatPowerSubcomponent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CombatPowerSubcomponent {
    pub function_type_id: i32,
    pub root_function_type_id: Option<i32>,
    pub points: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CosmeticOwnership {
    pub cosmetic_id: i64,
    pub category_id: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillLevel {
    pub skill_id: i64,
    #[serde(default)]
    pub base_skill_id: Option<i64>,
    pub level: Option<u32>,
    #[serde(default)]
    pub remodel_level: Option<u32>,
    #[serde(default)]
    pub skin_id: Option<i64>,
    #[serde(default)]
    pub replacement_skill_ids: Vec<i64>,
    #[serde(default)]
    pub unlocked_skin_ids: Vec<i64>,
}

/// Battle Imagine skill evidence exposed by the current character snapshot.
///
/// Skill IDs are intentionally kept separate from Fantasy IDs and inventory
/// item IDs until an exact-build static crosswalk proves those relationships.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BattleImagineSkill {
    pub skill_id: i64,
    pub base_skill_id: Option<i64>,
    pub level: Option<u32>,
    pub remodel_level: Option<u32>,
    pub skin_id: Option<i64>,
    pub replacement_skill_ids: Vec<i64>,
    pub unlocked_skin_ids: Vec<i64>,
    pub equipped_slot: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EquippedActionSlot {
    pub slot_id: i32,
    pub skill_id: i64,
    pub auto_battle_disabled: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TalentLevel {
    pub talent_id: i64,
    /// Original talent-tree node ID carried by the character container.
    /// This is distinct from the localized talent definition ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<i64>,
    pub level: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TalentProgressProfile {
    pub total_points: Option<u32>,
    pub total_reset_count: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CombatProfessionProfile {
    pub profession_id: i32,
    pub level: Option<u32>,
    pub experience: Option<i64>,
    #[serde(default)]
    pub skills: Vec<SkillLevel>,
    pub active_skill_ids: Vec<i64>,
    pub slotted_skill_ids: BTreeMap<i32, i64>,
    pub weapon_skin_id: Option<i64>,
    pub talent_node_ids: Vec<i64>,
    #[serde(default)]
    pub talent_points_used: Option<u32>,
    #[serde(default)]
    pub talent_stage_config_id: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifeProfessionProfile {
    pub profession_id: i32,
    pub level: Option<u32>,
    pub experience: Option<i64>,
    pub specialization_levels: BTreeMap<i32, u32>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionSummary {
    /// Identifies which independent container managers were actually present
    /// in this observation. The profile projector uses this to apply an
    /// authoritative clear only to the corresponding subsection instead of
    /// erasing unrelated collection data carried by earlier packets.
    #[serde(
        default,
        skip_serializing_if = "CollectionObservationSections::is_empty"
    )]
    pub observed_sections: CollectionObservationSections,
    pub fashion_points: Option<i64>,
    pub mount_points: Option<i64>,
    pub weapon_skin_points: Option<i64>,
    #[serde(default)]
    pub equipped_fashion_ids: BTreeMap<i32, i64>,
    pub owned_fashion_ids: Vec<i64>,
    pub owned_mount_ids: Vec<i64>,
    pub owned_weapon_skin_ids: Vec<i64>,
    #[serde(default)]
    pub owned_dye_ids: Vec<i64>,
    #[serde(default)]
    pub unlocked_module_ids: Vec<i64>,
    #[serde(default)]
    pub ride_ids: Vec<i64>,
    #[serde(default)]
    pub ride_skin_ids: Vec<i64>,
    #[serde(default)]
    pub unlocked_emoji_ids: Vec<i64>,
    #[serde(default)]
    pub vanity_pet_ids: Vec<i64>,
    #[serde(default)]
    pub summoned_vanity_pet_id: Option<i64>,
    #[serde(default)]
    pub fantasy_atlas_stages: BTreeMap<i64, u32>,
    #[serde(default)]
    pub handbook: Option<HandbookProgress>,
    #[serde(default)]
    pub photo_ids: Vec<i64>,
    #[serde(default)]
    pub photo_wall: BTreeMap<i32, i64>,
    #[serde(default)]
    pub achievements: Option<AchievementProgressProfile>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionObservationSections {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fashion: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub collection_book: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub personal_zone: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub rides: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub emojis: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub handbook: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub vanity_pets: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fantasy_atlas: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub achievements: bool,
}

impl CollectionObservationSections {
    pub const fn is_empty(&self) -> bool {
        !self.fashion
            && !self.collection_book
            && !self.personal_zone
            && !self.rides
            && !self.emojis
            && !self.handbook
            && !self.vanity_pets
            && !self.fantasy_atlas
            && !self.achievements
    }

    pub fn merge(&mut self, newer: Self) {
        self.fashion |= newer.fashion;
        self.collection_book |= newer.collection_book;
        self.personal_zone |= newer.personal_zone;
        self.rides |= newer.rides;
        self.emojis |= newer.emojis;
        self.handbook |= newer.handbook;
        self.vanity_pets |= newer.vanity_pets;
        self.fantasy_atlas |= newer.fantasy_atlas;
        self.achievements |= newer.achievements;
    }
}

/// Achievement state from the character container. The game carries permanent
/// achievements under season ID zero and time-limited achievements under their
/// actual season IDs; keeping both collections explicit prevents accidental
/// aggregation across the two systems.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AchievementProgressProfile {
    pub general: Vec<AchievementProgress>,
    pub seasons: Vec<SeasonAchievementProgress>,
    pub initialized_season_ids: Vec<u32>,
    pub version: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AchievementProgress {
    pub achievement_id: u32,
    pub finish_count: Option<u32>,
    pub reward_claimed: Option<bool>,
    pub begin_progress: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeasonAchievementProgress {
    pub season_id: u32,
    pub achievements: Vec<AchievementProgress>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandbookProgress {
    pub important_people_ids: Vec<i64>,
    pub reading_book_ids: Vec<i64>,
    pub dictionary_entry_ids: Vec<i64>,
    pub postcard_ids: Vec<i64>,
    pub monthly_card_ids: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityProgress {
    pub challenge_dungeons: Vec<DungeonProgress>,
    pub challenge_targets: Vec<DungeonTargetProgress>,
    pub master_mode_dungeons: Vec<MasterModeDungeonProgress>,
    pub weekly_tower: Option<WeeklyTowerProgress>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DungeonProgress {
    pub dungeon_id: i32,
    pub completion_count: Option<u32>,
    pub award_state: Option<i32>,
    pub score: Option<i32>,
    /// Raw game value; the exact time unit is not promoted yet.
    pub pass_time: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DungeonTargetProgress {
    pub dungeon_id: i32,
    pub target_id: i32,
    pub progress: Option<i32>,
    pub award_state: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MasterModeDungeonProgress {
    pub season_id: i32,
    pub difficulty_id: i32,
    pub dungeon: DungeonProgress,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeeklyTowerProgress {
    pub rule_id: Option<i32>,
    pub maximum_floor_id: Option<i32>,
    pub previous_maximum_floor_id: Option<i32>,
    pub claimed_floor_ids: Vec<i32>,
    pub maximum_jump_reward_floor_id: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeasonMedalProfile {
    pub season_id: Option<u32>,
    pub normal_holes: Vec<SeasonMedalHole>,
    pub core_hole: Option<SeasonMedalHole>,
    pub core_nodes: Vec<SeasonMedalNode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeasonMedalHole {
    pub hole_id: u32,
    pub level: Option<u32>,
    pub current_experience: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeasonMedalNode {
    pub node_id: u32,
    pub level: Option<u32>,
    pub selected: Option<bool>,
    pub slot_id: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeasonCultivationProfile {
    pub season_id: i32,
    pub lines: Vec<CultivationLineProfile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CultivationLineProfile {
    pub line_type_id: i32,
    pub area_ids: Vec<i32>,
    pub areas: Vec<CultivationAreaProfile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CultivationAreaProfile {
    pub area_id: i32,
    pub active: Option<bool>,
    pub active_effect_score: Option<i32>,
    pub normal_node_levels: BTreeMap<i32, u32>,
    pub middle_node_item_ids: BTreeMap<i32, i64>,
    pub big_node_fantasy_ids: BTreeMap<i32, i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReputationProgress {
    pub reputation_id: u32,
    pub level: Option<u32>,
    pub experience: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SocialDisplay {
    pub guild_id: Option<i64>,
    #[serde(default)]
    pub guild_name: Option<String>,
    #[serde(default)]
    pub equipped_title_id: Option<i64>,
    #[serde(default)]
    pub equipped_title_level: Option<u32>,
    pub title_ids: Vec<i64>,
    pub medal_ids: Vec<i64>,
    #[serde(default)]
    pub medal_slots: BTreeMap<i32, i64>,
    #[serde(default)]
    pub profile_theme_id: Option<i64>,
}

#[cfg(test)]
mod tests {
    use rlogs_events::RegionIdentity;
    use serde_json::json;

    use super::*;

    #[test]
    fn typed_profile_round_trips_through_the_game_neutral_event() {
        let profile = CharacterProfilePatch {
            character: CharacterIdentity {
                region: RegionIdentity {
                    deployment_id: "global".into(),
                    region_id: "north-america".into(),
                    realm_id: None,
                    world_id: Some("world-7".into()),
                },
                character_id: "public-character-123".into(),
            },
            display_name: Some("Example".into()),
            display_id: None,
            server_id: Some("7".into()),
            class_id: Some(3),
            specialization_id: Some(2),
            level: Some(60),
            progression: None,
            combat_power: None,
            combat_power_breakdown: None,
            combat_stats: None,
            season_strength: None,
            master_score: None,
            season: None,
            appearance: None,
            equipment: None,
            equipment_suit_entries: None,
            modules: None,
            owned_imagines: None,
            battle_imagine_skills: None,
            equipped_action_slots: None,
            active_skills: None,
            talents: None,
            talent_progress: None,
            combat_professions: None,
            life_professions: None,
            cosmetics: None,
            collection_summary: None,
            activity_progress: None,
            season_medals: None,
            season_cultivation: None,
            reputations: None,
            current_profession_project_id: None,
            social_display: None,
        };

        let event = profile.clone().into_game_event().unwrap();
        assert_eq!(
            CharacterProfilePatch::from_game_event(&event).unwrap(),
            profile
        );
    }

    #[test]
    fn talent_profile_state_serializes_selection_only_without_static_tree_data() {
        let talent = TalentLevel {
            talent_id: 3_061,
            node_id: Some(30_061),
            level: Some(1),
        };

        let value = serde_json::to_value(talent).unwrap();
        assert_eq!(
            value,
            json!({
                "talent_id": 3_061,
                "node_id": 30_061,
                "level": 1
            })
        );

        let object = value.as_object().unwrap();
        for website_owned_field in [
            "name",
            "description",
            "icon",
            "icon_address",
            "x",
            "y",
            "branch",
            "prerequisites",
            "dependents",
            "specialization_name",
        ] {
            assert!(
                !object.contains_key(website_owned_field),
                "profile talent state must not upload website-owned field {website_owned_field}"
            );
        }
    }
}
