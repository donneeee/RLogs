#!/usr/bin/env node

import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const options = parseArgs(process.argv.slice(2));
const executionPath = resolvePath(options.formulaExecutionProof);
const ledgerPath = resolvePath(options.damageLedger);
const fixedPointPath = resolvePath(options.fixedPointObservation);
const parentPath = resolvePath(options.parentObservation);
const chancePath = resolvePath(options.chanceObservation);
const combinedChancePath = resolvePath(options.combinedChanceObservation);
const outputPath = resolvePath(options.output);

const execution = readJson(executionPath, "formula execution proof");
const ledger = readJson(ledgerPath, "damage resolution ledger");
const fixedPoint = readJson(fixedPointPath, "fixed-point observation");
const parent = readJson(parentPath, "Lucky parent observation");
const chance = readJson(chancePath, "Lucky chance observation");
const combinedChance = readJson(combinedChancePath, "combined chance observation");

assertBuild(execution.game_build, options.gameBuild, "formula execution proof game build");
assertBuild(execution.packet_build, options.packetBuild, "formula execution proof packet build");
assertBuild(ledger.game_build, options.gameBuild, "damage ledger game build");
assertBuild(String(fixedPoint.client_build || "").replace(/^steam-/, ""), options.packetBuild, "fixed-point observation packet build");
assertBuild(parent.client_build, options.packetBuild, "Lucky parent observation packet build");
assertBuild(chance.game_build, options.packetBuild, "Lucky chance observation packet build");
assertBuild(combinedChance.client_build, options.packetBuild, "combined chance observation packet build");

const familyNames = new Set(["AttackLucky", "MAttackLucky"]);
const ledgerRows = (ledger.entries || []).filter((entry) => {
  const family = entry.formula?.family || entry.formula?.candidate?.damage_script;
  return familyNames.has(String(family));
});
const ledgerRowsByDamageId = new Map(ledgerRows.map((entry) => [String(entry.damage_attr_id), entry]));
const rows = [];

for (const row of execution.observed_damage_rows || []) {
  const family = String(row.damage_script || "<missing>");
  if (!familyNames.has(family) || Number(row.packet_damage_results || 0) <= 0) continue;
  const ledgerRow = ledgerRowsByDamageId.get(String(row.damage_id));
  if (!ledgerRow) {
    throw new Error(`packet-observed ${family} row ${row.damage_id} is absent from the current-build ledger`);
  }
  const shape = valueShape(row.packet_damage_value_shape);
  if (shape.results !== Number(row.packet_damage_results)) {
    throw new Error(`row ${row.damage_id} value-shape results do not conserve packet damage results`);
  }
  if (shape.with_normal_value !== 0 || shape.with_both_values !== 0) {
    throw new Error(`row ${row.damage_id} contains a normal component and is not a dedicated Lucky output`);
  }
  if (shape.with_lucky_value > 0 && shape.amount_matches_lucky_value !== shape.with_lucky_value) {
    throw new Error(`row ${row.damage_id} has a packet lucky_value that does not equal canonical amount`);
  }
  if (shape.with_lucky_value === 0 && shape.lucky_flag_true === 0) {
    throw new Error(`row ${row.damage_id} has neither an explicit Lucky component nor a Lucky outcome witness`);
  }
  rows.push({
    damage_attr_id: Number(row.damage_id),
    lookup_key: String(ledgerRow.lookup_key),
    ability_id: Number(ledgerRow.ability_id),
    hit_event_id: Number(ledgerRow.hit_event_id),
    formula_family: family,
    formula_signature_id: String(ledgerRow.formula?.formula_signature_id || ""),
    packet_damage_results: Number(row.packet_damage_results),
    packet_damage_value_shape: shape,
    packet_damage_source_actor_kinds: countMap(row.packet_damage_source_actor_kinds),
    packet_damage_target_actor_kinds: countMap(row.packet_damage_target_actor_kinds),
    executor_boundary: "server-authored-dedicated-lucky-component",
    counterfactual_input: "canonical packet damage amount",
  });
}
rows.sort((left, right) => left.damage_attr_id - right.damage_attr_id);

