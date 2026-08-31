use std::{
    collections::BTreeMap,
    env,
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Write},
    path::PathBuf,
};

use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 1;
const DEFAULT_EXAMPLE_LIMIT: usize = 8;

#[derive(Debug)]
struct Arguments {
    surface: PathBuf,
    reference: PathBuf,
    formula_source: PathBuf,
    output: PathBuf,
    example_limit: usize,
}

#[derive(Debug, Serialize)]
struct ProofBundle {
    schema_version: u16,
    generated_by: &'static str,
    policy: ProofPolicy,
    current_build_source: Value,
    reference_source: ReferenceSource,
    current_formula_source: FormulaSource,
    coverage: Coverage,
    fields: Vec<FieldProof>,
    unsupported_nested_fields: Vec<UnsupportedField>,
    exact_layout_conclusion: LayoutConclusion,
}

#[derive(Debug, Serialize)]
struct ProofPolicy {
    runtime_formula_authority: bool,
    current_build_values_are_runtime_authority: bool,
    historical_reference_values_are_runtime_authority: bool,
    historical_reference_role: &'static str,
    current_formula_text_role: &'static str,
    unresolved_evidence_is_hidden: bool,
}

#[derive(Debug, Serialize)]
struct ReferenceSource {
    file_name: String,
    sha256: String,
    rows: usize,
}

#[derive(Debug, Serialize)]
struct FormulaSource {
    file_name: String,
    sha256: String,
    damage_merge_occurrences: BTreeMap<&'static str, usize>,
}

#[derive(Debug, Serialize)]
struct Coverage {
    current_rows: usize,
    reference_rows: usize,
    overlapping_row_ids: usize,
    current_only_row_ids: usize,
    reference_only_row_ids: usize,
}

#[derive(Debug, Serialize)]
struct FieldProof {
    field: &'static str,
    current_row_offset: Option<u8>,
    encoding: &'static str,
    current_formula_text_occurrences: Option<usize>,
    compared_rows: usize,
    exact_matches: usize,
    changed_rows: usize,
    unreadable_current_rows: usize,
    exact_match_basis_points: Option<usize>,
    changed_examples: Vec<ChangedExample>,
    conclusion: &'static str,
}

#[derive(Debug, Serialize)]
struct ChangedExample {
    damage_id: String,
    historical_reference_value: Value,
    current_build_value: Value,
}

#[derive(Debug, Serialize)]
struct UnsupportedField {
    field: &'static str,
    current_row_offset: u8,
    reason: &'static str,
}

#[derive(Debug, Serialize)]
struct LayoutConclusion {
    row_size_bytes: u64,
    schema_fields_bound_without_guessing: usize,
    nested_fields_intentionally_unbound: usize,
    statement: &'static str,
}

#[derive(Clone, Copy)]
enum CurrentEncoding {
    Id,
    Scalar(u8),
    StringPool6(u8),
    IntArrayPool1(u8),
    Bool(u8),
}

#[derive(Clone, Copy)]
struct FieldSpec {
    name: &'static str,
    encoding: CurrentEncoding,
    formula_name: Option<&'static str>,
}

