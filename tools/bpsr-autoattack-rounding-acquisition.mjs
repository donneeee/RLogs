#!/usr/bin/env node

import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 3;
const SUPPORTED_SCHEMA_VERSIONS = new Set([1, 2, SCHEMA_VERSION]);
const GENERATED_BY = "tools/bpsr-autoattack-rounding-acquisition.mjs";
const GAME_BUILD = "24687926";
const ABILITY_ID = 2_900_840;
const EFFECT_ID = 2_110_140;
const SUMMON_MONSTER_ID = 3_000_043;
const ATTRIBUTE_TRANSITION = new Map([
  [11_030, 513],
  [11_031, 513],
  [11_034, 750],
  [11_330, 346],
  [11_331, 346],
  [11_332, 298],
]);
const EFFECT_FAMILY_ATTRIBUTE_IDS = new Set([
  ...ATTRIBUTE_TRANSITION.keys(),
  11_802,
]);
const VOLATILE_CURRENT_HP_ATTRIBUTE_ID = 11_310;
const MAX_RETAINED_PAIR_EXAMPLES = 100;
const MAX_RETAINED_BLOCKING_EXAMPLES = 20;
const MINIMUM_REPEATS_PER_STATE = 2;
const MINIMUM_DISTINCT_DIRECT_SOURCE_INSTANCES_PER_STATE = 2;
const MINIMUM_DISTINCT_STAGE_SIGNATURES = 2;
const COEFFICIENT_STAGE_BOUNDARIES = [
  "floor",
  "ceil",
  "nearest_half_up",
  "unrounded_rational",
];
const QUALIFICATION_STAGES = [
  {
    id: "actor_shape_and_skill_component",
    added_equal_fields: [
      "source/direct-source/target numeric actor identities",
      "ability_id and hit_event_id",
    ],
  },
  {
    id: "same_session_and_run",
    added_equal_fields: ["session_id and run_ordinal"],
  },
  {
    id: "same_source_and_target_entities",
    added_equal_fields: [
      "source_entity_uuid and target_entity_uuid",
    ],
  },
  {
    id: "same_direct_source_entity_instance",
    added_equal_fields: ["direct_source_entity_uuid"],
  },
  {
    id: "same_damage_outcome_flags",
    added_equal_fields: ["critical, lucky, damage_source, and damage_type"],
  },
  {
    id: "same_packet_formula_context",
    added_equal_fields: [
      "owner id/level/stage, type flags, property, damage mode, normal/rainbow flags, hit parts, damage weight, and component count",
    ],
  },
  {
    id: "same_target_attribute_state",
    added_equal_fields: ["target_attribute_state_id"],
  },
  {
    id: "same_target_status_state",
    added_equal_fields: ["target_status_state_id"],
  },
  {
    id: "same_source_attributes_outside_effect_family",
    added_equal_fields: ["source attributes outside the effect-2110140 family"],
  },
  {
    id: "same_source_statuses_outside_effect_family",
    added_equal_fields: ["source statuses outside effect 2110140 and its origin family"],
  },
  {
    id: "same_observed_actor_geometry",
    added_equal_fields: ["source/direct-source/target packet-observed positions"],
  },
  {
    id: "same_damage_packet_position",
    added_equal_fields: ["damage packet position"],
  },
];
const STRUCTURAL_STAGE_INDEX = QUALIFICATION_STAGES.findIndex(
  (stage) => stage.id === "same_target_status_state",
);
const STRICT_STAGE_INDEX = QUALIFICATION_STAGES.length - 1;

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "generate") generate(options);
else if (command === "verify") verify(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function generate(values) {
  const frontierPath = path.resolve(required(values, "frontier"));
  const cohortPath = path.resolve(required(values, "cohort"));
  const transitionProofPath = path.resolve(required(values, "transition-proof"));
  const outputPath = path.resolve(required(values, "output"));
  refuseExisting(outputPath);

  const frontier = readJson(frontierPath);
  const cohort = readJson(cohortPath);
  const transitionProof = readJson(transitionProofPath);
  validateInputs(frontier, cohort, transitionProof);
  const pairSearch = analyzePairs(
    cohort,
    frontier.identity.owner_entity_uuid,
    transitionProof,
  );
  const qualification = qualificationFunnel(
    cohort,
    frontier.identity.owner_entity_uuid,
  );
  const operatorAdjudication = deterministicOperatorAdjudication(
    cohort,
    frontier,
    frontier.identity.owner_entity_uuid,
    transitionProof,
  );

  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game_build: GAME_BUILD,
    identity: {
      ability_id: ABILITY_ID,
      support_effect_id: EFFECT_ID,
      summon_monster_id: SUMMON_MONSTER_ID,
      damage_script: "AutoAttack",
    },
    sources: {
      one_skill_operator_frontier: fileReceipt(frontierPath),
      formula_cohort: fileReceipt(cohortPath),
      primary_attack_transition_proof: fileReceipt(transitionProofPath),
    },
    policy: {
      exact_numeric_ids_and_build_are_authoritative: true,
      localized_names_are_runtime_keys: false,
      remote_player_cast_packets_required: false,
      current_character_snapshot_substitution_allowed: false,
      unresolved_pairs_preserved: true,
      candidate_boundaries_are_formula_authority: false,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
    },
    exact_transition_contract: {
      scope: "each new session/provider/recipient lifecycle must match its own exact transition-proof window",
      proof_lifecycle_windows: transitionProof.lifecycle_windows.length,
      consumed_damage_attribute_id: 11_330,
      internal_attack_add_attribute_id: 11_332,
      historical_run_0004_example_only: {
        present_minus_absent_attribute_deltas: Object.fromEntries(
          [...ATTRIBUTE_TRANSITION].map(([attributeId, delta]) => [String(attributeId), delta]),
        ),
        consumed_current_attack_delta: 346,
        internal_attack_add_delta: 298,
      },
      current_attack_delta_must_be_reproven_for_each_new_lifecycle: true,
      internal_attack_add_must_not_be_substituted_for_current_attack: true,
    },
    controlled_pair_contract: {
      required_equal_fields: [
        "same build, session, run, attributed source, target, ability, and exact hit_event_id",
        "same source/direct-source/target numeric actor identities, including direct summon monster_id 3000043",
        "same target attributes except volatile current HP 11310, and same target status state",
        "same source attributes outside the exact effect-family transition",
        "same source statuses outside effect 2110140 and exact origin-family rows",
        "same critical/lucky flags and retained packet damage-mode, stage, type, property, hit-part, and damage-weight fields",
        "same last packet-observed source/direct-source/target coordinates and damage packet position",
      ],
      required_changed_fields: [
        "effect 2110140 absent versus present with exactly one numeric provider",
        "present-minus-absent source attribute deltas equal the exact transition contract",
        "direct_source_entity_uuid varies as a replicated summon-trial identifier rather than an equality key",
      ],
      required_chronology:
        "at least one matching absent trial before activation, repeated present trials inside one exact lifecycle, and at least one matching absent trial after deactivation",
      local_component_index_is_formula_input: false,
      downstream_random_roll_observed: false,
      deterministic_repeat_or_authoritative_random_input_required: true,
      minimum_independent_qualifying_pairs: 2,
      minimum_repeats_per_absent_and_present_state: MINIMUM_REPEATS_PER_STATE,
      minimum_distinct_direct_source_instances_per_absent_and_present_state:
        MINIMUM_DISTINCT_DIRECT_SOURCE_INSTANCES_PER_STATE,
      minimum_distinct_coefficient_fixed_stage_signatures:
        MINIMUM_DISTINCT_STAGE_SIGNATURES,
      candidate_coefficient_stage_boundaries: Object.keys(
        frontier.autoattack_operator_frontier.tier0_rounding_discriminant
          .candidate_coefficient_stage_boundaries,
      ),
      adjudication:
        "Enumerate coefficient-stage and final integer boundaries on each qualifying pair, reject candidates that cannot share one exact downstream factor, and require replicated selection plus conservation. Compatibility alone is not proof that an unobserved random roll was shared.",
    },
    current_cohort_pair_search: pairSearch,
    controlled_pair_qualification_funnel: qualification,
    deterministic_operator_adjudication: operatorAdjudication,
    acquisition_recipe: [
      "Use the same locally observed recipient, stationary target, exact hit_event_id, and unchanged target/source context before, during, and after one exact effect-2110140 attribute lifecycle.",
      "Repeat each state across at least two numeric monster-3000043 summon instances. Preserve each direct_source_entity_uuid as trial identity, but do not require one runtime summon UUID to exist in both causal states.",
      "Retain the provider-owned status lifecycle and recipient attribute transition; do not wait for or synthesize unavailable remote-player cast packets.",
      "Record repeated absent and present hits so at least two independent pairs can select the same integer candidate despite an unobserved damage roll.",
      "Regenerate the schema-46 cohort and this worklist. Never reuse the run-0004 +346 current-Attack marginal for a different lifecycle without proving its exact transition.",
    ],
    conclusion: {
      current_exact_controlled_pairs_available:
        pairSearch.exact_transition_controlled_pairs,
      acquisition_ready: true,
      selected_coefficient_stage_boundary:
        operatorAdjudication.selected_coefficient_stage_boundary,
      exact_integer_rounding_proven:
        operatorAdjudication.exact_integer_rounding_proven,
      downstream_factor_cancellation_proven:
        operatorAdjudication.downstream_factor_cancellation_proven,
      exact_pair_counterfactual_marginal_proven:
        operatorAdjudication.exact_pair_counterfactual_marginal_proven,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
      blocker: operatorAdjudication.exact_integer_rounding_proven
        ? "The controlled sample selected an exact coefficient-stage boundary, but current-build protocol identity, canonical replay conservation, and protocol-event coverage remain separate mandatory runtime gates."
        : `The retained ${cohort.inputs.length}-RLOG owner cohort does not contain enough repeated, deterministic, exact-transition groups to select one coefficient-stage boundary. A controlled capture or authoritative server operator remains necessary.`,
    },
  };
  report.content_sha256 = contentHash(report);
  verifyReport(report);
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  fs.writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
  process.stdout.write(`${JSON.stringify(summary(report), null, 2)}\n`);
}

