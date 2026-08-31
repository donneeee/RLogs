use std::{
    collections::BTreeMap,
    env,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::Value;

const SCHEMA_VERSION: u16 = 1;
const TALENT_ID: i64 = 1_123;
const CONTROLLER_ID: i64 = 2_203_230;
const STACK_ID: i64 = 2_203_231;
const UNRELATED_FOCUS_ID: i64 = 55_223;

#[derive(Debug, Serialize)]
struct ProofArtifact {
    schema_version: u16,
    generated_by: &'static str,
    current_game_build: String,
    historical_packet_build: String,
    proof_state: &'static str,
    current_static: CurrentStatic,
    historical_lifecycle: HistoricalLifecycle,
    attribution_policy: AttributionPolicy,
}

#[derive(Debug, Serialize)]
struct CurrentStatic {
    talent_id: i64,
    talent_name: String,
    weapon_group: i64,
    controller: BuffIdentity,
    stack: StackIdentity,
    formula: Formula,
    rejected_unrelated_focus: BuffIdentity,
}

#[derive(Debug, Serialize)]
struct BuffIdentity {
    effect_id: i64,
    name: String,
    design_name: String,
    description: String,
    duration_seconds: f64,
}

#[derive(Debug, Serialize)]
struct StackIdentity {
    effect_id: i64,
    name: String,
    design_name: String,
    description: String,
    duration_seconds: f64,
    repeat_add_rule: Vec<i64>,
    maximum_stacks: i64,
}

#[derive(Debug, Serialize)]
struct Formula {
    qualifying_element: &'static str,
    damage_boost_per_stack_basis_points: i64,
    maximum_stacks: i64,
    maximum_damage_boost_basis_points: i64,
    duration_seconds: i64,
    recipient_scope: &'static str,
    replay_disposition: &'static str,
}

#[derive(Debug, Serialize)]
struct HistoricalLifecycle {
    controller: LifecycleAggregate,
    stack: LifecycleAggregate,
    unrelated_focus: LifecycleAggregate,
}

#[derive(Clone, Debug, Default, Serialize)]
struct LifecycleAggregate {
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
    maximum_stacks: u64,
    observed_active_micros: u64,
}

#[derive(Debug, Serialize)]
struct AttributionPolicy {
    controller_semantics: &'static str,
    stack_semantics: &'static str,
    unrelated_focus_semantics: &'static str,
    retained_runtime_effect_ids: Vec<i64>,
    rejected_runtime_effect_ids: Vec<i64>,
    transferable_effect_ids: Vec<i64>,
    current_build_packet_lifecycle_observed: bool,
    formula_replay_allowed_for_transfer: bool,
    build_gate: &'static str,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("BPSR Focused Shot effect-family proof failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
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
) -> Result<ProofArtifact, Box<dyn std::error::Error>> {
    let talents = read_json(decoded_root.join("TalentTable.json"))?;
    let buffs = read_json(decoded_root.join("BuffTable.json"))?;
    let descriptions = read_json(decoded_root.join("AttrDescription.json"))?;
    let audit = read_json(provider_audit_path)?;
    require_generated_by(&audit, "rlogs-bpsr-rdps-provider-mechanic-audit")?;

    let talent = required_row(&talents, TALENT_ID, "TalentTable")?;
    require_string(talent, "TalentName", "Focused Shot", "talent")?;
    require_i64(talent, "WeaponGroup", 11, "talent")?;
    require_json(
        talent,
        "TalentEffect",
        &serde_json::json!([[3, CONTROLLER_ID, 1]]),
        "talent effect",
    )?;

    let controller = required_row(&buffs, CONTROLLER_ID, "BuffTable")?;
    require_string(controller, "NameDesign", "专注射击", "controller")?;
    require_i64(controller, "TipsDescription", CONTROLLER_ID, "controller")?;
    require_json(
        controller,
        "RepeatAddRule",
        &serde_json::json!([0, 1]),
        "controller",
    )?;
    require_json(
        controller,
        "DestroyParam",
        &serde_json::json!([]),
        "controller",
    )?;

    let official = required_row(&descriptions, CONTROLLER_ID, "AttrDescription")?;
    let official_description = required_string(official, "Description", "controller description")?;
    for token in [
        "1%</style> Light DMG bonus",
        "lasts <style=\"accent-gn\">3</style>s",
        "<style=\"accent-gn\">4</style> times",
    ] {
        if !official_description.contains(token) {
            return Err(format!("Focused Shot description lost required token {token}").into());
        }
    }

    let stack = required_row(&buffs, STACK_ID, "BuffTable")?;
    require_string(stack, "Name", "Focused Shot", "stack")?;
    require_string(stack, "NameDesign", "专注射击_子BUFF", "stack")?;
    require_string(
        stack,
        "Desc",
        "Grants 1% Light Bonus, stacking up to 4 times.",
        "stack",
    )?;
    require_json(stack, "RepeatAddRule", &serde_json::json!([2, 4]), "stack")?;
    require_json(
        stack,
        "DestroyParam",
        &serde_json::json!([[0.0, 3.0]]),
        "stack",
    )?;

    let unrelated = required_row(&buffs, UNRELATED_FOCUS_ID, "BuffTable")?;
    require_string(unrelated, "Name", "Focus", "unrelated Focus")?;
    require_string(
        unrelated,
        "Desc",
        "Haste greatly increased, special attack CD -50%",
        "unrelated Focus",
    )?;
    require_json(
        unrelated,
        "DestroyParam",
        &serde_json::json!([[0.0, 12.0]]),
        "unrelated Focus",
    )?;

    let lifecycle = aggregate_audit(&audit)?;
    let controller_lifecycle = required_lifecycle(&lifecycle, CONTROLLER_ID)?;
    let stack_lifecycle = required_lifecycle(&lifecycle, STACK_ID)?;
    let unrelated_lifecycle = required_lifecycle(&lifecycle, UNRELATED_FOCUS_ID)?;
    require_self_only_historical(&controller_lifecycle, "controller")?;
    require_self_only_historical(&stack_lifecycle, "stack")?;
    require_self_only_historical(&unrelated_lifecycle, "unrelated Focus")?;
    if stack_lifecycle.maximum_stacks != 4 {
        return Err(format!(
            "historical Focused Shot stack maximum changed: expected 4, got {}",
            stack_lifecycle.maximum_stacks
        )
        .into());
    }

    Ok(ProofArtifact {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-focused-shot-effect-family-proof",
        current_game_build,
        historical_packet_build,
        proof_state: "current-static-owner-light-stack-formula-exact-plus-historical-self-lifecycle-unrelated-focus-separated",
        current_static: CurrentStatic {
            talent_id: TALENT_ID,
            talent_name: required_string(talent, "TalentName", "talent")?,
            weapon_group: required_i64_value(talent, "WeaponGroup", "talent")?,
            controller: buff_identity(controller, CONTROLLER_ID, official_description, 0.0)?,
            stack: StackIdentity {
                effect_id: STACK_ID,
                name: required_string(stack, "Name", "stack")?,
                design_name: required_string(stack, "NameDesign", "stack")?,
                description: required_string(stack, "Desc", "stack")?,
                duration_seconds: destroy_duration(stack)?,
                repeat_add_rule: i64_array(stack, "RepeatAddRule", "stack")?,
                maximum_stacks: 4,
            },
            formula: Formula {
                qualifying_element: "light",
                damage_boost_per_stack_basis_points: 100,
                maximum_stacks: 4,
                maximum_damage_boost_basis_points: 400,
                duration_seconds: 3,
                recipient_scope: "casting-player-only",
                replay_disposition: "ordinary-self-elemental-damage-context-never-transferred-rdps",
            },
            rejected_unrelated_focus: buff_identity(
                unrelated,
                UNRELATED_FOCUS_ID,
                required_string(unrelated, "Desc", "unrelated Focus")?,
                destroy_duration(unrelated)?,
            )?,
        },
        historical_lifecycle: HistoricalLifecycle {
            controller: controller_lifecycle,
            stack: stack_lifecycle,
            unrelated_focus: unrelated_lifecycle,
        },
        attribution_policy: AttributionPolicy {
            controller_semantics: "owner-only Focused Shot controller",
            stack_semantics: "owner-only one-percent Light damage stack with exact four-stack cap",
            unrelated_focus_semantics: "separate owner Focus Haste/cooldown mechanic retained as independent evidence",
            retained_runtime_effect_ids: vec![CONTROLLER_ID, STACK_ID],
            rejected_runtime_effect_ids: vec![UNRELATED_FOCUS_ID],
            transferable_effect_ids: vec![],
            current_build_packet_lifecycle_observed: false,
            formula_replay_allowed_for_transfer: false,
            build_gate: "historical packet evidence cannot authorize current-build replay; matching-build lifecycle and Light damage mapping remain required",
        },
    })
}

fn aggregate_audit(
    audit: &Value,
) -> Result<BTreeMap<i64, LifecycleAggregate>, Box<dyn std::error::Error>> {
    let reports = audit
        .get("reports")
        .and_then(Value::as_array)
        .ok_or("audit reports missing")?;
    let mut result = BTreeMap::new();
    for report in reports {
        for effect in report
            .get("effects")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let effect_id = required_i64_value(effect, "effect_id", "audit effect")?;
            if ![CONTROLLER_ID, STACK_ID, UNRELATED_FOCUS_ID].contains(&effect_id) {
                continue;
            }
            let lifecycle = effect.get("lifecycle").ok_or("audit lifecycle missing")?;
            let recipients = effect
                .get("recipient_scope")
                .ok_or("audit recipient scope missing")?;
            let entry = result
                .entry(effect_id)
                .or_insert_with(LifecycleAggregate::default);
            entry.status_events += u64_field(lifecycle, "status_events")?;
            entry.opened_windows += u64_field(lifecycle, "opened_windows")?;
            entry.closed_windows += u64_field(lifecycle, "closed_windows")?;
            entry.cross_actor_windows += u64_field(lifecycle, "cross_actor_windows")?;
            entry.source_missing_windows += u64_field(lifecycle, "source_missing_windows")?;
            entry.applied += u64_field(lifecycle, "applied")?;
            entry.refreshed += u64_field(lifecycle, "refreshed")?;
            entry.stacked += u64_field(lifecycle, "stacked")?;
            entry.consumed += u64_field(lifecycle, "consumed")?;
            entry.removed += u64_field(lifecycle, "removed")?;
            entry.maximum_stacks = entry
                .maximum_stacks
                .max(u64_field(lifecycle, "maximum_stacks")?);
            entry.observed_active_micros += u64_field(lifecycle, "observed_active_micros")?;
            entry.player_recipient_windows += u64_field(recipients, "player")?;
            entry.monster_recipient_windows += u64_field(recipients, "monster")?;
            entry.other_recipient_windows += u64_field(recipients, "other")?;
            entry.unresolved_recipient_windows += u64_field(recipients, "unresolved")?;
        }
    }
    Ok(result)
}

