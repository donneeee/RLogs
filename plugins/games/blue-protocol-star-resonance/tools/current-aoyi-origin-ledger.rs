use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const SCHEMA_VERSION: u16 = 18;
const FORMULA_GAP_EFFECT_IDS: [i64; 11] = [
    2_110_070, 2_110_077, 2_110_078, 2_110_102, 2_110_109, 2_110_110, 2_110_126, 2_110_132,
    2_110_138, 2_110_143, 3_210_211,
];

#[derive(Debug, Deserialize)]
struct OriginCatalog {
    game_build: String,
    #[serde(default)]
    effects: Vec<OriginEffect>,
    #[serde(default)]
    relations: Vec<OriginRelation>,
}

#[derive(Debug, Deserialize)]
struct RemodelConsumerProof {
    schema_version: u16,
    game_build: String,
    remodel_info_type: RemodelInfoTypeProof,
    assertions: RemodelConsumerAssertions,
    proof_state: String,
}

#[derive(Debug, Deserialize)]
struct RemodelInfoTypeProof {
    attribute: i64,
    buff: i64,
}

#[derive(Debug, Deserialize)]
struct RemodelConsumerAssertions {
    kind_1_is_direct_attribute_not_buff: bool,
    kind_3_is_buff_reference: bool,
    attribute_tuple_layout: Vec<String>,
    buff_tuple_layout: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct AoyiProjectileStatusProof {
    schema_version: u16,
    current_game_build: String,
    historical_packet_build: String,
    proof_state: String,
    current_static: AoyiProjectileCurrentStatic,
    historical_packet: AoyiProjectileHistoricalPacket,
    ownership_limits: AoyiProjectileOwnershipLimits,
}

#[derive(Debug, Deserialize)]
struct AoyiProjectileCurrentStatic {
    direct_owner_skill_id: i64,
    shared_owner_skill_ids: Vec<i64>,
    skill_effect_ids: Vec<i64>,
    projectile_config_id: i64,
    damage_attr_id: i64,
    recount_id: i64,
    target_status_id: i64,
    target_status_duration_seconds: f64,
    target_status_tags: Vec<i64>,
    projectile_duration_seconds: f64,
    projectile_hit_camp_types: Vec<i64>,
    damage_script: String,
}

#[derive(Debug, Deserialize)]
struct AoyiProjectileHistoricalPacket {
    session_id: String,
    source_actor_kind: String,
    source_projectile_config_id: i64,
    source_actor_ids: Vec<String>,
    target_actor_ids: Vec<String>,
    target_actor_kinds: Vec<String>,
    applied_count: u64,
    removed_count: u64,
}

#[derive(Debug, Deserialize)]
struct AoyiProjectileOwnershipLimits {
    player_provider_identity_available_in_historical_projection: bool,
    current_build_packet_lifecycle_observed: bool,
    exact_owner_selection_rule: String,
    rdps_gate: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct OriginEffect {
    effect_id: i64,
    #[serde(default)]
    status_events: u64,
    #[serde(default)]
    window_count: u64,
    #[serde(default)]
    cross_actor_window_count: u64,
    #[serde(default)]
    packet_origin_observations: u64,
    #[serde(default)]
    observed_sessions: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct OriginRelation {
    effect_id: i64,
    source_type_id: i32,
    source_config_id: i64,
    #[serde(default)]
    source_kind: Option<String>,
    #[serde(default)]
    configured_source_table: Option<String>,
    #[serde(default)]
    observation_count: u64,
    #[serde(default)]
    observed_sessions: Vec<String>,
}

#[derive(Debug, Serialize)]
struct Ledger {
    schema_version: u16,
    game_build: String,
    historical_runtime_build: String,
    policy: &'static str,
    summary: Summary,
    skills: Vec<SkillOrigin>,
    standalone_semantic_owner_candidates: Vec<SemanticOwnerCandidate>,
    legacy_formula_gap_effects: Vec<LegacyGapEffect>,
}

#[derive(Debug, Serialize)]
struct Summary {
    current_aoyi_skills: usize,
    external_offense_candidates: usize,
    external_opportunity_candidates: usize,
    external_produced_damage_candidates: usize,
    external_defense_or_healing_candidates: usize,
    self_only_offense_candidates: usize,
    skills_with_direct_attribute_transformations: usize,
    direct_attribute_transformations: usize,
    direct_attribute_percentage_lanes: usize,
    direct_attribute_additive_lanes: usize,
    skills_with_passive_owner_ids: usize,
    skills_with_owner_family_candidates: usize,
    skills_with_semantic_owner_candidates: usize,
    semantic_owner_candidates: usize,
    standalone_semantic_owner_candidates: usize,
    owner_family_candidate_buffs: usize,
    strong_owner_family_candidate_buffs: usize,
    broad_owner_prefix_candidate_buffs: usize,
    skills_with_exact_relationship_candidates: usize,
    exact_relationship_candidates: usize,
    skills_with_exact_damage_chain_candidates: usize,
    exact_damage_chain_candidates: usize,
    exact_damage_chain_ids: usize,
    missing_exact_damage_chain_ids: usize,
    exact_damage_chain_source_links: usize,
    exact_damage_attr_rows: usize,
    missing_exact_damage_attr_rows: usize,
    exact_source_target_damage_ids: usize,
    exact_source_target_damage_attr_rows: usize,
    missing_exact_source_target_damage_attr_rows: usize,
    exact_relationship_candidates_with_historical_packet_relations: usize,
    candidates_with_historical_packet_relations: usize,
    enabled_for_rdps: usize,
}

#[derive(Debug, Serialize)]
struct SkillOrigin {
    skill_id: i64,
    item_id: Option<i64>,
    monster_id: Option<i64>,
    season_id: Option<i64>,
    rarity_type: Option<i64>,
    classification: Option<i64>,
    name: String,
    monster_name: Option<String>,
    english_description: String,
    recipient_evidence: RecipientEvidence,
    candidate_classes: Vec<String>,
    passive_owner_buff_ids: Vec<i64>,
    owner_stems: Vec<String>,
    owner_family_candidates: Vec<FamilyBuffCandidate>,
    semantic_owner_candidates: Vec<SemanticOwnerCandidate>,
    component_routes: Vec<ComponentRoute>,
    exact_relationship_candidates: Vec<ExactRelationshipCandidate>,
    exact_damage_chain_candidates: Vec<ExactDamageChainCandidate>,
    direct_attribute_transformation_evidence: Vec<DirectAttributeTransformationEvidence>,
    passive_parameter_evidence: Vec<PassiveParameterEvidence>,
    active_modifier_parameter_evidence: Vec<ActiveModifierParameterEvidence>,
    base_parameters: Value,
    tier_effects: Value,
    promotion_state: &'static str,
    required_next_evidence: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct DirectAttributeTransformationEvidence {
    transformation_kind: i64,
    transformed_attribute_id: i64,
    base_attribute_id: i64,
    official_name: String,
    attribute_component: &'static str,
    attr_num_type: Option<i64>,
    base_raw_value: i64,
    tier_raw_values: Vec<DirectAttributeTierValue>,
    recipient_scope: &'static str,
    rdps_disposition: &'static str,
    value_interpretation: &'static str,
    consumer_proof: String,
    proof_state: String,
    runtime_authority: bool,
}

#[derive(Debug, Serialize)]
struct DirectAttributeTierValue {
    tier: i64,
    row_id: i64,
    raw_value: i64,
}

#[derive(Debug, Serialize)]
struct PassiveParameterEvidence {
    transformed_attribute_id: i64,
    description_template: String,
    parameter_encoding: &'static str,
    raw_units_per_percent: i64,
    raw_units_per_decimal: i64,
    lane_roles: Vec<String>,
    base_lanes: Vec<ParameterLane>,
    tier_lanes: Vec<TierParameterLanes>,
    proof_state: &'static str,
    runtime_authority: bool,
}

#[derive(Debug, Serialize)]
struct ParameterLane {
    lane: usize,
    role: String,
    raw_value: i64,
    percent_value: f64,
    decimal_value: f64,
}

#[derive(Debug, Serialize)]
struct TierParameterLanes {
    tier: i64,
    lanes: Vec<ParameterLane>,
}

#[derive(Clone, Debug, Serialize)]
struct ActiveModifierParameterEvidence {
    skill_effect_id: i64,
    active_effect_ids: Vec<i64>,
    recipient_scopes: Vec<&'static str>,
    rdps_dispositions: Vec<&'static str>,
    semantic_labels: Vec<String>,
    parameter_encoding: &'static str,
    raw_units_per_percent: i64,
    raw_units_per_decimal: i64,
    duration_seconds: Option<f64>,
    tiers: Vec<ActiveModifierTier>,
    proof_state: &'static str,
    runtime_authority: bool,
}

#[derive(Clone, Debug, Serialize)]
struct ActiveModifierTier {
    tier: i64,
    fields: Vec<ActiveModifierField>,
}

#[derive(Clone, Debug, Serialize)]
struct ActiveModifierField {
    key: String,
    semantic_role: String,
    contribution_role: &'static str,
    alias_of: Option<String>,
    raw_value: i64,
    percent_value: f64,
    decimal_value: f64,
    mapping_proof: &'static str,
}

#[derive(Debug, Serialize)]
struct ComponentRoute {
    component_id: &'static str,
    role: &'static str,
    effect_ids: Vec<i64>,
    source_config_ids: Vec<i64>,
    recipient_scope: &'static str,
    rdps_disposition: &'static str,
    proof_state: &'static str,
}

#[derive(Debug, Serialize)]
struct ExactDamageChainCandidate {
    skill_effect_id: i64,
    relationship_source: &'static str,
    damage_ids: Vec<i64>,
    resolved_damage_ids: Vec<i64>,
    missing_damage_ids: Vec<i64>,
    exact_effect_source_ids: Vec<String>,
    exact_effect_sources: Vec<Value>,
    damage_chains: Vec<Value>,
    damage_attr_rows: Vec<Value>,
    missing_damage_attr_ids: Vec<i64>,
    source_target_damage_ids: Vec<i64>,
    source_target_damage_attr_rows: Vec<Value>,
    missing_source_target_damage_attr_ids: Vec<i64>,
}

#[derive(Debug, Serialize)]
struct ExactRelationshipCandidate {
    rule_id: String,
    source_id: Option<String>,
    relationship_source: &'static str,
    relationship_kinds: Vec<String>,
    owner_skill_effect_ids: Vec<i64>,
    source_config_ids: Vec<i64>,
    runtime_buff_ids: Vec<i64>,
    source_buff_ids: Vec<i64>,
    target_damage_ids: Vec<i64>,
    modifier_source_ids: Vec<String>,
    formula_statuses: Vec<String>,
    historical_effects: Vec<OriginEffect>,
    historical_relations: Vec<OriginRelation>,
    uid_edges: Vec<Value>,
}

#[derive(Debug, Serialize)]
struct RecipientEvidence {
    state: &'static str,
    matched_phrases: Vec<String>,
    source: &'static str,
}

#[derive(Debug, Serialize)]
struct FamilyBuffCandidate {
    buff_id: i64,
    design_name: String,
    name: Option<String>,
    description: Option<String>,
    owner_stem: String,
    relationship: &'static str,
    owner_match_strength: &'static str,
    modifier_source_ids: Vec<String>,
    formula_statuses: Vec<String>,
    historical_localized_identity_unchanged: Option<bool>,
    historical_effect: Option<OriginEffect>,
    historical_relations: Vec<OriginRelation>,
}

#[derive(Debug, Serialize)]
struct SemanticOwnerCandidate {
    effect_id: i64,
    owner_skill_id: i64,
    relationship_source: &'static str,
    skill_effect_id: i64,
    source_subskill_id: Option<i64>,
    source_subskill_effect_id: Option<i64>,
    item_id: Option<i64>,
    monster_id: Option<i64>,
    runtime_monster_id: Option<i64>,
    transformed_attribute_id: Option<i64>,
    matching_terms: Vec<&'static str>,
    matching_duration_seconds: u64,
    stack_cap: Option<u64>,
    recipient_scope: &'static str,
    rdps_disposition: &'static str,
    proof_state: &'static str,
    runtime_authority: bool,
}

#[derive(Debug, Serialize)]
struct LegacyGapEffect {
    effect_id: i64,
    name: Option<String>,
    design_name: Option<String>,
    current_exact_component_skill_ids: Vec<i64>,
    current_exact_relationship_skill_ids: Vec<i64>,
    current_strong_owner_skill_ids: Vec<i64>,
    current_broad_owner_skill_ids: Vec<i64>,
    current_semantic_owner_skill_ids: Vec<i64>,
    current_semantic_recipient_scopes: Vec<String>,
    current_semantic_rdps_dispositions: Vec<String>,
    current_exact_component_recipient_scopes: Vec<String>,
    current_exact_component_rdps_dispositions: Vec<String>,
    current_active_modifier_parameter_evidence: Vec<ActiveModifierParameterEvidence>,
    current_owner_skill_ids: Vec<i64>,
    current_owner_family_match: bool,
    owner_evidence_state: &'static str,
    historical_effect: Option<OriginEffect>,
    historical_relations: Vec<OriginRelation>,
    disposition: &'static str,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("BPSR current Aoyi origin ledger failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = env::args_os().skip(1);
    let decoded_root = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    let skill_icons = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    let modifier_source_index = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    let modifier_relationship_table = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    let skill_damage_chain_bridge = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    let effect_sources = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    let origin_catalog = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    let historical_buff_names = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    let remodel_consumer_proof = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    let projectile_status_proof = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    let output = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    let game_build = arguments
        .next()
        .ok_or_else(usage)?
        .into_string()
        .map_err(|_| "game build must be UTF-8")?;
    if arguments.next().is_some() {
        return Err(usage().into());
    }

    let aoyi_table = read_json(decoded_root.join("SkillAoyiTable.json"))?;
    let skill_table = read_json(decoded_root.join("SkillTable.json"))?;
    let buff_table = read_json(decoded_root.join("BuffTable.json"))?;
    let monster_table = read_json(decoded_root.join("MonsterTable.json"))?;
    let icons = read_json(skill_icons)?;
    let modifier_sources = read_json(modifier_source_index)?;
    let modifier_relationships = read_json(modifier_relationship_table)?;
    let damage_chains = read_json(skill_damage_chain_bridge)?;
    let effect_sources = read_json(effect_sources)?;
    let skill_effect_table = read_json(decoded_root.join("SkillEffectTable.json"))?;
    let damage_attr_table = read_json(decoded_root.join("DamageAttrTable.json"))?;
    let attr_description_table = read_json(decoded_root.join("AttrDescription.json"))?;
    let fight_attr_table = read_json(decoded_root.join("FightAttrTable.json"))?;
    let skill_aoyi_star_table = read_json(decoded_root.join("SkillAoyiStarTable.json"))?;
    let historical_names = read_json(historical_buff_names)?;
    let origins: OriginCatalog =
        serde_json::from_reader(BufReader::new(File::open(origin_catalog)?))?;
    let remodel_consumer_proof: RemodelConsumerProof =
        serde_json::from_reader(BufReader::new(File::open(remodel_consumer_proof)?))?;
    validate_remodel_consumer_proof(&remodel_consumer_proof, &game_build)?;
    let projectile_status_proof: AoyiProjectileStatusProof =
        serde_json::from_reader(BufReader::new(File::open(projectile_status_proof)?))?;
    validate_projectile_status_proof(&projectile_status_proof, &game_build)?;

    let ledger = build_ledger(
        game_build,
        origins,
        &aoyi_table,
        &skill_table,
        &buff_table,
        &monster_table,
        &icons,
        &modifier_sources,
        &modifier_relationships,
        &skill_effect_table,
        &damage_attr_table,
        &attr_description_table,
        &fight_attr_table,
        &skill_aoyi_star_table,
        &damage_chains,
        &effect_sources,
        &historical_names,
        &remodel_consumer_proof,
        &projectile_status_proof,
    )?;
    let mut writer = BufWriter::new(File::create(output)?);
    serde_json::to_writer_pretty(&mut writer, &ledger)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn build_ledger(
    game_build: String,
    origins: OriginCatalog,
    aoyi_table: &Value,
    skill_table: &Value,
    buff_table: &Value,
    monster_table: &Value,
    icons: &Value,
    modifier_sources: &Value,
    modifier_relationships: &Value,
    skill_effect_table: &Value,
    damage_attr_table: &Value,
    attr_description_table: &Value,
    fight_attr_table: &Value,
    skill_aoyi_star_table: &Value,
    damage_chains: &Value,
    effect_sources: &Value,
    historical_names: &Value,
    remodel_consumer_proof: &RemodelConsumerProof,
    projectile_status_proof: &AoyiProjectileStatusProof,
) -> Result<Ledger, Box<dyn std::error::Error>> {
    let aoyi_rows = table_rows(aoyi_table)?;
    let skill_rows = table_rows(skill_table)?;
    let buff_rows = table_rows(buff_table)?;
    let monster_rows = table_rows(monster_table)?;
    let icon_rows = table_rows(icons)?;
    let historical_name_rows = table_rows(historical_names)?;
    let skill_effect_rows = table_rows(skill_effect_table)?;
    let damage_attr_rows = table_rows(damage_attr_table)?;
    let attr_description_rows = table_rows(attr_description_table)?;
    let fight_attr_rows = table_rows(fight_attr_table)?;
    let skill_aoyi_star_rows = table_rows(skill_aoyi_star_table)?;

    let buffs_by_id = rows_by_id(&buff_rows);
    let skills_by_id = rows_by_id(&skill_rows);
    let monsters_by_id = rows_by_id(&monster_rows);
    let icons_by_id = rows_by_id(&icon_rows);
    let historical_names_by_id = rows_by_id(&historical_name_rows);
    let skill_effects_by_id = rows_by_id(&skill_effect_rows);
    let damage_attrs_by_id = rows_by_id(&damage_attr_rows);
    let attr_descriptions_by_id = rows_by_id(&attr_description_rows);
    let origin_effects = origins
        .effects
        .iter()
        .map(|effect| (effect.effect_id, effect.clone()))
        .collect::<BTreeMap<_, _>>();

    let mut skills = Vec::new();
    for row in aoyi_rows {
        let skill_id = integer(row, "Id").ok_or("SkillAoyiTable row is missing Id")?;
        let icon = icons_by_id.get(&skill_id).copied();
        let description = icon
            .and_then(|value| nested_string(value, &["CleanDescriptions", "en"]))
            .or_else(|| icon.and_then(|value| nested_string(value, &["Descriptions", "en"])))
            .unwrap_or_default();
        let name = icon
            .and_then(|value| nested_string(value, &["Names", "en"]))
            .or_else(|| string(row, "ResonanceObject"))
            .unwrap_or_else(|| format!("Aoyi skill {skill_id}"));
        let monster_name = icon.and_then(|value| nested_string(value, &["MonsterNames", "en"]));
        let recipient_evidence = classify_recipients(&description);
        let candidate_classes = classify_candidate_classes(&description);

        let passive_owner_buff_ids = passive_owner_ids(row, &buffs_by_id);
        let owner_stems = passive_owner_buff_ids
            .iter()
            .filter_map(|id| buffs_by_id.get(id))
            .filter_map(|buff| string(buff, "NameDesign"))
            .filter_map(|name| owner_stem(&name))
            .collect::<BTreeSet<_>>();

        let mut owner_family_candidates = Vec::new();
        for stem in &owner_stems {
            for buff in &buff_rows {
                let Some(buff_id) = integer(buff, "Id") else {
                    continue;
                };
                if passive_owner_buff_ids.contains(&buff_id) {
                    continue;
                }
                let Some(design_name) = string(buff, "NameDesign") else {
                    continue;
                };
                if normalized_owner_name(&design_name).starts_with(stem) {
                    let strong_owner_match = strong_owner_family_match(stem, &design_name);
                    let (source_ids, formula_statuses) =
                        modifier_source_evidence(modifier_sources, buff_id);
                    let historical_relations = related_origins(&origins.relations, buff_id);
                    let historical_localized_identity_unchanged = historical_names_by_id
                        .get(&buff_id)
                        .map(|old| stable_localized_identity_unchanged(buff, old));
                    owner_family_candidates.push(FamilyBuffCandidate {
                        buff_id,
                        design_name,
                        name: string(buff, "Name"),
                        description: string(buff, "Desc"),
                        owner_stem: stem.clone(),
                        relationship: if strong_owner_match {
                            "current-aoyi-passive-owner-family"
                        } else {
                            "current-aoyi-passive-owner-prefix-broad"
                        },
                        owner_match_strength: if strong_owner_match {
                            "strong"
                        } else {
                            "broad"
                        },
                        modifier_source_ids: source_ids,
                        formula_statuses,
                        historical_localized_identity_unchanged,
                        historical_effect: origin_effects.get(&buff_id).cloned(),
                        historical_relations,
                    });
                }
            }
        }
        owner_family_candidates.sort_by_key(|candidate| candidate.buff_id);
        owner_family_candidates.dedup_by_key(|candidate| candidate.buff_id);
        let exact_relationship_candidates = exact_relationship_candidates(
            modifier_relationships,
            modifier_sources,
            &origins,
            &origin_effects,
            skill_id,
        );
        let exact_damage_chain_candidates = exact_damage_chain_candidates(
            &skill_effects_by_id,
            &damage_attrs_by_id,
            damage_chains,
            effect_sources,
            skill_id,
        );
        let semantic_owner_candidates = semantic_owner_candidates(
            row,
            icon,
            skill_id,
            &skills_by_id,
            &skill_effects_by_id,
            &buffs_by_id,
            &monsters_by_id,
        )?;
        let component_routes = exact_component_routes(
            row,
            skill_id,
            &buffs_by_id,
            &damage_attrs_by_id,
            &passive_owner_buff_ids,
            &owner_family_candidates,
            &exact_relationship_candidates,
            &exact_damage_chain_candidates,
            projectile_status_proof,
        )?;
        let passive_parameter_evidence =
            passive_parameter_evidence(row, icon, &attr_descriptions_by_id)?;
        let direct_attribute_transformation_evidence = direct_attribute_transformation_evidence(
            row,
            skill_id,
            &fight_attr_rows,
            &skill_aoyi_star_rows,
            remodel_consumer_proof,
        )?;
        let active_modifier_parameter_evidence = active_modifier_parameter_evidence(
            skill_id,
            icon,
            &skills_by_id,
            &skill_effects_by_id,
            &buffs_by_id,
            &component_routes,
            &semantic_owner_candidates,
        )?;

        skills.push(SkillOrigin {
            skill_id,
            item_id: integer(row, "AoyiItemId").filter(|id| *id != 0),
            monster_id: integer(row, "MonsterId").filter(|id| *id != 0),
            season_id: integer(row, "SeasonId"),
            rarity_type: integer(row, "RarityType"),
            classification: integer(row, "Classification"),
            name,
            monster_name,
            english_description: description,
            recipient_evidence,
            candidate_classes,
            passive_owner_buff_ids,
            owner_stems: owner_stems.into_iter().collect(),
            owner_family_candidates,
            semantic_owner_candidates,
            component_routes,
            exact_relationship_candidates,
            exact_damage_chain_candidates,
            direct_attribute_transformation_evidence,
            passive_parameter_evidence,
            active_modifier_parameter_evidence,
            base_parameters: row
                .get("BuffPar")
                .cloned()
                .unwrap_or(Value::Array(Vec::new())),
            tier_effects: icon
                .and_then(|value| value.get("TierEffects"))
                .cloned()
                .unwrap_or(Value::Array(Vec::new())),
            promotion_state: "blocked-pending-current-packet-formula-and-conservation-proof",
            required_next_evidence: vec![
                "current-build packet status origin",
                "provider and recipient status windows",
                "exact modifier magnitude and fixed-point lane",
                "counterfactual replay conservation",
            ],
        });
    }
    skills.sort_by_key(|skill| skill.skill_id);
    let standalone_semantic_owner_candidates = standalone_semantic_owner_candidates(
        &skills_by_id,
        &skill_effects_by_id,
        &buffs_by_id,
        &monsters_by_id,
    )?;

    let legacy_formula_gap_effects = FORMULA_GAP_EFFECT_IDS
        .into_iter()
        .map(|effect_id| {
            let component_owners = skills
                .iter()
                .filter(|skill| {
                    skill
                        .component_routes
                        .iter()
                        .any(|component| component.effect_ids.contains(&effect_id))
                })
                .map(|skill| skill.skill_id)
                .collect::<Vec<_>>();
            let exact_owners = skills
                .iter()
                .filter(|skill| {
                    skill.exact_relationship_candidates.iter().any(|candidate| {
                        candidate.runtime_buff_ids.contains(&effect_id)
                            || candidate.source_buff_ids.contains(&effect_id)
                    })
                })
                .map(|skill| skill.skill_id)
                .collect::<Vec<_>>();
            let strong_owners = skills
                .iter()
                .filter(|skill| {
                    skill.owner_family_candidates.iter().any(|candidate| {
                        candidate.buff_id == effect_id && candidate.owner_match_strength == "strong"
                    })
                })
                .map(|skill| skill.skill_id)
                .collect::<Vec<_>>();
            let broad_owners = skills
                .iter()
                .filter(|skill| {
                    skill.owner_family_candidates.iter().any(|candidate| {
                        candidate.buff_id == effect_id && candidate.owner_match_strength == "broad"
                    })
                })
                .map(|skill| skill.skill_id)
                .collect::<Vec<_>>();
            let semantic_owners = skills
                .iter()
                .flat_map(|skill| &skill.semantic_owner_candidates)
                .chain(&standalone_semantic_owner_candidates)
                .filter(|candidate| candidate.effect_id == effect_id)
                .map(|candidate| candidate.owner_skill_id)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let semantic_recipient_scopes = skills
                .iter()
                .flat_map(|skill| &skill.semantic_owner_candidates)
                .chain(&standalone_semantic_owner_candidates)
                .filter(|candidate| candidate.effect_id == effect_id)
                .map(|candidate| candidate.recipient_scope.to_owned())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let semantic_rdps_dispositions = skills
                .iter()
                .flat_map(|skill| &skill.semantic_owner_candidates)
                .chain(&standalone_semantic_owner_candidates)
                .filter(|candidate| candidate.effect_id == effect_id)
                .map(|candidate| candidate.rdps_disposition.to_owned())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let component_recipient_scopes = skills
                .iter()
                .flat_map(|skill| &skill.component_routes)
                .filter(|component| component.effect_ids.contains(&effect_id))
                .map(|component| component.recipient_scope.to_owned())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let component_rdps_dispositions = skills
                .iter()
                .flat_map(|skill| &skill.component_routes)
                .filter(|component| component.effect_ids.contains(&effect_id))
                .map(|component| component.rdps_disposition.to_owned())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let active_modifier_parameter_evidence = skills
                .iter()
                .flat_map(|skill| &skill.active_modifier_parameter_evidence)
                .filter(|evidence| evidence.active_effect_ids.contains(&effect_id))
                .cloned()
                .collect::<Vec<_>>();
            let owner_evidence_state = formula_gap_owner_evidence_state(
                &component_owners,
                &exact_owners,
                &strong_owners,
                &broad_owners,
                &semantic_owners,
            );
            let buff = buffs_by_id.get(&effect_id).copied();
            LegacyGapEffect {
                effect_id,
                name: buff.and_then(|value| string(value, "Name")),
                design_name: buff.and_then(|value| string(value, "NameDesign")),
                current_exact_component_skill_ids: component_owners.clone(),
                current_exact_relationship_skill_ids: exact_owners.clone(),
                current_strong_owner_skill_ids: strong_owners.clone(),
                current_broad_owner_skill_ids: broad_owners.clone(),
                current_semantic_owner_skill_ids: semantic_owners.clone(),
                current_semantic_recipient_scopes: semantic_recipient_scopes,
                current_semantic_rdps_dispositions: semantic_rdps_dispositions,
                current_exact_component_recipient_scopes: component_recipient_scopes,
                current_exact_component_rdps_dispositions: component_rdps_dispositions,
                current_active_modifier_parameter_evidence: active_modifier_parameter_evidence,
                current_owner_family_match: !strong_owners.is_empty() || !broad_owners.is_empty(),
                current_owner_skill_ids: component_owners
                    .into_iter()
                    .chain(exact_owners)
                    .chain(strong_owners)
                    .chain(broad_owners)
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect(),
                owner_evidence_state,
                historical_effect: origin_effects.get(&effect_id).cloned(),
                historical_relations: related_origins(&origins.relations, effect_id),
                disposition: "preserved-until-current-origin-and-formula-proof",
            }
        })
        .collect::<Vec<_>>();

    let summary = Summary {
        current_aoyi_skills: skills.len(),
        external_offense_candidates: count_class(&skills, "external-offense-stat")
            + count_class(&skills, "external-target-mitigation"),
        external_opportunity_candidates: count_class(&skills, "external-action-opportunity"),
        external_produced_damage_candidates: count_class(&skills, "external-produced-damage"),
        external_defense_or_healing_candidates: count_class(&skills, "external-defense")
            + count_class(&skills, "external-healing"),
        self_only_offense_candidates: count_class(&skills, "self-only-offense"),
        skills_with_direct_attribute_transformations: skills
            .iter()
            .filter(|skill| !skill.direct_attribute_transformation_evidence.is_empty())
            .count(),
        direct_attribute_transformations: skills
            .iter()
            .map(|skill| skill.direct_attribute_transformation_evidence.len())
            .sum(),
        direct_attribute_percentage_lanes: skills
            .iter()
            .flat_map(|skill| &skill.direct_attribute_transformation_evidence)
            .filter(|evidence| {
                matches!(
                    evidence.attribute_component,
                    "percentage" | "extra-percentage"
                )
            })
            .count(),
        direct_attribute_additive_lanes: skills
            .iter()
            .flat_map(|skill| &skill.direct_attribute_transformation_evidence)
            .filter(|evidence| {
                matches!(evidence.attribute_component, "additive" | "extra-additive")
            })
            .count(),
        skills_with_passive_owner_ids: skills
            .iter()
            .filter(|skill| !skill.passive_owner_buff_ids.is_empty())
            .count(),
        skills_with_owner_family_candidates: skills
            .iter()
            .filter(|skill| !skill.owner_family_candidates.is_empty())
            .count(),
        skills_with_semantic_owner_candidates: skills
            .iter()
            .filter(|skill| !skill.semantic_owner_candidates.is_empty())
            .count(),
        semantic_owner_candidates: skills
            .iter()
            .map(|skill| skill.semantic_owner_candidates.len())
            .sum::<usize>()
            + standalone_semantic_owner_candidates.len(),
        standalone_semantic_owner_candidates: standalone_semantic_owner_candidates.len(),
        owner_family_candidate_buffs: skills
            .iter()
            .map(|skill| skill.owner_family_candidates.len())
            .sum(),
        strong_owner_family_candidate_buffs: skills
            .iter()
            .flat_map(|skill| &skill.owner_family_candidates)
            .filter(|candidate| candidate.owner_match_strength == "strong")
            .count(),
        broad_owner_prefix_candidate_buffs: skills
            .iter()
            .flat_map(|skill| &skill.owner_family_candidates)
            .filter(|candidate| candidate.owner_match_strength == "broad")
            .count(),
        skills_with_exact_relationship_candidates: skills
            .iter()
            .filter(|skill| !skill.exact_relationship_candidates.is_empty())
            .count(),
        exact_relationship_candidates: skills
            .iter()
            .map(|skill| skill.exact_relationship_candidates.len())
            .sum(),
        skills_with_exact_damage_chain_candidates: skills
            .iter()
            .filter(|skill| !skill.exact_damage_chain_candidates.is_empty())
            .count(),
        exact_damage_chain_candidates: skills
            .iter()
            .map(|skill| skill.exact_damage_chain_candidates.len())
            .sum(),
        exact_damage_chain_ids: skills
            .iter()
            .flat_map(|skill| &skill.exact_damage_chain_candidates)
            .map(|candidate| candidate.resolved_damage_ids.len())
            .sum(),
        missing_exact_damage_chain_ids: skills
            .iter()
            .flat_map(|skill| &skill.exact_damage_chain_candidates)
            .map(|candidate| candidate.missing_damage_ids.len())
            .sum(),
        exact_damage_chain_source_links: skills
            .iter()
            .flat_map(|skill| &skill.exact_damage_chain_candidates)
            .map(|candidate| candidate.exact_effect_source_ids.len())
            .sum(),
        exact_damage_attr_rows: skills
            .iter()
            .flat_map(|skill| &skill.exact_damage_chain_candidates)
            .flat_map(|candidate| &candidate.damage_attr_rows)
            .filter_map(|row| integer(row, "Id"))
            .collect::<BTreeSet<_>>()
            .len(),
        missing_exact_damage_attr_rows: skills
            .iter()
            .flat_map(|skill| &skill.exact_damage_chain_candidates)
            .flat_map(|candidate| &candidate.missing_damage_attr_ids)
            .copied()
            .collect::<BTreeSet<_>>()
            .len(),
        exact_source_target_damage_ids: skills
            .iter()
            .flat_map(|skill| &skill.exact_damage_chain_candidates)
            .flat_map(|candidate| &candidate.source_target_damage_ids)
            .copied()
            .collect::<BTreeSet<_>>()
            .len(),
        exact_source_target_damage_attr_rows: skills
            .iter()
            .flat_map(|skill| &skill.exact_damage_chain_candidates)
            .flat_map(|candidate| &candidate.source_target_damage_attr_rows)
            .filter_map(|row| integer(row, "Id"))
            .collect::<BTreeSet<_>>()
            .len(),
        missing_exact_source_target_damage_attr_rows: skills
            .iter()
            .flat_map(|skill| &skill.exact_damage_chain_candidates)
            .flat_map(|candidate| &candidate.missing_source_target_damage_attr_ids)
            .copied()
            .collect::<BTreeSet<_>>()
            .len(),
        exact_relationship_candidates_with_historical_packet_relations: skills
            .iter()
            .flat_map(|skill| &skill.exact_relationship_candidates)
            .filter(|candidate| !candidate.historical_relations.is_empty())
            .count(),
        candidates_with_historical_packet_relations: skills
            .iter()
            .flat_map(|skill| &skill.owner_family_candidates)
            .filter(|candidate| !candidate.historical_relations.is_empty())
            .count(),
        enabled_for_rdps: 0,
    };

    Ok(Ledger {
        schema_version: SCHEMA_VERSION,
        game_build,
        historical_runtime_build: origins.game_build,
        policy: "Descriptions identify proof candidates only. Runtime attribution remains disabled until current-build packet origin, exact magnitude/formula, and conservation replay all agree. Self-only effects are never transferred to another provider.",
        summary,
        skills,
        standalone_semantic_owner_candidates,
        legacy_formula_gap_effects,
    })
}

fn formula_gap_owner_evidence_state(
    component_owners: &[i64],
    exact_relationship_owners: &[i64],
    strong_family_owners: &[i64],
    broad_family_owners: &[i64],
    semantic_owners: &[i64],
) -> &'static str {
    if !component_owners.is_empty() {
        "exact-component-route-current-runtime-reproof-required"
    } else if !exact_relationship_owners.is_empty() {
        "exact-generated-relationship"
    } else if !strong_family_owners.is_empty() {
        "strong-design-family-candidate-not-formula-authority"
    } else if !broad_family_owners.is_empty() {
        "broad-design-prefix-candidate-not-formula-authority"
    } else if semantic_owners.len() == 1 {
        "unique-semantic-duration-candidate-not-numeric-owner-edge"
    } else if semantic_owners.len() > 1 {
        "ambiguous-multiple-semantic-candidates-not-numeric-owner-edge"
    } else {
        "no-current-owner-candidate"
    }
}

fn semantic_owner_candidates(
    row: &Value,
    icon: Option<&Value>,
    skill_id: i64,
    skills_by_id: &BTreeMap<i64, &Value>,
    skill_effects_by_id: &BTreeMap<i64, &Value>,
    buffs_by_id: &BTreeMap<i64, &Value>,
    monsters_by_id: &BTreeMap<i64, &Value>,
) -> Result<Vec<SemanticOwnerCandidate>, Box<dyn std::error::Error>> {
    match skill_id {
        3_915 => lucky_goblin_semantic_candidate(
            row,
            skills_by_id,
            skill_effects_by_id,
            buffs_by_id,
            monsters_by_id,
        ),
        3_926 => meteor_shower_luck_semantic_candidate(
            row,
            skills_by_id,
            skill_effects_by_id,
            buffs_by_id,
        ),
        3_928 => predator_slash_semantic_candidate(
            row,
            skills_by_id,
            skill_effects_by_id,
            buffs_by_id,
            monsters_by_id,
        ),
        3_934 => blink_ambush_semantic_candidate(
            row,
            skills_by_id,
            skill_effects_by_id,
            buffs_by_id,
            monsters_by_id,
        ),
        3_937 => murloc_luck_semantic_candidate(
            row,
            skills_by_id,
            skill_effects_by_id,
            buffs_by_id,
            monsters_by_id,
        ),
        3_958 => {
            frost_breath_semantic_candidate(row, icon, skill_id, skill_effects_by_id, buffs_by_id)
        }
        3_964 => lucky_crit_semantic_candidate(
            row,
            skills_by_id,
            skill_effects_by_id,
            buffs_by_id,
            monsters_by_id,
        ),
        _ => Ok(Vec::new()),
    }
}

fn lucky_goblin_semantic_candidate(
    row: &Value,
    skills_by_id: &BTreeMap<i64, &Value>,
    skill_effects_by_id: &BTreeMap<i64, &Value>,
    buffs_by_id: &BTreeMap<i64, &Value>,
    monsters_by_id: &BTreeMap<i64, &Value>,
) -> Result<Vec<SemanticOwnerCandidate>, Box<dyn std::error::Error>> {
    require_aoyi_identity(
        row,
        3_000_013,
        10_019,
        &json!([[3, 3_200_009, 1]]),
        &json!([[1_120]]),
        "Stunt! Frenzied Shot",
    )?;
    require_skill_description(
        skills_by_id,
        3_915,
        391_501,
        &["your", "Luck", "Lucky Strike Chance"],
    )?;
    require_skill_effect_terms(
        skill_effects_by_id,
        391_501,
        3_915,
        &["Luck Bonus", "Lucky Strike Multiplier", "20s"],
    )?;
    require_skill_effect(skill_effects_by_id, 100_730_01, 100_730, None)?;
    require_runtime_monster(monsters_by_id, 3_000_011, 100_730)?;
    require_buff(
        buffs_by_id,
        2_110_109,
        "Lucky Goblin",
        "Increases your Luck",
        20,
        &[0, 1],
        14,
    )?;

    Ok(vec![self_only_semantic_candidate(
        2_110_109,
        3_915,
        391_501,
        Some(100_730),
        Some(100_730_01),
        3_000_013,
        10_019,
        Some(3_000_011),
        3_200_009,
        vec!["Luck", "Lucky Strike multiplier", "20-second duration"],
    )])
}

fn meteor_shower_luck_semantic_candidate(
    row: &Value,
    skills_by_id: &BTreeMap<i64, &Value>,
    skill_effects_by_id: &BTreeMap<i64, &Value>,
    buffs_by_id: &BTreeMap<i64, &Value>,
) -> Result<Vec<SemanticOwnerCandidate>, Box<dyn std::error::Error>> {
    require_aoyi_identity(
        row,
        3_000_024,
        10_007,
        &json!([[3, 3_210_020, 1]]),
        &json!([[2_240]]),
        "Arcane! Meteor Shower",
    )?;
    require_skill_description(
        skills_by_id,
        3_926,
        392_601,
        &["your", "Luck", "Luck effects"],
    )?;
    require_skill_effect_terms(
        skill_effects_by_id,
        392_601,
        3_926,
        &["Luck Bonus", "Luck Effect bonus", "20s"],
    )?;
    require_skill_effect(skill_effects_by_id, 369_901, 3_699, None)?;
    require_buff(
        buffs_by_id,
        2_110_102,
        "Lucky enhancement",
        "increases own luck",
        20,
        &[0, 1],
        14,
    )?;

    Ok(vec![self_only_semantic_candidate(
        2_110_102,
        3_926,
        392_601,
        Some(3_699),
        Some(369_901),
        3_000_024,
        10_007,
        None,
        3_210_020,
        vec!["Luck", "Luck-effect output", "20-second duration"],
    )])
}

fn murloc_luck_semantic_candidate(
    row: &Value,
    skills_by_id: &BTreeMap<i64, &Value>,
    skill_effects_by_id: &BTreeMap<i64, &Value>,
    buffs_by_id: &BTreeMap<i64, &Value>,
    monsters_by_id: &BTreeMap<i64, &Value>,
) -> Result<Vec<SemanticOwnerCandidate>, Box<dyn std::error::Error>> {
    require_aoyi_identity(
        row,
        3_000_016,
        10_104,
        &json!([[3, 3_210_080, 1]]),
        &json!([[1_120]]),
        "Stunt! Thunder Suppress",
    )?;
    require_skill_description(
        skills_by_id,
        3_937,
        393_701,
        &["your", "Luck", "Lucky Strike Chance"],
    )?;
    require_skill_effect_terms(
        skill_effects_by_id,
        393_701,
        3_937,
        &["Luck Bonus", "Lucky Strike Multiplier", "20s"],
    )?;
    require_skill_effect(skill_effects_by_id, 101_044_001, 1_010_440, None)?;
    require_runtime_monster(monsters_by_id, 3_000_029, 1_010_440)?;
    require_buff(
        buffs_by_id,
        2_110_110,
        "Murloc's Luck",
        "Lucky Strike DMG",
        20,
        &[0, 1],
        14,
    )?;

    Ok(vec![self_only_semantic_candidate(
        2_110_110,
        3_937,
        393_701,
        Some(1_010_440),
        Some(101_044_001),
        3_000_016,
        10_104,
        Some(3_000_029),
        3_210_080,
        vec!["Luck", "Lucky Strike multiplier", "20-second duration"],
    )])
}

fn lucky_crit_semantic_candidate(
    row: &Value,
    skills_by_id: &BTreeMap<i64, &Value>,
    skill_effects_by_id: &BTreeMap<i64, &Value>,
    buffs_by_id: &BTreeMap<i64, &Value>,
    monsters_by_id: &BTreeMap<i64, &Value>,
) -> Result<Vec<SemanticOwnerCandidate>, Box<dyn std::error::Error>> {
    require_aoyi_identity(
        row,
        3_000_113,
        3_000_060,
        &json!([[3, 3_200_035, 1]]),
        &json!([[500, 700]]),
        "Stunt! Cabbage Boomerang",
    )?;
    require_skill_description(skills_by_id, 3_964, 396_401, &["your", "Crit", "Crit DMG"])?;
    require_skill_effect_terms(
        skill_effects_by_id,
        396_401,
        3_964,
        &["Tier 1 Crit Boost", "Tier 1 Crit DMG Boost", "20s"],
    )?;
    require_skill_effect(skill_effects_by_id, 101_131_201, 1_011_312, None)?;
    require_runtime_monster(monsters_by_id, 3_000_060, 1_011_312)?;
    require_buff(
        buffs_by_id,
        2_110_132,
        "Lucky Crit",
        "Crit Rate and Crit DMG",
        20,
        &[0, 1],
        14,
    )?;

    Ok(vec![self_only_semantic_candidate(
        2_110_132,
        3_964,
        396_401,
        Some(1_011_312),
        Some(101_131_201),
        3_000_113,
        3_000_060,
        Some(3_000_060),
        3_200_035,
        vec!["Crit rate", "Crit damage", "20-second duration"],
    )])
}

#[allow(clippy::too_many_arguments)]
fn self_only_semantic_candidate(
    effect_id: i64,
    owner_skill_id: i64,
    skill_effect_id: i64,
    source_subskill_id: Option<i64>,
    source_subskill_effect_id: Option<i64>,
    item_id: i64,
    monster_id: i64,
    runtime_monster_id: Option<i64>,
    transformed_attribute_id: i64,
    matching_terms: Vec<&'static str>,
) -> SemanticOwnerCandidate {
    SemanticOwnerCandidate {
        effect_id,
        owner_skill_id,
        relationship_source: "current-build-aoyi-item-summon-subskill-self-wording-duration-and-fixed-point-parameters",
        skill_effect_id,
        source_subskill_id,
        source_subskill_effect_id,
        item_id: Some(item_id),
        monster_id: Some(monster_id),
        runtime_monster_id,
        transformed_attribute_id: Some(transformed_attribute_id),
        matching_terms,
        matching_duration_seconds: 20,
        stack_cap: Some(1),
        recipient_scope: "summon-caster-only-per-current-skill-and-buff-descriptions",
        rdps_disposition: "ordinary-owner-damage-never-transferred",
        proof_state: "current-build-unique-source-chain-and-self-semantics-not-runtime-authority",
        runtime_authority: false,
    }
}

fn require_aoyi_identity(
    row: &Value,
    item_id: i64,
    monster_id: i64,
    transforms: &Value,
    parameters: &Value,
    label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if integer(row, "AoyiItemId") != Some(item_id)
        || integer(row, "MonsterId") != Some(monster_id)
        || row.get("TransformationType") != Some(transforms)
        || row.get("BuffPar") != Some(parameters)
    {
        return Err(
            format!("{label} current-build identity or fixed-point parameters changed").into(),
        );
    }
    Ok(())
}

fn require_skill_effect_terms(
    skill_effects_by_id: &BTreeMap<i64, &Value>,
    skill_effect_id: i64,
    owner_skill_id: i64,
    expected_terms: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    require_skill_effect(skill_effects_by_id, skill_effect_id, owner_skill_id, None)?;
    let effect = skill_effects_by_id[&skill_effect_id];
    let searchable = effect
        .get("SkillAttrDes")
        .map(Value::to_string)
        .unwrap_or_default();
    if !expected_terms.iter().all(|term| searchable.contains(term)) {
        return Err(format!("skill-effect {skill_effect_id} semantic terms changed").into());
    }
    Ok(())
}

fn frost_breath_semantic_candidate(
    row: &Value,
    icon: Option<&Value>,
    skill_id: i64,
    skill_effects_by_id: &BTreeMap<i64, &Value>,
    buffs_by_id: &BTreeMap<i64, &Value>,
) -> Result<Vec<SemanticOwnerCandidate>, Box<dyn std::error::Error>> {
    let expected_transforms = json!([[1, 11152, 5040]]);
    if integer(row, "AoyiItemId") != Some(3_000_105)
        || integer(row, "MonsterId") != Some(3_000_054)
        || row.get("TransformationType") != Some(&expected_transforms)
    {
        return Err("Stunt! Frost Breath current-build identity changed".into());
    }

    let skill_effect = skill_effects_by_id
        .get(&395_801)
        .copied()
        .ok_or("Stunt! Frost Breath skill-effect row 395801 is missing")?;
    if integer(skill_effect, "SkillId") != Some(skill_id)
        || !skill_attr_description_matches(skill_effect, "Versatility Boost", "")
        || !skill_attr_description_matches(skill_effect, "Duration", "20s")
    {
        return Err("Stunt! Frost Breath skill-effect semantics or duration changed".into());
    }

    let buff = buffs_by_id
        .get(&2_110_126)
        .copied()
        .ok_or("Versatility Specialization buff 2110126 is missing")?;
    if string_ref(buff, "Name") != Some("Versatility Specialization")
        || !string_ref(buff, "Desc").is_some_and(|description| description.contains("Versatility"))
        || buff.get("DestroyParam") != Some(&json!([[0.0, 20.0]]))
    {
        return Err("Versatility Specialization identity or duration changed".into());
    }

    let icon = icon.ok_or("Stunt! Frost Breath localized icon record is missing")?;
    let description = nested_string(icon, &["CleanDescriptions", "en"])
        .ok_or("Stunt! Frost Breath English description is missing")?;
    if !description.contains("increases Versatility for a period of time") {
        return Err("Stunt! Frost Breath localized Versatility semantics changed".into());
    }
    require_frost_breath_tier_scalars(icon)?;

    Ok(vec![SemanticOwnerCandidate {
        effect_id: 2_110_126,
        owner_skill_id: skill_id,
        relationship_source: "current-build-unique-skill-effect-term-duration-and-localized-semantics",
        skill_effect_id: 395_801,
        source_subskill_id: None,
        source_subskill_effect_id: None,
        item_id: Some(3_000_105),
        monster_id: Some(3_000_054),
        runtime_monster_id: None,
        transformed_attribute_id: Some(11_152),
        matching_terms: vec!["Versatility", "20-second duration"],
        matching_duration_seconds: 20,
        stack_cap: Some(1),
        recipient_scope: "summon-caster-only-per-current-localized-description",
        rdps_disposition: "ordinary-owner-stats-never-transferred",
        proof_state: "current-build-unique-semantic-and-duration-candidate-not-numeric-owner-edge",
        runtime_authority: false,
    }])
}

fn predator_slash_semantic_candidate(
    row: &Value,
    skills_by_id: &BTreeMap<i64, &Value>,
    skill_effects_by_id: &BTreeMap<i64, &Value>,
    buffs_by_id: &BTreeMap<i64, &Value>,
    monsters_by_id: &BTreeMap<i64, &Value>,
) -> Result<Vec<SemanticOwnerCandidate>, Box<dyn std::error::Error>> {
    if integer(row, "AoyiItemId") != Some(3_000_027)
        || integer(row, "MonsterId") != Some(10_052)
        || row.get("TransformationType") != Some(&json!([[3, 3_210_040, 1]]))
    {
        return Err("Stunt! Predator Slash current-build Aoyi identity changed".into());
    }
    require_skill_effect(skill_effects_by_id, 392_801, 3_928, Some(75))?;
    require_skill_description(
        skills_by_id,
        1_005_240,
        100_524_001,
        &["ATK +", "5%", "10s", "10"],
    )?;
    require_skill_effect(skill_effects_by_id, 100_524_001, 1_005_240, None)?;
    require_runtime_monster(monsters_by_id, 3_000_030, 1_005_240)?;
    require_buff(
        buffs_by_id,
        2_110_077,
        "Intimidation",
        "Gain ATK for each enemy killed",
        10,
        &[2, 10],
        75,
    )?;

    Ok(vec![SemanticOwnerCandidate {
        effect_id: 2_110_077,
        owner_skill_id: 3_928,
        relationship_source: "current-build-aoyi-item-summon-subskill-tag-duration-and-stack-semantics",
        skill_effect_id: 392_801,
        source_subskill_id: Some(1_005_240),
        source_subskill_effect_id: Some(100_524_001),
        item_id: Some(3_000_027),
        monster_id: Some(10_052),
        runtime_monster_id: Some(3_000_030),
        transformed_attribute_id: Some(3_210_040),
        matching_terms: vec![
            "ATK +5% per defeated enemy",
            "10-second duration",
            "10 stacks",
        ],
        matching_duration_seconds: 10,
        stack_cap: Some(10),
        recipient_scope: "summon-caster-only-per-current-skill-description",
        rdps_disposition: "ordinary-owner-damage-never-transferred",
        proof_state: "current-build-unique-semantic-tag-duration-and-stack-candidate-not-numeric-owner-edge",
        runtime_authority: false,
    }])
}

fn blink_ambush_semantic_candidate(
    row: &Value,
    skills_by_id: &BTreeMap<i64, &Value>,
    skill_effects_by_id: &BTreeMap<i64, &Value>,
    buffs_by_id: &BTreeMap<i64, &Value>,
    monsters_by_id: &BTreeMap<i64, &Value>,
) -> Result<Vec<SemanticOwnerCandidate>, Box<dyn std::error::Error>> {
    if integer(row, "AoyiItemId") != Some(3_000_035)
        || integer(row, "MonsterId") != Some(10_038)
        || row.get("TransformationType") != Some(&json!([[1, 11_034, 400]]))
    {
        return Err("Stunt! Blink Ambush current-build Aoyi identity changed".into());
    }
    require_skill_effect(skill_effects_by_id, 393_401, 3_934, None)?;
    require_skill_description(
        skills_by_id,
        2_001_740,
        200_174_001,
        &["reduces target's DEF", "10s", "cannot stack"],
    )?;
    require_skill_effect(skill_effects_by_id, 200_174_001, 2_001_740, None)?;
    require_runtime_monster(monsters_by_id, 3_000_031, 2_001_740)?;
    require_buff(
        buffs_by_id,
        2_110_078,
        "Shock Defense Break",
        "Defense reduced",
        10,
        &[1, 1],
        78,
    )?;

    Ok(vec![SemanticOwnerCandidate {
        effect_id: 2_110_078,
        owner_skill_id: 3_934,
        relationship_source: "current-build-aoyi-item-summon-subskill-target-defense-duration-semantics",
        skill_effect_id: 393_401,
        source_subskill_id: Some(2_001_740),
        source_subskill_effect_id: Some(200_174_001),
        item_id: Some(3_000_035),
        monster_id: Some(10_038),
        runtime_monster_id: Some(3_000_031),
        transformed_attribute_id: Some(11_034),
        matching_terms: vec![
            "target DEF reduction",
            "10-second duration",
            "non-stackable",
        ],
        matching_duration_seconds: 10,
        stack_cap: Some(1),
        recipient_scope: "skill-target-enemy-per-current-skill-description",
        rdps_disposition: "external-target-mitigation-candidate-runtime-proof-required",
        proof_state: "current-build-unique-summon-subskill-semantic-and-duration-candidate-not-numeric-owner-edge",
        runtime_authority: false,
    }])
}

fn standalone_semantic_owner_candidates(
    skills_by_id: &BTreeMap<i64, &Value>,
    skill_effects_by_id: &BTreeMap<i64, &Value>,
    buffs_by_id: &BTreeMap<i64, &Value>,
    monsters_by_id: &BTreeMap<i64, &Value>,
) -> Result<Vec<SemanticOwnerCandidate>, Box<dyn std::error::Error>> {
    require_skill_description(
        skills_by_id,
        3_933,
        393_301,
        &["reducing the target's ATK", "10s", "Non-stackable"],
    )?;
    require_skill_effect(skill_effects_by_id, 393_301, 3_933, Some(77))?;
    require_skill_description(skills_by_id, 1_005_707, 100_570_701, &["Demoralizing Roar"])?;
    require_skill_effect(skill_effects_by_id, 100_570_701, 1_005_707, None)?;
    require_runtime_monster(monsters_by_id, 3_000_015, 1_005_707)?;
    require_buff(
        buffs_by_id,
        2_110_070,
        "Demoralizing Roar",
        "ATK reduced",
        10,
        &[1, 1],
        77,
    )?;

    Ok(vec![SemanticOwnerCandidate {
        effect_id: 2_110_070,
        owner_skill_id: 3_933,
        relationship_source: "current-build-skill-effect-tag-summon-subskill-target-attack-duration-semantics",
        skill_effect_id: 393_301,
        source_subskill_id: Some(1_005_707),
        source_subskill_effect_id: Some(100_570_701),
        item_id: None,
        monster_id: None,
        runtime_monster_id: Some(3_000_015),
        transformed_attribute_id: None,
        matching_terms: vec![
            "target ATK reduction",
            "10-second duration",
            "non-stackable",
        ],
        matching_duration_seconds: 10,
        stack_cap: Some(1),
        recipient_scope: "skill-target-enemy-per-current-skill-description",
        rdps_disposition: "defensive-mitigation-candidate-not-offensive-rdps",
        proof_state: "current-build-unique-skill-tag-summon-subskill-semantic-and-duration-candidate-not-numeric-owner-edge",
        runtime_authority: false,
    }])
}

fn require_skill_description(
    skills_by_id: &BTreeMap<i64, &Value>,
    skill_id: i64,
    skill_effect_id: i64,
    expected_terms: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    let skill = skills_by_id
        .get(&skill_id)
        .copied()
        .ok_or_else(|| format!("current SkillTable row {skill_id} is missing"))?;
    if !integer_array(skill, "EffectIDs").contains(&skill_effect_id) {
        return Err(format!("skill {skill_id} no longer owns effect {skill_effect_id}").into());
    }
    let searchable = format!(
        "{} {}",
        string_ref(skill, "Name").unwrap_or_default(),
        string_ref(skill, "Desc").unwrap_or_default()
    );
    if !expected_terms.iter().all(|term| searchable.contains(term)) {
        return Err(format!("skill {skill_id} semantic terms changed").into());
    }
    Ok(())
}

fn require_skill_effect(
    skill_effects_by_id: &BTreeMap<i64, &Value>,
    skill_effect_id: i64,
    owner_skill_id: i64,
    required_tag: Option<i64>,
) -> Result<(), Box<dyn std::error::Error>> {
    let skill_effect = skill_effects_by_id
        .get(&skill_effect_id)
        .copied()
        .ok_or_else(|| format!("current SkillEffectTable row {skill_effect_id} is missing"))?;
    if integer(skill_effect, "SkillId") != Some(owner_skill_id)
        || required_tag.is_some_and(|tag| !integer_array(skill_effect, "Tags").contains(&tag))
    {
        return Err(format!("skill-effect {skill_effect_id} ownership or tag changed").into());
    }
    Ok(())
}

fn require_runtime_monster(
    monsters_by_id: &BTreeMap<i64, &Value>,
    monster_id: i64,
    born_skill_id: i64,
) -> Result<(), Box<dyn std::error::Error>> {
    let monster = monsters_by_id
        .get(&monster_id)
        .copied()
        .ok_or_else(|| format!("current MonsterTable row {monster_id} is missing"))?;
    if integer(monster, "BornSkillId") != Some(born_skill_id)
        || !integer_array(monster, "SkillIds").contains(&born_skill_id)
    {
        return Err(format!("runtime monster {monster_id} skill route changed").into());
    }
    Ok(())
}

fn require_buff(
    buffs_by_id: &BTreeMap<i64, &Value>,
    buff_id: i64,
    expected_name: &str,
    expected_description_term: &str,
    duration_seconds: u64,
    repeat_add_rule: &[i64],
    required_tag: i64,
) -> Result<(), Box<dyn std::error::Error>> {
    let buff = buffs_by_id
        .get(&buff_id)
        .copied()
        .ok_or_else(|| format!("current BuffTable row {buff_id} is missing"))?;
    let duration_matches = buff
        .get("DestroyParam")
        .and_then(Value::as_array)
        .and_then(|outer| outer.first())
        .and_then(Value::as_array)
        .and_then(|inner| inner.get(1))
        .and_then(Value::as_f64)
        == Some(duration_seconds as f64);
    if string_ref(buff, "Name") != Some(expected_name)
        || !string_ref(buff, "Desc")
            .is_some_and(|description| description.contains(expected_description_term))
        || !duration_matches
        || integer_array(buff, "RepeatAddRule") != repeat_add_rule
        || !integer_array(buff, "Tags").contains(&required_tag)
    {
        return Err(format!("buff {buff_id} identity, duration, stacking, or tag changed").into());
    }
    Ok(())
}

fn integer_array(value: &Value, key: &str) -> Vec<i64> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(Value::as_i64).collect())
        .unwrap_or_default()
}

fn skill_attr_description_matches(skill_effect: &Value, label: &str, value: &str) -> bool {
    skill_effect
        .get("SkillAttrDes")
        .and_then(Value::as_array)
        .is_some_and(|entries| {
            entries.iter().any(|entry| {
                entry.as_array().is_some_and(|parts| {
                    parts.first().and_then(Value::as_str) == Some(label)
                        && parts.get(1).and_then(Value::as_str) == Some(value)
                })
            })
        })
}

fn require_frost_breath_tier_scalars(icon: &Value) -> Result<(), Box<dyn std::error::Error>> {
    let tiers = icon
        .get("TierEffects")
        .and_then(Value::as_array)
        .ok_or("Stunt! Frost Breath tier effects are missing")?;
    let expected = [
        (1, 130, 6552),
        (2, 260, 8064),
        (3, 390, 9576),
        (4, 520, 11088),
        (5, 650, 12600),
    ];
    if tiers.len() != expected.len() {
        return Err("Stunt! Frost Breath tier count changed".into());
    }
    for (tier, (expected_tier, raw_value, transformed_value)) in tiers.iter().zip(expected) {
        let actual_raw = tier
            .get("values")
            .and_then(Value::as_array)
            .and_then(|values| values.first())
            .and_then(|value| value.get("rawValue"))
            .and_then(Value::as_i64);
        let transformations = tier
            .get("TransformationType")
            .and_then(Value::as_array)
            .ok_or("Stunt! Frost Breath tier transformation is missing")?;
        let has_attribute_transform = transformations.iter().any(|value| {
            value.as_array().is_some_and(|parts| {
                parts.first().and_then(Value::as_i64) == Some(1)
                    && parts.get(1).and_then(Value::as_i64) == Some(11_152)
                    && parts.get(2).and_then(Value::as_i64) == Some(transformed_value)
            })
        });
        if integer(tier, "tier") != Some(expected_tier)
            || actual_raw != Some(raw_value)
            || !has_attribute_transform
        {
            return Err("Stunt! Frost Breath tier scalar identity changed".into());
        }
    }
    Ok(())
}

fn exact_component_routes(
    row: &Value,
    skill_id: i64,
    buffs_by_id: &BTreeMap<i64, &Value>,
    damage_attrs_by_id: &BTreeMap<i64, &Value>,
    passive_owner_buff_ids: &[i64],
    owner_family_candidates: &[FamilyBuffCandidate],
    exact_relationships: &[ExactRelationshipCandidate],
    damage_chains: &[ExactDamageChainCandidate],
    projectile_status_proof: &AoyiProjectileStatusProof,
) -> Result<Vec<ComponentRoute>, Box<dyn std::error::Error>> {
    if skill_id == 3_914 {
        let expected_transforms = json!([[1, 11_172, 2_240]]);
        if row.get("TransformationType") != Some(&expected_transforms) {
            return Err("Stunt! Blade Sweep direct Block transformation changed".into());
        }
        require_emitted_damage_ids(
            skill_id,
            damage_chains,
            &[139_140_102, 139_140_103, 31_004_010_200],
        )?;
        require_buff_identity(
            buffs_by_id,
            3_200_101,
            "\u{683c}\u{6321}-\u{5251}\u{5203}\u{6a2a}\u{626b}-\u{968f}\u{8d5b}\u{5b63}\u{7b49}\u{7ea7}\u{6210}\u{957f}",
            None,
        )?;
        return Ok(vec![
            ComponentRoute {
                component_id: "blade-sweep-equipped-block",
                role: "owner-equipped-flat-block-stat",
                effect_ids: vec![11_172, 3_200_101],
                source_config_ids: vec![3_914, 3_200_101],
                recipient_scope: "provider-only-while-equipped",
                rdps_disposition: "defense-lane-never-invent-damage-credit",
                proof_state: "current-static-direct-attribute-consumer-and-season-scaling-buff-identity-exact",
            },
            ComponentRoute {
                component_id: "blade-sweep-summon-damage-family",
                role: "owner-produced-direct-damage-family",
                effect_ids: vec![139_140_102, 139_140_103, 31_004_010_200],
                source_config_ids: vec![270, 3_914, 391_401],
                recipient_scope: "provider-owned-summon",
                rdps_disposition: "ordinary-owner-damage-never-support-credit",
                proof_state: "current-static-recount-and-damage-row-identities-exact",
            },
            ComponentRoute {
                component_id: "blade-sweep-target-armor-reduction",
                role: "transferable-external-target-mitigation",
                effect_ids: vec![projectile_status_proof.current_static.target_status_id],
                source_config_ids: vec![
                    270,
                    3_914,
                    3_946,
                    391_401,
                    394_601,
                    projectile_status_proof.current_static.projectile_config_id,
                    projectile_status_proof.current_static.damage_attr_id,
                ],
                recipient_scope: "packet-observed-enemy-targets-hit-by-shared-blade-sweep-projectile",
                rdps_disposition: "preserve-exact-status-window-block-transfer-until-current-packet-provider-and-armor-formula",
                proof_state: "current-static-chain-exact-plus-historical-projectile-status-edge-current-packet-provider-live-gated",
            },
        ]);
    }

    if skill_id == 3_946 {
        require_damage_id(skill_id, damage_chains, 136_990_101)?;
        require_damage_id(skill_id, damage_chains, 2_211_006_203)?;
        require_damage_id(skill_id, damage_chains, 11_007_300_102)?;
        require_damage_id(skill_id, damage_chains, 31_004_010_200)?;
        require_damage_id(skill_id, damage_chains, 110_086_410_103)?;
        return Ok(vec![
            ComponentRoute {
                component_id: "goblin-march-summoned-goblin-damage-family",
                role: "owner-produced-summoned-goblin-damage-family",
                effect_ids: vec![
                    136_990_101,
                    2_211_006_203,
                    11_007_300_102,
                    31_004_010_200,
                    110_086_410_103,
                ],
                source_config_ids: vec![3_946, 394_601],
                recipient_scope: "provider-owned-randomly-summoned-goblins",
                rdps_disposition: "ordinary-owner-damage-never-support-credit",
                proof_state: "current-static-damage-merge-branches-exact-runtime-branch-selection-packet-gated",
            },
            ComponentRoute {
                component_id: "goblin-march-shared-blade-sweep-target-armor-reduction",
                role: "conditional-shared-projectile-external-target-mitigation",
                effect_ids: vec![projectile_status_proof.current_static.target_status_id],
                source_config_ids: vec![
                    270,
                    3_914,
                    3_946,
                    391_401,
                    394_601,
                    projectile_status_proof.current_static.projectile_config_id,
                    projectile_status_proof.current_static.damage_attr_id,
                ],
                recipient_scope: "enemy-targets-hit-when-goblin-march-summons-the-shared-blade-sweep-projectile",
                rdps_disposition: "preserve-exact-status-window-select-owner-from-packet-never-from-shared-config",
                proof_state: "current-static-shared-owner-chain-exact-plus-historical-projectile-status-edge-current-packet-owner-live-gated",
            },
        ]);
    }

    if skill_id == 3_921 {
        let expected_transforms = json!([[1, 11_014, 600], [1, 11_024, 600], [1, 11_034, 600]]);
        if row.get("TransformationType") != Some(&expected_transforms) {
            return Err("Arcane! Time Decree direct main-stat transformations changed".into());
        }
        require_exact_relationship(exact_relationships, 2_110_034, 2_110_033)?;
        return Ok(vec![
            ComponentRoute {
                component_id: "time-decree-equipped-main-stats",
                role: "owner-equipped-main-stat-percentage-family",
                effect_ids: vec![11_014, 11_024, 11_034],
                source_config_ids: vec![3_921],
                recipient_scope: "provider-only-while-equipped",
                rdps_disposition: "ordinary-owner-stats-never-transferred",
                proof_state: "current-static-direct-attribute-consumer-and-identities-exact",
            },
            ComponentRoute {
                component_id: "time-decree-external-cooldown-speed",
                role: "transferable-external-action-opportunity",
                effect_ids: vec![2_110_034],
                source_config_ids: vec![2_110_033, 3_921, 392_101],
                recipient_scope: "provider-and-up-to-ten-nearby-allies",
                rdps_disposition: "cooldown-window-counterfactual-only-no-direct-damage-transfer",
                proof_state: "current-static-owner-source-duration-and-tier-values-exact-current-packet-live-gated",
            },
        ]);
    }

    if skill_id == 3_962 {
        let expected_transforms = json!([[3, 3_200_034, 1]]);
        let expected_parameters = json!([[500, 80_000, 4_500]]);
        if row.get("TransformationType") != Some(&expected_transforms)
            || row.get("BuffPar") != Some(&expected_parameters)
            || !passive_owner_buff_ids.contains(&3_200_034)
        {
            return Err("Stunt! Healing Bomb passive identity or parameters changed".into());
        }
        require_emitted_damage_ids(
            skill_id,
            damage_chains,
            &[
                111_013_040_101,
                111_013_040_102,
                2_320_003_403,
                2_211_013_103,
                2_211_003_503,
            ],
        )?;
        return Ok(vec![
            ComponentRoute {
                component_id: "healing-bomb-summon-damage-family",
                role: "owner-produced-direct-damage-family",
                effect_ids: vec![111_013_040_101, 111_013_040_102],
                source_config_ids: vec![301, 1_101_304, 396_201],
                recipient_scope: "provider-owned-summon",
                rdps_disposition: "ordinary-owner-damage-never-support-credit",
                proof_state: "current-static-recount-and-damage-row-identities-exact",
            },
            ComponentRoute {
                component_id: "healing-bomb-direct-party-heal",
                role: "external-produced-healing",
                effect_ids: vec![2_110_035, 2_211_003_503],
                source_config_ids: vec![3_962, 396_201],
                recipient_scope: "up-to-ten-nearby-allies-prioritizing-teammates",
                rdps_disposition: "healing-attribution-lane-never-invent-damage-credit",
                proof_state: "current-static-buff-source-and-healing-damage-row-identities-exact",
            },
            ComponentRoute {
                component_id: "healing-bomb-self-heal-conversion-passive",
                role: "owner-produced-damage-derived-from-self-healing",
                effect_ids: vec![3_200_034, 2_320_003_403],
                source_config_ids: vec![3_962, 3_200_034],
                recipient_scope: "provider-owned-passive-proc",
                rdps_disposition: "ordinary-owner-damage-never-support-credit",
                proof_state: "current-static-passive-recount-and-damage-row-identities-exact-formula-live-gated",
            },
            ComponentRoute {
                component_id: "healing-bomb-recount-sibling-healing",
                role: "preserved-recount-sibling-output",
                effect_ids: vec![2_211_013_103],
                source_config_ids: vec![301, 2_110_131],
                recipient_scope: "packet-actor-and-recipient-unresolved",
                rdps_disposition: "preserve-never-transfer-until-packet-source-edge-is-exact",
                proof_state: "current-static-recount-membership-and-healing-row-identity-exact",
            },
        ]);
    }

    if skill_id == 3_981 {
        let expected_transforms = json!([[1, 11_812, 400]]);
        if row.get("TransformationType") != Some(&expected_transforms) {
            return Err("Stunt! Lock-On Shot direct Shield transformation changed".into());
        }
        require_emitted_damage_ids(skill_id, damage_chains, &[111_218_100_101, 111_218_100_102])?;
        require_buff_output_route(
            buffs_by_id,
            damage_attrs_by_id,
            2_110_150,
            "\u{773c}\u{7403}\u{7cbe}\u{82f1}-\u{4e3b}\u{52a8}",
            15.0,
            2_211_015_001,
            "AddShield",
        )?;
        return Ok(vec![
            ComponentRoute {
                component_id: "lock-on-shot-equipped-shield-strength",
                role: "owner-equipped-flat-shield-strength",
                effect_ids: vec![11_812],
                source_config_ids: vec![3_981],
                recipient_scope: "provider-only-while-equipped",
                rdps_disposition: "defense-lane-never-invent-damage-credit",
                proof_state: "current-static-direct-attribute-consumer-and-identity-exact",
            },
            ComponentRoute {
                component_id: "lock-on-shot-summon-damage-family",
                role: "owner-produced-direct-damage-family",
                effect_ids: vec![111_218_100_101, 111_218_100_102],
                source_config_ids: vec![320, 1_121_810, 398_101],
                recipient_scope: "provider-owned-summon",
                rdps_disposition: "ordinary-owner-damage-never-support-credit",
                proof_state: "current-static-recount-and-damage-row-identities-exact",
            },
            ComponentRoute {
                component_id: "lock-on-shot-party-shield",
                role: "external-produced-shield",
                effect_ids: vec![2_110_150, 2_211_015_001],
                source_config_ids: vec![3_981, 398_101, 2_110_150],
                recipient_scope: "provider-and-up-to-ten-nearby-allies-prioritizing-teammates",
                rdps_disposition: "defense-lane-never-invent-damage-credit",
                proof_state: "current-static-unique-summon-buff-duration-and-shield-output-exact-packet-provider-recipient-live-gated",
            },
        ]);
    }

    if skill_id == 3_982 {
        let expected_transforms = json!([[1, 11_014, 600], [1, 11_024, 600], [1, 11_034, 600]]);
        if row.get("TransformationType") != Some(&expected_transforms) {
            return Err(
                "Arcane! Celestial Spirit Mage direct main-stat transformations changed".into(),
            );
        }
        require_exact_relationship(exact_relationships, 2_110_161, 2_110_161)?;
        require_exact_relationship_target(exact_relationships, 2_110_161, 2_211_016_105)?;
        require_exact_relationship(exact_relationships, 2_110_167, 2_110_166)?;
        require_emitted_damage_ids(
            skill_id,
            damage_chains,
            &[
                125_010_101,
                125_020_101,
                125_030_101,
                125_040_102,
                125_040_103,
                125_040_104,
                325_010_100,
            ],
        )?;
        return Ok(vec![
            ComponentRoute {
                component_id: "celestial-spirit-mage-equipped-main-stats",
                role: "owner-equipped-main-stat-percentage-family",
                effect_ids: vec![11_014, 11_024, 11_034],
                source_config_ids: vec![3_982],
                recipient_scope: "provider-only-while-equipped",
                rdps_disposition: "ordinary-owner-stats-never-transferred",
                proof_state: "current-static-direct-attribute-consumer-and-identities-exact",
            },
            ComponentRoute {
                component_id: "celestial-spirit-mage-transformation-state",
                role: "transformation-and-output-origin",
                effect_ids: vec![2_110_161, 2_211_016_105],
                source_config_ids: vec![3_982, 398_201],
                recipient_scope: "provider-transformation-state",
                rdps_disposition: "uptime-and-routing-only-never-directly-transferred",
                proof_state: "current-static-owner-source-and-output-row-identities-exact",
            },
            ComponentRoute {
                component_id: "celestial-spirit-mage-direct-damage-family",
                role: "owner-produced-transformation-damage-family",
                effect_ids: vec![
                    125_010_101,
                    125_020_101,
                    125_030_101,
                    125_040_102,
                    125_040_103,
                    125_040_104,
                    325_010_100,
                ],
                source_config_ids: vec![337, 338, 339, 3_982, 398_201],
                recipient_scope: "provider-owned-transformation",
                rdps_disposition: "ordinary-owner-damage-never-support-credit",
                proof_state: "current-static-recount-and-damage-row-identities-exact",
            },
            ComponentRoute {
                component_id: "celestial-guardian-morale-reduction",
                role: "mixed-defensive-attack-reduction-and-external-target-vulnerability",
                effect_ids: vec![2_110_167],
                source_config_ids: vec![2_110_166, 3_982, 398_201],
                recipient_scope: "up-to-five-enemies-in-range",
                rdps_disposition: "credit-only-other-players-marginal-damage-on-same-target-and-window",
                proof_state: "current-static-owner-source-duration-and-visible-semantics-exact-current-packet-live-gated",
            },
            ComponentRoute {
                component_id: "celestial-guardian-party-shield",
                role: "external-produced-shield",
                effect_ids: vec![2_110_168, 2_211_016_801],
                source_config_ids: vec![3_982, 398_201],
                recipient_scope: "up-to-ten-allies-in-range-prioritizing-teammates",
                rdps_disposition: "defense-lane-never-invent-damage-credit",
                proof_state: "current-static-lucy-design-name-duration-and-shield-row-exact-owner-edge-localized-only",
            },
            ComponentRoute {
                component_id: "celestial-spirit-damage-to-healing-conversion",
                role: "external-produced-healing-derived-from-provider-damage",
                effect_ids: vec![2_110_161],
                source_config_ids: vec![3_982, 398_201],
                recipient_scope: "up-to-ten-nearby-allies-prioritizing-teammates",
                rdps_disposition: "healing-attribution-lane-never-invent-damage-credit",
                proof_state: "current-localized-fifty-percent-conversion-semantics-packet-heal-output-id-unresolved",
            },
        ]);
    }

    match skill_id {
        3_915 => {
            return self_only_aoyi_components(
                row,
                skill_id,
                passive_owner_buff_ids,
                owner_family_candidates,
                damage_chains,
                "Stunt! Frenzied Shot",
                &json!([[3, 3_200_009, 1]]),
                &json!([[1_120]]),
                &[3_200_009],
                2_110_109,
                11_007_300_102,
                100_730,
            );
        }
        3_926 => {
            return self_only_aoyi_components(
                row,
                skill_id,
                passive_owner_buff_ids,
                owner_family_candidates,
                damage_chains,
                "Arcane! Meteor Shower",
                &json!([[3, 3_210_020, 1]]),
                &json!([[2_240]]),
                &[3_210_020, 3_210_021, 3_210_022],
                2_110_102,
                136_990_101,
                3_699,
            );
        }
        3_937 => {
            return self_only_aoyi_components(
                row,
                skill_id,
                passive_owner_buff_ids,
                owner_family_candidates,
                damage_chains,
                "Stunt! Thunder Suppress",
                &json!([[3, 3_210_080, 1]]),
                &json!([[1_120]]),
                &[3_210_080, 3_210_081],
                2_110_110,
                11_010_440_010_2,
                1_010_440,
            );
        }
        3_964 => {
            return self_only_aoyi_components(
                row,
                skill_id,
                passive_owner_buff_ids,
                owner_family_candidates,
                damage_chains,
                "Stunt! Cabbage Boomerang",
                &json!([[3, 3_200_035, 1]]),
                &json!([[500, 700]]),
                &[3_200_035],
                2_110_132,
                11_011_312_010_3,
                1_011_312,
            );
        }
        _ => {}
    }

    if skill_id == 3_935 {
        let expected_transforms = json!([[1, 11812, 800]]);
        if row.get("TransformationType") != Some(&expected_transforms) {
            return Err("Arcane! Thunder Roar shield transformation identity changed".into());
        }
        require_damage_id(skill_id, damage_chains, 2_211_009_604)?;
        require_source_target_damage_ids(
            skill_id,
            damage_chains,
            &[2_211_009_601, 2_211_009_603, 2_211_009_604],
        )?;
        return Ok(vec![
            ComponentRoute {
                component_id: "thunder-roar-electro-shield",
                role: "transferable-party-shield-with-tier-scalar",
                effect_ids: vec![2_110_096, 11_812],
                source_config_ids: vec![3_935, 393_501],
                recipient_scope: "provider-and-up-to-ten-allies",
                rdps_disposition: "defense-lane-never-invent-damage-credit",
                proof_state: "current-static-recipient-scope-duration-and-shield-identity-exact",
            },
            ComponentRoute {
                component_id: "thunder-roar-direct-cast-damage",
                role: "directly-referenced-aoyi-effect-damage",
                effect_ids: vec![2_211_009_604],
                source_config_ids: vec![3_935, 393_501],
                recipient_scope: "aoyi-caster-damage-source",
                rdps_disposition: "ordinary-owner-damage-never-support-credit",
                proof_state: "current-static-direct-skill-effect-damage-route-exact",
            },
            ComponentRoute {
                component_id: "thunder-roar-recipient-thunderstrike",
                role: "recipient-triggered-produced-damage",
                effect_ids: vec![2_110_096, 2_211_009_603],
                source_config_ids: vec![3_935, 393_501, 2_110_096],
                recipient_scope: "each-shielded-recipient-triggers-from-that-friendly-attack",
                rdps_disposition: "blocked-until-packet-attacker-owner-and-damage-conservation-proven",
                proof_state: "current-static-source-and-formula-rows-exact-current-packet-actor-unobserved",
            },
            ComponentRoute {
                component_id: "thunder-roar-recount-routing-placeholder",
                role: "zero-formula-source-and-recount-routing-row",
                effect_ids: vec![2_211_009_601],
                source_config_ids: vec![2_110_096, 273],
                recipient_scope: "no-produced-damage",
                rdps_disposition: "preserve-for-routing-never-count-as-damage",
                proof_state: "current-static-empty-formula-row-exact",
            },
        ]);
    }

    if skill_id == 3_969 {
        let expected_transforms = json!([[3, 3210180, 1]]);
        let expected_parameters = json!([[560, 166]]);
        if row.get("TransformationType") != Some(&expected_transforms)
            || row.get("BuffPar") != Some(&expected_parameters)
            || !passive_owner_buff_ids.contains(&3_210_180)
        {
            return Err("Arcane! Rift Recoil static component identities changed".into());
        }
        let family_ids = owner_family_candidates
            .iter()
            .map(|candidate| candidate.buff_id)
            .collect::<BTreeSet<_>>();
        if !family_ids.contains(&2_110_154) || !family_ids.contains(&3_210_181) {
            return Err(
                "Arcane! Rift Recoil Thunderwind child or passive stack status is missing".into(),
            );
        }
        require_damage_id(skill_id, damage_chains, 11_401_450_102)?;
        return Ok(vec![
            ComponentRoute {
                component_id: "arcane-rift-recoil-thunderwind-power",
                role: "owner-only-critical-rate-and-critical-damage-modifier",
                effect_ids: vec![2_110_138, 2_110_154],
                source_config_ids: vec![140_145, 2_110_138],
                recipient_scope: "summon-owner-only",
                rdps_disposition: "ordinary-owner-damage-never-transferred",
                proof_state: "current-static-self-wording-plus-historical-owner-linked-packet-lifecycle",
            },
            ComponentRoute {
                component_id: "arcane-rift-recoil-equipped-passive",
                role: "owner-equipped-passive-critical-damage-stack-family",
                effect_ids: vec![3_210_180, 3_210_181],
                source_config_ids: vec![3_969, 3_210_180],
                recipient_scope: "provider-only-while-equipped",
                rdps_disposition: "ordinary-owner-stats-never-transferred",
                proof_state: "current-static-identity-plus-historical-stack-origin",
            },
            ComponentRoute {
                component_id: "arcane-rift-recoil-summon-damage",
                role: "owner-produced-direct-damage",
                effect_ids: vec![11_401_450_102],
                source_config_ids: vec![140_145],
                recipient_scope: "provider-owned-summon",
                rdps_disposition: "ordinary-owner-damage-never-support-credit",
                proof_state: "current-static-damage-route-exact",
            },
        ]);
    }

    if skill_id == 3_957 {
        let expected_transforms = json!([[1, 11014, 600], [1, 11024, 600], [1, 11034, 600]]);
        if row.get("TransformationType") != Some(&expected_transforms) {
            return Err("Fatal Spiral transformation identities changed".into());
        }
        require_damage_id(skill_id, damage_chains, 111_007_400_108)?;
        return Ok(vec![
            ComponentRoute {
                component_id: "fatal-spiral-equipped-main-stat-transforms",
                role: "owner-equipped-passive-stat-family",
                effect_ids: vec![11_014, 11_024, 11_034],
                source_config_ids: vec![3_957],
                recipient_scope: "provider-only-while-equipped",
                rdps_disposition: "ordinary-owner-stats-never-transferred",
                proof_state: "current-static-identity-exact",
            },
            ComponentRoute {
                component_id: "fatal-spiral-summon-damage",
                role: "owner-produced-direct-damage",
                effect_ids: vec![111_007_400_108],
                source_config_ids: vec![1_100_740],
                recipient_scope: "provider-owned-summon",
                rdps_disposition: "ordinary-owner-damage-never-support-credit",
                proof_state: "current-static-damage-route-exact",
            },
            ComponentRoute {
                component_id: "fatal-spiral-shared-all-element-bonus",
                role: "transferable-external-modifier",
                effect_ids: vec![2_110_125],
                source_config_ids: vec![3_957, 2_110_125],
                recipient_scope: "provider-and-up-to-ten-nearby-allies",
                rdps_disposition: "packet-provider-recipient-window-counterfactual-only",
                proof_state: "current-build-packet-provider-recipient-identity-repeated-across-seven-equipped-source-observations-formula-still-live-gated",
            },
            ComponentRoute {
                component_id: "fatal-spiral-caster-side-marker",
                role: "provider-side-routing-marker",
                effect_ids: vec![2_110_124],
                source_config_ids: vec![3_957, 2_110_124],
                recipient_scope: "provider-only",
                rdps_disposition: "routing-only-never-count-or-transfer-as-damage",
                proof_state: "current-build-packet-lifecycle-repeated-across-seven-equipped-sources-and-always-self-targeted",
            },
        ]);
    }

    if skill_id == 3_971 {
        let expected_transforms = json!([[3, 3200038, 1]]);
        let expected_parameters = json!([[600, 800]]);
        if row.get("TransformationType") != Some(&expected_transforms)
            || row.get("BuffPar") != Some(&expected_parameters)
            || !passive_owner_buff_ids.contains(&3_200_038)
        {
            return Err("Superconductor Surge static component identities changed".into());
        }
        let family_ids = owner_family_candidates
            .iter()
            .map(|candidate| candidate.buff_id)
            .collect::<BTreeSet<_>>();
        if !family_ids.contains(&2_110_140) {
            return Err("Superconductor Surge Mechanical Power status 2110140 is missing".into());
        }
        require_damage_id(skill_id, damage_chains, 11_110_690_101)?;
        return Ok(vec![
            ComponentRoute {
                component_id: "superconductor-surge-mechanical-power-main-stats",
                role: "transferable-external-derived-attack-and-haste-modifier",
                effect_ids: vec![2_110_140],
                source_config_ids: vec![3_971],
                recipient_scope: "provider-and-up-to-ten-nearby-allies",
                rdps_disposition: "exact-wire-attribute-delta-counterfactual-only",
                proof_state: "historical-tier-four-loadout-lifecycle-and-isolated-removal-current-build-live-gated",
            },
            ComponentRoute {
                component_id: "superconductor-surge-mechanical-power-healing-received",
                role: "transferable-external-healing-modifier",
                effect_ids: vec![2_110_140],
                source_config_ids: vec![3_971],
                recipient_scope: "provider-and-up-to-ten-nearby-allies",
                rdps_disposition: "healing-attribution-lane-never-invent-damage-credit",
                proof_state: "current-localized-and-tier-parameter-identity-packet-formula-not-yet-isolated",
            },
            ComponentRoute {
                component_id: "superconductor-surge-equipped-passive",
                role: "owner-equipped-passive-stat-family",
                effect_ids: vec![3_200_038],
                source_config_ids: vec![3_971],
                recipient_scope: "provider-only-while-equipped",
                rdps_disposition: "ordinary-owner-stats-never-transferred",
                proof_state: "current-static-identity-exact",
            },
            ComponentRoute {
                component_id: "superconductor-surge-summon-damage",
                role: "owner-produced-direct-damage",
                effect_ids: vec![11_110_690_101],
                source_config_ids: vec![111_069],
                recipient_scope: "provider-owned-summon",
                rdps_disposition: "ordinary-owner-damage-never-support-credit",
                proof_state: "current-static-damage-route-exact",
            },
        ]);
    }

    if skill_id == 3_948 {
        require_buff_shape(
            buffs_by_id,
            2_110_111,
            "罗罗拉-咒术",
            1,
            1,
            &[2, 1],
            Some(20.0),
        )?;
        require_buff_shape(
            buffs_by_id,
            2_110_135,
            "罗罗拉-主动记时",
            0,
            2,
            &[0, 1],
            Some(20.0),
        )?;
        require_buff_shape(
            buffs_by_id,
            2_110_136,
            "罗罗拉-玩家身上监控",
            0,
            2,
            &[2, 99],
            None,
        )?;
        require_exact_relationship(exact_relationships, 2_110_111, 2_110_111)?;
        require_historical_relation(exact_relationships, 2_110_135, 2_110_111)?;
        require_historical_relation(exact_relationships, 2_110_136, 2_110_111)?;
        return Ok(vec![ComponentRoute {
            component_id: "rorola-personal-base-stacking-damage-and-life-steal",
            role: "self-only-modifier",
            effect_ids: vec![2_110_111, 2_110_135, 2_110_136],
            source_config_ids: vec![3_948, 394_801, 2_110_111],
            recipient_scope: "source-restricted-enemy-target-state-for-provider-damage-only",
            rdps_disposition: "ordinary-owner-damage-never-transferred",
            proof_state: "current-static-personal-wording-label-order-buff-lifecycle-and-historical-child-origin-exact",
        }]);
    }

    if skill_id != 3_974 {
        return Ok(Vec::new());
    }

    let family_ids = owner_family_candidates
        .iter()
        .map(|candidate| candidate.buff_id)
        .collect::<BTreeSet<_>>();
    let required_family_ids = BTreeSet::from([2_110_143, 2_110_151, 2_110_153, 3_210_211]);
    let missing_family_ids = required_family_ids
        .difference(&family_ids)
        .copied()
        .collect::<Vec<_>>();
    if !missing_family_ids.is_empty() || !passive_owner_buff_ids.contains(&3_210_210) {
        return Err(format!(
            "Precision Burst component identities changed: missing family IDs {missing_family_ids:?}, passive 3210210 present={}",
            passive_owner_buff_ids.contains(&3_210_210)
        )
        .into());
    }

    let exact_relation = |effect_id: i64, source_config_id: i64| {
        owner_family_candidates.iter().any(|candidate| {
            candidate.historical_relations.iter().any(|relation| {
                relation.effect_id == effect_id
                    && relation.source_type_id == 1
                    && relation.source_config_id == source_config_id
            })
        })
    };
    for (effect_id, source_config_id) in [
        (2_110_143, 2_110_151),
        (2_110_153, 2_110_151),
        (3_210_211, 3_210_210),
    ] {
        if !exact_relation(effect_id, source_config_id) {
            return Err(format!(
                "Precision Burst packet relation {effect_id} <- {source_config_id} is missing"
            )
            .into());
        }
    }

    Ok(vec![
        ComponentRoute {
            component_id: "precision-burst-aura-emitter",
            role: "emitter-and-status-origin",
            effect_ids: vec![2_110_151],
            source_config_ids: Vec::new(),
            recipient_scope: "area-emitter",
            rdps_disposition: "uptime-only-never-directly-transferred",
            proof_state: "current-static-identity-plus-historical-packet-origin",
        },
        ComponentRoute {
            component_id: "functional-amp-external-attack",
            role: "transferable-external-modifier",
            effect_ids: vec![2_110_143],
            source_config_ids: vec![2_110_151],
            recipient_scope: "provider-and-external-teammates-in-area",
            rdps_disposition: "exact-attack-and-mattack-counterfactual-only",
            proof_state: "historically-conserved-current-build-live-gated",
        },
        ComponentRoute {
            component_id: "precision-burst-self-multiplier",
            role: "self-only-modifier",
            effect_ids: vec![2_110_153],
            source_config_ids: vec![2_110_151],
            recipient_scope: "provider-only",
            rdps_disposition: "ordinary-owner-damage-never-transferred",
            proof_state: "current-static-identity-plus-historical-self-only-lifecycle",
        },
        ComponentRoute {
            component_id: "precision-burst-passive-damage",
            role: "owner-produced-damage-family",
            effect_ids: vec![3_210_210, 3_210_211],
            source_config_ids: vec![3_210_210],
            recipient_scope: "provider-owned-passive-proc",
            rdps_disposition: "ordinary-owner-damage-never-support-credit",
            proof_state: "current-static-damage-route-plus-historical-stack-lifecycle",
        },
    ])
}

#[allow(clippy::too_many_arguments)]
fn self_only_aoyi_components(
    row: &Value,
    skill_id: i64,
    passive_owner_buff_ids: &[i64],
    owner_family_candidates: &[FamilyBuffCandidate],
    damage_chains: &[ExactDamageChainCandidate],
    label: &'static str,
    transforms: &Value,
    parameters: &Value,
    passive_effect_ids: &[i64],
    active_effect_id: i64,
    damage_id: i64,
    damage_source_id: i64,
) -> Result<Vec<ComponentRoute>, Box<dyn std::error::Error>> {
    if row.get("TransformationType") != Some(transforms)
        || row.get("BuffPar") != Some(parameters)
        || !passive_owner_buff_ids.contains(&passive_effect_ids[0])
        || !owner_family_candidates
            .iter()
            .any(|candidate| candidate.buff_id == active_effect_id)
    {
        return Err(
            format!("{label} component identities or fixed-point parameters changed").into(),
        );
    }
    for child_id in passive_effect_ids.iter().skip(1) {
        if !owner_family_candidates
            .iter()
            .any(|candidate| candidate.buff_id == *child_id)
        {
            return Err(format!("{label} passive child status {child_id} is missing").into());
        }
    }
    require_damage_id(skill_id, damage_chains, damage_id)?;

    Ok(vec![
        ComponentRoute {
            component_id: "self-only-active-modifier",
            role: "owner-only-offensive-modifier-with-current-fixed-point-parameters",
            effect_ids: vec![active_effect_id],
            source_config_ids: vec![skill_id],
            recipient_scope: "summon-caster-only",
            rdps_disposition: "ordinary-owner-damage-never-transferred",
            proof_state: "current-static-source-chain-self-wording-duration-and-parameter-identity-exact",
        },
        ComponentRoute {
            component_id: "equipped-passive-family",
            role: "owner-equipped-passive-stat-or-proc-family",
            effect_ids: passive_effect_ids.to_vec(),
            source_config_ids: vec![skill_id, passive_effect_ids[0]],
            recipient_scope: "provider-only-while-equipped",
            rdps_disposition: "ordinary-owner-stats-and-procs-never-transferred",
            proof_state: "current-static-transform-and-family-identity-exact",
        },
        ComponentRoute {
            component_id: "summon-direct-damage",
            role: "owner-produced-direct-damage",
            effect_ids: vec![damage_id],
            source_config_ids: vec![damage_source_id],
            recipient_scope: "provider-owned-summon",
            rdps_disposition: "ordinary-owner-damage-never-support-credit",
            proof_state: "current-static-damage-route-exact",
        },
    ])
}

fn require_damage_id(
    skill_id: i64,
    damage_chains: &[ExactDamageChainCandidate],
    damage_id: i64,
) -> Result<(), Box<dyn std::error::Error>> {
    if damage_chains
        .iter()
        .any(|candidate| candidate.resolved_damage_ids.contains(&damage_id))
    {
        Ok(())
    } else {
        Err(
            format!("Aoyi skill {skill_id} current direct-damage identity {damage_id} is missing")
                .into(),
        )
    }
}

fn require_exact_relationship(
    relationships: &[ExactRelationshipCandidate],
    effect_id: i64,
    source_config_id: i64,
) -> Result<(), Box<dyn std::error::Error>> {
    let present = relationships.iter().any(|candidate| {
        let has_effect = candidate.runtime_buff_ids.contains(&effect_id)
            || candidate.source_buff_ids.contains(&effect_id);
        let has_source = candidate.source_config_ids.contains(&source_config_id)
            || (effect_id == source_config_id && candidate.source_buff_ids.contains(&effect_id));
        has_effect && has_source
    });
    if present {
        Ok(())
    } else {
        Err(
            format!("current exact relationship {effect_id} <- {source_config_id} is missing")
                .into(),
        )
    }
}

fn require_exact_relationship_target(
    relationships: &[ExactRelationshipCandidate],
    effect_id: i64,
    target_damage_id: i64,
) -> Result<(), Box<dyn std::error::Error>> {
    let present = relationships.iter().any(|candidate| {
        (candidate.runtime_buff_ids.contains(&effect_id)
            || candidate.source_buff_ids.contains(&effect_id))
            && candidate.target_damage_ids.contains(&target_damage_id)
    });
    if present {
        Ok(())
    } else {
        Err(format!(
            "current exact relationship {effect_id} -> damage {target_damage_id} is missing"
        )
        .into())
    }
}

fn require_historical_relation(
    relationships: &[ExactRelationshipCandidate],
    effect_id: i64,
    source_config_id: i64,
) -> Result<(), Box<dyn std::error::Error>> {
    let present = relationships.iter().any(|candidate| {
        candidate.historical_relations.iter().any(|relation| {
            relation.effect_id == effect_id
                && relation.source_type_id == 1
                && relation.source_config_id == source_config_id
                && relation.observation_count > 0
        })
    });
    if present {
        Ok(())
    } else {
        Err(
            format!("historical packet relationship {effect_id} <- {source_config_id} is missing")
                .into(),
        )
    }
}

fn require_buff_identity(
    buffs_by_id: &BTreeMap<i64, &Value>,
    buff_id: i64,
    expected_design_name: &str,
    expected_duration_seconds: Option<f64>,
) -> Result<(), Box<dyn std::error::Error>> {
    let buff = buffs_by_id
        .get(&buff_id)
        .copied()
        .ok_or_else(|| format!("current BuffTable row {buff_id} is missing"))?;
    if integer(buff, "Id") != Some(buff_id)
        || string_ref(buff, "NameDesign") != Some(expected_design_name)
    {
        return Err(format!("current BuffTable identity {buff_id} changed").into());
    }
    if let Some(expected_duration_seconds) = expected_duration_seconds {
        let actual_duration = buff
            .get("DestroyParam")
            .and_then(Value::as_array)
            .and_then(|groups| groups.first())
            .and_then(Value::as_array)
            .and_then(|values| values.last())
            .and_then(Value::as_f64);
        if actual_duration != Some(expected_duration_seconds) {
            return Err(format!(
                "current BuffTable duration {buff_id} changed from {expected_duration_seconds}s"
            )
            .into());
        }
    }
    Ok(())
}

fn require_buff_shape(
    buffs_by_id: &BTreeMap<i64, &Value>,
    buff_id: i64,
    expected_design_name: &str,
    expected_buff_type: i64,
    expected_visible: i64,
    expected_repeat_add_rule: &[i64],
    expected_duration_seconds: Option<f64>,
) -> Result<(), Box<dyn std::error::Error>> {
    require_buff_identity(
        buffs_by_id,
        buff_id,
        expected_design_name,
        expected_duration_seconds,
    )?;
    let buff = buffs_by_id
        .get(&buff_id)
        .copied()
        .ok_or_else(|| format!("current BuffTable row {buff_id} is missing"))?;
    let repeat_add_rule = integer_array(buff, "RepeatAddRule");
    if integer(buff, "BuffType") != Some(expected_buff_type)
        || integer(buff, "Visible") != Some(expected_visible)
        || repeat_add_rule != expected_repeat_add_rule
    {
        return Err(format!("current BuffTable runtime shape {buff_id} changed").into());
    }
    if expected_duration_seconds.is_none() && buff_duration_seconds(buff).is_some() {
        return Err(
            format!("current BuffTable duration {buff_id} unexpectedly became timed").into(),
        );
    }
    Ok(())
}

fn require_buff_output_route(
    buffs_by_id: &BTreeMap<i64, &Value>,
    damage_attrs_by_id: &BTreeMap<i64, &Value>,
    buff_id: i64,
    expected_design_name: &str,
    expected_duration_seconds: f64,
    output_id: i64,
    expected_damage_script: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    require_buff_identity(
        buffs_by_id,
        buff_id,
        expected_design_name,
        Some(expected_duration_seconds),
    )?;
    let output = damage_attrs_by_id
        .get(&output_id)
        .copied()
        .ok_or_else(|| format!("current DamageAttrTable output {output_id} is missing"))?;
    if integer(output, "Id") != Some(output_id)
        || integer(output, "TypeEnum") != Some(buff_id)
        || string_ref(output, "DamageScript") != Some(expected_damage_script)
    {
        return Err(format!(
            "current buff {buff_id} output {output_id} no longer uses {expected_damage_script}"
        )
        .into());
    }
    Ok(())
}

fn require_emitted_damage_ids(
    skill_id: i64,
    damage_chains: &[ExactDamageChainCandidate],
    required_ids: &[i64],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut available = BTreeSet::new();
    for candidate in damage_chains {
        available.extend(candidate.resolved_damage_ids.iter().copied());
        available.extend(candidate.source_target_damage_ids.iter().copied());
        for chain in &candidate.damage_chains {
            if let Some(damage_id) = integer(chain, "damageId") {
                available.insert(damage_id);
            }
            if let Some(parents) = chain.get("recountParents").and_then(Value::as_array) {
                for parent in parents {
                    if let Some(ids) = parent.get("damageIds").and_then(Value::as_array) {
                        available.extend(ids.iter().filter_map(Value::as_i64));
                    }
                }
            }
        }
    }

    let missing = required_ids
        .iter()
        .copied()
        .filter(|damage_id| !available.contains(damage_id))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Aoyi skill {skill_id} current emitted damage or healing IDs are missing: {missing:?}"
        )
        .into())
    }
}