function validateInputs(frontier, cohort, transitionProof) {
  assert.ok(
    [11, 12, 13].includes(Number(frontier.schema_version)),
    "unsupported one-skill frontier schema",
  );
  assert.equal(frontier.generated_by, "tools/bpsr-autoattack-single-skill-trace.mjs");
  assert.equal(String(frontier.game_build), GAME_BUILD);
  assert.equal(frontier.identity.ability_id, ABILITY_ID);
  assert.equal(frontier.identity.support_effect_id, EFFECT_ID);
  assert.equal(frontier.conclusion.exact_integer_rounding_proven, false);
  assert.equal(frontier.conclusion.promotion_decision, "fail_closed_no_provider_rdps_credit");
  assert.equal(
    frontier.autoattack_operator_frontier.tier0_rounding_discriminant
      .exact_same_packet_attribute_transition.current_physical_attack_delta,
    346,
  );
  assert.equal(
    frontier.autoattack_operator_frontier.tier0_rounding_discriminant
      .exact_same_packet_attribute_transition.internal_attack_add_delta,
    298,
  );

  assert.equal(cohort.schema_version, 46);
  assert.equal(String(cohort.game_build), GAME_BUILD);
  assert.ok(Array.isArray(cohort.samples) && cohort.samples.length > 0);
  assert.equal(
    Number(cohort.selection?.ability_id ?? cohort.selection?.ability_ids?.[0]),
    ABILITY_ID,
  );

  assert.equal(transitionProof.schema_version, 1);
  assert.equal(String(transitionProof.game_build), GAME_BUILD);
  assert.equal(Number(transitionProof.effect_id), EFFECT_ID);
  assert.ok(
    transitionProof.lifecycle_windows.length > 0,
    "transition proof contains no exact lifecycle windows",
  );
  for (const window of transitionProof.lifecycle_windows) validateTransitionWindow(window);
}

function analyzePairs(cohort, ownerEntityUuid, transitionProof = null) {
  const samples = cohort.samples.filter(
    (sample) =>
      Number(sample.ability_id) === ABILITY_ID &&
      String(sample.source_entity_uuid) === String(ownerEntityUuid) &&
      Number(sample.direct_source_actor_identity?.monster_id) === SUMMON_MONSTER_ID,
  );
  const present = samples.filter((sample) => effectProviders(cohort, sample).length > 0);
  const absent = samples.length - present.length;
  const structural = groupPairCandidates(cohort, samples, false, transitionProof);
  const strict = groupPairCandidates(cohort, samples, true, transitionProof);
  return {
    selected_samples: samples.length,
    effect_present_samples: present.length,
    effect_absent_samples: absent,
    structural_same_target_hit_groups: structural.groups,
    structural_same_target_hit_pairs: structural.pairs,
    exact_transition_controlled_groups: strict.groups,
    exact_transition_controlled_pairs: strict.exactTransitionPairs,
    rejected_strict_pairs_with_wrong_attribute_delta:
      strict.pairs - strict.exactTransitionPairs,
    retained_pair_examples: strict.examples,
    retained_pair_examples_truncated:
      strict.exactTransitionPairs > strict.examples.length,
  };
}

function qualificationFunnel(cohort, ownerEntityUuid) {
  const samples = cohort.samples.filter(
    (sample) =>
      Number(sample.ability_id) === ABILITY_ID &&
      String(sample.source_entity_uuid) === String(ownerEntityUuid) &&
      Number(sample.direct_source_actor_identity?.monster_id) === SUMMON_MONSTER_ID,
  );
  const exactInstanceFunnel = qualificationStages(cohort, samples, false, new Set());
  const independentTrialFunnel = qualificationStages(cohort, samples, true, new Set());
  const hpNormalizedTrialFunnel = qualificationStages(
    cohort,
    samples,
    true,
    new Set([VOLATILE_CURRENT_HP_ATTRIBUTE_ID]),
  );
  return {
    authority: "diagnostic_only_no_proof_rules_relaxed",
    selected_samples: samples.length,
    exact_direct_source_instance: exactInstanceFunnel,
    independent_direct_source_trials: {
      ...independentTrialFunnel,
      authority:
        "candidate_diagnostic_only_direct_source_uuid_not_yet_proven_formula_irrelevant",
      omitted_equal_field: "direct_source_entity_uuid",
      retained_equal_identity:
        "direct_source_actor_identity including numeric summon monster_id 3000043",
    },
    independent_direct_source_trials_current_hp_normalized: {
      ...hpNormalizedTrialFunnel,
      authority:
        "candidate_diagnostic_only_direct_source_uuid_not_proven_formula_irrelevant",
      omitted_equal_fields: [
        "direct_source_entity_uuid",
        "target current HP attribute 11310",
      ],
      volatile_attribute_authority:
        "plugins/games/blue-protocol-star-resonance/src/decoder.rs ATTR_CURRENT_HP and damage-attr-coefficient-proof.rs is_volatile_attribute",
    },
    note:
      "These funnels explain where retained absent/present samples stop matching. The independent-trial view does not prove that changing direct-source runtime UUID is formula-irrelevant and therefore cannot authorize attribution.",
  };
}

