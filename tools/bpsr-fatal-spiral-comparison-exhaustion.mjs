#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 1;
const GENERATED_BY = "tools/bpsr-fatal-spiral-comparison-exhaustion.mjs";
const GAME_BUILD = "24687926";
const EFFECT_ID = 2110125;
const ATTRIBUTE_IDS = [13100, 13101, 13102, 13103, 13104, 13105];
const TOP_ACTION_IDS = [1261, 1262, 1724, 2352, 44701, 55240, 121501, 2031101, 2031102, 2203531];

function fail(message) {
  throw new Error(message);
}

function readJson(file, label) {
  try {
    return JSON.parse(fs.readFileSync(file, "utf8"));
  } catch (error) {
    fail(`Cannot read ${label} ${file}: ${error.message}`);
  }
}

function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function digest(report) {
  const copy = structuredClone(report);
  delete copy.content_sha256;
  return crypto.createHash("sha256").update(canonical(copy)).digest("hex").toUpperCase();
}

async function descriptor(file) {
  const hash = crypto.createHash("sha256");
  let bytes = 0;
  for await (const chunk of fs.createReadStream(file)) {
    bytes += chunk.length;
    hash.update(chunk);
  }
  return {
    path: path.resolve(file).replaceAll("\\", "/"),
    bytes,
    sha256: hash.digest("hex"),
  };
}

function readCohortHeader(file) {
  const marker = Buffer.from('"attribute_states":');
  const stream = fs.openSync(file, "r");
  try {
    let buffered = Buffer.alloc(0);
    const chunk = Buffer.alloc(64 * 1024);
    while (buffered.length <= 4 * 1024 * 1024) {
      const read = fs.readSync(stream, chunk, 0, chunk.length, null);
      if (read === 0) break;
      buffered = Buffer.concat([buffered, chunk.subarray(0, read)]);
      const index = buffered.indexOf(marker);
      if (index >= 0) {
        const prefix = buffered.subarray(0, index).toString("utf8");
        return JSON.parse(`${prefix}"attribute_states":[],"status_states":[],"samples":[]}`);
      }
    }
  } finally {
    fs.closeSync(stream);
  }
  fail(`Formula cohort header exceeds bounded prefix or lacks attribute_states: ${file}`);
}

function sortedNumbers(values) {
  return [...new Set((values ?? []).map(Number))].sort((left, right) => left - right);
}

function labels(values) {
  return (values ?? []).map((value) => path.basename(String(value)).toLowerCase()).sort();
}

function transitionAggregate(proof, field) {
  const effects = proof[field] ?? [];
  const variants = effects.flatMap((effect) => effect.variants ?? []);
  return {
    variants: variants.length,
    candidate_present_groups: variants.reduce(
      (sum, row) => sum + Number(row.candidate_present_groups), 0,
    ),
    candidate_absent_formula_state_pairs: variants.reduce(
      (sum, row) => sum + Number(row.candidate_absent_formula_state_pairs), 0,
    ),
    rejected_without_source_attribute_transition: variants.reduce(
      (sum, row) => sum + Number(row.rejected_without_source_attribute_transition), 0,
    ),
    rejected_with_unselected_source_attribute_transition: variants.reduce(
      (sum, row) => sum + Number(row.rejected_with_unselected_source_attribute_transition), 0,
    ),
    rejected_with_excess_source_status_co_transitions: variants.reduce(
      (sum, row) => sum + Number(row.rejected_with_excess_source_status_co_transitions), 0,
    ),
    rejected_with_excess_target_status_co_transitions: variants.reduce(
      (sum, row) => sum + Number(row.rejected_with_excess_target_status_co_transitions), 0,
    ),
    controlled_pairs: effects.reduce((sum, row) => sum + Number(row.controlled_pairs), 0),
    divergent_output_pairs: effects.reduce(
      (sum, row) => sum + Number(row.divergent_output_pairs), 0,
    ),
    evaluated_candidate_pairs: variants.reduce(
      (sum, row) => sum + Number(
        row.all_element_damage_candidate_projection?.deterministic_pairs ?? 0,
      ), 0,
    ),
  };
}

