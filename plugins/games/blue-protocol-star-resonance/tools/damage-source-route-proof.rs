use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    fs::File,
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

use rlogs_game_bpsr::BpsrDamageSourceKind;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 9;

#[derive(Debug, Serialize)]
struct RouteCatalog {
    schema_version: u16,
    game_build: String,
    generated_by: &'static str,
    promotion_state: &'static str,
    inputs: Vec<InputArtifact>,
    policy: RoutePolicy,
    typed_route_proof: TypedRouteProof,
    damage_source_enum: Vec<EnumValue>,
    summary: RouteSummary,
    keys: Vec<RouteKey>,
}

#[derive(Debug, Serialize)]
struct TypedRouteProof {
    damage_type_2_rows: usize,
    damage_type_2_rows_with_buff_type_enum: usize,
    standard_decimal_buff_rows: usize,
    typed_buff_exception_rows: usize,
    damage_type_3_rows: usize,
    damage_type_3_rows_with_skill_effect_type_enum: usize,
    typed_bullet_rows_with_bullet_table: usize,
    typed_server_projectile_rows_with_run_and_shape: usize,
    typed_rows_without_physical_projectile_definition: usize,
}

#[derive(Debug, Serialize)]
struct InputArtifact {
    role: &'static str,
    file: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct RoutePolicy {
    runtime_formula_authority: bool,
    exact_build_tables_required: bool,
    packet_damage_source_required: bool,
    unknown_source_values_retained: bool,
    unresolved_candidates_hidden: bool,
    selection_rule: &'static str,
    recount_ownership_rule: &'static str,
}

#[derive(Debug, Serialize)]
struct EnumValue {
    name: String,
    value: i32,
    canonical_label: &'static str,
}

#[derive(Debug, Serialize)]
struct RouteSummary {
    lookup_keys: usize,
    candidate_rows: usize,
    candidates_with_static_route: usize,
    skill_effect_routes: usize,
    bullet_routes: usize,
    buff_routes: usize,
    candidates_with_recount_owner: usize,
    unresolved_route_candidates_with_recount_owner: usize,
    keys_resolvable_by_packet_source: usize,
    ambiguous_keys_resolvable_by_packet_source: usize,
    keys_with_unresolved_candidates: usize,
    keys_with_overlapping_source_routes: usize,
}

#[derive(Debug, Serialize)]
struct RouteKey {
    lookup_key: String,
    ability_id: i64,
    hit_event_id: i32,
    candidates: Vec<RouteCandidate>,
    selection_by_damage_source: Vec<SourceSelection>,
    resolution_state: &'static str,
}

#[derive(Debug, Serialize)]
struct RouteCandidate {
    damage_attr_id: i64,
    routes: Vec<StaticRoute>,
    recount_owners: Vec<RecountOwner>,
}

#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct RecountOwner {
    recount_id: i64,
    recount_name: String,
}

#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct StaticRoute {
    damage_source: BpsrDamageSourceKind,
    damage_source_id: i32,
    construction: &'static str,
    owner_table: &'static str,
    owner_id: i64,
    intermediary_id: Option<i64>,
}

#[derive(Debug, Serialize)]
struct SourceSelection {
    damage_source: BpsrDamageSourceKind,
    damage_source_id: i32,
    damage_attr_id: i64,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("damage-source route proof failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let surface_path = PathBuf::from(option(&arguments, "--surface")?);
    let damage_attr_path = PathBuf::from(option(&arguments, "--damage-attr-table")?);
    let bullet_path = PathBuf::from(option(&arguments, "--bullet-table")?);
    let bullet_run_path = PathBuf::from(option(&arguments, "--bullet-run-table")?);
    let bullet_shape_path = PathBuf::from(option(&arguments, "--bullet-shape-table")?);
    let buff_path = PathBuf::from(option(&arguments, "--buff-table")?);
    let skill_path = PathBuf::from(option(&arguments, "--skill-table")?);
    let skill_effect_path = PathBuf::from(option(&arguments, "--skill-effect-table")?);
    let skill_fight_level_path = PathBuf::from(option(&arguments, "--skill-fight-level-table")?);
    let recount_path = PathBuf::from(option(&arguments, "--recount-table")?);
    let il2cpp_path = PathBuf::from(option(&arguments, "--il2cpp-surface")?);
    let output_path = PathBuf::from(option(&arguments, "--output")?);
    let game_build = option(&arguments, "--build")?.to_owned();
    if arguments.len() != 26 {
        return Err(usage().into());
    }
    if game_build.is_empty() || !game_build.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("--build requires a numeric client build".into());
    }

    let surface = read_json(&surface_path)?;
    let damage_attrs = read_json(&damage_attr_path)?;
    let bullets = read_json(&bullet_path)?;
    let bullet_runs = read_json(&bullet_run_path)?;
    let bullet_shapes = read_json(&bullet_shape_path)?;
    let buffs = read_json(&buff_path)?;
    let skills = read_json(&skill_path)?;
    let skill_effects = read_json(&skill_effect_path)?;
    let skill_fight_levels = read_json(&skill_fight_level_path)?;
    let recount = read_json(&recount_path)?;
    let il2cpp = read_json(&il2cpp_path)?;
    require_build(&il2cpp, &game_build)?;
    let damage_source_enum = exact_damage_source_enum(&il2cpp)?;
    let lookup = surface
        .get("linked_hit_event_candidate_lookup")
        .and_then(Value::as_object)
        .ok_or("damage formula surface is missing linked_hit_event_candidate_lookup")?;
    let damage_attr_rows = damage_attrs
        .as_object()
        .ok_or("DamageAttrTable must be an object")?;
    let bullet_rows = bullets.as_object().ok_or("BulletTable must be an object")?;
    let bullet_run_rows = bullet_runs
        .as_object()
        .ok_or("BulletRunTable must be an object")?;
    let bullet_shape_rows = bullet_shapes
        .as_object()
        .ok_or("BulletShapeTable must be an object")?;
    let buff_rows = buffs.as_object().ok_or("BuffTable must be an object")?;
    let skill_rows = skills.as_object().ok_or("SkillTable must be an object")?;
    let skill_effect_rows = skill_effects
        .as_object()
        .ok_or("SkillEffectTable must be an object")?;
    let skill_fight_level_rows = skill_fight_levels
        .as_object()
        .ok_or("SkillFightLevelTable must be an object")?;
    let recount_rows = recount
        .as_object()
        .ok_or("RecountTable must be an object")?;
    let recount_owners_by_damage_id = recount_owners(recount_rows)?;

