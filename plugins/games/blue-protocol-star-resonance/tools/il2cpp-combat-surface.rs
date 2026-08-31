//! Build-locked extraction of the BPSR IL2CPP combat metadata surface.
//!
//! This is an offline research tool. It consumes Il2CppDumper's `dump.cs`
//! output and records exact enum identities, packet/message fields, and method
//! RVAs needed to audit combat decoding after a client update. It does not run
//! in capture, decoding, combat reduction, or the desktop UI.

use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    error::Error,
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Write},
    path::PathBuf,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 2;
const TARGET_TYPES: &[&str] = &[
    "EAttrType",
    "EBuffEffectLogicPbType",
    "EBuffEventType",
    "EDamageMode",
    "EDamageProperty",
    "EDamageSource",
    "EDamageType",
    "BuffInfo",
    "ClientHitPartInfo",
    "SyncDamageInfo",
    "DamageAttrTableBase",
    "DamageDataMgr",
    "DamageSkillData",
    "DamageDataToLua",
    "FightAttrTableBase",
    "FightAttrTranTableBase",
    "UserFightAttr",
    "ZMixAttr<T>",
    "ZMixAddAttr<T>",
    "ZMixMultiplyAttr<T>",
    "ZMixMaxValueAttr<T>",
    "ZMixMaxIndexAttr<T>",
];

