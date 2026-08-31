#!/usr/bin/env node

import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const options = parseArgs(process.argv.slice(2));

if (options.verify) {
  verify(readJson(resolvePath(options.verify), "module terminal-effect map"));
  console.log(`verified ${resolvePath(options.verify)}`);
  process.exit(0);
}

const build = required(options, "build");
if (!/^\d+$/.test(build)) throw new Error("--build must contain only ASCII digits");
const decodedRoot = resolvePath(required(options, "decoded-root"));
const outputPath = resolvePath(options.output || path.join(
  "plugins/games/blue-protocol-star-resonance/research/game-file-inventory/global",
  `steam-${build}`,
  "module-terminal-effect-map.v1.json",
));

const buffRows = tableRows(decodedRoot, "BuffTable");
const modEffectRows = tableRows(decodedRoot, "ModEffectTable");
const modEffectLibraryRows = tableRows(decodedRoot, "ModEffectLibTable");
const moduleRows = tableRows(decodedRoot, "ModTable");
const buffIds = new Set(buffRows.map((row) => positiveInteger(row.Id, "BuffTable.Id")));

const operationProfiles = buildOperationProfiles(modEffectRows, buffIds);
const buffOperation = operationProfiles.find((profile) => profile.operation === 3);
if (!buffOperation || !buffOperation.exact_buff_table_population_proof) {
  throw new Error("opcode 3 did not reproduce its exact full-population BuffTable proof");
}

const librariesByFamily = new Map();
for (const row of modEffectLibraryRows) {
  const familyId = positiveInteger(row.EffectConfig, "ModEffectLibTable.EffectConfig");
  const values = librariesByFamily.get(familyId) || [];
  values.push({
    row_id: positiveInteger(row.Id, "ModEffectLibTable.Id"),
    library_id: positiveInteger(row.EffectLibID, "ModEffectLibTable.EffectLibID"),
  });
  librariesByFamily.set(familyId, values);
}

const modulesByLibrary = new Map();
for (const row of moduleRows) {
  const module = {
    module_item_id: positiveInteger(row.Id, "ModTable.Id"),
    display_name: stringOrNull(row.Name),
    module_type: integerOrNull(row.ModType),
    effect_library_ids: uniqueIntegers(row.EffectLibId),
  };
  for (const libraryId of module.effect_library_ids) {
    const values = modulesByLibrary.get(libraryId) || [];
    values.push(module);
    modulesByLibrary.set(libraryId, values);
  }
}

const routes = [];
for (const row of modEffectRows) {
  const familyId = positiveInteger(row.EffectID, "ModEffectTable.EffectID");
  for (const [tupleIndex, tuple] of tuples(row.EffectConfig).entries()) {
    if (Number(tuple[0]) !== 3) continue;
    const terminalEffectId = positiveInteger(tuple[1], "ModEffectTable.EffectConfig terminal effect");
    if (!buffIds.has(terminalEffectId)) throw new Error(`opcode 3 target ${terminalEffectId} is absent from BuffTable`);
    const libraryRows = (librariesByFamily.get(familyId) || [])
      .sort((left, right) => left.library_id - right.library_id || left.row_id - right.row_id);
    const libraryIds = [...new Set(libraryRows.map((entry) => entry.library_id))];
    const capableModules = dedupeBy(
      libraryIds.flatMap((libraryId) => modulesByLibrary.get(libraryId) || []),
      (module) => module.module_item_id,
    ).sort((left, right) => left.module_item_id - right.module_item_id);
    routes.push({
      terminal_effect_id: terminalEffectId,
      endpoint_table: "BuffTable",
      endpoint_resolution_state: "exact-current-build-schema-population",
      mod_effect_row_id: positiveInteger(row.Id, "ModEffectTable.Id"),
      mod_effect_family_id: familyId,
      mod_effect_level: nonNegativeInteger(row.Level, "ModEffectTable.Level"),
      display_name: stringOrNull(row.EffectName),
      icon_path: stringOrNull(row.EffectConfigIcon),
      effect_config_tuple_index: tupleIndex,
      effect_config_tuple: tuple.map((value) => Number(value)),
      eligible_library_ids: libraryIds,
      eligible_library_rows: libraryRows,
      capable_module_items: capableModules,
      owning_source_resolution_state: capableModules.length === 1
        ? "single-capable-module-item-not-equipped-proof"
        : capableModules.length > 1
          ? "multiple-capable-module-items"
          : "no-capable-module-item",
      exact_equipped_source_proven: false,
    });
  }
}
routes.sort((left, right) => left.terminal_effect_id - right.terminal_effect_id
  || left.mod_effect_family_id - right.mod_effect_family_id
  || left.mod_effect_level - right.mod_effect_level
  || left.mod_effect_row_id - right.mod_effect_row_id);

