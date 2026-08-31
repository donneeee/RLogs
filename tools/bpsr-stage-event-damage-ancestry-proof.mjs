#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 2;
const EXPECTED_BUILD = "24687926";
const EXPECTED_EFFECT_ID = 31_602;

function fail(message) { throw new Error(message); }
function take(values, flag) {
  const index = values.indexOf(flag);
  if (index < 0 || index + 1 >= values.length) fail(`${flag} requires a value`);
  const value = values[index + 1];
  values.splice(index, 2);
  return value;
}
function parseArgs(argv) {
  const values = [...argv];
  const mode = values.shift();
  if (mode === "verify") {
    const input = path.resolve(take(values, "--input"));
    if (values.length) fail(`unknown arguments: ${values.join(" ")}`);
    return { mode, input };
  }
  if (mode !== "analyze") fail(usage());
  const result = {
    mode,
    build: take(values, "--build"),
    damageJoin: path.resolve(take(values, "--damage-join")),
    stageCatalog: path.resolve(take(values, "--stage-catalog")),
    skillTable: path.resolve(take(values, "--skill-table")),
    buffTable: path.resolve(take(values, "--buff-table")),
    output: path.resolve(take(values, "--output")),
  };
  if (values.length) fail(`unknown arguments: ${values.join(" ")}`);
  return result;
}
function usage() {
  return "usage: analyze --build 24687926 --damage-join <json> --stage-catalog <json> --skill-table <json> --buff-table <json> --output <json> | verify --input <json>";
}
function sha256(bytes) { return createHash("sha256").update(bytes).digest("hex"); }
function receipt(file, bytes) { return { path: file, bytes: bytes.length, sha256: sha256(bytes) }; }
function parseJson(bytes) {
  const text = bytes.toString("utf8");
  return JSON.parse(text.charCodeAt(0) === 0xfeff ? text.slice(1) : text);
}
function contentHash(report) {
  const copy = structuredClone(report);
  delete copy.content_sha256;
  return sha256(Buffer.from(JSON.stringify(copy)));
}
function addToMapSet(map, key, value) {
  const values = map.get(key) ?? new Set();
  values.add(value);
  map.set(key, values);
}
function stageFamily(type) {
  if (type === 0) return "normal";
  if (type === 1) return "singing";
  if (type === 2) return "charge";
  if (type === 3) return "aim";
  if (type === 4) return "aim_shooting";
  if (type === 8 || type === 9) return "guide";
  return "other";
}
function speedLane(type, atkSpeedSwitch) {
  if (type === 0 && atkSpeedSwitch === true) return "normal_attack_speed_attr_11720_plus_temporary_700";
  if (type === 0 && atkSpeedSwitch === false) return "unaffected_normal_stage_atk_speed_switch_false";
  if (type === 0) return "unresolved_normal_stage_atk_speed_switch";
  if (type === 1) return "singing_native_helper_numeric_boundary_open";
  if (type === 2) return "charge_speed_attr_11740";
  if (type === 8 || type === 9) return "guide_speed_attr_11730_plus_temporary_710";
  return "unaffected_stage_type";
}

function validateInputs(join, catalog, build) {
  if (Number(join?.schema_version) < 6 || join?.game_build !== build ||
      Number(join?.effect_id) !== EXPECTED_EFFECT_ID ||
      join?.policy?.provider_rdps_credit_allowed !== false) {
    fail("damage join is not the current fail-closed effect-31602 frontier");
  }
  if (Number(catalog?.schema_version) < 3 || catalog?.build !== build ||
      catalog?.authority?.exact_build_skill_logic_payload_decoded !== true ||
      catalog?.authority?.provider_rdps_credit_allowed !== false ||
      !Array.isArray(catalog?.event_parameter_rows) ||
      !Array.isArray(catalog?.stage_event_rows)) {
    fail("stage catalog lacks exact current-build stage-event parameters");
  }
}

