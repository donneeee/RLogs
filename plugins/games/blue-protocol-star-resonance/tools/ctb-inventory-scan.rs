use std::{
    collections::{BTreeMap, HashSet},
    env,
    ffi::OsString,
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use serde::Serialize;
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 1;
const HEADER_BYTES: usize = 20;
const HEADER_OVERLAP: usize = HEADER_BYTES - 1;
const SCAN_CHUNK_BYTES: usize = 32 * 1024 * 1024;
const HASH_CHUNK_BYTES: usize = 8 * 1024 * 1024;
const EXPECTED_POOL_COUNT: u32 = 10;
const MAX_ROWS: u32 = 2_000_000;
const MAX_ROW_SIZE: u32 = 4096;
const MAX_ROW_DATA_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug)]
struct Arguments {
    package: PathBuf,
    relative_package: String,
    meta: PathBuf,
    relative_meta: String,
    steam_manifest: PathBuf,
    expected_build: String,
    deployment_id: String,
    channel: String,
    output: PathBuf,
}

#[derive(Debug, Serialize)]
struct Inventory {
    schema_version: u16,
    generated_by: &'static str,
    game: &'static str,
    deployment_id: String,
    channel: String,
    distribution_app_id: String,
    build_id: String,
    policy: Policy,
    source: Source,
    summary: Summary,
    rejected_candidate_reasons: BTreeMap<String, usize>,
    tables: Vec<Table>,
}

#[derive(Debug, Serialize)]
struct Policy {
    runtime_parser_dependency: bool,
    raw_payloads_embedded: bool,
    absolute_paths_embedded: bool,
    unresolved_candidates_hidden: bool,
    candidate_tables_auto_promoted: bool,
    identity_authority: &'static str,
    promotion_requirement: &'static str,
}

#[derive(Debug, Serialize)]
struct Source {
    package_relative_path: String,
    package_bytes: u64,
    meta_relative_path: String,
    meta_bytes: u64,
    meta_entries: usize,
}

#[derive(Debug, Serialize)]
struct Summary {
    structural_header_candidates: usize,
    confirmed_tables: usize,
    address_bound_tables: usize,
    address_unbound_tables: usize,
    rejected_candidates: usize,
    confirmed_table_bytes: u64,
    package_coverage_percent: f64,
}

#[derive(Debug, Serialize)]
struct Table {
    address_keys: Vec<AddressKey>,
    offset: u64,
    bytes: u64,
    sha256: String,
    magic_hex: String,
    magic_u32_le: [u32; 2],
    shape: Shape,
    row_ids: RowIds,
}

#[derive(Debug, Serialize)]
struct AddressKey {
    key: u32,
    key_hex: String,
    entry_type: u8,
    package_index: u16,
}

#[derive(Debug, Serialize)]
struct Shape {
    rows: u32,
    row_size: u32,
    row_data_bytes: u32,
    pool_lengths: Vec<PoolLength>,
    trailing_bytes: u32,
}

#[derive(Debug, Serialize)]
struct PoolLength {
    r#type: u32,
    bytes: u32,
}

#[derive(Debug, Serialize)]
struct RowIds {
    key_width_candidates: Vec<u8>,
    minimum: u64,
    maximum: u64,
    unique: usize,
    duplicate_rows: usize,
    zero_rows: usize,
}

#[derive(Debug, Clone, Copy)]
struct Candidate {
    offset: u64,
    row_count: u32,
    row_size: u32,
    row_data_bytes: u32,
}

