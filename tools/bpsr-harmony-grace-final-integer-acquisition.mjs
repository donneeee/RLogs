#!/usr/bin/env node

import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 2;
const SUPPORTED_SCHEMA_VERSIONS = new Set([1, 2]);
const GENERATED_BY = "tools/bpsr-harmony-grace-final-integer-acquisition.mjs";
const GAME_BUILD = "24687926";
const EFFECT_ID = 3_003_052;
const PRIORITY_ABILITY_ID = 2_352;
const CURRENT_HP_ATTRIBUTE_ID = 11_310;
const EFFECT_FAMILY_ATTRIBUTE_IDS = new Set([
  11_030,
  11_031,
  11_034,
  11_330,
  11_331,
  11_332,
]);
const CANDIDATE_FINAL_BOUNDARIES = ["floor", "ceil", "nearest_half_up"];
const MINIMUM_REPEATS_PER_PHASE = 2;
const MINIMUM_DISTINCT_STAGE_SIGNATURES = 2;
const MAX_EXAMPLES = 20;

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "generate") generate(options);
else if (command === "verify") verify(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function generate(values) {
  const rssSamples = [];
  const sampleRss = (stage) => rssSamples.push({ stage, rss_bytes: process.memoryUsage().rss });
  sampleRss("start");
  const boundaryPath = path.resolve(required(values, "boundary"));
  const tracePath = path.resolve(required(values, "trace"));
  const cohortPath = path.resolve(required(values, "cohort"));
  const transitionProofPath = path.resolve(required(values, "transition-proof"));
  const counterfactualPath = values.get("counterfactual")
    ? path.resolve(values.get("counterfactual"))
    : null;
  const outputPath = path.resolve(required(values, "output"));
  refuseExisting(outputPath);

  const boundary = readJson(boundaryPath);
  const trace = readJson(tracePath);
  const cohort = readJson(cohortPath);
  const transitionProof = readJson(transitionProofPath);
  const counterfactual = counterfactualPath ? readJson(counterfactualPath) : null;
  sampleRss("inputs-loaded");
  const closedRunEligible = validateInputs(
    boundary,
    trace,
    cohort,
    transitionProof,
    counterfactual,
  );

  const analysis = analyze(trace, cohort, transitionProof);
  if (!closedRunEligible) {
    analysis.aba.selected_final_server_integer_boundary = null;
    analysis.aba.exact_final_server_integer_counterfactual_proven = false;
    analysis.aba.incomplete_capture_rejected_as_formula_authority = true;
  }
  sampleRss("analysis-complete");
  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game_build: GAME_BUILD,
    effect_id: EFFECT_ID,
    identity: {
      deployment_id: "global",
      protocol_pack_digest: boundary.identity.protocol_pack_digest,
      session_id: trace.session_id,
      priority_ability_id: PRIORITY_ABILITY_ID,
      priority_action_classification: "base-skill",
    },
    inputs: {
      current_pack_closed_run_boundary: fileReceipt(boundaryPath),
      exact_single_effect_trace: fileReceipt(tracePath),
      priority_ability_formula_cohort: fileReceipt(cohortPath),
      lifecycle_transition_proof: fileReceipt(transitionProofPath),
      ...(counterfactualPath
        ? { generic_counterfactual_audit: fileReceipt(counterfactualPath) }
        : {}),
    },
    policy: {
      exact_numeric_ids_and_build_are_authoritative: true,
      localized_names_are_runtime_keys: false,
      remote_player_cast_packets_required: false,
      provider_ownership_must_come_from_status_lifecycle: true,
      recipient_and_target_allegiance_inference_allowed: false,
      current_character_snapshot_substitution_allowed: false,
      unresolved_and_rejected_samples_preserved: true,
      candidate_integer_boundaries_are_formula_authority: false,
      ordinary_damage_mutation_allowed: false,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
      ui_display_allowed: false,
    },
    topology_contract: {
      support_path:
        "provider -> effect/status lifecycle -> recipient or enemy target",
      damage_path:
        "recipient damage action -> recipient or enemy target",
      join:
        "exact build/session/lifecycle instance/provider/recipient plus canonical damage sequence; no remote-player cast packet is required",
    },
    controlled_aba_contract: {
      lifecycle_eligibility:
        "only lifecycle instances with exact-provider-percent-transition witnesses for both apply and remove may enter final-integer adjudication; other valid contribution rows remain preserved outside the A/B/A search",
      phases: [
        "A1: effect 3003052 absent after an observed terminal or before the exact apply",
        "B: exactly one provider-owned effect 3003052 lifecycle active",
        "A2: effect 3003052 absent after that exact lifecycle terminal",
      ],
      minimum_repeats_per_phase: MINIMUM_REPEATS_PER_PHASE,
      minimum_distinct_coefficient_stage_signatures:
        MINIMUM_DISTINCT_STAGE_SIGNATURES,
      preferred_hit_event_ids: [1, 3],
      exact_equal_inputs: [
        "build, session, scene, recipient, direct source, target, ability, and hit_event_id",
        "numeric source/direct-source/target actor identities",
        "critical, lucky, damage mode, owner level/stage, type flags, property, normal-hit, passive, rainbow, hit-parts, and damage-weight inputs",
        "target attributes except current HP 11310 and every target status",
        "source attributes outside the proven Harmony primary/attack family",
        "source statuses after removing only exact effect 3003052",
        "packet-observed source/direct-source/target coordinates and packet position",
      ],
      exact_changed_inputs: [
        "effect 3003052 absent/present/absent with one numeric provider and one lifecycle instance",
        "11030 and 11031 change by the trace primary_provider_marginal",
        "11034 changes by provider_primary_raw_percent",
        "11330 and 11331 change by provider_attack_marginal",
        "11332 changes by attack_component_with_provider minus attack_component_without_provider",
      ],
      candidate_final_server_integer_boundaries: CANDIDATE_FINAL_BOUNDARIES,
      adjudication:
        "For each deterministic A/B/A group, project the absent integer from observed present damage times without-provider coefficient term divided by active coefficient term. Intersect exact boundaries across at least two distinct coefficient/fixed stage signatures. Reject ambiguity, nondeterminism, co-transition, or failed conservation.",
    },
    current_cohort: analysis.currentCohort,
    current_exact_aba_search: analysis.aba,
    current_near_match_funnel: analysis.nearMatchFunnel,
    nearest_reference_transition_diagnostic: analysis.nearestTransition,
    resource_bounds: {
      cohort_bytes: fs.statSync(cohortPath).size,
      selected_samples: analysis.currentCohort.selected_recipient_samples,
      trace_rows: analysis.currentCohort.priority_trace_rows,
      pair_comparisons: analysis.nearMatchFunnel.pair_comparisons,
      full_cartesian_pair_materialization: false,
      retained_near_match_examples_limit: MAX_EXAMPLES,
      rss_samples: rssSamples,
      maximum_sampled_rss_bytes: Math.max(...rssSamples.map((entry) => entry.rss_bytes)),
      configured_ram_ceiling_bytes: 36 * 1024 ** 3,
      sampled_rss_within_configured_ceiling:
        Math.max(...rssSamples.map((entry) => entry.rss_bytes)) <= 36 * 1024 ** 3,
    },
    acquisition_recipe: [
      "Use the same locally observed class-11 recipient and a stationary training target; record repeated ability-2352 hit 1 and hit 3 actions before, during, and after one Harmony Grace lifecycle.",
      "Keep every other party support effect and recipient self-buff unchanged. If any other source or target status changes, the group must remain rejected evidence.",
      "Retain the status apply/refresh/remove lifecycle, provider and recipient UUIDs, wire-start attributes/statuses, damage packets, positions, and the closed-run segment receipt. Do not wait for an unavailable remote-player cast packet.",
      "Regenerate the schema-46 ability cohort, replay audit, single-effect trace, and this receipt. Promotion remains fail-closed until one boundary survives replicated A/B/A groups and exact conservation.",
    ],
    conclusion: {
      current_exact_aba_groups: analysis.aba.qualifying_groups,
      current_distinct_stage_signatures:
        analysis.aba.qualifying_stage_signatures.length,
      selected_final_server_integer_boundary:
        analysis.aba.selected_final_server_integer_boundary,
      exact_final_server_integer_counterfactual_proven:
        analysis.aba.exact_final_server_integer_counterfactual_proven,
      acquisition_ready: closedRunEligible,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
      ui_display_allowed: false,
      blocker: !closedRunEligible
        ? "The packet-batched segment ended without the protocol Completed marker; its A/B/A search is retained as diagnostic evidence but cannot select production formula authority."
        : analysis.aba.exact_final_server_integer_counterfactual_proven
        ? "The final integer boundary is behaviorally selected for this exact A/B/A scope, but runtime/UI promotion remains a separate exact-build recipient/provider/stacking allowlist and aggregate-gate step."
        : "No replicated exact A/B/A group currently selects one final server integer boundary across two distinct stage signatures; capture the narrow ability-2352 control instead of mining another broad cohort.",
    },
  };
  report.content_sha256 = contentHash(report);
  verifyReport(report);
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  fs.writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
  process.stdout.write(`${JSON.stringify(summary(report), null, 2)}\n`);
}

