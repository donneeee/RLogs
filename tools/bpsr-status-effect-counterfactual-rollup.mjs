import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { readFile, stat, writeFile } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { pathToFileURL } from "node:url";

const SCHEMA_VERSION = 1;
const GENERATOR = "reviewed-status-effect-counterfactual-rollup";
const PROOF_GENERATOR = "rlogs-bpsr-status-effect-counterfactual-proof";
const REQUIRED_PROOF_SCHEMA = 5;

function fail(message) {
  throw new Error(message);
}

function parseArgs(argv) {
  const args = { proofs: [] };
  for (let index = 0; index < argv.length; index += 2) {
    const flag = argv[index];
    const value = argv[index + 1];
    if (!value) fail(`missing value for ${flag}`);
    if (flag === "--base") args.base = value;
    else if (flag === "--proof") args.proofs.push(value);
    else if (flag === "--output") args.output = value;
    else fail(`unknown argument ${flag}`);
  }
  if (!args.base || !args.output || args.proofs.length === 0) {
    fail("usage: node tools/bpsr-status-effect-counterfactual-rollup.mjs --base <reviewed-rollup.json> --proof <schema-5-proof.json> [--proof ...] --output <rollup.json>");
  }
  return args;
}

async function readJson(file) {
  return JSON.parse(await readFile(file, "utf8"));
}

async function sha256(file) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(file)) hash.update(chunk);
  return `sha256:${hash.digest("hex")}`;
}

async function descriptor(file) {
  const info = await stat(file);
  return {
    path: file.replaceAll("\\", "/"),
    bytes: info.size,
    sha256: await sha256(file),
  };
}

function count(value, label) {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed < 0) fail(`${label} must be a non-negative safe integer`);
  return parsed;
}

function validateMode(mode, label) {
  if (!mode || typeof mode !== "object") fail(`${label} is missing`);
  return {
    present_groups: count(mode.present_groups, `${label}.present_groups`),
    absent_status_state_unobserved_groups: count(
      mode.absent_status_state_unobserved_groups,
      `${label}.absent_status_state_unobserved_groups`,
    ),
    absent_identity_group_unobserved_groups: count(
      mode.absent_identity_group_unobserved_groups,
      `${label}.absent_identity_group_unobserved_groups`,
    ),
    controlled_groups: count(mode.controlled_groups, `${label}.controlled_groups`),
    sample_comparisons: count(mode.sample_comparisons, `${label}.sample_comparisons`),
    divergent_output_groups: count(mode.divergent_output_groups, `${label}.divergent_output_groups`),
  };
}

function addMode(target, source) {
  for (const key of Object.keys(target)) target[key] += source[key];
}

function validateCandidate(candidate, label) {
  if (!candidate || typeof candidate !== "object") fail(`${label} is missing`);
  if (candidate.candidate_selected !== false || candidate.formula_authority !== false ||
      candidate.runtime_authority !== false || candidate.ui_display_authority !== false ||
      candidate.provider_rdps_credit_allowed !== false ||
      candidate.exact_damage_projection_proven !== false ||
      candidate.exact_operation_order_proven !== false ||
      candidate.exact_integer_rounding_proven !== false) {
    fail(`${label} grants unsupported authority`);
  }
  const variants = new Map();
  for (const row of candidate.variants ?? []) {
    if (!["floor", "ceil", "round-half-up"].includes(row.rounding) || variants.has(row.rounding)) {
      fail(`${label} has an invalid rounding variant`);
    }
    variants.set(row.rounding, {
      rounding: row.rounding,
      compatible_groups: count(row.compatible_groups, `${label}.${row.rounding}.compatible_groups`),
      rejected_groups: count(row.rejected_groups, `${label}.${row.rounding}.rejected_groups`),
    });
  }
  if (variants.size !== 3) fail(`${label} must contain all three rounding variants`);
  return {
    controlled_divergent_groups: count(candidate.controlled_divergent_groups, `${label}.controlled_divergent_groups`),
    groups_with_target_physical_defense: count(candidate.groups_with_target_physical_defense, `${label}.groups_with_target_physical_defense`),
    groups_missing_target_physical_defense: count(candidate.groups_missing_target_physical_defense, `${label}.groups_missing_target_physical_defense`),
    groups_with_invalid_nonnegative_inputs: count(candidate.groups_with_invalid_nonnegative_inputs, `${label}.groups_with_invalid_nonnegative_inputs`),
    variants: [...variants.values()],
  };
}

