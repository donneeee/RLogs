use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::{Value, json};

const SCHEMA_VERSION: u16 = 1;
const BLADE_SWEEP_SKILL_ID: i64 = 3_914;
const GOBLIN_MARCH_SKILL_ID: i64 = 3_946;
const BLADE_SWEEP_EFFECT_ID: i64 = 391_401;
const GOBLIN_MARCH_EFFECT_ID: i64 = 394_601;
const PROJECTILE_CONFIG_ID: i64 = 10_040_102;
const DAMAGE_ATTR_ID: i64 = 31_004_010_200;
const RECOUNT_ID: i64 = 270;
const TARGET_STATUS_ID: i64 = 2_110_092;

#[derive(Debug, Serialize)]
struct ProofArtifact {
    schema_version: u16,
    current_game_build: String,
    historical_packet_build: String,
    proof_state: &'static str,
    current_static: CurrentStaticProof,
    historical_packet: HistoricalPacketProof,
    ownership_limits: OwnershipLimits,
}

#[derive(Debug, Serialize)]
struct CurrentStaticProof {
    direct_owner_skill_id: i64,
    shared_owner_skill_ids: Vec<i64>,
    skill_effect_ids: Vec<i64>,
    projectile_config_id: i64,
    damage_attr_id: i64,
    recount_id: i64,
    target_status_id: i64,
    target_status_duration_seconds: f64,
    target_status_tags: Vec<i64>,
    projectile_duration_seconds: f64,
    projectile_hit_camp_types: Vec<i64>,
    damage_script: String,
}

#[derive(Debug, Serialize)]
struct HistoricalPacketProof {
    session_id: String,
    activity_id: String,
    source_actor_kind: String,
    source_actor_ids: Vec<String>,
    source_entity_uuids: Vec<String>,
    source_projectile_config_id: i64,
    target_actor_ids: Vec<String>,
    target_entity_uuids: Vec<String>,
    target_actor_kinds: Vec<String>,
    applied_count: u64,
    removed_count: u64,
}

#[derive(Debug, Serialize)]
struct OwnershipLimits {
    player_provider_identity_available_in_historical_projection: bool,
    current_build_packet_lifecycle_observed: bool,
    exact_owner_selection_rule: &'static str,
    rdps_gate: &'static str,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("BPSR Aoyi projectile status proof failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = env::args_os().skip(1);
    let decoded_root = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    let combat_history = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    let output = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    let current_game_build = utf8_argument(arguments.next(), "current game build")?;
    let historical_packet_build = utf8_argument(arguments.next(), "historical packet build")?;
    if arguments.next().is_some() {
        return Err(usage().into());
    }