function validateCounterfactual(proof, cohort, cohortDescriptor, expected) {
  const summary = proof.summary ?? {};
  const exact = transitionAggregate(proof, "cross_entity_source_transition_diagnostic");
  const broad = transitionAggregate(
    proof,
    "cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_diagnostic",
  );
  const projections = (proof.cross_entity_source_transition_diagnostic ?? [])
    .flatMap((effect) => effect.variants ?? [])
    .map((variant) => variant.all_element_damage_candidate_projection)
    .filter(Boolean);
  if (
    proof.schema_version !== 17 ||
    proof.generated_by !== "rlogs-bpsr-status-effect-counterfactual-proof" ||
    proof.game_build !== GAME_BUILD ||
    proof.policy?.formula_authority !== false ||
    proof.policy?.runtime_authority !== false ||
    proof.policy?.all_element_damage_candidate_projection_authority !== false ||
    proof.policy?.structurally_absent_remote_skill_cast_packets_required !== false ||
    Number(proof.processing?.memory_limit_mib) !== 512 ||
    proof.processing?.measured_peak_within_configured_limit !== true ||
    sortedNumbers(proof.processing?.selected_effect_ids).join(",") !== String(EFFECT_ID) ||
    canonical(sortedNumbers(proof.processing?.selected_source_transition_attribute_ids)) !==
      canonical(ATTRIBUTE_IDS) ||
    proof.processing?.cross_entity_formula_state_diagnostic_enabled !== true ||
    Number(summary.samples) !== expected.samples ||
    Number(summary.exact_controlled_groups) !== 0 ||
    Number(summary.relaxed_controlled_groups) !== 0 ||
    Number(summary.cross_entity_formula_state_controlled_groups) !== 0 ||
    Number(summary.cross_entity_source_transition_controlled_pairs) !== 0 ||
    Number(summary.cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_controlled_pairs) !== 0 ||
    exact.candidate_present_groups !== expected.present ||
    exact.candidate_absent_formula_state_pairs !== 0 ||
    exact.controlled_pairs !== 0 ||
    broad.candidate_absent_formula_state_pairs !== expected.broadAbsentPairs ||
    broad.rejected_without_source_attribute_transition !== expected.broadRejectedNoAttribute ||
    broad.rejected_with_unselected_source_attribute_transition !== 0 ||
    broad.rejected_with_excess_source_status_co_transitions !==
      expected.broadRejectedSourceStatus ||
    broad.rejected_with_excess_target_status_co_transitions !== 0 ||
    broad.controlled_pairs !== 0 ||
    broad.divergent_output_pairs !== 0 ||
    broad.evaluated_candidate_pairs !== 0 ||
    projections.length === 0 ||
    projections.some((projection) =>
      projection.effect_id !== EFFECT_ID ||
      projection.current_attribute_id !== 13100 ||
      projection.fixed_point_denominator !== 10000 ||
      projection.deterministic_pairs !== 0 ||
      projection.candidate_selected !== false ||
      projection.formula_authority !== false ||
      projection.runtime_authority !== false ||
      projection.ui_display_authority !== false ||
      projection.provider_rdps_credit_allowed !== false) ||
    Number(proof.input?.bytes) !== cohortDescriptor.bytes ||
    String(proof.input?.sha256 ?? "").replace(/^sha256:/, "").toLowerCase() !==
      cohortDescriptor.sha256 ||
    canonical(labels(proof.input?.source_inputs)) !== canonical(labels(cohort.inputs))
  ) fail(`Counterfactual comparison proof is unsafe or inconsistent: ${expected.label}`);
  return {
    samples: Number(summary.samples),
    exact_present_groups: exact.candidate_present_groups,
    broad_absent_pairs: broad.candidate_absent_formula_state_pairs,
    broad_rejected_without_attribute_transition:
      broad.rejected_without_source_attribute_transition,
    broad_rejected_with_source_status_co_transitions:
      broad.rejected_with_excess_source_status_co_transitions,
    controlled_pairs: broad.controlled_pairs,
    evaluated_candidate_pairs: broad.evaluated_candidate_pairs,
    measured_peak_working_set_mib: Number(proof.processing.measured_peak_working_set_mib),
  };
}

function validateCohortHeader(header, expectedAbilities, expectedRlogs, label) {
  if (
    header.schema_version !== 44 ||
    header.generated_by !== "rlogs-bpsr-state-scaling-damage-proof" ||
    header.game_build !== GAME_BUILD ||
    header.policy?.formula_authority !== false ||
    canonical(sortedNumbers(header.selection?.ability_ids)) !==
      canonical(sortedNumbers(expectedAbilities)) ||
    (header.selection?.selected_effect_ids ?? []).length !== 0 ||
    (header.selection?.source_effect_ids ?? []).length !== 0 ||
    (header.selection?.target_effect_ids ?? []).length !== 0 ||
    canonical(labels(header.inputs)) !== canonical(expectedRlogs)
  ) fail(`Formula comparison cohort header is unsafe or inconsistent: ${label}`);
}

