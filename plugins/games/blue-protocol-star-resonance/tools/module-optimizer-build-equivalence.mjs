#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SOURCE_BUILD = "24252055";
const CURRENT_BUILD = "24687926";
const write = process.argv.includes("--write");
const toolRoot = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(toolRoot, "../../../..");
const pluginRoot = path.resolve(toolRoot, "..");
const catalogRoot = path.join(pluginRoot, "game-data/catalog");
const tableRoot = process.env.BPSR_TABLE_DATA_DIR
  ? path.resolve(process.env.BPSR_TABLE_DATA_DIR)
  : path.join(repoRoot, "Excels");
const buildManifestPath = path.join(
  pluginRoot,
  `research/game-file-inventory/global/steam-${CURRENT_BUILD}/complete-build-source-manifest.v1.json`,
);
const receiptPath = path.join(
  catalogRoot,
  "coverage/module-optimizer-scoring-build-equivalence.v1.json",
);

const readJson = (file) => JSON.parse(readFileSync(file, "utf8"));
const sha256 = (value) => createHash("sha256").update(value).digest("hex");
const jsonDigest = (value) => sha256(JSON.stringify(value));
const sortedJsonFiles = (directory) => readdirSync(directory)
  .filter((name) => name.endsWith(".json"))
  .sort()
  .map((name) => path.join(directory, name));
