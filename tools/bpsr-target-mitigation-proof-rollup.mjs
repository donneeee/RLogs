#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

const SCHEMA_VERSION = 1;
const GENERATOR = "tools/bpsr-target-mitigation-proof-rollup.mjs";
const EXPECTED_AXES = new Map([
  ["physical_defense", { id: 11350, constants: [22000, 6500] }],
  ["magic_defense", { id: 11360, constants: [22000, 6500] }],
  ["refined_defense", { id: 11420, constants: [9980, 6500] }],
  ["general_element_defense", { id: 13200, constants: [11000] }],
  ["fire_element_defense", { id: 13210, constants: [11000] }],
  ["water_element_defense", { id: 13220, constants: [11000] }],
  ["wood_element_defense", { id: 13230, constants: [11000] }],
  ["electric_element_defense", { id: 13240, constants: [11000] }],
  ["wind_element_defense", { id: 13250, constants: [11000] }],
  ["rock_element_defense", { id: 13260, constants: [11000] }],
  ["light_element_defense", { id: 13270, constants: [11000] }],
  ["dark_element_defense", { id: 13280, constants: [11000] }],
]);

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "build") buildCommand(options);
else if (command === "verify") verifyCommand(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function buildCommand(parsed) {
  const build = numericString(required(parsed, "build"), "build");
  const proofPaths = requiredMany(parsed, "proof").map((input) => path.resolve(input));
  const output = path.resolve(required(parsed, "output"));
  const proofs = proofPaths.map((input) => readJson(input, "target mitigation proof"));
  const report = buildReport(build, proofs, proofPaths.map((input) => fileDescriptor(input)), true);
  report.content_sha256 = contentHash(report);
  verifyReport(report);
  mkdirSync(path.dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  console.log(`wrote ${output}`);
}

function verifyCommand(parsed) {
  const input = path.resolve(required(parsed, "input"));
  verifyReport(readJson(input, "target mitigation rollup"));
  console.log(`verified ${input}`);
}

function buildReport(build, proofs, proofInputs, verifyCohortFiles) {
  if (proofs.length === 0 || proofs.length !== proofInputs.length) {
    throw new Error("Target mitigation rollup requires at least one proof and matching provenance");
  }
  const seenProofs = new Set();
  const seenCohorts = new Set();
  const seenSourceInputs = new Set();
  const runs = [];
  const axes = new Map([...EXPECTED_AXES].map(([name, expected]) => [name, emptyAxis(expected)]));

  for (let index = 0; index < proofs.length; index += 1) {
    const proof = proofs[index];
    const proofInput = proofInputs[index];
    validateProof(proof, proofInput, build, verifyCohortFiles);
    const proofKey = normalizedPath(proofInput.path);
    const cohortKey = normalizedPath(proof.input.path);
    if (seenProofs.has(proofKey) || seenCohorts.has(cohortKey)) {
      throw new Error("Target mitigation rollup contains a duplicate proof or cohort input");
    }
    seenProofs.add(proofKey);
    seenCohorts.add(cohortKey);
    for (const sourceInput of proof.input.source_inputs) {
      const sourceKey = path.basename(String(sourceInput)).toLowerCase();
      if (!sourceKey || seenSourceInputs.has(sourceKey)) {
        throw new Error("Target mitigation rollup contains a missing or duplicate source RLOG identity");
      }
      seenSourceInputs.add(sourceKey);
    }
    runs.push({
      proof: structuredClone(proofInput),
      cohort: structuredClone(proof.input),
      processing: structuredClone(proof.processing),
    });
    for (const [name, expected] of EXPECTED_AXES) {
      mergeAxis(axes.get(name), proof.axes[name], expected);
    }
  }
  runs.sort((left, right) => String(left.cohort.path).localeCompare(String(right.cohort.path)));
  const axisResults = Object.fromEntries([...axes].map(([name, axis]) => [name, finalizeAxis(axis)]));
  const axisValues = Object.values(axisResults);
  const total = (selector) => axisValues.reduce((sum, axis) => sum + selector(axis), 0);
  const controlledGroups = total((axis) => axis.counters.controlled_groups);
  const deterministicPairs = total((axis) => axis.counters.deterministic_pairs);
  const divergentPairs = total((axis) => axis.counters.divergent_output_pairs);
  const rejectedModelPairs = total((axis) =>
    Object.values(axis.models).reduce((sum, model) => sum + model.counters.rejected_pairs, 0)
  );
  const candidateModelsWithDivergentSupport = axisValues.reduce((sum, axis) =>
    sum + Object.values(axis.models).filter((model) =>
      axis.counters.divergent_output_pairs > 0 && model.counters.exact_pairs > 0 &&
      model.counters.rejected_pairs === 0
    ).length, 0);

  return {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATOR,
    game_build: build,
    policy: {
      exact_numeric_attribute_ids_and_build_are_authoritative: true,
      exact_input_hashes_are_embedded_and_verified: true,
      every_capture_is_analyzed_independently: true,
      cross_capture_pairing_allowed: false,
      bounded_memory_measurement_required_for_every_input: true,
      absence_of_controlled_pairs_is_not_formula_proof: true,
      candidate_curve_compatibility_is_not_combat_stage_authority: true,
      unresolved_evidence_is_preserved: true,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
    },
    inputs: { proofs: proofInputs.map((input) => structuredClone(input)) },
    runs,
    summary: {
      matching_build_capture_proofs: runs.length,
      matching_build_source_rlogs: seenSourceInputs.size,
      cohort_input_bytes: runs.reduce((sum, run) => sum + Number(run.cohort.bytes), 0),
      damage_samples: runs.reduce((sum, run) => sum + Number(run.processing.sample_count), 0),
      audited_axis_samples: total((axis) => axis.counters.samples_with_axis),
      controlled_groups: controlledGroups,
      deterministic_pairs: deterministicPairs,
      divergent_output_pairs: divergentPairs,
      rejected_model_pairs: rejectedModelPairs,
      candidate_models_with_divergent_support: candidateModelsWithDivergentSupport,
      maximum_measured_peak_working_set_bytes: Math.max(
        ...runs.map((run) => Number(run.processing.measured_peak_working_set_bytes)),
      ),
      maximum_measured_peak_working_set_mib: Math.max(
        ...runs.map((run) => Number(run.processing.measured_peak_working_set_mib)),
      ),
      exact_target_mitigation_formula_proven: false,
      operation_order_and_integer_rounding_proven: false,
      packet_conservation_proven: false,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
    },
    axes: axisResults,
    status: controlledGroups === 0
      ? "no-controlled-target-mitigation-pairs"
      : "controlled-target-mitigation-pairs-observed-review-required",
    blockers: [
      ...(controlledGroups === 0
        ? ["no same-capture exact target-mitigation counterfactual pair was observed"]
        : []),
      "combat-side target-mitigation row selection is unproven",
      "armor penetration or reduction operation relative to defense conversion is unproven",
      "damage-stage operation order and integer rounding are unproven",
      "canonical replay conservation is unproven",
    ],
  };
}

function validateProof(proof, proofInput, build, verifyCohortFile) {
  validateDescriptor(proofInput, false);
  if (proof?.schema_version !== 2 ||
    proof?.generated_by !== "rlogs-bpsr-target-mitigation-transform-proof" ||
    String(proof?.game_build) !== build ||
    proof?.policy?.runtime_authority !== false || proof?.policy?.formula_authority !== false ||
    proof?.policy?.unresolved_evidence_is_hidden !== false ||
    proof?.policy?.disk_partitions_preserve_exact_group_semantics !== true ||
    proof?.policy?.cross_capture_pairing_allowed !== false ||
    !proof.input || !Array.isArray(proof.input.source_inputs) ||
    proof.input.source_inputs.length === 0 || !proof.processing || !proof.axes) {
    throw new Error("Target mitigation proof violates schema-2 fail-closed policy");
  }
  validateDescriptor(proof.input, true);
  if (!Number.isSafeInteger(Number(proof.processing.memory_limit_mib)) ||
    Number(proof.processing.memory_limit_mib) < 64 ||
    !Number.isSafeInteger(Number(proof.processing.partition_count)) ||
    Number(proof.processing.partition_count) < 16 ||
    !Number.isSafeInteger(Number(proof.processing.sample_count)) ||
    Number(proof.processing.sample_count) <= 0 ||
    !Number.isSafeInteger(Number(proof.processing.measured_peak_working_set_bytes)) ||
    Number(proof.processing.measured_peak_working_set_bytes) <= 0 ||
    Number(proof.processing.measured_peak_working_set_mib) <= 0 ||
    proof.processing.measured_peak_within_configured_limit !== true ||
    Number(proof.processing.measured_peak_working_set_bytes) >
      Number(proof.processing.memory_limit_mib) * 1024 * 1024) {
    throw new Error("Target mitigation proof lacks a valid bounded-memory measurement");
  }
  const axisNames = Object.keys(proof.axes).sort();
  if (JSON.stringify(axisNames) !== JSON.stringify([...EXPECTED_AXES.keys()].sort())) {
    throw new Error("Target mitigation proof does not preserve all twelve exact axes");
  }
  for (const [name, expected] of EXPECTED_AXES) validateAxis(proof.axes[name], expected, name);
  if (verifyCohortFile) {
    const cohortPath = path.resolve(String(proof.input.path));
    if (!existsSync(cohortPath)) throw new Error(`Missing target mitigation cohort ${cohortPath}`);
    const actual = fileDescriptor(cohortPath, true);
    if (actual.bytes !== Number(proof.input.bytes) || actual.sha256 !== proof.input.sha256) {
      throw new Error(`Target mitigation cohort identity changed: ${cohortPath}`);
    }
  }
}

function validateAxis(axis, expected, name) {
  if (Number(axis?.current_attribute_id) !== expected.id ||
    !Array.isArray(axis?.family_attribute_ids) || axis.family_attribute_ids.length !== 6 ||
    !axis.counters || !axis.models) {
    throw new Error(`Target mitigation axis ${name} has invalid identity or coverage`);
  }
  for (const value of Object.values(axis.counters)) nonNegativeInteger(value, `${name} counter`);
  const constants = Object.values(axis.models).map((model) => Number(model.constant)).sort((a, b) => b - a);
  const expectedConstants = [...expected.constants].sort((a, b) => b - a);
  if (JSON.stringify(constants) !== JSON.stringify(expectedConstants)) {
    throw new Error(`Target mitigation axis ${name} candidate constants changed`);
  }
  for (const model of Object.values(axis.models)) {
    nonNegativeInteger(model.counters?.exact_pairs, `${name} exact pairs`);
    nonNegativeInteger(model.counters?.rejected_pairs, `${name} rejected pairs`);
  }
}

function emptyAxis(expected) {
  return {
    current_attribute_id: expected.id,
    family_attribute_ids: [],
    required_packet_property: undefined,
    counters: {
      samples_with_axis: 0,
      controlled_groups: 0,
      distinct_axis_pairs: 0,
      deterministic_pairs: 0,
      equal_output_pairs: 0,
      divergent_output_pairs: 0,
      nondeterministic_pairs: 0,
    },
    models: {},
  };
}

function mergeAxis(target, source, expected) {
  validateAxis(source, expected, String(source.current_attribute_id));
  if (target.family_attribute_ids.length === 0) {
    target.family_attribute_ids = structuredClone(source.family_attribute_ids);
    target.required_packet_property = source.required_packet_property ?? null;
    target.models = Object.fromEntries(Object.entries(source.models).map(([name, model]) => [name, {
      constant: Number(model.constant),
      counters: { exact_pairs: 0, rejected_pairs: 0 },
      exact_examples: [],
      rejected_examples: [],
    }]));
  } else if (JSON.stringify(target.family_attribute_ids) !== JSON.stringify(source.family_attribute_ids) ||
    target.required_packet_property !== (source.required_packet_property ?? null)) {
    throw new Error("Target mitigation axis identity changed across proofs");
  }
  for (const key of Object.keys(target.counters)) {
    target.counters[key] += Number(source.counters[key]);
  }
  for (const [name, sourceModel] of Object.entries(source.models)) {
    const targetModel = target.models[name];
    if (!targetModel || targetModel.constant !== Number(sourceModel.constant)) {
      throw new Error("Target mitigation model identity changed across proofs");
    }
    targetModel.counters.exact_pairs += Number(sourceModel.counters.exact_pairs);
    targetModel.counters.rejected_pairs += Number(sourceModel.counters.rejected_pairs);
    targetModel.exact_examples.push(...structuredClone(sourceModel.exact_examples ?? []));
    targetModel.rejected_examples.push(...structuredClone(sourceModel.rejected_examples ?? []));
  }
}

function finalizeAxis(axis) {
  return {
    ...axis,
    formula_proven: false,
    formula_authority: false,
    runtime_authority: false,
  };
}

function verifyReport(report) {
  if (report?.schema_version !== SCHEMA_VERSION || report?.generated_by !== GENERATOR ||
    !/^\d+$/.test(String(report?.game_build ?? "")) || report?.content_sha256 !== contentHash(report) ||
    report?.policy?.cross_capture_pairing_allowed !== false ||
    report?.policy?.absence_of_controlled_pairs_is_not_formula_proof !== true ||
    report?.policy?.formula_authority !== false || report?.policy?.runtime_authority !== false ||
    report?.policy?.provider_rdps_credit_allowed !== false ||
    !Array.isArray(report?.runs) || report.runs.length === 0 ||
    Number(report?.summary?.matching_build_capture_proofs) !== report.runs.length ||
    Number(report?.summary?.matching_build_source_rlogs) < report.runs.length ||
    Number(report?.summary?.maximum_measured_peak_working_set_bytes) <= 0 ||
    report?.summary?.exact_target_mitigation_formula_proven !== false ||
    report?.summary?.operation_order_and_integer_rounding_proven !== false ||
    report?.summary?.packet_conservation_proven !== false ||
    report?.summary?.formula_authority !== false || report?.summary?.runtime_authority !== false ||
    report?.summary?.provider_rdps_credit_allowed !== false ||
    !["no-controlled-target-mitigation-pairs", "controlled-target-mitigation-pairs-observed-review-required"]
      .includes(report?.status)) {
    throw new Error("Target mitigation rollup violates its fail-closed schema");
  }
  for (const proof of report.inputs?.proofs ?? []) validateDescriptor(proof, false);
  for (const [name, expected] of EXPECTED_AXES) {
    const axis = report.axes?.[name];
    validateAxis(axis, expected, name);
    if (axis.formula_proven !== false || axis.formula_authority !== false || axis.runtime_authority !== false) {
      throw new Error(`Target mitigation rollup unsafely promotes axis ${name}`);
    }
  }
  const controlledGroups = Object.values(report.axes).reduce(
    (sum, axis) => sum + Number(axis.counters.controlled_groups), 0,
  );
  if (controlledGroups !== Number(report.summary.controlled_groups) ||
    (controlledGroups === 0) !== (report.status === "no-controlled-target-mitigation-pairs")) {
    throw new Error("Target mitigation rollup summary does not match its axes");
  }
}

function selfTest() {
  const root = mkdtempSync(path.join(tmpdir(), "rlogs-target-mitigation-rollup-"));
  try {
    const cohort = path.join(root, "cohort.json");
    writeFileSync(cohort, "fixture\n", "utf8");
    const cohortDescriptor = fileDescriptor(cohort, true);
    const proof = fixtureProof(cohortDescriptor);
    const proofPath = path.join(root, "proof.json");
    writeFileSync(proofPath, `${JSON.stringify(proof)}\n`, "utf8");
    const report = buildReport("1", [proof], [fileDescriptor(proofPath)], true);
    report.content_sha256 = contentHash(report);
    verifyReport(report);
    const unsafe = structuredClone(report);
    unsafe.summary.exact_target_mitigation_formula_proven = true;
    unsafe.content_sha256 = contentHash(unsafe);
    expectReject(unsafe);
    const repeated = parseArgs(["--proof", "a", "--proof", "b"])["proof"];
    if (repeated?.join(",") !== "a,b") throw new Error("Repeated proof arguments were not preserved");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
  console.log("bpsr-target-mitigation-proof-rollup self-test passed");
}

function fixtureProof(cohort) {
  const axes = {};
  for (const [name, expected] of EXPECTED_AXES) {
    const models = {};
    expected.constants.forEach((constant, index) => {
      models[`candidate_${index}`] = {
        constant,
        counters: { exact_pairs: 0, rejected_pairs: 0 },
        exact_examples: [],
        rejected_examples: [],
      };
    });
    axes[name] = {
      current_attribute_id: expected.id,
      family_attribute_ids: [0, 1, 2, 3, 4, 5].map((offset) => expected.id + offset),
      required_packet_property: null,
      counters: {
        samples_with_axis: 1,
        controlled_groups: 0,
        distinct_axis_pairs: 0,
        deterministic_pairs: 0,
        equal_output_pairs: 0,
        divergent_output_pairs: 0,
        nondeterministic_pairs: 0,
      },
      models,
    };
  }
  return {
    schema_version: 2,
    generated_by: "rlogs-bpsr-target-mitigation-transform-proof",
    game_build: "1",
    policy: {
      runtime_authority: false,
      formula_authority: false,
      unresolved_evidence_is_hidden: false,
      disk_partitions_preserve_exact_group_semantics: true,
      cross_capture_pairing_allowed: false,
    },
    processing: {
      memory_limit_mib: 64,
      partition_count: 16,
      sample_count: 1,
      measured_peak_working_set_bytes: 1024,
      measured_peak_working_set_mib: 1024 / 1024 / 1024,
      measured_peak_within_configured_limit: true,
    },
    input: { ...cohort, source_inputs: ["fixture.rlog"] },
    axes,
  };
}

function expectReject(report) {
  try { verifyReport(report); } catch { return; }
  throw new Error("Self-test accepted unsafe target mitigation authority");
}

function fileDescriptor(file, prefixed = false) {
  const bytes = readFileSync(file);
  const sha = createHash("sha256").update(bytes).digest("hex");
  return {
    path: path.resolve(file).replaceAll("\\", "/"),
    bytes: statSync(file).size,
    sha256: prefixed ? `sha256:${sha}` : sha,
  };
}

function validateDescriptor(input, prefixed) {
  const pattern = prefixed ? /^sha256:[0-9a-f]{64}$/ : /^[0-9a-f]{64}$/;
  if (!String(input?.path ?? "") || !Number.isSafeInteger(Number(input?.bytes)) ||
    Number(input.bytes) <= 0 || !pattern.test(String(input?.sha256 ?? ""))) {
    throw new Error("Input descriptor lacks exact path, bytes, or SHA-256");
  }
}

function contentHash(value) {
  const copy = structuredClone(value);
  delete copy.content_sha256;
  return createHash("sha256").update(JSON.stringify(copy)).digest("hex");
}

function readJson(file, label) {
  try { return JSON.parse(readFileSync(file, "utf8")); }
  catch (error) { throw new Error(`Unable to read ${label} ${file}: ${error.message}`); }
}

function nonNegativeInteger(value, label) {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed < 0) throw new Error(`${label} must be a non-negative integer`);
  return parsed;
}

function numericString(value, label) {
  if (!/^\d+$/.test(String(value))) throw new Error(`${label} must contain only digits`);
  return String(value);
}

function normalizedPath(value) {
  return path.resolve(String(value)).replaceAll("\\", "/").toLowerCase();
}

function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 2) {
    const flag = args[index];
    const value = args[index + 1];
    if (!flag?.startsWith("--") || value === undefined || value.startsWith("--")) {
      throw new Error(`Invalid argument near ${flag ?? "<end>"}`);
    }
    const key = flag.slice(2);
    if (key === "proof") (parsed[key] ??= []).push(value);
    else parsed[key] = value;
  }
  return parsed;
}

function required(parsed, key) {
  if (!parsed[key]) throw new Error(`Missing --${key}`);
  return parsed[key];
}

function requiredMany(parsed, key) {
  const values = parsed[key];
  if (!Array.isArray(values) || values.length === 0) throw new Error(`Missing --${key}`);
  return values;
}

function usage(exitCode) {
  console.log("Usage:\n  node tools/bpsr-target-mitigation-proof-rollup.mjs build --build <id> --proof <json> [--proof <json> ...] --output <json>\n  node tools/bpsr-target-mitigation-proof-rollup.mjs verify --input <json>\n  node tools/bpsr-target-mitigation-proof-rollup.mjs self-test");
  process.exit(exitCode);
}