#[derive(Debug, Clone, Copy)]
struct MetaEntry {
    key: u32,
    entry_type: u8,
    package_index: u16,
    offset: u64,
    bytes: u64,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("CTB inventory scan failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments()?;
    if arguments.output.exists() {
        return Err(format!(
            "refusing to overwrite existing output {}",
            arguments.output.display()
        )
        .into());
    }

    let manifest = fs::read_to_string(&arguments.steam_manifest)?;
    let app_id = manifest_value(&manifest, "appid").ok_or("Steam manifest lacks appid")?;
    let build_id = manifest_value(&manifest, "buildid").ok_or("Steam manifest lacks buildid")?;
    if build_id != arguments.expected_build {
        return Err(format!(
            "Steam manifest build {build_id} does not match expected build {}",
            arguments.expected_build
        )
        .into());
    }

    let package_bytes = fs::metadata(&arguments.package)?.len();
    let meta_bytes = fs::metadata(&arguments.meta)?.len();
    let meta_entries = read_meta_entries(&arguments.meta)?;
    let address_index = address_index(&meta_entries);
    let candidates = scan_headers(&arguments.package, package_bytes)?;
    let mut tables = Vec::new();
    let mut rejected_candidate_reasons = BTreeMap::<String, usize>::new();
    for candidate in &candidates {
        match validate_table(
            &arguments.package,
            package_bytes,
            *candidate,
            &address_index,
        ) {
            Ok(table) => tables.push(table),
            Err(reason) => *rejected_candidate_reasons.entry(reason).or_default() += 1,
        }
    }
    tables.sort_by_key(|table| table.offset);

    let confirmed_table_bytes = tables.iter().map(|table| table.bytes).sum::<u64>();
    let address_bound_tables = tables
        .iter()
        .filter(|table| !table.address_keys.is_empty())
        .count();
    let rejected_candidates = rejected_candidate_reasons.values().sum::<usize>();
    let inventory = Inventory {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-ctb-inventory-scan",
        game: "blue-protocol-star-resonance",
        deployment_id: arguments.deployment_id,
        channel: arguments.channel,
        distribution_app_id: app_id,
        build_id,
        policy: Policy {
            runtime_parser_dependency: false,
            raw_payloads_embedded: false,
            absolute_paths_embedded: false,
            unresolved_candidates_hidden: false,
            candidate_tables_auto_promoted: false,
            identity_authority: "exact Steam build plus CTB primary-key index, fixed rows, ordered pool blocks, bounds, and per-table SHA-256",
            promotion_requirement: "exact table identity plus matching-build packet replay and conservation proof",
        },
        source: Source {
            package_relative_path: normalize_relative_path(&arguments.relative_package)?,
            package_bytes,
            meta_relative_path: normalize_relative_path(&arguments.relative_meta)?,
            meta_bytes,
            meta_entries: meta_entries.len(),
        },
        summary: Summary {
            structural_header_candidates: candidates.len(),
            confirmed_tables: tables.len(),
            address_bound_tables,
            address_unbound_tables: tables.len() - address_bound_tables,
            rejected_candidates,
            confirmed_table_bytes,
            package_coverage_percent: if package_bytes == 0 {
                0.0
            } else {
                confirmed_table_bytes as f64 * 100.0 / package_bytes as f64
            },
        },
        rejected_candidate_reasons,
        tables,
    };

    if let Some(parent) = arguments.output.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(&arguments.output)?);
    serde_json::to_writer_pretty(&mut writer, &inventory)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn scan_headers(
    package: &Path,
    package_bytes: u64,
) -> Result<Vec<Candidate>, Box<dyn std::error::Error>> {
    let mut reader = BufReader::with_capacity(SCAN_CHUNK_BYTES, File::open(package)?);
    let mut buffer = vec![0_u8; SCAN_CHUNK_BYTES + HEADER_OVERLAP];
    let mut carry = 0_usize;
    let mut total_read = 0_u64;
    let mut next_offset = 0_u64;
    let mut candidates = Vec::new();

    loop {
        let read = reader.read(&mut buffer[carry..])?;
        if read == 0 {
            break;
        }
        let available = carry + read;
        let base_offset = total_read.saturating_sub(carry as u64);
        total_read += read as u64;

        if available >= HEADER_BYTES {
            for index in 0..=(available - HEADER_BYTES) {
                let offset = base_offset + index as u64;
                if offset < next_offset {
                    continue;
                }
                if read_u32_slice(&buffer, index + 12) != EXPECTED_POOL_COUNT {
                    continue;
                }
                let row_count = read_u32_slice(&buffer, index + 8);
                let row_data_bytes = read_u32_slice(&buffer, index + 16);
                if let Some(row_size) = plausible_shape(row_count, row_data_bytes) {
                    let minimum_bytes = minimum_table_bytes(row_count, row_data_bytes)?;
                    if offset
                        .checked_add(minimum_bytes)
                        .is_some_and(|end| end <= package_bytes)
                    {
                        candidates.push(Candidate {
                            offset,
                            row_count,
                            row_size,
                            row_data_bytes,
                        });
                    }
                }
            }
            next_offset = base_offset + (available - HEADER_OVERLAP) as u64;
        }

        carry = available.min(HEADER_OVERLAP);
        buffer.copy_within(available - carry..available, 0);
    }
    Ok(candidates)
}

fn plausible_shape(row_count: u32, row_data_bytes: u32) -> Option<u32> {
    if row_count == 0 || row_count > MAX_ROWS || row_data_bytes == 0 {
        return None;
    }
    if u64::from(row_data_bytes) > MAX_ROW_DATA_BYTES || row_data_bytes % row_count != 0 {
        return None;
    }
    let row_size = row_data_bytes / row_count;
    (4..=MAX_ROW_SIZE).contains(&row_size).then_some(row_size)
}

fn minimum_table_bytes(row_count: u32, row_data_bytes: u32) -> Result<u64, &'static str> {
    u64::try_from(HEADER_BYTES)
        .ok()
        .and_then(|header| header.checked_add(u64::from(row_count).checked_mul(8)?))
        .and_then(|bytes| bytes.checked_add(u64::from(row_data_bytes)))
        .and_then(|bytes| bytes.checked_add(u64::from(EXPECTED_POOL_COUNT) * 8))
        .ok_or("candidate size overflow")
}

