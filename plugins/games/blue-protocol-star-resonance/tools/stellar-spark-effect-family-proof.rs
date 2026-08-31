use std::{
    collections::BTreeMap,
    env,
    error::Error,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::{Value, json};

const TALENT_ID: i64 = 341;
const CONTROLLER_ID: i64 = 2_208_420;
const STACK_ID: i64 = 2_208_421;

#[derive(Clone, Debug, Default, Serialize)]
struct Lifecycle {
    status_events: u64,
    opened_windows: u64,
    closed_windows: u64,
    cross_actor_windows: u64,
    source_missing_windows: u64,
    player_recipient_windows: u64,
    monster_recipient_windows: u64,
    other_recipient_windows: u64,
    unresolved_recipient_windows: u64,
    applied: u64,
    refreshed: u64,
    stacked: u64,
    consumed: u64,
    removed: u64,
    minimum_stacks: u64,
    maximum_stacks: u64,
    observed_active_micros: u64,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("BPSR Stellar Spark effect-family proof failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut arguments = env::args_os().skip(1);
    let decoded_root = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    let provider_audit = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    let output = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    let current_game_build = utf8_argument(arguments.next(), "current game build")?;
    let historical_packet_build = utf8_argument(arguments.next(), "historical packet build")?;
    if arguments.next().is_some() {
        return Err(usage().into());
    }

    let proof = build_proof(
        &decoded_root,
        &provider_audit,
        current_game_build,
        historical_packet_build,
    )?;
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(output)?);
    serde_json::to_writer_pretty(&mut writer, &proof)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn build_proof(
    decoded_root: &Path,
    provider_audit_path: &Path,
    current_game_build: String,
    historical_packet_build: String,
) -> Result<Value, Box<dyn Error>> {
    let talents = read_json(decoded_root.join("TalentTable.json"))?;
    let buffs = read_json(decoded_root.join("BuffTable.json"))?;
    let descriptions = read_json(decoded_root.join("AttrDescription.json"))?;
    let audit = read_json(provider_audit_path)?;
    require_generated_by(&audit, "rlogs-bpsr-rdps-provider-mechanic-audit")?;

    let talent = required_row(&talents, TALENT_ID, "TalentTable")?;
    require_string(talent, "TalentName", "Stellar Spark", "talent")?;
    require_i64(talent, "WeaponGroup", 3, "talent")?;
    require_json(
        talent,
        "TalentEffect",
        &json!([[3, CONTROLLER_ID, 1]]),
        "talent",
    )?;

    let controller = required_row(&buffs, CONTROLLER_ID, "BuffTable")?;
    require_i64(controller, "TipsDescription", CONTROLLER_ID, "controller")?;
    require_json(controller, "RepeatAddRule", &json!([0, 1]), "controller")?;
    require_json(controller, "DestroyParam", &json!([]), "controller")?;

    let stack = required_row(&buffs, STACK_ID, "BuffTable")?;
    require_json(stack, "RepeatAddRule", &json!([2, 10]), "stack")?;
    require_json(stack, "DestroyParam", &json!([]), "stack")?;

    let official = required_row(&descriptions, CONTROLLER_ID, "AttrDescription")?;
    let description = required_string(official, "Description", "description")?;
    for token in ["Expertise Skill", "Fire ATK", ">22<", ">10<"] {
        if !description.contains(token) {
            return Err(format!("Stellar Spark description lost required token {token}").into());
        }
    }

    let lifecycle = aggregate_audit(&audit)?;
    let controller_lifecycle = required_lifecycle(&lifecycle, CONTROLLER_ID)?;
    let stack_lifecycle = required_lifecycle(&lifecycle, STACK_ID)?;
    require_self_only(&controller_lifecycle, "controller")?;
    require_self_only(&stack_lifecycle, "stack")?;
    if stack_lifecycle.maximum_stacks != 10 {
        return Err(format!(
            "historical Stellar Spark maximum changed: expected 10, got {}",
            stack_lifecycle.maximum_stacks
        )
        .into());
    }

    Ok(json!({
        "schema_version": 1,
        "generated_by": "rlogs-bpsr-stellar-spark-effect-family-proof",
        "current_game_build": current_game_build,
        "historical_packet_build": historical_packet_build,
        "proof_state": "current-static-owner-fire-attack-stack-formula-exact-plus-historical-self-lifecycle",
        "current_static": {
            "talent_id": TALENT_ID,
            "talent_name": required_string(talent, "TalentName", "talent")?,
            "weapon_group": required_i64_value(talent, "WeaponGroup", "talent")?,
            "controller": {
                "effect_id": CONTROLLER_ID,
                "repeat_add_rule": [0, 1]
            },
            "stack": {
                "effect_id": STACK_ID,
                "repeat_add_rule": [2, 10],
                "maximum_stacks": 10
            },
            "official_description": description,
            "formula": {
                "qualifying_trigger": "expertise-skill-damage",
                "stat": "fireAttack",
                "formula_term": "elementalAttack",
                "formula_zone": "baseAttackTerm",
                "fire_attack_per_stack": 22,
                "maximum_stacks": 10,
                "maximum_fire_attack": 220,
                "duration_seconds": 10,
                "recipient_scope": "casting-player-only",
                "replay_disposition": "ordinary-self-fire-attack-context-never-transferred-rdps"
            }
        },
        "historical_lifecycle": {
            "controller": controller_lifecycle,
            "stack": stack_lifecycle
        },
        "attribution_policy": {
            "controller_semantics": "owner-only Stellar Spark controller",
            "stack_semantics": "owner-only twenty-two-flat-Fire-ATK stack with exact ten-stack cap",
            "retained_runtime_effect_ids": [CONTROLLER_ID, STACK_ID],
            "transferable_effect_ids": [],
            "current_build_packet_lifecycle_observed": false,
            "formula_replay_allowed_for_transfer": false,
            "build_gate": "historical packet evidence cannot authorize current-build replay; matching-build lifecycle and Fire ATK hit formula remain required"
        }
    }))
}

fn aggregate_audit(audit: &Value) -> Result<BTreeMap<i64, Lifecycle>, Box<dyn Error>> {
    let reports = audit
        .get("reports")
        .and_then(Value::as_array)
        .ok_or("audit reports missing")?;
    let mut result = BTreeMap::new();
    for effect in reports.iter().flat_map(|report| {
        report
            .get("effects")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
    }) {
        let effect_id = required_i64_value(effect, "effect_id", "audit effect")?;
        if ![CONTROLLER_ID, STACK_ID].contains(&effect_id) {
            continue;
        }
        let row = effect.get("lifecycle").ok_or("audit lifecycle missing")?;
        let recipients = effect
            .get("recipient_scope")
            .ok_or("audit recipient scope missing")?;
        let entry = result.entry(effect_id).or_insert_with(Lifecycle::default);
        entry.status_events += u64_field(row, "status_events")?;
        entry.opened_windows += u64_field(row, "opened_windows")?;
        entry.closed_windows += u64_field(row, "closed_windows")?;
        entry.cross_actor_windows += u64_field(row, "cross_actor_windows")?;
        entry.source_missing_windows += u64_field(row, "source_missing_windows")?;
        entry.applied += u64_field(row, "applied")?;
        entry.refreshed += u64_field(row, "refreshed")?;
        entry.stacked += u64_field(row, "stacked")?;
        entry.consumed += u64_field(row, "consumed")?;
        entry.removed += u64_field(row, "removed")?;
        let minimum = u64_field(row, "minimum_stacks")?;
        entry.minimum_stacks = if entry.minimum_stacks == 0 {
            minimum
        } else {
            entry.minimum_stacks.min(minimum)
        };
        entry.maximum_stacks = entry.maximum_stacks.max(u64_field(row, "maximum_stacks")?);
        entry.observed_active_micros += u64_field(row, "observed_active_micros")?;
        entry.player_recipient_windows += u64_field(recipients, "player")?;
        entry.monster_recipient_windows += u64_field(recipients, "monster")?;
        entry.other_recipient_windows += u64_field(recipients, "other")?;
        entry.unresolved_recipient_windows += u64_field(recipients, "unresolved")?;
    }
    Ok(result)
}

fn require_self_only(value: &Lifecycle, label: &str) -> Result<(), Box<dyn Error>> {
    if value.status_events == 0
        || value.opened_windows == 0
        || value.cross_actor_windows != 0
        || value.source_missing_windows != 0
        || value.player_recipient_windows != value.opened_windows
        || value.monster_recipient_windows != 0
        || value.other_recipient_windows != 0
        || value.unresolved_recipient_windows != 0
    {
        return Err(
            format!("historical Stellar Spark {label} is not exact self-only evidence").into(),
        );
    }
    Ok(())
}

fn required_lifecycle(
    lifecycle: &BTreeMap<i64, Lifecycle>,
    effect_id: i64,
) -> Result<Lifecycle, Box<dyn Error>> {
    lifecycle
        .get(&effect_id)
        .cloned()
        .ok_or_else(|| format!("audit effect {effect_id} missing").into())
}

fn read_json(path: impl AsRef<Path>) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_reader(BufReader::new(File::open(path)?))?)
}

