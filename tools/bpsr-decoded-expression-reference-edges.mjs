#!/usr/bin/env node

import { createHash } from "node:crypto";
import {
  existsSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "build") build(resolveBuildContext(options));
else if (command === "verify") verify(path.resolve(required(options, "input")), options["verify-sources"] !== "false");
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function resolveBuildContext(options) {
  const buildId = required(options, "build");
  if (!/^\d+$/.test(buildId)) throw new Error("Build must contain only ASCII digits");
  return {
    buildId,
    decodedRoot: path.resolve(required(options, "decoded-root")),
    referenceGraph: path.resolve(required(options, "reference-graph")),
    rules: path.resolve(required(options, "rules")),
    output: path.resolve(required(options, "output")),
  };
}

function build(context) {
  const graph = readJson(context.referenceGraph, "decoded reference graph");
  const ruleSet = readJson(context.rules, "decoded expression reference rules");
  requireBuild(graph, context.buildId, "decoded reference graph");
  if (ruleSet.schema_version !== 1 || !Array.isArray(ruleSet.rules)) {
    throw new Error("Decoded expression reference rules must use schema_version 1 and contain rules[]");
  }
  const nonReferenceFunctions = ruleSet.non_reference_functions ?? [];
  if (!Array.isArray(nonReferenceFunctions)) {
    throw new Error("Decoded expression reference rules non_reference_functions must be an array");
  }

  const tableFiles = new Map((graph.tables ?? []).map((entry) => [entry.table, entry]));
  const requiredTables = new Set();
  for (const rule of ruleSet.rules) {
    validateRule(rule);
    requiredTables.add(rule.source_table);
    requiredTables.add(rule.target_table);
  }
  for (const declaration of nonReferenceFunctions) {
    validateNonReferenceFunction(declaration);
    requiredTables.add(declaration.source_table);
  }

  const tables = new Map();
  const sources = [
    fingerprint("reference-graph", context.referenceGraph),
    fingerprint("expression-reference-rules", context.rules),
  ];
  for (const tableName of [...requiredTables].sort()) {
    const descriptor = tableFiles.get(tableName);
    if (!descriptor) throw new Error(`Rule requires table absent from reference graph: ${tableName}`);
    const file = path.join(context.decodedRoot, descriptor.file);
    requireFile(file, `decoded ${tableName}`);
    const entries = normalizeRows(readJson(file, `decoded ${tableName}`));
    if (entries.length !== descriptor.rows) {
      throw new Error(`decoded ${tableName} row mismatch: ${entries.length} != ${descriptor.rows}`);
    }
    const byId = new Map();
    for (const entry of entries) {
      if (byId.has(entry.rowId)) throw new Error(`decoded ${tableName} has duplicate row identity ${entry.rowId}`);
      byId.set(entry.rowId, entry);
    }
    tables.set(tableName, { descriptor, file, entries, byId });
    sources.push(fingerprint(`decoded:${tableName}`, file));
  }

  const exactEdges = [];
  const occurrences = [];
  const failures = [];
  const seenEdges = new Set();
  for (const rule of [...ruleSet.rules].sort((left, right) => left.id.localeCompare(right.id))) {
    const source = tables.get(rule.source_table);
    const target = tables.get(rule.target_table);
    for (const entry of source.entries) {
      if (!entry.value || typeof entry.value !== "object" || Array.isArray(entry.value)) continue;
      const fieldValue = entry.value[rule.source_field];
      if (fieldValue === undefined) continue;
      for (const textValue of collectStrings(fieldValue, `/${escapePointer(rule.source_field)}`)) {
        const parsed = parseExpressionFunctionCalls(
          textValue.value, rule.function_name, rule.expression_kind,
        );
        for (const malformed of parsed.malformed) {
          failures.push({
            rule_id: rule.id,
            source_table: rule.source_table,
            source_id: entry.rowId,
            source_field: rule.source_field,
            source_pointer: textValue.pointer,
            classification: "named-function-call-not-parseable",
            function_name: rule.function_name,
            byte_offset: malformed.byte_offset,
            excerpt: malformed.excerpt,
          });
        }
        for (const call of parsed.calls) {
          if (call.tokens.length === 0) {
            failures.push({
              rule_id: rule.id,
              source_table: rule.source_table,
              source_id: entry.rowId,
              source_field: rule.source_field,
              source_pointer: textValue.pointer,
              classification: "named-function-first-argument-has-no-identifiers",
              function_name: rule.function_name,
              byte_offset: call.byte_offset,
              expression: call.expression,
            });
          }
          for (const token of call.tokens) {
            const occurrence = {
              rule_id: rule.id,
              source_table: rule.source_table,
              source_id: entry.rowId,
              source_field: rule.source_field,
              source_pointer: textValue.pointer,
              source_row_sha256: hashText(stableStringify(entry.value)),
              relationship: rule.relationship,
              function_name: rule.function_name,
              function_argument_index: 0,
              expression: call.expression,
              byte_offset: call.byte_offset,
              candidate_target_table: rule.target_table,
              candidate_target_id: token,
            };
            occurrences.push(occurrence);
            const targetEntry = target.byId.get(token);
            if (!targetEntry) {
              failures.push({
                ...occurrence,
                classification: "exact-target-identity-absent",
              });
              continue;
            }
            const edge = {
              source_table: rule.source_table,
              source_id: entry.rowId,
              source_field: rule.source_field,
              source_pointer: textValue.pointer,
              relationship: rule.relationship,
              target_table: rule.target_table,
              target_id: token,
              proof: "current-build-typed-expression-reference",
              rule_id: rule.id,
              expression_kind: rule.expression_kind,
              function_name: rule.function_name,
              function_argument_index: 0,
              expression: call.expression,
              source_row_sha256: occurrence.source_row_sha256,
              target_row_sha256: hashText(stableStringify(targetEntry.value)),
            };
            const key = [
              edge.source_table, edge.source_id, edge.source_field, edge.source_pointer,
              edge.relationship, edge.target_table, edge.target_id, edge.proof,
            ].join("\u0000");
            if (!seenEdges.has(key)) {
              seenEdges.add(key);
              exactEdges.push(edge);
            }
          }
        }
      }
    }
  }

  const functionCoverage = new Map();
  const unclassifiedFunctions = [];
  const referenceFunctions = new Set(ruleSet.rules.map((rule) =>
    functionCoverageKey(rule.source_table, rule.source_field, rule.function_name)));
  const declaredNonReferences = new Map(nonReferenceFunctions.map((declaration) => [
    functionCoverageKey(
      declaration.source_table, declaration.source_field, declaration.function_name,
    ),
    declaration,
  ]));
  const coverageFields = new Map();
  for (const rule of ruleSet.rules) {
    coverageFields.set(`${rule.source_table}\u0000${rule.source_field}`, {
      source_table: rule.source_table,
      source_field: rule.source_field,
    });
  }
  for (const declaration of nonReferenceFunctions) {
    coverageFields.set(`${declaration.source_table}\u0000${declaration.source_field}`, {
      source_table: declaration.source_table,
      source_field: declaration.source_field,
    });
  }
  for (const field of coverageFields.values()) {
    const source = tables.get(field.source_table);
    for (const entry of source.entries) {
      if (!entry.value || typeof entry.value !== "object" || Array.isArray(entry.value)) continue;
      const fieldValue = entry.value[field.source_field];
      if (fieldValue === undefined) continue;
      for (const textValue of collectStrings(fieldValue, `/${escapePointer(field.source_field)}`)) {
        for (const call of discoverNamedFunctionCalls(textValue.value)) {
          const key = functionCoverageKey(field.source_table, field.source_field, call.function_name);
          const nonReference = declaredNonReferences.get(key);
          const disposition = referenceFunctions.has(key)
            ? "typed-reference-rule"
            : nonReference
              ? nonReference.classification
              : "unclassified";
          const coverage = functionCoverage.get(key) ?? {
            source_table: field.source_table,
            source_field: field.source_field,
            function_name: call.function_name,
            disposition,
            occurrences: 0,
            ...(nonReference ? { rationale: nonReference.rationale } : {}),
          };
          coverage.occurrences += 1;
          functionCoverage.set(key, coverage);
          if (disposition === "unclassified") {
            unclassifiedFunctions.push({
              source_table: field.source_table,
              source_id: entry.rowId,
              source_field: field.source_field,
              source_pointer: textValue.pointer,
              function_name: call.function_name,
              byte_offset: call.byte_offset,
              excerpt: textValue.value.slice(
                call.byte_offset, Math.min(textValue.value.length, call.byte_offset + 160),
              ),
              classification: "named-function-semantics-unclassified",
            });
          }
        }
      }
    }
  }

  exactEdges.sort(compareEdges);
  occurrences.sort(compareOccurrences);
  failures.sort(compareFailures);
  const functionInventory = [...functionCoverage.values()].sort((left, right) =>
    functionCoverageKey(left.source_table, left.source_field, left.function_name)
      .localeCompare(functionCoverageKey(right.source_table, right.source_field, right.function_name)));
  unclassifiedFunctions.sort(compareUnclassifiedFunctions);
  const result = {
    schema_version: 1,
    generated_by: "tools/bpsr-decoded-expression-reference-edges.mjs",
    game_build: context.buildId,
    source_files: sources.sort((left, right) => left.label.localeCompare(right.label)),
    rules: ruleSet.rules,
    non_reference_functions: nonReferenceFunctions,
    policy: {
      named_function_and_argument_position_are_rule_locked: true,
      target_identity_must_exist_in_exact_current_build_table: true,
      missing_targets_and_malformed_calls_retained: true,
      identifiers_rewritten: false,
      all_named_functions_in_rule_fields_inventoried: true,
      unclassified_named_functions_retained: true,
      unresolved_evidence_hidden: false,
    },
    summary: {
      rules: ruleSet.rules.length,
      required_tables: requiredTables.size,
      parsed_identifier_occurrences: occurrences.length,
      exact_edges: exactEdges.length,
      failures: failures.length,
      named_function_occurrences: functionInventory.reduce(
        (total, entry) => total + entry.occurrences, 0,
      ),
      unclassified_named_function_occurrences: unclassifiedFunctions.length,
      hidden_omissions: 0,
    },
    exact_edges: exactEdges,
    occurrences,
    failures,
    function_inventory: functionInventory,
    unclassified_functions: unclassifiedFunctions,
  };
  result.content_sha256 = hashText(stableStringify({ ...result, content_sha256: undefined }));
  mkdirSync(path.dirname(context.output), { recursive: true });
  writeJson(context.output, result);
  verify(context.output, true);
  console.log(
    `Decoded expression references built for ${context.buildId}: ${exactEdges.length} exact edges from ` +
    `${occurrences.length} typed occurrences; ${failures.length} failures retained.`,
  );
}

