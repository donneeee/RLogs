use std::{collections::HashMap, sync::OnceLock};

use rlogs_events::{StatusEvent, StatusState};
use serde::{Deserialize, Serialize};

const EFFECT_ORIGIN_RUNTIME_JSON: &str =
    include_str!("../game-data/runtime/rdps-effect-origin-runtime.v1.json");
const EMPTY_CANDIDATES: &[EffectSourceCandidate] = &[];
const EMPTY_TERMINAL_IDS: &[i64] = &[];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectFingerprintResolution {
    Exact,
    Ambiguous,
    #[default]
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectSourceCandidate {
    pub source_id: String,
    pub source_kind: String,
    pub source_name: Option<String>,
    pub source_entity_id: Option<i64>,
    #[serde(default)]
    pub equipment_suit_selector: Option<EquipmentSuitSelector>,
    #[serde(default)]
    pub dreamscope_selector: Option<DreamscopeEffectSelector>,
}

/// Exact current-character equipment-set evidence carried by `SuitAttr`.
///
/// `map_key` identifies the equipped set family and `attribute_key` identifies
/// the selected attribute variant. Neither value is inferred from the terminal
/// effect because multiple set families can intentionally emit the same effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EquipmentSuitSelector {
    pub map_key: i32,
    pub attribute_key: i32,
    #[serde(default)]
    pub required_pieces: Option<i32>,
}

/// Exact current-build Dreamscope selector attached to a terminal effect.
///
/// The terminal effect is the formula endpoint. This selector records which
/// selectable source(s) can emit it. Factor families retain all concrete item
/// grades so an exact profile snapshot can select the equipped variant without
/// deriving that selection from the endpoint itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DreamscopeEffectSelector {
    pub source_kind: EffectDreamscopeSourceKind,
    pub source_id: i64,
    #[serde(default)]
    pub candidate_item_ids: Vec<i64>,
    #[serde(default)]
    pub candidate_grades: Vec<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectDreamscopeSourceKind {
    TreeNode,
    AdvancedTreeEffect,
    FactorFamily,
}

/// Direct selections observed for the provider whose terminal effect fired.
///
/// These slices must come from packet/profile state. Inferred build results are
/// intentionally not accepted here because using an endpoint to prove the
/// source that produced the same endpoint would be circular.
#[derive(Debug, Clone, Copy, Default)]
pub struct ExactDreamscopeLoadout<'a> {
    pub tree_node_ids: &'a [i64],
    pub advanced_effect_ids: &'a [i64],
    pub factor_item_ids: &'a [i64],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedEquippedEffectOwner<'a> {
    pub resolution: EffectFingerprintResolution,
    pub source: Option<&'a EffectSourceCandidate>,
    pub applicable_candidates: usize,
    pub matched_candidates: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectFingerprintMatchKind {
    ExactPacketOrigin,
    UncataloguedPacketOrigin,
    EffectFallback,
    #[default]
    Uncatalogued,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectFingerprintCatalogSummary {
    pub effects: usize,
    pub origin_fingerprints: usize,
    pub exact_origin_endpoints: usize,
    pub exact_origin_owners: usize,
}

/// Borrowed, constant-time resolution for one packet-observed status event.
///
/// Endpoint certainty answers which formula endpoint fired. Owner certainty is
/// deliberately separate: a terminal effect can be exact while the equipped
/// source that produced it remains ambiguous.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedStatusEffectFingerprint<'a> {
    pub effect_id: i64,
    pub source_type_id: Option<i32>,
    pub source_config_id: Option<i64>,
    pub provider_entity_uuid: Option<i64>,
    pub recipient_entity_uuid: i64,
    pub state: StatusState,
    pub match_kind: EffectFingerprintMatchKind,
    pub formula_endpoint_present: bool,
    pub transfer_proof_state: &'a str,
    pub endpoint_resolution: EffectFingerprintResolution,
    pub owner_resolution: EffectFingerprintResolution,
    pub candidate_sources: &'a [EffectSourceCandidate],
    pub unresolved_terminal_ids: &'a [i64],
}

#[derive(Debug, Deserialize)]
struct RawCatalog {
    schema_version: u16,
    game_build: String,
    summary: RawSummary,
    effects_by_id: HashMap<String, RawEffect>,
}

#[derive(Debug, Deserialize)]
struct RawSummary {
    effects: usize,
    origin_fingerprints: usize,
    exact_origin_endpoints: usize,
    exact_origin_owners: usize,
}

#[derive(Debug, Deserialize)]
struct RawEffect {
    formula_endpoint_present: bool,
    transfer_proof_state: String,
    fallback: Fingerprint,
    origins_by_key: HashMap<String, Fingerprint>,
}

#[derive(Debug, Deserialize)]
struct Fingerprint {
    endpoint_resolution: EffectFingerprintResolution,
    owner_resolution: EffectFingerprintResolution,
    candidate_sources: Vec<EffectSourceCandidate>,
    unresolved_terminal_ids: Vec<i64>,
}

#[derive(Debug)]
struct EffectRecord {
    formula_endpoint_present: bool,
    transfer_proof_state: String,
    fallback: Fingerprint,
    origins: HashMap<(i32, i64), Fingerprint>,
}

#[derive(Debug)]
struct Catalog {
    game_build: String,
    summary: EffectFingerprintCatalogSummary,
    effects: HashMap<i64, EffectRecord>,
}

static CATALOG: OnceLock<Catalog> = OnceLock::new();

pub fn effect_fingerprint_catalog_game_build() -> &'static str {
    catalog().game_build.as_str()
}

