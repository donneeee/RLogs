#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { basename, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

function fail(message) {
  console.error(`generate-weapon-presentation: ${message}`);
  process.exit(1);
}

function parseArgs(argv) {
  const args = new Map();
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    const value = argv[index + 1];
    if (!key?.startsWith("--") || value === undefined) {
      fail(`expected --name value arguments, received ${argv.join(" ")}`);
    }
    args.set(key.slice(2), value);
  }
  return args;
}

function rustString(value) {
  return `"${String(value)
    .replaceAll("\\", "\\\\")
    .replaceAll('"', '\\"')
    .replaceAll("\n", "\\n")
    .replaceAll("\r", "\\r")
    .replaceAll("\t", "\\t")
    .replace(/[\u0000-\u001f\u007f]/g, (character) =>
      `\\u{${character.codePointAt(0).toString(16)}}`,
    )}"`;
}

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function badgeKind(itemId) {
  if (itemId >= 2_000_617 && itemId <= 2_000_625) return "far_sea";
  if (itemId >= 2_000_626 && itemId <= 2_000_634) return "ember_far_sea";
  return "weapon";
}

function required(args, key) {
  const value = args.get(key);
  if (!value) fail(`missing required --${key} argument`);
  return resolve(value);
}

const args = parseArgs(process.argv.slice(2));
const tablesDirectory = required(args, "tables");
const rustOutput = required(args, "rust-output");
const catalogDirectory = required(args, "catalog-dir");
const build = args.get("build") ?? fail("missing required --build argument");
const runtimeDirectory = resolve(fileURLToPath(new URL("../game-data/runtime", import.meta.url)));
const repositoryRoot = resolve(fileURLToPath(new URL("../../../../", import.meta.url)));
const localizationIdentity = readJson(join(runtimeDirectory, "localization-runtime.v1.json"));
if (
  localizationIdentity.schema_version !== 1
  || localizationIdentity.deployment_id !== "global"
  || localizationIdentity.client_build !== String(build)
  || typeof localizationIdentity.protocol_pack_digest !== "string"
) fail("localization runtime identity does not match the requested weapon build");
const buildManifest = readJson(join(
  repositoryRoot,
  `plugins/games/blue-protocol-star-resonance/research/game-file-inventory/global/steam-${build}/complete-build-source-manifest.v1.json`,
));

const tableNames = [
  "ItemTable.json",
  "EquipTable.json",
  "EquipBreakThroughTable.json",
  "EquipWeaponTable.json",
];
const tablePaths = Object.fromEntries(
  tableNames.map((name) => [name, join(tablesDirectory, name)]),
);
for (const path of Object.values(tablePaths)) {
  if (!existsSync(path)) fail(`table does not exist: ${path}`);
}
const itemTableManifest = buildManifest.files?.find((entry) =>
  entry.root === "decoded-game-tables" && entry.relativePath === "ItemTable.json");
const itemTableSha256 = createHash("sha256").update(readFileSync(tablePaths["ItemTable.json"])).digest("hex");
if (
  buildManifest.deployment !== localizationIdentity.deployment_id
  || String(buildManifest.gameBuild) !== String(build)
  || itemTableManifest?.authority !== "exact-current-build-static-data"
  || itemTableManifest.sha256 !== itemTableSha256
) fail("ItemTable does not match the exact current-build source manifest");
if (!existsSync(catalogDirectory)) fail(`catalog directory does not exist: ${catalogDirectory}`);

const itemTable = readJson(tablePaths["ItemTable.json"]);
const equipTable = readJson(tablePaths["EquipTable.json"]);
const breakthroughTable = readJson(tablePaths["EquipBreakThroughTable.json"]);
const weaponTable = readJson(tablePaths["EquipWeaponTable.json"]);
const inputHash = createHash("sha256");
for (const name of tableNames) inputHash.update(readFileSync(tablePaths[name]));
const sourceHash = inputHash.digest("hex");

const breakthroughsByEquipId = new Map();
for (const row of Object.values(breakthroughTable)) {
  const equipId = Number(row.EquipId);
  if (!breakthroughsByEquipId.has(equipId)) breakthroughsByEquipId.set(equipId, []);
  breakthroughsByEquipId.get(equipId).push(row);
}
for (const rows of breakthroughsByEquipId.values()) {
  rows.sort((left, right) => Number(left.BreakThroughTime) - Number(right.BreakThroughTime));
}