async function build(options) {
  const state = readJson(options.stateScalingProof, "Fatal Spiral state-scaling proof");
  const audit = readJson(options.rlogAudit, "current-build RLOG audit");
  if (
    state.schema_version !== 44 ||
    state.generated_by !== "rlogs-bpsr-state-scaling-damage-proof" ||
    state.game_build !== GAME_BUILD ||
    audit.game_build !== GAME_BUILD ||
    Number(audit.summary?.source_rlog_count) !== 26
  ) fail("Fatal Spiral action inventory or current-build RLOG audit is incompatible");
  const actionCounts = new Map();
  for (const group of state.formula_surface?.groups ?? []) {
    const id = Number(group.ability_id);
    actionCounts.set(id, (actionCounts.get(id) ?? 0) + Number(group.samples));
  }
  const allActions = sortedNumbers([...actionCounts.keys()]);
  const rankedTop = [...actionCounts.entries()]
    .sort((left, right) => right[1] - left[1] || left[0] - right[0])
    .slice(0, 10).map(([id]) => id).sort((left, right) => left - right);
  const remainingActions = allActions.filter((id) => !TOP_ACTION_IDS.includes(id));
  if (
    allActions.length !== 92 ||
    canonical(rankedTop) !== canonical(TOP_ACTION_IDS) ||
    remainingActions.length !== 82
  ) fail("Fatal Spiral action partition does not exactly cover the reviewed action inventory");

  const allRlogs = labels(audit.inputs?.source_rlogs?.map((row) => row.path));
  const activeRlogs = labels(state.inputs);
  if (allRlogs.length !== 26 || activeRlogs.length !== 6 ||
    activeRlogs.some((label) => !allRlogs.includes(label))) {
    fail("Fatal Spiral active RLOGs are not an exact subset of the reviewed current-build cohort");
  }

  const rows = [
    { key: "top10_six", cohort: options.topCohortSix, proof: options.topProofSix,
      abilities: TOP_ACTION_IDS, rlogs: activeRlogs,
      expected: { label: "top10-six", samples: 186821, present: 40661,
        broadAbsentPairs: 8798, broadRejectedNoAttribute: 15,
        broadRejectedSourceStatus: 8783 } },
    { key: "remaining82_six", cohort: options.remainingCohortSix,
      proof: options.remainingProofSix, abilities: remainingActions, rlogs: activeRlogs,
      expected: { label: "remaining82-six", samples: 131781, present: 27449,
        broadAbsentPairs: 3378, broadRejectedNoAttribute: 8,
        broadRejectedSourceStatus: 3370 } },
    { key: "top10_all26", cohort: options.topCohortAll,
      proof: options.topProofAll, abilities: TOP_ACTION_IDS, rlogs: allRlogs,
      expected: { label: "top10-all26", samples: 275647, present: 40661,
        broadAbsentPairs: 8798, broadRejectedNoAttribute: 15,
        broadRejectedSourceStatus: 8783 } },
    { key: "remaining82_all26", cohort: options.remainingCohortAll,
      proof: options.remainingProofAll, abilities: remainingActions, rlogs: allRlogs,
      expected: { label: "remaining82-all26", samples: 212899, present: 27449,
        broadAbsentPairs: 3378, broadRejectedNoAttribute: 8,
        broadRejectedSourceStatus: 3370 } },
  ];
  const inputs = {
    state_scaling_proof: await descriptor(options.stateScalingProof),
    current_build_rlog_audit: await descriptor(options.rlogAudit),
  };
  const results = {};
  for (const row of rows) {
    const cohortDescriptor = await descriptor(row.cohort);
    const proofDescriptor = await descriptor(row.proof);
    const header = readCohortHeader(row.cohort);
    validateCohortHeader(header, row.abilities, row.rlogs, row.key);
    const proof = readJson(row.proof, `${row.key} counterfactual proof`);
    results[row.key] = validateCounterfactual(
      proof, header, cohortDescriptor, row.expected,
    );
    inputs[`${row.key}_cohort`] = cohortDescriptor;
    inputs[`${row.key}_counterfactual`] = proofDescriptor;
  }

  const sixSamples = results.top10_six.samples + results.remaining82_six.samples;
  const allSamples = results.top10_all26.samples + results.remaining82_all26.samples;
  const sixPresent = results.top10_six.exact_present_groups +
    results.remaining82_six.exact_present_groups;
  const allPresent = results.top10_all26.exact_present_groups +
    results.remaining82_all26.exact_present_groups;
  const sixBroadAbsent = results.top10_six.broad_absent_pairs +
    results.remaining82_six.broad_absent_pairs;
  const allBroadAbsent = results.top10_all26.broad_absent_pairs +
    results.remaining82_all26.broad_absent_pairs;
  if (sixSamples !== 318602 || allSamples !== 488546 || sixPresent !== 68110 ||
    allPresent !== sixPresent || sixBroadAbsent !== 12176 || allBroadAbsent !== sixBroadAbsent) {
    fail("Combined Fatal Spiral comparison totals are inconsistent");
  }

  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game: "blue-protocol-star-resonance",
    game_build: GAME_BUILD,
    identity: {
      effect_id: EFFECT_ID,
      all_element_attribute_ids: ATTRIBUTE_IDS,
      observed_action_ids: allActions,
      high_volume_action_ids: TOP_ACTION_IDS,
      remaining_action_ids: remainingActions,
    },
    topology: {
      effect_edge: "provider -> effect/status lifecycle -> recipient or enemy target",
      damage_edge: "recipient damage action -> recipient or enemy target",
      source_side_join: "effect endpoint equals damage actor",
      target_side_join: "effect endpoint equals damage target",
    },
    policy: {
      remote_player_cast_packets_required: false,
      packet_absence_is_zero: false,
      current_snapshots_may_rewrite_historical_runs: false,
      unrelated_status_transitions_are_ignored: false,
      comparison_compatibility_is_formula_authority: false,
      provider_rdps_credit_allowed: false,
    },
    inputs,
    coverage: {
      reviewed_current_build_rlogs: 26,
      effect_observed_rlogs: 6,
      observed_action_ids: 92,
      action_partition_sizes: [10, 82],
      effect_observed_rlog_samples: sixSamples,
      all_reviewed_rlog_samples: allSamples,
      additional_absent_search_samples: allSamples - sixSamples,
      exact_effect_present_groups: allPresent,
      broad_diagnostic_absent_pairs: allBroadAbsent,
      controlled_pairs: 0,
      evaluated_integer_candidate_pairs: 0,
      maximum_counterfactual_working_set_mib: Math.max(
        ...Object.values(results).map((row) => row.measured_peak_working_set_mib),
      ),
      partitions: results,
    },
    proof_closure: {
      exact_numeric_action_inventory_partitioned_without_omission: true,
      active_and_absent_damage_states_retained: true,
      all_reviewed_current_build_rlogs_searched: true,
      additional_twenty_rlogs_added_new_structural_absent_candidates: false,
      retained_current_build_capture_frontier_exhausted: true,
      current_controlled_pairs_available: false,
      automatic_integer_candidate_evaluator_exercised_on_real_pair: false,
      exact_operation_order_proven: false,
      exact_integer_rounding_proven: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
      observed_damage_reassigned_to_provider: 0,
    },
    next_acquisition: {
      authoritative_server_damage_operator_or_new_controlled_capture_required: true,
      required_capture_contract:
        "same-build effect-present/effect-absent damage actions with complete invariant packet, actor, target, non-candidate source/target status, provider, and attribute state; unrelated transitions and missing values reject the pair",
    },
    content_sha256: "",
  };
  report.content_sha256 = digest(report);
  verify(report);
  return report;
}

