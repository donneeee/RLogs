#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

const [command = "help", ...rest] = process.argv.slice(2);

if (command === "diff") generate(parseOptions(rest));
else if (command === "verify") verify(parseOptions(rest));
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function generate(options) {
  const baselinePath = required(options, "baseline");
  const candidatePath = required(options, "candidate");
  const outputPath = required(options, "output");
  const baseline = readGraph(baselinePath);
  const candidate = readGraph(candidatePath);
  const report = buildDiff(baseline, candidate, {
    baseline: evidence(baselinePath),
    candidate: evidence(candidatePath),
  });
  assertReport(report);
  mkdirSync(path.dirname(path.resolve(outputPath)), { recursive: true });
  writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  console.log(summaryLine(report));
}

function verify(options) {
  const report = readJson(required(options, "input"), "decoded reference graph diff");
  assertReport(report);
  console.log(summaryLine(report));
}

function buildDiff(baseline, candidate, inputs) {
  const baselineTables = new Map(baseline.tables.map((table) => [table.table, table]));
  const candidateTables = new Map(candidate.tables.map((table) => [table.table, table]));
  const tableNames = [...new Set([...baselineTables.keys(), ...candidateTables.keys()])].sort(compareText);
  const tableChanges = [];

  for (const tableName of tableNames) {
    const before = baselineTables.get(tableName);
    const after = candidateTables.get(tableName);
    const domain = after?.domain ?? before?.domain ?? "other";
    if (!before || !after) {
      const rows = Object.keys((after ?? before).row_hashes ?? {}).sort(compareIds);
      tableChanges.push({
        table: tableName,
        domain,
        status: before ? "removed" : "added",
        baseline_rows: before?.rows ?? 0,
        candidate_rows: after?.rows ?? 0,
        baseline_semantic_sha256: before?.semantic_sha256 ?? null,
        candidate_semantic_sha256: after?.semantic_sha256 ?? null,
        added_rows: before ? [] : rows,
        removed_rows: before ? rows : [],
        changed_rows: [],
      });
      continue;
    }
    if (before.semantic_sha256 === after.semantic_sha256) continue;
    const beforeRows = before.row_hashes ?? {};
    const afterRows = after.row_hashes ?? {};
    const rowIds = [...new Set([...Object.keys(beforeRows), ...Object.keys(afterRows)])].sort(compareIds);
    const addedRows = rowIds.filter((id) => beforeRows[id] === undefined);
    const removedRows = rowIds.filter((id) => afterRows[id] === undefined);
    const changedRows = rowIds.filter((id) => beforeRows[id] !== undefined
      && afterRows[id] !== undefined && beforeRows[id] !== afterRows[id]);
    tableChanges.push({
      table: tableName,
      domain,
      status: "changed",
      baseline_rows: before.rows,
      candidate_rows: after.rows,
      baseline_semantic_sha256: before.semantic_sha256,
      candidate_semantic_sha256: after.semantic_sha256,
      added_rows: addedRows,
      removed_rows: removedRows,
      changed_rows: changedRows,
    });
  }

  const exactEdgeChanges = setDiff(baseline.exact_edges, candidate.exact_edges, edgeKey);
  const missingTargetChanges = setDiff(baseline.missing_targets, candidate.missing_targets, edgeKey);
  const ambiguousFieldChanges = compareAmbiguousFields(
    baseline.ambiguous_reference_fields,
    candidate.ambiguous_reference_fields,
  );
  const semanticFieldChanges = compareSemanticFieldRegistry(
    baseline.semantic_field_registry,
    candidate.semantic_field_registry,
  );
  const exactFieldSchemaChanges = compareExactFieldSchemas(
    baseline.exact_field_schemas,
    candidate.exact_field_schemas,
  ).map((entry) => ({
    ...entry,
    domain: tableDomainFor(entry.source_table, baseline, candidate),
  }));
  const callsiteProofChanges = compareCallsiteProofArtifacts(
    baseline.callsite_proof_artifact,
    candidate.callsite_proof_artifact,
  );
  const domainChanges = compareDomains(baseline, candidate, tableChanges);
  const affectedDomains = domainChanges.filter((entry) => entry.status !== "unchanged");

  const report = {
    schema_version: 4,
    generated_by: "tools/bpsr-decoded-reference-graph-diff.mjs",
    generated_at: new Date().toISOString(),
    baseline_build: String(baseline.game_build),
    candidate_build: String(candidate.game_build),
    inputs,
    policy: {
      decoded_rows_compared_by_stable_hash: true,
      declared_relationships_compared_exactly: true,
      missing_declared_targets_preserved: true,
      untyped_reference_groups_preserved: true,
      untyped_references_never_promoted_to_relationships: true,
      semantic_field_classifications_compared_by_evidence_hash: true,
      namespace_candidate_sets_compared_by_hash: true,
      exact_field_schemas_compared_with_build_locked_proof_metadata: true,
      callsite_proof_inputs_compared_by_content_hash: true,
      hidden_omissions: 0,
    },
    summary: {
      baseline_tables: baseline.tables.length,
      candidate_tables: candidate.tables.length,
      added_tables: tableChanges.filter((entry) => entry.status === "added").length,
      removed_tables: tableChanges.filter((entry) => entry.status === "removed").length,
      changed_tables: tableChanges.filter((entry) => entry.status === "changed").length,
      unchanged_tables: tableNames.length - tableChanges.length,
      added_rows: sum(tableChanges, "added_rows"),
      removed_rows: sum(tableChanges, "removed_rows"),
      changed_rows: sum(tableChanges, "changed_rows"),
      added_exact_edges: exactEdgeChanges.added.length,
      removed_exact_edges: exactEdgeChanges.removed.length,
      added_missing_targets: missingTargetChanges.added.length,
      removed_missing_targets: missingTargetChanges.removed.length,
      added_ambiguous_fields: ambiguousFieldChanges.filter((entry) => entry.status === "added").length,
      removed_ambiguous_fields: ambiguousFieldChanges.filter((entry) => entry.status === "removed").length,
      changed_ambiguous_fields: ambiguousFieldChanges.filter((entry) => entry.status === "changed").length,
      added_semantic_fields: semanticFieldChanges.filter((entry) => entry.status === "added").length,
      removed_semantic_fields: semanticFieldChanges.filter((entry) => entry.status === "removed").length,
      changed_semantic_fields: semanticFieldChanges.filter((entry) => entry.status === "changed").length,
      reclassified_semantic_fields: semanticFieldChanges.filter((entry) => entry.reclassified).length,
      changed_reference_candidate_sets: semanticFieldChanges.filter((entry) => entry.candidate_set_changed).length,
      added_exact_field_schemas: exactFieldSchemaChanges.filter((entry) => entry.status === "added").length,
      removed_exact_field_schemas: exactFieldSchemaChanges.filter((entry) => entry.status === "removed").length,
      changed_exact_field_schemas: exactFieldSchemaChanges.filter((entry) => entry.status === "changed").length,
      changed_current_build_callsite_proofs: exactFieldSchemaChanges.filter(
        (entry) => entry.current_build_proof_changed,
      ).length,
      callsite_proof_inputs_changed: callsiteProofChanges.changed,
      affected_semantic_domains: affectedDomains.length,
      hidden_omissions: 0,
    },
    affected_domains: affectedDomains,
    unchanged_domains: domainChanges.filter((entry) => entry.status === "unchanged").map((entry) => entry.domain),
    table_changes: tableChanges,
    exact_edge_changes: exactEdgeChanges,
    missing_target_changes: missingTargetChanges,
    semantic_field_changes: semanticFieldChanges,
    exact_field_schema_changes: exactFieldSchemaChanges,
    callsite_proof_changes: callsiteProofChanges,
    ambiguous_field_changes: ambiguousFieldChanges,
  };
  return report;
}

