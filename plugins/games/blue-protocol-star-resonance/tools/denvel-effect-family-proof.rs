use std::{
    env,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::Value;

const SCHEMA_VERSION: u16 = 1;
const SKILL_ID: i64 = 3_968;
const SKILL_EFFECT_ID: i64 = 396_801;
const ITEM_ID: i64 = 3_000_120;
const MONSTER_ID: i64 = 1_701;
const OWNER_BUFF_ID: i64 = 2_110_137;
const GRAVITY_COUNTER_ID: i64 = 2_110_152;
const PASSIVE_BUFF_ID: i64 = 3_200_036;
const SUMMON_DAMAGE_ID: i64 = 11_701_120_104;
const SELF_DAMAGE_ID: i64 = 2_211_013_704;
const SUMMON_RECOUNT_ID: i64 = 307;

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
    skill_id: i64,
    skill_effect_id: i64,
    item_id: i64,
    monster_id: i64,
    skill_name: String,
    official_description: String,
    active_owner_buff: BuffStatic,
    gravity_counter: BuffStatic,
    active_formula: ActiveFormula,
    passive_owner_effect: PassiveOwnerEffect,
    direct_damage: DirectDamage,
    unresolved_transform_tuples: Vec<TransformTuple>,
}

#[derive(Debug, Serialize)]
struct BuffStatic {
    effect_id: i64,
    name: String,
    design_name: String,
    duration_seconds: f64,
    role: &'static str,
}

#[derive(Debug, Serialize)]
struct ActiveFormula {
    base_damage_boost_basis_points: i64,
    extra_damage_boost_per_hp_step_basis_points: i64,
    hp_step: i64,
    total_damage_boost_cap_basis_points: i64,
    self_damage_per_tick_basis_points_of_max_hp: i64,
    self_damage_tick_seconds: f64,
    duration_seconds: i64,
    recipient_scope: &'static str,
    replay_disposition: &'static str,
}

#[derive(Debug, Serialize)]
struct PassiveOwnerEffect {
    effect_id: i64,
    description: String,
    base_damage_basis_points: i64,
    base_final_damage_reduction_basis_points: i64,
    tiers: Vec<PassiveTier>,
    recipient_scope: &'static str,
    rdps_disposition: &'static str,
}

#[derive(Debug, Serialize)]
struct PassiveTier {
    tier: i64,
    row_id: i64,
    damage_basis_points: i64,
    final_damage_reduction_basis_points: i64,
    active_formula_delta: FormulaDelta,
}

#[derive(Debug, Serialize)]
struct FormulaDelta {
    base_damage_boost_basis_points: i64,
    extra_damage_boost_per_hp_step_basis_points: i64,
    hp_step_delta: i64,
    total_damage_boost_cap_basis_points: i64,
}

#[derive(Debug, Serialize)]
struct DirectDamage {
    summon_damage_id: i64,
    summon_damage_script: String,
    summon_damage_type: i64,
    recount_parent_id: i64,
    recount_parent_name: String,
    self_damage_id: i64,
    self_damage_script: String,
    self_damage_type: i64,
    self_damage_type_enum: i64,
    rdps_disposition: &'static str,
}

#[derive(Debug, Serialize)]
struct TransformTuple {
    tier: i64,
    kind: i64,
    operand: i64,
    value: i64,
    proof_state: &'static str,
}

#[derive(Debug, Serialize)]
struct HistoricalLifecycle {
    owner_buff: LifecycleAggregate,
    gravity_counter: LifecycleAggregate,
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
    self_sourced_examples: u64,
    non_self_sourced_examples: u64,
    applied: u64,
    refreshed: u64,
    stacked: u64,
    consumed: u64,
    removed: u64,
    observed_active_micros: u64,
}