#[derive(Debug)]
struct Arguments {
    dump: PathBuf,
    identity: PathBuf,
    output: PathBuf,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ArtifactIdentity {
    byte_length: u64,
    sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata_version: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct BuildIdentity {
    game: String,
    deployment: String,
    channel: String,
    game_build: String,
    distribution_app_id: String,
    metadata: ArtifactIdentity,
    game_assembly: ArtifactIdentity,
}

#[derive(Debug, Serialize)]
struct CombatSurface {
    schema_version: u16,
    generated_by: &'static str,
    game: String,
    deployment: String,
    channel: String,
    distribution_app_id: String,
    build_id: String,
    source_identity: SourceIdentity,
    policy: Policy,
    summary: Summary,
    fight_attribute_families: Vec<FightAttributeFamily>,
    types: Vec<TypeSurface>,
    findings: Findings,
}

#[derive(Debug, Serialize)]
struct SourceIdentity {
    metadata: ArtifactIdentity,
    game_assembly: ArtifactIdentity,
    il2cpp_dump: ArtifactIdentity,
}

#[derive(Debug, Serialize)]
struct Policy {
    offline_research_only: bool,
    runtime_formula_authority: bool,
    packet_occurrence_authoritative: bool,
    native_addresses_retained: bool,
    unresolved_evidence_hidden: bool,
    exact_build_packet_replay_required_for_promotion: bool,
}

#[derive(Debug, Serialize)]
struct Summary {
    requested_types: usize,
    resolved_types: usize,
    enum_values: usize,
    fields: usize,
    properties: usize,
    methods: usize,
    fight_attribute_values: usize,
    fight_attribute_families: usize,
    combat_relevant_fight_attribute_families: usize,
}

#[derive(Debug, Clone, Serialize)]
struct TypeSurface {
    namespace: String,
    name: String,
    kind: String,
    type_def_index: Option<u32>,
    declaration: String,
    enum_values: Vec<EnumValue>,
    fields: Vec<String>,
    properties: Vec<String>,
    methods: Vec<MethodSurface>,
}

#[derive(Debug, Clone, Serialize)]
struct EnumValue {
    name: String,
    value: i64,
}

#[derive(Debug, Clone, Serialize)]
struct MethodSurface {
    signature: String,
    rva: Option<String>,
    offset: Option<String>,
}

#[derive(Debug, Serialize)]
struct FightAttributeFamily {
    base_id: i64,
    base_name: String,
    combat_relevant: bool,
    members: Vec<EnumValue>,
}

#[derive(Debug, Serialize)]
struct Findings {
    authoritative_damage_formula_location: &'static str,
    client_damage_entrypoint: &'static str,
    client_damage_entrypoint_state: &'static str,
    damage_attr_table_role: &'static str,
    sync_damage_wire_role: &'static str,
    buff_and_attribute_role: &'static str,
    promotion_boundary: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    None,
    Fields,
    Properties,
    Methods,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("IL2CPP combat surface extraction failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = arguments()?;
    let identity: BuildIdentity =
        serde_json::from_reader(BufReader::new(File::open(&args.identity)?))?;
    let mut dump_bytes = Vec::new();
    File::open(&args.dump)?.read_to_end(&mut dump_bytes)?;
    let dump_text = std::str::from_utf8(&dump_bytes)?;
    let mut types = parse_target_types(dump_text, TARGET_TYPES)?;
    types.sort_by(|left, right| left.name.cmp(&right.name));

    let attr_type = types
        .iter()
        .find(|surface| surface.name == "EAttrType")
        .ok_or("EAttrType was not extracted")?;
    let fight_attribute_families = fight_attribute_families(&attr_type.enum_values);
    let combat_relevant_families = fight_attribute_families
        .iter()
        .filter(|family| family.combat_relevant)
        .count();
    let report = CombatSurface {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-il2cpp-combat-surface",
        game: identity.game,
        deployment: identity.deployment,
        channel: identity.channel,
        distribution_app_id: identity.distribution_app_id,
        build_id: identity.game_build,
        source_identity: SourceIdentity {
            metadata: identity.metadata,
            game_assembly: identity.game_assembly,
            il2cpp_dump: ArtifactIdentity {
                byte_length: dump_bytes.len() as u64,
                sha256: hex_digest(&dump_bytes),
                metadata_version: None,
            },
        },
        policy: Policy {
            offline_research_only: true,
            runtime_formula_authority: false,
            packet_occurrence_authoritative: true,
            native_addresses_retained: true,
            unresolved_evidence_hidden: false,
            exact_build_packet_replay_required_for_promotion: true,
        },
        summary: Summary {
            requested_types: TARGET_TYPES.len(),
            resolved_types: types.len(),
            enum_values: types.iter().map(|surface| surface.enum_values.len()).sum(),
            fields: types.iter().map(|surface| surface.fields.len()).sum(),
            properties: types.iter().map(|surface| surface.properties.len()).sum(),
            methods: types.iter().map(|surface| surface.methods.len()).sum(),
            fight_attribute_values: attr_type.enum_values.len(),
            fight_attribute_families: fight_attribute_families.len(),
            combat_relevant_fight_attribute_families: combat_relevant_families,
        },
        fight_attribute_families,
        types,
        findings: Findings {
            authoritative_damage_formula_location: "server-side; absent from the client IL2CPP surface",
            client_damage_entrypoint: "Panda.ZGame.DamageDataMgr.SetEffectData(long, SyncDamageInfo, int)",
            client_damage_entrypoint_state: "aggregation and presentation consumer of packet-provided SyncDamageInfo",
            damage_attr_table_role: "current-build candidate coefficient, fixed-parameter, state, part-damage, property, tag, and presentation surface; DamageScript names a server operator but does not ship its implementation",
            sync_damage_wire_role: "authoritative server result and exact selection evidence; the client receives value, actual value, lucky value, HP and shield loss, source, owner, level, stage, hit, property, part, summoner, passive, weight, and mode fields",
            buff_and_attribute_role: "exact identity, wire-state, lifecycle, and candidate coefficient surface",
            promotion_boundary: "current-build packet replay must prove provider, recipient, magnitude, stage order, rounding, and conserved damage delta",
        },
    };

    if let Some(parent) = args.output.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(&args.output)?);
    serde_json::to_writer_pretty(&mut writer, &report)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    println!(
        "wrote {} types, {} fight attributes, and {} fight-attribute families to {}",
        report.summary.resolved_types,
        report.summary.fight_attribute_values,
        report.summary.fight_attribute_families,
        args.output.display()
    );
    Ok(())
}

fn parse_target_types(
    dump: &str,
    target_names: &[&str],
) -> Result<Vec<TypeSurface>, Box<dyn Error>> {
    let targets = target_names.iter().copied().collect::<BTreeSet<_>>();
    let lines = dump.lines().collect::<Vec<_>>();
    let mut namespace = String::new();
    let mut found = BTreeMap::<String, TypeSurface>::new();
    let mut index = 0usize;

    while index < lines.len() {
        let trimmed = lines[index].trim();
        if let Some(value) = trimmed.strip_prefix("// Namespace:") {
            namespace = value.trim().to_string();
            index += 1;
            continue;
        }
        let Some((kind, name)) = declaration_identity(trimmed) else {
            index += 1;
            continue;
        };
        if !targets.contains(name.as_str()) {
            index += 1;
            continue;
        }
        let (surface, next) = parse_type_block(&lines, index, &namespace, &kind, &name)?;
        if found.insert(name.clone(), surface).is_some() {
            return Err(format!("duplicate target type {name}").into());
        }
        index = next;
    }

    let missing = targets
        .iter()
        .filter(|name| !found.contains_key(**name))
        .copied()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!("missing target IL2CPP types: {}", missing.join(", ")).into());
    }
    Ok(found.into_values().collect())
}

