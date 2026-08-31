use std::{
    env,
    fs::{self, File},
    io::{BufWriter, Read, Write},
    path::{Path, PathBuf},
    process::Command,
};

use serde::Serialize;
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 1;
const ENUM_RELATIVE_PATH: &str = "common/enum_define.lua";
const WEAPON_VM_RELATIVE_PATH: &str = "ui/view_model/weapon_skill_vm.lua";
const FIGHT_ATTR_VM_RELATIVE_PATH: &str = "ui/view_model/attr_parse/fight_attr_parse_vm.lua";

#[derive(Debug, Serialize)]
struct Proof {
    schema_version: u16,
    game_build: String,
    purpose: &'static str,
    source: Source,
    remodel_info_type: RemodelInfoType,
    consumer_chain: Vec<ConsumerStep>,
    assertions: Assertions,
    proof_state: &'static str,
    runtime_authority: bool,
}

#[derive(Debug, Serialize)]
struct Source {
    lua_root: &'static str,
    decompiler: String,
    decompiler_sha256: String,
    bytecode_files: Vec<BytecodeFile>,
}

#[derive(Debug, Serialize)]
struct BytecodeFile {
    path: &'static str,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct RemodelInfoType {
    attribute: i64,
    buff: i64,
}

#[derive(Debug, Serialize)]
struct ConsumerStep {
    function: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<i64>,
    behavior: &'static str,
}

#[derive(Debug, Serialize)]
struct Assertions {
    kind_1_is_direct_attribute_not_buff: bool,
    kind_3_is_buff_reference: bool,
    attribute_tuple_layout: [&'static str; 3],
    buff_tuple_layout: [&'static str; 3],
}

fn main() {
    if let Err(error) = run() {
        eprintln!("BPSR Aoyi remodel consumer proof failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = env::args_os().skip(1);
    let mut lua_root = None;
    let mut decompiler = None;
    let mut game_build = None;
    let mut output = None;
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--lua-root") => lua_root = arguments.next().map(PathBuf::from),
            Some("--decompiler") => decompiler = arguments.next().map(PathBuf::from),
            Some("--build") => {
                game_build = arguments.next().and_then(|value| value.into_string().ok())
            }
            Some("--output") => output = arguments.next().map(PathBuf::from),
            _ => return Err(usage().into()),
        }
    }

    let lua_root = lua_root.ok_or_else(usage)?;
    let decompiler = decompiler.ok_or_else(usage)?;
    let game_build = game_build.ok_or_else(usage)?;
    let output = output.ok_or_else(usage)?;
    if game_build.trim().is_empty() {
        return Err("game build must not be empty".into());
    }

    let enum_path = lua_root.join(path_from_forward_slashes(ENUM_RELATIVE_PATH));
    let weapon_path = lua_root.join(path_from_forward_slashes(WEAPON_VM_RELATIVE_PATH));
    let fight_attr_path = lua_root.join(path_from_forward_slashes(FIGHT_ATTR_VM_RELATIVE_PATH));
    for path in [&enum_path, &weapon_path, &fight_attr_path, &decompiler] {
        if !path.is_file() {
            return Err(format!("required input does not exist: {}", path.display()).into());
        }
    }

    let enum_source = decompile(&decompiler, &enum_path)?;
    let weapon_source = decompile(&decompiler, &weapon_path)?;
    let fight_attr_source = decompile(&decompiler, &fight_attr_path)?;
    verify_consumer_sources(&enum_source, &weapon_source, &fight_attr_source)?;

    let proof = Proof {
        schema_version: SCHEMA_VERSION,
        game_build,
        purpose: "Build-locked proof for interpreting SkillAoyiTable and SkillAoyiStarTable TransformationType tuples without guessing their numeric kind.",
        source: Source {
            lua_root: "<exact-build-full-extract>/Luac/lua",
            decompiler: "cLuaDecompiler".to_owned(),
            decompiler_sha256: sha256_file(&decompiler)?,
            bytecode_files: vec![
                BytecodeFile {
                    path: ENUM_RELATIVE_PATH,
                    sha256: sha256_file(&enum_path)?,
                },
                BytecodeFile {
                    path: WEAPON_VM_RELATIVE_PATH,
                    sha256: sha256_file(&weapon_path)?,
                },
                BytecodeFile {
                    path: FIGHT_ATTR_VM_RELATIVE_PATH,
                    sha256: sha256_file(&fight_attr_path)?,
                },
            ],
        },
        remodel_info_type: RemodelInfoType {
            attribute: 1,
            buff: 3,
        },
        consumer_chain: vec![
            ConsumerStep {
                function: "WeaponSkillVM.ParseResonanceTransformation",
                kind: Some(1),
                behavior: "Reads tuple positions 2 and 3 as attrId and attrValue, then calls fightAttrParseVM.ParseFightAttrTips(attrId, attrValue).",
            },
            ConsumerStep {
                function: "WeaponSkillVM.ParseResonanceTransformation",
                kind: Some(3),
                behavior: "Reads tuple position 2 as buffId, resolves BuffTable, and calls buffAttrParseVM.ParseBufferTips with the matching BuffPar set.",
            },
            ConsumerStep {
                function: "fight_attr_parse_vm.GetFightAttrTableRow",
                kind: None,
                behavior: "Uses the attribute ID's final decimal digit as the component format type and subtracts it to resolve the base FightAttrTable row.",
            },
        ],
        assertions: Assertions {
            kind_1_is_direct_attribute_not_buff: true,
            kind_3_is_buff_reference: true,
            attribute_tuple_layout: ["kind", "attribute_id", "raw_value"],
            buff_tuple_layout: ["kind", "buff_id", "parameter_set_index"],
        },
        proof_state: "exact-current-build-decompiled-client-consumer",
        runtime_authority: false,
    };

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(output)?);
    serde_json::to_writer_pretty(&mut writer, &proof)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn verify_consumer_sources(
    enum_source: &str,
    weapon_source: &str,
    fight_attr_source: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    require_all(
        "enum_define.lua",
        enum_source,
        &["E.RemodelInfoType", "Attr = 1", "Buff = 3"],
    )?;
    require_all(
        "weapon_skill_vm.lua",
        weapon_source,
        &[
            "ParseResonanceTransformation",
            "type == E.RemodelInfoType.Attr",
            "local attrId = value[2]",
            "local attrValue = value[3]",
            "fightAttrParseVM.ParseFightAttrTips(attrId, attrValue)",
            "type == E.RemodelInfoType.Buff",
            "local buffId = value[2]",
            "buffAttrParseVM.ParseBufferTips(buffId, param)",
        ],
    )?;
    require_all(
        "fight_attr_parse_vm.lua",
        fight_attr_source,
        &[
            "function ret.GetFightAttrTableRow(fightAttrId)",
            "local formatType = fightAttrId % 10",
            "local configId = fightAttrId - formatType",
            "fightTableMgr.GetRow(configId, true)",
        ],
    )?;
    Ok(())
}

fn require_all(
    label: &str,
    source: &str,
    required: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    let missing = required
        .iter()
        .copied()
        .filter(|needle| !source.contains(needle))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!("{label} is missing required consumer evidence: {missing:?}").into())
    }
}