function verify(input, verifySources) {
  const value = readJson(input, "decoded expression reference edges");
  if (value.schema_version !== 1 || value.generated_by !== "tools/bpsr-decoded-expression-reference-edges.mjs") {
    throw new Error("Unexpected decoded expression reference artifact schema or generator");
  }
  if (!/^\d+$/.test(String(value.game_build))) throw new Error("Artifact game_build is invalid");
  if (!Array.isArray(value.exact_edges) || !Array.isArray(value.occurrences)
    || !Array.isArray(value.failures) || !Array.isArray(value.function_inventory)
    || !Array.isArray(value.unclassified_functions)) {
    throw new Error("Artifact must retain exact edges, occurrences, failures, and function coverage arrays");
  }
  if (value.summary.exact_edges !== value.exact_edges.length
    || value.summary.parsed_identifier_occurrences !== value.occurrences.length
    || value.summary.failures !== value.failures.length
    || value.summary.named_function_occurrences !== value.function_inventory.reduce(
      (total, entry) => total + entry.occurrences, 0,
    )
    || value.summary.unclassified_named_function_occurrences !== value.unclassified_functions.length
    || value.summary.hidden_omissions !== 0) {
    throw new Error("Artifact conservation counts do not match retained evidence");
  }
  const expectedHash = hashText(stableStringify({ ...value, content_sha256: undefined }));
  if (value.content_sha256 !== expectedHash) throw new Error("Artifact content_sha256 mismatch");
  const occurrenceKeys = new Set(value.occurrences.map((row) => [
    row.rule_id, row.source_table, row.source_id, row.source_pointer,
    row.candidate_target_table, row.candidate_target_id,
  ].join("\u0000")));
  for (const edge of value.exact_edges) {
    const key = [
      edge.rule_id, edge.source_table, edge.source_id, edge.source_pointer,
      edge.target_table, edge.target_id,
    ].join("\u0000");
    if (!occurrenceKeys.has(key)) throw new Error(`Exact edge lacks retained occurrence: ${key}`);
    if (edge.proof !== "current-build-typed-expression-reference") {
      throw new Error(`Unexpected exact edge proof ${edge.proof}`);
    }
  }
  if (verifySources) {
    for (const source of value.source_files) {
      requireFile(source.path, source.label);
      const stats = statSync(source.path);
      if (stats.size !== source.bytes || hashFile(source.path) !== source.sha256) {
        throw new Error(`Source changed after expression artifact generation: ${source.label}`);
      }
    }
  }
  console.log(`Decoded expression references verified for build ${value.game_build}: zero hidden omissions.`);
  return value;
}

