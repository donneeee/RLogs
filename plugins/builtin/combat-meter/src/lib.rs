//! Canonical-event combat timeline reducer.

use std::collections::{BTreeMap, BTreeSet};

use num_bigint::BigInt;
use num_integer::Integer;
use rlogs_combat::{
    ActorAncestryResolver, ActorOwnershipEvidence, ContributionDamageEvent,
    ContributionStatusEvent, ContributionStatusState, DamageContributionReducer,
    DamageContributionRule, DamageContributionScope, EffectDamageContribution,
    EncounterTerminalState, ExactDamageContributionEvent, ExactDamageContributionProjector,
    ExactRationalDamageContributionEvent, RunAnalysis, RunSegmentKind, RunSegmentSummary,
    RunTerminalState,
};
use rlogs_events::{
    ActorId, ActorKind, ActorLoadoutEvidence, ActorLoadoutSlot, ActorState, CanonicalEvent,
    CastState, CombatState, DungeonEventKind, EncounterState, EntityAttributeValue, EntityRef,
    EntityUuid, EventEnvelope, EventProvenance, EventTopic, EvidenceSource, LifeState, RunState,
    StatusState, TimelineEventKind,
};
use rlogs_log_format::RlogHeader;
use rlogs_plugin_api::PluginCapability;
use rlogs_plugin_runtime::{PluginFailure, PluginOutputSink, ReplayPlugin, ReplayPluginDescriptor};
use serde::{Deserialize, Serialize};

