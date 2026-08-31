use std::{
    collections::BTreeMap,
    env,
    fs::File,
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 1;
const VALUE_EXAMPLE_LIMIT: usize = 12;

#[derive(Debug)]
struct Arguments {
    worklist: PathBuf,
    il2cpp_surface: PathBuf,
    output: PathBuf,
    build: String,
}

#[derive(Debug, Deserialize)]
struct FamilyWorklist {
    schema_version: u16,
    game_build: String,
    summary: WorklistSummary,
    families: Vec<ScriptFamily>,
}

#[derive(Debug, Deserialize)]
struct WorklistSummary {
    candidate_rows: usize,
    formula_signatures: usize,
}

#[derive(Debug, Deserialize)]
struct ScriptFamily {
    damage_script: String,
    summary: ScriptFamilySummary,
    formula_signatures: Vec<FormulaSignatureGroup>,
}

#[derive(Debug, Deserialize)]
struct ScriptFamilySummary {
    candidate_rows: usize,
    formula_signatures: usize,
}

#[derive(Debug, Deserialize)]
struct FormulaSignatureGroup {
    candidate_rows: usize,
    signature: FormulaSignature,
}

#[derive(Debug, Deserialize)]
struct FormulaSignature {
    damage_script: String,
    type_enum: Option<i64>,
    damage_type: Option<i64>,
    coefficient_basis_points_by_stage: Vec<i64>,
    fixed_parameter_by_level: Vec<i64>,
    pve_loop_time: Option<i64>,
    pve_stunned_damage: Vec<i64>,
    pve_extinction_damage: Option<i64>,
    part_damage_radio: Vec<i64>,
    abnormal_damage_json: String,
    damage_property: Option<i64>,
    part_damage_type: Option<i64>,
    damage_weight_json: String,
    row_level: Option<i64>,
    tags: Vec<i64>,
    behit_light_is_open: Option<bool>,
    is_profession: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct Il2CppSurface {
    schema_version: u16,
    build_id: String,
    source_identity: Value,
    types: Vec<TypeSurface>,
}

#[derive(Debug, Deserialize)]
struct TypeSurface {
    name: String,
    fields: Vec<String>,
    properties: Vec<String>,
}

#[derive(Debug, Serialize)]
struct StaticInputInventory {
    schema_version: u16,
    game_build: String,
    generated_by: &'static str,
    promotion_state: &'static str,
    inputs: Vec<InputArtifact>,
    source_identity: Value,
    policy: InventoryPolicy,
    summary: InventorySummary,
    exact_client_surfaces: ClientSurfaces,
    families: Vec<FamilyInventory>,
}

#[derive(Debug, Serialize)]
struct InputArtifact {
    role: &'static str,
    file: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct InventoryPolicy {
    runtime_formula_authority: bool,
    server_operator_implementation_present: bool,
    unresolved_evidence_hidden: bool,
    static_field_values_are_formula_operators: bool,
    static_field_role: &'static str,
    grouping_role: &'static str,
    packet_role: &'static str,
    promotion_rule: &'static str,
}

#[derive(Debug, Serialize)]
struct InventorySummary {
    script_families: usize,
    candidate_rows: usize,
    full_semantic_signatures: usize,
    static_fields_per_signature: usize,
    sync_damage_fields: usize,
    damage_attr_properties: usize,
}

#[derive(Debug, Serialize)]
struct ClientSurfaces {
    sync_damage_info_fields: Vec<String>,
    damage_attr_table_properties: Vec<String>,
}

#[derive(Debug, Serialize)]
struct FamilyInventory {
    damage_script: String,
    candidate_rows: usize,
    full_semantic_signatures: usize,
    operator_proof_state: &'static str,
    static_field_distributions: BTreeMap<&'static str, FieldDistribution>,
    next_exact_evidence: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct FieldDistribution {
    evidence_role: &'static str,
    candidate_rows: usize,
    non_default_candidate_rows: usize,
    distinct_values: usize,
    most_common_values: Vec<ValueFrequency>,
    omitted_distinct_values: usize,
}

#[derive(Debug, Serialize)]
struct ValueFrequency {
    value_json: String,
    candidate_rows: usize,
}

#[derive(Debug, Default)]
struct FieldAccumulator {
    candidate_rows: usize,
    non_default_candidate_rows: usize,
    values: BTreeMap<String, usize>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("damage script static-input inventory failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = arguments()?;
    let worklist: FamilyWorklist = read_json(&args.worklist)?;
    let il2cpp: Il2CppSurface = read_json(&args.il2cpp_surface)?;
    validate_inputs(&worklist, &il2cpp, &args.build)?;

    let sync_damage = find_type(&il2cpp, "SyncDamageInfo")?;
    let damage_attr = find_type(&il2cpp, "DamageAttrTableBase")?;
    let mut family_candidate_rows = 0_usize;
    let mut family_signature_count = 0_usize;
    let mut families = Vec::with_capacity(worklist.families.len());
    for family in worklist.families {
        let inventory = inventory_family(family)?;
        family_candidate_rows += inventory.candidate_rows;
        family_signature_count += inventory.full_semantic_signatures;
        families.push(inventory);
    }
    if family_candidate_rows != worklist.summary.candidate_rows
        || family_signature_count != worklist.summary.formula_signatures
    {
        return Err("family inventory does not conserve worklist rows and signatures".into());
    }

    let report = StaticInputInventory {
        schema_version: SCHEMA_VERSION,
        game_build: args.build,
        generated_by: "rlogs-bpsr-damage-script-static-input-inventory",
        promotion_state: "research-only-server-operator-and-same-build-replay-required",
        inputs: vec![
            input_artifact("damage-script-family-worklist", &args.worklist)?,
            input_artifact("current-build-il2cpp-combat-surface", &args.il2cpp_surface)?,
        ],
        source_identity: il2cpp.source_identity.clone(),
        policy: InventoryPolicy {
            runtime_formula_authority: false,
            server_operator_implementation_present: false,
            unresolved_evidence_hidden: false,
            static_field_values_are_formula_operators: false,
            static_field_role: "exact current-build candidate inputs and branch metadata; a value's presence does not prove how a server DamageScript consumes it",
            grouping_role: "full semantic signatures retain every decoded DamageAttr distinction and are never treated as proven mathematical equivalence",
            packet_role: "SyncDamageInfo supplies the authoritative result and exact owner, level, stage, hit, source, property, part, summoner, passive, weight, and mode observations available for replay",
            promotion_rule: "resolve each family only from same-build packet occurrences with source and target state snapshots, isolated state changes, exact output conservation, and provider-recipient scope proof",
        },
        summary: InventorySummary {
            script_families: families.len(),
            candidate_rows: family_candidate_rows,
            full_semantic_signatures: family_signature_count,
            static_fields_per_signature: 17,
            sync_damage_fields: sync_damage.fields.len(),
            damage_attr_properties: damage_attr.properties.len(),
        },
        exact_client_surfaces: ClientSurfaces {
            sync_damage_info_fields: sync_damage.fields.clone(),
            damage_attr_table_properties: damage_attr.properties.clone(),
        },
        families,
    };

    let mut writer = BufWriter::new(File::create(&args.output)?);
    serde_json::to_writer_pretty(&mut writer, &report)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    eprintln!(
        "wrote {} exact script families, {} full semantic signatures, and {} conserved candidate rows",
        report.summary.script_families,
        report.summary.full_semantic_signatures,
        report.summary.candidate_rows
    );
    Ok(())
}

fn inventory_family(family: ScriptFamily) -> Result<FamilyInventory, String> {
    if family.formula_signatures.len() != family.summary.formula_signatures {
        return Err(format!(
            "family {} signature count differs from its summary",
            family.damage_script
        ));
    }
    let mut fields = field_accumulators();
    let mut candidate_rows = 0_usize;
    for group in family.formula_signatures {
        if group.signature.damage_script != family.damage_script {
            return Err(format!(
                "family {} contains signature for {}",
                family.damage_script, group.signature.damage_script
            ));
        }
        candidate_rows += group.candidate_rows;
        observe_signature(&mut fields, &group.signature, group.candidate_rows)?;
    }
    if candidate_rows != family.summary.candidate_rows {
        return Err(format!(
            "family {} candidate count differs from its summary",
            family.damage_script
        ));
    }
    Ok(FamilyInventory {
        damage_script: family.damage_script,
        candidate_rows,
        full_semantic_signatures: family.summary.formula_signatures,
        operator_proof_state: "server-implementation-absent-from-current-client",
        static_field_distributions: fields
            .into_iter()
            .map(|(name, accumulator)| (name, accumulator.finish(field_role(name))))
            .collect(),
        next_exact_evidence: vec![
            "same-build-packet-occurrence",
            "exact-damage-source-route",
            "source-and-target-attribute-snapshots",
            "owner-level-and-stage-selection",
            "status-and-HP-shield-lifecycle",
            "isolated-input-delta",
            "rounding-and-mitigation-order",
            "canonical-output-conservation",
            "provider-recipient-and-self-only-scope-before-rdps",
        ],
    })
}

fn field_accumulators() -> BTreeMap<&'static str, FieldAccumulator> {
    [
        "type_enum",
        "damage_type",
        "coefficient_basis_points_by_stage",
        "fixed_parameter_by_level",
        "pve_loop_time",
        "pve_stunned_damage",
        "pve_extinction_damage",
        "part_damage_radio",
        "abnormal_damage",
        "damage_property",
        "part_damage_type",
        "damage_weight",
        "row_level",
        "tags",
        "behit_light_is_open",
        "is_profession",
    ]
    .into_iter()
    .map(|name| (name, FieldAccumulator::default()))
    .collect()
}

fn observe_signature(
    fields: &mut BTreeMap<&'static str, FieldAccumulator>,
    signature: &FormulaSignature,
    count: usize,
) -> Result<(), String> {
    observe(
        fields,
        "type_enum",
        &signature.type_enum,
        count,
        signature.type_enum.unwrap_or(0) != 0,
    )?;
    observe(
        fields,
        "damage_type",
        &signature.damage_type,
        count,
        signature.damage_type.unwrap_or(0) != 0,
    )?;
    observe(
        fields,
        "coefficient_basis_points_by_stage",
        &signature.coefficient_basis_points_by_stage,
        count,
        !signature.coefficient_basis_points_by_stage.is_empty(),
    )?;
    observe(
        fields,
        "fixed_parameter_by_level",
        &signature.fixed_parameter_by_level,
        count,
        !signature.fixed_parameter_by_level.is_empty(),
    )?;
    observe(
        fields,
        "pve_loop_time",
        &signature.pve_loop_time,
        count,
        !matches!(signature.pve_loop_time, None | Some(0)),
    )?;
    observe(
        fields,
        "pve_stunned_damage",
        &signature.pve_stunned_damage,
        count,
        !signature.pve_stunned_damage.is_empty(),
    )?;
    observe(
        fields,
        "pve_extinction_damage",
        &signature.pve_extinction_damage,
        count,
        signature.pve_extinction_damage.unwrap_or(0) != 0,
    )?;
    observe(
        fields,
        "part_damage_radio",
        &signature.part_damage_radio,
        count,
        !signature.part_damage_radio.is_empty(),
    )?;
    observe_encoded(
        fields,
        "abnormal_damage",
        &signature.abnormal_damage_json,
        count,
    )?;
    observe(
        fields,
        "damage_property",
        &signature.damage_property,
        count,
        signature.damage_property.unwrap_or(0) != 0,
    )?;
    observe(
        fields,
        "part_damage_type",
        &signature.part_damage_type,
        count,
        signature.part_damage_type.unwrap_or(0) != 0,
    )?;
    observe_encoded(
        fields,
        "damage_weight",
        &signature.damage_weight_json,
        count,
    )?;
    observe(
        fields,
        "row_level",
        &signature.row_level,
        count,
        signature.row_level.unwrap_or(0) != 0,
    )?;
    observe(
        fields,
        "tags",
        &signature.tags,
        count,
        !signature.tags.is_empty(),
    )?;
    observe(
        fields,
        "behit_light_is_open",
        &signature.behit_light_is_open,
        count,
        signature.behit_light_is_open.unwrap_or(false),
    )?;
    observe(
        fields,
        "is_profession",
        &signature.is_profession,
        count,
        signature.is_profession.unwrap_or(false),
    )?;
    Ok(())
}

fn observe<T: Serialize>(
    fields: &mut BTreeMap<&'static str, FieldAccumulator>,
    name: &'static str,
    value: &T,
    count: usize,
    non_default: bool,
) -> Result<(), String> {
    let encoded =
        serde_json::to_string(value).map_err(|error| format!("cannot encode {name}: {error}"))?;
    fields
        .get_mut(name)
        .ok_or_else(|| format!("unknown field accumulator {name}"))?
        .observe(encoded, count, non_default);
    Ok(())
}

fn observe_encoded(
    fields: &mut BTreeMap<&'static str, FieldAccumulator>,
    name: &'static str,
    encoded: &str,
    count: usize,
) -> Result<(), String> {
    let parsed: Value = serde_json::from_str(encoded)
        .map_err(|error| format!("{name} is not canonical JSON: {error}"))?;
    let non_default = !matches!(&parsed, Value::Null)
        && !matches!(&parsed, Value::Array(values) if values.is_empty())
        && !matches!(&parsed, Value::Number(value) if value.as_i64() == Some(0))
        && !matches!(&parsed, Value::Bool(false));
    fields
        .get_mut(name)
        .ok_or_else(|| format!("unknown field accumulator {name}"))?
        .observe(encoded.to_owned(), count, non_default);
    Ok(())
}

impl FieldAccumulator {
    fn observe(&mut self, value: String, count: usize, non_default: bool) {
        self.candidate_rows += count;
        self.non_default_candidate_rows += usize::from(non_default) * count;
        *self.values.entry(value).or_default() += count;
    }

    fn finish(self, evidence_role: &'static str) -> FieldDistribution {
        let distinct_values = self.values.len();
        let mut values = self.values.into_iter().collect::<Vec<_>>();
        values.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        let most_common_values = values
            .into_iter()
            .take(VALUE_EXAMPLE_LIMIT)
            .map(|(value_json, candidate_rows)| ValueFrequency {
                value_json,
                candidate_rows,
            })
            .collect::<Vec<_>>();
        FieldDistribution {
            evidence_role,
            candidate_rows: self.candidate_rows,
            non_default_candidate_rows: self.non_default_candidate_rows,
            distinct_values,
            omitted_distinct_values: distinct_values.saturating_sub(most_common_values.len()),
            most_common_values,
        }
    }
}

fn field_role(name: &str) -> &'static str {
    match name {
        "row_level" => "selection-identity-retained-not-operator-proof",
        "type_enum" => "classification-or-selection-identity-role-unproven",
        _ => "decoded-current-build-semantic-candidate-not-operator-proof",
    }
}

fn find_type<'a>(surface: &'a Il2CppSurface, name: &str) -> Result<&'a TypeSurface, String> {
    surface
        .types
        .iter()
        .find(|surface| surface.name == name)
        .ok_or_else(|| format!("IL2CPP surface is missing {name}"))
}

fn validate_inputs(
    worklist: &FamilyWorklist,
    il2cpp: &Il2CppSurface,
    build: &str,
) -> Result<(), String> {
    if worklist.schema_version < 2 {
        return Err(format!(
            "family worklist schema {} lacks the full decoded DamageAttr surface",
            worklist.schema_version
        ));
    }
    if il2cpp.schema_version < 2 {
        return Err(format!(
            "IL2CPP surface schema {} lacks DamageAttrTableBase",
            il2cpp.schema_version
        ));
    }
    if worklist.game_build != build || il2cpp.build_id != build {
        return Err(format!(
            "build mismatch: requested {build}, worklist {}, IL2CPP {}",
            worklist.game_build, il2cpp.build_id
        ));
    }
    Ok(())
}

fn arguments() -> Result<Arguments, String> {
    let values = env::args().skip(1).collect::<Vec<_>>();
    if values.len() != 8 {
        return Err(usage());
    }
    let build = option(&values, "--build")?.to_owned();
    if build.is_empty() || !build.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("--build requires a numeric client build".to_owned());
    }
    Ok(Arguments {
        worklist: PathBuf::from(option(&values, "--worklist")?),
        il2cpp_surface: PathBuf::from(option(&values, "--il2cpp-surface")?),
        output: PathBuf::from(option(&values, "--output")?),
        build,
    })
}