#[derive(Debug, Serialize)]
struct AttributionPolicy {
    owner_buff_semantics: &'static str,
    gravity_counter_semantics: &'static str,
    passive_semantics: &'static str,
    summon_damage_semantics: &'static str,
    self_damage_semantics: &'static str,
    retained_runtime_effect_ids: Vec<i64>,
    transferable_effect_ids: Vec<i64>,
    current_build_packet_lifecycle_observed: bool,
    formula_replay_allowed_for_transfer: bool,
    rejected_conflation: &'static str,
    build_gate: &'static str,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("BPSR Denvel effect-family proof failed: {error}");
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
    let skills = read_json(decoded_root.join("SkillTable.json"))?;
    let effects = read_json(decoded_root.join("SkillEffectTable.json"))?;
    let levels = read_json(decoded_root.join("SkillFightLevelTable.json"))?;
    let aoyi = read_json(decoded_root.join("SkillAoyiTable.json"))?;
    let stars = read_json(decoded_root.join("SkillAoyiStarTable.json"))?;
    let buffs = read_json(decoded_root.join("BuffTable.json"))?;
    let attr_descriptions = read_json(decoded_root.join("AttrDescription.json"))?;
    let damage = read_json(decoded_root.join("DamageAttrTable.json"))?;
    let recount = read_json(decoded_root.join("RecountTable.json"))?;
    let audit = read_json(provider_audit_path)?;
    require_generated_by(&audit, "rlogs-bpsr-rdps-provider-mechanic-audit")?;

    let skill = required_row(&skills, SKILL_ID, "SkillTable")?;
    require_string(skill, "Name", "Arcane! Void Corruption Blade", "skill")?;
    require_i64_array_contains(skill, "EffectIDs", SKILL_EFFECT_ID, "skill")?;
    let description = required_string(skill, "Desc", "skill")?;
    for token in [
        "0.1%</style> of Max HP every 0.2s",
        "additional boost for every <style=\"accent-gn\">100,000</style> Max HP",
        "Total base and extra DMG boost is capped",
    ] {
        if !description.contains(token) {
            return Err(format!("Denvel description lost required token {token}").into());
        }
    }

    let effect = required_row(&effects, SKILL_EFFECT_ID, "SkillEffectTable")?;
    if required_i64(effect, "SkillId", "skill effect")? != SKILL_ID {
        return Err("Denvel skill effect owner changed".into());
    }
    let aoyi_row = required_row(&aoyi, SKILL_ID, "SkillAoyiTable")?;
    if required_i64(aoyi_row, "AoyiItemId", "aoyi")? != ITEM_ID
        || required_i64(aoyi_row, "MonsterId", "aoyi")? != MONSTER_ID
    {
        return Err("Denvel item or monster identity changed".into());
    }
    require_json(
        aoyi_row,
        "TransformationType",
        &serde_json::json!([[3, PASSIVE_BUFF_ID, 1]]),
        "aoyi passive transformation",
    )?;
    require_json(
        aoyi_row,
        "BuffPar",
        &serde_json::json!([[400, 400]]),
        "aoyi passive parameters",
    )?;

    let level = required_row(&levels, SKILL_EFFECT_ID, "SkillFightLevelTable")?;
    let active = FormulaDelta {
        base_damage_boost_basis_points: float_parameter(level, "attr")?,
        extra_damage_boost_per_hp_step_basis_points: float_parameter(level, "attrElse")?,
        hp_step_delta: float_parameter(level, "hp")?,
        total_damage_boost_cap_basis_points: float_parameter(level, "attrMax")?,
    };
    if (
        active.base_damage_boost_basis_points,
        active.extra_damage_boost_per_hp_step_basis_points,
        active.hp_step_delta,
        active.total_damage_boost_cap_basis_points,
    ) != (500, 300, 100_000, 2_000)
    {
        return Err("Denvel active formula parameters changed".into());
    }

