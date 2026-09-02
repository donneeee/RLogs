use std::collections::{BTreeMap, BTreeSet};

use rlogs_events::{
    CanonicalEvent, DamageEvent, EntityAttributeUpdateKind, EntityRef, EventEnvelope,
    ResourceCooldown, ResourceEvent, StatusEvent, StatusState, TimelineEventKind,
};
use serde::{Deserialize, Serialize};

use crate::{
    BPSR_GAME_PLUGIN_ID, BPSR_PROFILE_SCHEMA_ID, BPSR_PROFILE_SCHEMA_VERSION,
    CharacterProfilePatch, DreamscopeBuildInferenceAnalyzer, DreamscopeBuildInferenceError,
    DreamscopeBuildInferenceReport, DreamscopeSourceKind, PsychoscopeActionRelationKind,
    PsychoscopeFactorStat, PsychoscopeFactorValueUnit, PsychoscopeFormulaInputRole,
    PsychoscopeStatCreditPolicy, character_id_from_entity_uuid, combat_action_presentation,
    dreamscope_candidates_for_terminal_effect, dreamscope_catalog_game_build,
    dreamscope_factor_item_by_id, psychoscope_factor_by_item_id, psychoscope_factor_rules,
    psychoscope_factor_runtime_rules_enabled,
};

pub const FACTOR_CORRELATION_SCHEMA_VERSION: u16 = 8;
const FACTOR_CORRELATION_POLICY: &str =
    "evidence_only_disable_rdps_until_provider_recipient_stacking_review";
