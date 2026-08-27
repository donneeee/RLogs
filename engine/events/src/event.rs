use serde::{Deserialize, Serialize};

use crate::{
    AbilityId, EntityRef, EntityUuid, EventTopic, MonsterId, SceneId, StatusEffectId,
    StatusEffectInstanceId,
};

/// Capture-observed time is monotonic within a log. Game time is optional
/// because not every packet carries an authoritative server timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventTime {
    pub observed_micros: u64,
    pub game_time_millis: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceConfidence {
    Exact,
    Inferred,
    Estimated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EvidenceSource {
    Wire {
        capture_sequence: u64,
        connection_id: u64,
        stream_id: u64,
    },
    Derived {
        rule_id: String,
        evidence_sequences: Vec<u64>,
    },
    Manual {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventProvenance {
    pub confidence: EvidenceConfidence,
    pub source: EvidenceSource,
}

impl EventProvenance {
    pub fn wire(capture_sequence: u64, connection_id: u64, stream_id: u64) -> Self {
        Self {
            confidence: EvidenceConfidence::Exact,
            source: EvidenceSource::Wire {
                capture_sequence,
                connection_id,
                stream_id,
            },
        }
    }

    pub fn manual(reason: impl Into<String>) -> Self {
        Self {
            confidence: EvidenceConfidence::Exact,
            source: EvidenceSource::Manual {
                reason: reason.into(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineEventDraft {
    pub time: EventTime,
    pub provenance: EventProvenance,
    pub kind: TimelineEventKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineEvent {
    /// Stable within this run, beginning at one.
    pub sequence: u64,
    pub time: EventTime,
    pub provenance: EventProvenance,
    pub kind: TimelineEventKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum TimelineEventKind {
    RunBoundary {
        state: RunState,
        scene_id: Option<SceneId>,
        reason: BoundaryReason,
    },
    EncounterBoundary {
        state: EncounterState,
        encounter_id: Option<String>,
        reason: BoundaryReason,
    },
    CombatBoundary {
        state: CombatState,
        reason: BoundaryReason,
    },
    Actor(ActorEvent),
    EntityAttributes(EntityAttributeEvent),
    TemporaryAttributes(TemporaryAttributeEvent),
    Cast(CastEvent),
    Cooldown(CooldownEvent),
    Resource(ResourceEvent),
    Damage(DamageEvent),
    Healing(HealingEvent),
    Shield(ShieldEvent),
    Life {
        actor: EntityRef,
        state: LifeState,
    },
    Status(StatusEvent),
    /// A status lifecycle observation whose exact effect identity or transition
    /// semantics could not be resolved. This remains combat evidence and must
    /// not be promoted into a global data-loss boundary.
    UnresolvedStatus(UnresolvedStatusEvent),
    /// A combat-action lifecycle observation whose exact wire identity is
    /// retained but whose semantic provider relation is not yet proven.
    /// Consumers must not treat `container` as the provider.
    UnresolvedAction(UnresolvedActionEvent),
    Position(PositionEvent),
    /// A completed user-requested pause interval. This is emitted when capture
    /// resumes so the sealed log contains both exact endpoints.
    RecorderPause(RecorderPauseEvent),
    DataGap(DataGapEvent),
}

impl TimelineEventKind {
    pub fn topic(&self) -> EventTopic {
        match self {
            Self::RunBoundary { .. }
            | Self::EncounterBoundary { .. }
            | Self::CombatBoundary { .. } => EventTopic::Encounter,
            Self::Actor(_)
            | Self::EntityAttributes(_)
            | Self::TemporaryAttributes(_)
            | Self::Life { .. }
            | Self::Position(_) => EventTopic::Actor,
            Self::Cast(_)
            | Self::Cooldown(_)
            | Self::Resource(_)
            | Self::Damage(_)
            | Self::Healing(_)
            | Self::Shield(_)
            | Self::Status(_)
            | Self::UnresolvedStatus(_)
            | Self::UnresolvedAction(_) => EventTopic::Combat,
            Self::RecorderPause(_) | Self::DataGap(_) => EventTopic::DataQuality,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryReason {
    AuthoritativePacket,
    SceneTransition,
    HostileAction,
    ActorLifecycle,
    Completion,
    Wipe,
    InactivityFallback,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Entered,
    Started,
    Ended,
    Completed,
    Failed,
    Exited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EncounterState {
    Started,
    Cleared,
    Wiped,
    Ended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CombatState {
    Started,
    Ended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    Player,
    Monster,
    Npc,
    SceneObject,
    Zone,
    Projectile,
    Pet,
    TrainingDummy,
    Drop,
    Field,
    Trap,
    Collection,
    StaticObject,
    Vehicle,
    Toy,
    Housing,
    Unknown(i32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorState {
    Spawned,
    Updated,
    Transformed,
    Despawned,
}

/// Strength of the packet evidence behind an actor loadout projection.
/// Consumers must not let an unordered observed set replace a packet-proven
/// slot assignment. `ExactSlots` also makes an empty vector meaningful: the
/// observed slot group was explicitly empty at that point in time.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorLoadoutEvidence {
    #[default]
    Unobserved,
    ObservedSet,
    ExactSlots,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorLoadoutObservation {
    #[serde(default)]
    pub primary: ActorLoadoutEvidence,
    #[serde(default)]
    pub auxiliary: ActorLoadoutEvidence,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActorEvent {
    pub actor: EntityRef,
    pub state: ActorState,
    /// Exact protocol enum value retained even when `kind` is normalized.
    pub entity_type_id: i32,
    pub kind: ActorKind,
    pub monster_id: Option<MonsterId>,
    /// Stable public character identity when the game integration proves the
    /// runtime entity belongs to a player. Runtime actor/entity identifiers
    /// remain authoritative for packet evidence; this key only joins their
    /// time-scoped presentation and combat state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character_id: Option<String>,
    pub display_name: Option<String>,
    pub class_id: Option<i32>,
    /// Normalized combat specialization when the game integration exposes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub specialization_id: Option<i32>,
    pub level: Option<u32>,
    /// Game-defined overall character Ability Score / combat power.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ability_score: Option<i64>,
    /// Game-defined equipped weapon configuration ID when directly observed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weapon_item_id: Option<i64>,
    /// Per-instance weapon breakthrough count when directly observed. This is
    /// intentionally not inferred for remote party members.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weapon_breakthrough_count: Option<u32>,
    /// Game-defined seasonal progression or strength score for this actor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seasonal_score: Option<i64>,
    /// Exact equipped slots observed for the game's two primary summon-style
    /// abilities. IDs stay game-neutral and presentation is joined later.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub primary_loadout: Vec<ActorLoadoutSlot>,
    /// Exact equipped auxiliary/role slots. An empty vector means the packet
    /// did not prove this group and must not be rendered as an equipped item.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub auxiliary_loadout: Vec<ActorLoadoutSlot>,
    /// Packet-evidence strength for each loadout group. This is separate from
    /// the vectors so an exact empty snapshot can clear stale equipped data.
    #[serde(default)]
    pub loadout_observation: ActorLoadoutObservation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorLoadoutSlot {
    pub slot_id: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ability_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_id: Option<i64>,
    /// Game-defined equipped enhancement/remodel tier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityAttributeEvent {
    pub actor: EntityRef,
    /// Whether the protocol supplied an actor-appearance snapshot or a later
    /// sparse attribute delta. Older logs predate this distinction and remain
    /// explicitly unknown; consumers must never treat unknown as a snapshot.
    #[serde(default)]
    pub update_kind: EntityAttributeUpdateKind,
    /// Packet-proven ownership transition decoded by the active game plug-in.
    /// Raw attributes remain alongside this normalized relation as evidence.
    /// Consumers must not infer ownership from actor names or kinds when this
    /// field is absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ownership: Option<ActorOwnershipUpdate>,
    pub attributes: Vec<EntityAttribute>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ActorOwnershipUpdate {
    Confirmed { owner_entity_uuid: EntityUuid },
    Cleared,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityAttributeUpdateKind {
    #[default]
    Unknown,
    Snapshot,
    Delta,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityAttribute {
    pub attribute_id: i32,
    /// Exact attribute payload after protobuf field decoding.
    pub raw_value: Vec<u8>,
    pub decoded: Option<EntityAttributeValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum EntityAttributeValue {
    Integer(i64),
    Text(String),
    Position {
        x: f32,
        y: f32,
        z: f32,
        facing_radians: Option<f32>,
    },
}

/// Exact temporary modifier values carried by the game protocol.
///
/// The game plug-in owns the table that assigns semantics to each numeric ID.
/// Canonical logs retain the ID and raw signed value so later formula and rDPS
/// policy updates never need to replay packet captures or reinterpret a value
/// that was scaled during decoding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemporaryAttributeEvent {
    pub actor: EntityRef,
    pub update_kind: EntityAttributeUpdateKind,
    pub attributes: Vec<TemporaryAttribute>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemporaryAttribute {
    pub id: i32,
    pub value: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CastState {
    Started,
    Completed,
    Interrupted,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CastEvent {
    pub source: EntityRef,
    pub ability: AbilityId,
    pub target: Option<EntityRef>,
    pub state: CastState,
    /// Exact action-time inputs carried by a client gameplay request, when the
    /// game plug-in has a build-locked decoder for that request.
    ///
    /// This is deliberately optional: server-only cast notifications and
    /// older canonical logs remain valid, while reducers that need action-time
    /// speed evidence can fail closed when the snapshot is absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_timing: Option<ActionTimingSnapshot>,
}

/// Compact, game-neutral action snapshot retained at the instant an action is
/// requested.
///
/// All timing and speed values remain in exact decoded client units. The game
/// plug-in owns their formulas and must prove the relevant build contract
/// before using them for APM, cooldown, or rDPS calculations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionTimingSnapshot {
    /// Per-action instance identifier used to join later stage transitions.
    pub action_instance_id: i64,
    /// Base ability identifier before level-specific expansion.
    pub base_ability: AbilityId,
    pub ability_level: i32,
    pub slot_id: i32,
    /// Client action timestamp in the exact protocol unit.
    pub client_timestamp_raw: u64,
    /// Client action begin value in the exact protocol unit.
    pub begin_time_raw: i64,
    /// Exact fixed-point speed inputs captured for this action.
    pub attack_speed_basis_points: i32,
    pub cast_speed_basis_points: i32,
    pub charge_speed_basis_points: i32,
    /// Exact action flags needed to exclude passive or triggered work from
    /// active-action accounting without guessing from damage hit counts.
    pub passive: bool,
    pub activated_roulette: bool,
    pub target_part_id: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CooldownEvent {
    pub actor: EntityRef,
    pub ability: AbilityId,
    pub begin_time_millis: Option<i64>,
    pub duration_millis: Option<i32>,
    pub valid_duration_millis: Option<i32>,
    pub cooldown_type: Option<i32>,
    /// Exact `SkillCDInfo.profession_hold_begin_time` wire value.
    pub profession_hold_begin_time_millis: Option<i64>,
    /// Exact `SkillCDInfo.charge_count` wire value.
    pub charge_count: Option<i32>,
    /// Exact `SkillCDInfo.valid_cd_time` wire value.
    pub valid_cooldown_time_millis: Option<i32>,
    /// Exact `SkillCDInfo.sub_cd_ratio` wire value. Its gameplay unit is kept
    /// intentionally raw until an observed cooldown transition proves it.
    pub sub_cooldown_ratio_raw: Option<i32>,
    /// Exact `SkillCDInfo.sub_cd_fixed` wire value. Its gameplay unit is kept
    /// intentionally raw until an observed cooldown transition proves it.
    pub sub_cooldown_fixed_raw: Option<i64>,
    /// Exact `SkillCDInfo.accelerate_cd_ratio` wire value. Its gameplay unit is
    /// kept intentionally raw until an observed cooldown transition proves it.
    pub accelerate_cooldown_ratio_raw: Option<i32>,
}

/// Exact combat-resource state carried by the game protocol.
///
/// Resource identifiers and values deliberately remain as parallel wire
/// arrays. Consumers must not silently truncate a malformed or newly changed
/// packet by zipping arrays of different lengths. The game plug-in owns their
/// semantics and formula units.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceEvent {
    pub actor: EntityRef,
    pub update_kind: EntityAttributeUpdateKind,
    /// Exact IEEE-754 bits from the protocol's `origin_energy` float.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_energy_raw_bits: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_ids: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_values: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cooldowns: Vec<ResourceCooldown>,
}

/// Exact resource-cooldown fields from the local-player AOI stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceCooldown {
    pub resource_id: Option<i32>,
    pub begin_time_millis: Option<i64>,
    pub duration_millis: Option<i32>,
    pub valid_cooldown_time_millis: Option<i32>,
    pub existence_time_millis: Option<i32>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DamageFlags {
    pub critical: Option<bool>,
    pub lucky: Option<bool>,
    /// The wire damage-type flag says this hit may trigger Lucky. It does not
    /// mean that the damage value itself was Lucky-amplified.
    #[serde(default)]
    pub causes_lucky: Option<bool>,
    pub blocked: Option<bool>,
    pub periodic: Option<bool>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct DamagePosition {
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub z: Option<f32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DamageHitPart {
    pub part_id: Option<i32>,
    pub position: Option<DamagePosition>,
    pub damage_value: Option<i64>,
}

/// Exact packet-only detail retained for formula research and future game
/// plug-ins. Generic reducers can ignore this without losing canonical damage.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DamagePacketDetail {
    /// Raw immediate attacker UUID from `SyncDamageInfo`. Canonical
    /// `direct_source`/`source` remain the semantic attribution views.
    #[serde(default)]
    pub attacker_uuid: Option<i64>,
    /// Raw top-summoner UUID from `SyncDamageInfo` before attribution.
    #[serde(default)]
    pub top_summoner_uuid: Option<i64>,
    /// Raw owner/ability ID from `SyncDamageInfo` before `AbilityId` wrapping.
    #[serde(default)]
    pub owner_id: Option<i32>,
    /// Raw death bit on this exact combat result. The decoder also emits the
    /// semantic `LifeState::Died` event when this is true.
    #[serde(default)]
    pub dead: Option<bool>,
    pub missed: Option<bool>,
    /// Raw protobuf field 3. Current combat-result critical state is carried by
    /// bit 0 of `type_flags`; this field is retained separately as evidence.
    pub reported_critical: Option<bool>,
    pub type_flags: Option<i32>,
    pub normal_value: Option<i64>,
    pub lucky_value: Option<i64>,
    pub owner_level: Option<i32>,
    pub owner_stage: Option<i32>,
    pub normal_hit: Option<bool>,
    pub property: Option<i32>,
    pub position: Option<DamagePosition>,
    #[serde(default)]
    pub hit_parts: Vec<DamageHitPart>,
    pub damage_weight: Option<DamagePosition>,
    pub passive_uuid: Option<u32>,
    pub rainbow: Option<bool>,
    pub damage_mode: Option<i32>,
    /// Raw `SkillEffect.uuid` for the protobuf group that contained this
    /// component. This is not an actor identity; it is retained so game
    /// plug-ins can reconstruct the exact server-reported damage result.
    #[serde(default)]
    pub skill_effect_uuid: Option<i64>,
    /// Raw `SkillEffect.total_damage` for the protobuf group that contained
    /// this component.
    #[serde(default)]
    pub skill_effect_total_damage: Option<i64>,
    /// Zero-based `AoiSyncDelta` index inside the decoded wire message.
    #[serde(default)]
    pub skill_effect_group_index: Option<u32>,
    /// Zero-based position of this `DamageInfo` inside `SkillEffect.damage`.
    #[serde(default)]
    pub skill_effect_component_index: Option<u32>,
    /// Exact number of `DamageInfo` entries in the enclosing `SkillEffect`.
    #[serde(default)]
    pub skill_effect_component_count: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DamageEvent {
    /// Owner or top-summoner attribution used for totals.
    pub source: EntityRef,
    /// Immediate wire attacker when it differs from the attributed source.
    pub direct_source: Option<EntityRef>,
    pub target: EntityRef,
    pub ability: Option<AbilityId>,
    /// Primary amount reported by the game.
    pub amount: i64,
    pub actual_amount: Option<i64>,
    pub hp_loss: Option<i64>,
    pub shield_loss: Option<i64>,
    pub hit_event_id: Option<i32>,
    pub damage_source: Option<i32>,
    pub damage_type: Option<i32>,
    pub flags: DamageFlags,
    #[serde(default)]
    pub packet: DamagePacketDetail,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HealingEvent {
    pub source: EntityRef,
    pub direct_source: Option<EntityRef>,
    pub target: EntityRef,
    pub ability: Option<AbilityId>,
    pub amount: i64,
    /// Raw `DamageInfo.actual_value`. The game transports healing through the
    /// same combat-result message as damage, so this must remain available for
    /// formula proof even though generic healing reducers do not require it.
    #[serde(default)]
    pub actual_amount: Option<i64>,
    /// Raw `DamageInfo.hp_loss`; retained verbatim rather than assigning a
    /// healing-specific meaning that the packet does not prove.
    #[serde(default)]
    pub hp_loss: Option<i64>,
    /// Raw `DamageInfo.shield_loss`; retained verbatim for shield/heal family
    /// correlation.
    #[serde(default)]
    pub shield_loss: Option<i64>,
    #[serde(default)]
    pub hit_event_id: Option<i32>,
    #[serde(default)]
    pub damage_source: Option<i32>,
    #[serde(default)]
    pub damage_type: Option<i32>,
    pub effective_amount: Option<i64>,
    pub overheal: Option<i64>,
    pub critical: Option<bool>,
    pub periodic: Option<bool>,
    /// Full packet-only combat-result detail. BPSR calls this `DamageInfo` even
    /// for healing; preserving it prevents HP, lucky-heal, level, stage, and
    /// source-route evidence from disappearing at the canonical boundary.
    #[serde(default)]
    pub packet: DamagePacketDetail,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShieldEvent {
    pub source: EntityRef,
    pub target: EntityRef,
    pub ability: AbilityId,
    pub amount: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifeState {
    Died,
    Revived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusState {
    Applied,
    Refreshed,
    Stacked,
    Consumed,
    Removed,
}

/// Exact game-provided origin of a status effect.
///
/// The numeric namespace is interpreted by the game plug-in that decoded the
/// event. Keeping both values on the canonical event preserves the packet
/// evidence needed to map an effect back to its skill, item, passive, or other
/// configured source without temporal guessing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusOrigin {
    pub source_type_id: i32,
    pub source_config_id: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatusEvent {
    pub source: Option<EntityRef>,
    pub target: EntityRef,
    pub effect: StatusEffectId,
    /// Exact wire instance used to correlate apply/change/remove events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<StatusEffectInstanceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<StatusOrigin>,
    pub state: StatusState,
    pub stacks: Option<u32>,
    pub duration_millis: Option<u64>,
    /// Exact status level carried by the decoded game message, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<i32>,
    /// Exact part identifier carried by the decoded game message, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part_id: Option<i32>,
    /// Exact status count carried by the decoded game message, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<i32>,
    /// Exact game creation timestamp carried by the decoded message, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at_millis: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnresolvedStatusReason {
    MissingInstanceId,
    MissingEffectId,
    MissingActiveEffectMapping,
    PayloadDecodeFailed,
    AmbiguousTransition,
}

/// Lossless canonical evidence for a status lifecycle that cannot yet be
/// joined to an exact numeric effect ID. Consumers must treat it as a possible
/// confounder and must never infer an effect ID, magnitude, or provider credit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnresolvedStatusEvent {
    pub source: Option<EntityRef>,
    pub target: EntityRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<StatusEffectInstanceId>,
    pub state: Option<StatusState>,
    pub wire_event_type: Option<i32>,
    pub wire_logic_type: Option<i32>,
    pub reason: UnresolvedStatusReason,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub raw_payload: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnresolvedActionReason {
    /// The game message proves a containing actor and action/target fields, but
    /// exact-build evidence has not proven that the container owns or provides
    /// the action.
    ProviderOwnershipUnproven,
    /// The enclosing message carried an action record that the active
    /// build-locked decoder could not decode.
    PayloadDecodeFailed,
    /// The action record was present without the enclosing actor identity
    /// needed for a provider relation.
    MissingContainerIdentity,
}

/// Lossless canonical evidence for an action lifecycle that cannot yet be
/// joined to a proven provider. Numeric identities remain raw and authoritative;
/// localized names and inferred actor allegiance are never substituted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnresolvedActionEvent {
    /// Entity whose protocol delta contained this action record. This is an
    /// exact wire relation, but is deliberately not named `source` or
    /// `provider` because those semantics require separate proof.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<EntityRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<EntityRef>,
    /// Exact per-action/projectile instance identifier carried by the message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_instance_id: Option<i64>,
    /// Exact numeric action/table identity carried by the game message. Its
    /// table domain remains game-plug-in-owned until proven for the exact build.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_part_id: Option<i32>,
    /// Exact game-specific action discriminant. For BPSR fake-bullet records,
    /// this is the `EDamageSourceFakeBullet` numeric value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wire_action_type: Option<i32>,
    pub reason: UnresolvedActionReason,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub raw_payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PositionEvent {
    pub actor: EntityRef,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub facing_radians: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecorderPauseEvent {
    pub started_micros: u64,
    pub resumed_micros: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataGapKind {
    CaptureDrop,
    TcpGap,
    UnknownRoute,
    DecodeFailure,
    UnsupportedFragment,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataGapEvent {
    pub kind: DataGapKind,
    pub connection_id: Option<u64>,
    pub stream_id: Option<u64>,
    pub detail: String,
}

#[cfg(test)]
mod tests {
    use crate::{AbilityId, ActorId, EntityRef, EntityUuid};

    use super::{
        ActionTimingSnapshot, CastEvent, CastState, DamagePacketDetail, TimelineEventKind,
        UnresolvedActionEvent, UnresolvedActionReason,
    };

    #[test]
    fn older_packet_detail_without_raw_identity_fields_remains_readable() {
        let detail: DamagePacketDetail =
            serde_json::from_str(r#"{"owner_level":60,"normal_hit":true,"hit_parts":[]}"#)
                .expect("deserialize pre-raw-identity packet detail");

        assert_eq!(detail.attacker_uuid, None);
        assert_eq!(detail.top_summoner_uuid, None);
        assert_eq!(detail.owner_id, None);
        assert_eq!(detail.dead, None);
        assert_eq!(detail.owner_level, Some(60));
        assert_eq!(detail.normal_hit, Some(true));
    }

    #[test]
    fn raw_identity_and_death_evidence_round_trip_exactly() {
        let detail = DamagePacketDetail {
            attacker_uuid: Some(101),
            top_summoner_uuid: Some(202),
            owner_id: Some(303),
            dead: Some(true),
            ..DamagePacketDetail::default()
        };

        let json = serde_json::to_string(&detail).expect("serialize packet detail");
        let decoded: DamagePacketDetail =
            serde_json::from_str(&json).expect("deserialize packet detail");

        assert_eq!(decoded, detail);
    }

    #[test]
    fn older_cast_without_action_timing_remains_readable() {
        let cast: CastEvent = serde_json::from_str(
            r#"{"source":{"actor_id":1,"entity_uuid":2},"ability":2233,"target":null,"state":"started"}"#,
        )
        .expect("deserialize server-only cast");

        assert_eq!(cast.action_timing, None);
    }

    #[test]
    fn action_timing_round_trips_exact_client_units() {
        let cast = CastEvent {
            source: EntityRef {
                actor_id: ActorId(1),
                entity_uuid: EntityUuid(2),
            },
            ability: AbilityId(2_233),
            target: None,
            state: CastState::Started,
            action_timing: Some(ActionTimingSnapshot {
                action_instance_id: 9_001,
                base_ability: AbilityId(2_233),
                ability_level: 5,
                slot_id: 21,
                client_timestamp_raw: 1_786_202_388_123,
                begin_time_raw: 1_786_202_388_120,
                attack_speed_basis_points: 230,
                cast_speed_basis_points: 382,
                charge_speed_basis_points: 145,
                passive: true,
                activated_roulette: true,
                target_part_id: 3,
            }),
        };

        let json = serde_json::to_string(&cast).expect("serialize action cast");
        let decoded: CastEvent = serde_json::from_str(&json).expect("deserialize action cast");
        assert_eq!(decoded, cast);
    }

    #[test]
    fn unresolved_action_round_trips_without_provider_semantics() {
        let event = TimelineEventKind::UnresolvedAction(UnresolvedActionEvent {
            container: Some(EntityRef {
                actor_id: ActorId(1),
                entity_uuid: EntityUuid(2),
            }),
            target: Some(EntityRef {
                actor_id: ActorId(3),
                entity_uuid: EntityUuid(4),
            }),
            action_instance_id: Some(44),
            action_id: Some(220_101),
            target_part_id: Some(7),
            wire_action_type: Some(4),
            reason: UnresolvedActionReason::ProviderOwnershipUnproven,
            raw_payload: vec![8, 44, 16, 197, 183, 13],
        });

        let json = serde_json::to_string(&event).expect("serialize unresolved action");
        assert!(json.contains(r#""event":"unresolved_action""#));
        assert!(!json.contains(r#""provider":"#));
        let decoded: TimelineEventKind =
            serde_json::from_str(&json).expect("deserialize unresolved action");
        assert_eq!(decoded, event);
    }
}