fn required_lifecycle(
    lifecycle: &BTreeMap<i64, LifecycleAggregate>,
    effect_id: i64,
) -> Result<LifecycleAggregate, Box<dyn std::error::Error>> {
    lifecycle
        .get(&effect_id)
        .cloned()
        .ok_or_else(|| format!("audit effect {effect_id} missing").into())
}

fn require_self_only_historical(
    value: &LifecycleAggregate,
    label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if value.status_events == 0 || value.opened_windows == 0 {
        return Err(format!("historical {label} lifecycle is empty").into());
    }
    if value.cross_actor_windows != 0
        || value.source_missing_windows != 0
        || value.monster_recipient_windows != 0
        || value.other_recipient_windows != 0
        || value.unresolved_recipient_windows != 0
        || value.player_recipient_windows != value.opened_windows
    {
        return Err(
            format!("historical {label} lifecycle is not exact player self-only evidence").into(),
        );
    }
    Ok(())
}

fn buff_identity(
    row: &Value,
    effect_id: i64,
    description: String,
    duration_seconds: f64,
) -> Result<BuffIdentity, Box<dyn std::error::Error>> {
    Ok(BuffIdentity {
        effect_id,
        name: required_string(row, "Name", "buff")?,
        design_name: required_string(row, "NameDesign", "buff")?,
        description,
        duration_seconds,
    })
}