function qualificationStages(
  cohort,
  samples,
  omitDirectSourceInstance,
  omittedTargetAttributeIds,
) {
  let previousPairs = null;
  const stages = QUALIFICATION_STAGES.map((stage, stageIndex) => {
    const grouped = summarizeStateGroups(
      cohort,
      samples,
      (sample) => controlledKeyForStage(
        cohort,
        sample,
        stageIndex,
        omitDirectSourceInstance,
        omittedTargetAttributeIds,
      ),
    );
    const result = {
      stage: stageIndex + 1,
      id: stage.id,
      added_equal_fields: stage.added_equal_fields,
      groups_total: grouped.groupsTotal,
      groups_effect_present_only: grouped.presentOnlyGroups,
      groups_effect_absent_only: grouped.absentOnlyGroups,
      groups_with_both_states: grouped.bothStateGroups,
      cross_state_pairs: grouped.crossStatePairs,
      cross_state_pairs_removed_by_this_stage:
        previousPairs === null ? 0 : previousPairs - grouped.crossStatePairs,
    };
    previousPairs = grouped.crossStatePairs;
    return result;
  });
  const firstZeroIndex = stages.findIndex((stage) => stage.cross_state_pairs === 0);
  const lastNonzero = [...stages].reverse().find((stage) => stage.cross_state_pairs > 0);
  return {
    stages,
    last_stage_with_cross_state_pairs: lastNonzero?.id ?? null,
    first_stage_with_no_cross_state_pairs:
      firstZeroIndex >= 0 ? stages[firstZeroIndex].id : null,
    blocking_equal_fields:
      firstZeroIndex >= 0 ? stages[firstZeroIndex].added_equal_fields : [],
    retained_blocking_pair_examples:
      firstZeroIndex > 0
        ? eliminatedPairExamples(
          cohort,
          samples,
          firstZeroIndex,
          omitDirectSourceInstance,
          omittedTargetAttributeIds,
        )
        : [],
    fully_controlled_cross_state_pairs:
      stages.at(-1)?.cross_state_pairs ?? 0,
    capture_qualifies_for_downstream_adjudication:
      (stages.at(-1)?.cross_state_pairs ?? 0) > 0,
    note:
      "This funnel only explains where retained absent/present samples stop matching. It does not relax exact-build, lifecycle, repeatability, transition, rounding, or conservation requirements.",
  };
}

function eliminatedPairExamples(
  cohort,
  samples,
  blockingStageIndex,
  omitDirectSourceInstance,
  omittedTargetAttributeIds,
) {
  const previousGroups = new Map();
  for (const sample of samples) {
    const key = controlledKeyForStage(
      cohort,
      sample,
      blockingStageIndex - 1,
      omitDirectSourceInstance,
      omittedTargetAttributeIds,
    );
    const group = previousGroups.get(key) ?? { present: [], absent: [] };
    (effectProviders(cohort, sample).length > 0 ? group.present : group.absent)
      .push(sample);
    previousGroups.set(key, group);
  }
  const examples = [];
  for (const group of previousGroups.values()) {
    for (const present of group.present) {
      for (const absent of group.absent) {
        const presentKey = controlledKeyForStage(
          cohort,
          present,
          blockingStageIndex,
          omitDirectSourceInstance,
          omittedTargetAttributeIds,
        );
        const absentKey = controlledKeyForStage(
          cohort,
          absent,
          blockingStageIndex,
          omitDirectSourceInstance,
          omittedTargetAttributeIds,
        );
        if (presentKey === absentKey) continue;
        examples.push({
          session_id: present.session_id,
          run_ordinal: Number(present.run_ordinal),
          source_entity_uuid: String(present.source_entity_uuid),
          target_entity_uuid: String(present.target_entity_uuid),
          ability_id: Number(present.ability_id),
          hit_event_id: Number(present.hit_event_id),
          effect_absent: compactBlockingSample(absent),
          effect_present: compactBlockingSample(present),
          changed_fields: blockingStageDifference(
            cohort,
            absent,
            present,
            blockingStageIndex,
            omittedTargetAttributeIds,
          ),
          formula_authority: false,
        });
        if (examples.length >= MAX_RETAINED_BLOCKING_EXAMPLES) return examples;
      }
    }
  }
  return examples;
}

function compactBlockingSample(sample) {
  return {
    sequence: Number(sample.sequence),
    amount: Number(sample.amount),
    direct_source_entity_uuid: String(sample.direct_source_entity_uuid),
    source_attribute_state_id: Number(sample.source_attribute_state_id),
    source_status_state_id: Number(sample.source_status_state_id),
    target_attribute_state_id: Number(sample.target_attribute_state_id),
    target_status_state_id: Number(sample.target_status_state_id),
  };
}

function blockingStageDifference(
  cohort,
  absent,
  present,
  stageIndex,
  omittedTargetAttributeIds,
) {
  const stageId = QUALIFICATION_STAGES[stageIndex].id;
  if (stageId === "same_direct_source_entity_instance") {
    return {
      direct_source_entity_uuid: {
        absent: String(absent.direct_source_entity_uuid),
        present: String(present.direct_source_entity_uuid),
      },
    };
  }
  if (stageId === "same_target_attribute_state") {
    return {
      target_attributes: attributeStateDifference(
        cohort.attribute_states[absent.target_attribute_state_id],
        cohort.attribute_states[present.target_attribute_state_id],
        omittedTargetAttributeIds,
      ),
    };
  }
  if (stageId === "same_target_status_state") {
    return {
      target_status_state: {
        absent: cohort.status_states[absent.target_status_state_id],
        present: cohort.status_states[present.target_status_state_id],
      },
    };
  }
  return {
    stage: stageId,
    absent_key_fragment: controlledStageFragment(cohort, absent, stageIndex),
    present_key_fragment: controlledStageFragment(cohort, present, stageIndex),
  };
}

function attributeStateDifference(absentState, presentState, omittedAttributeIds = new Set()) {
  const absent = new Map(absentState.map((row) => [Number(row.attribute_id), Number(row.value)]));
  const present = new Map(presentState.map((row) => [Number(row.attribute_id), Number(row.value)]));
  return [...new Set([...absent.keys(), ...present.keys()])]
    .sort((left, right) => left - right)
    .filter((attributeId) => !omittedAttributeIds.has(attributeId))
    .filter((attributeId) => absent.get(attributeId) !== present.get(attributeId))
    .map((attributeId) => ({
      attribute_id: attributeId,
      absent: absent.get(attributeId) ?? null,
      present: present.get(attributeId) ?? null,
      present_minus_absent:
        (present.get(attributeId) ?? 0) - (absent.get(attributeId) ?? 0),
    }));
}

function controlledStageFragment(cohort, sample, stageIndex) {
  const packet = sample.packet ?? {};
  switch (QUALIFICATION_STAGES[stageIndex].id) {
    case "same_session_and_run":
      return { session_id: sample.session_id, run_ordinal: Number(sample.run_ordinal) };
    case "same_source_and_target_entities":
      return {
        source_entity_uuid: String(sample.source_entity_uuid),
        target_entity_uuid: String(sample.target_entity_uuid),
      };
    case "same_damage_outcome_flags":
      return {
        critical: sample.critical,
        lucky: sample.lucky,
        damage_source: sample.damage_source,
        damage_type: sample.damage_type,
      };
    case "same_packet_formula_context":
      return {
        owner_id: packet.owner_id,
        owner_level: packet.owner_level,
        owner_stage: packet.owner_stage,
        type_flags: packet.type_flags,
        property: packet.property,
        damage_mode: packet.damage_mode,
        normal_hit: packet.normal_hit,
        rainbow: packet.rainbow,
        hit_parts: packet.hit_parts,
        damage_weight: packet.damage_weight,
        skill_effect_component_count: packet.skill_effect_component_count,
      };
    case "same_source_attributes_outside_effect_family":
      return stableSourceAttributes(cohort, sample);
    case "same_source_statuses_outside_effect_family":
      return stableSourceStatuses(cohort, sample);
    case "same_observed_actor_geometry":
      return {
        source: sample.source_position_at_wire_message_start,
        direct_source: sample.direct_source_position_at_wire_message_start,
        target: sample.target_position_at_wire_message_start,
      };
    case "same_damage_packet_position":
      return packet.position;
    default:
      return null;
  }
}