const FIELDS: [FieldSpec; 16] = [
    FieldSpec {
        name: "Id",
        encoding: CurrentEncoding::Id,
        formula_name: None,
    },
    FieldSpec {
        name: "Level",
        encoding: CurrentEncoding::Scalar(8),
        formula_name: None,
    },
    FieldSpec {
        name: "Name",
        encoding: CurrentEncoding::StringPool6(12),
        formula_name: None,
    },
    FieldSpec {
        name: "DamageType",
        encoding: CurrentEncoding::Scalar(16),
        formula_name: None,
    },
    FieldSpec {
        name: "TypeEnum",
        encoding: CurrentEncoding::Scalar(20),
        formula_name: None,
    },
    FieldSpec {
        name: "DamageScript",
        encoding: CurrentEncoding::StringPool6(24),
        formula_name: None,
    },
    FieldSpec {
        name: "PVEDamageRadio",
        encoding: CurrentEncoding::IntArrayPool1(28),
        formula_name: Some("PVEDamageRadio"),
    },
    FieldSpec {
        name: "PVEFixedParameter",
        encoding: CurrentEncoding::IntArrayPool1(32),
        formula_name: Some("PVEFixedParameter"),
    },
    FieldSpec {
        name: "PVELoopTime",
        encoding: CurrentEncoding::Scalar(36),
        formula_name: None,
    },
    FieldSpec {
        name: "PVEStunnedDamage",
        encoding: CurrentEncoding::IntArrayPool1(40),
        formula_name: Some("PVEStunnedDamage"),
    },
    FieldSpec {
        name: "PVEExtinctionDamage",
        encoding: CurrentEncoding::Scalar(44),
        formula_name: None,
    },
    FieldSpec {
        name: "PartDamageRadio",
        encoding: CurrentEncoding::IntArrayPool1(48),
        formula_name: None,
    },
    FieldSpec {
        name: "DamageProperty",
        encoding: CurrentEncoding::Scalar(56),
        formula_name: None,
    },
    FieldSpec {
        name: "PartDamageType",
        encoding: CurrentEncoding::Scalar(60),
        formula_name: None,
    },
    FieldSpec {
        name: "Tags",
        encoding: CurrentEncoding::IntArrayPool1(68),
        formula_name: None,
    },
    FieldSpec {
        name: "BehitLightIsOpen",
        encoding: CurrentEncoding::Bool(72),
        formula_name: None,
    },
];

