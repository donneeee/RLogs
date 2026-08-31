use std::{
    env,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::Value;

const SCHEMA_VERSION: u16 = 1;
const TALENT_ID: i64 = 434;
const PARENT_EFFECT_ID: i64 = 2_205_310;
const OWNER_CHILD_EFFECT_ID: i64 = 2_205_311;
const COUNTDOWN_CHILD_EFFECT_ID: i64 = 2_205_312;

#[derive(Debug, Serialize)]
struct ProofArtifact {
    schema_version: u16,
    generated_by: &'static str,
    current_game_build: String,
    historical_packet_build: String,
    proof_state: &'static str,
    current_static: CurrentStatic,
    historical_origin_edges: Vec<HistoricalOriginEdge>,
    historical_lifecycle: HistoricalLifecycle,
    attribution_policy: AttributionPolicy,
}

#[derive(Debug, Serialize)]
struct CurrentStatic {
    talent_id: i64,
    talent_name: String,
    weapon_group: i64,
    parent_effect_id: i64,
    parent_effect_level: i64,
    child_effects: Vec<ChildStatic>,
    official_description: String,
    owner_critical_damage_basis_points: i64,
    owner_critical_damage_duration_seconds: i64,
    ally_haste_basis_points: i64,
    ally_haste_duration_seconds: i64,
    ally_haste_simultaneous_effect_limit: i64,
    owner_courage_per_second: i64,
    owner_sharp_stacks_per_tick: i64,
    owner_sharp_tick_seconds: i64,
}

#[derive(Debug, Serialize)]
struct ChildStatic {
    effect_id: i64,
    effect_level: i64,
    role: &'static str,
    destroy_param_seconds: Option<f64>,
    destroy_param_is_gameplay_duration: bool,
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
    owner_child: LifecycleAggregate,
    countdown_child: LifecycleAggregate,
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
    observed_active_micros: u64,
}

#[derive(Debug, Serialize)]
struct AttributionPolicy {
    parent_effect_semantics: &'static str,
    owner_child_semantics: &'static str,
    countdown_child_semantics: &'static str,
    external_recipient_semantics: &'static str,
    owner_only_semantics: Vec<&'static str>,
    source_owned_damage_semantics: &'static str,
    runtime_watch_effect_ids: Vec<i64>,
    current_build_packet_lifecycle_observed: bool,
    external_recipient_child_effect_proven: bool,
    formula_replay_allowed: bool,
    required_before_promotion: Vec<&'static str>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("BPSR Battle Cry effect-family proof failed: {error}");
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
    observed_origins_path: &Path,
    provider_audit_path: &Path,
    current_game_build: String,
    historical_packet_build: String,
) -> Result<ProofArtifact, Box<dyn std::error::Error>> {
    let talents = read_json(decoded_root.join("TalentTable.json"))?;
    let buffs = read_json(decoded_root.join("BuffTable.json"))?;
    let origins = read_json(observed_origins_path)?;
    let audit = read_json(provider_audit_path)?;

    require_build(
        &origins,
        &historical_packet_build,
        "observed status origins",
    )?;
    require_generated_by(&audit, "rlogs-bpsr-rdps-provider-mechanic-audit")?;

    let talent = required_row(&talents, TALENT_ID, "TalentTable")?;
    require_string(talent, "TalentName", "Battle Cry", "talent")?;
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
        return Err("Battle Cry no longer points to parent effect 2205310".into());
    }

    let description = talent
        .get("TalentDes")
        .and_then(Value::as_str)
        .ok_or("Battle Cry official talent description is missing")?
        .to_owned();
    for required in [
        "caster's Crit DMG by <style=\"accent-gn\">50%</style> for 10s",
        "All allies gain <style=\"accent-gn\">10%</style> Haste",
        "only one effect can be active at a time",
        "10</style> <style=\"accent-gn\">Courage</style> <style=\"accent-gn\">per</style> second",
        "1</style> <linktext=1152><style=\"ItemQuality_5\">Sharp</style> every <style=\"accent-gn\">2s",
    ] {
        if !description.contains(required) {
            return Err(format!("Battle Cry description lost required token {required}").into());
        }
    }

    let parent = required_row(&buffs, PARENT_EFFECT_ID, "BuffTable")?;
    let owner_child = required_row(&buffs, OWNER_CHILD_EFFECT_ID, "BuffTable")?;
    let countdown_child = required_row(&buffs, COUNTDOWN_CHILD_EFFECT_ID, "BuffTable")?;
    let countdown_destroy_seconds = destroy_param_seconds(countdown_child)?;
    if (countdown_destroy_seconds - 16.0).abs() > f64::EPSILON {
        return Err(format!(
            "Battle Cry countdown child DestroyParam changed from 16 to {countdown_destroy_seconds}"
        )
        .into());
    }

    let origin_edges = [OWNER_CHILD_EFFECT_ID, COUNTDOWN_CHILD_EFFECT_ID]
        .into_iter()
        .map(|child_effect_id| origin_edge(&origins, child_effect_id, PARENT_EFFECT_ID))
        .collect::<Result<Vec<_>, _>>()?;

    let parent_lifecycle = aggregate_effect(&audit, PARENT_EFFECT_ID)?;
    let owner_child_lifecycle = aggregate_effect(&audit, OWNER_CHILD_EFFECT_ID)?;
    let countdown_child_lifecycle = aggregate_effect(&audit, COUNTDOWN_CHILD_EFFECT_ID)?;
    if parent_lifecycle.cross_actor_windows != 0
        || owner_child_lifecycle.cross_actor_windows != 0
        || countdown_child_lifecycle.cross_actor_windows != 0
    {
        return Err(
            "retained historical Battle Cry corpus unexpectedly gained cross-actor windows".into(),
        );
    }
    if countdown_child_lifecycle.opened_windows == 0
        || countdown_child_lifecycle.applied == 0
        || countdown_child_lifecycle.removed == 0
    {
        return Err(
            "Battle Cry countdown child lacks a complete historical packet lifecycle".into(),
        );
    }

    Ok(ProofArtifact {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-battle-cry-effect-family-proof",
        current_game_build,
        historical_packet_build,
        proof_state: "current-static-family-and-mixed-scope-semantics-exact-plus-historical-self-lifecycles-external-recipient-child-unresolved-current-packet-gated",
        current_static: CurrentStatic {
            talent_id: TALENT_ID,
            talent_name: "Battle Cry".to_owned(),
            weapon_group,
            parent_effect_id: PARENT_EFFECT_ID,
            parent_effect_level: required_i64(parent, "Level", "parent buff")?,
            child_effects: vec![
                ChildStatic {
                    effect_id: OWNER_CHILD_EFFECT_ID,
                    effect_level: required_i64(owner_child, "Level", "owner child buff")?,
                    role: "runtime-child-owner-or-controller-branch",
                    destroy_param_seconds: None,
                    destroy_param_is_gameplay_duration: false,
                },
                ChildStatic {
                    effect_id: COUNTDOWN_CHILD_EFFECT_ID,
                    effect_level: required_i64(countdown_child, "Level", "countdown child buff")?,
                    role: "runtime-child-countdown-branch",
                    destroy_param_seconds: Some(countdown_destroy_seconds),
                    destroy_param_is_gameplay_duration: false,
                },
            ],
            official_description: description,
            owner_critical_damage_basis_points: 5_000,
            owner_critical_damage_duration_seconds: 10,
            ally_haste_basis_points: 1_000,
            ally_haste_duration_seconds: 10,
            ally_haste_simultaneous_effect_limit: 1,
            owner_courage_per_second: 10,
            owner_sharp_stacks_per_tick: 1,
            owner_sharp_tick_seconds: 2,
        },
        historical_origin_edges: origin_edges,
        historical_lifecycle: HistoricalLifecycle {
            parent: parent_lifecycle,
            owner_child: owner_child_lifecycle,
            countdown_child: countdown_child_lifecycle,
        },
        attribution_policy: AttributionPolicy {
            parent_effect_semantics: "persistent talent controller observed on the caster; it cannot by itself identify the ally Haste recipient window",
            owner_child_semantics: "runtime child of the controller observed only on the caster in the retained historical corpus; exact owner-side mechanic remains unresolved",
            countdown_child_semantics: "runtime countdown child with explicit historical apply/remove lifecycles and a 16-second table guard; neither proves the official 10-second ally Haste window",
            external_recipient_semantics: "only the official 10 percent ally Haste component is transferable rDPS; its packet child, provider-to-recipient windows, cadence formula placement, and nonstacking winner must be proven",
            owner_only_semantics: vec![
                "50 percent caster critical damage for 10 seconds",
                "10 Courage per second",
                "1 Sharp every 2 seconds",
            ],
            source_owned_damage_semantics: "the follow-up leap is direct source-owned attack damage and is never transferred support credit",
            runtime_watch_effect_ids: vec![
                PARENT_EFFECT_ID,
                OWNER_CHILD_EFFECT_ID,
                COUNTDOWN_CHILD_EFFECT_ID,
            ],
            current_build_packet_lifecycle_observed: false,
            external_recipient_child_effect_proven: false,
            formula_replay_allowed: false,
            required_before_promotion: vec![
                "matching-build Battle Cry cast with all three status IDs and resolved party recipients",
                "exact identification of the child carrying ally Haste versus owner critical damage and resources",
                "matching-build Haste attribute or action-cadence delta during the recipient window",
                "nonstacking provider-winner behavior when multiple Battle Cry sources overlap",
                "baseline and counterfactual timing replay with party conservation",
            ],
        },
    })
}

