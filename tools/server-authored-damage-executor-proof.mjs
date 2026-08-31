#!/usr/bin/env node

import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const options = parseArgs(process.argv.slice(2));
const executionPath = resolvePath(options.formulaExecutionProof);
const ledgerPath = resolvePath(options.damageLedger);
const scopePath = resolvePath(options.packetDamageScope);
const outputPath = resolvePath(options.output);

const execution = readJson(executionPath, "formula execution proof");
const ledger = readJson(ledgerPath, "damage resolution ledger");
const scope = readJson(scopePath, "packet-observed damage scope");

assertBuild(execution.game_build, options.gameBuild, "formula execution proof game build");
assertBuild(execution.packet_build, options.packetBuild, "formula execution proof packet build");
assertBuild(ledger.game_build, options.gameBuild, "damage ledger game build");
assertBuild(scope.packet_build, options.packetBuild, "packet damage scope packet build");

const familyNames = new Set(["AutoAttack", "SpAttack"]);
const observedAbilityIds = new Set(
  (scope.observed_ability_ids || []).map(Number).filter(Number.isSafeInteger),
);
const requiredRows = (ledger.entries || []).filter((entry) => {
  const family = entry.formula?.family || entry.formula?.candidate?.damage_script;
  return observedAbilityIds.has(Number(entry.ability_id))
    && entry.formula?.state === "nonstandard-or-missing"
    && familyNames.has(String(family));
});
const executionRowsByDamageId = new Map(
  (execution.observed_damage_rows || []).map((row) => [String(row.damage_id), row]),
);

const rows = requiredRows.map((ledgerRow) => {
  const damageAttrId = Number(ledgerRow.damage_attr_id);
  const family = String(
    ledgerRow.formula?.family || ledgerRow.formula?.candidate?.damage_script || "",
  );
  const executionRow = executionRowsByDamageId.get(String(damageAttrId));
  if (!executionRow) {
    throw new Error(`${family} row ${damageAttrId} lacks an exact packet execution witness`);
  }
  if (String(executionRow.damage_script) !== family) {
    throw new Error(
      `${family} row ${damageAttrId} execution family is ${executionRow.damage_script}`,
    );
  }
  const packetResults = Number(executionRow.packet_damage_results || 0);
  const shape = valueShape(executionRow.packet_damage_value_shape);
  if (packetResults <= 0 || shape.results !== packetResults) {
    throw new Error(`${family} row ${damageAttrId} does not conserve packet result count`);
  }
  if (shape.amount_nonzero !== packetResults || shape.amount_zero !== 0) {
    throw new Error(`${family} row ${damageAttrId} contains a zero canonical output`);
  }
  if (shape.with_normal_value !== packetResults || shape.with_both_values !== 0) {
    throw new Error(`${family} row ${damageAttrId} lacks an exclusive normal_value component`);
  }
  if (shape.amount_matches_normal_value !== packetResults) {
    throw new Error(`${family} row ${damageAttrId} canonical amount differs from normal_value`);
  }
  if (shape.without_component_value !== 0 || shape.nonzero_without_component_value !== 0) {
    throw new Error(`${family} row ${damageAttrId} contains componentless canonical damage`);
  }
  return {
    damage_attr_id: damageAttrId,
    lookup_key: String(ledgerRow.lookup_key),
    ability_id: Number(ledgerRow.ability_id),
    hit_event_id: Number(ledgerRow.hit_event_id),
    formula_family: family,
    formula_signature_id: String(ledgerRow.formula?.formula_signature_id || ""),
    packet_damage_results: packetResults,
    packet_damage_value_shape: shape,
    packet_damage_source_actor_kinds: countMap(executionRow.packet_damage_source_actor_kinds),
    packet_damage_target_actor_kinds: countMap(executionRow.packet_damage_target_actor_kinds),
    executor_boundary: "server-authored-normal-value-component",
    counterfactual_input: "canonical packet normal_value",
  };
}).sort((left, right) => left.damage_attr_id - right.damage_attr_id);