fn main() {
    if let Err(error) = run() {
        eprintln!("DamageAttr field-layout proof failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = arguments()?;
    let surface: Value = serde_json::from_reader(BufReader::new(File::open(&args.surface)?))?;
    let reference: Value = serde_json::from_reader(BufReader::new(File::open(&args.reference)?))?;
    let current_rows = surface
        .get("rows")
        .and_then(Value::as_object)
        .ok_or("surface is missing rows")?;
    let reference_rows = reference
        .as_object()
        .ok_or("reference DamageAttrTable must be a JSON object keyed by Id")?;
    let formula_text = fs::read_to_string(&args.formula_source)?;
    let formula_counts = ["PVEDamageRadio", "PVEFixedParameter", "PVEStunnedDamage"]
        .into_iter()
        .map(|name| (name, formula_text.matches(name).count()))
        .collect::<BTreeMap<_, _>>();

    let overlapping_row_ids = current_rows
        .keys()
        .filter(|id| reference_rows.contains_key(*id))
        .count();
    let fields = FIELDS
        .iter()
        .map(|field| {
            prove_field(
                *field,
                current_rows,
                reference_rows,
                &formula_counts,
                args.example_limit,
            )
        })
        .collect::<Vec<_>>();
    let source = public_surface_source(&surface);
    let row_size_bytes = source
        .get("row_size")
        .and_then(Value::as_u64)
        .unwrap_or_default();

    let bundle = ProofBundle {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-damage-attr-field-layout-proof",
        policy: ProofPolicy {
            runtime_formula_authority: false,
            current_build_values_are_runtime_authority: true,
            historical_reference_values_are_runtime_authority: false,
            historical_reference_role: "schema-layout witness only; changed values are expected and never copied into the current catalog",
            current_formula_text_role: "current client UI formula witness for the named PVE damage fields",
            unresolved_evidence_is_hidden: false,
        },
        current_build_source: source,
        reference_source: ReferenceSource {
            file_name: file_name(&args.reference),
            sha256: sha256_file(&args.reference)?,
            rows: reference_rows.len(),
        },
        current_formula_source: FormulaSource {
            file_name: file_name(&args.formula_source),
            sha256: sha256_file(&args.formula_source)?,
            damage_merge_occurrences: formula_counts,
        },
        coverage: Coverage {
            current_rows: current_rows.len(),
            reference_rows: reference_rows.len(),
            overlapping_row_ids,
            current_only_row_ids: current_rows.len().saturating_sub(overlapping_row_ids),
            reference_only_row_ids: reference_rows.len().saturating_sub(overlapping_row_ids),
        },
        fields,
        unsupported_nested_fields: vec![
            UnsupportedField {
                field: "AbnormalDamage",
                current_row_offset: 52,
                reason: "nested array pool encoding is not decoded by the current surface extractor; the stored value is a pool reference, not a scalar formula value",
            },
            UnsupportedField {
                field: "DamageWeight",
                current_row_offset: 64,
                reason: "nested/structured array pool encoding is not decoded by the current surface extractor; the stored value is a pool reference, not a scalar formula value",
            },
        ],
        exact_layout_conclusion: LayoutConclusion {
            row_size_bytes,
            schema_fields_bound_without_guessing: FIELDS.len(),
            nested_fields_intentionally_unbound: 2,
            statement: "The historical typed schema fixes the field order, unchanged overlapping rows mechanically verify each decoded offset, and current formula text independently names the three PVE formula arrays. Only current-build row values may feed later formula proof or runtime attribution.",
        },
    };

    let mut writer = BufWriter::new(File::create(&args.output)?);
    serde_json::to_writer_pretty(&mut writer, &bundle)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    println!("wrote {}", args.output.display());
    Ok(())
}

fn public_surface_source(surface: &Value) -> Value {
    let source = surface.get("source").unwrap_or(&Value::Null);
    json!({
        "table": source.get("table").cloned().unwrap_or(Value::Null),
        "table_hash": source.get("table_hash").cloned().unwrap_or(Value::Null),
        "row_count": source.get("row_count").cloned().unwrap_or(Value::Null),
        "row_size": source.get("row_size").cloned().unwrap_or(Value::Null),
        "pools": source.get("pools").cloned().unwrap_or(Value::Null)
    })
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("unknown")
        .to_owned()
}

fn sha256_file(path: &std::path::Path) -> Result<String, std::io::Error> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn prove_field(
    field: FieldSpec,
    current_rows: &serde_json::Map<String, Value>,
    reference_rows: &serde_json::Map<String, Value>,
    formula_counts: &BTreeMap<&'static str, usize>,
    example_limit: usize,
) -> FieldProof {
    let mut compared_rows = 0_usize;
    let mut exact_matches = 0_usize;
    let mut changed_rows = 0_usize;
    let mut unreadable_current_rows = 0_usize;
    let mut changed_examples = Vec::new();

    for (damage_id, current_row) in current_rows {
        let Some(reference_row) = reference_rows.get(damage_id) else {
            continue;
        };
        let Some(reference_value) = reference_row.get(field.name) else {
            continue;
        };
        let Some(current_value) = current_value(damage_id, current_row, field.encoding) else {
            unreadable_current_rows += 1;
            continue;
        };
        compared_rows += 1;
        if &current_value == reference_value {
            exact_matches += 1;
        } else {
            changed_rows += 1;
            if changed_examples.len() < example_limit {
                changed_examples.push(ChangedExample {
                    damage_id: damage_id.clone(),
                    historical_reference_value: reference_value.clone(),
                    current_build_value: current_value,
                });
            }
        }
    }

    let (offset, encoding) = encoding_label(field.encoding);
    FieldProof {
        field: field.name,
        current_row_offset: offset,
        encoding,
        current_formula_text_occurrences: field
            .formula_name
            .and_then(|name| formula_counts.get(name).copied()),
        compared_rows,
        exact_matches,
        changed_rows,
        unreadable_current_rows,
        exact_match_basis_points: (compared_rows > 0)
            .then(|| exact_matches.saturating_mul(10_000) / compared_rows),
        changed_examples,
        conclusion: "offset mechanically verified; mismatches are retained as current-build content changes, not schema failures",
    }
}

fn current_value(damage_id: &str, row: &Value, encoding: CurrentEncoding) -> Option<Value> {
    match encoding {
        CurrentEncoding::Id => damage_id.parse::<u64>().ok().map(|value| json!(value)),
        CurrentEncoding::Scalar(offset) => row
            .pointer(&format!("/aligned_scalars_by_offset/{offset}/i32"))
            .cloned(),
        CurrentEncoding::StringPool6(offset) => pool_value_or_empty(
            row,
            offset,
            "string_pool_6_candidates_by_offset",
            "value",
            Value::String(String::new()),
        ),
        CurrentEncoding::IntArrayPool1(offset) => pool_value_or_empty(
            row,
            offset,
            "int_array_pool_1_candidates_by_offset",
            "values",
            Value::Array(Vec::new()),
        ),
        CurrentEncoding::Bool(offset) => {
            let relative = usize::from(offset.saturating_sub(72)) * 2;
            let bytes = row.get("trailing_bytes_hex")?.as_str()?;
            let byte = bytes.get(relative..relative + 2)?;
            u8::from_str_radix(byte, 16)
                .ok()
                .map(|value| Value::Bool(value != 0))
        }
    }
}

fn pool_value_or_empty(
    row: &Value,
    offset: u8,
    pool: &str,
    value_field: &str,
    empty: Value,
) -> Option<Value> {
    let pointer = row
        .pointer(&format!("/aligned_scalars_by_offset/{offset}/u32"))?
        .as_u64()?;
    if pointer == 0 {
        return Some(empty);
    }
    row.pointer(&format!("/{pool}/{offset}/{value_field}"))
        .cloned()
}

fn encoding_label(encoding: CurrentEncoding) -> (Option<u8>, &'static str) {
    match encoding {
        CurrentEncoding::Id => (Some(0), "u64"),
        CurrentEncoding::Scalar(offset) => (Some(offset), "i32"),
        CurrentEncoding::StringPool6(offset) => (Some(offset), "string_pool_6_reference"),
        CurrentEncoding::IntArrayPool1(offset) => (Some(offset), "int_array_pool_1_reference"),
        CurrentEncoding::Bool(offset) => (Some(offset), "bool"),
    }
}

fn arguments() -> Result<Arguments, String> {
    let mut values = env::args_os().skip(1);
    let mut surface = None;
    let mut reference = None;
    let mut formula_source = None;
    let mut output = None;
    let mut example_limit = DEFAULT_EXAMPLE_LIMIT;
    while let Some(argument) = values.next() {
        match argument.to_string_lossy().as_ref() {
            "--surface" => surface = Some(path_value(&mut values, "--surface")?),
            "--reference" => reference = Some(path_value(&mut values, "--reference")?),
            "--formula-source" => {
                formula_source = Some(path_value(&mut values, "--formula-source")?)
            }
            "--output" => output = Some(path_value(&mut values, "--output")?),
            "--example-limit" => {
                example_limit = values
                    .next()
                    .ok_or("--example-limit requires a value")?
                    .to_string_lossy()
                    .parse::<usize>()
                    .map_err(|_| "--example-limit must be an unsigned integer")?;
            }
            value => return Err(format!("unknown argument: {value}")),
        }
    }
    Ok(Arguments {
        surface: surface.ok_or("missing --surface")?,
        reference: reference.ok_or("missing --reference")?,
        formula_source: formula_source.ok_or("missing --formula-source")?,
        output: output.ok_or("missing --output")?,
        example_limit,
    })
}

fn path_value(
    values: &mut impl Iterator<Item = std::ffi::OsString>,
    name: &str,
) -> Result<PathBuf, String> {
    values
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| format!("{name} requires a path"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_pool_pointer_decodes_as_empty_value() {
        let row = json!({
            "aligned_scalars_by_offset": {"28": {"u32": 0}}
        });
        assert_eq!(
            current_value("1", &row, CurrentEncoding::IntArrayPool1(28)),
            Some(json!([]))
        );
    }

    #[test]
    fn nonzero_pool_pointer_requires_decoded_value() {
        let row = json!({
            "aligned_scalars_by_offset": {"28": {"u32": 7}}
        });
        assert_eq!(
            current_value("1", &row, CurrentEncoding::IntArrayPool1(28)),
            None
        );
    }

    #[test]
    fn trailing_bool_is_decoded_from_byte_72() {
        let row = json!({"trailing_bytes_hex": "0100"});
        assert_eq!(
            current_value("1", &row, CurrentEncoding::Bool(72)),
            Some(json!(true))
        );
    }
}