function validateInputs(boundary, trace, cohort, transitionProof, counterfactual) {
  assert.equal(boundary.schema_version, 1);
  assert.ok([
    "tools/bpsr-harmony-grace-current-pack-boundary.mjs",
    "tools/bpsr-harmony-grace-current-pack-lifecycle-closure.mjs",
    "tools/bpsr-harmony-grace-capture-boundary.mjs",
  ].includes(boundary.generated_by));
  assert.equal(String(boundary.game_build), GAME_BUILD);
  assert.equal(Number(boundary.effect_id), EFFECT_ID);
  assert.equal(boundary.proof.exact_current_pack_identity, true);
  assert.equal(typeof boundary.proof.packet_batched_closed_run, "boolean");
  assert.equal(boundary.proof.ordinary_damage_conserved, true);
  assert.equal(boundary.policy.runtime_promotion_allowed, false);

  assert.equal(trace.schema_version, 6);
  assert.equal(trace.generated_by, "tools/bpsr-harmony-grace-single-effect-trace.mjs");
  assert.equal(String(trace.game_build), GAME_BUILD);
  assert.equal(Number(trace.effect_id), EFFECT_ID);
  assert.equal(trace.protocol_pack_digest, boundary.identity.protocol_pack_digest);
  assert.equal(trace.session_id, boundary.identity.session_id);
  assert.ok(Array.isArray(trace.traces) && trace.traces.length > 0);

  assert.equal(cohort.schema_version, 46);
  assert.equal(cohort.generated_by, "rlogs-bpsr-state-scaling-damage-proof");
  assert.equal(String(cohort.game_build), GAME_BUILD);
  assert.ok(cohort.selection.ability_ids.map(Number).includes(PRIORITY_ABILITY_ID));
  assert.ok(Array.isArray(cohort.samples) && cohort.samples.length > 0);

  assert.ok([1, 2].includes(Number(transitionProof.schema_version)));
  assert.equal(
    transitionProof.generated_by,
    "tools/bpsr-harmony-grace-lifecycle-transition-proof.mjs",
  );
  assert.equal(String(transitionProof.game_build), GAME_BUILD);
  assert.equal(Number(transitionProof.identity.effect_id), EFFECT_ID);
  const traceInstanceIds = [
    ...new Set(trace.traces.map((entry) => String(entry.lifecycle.instance_id))),
  ];
  for (const instanceId of traceInstanceIds) {
    assert.ok(
      boundary.identity.lifecycle_instance_ids.map(String).includes(instanceId),
      `trace instance ${instanceId} is absent from the closed boundary`,
    );
  }
  const priorityInstanceIds = [
    ...new Set(trace.traces
      .filter((entry) => Number(entry.ability_id) === PRIORITY_ABILITY_ID)
      .map((entry) => String(entry.lifecycle.instance_id))),
  ];
  let eligiblePriorityInstances = 0;
  for (const instanceId of priorityInstanceIds) {
    const exact = transitionProof.transition_witnesses.filter(
      (witness) =>
        String(witness.instance_id) === instanceId &&
        witness.classification === "exact-provider-percent-transition",
    );
    if (
      exact.some((witness) => witness.lifecycle_state === "applied") &&
      exact.some((witness) => witness.lifecycle_state === "removed")
    ) eligiblePriorityInstances += 1;
  }
  assert.ok(eligiblePriorityInstances > 0, "no priority lifecycle has exact apply and remove witnesses");

  if (counterfactual) {
    assert.equal(counterfactual.schema_version, 18);
    assert.equal(counterfactual.generated_by, "rlogs-bpsr-status-effect-counterfactual-proof");
    assert.equal(String(counterfactual.game_build), GAME_BUILD);
    assert.equal(counterfactual.processing.memory_limit_mib, 512);
    assert.equal(counterfactual.processing.measured_peak_within_configured_limit, true);
    assert.ok(counterfactual.effects.some(
      (entry) => Number(entry.effect_id) === EFFECT_ID && entry.locus === "source",
    ));
  }
  return boundary.proof.packet_batched_closed_run;
}