fn require_source_target_damage_ids(
    skill_id: i64,
    damage_chains: &[ExactDamageChainCandidate],
    damage_ids: &[i64],
) -> Result<(), Box<dyn std::error::Error>> {
    let available = damage_chains
        .iter()
        .flat_map(|candidate| candidate.source_target_damage_ids.iter().copied())
        .collect::<BTreeSet<_>>();
    let missing = damage_ids
        .iter()
        .copied()
        .filter(|damage_id| !available.contains(damage_id))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Aoyi skill {skill_id} is missing exact buff-source target damage IDs {missing:?}"
        )
        .into())
    }
}

fn exact_damage_chain_candidates(
    skill_effects_by_id: &BTreeMap<i64, &Value>,
    damage_attrs_by_id: &BTreeMap<i64, &Value>,
    damage_chain_bridge: &Value,
    effect_sources: &Value,
    skill_id: i64,
) -> Vec<ExactDamageChainCandidate> {
    let Some(chains_by_damage_id) = damage_chain_bridge
        .get("damageChains")
        .and_then(Value::as_object)
    else {
        return Vec::new();
    };

    let damage_to_source_ids = effect_sources
        .get("damageIdToEffectSourceIds")
        .and_then(Value::as_object);
    let sources_by_id = effect_sources
        .get("effectSourcesById")
        .and_then(Value::as_object);
    let mut candidates = Vec::new();
    for effect in skill_effects_by_id.values().copied() {
        if integer(effect, "SkillId") != Some(skill_id) {
            continue;
        }
        let Some(skill_effect_id) = integer(effect, "Id") else {
            continue;
        };
        let damage_ids = effect_damage_merge_ids(effect);
        if damage_ids.is_empty() {
            continue;
        }

        let mut resolved_damage_ids = Vec::new();
        let mut missing_damage_ids = Vec::new();
        let mut damage_chains = Vec::new();
        let mut exact_effect_source_ids = BTreeSet::new();
        for damage_id in &damage_ids {
            let damage_key = damage_id.to_string();
            match chains_by_damage_id.get(&damage_key) {
                Some(chain) => {
                    resolved_damage_ids.push(*damage_id);
                    damage_chains.push(chain.clone());
                }
                None => missing_damage_ids.push(*damage_id),
            }
            if let Some(source_ids) = damage_to_source_ids
                .and_then(|index| index.get(&damage_key))
                .and_then(Value::as_array)
            {
                exact_effect_source_ids.extend(
                    source_ids
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string),
                );
            }
        }
        let exact_effect_source_ids = exact_effect_source_ids.into_iter().collect::<Vec<_>>();
        let exact_effect_sources = exact_effect_source_ids
            .iter()
            .filter_map(|source_id| {
                sources_by_id
                    .and_then(|index| index.get(source_id))
                    .cloned()
            })
            .collect::<Vec<_>>();
        let (damage_attr_rows, missing_damage_attr_ids) =
            resolve_damage_attr_rows(&damage_ids, damage_attrs_by_id);
        let source_target_damage_ids = exact_effect_sources
            .iter()
            .flat_map(|source| {
                source
                    .get("targets")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
            })
            .filter(|target| string_ref(target, "targetKind") == Some("damage"))
            .filter_map(|target| integer(target, "damageId"))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let (source_target_damage_attr_rows, missing_source_target_damage_attr_ids) =
            resolve_damage_attr_rows(&source_target_damage_ids, damage_attrs_by_id);
        candidates.push(ExactDamageChainCandidate {
            skill_effect_id,
            relationship_source:
                "current-build SkillEffectTable damageMerge reference joined by exact damage ID",
            damage_ids,
            resolved_damage_ids,
            missing_damage_ids,
            exact_effect_source_ids,
            exact_effect_sources,
            damage_chains,
            damage_attr_rows,
            missing_damage_attr_ids,
            source_target_damage_ids,
            source_target_damage_attr_rows,
            missing_source_target_damage_attr_ids,
        });
    }
    candidates.sort_by_key(|candidate| candidate.skill_effect_id);
    candidates
}

