use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
};

use serde::{Deserialize, Serialize};

use rlogs_events::{ActorKind, ActorState, CanonicalEvent, EventEnvelope, TimelineEventKind};

use crate::{
    BPSR_GAME_PLUGIN_ID, BPSR_PROFILE_SCHEMA_ID, BPSR_PROFILE_SCHEMA_VERSION,
    CharacterProfilePatch, EffectDreamscopeSourceKind, EffectFingerprintMatchKind,
    character_id_from_entity_uuid, combat_action_presentation, resolve_effect_origin_fingerprint,
};

const DREAMSCOPE_BUILD_CATALOG_JSON: &str =
    include_str!("../game-data/runtime/dreamscope-build-inference.v1.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DreamscopeSourceKind {
    TreeNode,
    AdvancedTreeEffect,
    FactorFamily,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DreamscopeEvidenceKind {
    ExactTreeNode,
    ExactFactorItem,
    /// Packet-visible effect linked by an explicit, current-build formula
    /// bridge to a selected Dreamscope source endpoint.
    RuntimeEffect,
    TerminalEffect,
    /// Packet-observed effect plus its exact `(source_type_id,
    /// source_config_id)` formula-origin tuple. This can distinguish sources
    /// that share the same terminal effect without inventing an item grade.
    EffectOrigin,
    DamageAbility,
    RecountGroup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DreamscopeEvidenceResolution {
    Exact,
    Partial,
    Ambiguous,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DreamscopeBuildResolution {
    Exact,
    Partial,
    Ambiguous,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DreamscopeSourceCandidate {
    pub source_kind: DreamscopeSourceKind,
    pub source_id: i64,
    pub name: String,
    pub template_id: Option<i64>,
    #[serde(default)]
    pub item_ids: Vec<i64>,
    #[serde(default)]
    pub grades: Vec<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DreamscopeBuildEvidence {
    pub character_id: String,
    /// Present only when the selected tree was observed directly from that
    /// character's own profile/loadout packet. `None` means unobserved, not an
    /// empty tree.
    pub exact_tree_node_ids: Option<Vec<i64>>,
    /// Present only when the factor slots were observed directly. Exact item
    /// IDs retain grade; a terminal buff alone does not.
    pub exact_factor_item_ids: Option<Vec<i64>>,
    /// Packet-observed terminal status/buff IDs emitted by formulas or
    /// triggers. These are the strongest remote-build fingerprints.
    #[serde(default)]
    pub terminal_effect_ids: Vec<i64>,
    /// Exact packet formula origins retained beside their terminal effect.
    /// This is intentionally separate from `terminal_effect_ids`: the latter
    /// remains the lossless effect-only fallback for old or originless events.
    #[serde(default)]
    pub effect_origins: Vec<DreamscopeEffectOriginEvidence>,
    #[serde(default)]
    pub damage_ability_ids: Vec<i64>,
    #[serde(default)]
    pub recount_group_ids: Vec<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DreamscopeEffectOriginEvidence {
    pub effect_id: i64,
    pub source_type_id: i32,
    pub source_config_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DreamscopeEvidenceMatch {
    pub evidence_kind: DreamscopeEvidenceKind,
    pub evidence_id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_type_id: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_config_id: Option<i64>,
    pub resolution: DreamscopeEvidenceResolution,
    pub candidates: Vec<DreamscopeSourceCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DreamscopeBuildInference {
    pub schema_version: u16,
    pub game_build: String,
    pub character_id: String,
    pub resolution: DreamscopeBuildResolution,
    /// True only when both complete tree selections and complete factor-slot
    /// selections were directly observed. Unique emitted effects can prove a
    /// contributing choice, but never imply that the unobserved build is
    /// complete.
    pub complete_snapshot_observed: bool,
    pub resolved_tree_node_ids: Vec<i64>,
    pub resolved_advanced_effect_ids: Vec<i64>,
    pub resolved_factor_family_ids: Vec<i64>,
    pub resolved_factor_item_ids: Vec<i64>,
    pub matches: Vec<DreamscopeEvidenceMatch>,
    pub contradictions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DreamscopeCatalogSummary {
    pub templates: usize,
    pub selectable_tree_nodes: usize,
    pub advanced_tree_effects: usize,
    pub factor_slot_templates: usize,
    pub factor_slots: usize,
    pub factor_families: usize,
    pub factor_items: usize,
    pub terminal_effect_ids: usize,
    pub ambiguous_terminal_effect_ids: usize,
    pub runtime_effect_ids: usize,
    pub ambiguous_runtime_effect_ids: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnresolvedDreamscopeProviderEvidence {
    pub provider_entity_uuid: Option<i64>,
    pub evidence_kind: DreamscopeEvidenceKind,
    pub evidence_id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_type_id: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_config_id: Option<i64>,
    pub observation_count: u64,
    pub first_sequence: u64,
    pub last_sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DreamscopeBuildInferenceReport {
    pub schema_version: u16,
    pub catalog_game_build: String,
    pub observed_client_builds: Vec<String>,
    /// A build mismatch is a warning, not a processing lock. Hotfix captures
    /// continue through the current catalog while preserving the mismatch.
    pub catalog_out_of_date: bool,
    pub session_id: String,
    pub actors: Vec<DreamscopeBuildInference>,
    pub unresolved_provider_evidence: Vec<UnresolvedDreamscopeProviderEvidence>,
}

#[derive(Debug, thiserror::Error)]
pub enum DreamscopeBuildInferenceError {
    #[error("Dreamscope inference received multiple sessions: {expected} and {actual}")]
    MixedSessions { expected: String, actual: String },

    #[error("Dreamscope inference event sequence moved backward from {previous} to {actual}")]
    SequenceMovedBackward { previous: u64, actual: u64 },

    #[error("invalid BPSR profile evidence: {0}")]
    InvalidProfile(String),

    #[error("combat presentation lookup failed: {0}")]
    Presentation(String),
}

#[derive(Debug, Clone)]
struct UnresolvedEvidenceAccumulator {
    observation_count: u64,
    first_sequence: u64,
    last_sequence: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum RuntimeEvidenceKey {
    Simple(DreamscopeEvidenceKind, i64),
    EffectOrigin(DreamscopeEffectOriginEvidence),
}

/// Streaming, bounded-to-unique-IDs inference shared by historical and live
/// consumers. It only assigns runtime evidence after the source entity is
/// proven to be a player; monster-shaped UUIDs are never decoded as character
/// IDs merely because their bits happen to fit that wire format.
#[derive(Debug, Default)]
pub struct DreamscopeBuildInferenceAnalyzer {
    session_id: Option<String>,
    last_sequence: Option<u64>,
    observed_client_builds: BTreeSet<String>,
    player_character_ids_by_entity_uuid: BTreeMap<i64, String>,
    evidence_by_character_id: BTreeMap<String, DreamscopeBuildEvidence>,
    pending_by_entity_uuid:
        BTreeMap<i64, BTreeMap<RuntimeEvidenceKey, UnresolvedEvidenceAccumulator>>,
    unresolved_without_provider:
        BTreeMap<(Option<i64>, RuntimeEvidenceKey), UnresolvedEvidenceAccumulator>,
}

impl DreamscopeBuildInferenceAnalyzer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn observe(
        &mut self,
        envelope: &EventEnvelope,
    ) -> Result<(), DreamscopeBuildInferenceError> {
        self.validate(envelope)?;
        match &envelope.event {
            CanonicalEvent::CharacterProfileObserved { profile }
                if profile.game_plugin_id == BPSR_GAME_PLUGIN_ID
                    && profile.payload_schema_id == BPSR_PROFILE_SCHEMA_ID
                    && profile.payload_schema_version == BPSR_PROFILE_SCHEMA_VERSION =>
            {
                let profile = CharacterProfilePatch::from_game_event(profile).map_err(|error| {
                    DreamscopeBuildInferenceError::InvalidProfile(error.to_string())
                })?;
                self.observe_profile(&profile);
            }
            CanonicalEvent::Timeline(timeline) => match &timeline.kind {
                TimelineEventKind::Actor(actor) => {
                    if actor.kind == ActorKind::Player && actor.state != ActorState::Despawned {
                        self.observe_player_actor(actor.actor.entity_uuid.0);
                    }
                }
                TimelineEventKind::Status(status) => {
                    let effect_id = status.effect.0;
                    let observed_match = dreamscope_observed_effect_match(effect_id);
                    let fallback_kind = (observed_match.resolution
                        != DreamscopeEvidenceResolution::Unknown)
                        .then_some(observed_match.evidence_kind);
                    if let Some(origin) = status.origin {
                        let origin_match = dreamscope_observed_effect_origin_match(
                            effect_id,
                            origin.source_type_id,
                            origin.source_config_id,
                        );
                        if !origin_match.candidates.is_empty() || fallback_kind.is_some() {
                            self.observe_effect_origin_evidence(
                                envelope.sequence,
                                status.source.map(|source| source.entity_uuid.0),
                                DreamscopeEffectOriginEvidence {
                                    effect_id,
                                    source_type_id: origin.source_type_id,
                                    source_config_id: origin.source_config_id,
                                },
                            );
                        }
                    } else if let Some(evidence_kind) = fallback_kind {
                        self.observe_simple_evidence(
                            envelope.sequence,
                            status.source.map(|source| source.entity_uuid.0),
                            evidence_kind,
                            effect_id,
                        );
                    }
                }
                TimelineEventKind::Damage(damage) => {
                    let Some(ability_id) = damage.ability.map(|ability| ability.0) else {
                        return Ok(());
                    };
                    let provider = Some(damage.source.entity_uuid.0);
                    if catalog().candidates_by_damage_id.contains_key(&ability_id) {
                        self.observe_simple_evidence(
                            envelope.sequence,
                            provider,
                            DreamscopeEvidenceKind::DamageAbility,
                            ability_id,
                        );
                    }
                    if let Some(recount_group_id) = combat_action_presentation(ability_id)
                        .map_err(DreamscopeBuildInferenceError::Presentation)?
                        .and_then(|presentation| presentation.recount_group_id)
                        && catalog()
                            .candidates_by_recount_id
                            .contains_key(&recount_group_id)
                    {
                        self.observe_simple_evidence(
                            envelope.sequence,
                            provider,
                            DreamscopeEvidenceKind::RecountGroup,
                            recount_group_id,
                        );
                    }
                }
                _ => {}
            },
            _ => {}
        }
        Ok(())
    }

    pub fn finish(self) -> DreamscopeBuildInferenceReport {
        let mut unresolved = self.unresolved_without_provider;
        for (entity_uuid, pending) in self.pending_by_entity_uuid {
            for (key, value) in pending {
                unresolved.insert((Some(entity_uuid), key), value);
            }
        }
        let observed_client_builds = self.observed_client_builds.into_iter().collect::<Vec<_>>();
        let catalog_out_of_date = observed_client_builds
            .iter()
            .any(|build| build != dreamscope_catalog_game_build());
        let actors = self
            .evidence_by_character_id
            .values()
            .map(infer_dreamscope_build)
            .collect();
        let unresolved_provider_evidence = unresolved
            .into_iter()
            .map(|((provider_entity_uuid, key), value)| {
                let (evidence_kind, evidence_id, source_type_id, source_config_id) = match key {
                    RuntimeEvidenceKey::Simple(evidence_kind, evidence_id) => {
                        (evidence_kind, evidence_id, None, None)
                    }
                    RuntimeEvidenceKey::EffectOrigin(origin) => (
                        DreamscopeEvidenceKind::EffectOrigin,
                        origin.effect_id,
                        Some(origin.source_type_id),
                        Some(origin.source_config_id),
                    ),
                };
                UnresolvedDreamscopeProviderEvidence {
                    provider_entity_uuid,
                    evidence_kind,
                    evidence_id,
                    source_type_id,
                    source_config_id,
                    observation_count: value.observation_count,
                    first_sequence: value.first_sequence,
                    last_sequence: value.last_sequence,
                }
            })
            .collect();
        DreamscopeBuildInferenceReport {
            schema_version: catalog().schema_version,
            catalog_game_build: catalog().game_build.clone(),
            observed_client_builds,
            catalog_out_of_date,
            session_id: self.session_id.unwrap_or_default(),
            actors,
            unresolved_provider_evidence,
        }
    }

    fn validate(&mut self, envelope: &EventEnvelope) -> Result<(), DreamscopeBuildInferenceError> {
        if let Some(expected) = &self.session_id {
            if expected != &envelope.session_id {
                return Err(DreamscopeBuildInferenceError::MixedSessions {
                    expected: expected.clone(),
                    actual: envelope.session_id.clone(),
                });
            }
        } else {
            self.session_id = Some(envelope.session_id.clone());
        }
        if let Some(previous) = self.last_sequence
            && envelope.sequence <= previous
        {
            return Err(DreamscopeBuildInferenceError::SequenceMovedBackward {
                previous,
                actual: envelope.sequence,
            });
        }
        self.last_sequence = Some(envelope.sequence);
        self.observed_client_builds
            .insert(envelope.region.client_build.clone());
        Ok(())
    }

    fn observe_profile(&mut self, profile: &CharacterProfilePatch) {
        let character_id = profile.character.character_id.clone();
        let evidence = self.evidence_for_character(character_id);
        let selected_season = profile
            .season
            .as_ref()
            .and_then(|season| season.season_id)
            .and_then(|season_id| i32::try_from(season_id).ok())
            .and_then(|season_id| {
                profile
                    .season_cultivation
                    .as_ref()?
                    .iter()
                    .find(|entry| entry.season_id == season_id)
            })
            .or_else(|| {
                profile
                    .season_cultivation
                    .as_ref()?
                    .iter()
                    .max_by_key(|entry| entry.season_id)
            });
        let Some(selected_season) = selected_season else {
            return;
        };
        let selected_items = selected_season
            .lines
            .iter()
            .flat_map(|line| &line.areas)
            .filter(|area| area.active == Some(true))
            .flat_map(|area| area.middle_node_item_ids.values().copied())
            .collect::<BTreeSet<_>>();
        evidence.exact_factor_item_ids = Some(selected_items.into_iter().collect());
    }

    fn observe_player_actor(&mut self, entity_uuid: i64) {
        let Some(character_id) = character_id_from_entity_uuid(entity_uuid) else {
            return;
        };
        self.player_character_ids_by_entity_uuid
            .insert(entity_uuid, character_id.clone());
        self.evidence_for_character(character_id.clone());
        if let Some(pending) = self.pending_by_entity_uuid.remove(&entity_uuid) {
            let evidence = self.evidence_for_character(character_id);
            merge_pending(evidence, pending);
        }
    }

    fn observe_simple_evidence(
        &mut self,
        sequence: u64,
        provider_entity_uuid: Option<i64>,
        evidence_kind: DreamscopeEvidenceKind,
        evidence_id: i64,
    ) {
        self.observe_runtime_evidence(
            sequence,
            provider_entity_uuid,
            RuntimeEvidenceKey::Simple(evidence_kind, evidence_id),
        );
    }

    fn observe_effect_origin_evidence(
        &mut self,
        sequence: u64,
        provider_entity_uuid: Option<i64>,
        origin: DreamscopeEffectOriginEvidence,
    ) {
        self.observe_runtime_evidence(
            sequence,
            provider_entity_uuid,
            RuntimeEvidenceKey::EffectOrigin(origin),
        );
    }

    fn observe_runtime_evidence(
        &mut self,
        sequence: u64,
        provider_entity_uuid: Option<i64>,
        key: RuntimeEvidenceKey,
    ) {
        let Some(entity_uuid) = provider_entity_uuid else {
            push_unresolved(&mut self.unresolved_without_provider, None, key, sequence);
            return;
        };
        if let Some(character_id) = self
            .player_character_ids_by_entity_uuid
            .get(&entity_uuid)
            .cloned()
        {
            insert_runtime_evidence(self.evidence_for_character(character_id), key);
        } else {
            let pending = self.pending_by_entity_uuid.entry(entity_uuid).or_default();
            push_pending(pending, key, sequence);
        }
    }

    fn evidence_for_character(&mut self, character_id: String) -> &mut DreamscopeBuildEvidence {
        self.evidence_by_character_id
            .entry(character_id.clone())
            .or_insert_with(|| DreamscopeBuildEvidence {
                character_id,
                ..DreamscopeBuildEvidence::default()
            })
    }
}

fn insert_evidence(
    evidence: &mut DreamscopeBuildEvidence,
    evidence_kind: DreamscopeEvidenceKind,
    evidence_id: i64,
) {
    let values = match evidence_kind {
        DreamscopeEvidenceKind::RuntimeEffect | DreamscopeEvidenceKind::TerminalEffect => {
            &mut evidence.terminal_effect_ids
        }
        DreamscopeEvidenceKind::DamageAbility => &mut evidence.damage_ability_ids,
        DreamscopeEvidenceKind::RecountGroup => &mut evidence.recount_group_ids,
        DreamscopeEvidenceKind::ExactTreeNode
        | DreamscopeEvidenceKind::ExactFactorItem
        | DreamscopeEvidenceKind::EffectOrigin => return,
    };
    if !values.contains(&evidence_id) {
        values.push(evidence_id);
        values.sort_unstable();
    }
}

fn insert_runtime_evidence(evidence: &mut DreamscopeBuildEvidence, key: RuntimeEvidenceKey) {
    match key {
        RuntimeEvidenceKey::Simple(evidence_kind, evidence_id) => {
            insert_evidence(evidence, evidence_kind, evidence_id);
        }
        RuntimeEvidenceKey::EffectOrigin(origin) => {
            if !evidence.effect_origins.contains(&origin) {
                evidence.effect_origins.push(origin);
                evidence.effect_origins.sort_unstable();
            }
        }
    }
}

fn merge_pending(
    evidence: &mut DreamscopeBuildEvidence,
    pending: BTreeMap<RuntimeEvidenceKey, UnresolvedEvidenceAccumulator>,
) {
    for (key, _) in pending {
        insert_runtime_evidence(evidence, key);
    }
}

fn push_pending(
    pending: &mut BTreeMap<RuntimeEvidenceKey, UnresolvedEvidenceAccumulator>,
    key: RuntimeEvidenceKey,
    sequence: u64,
) {
    let value = pending.entry(key).or_insert(UnresolvedEvidenceAccumulator {
        observation_count: 0,
        first_sequence: sequence,
        last_sequence: sequence,
    });
    value.observation_count = value.observation_count.saturating_add(1);
    value.first_sequence = value.first_sequence.min(sequence);
    value.last_sequence = value.last_sequence.max(sequence);
}

fn push_unresolved(
    unresolved: &mut BTreeMap<(Option<i64>, RuntimeEvidenceKey), UnresolvedEvidenceAccumulator>,
    provider_entity_uuid: Option<i64>,
    key: RuntimeEvidenceKey,
    sequence: u64,
) {
    let value =
        unresolved
            .entry((provider_entity_uuid, key))
            .or_insert(UnresolvedEvidenceAccumulator {
                observation_count: 0,
                first_sequence: sequence,
                last_sequence: sequence,
            });
    value.observation_count = value.observation_count.saturating_add(1);
    value.first_sequence = value.first_sequence.min(sequence);
    value.last_sequence = value.last_sequence.max(sequence);
}

#[derive(Debug, Deserialize)]
struct DreamscopeBuildCatalog {
    schema_version: u16,
    game_build: String,
    summary: DreamscopeCatalogSummary,
    tree_nodes_by_id: BTreeMap<i64, TreeNode>,
    factor_items_by_id: BTreeMap<i64, DreamscopeFactorItemIdentity>,
    candidates_by_terminal_effect_id: BTreeMap<i64, Vec<DreamscopeSourceCandidate>>,
    candidates_by_runtime_effect_id: BTreeMap<i64, Vec<DreamscopeSourceCandidate>>,
    candidates_by_damage_id: BTreeMap<i64, Vec<DreamscopeSourceCandidate>>,
    candidates_by_recount_id: BTreeMap<i64, Vec<DreamscopeSourceCandidate>>,
}

#[derive(Debug, Deserialize)]
struct TreeNode {
    node_id: i64,
    node_type: i64,
    mutually_exclusive_node_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DreamscopeFactorItemIdentity {
    pub item_id: i64,
    pub family_id: i64,
    pub grade: i64,
}

static DREAMSCOPE_BUILD_CATALOG: OnceLock<DreamscopeBuildCatalog> = OnceLock::new();

fn catalog() -> &'static DreamscopeBuildCatalog {
    DREAMSCOPE_BUILD_CATALOG.get_or_init(|| {
        serde_json::from_str(DREAMSCOPE_BUILD_CATALOG_JSON)
            .expect("bundled Dreamscope build-inference catalog must be valid")
    })
}

pub fn dreamscope_catalog_summary() -> &'static DreamscopeCatalogSummary {
    &catalog().summary
}

pub fn dreamscope_catalog_game_build() -> &'static str {
    &catalog().game_build
}

/// Returns the exact factor family and grade selected by a current-build
/// Dreamscope item ID. This is identity evidence only: mechanics and rDPS
/// credit remain gated by the separately reviewed attribution catalog.
pub fn dreamscope_factor_item_by_id(item_id: i64) -> Option<&'static DreamscopeFactorItemIdentity> {
    catalog().factor_items_by_id.get(&item_id)
}

pub fn dreamscope_candidates_for_terminal_effect(
    effect_id: i64,
) -> &'static [DreamscopeSourceCandidate] {
    catalog()
        .candidates_by_terminal_effect_id
        .get(&effect_id)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

/// Resolves one packet-observed terminal status/effect ID against the exact
/// current-build Dreamscope catalog without implying a complete loadout.
pub fn dreamscope_terminal_effect_match(effect_id: i64) -> DreamscopeEvidenceMatch {
    evidence_match(
        DreamscopeEvidenceKind::TerminalEffect,
        effect_id,
        dreamscope_candidates_for_terminal_effect(effect_id).to_vec(),
        false,
    )
}

/// Resolves the actual effect ID observed in a packet. Explicit runtime-effect
/// bridges take precedence; direct terminal endpoints remain a strict fallback.
pub fn dreamscope_observed_effect_match(effect_id: i64) -> DreamscopeEvidenceMatch {
    let mut runtime_candidates = catalog()
        .candidates_by_runtime_effect_id
        .get(&effect_id)
        .cloned()
        .unwrap_or_default();
    runtime_candidates.extend(shared_dreamscope_candidates(effect_id, None));
    let runtime_candidates = merge_source_candidates(runtime_candidates);
    if !runtime_candidates.is_empty() {
        return evidence_match(
            DreamscopeEvidenceKind::RuntimeEffect,
            effect_id,
            runtime_candidates,
            false,
        );
    }
    dreamscope_terminal_effect_match(effect_id)
}

/// Converts the shared formula-endpoint catalog into the build-inference
/// vocabulary. Only candidates carrying an explicit Dreamscope selector are
/// admitted; semantic aliases without a selectable tree/factor identity stay
/// out of build inference.
fn shared_dreamscope_candidates(
    effect_id: i64,
    origin: Option<(i32, i64)>,
) -> Vec<DreamscopeSourceCandidate> {
    let fingerprint = resolve_effect_origin_fingerprint(
        effect_id,
        origin,
        None,
        0,
        rlogs_events::StatusState::Applied,
    );
    let candidates = fingerprint
        .candidate_sources
        .iter()
        .filter_map(|candidate| {
            let selector = candidate.dreamscope_selector.as_ref()?;
            let source_kind = match selector.source_kind {
                EffectDreamscopeSourceKind::TreeNode => DreamscopeSourceKind::TreeNode,
                EffectDreamscopeSourceKind::AdvancedTreeEffect => {
                    DreamscopeSourceKind::AdvancedTreeEffect
                }
                EffectDreamscopeSourceKind::FactorFamily => DreamscopeSourceKind::FactorFamily,
            };
            Some(DreamscopeSourceCandidate {
                source_kind,
                source_id: selector.source_id,
                name: candidate.source_name.clone().unwrap_or_default(),
                template_id: None,
                item_ids: selector.candidate_item_ids.clone(),
                grades: selector.candidate_grades.clone(),
            })
        })
        .collect::<Vec<_>>();
    merge_source_candidates(candidates)
}

/// Resolves one packet-observed effect through its exact formula-origin tuple.
///
/// The origin is used only when the shared effect catalog contains that exact
/// tuple. Within the Dreamscope domain, duplicate semantic representations
/// (for example a generic `phantom-factor` row and its concrete factor-family
/// row) collapse to the concrete tree/family identity. A factor family can be
/// proven this way, but its exact item grade remains partial unless an item ID
/// was observed from the owner's profile snapshot.
pub fn dreamscope_observed_effect_origin_match(
    effect_id: i64,
    source_type_id: i32,
    source_config_id: i64,
) -> DreamscopeEvidenceMatch {
    let fallback = dreamscope_observed_effect_match(effect_id);
    let fingerprint = resolve_effect_origin_fingerprint(
        effect_id,
        Some((source_type_id, source_config_id)),
        None,
        0,
        rlogs_events::StatusState::Applied,
    );

    let mut candidates = fallback.candidates;
    if fingerprint.match_kind == EffectFingerprintMatchKind::ExactPacketOrigin {
        let identities = fingerprint
            .candidate_sources
            .iter()
            .filter_map(|candidate| {
                Some((
                    dreamscope_source_kind(candidate.source_kind.as_str())?,
                    candidate.source_entity_id?,
                    candidate.source_name.clone().unwrap_or_default(),
                ))
            })
            .collect::<BTreeSet<_>>();
        if !identities.is_empty() {
            // The packet-visible effect bridge can be more specific than the
            // terminal formula endpoint and therefore carry only one example
            // item. Merge both views before narrowing so a proven family does
            // not accidentally masquerade as a proven grade.
            let mut candidate_pool = candidates.clone();
            if let Some(endpoint_candidates) = catalog()
                .candidates_by_terminal_effect_id
                .get(&source_config_id)
            {
                candidate_pool.extend(endpoint_candidates.iter().cloned());
            }
            let narrowed = merge_source_candidates(candidate_pool)
                .into_iter()
                .filter(|candidate| {
                    identities.iter().any(|(source_kind, source_id, _)| {
                        candidate.source_kind == *source_kind && candidate.source_id == *source_id
                    })
                })
                .collect::<Vec<_>>();
            candidates = if narrowed.is_empty() {
                identities
                    .into_iter()
                    .map(|(source_kind, source_id, name)| DreamscopeSourceCandidate {
                        source_kind,
                        source_id,
                        name,
                        template_id: None,
                        item_ids: Vec::new(),
                        grades: Vec::new(),
                    })
                    .collect()
            } else {
                narrowed
            };
        }
    }
    candidates.sort_by_key(|candidate| (candidate.source_kind, candidate.source_id));
    candidates.dedup_by_key(|candidate| (candidate.source_kind, candidate.source_id));

    let mut matched = evidence_match(
        DreamscopeEvidenceKind::EffectOrigin,
        effect_id,
        candidates,
        false,
    );
    matched.source_type_id = Some(source_type_id);
    matched.source_config_id = Some(source_config_id);
    matched
}

fn merge_source_candidates(
    candidates: Vec<DreamscopeSourceCandidate>,
) -> Vec<DreamscopeSourceCandidate> {
    let mut merged = BTreeMap::<(DreamscopeSourceKind, i64), DreamscopeSourceCandidate>::new();
    for candidate in candidates {
        let key = (candidate.source_kind, candidate.source_id);
        let entry = merged
            .entry(key)
            .or_insert_with(|| DreamscopeSourceCandidate {
                source_kind: candidate.source_kind,
                source_id: candidate.source_id,
                name: String::new(),
                template_id: None,
                item_ids: Vec::new(),
                grades: Vec::new(),
            });
        if entry.name.is_empty() && !candidate.name.is_empty() {
            entry.name = candidate.name;
        }
        if entry.template_id.is_none() {
            entry.template_id = candidate.template_id;
        }
        entry.item_ids.extend(candidate.item_ids);
        entry.grades.extend(candidate.grades);
        entry.item_ids.sort_unstable();
        entry.item_ids.dedup();
        entry.grades.sort_unstable();
        entry.grades.dedup();
    }
    merged.into_values().collect()
}

fn dreamscope_source_kind(source_kind: &str) -> Option<DreamscopeSourceKind> {
    match source_kind {
        "dreamscope-tree-node" => Some(DreamscopeSourceKind::TreeNode),
        "dreamscope-advanced-tree-effect" => Some(DreamscopeSourceKind::AdvancedTreeEffect),
        "dreamscope-factor-family" => Some(DreamscopeSourceKind::FactorFamily),
        _ => None,
    }
}

pub fn infer_dreamscope_build(evidence: &DreamscopeBuildEvidence) -> DreamscopeBuildInference {
    let catalog = catalog();
    let mut matches = Vec::new();
    let mut contradictions = Vec::new();
    let mut tree_node_ids = BTreeSet::new();
    let mut advanced_effect_ids = BTreeSet::new();
    let mut factor_family_ids = BTreeSet::new();
    let mut factor_item_ids = BTreeSet::new();

    if let Some(node_ids) = &evidence.exact_tree_node_ids {
        for node_id in unique(node_ids) {
            let candidates = catalog
                .tree_nodes_by_id
                .get(&node_id)
                .filter(|node| node.node_type == 1 && node.node_id == node_id)
                .map(|_| vec![tree_candidate(catalog, node_id)])
                .unwrap_or_default();
            if !candidates.is_empty() {
                tree_node_ids.insert(node_id);
            }
            matches.push(evidence_match(
                DreamscopeEvidenceKind::ExactTreeNode,
                node_id,
                candidates,
                true,
            ));
        }
        contradictions.extend(tree_contradictions(catalog, &tree_node_ids));
    }

    if let Some(item_ids) = &evidence.exact_factor_item_ids {
        for item_id in unique(item_ids) {
            let candidates = catalog
                .factor_items_by_id
                .get(&item_id)
                .filter(|item| item.item_id == item_id)
                .map(|item| {
                    factor_item_ids.insert(item_id);
                    factor_family_ids.insert(item.family_id);
                    vec![DreamscopeSourceCandidate {
                        source_kind: DreamscopeSourceKind::FactorFamily,
                        source_id: item.family_id,
                        name: factor_family_name(catalog, item.family_id),
                        template_id: None,
                        item_ids: vec![item_id],
                        grades: vec![item.grade],
                    }]
                })
                .unwrap_or_default();
            matches.push(evidence_match(
                DreamscopeEvidenceKind::ExactFactorItem,
                item_id,
                candidates,
                true,
            ));
        }
    }

    let origin_effect_ids = evidence
        .effect_origins
        .iter()
        .map(|origin| origin.effect_id)
        .collect::<BTreeSet<_>>();
    for origin in evidence
        .effect_origins
        .iter()
        .copied()
        .collect::<BTreeSet<_>>()
    {
        let effect_match = dreamscope_observed_effect_origin_match(
            origin.effect_id,
            origin.source_type_id,
            origin.source_config_id,
        );
        collect_resolved_sources(
            &effect_match.candidates,
            &mut tree_node_ids,
            &mut advanced_effect_ids,
            &mut factor_family_ids,
        );
        matches.push(effect_match);
    }
    for effect_id in unique(&evidence.terminal_effect_ids)
        .into_iter()
        .filter(|effect_id| !origin_effect_ids.contains(effect_id))
    {
        let effect_match = dreamscope_observed_effect_match(effect_id);
        let candidates = effect_match.candidates;
        collect_resolved_sources(
            &candidates,
            &mut tree_node_ids,
            &mut advanced_effect_ids,
            &mut factor_family_ids,
        );
        matches.push(evidence_match(
            effect_match.evidence_kind,
            effect_id,
            candidates,
            false,
        ));
    }
    for ability_id in unique(&evidence.damage_ability_ids) {
        let candidates = catalog
            .candidates_by_damage_id
            .get(&ability_id)
            .cloned()
            .unwrap_or_default();
        collect_resolved_sources(
            &candidates,
            &mut tree_node_ids,
            &mut advanced_effect_ids,
            &mut factor_family_ids,
        );
        matches.push(evidence_match(
            DreamscopeEvidenceKind::DamageAbility,
            ability_id,
            candidates,
            false,
        ));
    }
    for recount_id in unique(&evidence.recount_group_ids) {
        let candidates = catalog
            .candidates_by_recount_id
            .get(&recount_id)
            .cloned()
            .unwrap_or_default();
        collect_resolved_sources(
            &candidates,
            &mut tree_node_ids,
            &mut advanced_effect_ids,
            &mut factor_family_ids,
        );
        matches.push(evidence_match(
            DreamscopeEvidenceKind::RecountGroup,
            recount_id,
            candidates,
            false,
        ));
    }

    let complete_snapshot_observed =
        evidence.exact_tree_node_ids.is_some() && evidence.exact_factor_item_ids.is_some();
    let has_ambiguous = matches
        .iter()
        .any(|entry| entry.resolution == DreamscopeEvidenceResolution::Ambiguous);
    let has_resolved = matches.iter().any(|entry| {
        matches!(
            entry.resolution,
            DreamscopeEvidenceResolution::Exact | DreamscopeEvidenceResolution::Partial
        )
    });
    let resolution = if has_ambiguous || !contradictions.is_empty() {
        DreamscopeBuildResolution::Ambiguous
    } else if complete_snapshot_observed
        && matches
            .iter()
            .all(|entry| entry.resolution != DreamscopeEvidenceResolution::Unknown)
    {
        DreamscopeBuildResolution::Exact
    } else if has_resolved {
        DreamscopeBuildResolution::Partial
    } else {
        DreamscopeBuildResolution::Unknown
    };

    DreamscopeBuildInference {
        schema_version: catalog.schema_version,
        game_build: catalog.game_build.clone(),
        character_id: evidence.character_id.clone(),
        resolution,
        complete_snapshot_observed,
        resolved_tree_node_ids: tree_node_ids.into_iter().collect(),
        resolved_advanced_effect_ids: advanced_effect_ids.into_iter().collect(),
        resolved_factor_family_ids: factor_family_ids.into_iter().collect(),
        resolved_factor_item_ids: factor_item_ids.into_iter().collect(),
        matches,
        contradictions,
    }
}

fn evidence_match(
    evidence_kind: DreamscopeEvidenceKind,
    evidence_id: i64,
    candidates: Vec<DreamscopeSourceCandidate>,
    directly_observed: bool,
) -> DreamscopeEvidenceMatch {
    let resolution = match candidates.len() {
        0 => DreamscopeEvidenceResolution::Unknown,
        1 if directly_observed => DreamscopeEvidenceResolution::Exact,
        1 if candidates[0].source_kind != DreamscopeSourceKind::FactorFamily => {
            DreamscopeEvidenceResolution::Exact
        }
        1 if candidates[0].item_ids.len() == 1 => DreamscopeEvidenceResolution::Exact,
        1 => DreamscopeEvidenceResolution::Partial,
        _ => DreamscopeEvidenceResolution::Ambiguous,
    };
    DreamscopeEvidenceMatch {
        evidence_kind,
        evidence_id,
        source_type_id: None,
        source_config_id: None,
        resolution,
        candidates,
    }
}

fn collect_resolved_sources(
    candidates: &[DreamscopeSourceCandidate],
    tree_node_ids: &mut BTreeSet<i64>,
    advanced_effect_ids: &mut BTreeSet<i64>,
    factor_family_ids: &mut BTreeSet<i64>,
) {
    if candidates.len() != 1 {
        return;
    }
    let candidate = &candidates[0];
    match candidate.source_kind {
        DreamscopeSourceKind::TreeNode => {
            tree_node_ids.insert(candidate.source_id);
        }
        DreamscopeSourceKind::AdvancedTreeEffect => {
            advanced_effect_ids.insert(candidate.source_id);
        }
        DreamscopeSourceKind::FactorFamily => {
            factor_family_ids.insert(candidate.source_id);
        }
    }
}

fn tree_candidate(catalog: &DreamscopeBuildCatalog, node_id: i64) -> DreamscopeSourceCandidate {
    catalog
        .candidates_by_terminal_effect_id
        .values()
        .flatten()
        .find(|candidate| {
            candidate.source_kind == DreamscopeSourceKind::TreeNode
                && candidate.source_id == node_id
        })
        .cloned()
        .unwrap_or(DreamscopeSourceCandidate {
            source_kind: DreamscopeSourceKind::TreeNode,
            source_id: node_id,
            name: String::new(),
            template_id: None,
            item_ids: Vec::new(),
            grades: Vec::new(),
        })
}

fn factor_family_name(catalog: &DreamscopeBuildCatalog, family_id: i64) -> String {
    catalog
        .candidates_by_terminal_effect_id
        .values()
        .flatten()
        .find(|candidate| {
            candidate.source_kind == DreamscopeSourceKind::FactorFamily
                && candidate.source_id == family_id
        })
        .map(|candidate| candidate.name.clone())
        .unwrap_or_default()
}

fn tree_contradictions(catalog: &DreamscopeBuildCatalog, selected: &BTreeSet<i64>) -> Vec<String> {
    let mut contradictions = BTreeSet::new();
    for node_id in selected {
        let Some(node) = catalog.tree_nodes_by_id.get(node_id) else {
            continue;
        };
        for other_id in selected.range((node_id + 1)..) {
            let Some(other) = catalog.tree_nodes_by_id.get(other_id) else {
                continue;
            };
            if node.mutually_exclusive_node_id == Some(*other_id)
                || other.mutually_exclusive_node_id == Some(*node_id)
            {
                contradictions.insert(format!(
                    "tree nodes {node_id} and {other_id} are mutually exclusive"
                ));
            }
        }
    }
    contradictions.into_iter().collect()
}

fn unique(values: &[i64]) -> Vec<i64> {
    values
        .iter()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use rlogs_events::{
        ActorEvent, ActorId, EVENT_SCHEMA_VERSION, EntityRef, EntityUuid, EventProvenance,
        EventSensitivity, EventTime, RegionContext, RegionIdentity, StatusEffectId, StatusEvent,
        StatusOrigin, StatusState, TimelineEvent,
    };

    use super::*;

    fn evidence() -> DreamscopeBuildEvidence {
        DreamscopeBuildEvidence {
            character_id: "3296036".to_owned(),
            ..DreamscopeBuildEvidence::default()
        }
    }

    fn envelope(build: &str, sequence: u64, kind: TimelineEventKind) -> EventEnvelope {
        let time = EventTime {
            observed_micros: sequence * 1_000,
            game_time_millis: None,
        };
        let provenance = EventProvenance::manual("Dreamscope inference test");
        EventEnvelope {
            schema_version: EVENT_SCHEMA_VERSION,
            session_id: "dreamscope-inference-test".into(),
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
                evidence: vec![],
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

    fn status_envelope(
        build: &str,
        sequence: u64,
        source_entity_uuid: i64,
        effect_id: i64,
    ) -> EventEnvelope {
        status_envelope_with_origin(build, sequence, source_entity_uuid, effect_id, None)
    }

    fn status_envelope_with_origin(
        build: &str,
        sequence: u64,
        source_entity_uuid: i64,
        effect_id: i64,
        origin: Option<(i32, i64)>,
    ) -> EventEnvelope {
        envelope(
            build,
            sequence,
            TimelineEventKind::Status(StatusEvent {
                source: Some(EntityRef {
                    actor_id: ActorId(1),
                    entity_uuid: EntityUuid(source_entity_uuid),
                }),
                target: EntityRef {
                    actor_id: ActorId(2),
                    entity_uuid: EntityUuid(999),
                },
                effect: StatusEffectId(effect_id),
                instance_id: None,
                origin: origin.map(|(source_type_id, source_config_id)| StatusOrigin {
                    source_type_id,
                    source_config_id,
                }),
                state: StatusState::Applied,
                stacks: Some(1),
                level: None,
                part_id: None,
                count: None,
                created_at_millis: None,
                duration_millis: Some(10_000),
            }),
        )
    }

    fn actor_envelope(
        build: &str,
        sequence: u64,
        entity_uuid: i64,
        kind: ActorKind,
    ) -> EventEnvelope {
        envelope(
            build,
            sequence,
            TimelineEventKind::Actor(ActorEvent {
                actor: EntityRef {
                    actor_id: ActorId(1),
                    entity_uuid: EntityUuid(entity_uuid),
                },
                state: ActorState::Spawned,
                entity_type_id: 1,
                kind,
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
                primary_loadout: vec![],
                auxiliary_loadout: vec![],
                loadout_observation: Default::default(),
            }),
        )
    }

    #[test]
    fn catalog_keeps_tree_nodes_and_factor_slots_separate() {
        let summary = dreamscope_catalog_summary();
        assert_eq!(summary.selectable_tree_nodes, 134);
        assert_eq!(summary.factor_slot_templates, 8);
        assert_eq!(summary.factor_slots, 40);
        assert_eq!(summary.factor_items, 3_830);
    }

    #[test]
    fn current_build_factor_item_lookup_retains_exact_family_and_grade() {
        let item = dreamscope_factor_item_by_id(20_010_001).unwrap();
        assert_eq!(item.item_id, 20_010_001);
        assert_eq!(item.family_id, 200_101);
        assert_eq!(item.grade, 1);
        assert!(dreamscope_factor_item_by_id(-1).is_none());
    }

    #[test]
    fn harmony_grace_runtime_effect_resolves_exact_tree_choice() {
        let mut input = evidence();
        input.terminal_effect_ids = vec![3_003_052];
        let result = infer_dreamscope_build(&input);
        assert_eq!(result.resolution, DreamscopeBuildResolution::Partial);
        assert_eq!(result.resolved_tree_node_ids, vec![1_506]);
        assert_eq!(
            result.matches[0].resolution,
            DreamscopeEvidenceResolution::Exact
        );
        assert_eq!(result.matches[0].candidates[0].name, "Harmony Grace");
        assert_eq!(
            result.matches[0].evidence_kind,
            DreamscopeEvidenceKind::RuntimeEffect
        );
        assert!(!result.complete_snapshot_observed);
    }

    #[test]
    fn shared_formula_endpoint_exposes_factor_families_without_guessing_grade() {
        let matched = dreamscope_observed_effect_match(9_901);
        assert_eq!(matched.evidence_kind, DreamscopeEvidenceKind::RuntimeEffect);
        assert_eq!(matched.resolution, DreamscopeEvidenceResolution::Ambiguous);

        let factor_families = matched
            .candidates
            .iter()
            .filter(|candidate| candidate.source_kind == DreamscopeSourceKind::FactorFamily)
            .map(|candidate| candidate.source_id)
            .collect::<Vec<_>>();
        assert_eq!(factor_families, vec![202_284, 202_289, 202_291, 202_301]);
        assert!(
            matched
                .candidates
                .iter()
                .all(|candidate| candidate.item_ids.len() > 1)
        );
    }

    #[test]
    fn packet_origin_collapses_harmony_grace_semantic_duplicates_to_one_tree_node() {
        let matched = dreamscope_observed_effect_origin_match(3_003_052, 1, 3_003_053);
        assert_eq!(matched.evidence_kind, DreamscopeEvidenceKind::EffectOrigin);
        assert_eq!(matched.resolution, DreamscopeEvidenceResolution::Exact);
        assert_eq!(matched.candidates.len(), 1);
        assert_eq!(
            matched.candidates[0].source_kind,
            DreamscopeSourceKind::TreeNode
        );
        assert_eq!(matched.candidates[0].source_id, 1_506);
    }

    #[test]
    fn direct_terminal_effect_remains_available_without_a_runtime_bridge() {
        let terminal_effect_id = catalog()
            .candidates_by_terminal_effect_id
            .keys()
            .copied()
            .find(|effect_id| {
                !catalog()
                    .candidates_by_runtime_effect_id
                    .contains_key(effect_id)
                    && shared_dreamscope_candidates(*effect_id, None).is_empty()
            })
            .expect("catalog should retain direct terminal effects");
        let matched = dreamscope_observed_effect_match(terminal_effect_id);
        assert_eq!(
            matched.evidence_kind,
            DreamscopeEvidenceKind::TerminalEffect
        );
        assert_ne!(matched.resolution, DreamscopeEvidenceResolution::Unknown);
    }

    #[test]
    fn shared_terminal_effect_remains_ambiguous() {
        let mut input = evidence();
        input.terminal_effect_ids = vec![3_002_080];
        let result = infer_dreamscope_build(&input);
        assert_eq!(result.resolution, DreamscopeBuildResolution::Ambiguous);
        assert_eq!(
            result.matches[0].resolution,
            DreamscopeEvidenceResolution::Ambiguous
        );
        assert_eq!(result.matches[0].candidates.len(), 8);
        assert!(result.resolved_tree_node_ids.is_empty());
    }

    #[test]
    fn exact_packet_origin_narrows_shared_effect_to_factor_family_without_inventing_grade() {
        let matched = dreamscope_observed_effect_origin_match(9_901, 1, 3_052_430);
        assert_eq!(matched.evidence_kind, DreamscopeEvidenceKind::EffectOrigin);
        assert_eq!(matched.source_type_id, Some(1));
        assert_eq!(matched.source_config_id, Some(3_052_430));
        assert_eq!(matched.resolution, DreamscopeEvidenceResolution::Partial);
        assert_eq!(matched.candidates.len(), 1);
        assert_eq!(
            matched.candidates[0].source_kind,
            DreamscopeSourceKind::FactorFamily
        );
        assert_eq!(matched.candidates[0].source_id, 202_289);
        assert!(matched.candidates[0].item_ids.len() > 1);
    }

    #[test]
    fn effect_origin_is_retained_when_status_arrives_before_player_identity() {
        let entity_uuid = (3_296_036_i64 << 16) | 1;
        let mut analyzer = DreamscopeBuildInferenceAnalyzer::new();
        analyzer
            .observe(&status_envelope_with_origin(
                dreamscope_catalog_game_build(),
                1,
                entity_uuid,
                9_901,
                Some((1, 3_052_430)),
            ))
            .unwrap();
        analyzer
            .observe(&actor_envelope(
                dreamscope_catalog_game_build(),
                2,
                entity_uuid,
                ActorKind::Player,
            ))
            .unwrap();

        let report = analyzer.finish();
        assert!(report.unresolved_provider_evidence.is_empty());
        assert_eq!(report.actors[0].resolved_factor_family_ids, vec![202_289]);
        assert!(report.actors[0].resolved_factor_item_ids.is_empty());
        assert_eq!(
            report.actors[0].matches[0].resolution,
            DreamscopeEvidenceResolution::Partial
        );
    }

    #[test]
    fn mutually_exclusive_exact_nodes_are_reported_as_a_contradiction() {
        let mut input = evidence();
        input.exact_tree_node_ids = Some(vec![1_506, 1_507]);
        let result = infer_dreamscope_build(&input);
        assert_eq!(result.resolution, DreamscopeBuildResolution::Ambiguous);
        assert_eq!(result.contradictions.len(), 1);
        assert!(result.contradictions[0].contains("1506 and 1507"));
    }

    #[test]
    fn exact_factor_item_retains_grade_while_buff_only_does_not() {
        let mut buff_only = evidence();
        buff_only.terminal_effect_ids = vec![3_050_010];
        let buff_result = infer_dreamscope_build(&buff_only);
        assert_eq!(
            buff_result.matches[0].resolution,
            DreamscopeEvidenceResolution::Partial
        );
        assert!(buff_result.resolved_factor_item_ids.is_empty());

        let mut exact = evidence();
        exact.exact_factor_item_ids = Some(vec![20_020_001]);
        let exact_result = infer_dreamscope_build(&exact);
        assert_eq!(
            exact_result.matches[0].resolution,
            DreamscopeEvidenceResolution::Exact
        );
        assert_eq!(exact_result.resolved_factor_item_ids, vec![20_020_001]);
        assert_eq!(exact_result.matches[0].candidates[0].grades, vec![1]);
    }

    #[test]
    fn terminal_effect_before_player_spawn_is_joined_to_the_character() {
        let entity_uuid = (3_296_036_i64 << 16) | 1;
        let mut analyzer = DreamscopeBuildInferenceAnalyzer::new();
        analyzer
            .observe(&status_envelope(
                dreamscope_catalog_game_build(),
                1,
                entity_uuid,
                3_003_050,
            ))
            .unwrap();
        analyzer
            .observe(&actor_envelope(
                dreamscope_catalog_game_build(),
                2,
                entity_uuid,
                ActorKind::Player,
            ))
            .unwrap();

        let report = analyzer.finish();
        assert!(report.unresolved_provider_evidence.is_empty());
        assert_eq!(report.actors.len(), 1);
        assert_eq!(report.actors[0].character_id, "3296036");
        assert_eq!(report.actors[0].resolved_tree_node_ids, vec![1_506]);
    }

    #[test]
    fn monster_source_is_never_promoted_to_a_character() {
        let entity_uuid = (3_296_036_i64 << 16) | 1;
        let mut analyzer = DreamscopeBuildInferenceAnalyzer::new();
        analyzer
            .observe(&status_envelope(
                dreamscope_catalog_game_build(),
                1,
                entity_uuid,
                3_003_050,
            ))
            .unwrap();
        analyzer
            .observe(&actor_envelope(
                dreamscope_catalog_game_build(),
                2,
                entity_uuid,
                ActorKind::Monster,
            ))
            .unwrap();

        let report = analyzer.finish();
        assert!(report.actors.is_empty());
        assert_eq!(report.unresolved_provider_evidence.len(), 1);
        assert_eq!(report.unresolved_provider_evidence[0].observation_count, 1);
        assert_eq!(report.unresolved_provider_evidence[0].first_sequence, 1);
        assert_eq!(report.unresolved_provider_evidence[0].last_sequence, 1);
    }

    #[test]
    fn repeated_pending_evidence_retains_accurate_audit_counts() {
        let entity_uuid = 44;
        let mut analyzer = DreamscopeBuildInferenceAnalyzer::new();
        for sequence in 1..=3 {
            analyzer
                .observe(&status_envelope(
                    dreamscope_catalog_game_build(),
                    sequence,
                    entity_uuid,
                    3_003_050,
                ))
                .unwrap();
        }

        let report = analyzer.finish();
        assert_eq!(report.unresolved_provider_evidence.len(), 1);
        let unresolved = &report.unresolved_provider_evidence[0];
        assert_eq!(unresolved.observation_count, 3);
        assert_eq!(unresolved.first_sequence, 1);
        assert_eq!(unresolved.last_sequence, 3);
    }

    #[test]
    fn build_mismatch_warns_without_disabling_inference() {
        let entity_uuid = (3_296_036_i64 << 16) | 1;
        let mut analyzer = DreamscopeBuildInferenceAnalyzer::new();
        analyzer
            .observe(&actor_envelope(
                "hotfix-build",
                1,
                entity_uuid,
                ActorKind::Player,
            ))
            .unwrap();
        analyzer
            .observe(&status_envelope("hotfix-build", 2, entity_uuid, 3_003_050))
            .unwrap();

        let report = analyzer.finish();
        assert!(report.catalog_out_of_date);
        assert_eq!(report.actors[0].resolved_tree_node_ids, vec![1_506]);
    }
}