function buildIndexes(catalog, skillTable) {
  const damageAttrByParameterIndex = new Map();
  for (const parameter of catalog.event_parameter_rows) {
    if (parameter.param_name !== "damageAttrId" || parameter.param_type !== "int") continue;
    const damageAttrId = Number(parameter.param_value);
    if (!Number.isSafeInteger(damageAttrId)) continue;
    damageAttrByParameterIndex.set(Number(parameter.parameter_index), damageAttrId);
  }
  const eventRoutesByDamageAttr = new Map();
  for (const event of catalog.stage_event_rows) {
    if (event.dictionary_kind !== "skill") continue;
    for (const parameterIndex of event.parameter_indexes ?? []) {
      const damageAttrId = damageAttrByParameterIndex.get(Number(parameterIndex));
      if (damageAttrId === undefined) continue;
      const key = `${damageAttrId}:${event.dictionary_key}:${event.event_index}`;
      const routes = eventRoutesByDamageAttr.get(damageAttrId) ?? new Map();
      routes.set(key, {
        skill_logic_key: Number(event.dictionary_key),
        event_index: Number(event.event_index),
        event_name: event.name ?? null,
        parameter_index: Number(parameterIndex),
      });
      eventRoutesByDamageAttr.set(damageAttrId, routes);
    }
  }
  const stagesByKey = new Map();
  for (const stage of catalog.skill_stages ?? []) {
    const rows = stagesByKey.get(Number(stage.dictionary_key)) ?? [];
    rows.push(stage);
    stagesByKey.set(Number(stage.dictionary_key), rows);
  }
  const skillsByEffect = new Map();
  for (const row of Object.values(skillTable)) {
    if (!Number.isSafeInteger(Number(row?.Id))) continue;
    for (const effectId of row.EffectIDs ?? []) addToMapSet(skillsByEffect, Number(effectId), Number(row.Id));
  }
  return { eventRoutesByDamageAttr, stagesByKey, skillsByEffect };
}

function analyzeObservation(action, route, indexes, skillTable, buffTable) {
  const damageAttrIds = [...new Set((route.selected_damage_attr_ids ?? []).map(Number).filter(Number.isSafeInteger))];
  const eventRoutes = damageAttrIds.flatMap((id) => [...(indexes.eventRoutesByDamageAttr.get(id)?.values() ?? [])]);
  const buffRow = buffTable[String(action.action_id)] ?? buffTable[action.action_id] ?? null;
  const explicitBuffSkillId = Number(buffRow?.SkillId) > 0 && Number.isSafeInteger(Number(buffRow.SkillId))
    ? Number(buffRow.SkillId) : null;
  const explicitRootSkill = explicitBuffSkillId === null
    ? null : skillTable[String(explicitBuffSkillId)] ?? skillTable[explicitBuffSkillId] ?? null;
  const explicitSkillLogicKeys = explicitRootSkill
    ? [...new Set((explicitRootSkill.EffectIDs ?? []).map(Number).filter((key) => indexes.stagesByKey.has(key)))]
    : [];
  const eventSkillLogicKeys = [...new Set(eventRoutes.map((row) => row.skill_logic_key))];
  const skillLogicKeys = (explicitSkillLogicKeys.length ? explicitSkillLogicKeys : eventSkillLogicKeys)
    .sort((a, b) => a - b);
  const rootSkillIds = explicitRootSkill
    ? [explicitBuffSkillId]
    : [...new Set(skillLogicKeys.flatMap((key) => [...(indexes.skillsByEffect.get(key) ?? [])]))].sort((a, b) => a - b);
  const rootSkills = rootSkillIds.map((id) => skillTable[String(id)] ?? skillTable[id]).filter(Boolean);
  const allStages = skillLogicKeys.flatMap((key) => indexes.stagesByKey.get(key) ?? []);
  const stageTypes = [...new Set(allStages.map((row) => Number(row.stage_type)))].sort((a, b) => a - b);
  const atkSpeedSwitchValues = [...new Set(rootSkills.map((row) => row.AtkSpeedSwitch === true))];
  const lanes = stageTypes.flatMap((type) => atkSpeedSwitchValues.length
    ? atkSpeedSwitchValues.map((value) => speedLane(type, value))
    : [speedLane(type, null)]);
  const speedLanes = [...new Set(lanes)].sort();
  let resolution = "unresolved-no-static-skill-ancestry-route";
  if (explicitRootSkill && skillLogicKeys.length > 0 && stageTypes.length === 1 && speedLanes.length === 1) {
    resolution = "exact-current-build-buff-table-skill-id-uniform-speed-lane";
  } else if (explicitRootSkill && skillLogicKeys.length === 0) {
    resolution = "unresolved-buff-table-skill-id-without-skill-logic-key";
  } else if (skillLogicKeys.length > 1) resolution = "ambiguous-multiple-skill-stage-event-damage-attr-routes";
  else if (skillLogicKeys.length === 1 && rootSkillIds.length !== 1) resolution = "unresolved-skill-table-effect-owner-count";
  else if (skillLogicKeys.length === 1 && rootSkillIds.length === 1 &&
           (stageTypes.length !== 1 || speedLanes.length !== 1)) {
    resolution = "ambiguous-nonuniform-skill-stage-speed-lanes";
  } else if (skillLogicKeys.length === 1 && rootSkillIds.length === 1 &&
             stageTypes.length === 1 && speedLanes.length === 1) {
    resolution = "exact-current-build-skill-stage-event-damage-attr-uniform-speed-lane";
  }
  return {
    action_id: action.action_id,
    damage_source: route.damage_source,
    damage_memberships: route.damage_memberships,
    selected_damage_attr_ids: damageAttrIds,
    packet_owner_stage: route.owner_stage,
    packet_owner_level: route.owner_level,
    skill_stage_event_routes: eventRoutes.sort((a, b) => a.skill_logic_key - b.skill_logic_key || a.event_index - b.event_index),
    buff_table_skill_id: explicitBuffSkillId,
    candidate_skill_logic_keys: skillLogicKeys,
    candidate_root_skill_ids: rootSkillIds,
    candidate_root_skill_localized_name_evidence: rootSkills.map((row) => row.Name ?? null),
    candidate_stage_types: stageTypes,
    candidate_stage_families: [...new Set(stageTypes.map(stageFamily))].sort(),
    candidate_atk_speed_switch_values: atkSpeedSwitchValues,
    candidate_speed_lanes: speedLanes,
    resolution,
    action_instance_observed: false,
    buff_tick_speed_causality_proven: false,
    provider_rdps_credit_allowed: false,
  };
}