const MAXIMUM_CORRELATION_WINDOWS: usize = 100_000;
const MAXIMUM_UNMATCHED_LIFECYCLE_EVENTS: usize = 100_000;
const MAXIMUM_LIFECYCLE_SAMPLES_PER_WINDOW: usize = 8_192;
const MAXIMUM_RESOURCE_SAMPLES_PER_WINDOW: usize = 8_192;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactorSelectionEvidence {
    SourceOwnsFactor,
    RecipientOwnsFactor,
    SourceAndRecipientOwnFactor,
    StaticCatalogOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactorWindowCloseReason {
    Removed,
    ConsumedAtZeroStacks,
    DurationElapsed,
    Reapplied,
    LogEnded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactorActionDamageRole {
    Trigger,
    Target,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactorDamageActorRelation {
    Provider,
    Recipient,
    ProviderAndRecipient,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactorResourceActorRelation {
    Provider,
    Recipient,
    ProviderAndRecipient,
}

/// Reconstructable wire state only. Resource IDs, values, float bits, and
/// cooldown fields deliberately retain their packet identities without
/// assigning gameplay meanings or units.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactorResourceWireState {
    /// True only after an exact `Snapshot` update. A delta observed without a
    /// preceding snapshot remains explicitly partial.
    pub complete_snapshot: bool,
    pub origin_energy_raw_bits: Option<u32>,
    pub resource_ids: Vec<u32>,
    pub resource_values: Vec<u32>,
    pub cooldowns: Vec<ResourceCooldown>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactorResourceBaseline {
    pub actor_entity_uuid: i64,
    pub actor_relation: FactorResourceActorRelation,
    /// Exact state known immediately before this factor window opened. `None`
    /// means no reconstructable packet state was available; it is not an empty
    /// resource set.
    pub state_before_window: Option<FactorResourceWireState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactorResourceTransitionSample {
    pub sequence: u64,
    pub observed_micros: u64,
    pub actor_entity_uuid: i64,
    pub actor_relation: FactorResourceActorRelation,
    pub update_kind: EntityAttributeUpdateKind,
    pub origin_energy_raw_bits: Option<u32>,
    pub resource_ids: Vec<u32>,
    pub resource_values: Vec<u32>,
    pub cooldowns: Vec<ResourceCooldown>,
    /// Change flags are populated only when both sides of the comparison are
    /// backed by a complete packet snapshot. `None` is unknown, never false.
    pub origin_energy_changed: Option<bool>,
    pub resource_ids_changed: Option<bool>,
    pub resource_values_changed: Option<bool>,
    pub cooldowns_changed: Option<bool>,
    pub complete_state_after: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactorSelectionObservation {
    pub sequence: u64,
    pub observed_micros: u64,
    pub character_id: String,
    pub season_id: Option<i32>,
    pub selected_factor_item_ids: Vec<i64>,
    /// Exact current-build factor items whose selection identity and grade are
    /// known, but whose mechanics have not yet passed attribution review.
    pub unreviewed_factor_item_ids: Vec<i64>,
    /// Item IDs absent from the complete current-build selector catalog.
    pub unmapped_factor_item_ids: Vec<i64>,
    /// Exact selected-factor stat inputs retained for later formula replay.
    /// These records never enable rDPS by themselves.
    pub formula_inputs: Vec<SelectedFactorFormulaInput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedFactorFormulaInput {
    pub factor_item_id: i64,
    pub stat: PsychoscopeFactorStat,
    pub attribute_id: Option<i64>,
    pub value: i64,
    pub unit: PsychoscopeFactorValueUnit,
    pub role: PsychoscopeFormulaInputRole,
    pub credit_policy: PsychoscopeStatCreditPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactorLifecycleSample {
    pub sequence: u64,
    pub observed_micros: u64,
    pub state: StatusState,
    pub stacks: Option<u32>,
    pub duration_millis: Option<u64>,
    /// Exact packet status level. Some factor/status families may use this as a
    /// grade discriminator, but the audit never assumes that semantic without
    /// a controlled packet correlation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<i32>,
    /// Exact packet part identifier retained for factor-family disambiguation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part_id: Option<i32>,
    /// Exact packet count retained independently from stack count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<i32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactorDamageTotals {
    pub event_count: u64,
    pub amount: i64,
    pub first_observed_micros: Option<u64>,
    pub last_observed_micros: Option<u64>,
}

impl FactorDamageTotals {
    fn observe(&mut self, observed_micros: u64, amount: i64) {
        self.event_count = self.event_count.saturating_add(1);
        self.amount = self.amount.saturating_add(amount);
        self.first_observed_micros.get_or_insert(observed_micros);
        self.last_observed_micros = Some(observed_micros);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactorActionDamageAggregate {
    pub factor_item_id: i64,
    pub relation_index: usize,
    pub relation_kind: PsychoscopeActionRelationKind,
    pub action_role: FactorActionDamageRole,
    pub actor_relation: FactorDamageActorRelation,
    pub ability_id: i64,
    pub recount_group_id: i64,
    pub totals: FactorDamageTotals,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactorCorrelationWindow {
    pub window_id: u64,
    pub effect_id: i64,
    pub instance_id: Option<i64>,
    pub factor_item_ids: Vec<i64>,
    pub selection_evidence: FactorSelectionEvidence,
    pub selected_owner_character_ids: Vec<String>,
    pub provider_entity_uuid: Option<i64>,
    pub recipient_entity_uuid: i64,
    pub opened_sequence: u64,
    pub opened_observed_micros: u64,
    pub opened_from_non_apply_state: bool,
    pub closed_sequence: Option<u64>,
    pub closed_observed_micros: Option<u64>,
    pub close_reason: Option<FactorWindowCloseReason>,
    pub lifecycle: Vec<FactorLifecycleSample>,
    pub dropped_lifecycle_samples: u64,
    pub minimum_observed_stacks: Option<u32>,
    pub maximum_observed_stacks: Option<u32>,
    pub apply_count: u64,
    pub refresh_count: u64,
    pub stack_count: u64,
    pub consume_count: u64,
    pub remove_count: u64,
    pub refresh_without_stack_change_count: u64,
    pub recipient_outgoing_damage: FactorDamageTotals,
    pub recipient_incoming_damage: FactorDamageTotals,
    pub provider_outgoing_damage: FactorDamageTotals,
    pub action_damage: Vec<FactorActionDamageAggregate>,
    pub resource_baselines: Vec<FactorResourceBaseline>,
    pub resource_transitions: Vec<FactorResourceTransitionSample>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnmatchedFactorLifecycleEvent {
    pub sequence: u64,
    pub observed_micros: u64,
    pub effect_id: i64,
    pub instance_id: Option<i64>,
    pub provider_entity_uuid: Option<i64>,
    pub recipient_entity_uuid: i64,
    pub state: StatusState,
    pub stacks: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part_id: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<i32>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactorRuleCorrelationSummary {
    pub factor_item_id: i64,
    pub effect_id: i64,
    pub window_count: u64,
    pub exact_source_owner_window_count: u64,
    pub exact_recipient_owner_window_count: u64,
    pub static_catalog_only_window_count: u64,
    pub applied_count: u64,
    pub refreshed_count: u64,
    pub stacked_count: u64,
    pub consumed_count: u64,
    pub removed_count: u64,
    pub minimum_observed_stacks: Option<u32>,
    pub maximum_observed_stacks: Option<u32>,
    pub overlapping_window_pairs: u64,
    pub maximum_concurrent_instances_per_recipient: u32,
    pub maximum_concurrent_distinct_providers_per_recipient: u32,
    pub recipient_outgoing_damage: FactorDamageTotals,
    pub recipient_incoming_damage: FactorDamageTotals,
    pub provider_outgoing_damage: FactorDamageTotals,
    pub matched_action_damage: FactorDamageTotals,
    pub resource_transition_count: u64,
    pub provider_resource_transition_count: u64,
    pub recipient_resource_transition_count: u64,
    pub provider_and_recipient_resource_transition_count: u64,
    pub origin_energy_change_count: u64,
    pub resource_ids_change_count: u64,
    pub resource_values_change_count: u64,
    pub cooldown_change_count: u64,
    pub incomplete_state_after_count: u64,
    pub distinct_resource_ids: Vec<u32>,
    pub distinct_cooldown_resource_ids: Vec<i32>,
    pub attribution_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PsychoscopeFactorCorrelationReport {
    pub schema_version: u16,
    /// Exact client builds carried by the canonical events that were analyzed.
    /// This is intentionally separate from `game_build`, which identifies the
    /// static Dreamscope catalog used to interpret those events.
    pub observed_client_builds: Vec<String>,
    /// Protocol-pack digests carried by the same canonical events. These make
    /// capture provenance auditable without inferring it from filenames or
    /// analysis time.
    pub observed_protocol_pack_digests: Vec<String>,
    pub game_build: String,
    pub session_id: String,
    pub policy: String,
    pub rdps_attribution_enabled: bool,
    pub first_observed_micros: Option<u64>,
    pub last_observed_micros: Option<u64>,
    pub selection_observations: Vec<FactorSelectionObservation>,
    /// One canonical current-build inference stream shared by historical,
    /// live, and later rDPS consumers. Runtime terminal IDs may prove an
    /// observed choice, but ambiguous IDs never become guessed loadouts.
    pub dreamscope_build_inference: DreamscopeBuildInferenceReport,
    pub rule_summaries: Vec<FactorRuleCorrelationSummary>,
    pub windows: Vec<FactorCorrelationWindow>,
    pub unmatched_lifecycle_events: Vec<UnmatchedFactorLifecycleEvent>,
}

#[derive(Debug, Clone)]
struct ActiveFactorSelection {
    character_id: String,
    selected_factor_item_ids: BTreeSet<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ActionDamageKey {
    factor_item_id: i64,
    relation_index: usize,
    action_role: u8,
    actor_relation: u8,
    ability_id: i64,
    recount_group_id: i64,
}

#[derive(Debug, Clone)]
struct ActiveWindow {
    report: FactorCorrelationWindow,
    expiration_observed_micros: Option<u64>,
    action_damage: BTreeMap<ActionDamageKey, FactorActionDamageAggregate>,
    last_observed_stacks: Option<u32>,
}

#[derive(Debug, thiserror::Error)]
pub enum FactorCorrelationError {
    #[error("factor correlation received multiple sessions: {expected} and {actual}")]
    MixedSessions { expected: String, actual: String },

    #[error("factor correlation event sequence moved backward from {previous} to {actual}")]
    SequenceMovedBackward { previous: u64, actual: u64 },

    #[error("factor correlation exceeded its {limit} window safety limit")]
    WindowLimitExceeded { limit: usize },

    #[error("factor correlation exceeded its {limit} unmatched-lifecycle safety limit")]
    UnmatchedLifecycleLimitExceeded { limit: usize },

    #[error(
        "factor correlation window {window_id} exceeded its {limit} lifecycle-sample safety limit"
    )]
    LifecycleSampleLimitExceeded { window_id: u64, limit: usize },

    #[error(
        "factor correlation window {window_id} exceeded its {limit} resource-sample safety limit"
    )]
    ResourceSampleLimitExceeded { window_id: u64, limit: usize },

    #[error("invalid BPSR profile evidence: {0}")]
    InvalidProfile(String),

    #[error("factor catalog lookup failed: {0}")]
    Catalog(String),

    #[error("Dreamscope build inference failed: {0}")]
    BuildInference(#[from] DreamscopeBuildInferenceError),
}

#[derive(Debug, Default)]
pub struct PsychoscopeFactorCorrelationAnalyzer {
    session_id: Option<String>,
    observed_client_builds: BTreeSet<String>,
    observed_protocol_pack_digests: BTreeSet<String>,
    last_sequence: Option<u64>,
    first_observed_micros: Option<u64>,
    last_observed_micros: Option<u64>,
    selections_by_character: BTreeMap<String, ActiveFactorSelection>,
    selection_observations: Vec<FactorSelectionObservation>,
    active_windows: BTreeMap<u64, ActiveWindow>,
    closed_windows: Vec<FactorCorrelationWindow>,
    unmatched_lifecycle_events: Vec<UnmatchedFactorLifecycleEvent>,
    recount_groups_by_ability: BTreeMap<i64, BTreeSet<i64>>,
    dreamscope_build_inference: DreamscopeBuildInferenceAnalyzer,
    resource_state_by_entity_uuid: BTreeMap<i64, FactorResourceWireState>,
    next_window_id: u64,
}

impl PsychoscopeFactorCorrelationAnalyzer {
    pub fn new() -> Self {
        Self {
            next_window_id: 1,
            ..Self::default()
        }
    }

    pub fn observe(&mut self, envelope: &EventEnvelope) -> Result<(), FactorCorrelationError> {
        self.validate_envelope(envelope)?;
        self.dreamscope_build_inference.observe(envelope)?;
        self.expire_windows_before(envelope.time.observed_micros)?;

        match &envelope.event {
            CanonicalEvent::CharacterProfileObserved { profile }
                if profile.game_plugin_id == BPSR_GAME_PLUGIN_ID
                    && profile.payload_schema_id == BPSR_PROFILE_SCHEMA_ID
                    && profile.payload_schema_version == BPSR_PROFILE_SCHEMA_VERSION =>
            {
                let profile = CharacterProfilePatch::from_game_event(profile)
                    .map_err(|error| FactorCorrelationError::InvalidProfile(error.to_string()))?;
                self.observe_profile(envelope, &profile)?;
            }
            CanonicalEvent::Timeline(timeline) => match &timeline.kind {
                TimelineEventKind::Status(status) => self.observe_status(envelope, status)?,
                TimelineEventKind::Damage(damage) => {
                    self.observe_damage(envelope.time.observed_micros, damage)?
                }
                TimelineEventKind::Resource(resource) => {
                    self.observe_resource(envelope, resource)?
                }
                _ => {}
            },
            _ => {}
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<PsychoscopeFactorCorrelationReport, FactorCorrelationError> {
        let end = self.last_observed_micros.unwrap_or_default();
        let window_ids = self.active_windows.keys().copied().collect::<Vec<_>>();
        for window_id in window_ids {
            self.close_window(window_id, None, end, FactorWindowCloseReason::LogEnded);
        }
        self.closed_windows.sort_by_key(|window| window.window_id);
        let rule_summaries = summarize_rules(&self.closed_windows);
        let dreamscope_build_inference = self.dreamscope_build_inference.finish();
        Ok(PsychoscopeFactorCorrelationReport {
            schema_version: FACTOR_CORRELATION_SCHEMA_VERSION,
            observed_client_builds: self.observed_client_builds.into_iter().collect(),
            observed_protocol_pack_digests: self
                .observed_protocol_pack_digests
                .into_iter()
                .collect(),
            game_build: dreamscope_catalog_game_build().into(),
            session_id: self.session_id.unwrap_or_default(),
            policy: FACTOR_CORRELATION_POLICY.into(),
            rdps_attribution_enabled: false,
            first_observed_micros: self.first_observed_micros,
            last_observed_micros: self.last_observed_micros,
            selection_observations: self.selection_observations,
            dreamscope_build_inference,
            rule_summaries,
            windows: self.closed_windows,
            unmatched_lifecycle_events: self.unmatched_lifecycle_events,
        })
    }

    fn validate_envelope(
        &mut self,
        envelope: &EventEnvelope,
    ) -> Result<(), FactorCorrelationError> {
        if let Some(session_id) = &self.session_id {
            if session_id != &envelope.session_id {
                return Err(FactorCorrelationError::MixedSessions {
                    expected: session_id.clone(),
                    actual: envelope.session_id.clone(),
                });
            }
        } else {
            self.session_id = Some(envelope.session_id.clone());
        }
        if !envelope.region.client_build.trim().is_empty() {
            self.observed_client_builds
                .insert(envelope.region.client_build.clone());
        }
        if !envelope.region.protocol_pack_digest.trim().is_empty() {
            self.observed_protocol_pack_digests
                .insert(envelope.region.protocol_pack_digest.clone());
        }
        if let Some(previous) = self.last_sequence
            && envelope.sequence <= previous
        {
            return Err(FactorCorrelationError::SequenceMovedBackward {
                previous,
                actual: envelope.sequence,
            });
        }
        self.last_sequence = Some(envelope.sequence);
        self.first_observed_micros
            .get_or_insert(envelope.time.observed_micros);
        self.last_observed_micros = Some(envelope.time.observed_micros);
        Ok(())
    }

    fn observe_profile(
        &mut self,
        envelope: &EventEnvelope,
        profile: &CharacterProfilePatch,
    ) -> Result<(), FactorCorrelationError> {
        let Some(cultivation) = &profile.season_cultivation else {
            return Ok(());
        };
        let current_season_id = profile
            .season
            .as_ref()
            .and_then(|season| season.season_id)
            .and_then(|season_id| i32::try_from(season_id).ok());
        let selected_season = current_season_id
            .and_then(|season_id| {
                cultivation
                    .iter()
                    .find(|entry| entry.season_id == season_id)
            })
            .or_else(|| cultivation.iter().max_by_key(|entry| entry.season_id));
        let Some(selected_season) = selected_season else {
            return Ok(());
        };

        let selected_item_ids = selected_season
            .lines
            .iter()
            .flat_map(|line| &line.areas)
            .filter(|area| area.active == Some(true))
            .flat_map(|area| area.middle_node_item_ids.values().copied())
            .collect::<BTreeSet<_>>();
        let mut selected = Vec::new();
        let mut unreviewed = Vec::new();
        let mut unmapped = Vec::new();
        let mut formula_inputs = Vec::new();
        let runtime_rules_enabled =
            psychoscope_factor_runtime_rules_enabled().map_err(FactorCorrelationError::Catalog)?;
        for item_id in selected_item_ids {
            if dreamscope_factor_item_by_id(item_id).is_none() {
                unmapped.push(item_id);
                continue;
            }

            selected.push(item_id);
            if let Some(factor) =
                psychoscope_factor_by_item_id(item_id).map_err(FactorCorrelationError::Catalog)?
                && runtime_rules_enabled
            {
                formula_inputs.extend(factor.stat_modifiers.iter().map(|modifier| {
                    SelectedFactorFormulaInput {
                        factor_item_id: item_id,
                        stat: modifier.stat,
                        attribute_id: modifier.attribute_id,
                        value: modifier.value,
                        unit: modifier.unit,
                        role: modifier.formula_input_role,
                        credit_policy: modifier.credit_policy,
                    }
                }));
            } else {
                unreviewed.push(item_id);
            }
        }
        let character_id = profile.character.character_id.clone();
        self.selections_by_character.insert(
            character_id.clone(),
            ActiveFactorSelection {
                character_id: character_id.clone(),
                selected_factor_item_ids: selected.iter().copied().collect(),
            },
        );
        self.selection_observations
            .push(FactorSelectionObservation {
                sequence: envelope.sequence,
                observed_micros: envelope.time.observed_micros,
                character_id,
                season_id: Some(selected_season.season_id),
                selected_factor_item_ids: selected,
                unreviewed_factor_item_ids: unreviewed,
                unmapped_factor_item_ids: unmapped,
                formula_inputs,
            });
        Ok(())
    }

    fn observe_status(
        &mut self,
        envelope: &EventEnvelope,
        status: &StatusEvent,
    ) -> Result<(), FactorCorrelationError> {
        let effect_id = status.effect.0;
        let static_candidates = factor_item_candidates_for_effect(effect_id)?;
        if static_candidates.is_empty() {
            return Ok(());
        }

        if status.state == StatusState::Applied {
            if let Some(window_id) = self.find_window(status) {
                self.close_window(
                    window_id,
                    Some(envelope.sequence),
                    envelope.time.observed_micros,
                    FactorWindowCloseReason::Reapplied,
                );
            }
            self.open_window(envelope, status, static_candidates, false)?;
            return Ok(());
        }

        let (window_id, opened_window) = if let Some(window_id) = self.find_window(status) {
            (window_id, false)
        } else {
            let window_id = self.open_window(envelope, status, static_candidates, true)?;
            self.record_unmatched_lifecycle(envelope, status, "opened_from_non_apply_state")?;
            (window_id, true)
        };

        if !opened_window {
            self.update_window(window_id, envelope, status)?;
        }
        if status.state == StatusState::Removed {
            self.close_window(
                window_id,
                Some(envelope.sequence),
                envelope.time.observed_micros,
                FactorWindowCloseReason::Removed,
            );
        } else if status.state == StatusState::Consumed && status.stacks == Some(0) {
            self.close_window(
                window_id,
                Some(envelope.sequence),
                envelope.time.observed_micros,
                FactorWindowCloseReason::ConsumedAtZeroStacks,
            );
        }
        Ok(())
    }

    fn open_window(
        &mut self,
        envelope: &EventEnvelope,
        status: &StatusEvent,
        static_candidates: BTreeSet<i64>,
        opened_from_non_apply_state: bool,
    ) -> Result<u64, FactorCorrelationError> {
        if self.active_windows.len() + self.closed_windows.len() >= MAXIMUM_CORRELATION_WINDOWS {
            return Err(FactorCorrelationError::WindowLimitExceeded {
                limit: MAXIMUM_CORRELATION_WINDOWS,
            });
        }
        let source_selection = status
            .source
            .as_ref()
            .and_then(|source| self.selection_for_entity(source))
            .filter(|selection| {
                !selection
                    .selected_factor_item_ids
                    .is_disjoint(&static_candidates)
            });
        let recipient_selection = self
            .selection_for_entity(&status.target)
            .filter(|selection| {
                !selection
                    .selected_factor_item_ids
                    .is_disjoint(&static_candidates)
            });
        let selection_evidence = match (source_selection, recipient_selection) {
            (Some(_), Some(_)) => FactorSelectionEvidence::SourceAndRecipientOwnFactor,
            (Some(_), None) => FactorSelectionEvidence::SourceOwnsFactor,
            (None, Some(_)) => FactorSelectionEvidence::RecipientOwnsFactor,
            (None, None) => FactorSelectionEvidence::StaticCatalogOnly,
        };
        let factor_item_ids = match (source_selection, recipient_selection) {
            (None, None) => static_candidates.iter().copied().collect(),
            _ => source_selection
                .into_iter()
                .chain(recipient_selection)
                .flat_map(|selection| selection.selected_factor_item_ids.iter().copied())
                .filter(|item_id| static_candidates.contains(item_id))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
        };
        let selected_owner_character_ids = source_selection
            .into_iter()
            .chain(recipient_selection)
            .map(|selection| selection.character_id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let window_id = self.next_window_id;
        self.next_window_id = self.next_window_id.saturating_add(1);
        let mut window = ActiveWindow {
            report: FactorCorrelationWindow {
                window_id,
                effect_id: status.effect.0,
                instance_id: status.instance_id.map(|instance| instance.0),
                factor_item_ids,
                selection_evidence,
                selected_owner_character_ids,
                provider_entity_uuid: status.source.map(|source| source.entity_uuid.0),
                recipient_entity_uuid: status.target.entity_uuid.0,
                opened_sequence: envelope.sequence,
                opened_observed_micros: envelope.time.observed_micros,
                opened_from_non_apply_state,
                closed_sequence: None,
                closed_observed_micros: None,
                close_reason: None,
                lifecycle: Vec::new(),
                dropped_lifecycle_samples: 0,
                minimum_observed_stacks: None,
                maximum_observed_stacks: None,
                apply_count: 0,
                refresh_count: 0,
                stack_count: 0,
                consume_count: 0,
                remove_count: 0,
                refresh_without_stack_change_count: 0,
                recipient_outgoing_damage: FactorDamageTotals::default(),
                recipient_incoming_damage: FactorDamageTotals::default(),
                provider_outgoing_damage: FactorDamageTotals::default(),
                action_damage: Vec::new(),
                resource_baselines: factor_resource_baselines(
                    status.source.map(|source| source.entity_uuid.0),
                    status.target.entity_uuid.0,
                    &self.resource_state_by_entity_uuid,
                ),
                resource_transitions: Vec::new(),
            },
            expiration_observed_micros: None,
            action_damage: BTreeMap::new(),
            last_observed_stacks: None,
        };
        apply_lifecycle_sample(&mut window, envelope, status)?;
        self.active_windows.insert(window_id, window);
        Ok(window_id)
    }

    fn update_window(
        &mut self,
        window_id: u64,
        envelope: &EventEnvelope,
        status: &StatusEvent,
    ) -> Result<(), FactorCorrelationError> {
        if let Some(window) = self.active_windows.get_mut(&window_id) {
            apply_lifecycle_sample(window, envelope, status)?;
        }
        Ok(())
    }

    fn find_window(&self, status: &StatusEvent) -> Option<u64> {
        let instance_id = status.instance_id.map(|instance| instance.0);
        self.active_windows
            .iter()
            .filter(|(_, window)| {
                window.report.effect_id == status.effect.0
                    && window.report.recipient_entity_uuid == status.target.entity_uuid.0
                    && match instance_id {
                        Some(instance_id) => window.report.instance_id == Some(instance_id),
                        None => status.source.is_none_or(|source| {
                            window.report.provider_entity_uuid == Some(source.entity_uuid.0)
                        }),
                    }
            })
            .max_by_key(|(_, window)| window.report.opened_sequence)
            .map(|(window_id, _)| *window_id)
    }

    fn observe_damage(
        &mut self,
        observed_micros: u64,
        damage: &DamageEvent,
    ) -> Result<(), FactorCorrelationError> {
        let ability_id = damage.ability.map(|ability| ability.0);
        if let Some(ability_id) = ability_id
            && !self.recount_groups_by_ability.contains_key(&ability_id)
        {
            let group_ids = factor_recount_groups_for_ability(ability_id)?;
            self.recount_groups_by_ability.insert(ability_id, group_ids);
        }
        let recount_group_ids =
            ability_id.and_then(|ability_id| self.recount_groups_by_ability.get(&ability_id));
        for window in self.active_windows.values_mut() {
            let provider = window.report.provider_entity_uuid;
            let recipient = window.report.recipient_entity_uuid;
            let source = damage.source.entity_uuid.0;
            let target = damage.target.entity_uuid.0;
            if source == recipient {
                window
                    .report
                    .recipient_outgoing_damage
                    .observe(observed_micros, damage.amount);
            }
            if target == recipient {
                window
                    .report
                    .recipient_incoming_damage
                    .observe(observed_micros, damage.amount);
            }
            if provider == Some(source) {
                window
                    .report
                    .provider_outgoing_damage
                    .observe(observed_micros, damage.amount);
            }
            let Some(ability_id) = ability_id else {
                continue;
            };
            let Some(recount_group_ids) = recount_group_ids else {
                continue;
            };
            let actor_relation = match (provider == Some(source), source == recipient) {
                (true, true) => Some(FactorDamageActorRelation::ProviderAndRecipient),
                (true, false) => Some(FactorDamageActorRelation::Provider),
                (false, true) => Some(FactorDamageActorRelation::Recipient),
                (false, false) => None,
            };
            let Some(actor_relation) = actor_relation else {
                continue;
            };
            for factor_item_id in window.report.factor_item_ids.iter().copied() {
                let Some(factor) = psychoscope_factor_by_item_id(factor_item_id)
                    .map_err(FactorCorrelationError::Catalog)?
                else {
                    continue;
                };
                for (relation_index, relation) in factor.action_relations.iter().enumerate() {
                    let roles = [
                        (
                            FactorActionDamageRole::Trigger,
                            relation.trigger_recount_group_id,
                        ),
                        (
                            FactorActionDamageRole::Target,
                            relation.target_recount_group_id,
                        ),
                    ];
                    for (action_role, expected_recount_group_id) in roles {
                        let Some(recount_group_id) = expected_recount_group_id else {
                            continue;
                        };
                        if !recount_group_ids.contains(&recount_group_id) {
                            continue;
                        }
                        let key = ActionDamageKey {
                            factor_item_id,
                            relation_index,
                            action_role: action_role_code(action_role),
                            actor_relation: actor_relation_code(actor_relation),
                            ability_id,
                            recount_group_id,
                        };
                        window
                            .action_damage
                            .entry(key)
                            .or_insert_with(|| FactorActionDamageAggregate {
                                factor_item_id,
                                relation_index,
                                relation_kind: relation.kind,
                                action_role,
                                actor_relation,
                                ability_id,
                                recount_group_id,
                                totals: FactorDamageTotals::default(),
                            })
                            .totals
                            .observe(observed_micros, damage.amount);
                    }
                }
            }
        }
        Ok(())
    }

    fn observe_resource(
        &mut self,
        envelope: &EventEnvelope,
        resource: &ResourceEvent,
    ) -> Result<(), FactorCorrelationError> {
        let actor_entity_uuid = resource.actor.entity_uuid.0;
        let previous = self
            .resource_state_by_entity_uuid
            .get(&actor_entity_uuid)
            .cloned();
        let next = apply_resource_update(previous.as_ref(), resource);
        let comparable = previous
            .as_ref()
            .zip(next.as_ref())
            .filter(|(before, after)| before.complete_snapshot && after.complete_snapshot);

        for window in self.active_windows.values_mut() {
            let Some(actor_relation) = factor_resource_actor_relation(
                window.report.provider_entity_uuid,
                window.report.recipient_entity_uuid,
                actor_entity_uuid,
            ) else {
                continue;
            };
            if window.report.resource_transitions.len() >= MAXIMUM_RESOURCE_SAMPLES_PER_WINDOW {
                return Err(FactorCorrelationError::ResourceSampleLimitExceeded {
                    window_id: window.report.window_id,
                    limit: MAXIMUM_RESOURCE_SAMPLES_PER_WINDOW,
                });
            }
            window
                .report
                .resource_transitions
                .push(FactorResourceTransitionSample {
                    sequence: envelope.sequence,
                    observed_micros: envelope.time.observed_micros,
                    actor_entity_uuid,
                    actor_relation,
                    update_kind: resource.update_kind,
                    origin_energy_raw_bits: resource.origin_energy_raw_bits,
                    resource_ids: resource.resource_ids.clone(),
                    resource_values: resource.resource_values.clone(),
                    cooldowns: resource.cooldowns.clone(),
                    origin_energy_changed: comparable.map(|(before, after)| {
                        before.origin_energy_raw_bits != after.origin_energy_raw_bits
                    }),
                    resource_ids_changed: comparable
                        .map(|(before, after)| before.resource_ids != after.resource_ids),
                    resource_values_changed: comparable
                        .map(|(before, after)| before.resource_values != after.resource_values),
                    cooldowns_changed: comparable
                        .map(|(before, after)| before.cooldowns != after.cooldowns),
                    complete_state_after: next
                        .as_ref()
                        .is_some_and(|state| state.complete_snapshot),
                });
        }

        if let Some(next) = next {
            self.resource_state_by_entity_uuid
                .insert(actor_entity_uuid, next);
        } else {
            // An unknown update kind makes the prior reconstructed state
            // unsafe. The raw transition remains in every matching window.
            self.resource_state_by_entity_uuid
                .remove(&actor_entity_uuid);
        }
        Ok(())
    }

    fn selection_for_entity(&self, entity: &EntityRef) -> Option<&ActiveFactorSelection> {
        let character_id = character_id_from_entity_uuid(entity.entity_uuid.0)?;
        self.selections_by_character.get(&character_id)
    }

    fn expire_windows_before(
        &mut self,
        observed_micros: u64,
    ) -> Result<(), FactorCorrelationError> {
        let expired = self
            .active_windows
            .iter()
            .filter_map(|(window_id, window)| {
                window
                    .expiration_observed_micros
                    .filter(|expiration| *expiration < observed_micros)
                    .map(|expiration| (*window_id, expiration))
            })
            .collect::<Vec<_>>();
        for (window_id, expiration) in expired {
            self.close_window(
                window_id,
                None,
                expiration,
                FactorWindowCloseReason::DurationElapsed,
            );
        }
        Ok(())
    }

    fn close_window(
        &mut self,
        window_id: u64,
        sequence: Option<u64>,
        observed_micros: u64,
        reason: FactorWindowCloseReason,
    ) {
        let Some(mut window) = self.active_windows.remove(&window_id) else {
            return;
        };
        window.report.closed_sequence = sequence;
        window.report.closed_observed_micros = Some(observed_micros);
        window.report.close_reason = Some(reason);
        window.report.action_damage = window.action_damage.into_values().collect();
        self.closed_windows.push(window.report);
    }

    fn record_unmatched_lifecycle(
        &mut self,
        envelope: &EventEnvelope,
        status: &StatusEvent,
        reason: &str,
    ) -> Result<(), FactorCorrelationError> {
        if self.unmatched_lifecycle_events.len() >= MAXIMUM_UNMATCHED_LIFECYCLE_EVENTS {
            return Err(FactorCorrelationError::UnmatchedLifecycleLimitExceeded {
                limit: MAXIMUM_UNMATCHED_LIFECYCLE_EVENTS,
            });
        }
        self.unmatched_lifecycle_events
            .push(UnmatchedFactorLifecycleEvent {
                sequence: envelope.sequence,
                observed_micros: envelope.time.observed_micros,
                effect_id: status.effect.0,
                instance_id: status.instance_id.map(|instance| instance.0),
                provider_entity_uuid: status.source.map(|source| source.entity_uuid.0),
                recipient_entity_uuid: status.target.entity_uuid.0,
                state: status.state,
                stacks: status.stacks,
                level: status.level,
                part_id: status.part_id,
                count: status.count,
                reason: reason.into(),
            });
        Ok(())
    }
}

/// Returns every exact-build item/grade identity that can terminate at this
/// packet-observed effect. The reviewed attribution catalog is retained as a
/// fallback for explicit runtime-only routes, but it must not narrow an exact
/// family to the one or two grades whose mechanics have already been reviewed.
///
/// This is identity evidence only. Damage/action correlation below still uses
/// `psychoscope_factor_by_item_id`, so unreviewed grades cannot become formula
/// inputs or rDPS credit merely because their selection is now preserved.
fn factor_item_candidates_for_effect(
    effect_id: i64,
) -> Result<BTreeSet<i64>, FactorCorrelationError> {
    let mut candidates = psychoscope_factor_rules()
        .map_err(FactorCorrelationError::Catalog)?
        .iter()
        .filter(|factor| factor.primary_buff_id == Some(effect_id))
        .map(|factor| factor.item_id)
        .collect::<BTreeSet<_>>();
    candidates.extend(
        dreamscope_candidates_for_terminal_effect(effect_id)
            .iter()
            .filter(|candidate| candidate.source_kind == DreamscopeSourceKind::FactorFamily)
            .flat_map(|candidate| candidate.item_ids.iter().copied()),
    );
    Ok(candidates)
}

fn factor_recount_groups_for_ability(
    ability_id: i64,
) -> Result<BTreeSet<i64>, FactorCorrelationError> {
    let mut group_ids = BTreeSet::new();
    if let Some(group_id) = combat_action_presentation(ability_id)
        .map_err(FactorCorrelationError::Catalog)?
        .and_then(|action| action.recount_group_id)
    {
        group_ids.insert(group_id);
    }

    let referenced_group_ids = psychoscope_factor_rules()
        .map_err(FactorCorrelationError::Catalog)?
        .iter()
        .flat_map(|factor| {
            factor
                .action_relations
                .iter()
                .flat_map(|relation| {
                    [
                        relation.trigger_recount_group_id,
                        relation.target_recount_group_id,
                    ]
                })
                .chain(
                    factor
                        .energy_relations
                        .iter()
                        .map(|relation| relation.trigger_recount_group_id),
                )
        })
        .flatten()
        .collect::<BTreeSet<_>>();
    for group_id in referenced_group_ids {
        let Some(parent) =
            crate::psychoscope_recount_parent(group_id).map_err(FactorCorrelationError::Catalog)?
        else {
            continue;
        };
        if parent.activation_ability_ids.contains(&ability_id)
            || parent.observed_child_action_ids.contains(&ability_id)
            || parent.damage_ids.contains(&ability_id)
        {
            group_ids.insert(group_id);
        }
    }
    Ok(group_ids)
}

fn factor_resource_actor_relation(
    provider_entity_uuid: Option<i64>,
    recipient_entity_uuid: i64,
    actor_entity_uuid: i64,
) -> Option<FactorResourceActorRelation> {
    match (
        provider_entity_uuid == Some(actor_entity_uuid),
        recipient_entity_uuid == actor_entity_uuid,
    ) {
        (true, true) => Some(FactorResourceActorRelation::ProviderAndRecipient),
        (true, false) => Some(FactorResourceActorRelation::Provider),
        (false, true) => Some(FactorResourceActorRelation::Recipient),
        (false, false) => None,
    }
}

fn factor_resource_baselines(
    provider_entity_uuid: Option<i64>,
    recipient_entity_uuid: i64,
    states: &BTreeMap<i64, FactorResourceWireState>,
) -> Vec<FactorResourceBaseline> {
    let mut actors = BTreeSet::new();
    if let Some(provider_entity_uuid) = provider_entity_uuid {
        actors.insert(provider_entity_uuid);
    }
    actors.insert(recipient_entity_uuid);
    actors
        .into_iter()
        .filter_map(|actor_entity_uuid| {
            factor_resource_actor_relation(
                provider_entity_uuid,
                recipient_entity_uuid,
                actor_entity_uuid,
            )
            .map(|actor_relation| FactorResourceBaseline {
                actor_entity_uuid,
                actor_relation,
                state_before_window: states.get(&actor_entity_uuid).cloned(),
            })
        })
        .collect()
}

fn apply_resource_update(
    previous: Option<&FactorResourceWireState>,
    resource: &ResourceEvent,
) -> Option<FactorResourceWireState> {
    match resource.update_kind {
        EntityAttributeUpdateKind::Unknown => None,
        EntityAttributeUpdateKind::Snapshot => Some(FactorResourceWireState {
            complete_snapshot: true,
            origin_energy_raw_bits: resource.origin_energy_raw_bits,
            resource_ids: resource.resource_ids.clone(),
            resource_values: resource.resource_values.clone(),
            cooldowns: resource.cooldowns.clone(),
        }),
        EntityAttributeUpdateKind::Delta => {
            let mut next = previous.cloned().unwrap_or_default();
            if resource.origin_energy_raw_bits.is_some() {
                next.origin_energy_raw_bits = resource.origin_energy_raw_bits;
            }
            // Decoder-empty vectors mean the corresponding repeated wire field
            // was absent on this delta. The raw event above is still retained
            // verbatim, including unequal ID/value lengths.
            if !resource.resource_ids.is_empty() {
                next.resource_ids = resource.resource_ids.clone();
            }
            if !resource.resource_values.is_empty() {
                next.resource_values = resource.resource_values.clone();
            }
            if !resource.cooldowns.is_empty() {
                next.cooldowns = resource.cooldowns.clone();
            }
            Some(next)
        }
    }
}

fn apply_lifecycle_sample(
    window: &mut ActiveWindow,
    envelope: &EventEnvelope,
    status: &StatusEvent,
) -> Result<(), FactorCorrelationError> {
    if window.report.lifecycle.len() >= MAXIMUM_LIFECYCLE_SAMPLES_PER_WINDOW {
        return Err(FactorCorrelationError::LifecycleSampleLimitExceeded {
            window_id: window.report.window_id,
            limit: MAXIMUM_LIFECYCLE_SAMPLES_PER_WINDOW,
        });
    }
    window.report.lifecycle.push(FactorLifecycleSample {
        sequence: envelope.sequence,
        observed_micros: envelope.time.observed_micros,
        state: status.state,
        stacks: status.stacks,
        duration_millis: status.duration_millis,
        level: status.level,
        part_id: status.part_id,
        count: status.count,
    });
    match status.state {
        StatusState::Applied => window.report.apply_count += 1,
        StatusState::Refreshed => {
            window.report.refresh_count += 1;
            if status.stacks.is_none() || status.stacks == window.last_observed_stacks {
                window.report.refresh_without_stack_change_count += 1;
            }
        }
        StatusState::Stacked => window.report.stack_count += 1,
        StatusState::Consumed => window.report.consume_count += 1,
        StatusState::Removed => window.report.remove_count += 1,
    }
    if let Some(stacks) = status.stacks {
        window.report.minimum_observed_stacks = Some(
            window
                .report
                .minimum_observed_stacks
                .map_or(stacks, |current| current.min(stacks)),
        );
        window.report.maximum_observed_stacks = Some(
            window
                .report
                .maximum_observed_stacks
                .map_or(stacks, |current| current.max(stacks)),
        );
        window.last_observed_stacks = Some(stacks);
    }
    if let Some(duration_millis) = status.duration_millis {
        window.expiration_observed_micros = Some(
            envelope
                .time
                .observed_micros
                .saturating_add(duration_millis.saturating_mul(1_000)),
        );
    }
    Ok(())
}

fn summarize_rules(windows: &[FactorCorrelationWindow]) -> Vec<FactorRuleCorrelationSummary> {
    let mut summaries = BTreeMap::<i64, FactorRuleCorrelationSummary>::new();
    for window in windows {
        for factor_item_id in &window.factor_item_ids {
            let summary =
                summaries
                    .entry(*factor_item_id)
                    .or_insert_with(|| FactorRuleCorrelationSummary {
                        factor_item_id: *factor_item_id,
                        effect_id: window.effect_id,
                        window_count: 0,
                        exact_source_owner_window_count: 0,
                        exact_recipient_owner_window_count: 0,
                        static_catalog_only_window_count: 0,
                        applied_count: 0,
                        refreshed_count: 0,
                        stacked_count: 0,
                        consumed_count: 0,
                        removed_count: 0,
                        minimum_observed_stacks: None,
                        maximum_observed_stacks: None,
                        overlapping_window_pairs: 0,
                        maximum_concurrent_instances_per_recipient: 0,
                        maximum_concurrent_distinct_providers_per_recipient: 0,
                        recipient_outgoing_damage: FactorDamageTotals::default(),
                        recipient_incoming_damage: FactorDamageTotals::default(),
                        provider_outgoing_damage: FactorDamageTotals::default(),
                        matched_action_damage: FactorDamageTotals::default(),
                        resource_transition_count: 0,
                        provider_resource_transition_count: 0,
                        recipient_resource_transition_count: 0,
                        provider_and_recipient_resource_transition_count: 0,
                        origin_energy_change_count: 0,
                        resource_ids_change_count: 0,
                        resource_values_change_count: 0,
                        cooldown_change_count: 0,
                        incomplete_state_after_count: 0,
                        distinct_resource_ids: Vec::new(),
                        distinct_cooldown_resource_ids: Vec::new(),
                        attribution_enabled: false,
                    });
            summary.window_count += 1;
            match window.selection_evidence {
                FactorSelectionEvidence::SourceOwnsFactor => {
                    summary.exact_source_owner_window_count += 1
                }
                FactorSelectionEvidence::RecipientOwnsFactor => {
                    summary.exact_recipient_owner_window_count += 1
                }
                FactorSelectionEvidence::SourceAndRecipientOwnFactor => {
                    summary.exact_source_owner_window_count += 1;
                    summary.exact_recipient_owner_window_count += 1;
                }
                FactorSelectionEvidence::StaticCatalogOnly => {
                    summary.static_catalog_only_window_count += 1
                }
            }
            summary.applied_count += window.apply_count;
            summary.refreshed_count += window.refresh_count;
            summary.stacked_count += window.stack_count;
            summary.consumed_count += window.consume_count;
            summary.removed_count += window.remove_count;
            merge_stack_bounds(summary, window);
            merge_damage_totals(
                &mut summary.recipient_outgoing_damage,
                &window.recipient_outgoing_damage,
            );
            merge_damage_totals(
                &mut summary.recipient_incoming_damage,
                &window.recipient_incoming_damage,
            );
            merge_damage_totals(
                &mut summary.provider_outgoing_damage,
                &window.provider_outgoing_damage,
            );
            for action in window
                .action_damage
                .iter()
                .filter(|action| action.factor_item_id == *factor_item_id)
            {
                merge_damage_totals(&mut summary.matched_action_damage, &action.totals);
            }
            let mut resource_ids = summary
                .distinct_resource_ids
                .iter()
                .copied()
                .collect::<BTreeSet<_>>();
            let mut cooldown_resource_ids = summary
                .distinct_cooldown_resource_ids
                .iter()
                .copied()
                .collect::<BTreeSet<_>>();
            for transition in &window.resource_transitions {
                summary.resource_transition_count += 1;
                match transition.actor_relation {
                    FactorResourceActorRelation::Provider => {
                        summary.provider_resource_transition_count += 1
                    }
                    FactorResourceActorRelation::Recipient => {
                        summary.recipient_resource_transition_count += 1
                    }
                    FactorResourceActorRelation::ProviderAndRecipient => {
                        summary.provider_and_recipient_resource_transition_count += 1
                    }
                }
                summary.origin_energy_change_count +=
                    u64::from(transition.origin_energy_changed == Some(true));
                summary.resource_ids_change_count +=
                    u64::from(transition.resource_ids_changed == Some(true));
                summary.resource_values_change_count +=
                    u64::from(transition.resource_values_changed == Some(true));
                summary.cooldown_change_count +=
                    u64::from(transition.cooldowns_changed == Some(true));
                summary.incomplete_state_after_count += u64::from(!transition.complete_state_after);
                resource_ids.extend(transition.resource_ids.iter().copied());
                cooldown_resource_ids.extend(
                    transition
                        .cooldowns
                        .iter()
                        .filter_map(|cooldown| cooldown.resource_id),
                );
            }
            summary.distinct_resource_ids = resource_ids.into_iter().collect();
            summary.distinct_cooldown_resource_ids = cooldown_resource_ids.into_iter().collect();
        }
    }

    for (factor_item_id, summary) in &mut summaries {
        let factor_windows = windows
            .iter()
            .filter(|window| window.factor_item_ids.contains(factor_item_id))
            .collect::<Vec<_>>();
        for left_index in 0..factor_windows.len() {
            for right in factor_windows.iter().skip(left_index + 1) {
                let left = factor_windows[left_index];
                if left.recipient_entity_uuid == right.recipient_entity_uuid
                    && windows_overlap(left, right)
                {
                    summary.overlapping_window_pairs += 1;
                }
            }
        }
        let recipients = factor_windows
            .iter()
            .map(|window| window.recipient_entity_uuid)
            .collect::<BTreeSet<_>>();
        for recipient in recipients {
            let mut recipient_windows = factor_windows
                .iter()
                .copied()
                .filter(|window| window.recipient_entity_uuid == recipient)
                .collect::<Vec<_>>();
            recipient_windows.sort_by_key(|window| window.opened_observed_micros);
            for (index, current) in recipient_windows.iter().enumerate() {
                let concurrent = recipient_windows[..=index]
                    .iter()
                    .copied()
                    .filter(|candidate| {
                        candidate.opened_observed_micros <= current.opened_observed_micros
                            && candidate.closed_observed_micros.unwrap_or(u64::MAX)
                                > current.opened_observed_micros
                    })
                    .collect::<Vec<_>>();
                summary.maximum_concurrent_instances_per_recipient = summary
                    .maximum_concurrent_instances_per_recipient
                    .max(u32::try_from(concurrent.len()).unwrap_or(u32::MAX));
                let providers = concurrent
                    .iter()
                    .filter_map(|window| window.provider_entity_uuid)
                    .collect::<BTreeSet<_>>();
                summary.maximum_concurrent_distinct_providers_per_recipient = summary
                    .maximum_concurrent_distinct_providers_per_recipient
                    .max(u32::try_from(providers.len()).unwrap_or(u32::MAX));
            }
        }
    }
    summaries.into_values().collect()
}

fn merge_stack_bounds(
    summary: &mut FactorRuleCorrelationSummary,
    window: &FactorCorrelationWindow,
) {
    if let Some(stacks) = window.minimum_observed_stacks {
        summary.minimum_observed_stacks = Some(
            summary
                .minimum_observed_stacks
                .map_or(stacks, |current| current.min(stacks)),
        );
    }
    if let Some(stacks) = window.maximum_observed_stacks {
        summary.maximum_observed_stacks = Some(
            summary
                .maximum_observed_stacks
                .map_or(stacks, |current| current.max(stacks)),
        );
    }
}

fn merge_damage_totals(target: &mut FactorDamageTotals, source: &FactorDamageTotals) {
    target.event_count = target.event_count.saturating_add(source.event_count);
    target.amount = target.amount.saturating_add(source.amount);
    target.first_observed_micros =
        match (target.first_observed_micros, source.first_observed_micros) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (left, right) => left.or(right),
        };
    target.last_observed_micros = match (target.last_observed_micros, source.last_observed_micros) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (left, right) => left.or(right),
    };
}

fn windows_overlap(left: &FactorCorrelationWindow, right: &FactorCorrelationWindow) -> bool {
    let left_end = left.closed_observed_micros.unwrap_or(u64::MAX);
    let right_end = right.closed_observed_micros.unwrap_or(u64::MAX);
    left.opened_observed_micros < right_end && right.opened_observed_micros < left_end
}

const fn action_role_code(role: FactorActionDamageRole) -> u8 {
    match role {
        FactorActionDamageRole::Trigger => 0,
        FactorActionDamageRole::Target => 1,
    }
}

const fn actor_relation_code(relation: FactorDamageActorRelation) -> u8 {
    match relation {
        FactorDamageActorRelation::Provider => 0,
        FactorDamageActorRelation::Recipient => 1,
        FactorDamageActorRelation::ProviderAndRecipient => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rlogs_events::{
        AbilityId, ActorId, CharacterIdentity, DamageFlags, EntityUuid, EventProvenance,
        EventSensitivity, EventTime, GameProfileEvent, RegionContext, RegionIdentity,
        StatusEffectId, StatusEffectInstanceId, TimelineEvent,
    };

    fn entity(actor_id: u64, character_id: u64) -> EntityRef {
        EntityRef {
            actor_id: ActorId(actor_id),
            entity_uuid: EntityUuid(i64::try_from(character_id << 16).unwrap()),
        }
    }

    fn region() -> RegionContext {
        RegionContext {
            identity: RegionIdentity {
                deployment_id: "global".into(),
                region_id: "global".into(),
                realm_id: None,
                world_id: None,
            },
            client_build: dreamscope_catalog_game_build().into(),
            protocol_pack_digest: "test".into(),
            evidence: vec![],
        }
    }

    fn envelope(sequence: u64, observed_micros: u64, event: CanonicalEvent) -> EventEnvelope {
        EventEnvelope {
            schema_version: rlogs_events::EVENT_SCHEMA_VERSION,
            session_id: "factor-correlation-test".into(),
            sequence,
            region: region(),
            time: EventTime {
                observed_micros,
                game_time_millis: None,
            },
            provenance: EventProvenance::manual("unit test"),
            sensitivity: EventSensitivity::PersonalGameplay,
            event,
        }
    }

    fn profile_event(sequence: u64, observed_micros: u64, item_id: i64) -> EventEnvelope {
        let character = CharacterIdentity {
            region: region().identity,
            character_id: "3296036".into(),
        };
        let profile = CharacterProfilePatch {
            character: character.clone(),
            display_name: None,
            display_id: None,
            server_id: None,
            class_id: Some(11),
            specialization_id: Some(117),
            level: Some(60),
            progression: None,
            combat_power: None,
            combat_power_breakdown: None,
            combat_stats: None,
            season_strength: None,
            master_score: None,
            season: Some(crate::SeasonProfile {
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
            season_cultivation: Some(vec![crate::SeasonCultivationProfile {
                season_id: 3,
                lines: vec![crate::CultivationLineProfile {
                    line_type_id: 800_522,
                    area_ids: vec![8],
                    areas: vec![crate::CultivationAreaProfile {
                        area_id: 8,
                        active: Some(true),
                        active_effect_score: Some(56),
                        normal_node_levels: BTreeMap::new(),
                        middle_node_item_ids: BTreeMap::from([(178, item_id)]),
                        big_node_fantasy_ids: BTreeMap::new(),
                    }],
                }],
            }]),
            reputations: None,
            current_profession_project_id: None,
            social_display: None,
        };
        let payload = serde_json::to_value(profile).unwrap();
        envelope(
            sequence,
            observed_micros,
            CanonicalEvent::CharacterProfileObserved {
                profile: Box::new(GameProfileEvent {
                    game_plugin_id: BPSR_GAME_PLUGIN_ID.into(),
                    payload_schema_id: BPSR_PROFILE_SCHEMA_ID.into(),
                    payload_schema_version: BPSR_PROFILE_SCHEMA_VERSION,
                    character,
                    payload,
                }),
            },
        )
    }

    fn status_event(
        sequence: u64,
        observed_micros: u64,
        instance_id: i64,
        state: StatusState,
        stacks: Option<u32>,
    ) -> EventEnvelope {
        let owner = entity(1, 3_296_036);
        status_event_between(
            sequence,
            observed_micros,
            Some(owner),
            owner,
            3_053_100,
            instance_id,
            state,
            stacks,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn status_event_between(
        sequence: u64,
        observed_micros: u64,
        source: Option<EntityRef>,
        target: EntityRef,
        effect_id: i64,
        instance_id: i64,
        state: StatusState,
        stacks: Option<u32>,
    ) -> EventEnvelope {
        envelope(
            sequence,
            observed_micros,
            CanonicalEvent::Timeline(TimelineEvent {
                sequence,
                time: EventTime {
                    observed_micros,
                    game_time_millis: None,
                },
                provenance: EventProvenance::manual("unit test"),
                kind: TimelineEventKind::Status(StatusEvent {
                    source,
                    target,
                    effect: StatusEffectId(effect_id),
                    instance_id: Some(StatusEffectInstanceId(instance_id)),
                    origin: None,
                    state,
                    stacks,
                    level: None,
                    part_id: None,
                    count: None,
                    created_at_millis: None,
                    duration_millis: Some(10_000),
                }),
            }),
        )
    }

    fn damage_event(sequence: u64, observed_micros: u64, ability_id: i64) -> EventEnvelope {
        let owner = entity(1, 3_296_036);
        envelope(
            sequence,
            observed_micros,
            CanonicalEvent::Timeline(TimelineEvent {
                sequence,
                time: EventTime {
                    observed_micros,
                    game_time_millis: None,
                },
                provenance: EventProvenance::manual("unit test"),
                kind: TimelineEventKind::Damage(DamageEvent {
                    source: owner,
                    direct_source: None,
                    target: entity(2, 9_999_999),
                    ability: Some(AbilityId(ability_id)),
                    amount: 1_000,
                    actual_amount: None,
                    hp_loss: None,
                    shield_loss: None,
                    hit_event_id: None,
                    damage_source: None,
                    damage_type: None,
                    flags: DamageFlags::default(),
                    packet: Default::default(),
                }),
            }),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn resource_event(
        sequence: u64,
        observed_micros: u64,
        actor: EntityRef,
        update_kind: EntityAttributeUpdateKind,
        origin_energy_raw_bits: Option<u32>,
        resource_ids: Vec<u32>,
        resource_values: Vec<u32>,
        cooldowns: Vec<ResourceCooldown>,
    ) -> EventEnvelope {
        envelope(
            sequence,
            observed_micros,
            CanonicalEvent::Timeline(TimelineEvent {
                sequence,
                time: EventTime {
                    observed_micros,
                    game_time_millis: None,
                },
                provenance: EventProvenance::manual("unit test"),
                kind: TimelineEventKind::Resource(ResourceEvent {
                    actor,
                    update_kind,
                    origin_energy_raw_bits,
                    resource_ids,
                    resource_values,
                    cooldowns,
                }),
            }),
        )
    }

    #[test]
    fn correlates_exact_selection_lifecycle_stacks_and_recount_damage() {
        let action_id = crate::psychoscope_recount_parent(94)
            .unwrap()
            .unwrap()
            .damage_ids[0];
        let mut analyzer = PsychoscopeFactorCorrelationAnalyzer::new();
        for event in [
            profile_event(1, 1_000, 20_020_427),
            status_event(2, 2_000, 77, StatusState::Applied, Some(1)),
            damage_event(3, 3_000, action_id),
            status_event(4, 4_000, 77, StatusState::Refreshed, Some(1)),
            status_event(5, 5_000, 77, StatusState::Stacked, Some(2)),
            damage_event(6, 6_000, action_id),
            status_event(7, 7_000, 77, StatusState::Removed, None),
        ] {
            analyzer.observe(&event).unwrap();
        }
        let report = analyzer.finish().unwrap();
        assert!(!report.rdps_attribution_enabled);
        assert_eq!(report.windows.len(), 1);
        let window = &report.windows[0];
        assert_eq!(window.factor_item_ids, vec![20_020_427]);
        assert_eq!(
            window.selection_evidence,
            FactorSelectionEvidence::SourceAndRecipientOwnFactor
        );
        assert_eq!(window.minimum_observed_stacks, Some(1));
        assert_eq!(window.maximum_observed_stacks, Some(2));
        assert_eq!(window.refresh_without_stack_change_count, 1);
        assert_eq!(window.recipient_outgoing_damage.event_count, 2);
        assert_eq!(window.provider_outgoing_damage.event_count, 2);
        assert_eq!(window.action_damage.len(), 1);
        assert_eq!(window.close_reason, Some(FactorWindowCloseReason::Removed));
        assert_eq!(report.rule_summaries[0].matched_action_damage.amount, 2_000);
    }

    #[test]
    fn unreviewed_exact_grade_uses_complete_family_for_lifecycle_identity() {
        let owner = entity(1, 3_296_036);
        let mut analyzer = PsychoscopeFactorCorrelationAnalyzer::new();
        for event in [
            profile_event(1, 1_000, 20_020_400),
            status_event_between(
                2,
                2_000,
                Some(owner),
                owner,
                3_053_070,
                77,
                StatusState::Applied,
                Some(1),
            ),
        ] {
            analyzer.observe(&event).unwrap();
        }

        let report = analyzer.finish().unwrap();
        assert_eq!(report.schema_version, FACTOR_CORRELATION_SCHEMA_VERSION);
        assert!(!report.rdps_attribution_enabled);
        assert_eq!(report.selection_observations.len(), 1);
        assert_eq!(
            report.selection_observations[0].unreviewed_factor_item_ids,
            vec![20_020_400]
        );
        assert_eq!(report.windows.len(), 1);
        assert_eq!(report.windows[0].factor_item_ids, vec![20_020_400]);
        assert_eq!(
            report.windows[0].selection_evidence,
            FactorSelectionEvidence::SourceAndRecipientOwnFactor
        );
        assert!(report.windows[0].action_damage.is_empty());
    }

    #[test]
    fn retains_exact_provider_and_recipient_resource_transitions_without_zipping_arrays() {
        let provider = entity(1, 3_296_036);
        let recipient = entity(2, 9_999_998);
        let unrelated = entity(3, 9_999_997);
        let baseline_origin = 2.5_f32.to_bits();
        let changed_origin = 3.5_f32.to_bits();
        let cooldown = ResourceCooldown {
            resource_id: Some(17),
            begin_time_millis: Some(10),
            duration_millis: Some(20),
            valid_cooldown_time_millis: Some(15),
            existence_time_millis: Some(30),
        };
        let mut analyzer = PsychoscopeFactorCorrelationAnalyzer::new();
        for event in [
            profile_event(1, 1_000, 20_020_427),
            resource_event(
                2,
                2_000,
                provider,
                EntityAttributeUpdateKind::Snapshot,
                Some(baseline_origin),
                vec![7],
                vec![70],
                vec![],
            ),
            status_event_between(
                3,
                3_000,
                Some(provider),
                recipient,
                3_053_100,
                77,
                StatusState::Applied,
                Some(1),
            ),
            resource_event(
                4,
                4_000,
                provider,
                EntityAttributeUpdateKind::Delta,
                Some(changed_origin),
                vec![7, 8],
                vec![71],
                vec![cooldown],
            ),
            resource_event(
                5,
                5_000,
                recipient,
                EntityAttributeUpdateKind::Snapshot,
                None,
                vec![9],
                vec![90],
                vec![],
            ),
            resource_event(
                6,
                6_000,
                unrelated,
                EntityAttributeUpdateKind::Snapshot,
                None,
                vec![999],
                vec![999],
                vec![],
            ),
            status_event_between(
                7,
                7_000,
                Some(provider),
                recipient,
                3_053_100,
                77,
                StatusState::Removed,
                None,
            ),
        ] {
            analyzer.observe(&event).unwrap();
        }

        let report = analyzer.finish().unwrap();
        let window = &report.windows[0];
        assert_eq!(window.resource_baselines.len(), 2);
        let provider_baseline = window
            .resource_baselines
            .iter()
            .find(|baseline| baseline.actor_entity_uuid == provider.entity_uuid.0)
            .unwrap();
        assert_eq!(
            provider_baseline.actor_relation,
            FactorResourceActorRelation::Provider
        );
        assert_eq!(
            provider_baseline
                .state_before_window
                .as_ref()
                .unwrap()
                .resource_values,
            vec![70]
        );
        assert_eq!(window.resource_transitions.len(), 2);
        let provider_transition = &window.resource_transitions[0];
        assert_eq!(provider_transition.resource_ids, vec![7, 8]);
        assert_eq!(provider_transition.resource_values, vec![71]);
        assert_eq!(provider_transition.cooldowns, vec![cooldown]);
        assert_eq!(provider_transition.origin_energy_changed, Some(true));
        assert_eq!(provider_transition.resource_ids_changed, Some(true));
        assert_eq!(provider_transition.resource_values_changed, Some(true));
        assert_eq!(provider_transition.cooldowns_changed, Some(true));
        let recipient_transition = &window.resource_transitions[1];
        assert_eq!(
            recipient_transition.actor_relation,
            FactorResourceActorRelation::Recipient
        );
        assert_eq!(recipient_transition.resource_ids_changed, None);
        assert!(recipient_transition.complete_state_after);

        let summary = &report.rule_summaries[0];
        assert_eq!(summary.resource_transition_count, 2);
        assert_eq!(summary.provider_resource_transition_count, 1);
        assert_eq!(summary.recipient_resource_transition_count, 1);
        assert_eq!(summary.distinct_resource_ids, vec![7, 8, 9]);
        assert_eq!(summary.distinct_cooldown_resource_ids, vec![17]);
        assert!(!summary.attribution_enabled);
    }

    #[test]
    fn unknown_resource_update_is_retained_but_invalidates_reconstructed_state() {
        let owner = entity(1, 3_296_036);
        let mut analyzer = PsychoscopeFactorCorrelationAnalyzer::new();
        for event in [
            profile_event(1, 1_000, 20_020_427),
            resource_event(
                2,
                2_000,
                owner,
                EntityAttributeUpdateKind::Snapshot,
                None,
                vec![7],
                vec![70],
                vec![],
            ),
            status_event(3, 3_000, 77, StatusState::Applied, Some(1)),
            resource_event(
                4,
                4_000,
                owner,
                EntityAttributeUpdateKind::Unknown,
                None,
                vec![8],
                vec![80],
                vec![],
            ),
            resource_event(
                5,
                5_000,
                owner,
                EntityAttributeUpdateKind::Delta,
                None,
                vec![9],
                vec![90],
                vec![],
            ),
            status_event(6, 6_000, 77, StatusState::Removed, None),
        ] {
            analyzer.observe(&event).unwrap();
        }
        let report = analyzer.finish().unwrap();
        let transitions = &report.windows[0].resource_transitions;
        assert_eq!(transitions.len(), 2);
        assert_eq!(
            transitions[0].update_kind,
            EntityAttributeUpdateKind::Unknown
        );
        assert_eq!(transitions[0].resource_ids, vec![8]);
        assert!(!transitions[0].complete_state_after);
        assert_eq!(transitions[0].resource_ids_changed, None);
        assert_eq!(transitions[1].update_kind, EntityAttributeUpdateKind::Delta);
        assert!(!transitions[1].complete_state_after);
        assert_eq!(transitions[1].resource_ids_changed, None);
        assert_eq!(report.rule_summaries[0].incomplete_state_after_count, 2);
    }

    #[test]
    fn later_profile_snapshot_does_not_rewrite_an_open_window() {
        let mut analyzer = PsychoscopeFactorCorrelationAnalyzer::new();
        for event in [
            profile_event(1, 1_000, 20_020_427),
            status_event(2, 2_000, 77, StatusState::Applied, Some(1)),
            profile_event(3, 3_000, 20_020_887),
            status_event(4, 4_000, 77, StatusState::Removed, None),
        ] {
            analyzer.observe(&event).unwrap();
        }
        let report = analyzer.finish().unwrap();
        assert_eq!(report.windows[0].factor_item_ids, vec![20_020_427]);
        assert_eq!(report.selection_observations.len(), 2);
    }

    #[test]
    fn historical_max_health_factor_is_retained_without_current_build_formula_input() {
        let mut analyzer = PsychoscopeFactorCorrelationAnalyzer::new();
        analyzer
            .observe(&profile_event(1, 1_000, 20_021_025))
            .unwrap();
        let report = analyzer.finish().unwrap();
        let selection = &report.selection_observations[0];
        assert_eq!(selection.selected_factor_item_ids, vec![20_021_025]);
        assert_eq!(selection.unreviewed_factor_item_ids, vec![20_021_025]);
        assert!(selection.formula_inputs.is_empty());
        assert!(!report.rdps_attribution_enabled);
    }

    #[test]
    fn exact_current_build_selection_is_not_discarded_when_mechanics_are_unreviewed() {
        let mut analyzer = PsychoscopeFactorCorrelationAnalyzer::new();
        analyzer
            .observe(&profile_event(1, 1_000, 20_010_001))
            .unwrap();
        let report = analyzer.finish().unwrap();
        let selection = &report.selection_observations[0];

        assert_eq!(selection.selected_factor_item_ids, vec![20_010_001]);
        assert_eq!(selection.unreviewed_factor_item_ids, vec![20_010_001]);
        assert!(selection.unmapped_factor_item_ids.is_empty());
        assert!(selection.formula_inputs.is_empty());
        assert!(!report.rdps_attribution_enabled);
    }

    #[test]
    fn an_unmatched_change_is_retained_as_an_explicit_orphan_window() {
        let mut analyzer = PsychoscopeFactorCorrelationAnalyzer::new();
        analyzer
            .observe(&status_event(1, 1_000, 91, StatusState::Stacked, Some(2)))
            .unwrap();
        let report = analyzer.finish().unwrap();
        assert_eq!(report.windows.len(), 1);
        assert!(report.windows[0].opened_from_non_apply_state);
        assert_eq!(report.unmatched_lifecycle_events.len(), 1);
        assert_eq!(
            report.windows[0].selection_evidence,
            FactorSelectionEvidence::StaticCatalogOnly
        );
    }

    #[test]
    fn summarizes_recipient_overlap_distinct_providers_and_stack_ceiling() {
        let recipient = entity(9, 3_296_036);
        let provider_one = entity(1, 8_000_001);
        let provider_two = entity(2, 8_000_002);
        let mut analyzer = PsychoscopeFactorCorrelationAnalyzer::new();
        for event in [
            status_event_between(
                1,
                1_000,
                Some(provider_one),
                recipient,
                3_058_050,
                10,
                StatusState::Applied,
                Some(1),
            ),
            status_event_between(
                2,
                2_000,
                Some(provider_two),
                recipient,
                3_058_050,
                11,
                StatusState::Applied,
                Some(1),
            ),
            status_event_between(
                3,
                3_000,
                Some(provider_one),
                recipient,
                3_058_050,
                10,
                StatusState::Stacked,
                Some(3),
            ),
            status_event_between(
                4,
                4_000,
                Some(provider_one),
                recipient,
                3_058_050,
                10,
                StatusState::Removed,
                None,
            ),
            status_event_between(
                5,
                5_000,
                Some(provider_two),
                recipient,
                3_058_050,
                11,
                StatusState::Removed,
                None,
            ),
        ] {
            analyzer.observe(&event).unwrap();
        }
        let report = analyzer.finish().unwrap();
        assert_eq!(report.windows.len(), 2);
        let summary = &report.rule_summaries[0];
        assert_eq!(summary.maximum_observed_stacks, Some(3));
        assert_eq!(summary.overlapping_window_pairs, 1);
        assert_eq!(summary.maximum_concurrent_instances_per_recipient, 2);
        assert_eq!(
            summary.maximum_concurrent_distinct_providers_per_recipient,
            2
        );
        assert!(!summary.attribution_enabled);
    }
}