fn declaration_identity(line: &str) -> Option<(String, String)> {
    for kind in ["enum", "class", "struct", "interface"] {
        let marker = format!(" {kind} ");
        let Some(start) = line.find(&marker).map(|start| start + marker.len()) else {
            continue;
        };
        let remainder = &line[start..];
        let name = remainder
            .split(|character: char| character.is_whitespace() || character == ':')
            .next()?
            .trim();
        if !name.is_empty() {
            return Some((kind.to_string(), name.to_string()));
        }
    }
    None
}

fn parse_type_block(
    lines: &[&str],
    start: usize,
    namespace: &str,
    kind: &str,
    name: &str,
) -> Result<(TypeSurface, usize), Box<dyn Error>> {
    let declaration = lines[start].trim().to_string();
    let type_def_index = declaration
        .split("TypeDefIndex:")
        .nth(1)
        .and_then(|value| value.trim_end_matches(')').trim().parse().ok());
    let mut surface = TypeSurface {
        namespace: namespace.to_string(),
        name: name.to_string(),
        kind: kind.to_string(),
        type_def_index,
        declaration,
        enum_values: Vec::new(),
        fields: Vec::new(),
        properties: Vec::new(),
        methods: Vec::new(),
    };
    let mut depth = brace_delta(lines[start]);
    let mut entered_block = depth > 0;
    let mut section = Section::None;
    let mut pending_rva = None;
    let mut pending_offset = None;
    let mut index = start + 1;

    while index < lines.len() && (!entered_block || depth > 0) {
        let line = lines[index];
        let trimmed = line.trim();
        match trimmed {
            "// Fields" => section = Section::Fields,
            "// Properties" => section = Section::Properties,
            "// Methods" => section = Section::Methods,
            _ => match section {
                Section::Fields => {
                    if kind == "enum" {
                        if let Some(value) = parse_enum_value(trimmed) {
                            surface.enum_values.push(value);
                        }
                    } else if let Some(member) = member_signature(trimmed, false) {
                        surface.fields.push(member);
                    }
                }
                Section::Properties => {
                    if let Some(member) = member_signature(trimmed, true) {
                        surface.properties.push(member);
                    }
                }
                Section::Methods => {
                    if trimmed.starts_with("// RVA:") {
                        pending_rva = metadata_token(trimmed, "RVA:");
                        pending_offset = metadata_token(trimmed, "Offset:");
                    } else if is_method_signature(trimmed) {
                        surface.methods.push(MethodSurface {
                            signature: trimmed.to_string(),
                            rva: pending_rva.take(),
                            offset: pending_offset.take(),
                        });
                    }
                }
                Section::None => {}
            },
        }
        let delta = brace_delta(line);
        if delta > 0 {
            entered_block = true;
        }
        depth += delta;
        index += 1;
    }
    if !entered_block || depth != 0 {
        return Err(format!("unterminated type block for {name}").into());
    }
    Ok((surface, index))
}

fn brace_delta(line: &str) -> i32 {
    line.chars().fold(0, |depth, character| match character {
        '{' => depth + 1,
        '}' => depth - 1,
        _ => depth,
    })
}

fn parse_enum_value(line: &str) -> Option<EnumValue> {
    let remainder = line.split(" const ").nth(1)?;
    let (left, right) = remainder.split_once('=')?;
    let name = left.split_whitespace().last()?.trim().to_string();
    let value = right.trim().trim_end_matches(';').parse().ok()?;
    Some(EnumValue { name, value })
}

fn member_signature(line: &str, property: bool) -> Option<String> {
    if line.is_empty() || line.starts_with("//") || line.starts_with('[') {
        return None;
    }
    let signature = line.split("//").next()?.trim();
    let valid = if property {
        signature.contains("{ get;") || signature.contains("{ set;")
    } else {
        signature.ends_with(';')
    };
    valid.then(|| signature.to_string())
}