function tableDomainFor(tableName, baseline, candidate) {
  for (const graph of [candidate, baseline]) {
    const table = graph.tables.find((entry) => entry.table === tableName);
    if (table?.domain) return table.domain;
  }
  return "other";
}

function compareExactFieldSchemas(beforeValues = [], afterValues = []) {
  const before = new Map(beforeValues.map((entry) => [exactFieldSchemaKey(entry), entry]));
  const after = new Map(afterValues.map((entry) => [exactFieldSchemaKey(entry), entry]));
  const keys = [...new Set([...before.keys(), ...after.keys()])].sort(compareText);
  return keys.flatMap((key) => {
    const a = before.get(key);
    const b = after.get(key);
    const beforeSha = a ? semanticHash(a) : null;
    const afterSha = b ? semanticHash(b) : null;
    if (beforeSha === afterSha) return [];
    return [{
      key,
      source_table: b?.source_table ?? a?.source_table ?? null,
      field: b?.field ?? a?.field ?? null,
      status: !a ? "added" : !b ? "removed" : "changed",
      baseline_target_table: a?.target_table ?? null,
      candidate_target_table: b?.target_table ?? null,
      baseline_relationship: a?.relationship ?? null,
      candidate_relationship: b?.relationship ?? null,
      baseline_proof: a?.proof ?? "declared-field-schema",
      candidate_proof: b?.proof ?? "declared-field-schema",
      baseline_schema_sha256: beforeSha,
      candidate_schema_sha256: afterSha,
      current_build_proof_changed: Boolean(
        (a?.proof === "current-build-il2cpp-target-lookup")
          || (b?.proof === "current-build-il2cpp-target-lookup"),
      ),
    }];
  });
}

