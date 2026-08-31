use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    fs::File,
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

const CATALOG_SCHEMA_VERSION: u16 = 10;
const MINIMUM_SURFACE_SCHEMA_VERSION: u64 = 2;

#[derive(Debug, Serialize)]
struct RuntimeCatalog {
    schema_version: u16,
    game_build: String,
    generated_by: &'static str,
    promotion_state: &'static str,
    input: InputArtifact,
    decoded_table_input: InputArtifact,
    route_proof_input: InputArtifact,
    source: RuntimeSource,
    policy: RuntimePolicy,
    summary: RuntimeSummary,
    rules: Vec<RuntimeDamageStageRule>,
    coverage_gaps: Vec<DamageStageCoverageGap>,
}

#[derive(Debug, Serialize)]
struct InputArtifact {
    file: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct RuntimeSource {
    decoded_table: String,
    decoded_table_sha256: String,
    decoded_table_bytes: u64,
    row_count: usize,
}

#[derive(Debug, Serialize)]
struct RuntimePolicy {
    runtime_formula_authority: bool,
    packet_replay_required: bool,
    lookup_key: &'static str,
    ambiguous_keys: &'static str,
    formula_surface_equivalence: &'static str,
    nonstandard_scripts: &'static str,
    coefficient_selection: &'static str,
    fixed_parameter_selection: &'static str,
    unresolved_events: &'static str,
}

#[derive(Debug, Serialize)]
struct RuntimeSummary {
    source_rows: usize,
    lookup_keys: usize,
    multi_candidate_lookup_keys: usize,
    equivalent_multi_candidate_standard_rules: usize,
    mixed_standard_and_nonstandard_rules: usize,
    conflicting_standard_keys: usize,
    source_resolved_conflicting_keys: usize,
    source_specific_rules: usize,
    nonstandard_or_missing_script_keys: usize,
    nonstandard_or_missing_script_candidate_rows: usize,
    coverage_gap_records: usize,
    standard_attack_rules: usize,
    standard_magic_attack_rules: usize,
    standard_rules: usize,
}

#[derive(Debug, Serialize)]
struct RuntimeDamageStageRule {
    ability_id: i64,
    hit_event_id: i32,
    damage_source: Option<i32>,
    damage_attr_id: i64,
    equivalent_damage_attr_ids: Vec<i64>,
    excluded_nonstandard_damage_attr_ids: Vec<i64>,
    type_enum: Option<i64>,
    damage_type: Option<i64>,
    damage_script: String,
    coefficient_basis_points_by_stage: Vec<i64>,
    fixed_parameter_by_level: Vec<i64>,
    pve_loop_time: Option<i64>,
    pve_stunned_damage: Vec<i64>,
    pve_extinction_damage: Option<i64>,
    part_damage_radio: Vec<i64>,
    abnormal_damage: Value,
    damage_property: Option<i64>,
    part_damage_type: Option<i64>,
    damage_weight: Value,
    tags: Vec<i64>,
    behit_light_is_open: Option<bool>,
    is_profession: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct DamageStageCandidate {
    damage_attr_id: i64,
    linked_ability_id: Option<i64>,
    hit_event_suffix_candidate: Option<i64>,
    row_level: Option<i64>,
    name: Option<String>,
    type_enum: Option<i64>,
    damage_type: Option<i64>,
    damage_script: Option<String>,
    coefficient_basis_points_by_stage: Vec<i64>,
    fixed_parameter_by_level: Vec<i64>,
    pve_loop_time: Option<i64>,
    pve_stunned_damage: Vec<i64>,
    pve_extinction_damage: Option<i64>,
    part_damage_radio: Vec<i64>,
    abnormal_damage: Value,
    damage_property: Option<i64>,
    part_damage_type: Option<i64>,
    damage_weight: Value,
    tags: Vec<i64>,
    behit_light_is_open: Option<bool>,
    is_profession: Option<bool>,
}

#[derive(Debug, Serialize)]
struct DamageStageCoverageGap {
    gap_class: &'static str,
    lookup_key: String,
    ability_id: i64,
    hit_event_id: i32,
    reason: &'static str,
    candidates: Vec<DamageStageCandidate>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let input = PathBuf::from(option(&arguments, "--surface")?);
    let decoded_table_input = PathBuf::from(option(&arguments, "--decoded-table")?);
    let route_proof_input = PathBuf::from(option(&arguments, "--route-proof")?);
    let output = PathBuf::from(option(&arguments, "--output")?);
    let game_build = option(&arguments, "--build")?.to_owned();
    if game_build.is_empty() || !game_build.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("--build requires a numeric client build".into());
    }
    if arguments.len() != 10 {
        return Err(usage().into());
    }

