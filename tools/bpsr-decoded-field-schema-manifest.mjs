#!/usr/bin/env node

import { createHash } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  statSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, "..");
const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "generate") generate(options);
else if (command === "verify") verify(options);
else if (command === "diff") diff(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function generate(options) {
  const decodedRoot = resolvePath(required(options, "decodedRoot"));
  const semanticPath = resolvePath(required(options, "semanticFieldSchema"));
  const dumpPath = resolvePath(required(options, "il2cppDump"));
  const outputPath = resolvePath(required(options, "output"));
  const build = required(options, "build");
  if (!/^\d+$/.test(build)) throw new Error("Build must contain only ASCII digits");
  requireDirectory(decodedRoot, "decoded table root");

  const semantic = readJson(semanticPath, "semantic field-schema ledger");
  requireBuild(semantic, build, "semantic field-schema ledger");
  const relationshipByPath = new Map(
    semantic.fields.map((row) => [`${row.source_table}${row.path_pattern}`, compactRelationship(row)]),
  );
  const dumpSchema = parseIl2CppDump(readFileSync(dumpPath, "utf8"));
  const decodedFiles = listJsonFiles(decodedRoot);
  const fields = [];
  let decodedRows = 0;

  for (const file of decodedFiles) {
    const table = path.basename(file, ".json");
    const decoded = readJson(file, `decoded table ${table}`);
    if (!decoded || Array.isArray(decoded) || typeof decoded !== "object") {
      throw new Error(`${table} must be a JSON object keyed by decoded row identity`);
    }
    const rowKeys = Object.keys(decoded).sort(compareRowKeys);
    decodedRows += rowKeys.length;
    const accumulators = new Map();
    for (const rowKey of rowKeys) {
      const row = decoded[rowKey];
      if (!row || Array.isArray(row) || typeof row !== "object") {
        recordValue(accumulators, "/$row", row, rowKey);
        continue;
      }
      for (const key of Object.keys(row).sort()) walkValue(accumulators, `/${escapePath(key)}`, row[key], rowKey);
    }
    for (const [pathPattern, accumulator] of [...accumulators.entries()].sort(([a], [b]) => a.localeCompare(b))) {
      const topField = firstPathField(pathPattern);
      // Relationship evidence belongs only to the exact normalized path that
      // the ID ledger proved. Never smear a top-level ID relationship over
      // sibling or nested scalar parameters merely because they share a field.
      const relationship = relationshipByPath.get(`${table}${pathPattern}`) ?? null;
      const il2cpp = topField === "$row"
        ? { class_found: false, property_found: false, getter_found: false }
        : resolveIl2CppField(dumpSchema, table, topField);
      const row = finalizeField({
        table,
        pathPattern,
        topField,
        accumulator,
        tableRowCount: rowKeys.length,
        relationship,
        il2cpp,
      });
      fields.push(row);
    }
  }

  fields.sort(compareFieldRows);
  const summary = summarize(fields, decodedFiles.length, decodedRows);
  const output = {
    schema_version: 1,
    generated_by: "tools/bpsr-decoded-field-schema-manifest.mjs",
    game_build: String(build),
    inputs: {
      decoded_root: describeTree(decodedRoot, decodedFiles),
      semantic_field_schema: describeInput(semanticPath),
      il2cpp_dump: describeInput(dumpPath),
    },
    policy: {
      every_decoded_field_path_preserved: true,
      scalar_formula_parameters_included: true,
      arrays_and_nested_values_included: true,
      unresolved_semantics_hidden: false,
      field_name_mechanics_routing_is_not_formula_proof: true,
      relationship_proof_is_inherited_only_from_the_verified_id_field_ledger: true,
      steam_manifest_diff_is_a_physical_change_detector_not_semantic_authority: true,
      future_patch_diff_key: "source_table/path_pattern",
    },
    summary,
    fields,
  };
  output.semantic_sha256 = semanticHash(output, ["semantic_sha256"]);
  mkdirSync(path.dirname(outputPath), { recursive: true });
  writeFileSync(outputPath, `${JSON.stringify(output, null, 2)}\n`, "utf8");
  console.log(JSON.stringify(summary, null, 2));
  console.log(`Decoded field-schema manifest written: ${outputPath}`);
}

function verify(options) {
  const inputPath = resolvePath(required(options, "input"));
  const value = readJson(inputPath, "decoded field-schema manifest");
  verifyManifest(value);
  console.log(
    `Decoded field-schema manifest verified: ${value.summary.decoded_field_paths} paths across `
    + `${value.summary.decoded_tables} tables and ${value.summary.decoded_rows} rows.`,
  );
}

function verifyManifest(value) {
  if (value.schema_version !== 1) throw new Error("Decoded field-schema manifest schema_version must be 1");
  if (value.generated_by !== "tools/bpsr-decoded-field-schema-manifest.mjs") {
    throw new Error("Unexpected decoded field-schema manifest generator");
  }
  const expected = semanticHash(value, ["semantic_sha256"]);
  if (value.semantic_sha256 !== expected) {
    throw new Error(`Decoded field-schema manifest hash mismatch: stored ${value.semantic_sha256}, computed ${expected}`);
  }
  if (!Array.isArray(value.fields) || value.fields.length !== value.summary?.decoded_field_paths) {
    throw new Error("Decoded field-schema summary does not cover every field path");
  }
  const keys = new Set();
  for (const row of value.fields) {
    if (keys.has(row.key)) throw new Error(`Duplicate decoded field path: ${row.key}`);
    keys.add(row.key);
    if (!row.source_table || !row.path_pattern || !row.value_profile?.semantic_sha256) {
      throw new Error(`Incomplete decoded field row: ${row.key}`);
    }
    if (row.rows_present + row.rows_missing !== row.table_row_count) {
      throw new Error(`Row coverage mismatch: ${row.key}`);
    }
  }
  if (value.summary.structural_inventory_complete !== true) {
    throw new Error("Decoded field-schema structural inventory is not complete");
  }
}

function diff(options) {
  const baselinePath = resolvePath(required(options, "baseline"));
  const candidatePath = resolvePath(required(options, "candidate"));
  const outputPath = resolvePath(required(options, "output"));
  const baseline = readJson(baselinePath, "baseline decoded field-schema manifest");
  const candidate = readJson(candidatePath, "candidate decoded field-schema manifest");
  verifyManifest(baseline);
  verifyManifest(candidate);
  const before = new Map(baseline.fields.map((row) => [row.key, row]));
  const after = new Map(candidate.fields.map((row) => [row.key, row]));
  const added = [];
  const removed = [];
  const changed = [];
  for (const [key, row] of after) {
    const prior = before.get(key);
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
  for (const [key, row] of before) if (!after.has(key)) removed.push(compactDiffRow(row));
  const affectedTables = [...new Set([
    ...added.map((row) => row.source_table),
    ...removed.map((row) => row.source_table),
    ...changed.map((row) => row.candidate.source_table),
  ])].sort();
  const result = {
    schema_version: 1,
    generated_by: "tools/bpsr-decoded-field-schema-manifest.mjs",
    baseline_build: String(baseline.game_build),
    candidate_build: String(candidate.game_build),
    inputs: { baseline: describeInput(baselinePath), candidate: describeInput(candidatePath) },
    policy: {
      unchanged_field_paths_require_reproof: false,
      changed_field_paths_route_only_affected_tables_and_semantic_domains: true,
      removed_or_unresolved_evidence_hidden: false,
    },
    summary: {
      added: added.length,
      removed: removed.length,
      changed: changed.length,
      affected_tables: affectedTables.length,
    },
    affected_tables: affectedTables,
    added: added.sort(compareFieldRows),
    removed: removed.sort(compareFieldRows),
    changed: changed.sort(compareFieldRows),
  };
  result.semantic_sha256 = semanticHash(result, ["semantic_sha256"]);
  mkdirSync(path.dirname(outputPath), { recursive: true });
  writeFileSync(outputPath, `${JSON.stringify(result, null, 2)}\n`, "utf8");
  console.log(JSON.stringify(result.summary, null, 2));
}

function walkValue(accumulators, pathPattern, value, rowKey) {
  recordValue(accumulators, pathPattern, value, rowKey);
  if (Array.isArray(value)) {
    for (const item of value) walkValue(accumulators, `${pathPattern}[]`, item, rowKey);
  } else if (value && typeof value === "object") {
    for (const key of Object.keys(value).sort()) {
      walkValue(accumulators, `${pathPattern}/${escapePath(key)}`, value[key], rowKey);
    }
  }
}

function recordValue(accumulators, pathPattern, value, rowKey) {
  let row = accumulators.get(pathPattern);
  if (!row) {
    row = {
      occurrences: 0,
      rows_present: 0,
      last_row_key: null,
      null_count: 0,
      type_counts: {},
      numeric: { finite: 0, zero: 0, positive: 0, negative: 0, integer: 0, fractional: 0, minimum: null, maximum: null },
      string: { empty: 0, nonempty: 0, minimum_length: null, maximum_length: null, shapes: {} },
      array: { empty: 0, nonempty: 0, minimum_length: null, maximum_length: null },
      boolean: { true: 0, false: 0 },
      examples: new Map(),
      stream_hash: createHash("sha256"),
    };
    accumulators.set(pathPattern, row);
  }
  row.occurrences += 1;
  if (row.last_row_key !== rowKey) {
    row.rows_present += 1;
    row.last_row_key = rowKey;
  }
  const type = valueType(value);
  row.type_counts[type] = (row.type_counts[type] ?? 0) + 1;
  if (value === null) row.null_count += 1;
  if (type === "number") updateNumber(row.numeric, value);
  else if (type === "string") updateString(row.string, value);
  else if (type === "array") updateLength(row.array, value.length);
  else if (type === "boolean") row.boolean[value ? "true" : "false"] += 1;
  const canonical = canonicalValue(value);
  row.stream_hash.update(`${canonical.length}:`).update(canonical).update("\n");
  const exampleKey = createHash("sha256").update(canonical).digest("hex");
  if (row.examples.size < 12 && !row.examples.has(exampleKey)) row.examples.set(exampleKey, compactExample(value));
}

function finalizeField({ table, pathPattern, topField, accumulator, tableRowCount, relationship, il2cpp }) {
  const valueProfile = {
    occurrences: accumulator.occurrences,
    null_count: accumulator.null_count,
    type_counts: sortObject(accumulator.type_counts),
    occurrence_value_stream_sha256: accumulator.stream_hash.digest("hex"),
    numeric: accumulator.type_counts.number ? accumulator.numeric : null,
    string: accumulator.type_counts.string ? { ...accumulator.string, shapes: sortObject(accumulator.string.shapes) } : null,
    array: accumulator.type_counts.array ? accumulator.array : null,
    boolean: accumulator.type_counts.boolean ? accumulator.boolean : null,
    examples: [...accumulator.examples.values()],
  };
  valueProfile.semantic_sha256 = semanticHash(valueProfile, ["semantic_sha256"]);
  const mechanicsRouting = classifyMechanicsPath(table, pathPattern);
  const row = {
    key: `${table}${pathPattern}`,
    source_table: table,
    top_level_field: topField,
    path_pattern: pathPattern,
    table_row_count: tableRowCount,
    rows_present: accumulator.rows_present,
    rows_missing: tableRowCount - accumulator.rows_present,
    value_profile: valueProfile,
    mechanics_review_routing: mechanicsRouting,
    il2cpp_top_level_schema: il2cpp,
    semantic_relationship: relationship,
  };
  row.semantic_sha256 = semanticHash(row, ["semantic_sha256"]);
  return row;
}

function summarize(fields, tableCount, rowCount) {
  const typeCounts = {};
  for (const field of fields) {
    for (const type of Object.keys(field.value_profile.type_counts)) typeCounts[type] = (typeCounts[type] ?? 0) + 1;
  }
  return {
    decoded_tables: tableCount,
    decoded_rows: rowCount,
    decoded_field_paths: fields.length,
    scalar_field_paths: fields.filter((row) => hasScalarType(row)).length,
    array_field_paths: fields.filter((row) => row.value_profile.type_counts.array).length,
    object_field_paths: fields.filter((row) => row.value_profile.type_counts.object).length,
    field_paths_by_observed_type: sortObject(typeCounts),
    mechanics_sensitive_field_paths: fields.filter((row) => row.mechanics_review_routing.requires_semantic_review).length,
    id_relationship_field_paths_attached: fields.filter((row) => row.semantic_relationship).length,
    id_relationship_field_paths_open: fields.filter((row) => row.semantic_relationship?.resolution_state === "open").length,
    il2cpp_top_level_property_paths_found: fields.filter((row) => row.il2cpp_top_level_schema.property_found).length,
    structural_inventory_complete: true,
  };
}

function classifyMechanicsPath(table, pathPattern) {
  const text = `${table} ${pathPattern}`;
  const categories = [];
  const rules = [
    ["damage-or-attack", /damage|attack|hurt|injury/i],
    ["healing-or-shield", /heal|cure|shield|absorb/i],
    ["ratio-rate-or-coefficient", /ratio|rate|percent|coefficient|coefficient|scale|multiple/i],
    ["duration-time-or-interval", /duration|time|interval|cooldown|cd\b/i],
    ["stack-count-or-limit", /stack|count|limit|maximum|minimum|max\b|min\b/i],
    ["attribute-stat-or-hp", /attribute|\battr|stat|\bhp\b|health|crit|luck|haste|mastery/i],
    ["skill-buff-effect-or-factor", /skill|buff|effect|talent|factor|imagine|equipment|weapon|set/i],
    ["formula-parameter-or-value", /formula|parameter|\bparam|value|offset|threshold|weight|energy|consume|cost/i],
  ];
  for (const [category, regex] of rules) if (regex.test(text)) categories.push(category);
  return {
    requires_semantic_review: categories.length > 0,
    categories,
    evidence_state: categories.length > 0
      ? "name-routed-for-mechanics-review-not-semantic-proof"
      : "general-decoded-field",
  };
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
      if (match) current.getters.set(match[2], {
        return_type: normalizeType(match[1]),
        getter_rva: pendingRva,
        getter_rva_hex: `0x${pendingRva.toString(16).toUpperCase()}`,
        getter_name: `${current.name}.get_${match[2]}()`,
      });
      pendingRva = null;
    }
  }
  return classes;
}

