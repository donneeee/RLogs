#!/usr/bin/env node

import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCHEMA_VERSION = 1;
const GENERATOR = "tools/lucky-packet-component-proof.mjs";
const FAMILIES = new Set(["AttackLucky", "MAttackLucky"]);
const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);
if (command === "build") buildCommand(options);
else if (command === "verify") verifyCommand(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function buildCommand(parsed) {
  const build = numericString(required(parsed, "build"), "build");
  const activationPath = resolvePath(required(parsed, "activation-index"));
  const ledgerPath = resolvePath(required(parsed, "damage-ledger"));
  const outputPath = resolvePath(required(parsed, "output"));
  const report = buildReport(
    build,
    readJson(activationPath, "damage activation index"),
    readJson(ledgerPath, "damage resolution ledger"),
    {
      activation_index: fileDescriptor(activationPath),
      damage_resolution_ledger: fileDescriptor(ledgerPath),
    },
  );
  report.content_sha256 = contentHash(report);
  verifyReport(report);
  mkdirSync(path.dirname(outputPath), { recursive: true });
  writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  console.log(`wrote ${outputPath}`);
}

function buildReport(build, activation, ledger, inputs) {
  if (Number(activation?.schema_version) !== 1 ||
    activation?.generated_by !== "rlogs-bpsr-damage-attr-proof-compact" ||
    String(activation?.game_build) !== build || String(activation?.packet_build) !== build ||
    activation?.policy?.exact_packet_observation_index_only !== true ||
    activation?.policy?.static_identity_does_not_prove_transfer !== true ||
    activation?.policy?.unresolved_evidence_hidden !== false) {
    throw new Error("damage activation index is not exact-build fail-closed evidence");
  }
  if (Number(ledger?.schema_version) !== 2 ||
    ledger?.generated_by !== "rlogs-bpsr-damage-resolution-ledger" ||
    String(ledger?.game_build) !== build || ledger?.policy?.unresolved_evidence_hidden !== false ||
    ledger?.policy?.static_formula_is_runtime_authority !== false) {
    throw new Error("damage resolution ledger is not exact-build fail-closed evidence");
  }

  const ledgerRows = (ledger.entries || []).filter((entry) =>
    FAMILIES.has(String(entry?.formula?.family || entry?.formula?.candidate?.damage_script || "")));
  const ledgerByDamageId = new Map();
  for (const entry of ledgerRows) {
    const damageAttrId = positiveInteger(entry?.damage_attr_id, "ledger damage_attr_id");
    if (ledgerByDamageId.has(damageAttrId)) {
      throw new Error(`duplicate Lucky ledger row ${damageAttrId}`);
    }
    const family = String(entry?.formula?.family || entry?.formula?.candidate?.damage_script || "");
    if (entry?.formula?.state !== "nonstandard-or-missing" ||
      entry?.readiness !== "blocked-formula" || !FAMILIES.has(family) ||
      !String(entry?.formula?.formula_signature_id || "") ||
      !Array.isArray(entry?.source?.routes) || entry.source.routes.length === 0) {
      throw new Error(`Lucky ledger row ${damageAttrId} has an unsafe formula or source state`);
    }
    ledgerByDamageId.set(damageAttrId, entry);
  }

  const rows = [];
  for (const observed of activation.observed_damage_rows || []) {
    const family = String(observed?.damage_script || "");
    const results = nonnegativeInteger(observed?.packet_damage_results, "packet_damage_results");
    if (!FAMILIES.has(family) || results === 0) continue;
    const damageAttrId = positiveInteger(observed?.damage_id, "activation damage_id");
    const entry = ledgerByDamageId.get(damageAttrId);
    if (!entry) throw new Error(`packet-observed Lucky row ${damageAttrId} is absent from the ledger`);
    const ledgerFamily = String(entry?.formula?.family || entry?.formula?.candidate?.damage_script || "");
    const semantic = observed?.semantic_row;
    const shape = packetShape(observed?.packet_damage_value_shape);
    if (ledgerFamily !== family || Number(semantic?.Id) !== damageAttrId ||
      String(semantic?.DamageScript || "") !== family || shape.results !== results ||
      shape.with_normal_value !== 0 || shape.with_lucky_value !== results ||
      shape.with_both_values !== 0 || shape.amount_matches_lucky_value !== results ||
      shape.lucky_flag_true !== results) {
      throw new Error(`packet-observed Lucky row ${damageAttrId} is not a dedicated conserved component`);
    }
    rows.push({
      damage_attr_id: damageAttrId,
      lookup_key: String(entry.lookup_key),
      ability_id: positiveInteger(entry.ability_id, "ability_id"),
      hit_event_id: positiveInteger(entry.hit_event_id, "hit_event_id"),
      formula_family: family,
      formula_signature_id: String(entry.formula.formula_signature_id),
      original_ledger_source_state: String(entry.source.state),
      original_route_key_resolution_state: String(entry.source.route_key_resolution_state),
      static_routes: structuredClone(entry.source.routes),
      packet_damage_results: results,
      packet_damage_value_shape: shape,
      packet_damage_source_actor_kinds: countMap(observed.packet_damage_source_actor_kinds),
      packet_damage_target_actor_kinds: countMap(observed.packet_damage_target_actor_kinds),
      same_build_packet_occurrence_proven: true,
      packet_component_identity: "canonical-amount-equals-lucky-value",
      nonstandard_formula_semantics_proven: false,
      physical_defense_dependency_proven: false,
      magic_defense_dependency_proven: false,
      formula_authority: false,
      runtime_attribution_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    });
  }
  rows.sort((left, right) => left.damage_attr_id - right.damage_attr_id);

  const observedIds = new Set(rows.map((row) => row.damage_attr_id));
  const unobserved = [...ledgerByDamageId.values()]
    .filter((entry) => !observedIds.has(Number(entry.damage_attr_id)))
    .map((entry) => ({
      damage_attr_id: Number(entry.damage_attr_id),
      lookup_key: String(entry.lookup_key),
      ability_id: Number(entry.ability_id),
      hit_event_id: Number(entry.hit_event_id),
      formula_family: String(entry.formula?.family || entry.formula?.candidate?.damage_script || ""),
      formula_signature_id: String(entry.formula?.formula_signature_id || ""),
      same_build_packet_occurrence_proven: false,
      preserved_as_unobserved: true,
      formula_authority: false,
      provider_rdps_credit_allowed: false,
    }))
    .sort((left, right) => left.damage_attr_id - right.damage_attr_id);

  const families = [...FAMILIES].sort().map((family) => {
    const familyRows = rows.filter((row) => row.formula_family === family);
    return {
      formula_family: family,
      packet_observed_rows: familyRows.length,
      packet_damage_results: sum(familyRows.map((row) => row.packet_damage_results)),
      explicit_lucky_value_exact_matches: sum(
        familyRows.map((row) => row.packet_damage_value_shape.amount_matches_lucky_value),
      ),
      same_build_packet_occurrence_proven: familyRows.length > 0,
      formula_semantics_proven: false,
      physical_or_magic_mitigation_route_proven: false,
      provider_rdps_credit_allowed: false,
    };
  });

  return {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATOR,
    game_build: build,
    packet_build: build,
    policy: {
      exact_numeric_ids_and_build_are_authoritative: true,
      packet_amount_equals_lucky_value_is_authoritative_component_identity: true,
      static_route_and_packet_occurrence_do_not_prove_nonstandard_formula_semantics: true,
      physical_or_magic_mitigation_route_is_not_inferred_from_damage_script_name: true,
      unobserved_static_rows_are_preserved: true,
      packet_absence_is_not_zero: true,
      unresolved_evidence_is_hidden: false,
      packet_component_identity_authority: true,
      damage_formula_authority: false,
      runtime_attribution_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    inputs,
    summary: {
      ledger_lucky_rows: ledgerRows.length,
      packet_observed_rows: rows.length,
      unobserved_ledger_rows: unobserved.length,
      packet_damage_results: sum(rows.map((row) => row.packet_damage_results)),
      explicit_lucky_value_exact_matches: sum(
        rows.map((row) => row.packet_damage_value_shape.amount_matches_lucky_value),
      ),
      packet_component_conservation_proven: true,
      nonstandard_formula_semantics_proven: false,
      physical_or_magic_mitigation_route_proven: false,
      formula_authority: false,
      runtime_attribution_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    families,
    rows,
    unobserved_ledger_rows: unobserved,
    blockers: [
      "AttackLucky-and-MAttackLucky-server-operator-semantics-unproven",
      "physical-versus-magic-mitigation-route-unproven",
      "provider-window-magnitude-order-rounding-and-conservation-required-before-rdps-credit",
    ],
  };
}

function verifyCommand(parsed) {
  const input = resolvePath(required(parsed, "input"));
  const report = readJson(input, "Lucky packet component proof");
  verifyReport(report);
  console.log(`verified ${input}`);
}

function verifyReport(report) {
  const rows = report?.rows || [];
  const unobserved = report?.unobserved_ledger_rows || [];
  const total = sum(rows.map((row) => row.packet_damage_results));
  const matches = sum(rows.map((row) => row.packet_damage_value_shape?.amount_matches_lucky_value));
  if (Number(report?.schema_version) !== SCHEMA_VERSION || report?.generated_by !== GENERATOR ||
    !/^\d+$/.test(String(report?.game_build || "")) ||
    String(report?.packet_build) !== String(report?.game_build) ||
    report?.content_sha256 !== contentHash(report) ||
    report?.policy?.exact_numeric_ids_and_build_are_authoritative !== true ||
    report?.policy?.packet_amount_equals_lucky_value_is_authoritative_component_identity !== true ||
    report?.policy?.static_route_and_packet_occurrence_do_not_prove_nonstandard_formula_semantics !== true ||
    report?.policy?.physical_or_magic_mitigation_route_is_not_inferred_from_damage_script_name !== true ||
    report?.policy?.unobserved_static_rows_are_preserved !== true ||
    report?.policy?.packet_absence_is_not_zero !== true ||
    report?.policy?.unresolved_evidence_is_hidden !== false ||
    report?.policy?.packet_component_identity_authority !== true ||
    report?.policy?.damage_formula_authority !== false ||
    report?.policy?.runtime_attribution_authority !== false ||
    report?.policy?.ui_display_authority !== false ||
    report?.policy?.provider_rdps_credit_allowed !== false ||
    !Array.isArray(rows) || rows.length === 0 ||
    rows.some((row) => !FAMILIES.has(String(row?.formula_family || "")) ||
      Number(row?.packet_damage_results) <= 0 ||
      Number(row?.packet_damage_value_shape?.results) !== Number(row.packet_damage_results) ||
      Number(row?.packet_damage_value_shape?.with_normal_value) !== 0 ||
      Number(row?.packet_damage_value_shape?.with_lucky_value) !== Number(row.packet_damage_results) ||
      Number(row?.packet_damage_value_shape?.with_both_values) !== 0 ||
      Number(row?.packet_damage_value_shape?.amount_matches_lucky_value) !== Number(row.packet_damage_results) ||
      row?.same_build_packet_occurrence_proven !== true ||
      row?.packet_component_identity !== "canonical-amount-equals-lucky-value" ||
      row?.nonstandard_formula_semantics_proven !== false ||
      row?.physical_defense_dependency_proven !== false ||
      row?.magic_defense_dependency_proven !== false ||
      row?.formula_authority !== false || row?.runtime_attribution_authority !== false ||
      row?.ui_display_authority !== false || row?.provider_rdps_credit_allowed !== false) ||
    unobserved.some((row) => row?.same_build_packet_occurrence_proven !== false ||
      row?.preserved_as_unobserved !== true || row?.formula_authority !== false ||
      row?.provider_rdps_credit_allowed !== false) ||
    Number(report?.summary?.ledger_lucky_rows) !== rows.length + unobserved.length ||
    Number(report?.summary?.packet_observed_rows) !== rows.length ||
    Number(report?.summary?.unobserved_ledger_rows) !== unobserved.length ||
    Number(report?.summary?.packet_damage_results) !== total ||
    Number(report?.summary?.explicit_lucky_value_exact_matches) !== matches || total !== matches ||
    report?.summary?.packet_component_conservation_proven !== true ||
    report?.summary?.nonstandard_formula_semantics_proven !== false ||
    report?.summary?.physical_or_magic_mitigation_route_proven !== false ||
    report?.summary?.formula_authority !== false ||
    report?.summary?.runtime_attribution_authority !== false ||
    report?.summary?.ui_display_authority !== false ||
    report?.summary?.provider_rdps_credit_allowed !== false ||
    !Array.isArray(report?.families) || report.families.length !== 2 ||
    sum(report.families.map((family) => family.packet_damage_results)) !== total ||
    !Array.isArray(report?.blockers) || report.blockers.length !== 3) {
    throw new Error("Lucky packet component proof violates its schema or fail-closed contract");
  }
}

function selfTest() {
  const activation = {
    schema_version: 1,
    generated_by: "rlogs-bpsr-damage-attr-proof-compact",
    game_build: "1",
    packet_build: "1",
    policy: {
      exact_packet_observation_index_only: true,
      static_identity_does_not_prove_transfer: true,
      unresolved_evidence_hidden: false,
    },
    observed_damage_rows: [{
      damage_id: "2203110503",
      damage_script: "MAttackLucky",
      packet_damage_results: 2,
      packet_damage_value_shape: {
        results: 2, with_normal_value: 0, with_lucky_value: 2, with_both_values: 0,
        amount_matches_lucky_value: 2, lucky_flag_true: 2,
      },
      packet_damage_source_actor_kinds: { player: 2 },
      packet_damage_target_actor_kinds: { monster: 2 },
      semantic_row: { Id: 2203110503, DamageScript: "MAttackLucky" },
    }],
  };
  const formula = {
    state: "nonstandard-or-missing",
    family: "MAttackLucky",
    formula_signature_id: "formula-test",
    candidate: { damage_script: "MAttackLucky" },
  };
  const base = {
    lookup_key: "2031105:3", ability_id: 2031105, hit_event_id: 3,
    source: { state: "static-route-requires-packet-source", route_key_resolution_state: "pending", routes: [{ owner_table: "BuffTable", owner_id: 2031105 }] },
    formula, readiness: "blocked-formula",
  };
  const ledger = {
    schema_version: 2,
    generated_by: "rlogs-bpsr-damage-resolution-ledger",
    game_build: "1",
    policy: { unresolved_evidence_hidden: false, static_formula_is_runtime_authority: false },
    entries: [
      { ...base, damage_attr_id: 2203110503 },
      { ...base, lookup_key: "2031106:3", ability_id: 2031106, damage_attr_id: 2203110603 },
    ],
  };
  const report = buildReport("1", activation, ledger, {
    activation_index: { path: "a.json", bytes: 1, sha256: "a".repeat(64) },
    damage_resolution_ledger: { path: "b.json", bytes: 1, sha256: "b".repeat(64) },
  });
  report.content_sha256 = contentHash(report);
  verifyReport(report);
  if (report.summary.packet_observed_rows !== 1 || report.summary.unobserved_ledger_rows !== 1 ||
    report.summary.packet_damage_results !== 2) throw new Error("self-test conservation failure");
  const unsafe = structuredClone(report);
  unsafe.rows[0].physical_defense_dependency_proven = true;
  unsafe.content_sha256 = contentHash(unsafe);
  let rejected = false;
  try { verifyReport(unsafe); } catch { rejected = true; }
  if (!rejected) throw new Error("self-test accepted invented mitigation semantics");
  console.log("lucky-packet-component-proof self-test passed");
}

function packetShape(value) {
  return Object.fromEntries([
    "results", "with_normal_value", "with_lucky_value", "with_both_values",
    "amount_matches_lucky_value", "lucky_flag_true",
  ].map((key) => [key, nonnegativeInteger(value?.[key], key)]));
}

function countMap(value) {
  return Object.fromEntries(Object.entries(value || {})
    .map(([key, count]) => [String(key), nonnegativeInteger(count, key)])
    .filter(([, count]) => count > 0)
    .sort(([left], [right]) => left.localeCompare(right)));
}

function fileDescriptor(file) {
  const bytes = statSync(file).size;
  return { path: relative(file), bytes, sha256: createHash("sha256").update(readFileSync(file)).digest("hex") };
}

function contentHash(value) {
  const copy = structuredClone(value);
  delete copy.content_sha256;
  return createHash("sha256").update(stableStringify(copy)).digest("hex");
}

function stableStringify(value) {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  if (value && typeof value === "object") return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`;
  return JSON.stringify(value);
}

function parseArgs(args) {
  const result = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index];
    const value = args[index + 1];
    if (!key?.startsWith("--") || value === undefined) throw new Error(`expected --key value arguments; received ${args.join(" ")}`);
    result[key.slice(2)] = value;
  }
  return result;
}

function required(value, name) { if (!value?.[name]) throw new Error(`missing --${name}`); return value[name]; }
function resolvePath(value) { return path.isAbsolute(value) ? value : path.resolve(repoRoot, value); }
function relative(value) { return path.relative(repoRoot, value).replaceAll("\\", "/"); }
function readJson(file, label) { try { return JSON.parse(readFileSync(file, "utf8")); } catch (error) { throw new Error(`failed to read ${label} at ${file}: ${error.message}`); } }
function numericString(value, label) { if (!/^\d+$/.test(String(value))) throw new Error(`${label} must be numeric`); return String(value); }
function positiveInteger(value, label) { const number = Number(value); if (!Number.isSafeInteger(number) || number <= 0) throw new Error(`${label} must be a positive integer`); return number; }
function nonnegativeInteger(value, label) { const number = Number(value ?? 0); if (!Number.isSafeInteger(number) || number < 0) throw new Error(`${label} must be a nonnegative integer`); return number; }
function sum(values) { return values.reduce((total, value) => total + Number(value || 0), 0); }
function usage(exitCode) { console.log("Usage:\n  node tools/lucky-packet-component-proof.mjs build --build <id> --activation-index <json> --damage-ledger <json> --output <json>\n  node tools/lucky-packet-component-proof.mjs verify --input <json>\n  node tools/lucky-packet-component-proof.mjs self-test"); process.exit(exitCode); }