    let surface: Value = serde_json::from_reader(BufReader::new(File::open(&input)?))?;
    let decoded_table: Value =
        serde_json::from_reader(BufReader::new(File::open(&decoded_table_input)?))?;
    let route_proof: Value =
        serde_json::from_reader(BufReader::new(File::open(&route_proof_input)?))?;
    let surface_artifact = input_artifact(&input)?;
    let decoded_table_artifact = input_artifact(&decoded_table_input)?;
    let route_proof_artifact = input_artifact(&route_proof_input)?;
    let route_selections = parse_route_selections(&route_proof, &game_build)?;
    let rows = surface
        .get("rows")
        .and_then(Value::as_object)
        .ok_or("damage surface is missing rows")?;
    let decoded_rows = decoded_table
        .as_object()
        .ok_or("decoded DamageAttrTable must be an object keyed by Id")?;
    if rows.len() != decoded_rows.len()
        || rows.keys().any(|row_id| !decoded_rows.contains_key(row_id))
    {
        return Err("damage surface and decoded DamageAttrTable row identities differ".into());
    }
    let lookup = surface
        .get("linked_hit_event_candidate_lookup")
        .and_then(Value::as_object)
        .ok_or("damage surface is missing linked_hit_event_candidate_lookup")?;
    let source =
        validate_surface_identity(&surface, &decoded_table_artifact, rows.len(), &game_build)?;