function summarizeStateGroups(cohort, samples, keyForSample) {
  const groups = new Map();
  for (const sample of samples) {
    const key = keyForSample(sample);
    const group = groups.get(key) ?? { present: 0, absent: 0 };
    if (effectProviders(cohort, sample).length > 0) group.present += 1;
    else group.absent += 1;
    groups.set(key, group);
  }
  let presentOnlyGroups = 0;
  let absentOnlyGroups = 0;
  let bothStateGroups = 0;
  let crossStatePairs = 0;
  for (const group of groups.values()) {
    if (group.present > 0 && group.absent > 0) {
      bothStateGroups += 1;
      crossStatePairs += group.present * group.absent;
    } else if (group.present > 0) presentOnlyGroups += 1;
    else absentOnlyGroups += 1;
  }
  return {
    groupsTotal: groups.size,
    presentOnlyGroups,
    absentOnlyGroups,
    bothStateGroups,
    crossStatePairs,
  };
}

function groupPairCandidates(cohort, samples, strict, transitionProof) {
  const groups = new Map();
  for (const sample of samples) {
    const key = controlledKey(cohort, sample, strict);
    const group = groups.get(key) ?? { present: [], absent: [] };
    (effectProviders(cohort, sample).length > 0 ? group.present : group.absent)
      .push(sample);
    groups.set(key, group);
  }
  let comparableGroups = 0;
  let pairs = 0;
  let exactTransitionPairs = 0;
  const examples = [];
  for (const group of groups.values()) {
    if (group.present.length === 0 || group.absent.length === 0) continue;
    comparableGroups += 1;
    pairs += group.present.length * group.absent.length;
    if (!strict) continue;
    for (const present of group.present) {
      for (const absent of group.absent) {
        const providers = effectProviders(cohort, present);
        if (providers.length !== 1) continue;
        const contract = transitionProof
          ? transitionContractForSamples(transitionProof, [present], providers[0])
          : null;
        const expectedDeltas = contract?.attribute_deltas ?? ATTRIBUTE_TRANSITION;
        const deltas = attributeDeltas(
          cohort,
          present,
          absent,
          [...expectedDeltas.keys()],
        );
        if (!exactTransitionDeltas(deltas, expectedDeltas)) continue;
        exactTransitionPairs += 1;
        if (examples.length < MAX_RETAINED_PAIR_EXAMPLES) {
          examples.push({
            session_id: present.session_id,
            provider_entity_uuid: providers[0],
            source_entity_uuid: String(present.source_entity_uuid),
            direct_source_entity_uuid: String(present.direct_source_entity_uuid),
            target_entity_uuid: String(present.target_entity_uuid),
            hit_event_id: Number(present.hit_event_id),
            absent_sequence: Number(absent.sequence),
            present_sequence: Number(present.sequence),
            absent_amount: Number(absent.amount),
            present_amount: Number(present.amount),
            present_minus_absent_attribute_deltas: Object.fromEntries(
              [...deltas].map(([attributeId, delta]) => [String(attributeId), delta]),
            ),
            transition_contract: contract ? {
              status_instance_id: contract.status_instance_id,
              first_exclusive_sequence: contract.first_exclusive_sequence,
              last_exclusive_sequence: contract.last_exclusive_sequence,
            } : null,
            formula_authority: false,
          });
        }
      }
    }
  }
  return {
    groups: comparableGroups,
    pairs,
    exactTransitionPairs,
    examples,
  };
}

function controlledKey(cohort, sample, strict) {
  return controlledKeyForStage(
    cohort,
    sample,
    strict ? STRICT_STAGE_INDEX : STRUCTURAL_STAGE_INDEX,
    true,
    new Set([VOLATILE_CURRENT_HP_ATTRIBUTE_ID]),
  );
}

function controlledKeyForStage(
  cohort,
  sample,
  stageIndex,
  omitDirectSourceInstance = false,
  omittedTargetAttributeIds = new Set(),
) {
  const packet = sample.packet ?? {};
  const key = {
    source_actor_identity: sample.source_actor_identity,
    direct_source_actor_identity: sample.direct_source_actor_identity,
    target_actor_identity: sample.target_actor_identity,
    ability_id: Number(sample.ability_id),
    hit_event_id: Number(sample.hit_event_id),
  };
  if (stageIndex >= 1) {
    key.session_id = sample.session_id;
    key.run_ordinal = Number(sample.run_ordinal);
  }
  if (stageIndex >= 2) {
    key.source_entity_uuid = String(sample.source_entity_uuid);
    key.target_entity_uuid = String(sample.target_entity_uuid);
  }
  if (stageIndex >= 3) {
    if (!omitDirectSourceInstance) {
      key.direct_source_entity_uuid = String(sample.direct_source_entity_uuid);
    }
  }
  if (stageIndex >= 4) {
    key.critical = sample.critical;
    key.lucky = sample.lucky;
    key.damage_source = sample.damage_source;
    key.damage_type = sample.damage_type;
  }
  if (stageIndex >= 5) {
    key.packet = {
      owner_id: packet.owner_id,
      owner_level: packet.owner_level,
      owner_stage: packet.owner_stage,
      type_flags: packet.type_flags,
      property: packet.property,
      damage_mode: packet.damage_mode,
      normal_hit: packet.normal_hit,
      rainbow: packet.rainbow,
      hit_parts: packet.hit_parts,
      damage_weight: packet.damage_weight,
      skill_effect_component_count: packet.skill_effect_component_count,
    };
  }
  if (stageIndex >= 6) {
    if (omittedTargetAttributeIds.size === 0) {
      key.target_attribute_state_id = Number(sample.target_attribute_state_id);
    } else {
      key.target_attributes = cohort.attribute_states[sample.target_attribute_state_id]
        .filter((row) => !omittedTargetAttributeIds.has(Number(row.attribute_id)));
    }
  }
  if (stageIndex >= 7) {
    key.target_status_state_id = Number(sample.target_status_state_id);
  }
  if (stageIndex >= 8) {
    key.source_attributes_outside_effect_family = stableSourceAttributes(cohort, sample);
  }
  if (stageIndex >= 9) {
    key.source_statuses_outside_effect_family = stableSourceStatuses(cohort, sample);
  }
  if (stageIndex >= 10) {
    key.source_position_at_wire_message_start = sample.source_position_at_wire_message_start;
    key.direct_source_position_at_wire_message_start =
      sample.direct_source_position_at_wire_message_start;
    key.target_position_at_wire_message_start = sample.target_position_at_wire_message_start;
  }
  if (stageIndex >= 11) {
    key.damage_packet_position = packet.position;
  }
  return stableStringify(key);
}

function stableSourceAttributes(cohort, sample) {
  return cohort.attribute_states[sample.source_attribute_state_id]
    .filter((attribute) => !EFFECT_FAMILY_ATTRIBUTE_IDS.has(Number(attribute.attribute_id)))
    .map((attribute) => [Number(attribute.attribute_id), Number(attribute.value)]);
}

function stableSourceStatuses(cohort, sample) {
  return cohort.status_states[sample.source_status_state_id]
    .filter(
      (effect) =>
        Number(effect.effect_id) !== EFFECT_ID &&
        Number(effect.origin_source_config_id) !== EFFECT_ID,
    )
    .map((effect) => [
      Number(effect.effect_id),
      String(effect.source_entity_uuid),
      Number(effect.stacks),
      Number(effect.level),
      effect.origin_source_type_id,
      effect.origin_source_config_id,
    ]);
}

function effectProviders(cohort, sample) {
  return [...new Set(
    cohort.status_states[sample.source_status_state_id]
      .filter((effect) => Number(effect.effect_id) === EFFECT_ID)
      .map((effect) => String(effect.source_entity_uuid)),
  )].sort();
}

