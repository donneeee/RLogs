use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Serialize)]
struct Report {
    schema_version: u16,
    game_build: String,
    generated_by: &'static str,
    promotion_state: &'static str,
    inputs: Vec<InputArtifact>,
    policy: Policy,
    summary: Summary,
    effects: BTreeMap<String, EffectReport>,
}

#[derive(Debug, Serialize)]
struct InputArtifact {
    role: &'static str,
    file: String,
    bytes: usize,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct Policy {
    runtime_formula_authority: bool,
    exact_build_table_required: bool,
    packet_observation_is_authoritative: bool,
    localization_is_formula_authority: bool,
    automatically_ignored_statuses: usize,
    unresolved_effects_hidden: bool,
    classification_rule: &'static str,
}

#[derive(Debug, Serialize)]
struct Summary {
    mismatch_records: usize,
    distinct_effects: usize,
    candidate_pair_mentions: u64,
    effects_with_exact_buff_row: usize,
    effects_with_exact_source_config_buff_row: usize,
    effects_with_packet_origin: usize,
    effects_with_cross_actor_windows: usize,
    effects_with_observed_attribute_evidence: usize,
    effects_without_any_semantic_bridge: usize,
}

#[derive(Debug, Serialize)]
struct EffectReport {
    effect_id: i64,
    candidate_pair_mentions: u64,
    mismatch_records: usize,
    abilities: Vec<i64>,
    owner_sides: Vec<String>,
    directions: Vec<String>,
    source_relations: Vec<String>,
    observed_source_config_ids: Vec<i64>,
    exact_buff_row: Option<BuffSemantic>,
    exact_source_config_buff_rows: BTreeMap<String, BuffSemantic>,
    packet_origin: Option<PacketOriginSummary>,
    observed_attribute_evidence: Vec<AttributeEvidence>,
    safe_to_ignore_for_formula_pairing: bool,
    unresolved_reason: &'static str,
}

#[derive(Debug, Serialize)]
struct BuffSemantic {
    id: i64,
    name: Option<String>,
    description: Option<String>,
    design_name: Option<String>,
    note: Option<String>,
    icon: Option<String>,
    visible: Option<i64>,
    buff_type: Option<i64>,
    repeat_add_rule: Vec<Value>,
    tags: Vec<Value>,
    special_attributes: Vec<Value>,
}

#[derive(Debug, Serialize)]
struct PacketOriginSummary {
    status_events: u64,
    window_count: u64,
    cross_actor_window_count: u64,
    packet_origin_observations: u64,
    source_relation_count: u64,
    observed_sessions: Vec<String>,
}

#[derive(Debug, Serialize)]
struct AttributeEvidence {
    attribute_id: i64,
    transitions_examined: u64,
    isolated_transitions: u64,
    complete_before_and_after: u64,
    aggregates: Vec<AggregateEvidence>,
}

#[derive(Debug, Serialize)]
struct AggregateEvidence {
    state: Option<String>,
    raw_delta_units: Option<i64>,
    isolated: Option<bool>,
    provider_resolution: Option<String>,
    provider_kind: Option<String>,
    provider_is_target: Option<bool>,
    count: u64,
}

#[derive(Default)]
struct EffectAccumulator {
    candidate_pair_mentions: u64,
    mismatch_records: usize,
    abilities: BTreeSet<i64>,
    owner_sides: BTreeSet<String>,
    directions: BTreeSet<String>,
    source_relations: BTreeSet<String>,
    source_config_ids: BTreeSet<i64>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("status mismatch semantic surface failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if arguments.len() != 12 {
        return Err(usage().into());
    }
    let mismatch_path = PathBuf::from(option(&arguments, "--mismatch-report")?);
    let buff_path = PathBuf::from(option(&arguments, "--buff-table")?);
    let origins_path = PathBuf::from(option(&arguments, "--status-origins")?);
    let matrix_path = PathBuf::from(option(&arguments, "--attribute-matrix")?);
    let game_build = option(&arguments, "--build")?.to_owned();
    let output_path = PathBuf::from(option(&arguments, "--output")?);
    if game_build.is_empty() || !game_build.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("--build requires a numeric client build".into());
    }

