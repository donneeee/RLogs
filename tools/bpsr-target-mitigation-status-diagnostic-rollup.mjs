#!/usr/bin/env node

import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 1;
const GENERATOR = "tools/bpsr-target-mitigation-status-diagnostic-rollup.mjs";
const DIAGNOSTIC_GENERATOR =
  "rlogs-bpsr-target-mitigation-transform-proof:target-status-relaxed-diagnostic";
const EXPECTED_AXES = new Map([
  ["physical_defense", 11350], ["magic_defense", 11360], ["refined_defense", 11420],
  ["general_element_defense", 13200], ["fire_element_defense", 13210],
  ["water_element_defense", 13220], ["wood_element_defense", 13230],
  ["electric_element_defense", 13240], ["wind_element_defense", 13250],
  ["rock_element_defense", 13260], ["light_element_defense", 13270],
  ["dark_element_defense", 13280],
]);
const COUNTERS = [
  "samples_with_axis", "groups_with_multiple_target_status_or_axis_variants",
  "distinct_axis_pairs", "deterministic_pairs", "equal_output_pairs",
  "divergent_output_pairs", "nondeterministic_pairs",
  "pairs_with_selected_effect_in_status_delta",
  "pairs_with_only_selected_effect_in_status_delta", "same_axis_status_pairs",
  "same_axis_deterministic_pairs", "same_axis_equal_output_pairs",
  "same_axis_divergent_output_pairs", "same_axis_nondeterministic_pairs",
  "same_axis_pairs_with_selected_effect_in_status_delta",
  "same_axis_pairs_with_only_selected_effect_in_status_delta",
];
const CANDIDATE_CONSTANTS = [22000n, 6500n];

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);
if (command === "build") buildCommand(options);
else if (command === "verify") verifyCommand(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function buildCommand(parsed) {
  const build = numericString(required(parsed, "build"), "build");
  const effectId = positiveInteger(required(parsed, "effect"), "effect");
  const files = requiredMany(parsed, "diagnostic").map((file) => path.resolve(file));
  const output = path.resolve(required(parsed, "output"));
  if (new Set(files.map(normalizedPath)).size !== files.length) {
    throw new Error("duplicate target-status diagnostic input");
  }
  const inputs = files.map(fileDescriptor);
  const diagnostics = files.map((file, index) =>
    validateDiagnostic(readJson(file, "target-status diagnostic"), build, effectId, inputs[index]));
  const report = buildReport(build, effectId, diagnostics, inputs);
  report.content_sha256 = contentHash(report);
  verifyReport(report);
  mkdirSync(path.dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  verifyInputs(report);
  console.log(JSON.stringify(report.summary, null, 2));
}

function buildReport(build, effectId, diagnostics, inputs) {
  if (diagnostics.length === 0 || diagnostics.length !== inputs.length) {
    throw new Error("status diagnostic rollup requires matching evidence and descriptors");
  }
  const seenCohorts = new Set();
  const sourceRlogs = new Set();
  const axes = Object.fromEntries([...EXPECTED_AXES].map(([name, currentAttributeId]) => [name, {
    current_attribute_id: currentAttributeId,
    counters: Object.fromEntries(COUNTERS.map((counter) => [counter, 0])),
    near_pair_examples: [],
    same_axis_status_examples: [],
  }]));
  let damageSamples = 0;
  let maximumPeakBytes = 0;
  for (const diagnostic of diagnostics) {
    const cohortKey = normalizedPath(diagnostic.input.path);
    if (seenCohorts.has(cohortKey)) throw new Error(`duplicate cohort identity ${diagnostic.input.path}`);
    seenCohorts.add(cohortKey);
    diagnostic.input.source_inputs.forEach((source) => sourceRlogs.add(path.basename(String(source)).toLowerCase()));
    damageSamples += diagnostic.processing.sample_count;
    maximumPeakBytes = Math.max(maximumPeakBytes, diagnostic.processing.measured_peak_working_set_bytes);
    for (const [name] of EXPECTED_AXES) {
      const target = axes[name];
      const source = diagnostic.axes[name];
      for (const counter of COUNTERS) target.counters[counter] += Number(source.counters[counter]);
      target.near_pair_examples.push(...source.near_pair_examples.map((example) => ({
        input_sha256: diagnostic.descriptor.sha256,
        cohort_sha256: diagnostic.input.sha256,
        ...structuredClone(example),
      })));
      target.same_axis_status_examples.push(...source.same_axis_status_examples.map((example) => ({
        input_sha256: diagnostic.descriptor.sha256,
        cohort_sha256: diagnostic.input.sha256,
        ...structuredClone(example),
      })));
    }
  }
  for (const axis of Object.values(axes)) {
    axis.near_pair_examples = deduplicateExamples(axis.near_pair_examples);
    axis.same_axis_status_examples = deduplicateExamples(axis.same_axis_status_examples);
    axis.formula_authority = false;
    axis.runtime_authority = false;
  }
  const physical = axes.physical_defense;
  const candidateEvaluation = Object.fromEntries(CANDIDATE_CONSTANTS.map((constant) => {
    const rows = physical.near_pair_examples.map((example) => ({
      signature: exampleSignature(example),
      shared_base_interval: sharedBaseInterval(example, constant),
    }));
    return [constant.toString(), {
      model: "floor(nonnegative_base * constant / (constant + target_physical_defense_raw))",
      evaluated_unique_near_pairs: rows.length,
      compatible_unique_near_pairs: rows.filter((row) => row.shared_base_interval !== null).length,
      rejected_unique_near_pairs: rows.filter((row) => row.shared_base_interval === null).length,
      rows,
      candidate_compatibility_is_not_formula_proof: true,
      formula_authority: false,
    }];
  }));
  const selectedNearPairs = Number(physical.counters.pairs_with_selected_effect_in_status_delta);
  const selectedSameAxisPairs = Number(
    physical.counters.same_axis_pairs_with_selected_effect_in_status_delta,
  );
  return {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATOR,
    game_build: build,
    selected_effect_id: effectId,
    policy: {
      exact_numeric_effect_ids_attribute_ids_and_build_are_authoritative: true,
      exact_input_hashes_are_embedded_and_verified: true,
      every_capture_is_analyzed_independently: true,
      same_capture_only: true,
      cross_capture_pairing_allowed: false,
      complete_target_status_row_deltas_are_preserved: true,
      target_status_relaxation_is_diagnostic_only: true,
      near_pair_is_not_controlled_counterfactual_proof: true,
      absence_of_additional_local_pairs_is_not_formula_proof: true,
      structurally_unobservable_remote_player_packets_are_not_acquisition_requirements: true,
      acquisition_must_use_locally_observable_events_or_offline_exact_build_evidence: true,
      unknown_and_unresolved_evidence_is_preserved: true,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    inputs: { diagnostics: inputs.map((input) => structuredClone(input)) },
    summary: {
      matching_build_capture_diagnostics: diagnostics.length,
      unique_cohort_inputs: seenCohorts.size,
      unique_source_rlogs: sourceRlogs.size,
      damage_samples: damageSamples,
      audited_axis_samples: Object.values(axes).reduce(
        (sum, axis) => sum + Number(axis.counters.samples_with_axis), 0),
      physical_defense_axis_samples: Number(physical.counters.samples_with_axis),
      physical_defense_unique_near_pairs: physical.near_pair_examples.length,
      physical_defense_same_axis_status_pairs: physical.same_axis_status_examples.length,
      physical_defense_pairs_with_selected_effect_in_status_delta: selectedNearPairs,
      physical_defense_same_axis_pairs_with_selected_effect_in_status_delta: selectedSameAxisPairs,
      diagnostics_with_physical_defense_near_pairs: diagnostics.filter((diagnostic) =>
        Number(diagnostic.axes.physical_defense.counters.distinct_axis_pairs) > 0).length,
      maximum_measured_peak_working_set_bytes: maximumPeakBytes,
      maximum_measured_peak_working_set_mib: maximumPeakBytes / 1024 / 1024,
      exhaustive_local_search_added_independent_near_pair_cohorts:
        diagnostics.filter((diagnostic) =>
          Number(diagnostic.axes.physical_defense.counters.distinct_axis_pairs) > 0).length > 1,
      exact_target_mitigation_formula_proven: false,
      exact_operation_order_and_integer_rounding_proven: false,
      packet_conservation_proven: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    axes,
    physical_defense_candidate_evaluation: candidateEvaluation,
    conclusions: {
      selected_effect_occurs_in_every_observed_physical_defense_near_pair:
        physical.near_pair_examples.length > 0 && selectedNearPairs === physical.near_pair_examples.length,
      selected_effect_has_same_axis_damage_witness: selectedSameAxisPairs > 0,
      observed_near_pairs_remain_target_status_confounded: physical.near_pair_examples.length > 0,
      no_new_independent_local_control_was_found:
        diagnostics.filter((diagnostic) =>
          Number(diagnostic.axes.physical_defense.counters.distinct_axis_pairs) > 0).length <= 1,
      remote_player_packet_acquisition_required: false,
      next_safe_evidence_routes: [
        "locally observable same-capture exact target-defense control",
        "exact-build offline client equation with proven combat-stage binding",
        "canonical replay conservation after operation order and integer rounding are proven",
      ],
    },
    blockers: [
      "all observed physical-defense near-pairs also change target status state",
      "the selected effect has no same-axis damage witness in the searched local cohorts",
      "combat-side target-mitigation row selection is unproven",
      "armor penetration or reduction order relative to defense conversion is unproven",
      "damage-stage integer rounding and canonical replay conservation are unproven",
    ],
  };
}

function validateDiagnostic(value, build, effectId, descriptor) {
  if (Number(value?.schema_version) !== 3 || value?.generated_by !== DIAGNOSTIC_GENERATOR ||
    String(value?.game_build) !== build || Number(value?.selected_effect_id) !== effectId ||
    value?.policy?.same_capture_only !== true || value?.policy?.cross_capture_pairing_allowed !== false ||
    value?.policy?.only_target_status_state_is_relaxed !== true ||
    value?.policy?.complete_target_status_row_deltas_are_preserved !== true ||
    value?.policy?.same_axis_status_variants_are_audited_for_exact_outcome_invariance !== true ||
    value?.policy?.near_pair_is_not_controlled_counterfactual_proof !== true ||
    value?.authority?.exact_target_mitigation_formula_proven !== false ||
    value?.authority?.formula_authority !== false || value?.authority?.runtime_authority !== false ||
    value?.authority?.provider_rdps_credit_allowed !== false) {
    throw new Error(`unsafe target-status diagnostic ${descriptor.path}`);
  }
  validateCohortDescriptor(value.input);
  if (!Number.isSafeInteger(Number(value?.processing?.sample_count)) ||
    Number(value.processing.sample_count) <= 0 ||
    !Number.isSafeInteger(Number(value?.processing?.measured_peak_working_set_bytes)) ||
    Number(value.processing.measured_peak_working_set_bytes) <= 0 ||
    Number(value.processing.memory_limit_mib) < 64 ||
    value.processing.measured_peak_within_configured_limit !== true ||
    Number(value.processing.measured_peak_working_set_bytes) >
      Number(value.processing.memory_limit_mib) * 1024 * 1024) {
    throw new Error(`diagnostic is not bounded-memory evidence ${descriptor.path}`);
  }
  if (JSON.stringify(Object.keys(value.axes ?? {}).sort()) !==
    JSON.stringify([...EXPECTED_AXES.keys()].sort())) {
    throw new Error(`diagnostic axis inventory changed ${descriptor.path}`);
  }
  for (const [name, currentAttributeId] of EXPECTED_AXES) {
    const axis = value.axes[name];
    if (Number(axis?.current_attribute_id) !== currentAttributeId || !axis?.counters ||
      !Array.isArray(axis.near_pair_examples) || !Array.isArray(axis.same_axis_status_examples)) {
      throw new Error(`diagnostic axis ${name} is invalid`);
    }
    for (const counter of COUNTERS) nonNegativeInteger(axis.counters[counter], `${name}.${counter}`);
    if (axis.near_pair_examples.length !== Number(axis.counters.distinct_axis_pairs) ||
      axis.same_axis_status_examples.length !== Number(axis.counters.same_axis_status_pairs)) {
      throw new Error(`diagnostic examples are truncated for ${name}`);
    }
  }
  return {
    descriptor: structuredClone(descriptor),
    input: structuredClone(value.input),
    processing: {
      sample_count: Number(value.processing.sample_count),
      measured_peak_working_set_bytes: Number(value.processing.measured_peak_working_set_bytes),
    },
    axes: structuredClone(value.axes),
  };
}

function verifyCommand(parsed) {
  const input = path.resolve(required(parsed, "input"));
  const report = readJson(input, "status diagnostic rollup");
  verifyReport(report);
  verifyInputs(report);
  console.log(`verified ${input}`);
}

function verifyReport(report) {
  const summary = report?.summary;
  const physical = report?.axes?.physical_defense;
  if (Number(report?.schema_version) !== SCHEMA_VERSION || report?.generated_by !== GENERATOR ||
    !/^\d+$/.test(String(report?.game_build ?? "")) ||
    !Number.isSafeInteger(Number(report?.selected_effect_id)) || Number(report.selected_effect_id) <= 0 ||
    report?.content_sha256 !== contentHash(report) ||
    report?.policy?.same_capture_only !== true || report?.policy?.cross_capture_pairing_allowed !== false ||
    report?.policy?.target_status_relaxation_is_diagnostic_only !== true ||
    report?.policy?.near_pair_is_not_controlled_counterfactual_proof !== true ||
    report?.policy?.structurally_unobservable_remote_player_packets_are_not_acquisition_requirements !== true ||
    report?.policy?.formula_authority !== false || report?.policy?.runtime_authority !== false ||
    report?.policy?.ui_display_authority !== false || report?.policy?.provider_rdps_credit_allowed !== false ||
    !Array.isArray(report?.inputs?.diagnostics) || report.inputs.diagnostics.length === 0 ||
    Number(summary?.matching_build_capture_diagnostics) !== report.inputs.diagnostics.length ||
    Number(summary?.unique_cohort_inputs) !== report.inputs.diagnostics.length ||
    Number(summary?.damage_samples) <= 0 || Number(summary?.maximum_measured_peak_working_set_bytes) <= 0 ||
    summary?.exact_target_mitigation_formula_proven !== false ||
    summary?.exact_operation_order_and_integer_rounding_proven !== false ||
    summary?.packet_conservation_proven !== false || summary?.formula_authority !== false ||
    summary?.runtime_authority !== false || summary?.ui_display_authority !== false ||
    summary?.provider_rdps_credit_allowed !== false ||
    report?.conclusions?.remote_player_packet_acquisition_required !== false ||
    !Array.isArray(report?.conclusions?.next_safe_evidence_routes) ||
    !Array.isArray(report?.blockers) || report.blockers.length === 0) {
    throw new Error("status diagnostic rollup violates its fail-closed schema");
  }
  for (const descriptor of report.inputs.diagnostics) validateDescriptor(descriptor);
  if (JSON.stringify(Object.keys(report.axes ?? {}).sort()) !==
    JSON.stringify([...EXPECTED_AXES.keys()].sort())) throw new Error("rollup axis inventory changed");
  for (const [name, currentAttributeId] of EXPECTED_AXES) {
    const axis = report.axes[name];
    if (Number(axis?.current_attribute_id) !== currentAttributeId || axis?.formula_authority !== false ||
      axis?.runtime_authority !== false || !Array.isArray(axis?.near_pair_examples) ||
      !Array.isArray(axis?.same_axis_status_examples)) throw new Error(`unsafe rollup axis ${name}`);
    for (const counter of COUNTERS) nonNegativeInteger(axis.counters?.[counter], `${name}.${counter}`);
  }
  if (Number(summary.physical_defense_unique_near_pairs) !== physical.near_pair_examples.length ||
    Number(summary.physical_defense_same_axis_status_pairs) !== physical.same_axis_status_examples.length ||
    Number(summary.physical_defense_pairs_with_selected_effect_in_status_delta) !==
      Number(physical.counters.pairs_with_selected_effect_in_status_delta) ||
    Number(summary.physical_defense_same_axis_pairs_with_selected_effect_in_status_delta) !==
      Number(physical.counters.same_axis_pairs_with_selected_effect_in_status_delta)) {
    throw new Error("physical-defense summary does not reconcile");
  }
  for (const constant of CANDIDATE_CONSTANTS) {
    const model = report.physical_defense_candidate_evaluation?.[constant.toString()];
    if (model?.candidate_compatibility_is_not_formula_proof !== true || model?.formula_authority !== false ||
      Number(model?.evaluated_unique_near_pairs) !== physical.near_pair_examples.length ||
      Number(model?.compatible_unique_near_pairs) + Number(model?.rejected_unique_near_pairs) !==
        physical.near_pair_examples.length) throw new Error(`unsafe candidate model ${constant}`);
  }
}

function deduplicateExamples(examples) {
  const rows = new Map();
  for (const example of examples) {
    const key = exampleSignature(example);
    if (!rows.has(key)) rows.set(key, example);
  }
  return [...rows.values()].sort((left, right) => exampleSignature(left).localeCompare(exampleSignature(right)));
}

function exampleSignature(example) {
  const copy = structuredClone(example);
  delete copy.input_sha256;
  delete copy.cohort_sha256;
  return stableStringify(copy);
}

function sharedBaseInterval(example, constant) {
  const left = basePreimage(BigInt(example.left_outcome.amount), BigInt(example.left_raw), constant);
  const right = basePreimage(BigInt(example.right_outcome.amount), BigInt(example.right_raw), constant);
  const minimum = left[0] > right[0] ? left[0] : right[0];
  const maximum = left[1] < right[1] ? left[1] : right[1];
  return minimum <= maximum ? { minimum: minimum.toString(), maximum: maximum.toString() } : null;
}
function basePreimage(output, raw, constant) {
  const denominator = constant + raw;
  return [ceilDiv(output * denominator, constant), ceilDiv((output + 1n) * denominator, constant) - 1n];
}
function ceilDiv(numerator, denominator) { return (numerator + denominator - 1n) / denominator; }

function verifyInputs(report) {
  for (const descriptor of report.inputs.diagnostics) {
    const bytes = readFileSync(path.resolve(descriptor.path));
    if (bytes.length !== Number(descriptor.bytes) ||
      createHash("sha256").update(bytes).digest("hex") !== descriptor.sha256) {
      throw new Error(`input changed: ${descriptor.path}`);
    }
  }
}

function selfTest() {
  const compatible = sharedBaseInterval({
    left_outcome: { amount: 84356 }, left_raw: 5907,
    right_outcome: { amount: 86011 }, right_raw: 5370,
  }, 22000n);
  const rejected = sharedBaseInterval({
    left_outcome: { amount: 84356 }, left_raw: 5907,
    right_outcome: { amount: 86011 }, right_raw: 5370,
  }, 6500n);
  if (stableStringify(compatible) !== stableStringify({ minimum: "107006", maximum: "107006" }) ||
    rejected !== null) throw new Error("integer candidate evaluation failed");
  const unsafe = {
    schema_version: SCHEMA_VERSION, generated_by: GENERATOR, game_build: "1", selected_effect_id: 1,
    policy: {}, inputs: { diagnostics: [] }, summary: {}, axes: {}, conclusions: {}, blockers: [],
  };
  unsafe.content_sha256 = contentHash(unsafe);
  try { verifyReport(unsafe); throw new Error("unsafe fixture was accepted"); } catch (error) {
    if (error.message === "unsafe fixture was accepted") throw error;
  }
  console.log("bpsr-target-mitigation-status-diagnostic-rollup self-test passed");
}

function fileDescriptor(file) {
  const bytes = readFileSync(file);
  return { path: file.replaceAll("\\", "/"), bytes: statSync(file).size,
    sha256: createHash("sha256").update(bytes).digest("hex") };
}
function validateDescriptor(value) {
  if (!String(value?.path ?? "") || !Number.isSafeInteger(Number(value?.bytes)) || Number(value.bytes) <= 0 ||
    !/^[0-9a-f]{64}$/.test(String(value?.sha256 ?? ""))) throw new Error("invalid file descriptor");
}
function validateCohortDescriptor(value) {
  if (!String(value?.path ?? "") || !Number.isSafeInteger(Number(value?.bytes)) || Number(value.bytes) <= 0 ||
    !/^sha256:[0-9a-f]{64}$/.test(String(value?.sha256 ?? "")) ||
    !Array.isArray(value?.source_inputs) || value.source_inputs.length === 0) {
    throw new Error("invalid cohort descriptor");
  }
}
function contentHash(value) {
  const copy = structuredClone(value);
  delete copy.content_sha256;
  return createHash("sha256").update(stableStringify(copy)).digest("hex");
}
function stableStringify(value) {
  if (value === null || typeof value !== "object") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  return `{${Object.keys(value).sort().map((key) =>
    `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`;
}
function readJson(file, label) {
  try { return JSON.parse(readFileSync(file, "utf8")); }
  catch (error) { throw new Error(`unable to read ${label} ${file}: ${error.message}`); }
}
function normalizedPath(value) { return path.resolve(String(value)).replaceAll("\\", "/").toLowerCase(); }
function nonNegativeInteger(value, label) {
  if (!Number.isSafeInteger(Number(value)) || Number(value) < 0) throw new Error(`${label} is invalid`);
}
function numericString(value, label) {
  if (!/^\d+$/.test(String(value))) throw new Error(`${label} must be numeric`);
  return String(value);
}
function positiveInteger(value, label) {
  const number = Number(value);
  if (!Number.isSafeInteger(number) || number <= 0) throw new Error(`${label} must be positive`);
  return number;
}
function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index]?.replace(/^--/, "");
    const value = args[index + 1];
    if (!key || value === undefined) throw new Error(`invalid argument near ${args[index]}`);
    if (key === "diagnostic") (parsed[key] ??= []).push(value);
    else parsed[key] = value;
  }
  return parsed;
}
function required(parsed, key) { if (!parsed[key]) throw new Error(`missing --${key}`); return parsed[key]; }
function requiredMany(parsed, key) {
  if (!Array.isArray(parsed[key]) || parsed[key].length === 0) throw new Error(`missing --${key}`);
  return parsed[key];
}
function usage(code) {
  console.log("Usage:\n  node tools/bpsr-target-mitigation-status-diagnostic-rollup.mjs build --build <id> --effect <id> --diagnostic <json> [--diagnostic <json> ...] --output <json>\n  node tools/bpsr-target-mitigation-status-diagnostic-rollup.mjs verify --input <json>\n  node tools/bpsr-target-mitigation-status-diagnostic-rollup.mjs self-test");
  process.exit(code);
}