fn resolve_damage_attr_rows(
    damage_ids: &[i64],
    damage_attrs_by_id: &BTreeMap<i64, &Value>,
) -> (Vec<Value>, Vec<i64>) {
    let mut rows = Vec::new();
    let mut missing = Vec::new();
    for damage_id in damage_ids {
        match damage_attrs_by_id.get(damage_id) {
            Some(row) => rows.push((*row).clone()),
            None => missing.push(*damage_id),
        }
    }
    (rows, missing)
}

fn effect_damage_merge_ids(effect: &Value) -> Vec<i64> {
    let mut ids = BTreeSet::new();
    let Some(descriptions) = effect.get("SkillAttrDes").and_then(Value::as_array) else {
        return Vec::new();
    };
    for description in descriptions {
        let Some(parts) = description.as_array() else {
            continue;
        };
        for text in parts.iter().filter_map(Value::as_str) {
            let mut remainder = text;
            while let Some(marker) = remainder.find("damageMerge({") {
                let after_marker = &remainder[marker + "damageMerge({".len()..];
                let Some(end) = after_marker.find('}') else {
                    break;
                };
                for token in after_marker[..end].split(',') {
                    if let Ok(id) = token.trim().parse::<i64>() {
                        ids.insert(id);
                    }
                }
                remainder = &after_marker[end + 1..];
            }
        }
    }
    ids.into_iter().collect()
}