fn decompile(decompiler: &Path, bytecode: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new(decompiler)
        .arg("--dec")
        .arg(bytecode)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "decompiler failed for {}: {}",
            bytecode.display(),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

fn sha256_file(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn path_from_forward_slashes(value: &str) -> PathBuf {
    value.split('/').collect()
}

fn usage() -> &'static str {
    "usage: rlogs-bpsr-aoyi-remodel-consumer-proof --lua-root <exact-build-Luac/lua> --decompiler <cLuaDecompiler.exe> --build <client-build> --output <proof.json>"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_consumer_chain_passes() {
        let enum_source = "E.RemodelInfoType = {Attr = 1, Buff = 3}";
        let weapon_source = r#"
ParseResonanceTransformation
if type == E.RemodelInfoType.Attr then
local attrId = value[2]
local attrValue = value[3]
fightAttrParseVM.ParseFightAttrTips(attrId, attrValue)
elseif type == E.RemodelInfoType.Buff then
local buffId = value[2]
buffAttrParseVM.ParseBufferTips(buffId, param)
end
"#;
        let fight_source = r#"
function ret.GetFightAttrTableRow(fightAttrId)
local formatType = fightAttrId % 10
local configId = fightAttrId - formatType
fightTableMgr.GetRow(configId, true)
end
"#;
        verify_consumer_sources(enum_source, weapon_source, fight_source).unwrap();
    }

    #[test]
    fn changed_attribute_consumer_fails_closed() {
        let error = verify_consumer_sources(
            "E.RemodelInfoType = {Attr = 1, Buff = 3}",
            "ParseResonanceTransformation",
            "function ret.GetFightAttrTableRow(fightAttrId)",
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("weapon_skill_vm.lua"));
        assert!(error.contains("ParseFightAttrTips"));
    }
}