fn destroy_duration(row: &Value) -> Result<f64, Box<dyn std::error::Error>> {
    row.get("DestroyParam")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .and_then(Value::as_array)
        .and_then(|values| values.get(1))
        .and_then(Value::as_f64)
        .ok_or_else(|| "buff duration missing".into())
}

fn required_row<'a>(
    table: &'a Value,
    id: i64,
    label: &str,
) -> Result<&'a Value, Box<dyn std::error::Error>> {
    table
        .get(id.to_string())
        .ok_or_else(|| format!("{label} row {id} missing").into())
}

fn required_string(
    row: &Value,
    field: &str,
    label: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    row.get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("{label}.{field} missing").into())
}

fn require_string(
    row: &Value,
    field: &str,
    expected: &str,
    label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let actual = required_string(row, field, label)?;
    if actual != expected {
        return Err(
            format!("{label}.{field} changed: expected {expected:?}, got {actual:?}").into(),
        );
    }
    Ok(())
}

fn required_i64_value(
    row: &Value,
    field: &str,
    label: &str,
) -> Result<i64, Box<dyn std::error::Error>> {
    row.get(field)
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("{label}.{field} missing").into())
}

fn require_i64(
    row: &Value,
    field: &str,
    expected: i64,
    label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
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
) -> Result<(), Box<dyn std::error::Error>> {
    let actual = row
        .get(field)
        .ok_or_else(|| format!("{label}.{field} missing"))?;
    if actual != expected {
        return Err(format!("{label}.{field} changed: expected {expected}, got {actual}").into());
    }
    Ok(())
}