fn classify_recipients(description: &str) -> RecipientEvidence {
    let lower = description.to_ascii_lowercase();
    let external_phrases = [
        "you and your teammates",
        "yourself and up to",
        "yourself and 10 allies",
        "you and allies",
        "entire team",
        "nearby allies",
        "surrounding allies",
        "party members",
        "target's armor",
        "applying vulnerability",
        "reducing element resistance",
    ];
    let matched = external_phrases
        .iter()
        .filter(|phrase| lower.contains(**phrase))
        .map(|phrase| (*phrase).to_string())
        .collect::<Vec<_>>();
    let self_only = matched.is_empty()
        && ["your ", "yourself", "when you deal", "when you take"]
            .iter()
            .any(|phrase| lower.contains(phrase));
    RecipientEvidence {
        state: if !matched.is_empty() {
            "external-recipient-described"
        } else if self_only {
            "self-only-described"
        } else {
            "recipient-unresolved"
        },
        matched_phrases: matched,
        source: "current-build SkillTable localized description",
    }
}

fn classify_candidate_classes(description: &str) -> Vec<String> {
    let lower = description.to_ascii_lowercase();
    let external = [
        "you and your teammates",
        "yourself and up to",
        "yourself and 10 allies",
        "you and allies",
        "entire team",
        "nearby allies",
        "surrounding allies",
        "party members",
    ]
    .iter()
    .any(|phrase| lower.contains(phrase));
    let mut classes = BTreeSet::new();
    if external
        && [
            "increased atk",
            "atk, attack spd",
            "main stats",
            "all-element bonus",
        ]
        .iter()
        .any(|phrase| lower.contains(phrase))
    {
        classes.insert("external-offense-stat".to_string());
    }
    if lower.contains("target's armor")
        || lower.contains("applying vulnerability")
        || lower.contains("reducing element resistance")
    {
        classes.insert("external-target-mitigation".to_string());
    }
    if external && (lower.contains("skill cds") || lower.contains("cooldown")) {
        classes.insert("external-action-opportunity".to_string());
    }
    if external && lower.contains("generate thunderstrike") {
        classes.insert("external-produced-damage".to_string());
    }
    if external
        && (lower.contains("shield")
            || lower.contains("damage reduction")
            || lower.contains("cannot be defeated"))
    {
        classes.insert("external-defense".to_string());
    }
    if external && (lower.contains("healing") || lower.contains("revive")) {
        classes.insert("external-healing".to_string());
    }
    if !external
        && [
            "your crit",
            "your luck",
            "your block",
            "your armor",
            "your healing received",
            "increases versatility",
            "granting yourself",
        ]
        .iter()
        .any(|phrase| lower.contains(phrase))
    {
        classes.insert("self-only-offense".to_string());
    }
    classes.into_iter().collect()
}