    let owner_buff = required_row(&buffs, OWNER_BUFF_ID, "BuffTable")?;
    let gravity_counter = required_row(&buffs, GRAVITY_COUNTER_ID, "BuffTable")?;
    let passive_buff = required_row(&buffs, PASSIVE_BUFF_ID, "BuffTable")?;
    require_string(owner_buff, "Name", "Void Corruption Power", "owner buff")?;
    require_string(
        owner_buff,
        "Icon",
        "ui/atlas/buff/buff_icon136",
        "owner buff",
    )?;
    let owner_duration = destroy_param_seconds(owner_buff)?;
    let gravity_duration = destroy_param_seconds(gravity_counter)?;
    if (owner_duration - 20.0).abs() > f64::EPSILON || (gravity_duration - 4.2).abs() > f64::EPSILON
    {
        return Err("Denvel owner or gravity duration changed".into());
    }
    if required_i64(passive_buff, "TipsDescription", "passive buff")? != PASSIVE_BUFF_ID {
        return Err("Denvel passive description link changed".into());
    }
    let passive_description = required_string(
        required_row(&attr_descriptions, PASSIVE_BUFF_ID, "AttrDescription")?,
        "Description",
        "passive description",
    )?;
    if !passive_description.contains("DMG ") || !passive_description.contains("Final DMG Reduction")
    {
        return Err("Denvel passive description lanes changed".into());
    }

    let (tiers, unresolved_transform_tuples) = denvel_tiers(&stars)?;
    let summon_damage = required_row(&damage, SUMMON_DAMAGE_ID, "DamageAttrTable")?;
    let self_damage = required_row(&damage, SELF_DAMAGE_ID, "DamageAttrTable")?;
    require_string(summon_damage, "DamageScript", "AutoAttack", "summon damage")?;
    require_string(
        self_damage,
        "DamageScript",
        "SpAttackCanBlock",
        "self damage",
    )?;
    if required_i64(self_damage, "TypeEnum", "self damage")? != OWNER_BUFF_ID {
        return Err("Denvel self-damage row no longer references the owner buff".into());
    }
    let recount_row = required_row(&recount, SUMMON_RECOUNT_ID, "RecountTable")?;
    require_i64_array_contains(recount_row, "DamageId", SUMMON_DAMAGE_ID, "summon recount")?;

    let owner_lifecycle = aggregate_effect(&audit, OWNER_BUFF_ID)?;
    let gravity_lifecycle = aggregate_effect(&audit, GRAVITY_COUNTER_ID)?;
    require_self_only_lifecycle(&owner_lifecycle, "player", OWNER_BUFF_ID)?;
    require_self_only_lifecycle(&gravity_lifecycle, "monster", GRAVITY_COUNTER_ID)?;

