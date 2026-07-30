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
    /// A lossless flow snapshot changed without establishing a higher-level
    /// run boundary.
    FlowUpdated,
    Ended,
    ObjectiveUpdated,
    ObjectiveRemoved,
    BossEngaged,
    BossDefeated,
    Completed,
    Failed,
    Exited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DungeonFlowPhase {
    Null,
    Active,
    Ready,
    Playing,
    End,
    Settlement,
    Vote,
    Unknown(i32),
}

impl DungeonFlowPhase {
    pub fn from_protocol_id(value: i32) -> Self {
        match value {
            0 => Self::Null,
            1 => Self::Active,
            2 => Self::Ready,
            3 => Self::Playing,
            4 => Self::End,
            5 => Self::Settlement,
            6 => Self::Vote,
            other => Self::Unknown(other),
        }
    }
}

/// Exact `DungeonFlowInfo` evidence from the game protocol.
///
/// The time-like fields intentionally retain their raw signed values. Their
/// unit and whether they are timestamps, deadlines, or durations must be
/// established from current-build captures before normalization.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DungeonFlowSnapshot {
    pub state_id: Option<i32>,
    pub phase: Option<DungeonFlowPhase>,
    pub active_time_raw: Option<i32>,
    pub ready_time_raw: Option<i32>,
    pub play_time_raw: Option<i32>,
    pub end_time_raw: Option<i32>,
    pub settlement_time_raw: Option<i32>,
    pub dungeon_times_raw: Option<i32>,
    pub result_id: Option<i32>,
}

impl DungeonFlowSnapshot {
    pub fn has_evidence(&self) -> bool {
        self.state_id.is_some()
            || self.active_time_raw.is_some()
            || self.ready_time_raw.is_some()
            || self.play_time_raw.is_some()
            || self.end_time_raw.is_some()
            || self.settlement_time_raw.is_some()
            || self.dungeon_times_raw.is_some()
            || self.result_id.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DungeonEvent {
    pub kind: DungeonEventKind,
    pub dungeon_id: Option<DungeonId>,
    pub instance_id: Option<String>,
    pub difficulty_id: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objective_map_key: Option<i32>,
    pub objective_id: Option<i64>,
    pub objective_value: Option<i64>,
    #[serde(default)]
    pub objective_complete: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow: Option<DungeonFlowSnapshot>,
}