pub fn effect_fingerprint_catalog_summary() -> EffectFingerprintCatalogSummary {
    catalog().summary
}

pub fn resolve_status_effect_fingerprint(
    status: &StatusEvent,
) -> ResolvedStatusEffectFingerprint<'static> {
    resolve_effect_origin_fingerprint(
        status.effect.0,
        status
            .origin
            .map(|origin| (origin.source_type_id, origin.source_config_id)),
        status.source.map(|source| source.entity_uuid.0),
        status.target.entity_uuid.0,
        status.state,
    )
}

pub fn resolve_effect_origin_fingerprint(
    effect_id: i64,
    origin: Option<(i32, i64)>,
    provider_entity_uuid: Option<i64>,
    recipient_entity_uuid: i64,
    state: StatusState,
) -> ResolvedStatusEffectFingerprint<'static> {
    let Some(effect) = catalog().effects.get(&effect_id) else {
        return ResolvedStatusEffectFingerprint {
            effect_id,
            source_type_id: origin.map(|value| value.0),
            source_config_id: origin.map(|value| value.1),
            provider_entity_uuid,
            recipient_entity_uuid,
            state,
            match_kind: EffectFingerprintMatchKind::Uncatalogued,
            formula_endpoint_present: false,
            transfer_proof_state: "uncatalogued",
            endpoint_resolution: EffectFingerprintResolution::Unresolved,
            owner_resolution: EffectFingerprintResolution::Unresolved,
            candidate_sources: EMPTY_CANDIDATES,
            unresolved_terminal_ids: EMPTY_TERMINAL_IDS,
        };
    };

    let (match_kind, fingerprint) = match origin {
        Some(value) => match effect.origins.get(&value) {
            Some(fingerprint) => (
                EffectFingerprintMatchKind::ExactPacketOrigin,
                Some(fingerprint),
            ),
            None => (EffectFingerprintMatchKind::UncataloguedPacketOrigin, None),
        },
        None => (
            EffectFingerprintMatchKind::EffectFallback,
            Some(&effect.fallback),
        ),
    };

    ResolvedStatusEffectFingerprint {
        effect_id,
        source_type_id: origin.map(|value| value.0),
        source_config_id: origin.map(|value| value.1),
        provider_entity_uuid,
        recipient_entity_uuid,
        state,
        match_kind,
        formula_endpoint_present: effect.formula_endpoint_present,
        transfer_proof_state: effect.transfer_proof_state.as_str(),
        endpoint_resolution: fingerprint
            .map(|value| value.endpoint_resolution)
            .unwrap_or(EffectFingerprintResolution::Unresolved),
        owner_resolution: fingerprint
            .map(|value| value.owner_resolution)
            .unwrap_or(EffectFingerprintResolution::Unresolved),
        candidate_sources: fingerprint
            .map(|value| value.candidate_sources.as_slice())
            .unwrap_or(EMPTY_CANDIDATES),
        unresolved_terminal_ids: fingerprint
            .map(|value| value.unresolved_terminal_ids.as_slice())
            .unwrap_or(EMPTY_TERMINAL_IDS),
    }
}