function parseBraceListFunctionCalls(text, functionName) {
  const escaped = escapeRegex(functionName);
  const marker = new RegExp(`${escaped}\\s*\\(`, "g");
  const call = new RegExp(`${escaped}\\s*\\(\\s*\\{([^}]*)\\}`, "g");
  const calls = [];
  const matchedOffsets = new Set();
  for (const match of text.matchAll(call)) {
    matchedOffsets.add(match.index);
    const rawTokens = match[1].split(",").map((token) => token.trim()).filter(Boolean);
    const tokens = [];
    for (const token of rawTokens) {
      if (!/^\d+$/.test(token)) continue;
      const numeric = Number(token);
      if (!Number.isSafeInteger(numeric) || numeric < 0) continue;
      tokens.push(String(numeric));
    }
    calls.push({
      byte_offset: match.index,
      expression: match[0],
      tokens,
    });
  }
  const malformed = [];
  for (const match of text.matchAll(marker)) {
    if (!matchedOffsets.has(match.index)) {
      malformed.push({
        byte_offset: match.index,
        excerpt: text.slice(match.index, Math.min(text.length, match.index + 160)),
      });
    }
  }
  return { calls, malformed };
}

function parseScalarFunctionCalls(text, functionName) {
  const escaped = escapeRegex(functionName);
  const marker = new RegExp(`${escaped}\\s*\\(`, "g");
  const call = new RegExp(`${escaped}\\s*\\(\\s*(\\d+)\\s*(?=,|\\))`, "g");
  const calls = [];
  const matchedOffsets = new Set();
  for (const match of text.matchAll(call)) {
    matchedOffsets.add(match.index);
    const numeric = Number(match[1]);
    calls.push({
      byte_offset: match.index,
      expression: match[0],
      tokens: Number.isSafeInteger(numeric) && numeric >= 0 ? [String(numeric)] : [],
    });
  }
  const malformed = [];
  for (const match of text.matchAll(marker)) {
    if (!matchedOffsets.has(match.index)) {
      malformed.push({
        byte_offset: match.index,
        excerpt: text.slice(match.index, Math.min(text.length, match.index + 160)),
      });
    }
  }
  return { calls, malformed };
}

