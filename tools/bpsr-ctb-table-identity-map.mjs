#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

const GENERATOR = "tools/bpsr-ctb-table-identity-map.mjs";
const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "generate") generate(resolveOptions(options));
else if (command === "verify") verify(required(options, "input"));
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function resolveOptions(args) {
  return {
    build: required(args, "build"),
    decodedRoot: absolute(required(args, "decoded-root")),
    seasonRogueProbe: absolute(required(args, "season-rogue-probe")),
    talentProbe: absolute(required(args, "talent-probe")),
    output: absolute(required(args, "output")),
  };
}

function generate(config) {
  if (!/^\d+$/.test(config.build)) throw new Error("Build must contain only ASCII digits");
  requireFile(config.seasonRogueProbe, "season rogue entry probe");
  requireFile(config.talentProbe, "talent effect model probe");
  const rogueTableFile = path.join(config.decodedRoot, "RogueEntryTable.json");
  const talentTableFile = path.join(config.decodedRoot, "TalentTable.json");
  requireFile(rogueTableFile, "decoded RogueEntryTable");
  requireFile(talentTableFile, "decoded TalentTable");

  const seasonProbe = readJson(config.seasonRogueProbe, "season rogue entry probe");
  const talentProbe = readJson(config.talentProbe, "talent effect model probe");
  const rogueTable = readJson(rogueTableFile, "decoded RogueEntryTable");
  const talentTable = readJson(talentTableFile, "decoded TalentTable");

  const mappings = [
    proveMapping({
      rawSourceTable: uniqueSource(seasonProbe.entries, "sourceTable", "season rogue entries"),
      decodedTable: "RogueEntryTable",
      probeRows: seasonProbe.entries,
      decodedRows: rogueTable,
      probeId: (row) => row.entryId,
      requiredComparisons: [
        comparison("EntryId", (probe) => probe.entryId, (decoded) => decoded.EntryId),
        comparison("BuffId", (probe) => probe.buffId, (decoded) => decoded.BuffId),
        comparison("EntryQuality", (probe) => probe.rarityOrTier, (decoded) => decoded.EntryQuality),
        comparison("EntryIcon", (probe) => probe.textureIconPath, (decoded) => decoded.EntryIcon),
        comparison(
          "EntryDescription opcode-1 key",
          (probe) => probe.descriptionEntryKey,
          (decoded) => opcodeValue(decoded.EntryDescription, 1, 1),
        ),
      ],
      optionalComparisons: [
        comparison("EntryName presentation", (probe) => probe.name, (decoded) => decoded.EntryName),
        comparison("EntrySmallIcon presentation", (probe) => probe.iconPath, (decoded) => decoded.EntrySmallIcon),
      ],
    }),
    proveMapping({
      rawSourceTable: uniqueSource(talentProbe.talentRows, "sourceTable", "talent rows"),
      decodedTable: "TalentTable",
      probeRows: talentProbe.talentRows,
      decodedRows: talentTable,
      probeId: (row) => row.id,
      requiredComparisons: [
        comparison("Id", (probe) => probe.id, (decoded) => decoded.Id),
        comparison("TalentName", (probe) => probe.name, (decoded) => decoded.TalentName),
        comparison("TalentIcon", (probe) => probe.iconPath, (decoded) => decoded.TalentIcon),
        comparison(
          "TalentEffect raw records",
          (probe) => (probe.effectRecords ?? []).map((record) => record.rawValues),
          (decoded) => decoded.TalentEffect,
        ),
      ],
      optionalComparisons: [
        comparison("Des presentation", (probe) => probe.designName, (decoded) => decoded.Des),
      ],
    }),
  ].sort((a, b) => a.raw_source_table.localeCompare(b.raw_source_table));

  const mappingByRawSource = Object.fromEntries(
    mappings.map((entry) => [entry.raw_source_table, entry.decoded_table]),
  );
  const report = {
    schema_version: 1,
    generated_by: GENERATOR,
    game_build: config.build,
    policy: {
      raw_source_hashes_are_never_guessed: true,
      exact_row_key_set_required: true,
      all_required_structural_comparisons_must_match: true,
      optional_presentation_discrepancies_are_retained: true,
      identity_is_build_locked: true,
    },
    inputs: {
      season_rogue_probe: describeFile(config.seasonRogueProbe),
      talent_probe: describeFile(config.talentProbe),
      decoded_rogue_entry_table: describeFile(rogueTableFile),
      decoded_talent_table: describeFile(talentTableFile),
    },
    summary: {
      exact_current_build_mappings: mappings.length,
      raw_rows: mappings.reduce((sum, entry) => sum + entry.raw_rows, 0),
      decoded_rows: mappings.reduce((sum, entry) => sum + entry.decoded_rows, 0),
      required_comparisons: mappings.reduce(
        (sum, entry) => sum + entry.required_comparisons.reduce((inner, item) => inner + item.compared_rows, 0),
        0,
      ),
      required_mismatches: mappings.reduce(
        (sum, entry) => sum + entry.required_comparisons.reduce((inner, item) => inner + item.mismatch_count, 0),
        0,
      ),
      optional_discrepancies: mappings.reduce(
        (sum, entry) => sum + entry.optional_comparisons.reduce((inner, item) => inner + item.mismatch_count, 0),
        0,
      ),
      zero_hidden_omissions: true,
    },
    mapping_by_raw_source: mappingByRawSource,
    mappings,
  };
  report.content_sha256 = canonicalHash({ ...report, content_sha256: undefined });
  mkdirSync(path.dirname(config.output), { recursive: true });
  writeFileSync(config.output, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  verify(config.output);
  console.log(
    `CTB table identity map generated for build ${config.build}: ${mappings.length} exact mappings, ` +
      `${report.summary.raw_rows} source rows, ${report.summary.required_mismatches} required mismatches.`,
  );
}

function proveMapping(config) {
  if (!Array.isArray(config.probeRows)) throw new Error(`${config.rawSourceTable} probe rows are missing`);
  if (!config.decodedRows || Array.isArray(config.decodedRows) || typeof config.decodedRows !== "object") {
    throw new Error(`${config.decodedTable} decoded rows are invalid`);
  }
  const probeById = new Map(config.probeRows.map((row) => [String(config.probeId(row)), row]));
  if (probeById.size !== config.probeRows.length) throw new Error(`${config.rawSourceTable} contains duplicate row ids`);
  const decodedIds = Object.keys(config.decodedRows).sort(compareNumericStrings);
  const probeIds = [...probeById.keys()].sort(compareNumericStrings);
  if (canonicalJson(probeIds) !== canonicalJson(decodedIds)) {
    throw new Error(`${config.rawSourceTable} and ${config.decodedTable} do not have the same row-key set`);
  }
  const required = config.requiredComparisons.map((entry) => compareRows(entry, probeIds, probeById, config.decodedRows));
  const optional = config.optionalComparisons.map((entry) => compareRows(entry, probeIds, probeById, config.decodedRows));
  const requiredMismatchCount = required.reduce((sum, entry) => sum + entry.mismatch_count, 0);
  if (requiredMismatchCount !== 0) {
    throw new Error(`${config.rawSourceTable} -> ${config.decodedTable} has ${requiredMismatchCount} required mismatches`);
  }
  return {
    raw_source_table: config.rawSourceTable,
    decoded_table: config.decodedTable,
    proof_state: "exact-current-build-row-equivalence",
    raw_rows: probeIds.length,
    decoded_rows: decodedIds.length,
    row_key_coverage: `${probeIds.length}/${decodedIds.length}`,
    row_key_sha256: canonicalHash(probeIds),
    required_comparisons: required,
    optional_comparisons: optional,
  };
}

function comparison(name, probe, decoded) { return { name, probe, decoded }; }

function compareRows(definition, ids, probeById, decodedRows) {
  const discrepancies = [];
  let comparedRows = 0;
  let omittedProbeValues = 0;
  for (const id of ids) {
    const probeValue = definition.probe(probeById.get(id));
    const decodedValue = definition.decoded(decodedRows[id]);
    if (probeValue === undefined) omittedProbeValues += 1;
    else comparedRows += 1;
    if (canonicalJson(probeValue) === canonicalJson(decodedValue)) continue;
    discrepancies.push({ row_id: id, probe_value: probeValue ?? null, decoded_value: decodedValue ?? null });
  }
  return {
    name: definition.name,
    total_rows: ids.length,
    compared_rows: comparedRows,
    omitted_probe_values: omittedProbeValues,
    match_count: ids.length - discrepancies.length,
    mismatch_count: discrepancies.length,
    discrepancies,
  };
}

function opcodeValue(records, opcode, valueIndex) {
  for (const record of records ?? []) {
    if (Array.isArray(record) && Number(record[0]) === opcode) return record[valueIndex];
  }
  return undefined;
}

function uniqueSource(rows, key, label) {
  if (!Array.isArray(rows) || rows.length === 0) throw new Error(`${label} are missing`);
  const sources = [...new Set(rows.map((row) => row[key]).filter(Boolean))];
  if (sources.length !== 1 || !/^CTB:\d+$/.test(sources[0])) {
    throw new Error(`${label} do not have exactly one raw CTB source`);
  }
  return sources[0];
}

function verify(input) {
  const file = absolute(input);
  requireFile(file, "CTB table identity map");
  const report = readJson(file, "CTB table identity map");
  if (report.schema_version !== 1 || report.generated_by !== GENERATOR) {
    throw new Error("CTB table identity map schema or generator is invalid");
  }
  if (!/^\d+$/.test(String(report.game_build))) throw new Error("CTB table identity map build is invalid");
  if (!Array.isArray(report.mappings) || report.mappings.length === 0) throw new Error("CTB mappings are missing");
  if (report.policy.raw_source_hashes_are_never_guessed !== true || report.summary.zero_hidden_omissions !== true) {
    throw new Error("CTB table identity map violates evidence-retention policy");
  }
  const expectedMap = {};
  let rawRows = 0;
  let decodedRows = 0;
  let requiredComparisons = 0;
  let requiredMismatches = 0;
  let optionalDiscrepancies = 0;
  for (const mapping of report.mappings) {
    if (!/^CTB:\d+$/.test(mapping.raw_source_table)) throw new Error("Invalid raw CTB source table");
    if (expectedMap[mapping.raw_source_table]) throw new Error(`Duplicate CTB mapping ${mapping.raw_source_table}`);
    if (mapping.proof_state !== "exact-current-build-row-equivalence") throw new Error("Non-exact CTB mapping retained");
    if (mapping.raw_rows !== mapping.decoded_rows) throw new Error(`Row counts differ for ${mapping.raw_source_table}`);
    const mismatchCount = (mapping.required_comparisons ?? []).reduce((sum, entry) => sum + entry.mismatch_count, 0);
    if (mismatchCount !== 0) throw new Error(`Required CTB comparison mismatch for ${mapping.raw_source_table}`);
    expectedMap[mapping.raw_source_table] = mapping.decoded_table;
    rawRows += mapping.raw_rows;
    decodedRows += mapping.decoded_rows;
    requiredComparisons += (mapping.required_comparisons ?? []).reduce((sum, entry) => sum + entry.compared_rows, 0);
    requiredMismatches += mismatchCount;
    optionalDiscrepancies += (mapping.optional_comparisons ?? []).reduce((sum, entry) => sum + entry.mismatch_count, 0);
  }
  if (canonicalJson(expectedMap) !== canonicalJson(report.mapping_by_raw_source)) throw new Error("CTB mapping index mismatch");
  const expectedSummary = { rawRows, decodedRows, requiredComparisons, requiredMismatches, optionalDiscrepancies };
  const actualSummary = {
    rawRows: report.summary.raw_rows,
    decodedRows: report.summary.decoded_rows,
    requiredComparisons: report.summary.required_comparisons,
    requiredMismatches: report.summary.required_mismatches,
    optionalDiscrepancies: report.summary.optional_discrepancies,
  };
  if (canonicalJson(expectedSummary) !== canonicalJson(actualSummary)) throw new Error("CTB identity summary mismatch");
  const expectedHash = canonicalHash({ ...report, content_sha256: undefined });
  if (report.content_sha256 !== expectedHash) throw new Error("CTB table identity map content hash mismatch");
  console.log(`CTB table identity map verified: ${report.mappings.length} exact current-build mappings.`);
}

function selfTest() {
  const mapping = proveMapping({
    rawSourceTable: "CTB:1",
    decodedTable: "ExampleTable",
    probeRows: [{ id: 1, value: 7 }, { id: 2, value: 8 }],
    decodedRows: { 1: { Id: 1, Value: 7 }, 2: { Id: 2, Value: 8 } },
    probeId: (row) => row.id,
    requiredComparisons: [comparison("Id", (probe) => probe.id, (decoded) => decoded.Id)],
    optionalComparisons: [comparison("Value", (probe) => probe.value, (decoded) => decoded.Value)],
  });
  if (mapping.raw_rows !== 2 || mapping.required_comparisons[0].mismatch_count !== 0) {
    throw new Error("CTB identity self-test failed");
  }
  console.log("CTB table identity map self-test passed.");
}

function describeFile(file) { return { path: normalize(file), bytes: statSync(file).size, sha256: sha256(file) }; }
function sha256(file) { return createHash("sha256").update(readFileSync(file)).digest("hex"); }
function canonicalHash(value) { return createHash("sha256").update(canonicalJson(value)).digest("hex"); }
function canonicalJson(value) {
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).filter((key) => value[key] !== undefined).sort().map((key) => `${JSON.stringify(key)}:${canonicalJson(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}
function compareNumericStrings(a, b) { return a.length - b.length || a.localeCompare(b); }
function readJson(file, label) {
  try { return JSON.parse(readFileSync(file, "utf8")); }
  catch (error) { throw new Error(`Unable to read ${label} ${file}: ${error.message}`); }
}
function requireFile(file, label) {
  if (!existsSync(file) || !statSync(file).isFile()) throw new Error(`Missing ${label}: ${file}`);
}
function parseArgs(args) {
  const output = {};
  for (let index = 0; index < args.length; index += 2) {
    const token = args[index];
    if (!token?.startsWith("--")) throw new Error(`Unexpected argument ${token}`);
    const value = args[index + 1];
    if (!value || value.startsWith("--")) throw new Error(`Missing value for ${token}`);
    output[token.slice(2)] = value;
  }
  return output;
}
function required(args, key) { if (!args[key]) throw new Error(`Missing --${key}`); return String(args[key]); }
function absolute(value) { return path.resolve(value); }
function normalize(value) { return value.replaceAll("\\", "/"); }
function usage(exitCode) {
  console.log(`Usage:
  node tools/bpsr-ctb-table-identity-map.mjs generate --build BUILD --decoded-root DIR --season-rogue-probe FILE --talent-probe FILE --output FILE
  node tools/bpsr-ctb-table-identity-map.mjs verify --input FILE
  node tools/bpsr-ctb-table-identity-map.mjs self-test`);
  process.exit(exitCode);
}