/// Refines an effect's equipped owner using exact `SuitAttr` selections.
///
/// This is deliberately allocation-free for the live reducer. A formula
/// endpoint may be shared by multiple equipment families; only an exact
/// `(map_key, attribute_key)` pair may select one. Missing, crossed, or
/// duplicate evidence remains unresolved/ambiguous instead of being guessed.
pub fn resolve_equipped_effect_owner<'a>(
    fingerprint: &'a ResolvedStatusEffectFingerprint<'a>,
    equipped_suits: &[EquipmentSuitSelector],
) -> ResolvedEquippedEffectOwner<'a> {
    let applicable_candidates = fingerprint
        .candidate_sources
        .iter()
        .filter(|candidate| candidate.equipment_suit_selector.is_some())
        .count();

    if applicable_candidates == 0 {
        return ResolvedEquippedEffectOwner {
            resolution: fingerprint.owner_resolution,
            source: (fingerprint.owner_resolution == EffectFingerprintResolution::Exact
                && fingerprint.candidate_sources.len() == 1)
                .then(|| &fingerprint.candidate_sources[0]),
            applicable_candidates,
            matched_candidates: 0,
        };
    }

    let mut matched = fingerprint.candidate_sources.iter().filter(|candidate| {
        candidate.equipment_suit_selector.is_some_and(|required| {
            equipped_suits.iter().any(|equipped| {
                equipped.map_key == required.map_key
                    && equipped.attribute_key == required.attribute_key
            })
        })
    });
    let source = matched.next();
    let matched_candidates = usize::from(source.is_some()) + matched.count();
    let resolution = match matched_candidates {
        1 => EffectFingerprintResolution::Exact,
        2.. => EffectFingerprintResolution::Ambiguous,
        _ => EffectFingerprintResolution::Unresolved,
    };

    ResolvedEquippedEffectOwner {
        resolution,
        source: (matched_candidates == 1).then_some(source).flatten(),
        applicable_candidates,
        matched_candidates,
    }
}

/// Refines a terminal effect owner using only exact Dreamscope selections.
///
/// Resolution is allocation-free for the live reducer. A factor family is
/// selected only when one of its concrete candidate item IDs is present in the
/// provider snapshot; a family inferred from this same terminal effect is not
/// sufficient proof.
pub fn resolve_dreamscope_effect_owner<'a>(
    fingerprint: &'a ResolvedStatusEffectFingerprint<'a>,
    exact_loadout: ExactDreamscopeLoadout<'_>,
) -> ResolvedEquippedEffectOwner<'a> {
    let applicable_candidates = fingerprint
        .candidate_sources
        .iter()
        .filter(|candidate| candidate.dreamscope_selector.is_some())
        .count();

    if applicable_candidates == 0 {
        return ResolvedEquippedEffectOwner {
            resolution: fingerprint.owner_resolution,
            source: (fingerprint.owner_resolution == EffectFingerprintResolution::Exact
                && fingerprint.candidate_sources.len() == 1)
                .then(|| &fingerprint.candidate_sources[0]),
            applicable_candidates,
            matched_candidates: 0,
        };
    }

    let mut matched = fingerprint.candidate_sources.iter().filter(|candidate| {
        candidate
            .dreamscope_selector
            .as_ref()
            .is_some_and(|selector| match selector.source_kind {
                EffectDreamscopeSourceKind::TreeNode => {
                    exact_loadout.tree_node_ids.contains(&selector.source_id)
                }
                EffectDreamscopeSourceKind::AdvancedTreeEffect => exact_loadout
                    .advanced_effect_ids
                    .contains(&selector.source_id),
                EffectDreamscopeSourceKind::FactorFamily => selector
                    .candidate_item_ids
                    .iter()
                    .any(|item_id| exact_loadout.factor_item_ids.contains(item_id)),
            })
    });
    let source = matched.next();
    let matched_candidates = usize::from(source.is_some()) + matched.count();
    let resolution = match matched_candidates {
        1 => EffectFingerprintResolution::Exact,
        2.. => EffectFingerprintResolution::Ambiguous,
        _ => EffectFingerprintResolution::Unresolved,
    };

    ResolvedEquippedEffectOwner {
        resolution,
        source: (matched_candidates == 1).then_some(source).flatten(),
        applicable_candidates,
        matched_candidates,
    }
}