function attributeDeltas(
  cohort,
  present,
  absent,
  attributeIds = [...ATTRIBUTE_TRANSITION.keys()],
) {
  const presentAttributes = new Map(
    cohort.attribute_states[present.source_attribute_state_id]
      .map((attribute) => [Number(attribute.attribute_id), Number(attribute.value)]),
  );
  const absentAttributes = new Map(
    cohort.attribute_states[absent.source_attribute_state_id]
      .map((attribute) => [Number(attribute.attribute_id), Number(attribute.value)]),
  );
  return new Map(attributeIds.map((attributeId) => [
    attributeId,
    presentAttributes.get(attributeId) - absentAttributes.get(attributeId),
  ]));
}

function exactTransitionDeltas(deltas, expected = ATTRIBUTE_TRANSITION) {
  return [...expected].every(
    ([attributeId, expected]) => deltas.get(attributeId) === expected,
  );
}

function deterministicOperatorAdjudication(
  cohort,
  frontier,
  ownerEntityUuid,
  transitionProof,
) {
  const rowsByHit = exactAbilityRowsByHit(frontier);
  const samples = cohort.samples.filter(
    (sample) =>
      Number(sample.ability_id) === ABILITY_ID &&
      String(sample.source_entity_uuid) === String(ownerEntityUuid) &&
      Number(sample.direct_source_actor_identity?.monster_id) === SUMMON_MONSTER_ID,
  );
  const groups = new Map();
  for (const sample of samples) {
    const key = controlledKey(cohort, sample, true);
    const group = groups.get(key) ?? { present: [], absent: [] };
    (effectProviders(cohort, sample).length > 0 ? group.present : group.absent)
      .push(sample);
    groups.set(key, group);
  }

  let groupsWithBothStates = 0;
  let groupsRejectedForRepeatCount = 0;
  let groupsRejectedForNondeterministicDamage = 0;
  let groupsRejectedForOwnership = 0;
  let groupsRejectedForTransition = 0;
  let groupsRejectedForDirectSourceReplication = 0;
  let groupsRejectedForChronologicalAba = 0;
  let groupsRejectedForMissingStaticRow = 0;
  const qualifyingGroups = [];
  for (const group of groups.values()) {
    if (group.present.length === 0 || group.absent.length === 0) continue;
    groupsWithBothStates += 1;
    if (
      group.present.length < MINIMUM_REPEATS_PER_STATE ||
      group.absent.length < MINIMUM_REPEATS_PER_STATE
    ) {
      groupsRejectedForRepeatCount += 1;
      continue;
    }
    const presentAmounts = uniqueNumbers(group.present.map((sample) => sample.amount));
    const absentAmounts = uniqueNumbers(group.absent.map((sample) => sample.amount));
    if (presentAmounts.length !== 1 || absentAmounts.length !== 1) {
      groupsRejectedForNondeterministicDamage += 1;
      continue;
    }
    const providers = uniqueStrings(group.present.flatMap((sample) => effectProviders(cohort, sample)));
    if (
      providers.length !== 1 ||
      group.present.some((sample) => effectProviders(cohort, sample).length !== 1) ||
      group.absent.some((sample) => effectProviders(cohort, sample).length !== 0)
    ) {
      groupsRejectedForOwnership += 1;
      continue;
    }
    const contract = transitionContractForSamples(
      transitionProof,
      group.present,
      providers[0],
    );
    if (!contract) {
      groupsRejectedForTransition += 1;
      continue;
    }
    const absentBefore = group.absent.filter(
      (sample) => Number(sample.sequence) <= contract.first_exclusive_sequence,
    );
    const absentAfter = group.absent.filter(
      (sample) => Number(sample.sequence) >= contract.last_exclusive_sequence,
    );
    if (absentBefore.length === 0 || absentAfter.length === 0) {
      groupsRejectedForChronologicalAba += 1;
      continue;
    }
    const presentDirectInstances = uniqueStrings(
      group.present.map((sample) => sample.direct_source_entity_uuid),
    );
    const absentDirectInstances = uniqueStrings(
      group.absent.map((sample) => sample.direct_source_entity_uuid),
    );
    if (
      presentDirectInstances.length < MINIMUM_DISTINCT_DIRECT_SOURCE_INSTANCES_PER_STATE ||
      absentDirectInstances.length < MINIMUM_DISTINCT_DIRECT_SOURCE_INSTANCES_PER_STATE
    ) {
      groupsRejectedForDirectSourceReplication += 1;
      continue;
    }
    const present = group.present[0];
    const absent = group.absent[0];
    const deltas = attributeDeltas(
      cohort,
      present,
      absent,
      [...contract.attribute_deltas.keys()],
    );
    if (!exactTransitionDeltas(deltas, contract.attribute_deltas)) {
      groupsRejectedForTransition += 1;
      continue;
    }
    const row = rowsByHit.get(Number(present.hit_event_id));
    if (!row) {
      groupsRejectedForMissingStaticRow += 1;
      continue;
    }
    const presentAttack = attributeValue(cohort, present.source_attribute_state_id, 11_330);
    const absentAttack = attributeValue(cohort, absent.source_attribute_state_id, 11_330);
    const expectedCurrentAttackDelta = contract.attribute_deltas.get(11_330);
    if (
      !Number.isSafeInteger(expectedCurrentAttackDelta) ||
      expectedCurrentAttackDelta <= 0 ||
      presentAttack - absentAttack !== expectedCurrentAttackDelta
    ) {
      groupsRejectedForTransition += 1;
      continue;
    }
    const matchingBoundaries = COEFFICIENT_STAGE_BOUNDARIES.filter((mode) => {
      const activeBase = coefficientStageBase(
        presentAttack,
        row.coefficient_basis_points,
        row.fixed_parameter,
        mode,
      );
      const inactiveBase = coefficientStageBase(
        absentAttack,
        row.coefficient_basis_points,
        row.fixed_parameter,
        mode,
      );
      return exactRatioMatches(
        presentAmounts[0],
        absentAmounts[0],
        activeBase,
        inactiveBase,
      );
    });
    qualifyingGroups.push({
      session_id: present.session_id,
      provider_entity_uuid: providers[0],
      source_entity_uuid: String(present.source_entity_uuid),
      direct_source_entity_uuid: String(present.direct_source_entity_uuid),
      target_entity_uuid: String(present.target_entity_uuid),
      hit_event_id: Number(present.hit_event_id),
      coefficient_basis_points: row.coefficient_basis_points,
      fixed_parameter: row.fixed_parameter,
      absent_current_attack_11330: absentAttack,
      present_current_attack_11330: presentAttack,
      exact_lifecycle_transition: {
        status_instance_id: contract.status_instance_id,
        first_exclusive_sequence: contract.first_exclusive_sequence,
        last_exclusive_sequence: contract.last_exclusive_sequence,
        current_attack_delta_11330: expectedCurrentAttackDelta,
      },
      chronological_a_b_a: {
        absent_before_sequences: absentBefore.map((sample) => Number(sample.sequence)),
        present_sequences: group.present.map((sample) => Number(sample.sequence)),
        absent_after_sequences: absentAfter.map((sample) => Number(sample.sequence)),
      },
      independent_direct_source_instances: {
        absent: absentDirectInstances,
        present: presentDirectInstances,
      },
      absent_repeats: group.absent.length,
      present_repeats: group.present.length,
      absent_amount: absentAmounts[0],
      present_amount: presentAmounts[0],
      exact_ratio_matching_boundaries: matchingBoundaries,
    });
  }

  let commonBoundaries = [...COEFFICIENT_STAGE_BOUNDARIES];
  for (const group of qualifyingGroups) {
    commonBoundaries = commonBoundaries.filter((mode) =>
      group.exact_ratio_matching_boundaries.includes(mode));
  }
  const distinctStageSignatures = new Set(qualifyingGroups.map((group) =>
    `${group.coefficient_basis_points}:${group.fixed_parameter}`,
  ));
  const selectedBoundary =
    qualifyingGroups.length >= 2 &&
    distinctStageSignatures.size >= MINIMUM_DISTINCT_STAGE_SIGNATURES &&
    commonBoundaries.length === 1
      ? commonBoundaries[0]
      : null;
  const conservationExamples = selectedBoundary
    ? qualifyingGroups.map((group) => exactPairConservation(group, selectedBoundary))
    : [];
  const exactPairCounterfactual =
    selectedBoundary !== null &&
    conservationExamples.length === qualifyingGroups.length &&
    conservationExamples.every(
      (example) =>
        example.conserves_observed_damage_exactly &&
        example.provider_marginal_is_integer &&
        example.provider_marginal_equals_observed_present_minus_absent,
    );

  return {
    authority: "controlled_exact_build_behavioral_adjudication",
    candidate_boundaries: COEFFICIENT_STAGE_BOUNDARIES,
    minimum_repeats_per_state: MINIMUM_REPEATS_PER_STATE,
    minimum_distinct_direct_source_instances_per_state:
      MINIMUM_DISTINCT_DIRECT_SOURCE_INSTANCES_PER_STATE,
    minimum_distinct_stage_signatures: MINIMUM_DISTINCT_STAGE_SIGNATURES,
    groups_with_absent_and_present_states: groupsWithBothStates,
    groups_rejected_for_repeat_count: groupsRejectedForRepeatCount,
    groups_rejected_for_nondeterministic_damage: groupsRejectedForNondeterministicDamage,
    groups_rejected_for_provider_ownership: groupsRejectedForOwnership,
    groups_rejected_for_wrong_attribute_transition: groupsRejectedForTransition,
    groups_rejected_for_direct_source_instance_replication:
      groupsRejectedForDirectSourceReplication,
    groups_rejected_for_missing_chronological_a_b_a: groupsRejectedForChronologicalAba,
    groups_rejected_for_missing_static_row: groupsRejectedForMissingStaticRow,
    qualifying_deterministic_groups: qualifyingGroups.length,
    distinct_coefficient_fixed_stage_signatures: distinctStageSignatures.size,
    retained_qualifying_groups: qualifyingGroups.slice(0, MAX_RETAINED_PAIR_EXAMPLES),
    retained_qualifying_groups_truncated:
      qualifyingGroups.length > MAX_RETAINED_PAIR_EXAMPLES,
    common_exact_ratio_boundaries: commonBoundaries,
    selected_coefficient_stage_boundary: selectedBoundary,
    conservation_examples: conservationExamples.slice(0, MAX_RETAINED_PAIR_EXAMPLES),
    downstream_factor_cancellation_proven:
      selectedBoundary !== null && exactPairCounterfactual,
    exact_pair_counterfactual_marginal_proven: exactPairCounterfactual,
    exact_integer_rounding_proven:
      selectedBoundary !== null && exactPairCounterfactual,
    runtime_formula_authority: false,
    runtime_formula_authority_reason:
      "Selecting the coefficient-stage boundary does not close protocol-pack identity, full downstream stage order, stacking, or canonical replay conservation.",
  };
}