    let mut multi_candidate_lookup_keys = 0_usize;
    let mut equivalent_multi_candidate_standard_rules = 0_usize;
    let mut mixed_standard_and_nonstandard_rules = 0_usize;
    let mut conflicting_standard_keys = 0_usize;
    let mut source_resolved_conflicting_keys = 0_usize;
    let mut nonstandard_or_missing_script_keys = 0_usize;
    let mut nonstandard_or_missing_script_candidate_rows = 0_usize;
    let mut rules = BTreeMap::<(i64, i32, Option<i32>), RuntimeDamageStageRule>::new();
    let mut coverage_gaps = Vec::new();
    for (key, candidate_ids) in lookup {
        let Some((ability, hit)) = key.split_once(':') else {
            return Err(format!("invalid lookup key {key}").into());
        };
        let ability_id = ability.parse::<i64>()?;
        let hit_event_id = hit.parse::<i32>()?;
        let candidate_ids = candidate_ids
            .as_array()
            .ok_or_else(|| format!("lookup {key} does not contain an array"))?;
        multi_candidate_lookup_keys += usize::from(candidate_ids.len() > 1);
        let candidates = candidate_ids
            .iter()
            .map(|damage_attr_id| {
                let damage_attr_id = integer_value(damage_attr_id)
                    .ok_or_else(|| format!("lookup {key} contains a non-integer damage ID"))?;
                candidate(rows, decoded_rows, damage_attr_id)
            })
            .collect::<Result<Vec<_>, String>>()?;
        let standard = candidates
            .iter()
            .filter(|candidate| candidate.is_standard())
            .cloned()
            .collect::<Vec<_>>();
        let nonstandard = candidates
            .iter()
            .filter(|candidate| !candidate.is_standard())
            .cloned()
            .collect::<Vec<_>>();
        if !nonstandard.is_empty() {
            nonstandard_or_missing_script_keys += 1;
            nonstandard_or_missing_script_candidate_rows += nonstandard.len();
            coverage_gaps.push(DamageStageCoverageGap {
                gap_class: "nonstandard-or-missing-script",
                lookup_key: key.clone(),
                ability_id,
                hit_event_id,
                reason: if standard.is_empty() {
                    "no-standard-Attack-or-MAttack-candidate"
                } else {
                    "nonstandard-or-missing-candidate-excluded-from-standard-rule"
                },
                candidates: nonstandard.clone(),
            });
        }
        if standard.is_empty() {
            continue;
        }
        let first = &standard[0];
        if standard
            .iter()
            .skip(1)
            .any(|candidate| !candidate.formula_matches(first))
        {
            if let Some(selections) = exact_source_selections(key, &standard, &route_selections) {
                let excluded_nonstandard_damage_attr_ids = nonstandard
                    .iter()
                    .map(|candidate| candidate.damage_attr_id)
                    .collect::<Vec<_>>();
                for (damage_source, selected) in selections {
                    let rule = RuntimeDamageStageRule {
                        ability_id,
                        hit_event_id,
                        damage_source: Some(damage_source),
                        damage_attr_id: selected.damage_attr_id,
                        equivalent_damage_attr_ids: vec![selected.damage_attr_id],
                        excluded_nonstandard_damage_attr_ids: excluded_nonstandard_damage_attr_ids
                            .clone(),
                        type_enum: selected.type_enum,
                        damage_type: selected.damage_type,
                        damage_script: selected.damage_script.clone().unwrap_or_default(),
                        coefficient_basis_points_by_stage: selected
                            .coefficient_basis_points_by_stage
                            .clone(),
                        fixed_parameter_by_level: selected.fixed_parameter_by_level.clone(),
                        pve_loop_time: selected.pve_loop_time,
                        pve_stunned_damage: selected.pve_stunned_damage.clone(),
                        pve_extinction_damage: selected.pve_extinction_damage,
                        part_damage_radio: selected.part_damage_radio.clone(),
                        abnormal_damage: selected.abnormal_damage.clone(),
                        damage_property: selected.damage_property,
                        part_damage_type: selected.part_damage_type,
                        damage_weight: selected.damage_weight.clone(),
                        tags: selected.tags.clone(),
                        behit_light_is_open: selected.behit_light_is_open,
                        is_profession: selected.is_profession,
                    };
                    if rules
                        .insert((ability_id, hit_event_id, Some(damage_source)), rule)
                        .is_some()
                    {
                        return Err(format!(
                            "duplicate source-specific runtime key {key}:{damage_source}"
                        )
                        .into());
                    }
                }
                source_resolved_conflicting_keys += 1;
                continue;
            }
            conflicting_standard_keys += 1;
            coverage_gaps.push(DamageStageCoverageGap {
                gap_class: "conflicting-standard-formula",
                lookup_key: key.clone(),
                ability_id,
                hit_event_id,
                reason: "standard-candidates-have-conflicting-formula-inputs",
                candidates: standard,
            });
            continue;
        }
        equivalent_multi_candidate_standard_rules += usize::from(standard.len() > 1);
        let excluded_nonstandard_damage_attr_ids = nonstandard
            .iter()
            .map(|candidate| candidate.damage_attr_id)
            .collect::<Vec<_>>();
        mixed_standard_and_nonstandard_rules +=
            usize::from(!excluded_nonstandard_damage_attr_ids.is_empty());
        let rule = RuntimeDamageStageRule {
            ability_id,
            hit_event_id,
            damage_source: None,
            damage_attr_id: first.damage_attr_id,
            equivalent_damage_attr_ids: standard
                .iter()
                .map(|candidate| candidate.damage_attr_id)
                .collect(),
            excluded_nonstandard_damage_attr_ids,
            type_enum: first.type_enum,
            damage_type: first.damage_type,
            damage_script: first.damage_script.clone().unwrap_or_default(),
            coefficient_basis_points_by_stage: first.coefficient_basis_points_by_stage.clone(),
            fixed_parameter_by_level: first.fixed_parameter_by_level.clone(),
            pve_loop_time: first.pve_loop_time,
            pve_stunned_damage: first.pve_stunned_damage.clone(),
            pve_extinction_damage: first.pve_extinction_damage,
            part_damage_radio: first.part_damage_radio.clone(),
            abnormal_damage: first.abnormal_damage.clone(),
            damage_property: first.damage_property,
            part_damage_type: first.part_damage_type,
            damage_weight: first.damage_weight.clone(),
            tags: first.tags.clone(),
            behit_light_is_open: first.behit_light_is_open,
            is_profession: first.is_profession,
        };
        if rules
            .insert((ability_id, hit_event_id, None), rule)
            .is_some()
        {
            return Err(format!("duplicate runtime key {key}").into());
        }
    }