    let proof = build_proof(
        &decoded_root,
        &combat_history,
        current_game_build,
        historical_packet_build,
    )?;
    let mut writer = BufWriter::new(File::create(output)?);
    serde_json::to_writer_pretty(&mut writer, &proof)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn build_proof(
    decoded_root: &Path,
    combat_history_path: &Path,
    current_game_build: String,
    historical_packet_build: String,
) -> Result<ProofArtifact, Box<dyn std::error::Error>> {
    let bullets = read_json(decoded_root.join("BulletTable.json"))?;
    let damage_attrs = read_json(decoded_root.join("DamageAttrTable.json"))?;
    let recounts = read_json(decoded_root.join("RecountTable.json"))?;
    let skill_effects = read_json(decoded_root.join("SkillEffectTable.json"))?;
    let buffs = read_json(decoded_root.join("BuffTable.json"))?;
    let history = read_json(combat_history_path)?;

    let bullet = required_row(&bullets, PROJECTILE_CONFIG_ID, "BulletTable")?;
    require_integer(
        bullet,
        "BulletAttrId",
        DAMAGE_ATTR_ID,
        "Blade Sweep projectile",
    )?;
    require_number(bullet, "Duration", 1.0, "Blade Sweep projectile")?;
    require_integer_array(bullet, "HitCampType", &[1], "Blade Sweep projectile")?;

    let damage = required_row(&damage_attrs, DAMAGE_ATTR_ID, "DamageAttrTable")?;
    require_integer(
        damage,
        "TypeEnum",
        PROJECTILE_CONFIG_ID,
        "Blade Sweep damage",
    )?;
    require_string(damage, "DamageScript", "AutoAttack", "Blade Sweep damage")?;

    let recount = required_row(&recounts, RECOUNT_ID, "RecountTable")?;
    require_array_contains_integer(recount, "DamageId", DAMAGE_ATTR_ID, "Blade Sweep recount")?;

    let direct_effect = required_row(&skill_effects, BLADE_SWEEP_EFFECT_ID, "SkillEffectTable")?;
    require_integer(
        direct_effect,
        "SkillId",
        BLADE_SWEEP_SKILL_ID,
        "Blade Sweep effect",
    )?;
    require_skill_description(direct_effect, DAMAGE_ATTR_ID, "Armor Penetration", "10s")?;

    let shared_effect = required_row(&skill_effects, GOBLIN_MARCH_EFFECT_ID, "SkillEffectTable")?;
    require_integer(
        shared_effect,
        "SkillId",
        GOBLIN_MARCH_SKILL_ID,
        "Goblin March effect",
    )?;
    require_skill_description(shared_effect, DAMAGE_ATTR_ID, "Armor Penetration", "10s")?;

    let status = required_row(&buffs, TARGET_STATUS_ID, "BuffTable")?;
    require_number_pair(
        status,
        "DestroyParam",
        0.0,
        10.0,
        "Blade Sweep target status",
    )?;
    require_integer_array(status, "Tags", &[78], "Blade Sweep target status")?;

    let actual_history_build = history
        .get("client_build")
        .and_then(Value::as_str)
        .ok_or("combat history is missing client_build")?;
    if actual_history_build != historical_packet_build {
        return Err(format!(
            "combat history build {actual_history_build} does not match requested historical build {historical_packet_build}"
        )
        .into());
    }
    let packet = packet_proof(&history)?;
    if packet.applied_count == 0 {
        return Err(
            "historical combat projection has no Blade Sweep target-status applications".into(),
        );
    }

    Ok(ProofArtifact {
        schema_version: SCHEMA_VERSION,
        current_game_build,
        historical_packet_build,
        proof_state: "current-static-chain-exact-plus-historical-projectile-status-edge-current-packet-provider-live-gated",
        current_static: CurrentStaticProof {
            direct_owner_skill_id: BLADE_SWEEP_SKILL_ID,
            shared_owner_skill_ids: vec![BLADE_SWEEP_SKILL_ID, GOBLIN_MARCH_SKILL_ID],
            skill_effect_ids: vec![BLADE_SWEEP_EFFECT_ID, GOBLIN_MARCH_EFFECT_ID],
            projectile_config_id: PROJECTILE_CONFIG_ID,
            damage_attr_id: DAMAGE_ATTR_ID,
            recount_id: RECOUNT_ID,
            target_status_id: TARGET_STATUS_ID,
            target_status_duration_seconds: 10.0,
            target_status_tags: vec![78],
            projectile_duration_seconds: 1.0,
            projectile_hit_camp_types: vec![1],
            damage_script: "AutoAttack".to_string(),
        },
        historical_packet: packet,
        ownership_limits: OwnershipLimits {
            player_provider_identity_available_in_historical_projection: false,
            current_build_packet_lifecycle_observed: false,
            exact_owner_selection_rule: "attribute the status to the packet-observed projectile owner; never choose between Blade Sweep and Goblin March from the shared projectile config alone",
            rdps_gate: "preserve the enemy status window now; enable transferred damage only after current-build packet provider ownership and exact armor formula are proven",
        },
    })
}

fn packet_proof(history: &Value) -> Result<HistoricalPacketProof, Box<dyn std::error::Error>> {
    let session_id = required_string(history, "session_id")?;
    let run = history
        .get("runs")
        .and_then(Value::as_array)
        .and_then(|runs| runs.first())
        .ok_or("combat history has no run")?;
    let activity_id = required_string(run, "activity_id")?;
    let view = run
        .get("views")
        .and_then(Value::as_array)
        .and_then(|views| {
            views
                .iter()
                .find(|view| view.get("id").and_then(Value::as_str) == Some("all"))
        })
        .ok_or("combat history has no all view")?;
    let actors = view
        .get("actors")
        .and_then(Value::as_array)
        .ok_or("combat history all view has no actors")?;
    let actor_kinds = actors
        .iter()
        .filter_map(|actor| {
            Some((
                actor.get("actor_id")?.as_str()?.to_string(),
                actor.get("actor_kind")?.as_str()?.to_string(),
            ))
        })
        .collect::<BTreeMap<_, _>>();

    let mut source_actor_ids = BTreeSet::new();
    let mut source_entity_uuids = BTreeSet::new();
    let mut target_actor_ids = BTreeSet::new();
    let mut target_entity_uuids = BTreeSet::new();
    let mut target_actor_kinds = BTreeSet::new();
    let mut applied_count = 0_u64;
    let mut removed_count = 0_u64;

    for actor in actors {
        if actor.get("actor_kind").and_then(Value::as_str) != Some("projectile")
            || integer(actor, "monster_id") != Some(PROJECTILE_CONFIG_ID)
        {
            continue;
        }
        let matching_effects = actor
            .get("effects")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|effect| integer(effect, "effect_id") == Some(TARGET_STATUS_ID))
            .collect::<Vec<_>>();
        if matching_effects.is_empty() {
            continue;
        }
        source_actor_ids.insert(required_string(actor, "actor_id")?);
        source_entity_uuids.insert(required_string(actor, "entity_uuid")?);
        for effect in matching_effects {
            let target_actor_id = required_string(effect, "target_actor_id")?;
            target_actor_ids.insert(target_actor_id.clone());
            target_entity_uuids.insert(required_string(effect, "target_entity_uuid")?);
            if let Some(kind) = actor_kinds.get(&target_actor_id) {
                target_actor_kinds.insert(kind.clone());
            }
            applied_count =
                applied_count.saturating_add(integer(effect, "applied").unwrap_or(0) as u64);
            removed_count =
                removed_count.saturating_add(integer(effect, "removed").unwrap_or(0) as u64);
        }
    }
    if target_actor_kinds.iter().any(|kind| kind == "player") {
        return Err("Blade Sweep target-status proof unexpectedly targets a player".into());
    }

