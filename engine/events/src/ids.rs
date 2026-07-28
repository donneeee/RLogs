use serde::{Deserialize, Serialize};

/// A privacy-safe, log-local actor identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ActorId(pub u64);

/// A game ability identifier retained exactly as decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AbilityId(pub i64);

/// A game status-effect identifier retained exactly as decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StatusEffectId(pub i64);

/// A scene, map, or dungeon identifier retained exactly as decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SceneId(pub i32);
