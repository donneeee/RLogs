#!/usr/bin/env node

import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "build") buildCommand(options);
else if (command === "verify") {
  const inputs = required(options, "input");
  if (!Array.isArray(inputs) || inputs.length !== 1) {
    throw new Error("verify requires exactly one --input");
  }
  verifyFile(path.resolve(inputs[0]));
}
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function buildCommand(parsed) {
  const inputs = (parsed.input ?? []).map((file) => path.resolve(file));
  if (inputs.length === 0) throw new Error("At least one --input is required");
  const effectId = positiveInteger(required(parsed, "effect"), "effect");
  const output = path.resolve(required(parsed, "output"));
  const documents = inputs.map((file) => readJson(file, "near-controlled counterfactual proof"));
  const report = buildRollup(documents, inputs.map(fileDescriptor), effectId);
  report.content_sha256 = contentHash(report);
  mkdirSync(path.dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  verifyReport(report);
  console.log(`wrote ${output}`);
}

function buildRollup(documents, descriptors, effectId) {
  const builds = new Set();
  const runs = [];
  const nearExamples = [];
  for (let index = 0; index < documents.length; index += 1) {
    const document = documents[index];
    if (document?.schema_version !== 6 ||
      document?.generated_by !== "rlogs-bpsr-status-effect-counterfactual-proof" ||
      document?.policy?.formula_authority !== false ||
      document?.policy?.runtime_authority !== false ||
      document?.policy?.unresolved_evidence_is_hidden !== false ||
      document?.policy?.near_controlled_diagnostic_authority !== false ||
      !String(document?.policy?.near_controlled_diagnostic ?? "").includes("enumerated") ||
      document?.processing?.measured_peak_within_configured_limit !== true ||
      !document?.processing?.selected_effect_ids?.map(Number).includes(effectId)) {
      throw new Error(`Input ${index} is not a safe schema-6 near-controlled proof`);
    }
    const build = String(document.game_build ?? "");
    if (!/^\d+$/.test(build)) throw new Error(`Input ${index} has no exact numeric build`);
    builds.add(build);
    const exact = (document.effects ?? []).find((entry) =>
      Number(entry.effect_id) === effectId && entry.locus === "target"
    );
    const near = (document.near_controlled_target_diagnostic ?? []).find((entry) =>
      Number(entry.effect_id) === effectId && entry.locus === "target"
    );
    if (!exact || !near || near.formula_authority !== false ||
      near.runtime_authority !== false || near.provider_rdps_credit_allowed !== false) {
      throw new Error(`Input ${index} does not preserve target effect ${effectId}`);
    }
    const examples = (near.variants ?? []).flatMap((variant) => variant.examples ?? []);
    for (const example of examples) {
      validateNearExample(example, effectId, index);
      nearExamples.push(structuredClone(example));
    }
    runs.push({
      proof: descriptors[index],
      source_inputs: structuredClone(document.input?.source_inputs ?? []),
      samples: exactNonnegativeInteger(document.summary?.samples, `input ${index} samples`),
      measured_peak_working_set_mib: Number(document.processing?.measured_peak_working_set_mib),
      exact_controlled_groups: exactNonnegativeInteger(
        exact.exact_recorded_inputs?.controlled_groups,
        `input ${index} exact controlled groups`,
      ),
      exact_divergent_output_groups: exactNonnegativeInteger(
        exact.exact_recorded_inputs?.divergent_output_groups,
        `input ${index} exact divergent groups`,
      ),
      near_controlled_target_pairs: exactNonnegativeInteger(
        near.candidate_absent_near_pairs,
        `input ${index} near pairs`,
      ),
      near_controlled_target_divergent_pairs: exactNonnegativeInteger(
        near.divergent_output_pairs,
        `input ${index} near divergent pairs`,
      ),
      near_controlled_target_equal_pairs: (near.variants ?? []).reduce(
        (sum, variant) => sum + exactNonnegativeInteger(
          variant.equal_output_pairs,
          `input ${index} equal pairs`,
        ),
        0,
      ),
    });
  }
  if (builds.size !== 1) throw new Error("Near-controlled proof inputs span multiple builds");
  const exactDivergentRuns = runs.filter((run) => run.exact_divergent_output_groups > 0).length;
  const nearDivergentPairs = sum(runs, "near_controlled_target_divergent_pairs");
  const equalBundleExamples = nearExamples.filter((example) =>
    example.outputs_equal === true &&
    (example.target_attribute_transitions_excluding_current_hp ?? []).length === 0 &&
    ((example.target_status_present_only_co_transitions ?? []).length +
      (example.target_status_absent_only_co_transitions ?? []).length) > 0
  );
  return {
    schema_version: 1,
    generated_by: "bpsr-status-effect-near-controlled-rollup",
    game_build: [...builds][0],
    effect_id: effectId,
    policy: {
      exact_numeric_effect_id_is_authoritative: true,
      exact_input_build_is_authoritative: true,
      localized_names_are_evidence_only: true,
      cross_session_pairing_allowed: false,
      target_current_hp_relaxation_is_diagnostic_only: true,
      every_target_attribute_and_status_co_transition_is_preserved: true,
      near_controlled_pairs_never_grant_formula_or_runtime_authority: true,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
      unresolved_evidence_is_preserved: true,
    },
    inputs: descriptors,
    summary: {
      matching_capture_runs: runs.length,
      samples: sum(runs, "samples"),
      exact_controlled_groups: sum(runs, "exact_controlled_groups"),
      exact_divergent_output_groups: sum(runs, "exact_divergent_output_groups"),
      exact_divergent_capture_runs: exactDivergentRuns,
      near_controlled_target_pairs: sum(runs, "near_controlled_target_pairs"),
      near_controlled_target_divergent_pairs: nearDivergentPairs,
      near_controlled_target_equal_pairs: sum(runs, "near_controlled_target_equal_pairs"),
      equal_output_status_bundle_examples: equalBundleExamples.length,
    },
    runs,
    near_examples: nearExamples,
    interpretation: {
      independent_divergent_baseline_replication_proven: exactDivergentRuns > 1,
      additional_near_controlled_divergent_replication_observed: nearDivergentPairs > 0,
      equal_output_status_bundle_diagnostic_observed: equalBundleExamples.length > 0,
      equal_output_status_bundle_is_an_isolated_effect_zero_proof: false,
      target_current_hp_is_controlled_in_equal_output_bundle: false,
      candidate_status_is_isolated_in_equal_output_bundle: false,
      exact_transform_proven: false,
      operation_order_and_stacking_proven: false,
      runtime_integer_rounding_proven: false,
      canonical_party_conservation_proven: false,
      formula_authority: false,
      runtime_authority: false,
      ui_authority: false,
      provider_rdps_credit_allowed: false,
      next_required_evidence: [
        "independent exact-build divergent replication at a different baseline with the selected status isolated",
        "operation-stage and stacking-order isolation",
        "boundary-sensitive integer-rounding discrimination",
        "canonical party replay conservation over locally observable exact-build events",
      ],
    },
  };
}

function validateNearExample(example, effectId, index) {
  const presentOnly = example.target_status_present_only_co_transitions;
  const absentOnly = example.target_status_absent_only_co_transitions;
  const attributes = example.target_attribute_transitions_excluding_current_hp;
  if (Number(example.candidate_status?.effect_id) !== effectId ||
    !Array.isArray(presentOnly) || !Array.isArray(absentOnly) || !Array.isArray(attributes) ||
    attributes.some((entry) => Number(entry.attribute_id) === 11310) ||
    Number(example.transition_distance) !== attributes.length + presentOnly.length + absentOnly.length ||
    example.present_formula_context?.normalized_packet_input_sha256 !==
      example.absent_formula_context?.normalized_packet_input_sha256 ||
    JSON.stringify(example.present_formula_context?.source_attributes) !==
      JSON.stringify(example.absent_formula_context?.source_attributes) ||
    JSON.stringify(example.present_formula_context?.source_statuses) !==
      JSON.stringify(example.absent_formula_context?.source_statuses) ||
    JSON.stringify(example.present_formula_context?.status_provider_attributes) !==
      JSON.stringify(example.absent_formula_context?.status_provider_attributes)) {
    throw new Error(`Input ${index} contains an unsafe near-controlled example`);
  }
}

function verifyFile(file) {
  const report = readJson(file, "near-controlled rollup");
  verifyReport(report);
  console.log(`near-controlled rollup verified for build ${report.game_build}, effect ${report.effect_id}`);
}

function verifyReport(report) {
  if (report?.schema_version !== 1 ||
    report?.generated_by !== "bpsr-status-effect-near-controlled-rollup" ||
    !/^\d+$/.test(String(report.game_build ?? "")) ||
    !Number.isSafeInteger(Number(report.effect_id)) || Number(report.effect_id) <= 0 ||
    report.policy?.near_controlled_pairs_never_grant_formula_or_runtime_authority !== true ||
    report.policy?.every_target_attribute_and_status_co_transition_is_preserved !== true ||
    report.policy?.formula_authority !== false || report.policy?.runtime_authority !== false ||
    report.policy?.provider_rdps_credit_allowed !== false ||
    report.interpretation?.exact_transform_proven !== false ||
    report.interpretation?.formula_authority !== false ||
    report.interpretation?.runtime_authority !== false ||
    report.interpretation?.ui_authority !== false ||
    report.interpretation?.provider_rdps_credit_allowed !== false ||
    report.interpretation?.equal_output_status_bundle_is_an_isolated_effect_zero_proof !== false ||
    !Array.isArray(report.inputs) || report.inputs.length === 0 ||
    !Array.isArray(report.runs) || report.runs.length !== report.inputs.length ||
    !Array.isArray(report.near_examples) ||
    contentHash(report) !== report.content_sha256) {
    throw new Error("Near-controlled rollup is invalid or grants unsafe authority");
  }
  const nearPairs = sum(report.runs, "near_controlled_target_pairs");
  const nearDivergent = sum(report.runs, "near_controlled_target_divergent_pairs");
  if (nearPairs !== Number(report.summary?.near_controlled_target_pairs) ||
    nearDivergent !== Number(report.summary?.near_controlled_target_divergent_pairs)) {
    throw new Error("Near-controlled rollup summary does not conserve");
  }
  report.near_examples.forEach((example, index) =>
    validateNearExample(example, Number(report.effect_id), index)
  );
}

function selfTest() {
  const status = { effect_id: 7, source_entity_uuid: 9, stacks: 1, level: 1 };
  const context = {
    normalized_packet_input_sha256: `sha256:${"1".repeat(64)}`,
    source_attributes: [],
    source_statuses: [],
    status_provider_attributes: [],
  };
  const document = {
    schema_version: 6,
    generated_by: "rlogs-bpsr-status-effect-counterfactual-proof",
    game_build: "1",
    policy: {
      formula_authority: false,
      runtime_authority: false,
      unresolved_evidence_is_hidden: false,
      near_controlled_diagnostic_authority: false,
      near_controlled_diagnostic: "co-transitions are enumerated",
    },
    processing: {
      measured_peak_within_configured_limit: true,
      measured_peak_working_set_mib: 1,
      selected_effect_ids: [7],
    },
    input: { source_inputs: ["fixture.rlog"] },
    summary: { samples: 2 },
    effects: [{
      effect_id: 7,
      locus: "target",
      exact_recorded_inputs: { controlled_groups: 0, divergent_output_groups: 0 },
    }],
    near_controlled_target_diagnostic: [{
      effect_id: 7,
      locus: "target",
      candidate_absent_near_pairs: 1,
      divergent_output_pairs: 0,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
      variants: [{
        equal_output_pairs: 1,
        examples: [{
          candidate_status: status,
          outputs_equal: true,
          transition_distance: 1,
          target_attribute_transitions_excluding_current_hp: [],
          target_status_present_only_co_transitions: [{ effect_id: 8 }],
          target_status_absent_only_co_transitions: [],
          present_formula_context: context,
          absent_formula_context: structuredClone(context),
        }],
      }],
    }],
  };
  const report = buildRollup([document], [{ path: "fixture.json", bytes: 1, sha256: "0".repeat(64) }], 7);
  report.content_sha256 = contentHash(report);
  verifyReport(report);
  if (report.summary.equal_output_status_bundle_examples !== 1 ||
    report.interpretation.equal_output_status_bundle_is_an_isolated_effect_zero_proof !== false) {
    throw new Error("near-controlled rollup self-test failed");
  }
  console.log("bpsr-status-effect-near-controlled-rollup self-test passed");
}

function sum(rows, key) {
  return rows.reduce((total, row) => total + Number(row[key] ?? 0), 0);
}

function exactNonnegativeInteger(value, label) {
  const number = Number(value);
  if (!Number.isSafeInteger(number) || number < 0) throw new Error(`${label} is invalid`);
  return number;
}

function fileDescriptor(file) {
  const bytes = readFileSync(file);
  return {
    path: file.replaceAll("\\", "/"),
    bytes: statSync(file).size,
    sha256: createHash("sha256").update(bytes).digest("hex"),
  };
}

function contentHash(value) {
  const copy = structuredClone(value);
  delete copy.content_sha256;
  return createHash("sha256").update(JSON.stringify(copy)).digest("hex");
}

function readJson(file, label) {
  try {
    return JSON.parse(readFileSync(file, "utf8"));
  } catch (error) {
    throw new Error(`Unable to read ${label} ${file}: ${error.message}`);
  }
}

function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index];
    const value = args[index + 1];
    if (!key?.startsWith("--") || value === undefined || value.startsWith("--")) {
      throw new Error(`Invalid argument near ${key ?? "<end>"}`);
    }
    const name = key.slice(2);
    if (name === "input") (parsed.input ??= []).push(value);
    else parsed[name] = value;
  }
  return parsed;
}

function required(parsed, key) {
  if (!parsed[key]) throw new Error(`Missing --${key}`);
  return parsed[key];
}

function positiveInteger(value, label) {
  const number = Number(value);
  if (!Number.isSafeInteger(number) || number <= 0) throw new Error(`--${label} must be positive`);
  return number;
}

function usage(exitCode) {
  console.log("Usage:\n  node tools/bpsr-status-effect-near-controlled-rollup.mjs build --input <schema-6 proof> [--input ...] --effect <exact-id> --output <json>\n  node tools/bpsr-status-effect-near-controlled-rollup.mjs verify --input <json>\n  node tools/bpsr-status-effect-near-controlled-rollup.mjs self-test");
  process.exit(exitCode);
}