fn validate_table(
    package: &Path,
    package_bytes: u64,
    candidate: Candidate,
    address_index: &BTreeMap<(u16, u64, u64), Vec<MetaEntry>>,
) -> Result<Table, String> {
    let mut file = File::open(package).map_err(|_| "package reopen failed".to_owned())?;
    file.seek(SeekFrom::Start(candidate.offset))
        .map_err(|_| "candidate seek failed".to_owned())?;
    let mut header = [0_u8; HEADER_BYTES];
    file.read_exact(&mut header)
        .map_err(|_| "candidate header truncated".to_owned())?;
    if read_u32_slice(&header, 8) != candidate.row_count
        || read_u32_slice(&header, 12) != EXPECTED_POOL_COUNT
        || read_u32_slice(&header, 16) != candidate.row_data_bytes
    {
        return Err("candidate header changed during scan".to_owned());
    }

    let mut keys = Vec::with_capacity(candidate.row_count as usize);
    let mut key_bytes = [0_u8; 8];
    for _ in 0..candidate.row_count {
        file.read_exact(&mut key_bytes)
            .map_err(|_| "primary-key index truncated".to_owned())?;
        keys.push(u64::from_le_bytes(key_bytes));
    }

    let rows_start = candidate.offset + HEADER_BYTES as u64 + u64::from(candidate.row_count) * 8;
    file.seek(SeekFrom::Start(rows_start))
        .map_err(|_| "row seek failed".to_owned())?;
    let mut row = vec![0_u8; candidate.row_size as usize];
    let mut observed = HashSet::with_capacity(candidate.row_count as usize);
    let mut minimum = u64::MAX;
    let mut maximum = u64::MIN;
    let mut zero_rows = 0_usize;
    let mut key_width_4_matches = true;
    let mut key_width_8_matches = candidate.row_size >= 8;
    for (index, expected) in keys.into_iter().enumerate() {
        file.read_exact(&mut row)
            .map_err(|_| "row data truncated".to_owned())?;
        let first_u32 = u64::from(read_u32_slice(&row, 0));
        let first_u64 = (candidate.row_size >= 8)
            .then(|| u64::from_le_bytes(row[0..8].try_into().expect("validated u64 row slice")));
        key_width_4_matches &= first_u32 == expected;
        key_width_8_matches &= first_u64 == Some(expected);
        if !key_width_4_matches && !key_width_8_matches {
            return Err(format!("primary-key mismatch at row {index}"));
        }
        minimum = minimum.min(expected);
        maximum = maximum.max(expected);
        zero_rows += usize::from(expected == 0);
        observed.insert(expected);
    }

    let pools_start = rows_start + u64::from(candidate.row_data_bytes);
    file.seek(SeekFrom::Start(pools_start))
        .map_err(|_| "pool seek failed".to_owned())?;
    let mut pool_lengths = Vec::with_capacity(EXPECTED_POOL_COUNT as usize);
    let mut cursor = pools_start;
    let mut pool_header = [0_u8; 8];
    for expected_type in 1..=EXPECTED_POOL_COUNT {
        file.read_exact(&mut pool_header)
            .map_err(|_| "pool header truncated".to_owned())?;
        let pool_type = read_u32_slice(&pool_header, 0);
        let bytes = read_u32_slice(&pool_header, 4);
        if pool_type != expected_type {
            return Err(format!(
                "pool type {pool_type} observed where {expected_type} was required"
            ));
        }
        cursor = cursor
            .checked_add(8)
            .and_then(|value| value.checked_add(u64::from(bytes)))
            .ok_or_else(|| "pool extent overflow".to_owned())?;
        if cursor > package_bytes {
            return Err("pool payload exceeds package".to_owned());
        }
        file.seek(SeekFrom::Start(cursor))
            .map_err(|_| "pool payload seek failed".to_owned())?;
        pool_lengths.push(PoolLength {
            r#type: pool_type,
            bytes,
        });
    }

    let table_bytes = cursor - candidate.offset;
    let sha256 = hash_extent(package, candidate.offset, table_bytes)
        .map_err(|_| "table hashing failed".to_owned())?;
    let address_keys = address_index
        .get(&(0, candidate.offset, table_bytes))
        .into_iter()
        .flatten()
        .map(|entry| AddressKey {
            key: entry.key,
            key_hex: format!("0x{:08x}", entry.key),
            entry_type: entry.entry_type,
            package_index: entry.package_index,
        })
        .collect::<Vec<_>>();
    if address_keys.is_empty() {
        return Err("validated structure lacks exact meta extent".to_owned());
    }
    let mut key_width_candidates = Vec::new();
    if key_width_4_matches {
        key_width_candidates.push(4);
    }
    if key_width_8_matches {
        key_width_candidates.push(8);
    }
    Ok(Table {
        address_keys,
        offset: candidate.offset,
        bytes: table_bytes,
        sha256: format!("sha256:{sha256}"),
        magic_hex: header[0..8]
            .iter()
            .map(|value| format!("{value:02x}"))
            .collect(),
        magic_u32_le: [read_u32_slice(&header, 0), read_u32_slice(&header, 4)],
        shape: Shape {
            rows: candidate.row_count,
            row_size: candidate.row_size,
            row_data_bytes: candidate.row_data_bytes,
            pool_lengths,
            trailing_bytes: 0,
        },
        row_ids: RowIds {
            key_width_candidates,
            minimum,
            maximum,
            unique: observed.len(),
            duplicate_rows: candidate.row_count as usize - observed.len(),
            zero_rows,
        },
    })
}