function validateTransitionWindow(window) {
  assert.equal(Number(window.effect_id), EFFECT_ID);
  assert.ok(window.session_id);
  assert.ok(Number.isSafeInteger(Number(window.run_ordinal)));
  assert.ok(String(window.provider_entity_uuid));
  assert.ok(String(window.affected_entity_uuid));
  assert.equal(window.activation?.join_candidate_count, 1);
  assert.equal(window.deactivation?.join_candidate_count, 1);
  assert.equal(
    String(window.activation?.transition?.actor_entity_uuid),
    String(window.affected_entity_uuid),
  );
  assert.ok(Array.isArray(
    window.activation?.retained_family_members?.other_same_packet_changes,
  ));
  const firstExclusive = Number(
    window.effective_stat_window?.first_exclusive_canonical_source_rlog_sequence,
  );
  const lastExclusive = Number(
    window.effective_stat_window?.last_exclusive_canonical_source_rlog_sequence,
  );
  assert.ok(Number.isSafeInteger(firstExclusive));
  assert.ok(Number.isSafeInteger(lastExclusive) && lastExclusive > firstExclusive);
  const changed = window.activation.transition.changed_members;
  assert.ok(Array.isArray(changed) && changed.length > 0);
  const deltas = new Map(changed.map((member) => [
    Number(member.attribute_id),
    Number(member.delta),
  ]));
  assert.ok(Number.isSafeInteger(deltas.get(11_330)) && deltas.get(11_330) > 0);
  assert.ok(Number.isSafeInteger(deltas.get(11_332)) && deltas.get(11_332) > 0);
}

function transitionContractForSamples(transitionProof, presentSamples, providerEntityUuid) {
  if (!transitionProof || presentSamples.length === 0) return null;
  const contracts = presentSamples.map((sample) => {
    const matches = transitionProof.lifecycle_windows.filter((window) => {
      const firstExclusive = Number(
        window.effective_stat_window?.first_exclusive_canonical_source_rlog_sequence,
      );
      const lastExclusive = Number(
        window.effective_stat_window?.last_exclusive_canonical_source_rlog_sequence,
      );
      return window.session_id === sample.session_id &&
        Number(window.run_ordinal) === Number(sample.run_ordinal) &&
        String(window.provider_entity_uuid) === String(providerEntityUuid) &&
        String(window.affected_entity_uuid) === String(sample.source_entity_uuid) &&
        Number(sample.sequence) > firstExclusive &&
        Number(sample.sequence) < lastExclusive;
    });
    if (matches.length !== 1) return null;
    const window = matches[0];
    try {
      validateTransitionWindow(window);
    } catch {
      return null;
    }
    if (
      window.activation.retained_family_members.other_same_packet_changes.length !== 0
    ) {
      return null;
    }
    return {
      session_id: window.session_id,
      run_ordinal: Number(window.run_ordinal),
      provider_entity_uuid: String(window.provider_entity_uuid),
      affected_entity_uuid: String(window.affected_entity_uuid),
      status_instance_id: Number(window.status_instance_id),
      first_exclusive_sequence: Number(
        window.effective_stat_window.first_exclusive_canonical_source_rlog_sequence,
      ),
      last_exclusive_sequence: Number(
        window.effective_stat_window.last_exclusive_canonical_source_rlog_sequence,
      ),
      attribute_deltas: new Map(
        window.activation.transition.changed_members.map((member) => [
          Number(member.attribute_id),
          Number(member.delta),
        ]),
      ),
    };
  });
  if (contracts.some((contract) => contract === null)) return null;
  const identities = new Set(contracts.map((contract) =>
    `${contract.session_id}:${contract.run_ordinal}:${contract.status_instance_id}:${contract.first_exclusive_sequence}:${contract.last_exclusive_sequence}`,
  ));
  return identities.size === 1 ? contracts[0] : null;
}

function exactAbilityRowsByHit(frontier) {
  const selected = frontier.autoattack_operator_frontier.exact_coefficient_selection
    .selected_coefficient_basis_points_by_hit_event_id;
  return new Map(
    frontier.autoattack_operator_frontier.static_family_relation.exact_ability_rows.map((row) => {
      const hitEventId = Number(row.hit_event_id);
      const coefficient = Number(selected[String(hitEventId)]);
      const fixed = Number(row.fixed_parameter_by_level?.[0] ?? 0);
      assert.ok(Number.isSafeInteger(coefficient) && coefficient >= 0);
      assert.ok(Number.isSafeInteger(fixed));
      return [hitEventId, {
        coefficient_basis_points: coefficient,
        fixed_parameter: fixed,
      }];
    }),
  );
}

function coefficientStageBase(attack, coefficient, fixed, mode) {
  assert.ok(Number.isSafeInteger(attack) && attack >= 0);
  assert.ok(Number.isSafeInteger(coefficient) && coefficient >= 0);
  assert.ok(Number.isSafeInteger(fixed));
  const product = BigInt(attack) * BigInt(coefficient);
  const scale = 10_000n;
  if (mode === "unrounded_rational") {
    return reduceFraction(product + BigInt(fixed) * scale, scale);
  }
  let coefficientTerm;
  if (mode === "floor") coefficientTerm = product / scale;
  else if (mode === "ceil") coefficientTerm = (product + scale - 1n) / scale;
  else if (mode === "nearest_half_up") coefficientTerm = (product + scale / 2n) / scale;
  else throw new Error(`unsupported coefficient-stage boundary: ${mode}`);
  return { numerator: coefficientTerm + BigInt(fixed), denominator: 1n };
}