    Ok(ProofArtifact {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-denvel-effect-family-proof",
        current_game_build,
        historical_packet_build,
        proof_state: "current-static-family-formula-identities-exact-plus-historical-player-self-and-monster-self-lifecycles-no-transfer",
        current_static: CurrentStatic {
            skill_id: SKILL_ID,
            skill_effect_id: SKILL_EFFECT_ID,
            item_id: ITEM_ID,
            monster_id: MONSTER_ID,
            skill_name: required_string(skill, "Name", "skill")?,
            official_description: description,
            active_owner_buff: BuffStatic {
                effect_id: OWNER_BUFF_ID,
                name: required_string(owner_buff, "Name", "owner buff")?,
                design_name: required_string(owner_buff, "NameDesign", "owner buff")?,
                duration_seconds: owner_duration,
                role: "casting-player-self-only-active-damage-boost-controller",
            },
            gravity_counter: BuffStatic {
                effect_id: GRAVITY_COUNTER_ID,
                name: required_string(gravity_counter, "Name", "gravity counter")?,
                design_name: required_string(gravity_counter, "NameDesign", "gravity counter")?,
                duration_seconds: gravity_duration,
                role: "affected-monster-self-sourced-gravity-counter-not-a-damage-modifier",
            },
            active_formula: ActiveFormula {
                base_damage_boost_basis_points: active.base_damage_boost_basis_points,
                extra_damage_boost_per_hp_step_basis_points: active
                    .extra_damage_boost_per_hp_step_basis_points,
                hp_step: active.hp_step_delta,
                total_damage_boost_cap_basis_points: active.total_damage_boost_cap_basis_points,
                self_damage_per_tick_basis_points_of_max_hp: 10,
                self_damage_tick_seconds: 0.2,
                duration_seconds: 20,
                recipient_scope: "casting-player-only",
                replay_disposition: "ordinary-self-damage-and-self-boost-never-transferred-rdps",
            },
            passive_owner_effect: PassiveOwnerEffect {
                effect_id: PASSIVE_BUFF_ID,
                description: passive_description,
                base_damage_basis_points: 400,
                base_final_damage_reduction_basis_points: 400,
                tiers,
                recipient_scope: "equipping-player-only",
                rdps_disposition: "ordinary-owner-damage-and-defense-never-transferred-rdps",
            },
            direct_damage: DirectDamage {
                summon_damage_id: SUMMON_DAMAGE_ID,
                summon_damage_script: required_string(
                    summon_damage,
                    "DamageScript",
                    "summon damage",
                )?,
                summon_damage_type: required_i64(summon_damage, "DamageType", "summon damage")?,
                recount_parent_id: SUMMON_RECOUNT_ID,
                recount_parent_name: required_string(recount_row, "RecountName", "summon recount")?,
                self_damage_id: SELF_DAMAGE_ID,
                self_damage_script: required_string(self_damage, "DamageScript", "self damage")?,
                self_damage_type: required_i64(self_damage, "DamageType", "self damage")?,
                self_damage_type_enum: required_i64(self_damage, "TypeEnum", "self damage")?,
                rdps_disposition: "summon hit remains source-owned damage; HP drain is self-damage; neither is support transfer",
            },
            unresolved_transform_tuples,
        },
        historical_lifecycle: HistoricalLifecycle {
            owner_buff: owner_lifecycle,
            gravity_counter: gravity_lifecycle,
        },
        attribution_policy: AttributionPolicy {
            owner_buff_semantics: "effect 2110137 is the casting player's 20-second self damage-boost and HP-drain controller",
            gravity_counter_semantics: "effect 2110152 is a 4.2-second monster-side self-sourced gravity counter; retain it as a visible control effect but never evaluate it as genericDamagePct",
            passive_semantics: "effect 3200036 is the equipping player's passive DMG and Final DMG Reduction effect and is distinct from active effect 2110137",
            summon_damage_semantics: "damage 11701120104 is source-owned Denvel summon damage under recount parent 307",
            self_damage_semantics: "damage 2211013704 is the owner's Max-HP drain tied to effect 2110137 and never support credit",
            retained_runtime_effect_ids: vec![OWNER_BUFF_ID, GRAVITY_COUNTER_ID, PASSIVE_BUFF_ID],
            transferable_effect_ids: Vec::new(),
            current_build_packet_lifecycle_observed: false,
            formula_replay_allowed_for_transfer: false,
            rejected_conflation: "do not collapse 2110137 and 2110152 into one modifier family and do not treat the monster gravity counter as the player's damage boost",
            build_gate: "historical build 24252055 proves scope only; current build 24609362 remains packet-live-gated",
        },
    })
}

