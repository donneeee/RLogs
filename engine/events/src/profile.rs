use serde::{Deserialize, Serialize};

use crate::{CharacterIdentity, SceneId};

/// Game-neutral carrier for a privacy-reviewed character profile observation.
///
/// The trusted game plug-in owns the payload schema. Core only routes the
/// payload together with an explicit plug-in and schema identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameProfileEvent {
    pub game_plugin_id: String,
    pub payload_schema_id: String,
    pub payload_schema_version: u16,
    pub character: CharacterIdentity,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldContext {
    pub scene_id: Option<SceneId>,
    pub map_id: Option<u32>,
    pub line_id: Option<u32>,
    pub scene_instance_id: Option<String>,
    pub dungeon_instance_id: Option<String>,
}