function exactRatioMatches(presentAmount, absentAmount, activeBase, inactiveBase) {
  if (presentAmount <= 0 || absentAmount <= 0) return false;
  return BigInt(presentAmount) * inactiveBase.numerator * activeBase.denominator ===
    BigInt(absentAmount) * activeBase.numerator * inactiveBase.denominator;
}

function exactPairConservation(group, mode) {
  const activeBase = coefficientStageBase(
    group.present_current_attack_11330,
    group.coefficient_basis_points,
    group.fixed_parameter,
    mode,
  );
  const inactiveBase = coefficientStageBase(
    group.absent_current_attack_11330,
    group.coefficient_basis_points,
    group.fixed_parameter,
    mode,
  );
  const activeCommon = activeBase.numerator * inactiveBase.denominator;
  const inactiveCommon = inactiveBase.numerator * activeBase.denominator;
  const marginalCommon = activeCommon - inactiveCommon;
  const provider = reduceFraction(
    BigInt(group.present_amount) * marginalCommon,
    activeCommon,
  );
  const recipientNumerator =
    BigInt(group.present_amount) * provider.denominator - provider.numerator;
  const observedDelta = BigInt(group.present_amount - group.absent_amount);
  return {
    hit_event_id: group.hit_event_id,
    provider_share_numerator: provider.numerator.toString(),
    provider_share_denominator: provider.denominator.toString(),
    recipient_share_numerator: recipientNumerator.toString(),
    recipient_share_denominator: provider.denominator.toString(),
    observed_damage: group.present_amount,
    conserves_observed_damage_exactly:
      provider.numerator + recipientNumerator ===
      BigInt(group.present_amount) * provider.denominator,
    provider_marginal_is_integer: provider.numerator % provider.denominator === 0n,
    provider_marginal_equals_observed_present_minus_absent:
      provider.numerator === observedDelta * provider.denominator,
  };
}

function reduceFraction(numerator, denominator) {
  assert.ok(denominator > 0n);
  const divisor = greatestCommonDivisor(numerator, denominator);
  return { numerator: numerator / divisor, denominator: denominator / divisor };
}

function greatestCommonDivisor(left, right) {
  left = left < 0n ? -left : left;
  right = right < 0n ? -right : right;
  while (right !== 0n) [left, right] = [right, left % right];
  return left === 0n ? 1n : left;
}

function attributeValue(cohort, stateId, attributeId) {
  const value = cohort.attribute_states[stateId]?.find(
    (attribute) => Number(attribute.attribute_id) === attributeId,
  )?.value;
  assert.ok(Number.isSafeInteger(Number(value)));
  return Number(value);
}

function uniqueNumbers(values) {
  return [...new Set(values.map(Number))].sort((left, right) => left - right);
}

function uniqueStrings(values) {
  return [...new Set(values.map(String))].sort();
}

function verify(values) {
  const report = readJson(path.resolve(required(values, "input")));
  verifyReport(report);
  for (const source of Object.values(report.sources)) verifyFileReceipt(source);
  const cohort = readJson(report.sources.formula_cohort.path);
  const frontier = readJson(report.sources.one_skill_operator_frontier.path);
  const transitionProof = readJson(report.sources.primary_attack_transition_proof.path);
  assert.deepEqual(
    report.current_cohort_pair_search,
    analyzePairs(
      cohort,
      frontier.identity.owner_entity_uuid,
      report.schema_version >= 2 ? transitionProof : null,
    ),
  );
  if (report.schema_version >= 2) {
    assert.deepEqual(
      report.deterministic_operator_adjudication,
      deterministicOperatorAdjudication(
        cohort,
        frontier,
        frontier.identity.owner_entity_uuid,
        transitionProof,
      ),
    );
  }
  if (report.schema_version >= 3) {
    assert.deepEqual(
      report.controlled_pair_qualification_funnel,
      qualificationFunnel(cohort, frontier.identity.owner_entity_uuid),
    );
  }
  process.stdout.write(`${JSON.stringify(summary(report), null, 2)}\n`);
}

function verifyReport(report) {
  assert.ok(SUPPORTED_SCHEMA_VERSIONS.has(report.schema_version));
  assert.equal(report.generated_by, GENERATED_BY);
  assert.equal(report.game_build, GAME_BUILD);
  assert.equal(report.identity.ability_id, ABILITY_ID);
  assert.equal(report.policy.remote_player_cast_packets_required, false);
  assert.equal(report.policy.provider_rdps_credit_allowed, false);
  assert.equal(report.policy.runtime_promotion_allowed, false);
  assert.ok(report.current_cohort_pair_search.selected_samples > 0);
  assert.equal(
    report.current_cohort_pair_search.selected_samples,
    report.current_cohort_pair_search.effect_present_samples +
      report.current_cohort_pair_search.effect_absent_samples,
  );
  if (report.schema_version === 1) {
    assert.equal(report.current_cohort_pair_search.structural_same_target_hit_groups, 0);
    assert.equal(report.current_cohort_pair_search.structural_same_target_hit_pairs, 0);
    assert.equal(report.current_cohort_pair_search.exact_transition_controlled_pairs, 0);
  } else {
    assert.ok(report.deterministic_operator_adjudication);
    assert.equal(
      report.conclusion.exact_integer_rounding_proven,
      report.deterministic_operator_adjudication.exact_integer_rounding_proven,
    );
    assert.equal(
      report.conclusion.downstream_factor_cancellation_proven,
      report.deterministic_operator_adjudication.downstream_factor_cancellation_proven,
    );
  }
  if (report.schema_version >= 3) {
    const funnel = report.controlled_pair_qualification_funnel;
    assert.equal(funnel.authority, "diagnostic_only_no_proof_rules_relaxed");
    assert.equal(funnel.selected_samples, report.current_cohort_pair_search.selected_samples);
    assert.equal(funnel.exact_direct_source_instance.stages.length, QUALIFICATION_STAGES.length);
    assert.equal(
      funnel.independent_direct_source_trials_current_hp_normalized
        .fully_controlled_cross_state_pairs,
      report.current_cohort_pair_search.exact_transition_controlled_pairs +
        report.current_cohort_pair_search.rejected_strict_pairs_with_wrong_attribute_delta,
    );
    assert.equal(
      funnel.independent_direct_source_trials.omitted_equal_field,
      "direct_source_entity_uuid",
    );
    assert.deepEqual(
      funnel.independent_direct_source_trials_current_hp_normalized.omitted_equal_fields,
      ["direct_source_entity_uuid", "target current HP attribute 11310"],
    );
  }
  assert.equal(report.conclusion.acquisition_ready, true);
  assert.equal(report.conclusion.provider_rdps_credit_allowed, false);
  assert.equal(report.content_sha256, contentHash(report));
}

function summary(report) {
  return {
    game_build: report.game_build,
    ability_id: report.identity.ability_id,
    current_cohort_pair_search: report.current_cohort_pair_search,
    acquisition_ready: report.conclusion.acquisition_ready,
    exact_integer_rounding_proven: report.conclusion.exact_integer_rounding_proven,
    provider_rdps_credit_allowed: report.conclusion.provider_rdps_credit_allowed,
  };
}

