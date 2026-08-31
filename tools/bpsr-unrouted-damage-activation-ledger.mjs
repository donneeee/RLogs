#!/usr/bin/env node

import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, "..");
const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

const combatReferenceTables = new Set([
  "BuffTable",
  "BulletTable",
  "SkillDataTable",
  "SkillEffectTable",
  "SkillFightLevelTable",
  "SkillTable",
]);

if (command === "generate") generate(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function generate(options) {
  const buildRoot = resolvePath(required(options, "buildRoot"));
  const damageLedgerPath = path.join(buildRoot, "damage-resolution-ledger.v2.json");
  const referenceScanPath = path.join(buildRoot, "decoded-table-reference-scan.v3.json");
  const packetScopePath = resolvePath(options.packetScope ?? path.join(
    "plugins", "games", "blue-protocol-star-resonance", "research", "runtime-evidence",
    "global", "steam-24252055", "packet-observed-damage-scope.v1.json",
  ));
  const outputPath = resolvePath(options.output ?? path.join(buildRoot, "unrouted-damage-activation-ledger.v1.json"));
  for (const file of [damageLedgerPath, referenceScanPath, packetScopePath]) requireFile(file);

  const damageLedger = readJson(damageLedgerPath);
  const referenceScan = readJson(referenceScanPath);
  const packetScope = readJson(packetScopePath);
  const buildId = String(damageLedger.game_build);
  const unresolved = damageLedger.entries.filter((entry) => entry.source?.state === "unresolved");
  const referencesByKey = indexReferences(referenceScan.references ?? []);
  const observedAbilities = new Set((packetScope.observed_ability_ids ?? []).map(Number));
  const observedLookupKeys = new Set((packetScope.exact_lookup_keys ?? []).map(String));

  const entries = unresolved
    .map((entry) => classify(entry, referencesByKey.get(entry.lookup_key) ?? [], observedAbilities, observedLookupKeys, packetScope))
    .sort(compareEntries);
  const report = {
    schema_version: 1,
    generated_by: "tools/bpsr-unrouted-damage-activation-ledger.mjs",
    game: "blue-protocol-star-resonance",
    deployment: "global",
    channel: "steam",
    game_build: buildId,
    packet_evidence_build: String(packetScope.packet_build ?? "unknown"),
    policy: {
      current_build_definition_is_not_runtime_activation_proof: true,
      historical_non_observation_does_not_prove_dead_code: true,
      historical_packet_observation_does_not_prove_current_build_behavior: true,
      raw_numeric_collisions_are_preserved_but_never_promoted_as_source_routes: true,
      only_exact_combat_table_references_are_relationship_review_leads: true,
      route_less_unobserved_self_only_definitions_remain_visible_and_diffable: true,
      dormant_definitions_never_feed_runtime_rdps: true,
    },
    summary: summarize(entries),
    entries,
    inputs: {
      damage_resolution_ledger: relativeRepo(damageLedgerPath),
      decoded_table_reference_scan: relativeRepo(referenceScanPath),
      historical_packet_scope: relativeRepo(packetScopePath),
    },
  };
  writeJson(outputPath, report);
  console.log(`Unrouted damage activation ledger for build ${buildId}: ${entries.length} definitions.`);
  console.log(`Packet-observed unresolved definitions: ${report.summary.packet_observed_unresolved}.`);
  console.log(`Current combat-reference reviews: ${report.summary.current_combat_reference_reviews}.`);
  console.log(`Definition-only and unobserved: ${report.summary.definition_only_unobserved}.`);
  console.log(`Exact relationship blockers: ${report.summary.exact_relationship_blockers}.`);
  console.log(`Wrote ${relativeRepo(outputPath)}`);
}

function classify(entry, references, observedAbilities, observedLookupKeys, packetScope) {
  const selfReferences = references.filter((reference) => reference.table === "DamageAttrTable");
  const recountReferences = references.filter((reference) => reference.table === "RecountTable");
  const combatReferences = references.filter((reference) => combatReferenceTables.has(reference.table));
  const collisionReferences = references.filter((reference) =>
    reference.table !== "DamageAttrTable"
    && reference.table !== "RecountTable"
    && !combatReferenceTables.has(reference.table));
  const exactLookupObserved = observedLookupKeys.has(String(entry.lookup_key));
  const abilityObserved = observedAbilities.has(Number(entry.ability_id));
  const hasHistoricalPacketObservation = exactLookupObserved || abilityObserved;
  const hasCurrentCombatReference = combatReferences.length > 0;

  let activationStatus = "definition-only-unobserved-in-indexed-packet-corpus";
  if (hasHistoricalPacketObservation) activationStatus = "historical-packet-observed-current-route-unresolved";
  else if (hasCurrentCombatReference) activationStatus = "current-combat-reference-route-unresolved-unobserved";

  return {
    lookup_key: String(entry.lookup_key),
    ability_id: Number(entry.ability_id),
    hit_event_id: Number(entry.hit_event_id),
    damage_attr_id: Number(entry.damage_attr_id),
    damage_name: entry.formula?.rule?.name ?? null,
    damage_script: entry.formula?.rule?.damage_script ?? null,
    formula_state: entry.formula?.state ?? null,
    recount_state: entry.recount?.state ?? null,
    recount_owners: entry.recount?.owners ?? [],
    activation_status: activationStatus,
    has_historical_packet_observation: hasHistoricalPacketObservation,
    historical_exact_lookup_observed: exactLookupObserved,
    historical_ability_observed: abilityObserved,
    historical_packet_build: String(packetScope.packet_build ?? "unknown"),
    has_current_combat_reference_lead: hasCurrentCombatReference,
    current_build_source_route_proven: false,
    blocks_exact_current_build_relationship:
      (hasHistoricalPacketObservation || hasCurrentCombatReference),
    references: {
      combat_relationship_review_leads: normalizeReferences(combatReferences),
      numeric_collision_leads: normalizeReferences(collisionReferences),
      damage_definition_references: normalizeReferences(selfReferences),
      client_recount_references: normalizeReferences(recountReferences),
    },
    reference_counts: {
      combat_relationship_review_leads: combatReferences.length,
      numeric_collision_leads: collisionReferences.length,
      damage_definition_references: selfReferences.length,
      client_recount_references: recountReferences.length,
    },
  };
}

function indexReferences(references) {
  const index = new Map();
  for (const reference of references) {
    for (const lookupKey of reference.lookup_keys ?? []) {
      const values = index.get(String(lookupKey)) ?? [];
      values.push(reference);
      index.set(String(lookupKey), values);
    }
  }
  return index;
}

function normalizeReferences(references) {
  return references.map((reference) => ({
    value: reference.value,
    table: reference.table,
    row_key: reference.row_key,
    json_pointer: reference.json_pointer,
    containing_object_pointer: reference.containing_object_pointer,
    matched_roles: reference.matched_roles ?? [],
    value_encoding: reference.value_encoding ?? null,
  })).sort((left, right) => referenceKey(left).localeCompare(referenceKey(right)));
}

function referenceKey(reference) {
  return [reference.table, reference.row_key, reference.json_pointer, reference.value].join(":");
}

function summarize(entries) {
  const statusCounts = Object.fromEntries(
    [...new Set(entries.map((entry) => entry.activation_status))]
      .sort()
      .map((status) => [status, entries.filter((entry) => entry.activation_status === status).length]),
  );
  return {
    unresolved_static_route_definitions: entries.length,
    packet_observed_unresolved: entries.filter((entry) => entry.has_historical_packet_observation).length,
    current_combat_reference_reviews: entries.filter((entry) =>
      !entry.has_historical_packet_observation && entry.has_current_combat_reference_lead).length,
    definition_only_unobserved: entries.filter((entry) =>
      entry.activation_status === "definition-only-unobserved-in-indexed-packet-corpus").length,
    exact_relationship_blockers: entries.filter((entry) => entry.blocks_exact_current_build_relationship).length,
    activation_status_counts: statusCounts,
  };
}

function compareEntries(left, right) {
  return left.ability_id - right.ability_id || left.hit_event_id - right.hit_event_id || left.damage_attr_id - right.damage_attr_id;
}

function selfTest() {
  const refs = [
    { table: "DamageAttrTable", lookup_keys: ["1:2"] },
    { table: "SkillTable", lookup_keys: ["1:2"] },
    { table: "ItemTable", lookup_keys: ["1:2"] },
  ];
  const index = indexReferences(refs);
  assert(index.get("1:2").length === 3, "Reference index lost evidence");
  const value = classify({
    lookup_key: "1:2", ability_id: 1, hit_event_id: 2, damage_attr_id: 3,
    formula: { state: "standard-static-candidate", rule: { damage_script: "Attack" } },
    recount: { state: "no-parent", owners: [] },
  }, index.get("1:2"), new Set(), new Set(), { packet_build: "old" });
  assert(value.activation_status === "current-combat-reference-route-unresolved-unobserved", "Combat reference was not retained as a review blocker");
  assert(value.reference_counts.numeric_collision_leads === 1, "Numeric collision evidence was not retained separately");
  console.log("bpsr-unrouted-damage-activation-ledger self-test passed");
}

function parseArgs(args) {
  const output = {};
  for (let index = 0; index < args.length; index += 1) {
    const token = args[index];
    if (!token.startsWith("--")) throw new Error(`Unexpected argument ${token}`);
    const key = token.slice(2).replace(/-([a-z])/g, (_, letter) => letter.toUpperCase());
    const next = args[index + 1];
    if (!next || next.startsWith("--")) throw new Error(`Missing value for ${token}`);
    output[key] = next;
    index += 1;
  }
  return output;
}

function required(value, key) { if (!value[key]) throw new Error(`Missing --${key}`); return value[key]; }
function resolvePath(value) { return path.isAbsolute(value) ? value : path.resolve(repoRoot, value); }
function readJson(file) { return JSON.parse(readFileSync(file, "utf8")); }
function requireFile(file) { if (!existsSync(file)) throw new Error(`Missing required input ${file}`); }
function writeJson(file, value) { mkdirSync(path.dirname(file), { recursive: true }); writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`, "utf8"); }
function relativeRepo(file) { return path.relative(repoRoot, file).replaceAll("\\", "/"); }
function assert(condition, message) { if (!condition) throw new Error(message); }
function usage(exitCode) {
  console.log("Usage: node tools/bpsr-unrouted-damage-activation-ledger.mjs generate --build-root <path> [--packet-scope <path>] [--output <path>]");
  process.exit(exitCode);
}