function summarize(rows) {
  const total = rows.reduce((sum, row) => sum + Number(row.damage_memberships), 0);
  const exact = rows.filter((row) => row.resolution.startsWith("exact-")).reduce((sum, row) => sum + Number(row.damage_memberships), 0);
  const ambiguous = rows.filter((row) => row.resolution.startsWith("ambiguous-")).reduce((sum, row) => sum + Number(row.damage_memberships), 0);
  const unresolved = total - exact - ambiguous;
  const bySource = [1, 2].map((damageSource) => {
    const selected = rows.filter((row) => row.damage_source === damageSource);
    return {
      damage_source: damageSource,
      damage_memberships: selected.reduce((sum, row) => sum + Number(row.damage_memberships), 0),
      exact_uniform_speed_lane_memberships: selected.filter((row) => row.resolution.startsWith("exact-"))
        .reduce((sum, row) => sum + Number(row.damage_memberships), 0),
    };
  });
  return {
    exact_packet_selected_buff_memberships: total,
    exact_uniform_skill_speed_lane_memberships: exact,
    ambiguous_skill_speed_lane_memberships: ambiguous,
    unresolved_skill_speed_lane_memberships: unresolved,
    membership_partition_by_damage_source: bySource,
    exact_action_instance_joins: 0,
    provider_rdps_credit_allowed: false,
    runtime_promotion_allowed: false,
    observed_damage_reassigned_to_provider: 0,
  };
}