const records = Object.values(itemTable)
  .filter((row) => Number(row.Type) === 200)
  .map((item) => {
    const itemId = Number(item.Id);
    const equipment = equipTable[String(itemId)];
    const weapon = weaponTable[String(itemId)];
    const progression = [];
    if (equipment && Number.isFinite(Number(equipment.EquipGs))) {
      progression.push(Number(equipment.EquipGs));
      for (const row of breakthroughsByEquipId.get(itemId) ?? []) {
        progression.push(Number(row.EquipGs));
      }
    }
    const iconAddress = String(item.Icon ?? "").trim();
    if (!iconAddress) fail(`weapon ${itemId} has no ItemTable icon address`);
    return {
      itemId,
      englishName: String(item.Name ?? "").trim(),
      iconAddress,
      icon: `icons/weapons/items/${basename(iconAddress)}.png`,
      quality: Number(item.Quality),
      professionId: weapon ? Number(weapon.ProfessionId) : null,
      weaponSkinId: weapon ? Number(weapon.WeaponSkinId) : null,
      progression,
      badgeKind: badgeKind(itemId),
    };
  })
  .sort((left, right) => left.itemId - right.itemId);

if (records.length !== 722) {
  fail(`expected 722 current-build weapon rows, found ${records.length}`);
}
for (let index = 1; index < records.length; index += 1) {
  if (records[index - 1].itemId === records[index].itemId) {
    fail(`duplicate weapon item ID ${records[index].itemId}`);
  }
}
if (records.some((record) => !record.englishName || /[\u3400-\u9fff\uf900-\ufaff]/u.test(record.englishName))) {
  fail("current-build weapon names contain an empty or unresolved label");
}

const rust = [
  "// @generated by tools/generate-weapon-presentation.mjs.",
  `// Source: current global Steam build ${build}; joined table sha256 ${sourceHash}.`,
  "// Do not hand-edit. Regenerate after a reviewed client table extraction.",
  "",
  "use super::WeaponPresentationRecord;",
  "",
  "pub(super) const WEAPON_PRESENTATION_RECORDS: &[WeaponPresentationRecord] = &[",
  ...records.map(
    (record) =>
      `    WeaponPresentationRecord { item_id: ${record.itemId}, icon: ${rustString(record.icon)}, levels: &[${record.progression.join(", ")}], badge_kind: ${rustString(record.badgeKind)} },`,
  ),
  "];",
  "",
].join("\n");
writeFileSync(rustOutput, rust, "utf8");

const weaponNames = {
  schema_version: 1,
  locale: "en-US",
  deployment_id: localizationIdentity.deployment_id,
  client_build: localizationIdentity.client_build,
  protocol_pack_digest: localizationIdentity.protocol_pack_digest,
  source_item_table_sha256: itemTableSha256,
  weapons: records.map((record) => [record.itemId, record.englishName]),
};
writeFileSync(
  join(runtimeDirectory, "localization/en-US/weapon-names.v1.json"),
  `${JSON.stringify(weaponNames)}\n`,
  "utf8",
);

const catalogFiles = readdirSync(catalogDirectory).filter((name) => name.endsWith(".json"));
const filesByItemId = new Map();
for (const name of catalogFiles) {
  const match = /^(\d+)-/.exec(name);
  if (!match) continue;
  const itemId = Number(match[1]);
  if (filesByItemId.has(itemId)) fail(`multiple catalog files begin with ${itemId}-`);
  filesByItemId.set(itemId, join(catalogDirectory, name));
}

for (const record of records) {
  const path = filesByItemId.get(record.itemId);
  if (!path) fail(`catalog file is missing for weapon ${record.itemId}`);
  const existing = readJson(path);
  const catalog = {
    ...existing,
    schema_version: 3,
    localization_key: existing.localization_key ?? null,
    icon: record.icon,
    attributes: {
      equipment_weapon_id: record.itemId,
      english_name: record.englishName,
      profession_id: record.professionId,
      weapon_skin_id: record.weaponSkinId,
      quality: record.quality,
      item_table_icon_address: record.iconAddress,
      level_progression: record.progression,
      base_level: record.progression.at(0) ?? null,
      maximum_level: record.progression.at(-1) ?? null,
      badge_kind: record.badgeKind,
    },
    availability: [
      {
        deployment_id: "global",
        channel: "steam",
        client_build: String(build),
      },
    ],
    provenance: {
      source: "ctb.ItemTable+EquipTable+EquipBreakThroughTable+EquipWeaponTable",
      reference: `client-build:global/steam/${build}@sha256:${sourceHash}`,
      confidence: "corroborated",
    },
  };
  writeFileSync(path, `${JSON.stringify(catalog, null, 2)}\n`, "utf8");
}

const distinctIcons = new Set(records.map((record) => record.icon));
const progressive = records.filter((record) => record.progression.length > 1).length;
const levelUnknown = records.filter((record) => record.progression.length === 0).length;
console.log(
  JSON.stringify(
    {
      build: String(build),
      weapon_records: records.length,
      distinct_item_table_artworks: distinctIcons.size,
      progressive_weapon_records: progressive,
      fixed_level_weapon_records: records.length - progressive - levelUnknown,
      level_unknown_weapon_records: levelUnknown,
      source_sha256: sourceHash,
      rust_output: rustOutput,
      catalog_directory: catalogDirectory,
    },
    null,
    2,
  ),
);