function compareCallsiteProofArtifacts(before = null, after = null) {
  const summarize = (artifact) => artifact ? {
    present: true,
    schema_version: artifact.schema_version ?? null,
    game_build: artifact.game_build ?? null,
    promoted_field_schemas: artifact.promoted_field_schemas ?? 0,
    artifact_sha256: artifact.sha256 ?? null,
    binary_sha256: artifact.inputs?.binary?.sha256 ?? null,
    dump_sha256: artifact.inputs?.dump?.sha256 ?? null,
    candidate_artifact_sha256: artifact.inputs?.candidates?.sha256 ?? null,
    lookup_method_rvas: artifact.inputs?.contains_key?.rvas ?? [],
  } : {
    present: false,
    schema_version: null,
    game_build: null,
    promoted_field_schemas: 0,
    artifact_sha256: null,
    binary_sha256: null,
    dump_sha256: null,
    candidate_artifact_sha256: null,
    lookup_method_rvas: [],
  };
  const baseline = summarize(before);
  const candidate = summarize(after);
  return {
    changed: semanticHash(baseline) !== semanticHash(candidate),
    baseline,
    candidate,
  };
}

function exactFieldSchemaKey(entry) {
  return `${entry.source_table}/${entry.field}`;
}

function semanticHash(value) {
  return createHash("sha256").update(stableStringify(value)).digest("hex");
}