fn read_meta_entries(path: &Path) -> Result<Vec<MetaEntry>, Box<dyn std::error::Error>> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut prefix = [0_u8; 26];
    reader.read_exact(&mut prefix)?;
    let descriptor_count = u16::from_le_bytes(prefix[24..26].try_into()?);
    reader.seek(SeekFrom::Current(i64::from(descriptor_count) * 16))?;

    let first_count = read_i32(&mut reader)?;
    if first_count < 0 {
        return Err("negative first meta entry count".into());
    }
    let mut entries = Vec::with_capacity(first_count as usize);
    read_meta_entry_block(&mut reader, first_count as usize, &mut entries)?;

    let second_count = read_i32(&mut reader)?;
    if second_count < 0 {
        return Err("negative second meta entry count".into());
    }
    entries.reserve(second_count as usize);
    read_meta_entry_block(&mut reader, second_count as usize, &mut entries)?;
    Ok(entries)
}

fn read_meta_entry_block(
    reader: &mut impl Read,
    count: usize,
    entries: &mut Vec<MetaEntry>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = [0_u8; 15];
    for _ in 0..count {
        reader.read_exact(&mut bytes)?;
        let offset = i32::from_le_bytes(bytes[7..11].try_into()?);
        let length = i32::from_le_bytes(bytes[11..15].try_into()?);
        if offset < 0 || length < 0 {
            return Err("negative meta entry extent".into());
        }
        entries.push(MetaEntry {
            key: u32::from_le_bytes(bytes[0..4].try_into()?),
            entry_type: bytes[4],
            package_index: u16::from_le_bytes(bytes[5..7].try_into()?),
            offset: offset as u64,
            bytes: length as u64,
        });
    }
    Ok(())
}

