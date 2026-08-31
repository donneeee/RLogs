#!/usr/bin/env node

import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

const GENERATOR = "tools/bpsr-target-mitigation-near-pair-candidate-proof.mjs";
const DIAGNOSTIC_GENERATOR =
  "rlogs-bpsr-target-mitigation-transform-proof:target-status-relaxed-diagnostic";
const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);
if (command === "analyze") analyze(options);
else if (command === "verify") verifyCommand(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function analyze(parsed) {
  const build = numericString(required(parsed, "build"), "build");
  const diagnosticPath = path.resolve(required(parsed, "diagnostic"));
  const sameAxisDiagnosticPaths = requiredMany(parsed, "same-axis-diagnostic")
    .map((value) => path.resolve(value));
  const offlinePath = path.resolve(required(parsed, "offline-exhaustion-proof"));
  const confounderRollupPath = path.resolve(required(parsed, "status-confounder-rollup"));
  const output = path.resolve(required(parsed, "output"));
  const diagnostic = readJson(diagnosticPath, "target-status-relaxed diagnostic");
  const offline = readJson(offlinePath, "offline exhaustion proof");
  const confounderRollup = readJson(confounderRollupPath, "status confounder rollup");
  validateDiagnostic(diagnostic, build);
  validateOffline(offline, build);
  validateConfounderRollup(confounderRollup, build);
  const sameAxisStatusEvidence = buildSameAxisStatusEvidence(sameAxisDiagnosticPaths, build);
  const transformedConstant = BigInt(
    offline.current_build_client_candidates.character_sheet_transforms.DefPara.parameters[0],
  );
  const simpleConstant = BigInt(
    offline.current_build_client_candidates.attack_simply.AttackSimplyDefParam,
  );
  const examples = diagnostic.axes.physical_defense.near_pair_examples.map((example) => {
    const transformed = sharedBaseInterval(example, transformedConstant);
    const simple = sharedBaseInterval(example, simpleConstant);
    const leftIds = example.left_only_statuses.map((row) => Number(row.effect_id));
    const rightIds = example.right_only_statuses.map((row) => Number(row.effect_id));
    if (!leftIds.includes(2201452) || leftIds.length + rightIds.length <= 1 || transformed === null || simple !== null) {
      throw new Error("physical-defense near pair no longer has the expected exact candidate compatibility and confounding state");
    }
    return {
      session_id: String(example.session_id),
      run_ordinal: Number(example.run_ordinal),
      source_entity_uuid: String(example.source_entity_uuid),
      target_entity_uuid: String(example.target_entity_uuid),
      ability_id: Number(example.ability_id),
      left_raw_physical_defense: Number(example.left_raw),
      right_raw_physical_defense: Number(example.right_raw),
      left_amount: Number(example.left_outcome.amount),
      right_amount: Number(example.right_outcome.amount),
      transformed_curve_shared_base_interval: transformed,
      runtime_simple_curve_shared_base_interval: simple,
      left_sequences: structuredClone(example.left_sequences),
      right_sequences: structuredClone(example.right_sequences),
      left_only_statuses: structuredClone(example.left_only_statuses),
      right_only_statuses: structuredClone(example.right_only_statuses),
      status_state_is_confounded: true,
      formula_authority: false,
    };
  });
  const uniqueOutcomeSignatures = [...new Set(examples.map((row) => JSON.stringify([
    row.left_raw_physical_defense, row.right_raw_physical_defense, row.left_amount, row.right_amount,
  ])))];
  const candidateStatusEffectIds = [...new Set(examples.flatMap((row) =>
    [...row.left_only_statuses, ...row.right_only_statuses]
      .map((status) => Number(status.effect_id))))]
    .sort((left, right) => left - right);
  const witnessedEffects = new Set(
    sameAxisStatusEvidence.status_effect_witnesses.map((row) => Number(row.effect_id)),
  );
  sameAxisStatusEvidence.candidate_status_effect_ids_without_same_axis_witness =
    candidateStatusEffectIds.filter((effectId) => !witnessedEffects.has(effectId));
  sameAxisStatusEvidence.candidate_near_pair_remains_confounded = true;
  const report = {
    schema_version: 3,
    generated_by: GENERATOR,
    game_build: build,
    model_id: "target-physical-armor-counterfactual",
    status: "exact-integer-candidate-compatible-status-confounded",
    policy: {
      exact_numeric_ids_build_and_integer_arithmetic_are_authoritative: true,
      target_status_relaxation_is_diagnostic_only: true,
      candidate_compatibility_is_not_formula_proof: true,
      candidate_rejection_is_not_operation_order_proof: true,
      same_axis_equal_outcomes_are_local_invariance_evidence_not_global_zero_effect_proof: true,
      same_axis_divergent_outcomes_preserve_status_confounders: true,
      localized_names_are_evidence_only: true,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
    },
    inputs: {
      diagnostic: fileDescriptor(diagnosticPath),
      same_axis_diagnostics: sameAxisDiagnosticPaths.map(fileDescriptor),
      offline_exhaustion_proof: fileDescriptor(offlinePath),
      status_confounder_rollup: fileDescriptor(confounderRollupPath),
    },
    exact_candidate_evaluation: {
      model: "floor(nonnegative_base * constant / (constant + target_physical_defense_raw))",
      transformed_curve_constant: Number(transformedConstant),
      runtime_simple_curve_constant: Number(simpleConstant),
      packet_near_pair_rows: examples.length,
      unique_raw_and_outcome_signatures: uniqueOutcomeSignatures.length,
      transformed_curve_compatible_rows: examples.filter((row) => row.transformed_curve_shared_base_interval).length,
      transformed_curve_unique_shared_base_values: [...new Set(examples.flatMap((row) =>
        row.transformed_curve_shared_base_interval?.minimum === row.transformed_curve_shared_base_interval?.maximum
          ? [row.transformed_curve_shared_base_interval.minimum]
          : []))],
      runtime_simple_curve_compatible_rows: examples.filter((row) => row.runtime_simple_curve_shared_base_interval).length,
      exact_target_mitigation_formula_proven: false,
    },
    packet_near_pairs: examples,
    confounders: {
      selected_blade_sweep_effect_2110092_in_status_delta: false,
      exact_status_state_equal: false,
      status_delta_row_count_range: {
        minimum: Math.min(...examples.map((row) => row.left_only_statuses.length + row.right_only_statuses.length)),
        maximum: Math.max(...examples.map((row) => row.left_only_statuses.length + row.right_only_statuses.length)),
      },
      status_effect_ids: candidateStatusEffectIds,
      effect_2201452_present_on_higher_defense_side_in_every_row: true,
      effect_2201452_damage_stage_exclusivity_proven: false,
      same_axis_status_invariance: sameAxisStatusEvidence,
      counterfactual_exhaustion: {
        matching_build_capture_proofs:
          Number(confounderRollup.evidence_scope.matching_build_capture_proofs),
        matching_build_source_rlogs:
          Number(confounderRollup.evidence_scope.matching_build_source_rlogs.length),
        damage_samples: Number(confounderRollup.evidence_scope.damage_samples),
        target_locus_observed_samples:
          Number(confounderRollup.target_locus_summary.observed_samples),
        exact_target_locus_controlled_groups:
          Number(confounderRollup.target_locus_summary.exact_controlled_groups),
        every_common_confounder_observed_at_target_locus: true,
        every_common_confounder_exactly_controlled_at_target_locus: false,
        common_status_confounders_eliminated: false,
      },
    },
    acquisition_contract: {
      target: "repeat the physical-defense transition while holding the complete target status state fixed except for one proven defense-only mechanism",
      preferred_observed_identity: {
        ability_id: examples[0].ability_id,
        source_entity_uuid: examples[0].source_entity_uuid,
        target_entity_uuid: examples[0].target_entity_uuid,
        higher_raw_physical_defense: examples[0].left_raw_physical_defense,
        lower_raw_physical_defense: examples[0].right_raw_physical_defense,
      },
      required_closure: [
        "isolate the 5907 to 5370 physical-defense transition without unrelated target status changes",
        "observe one deterministic outcome per side under exact calculation, source, and target state controls",
        "test the 22000 transformed curve against every controlled pair and reject it on any incompatibility",
        "prove damage-stage ordering and integer rounding, then conserve canonical replay totals",
      ],
    },
    authority: {
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
  const written = readJson(output, "written near-pair candidate proof");
  verifyReport(written);
  verifyInputs(written);
  console.log(JSON.stringify(report.exact_candidate_evaluation, null, 2));
}

function validateDiagnostic(value, build) {
  const physical = value?.axes?.physical_defense;
  if (![2, 3].includes(Number(value?.schema_version)) ||
    value?.generated_by !== DIAGNOSTIC_GENERATOR ||
    String(value?.game_build) !== build || Number(value?.selected_effect_id) !== 2110092 ||
    value?.policy?.only_target_status_state_is_relaxed !== true ||
    value?.policy?.near_pair_is_not_controlled_counterfactual_proof !== true ||
    value?.policy?.formula_authority !== false || value?.policy?.runtime_authority !== false ||
    Number(physical?.counters?.distinct_axis_pairs) !== 3 ||
    Number(physical?.counters?.deterministic_pairs) !== 3 ||
    Number(physical?.counters?.divergent_output_pairs) !== 3 ||
    Number(physical?.counters?.pairs_with_selected_effect_in_status_delta) !== 0 ||
    !Array.isArray(physical?.near_pair_examples) || physical.near_pair_examples.length !== 3 ||
    Object.entries(value.axes).some(([axis, row]) => axis !== "physical_defense" &&
      Number(row.counters?.distinct_axis_pairs) !== 0)) {
    throw new Error("target-status-relaxed diagnostic is not the exact three-row current-build frontier");
  }
  if (Number(value.schema_version) === 3 &&
    (value?.policy?.same_axis_status_variants_are_audited_for_exact_outcome_invariance !== true ||
      value?.policy?.same_axis_equal_outcomes_are_local_invariance_evidence_not_global_zero_effect_proof !== true)) {
    throw new Error("schema-3 diagnostic does not preserve the same-axis status audit policy");
  }
}

function validateOffline(value, build) {
  if (![3, 4].includes(Number(value?.schema_version)) ||
    value?.generated_by !== "tools/target-mitigation-offline-exhaustion-proof.mjs" ||
    String(value?.game_build) !== build || value?.content_sha256 !== orderedContentHash(value) ||
    Number(value?.current_build_client_candidates?.character_sheet_transforms
      ?.DefPara?.parameters?.[0]) !== 22000 ||
    Number(value?.current_build_client_candidates?.attack_simply?.AttackSimplyDefParam) !== 6500 ||
    Number(value?.summary?.promoted_combat_formulas) !== 0) {
    throw new Error("offline exhaustion proof does not bind the expected current-build candidates");
  }
  if (Number(value.schema_version) === 4 &&
    (value?.policy?.mitigation_action_targets_are_allegiance_neutral !== true ||
      value?.policy?.current_actor_snapshots_are_never_substituted !== true ||
      value?.policy?.exact_client_combat_damage_consumer_proven !== false ||
      Number(value?.summary?.neutral_player_targets) !== 774 ||
      Number(value?.summary?.actor_scene_controlled_axis_pairs) !== 0)) {
    throw new Error("offline exhaustion proof omits the neutral action topology receipt");
  }
}

function validateConfounderRollup(value, build) {
  if (value?.schema_version !== 1 ||
    value?.generated_by !== "tools/bpsr-target-mitigation-status-confounder-rollup.mjs" ||
    String(value?.game_build) !== build || value?.content_sha256 !== stableContentHash(value) ||
    Number(value?.evidence_scope?.matching_build_capture_proofs) !== 24 ||
    Number(value?.evidence_scope?.matching_build_source_rlogs?.length) !== 26 ||
    Number(value?.evidence_scope?.damage_samples) !== 735016 ||
    Number(value?.target_locus_summary?.observed_samples) !== 3009 ||
    Number(value?.target_locus_summary?.exact_controlled_groups) !== 0 ||
    value?.target_locus_summary?.every_selected_effect_observed_at_target_locus !== true ||
    value?.target_locus_summary?.every_selected_effect_exactly_controlled_at_target_locus !== false ||
    value?.authority?.common_status_confounders_eliminated !== false ||
    value?.authority?.formula_authority !== false || value?.authority?.runtime_authority !== false) {
    throw new Error("status confounder rollup is not the complete fail-closed 24-proof frontier");
  }
}

function buildSameAxisStatusEvidence(files, build) {
  if (files.length !== 24 || new Set(files).size !== files.length) {
    throw new Error("same-axis status evidence must bind exactly 24 unique current-build diagnostics");
  }
  const counters = {
    same_axis_status_pairs: 0,
    same_axis_deterministic_pairs: 0,
    same_axis_equal_output_pairs: 0,
    same_axis_divergent_output_pairs: 0,
    same_axis_nondeterministic_pairs: 0,
  };
  const sourceInputs = new Set();
  const examples = [];
  let damageSamples = 0;
  let maxPeakWorkingSetBytes = 0;
  const memoryLimits = new Set();
  for (const file of files) {
    const value = readJson(file, "same-axis status diagnostic");
    const physical = value?.axes?.physical_defense;
    if (Number(value?.schema_version) !== 3 || value?.generated_by !== DIAGNOSTIC_GENERATOR ||
      String(value?.game_build) !== build || Number(value?.selected_effect_id) !== 2110092 ||
      value?.policy?.same_capture_only !== true || value?.policy?.cross_capture_pairing_allowed !== false ||
      value?.policy?.only_target_status_state_is_relaxed !== true ||
      value?.policy?.complete_target_status_row_deltas_are_preserved !== true ||
      value?.policy?.same_axis_status_variants_are_audited_for_exact_outcome_invariance !== true ||
      value?.policy?.same_axis_equal_outcomes_are_local_invariance_evidence_not_global_zero_effect_proof !== true ||
      value?.policy?.formula_authority !== false || value?.policy?.runtime_authority !== false ||
      value?.policy?.provider_rdps_credit_allowed !== false ||
      value?.processing?.measured_peak_within_configured_limit !== true ||
      !Number.isSafeInteger(Number(value?.processing?.sample_count)) ||
      !Number.isSafeInteger(Number(value?.processing?.measured_peak_working_set_bytes)) ||
      Number(value?.processing?.measured_peak_working_set_bytes) <= 0 ||
      Number(value?.processing?.memory_limit_mib) < 64 ||
      Number(value?.processing?.measured_peak_working_set_bytes) >
        Number(value?.processing?.memory_limit_mib) * 1024 * 1024 ||
      !String(value?.input?.sha256 ?? "").startsWith("sha256:") ||
      !Array.isArray(value?.input?.source_inputs) || value.input.source_inputs.length === 0 ||
      !Array.isArray(physical?.same_axis_status_examples) ||
      physical.same_axis_status_examples.length !== Number(physical?.counters?.same_axis_status_pairs)) {
      throw new Error(`same-axis diagnostic is not a bounded fail-closed schema-3 artifact: ${file}`);
    }
    for (const source of value.input.source_inputs) sourceInputs.add(path.basename(String(source)));
    damageSamples += Number(value.processing.sample_count);
    maxPeakWorkingSetBytes = Math.max(
      maxPeakWorkingSetBytes,
      Number(value.processing.measured_peak_working_set_bytes),
    );
    memoryLimits.add(Number(value.processing.memory_limit_mib));
    for (const key of Object.keys(counters)) counters[key] += Number(physical.counters?.[key] ?? 0);
    examples.push(...physical.same_axis_status_examples.map((example) => structuredClone(example)));
  }
  if (sourceInputs.size !== 26 || damageSamples !== 735016 ||
    counters.same_axis_status_pairs !== 5 || counters.same_axis_deterministic_pairs !== 5 ||
    counters.same_axis_equal_output_pairs !== 4 ||
    counters.same_axis_divergent_output_pairs !== 1 ||
    counters.same_axis_nondeterministic_pairs !== 0 || examples.length !== 5) {
    throw new Error("same-axis status evidence does not match the complete 24-diagnostic frontier");
  }
  const witnessMap = new Map();
  for (const example of examples) {
    const equalOutcome = Number(example?.left_outcome?.amount) === Number(example?.right_outcome?.amount) &&
      Number(example?.left_outcome?.normal_value) === Number(example?.right_outcome?.normal_value);
    const deltaRows = [...example.left_only_statuses, ...example.right_only_statuses];
    const effectIds = [...new Set(deltaRows.map((row) => Number(row.effect_id)))];
    for (const effectId of effectIds) {
      const witness = witnessMap.get(effectId) ?? {
        effect_id: effectId,
        pair_participations: 0,
        equal_outcome_pair_participations: 0,
        divergent_outcome_pair_participations: 0,
        single_effect_equal_outcome_pair_participations: 0,
        single_effect_divergent_outcome_pair_participations: 0,
      };
      witness.pair_participations += 1;
      if (equalOutcome) witness.equal_outcome_pair_participations += 1;
      else witness.divergent_outcome_pair_participations += 1;
      if (deltaRows.length === 1 && equalOutcome) {
        witness.single_effect_equal_outcome_pair_participations += 1;
      } else if (deltaRows.length === 1) {
        witness.single_effect_divergent_outcome_pair_participations += 1;
      }
      witnessMap.set(effectId, witness);
    }
  }
  const statusEffectWitnesses = [...witnessMap.values()]
    .sort((left, right) => left.effect_id - right.effect_id);
  const singleEffectEqual = statusEffectWitnesses
    .filter((row) => row.single_effect_equal_outcome_pair_participations > 0)
    .map((row) => row.effect_id);
  const divergentJoint = statusEffectWitnesses
    .filter((row) => row.divergent_outcome_pair_participations > 0)
    .map((row) => row.effect_id);
  if (JSON.stringify(singleEffectEqual) !== JSON.stringify([2203182]) ||
    JSON.stringify(divergentJoint) !== JSON.stringify([823226, 2110093])) {
    throw new Error("same-axis status witness classification changed");
  }
  return {
    matching_build_capture_diagnostics: files.length,
    matching_build_source_rlogs: sourceInputs.size,
    damage_samples: damageSamples,
    configured_memory_limit_mib_values: [...memoryLimits].sort((left, right) => left - right),
    maximum_measured_peak_working_set_bytes: maxPeakWorkingSetBytes,
    physical_defense_same_axis_status_pairs: counters.same_axis_status_pairs,
    physical_defense_same_axis_deterministic_pairs: counters.same_axis_deterministic_pairs,
    physical_defense_same_axis_equal_output_pairs: counters.same_axis_equal_output_pairs,
    physical_defense_same_axis_divergent_output_pairs: counters.same_axis_divergent_output_pairs,
    physical_defense_same_axis_nondeterministic_pairs: counters.same_axis_nondeterministic_pairs,
    status_effect_witnesses: statusEffectWitnesses,
    single_effect_equal_outcome_effect_ids: singleEffectEqual,
    effects_in_divergent_joint_status_delta: divergentJoint,
    exact_examples: examples,
    target_status_can_change_damage_outside_raw_defense: true,
    formula_authority: false,
    runtime_authority: false,
    provider_rdps_credit_allowed: false,
  };
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

function verifyCommand(parsed) {
  const input = path.resolve(required(parsed, "input"));
  const report = readJson(input, "near-pair candidate proof");
  verifyReport(report);
  verifyInputs(report);
  console.log(`verified ${input}`);
}
function verifyReport(report) {
  const sameAxis = report?.confounders?.same_axis_status_invariance;
  if (report?.schema_version !== 3 || report?.generated_by !== GENERATOR ||
    report?.content_sha256 !== contentHash(report) ||
    report?.status !== "exact-integer-candidate-compatible-status-confounded" ||
    report?.policy?.target_status_relaxation_is_diagnostic_only !== true ||
    report?.policy?.candidate_compatibility_is_not_formula_proof !== true ||
    report?.policy?.same_axis_equal_outcomes_are_local_invariance_evidence_not_global_zero_effect_proof !== true ||
    report?.policy?.same_axis_divergent_outcomes_preserve_status_confounders !== true ||
    report?.policy?.formula_authority !== false || report?.policy?.runtime_authority !== false ||
    report?.policy?.provider_rdps_credit_allowed !== false ||
    Number(report?.exact_candidate_evaluation?.transformed_curve_constant) !== 22000 ||
    Number(report?.exact_candidate_evaluation?.runtime_simple_curve_constant) !== 6500 ||
    Number(report?.exact_candidate_evaluation?.packet_near_pair_rows) !== 3 ||
    Number(report?.exact_candidate_evaluation?.transformed_curve_compatible_rows) !== 3 ||
    Number(report?.exact_candidate_evaluation?.runtime_simple_curve_compatible_rows) !== 0 ||
    JSON.stringify(report?.exact_candidate_evaluation?.transformed_curve_unique_shared_base_values) !==
      JSON.stringify(["107006"]) ||
    report?.exact_candidate_evaluation?.exact_target_mitigation_formula_proven !== false ||
    report?.confounders?.selected_blade_sweep_effect_2110092_in_status_delta !== false ||
    report?.confounders?.exact_status_state_equal !== false ||
    report?.confounders?.effect_2201452_damage_stage_exclusivity_proven !== false ||
    Number(sameAxis?.matching_build_capture_diagnostics) !== 24 ||
    Number(sameAxis?.matching_build_source_rlogs) !== 26 ||
    Number(sameAxis?.damage_samples) !== 735016 ||
    JSON.stringify(sameAxis?.configured_memory_limit_mib_values) !== JSON.stringify([192]) ||
    Number(sameAxis?.maximum_measured_peak_working_set_bytes) !== 135503872 ||
    Number(sameAxis?.physical_defense_same_axis_status_pairs) !== 5 ||
    Number(sameAxis?.physical_defense_same_axis_deterministic_pairs) !== 5 ||
    Number(sameAxis?.physical_defense_same_axis_equal_output_pairs) !== 4 ||
    Number(sameAxis?.physical_defense_same_axis_divergent_output_pairs) !== 1 ||
    Number(sameAxis?.physical_defense_same_axis_nondeterministic_pairs) !== 0 ||
    JSON.stringify(sameAxis?.single_effect_equal_outcome_effect_ids) !== JSON.stringify([2203182]) ||
    JSON.stringify(sameAxis?.effects_in_divergent_joint_status_delta) !== JSON.stringify([823226, 2110093]) ||
    JSON.stringify(sameAxis?.candidate_status_effect_ids_without_same_axis_witness) !==
      JSON.stringify([55301, 2201452]) ||
    sameAxis?.candidate_near_pair_remains_confounded !== true ||
    sameAxis?.target_status_can_change_damage_outside_raw_defense !== true ||
    sameAxis?.formula_authority !== false || sameAxis?.runtime_authority !== false ||
    sameAxis?.provider_rdps_credit_allowed !== false ||
    !Array.isArray(sameAxis?.exact_examples) || sameAxis.exact_examples.length !== 5 ||
    Number(report?.confounders?.counterfactual_exhaustion?.matching_build_capture_proofs) !== 24 ||
    Number(report?.confounders?.counterfactual_exhaustion?.matching_build_source_rlogs) !== 26 ||
    Number(report?.confounders?.counterfactual_exhaustion?.damage_samples) !== 735016 ||
    Number(report?.confounders?.counterfactual_exhaustion?.target_locus_observed_samples) !== 3009 ||
    Number(report?.confounders?.counterfactual_exhaustion
      ?.exact_target_locus_controlled_groups) !== 0 ||
    report?.confounders?.counterfactual_exhaustion
      ?.every_common_confounder_observed_at_target_locus !== true ||
    report?.confounders?.counterfactual_exhaustion
      ?.every_common_confounder_exactly_controlled_at_target_locus !== false ||
    report?.confounders?.counterfactual_exhaustion?.common_status_confounders_eliminated !== false ||
    report?.authority?.exact_target_mitigation_formula_proven !== false ||
    report?.authority?.formula_authority !== false || report?.authority?.runtime_authority !== false ||
    report?.authority?.provider_rdps_credit_allowed !== false ||
    !Array.isArray(report?.packet_near_pairs) || report.packet_near_pairs.length !== 3) {
    throw new Error("near-pair candidate proof violates its fail-closed schema");
  }
  validateDescriptor(report.inputs?.diagnostic);
  if (!Array.isArray(report.inputs?.same_axis_diagnostics) ||
    report.inputs.same_axis_diagnostics.length !== 24) {
    throw new Error("near-pair candidate proof must bind 24 same-axis diagnostics");
  }
  for (const descriptor of report.inputs.same_axis_diagnostics) validateDescriptor(descriptor);
  validateDescriptor(report.inputs?.offline_exhaustion_proof);
  validateDescriptor(report.inputs?.status_confounder_rollup);
}
function verifyInputs(report) { for (const descriptor of Object.values(report.inputs).flatMap((value) => Array.isArray(value) ? value : [value])) { const bytes = readFileSync(path.resolve(descriptor.path)); if (bytes.length !== Number(descriptor.bytes) || createHash("sha256").update(bytes).digest("hex") !== descriptor.sha256) throw new Error(`input changed: ${descriptor.path}`); } }
function selfTest() { const compatible = sharedBaseInterval({ left_outcome: { amount: 84356 }, left_raw: 5907, right_outcome: { amount: 86011 }, right_raw: 5370 }, 22000n); const rejected = sharedBaseInterval({ left_outcome: { amount: 84356 }, left_raw: 5907, right_outcome: { amount: 86011 }, right_raw: 5370 }, 6500n); if (JSON.stringify(compatible) !== JSON.stringify({ minimum: "107006", maximum: "107006" }) || rejected !== null) throw new Error("integer candidate self-test failed"); console.log("bpsr-target-mitigation-near-pair-candidate-proof self-test passed"); }
function fileDescriptor(file) { const bytes = readFileSync(file); return { path: file.replaceAll("\\", "/"), bytes: statSync(file).size, sha256: createHash("sha256").update(bytes).digest("hex") }; }
function validateDescriptor(value) { if (!String(value?.path ?? "") || !Number.isSafeInteger(Number(value?.bytes)) || Number(value.bytes) <= 0 || !/^[0-9a-f]{64}$/.test(String(value?.sha256 ?? ""))) throw new Error("invalid exact file descriptor"); }
function orderedContentHash(value) { const copy = structuredClone(value); delete copy.content_sha256; return createHash("sha256").update(JSON.stringify(copy)).digest("hex"); }
function stableContentHash(value) { const copy = structuredClone(value); delete copy.content_sha256; return createHash("sha256").update(stableStringify(copy)).digest("hex"); }
function contentHash(value) { const copy = structuredClone(value); delete copy.content_sha256; return createHash("sha256").update(stableStringify(copy)).digest("hex"); }
function stableStringify(value) { if (value === null || typeof value !== "object") return JSON.stringify(value); if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`; return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`; }
function readJson(file, label) { try { return JSON.parse(readFileSync(file, "utf8")); } catch (error) { throw new Error(`unable to read ${label} ${file}: ${error.message}`); } }
function numericString(value, label) { if (!/^\d+$/.test(String(value))) throw new Error(`${label} must be numeric`); return String(value); }
function parseArgs(args) { const parsed = {}; for (let index = 0; index < args.length; index += 2) { const key = args[index]?.replace(/^--/, ""); const value = args[index + 1]; if (!key || value === undefined) throw new Error(`invalid argument near ${args[index]}`); if (key === "same-axis-diagnostic") { (parsed[key] ??= []).push(value); } else { parsed[key] = value; } } return parsed; }
function required(parsed, key) { if (!parsed[key]) throw new Error(`missing --${key}`); return parsed[key]; }
function requiredMany(parsed, key) { if (!Array.isArray(parsed[key]) || parsed[key].length === 0) throw new Error(`missing --${key}`); return parsed[key]; }
function usage(code) { console.log("Usage:\n  node tools/bpsr-target-mitigation-near-pair-candidate-proof.mjs analyze --build <id> --diagnostic <json> --same-axis-diagnostic <json> [--same-axis-diagnostic <json> ...] --offline-exhaustion-proof <json> --status-confounder-rollup <json> --output <json>\n  node tools/bpsr-target-mitigation-near-pair-candidate-proof.mjs verify --input <json>\n  node tools/bpsr-target-mitigation-near-pair-candidate-proof.mjs self-test"); process.exit(code); }