fn catalog() -> &'static Catalog {
    CATALOG.get_or_init(|| {
        let raw: RawCatalog = serde_json::from_str(EFFECT_ORIGIN_RUNTIME_JSON)
            .expect("embedded rDPS effect-origin runtime must be valid JSON");
        assert_eq!(
            raw.schema_version, 1,
            "unsupported rDPS effect-origin runtime schema"
        );

        let effects = raw
            .effects_by_id
            .into_iter()
            .map(|(effect_id, effect)| {
                let effect_id = effect_id
                    .parse::<i64>()
                    .expect("effect-origin runtime keys must be integer effect IDs");
                let origins = effect
                    .origins_by_key
                    .into_iter()
                    .map(|(key, fingerprint)| {
                        let (source_type_id, source_config_id) = key
                            .split_once(':')
                            .expect("effect-origin runtime origin keys require type:config");
                        (
                            (
                                source_type_id
                                    .parse::<i32>()
                                    .expect("source type must be i32"),
                                source_config_id
                                    .parse::<i64>()
                                    .expect("source config must be i64"),
                            ),
                            fingerprint,
                        )
                    })
                    .collect();
                (
                    effect_id,
                    EffectRecord {
                        formula_endpoint_present: effect.formula_endpoint_present,
                        transfer_proof_state: effect.transfer_proof_state,
                        fallback: effect.fallback,
                        origins,
                    },
                )
            })
            .collect::<HashMap<_, _>>();

        assert_eq!(
            effects.len(),
            raw.summary.effects,
            "effect catalog lost entries"
        );
        Catalog {
            game_build: raw.game_build,
            summary: EffectFingerprintCatalogSummary {
                effects: raw.summary.effects,
                origin_fingerprints: raw.summary.origin_fingerprints,
                exact_origin_endpoints: raw.summary.exact_origin_endpoints,
                exact_origin_owners: raw.summary.exact_origin_owners,
            },
            effects,
        }
    })
}

#[cfg(test)]
mod tests {
    use rlogs_events::{
        ActorId, EntityRef, EntityUuid, StatusEffectId, StatusEvent, StatusOrigin, StatusState,
    };

    use super::{
        EffectDreamscopeSourceKind, EffectFingerprintMatchKind, EffectFingerprintResolution,
        EquipmentSuitSelector, ExactDreamscopeLoadout, effect_fingerprint_catalog_summary,
        resolve_dreamscope_effect_owner, resolve_equipped_effect_owner,
        resolve_status_effect_fingerprint,
    };

    fn status(effect_id: i64, origin: Option<(i32, i64)>) -> StatusEvent {
        StatusEvent {
            source: Some(EntityRef {
                actor_id: ActorId(1),
                entity_uuid: EntityUuid(100),
            }),
            target: EntityRef {
                actor_id: ActorId(2),
                entity_uuid: EntityUuid(200),
            },
            effect: StatusEffectId(effect_id),
            instance_id: None,
            origin: origin.map(|(source_type_id, source_config_id)| StatusOrigin {
                source_type_id,
                source_config_id,
            }),
            state: StatusState::Applied,
            stacks: None,
            duration_millis: None,
            level: None,
            part_id: None,
            count: None,
            created_at_millis: None,
        }
    }

