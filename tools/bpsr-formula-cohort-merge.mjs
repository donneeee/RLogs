#!/usr/bin/env node

import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const TOOL_SCHEMA_VERSION = 1;
const COHORT_SCHEMA_VERSION = 47;
const GENERATED_BY = "tools/bpsr-formula-cohort-merge.mjs";

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "generate") generate(options);
else if (command === "verify") verify(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function generate(options) {
  assert.ok(options.inputs.length >= 2, "merge requires at least two input cohorts");
  const inputPaths = options.inputs.map((input) => path.resolve(input));
  const outputPath = path.resolve(required(options.output, "output"));
  refuseExisting(outputPath);
  const cohorts = inputPaths.map(readJson);
  const merged = mergeCohorts(cohorts, inputPaths);
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  fs.writeFileSync(outputPath, `${JSON.stringify(merged)}\n`, { flag: "wx" });
  process.stdout.write(`${JSON.stringify(summary(merged), null, 2)}\n`);
}

function mergeCohorts(cohorts, inputPaths) {
  validateCompatible(cohorts);
  const first = cohorts[0];
  const merged = {
    schema_version: COHORT_SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game_build: String(first.game_build),
    policy: structuredClone(first.policy),
    selection: structuredClone(first.selection),
    gap_window_filter: structuredClone(first.gap_window_filter),
    transition_seed_filter: structuredClone(first.transition_seed_filter),
    inputs: [],
    attribute_states: [],
    status_states: [],
    samples: [],
  };
  const sourceReceipts = [];
  for (let index = 0; index < cohorts.length; index += 1) {
    const cohort = cohorts[index];
    const attributeStateOffset = merged.attribute_states.length;
    const statusStateOffset = merged.status_states.length;
    const sampleOffset = merged.samples.length;
    merged.inputs.push(...cohort.inputs);
    merged.attribute_states.push(...structuredClone(cohort.attribute_states));
    merged.status_states.push(...structuredClone(cohort.status_states));
    for (const original of cohort.samples) {
      const sample = structuredClone(original);
      sample.source_attribute_state_id += attributeStateOffset;
      if (sample.direct_source_attribute_state_id != null) {
        sample.direct_source_attribute_state_id += attributeStateOffset;
      }
      sample.target_attribute_state_id += attributeStateOffset;
      sample.source_status_state_id += statusStateOffset;
      sample.target_status_state_id += statusStateOffset;
      for (const provider of sample.status_provider_attribute_states ?? []) {
        if (provider.attribute_state_id != null) {
          provider.attribute_state_id += attributeStateOffset;
        }
      }
      merged.samples.push(sample);
    }
    sourceReceipts.push({
      ...fileReceipt(inputPaths[index]),
      declared_rlogs: cohort.inputs.length,
      attribute_state_offset: attributeStateOffset,
      attribute_states: cohort.attribute_states.length,
      status_state_offset: statusStateOffset,
      status_states: cohort.status_states.length,
      sample_offset: sampleOffset,
      samples: cohort.samples.length,
    });
  }
  assert.equal(new Set(merged.inputs).size, merged.inputs.length,
    "merged cohort contains a duplicate RLOG input");
  validateStateReferences(merged);
  merged.merge_receipt = {
    schema_version: TOOL_SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    source_cohorts: sourceReceipts,
    summary: {
      source_cohorts: cohorts.length,
      declared_rlogs: merged.inputs.length,
      attribute_states: merged.attribute_states.length,
      status_states: merged.status_states.length,
      samples: merged.samples.length,
    },
    policy: {
      source_order_preserved: true,
      sample_order_within_each_source_preserved: true,
      interned_state_ids_rebased_without_semantic_change: true,
      source_samples_deduplicated: false,
      current_character_snapshot_substitution_allowed: false,
      formula_authority: false,
    },
  };
  merged.content_sha256 = contentHash(merged);
  return merged;
}

function validateCompatible(cohorts) {
  const first = cohorts[0];
  assert.equal(first.schema_version, COHORT_SCHEMA_VERSION);
  assert.ok(Array.isArray(first.inputs));
  assert.ok(Array.isArray(first.attribute_states));
  assert.ok(Array.isArray(first.status_states));
  assert.ok(Array.isArray(first.samples));
  const stableFields = [
    "game_build",
    "policy",
    "selection",
    "gap_window_filter",
    "transition_seed_filter",
  ];
  for (const cohort of cohorts) {
    assert.equal(cohort.schema_version, COHORT_SCHEMA_VERSION);
    for (const field of stableFields) {
      assert.equal(
        stableStringify(cohort[field]),
        stableStringify(first[field]),
        `cohort ${field} mismatch`,
      );
    }
    validateStateReferences(cohort);
  }
}

function validateStateReferences(cohort) {
  for (const sample of cohort.samples) {
    for (const stateId of [
      sample.source_attribute_state_id,
      sample.direct_source_attribute_state_id,
      sample.target_attribute_state_id,
    ]) {
      if (stateId == null) continue;
      assert.ok(Number.isInteger(stateId) && cohort.attribute_states[stateId],
        `invalid attribute state ${stateId}`);
    }
    for (const stateId of [
      sample.source_status_state_id,
      sample.target_status_state_id,
    ]) {
      assert.ok(Number.isInteger(stateId) && cohort.status_states[stateId],
        `invalid status state ${stateId}`);
    }
    for (const provider of sample.status_provider_attribute_states ?? []) {
      if (provider.attribute_state_id == null) continue;
      assert.ok(
        Number.isInteger(provider.attribute_state_id) &&
          cohort.attribute_states[provider.attribute_state_id],
        `invalid provider attribute state ${provider.attribute_state_id}`,
      );
    }
  }
}

function verify(options) {
  const inputPath = path.resolve(required(options.input, "input"));
  const report = readJson(inputPath);
  assert.equal(report.schema_version, COHORT_SCHEMA_VERSION);
  assert.equal(report.generated_by, GENERATED_BY);
  assert.equal(report.merge_receipt.schema_version, TOOL_SCHEMA_VERSION);
  assert.equal(report.merge_receipt.generated_by, GENERATED_BY);
  assert.equal(report.merge_receipt.policy.formula_authority, false);
  assert.equal(report.content_sha256, contentHash(report));
  for (const receipt of report.merge_receipt.source_cohorts) verifyFileReceipt(receipt);
  validateStateReferences(report);
  const cohorts = report.merge_receipt.source_cohorts.map((receipt) => readJson(receipt.path));
  const regenerated = mergeCohorts(
    cohorts,
    report.merge_receipt.source_cohorts.map((receipt) => receipt.path),
  );
  assert.equal(regenerated.content_sha256, report.content_sha256);
  process.stdout.write(`${JSON.stringify(summary(report), null, 2)}\n`);
}

function summary(cohort) {
  return {
    game_build: cohort.game_build,
    source_cohorts: cohort.merge_receipt.summary.source_cohorts,
    declared_rlogs: cohort.inputs.length,
    attribute_states: cohort.attribute_states.length,
    status_states: cohort.status_states.length,
    samples: cohort.samples.length,
    content_sha256: cohort.content_sha256,
  };
}

function selfTest() {
  const base = {
    schema_version: COHORT_SCHEMA_VERSION,
    generated_by: "fixture",
    game_build: "1",
    policy: { formula_authority: false },
    selection: { ability_ids: [1] },
    gap_window_filter: null,
    transition_seed_filter: null,
    inputs: ["a.rlog"],
    attribute_states: [[{ attribute_id: 11_330, value: 10 }]],
    status_states: [[]],
    samples: [{
      source_attribute_state_id: 0,
      direct_source_attribute_state_id: 0,
      target_attribute_state_id: 0,
      source_status_state_id: 0,
      target_status_state_id: 0,
      status_provider_attribute_states: [{
        provider_entity_uuid: 1,
        attribute_state_id: 0,
      }],
    }],
  };
  const second = structuredClone(base);
  second.inputs = ["b.rlog"];
  const merged = mergeCohorts([base, second], [import.meta.filename, import.meta.filename]);
  assert.equal(merged.attribute_states.length, 2);
  assert.equal(merged.status_states.length, 2);
  assert.equal(merged.samples[1].source_attribute_state_id, 1);
  assert.equal(merged.samples[1].direct_source_attribute_state_id, 1);
  assert.equal(merged.samples[1].source_status_state_id, 1);
  assert.equal(
    merged.samples[1].status_provider_attribute_states[0].attribute_state_id,
    1,
  );
  process.stdout.write("self-test passed\n");
}

function fileReceipt(filePath) {
  const stat = fs.statSync(filePath);
  return {
    path: path.resolve(filePath).replaceAll("\\", "/"),
    bytes: stat.size,
    sha256: sha256(filePath),
  };
}

function verifyFileReceipt(receipt) {
  const actual = fileReceipt(receipt.path);
  assert.equal(actual.bytes, receipt.bytes);
  assert.equal(actual.sha256, receipt.sha256);
}

function sha256(filePath) {
  const hash = crypto.createHash("sha256");
  const fd = fs.openSync(filePath, "r");
  const buffer = Buffer.allocUnsafe(1024 * 1024);
  try {
    for (;;) {
      const bytes = fs.readSync(fd, buffer, 0, buffer.length, null);
      if (bytes === 0) break;
      hash.update(buffer.subarray(0, bytes));
    }
  } finally {
    fs.closeSync(fd);
  }
  return hash.digest("hex");
}

function contentHash(value) {
  const copy = structuredClone(value);
  delete copy.content_sha256;
  return crypto.createHash("sha256").update(stableStringify(copy)).digest("hex");
}

function stableStringify(value) {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map(
      (key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`,
    ).join(",")}}`;
  }
  return JSON.stringify(value);
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function refuseExisting(filePath) {
  if (fs.existsSync(filePath)) {
    throw new Error(`refusing to overwrite existing output: ${filePath}`);
  }
}

function required(value, key) {
  if (!value) throw new Error(`missing --${key}`);
  return value;
}

function parseArgs(args) {
  const options = { inputs: [], input: null, output: null };
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index];
    const value = args[index + 1];
    if (!key?.startsWith("--") || value == null) usage(1);
    if (key === "--input" && command === "generate") options.inputs.push(value);
    else if (key === "--input") options.input = value;
    else if (key === "--output") options.output = value;
    else usage(1);
  }
  return options;
}

function usage(exitCode) {
  process.stderr.write(
    "usage:\n" +
    "  node tools/bpsr-formula-cohort-merge.mjs generate --input FILE --input FILE [--input FILE ...] --output FILE\n" +
    "  node tools/bpsr-formula-cohort-merge.mjs verify --input FILE\n" +
    "  node tools/bpsr-formula-cohort-merge.mjs self-test\n",
  );
  process.exit(exitCode);
}