function resolveIl2CppField(classes, table, field) {
  const candidates = [`${table}Base`, table.endsWith("Table") ? `${table}Base` : `${table}TableBase`];
  let schema = null;
  for (const candidate of candidates) if (classes.has(candidate)) { schema = classes.get(candidate); break; }
  if (!schema) return { class_found: false, property_found: false, getter_found: false };
  const property = schema.properties.get(field) ?? null;
  const getter = schema.getters.get(field) ?? null;
  return {
    class_found: true,
    class_name: schema.name,
    property_found: property !== null,
    property_type: property?.property_type ?? null,
    getter_found: getter !== null,
    ...getter,
  };
}

function compactRelationship(row) {
  return {
    key: row.key,
    resolution_state: row.resolution_state,
    evidence_state: row.evidence_state,
    accepted_target_tables: row.accepted_target_tables,
    open_reason: row.open_reason,
    semantic_sha256: row.semantic_sha256,
  };
}

function compactDiffRow(row) {
  return {
    key: row.key,
    source_table: row.source_table,
    top_level_field: row.top_level_field,
    path_pattern: row.path_pattern,
    mechanics_review_routing: row.mechanics_review_routing,
    value_profile_sha256: row.value_profile.semantic_sha256,
    il2cpp_property_type: row.il2cpp_top_level_schema?.property_type ?? null,
    relationship_state: row.semantic_relationship?.resolution_state ?? null,
    semantic_sha256: row.semantic_sha256,
  };
}

