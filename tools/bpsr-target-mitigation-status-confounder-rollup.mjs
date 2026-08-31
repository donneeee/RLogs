#!/usr/bin/env node

import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

const GENERATOR = "tools/bpsr-target-mitigation-status-confounder-rollup.mjs";
const EFFECT_IDS = [55301, 823226, 2110093, 2201452];
const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);
if (command === "build") build(options);
else if (command === "verify") verifyCommand(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function build(parsed) {
  const buildId = numericString(required(parsed, "build"));
  const rollupPath = path.resolve(required(parsed, "target-mitigation-rollup"));
  const proofPaths = requiredMany(parsed, "proof").map((value) => path.resolve(value));
  const output = path.resolve(required(parsed, "output"));
  const targetRollup = readJson(rollupPath, "expanded target mitigation rollup");
  validateTargetRollup(targetRollup, buildId);
  const aggregates = new Map();
  const sourceRlogs = new Set();
  let samples = 0;
  for (const proofPath of proofPaths) {
    const proof = readJson(proofPath, "status confounder counterfactual proof");
    validateProof(proof, buildId);
    samples += Number(proof.summary.samples);
    for (const source of proof.input.source_inputs) {
      const key = path.basename(String(source)).toLowerCase();
      if (sourceRlogs.has(key)) throw new Error(`duplicate source RLOG ${key}`);
      sourceRlogs.add(key);
    }
    for (const effect of proof.effects) {
      const key = `${Number(effect.effect_id)}:${String(effect.locus)}`;
      const row = aggregates.get(key) ?? emptyAggregate(effect.effect_id, effect.locus);
      row.capture_proofs_with_observation += 1;
      row.observed_status_states += Number(effect.observation.observed_status_states);
      row.observed_samples += Number(effect.observation.observed_samples);
      addMode(row.exact_recorded_inputs, effect.exact_recorded_inputs);
      addMode(row.target_current_hp_excluded_diagnostic,
        effect.target_current_hp_excluded_diagnostic);
      aggregates.set(key, row);
    }
  }
  if (proofPaths.length !== Number(targetRollup.summary.matching_build_capture_proofs) ||
    sourceRlogs.size !== Number(targetRollup.summary.matching_build_source_rlogs) ||
    samples !== Number(targetRollup.summary.damage_samples)) {
    throw new Error("status confounder proofs do not exactly cover the expanded target mitigation rollup");
  }
  const expectedKeys = EFFECT_IDS.flatMap((effectId) =>
    ["source", "target"].map((locus) => `${effectId}:${locus}`));
  for (const key of expectedKeys) if (!aggregates.has(key)) aggregates.set(key,
    emptyAggregate(...key.split(":").map((value, index) => index === 0 ? Number(value) : value)));
  const effects = [...aggregates.values()].sort((left, right) =>
    left.effect_id - right.effect_id || left.locus.localeCompare(right.locus));
  const targetRows = effects.filter((row) => row.locus === "target");
  const report = {
    schema_version: 1,
    generated_by: GENERATOR,
    game_build: buildId,
    status: "all-observed-target-status-confounders-await-controlled-pairs",
    policy: {
      exact_numeric_effect_ids_and_build_are_authoritative: true,
      every_capture_is_analyzed_independently: true,
      cross_capture_pairing_allowed: false,
      current_hp_excluded_comparisons_are_diagnostic_only: true,
      absent_controlled_pairs_are_not_zero_effect_proof: true,
      unresolved_evidence_is_preserved: true,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
    },
    inputs: {
      target_mitigation_rollup: fileDescriptor(rollupPath),
      counterfactual_proofs: proofPaths.map(fileDescriptor),
    },
    evidence_scope: {
      matching_build_capture_proofs: proofPaths.length,
      matching_build_source_rlogs: [...sourceRlogs].sort(),
      damage_samples: samples,
      configured_memory_limit_mib_per_capture: 128,
      measured_peak_working_set_bytes: null,
      selected_effect_ids: EFFECT_IDS,
    },
    effects,
    target_locus_summary: {
      observed_samples: sum(targetRows, (row) => row.observed_samples),
      exact_controlled_groups: sum(targetRows, (row) => row.exact_recorded_inputs.controlled_groups),
      exact_divergent_output_groups:
        sum(targetRows, (row) => row.exact_recorded_inputs.divergent_output_groups),
      current_hp_excluded_controlled_groups:
        sum(targetRows, (row) => row.target_current_hp_excluded_diagnostic.controlled_groups),
      every_selected_effect_observed_at_target_locus:
        targetRows.every((row) => row.observed_samples > 0),
      every_selected_effect_exactly_controlled_at_target_locus:
        targetRows.every((row) => row.exact_recorded_inputs.controlled_groups > 0),
    },
    acquisition_contract: {
      target: "capture exact target-locus present and absent states for each common physical-defense near-pair confounder",
      effect_ids: EFFECT_IDS,
      required_controls: [
        "same capture, session, run, source, direct source, target, ability, passive, hit identity, damage flags, and normalized packet formula inputs",
        "complete source and target attributes, every non-candidate source and target status, and every status-provider at-event attribute state",
        "one exact status record removed with the otherwise-identical absent state observed",
        "deterministic output on both sides",
      ],
      success_condition: "each common status delta is independently controlled or the 22000 physical-defense pair is reproduced with no unrelated target status delta",
    },
    authority: {
      common_status_confounders_eliminated: false,
      exact_target_mitigation_formula_proven: false,
      exact_operation_order_and_integer_rounding_proven: false,
      packet_conservation_proven: false,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
    },
  };
  report.content_sha256 = contentHash(report);
  verifyReport(report);
  mkdirSync(path.dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  const written = readJson(output, "written status confounder rollup");
  verifyReport(written);
  verifyInputs(written);
  console.log(JSON.stringify({ evidence_scope: report.evidence_scope,
    target_locus_summary: report.target_locus_summary }, null, 2));
}

function validateTargetRollup(value, build) {
  if (value?.schema_version !== 1 ||
    value?.generated_by !== "tools/bpsr-target-mitigation-proof-rollup.mjs" ||
    String(value?.game_build) !== build || value?.content_sha256 !== orderedContentHash(value) ||
    Number(value?.summary?.controlled_groups) !== 0 ||
    value?.summary?.exact_target_mitigation_formula_proven !== false ||
    value?.policy?.cross_capture_pairing_allowed !== false) {
    throw new Error("expanded target mitigation rollup is not exact-build fail-closed evidence");
  }
}

function validateProof(value, build) {
  if (![4, 5].includes(Number(value?.schema_version)) ||
    value?.generated_by !== "rlogs-bpsr-status-effect-counterfactual-proof" ||
    String(value?.game_build) !== build ||
    JSON.stringify(value?.processing?.selected_effect_ids) !== JSON.stringify(EFFECT_IDS) ||
    Number(value?.processing?.memory_limit_mib) !== 128 ||
    value?.policy?.runtime_authority !== false || value?.policy?.formula_authority !== false ||
    value?.policy?.unresolved_evidence_is_hidden !== false ||
    !Array.isArray(value?.input?.source_inputs) || value.input.source_inputs.length === 0 ||
    !/^sha256:[0-9a-f]{64}$/.test(String(value?.input?.sha256 ?? "")) ||
    !Array.isArray(value?.effects)) {
    throw new Error("status counterfactual proof is not exact-build schema-4 fail-closed evidence");
  }
  if (Number(value.schema_version) >= 5 &&
    value?.policy?.candidate_projection_authority !== false) {
    throw new Error("status counterfactual proof has unsafe candidate projection authority");
  }
}

function emptyAggregate(effectId, locus) {
  return { effect_id: Number(effectId), locus: String(locus), capture_proofs_with_observation: 0,
    observed_status_states: 0, observed_samples: 0,
    exact_recorded_inputs: emptyMode(), target_current_hp_excluded_diagnostic: emptyMode(),
    confounder_eliminated: false, formula_authority: false };
}
function emptyMode() { return { present_groups: 0, present_samples: 0,
  absent_status_state_unobserved_groups: 0, absent_identity_group_unobserved_groups: 0,
  controlled_groups: 0, sample_comparisons: 0, deterministic_groups: 0,
  equal_output_groups: 0, divergent_output_groups: 0, nondeterministic_groups: 0 }; }
function addMode(target, source) { for (const key of Object.keys(target)) target[key] += Number(source[key]); }

function verifyCommand(parsed) { const input = path.resolve(required(parsed, "input")); const report = readJson(input, "status confounder rollup"); verifyReport(report); verifyInputs(report); console.log(`verified ${input}`); }
function verifyReport(report) {
  const targets = (report?.effects ?? []).filter((row) => row.locus === "target");
  if (report?.schema_version !== 1 || report?.generated_by !== GENERATOR ||
    report?.content_sha256 !== contentHash(report) ||
    report?.status !== "all-observed-target-status-confounders-await-controlled-pairs" ||
    report?.policy?.cross_capture_pairing_allowed !== false ||
    report?.policy?.absent_controlled_pairs_are_not_zero_effect_proof !== true ||
    report?.policy?.formula_authority !== false || report?.policy?.runtime_authority !== false ||
    Number(report?.evidence_scope?.matching_build_capture_proofs) !== 24 ||
    Number(report?.evidence_scope?.matching_build_source_rlogs?.length) !== 26 ||
    Number(report?.evidence_scope?.damage_samples) !== 735016 ||
    targets.length !== 4 || !targets.every((row) => row.observed_samples > 0 &&
      row.exact_recorded_inputs.controlled_groups === 0 && row.confounder_eliminated === false) ||
    Number(report?.target_locus_summary?.exact_controlled_groups) !== 0 ||
    report?.target_locus_summary?.every_selected_effect_observed_at_target_locus !== true ||
    report?.target_locus_summary?.every_selected_effect_exactly_controlled_at_target_locus !== false ||
    report?.authority?.common_status_confounders_eliminated !== false ||
    report?.authority?.exact_target_mitigation_formula_proven !== false ||
    report?.authority?.formula_authority !== false || report?.authority?.runtime_authority !== false ||
    report?.authority?.provider_rdps_credit_allowed !== false) {
    throw new Error("status confounder rollup violates its fail-closed schema");
  }
  validateDescriptor(report.inputs?.target_mitigation_rollup);
  if (!Array.isArray(report.inputs?.counterfactual_proofs) ||
    report.inputs.counterfactual_proofs.length !== 24) throw new Error("status confounder rollup input count changed");
  for (const descriptor of report.inputs.counterfactual_proofs) validateDescriptor(descriptor);
}
function verifyInputs(report) { for (const descriptor of [report.inputs.target_mitigation_rollup, ...report.inputs.counterfactual_proofs]) { const bytes = readFileSync(path.resolve(descriptor.path)); if (bytes.length !== Number(descriptor.bytes) || createHash("sha256").update(bytes).digest("hex") !== descriptor.sha256) throw new Error(`input changed: ${descriptor.path}`); } }
function selfTest() { const row = emptyAggregate(1, "target"); addMode(row.exact_recorded_inputs, { present_groups: 1, present_samples: 1, absent_status_state_unobserved_groups: 1, absent_identity_group_unobserved_groups: 0, controlled_groups: 0, sample_comparisons: 0, deterministic_groups: 0, equal_output_groups: 0, divergent_output_groups: 0, nondeterministic_groups: 0 }); if (row.exact_recorded_inputs.present_groups !== 1 || row.confounder_eliminated !== false) throw new Error("rollup self-test failed"); console.log("bpsr-target-mitigation-status-confounder-rollup self-test passed"); }
function fileDescriptor(file) { const bytes = readFileSync(file); return { path: file.replaceAll("\\", "/"), bytes: statSync(file).size, sha256: createHash("sha256").update(bytes).digest("hex") }; }
function validateDescriptor(value) { if (!String(value?.path ?? "") || !Number.isSafeInteger(Number(value?.bytes)) || Number(value.bytes) <= 0 || !/^[0-9a-f]{64}$/.test(String(value?.sha256 ?? ""))) throw new Error("invalid exact file descriptor"); }
function orderedContentHash(value) { const copy = structuredClone(value); delete copy.content_sha256; return createHash("sha256").update(JSON.stringify(copy)).digest("hex"); }
function contentHash(value) { const copy = structuredClone(value); delete copy.content_sha256; return createHash("sha256").update(stableStringify(copy)).digest("hex"); }
function stableStringify(value) { if (value === null || typeof value !== "object") return JSON.stringify(value); if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`; return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`; }
function readJson(file, label) { try { return JSON.parse(readFileSync(file, "utf8")); } catch (error) { throw new Error(`unable to read ${label} ${file}: ${error.message}`); } }
function sum(values, select) { return values.reduce((total, value) => total + Number(select(value)), 0); }
function numericString(value) { if (!/^\d+$/.test(String(value))) throw new Error("build must be numeric"); return String(value); }
function parseArgs(args) { const parsed = {}; for (let index = 0; index < args.length; index += 2) { const key = args[index]?.replace(/^--/, ""), value = args[index + 1]; if (!key || value === undefined) throw new Error(`invalid argument near ${args[index]}`); if (key === "proof") (parsed[key] ??= []).push(value); else parsed[key] = value; } return parsed; }
function required(parsed, key) { if (!parsed[key]) throw new Error(`missing --${key}`); return parsed[key]; }
function requiredMany(parsed, key) { if (!Array.isArray(parsed[key]) || parsed[key].length === 0) throw new Error(`missing --${key}`); return parsed[key]; }
function usage(code) { console.log("Usage:\n  node tools/bpsr-target-mitigation-status-confounder-rollup.mjs build --build <id> --target-mitigation-rollup <json> --proof <json> ... --output <json>\n  node tools/bpsr-target-mitigation-status-confounder-rollup.mjs verify --input <json>\n  node tools/bpsr-target-mitigation-status-confounder-rollup.mjs self-test"); process.exit(code); }