    let (mismatch_bytes, mismatch) = read_json(&mismatch_path)?;
    let (buff_bytes, buffs) = read_json(&buff_path)?;
    let (origins_bytes, origins) = read_json(&origins_path)?;
    let (matrix_bytes, matrix) = read_json(&matrix_path)?;
    let (effects, summary) = build_surface(&mismatch, &buffs, &origins, &matrix)?;
    let report = Report {
        schema_version: SCHEMA_VERSION,
        game_build,
        generated_by: "rlogs-bpsr-status-mismatch-semantic-surface",
        promotion_state: "offline_exact_build_diagnostic_inventory",
        inputs: vec![
            artifact(
                "component_aware_mismatch_report",
                &mismatch_path,
                &mismatch_bytes,
            ),
            artifact("exact_build_decoded_buff_table", &buff_path, &buff_bytes),
            artifact(
                "packet_observed_status_origins",
                &origins_path,
                &origins_bytes,
            ),
            artifact(
                "packet_observed_effect_attribute_matrix",
                &matrix_path,
                &matrix_bytes,
            ),
        ],
        policy: Policy {
            runtime_formula_authority: false,
            exact_build_table_required: true,
            packet_observation_is_authoritative: true,
            localization_is_formula_authority: false,
            automatically_ignored_statuses: 0,
            unresolved_effects_hidden: false,
            classification_rule: "retain every mismatched status; decoded names describe presentation only, while packet origins and isolated attribute transitions describe observed mechanics; no status becomes formula-inert without a separate causal proof",
        },
        summary,
        effects,
    };

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = fs::File::create(&output_path)?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, &report)?;
    writer.write_all(b"\n")?;
    eprintln!(
        "wrote {} retained mismatch effects ({} candidate-pair mentions) to {}",
        report.summary.distinct_effects,
        report.summary.candidate_pair_mentions,
        output_path.display()
    );
    Ok(())
}

fn build_surface(
    mismatch: &Value,
    buffs: &Value,
    origins: &Value,
    matrix: &Value,
) -> Result<(BTreeMap<String, EffectReport>, Summary), String> {
    let mismatch_rows = array(mismatch, "status_mismatch_inventory")?;
    let buff_rows = buffs
        .as_object()
        .ok_or_else(|| "BuffTable root must be an object".to_owned())?;
    let origin_rows = array(origins, "effects")?;
    let matrix_rows = array(matrix, "effects")?;
    let origins_by_id = index_by_id(origin_rows, "effect_id")?;
    let matrix_by_id = index_by_id(matrix_rows, "effect_id")?;

    let mut accumulators = BTreeMap::<i64, EffectAccumulator>::new();
    for row in mismatch_rows {
        let effect_id = required_i64(row, "effect_id")?;
        let entry = accumulators.entry(effect_id).or_default();
        entry.mismatch_records += 1;
        entry.candidate_pair_mentions = entry
            .candidate_pair_mentions
            .saturating_add(optional_u64(row, "candidate_occurrences").unwrap_or(0));
        insert_i64(row, "ability_id", &mut entry.abilities);
        insert_string(row, "owner_side", &mut entry.owner_sides);
        insert_string(row, "direction", &mut entry.directions);
        insert_string(row, "source_relation", &mut entry.source_relations);
        insert_i64(row, "source_config_id", &mut entry.source_config_ids);
    }

    let mut effects = BTreeMap::new();
    let mut effects_with_exact_buff_row = 0;
    let mut effects_with_exact_source_config_buff_row = 0;
    let mut effects_with_packet_origin = 0;
    let mut effects_with_cross_actor_windows = 0;
    let mut effects_with_observed_attribute_evidence = 0;
    let mut effects_without_any_semantic_bridge = 0;
    let candidate_pair_mentions = accumulators
        .values()
        .map(|entry| entry.candidate_pair_mentions)
        .sum();

    for (effect_id, accumulator) in accumulators {
        let exact_buff_row = buff_rows
            .get(&effect_id.to_string())
            .map(|row| buff_semantic(effect_id, row));
        if exact_buff_row.is_some() {
            effects_with_exact_buff_row += 1;
        }
        let mut source_rows = BTreeMap::new();
        for source_config_id in &accumulator.source_config_ids {
            if let Some(row) = buff_rows.get(&source_config_id.to_string()) {
                source_rows.insert(
                    source_config_id.to_string(),
                    buff_semantic(*source_config_id, row),
                );
            }
        }
        if !source_rows.is_empty() {
            effects_with_exact_source_config_buff_row += 1;
        }
        let packet_origin = origins_by_id
            .get(&effect_id)
            .map(|row| packet_origin_summary(row));
        if packet_origin.is_some() {
            effects_with_packet_origin += 1;
        }
        if packet_origin
            .as_ref()
            .is_some_and(|origin| origin.cross_actor_window_count > 0)
        {
            effects_with_cross_actor_windows += 1;
        }
        let observed_attribute_evidence = matrix_by_id
            .get(&effect_id)
            .map(|row| attribute_evidence(row))
            .unwrap_or_default();
        if !observed_attribute_evidence.is_empty() {
            effects_with_observed_attribute_evidence += 1;
        }
        if exact_buff_row.is_none()
            && source_rows.is_empty()
            && packet_origin.is_none()
            && observed_attribute_evidence.is_empty()
        {
            effects_without_any_semantic_bridge += 1;
        }
        effects.insert(
            effect_id.to_string(),
            EffectReport {
                effect_id,
                candidate_pair_mentions: accumulator.candidate_pair_mentions,
                mismatch_records: accumulator.mismatch_records,
                abilities: accumulator.abilities.into_iter().collect(),
                owner_sides: accumulator.owner_sides.into_iter().collect(),
                directions: accumulator.directions.into_iter().collect(),
                source_relations: accumulator.source_relations.into_iter().collect(),
                observed_source_config_ids: accumulator.source_config_ids.into_iter().collect(),
                exact_buff_row,
                exact_source_config_buff_rows: source_rows,
                packet_origin,
                observed_attribute_evidence,
                safe_to_ignore_for_formula_pairing: false,
                unresolved_reason: "mechanic relevance has not been causally disproven; retain as a formula-state difference",
            },
        );
    }

    let summary = Summary {
        mismatch_records: mismatch_rows.len(),
        distinct_effects: effects.len(),
        candidate_pair_mentions,
        effects_with_exact_buff_row,
        effects_with_exact_source_config_buff_row,
        effects_with_packet_origin,
        effects_with_cross_actor_windows,
        effects_with_observed_attribute_evidence,
        effects_without_any_semantic_bridge,
    };
    Ok((effects, summary))
}