    #[test]
    fn exact_packet_origin_resolves_endpoint_and_equipped_owner() {
        let resolved = resolve_status_effect_fingerprint(&status(25_204, Some((1, 2_204_030))));
        assert_eq!(
            resolved.match_kind,
            EffectFingerprintMatchKind::ExactPacketOrigin
        );
        assert_eq!(
            resolved.endpoint_resolution,
            EffectFingerprintResolution::Exact
        );
        assert_eq!(
            resolved.owner_resolution,
            EffectFingerprintResolution::Exact
        );
        assert_eq!(resolved.candidate_sources.len(), 1);
        assert_eq!(resolved.candidate_sources[0].source_id, "talent:202");
    }

    #[test]
    fn effect_only_fallback_never_claims_packet_origin_evidence() {
        let resolved = resolve_status_effect_fingerprint(&status(25_204, None));
        assert_eq!(
            resolved.match_kind,
            EffectFingerprintMatchKind::EffectFallback
        );
    }

    #[test]
    fn uncatalogued_packet_origin_never_falls_back_to_another_owner() {
        let resolved = resolve_status_effect_fingerprint(&status(2_203_521, Some((1, 9_999_999))));
        assert_eq!(
            resolved.match_kind,
            EffectFingerprintMatchKind::UncataloguedPacketOrigin
        );
        assert!(resolved.formula_endpoint_present);
        assert_eq!(resolved.transfer_proof_state, "scope_unproven");
        assert_eq!(
            resolved.endpoint_resolution,
            EffectFingerprintResolution::Unresolved
        );
        assert_eq!(
            resolved.owner_resolution,
            EffectFingerprintResolution::Unresolved
        );
        assert!(resolved.candidate_sources.is_empty());
        assert!(resolved.unresolved_terminal_ids.is_empty());
        assert_eq!(resolved.source_type_id, Some(1));
        assert_eq!(resolved.source_config_id, Some(9_999_999));
    }

    #[test]
    fn shared_child_effect_uses_each_exact_packet_origin_owner() {
        for (source_config_id, expected_source_id) in [
            (2_203_520, "talent:1152"),
            (2_203_620, "talent:1162"),
            (2_203_650, "talent:1165"),
            (2_203_670, "talent:1167"),
        ] {
            let resolved =
                resolve_status_effect_fingerprint(&status(2_203_521, Some((1, source_config_id))));
            assert_eq!(
                resolved.match_kind,
                EffectFingerprintMatchKind::ExactPacketOrigin
            );
            assert_eq!(
                resolved.endpoint_resolution,
                EffectFingerprintResolution::Exact
            );
            assert_eq!(
                resolved.owner_resolution,
                EffectFingerprintResolution::Exact
            );
            assert_eq!(resolved.candidate_sources.len(), 1);
            assert_eq!(resolved.candidate_sources[0].source_id, expected_source_id);
        }

        let fallback = resolve_status_effect_fingerprint(&status(2_203_521, None));
        assert_eq!(
            fallback.endpoint_resolution,
            EffectFingerprintResolution::Ambiguous
        );
        assert_eq!(
            fallback.owner_resolution,
            EffectFingerprintResolution::Exact
        );
        assert_eq!(fallback.candidate_sources.len(), 4);
    }

    #[test]
    fn overhealing_cooldown_preserves_buff_origin_without_claiming_a_formula_endpoint() {
        let resolved = resolve_status_effect_fingerprint(&status(2_202_113, Some((1, 21_423))));
        assert_eq!(
            resolved.match_kind,
            EffectFingerprintMatchKind::ExactPacketOrigin
        );
        assert_eq!(
            resolved.endpoint_resolution,
            EffectFingerprintResolution::Exact
        );
        assert_eq!(
            resolved.owner_resolution,
            EffectFingerprintResolution::Exact
        );
        assert!(!resolved.formula_endpoint_present);
        assert_eq!(resolved.transfer_proof_state, "non_damage_internal_marker");
        assert_eq!(resolved.candidate_sources.len(), 1);
        assert_eq!(resolved.candidate_sources[0].source_id, "buff-source:21423");
        assert_eq!(resolved.candidate_sources[0].source_kind, "buff");
        assert_eq!(
            resolved.candidate_sources[0].source_name.as_deref(),
            Some("Symbiotic Mark")
        );
    }