fn passive_owner_ids(row: &Value, buffs_by_id: &BTreeMap<i64, &Value>) -> Vec<i64> {
    let mut ids = BTreeSet::new();
    if let Some(transformations) = row.get("TransformationType").and_then(Value::as_array) {
        for transformation in transformations {
            let Some(parts) = transformation.as_array() else {
                continue;
            };
            if parts.first().and_then(Value::as_i64) == Some(3) {
                if let Some(id) = parts.get(1).and_then(Value::as_i64) {
                    if buffs_by_id.contains_key(&id) {
                        ids.insert(id);
                    }
                }
            }
        }
    }
    ids.into_iter().collect()
}

fn direct_attribute_transformation_evidence(
    row: &Value,
    skill_id: i64,
    fight_attr_rows: &[&Value],
    skill_aoyi_star_rows: &[&Value],
    remodel_consumer_proof: &RemodelConsumerProof,
) -> Result<Vec<DirectAttributeTransformationEvidence>, Box<dyn std::error::Error>> {
    let Some(transformations) = row.get("TransformationType").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    let mut evidence = Vec::new();
    for transformation in transformations {
        let Some(parts) = transformation.as_array() else {
            continue;
        };
        if parts.first().and_then(Value::as_i64)
            != Some(remodel_consumer_proof.remodel_info_type.attribute)
        {
            continue;
        }
        let transformed_attribute_id = parts
            .get(1)
            .and_then(Value::as_i64)
            .ok_or("kind-1 transformation has no attribute ID")?;
        let base_raw_value = parts
            .get(2)
            .and_then(Value::as_i64)
            .ok_or("kind-1 transformation has no raw value")?;
        let (base_row, attribute_component) = fight_attribute_component(
            transformed_attribute_id,
            fight_attr_rows,
        )
        .ok_or_else(|| {
            format!(
                "skill {skill_id} kind-1 transformation attribute {transformed_attribute_id} has no FightAttrTable component owner"
            )
        })?;
        let base_attribute_id =
            integer(base_row, "Id").ok_or("FightAttrTable component owner row has no Id")?;
        let official_name = string(base_row, "OfficialName")
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| format!("attribute {base_attribute_id}"));

        let mut tier_raw_values = skill_aoyi_star_rows
            .iter()
            .filter(|star| integer(star, "SkillId") == Some(skill_id))
            .filter_map(|star| {
                let tier = integer(star, "Level")?;
                let row_id = integer(star, "Id")?;
                star.get("TransformationType")
                    .and_then(Value::as_array)?
                    .iter()
                    .filter_map(Value::as_array)
                    .find_map(|parts| {
                        (parts.first().and_then(Value::as_i64) == Some(1)
                            && parts.get(1).and_then(Value::as_i64)
                                == Some(transformed_attribute_id))
                        .then(|| parts.get(2).and_then(Value::as_i64))
                        .flatten()
                    })
                    .map(|raw_value| DirectAttributeTierValue {
                        tier,
                        row_id,
                        raw_value,
                    })
            })
            .collect::<Vec<_>>();
        tier_raw_values.sort_by_key(|value| value.tier);

        evidence.push(DirectAttributeTransformationEvidence {
            transformation_kind: remodel_consumer_proof.remodel_info_type.attribute,
            transformed_attribute_id,
            base_attribute_id,
            official_name,
            attribute_component,
            attr_num_type: integer(base_row, "AttrNumType"),
            base_raw_value,
            tier_raw_values,
            recipient_scope: "aoyi-owner-only",
            rdps_disposition: "ordinary-owner-damage-never-transferred",
            value_interpretation: match attribute_component {
                "percentage" | "extra-percentage" => {
                    "signed-fixed-point-percent-100-raw-units-per-percent"
                }
                _ => "raw-additive-attribute-units-no-percent-coercion",
            },
            consumer_proof: format!(
                "build {} proof schema {}: enum_define.RemodelInfoType.Attr={} and weapon_skill_vm.ParseResonanceTransformation forwards (attribute_id, raw_value) to fight_attr_parse_vm.ParseFightAttrTips",
                remodel_consumer_proof.game_build,
                remodel_consumer_proof.schema_version,
                remodel_consumer_proof.remodel_info_type.attribute,
            ),
            proof_state: format!(
                "{}+fight-attribute-component-and-tier-rows-exact",
                remodel_consumer_proof.proof_state
            ),
            runtime_authority: false,
        });
    }
    evidence.sort_by_key(|entry| entry.transformed_attribute_id);
    Ok(evidence)
}

