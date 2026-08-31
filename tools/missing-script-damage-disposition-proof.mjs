#!/usr/bin/env node

import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const options = parseArgs(process.argv.slice(2));
const paths = Object.fromEntries([
  "formulaExecutionProof",
  "damageLedger",
  "packetDamageScope",
  "bulletTable",
  "skillTable",
  "skillEffectTable",
  "factorClosure",
  "statusOrigins",
].map((key) => [key, resolvePath(options[key])]));
const outputPath = resolvePath(options.output);

const execution = readJson(paths.formulaExecutionProof, "formula execution proof");
const ledger = readJson(paths.damageLedger, "damage resolution ledger");
const scope = readJson(paths.packetDamageScope, "packet-observed damage scope");
const bullets = readJson(paths.bulletTable, "BulletTable");
const skills = readJson(paths.skillTable, "SkillTable");
const skillEffects = readJson(paths.skillEffectTable, "SkillEffectTable");
const factorClosure = readJson(paths.factorClosure, "Psychoscope factor closure");
const statusOrigins = readJson(paths.statusOrigins, "observed status origins");

assertBuild(execution.game_build, options.gameBuild, "formula execution proof game build");
assertBuild(execution.packet_build, options.packetBuild, "formula execution proof packet build");
assertBuild(ledger.game_build, options.gameBuild, "damage ledger game build");
assertBuild(scope.packet_build, options.packetBuild, "packet damage scope packet build");
assertBuild(factorClosure.game_build, options.gameBuild, "factor closure game build");
assertBuild(statusOrigins.game_build, options.packetBuild, "status origins packet build");

const exactLookupKeys = new Set((scope.exact_lookup_keys || []).map(String));
const ledgerByDamageId = new Map(
  (ledger.entries || []).map((row) => [Number(row.damage_attr_id), row]),
);
const executionByDamageId = new Map(
  (execution.observed_damage_rows || []).map((row) => [Number(row.damage_id), row]),
);

const eyeballLaser = requireLedgerRow(3100970200, "1009702:0");
const eyeballBullet = bullets["1009702"];
assert(eyeballBullet, "BulletTable row 1009702 is missing");
assert(Number(eyeballBullet.BulletAttrId) === 3100970200,
  "BulletTable row 1009702 no longer selects damage row 3100970200");
assert(exactLookupKeys.has("1009702:2"), "packet scope lacks executed 1009702:2 sibling");
assert(!exactLookupKeys.has("1009702:0"), "1009702:0 executed and must be formula-proven");
assert(!executionByDamageId.has(3100970200), "damage row 3100970200 executed and must be formula-proven");
const eyeballSibling = requireExecutionRow(110097020102, "AutoAttack");
assertNormalValueConservation(eyeballSibling, "damage row 110097020102");

const judgmentSecondary = requireLedgerRow(124100104, "2410:4");
const judgmentSkill = skills["2410"];
const judgmentEffect = skillEffects["241001"];
assert(judgmentSkill, "SkillTable row 2410 is missing");
assert(judgmentEffect, "SkillEffectTable row 241001 is missing");
assert((judgmentSkill.EffectIDs || []).map(Number).includes(241001),
  "SkillTable row 2410 no longer selects SkillEffectTable row 241001");
const judgmentDisplay = JSON.stringify(judgmentEffect.SkillAttrDes || []);
assert(judgmentDisplay.includes("124100101"),
  "Judgment display no longer declares damage row 124100101");
assert(!judgmentDisplay.includes("124100104"),
  "Judgment display now declares damage row 124100104; classification must reopen");
assert(exactLookupKeys.has("2410:1"), "packet scope lacks executed Judgment row 2410:1");
assert(!exactLookupKeys.has("2410:4"), "Judgment row 2410:4 executed and must be formula-proven");
assert(!executionByDamageId.has(124100104), "damage row 124100104 executed and must be formula-proven");
const judgmentSibling = requireExecutionRow(124100101, "Attack");
assertNormalValueConservation(judgmentSibling, "damage row 124100101");

const sacredBlade = requireLedgerRow(2305444006, "3054440:6");
assert(exactLookupKeys.has("3054440:6"), "packet scope lacks Sacred Blade row 3054440:6");
const sacredBladeExecution = requireExecutionRow(2305444006, "<missing>");
assertNormalValueConservation(sacredBladeExecution, "damage row 2305444006");
const sacredBladeFamily = (factorClosure.families || []).find((family) =>
  (family.source_buff_ids || []).map(Number).includes(3054440)
    && (family.direct_damage_ids || []).map(Number).includes(2305444006));
assert(sacredBladeFamily, "Shield Knight Reality Factor X5 closure is missing");
assert(sacredBladeFamily.offline_route_state === "exact-static-output-route",
  "Shield Knight Reality Factor X5 lacks an exact static output route");