    let standard_attack_rules = rules
        .values()
        .filter(|rule| rule.damage_script == "Attack")
        .count();
    let standard_magic_attack_rules = rules
        .values()
        .filter(|rule| rule.damage_script == "MAttack")
        .count();
    let source_rows = rows.len();
    let source_specific_rules = rules
        .values()
        .filter(|rule| rule.damage_source.is_some())
        .count();
    let rules = rules.into_values().collect::<Vec<_>>();
    let catalog = RuntimeCatalog {
        schema_version: CATALOG_SCHEMA_VERSION,
        game_build,
        generated_by: "rlogs-bpsr-damage-stage-runtime-catalog",
        promotion_state: "candidate-only-current-build-packet-replay-required",
        input: surface_artifact,
        decoded_table_input: decoded_table_artifact,
        route_proof_input: route_proof_artifact,
        source,
        policy: RuntimePolicy {
            runtime_formula_authority: false,
            packet_replay_required: true,
            lookup_key: "packet ability_id plus semantic hit_event_id, with packet damage_source required only for source-disambiguated rules; omitted optional hit_event_id is zero",
            ambiguous_keys: "formula-equivalent standard candidates are retained together; conflicting candidates require an exact current-build static route for every candidate plus packet damage_source",
            formula_surface_equivalence: "candidate equivalence requires every current-build decoded semantic field to match: TypeEnum, DamageScript, damage type, coefficient and fixed-parameter vectors, PVE loop/stunned/extinction fields, part-damage vector and type, abnormal-damage structure, damage property, damage-weight structure, tags, hit-light behavior, and profession flag; row Id, linked ability, hit suffix, display name, and row Level remain separately retained selection identity and never establish runtime authority",
            nonstandard_scripts: "excluded until each DamageScript formula is independently proven",
            coefficient_selection: "one-value vectors are stage invariant; multi-value vectors use zero-based packet owner_stage and omitted owner_stage is zero",
            fixed_parameter_selection: "empty vectors contribute zero; populated vectors use one-based packet owner_level",
            unresolved_events: "canonical damage is always retained and never hidden",
        },
        summary: RuntimeSummary {
            source_rows,
            lookup_keys: lookup.len(),
            multi_candidate_lookup_keys,
            equivalent_multi_candidate_standard_rules,
            mixed_standard_and_nonstandard_rules,
            conflicting_standard_keys,
            source_resolved_conflicting_keys,
            source_specific_rules,
            nonstandard_or_missing_script_keys,
            nonstandard_or_missing_script_candidate_rows,
            coverage_gap_records: coverage_gaps.len(),
            standard_attack_rules,
            standard_magic_attack_rules,
            standard_rules: rules.len(),
        },
        rules,
        coverage_gaps,
    };

    let mut writer = BufWriter::new(File::create(output)?);
    serde_json::to_writer(&mut writer, &catalog)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    eprintln!(
        "wrote {} standard damage-stage rules ({} Attack, {} MAttack); retained {} conflicting standard keys and {} nonstandard/missing-script keys as explicit gaps",
        catalog.summary.standard_rules,
        catalog.summary.standard_attack_rules,
        catalog.summary.standard_magic_attack_rules,
        catalog.summary.conflicting_standard_keys,
        catalog.summary.nonstandard_or_missing_script_keys,
    );
    Ok(())
}

impl DamageStageCandidate {
    fn is_standard(&self) -> bool {
        matches!(self.damage_script.as_deref(), Some("Attack" | "MAttack"))
    }

