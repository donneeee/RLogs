use std::{
    ffi::OsString,
    fs::File,
    io::{BufWriter, Read, Seek, SeekFrom, Write},
    path::PathBuf,
};

use serde::Serialize;
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 1;
const ROW_SIZE: usize = 16;
const FIXED_POINT_SCALE: f64 = 10_000.0;

fn main() {
    if let Err(error) = run() {
        eprintln!("fight probability table proof failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments()?;
    let mut file = File::open(&arguments.package)?;
    file.seek(SeekFrom::Start(arguments.offset))?;
    let mut bytes = vec![0_u8; arguments.bytes];
    file.read_exact(&mut bytes)?;

    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    let expected_hash_matches = arguments
        .expected_sha256
        .as_ref()
        .map(|expected| normalize_sha256(expected) == sha256);
    if expected_hash_matches == Some(false) {
        return Err(format!(
            "table hash mismatch: expected {}, observed {sha256}",
            arguments.expected_sha256.as_deref().unwrap_or_default()
        )
        .into());
    }

    let magic = slice(&bytes, 0, 8)?;
    let row_count = read_u32(&bytes, 8)? as usize;
    let pool_count = read_u32(&bytes, 12)? as usize;
    let row_bytes = read_u32(&bytes, 16)? as usize;
    if row_bytes != row_count.saturating_mul(ROW_SIZE) {
        return Err(format!(
            "row byte count {row_bytes} does not match {row_count} rows of {ROW_SIZE} bytes"
        )
        .into());
    }

    let key_index_start = 20_usize;
    let key_index_bytes = row_count
        .checked_mul(8)
        .ok_or("primary-key index size overflowed")?;
    let rows_start = key_index_start
        .checked_add(key_index_bytes)
        .ok_or("row start overflowed")?;
    let pools_start = rows_start
        .checked_add(row_bytes)
        .ok_or("pool start overflowed")?;
    let expected_total = pools_start
        .checked_add(pool_count.saturating_mul(8))
        .ok_or("table size overflowed")?;
    if expected_total != bytes.len() {
        return Err(format!(
            "expected {expected_total} bytes for empty typed pools, observed {}",
            bytes.len()
        )
        .into());
    }

    let mut keys_match = true;
    let mut target_step_matches = true;
    let mut minimum_cap_matches = true;
    let mut coefficient_monotonic = true;
    let mut previous_coefficient = 0_u32;
    let mut rows = Vec::with_capacity(row_count);
    let mut maximum_probability_error_basis_points = 0.0_f64;

    for row_index in 0..row_count {
        let indexed_key = read_u64(&bytes, key_index_start + row_index * 8)?;
        let row_offset = rows_start + row_index * ROW_SIZE;
        let id = read_u32(&bytes, row_offset)?;
        let target_basis_points = read_u32(&bytes, row_offset + 4)?;
        let raw_coefficient_basis_points = read_u32(&bytes, row_offset + 8)?;
        let applied_coefficient_basis_points = read_u32(&bytes, row_offset + 12)?;
        keys_match &= indexed_key == u64::from(id);
        target_step_matches &= target_basis_points == id.saturating_mul(10);
        minimum_cap_matches &=
            applied_coefficient_basis_points == raw_coefficient_basis_points.max(1);
        coefficient_monotonic &= raw_coefficient_basis_points >= previous_coefficient;
        previous_coefficient = raw_coefficient_basis_points;

        let implied_probability_basis_points =
            prd_probability(raw_coefficient_basis_points) * FIXED_POINT_SCALE;
        let probability_error_basis_points =
            implied_probability_basis_points - f64::from(target_basis_points);
        if raw_coefficient_basis_points > 0 {
            maximum_probability_error_basis_points =
                maximum_probability_error_basis_points.max(probability_error_basis_points.abs());
        }
        rows.push(ProbabilityRow {
            row_index,
            id,
            target_basis_points,
            raw_coefficient_basis_points,
            applied_coefficient_basis_points,
            implied_prd_probability_basis_points: implied_probability_basis_points,
            probability_error_basis_points,
        });
    }

    let report = ProofBundle {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-fight-probability-table-proof",
        policy: ProofPolicy {
            runtime_parser_dependency: false,
            table_name_proves_critical_or_lucky_usage: false,
            displayed_percentage_is_per_attempt_probability: false,
            unresolved_probability_state_is_hidden: false,
            rdps_runtime_enabled: false,
        },
        source: SourceReport {
            package: package_label(&arguments.package),
            offset: arguments.offset,
            bytes: arguments.bytes,
            sha256: format!("sha256:{sha256}"),
            expected_sha256: arguments.expected_sha256,
            expected_hash_matches,
        },
        table: TableReport {
            name: "FightProbFixTable",
            magic_hex: magic
                .iter()
                .map(|value| format!("{value:02x}"))
                .collect::<String>(),
            row_count,
            row_size: ROW_SIZE,
            row_bytes,
            pool_count,
            pools_are_empty: true,
            primary_key_index_matches_rows: keys_match,
            target_step_matches,
            minimum_cap_matches,
            coefficient_monotonic,
            maximum_probability_error_basis_points,
        },
        interpretation: InterpretationReport {
            structurally_proven: "field_4 is id * 10; field_12 is max(field_8, 1); field_8 is monotonic",
            mathematically_proven: "for every nonzero field_8 row, the increasing per-failure probability process p(n)=min(1,n*field_8/10000) reproduces field_4 within the reported rounding error",
            unresolved: "which combat/non-combat rolls select this table and where each entity's failure counter is synchronized",
        },
        rows,
    };

    let mut writer = BufWriter::new(File::create(&arguments.output)?);
    serde_json::to_writer_pretty(&mut writer, &report)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    println!(
        "wrote {} FightProbFixTable rows to {}",
        report.rows.len(),
        arguments.output.display()
    );
    Ok(())
}

/// Long-run outcome probability for an increasing per-failure roll.
///
/// On attempt n the success probability is min(1, n*C). The expected number
/// of attempts is the survival sum E[N] = sum(P(N > n)); its reciprocal is the
/// long-run success probability.
fn prd_probability(coefficient_basis_points: u32) -> f64 {
    if coefficient_basis_points == 0 {
        return 0.0;
    }
    let coefficient = f64::from(coefficient_basis_points) / FIXED_POINT_SCALE;
    let mut survival = 1.0_f64;
    let mut expected_attempts = 1.0_f64;
    for attempt in 1_u32..=10_000 {
        let success = (f64::from(attempt) * coefficient).min(1.0);
        survival *= 1.0 - success;
        if survival == 0.0 {
            break;
        }
        expected_attempts += survival;
    }
    expected_attempts.recip()
}

#[derive(Serialize)]
struct ProofBundle {
    schema_version: u16,
    generated_by: &'static str,
    policy: ProofPolicy,
    source: SourceReport,
    table: TableReport,
    interpretation: InterpretationReport,
    rows: Vec<ProbabilityRow>,
}

#[derive(Serialize)]
struct ProofPolicy {
    runtime_parser_dependency: bool,
    table_name_proves_critical_or_lucky_usage: bool,
    displayed_percentage_is_per_attempt_probability: bool,
    unresolved_probability_state_is_hidden: bool,
    rdps_runtime_enabled: bool,
}

#[derive(Serialize)]
struct SourceReport {
    package: String,
    offset: u64,
    bytes: usize,
    sha256: String,
    expected_sha256: Option<String>,
    expected_hash_matches: Option<bool>,
}

#[derive(Serialize)]
struct TableReport {
    name: &'static str,
    magic_hex: String,
    row_count: usize,
    row_size: usize,
    row_bytes: usize,
    pool_count: usize,
    pools_are_empty: bool,
    primary_key_index_matches_rows: bool,
    target_step_matches: bool,
    minimum_cap_matches: bool,
    coefficient_monotonic: bool,
    maximum_probability_error_basis_points: f64,
}

#[derive(Serialize)]
struct InterpretationReport {
    structurally_proven: &'static str,
    mathematically_proven: &'static str,
    unresolved: &'static str,
}

#[derive(Serialize)]
struct ProbabilityRow {
    row_index: usize,
    id: u32,
    target_basis_points: u32,
    raw_coefficient_basis_points: u32,
    applied_coefficient_basis_points: u32,
    implied_prd_probability_basis_points: f64,
    probability_error_basis_points: f64,
}

struct Arguments {
    package: PathBuf,
    offset: u64,
    bytes: usize,
    expected_sha256: Option<String>,
    output: PathBuf,
}

fn arguments() -> Result<Arguments, Box<dyn std::error::Error>> {
    let mut values = std::env::args_os().skip(1);
    let package = PathBuf::from(required(&mut values, "package")?);
    let offset = text(&required(&mut values, "offset")?)?.parse::<u64>()?;
    let bytes = text(&required(&mut values, "bytes")?)?.parse::<usize>()?;
    let expected = text(&required(&mut values, "expected sha256 or -")?)?;
    let output = PathBuf::from(required(&mut values, "output")?);
    if values.next().is_some() {
        return Err("usage: rlogs-bpsr-fight-probability-table-proof <m0.pkg> <offset> <bytes> <sha256|-> <output.json>".into());
    }
    Ok(Arguments {
        package,
        offset,
        bytes,
        expected_sha256: (expected != "-").then_some(expected),
        output,
    })
}

fn required(
    values: &mut impl Iterator<Item = OsString>,
    name: &str,
) -> Result<OsString, Box<dyn std::error::Error>> {
    values
        .next()
        .ok_or_else(|| format!("missing {name}").into())
}

fn text(value: &OsString) -> Result<String, Box<dyn std::error::Error>> {
    value
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| "argument is not valid Unicode".into())
}

fn normalize_sha256(value: &str) -> String {
    value
        .trim()
        .strip_prefix("sha256:")
        .unwrap_or(value.trim())
        .to_ascii_lowercase()
}

fn package_label(path: &std::path::Path) -> String {
    let file = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("m0.pkg");
    let parent = path
        .parent()
        .and_then(std::path::Path::file_name)
        .and_then(|value| value.to_str())
        .unwrap_or("container");
    format!("{parent}/{file}")
}

fn slice(bytes: &[u8], offset: usize, length: usize) -> Result<&[u8], Box<dyn std::error::Error>> {
    let end = offset.checked_add(length).ok_or("slice overflowed")?;
    bytes
        .get(offset..end)
        .ok_or_else(|| format!("slice {offset}..{end} exceeds {} bytes", bytes.len()).into())
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, Box<dyn std::error::Error>> {
    Ok(u32::from_le_bytes(slice(bytes, offset, 4)?.try_into()?))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, Box<dyn std::error::Error>> {
    Ok(u64::from_le_bytes(slice(bytes, offset, 8)?.try_into()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_current_table_coefficients_reproduce_target_probabilities() {
        for (target, coefficient) in [(1_000.0, 147), (5_000.0, 3_021), (9_000.0, 8_889)] {
            let observed = prd_probability(coefficient) * FIXED_POINT_SCALE;
            assert!((observed - target).abs() <= 20.0, "{observed} vs {target}");
        }
    }

    #[test]
    fn zero_and_certain_coefficients_are_exact() {
        assert_eq!(prd_probability(0), 0.0);
        assert_eq!(prd_probability(10_000), 1.0);
    }
}