pub const COMBAT_METER_PLUGIN_ID: &str = "app.rlogs.combat-meter";
pub const COMBAT_SNAPSHOT_SCHEMA_ID: &str = "app.rlogs.combat-meter.snapshot";
pub const COMBAT_SNAPSHOT_SCHEMA_VERSION: u16 = 5;
pub const COMBAT_HISTORY_SCHEMA_VERSION: u16 = 1;
const COMBAT_INACTIVITY_TIMEOUT_MICROS: u64 = 8_000_000;
const MINIMUM_PERSONAL_ACTIVE_MICROS: u64 = 1_000_000;
const MAXIMUM_RUN_ENTRY_BOUNDARIES: usize = 256;
/// Live hover/drilldown data is derived state and must stay bounded even when a
/// capture runs for hours. History keeps its existing complete projection from
/// compact facts; only the ephemeral overlay relationship ledger uses this cap.
const MAXIMUM_LIVE_RDPS_INFLUENCE_RELATIONSHIPS: usize = 4_096;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CombatHistorySnapshot {
    pub schema_version: u16,
    pub session_id: String,
    pub deployment_id: String,
    pub region_id: String,
    pub world_id: Option<String>,
    pub client_build: String,
    pub protocol_pack_digest: String,
    /// Exact identity of the formula inputs and projection algorithm used to
    /// derive the rDPS fields below. Older history remains readable and is
    /// eligible for sealed-log replay when this is absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rdps_formula_identity: Option<String>,
    pub runs: Vec<CombatRunHistory>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CombatRunHistory {
    pub run_index: u32,
    pub activity_id: Option<String>,
    pub activity_family_id: Option<String>,
    pub scene_id: Option<i32>,
    /// Localized display-only scene label supplied by the owning game plug-in.
    /// The packet-derived scene ID remains authoritative.
    #[serde(default)]
    pub presentation_scene_name: Option<String>,
    pub instance_id: Option<String>,
    pub difficulty_family: Option<String>,
    pub difficulty_tier: Option<u32>,
    pub terminal_state: String,
    pub entered_micros: Option<u64>,
    pub started_micros: u64,
    pub first_combat_micros: Option<u64>,
    pub ended_micros: Option<u64>,
    pub load_time_micros: Option<u64>,
    pub precombat_time_micros: Option<u64>,
    /// Entry-to-completion wall time. Loading, cutscenes, transitions, and
    /// downtime are never removed.
    #[serde(default)]
    pub total_run_time_micros: Option<u64>,
    /// Reviewed gameplay intervals only: dungeon start through mobbing clear,
    /// then boss engagement through completion. The transition is excluded.
    pub game_time_micros: Option<u64>,
    /// Mobbing plus only the winning boss attempt. Losing boss attempts and
    /// their recovery gaps remain in the real views but not this projection.
    #[serde(default)]
    pub true_time_micros: Option<u64>,
    #[serde(default)]
    pub retry_count: u32,
    #[serde(default)]
    pub boss_retry_count: u32,
    /// Encounter attempts that ended in a packet-proven party wipe.
    #[serde(default)]
    pub wipe_count: u32,
    /// Encounter attempts that ended in a packet-proven clear.
    #[serde(default)]
    pub cleared_encounter_count: u32,
    /// Terminal state of the most recently closed encounter attempt.
    #[serde(default)]
    pub last_encounter_terminal_state: Option<String>,
    /// Indicates whether the game plug-in supplied at least one reviewed,
    /// deterministic contribution rule. A missing rule set must never make
    /// rDPS silently equal DPS.
    pub rdps_status: String,
    /// APM needs a game-owned classification of player-pressed actions.
    pub apm_status: String,
    pub views: Vec<CombatHistoryView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CombatHistoryView {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub segment_indices: Vec<u32>,
    pub elapsed_micros: u64,
    pub active_combat_micros: u64,
    pub actors: Vec<HistoryActorSummary>,
    pub targets: Vec<HistoryTargetIdentity>,
    /// Compact, exact relationships projected from packet-proven damage
    /// counterfactuals. Rows are grouped by effect, provider, recipient,
    /// affected ability, and damage target so history consumers can answer
    /// "what affected this damage and by how much" without replaying packets.
    #[serde(default)]
    pub damage_influences: Vec<HistoryDamageInfluenceSummary>,
    /// Display-only identities for rDPS effects referenced by this view.
    /// Exact numeric effect IDs remain the join and attribution authority.
    #[serde(default)]
    pub rdps_effect_presentations: Vec<HistoryRdpsEffectPresentation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryRdpsEffectPresentation {
    pub effect_id: String,
    pub presentation_name: String,
    pub presentation_kind: String,
    pub presentation_resolution: String,
    #[serde(default)]
    pub icon_asset_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryRationalDamageDelta {
    /// Reduced exact numerator. Text preserves values beyond JavaScript's
    /// safe-integer boundary.
    pub numerator: String,
    /// Reduced exact denominator. A consumer may display a decimal, but this
    /// fraction remains the authoritative amount.
    pub denominator: String,
    pub contribution_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryDamageInfluenceSummary {
    pub effect_id: String,
    #[serde(default)]
    pub attribution_component: Option<String>,
    #[serde(default = "default_complete_effect")]
    pub complete_effect: bool,
    pub provider_actor_id: String,
    pub provider_entity_uuid: String,
    pub recipient_actor_id: String,
    pub recipient_entity_uuid: String,
    /// Ability/damage ID on the packet-observed damage event.
    pub affected_ability_id: Option<String>,
    pub target_actor_id: Option<String>,
    pub target_entity_uuid: Option<String>,
    pub first_observed_micros: u64,
    pub last_observed_micros: u64,
    /// Unique canonical damage events represented by this relationship.
    pub damage_event_count: u64,
    /// Exact number of packet-reported critical hits among those unique
    /// damage events. Absent when the source event did not retain critical
    /// evidence (for example, an older history artifact).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub critical_hit_count: Option<u64>,
    /// Sum of the packet-observed damage values for those unique events. This
    /// is context, not an attribution total, and may appear in more than one
    /// relationship when multiple proven sources affect the same event.
    pub observed_damage: String,
    /// Exact integer counterfactual deltas. This is never inferred from a
    /// rounded rational term.
    pub exact_integer_delta: String,
    /// Exact rational counterfactual deltas grouped by denominator.
    #[serde(default)]
    pub exact_rational_deltas: Vec<HistoryRationalDamageDelta>,
    /// Integer amount actually applied to actor rDPS after the reducer sums
    /// all exact fractions for this effect/provider/recipient and rounds once.
    /// Missing means a legacy row or a fail-closed allocation mismatch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attributed_rdps: Option<String>,
    /// False only for legacy/custom projectors that emitted a contribution
    /// outside the damage event they were describing. Such evidence remains
    /// visible but must not be joined to a damage ID or target.
    pub damage_context_complete: bool,
}

fn default_complete_effect() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryActorSummary {
    pub actor_id: String,
    pub entity_uuid: String,
    /// Static game-owned monster identity observed on the actor event. This is
    /// intentionally kept separate from the per-spawn runtime entity UUID.
    #[serde(default)]
    pub monster_id: Option<String>,
    /// Stable game character identity, when a game-owned presentation pass can
    /// safely derive or join it. The raw entity UUID remains authoritative.
    #[serde(default)]
    pub character_id: Option<String>,
    pub display_name: Option<String>,
    pub actor_kind: Option<String>,
    /// Localized display-only identity. Raw packet-derived identity above is
    /// preserved for replay, audits, and the pre-localization Event Viewer.
    #[serde(default)]
    pub presentation_name: Option<String>,
    #[serde(default)]
    pub presentation_kind: Option<String>,
    pub class_id: Option<i32>,
    #[serde(default)]
    pub specialization_id: Option<i32>,
    #[serde(default)]
    pub presentation_class_name: Option<String>,
    #[serde(default)]
    pub presentation_specialization_name: Option<String>,
    #[serde(default)]
    pub icon_asset_path: Option<String>,
    #[serde(default)]
    pub weapon_icon_asset_path: Option<String>,
    #[serde(default)]
    pub presentation_role: Option<String>,
    #[serde(default)]
    pub presentation_accent: Option<String>,
    pub level: Option<u32>,
    #[serde(default)]
    pub ability_score: Option<i64>,
    #[serde(default)]
    pub weapon_item_id: Option<i64>,
    #[serde(default)]
    pub weapon_breakthrough_count: Option<u32>,
    #[serde(default)]
    pub weapon_presentation_name: Option<String>,
    #[serde(default)]
    pub weapon_level: Option<u32>,
    #[serde(default)]
    pub weapon_level_min: Option<u32>,
    #[serde(default)]
    pub weapon_level_max: Option<u32>,
    #[serde(default)]
    pub weapon_badge_kind: Option<String>,
    #[serde(default)]
    pub seasonal_score: Option<i64>,
    #[serde(default)]
    pub primary_loadout: Vec<HistoryLoadoutSlot>,
    #[serde(default)]
    pub auxiliary_loadout: Vec<HistoryLoadoutSlot>,
    pub damage: i64,
    pub effective_damage: i64,
    pub damage_taken: i64,
    pub healing: i64,
    pub effective_healing: i64,
    pub shielding: i64,
    pub hits: u64,
    pub critical_hits: u64,
    pub deaths: u64,
    /// Seconds from the selected view origin where this actor died. Older
    /// history artifacts deserialize with an empty list.
    #[serde(default)]
    pub death_seconds: Vec<u32>,
    /// Damage divided by the selected elapsed time.
    pub dps: f64,
    /// Damage divided by selected active-combat time. Downtime never lowers it.
    pub encounter_dps: f64,
    pub hps: f64,
    pub tps: f64,
    pub rdps: Option<f64>,
    /// Raw damage after subtracting externally received contribution and adding
    /// contribution provided to other actors. This is kept as an integer so
    /// the adjusted party total can be audited exactly.
    #[serde(default)]
    pub rdps_damage: Option<i64>,
    #[serde(default)]
    pub rdps_contribution_given: Option<i64>,
    #[serde(default)]
    pub rdps_contribution_received: Option<i64>,
    /// True when the numeric rDPS fields are conserved, packet-proven known
    /// subtotals but one or more external formula inputs remain unresolved.
    /// Ordinary damage is never affected by this marker.
    #[serde(default)]
    pub rdps_incomplete: bool,
    pub apm: Option<f64>,
    pub observed_cast_events: u64,
    pub abilities: Vec<HistoryAbilitySummary>,
    pub targets: Vec<HistoryTargetSummary>,
    pub effects: Vec<HistoryEffectSummary>,
    pub series: Vec<HistorySeriesPoint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryLoadoutSlot {
    pub slot_id: i32,
    #[serde(default)]
    pub ability_id: Option<i64>,
    #[serde(default)]
    pub item_id: Option<i64>,
    #[serde(default)]
    pub tier: Option<u32>,
    #[serde(default)]
    pub presentation_name: Option<String>,
    #[serde(default)]
    pub icon_asset_path: Option<String>,
    #[serde(default)]
    pub item_tier: Option<u32>,
    #[serde(default)]
    pub maximum_tier: Option<u32>,
}

impl From<&ActorLoadoutSlot> for HistoryLoadoutSlot {
    fn from(slot: &ActorLoadoutSlot) -> Self {
        Self {
            slot_id: slot.slot_id,
            ability_id: slot.ability_id,
            item_id: slot.item_id,
            tier: slot.tier,
            presentation_name: None,
            icon_asset_path: None,
            item_tier: None,
            maximum_tier: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryAbilitySummary {
    pub ability_id: String,
    #[serde(default)]
    pub presentation_name: Option<String>,
    #[serde(default)]
    pub presentation_kind: Option<String>,
    #[serde(default)]
    pub presentation_resolution: Option<String>,
    #[serde(default)]
    pub icon_asset_path: Option<String>,
    /// Optional game-owned presentation group. This never replaces the raw
    /// child ability row or changes actor totals; consumers may add a separate
    /// aggregate parent while retaining every observed child.
    #[serde(default)]
    pub presentation_recount_group_id: Option<String>,
    #[serde(default)]
    pub presentation_recount_group_name: Option<String>,
    pub casts: u64,
    pub hits: u64,
    pub critical_hits: u64,
    pub damage: i64,
    pub effective_damage: i64,
    pub healing: i64,
    pub effective_healing: i64,
    pub shielding: i64,
    pub dps: f64,
    pub encounter_dps: f64,
    pub hps: f64,
    pub targets: Vec<HistoryAbilityTargetSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryAbilityTargetSummary {
    pub actor_id: String,
    pub entity_uuid: String,
    pub damage: i64,
    pub effective_damage: i64,
    pub healing: i64,
    pub effective_healing: i64,
    pub shielding: i64,
    pub hits: u64,
    pub critical_hits: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryTargetSummary {
    pub actor_id: String,
    pub entity_uuid: String,
    pub damage: i64,
    pub effective_damage: i64,
    pub hits: u64,
    pub critical_hits: u64,
    pub effect_events: u64,
    /// Sparse one-second values exchanged with this exact counterpart. Damage
    /// and healing are outgoing from the owning actor; damage taken is
    /// incoming from this counterpart. This lets History filter charts by an
    /// entity without replaying the canonical archive.
    #[serde(default)]
    pub series: Vec<HistorySeriesPoint>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryTargetIdentity {
    pub actor_id: String,
    pub entity_uuid: String,
    #[serde(default)]
    pub monster_id: Option<String>,
    pub display_name: Option<String>,
    pub actor_kind: Option<String>,
    /// Localized display-only name supplied by the owning game integration.
    /// The packet-derived identifiers and name above remain authoritative.
    #[serde(default)]
    pub presentation_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEffectSummary {
    pub effect_id: String,
    #[serde(default)]
    pub presentation_name: Option<String>,
    #[serde(default)]
    pub presentation_kind: Option<String>,
    #[serde(default)]
    pub presentation_resolution: Option<String>,
    #[serde(default)]
    pub icon_asset_path: Option<String>,
    pub target_actor_id: String,
    pub target_entity_uuid: String,
    pub applied: u64,
    pub refreshed: u64,
    pub stacked: u64,
    pub consumed: u64,
    pub removed: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistorySeriesPoint {
    pub second: u32,
    pub damage: i64,
    pub effective_healing: i64,
    pub damage_taken: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CombatTimelineSnapshot {
    pub schema_version: u16,
    pub session_id: String,
    pub deployment_id: String,
    pub region_id: String,
    pub world_id: Option<String>,
    pub client_build: String,
    pub protocol_pack_digest: String,
    /// Formula readiness is independent from capture/parser readiness. A game
    /// update can make this status stale while every canonical event and raw
    /// combat total continues to flow normally.
    pub rdps_status: String,
    pub encounter_id: Option<String>,
    pub encounter_state: Option<String>,
    /// Packet-derived scene identity. Presentation/localization remains owned
    /// by the active game plug-in.
    #[serde(default)]
    pub scene_id: Option<i32>,
    pub event_count: u64,
    pub data_gap_count: u64,
    pub combat_window_count: u32,
    /// True while the reducer has an open combat window. Consumers must use
    /// this state instead of inferring combat from non-zero totals because a
    /// live meter retains those totals between pulls.
    #[serde(default)]
    pub combat_active: bool,
    /// Timestamp of the latest hostile event while combat is active. The
    /// overlay uses changes to this value to mirror the reducer's inactivity
    /// timeout even when no later packet arrives to close the window.
    #[serde(default)]
    pub last_hostile_micros: Option<u64>,
    /// Timestamp of the latest event consumed by the reducer. Together with
    /// `last_hostile_micros`, this lets a UI schedule only the remaining part
    /// of the reducer-owned inactivity window.
    #[serde(default)]
    pub latest_event_micros: Option<u64>,
    /// Reducer-owned inactivity interval. Presentation plug-ins may delay
    /// hiding further, but must not invent a different combat timeout.
    #[serde(default)]
    pub combat_inactivity_timeout_micros: u64,
    pub combat_started_micros: Option<u64>,
    pub combat_ended_micros: Option<u64>,
    pub active_combat_micros: u64,
    /// Elapsed wall time for only the current pull. This resets when a wipe or
    /// forced attempt reset is observed, then starts again on the next hostile
    /// event without changing the cumulative encounter or run clocks.
    #[serde(default)]
    pub attempt_elapsed_micros: Option<u64>,
    /// Cumulative elapsed-combat time across completed and current segments.
    /// Transitions and retry recovery pause this clock; a retry resumes it on
    /// the first hostile event without erasing the failed attempt.
    #[serde(default)]
    pub encounter_elapsed_micros: Option<u64>,
    /// Packet timestamp that froze the current encounter clock. A missing
    /// value means the current encounter remains open.
    #[serde(default)]
    pub encounter_terminal_micros: Option<u64>,
    /// Packet timestamp that froze the full run clock. Scene departure and
    /// authoritative run completion set this; ordinary boss clears do not.
    #[serde(default)]
    pub run_terminal_micros: Option<u64>,
    /// Elapsed wall time from the latest packet-derived dungeon entry through
    /// the latest canonical event. This clock never substitutes for reviewed
    /// in-game or projected-best timing.
    #[serde(default)]
    pub run_elapsed_micros: Option<u64>,
    /// Reserved for the authoritative game timer once its packet boundaries
    /// are proven for the active scene.
    #[serde(default)]
    pub game_time_micros: Option<u64>,
    /// Reserved for the reviewed winning mobbing plus boss projection.
    #[serde(default)]
    pub true_time_micros: Option<u64>,
    pub closed_at_log_end: bool,
    /// Bounded, ephemeral rDPS relationships for live skill drilldown. These
    /// rows are never written into sealed `.rlog` capture payloads.
    #[serde(default)]
    pub rdps_damage_influences: Vec<HistoryDamageInfluenceSummary>,
    /// True when additional live relationships were omitted by the hard cap.
    /// Actor totals remain authoritative; consumers must treat omitted detail
    /// as unavailable rather than redistribute the missing amount.
    #[serde(default)]
    pub rdps_damage_influences_truncated: bool,
    /// Display-only names/icons for effects referenced by the ephemeral live
    /// rows. The game-agnostic reducer leaves this empty for the desktop game
    /// adapter to enrich without changing canonical capture data.
    #[serde(default)]
    pub rdps_effect_presentations: Vec<HistoryRdpsEffectPresentation>,
    pub actors: Vec<ActorCombatSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActorCombatSummary {
    /// Decimal text preserves the full canonical identifier across browser boundaries.
    pub actor_id: String,
    /// Decimal text preserves signed game UUID values without JavaScript precision loss.
    pub entity_uuid: String,
    #[serde(default)]
    pub character_id: Option<String>,
    pub display_name: Option<String>,
    pub actor_kind: Option<String>,
    #[serde(default)]
    pub monster_id: Option<i64>,
    #[serde(default)]
    pub current_hp: Option<i64>,
    #[serde(default)]
    pub max_hp: Option<i64>,
    pub class_id: Option<i32>,
    #[serde(default)]
    pub specialization_id: Option<i32>,
    pub level: Option<u32>,
    #[serde(default)]
    pub ability_score: Option<i64>,
    #[serde(default)]
    pub weapon_item_id: Option<i64>,
    #[serde(default)]
    pub weapon_breakthrough_count: Option<u32>,
    #[serde(default)]
    pub seasonal_score: Option<i64>,
    #[serde(default)]
    pub primary_loadout: Vec<ActorLoadoutSlot>,
    #[serde(default)]
    pub auxiliary_loadout: Vec<ActorLoadoutSlot>,
    pub reported_damage: i64,
    pub effective_damage: i64,
    pub hp_damage: i64,
    pub shield_damage: i64,
    pub damage_during_combat: i64,
    pub damage_taken: i64,
    /// Legacy live active-combat rate retained for snapshot compatibility.
    pub dps: f64,
    /// Damage divided by the current attempt wall clock. Wipes reset it.
    #[serde(default)]
    pub run_dps: f64,
    /// Full-run damage divided by the sum of completed/open segment clocks.
    /// Transitions pause it; wipes do not erase failed-attempt damage.
    #[serde(default)]
    pub encounter_dps: f64,
    /// Damage divided by the sum of actual active-combat windows.
    #[serde(default)]
    pub active_dps: f64,
    pub hps: f64,
    pub tps: f64,
    #[serde(default)]
    pub rdps_damage: Option<i64>,
    #[serde(default)]
    pub rdps: Option<f64>,
    #[serde(default)]
    pub rdps_contribution_given: Option<i64>,
    #[serde(default)]
    pub rdps_contribution_received: Option<i64>,
    #[serde(default)]
    pub rdps_incomplete: bool,
    pub reported_healing: i64,
    pub effective_healing: i64,
    pub overheal: i64,
    pub shielding: i64,
    pub casts: u64,
    pub hits: u64,
    pub critical_hits: u64,
    pub deaths: u64,
    pub revives: u64,
    pub position_samples: u64,
    pub path_distance: f64,
    pub abilities: Vec<AbilityCombatSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AbilityCombatSummary {
    /// Decimal text preserves signed game identifiers without JavaScript precision loss.
    pub ability_id: String,
    pub casts: u64,
    pub hits: u64,
    pub critical_hits: u64,
    pub reported_damage: i64,
    pub effective_damage: i64,
    pub reported_healing: i64,
    pub effective_healing: i64,
    pub shielding: i64,
}

#[derive(Debug, Clone, Default)]
struct ActorAccumulator {
    entity_uuid: i64,
    character_id: Option<String>,
    monster_id: Option<i64>,
    current_hp: Option<i64>,
    max_hp: Option<i64>,
    display_name: Option<String>,
    actor_kind: Option<String>,
    class_id: Option<i32>,
    specialization_id: Option<i32>,
    level: Option<u32>,
    ability_score: Option<i64>,
    weapon_item_id: Option<i64>,
    weapon_breakthrough_count: Option<u32>,
    seasonal_score: Option<i64>,
    primary_loadout: Vec<ActorLoadoutSlot>,
    auxiliary_loadout: Vec<ActorLoadoutSlot>,
    primary_loadout_evidence: ActorLoadoutEvidence,
    auxiliary_loadout_evidence: ActorLoadoutEvidence,
    identity_observed_micros: u64,
    equipment_observed_micros: u64,
    primary_loadout_observed_micros: u64,
    auxiliary_loadout_observed_micros: u64,
    reported_damage: i64,
    effective_damage: i64,
    hp_damage: i64,
    shield_damage: i64,
    damage_during_combat: i64,
    damage_taken: i64,
    reported_healing: i64,
    effective_healing: i64,
    overheal: i64,
    shielding: i64,
    casts: u64,
    hits: u64,
    critical_hits: u64,
    deaths: u64,
    revives: u64,
    position_samples: u64,
    path_distance: f64,
    last_position: Option<(f32, f32, f32)>,
    abilities: BTreeMap<i64, AbilityAccumulator>,
}

impl ActorAccumulator {
    fn merge_from(&mut self, other: Self) {
        let other_identity_observed_micros = other.identity_observed_micros;
        let other_equipment_observed_micros = other.equipment_observed_micros;
        let other_primary_loadout_observed_micros = other.primary_loadout_observed_micros;
        let other_auxiliary_loadout_observed_micros = other.auxiliary_loadout_observed_micros;

        merge_observed_option(
            &mut self.character_id,
            other.character_id,
            self.identity_observed_micros,
            other_identity_observed_micros,
        );
        self.monster_id = other.monster_id.or(self.monster_id);
        self.current_hp = other.current_hp.or(self.current_hp);
        self.max_hp = other.max_hp.or(self.max_hp);
        merge_observed_option(
            &mut self.display_name,
            other.display_name,
            self.identity_observed_micros,
            other_identity_observed_micros,
        );
        merge_observed_option(
            &mut self.actor_kind,
            other.actor_kind,
            self.identity_observed_micros,
            other_identity_observed_micros,
        );
        merge_observed_option(
            &mut self.class_id,
            other.class_id,
            self.identity_observed_micros,
            other_identity_observed_micros,
        );
        merge_observed_option(
            &mut self.specialization_id,
            other.specialization_id,
            self.identity_observed_micros,
            other_identity_observed_micros,
        );
        merge_observed_option(
            &mut self.level,
            other.level,
            self.identity_observed_micros,
            other_identity_observed_micros,
        );
        merge_observed_option(
            &mut self.ability_score,
            other.ability_score,
            self.identity_observed_micros,
            other_identity_observed_micros,
        );
        merge_observed_option(
            &mut self.seasonal_score,
            other.seasonal_score,
            self.identity_observed_micros,
            other_identity_observed_micros,
        );
        merge_observed_option(
            &mut self.weapon_item_id,
            other.weapon_item_id,
            self.equipment_observed_micros,
            other_equipment_observed_micros,
        );
        merge_observed_option(
            &mut self.weapon_breakthrough_count,
            other.weapon_breakthrough_count,
            self.equipment_observed_micros,
            other_equipment_observed_micros,
        );
        if should_replace_loadout(
            self.primary_loadout_evidence,
            self.primary_loadout_observed_micros,
            other.primary_loadout_evidence,
            other_primary_loadout_observed_micros,
        ) {
            self.primary_loadout = other.primary_loadout;
            self.primary_loadout_evidence = other.primary_loadout_evidence;
            self.primary_loadout_observed_micros = other_primary_loadout_observed_micros;
        }
        if should_replace_loadout(
            self.auxiliary_loadout_evidence,
            self.auxiliary_loadout_observed_micros,
            other.auxiliary_loadout_evidence,
            other_auxiliary_loadout_observed_micros,
        ) {
            self.auxiliary_loadout = other.auxiliary_loadout;
            self.auxiliary_loadout_evidence = other.auxiliary_loadout_evidence;
            self.auxiliary_loadout_observed_micros = other_auxiliary_loadout_observed_micros;
        }
        self.identity_observed_micros = self
            .identity_observed_micros
            .max(other_identity_observed_micros);
        self.equipment_observed_micros = self
            .equipment_observed_micros
            .max(other_equipment_observed_micros);
        self.reported_damage = self.reported_damage.saturating_add(other.reported_damage);
        self.effective_damage = self.effective_damage.saturating_add(other.effective_damage);
        self.hp_damage = self.hp_damage.saturating_add(other.hp_damage);
        self.shield_damage = self.shield_damage.saturating_add(other.shield_damage);
        self.damage_during_combat = self
            .damage_during_combat
            .saturating_add(other.damage_during_combat);
        self.damage_taken = self.damage_taken.saturating_add(other.damage_taken);
        self.reported_healing = self.reported_healing.saturating_add(other.reported_healing);
        self.effective_healing = self
            .effective_healing
            .saturating_add(other.effective_healing);
        self.overheal = self.overheal.saturating_add(other.overheal);
        self.shielding = self.shielding.saturating_add(other.shielding);
        self.casts = self.casts.saturating_add(other.casts);
        self.hits = self.hits.saturating_add(other.hits);
        self.critical_hits = self.critical_hits.saturating_add(other.critical_hits);
        self.deaths = self.deaths.saturating_add(other.deaths);
        self.revives = self.revives.saturating_add(other.revives);
        self.position_samples = self.position_samples.saturating_add(other.position_samples);
        self.path_distance += other.path_distance;
        self.last_position = other.last_position.or(self.last_position);
        for (ability_id, other_ability) in other.abilities {
            let ability = self.abilities.entry(ability_id).or_default();
            ability.casts = ability.casts.saturating_add(other_ability.casts);
            ability.hits = ability.hits.saturating_add(other_ability.hits);
            ability.critical_hits = ability
                .critical_hits
                .saturating_add(other_ability.critical_hits);
            ability.reported_damage = ability
                .reported_damage
                .saturating_add(other_ability.reported_damage);
            ability.effective_damage = ability
                .effective_damage
                .saturating_add(other_ability.effective_damage);
            ability.reported_healing = ability
                .reported_healing
                .saturating_add(other_ability.reported_healing);
            ability.effective_healing = ability
                .effective_healing
                .saturating_add(other_ability.effective_healing);
            ability.shielding = ability.shielding.saturating_add(other_ability.shielding);
        }
    }

    /// Retain only stable player identity while discarding mutable class,
    /// equipment, and loadout state from the previous run or attempt.
    fn identity_only(&self) -> Self {
        Self {
            entity_uuid: self.entity_uuid,
            character_id: self.character_id.clone(),
            display_name: self.display_name.clone(),
            actor_kind: self.actor_kind.clone(),
            identity_observed_micros: self.identity_observed_micros,
            ..Self::default()
        }
    }

    /// Player loadout/profile packets can arrive before the AOI stream has
    /// classified the same actor as `player`. Keep that packet-proven
    /// presentation state across run and wipe resets without retaining actors
    /// that only look like monsters, projectiles, pets, or scene objects.
    fn has_player_identity_evidence(&self) -> bool {
        self.actor_kind.as_deref() == Some("player") || self.character_id.is_some()
    }
}

fn should_replace_loadout(
    current_evidence: ActorLoadoutEvidence,
    current_observed_micros: u64,
    incoming_evidence: ActorLoadoutEvidence,
    incoming_observed_micros: u64,
) -> bool {
    incoming_evidence != ActorLoadoutEvidence::Unobserved
        && (incoming_evidence > current_evidence
            || (incoming_evidence == current_evidence
                && incoming_observed_micros >= current_observed_micros))
}

fn merge_observed_option<T>(
    target: &mut Option<T>,
    incoming: Option<T>,
    target_observed_micros: u64,
    incoming_observed_micros: u64,
) {
    if incoming.is_some()
        && (target.is_none() || incoming_observed_micros >= target_observed_micros)
    {
        *target = incoming;
    }
}

/// Compact presentation evidence retained at the time an actor identity was
/// observed. Combat facts intentionally do not clone this payload: one
/// versioned ledger is shared by history, the live overlay, submissions, TPS,
/// and rDPS presentation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct HistoryActorIdentitySnapshot {
    entity_uuid: i64,
    character_id: Option<String>,
    monster_id: Option<i64>,
    display_name: Option<String>,
    actor_kind: Option<String>,
    class_id: Option<i32>,
    specialization_id: Option<i32>,
    level: Option<u32>,
    ability_score: Option<i64>,
    weapon_item_id: Option<i64>,
    weapon_breakthrough_count: Option<u32>,
    seasonal_score: Option<i64>,
    primary_loadout: Vec<HistoryLoadoutSlot>,
    auxiliary_loadout: Vec<HistoryLoadoutSlot>,
}

impl From<&ActorAccumulator> for HistoryActorIdentitySnapshot {
    fn from(actor: &ActorAccumulator) -> Self {
        Self {
            entity_uuid: actor.entity_uuid,
            character_id: actor.character_id.clone(),
            monster_id: actor.monster_id,
            display_name: actor.display_name.clone(),
            actor_kind: actor.actor_kind.clone(),
            class_id: actor.class_id,
            specialization_id: actor.specialization_id,
            level: actor.level,
            ability_score: actor.ability_score,
            weapon_item_id: actor.weapon_item_id,
            weapon_breakthrough_count: actor.weapon_breakthrough_count,
            seasonal_score: actor.seasonal_score,
            primary_loadout: actor
                .primary_loadout
                .iter()
                .map(HistoryLoadoutSlot::from)
                .collect(),
            auxiliary_loadout: actor
                .auxiliary_loadout
                .iter()
                .map(HistoryLoadoutSlot::from)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HistoryActorIdentityVersion {
    observed_micros: u64,
    identity: HistoryActorIdentitySnapshot,
}

#[derive(Debug, Clone, Default)]
struct AbilityAccumulator {
    casts: u64,
    hits: u64,
    critical_hits: u64,
    reported_damage: i64,
    effective_damage: i64,
    reported_healing: i64,
    effective_healing: i64,
    shielding: i64,
}

#[derive(Debug, Clone)]
struct CombatFact {
    observed_micros: u64,
    source_actor_id: u64,
    source_entity_uuid: i64,
    target: Option<(u64, i64)>,
    /// User-facing breakdown identity. Raw `ability_id` remains authoritative
    /// for rDPS joins and canonical audit replay.
    breakdown_ability_id: Option<i64>,
    ability_id: Option<i64>,
    kind: CombatFactKind,
}

#[derive(Debug, Clone, Copy)]
struct DamageProjectionContext {
    event_sequence: u64,
    recipient_actor_id: u64,
    recipient_entity_uuid: i64,
    affected_ability_id: Option<i64>,
    target_actor_id: u64,
    target_entity_uuid: i64,
    critical: Option<bool>,
}

#[derive(Debug, Clone)]
enum CombatFactKind {
    StatusReset,
    Cast,
    Damage {
        reported: i64,
        effective: i64,
        critical: bool,
    },
    Healing {
        reported: i64,
        effective: i64,
    },
    Shield {
        amount: i64,
    },
    Life {
        state: LifeState,
    },
    Status {
        effect_id: i64,
        attribution_source_actor_id: Option<u64>,
        instance_id: Option<i64>,
        state: StatusState,
        stacks: Option<u32>,
        duration_millis: Option<u64>,
    },
    ExactDamageContribution {
        effect_id: i64,
        scope: DamageContributionScope,
        provider_actor_id: u64,
        recipient_actor_id: u64,
        amount: i64,
        observed_damage: i64,
        damage_event_sequence: Option<u64>,
        affected_ability_id: Option<i64>,
        affected_target: Option<(u64, i64)>,
        critical: Option<bool>,
    },
    ExactRationalDamageContribution {
        effect_id: i64,
        scope: DamageContributionScope,
        provider_actor_id: u64,
        recipient_actor_id: u64,
        numerator: i128,
        denominator: i128,
        observed_damage: i64,
        damage_event_sequence: Option<u64>,
        affected_ability_id: Option<i64>,
        affected_target: Option<(u64, i64)>,
        critical: Option<bool>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct HistoryDamageInfluenceKey {
    effect_id: i64,
    scope: DamageContributionScope,
    provider_actor_id: u64,
    provider_entity_uuid: i64,
    recipient_actor_id: u64,
    recipient_entity_uuid: i64,
    affected_ability_id: Option<i64>,
    target_actor_id: Option<u64>,
    target_entity_uuid: Option<i64>,
}

#[derive(Debug, Clone, Default)]
struct HistoryDamageInfluenceAccumulator {
    first_observed_micros: Option<u64>,
    last_observed_micros: u64,
    damage_event_count: u64,
    critical_hit_count: Option<u64>,
    observed_damage: i64,
    exact_integer_delta: i64,
    rational_by_denominator: BTreeMap<i128, (i128, u64)>,
    last_damage_event_sequence: Option<u64>,
    damage_context_complete: bool,
}

#[derive(Debug, Clone, Default)]
struct HistoryValueAccumulator {
    entity_uuid: i64,
    casts: u64,
    hits: u64,
    critical_hits: u64,
    damage: i64,
    effective_damage: i64,
    damage_taken: i64,
    healing: i64,
    effective_healing: i64,
    shielding: i64,
    deaths: u64,
    death_seconds: Vec<u32>,
    rdps_damage: Option<i64>,
    rdps_contribution_given: Option<i64>,
    rdps_contribution_received: Option<i64>,
    rdps_incomplete: bool,
    abilities: BTreeMap<i64, HistoryAbilityAccumulator>,
    targets: BTreeMap<u64, HistoryTargetAccumulator>,
    effects: BTreeMap<(i64, u64), HistoryEffectAccumulator>,
    series: BTreeMap<u32, HistorySeriesAccumulator>,
}

#[derive(Debug, Clone, Default)]
struct HistoryAbilityAccumulator {
    casts: u64,
    hits: u64,
    critical_hits: u64,
    damage: i64,
    effective_damage: i64,
    healing: i64,
    effective_healing: i64,
    shielding: i64,
    targets: BTreeMap<u64, HistoryAbilityTargetAccumulator>,
}

#[derive(Debug, Clone, Default)]
struct HistoryAbilityTargetAccumulator {
    entity_uuid: i64,
    damage: i64,
    effective_damage: i64,
    healing: i64,
    effective_healing: i64,
    shielding: i64,
    hits: u64,
    critical_hits: u64,
}

#[derive(Debug, Clone, Default)]
struct HistoryTargetAccumulator {
    entity_uuid: i64,
    damage: i64,
    effective_damage: i64,
    hits: u64,
    critical_hits: u64,
    effect_events: u64,
    series: BTreeMap<u32, HistorySeriesAccumulator>,
}

#[derive(Debug, Clone, Default)]
struct HistoryEffectAccumulator {
    target_entity_uuid: i64,
    applied: u64,
    refreshed: u64,
    stacked: u64,
    consumed: u64,
    removed: u64,
}

#[derive(Debug, Clone, Default)]
struct HistorySeriesAccumulator {
    damage: i64,
    effective_healing: i64,
    damage_taken: i64,
}

#[derive(Debug, Default)]
pub struct CombatTimelinePlugin {
    header: Option<RlogHeader>,
    actors: BTreeMap<u64, ActorAccumulator>,
    /// Current runtime actor aliases keyed to the first canonical actor seen
    /// for a packet-proven stable character identity.
    actor_aliases: BTreeMap<u64, u64>,
    character_actors: BTreeMap<String, u64>,
    encounter_id: Option<String>,
    encounter_state: Option<String>,
    scene_id: Option<i32>,
    /// Game-owned scenes whose intermediate boss clears belong to one
    /// continuous Gauntlet encounter. The game catalog, not boss-count
    /// heuristics, owns this classification.
    continuous_encounter_scene_ids: BTreeSet<i32>,
    health_attributes: Option<LiveHealthAttributeMapping>,
    active_combat_started: Option<u64>,
    first_combat_started: Option<u64>,
    /// First hostile timestamp for the currently open segment. This resets
    /// between independently cleared raid bosses and across retry recovery,
    /// but not inside a game-owned boss-rush encounter.
    encounter_combat_started: Option<u64>,
    /// Completed mobbing/boss/failed-attempt time already banked into live
    /// eDPS. Transition and retry-recovery gaps are excluded.
    encounter_elapsed_micros: u64,
    last_combat_ended: Option<u64>,
    /// Freezes the live encounter-rate denominator on authoritative run
    /// completion. Encounter clears do not set this because raids can contain
    /// several bosses in separate realms under one scene ID.
    encounter_terminal_micros: Option<u64>,
    run_terminal_micros: Option<u64>,
    /// Full-run damage numerator for eDPS. Per-attempt actor accumulators can
    /// reset on wipes without erasing failed-attempt damage from this map.
    run_damage_during_combat: BTreeMap<u64, i64>,
    active_combat_micros: u64,
    /// Damage-driven combat windows retained independently from the
    /// game-specific run analyzer. Reviewed encounter rules remain
    /// authoritative when they provide active time; these windows are the
    /// evidence-backed fallback for newly added scenes whose rules have not
    /// been authored yet.
    combat_windows: Vec<(u64, u64)>,
    combat_window_count: u32,
    last_event_micros: Option<u64>,
    last_hostile_micros: Option<u64>,
    relevant_connection_ids: BTreeSet<u64>,
    run_entered_micros: Vec<u64>,
    history_facts: Vec<CombatFact>,
    /// Timestamped actor identity evidence. Matching the entity UUID as well
    /// as the short actor ID prevents a later spawn or current loadout from
    /// rewriting a completed run.
    history_identities: BTreeMap<u64, Vec<HistoryActorIdentityVersion>>,
    /// One time-aware ownership graph shared by live totals and every history
    /// projection. Game plug-ins provide the evidence; this reducer applies it
    /// consistently to pets, summons, projectiles, TPS, and rDPS.
    actor_ancestry: ActorAncestryResolver,
    contribution_rules: Vec<DamageContributionRule>,
    /// Optional game-owned resolver for user-facing child damage identities.
    /// The canonical raw ability remains authoritative for audit and rDPS
    /// joins. This fallback lets a newer exact game catalog reproject older
    /// sealed events that predate `DamagePacketDetail::breakdown_ability_id`.
    ability_breakdown_resolver: Option<AbilityBreakdownResolver>,
    exact_contribution_projector: Option<Box<dyn ExactDamageContributionProjector>>,
    projected_contributions: Vec<ExactDamageContributionEvent>,
    projected_rational_contributions: Vec<ExactRationalDamageContributionEvent>,
    /// Exact contribution terms emitted for the most recently observed event.
    /// This bounded, reused buffer lets research validators audit the same
    /// projection the live meter consumed without running a second projector.
    latest_exact_contributions: Vec<ExactDamageContributionEvent>,
    latest_exact_rational_contributions: Vec<ExactRationalDamageContributionEvent>,
    live_attribution: DamageContributionReducer,
    live_damage_influences: BTreeMap<HistoryDamageInfluenceKey, HistoryDamageInfluenceAccumulator>,
    live_damage_influences_truncated: bool,
    event_count: u64,
    data_gap_count: u64,
    closed_at_log_end: bool,
}

/// Game-owned attribute IDs used to expose exact live health without teaching
/// the game-agnostic combat reducer any protocol-specific numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveHealthAttributeMapping {
    pub current_hp: i32,
    pub max_hp: i32,
}

/// Pure, game-owned mapping from a canonical damage event to its exact
/// user-facing child identity. Function pointers keep the generic meter free
/// of game IDs and make replay deterministic and reset-safe.
pub type AbilityBreakdownResolver =
    fn(raw_ability_id: i64, hit_event_id: Option<i32>, damage_source: Option<i32>) -> Option<i64>;

impl CombatTimelinePlugin {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_damage_contribution_rules(
        rules: Vec<DamageContributionRule>,
    ) -> Result<Self, String> {
        Self::with_damage_contribution_projection(rules, None)
    }

    pub fn with_damage_contribution_projection(
        rules: Vec<DamageContributionRule>,
        exact_contribution_projector: Option<Box<dyn ExactDamageContributionProjector>>,
    ) -> Result<Self, String> {
        let live_attribution =
            DamageContributionReducer::new(rules.clone()).map_err(|error| error.to_string())?;
        Ok(Self {
            contribution_rules: rules,
            exact_contribution_projector,
            live_attribution,
            ..Self::default()
        })
    }

    pub fn with_live_health_attributes(mut self, mapping: LiveHealthAttributeMapping) -> Self {
        self.health_attributes = Some(mapping);
        self
    }

    pub fn with_ability_breakdown_resolver(mut self, resolver: AbilityBreakdownResolver) -> Self {
        self.ability_breakdown_resolver = Some(resolver);
        self
    }

    pub fn with_continuous_encounter_scenes(
        mut self,
        scene_ids: impl IntoIterator<Item = i32>,
    ) -> Self {
        self.continuous_encounter_scene_ids = scene_ids.into_iter().collect();
        self
    }

    /// Starts an incremental live projection without invoking the replay host.
    pub fn begin_live(&mut self, header: &RlogHeader) {
        let contribution_rules = self.contribution_rules.clone();
        let health_attributes = self.health_attributes;
        let ability_breakdown_resolver = self.ability_breakdown_resolver;
        let mut exact_contribution_projector = self.exact_contribution_projector.take();
        if let Some(projector) = exact_contribution_projector.as_mut() {
            projector.reset();
        }
        *self = Self::with_damage_contribution_projection(
            contribution_rules,
            exact_contribution_projector,
        )
        .expect("previously validated rDPS rules remain valid");
        self.health_attributes = health_attributes;
        self.ability_breakdown_resolver = ability_breakdown_resolver;
        self.header = Some(header.clone());
    }

    /// Starts a clean run projection without forgetting player identities
    /// already learned by the continuous packet stream.
    ///
    /// Capture can begin before or during a run, and team/AOI identity packets
    /// are not guaranteed to repeat immediately after a dungeon boundary. The
    /// metric reset must therefore preserve only player presentation evidence;
    /// all combat totals, targets, effects, and monster state still restart.
    pub fn begin_live_preserving_player_identities(&mut self, header: &RlogHeader) {
        let actor_aliases = self.actor_aliases.clone();
        let character_actors = self.character_actors.clone();
        let player_identities = self
            .actors
            .iter()
            .filter(|(_, actor)| actor.has_player_identity_evidence())
            .map(|(actor_id, actor)| (*actor_id, actor.identity_only()))
            .collect();
        self.begin_live(header);
        self.actors = player_identities;
        self.actor_aliases = actor_aliases;
        self.character_actors = character_actors;
        for actor_id in self.actors.keys().copied().collect::<Vec<_>>() {
            self.live_attribution.set_provider_eligible(actor_id, true);
            self.record_history_identity(0, actor_id);
        }
    }

    fn record_run_entry(&mut self, observed_micros: u64) {
        if self.run_entered_micros.last() == Some(&observed_micros) {
            return;
        }
        if self.run_entered_micros.len() == MAXIMUM_RUN_ENTRY_BOUNDARIES {
            self.run_entered_micros.remove(0);
        }
        self.run_entered_micros.push(observed_micros);
        self.encounter_combat_started = None;
        self.encounter_elapsed_micros = 0;
        self.encounter_terminal_micros = None;
        self.run_terminal_micros = None;
        self.run_damage_during_combat.clear();
    }

    fn mark_run_terminal(&mut self, observed_micros: u64) {
        self.run_terminal_micros.get_or_insert(observed_micros);
        self.close_encounter_clock(observed_micros);
        self.end_combat(observed_micros);
    }

    fn begin_encounter(&mut self, observed_micros: u64) {
        if self.encounter_terminal_micros.is_some() {
            if self.first_combat_started.is_some() {
                self.reset_live_attempt(observed_micros);
            }
            self.encounter_combat_started = self.active_combat_started.map(|_| observed_micros);
        }
        self.encounter_terminal_micros = None;
    }

    fn close_encounter_clock(&mut self, observed_micros: u64) {
        if let Some(started) = self.encounter_combat_started.take() {
            self.encounter_elapsed_micros = self
                .encounter_elapsed_micros
                .saturating_add(observed_micros.saturating_sub(started));
        }
        self.encounter_terminal_micros
            .get_or_insert(observed_micros);
    }

    fn mark_encounter_terminal(&mut self, observed_micros: u64) {
        self.close_encounter_clock(observed_micros);
        self.end_combat(observed_micros);
    }

    fn finish_encounter(&mut self, observed_micros: u64) {
        if self
            .scene_id
            .is_some_and(|scene_id| self.continuous_encounter_scene_ids.contains(&scene_id))
        {
            self.end_combat(observed_micros);
        } else {
            self.mark_encounter_terminal(observed_micros);
        }
    }

    fn run_entry_for(&self, started_micros: u64) -> Option<u64> {
        self.run_entered_micros
            .iter()
            .rev()
            .copied()
            .find(|entered_micros| *entered_micros <= started_micros)
    }

    /// Clears only the current live attempt after a packet-proven wipe.
    /// Completed attempt facts remain available to history and the run reducer;
    /// stable player presentation survives so the next pull does not regress to
    /// anonymous actor labels while waiting for another AOI update.
    fn reset_live_attempt(&mut self, observed_micros: u64) {
        // A wipe pauses cumulative eDPS. Its failed-attempt damage and elapsed
        // segment stay banked, while recovery time before the next hostile
        // event is excluded. DPS, aDPS, and rDPS reset below.
        self.close_encounter_clock(observed_micros);
        self.end_combat(observed_micros);
        self.actor_ancestry
            .end_active_ownership_intervals(observed_micros);
        self.actors
            .retain(|_, actor| actor.has_player_identity_evidence());
        for actor in self.actors.values_mut() {
            *actor = actor.identity_only();
        }
        self.encounter_id = None;
        self.encounter_state = Some("wiped".into());
        self.active_combat_started = None;
        self.first_combat_started = None;
        self.last_combat_ended = Some(observed_micros);
        self.active_combat_micros = 0;
        self.combat_windows.clear();
        self.combat_window_count = 0;
        self.last_hostile_micros = None;
        self.projected_contributions.clear();
        self.projected_rational_contributions.clear();
        self.latest_exact_contributions.clear();
        self.latest_exact_rational_contributions.clear();
        self.live_damage_influences.clear();
        self.live_damage_influences_truncated = false;
        if let Some(projector) = self.exact_contribution_projector.as_mut() {
            projector.reset();
        }
        self.live_attribution = DamageContributionReducer::new(self.contribution_rules.clone())
            .expect("previously validated rDPS rules remain valid");
        for actor_id in self.actors.keys().copied() {
            self.live_attribution.set_provider_eligible(actor_id, true);
        }
    }

    /// User-requested presentation reset. Callers must first persist a manual
    /// boundary when a run is active; this method only clears the live attempt
    /// while retaining packet-proven player identity for the next pull.
    pub fn force_reset_live_attempt(&mut self, observed_micros: u64) {
        self.reset_live_attempt(observed_micros);
    }

    /// Applies one already-filtered canonical event to the live projection.
    pub fn observe_live(&mut self, envelope: &EventEnvelope) {
        self.event_count = self.event_count.saturating_add(1);
        self.last_event_micros = Some(envelope.time.observed_micros);
        self.close_inactive_combat(envelope.time.observed_micros);
        self.project_exact_contributions(envelope);
        if let CanonicalEvent::WorldChanged(world) = &envelope.event {
            if let Some(scene_id) = world.scene_id {
                if self.scene_id.is_some_and(|previous| previous != scene_id.0)
                    && self.first_combat_started.is_some()
                    && self.run_terminal_micros.is_none()
                {
                    self.mark_run_terminal(envelope.time.observed_micros);
                }
                self.scene_id = Some(scene_id.0);
            }
            return;
        }
        if let CanonicalEvent::Dungeon(dungeon) = &envelope.event {
            if dungeon.kind == DungeonEventKind::Entered {
                self.record_run_entry(envelope.time.observed_micros);
                self.push_history_fact(CombatFact {
                    observed_micros: envelope.time.observed_micros,
                    source_actor_id: 0,
                    source_entity_uuid: 0,
                    target: None,
                    breakdown_ability_id: None,
                    ability_id: None,
                    kind: CombatFactKind::StatusReset,
                });
                self.live_attribution.reset_statuses();
            }
            if let Some(connection_id) = wire_connection_id(&envelope.provenance) {
                self.relevant_connection_ids.insert(connection_id);
            }
            if dungeon.kind == DungeonEventKind::ObjectiveUpdated
                && dungeon.objective_complete == Some(true)
            {
                // Objective completion can be an intermediate dungeon segment
                // (mobbing -> boss), not necessarily the whole run. Close and
                // bank eDPS here; only authoritative run-terminal events freeze
                // the run clock.
                self.finish_encounter(envelope.time.observed_micros);
            } else if matches!(
                dungeon.kind,
                DungeonEventKind::Completed
                    | DungeonEventKind::Failed
                    | DungeonEventKind::Ended
                    | DungeonEventKind::Exited
            ) {
                self.mark_run_terminal(envelope.time.observed_micros);
            }
            return;
        }
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            return;
        };
        match &timeline.kind {
            TimelineEventKind::EncounterBoundary {
                state,
                encounter_id,
                ..
            } => {
                self.encounter_id = encounter_id.clone().or_else(|| self.encounter_id.clone());
                self.encounter_state = Some(encounter_state_name(*state).into());
                match state {
                    EncounterState::Started => self.begin_encounter(envelope.time.observed_micros),
                    EncounterState::Wiped => self.reset_live_attempt(envelope.time.observed_micros),
                    EncounterState::Cleared | EncounterState::Ended => {
                        self.finish_encounter(envelope.time.observed_micros)
                    }
                }
            }
            TimelineEventKind::CombatBoundary { state, .. } => match state {
                CombatState::Started => self.begin_combat(envelope.time.observed_micros),
                CombatState::Ended => self.end_combat(envelope.time.observed_micros),
            },
            TimelineEventKind::Actor(actor) => {
                if matches!(
                    actor.state,
                    ActorState::Spawned | ActorState::Transformed | ActorState::Despawned
                ) {
                    self.actor_ancestry
                        .clear_owner(envelope.time.observed_micros, actor.actor);
                }
                if actor.state != ActorState::Despawned {
                    self.actor_ancestry.observe_entity(actor.actor);
                }
                let actor_id = if actor.kind == ActorKind::Player {
                    if let Some(character_id) = actor.character_id.as_deref() {
                        self.observe_character_identity(
                            actor.actor.actor_id.0,
                            character_id,
                            envelope.time.observed_micros,
                        )
                    } else {
                        self.canonical_actor_id(actor.actor.actor_id.0)
                    }
                } else {
                    self.canonical_actor_id(actor.actor.actor_id.0)
                };
                self.live_attribution.set_provider_eligible(
                    actor_id,
                    actor.state != ActorState::Despawned && actor.kind == ActorKind::Player,
                );
                {
                    // Pass the packet's raw actor ID here. `actor_mut` follows
                    // the alias to the UID-owned accumulator while preserving
                    // that accumulator's existing runtime entity UUID. Passing
                    // `actor_id` (already canonicalized) made a second entity
                    // for the same character look like short-ID reuse and
                    // incorrectly cleared the captured loadout.
                    let accumulator =
                        self.actor_mut(actor.actor.actor_id.0, actor.actor.entity_uuid.0);
                    if actor.state != ActorState::Despawned {
                        let observed_micros = envelope.time.observed_micros;
                        let identity_observed_micros = accumulator.identity_observed_micros;
                        merge_observed_option(
                            &mut accumulator.character_id,
                            actor.character_id.clone(),
                            identity_observed_micros,
                            observed_micros,
                        );
                        accumulator.monster_id = actor
                            .monster_id
                            .map(|monster_id| monster_id.0)
                            .or(accumulator.monster_id);
                        merge_observed_option(
                            &mut accumulator.display_name,
                            actor.display_name.clone(),
                            identity_observed_micros,
                            observed_micros,
                        );
                        merge_observed_option(
                            &mut accumulator.actor_kind,
                            Some(actor_kind_name(actor.kind)),
                            identity_observed_micros,
                            observed_micros,
                        );
                        merge_observed_option(
                            &mut accumulator.class_id,
                            actor.class_id,
                            identity_observed_micros,
                            observed_micros,
                        );
                        merge_observed_option(
                            &mut accumulator.specialization_id,
                            actor.specialization_id,
                            identity_observed_micros,
                            observed_micros,
                        );
                        merge_observed_option(
                            &mut accumulator.level,
                            actor.level,
                            identity_observed_micros,
                            observed_micros,
                        );
                        merge_observed_option(
                            &mut accumulator.ability_score,
                            actor.ability_score,
                            identity_observed_micros,
                            observed_micros,
                        );
                        merge_observed_option(
                            &mut accumulator.seasonal_score,
                            actor.seasonal_score,
                            identity_observed_micros,
                            observed_micros,
                        );
                        accumulator.identity_observed_micros =
                            identity_observed_micros.max(observed_micros);

                        let equipment_observed_micros = accumulator.equipment_observed_micros;
                        merge_observed_option(
                            &mut accumulator.weapon_item_id,
                            actor.weapon_item_id,
                            equipment_observed_micros,
                            observed_micros,
                        );
                        merge_observed_option(
                            &mut accumulator.weapon_breakthrough_count,
                            actor.weapon_breakthrough_count,
                            equipment_observed_micros,
                            observed_micros,
                        );
                        if actor.weapon_item_id.is_some()
                            || actor.weapon_breakthrough_count.is_some()
                        {
                            accumulator.equipment_observed_micros =
                                equipment_observed_micros.max(observed_micros);
                        }
                        if should_replace_loadout(
                            accumulator.primary_loadout_evidence,
                            accumulator.primary_loadout_observed_micros,
                            actor.loadout_observation.primary,
                            observed_micros,
                        ) {
                            accumulator
                                .primary_loadout
                                .clone_from(&actor.primary_loadout);
                            accumulator.primary_loadout_evidence =
                                actor.loadout_observation.primary;
                            accumulator.primary_loadout_observed_micros = observed_micros;
                        }
                        if should_replace_loadout(
                            accumulator.auxiliary_loadout_evidence,
                            accumulator.auxiliary_loadout_observed_micros,
                            actor.loadout_observation.auxiliary,
                            observed_micros,
                        ) {
                            accumulator
                                .auxiliary_loadout
                                .clone_from(&actor.auxiliary_loadout);
                            accumulator.auxiliary_loadout_evidence =
                                actor.loadout_observation.auxiliary;
                            accumulator.auxiliary_loadout_observed_micros = observed_micros;
                        }
                    }
                }
                if actor.state != ActorState::Despawned {
                    self.record_history_identity(envelope.time.observed_micros, actor_id);
                }
            }
            TimelineEventKind::Cast(cast) => {
                if cast.state != CastState::Started {
                    return;
                }
                self.actor_ancestry.observe_entity(cast.source);
                if let Some(target) = cast.target {
                    self.actor_ancestry.observe_entity(target);
                }
                let source = self
                    .actor_ancestry
                    .resolve_entity_at(cast.source, envelope.time.observed_micros);
                let accumulator = self.actor_mut(source.actor_id.0, source.entity_uuid.0);
                accumulator.casts = accumulator.casts.saturating_add(1);
                let ability = accumulator.abilities.entry(cast.ability.0).or_default();
                ability.casts = ability.casts.saturating_add(1);
                self.push_history_fact(CombatFact {
                    observed_micros: envelope.time.observed_micros,
                    source_actor_id: cast.source.actor_id.0,
                    source_entity_uuid: cast.source.entity_uuid.0,
                    target: cast
                        .target
                        .map(|target| (target.actor_id.0, target.entity_uuid.0)),
                    breakdown_ability_id: Some(cast.ability.0),
                    ability_id: Some(cast.ability.0),
                    kind: CombatFactKind::Cast,
                });
            }
            TimelineEventKind::Damage(damage) => {
                self.actor_ancestry
                    .observe_damage(envelope.time.observed_micros, damage);
                let source = self
                    .actor_ancestry
                    .resolve_entity_at(damage.source, envelope.time.observed_micros);
                let target = self
                    .actor_ancestry
                    .resolve_entity_at(damage.target, envelope.time.observed_micros);
                if let Some(connection_id) = wire_connection_id(&envelope.provenance) {
                    self.relevant_connection_ids.insert(connection_id);
                }
                self.begin_combat(envelope.time.observed_micros);
                self.last_hostile_micros = Some(envelope.time.observed_micros);
                let during_combat = self.active_combat_started.is_some();
                let reported = nonnegative(damage.amount);
                let effective = nonnegative(
                    damage
                        .actual_amount
                        .or(damage.hp_loss)
                        .unwrap_or(damage.amount),
                );
                let breakdown_ability_id = damage.ability.map(|ability| {
                    damage.packet.breakdown_ability_id.unwrap_or_else(|| {
                        self.ability_breakdown_resolver
                            .and_then(|resolver| {
                                resolver(ability.0, damage.hit_event_id, damage.damage_source)
                            })
                            .unwrap_or(ability.0)
                    })
                });
                {
                    let target_actor = self.actor_mut(target.actor_id.0, target.entity_uuid.0);
                    target_actor.damage_taken = target_actor.damage_taken.saturating_add(effective);
                }
                let accumulator = self.actor_mut(source.actor_id.0, source.entity_uuid.0);
                accumulator.reported_damage = accumulator.reported_damage.saturating_add(reported);
                accumulator.effective_damage =
                    accumulator.effective_damage.saturating_add(effective);
                accumulator.hp_damage = accumulator
                    .hp_damage
                    .saturating_add(nonnegative(damage.hp_loss.unwrap_or(0)));
                accumulator.shield_damage = accumulator
                    .shield_damage
                    .saturating_add(nonnegative(damage.shield_loss.unwrap_or(0)));
                if during_combat {
                    accumulator.damage_during_combat =
                        accumulator.damage_during_combat.saturating_add(reported);
                }
                accumulator.hits = accumulator.hits.saturating_add(1);
                if damage.flags.critical == Some(true) {
                    accumulator.critical_hits = accumulator.critical_hits.saturating_add(1);
                }
                if let Some(breakdown_ability_id) = breakdown_ability_id {
                    let ability = accumulator
                        .abilities
                        .entry(breakdown_ability_id)
                        .or_default();
                    ability.hits = ability.hits.saturating_add(1);
                    ability.reported_damage = ability.reported_damage.saturating_add(reported);
                    ability.effective_damage = ability.effective_damage.saturating_add(effective);
                    if damage.flags.critical == Some(true) {
                        ability.critical_hits = ability.critical_hits.saturating_add(1);
                    }
                }
                if during_combat {
                    let source_actor_id = self.canonical_actor_id(source.actor_id.0);
                    let run_damage = self
                        .run_damage_during_combat
                        .entry(source_actor_id)
                        .or_default();
                    *run_damage = run_damage.saturating_add(reported);
                }
                self.push_history_fact(CombatFact {
                    observed_micros: envelope.time.observed_micros,
                    source_actor_id: damage.source.actor_id.0,
                    source_entity_uuid: damage.source.entity_uuid.0,
                    target: Some((damage.target.actor_id.0, damage.target.entity_uuid.0)),
                    breakdown_ability_id,
                    ability_id: damage.ability.map(|ability| ability.0),
                    kind: CombatFactKind::Damage {
                        reported,
                        effective,
                        critical: damage.flags.critical == Some(true),
                    },
                });
                let source_actor_id = self.canonical_actor_id(source.actor_id.0);
                let target_actor_id = self.canonical_actor_id(target.actor_id.0);
                self.live_attribution
                    .observe_damage(ContributionDamageEvent {
                        observed_micros: envelope.time.observed_micros,
                        source_actor_id,
                        target_actor_id,
                        amount: reported,
                        included: true,
                    });
            }
            TimelineEventKind::Healing(healing) => {
                self.actor_ancestry.observe_entity(healing.target);
                self.actor_ancestry.observe_attributed_source(
                    envelope.time.observed_micros,
                    healing.source,
                    healing.direct_source,
                );
                let source = self
                    .actor_ancestry
                    .resolve_entity_at(healing.source, envelope.time.observed_micros);
                let target = self
                    .actor_ancestry
                    .resolve_entity_at(healing.target, envelope.time.observed_micros);
                self.actor_mut(target.actor_id.0, target.entity_uuid.0);
                let accumulator = self.actor_mut(source.actor_id.0, source.entity_uuid.0);
                let reported = nonnegative(healing.amount);
                let effective = nonnegative(healing.effective_amount.unwrap_or(healing.amount));
                accumulator.reported_healing =
                    accumulator.reported_healing.saturating_add(reported);
                accumulator.effective_healing =
                    accumulator.effective_healing.saturating_add(effective);
                accumulator.overheal = accumulator
                    .overheal
                    .saturating_add(nonnegative(healing.overheal.unwrap_or(0)));
                if let Some(ability_id) = healing.ability {
                    let ability = accumulator.abilities.entry(ability_id.0).or_default();
                    ability.reported_healing = ability.reported_healing.saturating_add(reported);
                    ability.effective_healing = ability.effective_healing.saturating_add(effective);
                }
                self.push_history_fact(CombatFact {
                    observed_micros: envelope.time.observed_micros,
                    source_actor_id: healing.source.actor_id.0,
                    source_entity_uuid: healing.source.entity_uuid.0,
                    target: Some((healing.target.actor_id.0, healing.target.entity_uuid.0)),
                    breakdown_ability_id: healing.ability.map(|ability| ability.0),
                    ability_id: healing.ability.map(|ability| ability.0),
                    kind: CombatFactKind::Healing {
                        reported,
                        effective,
                    },
                });
            }
            TimelineEventKind::Shield(shield) => {
                self.actor_ancestry.observe_entity(shield.source);
                self.actor_ancestry.observe_entity(shield.target);
                let source = self
                    .actor_ancestry
                    .resolve_entity_at(shield.source, envelope.time.observed_micros);
                let target = self
                    .actor_ancestry
                    .resolve_entity_at(shield.target, envelope.time.observed_micros);
                self.actor_mut(target.actor_id.0, target.entity_uuid.0);
                let accumulator = self.actor_mut(source.actor_id.0, source.entity_uuid.0);
                let amount = nonnegative(shield.amount);
                accumulator.shielding = accumulator.shielding.saturating_add(amount);
                let ability = accumulator.abilities.entry(shield.ability.0).or_default();
                ability.shielding = ability.shielding.saturating_add(amount);
                self.push_history_fact(CombatFact {
                    observed_micros: envelope.time.observed_micros,
                    source_actor_id: shield.source.actor_id.0,
                    source_entity_uuid: shield.source.entity_uuid.0,
                    target: Some((shield.target.actor_id.0, shield.target.entity_uuid.0)),
                    breakdown_ability_id: Some(shield.ability.0),
                    ability_id: Some(shield.ability.0),
                    kind: CombatFactKind::Shield { amount },
                });
            }
            TimelineEventKind::Life { actor, state } => {
                self.actor_ancestry.observe_entity(*actor);
                let actor = self
                    .actor_ancestry
                    .resolve_entity_at(*actor, envelope.time.observed_micros);
                let accumulator = self.actor_mut(actor.actor_id.0, actor.entity_uuid.0);
                match state {
                    LifeState::Died => accumulator.deaths = accumulator.deaths.saturating_add(1),
                    LifeState::Revived => {
                        accumulator.revives = accumulator.revives.saturating_add(1)
                    }
                }
                self.push_history_fact(CombatFact {
                    observed_micros: envelope.time.observed_micros,
                    source_actor_id: actor.actor_id.0,
                    source_entity_uuid: actor.entity_uuid.0,
                    target: None,
                    breakdown_ability_id: None,
                    ability_id: None,
                    kind: CombatFactKind::Life { state: *state },
                });
            }
            TimelineEventKind::Position(position) => {
                let accumulator =
                    self.actor_mut(position.actor.actor_id.0, position.actor.entity_uuid.0);
                accumulator.position_samples = accumulator.position_samples.saturating_add(1);
                if let Some((x, y, z)) = accumulator.last_position {
                    let dx = f64::from(position.x - x);
                    let dy = f64::from(position.y - y);
                    let dz = f64::from(position.z - z);
                    accumulator.path_distance += (dx * dx + dy * dy + dz * dz).sqrt();
                }
                accumulator.last_position = Some((position.x, position.y, position.z));
            }
            TimelineEventKind::DataGap(gap) => {
                if gap.connection_id.is_none()
                    || gap
                        .connection_id
                        .is_some_and(|id| self.relevant_connection_ids.contains(&id))
                {
                    self.data_gap_count = self.data_gap_count.saturating_add(1);
                }
            }
            TimelineEventKind::RunBoundary {
                state, scene_id, ..
            } => {
                if scene_id.is_some() {
                    self.scene_id = scene_id.map(|scene_id| scene_id.0);
                }
                if *state == RunState::Entered {
                    self.record_run_entry(envelope.time.observed_micros);
                    self.push_history_fact(CombatFact {
                        observed_micros: envelope.time.observed_micros,
                        source_actor_id: 0,
                        source_entity_uuid: 0,
                        target: None,
                        breakdown_ability_id: None,
                        ability_id: None,
                        kind: CombatFactKind::StatusReset,
                    });
                    self.live_attribution.reset_statuses();
                }
                if matches!(
                    state,
                    RunState::Completed | RunState::Failed | RunState::Ended | RunState::Exited
                ) {
                    self.mark_run_terminal(envelope.time.observed_micros);
                }
            }
            TimelineEventKind::EntityAttributes(attributes) => {
                self.actor_ancestry.observe_entity(attributes.actor);
                match attributes.ownership {
                    Some(rlogs_events::ActorOwnershipUpdate::Confirmed { owner_entity_uuid }) => {
                        self.actor_ancestry.observe_owner_entity(
                            envelope.time.observed_micros,
                            attributes.actor,
                            owner_entity_uuid.0,
                            ActorOwnershipEvidence::ConfirmedEntityAttributes,
                        )
                    }
                    Some(rlogs_events::ActorOwnershipUpdate::Cleared) => self
                        .actor_ancestry
                        .clear_owner(envelope.time.observed_micros, attributes.actor),
                    None => {}
                }
                if let Some(mapping) = self.health_attributes {
                    let actor =
                        self.actor_mut(attributes.actor.actor_id.0, attributes.actor.entity_uuid.0);
                    for attribute in &attributes.attributes {
                        let Some(EntityAttributeValue::Integer(value)) = &attribute.decoded else {
                            continue;
                        };
                        if attribute.attribute_id == mapping.current_hp {
                            actor.current_hp = Some((*value).max(0));
                        } else if attribute.attribute_id == mapping.max_hp {
                            actor.max_hp = Some((*value).max(0));
                        }
                    }
                }
            }
            TimelineEventKind::TemporaryAttributes(_)
            | TimelineEventKind::Cooldown(_)
            | TimelineEventKind::Resource(_)
            | TimelineEventKind::RecorderPause(_) => {}
            TimelineEventKind::Status(status) => {
                self.actor_ancestry.observe_entity(status.target);
                if let Some(source) = status.source {
                    self.actor_ancestry.observe_entity(source);
                }
                let target = self
                    .actor_ancestry
                    .resolve_entity_at(status.target, envelope.time.observed_micros);
                let source = self.actor_ancestry.resolve_entity_at(
                    status.source.unwrap_or(status.target),
                    envelope.time.observed_micros,
                );
                self.actor_mut(target.actor_id.0, target.entity_uuid.0);
                self.actor_mut(source.actor_id.0, source.entity_uuid.0);
                let source_actor_id = self.canonical_actor_id(source.actor_id.0);
                let target_actor_id = self.canonical_actor_id(target.actor_id.0);
                self.live_attribution
                    .observe_status(ContributionStatusEvent {
                        observed_micros: envelope.time.observed_micros,
                        source_actor_id: status.source.map(|_| source_actor_id),
                        target_actor_id,
                        effect_id: status.effect.0,
                        instance_id: status.instance_id.map(|instance| instance.0),
                        state: contribution_status_state(status.state),
                        stacks: status.stacks,
                        duration_millis: status.duration_millis,
                    });
                self.push_history_fact(CombatFact {
                    observed_micros: envelope.time.observed_micros,
                    source_actor_id: source.actor_id.0,
                    source_entity_uuid: source.entity_uuid.0,
                    target: Some((target.actor_id.0, target.entity_uuid.0)),
                    breakdown_ability_id: None,
                    ability_id: None,
                    kind: CombatFactKind::Status {
                        effect_id: status.effect.0,
                        attribution_source_actor_id: status.source.map(|_| source.actor_id.0),
                        instance_id: status.instance_id.map(|instance| instance.0),
                        state: status.state,
                        stacks: status.stacks,
                        duration_millis: status.duration_millis,
                    },
                });
            }
            TimelineEventKind::UnresolvedStatus(status) => {
                // Preserve actor identity without fabricating an effect ID or
                // feeding an unresolved lifecycle into provider attribution.
                self.actor_mut(status.target.actor_id.0, status.target.entity_uuid.0);
                if let Some(source) = status.source {
                    self.actor_mut(source.actor_id.0, source.entity_uuid.0);
                }
            }
            TimelineEventKind::UnresolvedAction(action) => {
                // Preserve exact wire participants without treating an
                // unresolved action/table identity as a cast or damage event.
                // This must not alter ordinary totals or provider attribution.
                if let Some(container) = action.container {
                    self.actor_mut(container.actor_id.0, container.entity_uuid.0);
                }
                if let Some(target) = action.target {
                    self.actor_mut(target.actor_id.0, target.entity_uuid.0);
                }
            }
        }
    }

    /// Exact integer contribution terms emitted while observing the latest
    /// canonical event. The slice is replaced on the next `observe_live` call.
    pub fn latest_exact_contributions(&self) -> &[ExactDamageContributionEvent] {
        &self.latest_exact_contributions
    }

    /// Exact rational contribution terms emitted while observing the latest
    /// canonical event. The slice is replaced on the next `observe_live` call.
    pub fn latest_exact_rational_contributions(&self) -> &[ExactRationalDamageContributionEvent] {
        &self.latest_exact_rational_contributions
    }

    /// Reports the game projector's current formula authority/readiness state.
    /// Capture and raw combat totals remain available for provisional builds.
    pub fn damage_contribution_status(&self) -> String {
        self.rdps_status()
    }

    /// Number of compact facts retained for the current reviewed run. This is
    /// diagnostic metadata only; callers must not use it to change combat
    /// semantics or discard exact history.
    pub fn retained_history_fact_count(&self) -> usize {
        self.history_facts.len()
    }

    fn project_exact_contributions(&mut self, envelope: &EventEnvelope) {
        self.latest_exact_contributions.clear();
        self.latest_exact_rational_contributions.clear();
        let damage_context = match &envelope.event {
            CanonicalEvent::Timeline(timeline) => match &timeline.kind {
                TimelineEventKind::Damage(damage) => Some(DamageProjectionContext {
                    event_sequence: envelope.sequence,
                    recipient_actor_id: self.canonical_actor_id(damage.source.actor_id.0),
                    recipient_entity_uuid: damage.source.entity_uuid.0,
                    affected_ability_id: damage.ability.map(|ability| ability.0),
                    target_actor_id: self.canonical_actor_id(damage.target.actor_id.0),
                    target_entity_uuid: damage.target.entity_uuid.0,
                    critical: damage.flags.critical,
                }),
                _ => None,
            },
            _ => None,
        };
        self.projected_contributions.clear();
        self.projected_rational_contributions.clear();
        {
            let Some(projector) = self.exact_contribution_projector.as_mut() else {
                return;
            };
            projector.observe(
                envelope,
                &mut self.projected_contributions,
                &mut self.projected_rational_contributions,
            );
        }
        self.consume_projected_contributions(damage_context);
    }

    fn finish_exact_contributions(&mut self) {
        self.latest_exact_contributions.clear();
        self.latest_exact_rational_contributions.clear();
        self.projected_contributions.clear();
        self.projected_rational_contributions.clear();
        {
            let Some(projector) = self.exact_contribution_projector.as_mut() else {
                return;
            };
            projector.finish(
                &mut self.projected_contributions,
                &mut self.projected_rational_contributions,
            );
        }
        self.consume_projected_contributions(None);
    }

    fn consume_projected_contributions(&mut self, damage_context: Option<DamageProjectionContext>) {
        let mut projected = std::mem::take(&mut self.projected_contributions);
        for mut contribution in projected.iter().copied() {
            contribution.provider_actor_id =
                self.canonical_actor_id(contribution.provider_actor_id);
            contribution.recipient_actor_id =
                self.canonical_actor_id(contribution.recipient_actor_id);
            self.latest_exact_contributions.push(contribution);
            self.live_attribution
                .observe_exact_contribution(contribution);
            let provider_entity_uuid = self
                .actors
                .get(&contribution.provider_actor_id)
                .map_or(0, |actor| actor.entity_uuid);
            let recipient_entity_uuid = self
                .actors
                .get(&contribution.recipient_actor_id)
                .map_or_else(
                    || {
                        damage_context
                            .filter(|context| {
                                context.recipient_actor_id == contribution.recipient_actor_id
                            })
                            .map_or(0, |context| context.recipient_entity_uuid)
                    },
                    |actor| actor.entity_uuid,
                );
            self.observe_live_damage_influence(HistoryDamageInfluenceObservation {
                observed_micros: contribution.observed_micros,
                effect_id: contribution.effect_id,
                scope: contribution.scope,
                provider_actor_id: contribution.provider_actor_id,
                provider_entity_uuid,
                recipient_actor_id: contribution.recipient_actor_id,
                recipient_entity_uuid,
                damage_event_sequence: damage_context.map(|context| context.event_sequence),
                affected_ability_id: damage_context.and_then(|context| context.affected_ability_id),
                affected_target: damage_context
                    .map(|context| (context.target_actor_id, context.target_entity_uuid)),
                critical: damage_context.and_then(|context| context.critical),
                observed_damage: contribution.observed_damage,
                exact_integer_delta: Some(contribution.amount),
                exact_rational_delta: None,
            });
            self.push_history_fact(CombatFact {
                observed_micros: contribution.observed_micros,
                source_actor_id: contribution.provider_actor_id,
                source_entity_uuid: provider_entity_uuid,
                target: Some((contribution.recipient_actor_id, recipient_entity_uuid)),
                breakdown_ability_id: None,
                ability_id: None,
                kind: CombatFactKind::ExactDamageContribution {
                    effect_id: contribution.effect_id,
                    scope: contribution.scope,
                    provider_actor_id: contribution.provider_actor_id,
                    recipient_actor_id: contribution.recipient_actor_id,
                    amount: contribution.amount,
                    observed_damage: contribution.observed_damage,
                    damage_event_sequence: damage_context.map(|context| context.event_sequence),
                    affected_ability_id: damage_context
                        .and_then(|context| context.affected_ability_id),
                    affected_target: damage_context
                        .map(|context| (context.target_actor_id, context.target_entity_uuid)),
                    critical: damage_context.and_then(|context| context.critical),
                },
            });
        }
        projected.clear();
        self.projected_contributions = projected;

        let mut rational = std::mem::take(&mut self.projected_rational_contributions);
        for mut contribution in rational.iter().copied() {
            contribution.provider_actor_id =
                self.canonical_actor_id(contribution.provider_actor_id);
            contribution.recipient_actor_id =
                self.canonical_actor_id(contribution.recipient_actor_id);
            let contribution_damage_context = contribution
                .deferred_damage_context
                .map(|context| DamageProjectionContext {
                    event_sequence: context.event_sequence,
                    recipient_actor_id: contribution.recipient_actor_id,
                    recipient_entity_uuid: context.recipient_entity_uuid,
                    affected_ability_id: context.affected_ability_id,
                    target_actor_id: self.canonical_actor_id(context.target_actor_id),
                    target_entity_uuid: context.target_entity_uuid,
                    critical: None,
                })
                .or(damage_context);
            self.latest_exact_rational_contributions.push(contribution);
            self.live_attribution
                .observe_exact_rational_contribution(contribution);
            let provider_entity_uuid = self
                .actors
                .get(&contribution.provider_actor_id)
                .map_or(0, |actor| actor.entity_uuid);
            let recipient_entity_uuid = self
                .actors
                .get(&contribution.recipient_actor_id)
                .map_or_else(
                    || {
                        contribution_damage_context
                            .filter(|context| {
                                context.recipient_actor_id == contribution.recipient_actor_id
                            })
                            .map_or(0, |context| context.recipient_entity_uuid)
                    },
                    |actor| actor.entity_uuid,
                );
            self.observe_live_damage_influence(HistoryDamageInfluenceObservation {
                observed_micros: contribution.observed_micros,
                effect_id: contribution.effect_id,
                scope: contribution.scope,
                provider_actor_id: contribution.provider_actor_id,
                provider_entity_uuid,
                recipient_actor_id: contribution.recipient_actor_id,
                recipient_entity_uuid,
                damage_event_sequence: contribution_damage_context
                    .map(|context| context.event_sequence),
                affected_ability_id: contribution_damage_context
                    .and_then(|context| context.affected_ability_id),
                affected_target: contribution_damage_context
                    .map(|context| (context.target_actor_id, context.target_entity_uuid)),
                critical: contribution_damage_context.and_then(|context| context.critical),
                observed_damage: contribution.observed_damage,
                exact_integer_delta: None,
                exact_rational_delta: Some((contribution.numerator, contribution.denominator)),
            });
            self.push_history_fact(CombatFact {
                observed_micros: contribution.observed_micros,
                source_actor_id: contribution.provider_actor_id,
                source_entity_uuid: provider_entity_uuid,
                target: Some((contribution.recipient_actor_id, recipient_entity_uuid)),
                breakdown_ability_id: None,
                ability_id: None,
                kind: CombatFactKind::ExactRationalDamageContribution {
                    effect_id: contribution.effect_id,
                    scope: contribution.scope,
                    provider_actor_id: contribution.provider_actor_id,
                    recipient_actor_id: contribution.recipient_actor_id,
                    numerator: contribution.numerator,
                    denominator: contribution.denominator,
                    observed_damage: contribution.observed_damage,
                    damage_event_sequence: contribution_damage_context
                        .map(|context| context.event_sequence),
                    affected_ability_id: contribution_damage_context
                        .and_then(|context| context.affected_ability_id),
                    affected_target: contribution_damage_context
                        .map(|context| (context.target_actor_id, context.target_entity_uuid)),
                    critical: contribution_damage_context.and_then(|context| context.critical),
                },
            });
        }
        rational.clear();
        self.projected_rational_contributions = rational;
    }

    fn observe_live_damage_influence(&mut self, observation: HistoryDamageInfluenceObservation) {
        let key = history_damage_influence_key(observation);
        if !self.live_damage_influences.contains_key(&key)
            && self.live_damage_influences.len() >= MAXIMUM_LIVE_RDPS_INFLUENCE_RELATIONSHIPS
        {
            self.live_damage_influences_truncated = true;
            return;
        }
        observe_history_damage_influence(&mut self.live_damage_influences, observation);
    }

    fn rdps_enabled(&self) -> bool {
        !self.contribution_rules.is_empty()
            || self
                .exact_contribution_projector
                .as_ref()
                .is_some_and(|projector| projector.enabled())
    }

    fn rdps_status(&self) -> String {
        if !self.contribution_rules.is_empty() {
            return "partial_packet_proven_rules".into();
        }
        self.exact_contribution_projector.as_ref().map_or_else(
            || "pending_reviewed_effect_rules".into(),
            |projector| projector.status(),
        )
    }

    fn actor_mut(&mut self, actor_id: u64, entity_uuid: i64) -> &mut ActorAccumulator {
        let canonical_actor_id = self.canonical_actor_id(actor_id);
        let canonical_entity_uuid = if canonical_actor_id != actor_id {
            self.actors
                .get(&canonical_actor_id)
                .map(|actor| actor.entity_uuid)
                .filter(|entity_uuid| *entity_uuid != 0)
                .unwrap_or(entity_uuid)
        } else {
            entity_uuid
        };
        let actor = self.actors.entry(canonical_actor_id).or_default();
        if actor.entity_uuid != 0
            && canonical_entity_uuid != 0
            && actor.entity_uuid != canonical_entity_uuid
        {
            // Short actor IDs are reused. A new entity must never inherit the
            // previous entity's name, class, loadout, health, or totals.
            *actor = ActorAccumulator::default();
        }
        actor.entity_uuid = canonical_entity_uuid;
        actor
    }

    fn canonical_actor_id(&self, actor_id: u64) -> u64 {
        let mut current = actor_id;
        for _ in 0..64 {
            let Some(next) = self.actor_aliases.get(&current).copied() else {
                break;
            };
            if next == current {
                break;
            }
            current = next;
        }
        current
    }

    fn observe_character_identity(
        &mut self,
        raw_actor_id: u64,
        character_id: &str,
        observed_micros: u64,
    ) -> u64 {
        if let Some(previous) = self
            .actors
            .get(&self.canonical_actor_id(raw_actor_id))
            .and_then(|actor| actor.character_id.as_deref())
        {
            if previous != character_id {
                self.actor_aliases.remove(&raw_actor_id);
            }
        }
        let raw_canonical = self.canonical_actor_id(raw_actor_id);
        let canonical = self
            .character_actors
            .get(character_id)
            .copied()
            .map(|actor_id| self.canonical_actor_id(actor_id))
            .unwrap_or(raw_canonical);
        self.character_actors
            .insert(character_id.to_owned(), canonical);
        if raw_canonical != canonical {
            self.actor_aliases.insert(raw_canonical, canonical);
            self.actor_aliases.insert(raw_actor_id, canonical);
            for alias in self
                .actor_aliases
                .iter()
                .filter_map(|(alias, target)| (*target == raw_canonical).then_some(*alias))
                .collect::<Vec<_>>()
            {
                self.actor_aliases.insert(alias, canonical);
            }
            if let Some(other) = self.actors.remove(&raw_canonical) {
                self.actors.entry(canonical).or_default().merge_from(other);
            }
            if let Some(other_damage) = self.run_damage_during_combat.remove(&raw_canonical) {
                let canonical_damage = self.run_damage_during_combat.entry(canonical).or_default();
                *canonical_damage = canonical_damage.saturating_add(other_damage);
            }
            if let Some(mut versions) = self.history_identities.remove(&raw_canonical) {
                let canonical_versions = self.history_identities.entry(canonical).or_default();
                canonical_versions.append(&mut versions);
                canonical_versions.sort_by_key(|version| version.observed_micros);
                canonical_versions.dedup_by(|right, left| right.identity == left.identity);
            }
            self.live_attribution.remap_actor(raw_canonical, canonical);
        }
        let actor = self.actors.entry(canonical).or_default();
        let identity_observed_micros = actor.identity_observed_micros;
        merge_observed_option(
            &mut actor.character_id,
            Some(character_id.to_owned()),
            identity_observed_micros,
            observed_micros,
        );
        actor.identity_observed_micros = identity_observed_micros.max(observed_micros);
        self.record_history_identity(observed_micros, canonical);
        canonical
    }

    fn record_history_identity(&mut self, observed_micros: u64, actor_id: u64) {
        let actor_id = self.canonical_actor_id(actor_id);
        let Some(actor) = self.actors.get(&actor_id) else {
            return;
        };
        let identity = HistoryActorIdentitySnapshot::from(actor);
        let versions = self.history_identities.entry(actor_id).or_default();
        if versions
            .last()
            .is_some_and(|version| version.identity == identity)
        {
            return;
        }
        versions.push(HistoryActorIdentityVersion {
            observed_micros,
            identity,
        });
    }

    fn history_identity_at(
        &self,
        actor_id: u64,
        entity_uuid: i64,
        last_selected_micros: u64,
    ) -> Option<&HistoryActorIdentitySnapshot> {
        let actor_id = self.canonical_actor_id(actor_id);
        self.history_identities
            .get(&actor_id)?
            .iter()
            .rev()
            .find_map(|version| {
                (version.observed_micros <= last_selected_micros
                    && (entity_uuid == 0
                        || version.identity.entity_uuid == entity_uuid
                        || version.identity.character_id.is_some()))
                .then_some(&version.identity)
            })
    }

    fn begin_combat(&mut self, observed_micros: u64) {
        if self.active_combat_started.is_none() {
            if self.encounter_terminal_micros.is_some() {
                self.begin_encounter(observed_micros);
            }
            self.active_combat_started = Some(observed_micros);
            self.first_combat_started.get_or_insert(observed_micros);
            self.encounter_combat_started.get_or_insert(observed_micros);
            self.combat_window_count = self.combat_window_count.saturating_add(1);
        }
    }

    fn end_combat(&mut self, observed_micros: u64) {
        if let Some(started) = self.active_combat_started.take() {
            if observed_micros > started {
                self.combat_windows.push((started, observed_micros));
            }
            self.active_combat_micros = self
                .active_combat_micros
                .saturating_add(observed_micros.saturating_sub(started));
            self.last_combat_ended = Some(observed_micros);
        }
        self.last_hostile_micros = None;
    }

    fn close_inactive_combat(&mut self, observed_micros: u64) {
        let Some(last_hostile_micros) = self.last_hostile_micros else {
            return;
        };
        let timeout_at = last_hostile_micros.saturating_add(COMBAT_INACTIVITY_TIMEOUT_MICROS);
        if observed_micros >= timeout_at {
            self.end_combat(timeout_at);
        }
    }

    pub fn live_snapshot(&self) -> Result<CombatTimelineSnapshot, PluginFailure> {
        self.snapshot(false)
    }

    /// Produces the compact projection used by the real-time overlay.
    ///
    /// Position-only and otherwise inactive entities remain in the native
    /// reducer for history accuracy, but are not cloned or transferred to the
    /// overlay until they contribute a combat-relevant action.
    pub fn live_overlay_snapshot(&self) -> Result<CombatTimelineSnapshot, PluginFailure> {
        self.snapshot(true)
    }

    fn snapshot(&self, active_actors_only: bool) -> Result<CombatTimelineSnapshot, PluginFailure> {
        let header = self
            .header
            .as_ref()
            .ok_or_else(|| PluginFailure::Message("combat plug-in was not initialized".into()))?;
        let duration = self.active_combat_micros.saturating_add(
            self.active_combat_started
                .zip(self.last_event_micros)
                .map_or(0, |(started, latest)| latest.saturating_sub(started)),
        );
        // The first hostile event opens combat at its own timestamp, so the
        // exact elapsed duration is still zero for that snapshot. Keep the
        // clock exact, but use the same one-second minimum as history rates so
        // the first positive damage event immediately produces a useful aDPS.
        let rate_duration = if self.first_combat_started.is_some() {
            duration.max(MINIMUM_PERSONAL_ACTIVE_MICROS)
        } else {
            0
        };
        let encounter_duration = self.encounter_elapsed_micros.saturating_add(
            self.encounter_combat_started
                .zip(self.last_event_micros)
                .map_or(0, |(started, ended)| ended.saturating_sub(started)),
        );
        let encounter_rate_duration = if encounter_duration > 0 {
            encounter_duration.max(MINIMUM_PERSONAL_ACTIVE_MICROS)
        } else {
            0
        };
        let attempt_duration = self
            .first_combat_started
            .zip(self.encounter_terminal_micros.or(self.last_event_micros))
            .map_or(0, |(started, ended)| ended.saturating_sub(started));
        let attempt_rate_duration = if self.first_combat_started.is_some() {
            attempt_duration.max(MINIMUM_PERSONAL_ACTIVE_MICROS)
        } else {
            0
        };
        let contribution_summary = self.live_attribution.summary();
        let mut rdps_damage_influences = finish_history_damage_influences(
            self.live_damage_influences.clone(),
            &contribution_summary.rational_effect_projections,
        );
        if self.live_damage_influences_truncated {
            for influence in &mut rdps_damage_influences {
                if !influence.exact_rational_deltas.is_empty() {
                    influence.attributed_rdps = None;
                }
            }
        }
        let rdps_enabled = self.rdps_enabled();
        let incomplete_rdps_actor_ids = self
            .exact_contribution_projector
            .as_ref()
            .map_or_else(Vec::new, |projector| projector.incomplete_rdps_actor_ids())
            .into_iter()
            .collect::<BTreeSet<_>>();
        let actors = self
            .actors
            .iter()
            .filter(|(actor_id, actor)| {
                !active_actors_only
                    || has_live_meter_activity(actor)
                    || contribution_summary
                        .actors
                        .get(actor_id)
                        .is_some_and(|contribution| {
                            contribution.contribution_given != 0
                                || contribution.contribution_received != 0
                        })
            })
            .map(|(actor_id, actor)| {
                let run_damage = self
                    .run_damage_during_combat
                    .get(actor_id)
                    .copied()
                    .unwrap_or(actor.damage_during_combat);
                let contribution = contribution_summary.actors.get(actor_id);
                let rdps_incomplete = incomplete_rdps_actor_ids.contains(actor_id);
                ActorCombatSummary {
                    actor_id: actor_id.to_string(),
                    entity_uuid: actor.entity_uuid.to_string(),
                    character_id: actor.character_id.clone(),
                    display_name: actor.display_name.clone(),
                    actor_kind: actor.actor_kind.clone(),
                    monster_id: actor.monster_id,
                    current_hp: actor.current_hp,
                    max_hp: actor.max_hp,
                    class_id: actor.class_id,
                    specialization_id: actor.specialization_id,
                    level: actor.level,
                    ability_score: actor.ability_score,
                    weapon_item_id: actor.weapon_item_id,
                    weapon_breakthrough_count: actor.weapon_breakthrough_count,
                    seasonal_score: actor.seasonal_score,
                    primary_loadout: actor.primary_loadout.clone(),
                    auxiliary_loadout: actor.auxiliary_loadout.clone(),
                    reported_damage: actor.reported_damage,
                    effective_damage: actor.effective_damage,
                    hp_damage: actor.hp_damage,
                    shield_damage: actor.shield_damage,
                    damage_during_combat: actor.damage_during_combat,
                    damage_taken: actor.damage_taken,
                    dps: if rate_duration == 0 {
                        0.0
                    } else {
                        actor.damage_during_combat as f64 * 1_000_000.0 / rate_duration as f64
                    },
                    run_dps: if attempt_rate_duration == 0 {
                        0.0
                    } else {
                        actor.damage_during_combat as f64 * 1_000_000.0
                            / attempt_rate_duration as f64
                    },
                    encounter_dps: if encounter_rate_duration == 0 {
                        0.0
                    } else {
                        run_damage as f64 * 1_000_000.0 / encounter_rate_duration as f64
                    },
                    active_dps: if rate_duration == 0 {
                        0.0
                    } else {
                        actor.damage_during_combat as f64 * 1_000_000.0 / rate_duration as f64
                    },
                    hps: if rate_duration == 0 {
                        0.0
                    } else {
                        actor.reported_healing as f64 * 1_000_000.0 / rate_duration as f64
                    },
                    tps: if rate_duration == 0 {
                        0.0
                    } else {
                        actor.damage_taken as f64 * 1_000_000.0 / rate_duration as f64
                    },
                    rdps_damage: rdps_enabled.then(|| {
                        contribution.map_or(actor.reported_damage, |actor| actor.rdps_damage)
                    }),
                    rdps: rdps_enabled.then(|| {
                        if rate_duration == 0 {
                            0.0
                        } else {
                            contribution.map_or(actor.reported_damage, |actor| actor.rdps_damage)
                                as f64
                                * 1_000_000.0
                                / rate_duration as f64
                        }
                    }),
                    rdps_contribution_given: rdps_enabled
                        .then(|| contribution.map_or(0, |actor| actor.contribution_given)),
                    rdps_contribution_received: rdps_enabled
                        .then(|| contribution.map_or(0, |actor| actor.contribution_received)),
                    rdps_incomplete,
                    reported_healing: actor.reported_healing,
                    effective_healing: actor.effective_healing,
                    overheal: actor.overheal,
                    shielding: actor.shielding,
                    casts: actor.casts,
                    hits: actor.hits,
                    critical_hits: actor.critical_hits,
                    deaths: actor.deaths,
                    revives: actor.revives,
                    position_samples: actor.position_samples,
                    path_distance: actor.path_distance,
                    abilities: actor
                        .abilities
                        .iter()
                        .map(|(ability_id, ability)| AbilityCombatSummary {
                            ability_id: ability_id.to_string(),
                            casts: ability.casts,
                            hits: ability.hits,
                            critical_hits: ability.critical_hits,
                            reported_damage: ability.reported_damage,
                            effective_damage: ability.effective_damage,
                            reported_healing: ability.reported_healing,
                            effective_healing: ability.effective_healing,
                            shielding: ability.shielding,
                        })
                        .collect(),
                }
            })
            .collect();
        Ok(CombatTimelineSnapshot {
            schema_version: COMBAT_SNAPSHOT_SCHEMA_VERSION,
            session_id: header.session_id.clone(),
            deployment_id: header.region.identity.deployment_id.clone(),
            region_id: header.region.identity.region_id.clone(),
            world_id: header.region.identity.world_id.clone(),
            client_build: header.region.client_build.clone(),
            protocol_pack_digest: header.region.protocol_pack_digest.clone(),
            rdps_status: self.rdps_status(),
            encounter_id: self.encounter_id.clone(),
            encounter_state: self.encounter_state.clone(),
            scene_id: self.scene_id,
            event_count: self.event_count,
            data_gap_count: self.data_gap_count,
            combat_window_count: self.combat_window_count,
            combat_active: self.active_combat_started.is_some(),
            last_hostile_micros: self.last_hostile_micros,
            latest_event_micros: self.last_event_micros,
            combat_inactivity_timeout_micros: COMBAT_INACTIVITY_TIMEOUT_MICROS,
            combat_started_micros: self.first_combat_started,
            combat_ended_micros: self.last_combat_ended,
            active_combat_micros: duration,
            attempt_elapsed_micros: self.first_combat_started.map(|_| attempt_duration),
            encounter_elapsed_micros: (encounter_duration > 0).then_some(encounter_duration),
            encounter_terminal_micros: self.encounter_terminal_micros,
            run_terminal_micros: self.run_terminal_micros,
            run_elapsed_micros: self
                .run_entered_micros
                .last()
                .copied()
                .zip(self.run_terminal_micros.or(self.last_event_micros))
                .map(|(entered, latest)| latest.saturating_sub(entered)),
            game_time_micros: None,
            true_time_micros: None,
            closed_at_log_end: self.closed_at_log_end,
            rdps_damage_influences,
            rdps_damage_influences_truncated: self.live_damage_influences_truncated,
            rdps_effect_presentations: Vec::new(),
            actors,
        })
    }

    /// Freezes the filterable history cube from compact combat facts already
    /// observed by the live reducer. No sealed-log replay is required.
    pub fn history_snapshot(
        &self,
        runs: &[RunAnalysis],
    ) -> Result<CombatHistorySnapshot, PluginFailure> {
        let header = self
            .header
            .as_ref()
            .ok_or_else(|| PluginFailure::Message("combat plug-in was not initialized".into()))?;
        let runs = runs
            .iter()
            .enumerate()
            .map(|(index, run)| self.build_run_history(index, run))
            .collect();
        Ok(CombatHistorySnapshot {
            schema_version: COMBAT_HISTORY_SCHEMA_VERSION,
            session_id: header.session_id.clone(),
            deployment_id: header.region.identity.deployment_id.clone(),
            region_id: header.region.identity.region_id.clone(),
            world_id: header.region.identity.world_id.clone(),
            client_build: header.region.client_build.clone(),
            protocol_pack_digest: header.region.protocol_pack_digest.clone(),
            rdps_formula_identity: self
                .exact_contribution_projector
                .as_ref()
                .and_then(|projector| projector.formula_identity())
                .map(str::to_owned),
            runs,
        })
    }

    fn build_run_history(&self, run_index: usize, run: &RunAnalysis) -> CombatRunHistory {
        let (mut history, specs) = self.build_run_history_definition(run_index, run, true);
        history.views = specs
            .iter()
            .map(|spec| self.build_history_view(spec))
            .collect();
        history
    }

    /// Refreshes clocks and run/attempt metadata without rebuilding actor,
    /// target, ability, or rDPS history from every retained combat fact.
    ///
    /// Returns `false` when the run's view structure changed and the caller
    /// must take a new full history snapshot. This keeps live presentation
    /// refreshes bounded while preserving exact projections at phase and run
    /// boundaries.
    pub fn try_refresh_run_history_metadata(
        &self,
        history: &mut CombatRunHistory,
        run_index: usize,
        run: &RunAnalysis,
    ) -> bool {
        let (mut refreshed, specs) = self.build_run_history_definition(run_index, run, false);
        let same_structure = history.views.len() == specs.len()
            && history.views.iter().zip(&specs).all(|(view, spec)| {
                view.id == spec.id
                    && view.kind == spec.kind
                    && view.segment_indices == spec.segment_indices
            });
        if !same_structure {
            return false;
        }

        refreshed.presentation_scene_name = history.presentation_scene_name.clone();
        refreshed.views = std::mem::take(&mut history.views);
        for (view, spec) in refreshed.views.iter_mut().zip(specs) {
            view.label = spec.label;
            view.elapsed_micros = spec.elapsed_micros;
            view.active_combat_micros = spec.active_combat_micros;
        }
        *history = refreshed;
        true
    }

    fn build_run_history_definition(
        &self,
        run_index: usize,
        run: &RunAnalysis,
        infer_minimum_active_time_from_facts: bool,
    ) -> (CombatRunHistory, Vec<HistoryViewSpec>) {
        let ended_micros = run.timing.ended_micros;
        let first_combat_micros = run
            .encounters
            .iter()
            .flat_map(|encounter| encounter.combat_windows.iter())
            .map(|window| window.started_micros)
            .min()
            .or(self.first_combat_started);
        let entered_micros = self.run_entry_for(run.timing.started_micros);
        let started_micros = run.timing.started_micros;

        let segment_intervals = run
            .segments
            .iter()
            .flat_map(|segment| history_segment_edps_intervals(run, segment))
            .collect::<Vec<_>>();
        let pruned_game_time_micros = segment_intervals
            .iter()
            .map(|(started, ended)| ended.saturating_sub(*started))
            .sum::<u64>();
        // Live encounter snapshots close their currently open segment at the
        // latest observed packet before this projection is built. Publishing
        // the reviewed sum while the run is still open lets Game time pause
        // as soon as mobbing closes, then resume only when the boss segment
        // actually opens. Waiting for `ended_micros` made the desktop fall
        // back to the continuously growing run clock during that transition.
        let reviewed_game_time_micros = if run.segments.is_empty() {
            run.timing.wall_time_micros.unwrap_or_else(|| {
                run.timing
                    .observed_until_micros
                    .saturating_sub(run.timing.started_micros)
            })
        } else {
            pruned_game_time_micros
        };
        let all_intervals = if segment_intervals.is_empty() {
            vec![(
                run.timing.started_micros,
                ended_micros.unwrap_or(run.timing.observed_until_micros),
            )]
        } else {
            segment_intervals
        };

        let all_segments = run
            .segments
            .iter()
            .map(|segment| segment.index)
            .collect::<Vec<_>>();
        let projected_best = projected_best_intervals(run);
        let true_time_micros = projected_best.as_ref().map(|projection| {
            projection
                .intervals
                .iter()
                .map(|(started, ended)| ended.saturating_sub(*started))
                .sum::<u64>()
        });
        let mut specs = vec![HistoryViewSpec {
            id: "all".into(),
            label: "Entire run".into(),
            kind: "all".into(),
            segment_indices: all_segments,
            intervals: all_intervals,
            elapsed_micros: reviewed_game_time_micros,
            active_combat_micros: run.timing.active_combat_micros,
            compress_intervals: false,
        }];
        if let Some(projection) = projected_best {
            specs.push(HistoryViewSpec {
                id: "true_time".into(),
                label: "True Time".into(),
                kind: "projected_best".into(),
                segment_indices: projection.segment_indices,
                intervals: projection.intervals,
                elapsed_micros: true_time_micros.unwrap_or_default(),
                active_combat_micros: projection.active_combat_micros,
                compress_intervals: true,
            });
        }

        for (kind, id, label) in [
            (RunSegmentKind::Mobbing, "mobbing", "Mobbing"),
            (RunSegmentKind::Boss, "boss", "Bossing"),
            (RunSegmentKind::RaidBoss, "raid_boss", "Raid boss"),
            (RunSegmentKind::Gauntlet, "gauntlet", "Gauntlet"),
        ] {
            let selected = run
                .segments
                .iter()
                .filter(|segment| segment.kind == kind)
                .collect::<Vec<_>>();
            if selected.is_empty() {
                continue;
            }
            let intervals = selected
                .iter()
                .flat_map(|segment| history_segment_view_intervals(run, segment))
                .collect::<Vec<_>>();
            if intervals.is_empty() {
                continue;
            }
            specs.push(HistoryViewSpec {
                id: id.into(),
                label: label.into(),
                kind: id.into(),
                segment_indices: selected.iter().map(|segment| segment.index).collect(),
                elapsed_micros: intervals
                    .iter()
                    .map(|(started, ended)| ended.saturating_sub(*started))
                    .sum(),
                intervals,
                active_combat_micros: selected
                    .iter()
                    .map(|segment| history_segment_view_active_combat_micros(run, segment))
                    .sum(),
                compress_intervals: false,
            });
        }

        let mut failed_boss_attempts = run
            .encounters
            .iter()
            .filter(|encounter| {
                !encounter.is_successful_attempt
                    && encounter.terminal_state != EncounterTerminalState::Open
                    && run
                        .segments
                        .get(encounter.segment_index as usize)
                        .is_some_and(|segment| is_boss_segment_kind(segment.kind))
            })
            .collect::<Vec<_>>();
        failed_boss_attempts
            .sort_unstable_by_key(|encounter| (encounter.started_micros, encounter.index));
        for (retry_index, encounter) in failed_boss_attempts.into_iter().enumerate() {
            specs.push(HistoryViewSpec {
                id: format!("retry:{}", retry_index + 1),
                label: format!("Retry #{}", retry_index + 1),
                kind: "retry".into(),
                segment_indices: vec![encounter.segment_index],
                intervals: vec![(encounter.started_micros, encounter.ended_micros)],
                elapsed_micros: encounter.wall_time_micros,
                active_combat_micros: encounter.active_combat_micros,
                compress_intervals: false,
            });
        }

        if run.segments.len() > 2 {
            for segment in &run.segments {
                specs.push(HistoryViewSpec {
                    id: format!("segment:{}", segment.index),
                    label: format!("{} {}", segment_kind_label(segment.kind), segment.index + 1),
                    kind: "segment".into(),
                    segment_indices: vec![segment.index],
                    intervals: vec![(segment.started_micros, segment.ended_micros)],
                    elapsed_micros: segment.wall_time_micros,
                    active_combat_micros: segment.active_combat_micros,
                    compress_intervals: false,
                });
            }
        }

        for spec in &mut specs {
            spec.active_combat_micros = if infer_minimum_active_time_from_facts {
                self.history_active_combat_micros(spec.active_combat_micros, &spec.intervals)
            } else {
                self.history_active_combat_micros_without_facts(
                    spec.active_combat_micros,
                    &spec.intervals,
                )
            };
        }

        let history = CombatRunHistory {
            run_index: run_index as u32,
            activity_id: run.identity.activity_id.clone(),
            activity_family_id: run.identity.activity_family_id.clone(),
            scene_id: run.identity.scene_id,
            presentation_scene_name: None,
            instance_id: run.identity.instance_id.clone(),
            difficulty_family: run.identity.difficulty_family.clone(),
            difficulty_tier: run.identity.difficulty_tier,
            terminal_state: run_terminal_state_name(run.terminal_state).into(),
            entered_micros,
            started_micros,
            first_combat_micros,
            ended_micros,
            load_time_micros: entered_micros.map(|entered| started_micros.saturating_sub(entered)),
            precombat_time_micros: first_combat_micros
                .map(|first| first.saturating_sub(started_micros)),
            total_run_time_micros: entered_micros
                .zip(ended_micros)
                .map(|(entered, ended)| ended.saturating_sub(entered)),
            game_time_micros: Some(reviewed_game_time_micros),
            true_time_micros,
            retry_count: run.segments.iter().map(|segment| segment.retry_count).sum(),
            boss_retry_count: run
                .segments
                .iter()
                .filter(|segment| {
                    matches!(
                        segment.kind,
                        RunSegmentKind::Boss | RunSegmentKind::RaidBoss | RunSegmentKind::Gauntlet
                    )
                })
                .map(|segment| segment.retry_count)
                .sum(),
            wipe_count: run
                .encounters
                .iter()
                .filter(|encounter| encounter.terminal_state == EncounterTerminalState::Wiped)
                .count() as u32,
            cleared_encounter_count: run
                .encounters
                .iter()
                .filter(|encounter| encounter.terminal_state == EncounterTerminalState::Cleared)
                .count() as u32,
            last_encounter_terminal_state: run
                .encounters
                .last()
                .map(|encounter| encounter_terminal_state_name(encounter.terminal_state).into()),
            rdps_status: self.rdps_status(),
            apm_status: "pending_active_action_classification".into(),
            views: Vec::new(),
        };
        (history, specs)
    }

    fn canonical_history_fact(&self, fact: &CombatFact) -> CombatFact {
        let resolve = |actor_id: u64, entity_uuid: i64| {
            self.actor_ancestry.resolve_entity_at(
                EntityRef {
                    actor_id: ActorId(actor_id),
                    entity_uuid: EntityUuid(entity_uuid),
                },
                fact.observed_micros,
            )
        };
        let source = resolve(fact.source_actor_id, fact.source_entity_uuid);
        let source_actor_id = self.canonical_actor_id(source.actor_id.0);
        let target = fact.target.map(|(actor_id, entity_uuid)| {
            let target = resolve(actor_id, entity_uuid);
            (
                self.canonical_actor_id(target.actor_id.0),
                target.entity_uuid.0,
            )
        });
        let kind = match &fact.kind {
            CombatFactKind::Status {
                effect_id,
                attribution_source_actor_id,
                instance_id,
                state,
                stacks,
                duration_millis,
            } => CombatFactKind::Status {
                effect_id: *effect_id,
                attribution_source_actor_id: attribution_source_actor_id.map(|actor_id| {
                    self.canonical_actor_id(
                        self.actor_ancestry
                            .resolve_actor_id_at(actor_id, fact.observed_micros),
                    )
                }),
                instance_id: *instance_id,
                state: *state,
                stacks: *stacks,
                duration_millis: *duration_millis,
            },
            CombatFactKind::ExactDamageContribution {
                effect_id,
                scope,
                provider_actor_id,
                recipient_actor_id,
                amount,
                observed_damage,
                damage_event_sequence,
                affected_ability_id,
                affected_target,
                critical,
            } => CombatFactKind::ExactDamageContribution {
                effect_id: *effect_id,
                scope: *scope,
                provider_actor_id: self.canonical_actor_id(
                    self.actor_ancestry
                        .resolve_actor_id_at(*provider_actor_id, fact.observed_micros),
                ),
                recipient_actor_id: self.canonical_actor_id(
                    self.actor_ancestry
                        .resolve_actor_id_at(*recipient_actor_id, fact.observed_micros),
                ),
                amount: *amount,
                observed_damage: *observed_damage,
                damage_event_sequence: *damage_event_sequence,
                affected_ability_id: *affected_ability_id,
                affected_target: affected_target.map(|(actor_id, entity_uuid)| {
                    let target = resolve(actor_id, entity_uuid);
                    (
                        self.canonical_actor_id(target.actor_id.0),
                        target.entity_uuid.0,
                    )
                }),
                critical: *critical,
            },
            CombatFactKind::ExactRationalDamageContribution {
                effect_id,
                scope,
                provider_actor_id,
                recipient_actor_id,
                numerator,
                denominator,
                observed_damage,
                damage_event_sequence,
                affected_ability_id,
                affected_target,
                critical,
            } => CombatFactKind::ExactRationalDamageContribution {
                effect_id: *effect_id,
                scope: *scope,
                provider_actor_id: self.canonical_actor_id(
                    self.actor_ancestry
                        .resolve_actor_id_at(*provider_actor_id, fact.observed_micros),
                ),
                recipient_actor_id: self.canonical_actor_id(
                    self.actor_ancestry
                        .resolve_actor_id_at(*recipient_actor_id, fact.observed_micros),
                ),
                numerator: *numerator,
                denominator: *denominator,
                observed_damage: *observed_damage,
                damage_event_sequence: *damage_event_sequence,
                affected_ability_id: *affected_ability_id,
                affected_target: affected_target.map(|(actor_id, entity_uuid)| {
                    let target = resolve(actor_id, entity_uuid);
                    (
                        self.canonical_actor_id(target.actor_id.0),
                        target.entity_uuid.0,
                    )
                }),
                critical: *critical,
            },
            other => other.clone(),
        };
        CombatFact {
            observed_micros: fact.observed_micros,
            source_actor_id,
            source_entity_uuid: source.entity_uuid.0,
            target,
            breakdown_ability_id: fact.breakdown_ability_id,
            ability_id: fact.ability_id,
            kind,
        }
    }

    /// Stores the compact history projection with the ownership evidence that
    /// was valid when the event arrived. The canonical `.rlog` still retains
    /// the direct packet identities for the raw event viewer, but history must
    /// never consult a later live ownership snapshot: actor IDs are reused and
    /// summons can despawn between runs.
    fn push_history_fact(&mut self, fact: CombatFact) {
        let canonical = self.canonical_history_fact(&fact);
        self.history_facts.push(canonical);
    }

    fn build_history_view(&self, spec: &HistoryViewSpec) -> CombatHistoryView {
        let origin_micros = spec
            .intervals
            .iter()
            .map(|(started, _)| *started)
            .min()
            .unwrap_or_default();
        let last_selected_micros = spec
            .intervals
            .iter()
            .map(|(_, ended)| *ended)
            .max()
            .unwrap_or_default();
        let mut values = BTreeMap::<u64, HistoryValueAccumulator>::new();
        let mut damage_influences =
            BTreeMap::<HistoryDamageInfluenceKey, HistoryDamageInfluenceAccumulator>::new();
        let mut attribution = DamageContributionReducer::new(self.contribution_rules.clone())
            .expect("combat plug-in stores only validated rDPS rules");
        for fact in &self.history_facts {
            if fact.observed_micros > last_selected_micros {
                break;
            }
            attribution.set_provider_eligible(
                fact.source_actor_id,
                self.history_identity_at(
                    fact.source_actor_id,
                    fact.source_entity_uuid,
                    last_selected_micros,
                )
                .is_some_and(|identity| identity.actor_kind.as_deref() == Some("player")),
            );
            if let Some((target_actor_id, target_entity_uuid)) = fact.target {
                attribution.set_provider_eligible(
                    target_actor_id,
                    self.history_identity_at(
                        target_actor_id,
                        target_entity_uuid,
                        last_selected_micros,
                    )
                    .is_some_and(|identity| identity.actor_kind.as_deref() == Some("player")),
                );
            }
            let offset_micros = history_fact_offset(
                fact.observed_micros,
                &spec.intervals,
                origin_micros,
                spec.compress_intervals,
            );
            match fact.kind {
                CombatFactKind::StatusReset => attribution.reset_statuses(),
                CombatFactKind::Status {
                    effect_id,
                    attribution_source_actor_id,
                    instance_id,
                    state,
                    stacks,
                    duration_millis,
                } => attribution.observe_status(ContributionStatusEvent {
                    observed_micros: fact.observed_micros,
                    source_actor_id: attribution_source_actor_id,
                    target_actor_id: fact.target.map_or(0, |target| target.0),
                    effect_id,
                    instance_id,
                    state: contribution_status_state(state),
                    stacks,
                    duration_millis,
                }),
                CombatFactKind::Damage { reported, .. } => {
                    if let Some((target_actor_id, _)) = fact.target {
                        attribution.observe_damage(ContributionDamageEvent {
                            observed_micros: fact.observed_micros,
                            source_actor_id: fact.source_actor_id,
                            target_actor_id,
                            amount: reported,
                            included: offset_micros.is_some(),
                        });
                    }
                }
                CombatFactKind::ExactDamageContribution {
                    effect_id,
                    scope,
                    provider_actor_id,
                    recipient_actor_id,
                    amount,
                    observed_damage,
                    damage_event_sequence,
                    affected_ability_id,
                    affected_target,
                    critical,
                } => {
                    attribution.observe_exact_contribution(ExactDamageContributionEvent {
                        observed_micros: fact.observed_micros,
                        effect_id,
                        provider_actor_id,
                        recipient_actor_id,
                        scope,
                        amount,
                        observed_damage,
                        included: offset_micros.is_some(),
                    });
                    if offset_micros.is_some() {
                        observe_history_damage_influence(
                            &mut damage_influences,
                            HistoryDamageInfluenceObservation {
                                observed_micros: fact.observed_micros,
                                effect_id,
                                scope,
                                provider_actor_id,
                                provider_entity_uuid: fact.source_entity_uuid,
                                recipient_actor_id,
                                recipient_entity_uuid: fact
                                    .target
                                    .filter(|(actor_id, _)| *actor_id == recipient_actor_id)
                                    .map_or(0, |(_, entity_uuid)| entity_uuid),
                                damage_event_sequence,
                                affected_ability_id,
                                affected_target,
                                critical,
                                observed_damage,
                                exact_integer_delta: Some(amount),
                                exact_rational_delta: None,
                            },
                        );
                    }
                }
                CombatFactKind::ExactRationalDamageContribution {
                    effect_id,
                    scope,
                    provider_actor_id,
                    recipient_actor_id,
                    numerator,
                    denominator,
                    observed_damage,
                    damage_event_sequence,
                    affected_ability_id,
                    affected_target,
                    critical,
                } => {
                    attribution.observe_exact_rational_contribution(
                        ExactRationalDamageContributionEvent {
                            observed_micros: fact.observed_micros,
                            effect_id,
                            provider_actor_id,
                            recipient_actor_id,
                            scope,
                            numerator,
                            denominator,
                            observed_damage,
                            included: offset_micros.is_some(),
                            deferred_damage_context: None,
                        },
                    );
                    if offset_micros.is_some() {
                        observe_history_damage_influence(
                            &mut damage_influences,
                            HistoryDamageInfluenceObservation {
                                observed_micros: fact.observed_micros,
                                effect_id,
                                scope,
                                provider_actor_id,
                                provider_entity_uuid: fact.source_entity_uuid,
                                recipient_actor_id,
                                recipient_entity_uuid: fact
                                    .target
                                    .filter(|(actor_id, _)| *actor_id == recipient_actor_id)
                                    .map_or(0, |(_, entity_uuid)| entity_uuid),
                                damage_event_sequence,
                                affected_ability_id,
                                affected_target,
                                critical,
                                observed_damage,
                                exact_integer_delta: None,
                                exact_rational_delta: Some((numerator, denominator)),
                            },
                        );
                    }
                }
                _ => {}
            }
            let Some(offset_micros) = offset_micros else {
                continue;
            };
            values.entry(fact.source_actor_id).or_default().entity_uuid = fact.source_entity_uuid;
            if let Some((target_actor_id, target_entity_uuid)) = fact.target {
                values.entry(target_actor_id).or_default().entity_uuid = target_entity_uuid;
            }
            let second = offset_micros
                .saturating_div(1_000_000)
                .min(u64::from(u32::MAX)) as u32;
            match fact.kind {
                CombatFactKind::StatusReset => {}
                CombatFactKind::Cast => {
                    let source = values.entry(fact.source_actor_id).or_default();
                    source.casts = source.casts.saturating_add(1);
                    let ability = source
                        .abilities
                        .entry(
                            fact.breakdown_ability_id
                                .or(fact.ability_id)
                                .unwrap_or_default(),
                        )
                        .or_default();
                    ability.casts = ability.casts.saturating_add(1);
                }
                CombatFactKind::Damage {
                    reported,
                    effective,
                    critical,
                } => {
                    let Some((target_actor_id, target_entity_uuid)) = fact.target else {
                        continue;
                    };
                    {
                        let source = values.entry(fact.source_actor_id).or_default();
                        source.damage = source.damage.saturating_add(reported);
                        source.effective_damage = source.effective_damage.saturating_add(effective);
                        source.hits = source.hits.saturating_add(1);
                        source.critical_hits =
                            source.critical_hits.saturating_add(u64::from(critical));
                        let target = source.targets.entry(target_actor_id).or_default();
                        target.entity_uuid = target_entity_uuid;
                        target.damage = target.damage.saturating_add(reported);
                        target.effective_damage = target.effective_damage.saturating_add(effective);
                        target.hits = target.hits.saturating_add(1);
                        target.critical_hits =
                            target.critical_hits.saturating_add(u64::from(critical));
                        let target_point = target.series.entry(second).or_default();
                        target_point.damage = target_point.damage.saturating_add(reported);
                        let ability = source
                            .abilities
                            .entry(
                                fact.breakdown_ability_id
                                    .or(fact.ability_id)
                                    .unwrap_or_default(),
                            )
                            .or_default();
                        ability.damage = ability.damage.saturating_add(reported);
                        ability.effective_damage =
                            ability.effective_damage.saturating_add(effective);
                        ability.hits = ability.hits.saturating_add(1);
                        ability.critical_hits =
                            ability.critical_hits.saturating_add(u64::from(critical));
                        let ability_target = ability.targets.entry(target_actor_id).or_default();
                        ability_target.entity_uuid = target_entity_uuid;
                        ability_target.damage = ability_target.damage.saturating_add(reported);
                        ability_target.effective_damage =
                            ability_target.effective_damage.saturating_add(effective);
                        ability_target.hits = ability_target.hits.saturating_add(1);
                        ability_target.critical_hits = ability_target
                            .critical_hits
                            .saturating_add(u64::from(critical));
                        let point = source.series.entry(second).or_default();
                        point.damage = point.damage.saturating_add(reported);
                    }
                    let target = values.entry(target_actor_id).or_default();
                    target.damage_taken = target.damage_taken.saturating_add(effective);
                    let source = target.targets.entry(fact.source_actor_id).or_default();
                    source.entity_uuid = fact.source_entity_uuid;
                    let source_point = source.series.entry(second).or_default();
                    source_point.damage_taken = source_point.damage_taken.saturating_add(effective);
                    let point = target.series.entry(second).or_default();
                    point.damage_taken = point.damage_taken.saturating_add(effective);
                }
                CombatFactKind::Healing {
                    reported,
                    effective,
                } => {
                    let source = values.entry(fact.source_actor_id).or_default();
                    source.healing = source.healing.saturating_add(reported);
                    source.effective_healing = source.effective_healing.saturating_add(effective);
                    let ability = source
                        .abilities
                        .entry(
                            fact.breakdown_ability_id
                                .or(fact.ability_id)
                                .unwrap_or_default(),
                        )
                        .or_default();
                    ability.healing = ability.healing.saturating_add(reported);
                    ability.effective_healing = ability.effective_healing.saturating_add(effective);
                    if let Some((target_actor_id, target_entity_uuid)) = fact.target {
                        let target = ability.targets.entry(target_actor_id).or_default();
                        target.entity_uuid = target_entity_uuid;
                        target.healing = target.healing.saturating_add(reported);
                        target.effective_healing =
                            target.effective_healing.saturating_add(effective);
                        let target = source.targets.entry(target_actor_id).or_default();
                        target.entity_uuid = target_entity_uuid;
                        let target_point = target.series.entry(second).or_default();
                        target_point.effective_healing =
                            target_point.effective_healing.saturating_add(effective);
                    }
                    let point = source.series.entry(second).or_default();
                    point.effective_healing = point.effective_healing.saturating_add(effective);
                }
                CombatFactKind::Shield { amount } => {
                    let source = values.entry(fact.source_actor_id).or_default();
                    source.shielding = source.shielding.saturating_add(amount);
                    let ability = source
                        .abilities
                        .entry(
                            fact.breakdown_ability_id
                                .or(fact.ability_id)
                                .unwrap_or_default(),
                        )
                        .or_default();
                    ability.shielding = ability.shielding.saturating_add(amount);
                    if let Some((target_actor_id, target_entity_uuid)) = fact.target {
                        let target = ability.targets.entry(target_actor_id).or_default();
                        target.entity_uuid = target_entity_uuid;
                        target.shielding = target.shielding.saturating_add(amount);
                    }
                }
                CombatFactKind::Life { state } => {
                    if state == LifeState::Died {
                        let actor = values.entry(fact.source_actor_id).or_default();
                        actor.deaths = actor.deaths.saturating_add(1);
                        actor.death_seconds.push(second);
                    }
                }
                CombatFactKind::Status {
                    effect_id, state, ..
                } => {
                    let Some((target_actor_id, target_entity_uuid)) = fact.target else {
                        continue;
                    };
                    let source = values.entry(fact.source_actor_id).or_default();
                    let target = source.targets.entry(target_actor_id).or_insert_with(|| {
                        HistoryTargetAccumulator {
                            entity_uuid: target_entity_uuid,
                            ..HistoryTargetAccumulator::default()
                        }
                    });
                    target.effect_events = target.effect_events.saturating_add(1);
                    let effect = source
                        .effects
                        .entry((effect_id, target_actor_id))
                        .or_default();
                    effect.target_entity_uuid = target_entity_uuid;
                    match state {
                        StatusState::Applied => effect.applied = effect.applied.saturating_add(1),
                        StatusState::Refreshed => {
                            effect.refreshed = effect.refreshed.saturating_add(1)
                        }
                        StatusState::Stacked => effect.stacked = effect.stacked.saturating_add(1),
                        StatusState::Consumed => {
                            effect.consumed = effect.consumed.saturating_add(1)
                        }
                        StatusState::Removed => effect.removed = effect.removed.saturating_add(1),
                    }
                }
                CombatFactKind::ExactDamageContribution { .. }
                | CombatFactKind::ExactRationalDamageContribution { .. } => {}
            }
        }

        let damage_influences = if self.rdps_enabled() {
            let contribution = attribution.summary();
            debug_assert!(contribution.is_conserved());
            for actor_id in contribution.actors.keys() {
                values.entry(*actor_id).or_default();
            }
            for (actor_id, value) in &mut values {
                let actor = contribution.actors.get(actor_id);
                value.rdps_damage = Some(actor.map_or(value.damage, |actor| actor.rdps_damage));
                value.rdps_contribution_given =
                    Some(actor.map_or(0, |actor| actor.contribution_given));
                value.rdps_contribution_received =
                    Some(actor.map_or(0, |actor| actor.contribution_received));
            }
            if let Some(projector) = self.exact_contribution_projector.as_ref() {
                for actor_id in projector.incomplete_rdps_actor_ids() {
                    if let Some(value) = values.get_mut(&actor_id) {
                        value.rdps_incomplete = true;
                    }
                }
            }
            finish_history_damage_influences(
                damage_influences,
                &contribution.rational_effect_projections,
            )
        } else {
            finish_history_damage_influences(damage_influences, &[])
        };

        let elapsed_seconds = seconds(spec.elapsed_micros);
        let active_seconds = seconds(spec.active_combat_micros);
        let actors = values
            .into_iter()
            .map(|(actor_id, value)| {
                self.finish_history_actor(
                    actor_id,
                    value,
                    elapsed_seconds,
                    active_seconds,
                    last_selected_micros,
                )
            })
            .collect::<Vec<_>>();
        let target_identities = actors
            .iter()
            .filter(|actor| actor.actor_kind.as_deref() == Some("player"))
            .flat_map(|actor| actor.targets.iter())
            .filter(|target| target.damage > 0 || target.effect_events > 0)
            .map(|target| (target.actor_id.clone(), target.entity_uuid.clone()))
            .collect::<BTreeSet<_>>();
        let targets = target_identities
            .into_iter()
            .filter_map(|(actor_id, entity_uuid)| {
                let parsed_actor_id = actor_id.parse::<u64>().ok()?;
                let parsed_entity_uuid = entity_uuid.parse::<i64>().ok()?;
                let identity = self.history_identity_at(
                    parsed_actor_id,
                    parsed_entity_uuid,
                    last_selected_micros,
                );
                // Preserve all canonical actor/event evidence, but admit only
                // encounter targets to a formal run's selector. Owned pets
                // are already attributed to their players, while training
                // dummies can remain visible as ambient AOI traffic after the
                // client enters a dungeon. Projectiles are intentionally kept:
                // damageable encounter mechanics use that actor kind.
                if identity.is_some_and(|identity| {
                    !history_target_is_selectable(identity.actor_kind.as_deref())
                }) {
                    return None;
                }
                Some(HistoryTargetIdentity {
                    actor_id,
                    entity_uuid,
                    monster_id: identity
                        .and_then(|identity| identity.monster_id)
                        .map(|monster_id| monster_id.to_string()),
                    display_name: identity.and_then(|identity| identity.display_name.clone()),
                    actor_kind: identity.and_then(|identity| identity.actor_kind.clone()),
                    presentation_name: None,
                })
            })
            .collect();

        CombatHistoryView {
            id: spec.id.clone(),
            label: spec.label.clone(),
            kind: spec.kind.clone(),
            segment_indices: spec.segment_indices.clone(),
            elapsed_micros: spec.elapsed_micros,
            active_combat_micros: spec.active_combat_micros,
            actors,
            targets,
            damage_influences,
            rdps_effect_presentations: Vec::new(),
        }
    }

    fn inferred_active_combat_micros(&self, intervals: &[(u64, u64)]) -> u64 {
        let closed = self.combat_windows.iter().copied();
        let open = self
            .active_combat_started
            .zip(self.last_event_micros)
            .filter(|(started, ended)| ended > started)
            .into_iter();
        closed
            .chain(open)
            .map(|window| interval_overlap_micros(window, intervals))
            .sum()
    }

    fn history_active_combat_micros(
        &self,
        reviewed_active_combat_micros: u64,
        intervals: &[(u64, u64)],
    ) -> u64 {
        let inferred = self
            .history_active_combat_micros_without_facts(reviewed_active_combat_micros, intervals);
        if inferred > 0 {
            return inferred;
        }
        if self.history_facts.iter().any(|fact| {
            matches!(&fact.kind, CombatFactKind::Damage { reported, .. } if *reported > 0)
                && history_fact_offset(fact.observed_micros, intervals, 0, false).is_some()
        }) {
            MINIMUM_PERSONAL_ACTIVE_MICROS
        } else {
            0
        }
    }

    fn history_active_combat_micros_without_facts(
        &self,
        reviewed_active_combat_micros: u64,
        intervals: &[(u64, u64)],
    ) -> u64 {
        if reviewed_active_combat_micros > 0 {
            reviewed_active_combat_micros
        } else {
            self.inferred_active_combat_micros(intervals)
        }
    }

    fn finish_history_actor(
        &self,
        actor_id: u64,
        value: HistoryValueAccumulator,
        elapsed_seconds: f64,
        active_seconds: f64,
        last_selected_micros: u64,
    ) -> HistoryActorSummary {
        let identity = self.history_identity_at(actor_id, value.entity_uuid, last_selected_micros);
        let entity_uuid = identity.map_or(value.entity_uuid, |actor| actor.entity_uuid);
        HistoryActorSummary {
            actor_id: actor_id.to_string(),
            entity_uuid: entity_uuid.to_string(),
            monster_id: identity
                .and_then(|actor| actor.monster_id)
                .map(|monster_id| monster_id.to_string()),
            character_id: None,
            display_name: identity.and_then(|actor| actor.display_name.clone()),
            actor_kind: identity.and_then(|actor| actor.actor_kind.clone()),
            presentation_name: None,
            presentation_kind: None,
            class_id: identity.and_then(|actor| actor.class_id),
            specialization_id: identity.and_then(|actor| actor.specialization_id),
            presentation_class_name: None,
            presentation_specialization_name: None,
            icon_asset_path: None,
            weapon_icon_asset_path: None,
            presentation_role: None,
            presentation_accent: None,
            level: identity.and_then(|actor| actor.level),
            ability_score: identity.and_then(|actor| actor.ability_score),
            weapon_item_id: identity.and_then(|actor| actor.weapon_item_id),
            weapon_breakthrough_count: identity.and_then(|actor| actor.weapon_breakthrough_count),
            weapon_presentation_name: None,
            weapon_level: None,
            weapon_level_min: None,
            weapon_level_max: None,
            weapon_badge_kind: None,
            seasonal_score: identity.and_then(|actor| actor.seasonal_score),
            primary_loadout: identity
                .map(|actor| actor.primary_loadout.clone())
                .unwrap_or_default(),
            auxiliary_loadout: identity
                .map(|actor| actor.auxiliary_loadout.clone())
                .unwrap_or_default(),
            damage: value.damage,
            effective_damage: value.effective_damage,
            damage_taken: value.damage_taken,
            healing: value.healing,
            effective_healing: value.effective_healing,
            shielding: value.shielding,
            hits: value.hits,
            critical_hits: value.critical_hits,
            deaths: value.deaths,
            death_seconds: value.death_seconds,
            dps: rate_per_second(value.damage, elapsed_seconds),
            encounter_dps: rate_per_second(value.damage, active_seconds),
            hps: rate_per_second(value.healing, elapsed_seconds),
            tps: rate_per_second(value.damage_taken, elapsed_seconds),
            rdps: value
                .rdps_damage
                .map(|damage| rate_per_second(damage, active_seconds)),
            rdps_damage: value.rdps_damage,
            rdps_contribution_given: value.rdps_contribution_given,
            rdps_contribution_received: value.rdps_contribution_received,
            rdps_incomplete: value.rdps_incomplete,
            apm: None,
            observed_cast_events: value.casts,
            abilities: value
                .abilities
                .into_iter()
                .map(|(ability_id, ability)| HistoryAbilitySummary {
                    ability_id: ability_id.to_string(),
                    presentation_name: None,
                    presentation_kind: None,
                    presentation_resolution: None,
                    icon_asset_path: None,
                    presentation_recount_group_id: None,
                    presentation_recount_group_name: None,
                    casts: ability.casts,
                    hits: ability.hits,
                    critical_hits: ability.critical_hits,
                    damage: ability.damage,
                    effective_damage: ability.effective_damage,
                    healing: ability.healing,
                    effective_healing: ability.effective_healing,
                    shielding: ability.shielding,
                    dps: rate_per_second(ability.damage, elapsed_seconds),
                    encounter_dps: rate_per_second(ability.damage, active_seconds),
                    hps: rate_per_second(ability.healing, elapsed_seconds),
                    targets: ability
                        .targets
                        .into_iter()
                        .map(|(target_actor_id, target)| HistoryAbilityTargetSummary {
                            actor_id: target_actor_id.to_string(),
                            entity_uuid: target.entity_uuid.to_string(),
                            damage: target.damage,
                            effective_damage: target.effective_damage,
                            healing: target.healing,
                            effective_healing: target.effective_healing,
                            shielding: target.shielding,
                            hits: target.hits,
                            critical_hits: target.critical_hits,
                        })
                        .collect(),
                })
                .collect(),
            targets: value
                .targets
                .into_iter()
                .map(|(target_actor_id, target)| HistoryTargetSummary {
                    actor_id: target_actor_id.to_string(),
                    entity_uuid: target.entity_uuid.to_string(),
                    damage: target.damage,
                    effective_damage: target.effective_damage,
                    hits: target.hits,
                    critical_hits: target.critical_hits,
                    effect_events: target.effect_events,
                    series: target
                        .series
                        .into_iter()
                        .map(|(second, point)| HistorySeriesPoint {
                            second,
                            damage: point.damage,
                            effective_healing: point.effective_healing,
                            damage_taken: point.damage_taken,
                        })
                        .collect(),
                })
                .collect(),
            effects: value
                .effects
                .into_iter()
                .map(
                    |((effect_id, target_actor_id), effect)| HistoryEffectSummary {
                        effect_id: effect_id.to_string(),
                        presentation_name: None,
                        presentation_kind: None,
                        presentation_resolution: None,
                        icon_asset_path: None,
                        target_actor_id: target_actor_id.to_string(),
                        target_entity_uuid: effect.target_entity_uuid.to_string(),
                        applied: effect.applied,
                        refreshed: effect.refreshed,
                        stacked: effect.stacked,
                        consumed: effect.consumed,
                        removed: effect.removed,
                    },
                )
                .collect(),
            series: value
                .series
                .into_iter()
                .map(|(second, point)| HistorySeriesPoint {
                    second,
                    damage: point.damage,
                    effective_healing: point.effective_healing,
                    damage_taken: point.damage_taken,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone)]
struct HistoryViewSpec {
    id: String,
    label: String,
    kind: String,
    segment_indices: Vec<u32>,
    intervals: Vec<(u64, u64)>,
    elapsed_micros: u64,
    active_combat_micros: u64,
    compress_intervals: bool,
}

#[derive(Debug, Clone, Copy)]
struct HistoryDamageInfluenceObservation {
    observed_micros: u64,
    effect_id: i64,
    scope: DamageContributionScope,
    provider_actor_id: u64,
    provider_entity_uuid: i64,
    recipient_actor_id: u64,
    recipient_entity_uuid: i64,
    damage_event_sequence: Option<u64>,
    affected_ability_id: Option<i64>,
    affected_target: Option<(u64, i64)>,
    critical: Option<bool>,
    observed_damage: i64,
    exact_integer_delta: Option<i64>,
    exact_rational_delta: Option<(i128, i128)>,
}

fn observe_history_damage_influence(
    influences: &mut BTreeMap<HistoryDamageInfluenceKey, HistoryDamageInfluenceAccumulator>,
    observation: HistoryDamageInfluenceObservation,
) {
    let key = history_damage_influence_key(observation);
    let context_complete =
        observation.damage_event_sequence.is_some() && observation.affected_target.is_some();
    let accumulator = influences.entry(key).or_default();
    let is_first = accumulator.first_observed_micros.is_none();
    accumulator.first_observed_micros = Some(
        accumulator
            .first_observed_micros
            .map_or(observation.observed_micros, |first| {
                first.min(observation.observed_micros)
            }),
    );
    accumulator.last_observed_micros = accumulator
        .last_observed_micros
        .max(observation.observed_micros);
    if is_first {
        accumulator.damage_context_complete = context_complete;
    } else {
        accumulator.damage_context_complete &= context_complete;
    }

    let new_damage_event = observation.damage_event_sequence.is_none()
        || accumulator.last_damage_event_sequence != observation.damage_event_sequence;
    if new_damage_event {
        let first_damage_event = accumulator.damage_event_count == 0;
        accumulator.damage_event_count = accumulator.damage_event_count.saturating_add(1);
        accumulator.critical_hit_count = match (
            accumulator.critical_hit_count,
            observation.critical,
            first_damage_event,
        ) {
            (_, None, _) => None,
            (Some(total), Some(critical), _) => Some(total.saturating_add(u64::from(critical))),
            (None, Some(critical), true) => Some(u64::from(critical)),
            (None, Some(_), false) => None,
        };
        accumulator.observed_damage = accumulator
            .observed_damage
            .saturating_add(observation.observed_damage);
        accumulator.last_damage_event_sequence = observation.damage_event_sequence;
    }
    if let Some(delta) = observation.exact_integer_delta {
        accumulator.exact_integer_delta = accumulator.exact_integer_delta.saturating_add(delta);
    }
    if let Some((numerator, denominator)) = observation.exact_rational_delta {
        let term = accumulator
            .rational_by_denominator
            .entry(denominator)
            .or_default();
        term.0 = term.0.saturating_add(numerator);
        term.1 = term.1.saturating_add(1);
    }
}

fn history_damage_influence_key(
    observation: HistoryDamageInfluenceObservation,
) -> HistoryDamageInfluenceKey {
    HistoryDamageInfluenceKey {
        effect_id: observation.effect_id,
        scope: observation.scope,
        provider_actor_id: observation.provider_actor_id,
        provider_entity_uuid: observation.provider_entity_uuid,
        recipient_actor_id: observation.recipient_actor_id,
        recipient_entity_uuid: observation.recipient_entity_uuid,
        affected_ability_id: observation.affected_ability_id,
        target_actor_id: observation.affected_target.map(|(actor_id, _)| actor_id),
        target_entity_uuid: observation
            .affected_target
            .map(|(_, entity_uuid)| entity_uuid),
    }
}

fn finish_history_damage_influences(
    influences: BTreeMap<HistoryDamageInfluenceKey, HistoryDamageInfluenceAccumulator>,
    rational_effect_projections: &[EffectDamageContribution],
) -> Vec<HistoryDamageInfluenceSummary> {
    let influences = influences.into_iter().collect::<Vec<_>>();
    let mut attributed_rdps = influences
        .iter()
        .map(|(_, accumulator)| Some(accumulator.exact_integer_delta))
        .collect::<Vec<_>>();
    let mut rational_rows = BTreeMap::<(i64, u64, u64), Vec<usize>>::new();
    for (index, (key, accumulator)) in influences.iter().enumerate() {
        if !accumulator.rational_by_denominator.is_empty() {
            rational_rows
                .entry((key.effect_id, key.provider_actor_id, key.recipient_actor_id))
                .or_default()
                .push(index);
        }
    }
    let projected_totals = rational_effect_projections
        .iter()
        .map(|projection| {
            (
                (
                    projection.effect_id,
                    projection.provider_actor_id,
                    projection.recipient_actor_id,
                ),
                projection.amount,
            )
        })
        .collect::<BTreeMap<_, _>>();
    for (group, row_indices) in rational_rows {
        let target = projected_totals.get(&group).copied().unwrap_or_default();
        let Some(allocations) = allocate_history_rational_rows(&influences, &row_indices, target)
        else {
            for row_index in row_indices {
                attributed_rdps[row_index] = None;
            }
            continue;
        };
        for (row_index, rational_amount) in allocations {
            attributed_rdps[row_index] = attributed_rdps[row_index]
                .and_then(|integer_amount| integer_amount.checked_add(rational_amount));
        }
    }

    influences
        .into_iter()
        .enumerate()
        .map(
            |(index, (key, accumulator))| HistoryDamageInfluenceSummary {
                effect_id: key.effect_id.to_string(),
                attribution_component: key.scope.component_key().map(str::to_owned),
                complete_effect: key.scope.is_complete_effect(),
                provider_actor_id: key.provider_actor_id.to_string(),
                provider_entity_uuid: key.provider_entity_uuid.to_string(),
                recipient_actor_id: key.recipient_actor_id.to_string(),
                recipient_entity_uuid: key.recipient_entity_uuid.to_string(),
                affected_ability_id: key.affected_ability_id.map(|value| value.to_string()),
                target_actor_id: key.target_actor_id.map(|value| value.to_string()),
                target_entity_uuid: key.target_entity_uuid.map(|value| value.to_string()),
                first_observed_micros: accumulator.first_observed_micros.unwrap_or_default(),
                last_observed_micros: accumulator.last_observed_micros,
                damage_event_count: accumulator.damage_event_count,
                critical_hit_count: accumulator.critical_hit_count,
                observed_damage: accumulator.observed_damage.to_string(),
                exact_integer_delta: accumulator.exact_integer_delta.to_string(),
                exact_rational_deltas: accumulator
                    .rational_by_denominator
                    .into_iter()
                    .map(|(denominator, (numerator, contribution_count))| {
                        let divisor = greatest_common_divisor_i128(numerator, denominator);
                        HistoryRationalDamageDelta {
                            numerator: (numerator / divisor).to_string(),
                            denominator: (denominator / divisor).to_string(),
                            contribution_count,
                        }
                    })
                    .collect(),
                attributed_rdps: attributed_rdps[index].map(|amount| amount.to_string()),
                damage_context_complete: accumulator.damage_context_complete,
            },
        )
        .collect()
}

#[derive(Debug, Clone)]
struct HistoryExactRational {
    numerator: BigInt,
    denominator: BigInt,
}

impl Default for HistoryExactRational {
    fn default() -> Self {
        Self {
            numerator: BigInt::from(0),
            denominator: BigInt::from(1),
        }
    }
}

impl HistoryExactRational {
    fn add(&mut self, numerator: i128, denominator: i128) -> Option<()> {
        if numerator < 0 || denominator <= 0 {
            return None;
        }
        let numerator = BigInt::from(numerator);
        let denominator = BigInt::from(denominator);
        let shared = self.denominator.gcd(&denominator);
        let left_factor = &denominator / &shared;
        let right_factor = &self.denominator / &shared;
        let next_numerator = &self.numerator * &left_factor + numerator * right_factor;
        let next_denominator = &self.denominator * left_factor;
        let divisor = next_numerator.gcd(&next_denominator);
        self.numerator = next_numerator / &divisor;
        self.denominator = next_denominator / divisor;
        Some(())
    }
}

fn allocate_history_rational_rows(
    influences: &[(HistoryDamageInfluenceKey, HistoryDamageInfluenceAccumulator)],
    row_indices: &[usize],
    target: i64,
) -> Option<Vec<(usize, i64)>> {
    if target < 0 {
        return None;
    }
    let mut rows = Vec::with_capacity(row_indices.len());
    let mut base_total = BigInt::from(0);
    let mut exact_total = HistoryExactRational::default();
    for row_index in row_indices {
        let accumulator = &influences.get(*row_index)?.1;
        let mut exact = HistoryExactRational::default();
        for (denominator, (numerator, _)) in &accumulator.rational_by_denominator {
            exact.add(*numerator, *denominator)?;
            exact_total.add(*numerator, *denominator)?;
        }
        let floor = &exact.numerator / &exact.denominator;
        let remainder = &exact.numerator % &exact.denominator;
        base_total += &floor;
        rows.push((*row_index, floor, remainder, exact.denominator));
    }
    let (rounded_total, remainder) = exact_total.numerator.div_rem(&exact_total.denominator);
    let rounded_total = if remainder * BigInt::from(2) >= exact_total.denominator {
        rounded_total + 1
    } else {
        rounded_total
    };
    if rounded_total != BigInt::from(target) {
        return None;
    }
    let remaining = BigInt::from(target) - base_total;
    let remaining = usize::try_from(remaining).ok()?;
    if remaining > rows.len() {
        return None;
    }
    rows.sort_by(|left, right| {
        let left_fraction = &left.2 * &right.3;
        let right_fraction = &right.2 * &left.3;
        right_fraction
            .cmp(&left_fraction)
            .then_with(|| left.0.cmp(&right.0))
    });
    let mut allocations = Vec::with_capacity(rows.len());
    for (rank, (row_index, floor, _, _)) in rows.into_iter().enumerate() {
        let mut amount = i64::try_from(floor).ok()?;
        if rank < remaining {
            amount = amount.checked_add(1)?;
        }
        allocations.push((row_index, amount));
    }
    Some(allocations)
}

fn greatest_common_divisor_i128(mut left: i128, mut right: i128) -> i128 {
    left = left.abs();
    right = right.abs();
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left.max(1)
}

fn has_live_meter_activity(actor: &ActorAccumulator) -> bool {
    actor.reported_damage != 0
        || actor.damage_taken != 0
        || actor.reported_healing != 0
        || actor.effective_healing != 0
        || actor.overheal != 0
        || actor.shielding != 0
        || actor.casts != 0
        || actor.hits != 0
        || actor.deaths != 0
        || actor.revives != 0
}

impl ReplayPlugin for CombatTimelinePlugin {
    fn descriptor(&self) -> ReplayPluginDescriptor {
        ReplayPluginDescriptor {
            id: COMBAT_METER_PLUGIN_ID.into(),
            name: "Combat timeline".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            capabilities: BTreeSet::from([
                PluginCapability::EventsRead,
                PluginCapability::EncountersRead,
            ]),
            subscriptions: BTreeSet::from([
                EventTopic::Actor,
                EventTopic::Combat,
                EventTopic::Encounter,
                EventTopic::Dungeon,
                EventTopic::DataQuality,
            ]),
        }
    }

    fn begin(
        &mut self,
        header: &RlogHeader,
        _: &mut PluginOutputSink<'_>,
    ) -> Result<(), PluginFailure> {
        self.begin_live(header);
        Ok(())
    }

    fn on_event(
        &mut self,
        envelope: &EventEnvelope,
        _: &mut PluginOutputSink<'_>,
    ) -> Result<(), PluginFailure> {
        self.observe_live(envelope);
        Ok(())
    }

    fn finish(&mut self, output: &mut PluginOutputSink<'_>) -> Result<(), PluginFailure> {
        self.finish_exact_contributions();
        if self.active_combat_started.is_some() {
            self.closed_at_log_end = true;
            self.end_combat(self.last_event_micros.unwrap_or_default());
        }
        output.snapshot(
            COMBAT_SNAPSHOT_SCHEMA_ID,
            COMBAT_SNAPSHOT_SCHEMA_VERSION,
            &self.live_snapshot()?,
        )
    }
}

fn nonnegative(value: i64) -> i64 {
    value.max(0)
}

fn contribution_status_state(state: StatusState) -> ContributionStatusState {
    match state {
        StatusState::Applied => ContributionStatusState::Applied,
        StatusState::Refreshed => ContributionStatusState::Refreshed,
        StatusState::Stacked => ContributionStatusState::Stacked,
        StatusState::Consumed => ContributionStatusState::Consumed,
        StatusState::Removed => ContributionStatusState::Removed,
    }
}

fn seconds(micros: u64) -> f64 {
    if micros == 0 {
        0.0
    } else {
        micros.max(MINIMUM_PERSONAL_ACTIVE_MICROS) as f64 / 1_000_000.0
    }
}

fn interval_overlap_micros(window: (u64, u64), intervals: &[(u64, u64)]) -> u64 {
    intervals
        .iter()
        .map(|(started, ended)| window.1.min(*ended).saturating_sub(window.0.max(*started)))
        .sum()
}

fn rate_per_second(value: i64, duration_seconds: f64) -> f64 {
    if duration_seconds <= 0.0 {
        0.0
    } else {
        value as f64 / duration_seconds
    }
}

fn history_fact_offset(
    observed_micros: u64,
    intervals: &[(u64, u64)],
    origin_micros: u64,
    compress_intervals: bool,
) -> Option<u64> {
    if !compress_intervals {
        return intervals
            .iter()
            .any(|(started, ended)| observed_micros >= *started && observed_micros <= *ended)
            .then(|| observed_micros.saturating_sub(origin_micros));
    }
    let mut projected_offset = 0_u64;
    for (started, ended) in intervals {
        if observed_micros >= *started && observed_micros <= *ended {
            return Some(projected_offset.saturating_add(observed_micros.saturating_sub(*started)));
        }
        projected_offset = projected_offset.saturating_add(ended.saturating_sub(*started));
    }
    None
}

fn history_segment_interval(run: &RunAnalysis, segment: &RunSegmentSummary) -> (u64, u64) {
    let started_micros = if is_boss_segment_kind(segment.kind) {
        segment
            .encounter_indices
            .iter()
            .filter_map(|index| run.encounters.get(*index as usize))
            .map(|encounter| encounter.started_micros)
            .min()
            .unwrap_or(segment.started_micros)
    } else {
        segment.started_micros
    };
    (
        started_micros.min(segment.ended_micros),
        segment.ended_micros,
    )
}

/// Reconstructs the elapsed-combat (eDPS) clock for a saved run.
///
/// Normal boss retries contribute every attempt's combat interval, but the
/// recovery gap between a wipe and the next pull is omitted. Gauntlet is the
/// deliberate exception: its bosses form one continuous encounter, so the
/// segment interval remains intact until the whole gauntlet completes.
fn history_segment_edps_intervals(
    run: &RunAnalysis,
    segment: &RunSegmentSummary,
) -> Vec<(u64, u64)> {
    match segment.kind {
        RunSegmentKind::Boss | RunSegmentKind::RaidBoss => {
            let intervals = segment
                .encounter_indices
                .iter()
                .filter_map(|index| run.encounters.get(*index as usize))
                .map(|encounter| {
                    (
                        encounter.started_micros.min(encounter.ended_micros),
                        encounter.ended_micros,
                    )
                })
                .collect::<Vec<_>>();
            if intervals.is_empty() {
                vec![history_segment_interval(run, segment)]
            } else {
                intervals
            }
        }
        RunSegmentKind::Gauntlet | RunSegmentKind::Mobbing | RunSegmentKind::Unknown => {
            vec![history_segment_interval(run, segment)]
        }
    }
}

fn is_boss_segment_kind(kind: RunSegmentKind) -> bool {
    matches!(
        kind,
        RunSegmentKind::Boss | RunSegmentKind::RaidBoss | RunSegmentKind::Gauntlet
    )
}

fn history_segment_view_intervals(
    run: &RunAnalysis,
    segment: &RunSegmentSummary,
) -> Vec<(u64, u64)> {
    match segment.kind {
        RunSegmentKind::Boss | RunSegmentKind::RaidBoss => segment
            .winning_attempt_index
            .and_then(|index| run.encounters.get(index as usize))
            .map(|encounter| vec![(encounter.started_micros, encounter.ended_micros)])
            .unwrap_or_default(),
        RunSegmentKind::Gauntlet => segment
            .successful_attempt_indices
            .iter()
            .filter_map(|index| run.encounters.get(*index as usize))
            .map(|encounter| (encounter.started_micros, encounter.ended_micros))
            .collect(),
        RunSegmentKind::Mobbing | RunSegmentKind::Unknown => {
            vec![history_segment_interval(run, segment)]
        }
    }
}

fn history_segment_view_active_combat_micros(
    run: &RunAnalysis,
    segment: &RunSegmentSummary,
) -> u64 {
    match segment.kind {
        RunSegmentKind::Boss | RunSegmentKind::RaidBoss => segment
            .winning_attempt_index
            .and_then(|index| run.encounters.get(index as usize))
            .map(|encounter| encounter.active_combat_micros)
            .unwrap_or_default(),
        RunSegmentKind::Gauntlet => segment
            .successful_attempt_indices
            .iter()
            .filter_map(|index| run.encounters.get(*index as usize))
            .map(|encounter| encounter.active_combat_micros)
            .sum(),
        RunSegmentKind::Mobbing | RunSegmentKind::Unknown => segment.active_combat_micros,
    }
}

struct ProjectedBestIntervals {
    intervals: Vec<(u64, u64)>,
    segment_indices: Vec<u32>,
    active_combat_micros: u64,
}

fn projected_best_intervals(run: &RunAnalysis) -> Option<ProjectedBestIntervals> {
    let mut intervals = Vec::new();
    let mut segment_indices = Vec::new();
    let mut active_combat_micros = 0_u64;
    let mut has_mobbing = false;
    let mut has_boss = false;
    for segment in &run.segments {
        match segment.kind {
            RunSegmentKind::Mobbing => {
                has_mobbing = true;
                intervals.push(history_segment_interval(run, segment));
                segment_indices.push(segment.index);
                active_combat_micros =
                    active_combat_micros.saturating_add(segment.active_combat_micros);
            }
            RunSegmentKind::Boss | RunSegmentKind::RaidBoss | RunSegmentKind::Gauntlet => {
                let selected_intervals = history_segment_view_intervals(run, segment);
                if selected_intervals.is_empty() {
                    continue;
                }
                has_boss = true;
                intervals.extend(selected_intervals);
                segment_indices.push(segment.index);
                active_combat_micros = active_combat_micros
                    .saturating_add(history_segment_view_active_combat_micros(run, segment));
            }
            RunSegmentKind::Unknown => {}
        }
    }
    if !has_mobbing || !has_boss || intervals.is_empty() {
        return None;
    }
    intervals.sort_unstable();
    segment_indices.sort_unstable();
    segment_indices.dedup();
    Some(ProjectedBestIntervals {
        intervals,
        segment_indices,
        active_combat_micros,
    })
}

fn segment_kind_label(kind: RunSegmentKind) -> &'static str {
    match kind {
        RunSegmentKind::Mobbing => "Mobbing",
        RunSegmentKind::Boss => "Bossing",
        RunSegmentKind::RaidBoss => "Raid boss",
        RunSegmentKind::Gauntlet => "Gauntlet",
        RunSegmentKind::Unknown => "Segment",
    }
}

fn run_terminal_state_name(state: RunTerminalState) -> &'static str {
    match state {
        RunTerminalState::Open => "open",
        RunTerminalState::Completed => "completed",
        RunTerminalState::Failed => "failed",
        RunTerminalState::Ended => "ended",
        RunTerminalState::Exited => "exited",
        RunTerminalState::Superseded => "superseded",
    }
}

fn encounter_terminal_state_name(state: EncounterTerminalState) -> &'static str {
    match state {
        EncounterTerminalState::Open => "open",
        EncounterTerminalState::Cleared => "cleared",
        EncounterTerminalState::Wiped => "wiped",
        EncounterTerminalState::Ended => "ended",
    }
}

fn wire_connection_id(provenance: &EventProvenance) -> Option<u64> {
    match &provenance.source {
        EvidenceSource::Wire { connection_id, .. } => Some(*connection_id),
        EvidenceSource::Derived { .. } | EvidenceSource::Manual { .. } => None,
    }
}

fn encounter_state_name(state: EncounterState) -> &'static str {
    match state {
        EncounterState::Started => "started",
        EncounterState::Cleared => "cleared",
        EncounterState::Wiped => "wiped",
        EncounterState::Ended => "ended",
    }
}

fn actor_kind_name(kind: ActorKind) -> String {
    match kind {
        ActorKind::Player => "player".into(),
        ActorKind::Monster => "monster".into(),
        ActorKind::Npc => "npc".into(),
        ActorKind::SceneObject => "scene_object".into(),
        ActorKind::Zone => "zone".into(),
        ActorKind::Projectile => "projectile".into(),
        ActorKind::Pet => "pet".into(),
        ActorKind::TrainingDummy => "training_dummy".into(),
        ActorKind::Drop => "drop".into(),
        ActorKind::Field => "field".into(),
        ActorKind::Trap => "trap".into(),
        ActorKind::Collection => "collection".into(),
        ActorKind::StaticObject => "static_object".into(),
        ActorKind::Vehicle => "vehicle".into(),
        ActorKind::Toy => "toy".into(),
        ActorKind::Housing => "housing".into(),
        ActorKind::Unknown(value) => format!("unknown:{value}"),
    }
}

fn history_target_is_selectable(actor_kind: Option<&str>) -> bool {
    !matches!(actor_kind, Some("pet" | "training_dummy"))
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::BufReader;
    use std::path::Path;

    use rlogs_combat::{
        ActivityKind, CombatWindowSummary, EncounterKind, EncounterSummary, EncounterTerminalState,
        RunIdentity, RunSegmentSummary, RunSubmissionDisposition, RunTiming,
    };
    use rlogs_events::{
        AbilityId, ActorEvent, ActorId, CanonicalEventDraft, CanonicalEventDraftKind, DamageEvent,
        DamageFlags, DungeonEvent, EntityRef, EntityUuid, EventEnvelopeFactory, EventProvenance,
        EventSensitivity, EventTime, StatusEffectId, StatusEffectInstanceId, StatusEvent,
    };
    use rlogs_log_format::{RlogLimits, RlogReader};
    use rlogs_plugin_runtime::{PluginOutput, PluginRunLimits, replay_rlog};

    #[test]
    fn formal_history_target_selector_excludes_only_owned_nonencounter_helpers() {
        assert!(!history_target_is_selectable(Some("pet")));
        assert!(!history_target_is_selectable(Some("training_dummy")));
        assert!(history_target_is_selectable(Some("projectile")));
        assert!(history_target_is_selectable(Some("monster")));
    }

    #[test]
    fn cast_starts_are_counted_independently_from_landed_hits() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/fixtures/replay/reference-combat.rlog");
        let reader = RlogReader::new(
            BufReader::new(File::open(fixture).unwrap()),
            RlogLimits::default(),
        )
        .unwrap();
        let mut plugin = CombatTimelinePlugin::new();
        plugin.begin_live(reader.header());
        let mut factory = EventEnvelopeFactory::new(
            reader.header().session_id.clone(),
            reader.header().region.clone(),
        );
        let source = EntityRef {
            actor_id: ActorId(1),
            entity_uuid: EntityUuid(101),
        };
        let target = EntityRef {
            actor_id: ActorId(2),
            entity_uuid: EntityUuid(102),
        };
        let ability = AbilityId(2_233);

        for (observed_micros, state) in [
            (1_000_000, CastState::Started),
            (1_100_000, CastState::Completed),
        ] {
            let cast = factory
                .emit(CanonicalEventDraft {
                    time: EventTime {
                        observed_micros,
                        game_time_millis: None,
                    },
                    provenance: EventProvenance::wire(observed_micros, 1, observed_micros),
                    sensitivity: EventSensitivity::PublicGameplay,
                    kind: CanonicalEventDraftKind::Timeline(TimelineEventKind::Cast(
                        rlogs_events::CastEvent {
                            source,
                            ability,
                            target: Some(target),
                            state,
                            action_timing: None,
                        },
                    )),
                })
                .unwrap();
            plugin.observe_live(&cast);
        }

        let missed = plugin.live_snapshot().unwrap();
        let actor = missed
            .actors
            .iter()
            .find(|actor| actor.actor_id == "1")
            .unwrap();
        assert_eq!(actor.casts, 1, "completion must not count as another cast");
        assert_eq!(actor.hits, 0, "a ranged miss still has no landed hit");
        assert_eq!(actor.abilities[0].casts, 1);
        assert_eq!(actor.abilities[0].hits, 0);
        assert_eq!(
            plugin
                .history_facts
                .iter()
                .filter(|fact| matches!(fact.kind, CombatFactKind::Cast))
                .count(),
            1
        );

        for sequence in 0..2_u64 {
            let damage = factory
                .emit(CanonicalEventDraft {
                    time: EventTime {
                        observed_micros: 2_000_000 + sequence,
                        game_time_millis: None,
                    },
                    provenance: EventProvenance::wire(2_000_000 + sequence, 1, sequence),
                    sensitivity: EventSensitivity::PublicGameplay,
                    kind: CanonicalEventDraftKind::Timeline(TimelineEventKind::Damage(
                        DamageEvent {
                            source,
                            direct_source: None,
                            target,
                            ability: Some(ability),
                            amount: 100,
                            actual_amount: Some(100),
                            hp_loss: Some(100),
                            shield_loss: None,
                            hit_event_id: None,
                            damage_source: None,
                            damage_type: None,
                            flags: DamageFlags::default(),
                            packet: Default::default(),
                        },
                    )),
                })
                .unwrap();
            plugin.observe_live(&damage);
        }

        let multi_hit = plugin.live_snapshot().unwrap();
        let actor = multi_hit
            .actors
            .iter()
            .find(|actor| actor.actor_id == "1")
            .unwrap();
        assert_eq!(actor.casts, 1);
        assert_eq!(actor.hits, 2, "one cast may land more than one hit");
        assert_eq!(actor.abilities[0].casts, 1);
        assert_eq!(actor.abilities[0].hits, 2);
    }

    use super::*;

    #[test]
    fn history_selects_the_latest_entry_for_each_run_start() {
        let mut plugin = CombatTimelinePlugin::new();
        plugin.record_run_entry(10);
        plugin.record_run_entry(10);
        plugin.record_run_entry(100);

        assert_eq!(plugin.run_entered_micros, vec![10, 100]);
        assert_eq!(plugin.run_entry_for(5), None);
        assert_eq!(plugin.run_entry_for(20), Some(10));
        assert_eq!(plugin.run_entry_for(150), Some(100));
    }

    #[test]
    fn live_rates_keep_run_encounter_and_active_clocks_distinct_and_frozen() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/fixtures/replay/reference-combat.rlog");
        let reader = RlogReader::new(
            BufReader::new(File::open(fixture).unwrap()),
            RlogLimits::default(),
        )
        .unwrap();
        let mut plugin = CombatTimelinePlugin::new();
        plugin.begin_live(reader.header());
        plugin.record_run_entry(1_000_000);
        plugin.begin_combat(5_000_000);
        let actor = plugin.actor_mut(1, 100);
        actor.reported_damage = 9_000;
        actor.damage_during_combat = 9_000;
        plugin.last_event_micros = Some(10_000_000);
        plugin.finish_encounter(10_000_000);
        plugin.mark_run_terminal(12_000_000);
        // Later packets must not extend either completed denominator.
        plugin.last_event_micros = Some(20_000_000);

        let snapshot = plugin.live_snapshot().unwrap();
        assert_eq!(snapshot.encounter_elapsed_micros, Some(5_000_000));
        assert_eq!(snapshot.run_elapsed_micros, Some(11_000_000));
        assert_eq!(snapshot.encounter_terminal_micros, Some(10_000_000));
        assert_eq!(snapshot.run_terminal_micros, Some(12_000_000));
        let actor = snapshot
            .actors
            .iter()
            .find(|actor| actor.actor_id == "1")
            .unwrap();
        assert!((actor.run_dps - 1_800.0).abs() < 0.001);
        assert!((actor.encounter_dps - 1_800.0).abs() < 0.001);
        assert!((actor.active_dps - 1_800.0).abs() < 0.001);
    }

    #[test]
    fn gauntlet_boss_clear_keeps_one_encounter_clock_until_run_completion() {
        let mut plugin = CombatTimelinePlugin::new().with_continuous_encounter_scenes([7_777]);
        plugin.scene_id = Some(7_777);
        plugin.begin_combat(5_000_000);
        plugin.finish_encounter(10_000_000);

        assert_eq!(plugin.encounter_combat_started, Some(5_000_000));
        assert_eq!(plugin.encounter_terminal_micros, None);
        assert_eq!(plugin.active_combat_started, None);

        plugin.begin_combat(15_000_000);
        assert_eq!(plugin.encounter_combat_started, Some(5_000_000));
        plugin.mark_run_terminal(20_000_000);
        assert_eq!(plugin.encounter_terminal_micros, Some(20_000_000));
    }

    #[test]
    fn retry_resets_attempt_rates_but_pauses_and_resumes_cumulative_edps() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/fixtures/replay/reference-combat.rlog");
        let reader = RlogReader::new(
            BufReader::new(File::open(fixture).unwrap()),
            RlogLimits::default(),
        )
        .unwrap();
        let mut plugin = CombatTimelinePlugin::new();
        plugin.begin_live(reader.header());
        plugin.record_run_entry(0);

        plugin.begin_combat(1_000_000);
        {
            let actor = plugin.actor_mut(1, 100);
            actor.actor_kind = Some("player".into());
            actor.reported_damage = 3_000;
            actor.damage_during_combat = 3_000;
        }
        plugin.run_damage_during_combat.insert(1, 3_000);
        plugin.last_event_micros = Some(4_000_000);
        plugin.reset_live_attempt(4_000_000);

        // Recovery packets do not advance eDPS, while attempt-scoped rates
        // have already reset to zero.
        plugin.last_event_micros = Some(10_000_000);
        let recovery = plugin.live_snapshot().unwrap();
        assert_eq!(recovery.attempt_elapsed_micros, None);
        assert_eq!(recovery.encounter_elapsed_micros, Some(3_000_000));
        let actor = recovery
            .actors
            .iter()
            .find(|actor| actor.actor_id == "1")
            .unwrap();
        assert_eq!(actor.run_dps, 0.0);
        assert_eq!(actor.active_dps, 0.0);
        assert!((actor.encounter_dps - 1_000.0).abs() < 0.001);

        // The retry's first hostile event resumes the cumulative clock. New
        // attempt DPS/aDPS use only the retry, while eDPS includes both pulls.
        plugin.begin_combat(10_000_000);
        {
            let actor = plugin.actor_mut(1, 100);
            actor.actor_kind = Some("player".into());
            actor.reported_damage = 2_000;
            actor.damage_during_combat = 2_000;
        }
        plugin.run_damage_during_combat.insert(1, 5_000);
        plugin.last_event_micros = Some(12_000_000);
        let retry = plugin.live_snapshot().unwrap();
        assert_eq!(retry.attempt_elapsed_micros, Some(2_000_000));
        assert_eq!(retry.encounter_elapsed_micros, Some(5_000_000));
        let actor = retry
            .actors
            .iter()
            .find(|actor| actor.actor_id == "1")
            .unwrap();
        assert!((actor.run_dps - 1_000.0).abs() < 0.001);
        assert!((actor.active_dps - 1_000.0).abs() < 0.001);
        assert!((actor.encounter_dps - 1_000.0).abs() < 0.001);
    }

    #[test]
    fn exact_influence_rows_preserve_damage_identity_and_exact_amounts() {
        let mut influences = BTreeMap::new();
        let base = HistoryDamageInfluenceObservation {
            observed_micros: 1_000,
            effect_id: 55_228,
            scope: DamageContributionScope::Component("target-vulnerability"),
            provider_actor_id: 1,
            provider_entity_uuid: 101,
            recipient_actor_id: 2,
            recipient_entity_uuid: 102,
            damage_event_sequence: Some(10),
            affected_ability_id: Some(2_206_290),
            affected_target: Some((3, 103)),
            critical: Some(true),
            observed_damage: 100_000,
            exact_integer_delta: Some(5_000),
            exact_rational_delta: None,
        };
        observe_history_damage_influence(&mut influences, base);
        observe_history_damage_influence(
            &mut influences,
            HistoryDamageInfluenceObservation {
                observed_micros: 2_000,
                damage_event_sequence: Some(11),
                observed_damage: 120_000,
                exact_integer_delta: Some(6_000),
                ..base
            },
        );
        // A second exact term for the same canonical damage event must not
        // duplicate its observed damage context or event count.
        observe_history_damage_influence(
            &mut influences,
            HistoryDamageInfluenceObservation {
                observed_micros: 2_000,
                damage_event_sequence: Some(11),
                observed_damage: 120_000,
                exact_integer_delta: None,
                exact_rational_delta: Some((2, 4)),
                ..base
            },
        );

        let rows = finish_history_damage_influences(
            influences,
            &[EffectDamageContribution {
                effect_id: 55_228,
                provider_actor_id: 1,
                recipient_actor_id: 2,
                amount: 1,
            }],
        );
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.effect_id, "55228");
        assert_eq!(
            row.attribution_component.as_deref(),
            Some("target-vulnerability")
        );
        assert_eq!(row.affected_ability_id.as_deref(), Some("2206290"));
        assert_eq!(row.target_actor_id.as_deref(), Some("3"));
        assert_eq!(row.damage_event_count, 2);
        assert_eq!(row.critical_hit_count, Some(2));
        assert_eq!(row.observed_damage, "220000");
        assert_eq!(row.exact_integer_delta, "11000");
        assert_eq!(row.exact_rational_deltas.len(), 1);
        assert_eq!(row.exact_rational_deltas[0].numerator, "1");
        assert_eq!(row.exact_rational_deltas[0].denominator, "2");
        assert_eq!(row.attributed_rdps.as_deref(), Some("11001"));
        assert!(row.damage_context_complete);
    }

    #[test]
    fn rational_influence_rows_use_stable_conserved_largest_remainders() {
        let mut influences = BTreeMap::new();
        let base = HistoryDamageInfluenceObservation {
            observed_micros: 1_000,
            effect_id: 55_228,
            scope: DamageContributionScope::Component("target-vulnerability"),
            provider_actor_id: 1,
            provider_entity_uuid: 101,
            recipient_actor_id: 2,
            recipient_entity_uuid: 102,
            damage_event_sequence: Some(10),
            affected_ability_id: Some(5_001),
            affected_target: Some((3, 103)),
            critical: Some(false),
            observed_damage: 100,
            exact_integer_delta: Some(2),
            exact_rational_delta: Some((1, 3)),
        };
        observe_history_damage_influence(&mut influences, base);
        observe_history_damage_influence(
            &mut influences,
            HistoryDamageInfluenceObservation {
                damage_event_sequence: Some(11),
                affected_ability_id: Some(5_002),
                exact_integer_delta: Some(3),
                ..base
            },
        );

        let rows = finish_history_damage_influences(
            influences.clone(),
            &[EffectDamageContribution {
                effect_id: 55_228,
                provider_actor_id: 1,
                recipient_actor_id: 2,
                amount: 1,
            }],
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].affected_ability_id.as_deref(), Some("5001"));
        assert_eq!(rows[0].attributed_rdps.as_deref(), Some("3"));
        assert_eq!(rows[1].affected_ability_id.as_deref(), Some("5002"));
        assert_eq!(rows[1].attributed_rdps.as_deref(), Some("3"));
        assert_eq!(
            rows.iter()
                .map(|row| row
                    .attributed_rdps
                    .as_deref()
                    .unwrap()
                    .parse::<i64>()
                    .unwrap())
                .sum::<i64>(),
            6
        );

        let mismatched = finish_history_damage_influences(
            influences,
            &[EffectDamageContribution {
                effect_id: 55_228,
                provider_actor_id: 1,
                recipient_actor_id: 2,
                amount: 0,
            }],
        );
        assert!(mismatched.iter().all(|row| row.attributed_rdps.is_none()));
    }

    #[test]
    fn unproven_hp_scaled_damage_remains_in_the_meter_without_attribution() {
        #[derive(Debug)]
        struct NoProofProjector;

        impl ExactDamageContributionProjector for NoProofProjector {
            fn enabled(&self) -> bool {
                true
            }

            fn reset(&mut self) {}

            fn observe(
                &mut self,
                _envelope: &EventEnvelope,
                _output: &mut Vec<ExactDamageContributionEvent>,
                _rational_output: &mut Vec<ExactRationalDamageContributionEvent>,
            ) {
                // An unresolved HP formula produces no attribution fact. The
                // canonical damage event must still pass through unchanged.
            }
        }

        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/fixtures/replay/reference-combat.rlog");
        let reader = RlogReader::new(
            BufReader::new(File::open(fixture).unwrap()),
            RlogLimits::default(),
        )
        .unwrap();
        let mut header = reader.header().clone();
        header.session_id = "unproven-hp-scaled-damage".into();
        let mut plugin = CombatTimelinePlugin::with_damage_contribution_projection(
            Vec::new(),
            Some(Box::new(NoProofProjector)),
        )
        .unwrap();
        plugin.begin_live(&header);

        let source = EntityRef {
            actor_id: ActorId(1),
            entity_uuid: EntityUuid(101),
        };
        let target = EntityRef {
            actor_id: ActorId(2),
            entity_uuid: EntityUuid(102),
        };
        let mut factory =
            EventEnvelopeFactory::new(header.session_id.clone(), header.region.clone());
        let envelope = factory
            .emit(CanonicalEventDraft {
                time: EventTime {
                    observed_micros: 1_000_000,
                    game_time_millis: None,
                },
                provenance: EventProvenance::wire(1_000_000, 1, 1),
                sensitivity: EventSensitivity::PublicGameplay,
                kind: CanonicalEventDraftKind::Timeline(TimelineEventKind::Damage(DamageEvent {
                    source,
                    direct_source: None,
                    target,
                    ability: Some(AbilityId(2_206_290)),
                    amount: 2_737_001,
                    actual_amount: Some(2_737_001),
                    hp_loss: Some(2_737_001),
                    shield_loss: None,
                    hit_event_id: None,
                    damage_source: None,
                    damage_type: None,
                    flags: DamageFlags::default(),
                    packet: Default::default(),
                })),
            })
            .unwrap();
        plugin.observe_live(&envelope);

        let snapshot = plugin.live_snapshot().unwrap();
        let actor = snapshot
            .actors
            .iter()
            .find(|actor| actor.actor_id == "1")
            .unwrap();
        assert_eq!(actor.reported_damage, 2_737_001);
        assert_eq!(actor.effective_damage, 2_737_001);
        assert_eq!(actor.hp_damage, 2_737_001);
        assert_eq!(actor.rdps_damage, Some(2_737_001));
        assert_eq!(actor.rdps_contribution_given, Some(0));
        assert_eq!(actor.rdps_contribution_received, Some(0));

        let history = plugin.build_history_view(&HistoryViewSpec {
            id: "all".into(),
            label: "Entire run".into(),
            kind: "all".into(),
            segment_indices: vec![0],
            intervals: vec![(0, 2_000_000)],
            elapsed_micros: 2_000_000,
            active_combat_micros: 1_000_000,
            compress_intervals: false,
        });
        let actor = history
            .actors
            .iter()
            .find(|actor| actor.actor_id == "1")
            .unwrap();
        assert_eq!(actor.damage, 2_737_001);
        assert_eq!(actor.rdps_damage, Some(2_737_001));
        assert_eq!(actor.rdps, Some(2_737_001.0));
        assert_eq!(actor.rdps_contribution_given, Some(0));
        assert_eq!(actor.rdps_contribution_received, Some(0));
    }

    #[test]
    fn unresolved_external_formula_marks_known_rdps_subtotal_incomplete() {
        #[derive(Debug)]
        struct IncompleteProjector;

        impl ExactDamageContributionProjector for IncompleteProjector {
            fn enabled(&self) -> bool {
                true
            }

            fn incomplete_rdps_actor_ids(&self) -> Vec<u64> {
                vec![1]
            }

            fn reset(&mut self) {}

            fn observe(
                &mut self,
                _envelope: &EventEnvelope,
                _output: &mut Vec<ExactDamageContributionEvent>,
                _rational_output: &mut Vec<ExactRationalDamageContributionEvent>,
            ) {
            }
        }

        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/fixtures/replay/reference-combat.rlog");
        let reader = RlogReader::new(
            BufReader::new(File::open(fixture).unwrap()),
            RlogLimits::default(),
        )
        .unwrap();
        let mut header = reader.header().clone();
        header.session_id = "incomplete-external-formula".into();
        let mut plugin = CombatTimelinePlugin::with_damage_contribution_projection(
            Vec::new(),
            Some(Box::new(IncompleteProjector)),
        )
        .unwrap();
        plugin.begin_live(&header);

        let mut factory =
            EventEnvelopeFactory::new(header.session_id.clone(), header.region.clone());
        let envelope = factory
            .emit(CanonicalEventDraft {
                time: EventTime {
                    observed_micros: 1_000_000,
                    game_time_millis: None,
                },
                provenance: EventProvenance::wire(1_000_000, 1, 1),
                sensitivity: EventSensitivity::PublicGameplay,
                kind: CanonicalEventDraftKind::Timeline(TimelineEventKind::Damage(DamageEvent {
                    source: EntityRef {
                        actor_id: ActorId(1),
                        entity_uuid: EntityUuid(101),
                    },
                    direct_source: None,
                    target: EntityRef {
                        actor_id: ActorId(2),
                        entity_uuid: EntityUuid(102),
                    },
                    ability: Some(AbilityId(2_206_290)),
                    amount: 2_737_001,
                    actual_amount: Some(2_737_001),
                    hp_loss: Some(2_737_001),
                    shield_loss: None,
                    hit_event_id: None,
                    damage_source: None,
                    damage_type: None,
                    flags: DamageFlags::default(),
                    packet: Default::default(),
                })),
            })
            .unwrap();
        plugin.observe_live(&envelope);

        let snapshot = plugin.live_snapshot().unwrap();
        let actor = snapshot
            .actors
            .iter()
            .find(|actor| actor.actor_id == "1")
            .unwrap();
        assert_eq!(actor.reported_damage, 2_737_001);
        assert_eq!(actor.rdps_damage, Some(2_737_001));
        assert_eq!(actor.rdps, Some(2_737_001.0));
        assert_eq!(actor.rdps_contribution_given, Some(0));
        assert_eq!(actor.rdps_contribution_received, Some(0));
        assert!(actor.rdps_incomplete);
    }

    #[test]
    fn latest_projection_view_reuses_the_meter_projector_output_and_clears_next_event() {
        #[derive(Debug)]
        struct OneContributionProjector;

        impl ExactDamageContributionProjector for OneContributionProjector {
            fn enabled(&self) -> bool {
                true
            }

            fn reset(&mut self) {}

            fn observe(
                &mut self,
                envelope: &EventEnvelope,
                output: &mut Vec<ExactDamageContributionEvent>,
                rational_output: &mut Vec<ExactRationalDamageContributionEvent>,
            ) {
                if matches!(
                    &envelope.event,
                    CanonicalEvent::Timeline(timeline)
                        if matches!(timeline.kind, TimelineEventKind::Damage(_))
                ) {
                    output.push(ExactDamageContributionEvent {
                        observed_micros: envelope.time.observed_micros,
                        effect_id: 9001,
                        provider_actor_id: 1,
                        recipient_actor_id: 2,
                        scope: DamageContributionScope::Component("test-integer"),
                        amount: 10,
                        observed_damage: 100,
                        included: true,
                    });
                    rational_output.push(ExactRationalDamageContributionEvent {
                        observed_micros: 500,
                        effect_id: 9001,
                        provider_actor_id: 1,
                        recipient_actor_id: 2,
                        scope: DamageContributionScope::Component("test-rational"),
                        numerator: 1,
                        denominator: 2,
                        observed_damage: 100,
                        included: true,
                        deferred_damage_context: Some(
                            rlogs_combat::DeferredDamageContributionContext {
                                event_sequence: 77,
                                recipient_entity_uuid: 102,
                                affected_ability_id: Some(88),
                                target_actor_id: 9,
                                target_entity_uuid: 909,
                            },
                        ),
                    });
                }
            }

            fn finish(
                &mut self,
                _output: &mut Vec<ExactDamageContributionEvent>,
                rational_output: &mut Vec<ExactRationalDamageContributionEvent>,
            ) {
                rational_output.push(ExactRationalDamageContributionEvent {
                    observed_micros: 600,
                    effect_id: 9002,
                    provider_actor_id: 1,
                    recipient_actor_id: 2,
                    scope: DamageContributionScope::Component("test-finish-rational"),
                    numerator: 1,
                    denominator: 4,
                    observed_damage: 100,
                    included: true,
                    deferred_damage_context: Some(
                        rlogs_combat::DeferredDamageContributionContext {
                            event_sequence: 78,
                            recipient_entity_uuid: 102,
                            affected_ability_id: Some(89),
                            target_actor_id: 9,
                            target_entity_uuid: 909,
                        },
                    ),
                });
            }
        }

        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/fixtures/replay/reference-combat.rlog");
        let reader = RlogReader::new(
            BufReader::new(File::open(fixture).unwrap()),
            RlogLimits::default(),
        )
        .unwrap();
        let mut header = reader.header().clone();
        header.session_id = "latest-projection-view".into();
        let mut plugin = CombatTimelinePlugin::with_damage_contribution_projection(
            Vec::new(),
            Some(Box::new(OneContributionProjector)),
        )
        .unwrap();
        plugin.begin_live(&header);
        let mut factory =
            EventEnvelopeFactory::new(header.session_id.clone(), header.region.clone());
        let source = EntityRef {
            actor_id: ActorId(2),
            entity_uuid: EntityUuid(102),
        };
        let target = EntityRef {
            actor_id: ActorId(3),
            entity_uuid: EntityUuid(103),
        };
        let damage = factory
            .emit(CanonicalEventDraft {
                time: EventTime {
                    observed_micros: 1_000,
                    game_time_millis: None,
                },
                provenance: EventProvenance::wire(1_000, 1, 1),
                sensitivity: EventSensitivity::PublicGameplay,
                kind: CanonicalEventDraftKind::Timeline(TimelineEventKind::Damage(DamageEvent {
                    source,
                    direct_source: None,
                    target,
                    ability: Some(AbilityId(55)),
                    amount: 100,
                    actual_amount: Some(100),
                    hp_loss: Some(100),
                    shield_loss: None,
                    hit_event_id: None,
                    damage_source: None,
                    damage_type: None,
                    flags: DamageFlags::default(),
                    packet: Default::default(),
                })),
            })
            .unwrap();
        plugin.observe_live(&damage);
        assert_eq!(plugin.latest_exact_contributions().len(), 1);
        assert_eq!(plugin.latest_exact_rational_contributions().len(), 1);
        assert_eq!(plugin.latest_exact_contributions()[0].amount, 10);
        let deferred_fact = plugin
            .history_facts
            .iter()
            .find(|fact| {
                matches!(
                    fact.kind,
                    CombatFactKind::ExactRationalDamageContribution {
                        effect_id: 9001,
                        ..
                    }
                )
            })
            .expect("deferred rational contribution should remain auditable");
        assert_eq!(deferred_fact.observed_micros, 500);
        assert_eq!(deferred_fact.target, Some((2, 102)));
        assert!(matches!(
            deferred_fact.kind,
            CombatFactKind::ExactRationalDamageContribution {
                damage_event_sequence: Some(77),
                affected_ability_id: Some(88),
                affected_target: Some((9, 909)),
                ..
            }
        ));
        let live_detail = plugin.live_overlay_snapshot().unwrap();
        assert!(!live_detail.rdps_damage_influences_truncated);
        let integer_row = live_detail
            .rdps_damage_influences
            .iter()
            .find(|row| row.attribution_component.as_deref() == Some("test-integer"))
            .expect("integer contribution should flow to live skill detail");
        assert_eq!(integer_row.effect_id, "9001");
        assert_eq!(integer_row.provider_actor_id, "1");
        assert_eq!(integer_row.recipient_actor_id, "2");
        assert_eq!(integer_row.affected_ability_id.as_deref(), Some("55"));
        assert_eq!(integer_row.attributed_rdps.as_deref(), Some("10"));
        let rational_row = live_detail
            .rdps_damage_influences
            .iter()
            .find(|row| row.attribution_component.as_deref() == Some("test-rational"))
            .expect("rational contribution should flow to live skill detail");
        assert_eq!(rational_row.affected_ability_id.as_deref(), Some("88"));
        assert_eq!(rational_row.attributed_rdps.as_deref(), Some("1"));

        let movement = factory
            .emit(CanonicalEventDraft {
                time: EventTime {
                    observed_micros: 2_000,
                    game_time_millis: None,
                },
                provenance: EventProvenance::wire(2_000, 1, 2),
                sensitivity: EventSensitivity::PublicGameplay,
                kind: CanonicalEventDraftKind::Timeline(TimelineEventKind::Position(
                    rlogs_events::PositionEvent {
                        actor: source,
                        x: 1.0,
                        y: 2.0,
                        z: 3.0,
                        facing_radians: None,
                    },
                )),
            })
            .unwrap();
        plugin.observe_live(&movement);
        assert!(plugin.latest_exact_contributions().is_empty());
        assert!(plugin.latest_exact_rational_contributions().is_empty());

        plugin.finish_exact_contributions();
        assert_eq!(plugin.latest_exact_rational_contributions().len(), 1);
        assert_eq!(
            plugin.latest_exact_rational_contributions()[0].effect_id,
            9002
        );
        assert!(plugin.history_facts.iter().any(|fact| {
            matches!(
                fact.kind,
                CombatFactKind::ExactRationalDamageContribution {
                    effect_id: 9002,
                    damage_event_sequence: Some(78),
                    affected_ability_id: Some(89),
                    affected_target: Some((9, 909)),
                    ..
                }
            )
        }));
    }

    #[test]
    fn reference_replay_produces_exact_combat_and_movement_totals() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/fixtures/replay/reference-combat.rlog");
        let report = replay_rlog(
            BufReader::new(File::open(fixture).unwrap()),
            CombatTimelinePlugin::new(),
            RlogLimits::default(),
            PluginRunLimits::default(),
        )
        .unwrap();
        assert_eq!(report.rlog.event_count, 13);
        assert_eq!(report.metrics.events_delivered, 13);
        let PluginOutput::Snapshot {
            schema_id,
            schema_version,
            payload,
        } = &report.outputs[0]
        else {
            panic!("expected combat snapshot");
        };
        assert_eq!(schema_id, COMBAT_SNAPSHOT_SCHEMA_ID);
        assert_eq!(*schema_version, COMBAT_SNAPSHOT_SCHEMA_VERSION);
        let snapshot: CombatTimelineSnapshot = serde_json::from_value(payload.clone()).unwrap();
        assert_eq!(snapshot.active_combat_micros, 10_000_000);
        assert_eq!(snapshot.combat_window_count, 1);
        assert!(!snapshot.combat_active);
        assert_eq!(snapshot.last_hostile_micros, None);
        assert_eq!(snapshot.combat_inactivity_timeout_micros, 8_000_000);
        assert_eq!(snapshot.encounter_state.as_deref(), Some("cleared"));
        assert!(!snapshot.closed_at_log_end);

        let player = snapshot
            .actors
            .iter()
            .find(|actor| actor.actor_id == "1")
            .unwrap();
        assert_eq!(player.reported_damage, 20_000);
        assert_eq!(player.effective_damage, 20_000);
        assert_eq!(player.damage_during_combat, 20_000);
        assert_eq!(player.dps, 2_000.0);
        assert_eq!(player.reported_healing, 3_000);
        assert_eq!(player.effective_healing, 2_000);
        assert_eq!(player.overheal, 1_000);
        assert_eq!(player.hps, 300.0);
        assert_eq!(player.tps, 0.0);
        assert_eq!(player.casts, 1);
        assert_eq!(player.hits, 2);
        assert_eq!(player.critical_hits, 1);
        assert_eq!(player.position_samples, 2);
        assert_eq!(player.path_distance, 5.0);

        let boss = snapshot
            .actors
            .iter()
            .find(|actor| actor.actor_id == "2")
            .unwrap();
        assert_eq!(boss.deaths, 1);
        assert_eq!(boss.damage_taken, 20_000);
        assert_eq!(boss.tps, 2_000.0);
    }

    #[test]
    fn live_snapshot_includes_the_still_open_combat_window() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/fixtures/replay/reference-combat.rlog");
        let mut reader = RlogReader::new(
            BufReader::new(File::open(fixture).unwrap()),
            RlogLimits::default(),
        )
        .unwrap();
        let mut plugin = CombatTimelinePlugin::new();
        plugin.begin_live(reader.header());
        for _ in 0..10 {
            plugin.observe_live(&reader.next_event().unwrap().unwrap());
        }

        let snapshot = plugin.live_snapshot().unwrap();
        assert_eq!(snapshot.active_combat_micros, 7_000_000);
        assert!(snapshot.combat_active);
        assert!(snapshot.last_hostile_micros.is_some());
        assert!(snapshot.latest_event_micros.is_some());
        assert_eq!(snapshot.combat_ended_micros, None);
        let player = snapshot
            .actors
            .iter()
            .find(|actor| actor.actor_id == "1")
            .unwrap();
        assert!((player.dps - (20_000.0 / 7.0)).abs() < 0.001);

        let overlay = plugin.live_overlay_snapshot().unwrap();
        assert_eq!(overlay.actors.len(), 2);
        assert!(overlay.actors.iter().any(|actor| actor.actor_id == "1"));
        let damaged_target = overlay
            .actors
            .iter()
            .find(|actor| actor.actor_id == "2")
            .unwrap();
        assert_eq!(damaged_target.damage_taken, 20_000);
    }

    #[test]
    fn first_damage_immediately_produces_live_encounter_dps() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/fixtures/replay/reference-combat.rlog");
        let reader = RlogReader::new(
            BufReader::new(File::open(fixture).unwrap()),
            RlogLimits::default(),
        )
        .unwrap();
        let mut header = reader.header().clone();
        header.session_id = "first-damage-edps".into();
        let mut factory =
            EventEnvelopeFactory::new(header.session_id.clone(), header.region.clone());
        let mut plugin = CombatTimelinePlugin::new();
        plugin.begin_live(&header);
        let envelope = factory
            .emit(CanonicalEventDraft {
                time: EventTime {
                    observed_micros: 5_000_000,
                    game_time_millis: None,
                },
                provenance: EventProvenance::wire(5_000_000, 1, 1),
                sensitivity: EventSensitivity::PublicGameplay,
                kind: CanonicalEventDraftKind::Timeline(TimelineEventKind::Damage(DamageEvent {
                    source: EntityRef {
                        actor_id: ActorId(1),
                        entity_uuid: EntityUuid(100),
                    },
                    direct_source: None,
                    target: EntityRef {
                        actor_id: ActorId(2),
                        entity_uuid: EntityUuid(200),
                    },
                    ability: Some(AbilityId(55)),
                    amount: 9_000,
                    actual_amount: Some(9_000),
                    hp_loss: Some(9_000),
                    shield_loss: None,
                    hit_event_id: None,
                    damage_source: None,
                    damage_type: None,
                    flags: DamageFlags::default(),
                    packet: Default::default(),
                })),
            })
            .unwrap();
        plugin.observe_live(&envelope);

        let snapshot = plugin.live_snapshot().unwrap();
        assert_eq!(snapshot.active_combat_micros, 0);
        assert!(snapshot.combat_active);
        assert_eq!(snapshot.last_hostile_micros, Some(5_000_000));
        assert_eq!(snapshot.latest_event_micros, Some(5_000_000));
        let player = snapshot
            .actors
            .iter()
            .find(|actor| actor.actor_id == "1")
            .unwrap();
        assert_eq!(player.damage_during_combat, 9_000);
        assert_eq!(player.dps, 9_000.0);
        assert_eq!(player.hps, 0.0);
        assert_eq!(player.tps, 0.0);
        let target = snapshot
            .actors
            .iter()
            .find(|actor| actor.actor_id == "2")
            .unwrap();
        assert_eq!(target.damage_taken, 9_000);
        assert_eq!(target.tps, 9_000.0);
        assert_eq!(
            plugin.history_active_combat_micros(0, &[(5_000_000, 5_000_000)]),
            MINIMUM_PERSONAL_ACTIVE_MICROS
        );
    }

    #[test]
    fn exact_breakdown_identity_splits_one_raw_wire_action_without_rewriting_audit_identity() {
        fn legacy_breakdown_resolver(
            raw_ability_id: i64,
            hit_event_id: Option<i32>,
            _damage_source: Option<i32>,
        ) -> Option<i64> {
            match (raw_ability_id, hit_event_id) {
                (2_203_291, Some(7)) => Some(2_220_329_107),
                (2_203_291, Some(9)) => Some(2_220_329_109),
                _ => None,
            }
        }

        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/fixtures/replay/reference-combat.rlog");
        let reader = RlogReader::new(
            BufReader::new(File::open(fixture).unwrap()),
            RlogLimits::default(),
        )
        .unwrap();
        let mut header = reader.header().clone();
        header.session_id = "split-breakdown-identity".into();
        let mut plugin =
            CombatTimelinePlugin::new().with_ability_breakdown_resolver(legacy_breakdown_resolver);
        plugin.begin_live(&header);
        let mut factory =
            EventEnvelopeFactory::new(header.session_id.clone(), header.region.clone());
        let source = EntityRef {
            actor_id: ActorId(1),
            entity_uuid: EntityUuid(101),
        };
        let target = EntityRef {
            actor_id: ActorId(2),
            entity_uuid: EntityUuid(102),
        };
        for (sequence, hit_event_id, breakdown_ability_id, amount) in [
            (1, 7, Some(2_220_329_107), 1_500),
            (2, 9, Some(2_220_329_109), 3_500),
            // Legacy sealed events did not serialize a precomputed breakdown
            // ID. Replay must recover it from the retained raw hit identity.
            (3, 7, None, 500),
            (4, 9, None, 700),
        ] {
            let observed_micros = sequence * 1_000_000;
            let envelope = factory
                .emit(CanonicalEventDraft {
                    time: EventTime {
                        observed_micros,
                        game_time_millis: None,
                    },
                    provenance: EventProvenance::wire(observed_micros, 1, sequence),
                    sensitivity: EventSensitivity::PublicGameplay,
                    kind: CanonicalEventDraftKind::Timeline(TimelineEventKind::Damage(
                        DamageEvent {
                            source,
                            direct_source: None,
                            target,
                            ability: Some(AbilityId(2_203_291)),
                            amount,
                            actual_amount: Some(amount),
                            hp_loss: Some(amount),
                            shield_loss: None,
                            hit_event_id: Some(hit_event_id),
                            damage_source: None,
                            damage_type: None,
                            flags: DamageFlags::default(),
                            packet: rlogs_events::DamagePacketDetail {
                                breakdown_ability_id,
                                ..Default::default()
                            },
                        },
                    )),
                })
                .unwrap();
            plugin.observe_live(&envelope);
        }

        let snapshot = plugin.live_snapshot().unwrap();
        let actor = snapshot
            .actors
            .iter()
            .find(|actor| actor.actor_id == "1")
            .unwrap();
        assert_eq!(actor.reported_damage, 6_200);
        assert_eq!(
            actor
                .abilities
                .iter()
                .map(|ability| (ability.ability_id.as_str(), ability.reported_damage))
                .collect::<Vec<_>>(),
            vec![("2220329107", 2_000), ("2220329109", 4_200)]
        );
        let history = plugin.build_history_view(&HistoryViewSpec {
            id: "all".into(),
            label: "Entire run".into(),
            kind: "all".into(),
            segment_indices: vec![0],
            intervals: vec![(0, 5_000_000)],
            elapsed_micros: 5_000_000,
            active_combat_micros: 4_000_000,
            compress_intervals: false,
        });
        let actor = history
            .actors
            .iter()
            .find(|actor| actor.actor_id == "1")
            .unwrap();
        assert_eq!(
            actor
                .abilities
                .iter()
                .map(|ability| (ability.ability_id.as_str(), ability.damage))
                .collect::<Vec<_>>(),
            vec![("2220329107", 2_000), ("2220329109", 4_200)]
        );
        assert!(plugin.history_facts.iter().all(|fact| {
            fact.ability_id == Some(2_203_291)
                && fact
                    .breakdown_ability_id
                    .is_some_and(|id| id == 2_220_329_107 || id == 2_220_329_109)
        }));
    }

    #[test]
    fn late_player_identity_updates_the_existing_damaged_actor() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/fixtures/replay/reference-combat.rlog");
        let reader = RlogReader::new(
            BufReader::new(File::open(fixture).unwrap()),
            RlogLimits::default(),
        )
        .unwrap();
        let mut header = reader.header().clone();
        header.session_id = "late-player-identity".into();

        let mut plugin = CombatTimelinePlugin::new();
        plugin.begin_live(&header);
        let mut factory =
            EventEnvelopeFactory::new(header.session_id.clone(), header.region.clone());
        let player = EntityRef {
            actor_id: ActorId(6),
            entity_uuid: EntityUuid(216_009_015_936),
        };
        let target = EntityRef {
            actor_id: ActorId(7),
            entity_uuid: EntityUuid(1310784),
        };
        let damage = CanonicalEventDraftKind::Timeline(TimelineEventKind::Damage(DamageEvent {
            source: player,
            direct_source: None,
            target,
            ability: Some(AbilityId(2233)),
            amount: 12_345,
            actual_amount: Some(12_345),
            hp_loss: Some(12_345),
            shield_loss: None,
            hit_event_id: None,
            damage_source: None,
            damage_type: None,
            flags: DamageFlags::default(),
            packet: Default::default(),
        }));
        let identity = CanonicalEventDraftKind::Timeline(TimelineEventKind::Actor(ActorEvent {
            actor: player,
            state: ActorState::Updated,
            entity_type_id: 0,
            kind: ActorKind::Player,
            character_id: None,
            monster_id: None,
            display_name: Some("MarieRose".into()),
            class_id: Some(11),
            specialization_id: Some(117),
            level: None,
            ability_score: None,
            weapon_item_id: None,
            weapon_breakthrough_count: None,
            seasonal_score: None,
            primary_loadout: Vec::new(),
            auxiliary_loadout: Vec::new(),
            loadout_observation: Default::default(),
        }));

        for (observed_micros, kind) in [(1_000_000, damage), (2_000_000, identity)] {
            let envelope = factory
                .emit(CanonicalEventDraft {
                    time: EventTime {
                        observed_micros,
                        game_time_millis: None,
                    },
                    provenance: EventProvenance::wire(observed_micros, 1, 1),
                    sensitivity: EventSensitivity::PublicGameplay,
                    kind,
                })
                .unwrap();
            plugin.observe_live(&envelope);
        }

        let snapshot = plugin.live_snapshot().unwrap();
        let player = snapshot
            .actors
            .iter()
            .find(|actor| actor.entity_uuid == "216009015936")
            .unwrap();
        assert_eq!(player.actor_id, "6");
        assert_eq!(player.display_name.as_deref(), Some("MarieRose"));
        assert_eq!(player.class_id, Some(11));
        assert_eq!(player.specialization_id, Some(117));
        assert_eq!(player.reported_damage, 12_345);

        let history = plugin.build_history_view(&HistoryViewSpec {
            id: "all".into(),
            label: "Entire run".into(),
            kind: "all".into(),
            segment_indices: vec![0],
            intervals: vec![(0, 2_000_000)],
            elapsed_micros: 2_000_000,
            active_combat_micros: 1_000_000,
            compress_intervals: false,
        });
        let player = history
            .actors
            .iter()
            .find(|actor| actor.entity_uuid == "216009015936")
            .unwrap();
        assert_eq!(player.display_name.as_deref(), Some("MarieRose"));
        assert_eq!(player.class_id, Some(11));
        assert_eq!(player.specialization_id, Some(117));
        assert_eq!(player.damage, 12_345);
    }

    #[test]
    fn packet_proven_character_uid_merges_runtime_aliases_and_exact_imagine_slots() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/fixtures/replay/reference-combat.rlog");
        let reader = RlogReader::new(
            BufReader::new(File::open(fixture).unwrap()),
            RlogLimits::default(),
        )
        .unwrap();
        let mut header = reader.header().clone();
        header.session_id = "packet-proven-character-alias".into();

        let mut plugin = CombatTimelinePlugin::new();
        plugin.begin_live(&header);
        let mut factory =
            EventEnvelopeFactory::new(header.session_id.clone(), header.region.clone());
        let early_entity = EntityRef {
            actor_id: ActorId(4),
            entity_uuid: EntityUuid(216_009_015_936),
        };
        let named_entity = EntityRef {
            actor_id: ActorId(49),
            entity_uuid: EntityUuid(216_009_015_296),
        };
        let target = EntityRef {
            actor_id: ActorId(7),
            entity_uuid: EntityUuid(1_310_784),
        };
        let loadout = |second_ability_id, second_item_id| {
            (
                vec![
                    ActorLoadoutSlot {
                        slot_id: 7,
                        ability_id: Some(3_948),
                        item_id: Some(3_000_101),
                        tier: Some(5),
                    },
                    ActorLoadoutSlot {
                        slot_id: 8,
                        ability_id: Some(second_ability_id),
                        item_id: Some(second_item_id),
                        tier: Some(5),
                    },
                ],
                vec![ActorLoadoutSlot {
                    slot_id: 21,
                    ability_id: Some(3_021),
                    item_id: Some(3_000_009),
                    tier: Some(4),
                }],
            )
        };
        let actor =
            |entity: EntityRef, display_name: Option<&str>, second_ability_id, second_item_id| {
                let (primary_loadout, auxiliary_loadout) =
                    loadout(second_ability_id, second_item_id);
                CanonicalEventDraftKind::Timeline(TimelineEventKind::Actor(ActorEvent {
                    actor: entity,
                    state: ActorState::Updated,
                    entity_type_id: 1,
                    kind: ActorKind::Player,
                    character_id: Some("3296036".into()),
                    monster_id: None,
                    display_name: display_name.map(str::to_owned),
                    class_id: Some(11),
                    specialization_id: Some(117),
                    level: Some(60),
                    ability_score: Some(61_734),
                    weapon_item_id: Some(11_701_001),
                    weapon_breakthrough_count: Some(280),
                    seasonal_score: Some(3_585),
                    primary_loadout,
                    auxiliary_loadout,
                    loadout_observation: rlogs_events::ActorLoadoutObservation {
                        primary: ActorLoadoutEvidence::ExactSlots,
                        auxiliary: ActorLoadoutEvidence::ExactSlots,
                    },
                }))
            };
        let damage = |source: EntityRef, amount| {
            CanonicalEventDraftKind::Timeline(TimelineEventKind::Damage(DamageEvent {
                source,
                direct_source: None,
                target,
                ability: Some(AbilityId(2233)),
                amount,
                actual_amount: Some(amount),
                hp_loss: Some(amount),
                shield_loss: None,
                hit_event_id: None,
                damage_source: None,
                damage_type: None,
                flags: DamageFlags::default(),
                packet: Default::default(),
            }))
        };

        for (observed_micros, kind) in [
            (1_000_000, actor(early_entity, None, 3_969, 3_000_121)),
            (2_000_000, damage(early_entity, 100)),
            (
                3_000_000,
                actor(named_entity, Some("MarieRose"), 3_982, 3_001_001),
            ),
            (4_000_000, damage(named_entity, 200)),
        ] {
            let envelope = factory
                .emit(CanonicalEventDraft {
                    time: EventTime {
                        observed_micros,
                        game_time_millis: None,
                    },
                    provenance: EventProvenance::wire(observed_micros, 1, 1),
                    sensitivity: EventSensitivity::PublicGameplay,
                    kind,
                })
                .unwrap();
            plugin.observe_live(&envelope);
        }

        let snapshot = plugin.live_snapshot().unwrap();
        let players = snapshot
            .actors
            .iter()
            .filter(|actor| actor.character_id.as_deref() == Some("3296036"))
            .collect::<Vec<_>>();
        assert_eq!(players.len(), 1);
        let player = players[0];
        assert_eq!(player.actor_id, "4");
        assert_eq!(player.entity_uuid, "216009015936");
        assert_eq!(player.display_name.as_deref(), Some("MarieRose"));
        assert_eq!(player.reported_damage, 300);
        assert_eq!(player.primary_loadout.len(), 2);
        assert_eq!(player.primary_loadout[0].item_id, Some(3_000_101));
        assert_eq!(player.primary_loadout[0].tier, Some(5));
        assert_eq!(player.primary_loadout[1].ability_id, Some(3_982));
        assert_eq!(player.primary_loadout[1].item_id, Some(3_001_001));
        assert_eq!(player.primary_loadout[1].tier, Some(5));
        assert_eq!(player.auxiliary_loadout.len(), 1);
        assert_eq!(player.auxiliary_loadout[0].item_id, Some(3_000_009));
        assert_eq!(player.auxiliary_loadout[0].tier, Some(4));
    }

    #[test]
    fn alias_merge_never_replaces_newer_imagines_with_an_older_snapshot() {
        let loadout = |ability_id, item_id| {
            vec![ActorLoadoutSlot {
                slot_id: 8,
                ability_id: Some(ability_id),
                item_id: Some(item_id),
                tier: Some(5),
            }]
        };
        let newer_lucy = ActorAccumulator {
            entity_uuid: 216_009_015_936,
            character_id: Some("3296036".into()),
            primary_loadout: loadout(3_982, 3_001_001),
            primary_loadout_evidence: ActorLoadoutEvidence::ExactSlots,
            primary_loadout_observed_micros: 3_000_000,
            ..ActorAccumulator::default()
        };
        let older_igoreus = ActorAccumulator {
            entity_uuid: 216_009_015_296,
            character_id: Some("3296036".into()),
            primary_loadout: loadout(3_969, 3_000_121),
            primary_loadout_evidence: ActorLoadoutEvidence::ExactSlots,
            primary_loadout_observed_micros: 1_000_000,
            ..ActorAccumulator::default()
        };

        let mut canonical = newer_lucy.clone();
        canonical.merge_from(older_igoreus.clone());
        assert_eq!(canonical.primary_loadout[0].ability_id, Some(3_982));
        assert_eq!(canonical.primary_loadout[0].item_id, Some(3_001_001));

        let mut canonical = older_igoreus;
        canonical.merge_from(newer_lucy);
        assert_eq!(canonical.primary_loadout[0].ability_id, Some(3_982));
        assert_eq!(canonical.primary_loadout[0].item_id, Some(3_001_001));
    }

    #[test]
    fn exact_local_loadout_outranks_newer_remote_observations_and_can_clear_slots() {
        let loadout = |ability_id, item_id| {
            vec![ActorLoadoutSlot {
                slot_id: 8,
                ability_id: Some(ability_id),
                item_id: Some(item_id),
                tier: Some(5),
            }]
        };
        let mut actor = ActorAccumulator {
            entity_uuid: 216_009_015_936,
            character_id: Some("3296036".into()),
            primary_loadout: loadout(3_982, 3_001_001),
            primary_loadout_evidence: ActorLoadoutEvidence::ExactSlots,
            primary_loadout_observed_micros: 1_000_000,
            ..ActorAccumulator::default()
        };

        actor.merge_from(ActorAccumulator {
            entity_uuid: 216_009_015_936,
            primary_loadout: loadout(3_969, 3_000_121),
            primary_loadout_evidence: ActorLoadoutEvidence::ObservedSet,
            primary_loadout_observed_micros: 2_000_000,
            ..ActorAccumulator::default()
        });
        assert_eq!(actor.primary_loadout[0].ability_id, Some(3_982));
        assert_eq!(
            actor.primary_loadout_evidence,
            ActorLoadoutEvidence::ExactSlots
        );
        assert_eq!(actor.primary_loadout_observed_micros, 1_000_000);

        actor.merge_from(ActorAccumulator {
            entity_uuid: 216_009_015_936,
            primary_loadout: loadout(4_001, 3_001_101),
            primary_loadout_evidence: ActorLoadoutEvidence::ExactSlots,
            primary_loadout_observed_micros: 3_000_000,
            ..ActorAccumulator::default()
        });
        assert_eq!(actor.primary_loadout[0].ability_id, Some(4_001));

        actor.merge_from(ActorAccumulator {
            entity_uuid: 216_009_015_936,
            primary_loadout: Vec::new(),
            primary_loadout_evidence: ActorLoadoutEvidence::ExactSlots,
            primary_loadout_observed_micros: 4_000_000,
            ..ActorAccumulator::default()
        });
        assert!(actor.primary_loadout.is_empty());
        assert_eq!(
            actor.primary_loadout_evidence,
            ActorLoadoutEvidence::ExactSlots
        );
        assert_eq!(actor.primary_loadout_observed_micros, 4_000_000);
    }

    #[test]
    fn dungeon_and_wipe_resets_preserve_only_stable_player_identity() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/fixtures/replay/reference-combat.rlog");
        let reader = RlogReader::new(
            BufReader::new(File::open(fixture).unwrap()),
            RlogLimits::default(),
        )
        .unwrap();
        let mut header = reader.header().clone();
        header.session_id = "identity-preserving-run-reset".into();

        let mut plugin = CombatTimelinePlugin::new();
        plugin.begin_live(&header);
        let mut factory =
            EventEnvelopeFactory::new(header.session_id.clone(), header.region.clone());
        let player = EntityRef {
            actor_id: ActorId(6),
            entity_uuid: EntityUuid(216_009_015_936),
        };
        let target = EntityRef {
            actor_id: ActorId(7),
            entity_uuid: EntityUuid(1_310_784),
        };
        let identity = CanonicalEventDraftKind::Timeline(TimelineEventKind::Actor(ActorEvent {
            actor: player,
            state: ActorState::Updated,
            entity_type_id: 1,
            kind: ActorKind::Player,
            character_id: Some("3296036".into()),
            monster_id: None,
            display_name: Some("MarieRose".into()),
            class_id: Some(11),
            specialization_id: Some(117),
            level: Some(60),
            ability_score: Some(61_734),
            weapon_item_id: Some(11_701_001),
            weapon_breakthrough_count: Some(280),
            seasonal_score: Some(3_585),
            primary_loadout: vec![ActorLoadoutSlot {
                slot_id: 1,
                ability_id: Some(29_001_010),
                item_id: Some(1_000_101),
                tier: Some(5),
            }],
            auxiliary_loadout: vec![ActorLoadoutSlot {
                slot_id: 21,
                ability_id: Some(3_027),
                item_id: Some(1_000_202),
                tier: Some(4),
            }],
            loadout_observation: rlogs_events::ActorLoadoutObservation {
                primary: ActorLoadoutEvidence::ExactSlots,
                auxiliary: ActorLoadoutEvidence::ExactSlots,
            },
        }));
        let damage = |amount| {
            CanonicalEventDraftKind::Timeline(TimelineEventKind::Damage(DamageEvent {
                source: player,
                direct_source: None,
                target,
                ability: Some(AbilityId(2233)),
                amount,
                actual_amount: Some(amount),
                hp_loss: Some(amount),
                shield_loss: None,
                hit_event_id: None,
                damage_source: None,
                damage_type: None,
                flags: DamageFlags::default(),
                packet: Default::default(),
            }))
        };

        for (observed_micros, kind) in [(1_000_000, identity), (2_000_000, damage(12_345))] {
            let envelope = factory
                .emit(CanonicalEventDraft {
                    time: EventTime {
                        observed_micros,
                        game_time_millis: None,
                    },
                    provenance: EventProvenance::wire(observed_micros, 1, 1),
                    sensitivity: EventSensitivity::PublicGameplay,
                    kind,
                })
                .unwrap();
            plugin.observe_live(&envelope);
        }

        plugin.begin_live_preserving_player_identities(&header);
        let envelope = factory
            .emit(CanonicalEventDraft {
                time: EventTime {
                    observed_micros: 3_000_000,
                    game_time_millis: None,
                },
                provenance: EventProvenance::wire(3_000_000, 1, 1),
                sensitivity: EventSensitivity::PublicGameplay,
                kind: damage(6_789),
            })
            .unwrap();
        plugin.observe_live(&envelope);

        let snapshot = plugin.live_snapshot().unwrap();
        let player = snapshot
            .actors
            .iter()
            .find(|actor| actor.entity_uuid == "216009015936")
            .unwrap();
        assert_eq!(player.display_name.as_deref(), Some("MarieRose"));
        assert_eq!(player.character_id.as_deref(), Some("3296036"));
        assert_eq!(player.class_id, None);
        assert_eq!(player.specialization_id, None);
        assert_eq!(player.level, None);
        assert_eq!(player.ability_score, None);
        assert_eq!(player.weapon_item_id, None);
        assert_eq!(player.weapon_breakthrough_count, None);
        assert_eq!(player.seasonal_score, None);
        assert!(player.primary_loadout.is_empty());
        assert!(player.auxiliary_loadout.is_empty());
        assert_eq!(player.reported_damage, 6_789);

        // A packet-proven wipe also discards mutable presentation. Only a new
        // packet observation from the new attempt may restore that loadout.
        plugin.reset_live_attempt(4_000_000);
        let envelope = factory
            .emit(CanonicalEventDraft {
                time: EventTime {
                    observed_micros: 5_000_000,
                    game_time_millis: None,
                },
                provenance: EventProvenance::wire(5_000_000, 1, 1),
                sensitivity: EventSensitivity::PublicGameplay,
                kind: damage(2_222),
            })
            .unwrap();
        plugin.observe_live(&envelope);

        let snapshot = plugin.live_snapshot().unwrap();
        let player = snapshot
            .actors
            .iter()
            .find(|actor| actor.entity_uuid == "216009015936")
            .unwrap();
        assert_eq!(player.display_name.as_deref(), Some("MarieRose"));
        assert_eq!(player.character_id.as_deref(), Some("3296036"));
        assert_eq!(player.weapon_item_id, None);
        assert!(player.primary_loadout.is_empty());
        assert!(player.auxiliary_loadout.is_empty());
        assert_eq!(player.reported_damage, 2_222);
    }

    #[test]
    fn packet_proven_children_collapse_to_their_owner_without_hiding_unresolved_entities() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/fixtures/replay/reference-combat.rlog");
        let reader = RlogReader::new(
            BufReader::new(File::open(fixture).unwrap()),
            RlogLimits::default(),
        )
        .unwrap();
        let mut header = reader.header().clone();
        header.session_id = "packet-proven-actor-ancestry".into();

        let mut plugin = CombatTimelinePlugin::new();
        plugin.begin_live(&header);
        let mut factory =
            EventEnvelopeFactory::new(header.session_id.clone(), header.region.clone());
        let player = EntityRef {
            actor_id: ActorId(1),
            entity_uuid: EntityUuid(101),
        };
        let pet = EntityRef {
            actor_id: ActorId(2),
            entity_uuid: EntityUuid(202),
        };
        let boss = EntityRef {
            actor_id: ActorId(3),
            entity_uuid: EntityUuid(303),
        };
        let boss_projectile = EntityRef {
            actor_id: ActorId(4),
            entity_uuid: EntityUuid(404),
        };
        let unresolved_projectile = EntityRef {
            actor_id: ActorId(5),
            entity_uuid: EntityUuid(505),
        };
        let actor = |entity: EntityRef, state, kind, name: &str| {
            CanonicalEventDraftKind::Timeline(TimelineEventKind::Actor(ActorEvent {
                actor: entity,
                state,
                entity_type_id: 0,
                kind,
                character_id: None,
                monster_id: None,
                display_name: Some(name.into()),
                class_id: None,
                specialization_id: None,
                level: None,
                ability_score: None,
                weapon_item_id: None,
                weapon_breakthrough_count: None,
                seasonal_score: None,
                primary_loadout: Vec::new(),
                auxiliary_loadout: Vec::new(),
                loadout_observation: Default::default(),
            }))
        };
        let damage = |source: EntityRef,
                      direct_source: Option<EntityRef>,
                      target: EntityRef,
                      amount: i64| {
            CanonicalEventDraftKind::Timeline(TimelineEventKind::Damage(DamageEvent {
                source,
                direct_source,
                target,
                ability: Some(AbilityId(55)),
                amount,
                actual_amount: Some(amount),
                hp_loss: Some(amount),
                shield_loss: None,
                hit_event_id: None,
                damage_source: None,
                damage_type: None,
                flags: DamageFlags::default(),
                packet: Default::default(),
            }))
        };
        let ownership_attributes = |actor: EntityRef, owner: EntityRef| {
            CanonicalEventDraftKind::Timeline(TimelineEventKind::EntityAttributes(
                rlogs_events::EntityAttributeEvent {
                    actor,
                    update_kind: rlogs_events::EntityAttributeUpdateKind::Snapshot,
                    ownership: Some(rlogs_events::ActorOwnershipUpdate::Confirmed {
                        owner_entity_uuid: owner.entity_uuid,
                    }),
                    attributes: vec![
                        rlogs_events::EntityAttribute {
                            attribute_id: 90,
                            raw_value: Vec::new(),
                            decoded: Some(rlogs_events::EntityAttributeValue::Integer(
                                owner.entity_uuid.0,
                            )),
                        },
                        rlogs_events::EntityAttribute {
                            attribute_id: 91,
                            raw_value: Vec::new(),
                            decoded: Some(rlogs_events::EntityAttributeValue::Integer(
                                owner.entity_uuid.0,
                            )),
                        },
                    ],
                },
            ))
        };

        for (observed_micros, kind) in [
            (
                100_000,
                actor(player, ActorState::Spawned, ActorKind::Player, "Player"),
            ),
            (
                200_000,
                actor(pet, ActorState::Spawned, ActorKind::Pet, "Pet"),
            ),
            (
                300_000,
                actor(boss, ActorState::Spawned, ActorKind::Monster, "Boss"),
            ),
            (
                400_000,
                actor(
                    boss_projectile,
                    ActorState::Spawned,
                    ActorKind::Projectile,
                    "Boss projectile",
                ),
            ),
            (
                500_000,
                actor(
                    unresolved_projectile,
                    ActorState::Spawned,
                    ActorKind::Projectile,
                    "Unresolved projectile",
                ),
            ),
            // The owner pair is emitted before combat. Consumers can now use
            // one normalized relation instead of decoding game attributes on
            // their own or waiting for a later damage packet.
            (750_000, ownership_attributes(boss_projectile, boss)),
            // The packet says the pet was the immediate attacker and the
            // player was the attributed top owner.
            (1_000_000, damage(player, Some(pet), boss, 1_000)),
            // This hit deliberately carries no direct-source proof. The
            // preceding canonical ownership event is authoritative.
            (2_000_000, damage(boss, None, player, 100)),
            // A later hit on that projectile must recount to the boss target.
            (3_000_000, damage(player, None, boss_projectile, 250)),
            // No owner evidence exists for this projectile, so it remains
            // visible instead of being guessed away.
            (4_000_000, damage(player, None, unresolved_projectile, 125)),
            (
                5_000_000,
                actor(pet, ActorState::Despawned, ActorKind::Pet, "Pet"),
            ),
        ] {
            let envelope = factory
                .emit(CanonicalEventDraft {
                    time: EventTime {
                        observed_micros,
                        game_time_millis: None,
                    },
                    provenance: EventProvenance::wire(observed_micros, 1, 1),
                    sensitivity: EventSensitivity::PublicGameplay,
                    kind,
                })
                .unwrap();
            plugin.observe_live(&envelope);
        }

        // The canonical event stream still contains the immediate pet actor;
        // only the compact consumer projection is recounted.
        let pet_damage_fact = plugin
            .history_facts
            .iter()
            .find(|fact| fact.observed_micros == 1_000_000)
            .unwrap();
        assert_eq!(pet_damage_fact.source_actor_id, player.actor_id.0);
        assert_eq!(pet_damage_fact.source_entity_uuid, player.entity_uuid.0);

        let history = plugin.build_history_view(&HistoryViewSpec {
            id: "all".into(),
            label: "Entire run".into(),
            kind: "all".into(),
            segment_indices: vec![0],
            intervals: vec![(0, 6_000_000)],
            elapsed_micros: 6_000_000,
            active_combat_micros: 3_000_000,
            compress_intervals: false,
        });
        let player_history = history
            .actors
            .iter()
            .find(|actor| actor.actor_id == player.actor_id.0.to_string())
            .unwrap();
        assert_eq!(player_history.damage, 1_375);
        assert!(
            history
                .actors
                .iter()
                .all(|actor| actor.actor_id != pet.actor_id.0.to_string() || actor.damage == 0)
        );

        let target_ids = history
            .targets
            .iter()
            .map(|target| target.actor_id.clone())
            .collect::<BTreeSet<_>>();
        assert!(target_ids.contains(&boss.actor_id.0.to_string()));
        assert!(target_ids.contains(&unresolved_projectile.actor_id.0.to_string()));
        assert!(!target_ids.contains(&boss_projectile.actor_id.0.to_string()));
        assert!(!target_ids.contains(&pet.actor_id.0.to_string()));
    }

    #[test]
    fn later_actor_reuse_cannot_rewrite_already_stored_history_attribution() {
        let mut plugin = CombatTimelinePlugin::new();
        let owner = EntityRef {
            actor_id: ActorId(1),
            entity_uuid: EntityUuid(101),
        };
        let child = EntityRef {
            actor_id: ActorId(2),
            entity_uuid: EntityUuid(202),
        };
        let target = EntityRef {
            actor_id: ActorId(3),
            entity_uuid: EntityUuid(303),
        };
        {
            let actor = plugin.actor_mut(owner.actor_id.0, owner.entity_uuid.0);
            actor.display_name = Some("Original owner".into());
            actor.actor_kind = Some("player".into());
            actor.class_id = Some(11);
        }
        plugin.record_history_identity(500, owner.actor_id.0);
        {
            let actor = plugin.actor_mut(target.actor_id.0, target.entity_uuid.0);
            actor.display_name = Some("Original target".into());
            actor.actor_kind = Some("monster".into());
            actor.monster_id = Some(80_017);
        }
        plugin.record_history_identity(500, target.actor_id.0);
        plugin
            .actor_ancestry
            .observe_attributed_source(1_000, owner, Some(child));
        plugin.push_history_fact(CombatFact {
            observed_micros: 1_000,
            source_actor_id: child.actor_id.0,
            source_entity_uuid: child.entity_uuid.0,
            target: Some((target.actor_id.0, target.entity_uuid.0)),
            breakdown_ability_id: Some(55),
            ability_id: Some(55),
            kind: CombatFactKind::Damage {
                reported: 100,
                effective: 100,
                critical: false,
            },
        });

        plugin.actor_ancestry.clear_owner(2_000, child);
        plugin.actor_ancestry.observe_entity(EntityRef {
            actor_id: child.actor_id,
            entity_uuid: EntityUuid(999),
        });
        {
            let actor = plugin.actor_mut(owner.actor_id.0, 999);
            actor.display_name = Some("Reused actor".into());
            actor.actor_kind = Some("monster".into());
            actor.class_id = None;
        }
        plugin.record_history_identity(2_000, owner.actor_id.0);

        let stored = plugin.history_facts.first().unwrap();
        assert_eq!(stored.source_actor_id, owner.actor_id.0);
        assert_eq!(stored.source_entity_uuid, owner.entity_uuid.0);

        let history = plugin.build_history_view(&HistoryViewSpec {
            id: "all".into(),
            label: "Entire run".into(),
            kind: "all".into(),
            segment_indices: vec![0],
            intervals: vec![(0, 2_500)],
            elapsed_micros: 2_500,
            active_combat_micros: 1_000,
            compress_intervals: false,
        });
        let owner_history = history
            .actors
            .iter()
            .find(|actor| actor.entity_uuid == owner.entity_uuid.0.to_string())
            .unwrap();
        assert_eq!(
            owner_history.display_name.as_deref(),
            Some("Original owner")
        );
        assert_eq!(owner_history.actor_kind.as_deref(), Some("player"));
        assert_eq!(owner_history.class_id, Some(11));
        assert_eq!(owner_history.damage, 100);
    }

    #[test]
    fn wipe_closes_child_ownership_before_short_actor_id_reuse() {
        let mut plugin = CombatTimelinePlugin::new();
        let first_owner = EntityRef {
            actor_id: ActorId(1),
            entity_uuid: EntityUuid(101),
        };
        let reused_child = EntityRef {
            actor_id: ActorId(2),
            entity_uuid: EntityUuid(202),
        };
        plugin.actor_ancestry.observe_relation(
            1_000,
            reused_child,
            first_owner,
            rlogs_combat::ActorOwnershipEvidence::ConfirmedEntityAttributes,
        );
        assert_eq!(
            plugin.actor_ancestry.resolve_entity_at(reused_child, 1_999),
            first_owner
        );

        plugin.reset_live_attempt(2_000);

        let next_attempt_entity = EntityRef {
            actor_id: reused_child.actor_id,
            entity_uuid: EntityUuid(999),
        };
        plugin.actor_ancestry.observe_entity(next_attempt_entity);
        assert_eq!(
            plugin
                .actor_ancestry
                .resolve_entity_at(next_attempt_entity, 2_001),
            next_attempt_entity
        );
        assert_eq!(
            plugin.actor_ancestry.resolve_entity_at(reused_child, 1_999),
            first_owner
        );
    }

    #[test]
    fn reviewed_vulnerability_transfers_live_rdps_and_stops_on_removal() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/fixtures/replay/reference-combat.rlog");
        let reader = RlogReader::new(
            BufReader::new(File::open(fixture).unwrap()),
            RlogLimits::default(),
        )
        .unwrap();
        let mut header = reader.header().clone();
        header.session_id = "reviewed-vulnerability-rdps".into();
        let rule = DamageContributionRule {
            effect_id: 2_203_031,
            kind: rlogs_combat::DamageContributionKind::TargetVulnerability,
            magnitude_basis_points: 1_000,
            stacking: rlogs_combat::DamageContributionStacking::Fixed,
        };
        let mut plugin = CombatTimelinePlugin::with_damage_contribution_rules(vec![rule]).unwrap();
        plugin.begin_live(&header);
        let mut factory =
            EventEnvelopeFactory::new(header.session_id.clone(), header.region.clone());
        let provider = EntityRef {
            actor_id: ActorId(1),
            entity_uuid: EntityUuid(101),
        };
        let recipient = EntityRef {
            actor_id: ActorId(2),
            entity_uuid: EntityUuid(102),
        };
        let target = EntityRef {
            actor_id: ActorId(3),
            entity_uuid: EntityUuid(103),
        };
        let actor = |entity: EntityRef, kind: ActorKind, name: &str| {
            CanonicalEventDraftKind::Timeline(TimelineEventKind::Actor(ActorEvent {
                actor: entity,
                state: ActorState::Spawned,
                entity_type_id: 0,
                kind,
                character_id: None,
                monster_id: None,
                display_name: Some(name.into()),
                class_id: None,
                specialization_id: None,
                level: None,
                ability_score: None,
                weapon_item_id: None,
                weapon_breakthrough_count: None,
                seasonal_score: None,
                primary_loadout: Vec::new(),
                auxiliary_loadout: Vec::new(),
                loadout_observation: Default::default(),
            }))
        };
        let damage = |source: EntityRef, amount| {
            CanonicalEventDraftKind::Timeline(TimelineEventKind::Damage(DamageEvent {
                source,
                direct_source: None,
                target,
                ability: Some(AbilityId(55)),
                amount,
                actual_amount: Some(amount),
                hp_loss: Some(amount),
                shield_loss: None,
                hit_event_id: None,
                damage_source: None,
                damage_type: None,
                flags: DamageFlags::default(),
                packet: Default::default(),
            }))
        };
        let status = |state| {
            CanonicalEventDraftKind::Timeline(TimelineEventKind::Status(StatusEvent {
                source: Some(provider),
                target,
                effect: StatusEffectId(2_203_031),
                instance_id: Some(StatusEffectInstanceId(77)),
                origin: None,
                state,
                stacks: Some(1),
                duration_millis: None,
                level: None,
                part_id: None,
                count: None,
                created_at_millis: None,
            }))
        };
        for (observed_micros, kind) in [
            (100_000, actor(provider, ActorKind::Player, "Provider")),
            (200_000, actor(recipient, ActorKind::Player, "Recipient")),
            (300_000, actor(target, ActorKind::Monster, "Target")),
            (1_000_000, status(StatusState::Applied)),
            (2_000_000, damage(recipient, 1_100)),
            (4_000_000, status(StatusState::Removed)),
            (5_000_000, damage(recipient, 1_100)),
        ] {
            let envelope = factory
                .emit(CanonicalEventDraft {
                    time: EventTime {
                        observed_micros,
                        game_time_millis: None,
                    },
                    provenance: EventProvenance::wire(observed_micros, 1, 1),
                    sensitivity: EventSensitivity::PublicGameplay,
                    kind,
                })
                .unwrap();
            plugin.observe_live(&envelope);
        }

        let snapshot = plugin.live_snapshot().unwrap();
        let provider = snapshot
            .actors
            .iter()
            .find(|actor| actor.actor_id == "1")
            .unwrap();
        let recipient = snapshot
            .actors
            .iter()
            .find(|actor| actor.actor_id == "2")
            .unwrap();
        assert_eq!(provider.reported_damage, 0);
        assert_eq!(provider.rdps_damage, Some(100));
        assert_eq!(provider.rdps_contribution_given, Some(100));
        assert_eq!(provider.rdps_contribution_received, Some(0));
        assert_eq!(recipient.reported_damage, 2_200);
        assert_eq!(recipient.rdps_damage, Some(2_100));
        assert_eq!(recipient.rdps_contribution_given, Some(0));
        assert_eq!(recipient.rdps_contribution_received, Some(100));
        assert_eq!(
            snapshot
                .actors
                .iter()
                .map(|actor| actor.reported_damage)
                .sum::<i64>(),
            snapshot
                .actors
                .iter()
                .map(|actor| actor.rdps_damage.unwrap_or_default())
                .sum::<i64>()
        );

        let overlay = plugin.live_overlay_snapshot().unwrap();
        let overlay_provider = overlay
            .actors
            .iter()
            .find(|actor| actor.actor_id == "1")
            .expect("a support-only rDPS provider must remain visible in the live overlay");
        assert_eq!(overlay_provider.reported_damage, 0);
        assert_eq!(overlay_provider.rdps_damage, Some(100));
        assert_eq!(overlay_provider.rdps_contribution_given, Some(100));
        assert_eq!(
            overlay
                .actors
                .iter()
                .map(|actor| actor.reported_damage)
                .sum::<i64>(),
            overlay
                .actors
                .iter()
                .map(|actor| actor.rdps_damage.unwrap_or_default())
                .sum::<i64>(),
            "visible live-overlay rows must conserve raw damage and rDMG"
        );

        let history = plugin.build_history_view(&HistoryViewSpec {
            id: "all".into(),
            label: "Entire run".into(),
            kind: "all".into(),
            segment_indices: vec![0],
            intervals: vec![(0, 6_000_000)],
            elapsed_micros: 6_000_000,
            active_combat_micros: 3_000_000,
            compress_intervals: false,
        });
        let history_provider = history
            .actors
            .iter()
            .find(|actor| actor.actor_id == "1")
            .unwrap();
        let history_recipient = history
            .actors
            .iter()
            .find(|actor| actor.actor_id == "2")
            .unwrap();
        assert_eq!(history_provider.rdps_damage, Some(100));
        assert_eq!(history_provider.rdps_contribution_given, Some(100));
        assert_eq!(history_recipient.rdps_damage, Some(2_100));
        assert_eq!(history_recipient.rdps_contribution_received, Some(100));
    }

    #[test]
    fn live_rdps_influence_ledger_is_hard_capped() {
        let mut plugin = CombatTimelinePlugin::new();
        for effect_id in 1..=(MAXIMUM_LIVE_RDPS_INFLUENCE_RELATIONSHIPS as i64 + 1) {
            plugin.observe_live_damage_influence(HistoryDamageInfluenceObservation {
                observed_micros: effect_id as u64,
                effect_id,
                scope: DamageContributionScope::CompleteEffect,
                provider_actor_id: 1,
                provider_entity_uuid: 101,
                recipient_actor_id: 2,
                recipient_entity_uuid: 102,
                damage_event_sequence: Some(effect_id as u64),
                affected_ability_id: Some(55),
                affected_target: Some((3, 103)),
                critical: None,
                observed_damage: 1,
                exact_integer_delta: Some(1),
                exact_rational_delta: None,
            });
        }

        assert_eq!(
            plugin.live_damage_influences.len(),
            MAXIMUM_LIVE_RDPS_INFLUENCE_RELATIONSHIPS
        );
        assert!(plugin.live_damage_influences_truncated);

        plugin.reset_live_attempt(10_000);
        assert!(plugin.live_damage_influences.is_empty());
        assert!(!plugin.live_damage_influences_truncated);
    }

    #[test]
    fn damage_and_dungeon_objectives_bound_live_combat_without_a_second_pass() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/fixtures/replay/reference-combat.rlog");
        let reader = RlogReader::new(
            BufReader::new(File::open(fixture).unwrap()),
            RlogLimits::default(),
        )
        .unwrap();
        let mut header = reader.header().clone();
        header.session_id = "inferred-combat".into();
        let mut factory =
            EventEnvelopeFactory::new(header.session_id.clone(), header.region.clone());
        let mut plugin = CombatTimelinePlugin::new();
        plugin.begin_live(&header);
        let player = EntityRef {
            actor_id: ActorId(1),
            entity_uuid: EntityUuid(100),
        };
        let target = EntityRef {
            actor_id: ActorId(2),
            entity_uuid: EntityUuid(200),
        };
        let damage = |amount| {
            CanonicalEventDraftKind::Timeline(TimelineEventKind::Damage(DamageEvent {
                source: player,
                direct_source: None,
                target,
                ability: Some(AbilityId(55)),
                amount,
                actual_amount: Some(amount),
                hp_loss: Some(amount),
                shield_loss: None,
                hit_event_id: None,
                damage_source: None,
                damage_type: None,
                flags: DamageFlags::default(),
                packet: Default::default(),
            }))
        };
        let dungeon = |kind, objective_complete| {
            CanonicalEventDraftKind::Dungeon(DungeonEvent {
                kind,
                dungeon_id: None,
                instance_id: Some("run-1".into()),
                difficulty_id: None,
                objective_map_key: None,
                objective_id: None,
                objective_value: None,
                objective_complete,
                objective_catalog: None,
                flow: None,
            })
        };

        {
            let mut emit = |observed_micros, kind| {
                let envelope = factory
                    .emit(CanonicalEventDraft {
                        time: EventTime {
                            observed_micros,
                            game_time_millis: None,
                        },
                        provenance: EventProvenance::wire(observed_micros, 1, 1),
                        sensitivity: EventSensitivity::PublicGameplay,
                        kind,
                    })
                    .unwrap();
                plugin.observe_live(&envelope);
            };
            emit(1_000_000, damage(9_000));
            emit(
                4_000_000,
                dungeon(DungeonEventKind::ObjectiveUpdated, Some(true)),
            );
            emit(5_000_000, damage(3_000));
            emit(14_000_000, dungeon(DungeonEventKind::FlowUpdated, None));
        }

        let snapshot = plugin.live_snapshot().unwrap();
        assert_eq!(snapshot.combat_window_count, 1);
        assert_eq!(snapshot.active_combat_micros, 8_000_000);
        assert_eq!(snapshot.combat_started_micros, Some(5_000_000));
        assert_eq!(snapshot.combat_ended_micros, Some(13_000_000));
        assert!(!snapshot.closed_at_log_end);
        let player = snapshot
            .actors
            .iter()
            .find(|actor| actor.actor_id == "1")
            .unwrap();
        assert_eq!(player.damage_during_combat, 3_000);
        assert!((player.run_dps - (3_000.0 / 9.0)).abs() < 0.001);
        assert!((player.active_dps - (3_000.0 / 8.0)).abs() < 0.001);
        assert!((player.encounter_dps - (12_000.0 / 12.0)).abs() < 0.001);
    }

    #[test]
    fn public_snapshot_serializes_exact_identifiers_as_decimal_text() {
        let actor = ActorCombatSummary {
            actor_id: u64::MAX.to_string(),
            entity_uuid: i64::MIN.to_string(),
            character_id: None,
            display_name: None,
            actor_kind: None,
            monster_id: None,
            current_hp: None,
            max_hp: None,
            class_id: None,
            specialization_id: None,
            level: None,
            ability_score: None,
            weapon_item_id: None,
            weapon_breakthrough_count: None,
            seasonal_score: None,
            primary_loadout: Vec::new(),
            auxiliary_loadout: Vec::new(),
            reported_damage: 0,
            effective_damage: 0,
            hp_damage: 0,
            shield_damage: 0,
            damage_during_combat: 0,
            damage_taken: 0,
            dps: 0.0,
            run_dps: 0.0,
            encounter_dps: 0.0,
            active_dps: 0.0,
            hps: 0.0,
            tps: 0.0,
            rdps_damage: None,
            rdps: None,
            rdps_contribution_given: None,
            rdps_contribution_received: None,
            rdps_incomplete: false,
            reported_healing: 0,
            effective_healing: 0,
            overheal: 0,
            shielding: 0,
            casts: 0,
            hits: 0,
            critical_hits: 0,
            deaths: 0,
            revives: 0,
            position_samples: 0,
            path_distance: 0.0,
            abilities: vec![AbilityCombatSummary {
                ability_id: i64::MAX.to_string(),
                casts: 0,
                hits: 0,
                critical_hits: 0,
                reported_damage: 0,
                effective_damage: 0,
                reported_healing: 0,
                effective_healing: 0,
                shielding: 0,
            }],
        };
        let value = serde_json::to_value(actor).unwrap();

        assert_eq!(value["actor_id"], u64::MAX.to_string());
        assert_eq!(value["entity_uuid"], i64::MIN.to_string());
        assert_eq!(value["abilities"][0]["ability_id"], i64::MAX.to_string());
    }

    #[test]
    fn history_views_keep_elapsed_and_active_dps_denominators_distinct() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/fixtures/replay/reference-combat.rlog");
        let mut reader = RlogReader::new(
            BufReader::new(File::open(fixture).unwrap()),
            RlogLimits::default(),
        )
        .unwrap();
        let mut plugin = CombatTimelinePlugin::new();
        plugin.begin_live(reader.header());
        while let Some(event) = reader.next_event().unwrap() {
            plugin.observe_live(&event);
        }
        let target_identity = plugin.actors.get_mut(&2).unwrap();
        target_identity.monster_id = Some(33_701);
        target_identity.actor_kind = Some("monster".into());
        plugin.record_history_identity(0, 2);
        let pet_identity = plugin.actor_mut(3, 3001);
        pet_identity.monster_id = Some(2);
        pet_identity.actor_kind = Some("pet".into());
        plugin.record_history_identity(0, 3);
        let run = RunAnalysis {
            schema_version: 1,
            source_session_id: reader.header().session_id.clone(),
            encounter_ruleset_id: Some("test.rules".into()),
            encounter_ruleset_version: Some(1),
            identity: RunIdentity {
                activity_kind: ActivityKind::Dungeon,
                activity_id: Some("scene.test".into()),
                activity_family_id: Some("test".into()),
                scene_id: Some(1),
                observed_dungeon_id: None,
                instance_id: Some("instance-1".into()),
                difficulty_family: Some("hard".into()),
                difficulty_id: None,
                difficulty_tier: None,
                route_id: None,
                raid_route_kind: None,
            },
            partition: None,
            terminal_state: RunTerminalState::Completed,
            authoritative_start: true,
            authoritative_completion: true,
            timing: RunTiming {
                started_micros: 0,
                ended_micros: Some(20_000_000),
                observed_until_micros: 20_000_000,
                wall_time_micros: Some(20_000_000),
                // Simulates a newly mapped scene before it has reviewed run
                // rules. The history projection must retain the live
                // reducer's damage-driven combat denominator.
                active_combat_micros: 0,
                noncombat_micros: Some(10_000_000),
                manual_pause_micros: 0,
            },
            segments: vec![RunSegmentSummary {
                index: 0,
                kind: RunSegmentKind::Boss,
                started_micros: 0,
                ended_micros: 20_000_000,
                wall_time_micros: 20_000_000,
                active_combat_micros: 0,
                attempt_count: 1,
                retry_count: 0,
                total_attempt_wall_time_micros: 20_000_000,
                total_attempt_active_combat_micros: 10_000_000,
                elapsed_trying_micros: 20_000_000,
                between_attempts_micros: 0,
                successful_attempt_indices: vec![0],
                successful_attempt_wall_time_micros: 20_000_000,
                successful_attempt_active_combat_micros: 10_000_000,
                winning_attempt_index: Some(0),
                winning_attempt_wall_time_micros: Some(20_000_000),
                winning_attempt_active_combat_micros: Some(10_000_000),
                encounter_indices: vec![0],
                closed_at_run_end: false,
            }],
            encounters: vec![EncounterSummary {
                index: 0,
                encounter_id: Some("boss".into()),
                kind: EncounterKind::Boss,
                segment_index: 0,
                attempt_number: 1,
                is_retry: false,
                is_successful_attempt: true,
                terminal_state: EncounterTerminalState::Cleared,
                started_micros: 0,
                ended_micros: 20_000_000,
                wall_time_micros: 20_000_000,
                active_combat_micros: 10_000_000,
                combat_windows: vec![CombatWindowSummary {
                    started_micros: 1_000_000,
                    ended_micros: 11_000_000,
                    duration_micros: 10_000_000,
                    closed_at_boundary: false,
                }],
                closed_at_run_end: false,
            }],
            manual_pauses: vec![],
            data_gap_count: 0,
            findings: vec![],
            submission_disposition: RunSubmissionDisposition::RankCandidate,
        };

        plugin.history_facts.push(CombatFact {
            observed_micros: 5_000_000,
            source_actor_id: 1,
            source_entity_uuid: 1001,
            target: None,
            breakdown_ability_id: None,
            ability_id: None,
            kind: CombatFactKind::Life {
                state: LifeState::Died,
            },
        });
        for (observed_micros, state) in [
            (6_000_000, StatusState::Applied),
            (7_000_000, StatusState::Stacked),
            (8_000_000, StatusState::Consumed),
            (9_000_000, StatusState::Removed),
        ] {
            plugin.history_facts.push(CombatFact {
                observed_micros,
                source_actor_id: 1,
                source_entity_uuid: 1001,
                target: Some((2, 2001)),
                breakdown_ability_id: None,
                ability_id: None,
                kind: CombatFactKind::Status {
                    effect_id: 2_203_031,
                    attribution_source_actor_id: Some(1),
                    instance_id: Some(2_203_031),
                    state,
                    stacks: Some(1),
                    duration_millis: None,
                },
            });
        }
        plugin.history_facts.push(CombatFact {
            observed_micros: 9_500_000,
            source_actor_id: 1,
            source_entity_uuid: 1001,
            target: Some((3, 3001)),
            breakdown_ability_id: None,
            ability_id: None,
            kind: CombatFactKind::Status {
                effect_id: 2_203_293,
                attribution_source_actor_id: Some(1),
                instance_id: Some(2_203_293),
                state: StatusState::Applied,
                stacks: Some(1),
                duration_millis: None,
            },
        });
        plugin.history_facts.push(CombatFact {
            observed_micros: 10_000_000,
            source_actor_id: 2,
            source_entity_uuid: 2001,
            target: Some((1, 1001)),
            breakdown_ability_id: Some(9001),
            ability_id: Some(9001),
            kind: CombatFactKind::Damage {
                reported: 500,
                effective: 400,
                critical: false,
            },
        });

        let history = plugin.history_snapshot(&[run]).unwrap();
        let all = history.runs[0]
            .views
            .iter()
            .find(|view| view.id == "all")
            .unwrap();
        let player = all
            .actors
            .iter()
            .find(|actor| actor.actor_id == "1")
            .unwrap();
        assert_eq!(player.damage, 20_000);
        assert_eq!(player.dps, 1_000.0);
        assert_eq!(player.encounter_dps, 2_000.0);
        assert_eq!(player.rdps, None);
        assert_eq!(player.apm, None);
        assert_eq!(player.death_seconds, vec![5]);
        assert_eq!(player.abilities[0].targets[0].actor_id, "2");
        let boss_target = player
            .targets
            .iter()
            .find(|target| target.actor_id == "2")
            .unwrap();
        assert_eq!(boss_target.effect_events, 4);
        assert_eq!(
            boss_target
                .series
                .iter()
                .map(|point| point.damage)
                .sum::<i64>(),
            20_000
        );
        assert_eq!(
            boss_target
                .series
                .iter()
                .map(|point| point.damage_taken)
                .sum::<i64>(),
            400
        );
        let effect = player
            .effects
            .iter()
            .find(|effect| effect.effect_id == "2203031")
            .unwrap();
        assert_eq!(effect.target_actor_id, "2");
        assert_eq!(effect.target_entity_uuid, "2001");
        assert_eq!(effect.applied, 1);
        assert_eq!(effect.stacked, 1);
        assert_eq!(effect.consumed, 1);
        assert_eq!(effect.removed, 1);
        let target = all
            .targets
            .iter()
            .find(|target| target.actor_id == "2")
            .unwrap();
        assert_eq!(target.monster_id.as_deref(), Some("33701"));
        assert_eq!(target.entity_uuid, "2001");
        assert!(all.targets.iter().all(|target| target.actor_id != "3"));
        assert!(
            player
                .targets
                .iter()
                .any(|target| { target.actor_id == "3" && target.effect_events == 1 })
        );
        assert!(!player.series.is_empty());
        assert_eq!(history.runs[0].rdps_status, "pending_reviewed_effect_rules");
        assert_eq!(
            history.runs[0].apm_status,
            "pending_active_action_classification"
        );
    }

    #[test]
    fn projected_timeline_closes_removed_retry_gaps() {
        let intervals = [(1_000_000, 4_000_000), (8_000_000, 12_000_000)];

        assert_eq!(
            history_fact_offset(2_000_000, &intervals, 1_000_000, true),
            Some(1_000_000)
        );
        assert_eq!(
            history_fact_offset(9_000_000, &intervals, 1_000_000, true),
            Some(4_000_000)
        );
        assert_eq!(
            history_fact_offset(6_000_000, &intervals, 1_000_000, true),
            None
        );
    }

    #[test]
    fn history_separates_failed_boss_pulls_from_the_winning_boss_view() {
        let run = RunAnalysis {
            schema_version: 1,
            source_session_id: "retry-projection".into(),
            encounter_ruleset_id: Some("test.rules".into()),
            encounter_ruleset_version: Some(1),
            identity: RunIdentity {
                activity_kind: ActivityKind::Dungeon,
                activity_id: Some("scene.retry".into()),
                activity_family_id: Some("retry".into()),
                scene_id: Some(1),
                observed_dungeon_id: None,
                instance_id: Some("instance-retry".into()),
                difficulty_family: Some("hard".into()),
                difficulty_id: None,
                difficulty_tier: None,
                route_id: None,
                raid_route_kind: None,
            },
            partition: None,
            terminal_state: RunTerminalState::Completed,
            authoritative_start: true,
            authoritative_completion: true,
            timing: RunTiming {
                started_micros: 0,
                ended_micros: Some(50_000_000),
                observed_until_micros: 50_000_000,
                wall_time_micros: Some(50_000_000),
                active_combat_micros: 27_000_000,
                noncombat_micros: Some(23_000_000),
                manual_pause_micros: 0,
            },
            segments: vec![
                RunSegmentSummary {
                    index: 0,
                    kind: RunSegmentKind::Mobbing,
                    started_micros: 0,
                    ended_micros: 10_000_000,
                    wall_time_micros: 10_000_000,
                    active_combat_micros: 8_000_000,
                    attempt_count: 1,
                    retry_count: 0,
                    total_attempt_wall_time_micros: 10_000_000,
                    total_attempt_active_combat_micros: 8_000_000,
                    elapsed_trying_micros: 10_000_000,
                    between_attempts_micros: 0,
                    successful_attempt_indices: vec![0],
                    successful_attempt_wall_time_micros: 10_000_000,
                    successful_attempt_active_combat_micros: 8_000_000,
                    winning_attempt_index: Some(0),
                    winning_attempt_wall_time_micros: Some(10_000_000),
                    winning_attempt_active_combat_micros: Some(8_000_000),
                    encounter_indices: vec![0],
                    closed_at_run_end: false,
                },
                RunSegmentSummary {
                    index: 1,
                    kind: RunSegmentKind::Boss,
                    started_micros: 20_000_000,
                    ended_micros: 50_000_000,
                    wall_time_micros: 30_000_000,
                    active_combat_micros: 19_000_000,
                    attempt_count: 2,
                    retry_count: 1,
                    total_attempt_wall_time_micros: 20_000_000,
                    total_attempt_active_combat_micros: 19_000_000,
                    elapsed_trying_micros: 30_000_000,
                    between_attempts_micros: 10_000_000,
                    successful_attempt_indices: vec![2],
                    successful_attempt_wall_time_micros: 10_000_000,
                    successful_attempt_active_combat_micros: 9_000_000,
                    winning_attempt_index: Some(2),
                    winning_attempt_wall_time_micros: Some(10_000_000),
                    winning_attempt_active_combat_micros: Some(9_000_000),
                    encounter_indices: vec![1, 2],
                    closed_at_run_end: false,
                },
            ],
            encounters: vec![
                EncounterSummary {
                    index: 0,
                    encounter_id: Some("mobbing".into()),
                    kind: EncounterKind::Mobbing,
                    segment_index: 0,
                    attempt_number: 1,
                    is_retry: false,
                    is_successful_attempt: true,
                    terminal_state: EncounterTerminalState::Cleared,
                    started_micros: 0,
                    ended_micros: 10_000_000,
                    wall_time_micros: 10_000_000,
                    active_combat_micros: 8_000_000,
                    combat_windows: vec![],
                    closed_at_run_end: false,
                },
                EncounterSummary {
                    index: 1,
                    encounter_id: Some("boss".into()),
                    kind: EncounterKind::Boss,
                    segment_index: 1,
                    attempt_number: 1,
                    is_retry: false,
                    is_successful_attempt: false,
                    terminal_state: EncounterTerminalState::Wiped,
                    started_micros: 20_000_000,
                    ended_micros: 30_000_000,
                    wall_time_micros: 10_000_000,
                    active_combat_micros: 10_000_000,
                    combat_windows: vec![],
                    closed_at_run_end: false,
                },
                EncounterSummary {
                    index: 2,
                    encounter_id: Some("boss".into()),
                    kind: EncounterKind::Boss,
                    segment_index: 1,
                    attempt_number: 2,
                    is_retry: true,
                    is_successful_attempt: true,
                    terminal_state: EncounterTerminalState::Cleared,
                    started_micros: 40_000_000,
                    ended_micros: 50_000_000,
                    wall_time_micros: 10_000_000,
                    active_combat_micros: 9_000_000,
                    combat_windows: vec![],
                    closed_at_run_end: false,
                },
            ],
            manual_pauses: vec![],
            data_gap_count: 0,
            findings: vec![],
            submission_disposition: RunSubmissionDisposition::RankCandidate,
        };

        assert_eq!(
            history_segment_view_intervals(&run, &run.segments[1]),
            vec![(40_000_000, 50_000_000)]
        );
        assert_eq!(
            history_segment_edps_intervals(&run, &run.segments[1]),
            vec![(20_000_000, 30_000_000), (40_000_000, 50_000_000)]
        );
        let history = CombatTimelinePlugin::new().build_run_history(0, &run);
        let entire_run = history.views.iter().find(|view| view.id == "all").unwrap();
        let bossing = history.views.iter().find(|view| view.id == "boss").unwrap();
        let retry = history
            .views
            .iter()
            .find(|view| view.id == "retry:1")
            .unwrap();
        let true_time = history
            .views
            .iter()
            .find(|view| view.id == "true_time")
            .unwrap();

        assert_eq!(entire_run.elapsed_micros, 30_000_000);
        assert_eq!(history.game_time_micros, Some(30_000_000));
        assert_eq!(bossing.label, "Bossing");
        assert_eq!(bossing.elapsed_micros, 10_000_000);
        assert_eq!(bossing.active_combat_micros, 9_000_000);
        assert_eq!(retry.label, "Retry #1");
        assert_eq!(retry.kind, "retry");
        assert_eq!(retry.elapsed_micros, 10_000_000);
        assert_eq!(retry.active_combat_micros, 10_000_000);
        assert_eq!(true_time.elapsed_micros, 20_000_000);
        assert_eq!(history.true_time_micros, Some(20_000_000));

        let mut live_transition = run.clone();
        live_transition.terminal_state = RunTerminalState::Open;
        live_transition.authoritative_completion = false;
        live_transition.timing.ended_micros = None;
        live_transition.timing.observed_until_micros = 15_000_000;
        live_transition.timing.wall_time_micros = None;
        live_transition.timing.noncombat_micros = None;
        live_transition.segments.truncate(1);
        live_transition.encounters.truncate(1);
        live_transition.submission_disposition = RunSubmissionDisposition::NotCompleted;

        let transition_history = CombatTimelinePlugin::new().build_run_history(0, &live_transition);
        assert_eq!(transition_history.terminal_state, "open");
        assert_eq!(transition_history.ended_micros, None);
        assert_eq!(transition_history.game_time_micros, Some(10_000_000));
        assert_eq!(
            transition_history
                .views
                .iter()
                .find(|view| view.id == "all")
                .unwrap()
                .elapsed_micros,
            10_000_000,
            "Game time must remain frozen after mobbing closes even as later packets advance the run"
        );

        let meter = CombatTimelinePlugin::new();
        let mut open_mobbing = live_transition.clone();
        open_mobbing.timing.observed_until_micros = 8_000_000;
        open_mobbing.timing.active_combat_micros = 7_000_000;
        open_mobbing.segments[0].ended_micros = 8_000_000;
        open_mobbing.segments[0].wall_time_micros = 8_000_000;
        open_mobbing.segments[0].active_combat_micros = 7_000_000;
        let mut live_history = meter.build_run_history(0, &open_mobbing);
        live_history.presentation_scene_name = Some("Localized scene".into());
        live_history.views[0].actors.reserve(4);
        let retained_actor_capacity = live_history.views[0].actors.capacity();

        open_mobbing.timing.observed_until_micros = 10_000_000;
        open_mobbing.timing.active_combat_micros = 8_000_000;
        open_mobbing.segments[0].ended_micros = 10_000_000;
        open_mobbing.segments[0].wall_time_micros = 10_000_000;
        open_mobbing.segments[0].active_combat_micros = 8_000_000;
        assert!(meter.try_refresh_run_history_metadata(&mut live_history, 0, &open_mobbing));
        assert_eq!(live_history.game_time_micros, Some(10_000_000));
        assert_eq!(
            live_history.presentation_scene_name.as_deref(),
            Some("Localized scene")
        );
        assert_eq!(
            live_history.views[0].actors.capacity(),
            retained_actor_capacity
        );

        let before_structure_change = live_history.clone();
        assert!(
            !meter.try_refresh_run_history_metadata(&mut live_history, 0, &run),
            "opening boss/retry views must request one new exact history projection"
        );
        assert_eq!(live_history, before_structure_change);
    }
}