function changedProperties(a, b) {
  const pairs = [
    ["value_profile", a.value_profile.semantic_sha256, b.value_profile.semantic_sha256],
    ["mechanics_review_routing", a.mechanics_review_routing, b.mechanics_review_routing],
    ["il2cpp_property_type", a.il2cpp_top_level_schema?.property_type, b.il2cpp_top_level_schema?.property_type],
    ["il2cpp_getter_rva", a.il2cpp_top_level_schema?.getter_rva, b.il2cpp_top_level_schema?.getter_rva],
    ["semantic_relationship", a.semantic_relationship?.semantic_sha256, b.semantic_relationship?.semantic_sha256],
  ];
  return pairs.filter(([, left, right]) => JSON.stringify(left) !== JSON.stringify(right)).map(([name]) => name);
}

function updateNumber(profile, value) {
  if (!Number.isFinite(value)) return;
  profile.finite += 1;
  if (value === 0) profile.zero += 1;
  else if (value > 0) profile.positive += 1;
  else profile.negative += 1;
  if (Number.isInteger(value)) profile.integer += 1;
  else profile.fractional += 1;
  profile.minimum = profile.minimum === null ? value : Math.min(profile.minimum, value);
  profile.maximum = profile.maximum === null ? value : Math.max(profile.maximum, value);
}