function addCandidate(target, source) {
  for (const key of [
    "controlled_divergent_groups",
    "groups_with_target_physical_defense",
    "groups_missing_target_physical_defense",
    "groups_with_invalid_nonnegative_inputs",
  ]) target[key] += source[key];
  for (const sourceVariant of source.variants) {
    const targetVariant = target.variants.find((row) => row.rounding === sourceVariant.rounding);
    targetVariant.compatible_groups += sourceVariant.compatible_groups;
    targetVariant.rejected_groups += sourceVariant.rejected_groups;
  }
}

function runNameFromRlog(rlog) {
  return path.basename(rlog, ".rlog");
}

async function buildRollup(base, baseFile, proofFiles) {
  if (base?.schema_version !== SCHEMA_VERSION || base?.generated_by !== GENERATOR ||
      base?.policy?.cross_session_pairing_allowed !== false ||
      base?.policy?.unresolved_evidence_is_preserved !== true ||
      !Array.isArray(base.runs) || base.runs.length === 0) {
    fail("base rollup is not reviewed fail-closed schema-1 evidence");
  }
  const build = String(base.game_build);
  const effectId = count(base.effect_id, "base.effect_id");
  const baseRuns = new Map(base.runs.map((run) => [String(run.run), run]));
  if (baseRuns.size !== base.runs.length) fail("base rollup contains duplicate runs");

  const locusTotals = new Map(["source", "target"].map((locus) => [locus, {
    locus,
    observed_samples: 0,
    exact: validateMode({
      present_groups: 0,
      absent_status_state_unobserved_groups: 0,
      absent_identity_group_unobserved_groups: 0,
      controlled_groups: 0,
      sample_comparisons: 0,
      divergent_output_groups: 0,
    }, `${locus}.zero`),
    target_current_hp_excluded_diagnostic: validateMode({
      present_groups: 0,
      absent_status_state_unobserved_groups: 0,
      absent_identity_group_unobserved_groups: 0,
      controlled_groups: 0,
      sample_comparisons: 0,
      divergent_output_groups: 0,
    }, `${locus}.zero-diagnostic`),
  }]));
  const candidateTotal = {
    effect_id: effectId,
    locus: "target",
    hypothesis: "650-basis-point raw physical-defense reduction before the 22000/(22000+defense) curve",
    controlled_divergent_groups: 0,
    groups_with_target_physical_defense: 0,
    groups_missing_target_physical_defense: 0,
    groups_with_invalid_nonnegative_inputs: 0,
    variants: ["floor", "ceil", "round-half-up"].map((rounding) => ({
      rounding,
      compatible_groups: 0,
      rejected_groups: 0,
    })),
    candidate_selected: false,
    exact_damage_projection_proven: false,
    exact_operation_order_proven: false,
    exact_integer_rounding_proven: false,
    formula_authority: false,
    runtime_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
  };
  const seenRuns = new Set();
  const runs = [];
  let samples = 0;
  let maximumPeakBytes = 0;
  let maximumPeakMib = 0;

  for (const proofFile of proofFiles) {
    const proof = await readJson(proofFile);
    if (proof?.schema_version !== REQUIRED_PROOF_SCHEMA || proof?.generated_by !== PROOF_GENERATOR ||
        String(proof?.game_build) !== build || proof?.policy?.candidate_projection_authority !== false ||
        proof?.policy?.runtime_authority !== false || proof?.policy?.formula_authority !== false ||
        proof?.processing?.measured_peak_within_configured_limit !== true ||
        JSON.stringify(proof?.processing?.selected_effect_ids) !== JSON.stringify([effectId])) {
      fail(`${proofFile} is not a bounded fail-closed schema-${REQUIRED_PROOF_SCHEMA} proof for effect ${effectId}`);
    }
    const sourceInputs = proof?.input?.source_inputs;
    if (!Array.isArray(sourceInputs) || sourceInputs.length !== 1) fail(`${proofFile} must bind exactly one source RLOG`);
    const runName = runNameFromRlog(sourceInputs[0]);
    const baseRun = baseRuns.get(runName);
    if (!baseRun || seenRuns.has(runName)) fail(`${proofFile} does not map uniquely to the reviewed capture cohort`);
    seenRuns.add(runName);

    const effects = proof.effects ?? [];
    for (const locus of ["source", "target"]) {
      const matches = effects.filter((row) => row.locus === locus && Number(row.effect_id) === effectId);
      if (matches.length !== 1) fail(`${proofFile} must contain one ${locus}:${effectId} row`);
      const row = matches[0];
      const total = locusTotals.get(locus);
      total.observed_samples += count(row?.observation?.observed_samples, `${proofFile}.${locus}.observed_samples`);
      const exact = validateMode(row.exact_recorded_inputs, `${proofFile}.${locus}.exact`);
      const relaxed = validateMode(row.target_current_hp_excluded_diagnostic, `${proofFile}.${locus}.relaxed`);
      addMode(total.exact, exact);
      addMode(total.target_current_hp_excluded_diagnostic, relaxed);
      if (locus === "source" && (row.exact_recorded_inputs.blade_sweep_candidate_projection !== null ||
          row.target_current_hp_excluded_diagnostic.blade_sweep_candidate_projection !== null)) {
        fail(`${proofFile} source-locus candidate projection must be null`);
      }
      if (locus === "target") {
        addCandidate(candidateTotal, validateCandidate(
          row.exact_recorded_inputs.blade_sweep_candidate_projection,
          `${proofFile}.target.exact.candidate`,
        ));
        validateCandidate(
          row.target_current_hp_excluded_diagnostic.blade_sweep_candidate_projection,
          `${proofFile}.target.relaxed.candidate`,
        );
      }
    }

    const cohortFile = String(proof.input.path).replaceAll("/", path.sep);
    const actualCohort = await descriptor(cohortFile);
    if (actualCohort.bytes !== count(proof.input.bytes, `${proofFile}.input.bytes`) ||
        actualCohort.sha256 !== proof.input.sha256) fail(`${proofFile} cohort descriptor does not match disk`);
    const rlogFile = String(baseRun.rlog.path).replaceAll("/", path.sep);
    const actualRlog = await descriptor(rlogFile);
    if (actualRlog.bytes !== count(baseRun.rlog.bytes, `${runName}.rlog.bytes`) ||
        actualRlog.sha256 !== baseRun.rlog.sha256) fail(`${runName} RLOG no longer matches the reviewed cohort`);
    samples += count(proof.summary.samples, `${proofFile}.summary.samples`);
    maximumPeakBytes = Math.max(maximumPeakBytes, count(proof.processing.measured_peak_working_set_bytes, `${proofFile}.peak_bytes`));
    maximumPeakMib = Math.max(maximumPeakMib, Number(proof.processing.measured_peak_working_set_mib));
    runs.push({
      run: runName,
      rlog: actualRlog,
      cohort: { ...actualCohort, samples: count(proof.summary.samples, `${proofFile}.samples`), schema_version: 40 },
      proof: await descriptor(proofFile),
      measured_peak_working_set_bytes: count(proof.processing.measured_peak_working_set_bytes, `${proofFile}.peak_bytes`),
      measured_peak_working_set_mib: Number(proof.processing.measured_peak_working_set_mib),
      measured_peak_within_configured_limit: true,
    });
  }
  if (seenRuns.size !== baseRuns.size) fail("proof set does not exactly cover the reviewed capture runs");
  runs.sort((left, right) => left.run.localeCompare(right.run));
  const source = locusTotals.get("source");
  const target = locusTotals.get("target");
  const exactControlled = source.exact.controlled_groups + target.exact.controlled_groups;
  const exactComparisons = source.exact.sample_comparisons + target.exact.sample_comparisons;
  const exactDivergent = source.exact.divergent_output_groups + target.exact.divergent_output_groups;
  const relaxedControlled = source.target_current_hp_excluded_diagnostic.controlled_groups + target.target_current_hp_excluded_diagnostic.controlled_groups;
  const relaxedComparisons = source.target_current_hp_excluded_diagnostic.sample_comparisons + target.target_current_hp_excluded_diagnostic.sample_comparisons;
  const relaxedDivergent = source.target_current_hp_excluded_diagnostic.divergent_output_groups + target.target_current_hp_excluded_diagnostic.divergent_output_groups;

  return {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATOR,
    game_build: build,
    effect_id: effectId,
    policy: {
      exact_numeric_effect_id_is_authoritative: true,
      localized_names_are_evidence_only: true,
      cross_session_pairing_allowed: false,
      current_hp_relaxation_is_diagnostic_only: true,
      schema_40_status_provider_attribute_context_required: true,
      bounded_memory_measurement_required_for_every_input: true,
      candidate_projection_is_diagnostic_only: true,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      unresolved_evidence_is_preserved: true,
      provider_rdps_credit_allowed: false,
    },
    supersedes: await descriptor(baseFile),
    capture_locator: base.capture_locator,
    processing: {
      proof_schema_version: REQUIRED_PROOF_SCHEMA,
      formula_cohort_schema_version: 40,
      maximum_measured_peak_working_set_bytes: maximumPeakBytes,
      maximum_measured_peak_working_set_mib: maximumPeakMib,
      every_input_within_configured_memory_limit: true,
    },
    summary: {
      matching_capture_runs: runs.length,
      formula_damage_samples: samples,
      source_locus_observed_samples: source.observed_samples,
      target_locus_observed_samples: target.observed_samples,
      exact_controlled_groups: exactControlled,
      exact_sample_comparisons: exactComparisons,
      exact_divergent_output_groups: exactDivergent,
      relaxed_controlled_groups: relaxedControlled,
      relaxed_sample_comparisons: relaxedComparisons,
      relaxed_divergent_output_groups: relaxedDivergent,
    },
    loci: [source, target],
    blade_sweep_candidate_projection: candidateTotal,
    runs,
    status: exactControlled === 0
      ? "matching-build-provider-context-counterfactual-unproven"
      : "matching-build-provider-context-counterfactual-observed-awaiting-review",
    blockers: [
      ...(exactControlled === 0 ? ["no same-session exact one-status-removed comparison was observed"] : []),
      ...(relaxedControlled === 0 ? ["target CurrentHP exclusion did not produce a controlled comparison"] : []),
      "exact mitigation or vulnerability projection remains unproven",
      "stacking, operation order, and integer rounding remain unproven",
      "canonical party conservation replay remains unproven",
    ],
    next_proof_action: "Acquire a matching-build same-session comparison with identical damage identity, packet inputs, source and target attributes, every status-provider attribute state, and every non-candidate status, observing both candidate-present and candidate-absent target states.",
  };
}