const luckyMultiplier = (fixedPoint.proven_components || []).find(
  (component) => Number(component.attribute_id) === 12530,
);
if (luckyMultiplier?.status !== "packet-proven") {
  throw new Error("fixed-point observation does not packet-prove attribute 12530");
}
if (luckyMultiplier.formula_when_lucky !== "lucky_factor = lucky_damage_raw / 10000") {
  throw new Error("fixed-point observation has an unexpected Lucky multiplier convention");
}
const accounting = luckyMultiplier.exact_rdps_accounting_definition || {};
if (accounting.representation !== "reduced rational numerator and denominator"
  || Number(accounting.events || 0) <= 0) {
  throw new Error("fixed-point observation lacks conserved rational Lucky accounting evidence");
}
if (Number(parent.parent_packet_rule?.unresolved_parent_events ?? -1) !== 0) {
  throw new Error("Lucky parent observation retains unresolved triggering parents");
}
const luckyOccurrence = (chance.exact_accounting_components || []).find(
  (component) => component.component === "lucky_occurrence_row",
);
if (luckyOccurrence?.proof_state !== "exact_conserved_accounting_component") {
  throw new Error("Lucky chance observation lacks an exact occurrence accounting component");
}
if (Number(combinedChance.packet_component_identity?.amount_matches_lucky_value || 0)
  !== Number(combinedChance.packet_component_identity?.eligible_combined_candidates || -1)) {
  throw new Error("combined Critical-plus-Lucky observation does not conserve packet Lucky components");
}

const families = [...familyNames].sort().map((family) => {
  const familyRows = rows.filter((row) => row.formula_family === family);
  if (familyRows.length === 0) throw new Error(`${family} has no packet-observed exact row witness`);
  const explicitLuckyRows = familyRows.filter(
    (row) => row.packet_damage_value_shape.with_lucky_value > 0,
  );
  if (explicitLuckyRows.length === 0) {
    throw new Error(`${family} has no row with an explicit packet lucky_value witness`);
  }
  return {
    formula_family: family,
    proof_state: "offline-rdps-executor-complete",
    executor_boundary: "server-authored-dedicated-lucky-component",
    packet_damage_results: sum(familyRows.map((row) => row.packet_damage_results)),
    explicit_lucky_value_results: sum(
      familyRows.map((row) => row.packet_damage_value_shape.with_lucky_value),
    ),
    explicit_lucky_value_exact_matches: sum(
      familyRows.map((row) => row.packet_damage_value_shape.amount_matches_lucky_value),
    ),
    current_build_damage_attr_ids: familyRows.map((row) => row.damage_attr_id),
    current_build_formula_signature_ids: unique(
      familyRows.map((row) => row.formula_signature_id).filter(Boolean),
    ),
    current_build_lookup_keys: unique(familyRows.map((row) => row.lookup_key)),
    exact_rdps_counterfactuals: [
      "Lucky occurrence probability share",
      "Critical-plus-Lucky joint occurrence share",
      "Lucky damage multiplier share",
    ],
  };
});