function updateString(profile, value) {
  if (value.length === 0) profile.empty += 1;
  else profile.nonempty += 1;
  profile.minimum_length = profile.minimum_length === null ? value.length : Math.min(profile.minimum_length, value.length);
  profile.maximum_length = profile.maximum_length === null ? value.length : Math.max(profile.maximum_length, value.length);
  const shape = /^-?\d+(?:\.\d+)?$/.test(value)
    ? "numeric-text"
    : /[\\/]/.test(value) || /\.(?:png|jpg|jpeg|webp|asset|prefab|bytes)$/i.test(value)
      ? "path-or-asset"
      : value.length > 96 ? "long-text" : "text";
  profile.shapes[shape] = (profile.shapes[shape] ?? 0) + 1;
}

function updateLength(profile, length) {
  if (length === 0) profile.empty += 1;
  else profile.nonempty += 1;
  profile.minimum_length = profile.minimum_length === null ? length : Math.min(profile.minimum_length, length);
  profile.maximum_length = profile.maximum_length === null ? length : Math.max(profile.maximum_length, length);
}

function canonicalValue(value) {
  if (value === undefined) return "undefined";
  return stableStringify(value, new Set());
}

function compactExample(value) {
  if (typeof value === "string") return value.length > 160 ? `${value.slice(0, 157)}...` : value;
  if (Array.isArray(value)) return { type: "array", length: value.length };
  if (value && typeof value === "object") return { type: "object", keys: Object.keys(value).sort().slice(0, 12) };
  return value;
}

