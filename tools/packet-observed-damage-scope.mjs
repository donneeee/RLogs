#!/usr/bin/env node

import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const options = parseArgs(process.argv.slice(2));
const stateInventoryPath = resolvePath(options.stateInventory);
const exactKeysPath = resolvePath(options.exactKeys);
const outputPath = resolvePath(options.output);

const stateInventoryText = readFileSync(stateInventoryPath, "utf8");
const exactKeysText = readFileSync(exactKeysPath, "utf8");
const stateInventory = JSON.parse(stateInventoryText);
const exactRows = parseCsv(exactKeysText);

const observedAbilityIds = numbers(
  (stateInventory.abilities || []).map((row) => row.ability_id),
);
const exactLookupKeys = unique(
  exactRows
    .map((row) => {
      const abilityId = Number(row.ability);
      const hitEventId = Number(row.hit);
      return Number.isSafeInteger(abilityId) && Number.isSafeInteger(hitEventId)
        ? `${abilityId}:${hitEventId}`
        : null;
    })
    .filter(Boolean),
).sort(compareLookupKeys);

const exactAbilityIds = numbers(exactRows.map((row) => row.ability));
const exactAbilitySet = new Set(exactAbilityIds);
const result = {
  schema_version: 1,
  generated_by: "tools/packet-observed-damage-scope.mjs",
  game: "blue-protocol-star-resonance",
  packet_build: String(stateInventory.game_build || options.packetBuild),
  policy: {
    canonical_events_are_never_removed: true,
    observed_ability_scope_is_conservative: true,
    all_current_build_candidate_hit_rows_for_observed_abilities_remain_in_scope: true,
    exact_lookup_keys_are_a_proven_subset_not_a_filter: true,
    unobserved_current_build_rows_remain_cataloged: true,
    capture_is_final_validation_not_discovery: true,
  },
  inputs: {
    state_inventory: relative(stateInventoryPath),
    state_inventory_sha256: sha256(stateInventoryText),
    exact_key_inventory: relative(exactKeysPath),
    exact_key_inventory_sha256: sha256(exactKeysText),
  },
  summary: {
    observed_abilities: observedAbilityIds.length,
    abilities_with_exact_lookup_keys: exactAbilityIds.length,
    observed_abilities_without_exact_lookup_keys:
      observedAbilityIds.filter((id) => !exactAbilitySet.has(id)).length,
    exact_lookup_keys: exactLookupKeys.length,
    packet_damage_events: (stateInventory.abilities || []).reduce(
      (total, row) => total + Number(row.damage_events || 0),
      0,
    ),
  },
  observed_ability_ids: observedAbilityIds,
  exact_lookup_keys: exactLookupKeys,
};

writeFileSync(outputPath, `${JSON.stringify(result, null, 2)}\n`, "utf8");
console.log(JSON.stringify(result.summary, null, 2));

function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index]?.replace(/^--/, "");
    const value = args[index + 1];
    if (!key || value === undefined) throw new Error(`invalid argument near ${args[index]}`);
    parsed[key] = value;
  }
  for (const required of ["packetBuild", "stateInventory", "exactKeys", "output"]) {
    if (!parsed[required]) throw new Error(`missing --${required}`);
  }
  return parsed;
}

function parseCsv(text) {
  const lines = text.trim().split(/\r?\n/);
  if (lines.length === 0) return [];
  const headers = parseCsvLine(lines[0]);
  return lines.slice(1).filter(Boolean).map((line) => {
    const values = parseCsvLine(line);
    return Object.fromEntries(headers.map((header, index) => [header, values[index] || ""]));
  });
}

function parseCsvLine(line) {
  const values = [];
  let value = "";
  let quoted = false;
  for (let index = 0; index < line.length; index += 1) {
    const char = line[index];
    if (char === '"') {
      if (quoted && line[index + 1] === '"') {
        value += '"';
        index += 1;
      } else {
        quoted = !quoted;
      }
    } else if (char === "," && !quoted) {
      values.push(value);
      value = "";
    } else {
      value += char;
    }
  }
  values.push(value);
  return values;
}

function resolvePath(value) {
  return path.isAbsolute(value) ? value : path.resolve(repoRoot, value);
}

function relative(filePath) {
  return path.relative(repoRoot, filePath).replaceAll("\\", "/");
}

function sha256(text) {
  return createHash("sha256").update(text).digest("hex");
}

function unique(values) {
  return [...new Set(values)];
}

function numbers(values) {
  return unique(values.map(Number).filter(Number.isSafeInteger)).sort((left, right) => left - right);
}

function compareLookupKeys(left, right) {
  const [leftAbility, leftHit] = left.split(":").map(Number);
  const [rightAbility, rightHit] = right.split(":").map(Number);
  return leftAbility - rightAbility || leftHit - rightHit;
}