function stableStringify(value) {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort(compareText).map(
      (key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`,
    ).join(",")}}`;
  }
  return JSON.stringify(value);
}

function compareSemanticFieldRegistry(beforeValues = [], afterValues = []) {
  const before = new Map(beforeValues.map((entry) => [entry.key, entry]));
  const after = new Map(afterValues.map((entry) => [entry.key, entry]));
  const keys = [...new Set([...before.keys(), ...after.keys()])].sort(compareText);
  return keys.flatMap((key) => {
    const a = before.get(key);
    const b = after.get(key);
    if (a && b
      && a.semantic_sha256 === b.semantic_sha256
      && (a.candidate_sha256 ?? null) === (b.candidate_sha256 ?? null)) return [];
    return [{
      key,
      source_table: b?.source_table ?? a?.source_table ?? null,
      field: b?.field ?? a?.field ?? null,
      path_pattern: b?.path_pattern ?? a?.path_pattern ?? null,
      status: !a ? "added" : !b ? "removed" : "changed",
      baseline_classification: a?.classification ?? null,
      candidate_classification: b?.classification ?? null,
      baseline_semantic_sha256: a?.semantic_sha256 ?? null,
      candidate_semantic_sha256: b?.semantic_sha256 ?? null,
      baseline_candidate_sha256: a?.candidate_sha256 ?? null,
      candidate_candidate_sha256: b?.candidate_sha256 ?? null,
      baseline_occurrences: a?.nonzero_occurrences ?? 0,
      candidate_occurrences: b?.nonzero_occurrences ?? 0,
      reclassified: Boolean(a && b && a.classification !== b.classification),
      candidate_set_changed: Boolean(a && b
        && (a.candidate_sha256 ?? null) !== (b.candidate_sha256 ?? null)),
    }];
  });
}

function compareDomains(baseline, candidate, tableChanges) {
  const before = new Map(baseline.domains.map((entry) => [entry.domain, entry]));
  const after = new Map(candidate.domains.map((entry) => [entry.domain, entry]));
  const changedTables = new Map();
  for (const entry of tableChanges) {
    if (!changedTables.has(entry.domain)) changedTables.set(entry.domain, []);
    changedTables.get(entry.domain).push(entry.table);
  }
  return [...new Set([...before.keys(), ...after.keys()])].sort(compareText).map((domain) => {
    const a = before.get(domain);
    const b = after.get(domain);
    const status = !a ? "added" : !b ? "removed"
      : a.semantic_sha256 === b.semantic_sha256 ? "unchanged" : "changed";
    return {
      domain,
      status,
      baseline_rows: a?.rows ?? 0,
      candidate_rows: b?.rows ?? 0,
      baseline_semantic_sha256: a?.semantic_sha256 ?? null,
      candidate_semantic_sha256: b?.semantic_sha256 ?? null,
      changed_tables: (changedTables.get(domain) ?? []).sort(compareText),
    };
  });
}

function compareAmbiguousFields(beforeObject = {}, afterObject = {}) {
  const keys = [...new Set([...Object.keys(beforeObject), ...Object.keys(afterObject)])].sort(compareText);
  return keys.flatMap((key) => {
    const before = beforeObject[key];
    const after = afterObject[key];
    if (before && after && before.occurrences === after.occurrences && before.row_count === after.row_count) return [];
    return [{
      key,
      source_table: after?.source_table ?? before?.source_table ?? null,
      field: after?.field ?? before?.field ?? null,
      status: !before ? "added" : !after ? "removed" : "changed",
      baseline_occurrences: before?.occurrences ?? 0,
      candidate_occurrences: after?.occurrences ?? 0,
      baseline_rows: before?.row_count ?? 0,
      candidate_rows: after?.row_count ?? 0,
    }];
  });
}

function setDiff(beforeValues = [], afterValues = [], keyFn) {
  const before = new Map(beforeValues.map((value) => [keyFn(value), value]));
  const after = new Map(afterValues.map((value) => [keyFn(value), value]));
  return {
    added: [...after].filter(([key]) => !before.has(key)).sort(([a], [b]) => compareText(a, b)).map(([, value]) => value),
    removed: [...before].filter(([key]) => !after.has(key)).sort(([a], [b]) => compareText(a, b)).map(([, value]) => value),
  };
}

function edgeKey(edge) {
  return JSON.stringify([
    edge.source_table, String(edge.source_id), edge.source_field, edge.source_pointer,
    edge.relationship,
    edge.target_table ?? edge.target_tables ?? null,
    edge.resolved_target_tables ?? null,
    String(edge.target_id), edge.proof,
  ]);
}

function readGraph(file) {
  const graph = readJson(file, "decoded reference graph");
  if (graph.generated_by !== "DecodedTableReferenceGraph.gen") {
    throw new Error(`${file} was not generated by DecodedTableReferenceGraph.gen`);
  }
  if (!/^\d+$/.test(String(graph.game_build))) throw new Error(`${file} has an invalid game_build`);
  for (const field of ["domains", "tables", "exact_edges", "missing_targets"]) {
    if (!Array.isArray(graph[field])) throw new Error(`${file} is missing ${field}`);
  }
  if (!graph.ambiguous_reference_fields || Array.isArray(graph.ambiguous_reference_fields)) {
    throw new Error(`${file} is missing ambiguous_reference_fields`);
  }
  if (Number(graph.schema_version) >= 2 && !Array.isArray(graph.semantic_field_registry)) {
    throw new Error(`${file} is missing semantic_field_registry`);
  }
  if (Number(graph.schema_version) >= 3 && !graph.reference_candidate_artifact) {
    throw new Error(`${file} is missing reference_candidate_artifact`);
  }
  if (Number(graph.schema_version) >= 4 && !Array.isArray(graph.exact_field_schemas)) {
    throw new Error(`${file} is missing exact_field_schemas`);
  }
  if (Number(graph.schema_version) >= 5) {
    for (const edge of [...graph.exact_edges, ...graph.missing_targets]) {
      if (!edge.target_table && !Array.isArray(edge.target_tables)) {
        throw new Error(`${file} has an edge without target_table or target_tables`);
      }
    }
  }
  return graph;
}

function assertReport(report) {
  if (report.schema_version !== 4 || report.generated_by !== "tools/bpsr-decoded-reference-graph-diff.mjs") {
    throw new Error("Unsupported decoded reference graph diff schema or generator");
  }
  if (!/^\d+$/.test(String(report.baseline_build)) || !/^\d+$/.test(String(report.candidate_build))) {
    throw new Error("Diff build identities must contain only ASCII digits");
  }
  if (report.policy?.hidden_omissions !== 0 || report.summary?.hidden_omissions !== 0) {
    throw new Error("Decoded reference graph diff contains hidden omissions");
  }
  const tableChanges = report.table_changes ?? [];
  const expected = {
    added_tables: tableChanges.filter((entry) => entry.status === "added").length,
    removed_tables: tableChanges.filter((entry) => entry.status === "removed").length,
    changed_tables: tableChanges.filter((entry) => entry.status === "changed").length,
    added_rows: sum(tableChanges, "added_rows"),
    removed_rows: sum(tableChanges, "removed_rows"),
    changed_rows: sum(tableChanges, "changed_rows"),
    added_exact_edges: report.exact_edge_changes?.added?.length ?? -1,
    removed_exact_edges: report.exact_edge_changes?.removed?.length ?? -1,
    added_missing_targets: report.missing_target_changes?.added?.length ?? -1,
    removed_missing_targets: report.missing_target_changes?.removed?.length ?? -1,
    affected_semantic_domains: report.affected_domains?.length ?? -1,
  };
  for (const [key, value] of Object.entries(expected)) {
    if (report.summary?.[key] !== value) throw new Error(`Summary mismatch for ${key}: ${report.summary?.[key]} != ${value}`);
  }
  const ambiguous = report.ambiguous_field_changes ?? [];
  for (const status of ["added", "removed", "changed"]) {
    const key = `${status}_ambiguous_fields`;
    const value = ambiguous.filter((entry) => entry.status === status).length;
    if (report.summary?.[key] !== value) throw new Error(`Summary mismatch for ${key}`);
  }
  const semanticFields = report.semantic_field_changes ?? [];
  for (const status of ["added", "removed", "changed"]) {
    const key = `${status}_semantic_fields`;
    const value = semanticFields.filter((entry) => entry.status === status).length;
    if (report.summary?.[key] !== value) throw new Error(`Summary mismatch for ${key}`);
  }
  const reclassified = semanticFields.filter((entry) => entry.reclassified).length;
  if (report.summary?.reclassified_semantic_fields !== reclassified) {
    throw new Error("Summary mismatch for reclassified_semantic_fields");
  }
  const candidateSets = semanticFields.filter((entry) => entry.candidate_set_changed).length;
  if (report.summary?.changed_reference_candidate_sets !== candidateSets) {
    throw new Error("Summary mismatch for changed_reference_candidate_sets");
  }
  const exactSchemas = report.exact_field_schema_changes ?? [];
  for (const status of ["added", "removed", "changed"]) {
    const key = `${status}_exact_field_schemas`;
    const value = exactSchemas.filter((entry) => entry.status === status).length;
    if (report.summary?.[key] !== value) throw new Error(`Summary mismatch for ${key}`);
  }
  const currentBuildProofs = exactSchemas.filter((entry) => entry.current_build_proof_changed).length;
  if (report.summary?.changed_current_build_callsite_proofs !== currentBuildProofs) {
    throw new Error("Summary mismatch for changed_current_build_callsite_proofs");
  }
  if (report.summary?.callsite_proof_inputs_changed !== report.callsite_proof_changes?.changed) {
    throw new Error("Summary mismatch for callsite_proof_inputs_changed");
  }
}

function selfTest() {
  const table = (semantic, hashes) => ({
    table: "SkillTable", domain: "skills-actions", semantic_sha256: semantic,
    rows: Object.keys(hashes).length, row_hashes: hashes,
  });
  const domain = (semantic, rows) => ({ domain: "skills-actions", semantic_sha256: semantic, rows, tables: [] });
  const base = {
    game_build: "1", tables: [table("a", { 1: "a", 2: "b" })], domains: [domain("a", 2)],
    exact_edges: [{ source_table: "SkillTable", source_id: "1", source_field: "NextSkillId", source_pointer: "/NextSkillId", relationship: "skill-next-skill", target_table: "SkillTable", target_id: "2", proof: "declared-field-schema" }],
    missing_targets: [], ambiguous_reference_fields: { "SkillTable.OwnerId": { source_table: "SkillTable", field: "OwnerId", occurrences: 1, row_count: 1 } },
    semantic_field_registry: [{ key: "SkillTable/OwnerId", source_table: "SkillTable", field: "OwnerId", classification: "reference-like-unproven", semantic_sha256: "same", candidate_sha256: "before", nonzero_occurrences: 1 }],
    exact_field_schemas: [{ source_table: "SkillTable", field: "NextSkillId", target_table: "SkillTable", relationship: "skill-next-skill", value_shape: "scalar-or-array" }],
    callsite_proof_artifact: null,
  };
  const next = structuredClone(base);
  next.game_build = "2";
  next.tables = [table("b", { 1: "c", 3: "d" })];
  next.domains = [domain("b", 2)];
  next.exact_edges = [];
  next.missing_targets = [{ ...base.exact_edges[0], target_id: "9" }];
  next.ambiguous_reference_fields["SkillTable.OwnerId"].occurrences = 2;
  next.semantic_field_registry[0].candidate_sha256 = "after";
  next.exact_field_schemas.push({
    source_table: "SkillTable", field: "OwnerId", target_table: "OwnerTable",
    relationship: "current-build-table-key-reference", value_shape: "scalar-or-array",
    proof: "current-build-il2cpp-target-lookup",
    proof_metadata: { game_build: "2", binary_sha256: "binary-two" },
  });
  next.callsite_proof_artifact = {
    schema_version: 2, game_build: "2", sha256: "proof-two", promoted_field_schemas: 1,
    inputs: { binary: { sha256: "binary-two" }, dump: { sha256: "dump-two" }, candidates: { sha256: "candidates-two" }, contains_key: { rvas: [1] } },
  };
  const report = buildDiff(base, next, { baseline: {}, candidate: {} });
  assertReport(report);
  const expected = { changed_rows: 1, added_rows: 1, removed_rows: 1, removed_exact_edges: 1, added_missing_targets: 1, affected_semantic_domains: 1 };
  for (const [key, value] of Object.entries(expected)) {
    if (report.summary[key] !== value) throw new Error(`Self-test failed for ${key}`);
  }
  if (report.summary.changed_reference_candidate_sets !== 1
    || report.summary.changed_semantic_fields !== 1
    || report.summary.added_exact_field_schemas !== 1
    || report.summary.changed_current_build_callsite_proofs !== 1
    || report.summary.callsite_proof_inputs_changed !== true) {
    throw new Error("Self-test failed to detect a namespace-candidate-only change");
  }
  console.log("Decoded reference graph diff self-test passed.");
}

function evidence(file) {
  const resolved = path.resolve(file);
  if (!existsSync(resolved)) throw new Error(`Missing input: ${resolved}`);
  return { path: resolved, sha256: createHash("sha256").update(readFileSync(resolved)).digest("hex") };
}

function readJson(file, label) {
  try { return JSON.parse(readFileSync(path.resolve(file), "utf8")); }
  catch (error) { throw new Error(`Cannot read ${label} ${file}: ${error.message}`); }
}

function parseOptions(args) {
  const options = {};
  for (let i = 0; i < args.length; i += 2) {
    const key = args[i];
    if (!key?.startsWith("--") || args[i + 1] === undefined) throw new Error(`Expected --name value, received ${key ?? "<end>"}`);
    options[key.slice(2)] = args[i + 1];
  }
  return options;
}

function required(options, key) {
  if (!options[key]) throw new Error(`Missing required --${key}`);
  return options[key];
}

function sum(entries, field) { return entries.reduce((total, entry) => total + (entry[field]?.length ?? 0), 0); }
function compareText(a, b) { return String(a).localeCompare(String(b)); }
function compareIds(a, b) { return /^\d+$/.test(a) && /^\d+$/.test(b) ? BigInt(a) < BigInt(b) ? -1 : BigInt(a) > BigInt(b) ? 1 : 0 : compareText(a, b); }
function summaryLine(report) { return `Decoded reference graph ${report.baseline_build} -> ${report.candidate_build}: ${report.summary.affected_semantic_domains} affected domains, ${report.summary.changed_tables} changed tables, ${report.summary.added_rows}/${report.summary.removed_rows}/${report.summary.changed_rows} added/removed/changed rows, zero hidden omissions.`; }
function usage(exitCode) {
  console.log(`Usage:\n  node tools/bpsr-decoded-reference-graph-diff.mjs diff --baseline <graph.json> --candidate <graph.json> --output <diff.json>\n  node tools/bpsr-decoded-reference-graph-diff.mjs verify --input <diff.json>\n  node tools/bpsr-decoded-reference-graph-diff.mjs self-test`);
  process.exitCode = exitCode;
}
