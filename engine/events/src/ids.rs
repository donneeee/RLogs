use serde::{Deserialize, Serialize};

/// A privacy-safe, log-local actor identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ActorId(pub u64);

/// Exact signed entity UUID carried by the game protocol.
///
/// This is gameplay evidence, not an account identifier. Submission builders
/// may still replace or omit it according to the public-log policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EntityUuid(pub i64);

/// Plugin-facing entity identity containing both the compact log-local ID and
/// the exact game UUID needed to correlate packet evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntityRef {
    pub actor_id: ActorId,
    pub entity_uuid: EntityUuid,
}

/// A game ability identifier retained exactly as decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AbilityId(pub i64);

/// A game status-effect identifier retained exactly as decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StatusEffectId(pub i64);

/// Exact instance identifier for one application of a status effect.
///
/// A status-effect ID identifies what the effect is; this value distinguishes
/// concurrent applications and lets reducers match lifecycle updates without
/// guessing from localized names or timestamps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StatusEffectInstanceId(pub i64);

/// A scene, map, or dungeon identifier retained exactly as decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SceneId(pub i32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MapId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DungeonId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MonsterId(pub i64);