fn i64_array(
    row: &Value,
    field: &str,
    label: &str,
) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
    row.get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{label}.{field} missing"))?
        .iter()
        .map(|value| {
            value
                .as_i64()
                .ok_or_else(|| format!("{label}.{field} contains non-integer").into())
        })
        .collect()
}

fn u64_field(row: &Value, field: &str) -> Result<u64, Box<dyn std::error::Error>> {
    row.get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("audit field {field} missing").into())
}

fn require_generated_by(value: &Value, expected: &str) -> Result<(), Box<dyn std::error::Error>> {
    match value.get("generated_by").and_then(Value::as_str) {
        Some(actual) if actual == expected => Ok(()),
        actual => Err(format!("generated_by mismatch: expected {expected}, got {actual:?}").into()),
    }
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

fn usage() -> String {
    "usage: rlogs-bpsr-focused-shot-effect-family-proof <decoded-table-root> <provider-audit.json> <output.json> <current-build> <historical-packet-build>".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn historical_scope_rejects_cross_actor_windows() {
        let value = LifecycleAggregate {
            status_events: 2,
            opened_windows: 1,
            player_recipient_windows: 1,
            cross_actor_windows: 1,
            ..LifecycleAggregate::default()
        };
        assert!(require_self_only_historical(&value, "test").is_err());
    }

    #[test]
    fn historical_scope_accepts_exact_self_window() {
        let value = LifecycleAggregate {
            status_events: 2,
            opened_windows: 1,
            player_recipient_windows: 1,
            maximum_stacks: 4,
            ..LifecycleAggregate::default()
        };
        assert!(require_self_only_historical(&value, "test").is_ok());
    }
}