function verify(report) {
  if (
    report.schema_version !== SCHEMA_VERSION || report.generated_by !== GENERATED_BY ||
    report.game_build !== GAME_BUILD || Number(report.identity?.effect_id) !== EFFECT_ID ||
    Number(report.coverage?.reviewed_current_build_rlogs) !== 26 ||
    Number(report.coverage?.effect_observed_rlogs) !== 6 ||
    Number(report.coverage?.observed_action_ids) !== 92 ||
    Number(report.coverage?.effect_observed_rlog_samples) !== 318602 ||
    Number(report.coverage?.all_reviewed_rlog_samples) !== 488546 ||
    Number(report.coverage?.additional_absent_search_samples) !== 169944 ||
    Number(report.coverage?.exact_effect_present_groups) !== 68110 ||
    Number(report.coverage?.broad_diagnostic_absent_pairs) !== 12176 ||
    Number(report.coverage?.controlled_pairs) !== 0 ||
    Number(report.coverage?.evaluated_integer_candidate_pairs) !== 0 ||
    report.proof_closure?.exact_numeric_action_inventory_partitioned_without_omission !== true ||
    report.proof_closure?.active_and_absent_damage_states_retained !== true ||
    report.proof_closure?.all_reviewed_current_build_rlogs_searched !== true ||
    report.proof_closure?.additional_twenty_rlogs_added_new_structural_absent_candidates !== false ||
    report.proof_closure?.retained_current_build_capture_frontier_exhausted !== true ||
    report.proof_closure?.current_controlled_pairs_available !== false ||
    report.proof_closure?.exact_operation_order_proven !== false ||
    report.proof_closure?.exact_integer_rounding_proven !== false ||
    report.proof_closure?.formula_authority !== false ||
    report.proof_closure?.runtime_authority !== false ||
    report.proof_closure?.ui_display_authority !== false ||
    report.proof_closure?.provider_rdps_credit_allowed !== false ||
    Number(report.proof_closure?.observed_damage_reassigned_to_provider) !== 0 ||
    report.content_sha256 !== digest(report)
  ) fail("Fatal Spiral comparison exhaustion receipt is unsafe or invalid");
}