export { addCandidate, addMode, buildRollup, validateCandidate, validateMode };

function selfTest() {
  const candidate = {
    controlled_divergent_groups: 1,
    groups_with_target_physical_defense: 1,
    groups_missing_target_physical_defense: 0,
    groups_with_invalid_nonnegative_inputs: 0,
    variants: ["floor", "ceil", "round-half-up"].map((rounding) => ({
      rounding,
      compatible_groups: rounding === "ceil" ? 0 : 1,
      rejected_groups: rounding === "ceil" ? 1 : 0,
    })),
    candidate_selected: false,
    exact_damage_projection_proven: false,
    exact_operation_order_proven: false,
    exact_integer_rounding_proven: false,
    formula_authority: false,
    runtime_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
  };
  validateCandidate(candidate, "fixture");
  const unsafe = structuredClone(candidate);
  unsafe.ui_display_authority = true;
  try {
    validateCandidate(unsafe, "unsafe fixture");
  } catch {
    process.stdout.write("bpsr-status-effect-counterfactual-rollup self-test passed\n");
    return;
  }
  fail("self-test accepted unsupported UI authority");
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  if (process.argv[2] === "self-test") {
    selfTest();
  } else {
    const args = parseArgs(process.argv.slice(2));
    const base = await readJson(args.base);
    const rollup = await buildRollup(base, args.base, args.proofs);
    await writeFile(args.output, `${JSON.stringify(rollup, null, 2)}\n`, "utf8");
    process.stdout.write(`wrote ${args.output}\n`);
  }
}