    let mut candidate_rows = 0_usize;
    let mut candidates_with_static_route = 0_usize;
    let mut skill_effect_routes = 0_usize;
    let mut bullet_routes = 0_usize;
    let mut buff_routes = 0_usize;
    let mut candidates_with_recount_owner = 0_usize;
    let mut unresolved_route_candidates_with_recount_owner = 0_usize;
    let mut keys_resolvable_by_packet_source = 0_usize;
    let mut ambiguous_keys_resolvable_by_packet_source = 0_usize;
    let mut keys_with_unresolved_candidates = 0_usize;
    let mut keys_with_overlapping_source_routes = 0_usize;
    let mut keys = Vec::with_capacity(lookup.len());

    for (lookup_key, raw_candidates) in lookup {
        let (ability_id, hit_event_id) = parse_lookup_key(lookup_key)?;
        let candidate_ids = raw_candidates
            .as_array()
            .ok_or_else(|| format!("lookup {lookup_key} must contain an array"))?
            .iter()
            .map(required_i64)
            .collect::<Result<Vec<_>, _>>()?;
        candidate_rows = candidate_rows.saturating_add(candidate_ids.len());
        let mut candidates = Vec::with_capacity(candidate_ids.len());
        for damage_attr_id in candidate_ids {
            let mut routes = exact_routes_with_typed_tables(
                ability_id,
                hit_event_id,
                damage_attr_id,
                bullet_rows,
                buff_rows,
                skill_rows,
                skill_effect_rows,
                skill_fight_level_rows,
                damage_attr_rows,
                bullet_run_rows,
                bullet_shape_rows,
            )?;
            routes.sort();
            routes.dedup();
            candidates_with_static_route += usize::from(!routes.is_empty());
            skill_effect_routes += routes
                .iter()
                .filter(|route| route.damage_source == BpsrDamageSourceKind::Skill)
                .count();
            bullet_routes += routes
                .iter()
                .filter(|route| route.damage_source == BpsrDamageSourceKind::Bullet)
                .count();
            buff_routes += routes
                .iter()
                .filter(|route| route.damage_source == BpsrDamageSourceKind::Buff)
                .count();
            let recount_owners = recount_owners_by_damage_id
                .get(&damage_attr_id)
                .cloned()
                .unwrap_or_default();
            candidates_with_recount_owner += usize::from(!recount_owners.is_empty());
            unresolved_route_candidates_with_recount_owner +=
                usize::from(routes.is_empty() && !recount_owners.is_empty());
            candidates.push(RouteCandidate {
                damage_attr_id,
                routes,
                recount_owners,
            });
        }

        let mut source_to_candidates = BTreeMap::<BpsrDamageSourceKind, BTreeSet<i64>>::new();
        for candidate in &candidates {
            for route in &candidate.routes {
                source_to_candidates
                    .entry(route.damage_source)
                    .or_default()
                    .insert(candidate.damage_attr_id);
            }
        }
        let overlapping_source_routes = source_to_candidates.values().any(|ids| ids.len() > 1);
        keys_with_overlapping_source_routes += usize::from(overlapping_source_routes);
        let selection_by_damage_source = source_to_candidates
            .into_iter()
            .filter_map(|(damage_source, ids)| {
                (ids.len() == 1).then(|| SourceSelection {
                    damage_source,
                    damage_source_id: damage_source.protocol_id(),
                    damage_attr_id: *ids.iter().next().expect("one source candidate"),
                })
            })
            .collect::<Vec<_>>();
        let unresolved_candidates = candidates
            .iter()
            .any(|candidate| candidate.routes.is_empty());
        keys_with_unresolved_candidates += usize::from(unresolved_candidates);
        let every_candidate_selectable = candidates.iter().all(|candidate| {
            selection_by_damage_source
                .iter()
                .any(|selection| selection.damage_attr_id == candidate.damage_attr_id)
        });
        let resolvable = !candidates.is_empty()
            && !overlapping_source_routes
            && !unresolved_candidates
            && every_candidate_selectable;
        keys_resolvable_by_packet_source += usize::from(resolvable);
        ambiguous_keys_resolvable_by_packet_source +=
            usize::from(resolvable && candidates.len() > 1);
        keys.push(RouteKey {
            lookup_key: lookup_key.clone(),
            ability_id,
            hit_event_id,
            candidates,
            selection_by_damage_source,
            resolution_state: if resolvable {
                "exact-static-route-awaiting-same-build-packet-occurrence"
            } else {
                "unresolved-retained"
            },
        });
    }

    let catalog = RouteCatalog {
        schema_version: SCHEMA_VERSION,
        game_build,
        generated_by: "rlogs-bpsr-damage-source-route-proof",
        promotion_state: "candidate-only-current-build-packet-occurrence-required",
        inputs: vec![
            input_artifact("damage_formula_surface", &surface_path)?,
            input_artifact("damage_attr_table", &damage_attr_path)?,
            input_artifact("bullet_table", &bullet_path)?,
            input_artifact("bullet_run_table", &bullet_run_path)?,
            input_artifact("bullet_shape_table", &bullet_shape_path)?,
            input_artifact("buff_table", &buff_path)?,
            input_artifact("skill_table", &skill_path)?,
            input_artifact("skill_effect_table", &skill_effect_path)?,
            input_artifact("skill_fight_level_table", &skill_fight_level_path)?,
            input_artifact("recount_table", &recount_path)?,
            input_artifact("il2cpp_combat_surface", &il2cpp_path)?,
        ],
        policy: RoutePolicy {
            runtime_formula_authority: false,
            exact_build_tables_required: true,
            packet_damage_source_required: true,
            unknown_source_values_retained: true,
            unresolved_candidates_hidden: false,
            selection_rule: "select only when the current-build table construction maps exactly one candidate to the packet EDamageSource; otherwise retain every candidate",
            recount_ownership_rule: "RecountTable.DamageId membership is retained as exact display and aggregation ownership only; it never proves packet EDamageSource, server formula selection, or runtime provider identity",
        },
        typed_route_proof: typed_route_proof(
            damage_attr_rows,
            bullet_rows,
            bullet_run_rows,
            bullet_shape_rows,
            buff_rows,
            skill_effect_rows,
        ),
        damage_source_enum,
        summary: RouteSummary {
            lookup_keys: lookup.len(),
            candidate_rows,
            candidates_with_static_route,
            skill_effect_routes,
            bullet_routes,
            buff_routes,
            candidates_with_recount_owner,
            unresolved_route_candidates_with_recount_owner,
            keys_resolvable_by_packet_source,
            ambiguous_keys_resolvable_by_packet_source,
            keys_with_unresolved_candidates,
            keys_with_overlapping_source_routes,
        },
        keys,
    };
    let mut writer = BufWriter::new(File::create(&output_path)?);
    serde_json::to_writer(&mut writer, &catalog)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    eprintln!(
        "wrote {} route keys; {} ambiguous keys are exactly source-selectable; {} keys retain unresolved candidates",
        catalog.summary.lookup_keys,
        catalog.summary.ambiguous_keys_resolvable_by_packet_source,
        catalog.summary.keys_with_unresolved_candidates,
    );
    Ok(())
}