function parse(argv) {
  const args = {};
  for (let index = 0; index < argv.length; index += 2) {
    const flag = argv[index];
    const value = argv[index + 1];
    if (!flag?.startsWith("--") || value == null) fail(`Invalid argument ${flag ?? "<missing>"}`);
    args[flag.slice(2)] = value;
  }
  return args;
}

function required(args, name) {
  if (!args[name]) fail(`Missing --${name}`);
  return path.resolve(args[name]);
}

function selfTest() {
  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game_build: GAME_BUILD,
    identity: { effect_id: EFFECT_ID },
    coverage: {
      reviewed_current_build_rlogs: 26, effect_observed_rlogs: 6,
      observed_action_ids: 92, effect_observed_rlog_samples: 318602,
      all_reviewed_rlog_samples: 488546, additional_absent_search_samples: 169944,
      exact_effect_present_groups: 68110, broad_diagnostic_absent_pairs: 12176,
      controlled_pairs: 0, evaluated_integer_candidate_pairs: 0,
    },
    proof_closure: {
      exact_numeric_action_inventory_partitioned_without_omission: true,
      active_and_absent_damage_states_retained: true,
      all_reviewed_current_build_rlogs_searched: true,
      additional_twenty_rlogs_added_new_structural_absent_candidates: false,
      retained_current_build_capture_frontier_exhausted: true,
      current_controlled_pairs_available: false,
      exact_operation_order_proven: false, exact_integer_rounding_proven: false,
      formula_authority: false, runtime_authority: false, ui_display_authority: false,
      provider_rdps_credit_allowed: false, observed_damage_reassigned_to_provider: 0,
    },
    content_sha256: "",
  };
  report.content_sha256 = digest(report);
  verify(report);
  report.coverage.controlled_pairs = 1;
  try { verify(report); fail("self-test accepted a controlled-pair mismatch"); }
  catch (error) { if (error.message === "self-test accepted a controlled-pair mismatch") throw error; }
  console.log("bpsr-fatal-spiral-comparison-exhaustion self-test passed");
}

const [command = "help", ...argv] = process.argv.slice(2);
try {
  if (command === "self-test") selfTest();
  else if (command === "verify") {
    const args = parse(argv);
    verify(readJson(required(args, "input"), "comparison exhaustion receipt"));
    console.log("Fatal Spiral comparison exhaustion receipt verified");
  } else if (command === "build") {
    const args = parse(argv);
    const output = required(args, "output");
    if (fs.existsSync(output)) fail(`Refusing to overwrite ${output}`);
    const report = await build({
      stateScalingProof: required(args, "state-scaling-proof"),
      rlogAudit: required(args, "rlog-audit"),
      topCohortSix: required(args, "top-cohort-six"),
      topProofSix: required(args, "top-proof-six"),
      remainingCohortSix: required(args, "remaining-cohort-six"),
      remainingProofSix: required(args, "remaining-proof-six"),
      topCohortAll: required(args, "top-cohort-all"),
      topProofAll: required(args, "top-proof-all"),
      remainingCohortAll: required(args, "remaining-cohort-all"),
      remainingProofAll: required(args, "remaining-proof-all"),
    });
    fs.writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
    console.log(JSON.stringify({ output, coverage: report.coverage }, null, 2));
  } else {
    console.log("Usage: node tools/bpsr-fatal-spiral-comparison-exhaustion.mjs build <inputs> --output <json> | verify --input <json> | self-test");
    process.exitCode = command === "help" ? 0 : 1;
  }
} catch (error) {
  console.error(error.stack ?? String(error));
  process.exitCode = 1;
}