fn option<'a>(arguments: &'a [String], name: &str) -> Result<&'a str, String> {
    arguments
        .iter()
        .position(|argument| argument == name)
        .and_then(|index| arguments.get(index + 1))
        .map(String::as_str)
        .ok_or_else(usage)
}

fn usage() -> String {
    "usage: rlogs-bpsr-damage-script-static-input-inventory --worklist <damage-script-family-worklist.json> --il2cpp-surface <current-build-il2cpp-combat-surface.json> --build <numeric-client-build> --output <inventory.json>".to_owned()
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, Box<dyn std::error::Error>> {
    Ok(serde_json::from_reader(BufReader::new(File::open(path)?))?)
}

fn input_artifact(
    role: &'static str,
    path: &Path,
) -> Result<InputArtifact, Box<dyn std::error::Error>> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut hasher = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        bytes += read as u64;
    }
    Ok(InputArtifact {
        role,
        file: path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("unknown")
            .to_owned(),
        bytes,
        sha256: format!("{:x}", hasher.finalize()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_accumulator_weights_candidate_rows() {
        let mut accumulator = FieldAccumulator::default();
        accumulator.observe("[10000]".to_owned(), 3, true);
        accumulator.observe("[]".to_owned(), 2, false);
        let distribution = accumulator.finish("test");
        assert_eq!(distribution.candidate_rows, 5);
        assert_eq!(distribution.non_default_candidate_rows, 3);
        assert_eq!(distribution.distinct_values, 2);
        assert_eq!(distribution.most_common_values[0].candidate_rows, 3);
    }

    #[test]
    fn encoded_empty_arrays_are_default_but_nested_values_are_retained() {
        let mut fields = BTreeMap::from([("abnormal_damage", FieldAccumulator::default())]);
        observe_encoded(&mut fields, "abnormal_damage", "[]", 2).unwrap();
        observe_encoded(&mut fields, "abnormal_damage", "[[0]]", 3).unwrap();
        let distribution = fields.remove("abnormal_damage").unwrap().finish("test");
        assert_eq!(distribution.candidate_rows, 5);
        assert_eq!(distribution.non_default_candidate_rows, 3);
    }
}
