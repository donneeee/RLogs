use serde::{Deserialize, Serialize};

use crate::{CharacterIdentity, DungeonId, EntityRef, MapId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatChannel {
    World,
    Nearby,
    Party,
    Guild,
    System,
    Direct,
    Unknown(i32),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatEvent {
    pub channel: ChatChannel,
    pub sender: Option<EntityRef>,
    pub sender_character: Option<CharacterIdentity>,
    pub message_id: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MapEventKind {
    Entered,
    Exited,
    MarkerAdded,
    MarkerUpdated,
    MarkerRemoved,
    ObjectiveUpdated,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MapEvent {
    pub kind: MapEventKind,
    pub map_id: Option<MapId>,
    pub marker_id: Option<i64>,
    pub related_entity: Option<EntityRef>,
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub z: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DungeonEventKind {
    Entered,
    Started,
    ObjectiveUpdated,
    BossEngaged,
    BossDefeated,
    Completed,
    Failed,
    Exited,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DungeonEvent {
    pub kind: DungeonEventKind,
    pub dungeon_id: Option<DungeonId>,
    pub instance_id: Option<String>,
    pub difficulty_id: Option<i32>,
    pub objective_id: Option<i64>,
    pub objective_value: Option<i64>,
}
