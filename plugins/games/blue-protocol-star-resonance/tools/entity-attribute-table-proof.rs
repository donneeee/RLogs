use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    fs::File,
    io::{BufWriter, Read, Seek, SeekFrom, Write},
    path::PathBuf,
};

use serde::Serialize;
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 2;
const ENTITY_ATTRIBUTE_ROW_SIZE: usize = 29;

#[derive(Debug)]
struct Arguments {
    package: PathBuf,
    offset: u64,
    bytes: usize,
    expected_sha256: Option<String>,
    output: PathBuf,
}

#[derive(Debug, Serialize)]
struct ProofBundle {
    schema_version: u16,
    generated_by: &'static str,
    policy: ProofPolicy,
    source: SourceReport,
    table: TableReport,
    pools: Vec<PoolReport>,
    rows: Vec<EntityAttributeRow>,
}

#[derive(Debug, Serialize)]
struct ProofPolicy {
    runtime_parser_dependency: bool,
    localized_names_are_formula_authority: bool,
    unresolved_fields_are_hidden: bool,
    row_identity_authority: &'static str,
    field_layout_authority: &'static str,
    array_semantics: &'static str,
}

#[derive(Debug, Serialize)]
struct SourceReport {
    package: String,
    offset: u64,
    bytes: usize,
    sha256: String,
    expected_sha256: Option<String>,
    expected_hash_matches: Option<bool>,
}

#[derive(Debug, Serialize)]
struct TableReport {
    magic_hex: String,
    row_count: usize,
    row_size: usize,
    row_bytes: usize,
    pool_count: usize,
    primary_key_index_matches_rows: bool,
    consumed_bytes: usize,
}

#[derive(Debug, Serialize)]
struct PoolReport {
    pool_type: u32,
    bytes: usize,
    decoded_records: usize,
}

#[derive(Debug, Serialize)]
struct EntityAttributeRow {
    row_index: usize,
    id: u32,
    comment_zh_cn: Option<String>,
    level: PoolReference,
    season: PoolReference,
    season_level: PoolReference,
    season_rank: PoolReference,
    fight_value_coefficient: u32,
    is_load_rank: bool,
}

#[derive(Debug, Serialize)]
struct PoolReference {
    raw_offset: u32,
    int_array: Option<Vec<i32>>,
}

