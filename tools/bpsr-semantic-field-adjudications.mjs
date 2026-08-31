#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "build") build(resolveContext(options));
else if (command === "verify") verify(path.resolve(required(options, "input")));
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function resolveContext(parsed) {
  const buildId = required(parsed, "build");
  if (!/^\d+$/.test(buildId)) throw new Error("Build must contain only ASCII digits");
  return {
    build: buildId,
    semanticSchema: path.resolve(required(parsed, "semantic-schema")),
    decodedRoot: path.resolve(required(parsed, "decoded-root")),
    rules: path.resolve(required(parsed, "rules")),
    output: path.resolve(required(parsed, "output")),
  };
}

function build(context) {
  const started = performance.now();
  requireFile(context.semanticSchema, "semantic field schema");
  requireFile(context.rules, "semantic field adjudication rules");
  const schema = readJson(context.semanticSchema, "semantic field schema");
  const rules = readJson(context.rules, "semantic field adjudication rules");
  if (String(schema.game_build) !== context.build) throw new Error("Semantic field schema build mismatch");
  if (rules.schema_version !== 1 || !Array.isArray(rules.rules)) throw new Error("Adjudication rules schema_version must be 1");

  const adjudications = rules.rules.map((rule) => adjudicateRule(rule, schema, context.decodedRoot));
  const report = {
    schema_version: 1,
    generated_by: "tools/bpsr-semantic-field-adjudications.mjs",
    game_build: context.build,
    policy: {
      search_reduction_only: true,
      never_promotes_relationships: true,
      source_occurrences_retained: true,
      failed_proof_keeps_field_actionable: true,
      rules_reproved_per_build: true,
      zero_hidden_omissions: true,
    },
    inputs: {
      semantic_field_schema: fileDescriptor(context.semanticSchema),
      rules: fileDescriptor(context.rules),
      decoded_tables: [...new Set(rules.rules.flatMap((rule) => [rule.source.table, rule.corroboration.table]))]
        .sort(compareText)
        .map((table) => fileDescriptor(path.join(context.decodedRoot, `${table}.json`))),
    },
    summary: {
      rules: adjudications.length,
      proved_non_actionable_fields: adjudications.filter((item) => item.proof_passed && item.disposition === "non-actionable-for-exact-output-routing").length,
      failed_rules: adjudications.filter((item) => !item.proof_passed).length,
      retained_source_occurrences: adjudications.reduce((sum, item) => sum + item.source_profile.occurrences, 0),
      hidden_occurrences: 0,
    },
    adjudications,
  };
  report.content_sha256 = contentHash(report);
  mkdirSync(path.dirname(context.output), { recursive: true });
  writeFileSync(context.output, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  verify(context.output);
  console.log(
    `Semantic field adjudications built for ${context.build}: ${report.summary.proved_non_actionable_fields} fields proved non-actionable, ` +
    `${report.summary.retained_source_occurrences} source occurrences retained, ${report.summary.failed_rules} failed rules in ${Math.round(performance.now() - started)} ms.`,
  );
}

function adjudicateRule(rule, schema, decodedRoot) {
  validateRule(rule);
  const semantic = schema.fields.find((field) => field.key === rule.semantic_field_key);
  if (!semantic) throw new Error(`Semantic field is absent: ${rule.semantic_field_key}`);
  if (semantic.source_table !== rule.source.table || semantic.field !== rule.source.field) {
    throw new Error(`Semantic field source mismatch for ${rule.semantic_field_key}`);
  }
  const sourceFile = path.join(decodedRoot, `${rule.source.table}.json`);
  const corroborationFile = path.join(decodedRoot, `${rule.corroboration.table}.json`);
  requireFile(sourceFile, rule.source.table);
  requireFile(corroborationFile, rule.corroboration.table);
  const sourceRows = Object.values(readJson(sourceFile, rule.source.table));
  const corroborationRows = Object.values(readJson(corroborationFile, rule.corroboration.table));
  const sourceValues = sourceRows.flatMap((row) => scalarValues(row[rule.source.field]));
  const corroboratingValues = corroborationRows.flatMap((row) => scalarValues(row[rule.corroboration.field]));
  const sourceDistinct = distinctSorted(sourceValues);
  const corroboratingDistinct = distinctSorted(corroboratingValues);
  const sentinelValues = new Set((rule.source.sentinel_values ?? []).map(String));
  const sourceNonSentinel = sourceValues.filter((value) => !sentinelValues.has(value));
  const sourceNonSentinelDistinct = distinctSorted(sourceNonSentinel);
  const sourceSentinelValues = sourceValues.filter((value) => sentinelValues.has(value));
  const missingFromMembership = sourceDistinct.filter((value) => !corroboratingDistinct.includes(value));
  const missingNonSentinelFromIdentity = sourceNonSentinelDistinct.filter((value) => !corroboratingDistinct.includes(value));
  const orphanMembership = corroboratingDistinct.filter((value) => !sourceDistinct.includes(value));
  const valueCounts = sourceDistinct.map((value) => ({ value, occurrences: sourceValues.filter((item) => item === value).length }));
  const corroboratingValueCounts = corroboratingDistinct.map((value) => ({
    value,
    occurrences: corroboratingValues.filter((item) => item === value).length,
  }));
  const requiredCorroborationFields = rule.corroboration.required_fields ?? [];
  const corroborationRowsMissingRequiredFields = corroborationRows
    .map((row) => ({
      row_id: String(row[rule.corroboration.row_identity_field]),
      missing_fields: requiredCorroborationFields.filter((field) => row[field] === undefined || row[field] === null),
    }))
    .filter((item) => item.missing_fields.length > 0);
  const binary = semantic.binary_callsite_proof ?? {};
  const proofChecks = {
    "semantic-field-has-no-accepted-target": (semantic.accepted_target_tables ?? []).length === 0 && (semantic.declared_relationships ?? []).length === 0,
    "semantic-field-has-no-proven-binary-target": (binary.proven_target_tables ?? []).length === 0 && (binary.corroborated_target_tables ?? []).length === 0,
    "source-values-are-non-injective": sourceRows.length > sourceDistinct.length && valueCounts.some((item) => item.occurrences > 1),
    "source-values-equal-corroborating-membership-union": missingFromMembership.length === 0 && orphanMembership.length === 0 && sourceDistinct.length > 0,
    "source-non-sentinel-values-contained-in-corroborating-identity-set":
      sourceNonSentinelDistinct.length > 0 && missingNonSentinelFromIdentity.length === 0,
    "corroborating-identity-values-are-unique":
      corroboratingValues.length > 0 && corroboratingValueCounts.every((item) => item.occurrences === 1),
    "corroborating-required-fields-present":
      requiredCorroborationFields.length > 0 && corroborationRowsMissingRequiredFields.length === 0,
  };
  const unknownProofs = rule.required_proofs.filter((name) => !(name in proofChecks));
  if (unknownProofs.length > 0) throw new Error(`Unknown required proofs for ${rule.semantic_field_key}: ${unknownProofs.join(", ")}`);
  const proofPassed = rule.required_proofs.every((name) => proofChecks[name]);
  return {
    semantic_field_key: rule.semantic_field_key,
    semantic_role: rule.semantic_role,
    requested_disposition: rule.disposition,
    disposition: proofPassed ? rule.disposition : "actionable-proof-failed",
    proof_passed: proofPassed,
    required_proofs: rule.required_proofs.map((name) => ({ name, passed: proofChecks[name] })),
    source: rule.source,
    corroboration: rule.corroboration,
    source_profile: {
      rows: sourceRows.length,
      occurrences: sourceValues.length,
      distinct_values: sourceDistinct,
      sentinel_values: [...sentinelValues].sort(compareIdentifiers),
      sentinel_occurrences: sourceSentinelValues.length,
      non_sentinel_occurrences: sourceNonSentinel.length,
      non_sentinel_distinct_values: sourceNonSentinelDistinct,
      value_counts: valueCounts,
      storage_key_matches: Number(semantic.decoded_value_profile?.storage_key_matches ?? 0),
      storage_key_mismatches: Number(semantic.decoded_value_profile?.storage_key_mismatches ?? 0),
    },
    corroboration_profile: {
      rows: corroborationRows.length,
      occurrences: corroboratingValues.length,
      distinct_values: corroboratingDistinct,
      value_counts: corroboratingValueCounts,
      missing_source_values: missingFromMembership,
      missing_non_sentinel_source_values: missingNonSentinelFromIdentity,
      orphan_membership_values: orphanMembership,
      required_fields: requiredCorroborationFields,
      rows_missing_required_fields: corroborationRowsMissingRequiredFields,
      representative_rows: corroborationRows
        .filter((row) => scalarValues(row[rule.corroboration.field]).length > 0)
        .slice(0, 8)
        .map((row) => ({
          row_id: String(row[rule.corroboration.row_identity_field]),
          values: scalarValues(row[rule.corroboration.field]),
        })),
    },
    semantic_evidence: {
      resolution_state: semantic.resolution_state,
      evidence_state: semantic.evidence_state,
      accepted_target_tables: semantic.accepted_target_tables ?? [],
      declared_relationships: semantic.declared_relationships ?? [],
      binary_promotion_state: binary.promotion_state ?? null,
      proven_target_tables: binary.proven_target_tables ?? [],
      corroborated_target_tables: binary.corroborated_target_tables ?? [],
      il2cpp_schema: semantic.il2cpp_schema ?? null,
      semantic_sha256: semantic.semantic_sha256,
    },
    acceleration_effect: proofPassed
      ? "exclude-this-field-from-exact-damage-output-route-search-but-retain-every-occurrence"
      : "none-field-remains-actionable",
  };
}

function verify(input) {
  const report = readJson(input, "semantic field adjudications");
  if (report.schema_version !== 1) throw new Error("Adjudication schema_version must be 1");
  if (report.content_sha256 !== contentHash(report)) throw new Error("Adjudication content hash mismatch");
  if (!report.policy?.search_reduction_only || !report.policy?.source_occurrences_retained || !report.policy?.zero_hidden_omissions) {
    throw new Error("Adjudication safety policy is incomplete");
  }
  const seen = new Set();
  let passed = 0;
  let failed = 0;
  let occurrences = 0;
  for (const item of report.adjudications ?? []) {
    if (seen.has(item.semantic_field_key)) throw new Error(`Duplicate adjudication ${item.semantic_field_key}`);
    seen.add(item.semantic_field_key);
    if (!Array.isArray(item.required_proofs) || item.required_proofs.length === 0) throw new Error(`Missing proofs for ${item.semantic_field_key}`);
    if (item.proof_passed !== item.required_proofs.every((proof) => proof.passed)) throw new Error(`Proof aggregate mismatch for ${item.semantic_field_key}`);
    if (!item.proof_passed && item.disposition !== "actionable-proof-failed") throw new Error(`Failed adjudication hid ${item.semantic_field_key}`);
    if (item.proof_passed && item.disposition === "non-actionable-for-exact-output-routing") passed += 1;
    if (!item.proof_passed) failed += 1;
    occurrences += item.source_profile.occurrences;
  }
  if (report.summary.proved_non_actionable_fields !== passed || report.summary.failed_rules !== failed) throw new Error("Adjudication summary mismatch");
  if (report.summary.retained_source_occurrences !== occurrences || report.summary.hidden_occurrences !== 0) throw new Error("Occurrence conservation mismatch");
  console.log(`Semantic field adjudications verified for build ${report.game_build}: ${passed} proved, ${failed} failed, zero hidden occurrences.`);
  return report;
}

function selfTest() {
  const root = mkdtempSync(path.join(tmpdir(), "rlogs-field-adjudications-"));
  try {
    const decoded = path.join(root, "decoded");
    mkdirSync(decoded);
    const schema = path.join(root, "schema.json");
    const rules = path.join(root, "rules.json");
    const output = path.join(root, "output.json");
    writeFileSync(schema, JSON.stringify({ game_build: "1", fields: [{
      key: "Entry/DatabaseId", source_table: "Entry", field: "DatabaseId", resolution_state: "open", evidence_state: "ambiguous",
      accepted_target_tables: [], declared_relationships: [], decoded_value_profile: { storage_key_matches: 0, storage_key_mismatches: 3 },
      binary_callsite_proof: { promotion_state: "not-callsite-corroborated", proven_target_tables: [], corroborated_target_tables: [] },
      il2cpp_schema: { getter_found: true }, semantic_sha256: "test",
    }, {
      key: "Skill/SlotId", source_table: "Skill", field: "SlotId", resolution_state: "open", evidence_state: "ambiguous",
      accepted_target_tables: [], declared_relationships: [], decoded_value_profile: { storage_key_matches: 0, storage_key_mismatches: 4 },
      binary_callsite_proof: { promotion_state: "not-callsite-corroborated", proven_target_tables: [], corroborated_target_tables: [] },
      il2cpp_schema: { getter_found: true }, semantic_sha256: "test-slot",
    }] }));
    writeFileSync(path.join(decoded, "Entry.json"), JSON.stringify({ 10: { Id: 10, DatabaseId: 1 }, 11: { Id: 11, DatabaseId: 1 }, 12: { Id: 12, DatabaseId: 2 } }));
    writeFileSync(path.join(decoded, "Scene.json"), JSON.stringify({ 20: { Id: 20, DatabaseIds: [1, 2] } }));
    writeFileSync(path.join(decoded, "Skill.json"), JSON.stringify({
      1: { Id: 1, SlotId: 0 }, 2: { Id: 2, SlotId: 3 }, 3: { Id: 3, SlotId: 4 }, 4: { Id: 4, SlotId: 4 },
    }));
    writeFileSync(path.join(decoded, "Slot.json"), JSON.stringify({
      3: { Id: 3, Logic: 1, Key: 12 }, 4: { Id: 4, Logic: 1, Key: 13 }, 5: { Id: 5, Logic: 1, Key: 14 },
    }));
    writeFileSync(rules, JSON.stringify({ schema_version: 1, rules: [{
      semantic_field_key: "Entry/DatabaseId", semantic_role: "scene-membership", disposition: "non-actionable-for-exact-output-routing",
      source: { table: "Entry", field: "DatabaseId", row_identity_field: "Id" },
      corroboration: { table: "Scene", field: "DatabaseIds", row_identity_field: "Id" },
      required_proofs: ["semantic-field-has-no-accepted-target", "semantic-field-has-no-proven-binary-target", "source-values-are-non-injective", "source-values-equal-corroborating-membership-union"],
    }, {
      semantic_field_key: "Skill/SlotId", semantic_role: "slot-identity-reference", disposition: "non-actionable-for-exact-output-routing",
      source: { table: "Skill", field: "SlotId", row_identity_field: "Id", sentinel_values: ["0"] },
      corroboration: { table: "Slot", field: "Id", row_identity_field: "Id", required_fields: ["Logic", "Key"] },
      required_proofs: [
        "semantic-field-has-no-accepted-target", "semantic-field-has-no-proven-binary-target",
        "source-non-sentinel-values-contained-in-corroborating-identity-set",
        "corroborating-identity-values-are-unique", "corroborating-required-fields-present",
      ],
    }] }));
    build({ build: "1", semanticSchema: schema, decodedRoot: decoded, rules, output });
    const report = readJson(output, "self-test output");
    if (report.summary.proved_non_actionable_fields !== 2 || report.summary.retained_source_occurrences !== 7) throw new Error("Self-test adjudication failed");
    const slot = report.adjudications.find((item) => item.semantic_field_key === "Skill/SlotId");
    if (slot?.source_profile.sentinel_occurrences !== 1 || slot?.corroboration_profile.orphan_membership_values.join(",") !== "5") {
      throw new Error("Self-test exact identity join failed");
    }
    console.log("Semantic field adjudications self-test passed: membership and exact identity proofs reduce search without hiding source occurrences.");
  } finally { rmSync(root, { recursive: true, force: true }); }
}

function validateRule(rule) {
  for (const key of ["semantic_field_key", "semantic_role", "disposition", "source", "corroboration", "required_proofs"]) {
    if (rule[key] === undefined) throw new Error(`Adjudication rule lacks ${key}`);
  }
  if (!Array.isArray(rule.required_proofs) || rule.required_proofs.length === 0) throw new Error(`Adjudication rule lacks required proofs: ${rule.semantic_field_key}`);
  if (rule.source.sentinel_values !== undefined && !Array.isArray(rule.source.sentinel_values)) {
    throw new Error(`Adjudication source sentinel_values must be an array: ${rule.semantic_field_key}`);
  }
  if (rule.corroboration.required_fields !== undefined && !Array.isArray(rule.corroboration.required_fields)) {
    throw new Error(`Adjudication corroboration required_fields must be an array: ${rule.semantic_field_key}`);
  }
}
function scalarValues(value) { return Array.isArray(value) ? value.flatMap(scalarValues) : value === undefined || value === null ? [] : [String(value)]; }
function distinctSorted(values) { return [...new Set(values)].sort(compareIdentifiers); }
function compareText(a, b) { return String(a ?? "").localeCompare(String(b ?? ""), "en", { numeric: true }); }
function compareIdentifiers(a, b) { try { const x = BigInt(a); const y = BigInt(b); return x < y ? -1 : x > y ? 1 : 0; } catch { return compareText(a, b); } }
function contentHash(value) { const copy = structuredClone(value); delete copy.content_sha256; return createHash("sha256").update(JSON.stringify(copy)).digest("hex"); }
function fileDescriptor(file) { requireFile(file, "input"); const data = readFileSync(file); return { path: file.replaceAll("\\", "/"), bytes: data.length, sha256: createHash("sha256").update(data).digest("hex") }; }
function requireFile(file, label) { if (!existsSync(file)) throw new Error(`Missing ${label}: ${file}`); }
function readJson(file, label) { try { return JSON.parse(readFileSync(file, "utf8")); } catch (error) { throw new Error(`Unable to read ${label} ${file}: ${error.message}`); } }
function parseArgs(args) { const parsed = {}; for (let i = 0; i < args.length; i += 2) { const key = args[i]; const value = args[i + 1]; if (!key?.startsWith("--") || value === undefined || value.startsWith("--")) throw new Error(`Invalid argument near ${key}`); parsed[key.slice(2)] = value; } return parsed; }
function required(value, key) { if (!value[key]) throw new Error(`Missing --${key}`); return value[key]; }
function usage(exitCode) { console.log(`Usage:\n  node tools/bpsr-semantic-field-adjudications.mjs build --build <id> --semantic-schema <json> --decoded-root <dir> --rules <json> --output <json>\n  node tools/bpsr-semantic-field-adjudications.mjs verify --input <json>\n  node tools/bpsr-semantic-field-adjudications.mjs self-test`); process.exit(exitCode); }