fn denvel_tiers(
    stars: &Value,
) -> Result<(Vec<PassiveTier>, Vec<TransformTuple>), Box<dyn std::error::Error>> {
    let mut rows = stars
        .as_object()
        .ok_or("SkillAoyiStarTable is not an object")?
        .values()
        .filter(|row| row.get("SkillId").and_then(Value::as_i64) == Some(SKILL_ID))
        .collect::<Vec<_>>();
    rows.sort_by_key(|row| row.get("Level").and_then(Value::as_i64).unwrap_or_default());
    if rows.len() != 5 {
        return Err(format!("Denvel tier count changed from 5 to {}", rows.len()).into());
    }
    let expected = [
        (1, 286, 520, 100, 60, 0, 400),
        (2, 287, 640, 200, 120, 0, 800),
        (3, 288, 760, 300, 180, 0, 1_200),
        (4, 289, 880, 400, 240, 0, 1_600),
        (5, 290, 1_000, 500, 300, 0, 2_000),
    ];
    let mut tiers = Vec::new();
    let mut unresolved = Vec::new();
    for (row, (tier, row_id, passive_value, attr, attr_else, hp, attr_max)) in
        rows.into_iter().zip(expected)
    {
        if required_i64(row, "Level", "star")? != tier || required_i64(row, "Id", "star")? != row_id
        {
            return Err(format!("Denvel tier {tier} identity changed").into());
        }
        let buff_par = row
            .get("BuffPar")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(Value::as_array)
            .ok_or("Denvel tier BuffPar is missing")?;
        if buff_par.first().and_then(Value::as_i64) != Some(passive_value)
            || buff_par.get(1).and_then(Value::as_i64) != Some(passive_value)
        {
            return Err(format!("Denvel tier {tier} passive values changed").into());
        }
        let delta = FormulaDelta {
            base_damage_boost_basis_points: float_parameter(row, "attr")?,
            extra_damage_boost_per_hp_step_basis_points: float_parameter(row, "attrElse")?,
            hp_step_delta: float_parameter(row, "hp")?,
            total_damage_boost_cap_basis_points: float_parameter(row, "attrMax")?,
        };
        if (
            delta.base_damage_boost_basis_points,
            delta.extra_damage_boost_per_hp_step_basis_points,
            delta.hp_step_delta,
            delta.total_damage_boost_cap_basis_points,
        ) != (attr, attr_else, hp, attr_max)
        {
            return Err(format!("Denvel tier {tier} active formula deltas changed").into());
        }
        let transforms = row
            .get("TransformationType")
            .and_then(Value::as_array)
            .ok_or("Denvel tier transformations are missing")?;
        if !transforms.iter().any(|value| {
            value.as_array().is_some_and(|parts| {
                parts.first().and_then(Value::as_i64) == Some(3)
                    && parts.get(1).and_then(Value::as_i64) == Some(PASSIVE_BUFF_ID)
                    && parts.get(2).and_then(Value::as_i64) == Some(1)
            })
        }) {
            return Err(format!("Denvel tier {tier} lost passive buff transform").into());
        }
        for parts in transforms.iter().filter_map(Value::as_array) {
            let kind = parts.first().and_then(Value::as_i64).unwrap_or_default();
            if kind == 7 || kind == 9 {
                unresolved.push(TransformTuple {
                    tier,
                    kind,
                    operand: parts.get(1).and_then(Value::as_i64).unwrap_or_default(),
                    value: parts.get(2).and_then(Value::as_i64).unwrap_or_default(),
                    proof_state:
                        "retained-exact-tuple-consumer-semantics-not-proven-not-used-for-rdps",
                });
            }
        }
        tiers.push(PassiveTier {
            tier,
            row_id,
            damage_basis_points: passive_value,
            final_damage_reduction_basis_points: passive_value,
            active_formula_delta: delta,
        });
    }
    Ok((tiers, unresolved))
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
        for example in effect
            .get("examples")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let source = example
                .get("resolved_provider_entity_uuid")
                .and_then(Value::as_i64);
            let target = example.get("target_entity_uuid").and_then(Value::as_i64);
            if source.is_some() && source == target {
                result.self_sourced_examples += 1;
            } else {
                result.non_self_sourced_examples += 1;
            }
        }
    }
    if result.status_events == 0 {
        return Err(format!("provider audit has no status events for effect {effect_id}").into());
    }
    Ok(result)
}

fn require_self_only_lifecycle(
    value: &LifecycleAggregate,
    recipient: &str,
    effect_id: i64,
) -> Result<(), Box<dyn std::error::Error>> {
    let recipient_ok = match recipient {
        "player" => value.player_recipient_windows > 0 && value.monster_recipient_windows == 0,
        "monster" => value.monster_recipient_windows > 0 && value.player_recipient_windows == 0,
        _ => false,
    };
    if !recipient_ok
        || value.cross_actor_windows != 0
        || value.source_missing_windows != 0
        || value.non_self_sourced_examples != 0
        || value.self_sourced_examples == 0
        || value.other_recipient_windows != 0
        || value.unresolved_recipient_windows != 0
    {
        return Err(format!(
            "effect {effect_id} no longer has an exact {recipient}-self historical lifecycle"
        )
        .into());
    }
    Ok(())
}