function valueType(value) {
  if (value === null) return "null";
  if (Array.isArray(value)) return "array";
  return typeof value;
}

function hasScalarType(row) {
  return ["number", "string", "boolean", "null"].some((type) => row.value_profile.type_counts[type]);
}

function firstPathField(pathPattern) {
  return pathPattern.slice(1).split(/[\[/]/, 1)[0].replaceAll("~1", "/").replaceAll("~0", "~");
}

function escapePath(value) { return String(value).replaceAll("~", "~0").replaceAll("/", "~1"); }
function normalizeType(type) { return type.replace(/\s+/g, " ").trim(); }
function sortObject(value) { return Object.fromEntries(Object.entries(value).sort(([a], [b]) => a.localeCompare(b))); }
function compareFieldRows(a, b) { return (a.source_table ?? a.candidate?.source_table ?? "").localeCompare(b.source_table ?? b.candidate?.source_table ?? "") || String(a.path_pattern ?? a.candidate?.path_pattern ?? a.key).localeCompare(String(b.path_pattern ?? b.candidate?.path_pattern ?? b.key)); }
function compareRowKeys(a, b) { return /^\d+$/.test(a) && /^\d+$/.test(b) ? BigInt(a) < BigInt(b) ? -1 : BigInt(a) > BigInt(b) ? 1 : 0 : a.localeCompare(b); }

function listJsonFiles(root) {
  const files = [];
  const visit = (directory) => {
    for (const entry of readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      const full = path.join(directory, entry.name);
      if (entry.isDirectory()) visit(full);
      else if (entry.isFile() && entry.name.endsWith(".json")) files.push(full);
    }
  };
  visit(root);
  return files;
}

function semanticHash(value, excludedKeys = []) {
  const normalized = JSON.parse(JSON.stringify(value));
  return createHash("sha256").update(stableStringify(normalized, new Set(excludedKeys))).digest("hex");
}

function stableStringify(value, excluded) {
  if (Array.isArray(value)) return `[${value.map((item) => stableStringify(item, excluded)).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).filter((key) => !excluded.has(key) && value[key] !== undefined).sort()
      .map((key) => `${JSON.stringify(key)}:${stableStringify(value[key], excluded)}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function describeTree(root, files) {
  const relative = files.map((file) => path.relative(root, file).replaceAll("\\", "/"));
  return {
    path: root,
    json_files: files.length,
    bytes: files.reduce((sum, file) => sum + statSync(file).size, 0),
    file_list_sha256: createHash("sha256").update(relative.join("\n")).digest("hex"),
    aggregate_content_sha256: createHash("sha256").update(files.map((file) => `${path.relative(root, file)}\0${sha256(file)}`).join("\n")).digest("hex"),
  };
}

function describeInput(file) { return { path: file, bytes: statSync(file).size, sha256: sha256(file) }; }
function sha256(file) { return createHash("sha256").update(readFileSync(file)).digest("hex"); }
function readJson(file, label) { if (!existsSync(file)) throw new Error(`${label} does not exist: ${file}`); return JSON.parse(readFileSync(file, "utf8")); }
function requireBuild(value, build, label) { if (String(value.game_build) !== String(build)) throw new Error(`${label} is build ${value.game_build ?? "<missing>"}, expected ${build}`); }
function requireDirectory(directory, label) { if (!existsSync(directory) || !statSync(directory).isDirectory()) throw new Error(`Missing ${label}: ${directory}`); }
function required(value, key) { if (!value[key]) throw new Error(`Missing --${toKebab(key)}`); return String(value[key]); }
function resolvePath(value) { return path.isAbsolute(value) ? value : path.resolve(repoRoot, value); }
function toKebab(value) { return value.replace(/[A-Z]/g, (letter) => `-${letter.toLowerCase()}`); }

function parseArgs(args) {
  const result = {};
  for (let index = 0; index < args.length; index += 1) {
    const token = args[index];
    if (!token.startsWith("--")) throw new Error(`Unexpected argument ${token}`);
    const key = token.slice(2).replace(/-([a-z])/g, (_, letter) => letter.toUpperCase());
    const next = args[index + 1];
    if (!next || next.startsWith("--")) throw new Error(`Missing value for ${token}`);
    result[key] = next;
    index += 1;
  }
  return result;
}

function selfTest() {
  const sample = {
    source_table: "SkillTable",
    path_pattern: "/DamageRatio",
    value_profile: { semantic_sha256: "a" },
    mechanics_review_routing: { requires_semantic_review: true },
    il2cpp_top_level_schema: { property_type: "float", getter_rva: 1 },
    semantic_relationship: null,
  };
  if (!classifyMechanicsPath("SkillTable", "/DamageRatio").requires_semantic_review) throw new Error("Mechanics routing self-test failed");
  if (!changedProperties(sample, { ...sample, value_profile: { semantic_sha256: "b" } }).includes("value_profile")) throw new Error("Diff self-test failed");
  const accumulators = new Map();
  walkValue(accumulators, "/Values", [[0, 5.2], [1, 3.4]], "1");
  if (!accumulators.has("/Values[][]")) throw new Error("Nested array path self-test failed");
  console.log("Decoded field-schema manifest self-test passed.");
}

function usage(exitCode) {
  console.log(`Usage:
  node tools/bpsr-decoded-field-schema-manifest.mjs generate --decoded-root <dir> --semantic-field-schema <json> --il2cpp-dump <dump.cs> --build <id> --output <json>
  node tools/bpsr-decoded-field-schema-manifest.mjs verify --input <json>
  node tools/bpsr-decoded-field-schema-manifest.mjs diff --baseline <json> --candidate <json> --output <json>
  node tools/bpsr-decoded-field-schema-manifest.mjs self-test`);
  process.exit(exitCode);
}