#[cfg(test)]
fn exact_routes(
    ability_id: i64,
    hit_event_id: i32,
    damage_attr_id: i64,
    bullets: &serde_json::Map<String, Value>,
    buffs: &serde_json::Map<String, Value>,
    skills: &serde_json::Map<String, Value>,
    skill_effects: &serde_json::Map<String, Value>,
    skill_fight_levels: &serde_json::Map<String, Value>,
) -> Result<Vec<StaticRoute>, String> {
    exact_routes_with_typed_tables(
        ability_id,
        hit_event_id,
        damage_attr_id,
        bullets,
        buffs,
        skills,
        skill_effects,
        skill_fight_levels,
        &serde_json::Map::new(),
        &serde_json::Map::new(),
        &serde_json::Map::new(),
    )
}

#[allow(clippy::too_many_arguments)]
fn exact_routes_with_typed_tables(
    ability_id: i64,
    hit_event_id: i32,
    damage_attr_id: i64,
    bullets: &serde_json::Map<String, Value>,
    buffs: &serde_json::Map<String, Value>,
    skills: &serde_json::Map<String, Value>,
    skill_effects: &serde_json::Map<String, Value>,
    skill_fight_levels: &serde_json::Map<String, Value>,
    damage_attrs: &serde_json::Map<String, Value>,
    bullet_runs: &serde_json::Map<String, Value>,
    bullet_shapes: &serde_json::Map<String, Value>,
) -> Result<Vec<StaticRoute>, String> {
    if !(0..=99).contains(&hit_event_id) {
        return Ok(Vec::new());
    }
    let mut routes = Vec::new();
    if buffs.contains_key(&ability_id.to_string()) {
        let constructed = format!("2{ability_id}{hit_event_id:02}")
            .parse::<i64>()
            .map_err(|_| format!("buff damage ID overflow for buff {ability_id}"))?;
        if constructed == damage_attr_id {
            routes.push(StaticRoute {
                damage_source: BpsrDamageSourceKind::Buff,
                damage_source_id: BpsrDamageSourceKind::Buff.protocol_id(),
                construction: "decimal 2 + BuffTable.Id + two-digit packet hit_event_id",
                owner_table: "BuffTable",
                owner_id: ability_id,
                intermediary_id: None,
            });
        }
    }
    let damage_attr_text = damage_attr_id.to_string();
    let hit_suffix = format!("{hit_event_id:02}");
    if let Some(encoded_buff) = damage_attr_text
        .strip_prefix('2')
        .and_then(|value| value.strip_suffix(&hit_suffix))
        .and_then(|value| value.parse::<i64>().ok())
    {
        if encoded_buff != ability_id && buffs.contains_key(&encoded_buff.to_string()) {
            routes.push(StaticRoute {
                damage_source: BpsrDamageSourceKind::Buff,
                damage_source_id: BpsrDamageSourceKind::Buff.protocol_id(),
                construction: "DamageAttrTable.Id decimal 2 prefix and packet hit suffix identify an exact existing BuffTable.Id",
                owner_table: "DamageAttrTable.Id -> BuffTable",
                owner_id: ability_id,
                intermediary_id: Some(encoded_buff),
            });
        }
    }
    if !routes
        .iter()
        .any(|route| route.damage_source == BpsrDamageSourceKind::Buff)
    {
        if let Some(damage_attr) = damage_attrs.get(&damage_attr_id.to_string()) {
            if integer_field(damage_attr, "DamageType") == Some(2)
                && integer_field(damage_attr, "TypeEnum") == Some(ability_id)
                && buffs.contains_key(&ability_id.to_string())
            {
                routes.push(StaticRoute {
                    damage_source: BpsrDamageSourceKind::Buff,
                    damage_source_id: BpsrDamageSourceKind::Buff.protocol_id(),
                    construction: "DamageAttrTable.DamageType=2 and TypeEnum equals the packet ability id, which resolves to an exact current-build BuffTable.Id",
                    owner_table: "DamageAttrTable.TypeEnum -> BuffTable",
                    owner_id: ability_id,
                    intermediary_id: None,
                });
            }
        }
    }
    let skill_table_effects = skills
        .get(&ability_id.to_string())
        .and_then(|skill| skill.get("EffectIDs"))
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(value_i64).collect::<BTreeSet<_>>())
        .unwrap_or_default();

    let mut declared_effects = BTreeMap::<i64, BTreeSet<&'static str>>::new();
    for effect_id in skill_table_effects {
        declared_effects
            .entry(effect_id)
            .or_default()
            .insert("SkillTable.EffectIDs");
    }
    for effect in skill_effects.values() {
        if integer_field(effect, "SkillId") == Some(ability_id) {
            if let Some(effect_id) = integer_field(effect, "Id") {
                declared_effects
                    .entry(effect_id)
                    .or_default()
                    .insert("SkillEffectTable.SkillId -> Id");
            }
        }
    }
    for level in skill_fight_levels.values() {
        if integer_field(level, "SkillId") == Some(ability_id) {
            if let Some(effect_id) = integer_field(level, "SkillEffectId") {
                declared_effects
                    .entry(effect_id)
                    .or_default()
                    .insert("SkillFightLevelTable.SkillId -> SkillEffectId");
            }
        }
    }

    let mut candidate_bullet_ids = declared_effects.clone();
    candidate_bullet_ids
        .entry(ability_id)
        .or_default()
        .insert("packet ability id");
    for (bullet_id, origins) in candidate_bullet_ids {
        let Some(bullet) = bullets.get(&bullet_id.to_string()) else {
            continue;
        };
        if let Some(bullet_attr_id) = integer_field(bullet, "BulletAttrId") {
            for origin in origins {
                let owner_table = bullet_owner_table(origin);
                if bullet_attr_id == damage_attr_id {
                    routes.push(StaticRoute {
                        damage_source: BpsrDamageSourceKind::Bullet,
                        damage_source_id: BpsrDamageSourceKind::Bullet.protocol_id(),
                        construction: bullet_exact_construction(origin),
                        owner_table,
                        owner_id: ability_id,
                        intermediary_id: Some(bullet_id),
                    });
                } else if bullet_attr_id
                    .checked_mul(100)
                    .and_then(|value| value.checked_add(i64::from(hit_event_id)))
                    == Some(damage_attr_id)
                {
                    routes.push(StaticRoute {
                        damage_source: BpsrDamageSourceKind::Bullet,
                        damage_source_id: BpsrDamageSourceKind::Bullet.protocol_id(),
                        construction: bullet_suffix_construction(origin),
                        owner_table,
                        owner_id: ability_id,
                        intermediary_id: Some(bullet_id),
                    });
                } else if bullet_attr_id
                    .checked_div(100)
                    .and_then(|value| value.checked_mul(100))
                    .and_then(|value| value.checked_add(i64::from(hit_event_id)))
                    == Some(damage_attr_id)
                {
                    routes.push(StaticRoute {
                        damage_source: BpsrDamageSourceKind::Bullet,
                        damage_source_id: BpsrDamageSourceKind::Bullet.protocol_id(),
                        construction: bullet_replaced_suffix_construction(origin),
                        owner_table,
                        owner_id: ability_id,
                        intermediary_id: Some(bullet_id),
                    });
                } else {
                    let encoded_bullet_damage = format!("3{bullet_id}{hit_event_id:02}")
                        .parse::<i64>()
                        .map_err(|_| format!("bullet damage ID overflow for bullet {bullet_id}"))?;
                    if encoded_bullet_damage == damage_attr_id {
                        routes.push(StaticRoute {
                            damage_source: BpsrDamageSourceKind::Bullet,
                            damage_source_id: BpsrDamageSourceKind::Bullet.protocol_id(),
                            construction: bullet_id_construction(origin),
                            owner_table,
                            owner_id: ability_id,
                            intermediary_id: Some(bullet_id),
                        });
                    }
                }
            }
        }
    }
    if !routes
        .iter()
        .any(|route| route.damage_source == BpsrDamageSourceKind::Bullet)
    {
        if let Some(encoded_bullet) = damage_attr_text
            .strip_prefix('3')
            .and_then(|value| value.strip_suffix(&hit_suffix))
            .and_then(|value| value.parse::<i64>().ok())
        {
            if encoded_bullet != ability_id {
                if let Some(bullet) = bullets.get(&encoded_bullet.to_string()) {
                    if integer_field(bullet, "BulletAttrId") == Some(damage_attr_id) {
                        routes.push(StaticRoute {
                            damage_source: BpsrDamageSourceKind::Bullet,
                            damage_source_id: BpsrDamageSourceKind::Bullet.protocol_id(),
                            construction: "DamageAttrTable.Id decimal 3 prefix and packet hit suffix identify an exact existing BulletTable.Id whose BulletAttrId equals DamageAttrTable.Id",
                            owner_table: "DamageAttrTable.Id -> BulletTable",
                            owner_id: ability_id,
                            intermediary_id: Some(encoded_bullet),
                        });
                    }
                }
            }
        }
    }
    if !routes
        .iter()
        .any(|route| route.damage_source == BpsrDamageSourceKind::Bullet)
    {
        if let Some(damage_attr) = damage_attrs.get(&damage_attr_id.to_string()) {
            let physical_id = ability_id
                .checked_mul(100)
                .and_then(|value| value.checked_add(i64::from(hit_event_id)));
            let has_client_bullet = bullets.contains_key(&ability_id.to_string());
            let has_server_projectile = physical_id.is_some_and(|id| {
                bullet_runs.contains_key(&id.to_string())
                    && bullet_shapes.contains_key(&id.to_string())
            });
            if integer_field(damage_attr, "DamageType") == Some(3)
                && integer_field(damage_attr, "TypeEnum") == Some(ability_id)
                && skill_effects.contains_key(&ability_id.to_string())
                && (has_client_bullet || has_server_projectile)
            {
                routes.push(StaticRoute {
                    damage_source: BpsrDamageSourceKind::Bullet,
                    damage_source_id: BpsrDamageSourceKind::Bullet.protocol_id(),
                    construction: if has_client_bullet {
                        "DamageAttrTable.DamageType=3 and TypeEnum equals the packet ability id, which resolves to exact current-build SkillEffectTable and BulletTable rows"
                    } else {
                        "DamageAttrTable.DamageType=3 and TypeEnum equals the packet ability id, which resolves to an exact SkillEffectTable row and matching BulletRunTable and BulletShapeTable rows at effect*100+hit"
                    },
                    owner_table: if has_client_bullet {
                        "DamageAttrTable.TypeEnum -> SkillEffectTable + BulletTable"
                    } else {
                        "DamageAttrTable.TypeEnum -> SkillEffectTable + BulletRunTable + BulletShapeTable"
                    },
                    owner_id: ability_id,
                    intermediary_id: Some(ability_id),
                });
            }
        }
    }
    for (effect_id, origins) in declared_effects {
        let constructed = format!("1{effect_id}{hit_event_id:02}")
            .parse::<i64>()
            .map_err(|_| format!("skill-effect damage ID overflow for effect {effect_id}"))?;
        if constructed == damage_attr_id {
            for origin in origins {
                let effect_row_exists = skill_effects.contains_key(&effect_id.to_string());
                routes.push(StaticRoute {
                    damage_source: BpsrDamageSourceKind::Skill,
                    damage_source_id: BpsrDamageSourceKind::Skill.protocol_id(),
                    construction: skill_effect_construction(origin, effect_row_exists),
                    owner_table: origin,
                    owner_id: ability_id,
                    intermediary_id: Some(effect_id),
                });
            }
        }
    }
    Ok(routes)
}

