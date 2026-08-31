use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsString,
    fs::{self, File},
    io::{BufWriter, Write},
    path::PathBuf,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 1;
const TABLE_BASE_SUFFIX: &str = "TableBase";

#[derive(Debug)]
struct Arguments {
    inventory: PathBuf,
    dumps: Vec<PathBuf>,
    output: PathBuf,
}

#[derive(Debug, Deserialize)]
struct Inventory {
    deployment_id: String,
    channel: String,
    distribution_app_id: String,
    build_id: String,
    tables: Vec<InventoryTable>,
}

#[derive(Debug, Deserialize)]
struct InventoryTable {
    address_keys: Vec<AddressKey>,
}

#[derive(Debug, Deserialize)]
struct AddressKey {
    key: u32,
}

#[derive(Debug, Serialize)]
struct IdentityOverlay {
    schema_version: u16,
    generated_by: &'static str,
    game: &'static str,
    deployment_id: String,
    channel: String,
    distribution_app_id: String,
    build_id: String,
    policy: Policy,
    summary: Summary,
    sources: Vec<Source>,
    identities: Vec<Identity>,
}

#[derive(Debug, Serialize)]
struct Policy {
    hash_algorithm: &'static str,
    class_pattern: &'static str,
    exact_hash_match_required: bool,
    unmatched_inventory_tables_hidden: bool,
    non_hash_guesses_auto_promoted: bool,
}

#[derive(Debug, Serialize)]
struct Summary {
    inventory_tables: usize,
    distinct_table_base_names: usize,
    exact_hash_matches: usize,
    unmatched_inventory_tables: usize,
    names_seen_in_all_dumps: usize,
    names_seen_in_one_dump: usize,
}

#[derive(Debug, Serialize)]
struct Source {
    file_name: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct Identity {
    table_key: u32,
    table_key_hex: String,
    table_name: String,
    stable_key: String,
    domain: &'static str,
    evidence: IdentityEvidence,
}

#[derive(Debug, Serialize)]
struct IdentityEvidence {
    hash33_verified: bool,
    dump_source_count: usize,
    dump_sources: Vec<String>,
    generated_class_name: String,
    normalization: &'static str,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("CTB table-name resolver failed: {error}");
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

    let inventory: Inventory = serde_json::from_slice(&fs::read(&arguments.inventory)?)?;
    let inventory_keys = inventory_keys(&inventory.tables)?;
    let mut candidates = BTreeMap::<String, BTreeSet<String>>::new();
    let mut sources = Vec::new();
    for dump in &arguments.dumps {
        let bytes = fs::read(dump)?;
        let source_name = source_label(dump)?;
        for name in extract_table_base_names(&bytes) {
            candidates
                .entry(name)
                .or_default()
                .insert(source_name.clone());
        }
        sources.push(Source {
            file_name: source_name,
            bytes: bytes.len() as u64,
            sha256: format!("sha256:{:x}", Sha256::digest(&bytes)),
        });
    }

    let mut identities = Vec::new();
    for (generated_class_name, dump_sources) in &candidates {
        let table_stem = generated_class_name
            .strip_suffix("Base")
            .expect("extracted class name must end in Base");
        let table_name = format!("{table_stem}.ctb");
        let table_key = hash33(&table_name);
        if !inventory_keys.contains(&table_key) {
            continue;
        }
        identities.push(Identity {
            table_key,
            table_key_hex: format!("0x{table_key:08x}"),
            stable_key: format!("ctb.{table_stem}"),
            domain: domain_for(table_stem),
            table_name,
            evidence: IdentityEvidence {
                hash33_verified: true,
                dump_source_count: dump_sources.len(),
                dump_sources: dump_sources.iter().cloned().collect(),
                generated_class_name: generated_class_name.clone(),
                normalization: "strip the generated class suffix Base, append .ctb, then verify seeded hash33",
            },
        });
    }
    identities.sort_by_key(|identity| identity.table_key);
    ensure_unique_identities(&identities)?;

    let names_seen_in_all_dumps = candidates
        .values()
        .filter(|sources| sources.len() == arguments.dumps.len())
        .count();
    let names_seen_in_one_dump = candidates
        .values()
        .filter(|sources| sources.len() == 1)
        .count();
    let output = IdentityOverlay {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-ctb-table-name-resolver",
        game: "blue-protocol-star-resonance",
        deployment_id: inventory.deployment_id,
        channel: inventory.channel,
        distribution_app_id: inventory.distribution_app_id,
        build_id: inventory.build_id,
        policy: Policy {
            hash_algorithm: "djb2/hash33 seeded with 5381 using wrapping u32 arithmetic",
            class_pattern: "generated IL2CPP class identifier ending in TableBase",
            exact_hash_match_required: true,
            unmatched_inventory_tables_hidden: false,
            non_hash_guesses_auto_promoted: false,
        },
        summary: Summary {
            inventory_tables: inventory_keys.len(),
            distinct_table_base_names: candidates.len(),
            exact_hash_matches: identities.len(),
            unmatched_inventory_tables: inventory_keys.len().saturating_sub(identities.len()),
            names_seen_in_all_dumps,
            names_seen_in_one_dump,
        },
        sources,
        identities,
    };