function parseExpressionFunctionCalls(text, functionName, expressionKind) {
  if (expressionKind === "named-function-brace-list-first-argument") {
    return parseBraceListFunctionCalls(text, functionName);
  }
  if (expressionKind === "named-function-scalar-first-argument") {
    return parseScalarFunctionCalls(text, functionName);
  }
  throw new Error(`Unsupported expression_kind: ${expressionKind}`);
}

function discoverNamedFunctionCalls(text) {
  const calls = [];
  for (const match of text.matchAll(/\b(skillpara\.[A-Za-z_][A-Za-z0-9_]*)\s*\(/g)) {
    calls.push({ function_name: match[1], byte_offset: match.index });
  }
  return calls;
}

function collectStrings(value, pointer) {
  if (typeof value === "string") return [{ pointer, value }];
  if (Array.isArray(value)) {
    return value.flatMap((entry, index) => collectStrings(entry, `${pointer}/${index}`));
  }
  if (value && typeof value === "object") {
    return Object.entries(value).flatMap(([key, entry]) =>
      collectStrings(entry, `${pointer}/${escapePointer(key)}`));
  }
  return [];
}

function validateRule(rule) {
  for (const field of ["id", "source_table", "source_field", "function_name", "relationship", "target_table"]) {
    if (typeof rule[field] !== "string" || !rule[field]) throw new Error(`Expression rule is missing ${field}`);
  }
  if (!["named-function-brace-list-first-argument", "named-function-scalar-first-argument"]
    .includes(rule.expression_kind)) {
    throw new Error(`Unsupported expression_kind in ${rule.id}: ${rule.expression_kind}`);
  }
}

function validateNonReferenceFunction(declaration) {
  for (const field of [
    "source_table", "source_field", "function_name", "classification", "rationale",
  ]) {
    if (typeof declaration[field] !== "string" || !declaration[field]) {
      throw new Error(`Non-reference function declaration is missing ${field}`);
    }
  }
}

function functionCoverageKey(sourceTable, sourceField, functionName) {
  return [sourceTable, sourceField, functionName].join("\u0000");
}

function normalizeRows(parsed) {
  if (Array.isArray(parsed)) {
    return parsed.map((value, index) => ({ storageKey: String(index), rowId: inferRowId(value, String(index)), value }));
  }
  if (!parsed || typeof parsed !== "object") return [{ storageKey: "0", rowId: "0", value: parsed }];
  return Object.entries(parsed).map(([storageKey, value]) => ({
    storageKey: String(storageKey),
    rowId: inferRowId(value, storageKey),
    value,
  }));
}

function inferRowId(value, fallback) {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    for (const field of ["Id", "ID", "id", "EntryId", "UId", "Uid", "UID"]) {
      const candidate = value[field];
      if (Number.isSafeInteger(candidate) || typeof candidate === "string") return String(candidate);
    }
  }
  return String(fallback);
}