function selfTest() {
  const absentAttributes = [...ATTRIBUTE_TRANSITION].map(([attributeId]) => ({
    attribute_id: attributeId,
    value: 1_000,
  }));
  const presentAttributes = absentAttributes.map((attribute) => ({
    ...attribute,
    value: attribute.value + ATTRIBUTE_TRANSITION.get(attribute.attribute_id),
  }));
  const base = {
    session_id: "s",
    run_ordinal: 1,
    sequence: 1,
    source_entity_uuid: 10,
    direct_source_entity_uuid: 20,
    target_entity_uuid: 30,
    source_actor_identity: { class_id: 11 },
    direct_source_actor_identity: { monster_id: SUMMON_MONSTER_ID },
    target_actor_identity: { monster_id: 1 },
    ability_id: ABILITY_ID,
    hit_event_id: 3,
    amount: 100,
    critical: false,
    lucky: false,
    damage_source: null,
    damage_type: null,
    source_attribute_state_id: 0,
    target_attribute_state_id: 2,
    source_status_state_id: 0,
    target_status_state_id: 2,
    source_position_at_wire_message_start: { x: 1, y: 2, z: 3 },
    direct_source_position_at_wire_message_start: { x: 1, y: 2, z: 3 },
    target_position_at_wire_message_start: { x: 4, y: 5, z: 6 },
    packet: {
      owner_id: ABILITY_ID,
      owner_level: 1,
      owner_stage: 1,
      type_flags: 1,
      property: null,
      damage_mode: 1,
      normal_hit: null,
      rainbow: null,
      hit_parts: [],
      damage_weight: { x: null, y: null, z: null },
      skill_effect_component_count: 1,
      position: { x: 1, y: 2, z: 3 },
    },
  };
  const present = structuredClone(base);
  present.sequence = 2;
  present.amount = 110;
  present.source_attribute_state_id = 1;
  present.source_status_state_id = 1;
  const cohort = {
    attribute_states: [absentAttributes, presentAttributes, []],
    status_states: [[], [{
      effect_id: EFFECT_ID,
      source_entity_uuid: 99,
      stacks: 1,
      level: 1,
      origin_source_type_id: null,
      origin_source_config_id: null,
    }], []],
    samples: [base, present],
  };
  const analysis = analyzePairs(cohort, "10");
  assert.equal(analysis.structural_same_target_hit_pairs, 1);
  assert.equal(analysis.exact_transition_controlled_pairs, 1);
  assert.equal(analysis.retained_pair_examples[0].provider_entity_uuid, "99");
  const funnel = qualificationFunnel(cohort, "10");
  assert.equal(funnel.exact_direct_source_instance.fully_controlled_cross_state_pairs, 1);
  assert.equal(
    funnel.exact_direct_source_instance.capture_qualifies_for_downstream_adjudication,
    true,
  );

  const frontier = {
    autoattack_operator_frontier: {
      exact_coefficient_selection: {
        selected_coefficient_basis_points_by_hit_event_id: {
          "3": 34_500,
          "7": 27_000,
        },
      },
      static_family_relation: {
        exact_ability_rows: [
          {
            hit_event_id: 3,
            fixed_parameter_by_level: [34],
          },
          {
            hit_event_id: 7,
            fixed_parameter_by_level: [27],
          },
        ],
      },
    },
  };
  const controlledBase = structuredClone(base);
  controlledBase.amount = 3_484;
  const controlledPresent = structuredClone(controlledBase);
  controlledPresent.source_attribute_state_id = 1;
  controlledPresent.source_status_state_id = 1;
  controlledPresent.amount = 4_677;
  const controlledHit7 = structuredClone(controlledBase);
  controlledHit7.hit_event_id = 7;
  controlledHit7.packet.owner_stage = 3;
  controlledHit7.amount = 2_727;
  const controlledHit7Present = structuredClone(controlledHit7);
  controlledHit7Present.source_attribute_state_id = 1;
  controlledHit7Present.source_status_state_id = 1;
  controlledHit7Present.amount = 3_661;
  const repeated = [
    controlledBase,
    structuredClone(controlledBase),
    controlledPresent,
    structuredClone(controlledPresent),
    controlledHit7,
    structuredClone(controlledHit7),
    controlledHit7Present,
    structuredClone(controlledHit7Present),
  ].map((sample, index) => ({
    ...sample,
    sequence: [1, 21, 12, 13, 2, 22, 14, 15][index],
    direct_source_entity_uuid: 20 + index,
    target_attribute_state_id: sample.source_status_state_id === 1 ? 3 : 2,
  }));
  const controlledCohort = {
    ...cohort,
    attribute_states: [
      absentAttributes,
      presentAttributes,
      [{ attribute_id: VOLATILE_CURRENT_HP_ATTRIBUTE_ID, value: 1_000_000 }],
      [{ attribute_id: VOLATILE_CURRENT_HP_ATTRIBUTE_ID, value: 900_000 }],
    ],
    samples: repeated,
  };
  const syntheticTransitionProof = {
    lifecycle_windows: [{
      session_id: "s",
      run_ordinal: 1,
      effect_id: EFFECT_ID,
      status_instance_id: 44,
      provider_entity_uuid: "99",
      affected_entity_uuid: "10",
      activation: {
        join_candidate_count: 1,
        transition: {
          actor_entity_uuid: "10",
          changed_members: [...ATTRIBUTE_TRANSITION].map(([attributeId, delta]) => ({
            attribute_id: attributeId,
            delta,
          })),
        },
        retained_family_members: { other_same_packet_changes: [] },
      },
      deactivation: { join_candidate_count: 1 },
      effective_stat_window: {
        first_exclusive_canonical_source_rlog_sequence: 10,
        last_exclusive_canonical_source_rlog_sequence: 20,
      },
    }],
  };
  const adjudication = deterministicOperatorAdjudication(
    controlledCohort,
    frontier,
    "10",
    syntheticTransitionProof,
  );
  assert.equal(adjudication.qualifying_deterministic_groups, 2);
  assert.deepEqual(adjudication.common_exact_ratio_boundaries, ["floor"]);
  assert.equal(adjudication.selected_coefficient_stage_boundary, "floor");
  assert.equal(adjudication.exact_integer_rounding_proven, true);
  assert.ok(adjudication.conservation_examples.every(
    (example) => example.provider_marginal_equals_observed_present_minus_absent,
  ));

  const ambiguous = deterministicOperatorAdjudication(
    { ...controlledCohort, samples: repeated.filter((sample) => sample.hit_event_id === 7) },
    frontier,
    "10",
    syntheticTransitionProof,
  );
  assert.equal(ambiguous.selected_coefficient_stage_boundary, null);
  assert.equal(ambiguous.exact_integer_rounding_proven, false);

  const ambiguousProviderCohort = structuredClone(controlledCohort);
  ambiguousProviderCohort.status_states[1].push({
    ...ambiguousProviderCohort.status_states[1][0],
    source_entity_uuid: 100,
  });
  const missingOwnership = deterministicOperatorAdjudication(
    ambiguousProviderCohort,
    frontier,
    "10",
    syntheticTransitionProof,
  );
  assert.equal(missingOwnership.qualifying_deterministic_groups, 0);
  assert.ok(missingOwnership.groups_rejected_for_provider_ownership > 0);

  const failedCounterfactual = exactPairConservation({
    hit_event_id: 3,
    coefficient_basis_points: 34_500,
    fixed_parameter: 34,
    absent_current_attack_11330: 1_000,
    present_current_attack_11330: 1_346,
    absent_amount: 3_484,
    present_amount: 4_678,
  }, "floor");
  assert.equal(failedCounterfactual.conserves_observed_damage_exactly, true);
  assert.equal(
    failedCounterfactual.provider_marginal_equals_observed_present_minus_absent,
    false,
  );
  process.stdout.write("self-test passed\n");
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
  if (fs.existsSync(filePath)) {
    throw new Error(`refusing to overwrite existing output: ${filePath}`);
  }
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
    "  node tools/bpsr-autoattack-rounding-acquisition.mjs generate --frontier FILE --cohort FILE --transition-proof FILE --output FILE\n" +
    "  node tools/bpsr-autoattack-rounding-acquisition.mjs verify --input FILE\n" +
    "  node tools/bpsr-autoattack-rounding-acquisition.mjs self-test\n",
  );
  process.exit(exitCode);
}
