#!/usr/bin/env node

import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const sourceRoot = path.join(repoRoot, "plugins/games/blue-protocol-star-resonance/game-data/catalog");
const inventoryPath = path.join(
  repoRoot,
  "plugins/games/blue-protocol-star-resonance/research/game-file-inventory/global/steam-24687926/ctb-indexed-inventory-v2.json",
);
const namesPath = path.join(
  repoRoot,
  "plugins/games/blue-protocol-star-resonance/research/game-file-inventory/global/steam-24687926/ctb-table-name-identities.v1.json",
);
const outputPath = path.join(
  repoRoot,
  "plugins/games/blue-protocol-star-resonance/game-data/runtime/dungeon-objectives.current-build.v1.json",
);
const deploymentId = "global";
const channel = "steam";
const clientBuild = "24687926";

const inventory = readJson(inventoryPath);
const names = readJson(namesPath);
if (String(inventory.build_id) !== clientBuild) throw new Error("CTB inventory build does not match the requested runtime build");
const identity = names.identities.find((entry) => entry.table_name === "TargetTable.ctb");
if (!identity) throw new Error("TargetTable.ctb has no reviewed current-build identity");
const tables = inventory.tables.filter((table) => table.address_keys.some((entry) => entry.key === identity.table_key));
if (tables.length !== 1) throw new Error(`expected one current-build TargetTable.ctb, found ${tables.length}`);
const table = tables[0];

const records = collectJson(path.join(sourceRoot, "activity-targets"))
  .map(readJson)
  .filter((record) => record.kind === "activity_target" && record.provenance?.source === "ctb.TargetTable")
  .sort((left, right) => left.id - right.id);
if (records.length !== table.shape.rows) {
  throw new Error(`source catalog has ${records.length} TargetTable rows but current build has ${table.shape.rows}`);
}
const sourceDigests = new Set(records.map((record) => digestFromReference(record.provenance.reference)));
if (sourceDigests.size !== 1 || !sourceDigests.has(table.sha256)) {
  throw new Error(`current TargetTable digest ${table.sha256} is not byte-identical to the reviewed source table`);
}

const localizationRoot = path.join(sourceRoot, "localization");
const localizedByKey = new Map();
for (const localeEntry of readdirSync(localizationRoot, { withFileTypes: true })) {
  if (!localeEntry.isDirectory()) continue;
  const file = path.join(localizationRoot, localeEntry.name, "behavior", "catalog.json");
  let rows;
  try { rows = readJson(file); } catch { continue; }
  for (const row of rows) {
    if (!row.key?.startsWith("activity-target.")) continue;
    if (digestFromReference(row.provenance?.reference) !== table.sha256) continue;
    const localized = localizedByKey.get(row.key) ?? {};
    localized[localeEntry.name] = row.text;
    localizedByKey.set(row.key, localized);
  }
}

const entries = records.map((record) => {
  const requiredCount = record.attributes?.required_count;
  if (!Number.isSafeInteger(requiredCount) || requiredCount < 0) {
    throw new Error(`activity target ${record.id} has invalid required_count`);
  }
  const localizationKey = typeof record.localization_key === "string" && record.localization_key
    ? record.localization_key
    : null;
  return {
    objective_id: record.id,
    stable_key: record.stable_key,
    localization_key: localizationKey,
    required_count: requiredCount,
    scene_event_keys: record.attributes.scene_event_keys ?? [],
    names: localizationKey === null ? {} : localizedByKey.get(localizationKey) ?? {},
  };
});

const output = {
  schema_version: 1,
  generated_by: "tools/bpsr-dungeon-objective-runtime.mjs",
  deployment_id: deploymentId,
  channel,
  client_build: clientBuild,
  proof: {
    source_table: "ctb.TargetTable",
    current_table_key: identity.table_key,
    current_table_sha256: table.sha256,
    current_table_rows: table.shape.rows,
    source_and_current_tables_are_byte_identical: true,
    inventory: path.relative(repoRoot, inventoryPath).replaceAll("\\", "/"),
  },
  entries,
};
const canonical = `${JSON.stringify(output, null, 2)}\n`;
output.content_sha256 = `sha256:${createHash("sha256").update(canonical).digest("hex")}`;
const rendered = `${JSON.stringify(output, null, 2)}\n`;
if (process.argv.includes("--check")) {
  let current;
  try {
    current = readFileSync(outputPath, "utf8");
  } catch {
    throw new Error(`generated runtime catalog is missing: ${path.relative(repoRoot, outputPath)}`);
  }
  if (current !== rendered) {
    throw new Error("generated dungeon objective runtime catalog is stale; regenerate it without --check");
  }
  console.log(`Verified ${path.relative(repoRoot, outputPath)} with ${entries.length} byte-identical current-build objectives.`);
  process.exit(0);
}
mkdirSync(path.dirname(outputPath), { recursive: true });
writeFileSync(outputPath, rendered);
console.log(`Wrote ${path.relative(repoRoot, outputPath)} with ${entries.length} byte-identical current-build objectives.`);

function collectJson(directory, output = []) {
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const target = path.join(directory, entry.name);
    if (entry.isDirectory()) collectJson(target, output);
    else if (entry.isFile() && entry.name.endsWith(".json")) output.push(target);
  }
  return output;
}

function readJson(file) {
  return JSON.parse(readFileSync(file, "utf8"));
}

function digestFromReference(reference) {
  const match = typeof reference === "string" ? reference.match(/@(sha256:[0-9a-f]{64})$/) : null;
  return match?.[1] ?? null;
}