const writeJson = (file, value) => writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`);
const fail = (message) => { throw new Error(message); };

if (!existsSync(buildManifestPath)) fail(`missing current-build source manifest: ${buildManifestPath}`);
const buildManifestBytes = readFileSync(buildManifestPath);
const buildManifest = JSON.parse(buildManifestBytes);
if (String(buildManifest.gameBuild) !== CURRENT_BUILD || buildManifest.deployment !== "global") {
  fail("current-build source manifest has the wrong identity");
}

const tableNames = ["ModEffectTable.json", "ModEffectLibTable.json", "ModLinkEffectTable.json"];
const tableEvidence = {};
for (const name of tableNames) {
  const file = path.join(tableRoot, name);
  const bytes = readFileSync(file);
  const digest = sha256(bytes);
  const evidence = buildManifest.files?.find((entry) =>
    entry.root === "decoded-game-tables" && entry.relativePath === name);
  if (evidence?.authority !== "exact-current-build-static-data" || evidence.sha256 !== digest) {
    fail(`${name} does not match the exact ${CURRENT_BUILD} source manifest`);
  }
  tableEvidence[name] = { sha256: digest, bytes: bytes.length };
}

const effectFiles = sortedJsonFiles(path.join(catalogRoot, "module-effects"));
const sourceEffects = effectFiles.map((file) => {
  const record = readJson(file);
  if (record.kind !== "module_effect" || record.schema_version !== 2) {
    fail(`invalid module-effect catalog row: ${file}`);
  }
  return {
    id: record.id,
    effect_library_ids: [...record.attributes.effect_library_ids].sort((a, b) => a - b),
    levels: record.attributes.levels.map((level) => ({
      row_id: level.row_id,
      level: level.level,
      required_link_points: level.required_link_points,
      fight_value: level.fight_value,
    })),
  };
}).sort((a, b) => a.id - b.id);

const currentEffectRows = Object.values(readJson(path.join(tableRoot, "ModEffectTable.json")));
const currentEffectLibraries = Object.values(readJson(path.join(tableRoot, "ModEffectLibTable.json")));
const librariesByEffect = new Map();
for (const row of currentEffectLibraries) {
  const values = librariesByEffect.get(row.EffectConfig) ?? new Set();
  values.add(row.EffectLibID);
  librariesByEffect.set(row.EffectConfig, values);
}
const levelsByEffect = new Map();
for (const row of currentEffectRows) {
  const levels = levelsByEffect.get(row.EffectID) ?? [];
  levels.push({
    row_id: row.Id,
    level: row.Level,
    required_link_points: row.EnhancementNum,
    fight_value: row.FightValue,
  });
  levelsByEffect.set(row.EffectID, levels);
}
const currentEffects = [...levelsByEffect.entries()].map(([id, levels]) => ({
  id,
  effect_library_ids: [...(librariesByEffect.get(id) ?? [])].sort((a, b) => a - b),
  levels: levels.sort((a, b) => a.row_id - b.row_id),
})).sort((a, b) => a.id - b.id);

const linkFiles = sortedJsonFiles(path.join(catalogRoot, "module-link-effects"));
const sourceLinks = linkFiles.map((file) => {
  const record = readJson(file);
  if (record.kind !== "module_link_effect" || record.schema_version !== 2) {
    fail(`invalid module-link catalog row: ${file}`);
  }
  return {
    row_id: record.attributes.source_row_id,
    link_value: record.attributes.link_value,
    fight_value: record.attributes.fight_value,
  };
}).sort((a, b) => a.row_id - b.row_id);
const currentLinks = Object.values(readJson(path.join(tableRoot, "ModLinkEffectTable.json")))
  .map((row) => ({ row_id: row.Id, link_value: row.LinkTime, fight_value: row.FightValue }))
  .sort((a, b) => a.row_id - b.row_id);

const differences = [];
if (JSON.stringify(sourceEffects) !== JSON.stringify(currentEffects)) {
  differences.push("module-effect families, library membership, thresholds, or fight values differ");
}
if (JSON.stringify(sourceLinks) !== JSON.stringify(currentLinks)) {
  differences.push("module-link values or fight values differ");
}

const sourceReferences = [...new Set(effectFiles.map((file) => readJson(file).provenance?.reference))];
const sourceLinkReferences = [...new Set(linkFiles.map((file) => readJson(file).provenance?.reference))];
if (sourceReferences.length !== 1 || sourceLinkReferences.length !== 1) {
  fail("source-build optimizer catalog provenance is not uniform");
}

const receipt = {
  schema_version: 1,
  kind: "module_optimizer_scoring_build_equivalence",
  deployment_id: "global",
  channel: "steam",
  source_build: SOURCE_BUILD,
  current_build: CURRENT_BUILD,
  authority: "exact-current-build-static-data",
  source_catalog: {
    manifest: "../manifest.json",
    module_effect_reference: sourceReferences[0],
    module_link_reference: sourceLinkReferences[0],
  },
  current_build_source: {
    manifest: `../../../research/game-file-inventory/global/steam-${CURRENT_BUILD}/complete-build-source-manifest.v1.json`,
    manifest_sha256: sha256(buildManifestBytes),
    tables: tableEvidence,
  },
  comparison: {
    module_effect_family_count: sourceEffects.length,
    module_effect_scoring_row_count: sourceEffects.reduce((sum, effect) => sum + effect.levels.length, 0),
    module_link_scoring_row_count: sourceLinks.length,
    source_module_effect_digest: jsonDigest(sourceEffects),
    current_module_effect_digest: jsonDigest(currentEffects),
    source_module_link_digest: jsonDigest(sourceLinks),
    current_module_link_digest: jsonDigest(currentLinks),
    identical: differences.length === 0,
    differences,
  },
  policy: {
    every_effect_family_and_library_membership_must_match: true,
    every_effect_threshold_and_fight_value_row_must_match: true,
    every_total_link_and_fight_value_row_must_match: true,
    promotion_fails_closed_on_any_difference: true,
  },
};

if (differences.length > 0) {
  process.stderr.write(`${JSON.stringify(receipt, null, 2)}\n`);
  fail(`optimizer scoring differs between ${SOURCE_BUILD} and ${CURRENT_BUILD}; authority was not changed`);
}

if (write) {
  writeJson(receiptPath, receipt);
  const availability = { deployment_id: "global", channel: "steam", client_build: CURRENT_BUILD };
  for (const file of [...effectFiles, ...linkFiles]) {
    const record = readJson(file);
    if (!record.availability.some((entry) => entry.deployment_id === availability.deployment_id
      && entry.channel === availability.channel && entry.client_build === availability.client_build)) {
      record.availability.push(availability);
      writeJson(file, record);
    }
  }
  const manifestPath = path.join(catalogRoot, "manifest.json");
  const manifest = readJson(manifestPath);
  if (!manifest.supported_builds.some((entry) => entry.deployment_id === availability.deployment_id
    && entry.channel === availability.channel && entry.client_build === availability.client_build)) {
    manifest.supported_builds.push(availability);
  }
  manifest.catalog_revision = "steam-24252055-24687926-reviewed-catalog-v6";
  manifest.scoring_equivalence_receipts = [
    "coverage/module-optimizer-scoring-build-equivalence.v1.json",
  ];
  writeJson(manifestPath, manifest);
}

process.stdout.write(
  `verified ${sourceEffects.length} effect families, `
  + `${receipt.comparison.module_effect_scoring_row_count} effect rows, and `
  + `${sourceLinks.length} link rows: ${SOURCE_BUILD} == ${CURRENT_BUILD}`
  + `${write ? "; promoted current-build authority" : ""}\n`,
);