    Ok(HistoricalPacketProof {
        session_id,
        activity_id,
        source_actor_kind: "projectile".to_string(),
        source_actor_ids: source_actor_ids.into_iter().collect(),
        source_entity_uuids: source_entity_uuids.into_iter().collect(),
        source_projectile_config_id: PROJECTILE_CONFIG_ID,
        target_actor_ids: target_actor_ids.into_iter().collect(),
        target_entity_uuids: target_entity_uuids.into_iter().collect(),
        target_actor_kinds: target_actor_kinds.into_iter().collect(),
        applied_count,
        removed_count,
    })
}

fn required_row<'a>(
    table: &'a Value,
    id: i64,
    label: &str,
) -> Result<&'a Value, Box<dyn std::error::Error>> {
    table
        .get(id.to_string())
        .ok_or_else(|| format!("{label} row {id} is missing").into())
}

fn require_skill_description(
    row: &Value,
    damage_id: i64,
    label: &str,
    duration: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let serialized = serde_json::to_string(row.get("SkillAttrDes").unwrap_or(&Value::Null))?;
    if !serialized.contains(&damage_id.to_string())
        || !serialized.contains(label)
        || !serialized.contains(duration)
    {
        return Err(format!(
            "skill effect no longer contains {damage_id}, {label}, and {duration}"
        )
        .into());
    }
    Ok(())
}