const totalResults = sum(rows.map((row) => row.packet_damage_results));
const explicitLuckyResults = sum(rows.map((row) => row.packet_damage_value_shape.with_lucky_value));
const explicitLuckyMatches = sum(rows.map((row) => row.packet_damage_value_shape.amount_matches_lucky_value));
const result = {
  schema_version: 1,
  generated_by: "tools/lucky-damage-executor-proof.mjs",
  game: "blue-protocol-star-resonance",
  game_build: String(options.gameBuild),
  packet_build: String(options.packetBuild),
  policy: {
    runtime_formula_authority: true,
    packet_amount_is_authoritative_lucky_component: true,
    hidden_randomized_pre_lucky_base_is_reconstructed: false,
    hidden_randomized_pre_lucky_base_is_required_for_rdps: false,
    complete_hit_is_replaced: false,
    triggering_parent_relation_is_retained: true,
    exact_counterfactual_representation: "reduced rational",
    unresolved_evidence_is_hidden: false,
    current_build_row_and_signature_coverage_required_by_gate: true,
  },
  inputs: {
    formula_execution_proof: relative(executionPath),
    damage_resolution_ledger: relative(ledgerPath),
    fixed_point_observation: relative(fixedPointPath),
    parent_observation: relative(parentPath),
    lucky_chance_observation: relative(chancePath),
    combined_chance_observation: relative(combinedChancePath),
  },
  summary: {
    proof_state: "offline-rdps-executor-complete",
    formula_families: families.length,
    exact_packet_rows: rows.length,
    packet_damage_results: totalResults,
    packet_results_with_normal_value: sum(
      rows.map((row) => row.packet_damage_value_shape.with_normal_value),
    ),
    explicit_lucky_value_results: explicitLuckyResults,
    explicit_lucky_value_exact_matches: explicitLuckyMatches,
    explicit_lucky_value_conservation: explicitLuckyMatches === explicitLuckyResults,
    parent_relations_proven: Number(parent.parent_packet_rule?.lucky_events || 0),
    parent_relations_unresolved: Number(parent.parent_packet_rule?.unresolved_parent_events || 0),
    exact_multiplier_counterfactual_events: Number(accounting.events || 0),
    combined_occurrence_packet_components_proven: Number(
      combinedChance.packet_component_identity?.eligible_combined_candidates || 0,
    ),
  },
  executor_contract: {
    input: "canonical packet damage amount from an exact AttackLucky or MAttackLucky DamageAttr row",
    occurrence_counterfactual: String(luckyOccurrence.formula),
    combined_occurrence_counterfactual: String(combinedChance.exact_joint_counterfactual?.formula),
    multiplier_counterfactual: String(accounting.formula),
    output: "one conserved reduced-rational provider gain and identical recipient subtraction per proven external component",
    integer_boundary: String(accounting.integer_boundary),
  },
  families,
  rows,
};

writeFileSync(outputPath, `${JSON.stringify(result, null, 2)}\n`, "utf8");
console.log(JSON.stringify(result.summary, null, 2));

function valueShape(value) {
  return Object.fromEntries([
    "results",
    "with_normal_value",
    "with_lucky_value",
    "with_both_values",
    "amount_matches_normal_value",
    "amount_matches_lucky_value",
    "amount_matches_normal_plus_lucky",
    "lucky_flag_true",
    "causes_lucky_true",
  ].map((key) => [key, positiveInteger(value?.[key])]));
}

function countMap(value) {
  return Object.fromEntries(Object.entries(value || {})
    .map(([key, count]) => [String(key), positiveInteger(count)])
    .filter(([, count]) => count > 0)
    .sort(([left], [right]) => left.localeCompare(right)));
}

function positiveInteger(value) {
  const number = Number(value || 0);
  if (!Number.isSafeInteger(number) || number < 0) throw new Error(`invalid count ${value}`);
  return number;
}

function unique(values) {
  return [...new Set(values)].sort();
}

function sum(values) {
  return values.reduce((total, value) => total + Number(value || 0), 0);
}

function assertBuild(actual, expected, label) {
  if (String(actual) !== String(expected)) {
    throw new Error(`${label} ${actual} differs from expected ${expected}`);
  }
}

function readJson(filePath, label) {
  try {
    return JSON.parse(readFileSync(filePath, "utf8"));
  } catch (error) {
    throw new Error(`failed to read ${label} at ${filePath}: ${error.message}`);
  }
}

function parseArgs(args) {
  const result = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index];
    const value = args[index + 1];
    if (!key?.startsWith("--") || value === undefined) {
      throw new Error(`expected --key value arguments; received ${args.join(" ")}`);
    }
    result[key.slice(2)] = value;
  }
  for (const required of [
    "gameBuild", "packetBuild", "formulaExecutionProof", "damageLedger",
    "fixedPointObservation", "parentObservation", "chanceObservation",
    "combinedChanceObservation", "output",
  ]) {
    if (!result[required]) throw new Error(`missing --${required}`);
  }
  return result;
}

function resolvePath(value) {
  return path.isAbsolute(value) ? value : path.resolve(repoRoot, value);
}

function relative(value) {
  return path.relative(repoRoot, value).replaceAll("\\", "/");
}