fn typed_route_proof(
    damage_attrs: &serde_json::Map<String, Value>,
    bullets: &serde_json::Map<String, Value>,
    bullet_runs: &serde_json::Map<String, Value>,
    bullet_shapes: &serde_json::Map<String, Value>,
    buffs: &serde_json::Map<String, Value>,
    skill_effects: &serde_json::Map<String, Value>,
) -> TypedRouteProof {
    let mut proof = TypedRouteProof {
        damage_type_2_rows: 0,
        damage_type_2_rows_with_buff_type_enum: 0,
        standard_decimal_buff_rows: 0,
        typed_buff_exception_rows: 0,
        damage_type_3_rows: 0,
        damage_type_3_rows_with_skill_effect_type_enum: 0,
        typed_bullet_rows_with_bullet_table: 0,
        typed_server_projectile_rows_with_run_and_shape: 0,
        typed_rows_without_physical_projectile_definition: 0,
    };
    for damage_attr in damage_attrs.values() {
        let Some(damage_attr_id) = integer_field(damage_attr, "Id") else {
            continue;
        };
        let Some(type_enum) = integer_field(damage_attr, "TypeEnum") else {
            continue;
        };
        match integer_field(damage_attr, "DamageType") {
            Some(2) => {
                proof.damage_type_2_rows += 1;
                if buffs.contains_key(&type_enum.to_string()) {
                    proof.damage_type_2_rows_with_buff_type_enum += 1;
                    if damage_attr_id.to_string().starts_with('2') {
                        proof.standard_decimal_buff_rows += 1;
                    } else {
                        proof.typed_buff_exception_rows += 1;
                    }
                }
            }
            Some(3) => {
                proof.damage_type_3_rows += 1;
                if skill_effects.contains_key(&type_enum.to_string()) {
                    proof.damage_type_3_rows_with_skill_effect_type_enum += 1;
                    if bullets.contains_key(&type_enum.to_string()) {
                        proof.typed_bullet_rows_with_bullet_table += 1;
                    } else {
                        let hit = damage_attr_id.rem_euclid(100);
                        let physical_id = type_enum
                            .checked_mul(100)
                            .and_then(|value| value.checked_add(hit));
                        if physical_id.is_some_and(|id| {
                            bullet_runs.contains_key(&id.to_string())
                                && bullet_shapes.contains_key(&id.to_string())
                        }) {
                            proof.typed_server_projectile_rows_with_run_and_shape += 1;
                        } else {
                            proof.typed_rows_without_physical_projectile_definition += 1;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    proof
}

fn recount_owners(
    rows: &serde_json::Map<String, Value>,
) -> Result<BTreeMap<i64, Vec<RecountOwner>>, String> {
    let mut owners = BTreeMap::<i64, BTreeSet<RecountOwner>>::new();
    for (row_key, row) in rows {
        let recount_id = integer_field(row, "Id")
            .ok_or_else(|| format!("RecountTable row {row_key} is missing Id"))?;
        let recount_name = row
            .get("RecountName")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let damage_ids = row
            .get("DamageId")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("RecountTable row {row_key} is missing DamageId"))?;
        for damage_id in damage_ids {
            let damage_id = required_i64(damage_id)?;
            owners.entry(damage_id).or_default().insert(RecountOwner {
                recount_id,
                recount_name: recount_name.clone(),
            });
        }
    }
    Ok(owners
        .into_iter()
        .map(|(damage_id, owners)| (damage_id, owners.into_iter().collect()))
        .collect())
}

fn bullet_owner_table(origin: &'static str) -> &'static str {
    match origin {
        "packet ability id" => "BulletTable",
        "SkillTable.EffectIDs" => "SkillTable.EffectIDs -> BulletTable",
        "SkillEffectTable.SkillId -> Id" => "SkillEffectTable.SkillId -> Id -> BulletTable",
        "SkillFightLevelTable.SkillId -> SkillEffectId" => {
            "SkillFightLevelTable.SkillId -> SkillEffectId -> BulletTable"
        }
        _ => "unrecognized-static-origin",
    }
}

fn bullet_exact_construction(origin: &'static str) -> &'static str {
    match origin {
        "packet ability id" => {
            "packet ability id -> BulletTable.BulletAttrId equals DamageAttrTable.Id"
        }
        "SkillTable.EffectIDs" => {
            "SkillTable.EffectIDs -> BulletTable.BulletAttrId equals DamageAttrTable.Id"
        }
        "SkillEffectTable.SkillId -> Id" => {
            "SkillEffectTable.SkillId -> Id -> BulletTable.BulletAttrId equals DamageAttrTable.Id"
        }
        "SkillFightLevelTable.SkillId -> SkillEffectId" => {
            "SkillFightLevelTable.SkillId -> SkillEffectId -> BulletTable.BulletAttrId equals DamageAttrTable.Id"
        }
        _ => "unrecognized-static-origin",
    }
}

fn bullet_suffix_construction(origin: &'static str) -> &'static str {
    match origin {
        "packet ability id" => {
            "packet ability id -> BulletTable.BulletAttrId * 100 + packet hit_event_id"
        }
        "SkillTable.EffectIDs" => {
            "SkillTable.EffectIDs -> BulletTable.BulletAttrId * 100 + packet hit_event_id"
        }
        "SkillEffectTable.SkillId -> Id" => {
            "SkillEffectTable.SkillId -> Id -> BulletTable.BulletAttrId * 100 + packet hit_event_id"
        }
        "SkillFightLevelTable.SkillId -> SkillEffectId" => {
            "SkillFightLevelTable.SkillId -> SkillEffectId -> BulletTable.BulletAttrId * 100 + packet hit_event_id"
        }
        _ => "unrecognized-static-origin",
    }
}

fn bullet_replaced_suffix_construction(origin: &'static str) -> &'static str {
    match origin {
        "packet ability id" => {
            "packet ability id -> BulletTable.BulletAttrId final two digits replaced by packet hit_event_id"
        }
        "SkillTable.EffectIDs" => {
            "SkillTable.EffectIDs -> BulletTable.BulletAttrId final two digits replaced by packet hit_event_id"
        }
        "SkillEffectTable.SkillId -> Id" => {
            "SkillEffectTable.SkillId -> Id -> BulletTable.BulletAttrId final two digits replaced by packet hit_event_id"
        }
        "SkillFightLevelTable.SkillId -> SkillEffectId" => {
            "SkillFightLevelTable.SkillId -> SkillEffectId -> BulletTable.BulletAttrId final two digits replaced by packet hit_event_id"
        }
        _ => "unrecognized-static-origin",
    }
}

fn bullet_id_construction(origin: &'static str) -> &'static str {
    match origin {
        "packet ability id" => {
            "decimal 3 + exact BulletTable.Id selected by packet ability id + two-digit packet hit_event_id; BulletAttrId is inconsistent with the encoded damage ID"
        }
        "SkillTable.EffectIDs" => {
            "decimal 3 + exact BulletTable.Id selected by SkillTable.EffectIDs + two-digit packet hit_event_id; BulletAttrId is inconsistent with the encoded damage ID"
        }
        "SkillEffectTable.SkillId -> Id" => {
            "decimal 3 + exact BulletTable.Id selected by SkillEffectTable.SkillId -> Id + two-digit packet hit_event_id; BulletAttrId is inconsistent with the encoded damage ID"
        }
        "SkillFightLevelTable.SkillId -> SkillEffectId" => {
            "decimal 3 + exact BulletTable.Id selected by SkillFightLevelTable.SkillId -> SkillEffectId + two-digit packet hit_event_id; BulletAttrId is inconsistent with the encoded damage ID"
        }
        _ => "unrecognized-static-origin",
    }
}

fn skill_effect_construction(origin: &'static str, effect_row_exists: bool) -> &'static str {
    match (origin, effect_row_exists) {
        ("SkillTable.EffectIDs", true) => {
            "decimal 1 + SkillTable.EffectIDs member + two-digit packet hit_event_id; referenced SkillEffectTable row exists"
        }
        ("SkillTable.EffectIDs", false) => {
            "decimal 1 + SkillTable.EffectIDs member + two-digit packet hit_event_id; server-only referenced effect is absent from current SkillEffectTable"
        }
        ("SkillEffectTable.SkillId -> Id", _) => {
            "decimal 1 + SkillEffectTable.Id selected by exact SkillId + two-digit packet hit_event_id"
        }
        ("SkillFightLevelTable.SkillId -> SkillEffectId", true) => {
            "decimal 1 + SkillFightLevelTable.SkillEffectId selected by exact SkillId + two-digit packet hit_event_id; referenced SkillEffectTable row exists"
        }
        ("SkillFightLevelTable.SkillId -> SkillEffectId", false) => {
            "decimal 1 + SkillFightLevelTable.SkillEffectId selected by exact SkillId + two-digit packet hit_event_id; referenced effect is absent from current SkillEffectTable"
        }
        _ => "unrecognized-static-origin",
    }
}

fn exact_damage_source_enum(il2cpp: &Value) -> Result<Vec<EnumValue>, String> {
    let types = il2cpp
        .get("types")
        .and_then(Value::as_array)
        .ok_or_else(|| "IL2CPP combat surface is missing types".to_owned())?;
    let values = types
        .iter()
        .find(|value| {
            value.get("namespace").and_then(Value::as_str) == Some("Zproto")
                && value.get("name").and_then(Value::as_str) == Some("EDamageSource")
        })
        .and_then(|value| value.get("enum_values"))
        .and_then(Value::as_array)
        .ok_or_else(|| "IL2CPP combat surface is missing Zproto.EDamageSource".to_owned())?;
    let expected = [
        ("EDamageSourceSkill", BpsrDamageSourceKind::Skill),
        ("EDamageSourceBullet", BpsrDamageSourceKind::Bullet),
        ("EDamageSourceBuff", BpsrDamageSourceKind::Buff),
        ("EDamageSourceFall", BpsrDamageSourceKind::Fall),
        ("EDamageSourceFakeBullet", BpsrDamageSourceKind::FakeBullet),
        ("EDamageSourceOther", BpsrDamageSourceKind::Other),
    ];
    expected
        .into_iter()
        .map(|(name, kind)| {
            let found = values.iter().find(|value| {
                value.get("name").and_then(Value::as_str) == Some(name)
                    && value.get("value").and_then(Value::as_i64)
                        == Some(i64::from(kind.protocol_id()))
            });
            if found.is_none() {
                return Err(format!(
                    "current IL2CPP EDamageSource does not contain exact {name}={} discriminant",
                    kind.protocol_id()
                ));
            }
            Ok(EnumValue {
                name: name.to_owned(),
                value: kind.protocol_id(),
                canonical_label: kind.as_str(),
            })
        })
        .collect()
}

fn require_build(il2cpp: &Value, expected: &str) -> Result<(), String> {
    let actual = il2cpp
        .get("build_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "IL2CPP combat surface is missing build_id".to_owned())?;
    if actual != expected {
        return Err(format!(
            "IL2CPP combat surface build {actual} does not match --build {expected}"
        ));
    }
    Ok(())
}

fn parse_lookup_key(value: &str) -> Result<(i64, i32), String> {
    let (ability, hit) = value
        .split_once(':')
        .ok_or_else(|| format!("invalid lookup key {value}"))?;
    Ok((
        ability
            .parse()
            .map_err(|_| format!("invalid ability in lookup key {value}"))?,
        hit.parse()
            .map_err(|_| format!("invalid hit in lookup key {value}"))?,
    ))
}

fn integer_field(value: &Value, field: &str) -> Option<i64> {
    value.get(field).and_then(value_i64)
}

fn value_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
}

fn required_i64(value: &Value) -> Result<i64, String> {
    value_i64(value).ok_or_else(|| "candidate damage ID is not an integer".to_owned())
}

fn read_json(path: &Path) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_reader(BufReader::new(File::open(path)?))?)
}

fn option<'a>(arguments: &'a [String], flag: &str) -> Result<&'a str, String> {
    let position = arguments
        .iter()
        .position(|value| value == flag)
        .ok_or_else(usage)?;
    arguments
        .get(position + 1)
        .map(String::as_str)
        .ok_or_else(usage)
}

fn usage() -> String {
    "usage: rlogs-bpsr-damage-source-route-proof --surface <DamageFormulaSurface.json> --damage-attr-table <DamageAttrTable.json> --bullet-table <BulletTable.json> --bullet-run-table <BulletRunTable.json> --bullet-shape-table <BulletShapeTable.json> --buff-table <BuffTable.json> --skill-table <SkillTable.json> --skill-effect-table <SkillEffectTable.json> --skill-fight-level-table <SkillFightLevelTable.json> --recount-table <RecountTable.json> --il2cpp-surface <il2cpp-combat-surface.json> --build <numeric-client-build> --output <route-proof.json>".to_owned()
}

fn input_artifact(
    role: &'static str,
    path: &Path,
) -> Result<InputArtifact, Box<dyn std::error::Error>> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut hasher = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
        bytes = bytes.saturating_add(count as u64);
    }
    Ok(InputArtifact {
        role,
        file: path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("external-artifact")
            .to_owned(),
        bytes,
        sha256: format!("{:x}", hasher.finalize()),
    })
}

#[cfg(test)]
mod tests {
    use super::{exact_routes, exact_routes_with_typed_tables, parse_lookup_key, recount_owners};
    use rlogs_game_bpsr::BpsrDamageSourceKind;
    use serde_json::json;

    #[test]
    fn parses_ability_and_hit_lookup_key() {
        assert_eq!(parse_lookup_key("920201:1").unwrap(), (920_201, 1));
        assert!(parse_lookup_key("920201").is_err());
    }

    #[test]
    fn recount_owners_are_exact_but_separate_from_packet_routes() {
        let rows = json!({
            "7": { "Id": 7, "RecountName": "Parent A", "DamageId": [101, 102] },
            "8": { "Id": 8, "RecountName": "Parent B", "DamageId": [102] }
        });
        let owners = recount_owners(rows.as_object().unwrap()).unwrap();

        assert_eq!(owners[&101].len(), 1);
        assert_eq!(owners[&102].len(), 2);
        assert_eq!(owners[&102][0].recount_id, 7);
        assert_eq!(owners[&102][1].recount_name, "Parent B");
    }

    #[test]
    fn resolves_conflicting_skill_and_bullet_rows_by_exact_table_construction() {
        let bullets = json!({
            "920201": { "Id": 920201, "BulletAttrId": 3920201 }
        });
        let skills = json!({
            "920201": { "Id": 920201, "EffectIDs": [92020101] }
        });
        let effects = json!({
            "92020101": { "Id": 92020101, "SkillId": 920200 }
        });
        let buffs = json!({});
        let bullet_routes = exact_routes(
            920_201,
            1,
            392_020_101,
            bullets.as_object().unwrap(),
            buffs.as_object().unwrap(),
            skills.as_object().unwrap(),
            effects.as_object().unwrap(),
            buffs.as_object().unwrap(),
        )
        .unwrap();
        let skill_routes = exact_routes(
            920_201,
            1,
            19_202_010_101,
            bullets.as_object().unwrap(),
            buffs.as_object().unwrap(),
            skills.as_object().unwrap(),
            effects.as_object().unwrap(),
            buffs.as_object().unwrap(),
        )
        .unwrap();

        assert_eq!(bullet_routes.len(), 1);
        assert_eq!(bullet_routes[0].damage_source, BpsrDamageSourceKind::Bullet);
        assert_eq!(skill_routes.len(), 1);
        assert_eq!(skill_routes[0].damage_source, BpsrDamageSourceKind::Skill);
    }

    #[test]
    fn resolves_bullet_rows_whose_attr_id_is_already_the_complete_damage_id() {
        let bullets = json!({
            "1000001": { "Id": 1000001, "BulletAttrId": 3_100_000_100_u64 }
        });
        let empty = json!({});
        let routes = exact_routes(
            1_000_001,
            0,
            3_100_000_100,
            bullets.as_object().unwrap(),
            empty.as_object().unwrap(),
            empty.as_object().unwrap(),
            empty.as_object().unwrap(),
            empty.as_object().unwrap(),
        )
        .unwrap();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].damage_source, BpsrDamageSourceKind::Bullet);
    }

    #[test]
    fn resolves_indirect_skill_effect_bullets_and_embedded_hit_suffixes() {
        let bullets = json!({
            "170101": { "Id": 170101, "BulletAttrId": 317010100 },
            "304301": { "Id": 304301, "BulletAttrId": 330430101 }
        });
        let skills = json!({
            "1701": { "Id": 1701, "EffectIDs": [170101] },
            "304301": { "Id": 304301, "EffectIDs": [] }
        });
        let empty = json!({});
        let indirect = exact_routes(
            1_701,
            0,
            317_010_100,
            bullets.as_object().unwrap(),
            empty.as_object().unwrap(),
            skills.as_object().unwrap(),
            empty.as_object().unwrap(),
            empty.as_object().unwrap(),
        )
        .unwrap();
        let replaced_suffix = exact_routes(
            304_301,
            0,
            330_430_100,
            bullets.as_object().unwrap(),
            empty.as_object().unwrap(),
            skills.as_object().unwrap(),
            empty.as_object().unwrap(),
            empty.as_object().unwrap(),
        )
        .unwrap();

        assert_eq!(indirect.len(), 1);
        assert_eq!(indirect[0].damage_source, BpsrDamageSourceKind::Bullet);
        assert_eq!(indirect[0].intermediary_id, Some(170_101));
        assert_eq!(replaced_suffix.len(), 1);
        assert_eq!(
            replaced_suffix[0].damage_source,
            BpsrDamageSourceKind::Bullet
        );
    }

    #[test]
    fn resolves_damage_id_that_embeds_a_different_exact_bullet_id() {
        let bullets = json!({
            "10130301": { "Id": 10130301, "BulletAttrId": 3_101_303_010_0_u64 },
            "10130401": { "Id": 10130401, "BulletAttrId": 3_101_304_010_0_u64 }
        });
        let empty = json!({});
        let routes = exact_routes(
            10_130_401,
            0,
            31_013_030_100,
            bullets.as_object().unwrap(),
            empty.as_object().unwrap(),
            empty.as_object().unwrap(),
            empty.as_object().unwrap(),
            empty.as_object().unwrap(),
        )
        .unwrap();

        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].damage_source, BpsrDamageSourceKind::Bullet);
        assert_eq!(routes[0].owner_table, "DamageAttrTable.Id -> BulletTable");
        assert_eq!(routes[0].intermediary_id, Some(10_130_301));
    }

    #[test]
    fn resolves_exact_bullet_id_encoding_when_bullet_attr_is_stale() {
        let bullets = json!({
            "11010301": { "Id": 11010301, "BulletAttrId": 3_401_000_100_u64 }
        });
        let empty = json!({});
        let routes = exact_routes(
            11_010_301,
            1,
            31_101_030_101,
            bullets.as_object().unwrap(),
            empty.as_object().unwrap(),
            empty.as_object().unwrap(),
            empty.as_object().unwrap(),
            empty.as_object().unwrap(),
        )
        .unwrap();

        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].damage_source, BpsrDamageSourceKind::Bullet);
        assert_eq!(routes[0].owner_table, "BulletTable");
        assert_eq!(routes[0].intermediary_id, Some(11_010_301));
    }

    #[test]
    fn retains_exact_skill_table_routes_when_the_referenced_effect_is_server_only() {
        let empty = json!({});
        let skills = json!({
            "301537": { "Id": 301537, "EffectIDs": [30153701] }
        });
        let routes = exact_routes(
            301_537,
            2,
            13_015_370_102,
            empty.as_object().unwrap(),
            empty.as_object().unwrap(),
            skills.as_object().unwrap(),
            empty.as_object().unwrap(),
            empty.as_object().unwrap(),
        )
        .unwrap();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].damage_source, BpsrDamageSourceKind::Skill);
        assert_eq!(routes[0].owner_table, "SkillTable.EffectIDs");
    }

    #[test]
    fn resolves_buff_damage_from_exact_buff_id_construction() {
        let empty = json!({});
        let buffs = json!({ "55240": { "Id": 55240 } });
        let routes = exact_routes(
            55_240,
            3,
            25_524_003,
            empty.as_object().unwrap(),
            buffs.as_object().unwrap(),
            empty.as_object().unwrap(),
            empty.as_object().unwrap(),
            empty.as_object().unwrap(),
        )
        .unwrap();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].damage_source, BpsrDamageSourceKind::Buff);
    }

    #[test]
    fn resolves_triggering_skill_damage_through_embedded_existing_buff_id() {
        let empty = json!({});
        let buffs = json!({ "2203311": { "Id": 2203311 } });
        let routes = exact_routes(
            220_108,
            1,
            2_220_331_101,
            empty.as_object().unwrap(),
            buffs.as_object().unwrap(),
            empty.as_object().unwrap(),
            empty.as_object().unwrap(),
            empty.as_object().unwrap(),
        )
        .unwrap();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].damage_source, BpsrDamageSourceKind::Buff);
        assert_eq!(routes[0].owner_id, 220_108);
        assert_eq!(routes[0].intermediary_id, Some(2_203_311));
    }

    #[test]
    fn does_not_guess_unmatched_or_out_of_range_routes() {
        let empty = json!({});
        assert!(
            exact_routes(
                920_201,
                100,
                1,
                empty.as_object().unwrap(),
                empty.as_object().unwrap(),
                empty.as_object().unwrap(),
                empty.as_object().unwrap(),
                empty.as_object().unwrap(),
            )
            .unwrap()
            .is_empty()
        );
    }

    #[test]
    fn resolves_skill_fight_level_effect_to_exact_bullet_damage() {
        let bullets = json!({
            "1000101": { "Id": 1000101, "BulletAttrId": 3_100_010_100_u64 }
        });
        let levels = json!({
            "1000101": { "Id": 1000101, "SkillId": 10001, "SkillEffectId": 1000101 }
        });
        let empty = json!({});
        let routes = exact_routes(
            10_001,
            0,
            3_100_010_100,
            bullets.as_object().unwrap(),
            empty.as_object().unwrap(),
            empty.as_object().unwrap(),
            empty.as_object().unwrap(),
            levels.as_object().unwrap(),
        )
        .unwrap();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].damage_source, BpsrDamageSourceKind::Bullet);
        assert_eq!(routes[0].intermediary_id, Some(1_000_101));
        assert_eq!(
            routes[0].owner_table,
            "SkillFightLevelTable.SkillId -> SkillEffectId -> BulletTable"
        );
    }

    #[test]
    fn resolves_nonstandard_buff_damage_from_exact_damage_type_enum() {
        let empty = json!({});
        let buffs = json!({ "881622": { "Id": 881622 } });
        let damage_attrs = json!({
            "11702590101": {
                "Id": 11702590101_u64,
                "DamageType": 2,
                "TypeEnum": 881622
            }
        });
        let routes = exact_routes_with_typed_tables(
            881_622,
            1,
            11_702_590_101,
            empty.as_object().unwrap(),
            buffs.as_object().unwrap(),
            empty.as_object().unwrap(),
            empty.as_object().unwrap(),
            empty.as_object().unwrap(),
            damage_attrs.as_object().unwrap(),
            empty.as_object().unwrap(),
            empty.as_object().unwrap(),
        )
        .unwrap();

        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].damage_source, BpsrDamageSourceKind::Buff);
        assert_eq!(
            routes[0].owner_table,
            "DamageAttrTable.TypeEnum -> BuffTable"
        );
    }

    #[test]
    fn resolves_server_projectile_from_effect_run_and_shape_tables() {
        let empty = json!({});
        let effects = json!({
            "11102401": { "Id": 11102401, "SkillId": 111024 }
        });
        let damage_attrs = json!({
            "31110240100": {
                "Id": 31110240100_u64,
                "DamageType": 3,
                "TypeEnum": 11102401
            }
        });
        let bullet_runs = json!({ "1110240100": { "Id": 1110240100 } });
        let bullet_shapes = json!({ "1110240100": { "Id": 1110240100 } });
        let routes = exact_routes_with_typed_tables(
            11_102_401,
            0,
            31_110_240_100,
            empty.as_object().unwrap(),
            empty.as_object().unwrap(),
            empty.as_object().unwrap(),
            effects.as_object().unwrap(),
            empty.as_object().unwrap(),
            damage_attrs.as_object().unwrap(),
            bullet_runs.as_object().unwrap(),
            bullet_shapes.as_object().unwrap(),
        )
        .unwrap();

        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].damage_source, BpsrDamageSourceKind::Bullet);
        assert_eq!(
            routes[0].owner_table,
            "DamageAttrTable.TypeEnum -> SkillEffectTable + BulletRunTable + BulletShapeTable"
        );
    }
}
