//! Privacy-reviewed partial protobuf schemas for the first Global/CN lineage.
//!
//! These messages intentionally declare only gameplay fields used by RLogs.
//! Prost skips undeclared fields without materializing them. In particular,
//! the character-base schema does not declare account IDs, open IDs, login
//! state, credentials, tokens, or private account-security fields.

use prost::Message;

#[derive(Clone, PartialEq, Message)]
pub(crate) struct NotifyEnterWorld {
    #[prost(message, optional, tag = "1")]
    pub request: Option<NotifyEnterWorldRequest>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct NotifyEnterWorldRequest {
    // Tags 1 and 2 contain account/login data and are intentionally undeclared.
    #[prost(string, optional, tag = "3")]
    pub scene_host: Option<String>,
    #[prost(int32, optional, tag = "4")]
    pub scene_port: Option<i32>,
    // Transform and scene-line fields are outside this decoder's narrow purpose.
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct SyncServerTime {
    #[prost(int64, optional, tag = "1")]
    pub client_milliseconds: Option<i64>,
    #[prost(int64, optional, tag = "2")]
    pub server_milliseconds: Option<i64>,
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct SyncSeason {
    #[prost(int32, optional, tag = "1")]
    pub season_id: Option<i32>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct NotifySocialData {
    #[prost(message, optional, tag = "1")]
    pub request: Option<NotifySocialDataRequest>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct NotifySocialDataRequest {
    #[prost(message, optional, tag = "1")]
    pub data: Option<SocialData>,
}

/// Privacy-reviewed public subset of the owner/social character snapshot.
///
/// Account ID (tag 2), account data (tag 14), user-supplied image data, and
/// all unrelated social/account subtrees are deliberately undeclared.
#[derive(Clone, PartialEq, Message)]
pub(crate) struct SocialData {
    #[prost(int64, optional, tag = "1")]
    pub character_id: Option<i64>,
    #[prost(message, optional, tag = "3")]
    pub basic: Option<SocialBasicData>,
    #[prost(message, optional, tag = "4")]
    pub avatar: Option<SocialAvatarInfo>,
    #[prost(message, optional, tag = "6")]
    pub profession: Option<SocialProfessionData>,
    #[prost(message, optional, tag = "7")]
    pub equipment: Option<SocialEquipmentData>,
    #[prost(message, optional, tag = "10")]
    pub scene: Option<SceneData>,
    #[prost(message, optional, tag = "11")]
    pub user_attributes: Option<SocialUserAttributes>,
    #[prost(message, optional, tag = "13")]
    pub guild: Option<SocialGuildData>,
    #[prost(message, optional, tag = "16")]
    pub personal_zone: Option<SocialPersonalZone>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct SocialBasicData {
    #[prost(int64, optional, tag = "1")]
    pub character_id: Option<i64>,
    #[prost(int64, optional, tag = "2")]
    pub display_id: Option<i64>,
    #[prost(string, optional, tag = "3")]
    pub display_name: Option<String>,
    #[prost(int32, optional, tag = "4")]
    pub gender_id: Option<i32>,
    #[prost(int32, optional, tag = "5")]
    pub body_size_id: Option<i32>,
    #[prost(int32, optional, tag = "6")]
    pub level: Option<i32>,
    #[prost(uint32, optional, tag = "7")]
    pub scene_id: Option<u32>,
    #[prost(string, optional, tag = "10")]
    pub scene_instance_id: Option<String>,
    #[prost(int32, optional, tag = "19")]
    pub season_level: Option<i32>,
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct SocialAvatarInfo {
    #[prost(int32, optional, tag = "1")]
    pub avatar_id: Option<i32>,
    // Profile and half-body image messages (tags 2 and 3) are deliberately
    // undeclared because they can contain user-supplied URLs.
    #[prost(int32, optional, tag = "4")]
    pub business_card_style_id: Option<i32>,
    #[prost(int32, optional, tag = "5")]
    pub avatar_frame_id: Option<i32>,
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct SocialProfessionData {
    #[prost(int32, optional, tag = "1")]
    pub profession_id: Option<i32>,
    #[prost(int32, optional, tag = "2")]
    pub weapon_skin_id: Option<i32>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct SocialEquipmentData {
    #[prost(message, repeated, tag = "1")]
    pub items: Vec<SocialEquipmentItem>,
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct SocialEquipmentItem {
    #[prost(int32, optional, tag = "1")]
    pub slot_id: Option<i32>,
    #[prost(int32, optional, tag = "2")]
    pub item_id: Option<i32>,
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct SocialUserAttributes {
    #[prost(int64, optional, tag = "4")]
    pub combat_power: Option<i64>,
    #[prost(int32, optional, tag = "5")]
    pub season_strength: Option<i32>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct SocialGuildData {
    #[prost(int64, optional, tag = "1")]
    pub guild_id: Option<i64>,
    #[prost(string, optional, tag = "2")]
    pub guild_name: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct SocialPersonalZone {
    #[prost(map = "int32, int32", tag = "5")]
    pub medals: std::collections::HashMap<i32, i32>,
    #[prost(int32, optional, tag = "7")]
    pub business_card_style_id: Option<i32>,
    #[prost(int32, optional, tag = "8")]
    pub avatar_frame_id: Option<i32>,
    #[prost(int32, optional, tag = "11")]
    pub title_id: Option<i32>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct SyncDungeonData {
    #[prost(message, optional, tag = "1")]
    pub dungeon: Option<DungeonSyncData>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct SyncDungeonDirtyData {
    #[prost(message, optional, tag = "1")]
    pub data: Option<BufferStream>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct DungeonSyncData {
    #[prost(int64, optional, tag = "1")]
    pub scene_uuid: Option<i64>,
    #[prost(message, optional, tag = "2")]
    pub flow_info: Option<DungeonFlowInfo>,
    #[prost(message, optional, tag = "4")]
    pub target: Option<DungeonTarget>,
    // Settlement, player-list, social, and reward fields are intentionally
    // undeclared. They are not needed to establish a public run timeline.
    #[prost(message, optional, tag = "21")]
    pub scene_info: Option<DungeonSceneInfo>,
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct DungeonFlowInfo {
    #[prost(int32, optional, tag = "1")]
    pub state: Option<i32>,
    #[prost(int32, optional, tag = "2")]
    pub active_time: Option<i32>,
    #[prost(int32, optional, tag = "3")]
    pub ready_time: Option<i32>,
    #[prost(int32, optional, tag = "4")]
    pub play_time: Option<i32>,
    #[prost(int32, optional, tag = "5")]
    pub end_time: Option<i32>,
    #[prost(int32, optional, tag = "6")]
    pub settlement_time: Option<i32>,
    #[prost(int32, optional, tag = "7")]
    pub dungeon_times: Option<i32>,
    #[prost(int32, optional, tag = "8")]
    pub result: Option<i32>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct DungeonTarget {
    #[prost(map = "int32, message", tag = "1")]
    pub target_data: std::collections::HashMap<i32, DungeonTargetData>,
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct DungeonTargetData {
    #[prost(int32, optional, tag = "1")]
    pub target_id: Option<i32>,
    #[prost(int32, optional, tag = "2")]
    pub value: Option<i32>,
    #[prost(int32, optional, tag = "3")]
    pub complete: Option<i32>,
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct DungeonSceneInfo {
    #[prost(int32, optional, tag = "1")]
    pub difficulty: Option<i32>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct EnterScene {
    #[prost(message, optional, tag = "1")]
    pub enter_scene_info: Option<EnterSceneInfo>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct EnterSceneInfo {
    #[prost(message, optional, tag = "1")]
    pub scene_attrs: Option<AttrCollection>,
    #[prost(message, optional, tag = "2")]
    pub player_entity: Option<Entity>,
    #[prost(string, optional, tag = "3")]
    pub scene_instance_id: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct NotifyLoadSceneEnd {
    #[prost(message, optional, tag = "1")]
    pub response: Option<NotifyLoadSceneEndResponse>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct NotifyLoadSceneEndResponse {
    #[prost(int32, optional, tag = "1")]
    pub scene_id: Option<i32>,
    #[prost(string, optional, tag = "2")]
    pub scene_instance_id: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct Entity {
    #[prost(int64, optional, tag = "1")]
    pub uuid: Option<i64>,
    #[prost(int32, optional, tag = "2")]
    pub entity_type: Option<i32>,
    #[prost(message, optional, tag = "3")]
    pub attributes: Option<AttrCollection>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct AttrCollection {
    #[prost(int64, optional, tag = "1")]
    pub uuid: Option<i64>,
    #[prost(message, repeated, tag = "2")]
    pub attributes: Vec<Attr>,
    #[prost(message, repeated, tag = "3")]
    pub map_attributes: Vec<MapAttr>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct Attr {
    #[prost(int32, optional, tag = "1")]
    pub id: Option<i32>,
    #[prost(bytes = "vec", optional, tag = "2")]
    pub raw_data: Option<Vec<u8>>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct MapAttr {
    #[prost(bool, optional, tag = "1")]
    pub is_clear: Option<bool>,
    #[prost(int32, optional, tag = "2")]
    pub id: Option<i32>,
    #[prost(message, repeated, tag = "3")]
    pub values: Vec<MapAttrValue>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct MapAttrValue {
    #[prost(bool, optional, tag = "1")]
    pub is_remove: Option<bool>,
    #[prost(bytes = "vec", optional, tag = "2")]
    pub key: Option<Vec<u8>>,
    #[prost(bytes = "vec", optional, tag = "3")]
    pub value: Option<Vec<u8>>,
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct Position {
    #[prost(float, optional, tag = "1")]
    pub x: Option<f32>,
    #[prost(float, optional, tag = "2")]
    pub y: Option<f32>,
    #[prost(float, optional, tag = "3")]
    pub z: Option<f32>,
    #[prost(float, optional, tag = "4")]
    pub facing_radians: Option<f32>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct SyncNearEntities {
    #[prost(message, repeated, tag = "1")]
    pub appeared: Vec<Entity>,
    #[prost(message, repeated, tag = "2")]
    pub disappeared: Vec<DisappearEntity>,
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct DisappearEntity {
    #[prost(int64, optional, tag = "1")]
    pub uuid: Option<i64>,
    #[prost(int32, optional, tag = "2")]
    pub reason: Option<i32>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct SyncContainerData {
    #[prost(message, optional, tag = "1")]
    pub character: Option<CharacterSerialize>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct SyncContainerDirtyData {
    #[prost(message, optional, tag = "1")]
    pub data: Option<BufferStream>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct BufferStream {
    #[prost(bytes = "vec", optional, tag = "1")]
    pub buffer: Option<Vec<u8>>,
    #[prost(int32, optional, tag = "2")]
    pub stream_type: Option<i32>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct CharacterSerialize {
    #[prost(int64, optional, tag = "1")]
    pub character_id: Option<i64>,
    #[prost(message, optional, tag = "2")]
    pub base: Option<CharacterBase>,
    #[prost(message, optional, tag = "3")]
    pub scene: Option<SceneData>,
    #[prost(message, optional, tag = "7")]
    pub item_package: Option<ItemPackage>,
    #[prost(message, optional, tag = "12")]
    pub equipment: Option<EquipmentList>,
    #[prost(message, optional, tag = "17")]
    pub fashion: Option<FashionManager>,
    #[prost(message, optional, tag = "18")]
    pub profile_list: Option<ProfileList>,
    #[prost(message, optional, tag = "22")]
    pub role_level: Option<RoleLevel>,
    #[prost(message, optional, tag = "34")]
    pub role_face: Option<RoleFace>,
    #[prost(message, optional, tag = "42")]
    pub collection_book: Option<CollectionBook>,
    #[prost(message, optional, tag = "46")]
    pub challenge_dungeons: Option<ChallengeDungeonInfo>,
    #[prost(message, optional, tag = "50")]
    pub season_center: Option<SeasonCenter>,
    #[prost(message, optional, tag = "51")]
    pub personal_zone: Option<PersonalZone>,
    #[prost(message, optional, tag = "52")]
    pub season_medals: Option<SeasonMedalInfo>,
    #[prost(message, optional, tag = "55")]
    pub slots: Option<SlotList>,
    #[prost(message, optional, tag = "57")]
    pub modules: Option<ModuleData>,
    #[prost(message, optional, tag = "61")]
    pub professions: Option<ProfessionList>,
    #[prost(message, optional, tag = "67")]
    pub weekly_tower: Option<WeeklyTowerRecord>,
    #[prost(message, optional, tag = "70")]
    pub rides: Option<RideList>,
    #[prost(message, optional, tag = "72")]
    pub life_professions: Option<LifeProfessionList>,
    #[prost(message, optional, tag = "82")]
    pub unlocked_emojis: Option<UnlockEmojiData>,
    #[prost(message, optional, tag = "89")]
    pub handbook: Option<HandbookData>,
    #[prost(message, optional, tag = "90")]
    pub master_mode_dungeons: Option<MasterModeDungeonInfo>,
    #[prost(message, optional, tag = "96")]
    pub fight_power: Option<FightPower>,
    #[prost(message, optional, tag = "101")]
    pub season_cultivation: Option<SeasonCultivateLineData>,
    #[prost(message, optional, tag = "102")]
    pub season_role_levels: Option<SeasonRoleLevelData>,
    #[prost(message, optional, tag = "103")]
    pub reputations: Option<ReputationList>,
    #[prost(message, optional, tag = "106")]
    pub current_profession_project: Option<CurrentProfessionProject>,
    #[prost(message, optional, tag = "120")]
    pub vanity_pets: Option<VanityPetManager>,
    #[prost(message, optional, tag = "121")]
    pub fantasy_atlas: Option<FantasyAtlasData>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct CharacterBase {
    #[prost(int64, optional, tag = "1")]
    pub character_id: Option<i64>,
    // Tag 2 is account_id and is intentionally not declared.
    #[prost(int64, optional, tag = "3")]
    pub display_id: Option<i64>,
    #[prost(uint32, optional, tag = "4")]
    pub server_id: Option<u32>,
    #[prost(string, optional, tag = "5")]
    pub display_name: Option<String>,
    #[prost(int32, optional, tag = "6")]
    pub gender_id: Option<i32>,
    #[prost(message, optional, tag = "14")]
    pub face: Option<FaceData>,
    #[prost(int32, optional, tag = "22")]
    pub body_size_id: Option<i32>,
    #[prost(message, optional, tag = "25")]
    pub avatar: Option<CharacterAvatarInfo>,
    #[prost(int32, optional, tag = "31")]
    pub initial_class_id: Option<i32>,
    #[prost(int32, optional, tag = "35")]
    pub combat_power: Option<i32>,
    // Tags containing account/open/login state are intentionally not declared.
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct FaceData {
    #[prost(map = "int32, int32", tag = "1")]
    pub options: std::collections::HashMap<i32, i32>,
    #[prost(map = "int32, message", tag = "2")]
    pub colors: std::collections::HashMap<i32, IntVec3>,
    #[prost(float, optional, tag = "3")]
    pub height: Option<f32>,
    #[prost(int32, optional, tag = "4")]
    pub body_size_id: Option<i32>,
    #[prost(int32, optional, tag = "5")]
    pub voice_id: Option<i32>,
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct IntVec3 {
    #[prost(int32, optional, tag = "1")]
    pub x: Option<i32>,
    #[prost(int32, optional, tag = "2")]
    pub y: Option<i32>,
    #[prost(int32, optional, tag = "3")]
    pub z: Option<i32>,
}

/// Character-facing avatar IDs only.
///
/// Profile and half-body picture messages are deliberately undeclared because
/// they contain user-supplied URLs and verification metadata.
#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct CharacterAvatarInfo {
    #[prost(int32, optional, tag = "1")]
    pub avatar_id: Option<i32>,
    #[prost(int32, optional, tag = "4")]
    pub business_card_style_id: Option<i32>,
    #[prost(int32, optional, tag = "5")]
    pub avatar_frame_id: Option<i32>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct SceneData {
    #[prost(uint32, optional, tag = "1")]
    pub map_id: Option<u32>,
    #[prost(uint32, optional, tag = "2")]
    pub channel_id: Option<u32>,
    #[prost(message, optional, tag = "3")]
    pub position: Option<Position>,
    #[prost(uint32, optional, tag = "6")]
    pub level_map_id: Option<u32>,
    #[prost(string, optional, tag = "13")]
    pub scene_instance_id: Option<String>,
    #[prost(string, optional, tag = "14")]
    pub dungeon_instance_id: Option<String>,
    #[prost(uint32, optional, tag = "15")]
    pub line_id: Option<u32>,
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct RoleLevel {
    #[prost(int32, optional, tag = "1")]
    pub level: Option<i32>,
    #[prost(int64, optional, tag = "2")]
    pub current_experience: Option<i64>,
    #[prost(int32, optional, tag = "11")]
    pub previous_season_max_level: Option<i32>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct ItemPackage {
    #[prost(map = "int32, message", tag = "1")]
    pub packages: std::collections::HashMap<i32, ItemPackageSection>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct ItemPackageSection {
    #[prost(map = "int64, message", tag = "4")]
    pub items: std::collections::HashMap<i64, ItemRecord>,
}

/// Privacy-reviewed item fields used to enrich equipped character gear and
/// module optimizer inputs.
///
/// Acquisition timestamps, expiration, binding, source, currencies, and
/// unrelated inventory state are deliberately undeclared.
#[derive(Clone, PartialEq, Message)]
pub(crate) struct ItemRecord {
    #[prost(int64, optional, tag = "1")]
    pub uuid: Option<i64>,
    #[prost(int32, optional, tag = "2")]
    pub item_id: Option<i32>,
    #[prost(int64, optional, tag = "3")]
    pub count: Option<i64>,
    #[prost(int32, optional, tag = "9")]
    pub quality: Option<i32>,
    #[prost(message, optional, tag = "10")]
    pub equipment_attributes: Option<EquipmentAttributes>,
    #[prost(message, optional, tag = "11")]
    pub module_attributes: Option<ModuleAttributes>,
    #[prost(message, optional, tag = "13")]
    pub module_parts: Option<ModuleParts>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct ModuleAttributes {
    #[prost(int32, optional, tag = "1")]
    pub load_flag: Option<i32>,
    #[prost(int32, optional, tag = "2")]
    pub module_type: Option<i32>,
    #[prost(int32, optional, tag = "3")]
    pub level: Option<i32>,
    // Attribute effect parameters are intentionally not declared. Website
    // optimization joins typed part IDs against exact-build static catalogs.
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct ModuleParts {
    #[prost(int32, repeated, tag = "1")]
    pub part_ids: Vec<i32>,
    #[prost(message, repeated, tag = "2")]
    pub upgrade_records: Vec<ModulePartUpgradeRecord>,
}

#[derive(Clone, Copy, PartialEq, Eq, Message)]
pub(crate) struct ModulePartUpgradeRecord {
    #[prost(int32, optional, tag = "1")]
    pub part_id: Option<i32>,
    #[prost(bool, optional, tag = "2")]
    pub succeeded: Option<bool>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct ModuleData {
    #[prost(map = "int32, int64", tag = "1")]
    pub equipped_slots: std::collections::HashMap<i32, i64>,
    #[prost(map = "int64, message", tag = "2")]
    pub module_infos: std::collections::HashMap<i64, ModuleInfo>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct ModuleInfo {
    #[prost(int32, repeated, tag = "1")]
    pub part_ids: Vec<i32>,
    #[prost(message, repeated, tag = "2")]
    pub upgrade_records: Vec<ModulePartUpgradeRecord>,
    #[prost(int32, optional, tag = "3")]
    pub success_rate: Option<i32>,
    #[prost(int32, repeated, tag = "4")]
    pub initial_link_points: Vec<i32>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct EquipmentList {
    #[prost(map = "int32, message", tag = "1")]
    pub equipped: std::collections::HashMap<i32, EquippedItem>,
    #[prost(map = "int64, message", tag = "5")]
    pub enchantments: std::collections::HashMap<i64, EquipmentEnchantment>,
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct EquippedItem {
    #[prost(int32, optional, tag = "1")]
    pub slot_id: Option<i32>,
    #[prost(uint64, optional, tag = "2")]
    pub item_uuid: Option<u64>,
    #[prost(uint32, optional, tag = "3")]
    pub refinement_level: Option<u32>,
    #[prost(uint32, optional, tag = "4")]
    pub refinement_failed_count: Option<u32>,
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct EquipmentEnchantment {
    #[prost(int32, optional, tag = "1")]
    pub enchantment_id: Option<i32>,
    #[prost(int32, optional, tag = "2")]
    pub level: Option<i32>,
    #[prost(int32, optional, tag = "3")]
    pub enchantment_type: Option<i32>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct EquipmentAttributes {
    #[prost(map = "uint32, uint32", tag = "4")]
    pub base: std::collections::HashMap<u32, u32>,
    #[prost(int32, optional, tag = "7")]
    pub perfection_value: Option<i32>,
    #[prost(int32, optional, tag = "8")]
    pub recast_count: Option<i32>,
    #[prost(int32, optional, tag = "9")]
    pub total_recast_count: Option<i32>,
    #[prost(map = "int32, int32", tag = "10")]
    pub basic: std::collections::HashMap<i32, i32>,
    #[prost(map = "int32, int32", tag = "11")]
    pub advanced: std::collections::HashMap<i32, i32>,
    #[prost(map = "int32, int32", tag = "12")]
    pub recast: std::collections::HashMap<i32, i32>,
    #[prost(int32, optional, tag = "13")]
    pub perfection_level: Option<i32>,
    #[prost(map = "int32, int32", tag = "14")]
    pub rare_quality: std::collections::HashMap<i32, i32>,
    #[prost(int32, optional, tag = "15")]
    pub max_perfection_value: Option<i32>,
    #[prost(int32, optional, tag = "18")]
    pub breakthrough_level: Option<i32>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct ProfessionList {
    #[prost(int32, optional, tag = "1")]
    pub current_profession_id: Option<i32>,
    #[prost(map = "int32, message", tag = "4")]
    pub professions: std::collections::HashMap<i32, ProfessionInfo>,
    #[prost(map = "int32, message", tag = "7")]
    pub battle_imagine_skills: std::collections::HashMap<i32, ProfessionSkillInfo>,
    #[prost(uint32, optional, tag = "8")]
    pub total_talent_points: Option<u32>,
    #[prost(uint32, optional, tag = "9")]
    pub total_talent_reset_count: Option<u32>,
    #[prost(map = "int32, message", tag = "10")]
    pub talents: std::collections::HashMap<i32, ProfessionTalentInfo>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct ProfessionInfo {
    #[prost(int32, optional, tag = "1")]
    pub profession_id: Option<i32>,
    #[prost(int32, optional, tag = "2")]
    pub level: Option<i32>,
    #[prost(int64, optional, tag = "3")]
    pub experience: Option<i64>,
    #[prost(map = "int32, message", tag = "4")]
    pub skills: std::collections::HashMap<i32, ProfessionSkillInfo>,
    #[prost(int32, repeated, tag = "6")]
    pub active_skill_ids: Vec<i32>,
    #[prost(map = "int32, int32", tag = "7")]
    pub slotted_skill_ids: std::collections::HashMap<i32, i32>,
    #[prost(int32, optional, tag = "8")]
    pub weapon_skin_id: Option<i32>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct ProfessionSkillInfo {
    #[prost(int32, optional, tag = "1")]
    pub skill_id: Option<i32>,
    #[prost(int32, optional, tag = "2")]
    pub level: Option<i32>,
    #[prost(int32, repeated, tag = "3")]
    pub replacement_skill_ids: Vec<i32>,
    #[prost(int32, optional, tag = "4")]
    pub remodel_level: Option<i32>,
    #[prost(int32, optional, tag = "5")]
    pub current_skin_id: Option<i32>,
    #[prost(map = "int32, bool", tag = "6")]
    pub unlocked_skin_ids: std::collections::HashMap<i32, bool>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct ProfessionTalentInfo {
    #[prost(uint32, optional, tag = "1")]
    pub used_talent_points: Option<u32>,
    #[prost(uint32, repeated, tag = "2")]
    pub talent_node_ids: Vec<u32>,
    #[prost(int32, optional, tag = "3")]
    pub talent_stage_config_id: Option<i32>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct LifeProfessionList {
    #[prost(map = "int32, message", tag = "1")]
    pub professions: std::collections::HashMap<i32, LifeProfessionInfo>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct LifeProfessionInfo {
    #[prost(int32, optional, tag = "1")]
    pub profession_id: Option<i32>,
    #[prost(int32, optional, tag = "2")]
    pub level: Option<i32>,
    #[prost(int32, optional, tag = "3")]
    pub experience: Option<i32>,
    #[prost(map = "int32, message", tag = "5")]
    pub specializations: std::collections::HashMap<i32, LifeProfessionSpecialization>,
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct LifeProfessionSpecialization {
    #[prost(int32, optional, tag = "1")]
    pub specialization_id: Option<i32>,
    #[prost(int32, optional, tag = "2")]
    pub level: Option<i32>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct FashionManager {
    #[prost(map = "int32, int32", tag = "1")]
    pub equipped_fashion: std::collections::HashMap<i32, i32>,
    #[prost(map = "int32, bool", tag = "5")]
    pub owned_fashion: std::collections::HashMap<i32, bool>,
    #[prost(map = "int32, bool", tag = "6")]
    pub owned_mounts: std::collections::HashMap<i32, bool>,
    #[prost(map = "int32, bool", tag = "7")]
    pub owned_weapon_skins: std::collections::HashMap<i32, bool>,
    #[prost(int32, optional, tag = "9")]
    pub fashion_points: Option<i32>,
    #[prost(int32, optional, tag = "10")]
    pub mount_points: Option<i32>,
    #[prost(int32, optional, tag = "11")]
    pub weapon_skin_points: Option<i32>,
    #[prost(map = "int32, bool", tag = "19")]
    pub owned_dyes: std::collections::HashMap<i32, bool>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct ProfileList {
    #[prost(map = "int32, bool", tag = "1")]
    pub unlocked_profile_ids: std::collections::HashMap<i32, bool>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct RoleFace {
    #[prost(map = "int32, bool", tag = "1")]
    pub unlocked_item_ids: std::collections::HashMap<i32, bool>,
    #[prost(int32, repeated, tag = "3")]
    pub unlocked_voice_ids: Vec<i32>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct CollectionBook {
    #[prost(map = "int32, bool", tag = "1")]
    pub unlocked_module_ids: std::collections::HashMap<i32, bool>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct ChallengeDungeonInfo {
    #[prost(map = "int32, message", tag = "1")]
    pub dungeons: std::collections::HashMap<i32, DungeonProgress>,
    #[prost(map = "int32, message", tag = "2")]
    pub target_awards: std::collections::HashMap<i32, DungeonTargetAwards>,
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct DungeonProgress {
    #[prost(int32, optional, tag = "1")]
    pub dungeon_id: Option<i32>,
    #[prost(int32, optional, tag = "2")]
    pub completion_count: Option<i32>,
    #[prost(int32, optional, tag = "3")]
    pub award_state: Option<i32>,
    #[prost(int32, optional, tag = "4")]
    pub score: Option<i32>,
    #[prost(int32, optional, tag = "5")]
    pub pass_time: Option<i32>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct DungeonTargetAwards {
    #[prost(map = "int32, message", tag = "1")]
    pub targets: std::collections::HashMap<i32, DungeonTargetProgress>,
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct DungeonTargetProgress {
    #[prost(int32, optional, tag = "1")]
    pub target_id: Option<i32>,
    #[prost(int32, optional, tag = "2")]
    pub progress: Option<i32>,
    #[prost(int32, optional, tag = "3")]
    pub award_state: Option<i32>,
}

/// Privacy-reviewed public display fields from the character's personal zone.
///
/// Online periods, editor state, actions, photos, photo-wall data, and other
/// user-generated content are deliberately undeclared.
#[derive(Clone, PartialEq, Message)]
pub(crate) struct PersonalZone {
    #[prost(map = "int32, int32", tag = "5")]
    pub medals: std::collections::HashMap<i32, i32>,
    #[prost(int32, optional, tag = "6")]
    pub theme_id: Option<i32>,
    #[prost(int32, optional, tag = "7")]
    pub business_card_style_id: Option<i32>,
    #[prost(int32, optional, tag = "8")]
    pub avatar_frame_id: Option<i32>,
    #[prost(int32, optional, tag = "11")]
    pub title_id: Option<i32>,
    #[prost(int32, optional, tag = "13")]
    pub fashion_collection_points: Option<i32>,
    #[prost(int32, optional, tag = "18")]
    pub ride_collection_points: Option<i32>,
    #[prost(int32, optional, tag = "20")]
    pub weapon_skin_collection_points: Option<i32>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct SeasonMedalInfo {
    #[prost(uint32, optional, tag = "1")]
    pub season_id: Option<u32>,
    #[prost(map = "uint32, message", tag = "2")]
    pub normal_holes: std::collections::HashMap<u32, MedalHole>,
    #[prost(message, optional, tag = "3")]
    pub core_hole: Option<MedalHole>,
    #[prost(map = "uint32, message", tag = "4")]
    pub core_nodes: std::collections::HashMap<u32, MedalNode>,
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct MedalHole {
    #[prost(uint32, optional, tag = "1")]
    pub hole_id: Option<u32>,
    #[prost(uint32, optional, tag = "2")]
    pub level: Option<u32>,
    #[prost(uint32, optional, tag = "3")]
    pub current_experience: Option<u32>,
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct MedalNode {
    #[prost(uint32, optional, tag = "1")]
    pub node_id: Option<u32>,
    #[prost(uint32, optional, tag = "2")]
    pub level: Option<u32>,
    #[prost(bool, optional, tag = "3")]
    pub selected: Option<bool>,
    #[prost(int32, optional, tag = "4")]
    pub slot_id: Option<i32>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct SlotList {
    #[prost(map = "int32, message", tag = "1")]
    pub slots: std::collections::HashMap<i32, SlotInfo>,
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct SlotInfo {
    #[prost(int32, optional, tag = "1")]
    pub slot_id: Option<i32>,
    #[prost(int32, optional, tag = "2")]
    pub skill_id: Option<i32>,
    #[prost(bool, optional, tag = "3")]
    pub auto_battle_disabled: Option<bool>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct WeeklyTowerRecord {
    #[prost(int64, optional, tag = "1")]
    pub begin_time: Option<i64>,
    #[prost(int32, optional, tag = "2")]
    pub maximum_floor_id: Option<i32>,
    #[prost(int32, repeated, tag = "3")]
    pub claimed_floor_ids: Vec<i32>,
    #[prost(int32, optional, tag = "4")]
    pub rule_id: Option<i32>,
    #[prost(int32, optional, tag = "5")]
    pub maximum_jump_reward_floor_id: Option<i32>,
    #[prost(int32, optional, tag = "6")]
    pub previous_maximum_floor_id: Option<i32>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct RideList {
    #[prost(map = "int32, message", tag = "1")]
    pub rides: std::collections::HashMap<i32, RideData>,
    #[prost(int32, optional, tag = "2")]
    pub property_type: Option<i32>,
    #[prost(map = "int32, message", tag = "3")]
    pub skins: std::collections::HashMap<i32, RideSkinContainer>,
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct RideData {
    #[prost(int32, optional, tag = "1")]
    pub ride_id: Option<i32>,
}

/// Ride skin identity only. Activation timestamps are deliberately undeclared.
#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct RideSkinContainer {
    #[prost(int32, optional, tag = "1")]
    pub skin_id: Option<i32>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct UnlockEmojiData {
    #[prost(map = "int32, bool", tag = "1")]
    pub unlocked_ids: std::collections::HashMap<i32, bool>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct HandbookData {
    #[prost(map = "int32, message", tag = "1")]
    pub important_people: std::collections::HashMap<i32, HandbookEntry>,
    #[prost(map = "int32, message", tag = "2")]
    pub reading_books: std::collections::HashMap<i32, HandbookEntry>,
    #[prost(map = "int32, message", tag = "3")]
    pub dictionary: std::collections::HashMap<i32, HandbookEntry>,
    #[prost(map = "int32, message", tag = "4")]
    pub postcards: std::collections::HashMap<i32, HandbookEntry>,
    #[prost(map = "int32, message", tag = "5")]
    pub monthly_cards: std::collections::HashMap<i32, HandbookEntry>,
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct HandbookEntry {
    #[prost(int32, optional, tag = "1")]
    pub entry_id: Option<i32>,
    #[prost(bool, optional, tag = "2")]
    pub unlocked: Option<bool>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct MasterModeDungeonInfo {
    #[prost(map = "int32, message", tag = "1")]
    pub seasons: std::collections::HashMap<i32, MasterModeSeason>,
    #[prost(bool, optional, tag = "2")]
    pub visible: Option<bool>,
    #[prost(int32, optional, tag = "3")]
    pub current_display_season_id: Option<i32>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct MasterModeSeason {
    #[prost(map = "int32, message", tag = "1")]
    pub difficulties: std::collections::HashMap<i32, MasterModeDifficulty>,
    // Update timestamps and reward-claim state are intentionally undeclared.
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct MasterModeDifficulty {
    #[prost(map = "int32, message", tag = "1")]
    pub dungeons: std::collections::HashMap<i32, DungeonProgress>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct SeasonCultivateLineData {
    #[prost(map = "int32, message", tag = "1")]
    pub seasons: std::collections::HashMap<i32, CultivateLine>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct CultivateLine {
    #[prost(map = "int32, message", tag = "1")]
    pub lines: std::collections::HashMap<i32, CultivateLineSubtype>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct CultivateLineSubtype {
    #[prost(map = "int32, message", tag = "1")]
    pub areas: std::collections::HashMap<i32, CultivateArea>,
    #[prost(int32, repeated, tag = "2")]
    pub area_ids: Vec<i32>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct CultivateArea {
    #[prost(map = "int32, message", tag = "1")]
    pub normal_nodes: std::collections::HashMap<i32, CultivateNormalNode>,
    #[prost(map = "int32, message", tag = "2")]
    pub middle_nodes: std::collections::HashMap<i32, CultivateMiddleNode>,
    #[prost(map = "int32, message", tag = "3")]
    pub big_nodes: std::collections::HashMap<i32, CultivateBigNode>,
    #[prost(int32, optional, tag = "4")]
    pub active_effect_score: Option<i32>,
    #[prost(bool, optional, tag = "5")]
    pub active: Option<bool>,
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct CultivateNormalNode {
    #[prost(int32, optional, tag = "1")]
    pub active_level: Option<i32>,
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct CultivateMiddleNode {
    #[prost(int32, optional, tag = "1")]
    pub item_id: Option<i32>,
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct CultivateBigNode {
    #[prost(int32, optional, tag = "1")]
    pub fantasy_id: Option<i32>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct ReputationList {
    #[prost(map = "uint32, message", tag = "1")]
    pub reputations: std::collections::HashMap<u32, ReputationInfo>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct ReputationInfo {
    #[prost(uint64, optional, tag = "1")]
    pub experience: Option<u64>,
    #[prost(int32, optional, tag = "2")]
    pub level: Option<i32>,
    #[prost(map = "int32, bool", tag = "3")]
    pub claimed_awards: std::collections::HashMap<i32, bool>,
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct CurrentProfessionProject {
    #[prost(int32, optional, tag = "1")]
    pub project_id: Option<i32>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct VanityPetManager {
    #[prost(map = "int64, message", tag = "1")]
    pub pets: std::collections::HashMap<i64, VanityPet>,
    #[prost(map = "int32, bool", tag = "2")]
    pub unlocked_pet_type_ids: std::collections::HashMap<i32, bool>,
    #[prost(message, optional, tag = "3")]
    pub summon: Option<VanityPetSummon>,
    // Fault-tolerance item instance UUIDs are deliberately undeclared.
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct VanityPet {
    #[prost(int64, optional, tag = "1")]
    pub instance_id: Option<i64>,
    #[prost(int32, optional, tag = "2")]
    pub pet_id: Option<i32>,
    // Energy and item instance UUID are deliberately undeclared.
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct VanityPetSummon {
    #[prost(int64, optional, tag = "1")]
    pub summoned_instance_id: Option<i64>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct FantasyAtlasData {
    #[prost(map = "int32, message", tag = "1")]
    pub entries: std::collections::HashMap<i32, FantasyAtlasEntry>,
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct FantasyAtlasEntry {
    #[prost(int32, optional, tag = "1")]
    pub activated_stage: Option<i32>,
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct SeasonCenter {
    #[prost(int32, optional, tag = "1")]
    pub season_id: Option<i32>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct SeasonRoleLevelData {
    #[prost(map = "int32, message", tag = "1")]
    pub levels: std::collections::HashMap<i32, SeasonRoleLevel>,
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct SeasonRoleLevel {
    #[prost(int32, optional, tag = "1")]
    pub level: Option<i32>,
    #[prost(int64, optional, tag = "2")]
    pub current_experience: Option<i64>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct FightPower {
    #[prost(int32, optional, tag = "1")]
    pub total: Option<i32>,
    #[prost(map = "int32, message", tag = "2")]
    pub components: std::collections::HashMap<i32, FightPowerComponent>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct FightPowerComponent {
    #[prost(int32, optional, tag = "1")]
    pub function_type_id: Option<i32>,
    #[prost(int32, optional, tag = "2")]
    pub total_points: Option<i32>,
    #[prost(int32, optional, tag = "3")]
    pub points: Option<i32>,
    #[prost(map = "int32, message", tag = "4")]
    pub subcomponents: std::collections::HashMap<i32, FightPowerSubcomponent>,
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct FightPowerSubcomponent {
    #[prost(int32, optional, tag = "1")]
    pub function_type_id: Option<i32>,
    #[prost(int32, optional, tag = "2")]
    pub root_function_type_id: Option<i32>,
    #[prost(int32, optional, tag = "3")]
    pub points: Option<i32>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct SyncNearDeltaInfo {
    #[prost(message, repeated, tag = "1")]
    pub deltas: Vec<AoiSyncDelta>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct SyncToMeDeltaInfo {
    #[prost(message, optional, tag = "1")]
    pub delta: Option<AoiSyncToMeDelta>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct AoiSyncToMeDelta {
    #[prost(message, optional, tag = "1")]
    pub base_delta: Option<AoiSyncDelta>,
    #[prost(message, repeated, tag = "3")]
    pub cooldowns: Vec<SkillCooldown>,
    #[prost(int64, optional, tag = "5")]
    pub uuid: Option<i64>,
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct SkillCooldown {
    #[prost(int32, optional, tag = "1")]
    pub skill_level_id: Option<i32>,
    #[prost(int64, optional, tag = "2")]
    pub begin_time: Option<i64>,
    #[prost(int32, optional, tag = "3")]
    pub duration: Option<i32>,
    #[prost(int32, optional, tag = "4")]
    pub cooldown_type: Option<i32>,
    #[prost(int32, optional, tag = "5")]
    pub valid_duration: Option<i32>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct AoiSyncDelta {
    #[prost(int64, optional, tag = "1")]
    pub uuid: Option<i64>,
    #[prost(message, optional, tag = "2")]
    pub attributes: Option<AttrCollection>,
    #[prost(message, optional, tag = "7")]
    pub skill_effects: Option<SkillEffect>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct SkillEffect {
    #[prost(int64, optional, tag = "1")]
    pub uuid: Option<i64>,
    #[prost(message, repeated, tag = "2")]
    pub damage: Vec<DamageInfo>,
    #[prost(int64, optional, tag = "3")]
    pub total_damage: Option<i64>,
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct DamageInfo {
    #[prost(int32, optional, tag = "1")]
    pub damage_source: Option<i32>,
    #[prost(bool, optional, tag = "2")]
    pub missed: Option<bool>,
    #[prost(bool, optional, tag = "3")]
    pub critical: Option<bool>,
    #[prost(int32, optional, tag = "4")]
    pub damage_type: Option<i32>,
    #[prost(int32, optional, tag = "5")]
    pub type_flags: Option<i32>,
    #[prost(int64, optional, tag = "6")]
    pub value: Option<i64>,
    #[prost(int64, optional, tag = "7")]
    pub actual_value: Option<i64>,
    #[prost(int64, optional, tag = "8")]
    pub lucky_value: Option<i64>,
    #[prost(int64, optional, tag = "9")]
    pub hp_loss: Option<i64>,
    #[prost(int64, optional, tag = "10")]
    pub shield_loss: Option<i64>,
    #[prost(int64, optional, tag = "11")]
    pub attacker_uuid: Option<i64>,
    #[prost(int32, optional, tag = "12")]
    pub owner_id: Option<i32>,
    #[prost(int32, optional, tag = "15")]
    pub hit_event_id: Option<i32>,
    #[prost(bool, optional, tag = "17")]
    pub dead: Option<bool>,
    #[prost(int64, optional, tag = "21")]
    pub top_summoner_uuid: Option<i64>,
}

#[derive(Clone, Copy, PartialEq, Message)]
pub(crate) struct NotifyReviveUser {
    #[prost(int64, optional, tag = "1")]
    pub actor_uuid: Option<i64>,
}
