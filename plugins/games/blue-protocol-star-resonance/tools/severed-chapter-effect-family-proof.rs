use std::{
    env,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::Value;

const SCHEMA_VERSION: u16 = 1;
const TALENT_ID: i64 = 1_311;
const PARENT_EFFECT_ID: i64 = 2_207_120;
const CHILD_EFFECT_ID: i64 = 2_207_121;

#[derive(Debug, Serialize)]
struct ProofArtifact {
    schema_version: u16,
    generated_by: &'static str,
    current_game_build: String,
    historical_packet_build: String,
    proof_state: &'static str,
    current_static: CurrentStatic,
    historical_origin_edge: HistoricalOriginEdge,
    historical_lifecycle: HistoricalLifecycle,
    attribution_policy: AttributionPolicy,
}

#[derive(Debug, Serialize)]
struct CurrentStatic {
    talent_id: i64,
    talent_name: String,
    weapon_group: i64,
    parent_effect_id: i64,
    child_effect_id: i64,
    parent_effect_level: i64,
    child_effect_level: i64,
    child_duration_seconds: f64,
    official_description: String,
    base_resilience_break_efficiency_basis_points: i64,
    heroic_resilience_break_efficiency_basis_points: i64,
    heroic_broken_target_damage_basis_points: i64,
}

#[derive(Debug, Serialize)]
struct HistoricalOriginEdge {
    effect_id: i64,
    source_type_id: i64,
    source_kind: String,
    source_config_id: i64,
    observation_count: u64,
    observed_sessions: Vec<String>,
}

#[derive(Debug, Serialize)]
struct HistoricalLifecycle {
    parent: LifecycleAggregate,
    child: LifecycleAggregate,
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
    consumed: u64,
    removed: u64,
}

#[derive(Debug, Serialize)]
struct AttributionPolicy {
    parent_effect_semantics: &'static str,
    child_effect_semantics: &'static str,
    runtime_watch_effect_ids: Vec<i64>,
    current_build_packet_lifecycle_observed: bool,
    formula_replay_allowed: bool,
    required_before_promotion: Vec<&'static str>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("BPSR Severed Chapter effect-family proof failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = env::args_os().skip(1);
    let decoded_root = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    let observed_origins = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    let provider_audit = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    let output = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    let current_game_build = utf8_argument(arguments.next(), "current game build")?;
    let historical_packet_build = utf8_argument(arguments.next(), "historical packet build")?;
    if arguments.next().is_some() {
        return Err(usage().into());
    }

    let proof = build_proof(
        &decoded_root,
        &observed_origins,
        &provider_audit,
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
    observed_origins_path: &Path,
    provider_audit_path: &Path,
    current_game_build: String,
    historical_packet_build: String,
) -> Result<ProofArtifact, Box<dyn std::error::Error>> {
    let talents = read_json(decoded_root.join("TalentTable.json"))?;
    let buffs = read_json(decoded_root.join("BuffTable.json"))?;
    let descriptions = read_json(decoded_root.join("AttrDescription.json"))?;
    let origins = read_json(observed_origins_path)?;
    let audit = read_json(provider_audit_path)?;

    require_build(
        &origins,
        &historical_packet_build,
        "observed status origins",
    )?;
    require_generated_by(&audit, "rlogs-bpsr-rdps-provider-mechanic-audit")?;

    let talent = required_row(&talents, TALENT_ID, "TalentTable")?;
    require_string(talent, "TalentName", "Severed Chapter", "talent")?;
    let weapon_group = required_i64(talent, "WeaponGroup", "talent")?;
    if !talent
        .get("TalentEffect")
        .and_then(Value::as_array)
        .is_some_and(|rows| {
            rows.iter().any(|row| {
                row.as_array().is_some_and(|items| {
                    items.get(1).and_then(Value::as_i64) == Some(PARENT_EFFECT_ID)
                })
            })
        })
    {
        return Err("Severed Chapter no longer points to parent effect 2207120".into());
    }

    let parent = required_row(&buffs, PARENT_EFFECT_ID, "BuffTable")?;
    let child = required_row(&buffs, CHILD_EFFECT_ID, "BuffTable")?;
    let parent_level = required_i64(parent, "Level", "parent buff")?;
    let child_level = required_i64(child, "Level", "child buff")?;
    let child_duration_seconds = child
        .get("DestroyParam")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .and_then(Value::as_array)
        .and_then(|row| row.get(1))
        .and_then(Value::as_f64)
        .ok_or("child buff does not have a numeric DestroyParam duration")?;
    if (child_duration_seconds - 2.0).abs() > f64::EPSILON {
        return Err(format!(
            "Severed Chapter child duration changed from 2 seconds to {child_duration_seconds}"
        )
        .into());
    }

    let description = required_row(&descriptions, PARENT_EFFECT_ID, "AttrDescription")?
        .get("Description")
        .and_then(Value::as_str)
        .ok_or("Severed Chapter official description is missing")?
        .to_owned();
    for required in [
        "10%",
        "+<style=\"accent-gn\">30%</style>",
        "15%</style> more DMG",
    ] {
        if !description.contains(required) {
            return Err(
                format!("Severed Chapter description lost required token {required}").into(),
            );
        }
    }

    let origin = origins
        .get("relations")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find(|row| {
                row.get("effect_id").and_then(Value::as_i64) == Some(CHILD_EFFECT_ID)
                    && row.get("source_type_id").and_then(Value::as_i64) == Some(1)
                    && row.get("source_config_id").and_then(Value::as_i64) == Some(PARENT_EFFECT_ID)
            })
        })
        .ok_or("historical packet origins do not contain child 2207121 -> buff 2207120")?;
    let origin_edge = HistoricalOriginEdge {
        effect_id: CHILD_EFFECT_ID,
        source_type_id: 1,
        source_kind: origin
            .get("source_kind")
            .and_then(Value::as_str)
            .unwrap_or("buff")
            .to_owned(),
        source_config_id: PARENT_EFFECT_ID,
        observation_count: required_u64(origin, "observation_count", "origin edge")?,
        observed_sessions: string_array(origin, "observed_sessions")?,
    };

    let parent_lifecycle = aggregate_effect(&audit, PARENT_EFFECT_ID)?;
    let child_lifecycle = aggregate_effect(&audit, CHILD_EFFECT_ID)?;
    if child_lifecycle.cross_actor_windows == 0 {
        return Err("historical child effect has no cross-actor windows".into());
    }
    if parent_lifecycle.cross_actor_windows != 0 {
        return Err("historical parent effect unexpectedly has cross-actor windows".into());
    }

    Ok(ProofArtifact {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-severed-chapter-effect-family-proof",
        current_game_build,
        historical_packet_build,
        proof_state: "current-static-family-exact-plus-historical-child-origin-and-cross-actor-lifecycle-current-packet-gated",
        current_static: CurrentStatic {
            talent_id: TALENT_ID,
            talent_name: "Severed Chapter".to_owned(),
            weapon_group,
            parent_effect_id: PARENT_EFFECT_ID,
            child_effect_id: CHILD_EFFECT_ID,
            parent_effect_level: parent_level,
            child_effect_level: child_level,
            child_duration_seconds,
            official_description: description,
            base_resilience_break_efficiency_basis_points: 1_000,
            heroic_resilience_break_efficiency_basis_points: 3_000,
            heroic_broken_target_damage_basis_points: 1_500,
        },
        historical_origin_edge: origin_edge,
        historical_lifecycle: HistoricalLifecycle {
            parent: parent_lifecycle,
            child: child_lifecycle,
        },
        attribution_policy: AttributionPolicy {
            parent_effect_semantics: "persistent owner talent state; never use this status alone as the 15 percent transferred-damage window",
            child_effect_semantics: "runtime child of the parent buff with historical cross-actor recipients; retain as the packet watch surface for the Heroic Melody branch",
            runtime_watch_effect_ids: vec![PARENT_EFFECT_ID, CHILD_EFFECT_ID],
            current_build_packet_lifecycle_observed: false,
            formula_replay_allowed: false,
            required_before_promotion: vec![
                "matching-build child effect lifecycle with resolved provider and recipient identities",
                "proof of the resilience-broken target condition on each affected damage event",
                "server damage formula placement for the 15 percent multiplier",
                "baseline and counterfactual replay with party conservation",
            ],
        },
    })
}