fn read_i32(reader: &mut impl Read) -> Result<i32, std::io::Error> {
    let mut bytes = [0_u8; 4];
    reader.read_exact(&mut bytes)?;
    Ok(i32::from_le_bytes(bytes))
}

fn address_index(entries: &[MetaEntry]) -> BTreeMap<(u16, u64, u64), Vec<MetaEntry>> {
    let mut index = BTreeMap::<(u16, u64, u64), Vec<MetaEntry>>::new();
    for entry in entries {
        index
            .entry((entry.package_index, entry.offset, entry.bytes))
            .or_default()
            .push(*entry);
    }
    index
}

#[cfg(test)]
fn hash33(value: &str) -> u32 {
    value.chars().fold(5381_u32, |hash, character| {
        hash.wrapping_mul(33).wrapping_add(character as u32)
    })
}

fn hash_extent(path: &Path, offset: u64, bytes: u64) -> Result<String, std::io::Error> {
    let mut file = BufReader::with_capacity(HASH_CHUNK_BYTES, File::open(path)?);
    file.seek(SeekFrom::Start(offset))?;
    let mut remaining = bytes;
    let mut buffer = vec![0_u8; HASH_CHUNK_BYTES];
    let mut hasher = Sha256::new();
    while remaining > 0 {
        let wanted = usize::try_from(remaining.min(buffer.len() as u64)).unwrap_or(buffer.len());
        file.read_exact(&mut buffer[..wanted])?;
        hasher.update(&buffer[..wanted]);
        remaining -= wanted as u64;
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn normalize_relative_path(value: &str) -> Result<String, &'static str> {
    let normalized = value.replace('\\', "/");
    if normalized.is_empty()
        || normalized.starts_with('/')
        || normalized.contains(":/")
        || normalized.split('/').any(|part| part == "..")
    {
        return Err("relative package must be a safe relative path");
    }
    Ok(normalized)
}

fn manifest_value(contents: &str, key: &str) -> Option<String> {
    contents.lines().find_map(|line| {
        let quoted = line.split('"').collect::<Vec<_>>();
        (quoted.len() >= 4 && quoted[1] == key).then(|| quoted[3].to_owned())
    })
}

fn read_u32_slice(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("validated u32 slice"),
    )
}