function compareEdges(left, right) {
  return [left.source_table, left.source_id, left.source_pointer, left.target_table, left.target_id]
    .join("\u0000").localeCompare([right.source_table, right.source_id, right.source_pointer, right.target_table, right.target_id].join("\u0000"));
}

function compareOccurrences(left, right) {
  return [left.rule_id, left.source_table, left.source_id, left.source_pointer, left.candidate_target_id]
    .join("\u0000").localeCompare([right.rule_id, right.source_table, right.source_id, right.source_pointer, right.candidate_target_id].join("\u0000"));
}

function compareFailures(left, right) {
  return [left.rule_id, left.source_table, left.source_id, left.source_pointer, left.classification, left.candidate_target_id ?? ""]
    .join("\u0000").localeCompare([right.rule_id, right.source_table, right.source_id, right.source_pointer, right.classification, right.candidate_target_id ?? ""].join("\u0000"));
}

function compareUnclassifiedFunctions(left, right) {
  return [left.source_table, left.source_id, left.source_pointer, left.function_name, left.byte_offset]
    .join("\u0000").localeCompare(
      [right.source_table, right.source_id, right.source_pointer, right.function_name, right.byte_offset]
        .join("\u0000"),
    );
}

function selfTest() {
  const root = mkdtempSync(path.join(tmpdir(), "rlogs-expression-edge-test-"));
  try {
    const decodedRoot = path.join(root, "decoded");
    mkdirSync(decodedRoot);
    writeJson(path.join(decodedRoot, "SkillEffectTable.json"), {
      "100": { Id: 100, SkillAttrDes: [["Damage", "{*skillpara.damageMerge({111,222,222},{1,1,1},\"x\",\"up\")*}"], ["Other", "value 999"]] },
      "101": { Id: 101, SkillAttrDes: [["Missing", "skillpara.damageMerge({333},{1},\"x\",\"up\")"], ["Malformed", "skillpara.damageMerge(444)"]] },
      "102": { Id: 102, SkillAttrDes: [["Direct", "skillpara.damage(111,\"x\")"], ["Missing", "skillpara.damage(333,\"x\")"], ["Malformed", "skillpara.damage(\"oops\",\"x\")"], ["Field", "skillpara.effect(\"shieldHp\",\"up\")"], ["Unknown", "skillpara.futureRule(777)"], ["Other", "value 111"]] },
    });
    writeJson(path.join(decodedRoot, "DamageAttrTable.json"), {
      "111": { Id: 111, Type: "Attack" },
      "222": { Id: 222, Type: "Attack" },
      "999": { Id: 999, Type: "Unrelated" },
    });
    const graphPath = path.join(root, "graph.json");
    writeJson(graphPath, {
      game_build: "1",
      tables: [
        { table: "SkillEffectTable", file: "SkillEffectTable.json", rows: 3 },
        { table: "DamageAttrTable", file: "DamageAttrTable.json", rows: 3 },
      ],
    });
    const rulesPath = path.join(root, "rules.json");
    writeJson(rulesPath, {
      schema_version: 1,
      rules: [
        {
          id: "test-damage-merge",
          source_table: "SkillEffectTable",
          source_field: "SkillAttrDes",
          expression_kind: "named-function-brace-list-first-argument",
          function_name: "skillpara.damageMerge",
          relationship: "skill-effect-produced-damage",
          target_table: "DamageAttrTable",
        },
        {
          id: "test-damage",
          source_table: "SkillEffectTable",
          source_field: "SkillAttrDes",
          expression_kind: "named-function-scalar-first-argument",
          function_name: "skillpara.damage",
          relationship: "skill-effect-produced-damage",
          target_table: "DamageAttrTable",
        },
      ],
      non_reference_functions: [{
        source_table: "SkillEffectTable",
        source_field: "SkillAttrDes",
        function_name: "skillpara.effect",
        classification: "non-reference-field-selector",
        rationale: "The first argument names an effect field; it is not a numeric table identity.",
      }],
    });
    const output = path.join(root, "edges.json");
    build({ buildId: "1", decodedRoot, referenceGraph: graphPath, rules: rulesPath, output });
    const value = verify(output, true);
    if (value.summary.exact_edges !== 3 || value.summary.parsed_identifier_occurrences !== 6 || value.summary.failures !== 4) {
      throw new Error(`Self-test conservation failed: ${JSON.stringify(value.summary)}`);
    }
    if (value.summary.named_function_occurrences !== 8
      || value.summary.unclassified_named_function_occurrences !== 1
      || value.function_inventory.length !== 4) {
      throw new Error(`Self-test function coverage failed: ${JSON.stringify(value.summary)}`);
    }
    if (value.exact_edges.some((edge) => edge.target_id === "999")) {
      throw new Error("Unrelated numeric text was incorrectly promoted");
    }
    console.log("bpsr-decoded-expression-reference-edges self-test passed");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

function fingerprint(label, file) {
  requireFile(file, label);
  const stats = statSync(file);
  return { label, path: normalizePath(path.resolve(file)), bytes: stats.size, sha256: hashFile(file) };
}

function stableStringify(value) {
  if (value === undefined) return "undefined";
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function escapePointer(value) { return String(value).replaceAll("~", "~0").replaceAll("/", "~1"); }
function escapeRegex(value) { return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"); }
function normalizePath(value) { return value.replaceAll("\\", "/"); }
function hashFile(file) { return createHash("sha256").update(readFileSync(file)).digest("hex"); }
function hashText(value) { return createHash("sha256").update(value).digest("hex"); }
function readJson(file, label) {
  try { return JSON.parse(readFileSync(file, "utf8")); }
  catch (error) { throw new Error(`Unable to read ${label} ${file}: ${error.message}`); }
}
function writeJson(file, value) { writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`, "utf8"); }
function requireBuild(value, buildId, label) {
  if (String(value.game_build) !== String(buildId)) throw new Error(`${label} build ${value.game_build} does not match ${buildId}`);
}
function requireFile(file, label) {
  if (!existsSync(file) || !statSync(file).isFile()) throw new Error(`Missing ${label}: ${file}`);
}
function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (!arg.startsWith("--")) throw new Error(`Unexpected argument ${arg}`);
    const key = arg.slice(2);
    const value = args[index + 1];
    if (value === undefined || value.startsWith("--")) throw new Error(`Missing value for --${key}`);
    parsed[key] = value;
    index += 1;
  }
  return parsed;
}
function required(value, key) {
  if (!value[key]) throw new Error(`Missing --${key}`);
  return value[key];
}
function usage(exitCode) {
  console.log(`Usage:
  node tools/bpsr-decoded-expression-reference-edges.mjs build --build <id> --decoded-root <dir> --reference-graph <json> --rules <json> --output <json>
  node tools/bpsr-decoded-expression-reference-edges.mjs verify --input <json> [--verify-sources false]
  node tools/bpsr-decoded-expression-reference-edges.mjs self-test`);
  process.exit(exitCode);
}