fn aggregate_effect(
    audit: &Value,
    effect_id: i64,
) -> Result<LifecycleAggregate, Box<dyn std::error::Error>> {
    let mut result = LifecycleAggregate::default();
    for report in audit
        .get("reports")
        .and_then(Value::as_array)
        .ok_or("provider audit reports are missing")?
    {
        let Some(effect) = report
            .get("effects")
            .and_then(Value::as_array)
            .and_then(|rows| {
                rows.iter()
                    .find(|row| row.get("effect_id").and_then(Value::as_i64) == Some(effect_id))
            })
        else {
            continue;
        };
        let lifecycle = effect
            .get("lifecycle")
            .ok_or("effect lifecycle is missing")?;
        let recipients = effect
            .get("recipient_scope")
            .ok_or("effect recipient scope is missing")?;
        result.status_events += required_u64(lifecycle, "status_events", "lifecycle")?;
        result.opened_windows += required_u64(lifecycle, "opened_windows", "lifecycle")?;
        result.closed_windows += required_u64(lifecycle, "closed_windows", "lifecycle")?;
        result.cross_actor_windows += required_u64(lifecycle, "cross_actor_windows", "lifecycle")?;
        result.source_missing_windows +=
            required_u64(lifecycle, "source_missing_windows", "lifecycle")?;
        result.applied += required_u64(lifecycle, "applied", "lifecycle")?;
        result.refreshed += required_u64(lifecycle, "refreshed", "lifecycle")?;
        result.consumed += required_u64(lifecycle, "consumed", "lifecycle")?;
        result.removed += required_u64(lifecycle, "removed", "lifecycle")?;
        result.player_recipient_windows += required_u64(recipients, "player", "recipient")?;
        result.monster_recipient_windows += required_u64(recipients, "monster", "recipient")?;
        result.other_recipient_windows += required_u64(recipients, "other", "recipient")?;
        result.unresolved_recipient_windows += required_u64(recipients, "unresolved", "recipient")?;
    }
    if result.status_events == 0 {
        return Err(format!("provider audit has no status events for effect {effect_id}").into());
    }
    Ok(result)
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

fn require_build(
    value: &Value,
    expected: &str,
    label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let actual = value
        .get("game_build")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{label} game_build is missing"))?;
    if actual != expected {
        return Err(format!("{label} build {actual} does not match {expected}").into());
    }
    Ok(())
}

fn require_generated_by(value: &Value, expected: &str) -> Result<(), Box<dyn std::error::Error>> {
    let actual = value
        .get("generated_by")
        .and_then(Value::as_str)
        .ok_or("provider audit generated_by is missing")?;
    if actual != expected {
        return Err(
            format!("provider audit was generated by {actual}, expected {expected}").into(),
        );
    }
    Ok(())
}

fn require_string(
    value: &Value,
    key: &str,
    expected: &str,
    label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let actual = value.get(key).and_then(Value::as_str).unwrap_or_default();
    if actual != expected {
        return Err(format!("{label} {key} is {actual:?}, expected {expected:?}").into());
    }
    Ok(())
}

fn required_i64(value: &Value, key: &str, label: &str) -> Result<i64, Box<dyn std::error::Error>> {
    value
        .get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("{label} {key} is missing or non-integer").into())
}