const terminalEffects = [...groupBy(routes, (route) => route.terminal_effect_id).entries()]
  .sort(([left], [right]) => left - right)
  .map(([terminalEffectId, terminalRoutes]) => ({
    terminal_effect_id: terminalEffectId,
    endpoint_table: "BuffTable",
    endpoint_resolution_state: "exact-current-build-schema-population",
    candidate_mod_effect_family_ids: [...new Set(terminalRoutes.map((route) => route.mod_effect_family_id))].sort(compareNumbers),
    candidate_mod_effect_levels: [...new Set(terminalRoutes.map((route) => route.mod_effect_level))].sort(compareNumbers),
    capable_module_item_ids: [...new Set(terminalRoutes.flatMap((route) =>
      route.capable_module_items.map((module) => module.module_item_id)))].sort(compareNumbers),
    exact_equipped_source_proven: false,
    routes: terminalRoutes,
  }));

const result = {
  schema_version: 1,
  game: "blue-protocol-star-resonance",
  game_build: build,
  generated_by: "tools/bpsr-module-terminal-effect-map.mjs",
  policy: {
    terminal_effect_id_is_exact_runtime_endpoint: true,
    current_build_schema_population_is_required: true,
    effect_family_and_level_are_exact_static_routes: true,
    capable_module_items_are_candidate_sources_not_equipped_proof: true,
    packet_or_snapshot_evidence_is_required_for_exact_equipped_source: true,
    display_names_are_presentation_only: true,
    numeric_neighbors_never_create_relationships: true,
    zero_hidden_omissions: true,
  },
  inputs: {
    decoded_root: relativePath(decodedRoot),
    tables: ["BuffTable.json", "ModEffectTable.json", "ModEffectLibTable.json", "ModTable.json"],
  },
  schema_proof: {
    source_table: "ModEffectTable",
    source_field: "EffectConfig",
    tuple_shape: ["operation", "target_id", "value"],
    operation_profiles: operationProfiles,
    promoted_relationship: {
      operation: 3,
      target_tuple_index: 1,
      target_table: "BuffTable",
      proof_state: "exact-full-population-current-build",
      populated_targets: buffOperation.populated_targets,
      distinct_targets: buffOperation.distinct_targets,
      missing_targets: [],
    },
  },
  summary: {
    mod_effect_rows: modEffectRows.length,
    mod_effect_config_tuples: operationProfiles.reduce((sum, profile) => sum + profile.populated_targets, 0),
    exact_buff_endpoint_tuples: routes.length,
    exact_terminal_effect_ids: terminalEffects.length,
    exact_mod_effect_family_ids: new Set(routes.map((route) => route.mod_effect_family_id)).size,
    capable_module_item_ids: new Set(routes.flatMap((route) =>
      route.capable_module_items.map((module) => module.module_item_id))).size,
    exact_equipped_module_sources: 0,
    hidden_routes: 0,
    conservation_complete: true,
  },
  terminal_effects_by_id: Object.fromEntries(terminalEffects.map((entry) => [String(entry.terminal_effect_id), entry])),
  terminal_effects: terminalEffects,
};

verify(result);
mkdirSync(path.dirname(outputPath), { recursive: true });
writeFileSync(outputPath, `${JSON.stringify(result, null, 2)}\n`, "utf8");
console.log(JSON.stringify({ output: outputPath, ...result.summary }, null, 2));

function buildOperationProfiles(rows, knownBuffIds) {
  const profiles = new Map();
  for (const row of rows) {
    for (const tuple of tuples(row.EffectConfig)) {
      const operation = Number(tuple[0]);
      const targetId = positiveInteger(tuple[1], "ModEffectTable.EffectConfig target");
      const profile = profiles.get(operation) || { operation, targets: [] };
      profile.targets.push(targetId);
      profiles.set(operation, profile);
    }
  }
  return [...profiles.values()].sort((left, right) => left.operation - right.operation).map((profile) => {
    const distinctTargets = [...new Set(profile.targets)].sort(compareNumbers);
    const matchingBuffTargets = distinctTargets.filter((targetId) => knownBuffIds.has(targetId));
    const missingTargets = distinctTargets.filter((targetId) => !knownBuffIds.has(targetId));
    return {
      operation: profile.operation,
      populated_targets: profile.targets.length,
      distinct_targets: distinctTargets.length,
      matching_buff_table_targets: matchingBuffTargets.length,
      missing_from_buff_table_targets: missingTargets,
      exact_buff_table_population_proof: profile.targets.length > 0
        && missingTargets.length === 0
        && matchingBuffTargets.length === distinctTargets.length,
    };
  });
}