#[derive(Debug)]
struct ParsedPool {
    pool_type: u32,
    bytes: usize,
    int_arrays: BTreeMap<u32, Vec<i32>>,
    strings: BTreeMap<u32, String>,
    decoded_records: usize,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("entity attribute table proof failed: {error}");
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
    if row_bytes != row_count.saturating_mul(ENTITY_ATTRIBUTE_ROW_SIZE) {
        return Err(format!(
            "row byte count {row_bytes} does not match {row_count} rows of {ENTITY_ATTRIBUTE_ROW_SIZE} bytes"
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
    slice(&bytes, rows_start, row_bytes)?;

    let primary_keys = (0..row_count)
        .map(|index| read_u64(&bytes, key_index_start + index * 8))
        .collect::<Result<Vec<_>, _>>()?;

    let (pools, consumed_bytes) = parse_pools(&bytes, pools_start, pool_count)?;
    if consumed_bytes != bytes.len() {
        return Err(format!(
            "table parser consumed {consumed_bytes} of {} bytes",
            bytes.len()
        )
        .into());
    }
    let int_arrays = pools
        .iter()
        .find(|pool| pool.pool_type == 1)
        .map(|pool| &pool.int_arrays)
        .ok_or("table lacks integer-array pool type 1")?;
    let strings = pools
        .iter()
        .find(|pool| pool.pool_type == 6)
        .map(|pool| &pool.strings)
        .ok_or("table lacks UTF-8 string pool type 6")?;

    let mut primary_key_index_matches_rows = true;
    let mut rows = Vec::with_capacity(row_count);
    for (row_index, indexed_key) in primary_keys.into_iter().enumerate() {
        let row_offset = rows_start + row_index * ENTITY_ATTRIBUTE_ROW_SIZE;
        let id = read_u32(&bytes, row_offset)?;
        primary_key_index_matches_rows &= indexed_key == u64::from(id);
        let name_ref = read_u32(&bytes, row_offset + 4)?;
        rows.push(EntityAttributeRow {
            row_index,
            id,
            comment_zh_cn: strings.get(&name_ref).cloned(),
            level: pool_reference(read_u32(&bytes, row_offset + 8)?, int_arrays),
            season: pool_reference(read_u32(&bytes, row_offset + 12)?, int_arrays),
            season_level: pool_reference(read_u32(&bytes, row_offset + 16)?, int_arrays),
            season_rank: pool_reference(read_u32(&bytes, row_offset + 20)?, int_arrays),
            fight_value_coefficient: read_u32(&bytes, row_offset + 24)?,
            is_load_rank: bytes[row_offset + 28] != 0,
        });
    }

    let bundle = ProofBundle {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-entity-attribute-table-proof",
        policy: ProofPolicy {
            runtime_parser_dependency: false,
            localized_names_are_formula_authority: false,
            unresolved_fields_are_hidden: false,
            row_identity_authority: "exact-build CTB primary-key index and fixed row bytes",
            field_layout_authority: "matching-table decoded schema names plus the exact 29-byte CTB row layout",
            array_semantics: "current-build Level, Season, SeasonLv, and SeasonRank integer-array values are preserved exactly; none is treated as a defense or mitigation scalar",
        },
        source: SourceReport {
            package: arguments.package.display().to_string(),
            offset: arguments.offset,
            bytes: arguments.bytes,
            sha256: format!("sha256:{sha256}"),
            expected_sha256: arguments.expected_sha256,
            expected_hash_matches,
        },
        table: TableReport {
            magic_hex: magic
                .iter()
                .map(|value| format!("{value:02x}"))
                .collect::<String>(),
            row_count,
            row_size: ENTITY_ATTRIBUTE_ROW_SIZE,
            row_bytes,
            pool_count,
            primary_key_index_matches_rows,
            consumed_bytes,
        },
        pools: pools
            .iter()
            .map(|pool| PoolReport {
                pool_type: pool.pool_type,
                bytes: pool.bytes,
                decoded_records: pool.decoded_records,
            })
            .collect(),
        rows,
    };

    let output = File::create(&arguments.output)?;
    let mut writer = BufWriter::new(output);
    serde_json::to_writer_pretty(&mut writer, &bundle)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn parse_pools(
    bytes: &[u8],
    mut cursor: usize,
    pool_count: usize,
) -> Result<(Vec<ParsedPool>, usize), Box<dyn std::error::Error>> {
    let mut pools = Vec::with_capacity(pool_count);
    for _ in 0..pool_count {
        let pool_type = read_u32(bytes, cursor)?;
        let pool_bytes = read_u32(bytes, cursor + 4)? as usize;
        cursor += 8;
        let data = slice(bytes, cursor, pool_bytes)?;
        let mut pool = ParsedPool {
            pool_type,
            bytes: pool_bytes,
            int_arrays: BTreeMap::new(),
            strings: BTreeMap::new(),
            decoded_records: 0,
        };
        match pool_type {
            1 => pool.int_arrays = parse_int_arrays(data)?,
            6 => pool.strings = parse_strings(data)?,
            _ => {}
        }
        pool.decoded_records = match pool_type {
            1 => pool.int_arrays.len(),
            6 => pool.strings.len(),
            _ => 0,
        };
        pools.push(pool);
        cursor += pool_bytes;
    }
    Ok((pools, cursor))
}

fn parse_int_arrays(bytes: &[u8]) -> Result<BTreeMap<u32, Vec<i32>>, String> {
    let mut result = BTreeMap::new();
    let mut cursor = 0_usize;
    while cursor < bytes.len() {
        let record_offset = u32::try_from(cursor).map_err(|_| "integer pool offset overflowed")?;
        let count = read_u16(bytes, cursor)? as usize;
        cursor += 2;
        let payload_bytes = count
            .checked_mul(4)
            .ok_or("integer array byte count overflowed")?;
        slice(bytes, cursor, payload_bytes)?;
        let values = (0..count)
            .map(|index| read_i32(bytes, cursor + index * 4))
            .collect::<Result<Vec<_>, _>>()?;
        result.insert(record_offset, values);
        cursor += payload_bytes;
    }
    Ok(result)
}

fn parse_strings(bytes: &[u8]) -> Result<BTreeMap<u32, String>, String> {
    let mut result = BTreeMap::new();
    let mut cursor = 0_usize;
    while cursor < bytes.len() {
        let record_offset = u32::try_from(cursor).map_err(|_| "string pool offset overflowed")?;
        let count = read_u16(bytes, cursor)? as usize;
        cursor += 2;
        let value = std::str::from_utf8(slice(bytes, cursor, count)?).map_err(|error| {
            format!("invalid UTF-8 string at pool offset {record_offset}: {error}")
        })?;
        result.insert(record_offset, value.to_owned());
        cursor += count;
    }
    Ok(result)
}

fn pool_reference(raw_offset: u32, values: &BTreeMap<u32, Vec<i32>>) -> PoolReference {
    PoolReference {
        raw_offset,
        int_array: values.get(&raw_offset).cloned(),
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, String> {
    Ok(u16::from_le_bytes(
        slice(bytes, offset, 2)?
            .try_into()
            .map_err(|_| "u16 width mismatch")?,
    ))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(
        slice(bytes, offset, 4)?
            .try_into()
            .map_err(|_| "u32 width mismatch")?,
    ))
}

fn read_i32(bytes: &[u8], offset: usize) -> Result<i32, String> {
    Ok(i32::from_le_bytes(
        slice(bytes, offset, 4)?
            .try_into()
            .map_err(|_| "i32 width mismatch")?,
    ))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, String> {
    Ok(u64::from_le_bytes(
        slice(bytes, offset, 8)?
            .try_into()
            .map_err(|_| "u64 width mismatch")?,
    ))
}

fn slice(bytes: &[u8], offset: usize, length: usize) -> Result<&[u8], String> {
    let end = offset.checked_add(length).ok_or("byte range overflowed")?;
    bytes
        .get(offset..end)
        .ok_or_else(|| format!("byte range {offset}..{end} exceeds {} bytes", bytes.len()))
}

fn normalize_sha256(value: &str) -> String {
    value
        .strip_prefix("sha256:")
        .unwrap_or(value)
        .to_ascii_lowercase()
}

fn arguments() -> Result<Arguments, String> {
    let mut values = env::args_os().skip(1).collect::<Vec<_>>();
    let package = PathBuf::from(take_value(&mut values, "--package")?);
    let offset = parse_u64(take_value(&mut values, "--offset")?, "--offset")?;
    let bytes = parse_usize(take_value(&mut values, "--bytes")?, "--bytes")?;
    let expected_sha256 = take_optional_value(&mut values, "--expected-sha256")
        .map(|value| value.to_string_lossy().to_string());
    let output = PathBuf::from(take_value(&mut values, "--output")?);
    if !values.is_empty() {
        return Err(usage());
    }
    Ok(Arguments {
        package,
        offset,
        bytes,
        expected_sha256,
        output,
    })
}

fn parse_u64(value: OsString, flag: &str) -> Result<u64, String> {
    value
        .to_string_lossy()
        .parse::<u64>()
        .map_err(|_| format!("{flag} requires a non-negative integer"))
}

fn parse_usize(value: OsString, flag: &str) -> Result<usize, String> {
    value
        .to_string_lossy()
        .parse::<usize>()
        .map_err(|_| format!("{flag} requires a non-negative integer"))
}

fn take_value(values: &mut Vec<OsString>, flag: &str) -> Result<OsString, String> {
    let position = values
        .iter()
        .position(|value| value == flag)
        .ok_or_else(usage)?;
    if position + 1 >= values.len() {
        return Err(format!("{flag} requires a value"));
    }
    let value = values.remove(position + 1);
    values.remove(position);
    Ok(value)
}

fn take_optional_value(values: &mut Vec<OsString>, flag: &str) -> Option<OsString> {
    let position = values.iter().position(|value| value == flag)?;
    if position + 1 >= values.len() {
        return None;
    }
    let value = values.remove(position + 1);
    values.remove(position);
    Some(value)
}

fn usage() -> String {
    "usage: rlogs-bpsr-entity-attribute-table-proof --package <m0.pkg> --offset <bytes> --bytes <length> [--expected-sha256 <digest>] --output <proof.json>".to_owned()
}

#[cfg(test)]
mod tests {
    use super::{parse_int_arrays, parse_strings};

    #[test]
    fn decodes_integer_array_pool_records_by_byte_offset() {
        let bytes = [2, 0, 1, 0, 0, 0, 2, 0, 0, 0, 0, 0];
        let records = parse_int_arrays(&bytes).expect("pool should decode");
        assert_eq!(records.get(&0), Some(&vec![1, 2]));
        assert_eq!(records.get(&10), Some(&Vec::new()));
    }

    #[test]
    fn decodes_utf8_pool_records_by_byte_offset() {
        let bytes = [3, 0, b'a', b'b', b'c', 0, 0];
        let records = parse_strings(&bytes).expect("pool should decode");
        assert_eq!(records.get(&0).map(String::as_str), Some("abc"));
        assert_eq!(records.get(&5).map(String::as_str), Some(""));
    }
}