fn required_u64(value: &Value, key: &str, label: &str) -> Result<u64, Box<dyn std::error::Error>> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("{label} {key} is missing or non-integer").into())
}

fn string_array(value: &Value, key: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    value
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{key} is missing or non-array"))?
        .iter()
        .map(|item| {
            item.as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| format!("{key} contains a non-string").into())
        })
        .collect()
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
        .map_err(|_| format!("{label} is not valid UTF-8").into())
}

fn usage() -> String {
    "usage: rlogs-bpsr-severed-chapter-effect-family-proof <decoded-table-root> <observed-status-origins.json> <provider-audit.json> <output.json> <current-build> <historical-packet-build>".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn aggregates_parent_and_child_lifecycles_without_collapsing_them() {
        let audit = json!({
            "reports": [{"effects": [
                {"effect_id": PARENT_EFFECT_ID, "lifecycle": {"status_events": 2, "opened_windows": 1, "closed_windows": 1, "cross_actor_windows": 0, "source_missing_windows": 0, "applied": 1, "refreshed": 0, "consumed": 0, "removed": 1}, "recipient_scope": {"player": 1, "monster": 0, "other": 0, "unresolved": 0}},
                {"effect_id": CHILD_EFFECT_ID, "lifecycle": {"status_events": 8, "opened_windows": 3, "closed_windows": 3, "cross_actor_windows": 2, "source_missing_windows": 0, "applied": 3, "refreshed": 0, "consumed": 2, "removed": 3}, "recipient_scope": {"player": 3, "monster": 0, "other": 0, "unresolved": 0}}
            ]}]
        });
        let parent = aggregate_effect(&audit, PARENT_EFFECT_ID).unwrap();
        let child = aggregate_effect(&audit, CHILD_EFFECT_ID).unwrap();
        assert_eq!(parent.cross_actor_windows, 0);
        assert_eq!(child.cross_actor_windows, 2);
        assert_eq!(child.status_events, 8);
    }
}
