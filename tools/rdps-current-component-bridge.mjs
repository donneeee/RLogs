#!/usr/bin/env node

import { existsSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const options = parseArgs(process.argv.slice(2));
const originPath = resolvePath(options.originLedger);
const decodedRoot = resolvePath(options.decodedRoot);
const outputPath = resolvePath(options.output);
const origin = readJson(originPath, "current Aoyi origin ledger");

const fightAttributes = readJson(path.join(decodedRoot, "FightAttrTable.json"), "FightAttrTable.json");
const attributeVariants = indexAttributeVariants(fightAttributes);
const tables = [
  ["buff", "BuffTable.json"],
  ["damage", "DamageAttrTable.json"],
  ["recount", "RecountTable.json"],
  ["skill-effect", "SkillEffectTable.json"],
  ["skill", "SkillTable.json"],
  ["projectile", "BulletTable.json"],
].map(([uidKind, file]) => ({
  uidKind,
  file,
  rows: readJson(path.join(decodedRoot, file), file),
}));

validateBuild();

const routesByEffect = new Map();
let routeCount = 0;
for (const skill of origin.skills || []) {
  for (const route of skill.component_routes || []) {
    routeCount += 1;
    for (const effectId of numbers(route.effect_ids)) {
      const rows = routesByEffect.get(effectId) || [];
      rows.push(compactRoute(skill, route));
      routesByEffect.set(effectId, rows);
    }
  }
}

const sourcesByRuleId = {};
const unresolvedIds = [];
const ambiguousIds = [];
const uidKindCounts = {};
const sourceConfigIds = new Set();
for (const [effectId, routes] of [...routesByEffect.entries()].sort(([left], [right]) => left - right)) {
  const classification = classifyUid(effectId);
  if (classification.matches.length === 0) unresolvedIds.push(effectId);
  if (classification.matches.length > 1) ambiguousIds.push({
    uid: effectId,
    selected_uid_kind: classification.uidKind,
    matches: classification.matches,
  });
  uidKindCounts[classification.uidKind] = (uidKindCounts[classification.uidKind] || 0) + 1;
  for (const route of routes) {
    for (const uid of route.source_config_ids) sourceConfigIds.add(uid);
  }
  const sourceRuleId = `current-component:effect:${effectId}`;
  sourcesByRuleId[sourceRuleId] = {
    sourceRuleId,
    sourceId: `current-component-effect:${effectId}`,
    sourceName: componentSourceName(effectId, routes),
    relationshipStatus: classification.uidKind === "unresolved"
      ? "current-build-component-route-uid-kind-unresolved"
      : "current-build-component-route-exact",
    uidEdges: uniqueEdges([
      {
        edgeKind: "component-output",
        uidKind: classification.uidKind,
        uid: effectId,
        role: "runtime-or-formula-component",
        source: "current-aoyi-rdps-origin-ledger.component_routes",
        status: classification.matches.length > 1
          ? "typed-with-multiple-table-definitions"
          : classification.uidKind === "unresolved" ? "unresolved" : "exact-current-build-table-row",
        relationshipKind: "current-build-component-output",
      },
      ...routes.flatMap((route) => [
        {
          edgeKind: "owner-source",
          uidKind: "skill-aoyi",
          uid: route.skill_id,
          role: "owner",
          source: "current-aoyi-rdps-origin-ledger.skills",
          status: "exact-current-build-owner-skill",
          relationshipKind: route.component_id,
        },
        ...(route.item_id ? [{
          edgeKind: "owner-item",
          uidKind: "item",
          uid: route.item_id,
          role: "owner",
          source: "current-aoyi-rdps-origin-ledger.skills",
          status: "exact-current-build-owner-item",
          relationshipKind: route.component_id,
        }] : []),
        ...route.source_config_ids.flatMap((uid) => sourceConfigEdges(uid, route)),
      ]),
    ]),
    componentRoutes: routes,
    tableClassification: classification,
  };
}

const sourceConfigClassifications = [...sourceConfigIds]
  .sort((left, right) => left - right)
  .map((uid) => ({ uid, ...classifyUid(uid) }));
const unresolvedSourceConfigIds = sourceConfigClassifications
  .filter((row) => row.matches.length === 0)
  .map((row) => row.uid);
const ambiguousSourceConfigIds = sourceConfigClassifications
  .filter((row) => row.matches.length > 1)
  .map((row) => ({ uid: row.uid, selected_uid_kind: row.uidKind, matches: row.matches }));

const result = {
  schema_version: 1,
  generated_by: "tools/rdps-current-component-bridge.mjs",
  game_build: String(origin.game_build),
  policy: {
    origin_ledger_is_current_build_static_authority: true,
    component_ids_are_typed_by_exact_decoded_table_membership: true,
    unresolved_or_ambiguous_ids_are_preserved: true,
    relationship_proof_does_not_enable_rdps_without_formula_and_packet_gates: true,
  },
  inputs: {
    origin_ledger: relativePath(originPath),
    decoded_root: relativePath(decodedRoot),
  },
  summary: {
    owner_skills: (origin.skills || []).length,
    component_routes: routeCount,
    unique_component_ids: routesByEffect.size,
    source_rules: Object.keys(sourcesByRuleId).length,
    uid_kind_counts: uidKindCounts,
    unresolved_ids: unresolvedIds.length,
    ambiguous_ids: ambiguousIds.length,
    unique_source_config_ids: sourceConfigIds.size,
    unresolved_source_config_ids: unresolvedSourceConfigIds.length,
    ambiguous_source_config_ids: ambiguousSourceConfigIds.length,
  },
  unresolved_ids: unresolvedIds,
  ambiguous_ids: ambiguousIds,
  unresolved_source_config_ids: unresolvedSourceConfigIds,
  ambiguous_source_config_ids: ambiguousSourceConfigIds,
  source_config_classifications: sourceConfigClassifications,
  sourcesByRuleId,
};

writeFileSync(outputPath, `${JSON.stringify(result, null, 2)}\n`, "utf8");
console.log(JSON.stringify(result.summary));

function compactRoute(skill, route) {
  return {
    skill_id: integer(skill.skill_id),
    skill_name: String(skill.name || `Aoyi ${skill.skill_id}`),
    item_id: integer(skill.item_id),
    monster_id: integer(skill.monster_id),
    component_id: String(route.component_id || "unlabeled-component"),
    role: String(route.role || "unresolved-role"),
    effect_ids: numbers(route.effect_ids),
    source_config_ids: numbers(route.source_config_ids),
    recipient_scope: String(route.recipient_scope || "unresolved-recipient-scope"),
    rdps_disposition: String(route.rdps_disposition || "unresolved-rdps-disposition"),
    proof_state: String(route.proof_state || "unresolved-proof-state"),
  };
}

function classifyUid(uid) {
  const matches = [
    ...(attributeVariants.get(Number(uid)) || []),
    ...tables
    .filter((table) => hasRow(table.rows, uid))
    .map((table) => ({ uid_kind: table.uidKind, table: table.file })),
  ];
  const kinds = [...new Set(matches.map((match) => match.uid_kind))];
  return {
    uidKind: kinds[0] || "unresolved",
    matches,
  };
}

function sourceConfigEdges(uid, route) {
  const classification = classifyUid(uid);
  const matches = classification.matches.length > 0
    ? classification.matches
    : [{ uid_kind: "unresolved", table: null }];
  const definitions = new Map();
  for (const match of matches) {
    const key = `${match.uid_kind}:${match.table || "unresolved"}`;
    definitions.set(key, match);
  }
  return [...definitions.values()].map((match) => ({
    edgeKind: "source-config-row",
    uidKind: match.uid_kind,
    uid,
    role: "source",
    source: "current-aoyi-rdps-origin-ledger.component_routes",
    sourceTable: match.table,
    status: classification.matches.length > 1
      ? "exact-current-build-component-source-config-multiple-definitions-preserved"
      : classification.matches.length === 0
        ? "unresolved"
        : "exact-current-build-component-source-config",
    relationshipKind: route.component_id,
  }));
}

function indexAttributeVariants(table) {
  const variants = new Map();
  const fields = ["AttrFinal", "AttrTotal", "AttrAdd", "AttrExAdd", "AttrPer", "AttrExPer"];
  for (const row of objectRows(table)) {
    const baseUid = Number(row?.Id ?? row?.id);
    if (!Number.isSafeInteger(baseUid) || baseUid <= 0) continue;
    for (const field of fields) {
      const uid = Number(row?.[field]);
      if (!Number.isSafeInteger(uid) || uid <= 0) continue;
      const matches = variants.get(uid) || [];
      matches.push({
        uid_kind: "attribute",
        table: "FightAttrTable.json",
        attribute_family_id: baseUid,
        attribute_family_name: String(row.OfficialName || row.EnumName || row.Name || baseUid),
        attribute_lane: field,
      });
      variants.set(uid, matches);
    }
  }
  return variants;
}

function objectRows(value) {
  return Array.isArray(value) ? value : Object.values(value || {});
}

function hasRow(table, uid) {
  if (Array.isArray(table)) {
    return table.some((row) => Number(row?.Id ?? row?.id) === Number(uid));
  }
  return Boolean(table && typeof table === "object" && Object.hasOwn(table, String(uid)));
}

function componentSourceName(effectId, routes) {
  const owners = [...new Set(routes.map((route) => route.skill_name))].sort();
  return `${owners.join(" / ")} component ${effectId}`;
}

function uniqueEdges(edges) {
  const rows = new Map();
  for (const edge of edges) {
    const normalized = Object.fromEntries(Object.entries(edge).filter(([, value]) => value !== null && value !== undefined));
    const key = JSON.stringify(normalized);
    rows.set(key, normalized);
  }
  return [...rows.values()].sort((left, right) => JSON.stringify(left).localeCompare(JSON.stringify(right)));
}

function numbers(values) {
  return [...new Set((values || []).map(Number).filter((value) => Number.isSafeInteger(value) && value > 0))]
    .sort((left, right) => left - right);
}

function integer(value) {
  const number = Number(value);
  return Number.isSafeInteger(number) && number > 0 ? number : null;
}

function validateBuild() {
  if (String(origin.game_build) !== String(options.gameBuild)) {
    throw new Error(`origin ledger build ${origin.game_build} differs from requested build ${options.gameBuild}`);
  }
}

function parseArgs(argv) {
  const parsed = {};
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (!argument.startsWith("--") || index + 1 >= argv.length) throw new Error(`Invalid argument: ${argument}`);
    parsed[argument.slice(2)] = argv[index + 1];
    index += 1;
  }
  for (const name of ["originLedger", "decodedRoot", "output", "gameBuild"]) {
    if (!parsed[name]) throw new Error(`--${name} is required`);
  }
  return parsed;
}

function readJson(file, label) {
  if (!existsSync(file)) throw new Error(`${label} does not exist: ${file}`);
  return JSON.parse(readFileSync(file, "utf8"));
}

function resolvePath(value) {
  return path.isAbsolute(value) ? value : path.resolve(repoRoot, value);
}

function relativePath(value) {
  return path.relative(repoRoot, value).replaceAll("\\", "/");
}