assert((sacredBladeFamily.exact_recount_ids || []).map(Number).includes(151),
  "Sacred Blade exact recount parent 151 is missing");
const gradeRoutes = [...(sacredBladeFamily.grade_routes || [])]
  .sort((left, right) => Number(left.grade) - Number(right.grade));
assert(gradeRoutes.length === 10, "Sacred Blade does not expose all ten factor grades");
const expectedBasisPoints = [200, 331, 462, 593, 724, 855, 986, 1117, 1248, 1380];
const expectedEnergyThresholds = [450, 416, 382, 348, 314, 280, 246, 212, 178, 144];
for (let index = 0; index < gradeRoutes.length; index += 1) {
  const route = gradeRoutes[index];
  assert(Number(route.grade) === index + 1, `Sacred Blade grade ${index + 1} is missing`);
  const mechanicParameters = (route.parameter_values || []).slice(0, 2).map(Number);
  assert(mechanicParameters.includes(expectedBasisPoints[index]),
    `Sacred Blade grade ${index + 1} HP basis points changed`);
  assert(mechanicParameters.includes(expectedEnergyThresholds[index]),
    `Sacred Blade grade ${index + 1} energy threshold changed`);
  assert(Number(route.parameter_values?.[2]) === 3000,
    `Sacred Blade grade ${index + 1} cooldown changed`);
}
const sacredBladeOrigin = (statusOrigins.effects || []).find(
  (row) => Number(row.effect_id) === 3054440,
);
assert(sacredBladeOrigin, "status origin evidence for effect 3054440 is missing");
assert(Number(sacredBladeOrigin.window_count || 0) > 0,
  "effect 3054440 lacks a packet-observed status window");
assert(Number(sacredBladeOrigin.cross_actor_window_count || 0) === 0,
  "effect 3054440 now has a cross-actor status window");
assert(Number(sacredBladeOrigin.source_player_window_count || 0)
  === Number(sacredBladeOrigin.window_count || 0),
  "effect 3054440 is not exclusively player-sourced in observed windows");
assert(Number(sacredBladeOrigin.target_player_window_count || 0)
  === Number(sacredBladeOrigin.window_count || 0),
  "effect 3054440 is not exclusively player-targeted in observed windows");

const rows = [
  {
    ...identity(eyeballLaser),
    disposition: "retained-unexecuted-bullet-definition",
    proof_state: "offline-classification-complete",
    packet_execution_state: "exact-lookup-key-absent",
    retained_as: "current-build bullet definition and incoming-route candidate",
    sibling_execution: executionIdentity(eyeballSibling),
    static_evidence: {
      table: "BulletTable",
      row_id: 1009702,
      bullet_attr_id: 3100970200,
      hit_camp_type: (eyeballBullet.HitCampType || []).map(Number),
    },
  },
  {
    ...identity(judgmentSecondary),
    disposition: "retained-unexecuted-secondary-skill-definition",
    proof_state: "offline-classification-complete",
    packet_execution_state: "exact-lookup-key-absent",
    retained_as: "current-build secondary Judgment definition",
    sibling_execution: executionIdentity(judgmentSibling),
    static_evidence: {
      skill_id: 2410,
      skill_effect_id: 241001,
      displayed_damage_attr_ids: [124100101],
      displayed_healing_formula: "lost-HP effect",
      omitted_secondary_damage_attr_id: 124100104,
    },
  },
  {
    ...identity(sacredBlade),
    disposition: "executed-source-owned-current-hp-factor-output",
    proof_state: "offline-formula-boundary-complete",
    packet_execution_state: "exact-normal-value-output",
    packet_execution: executionIdentity(sacredBladeExecution),
    source_contract: {
      source_buff_id: 3054440,
      factor_family_id: Number(sacredBladeFamily.family_id),
      class_gate_ids: (sacredBladeFamily.class_gate_ids || []).map(Number),
      formula_input: "owner current HP at trigger time",
      pre_shared_pipeline_formula: "floor(current_hp * grade_basis_points / 10000)",
      grade_basis_points: expectedBasisPoints,
      grade_energy_thresholds: expectedEnergyThresholds,
      raw_grade_parameter_pairs: gradeRoutes.map((route) =>
        (route.parameter_values || []).slice(0, 2).map(Number)),
      cooldown_milliseconds: 3000,
      exact_recount_parent_ids: [151],
      provider_scope: "self-source-only",
      external_rdps_transfer_created_by_source: false,
    },
    delegated_counterfactuals: [
      "target-physical-armor-counterfactual",
      "elemental-resistance-counterfactual",
      "primary-stat-to-attack-transform",
      "mastery-property-transform",
    ],
    final_validation_obligations: sacredBladeFamily.final_validation_obligations || [],
  },
];