    fn formula_matches(&self, other: &Self) -> bool {
        self.damage_script == other.damage_script
            && self.type_enum == other.type_enum
            && self.damage_type == other.damage_type
            && self.coefficient_basis_points_by_stage == other.coefficient_basis_points_by_stage
            && self.fixed_parameter_by_level == other.fixed_parameter_by_level
            && self.pve_loop_time == other.pve_loop_time
            && self.pve_stunned_damage == other.pve_stunned_damage
            && self.pve_extinction_damage == other.pve_extinction_damage
            && self.part_damage_radio == other.part_damage_radio
            && self.abnormal_damage == other.abnormal_damage
            && self.damage_property == other.damage_property
            && self.part_damage_type == other.part_damage_type
            && self.damage_weight == other.damage_weight
            && self.tags == other.tags
            && self.behit_light_is_open == other.behit_light_is_open
            && self.is_profession == other.is_profession
    }
}

fn candidate(
    rows: &serde_json::Map<String, Value>,
    decoded_rows: &serde_json::Map<String, Value>,
    damage_attr_id: i64,
) -> Result<DamageStageCandidate, String> {
    let row = rows
        .get(&damage_attr_id.to_string())
        .ok_or_else(|| format!("damage row {damage_attr_id} is missing"))?;
    let decoded = decoded_rows
        .get(&damage_attr_id.to_string())
        .and_then(Value::as_object)
        .ok_or_else(|| format!("decoded damage row {damage_attr_id} is missing or invalid"))?;
    if decoded.get("Id").and_then(integer_value) != Some(damage_attr_id) {
        return Err(format!(
            "decoded damage row {damage_attr_id} has a mismatched Id"
        ));
    }
    Ok(DamageStageCandidate {
        damage_attr_id,
        linked_ability_id: integer_at(row, "/linked_id"),
        hit_event_suffix_candidate: integer_at(row, "/hit_event_suffix_candidate"),
        row_level: decoded.get("Level").and_then(integer_value),
        name: decoded
            .get("Name")
            .and_then(Value::as_str)
            .map(str::to_owned),
        type_enum: decoded.get("TypeEnum").and_then(integer_value),
        damage_type: decoded.get("DamageType").and_then(integer_value),
        damage_script: decoded
            .get("DamageScript")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned),
        coefficient_basis_points_by_stage: decoded_integer_array(decoded, "PVEDamageRadio")?,
        fixed_parameter_by_level: decoded_integer_array(decoded, "PVEFixedParameter")?,
        pve_loop_time: decoded.get("PVELoopTime").and_then(integer_value),
        pve_stunned_damage: decoded_integer_array(decoded, "PVEStunnedDamage")?,
        pve_extinction_damage: decoded.get("PVEExtinctionDamage").and_then(integer_value),
        part_damage_radio: decoded_integer_array(decoded, "PartDamageRadio")?,
        abnormal_damage: decoded
            .get("AbnormalDamage")
            .cloned()
            .ok_or_else(|| format!("decoded damage row {damage_attr_id} lacks AbnormalDamage"))?,
        damage_property: decoded.get("DamageProperty").and_then(integer_value),
        part_damage_type: decoded.get("PartDamageType").and_then(integer_value),
        damage_weight: decoded
            .get("DamageWeight")
            .cloned()
            .ok_or_else(|| format!("decoded damage row {damage_attr_id} lacks DamageWeight"))?,
        tags: decoded_integer_array(decoded, "Tags")?,
        behit_light_is_open: decoded.get("BehitLightIsOpen").and_then(Value::as_bool),
        is_profession: decoded.get("IsProfession").and_then(Value::as_bool),
    })
}

fn decoded_integer_array(
    row: &serde_json::Map<String, Value>,
    field: &str,
) -> Result<Vec<i64>, String> {
    row.get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("decoded damage row lacks integer array {field}"))?
        .iter()
        .map(|value| {
            integer_value(value)
                .ok_or_else(|| format!("decoded damage field {field} contains a non-integer"))
        })
        .collect()
}