fn is_method_signature(line: &str) -> bool {
    !line.starts_with("//")
        && !line.starts_with('[')
        && line.contains('(')
        && (line.ends_with("{ }") || line.ends_with(';'))
}

fn metadata_token(line: &str, label: &str) -> Option<String> {
    let value = line.split(label).nth(1)?.trim().split_whitespace().next()?;
    (value != "-1").then(|| value.to_string())
}

fn fight_attribute_families(values: &[EnumValue]) -> Vec<FightAttributeFamily> {
    let mut grouped = BTreeMap::<i64, Vec<EnumValue>>::new();
    for value in values.iter().filter(|value| value.value >= 10_000) {
        let member = value.value.rem_euclid(10);
        let base_id = if member <= 5 {
            value.value - member
        } else {
            value.value
        };
        grouped.entry(base_id).or_default().push(value.clone());
    }
    grouped
        .into_iter()
        .map(|(base_id, mut members)| {
            members.sort_by_key(|member| member.value);
            let base_name = members
                .iter()
                .find(|member| member.value == base_id)
                .map(|member| member.name.clone())
                .unwrap_or_else(|| members[0].name.clone());
            FightAttributeFamily {
                base_id,
                combat_relevant: combat_relevant(&base_name),
                base_name,
                members,
            }
        })
        .collect()
}

fn combat_relevant(name: &str) -> bool {
    [
        "Attack",
        "Damage",
        "Defense",
        "Crit",
        "Cri",
        "Luck",
        "Mastery",
        "Versatility",
        "Season",
        "Element",
        "Power",
        "Haste",
        "Speed",
        "Hp",
        "HP",
        "Heal",
        "Shield",
        "Block",
        "Hit",
    ]
    .iter()
    .any(|token| name.contains(token))
}

fn arguments() -> Result<Arguments, Box<dyn Error>> {
    let mut values = BTreeMap::new();
    let mut args = env::args().skip(1);
    while let Some(argument) = args.next() {
        let key = argument
            .strip_prefix("--")
            .ok_or_else(|| format!("unexpected positional argument {argument}"))?;
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for --{key}"))?;
        values.insert(key.to_string(), value);
    }
    Ok(Arguments {
        dump: required_path(&values, "dump")?,
        identity: required_path(&values, "identity")?,
        output: required_path(&values, "output")?,
    })
}

fn required_path(values: &BTreeMap<String, String>, key: &str) -> Result<PathBuf, Box<dyn Error>> {
    values
        .get(key)
        .map(PathBuf::from)
        .ok_or_else(|| format!("missing --{key}").into())
}

fn hex_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_enum_and_method_surface() {
        let dump = r#"// Namespace: Example
public enum EAttrType // TypeDefIndex: 1
{
    // Fields
    public int value__; // 0x0
    public const EAttrType AttrAttack = 11330;
    public const EAttrType AttrAttackTotal = 11331;
}

// Namespace: Example
public class DamageDataMgr // TypeDefIndex: 2
{
    // Fields
    private int value; // 0x10
    // Properties
    public int Value { get; }
    // Methods
    // RVA: 0x123 Offset: 0x100 VA: 0x180000123
    public void SetEffectData() { }
}
"#;
        let surfaces = parse_target_types(dump, &["EAttrType", "DamageDataMgr"]).unwrap();
        let attributes = surfaces
            .iter()
            .find(|surface| surface.name == "EAttrType")
            .unwrap();
        assert_eq!(attributes.enum_values.len(), 2);
        let manager = surfaces
            .iter()
            .find(|surface| surface.name == "DamageDataMgr")
            .unwrap();
        assert_eq!(manager.fields.len(), 1);
        assert_eq!(manager.properties.len(), 1);
        assert_eq!(manager.methods[0].rva.as_deref(), Some("0x123"));
    }

    #[test]
    fn groups_six_member_fight_attribute_family() {
        let values = (0..=5)
            .map(|offset| EnumValue {
                name: format!("AttrAttack{offset}"),
                value: 11_330 + offset,
            })
            .collect::<Vec<_>>();
        let families = fight_attribute_families(&values);
        assert_eq!(families.len(), 1);
        assert_eq!(families[0].base_id, 11_330);
        assert_eq!(families[0].members.len(), 6);
        assert!(families[0].combat_relevant);
    }
}