function validate(report) {
  const summary = report?.summary ?? {};
  if (![1, SCHEMA_VERSION].includes(Number(report?.schema_version)) || report?.game_build !== EXPECTED_BUILD ||
      Number(report?.effect_id) !== EXPECTED_EFFECT_ID ||
      report?.policy?.buff_tick_speed_causality_proven !== false ||
      report?.policy?.provider_rdps_credit_allowed !== false ||
      Number(summary[Number(report.schema_version) >= 2
        ? "exact_packet_selected_buff_memberships"
        : "exact_packet_selected_bullet_or_buff_memberships"]) !==
          Number(summary.exact_uniform_skill_speed_lane_memberships) +
          Number(summary.ambiguous_skill_speed_lane_memberships) +
          Number(summary.unresolved_skill_speed_lane_memberships) ||
      Number(summary.exact_action_instance_joins) !== 0 ||
      summary.provider_rdps_credit_allowed !== false || summary.runtime_promotion_allowed !== false ||
      Number(summary.observed_damage_reassigned_to_provider) !== 0 ||
      report.content_sha256 !== contentHash(report)) {
    fail("stage-event damage ancestry proof is inconsistent or unsafe");
  }
}

function generate(options) {
  if (options.build !== EXPECTED_BUILD) fail(`this proof supports exact build ${EXPECTED_BUILD}`);
  if (existsSync(options.output)) fail(`refusing to overwrite ${options.output}`);
  const joinBytes = readFileSync(options.damageJoin);
  const catalogBytes = readFileSync(options.stageCatalog);
  const skillBytes = readFileSync(options.skillTable);
  const buffBytes = readFileSync(options.buffTable);
  const join = parseJson(joinBytes);
  const catalog = parseJson(catalogBytes);
  const skillTable = parseJson(skillBytes);
  const buffTable = parseJson(buffBytes);
  validateInputs(join, catalog, options.build);
  const indexes = buildIndexes(catalog, skillTable);
  const rows = [];
  for (const action of join.action_candidates ?? []) {
    for (const route of action.packet_damage_route_observations ?? []) {
      if (route.resolution !== "exact-static-route-selected-by-observed-packet-damage-source" ||
          Number(route.damage_source) !== 2) continue;
      rows.push(analyzeObservation(action, route, indexes, skillTable, buffTable));
    }
  }
  rows.sort((a, b) => b.damage_memberships - a.damage_memberships || a.action_id - b.action_id);
  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: "tools/bpsr-stage-event-damage-ancestry-proof.mjs",
    game: "blue-protocol-star-resonance",
    deployment: "global",
    game_build: options.build,
    effect_id: EXPECTED_EFFECT_ID,
    inputs: {
      damage_skill_join: receipt(options.damageJoin, joinBytes),
      stage_logic_catalog: receipt(options.stageCatalog, catalogBytes),
      current_build_skill_table: receipt(options.skillTable, skillBytes),
      current_build_buff_table: receipt(options.buffTable, buffBytes),
    },
    policy: {
      exact_numeric_ids_and_build_are_authoritative: true,
      localized_names_are_evidence_only: true,
      stage_event_damage_attr_foreign_key_may_prove_static_skill_ancestry: true,
      buff_table_skill_id_foreign_key_may_prove_static_skill_ancestry: true,
      stage_event_damage_attr_foreign_key_is_action_instance: false,
      buff_tick_speed_causality_proven: false,
      remote_player_cast_packets_required: false,
      provider_rdps_credit_allowed: false,
    },
    observations: rows,
    summary: summarize(rows),
    blockers: [
      "a static SkillDict stage-event damageAttr route is not a remote action instance or action-time speed snapshot",
      "BuffTable periodic damage timing and its dependence on application opportunities versus independent tick cadence remain unproven",
      "provider-removed opportunity timing, operation order, integer rounding, and conservation remain unproven",
      "current-build protocol-pack identity and required replay gates remain missing",
    ],
  };
  report.content_sha256 = contentHash(report);
  validate(report);
  writeFileSync(options.output, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
  process.stdout.write(`analyzed ${report.summary.exact_packet_selected_buff_memberships} buff memberships; exact uniform lanes=${report.summary.exact_uniform_skill_speed_lane_memberships}; provider credit=false\n`);
}

const options = parseArgs(process.argv.slice(2));
if (options.mode === "verify") {
  const report = parseJson(readFileSync(options.input));
  validate(report);
  const total = report.summary[Number(report.schema_version) >= 2
    ? "exact_packet_selected_buff_memberships"
    : "exact_packet_selected_bullet_or_buff_memberships"];
  process.stdout.write(`verified ${total} memberships; provider credit=false\n`);
} else generate(options);