    if let Some(parent) = arguments.output.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(&arguments.output)?);
    serde_json::to_writer_pretty(&mut writer, &output)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn inventory_keys(tables: &[InventoryTable]) -> Result<BTreeSet<u32>, String> {
    let mut keys = BTreeSet::new();
    for table in tables {
        if table.address_keys.len() != 1 {
            return Err(format!(
                "inventory table has {} address keys",
                table.address_keys.len()
            ));
        }
        if !keys.insert(table.address_keys[0].key) {
            return Err(format!(
                "duplicate inventory table key {}",
                table.address_keys[0].key
            ));
        }
    }
    Ok(keys)
}

fn extract_table_base_names(bytes: &[u8]) -> BTreeSet<String> {
    let text = String::from_utf8_lossy(bytes);
    text.split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|token| {
            token.ends_with(TABLE_BASE_SUFFIX)
                && token
                    .as_bytes()
                    .first()
                    .is_some_and(|first| first.is_ascii_alphabetic() || *first == b'_')
        })
        .map(ToOwned::to_owned)
        .collect()
}

fn source_label(path: &std::path::Path) -> Result<String, &'static str> {
    let mut components = path
        .components()
        .rev()
        .filter_map(|component| component.as_os_str().to_str());
    let file = components
        .next()
        .ok_or("dump path has no UTF-8 file name")?;
    let parent = components.next();
    Ok(parent.map_or_else(|| file.to_owned(), |parent| format!("{parent}/{file}")))
}

fn ensure_unique_identities(identities: &[Identity]) -> Result<(), String> {
    let mut keys = BTreeSet::new();
    for identity in identities {
        if !keys.insert(identity.table_key) {
            return Err(format!(
                "multiple generated table names hash to inventory key {}",
                identity.table_key
            ));
        }
    }
    Ok(())
}

fn domain_for(table_stem: &str) -> &'static str {
    if table_stem.starts_with("Item") {
        "items-and-equipment"
    } else if table_stem.contains("Skill")
        || table_stem.contains("Buff")
        || table_stem.contains("Damage")
        || table_stem.contains("Fight")
    {
        "combat"
    } else {
        "unreviewed"
    }
}

fn hash33(value: &str) -> u32 {
    value.chars().fold(5381_u32, |hash, character| {
        hash.wrapping_mul(33).wrapping_add(character as u32)
    })
}

fn arguments() -> Result<Arguments, Box<dyn std::error::Error>> {
    let mut inventory = None;
    let mut dumps = Vec::new();
    let mut output = None;
    let mut args = env::args_os().skip(1);
    while let Some(argument) = args.next() {
        match argument.to_string_lossy().as_ref() {
            "--inventory" => inventory = Some(PathBuf::from(next_value(&mut args, "--inventory")?)),
            "--dump" => dumps.push(PathBuf::from(next_value(&mut args, "--dump")?)),
            "--output" => output = Some(PathBuf::from(next_value(&mut args, "--output")?)),
            unknown => return Err(format!("unknown argument {unknown}").into()),
        }
    }
    if dumps.is_empty() {
        return Err("at least one --dump is required".into());
    }
    Ok(Arguments {
        inventory: inventory.ok_or("missing --inventory")?,
        dumps,
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
    fn hash33_matches_reviewed_table_keys() {
        assert_eq!(hash33("BuffTable.ctb"), 983_121_143);
        assert_eq!(hash33("SkillTable.ctb"), 3_004_324_915);
        assert_eq!(hash33("SkillFightLevelTable.ctb"), 2_264_782_525);
        assert_eq!(hash33("ItemTempTable.ctb"), 2_785_161_081);
    }

    #[test]
    fn extracts_generated_table_base_identifiers_only() {
        let names = extract_table_base_names(
            b"public class ItemTempTableBase { } public class NotATable { } SkillTableBase",
        );
        assert_eq!(
            names,
            BTreeSet::from(["ItemTempTableBase".to_owned(), "SkillTableBase".to_owned()])
        );
    }

    #[test]
    fn classifies_item_temp_table_without_affecting_identity() {
        assert_eq!(domain_for("ItemTempTable"), "items-and-equipment");
        assert_eq!(domain_for("RogueTable"), "unreviewed");
    }

    #[test]
    fn source_labels_distinguish_same_named_dump_files_without_absolute_paths() {
        assert_eq!(
            source_label(std::path::Path::new("one/dump.cs")),
            Ok("one/dump.cs".to_owned())
        );
        assert_eq!(
            source_label(std::path::Path::new("two/dump.cs")),
            Ok("two/dump.cs".to_owned())
        );
    }
}