fn require_integer(
    row: &Value,
    key: &str,
    expected: i64,
    label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if integer(row, key) != Some(expected) {
        return Err(format!("{label} {key} changed from {expected}").into());
    }
    Ok(())
}

fn require_string(
    row: &Value,
    key: &str,
    expected: &str,
    label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if row.get(key).and_then(Value::as_str) != Some(expected) {
        return Err(format!("{label} {key} changed from {expected}").into());
    }
    Ok(())
}

fn require_number(
    row: &Value,
    key: &str,
    expected: f64,
    label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if row.get(key).and_then(Value::as_f64) != Some(expected) {
        return Err(format!("{label} {key} changed from {expected}").into());
    }
    Ok(())
}

fn require_integer_array(
    row: &Value,
    key: &str,
    expected: &[i64],
    label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let actual = row
        .get(key)
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(Value::as_i64).collect::<Vec<_>>());
    if actual.as_deref() != Some(expected) {
        return Err(format!("{label} {key} changed from {expected:?}").into());
    }
    Ok(())
}

fn require_array_contains_integer(
    row: &Value,
    key: &str,
    expected: i64,
    label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let contains = row
        .get(key)
        .and_then(Value::as_array)
        .is_some_and(|values| values.iter().any(|value| value.as_i64() == Some(expected)));
    if !contains {
        return Err(format!("{label} {key} no longer contains {expected}").into());
    }
    Ok(())
}

fn require_number_pair(
    row: &Value,
    key: &str,
    first: f64,
    second: f64,
    label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if row.get(key) != Some(&json!([[first, second]])) {
        return Err(format!("{label} {key} changed from [[{first}, {second}]]").into());
    }
    Ok(())
}

fn integer(row: &Value, key: &str) -> Option<i64> {
    let value = row.get(key)?;
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}

fn required_string(row: &Value, key: &str) -> Result<String, Box<dyn std::error::Error>> {
    row.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("row is missing string {key}").into())
}

fn read_json(path: impl AsRef<Path>) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_reader(BufReader::new(File::open(path)?))?)
}

fn utf8_argument(
    value: Option<std::ffi::OsString>,
    label: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    value
        .ok_or_else(usage)?
        .into_string()
        .map_err(|_| format!("{label} must be UTF-8").into())
}

fn usage() -> &'static str {
    "usage: rlogs-bpsr-aoyi-projectile-status-proof <decoded-root> <combat-history.json> <output.json> <current-game-build> <historical-packet-build>"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_projection_requires_projectile_identity() {
        let history = json!({
            "session_id": "session",
            "runs": [{
                "activity_id": "scene.12023",
                "views": [{
                    "id": "all",
                    "actors": [
                        {"actor_id":"1","entity_uuid":"10","actor_kind":"monster","monster_id":"7","effects":[]},
                        {"actor_id":"2","entity_uuid":"20","actor_kind":"projectile","monster_id":"10040102","effects":[{"effect_id":"2110092","target_actor_id":"1","target_entity_uuid":"10","applied":1,"removed":1}]}
                    ]
                }]
            }]
        });
        let proof = packet_proof(&history).expect("packet edge should resolve");
        assert_eq!(proof.source_actor_ids, ["2"]);
        assert_eq!(proof.target_actor_kinds, ["monster"]);
        assert_eq!(proof.applied_count, 1);
    }

    #[test]
    fn packet_projection_rejects_player_recipient() {
        let history = json!({
            "session_id": "session",
            "runs": [{
                "activity_id": "scene.12023",
                "views": [{
                    "id": "all",
                    "actors": [
                        {"actor_id":"1","entity_uuid":"10","actor_kind":"player","effects":[]},
                        {"actor_id":"2","entity_uuid":"20","actor_kind":"projectile","monster_id":"10040102","effects":[{"effect_id":"2110092","target_actor_id":"1","target_entity_uuid":"10","applied":1,"removed":1}]}
                    ]
                }]
            }]
        });
        assert!(packet_proof(&history).is_err());
    }
}
