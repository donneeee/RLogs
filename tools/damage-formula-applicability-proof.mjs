#!/usr/bin/env node

import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const options = parseArgs(process.argv.slice(2));
const formulaProofPath = resolvePath(options.formulaProof);
const outputPath = resolvePath(options.output);
const formulaProof = readJson(formulaProofPath, "formula execution proof");

const incomingOnlyFamilies = new Set(["MstSpSkillAttack", "MstSpSkillMAttack"]);
const outgoingTargetKinds = new Set(["monster", "npc", "training_dummy"]);
const rows = [];

for (const row of formulaProof.observed_damage_rows || []) {
  const family = String(row.damage_script || "<missing>");
  if (!incomingOnlyFamilies.has(family)) continue;

  const damageResults = Number(row.packet_damage_results || 0);
  if (damageResults <= 0) continue;

  const targetKinds = countMap(row.packet_damage_target_actor_kinds);
  const targetCount = sum(Object.values(targetKinds));
  if (targetCount !== damageResults) {
    throw new Error(
      `damage row ${row.damage_id} actor-domain target count ${targetCount} does not conserve ${damageResults} packet damage results`,
    );
  }

  const outgoingTargets = Object.fromEntries(
    Object.entries(targetKinds).filter(([kind]) => outgoingTargetKinds.has(kind)),
  );
  const disposition = Object.keys(outgoingTargets).length === 0
    ? "retained-incoming-only-no-outgoing-counterfactual"
    : "outgoing-counterfactual-formula-required";

  rows.push({
    damage_attr_id: Number(row.damage_id),
    damage_script: family,
    packet_damage_results: damageResults,
    packet_damage_source_actor_kinds: countMap(row.packet_damage_source_actor_kinds),
    packet_damage_target_actor_kinds: targetKinds,
    outgoing_target_actor_kinds: outgoingTargets,
    rdps_formula_disposition: disposition,
    retained_metrics: disposition === "retained-incoming-only-no-outgoing-counterfactual"
      ? ["raw-event", "damage-taken", "tps", "death-timeline"]
      : ["raw-event", "outgoing-damage", "rdps-counterfactual"],
    proof_authority: "exact-packet-row-result-kind-and-target-actor-domain",
  });
}

rows.sort((left, right) => left.damage_attr_id - right.damage_attr_id);
const incomingOnlyRows = rows.filter(
  (row) => row.rdps_formula_disposition === "retained-incoming-only-no-outgoing-counterfactual",
);
const outgoingRows = rows.filter(
  (row) => row.rdps_formula_disposition === "outgoing-counterfactual-formula-required",
);

const result = {
  schema_version: 1,
  generated_by: "tools/damage-formula-applicability-proof.mjs",
  game: "blue-protocol-star-resonance",
  game_build: String(options.gameBuild),
  packet_build: String(options.packetBuild),
  policy: {
    observed_scope_only: true,
    formula_executor_is_not_inferred: true,
    incoming_damage_is_never_hidden: true,
    incoming_damage_remains_available_to_tps_and_death_timelines: true,
    outgoing_counterfactual_requires_monster_npc_or_training_dummy_target: true,
    actor_kind_zero_is_not_promoted_to_an_enemy_target: true,
  },
  input: {
    formula_execution_proof: relative(formulaProofPath),
    schema_version: Number(formulaProof.schema_version || 0),
  },
  summary: {
    reviewed_rows: rows.length,
    retained_incoming_only_rows: incomingOnlyRows.length,
    outgoing_counterfactual_rows: outgoingRows.length,
    retained_incoming_only_packet_damage_results: sum(
      incomingOnlyRows.map((row) => row.packet_damage_results),
    ),
    outgoing_counterfactual_packet_damage_results: sum(
      outgoingRows.map((row) => row.packet_damage_results),
    ),
  },
  rows,
};

writeFileSync(outputPath, `${JSON.stringify(result, null, 2)}\n`, "utf8");
console.log(JSON.stringify(result.summary, null, 2));

function countMap(value) {
  return Object.fromEntries(
    Object.entries(value || {})
      .map(([key, count]) => [String(key), Number(count)])
      .filter(([, count]) => Number.isSafeInteger(count) && count > 0)
      .sort(([left], [right]) => left.localeCompare(right)),
  );
}

function sum(values) {
  return values.reduce((total, value) => total + Number(value || 0), 0);
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
  for (const required of ["gameBuild", "packetBuild", "formulaProof", "output"]) {
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