    #[test]
    fn ambiguous_factor_owner_is_not_promoted_to_exact() {
        let resolved = resolve_status_effect_fingerprint(&status(9_901, None));
        assert_eq!(
            resolved.endpoint_resolution,
            EffectFingerprintResolution::Ambiguous
        );
        assert_eq!(
            resolved.owner_resolution,
            EffectFingerprintResolution::Ambiguous
        );
        assert!(resolved.candidate_sources.len() > 1);
    }

    #[test]
    fn duplicate_dreamscope_table_aliases_resolve_to_one_tree_node() {
        let resolved = resolve_status_effect_fingerprint(&status(3_003_052, Some((1, 3_003_053))));
        assert_eq!(
            resolved.endpoint_resolution,
            EffectFingerprintResolution::Exact
        );
        assert_eq!(
            resolved.owner_resolution,
            EffectFingerprintResolution::Exact
        );
        assert_eq!(resolved.candidate_sources.len(), 1);
        assert_eq!(
            resolved.candidate_sources[0].source_id,
            "dreamscope-tree_node:1506:terminal:3003050"
        );
        assert_eq!(resolved.candidate_sources[0].source_entity_id, Some(1_506));
    }

    #[test]
    fn talent_runtime_endpoint_resolves_to_owning_tree_node() {
        let resolved = resolve_status_effect_fingerprint(&status(2_204_320, None));
        assert_eq!(
            resolved.match_kind,
            EffectFingerprintMatchKind::EffectFallback
        );
        assert_eq!(
            resolved.endpoint_resolution,
            EffectFingerprintResolution::Exact
        );
        assert_eq!(
            resolved.owner_resolution,
            EffectFingerprintResolution::Exact
        );
        assert_eq!(resolved.candidate_sources.len(), 1);
        assert_eq!(resolved.candidate_sources[0].source_id, "talent:235");
        assert_eq!(resolved.candidate_sources[0].source_entity_id, Some(235));
    }

    #[test]
    fn exact_suit_pair_resolves_a_shared_terminal_effect_owner() {
        let resolved = resolve_status_effect_fingerprint(&status(2_407_280, None));
        assert_eq!(
            resolved.owner_resolution,
            EffectFingerprintResolution::Ambiguous
        );

        let selected = resolve_equipped_effect_owner(
            &resolved,
            &[EquipmentSuitSelector {
                map_key: 101,
                attribute_key: 464,
                required_pieces: Some(2),
            }],
        );
        assert_eq!(selected.resolution, EffectFingerprintResolution::Exact);
        assert_eq!(selected.applicable_candidates, 2);
        assert_eq!(selected.matched_candidates, 1);
        let source = selected
            .source
            .expect("exact suit pair must select a source");
        assert_eq!(source.source_id, "equipment-set:101:2:variant:464");
        assert_eq!(
            source.equipment_suit_selector,
            Some(EquipmentSuitSelector {
                map_key: 101,
                attribute_key: 464,
                required_pieces: Some(2),
            })
        );
    }

    #[test]
    fn the_other_exact_suit_family_selects_its_own_source() {
        let resolved = resolve_status_effect_fingerprint(&status(2_407_280, None));
        let selected = resolve_equipped_effect_owner(
            &resolved,
            &[EquipmentSuitSelector {
                map_key: 102,
                attribute_key: 1_786,
                required_pieces: Some(2),
            }],
        );

        assert_eq!(selected.resolution, EffectFingerprintResolution::Exact);
        assert_eq!(
            selected.source.map(|source| source.source_id.as_str()),
            Some("equipment-set:102:2:variant:1786")
        );
    }