fn required_row<'a>(table: &'a Value, id: i64, label: &str) -> Result<&'a Value, Box<dyn Error>> {
    table
        .get(id.to_string())
        .ok_or_else(|| format!("{label} row {id} missing").into())
}

fn required_string(row: &Value, field: &str, label: &str) -> Result<String, Box<dyn Error>> {
    row.get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("{label}.{field} missing").into())
}

fn required_i64_value(row: &Value, field: &str, label: &str) -> Result<i64, Box<dyn Error>> {
    row.get(field)
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("{label}.{field} missing").into())
}

fn require_string(
    row: &Value,
    field: &str,
    expected: &str,
    label: &str,
) -> Result<(), Box<dyn Error>> {
    let actual = required_string(row, field, label)?;
    if actual != expected {
        return Err(
            format!("{label}.{field} changed: expected {expected:?}, got {actual:?}").into(),
        );
    }
    Ok(())
}

fn require_i64(row: &Value, field: &str, expected: i64, label: &str) -> Result<(), Box<dyn Error>> {
    let actual = required_i64_value(row, field, label)?;
    if actual != expected {
        return Err(format!("{label}.{field} changed: expected {expected}, got {actual}").into());
    }
    Ok(())
}

fn require_json(
    row: &Value,
    field: &str,
    expected: &Value,
    label: &str,
) -> Result<(), Box<dyn Error>> {
    let actual = row
        .get(field)
        .ok_or_else(|| format!("{label}.{field} missing"))?;
    if actual != expected {
        return Err(format!("{label}.{field} changed: expected {expected}, got {actual}").into());
    }
    Ok(())
}

fn u64_field(row: &Value, field: &str) -> Result<u64, Box<dyn Error>> {
    row.get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("audit field {field} missing").into())
}

fn require_generated_by(value: &Value, expected: &str) -> Result<(), Box<dyn Error>> {
    if value.get("generated_by").and_then(Value::as_str) != Some(expected) {
        return Err(format!("expected {expected} input").into());
    }
    Ok(())
}

fn utf8_argument(value: Option<std::ffi::OsString>, label: &str) -> Result<String, Box<dyn Error>> {
    value
        .ok_or_else(usage)?
        .into_string()
        .map_err(|_| format!("{label} must be UTF-8").into())
}

fn usage() -> String {
    "usage: rlogs-bpsr-stellar-spark-effect-family-proof <decoded-table-root> <provider-audit.json> <output.json> <current-build> <historical-packet-build>".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn historical_scope_rejects_cross_actor_windows() {
        let value = Lifecycle {
            status_events: 2,
            opened_windows: 1,
            player_recipient_windows: 1,
            cross_actor_windows: 1,
            ..Lifecycle::default()
        };
        assert!(require_self_only(&value, "test").is_err());
    }
}