fn fight_attribute_component<'a>(
    transformed_attribute_id: i64,
    fight_attr_rows: &[&'a Value],
) -> Option<(&'a Value, &'static str)> {
    const COMPONENTS: [(&str, &str); 6] = [
        ("AttrFinal", "final"),
        ("AttrTotal", "total"),
        ("AttrAdd", "additive"),
        ("AttrExAdd", "extra-additive"),
        ("AttrPer", "percentage"),
        ("AttrExPer", "extra-percentage"),
    ];
    fight_attr_rows.iter().find_map(|row| {
        COMPONENTS.iter().find_map(|(field, component)| {
            (integer(row, field) == Some(transformed_attribute_id)).then_some((*row, *component))
        })
    })
}

fn passive_parameter_evidence(
    row: &Value,
    icon: Option<&Value>,
    attr_descriptions_by_id: &BTreeMap<i64, &Value>,
) -> Result<Vec<PassiveParameterEvidence>, Box<dyn std::error::Error>> {
    let mut evidence = Vec::new();
    let Some(transformations) = row.get("TransformationType").and_then(Value::as_array) else {
        return Ok(evidence);
    };
    let parameter_sets = row
        .get("BuffPar")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    for transformation in transformations {
        let Some(parts) = transformation.as_array() else {
            continue;
        };
        if parts.first().and_then(Value::as_i64) != Some(3) {
            continue;
        }
        let Some(attribute_id) = parts.get(1).and_then(Value::as_i64) else {
            continue;
        };
        let parameter_set_index = parts
            .get(2)
            .and_then(Value::as_i64)
            .filter(|index| *index > 0)
            .ok_or("kind-3 transformation has no positive parameter-set index")?
            as usize;
        let Some(description) = attr_descriptions_by_id
            .get(&attribute_id)
            .and_then(|value| string(value, "Description"))
        else {
            continue;
        };
        let referenced_lanes = unmarkpercent_lanes(&description);
        if referenced_lanes.is_empty() {
            continue;
        }
        let raw_base = parameter_sets
            .get(parameter_set_index - 1)
            .and_then(Value::as_array)
            .ok_or_else(|| {
                format!(
                    "attribute {attribute_id} references missing BuffPar set {parameter_set_index}"
                )
            })?;
        let lane_roles = referenced_lanes
            .iter()
            .map(|lane| passive_lane_role(attribute_id, *lane))
            .collect::<Vec<_>>();
        let base_lanes = parameter_lanes(raw_base, &referenced_lanes, &lane_roles)?;

        let mut tier_lanes = Vec::new();
        if let Some(records) = icon
            .and_then(|value| value.get("parameterRecords"))
            .and_then(Value::as_array)
        {
            for record in records {
                if integer(record, "buffId") != Some(attribute_id)
                    || integer(record, "parameterSetIndex") != Some(parameter_set_index as i64)
                {
                    continue;
                }
                let tier = integer(record, "tier").unwrap_or_default();
                if tier <= 0 {
                    continue;
                }
                let mut values = BTreeMap::new();
                if let Some(parameters) = record.get("parameterValues").and_then(Value::as_array) {
                    for parameter in parameters {
                        if let (Some(index), Some(raw)) = (
                            integer(parameter, "parameterIndex"),
                            integer(parameter, "rawValue"),
                        ) {
                            values.insert(index as usize, raw);
                        }
                    }
                }
                let lanes = referenced_lanes
                    .iter()
                    .zip(&lane_roles)
                    .map(|(lane, role)| {
                        values
                            .get(lane)
                            .copied()
                            .map(|raw| parameter_lane(*lane, role.clone(), raw))
                            .ok_or_else(|| {
                                format!(
                                    "attribute {attribute_id} tier {tier} is missing parameter lane {lane}"
                                )
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                tier_lanes.push(TierParameterLanes { tier, lanes });
            }
        }
        tier_lanes.sort_by_key(|tier| tier.tier);

        evidence.push(PassiveParameterEvidence {
            transformed_attribute_id: attribute_id,
            description_template: description,
            parameter_encoding: "signed-fixed-point-percent",
            raw_units_per_percent: 100,
            raw_units_per_decimal: 10_000,
            lane_roles,
            base_lanes,
            tier_lanes,
            proof_state: "current-build-transform-parameter-set-and-description-consumer-exact",
            runtime_authority: false,
        });
    }
    evidence.sort_by_key(|entry| entry.transformed_attribute_id);
    Ok(evidence)
}

fn active_modifier_parameter_evidence(
    skill_id: i64,
    icon: Option<&Value>,
    skills_by_id: &BTreeMap<i64, &Value>,
    skill_effects_by_id: &BTreeMap<i64, &Value>,
    buffs_by_id: &BTreeMap<i64, &Value>,
    component_routes: &[ComponentRoute],
    semantic_owner_candidates: &[SemanticOwnerCandidate],
) -> Result<Vec<ActiveModifierParameterEvidence>, Box<dyn std::error::Error>> {
    let modifier_routes = component_routes
        .iter()
        .filter(|route| active_tier_parameter_route(route))
        .collect::<Vec<_>>();
    let active_effect_ids = modifier_routes
        .iter()
        .flat_map(|route| route.effect_ids.iter().copied())
        .chain(
            semantic_owner_candidates
                .iter()
                .map(|candidate| candidate.effect_id),
        )
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if active_effect_ids.is_empty() && modifier_routes.is_empty() {
        return Ok(Vec::new());
    }

    let semantic_skill_effect_ids = semantic_owner_candidates
        .iter()
        .filter_map(|candidate| candidate.source_subskill_effect_id)
        .filter(|effect_id| skill_effects_by_id.contains_key(effect_id))
        .collect::<BTreeSet<_>>();
    let skill_effect_ids = skills_by_id
        .get(&skill_id)
        .map(|row| integer_array(row, "EffectIDs"))
        .unwrap_or_default();
    let direct_skill_effect_id = skill_effect_ids.iter().copied().find(|effect_id| {
        skill_effects_by_id
            .get(effect_id)
            .is_some_and(|effect| integer(effect, "SkillId") == Some(skill_id))
    });
    let mut exact_owner_fallback = skill_effects_by_id
        .iter()
        .filter_map(|(effect_id, effect)| {
            (integer(effect, "SkillId") == Some(skill_id)).then_some(*effect_id)
        })
        .collect::<Vec<_>>();
    exact_owner_fallback.sort_unstable();
    exact_owner_fallback.dedup();
    let semantic_skill_effect_id = (semantic_skill_effect_ids.len() == 1).then(|| {
        *semantic_skill_effect_ids
            .iter()
            .next()
            .expect("one semantic skill effect")
    });
    let skill_effect_id = semantic_skill_effect_id
        .or(direct_skill_effect_id)
        .or_else(|| (exact_owner_fallback.len() == 1).then(|| exact_owner_fallback[0]));
    let Some(skill_effect_id) = skill_effect_id else {
        return Ok(Vec::new());
    };
    let Some(skill_effect) = skill_effects_by_id.get(&skill_effect_id).copied() else {
        return Ok(Vec::new());
    };
    let semantic_labels = modifier_semantic_labels(skill_effect);
    let Some(tier_effects) = icon
        .and_then(|value| value.get("TierEffects"))
        .and_then(Value::as_array)
    else {
        return Ok(Vec::new());
    };
    if semantic_labels.is_empty() || tier_effects.is_empty() {
        return Ok(Vec::new());
    }

    let mut tiers = Vec::new();
    for tier_effect in tier_effects {
        let tier = integer(tier_effect, "tier").unwrap_or_default();
        if tier <= 0 {
            continue;
        }
        let Some(values) = tier_effect.get("values").and_then(Value::as_array) else {
            continue;
        };
        let keys = values
            .iter()
            .filter_map(|value| string_ref(value, "key"))
            .collect::<BTreeSet<_>>();
        let grammar = active_parameter_grammar(&semantic_labels, &keys);
        let Some(grammar) = grammar else {
            continue;
        };

        let mut fields = Vec::new();
        for value in values {
            let Some(key) = string(value, "key") else {
                continue;
            };
            let raw_value = integer(value, "rawValue").ok_or_else(|| {
                format!("skill {skill_id} tier {tier} field {key} has no integer rawValue")
            })?;
            fields.push(active_modifier_field(
                &key,
                raw_value,
                &semantic_labels,
                grammar,
            )?);
        }
        tiers.push(ActiveModifierTier { tier, fields });
    }
    if tiers.is_empty() {
        return Ok(Vec::new());
    }
    tiers.sort_by_key(|tier| tier.tier);

    let duration_seconds = active_effect_ids
        .iter()
        .filter_map(|effect_id| buffs_by_id.get(effect_id).copied())
        .filter_map(buff_duration_seconds)
        .reduce(f64::max);
    Ok(vec![ActiveModifierParameterEvidence {
        skill_effect_id,
        active_effect_ids,
        recipient_scopes: modifier_routes
            .iter()
            .map(|route| route.recipient_scope)
            .chain(
                semantic_owner_candidates
                    .iter()
                    .map(|candidate| candidate.recipient_scope),
            )
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        rdps_dispositions: modifier_routes
            .iter()
            .map(|route| route.rdps_disposition)
            .chain(
                semantic_owner_candidates
                    .iter()
                    .map(|candidate| candidate.rdps_disposition),
            )
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        semantic_labels,
        parameter_encoding: "signed-fixed-point-percent",
        raw_units_per_percent: 100,
        raw_units_per_decimal: 10_000,
        duration_seconds,
        tiers,
        proof_state: "current-build-skill-effect-label-order-and-tier-key-family-exact",
        runtime_authority: false,
    }])
}

fn active_tier_parameter_route(route: &ComponentRoute) -> bool {
    route.component_id == "self-only-active-modifier"
        || matches!(
            route.role,
            "self-only-modifier"
                | "owner-only-critical-rate-and-critical-damage-modifier"
                | "transferable-external-action-opportunity"
                | "transferable-external-derived-attack-and-haste-modifier"
                | "transferable-external-healing-modifier"
                | "transferable-external-modifier"
                | "transferable-external-target-mitigation"
                | "transferable-party-shield-with-tier-scalar"
                | "mixed-defensive-attack-reduction-and-external-target-vulnerability"
                | "external-produced-shield"
        )
}

#[derive(Clone, Copy)]
enum ActiveParameterGrammar {
    SinglePercent,
    SingleNamed,
    UnnumberedPair,
    NumberedPairs,
    DirectOrdered,
    ShieldThenDirectOrdered,
    RorolaPersonalTriplet,
}

fn active_parameter_grammar(
    semantic_labels: &[String],
    keys: &BTreeSet<&str>,
) -> Option<ActiveParameterGrammar> {
    if semantic_labels
        == [
            "DMG Boost".to_owned(),
            "Extra DMG Boost".to_owned(),
            "Life Steal Ratio".to_owned(),
        ]
        && keys == &BTreeSet::from(["attrPer", "attrPerElse", "hpPer"])
    {
        return Some(ActiveParameterGrammar::RorolaPersonalTriplet);
    }
    if semantic_labels.len() == 1 && keys.len() == 1 && keys.contains("attrPer") {
        return Some(ActiveParameterGrammar::SinglePercent);
    }
    if semantic_labels.len() == 1
        && keys.len() == 1
        && (keys.contains("shield") || keys.contains("shieldHp"))
    {
        return Some(ActiveParameterGrammar::SingleNamed);
    }
    const UNNUMBERED: [&str; 4] = ["attrPer", "attrAdd", "attrLv", "attrMax"];
    if semantic_labels.len() == 2
        && keys.iter().all(|key| UNNUMBERED.contains(key))
        && keys.contains("attrPer")
        && keys.contains("attrAdd")
    {
        return Some(ActiveParameterGrammar::UnnumberedPair);
    }
    if semantic_labels.len() >= 2
        && semantic_labels.len() % 2 == 0
        && semantic_labels.iter().all(|label| {
            let lower = label.to_ascii_lowercase();
            lower.contains("boost") || lower.contains("bonus")
        })
        && keys.iter().all(|key| numbered_active_parameter_key(key))
        && keys.iter().any(|key| key.starts_with("attrPer"))
        && keys
            .iter()
            .any(|key| numbered_suffix(key, "attr").is_some())
    {
        return Some(ActiveParameterGrammar::NumberedPairs);
    }
    let direct_indices = keys
        .iter()
        .filter_map(|key| direct_ordered_active_parameter_index(key))
        .collect::<BTreeSet<_>>();
    if semantic_labels.len() == keys.len()
        && direct_indices.len() == keys.len()
        && direct_indices == (0..semantic_labels.len()).collect::<BTreeSet<_>>()
    {
        return Some(ActiveParameterGrammar::DirectOrdered);
    }
    if semantic_labels.len() == keys.len()
        && keys.contains("shieldHp")
        && direct_indices.len() + 1 == keys.len()
        && direct_indices == (0..semantic_labels.len() - 1).collect::<BTreeSet<_>>()
    {
        return Some(ActiveParameterGrammar::ShieldThenDirectOrdered);
    }
    None
}

fn active_modifier_field(
    key: &str,
    raw_value: i64,
    semantic_labels: &[String],
    grammar: ActiveParameterGrammar,
) -> Result<ActiveModifierField, Box<dyn std::error::Error>> {
    let (semantic_role, contribution_role, alias_of) = match grammar {
        ActiveParameterGrammar::SinglePercent => {
            (semantic_labels[0].clone(), "active-stat-modifier", None)
        }
        ActiveParameterGrammar::SingleNamed => (
            semantic_labels[0].clone(),
            "active-tier-parameter-not-total-magnitude",
            None,
        ),
        ActiveParameterGrammar::UnnumberedPair => match key {
            "attrPer" => (semantic_labels[0].clone(), "active-stat-modifier", None),
            "attrAdd" => (semantic_labels[1].clone(), "active-stat-modifier", None),
            "attrMax" => (
                semantic_labels[1].clone(),
                "mirrored-ui-cap-alias-do-not-double-count",
                Some("attrAdd".to_owned()),
            ),
            "attrLv" => (
                "level scaling".to_owned(),
                "auxiliary-zero-lane-not-a-separate-modifier",
                None,
            ),
            _ => return Err(format!("unsupported unnumbered active parameter key {key}").into()),
        },
        ActiveParameterGrammar::NumberedPairs => {
            if let Some(index) = numbered_suffix(key, "attrPer") {
                let label_index = (index - 1) * 2 + 1;
                (
                    semantic_labels
                        .get(label_index)
                        .ok_or("numbered attrPer key exceeds semantic label pairs")?
                        .clone(),
                    "active-stat-modifier",
                    None,
                )
            } else if let Some(index) = numbered_suffix(key, "attrMax") {
                let label_index = (index - 1) * 2;
                (
                    semantic_labels
                        .get(label_index)
                        .ok_or("numbered attrMax key exceeds semantic label pairs")?
                        .clone(),
                    "mirrored-ui-cap-alias-do-not-double-count",
                    Some(format!("attr{index}")),
                )
            } else if let Some(index) = numbered_suffix(key, "attrLv") {
                (
                    format!("tier {index} level scaling"),
                    "auxiliary-zero-lane-not-a-separate-modifier",
                    None,
                )
            } else if let Some(index) = numbered_suffix(key, "attr") {
                let label_index = (index - 1) * 2;
                (
                    semantic_labels
                        .get(label_index)
                        .ok_or("numbered attr key exceeds semantic label pairs")?
                        .clone(),
                    "active-stat-modifier",
                    None,
                )
            } else {
                return Err(format!("unsupported numbered active parameter key {key}").into());
            }
        }
        ActiveParameterGrammar::DirectOrdered => {
            let index = direct_ordered_active_parameter_index(key)
                .ok_or_else(|| format!("unsupported direct active parameter key {key}"))?;
            (
                semantic_labels
                    .get(index)
                    .ok_or("direct active parameter key exceeds semantic labels")?
                    .clone(),
                "active-tier-parameter-not-total-magnitude",
                None,
            )
        }
        ActiveParameterGrammar::ShieldThenDirectOrdered => {
            let index = if key == "shieldHp" {
                0
            } else {
                direct_ordered_active_parameter_index(key)
                    .ok_or_else(|| format!("unsupported mixed active parameter key {key}"))?
                    + 1
            };
            (
                semantic_labels
                    .get(index)
                    .ok_or("mixed active parameter key exceeds semantic labels")?
                    .clone(),
                "active-tier-parameter-not-total-magnitude",
                None,
            )
        }
        ActiveParameterGrammar::RorolaPersonalTriplet => match key {
            "attrPer" => (
                semantic_labels[0].clone(),
                "active-personal-base-damage-boost",
                None,
            ),
            "attrPerElse" => (
                semantic_labels[1].clone(),
                "active-personal-stacking-damage-boost-per-ten-hits",
                None,
            ),
            "hpPer" => (
                semantic_labels[2].clone(),
                "active-personal-life-steal-ratio",
                None,
            ),
            _ => return Err(format!("unsupported Rorola active parameter key {key}").into()),
        },
    };
    Ok(ActiveModifierField {
        key: key.to_owned(),
        semantic_role,
        contribution_role,
        alias_of,
        raw_value,
        percent_value: raw_value as f64 / 100.0,
        decimal_value: raw_value as f64 / 10_000.0,
        mapping_proof: "current SkillEffectTable ordered semantic labels joined to exact SkillAoyiStarTable field grammar",
    })
}

fn modifier_semantic_labels(skill_effect: &Value) -> Vec<String> {
    skill_effect
        .get("SkillAttrDes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_array)
        .filter_map(|parts| {
            let label = parts.first().and_then(Value::as_str)?;
            let description = parts.get(1).and_then(Value::as_str).unwrap_or_default();
            let lower = label.to_ascii_lowercase();
            let description_lower = description.to_ascii_lowercase();
            (!description_lower.contains("damagemerge")
                && !lower.contains("total dmg")
                && !lower.contains("duration")
                && !lower.contains("cooldown")
                && lower.trim() != "cd")
                .then_some(label)
        })
        .map(str::to_owned)
        .collect()
}

fn numbered_active_parameter_key(key: &str) -> bool {
    ["attrPer", "attrMax", "attrLv", "attr"]
        .iter()
        .any(|prefix| numbered_suffix(key, prefix).is_some())
}

fn direct_ordered_active_parameter_index(key: &str) -> Option<usize> {
    let suffix = key.strip_prefix("attr")?;
    let mut characters = suffix.chars();
    let character = characters.next()?;
    if characters.next().is_some() || !character.is_ascii_uppercase() {
        return None;
    }
    Some((character as u8 - b'A') as usize)
}

fn numbered_suffix(key: &str, prefix: &str) -> Option<usize> {
    key.strip_prefix(prefix)?
        .parse::<usize>()
        .ok()
        .filter(|value| *value > 0)
}

fn buff_duration_seconds(buff: &Value) -> Option<f64> {
    buff.get("DestroyParam")?
        .as_array()?
        .iter()
        .filter_map(Value::as_array)
        .flat_map(|values| values.iter())
        .filter_map(Value::as_f64)
        .filter(|value| *value > 0.0)
        .reduce(f64::max)
}

fn unmarkpercent_lanes(description: &str) -> Vec<usize> {
    const PREFIX: &str = "Decision.unmarkpercent(";
    let mut lanes = BTreeSet::new();
    let mut remainder = description;
    while let Some(offset) = remainder.find(PREFIX) {
        remainder = &remainder[offset + PREFIX.len()..];
        let digits = remainder
            .chars()
            .take_while(|character| character.is_ascii_digit())
            .collect::<String>();
        if let Ok(lane) = digits.parse::<usize>() {
            if lane > 0 {
                lanes.insert(lane);
            }
        }
    }
    lanes.into_iter().collect()
}

fn passive_lane_role(attribute_id: i64, lane: usize) -> String {
    match (attribute_id, lane) {
        (3_200_009, 1) => "lucky-effect-attack-damage-percent".to_owned(),
        (3_210_020, 1) => "lucky-effect-magic-damage-percent".to_owned(),
        (3_210_080, 1) => "lucky-effect-magic-damage-percent".to_owned(),
        (3_200_035, 1) => "crit-damage-min-percent".to_owned(),
        (3_200_035, 2) => "crit-damage-max-percent".to_owned(),
        _ => format!("decision-unmarkpercent-parameter-{lane}"),
    }
}

fn parameter_lanes(
    raw_values: &[Value],
    referenced_lanes: &[usize],
    lane_roles: &[String],
) -> Result<Vec<ParameterLane>, Box<dyn std::error::Error>> {
    referenced_lanes
        .iter()
        .zip(lane_roles)
        .map(|(lane, role)| {
            raw_values
                .get(*lane - 1)
                .and_then(Value::as_i64)
                .map(|raw| parameter_lane(*lane, role.clone(), raw))
                .ok_or_else(|| format!("BuffPar is missing integer lane {lane}").into())
        })
        .collect()
}

fn parameter_lane(lane: usize, role: String, raw_value: i64) -> ParameterLane {
    ParameterLane {
        lane,
        role,
        raw_value,
        percent_value: raw_value as f64 / 100.0,
        decimal_value: raw_value as f64 / 10_000.0,
    }
}

fn owner_stem(value: &str) -> Option<String> {
    let normalized = normalized_owner_name(value);
    for suffix in ["被动buff", "被动", "天生buff", "天生"] {
        if let Some(stem) = normalized.strip_suffix(suffix) {
            if !stem.is_empty() {
                return Some(stem.to_string());
            }
        }
    }
    None
}

fn normalized_owner_name(value: &str) -> String {
    value.to_lowercase().replace([' ', '-', '_', '·'], "")
}

fn exact_relationship_candidates(
    relationships: &Value,
    modifier_sources: &Value,
    origins: &OriginCatalog,
    origin_effects: &BTreeMap<i64, OriginEffect>,
    skill_id: i64,
) -> Vec<ExactRelationshipCandidate> {
    let Some(rules) = relationships
        .get("sourcesByRuleId")
        .and_then(Value::as_object)
    else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    for (rule_id, rule) in rules {
        let Some(edges) = rule.get("uidEdges").and_then(Value::as_array) else {
            continue;
        };
        let exact_owner = edges.iter().any(|edge| {
            string_ref(edge, "edgeKind") == Some("owner-source")
                && string_ref(edge, "uidKind") == Some("skill-aoyi")
                && integer(edge, "uid") == Some(skill_id)
                && string_ref(edge, "source") == Some("BuffName")
                && string_ref(edge, "relationshipKind") == Some("battle-imagine-runtime-buff-row")
        });
        if !exact_owner {
            continue;
        }

        let owner_skill_effect_ids = edge_ids(edges, "owner-skill-effect");
        let source_config_ids = edge_ids(edges, "source-config-row");
        let runtime_buff_ids = edges
            .iter()
            .filter(|edge| {
                matches!(
                    string_ref(edge, "edgeKind"),
                    Some("observed-buff" | "runtime-buff-alias")
                )
            })
            .filter_map(|edge| integer(edge, "uid"))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let source_buff_ids = edge_ids(edges, "source-buff");
        let target_damage_ids = edge_ids(edges, "target-damage-row");
        let relationship_kinds = edges
            .iter()
            .filter_map(|edge| string(edge, "relationshipKind"))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();

        let evidence_buff_ids = runtime_buff_ids
            .iter()
            .chain(source_buff_ids.iter())
            .copied()
            .collect::<BTreeSet<_>>();
        let mut modifier_source_ids = BTreeSet::new();
        let mut formula_statuses = BTreeSet::new();
        for buff_id in &evidence_buff_ids {
            let (source_ids, statuses) = modifier_source_evidence(modifier_sources, *buff_id);
            modifier_source_ids.extend(source_ids);
            formula_statuses.extend(statuses);
        }
        let historical_effects = evidence_buff_ids
            .iter()
            .filter_map(|buff_id| origin_effects.get(buff_id).cloned())
            .collect::<Vec<_>>();
        let involved_ids = evidence_buff_ids
            .iter()
            .chain(source_config_ids.iter())
            .copied()
            .collect::<BTreeSet<_>>();
        let historical_relations = origins
            .relations
            .iter()
            .filter(|relation| {
                involved_ids.contains(&relation.effect_id)
                    || involved_ids.contains(&relation.source_config_id)
            })
            .cloned()
            .collect::<Vec<_>>();

        candidates.push(ExactRelationshipCandidate {
            rule_id: rule_id.clone(),
            source_id: string(rule, "sourceId"),
            relationship_source: "current-build ModifierRelationshipTable owner-source edge",
            relationship_kinds,
            owner_skill_effect_ids,
            source_config_ids,
            runtime_buff_ids,
            source_buff_ids,
            target_damage_ids,
            modifier_source_ids: modifier_source_ids.into_iter().collect(),
            formula_statuses: formula_statuses.into_iter().collect(),
            historical_effects,
            historical_relations,
            uid_edges: edges.clone(),
        });
    }
    candidates.sort_by(|left, right| left.rule_id.cmp(&right.rule_id));
    candidates
}

fn edge_ids(edges: &[Value], edge_kind: &str) -> Vec<i64> {
    edges
        .iter()
        .filter(|edge| string_ref(edge, "edgeKind") == Some(edge_kind))
        .filter_map(|edge| integer(edge, "uid"))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn strong_owner_family_match(stem: &str, design_name: &str) -> bool {
    let normalized = normalized_owner_name(design_name);
    let Some(remainder) = normalized.strip_prefix(stem) else {
        return false;
    };
    [
        "主动",
        "光环",
        "全队",
        "对自己",
        "被动叠层",
        "被动触发",
        "触发",
        "增益",
    ]
    .iter()
    .any(|prefix| remainder.starts_with(prefix))
}

fn modifier_source_evidence(index: &Value, buff_id: i64) -> (Vec<String>, Vec<String>) {
    let Some(rows) = index
        .get("byBuffId")
        .and_then(|value| value.get(buff_id.to_string()))
        .and_then(Value::as_array)
    else {
        return (Vec::new(), Vec::new());
    };
    let source_ids = rows
        .iter()
        .filter_map(|row| string(row, "sourceId"))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let formula_statuses = rows
        .iter()
        .filter_map(|row| {
            string(row, "contributionStatus")
                .or_else(|| nested_string(row, &["attributionModel", "status"]))
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    (source_ids, formula_statuses)
}

fn related_origins(relations: &[OriginRelation], buff_id: i64) -> Vec<OriginRelation> {
    relations
        .iter()
        .filter(|relation| relation.effect_id == buff_id || relation.source_config_id == buff_id)
        .cloned()
        .collect()
}

#[derive(Debug, PartialEq, Eq)]
struct StableLocalizedIdentity {
    id: Option<i64>,
    design_name: Option<String>,
    english_name: Option<String>,
    english_description: Option<String>,
    icon_path: Option<String>,
}

fn stable_localized_identity(value: &Value) -> StableLocalizedIdentity {
    StableLocalizedIdentity {
        id: integer(value, "Id"),
        // Historical BuffName exports use DesignName/Names.design while the
        // decoded current-build BuffTable calls the same field NameDesign.
        design_name: nonempty_string(value, "DesignName")
            .or_else(|| nonempty_nested_string(value, &["Names", "design"]))
            .or_else(|| nonempty_string(value, "NameDesign")),
        english_name: nonempty_nested_string(value, &["Names", "en"])
            .or_else(|| nonempty_string(value, "Name")),
        english_description: nonempty_nested_string(value, &["CleanDescriptions", "en"])
            .or_else(|| nonempty_nested_string(value, &["Descriptions", "en"]))
            .or_else(|| nonempty_string(value, "Desc")),
        icon_path: nonempty_string(value, "IconPath").or_else(|| nonempty_string(value, "Icon")),
    }
}

fn stable_localized_identity_unchanged(current: &Value, historical: &Value) -> bool {
    let current = stable_localized_identity(current);
    let historical = stable_localized_identity(historical);

    current.id == historical.id
        && current.design_name.is_some()
        && current.design_name == historical.design_name
        && shared_field_unchanged(&current.english_name, &historical.english_name)
        && shared_field_unchanged(
            &current.english_description,
            &historical.english_description,
        )
        && shared_field_unchanged(&current.icon_path, &historical.icon_path)
}

fn shared_field_unchanged(current: &Option<String>, historical: &Option<String>) -> bool {
    match (current, historical) {
        (Some(current), Some(historical)) => current == historical,
        _ => true,
    }
}

fn nonempty_string(value: &Value, key: &str) -> Option<String> {
    string(value, key).filter(|value| !value.trim().is_empty())
}

fn nonempty_nested_string(value: &Value, path: &[&str]) -> Option<String> {
    nested_string(value, path).filter(|value| !value.trim().is_empty())
}

fn count_class(skills: &[SkillOrigin], class: &str) -> usize {
    skills
        .iter()
        .filter(|skill| skill.candidate_classes.iter().any(|value| value == class))
        .count()
}

fn table_rows(value: &Value) -> Result<Vec<&Value>, Box<dyn std::error::Error>> {
    match value {
        Value::Array(rows) => Ok(rows.iter().collect()),
        Value::Object(rows) => Ok(rows.values().collect()),
        _ => Err("expected a JSON array or keyed object table".into()),
    }
}

fn rows_by_id<'a>(rows: &[&'a Value]) -> BTreeMap<i64, &'a Value> {
    rows.iter()
        .filter_map(|row| integer(row, "Id").map(|id| (id, *row)))
        .collect()
}

fn integer(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(Value::as_i64)
}

fn string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

fn string_ref<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn nested_string(value: &Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current.as_str().map(str::to_string)
}

fn read_json(path: impl AsRef<Path>) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_reader(BufReader::new(File::open(path)?))?)
}

fn usage() -> &'static str {
    "usage: rlogs-bpsr-current-aoyi-origin-ledger <decoded-root> <skill-aoyi-icons.json> <modifier-source-index.json> <modifier-relationship-table.json> <skill-damage-chain-bridge.json> <effect-sources.json> <origin-catalog.json> <historical-buff-name.json> <aoyi-remodel-consumer-proof.json> <aoyi-projectile-status-proof.json> <output.json> <game-build>"
}

fn validate_remodel_consumer_proof(
    proof: &RemodelConsumerProof,
    expected_build: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if proof.schema_version == 0 {
        return Err("Aoyi remodel consumer proof schema must be positive".into());
    }
    if proof.game_build != expected_build {
        return Err(format!(
            "Aoyi remodel consumer proof build {} does not match requested build {expected_build}",
            proof.game_build
        )
        .into());
    }
    if proof.remodel_info_type.attribute != 1 || proof.remodel_info_type.buff != 3 {
        return Err("Aoyi remodel consumer proof does not establish Attr=1 and Buff=3".into());
    }
    if !proof.assertions.kind_1_is_direct_attribute_not_buff
        || !proof.assertions.kind_3_is_buff_reference
    {
        return Err("Aoyi remodel consumer proof assertions are incomplete".into());
    }
    if proof.assertions.attribute_tuple_layout != ["kind", "attribute_id", "raw_value"]
        || proof.assertions.buff_tuple_layout != ["kind", "buff_id", "parameter_set_index"]
    {
        return Err("Aoyi remodel consumer proof tuple layouts are not exact".into());
    }
    Ok(())
}

fn validate_projectile_status_proof(
    proof: &AoyiProjectileStatusProof,
    expected_build: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let current = &proof.current_static;
    let packet = &proof.historical_packet;
    let limits = &proof.ownership_limits;
    if proof.schema_version != 1 || proof.current_game_build != expected_build {
        return Err("Aoyi projectile status proof schema or current build changed".into());
    }
    if proof.historical_packet_build != "24252055"
        || proof.proof_state
            != "current-static-chain-exact-plus-historical-projectile-status-edge-current-packet-provider-live-gated"
    {
        return Err("Aoyi projectile status proof history identity changed".into());
    }
    if current.direct_owner_skill_id != 3_914
        || current.shared_owner_skill_ids != [3_914, 3_946]
        || current.skill_effect_ids != [391_401, 394_601]
        || current.projectile_config_id != 10_040_102
        || current.damage_attr_id != 31_004_010_200
        || current.recount_id != 270
        || current.target_status_id != 2_110_092
        || current.target_status_duration_seconds != 10.0
        || current.target_status_tags != [78]
        || current.projectile_duration_seconds != 1.0
        || current.projectile_hit_camp_types != [1]
        || current.damage_script != "AutoAttack"
    {
        return Err("Aoyi projectile status proof current static chain changed".into());
    }
    if packet.session_id != "monitor-1785609048000.run-0003"
        || packet.source_actor_kind != "projectile"
        || packet.source_projectile_config_id != 10_040_102
        || packet.source_actor_ids.is_empty()
        || packet.target_actor_ids.is_empty()
        || packet
            .target_actor_kinds
            .iter()
            .any(|kind| kind == "player")
        || packet.applied_count == 0
        || packet.removed_count == 0
    {
        return Err("Aoyi projectile status proof historical packet edge is incomplete".into());
    }
    if limits.player_provider_identity_available_in_historical_projection
        || limits.current_build_packet_lifecycle_observed
        || !limits
            .exact_owner_selection_rule
            .contains("packet-observed projectile owner")
        || !limits.rdps_gate.contains("exact armor formula")
    {
        return Err("Aoyi projectile status proof ownership safety gate changed".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formula_gap_inventory_keeps_every_current_watchlist_effect() {
        assert_eq!(FORMULA_GAP_EFFECT_IDS.len(), 11);
        assert!(FORMULA_GAP_EFFECT_IDS.contains(&2_110_102));
        assert!(FORMULA_GAP_EFFECT_IDS.contains(&2_110_109));
        assert!(FORMULA_GAP_EFFECT_IDS.contains(&3_210_211));
    }

    #[test]
    fn exact_component_owner_precedes_weaker_owner_candidates() {
        assert_eq!(
            formula_gap_owner_evidence_state(&[3_969], &[3_999], &[3_998], &[3_997], &[3_996]),
            "exact-component-route-current-runtime-reproof-required"
        );
        assert_eq!(
            formula_gap_owner_evidence_state(&[], &[3_999], &[3_998], &[3_997], &[3_996]),
            "exact-generated-relationship"
        );
        assert_eq!(
            formula_gap_owner_evidence_state(&[], &[], &[3_998], &[3_997], &[3_996]),
            "strong-design-family-candidate-not-formula-authority"
        );
        assert_eq!(
            formula_gap_owner_evidence_state(&[], &[], &[], &[3_997], &[3_996]),
            "broad-design-prefix-candidate-not-formula-authority"
        );
        assert_eq!(
            formula_gap_owner_evidence_state(&[], &[], &[], &[], &[3_996]),
            "unique-semantic-duration-candidate-not-numeric-owner-edge"
        );
        assert_eq!(
            formula_gap_owner_evidence_state(&[], &[], &[], &[], &[3_996, 3_997]),
            "ambiguous-multiple-semantic-candidates-not-numeric-owner-edge"
        );
        assert_eq!(
            formula_gap_owner_evidence_state(&[], &[], &[], &[], &[]),
            "no-current-owner-candidate"
        );
    }

    #[test]
    fn semantic_owner_candidate_is_never_runtime_authority() {
        let candidate = SemanticOwnerCandidate {
            effect_id: 2_110_126,
            owner_skill_id: 3_958,
            relationship_source: "test",
            skill_effect_id: 395_801,
            source_subskill_id: None,
            source_subskill_effect_id: None,
            item_id: Some(3_000_105),
            monster_id: Some(3_000_054),
            runtime_monster_id: None,
            transformed_attribute_id: Some(11_152),
            matching_terms: vec!["Versatility", "20-second duration"],
            matching_duration_seconds: 20,
            stack_cap: Some(1),
            recipient_scope: "self",
            rdps_disposition: "ordinary-owner-stats-never-transferred",
            proof_state: "candidate-only",
            runtime_authority: false,
        };
        assert!(!candidate.runtime_authority);
    }

    #[test]
    fn owner_stems_are_conservative() {
        assert_eq!(owner_stem("眼球王-被动"), Some("眼球王".to_string()));
        assert_eq!(
            owner_stem("卷心菜精英1-被动"),
            Some("卷心菜精英1".to_string())
        );
        assert_eq!(owner_stem("普通增益"), None);
    }

    #[test]
    fn owner_family_strength_keeps_broad_rows_without_calling_them_exact() {
        assert!(strong_owner_family_match("眼球王", "眼球王-主动buff"));
        assert!(strong_owner_family_match(
            "巨塔boss",
            "巨塔BOSS-全队属性增幅"
        ));
        assert!(!strong_owner_family_match("巨塔boss", "巨塔BOSS出生BUFF"));
    }

    #[test]
    fn recipient_classifier_separates_party_and_self_text() {
        let party = classify_recipients(
            "You and your teammates within the area gain increased ATK and Attack SPD.",
        );
        assert_eq!(party.state, "external-recipient-described");
        let own = classify_recipients("After casting, your Crit and Crit DMG are increased.");
        assert_eq!(own.state, "self-only-described");
    }

    #[test]
    fn damage_merge_ids_are_exact_and_deduplicated() {
        let effect = serde_json::json!({
            "SkillAttrDes": [
                ["Total DMG", "{*skillpara.damageMerge({111,222},{1,1},\"PVEDamageRadio\",\"up\")*}"],
                ["Repeated", "{*skillpara.damageMerge({222},{1},\"PVEFixedParameter\",\"un\")*}"]
            ]
        });
        assert_eq!(effect_damage_merge_ids(&effect), vec![111, 222]);
    }

    #[test]
    fn offensive_candidate_classes_do_not_promote_self_only_text() {
        let party = classify_candidate_classes(
            "You and your teammates within the area gain increased ATK, Attack SPD, and Casting SPD.",
        );
        assert!(party.iter().any(|class| class == "external-offense-stat"));
        let own = classify_candidate_classes("After casting, your Crit and Crit DMG increase.");
        assert!(own.iter().any(|class| class == "self-only-offense"));
        assert!(!own.iter().any(|class| class == "external-offense-stat"));
    }

    #[test]
    fn passive_owner_ids_require_a_real_buff_row() {
        let row = serde_json::json!({"TransformationType": [[3, 3210210, 1], [1, 11152, 5040]]});
        let buff = serde_json::json!({"Id": 3210210});
        let mut buffs = BTreeMap::new();
        buffs.insert(3_210_210, &buff);
        assert_eq!(passive_owner_ids(&row, &buffs), vec![3_210_210]);
    }

    #[test]
    fn unmarkpercent_lane_parser_keeps_exact_referenced_lanes() {
        assert_eq!(
            unmarkpercent_lanes(
                "between {*Decision.unmarkpercent(2)*} and {*Decision.unmarkpercent(1)*}; repeat {*Decision.unmarkpercent(2)*}"
            ),
            vec![1, 2]
        );
    }

    #[test]
    fn fixed_point_parameter_lane_reports_percent_and_decimal_forms() {
        let lane = parameter_lane(1, "lucky-effect-attack-damage-percent".to_owned(), 1_120);
        assert_eq!(lane.raw_value, 1_120);
        assert_eq!(lane.percent_value, 11.2);
        assert_eq!(lane.decimal_value, 0.112);
    }

    #[test]
    fn passive_parameter_evidence_joins_transform_description_and_tiers() {
        let row = json!({
            "TransformationType": [[3, 3200009, 1]],
            "BuffPar": [[1120]]
        });
        let icon = json!({
            "parameterRecords": [
                {
                    "buffId": 3200009,
                    "tier": 0,
                    "parameterSetIndex": 1,
                    "parameterValues": [{"parameterIndex": 1, "rawValue": 1120}]
                },
                {
                    "buffId": 3200009,
                    "tier": 1,
                    "parameterSetIndex": 1,
                    "parameterValues": [{"parameterIndex": 1, "rawValue": 1456}]
                }
            ]
        });
        let description = json!({
            "Id": 3200009,
            "Description": "dealing {*Decision.unmarkpercent(1)*} ATK"
        });
        let descriptions = BTreeMap::from([(3_200_009, &description)]);

        let result = passive_parameter_evidence(&row, Some(&icon), &descriptions).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].base_lanes[0].percent_value, 11.2);
        assert_eq!(result[0].tier_lanes[0].lanes[0].percent_value, 14.56);
        assert!(!result[0].runtime_authority);
    }

    #[test]
    fn direct_attribute_transform_keeps_percentage_and_additive_lanes_distinct() {
        let percentage_row = json!({
            "Id": 3934,
            "TransformationType": [[1, 11034, 400]]
        });
        let additive_row = json!({
            "Id": 3958,
            "TransformationType": [[1, 11152, 5040]]
        });
        let agility = json!({
            "Id": 11030,
            "AttrFinal": 11030,
            "AttrTotal": 11031,
            "AttrAdd": 11032,
            "AttrExAdd": 11033,
            "AttrPer": 11034,
            "AttrExPer": 11035,
            "AttrNumType": 0,
            "OfficialName": "Agility"
        });
        let versatility = json!({
            "Id": 11150,
            "AttrFinal": 11150,
            "AttrTotal": 11151,
            "AttrAdd": 11152,
            "AttrExAdd": 11153,
            "AttrPer": 11154,
            "AttrExPer": 11155,
            "AttrNumType": 0,
            "OfficialName": "Versatility"
        });
        let tier_one = json!({
            "Id": 116,
            "SkillId": 3934,
            "Level": 1,
            "TransformationType": [[7, 3934, 1], [1, 11034, 520]]
        });
        let fight_attrs = vec![&agility, &versatility];
        let tiers = vec![&tier_one];
        let remodel_proof = RemodelConsumerProof {
            schema_version: 1,
            game_build: "24609362".to_owned(),
            remodel_info_type: RemodelInfoTypeProof {
                attribute: 1,
                buff: 3,
            },
            assertions: RemodelConsumerAssertions {
                kind_1_is_direct_attribute_not_buff: true,
                kind_3_is_buff_reference: true,
                attribute_tuple_layout: vec![
                    "kind".to_owned(),
                    "attribute_id".to_owned(),
                    "raw_value".to_owned(),
                ],
                buff_tuple_layout: vec![
                    "kind".to_owned(),
                    "buff_id".to_owned(),
                    "parameter_set_index".to_owned(),
                ],
            },
            proof_state: "exact-current-build-decompiled-client-consumer".to_owned(),
        };

        let percentage = direct_attribute_transformation_evidence(
            &percentage_row,
            3934,
            &fight_attrs,
            &tiers,
            &remodel_proof,
        )
        .unwrap();
        assert_eq!(percentage[0].attribute_component, "percentage");
        assert_eq!(percentage[0].base_raw_value, 400);
        assert_eq!(percentage[0].tier_raw_values[0].raw_value, 520);
        assert_eq!(
            percentage[0].value_interpretation,
            "signed-fixed-point-percent-100-raw-units-per-percent"
        );

        let additive = direct_attribute_transformation_evidence(
            &additive_row,
            3958,
            &fight_attrs,
            &[],
            &remodel_proof,
        )
        .unwrap();
        assert_eq!(additive[0].attribute_component, "additive");
        assert_eq!(additive[0].base_raw_value, 5040);
        assert_eq!(
            additive[0].value_interpretation,
            "raw-additive-attribute-units-no-percent-coercion"
        );
        assert_eq!(
            additive[0].rdps_disposition,
            "ordinary-owner-damage-never-transferred"
        );
    }

    #[test]
    fn active_modifier_parameter_evidence_uses_exact_effect_owner_fallback_and_avoids_alias_double_counting()
     {
        let skill_effect = json!({
            "Id": 391501,
            "SkillId": 3915,
            "SkillAttrDes": [
                ["Total DMG", "ignored"],
                ["Luck Bonus", ""],
                ["Lucky Strike Multiplier", ""],
                ["Duration 20s", ""],
                ["CD", ""]
            ]
        });
        let buff = json!({"Id": 2110109, "DestroyParam": [[20.0]]});
        let icon = json!({
            "TierEffects": [{
                "tier": 1,
                "values": [
                    {"key": "attrPer", "rawValue": 200},
                    {"key": "attrAdd", "rawValue": 896},
                    {"key": "attrLv", "rawValue": 0},
                    {"key": "attrMax", "rawValue": 896}
                ]
            }]
        });
        let skills = BTreeMap::new();
        let effects = BTreeMap::from([(391501, &skill_effect)]);
        let buffs = BTreeMap::from([(2110109, &buff)]);
        let routes = vec![ComponentRoute {
            component_id: "self-only-active-modifier",
            role: "test",
            effect_ids: vec![2110109],
            source_config_ids: Vec::new(),
            recipient_scope: "self-only",
            rdps_disposition: "excluded",
            proof_state: "test",
        }];

        let result = active_modifier_parameter_evidence(
            3915,
            Some(&icon),
            &skills,
            &effects,
            &buffs,
            &routes,
            &[],
        )
        .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].duration_seconds, Some(20.0));
        assert_eq!(result[0].tiers[0].fields[0].semantic_role, "Luck Bonus");
        assert_eq!(result[0].tiers[0].fields[0].percent_value, 2.0);
        assert_eq!(result[0].tiers[0].fields[1].percent_value, 8.96);
        assert_eq!(
            result[0].tiers[0].fields[3].contribution_role,
            "mirrored-ui-cap-alias-do-not-double-count"
        );
        assert_eq!(
            result[0].tiers[0].fields[3].alias_of.as_deref(),
            Some("attrAdd")
        );
        assert!(!result[0].runtime_authority);
    }

    #[test]
    fn active_modifier_parameter_evidence_maps_numbered_crit_pairs_by_label_order() {
        let labels = vec![
            "Tier 1 Crit Boost".to_owned(),
            "Tier 1 Crit DMG Boost".to_owned(),
            "Tier 2 Crit Boost".to_owned(),
            "Tier 2 Crit DMG Boost".to_owned(),
        ];
        let crit =
            active_modifier_field("attr2", 896, &labels, ActiveParameterGrammar::NumberedPairs)
                .unwrap();
        let crit_damage = active_modifier_field(
            "attrPer2",
            160,
            &labels,
            ActiveParameterGrammar::NumberedPairs,
        )
        .unwrap();
        let alias = active_modifier_field(
            "attrMax2",
            896,
            &labels,
            ActiveParameterGrammar::NumberedPairs,
        )
        .unwrap();

        assert_eq!(crit.semantic_role, "Tier 2 Crit Boost");
        assert_eq!(crit.percent_value, 8.96);
        assert_eq!(crit_damage.semantic_role, "Tier 2 Crit DMG Boost");
        assert_eq!(crit_damage.percent_value, 1.6);
        assert_eq!(alias.alias_of.as_deref(), Some("attr2"));
    }

    #[test]
    fn active_modifier_parameter_evidence_joins_semantic_summon_subskill_tier_lane() {
        let owner_effect = json!({"Id": 393401, "SkillId": 3934, "SkillAttrDes": []});
        let summon_effect = json!({
            "Id": 200174001,
            "SkillId": 2001740,
            "SkillAttrDes": [
                ["Total DMG", "ignored"],
                ["Target's Armor Decrease", ""]
            ]
        });
        let buff = json!({"Id": 2110078, "DestroyParam": [[0.0, 10.0]]});
        let icon = json!({
            "TierEffects": [{
                "tier": 1,
                "values": [{"key": "attrPer", "rawValue": 200}]
            }]
        });
        let owner_skill = json!({"Id": 3934, "EffectIDs": [393401]});
        let skills = BTreeMap::from([(3934, &owner_skill)]);
        let effects = BTreeMap::from([(393401, &owner_effect), (200174001, &summon_effect)]);
        let buffs = BTreeMap::from([(2110078, &buff)]);
        let semantic = vec![SemanticOwnerCandidate {
            effect_id: 2110078,
            owner_skill_id: 3934,
            relationship_source: "test",
            skill_effect_id: 393401,
            source_subskill_id: Some(2001740),
            source_subskill_effect_id: Some(200174001),
            item_id: None,
            monster_id: None,
            runtime_monster_id: None,
            transformed_attribute_id: None,
            matching_terms: Vec::new(),
            matching_duration_seconds: 10,
            stack_cap: Some(1),
            recipient_scope: "skill-target-enemy",
            rdps_disposition: "external-target-mitigation",
            proof_state: "test",
            runtime_authority: false,
        }];

        let result = active_modifier_parameter_evidence(
            3934,
            Some(&icon),
            &skills,
            &effects,
            &buffs,
            &[],
            &semantic,
        )
        .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].skill_effect_id, 200174001);
        assert_eq!(result[0].active_effect_ids, vec![2110078]);
        assert_eq!(result[0].duration_seconds, Some(10.0));
        assert_eq!(
            result[0].tiers[0].fields[0].semantic_role,
            "Target's Armor Decrease"
        );
        assert_eq!(result[0].tiers[0].fields[0].percent_value, 2.0);
    }

    #[test]
    fn active_modifier_parameter_evidence_maps_external_direct_tier_fields() {
        let skill = json!({"Id": 3974, "EffectIDs": [397401]});
        let skill_effect = json!({
            "Id": 397401,
            "SkillId": 3974,
            "SkillAttrDes": [
                ["Total DMG", "ignored"],
                ["ATK bonus", ""],
                ["Attack SPD Boost", ""],
                ["Casting SPD Boost", ""],
                ["Duration", "15s"],
                ["CD", ""]
            ]
        });
        let buff = json!({"Id": 2110143, "DestroyParam": [[0.0, 1.0]]});
        let icon = json!({
            "TierEffects": [{
                "tier": 3,
                "values": [
                    {"key": "attrA", "rawValue": 180},
                    {"key": "attrB", "rawValue": 180},
                    {"key": "attrC", "rawValue": 360}
                ]
            }]
        });
        let skills = BTreeMap::from([(3974, &skill)]);
        let effects = BTreeMap::from([(397401, &skill_effect)]);
        let buffs = BTreeMap::from([(2110143, &buff)]);
        let routes = vec![ComponentRoute {
            component_id: "functional-amp-external-attack",
            role: "transferable-external-modifier",
            effect_ids: vec![2110143],
            source_config_ids: vec![2110151],
            recipient_scope: "provider-and-external-teammates-in-area",
            rdps_disposition: "exact-attack-and-mattack-counterfactual-only",
            proof_state: "test",
        }];

        let result = active_modifier_parameter_evidence(
            3974,
            Some(&icon),
            &skills,
            &effects,
            &buffs,
            &routes,
            &[],
        )
        .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].duration_seconds, Some(1.0));
        assert_eq!(
            result[0].recipient_scopes,
            vec!["provider-and-external-teammates-in-area"]
        );
        assert_eq!(result[0].tiers[0].fields[0].semantic_role, "ATK bonus");
        assert_eq!(
            result[0].tiers[0].fields[1].semantic_role,
            "Attack SPD Boost"
        );
        assert_eq!(
            result[0].tiers[0].fields[2].semantic_role,
            "Casting SPD Boost"
        );
        assert_eq!(
            result[0].tiers[0].fields[0].contribution_role,
            "active-tier-parameter-not-total-magnitude"
        );
    }

    #[test]
    fn active_tier_parameter_routes_include_known_modifier_families_but_not_produced_damage() {
        let route = |role| ComponentRoute {
            component_id: "test-component",
            role,
            effect_ids: Vec::new(),
            source_config_ids: Vec::new(),
            recipient_scope: "test",
            rdps_disposition: "test",
            proof_state: "test",
        };

        for role in [
            "transferable-external-action-opportunity",
            "transferable-external-derived-attack-and-haste-modifier",
            "transferable-external-healing-modifier",
            "transferable-external-modifier",
            "transferable-external-target-mitigation",
            "transferable-party-shield-with-tier-scalar",
            "mixed-defensive-attack-reduction-and-external-target-vulnerability",
            "external-produced-shield",
        ] {
            assert!(active_tier_parameter_route(&route(role)), "missing {role}");
        }
        assert!(!active_tier_parameter_route(&route(
            "recipient-triggered-produced-damage"
        )));
        assert!(!active_tier_parameter_route(&route(
            "owner-equipped-passive-stat-family"
        )));
    }

    #[test]
    fn active_modifier_parameter_grammar_maps_named_shield_tier() {
        let labels = vec!["Shield".to_owned()];
        let keys = BTreeSet::from(["shield"]);
        let grammar = active_parameter_grammar(&labels, &keys).expect("shield grammar");
        let field = active_modifier_field("shield", 1_800, &labels, grammar).unwrap();

        assert_eq!(field.semantic_role, "Shield");
        assert_eq!(field.percent_value, 18.0);
        assert_eq!(
            field.contribution_role,
            "active-tier-parameter-not-total-magnitude"
        );
    }

    #[test]
    fn active_modifier_parameter_grammar_maps_shield_then_ordered_debuffs() {
        let labels = vec![
            "Celestial Spirit Guard Shield".to_owned(),
            "Celestial Spirit Guard ATK Reduction".to_owned(),
            "Celestial Spirit Guard Vulnerability".to_owned(),
            "Celestial Spirit Guard Elemental Resistance Reduction".to_owned(),
        ];
        let keys = BTreeSet::from(["shieldHp", "attrA", "attrB", "attrC"]);
        let grammar = active_parameter_grammar(&labels, &keys).expect("mixed shield grammar");

        assert_eq!(
            active_modifier_field("shieldHp", 450, &labels, grammar)
                .unwrap()
                .semantic_role,
            "Celestial Spirit Guard Shield"
        );
        assert_eq!(
            active_modifier_field("attrA", 180, &labels, grammar)
                .unwrap()
                .semantic_role,
            "Celestial Spirit Guard ATK Reduction"
        );
        assert_eq!(
            active_modifier_field("attrC", 180, &labels, grammar)
                .unwrap()
                .semantic_role,
            "Celestial Spirit Guard Elemental Resistance Reduction"
        );
    }

    #[test]
    fn modifier_semantic_labels_exclude_damage_and_cooldown_rows_without_hiding_modifiers() {
        let effect = json!({
            "SkillAttrDes": [
                ["DMG per hit", "{*skillpara.damageMerge({2211009604},{1},\"PVEDamageRadio\",\"up\")*}"],
                ["Shield", "{*skillpara.effect(\"shield\",\"up\")*} Max HP"],
                ["Duration", "15s"],
                ["Transformation Cooldown", "70s"],
                ["ATK Reduction", ""]
            ]
        });

        assert_eq!(
            modifier_semantic_labels(&effect),
            vec!["Shield".to_owned(), "ATK Reduction".to_owned()]
        );
    }

    #[test]
    fn active_modifier_parameter_grammar_fails_closed_for_unknown_fields() {
        let labels = vec!["ATK Boost".to_owned(), "Haste Boost".to_owned()];
        let keys = BTreeSet::from(["attrPer", "mystery"]);
        assert!(active_parameter_grammar(&labels, &keys).is_none());
    }

    #[test]
    fn exact_relationship_candidates_require_the_exact_owner_edge() {
        let relationships = serde_json::json!({
            "sourcesByRuleId": {
                "mrs:exact": {
                    "sourceId": "buff-source:2110034",
                    "uidEdges": [
                        {"edgeKind":"owner-source","uidKind":"skill-aoyi","uid":3921,"source":"BuffName","relationshipKind":"battle-imagine-runtime-buff-row"},
                        {"edgeKind":"owner-skill-effect","uidKind":"skill-effect","uid":392101},
                        {"edgeKind":"source-config-row","uidKind":"source-config","uid":2110033},
                        {"edgeKind":"runtime-buff-alias","uidKind":"buff","uid":2110034}
                    ]
                },
                "mrs:broad": {
                    "sourceId": "buff-source:999",
                    "uidEdges": [
                        {"edgeKind":"owner-source","uidKind":"skill-aoyi","uid":3921,"source":"name-prefix","relationshipKind":"broad"}
                    ]
                }
            }
        });
        let origins = OriginCatalog {
            game_build: "old".to_string(),
            effects: Vec::new(),
            relations: Vec::new(),
        };
        let candidates = exact_relationship_candidates(
            &relationships,
            &serde_json::json!({"byBuffId": {}}),
            &origins,
            &BTreeMap::new(),
            3921,
        );
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].source_config_ids, vec![2_110_033]);
        assert_eq!(candidates[0].runtime_buff_ids, vec![2_110_034]);
        assert_eq!(candidates[0].owner_skill_effect_ids, vec![392_101]);
    }

    #[test]
    fn exact_lock_on_shot_shield_route_requires_buff_duration_and_output_identity() {
        let buff = json!({
            "Id": 2110150,
            "NameDesign": "\u{773c}\u{7403}\u{7cbe}\u{82f1}-\u{4e3b}\u{52a8}",
            "DestroyParam": [[0.0, 15.0]]
        });
        let output = json!({
            "Id": 2211015001_i64,
            "TypeEnum": 2110150,
            "DamageScript": "AddShield"
        });
        let buffs = BTreeMap::from([(2_110_150, &buff)]);
        let outputs = BTreeMap::from([(2_211_015_001, &output)]);

        require_buff_output_route(
            &buffs,
            &outputs,
            2_110_150,
            "\u{773c}\u{7403}\u{7cbe}\u{82f1}-\u{4e3b}\u{52a8}",
            15.0,
            2_211_015_001,
            "AddShield",
        )
        .unwrap();
    }

    #[test]
    fn exact_lock_on_shot_shield_route_rejects_duration_or_script_drift() {
        let buff = json!({
            "Id": 2110150,
            "NameDesign": "\u{773c}\u{7403}\u{7cbe}\u{82f1}-\u{4e3b}\u{52a8}",
            "DestroyParam": [[0.0, 10.0]]
        });
        let output = json!({
            "Id": 2211015001_i64,
            "TypeEnum": 2110150,
            "DamageScript": "Attack"
        });
        let buffs = BTreeMap::from([(2_110_150, &buff)]);
        let outputs = BTreeMap::from([(2_211_015_001, &output)]);

        assert!(
            require_buff_output_route(
                &buffs,
                &outputs,
                2_110_150,
                "\u{773c}\u{7403}\u{7cbe}\u{82f1}-\u{4e3b}\u{52a8}",
                15.0,
                2_211_015_001,
                "AddShield",
            )
            .is_err()
        );
    }

    #[test]
    fn stable_identity_normalizes_current_and_historical_buff_name_schemas() {
        let current = serde_json::json!({
            "Id": 2110143,
            "NameDesign": "眼球王-主动buff",
            "Name": "Functional Amp",
            "Desc": "Increases ATK, Attack SPD, and Casting SPD.",
            "Icon": "ui/atlas/buff/buff_icon03"
        });
        let historical = serde_json::json!({
            "Id": 2110143,
            "NameDesign": "Functional Amp",
            "DesignName": "眼球王-主动buff",
            "Names": {
                "en": "Functional Amp",
                "design": "眼球王-主动buff"
            },
            "CleanDescriptions": {
                "en": "Increases ATK, Attack SPD, and Casting SPD."
            },
            "IconPath": "ui/atlas/buff/buff_icon03"
        });

        assert!(stable_localized_identity_unchanged(&current, &historical));
    }

    #[test]
    fn stable_identity_ignores_fields_absent_from_historical_export() {
        let current = serde_json::json!({
            "Id": 2110151,
            "NameDesign": "眼球王光环",
            "Name": "气刃突刺计数",
            "Desc": "气刃突刺计数"
        });
        let historical = serde_json::json!({
            "Id": 2110151,
            "DesignName": "眼球王光环",
            "Names": {"design": "眼球王光环"}
        });

        assert!(stable_localized_identity_unchanged(&current, &historical));
    }

    #[test]
    fn stable_identity_rejects_changed_shared_fields() {
        let current = serde_json::json!({
            "Id": 2110143,
            "NameDesign": "眼球王-主动buff",
            "Name": "Functional Amp",
            "Desc": "Changed description"
        });
        let historical = serde_json::json!({
            "Id": 2110143,
            "DesignName": "眼球王-主动buff",
            "Names": {"en": "Functional Amp", "design": "眼球王-主动buff"},
            "Descriptions": {"en": "Original description"}
        });

        assert!(!stable_localized_identity_unchanged(&current, &historical));
    }
}