function analyze(trace, cohort, transitionProof) {
  const allPriorityTraces = trace.traces.filter(
    (entry) => Number(entry.ability_id) === PRIORITY_ABILITY_ID,
  );
  const eligibleInstances = exactApplyRemoveInstanceIds(transitionProof);
  const priorityTraces = allPriorityTraces.filter(
    (entry) => eligibleInstances.has(String(entry.lifecycle.instance_id)),
  );
  const excludedPriorityTraces = allPriorityTraces.filter(
    (entry) => !eligibleInstances.has(String(entry.lifecycle.instance_id)),
  );
  const recipientIds = new Set(priorityTraces.map((entry) => String(entry.recipient_entity_uuid)));
  const selected = cohort.samples.filter(
    (sample) =>
      Number(sample.ability_id) === PRIORITY_ABILITY_ID &&
      recipientIds.has(String(sample.source_entity_uuid)),
  );
  const present = selected.filter((sample) => effectRows(cohort, sample).length > 0);
  const absent = selected.filter((sample) => effectRows(cohort, sample).length === 0);
  const aba = findExactAbaGroups(trace, cohort, selected, priorityTraces);
  const nearMatchFunnel = buildNearMatchFunnel(cohort, selected, priorityTraces);
  return {
    currentCohort: {
      total_priority_ability_samples: cohort.samples.length,
      selected_recipient_samples: selected.length,
      effect_present_samples: present.length,
      effect_absent_samples: absent.length,
      distinct_targets: new Set(selected.map((sample) => String(sample.target_entity_uuid))).size,
      total_priority_trace_rows: allPriorityTraces.length,
      priority_trace_rows: priorityTraces.length,
      excluded_priority_trace_rows_without_exact_apply_remove_witness:
        excludedPriorityTraces.length,
      eligible_lifecycle_instance_ids: [...new Set(priorityTraces.map(
        (entry) => String(entry.lifecycle.instance_id),
      ))].sort(),
      excluded_lifecycle_instance_ids: [...new Set(excludedPriorityTraces.map(
        (entry) => String(entry.lifecycle.instance_id),
      ))].sort(),
      priority_trace_hit_event_ids: [...new Set(priorityTraces.map(
        (entry) => Number(entry.arithmetic.hit_event_id),
      ))].sort((left, right) => left - right),
    },
    aba,
    nearMatchFunnel,
    nearestTransition: nearestTransitionDiagnostic(trace, cohort, selected, priorityTraces),
  };
}

function exactApplyRemoveInstanceIds(transitionProof) {
  const states = new Map();
  for (const witness of transitionProof.transition_witnesses ?? []) {
    if (witness.classification !== "exact-provider-percent-transition") continue;
    const instanceId = String(witness.instance_id);
    const bucket = states.get(instanceId) ?? new Set();
    bucket.add(witness.lifecycle_state);
    states.set(instanceId, bucket);
  }
  return new Set([...states.entries()]
    .filter(([, lifecycleStates]) =>
      lifecycleStates.has("applied") && lifecycleStates.has("removed"))
    .map(([instanceId]) => instanceId));
}

function findExactAbaGroups(trace, cohort, samples, priorityTraces) {
  const traceBySequence = new Map(priorityTraces.map(
    (entry) => [Number(entry.damage_sequence), entry],
  ));
  const groups = new Map();
  for (const sample of samples) {
    const key = controlledKey(cohort, sample);
    const group = groups.get(key) ?? { absent: [], present: [] };
    const effect = effectRows(cohort, sample);
    (effect.length === 0 ? group.absent : group.present).push(sample);
    groups.set(key, group);
  }

  const examples = [];
  const survivors = new Set(CANDIDATE_FINAL_BOUNDARIES);
  const stageSignatures = new Set();
  let structuralGroups = 0;
  let chronologicalGroups = 0;
  let qualifyingGroups = 0;
  let rejectedProviderOwnership = 0;
  let rejectedAttributeTransition = 0;
  let rejectedPhaseRepeats = 0;
  let rejectedNondeterminism = 0;

  for (const group of groups.values()) {
    const tracedPresent = group.present.filter((sample) => traceBySequence.has(Number(sample.sequence)));
    if (tracedPresent.length === 0 || group.absent.length === 0) continue;
    structuralGroups += 1;
    const reference = traceBySequence.get(Number(tracedPresent[0].sequence));
    const lifecycle = reference.lifecycle;
    const before = group.absent.filter(
      (sample) => Number(sample.observed_micros) < Number(lifecycle.observed_micros),
    );
    const after = group.absent.filter(
      (sample) => Number(sample.observed_micros) > Number(lifecycle.terminal.observed_micros),
    );
    const during = tracedPresent.filter(
      (sample) =>
        Number(sample.observed_micros) >= Number(lifecycle.observed_micros) &&
        Number(sample.observed_micros) <= Number(lifecycle.terminal.observed_micros),
    );
    if (before.length > 0 && during.length > 0 && after.length > 0) chronologicalGroups += 1;
    if ([before, during, after].some((phase) => phase.length < MINIMUM_REPEATS_PER_PHASE)) {
      rejectedPhaseRepeats += 1;
      continue;
    }
    if (!during.every((sample) => exactProviderOwnedEffect(cohort, sample, reference))) {
      rejectedProviderOwnership += 1;
      continue;
    }
    if (![...before, ...after].every((absentSample) =>
      during.every((presentSample) => exactExpectedAttributeTransition(
        cohort,
        absentSample,
        presentSample,
        traceBySequence.get(Number(presentSample.sequence)),
      )),
    )) {
      rejectedAttributeTransition += 1;
      continue;
    }
    const phaseAmounts = [before, during, after].map(
      (phase) => new Set(phase.map((sample) => String(sample.amount))),
    );
    if (phaseAmounts.some((amounts) => amounts.size !== 1) ||
        [...phaseAmounts[0]][0] !== [...phaseAmounts[2]][0]) {
      rejectedNondeterminism += 1;
      continue;
    }

    const absentAmount = BigInt([...phaseAmounts[0]][0]);
    let groupCandidates = new Set(CANDIDATE_FINAL_BOUNDARIES);
    for (const presentSample of during) {
      const traceEntry = traceBySequence.get(Number(presentSample.sequence));
      const activeTerm = BigInt(traceEntry.arithmetic.active_coefficient_term);
      const withoutTerm = BigInt(traceEntry.arithmetic.without_provider_coefficient_term);
      const presentAmount = BigInt(presentSample.amount);
      groupCandidates = new Set([...groupCandidates].filter(
        (boundary) => roundRatio(presentAmount * withoutTerm, activeTerm, boundary) === absentAmount,
      ));
      stageSignatures.add([
        traceEntry.arithmetic.hit_event_id,
        traceEntry.arithmetic.coefficient_basis_points,
        traceEntry.arithmetic.fixed_parameter,
        activeTerm,
        withoutTerm,
      ].join("|"));
    }
    if (groupCandidates.size === 0) continue;
    qualifyingGroups += 1;
    for (const candidate of [...survivors]) {
      if (!groupCandidates.has(candidate)) survivors.delete(candidate);
    }
    if (examples.length < MAX_EXAMPLES) {
      examples.push({
        lifecycle_instance_id: String(reference.lifecycle.instance_id),
        provider_entity_uuid: String(reference.provider_entity_uuid),
        recipient_entity_uuid: String(reference.recipient_entity_uuid),
        target_entity_uuid: String(reference.damage_target_entity_uuid),
        hit_event_id: Number(reference.arithmetic.hit_event_id),
        before_sequences: before.map((sample) => Number(sample.sequence)),
        present_sequences: during.map((sample) => Number(sample.sequence)),
        after_sequences: after.map((sample) => Number(sample.sequence)),
        absent_amount: absentAmount.toString(),
        present_amount: String(during[0].amount),
        compatible_boundaries: [...groupCandidates],
        ordinary_damage_conservation: true,
      });
    }
  }

  const exact = qualifyingGroups >= 2 &&
    stageSignatures.size >= MINIMUM_DISTINCT_STAGE_SIGNATURES &&
    survivors.size === 1;
  return {
    structural_present_absent_groups: structuralGroups,
    chronological_a_b_a_groups: chronologicalGroups,
    qualifying_groups: qualifyingGroups,
    rejected_for_phase_repeat_requirement: rejectedPhaseRepeats,
    rejected_for_provider_ownership: rejectedProviderOwnership,
    rejected_for_exact_attribute_transition: rejectedAttributeTransition,
    rejected_for_nondeterministic_damage: rejectedNondeterminism,
    qualifying_stage_signatures: [...stageSignatures].sort(),
    compatible_final_server_integer_boundaries: [...survivors],
    selected_final_server_integer_boundary: exact ? [...survivors][0] : null,
    exact_final_server_integer_counterfactual_proven: exact,
    retained_examples: examples,
    retained_examples_truncated: qualifyingGroups > examples.length,
  };
}

