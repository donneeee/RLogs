//! Indexed matching-build evidence tracking for the BPSR rDPS research gate.
//!
//! The generated validation manifest is research input, not an rDPS formula
//! pack. This analyzer records which exact canonical event families were
//! observed for each proof obligation. It never promotes a relationship merely
//! because all event families appeared, and it never scans every obligation for
//! every event.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use rlogs_combat::{ExactDamageContributionEvent, ExactRationalDamageContributionEvent};
use rlogs_events::{
    ActorEvent, ActorLoadoutEvidence, ActorLoadoutSlot, ActorState, CanonicalEvent, CooldownEvent,
    EntityAttributeEvent, EntityAttributeUpdateKind, EventEnvelope, HealingEvent, ResourceEvent,
    StatusEvent, TemporaryAttributeEvent, TimelineEvent, TimelineEventKind,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::rdps_runtime::{
    PromotedRemoteEffectMagnitudeModel, promoted_remote_effect_magnitude_model,
};
use crate::state_rdps::{RemoteRdpsEvidencePolicy, remote_rdps_evidence_policy};
use crate::{
    BPSR_GAME_PLUGIN_ID, BPSR_PROFILE_SCHEMA_ID, BPSR_PROFILE_SCHEMA_VERSION, BpsrFightSourceKind,
    CharacterProfilePatch, DecoderKind, DreamscopeEvidenceMatch, DreamscopeEvidenceResolution,
    EffectDreamscopeSourceKind, EffectFingerprintMatchKind, EffectFingerprintResolution,
    ExactDreamscopeLoadout, ProtocolPack, ShieldListSnapshot, character_id_from_entity_uuid,
    decode_known_entity_attribute_value, decode_shield_list, dreamscope_observed_effect_match,
    resolve_dreamscope_effect_owner, resolve_status_effect_fingerprint,
};

pub const RDPS_VALIDATION_REPORT_SCHEMA_VERSION: u16 = 10;
const BUNDLED_VALIDATION_WATCH: &str =
    include_str!("../game-data/runtime/rdps-validation-watch.candidate.v1.json");

const ACTOR: u16 = 1 << 0;
const CAST: u16 = 1 << 1;
const DAMAGE: u16 = 1 << 2;
const STATUS: u16 = 1 << 3;
const ENTITY_ATTRIBUTES: u16 = 1 << 4;
const TEMPORARY_ATTRIBUTES: u16 = 1 << 5;
const FORMULA_INPUTS: u16 = 1 << 6;
const PROFILE_SELECTION: u16 = 1 << 7;
const RESOURCE: u16 = 1 << 8;
const COOLDOWN: u16 = 1 << 9;
const HEALING: u16 = 1 << 10;
const SHIELD_STATE: u16 = 1 << 11;

#[derive(Debug, Error)]
pub enum RdpsValidationError {
    #[error("invalid rDPS validation manifest JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported rDPS validation manifest schema {0}")]
    UnsupportedSchema(u16),
    #[error("unsupported rDPS validation report schema {0}")]
    UnsupportedReportSchema(u16),
    #[error("rDPS validation manifest has no game build")]
    MissingGameBuild,
    #[error("duplicate rDPS validation obligation ID {0}")]
    DuplicateObligation(String),
    #[error("rDPS validation obligation {0} has no required event kinds")]
    MissingRequiredEvents(String),
    #[error("rDPS validation obligation {obligation_id} uses unknown event kind {kind}")]
    UnknownEventKind { obligation_id: String, kind: String },
    #[error("rDPS validation obligation {obligation_id} uses unknown validation route {route}")]
    UnknownValidationRoute {
        obligation_id: String,
        route: String,
    },
    #[error("rDPS validation obligation {obligation_id} has an invalid named route: {reason}")]
    InvalidNamedRoute {
        obligation_id: String,
        reason: String,
    },
    #[error(
        "rDPS validation obligation {obligation_id} has invalid formula input {input_key}: {reason}"
    )]
    InvalidFormulaInput {
        obligation_id: String,
        input_key: String,
        reason: String,
    },
    #[error("incompatible rDPS validation report: {0}")]
    IncompatibleReport(String),
    #[error("invalid numeric value {value:?} in rDPS validation report field {field}")]
    InvalidReportNumber { field: String, value: String },
    #[error("protocol pack cannot collect required rDPS evidence families: {missing_event_kinds}")]
    MissingProtocolCapabilities { missing_event_kinds: String },
}

#[derive(Debug, Clone, Deserialize)]
struct ValidationManifest {
    schema_version: u16,
    game_build: String,
    #[serde(default)]
    validation_report_schema: Option<u16>,
    #[serde(default)]
    damage_packet_selectors: Vec<DamagePacketSelector>,
    obligations: Vec<ValidationObligation>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
struct DamagePacketSelector {
    damage_id: i64,
    ability_id: i64,
    hit_event_id: i32,
}

#[derive(Debug, Clone, Deserialize)]
struct ValidationObligation {
    obligation_id: String,
    domain: String,
    subject_kind: String,
    subject_id: String,
    subject_name: String,
    #[serde(default)]
    requirements: Vec<String>,
    required_event_kinds: Vec<String>,
    #[serde(default)]
    selectors: ValidationSelectors,
    #[serde(default)]
    formula_inputs: Vec<ValidationFormulaInput>,
    #[serde(default)]
    evidence: ValidationEvidence,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ValidationEvidence {
    #[serde(default)]
    validation_route: Option<String>,
    #[serde(default)]
    component_kind: Option<String>,
    #[serde(default)]
    property_ids: Vec<i32>,
}

#[derive(Debug, Clone, Deserialize)]
struct ValidationFormulaInput {
    input_key: String,
    label: String,
    #[serde(default = "default_formula_input_kind")]
    input_kind: String,
    actor_role: String,
    completion: String,
    #[serde(default)]
    candidate_attribute_ids: Vec<i64>,
    #[serde(default)]
    candidate_ability_ids: Vec<i64>,
    #[serde(default)]
    loadout_scope: Option<String>,
    #[serde(default)]
    allowed_tiers: Vec<u32>,
    #[serde(default)]
    class_attribute_routes: Vec<ValidationFormulaClassAttributeRoute>,
}

#[derive(Debug, Clone, Deserialize)]
struct ValidationFormulaClassAttributeRoute {
    class_ids: Vec<i32>,
    candidate_attribute_ids: Vec<i64>,
}

fn default_formula_input_kind() -> String {
    "attribute".into()
}

fn valid_class_attribute_routes(routes: &[ValidationFormulaClassAttributeRoute]) -> bool {
    if routes.is_empty() {
        return false;
    }
    let mut classes = BTreeSet::new();
    routes.iter().all(|route| {
        !route.class_ids.is_empty()
            && !route.candidate_attribute_ids.is_empty()
            && route
                .class_ids
                .iter()
                .all(|class_id| *class_id > 0 && classes.insert(*class_id))
            && route.candidate_attribute_ids.iter().all(|id| *id > 0)
    })
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ValidationSelectors {
    #[serde(default)]
    source_rule_ids: Vec<String>,
    #[serde(default)]
    effect_ids: Vec<i64>,
    #[serde(default)]
    skill_ids: Vec<i64>,
    #[serde(default)]
    damage_ids: Vec<i64>,
    #[serde(default)]
    recount_ids: Vec<i64>,
    #[serde(default)]
    attribute_ids: Vec<i64>,
    #[serde(default)]
    class_ids: Vec<i64>,
    #[serde(default)]
    specialization_ids: Vec<i64>,
    #[serde(default)]
    item_ids: Vec<i64>,
    #[serde(default)]
    source_config_ids: Vec<i64>,
    #[serde(default)]
    equipment_suit_entries: Vec<ValidationEquipmentSuitSelector>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct ValidationEquipmentSuitSelector {
    map_key: i32,
    attribute_key: i32,
}

#[derive(Debug, Clone)]
struct ObligationDefinition {
    obligation_id: String,
    domain: String,
    subject_kind: String,
    subject_id: String,
    subject_name: String,
    requirements: Vec<String>,
    selector_contract: String,
    required_mask: u16,
    has_item_selectors: bool,
    has_skill_selectors: bool,
    has_source_config_selectors: bool,
    has_equipment_suit_selectors: bool,
    class_selectors: BTreeSet<i64>,
    specialization_selectors: BTreeSet<i64>,
    formula_inputs: Vec<ValidationFormulaInput>,
    validation_route: Option<MasteryValidationRoute>,
    component_kind: Option<String>,
    property_ids: BTreeSet<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MasteryValidationRoute {
    OutgoingDamage,
    OutgoingSelectedAbilityDamage,
    OwnedCompanionOutgoingDamage,
    OutgoingHealing,
    OutgoingShieldOrBarrierState,
    NamedShieldState,
    IncomingDamageMitigation,
    OwnedResourceTransition,
    SelectedAbilityCooldownTransition,
    NamedSkillOutput,
    NamedStatusLifecycle,
    NamedResourceDecayLifecycle,
}

#[derive(Debug, Clone, Copy, Default)]
struct ValidationEventActors {
    source: Option<u64>,
    target: Option<u64>,
    direct_source: Option<u64>,
    blocked: Option<bool>,
    lucky: Option<bool>,
    property: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ValidationCooldownState {
    begin_time_millis: Option<i64>,
    duration_millis: Option<i32>,
    valid_duration_millis: Option<i32>,
    cooldown_type: Option<i32>,
    profession_hold_begin_time_millis: Option<i64>,
    charge_count: Option<i32>,
    valid_cooldown_time_millis: Option<i32>,
    sub_cooldown_ratio_raw: Option<i32>,
    sub_cooldown_fixed_raw: Option<i64>,
    accelerate_cooldown_ratio_raw: Option<i32>,
}

impl From<&CooldownEvent> for ValidationCooldownState {
    fn from(event: &CooldownEvent) -> Self {
        Self {
            begin_time_millis: event.begin_time_millis,
            duration_millis: event.duration_millis,
            valid_duration_millis: event.valid_duration_millis,
            cooldown_type: event.cooldown_type,
            profession_hold_begin_time_millis: event.profession_hold_begin_time_millis,
            charge_count: event.charge_count,
            valid_cooldown_time_millis: event.valid_cooldown_time_millis,
            sub_cooldown_ratio_raw: event.sub_cooldown_ratio_raw,
            sub_cooldown_fixed_raw: event.sub_cooldown_fixed_raw,
            accelerate_cooldown_ratio_raw: event.accelerate_cooldown_ratio_raw,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct LastAttributeValue {
    value: i64,
    sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DamagePacketEvidenceKey {
    context: u8,
    source_actor: u64,
    direct_source_actor: Option<u64>,
    target_actor: u64,
    ability_id: Option<i64>,
    hit_event_id: Option<i32>,
    owner_id: Option<i32>,
    damage_source: Option<i32>,
    damage_type: Option<i32>,
    type_flags: Option<i32>,
    property: Option<i32>,
    passive_uuid: Option<u32>,
    damage_mode: Option<i32>,
    skill_effect_uuid: Option<i64>,
    skill_effect_group_index: Option<u32>,
    skill_effect_component_index: Option<u32>,
    skill_effect_component_count: Option<u32>,
}

#[derive(Debug, Clone, Default)]
struct DamagePacketEvidenceAggregate {
    first_sequence: Option<u64>,
    last_sequence: Option<u64>,
    first_observed_micros: Option<u64>,
    last_observed_micros: Option<u64>,
    event_count: u64,
    amount: i128,
    actual_amount: i128,
    hp_loss: i128,
    shield_loss: i128,
    normal_value: i128,
    lucky_value: i128,
}

#[derive(Debug, Clone)]
struct RecentDamageEvent {
    sequence: u64,
    observed_micros: u64,
    event: rlogs_events::DamageEvent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct StatusWindowKey {
    target_actor: u64,
    effect_id: i64,
    instance_id: Option<i64>,
    /// Packets without an instance ID still need distinct concurrent windows
    /// when two providers apply the same effect to the same recipient.
    provider_discriminator: Option<u64>,
}

#[derive(Debug, Clone)]
struct ActiveValidationStatusWindow {
    obligations: Vec<usize>,
    dreamscope_effect_id: Option<i64>,
    provider_actor: Option<u64>,
    /// Canonical observation point for the apply/refresh/stack transition
    /// which opened this exact window. Damage observed at the same timestamp
    /// cannot be ordered against that transition and therefore remains
    /// unresolved rather than being assigned to the window.
    opened_sequence: u64,
    opened_observed_micros: u64,
    /// Packet-carried duration for this exact transition. `None` remains an
    /// unbounded/unknown window until an explicit terminal event; it is never
    /// replaced with a catalog or guessed duration.
    duration_millis: Option<u64>,
    /// Exact current stack count carried by the canonical status lifecycle.
    /// `None` is retained as unknown evidence and must never be guessed.
    current_stacks: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValidationStatusWindowMembership {
    Proven,
    UnresolvedOrder,
    Expired,
}

fn validation_status_window_membership(
    window: &ActiveValidationStatusWindow,
    damage_sequence: u64,
    damage_observed_micros: u64,
) -> ValidationStatusWindowMembership {
    if damage_observed_micros >= window.opened_observed_micros {
        let elapsed_micros = damage_observed_micros - window.opened_observed_micros;
        if window.duration_millis.is_some_and(|duration_millis| {
            u128::from(elapsed_micros) >= u128::from(duration_millis) * 1_000
        }) {
            return ValidationStatusWindowMembership::Expired;
        }
    }
    // Sequence order is a canonical serialization fact, not server operation
    // order. Require both a later sequence and a later observation timestamp;
    // equal-time rows remain available as unresolved evidence.
    if damage_sequence <= window.opened_sequence
        || damage_observed_micros <= window.opened_observed_micros
    {
        return ValidationStatusWindowMembership::UnresolvedOrder;
    }
    ValidationStatusWindowMembership::Proven
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct StatusWindowStackKey {
    effect_id: i64,
    instance_id: Option<i64>,
    provider_actor: Option<u64>,
    stacks: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct StackAtDamageKey {
    context: u8,
    windows: Vec<StatusWindowStackKey>,
}

#[derive(Debug, Clone, Default)]
struct StackAtDamageAggregate {
    event_count: u64,
    damage: i128,
}

/// Time-scoped packet evidence for one actor's selected combat sources.
///
/// Exact slot snapshots and unordered remote observations intentionally live
/// in different lanes. An unordered set may prove that a source was observed,
/// but it can never replace a packet-proven slot assignment. `Some([])` in an
/// exact lane is meaningful and clears stale equipment for that slot group.
#[derive(Debug, Clone, Default)]
struct ActorRuntimeSelectionState {
    entity_uuid: Option<i64>,
    class_id: Option<i32>,
    class_observed: Option<LoadoutObservationPoint>,
    specialization_id: Option<i32>,
    weapon_item_id: Option<i64>,
    primary_exact: Option<Vec<ActorLoadoutSlot>>,
    primary_exact_observed: Option<LoadoutObservationPoint>,
    primary_observed_set: Option<Vec<ActorLoadoutSlot>>,
    primary_observed_set_observed: Option<LoadoutObservationPoint>,
    auxiliary_exact: Option<Vec<ActorLoadoutSlot>>,
    auxiliary_exact_observed: Option<LoadoutObservationPoint>,
    auxiliary_observed_set: Option<Vec<ActorLoadoutSlot>>,
    auxiliary_observed_set_observed: Option<LoadoutObservationPoint>,
    last_sequence: u64,
    last_observed_micros: u64,
}

#[derive(Debug, Clone, Copy)]
struct LoadoutObservationPoint {
    sequence: u64,
    observed_micros: u64,
}

impl ActorRuntimeSelectionState {
    fn update(&mut self, sequence: u64, observed_micros: u64, event: &ActorEvent) {
        if event.state == ActorState::Spawned
            || self
                .entity_uuid
                .is_some_and(|entity_uuid| entity_uuid != event.actor.entity_uuid.0)
        {
            *self = Self::default();
        }
        self.entity_uuid = Some(event.actor.entity_uuid.0);
        if let Some(class_id) = event.class_id {
            self.class_id = Some(class_id);
            self.class_observed = Some(LoadoutObservationPoint {
                sequence,
                observed_micros,
            });
        }
        if let Some(specialization_id) = event.specialization_id {
            self.specialization_id = Some(specialization_id);
        }
        if let Some(weapon_item_id) = event.weapon_item_id {
            self.weapon_item_id = Some(weapon_item_id);
        }
        update_loadout_evidence_lane(
            event.loadout_observation.primary,
            &event.primary_loadout,
            &mut self.primary_exact,
            &mut self.primary_exact_observed,
            &mut self.primary_observed_set,
            &mut self.primary_observed_set_observed,
            sequence,
            observed_micros,
        );
        update_loadout_evidence_lane(
            event.loadout_observation.auxiliary,
            &event.auxiliary_loadout,
            &mut self.auxiliary_exact,
            &mut self.auxiliary_exact_observed,
            &mut self.auxiliary_observed_set,
            &mut self.auxiliary_observed_set_observed,
            sequence,
            observed_micros,
        );
        self.last_sequence = sequence;
        self.last_observed_micros = observed_micros;
    }

    fn selected_slots(&self) -> impl Iterator<Item = &ActorLoadoutSlot> {
        self.primary_exact
            .as_ref()
            .or(self.primary_observed_set.as_ref())
            .into_iter()
            .flatten()
            .chain(
                self.auxiliary_exact
                    .as_ref()
                    .or(self.auxiliary_observed_set.as_ref())
                    .into_iter()
                    .flatten(),
            )
    }
}

#[allow(clippy::too_many_arguments)]
fn update_loadout_evidence_lane(
    evidence: ActorLoadoutEvidence,
    slots: &[ActorLoadoutSlot],
    exact: &mut Option<Vec<ActorLoadoutSlot>>,
    exact_observed: &mut Option<LoadoutObservationPoint>,
    observed_set: &mut Option<Vec<ActorLoadoutSlot>>,
    observed_set_observed: &mut Option<LoadoutObservationPoint>,
    sequence: u64,
    observed_micros: u64,
) {
    let point = LoadoutObservationPoint {
        sequence,
        observed_micros,
    };
    match evidence {
        ActorLoadoutEvidence::Unobserved => {}
        ActorLoadoutEvidence::ObservedSet => {
            *observed_set = Some(slots.to_vec());
            *observed_set_observed = Some(point);
        }
        ActorLoadoutEvidence::ExactSlots => {
            *exact = Some(slots.to_vec());
            *exact_observed = Some(point);
            // The exact snapshot supersedes weaker observations at this point
            // in the timeline, including an exact empty snapshot.
            *observed_set = None;
            *observed_set_observed = None;
        }
    }
}

fn class_attribute_formula_input(
    selection: Option<&ActorRuntimeSelectionState>,
    input: &ValidationFormulaInput,
) -> (
    &'static str,
    Option<i32>,
    Option<LoadoutObservationPoint>,
    Vec<i64>,
) {
    let Some(selection) = selection else {
        return ("missing-current-class-state", None, None, Vec::new());
    };
    let Some(class_id) = selection.class_id else {
        return ("missing-current-class", None, None, Vec::new());
    };
    let observation = selection.class_observed;
    let Some(route) = input
        .class_attribute_routes
        .iter()
        .find(|route| route.class_ids.contains(&class_id))
    else {
        return (
            "unsupported-current-class",
            Some(class_id),
            observation,
            Vec::new(),
        );
    };
    (
        "route-selected",
        Some(class_id),
        observation,
        route.candidate_attribute_ids.clone(),
    )
}

fn loadout_formula_input(
    selection: Option<&ActorRuntimeSelectionState>,
    input: &ValidationFormulaInput,
) -> (&'static str, Vec<RdpsValidationFormulaLoadoutValue>) {
    let Some(selection) = selection else {
        return ("missing-current-loadout-state", Vec::new());
    };
    let scope = input.loadout_scope.as_deref().unwrap_or("any");
    let mut exact_snapshot_seen = false;
    let mut observed_snapshot_seen = false;
    let mut exact_values = Vec::new();
    let mut observed_values = Vec::new();
    if matches!(scope, "primary" | "any") {
        append_loadout_formula_values(
            "exact_slots",
            "primary",
            selection.primary_exact.as_deref(),
            selection.primary_exact_observed,
            input,
            &mut exact_snapshot_seen,
            &mut exact_values,
        );
        append_loadout_formula_values(
            "observed_set",
            "primary",
            selection.primary_observed_set.as_deref(),
            selection.primary_observed_set_observed,
            input,
            &mut observed_snapshot_seen,
            &mut observed_values,
        );
    }
    if matches!(scope, "auxiliary" | "any") {
        append_loadout_formula_values(
            "exact_slots",
            "auxiliary",
            selection.auxiliary_exact.as_deref(),
            selection.auxiliary_exact_observed,
            input,
            &mut exact_snapshot_seen,
            &mut exact_values,
        );
        append_loadout_formula_values(
            "observed_set",
            "auxiliary",
            selection.auxiliary_observed_set.as_deref(),
            selection.auxiliary_observed_set_observed,
            input,
            &mut observed_snapshot_seen,
            &mut observed_values,
        );
    }

    let mut values = exact_values.clone();
    values.extend(observed_values.iter().cloned());
    if exact_values.is_empty() {
        return if exact_snapshot_seen {
            ("selected-ability-not-present-in-exact-snapshot", values)
        } else if !observed_values.is_empty() {
            ("observed-set-only", values)
        } else {
            ("missing-exact-loadout-snapshot", values)
        };
    }
    if exact_values.iter().any(|value| value.tier.is_none()) {
        return ("missing-equipped-tier", values);
    }
    let tiers = exact_values
        .iter()
        .filter_map(|value| value.tier)
        .collect::<BTreeSet<_>>();
    if tiers.len() != 1 {
        return ("ambiguous-current-tier", values);
    }
    let tier = *tiers.iter().next().expect("one exact tier");
    if !input.allowed_tiers.contains(&tier) {
        return ("unsupported-current-tier", values);
    }
    ("complete", values)
}

#[allow(clippy::too_many_arguments)]
fn append_loadout_formula_values(
    evidence: &str,
    scope: &str,
    slots: Option<&[ActorLoadoutSlot]>,
    observed: Option<LoadoutObservationPoint>,
    input: &ValidationFormulaInput,
    snapshot_seen: &mut bool,
    output: &mut Vec<RdpsValidationFormulaLoadoutValue>,
) {
    let (Some(slots), Some(observed)) = (slots, observed) else {
        return;
    };
    *snapshot_seen = true;
    for slot in slots {
        if !slot
            .ability_id
            .is_some_and(|ability| input.candidate_ability_ids.contains(&ability))
        {
            continue;
        }
        output.push(RdpsValidationFormulaLoadoutValue {
            evidence: evidence.into(),
            scope: scope.into(),
            slot_id: slot.slot_id,
            ability_id: slot.ability_id.map(|value| value.to_string()),
            item_id: slot.item_id.map(|value| value.to_string()),
            tier: slot.tier,
            observation_sequence: observed.sequence,
            observation_observed_micros: observed.observed_micros,
        });
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ValidationResourceState {
    origin_energy_raw_bits: Option<u32>,
    resource_ids: Vec<u32>,
    resource_values: Vec<u32>,
}

#[derive(Debug, Clone, Default)]
struct ObligationState {
    observed_mask: u16,
    direct_matches: u64,
    contextual_matches: u64,
    first_sequence: Option<u64>,
    last_sequence: Option<u64>,
    matched_identifiers: BTreeSet<String>,
    status_states: BTreeMap<String, u64>,
    selected_actor_ids: BTreeSet<u64>,
    provider_recipient_observations: BTreeMap<(Option<u64>, u64, i64), u64>,
    status_origin_observations: BTreeMap<(Option<u64>, u64, i64, i32, i64), u64>,
    status_instance_ids: BTreeSet<i64>,
    minimum_stacks: Option<u32>,
    maximum_stacks: Option<u32>,
    maximum_concurrent_instances: u32,
    maximum_concurrent_providers: u32,
    ambiguous_status_removals: u64,
    direct_damage_events: u64,
    direct_damage: i128,
    recipient_window_damage_events: u64,
    recipient_window_damage: i128,
    unresolved_recipient_window_damage_events: u64,
    target_window_damage_events: u64,
    target_window_damage: i128,
    unresolved_target_window_damage_events: u64,
    expired_status_windows: u64,
    single_provider_window_damage_events: u64,
    single_provider_window_damage: i128,
    ambiguous_provider_window_damage_events: u64,
    stack_at_damage: BTreeMap<StackAtDamageKey, StackAtDamageAggregate>,
    formula_input_snapshots: Vec<RdpsValidationFormulaInputSnapshot>,
    packet_damage_rows: BTreeMap<DamagePacketEvidenceKey, DamagePacketEvidenceAggregate>,
    attribute_values: BTreeMap<String, BTreeSet<i64>>,
    attribute_transition_counts: BTreeMap<String, u64>,
    projection_statuses: BTreeSet<String>,
    projected_provider_recipient_observations: BTreeMap<(u64, u64, i64), u64>,
    projected_integer_events: u64,
    projected_integer_amount: i128,
    projected_integer_observed_damage: i128,
    projected_rational_events: u64,
    projected_rational_totals: BTreeMap<i128, (i128, u64)>,
    projected_rational_observed_damage: i128,
    projected_invalid_events: u64,
    projected_excluded_events: u64,
}

/// Packet-observed lifecycle evidence for one exact-build Dreamscope terminal
/// effect. This intentionally mirrors the shared validation window counters,
/// but does not promote the effect into rDPS until its source scope and formula
/// have been proven.
#[derive(Debug, Clone, Default)]
struct DreamscopeTerminalEffectState {
    status_states: BTreeMap<String, u64>,
    provider_recipient_observations: BTreeMap<(Option<u64>, u64), u64>,
    status_instance_ids: BTreeSet<i64>,
    packet_levels: BTreeMap<String, u64>,
    packet_part_ids: BTreeMap<String, u64>,
    packet_counts: BTreeMap<String, u64>,
    packet_durations_millis: BTreeMap<String, u64>,
    minimum_stacks: Option<u32>,
    maximum_stacks: Option<u32>,
    maximum_concurrent_instances: u32,
    maximum_concurrent_providers: u32,
    ambiguous_status_removals: u64,
    /// Active windows whose apply/refresh packet carried no duration and
    /// which have not yet received a matching terminal/refresh boundary.
    open_unbounded_status_windows: u64,
    recipient_window_damage_events: u64,
    recipient_window_damage: i128,
    unresolved_recipient_window_damage_events: u64,
    external_provider_window_damage_events: u64,
    external_provider_window_damage: i128,
    target_window_damage_events: u64,
    target_window_damage: i128,
    unresolved_target_window_damage_events: u64,
    expired_status_windows: u64,
    single_provider_window_damage_events: u64,
    single_provider_window_damage: i128,
    ambiguous_provider_window_damage_events: u64,
    stack_at_damage: BTreeMap<StackAtDamageKey, StackAtDamageAggregate>,
    source_observations: BTreeMap<DreamscopeSourceObservationKey, u64>,
    scalar_resolution: RdpsValidationRemoteScalarResolution,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DreamscopeSourceObservationKey {
    provider_actor_id: Option<u64>,
    source_type_id: Option<i32>,
    source_config_id: Option<i64>,
    match_kind: EffectFingerprintMatchKind,
    route_resolution: EffectFingerprintResolution,
    equipped_variant_resolution: EffectFingerprintResolution,
    resolution: EffectFingerprintResolution,
    source_id: Option<String>,
    source_kind: Option<String>,
    selected_factor_item_id: Option<i64>,
    selected_factor_grade: Option<i64>,
}

fn optional_packet_value<T: ToString>(value: Option<T>) -> String {
    value.map_or_else(|| "null".to_owned(), |value| value.to_string())
}

fn default_report_number() -> String {
    "0".to_owned()
}

fn dreamscope_remote_calculation_readiness(
    effect_id: i64,
    state: &DreamscopeTerminalEffectState,
) -> RdpsValidationRemoteCalculationReadiness {
    let policy = remote_rdps_evidence_policy();
    let (
        observed_provider_scope,
        self_provider_observations,
        external_provider_observations,
        unknown_provider_observations,
    ) = dreamscope_observed_provider_scope(state);
    // Only packet evidence with an external provider can become an rDPS
    // credit. Unknown scope remains a candidate so incomplete evidence is
    // never silently discarded. A self-only result is deliberately scoped to
    // these captures; it is not promoted into a universal game rule.
    let external_attribution_candidate =
        observed_provider_scope != RdpsValidationObservedProviderScope::ObservedSelfOnly;
    let build_metadata_required = policy.build_snapshot_required
        || policy.character_level_required
        || policy.exact_equipment_required
        || policy.exact_factor_tree_required;
    let route_exact = !state.source_observations.is_empty()
        && state
            .source_observations
            .keys()
            .all(|source| source.route_resolution == EffectFingerprintResolution::Exact);
    let provider_recipient_exact = !state.provider_recipient_observations.is_empty()
        && state
            .provider_recipient_observations
            .keys()
            .all(|(provider, _)| provider.is_some())
        && state
            .source_observations
            .keys()
            .all(|source| source.provider_actor_id.is_some());
    let promoted_scalar_resolution = match promoted_remote_effect_magnitude_model(effect_id) {
        Ok(Some(PromotedRemoteEffectMagnitudeModel::CounterfactualReplay)) => {
            RdpsValidationRemoteScalarResolution::CounterfactualReplay
        }
        Ok(None) | Err(_) => RdpsValidationRemoteScalarResolution::Unresolved,
    };
    let requires_recipient_damage_lane = matches!(
        promoted_scalar_resolution,
        RdpsValidationRemoteScalarResolution::CounterfactualReplay
    );
    let has_recipient_window_evidence = state.recipient_window_damage_events > 0
        || state.unresolved_recipient_window_damage_events > 0;
    let has_target_window_evidence =
        state.target_window_damage_events > 0 || state.unresolved_target_window_damage_events > 0;
    let recipient_window_lifecycle_exact = has_recipient_window_evidence
        && state.unresolved_recipient_window_damage_events == 0
        && state.ambiguous_status_removals == 0
        && state.open_unbounded_status_windows == 0;
    let target_window_lifecycle_exact = has_target_window_evidence
        && state.unresolved_target_window_damage_events == 0
        && state.ambiguous_status_removals == 0
        && state.open_unbounded_status_windows == 0;
    // Lifecycle readiness is damage-lane scoped. A terminal observed
    // elsewhere for the same numeric effect is not proof that a recipient or
    // target damage row belonged to the effect window. Every observed lane
    // must have at least one retained row and no unresolved membership.
    let lifecycle_exact = (!requires_recipient_damage_lane || recipient_window_lifecycle_exact)
        && (has_recipient_window_evidence || has_target_window_evidence)
        && (!has_recipient_window_evidence || recipient_window_lifecycle_exact)
        && (!has_target_window_evidence || target_window_lifecycle_exact);
    let scalar_resolution =
        strongest_remote_scalar_resolution(state.scalar_resolution, promoted_scalar_resolution);
    let scalar_exact = scalar_resolution != RdpsValidationRemoteScalarResolution::Unresolved;

    let mut blockers = Vec::new();
    if external_attribution_candidate {
        if observed_provider_scope == RdpsValidationObservedProviderScope::Unknown {
            blockers.push("observed_provider_scope".to_owned());
        }
        if !route_exact {
            blockers.push("exact_packet_or_formula_route".to_owned());
        }
        if !provider_recipient_exact {
            blockers.push("exact_provider_recipient".to_owned());
        }
        if !lifecycle_exact {
            if (requires_recipient_damage_lane || has_recipient_window_evidence)
                && !recipient_window_lifecycle_exact
            {
                blockers.push("exact_recipient_window_lifecycle".to_owned());
            }
            if has_target_window_evidence && !target_window_lifecycle_exact {
                blockers.push("exact_target_window_lifecycle".to_owned());
            }
            if !has_recipient_window_evidence && !has_target_window_evidence {
                blockers.push("exact_status_window_membership".to_owned());
            }
        }
        if !scalar_exact {
            blockers.push("runtime_applied_magnitude".to_owned());
        }
    }

    RdpsValidationRemoteCalculationReadiness {
        build_metadata_required,
        observed_provider_scope,
        self_provider_observations,
        external_provider_observations,
        unknown_provider_observations,
        external_attribution_candidate,
        route_exact,
        provider_recipient_exact,
        recipient_window_lifecycle_exact,
        target_window_lifecycle_exact,
        lifecycle_exact,
        scalar_resolution,
        calculation_ready: external_attribution_candidate && blockers.is_empty(),
        blockers,
    }
}

fn dreamscope_observed_provider_scope(
    state: &DreamscopeTerminalEffectState,
) -> (RdpsValidationObservedProviderScope, u64, u64, u64) {
    let mut self_observations = 0_u64;
    let mut external_observations = 0_u64;
    let mut unknown_observations = 0_u64;

    for (&(provider, recipient), &count) in &state.provider_recipient_observations {
        match provider {
            Some(provider) if provider == recipient => {
                self_observations = self_observations.saturating_add(count)
            }
            Some(_) => external_observations = external_observations.saturating_add(count),
            None => unknown_observations = unknown_observations.saturating_add(count),
        }
    }

    let scope =
        if unknown_observations > 0 || (self_observations == 0 && external_observations == 0) {
            RdpsValidationObservedProviderScope::Unknown
        } else if self_observations > 0 && external_observations > 0 {
            RdpsValidationObservedProviderScope::ObservedMixed
        } else if external_observations > 0 {
            RdpsValidationObservedProviderScope::ObservedExternalOnly
        } else {
            RdpsValidationObservedProviderScope::ObservedSelfOnly
        };

    (
        scope,
        self_observations,
        external_observations,
        unknown_observations,
    )
}

fn remote_scalar_resolution_key(resolution: RdpsValidationRemoteScalarResolution) -> &'static str {
    match resolution {
        RdpsValidationRemoteScalarResolution::Unresolved => "unresolved",
        RdpsValidationRemoteScalarResolution::PacketScalar => "packet_scalar",
        RdpsValidationRemoteScalarResolution::RecipientAttributeTransition => {
            "recipient_attribute_transition"
        }
        RdpsValidationRemoteScalarResolution::CounterfactualReplay => "counterfactual_replay",
    }
}

fn remote_rdps_readiness_ledger(
    states: &BTreeMap<i64, DreamscopeTerminalEffectState>,
) -> RdpsValidationRemoteReadinessLedger {
    let mut summary = RdpsValidationRemoteReadinessSummary::default();
    let mut retained_damage = 0_i128;
    let mut retained_external_damage = 0_i128;
    let mut effects = Vec::with_capacity(states.len());

    for (&effect_id, state) in states {
        let readiness = dreamscope_remote_calculation_readiness(effect_id, state);
        summary.observed_effects = summary.observed_effects.saturating_add(1);
        match readiness.observed_provider_scope {
            RdpsValidationObservedProviderScope::Unknown => {
                summary.unknown_provider_scope_effects =
                    summary.unknown_provider_scope_effects.saturating_add(1)
            }
            RdpsValidationObservedProviderScope::ObservedSelfOnly => {
                summary.observed_self_only_effects =
                    summary.observed_self_only_effects.saturating_add(1)
            }
            RdpsValidationObservedProviderScope::ObservedExternalOnly => {
                summary.observed_external_only_effects =
                    summary.observed_external_only_effects.saturating_add(1)
            }
            RdpsValidationObservedProviderScope::ObservedMixed => {
                summary.observed_mixed_effects = summary.observed_mixed_effects.saturating_add(1)
            }
        }
        if readiness.external_attribution_candidate {
            summary.external_attribution_candidate_effects = summary
                .external_attribution_candidate_effects
                .saturating_add(1);
            if readiness.calculation_ready {
                summary.calculation_ready_effects =
                    summary.calculation_ready_effects.saturating_add(1);
            } else {
                summary.unresolved_effects = summary.unresolved_effects.saturating_add(1);
            }
        } else {
            summary.non_external_observed_effects =
                summary.non_external_observed_effects.saturating_add(1);
        }
        if state.recipient_window_damage_events > 0 {
            summary.effects_with_retained_recipient_window_damage = summary
                .effects_with_retained_recipient_window_damage
                .saturating_add(1);
        }
        summary.retained_recipient_window_damage_events = summary
            .retained_recipient_window_damage_events
            .saturating_add(state.recipient_window_damage_events);
        retained_damage = retained_damage.saturating_add(state.recipient_window_damage);
        if state.external_provider_window_damage_events > 0 {
            summary.effects_with_retained_external_provider_window_damage = summary
                .effects_with_retained_external_provider_window_damage
                .saturating_add(1);
        }
        summary.retained_external_provider_window_damage_events = summary
            .retained_external_provider_window_damage_events
            .saturating_add(state.external_provider_window_damage_events);
        retained_external_damage =
            retained_external_damage.saturating_add(state.external_provider_window_damage);

        for blocker in &readiness.blockers {
            let count = summary.blockers.entry(blocker.clone()).or_default();
            *count = count.saturating_add(1);
        }
        let scalar_count = summary
            .scalar_resolutions
            .entry(remote_scalar_resolution_key(readiness.scalar_resolution).to_owned())
            .or_default();
        *scalar_count = scalar_count.saturating_add(1);

        effects.push(RdpsValidationRemoteEffectReadiness {
            effect_id: effect_id.to_string(),
            observed_provider_scope: readiness.observed_provider_scope,
            self_provider_observations: readiness.self_provider_observations,
            external_provider_observations: readiness.external_provider_observations,
            unknown_provider_observations: readiness.unknown_provider_observations,
            external_attribution_candidate: readiness.external_attribution_candidate,
            route_exact: readiness.route_exact,
            provider_recipient_exact: readiness.provider_recipient_exact,
            recipient_window_lifecycle_exact: readiness.recipient_window_lifecycle_exact,
            target_window_lifecycle_exact: readiness.target_window_lifecycle_exact,
            lifecycle_exact: readiness.lifecycle_exact,
            scalar_resolution: readiness.scalar_resolution,
            calculation_ready: readiness.calculation_ready,
            blockers: readiness.blockers,
            retained_recipient_window_damage_events: state.recipient_window_damage_events,
            retained_recipient_window_damage: state.recipient_window_damage.to_string(),
            retained_external_provider_window_damage_events: state
                .external_provider_window_damage_events,
            retained_external_provider_window_damage: state
                .external_provider_window_damage
                .to_string(),
        });
    }

    summary.retained_recipient_window_damage = retained_damage.to_string();
    summary.retained_external_provider_window_damage = retained_external_damage.to_string();
    RdpsValidationRemoteReadinessLedger {
        policy: remote_rdps_evidence_policy(),
        summary,
        effects,
    }
}

/// Returns the one concrete Dreamscope route identified by an exact packet
/// origin. Generated catalogs may retain a generic `phantom-factor` alias next
/// to the concrete factor-family candidate. That alias is useful provenance,
/// but it must not make the runtime route ambiguous when every concrete
/// Dreamscope selector names the same source family.
fn unique_dreamscope_route_source<'a>(
    fingerprint: &'a crate::ResolvedStatusEffectFingerprint<'_>,
) -> Option<&'a crate::EffectSourceCandidate> {
    if fingerprint.match_kind != EffectFingerprintMatchKind::ExactPacketOrigin {
        return None;
    }

    let mut concrete = fingerprint
        .candidate_sources
        .iter()
        .filter(|candidate| candidate.dreamscope_selector.is_some());
    let first = concrete.next()?;
    let first_selector = first
        .dreamscope_selector
        .as_ref()
        .expect("filtered Dreamscope candidate must have a selector");

    concrete
        .all(|candidate| {
            candidate
                .dreamscope_selector
                .as_ref()
                .is_some_and(|selector| {
                    selector.source_kind == first_selector.source_kind
                        && selector.source_id == first_selector.source_id
                })
        })
        .then_some(first)
}

#[derive(Debug, Clone, Default)]
struct NumericIndexes {
    effects: HashMap<i64, Vec<usize>>,
    skills: HashMap<i64, Vec<usize>>,
    damage: HashMap<i64, Vec<usize>>,
    recount: HashMap<i64, Vec<usize>>,
    attributes: HashMap<i64, Vec<usize>>,
    formula_input_attributes: HashMap<i64, Vec<usize>>,
    classes: HashMap<i64, Vec<usize>>,
    specializations: HashMap<i64, Vec<usize>>,
    items: HashMap<i64, Vec<usize>>,
    source_configs: HashMap<i64, Vec<usize>>,
    equipment_suits: HashMap<(i32, i32), Vec<usize>>,
    damage_packet: HashMap<(i64, i32), Vec<usize>>,
}

/// One-pass, indexed candidate-evidence tracker.
///
/// `candidate_event_coverage_complete` in its report means that all required
/// canonical event families were observed around a selected/directly matched
/// mechanic. It does not mean the mechanic has passed formula, counterfactual,
/// stacking, or conservation proof.
#[derive(Debug, Clone)]
pub struct RdpsValidationAnalyzer {
    manifest_build: String,
    definitions: Vec<ObligationDefinition>,
    states: Vec<ObligationState>,
    indexes: NumericIndexes,
    /// Dynamic mechanic context activated by directly observed events.
    actor_active: HashMap<u64, BTreeSet<usize>>,
    /// Packet-time actor selection snapshots. This is kept separate from
    /// dynamic mechanic activation so a sparse actor update cannot erase
    /// unrelated status/cast evidence.
    actor_selection_state: HashMap<u64, ActorRuntimeSelectionState>,
    /// Obligation context derived from the current packet-time selection
    /// state. Exact-empty snapshots can clear this lane without disturbing
    /// dynamic mechanic context.
    actor_selection_active: HashMap<u64, BTreeSet<usize>>,
    /// Exact current-season factor selections learned from local character
    /// profile snapshots, keyed by stable public character UID.
    factor_active_by_character: HashMap<String, BTreeSet<usize>>,
    /// Exact concrete Dreamscope factor item IDs from the same profile
    /// snapshots. These remain separate from inferred obligation matches so a
    /// terminal effect can never prove its own equipped source circularly.
    exact_factor_items_by_character: HashMap<String, Vec<i64>>,
    /// Exact equipment-set source selections learned from local character
    /// snapshots, keyed by stable public character UID.
    equipment_suit_active_by_character: HashMap<String, BTreeSet<usize>>,
    /// Source actors proven for an origin-specific obligation by either an
    /// exact status origin or their own exact equipment snapshot.
    source_origin_providers: HashMap<usize, BTreeSet<u64>>,
    /// Stable character UID associated with each current runtime actor.
    character_by_actor: HashMap<u64, String>,
    /// Actor identities observed during the current session. Later exact
    /// mechanic events reuse this bounded state instead of requiring an actor
    /// record and a status/damage event to arrive in the same envelope.
    observed_actor_sequences: HashMap<u64, u64>,
    /// Exact selected factors installed onto the corresponding runtime actor.
    factor_active_by_actor: HashMap<u64, BTreeSet<usize>>,
    /// Exact factor item IDs installed on a runtime actor after public
    /// character UID correlation.
    exact_factor_items_by_actor: HashMap<u64, Vec<i64>>,
    /// Last exact resource snapshot/delta fields per actor. A snapshot creates
    /// the baseline; only a subsequent changed value satisfies a transition
    /// obligation.
    resource_state_by_actor: HashMap<u64, ValidationResourceState>,
    /// Last exact cooldown state per actor and ability. The first observation
    /// is a baseline; only a later changed wire state satisfies a transition
    /// obligation.
    cooldown_state_by_actor_ability: HashMap<(u64, i64), ValidationCooldownState>,
    /// Exact entity-attribute 60050 baseline per actor. The first decoded list
    /// establishes state; only a later distinct list proves a shield change.
    shield_state_by_actor: HashMap<u64, ShieldListSnapshot>,
    /// Selector context learned from authoritative/sparse entity attributes.
    entity_attribute_active: HashMap<u64, BTreeSet<usize>>,
    /// Selector context learned from authoritative/sparse temporary attributes.
    temporary_attribute_active: HashMap<u64, BTreeSet<usize>>,
    status_windows: HashMap<StatusWindowKey, ActiveValidationStatusWindow>,
    status_active_counts: HashMap<u64, HashMap<usize, u32>>,
    status_providers: HashMap<(u64, usize), HashMap<Option<u64>, u32>>,
    dreamscope_terminal_effects: BTreeMap<i64, DreamscopeTerminalEffectState>,
    dreamscope_active_counts: HashMap<u64, HashMap<i64, u32>>,
    dreamscope_providers: HashMap<(u64, i64), HashMap<Option<u64>, u32>>,
    last_attribute_values: HashMap<(u64, u8, i64), LastAttributeValue>,
    /// Small ordering guard for server routes that emit damage immediately
    /// before the status event which identifies their source mechanic.
    recent_damage_events: VecDeque<RecentDamageEvent>,
    observed_builds: BTreeSet<String>,
    total_events: u64,
    relevant_events: u64,
    projected_integer_events: u64,
    projected_rational_events: u64,
    projected_invalid_events: u64,
    projected_excluded_events: u64,
    projection_statuses: BTreeSet<String>,
    unmatched_projected_effects: BTreeMap<i64, u64>,
    candidate_epoch: Vec<u64>,
    direct_epoch: Vec<u64>,
    contextual_epoch: Vec<u64>,
    activation_epoch: Vec<u64>,
    factor_selection_epoch: Vec<u64>,
    /// A factor event must match both the selected factor and a concrete
    /// mechanic signal (its source/output ID or an exact resource transition).
    factor_mechanic_epoch: Vec<u64>,
    /// Exact source origin for rules whose effect ID is intentionally shared
    /// by multiple equipment-set variants.
    source_selection_epoch: Vec<u64>,
    scratch_candidates: Vec<usize>,
    epoch: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RdpsValidationReport {
    pub schema_version: u16,
    pub manifest_game_build: String,
    pub observed_game_builds: Vec<String>,
    pub provisional_build_mismatch: bool,
    pub warnings: Vec<String>,
    pub total_events: u64,
    pub relevant_events: u64,
    pub projection: RdpsValidationProjectionSummary,
    pub summary: RdpsValidationSummary,
    pub by_domain: BTreeMap<String, RdpsValidationDomainSummary>,
    pub obligations: Vec<RdpsValidationObligationReport>,
    /// Current-build terminal effects observed on the wire and correlated
    /// through the same provider/recipient windows as the validation manifest.
    /// These are evidence records, not automatically enabled rDPS rules.
    #[serde(default)]
    pub dreamscope_terminal_effects: Vec<RdpsValidationDreamscopeTerminalEffectReport>,
    /// Compact, machine-readable progress ledger derived from the exact same
    /// per-effect evidence above. This is the authoritative answer to "what
    /// still blocks remote rDPS?"; it never treats a missing remote loadout as
    /// a blocker and never drops retained damage for an unresolved effect.
    #[serde(default)]
    pub remote_rdps_readiness: RdpsValidationRemoteReadinessLedger,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RdpsValidationRemoteEffectReadiness {
    pub effect_id: String,
    pub observed_provider_scope: RdpsValidationObservedProviderScope,
    pub self_provider_observations: u64,
    pub external_provider_observations: u64,
    pub unknown_provider_observations: u64,
    pub external_attribution_candidate: bool,
    pub route_exact: bool,
    pub provider_recipient_exact: bool,
    #[serde(default)]
    pub recipient_window_lifecycle_exact: bool,
    #[serde(default)]
    pub target_window_lifecycle_exact: bool,
    pub lifecycle_exact: bool,
    pub scalar_resolution: RdpsValidationRemoteScalarResolution,
    pub calculation_ready: bool,
    pub blockers: Vec<String>,
    pub retained_recipient_window_damage_events: u64,
    pub retained_recipient_window_damage: String,
    pub retained_external_provider_window_damage_events: u64,
    pub retained_external_provider_window_damage: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RdpsValidationRemoteReadinessSummary {
    pub observed_effects: u64,
    pub observed_self_only_effects: u64,
    pub observed_external_only_effects: u64,
    pub observed_mixed_effects: u64,
    pub unknown_provider_scope_effects: u64,
    pub external_attribution_candidate_effects: u64,
    pub non_external_observed_effects: u64,
    pub calculation_ready_effects: u64,
    pub unresolved_effects: u64,
    pub effects_with_retained_recipient_window_damage: u64,
    pub retained_recipient_window_damage_events: u64,
    pub retained_recipient_window_damage: String,
    pub effects_with_retained_external_provider_window_damage: u64,
    pub retained_external_provider_window_damage_events: u64,
    pub retained_external_provider_window_damage: String,
    pub blockers: BTreeMap<String, u64>,
    pub scalar_resolutions: BTreeMap<String, u64>,
}

impl Default for RdpsValidationRemoteReadinessSummary {
    fn default() -> Self {
        Self {
            observed_effects: 0,
            observed_self_only_effects: 0,
            observed_external_only_effects: 0,
            observed_mixed_effects: 0,
            unknown_provider_scope_effects: 0,
            external_attribution_candidate_effects: 0,
            non_external_observed_effects: 0,
            calculation_ready_effects: 0,
            unresolved_effects: 0,
            effects_with_retained_recipient_window_damage: 0,
            retained_recipient_window_damage_events: 0,
            retained_recipient_window_damage: "0".to_owned(),
            effects_with_retained_external_provider_window_damage: 0,
            retained_external_provider_window_damage_events: 0,
            retained_external_provider_window_damage: "0".to_owned(),
            blockers: BTreeMap::new(),
            scalar_resolutions: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RdpsValidationRemoteReadinessLedger {
    pub policy: RemoteRdpsEvidencePolicy,
    pub summary: RdpsValidationRemoteReadinessSummary,
    pub effects: Vec<RdpsValidationRemoteEffectReadiness>,
}

impl Default for RdpsValidationRemoteReadinessLedger {
    fn default() -> Self {
        Self {
            policy: remote_rdps_evidence_policy(),
            summary: RdpsValidationRemoteReadinessSummary::default(),
            effects: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RdpsValidationDreamscopeTerminalEffectReport {
    pub effect_id: String,
    pub source_match: DreamscopeEvidenceMatch,
    pub status_states: BTreeMap<String, u64>,
    pub provider_recipient_observations: Vec<RdpsValidationProviderRecipientObservation>,
    pub status_instance_ids: Vec<String>,
    /// Raw status fields retained exactly as observed. These values are
    /// evidence inputs, not assumed factor grades or build levels.
    #[serde(default)]
    pub packet_levels: BTreeMap<String, u64>,
    #[serde(default)]
    pub packet_part_ids: BTreeMap<String, u64>,
    #[serde(default)]
    pub packet_counts: BTreeMap<String, u64>,
    #[serde(default)]
    pub packet_durations_millis: BTreeMap<String, u64>,
    pub minimum_stacks: Option<u32>,
    pub maximum_stacks: Option<u32>,
    pub maximum_concurrent_instances: u32,
    pub maximum_concurrent_providers: u32,
    pub ambiguous_status_removals: u64,
    #[serde(default)]
    pub open_unbounded_status_windows: u64,
    pub recipient_window_damage_events: u64,
    pub recipient_window_damage: String,
    #[serde(default)]
    pub unresolved_recipient_window_damage_events: u64,
    #[serde(default)]
    pub external_provider_window_damage_events: u64,
    #[serde(default = "default_report_number")]
    pub external_provider_window_damage: String,
    pub target_window_damage_events: u64,
    pub target_window_damage: String,
    #[serde(default)]
    pub unresolved_target_window_damage_events: u64,
    #[serde(default)]
    pub expired_status_windows: u64,
    pub single_provider_window_damage_events: u64,
    pub single_provider_window_damage: String,
    pub ambiguous_provider_window_damage_events: u64,
    /// Exact active effect instances, providers, and resulting stack counts at
    /// the moment retained damage occurred. These observations are evidence
    /// for later formulas; they are not attribution guesses.
    #[serde(default)]
    pub stack_at_damage_observations: Vec<RdpsValidationStackAtDamageObservation>,
    /// Provider-specific source resolution from the exact packet endpoint and,
    /// where necessary, that provider's exact captured factor selection.
    /// Unresolved and ambiguous rows are retained rather than hidden.
    #[serde(default)]
    pub source_observations: Vec<RdpsValidationDreamscopeSourceObservation>,
    /// Whether this terminal effect can be calculated for a remote player
    /// without reconstructing that player's private build.
    #[serde(default)]
    pub remote_calculation: RdpsValidationRemoteCalculationReadiness,
}

/// How the exact runtime magnitude applied by a remote effect was proven.
/// Exact build metadata is deliberately absent: it may explain the magnitude,
/// but is never a prerequisite for calculating packet-observed rDPS.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RdpsValidationRemoteScalarResolution {
    #[default]
    Unresolved,
    PacketScalar,
    RecipientAttributeTransition,
    CounterfactualReplay,
}

/// Provider scope proven by the packet observations in this report. This is a
/// capture-scoped fact, not a claim that the effect can never behave
/// differently in another build or unobserved configuration.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RdpsValidationObservedProviderScope {
    #[default]
    Unknown,
    ObservedSelfOnly,
    ObservedExternalOnly,
    ObservedMixed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RdpsValidationRemoteCalculationReadiness {
    /// Always false. Remote factor grade, equipment level, and exact tree are
    /// optional explanation metadata, never rDPS calculation requirements.
    pub build_metadata_required: bool,
    pub observed_provider_scope: RdpsValidationObservedProviderScope,
    pub self_provider_observations: u64,
    pub external_provider_observations: u64,
    pub unknown_provider_observations: u64,
    pub external_attribution_candidate: bool,
    pub route_exact: bool,
    pub provider_recipient_exact: bool,
    /// Whether every observed recipient-outgoing row had provable membership
    /// in the exact status window at its canonical observation timestamp.
    #[serde(default)]
    pub recipient_window_lifecycle_exact: bool,
    /// Whether every observed target-incoming row had provable membership in
    /// the exact status window at its canonical observation timestamp.
    #[serde(default)]
    pub target_window_lifecycle_exact: bool,
    pub lifecycle_exact: bool,
    pub scalar_resolution: RdpsValidationRemoteScalarResolution,
    pub calculation_ready: bool,
    pub blockers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RdpsValidationDreamscopeSourceObservation {
    pub provider_actor_id: Option<String>,
    pub source_type_id: Option<i32>,
    pub source_config_id: Option<String>,
    pub match_kind: EffectFingerprintMatchKind,
    /// Certainty of the packet/formula route to the source family. An exact
    /// route does not imply that a factor grade or tree selection is known.
    #[serde(default)]
    pub route_resolution: EffectFingerprintResolution,
    /// Certainty of the provider's concrete equipped variant at event time.
    /// This becomes exact only from captured loadout/profile evidence.
    #[serde(default)]
    pub equipped_variant_resolution: EffectFingerprintResolution,
    /// Backward-compatible alias for `route_resolution` in report consumers.
    pub resolution: EffectFingerprintResolution,
    pub source_id: Option<String>,
    pub source_kind: Option<String>,
    #[serde(default)]
    pub selected_factor_item_id: Option<String>,
    #[serde(default)]
    pub selected_factor_grade: Option<i64>,
    pub observation_count: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RdpsValidationSummary {
    pub total_obligations: u64,
    pub no_candidate_evidence: u64,
    pub partial_candidate_event_coverage: u64,
    pub candidate_event_coverage_complete: u64,
    pub proof_promotions: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RdpsValidationDomainSummary {
    pub total: u64,
    pub no_candidate_evidence: u64,
    pub partial_candidate_event_coverage: u64,
    pub candidate_event_coverage_complete: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RdpsValidationProgress {
    pub total_obligations: u64,
    pub no_candidate_evidence: u64,
    pub partial_candidate_event_coverage: u64,
    pub candidate_event_coverage_complete: u64,
    pub by_domain: BTreeMap<String, RdpsValidationDomainSummary>,
}

/// Static capture-readiness check between a validation manifest and one
/// selected protocol pack. This proves only that the pack has decoder paths
/// capable of emitting every required canonical event family. It does not
/// claim that any selector, relationship, or formula was observed on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RdpsValidationCapturePreflight {
    pub manifest_game_build: String,
    pub protocol_pack_game_build: String,
    pub exact_build_match: bool,
    pub required_event_kinds: Vec<String>,
    pub available_event_kinds: Vec<String>,
    pub missing_event_kinds: Vec<String>,
    pub capture_capable: bool,
    pub exact_build_proof_capable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RdpsValidationObligationReport {
    pub obligation_id: String,
    pub domain: String,
    pub subject_kind: String,
    pub subject_id: String,
    pub subject_name: String,
    pub requirements: Vec<String>,
    pub selector_contract: String,
    pub coverage_state: String,
    pub required_event_kinds: Vec<String>,
    pub observed_event_kinds: Vec<String>,
    pub missing_event_kinds: Vec<String>,
    pub direct_matches: u64,
    pub contextual_matches: u64,
    pub first_sequence: Option<u64>,
    pub last_sequence: Option<u64>,
    pub matched_identifiers: Vec<String>,
    pub status_states: BTreeMap<String, u64>,
    pub selected_actor_ids: Vec<String>,
    pub provider_recipient_observations: Vec<RdpsValidationProviderRecipientObservation>,
    /// Exact child-effect and packet-origin tuples retained independently.
    /// The child effect remains the observed runtime identity; the origin
    /// config is evidence for a source edge, never a substitute effect ID.
    #[serde(default)]
    pub status_origin_observations: Vec<RdpsValidationStatusOriginObservation>,
    pub status_instance_ids: Vec<String>,
    pub minimum_stacks: Option<u32>,
    pub maximum_stacks: Option<u32>,
    pub maximum_concurrent_instances: u32,
    pub maximum_concurrent_providers: u32,
    pub ambiguous_status_removals: u64,
    pub direct_damage_events: u64,
    pub direct_damage: String,
    pub recipient_window_damage_events: u64,
    pub recipient_window_damage: String,
    #[serde(default)]
    pub unresolved_recipient_window_damage_events: u64,
    pub target_window_damage_events: u64,
    pub target_window_damage: String,
    #[serde(default)]
    pub unresolved_target_window_damage_events: u64,
    #[serde(default)]
    pub expired_status_windows: u64,
    pub single_provider_window_damage_events: u64,
    pub single_provider_window_damage: String,
    pub ambiguous_provider_window_damage_events: u64,
    /// Exact active effect instances, providers, and resulting stack counts at
    /// the moment retained damage occurred.
    #[serde(default)]
    pub stack_at_damage_observations: Vec<RdpsValidationStackAtDamageObservation>,
    /// Total snapshot count before an optional closure-only proof projection
    /// removes repeated rows from the derived audit artifact. Raw `.rlog`
    /// events remain the lossless source of truth.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formula_input_snapshot_count: Option<u64>,
    /// Count of snapshots whose captured formula inputs were complete before
    /// closure-only compaction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complete_formula_input_snapshot_count: Option<u64>,
    pub formula_input_snapshots: Vec<RdpsValidationFormulaInputSnapshot>,
    /// Total packet-damage evidence row count before closure-only compaction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packet_damage_row_count: Option<u64>,
    pub packet_damage_rows: Vec<RdpsValidationPacketDamageRow>,
    pub attribute_values: BTreeMap<String, Vec<String>>,
    pub attribute_transition_counts: BTreeMap<String, u64>,
    pub projection_statuses: Vec<String>,
    pub projected_provider_recipient_observations:
        Vec<RdpsValidationProjectedProviderRecipientObservation>,
    pub projected_integer_events: u64,
    pub projected_integer_amount: String,
    pub projected_integer_observed_damage: String,
    pub projected_rational_events: u64,
    pub projected_rational_totals: Vec<RdpsValidationRationalContributionTotal>,
    pub projected_rational_observed_damage: String,
    pub projected_invalid_events: u64,
    pub projected_excluded_events: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RdpsValidationFormulaInputSnapshot {
    /// Canonical source session for the trigger. Older schema-10 reports
    /// deserialize with an empty value and must not be grouped across files by
    /// sequence alone.
    #[serde(default)]
    pub session_id: String,
    pub trigger_sequence: u64,
    #[serde(default)]
    pub trigger_observed_micros: u64,
    /// Which endpoint of the triggering canonical event supplied `actor_id`.
    /// Older schema-10 reports deserialize with an empty value; consumers must
    /// not infer a role from allegiance or actor kind.
    #[serde(default)]
    pub actor_role: String,
    pub actor_id: Option<String>,
    /// Exact event-time class observation used by a class-selected input.
    /// Empty on older schema-10 reports and on non-class-selected inputs.
    #[serde(default)]
    pub class_id: Option<i32>,
    #[serde(default)]
    pub class_observation_sequence: Option<u64>,
    #[serde(default)]
    pub class_observation_observed_micros: Option<u64>,
    pub input_key: String,
    pub label: String,
    pub state: String,
    pub values: Vec<RdpsValidationFormulaInputValue>,
    #[serde(default)]
    pub loadout_values: Vec<RdpsValidationFormulaLoadoutValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RdpsValidationFormulaInputValue {
    pub lane: String,
    pub attribute_id: String,
    pub value: String,
    pub attribute_sequence: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RdpsValidationFormulaLoadoutValue {
    pub evidence: String,
    pub scope: String,
    pub slot_id: i32,
    pub ability_id: Option<String>,
    pub item_id: Option<String>,
    pub tier: Option<u32>,
    pub observation_sequence: u64,
    pub observation_observed_micros: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RdpsValidationPacketDamageRow {
    pub context: String,
    pub source_actor_id: String,
    pub direct_source_actor_id: Option<String>,
    pub target_actor_id: String,
    pub ability_id: Option<String>,
    pub hit_event_id: Option<i32>,
    pub owner_id: Option<i32>,
    pub damage_source: Option<i32>,
    pub damage_type: Option<i32>,
    pub type_flags: Option<i32>,
    pub property: Option<i32>,
    pub passive_uuid: Option<u32>,
    pub damage_mode: Option<i32>,
    pub skill_effect_uuid: Option<String>,
    pub skill_effect_group_index: Option<u32>,
    pub skill_effect_component_index: Option<u32>,
    pub skill_effect_component_count: Option<u32>,
    pub first_sequence: Option<u64>,
    pub last_sequence: Option<u64>,
    pub first_observed_micros: Option<u64>,
    pub last_observed_micros: Option<u64>,
    pub event_count: u64,
    pub amount: String,
    pub actual_amount: String,
    pub hp_loss: String,
    pub shield_loss: String,
    pub normal_value: String,
    pub lucky_value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RdpsValidationProviderRecipientObservation {
    pub provider_actor_id: Option<String>,
    pub recipient_actor_id: String,
    pub effect_id: String,
    pub observation_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RdpsValidationStatusOriginObservation {
    pub provider_actor_id: Option<String>,
    pub recipient_actor_id: String,
    pub effect_id: String,
    pub origin_source_type_id: i32,
    pub origin_source_config_id: String,
    pub observation_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RdpsValidationStackAtDamageObservation {
    pub context: String,
    pub active_windows: Vec<RdpsValidationActiveWindowStack>,
    pub event_count: u64,
    pub damage: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RdpsValidationActiveWindowStack {
    pub effect_id: String,
    pub status_instance_id: Option<String>,
    pub provider_actor_id: Option<String>,
    pub stacks: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RdpsValidationProjectionSummary {
    pub integer_events: u64,
    pub rational_events: u64,
    pub invalid_events: u64,
    pub excluded_events: u64,
    pub statuses: Vec<String>,
    pub unmatched_effects: Vec<RdpsValidationUnmatchedProjectedEffect>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RdpsValidationUnmatchedProjectedEffect {
    pub effect_id: String,
    pub observation_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RdpsValidationProjectedProviderRecipientObservation {
    pub provider_actor_id: String,
    pub recipient_actor_id: String,
    pub effect_id: String,
    pub observation_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RdpsValidationRationalContributionTotal {
    pub numerator: String,
    pub denominator: String,
    pub event_count: u64,
}

impl RdpsValidationAnalyzer {
    /// Loads the generated current-build watch pack embedded in the BPSR
    /// plug-in. The pack can collect candidate evidence but cannot promote an
    /// rDPS rule by itself.
    pub fn bundled() -> Result<Self, RdpsValidationError> {
        Self::from_manifest_json(BUNDLED_VALIDATION_WATCH)
    }

    pub fn from_manifest_json(json: &str) -> Result<Self, RdpsValidationError> {
        let manifest: ValidationManifest = serde_json::from_str(json)?;
        if manifest.schema_version != 2 {
            return Err(RdpsValidationError::UnsupportedSchema(
                manifest.schema_version,
            ));
        }
        if let Some(schema) = manifest.validation_report_schema
            && schema != RDPS_VALIDATION_REPORT_SCHEMA_VERSION
        {
            return Err(RdpsValidationError::UnsupportedReportSchema(schema));
        }
        if manifest.game_build.trim().is_empty() {
            return Err(RdpsValidationError::MissingGameBuild);
        }

        let mut definitions = Vec::with_capacity(manifest.obligations.len());
        let mut states = Vec::with_capacity(manifest.obligations.len());
        let mut indexes = NumericIndexes::default();
        let mut obligation_ids = BTreeSet::new();
        for obligation in manifest.obligations {
            if !obligation_ids.insert(obligation.obligation_id.clone()) {
                return Err(RdpsValidationError::DuplicateObligation(
                    obligation.obligation_id,
                ));
            }
            let required_mask =
                event_mask(&obligation.obligation_id, &obligation.required_event_kinds)?;
            if required_mask == 0 {
                return Err(RdpsValidationError::MissingRequiredEvents(
                    obligation.obligation_id,
                ));
            }
            let index = definitions.len();
            let validation_route = obligation
                .evidence
                .validation_route
                .as_deref()
                .map(|route| mastery_validation_route(&obligation.obligation_id, route))
                .transpose()?;
            if obligation.domain == "mastery-property" && validation_route.is_none() {
                return Err(RdpsValidationError::UnknownValidationRoute {
                    obligation_id: obligation.obligation_id,
                    route: "missing".into(),
                });
            }
            if matches!(
                validation_route,
                Some(
                    MasteryValidationRoute::NamedStatusLifecycle
                        | MasteryValidationRoute::NamedShieldState
                        | MasteryValidationRoute::NamedResourceDecayLifecycle
                )
            ) && obligation.selectors.effect_ids.is_empty()
            {
                return Err(RdpsValidationError::InvalidNamedRoute {
                    obligation_id: obligation.obligation_id.clone(),
                    reason:
                        "named status, shield, and resource routes require an exact effect selector"
                            .into(),
                });
            }
            for input in &obligation.formula_inputs {
                let valid_actor_role = matches!(input.actor_role.as_str(), "source" | "target");
                let valid_contract = match input.input_kind.as_str() {
                    "attribute" => {
                        input.completion == "any-current-value-observed-before-trigger"
                            && !input.candidate_attribute_ids.is_empty()
                            && input.candidate_ability_ids.is_empty()
                            && input.loadout_scope.is_none()
                            && input.allowed_tiers.is_empty()
                            && input.class_attribute_routes.is_empty()
                    }
                    "loadout_tier" => {
                        input.completion == "exact-current-equipped-tier-observed-before-trigger"
                            && input.candidate_attribute_ids.is_empty()
                            && !input.candidate_ability_ids.is_empty()
                            && matches!(
                                input.loadout_scope.as_deref(),
                                Some("primary" | "auxiliary" | "any")
                            )
                            && !input.allowed_tiers.is_empty()
                            && input.class_attribute_routes.is_empty()
                    }
                    "class_attribute" => {
                        input.completion
                            == "exact-current-class-selected-value-observed-before-trigger"
                            && input.candidate_attribute_ids.is_empty()
                            && input.candidate_ability_ids.is_empty()
                            && input.loadout_scope.is_none()
                            && input.allowed_tiers.is_empty()
                            && valid_class_attribute_routes(&input.class_attribute_routes)
                    }
                    _ => false,
                };
                if !valid_actor_role || !valid_contract {
                    return Err(RdpsValidationError::InvalidFormulaInput {
                        obligation_id: obligation.obligation_id.clone(),
                        input_key: input.input_key.clone(),
                        reason: "expected a supported source/target attribute or exact loadout-tier input contract".into(),
                    });
                }
                match input.input_kind.as_str() {
                    "attribute" => index_values(
                        &mut indexes.formula_input_attributes,
                        &input.candidate_attribute_ids,
                        index,
                    ),
                    "class_attribute" => {
                        for route in &input.class_attribute_routes {
                            index_values(
                                &mut indexes.formula_input_attributes,
                                &route.candidate_attribute_ids,
                                index,
                            );
                        }
                    }
                    _ => {}
                }
            }
            index_values(
                &mut indexes.effects,
                &obligation.selectors.effect_ids,
                index,
            );
            index_values(&mut indexes.skills, &obligation.selectors.skill_ids, index);
            index_values(&mut indexes.damage, &obligation.selectors.damage_ids, index);
            index_values(
                &mut indexes.recount,
                &obligation.selectors.recount_ids,
                index,
            );
            index_values(
                &mut indexes.attributes,
                &obligation.selectors.attribute_ids,
                index,
            );
            index_values(&mut indexes.classes, &obligation.selectors.class_ids, index);
            index_values(
                &mut indexes.specializations,
                &obligation.selectors.specialization_ids,
                index,
            );
            index_values(&mut indexes.items, &obligation.selectors.item_ids, index);
            index_values(
                &mut indexes.source_configs,
                &obligation.selectors.source_config_ids,
                index,
            );
            for selector in &obligation.selectors.equipment_suit_entries {
                if selector.map_key <= 0 || selector.attribute_key <= 0 {
                    return Err(RdpsValidationError::InvalidNamedRoute {
                        obligation_id: obligation.obligation_id.clone(),
                        reason: "equipment suit selectors require positive map and attribute keys"
                            .into(),
                    });
                }
                index_values(
                    &mut indexes.equipment_suits,
                    &[(selector.map_key, selector.attribute_key)],
                    index,
                );
            }
            let selector_contract = serde_json::to_string(&obligation.selectors)?;
            definitions.push(ObligationDefinition {
                obligation_id: obligation.obligation_id,
                domain: obligation.domain,
                subject_kind: obligation.subject_kind,
                subject_id: obligation.subject_id,
                subject_name: obligation.subject_name,
                requirements: obligation.requirements,
                selector_contract,
                required_mask,
                has_item_selectors: !obligation.selectors.item_ids.is_empty(),
                has_skill_selectors: !obligation.selectors.skill_ids.is_empty(),
                has_source_config_selectors: !obligation.selectors.source_config_ids.is_empty(),
                has_equipment_suit_selectors: !obligation
                    .selectors
                    .equipment_suit_entries
                    .is_empty(),
                class_selectors: obligation.selectors.class_ids.iter().copied().collect(),
                specialization_selectors: obligation
                    .selectors
                    .specialization_ids
                    .iter()
                    .copied()
                    .collect(),
                formula_inputs: obligation.formula_inputs,
                validation_route,
                component_kind: obligation.evidence.component_kind,
                property_ids: obligation.evidence.property_ids.into_iter().collect(),
            });
            states.push(ObligationState::default());
        }
        for selector in manifest.damage_packet_selectors {
            let Some(obligations) = indexes.damage.get(&selector.damage_id) else {
                continue;
            };
            let packet_obligations = indexes
                .damage_packet
                .entry((selector.ability_id, selector.hit_event_id))
                .or_default();
            for &obligation in obligations {
                if !packet_obligations.contains(&obligation) {
                    packet_obligations.push(obligation);
                }
            }
        }
        let obligation_count = definitions.len();

        Ok(Self {
            manifest_build: manifest.game_build,
            definitions,
            states,
            indexes,
            actor_active: HashMap::new(),
            actor_selection_state: HashMap::new(),
            actor_selection_active: HashMap::new(),
            factor_active_by_character: HashMap::new(),
            exact_factor_items_by_character: HashMap::new(),
            equipment_suit_active_by_character: HashMap::new(),
            source_origin_providers: HashMap::new(),
            character_by_actor: HashMap::new(),
            observed_actor_sequences: HashMap::new(),
            factor_active_by_actor: HashMap::new(),
            exact_factor_items_by_actor: HashMap::new(),
            resource_state_by_actor: HashMap::new(),
            cooldown_state_by_actor_ability: HashMap::new(),
            shield_state_by_actor: HashMap::new(),
            entity_attribute_active: HashMap::new(),
            temporary_attribute_active: HashMap::new(),
            status_windows: HashMap::new(),
            status_active_counts: HashMap::new(),
            status_providers: HashMap::new(),
            dreamscope_terminal_effects: BTreeMap::new(),
            dreamscope_active_counts: HashMap::new(),
            dreamscope_providers: HashMap::new(),
            last_attribute_values: HashMap::new(),
            recent_damage_events: VecDeque::new(),
            observed_builds: BTreeSet::new(),
            total_events: 0,
            relevant_events: 0,
            projected_integer_events: 0,
            projected_rational_events: 0,
            projected_invalid_events: 0,
            projected_excluded_events: 0,
            projection_statuses: BTreeSet::new(),
            unmatched_projected_effects: BTreeMap::new(),
            candidate_epoch: vec![0; obligation_count],
            direct_epoch: vec![0; obligation_count],
            contextual_epoch: vec![0; obligation_count],
            activation_epoch: vec![0; obligation_count],
            factor_selection_epoch: vec![0; obligation_count],
            factor_mechanic_epoch: vec![0; obligation_count],
            source_selection_epoch: vec![0; obligation_count],
            scratch_candidates: Vec::new(),
            epoch: 0,
        })
    }

    /// Audits whether `pack` can produce every canonical event family required
    /// by this manifest. Build mismatches remain capture-capable so hotfixes can
    /// be investigated, but are never reported as exact-build proof-capable.
    pub fn capture_preflight(&self, pack: &ProtocolPack) -> RdpsValidationCapturePreflight {
        let required_mask = self
            .definitions
            .iter()
            .fold(0_u16, |mask, definition| mask | definition.required_mask);
        let mut available_mask = pack
            .definition()
            .routes
            .iter()
            .filter_map(|route| pack.decoder(&route.route))
            .fold(0_u16, |mask, decoder| mask | decoder_event_mask(decoder));

        // Formula inputs are snapshots of exact source attributes observed
        // before a matching damage trigger. They are a derived evidence family,
        // so the pack must be able to emit both underlying families.
        if available_mask & ENTITY_ATTRIBUTES != 0 && available_mask & DAMAGE != 0 {
            available_mask |= FORMULA_INPUTS;
        }

        let missing_mask = required_mask & !available_mask;
        let exact_build_match = self.manifest_build == pack.definition().target.build_id;
        RdpsValidationCapturePreflight {
            manifest_game_build: self.manifest_build.clone(),
            protocol_pack_game_build: pack.definition().target.build_id.clone(),
            exact_build_match,
            required_event_kinds: mask_names(required_mask),
            available_event_kinds: mask_names(available_mask),
            missing_event_kinds: mask_names(missing_mask),
            capture_capable: missing_mask == 0,
            exact_build_proof_capable: missing_mask == 0 && exact_build_match,
        }
    }

    pub fn ensure_capture_capable(
        &self,
        pack: &ProtocolPack,
    ) -> Result<RdpsValidationCapturePreflight, RdpsValidationError> {
        let preflight = self.capture_preflight(pack);
        if !preflight.capture_capable {
            return Err(RdpsValidationError::MissingProtocolCapabilities {
                missing_event_kinds: preflight.missing_event_kinds.join(", "),
            });
        }
        Ok(preflight)
    }

    pub fn manifest_game_build(&self) -> &str {
        &self.manifest_build
    }

    /// Records the client build for a validation session before the first
    /// canonical event arrives. This makes an empty or interrupted session
    /// retain the same exact/provisional build identity as a populated one.
    pub fn observe_game_build(&mut self, game_build: impl Into<String>) {
        self.observed_builds.insert(game_build.into());
    }

    /// Clears actor-scoped correlation state between sealed sessions while
    /// retaining accumulated obligation evidence and build observations.
    pub fn begin_session(&mut self) {
        self.actor_active.clear();
        self.actor_selection_state.clear();
        self.actor_selection_active.clear();
        self.factor_active_by_character.clear();
        self.exact_factor_items_by_character.clear();
        self.equipment_suit_active_by_character.clear();
        self.source_origin_providers.clear();
        self.character_by_actor.clear();
        self.observed_actor_sequences.clear();
        self.factor_active_by_actor.clear();
        self.exact_factor_items_by_actor.clear();
        self.resource_state_by_actor.clear();
        self.cooldown_state_by_actor_ability.clear();
        self.shield_state_by_actor.clear();
        self.entity_attribute_active.clear();
        self.temporary_attribute_active.clear();
        self.status_windows.clear();
        self.status_active_counts.clear();
        self.status_providers.clear();
        self.dreamscope_active_counts.clear();
        self.dreamscope_providers.clear();
        self.last_attribute_values.clear();
        self.recent_damage_events.clear();
    }

    /// Clears recipient-scoped status windows at a dungeon boundary while
    /// retaining the latest exact actor loadout snapshot for the next run.
    pub fn clear_transient_context(&mut self) {
        self.entity_attribute_active.clear();
        self.temporary_attribute_active.clear();
        self.status_windows.clear();
        self.status_active_counts.clear();
        self.status_providers.clear();
        self.dreamscope_active_counts.clear();
        self.dreamscope_providers.clear();
        self.last_attribute_values.clear();
        self.cooldown_state_by_actor_ability.clear();
        self.shield_state_by_actor.clear();
        self.recent_damage_events.clear();
    }

    /// Merges one completed validation report into this analyzer.
    ///
    /// Reports are merged only after their schema, manifest build, obligation
    /// identity, and required event masks match exactly. Transient actor/status
    /// correlation state is intentionally not restored: each source report has
    /// already closed its own session, while the accumulated evidence remains
    /// available across captures and application restarts.
    pub fn merge_report(
        &mut self,
        report: &RdpsValidationReport,
    ) -> Result<(), RdpsValidationError> {
        let mut merged = self.clone();
        merged.merge_report_in_place(report)?;
        *self = merged;
        Ok(())
    }

    fn merge_report_in_place(
        &mut self,
        report: &RdpsValidationReport,
    ) -> Result<(), RdpsValidationError> {
        if report.schema_version != RDPS_VALIDATION_REPORT_SCHEMA_VERSION {
            return Err(RdpsValidationError::IncompatibleReport(format!(
                "schema {} does not match {}",
                report.schema_version, RDPS_VALIDATION_REPORT_SCHEMA_VERSION
            )));
        }
        if report.manifest_game_build != self.manifest_build {
            return Err(RdpsValidationError::IncompatibleReport(format!(
                "manifest build {} does not match {}",
                report.manifest_game_build, self.manifest_build
            )));
        }
        let observed_build_mismatch = report
            .observed_game_builds
            .iter()
            .any(|build| build != &self.manifest_build);
        if report.provisional_build_mismatch != observed_build_mismatch {
            return Err(RdpsValidationError::IncompatibleReport(
                "provisional build-mismatch flag disagrees with observed builds".into(),
            ));
        }
        if observed_build_mismatch {
            return Err(RdpsValidationError::IncompatibleReport(format!(
                "provisional evidence from observed build(s) {} cannot enter exact-build cumulative proof for {}",
                report.observed_game_builds.join(", "),
                self.manifest_build,
            )));
        }
        if report.obligations.len() != self.definitions.len() {
            return Err(RdpsValidationError::IncompatibleReport(format!(
                "obligation count {} does not match {}",
                report.obligations.len(),
                self.definitions.len()
            )));
        }

        let report_by_id = report
            .obligations
            .iter()
            .map(|obligation| (obligation.obligation_id.as_str(), obligation))
            .collect::<HashMap<_, _>>();
        if report_by_id.len() != report.obligations.len() {
            return Err(RdpsValidationError::IncompatibleReport(
                "report contains duplicate obligation IDs".into(),
            ));
        }

        for (index, definition) in self.definitions.iter().enumerate() {
            let Some(source) = report_by_id.get(definition.obligation_id.as_str()) else {
                return Err(RdpsValidationError::IncompatibleReport(format!(
                    "report is missing obligation {}",
                    definition.obligation_id
                )));
            };
            if source.domain != definition.domain
                || source.subject_kind != definition.subject_kind
                || source.subject_id != definition.subject_id
                || source.subject_name != definition.subject_name
                || source.requirements != definition.requirements
                || source.selector_contract != definition.selector_contract
            {
                return Err(RdpsValidationError::IncompatibleReport(format!(
                    "obligation {} metadata differs from the current manifest",
                    definition.obligation_id
                )));
            }
            let required_mask =
                event_mask(&definition.obligation_id, &source.required_event_kinds)?;
            if required_mask != definition.required_mask {
                return Err(RdpsValidationError::IncompatibleReport(format!(
                    "obligation {} required event mask differs from the current manifest",
                    definition.obligation_id
                )));
            }
            let observed_mask =
                event_mask(&definition.obligation_id, &source.observed_event_kinds)?;
            if observed_mask & !definition.required_mask != 0 {
                return Err(RdpsValidationError::IncompatibleReport(format!(
                    "obligation {} reports an event outside its required mask",
                    definition.obligation_id
                )));
            }
            merge_obligation_report(&mut self.states[index], source, observed_mask)?;
        }

        let mut terminal_effect_ids = BTreeSet::new();
        for source in &report.dreamscope_terminal_effects {
            let effect_id = parse_report_number::<i64>(
                "dreamscope_terminal_effects.effect_id",
                &source.effect_id,
            )?;
            if !terminal_effect_ids.insert(effect_id) {
                return Err(RdpsValidationError::IncompatibleReport(format!(
                    "report contains duplicate Dreamscope terminal effect ID {effect_id}"
                )));
            }
            let expected_match = dreamscope_observed_effect_match(effect_id);
            if source.source_match != expected_match {
                return Err(RdpsValidationError::IncompatibleReport(format!(
                    "Dreamscope terminal effect {effect_id} source mapping differs from the current-build catalog"
                )));
            }
            merge_dreamscope_terminal_effect_report(
                self.dreamscope_terminal_effects
                    .entry(effect_id)
                    .or_default(),
                effect_id,
                source,
            )?;
        }

        self.observed_builds
            .extend(report.observed_game_builds.iter().cloned());
        self.total_events = self.total_events.saturating_add(report.total_events);
        self.relevant_events = self.relevant_events.saturating_add(report.relevant_events);
        self.projected_integer_events = self
            .projected_integer_events
            .saturating_add(report.projection.integer_events);
        self.projected_rational_events = self
            .projected_rational_events
            .saturating_add(report.projection.rational_events);
        self.projected_invalid_events = self
            .projected_invalid_events
            .saturating_add(report.projection.invalid_events);
        self.projected_excluded_events = self
            .projected_excluded_events
            .saturating_add(report.projection.excluded_events);
        self.projection_statuses
            .extend(report.projection.statuses.iter().cloned());
        for unmatched in &report.projection.unmatched_effects {
            let effect_id = parse_report_number::<i64>(
                "projection.unmatched_effects.effect_id",
                &unmatched.effect_id,
            )?;
            let count = self
                .unmatched_projected_effects
                .entry(effect_id)
                .or_default();
            *count = count.saturating_add(unmatched.observation_count);
        }
        self.begin_session();
        Ok(())
    }

    pub fn observe(&mut self, envelope: &EventEnvelope) {
        self.observed_builds
            .insert(envelope.region.client_build.clone());
        self.total_events = self.total_events.saturating_add(1);
        match &envelope.event {
            CanonicalEvent::Timeline(timeline) => {
                self.observe_timeline(&envelope.session_id, envelope.sequence, timeline)
            }
            CanonicalEvent::CharacterProfileObserved { profile }
                if profile.game_plugin_id == BPSR_GAME_PLUGIN_ID
                    && profile.payload_schema_id == BPSR_PROFILE_SCHEMA_ID
                    && profile.payload_schema_version == BPSR_PROFILE_SCHEMA_VERSION =>
            {
                if let Ok(profile) = CharacterProfilePatch::from_game_event(profile) {
                    self.observe_profile_selection(envelope.sequence, &profile);
                }
            }
            _ => {}
        }
    }

    /// Audits the exact contribution terms already produced by the live meter
    /// for one canonical event. This consumes no packet state and performs no
    /// second formula projection; it only indexes the emitted terms back to the
    /// matching-build proof obligations that named their effect IDs.
    pub fn observe_projected_contributions(
        &mut self,
        event_sequence: u64,
        integer: &[ExactDamageContributionEvent],
        rational: &[ExactRationalDamageContributionEvent],
        projection_status: &str,
    ) {
        if integer.is_empty() && rational.is_empty() {
            return;
        }
        let projection_status = projection_status.trim();
        if !projection_status.is_empty() {
            self.projection_statuses
                .insert(projection_status.to_owned());
        }

        for contribution in integer {
            self.projected_integer_events = self.projected_integer_events.saturating_add(1);
            if !contribution.included {
                self.projected_excluded_events = self.projected_excluded_events.saturating_add(1);
            }
            let valid = contribution.provider_actor_id != contribution.recipient_actor_id
                && contribution.amount > 0
                && contribution.observed_damage > 0
                && contribution.amount <= contribution.observed_damage;
            if !valid {
                self.projected_invalid_events = self.projected_invalid_events.saturating_add(1);
            }
            self.observe_projected_integer(event_sequence, *contribution, projection_status, valid);
        }

        for contribution in rational {
            self.projected_rational_events = self.projected_rational_events.saturating_add(1);
            if !contribution.included {
                self.projected_excluded_events = self.projected_excluded_events.saturating_add(1);
            }
            let maximum =
                i128::from(contribution.observed_damage).checked_mul(contribution.denominator);
            let valid = contribution.provider_actor_id != contribution.recipient_actor_id
                && contribution.numerator > 0
                && contribution.denominator > 0
                && contribution.observed_damage > 0
                && maximum.is_some_and(|maximum| contribution.numerator <= maximum);
            if !valid {
                self.projected_invalid_events = self.projected_invalid_events.saturating_add(1);
            }
            self.observe_projected_rational(
                event_sequence,
                *contribution,
                projection_status,
                valid,
            );
        }
    }

    fn observe_projected_integer(
        &mut self,
        event_sequence: u64,
        contribution: ExactDamageContributionEvent,
        projection_status: &str,
        valid: bool,
    ) {
        let Some(obligations) = self.indexes.effects.get(&contribution.effect_id).cloned() else {
            *self
                .unmatched_projected_effects
                .entry(contribution.effect_id)
                .or_default() += 1;
            return;
        };
        for obligation in obligations {
            if self.definitions[obligation].has_source_config_selectors
                && !self
                    .source_origin_providers
                    .get(&obligation)
                    .is_some_and(|providers| providers.contains(&contribution.provider_actor_id))
            {
                continue;
            }
            let state = &mut self.states[obligation];
            observe_projection_common(
                state,
                event_sequence,
                contribution.effect_id,
                contribution.provider_actor_id,
                contribution.recipient_actor_id,
                projection_status,
            );
            state.projected_integer_events = state.projected_integer_events.saturating_add(1);
            if !contribution.included {
                state.projected_excluded_events = state.projected_excluded_events.saturating_add(1);
            } else if valid {
                state.projected_integer_amount = state
                    .projected_integer_amount
                    .saturating_add(i128::from(contribution.amount));
                state.projected_integer_observed_damage = state
                    .projected_integer_observed_damage
                    .saturating_add(i128::from(contribution.observed_damage));
            }
            if !valid {
                state.projected_invalid_events = state.projected_invalid_events.saturating_add(1);
            }
        }
    }

    fn observe_projected_rational(
        &mut self,
        event_sequence: u64,
        contribution: ExactRationalDamageContributionEvent,
        projection_status: &str,
        valid: bool,
    ) {
        let Some(obligations) = self.indexes.effects.get(&contribution.effect_id).cloned() else {
            *self
                .unmatched_projected_effects
                .entry(contribution.effect_id)
                .or_default() += 1;
            return;
        };
        for obligation in obligations {
            if self.definitions[obligation].has_source_config_selectors
                && !self
                    .source_origin_providers
                    .get(&obligation)
                    .is_some_and(|providers| providers.contains(&contribution.provider_actor_id))
            {
                continue;
            }
            let state = &mut self.states[obligation];
            observe_projection_common(
                state,
                event_sequence,
                contribution.effect_id,
                contribution.provider_actor_id,
                contribution.recipient_actor_id,
                projection_status,
            );
            state.projected_rational_events = state.projected_rational_events.saturating_add(1);
            if !contribution.included {
                state.projected_excluded_events = state.projected_excluded_events.saturating_add(1);
            } else if valid {
                let total = state
                    .projected_rational_totals
                    .entry(contribution.denominator)
                    .or_default();
                total.0 = total.0.saturating_add(contribution.numerator);
                total.1 = total.1.saturating_add(1);
                state.projected_rational_observed_damage = state
                    .projected_rational_observed_damage
                    .saturating_add(i128::from(contribution.observed_damage));
            }
            if !valid {
                state.projected_invalid_events = state.projected_invalid_events.saturating_add(1);
            }
        }
    }

    fn observe_timeline(&mut self, session_id: &str, sequence: u64, timeline: &TimelineEvent) {
        self.begin_event();
        match &timeline.kind {
            TimelineEventKind::Actor(event) => {
                self.observe_actor(sequence, timeline.time.observed_micros, event)
            }
            TimelineEventKind::EntityAttributes(event) => {
                self.observe_entity_attributes(sequence, event)
            }
            TimelineEventKind::TemporaryAttributes(event) => {
                self.observe_temporary_attributes(sequence, event)
            }
            TimelineEventKind::Cast(event) => {
                let actor_id = event.source.actor_id.0;
                self.add_index_matches(SelectorIndex::Skill, event.ability.0, "skill", false);
                if let Some(timing) = event.action_timing {
                    self.add_index_matches(
                        SelectorIndex::Skill,
                        timing.base_ability.0,
                        "base_skill",
                        false,
                    );
                }
                self.add_actor_context(actor_id, true);
                self.commit_matches(
                    sequence,
                    CAST,
                    ValidationEventActors {
                        source: Some(actor_id),
                        ..Default::default()
                    },
                    false,
                    None,
                );
            }
            TimelineEventKind::Cooldown(event) => self.observe_cooldown(sequence, event),
            TimelineEventKind::Resource(event) => self.observe_resource(sequence, event),
            TimelineEventKind::Healing(event) => self.observe_healing(sequence, event),
            TimelineEventKind::Damage(event) => {
                let actor_id = event.source.actor_id.0;
                if let Some(ability) = event.ability {
                    self.add_index_matches(SelectorIndex::Skill, ability.0, "skill", false);
                    self.add_index_matches(SelectorIndex::Recount, ability.0, "recount", false);
                }
                if let Some(owner_id) = event.packet.owner_id {
                    self.add_index_matches(
                        SelectorIndex::Skill,
                        i64::from(owner_id),
                        "owner_skill",
                        false,
                    );
                    self.add_index_matches(
                        SelectorIndex::Recount,
                        i64::from(owner_id),
                        "owner_recount",
                        false,
                    );
                }
                if let (Some(ability), Some(hit_event_id)) = (event.ability, event.hit_event_id) {
                    self.add_damage_packet_matches(ability.0, hit_event_id);
                }
                self.add_actor_context(actor_id, true);
                if let Some(direct) = event.direct_source {
                    self.add_actor_context(direct.actor_id.0, true);
                }
                self.add_actor_context(event.target.actor_id.0, false);
                self.mark_active_selected_factor_damage(
                    actor_id,
                    event.direct_source.map(|source| source.actor_id.0),
                    event.target.actor_id.0,
                );
                self.observe_damage_evidence(sequence, timeline.time.observed_micros, event);
                self.commit_matches(
                    sequence,
                    DAMAGE,
                    ValidationEventActors {
                        source: Some(actor_id),
                        target: Some(event.target.actor_id.0),
                        direct_source: event.direct_source.map(|source| source.actor_id.0),
                        blocked: event.flags.blocked,
                        lucky: event.flags.lucky,
                        property: event.packet.property,
                    },
                    false,
                    None,
                );
            }
            TimelineEventKind::Status(event) => {
                self.observe_status(session_id, sequence, timeline.time.observed_micros, event)
            }
            _ => {}
        }
    }

    fn observe_actor(&mut self, sequence: u64, observed_micros: u64, event: &ActorEvent) {
        let actor_id = event.actor.actor_id.0;
        self.observed_actor_sequences.insert(actor_id, sequence);
        let starts_new_lifetime = event.state == ActorState::Spawned
            || self
                .actor_selection_state
                .get(&actor_id)
                .and_then(|state| state.entity_uuid)
                .is_some_and(|entity_uuid| entity_uuid != event.actor.entity_uuid.0);
        if starts_new_lifetime || event.state == ActorState::Despawned {
            self.clear_actor_formula_context(actor_id);
        }
        if event.state == ActorState::Despawned {
            self.commit_matches(
                sequence,
                ACTOR,
                ValidationEventActors {
                    source: Some(actor_id),
                    ..Default::default()
                },
                false,
                None,
            );
            return;
        }
        if let Some(class_id) = event.class_id {
            self.add_index_matches(SelectorIndex::Class, i64::from(class_id), "class", false);
        }
        if let Some(spec_id) = event.specialization_id {
            self.add_index_matches(
                SelectorIndex::Specialization,
                i64::from(spec_id),
                "specialization",
                false,
            );
        }
        if let Some(item_id) = event.weapon_item_id {
            self.add_index_matches(SelectorIndex::Item, item_id, "weapon_item", true);
        }
        for slot in event
            .primary_loadout
            .iter()
            .chain(event.auxiliary_loadout.iter())
        {
            if let Some(ability_id) = slot.ability_id {
                self.add_index_matches(SelectorIndex::Skill, ability_id, "loadout_skill", true);
            }
            if let Some(item_id) = slot.item_id {
                self.add_index_matches(SelectorIndex::Item, item_id, "loadout_item", true);
            }
        }
        self.commit_matches(
            sequence,
            ACTOR,
            ValidationEventActors {
                source: Some(event.actor.actor_id.0),
                ..Default::default()
            },
            false,
            None,
        );
        self.actor_selection_state
            .entry(actor_id)
            .or_default()
            .update(sequence, observed_micros, event);
        self.replace_actor_selection_context(actor_id);
        if let Some(character_id) = character_id_from_entity_uuid(event.actor.entity_uuid.0) {
            self.character_by_actor
                .insert(event.actor.actor_id.0, character_id.clone());
            self.install_factor_actor_context(event.actor.actor_id.0, &character_id);
            self.install_equipment_suit_actor_context(event.actor.actor_id.0, &character_id);
        }
    }

    fn clear_actor_formula_context(&mut self, actor_id: u64) {
        self.actor_selection_state.remove(&actor_id);
        self.actor_selection_active
            .insert(actor_id, BTreeSet::new());
        self.entity_attribute_active.remove(&actor_id);
        self.temporary_attribute_active.remove(&actor_id);
        self.clear_attribute_values(actor_id, 0);
        self.clear_attribute_values(actor_id, 1);
        self.character_by_actor.remove(&actor_id);
        self.factor_active_by_actor.remove(&actor_id);
        self.exact_factor_items_by_actor.remove(&actor_id);
        self.resource_state_by_actor.remove(&actor_id);
        self.cooldown_state_by_actor_ability
            .retain(|(known_actor, _), _| *known_actor != actor_id);
        self.shield_state_by_actor.remove(&actor_id);
    }

    fn observe_profile_selection(&mut self, sequence: u64, profile: &CharacterProfilePatch) {
        self.begin_event();
        let mut exact_factor_item_ids = Vec::new();
        if let Some(cultivation) = &profile.season_cultivation {
            let current_season_id = profile
                .season
                .as_ref()
                .and_then(|season| season.season_id)
                .and_then(|season_id| i32::try_from(season_id).ok());
            if let Some(selected_season) = current_season_id
                .and_then(|season_id| {
                    cultivation
                        .iter()
                        .find(|entry| entry.season_id == season_id)
                })
                .or_else(|| cultivation.iter().max_by_key(|entry| entry.season_id))
            {
                for item_id in selected_season
                    .lines
                    .iter()
                    .flat_map(|line| &line.areas)
                    .filter(|area| area.active == Some(true))
                    .flat_map(|area| area.middle_node_item_ids.values().copied())
                {
                    exact_factor_item_ids.push(item_id);
                    self.add_index_matches(
                        SelectorIndex::Item,
                        item_id,
                        "profile_factor_item",
                        true,
                    );
                }
            }
        }
        exact_factor_item_ids.sort_unstable();
        exact_factor_item_ids.dedup();
        self.exact_factor_items_by_character.insert(
            profile.character.character_id.clone(),
            exact_factor_item_ids,
        );
        if let Some(entries) = &profile.equipment_suit_entries {
            for entry in entries {
                for &attribute_key in entry.attributes.keys() {
                    self.add_equipment_suit_match(entry.map_key, attribute_key);
                }
            }
        }
        let selected = self
            .direct_candidates()
            .into_iter()
            .filter(|&obligation| self.definitions[obligation].domain == "psychoscope-factor")
            .collect::<BTreeSet<_>>();
        for &obligation in &selected {
            self.states[obligation].matched_identifiers.insert(format!(
                "profile_character:{}",
                profile.character.character_id
            ));
        }
        let selection_is_empty = selected.is_empty();
        self.factor_active_by_character
            .insert(profile.character.character_id.clone(), selected);
        let selected_suits = self
            .direct_candidates()
            .into_iter()
            .filter(|&obligation| self.definitions[obligation].has_equipment_suit_selectors)
            .collect::<BTreeSet<_>>();
        self.equipment_suit_active_by_character.insert(
            profile.character.character_id.clone(),
            selected_suits.clone(),
        );
        let actors = self
            .character_by_actor
            .iter()
            .filter_map(|(&actor_id, character_id)| {
                (character_id == &profile.character.character_id).then_some(actor_id)
            })
            .collect::<Vec<_>>();
        for actor_id in actors {
            self.install_factor_actor_context(actor_id, &profile.character.character_id);
            for &obligation in &selected_suits {
                self.source_origin_providers
                    .entry(obligation)
                    .or_default()
                    .insert(actor_id);
                self.states[obligation].selected_actor_ids.insert(actor_id);
            }
        }
        if selection_is_empty && selected_suits.is_empty() {
            return;
        }
        self.commit_matches(
            sequence,
            PROFILE_SELECTION,
            ValidationEventActors::default(),
            false,
            None,
        );
    }

    fn install_factor_actor_context(&mut self, actor_id: u64, character_id: &str) {
        let selected = self
            .factor_active_by_character
            .get(character_id)
            .cloned()
            .unwrap_or_default();
        for &obligation in &selected {
            self.states[obligation].selected_actor_ids.insert(actor_id);
        }
        self.factor_active_by_actor.insert(actor_id, selected);
        let exact_items = self
            .exact_factor_items_by_character
            .get(character_id)
            .cloned()
            .unwrap_or_default();
        self.exact_factor_items_by_actor
            .insert(actor_id, exact_items);
    }

    fn install_equipment_suit_actor_context(&mut self, actor_id: u64, character_id: &str) {
        let selected = self
            .equipment_suit_active_by_character
            .get(character_id)
            .cloned()
            .unwrap_or_default();
        for obligation in selected {
            self.source_origin_providers
                .entry(obligation)
                .or_default()
                .insert(actor_id);
            self.states[obligation].selected_actor_ids.insert(actor_id);
        }
    }

    fn observe_cooldown(&mut self, sequence: u64, event: &CooldownEvent) {
        let actor_id = event.actor.actor_id.0;
        let key = (actor_id, event.ability.0);
        let next = ValidationCooldownState::from(event);
        let changed = self
            .cooldown_state_by_actor_ability
            .insert(key, next)
            .is_some_and(|previous| previous != next);
        if !changed {
            return;
        }
        self.add_index_matches(
            SelectorIndex::Skill,
            event.ability.0,
            "cooldown_skill",
            false,
        );
        self.add_actor_context(actor_id, true);
        self.mark_active_selected_factor_cooldown(actor_id, event.ability.0);
        self.commit_matches(
            sequence,
            COOLDOWN,
            ValidationEventActors {
                source: Some(actor_id),
                ..Default::default()
            },
            false,
            None,
        );
    }

    /// Some Reality factors alter cooldown flow without owning a distinct
    /// output skill ID in the current tables. Correlate those rows only when
    /// the exact factor is selected, its exact source status is currently
    /// active on this actor, and a real cooldown wire-state transition occurs.
    fn mark_active_selected_factor_cooldown(&mut self, actor_id: u64, ability_id: i64) {
        let selected = self
            .factor_active_by_actor
            .get(&actor_id)
            .map(|active| active.iter().copied().collect::<Vec<_>>())
            .unwrap_or_default();
        let active_status = self
            .status_active_counts
            .get(&actor_id)
            .map(|active| active.keys().copied().collect::<BTreeSet<_>>())
            .unwrap_or_default();
        for obligation in selected {
            if self.definitions[obligation].required_mask & COOLDOWN == 0
                || self.definitions[obligation].has_skill_selectors
                || !active_status.contains(&obligation)
            {
                continue;
            }
            self.mark_candidate(obligation, false, false);
            self.factor_selection_epoch[obligation] = self.epoch;
            self.factor_mechanic_epoch[obligation] = self.epoch;
            self.states[obligation].matched_identifiers.insert(format!(
                "cooldown_transition:{ability_id}:during-source-status"
            ));
        }
    }

    fn observe_resource(&mut self, sequence: u64, event: &ResourceEvent) {
        let actor_id = event.actor.actor_id.0;
        if event.origin_energy_raw_bits.is_none()
            && event.resource_ids.is_empty()
            && event.resource_values.is_empty()
        {
            return;
        }
        let previous = self.resource_state_by_actor.get(&actor_id).cloned();
        let mut next = if event.update_kind == EntityAttributeUpdateKind::Snapshot {
            ValidationResourceState::default()
        } else {
            previous.clone().unwrap_or_default()
        };
        if let Some(origin_energy_raw_bits) = event.origin_energy_raw_bits {
            next.origin_energy_raw_bits = Some(origin_energy_raw_bits);
        }
        if !event.resource_ids.is_empty() {
            next.resource_ids.clone_from(&event.resource_ids);
        }
        if !event.resource_values.is_empty() {
            next.resource_values.clone_from(&event.resource_values);
        }
        let changed = previous.is_some_and(|previous| previous != next);
        self.resource_state_by_actor.insert(actor_id, next);
        if !changed {
            return;
        }
        self.add_actor_context(actor_id, true);
        self.mark_active_selected_factor_resource(actor_id);
        self.commit_matches(
            sequence,
            RESOURCE,
            ValidationEventActors {
                source: Some(actor_id),
                ..Default::default()
            },
            false,
            None,
        );
    }

    fn mark_active_selected_factor_resource(&mut self, actor_id: u64) {
        let selected = self
            .factor_active_by_actor
            .get(&actor_id)
            .map(|active| active.iter().copied().collect::<Vec<_>>())
            .unwrap_or_default();
        let active_status = self
            .status_active_counts
            .get(&actor_id)
            .map(|active| active.keys().copied().collect::<BTreeSet<_>>())
            .unwrap_or_default();
        for obligation in selected {
            if self.definitions[obligation].required_mask & RESOURCE == 0
                || !active_status.contains(&obligation)
            {
                continue;
            }
            self.mark_candidate(obligation, false, false);
            self.factor_selection_epoch[obligation] = self.epoch;
            self.factor_mechanic_epoch[obligation] = self.epoch;
            self.states[obligation]
                .matched_identifiers
                .insert("resource:changed-during-source-status".into());
        }
    }

    /// Correlate damage without a unique factor-owned skill ID only while the
    /// exact factor status is active and its provider has that factor selected.
    /// This covers both recipient buffs and target debuffs without allowing a
    /// shared effect ID from an unselected provider to satisfy the obligation.
    fn mark_active_selected_factor_damage(
        &mut self,
        source_actor: u64,
        direct_source_actor: Option<u64>,
        target_actor: u64,
    ) {
        let mut actors = vec![source_actor, target_actor];
        if let Some(direct_source_actor) = direct_source_actor {
            actors.push(direct_source_actor);
        }
        actors.sort_unstable();
        actors.dedup();

        let mut matched = BTreeSet::new();
        for actor_id in actors {
            let Some(active) = self.status_active_counts.get(&actor_id) else {
                continue;
            };
            for &obligation in active.keys() {
                let definition = &self.definitions[obligation];
                if definition.domain != "psychoscope-factor"
                    || definition.required_mask & DAMAGE == 0
                {
                    continue;
                }
                let provider_selected = self
                    .status_providers
                    .get(&(actor_id, obligation))
                    .is_some_and(|providers| {
                        providers.iter().any(|(provider, count)| {
                            *count > 0
                                && provider.is_some_and(|provider| {
                                    self.factor_active_by_actor
                                        .get(&provider)
                                        .is_some_and(|selected| selected.contains(&obligation))
                                })
                        })
                    });
                if provider_selected {
                    matched.insert(obligation);
                }
            }
        }
        for obligation in matched {
            self.mark_candidate(obligation, false, false);
            self.factor_selection_epoch[obligation] = self.epoch;
            self.factor_mechanic_epoch[obligation] = self.epoch;
            self.states[obligation]
                .matched_identifiers
                .insert("damage:during-selected-factor-status".into());
        }
    }

    fn observe_entity_attributes(&mut self, sequence: u64, event: &EntityAttributeEvent) {
        if event.update_kind == EntityAttributeUpdateKind::Snapshot {
            self.clear_attribute_values(event.actor.actor_id.0, 0);
        }
        let actor_id = event.actor.actor_id.0;
        let mut shield_state_changed = false;
        for attribute in &event.attributes {
            self.add_index_matches(
                SelectorIndex::Attribute,
                i64::from(attribute.attribute_id),
                "entity_attribute",
                false,
            );
            let decoded = attribute.decoded.clone().or_else(|| {
                decode_known_entity_attribute_value(attribute.attribute_id, &attribute.raw_value)
            });
            if let Some(rlogs_events::EntityAttributeValue::Integer(value)) = decoded {
                self.observe_attribute_value(
                    sequence,
                    event.actor.actor_id.0,
                    0,
                    i64::from(attribute.attribute_id),
                    value,
                );
            }
            if attribute.attribute_id == 60_050 {
                if let Ok(next) = decode_shield_list(&attribute.raw_value) {
                    let previous = self.shield_state_by_actor.insert(actor_id, next.clone());
                    shield_state_changed = previous.is_some_and(|previous| previous != next);
                }
            }
        }
        let direct = self.direct_candidates();
        self.update_attribute_actor_context(actor_id, false, event.update_kind, &direct);
        self.add_actor_context(actor_id, true);
        let actors = ValidationEventActors {
            source: Some(actor_id),
            ..Default::default()
        };
        self.commit_matches(sequence, ENTITY_ATTRIBUTES, actors, false, None);
        if shield_state_changed {
            self.commit_matches(sequence, SHIELD_STATE, actors, false, None);
        }
    }

    fn observe_healing(&mut self, sequence: u64, event: &HealingEvent) {
        let actor_id = event.source.actor_id.0;
        if let Some(ability) = event.ability {
            self.add_index_matches(SelectorIndex::Skill, ability.0, "skill", false);
            self.add_index_matches(SelectorIndex::Recount, ability.0, "recount", false);
        }
        if let Some(owner_id) = event.packet.owner_id {
            self.add_index_matches(
                SelectorIndex::Skill,
                i64::from(owner_id),
                "owner_skill",
                false,
            );
            self.add_index_matches(
                SelectorIndex::Recount,
                i64::from(owner_id),
                "owner_recount",
                false,
            );
        }
        if let (Some(ability), Some(hit_event_id)) = (event.ability, event.hit_event_id) {
            self.add_damage_packet_matches(ability.0, hit_event_id);
        }
        self.add_actor_context(actor_id, true);
        if let Some(direct) = event.direct_source {
            self.add_actor_context(direct.actor_id.0, true);
        }
        self.add_actor_context(event.target.actor_id.0, false);
        self.commit_matches(
            sequence,
            HEALING,
            ValidationEventActors {
                source: Some(actor_id),
                target: Some(event.target.actor_id.0),
                direct_source: event.direct_source.map(|source| source.actor_id.0),
                ..Default::default()
            },
            false,
            None,
        );
    }

    fn observe_temporary_attributes(&mut self, sequence: u64, event: &TemporaryAttributeEvent) {
        if event.update_kind == EntityAttributeUpdateKind::Snapshot {
            self.clear_attribute_values(event.actor.actor_id.0, 1);
        }
        for attribute in &event.attributes {
            self.add_index_matches(
                SelectorIndex::Attribute,
                i64::from(attribute.id),
                "temporary_attribute",
                false,
            );
            self.observe_attribute_value(
                sequence,
                event.actor.actor_id.0,
                1,
                i64::from(attribute.id),
                i64::from(attribute.value),
            );
        }
        let actor_id = event.actor.actor_id.0;
        let direct = self.direct_candidates();
        self.update_attribute_actor_context(actor_id, true, event.update_kind, &direct);
        self.add_actor_context(actor_id, true);
        self.commit_matches(
            sequence,
            TEMPORARY_ATTRIBUTES,
            ValidationEventActors {
                source: Some(actor_id),
                ..Default::default()
            },
            false,
            None,
        );
    }

    fn observe_status(
        &mut self,
        session_id: &str,
        sequence: u64,
        observed_micros: u64,
        event: &StatusEvent,
    ) {
        let effect_fingerprint = resolve_status_effect_fingerprint(event);
        let dreamscope_match = dreamscope_observed_effect_match(event.effect.0);
        // The shared formula catalog is the authoritative runtime endpoint
        // inventory. Some packet-observed factor endpoints predate the
        // Dreamscope-only compatibility catalog, so retain them whenever a
        // source candidate carries an exact Dreamscope selector. Exact actor
        // loadout evidence may then disambiguate the source family without
        // deriving that loadout from the same terminal effect.
        let has_dreamscope_candidate = effect_fingerprint
            .candidate_sources
            .iter()
            .any(|candidate| candidate.dreamscope_selector.is_some());
        let dreamscope_effect_id = (dreamscope_match.resolution
            != DreamscopeEvidenceResolution::Unknown
            || has_dreamscope_candidate)
            .then_some(event.effect.0);
        self.add_index_matches(SelectorIndex::Effect, event.effect.0, "effect", false);
        if let Some(origin) = event.origin {
            match BpsrFightSourceKind::from_protocol_id(origin.source_type_id) {
                Some(BpsrFightSourceKind::Skill) => self.add_index_matches(
                    SelectorIndex::Skill,
                    origin.source_config_id,
                    "origin_skill",
                    false,
                ),
                Some(BpsrFightSourceKind::Buff) => self.add_index_matches(
                    SelectorIndex::Effect,
                    origin.source_config_id,
                    "origin_buff",
                    false,
                ),
                Some(BpsrFightSourceKind::Talent | BpsrFightSourceKind::SeasonTalent) => {
                    self.add_index_matches(
                        SelectorIndex::Skill,
                        origin.source_config_id,
                        "origin_talent",
                        false,
                    );
                    self.add_index_matches(
                        SelectorIndex::Effect,
                        origin.source_config_id,
                        "origin_talent_effect",
                        false,
                    );
                }
                Some(BpsrFightSourceKind::Equip | BpsrFightSourceKind::Mod) => {
                    self.add_index_matches(
                        SelectorIndex::Item,
                        origin.source_config_id,
                        "origin_item",
                        false,
                    );
                    self.add_source_config_matches(
                        origin.source_config_id,
                        event.source.map(|source| source.actor_id.0),
                    );
                }
                _ => {}
            }
        }
        if let Some(source) = event.source {
            self.add_actor_context(source.actor_id.0, true);
            self.add_actor_context(event.target.actor_id.0, false);
        } else {
            self.add_actor_context(event.target.actor_id.0, true);
        }
        let direct_obligations = self.direct_candidates_for_event();
        // A status origin is routing evidence, not the lifecycle whose formula
        // inputs are being snapshotted. In particular, child lockout effects
        // may carry the parent buff ID as `origin.source_config_id`. Retain
        // those origin matches for the general obligation report, but trigger
        // formula inputs only for an exact observed effect-ID selector.
        let exact_effect_obligations = self
            .indexes
            .effects
            .get(&event.effect.0)
            .cloned()
            .unwrap_or_default();
        let formula_trigger_obligations = if matches!(
            event.state,
            rlogs_events::StatusState::Applied
                | rlogs_events::StatusState::Refreshed
                | rlogs_events::StatusState::Stacked
        ) {
            direct_obligations
                .iter()
                .copied()
                .filter(|obligation| exact_effect_obligations.contains(obligation))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        self.add_observed_actor_identity(event.source.map(|source| source.actor_id.0));
        self.observe_status_evidence(event, &direct_obligations);
        if dreamscope_effect_id.is_some() {
            self.observe_dreamscope_source_resolution(event, &effect_fingerprint);
            self.observe_dreamscope_status_evidence(event);
        }
        self.observe_formula_input_snapshots(
            session_id,
            sequence,
            observed_micros,
            event.source.map(|source| source.actor_id.0),
            Some(event.target.actor_id.0),
            &formula_trigger_obligations,
        );
        self.observe_pre_trigger_damage(
            sequence,
            observed_micros,
            event.source.map(|source| source.actor_id.0),
            &formula_trigger_obligations,
        );
        self.commit_matches(
            sequence,
            STATUS,
            ValidationEventActors {
                source: event.source.map(|source| source.actor_id.0),
                target: Some(event.target.actor_id.0),
                direct_source: None,
                ..Default::default()
            },
            false,
            Some(status_state_name(event)),
        );
        self.expire_status_windows(event.target.actor_id.0, sequence, observed_micros);
        self.update_status_window(
            sequence,
            observed_micros,
            event,
            direct_obligations,
            dreamscope_effect_id,
        );
        self.observe_status_concurrency(event.target.actor_id.0);
    }

    fn observe_dreamscope_source_resolution(
        &mut self,
        event: &StatusEvent,
        fingerprint: &crate::ResolvedStatusEffectFingerprint<'_>,
    ) {
        let provider_actor_id = event.source.map(|source| source.actor_id.0);
        let exact_factor_items = provider_actor_id
            .and_then(|actor_id| self.exact_factor_items_by_actor.get(&actor_id))
            .map(Vec::as_slice)
            .unwrap_or_default();
        let has_factor_candidates = fingerprint.candidate_sources.iter().any(|candidate| {
            candidate
                .dreamscope_selector
                .as_ref()
                .is_some_and(|selector| {
                    selector.source_kind == EffectDreamscopeSourceKind::FactorFamily
                })
        });

        let selected = (has_factor_candidates && !exact_factor_items.is_empty()).then(|| {
            resolve_dreamscope_effect_owner(
                fingerprint,
                ExactDreamscopeLoadout {
                    factor_item_ids: exact_factor_items,
                    ..Default::default()
                },
            )
        });
        let equipped_variant_resolution = selected
            .as_ref()
            .map(|selected| selected.resolution)
            .unwrap_or_default();
        let exact_route_source = unique_dreamscope_route_source(fingerprint);
        let owner_source = (fingerprint.owner_resolution == EffectFingerprintResolution::Exact
            && fingerprint.candidate_sources.len() == 1)
            .then(|| &fingerprint.candidate_sources[0]);
        let source = selected
            .as_ref()
            .and_then(|selected| {
                (selected.resolution == EffectFingerprintResolution::Exact)
                    .then_some(selected.source)
                    .flatten()
            })
            .or(exact_route_source)
            .or(owner_source);
        let route_resolution = if source.is_some() {
            EffectFingerprintResolution::Exact
        } else if fingerprint.candidate_sources.is_empty() {
            EffectFingerprintResolution::Unresolved
        } else {
            EffectFingerprintResolution::Ambiguous
        };
        let (selected_factor_item_id, selected_factor_grade) = source
            .and_then(|candidate| candidate.dreamscope_selector.as_ref())
            .filter(|selector| {
                selector.source_kind == EffectDreamscopeSourceKind::FactorFamily
                    && equipped_variant_resolution == EffectFingerprintResolution::Exact
            })
            .and_then(|selector| {
                let mut matches = selector
                    .candidate_item_ids
                    .iter()
                    .enumerate()
                    .filter(|(_, item_id)| exact_factor_items.contains(item_id))
                    .map(|(index, &item_id)| {
                        (item_id, selector.candidate_grades.get(index).copied())
                    });
                let first = matches.next();
                (matches.next().is_none()).then_some(first).flatten()
            })
            .map_or((None, None), |(item_id, grade)| (Some(item_id), grade));
        let key = DreamscopeSourceObservationKey {
            provider_actor_id,
            source_type_id: fingerprint.source_type_id,
            source_config_id: fingerprint.source_config_id,
            match_kind: fingerprint.match_kind,
            route_resolution,
            equipped_variant_resolution,
            resolution: route_resolution,
            source_id: source.map(|candidate| candidate.source_id.clone()),
            source_kind: source.map(|candidate| candidate.source_kind.clone()),
            selected_factor_item_id,
            selected_factor_grade,
        };
        let count = self
            .dreamscope_terminal_effects
            .entry(event.effect.0)
            .or_default()
            .source_observations
            .entry(key)
            .or_default();
        *count = count.saturating_add(1);
    }

    fn begin_event(&mut self) {
        self.scratch_candidates.clear();
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == 0 {
            self.candidate_epoch.fill(0);
            self.direct_epoch.fill(0);
            self.contextual_epoch.fill(0);
            self.activation_epoch.fill(0);
            self.factor_selection_epoch.fill(0);
            self.factor_mechanic_epoch.fill(0);
            self.source_selection_epoch.fill(0);
            self.epoch = 1;
        }
    }

    fn add_index_matches(
        &mut self,
        index: SelectorIndex,
        value: i64,
        label: &'static str,
        activates: bool,
    ) {
        let values = match index {
            SelectorIndex::Effect => self.indexes.effects.get(&value),
            SelectorIndex::Skill => self.indexes.skills.get(&value),
            SelectorIndex::Recount => self.indexes.recount.get(&value),
            SelectorIndex::Attribute => self.indexes.attributes.get(&value),
            SelectorIndex::Class => self.indexes.classes.get(&value),
            SelectorIndex::Specialization => self.indexes.specializations.get(&value),
            SelectorIndex::Item => self.indexes.items.get(&value),
        };
        let Some(values) = values else { return };
        let values = values.clone();
        for obligation in values {
            self.mark_candidate(obligation, true, activates);
            if self.definitions[obligation].domain == "psychoscope-factor" {
                self.factor_mechanic_epoch[obligation] = self.epoch;
            }
            self.states[obligation]
                .matched_identifiers
                .insert(format!("{label}:{value}"));
        }
    }

    fn add_equipment_suit_match(&mut self, map_key: i32, attribute_key: i32) {
        let Some(values) = self
            .indexes
            .equipment_suits
            .get(&(map_key, attribute_key))
            .cloned()
        else {
            return;
        };
        for obligation in values {
            self.mark_candidate(obligation, true, true);
            self.source_selection_epoch[obligation] = self.epoch;
            self.states[obligation]
                .matched_identifiers
                .insert(format!("equipment_suit:{map_key}:{attribute_key}"));
        }
    }

    fn add_source_config_matches(&mut self, source_config_id: i64, provider_actor_id: Option<u64>) {
        let Some(values) = self.indexes.source_configs.get(&source_config_id).cloned() else {
            return;
        };
        for obligation in values {
            self.mark_candidate(obligation, true, true);
            self.source_selection_epoch[obligation] = self.epoch;
            if let Some(provider_actor_id) = provider_actor_id {
                self.source_origin_providers
                    .entry(obligation)
                    .or_default()
                    .insert(provider_actor_id);
            }
            self.states[obligation]
                .matched_identifiers
                .insert(format!("source_config:{source_config_id}"));
        }
    }

    fn add_actor_context(&mut self, actor_id: u64, include_factor_selection: bool) {
        let mut actor_context = BTreeSet::new();
        for lane in [
            &self.actor_active,
            &self.actor_selection_active,
            &self.entity_attribute_active,
            &self.temporary_attribute_active,
        ] {
            if let Some(active) = lane.get(&actor_id) {
                actor_context.extend(active.iter().copied());
            }
        }
        for obligation in actor_context {
            self.mark_candidate(obligation, false, false);
        }
        if include_factor_selection {
            let selected_factors = self
                .factor_active_by_actor
                .get(&actor_id)
                .map(|active| active.iter().copied().collect::<Vec<_>>())
                .unwrap_or_default();
            for obligation in selected_factors {
                self.mark_candidate(obligation, false, false);
                self.factor_selection_epoch[obligation] = self.epoch;
            }
        }
        let selected_sources = self
            .source_origin_providers
            .iter()
            .filter_map(|(&obligation, providers)| {
                providers.contains(&actor_id).then_some(obligation)
            })
            .collect::<Vec<_>>();
        for obligation in selected_sources {
            self.mark_candidate(obligation, false, false);
            self.source_selection_epoch[obligation] = self.epoch;
        }
        let status_active = self
            .status_active_counts
            .get(&actor_id)
            .map(|active| active.keys().copied().collect::<Vec<_>>())
            .unwrap_or_default();
        for obligation in status_active {
            self.mark_candidate(obligation, false, false);
            if self.definitions[obligation].has_source_config_selectors {
                self.source_selection_epoch[obligation] = self.epoch;
            }
        }
    }

    fn add_observed_actor_identity(&mut self, actor_id: Option<u64>) {
        let Some(actor_id) = actor_id else { return };
        let Some(&actor_sequence) = self.observed_actor_sequences.get(&actor_id) else {
            return;
        };
        let candidates = self.scratch_candidates.clone();
        for obligation in candidates {
            if self.definitions[obligation].required_mask & ACTOR == 0 {
                continue;
            }
            let state = &mut self.states[obligation];
            if state.observed_mask & ACTOR == 0 {
                state.observed_mask |= ACTOR;
                state.first_sequence = Some(
                    state
                        .first_sequence
                        .map_or(actor_sequence, |sequence| sequence.min(actor_sequence)),
                );
                state.last_sequence = Some(
                    state
                        .last_sequence
                        .map_or(actor_sequence, |sequence| sequence.max(actor_sequence)),
                );
                state.contextual_matches = state.contextual_matches.saturating_add(1);
            }
            state.selected_actor_ids.insert(actor_id);
            state
                .matched_identifiers
                .insert(format!("actor_identity:{actor_id}"));
        }
    }

    fn add_damage_packet_matches(&mut self, ability_id: i64, hit_event_id: i32) {
        let Some(values) = self
            .indexes
            .damage_packet
            .get(&(ability_id, hit_event_id))
            .cloned()
        else {
            return;
        };
        for obligation in values {
            self.mark_candidate(obligation, true, false);
            self.states[obligation]
                .matched_identifiers
                .insert(format!("damage_packet:{ability_id}:{hit_event_id}"));
        }
    }

    fn mark_candidate(&mut self, obligation: usize, direct: bool, activates: bool) {
        if self.candidate_epoch[obligation] != self.epoch {
            self.candidate_epoch[obligation] = self.epoch;
            self.scratch_candidates.push(obligation);
        }
        if direct {
            self.direct_epoch[obligation] = self.epoch;
        } else {
            self.contextual_epoch[obligation] = self.epoch;
        }
        if activates {
            self.activation_epoch[obligation] = self.epoch;
        }
    }

    fn direct_candidates(&self) -> Vec<usize> {
        self.scratch_candidates
            .iter()
            .copied()
            .filter(|&obligation| self.direct_epoch[obligation] == self.epoch)
            .collect()
    }

    fn direct_candidates_for_event(&self) -> Vec<usize> {
        self.direct_candidates()
            .into_iter()
            .filter(|&obligation| {
                let definition = &self.definitions[obligation];
                (definition.domain != "psychoscope-factor"
                    || (self.factor_selection_epoch[obligation] == self.epoch
                        && self.factor_mechanic_epoch[obligation] == self.epoch))
                    && (!definition.has_source_config_selectors
                        || self.source_selection_epoch[obligation] == self.epoch)
            })
            .collect()
    }

    fn replace_actor_selection_context(&mut self, actor_id: u64) {
        let Some(selection) = self.actor_selection_state.get(&actor_id) else {
            return;
        };
        let class_id = selection.class_id;
        let specialization_id = selection.specialization_id;
        let weapon_item_id = selection.weapon_item_id;
        let slots = selection.selected_slots().cloned().collect::<Vec<_>>();

        let mut item_matches = BTreeSet::new();
        let mut skill_matches = BTreeSet::new();
        if let Some(item_id) = weapon_item_id {
            if let Some(obligations) = self.indexes.items.get(&item_id) {
                item_matches.extend(obligations.iter().copied());
            }
        }
        for slot in &slots {
            if let Some(item_id) = slot.item_id
                && let Some(obligations) = self.indexes.items.get(&item_id)
            {
                item_matches.extend(obligations.iter().copied());
            }
            if let Some(ability_id) = slot.ability_id
                && let Some(obligations) = self.indexes.skills.get(&ability_id)
            {
                skill_matches.extend(obligations.iter().copied());
            }
        }

        let mut candidates = BTreeSet::new();
        if let Some(class_id) = class_id
            && let Some(obligations) = self.indexes.classes.get(&i64::from(class_id))
        {
            candidates.extend(obligations.iter().copied());
        }
        if let Some(specialization_id) = specialization_id
            && let Some(obligations) = self
                .indexes
                .specializations
                .get(&i64::from(specialization_id))
        {
            candidates.extend(obligations.iter().copied());
        }
        candidates.extend(item_matches.iter().copied());
        candidates.extend(skill_matches.iter().copied());

        let selected = candidates
            .into_iter()
            .filter(|&obligation| {
                let definition = &self.definitions[obligation];
                if definition.has_item_selectors {
                    return item_matches.contains(&obligation);
                }
                if !definition.specialization_selectors.is_empty() {
                    return specialization_id.is_some_and(|value| {
                        definition
                            .specialization_selectors
                            .contains(&i64::from(value))
                    });
                }
                if !definition.class_selectors.is_empty() {
                    return class_id.is_some_and(|value| {
                        definition.class_selectors.contains(&i64::from(value))
                    });
                }
                skill_matches.contains(&obligation)
            })
            .collect::<BTreeSet<_>>();
        for &obligation in &selected {
            self.states[obligation].selected_actor_ids.insert(actor_id);
        }
        self.actor_selection_active.insert(actor_id, selected);
    }

    fn update_attribute_actor_context(
        &mut self,
        actor_id: u64,
        temporary: bool,
        update_kind: EntityAttributeUpdateKind,
        direct: &[usize],
    ) {
        let actor_identity = self
            .actor_selection_active
            .get(&actor_id)
            .or_else(|| self.actor_active.get(&actor_id));
        let selected = direct
            .iter()
            .copied()
            .filter(|&obligation| {
                let definition = &self.definitions[obligation];
                (definition.class_selectors.is_empty()
                    && definition.specialization_selectors.is_empty())
                    || actor_identity.is_some_and(|active| active.contains(&obligation))
            })
            .collect::<BTreeSet<_>>();
        for &obligation in &selected {
            self.states[obligation].selected_actor_ids.insert(actor_id);
        }
        let lane = if temporary {
            &mut self.temporary_attribute_active
        } else {
            &mut self.entity_attribute_active
        };
        match update_kind {
            EntityAttributeUpdateKind::Snapshot => {
                lane.insert(actor_id, selected);
            }
            // Delta and legacy Unknown packets are sparse. They may add or
            // update selected attributes, but absence never means removal.
            EntityAttributeUpdateKind::Delta | EntityAttributeUpdateKind::Unknown => {
                lane.entry(actor_id).or_default().extend(selected);
            }
        }
    }

    fn clear_attribute_values(&mut self, actor_id: u64, channel: u8) {
        self.last_attribute_values
            .retain(|(known_actor, known_channel, _), _| {
                *known_actor != actor_id || *known_channel != channel
            });
    }

    fn observe_status_evidence(&mut self, event: &StatusEvent, obligations: &[usize]) {
        let provider = event.source.map(|source| source.actor_id.0);
        let recipient = event.target.actor_id.0;
        for &obligation in obligations {
            let state = &mut self.states[obligation];
            *state
                .provider_recipient_observations
                .entry((provider, recipient, event.effect.0))
                .or_default() += 1;
            if let Some(origin) = event.origin {
                *state
                    .status_origin_observations
                    .entry((
                        provider,
                        recipient,
                        event.effect.0,
                        origin.source_type_id,
                        origin.source_config_id,
                    ))
                    .or_default() += 1;
            }
            if let Some(instance_id) = event.instance_id {
                state.status_instance_ids.insert(instance_id.0);
            }
            if let Some(stacks) = event.stacks {
                state.minimum_stacks =
                    Some(state.minimum_stacks.map_or(stacks, |old| old.min(stacks)));
                state.maximum_stacks =
                    Some(state.maximum_stacks.map_or(stacks, |old| old.max(stacks)));
            }
        }
    }

    fn observe_dreamscope_status_evidence(&mut self, event: &StatusEvent) {
        let provider = event.source.map(|source| source.actor_id.0);
        let recipient = event.target.actor_id.0;
        let state = self
            .dreamscope_terminal_effects
            .entry(event.effect.0)
            .or_default();
        *state
            .status_states
            .entry(status_state_name(event).to_owned())
            .or_default() += 1;
        *state
            .provider_recipient_observations
            .entry((provider, recipient))
            .or_default() += 1;
        if let Some(instance_id) = event.instance_id {
            state.status_instance_ids.insert(instance_id.0);
        }
        *state
            .packet_levels
            .entry(optional_packet_value(event.level))
            .or_default() += 1;
        *state
            .packet_part_ids
            .entry(optional_packet_value(event.part_id))
            .or_default() += 1;
        *state
            .packet_counts
            .entry(optional_packet_value(event.count))
            .or_default() += 1;
        *state
            .packet_durations_millis
            .entry(optional_packet_value(event.duration_millis))
            .or_default() += 1;
        if let Some(stacks) = event.stacks {
            state.minimum_stacks = Some(state.minimum_stacks.map_or(stacks, |old| old.min(stacks)));
            state.maximum_stacks = Some(state.maximum_stacks.map_or(stacks, |old| old.max(stacks)));
        }
    }

    fn observe_status_concurrency(&mut self, recipient_actor: u64) {
        if let Some(active) = self.status_active_counts.get(&recipient_actor) {
            let observations = active
                .iter()
                .map(|(&obligation, &instances)| {
                    let providers = self
                        .status_providers
                        .get(&(recipient_actor, obligation))
                        .map_or(0, |providers| providers.len());
                    (obligation, instances, providers)
                })
                .collect::<Vec<_>>();
            for (obligation, instances, providers) in observations {
                let state = &mut self.states[obligation];
                state.maximum_concurrent_instances =
                    state.maximum_concurrent_instances.max(instances);
                state.maximum_concurrent_providers = state
                    .maximum_concurrent_providers
                    .max(u32::try_from(providers).unwrap_or(u32::MAX));
            }
        }
        if let Some(active) = self.dreamscope_active_counts.get(&recipient_actor) {
            let observations = active
                .iter()
                .map(|(&effect_id, &instances)| {
                    let providers = self
                        .dreamscope_providers
                        .get(&(recipient_actor, effect_id))
                        .map_or(0, |providers| providers.len());
                    (effect_id, instances, providers)
                })
                .collect::<Vec<_>>();
            for (effect_id, instances, providers) in observations {
                let state = self
                    .dreamscope_terminal_effects
                    .entry(effect_id)
                    .or_default();
                state.maximum_concurrent_instances =
                    state.maximum_concurrent_instances.max(instances);
                state.maximum_concurrent_providers = state
                    .maximum_concurrent_providers
                    .max(u32::try_from(providers).unwrap_or(u32::MAX));
            }
        }
    }

    fn observe_damage_evidence(
        &mut self,
        sequence: u64,
        observed_micros: u64,
        event: &rlogs_events::DamageEvent,
    ) {
        let amount = i128::from(event.amount.max(0));
        let direct = self.direct_candidates_for_event();
        for obligation in direct {
            let state = &mut self.states[obligation];
            state.direct_damage_events = state.direct_damage_events.saturating_add(1);
            state.direct_damage = state.direct_damage.saturating_add(amount);
            if !self.definitions[obligation].formula_inputs.is_empty() {
                record_packet_damage_row(state, event, 0, sequence, observed_micros);
            }
        }
        self.observe_window_damage(
            sequence,
            observed_micros,
            event.source.actor_id.0,
            event,
            amount,
            true,
        );
        self.observe_window_damage(
            sequence,
            observed_micros,
            event.target.actor_id.0,
            event,
            amount,
            false,
        );
        self.remember_recent_damage(sequence, observed_micros, event);
    }

    fn observe_window_damage(
        &mut self,
        sequence: u64,
        observed_micros: u64,
        actor_id: u64,
        event: &rlogs_events::DamageEvent,
        amount: i128,
        outgoing: bool,
    ) {
        self.expire_status_windows(actor_id, sequence, observed_micros);
        if let Some(active) = self.status_active_counts.get(&actor_id) {
            let obligations = active.keys().copied().collect::<Vec<_>>();
            for obligation in obligations {
                if !self.obligation_window_membership_is_exact(
                    actor_id,
                    obligation,
                    sequence,
                    observed_micros,
                ) {
                    let state = &mut self.states[obligation];
                    if outgoing {
                        state.unresolved_recipient_window_damage_events = state
                            .unresolved_recipient_window_damage_events
                            .saturating_add(1);
                    } else {
                        state.unresolved_target_window_damage_events = state
                            .unresolved_target_window_damage_events
                            .saturating_add(1);
                    }
                    continue;
                }
                let stack_windows = self.stack_windows_for_obligation(actor_id, obligation);
                let provider_counts = self.status_providers.get(&(actor_id, obligation));
                let external_providers = provider_counts
                    .into_iter()
                    .flat_map(|providers| providers.iter())
                    .filter(|(provider, count)| {
                        **count > 0 && provider.is_some_and(|provider| provider != actor_id)
                    })
                    .count();
                let state = &mut self.states[obligation];
                record_stack_at_damage(
                    &mut state.stack_at_damage,
                    if outgoing { 1 } else { 2 },
                    stack_windows,
                    amount,
                );
                if !self.definitions[obligation].formula_inputs.is_empty() {
                    record_packet_damage_row(
                        state,
                        event,
                        if outgoing { 1 } else { 2 },
                        sequence,
                        observed_micros,
                    );
                }
                if outgoing {
                    state.recipient_window_damage_events =
                        state.recipient_window_damage_events.saturating_add(1);
                    state.recipient_window_damage =
                        state.recipient_window_damage.saturating_add(amount);
                    if external_providers == 1 {
                        state.single_provider_window_damage_events =
                            state.single_provider_window_damage_events.saturating_add(1);
                        state.single_provider_window_damage =
                            state.single_provider_window_damage.saturating_add(amount);
                    } else if external_providers > 1 {
                        state.ambiguous_provider_window_damage_events = state
                            .ambiguous_provider_window_damage_events
                            .saturating_add(1);
                    }
                } else {
                    state.target_window_damage_events =
                        state.target_window_damage_events.saturating_add(1);
                    state.target_window_damage = state.target_window_damage.saturating_add(amount);
                }
            }
        }

        if let Some(active) = self.dreamscope_active_counts.get(&actor_id) {
            let effect_ids = active.keys().copied().collect::<Vec<_>>();
            for effect_id in effect_ids {
                if !self.dreamscope_window_membership_is_exact(
                    actor_id,
                    effect_id,
                    sequence,
                    observed_micros,
                ) {
                    let state = self
                        .dreamscope_terminal_effects
                        .entry(effect_id)
                        .or_default();
                    if outgoing {
                        state.unresolved_recipient_window_damage_events = state
                            .unresolved_recipient_window_damage_events
                            .saturating_add(1);
                    } else {
                        state.unresolved_target_window_damage_events = state
                            .unresolved_target_window_damage_events
                            .saturating_add(1);
                    }
                    continue;
                }
                let stack_windows = self.stack_windows_for_dreamscope(actor_id, effect_id);
                let external_providers = self
                    .dreamscope_providers
                    .get(&(actor_id, effect_id))
                    .into_iter()
                    .flat_map(|providers| providers.iter())
                    .filter(|(provider, count)| {
                        **count > 0 && provider.is_some_and(|provider| provider != actor_id)
                    })
                    .count();
                let state = self
                    .dreamscope_terminal_effects
                    .entry(effect_id)
                    .or_default();
                record_stack_at_damage(
                    &mut state.stack_at_damage,
                    if outgoing { 1 } else { 2 },
                    stack_windows,
                    amount,
                );
                if outgoing {
                    state.recipient_window_damage_events =
                        state.recipient_window_damage_events.saturating_add(1);
                    state.recipient_window_damage =
                        state.recipient_window_damage.saturating_add(amount);
                    if external_providers > 0 {
                        state.external_provider_window_damage_events = state
                            .external_provider_window_damage_events
                            .saturating_add(1);
                        state.external_provider_window_damage =
                            state.external_provider_window_damage.saturating_add(amount);
                    }
                    if external_providers == 1 {
                        state.single_provider_window_damage_events =
                            state.single_provider_window_damage_events.saturating_add(1);
                        state.single_provider_window_damage =
                            state.single_provider_window_damage.saturating_add(amount);
                    } else if external_providers > 1 {
                        state.ambiguous_provider_window_damage_events = state
                            .ambiguous_provider_window_damage_events
                            .saturating_add(1);
                    }
                } else {
                    state.target_window_damage_events =
                        state.target_window_damage_events.saturating_add(1);
                    state.target_window_damage = state.target_window_damage.saturating_add(amount);
                }
            }
        }
    }

    fn obligation_window_membership_is_exact(
        &self,
        actor_id: u64,
        obligation: usize,
        sequence: u64,
        observed_micros: u64,
    ) -> bool {
        let mut saw_window = false;
        for (key, window) in &self.status_windows {
            if key.target_actor != actor_id || !window.obligations.contains(&obligation) {
                continue;
            }
            saw_window = true;
            if validation_status_window_membership(window, sequence, observed_micros)
                != ValidationStatusWindowMembership::Proven
            {
                return false;
            }
        }
        saw_window
    }

    fn dreamscope_window_membership_is_exact(
        &self,
        actor_id: u64,
        effect_id: i64,
        sequence: u64,
        observed_micros: u64,
    ) -> bool {
        let mut saw_window = false;
        for (key, window) in &self.status_windows {
            if key.target_actor != actor_id || window.dreamscope_effect_id != Some(effect_id) {
                continue;
            }
            saw_window = true;
            if validation_status_window_membership(window, sequence, observed_micros)
                != ValidationStatusWindowMembership::Proven
            {
                return false;
            }
        }
        saw_window
    }

    fn expire_status_windows(&mut self, actor_id: u64, sequence: u64, observed_micros: u64) {
        let expired = self
            .status_windows
            .iter()
            .filter(|(key, window)| {
                key.target_actor == actor_id
                    && validation_status_window_membership(window, sequence, observed_micros)
                        == ValidationStatusWindowMembership::Expired
            })
            .map(|(key, window)| {
                (
                    *key,
                    window.obligations.clone(),
                    window.dreamscope_effect_id,
                )
            })
            .collect::<Vec<_>>();
        for (key, obligations, dreamscope_effect_id) in expired {
            for obligation in obligations {
                self.states[obligation].expired_status_windows = self.states[obligation]
                    .expired_status_windows
                    .saturating_add(1);
            }
            if let Some(effect_id) = dreamscope_effect_id {
                let state = self
                    .dreamscope_terminal_effects
                    .entry(effect_id)
                    .or_default();
                state.expired_status_windows = state.expired_status_windows.saturating_add(1);
            }
            self.remove_status_window(key);
        }
    }

    fn stack_windows_for_obligation(
        &self,
        actor_id: u64,
        obligation: usize,
    ) -> Vec<StatusWindowStackKey> {
        let mut windows = self
            .status_windows
            .iter()
            .filter(|(key, window)| {
                key.target_actor == actor_id && window.obligations.contains(&obligation)
            })
            .map(|(key, window)| StatusWindowStackKey {
                effect_id: key.effect_id,
                instance_id: key.instance_id,
                provider_actor: window.provider_actor,
                stacks: window.current_stacks,
            })
            .collect::<Vec<_>>();
        windows.sort();
        windows
    }

    fn stack_windows_for_dreamscope(
        &self,
        actor_id: u64,
        effect_id: i64,
    ) -> Vec<StatusWindowStackKey> {
        let mut windows = self
            .status_windows
            .iter()
            .filter(|(key, window)| {
                key.target_actor == actor_id && window.dreamscope_effect_id == Some(effect_id)
            })
            .map(|(key, window)| StatusWindowStackKey {
                effect_id: key.effect_id,
                instance_id: key.instance_id,
                provider_actor: window.provider_actor,
                stacks: window.current_stacks,
            })
            .collect::<Vec<_>>();
        windows.sort();
        windows
    }

    fn remember_recent_damage(
        &mut self,
        sequence: u64,
        observed_micros: u64,
        event: &rlogs_events::DamageEvent,
    ) {
        const PRE_TRIGGER_WINDOW_MICROS: u64 = 250_000;
        const MAX_RECENT_DAMAGE_EVENTS: usize = 256;
        while self.recent_damage_events.front().is_some_and(|recent| {
            observed_micros.saturating_sub(recent.observed_micros) > PRE_TRIGGER_WINDOW_MICROS
        }) {
            self.recent_damage_events.pop_front();
        }
        self.recent_damage_events.push_back(RecentDamageEvent {
            sequence,
            observed_micros,
            event: event.clone(),
        });
        while self.recent_damage_events.len() > MAX_RECENT_DAMAGE_EVENTS {
            self.recent_damage_events.pop_front();
        }
    }

    fn observe_pre_trigger_damage(
        &mut self,
        trigger_sequence: u64,
        trigger_micros: u64,
        source_actor: Option<u64>,
        obligations: &[usize],
    ) {
        const PRE_TRIGGER_WINDOW_MICROS: u64 = 250_000;
        let Some(source_actor) = source_actor else {
            return;
        };
        let relevant_obligations = obligations
            .iter()
            .copied()
            .filter(|&obligation| !self.definitions[obligation].formula_inputs.is_empty())
            .collect::<Vec<_>>();
        if relevant_obligations.is_empty() {
            return;
        }
        let recent = self
            .recent_damage_events
            .iter()
            .filter(|recent| {
                recent.sequence < trigger_sequence
                    && trigger_micros.saturating_sub(recent.observed_micros)
                        <= PRE_TRIGGER_WINDOW_MICROS
                    && (recent.event.source.actor_id.0 == source_actor
                        || recent
                            .event
                            .direct_source
                            .is_some_and(|direct| direct.actor_id.0 == source_actor))
            })
            .cloned()
            .collect::<Vec<_>>();
        for obligation in relevant_obligations {
            if !recent.is_empty() {
                self.states[obligation].observed_mask |= DAMAGE;
                self.states[obligation]
                    .matched_identifiers
                    .insert("damage:pre-trigger-buffer".into());
            }
            for recent in &recent {
                record_packet_damage_row(
                    &mut self.states[obligation],
                    &recent.event,
                    3,
                    recent.sequence,
                    recent.observed_micros,
                );
            }
        }
    }

    fn observe_attribute_value(
        &mut self,
        sequence: u64,
        actor_id: u64,
        lane: u8,
        id: i64,
        value: i64,
    ) {
        let selector_obligations = self.indexes.attributes.get(&id).cloned();
        let is_formula_input = self.indexes.formula_input_attributes.contains_key(&id);
        if selector_obligations.is_none() && !is_formula_input {
            return;
        }
        let label = if lane == 0 {
            format!("entity:{id}")
        } else {
            format!("temporary:{id}")
        };
        let previous = self
            .last_attribute_values
            .insert((actor_id, lane, id), LastAttributeValue { value, sequence });
        for obligation in selector_obligations.unwrap_or_default() {
            self.states[obligation]
                .attribute_values
                .entry(label.clone())
                .or_default()
                .insert(value);
            if previous.is_some_and(|previous| previous.value != value) {
                *self.states[obligation]
                    .attribute_transition_counts
                    .entry(label.clone())
                    .or_default() += 1;
            }
        }
    }

    fn observe_formula_input_snapshots(
        &mut self,
        session_id: &str,
        trigger_sequence: u64,
        trigger_observed_micros: u64,
        source_actor_id: Option<u64>,
        target_actor_id: Option<u64>,
        obligations: &[usize],
    ) {
        for &obligation in obligations {
            let inputs = self.definitions[obligation].formula_inputs.clone();
            if inputs.is_empty() {
                continue;
            }
            let mut all_complete = true;
            let snapshots = inputs
                .into_iter()
                .map(|input| {
                    let actor_id = match input.actor_role.as_str() {
                        "source" => source_actor_id,
                        "target" => target_actor_id,
                        _ => None,
                    };
                    let mut values = Vec::new();
                    let mut loadout_values = Vec::new();
                    let selection =
                        actor_id.and_then(|actor_id| self.actor_selection_state.get(&actor_id));
                    let (class_route_state, class_id, class_observation, attribute_ids) =
                        if input.input_kind == "class_attribute" {
                            let (state, class_id, observation, attribute_ids) =
                                class_attribute_formula_input(selection, &input);
                            (Some(state), class_id, observation, attribute_ids)
                        } else {
                            (None, None, None, input.candidate_attribute_ids.clone())
                        };
                    if matches!(input.input_kind.as_str(), "attribute" | "class_attribute")
                        && let Some(actor_id) = actor_id
                    {
                        for attribute_id in &attribute_ids {
                            for (lane, lane_name) in [(0, "entity"), (1, "temporary")] {
                                if let Some(observed) =
                                    self.last_attribute_values
                                        .get(&(actor_id, lane, *attribute_id))
                                {
                                    values.push(RdpsValidationFormulaInputValue {
                                        lane: lane_name.into(),
                                        attribute_id: attribute_id.to_string(),
                                        value: observed.value.to_string(),
                                        attribute_sequence: observed.sequence,
                                    });
                                }
                            }
                        }
                    }
                    let state = if actor_id.is_none() {
                        all_complete = false;
                        match input.actor_role.as_str() {
                            "target" => "missing-target-actor",
                            _ => "missing-source-actor",
                        }
                    } else if input.input_kind == "loadout_tier" {
                        let result = loadout_formula_input(selection, &input);
                        loadout_values = result.1;
                        if result.0 != "complete" {
                            all_complete = false;
                        }
                        result.0
                    } else if let Some(class_route_state) = class_route_state
                        && class_route_state != "route-selected"
                    {
                        all_complete = false;
                        class_route_state
                    } else if values.is_empty() {
                        all_complete = false;
                        "missing-current-value"
                    } else {
                        "complete"
                    };
                    RdpsValidationFormulaInputSnapshot {
                        session_id: session_id.to_owned(),
                        trigger_sequence,
                        trigger_observed_micros,
                        actor_role: input.actor_role,
                        actor_id: actor_id.map(|value| value.to_string()),
                        class_id,
                        class_observation_sequence: class_observation.map(|point| point.sequence),
                        class_observation_observed_micros: class_observation
                            .map(|point| point.observed_micros),
                        input_key: input.input_key,
                        label: input.label,
                        state: state.into(),
                        values,
                        loadout_values,
                    }
                })
                .collect::<Vec<_>>();
            self.states[obligation]
                .formula_input_snapshots
                .extend(snapshots);
            if all_complete {
                self.states[obligation].observed_mask |= FORMULA_INPUTS;
            }
        }
    }

    fn update_status_window(
        &mut self,
        sequence: u64,
        observed_micros: u64,
        event: &StatusEvent,
        obligations: Vec<usize>,
        dreamscope_effect_id: Option<i64>,
    ) {
        use rlogs_events::StatusState;

        let key = StatusWindowKey {
            target_actor: event.target.actor_id.0,
            effect_id: event.effect.0,
            instance_id: event.instance_id.map(|instance| instance.0),
            provider_discriminator: event
                .instance_id
                .is_none()
                .then(|| event.source.map(|source| source.actor_id.0))
                .flatten(),
        };
        match event.state {
            StatusState::Applied | StatusState::Refreshed | StatusState::Stacked => {
                if obligations.is_empty() && dreamscope_effect_id.is_none() {
                    return;
                }
                self.remove_status_window(key);
                for &obligation in &obligations {
                    let count = self
                        .status_active_counts
                        .entry(key.target_actor)
                        .or_default()
                        .entry(obligation)
                        .or_default();
                    *count = count.saturating_add(1);
                }
                let provider_actor = event.source.map(|source| source.actor_id.0);
                for &obligation in &obligations {
                    let count = self
                        .status_providers
                        .entry((key.target_actor, obligation))
                        .or_default()
                        .entry(provider_actor)
                        .or_default();
                    *count = count.saturating_add(1);
                }
                if let Some(effect_id) = dreamscope_effect_id {
                    let active_count = self
                        .dreamscope_active_counts
                        .entry(key.target_actor)
                        .or_default()
                        .entry(effect_id)
                        .or_default();
                    *active_count = active_count.saturating_add(1);
                    let provider_count = self
                        .dreamscope_providers
                        .entry((key.target_actor, effect_id))
                        .or_default()
                        .entry(provider_actor)
                        .or_default();
                    *provider_count = provider_count.saturating_add(1);
                    if event.duration_millis.is_none() {
                        let state = self
                            .dreamscope_terminal_effects
                            .entry(effect_id)
                            .or_default();
                        state.open_unbounded_status_windows =
                            state.open_unbounded_status_windows.saturating_add(1);
                    }
                }
                self.status_windows.insert(
                    key,
                    ActiveValidationStatusWindow {
                        obligations,
                        dreamscope_effect_id,
                        provider_actor,
                        opened_sequence: sequence,
                        opened_observed_micros: observed_micros,
                        duration_millis: event.duration_millis,
                        current_stacks: event.stacks,
                    },
                );
            }
            StatusState::Removed => self.remove_status_window_fail_closed(key),
            StatusState::Consumed if event.stacks.unwrap_or(0) == 0 => {
                self.remove_status_window_fail_closed(key)
            }
            StatusState::Consumed => {
                if let Some(window) = self.status_windows.get_mut(&key) {
                    window.current_stacks = event.stacks;
                }
            }
        }
    }

    fn remove_status_window(&mut self, key: StatusWindowKey) {
        let Some(window) = self.status_windows.remove(&key) else {
            return;
        };
        let ActiveValidationStatusWindow {
            obligations,
            dreamscope_effect_id,
            provider_actor,
            duration_millis,
            ..
        } = window;
        if let Some(effect_id) = dreamscope_effect_id
            && duration_millis.is_none()
        {
            let state = self
                .dreamscope_terminal_effects
                .entry(effect_id)
                .or_default();
            state.open_unbounded_status_windows =
                state.open_unbounded_status_windows.saturating_sub(1);
        }
        let mut remove_actor = false;
        if let Some(active) = self.status_active_counts.get_mut(&key.target_actor) {
            for obligation in obligations {
                if let Some(count) = active.get_mut(&obligation) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        active.remove(&obligation);
                    }
                }
                let provider_key = (key.target_actor, obligation);
                let mut remove_providers = false;
                if let Some(providers) = self.status_providers.get_mut(&provider_key) {
                    if let Some(count) = providers.get_mut(&provider_actor) {
                        *count = count.saturating_sub(1);
                        if *count == 0 {
                            providers.remove(&provider_actor);
                        }
                    }
                    remove_providers = providers.is_empty();
                }
                if remove_providers {
                    self.status_providers.remove(&provider_key);
                }
            }
            remove_actor = active.is_empty();
        }
        if remove_actor {
            self.status_active_counts.remove(&key.target_actor);
        }
        if let Some(effect_id) = dreamscope_effect_id {
            let mut remove_dreamscope_actor = false;
            if let Some(active) = self.dreamscope_active_counts.get_mut(&key.target_actor) {
                if let Some(count) = active.get_mut(&effect_id) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        active.remove(&effect_id);
                    }
                }
                remove_dreamscope_actor = active.is_empty();
            }
            if remove_dreamscope_actor {
                self.dreamscope_active_counts.remove(&key.target_actor);
            }
            let provider_key = (key.target_actor, effect_id);
            let mut remove_providers = false;
            if let Some(providers) = self.dreamscope_providers.get_mut(&provider_key) {
                if let Some(count) = providers.get_mut(&provider_actor) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        providers.remove(&provider_actor);
                    }
                }
                remove_providers = providers.is_empty();
            }
            if remove_providers {
                self.dreamscope_providers.remove(&provider_key);
            }
        }
    }

    /// A removal without either an instance or provider cannot be assigned to
    /// one concurrent window. End every matching candidate window so later
    /// damage is never credited through stale state, and retain the ambiguity
    /// in each affected obligation's evidence report.
    fn remove_status_window_fail_closed(&mut self, key: StatusWindowKey) {
        if self.status_windows.contains_key(&key) {
            self.remove_status_window(key);
            return;
        }
        if key.instance_id.is_some() || key.provider_discriminator.is_some() {
            return;
        }
        let matching = self
            .status_windows
            .iter()
            .filter(|(candidate, _)| {
                candidate.target_actor == key.target_actor && candidate.effect_id == key.effect_id
            })
            .map(|(candidate, window)| {
                (
                    *candidate,
                    window.obligations.clone(),
                    window.dreamscope_effect_id,
                )
            })
            .collect::<Vec<_>>();
        if matching.is_empty() {
            return;
        }
        let mut obligations = BTreeSet::new();
        let mut dreamscope_effects = BTreeSet::new();
        for (candidate, candidate_obligations, dreamscope_effect_id) in matching {
            obligations.extend(candidate_obligations);
            dreamscope_effects.extend(dreamscope_effect_id);
            self.remove_status_window(candidate);
        }
        for obligation in obligations {
            self.states[obligation].ambiguous_status_removals = self.states[obligation]
                .ambiguous_status_removals
                .saturating_add(1);
        }
        for effect_id in dreamscope_effects {
            let state = self
                .dreamscope_terminal_effects
                .entry(effect_id)
                .or_default();
            state.ambiguous_status_removals = state.ambiguous_status_removals.saturating_add(1);
        }
    }

    fn commit_matches(
        &mut self,
        sequence: u64,
        event_mask: u16,
        actors: ValidationEventActors,
        activate_direct: bool,
        status_state: Option<&'static str>,
    ) {
        let mut matched_any = false;
        for candidate_index in 0..self.scratch_candidates.len() {
            let obligation = self.scratch_candidates[candidate_index];
            if self.definitions[obligation].required_mask & event_mask == 0 {
                continue;
            }
            if self.definitions[obligation].domain == "mastery-property"
                && !self.mastery_route_matches(obligation, event_mask, actors)
            {
                continue;
            }
            if self.definitions[obligation].domain == "target-mitigation"
                && event_mask == DAMAGE
                && !actors.target.is_some_and(|target| {
                    self.entity_attribute_active
                        .get(&target)
                        .is_some_and(|active| active.contains(&obligation))
                        && self
                            .temporary_attribute_active
                            .get(&target)
                            .is_some_and(|active| active.contains(&obligation))
                })
            {
                continue;
            }
            if self.definitions[obligation].domain == "psychoscope-factor"
                && event_mask != PROFILE_SELECTION
                && (self.factor_selection_epoch[obligation] != self.epoch
                    || self.factor_mechanic_epoch[obligation] != self.epoch)
            {
                continue;
            }
            if self.definitions[obligation].has_source_config_selectors
                && self.source_selection_epoch[obligation] != self.epoch
            {
                continue;
            }
            matched_any = true;
            let direct = self.direct_epoch[obligation] == self.epoch;
            let was_contextual = !direct && self.contextual_epoch[obligation] == self.epoch;
            let state = &mut self.states[obligation];
            state.observed_mask |= event_mask;
            state.first_sequence.get_or_insert(sequence);
            state.last_sequence = Some(sequence);
            if was_contextual {
                state.contextual_matches = state.contextual_matches.saturating_add(1);
            } else {
                state.direct_matches = state.direct_matches.saturating_add(1);
            }
            if let Some(status_state) = status_state {
                *state.status_states.entry(status_state.into()).or_default() += 1;
            }
            if activate_direct && direct && self.activation_epoch[obligation] == self.epoch {
                if let Some(actor_id) = actors.source {
                    self.actor_active
                        .entry(actor_id)
                        .or_default()
                        .insert(obligation);
                }
            }
        }
        if matched_any {
            self.relevant_events = self.relevant_events.saturating_add(1);
        }
    }

    fn mastery_route_matches(
        &self,
        obligation: usize,
        event_mask: u16,
        actors: ValidationEventActors,
    ) -> bool {
        if event_mask == ACTOR {
            return true;
        }
        let definition = &self.definitions[obligation];
        let Some(route) = definition.validation_route else {
            return false;
        };
        let active = |actor_id: Option<u64>| {
            actor_id.is_some_and(|actor_id| {
                self.actor_active
                    .get(&actor_id)
                    .is_some_and(|obligations| obligations.contains(&obligation))
                    || self
                        .actor_selection_active
                        .get(&actor_id)
                        .is_some_and(|obligations| obligations.contains(&obligation))
            })
        };
        let directly_selected = self.direct_epoch[obligation] == self.epoch;
        let property_matches = definition.property_ids.is_empty()
            || actors
                .property
                .is_some_and(|property| definition.property_ids.contains(&property));

        match event_mask {
            ENTITY_ATTRIBUTES | TEMPORARY_ATTRIBUTES => active(actors.source),
            DAMAGE => match route {
                MasteryValidationRoute::OutgoingDamage => active(actors.source) && property_matches,
                MasteryValidationRoute::OutgoingSelectedAbilityDamage
                | MasteryValidationRoute::NamedSkillOutput => {
                    active(actors.source) && directly_selected && property_matches
                }
                MasteryValidationRoute::OwnedCompanionOutgoingDamage => {
                    active(actors.source)
                        && actors
                            .direct_source
                            .is_some_and(|direct| Some(direct) != actors.source)
                }
                MasteryValidationRoute::IncomingDamageMitigation => {
                    if !active(actors.target) || !property_matches {
                        return false;
                    }
                    match definition.component_kind.as_deref() {
                        Some("block-damage-reduction") => actors.blocked == Some(true),
                        Some("lucky-block-damage-reduction") => {
                            actors.blocked == Some(true) && actors.lucky == Some(true)
                        }
                        Some("all-element-resistance") => actors.property.is_some_and(|p| p != 0),
                        _ => true,
                    }
                }
                _ => false,
            },
            HEALING => match route {
                MasteryValidationRoute::OutgoingHealing => active(actors.source),
                MasteryValidationRoute::OutgoingShieldOrBarrierState => {
                    active(actors.source) && directly_selected
                }
                _ => false,
            },
            SHIELD_STATE => {
                (route == MasteryValidationRoute::OutgoingShieldOrBarrierState
                    && active(actors.source))
                    || (route == MasteryValidationRoute::NamedShieldState
                        && actors.source.is_some_and(|actor_id| {
                            self.status_active_counts
                                .get(&actor_id)
                                .is_some_and(|active| active.contains_key(&obligation))
                        }))
            }
            RESOURCE => {
                (route == MasteryValidationRoute::OwnedResourceTransition && active(actors.source))
                    || (route == MasteryValidationRoute::NamedResourceDecayLifecycle
                        && actors.source.is_some_and(|actor_id| {
                            self.status_active_counts
                                .get(&actor_id)
                                .is_some_and(|active| active.contains_key(&obligation))
                        }))
            }
            COOLDOWN => {
                route == MasteryValidationRoute::SelectedAbilityCooldownTransition
                    && active(actors.source)
                    && directly_selected
            }
            STATUS => {
                matches!(
                    route,
                    MasteryValidationRoute::NamedStatusLifecycle
                        | MasteryValidationRoute::NamedShieldState
                        | MasteryValidationRoute::NamedResourceDecayLifecycle
                ) && directly_selected
                    && (active(actors.source) || active(actors.target))
            }
            CAST => match route {
                MasteryValidationRoute::OutgoingSelectedAbilityDamage
                | MasteryValidationRoute::SelectedAbilityCooldownTransition
                | MasteryValidationRoute::NamedSkillOutput
                | MasteryValidationRoute::NamedStatusLifecycle => {
                    active(actors.source) && directly_selected
                }
                _ => false,
            },
            _ => false,
        }
    }

    pub fn report(&self) -> RdpsValidationReport {
        let mut summary = RdpsValidationSummary {
            total_obligations: self.definitions.len() as u64,
            ..RdpsValidationSummary::default()
        };
        let mut by_domain = BTreeMap::<String, RdpsValidationDomainSummary>::new();
        let mut obligations = Vec::with_capacity(self.definitions.len());
        for (definition, state) in self.definitions.iter().zip(&self.states) {
            let observed_required = state.observed_mask & definition.required_mask;
            let coverage_state = if observed_required == 0 {
                summary.no_candidate_evidence += 1;
                "no-candidate-evidence"
            } else if observed_required == definition.required_mask {
                summary.candidate_event_coverage_complete += 1;
                "candidate-event-coverage-complete"
            } else {
                summary.partial_candidate_event_coverage += 1;
                "partial-candidate-event-coverage"
            };
            let domain = by_domain.entry(definition.domain.clone()).or_default();
            domain.total += 1;
            match coverage_state {
                "no-candidate-evidence" => domain.no_candidate_evidence += 1,
                "candidate-event-coverage-complete" => {
                    domain.candidate_event_coverage_complete += 1
                }
                _ => domain.partial_candidate_event_coverage += 1,
            }
            obligations.push(RdpsValidationObligationReport {
                obligation_id: definition.obligation_id.clone(),
                domain: definition.domain.clone(),
                subject_kind: definition.subject_kind.clone(),
                subject_id: definition.subject_id.clone(),
                subject_name: definition.subject_name.clone(),
                requirements: definition.requirements.clone(),
                selector_contract: definition.selector_contract.clone(),
                coverage_state: coverage_state.into(),
                required_event_kinds: mask_names(definition.required_mask),
                observed_event_kinds: mask_names(observed_required),
                missing_event_kinds: mask_names(definition.required_mask & !observed_required),
                direct_matches: state.direct_matches,
                contextual_matches: state.contextual_matches,
                first_sequence: state.first_sequence,
                last_sequence: state.last_sequence,
                matched_identifiers: state.matched_identifiers.iter().cloned().collect(),
                status_states: state.status_states.clone(),
                selected_actor_ids: state
                    .selected_actor_ids
                    .iter()
                    .map(u64::to_string)
                    .collect(),
                provider_recipient_observations: state
                    .provider_recipient_observations
                    .iter()
                    .map(
                        |(
                            &(provider_actor_id, recipient_actor_id, effect_id),
                            &observation_count,
                        )| {
                            RdpsValidationProviderRecipientObservation {
                                provider_actor_id: provider_actor_id.map(|id| id.to_string()),
                                recipient_actor_id: recipient_actor_id.to_string(),
                                effect_id: effect_id.to_string(),
                                observation_count,
                            }
                        },
                    )
                    .collect(),
                status_origin_observations: state
                    .status_origin_observations
                    .iter()
                    .map(
                        |(
                            &(
                                provider_actor_id,
                                recipient_actor_id,
                                effect_id,
                                origin_source_type_id,
                                origin_source_config_id,
                            ),
                            &observation_count,
                        )| {
                            RdpsValidationStatusOriginObservation {
                                provider_actor_id: provider_actor_id.map(|id| id.to_string()),
                                recipient_actor_id: recipient_actor_id.to_string(),
                                effect_id: effect_id.to_string(),
                                origin_source_type_id,
                                origin_source_config_id: origin_source_config_id.to_string(),
                                observation_count,
                            }
                        },
                    )
                    .collect(),
                status_instance_ids: state
                    .status_instance_ids
                    .iter()
                    .map(i64::to_string)
                    .collect(),
                minimum_stacks: state.minimum_stacks,
                maximum_stacks: state.maximum_stacks,
                maximum_concurrent_instances: state.maximum_concurrent_instances,
                maximum_concurrent_providers: state.maximum_concurrent_providers,
                ambiguous_status_removals: state.ambiguous_status_removals,
                direct_damage_events: state.direct_damage_events,
                direct_damage: state.direct_damage.to_string(),
                recipient_window_damage_events: state.recipient_window_damage_events,
                recipient_window_damage: state.recipient_window_damage.to_string(),
                unresolved_recipient_window_damage_events: state
                    .unresolved_recipient_window_damage_events,
                target_window_damage_events: state.target_window_damage_events,
                target_window_damage: state.target_window_damage.to_string(),
                unresolved_target_window_damage_events: state
                    .unresolved_target_window_damage_events,
                expired_status_windows: state.expired_status_windows,
                single_provider_window_damage_events: state.single_provider_window_damage_events,
                single_provider_window_damage: state.single_provider_window_damage.to_string(),
                ambiguous_provider_window_damage_events: state
                    .ambiguous_provider_window_damage_events,
                stack_at_damage_observations: stack_at_damage_report(&state.stack_at_damage),
                formula_input_snapshot_count: None,
                complete_formula_input_snapshot_count: None,
                formula_input_snapshots: state.formula_input_snapshots.clone(),
                packet_damage_row_count: None,
                packet_damage_rows: state
                    .packet_damage_rows
                    .iter()
                    .map(|(key, aggregate)| RdpsValidationPacketDamageRow {
                        context: damage_evidence_context_name(key.context).into(),
                        source_actor_id: key.source_actor.to_string(),
                        direct_source_actor_id: key.direct_source_actor.map(|id| id.to_string()),
                        target_actor_id: key.target_actor.to_string(),
                        ability_id: key.ability_id.map(|id| id.to_string()),
                        hit_event_id: key.hit_event_id,
                        owner_id: key.owner_id,
                        damage_source: key.damage_source,
                        damage_type: key.damage_type,
                        type_flags: key.type_flags,
                        property: key.property,
                        passive_uuid: key.passive_uuid,
                        damage_mode: key.damage_mode,
                        skill_effect_uuid: key.skill_effect_uuid.map(|id| id.to_string()),
                        skill_effect_group_index: key.skill_effect_group_index,
                        skill_effect_component_index: key.skill_effect_component_index,
                        skill_effect_component_count: key.skill_effect_component_count,
                        first_sequence: aggregate.first_sequence,
                        last_sequence: aggregate.last_sequence,
                        first_observed_micros: aggregate.first_observed_micros,
                        last_observed_micros: aggregate.last_observed_micros,
                        event_count: aggregate.event_count,
                        amount: aggregate.amount.to_string(),
                        actual_amount: aggregate.actual_amount.to_string(),
                        hp_loss: aggregate.hp_loss.to_string(),
                        shield_loss: aggregate.shield_loss.to_string(),
                        normal_value: aggregate.normal_value.to_string(),
                        lucky_value: aggregate.lucky_value.to_string(),
                    })
                    .collect(),
                attribute_values: state
                    .attribute_values
                    .iter()
                    .map(|(id, values)| {
                        (
                            id.clone(),
                            values.iter().map(i64::to_string).collect::<Vec<_>>(),
                        )
                    })
                    .collect(),
                attribute_transition_counts: state.attribute_transition_counts.clone(),
                projection_statuses: state.projection_statuses.iter().cloned().collect(),
                projected_provider_recipient_observations: state
                    .projected_provider_recipient_observations
                    .iter()
                    .map(
                        |(
                            &(provider_actor_id, recipient_actor_id, effect_id),
                            &observation_count,
                        )| {
                            RdpsValidationProjectedProviderRecipientObservation {
                                provider_actor_id: provider_actor_id.to_string(),
                                recipient_actor_id: recipient_actor_id.to_string(),
                                effect_id: effect_id.to_string(),
                                observation_count,
                            }
                        },
                    )
                    .collect(),
                projected_integer_events: state.projected_integer_events,
                projected_integer_amount: state.projected_integer_amount.to_string(),
                projected_integer_observed_damage: state
                    .projected_integer_observed_damage
                    .to_string(),
                projected_rational_events: state.projected_rational_events,
                projected_rational_totals: state
                    .projected_rational_totals
                    .iter()
                    .map(|(&denominator, &(numerator, event_count))| {
                        RdpsValidationRationalContributionTotal {
                            numerator: numerator.to_string(),
                            denominator: denominator.to_string(),
                            event_count,
                        }
                    })
                    .collect(),
                projected_rational_observed_damage: state
                    .projected_rational_observed_damage
                    .to_string(),
                projected_invalid_events: state.projected_invalid_events,
                projected_excluded_events: state.projected_excluded_events,
            });
        }

        let dreamscope_terminal_effects = self
            .dreamscope_terminal_effects
            .iter()
            .map(
                |(&effect_id, state)| RdpsValidationDreamscopeTerminalEffectReport {
                    effect_id: effect_id.to_string(),
                    source_match: dreamscope_observed_effect_match(effect_id),
                    status_states: state.status_states.clone(),
                    provider_recipient_observations: state
                        .provider_recipient_observations
                        .iter()
                        .map(
                            |(&(provider_actor_id, recipient_actor_id), &observation_count)| {
                                RdpsValidationProviderRecipientObservation {
                                    provider_actor_id: provider_actor_id.map(|id| id.to_string()),
                                    recipient_actor_id: recipient_actor_id.to_string(),
                                    effect_id: effect_id.to_string(),
                                    observation_count,
                                }
                            },
                        )
                        .collect(),
                    status_instance_ids: state
                        .status_instance_ids
                        .iter()
                        .map(i64::to_string)
                        .collect(),
                    packet_levels: state.packet_levels.clone(),
                    packet_part_ids: state.packet_part_ids.clone(),
                    packet_counts: state.packet_counts.clone(),
                    packet_durations_millis: state.packet_durations_millis.clone(),
                    minimum_stacks: state.minimum_stacks,
                    maximum_stacks: state.maximum_stacks,
                    maximum_concurrent_instances: state.maximum_concurrent_instances,
                    maximum_concurrent_providers: state.maximum_concurrent_providers,
                    ambiguous_status_removals: state.ambiguous_status_removals,
                    open_unbounded_status_windows: state.open_unbounded_status_windows,
                    recipient_window_damage_events: state.recipient_window_damage_events,
                    recipient_window_damage: state.recipient_window_damage.to_string(),
                    unresolved_recipient_window_damage_events: state
                        .unresolved_recipient_window_damage_events,
                    external_provider_window_damage_events: state
                        .external_provider_window_damage_events,
                    external_provider_window_damage: state
                        .external_provider_window_damage
                        .to_string(),
                    target_window_damage_events: state.target_window_damage_events,
                    target_window_damage: state.target_window_damage.to_string(),
                    unresolved_target_window_damage_events: state
                        .unresolved_target_window_damage_events,
                    expired_status_windows: state.expired_status_windows,
                    single_provider_window_damage_events: state
                        .single_provider_window_damage_events,
                    single_provider_window_damage: state.single_provider_window_damage.to_string(),
                    ambiguous_provider_window_damage_events: state
                        .ambiguous_provider_window_damage_events,
                    stack_at_damage_observations: stack_at_damage_report(&state.stack_at_damage),
                    source_observations: state
                        .source_observations
                        .iter()
                        .map(|(key, &observation_count)| {
                            RdpsValidationDreamscopeSourceObservation {
                                provider_actor_id: key.provider_actor_id.map(|id| id.to_string()),
                                source_type_id: key.source_type_id,
                                source_config_id: key.source_config_id.map(|id| id.to_string()),
                                match_kind: key.match_kind,
                                route_resolution: key.route_resolution,
                                equipped_variant_resolution: key.equipped_variant_resolution,
                                resolution: key.resolution,
                                source_id: key.source_id.clone(),
                                source_kind: key.source_kind.clone(),
                                selected_factor_item_id: key
                                    .selected_factor_item_id
                                    .map(|id| id.to_string()),
                                selected_factor_grade: key.selected_factor_grade,
                                observation_count,
                            }
                        })
                        .collect(),
                    remote_calculation: dreamscope_remote_calculation_readiness(effect_id, state),
                },
            )
            .collect();
        let remote_rdps_readiness = remote_rdps_readiness_ledger(&self.dreamscope_terminal_effects);

        let provisional_build_mismatch = self
            .observed_builds
            .iter()
            .any(|build| build != &self.manifest_build);
        let mut warnings = Vec::new();
        if provisional_build_mismatch {
            warnings.push(format!(
                "validation manifest build {} differs from observed build(s) {}; unchanged routes were evaluated provisionally and no unknown relationship was promoted",
                self.manifest_build,
                self.observed_builds
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if self.projected_invalid_events > 0 {
            warnings.push(format!(
                "{} projected contribution event(s) failed fail-closed conservation checks and were retained as invalid evidence",
                self.projected_invalid_events
            ));
        }
        if !self.unmatched_projected_effects.is_empty() {
            warnings.push(format!(
                "{} projected contribution effect ID(s) were not indexed by the matching-build validation manifest and were retained as unmatched evidence",
                self.unmatched_projected_effects.len()
            ));
        }

        RdpsValidationReport {
            schema_version: RDPS_VALIDATION_REPORT_SCHEMA_VERSION,
            manifest_game_build: self.manifest_build.clone(),
            observed_game_builds: self.observed_builds.iter().cloned().collect(),
            provisional_build_mismatch,
            warnings,
            total_events: self.total_events,
            relevant_events: self.relevant_events,
            projection: RdpsValidationProjectionSummary {
                integer_events: self.projected_integer_events,
                rational_events: self.projected_rational_events,
                invalid_events: self.projected_invalid_events,
                excluded_events: self.projected_excluded_events,
                statuses: self.projection_statuses.iter().cloned().collect(),
                unmatched_effects: self
                    .unmatched_projected_effects
                    .iter()
                    .map(|(&effect_id, &observation_count)| {
                        RdpsValidationUnmatchedProjectedEffect {
                            effect_id: effect_id.to_string(),
                            observation_count,
                        }
                    })
                    .collect(),
            },
            summary,
            by_domain,
            obligations,
            dreamscope_terminal_effects,
            remote_rdps_readiness,
        }
    }

    pub fn progress(&self) -> RdpsValidationProgress {
        let mut progress = RdpsValidationProgress::default();
        for (definition, state) in self.definitions.iter().zip(&self.states) {
            let observed_required = state.observed_mask & definition.required_mask;
            accumulate_validation_progress(&mut progress, definition, observed_required);
        }
        progress
    }

    /// Returns cumulative event-family coverage without importing any evidence
    /// counters or transient actor state into either analyzer.
    ///
    /// The live desktop uses this to display the union of its immutable
    /// exact-build session baseline and the current capture. Per-session
    /// reports remain independent, so rebuilding the durable cumulative report
    /// cannot count an older session twice.
    pub fn progress_with_baseline(
        &self,
        baseline: &Self,
    ) -> Result<RdpsValidationProgress, RdpsValidationError> {
        if self.manifest_build != baseline.manifest_build
            || self.definitions.len() != baseline.definitions.len()
        {
            return Err(RdpsValidationError::IncompatibleReport(
                "coverage baseline uses a different validation manifest".into(),
            ));
        }

        let mut progress = RdpsValidationProgress::default();
        for index in 0..self.definitions.len() {
            let definition = &self.definitions[index];
            let baseline_definition = &baseline.definitions[index];
            if definition.obligation_id != baseline_definition.obligation_id
                || definition.required_mask != baseline_definition.required_mask
                || definition.selector_contract != baseline_definition.selector_contract
            {
                return Err(RdpsValidationError::IncompatibleReport(format!(
                    "coverage baseline obligation {} differs from the active manifest",
                    definition.obligation_id
                )));
            }
            let observed_required = (self.states[index].observed_mask
                | baseline.states[index].observed_mask)
                & definition.required_mask;
            accumulate_validation_progress(&mut progress, definition, observed_required);
        }
        Ok(progress)
    }
}

fn accumulate_validation_progress(
    progress: &mut RdpsValidationProgress,
    definition: &ObligationDefinition,
    observed_required: u16,
) {
    progress.total_obligations += 1;
    let domain = progress
        .by_domain
        .entry(definition.domain.clone())
        .or_default();
    domain.total += 1;
    if observed_required == 0 {
        progress.no_candidate_evidence += 1;
        domain.no_candidate_evidence += 1;
    } else if observed_required == definition.required_mask {
        progress.candidate_event_coverage_complete += 1;
        domain.candidate_event_coverage_complete += 1;
    } else {
        progress.partial_candidate_event_coverage += 1;
        domain.partial_candidate_event_coverage += 1;
    }
}

fn observe_projection_common(
    state: &mut ObligationState,
    event_sequence: u64,
    effect_id: i64,
    provider_actor_id: u64,
    recipient_actor_id: u64,
    projection_status: &str,
) {
    state.first_sequence = Some(
        state
            .first_sequence
            .map_or(event_sequence, |first| first.min(event_sequence)),
    );
    state.last_sequence = Some(
        state
            .last_sequence
            .map_or(event_sequence, |last| last.max(event_sequence)),
    );
    state
        .matched_identifiers
        .insert(format!("projected_effect:{effect_id}"));
    if !projection_status.is_empty() {
        state
            .projection_statuses
            .insert(projection_status.to_owned());
    }
    let observations = state
        .projected_provider_recipient_observations
        .entry((provider_actor_id, recipient_actor_id, effect_id))
        .or_default();
    *observations = observations.saturating_add(1);
}

fn merge_obligation_report(
    state: &mut ObligationState,
    source: &RdpsValidationObligationReport,
    observed_mask: u16,
) -> Result<(), RdpsValidationError> {
    state.observed_mask |= observed_mask;
    state.direct_matches = state.direct_matches.saturating_add(source.direct_matches);
    state.contextual_matches = state
        .contextual_matches
        .saturating_add(source.contextual_matches);
    if let Some(first) = source.first_sequence {
        state.first_sequence = Some(state.first_sequence.map_or(first, |value| value.min(first)));
    }
    if let Some(last) = source.last_sequence {
        state.last_sequence = Some(state.last_sequence.map_or(last, |value| value.max(last)));
    }
    state
        .matched_identifiers
        .extend(source.matched_identifiers.iter().cloned());
    merge_counts(&mut state.status_states, &source.status_states);
    for actor_id in &source.selected_actor_ids {
        state.selected_actor_ids.insert(parse_report_number(
            "obligations.selected_actor_ids",
            actor_id,
        )?);
    }
    for observation in &source.provider_recipient_observations {
        let provider = observation
            .provider_actor_id
            .as_deref()
            .map(|value| parse_report_number("provider_actor_id", value))
            .transpose()?;
        let recipient = parse_report_number("recipient_actor_id", &observation.recipient_actor_id)?;
        let effect_id = parse_report_number("effect_id", &observation.effect_id)?;
        let count = state
            .provider_recipient_observations
            .entry((provider, recipient, effect_id))
            .or_default();
        *count = count.saturating_add(observation.observation_count);
    }
    for observation in &source.status_origin_observations {
        let provider = observation
            .provider_actor_id
            .as_deref()
            .map(|value| parse_report_number("status_origin.provider_actor_id", value))
            .transpose()?;
        let recipient = parse_report_number(
            "status_origin.recipient_actor_id",
            &observation.recipient_actor_id,
        )?;
        let effect_id = parse_report_number("status_origin.effect_id", &observation.effect_id)?;
        let origin_source_config_id = parse_report_number(
            "status_origin.origin_source_config_id",
            &observation.origin_source_config_id,
        )?;
        let count = state
            .status_origin_observations
            .entry((
                provider,
                recipient,
                effect_id,
                observation.origin_source_type_id,
                origin_source_config_id,
            ))
            .or_default();
        *count = count.saturating_add(observation.observation_count);
    }
    for instance_id in &source.status_instance_ids {
        state
            .status_instance_ids
            .insert(parse_report_number("status_instance_id", instance_id)?);
    }
    state.minimum_stacks = match (state.minimum_stacks, source.minimum_stacks) {
        (Some(current), Some(source)) => Some(current.min(source)),
        (None, source) => source,
        (current, None) => current,
    };
    state.maximum_stacks = match (state.maximum_stacks, source.maximum_stacks) {
        (Some(current), Some(source)) => Some(current.max(source)),
        (None, source) => source,
        (current, None) => current,
    };
    state.maximum_concurrent_instances = state
        .maximum_concurrent_instances
        .max(source.maximum_concurrent_instances);
    state.maximum_concurrent_providers = state
        .maximum_concurrent_providers
        .max(source.maximum_concurrent_providers);
    state.ambiguous_status_removals = state
        .ambiguous_status_removals
        .saturating_add(source.ambiguous_status_removals);
    state.direct_damage_events = state
        .direct_damage_events
        .saturating_add(source.direct_damage_events);
    state.direct_damage = state.direct_damage.saturating_add(parse_report_number(
        "obligations.direct_damage",
        &source.direct_damage,
    )?);
    state.recipient_window_damage_events = state
        .recipient_window_damage_events
        .saturating_add(source.recipient_window_damage_events);
    state.recipient_window_damage =
        state
            .recipient_window_damage
            .saturating_add(parse_report_number(
                "obligations.recipient_window_damage",
                &source.recipient_window_damage,
            )?);
    state.unresolved_recipient_window_damage_events = state
        .unresolved_recipient_window_damage_events
        .saturating_add(source.unresolved_recipient_window_damage_events);
    state.target_window_damage_events = state
        .target_window_damage_events
        .saturating_add(source.target_window_damage_events);
    state.target_window_damage = state
        .target_window_damage
        .saturating_add(parse_report_number(
            "obligations.target_window_damage",
            &source.target_window_damage,
        )?);
    state.unresolved_target_window_damage_events = state
        .unresolved_target_window_damage_events
        .saturating_add(source.unresolved_target_window_damage_events);
    state.expired_status_windows = state
        .expired_status_windows
        .saturating_add(source.expired_status_windows);
    state.single_provider_window_damage_events = state
        .single_provider_window_damage_events
        .saturating_add(source.single_provider_window_damage_events);
    state.single_provider_window_damage =
        state
            .single_provider_window_damage
            .saturating_add(parse_report_number(
                "obligations.single_provider_window_damage",
                &source.single_provider_window_damage,
            )?);
    state.ambiguous_provider_window_damage_events = state
        .ambiguous_provider_window_damage_events
        .saturating_add(source.ambiguous_provider_window_damage_events);
    merge_stack_at_damage_observations(
        &mut state.stack_at_damage,
        &source.stack_at_damage_observations,
    )?;
    state
        .formula_input_snapshots
        .extend(source.formula_input_snapshots.iter().cloned());
    for row in &source.packet_damage_rows {
        let context = match row.context.as_str() {
            "direct-selector" => 0,
            "recipient-window" => 1,
            "target-window" => 2,
            other => {
                return Err(RdpsValidationError::IncompatibleReport(format!(
                    "unknown packet damage context {other}"
                )));
            }
        };
        let key = DamagePacketEvidenceKey {
            context,
            source_actor: parse_report_number(
                "packet_damage.source_actor_id",
                &row.source_actor_id,
            )?,
            direct_source_actor: row
                .direct_source_actor_id
                .as_deref()
                .map(|value| parse_report_number("packet_damage.direct_source_actor_id", value))
                .transpose()?,
            target_actor: parse_report_number(
                "packet_damage.target_actor_id",
                &row.target_actor_id,
            )?,
            ability_id: row
                .ability_id
                .as_deref()
                .map(|value| parse_report_number("packet_damage.ability_id", value))
                .transpose()?,
            hit_event_id: row.hit_event_id,
            owner_id: row.owner_id,
            damage_source: row.damage_source,
            damage_type: row.damage_type,
            type_flags: row.type_flags,
            property: row.property,
            passive_uuid: row.passive_uuid,
            damage_mode: row.damage_mode,
            skill_effect_uuid: row
                .skill_effect_uuid
                .as_deref()
                .map(|value| parse_report_number("packet_damage.skill_effect_uuid", value))
                .transpose()?,
            skill_effect_group_index: row.skill_effect_group_index,
            skill_effect_component_index: row.skill_effect_component_index,
            skill_effect_component_count: row.skill_effect_component_count,
        };
        let aggregate = state.packet_damage_rows.entry(key).or_default();
        aggregate.first_sequence = minimum_option(aggregate.first_sequence, row.first_sequence);
        aggregate.last_sequence = maximum_option(aggregate.last_sequence, row.last_sequence);
        aggregate.first_observed_micros =
            minimum_option(aggregate.first_observed_micros, row.first_observed_micros);
        aggregate.last_observed_micros =
            maximum_option(aggregate.last_observed_micros, row.last_observed_micros);
        aggregate.event_count = aggregate.event_count.saturating_add(row.event_count);
        aggregate.amount = aggregate
            .amount
            .saturating_add(parse_report_number("packet_damage.amount", &row.amount)?);
        aggregate.actual_amount = aggregate.actual_amount.saturating_add(parse_report_number(
            "packet_damage.actual_amount",
            &row.actual_amount,
        )?);
        aggregate.hp_loss = aggregate
            .hp_loss
            .saturating_add(parse_report_number("packet_damage.hp_loss", &row.hp_loss)?);
        aggregate.shield_loss = aggregate.shield_loss.saturating_add(parse_report_number(
            "packet_damage.shield_loss",
            &row.shield_loss,
        )?);
        aggregate.normal_value = aggregate.normal_value.saturating_add(parse_report_number(
            "packet_damage.normal_value",
            &row.normal_value,
        )?);
        aggregate.lucky_value = aggregate.lucky_value.saturating_add(parse_report_number(
            "packet_damage.lucky_value",
            &row.lucky_value,
        )?);
    }
    for (attribute_id, values) in &source.attribute_values {
        let target = state
            .attribute_values
            .entry(attribute_id.clone())
            .or_default();
        for value in values {
            target.insert(parse_report_number("attribute_value", value)?);
        }
    }
    merge_counts(
        &mut state.attribute_transition_counts,
        &source.attribute_transition_counts,
    );
    state
        .projection_statuses
        .extend(source.projection_statuses.iter().cloned());
    for observation in &source.projected_provider_recipient_observations {
        let provider = parse_report_number(
            "projected_provider_actor_id",
            &observation.provider_actor_id,
        )?;
        let recipient = parse_report_number(
            "projected_recipient_actor_id",
            &observation.recipient_actor_id,
        )?;
        let effect_id = parse_report_number("projected_effect_id", &observation.effect_id)?;
        let count = state
            .projected_provider_recipient_observations
            .entry((provider, recipient, effect_id))
            .or_default();
        *count = count.saturating_add(observation.observation_count);
    }
    state.projected_integer_events = state
        .projected_integer_events
        .saturating_add(source.projected_integer_events);
    state.projected_integer_amount =
        state
            .projected_integer_amount
            .saturating_add(parse_report_number(
                "obligations.projected_integer_amount",
                &source.projected_integer_amount,
            )?);
    state.projected_integer_observed_damage = state
        .projected_integer_observed_damage
        .saturating_add(parse_report_number(
            "obligations.projected_integer_observed_damage",
            &source.projected_integer_observed_damage,
        )?);
    state.projected_rational_events = state
        .projected_rational_events
        .saturating_add(source.projected_rational_events);
    for total in &source.projected_rational_totals {
        let denominator = parse_report_number(
            "obligations.projected_rational_totals.denominator",
            &total.denominator,
        )?;
        if denominator <= 0 {
            return Err(RdpsValidationError::InvalidReportNumber {
                field: "obligations.projected_rational_totals.denominator".into(),
                value: total.denominator.clone(),
            });
        }
        let numerator = parse_report_number(
            "obligations.projected_rational_totals.numerator",
            &total.numerator,
        )?;
        let target = state
            .projected_rational_totals
            .entry(denominator)
            .or_default();
        target.0 = target.0.saturating_add(numerator);
        target.1 = target.1.saturating_add(total.event_count);
    }
    state.projected_rational_observed_damage = state
        .projected_rational_observed_damage
        .saturating_add(parse_report_number(
            "obligations.projected_rational_observed_damage",
            &source.projected_rational_observed_damage,
        )?);
    state.projected_invalid_events = state
        .projected_invalid_events
        .saturating_add(source.projected_invalid_events);
    state.projected_excluded_events = state
        .projected_excluded_events
        .saturating_add(source.projected_excluded_events);
    Ok(())
}

fn merge_dreamscope_terminal_effect_report(
    state: &mut DreamscopeTerminalEffectState,
    effect_id: i64,
    source: &RdpsValidationDreamscopeTerminalEffectReport,
) -> Result<(), RdpsValidationError> {
    merge_counts(&mut state.status_states, &source.status_states);
    merge_counts(&mut state.packet_levels, &source.packet_levels);
    merge_counts(&mut state.packet_part_ids, &source.packet_part_ids);
    merge_counts(&mut state.packet_counts, &source.packet_counts);
    merge_counts(
        &mut state.packet_durations_millis,
        &source.packet_durations_millis,
    );
    state.scalar_resolution = strongest_remote_scalar_resolution(
        state.scalar_resolution,
        source.remote_calculation.scalar_resolution,
    );
    for observation in &source.provider_recipient_observations {
        let observation_effect_id =
            parse_report_number::<i64>("dreamscope observation effect_id", &observation.effect_id)?;
        if observation_effect_id != effect_id {
            return Err(RdpsValidationError::IncompatibleReport(format!(
                "Dreamscope terminal effect {effect_id} contains observation for effect {observation_effect_id}"
            )));
        }
        let provider = observation
            .provider_actor_id
            .as_deref()
            .map(|value| parse_report_number("dreamscope provider_actor_id", value))
            .transpose()?;
        let recipient = parse_report_number(
            "dreamscope recipient_actor_id",
            &observation.recipient_actor_id,
        )?;
        let count = state
            .provider_recipient_observations
            .entry((provider, recipient))
            .or_default();
        *count = count.saturating_add(observation.observation_count);
    }
    for observation in &source.source_observations {
        let provider_actor_id = observation
            .provider_actor_id
            .as_deref()
            .map(|value| parse_report_number("dreamscope source provider_actor_id", value))
            .transpose()?;
        let source_config_id = observation
            .source_config_id
            .as_deref()
            .map(|value| parse_report_number("dreamscope source_config_id", value))
            .transpose()?;
        let route_resolution = if observation.route_resolution
            == EffectFingerprintResolution::Unresolved
            && observation.resolution != EffectFingerprintResolution::Unresolved
        {
            observation.resolution
        } else {
            observation.route_resolution
        };
        let key = DreamscopeSourceObservationKey {
            provider_actor_id,
            source_type_id: observation.source_type_id,
            source_config_id,
            match_kind: observation.match_kind,
            route_resolution,
            equipped_variant_resolution: observation.equipped_variant_resolution,
            resolution: observation.resolution,
            source_id: observation.source_id.clone(),
            source_kind: observation.source_kind.clone(),
            selected_factor_item_id: observation
                .selected_factor_item_id
                .as_deref()
                .map(|value| parse_report_number("dreamscope selected_factor_item_id", value))
                .transpose()?,
            selected_factor_grade: observation.selected_factor_grade,
        };
        let count = state.source_observations.entry(key).or_default();
        *count = count.saturating_add(observation.observation_count);
    }
    for instance_id in &source.status_instance_ids {
        state.status_instance_ids.insert(parse_report_number(
            "dreamscope status_instance_id",
            instance_id,
        )?);
    }
    state.minimum_stacks = match (state.minimum_stacks, source.minimum_stacks) {
        (Some(current), Some(source)) => Some(current.min(source)),
        (None, source) => source,
        (current, None) => current,
    };
    state.maximum_stacks = match (state.maximum_stacks, source.maximum_stacks) {
        (Some(current), Some(source)) => Some(current.max(source)),
        (None, source) => source,
        (current, None) => current,
    };
    state.maximum_concurrent_instances = state
        .maximum_concurrent_instances
        .max(source.maximum_concurrent_instances);
    state.maximum_concurrent_providers = state
        .maximum_concurrent_providers
        .max(source.maximum_concurrent_providers);
    state.ambiguous_status_removals = state
        .ambiguous_status_removals
        .saturating_add(source.ambiguous_status_removals);
    state.open_unbounded_status_windows = state
        .open_unbounded_status_windows
        .saturating_add(source.open_unbounded_status_windows);
    state.recipient_window_damage_events = state
        .recipient_window_damage_events
        .saturating_add(source.recipient_window_damage_events);
    state.recipient_window_damage =
        state
            .recipient_window_damage
            .saturating_add(parse_report_number(
                "dreamscope recipient_window_damage",
                &source.recipient_window_damage,
            )?);
    state.unresolved_recipient_window_damage_events = state
        .unresolved_recipient_window_damage_events
        .saturating_add(source.unresolved_recipient_window_damage_events);
    state.external_provider_window_damage_events = state
        .external_provider_window_damage_events
        .saturating_add(source.external_provider_window_damage_events);
    state.external_provider_window_damage =
        state
            .external_provider_window_damage
            .saturating_add(parse_report_number(
                "dreamscope external_provider_window_damage",
                &source.external_provider_window_damage,
            )?);
    state.target_window_damage_events = state
        .target_window_damage_events
        .saturating_add(source.target_window_damage_events);
    state.target_window_damage = state
        .target_window_damage
        .saturating_add(parse_report_number(
            "dreamscope target_window_damage",
            &source.target_window_damage,
        )?);
    state.unresolved_target_window_damage_events = state
        .unresolved_target_window_damage_events
        .saturating_add(source.unresolved_target_window_damage_events);
    state.expired_status_windows = state
        .expired_status_windows
        .saturating_add(source.expired_status_windows);
    state.single_provider_window_damage_events = state
        .single_provider_window_damage_events
        .saturating_add(source.single_provider_window_damage_events);
    state.single_provider_window_damage =
        state
            .single_provider_window_damage
            .saturating_add(parse_report_number(
                "dreamscope single_provider_window_damage",
                &source.single_provider_window_damage,
            )?);
    state.ambiguous_provider_window_damage_events = state
        .ambiguous_provider_window_damage_events
        .saturating_add(source.ambiguous_provider_window_damage_events);
    merge_stack_at_damage_observations(
        &mut state.stack_at_damage,
        &source.stack_at_damage_observations,
    )?;
    Ok(())
}

fn strongest_remote_scalar_resolution(
    left: RdpsValidationRemoteScalarResolution,
    right: RdpsValidationRemoteScalarResolution,
) -> RdpsValidationRemoteScalarResolution {
    use RdpsValidationRemoteScalarResolution::{
        CounterfactualReplay, PacketScalar, RecipientAttributeTransition, Unresolved,
    };

    match (left, right) {
        (PacketScalar, _) | (_, PacketScalar) => PacketScalar,
        (RecipientAttributeTransition, _) | (_, RecipientAttributeTransition) => {
            RecipientAttributeTransition
        }
        (CounterfactualReplay, _) | (_, CounterfactualReplay) => CounterfactualReplay,
        (Unresolved, Unresolved) => Unresolved,
    }
}

fn merge_counts(target: &mut BTreeMap<String, u64>, source: &BTreeMap<String, u64>) {
    for (key, value) in source {
        let count = target.entry(key.clone()).or_default();
        *count = count.saturating_add(*value);
    }
}

fn parse_report_number<T>(field: &str, value: &str) -> Result<T, RdpsValidationError>
where
    T: std::str::FromStr,
{
    value
        .parse()
        .map_err(|_| RdpsValidationError::InvalidReportNumber {
            field: field.into(),
            value: value.into(),
        })
}

#[derive(Clone, Copy)]
enum SelectorIndex {
    Effect,
    Skill,
    Recount,
    Attribute,
    Class,
    Specialization,
    Item,
}

fn index_values<T>(index: &mut HashMap<T, Vec<usize>>, values: &[T], obligation: usize)
where
    T: Copy + Eq + std::hash::Hash,
{
    for &value in values {
        let obligations = index.entry(value).or_default();
        if obligations.last().copied() != Some(obligation) {
            obligations.push(obligation);
        }
    }
}

fn record_packet_damage_row(
    state: &mut ObligationState,
    event: &rlogs_events::DamageEvent,
    context: u8,
    sequence: u64,
    observed_micros: u64,
) {
    let key = DamagePacketEvidenceKey {
        context,
        source_actor: event.source.actor_id.0,
        direct_source_actor: event.direct_source.map(|source| source.actor_id.0),
        target_actor: event.target.actor_id.0,
        ability_id: event.ability.map(|ability| ability.0),
        hit_event_id: event.hit_event_id,
        owner_id: event.packet.owner_id,
        damage_source: event.damage_source,
        damage_type: event.damage_type,
        type_flags: event.packet.type_flags,
        property: event.packet.property,
        passive_uuid: event.packet.passive_uuid,
        damage_mode: event.packet.damage_mode,
        skill_effect_uuid: event.packet.skill_effect_uuid,
        skill_effect_group_index: event.packet.skill_effect_group_index,
        skill_effect_component_index: event.packet.skill_effect_component_index,
        skill_effect_component_count: event.packet.skill_effect_component_count,
    };
    let aggregate = state.packet_damage_rows.entry(key).or_default();
    aggregate.first_sequence = minimum_option(aggregate.first_sequence, Some(sequence));
    aggregate.last_sequence = maximum_option(aggregate.last_sequence, Some(sequence));
    aggregate.first_observed_micros =
        minimum_option(aggregate.first_observed_micros, Some(observed_micros));
    aggregate.last_observed_micros =
        maximum_option(aggregate.last_observed_micros, Some(observed_micros));
    aggregate.event_count = aggregate.event_count.saturating_add(1);
    aggregate.amount = aggregate.amount.saturating_add(i128::from(event.amount));
    aggregate.actual_amount = aggregate
        .actual_amount
        .saturating_add(i128::from(event.actual_amount.unwrap_or_default()));
    aggregate.hp_loss = aggregate
        .hp_loss
        .saturating_add(i128::from(event.hp_loss.unwrap_or_default()));
    aggregate.shield_loss = aggregate
        .shield_loss
        .saturating_add(i128::from(event.shield_loss.unwrap_or_default()));
    aggregate.normal_value = aggregate
        .normal_value
        .saturating_add(i128::from(event.packet.normal_value.unwrap_or_default()));
    aggregate.lucky_value = aggregate
        .lucky_value
        .saturating_add(i128::from(event.packet.lucky_value.unwrap_or_default()));
}

fn record_stack_at_damage(
    observations: &mut BTreeMap<StackAtDamageKey, StackAtDamageAggregate>,
    context: u8,
    windows: Vec<StatusWindowStackKey>,
    damage: i128,
) {
    let aggregate = observations
        .entry(StackAtDamageKey { context, windows })
        .or_default();
    aggregate.event_count = aggregate.event_count.saturating_add(1);
    aggregate.damage = aggregate.damage.saturating_add(damage);
}

fn stack_at_damage_report(
    observations: &BTreeMap<StackAtDamageKey, StackAtDamageAggregate>,
) -> Vec<RdpsValidationStackAtDamageObservation> {
    observations
        .iter()
        .map(|(key, aggregate)| RdpsValidationStackAtDamageObservation {
            context: damage_evidence_context_name(key.context).into(),
            active_windows: key
                .windows
                .iter()
                .map(|window| RdpsValidationActiveWindowStack {
                    effect_id: window.effect_id.to_string(),
                    status_instance_id: window.instance_id.map(|id| id.to_string()),
                    provider_actor_id: window.provider_actor.map(|id| id.to_string()),
                    stacks: window.stacks,
                })
                .collect(),
            event_count: aggregate.event_count,
            damage: aggregate.damage.to_string(),
        })
        .collect()
}

fn merge_stack_at_damage_observations(
    target: &mut BTreeMap<StackAtDamageKey, StackAtDamageAggregate>,
    source: &[RdpsValidationStackAtDamageObservation],
) -> Result<(), RdpsValidationError> {
    for observation in source {
        let context = match observation.context.as_str() {
            "recipient-window" => 1,
            "target-window" => 2,
            other => {
                return Err(RdpsValidationError::IncompatibleReport(format!(
                    "unknown stack-at-damage context {other}"
                )));
            }
        };
        let mut windows = observation
            .active_windows
            .iter()
            .map(|window| {
                Ok(StatusWindowStackKey {
                    effect_id: parse_report_number("stack_at_damage.effect_id", &window.effect_id)?,
                    instance_id: window
                        .status_instance_id
                        .as_deref()
                        .map(|value| {
                            parse_report_number("stack_at_damage.status_instance_id", value)
                        })
                        .transpose()?,
                    provider_actor: window
                        .provider_actor_id
                        .as_deref()
                        .map(|value| {
                            parse_report_number("stack_at_damage.provider_actor_id", value)
                        })
                        .transpose()?,
                    stacks: window.stacks,
                })
            })
            .collect::<Result<Vec<_>, RdpsValidationError>>()?;
        windows.sort();
        let aggregate = target
            .entry(StackAtDamageKey { context, windows })
            .or_default();
        aggregate.event_count = aggregate
            .event_count
            .saturating_add(observation.event_count);
        aggregate.damage = aggregate.damage.saturating_add(parse_report_number(
            "stack_at_damage.damage",
            &observation.damage,
        )?);
    }
    Ok(())
}

fn damage_evidence_context_name(context: u8) -> &'static str {
    match context {
        0 => "direct-selector",
        1 => "recipient-window",
        2 => "target-window",
        3 => "pre-trigger-buffer",
        _ => "unknown",
    }
}

fn minimum_option(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn maximum_option(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn event_mask(obligation_id: &str, names: &[String]) -> Result<u16, RdpsValidationError> {
    let mut mask = 0;
    for name in names {
        mask |= match name.as_str() {
            "actor" => ACTOR,
            "cast" => CAST,
            "damage" => DAMAGE,
            "status" => STATUS,
            "entity_attributes" => ENTITY_ATTRIBUTES,
            "temporary_attributes" => TEMPORARY_ATTRIBUTES,
            "formula_inputs" => FORMULA_INPUTS,
            "profile_selection" => PROFILE_SELECTION,
            "resource" => RESOURCE,
            "cooldown" => COOLDOWN,
            "healing" => HEALING,
            "shield_state" => SHIELD_STATE,
            _ => {
                return Err(RdpsValidationError::UnknownEventKind {
                    obligation_id: obligation_id.into(),
                    kind: name.clone(),
                });
            }
        };
    }
    Ok(mask)
}

fn mastery_validation_route(
    obligation_id: &str,
    route: &str,
) -> Result<MasteryValidationRoute, RdpsValidationError> {
    let route = match route {
        "outgoing-damage" => MasteryValidationRoute::OutgoingDamage,
        "outgoing-selected-ability-damage" => MasteryValidationRoute::OutgoingSelectedAbilityDamage,
        "owned-companion-outgoing-damage" => MasteryValidationRoute::OwnedCompanionOutgoingDamage,
        "outgoing-healing" => MasteryValidationRoute::OutgoingHealing,
        "outgoing-shield-or-barrier-state" => MasteryValidationRoute::OutgoingShieldOrBarrierState,
        "named-shield-state" => MasteryValidationRoute::NamedShieldState,
        "incoming-damage-mitigation" => MasteryValidationRoute::IncomingDamageMitigation,
        "owned-resource-transition" => MasteryValidationRoute::OwnedResourceTransition,
        "selected-ability-cooldown-transition" => {
            MasteryValidationRoute::SelectedAbilityCooldownTransition
        }
        "named-skill-output" => MasteryValidationRoute::NamedSkillOutput,
        "named-status-lifecycle" => MasteryValidationRoute::NamedStatusLifecycle,
        "named-resource-decay-lifecycle" => MasteryValidationRoute::NamedResourceDecayLifecycle,
        _ => {
            return Err(RdpsValidationError::UnknownValidationRoute {
                obligation_id: obligation_id.into(),
                route: route.into(),
            });
        }
    };
    Ok(route)
}

fn mask_names(mask: u16) -> Vec<String> {
    [
        (ACTOR, "actor"),
        (CAST, "cast"),
        (DAMAGE, "damage"),
        (STATUS, "status"),
        (ENTITY_ATTRIBUTES, "entity_attributes"),
        (TEMPORARY_ATTRIBUTES, "temporary_attributes"),
        (FORMULA_INPUTS, "formula_inputs"),
        (PROFILE_SELECTION, "profile_selection"),
        (RESOURCE, "resource"),
        (COOLDOWN, "cooldown"),
        (HEALING, "healing"),
        (SHIELD_STATE, "shield_state"),
    ]
    .into_iter()
    .filter(|(bit, _)| mask & *bit != 0)
    .map(|(_, name)| name.to_owned())
    .collect()
}

const fn decoder_event_mask(decoder: DecoderKind) -> u16 {
    match decoder {
        DecoderKind::SyncNearEntitiesV1 => {
            ACTOR | ENTITY_ATTRIBUTES | SHIELD_STATE | TEMPORARY_ATTRIBUTES | STATUS
        }
        DecoderKind::SyncNearDeltaV1 | DecoderKind::SyncToMeDeltaV1 => {
            ACTOR
                | ENTITY_ATTRIBUTES
                | SHIELD_STATE
                | TEMPORARY_ATTRIBUTES
                | STATUS
                | DAMAGE
                | HEALING
                | RESOURCE
                | COOLDOWN
        }
        DecoderKind::SyncClientUseSkillV1 | DecoderKind::WorldUseSlotV1 => CAST,
        DecoderKind::NotifyReviveV1
        | DecoderKind::NotifyEnterWorldV1
        | DecoderKind::SyncServerTimeV1
        | DecoderKind::SyncSeasonV1
        | DecoderKind::SyncDungeonDataV1
        | DecoderKind::SyncDungeonDirtyDataV1
        | DecoderKind::EnterSceneV1
        | DecoderKind::NotifyLoadSceneEndV1
        | DecoderKind::NotifySocialDataV1
        | DecoderKind::NotifyUnionInfoV1
        | DecoderKind::NotifyTeamMemberInfoV1
        | DecoderKind::NotifyJoinTeamV1
        | DecoderKind::NotifyLeaveTeamV1
        | DecoderKind::NoticeTeamDissolveV1
        | DecoderKind::GetAlbumPhotosV1
        | DecoderKind::GetPhotoV1 => 0,
        DecoderKind::SyncContainerDataV1 => ACTOR | PROFILE_SELECTION | RESOURCE | COOLDOWN,
        DecoderKind::SyncContainerDirtyDataV1 => PROFILE_SELECTION | RESOURCE | COOLDOWN,
    }
}

fn status_state_name(event: &StatusEvent) -> &'static str {
    use rlogs_events::StatusState;
    match event.state {
        StatusState::Applied => "applied",
        StatusState::Refreshed => "refreshed",
        StatusState::Stacked => "stacked",
        StatusState::Consumed => "consumed",
        StatusState::Removed => "removed",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use prost::Message;
    use rlogs_combat::{
        DamageContributionScope, ExactDamageContributionEvent, ExactRationalDamageContributionEvent,
    };
    use rlogs_events::{
        AbilityId, ActorEvent, ActorId, ActorKind, ActorLoadoutEvidence, ActorLoadoutSlot,
        ActorState, CanonicalEvent, CharacterIdentity, CooldownEvent, EntityAttribute,
        EntityAttributeEvent, EntityAttributeUpdateKind, EntityAttributeValue, EntityRef,
        EntityUuid, EventEnvelope, EventProvenance, EventSensitivity, EventTime, GameProfileEvent,
        HealingEvent, RegionContext, RegionIdentity, ResourceEvent, StatusEffectId,
        StatusEffectInstanceId, StatusOrigin, StatusState, TemporaryAttribute,
        TemporaryAttributeEvent, TimelineEvent, TimelineEventKind,
    };

    use crate::{
        BPSR_GAME_PLUGIN_ID, BPSR_PROFILE_SCHEMA_ID, BPSR_PROFILE_SCHEMA_VERSION,
        CharacterProfilePatch, CultivationAreaProfile, CultivationLineProfile, DecoderKind,
        EquipmentSuitEntryProfile, FragmentKind, MappingConfidence, PROTOCOL_PACK_SCHEMA_VERSION,
        PacketDirection, ProtocolPack, ProtocolPackDefinition, ProtocolPackRoute,
        ProtocolPackRouteDisposition, ProtocolPackTarget, RdpsValidationObservedProviderScope,
        RouteKey, SeasonCultivationProfile, SeasonProfile,
    };

    use super::{
        DreamscopeEvidenceResolution, DreamscopeTerminalEffectState, EffectFingerprintResolution,
        RDPS_VALIDATION_REPORT_SCHEMA_VERSION, RdpsValidationAnalyzer, RdpsValidationError,
        RdpsValidationRemoteScalarResolution, dreamscope_observed_effect_match,
        dreamscope_remote_calculation_readiness,
    };

    const MANIFEST: &str = r#"{
      "schema_version": 2,
      "game_build": "24609362",
      "obligations": [
        {
          "obligation_id": "factor:1", "domain": "factor", "subject_kind": "factor",
          "subject_id": "1", "subject_name": "Example", "requirements": ["lifecycle"],
          "required_event_kinds": ["actor", "cast", "damage", "status"],
          "selectors": {"effect_ids": [9001], "skill_ids": [7001], "item_ids": [5001]}
        },
        {
          "obligation_id": "attribute:2", "domain": "formula", "subject_kind": "attribute",
          "subject_id": "2", "subject_name": "Attribute", "requirements": ["attribute"],
          "required_event_kinds": ["entity_attributes", "temporary_attributes", "damage"],
          "selectors": {"attribute_ids": [116]}
        }
      ]
    }"#;

    const CLASS_AND_ITEM_MANIFEST: &str = r#"{
      "schema_version": 2,
      "game_build": "24609362",
      "obligations": [{
        "obligation_id": "factor:1", "domain": "factor", "subject_kind": "factor",
        "subject_id": "1", "subject_name": "Example", "requirements": ["lifecycle"],
        "required_event_kinds": ["actor", "cast"],
        "selectors": {"class_ids": [12], "item_ids": [5001]}
      }]
    }"#;

    const FORMULA_INPUT_MANIFEST: &str = r#"{
      "schema_version": 2,
      "game_build": "24609362",
      "obligations": [{
        "obligation_id": "packet-output:1", "domain": "packet-output-route",
        "subject_kind": "packet-output-route", "subject_id": "1",
        "subject_name": "HP output", "requirements": ["exact packet output"],
        "required_event_kinds": ["damage", "formula_inputs", "status"],
        "selectors": {"effect_ids": [9001]},
        "formula_inputs": [{
          "input_key": "hp", "label": "current HP", "actor_role": "source",
          "completion": "any-current-value-observed-before-trigger",
          "candidate_attribute_ids": [11310]
        }]
      }]
    }"#;

    const LOADOUT_TIER_INPUT_MANIFEST: &str = r#"{
      "schema_version": 2,
      "game_build": "24609362",
      "obligations": [{
        "obligation_id": "imagine-tier:3971", "domain": "party-effect-runtime-input-frontier",
        "subject_kind": "status-effect", "subject_id": "9001",
        "subject_name": "Imagine tier", "requirements": ["event-time exact provider tier"],
        "required_event_kinds": ["formula_inputs", "status"],
        "selectors": {"effect_ids": [9001]},
        "formula_inputs": [{
          "input_key": "provider-imagine-tier", "label": "provider equipped Imagine tier",
          "input_kind": "loadout_tier", "actor_role": "source",
          "completion": "exact-current-equipped-tier-observed-before-trigger",
          "candidate_ability_ids": [3971], "loadout_scope": "primary",
          "allowed_tiers": [0, 1, 2, 3, 4, 5]
        }]
      }]
    }"#;

    const CLASS_ATTRIBUTE_INPUT_MANIFEST: &str = r#"{
      "schema_version": 2,
      "game_build": "24609362",
      "obligations": [{
        "obligation_id": "recipient-attack:9001", "domain": "party-effect-runtime-input-frontier",
        "subject_kind": "status-effect", "subject_id": "9001",
        "subject_name": "Recipient selected attack", "requirements": ["event-time class-selected attack"],
        "required_event_kinds": ["formula_inputs", "status"],
        "selectors": {"effect_ids": [9001]},
        "formula_inputs": [{
          "input_key": "recipient-attack", "label": "recipient class-selected attack",
          "input_kind": "class_attribute", "actor_role": "target",
          "completion": "exact-current-class-selected-value-observed-before-trigger",
          "class_attribute_routes": [
            {"class_ids": [1, 3, 4, 9, 11, 12], "candidate_attribute_ids": [11330]},
            {"class_ids": [2, 5, 13], "candidate_attribute_ids": [11340]}
          ]
        }]
      }]
    }"#;

    const PROVIDER_IDENTITY_MANIFEST: &str = r#"{
      "schema_version": 2,
      "game_build": "24609362",
      "obligations": [{
        "obligation_id": "provider:1", "domain": "offensive-runtime-gate",
        "subject_kind": "source-rule", "subject_id": "provider:1",
        "subject_name": "Provider identity", "requirements": ["provider identity"],
        "required_event_kinds": ["actor", "status"],
        "selectors": {"source_rule_ids": ["provider:1"], "effect_ids": [9001]}
      }]
    }"#;

    const EQUIPMENT_SUIT_ORIGIN_MANIFEST: &str = r#"{
      "schema_version": 2,
      "game_build": "24609362",
      "obligations": [
        {
          "obligation_id": "equipment:101:464", "domain": "offensive-runtime-gate",
          "subject_kind": "source-rule", "subject_id": "101:464",
          "subject_name": "Set 101 variant 464", "requirements": ["origin"],
          "required_event_kinds": ["profile_selection", "status", "damage"],
          "selectors": {
            "effect_ids": [2407280], "source_config_ids": [464],
            "equipment_suit_entries": [{"map_key": 101, "attribute_key": 464}]
          }
        },
        {
          "obligation_id": "equipment:102:1786", "domain": "offensive-runtime-gate",
          "subject_kind": "source-rule", "subject_id": "102:1786",
          "subject_name": "Set 102 variant 1786", "requirements": ["origin"],
          "required_event_kinds": ["profile_selection", "status", "damage"],
          "selectors": {
            "effect_ids": [2407280], "source_config_ids": [1786],
            "equipment_suit_entries": [{"map_key": 102, "attribute_key": 1786}]
          }
        }
      ]
    }"#;

    const FACTOR_SELECTION_MANIFEST: &str = r#"{
      "schema_version": 2,
      "game_build": "24609362",
      "obligations": [
        {
          "obligation_id": "factor-family:202101", "domain": "psychoscope-factor",
          "subject_kind": "factor-family", "subject_id": "202101",
          "subject_name": "Selected", "requirements": ["selection", "resource"],
          "required_event_kinds": ["profile_selection", "status", "resource"],
          "selectors": {"effect_ids": [9001], "item_ids": [20020001]}
        },
        {
          "obligation_id": "factor-family:202102", "domain": "psychoscope-factor",
          "subject_kind": "factor-family", "subject_id": "202102",
          "subject_name": "Not selected", "requirements": ["selection", "resource"],
          "required_event_kinds": ["profile_selection", "status", "resource"],
          "selectors": {"effect_ids": [9001], "item_ids": [20021001]}
        }
      ]
    }"#;

    const COOLDOWN_FACTOR_MANIFEST: &str = r#"{
      "schema_version": 2,
      "game_build": "24609362",
      "obligations": [
        {
          "obligation_id": "factor-family:202303", "domain": "psychoscope-factor",
          "subject_kind": "factor-family", "subject_id": "202303",
          "subject_name": "Selected cooldown factor",
          "requirements": ["selection", "cooldown-progress-transition"],
          "required_event_kinds": ["profile_selection", "status", "cooldown"],
          "selectors": {"effect_ids": [9001], "item_ids": [20022021]}
        },
        {
          "obligation_id": "factor-family:202327", "domain": "psychoscope-factor",
          "subject_kind": "factor-family", "subject_id": "202327",
          "subject_name": "Unselected cooldown factor",
          "requirements": ["selection", "cooldown-progress-transition"],
          "required_event_kinds": ["profile_selection", "status", "cooldown"],
          "selectors": {"effect_ids": [9001], "item_ids": [20022261]}
        }
      ]
    }"#;

    const DAMAGE_FACTOR_MANIFEST: &str = r#"{
      "schema_version": 2,
      "game_build": "24609362",
      "obligations": [
        {
          "obligation_id": "factor-family:202101", "domain": "psychoscope-factor",
          "subject_kind": "factor-family", "subject_id": "202101",
          "subject_name": "Selected damage factor",
          "requirements": ["selection", "damage-during-status"],
          "required_event_kinds": ["profile_selection", "status", "damage"],
          "selectors": {"effect_ids": [9001], "item_ids": [20020001]}
        },
        {
          "obligation_id": "factor-family:202102", "domain": "psychoscope-factor",
          "subject_kind": "factor-family", "subject_id": "202102",
          "subject_name": "Unselected damage factor",
          "requirements": ["selection", "damage-during-status"],
          "required_event_kinds": ["profile_selection", "status", "damage"],
          "selectors": {"effect_ids": [9001], "item_ids": [20021001]}
        }
      ]
    }"#;

    const TARGET_MITIGATION_MANIFEST: &str = r#"{
      "schema_version": 2,
      "game_build": "24609362",
      "obligations": [{
        "obligation_id": "target-mitigation:armor", "domain": "target-mitigation",
        "subject_kind": "formula-model", "subject_id": "armor",
        "subject_name": "Target armor", "requirements": ["target counterfactual"],
        "required_event_kinds": ["damage", "entity_attributes", "temporary_attributes"],
        "selectors": {"attribute_ids": [116]}
      }]
    }"#;

    fn capability_pack(build: &str, decoders: &[DecoderKind]) -> ProtocolPack {
        ProtocolPack::build(ProtocolPackDefinition {
            schema_version: PROTOCOL_PACK_SCHEMA_VERSION,
            pack_id: format!("capability-{build}-{}", decoders.len()),
            target: ProtocolPackTarget {
                deployment_id: "global".into(),
                region_id: None,
                channel: "steam".into(),
                build_id: build.into(),
                executable_version: None,
            },
            acquisition: Default::default(),
            provenance: Vec::new(),
            routes: decoders
                .iter()
                .copied()
                .enumerate()
                .map(|(index, decoder)| ProtocolPackRoute {
                    route: RouteKey::new(
                        PacketDirection::ServerToClient,
                        FragmentKind::Notify,
                        1,
                        u32::try_from(index + 1).unwrap(),
                    ),
                    service_name: "TestNtf".into(),
                    method_name: format!("method-{index}"),
                    message_name: None,
                    confidence: MappingConfidence::Verified,
                    provenance: Vec::new(),
                    features: Vec::new(),
                    disposition: ProtocolPackRouteDisposition::Allowed {
                        domain: decoder.domain(),
                        decoder,
                    },
                })
                .collect(),
        })
        .unwrap()
    }

    fn actor() -> EntityRef {
        EntityRef {
            actor_id: ActorId(7),
            entity_uuid: EntityUuid(70),
        }
    }

    fn envelope(build: &str, sequence: u64, kind: TimelineEventKind) -> EventEnvelope {
        let time = EventTime {
            observed_micros: sequence,
            game_time_millis: None,
        };
        let provenance = EventProvenance::manual("test");
        EventEnvelope {
            schema_version: rlogs_events::EVENT_SCHEMA_VERSION,
            session_id: "test".into(),
            sequence,
            region: RegionContext {
                identity: RegionIdentity {
                    deployment_id: "global".into(),
                    region_id: "global".into(),
                    realm_id: None,
                    world_id: None,
                },
                client_build: build.into(),
                protocol_pack_digest: "test".into(),
                evidence: Vec::new(),
            },
            time,
            provenance: provenance.clone(),
            sensitivity: EventSensitivity::PublicGameplay,
            event: CanonicalEvent::Timeline(TimelineEvent {
                sequence,
                time,
                provenance,
                kind,
            }),
        }
    }

    fn envelope_at(
        build: &str,
        sequence: u64,
        observed_micros: u64,
        kind: TimelineEventKind,
    ) -> EventEnvelope {
        let mut envelope = envelope(build, sequence, kind);
        envelope.time.observed_micros = observed_micros;
        let CanonicalEvent::Timeline(timeline) = &mut envelope.event else {
            unreachable!();
        };
        timeline.time.observed_micros = observed_micros;
        envelope
    }

    fn profile_envelope(sequence: u64, character_id: &str, item_id: i64) -> EventEnvelope {
        let identity = CharacterIdentity {
            region: RegionIdentity {
                deployment_id: "global".into(),
                region_id: "global".into(),
                realm_id: None,
                world_id: None,
            },
            character_id: character_id.into(),
        };
        let profile = CharacterProfilePatch {
            character: identity.clone(),
            display_name: None,
            display_id: None,
            server_id: None,
            class_id: None,
            specialization_id: None,
            level: None,
            progression: None,
            combat_power: None,
            combat_power_breakdown: None,
            season_strength: None,
            master_score: None,
            season: Some(SeasonProfile {
                season_id: Some(3),
                level: None,
                experience: None,
                power: None,
                strength: None,
            }),
            appearance: None,
            equipment: None,
            equipment_suit_entries: None,
            modules: None,
            owned_imagines: None,
            battle_imagine_skills: None,
            equipped_action_slots: None,
            active_skills: None,
            talents: None,
            talent_progress: None,
            combat_professions: None,
            life_professions: None,
            cosmetics: None,
            collection_summary: None,
            activity_progress: None,
            season_medals: None,
            season_cultivation: Some(vec![SeasonCultivationProfile {
                season_id: 3,
                lines: vec![CultivationLineProfile {
                    line_type_id: 1,
                    area_ids: vec![1],
                    areas: vec![CultivationAreaProfile {
                        area_id: 1,
                        active: Some(true),
                        active_effect_score: None,
                        normal_node_levels: BTreeMap::new(),
                        middle_node_item_ids: BTreeMap::from([(1, item_id)]),
                        big_node_fantasy_ids: BTreeMap::new(),
                    }],
                }],
            }]),
            reputations: None,
            current_profession_project_id: None,
            social_display: None,
        };
        let time = EventTime {
            observed_micros: sequence,
            game_time_millis: None,
        };
        EventEnvelope {
            schema_version: 6,
            session_id: "test".into(),
            sequence,
            region: RegionContext {
                identity: identity.region.clone(),
                client_build: "24609362".into(),
                protocol_pack_digest: "test".into(),
                evidence: Vec::new(),
            },
            time,
            provenance: EventProvenance::manual("test"),
            sensitivity: EventSensitivity::PersonalGameplay,
            event: CanonicalEvent::CharacterProfileObserved {
                profile: Box::new(GameProfileEvent {
                    game_plugin_id: BPSR_GAME_PLUGIN_ID.into(),
                    payload_schema_id: BPSR_PROFILE_SCHEMA_ID.into(),
                    payload_schema_version: BPSR_PROFILE_SCHEMA_VERSION,
                    character: identity,
                    payload: serde_json::to_value(profile).unwrap(),
                }),
            },
        }
    }

    fn equipment_suit_profile_envelope(
        sequence: u64,
        character_id: &str,
        map_key: i32,
        attribute_key: i32,
    ) -> EventEnvelope {
        let mut envelope = profile_envelope(sequence, character_id, 1);
        let CanonicalEvent::CharacterProfileObserved { profile } = &mut envelope.event else {
            unreachable!();
        };
        let mut patch = CharacterProfilePatch::from_game_event(profile).unwrap();
        patch.season_cultivation = None;
        patch.equipment_suit_entries = Some(vec![EquipmentSuitEntryProfile {
            map_key,
            attribute_type: Some(2),
            attributes: BTreeMap::from([(attribute_key, 1)]),
        }]);
        **profile = patch.into_game_event().unwrap();
        envelope
    }

    fn equipment_status_event(
        source_actor: u64,
        target_actor: u64,
        source_config_id: i64,
    ) -> TimelineEventKind {
        TimelineEventKind::Status(rlogs_events::StatusEvent {
            source: Some(entity(source_actor)),
            target: entity(target_actor),
            effect: StatusEffectId(2_407_280),
            instance_id: Some(StatusEffectInstanceId(44)),
            origin: Some(StatusOrigin {
                source_type_id: 10,
                source_config_id,
            }),
            state: StatusState::Applied,
            stacks: Some(1),
            duration_millis: Some(10_000),
            level: None,
            part_id: None,
            count: None,
            created_at_millis: None,
        })
    }

    fn actor_event(class_id: Option<i32>, item_id: Option<i64>) -> ActorEvent {
        let primary_loadout = item_id
            .map(|item_id| ActorLoadoutSlot {
                slot_id: 1,
                ability_id: None,
                item_id: Some(item_id),
                tier: Some(1),
            })
            .into_iter()
            .collect();
        ActorEvent {
            actor: actor(),
            state: ActorState::Updated,
            entity_type_id: 1,
            kind: ActorKind::Player,
            character_id: None,
            monster_id: None,
            display_name: None,
            class_id,
            specialization_id: None,
            level: None,
            ability_score: None,
            weapon_item_id: None,
            weapon_breakthrough_count: None,
            seasonal_score: None,
            primary_loadout,
            auxiliary_loadout: Vec::new(),
            loadout_observation: rlogs_events::ActorLoadoutObservation {
                primary: if item_id.is_some() {
                    ActorLoadoutEvidence::ExactSlots
                } else {
                    ActorLoadoutEvidence::Unobserved
                },
                auxiliary: ActorLoadoutEvidence::Unobserved,
            },
        }
    }

    fn cast_event(source: EntityRef, ability: i64) -> TimelineEventKind {
        TimelineEventKind::Cast(rlogs_events::CastEvent {
            source,
            ability: AbilityId(ability),
            target: None,
            state: rlogs_events::CastState::Started,
            action_timing: None,
        })
    }

    fn cooldown_event(
        source: EntityRef,
        ability: i64,
        begin_time_millis: i64,
    ) -> TimelineEventKind {
        TimelineEventKind::Cooldown(CooldownEvent {
            actor: source,
            ability: AbilityId(ability),
            begin_time_millis: Some(begin_time_millis),
            duration_millis: Some(10_000),
            valid_duration_millis: Some(10_000),
            cooldown_type: Some(1),
            profession_hold_begin_time_millis: None,
            charge_count: Some(0),
            valid_cooldown_time_millis: Some(10_000),
            sub_cooldown_ratio_raw: None,
            sub_cooldown_fixed_raw: None,
            accelerate_cooldown_ratio_raw: None,
        })
    }

    fn entity(actor_id: u64) -> EntityRef {
        EntityRef {
            actor_id: ActorId(actor_id),
            entity_uuid: EntityUuid(i64::try_from(actor_id * 10).unwrap()),
        }
    }

    fn status_event(
        source_actor: u64,
        target_actor: u64,
        instance_id: Option<i64>,
        state: StatusState,
        stacks: Option<u32>,
    ) -> TimelineEventKind {
        status_event_for_effect(source_actor, target_actor, 9001, instance_id, state, stacks)
    }

    fn status_event_for_effect(
        source_actor: u64,
        target_actor: u64,
        effect_id: i64,
        instance_id: Option<i64>,
        state: StatusState,
        stacks: Option<u32>,
    ) -> TimelineEventKind {
        TimelineEventKind::Status(rlogs_events::StatusEvent {
            source: Some(entity(source_actor)),
            target: entity(target_actor),
            effect: StatusEffectId(effect_id),
            instance_id: instance_id.map(StatusEffectInstanceId),
            origin: None,
            state,
            stacks,
            duration_millis: None,
            level: None,
            part_id: None,
            count: None,
            created_at_millis: None,
        })
    }

    fn status_event_for_effect_with_origin(
        source_actor: u64,
        target_actor: u64,
        effect_id: i64,
        source_type_id: i32,
        source_config_id: i64,
        state: StatusState,
    ) -> TimelineEventKind {
        let is_active = matches!(
            state,
            StatusState::Applied | StatusState::Refreshed | StatusState::Stacked
        );
        TimelineEventKind::Status(rlogs_events::StatusEvent {
            source: Some(entity(source_actor)),
            target: entity(target_actor),
            effect: StatusEffectId(effect_id),
            instance_id: None,
            origin: Some(StatusOrigin {
                source_type_id,
                source_config_id,
            }),
            state,
            stacks: Some(u32::from(is_active)),
            duration_millis: is_active.then_some(5_000),
            level: is_active.then_some(1),
            part_id: None,
            count: is_active.then_some(-1),
            created_at_millis: None,
        })
    }

    fn damage_event(source_actor: u64, target_actor: u64, amount: i64) -> TimelineEventKind {
        TimelineEventKind::Damage(rlogs_events::DamageEvent {
            source: entity(source_actor),
            direct_source: None,
            target: entity(target_actor),
            ability: None,
            amount,
            actual_amount: None,
            hp_loss: None,
            shield_loss: None,
            hit_event_id: None,
            damage_source: None,
            damage_type: None,
            flags: Default::default(),
            packet: Default::default(),
        })
    }

    fn healing_event(source_actor: u64, target_actor: u64, amount: i64) -> TimelineEventKind {
        TimelineEventKind::Healing(HealingEvent {
            source: entity(source_actor),
            direct_source: None,
            target: entity(target_actor),
            ability: None,
            amount,
            actual_amount: Some(amount),
            hp_loss: None,
            shield_loss: None,
            hit_event_id: None,
            damage_source: None,
            damage_type: Some(2),
            effective_amount: Some(amount),
            overheal: Some(0),
            critical: Some(false),
            periodic: Some(false),
            packet: Default::default(),
        })
    }

    fn shield_attribute_event(
        actor_id: u64,
        update_kind: EntityAttributeUpdateKind,
        current_value: i64,
    ) -> TimelineEventKind {
        let raw_value = crate::game_schema_v1::AttrShieldList {
            shields: vec![crate::game_schema_v1::AttrShieldInfo {
                uuid: Some(44),
                shield_type: Some(1),
                current_value: Some(current_value),
                initial_value: Some(100),
                max_value: Some(100),
            }],
        }
        .encode_to_vec();
        TimelineEventKind::EntityAttributes(EntityAttributeEvent {
            actor: entity(actor_id),
            update_kind,
            ownership: None,
            attributes: vec![EntityAttribute {
                attribute_id: 60_050,
                raw_value,
                decoded: None,
            }],
        })
    }

    #[test]
    fn child_status_origin_tuple_is_retained_without_replacing_effect_identity() {
        const ORIGIN_MANIFEST: &str = r#"{
          "schema_version": 2,
          "game_build": "24609362",
          "obligations": [{
            "obligation_id": "origin:2203410", "domain": "runtime-candidate-correlation",
            "subject_kind": "source-rule", "subject_id": "talent:1141",
            "subject_name": "Light and Shadow Drain", "requirements": ["exact origin"],
            "required_event_kinds": ["status"],
            "selectors": {"effect_ids": [2203410]}
          }]
        }"#;
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(ORIGIN_MANIFEST).unwrap();
        analyzer.observe(&envelope(
            "24609362",
            1,
            status_event_for_effect_with_origin(
                486,
                512,
                2_203_141,
                1,
                2_203_410,
                StatusState::Applied,
            ),
        ));

        let report = analyzer.report();
        let obligation = &report.obligations[0];
        assert_eq!(
            obligation.provider_recipient_observations[0].effect_id,
            "2203141"
        );
        let origin = &obligation.status_origin_observations[0];
        assert_eq!(origin.provider_actor_id.as_deref(), Some("486"));
        assert_eq!(origin.recipient_actor_id, "512");
        assert_eq!(origin.effect_id, "2203141");
        assert_eq!(origin.origin_source_type_id, 1);
        assert_eq!(origin.origin_source_config_id, "2203410");
        assert_eq!(origin.observation_count, 1);

        let mut cumulative = RdpsValidationAnalyzer::from_manifest_json(ORIGIN_MANIFEST).unwrap();
        cumulative.merge_report(&report).unwrap();
        assert_eq!(
            cumulative.report().obligations[0].status_origin_observations[0].observation_count,
            1
        );
    }

    #[test]
    fn selected_item_activates_only_its_actor_context() {
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        analyzer.observe(&envelope(
            "24609362",
            1,
            TimelineEventKind::Actor(ActorEvent {
                actor: actor(),
                state: ActorState::Updated,
                entity_type_id: 1,
                kind: ActorKind::Player,
                character_id: None,
                monster_id: None,
                display_name: None,
                class_id: None,
                specialization_id: None,
                level: None,
                ability_score: None,
                weapon_item_id: None,
                weapon_breakthrough_count: None,
                seasonal_score: None,
                primary_loadout: vec![ActorLoadoutSlot {
                    slot_id: 1,
                    ability_id: None,
                    item_id: Some(5001),
                    tier: Some(1),
                }],
                auxiliary_loadout: Vec::new(),
                loadout_observation: rlogs_events::ActorLoadoutObservation {
                    primary: ActorLoadoutEvidence::ExactSlots,
                    auxiliary: ActorLoadoutEvidence::Unobserved,
                },
            }),
        ));
        analyzer.observe(&envelope(
            "24609362",
            2,
            TimelineEventKind::Cast(rlogs_events::CastEvent {
                source: actor(),
                ability: AbilityId(1),
                target: None,
                state: rlogs_events::CastState::Started,
                action_timing: None,
            }),
        ));
        analyzer.observe(&envelope(
            "24609362",
            3,
            TimelineEventKind::Status(rlogs_events::StatusEvent {
                source: Some(actor()),
                target: actor(),
                effect: StatusEffectId(9001),
                instance_id: None,
                origin: Some(StatusOrigin {
                    source_type_id: 0,
                    source_config_id: 7001,
                }),
                state: StatusState::Applied,
                stacks: Some(1),
                duration_millis: None,
                level: None,
                part_id: None,
                count: None,
                created_at_millis: None,
            }),
        ));
        analyzer.observe(&envelope(
            "24609362",
            4,
            TimelineEventKind::Damage(rlogs_events::DamageEvent {
                source: actor(),
                direct_source: None,
                target: EntityRef {
                    actor_id: ActorId(8),
                    entity_uuid: EntityUuid(80),
                },
                ability: Some(AbilityId(7001)),
                amount: 10,
                actual_amount: None,
                hp_loss: None,
                shield_loss: None,
                hit_event_id: None,
                damage_source: None,
                damage_type: None,
                flags: Default::default(),
                packet: Default::default(),
            }),
        ));

        let report = analyzer.report();
        assert_eq!(report.summary.candidate_event_coverage_complete, 1);
        assert_eq!(report.summary.no_candidate_evidence, 1);
        assert_eq!(report.summary.proof_promotions, 0);
    }

    #[test]
    fn unordered_loadout_observation_cannot_replace_exact_slots() {
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        analyzer.observe(&envelope(
            "24609362",
            1,
            TimelineEventKind::Actor(actor_event(None, Some(5001))),
        ));

        let mut observed_set = actor_event(None, Some(9999));
        observed_set.loadout_observation.primary = ActorLoadoutEvidence::ObservedSet;
        analyzer.observe(&envelope(
            "24609362",
            2,
            TimelineEventKind::Actor(observed_set),
        ));

        let state = analyzer.actor_selection_state.get(&7).unwrap();
        assert_eq!(state.primary_exact.as_ref().unwrap()[0].item_id, Some(5001));
        assert_eq!(
            state.primary_observed_set.as_ref().unwrap()[0].item_id,
            Some(9999)
        );
        assert_eq!(
            state.selected_slots().next().and_then(|slot| slot.item_id),
            Some(5001)
        );
    }

    #[test]
    fn exact_empty_loadout_clears_stale_selected_item_context() {
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        analyzer.observe(&envelope(
            "24609362",
            1,
            TimelineEventKind::Actor(actor_event(None, Some(5001))),
        ));

        let mut exact_empty = actor_event(None, None);
        exact_empty.loadout_observation.primary = ActorLoadoutEvidence::ExactSlots;
        analyzer.observe(&envelope(
            "24609362",
            2,
            TimelineEventKind::Actor(exact_empty),
        ));

        let state = analyzer.actor_selection_state.get(&7).unwrap();
        assert!(state.primary_exact.as_ref().unwrap().is_empty());
        assert!(state.primary_observed_set.is_none());
        assert!(state.selected_slots().next().is_none());
        assert!(
            analyzer
                .actor_selection_active
                .get(&7)
                .is_some_and(BTreeSet::is_empty)
        );
    }

    #[test]
    fn unobserved_loadout_payload_cannot_mutate_exact_slots() {
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        analyzer.observe(&envelope(
            "24609362",
            1,
            TimelineEventKind::Actor(actor_event(None, Some(5001))),
        ));

        let mut unobserved = actor_event(None, Some(9999));
        unobserved.loadout_observation.primary = ActorLoadoutEvidence::Unobserved;
        analyzer.observe(&envelope(
            "24609362",
            2,
            TimelineEventKind::Actor(unobserved),
        ));

        let state = analyzer.actor_selection_state.get(&7).unwrap();
        assert_eq!(
            state.selected_slots().next().and_then(|slot| slot.item_id),
            Some(5001)
        );
        assert!(state.primary_observed_set.is_none());
    }

    #[test]
    fn later_exact_slot_snapshot_replaces_tier_at_its_event_time() {
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        analyzer.observe(&envelope(
            "24609362",
            1,
            TimelineEventKind::Actor(actor_event(None, Some(5001))),
        ));

        let mut tier_four = actor_event(None, Some(5001));
        tier_four.primary_loadout[0].tier = Some(4);
        analyzer.observe(&envelope(
            "24609362",
            9,
            TimelineEventKind::Actor(tier_four),
        ));

        let state = analyzer.actor_selection_state.get(&7).unwrap();
        assert_eq!(state.primary_exact.as_ref().unwrap()[0].tier, Some(4));
        assert_eq!(state.last_sequence, 9);
        assert_eq!(state.last_observed_micros, 9);
    }

    #[test]
    fn shared_equipment_effect_requires_the_exact_suit_pair_and_origin() {
        let mut analyzer =
            RdpsValidationAnalyzer::from_manifest_json(EQUIPMENT_SUIT_ORIGIN_MANIFEST).unwrap();

        // The outer set key alone is insufficient: the exact (101, 464) pair
        // is required, so this crossed pair activates neither source rule.
        analyzer.observe(&equipment_suit_profile_envelope(1, "7", 101, 1786));
        analyzer.observe(&envelope("24609362", 2, equipment_status_event(7, 8, 464)));
        analyzer.observe(&envelope("24609362", 3, damage_event(8, 9, 100)));
        let report = analyzer.report();
        assert_eq!(report.summary.candidate_event_coverage_complete, 0);

        analyzer.begin_session();
        analyzer.observe(&equipment_suit_profile_envelope(4, "7", 101, 464));
        analyzer.observe(&envelope(
            "24609362",
            5,
            TimelineEventKind::Actor(ActorEvent {
                actor: entity(7),
                ..actor_event(None, None)
            }),
        ));
        analyzer.observe(&envelope("24609362", 6, equipment_status_event(7, 8, 464)));
        analyzer.observe(&envelope("24609362", 7, damage_event(8, 9, 100)));

        let report = analyzer.report();
        let first = report
            .obligations
            .iter()
            .find(|row| row.obligation_id == "equipment:101:464")
            .unwrap();
        let second = report
            .obligations
            .iter()
            .find(|row| row.obligation_id == "equipment:102:1786")
            .unwrap();
        assert_eq!(first.coverage_state, "candidate-event-coverage-complete");
        assert_ne!(second.coverage_state, "candidate-event-coverage-complete");
        assert!(second.observed_event_kinds.is_empty());
        assert!(
            first
                .matched_identifiers
                .iter()
                .any(|value| value == "equipment_suit:101:464")
        );
        assert!(
            first
                .matched_identifiers
                .iter()
                .any(|value| value == "source_config:464")
        );
    }

    #[test]
    fn provider_actor_identity_is_correlated_across_later_status_events() {
        let mut analyzer =
            RdpsValidationAnalyzer::from_manifest_json(PROVIDER_IDENTITY_MANIFEST).unwrap();
        analyzer.observe(&envelope(
            "24609362",
            1,
            TimelineEventKind::Actor(ActorEvent {
                actor: entity(7),
                ..actor_event(None, None)
            }),
        ));
        analyzer.observe(&envelope(
            "24609362",
            2,
            status_event(7, 8, Some(44), StatusState::Applied, Some(1)),
        ));

        let report = analyzer.report();
        assert_eq!(
            report.obligations[0].coverage_state,
            "candidate-event-coverage-complete"
        );
        assert!(
            report.obligations[0]
                .matched_identifiers
                .iter()
                .any(|value| value == "actor_identity:7")
        );
        assert_eq!(report.obligations[0].first_sequence, Some(1));
        assert_eq!(report.obligations[0].last_sequence, Some(2));
        assert_eq!(report.obligations[0].contextual_matches, 1);
    }

    #[test]
    fn shared_effect_projection_is_recorded_only_for_the_proven_provider_origin() {
        let mut analyzer =
            RdpsValidationAnalyzer::from_manifest_json(EQUIPMENT_SUIT_ORIGIN_MANIFEST).unwrap();
        analyzer.observe(&equipment_suit_profile_envelope(1, "7", 101, 464));
        analyzer.observe(&envelope(
            "24609362",
            2,
            TimelineEventKind::Actor(ActorEvent {
                actor: entity(7),
                ..actor_event(None, None)
            }),
        ));
        analyzer.observe(&envelope("24609362", 3, equipment_status_event(7, 8, 464)));
        analyzer.observe_projected_contributions(
            4,
            &[ExactDamageContributionEvent {
                observed_micros: 4,
                effect_id: 2_407_280,
                provider_actor_id: 7,
                recipient_actor_id: 8,
                scope: DamageContributionScope::CompleteEffect,
                amount: 5,
                observed_damage: 100,
                included: true,
            }],
            &[],
            "candidate",
        );
        let report = analyzer.report();
        let first = report
            .obligations
            .iter()
            .find(|row| row.obligation_id == "equipment:101:464")
            .unwrap();
        let second = report
            .obligations
            .iter()
            .find(|row| row.obligation_id == "equipment:102:1786")
            .unwrap();
        assert_eq!(first.projected_integer_events, 1);
        assert_eq!(second.projected_integer_events, 0);
    }

    #[test]
    fn profile_selected_factor_and_changed_resource_are_correlated_without_shared_id_bleed() {
        let mut analyzer =
            RdpsValidationAnalyzer::from_manifest_json(FACTOR_SELECTION_MANIFEST).unwrap();
        let character_id = "3296036";
        let actor = EntityRef {
            actor_id: ActorId(7),
            entity_uuid: EntityUuid((3_296_036_i64 << 16) | 7),
        };
        analyzer.observe(&profile_envelope(1, character_id, 20_020_001));
        analyzer.observe(&envelope(
            "24609362",
            2,
            TimelineEventKind::Actor(ActorEvent {
                actor,
                state: ActorState::Updated,
                entity_type_id: 1,
                kind: ActorKind::Player,
                character_id: Some(character_id.into()),
                monster_id: None,
                display_name: None,
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
            }),
        ));
        analyzer.observe(&envelope(
            "24609362",
            3,
            TimelineEventKind::Resource(ResourceEvent {
                actor,
                update_kind: EntityAttributeUpdateKind::Snapshot,
                origin_energy_raw_bits: Some(0_f32.to_bits()),
                resource_ids: vec![1],
                resource_values: vec![0],
                cooldowns: Vec::new(),
            }),
        ));
        analyzer.observe(&envelope(
            "24609362",
            4,
            TimelineEventKind::Status(rlogs_events::StatusEvent {
                source: Some(actor),
                target: actor,
                effect: StatusEffectId(9001),
                instance_id: Some(StatusEffectInstanceId(91)),
                origin: None,
                state: StatusState::Applied,
                stacks: Some(1),
                duration_millis: None,
                level: None,
                part_id: None,
                count: None,
                created_at_millis: None,
            }),
        ));
        analyzer.observe(&envelope(
            "24609362",
            5,
            TimelineEventKind::Resource(ResourceEvent {
                actor,
                update_kind: EntityAttributeUpdateKind::Delta,
                origin_energy_raw_bits: Some(1_f32.to_bits()),
                resource_ids: vec![1],
                resource_values: vec![1],
                cooldowns: Vec::new(),
            }),
        ));

        let report = analyzer.report();
        assert_eq!(report.summary.candidate_event_coverage_complete, 1);
        assert_eq!(report.summary.no_candidate_evidence, 1);
        assert_eq!(
            report.obligations[0].observed_event_kinds,
            vec!["status", "profile_selection", "resource"]
        );
        assert_eq!(report.obligations[1].direct_matches, 0);
        assert_eq!(report.obligations[1].contextual_matches, 0);
    }

    #[test]
    fn selected_factor_cooldown_requires_changed_state_inside_its_source_status() {
        let mut analyzer =
            RdpsValidationAnalyzer::from_manifest_json(COOLDOWN_FACTOR_MANIFEST).unwrap();
        let character_id = "3296036";
        let actor = EntityRef {
            actor_id: ActorId(7),
            entity_uuid: EntityUuid((3_296_036_i64 << 16) | 7),
        };
        analyzer.observe(&profile_envelope(1, character_id, 20_022_021));
        analyzer.observe(&envelope(
            "24609362",
            2,
            TimelineEventKind::Actor(ActorEvent {
                actor,
                state: ActorState::Updated,
                entity_type_id: 1,
                kind: ActorKind::Player,
                character_id: Some(character_id.into()),
                monster_id: None,
                display_name: None,
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
            }),
        ));
        // The first cooldown packet is only a baseline and cannot satisfy a
        // progress-transition requirement.
        analyzer.observe(&envelope("24609362", 3, cooldown_event(actor, 7001, 100)));
        // Nor can a transition outside the factor's exact status window.
        analyzer.observe(&envelope("24609362", 4, cooldown_event(actor, 7001, 200)));
        analyzer.observe(&envelope(
            "24609362",
            5,
            TimelineEventKind::Status(rlogs_events::StatusEvent {
                source: Some(actor),
                target: actor,
                effect: StatusEffectId(9001),
                instance_id: Some(StatusEffectInstanceId(91)),
                origin: None,
                state: StatusState::Applied,
                stacks: Some(1),
                duration_millis: None,
                level: None,
                part_id: None,
                count: None,
                created_at_millis: None,
            }),
        ));
        analyzer.observe(&envelope("24609362", 6, cooldown_event(actor, 7001, 300)));

        let report = analyzer.report();
        assert_eq!(report.summary.candidate_event_coverage_complete, 1);
        assert_eq!(report.summary.no_candidate_evidence, 1);
        assert_eq!(
            report.obligations[0].observed_event_kinds,
            vec!["status", "profile_selection", "cooldown"]
        );
        assert!(
            report.obligations[0]
                .matched_identifiers
                .contains(&"cooldown_transition:7001:during-source-status".into())
        );
        assert_eq!(report.obligations[1].direct_matches, 0);
        assert_eq!(report.obligations[1].contextual_matches, 0);
    }

    #[test]
    fn selected_factor_damage_is_correlated_through_its_exact_active_status() {
        let mut analyzer =
            RdpsValidationAnalyzer::from_manifest_json(DAMAGE_FACTOR_MANIFEST).unwrap();
        let character_id = "3296036";
        let provider = EntityRef {
            actor_id: ActorId(7),
            entity_uuid: EntityUuid((3_296_036_i64 << 16) | 7),
        };
        analyzer.observe(&profile_envelope(1, character_id, 20_020_001));
        analyzer.observe(&envelope(
            "24609362",
            2,
            TimelineEventKind::Actor(ActorEvent {
                actor: provider,
                ..actor_event(None, None)
            }),
        ));
        analyzer.observe(&envelope(
            "24609362",
            3,
            status_event(7, 8, Some(91), StatusState::Applied, Some(1)),
        ));
        analyzer.observe(&envelope("24609362", 4, damage_event(8, 9, 125)));

        let report = analyzer.report();
        assert_eq!(report.summary.candidate_event_coverage_complete, 1);
        assert_eq!(report.summary.no_candidate_evidence, 1);
        assert_eq!(
            report.obligations[0].observed_event_kinds,
            vec!["damage", "status", "profile_selection"]
        );
        assert!(
            report.obligations[0]
                .matched_identifiers
                .contains(&"damage:during-selected-factor-status".into())
        );
        assert!(report.obligations[1].observed_event_kinds.is_empty());
    }

    #[test]
    fn class_supports_evidence_but_does_not_select_an_item_factor() {
        let mut analyzer =
            RdpsValidationAnalyzer::from_manifest_json(CLASS_AND_ITEM_MANIFEST).unwrap();
        analyzer.observe(&envelope(
            "24609362",
            1,
            TimelineEventKind::Actor(actor_event(Some(12), None)),
        ));
        analyzer.observe(&envelope("24609362", 2, cast_event(actor(), 1)));

        let report = analyzer.report();
        assert_eq!(report.summary.partial_candidate_event_coverage, 1);
        assert_eq!(report.obligations[0].contextual_matches, 0);
        assert_eq!(report.obligations[0].observed_event_kinds, vec!["actor"]);
    }

    #[test]
    fn authoritative_loadout_replacement_removes_the_old_factor_context() {
        let mut analyzer =
            RdpsValidationAnalyzer::from_manifest_json(CLASS_AND_ITEM_MANIFEST).unwrap();
        analyzer.observe(&envelope(
            "24609362",
            1,
            TimelineEventKind::Actor(actor_event(Some(12), Some(5001))),
        ));
        analyzer.observe(&envelope(
            "24609362",
            2,
            TimelineEventKind::Actor(actor_event(Some(12), Some(9999))),
        ));
        analyzer.observe(&envelope("24609362", 3, cast_event(actor(), 1)));

        let report = analyzer.report();
        assert_eq!(report.summary.partial_candidate_event_coverage, 1);
        assert_eq!(report.obligations[0].contextual_matches, 0);
    }

    #[test]
    fn class_and_specialization_selectors_must_match_the_same_actor_snapshot() {
        let manifest = r#"{
          "schema_version": 2,
          "game_build": "24609362",
          "obligations": [{
            "obligation_id": "mastery:101:0", "domain": "mastery-property",
            "subject_kind": "specialization-component", "subject_id": "mastery:101:0",
            "subject_name": "Example mastery", "requirements": ["specialization"],
            "required_event_kinds": ["actor", "damage", "entity_attributes"],
            "selectors": {
              "attribute_ids": [11140], "class_ids": [1], "specialization_ids": [101]
            },
            "evidence": {"component_kind": "skill-filtered-damage", "validation_route": "outgoing-damage"}
          }]
        }"#;
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(manifest).unwrap();
        analyzer.observe(&envelope(
            "24609362",
            1,
            TimelineEventKind::Actor(ActorEvent {
                specialization_id: Some(999),
                ..actor_event(Some(1), None)
            }),
        ));
        analyzer.observe(&envelope(
            "24609362",
            2,
            entity_attribute_event(7, EntityAttributeUpdateKind::Snapshot, &[(11140, 5000)]),
        ));
        analyzer.observe(&envelope("24609362", 3, damage_event(7, 8, 100)));
        assert_eq!(
            analyzer.report().obligations[0].observed_event_kinds,
            vec!["actor"]
        );

        analyzer.observe(&envelope(
            "24609362",
            4,
            TimelineEventKind::Actor(ActorEvent {
                specialization_id: Some(101),
                ..actor_event(Some(1), None)
            }),
        ));
        analyzer.observe(&envelope(
            "24609362",
            5,
            entity_attribute_event(7, EntityAttributeUpdateKind::Snapshot, &[(11140, 5100)]),
        ));
        analyzer.observe(&envelope("24609362", 6, damage_event(7, 8, 100)));
        assert_eq!(
            analyzer.report().obligations[0].coverage_state,
            "candidate-event-coverage-complete"
        );
    }

    #[test]
    fn mastery_healing_is_observed_as_healing_without_fabricating_damage() {
        let manifest = r#"{
          "schema_version": 2,
          "game_build": "24609362",
          "obligations": [{
            "obligation_id": "mastery:111:0", "domain": "mastery-property",
            "subject_kind": "specialization-component", "subject_id": "mastery:111:0",
            "subject_name": "Healing mastery", "requirements": ["specialization"],
            "required_event_kinds": ["actor", "entity_attributes", "healing"],
            "selectors": {
              "attribute_ids": [11140], "class_ids": [11], "specialization_ids": [111]
            },
            "evidence": {"component_kind": "healing", "validation_route": "outgoing-healing"}
          }]
        }"#;
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(manifest).unwrap();
        analyzer.observe(&envelope(
            "24609362",
            1,
            TimelineEventKind::Actor(ActorEvent {
                specialization_id: Some(111),
                ..actor_event(Some(11), None)
            }),
        ));
        analyzer.observe(&envelope(
            "24609362",
            2,
            entity_attribute_event(7, EntityAttributeUpdateKind::Snapshot, &[(11140, 5000)]),
        ));
        analyzer.observe(&envelope("24609362", 3, healing_event(7, 8, 250)));

        let obligation = &analyzer.report().obligations[0];
        assert_eq!(
            obligation.coverage_state,
            "candidate-event-coverage-complete"
        );
        assert_eq!(
            obligation.observed_event_kinds,
            vec!["actor", "entity_attributes", "healing"]
        );
        assert!(!obligation.observed_event_kinds.contains(&"damage".into()));
    }

    #[test]
    fn shield_attribute_requires_a_changed_state_after_its_baseline() {
        let manifest = r#"{
          "schema_version": 2,
          "game_build": "24609362",
          "obligations": [{
            "obligation_id": "mastery:110:0", "domain": "mastery-property",
            "subject_kind": "specialization-component", "subject_id": "mastery:110:0",
            "subject_name": "Shield mastery", "requirements": ["specialization"],
            "required_event_kinds": ["actor", "entity_attributes", "shield_state"],
            "selectors": {
              "attribute_ids": [60050], "class_ids": [11], "specialization_ids": [110]
            },
            "evidence": {"component_kind": "shield-strength", "validation_route": "outgoing-shield-or-barrier-state"}
          }]
        }"#;
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(manifest).unwrap();
        analyzer.observe(&envelope(
            "24609362",
            1,
            TimelineEventKind::Actor(ActorEvent {
                specialization_id: Some(110),
                ..actor_event(Some(11), None)
            }),
        ));
        analyzer.observe(&envelope(
            "24609362",
            2,
            shield_attribute_event(7, EntityAttributeUpdateKind::Snapshot, 100),
        ));
        assert_eq!(
            analyzer.report().obligations[0].observed_event_kinds,
            vec!["actor", "entity_attributes"]
        );

        analyzer.observe(&envelope(
            "24609362",
            3,
            shield_attribute_event(7, EntityAttributeUpdateKind::Delta, 75),
        ));
        assert_eq!(
            analyzer.report().obligations[0].coverage_state,
            "candidate-event-coverage-complete"
        );
    }

    #[test]
    fn named_radiant_shield_requires_its_exact_status_before_shield_change() {
        let manifest = r#"{
          "schema_version": 2,
          "game_build": "24609362",
          "obligations": [{
            "obligation_id": "mastery:122:0", "domain": "mastery-property",
            "subject_kind": "specialization-component", "subject_id": "mastery:122:0",
            "subject_name": "Radiant Shield", "requirements": ["exact status and shield change"],
            "required_event_kinds": ["actor", "entity_attributes", "shield_state", "status"],
            "selectors": {
              "effect_ids": [2206011], "attribute_ids": [11140],
              "class_ids": [12], "specialization_ids": [122]
            },
            "evidence": {"component_kind": "named-shield-gain", "validation_route": "named-shield-state"}
          }]
        }"#;
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(manifest).unwrap();
        analyzer.observe(&envelope(
            "24609362",
            1,
            TimelineEventKind::Actor(ActorEvent {
                specialization_id: Some(122),
                ..actor_event(Some(12), None)
            }),
        ));
        analyzer.observe(&envelope(
            "24609362",
            2,
            entity_attribute_event(7, EntityAttributeUpdateKind::Snapshot, &[(11140, 5000)]),
        ));
        analyzer.observe(&envelope(
            "24609362",
            3,
            shield_attribute_event(7, EntityAttributeUpdateKind::Snapshot, 100),
        ));
        analyzer.observe(&envelope(
            "24609362",
            4,
            shield_attribute_event(7, EntityAttributeUpdateKind::Delta, 75),
        ));
        assert!(
            !analyzer.report().obligations[0]
                .observed_event_kinds
                .contains(&"shield_state".into())
        );

        let mut radiant_status = status_event(7, 7, Some(44), StatusState::Applied, Some(1));
        if let TimelineEventKind::Status(event) = &mut radiant_status {
            event.effect = StatusEffectId(2_206_011);
        }
        analyzer.observe(&envelope("24609362", 5, radiant_status));
        analyzer.observe(&envelope(
            "24609362",
            6,
            shield_attribute_event(7, EntityAttributeUpdateKind::Delta, 50),
        ));
        assert_eq!(
            analyzer.report().obligations[0].coverage_state,
            "candidate-event-coverage-complete"
        );
    }

    #[test]
    fn healing_melody_resource_change_requires_exact_active_state() {
        let manifest = r#"{
          "schema_version": 2,
          "game_build": "24609362",
          "obligations": [{
            "obligation_id": "mastery:120:0", "domain": "mastery-property",
            "subject_kind": "specialization-component", "subject_id": "mastery:120:0",
            "subject_name": "Healing Melody", "requirements": ["exact status and resource change"],
            "required_event_kinds": ["actor", "entity_attributes", "resource", "status"],
            "selectors": {
              "effect_ids": [55332], "attribute_ids": [11140],
              "class_ids": [13], "specialization_ids": [120]
            },
            "evidence": {"component_kind": "named-decay-speed-reduction", "validation_route": "named-resource-decay-lifecycle"}
          }]
        }"#;
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(manifest).unwrap();
        let actor = entity(7);
        analyzer.observe(&envelope(
            "24609362",
            1,
            TimelineEventKind::Actor(ActorEvent {
                specialization_id: Some(120),
                ..actor_event(Some(13), None)
            }),
        ));
        analyzer.observe(&envelope(
            "24609362",
            2,
            entity_attribute_event(7, EntityAttributeUpdateKind::Snapshot, &[(11140, 5000)]),
        ));
        analyzer.observe(&envelope(
            "24609362",
            3,
            TimelineEventKind::Resource(ResourceEvent {
                actor,
                update_kind: EntityAttributeUpdateKind::Snapshot,
                origin_energy_raw_bits: Some(3_f32.to_bits()),
                resource_ids: vec![1],
                resource_values: vec![3],
                cooldowns: Vec::new(),
            }),
        ));
        analyzer.observe(&envelope(
            "24609362",
            4,
            TimelineEventKind::Resource(ResourceEvent {
                actor,
                update_kind: EntityAttributeUpdateKind::Delta,
                origin_energy_raw_bits: Some(2_f32.to_bits()),
                resource_ids: vec![1],
                resource_values: vec![2],
                cooldowns: Vec::new(),
            }),
        ));
        assert!(
            !analyzer.report().obligations[0]
                .observed_event_kinds
                .contains(&"resource".into())
        );

        let mut melody_status = status_event(7, 7, Some(45), StatusState::Applied, Some(1));
        if let TimelineEventKind::Status(event) = &mut melody_status {
            event.effect = StatusEffectId(55_332);
        }
        analyzer.observe(&envelope("24609362", 5, melody_status));
        analyzer.observe(&envelope(
            "24609362",
            6,
            TimelineEventKind::Resource(ResourceEvent {
                actor,
                update_kind: EntityAttributeUpdateKind::Delta,
                origin_energy_raw_bits: Some(1_f32.to_bits()),
                resource_ids: vec![1],
                resource_values: vec![1],
                cooldowns: Vec::new(),
            }),
        ));
        assert_eq!(
            analyzer.report().obligations[0].coverage_state,
            "candidate-event-coverage-complete"
        );
    }

    #[test]
    fn recipient_status_context_ends_when_the_instance_is_removed() {
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        let recipient = EntityRef {
            actor_id: ActorId(8),
            entity_uuid: EntityUuid(80),
        };
        let status = |state| {
            TimelineEventKind::Status(rlogs_events::StatusEvent {
                source: Some(actor()),
                target: recipient,
                effect: StatusEffectId(9001),
                instance_id: Some(StatusEffectInstanceId(44)),
                origin: None,
                state,
                stacks: None,
                duration_millis: None,
                level: None,
                part_id: None,
                count: None,
                created_at_millis: None,
            })
        };
        analyzer.observe(&envelope("24609362", 1, status(StatusState::Applied)));
        analyzer.observe(&envelope("24609362", 2, cast_event(recipient, 1)));
        analyzer.observe(&envelope("24609362", 3, status(StatusState::Removed)));
        analyzer.observe(&envelope("24609362", 4, cast_event(recipient, 1)));

        let report = analyzer.report();
        assert_eq!(report.obligations[0].contextual_matches, 1);
        assert_eq!(report.obligations[0].status_states["applied"], 1);
        assert_eq!(report.obligations[0].status_states["removed"], 1);
    }

    #[test]
    fn recipient_window_damage_is_retained_only_while_one_external_provider_is_active() {
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        analyzer.observe(&envelope(
            "24609362",
            1,
            status_event(7, 8, Some(44), StatusState::Applied, Some(2)),
        ));
        analyzer.observe(&envelope("24609362", 2, damage_event(8, 9, 125)));
        analyzer.observe(&envelope(
            "24609362",
            3,
            status_event(7, 8, Some(44), StatusState::Removed, Some(0)),
        ));
        analyzer.observe(&envelope("24609362", 4, damage_event(8, 9, 75)));

        let obligation = &analyzer.report().obligations[0];
        assert_eq!(obligation.recipient_window_damage_events, 1);
        assert_eq!(obligation.recipient_window_damage, "125");
        assert_eq!(obligation.single_provider_window_damage_events, 1);
        assert_eq!(obligation.single_provider_window_damage, "125");
        assert_eq!(obligation.ambiguous_provider_window_damage_events, 0);
        assert_eq!(obligation.maximum_concurrent_instances, 1);
        assert_eq!(obligation.maximum_concurrent_providers, 1);
        assert_eq!(obligation.minimum_stacks, Some(0));
        assert_eq!(obligation.maximum_stacks, Some(2));
        assert_eq!(obligation.stack_at_damage_observations.len(), 1);
        let stack_evidence = &obligation.stack_at_damage_observations[0];
        assert_eq!(stack_evidence.context, "recipient-window");
        assert_eq!(stack_evidence.event_count, 1);
        assert_eq!(stack_evidence.damage, "125");
        assert_eq!(stack_evidence.active_windows.len(), 1);
        assert_eq!(stack_evidence.active_windows[0].effect_id, "9001");
        assert_eq!(
            stack_evidence.active_windows[0]
                .status_instance_id
                .as_deref(),
            Some("44")
        );
        assert_eq!(
            stack_evidence.active_windows[0]
                .provider_actor_id
                .as_deref(),
            Some("7")
        );
        assert_eq!(stack_evidence.active_windows[0].stacks, Some(2));
    }

    #[test]
    fn stack_transitions_are_retained_at_each_damage_event_without_guessing() {
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        analyzer.observe(&envelope(
            "24609362",
            1,
            status_event(7, 8, Some(44), StatusState::Applied, Some(2)),
        ));
        analyzer.observe(&envelope("24609362", 2, damage_event(8, 9, 125)));
        analyzer.observe(&envelope(
            "24609362",
            3,
            status_event(7, 8, Some(44), StatusState::Consumed, Some(1)),
        ));
        analyzer.observe(&envelope("24609362", 4, damage_event(8, 9, 75)));
        analyzer.observe(&envelope(
            "24609362",
            5,
            status_event(7, 8, Some(44), StatusState::Consumed, Some(0)),
        ));
        analyzer.observe(&envelope("24609362", 6, damage_event(8, 9, 50)));

        let obligation = &analyzer.report().obligations[0];
        assert_eq!(obligation.recipient_window_damage_events, 2);
        assert_eq!(obligation.recipient_window_damage, "200");
        assert_eq!(obligation.stack_at_damage_observations.len(), 2);
        let stack_two = obligation
            .stack_at_damage_observations
            .iter()
            .find(|entry| entry.active_windows[0].stacks == Some(2))
            .expect("two-stack damage evidence should be retained");
        assert_eq!(stack_two.event_count, 1);
        assert_eq!(stack_two.damage, "125");
        let stack_one = obligation
            .stack_at_damage_observations
            .iter()
            .find(|entry| entry.active_windows[0].stacks == Some(1))
            .expect("one-stack damage evidence should be retained");
        assert_eq!(stack_one.event_count, 1);
        assert_eq!(stack_one.damage, "75");
    }

    #[test]
    fn current_build_terminal_effects_use_the_shared_provider_recipient_window_engine() {
        const TERMINAL_EFFECT_ID: i64 = 3_003_052;
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        assert!(!analyzer.indexes.effects.contains_key(&TERMINAL_EFFECT_ID));
        assert_ne!(
            dreamscope_observed_effect_match(TERMINAL_EFFECT_ID).resolution,
            DreamscopeEvidenceResolution::Unknown
        );

        analyzer.observe(&envelope(
            "24609362",
            1,
            status_event_for_effect(
                7,
                8,
                TERMINAL_EFFECT_ID,
                Some(55),
                StatusState::Applied,
                Some(3),
            ),
        ));
        analyzer.observe(&envelope("24609362", 2, damage_event(8, 9, 125)));
        analyzer.observe(&envelope(
            "24609362",
            3,
            status_event_for_effect(
                7,
                8,
                TERMINAL_EFFECT_ID,
                Some(55),
                StatusState::Removed,
                Some(0),
            ),
        ));
        analyzer.observe(&envelope("24609362", 4, damage_event(8, 9, 75)));

        let report = analyzer.report();
        let terminal = report
            .dreamscope_terminal_effects
            .iter()
            .find(|entry| entry.effect_id == TERMINAL_EFFECT_ID.to_string())
            .expect("current-build terminal effect should be retained");
        assert_eq!(terminal.source_match.evidence_id, TERMINAL_EFFECT_ID);
        assert_eq!(
            terminal.source_match.evidence_kind,
            crate::DreamscopeEvidenceKind::RuntimeEffect
        );
        assert_eq!(terminal.provider_recipient_observations.len(), 1);
        assert_eq!(
            terminal.provider_recipient_observations[0].observation_count,
            2
        );
        assert_eq!(terminal.status_states["applied"], 1);
        assert_eq!(terminal.status_states["removed"], 1);
        assert_eq!(terminal.status_instance_ids, vec!["55"]);
        assert_eq!(terminal.recipient_window_damage_events, 1);
        assert_eq!(terminal.recipient_window_damage, "125");
        assert_eq!(terminal.external_provider_window_damage_events, 1);
        assert_eq!(terminal.external_provider_window_damage, "125");
        assert_eq!(terminal.single_provider_window_damage_events, 1);
        assert_eq!(terminal.single_provider_window_damage, "125");
        assert_eq!(terminal.ambiguous_provider_window_damage_events, 0);
        assert_eq!(terminal.maximum_concurrent_instances, 1);
        assert_eq!(terminal.maximum_concurrent_providers, 1);
        assert_eq!(terminal.stack_at_damage_observations.len(), 1);
        assert_eq!(
            terminal.stack_at_damage_observations[0].active_windows[0].stacks,
            Some(3)
        );
        assert_eq!(terminal.stack_at_damage_observations[0].damage, "125");

        let ledger = &report.remote_rdps_readiness;
        assert!(!ledger.policy.build_snapshot_required);
        assert!(!ledger.policy.character_level_required);
        assert!(!ledger.policy.exact_equipment_required);
        assert!(!ledger.policy.exact_factor_tree_required);
        assert!(ledger.policy.retain_damage_when_unresolved);
        assert_eq!(ledger.summary.observed_effects, 1);
        assert_eq!(ledger.summary.observed_external_only_effects, 1);
        assert_eq!(ledger.summary.external_attribution_candidate_effects, 1);
        assert_eq!(ledger.summary.non_external_observed_effects, 0);
        assert_eq!(ledger.summary.unresolved_effects, 0);
        assert_eq!(ledger.summary.calculation_ready_effects, 1);
        assert_eq!(
            ledger.summary.effects_with_retained_recipient_window_damage,
            1
        );
        assert_eq!(ledger.summary.retained_recipient_window_damage_events, 1);
        assert_eq!(ledger.summary.retained_recipient_window_damage, "125");
        assert_eq!(
            ledger
                .summary
                .effects_with_retained_external_provider_window_damage,
            1
        );
        assert_eq!(
            ledger
                .summary
                .retained_external_provider_window_damage_events,
            1
        );
        assert_eq!(
            ledger.summary.retained_external_provider_window_damage,
            "125"
        );
        let readiness = &ledger.effects[0];
        assert_eq!(readiness.effect_id, TERMINAL_EFFECT_ID.to_string());
        assert_eq!(
            readiness.observed_provider_scope,
            RdpsValidationObservedProviderScope::ObservedExternalOnly
        );
        assert_eq!(readiness.self_provider_observations, 0);
        assert_eq!(readiness.external_provider_observations, 2);
        assert_eq!(readiness.unknown_provider_observations, 0);
        assert!(readiness.external_attribution_candidate);
        assert_eq!(
            readiness.scalar_resolution,
            RdpsValidationRemoteScalarResolution::CounterfactualReplay
        );
        assert!(readiness.calculation_ready);
        assert!(readiness.blockers.is_empty());
        assert_eq!(readiness.retained_recipient_window_damage_events, 1);
        assert_eq!(readiness.retained_recipient_window_damage, "125");
        assert_eq!(readiness.retained_external_provider_window_damage_events, 1);
        assert_eq!(readiness.retained_external_provider_window_damage, "125");

        let mut cumulative = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        cumulative.merge_report(&report).unwrap();
        let merged = cumulative.report();
        let terminal = merged
            .dreamscope_terminal_effects
            .iter()
            .find(|entry| entry.effect_id == TERMINAL_EFFECT_ID.to_string())
            .expect("terminal evidence should survive cumulative report merging");
        assert_eq!(terminal.recipient_window_damage, "125");
        assert_eq!(terminal.external_provider_window_damage, "125");
        assert_eq!(terminal.single_provider_window_damage, "125");
        assert_eq!(terminal.stack_at_damage_observations.len(), 1);
        assert_eq!(terminal.stack_at_damage_observations[0].damage, "125");
    }

    #[test]
    fn duration_bound_status_window_expires_at_the_packet_duration_boundary() {
        const TERMINAL_EFFECT_ID: i64 = 3_003_052;
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        let mut applied = status_event_for_effect(
            7,
            8,
            TERMINAL_EFFECT_ID,
            Some(55),
            StatusState::Applied,
            Some(1),
        );
        let TimelineEventKind::Status(status) = &mut applied else {
            unreachable!();
        };
        status.duration_millis = Some(1);

        analyzer.observe(&envelope_at("24609362", 1, 1_000, applied));
        analyzer.observe(&envelope_at("24609362", 2, 1_999, damage_event(8, 9, 125)));
        analyzer.observe(&envelope_at("24609362", 3, 2_000, damage_event(8, 9, 75)));

        let terminal = &analyzer.report().dreamscope_terminal_effects[0];
        assert_eq!(terminal.recipient_window_damage_events, 1);
        assert_eq!(terminal.recipient_window_damage, "125");
        assert_eq!(terminal.unresolved_recipient_window_damage_events, 0);
        assert_eq!(terminal.expired_status_windows, 1);
        assert!(terminal.remote_calculation.recipient_window_lifecycle_exact);
        assert!(!terminal.remote_calculation.target_window_lifecycle_exact);
        assert!(terminal.remote_calculation.lifecycle_exact);
    }

    #[test]
    fn harmony_readiness_requires_its_recipient_damage_lane() {
        const HARMONY_GRACE_EFFECT_ID: i64 = 3_003_052;
        let state = DreamscopeTerminalEffectState {
            provider_recipient_observations: BTreeMap::from([((Some(7), 8), 1)]),
            target_window_damage_events: 1,
            target_window_damage: 125,
            ..DreamscopeTerminalEffectState::default()
        };

        let readiness = dreamscope_remote_calculation_readiness(HARMONY_GRACE_EFFECT_ID, &state);

        assert!(!readiness.recipient_window_lifecycle_exact);
        assert!(readiness.target_window_lifecycle_exact);
        assert!(!readiness.lifecycle_exact);
        assert!(!readiness.calculation_ready);
        assert!(
            readiness
                .blockers
                .contains(&"exact_recipient_window_lifecycle".to_owned())
        );
    }

    #[test]
    fn missing_duration_stays_unresolved_until_an_exact_terminal() {
        const HARMONY_GRACE_EFFECT_ID: i64 = 3_003_052;
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        let mut applied = status_event_for_effect(
            7,
            8,
            HARMONY_GRACE_EFFECT_ID,
            Some(55),
            StatusState::Applied,
            Some(1),
        );
        let TimelineEventKind::Status(status) = &mut applied else {
            unreachable!();
        };
        status.duration_millis = None;

        analyzer.observe(&envelope_at("24609362", 1, 1_000, applied));
        analyzer.observe(&envelope_at("24609362", 2, 2_000, damage_event(8, 9, 125)));

        let report = analyzer.report();
        let terminal = &report.dreamscope_terminal_effects[0];
        assert_eq!(terminal.recipient_window_damage_events, 1);
        assert_eq!(terminal.open_unbounded_status_windows, 1);
        assert!(!terminal.remote_calculation.recipient_window_lifecycle_exact);
        assert!(!terminal.remote_calculation.calculation_ready);

        analyzer.observe(&envelope_at(
            "24609362",
            3,
            3_000,
            status_event_for_effect(
                7,
                8,
                HARMONY_GRACE_EFFECT_ID,
                Some(55),
                StatusState::Removed,
                Some(0),
            ),
        ));
        let closed = analyzer.report();
        let terminal = &closed.dreamscope_terminal_effects[0];
        assert_eq!(terminal.open_unbounded_status_windows, 0);
        assert!(terminal.remote_calculation.recipient_window_lifecycle_exact);
    }

    #[test]
    fn status_apply_expires_old_instances_before_concurrency_is_observed() {
        const HARMONY_GRACE_EFFECT_ID: i64 = 3_003_052;
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        let mut first = status_event_for_effect(
            7,
            8,
            HARMONY_GRACE_EFFECT_ID,
            Some(55),
            StatusState::Applied,
            Some(1),
        );
        let TimelineEventKind::Status(status) = &mut first else {
            unreachable!();
        };
        status.duration_millis = Some(1);
        let mut second = status_event_for_effect(
            7,
            8,
            HARMONY_GRACE_EFFECT_ID,
            Some(56),
            StatusState::Applied,
            Some(1),
        );
        let TimelineEventKind::Status(status) = &mut second else {
            unreachable!();
        };
        status.duration_millis = Some(1);

        analyzer.observe(&envelope_at("24609362", 1, 1_000, first));
        analyzer.observe(&envelope_at("24609362", 2, 2_000, second));

        let report = analyzer.report();
        let terminal = &report.dreamscope_terminal_effects[0];
        assert_eq!(terminal.expired_status_windows, 1);
        assert_eq!(terminal.maximum_concurrent_instances, 1);
        assert_eq!(terminal.maximum_concurrent_providers, 1);
    }

    #[test]
    fn equal_time_damage_is_unresolved_even_when_serialized_after_apply() {
        const TERMINAL_EFFECT_ID: i64 = 3_003_052;
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        analyzer.observe(&envelope_at(
            "24609362",
            1,
            1_000,
            status_event_for_effect(
                7,
                8,
                TERMINAL_EFFECT_ID,
                Some(55),
                StatusState::Applied,
                Some(1),
            ),
        ));
        analyzer.observe(&envelope_at("24609362", 2, 1_000, damage_event(8, 9, 125)));
        analyzer.observe(&envelope_at(
            "24609362",
            3,
            2_000,
            status_event_for_effect(
                7,
                8,
                TERMINAL_EFFECT_ID,
                Some(55),
                StatusState::Removed,
                Some(0),
            ),
        ));

        let terminal = &analyzer.report().dreamscope_terminal_effects[0];
        assert_eq!(terminal.recipient_window_damage_events, 0);
        assert_eq!(terminal.recipient_window_damage, "0");
        assert_eq!(terminal.unresolved_recipient_window_damage_events, 1);
        assert!(!terminal.remote_calculation.recipient_window_lifecycle_exact);
        assert!(!terminal.remote_calculation.lifecycle_exact);
        assert!(
            terminal
                .remote_calculation
                .blockers
                .contains(&"exact_recipient_window_lifecycle".to_owned())
        );
    }

    #[test]
    fn target_lane_unresolved_rows_cannot_borrow_recipient_or_terminal_proof() {
        const TERMINAL_EFFECT_ID: i64 = 3_003_052;
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        analyzer.observe(&envelope_at(
            "24609362",
            1,
            1_000,
            status_event_for_effect(
                7,
                8,
                TERMINAL_EFFECT_ID,
                Some(55),
                StatusState::Applied,
                Some(1),
            ),
        ));
        analyzer.observe(&envelope_at("24609362", 2, 2_000, damage_event(8, 9, 125)));
        analyzer.observe(&envelope_at(
            "24609362",
            3,
            3_000,
            status_event_for_effect(
                7,
                8,
                TERMINAL_EFFECT_ID,
                Some(55),
                StatusState::Refreshed,
                Some(1),
            ),
        ));
        analyzer.observe(&envelope_at("24609362", 4, 3_000, damage_event(9, 8, 300)));
        analyzer.observe(&envelope_at(
            "24609362",
            5,
            4_000,
            status_event_for_effect(
                7,
                8,
                TERMINAL_EFFECT_ID,
                Some(55),
                StatusState::Removed,
                Some(0),
            ),
        ));

        let terminal = &analyzer.report().dreamscope_terminal_effects[0];
        assert_eq!(terminal.recipient_window_damage_events, 1);
        assert_eq!(terminal.unresolved_recipient_window_damage_events, 0);
        assert_eq!(terminal.target_window_damage_events, 0);
        assert_eq!(terminal.unresolved_target_window_damage_events, 1);
        assert!(terminal.remote_calculation.recipient_window_lifecycle_exact);
        assert!(!terminal.remote_calculation.target_window_lifecycle_exact);
        assert!(!terminal.remote_calculation.lifecycle_exact);
        assert!(
            terminal
                .remote_calculation
                .blockers
                .contains(&"exact_target_window_lifecycle".to_owned())
        );
    }

    #[test]
    fn observed_self_only_terminal_effects_remain_visible_without_becoming_rdps_blockers() {
        const TERMINAL_EFFECT_ID: i64 = 3_003_052;
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();

        analyzer.observe(&envelope(
            "24609362",
            1,
            status_event_for_effect(
                8,
                8,
                TERMINAL_EFFECT_ID,
                Some(55),
                StatusState::Applied,
                Some(3),
            ),
        ));
        analyzer.observe(&envelope("24609362", 2, damage_event(8, 9, 125)));
        analyzer.observe(&envelope(
            "24609362",
            3,
            status_event_for_effect(
                8,
                8,
                TERMINAL_EFFECT_ID,
                Some(55),
                StatusState::Removed,
                Some(0),
            ),
        ));

        let report = analyzer.report();
        let terminal = &report.dreamscope_terminal_effects[0];
        assert_eq!(terminal.recipient_window_damage_events, 1);
        assert_eq!(terminal.recipient_window_damage, "125");
        assert_eq!(terminal.external_provider_window_damage_events, 0);
        assert_eq!(terminal.external_provider_window_damage, "0");

        let ledger = &report.remote_rdps_readiness;
        assert_eq!(ledger.summary.observed_effects, 1);
        assert_eq!(ledger.summary.observed_self_only_effects, 1);
        assert_eq!(ledger.summary.external_attribution_candidate_effects, 0);
        assert_eq!(ledger.summary.non_external_observed_effects, 1);
        assert_eq!(ledger.summary.calculation_ready_effects, 0);
        assert_eq!(ledger.summary.unresolved_effects, 0);
        assert!(ledger.summary.blockers.is_empty());
        assert_eq!(ledger.summary.retained_recipient_window_damage, "125");
        assert_eq!(ledger.summary.retained_external_provider_window_damage, "0");

        let readiness = &ledger.effects[0];
        assert_eq!(
            readiness.observed_provider_scope,
            RdpsValidationObservedProviderScope::ObservedSelfOnly
        );
        assert_eq!(readiness.self_provider_observations, 2);
        assert_eq!(readiness.external_provider_observations, 0);
        assert!(!readiness.external_attribution_candidate);
        assert!(!readiness.calculation_ready);
        assert!(readiness.blockers.is_empty());
    }

    #[test]
    fn mixed_terminal_effect_scope_only_retains_external_provider_damage_for_rdps() {
        const TERMINAL_EFFECT_ID: i64 = 3_003_052;
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();

        analyzer.observe(&envelope(
            "24609362",
            1,
            status_event_for_effect(
                7,
                8,
                TERMINAL_EFFECT_ID,
                Some(55),
                StatusState::Applied,
                Some(3),
            ),
        ));
        analyzer.observe(&envelope("24609362", 2, damage_event(8, 9, 125)));
        analyzer.observe(&envelope(
            "24609362",
            3,
            status_event_for_effect(
                7,
                8,
                TERMINAL_EFFECT_ID,
                Some(55),
                StatusState::Removed,
                Some(0),
            ),
        ));
        analyzer.observe(&envelope(
            "24609362",
            4,
            status_event_for_effect(
                8,
                8,
                TERMINAL_EFFECT_ID,
                Some(56),
                StatusState::Applied,
                Some(3),
            ),
        ));
        analyzer.observe(&envelope("24609362", 5, damage_event(8, 9, 75)));
        analyzer.observe(&envelope(
            "24609362",
            6,
            status_event_for_effect(
                8,
                8,
                TERMINAL_EFFECT_ID,
                Some(56),
                StatusState::Removed,
                Some(0),
            ),
        ));

        let report = analyzer.report();
        let terminal = &report.dreamscope_terminal_effects[0];
        assert_eq!(terminal.recipient_window_damage, "200");
        assert_eq!(terminal.external_provider_window_damage, "125");

        let ledger = &report.remote_rdps_readiness;
        assert_eq!(ledger.summary.observed_mixed_effects, 1);
        assert_eq!(ledger.summary.external_attribution_candidate_effects, 1);
        assert_eq!(ledger.summary.retained_recipient_window_damage, "200");
        assert_eq!(
            ledger.summary.retained_external_provider_window_damage,
            "125"
        );
        let readiness = &ledger.effects[0];
        assert_eq!(
            readiness.observed_provider_scope,
            RdpsValidationObservedProviderScope::ObservedMixed
        );
        assert_eq!(readiness.self_provider_observations, 2);
        assert_eq!(readiness.external_provider_observations, 2);
        assert!(readiness.external_attribution_candidate);
    }

    #[test]
    fn exact_provider_factor_selection_resolves_a_shared_terminal_effect_in_the_report() {
        const CHARACTER_ID: i64 = 42;
        const FACTOR_ITEM_ID: i64 = 20_021_881;
        const TERMINAL_EFFECT_ID: i64 = 9_901;
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();

        analyzer.observe(&profile_envelope(
            1,
            &CHARACTER_ID.to_string(),
            FACTOR_ITEM_ID,
        ));
        let mut provider = actor_event(None, None);
        provider.actor.entity_uuid = EntityUuid((CHARACTER_ID << 16) | 1);
        analyzer.observe(&envelope("24609362", 2, TimelineEventKind::Actor(provider)));
        analyzer.observe(&envelope(
            "24609362",
            3,
            status_event_for_effect(
                7,
                8,
                TERMINAL_EFFECT_ID,
                Some(56),
                StatusState::Applied,
                Some(1),
            ),
        ));
        analyzer.observe(&envelope(
            "24609362",
            4,
            status_event_for_effect(
                7,
                8,
                TERMINAL_EFFECT_ID,
                Some(56),
                StatusState::Removed,
                Some(0),
            ),
        ));

        let report = analyzer.report();
        let terminal = report
            .dreamscope_terminal_effects
            .iter()
            .find(|entry| entry.effect_id == TERMINAL_EFFECT_ID.to_string())
            .expect("shared terminal effect should be retained");
        assert_eq!(terminal.source_observations.len(), 1);
        let source = &terminal.source_observations[0];
        assert_eq!(source.provider_actor_id.as_deref(), Some("7"));
        assert_eq!(source.route_resolution, EffectFingerprintResolution::Exact);
        assert_eq!(
            source.equipped_variant_resolution,
            EffectFingerprintResolution::Exact
        );
        assert_eq!(source.resolution, EffectFingerprintResolution::Exact);
        assert_eq!(
            source.source_id.as_deref(),
            Some("dreamscope-factor_family:202289:terminal:3052430")
        );
        assert_eq!(
            source.source_kind.as_deref(),
            Some("dreamscope-factor-family")
        );
        assert_eq!(source.selected_factor_item_id.as_deref(), Some("20021881"));
        assert_eq!(source.selected_factor_grade, Some(1));
        assert_eq!(source.observation_count, 2);
        assert!(!terminal.remote_calculation.build_metadata_required);
        assert!(terminal.remote_calculation.route_exact);
        assert!(terminal.remote_calculation.provider_recipient_exact);
        assert!(!terminal.remote_calculation.recipient_window_lifecycle_exact);
        assert!(!terminal.remote_calculation.target_window_lifecycle_exact);
        assert!(!terminal.remote_calculation.lifecycle_exact);
        assert_eq!(
            terminal.remote_calculation.scalar_resolution,
            RdpsValidationRemoteScalarResolution::Unresolved
        );
        assert!(!terminal.remote_calculation.calculation_ready);
        assert_eq!(
            terminal.remote_calculation.blockers,
            vec![
                "exact_status_window_membership",
                "runtime_applied_magnitude"
            ]
        );
        let ledger = &report.remote_rdps_readiness;
        assert_eq!(ledger.summary.observed_effects, 1);
        assert_eq!(ledger.summary.calculation_ready_effects, 0);
        assert_eq!(ledger.summary.unresolved_effects, 1);
        assert_eq!(
            ledger.summary.blockers.get("runtime_applied_magnitude"),
            Some(&1)
        );
        assert_eq!(
            ledger.summary.scalar_resolutions.get("unresolved"),
            Some(&1)
        );
        assert_eq!(
            ledger.effects[0].blockers,
            vec![
                "exact_status_window_membership",
                "runtime_applied_magnitude"
            ]
        );

        let mut cumulative = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        cumulative.merge_report(&report).unwrap();
        let merged = cumulative.report();
        let merged_source = &merged
            .dreamscope_terminal_effects
            .iter()
            .find(|entry| entry.effect_id == TERMINAL_EFFECT_ID.to_string())
            .expect("source proof should survive cumulative report merging")
            .source_observations[0];
        assert_eq!(merged_source.resolution, EffectFingerprintResolution::Exact);
        assert_eq!(merged_source.observation_count, 2);
    }

    #[test]
    fn exact_packet_origin_resolves_remote_factor_family_without_inventing_grade() {
        const TERMINAL_EFFECT_ID: i64 = 9_901;
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();

        analyzer.observe(&envelope(
            "24609362",
            1,
            status_event_for_effect_with_origin(
                7,
                8,
                TERMINAL_EFFECT_ID,
                1,
                3_052_430,
                StatusState::Applied,
            ),
        ));
        analyzer.observe(&envelope(
            "24609362",
            2,
            status_event_for_effect_with_origin(
                7,
                8,
                TERMINAL_EFFECT_ID,
                1,
                3_052_430,
                StatusState::Removed,
            ),
        ));

        let report = analyzer.report();
        let terminal = report
            .dreamscope_terminal_effects
            .iter()
            .find(|entry| entry.effect_id == TERMINAL_EFFECT_ID.to_string())
            .expect("terminal effect should be retained");
        let source = &terminal.source_observations[0];
        assert_eq!(
            source.match_kind,
            crate::EffectFingerprintMatchKind::ExactPacketOrigin
        );
        assert_eq!(source.route_resolution, EffectFingerprintResolution::Exact);
        assert_eq!(source.resolution, EffectFingerprintResolution::Exact);
        assert_eq!(
            source.equipped_variant_resolution,
            EffectFingerprintResolution::Unresolved
        );
        assert_eq!(
            source.source_id.as_deref(),
            Some("dreamscope-factor_family:202289:terminal:3052430")
        );
        assert_eq!(source.selected_factor_item_id, None);
        assert_eq!(source.selected_factor_grade, None);
        assert_eq!(terminal.packet_levels["1"], 1);
        assert_eq!(terminal.packet_levels["null"], 1);
        assert_eq!(terminal.packet_part_ids["null"], 2);
        assert_eq!(terminal.packet_counts["-1"], 1);
        assert_eq!(terminal.packet_counts["null"], 1);
        assert_eq!(terminal.packet_durations_millis["5000"], 1);
        assert_eq!(terminal.packet_durations_millis["null"], 1);
        assert!(!terminal.remote_calculation.build_metadata_required);
        assert!(terminal.remote_calculation.route_exact);
        assert!(terminal.remote_calculation.provider_recipient_exact);
        assert!(!terminal.remote_calculation.recipient_window_lifecycle_exact);
        assert!(!terminal.remote_calculation.target_window_lifecycle_exact);
        assert!(!terminal.remote_calculation.lifecycle_exact);
        assert_eq!(
            terminal.remote_calculation.scalar_resolution,
            RdpsValidationRemoteScalarResolution::Unresolved
        );
        assert!(!terminal.remote_calculation.calculation_ready);
        assert_eq!(
            terminal.remote_calculation.blockers,
            vec![
                "exact_status_window_membership",
                "runtime_applied_magnitude"
            ]
        );
    }

    #[test]
    fn same_effect_from_two_providers_is_retained_as_ambiguous_without_instance_ids() {
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        analyzer.observe(&envelope(
            "24609362",
            1,
            status_event(7, 8, None, StatusState::Applied, Some(1)),
        ));
        analyzer.observe(&envelope(
            "24609362",
            2,
            status_event(9, 8, None, StatusState::Applied, Some(1)),
        ));
        analyzer.observe(&envelope("24609362", 3, damage_event(8, 10, 200)));
        analyzer.observe(&envelope(
            "24609362",
            4,
            status_event(7, 8, None, StatusState::Removed, Some(0)),
        ));
        analyzer.observe(&envelope("24609362", 5, damage_event(8, 10, 50)));

        let obligation = &analyzer.report().obligations[0];
        assert_eq!(obligation.recipient_window_damage_events, 2);
        assert_eq!(obligation.recipient_window_damage, "250");
        assert_eq!(obligation.single_provider_window_damage_events, 1);
        assert_eq!(obligation.single_provider_window_damage, "50");
        assert_eq!(obligation.ambiguous_provider_window_damage_events, 1);
        assert_eq!(obligation.maximum_concurrent_instances, 2);
        assert_eq!(obligation.maximum_concurrent_providers, 2);
        assert_eq!(obligation.provider_recipient_observations.len(), 2);
        assert_eq!(obligation.stack_at_damage_observations.len(), 2);
        let ambiguous = obligation
            .stack_at_damage_observations
            .iter()
            .find(|entry| entry.active_windows.len() == 2)
            .expect("the two-provider window should remain explicit");
        assert_eq!(ambiguous.event_count, 1);
        assert_eq!(ambiguous.damage, "200");
        assert_eq!(
            ambiguous
                .active_windows
                .iter()
                .map(|window| window.provider_actor_id.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("7"), Some("9")]
        );
        let exact = obligation
            .stack_at_damage_observations
            .iter()
            .find(|entry| entry.active_windows.len() == 1)
            .expect("the remaining exact provider window should be retained");
        assert_eq!(exact.damage, "50");
        assert_eq!(
            exact.active_windows[0].provider_actor_id.as_deref(),
            Some("9")
        );
    }

    #[test]
    fn providerless_removal_ends_all_matching_windows_without_stale_attribution() {
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        analyzer.observe(&envelope(
            "24609362",
            1,
            status_event(7, 8, None, StatusState::Applied, Some(1)),
        ));
        analyzer.observe(&envelope(
            "24609362",
            2,
            status_event(9, 8, None, StatusState::Applied, Some(1)),
        ));
        analyzer.observe(&envelope(
            "24609362",
            3,
            TimelineEventKind::Status(rlogs_events::StatusEvent {
                source: None,
                target: entity(8),
                effect: StatusEffectId(9001),
                instance_id: None,
                origin: None,
                state: StatusState::Removed,
                stacks: Some(0),
                duration_millis: None,
                level: None,
                part_id: None,
                count: None,
                created_at_millis: None,
            }),
        ));
        analyzer.observe(&envelope("24609362", 4, damage_event(8, 10, 500)));

        let obligation = &analyzer.report().obligations[0];
        assert_eq!(obligation.ambiguous_status_removals, 1);
        assert_eq!(obligation.recipient_window_damage_events, 0);
        assert_eq!(obligation.recipient_window_damage, "0");
    }

    #[test]
    fn target_window_damage_and_attribute_transitions_are_recorded_separately() {
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        analyzer.observe(&envelope(
            "24609362",
            1,
            status_event(7, 8, Some(55), StatusState::Applied, Some(1)),
        ));
        analyzer.observe(&envelope("24609362", 2, damage_event(9, 8, 300)));
        for (sequence, value) in [(3, 100), (4, 100), (5, 120)] {
            analyzer.observe(&envelope(
                "24609362",
                sequence,
                TimelineEventKind::EntityAttributes(EntityAttributeEvent {
                    actor: entity(8),
                    update_kind: EntityAttributeUpdateKind::Delta,
                    ownership: None,
                    attributes: vec![EntityAttribute {
                        attribute_id: 116,
                        raw_value: Vec::new(),
                        decoded: Some(EntityAttributeValue::Integer(value)),
                    }],
                }),
            ));
        }
        for (sequence, value) in [(6, 5), (7, 7)] {
            analyzer.observe(&envelope(
                "24609362",
                sequence,
                TimelineEventKind::TemporaryAttributes(TemporaryAttributeEvent {
                    actor: entity(8),
                    update_kind: EntityAttributeUpdateKind::Delta,
                    attributes: vec![TemporaryAttribute { id: 116, value }],
                }),
            ));
        }

        let report = analyzer.report();
        let status_obligation = &report.obligations[0];
        assert_eq!(status_obligation.target_window_damage_events, 1);
        assert_eq!(status_obligation.target_window_damage, "300");
        let attribute_obligation = &report.obligations[1];
        assert_eq!(
            attribute_obligation.attribute_values["entity:116"],
            vec!["100", "120"]
        );
        assert_eq!(
            attribute_obligation.attribute_values["temporary:116"],
            vec!["5", "7"]
        );
        assert_eq!(
            attribute_obligation.attribute_transition_counts["entity:116"],
            1
        );
        assert_eq!(
            attribute_obligation.attribute_transition_counts["temporary:116"],
            1
        );
    }

    #[test]
    fn target_mitigation_requires_both_attribute_lanes_on_the_damaged_target() {
        let mut analyzer =
            RdpsValidationAnalyzer::from_manifest_json(TARGET_MITIGATION_MANIFEST).unwrap();
        analyzer.observe(&envelope(
            "24609362",
            1,
            entity_attribute_event(7, EntityAttributeUpdateKind::Snapshot, &[(116, 100)]),
        ));
        analyzer.observe(&envelope(
            "24609362",
            2,
            temporary_attribute_event(7, EntityAttributeUpdateKind::Snapshot, &[(116, 5)]),
        ));
        analyzer.observe(&envelope("24609362", 3, damage_event(7, 8, 100)));
        assert_eq!(
            analyzer.report().obligations[0].observed_event_kinds,
            vec!["entity_attributes", "temporary_attributes"]
        );

        analyzer.observe(&envelope(
            "24609362",
            4,
            entity_attribute_event(8, EntityAttributeUpdateKind::Snapshot, &[(116, 120)]),
        ));
        analyzer.observe(&envelope(
            "24609362",
            5,
            temporary_attribute_event(8, EntityAttributeUpdateKind::Snapshot, &[(116, 7)]),
        ));
        analyzer.observe(&envelope("24609362", 6, damage_event(7, 8, 90)));
        assert_eq!(
            analyzer.report().obligations[0].coverage_state,
            "candidate-event-coverage-complete"
        );
    }

    fn entity_attribute_event(
        actor_id: u64,
        update_kind: EntityAttributeUpdateKind,
        attributes: &[(i32, i64)],
    ) -> TimelineEventKind {
        TimelineEventKind::EntityAttributes(EntityAttributeEvent {
            actor: entity(actor_id),
            update_kind,
            ownership: None,
            attributes: attributes
                .iter()
                .map(|&(attribute_id, value)| EntityAttribute {
                    attribute_id,
                    raw_value: Vec::new(),
                    decoded: Some(EntityAttributeValue::Integer(value)),
                })
                .collect(),
        })
    }

    fn temporary_attribute_event(
        actor_id: u64,
        update_kind: EntityAttributeUpdateKind,
        attributes: &[(i32, i32)],
    ) -> TimelineEventKind {
        TimelineEventKind::TemporaryAttributes(TemporaryAttributeEvent {
            actor: entity(actor_id),
            update_kind,
            attributes: attributes
                .iter()
                .map(|&(id, value)| TemporaryAttribute { id, value })
                .collect(),
        })
    }

    #[test]
    fn formula_input_is_snapshotted_only_when_its_route_triggers() {
        let mut analyzer =
            RdpsValidationAnalyzer::from_manifest_json(FORMULA_INPUT_MANIFEST).unwrap();
        analyzer.observe(&envelope(
            "24609362",
            1,
            entity_attribute_event(7, EntityAttributeUpdateKind::Snapshot, &[(11310, 52_000)]),
        ));
        let before = analyzer.report();
        assert_eq!(before.summary.no_candidate_evidence, 1);
        assert!(before.obligations[0].formula_input_snapshots.is_empty());

        analyzer.observe(&envelope(
            "24609362",
            2,
            status_event(7, 8, Some(1), StatusState::Applied, Some(1)),
        ));
        let obligation = &analyzer.report().obligations[0];
        assert_eq!(
            obligation.coverage_state,
            "partial-candidate-event-coverage"
        );
        assert_eq!(
            obligation.observed_event_kinds,
            vec!["status", "formula_inputs"]
        );
        let snapshot = &obligation.formula_input_snapshots[0];
        assert_eq!(snapshot.state, "complete");
        assert_eq!(snapshot.session_id, "test");
        assert_eq!(snapshot.trigger_observed_micros, 2);
        assert_eq!(snapshot.actor_role, "source");
        assert_eq!(snapshot.actor_id.as_deref(), Some("7"));
        assert_eq!(snapshot.values[0].attribute_id, "11310");
        assert_eq!(snapshot.values[0].value, "52000");
        assert_eq!(snapshot.values[0].attribute_sequence, 1);
    }

    #[test]
    fn target_formula_input_snapshots_the_status_recipient_not_the_provider() {
        let manifest = FORMULA_INPUT_MANIFEST
            .replace("\"actor_role\": \"source\"", "\"actor_role\": \"target\"");
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(&manifest).unwrap();
        analyzer.observe(&envelope(
            "24609362",
            1,
            entity_attribute_event(7, EntityAttributeUpdateKind::Snapshot, &[(11310, 52_000)]),
        ));
        analyzer.observe(&envelope(
            "24609362",
            2,
            entity_attribute_event(8, EntityAttributeUpdateKind::Snapshot, &[(11310, 41_000)]),
        ));
        analyzer.observe(&envelope(
            "24609362",
            3,
            status_event(7, 8, Some(1), StatusState::Applied, Some(1)),
        ));

        let obligation = &analyzer.report().obligations[0];
        let snapshot = &obligation.formula_input_snapshots[0];
        assert_eq!(snapshot.state, "complete");
        assert_eq!(snapshot.actor_role, "target");
        assert_eq!(snapshot.actor_id.as_deref(), Some("8"));
        assert_eq!(snapshot.values[0].attribute_id, "11310");
        assert_eq!(snapshot.values[0].value, "41000");
        assert_eq!(snapshot.values[0].attribute_sequence, 2);
    }

    #[test]
    fn loadout_tier_input_requires_an_exact_supported_event_time_tier() {
        let provider = |evidence, tier| {
            let mut event = actor_event(None, None);
            event.primary_loadout = vec![ActorLoadoutSlot {
                slot_id: 8,
                ability_id: Some(3_971),
                item_id: Some(3_000_123),
                tier: Some(tier),
            }];
            event.loadout_observation.primary = evidence;
            event
        };

        let mut exact =
            RdpsValidationAnalyzer::from_manifest_json(LOADOUT_TIER_INPUT_MANIFEST).unwrap();
        exact.observe(&envelope(
            "24609362",
            1,
            TimelineEventKind::Actor(provider(ActorLoadoutEvidence::ExactSlots, 5)),
        ));
        exact.observe(&envelope(
            "24609362",
            2,
            status_event(7, 8, Some(1), StatusState::Applied, Some(1)),
        ));
        let snapshot = &exact.report().obligations[0].formula_input_snapshots[0];
        assert_eq!(snapshot.state, "complete");
        assert!(snapshot.values.is_empty());
        assert_eq!(snapshot.loadout_values.len(), 1);
        assert_eq!(snapshot.loadout_values[0].evidence, "exact_slots");
        assert_eq!(snapshot.loadout_values[0].scope, "primary");
        assert_eq!(snapshot.loadout_values[0].tier, Some(5));
        assert_eq!(snapshot.loadout_values[0].observation_sequence, 1);

        let mut observed =
            RdpsValidationAnalyzer::from_manifest_json(LOADOUT_TIER_INPUT_MANIFEST).unwrap();
        observed.observe(&envelope(
            "24609362",
            1,
            TimelineEventKind::Actor(provider(ActorLoadoutEvidence::ObservedSet, 5)),
        ));
        observed.observe(&envelope(
            "24609362",
            2,
            status_event(7, 8, Some(1), StatusState::Applied, Some(1)),
        ));
        let snapshot = &observed.report().obligations[0].formula_input_snapshots[0];
        assert_eq!(snapshot.state, "observed-set-only");
        assert_eq!(snapshot.loadout_values[0].evidence, "observed_set");

        let mut base_tier =
            RdpsValidationAnalyzer::from_manifest_json(LOADOUT_TIER_INPUT_MANIFEST).unwrap();
        base_tier.observe(&envelope(
            "24609362",
            1,
            TimelineEventKind::Actor(provider(ActorLoadoutEvidence::ExactSlots, 0)),
        ));
        base_tier.observe(&envelope(
            "24609362",
            2,
            status_event(7, 8, Some(1), StatusState::Applied, Some(1)),
        ));
        let snapshot = &base_tier.report().obligations[0].formula_input_snapshots[0];
        assert_eq!(snapshot.state, "complete");
        assert_eq!(snapshot.loadout_values[0].tier, Some(0));

        let mut unsupported =
            RdpsValidationAnalyzer::from_manifest_json(LOADOUT_TIER_INPUT_MANIFEST).unwrap();
        unsupported.observe(&envelope(
            "24609362",
            1,
            TimelineEventKind::Actor(provider(ActorLoadoutEvidence::ExactSlots, 6)),
        ));
        unsupported.observe(&envelope(
            "24609362",
            2,
            status_event(7, 8, Some(1), StatusState::Applied, Some(1)),
        ));
        let snapshot = &unsupported.report().obligations[0].formula_input_snapshots[0];
        assert_eq!(snapshot.state, "unsupported-current-tier");
        assert_eq!(snapshot.loadout_values[0].tier, Some(6));
    }

    #[test]
    fn class_attribute_input_selects_only_the_event_time_class_route() {
        let mut analyzer =
            RdpsValidationAnalyzer::from_manifest_json(CLASS_ATTRIBUTE_INPUT_MANIFEST).unwrap();
        let mut recipient = actor_event(Some(13), None);
        recipient.actor = entity(8);
        analyzer.observe(&envelope(
            "24609362",
            1,
            TimelineEventKind::Actor(recipient),
        ));
        analyzer.observe(&envelope(
            "24609362",
            2,
            entity_attribute_event(
                8,
                EntityAttributeUpdateKind::Snapshot,
                &[(11330, 99_999), (11340, 41_000)],
            ),
        ));
        analyzer.observe(&envelope(
            "24609362",
            3,
            status_event(7, 8, Some(1), StatusState::Applied, Some(1)),
        ));

        let snapshot = &analyzer.report().obligations[0].formula_input_snapshots[0];
        assert_eq!(snapshot.state, "complete");
        assert_eq!(snapshot.actor_role, "target");
        assert_eq!(snapshot.actor_id.as_deref(), Some("8"));
        assert_eq!(snapshot.class_id, Some(13));
        assert_eq!(snapshot.class_observation_sequence, Some(1));
        assert_eq!(snapshot.class_observation_observed_micros, Some(1));
        assert_eq!(snapshot.values.len(), 1);
        assert_eq!(snapshot.values[0].attribute_id, "11340");
        assert_eq!(snapshot.values[0].value, "41000");
    }

    #[test]
    fn actor_lifetime_boundary_clears_class_selected_formula_state() {
        let mut analyzer =
            RdpsValidationAnalyzer::from_manifest_json(CLASS_ATTRIBUTE_INPUT_MANIFEST).unwrap();
        let mut recipient = actor_event(Some(13), None);
        recipient.actor = entity(8);
        analyzer.observe(&envelope(
            "24609362",
            1,
            TimelineEventKind::Actor(recipient.clone()),
        ));
        analyzer.observe(&envelope(
            "24609362",
            2,
            entity_attribute_event(8, EntityAttributeUpdateKind::Snapshot, &[(11340, 41_000)]),
        ));
        recipient.state = ActorState::Despawned;
        recipient.class_id = None;
        analyzer.observe(&envelope(
            "24609362",
            3,
            TimelineEventKind::Actor(recipient),
        ));
        analyzer.observe(&envelope(
            "24609362",
            4,
            status_event(7, 8, Some(1), StatusState::Applied, Some(1)),
        ));

        let snapshot = &analyzer.report().obligations[0].formula_input_snapshots[0];
        assert_eq!(snapshot.state, "missing-current-class-state");
        assert_eq!(snapshot.class_id, None);
        assert!(snapshot.values.is_empty());
    }

    #[test]
    fn status_origin_match_does_not_trigger_parent_effect_formula_inputs() {
        let mut analyzer =
            RdpsValidationAnalyzer::from_manifest_json(FORMULA_INPUT_MANIFEST).unwrap();
        analyzer.observe(&envelope(
            "24609362",
            1,
            entity_attribute_event(7, EntityAttributeUpdateKind::Snapshot, &[(11310, 52_000)]),
        ));
        let mut child = status_event(7, 8, Some(1), StatusState::Applied, Some(1));
        let TimelineEventKind::Status(child) = &mut child else {
            unreachable!();
        };
        child.effect = StatusEffectId(2_110_049);
        child.origin = Some(StatusOrigin {
            source_type_id: 1,
            source_config_id: 9_001,
        });
        analyzer.observe(&envelope(
            "24609362",
            2,
            TimelineEventKind::Status(child.clone()),
        ));

        let obligation = &analyzer.report().obligations[0];
        assert!(obligation.formula_input_snapshots.is_empty());
        assert!(
            obligation
                .matched_identifiers
                .iter()
                .any(|value| value == "origin_buff:9001")
        );
    }

    #[test]
    fn removal_does_not_capture_a_pre_effect_formula_input_snapshot() {
        let mut analyzer =
            RdpsValidationAnalyzer::from_manifest_json(FORMULA_INPUT_MANIFEST).unwrap();
        analyzer.observe(&envelope(
            "24609362",
            1,
            entity_attribute_event(7, EntityAttributeUpdateKind::Snapshot, &[(11310, 52_000)]),
        ));
        analyzer.observe(&envelope(
            "24609362",
            2,
            status_event(7, 8, Some(1), StatusState::Removed, Some(1)),
        ));

        let obligation = &analyzer.report().obligations[0];
        assert!(obligation.formula_input_snapshots.is_empty());
        assert_eq!(obligation.status_states.get("removed"), Some(&1));
    }

    #[test]
    fn missing_formula_input_is_retained_as_incomplete() {
        let mut analyzer =
            RdpsValidationAnalyzer::from_manifest_json(FORMULA_INPUT_MANIFEST).unwrap();
        analyzer.observe(&envelope(
            "24609362",
            1,
            status_event(7, 8, Some(1), StatusState::Applied, Some(1)),
        ));
        let obligation = &analyzer.report().obligations[0];
        assert_eq!(obligation.observed_event_kinds, vec!["status"]);
        assert_eq!(
            obligation.formula_input_snapshots[0].state,
            "missing-current-value"
        );
        assert!(obligation.formula_input_snapshots[0].values.is_empty());
    }

    #[test]
    fn immediately_preceding_damage_is_retained_for_packet_output_discovery() {
        let mut analyzer =
            RdpsValidationAnalyzer::from_manifest_json(FORMULA_INPUT_MANIFEST).unwrap();
        analyzer.observe(&envelope(
            "24609362",
            1,
            entity_attribute_event(7, EntityAttributeUpdateKind::Snapshot, &[(11310, 52_000)]),
        ));
        analyzer.observe(&envelope("24609362", 2, damage_event(7, 9, 1_040)));
        analyzer.observe(&envelope(
            "24609362",
            3,
            status_event(7, 8, Some(1), StatusState::Applied, Some(1)),
        ));

        let report = analyzer.report();
        assert_eq!(report.summary.candidate_event_coverage_complete, 1);
        assert_eq!(report.summary.proof_promotions, 0);
        let obligation = &report.obligations[0];
        assert_eq!(obligation.packet_damage_rows.len(), 1);
        assert_eq!(
            obligation.packet_damage_rows[0].context,
            "pre-trigger-buffer"
        );
        assert_eq!(obligation.packet_damage_rows[0].amount, "1040");
        assert_eq!(obligation.packet_damage_rows[0].first_sequence, Some(2));
        assert_eq!(obligation.packet_damage_rows[0].last_sequence, Some(2));
    }

    #[test]
    fn attribute_selector_context_reaches_later_actor_events() {
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        analyzer.observe(&envelope(
            "24609362",
            1,
            entity_attribute_event(7, EntityAttributeUpdateKind::Snapshot, &[(116, 100)]),
        ));
        analyzer.observe(&envelope(
            "24609362",
            2,
            temporary_attribute_event(7, EntityAttributeUpdateKind::Delta, &[(116, 5)]),
        ));
        analyzer.observe(&envelope("24609362", 3, damage_event(7, 8, 100)));

        let obligation = &analyzer.report().obligations[1];
        assert_eq!(
            obligation.observed_event_kinds,
            vec!["damage", "entity_attributes", "temporary_attributes"]
        );
        assert_eq!(
            obligation.coverage_state,
            "candidate-event-coverage-complete"
        );
        assert_eq!(obligation.selected_actor_ids, vec!["7"]);
    }

    #[test]
    fn authoritative_attribute_snapshot_clears_only_its_own_lane() {
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        analyzer.observe(&envelope(
            "24609362",
            1,
            entity_attribute_event(7, EntityAttributeUpdateKind::Snapshot, &[(116, 100)]),
        ));
        analyzer.observe(&envelope(
            "24609362",
            2,
            temporary_attribute_event(7, EntityAttributeUpdateKind::Snapshot, &[(116, 5)]),
        ));
        analyzer.observe(&envelope(
            "24609362",
            3,
            entity_attribute_event(7, EntityAttributeUpdateKind::Snapshot, &[(999, 1)]),
        ));
        analyzer.observe(&envelope("24609362", 4, damage_event(7, 8, 100)));

        let obligation = &analyzer.report().obligations[1];
        assert!(
            obligation
                .observed_event_kinds
                .contains(&"damage".to_owned())
        );

        analyzer.observe(&envelope(
            "24609362",
            5,
            temporary_attribute_event(7, EntityAttributeUpdateKind::Snapshot, &[(999, 1)]),
        ));
        let before = analyzer.report().obligations[1].contextual_matches;
        analyzer.observe(&envelope("24609362", 6, damage_event(7, 8, 100)));
        assert_eq!(analyzer.report().obligations[1].contextual_matches, before);
    }

    #[test]
    fn sparse_attribute_updates_do_not_clear_prior_selector_context() {
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        analyzer.observe(&envelope(
            "24609362",
            1,
            entity_attribute_event(7, EntityAttributeUpdateKind::Snapshot, &[(116, 100)]),
        ));
        analyzer.observe(&envelope(
            "24609362",
            2,
            entity_attribute_event(7, EntityAttributeUpdateKind::Delta, &[(999, 1)]),
        ));
        analyzer.observe(&envelope(
            "24609362",
            3,
            entity_attribute_event(7, EntityAttributeUpdateKind::Unknown, &[(998, 1)]),
        ));
        analyzer.observe(&envelope("24609362", 4, damage_event(7, 8, 100)));

        let obligation = &analyzer.report().obligations[1];
        assert!(
            obligation
                .observed_event_kinds
                .contains(&"damage".to_owned())
        );
        assert_eq!(obligation.attribute_values["entity:116"], vec!["100"]);
    }

    #[test]
    fn build_drift_warns_without_discarding_candidate_evidence() {
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        analyzer.observe(&envelope(
            "24609999",
            1,
            TimelineEventKind::Status(rlogs_events::StatusEvent {
                source: Some(actor()),
                target: actor(),
                effect: StatusEffectId(9001),
                instance_id: None,
                origin: None,
                state: StatusState::Applied,
                stacks: None,
                duration_millis: None,
                level: None,
                part_id: None,
                count: None,
                created_at_millis: None,
            }),
        ));
        let report = analyzer.report();
        assert!(report.provisional_build_mismatch);
        assert_eq!(report.summary.partial_candidate_event_coverage, 1);
        assert!(!report.warnings.is_empty());
    }

    #[test]
    fn projected_terms_are_linked_without_promoting_candidate_coverage() {
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        analyzer.observe_projected_contributions(
            41,
            &[ExactDamageContributionEvent {
                observed_micros: 41,
                effect_id: 9001,
                provider_actor_id: 7,
                recipient_actor_id: 8,
                scope: DamageContributionScope::CompleteEffect,
                amount: 20,
                observed_damage: 100,
                included: true,
            }],
            &[ExactRationalDamageContributionEvent {
                observed_micros: 41,
                effect_id: 9001,
                provider_actor_id: 7,
                recipient_actor_id: 8,
                scope: DamageContributionScope::CompleteEffect,
                numerator: 1,
                denominator: 2,
                observed_damage: 100,
                included: true,
                deferred_damage_context: None,
            }],
            "provisional-reviewed-formulas",
        );

        let report = analyzer.report();
        assert_eq!(report.summary.proof_promotions, 0);
        assert_eq!(report.summary.no_candidate_evidence, 2);
        assert_eq!(report.projection.integer_events, 1);
        assert_eq!(report.projection.rational_events, 1);
        assert_eq!(report.projection.invalid_events, 0);
        let obligation = &report.obligations[0];
        assert_eq!(obligation.projected_integer_amount, "20");
        assert_eq!(obligation.projected_integer_observed_damage, "100");
        assert_eq!(obligation.projected_rational_totals.len(), 1);
        assert_eq!(obligation.projected_rational_totals[0].numerator, "1");
        assert_eq!(obligation.projected_rational_totals[0].denominator, "2");
        assert_eq!(
            obligation.projected_provider_recipient_observations.len(),
            1
        );
        assert_eq!(
            obligation.projected_provider_recipient_observations[0].observation_count,
            2
        );
    }

    #[test]
    fn invalid_and_unmatched_projected_terms_are_retained_fail_closed() {
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        analyzer.observe_projected_contributions(
            42,
            &[
                ExactDamageContributionEvent {
                    observed_micros: 42,
                    effect_id: 9001,
                    provider_actor_id: 7,
                    recipient_actor_id: 7,
                    scope: DamageContributionScope::CompleteEffect,
                    amount: 101,
                    observed_damage: 100,
                    included: true,
                },
                ExactDamageContributionEvent {
                    observed_micros: 42,
                    effect_id: 9999,
                    provider_actor_id: 7,
                    recipient_actor_id: 8,
                    scope: DamageContributionScope::CompleteEffect,
                    amount: 10,
                    observed_damage: 100,
                    included: false,
                },
            ],
            &[],
            "candidate",
        );

        let report = analyzer.report();
        assert_eq!(report.projection.integer_events, 2);
        assert_eq!(report.projection.invalid_events, 1);
        assert_eq!(report.projection.excluded_events, 1);
        assert_eq!(report.projection.unmatched_effects.len(), 1);
        assert_eq!(report.projection.unmatched_effects[0].effect_id, "9999");
        assert_eq!(report.obligations[0].projected_invalid_events, 1);
        assert_eq!(report.obligations[0].projected_integer_amount, "0");
        assert_eq!(report.summary.proof_promotions, 0);
    }

    #[test]
    fn reports_merge_across_sessions_without_promoting_proof() {
        let mut first = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        first.observe(&envelope(
            "24609362",
            1,
            TimelineEventKind::Actor(actor_event(None, Some(5001))),
        ));
        first.observe_projected_contributions(
            2,
            &[ExactDamageContributionEvent {
                observed_micros: 2,
                effect_id: 9001,
                provider_actor_id: 7,
                recipient_actor_id: 8,
                scope: DamageContributionScope::CompleteEffect,
                amount: 20,
                observed_damage: 100,
                included: true,
            }],
            &[],
            "candidate",
        );

        let mut second = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        second.observe(&envelope("24609362", 3, cast_event(actor(), 7001)));
        second.observe(&envelope(
            "24609362",
            4,
            status_event(7, 8, Some(44), StatusState::Applied, Some(2)),
        ));
        second.observe(&envelope("24609362", 5, damage_event(8, 9, 125)));

        let first_report = first.report();
        let second_report = second.report();
        let encoded = serde_json::to_string(&first_report).unwrap();
        let decoded = serde_json::from_str(&encoded).unwrap();
        let mut aggregate = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        aggregate.merge_report(&decoded).unwrap();
        aggregate.merge_report(&second_report).unwrap();

        let report = aggregate.report();
        assert_eq!(report.total_events, 4);
        assert_eq!(report.summary.candidate_event_coverage_complete, 1);
        assert_eq!(report.summary.proof_promotions, 0);
        assert_eq!(report.projection.integer_events, 1);
        assert_eq!(report.obligations[0].projected_integer_amount, "20");
        assert_eq!(report.obligations[0].recipient_window_damage, "125");
        assert_eq!(report.obligations[0].maximum_stacks, Some(2));
        assert_eq!(report.obligations[0].stack_at_damage_observations.len(), 1);
        assert_eq!(
            report.obligations[0].stack_at_damage_observations[0].damage,
            "125"
        );
        assert_eq!(
            report.obligations[0].stack_at_damage_observations[0].active_windows[0].stacks,
            Some(2)
        );
    }

    #[test]
    fn live_progress_unions_durable_baseline_without_mutating_either_session() {
        let mut baseline = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        baseline.observe(&envelope(
            "24609362",
            1,
            TimelineEventKind::Actor(actor_event(None, Some(5001))),
        ));

        let mut current = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        current.observe(&envelope("24609362", 2, cast_event(actor(), 7001)));
        current.observe(&envelope(
            "24609362",
            3,
            status_event(7, 8, Some(44), StatusState::Applied, Some(2)),
        ));
        current.observe(&envelope("24609362", 4, damage_event(8, 9, 125)));

        assert_eq!(baseline.progress().candidate_event_coverage_complete, 0);
        assert_eq!(current.progress().candidate_event_coverage_complete, 0);
        let cumulative = current.progress_with_baseline(&baseline).unwrap();
        assert_eq!(cumulative.candidate_event_coverage_complete, 1);
        assert_eq!(cumulative.partial_candidate_event_coverage, 0);
        assert_eq!(cumulative.no_candidate_evidence, 1);
        assert_eq!(cumulative.by_domain["factor"].total, 1);
        assert_eq!(
            cumulative.by_domain["factor"].candidate_event_coverage_complete,
            1
        );
        assert_eq!(cumulative.by_domain["formula"].no_candidate_evidence, 1);
        assert_eq!(baseline.report().total_events, 1);
        assert_eq!(current.report().total_events, 3);
    }

    #[test]
    fn manifest_rejects_a_mismatched_validation_report_schema() {
        let manifest = MANIFEST.replacen(
            "\"game_build\": \"24609362\"",
            "\"game_build\": \"24609362\", \"validation_report_schema\": 999",
            1,
        );
        assert!(matches!(
            RdpsValidationAnalyzer::from_manifest_json(&manifest),
            Err(RdpsValidationError::UnsupportedReportSchema(999))
        ));
    }

    #[test]
    fn incompatible_report_merge_is_transactional() {
        let mut source = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        source.observe(&envelope(
            "24609362",
            1,
            TimelineEventKind::Actor(actor_event(None, Some(5001))),
        ));
        let mut report = source.report();
        report.obligations[1].required_event_kinds = vec!["damage".into()];

        let mut aggregate = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        assert!(aggregate.merge_report(&report).is_err());
        let unchanged = aggregate.report();
        assert_eq!(unchanged.total_events, 0);
        assert_eq!(unchanged.summary.no_candidate_evidence, 2);
        assert_eq!(unchanged.obligations[0].direct_matches, 0);
    }

    #[test]
    fn report_with_stale_selector_contract_cannot_enter_cumulative_proof() {
        let source = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        let mut report = source.report();
        report.obligations[0].selector_contract = "{\"effect_ids\":[999999]}".into();

        let mut aggregate = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        let error = aggregate.merge_report(&report).unwrap_err().to_string();

        assert!(error.contains("metadata differs from the current manifest"));
        assert_eq!(aggregate.report().total_events, 0);
    }

    #[test]
    fn provisional_build_mismatch_cannot_enter_exact_cumulative_proof() {
        let mut source = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        source.observe(&envelope(
            "24699999",
            1,
            TimelineEventKind::Actor(actor_event(None, Some(5001))),
        ));
        let report = source.report();
        assert!(report.provisional_build_mismatch);

        let mut aggregate = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        let error = aggregate.merge_report(&report).unwrap_err().to_string();
        assert!(error.contains("cannot enter exact-build cumulative proof"));
        assert_eq!(aggregate.report().total_events, 0);
    }

    #[test]
    fn bundled_current_build_watch_is_complete_and_non_authoritative() {
        let analyzer = RdpsValidationAnalyzer::bundled().unwrap();
        let report = analyzer.report();
        assert_eq!(analyzer.manifest_game_build(), "24609362");
        assert_eq!(report.summary.total_obligations, 350);
        assert_eq!(report.summary.no_candidate_evidence, 350);
        assert_eq!(report.summary.proof_promotions, 0);
        assert_eq!(report.schema_version, RDPS_VALIDATION_REPORT_SCHEMA_VERSION);
    }

    #[test]
    fn capture_preflight_accepts_complete_current_build_decoder_surface() {
        let analyzer = RdpsValidationAnalyzer::bundled().unwrap();
        let pack = capability_pack(
            "24609362",
            &[
                DecoderKind::SyncNearEntitiesV1,
                DecoderKind::SyncNearDeltaV1,
                DecoderKind::SyncClientUseSkillV1,
                DecoderKind::SyncContainerDataV1,
            ],
        );

        let preflight = analyzer.ensure_capture_capable(&pack).unwrap();
        assert!(preflight.capture_capable);
        assert!(preflight.exact_build_proof_capable);
        assert!(preflight.missing_event_kinds.is_empty());
        assert_eq!(
            preflight.required_event_kinds,
            vec![
                "actor",
                "damage",
                "status",
                "entity_attributes",
                "temporary_attributes",
                "formula_inputs",
                "profile_selection",
                "resource",
                "cooldown",
                "healing",
                "shield_state",
            ]
        );
    }

    #[test]
    fn newer_promoted_pack_remains_provisional_against_the_bundled_watch() {
        let analyzer = RdpsValidationAnalyzer::bundled().unwrap();
        let pack = ProtocolPack::from_json(include_bytes!(
            "../protocol-packs/global/steam-24687926/pack.json"
        ))
        .unwrap();

        let preflight = analyzer.ensure_capture_capable(&pack).unwrap();
        assert!(preflight.capture_capable);
        assert!(!preflight.exact_build_match);
        assert!(!preflight.exact_build_proof_capable);
        assert_eq!(preflight.protocol_pack_game_build, "24687926");
        assert!(preflight.missing_event_kinds.is_empty());
    }

    #[test]
    fn capture_preflight_keeps_hotfix_mismatch_provisional() {
        let analyzer = RdpsValidationAnalyzer::bundled().unwrap();
        let pack = capability_pack(
            "24699999",
            &[
                DecoderKind::SyncNearEntitiesV1,
                DecoderKind::SyncNearDeltaV1,
                DecoderKind::SyncClientUseSkillV1,
                DecoderKind::SyncContainerDataV1,
            ],
        );

        let preflight = analyzer.ensure_capture_capable(&pack).unwrap();
        assert!(preflight.capture_capable);
        assert!(!preflight.exact_build_match);
        assert!(!preflight.exact_build_proof_capable);
    }

    #[test]
    fn capture_preflight_rejects_pack_without_profile_decoder() {
        let analyzer = RdpsValidationAnalyzer::bundled().unwrap();
        let pack = capability_pack(
            "24609362",
            &[
                DecoderKind::SyncNearEntitiesV1,
                DecoderKind::SyncNearDeltaV1,
            ],
        );

        let error = analyzer.ensure_capture_capable(&pack).unwrap_err();
        assert!(matches!(
            error,
            RdpsValidationError::MissingProtocolCapabilities { .. }
        ));
        assert!(error.to_string().contains("profile_selection"));
    }

    #[test]
    fn explicitly_observed_build_marks_an_empty_mismatch_provisional() {
        let mut analyzer = RdpsValidationAnalyzer::from_manifest_json(MANIFEST).unwrap();
        analyzer.observe_game_build("24699999");

        let report = analyzer.report();
        assert_eq!(report.total_events, 0);
        assert_eq!(report.observed_game_builds, vec!["24699999"]);
        assert!(report.provisional_build_mismatch);
        assert_eq!(report.summary.proof_promotions, 0);
        assert!(report.warnings.iter().any(|warning| {
            warning.contains("manifest build 24609362")
                && warning.contains("observed build(s) 24699999")
        }));
    }
}
