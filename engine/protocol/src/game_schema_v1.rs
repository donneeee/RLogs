//! Privacy-reviewed partial protobuf schemas for the first Global/CN lineage.
//!
//! These messages intentionally declare only gameplay fields used by RLogs.
//! Prost skips undeclared fields without materializing them. In particular,
//! the character-base schema does not declare account IDs, open IDs, login
//! state, credentials, tokens, or private account-security fields.

use prost::Message;

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
pub(crate) struct CharacterSerialize {
    #[prost(int64, optional, tag = "1")]
    pub character_id: Option<i64>,
    #[prost(message, optional, tag = "2")]
    pub base: Option<CharacterBase>,
    #[prost(message, optional, tag = "3")]
    pub scene: Option<SceneData>,
    #[prost(message, optional, tag = "22")]
    pub role_level: Option<RoleLevel>,
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
    #[prost(int32, optional, tag = "31")]
    pub initial_class_id: Option<i32>,
    #[prost(int32, optional, tag = "35")]
    pub combat_power: Option<i32>,
    // Tags containing account/open/login state are intentionally not declared.
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct SceneData {
    #[prost(uint32, optional, tag = "1")]
    pub map_id: Option<u32>,
    #[prost(uint32, optional, tag = "2")]
    pub channel_id: Option<u32>,
    #[prost(message, optional, tag = "3")]
    pub position: Option<Position>,
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