fn attribute_evidence(row: &Value) -> Vec<AttributeEvidence> {
    row.get("attributes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|attribute| {
            let isolated = optional_u64(attribute, "isolated_transitions").unwrap_or(0);
            let aggregates = attribute
                .get("aggregates")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|aggregate| AggregateEvidence {
                    state: optional_string(aggregate, "state"),
                    raw_delta_units: optional_i64(aggregate, "raw_delta_units"),
                    isolated: aggregate.get("isolated").and_then(Value::as_bool),
                    provider_resolution: optional_string(aggregate, "provider_resolution"),
                    provider_kind: optional_string(aggregate, "provider_kind"),
                    provider_is_target: aggregate
                        .get("provider_is_target")
                        .and_then(Value::as_bool),
                    count: optional_u64(aggregate, "count").unwrap_or(0),
                })
                .collect::<Vec<_>>();
            if isolated == 0 && aggregates.is_empty() {
                return None;
            }
            Some(AttributeEvidence {
                attribute_id: required_i64(attribute, "attribute_id").ok()?,
                transitions_examined: optional_u64(attribute, "transitions_examined").unwrap_or(0),
                isolated_transitions: isolated,
                complete_before_and_after: optional_u64(attribute, "complete_before_and_after")
                    .unwrap_or(0),
                aggregates,
            })
        })
        .collect()
}

fn buff_semantic(id: i64, row: &Value) -> BuffSemantic {
    BuffSemantic {
        id,
        name: optional_nonempty_string(row, "Name"),
        description: optional_nonempty_string(row, "Desc"),
        design_name: optional_nonempty_string(row, "NameDesign"),
        note: optional_nonempty_string(row, "Note"),
        icon: optional_nonempty_string(row, "Icon"),
        visible: optional_i64(row, "Visible"),
        buff_type: optional_i64(row, "BuffType"),
        repeat_add_rule: row
            .get("RepeatAddRule")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        tags: row
            .get("Tags")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        special_attributes: row
            .get("SpecialAttr")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
    }
}

fn packet_origin_summary(row: &Value) -> PacketOriginSummary {
    PacketOriginSummary {
        status_events: optional_u64(row, "status_events").unwrap_or(0),
        window_count: optional_u64(row, "window_count").unwrap_or(0),
        cross_actor_window_count: optional_u64(row, "cross_actor_window_count").unwrap_or(0),
        packet_origin_observations: optional_u64(row, "packet_origin_observations").unwrap_or(0),
        source_relation_count: optional_u64(row, "source_relation_count").unwrap_or(0),
        observed_sessions: row
            .get("observed_sessions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
    }
}

fn index_by_id<'a>(rows: &'a [Value], field: &str) -> Result<BTreeMap<i64, &'a Value>, String> {
    let mut index = BTreeMap::new();
    for row in rows {
        let id = required_i64(row, field)?;
        if index.insert(id, row).is_some() {
            return Err(format!("duplicate {field} {id}"));
        }
    }
    Ok(index)
}

fn array<'a>(root: &'a Value, field: &str) -> Result<&'a [Value], String> {
    root.get(field)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| format!("missing array {field}"))
}

