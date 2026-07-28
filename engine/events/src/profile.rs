use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{CharacterIdentity, SceneId};

/// A partial, privacy-reviewed character update.
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
    pub season_strength: Option<i64>,
    pub season: Option<SeasonProfile>,
    pub appearance: Option<CharacterAppearance>,
    pub equipment: Option<Vec<EquipmentItem>>,
    pub owned_imagines: Option<Vec<ImagineOwnership>>,
    pub active_skills: Option<Vec<SkillLevel>>,
    pub talents: Option<Vec<TalentLevel>>,
    pub combat_professions: Option<Vec<CombatProfessionProfile>>,
    pub life_professions: Option<Vec<LifeProfessionProfile>>,
    pub cosmetics: Option<Vec<CosmeticOwnership>>,
    pub collection_summary: Option<CollectionSummary>,
    pub social_display: Option<SocialDisplay>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharacterProgression {
    pub current_experience: Option<i64>,
    pub previous_season_max_level: Option<u32>,
}

/// Character-facing appearance IDs only.
///
/// User-supplied image URLs and account platform metadata are deliberately not
/// part of this contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CharacterAppearance {
    pub gender_id: Option<i32>,
    pub body_size_id: Option<i32>,
    pub height: Option<f32>,
    pub face_options: BTreeMap<i32, i32>,
    pub color_options: BTreeMap<i32, RgbColor>,
    pub avatar_id: Option<i32>,
    pub business_card_style_id: Option<i32>,
    pub avatar_frame_id: Option<i32>,
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
    pub enchantment_ids: Vec<i64>,
    pub set_id: Option<i64>,
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
    pub power: Option<i64>,
    pub strength: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CosmeticOwnership {
    pub cosmetic_id: i64,
    pub category_id: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillLevel {
    pub skill_id: i64,
    pub level: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TalentLevel {
    pub talent_id: i64,
    pub level: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CombatProfessionProfile {
    pub profession_id: i32,
    pub level: Option<u32>,
    pub experience: Option<i64>,
    pub active_skill_ids: Vec<i64>,
    pub slotted_skill_ids: BTreeMap<i32, i64>,
    pub weapon_skin_id: Option<i64>,
    pub talent_node_ids: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifeProfessionProfile {
    pub profession_id: i32,
    pub level: Option<u32>,
    pub experience: Option<i64>,
    pub specialization_levels: BTreeMap<i32, u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionSummary {
    pub fashion_points: Option<i64>,
    pub mount_points: Option<i64>,
    pub weapon_skin_points: Option<i64>,
    pub owned_fashion_ids: Vec<i64>,
    pub owned_mount_ids: Vec<i64>,
    pub owned_weapon_skin_ids: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SocialDisplay {
    pub guild_id: Option<i64>,
    pub title_ids: Vec<i64>,
    pub medal_ids: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldContext {
    pub scene_id: Option<SceneId>,
    pub map_id: Option<u32>,
    pub line_id: Option<u32>,
    pub scene_instance_id: Option<String>,
    pub dungeon_instance_id: Option<String>,
}