    #[test]
    fn crossed_suit_keys_never_guess_a_shared_effect_owner() {
        let resolved = resolve_status_effect_fingerprint(&status(2_407_280, None));
        let selected = resolve_equipped_effect_owner(
            &resolved,
            &[EquipmentSuitSelector {
                map_key: 101,
                attribute_key: 1_786,
                required_pieces: Some(2),
            }],
        );

        assert_eq!(selected.resolution, EffectFingerprintResolution::Unresolved);
        assert_eq!(selected.matched_candidates, 0);
        assert!(selected.source.is_none());
    }

    #[test]
    fn exact_tree_node_selects_the_terminal_effect_owner() {
        let resolved = resolve_status_effect_fingerprint(&status(3_003_052, Some((1, 3_003_053))));
        let selected = resolve_dreamscope_effect_owner(
            &resolved,
            ExactDreamscopeLoadout {
                tree_node_ids: &[1_506],
                ..Default::default()
            },
        );

        assert_eq!(selected.resolution, EffectFingerprintResolution::Exact);
        assert_eq!(selected.applicable_candidates, 1);
        assert_eq!(selected.matched_candidates, 1);
        let source = selected
            .source
            .expect("exact tree node must select its terminal effect owner");
        assert_eq!(
            source.source_id,
            "dreamscope-tree_node:1506:terminal:3003050"
        );
        assert_eq!(
            source
                .dreamscope_selector
                .as_ref()
                .map(|selector| selector.source_kind),
            Some(EffectDreamscopeSourceKind::TreeNode)
        );
    }

    #[test]
    fn exact_factor_item_disambiguates_a_shared_terminal_effect() {
        let resolved = resolve_status_effect_fingerprint(&status(9_901, None));
        let selected = resolve_dreamscope_effect_owner(
            &resolved,
            ExactDreamscopeLoadout {
                factor_item_ids: &[20_021_881],
                ..Default::default()
            },
        );

        assert_eq!(selected.resolution, EffectFingerprintResolution::Exact);
        assert_eq!(selected.applicable_candidates, 4);
        assert_eq!(selected.matched_candidates, 1);
        assert_eq!(
            selected.source.map(|source| source.source_id.as_str()),
            Some("dreamscope-factor_family:202289:terminal:3052430")
        );
    }

    #[test]
    fn absent_factor_selection_never_guesses_a_shared_terminal_effect() {
        let resolved = resolve_status_effect_fingerprint(&status(9_901, None));
        let selected = resolve_dreamscope_effect_owner(
            &resolved,
            ExactDreamscopeLoadout {
                factor_item_ids: &[99_999_999],
                ..Default::default()
            },
        );

        assert_eq!(selected.resolution, EffectFingerprintResolution::Unresolved);
        assert_eq!(selected.applicable_candidates, 4);
        assert_eq!(selected.matched_candidates, 0);
        assert!(selected.source.is_none());
    }

    #[test]
    fn unresolved_origin_retains_terminal_formula_evidence() {
        let resolved = resolve_status_effect_fingerprint(&status(25_401, Some((1, 27_004))));
        assert_eq!(
            resolved.owner_resolution,
            EffectFingerprintResolution::Unresolved
        );
        assert!(resolved.unresolved_terminal_ids.contains(&27_003));
    }

    #[test]
    fn uncatalogued_packet_effect_is_returned_not_hidden() {
        let resolved = resolve_status_effect_fingerprint(&status(9_999_999_999, Some((10, 7))));
        assert_eq!(
            resolved.match_kind,
            EffectFingerprintMatchKind::Uncatalogued
        );
        assert_eq!(resolved.source_type_id, Some(10));
        assert_eq!(resolved.source_config_id, Some(7));
        assert_eq!(
            resolved.endpoint_resolution,
            EffectFingerprintResolution::Unresolved
        );
    }

    #[test]
    fn embedded_catalog_conserves_generated_summary() {
        let summary = effect_fingerprint_catalog_summary();
        assert_eq!(summary.effects, 1_699);
        assert_eq!(summary.origin_fingerprints, 616);
        assert_eq!(summary.exact_origin_endpoints, 350);
        assert_eq!(summary.exact_origin_owners, 334);
    }
}