fn origin_edge(
    origins: &Value,
    child_effect_id: i64,
    parent_effect_id: i64,
) -> Result<HistoricalOriginEdge, Box<dyn std::error::Error>> {
    let origin = origins
        .get("relations")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find(|row| {
                row.get("effect_id").and_then(Value::as_i64) == Some(child_effect_id)
                    && row.get("source_type_id").and_then(Value::as_i64) == Some(1)
                    && row.get("source_config_id").and_then(Value::as_i64)
                        == Some(parent_effect_id)
            })
        })
        .ok_or_else(|| {
            format!(
                "historical packet origins do not contain child {child_effect_id} -> buff {parent_effect_id}"
            )
        })?;
    Ok(HistoricalOriginEdge {
        effect_id: child_effect_id,
        source_type_id: 1,
        source_kind: origin
            .get("source_kind")
            .and_then(Value::as_str)
            .unwrap_or("buff")
            .to_owned(),
        source_config_id: parent_effect_id,
        observation_count: required_u64(origin, "observation_count", "origin edge")?,
        observed_sessions: string_array(origin, "observed_sessions")?,
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
        result.stacked += required_u64(lifecycle, "stacked", "lifecycle")?;
        result.consumed += required_u64(lifecycle, "consumed", "lifecycle")?;
        result.removed += required_u64(lifecycle, "removed", "lifecycle")?;
        result.observed_active_micros +=
            required_u64(lifecycle, "observed_active_micros", "lifecycle")?;
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

fn destroy_param_seconds(value: &Value) -> Result<f64, Box<dyn std::error::Error>> {
    value
        .get("DestroyParam")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .and_then(Value::as_array)
        .and_then(|row| row.get(1))
        .and_then(Value::as_f64)
        .ok_or_else(|| "countdown child buff does not have a numeric DestroyParam".into())
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
    "usage: rlogs-bpsr-battle-cry-effect-family-proof <decoded-table-root> <observed-status-origins.json> <provider-audit.json> <output.json> <current-build> <historical-packet-build>".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn aggregates_all_family_members_without_collapsing_them() {
        let audit = json!({
            "reports": [{"effects": [
                {"effect_id": PARENT_EFFECT_ID, "lifecycle": {"status_events": 1, "opened_windows": 1, "closed_windows": 1, "cross_actor_windows": 0, "source_missing_windows": 0, "applied": 1, "refreshed": 0, "stacked": 0, "consumed": 0, "removed": 0, "observed_active_micros": 30}, "recipient_scope": {"player": 1, "monster": 0, "other": 0, "unresolved": 0}},
                {"effect_id": OWNER_CHILD_EFFECT_ID, "lifecycle": {"status_events": 1, "opened_windows": 1, "closed_windows": 1, "cross_actor_windows": 0, "source_missing_windows": 0, "applied": 1, "refreshed": 0, "stacked": 0, "consumed": 0, "removed": 0, "observed_active_micros": 20}, "recipient_scope": {"player": 1, "monster": 0, "other": 0, "unresolved": 0}},
                {"effect_id": COUNTDOWN_CHILD_EFFECT_ID, "lifecycle": {"status_events": 2, "opened_windows": 1, "closed_windows": 1, "cross_actor_windows": 0, "source_missing_windows": 0, "applied": 1, "refreshed": 0, "stacked": 0, "consumed": 0, "removed": 1, "observed_active_micros": 16}, "recipient_scope": {"player": 1, "monster": 0, "other": 0, "unresolved": 0}}
            ]}]
        });
        assert_eq!(
            aggregate_effect(&audit, PARENT_EFFECT_ID)
                .unwrap()
                .status_events,
            1
        );
        assert_eq!(
            aggregate_effect(&audit, OWNER_CHILD_EFFECT_ID)
                .unwrap()
                .opened_windows,
            1
        );
        let countdown = aggregate_effect(&audit, COUNTDOWN_CHILD_EFFECT_ID).unwrap();
        assert_eq!(countdown.status_events, 2);
        assert_eq!(countdown.removed, 1);
    }
}
