#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "generate") generate(options);
else if (command === "verify") verify(options);
else if (command === "diff") diff(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function generate(options) {
  const graphPath = resolvePath(required(options, "graph"));
  const candidatePath = resolvePath(required(options, "candidates"));
  const proofPath = resolvePath(required(options, "callsite-proofs"));
  const dumpPath = resolvePath(required(options, "il2cpp-dump"));
  const outputPath = resolvePath(required(options, "output"));
  const build = required(options, "build");
  if (!/^\d+$/.test(build)) throw new Error("Build must contain only ASCII digits");

  const graph = readJson(graphPath, "decoded reference graph");
  const proof = readJson(proofPath, "current-build callsite proof");
  requireBuild(graph, build, "decoded reference graph");
  requireBuild(proof, build, "current-build callsite proof");
  if (![5, 6].includes(graph.schema_version)) {
    throw new Error("Decoded reference graph schema_version must be 5 or 6");
  }
  if (proof.schema_version !== 3) throw new Error("Callsite proof schema_version must be 3");

  const candidates = readJsonLines(candidatePath, "reference-candidate ledger");
  const candidateByKey = new Map(candidates.map((row) => [row.semantic_field_key, row]));
  const proofByKey = new Map(proof.fields.map((row) => [row.semantic_field_key, row]));
  const declaredByKey = groupDeclaredSchemas(graph.exact_field_schemas);
  const dumpSchema = parseIl2CppDump(readFileSync(dumpPath, "utf8"));

  const fields = graph.semantic_field_registry.map((registry) => {
    const candidate = candidateByKey.get(registry.key) ?? null;
    const callsite = proofByKey.get(registry.key) ?? null;
    const declared = declaredByKey.get(registry.key) ?? [];
    const il2cpp = resolveIl2CppField(dumpSchema, registry.source_table, registry.field);
    return buildFieldLedgerRow({ registry, candidate, callsite, declared, il2cpp });
  }).sort(compareSemanticFields);

  const summary = summarize(fields);
  const output = {
    schema_version: 1,
    generated_by: "tools/bpsr-semantic-field-schema-ledger.mjs",
    game_build: String(build),
    inputs: {
      decoded_reference_graph: describeInput(graphPath),
      reference_candidates: describeInput(candidatePath),
      callsite_proofs: describeInput(proofPath),
      il2cpp_dump: describeInput(dumpPath),
    },
    policy: {
      every_decoded_semantic_field_preserved: true,
      unresolved_identifier_fields_hidden: false,
      numeric_namespace_overlap_is_relationship_proof: false,
      schema_domain_closure_requires_unique_full_coverage_and_exact_name_alignment: true,
      schema_domain_closure_is_distinct_from_binary_callsite_proof: true,
      future_patch_diff_key: "source_table/field/path_pattern",
    },
    summary,
    fields,
  };
  output.semantic_sha256 = semanticHash(output, ["generated_at", "semantic_sha256"]);
  writeFileSync(outputPath, `${JSON.stringify(output, null, 2)}\n`, "utf8");
  console.log(JSON.stringify(summary, null, 2));
  console.log(`Semantic field schema ledger written: ${outputPath}`);
}

function verify(options) {
  const inputPath = resolvePath(required(options, "input"));
  const value = readJson(inputPath, "semantic field schema ledger");
  verifyLedger(value);
  const unresolved = value.fields.filter((row) => row.resolution_state === "open").length;
  console.log(`Semantic field schema ledger verified: ${value.fields.length} fields, ${unresolved} open.`);
}

function verifyLedger(value) {
  if (value.schema_version !== 1) throw new Error("Semantic field schema ledger schema_version must be 1");
  if (value.generated_by !== "tools/bpsr-semantic-field-schema-ledger.mjs") {
    throw new Error("Unexpected semantic field schema ledger generator");
  }
  const expected = semanticHash(value, ["generated_at", "semantic_sha256"]);
  if (value.semantic_sha256 !== expected) {
    throw new Error(`Semantic field schema ledger hash mismatch: stored ${value.semantic_sha256}, computed ${expected}`);
  }
  if (!Array.isArray(value.fields) || value.fields.length !== value.summary?.semantic_field_groups) {
    throw new Error("Semantic field schema ledger summary does not cover every field row");
  }
  const keys = new Set();
  for (const row of value.fields) {
    if (keys.has(row.key)) throw new Error(`Duplicate semantic field key: ${row.key}`);
    keys.add(row.key);
    if (!row.evidence_state || !row.resolution_state) throw new Error(`Incomplete ledger row: ${row.key}`);
  }
  const unresolved = value.fields.filter((row) => row.resolution_state === "open").length;
  if (unresolved !== value.summary.open_field_groups) throw new Error("Open-field count mismatch");
}

function diff(options) {
  const baselinePath = resolvePath(required(options, "baseline"));
  const candidatePath = resolvePath(required(options, "candidate"));
  const outputPath = resolvePath(required(options, "output"));
  const baseline = readJson(baselinePath, "baseline semantic field ledger");
  const candidate = readJson(candidatePath, "candidate semantic field ledger");
  verifyLedger(baseline);
  verifyLedger(candidate);
  const baselineRows = new Map(baseline.fields.map((row) => [row.key, row]));
  const candidateRows = new Map(candidate.fields.map((row) => [row.key, row]));
  const added = [];
  const removed = [];
  const changed = [];
  for (const [key, row] of candidateRows) {
    const prior = baselineRows.get(key);
    if (!prior) added.push(compactDiffRow(row));
    else if (prior.semantic_sha256 !== row.semantic_sha256) {
      changed.push({
        key,
        baseline: compactDiffRow(prior),
        candidate: compactDiffRow(row),
        changes: changedProperties(prior, row),
      });
    }
  }
  for (const [key, row] of baselineRows) {
    if (!candidateRows.has(key)) removed.push(compactDiffRow(row));
  }
  const result = {
    schema_version: 1,
    generated_by: "tools/bpsr-semantic-field-schema-ledger.mjs",
    baseline_build: String(baseline.game_build),
    candidate_build: String(candidate.game_build),
    inputs: { baseline: describeInput(baselinePath), candidate: describeInput(candidatePath) },
    policy: {
      changed_fields_route_only_affected_semantic_domains: true,
      unchanged_fields_require_reproof: false,
      unresolved_evidence_hidden: false,
    },
    summary: { added: added.length, removed: removed.length, changed: changed.length },
    added: added.sort(compareKeys),
    removed: removed.sort(compareKeys),
    changed: changed.sort(compareKeys),
  };
  result.semantic_sha256 = semanticHash(result, ["generated_at", "semantic_sha256"]);
  writeFileSync(outputPath, `${JSON.stringify(result, null, 2)}\n`, "utf8");
  console.log(JSON.stringify(result.summary, null, 2));
}

function buildFieldLedgerRow({ registry, candidate, callsite, declared, il2cpp }) {
  const provenTargets = [...new Set(callsite?.proven_target_tables ?? [])].sort();
  const fullCoverage = (candidate?.target_candidates ?? [])
    .filter((entry) => entry.all_distinct_values_resolve && entry.all_nonzero_occurrences_resolve);
  const exactNameTargets = fullCoverage.filter((entry) => entry.name_alignment?.score === 100);
  let evidenceState;
  let resolutionState;
  let acceptedTargets = [];
  let openReason = null;

  if (declared.length > 0) {
    evidenceState = "declared-reference";
    resolutionState = "closed";
    acceptedTargets = declared.map((entry) => entry.target_table).sort();
  } else if (provenTargets.length > 0) {
    evidenceState = "current-build-binary-callsite-proven-reference";
    resolutionState = "closed";
    acceptedTargets = provenTargets;
  } else if (registry.classification === "row-primary-key") {
    evidenceState = "decoded-storage-key-proven-primary-key";
    resolutionState = "closed";
  } else if (registry.classification !== "reference-like-unproven") {
    evidenceState = registry.classification;
    resolutionState = "closed";
  } else if (registry.nonzero_occurrences === 0) {
    evidenceState = "dormant-zero-only-identifier";
    resolutionState = "open";
    openReason = "No nonzero current-build values exist to establish a target namespace or runtime meaning.";
  } else if (fullCoverage.length === 1 && exactNameTargets.length === 1) {
    evidenceState = "schema-domain-closed-reference";
    resolutionState = "closed";
    acceptedTargets = [exactNameTargets[0].target_table];
  } else if (fullCoverage.length > 1) {
    evidenceState = "namespace-ambiguous-identifier";
    resolutionState = "open";
    openReason = `${fullCoverage.length} tables fully cover the observed values; numeric overlap cannot identify the relationship.`;
  } else if (fullCoverage.length === 1) {
    evidenceState = "unique-full-coverage-reference-needs-semantic-proof";
    resolutionState = "open";
    openReason = `All values exist in ${fullCoverage[0].target_table}, but field/table semantics or binary use are not yet proven.`;
  } else if ((candidate?.values_without_any_table_match ?? 0) > 0) {
    evidenceState = "no-complete-decoded-table-target";
    resolutionState = "open";
    openReason = `${candidate.values_without_any_table_match} distinct values have no decoded-table key match; enum, bitmask, composite, external, or missing-schema meaning remains to prove.`;
  } else if ((candidate?.target_candidates?.length ?? 0) > 0) {
    evidenceState = "partial-namespace-coverage-needs-schema";
    resolutionState = "open";
    openReason = "Observed values partially overlap decoded table namespaces, but no target covers the complete field domain.";
  } else {
    evidenceState = "identifier-domain-needs-schema";
    resolutionState = "open";
    openReason = "No decoded target namespace or binary relationship is currently proven.";
  }

  const row = {
    key: registry.key,
    source_table: registry.source_table,
    field: registry.field,
    path_pattern: registry.path_pattern,
    resolution_state: resolutionState,
    evidence_state: evidenceState,
    accepted_target_tables: acceptedTargets,
    open_reason: openReason,
    decoded_value_profile: {
      row_count: registry.row_count,
      numeric_occurrences: registry.numeric_occurrences,
      zero_occurrences: registry.zero_occurrences,
      nonzero_occurrences: registry.nonzero_occurrences,
      distinct_value_count: registry.distinct_value_count,
      minimum: registry.minimum,
      maximum: registry.maximum,
      value_shapes: registry.value_shapes,
      storage_key_matches: registry.storage_key_matches,
      storage_key_mismatches: registry.storage_key_mismatches,
      semantic_sha256: registry.semantic_sha256,
    },
    il2cpp_schema: il2cpp,
    declared_relationships: declared,
    binary_callsite_proof: compactCallsite(callsite),
    candidate_analysis: compactCandidate(candidate),
  };
  row.semantic_sha256 = semanticHash(row, ["semantic_sha256"]);
  return row;
}

function parseIl2CppDump(text) {
  const classes = new Map();
  let current = null;
  let section = null;
  let pendingRva = null;
  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.trim();
    const classMatch = line.match(/^(?:public|private|protected|internal).*?\bclass\s+([A-Za-z0-9_`]+)\b/);
    if (classMatch) {
      current = { name: classMatch[1], properties: new Map(), getters: new Map() };
      classes.set(current.name, current);
      section = null;
      pendingRva = null;
      continue;
    }
    if (!current) continue;
    if (line === "// Properties") { section = "properties"; continue; }
    if (line === "// Methods") { section = "methods"; continue; }
    if (line.startsWith("// RVA:")) {
      const rva = line.match(/RVA:\s*0x([0-9A-Fa-f]+)/);
      pendingRva = rva ? Number.parseInt(rva[1], 16) : null;
      continue;
    }
    if (section === "properties") {
      const match = line.match(/^(?:public|private|protected|internal)\s+(.+?)\s+([A-Za-z_][A-Za-z0-9_]*)\s*\{\s*get;/);
      if (match) current.properties.set(match[2], { property_type: normalizeType(match[1]) });
    } else if (section === "methods" && pendingRva !== null) {
      const match = line.match(/^(?:public|private|protected|internal)\s+(.+?)\s+get_([A-Za-z_][A-Za-z0-9_]*)\(\)/);
      if (match) {
        current.getters.set(match[2], {
          return_type: normalizeType(match[1]),
          getter_rva: pendingRva,
          getter_rva_hex: `0x${pendingRva.toString(16).toUpperCase()}`,
          getter_name: `${current.name}.get_${match[2]}()`,
        });
      }
      pendingRva = null;
    }
  }
  return classes;
}

function resolveIl2CppField(classes, table, field) {
  const candidates = [`${table}Base`, table.endsWith("Table") ? `${table}Base` : `${table}TableBase`];
  let classSchema = null;
  for (const name of candidates) {
    if (classes.has(name)) { classSchema = classes.get(name); break; }
  }
  if (!classSchema) return { class_found: false, property_found: false, getter_found: false };
  const property = classSchema.properties.get(field) ?? null;
  const getter = classSchema.getters.get(field) ?? null;
  return {
    class_found: true,
    class_name: classSchema.name,
    property_found: property !== null,
    property_type: property?.property_type ?? null,
    value_shape: classifyIl2CppType(property?.property_type ?? getter?.return_type ?? null),
    getter_found: getter !== null,
    ...getter,
  };
}

function compactCallsite(row) {
  if (!row) return null;
  return {
    source_getter_found: row.source_getter_found,
    promotion_state: row.promotion_state,
    corroborated_target_tables: row.corroborated_target_tables,
    proven_target_tables: row.proven_target_tables,
    candidate_proofs: (row.candidate_proofs ?? []).map((entry) => ({
      target_table: entry.target_table,
      namespace_coverage: entry.namespace_coverage,
      name_alignment: entry.name_alignment,
      source_getter_rvas: entry.source_getter_rvas,
      source_getter_rva_aliases: entry.source_getter_rva_aliases,
      current_build_shared_consumer_corroborated: entry.current_build_shared_consumer_corroborated,
      current_build_target_lookup_proven: entry.current_build_target_lookup_proven,
      current_build_target_lookup_dataflow_proofs: entry.current_build_target_lookup_dataflow_proofs,
    })),
  };
}

function compactCandidate(row) {
  if (!row) return null;
  return {
    nonzero_occurrences: row.nonzero_occurrences,
    distinct_value_count: row.distinct_value_count,
    values_without_any_table_match: row.values_without_any_table_match,
    full_coverage_candidate_tables: row.full_coverage_candidate_tables,
    missing_value_examples: row.missing_value_examples,
    candidate_sha256: row.candidate_sha256,
    target_candidates: row.target_candidates,
  };
}

function summarize(fields) {
  const byEvidence = {};
  const il2cpp = { class_found: 0, property_found: 0, getter_found: 0 };
  for (const row of fields) {
    byEvidence[row.evidence_state] = (byEvidence[row.evidence_state] ?? 0) + 1;
    if (row.il2cpp_schema.class_found) il2cpp.class_found += 1;
    if (row.il2cpp_schema.property_found) il2cpp.property_found += 1;
    if (row.il2cpp_schema.getter_found) il2cpp.getter_found += 1;
  }
  return {
    semantic_field_groups: fields.length,
    closed_field_groups: fields.filter((row) => row.resolution_state === "closed").length,
    open_field_groups: fields.filter((row) => row.resolution_state === "open").length,
    accepted_reference_field_groups: fields.filter((row) => row.accepted_target_tables.length > 0).length,
    evidence_states: Object.fromEntries(Object.entries(byEvidence).sort(([a], [b]) => a.localeCompare(b))),
    il2cpp_schema_coverage: il2cpp,
  };
}

function groupDeclaredSchemas(schemas) {
  const grouped = new Map();
  for (const schema of schemas) {
    const key = `${schema.source_table}/${schema.field}`;
    if (!grouped.has(key)) grouped.set(key, []);
    grouped.get(key).push({
      target_table: schema.target_table,
      relationship: schema.relationship,
      value_shape: schema.value_shape,
      missing_target_classification: schema.missing_target_classification ?? null,
      missing_target_blocks_mechanics: schema.missing_target_blocks_mechanics ?? null,
    });
  }
  return grouped;
}

function compactDiffRow(row) {
  return {
    key: row.key,
    source_table: row.source_table,
    field: row.field,
    path_pattern: row.path_pattern,
    resolution_state: row.resolution_state,
    evidence_state: row.evidence_state,
    accepted_target_tables: row.accepted_target_tables,
    open_reason: row.open_reason,
    decoded_semantic_sha256: row.decoded_value_profile?.semantic_sha256 ?? null,
    il2cpp_property_type: row.il2cpp_schema?.property_type ?? null,
    getter_rva: row.il2cpp_schema?.getter_rva ?? null,
    semantic_sha256: row.semantic_sha256,
  };
}

function changedProperties(a, b) {
  const pairs = [
    ["resolution_state", a.resolution_state, b.resolution_state],
    ["evidence_state", a.evidence_state, b.evidence_state],
    ["accepted_target_tables", a.accepted_target_tables, b.accepted_target_tables],
    ["decoded_value_profile", a.decoded_value_profile?.semantic_sha256, b.decoded_value_profile?.semantic_sha256],
    ["il2cpp_property_type", a.il2cpp_schema?.property_type, b.il2cpp_schema?.property_type],
    ["getter_rva", a.il2cpp_schema?.getter_rva, b.il2cpp_schema?.getter_rva],
  ];
  return pairs.filter(([, left, right]) => JSON.stringify(left) !== JSON.stringify(right)).map(([name]) => name);
}

function classifyIl2CppType(type) {
  if (!type) return "unknown";
  if (/Array$|\[\]$/.test(type)) return "array";
  if (/Dictionary|Map/.test(type)) return "map";
  if (/^bool$/.test(type)) return "boolean";
  if (/^(?:u?int|long|ulong|short|ushort|byte|sbyte|float|double|decimal)/i.test(type)) return "scalar";
  if (/string|MLString/i.test(type)) return "text";
  return "object-or-enum";
}

function normalizeType(type) {
  return type.replace(/\s+/g, " ").trim();
}

function semanticHash(value, excludedKeys = []) {
  const excluded = new Set(excludedKeys);
  // Hash the exact JSON data model that is persisted. In-memory objects can
  // contain `undefined` values (and arrays can contain undefined slots), while
  // JSON serialization drops object properties and converts array slots to
  // null. Normalizing first keeps generate and verify byte-for-byte semantic.
  const normalized = JSON.parse(JSON.stringify(value));
  return createHash("sha256").update(stableStringify(normalized, excluded)).digest("hex");
}

function stableStringify(value, excluded) {
  if (Array.isArray(value)) return `[${value.map((item) => stableStringify(item, excluded)).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value)
      .filter((key) => !excluded.has(key) && value[key] !== undefined)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${stableStringify(value[key], excluded)}`)
      .join(",")}}`;
  }
  return JSON.stringify(value);
}

function describeInput(file) {
  return { path: file, bytes: statSync(file).size, sha256: sha256(file) };
}

function sha256(file) {
  return createHash("sha256").update(readFileSync(file)).digest("hex");
}

function readJson(file, label) {
  if (!existsSync(file)) throw new Error(`${label} does not exist: ${file}`);
  return JSON.parse(readFileSync(file, "utf8"));
}

function readJsonLines(file, label) {
  if (!existsSync(file)) throw new Error(`${label} does not exist: ${file}`);
  const text = readFileSync(file, "utf8").trim();
  return text ? text.split(/\r?\n/).map((line, index) => {
    try { return JSON.parse(line); }
    catch (error) { throw new Error(`${label} line ${index + 1}: ${error.message}`); }
  }) : [];
}

function requireBuild(value, build, label) {
  if (String(value.game_build ?? "") !== String(build)) {
    throw new Error(`${label} is build ${value.game_build ?? "<missing>"}, expected ${build}`);
  }
}

function parseArgs(args) {
  const result = {};
  for (let index = 0; index < args.length; index += 1) {
    const item = args[index];
    if (!item.startsWith("--")) throw new Error(`Unexpected argument: ${item}`);
    const key = item.slice(2).replaceAll("-", "_");
    const next = args[index + 1];
    if (next === undefined || next.startsWith("--")) result[key] = true;
    else { result[key] = next; index += 1; }
  }
  return result;
}

function required(options, name) {
  const key = name.replaceAll("-", "_");
  const value = options[key];
  if (value === undefined || value === true || value === "") throw new Error(`Missing --${name}`);
  return String(value);
}

function resolvePath(file) {
  return path.resolve(process.cwd(), file);
}

function compareSemanticFields(a, b) {
  return a.source_table.localeCompare(b.source_table) || a.field.localeCompare(b.field) || a.path_pattern.localeCompare(b.path_pattern);
}

function compareKeys(a, b) {
  return a.key.localeCompare(b.key);
}

function selfTest() {
  const dump = `
public class ExampleTableBase : ZTableRow<int> {
  // Properties
  public int Id { get; }
  public Int32Array SkillIds { get; }
  // Methods
  // RVA: 0x123 Offset: 0 VA: 0
  public Int32Array get_SkillIds() { }
}`;
  const classes = parseIl2CppDump(dump);
  const schema = resolveIl2CppField(classes, "ExampleTable", "SkillIds");
  assert(schema.class_found && schema.property_found && schema.getter_found, "IL2CPP property/getter discovery");
  assert(schema.property_type === "Int32Array" && schema.getter_rva === 0x123, "IL2CPP type/RVA preservation");
  const row = buildFieldLedgerRow({
    registry: {
      key: "ExampleTable/SkillIds", source_table: "ExampleTable", field: "SkillIds", path_pattern: "/SkillIds",
      classification: "reference-like-unproven", row_count: 1, numeric_occurrences: 2, zero_occurrences: 0,
      nonzero_occurrences: 2, distinct_value_count: 2, minimum: 1, maximum: 2, value_shapes: ["array"],
      storage_key_matches: 0, storage_key_mismatches: 0, semantic_sha256: "fixture",
    },
    candidate: {
      nonzero_occurrences: 2, distinct_value_count: 2, values_without_any_table_match: 0,
      full_coverage_candidate_tables: 1, missing_value_examples: [], candidate_sha256: "fixture",
      target_candidates: [{ target_table: "SkillTable", all_distinct_values_resolve: true,
        all_nonzero_occurrences_resolve: true, name_alignment: { score: 100, kind: "exact-normalized-stem" } }],
    },
    callsite: null, declared: [], il2cpp: schema,
  });
  assert(row.evidence_state === "schema-domain-closed-reference", "schema-domain closure classification");
  assert(row.accepted_target_tables[0] === "SkillTable", "schema-domain accepted target");
  console.log("bpsr-semantic-field-schema-ledger self-test passed");
}

function assert(condition, label) {
  if (!condition) throw new Error(`Self-test failed: ${label}`);
}

function usage(exitCode) {
  console.log(`Usage:
  node tools/bpsr-semantic-field-schema-ledger.mjs generate --build BUILD --graph FILE --candidates FILE --callsite-proofs FILE --il2cpp-dump FILE --output FILE
  node tools/bpsr-semantic-field-schema-ledger.mjs verify --input FILE
  node tools/bpsr-semantic-field-schema-ledger.mjs diff --baseline FILE --candidate FILE --output FILE
  node tools/bpsr-semantic-field-schema-ledger.mjs self-test`);
  process.exit(exitCode);
}