const families = [...familyNames].sort().map((family) => {
  const familyRows = rows.filter((row) => row.formula_family === family);
  if (familyRows.length === 0) {
    throw new Error(`${family} has no packet-observed exact row witness`);
  }
  const results = sum(familyRows.map((row) => row.packet_damage_results));
  const matches = sum(
    familyRows.map((row) => row.packet_damage_value_shape.amount_matches_normal_value),
  );
  if (results !== matches) {
    throw new Error(`${family} does not conserve its server-authored normal components`);
  }
  return {
    formula_family: family,
    proof_state: "offline-packet-output-executor-complete",
    executor_boundary: "server-authored-normal-value-component",
    exact_packet_rows: familyRows.length,
    packet_damage_results: results,
    exact_normal_value_matches: matches,
    current_build_damage_attr_ids: familyRows.map((row) => row.damage_attr_id),
    current_build_formula_signature_ids: unique(
      familyRows.map((row) => row.formula_signature_id).filter(Boolean),
    ),
    current_build_lookup_keys: unique(familyRows.map((row) => row.lookup_key)),
    delegated_counterfactuals: [
      "target physical armor",
      "elemental resistance",
      "primary-stat to attack",
      "mastery property",
      "source-specific state-scaled formulas",
    ],
  };
});

const totalResults = sum(rows.map((row) => row.packet_damage_results));
const exactMatches = sum(
  rows.map((row) => row.packet_damage_value_shape.amount_matches_normal_value),
);
const result = {
  schema_version: 1,
  generated_by: "tools/server-authored-damage-executor-proof.mjs",
  game: "blue-protocol-star-resonance",
  game_build: String(options.gameBuild),
  packet_build: String(options.packetBuild),
  policy: {
    runtime_packet_output_is_authoritative: true,
    packet_normal_value_is_authoritative_component: true,
    base_formula_reconstruction_claimed: false,
    source_specific_formula_proofs_remain_separate: true,
    shared_provider_counterfactuals_remain_separate: true,
    unresolved_evidence_is_hidden: false,
    current_gate_row_and_signature_coverage_is_exhaustive: true,
  },
  inputs: {
    formula_execution_proof: relative(executionPath),
    damage_resolution_ledger: relative(ledgerPath),
    packet_observed_damage_scope: relative(scopePath),
  },
  summary: {
    proof_state: "offline-packet-output-executor-complete",
    formula_families: families.length,
    exact_packet_rows: rows.length,
    packet_damage_results: totalResults,
    packet_results_with_normal_value: sum(
      rows.map((row) => row.packet_damage_value_shape.with_normal_value),
    ),
    exact_normal_value_matches: exactMatches,
    normal_value_conservation: exactMatches === totalResults,
    componentless_nonzero_results: sum(
      rows.map((row) => row.packet_damage_value_shape.nonzero_without_component_value),
    ),
  },
  executor_contract: {
    input: "canonical packet damage from an exact AutoAttack or SpAttack DamageAttr row",
    output: "one server-authored normal_value component conserved as canonical damage",
    provider_counterfactuals: "delegated to separately proven shared and source-specific models",
  },
  families,
  rows,
};

writeFileSync(outputPath, `${JSON.stringify(result, null, 2)}\n`, "utf8");
console.log(JSON.stringify(result.summary, null, 2));

function valueShape(value) {
  return Object.fromEntries([
    "results",
    "amount_zero",
    "amount_nonzero",
    "with_normal_value",
    "with_lucky_value",
    "with_both_values",
    "without_component_value",
    "zero_without_component_value",
    "nonzero_without_component_value",
    "amount_matches_normal_value",
    "amount_matches_lucky_value",
    "amount_matches_normal_plus_lucky",
    "lucky_flag_true",
    "causes_lucky_true",
  ].map((key) => [key, Number(value?.[key] || 0)]));
}

function countMap(value) {
  return Object.fromEntries(
    Object.entries(value || {})
      .map(([key, count]) => [String(key), Number(count || 0)])
      .sort(([left], [right]) => left.localeCompare(right)),
  );
}

function sum(values) {
  return values.reduce((total, value) => total + Number(value || 0), 0);
}

function unique(values) {
  return [...new Set(values)].sort((left, right) => String(left).localeCompare(String(right)));
}

function readJson(filePath, label) {
  try {
    return JSON.parse(readFileSync(filePath, "utf8"));
  } catch (error) {
    throw new Error(`failed to read ${label} ${filePath}: ${error.message}`);
  }
}

function assertBuild(actual, expected, label) {
  if (String(actual) !== String(expected)) {
    throw new Error(`${label} ${actual} differs from requested build ${expected}`);
  }
}

function parseArgs(args) {
  const result = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index]?.replace(/^--/, "");
    const value = args[index + 1];
    if (!key || value === undefined) throw new Error(`invalid argument near ${args[index]}`);
    result[key] = value;
  }
  for (const required of [
    "gameBuild", "packetBuild", "formulaExecutionProof", "damageLedger",
    "packetDamageScope", "output",
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