fn parse_route_selections(
    route_proof: &Value,
    expected_build: &str,
) -> Result<BTreeMap<String, Vec<(i32, i64)>>, String> {
    let schema_version = route_proof
        .get("schema_version")
        .and_then(Value::as_u64)
        .ok_or_else(|| "damage-source route proof is missing schema_version".to_owned())?;
    if schema_version < 2 {
        return Err("damage-source route proof schema 2 or newer is required".to_owned());
    }
    let actual_build = route_proof
        .get("game_build")
        .and_then(Value::as_str)
        .ok_or_else(|| "damage-source route proof is missing game_build".to_owned())?;
    if actual_build != expected_build {
        return Err(format!(
            "damage-source route proof build {actual_build} does not match --build {expected_build}"
        ));
    }
    route_proof
        .get("keys")
        .and_then(Value::as_array)
        .ok_or_else(|| "damage-source route proof is missing keys".to_owned())?
        .iter()
        .map(|key| {
            let lookup_key = key
                .get("lookup_key")
                .and_then(Value::as_str)
                .ok_or_else(|| "route key is missing lookup_key".to_owned())?
                .to_owned();
            let selections = key
                .get("selection_by_damage_source")
                .and_then(Value::as_array)
                .ok_or_else(|| format!("route key {lookup_key} is missing selections"))?
                .iter()
                .map(|selection| {
                    let source = selection
                        .get("damage_source_id")
                        .and_then(Value::as_i64)
                        .and_then(|value| i32::try_from(value).ok())
                        .ok_or_else(|| {
                            format!("route key {lookup_key} has invalid damage_source_id")
                        })?;
                    let damage_attr_id = selection
                        .get("damage_attr_id")
                        .and_then(integer_value)
                        .ok_or_else(|| {
                            format!("route key {lookup_key} has invalid damage_attr_id")
                        })?;
                    Ok((source, damage_attr_id))
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok((lookup_key, selections))
        })
        .collect()
}

fn exact_source_selections<'a>(
    lookup_key: &str,
    standard: &'a [DamageStageCandidate],
    route_selections: &BTreeMap<String, Vec<(i32, i64)>>,
) -> Option<Vec<(i32, &'a DamageStageCandidate)>> {
    let standard_ids = standard
        .iter()
        .map(|candidate| candidate.damage_attr_id)
        .collect::<BTreeSet<_>>();
    let mut sources = BTreeSet::new();
    let mut covered_ids = BTreeSet::new();
    let mut selected = Vec::new();
    for (damage_source, damage_attr_id) in route_selections.get(lookup_key)? {
        let Some(candidate) = standard
            .iter()
            .find(|candidate| candidate.damage_attr_id == *damage_attr_id)
        else {
            continue;
        };
        if !sources.insert(*damage_source) {
            return None;
        }
        covered_ids.insert(*damage_attr_id);
        selected.push((*damage_source, candidate));
    }
    (!selected.is_empty() && covered_ids == standard_ids).then_some(selected)
}

fn integer_at(value: &Value, pointer: &str) -> Option<i64> {
    value.pointer(pointer).and_then(integer_value)
}

fn option<'a>(arguments: &'a [String], flag: &str) -> Result<&'a str, String> {
    let position = arguments
        .iter()
        .position(|value| value == flag)
        .ok_or_else(usage)?;
    arguments
        .get(position + 1)
        .map(String::as_str)
        .ok_or_else(usage)
}

fn usage() -> String {
    "usage: rlogs-bpsr-damage-stage-runtime-catalog --surface <DamageFormulaSurface.json> --decoded-table <current-build DamageAttrTable.json> --route-proof <damage-source-route-proof.json> --build <numeric-client-build> --output <candidate.json>".to_owned()
}

fn input_artifact(path: &Path) -> Result<InputArtifact, Box<dyn std::error::Error>> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut hasher = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
        bytes = bytes.saturating_add(count as u64);
    }
    Ok(InputArtifact {
        file: path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("external-artifact")
            .to_owned(),
        bytes,
        sha256: format!("{:x}", hasher.finalize()),
    })
}