fn float_parameter(value: &Value, key: &str) -> Result<i64, Box<dyn std::error::Error>> {
    value
        .get("FloatParameter")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find_map(|row| {
                let parts = row.as_array()?;
                (parts.first().and_then(Value::as_str) == Some(key))
                    .then(|| parts.get(1).and_then(Value::as_str)?.parse::<i64>().ok())
                    .flatten()
            })
        })
        .ok_or_else(|| format!("FloatParameter {key} is missing or non-integer").into())
}

fn destroy_param_seconds(value: &Value) -> Result<f64, Box<dyn std::error::Error>> {
    value
        .get("DestroyParam")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .and_then(Value::as_array)
        .and_then(|row| row.get(1))
        .and_then(Value::as_f64)
        .ok_or_else(|| "buff does not have a numeric DestroyParam".into())
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

fn required_string(
    value: &Value,
    key: &str,
    label: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("{label} {key} is missing or non-string").into())
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

fn require_i64_array_contains(
    value: &Value,
    key: &str,
    expected: i64,
    label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if !value
        .get(key)
        .and_then(Value::as_array)
        .is_some_and(|items| items.iter().any(|item| item.as_i64() == Some(expected)))
    {
        return Err(format!("{label} {key} does not contain {expected}").into());
    }
    Ok(())
}

fn require_json(
    value: &Value,
    key: &str,
    expected: &Value,
    label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if value.get(key) != Some(expected) {
        return Err(format!("{label} changed").into());
    }
    Ok(())
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
    "usage: rlogs-bpsr-denvel-effect-family-proof <decoded-table-root> <provider-audit.json> <output.json> <current-build> <historical-packet-build>".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn self_only_lifecycle_rejects_cross_actor_examples() {
        let exact = LifecycleAggregate {
            player_recipient_windows: 1,
            self_sourced_examples: 1,
            ..Default::default()
        };
        require_self_only_lifecycle(&exact, "player", OWNER_BUFF_ID).unwrap();
        let wrong = LifecycleAggregate {
            player_recipient_windows: 1,
            non_self_sourced_examples: 1,
            ..Default::default()
        };
        assert!(require_self_only_lifecycle(&wrong, "player", OWNER_BUFF_ID).is_err());
    }

    #[test]
    fn aggregates_player_and_monster_effects_without_conflation() {
        let audit = json!({"reports": [{"effects": [
            {"effect_id": OWNER_BUFF_ID, "lifecycle": {"status_events": 2, "opened_windows": 1, "closed_windows": 1, "cross_actor_windows": 0, "source_missing_windows": 0, "applied": 1, "refreshed": 0, "stacked": 0, "consumed": 0, "removed": 1, "observed_active_micros": 20}, "recipient_scope": {"player": 1, "monster": 0, "other": 0, "unresolved": 0}, "examples": [{"resolved_provider_entity_uuid": 9, "target_entity_uuid": 9}]},
            {"effect_id": GRAVITY_COUNTER_ID, "lifecycle": {"status_events": 3, "opened_windows": 1, "closed_windows": 1, "cross_actor_windows": 0, "source_missing_windows": 0, "applied": 1, "refreshed": 1, "stacked": 0, "consumed": 0, "removed": 1, "observed_active_micros": 4}, "recipient_scope": {"player": 0, "monster": 1, "other": 0, "unresolved": 0}, "examples": [{"resolved_provider_entity_uuid": 10, "target_entity_uuid": 10}]}
        ]}]});
        let owner = aggregate_effect(&audit, OWNER_BUFF_ID).unwrap();
        let gravity = aggregate_effect(&audit, GRAVITY_COUNTER_ID).unwrap();
        assert_eq!(owner.player_recipient_windows, 1);
        assert_eq!(gravity.monster_recipient_windows, 1);
        assert_eq!(owner.self_sourced_examples, 1);
        assert_eq!(gravity.self_sourced_examples, 1);
    }
}