fn required_i64(row: &Value, field: &str) -> Result<i64, String> {
    row.get(field)
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("missing integer {field} in {}", json!(row)))
}

fn optional_i64(row: &Value, field: &str) -> Option<i64> {
    row.get(field).and_then(Value::as_i64)
}

fn optional_u64(row: &Value, field: &str) -> Option<u64> {
    row.get(field).and_then(Value::as_u64)
}

fn optional_string(row: &Value, field: &str) -> Option<String> {
    row.get(field).and_then(Value::as_str).map(str::to_owned)
}

fn optional_nonempty_string(row: &Value, field: &str) -> Option<String> {
    optional_string(row, field).filter(|value| !value.is_empty())
}

fn insert_i64(row: &Value, field: &str, destination: &mut BTreeSet<i64>) {
    if let Some(value) = optional_i64(row, field) {
        destination.insert(value);
    }
}

fn insert_string(row: &Value, field: &str, destination: &mut BTreeSet<String>) {
    if let Some(value) = optional_string(row, field) {
        destination.insert(value);
    }
}

fn read_json(path: &Path) -> Result<(Vec<u8>, Value), Box<dyn std::error::Error>> {
    let bytes = fs::read(path)?;
    let value = serde_json::from_slice(&bytes)?;
    Ok((bytes, value))
}

fn artifact(role: &'static str, path: &Path, bytes: &[u8]) -> InputArtifact {
    InputArtifact {
        role,
        file: display_path(path),
        bytes: bytes.len(),
        sha256: hex_digest(bytes),
    }
}

fn option<'a>(arguments: &'a [String], name: &str) -> Result<&'a str, String> {
    arguments
        .windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str())
        .ok_or_else(|| format!("missing {name}; {}", usage()))
}

fn usage() -> &'static str {
    "usage: rlogs-bpsr-status-mismatch-semantic-surface --mismatch-report <external-proof.json> --buff-table <BuffTable.json> --status-origins <origins.json> --attribute-matrix <matrix.json> --build <numeric-build> --output <surface.json>"
}

fn display_path(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}

fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retains_every_mismatch_and_never_marks_it_ignorable() {
        let mismatch = json!({"status_mismatch_inventory": [
            {"effect_id": 20, "ability_id": 3, "candidate_occurrences": 4, "owner_side": "source", "direction": "active_only", "source_relation": "provider", "source_config_id": 10},
            {"effect_id": 20, "ability_id": 4, "candidate_occurrences": 5, "owner_side": "target", "direction": "inactive_only", "source_relation": "recipient", "source_config_id": 11}
        ]});
        let buffs = json!({
            "20": {"Name": "Effect", "Desc": "desc", "RepeatAddRule": [], "Tags": [], "SpecialAttr": []},
            "10": {"Name": "Source", "Desc": "source", "RepeatAddRule": [], "Tags": [], "SpecialAttr": []}
        });
        let origins = json!({"effects": [{"effect_id": 20, "status_events": 8, "window_count": 2, "cross_actor_window_count": 1, "packet_origin_observations": 2, "source_relation_count": 1, "observed_sessions": ["s"]}]});
        let matrix = json!({"effects": [{"effect_id": 20, "attributes": [{"attribute_id": 11330, "transitions_examined": 2, "complete_before_and_after": 2, "isolated_transitions": 1, "aggregates": [{"state": "applied", "raw_delta_units": 360, "isolated": true, "count": 1}]}]}]});
        let (effects, summary) = build_surface(&mismatch, &buffs, &origins, &matrix).unwrap();
        assert_eq!(summary.mismatch_records, 2);
        assert_eq!(summary.distinct_effects, 1);
        assert_eq!(summary.candidate_pair_mentions, 9);
        let effect = &effects["20"];
        assert_eq!(effect.abilities, vec![3, 4]);
        assert_eq!(effect.observed_source_config_ids, vec![10, 11]);
        assert_eq!(effect.exact_source_config_buff_rows.len(), 1);
        assert_eq!(effect.observed_attribute_evidence.len(), 1);
        assert!(!effect.safe_to_ignore_for_formula_pairing);
    }

    #[test]
    fn retains_an_effect_without_any_bridge() {
        let mismatch =
            json!({"status_mismatch_inventory": [{"effect_id": 99, "candidate_occurrences": 1}]});
        let (effects, summary) = build_surface(
            &mismatch,
            &json!({}),
            &json!({"effects": []}),
            &json!({"effects": []}),
        )
        .unwrap();
        assert_eq!(summary.effects_without_any_semantic_bridge, 1);
        assert!(!effects["99"].safe_to_ignore_for_formula_pairing);
    }
}