fn validate_surface_identity(
    surface: &Value,
    decoded_table: &InputArtifact,
    row_count: usize,
    expected_build: &str,
) -> Result<RuntimeSource, String> {
    let schema_version = surface
        .get("schema_version")
        .and_then(Value::as_u64)
        .ok_or_else(|| "damage surface is missing schema_version".to_owned())?;
    if schema_version < MINIMUM_SURFACE_SCHEMA_VERSION {
        return Err(format!(
            "damage surface schema {schema_version} predates the exact decoded-table identity contract"
        ));
    }
    let actual_build = surface
        .get("game_build")
        .and_then(Value::as_str)
        .ok_or_else(|| "damage surface is missing game_build".to_owned())?;
    if actual_build != expected_build {
        return Err(format!(
            "damage surface build {actual_build} does not match --build {expected_build}"
        ));
    }
    let policy = surface
        .get("policy")
        .and_then(Value::as_object)
        .ok_or_else(|| "damage surface is missing policy".to_owned())?;
    if policy
        .get("exact_build_table_required")
        .and_then(Value::as_bool)
        != Some(true)
        || policy
            .get("unresolved_rows_hidden")
            .and_then(Value::as_bool)
            != Some(false)
    {
        return Err(
            "damage surface policy does not preserve the exact-build fail-closed contract"
                .to_owned(),
        );
    }
    let input = surface
        .get("input")
        .and_then(Value::as_object)
        .ok_or_else(|| "damage surface is missing input identity".to_owned())?;
    if input.get("role").and_then(Value::as_str) != Some("exact_build_decoded_damage_attr_table") {
        return Err(
            "damage surface input role is not the exact decoded DamageAttr table".to_owned(),
        );
    }
    let surface_sha256 = input
        .get("sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| "damage surface input is missing sha256".to_owned())?;
    if !surface_sha256.eq_ignore_ascii_case(&decoded_table.sha256) {
        return Err("damage surface and decoded DamageAttr table sha256 differ".to_owned());
    }
    if input.get("bytes").and_then(Value::as_u64) != Some(decoded_table.bytes) {
        return Err("damage surface and decoded DamageAttr table byte lengths differ".to_owned());
    }
    let summary = surface
        .get("summary")
        .and_then(Value::as_object)
        .ok_or_else(|| "damage surface is missing summary".to_owned())?;
    let expected_row_count = u64::try_from(row_count)
        .map_err(|_| "damage surface row count cannot be represented".to_owned())?;
    if summary.get("decoded_rows").and_then(Value::as_u64) != Some(expected_row_count)
        || summary.get("emitted_rows").and_then(Value::as_u64) != Some(expected_row_count)
    {
        return Err("damage surface summary row counts do not match retained rows".to_owned());
    }

    Ok(RuntimeSource {
        decoded_table: decoded_table.file.clone(),
        decoded_table_sha256: decoded_table.sha256.clone(),
        decoded_table_bytes: decoded_table.bytes,
        row_count,
    })
}

fn integer_value(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
}

#[cfg(test)]
mod tests {
    use super::{
        DamageStageCandidate, InputArtifact, exact_source_selections, parse_route_selections,
        validate_surface_identity,
    };
    use serde_json::{Value, json};
    use std::collections::BTreeMap;

    fn candidate(
        damage_attr_id: i64,
        damage_script: Option<&str>,
        coefficients: &[i64],
        fixed: &[i64],
    ) -> DamageStageCandidate {
        DamageStageCandidate {
            damage_attr_id,
            linked_ability_id: None,
            hit_event_suffix_candidate: None,
            row_level: None,
            name: None,
            type_enum: None,
            damage_type: None,
            damage_script: damage_script.map(str::to_owned),
            coefficient_basis_points_by_stage: coefficients.to_vec(),
            fixed_parameter_by_level: fixed.to_vec(),
            pve_loop_time: None,
            pve_stunned_damage: Vec::new(),
            pve_extinction_damage: None,
            part_damage_radio: Vec::new(),
            abnormal_damage: Value::Array(Vec::new()),
            damage_property: None,
            part_damage_type: None,
            damage_weight: Value::Array(Vec::new()),
            tags: Vec::new(),
            behit_light_is_open: None,
            is_profession: None,
        }
    }

    #[test]
    fn formula_equivalence_does_not_depend_on_damage_attr_identity() {
        let left = candidate(31_013_030_100, Some("Attack"), &[8_000], &[]);
        let right = candidate(31_013_040_100, Some("Attack"), &[8_000], &[]);

        assert!(left.formula_matches(&right));
    }

    #[test]
    fn formula_equivalence_requires_every_decoded_semantic_input_to_match() {
        let baseline = candidate(1, Some("Attack"), &[10_000], &[50]);

        assert!(!baseline.formula_matches(&candidate(2, Some("MAttack"), &[10_000], &[50])));
        assert!(!baseline.formula_matches(&candidate(3, Some("Attack"), &[9_999], &[50])));
        assert!(!baseline.formula_matches(&candidate(4, Some("Attack"), &[10_000], &[51])));

        let mut different_damage_type = baseline.clone();
        different_damage_type.damage_type = Some(2);
        assert!(!baseline.formula_matches(&different_damage_type));

        let mut different_loop_time = baseline.clone();
        different_loop_time.pve_loop_time = Some(1);
        assert!(!baseline.formula_matches(&different_loop_time));

        let mut different_property = baseline.clone();
        different_property.damage_property = Some(1);
        assert!(!baseline.formula_matches(&different_property));

        let mut different_part_type = baseline.clone();
        different_part_type.part_damage_type = Some(1);
        assert!(!baseline.formula_matches(&different_part_type));

        let mut different_tags = baseline.clone();
        different_tags.tags = vec![8];
        assert!(!baseline.formula_matches(&different_tags));

        let mut different_type_enum = baseline.clone();
        different_type_enum.type_enum = Some(1);
        assert!(!baseline.formula_matches(&different_type_enum));

        let mut different_stunned = baseline.clone();
        different_stunned.pve_stunned_damage = vec![100];
        assert!(!baseline.formula_matches(&different_stunned));

        let mut different_part_radio = baseline.clone();
        different_part_radio.part_damage_radio = vec![20];
        assert!(!baseline.formula_matches(&different_part_radio));

        let mut different_abnormal = baseline.clone();
        different_abnormal.abnormal_damage = json!([[0]]);
        assert!(!baseline.formula_matches(&different_abnormal));

        let mut different_light = baseline.clone();
        different_light.behit_light_is_open = Some(true);
        assert!(!baseline.formula_matches(&different_light));
    }

    #[test]
    fn row_level_is_retained_identity_not_formula_surface_authority() {
        let mut left = candidate(1, Some("Attack"), &[10_000], &[50]);
        left.row_level = Some(1);
        let mut right = candidate(2, Some("Attack"), &[10_000], &[50]);
        right.row_level = Some(2);

        assert!(left.formula_matches(&right));
    }

    #[test]
    fn only_attack_and_magic_attack_are_standard() {
        assert!(candidate(1, Some("Attack"), &[], &[]).is_standard());
        assert!(candidate(2, Some("MAttack"), &[], &[]).is_standard());
        assert!(!candidate(3, Some("HpAttack"), &[], &[]).is_standard());
        assert!(!candidate(4, None, &[], &[]).is_standard());
    }

    #[test]
    fn source_specific_selection_requires_every_conflicting_candidate() {
        let candidates = vec![
            candidate(392_020_101, Some("Attack"), &[10_000], &[]),
            candidate(19_202_010_101, Some("Attack"), &[5_000], &[]),
        ];
        let mut routes = BTreeMap::new();
        routes.insert(
            "920201:1".to_owned(),
            vec![(0, 19_202_010_101), (1, 392_020_101)],
        );
        let selected = exact_source_selections("920201:1", &candidates, &routes).unwrap();
        assert_eq!(selected.len(), 2);

        routes.insert("920201:1".to_owned(), vec![(1, 392_020_101)]);
        assert!(exact_source_selections("920201:1", &candidates, &routes).is_none());
    }

    #[test]
    fn route_proof_requires_matching_build_and_numeric_source_ids() {
        let proof = json!({
            "schema_version": 2,
            "game_build": "24568685",
            "keys": [{
                "lookup_key": "920201:1",
                "selection_by_damage_source": [{
                    "damage_source_id": 1,
                    "damage_attr_id": 392020101
                }]
            }]
        });
        assert_eq!(
            parse_route_selections(&proof, "24568685").unwrap()["920201:1"],
            vec![(1, 392_020_101)]
        );
        assert!(parse_route_selections(&proof, "24252055").is_err());
    }

    #[test]
    fn surface_identity_requires_matching_build_sha_bytes_rows_and_policy() {
        let surface = json!({
            "schema_version": 2,
            "game_build": "24687926",
            "input": {
                "role": "exact_build_decoded_damage_attr_table",
                "bytes": 123,
                "sha256": "AABB"
            },
            "policy": {
                "exact_build_table_required": true,
                "unresolved_rows_hidden": false
            },
            "summary": {
                "decoded_rows": 5,
                "emitted_rows": 5
            }
        });
        let decoded = InputArtifact {
            file: "DamageAttrTable.json".to_owned(),
            bytes: 123,
            sha256: "aabb".to_owned(),
        };

        let source = validate_surface_identity(&surface, &decoded, 5, "24687926").unwrap();
        assert_eq!(source.decoded_table_sha256, "aabb");
        assert_eq!(source.row_count, 5);
        assert!(validate_surface_identity(&surface, &decoded, 5, "24252055").is_err());

        let mut mismatched_sha = surface.clone();
        mismatched_sha["input"]["sha256"] = json!("ccdd");
        assert!(validate_surface_identity(&mismatched_sha, &decoded, 5, "24687926").is_err());
    }
}