fn arguments() -> Result<Arguments, Box<dyn std::error::Error>> {
    let mut package = None;
    let mut relative_package = None;
    let mut meta = None;
    let mut relative_meta = None;
    let mut steam_manifest = None;
    let mut expected_build = None;
    let mut deployment_id = None;
    let mut channel = None;
    let mut output = None;
    let mut args = env::args_os().skip(1);
    while let Some(argument) = args.next() {
        match argument.to_string_lossy().as_ref() {
            "--package" => package = Some(PathBuf::from(next_value(&mut args, "--package")?)),
            "--relative-package" => {
                relative_package = Some(
                    next_value(&mut args, "--relative-package")?
                        .to_string_lossy()
                        .into_owned(),
                )
            }
            "--meta" => meta = Some(PathBuf::from(next_value(&mut args, "--meta")?)),
            "--relative-meta" => {
                relative_meta = Some(
                    next_value(&mut args, "--relative-meta")?
                        .to_string_lossy()
                        .into_owned(),
                )
            }
            "--steam-manifest" => {
                steam_manifest = Some(PathBuf::from(next_value(&mut args, "--steam-manifest")?))
            }
            "--expected-build" => {
                expected_build = Some(
                    next_value(&mut args, "--expected-build")?
                        .to_string_lossy()
                        .into_owned(),
                )
            }
            "--deployment" => {
                deployment_id = Some(
                    next_value(&mut args, "--deployment")?
                        .to_string_lossy()
                        .into_owned(),
                )
            }
            "--channel" => {
                channel = Some(
                    next_value(&mut args, "--channel")?
                        .to_string_lossy()
                        .into_owned(),
                )
            }
            "--output" => output = Some(PathBuf::from(next_value(&mut args, "--output")?)),
            unknown => return Err(format!("unknown argument {unknown}").into()),
        }
    }
    Ok(Arguments {
        package: package.ok_or("missing --package")?,
        relative_package: relative_package.ok_or("missing --relative-package")?,
        meta: meta.ok_or("missing --meta")?,
        relative_meta: relative_meta.ok_or("missing --relative-meta")?,
        steam_manifest: steam_manifest.ok_or("missing --steam-manifest")?,
        expected_build: expected_build.ok_or("missing --expected-build")?,
        deployment_id: deployment_id.ok_or("missing --deployment")?,
        channel: channel.ok_or("missing --channel")?,
        output: output.ok_or("missing --output")?,
    })
}

fn next_value(
    args: &mut impl Iterator<Item = OsString>,
    flag: &str,
) -> Result<OsString, Box<dyn std::error::Error>> {
    args.next()
        .ok_or_else(|| format!("missing value for {flag}").into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plausible_shapes_require_integral_bounded_rows() {
        assert_eq!(plausible_shape(3, 87), Some(29));
        assert_eq!(plausible_shape(3, 88), None);
        assert_eq!(plausible_shape(0, 0), None);
        assert_eq!(plausible_shape(1, MAX_ROW_SIZE + 1), None);
    }

    #[test]
    fn manifest_parser_reads_exact_quoted_key() {
        let manifest = "\"appid\"\t\t\"3681810\"\n\"buildid\"\t\t\"24568685\"\n";
        assert_eq!(
            manifest_value(manifest, "appid").as_deref(),
            Some("3681810")
        );
        assert_eq!(
            manifest_value(manifest, "buildid").as_deref(),
            Some("24568685")
        );
        assert_eq!(manifest_value(manifest, "id"), None);
    }

    #[test]
    fn relative_paths_cannot_escape_or_be_absolute() {
        assert_eq!(
            normalize_relative_path("bpsr\\BPSR_STEAM_Data\\m0.pkg").unwrap(),
            "bpsr/BPSR_STEAM_Data/m0.pkg"
        );
        assert!(normalize_relative_path("G:\\game\\m0.pkg").is_err());
        assert!(normalize_relative_path("../m0.pkg").is_err());
    }

    #[test]
    fn minimum_extent_includes_headers_index_rows_and_pools() {
        assert_eq!(minimum_table_bytes(3, 87).unwrap(), 20 + 24 + 87 + 80);
    }

    #[test]
    fn hash33_matches_reviewed_table_keys() {
        assert_eq!(hash33("BuffTable.ctb"), 983_121_143);
        assert_eq!(hash33("SkillTable.ctb"), 3_004_324_915);
        assert_eq!(hash33("SkillFightLevelTable.ctb"), 2_264_782_525);
    }
}