function buildNearMatchFunnel(cohort, samples, priorityTraces) {
  const absentSamples = samples.filter((sample) => effectRows(cohort, sample).length === 0);
  const sampleBySequence = new Map(samples.map((sample) => [Number(sample.sequence), sample]));
  const componentsBySequence = new Map(samples.map((sample) => [
    Number(sample.sequence),
    controlledComponents(cohort, sample),
  ]));
  const checkOrder = [
    "same_target_entity",
    "same_direct_source_entity",
    "same_actor_identities",
    "same_critical_and_lucky",
    "same_packet_formula_context",
    "same_source_non_effect_attributes",
    "exact_expected_effect_attribute_transition",
    "same_target_attributes_without_current_hp",
    "same_source_statuses_without_effect",
    "same_target_statuses",
    "same_geometry",
  ];
  const cumulativeSurvivors = Object.fromEntries(checkOrder.map((name) => [name, 0]));
  const mismatchPairCounts = Object.fromEntries(checkOrder.map((name) => [name, 0]));
  const phaseCounts = { A1: 0, A2: 0 };
  const nearest = [];
  let pairComparisons = 0;
  let traceRowsJoined = 0;
  let traceRowsWithoutPresentSample = 0;
  let tracePhasePairsWithoutBaseCandidate = 0;

  for (const traceEntry of priorityTraces) {
    const present = sampleBySequence.get(Number(traceEntry.damage_sequence));
    if (!present) {
      traceRowsWithoutPresentSample += 1;
      continue;
    }
    traceRowsJoined += 1;
    const presentComponents = componentsBySequence.get(Number(present.sequence));
    for (const phase of ["A1", "A2"]) {
      const boundaryMicros = phase === "A1"
        ? Number(traceEntry.lifecycle.observed_micros)
        : Number(traceEntry.lifecycle.terminal.observed_micros);
      const candidates = absentSamples.filter((candidate) => {
        const before = Number(candidate.observed_micros) < boundaryMicros;
        const after = Number(candidate.observed_micros) > boundaryMicros;
        return (phase === "A1" ? before : after) &&
          String(candidate.session_id) === String(present.session_id) &&
          Number(candidate.run_ordinal) === Number(present.run_ordinal) &&
          Number(candidate.scene_id) === Number(present.scene_id) &&
          String(candidate.source_entity_uuid) === String(present.source_entity_uuid) &&
          Number(candidate.ability_id) === Number(present.ability_id) &&
          Number(candidate.hit_event_id) === Number(present.hit_event_id);
      });
      phaseCounts[phase] += candidates.length;
      if (candidates.length === 0) tracePhasePairsWithoutBaseCandidate += 1;
      const ranked = [];
      for (const absent of candidates) {
        pairComparisons += 1;
        const absentComponents = componentsBySequence.get(Number(absent.sequence));
        const checks = compareControlledComponents(
          cohort,
          absent,
          present,
          absentComponents,
          presentComponents,
          traceEntry,
        );
        let survives = true;
        for (const name of checkOrder) {
          if (!checks[name]) {
            mismatchPairCounts[name] += 1;
            survives = false;
          }
          if (survives) cumulativeSurvivors[name] += 1;
        }
        const mismatches = checkOrder.filter((name) => !checks[name]);
        ranked.push({
          phase,
          lifecycle_instance_id: String(traceEntry.lifecycle.instance_id),
          provider_entity_uuid: String(traceEntry.provider_entity_uuid),
          recipient_entity_uuid: String(traceEntry.recipient_entity_uuid),
          present_sequence: Number(present.sequence),
          absent_sequence: Number(absent.sequence),
          target_entity_uuid_present: String(present.target_entity_uuid),
          target_entity_uuid_absent: String(absent.target_entity_uuid),
          direct_source_entity_uuid_present: String(present.direct_source_entity_uuid),
          direct_source_entity_uuid_absent: String(absent.direct_source_entity_uuid),
          observed_micros_distance: Math.abs(
            Number(present.observed_micros) - Number(absent.observed_micros),
          ),
          present_damage: String(present.amount),
          absent_damage: String(absent.amount),
          mismatch_count: mismatches.length,
          mismatches,
          source_status_co_transition_count: statusDifference(
            statuses(cohort, absent.source_status_state_id),
            statuses(cohort, present.source_status_state_id),
          ).length,
          target_status_co_transition_count: statusDifferenceIncludingEffect(
            statuses(cohort, absent.target_status_state_id),
            statuses(cohort, present.target_status_state_id),
          ).length,
          non_effect_source_attribute_difference_ids: attributeDifferenceIds(
            absentComponents.source_attributes,
            presentComponents.source_attributes,
          ),
          target_attribute_difference_ids: attributeDifferenceIds(
            absentComponents.target_attributes,
            presentComponents.target_attributes,
          ),
          exact_controlled_pair: mismatches.length === 0,
          rejected_as_formula_authority: mismatches.length !== 0,
        });
      }
      ranked.sort((left, right) =>
        left.mismatch_count - right.mismatch_count ||
        left.observed_micros_distance - right.observed_micros_distance ||
        left.absent_sequence - right.absent_sequence);
      nearest.push(...ranked.slice(0, 2));
    }
  }

  nearest.sort((left, right) =>
    left.mismatch_count - right.mismatch_count ||
    left.observed_micros_distance - right.observed_micros_distance ||
    left.present_sequence - right.present_sequence ||
    left.absent_sequence - right.absent_sequence);
  const funnel = [];
  let inputPairs = pairComparisons;
  for (const name of checkOrder) {
    const survivors = cumulativeSurvivors[name];
    funnel.push({
      check: name,
      input_pairs: inputPairs,
      surviving_pairs: survivors,
      rejected_pairs: inputPairs - survivors,
    });
    inputPairs = survivors;
  }
  const mismatchRanking = Object.entries(mismatchPairCounts)
    .map(([check, pairs]) => ({ check, mismatching_pairs: pairs }))
    .sort((left, right) =>
      right.mismatching_pairs - left.mismatching_pairs || left.check.localeCompare(right.check));
  return {
    trace_rows: priorityTraces.length,
    trace_rows_joined_to_present_sample: traceRowsJoined,
    trace_rows_without_present_sample: traceRowsWithoutPresentSample,
    trace_phase_pairs_without_base_candidate: tracePhasePairsWithoutBaseCandidate,
    base_candidate_definition:
      "same exact build/session/run/scene/recipient/ability/hit and correct before-or-after lifecycle phase; target, direct source, packet, state, and geometry are evaluated separately",
    phase_base_candidate_pairs: phaseCounts,
    pair_comparisons: pairComparisons,
    cumulative_exactness_funnel: funnel,
    mismatch_pair_counts: mismatchRanking,
    exact_controlled_pairs: funnel.at(-1)?.surviving_pairs ?? 0,
    retained_nearest_pairs: nearest.slice(0, MAX_EXAMPLES),
    retained_nearest_pairs_truncated: nearest.length > MAX_EXAMPLES,
    formula_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function controlledComponents(cohort, sample) {
  const sourceAttributes = attributes(cohort, sample.source_attribute_state_id)
    .filter((entry) => !EFFECT_FAMILY_ATTRIBUTE_IDS.has(Number(entry.attribute_id)));
  const targetAttributes = attributes(cohort, sample.target_attribute_state_id)
    .filter((entry) => Number(entry.attribute_id) !== CURRENT_HP_ATTRIBUTE_ID);
  const sourceStatuses = statuses(cohort, sample.source_status_state_id)
    .filter((entry) => Number(entry.effect_id) !== EFFECT_ID);
  const packet = structuredClone(sample.packet ?? {});
  delete packet.normal_value;
  delete packet.lucky_value;
  delete packet.skill_effect_total_damage;
  delete packet.skill_effect_group_index;
  delete packet.skill_effect_component_index;
  delete packet.skill_effect_component_count;
  return {
    session_id: sample.session_id,
    run_ordinal: sample.run_ordinal,
    scene_id: sample.scene_id,
    source_entity_uuid: String(sample.source_entity_uuid),
    direct_source_entity_uuid: String(sample.direct_source_entity_uuid),
    target_entity_uuid: String(sample.target_entity_uuid),
    source_actor_identity: sample.source_actor_identity,
    direct_source_actor_identity: sample.direct_source_actor_identity,
    target_actor_identity: sample.target_actor_identity,
    ability_id: sample.ability_id,
    hit_event_id: sample.hit_event_id,
    critical: sample.critical,
    lucky: sample.lucky,
    packet,
    source_attributes: sourceAttributes,
    target_attributes: targetAttributes,
    source_statuses: sourceStatuses,
    target_statuses: statuses(cohort, sample.target_status_state_id),
    source_position: coordinates(sample.source_position_at_wire_message_start),
    direct_source_position: coordinates(sample.direct_source_position_at_wire_message_start),
    target_position: coordinates(sample.target_position_at_wire_message_start),
  };
}

function compareControlledComponents(
  cohort,
  absent,
  present,
  absentComponents,
  presentComponents,
  traceEntry,
) {
  const same = (left, right) => stableStringify(left) === stableStringify(right);
  return {
    same_target_entity:
      absentComponents.target_entity_uuid === presentComponents.target_entity_uuid,
    same_direct_source_entity:
      absentComponents.direct_source_entity_uuid === presentComponents.direct_source_entity_uuid,
    same_actor_identities: same(
      [
        absentComponents.source_actor_identity,
        absentComponents.direct_source_actor_identity,
        absentComponents.target_actor_identity,
      ],
      [
        presentComponents.source_actor_identity,
        presentComponents.direct_source_actor_identity,
        presentComponents.target_actor_identity,
      ],
    ),
    same_critical_and_lucky:
      absentComponents.critical === presentComponents.critical &&
      absentComponents.lucky === presentComponents.lucky,
    same_packet_formula_context: same(absentComponents.packet, presentComponents.packet),
    same_source_non_effect_attributes: same(
      absentComponents.source_attributes,
      presentComponents.source_attributes,
    ),
    exact_expected_effect_attribute_transition: exactExpectedAttributeTransition(
      cohort,
      absent,
      present,
      traceEntry,
    ),
    same_target_attributes_without_current_hp: same(
      absentComponents.target_attributes,
      presentComponents.target_attributes,
    ),
    same_source_statuses_without_effect: same(
      absentComponents.source_statuses,
      presentComponents.source_statuses,
    ),
    same_target_statuses: same(
      absentComponents.target_statuses,
      presentComponents.target_statuses,
    ),
    same_geometry: same(
      [
        absentComponents.source_position,
        absentComponents.direct_source_position,
        absentComponents.target_position,
      ],
      [
        presentComponents.source_position,
        presentComponents.direct_source_position,
        presentComponents.target_position,
      ],
    ),
  };
}

function attributeDifferenceIds(left, right) {
  const leftMap = new Map(left.map((entry) => [Number(entry.attribute_id), String(entry.value)]));
  const rightMap = new Map(right.map((entry) => [Number(entry.attribute_id), String(entry.value)]));
  return [...new Set([...leftMap.keys(), ...rightMap.keys()])]
    .filter((attributeId) => leftMap.get(attributeId) !== rightMap.get(attributeId))
    .sort((a, b) => a - b);
}

function controlledKey(cohort, sample) {
  return stableStringify(controlledComponents(cohort, sample));
}

function exactProviderOwnedEffect(cohort, sample, traceEntry) {
  const effects = effectRows(cohort, sample);
  return effects.length === 1 &&
    String(effects[0].source_entity_uuid) === String(traceEntry.provider_entity_uuid) &&
    Number(effects[0].stacks) === 1 &&
    Number(effects[0].origin_source_type_id) === Number(traceEntry.lifecycle.source_type_id) &&
    Number(effects[0].origin_source_config_id) === Number(traceEntry.lifecycle.source_config_id);
}

function exactExpectedAttributeTransition(cohort, absent, present, traceEntry) {
  if (!traceEntry) return false;
  const absentMap = attributeMap(attributes(cohort, absent.source_attribute_state_id));
  const presentMap = attributeMap(attributes(cohort, present.source_attribute_state_id));
  const arithmetic = traceEntry.arithmetic;
  const componentDelta = BigInt(arithmetic.attack_component_with_provider) -
    BigInt(arithmetic.attack_component_without_provider);
  const expected = new Map([
    [11_030, BigInt(arithmetic.primary_provider_marginal)],
    [11_031, BigInt(arithmetic.primary_provider_marginal)],
    [11_034, BigInt(arithmetic.provider_primary_raw_percent)],
    [11_330, BigInt(arithmetic.provider_attack_marginal)],
    [11_331, BigInt(arithmetic.provider_attack_marginal)],
    [11_332, componentDelta],
  ]);
  for (const [attributeId, delta] of expected) {
    if (!absentMap.has(attributeId) || !presentMap.has(attributeId)) return false;
    if (presentMap.get(attributeId) - absentMap.get(attributeId) !== delta) return false;
  }
  return true;
}

function nearestTransitionDiagnostic(trace, cohort, samples, priorityTraces) {
  if (priorityTraces.length === 0) return null;
  const reference = priorityTraces[0];
  const candidates = samples.filter(
    (sample) =>
      String(sample.target_entity_uuid) === String(reference.damage_target_entity_uuid) &&
      Number(sample.hit_event_id) === Number(reference.arithmetic.hit_event_id) &&
      effectRows(cohort, sample).length === 0,
  );
  const before = candidates
    .filter((sample) => Number(sample.observed_micros) <= Number(reference.lifecycle.observed_micros))
    .sort((left, right) => Number(right.observed_micros) - Number(left.observed_micros))[0];
  const after = candidates
    .filter((sample) => Number(sample.observed_micros) >= Number(reference.lifecycle.terminal.observed_micros))
    .sort((left, right) => Number(left.observed_micros) - Number(right.observed_micros))[0];
  const present = cohort.samples.find(
    (sample) => Number(sample.sequence) === Number(reference.damage_sequence),
  );
  if (!present) return { reference_damage_sequence: reference.damage_sequence, cohort_joined: false };
  const diagnostics = [before, after].map((absentSample, index) => {
    if (!absentSample) return { phase: index === 0 ? "A1" : "A2", observed: false };
    const statusDiff = statusDifference(
      statuses(cohort, absentSample.source_status_state_id),
      statuses(cohort, present.source_status_state_id),
    );
    return {
      phase: index === 0 ? "A1" : "A2",
      observed: true,
      absent_sequence: absentSample.sequence,
      absent_observed_micros: absentSample.observed_micros,
      absent_damage: String(absentSample.amount),
      present_damage: String(present.amount),
      observed_damage_delta: String(BigInt(present.amount) - BigInt(absentSample.amount)),
      exact_expected_attribute_transition: exactExpectedAttributeTransition(
        cohort,
        absentSample,
        present,
        reference,
      ),
      source_status_co_transition_count: statusDiff.length,
      source_status_co_transitions: statusDiff.slice(0, 12),
      rejected_as_formula_authority: true,
    };
  });
  return {
    reference_damage_sequence: reference.damage_sequence,
    lifecycle_instance_id: String(reference.lifecycle.instance_id),
    apply_observed_micros: reference.lifecycle.observed_micros,
    terminal_observed_micros: reference.lifecycle.terminal.observed_micros,
    present_sequence: present.sequence,
    present_damage: String(present.amount),
    exact_rational_provider_marginal: reference.contribution,
    phases: diagnostics,
    conclusion:
      "Nearby absent samples exist, but co-transitioned source state means their observed damage difference cannot be assigned to Harmony Grace.",
  };
}

function statusDifference(left, right) {
  return statusDifferenceWithPolicy(left, right, true);
}

function statusDifferenceIncludingEffect(left, right) {
  return statusDifferenceWithPolicy(left, right, false);
}

function statusDifferenceWithPolicy(left, right, excludeHarmonyEffect) {
  const key = (entry) => [
    entry.effect_id,
    String(entry.source_entity_uuid),
    entry.origin_source_type_id,
    entry.origin_source_config_id,
  ].join("|");
  const a = new Map(left.map((entry) => [key(entry), entry]));
  const b = new Map(right.map((entry) => [key(entry), entry]));
  const result = [];
  for (const identity of new Set([...a.keys(), ...b.keys()])) {
    if (excludeHarmonyEffect && identity.startsWith(`${EFFECT_ID}|`)) continue;
    const before = a.get(identity) ?? null;
    const after = b.get(identity) ?? null;
    if (stableStringify(before) !== stableStringify(after)) {
      result.push({ identity, absent: before, present: after });
    }
  }
  return result;
}

function effectRows(cohort, sample) {
  return statuses(cohort, sample.source_status_state_id)
    .filter((entry) => Number(entry.effect_id) === EFFECT_ID);
}

function statuses(cohort, stateId) {
  return cohort.status_states[Number(stateId)] ?? [];
}

function attributes(cohort, stateId) {
  return cohort.attribute_states[Number(stateId)] ?? [];
}

function attributeMap(values) {
  return new Map(values.map((entry) => [Number(entry.attribute_id), BigInt(entry.value)]));
}

function coordinates(position) {
  if (!position) return null;
  return { x: position.x, y: position.y, z: position.z };
}

function roundRatio(numerator, denominator, boundary) {
  assert.ok(numerator >= 0n && denominator > 0n);
  if (boundary === "floor") return numerator / denominator;
  if (boundary === "ceil") return (numerator + denominator - 1n) / denominator;
  if (boundary === "nearest_half_up") {
    return (2n * numerator + denominator) / (2n * denominator);
  }
  throw new Error(`unsupported boundary ${boundary}`);
}

function verify(values) {
  const inputPath = path.resolve(required(values, "input"));
  const report = readJson(inputPath);
  verifyReport(report);
  for (const receipt of Object.values(report.inputs)) verifyFileReceipt(receipt);
  process.stdout.write(`${JSON.stringify(summary(report), null, 2)}\n`);
}

function verifyReport(report) {
  assert.equal(SUPPORTED_SCHEMA_VERSIONS.has(Number(report.schema_version)), true);
  assert.equal(report.generated_by, GENERATED_BY);
  assert.equal(String(report.game_build), GAME_BUILD);
  assert.equal(Number(report.effect_id), EFFECT_ID);
  assert.equal(report.policy.remote_player_cast_packets_required, false);
  assert.equal(report.policy.provider_rdps_credit_allowed, false);
  assert.equal(report.policy.runtime_promotion_allowed, false);
  assert.equal(report.policy.ui_display_allowed, false);
  assert.equal(report.conclusion.provider_rdps_credit_allowed, false);
  assert.equal(report.conclusion.runtime_promotion_allowed, false);
  assert.equal(report.conclusion.ui_display_allowed, false);
  assert.equal(typeof report.conclusion.acquisition_ready, "boolean");
  if (!report.conclusion.acquisition_ready) {
    assert.equal(
      report.current_exact_aba_search.incomplete_capture_rejected_as_formula_authority,
      true,
    );
  }
  const exact = report.current_exact_aba_search.exact_final_server_integer_counterfactual_proven;
  assert.equal(
    report.conclusion.exact_final_server_integer_counterfactual_proven,
    exact,
  );
  if (exact) {
    assert.equal(report.current_exact_aba_search.compatible_final_server_integer_boundaries.length, 1);
    assert.ok(report.current_exact_aba_search.qualifying_groups >= 2);
    assert.ok(
      report.current_exact_aba_search.qualifying_stage_signatures.length >=
      MINIMUM_DISTINCT_STAGE_SIGNATURES,
    );
  } else {
    assert.equal(report.conclusion.selected_final_server_integer_boundary, null);
  }
  if (Number(report.schema_version) >= 2) {
    assert.ok(report.current_near_match_funnel.pair_comparisons >= 0);
    assert.ok(Array.isArray(report.current_near_match_funnel.cumulative_exactness_funnel));
    assert.equal(report.current_near_match_funnel.formula_authority, false);
    assert.equal(report.current_near_match_funnel.provider_rdps_credit_allowed, false);
    assert.equal(report.resource_bounds.full_cartesian_pair_materialization, false);
    assert.equal(report.resource_bounds.sampled_rss_within_configured_ceiling, true);
  }
  assert.equal(report.content_sha256, contentHash(report));
}

function selfTest() {
  assert.equal(roundRatio(10n, 3n, "floor"), 3n);
  assert.equal(roundRatio(10n, 3n, "ceil"), 4n);
  assert.equal(roundRatio(10n, 3n, "nearest_half_up"), 3n);
  assert.equal(roundRatio(11n, 3n, "nearest_half_up"), 4n);
  assert.deepEqual(
    [...exactApplyRemoveInstanceIds({ transition_witnesses: [
      { instance_id: "7", classification: "exact-provider-percent-transition", lifecycle_state: "applied" },
      { instance_id: "7", classification: "exact-provider-percent-transition", lifecycle_state: "removed" },
      { instance_id: "8", classification: "exact-provider-percent-transition", lifecycle_state: "applied" },
      { instance_id: "8", classification: "simultaneous-family-input-change", lifecycle_state: "removed" },
    ] })],
    ["7"],
  );

  const cohort = syntheticCohort();
  const trace = syntheticTrace();
  const analysis = findExactAbaGroups(
    trace,
    cohort,
    cohort.samples,
    trace.traces,
  );
  assert.equal(analysis.qualifying_groups, 2);
  assert.equal(analysis.qualifying_stage_signatures.length, 2);
  assert.deepEqual(analysis.compatible_final_server_integer_boundaries, ["floor"]);
  assert.equal(analysis.selected_final_server_integer_boundary, "floor");
  assert.equal(analysis.exact_final_server_integer_counterfactual_proven, true);
  const nearMatch = buildNearMatchFunnel(cohort, cohort.samples, trace.traces);
  assert.equal(nearMatch.trace_rows_joined_to_present_sample, 4);
  assert.equal(nearMatch.pair_comparisons, 16);
  assert.equal(nearMatch.exact_controlled_pairs, 16);
  assert.equal(nearMatch.retained_nearest_pairs[0].mismatch_count, 0);

  const confounded = structuredClone(cohort);
  confounded.status_states[0].push({
    effect_id: 999,
    source_entity_uuid: 10,
    stacks: 1,
    level: 1,
    origin_source_type_id: null,
    origin_source_config_id: null,
  });
  const rejected = findExactAbaGroups(trace, confounded, confounded.samples, trace.traces);
  assert.equal(rejected.qualifying_groups, 0);
  assert.equal(rejected.exact_final_server_integer_counterfactual_proven, false);
  process.stdout.write("self-test passed\n");
}

function syntheticCohort() {
  const absentAttributes = [
    { attribute_id: 11_030, value: 1_000 },
    { attribute_id: 11_031, value: 900 },
    { attribute_id: 11_034, value: 100 },
    { attribute_id: 11_330, value: 1_000 },
    { attribute_id: 11_331, value: 900 },
    { attribute_id: 11_332, value: 800 },
    { attribute_id: 11_350, value: 500 },
  ];
  const presentAttributes = absentAttributes.map((entry) => ({ ...entry }));
  const deltas = new Map([[11_030, 10], [11_031, 10], [11_034, 200], [11_330, 10], [11_331, 10], [11_332, 8]]);
  for (const entry of presentAttributes) entry.value += deltas.get(entry.attribute_id) ?? 0;
  const effect = {
    effect_id: EFFECT_ID,
    source_entity_uuid: 99,
    stacks: 1,
    level: 1,
    origin_source_type_id: 1,
    origin_source_config_id: 3_003_053,
  };
  const base = {
    session_id: "s",
    run_ordinal: 1,
    scene_id: 1,
    source_entity_uuid: 10,
    direct_source_entity_uuid: 11,
    target_entity_uuid: 12,
    source_actor_identity: { entity_type_id: 10, class_id: 11 },
    direct_source_actor_identity: { entity_type_id: 1, monster_id: 3_100_002 },
    target_actor_identity: { entity_type_id: 1, monster_id: 1 },
    ability_id: PRIORITY_ABILITY_ID,
    hit_event_id: 1,
    critical: false,
    lucky: false,
    packet: { owner_id: PRIORITY_ABILITY_ID, owner_level: 1, damage_mode: 1, property: 7 },
    source_attribute_state_id: 0,
    target_attribute_state_id: 2,
    source_status_state_id: 0,
    target_status_state_id: 0,
    source_position_at_wire_message_start: { x: 1, y: 2, z: 3 },
    direct_source_position_at_wire_message_start: { x: 1, y: 2, z: 3 },
    target_position_at_wire_message_start: { x: 4, y: 5, z: 6 },
  };
  const samples = [];
  let sequence = 1;
  for (const hit of [1, 3]) {
    for (const [phase, times, amount, state] of [
      ["before", [10, 11], hit === 1 ? 82 : 64, 0],
      ["present", [20, 21], hit === 1 ? 101 : 81, 1],
      ["after", [40, 41], hit === 1 ? 82 : 64, 0],
    ]) {
      for (const observed_micros of times) {
        samples.push({
          ...structuredClone(base),
          sequence: sequence++,
          observed_micros,
          hit_event_id: hit,
          amount,
          normal_value: amount,
          source_attribute_state_id: state,
          source_status_state_id: state,
          synthetic_phase: phase,
        });
      }
    }
  }
  return {
    attribute_states: [absentAttributes, presentAttributes, [{ attribute_id: 11_350, value: 500 }]],
    status_states: [[], [effect]],
    samples,
  };
}

function syntheticTrace() {
  const rows = [];
  for (const [sequence, hit, active, without] of [
    [3, 1, 100, 82],
    [4, 1, 100, 82],
    [9, 3, 80, 64],
    [10, 3, 80, 64],
  ]) {
    rows.push({
      damage_sequence: sequence,
      provider_entity_uuid: "99",
      recipient_entity_uuid: "10",
      damage_target_entity_uuid: "12",
      ability_id: PRIORITY_ABILITY_ID,
      lifecycle: {
        instance_id: "7",
        source_type_id: 1,
        source_config_id: 3_003_053,
        observed_micros: 15,
        terminal: { observed_micros: 35 },
      },
      arithmetic: {
        hit_event_id: hit,
        coefficient_basis_points: hit === 1 ? "10000" : "8000",
        fixed_parameter: "0",
        primary_provider_marginal: "10",
        provider_primary_raw_percent: "200",
        provider_attack_marginal: "10",
        attack_component_with_provider: "100",
        attack_component_without_provider: "92",
        active_coefficient_term: String(active),
        without_provider_coefficient_term: String(without),
      },
    });
  }
  return { traces: rows };
}

function summary(report) {
  return {
    output_schema_version: report.schema_version,
    selected_recipient_samples: report.current_cohort.selected_recipient_samples,
    effect_absent_samples: report.current_cohort.effect_absent_samples,
    effect_present_samples: report.current_cohort.effect_present_samples,
    exact_aba_groups: report.current_exact_aba_search.qualifying_groups,
    distinct_stage_signatures:
      report.current_exact_aba_search.qualifying_stage_signatures.length,
    selected_boundary:
      report.current_exact_aba_search.selected_final_server_integer_boundary,
    exact_final_integer_proven:
      report.current_exact_aba_search.exact_final_server_integer_counterfactual_proven,
    near_match_pair_comparisons:
      report.current_near_match_funnel?.pair_comparisons ?? null,
    near_match_exact_controlled_pairs:
      report.current_near_match_funnel?.exact_controlled_pairs ?? null,
    maximum_sampled_rss_bytes: report.resource_bounds?.maximum_sampled_rss_bytes ?? null,
    runtime_promotion_allowed: report.conclusion.runtime_promotion_allowed,
  };
}

function fileReceipt(filePath) {
  const stat = fs.statSync(filePath);
  return {
    path: filePath.replaceAll("\\", "/"),
    bytes: stat.size,
    sha256: sha256(filePath),
  };
}

function verifyFileReceipt(receipt) {
  const actual = fileReceipt(path.resolve(receipt.path));
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
  if (fs.existsSync(filePath)) throw new Error(`refusing to overwrite existing output: ${filePath}`);
}

function required(values, key) {
  const value = values.get(key);
  if (!value) throw new Error(`missing --${key}`);
  return value;
}

function parseArgs(args) {
  const values = new Map();
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index];
    const value = args[index + 1];
    if (!key?.startsWith("--") || value == null) usage(1);
    values.set(key.slice(2), value);
  }
  return values;
}

function usage(exitCode) {
  process.stderr.write(
    "usage:\n" +
    "  node tools/bpsr-harmony-grace-final-integer-acquisition.mjs generate --boundary FILE --trace FILE --cohort FILE --transition-proof FILE [--counterfactual FILE] --output FILE\n" +
    "  node tools/bpsr-harmony-grace-final-integer-acquisition.mjs verify --input FILE\n" +
    "  node tools/bpsr-harmony-grace-final-integer-acquisition.mjs self-test\n",
  );
  process.exit(exitCode);
}