function verify(catalog) {
  if (catalog.schema_version !== 1 || !/^\d+$/.test(String(catalog.game_build || ""))) {
    throw new Error("module terminal-effect map has invalid identity");
  }
  if (!catalog.policy?.zero_hidden_omissions || !catalog.policy?.capable_module_items_are_candidate_sources_not_equipped_proof) {
    throw new Error("module terminal-effect map safety policy is incomplete");
  }
  const proof = catalog.schema_proof?.promoted_relationship;
  if (proof?.operation !== 3 || proof.target_table !== "BuffTable"
      || proof.proof_state !== "exact-full-population-current-build"
      || (proof.missing_targets || []).length !== 0) {
    throw new Error("module terminal-effect map lacks exact opcode-3 endpoint proof");
  }
  const terminalEffects = catalog.terminal_effects || [];
  const routes = terminalEffects.flatMap((entry) => entry.routes || []);
  if (terminalEffects.length !== catalog.summary.exact_terminal_effect_ids
      || routes.length !== catalog.summary.exact_buff_endpoint_tuples
      || catalog.summary.hidden_routes !== 0) {
    throw new Error("module terminal-effect map conservation mismatch");
  }
  const ids = terminalEffects.map((entry) => positiveInteger(entry.terminal_effect_id, "terminal effect id"));
  if (new Set(ids).size !== ids.length) throw new Error("duplicate terminal effect group");
  for (const entry of terminalEffects) {
    if (catalog.terminal_effects_by_id?.[String(entry.terminal_effect_id)]?.terminal_effect_id !== entry.terminal_effect_id) {
      throw new Error(`terminal effect index mismatch for ${entry.terminal_effect_id}`);
    }
    if (entry.exact_equipped_source_proven !== false) throw new Error(`terminal effect ${entry.terminal_effect_id} overclaims equipped source`);
    for (const route of entry.routes) {
      if (route.terminal_effect_id !== entry.terminal_effect_id || route.endpoint_table !== "BuffTable") {
        throw new Error(`terminal effect route mismatch for ${entry.terminal_effect_id}`);
      }
      if (route.exact_equipped_source_proven !== false) throw new Error(`route ${route.mod_effect_row_id} overclaims equipped source`);
    }
  }
}

function tableRows(root, table) {
  const filePath = path.join(root, `${table}.json`);
  return Object.values(readJson(filePath, table));
}

function tuples(value) {
  return Array.isArray(value) ? value.filter((entry) => Array.isArray(entry) && entry.length >= 2) : [];
}

function groupBy(values, keyFn) {
  const result = new Map();
  for (const value of values) {
    const key = keyFn(value);
    const group = result.get(key) || [];
    group.push(value);
    result.set(key, group);
  }
  return result;
}

function dedupeBy(values, keyFn) {
  return [...new Map(values.map((value) => [keyFn(value), value])).values()];
}

function uniqueIntegers(value) {
  return [...new Set((Array.isArray(value) ? value : []).map((entry) => positiveInteger(entry, "integer list value")))].sort(compareNumbers);
}

function positiveInteger(value, label) {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed <= 0) throw new Error(`${label} must be a positive integer`);
  return parsed;
}

function nonNegativeInteger(value, label) {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed < 0) throw new Error(`${label} must be a non-negative integer`);
  return parsed;
}

function integerOrNull(value) {
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) ? parsed : null;
}

function stringOrNull(value) {
  const text = String(value ?? "").trim();
  return text === "" ? null : text;
}

function compareNumbers(left, right) {
  return left - right;
}

function parseArgs(args) {
  const result = {};
  for (let index = 0; index < args.length; index += 2) {
    const argument = args[index];
    if (!argument?.startsWith("--")) throw new Error(`unexpected argument ${argument}`);
    const value = args[index + 1];
    if (!value) throw new Error(`missing value for ${argument}`);
    result[argument.slice(2)] = value;
  }
  return result;
}

function required(values, key) {
  if (!values[key]) throw new Error(`missing --${key}`);
  return values[key];
}

function resolvePath(value) {
  return path.isAbsolute(value) ? value : path.resolve(repositoryRoot, value);
}

function relativePath(value) {
  return path.relative(repositoryRoot, value).replaceAll("\\", "/");
}

function readJson(filePath, label) {
  if (!existsSync(filePath)) throw new Error(`${label} not found: ${filePath}`);
  return JSON.parse(readFileSync(filePath, "utf8"));
}
