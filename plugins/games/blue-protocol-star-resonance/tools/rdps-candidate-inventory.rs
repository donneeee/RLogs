use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    fs::{self, File},
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

const CANDIDATE_INVENTORY_SCHEMA_VERSION: u16 = 2;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OriginCatalog {
    schema_version: u16,
    game_build: String,
    policy: String,
    summary: Value,
    effects: Vec<ObservedEffect>,
    relations: Vec<ObservedOriginRelation>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObservedEffect {
    effect_id: i64,
    status_events: u64,
    window_count: u64,
    cross_actor_window_count: u64,
    source_missing_window_count: u64,
    source_player_window_count: u64,
    target_player_window_count: u64,
    target_monster_window_count: u64,
    cross_actor_provider_recipient_windows: ProviderRecipientMatrix,
    applied: u64,
    refreshed: u64,
    stacked: u64,
    consumed: u64,
    removed: u64,
    minimum_stacks: Option<u32>,
    maximum_stacks: Option<u32>,
    packet_origin_observations: u64,
    source_relation_count: usize,
    observed_sessions: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProviderRecipientMatrix {
    resolved_player_to_player: u64,
    resolved_player_to_monster: u64,
    resolved_player_to_other: u64,
    non_player_to_player: u64,
    non_player_to_monster: u64,
    non_player_to_other: u64,
    unresolved_to_player: u64,
    unresolved_to_monster: u64,
    unresolved_to_other: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ObservedOriginRelation {
    effect_id: i64,
    source_type_id: i32,
    source_kind: Option<String>,
    configured_source_table: Option<String>,
    source_config_id: i64,
    observation_count: u64,
    observed_sessions: Vec<String>,
}

#[derive(Debug, Serialize)]
struct CandidateInventory {
    schema_version: u16,
    game_build: String,
    generated_by: &'static str,
    policy: InventoryPolicy,
    inputs: InventoryInputs,
    summary: InventorySummary,
    candidates: Vec<Candidate>,
}

#[derive(Debug, Serialize)]
struct InventoryPolicy {
    selection: &'static str,
    packet_origin_is_exact: bool,
    temporal_correlation_is_origin: bool,
    formula_metadata_alone_enables_rdps: bool,
    unresolved_evidence_is_hidden: bool,
    enablement_gate: &'static str,
}

#[derive(Debug, Serialize)]
struct InventoryInputs {
    origin_catalog: String,
    display_bridge: String,
    current_source_rows: String,
    formula_term_table: String,
    module_effect_catalog_directory: String,
}

#[derive(Debug, Serialize)]
struct InventorySummary {
    packet_observed_effects: usize,
    exact_player_provider_to_player_recipient_effects: usize,
    exact_player_provider_to_player_recipient_windows: u64,
    exact_player_provider_to_monster_recipient_effects: usize,
    exact_player_provider_to_monster_recipient_windows: u64,
    exact_cross_actor_candidate_effects: usize,
    candidates_with_current_buff_row: usize,
    candidates_with_packet_origin: usize,
    candidates_with_formula_evidence: usize,
    candidates_without_formula_evidence: usize,
    module_effect_families_loaded: usize,
    module_effect_levels_loaded: usize,
    candidates_with_module_effect_evidence: usize,
    linked_module_effect_families: usize,
    linked_module_effect_levels: usize,
    automatically_enabled_for_rdps: usize,
}

#[derive(Debug, Serialize)]
struct Candidate {
    effect_id: i64,
    exact_player_provider_to_player_recipient_windows: u64,
    exact_player_provider_to_monster_recipient_windows: u64,
    selection_reasons: Vec<&'static str>,
    full_cross_actor_matrix: ProviderRecipientMatrix,
    lifecycle: Lifecycle,
    observed_sessions: Vec<String>,
    current_buff_row: Option<CurrentBuffIdentity>,
    packet_origins: Vec<ObservedOriginRelation>,
    exact_related_buff_ids: Vec<RelatedBuffId>,
    formula_evidence: Vec<FormulaEvidence>,
    module_effect_evidence: Vec<ModuleEffectEvidence>,
    rdps_enablement: RdpsEnablement,
}

#[derive(Debug, Deserialize)]
struct ModuleEffectCatalogFile {
    schema_version: u16,
    kind: String,
    id: i64,
    stable_key: String,
    attributes: ModuleEffectAttributes,
    availability: Vec<ModuleEffectAvailability>,
    provenance: ModuleEffectProvenance,
}

#[derive(Debug, Deserialize)]
struct ModuleEffectAttributes {
    name_id: i64,
    levels: Vec<ModuleEffectLevel>,
}

#[derive(Debug, Deserialize)]
struct ModuleEffectLevel {
    row_id: i64,
    level: i64,
    name_id: i64,
    overview_id: i64,
    required_link_points: i64,
    effect_config_records: Vec<Vec<i64>>,
    effect_keys: Vec<Vec<String>>,
    effect_values: Vec<Vec<i64>>,
    fight_value: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ModuleEffectAvailability {
    deployment_id: String,
    channel: String,
    client_build: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ModuleEffectProvenance {
    source: String,
    reference: String,
    confidence: String,
}

#[derive(Debug, Serialize)]
struct ModuleEffectEvidence {
    module_effect_id: i64,
    stable_key: String,
    name_localization_id: i64,
    matching_levels: Vec<ModuleEffectLevelEvidence>,
    availability: Vec<ModuleEffectAvailability>,
    provenance: ModuleEffectProvenance,
}

#[derive(Debug, Serialize)]
struct ModuleEffectLevelEvidence {
    row_id: i64,
    level: i64,
    name_localization_id: i64,
    overview_localization_id: i64,
    required_link_points: i64,
    linked_runtime_buff_ids: Vec<i64>,
    matching_related_buff_ids: Vec<i64>,
    effect_config_records: Vec<Vec<i64>>,
    effect_keys: Vec<Vec<String>>,
    effect_values: Vec<Vec<i64>>,
    fight_value: i64,
}

#[derive(Debug, Serialize)]
struct Lifecycle {
    status_events: u64,
    windows: u64,
    cross_actor_windows: u64,
    source_missing_windows: u64,
    source_player_windows: u64,
    target_player_windows: u64,
    target_monster_windows: u64,
    applied: u64,
    refreshed: u64,
    stacked: u64,
    consumed: u64,
    removed: u64,
    minimum_stacks: Option<u32>,
    maximum_stacks: Option<u32>,
    packet_origin_observations: u64,
    source_relation_count: usize,
}

#[derive(Debug, Clone, Serialize)]
struct CurrentBuffIdentity {
    resolution: Option<String>,
    technical_name: Option<String>,
    name_localization_id: Option<i64>,
    description_localization_id: Option<i64>,
    icon_path: Option<String>,
    visible_mode_id: Option<i64>,
    visible_scope: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct RelatedBuffId {
    buff_id: i64,
    relationship: String,
    evidence: String,
}

#[derive(Debug, Serialize)]
struct FormulaEvidence {
    buff_id: i64,
    relationship: String,
    evidence: String,
    key: String,
    name: Option<String>,
    formula_readiness: Option<String>,
    value_resolution: Option<String>,
    scope_kinds: Vec<String>,
    stack_policy: Option<String>,
    formula_zone_ids: Vec<String>,
    component_value_hints: Value,
    runtime_proof_required: Vec<String>,
}

#[derive(Debug, Serialize)]
struct RdpsEnablement {
    enabled: bool,
    state: &'static str,
    missing_proofs: [&'static str; 3],
}

fn main() {
    if let Err(error) = run() {
        eprintln!("BPSR rDPS candidate inventory failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = env::args_os().skip(1);
    let origin_catalog_path = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    let display_bridge_path = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    let current_rows_path = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    let formula_terms_path = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    let module_effect_catalog_path = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    let output_path = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    if arguments.next().is_some() {
        return Err(usage().into());
    }

    let catalog: OriginCatalog = read_json(&origin_catalog_path)?;
    if catalog.schema_version != 2 || catalog.effects.is_empty() {
        return Err("origin catalog must be a non-empty schema-2 catalog".into());
    }
    if catalog.policy != "packet_observed_relationships_only_no_inferred_origins"
        || !catalog.summary.is_object()
    {
        return Err(
            "origin catalog policy or summary is not the expected exact packet catalog".into(),
        );
    }
    let display_bridge: Value = read_json(&display_bridge_path)?;
    let current_rows: Value = read_json(&current_rows_path)?;
    let formula_terms: Value = read_json(&formula_terms_path)?;
    let module_effect_catalog = read_module_effect_catalog(&module_effect_catalog_path)?;

    let inventory = build_inventory(
        catalog,
        &display_bridge,
        &current_rows,
        &formula_terms,
        &module_effect_catalog,
        InventoryInputs {
            origin_catalog: display_path(&origin_catalog_path),
            display_bridge: display_path(&display_bridge_path),
            current_source_rows: display_path(&current_rows_path),
            formula_term_table: display_path(&formula_terms_path),
            module_effect_catalog_directory: display_path(&module_effect_catalog_path),
        },
    )?;

    let mut writer = BufWriter::new(File::create(output_path)?);
    serde_json::to_writer_pretty(&mut writer, &inventory)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn build_inventory(
    catalog: OriginCatalog,
    display_bridge: &Value,
    current_rows: &Value,
    formula_terms: &Value,
    module_effect_catalog: &[ModuleEffectCatalogFile],
    inputs: InventoryInputs,
) -> Result<CandidateInventory, Box<dyn std::error::Error>> {
    let relation_rows = display_bridge
        .get("relations")
        .and_then(Value::as_array)
        .ok_or("display bridge lacks relations")?;
    let current_effect_rows = current_rows
        .get("observed_effects")
        .and_then(Value::as_array)
        .ok_or("current source rows lack observed_effects")?;
    let formula_entries = formula_terms
        .get("entriesByKey")
        .and_then(Value::as_object)
        .ok_or("formula term table lacks entriesByKey")?;

    let current_by_id = current_effect_rows
        .iter()
        .filter_map(|row| value_i64(row.get("effect_id")?).map(|id| (id, row)))
        .collect::<BTreeMap<_, _>>();
    let bridge_by_id = relation_rows.iter().fold(
        BTreeMap::<i64, Vec<&Value>>::new(),
        |mut result, relation| {
            if let Some(id) = relation.get("effect_id").and_then(value_i64) {
                result.entry(id).or_default().push(relation);
            }
            result
        },
    );
    let origins_by_id = catalog.relations.iter().cloned().fold(
        BTreeMap::<i64, Vec<ObservedOriginRelation>>::new(),
        |mut result, relation| {
            result.entry(relation.effect_id).or_default().push(relation);
            result
        },
    );

    let packet_observed_effects = catalog.effects.len();
    let mut candidates = Vec::new();
    for effect in catalog.effects.into_iter().filter(|effect| {
        let matrix = &effect.cross_actor_provider_recipient_windows;
        matrix.resolved_player_to_player > 0 || matrix.resolved_player_to_monster > 0
    }) {
        let packet_origins = origins_by_id
            .get(&effect.effect_id)
            .cloned()
            .unwrap_or_default();
        let bridges = bridge_by_id
            .get(&effect.effect_id)
            .cloned()
            .unwrap_or_default();
        let related_ids = exact_related_buff_ids(effect.effect_id, &packet_origins, &bridges);
        let formula_evidence = related_ids
            .iter()
            .filter_map(|related| {
                formula_evidence(
                    related,
                    formula_entries.get(&format!("buffs:{}", related.buff_id))?,
                )
            })
            .collect::<Vec<_>>();
        let module_effect_evidence =
            matching_module_effect_evidence(&related_ids, module_effect_catalog);
        let current_buff_row = current_by_id
            .get(&effect.effect_id)
            .and_then(|row| current_buff_identity(row));

        let player_to_player = effect
            .cross_actor_provider_recipient_windows
            .resolved_player_to_player;
        let player_to_monster = effect
            .cross_actor_provider_recipient_windows
            .resolved_player_to_monster;
        let mut selection_reasons = Vec::with_capacity(2);
        if player_to_player > 0 {
            selection_reasons.push("player-provider-to-different-player-recipient");
        }
        if player_to_monster > 0 {
            selection_reasons.push("player-provider-to-monster-recipient");
        }

        candidates.push(Candidate {
            effect_id: effect.effect_id,
            exact_player_provider_to_player_recipient_windows: player_to_player,
            exact_player_provider_to_monster_recipient_windows: player_to_monster,
            selection_reasons,
            full_cross_actor_matrix: effect.cross_actor_provider_recipient_windows,
            lifecycle: Lifecycle {
                status_events: effect.status_events,
                windows: effect.window_count,
                cross_actor_windows: effect.cross_actor_window_count,
                source_missing_windows: effect.source_missing_window_count,
                source_player_windows: effect.source_player_window_count,
                target_player_windows: effect.target_player_window_count,
                target_monster_windows: effect.target_monster_window_count,
                applied: effect.applied,
                refreshed: effect.refreshed,
                stacked: effect.stacked,
                consumed: effect.consumed,
                removed: effect.removed,
                minimum_stacks: effect.minimum_stacks,
                maximum_stacks: effect.maximum_stacks,
                packet_origin_observations: effect.packet_origin_observations,
                source_relation_count: effect.source_relation_count,
            },
            observed_sessions: effect.observed_sessions,
            current_buff_row,
            packet_origins,
            exact_related_buff_ids: related_ids,
            formula_evidence,
            module_effect_evidence,
            rdps_enablement: RdpsEnablement {
                enabled: false,
                state: "candidate-requires-runtime-mechanic-proof",
                missing_proofs: [
                    "party-benefit-not-owner-only",
                    "exact-formula-component-and-magnitude",
                    "source-on/source-off-recipient-damage-replay",
                ],
            },
        });
    }
    candidates.sort_by_key(|candidate| {
        (
            std::cmp::Reverse(
                candidate
                    .exact_player_provider_to_player_recipient_windows
                    .saturating_add(candidate.exact_player_provider_to_monster_recipient_windows),
            ),
            candidate.effect_id,
        )
    });

    let summary = InventorySummary {
        packet_observed_effects,
        exact_player_provider_to_player_recipient_effects: candidates
            .iter()
            .filter(|candidate| candidate.exact_player_provider_to_player_recipient_windows > 0)
            .count(),
        exact_player_provider_to_player_recipient_windows: candidates
            .iter()
            .map(|candidate| candidate.exact_player_provider_to_player_recipient_windows)
            .sum(),
        exact_player_provider_to_monster_recipient_effects: candidates
            .iter()
            .filter(|candidate| candidate.exact_player_provider_to_monster_recipient_windows > 0)
            .count(),
        exact_player_provider_to_monster_recipient_windows: candidates
            .iter()
            .map(|candidate| candidate.exact_player_provider_to_monster_recipient_windows)
            .sum(),
        exact_cross_actor_candidate_effects: candidates.len(),
        candidates_with_current_buff_row: candidates
            .iter()
            .filter(|candidate| candidate.current_buff_row.is_some())
            .count(),
        candidates_with_packet_origin: candidates
            .iter()
            .filter(|candidate| !candidate.packet_origins.is_empty())
            .count(),
        candidates_with_formula_evidence: candidates
            .iter()
            .filter(|candidate| !candidate.formula_evidence.is_empty())
            .count(),
        candidates_without_formula_evidence: candidates
            .iter()
            .filter(|candidate| candidate.formula_evidence.is_empty())
            .count(),
        module_effect_families_loaded: module_effect_catalog.len(),
        module_effect_levels_loaded: module_effect_catalog
            .iter()
            .map(|family| family.attributes.levels.len())
            .sum(),
        candidates_with_module_effect_evidence: candidates
            .iter()
            .filter(|candidate| !candidate.module_effect_evidence.is_empty())
            .count(),
        linked_module_effect_families: candidates
            .iter()
            .map(|candidate| candidate.module_effect_evidence.len())
            .sum(),
        linked_module_effect_levels: candidates
            .iter()
            .flat_map(|candidate| candidate.module_effect_evidence.iter())
            .map(|family| family.matching_levels.len())
            .sum(),
        automatically_enabled_for_rdps: 0,
    };

    Ok(CandidateInventory {
        schema_version: CANDIDATE_INVENTORY_SCHEMA_VERSION,
        game_build: catalog.game_build,
        generated_by: "rlogs-bpsr-rdps-candidate-inventory",
        policy: InventoryPolicy {
            selection: "exact resolved player-provider to different player-recipient or monster-recipient packet windows",
            packet_origin_is_exact: true,
            temporal_correlation_is_origin: false,
            formula_metadata_alone_enables_rdps: false,
            unresolved_evidence_is_hidden: false,
            enablement_gate: "exact party scope plus formula magnitude plus source-on/source-off recipient-damage replay",
        },
        inputs,
        summary,
        candidates,
    })
}

fn read_module_effect_catalog(
    directory: &Path,
) -> Result<Vec<ModuleEffectCatalogFile>, Box<dyn std::error::Error>> {
    let mut paths = fs::read_dir(directory)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|extension| extension.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    paths.sort();

    let mut result = Vec::with_capacity(paths.len());
    for path in paths {
        let family: ModuleEffectCatalogFile = read_json(&path)?;
        if family.schema_version != 2 || family.kind != "module_effect" {
            return Err(format!(
                "module-effect catalog file is not schema-2 module_effect: {}",
                path.display()
            )
            .into());
        }
        result.push(family);
    }
    if result.is_empty() {
        return Err("module-effect catalog directory contains no JSON families".into());
    }
    Ok(result)
}

fn matching_module_effect_evidence(
    related_ids: &[RelatedBuffId],
    catalog: &[ModuleEffectCatalogFile],
) -> Vec<ModuleEffectEvidence> {
    let related = related_ids
        .iter()
        .map(|row| row.buff_id)
        .collect::<BTreeSet<_>>();
    let mut result = catalog
        .iter()
        .filter_map(|family| {
            let matching_levels = family
                .attributes
                .levels
                .iter()
                .filter_map(|level| {
                    let linked_runtime_buff_ids = level
                        .effect_config_records
                        .iter()
                        .filter(|record| record.first() == Some(&3))
                        .filter_map(|record| record.get(1).copied())
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect::<Vec<_>>();
                    let matching_related_buff_ids = linked_runtime_buff_ids
                        .iter()
                        .copied()
                        .filter(|buff_id| related.contains(buff_id))
                        .collect::<Vec<_>>();
                    (!matching_related_buff_ids.is_empty()).then(|| ModuleEffectLevelEvidence {
                        row_id: level.row_id,
                        level: level.level,
                        name_localization_id: level.name_id,
                        overview_localization_id: level.overview_id,
                        required_link_points: level.required_link_points,
                        linked_runtime_buff_ids,
                        matching_related_buff_ids,
                        effect_config_records: level.effect_config_records.clone(),
                        effect_keys: level.effect_keys.clone(),
                        effect_values: level.effect_values.clone(),
                        fight_value: level.fight_value,
                    })
                })
                .collect::<Vec<_>>();
            (!matching_levels.is_empty()).then(|| ModuleEffectEvidence {
                module_effect_id: family.id,
                stable_key: family.stable_key.clone(),
                name_localization_id: family.attributes.name_id,
                matching_levels,
                availability: family.availability.clone(),
                provenance: family.provenance.clone(),
            })
        })
        .collect::<Vec<_>>();
    result.sort_by_key(|family| family.module_effect_id);
    result
}

fn exact_related_buff_ids(
    effect_id: i64,
    packet_origins: &[ObservedOriginRelation],
    bridge_relations: &[&Value],
) -> Vec<RelatedBuffId> {
    let mut result = BTreeSet::new();
    insert_related(
        &mut result,
        effect_id,
        "observed-effect",
        "packet-status-effect-id",
    );
    for origin in packet_origins
        .iter()
        .filter(|origin| origin.source_type_id == 1)
    {
        insert_related(
            &mut result,
            origin.source_config_id,
            "packet-origin",
            "exact-StatusOrigin-source-type-1",
        );
    }
    for relation in bridge_relations {
        for path_key in [
            "packet_visible_ancestor_paths",
            "packet_exact_parent_ancestor_paths",
        ] {
            for path in relation
                .get(path_key)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                for link in path
                    .get("chain")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if link.get("source_type_id").and_then(value_i64) == Some(1) {
                        if let Some(id) = link.get("source_config_id").and_then(value_i64) {
                            insert_related(
                                &mut result,
                                id,
                                "packet-ancestor",
                                "exact-transitive-StatusOrigin-chain",
                            );
                        }
                    }
                }
            }
        }
        if let Some(id) = relation
            .pointer("/effect_official_presentation/adjacent_parent_buff_id")
            .and_then(value_i64)
        {
            insert_related(
                &mut result,
                id,
                "official-adjacent-parent",
                "exact-current-game-presentation-link",
            );
        }
        if let Some(id) = relation
            .pointer("/exact_equipment_set_parent/buff_id")
            .and_then(value_i64)
        {
            insert_related(
                &mut result,
                id,
                "equipment-set-parent",
                "exact-current-equipment-set-link",
            );
        }
        for candidate in relation
            .get("exact_bdtag_parent_candidates")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(id) = candidate.get("source_buff_id").and_then(value_i64) {
                insert_related(
                    &mut result,
                    id,
                    "battle-imagine-buff-parent",
                    "exact-current-BDTag-source-buff-link",
                );
            }
        }
        for candidate in relation
            .get("parent_candidates")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            for id in candidate
                .get("buff_ids")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(value_i64)
            {
                insert_related(
                    &mut result,
                    id,
                    "exact-parent-candidate",
                    "current-game-table-parent-link-preserved-without-selection",
                );
            }
        }
    }
    result.into_iter().collect()
}

fn insert_related(
    result: &mut BTreeSet<RelatedBuffId>,
    buff_id: i64,
    relationship: &str,
    evidence: &str,
) {
    result.insert(RelatedBuffId {
        buff_id,
        relationship: relationship.to_string(),
        evidence: evidence.to_string(),
    });
}

fn formula_evidence(related: &RelatedBuffId, entry: &Value) -> Option<FormulaEvidence> {
    Some(FormulaEvidence {
        buff_id: related.buff_id,
        relationship: related.relationship.clone(),
        evidence: related.evidence.clone(),
        key: entry.get("key")?.as_str()?.to_string(),
        name: value_string(entry.get("name")),
        formula_readiness: value_string(entry.get("formulaReadiness")),
        value_resolution: value_string(entry.get("valueResolution")),
        scope_kinds: string_array(entry.get("scopeKinds")),
        stack_policy: value_string(entry.get("stackPolicy")),
        formula_zone_ids: string_array(entry.get("formulaZoneIds")),
        component_value_hints: entry
            .get("componentValueHints")
            .cloned()
            .unwrap_or(Value::Array(Vec::new())),
        runtime_proof_required: string_array(entry.get("runtimeProofRequired")),
    })
}

fn current_buff_identity(row: &Value) -> Option<CurrentBuffIdentity> {
    let known = row.get("rows")?.as_array()?.first()?.get("known_fields")?;
    Some(CurrentBuffIdentity {
        resolution: value_string(row.get("buff_table_resolution")),
        technical_name: value_string(known.get("design_name")),
        name_localization_id: known.get("name_localization_id").and_then(value_i64),
        description_localization_id: known.get("description_localization_id").and_then(value_i64),
        icon_path: value_string(known.get("icon_path")),
        visible_mode_id: known.get("visible_mode_id").and_then(value_i64),
        visible_scope: value_string(known.get("visible_scope")),
    })
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str().map(str::to_string))
        .collect()
}

fn value_string(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_string)
}

fn value_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, Box<dyn std::error::Error>> {
    Ok(serde_json::from_reader(BufReader::new(File::open(path)?))?)
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn usage() -> &'static str {
    "usage: rlogs-bpsr-rdps-candidate-inventory <origin-catalog.json> <display-bridge.json> <current-source-rows.json> <ModifierFormulaTermTable.json> <module-effect-catalog-directory> <output.json>"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn related_ids_preserve_effect_origin_and_transitive_parent() {
        let origin = ObservedOriginRelation {
            effect_id: 3003052,
            source_type_id: 1,
            source_kind: Some("buff".to_string()),
            configured_source_table: Some("BuffTable.ctb".to_string()),
            source_config_id: 3003053,
            observation_count: 2,
            observed_sessions: vec!["session".to_string()],
        };
        let bridge = serde_json::json!({
            "packet_exact_parent_ancestor_paths": [{
                "chain": [{"source_type_id": 1, "source_config_id": 3003050}]
            }]
        });

        let ids = exact_related_buff_ids(3003052, &[origin], &[&bridge]);
        let actual = ids
            .iter()
            .map(|row| (row.buff_id, row.relationship.as_str()))
            .collect::<BTreeSet<_>>();
        assert!(actual.contains(&(3003052, "observed-effect")));
        assert!(actual.contains(&(3003053, "packet-origin")));
        assert!(actual.contains(&(3003050, "packet-ancestor")));
    }

    #[test]
    fn module_effect_evidence_preserves_every_matching_level_without_selecting_one() {
        let related = vec![RelatedBuffId {
            buff_id: 2302120,
            relationship: "packet-ancestor".to_string(),
            evidence: "exact-transitive-StatusOrigin-chain".to_string(),
        }];
        let family = ModuleEffectCatalogFile {
            schema_version: 2,
            kind: "module_effect".to_string(),
            id: 2406,
            stable_key: "module-effect.2406".to_string(),
            attributes: ModuleEffectAttributes {
                name_id: 985305052,
                levels: vec![
                    ModuleEffectLevel {
                        row_id: 145,
                        level: 4,
                        name_id: 985305056,
                        overview_id: 647913846,
                        required_link_points: 12,
                        effect_config_records: vec![vec![5, 99006, 40]],
                        effect_keys: vec![vec!["attrA".to_string(), "attrB".to_string()]],
                        effect_values: vec![vec![0]],
                        fight_value: 89,
                    },
                    ModuleEffectLevel {
                        row_id: 146,
                        level: 5,
                        name_id: 985305057,
                        overview_id: 647913847,
                        required_link_points: 16,
                        effect_config_records: vec![vec![3, 2302120, 1]],
                        effect_keys: vec![vec!["attrA".to_string(), "attrB".to_string()]],
                        effect_values: vec![vec![310, 200]],
                        fight_value: 298,
                    },
                    ModuleEffectLevel {
                        row_id: 147,
                        level: 6,
                        name_id: 985305058,
                        overview_id: 647913848,
                        required_link_points: 20,
                        effect_config_records: vec![vec![3, 2302120, 1]],
                        effect_keys: vec![vec!["attrA".to_string(), "attrB".to_string()]],
                        effect_values: vec![vec![520, 340]],
                        fight_value: 448,
                    },
                ],
            },
            availability: vec![ModuleEffectAvailability {
                deployment_id: "global".to_string(),
                channel: "steam".to_string(),
                client_build: "24252055".to_string(),
            }],
            provenance: ModuleEffectProvenance {
                source: "ctb.ModEffectTable+ctb.ModEffectLibTable".to_string(),
                reference: "client-build:test".to_string(),
                confidence: "verified".to_string(),
            },
        };

        let evidence = matching_module_effect_evidence(&related, &[family]);
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].module_effect_id, 2406);
        assert_eq!(
            evidence[0]
                .matching_levels
                .iter()
                .map(|level| level.level)
                .collect::<Vec<_>>(),
            vec![5, 6]
        );
        assert_eq!(
            evidence[0].matching_levels[0].effect_values,
            vec![vec![310, 200]]
        );
    }
}