const result = {
  schema_version: 1,
  generated_by: "tools/missing-script-damage-disposition-proof.mjs",
  game: "blue-protocol-star-resonance",
  game_build: String(options.gameBuild),
  packet_build: String(options.packetBuild),
  policy: {
    unresolved_evidence_is_hidden: false,
    unexecuted_rows_remain_cataloged: true,
    newly_observed_exact_lookup_key_reopens_gate: true,
    executed_packet_output_is_retained: true,
    packet_normal_value_is_authoritative_final_component: true,
    factor_base_formula_and_shared_damage_transforms_are_separate: true,
    external_provider_rdps_is_not_inferred_from_self_owned_factor_damage: true,
  },
  inputs: Object.fromEntries(Object.entries(paths).map(([key, value]) => [key, relative(value)])),
  summary: {
    proof_state: "offline-missing-script-classification-complete",
    classified_rows: rows.length,
    retained_unexecuted_definitions: 2,
    executed_source_owned_outputs: 1,
    packet_damage_results_conserved: Number(sacredBladeExecution.packet_damage_results || 0),
    remaining_missing_script_rows: 0,
  },
  rows,
};

writeFileSync(outputPath, `${JSON.stringify(result, null, 2)}\n`, "utf8");
console.log(JSON.stringify(result.summary, null, 2));

function requireLedgerRow(damageAttrId, lookupKey) {
  const row = ledgerByDamageId.get(damageAttrId);
  assert(row, `damage ledger row ${damageAttrId} is missing`);
  assert(String(row.lookup_key) === lookupKey,
    `damage row ${damageAttrId} lookup key changed from ${lookupKey}`);
  assert(row.formula?.state === "nonstandard-or-missing",
    `damage row ${damageAttrId} no longer requires missing-script classification`);
  assert(String(row.formula?.family || "<missing>") === "<missing>",
    `damage row ${damageAttrId} now has a named executor family`);
  return row;
}

function requireExecutionRow(damageAttrId, family) {
  const row = executionByDamageId.get(damageAttrId);
  assert(row, `packet execution row ${damageAttrId} is missing`);
  assert(String(row.damage_script) === family,
    `packet execution row ${damageAttrId} family changed from ${family}`);
  return row;
}

function assertNormalValueConservation(row, label) {
  const results = Number(row.packet_damage_results || 0);
  const shape = row.packet_damage_value_shape || {};
  assert(results > 0, `${label} has no packet damage results`);
  assert(Number(shape.results || 0) === results, `${label} result count does not conserve`);
  assert(Number(shape.amount_nonzero || 0) === results, `${label} contains zero outputs`);
  assert(Number(shape.with_normal_value || 0) === results, `${label} lacks normal_value outputs`);
  assert(Number(shape.with_both_values || 0) === 0, `${label} mixes normal and lucky components`);
  assert(Number(shape.without_component_value || 0) === 0, `${label} has componentless output`);
  assert(Number(shape.amount_matches_normal_value || 0) === results,
    `${label} canonical amount differs from normal_value`);
}

function identity(row) {
  return {
    damage_attr_id: Number(row.damage_attr_id),
    lookup_key: String(row.lookup_key),
    ability_id: Number(row.ability_id),
    hit_event_id: Number(row.hit_event_id),
    formula_signature_id: String(row.formula?.formula_signature_id || ""),
  };
}

function executionIdentity(row) {
  return {
    damage_attr_id: Number(row.damage_id),
    formula_family: String(row.damage_script),
    packet_damage_results: Number(row.packet_damage_results || 0),
    packet_damage_value_shape: row.packet_damage_value_shape,
    packet_damage_source_actor_kinds: row.packet_damage_source_actor_kinds || {},
    packet_damage_target_actor_kinds: row.packet_damage_target_actor_kinds || {},
  };
}

function readJson(filePath, label) {
  try {
    return JSON.parse(readFileSync(filePath, "utf8"));
  } catch (error) {
    throw new Error(`failed to read ${label} ${filePath}: ${error.message}`);
  }
}

function assertBuild(actual, expected, label) {
  assert(String(actual) === String(expected),
    `${label} ${actual} differs from requested build ${expected}`);
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index]?.replace(/^--/, "");
    const value = args[index + 1];
    if (!key || value === undefined) throw new Error(`invalid argument near ${args[index]}`);
    parsed[key] = value;
  }
  for (const required of [
    "gameBuild", "packetBuild", "formulaExecutionProof", "damageLedger",
    "packetDamageScope", "bulletTable", "skillTable", "skillEffectTable",
    "factorClosure", "statusOrigins", "output",
  ]) {
    if (!parsed[required]) throw new Error(`missing --${required}`);
  }
  return parsed;
}

function resolvePath(value) {
  return path.isAbsolute(value) ? value : path.resolve(repoRoot, value);
}

function relative(filePath) {
  return path.relative(repoRoot, filePath).replaceAll("\\", "/");
}
