#!/usr/bin/env node

import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 3;
const GENERATOR = "tools/bpsr-blade-sweep-counterfactual-discriminants.mjs";
const EFFECT_ID = 2110092;
const ABILITY_ID = 823225;
const HIT_EVENT_ID = 3;
const DAMAGE_SOURCE_ID = 2;
const DAMAGE_ATTR_ID = 282322503;
const PHYSICAL_DEFENSE_ATTRIBUTE_ID = 11350;
const SCALE = 10_000n;

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);
if (command === "analyze") analyze(options);
else if (command === "verify") verifyCommand(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function analyze(parsed) {
  const build = numericString(required(parsed, "build"), "build");
  const scalarPath = path.resolve(required(parsed, "scalar-proof"));
  const nearPairPath = path.resolve(required(parsed, "near-pair-proof"));
  const acquisitionPath = path.resolve(required(parsed, "acquisition-worklist"));
  const actorSceneCohortPath = path.resolve(required(parsed, "actor-scene-cohort"));
  const activationIndexPath = path.resolve(required(parsed, "damage-activation-index"));
  const formulaSurfacePath = path.resolve(required(parsed, "damage-formula-surface"));
  const routeProofPath = path.resolve(required(parsed, "damage-source-route-proof"));
  const output = path.resolve(required(parsed, "output"));
  const scalar = readJson(scalarPath, "Blade Sweep scalar proof");
  const nearPair = readJson(nearPairPath, "target-mitigation near-pair proof");
  const acquisition = readJson(acquisitionPath, "target-mitigation acquisition worklist");
  const actorSceneCohort = readJson(actorSceneCohortPath, "actor-scene formula cohort");
  const activationIndex = readJson(activationIndexPath, "damage activation index");
  const formulaSurface = readJson(formulaSurfacePath, "damage formula surface");
  const routeProof = readJson(routeProofPath, "damage-source route proof");
  validateScalar(scalar, build);
  validateNearPair(nearPair, build);
  validateAcquisition(acquisition, build);
  const observedCurve = validateActorSceneCohort(actorSceneCohort, build);
  const packetFormulaIdentity = validatePacketFormulaIdentity(
    activationIndex,
    formulaSurface,
    routeProof,
    build,
  );

  const penetrationBasisPoints = BigInt(
    scalar.summary.observed_runtime_armor_penetration_basis_points,
  );
  const curveConstant = BigInt(
    nearPair.exact_candidate_evaluation.transformed_curve_constant,
  );
  const sharedBases = nearPair.exact_candidate_evaluation
    .transformed_curve_unique_shared_base_values.map((value) => BigInt(value));
  const rawDefenses = [...new Set(nearPair.packet_near_pairs.flatMap((row) => [
    Number(row.left_raw_physical_defense),
    Number(row.right_raw_physical_defense),
  ]))].sort((left, right) => right - left);
  if (sharedBases.length !== 1 || sharedBases[0] !== 107006n ||
    JSON.stringify(rawDefenses) !== JSON.stringify([5907, 5370])) {
    throw new Error("near-pair candidate inputs no longer expose the expected exact discriminant points");
  }

  const rows = rawDefenses.map((rawDefense) => counterfactualRow(
    sharedBases[0],
    BigInt(rawDefense),
    curveConstant,
    penetrationBasisPoints,
  ));
  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATOR,
    game_build: build,
    effect_id: EFFECT_ID,
    status: "exact-candidate-discriminants-awaiting-controlled-packet-proof",
    policy: {
      exact_numeric_ids_build_and_integer_arithmetic_are_authoritative: true,
      static_armor_penetration_scalar_is_proven_but_its_damage_projection_is_not: true,
      status_confounded_shared_base_is_a_candidate_test_point_not_formula_proof: true,
      candidate_rounding_variants_are_enumerated_not_selected: true,
      exact_packet_component_and_static_coefficient_identity_are_proven: true,
      coefficient_identity_does_not_prove_defense_curve_or_formula_stage: true,
      three_point_integer_curve_compatibility_is_not_causal_formula_proof: true,
      same_input_status_invariance_is_context_bounded_not_global_formula_authority: true,
      ordinary_damage_and_candidate_redistribution_conserve_per_event: true,
      structurally_unobservable_remote_player_packets_are_not_required: true,
      localized_names_are_evidence_only: true,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    inputs: {
      scalar_proof: fileDescriptor(scalarPath),
      near_pair_proof: fileDescriptor(nearPairPath),
      acquisition_worklist: fileDescriptor(acquisitionPath),
      actor_scene_cohort: fileDescriptor(actorSceneCohortPath),
      damage_activation_index: fileDescriptor(activationIndexPath),
      damage_formula_surface: fileDescriptor(formulaSurfacePath),
      damage_source_route_proof: fileDescriptor(routeProofPath),
    },
    proven_inputs: {
      exact_effect_id: EFFECT_ID,
      observed_runtime_tier: 5,
      armor_penetration_basis_points: Number(penetrationBasisPoints),
      armor_penetration_percent: 6.5,
      provider_ownership_proven: true,
    },
    packet_formula_identity: packetFormulaIdentity,
    observed_baseline_curve: observedCurve,
    candidate_transform: {
      model: "floor(nonnegative_base * 22000 / (22000 + effective_target_physical_defense_raw))",
      defense_curve_constant: Number(curveConstant),
      hypothesized_penetration_stage:
        "effective defense = round(raw defense * (10000 - 650) / 10000) before the defense transform",
      hypothesis_proven: false,
      operation_order_proven: false,
      integer_rounding_proven: false,
    },
    exact_discriminant_rows: rows,
    acquisition_contract: {
      required_observation:
        "an otherwise identical same-build damage event with effect 2110092 absent and present, complete observable source and target state, and exact target defense snapshot",
      rejection_rule:
        "reject every candidate whose exact predicted packet amount disagrees with any deterministic controlled observation",
      rounding_discrimination:
        "the enumerated floor and ceil or half-up variants differ by three damage at both current exact test points",
      conservation_check:
        "for each retained candidate, recipient counterfactual damage plus provider candidate contribution must equal observed ordinary damage",
      remote_player_packet_dependency: false,
    },
    authority: {
      exact_damage_projection_proven: false,
      exact_operation_order_proven: false,
      exact_integer_rounding_proven: false,
      packet_conservation_proven: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    blockers: [
      "the 22000 defense transform is candidate-compatible but target-status-confounded",
      "exact packet component 282322503 and coefficient 25000 are bound, but the coefficient-to-base and defense-stage operation order remain unproven",
      "the 650-basis-point scalar is not yet proven to reduce raw defense before mitigation",
      "floor versus ceil or half-up effective-defense rounding is unproven",
      "no controlled effect-2110092 absent-versus-present damage projection exists",
      "canonical replay conservation is unproven",
    ],
  };
  report.content_sha256 = stableContentHash(report);
  verifyReport(report);
  mkdirSync(path.dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  const written = readJson(output, "written counterfactual discriminants");
  verifyReport(written);
  verifyInputs(written);
  console.log(JSON.stringify({ status: report.status, rows: report.exact_discriminant_rows }, null, 2));
}

function counterfactualRow(base, rawDefense, constant, scalarBasisPoints) {
  const baseline = defenseTransform(base, rawDefense, constant);
  const scaledNumerator = rawDefense * (SCALE - scalarBasisPoints);
  const variants = [
    ["floor-effective-defense", scaledNumerator / SCALE],
    ["ceil-effective-defense", ceilDiv(scaledNumerator, SCALE)],
    ["round-half-up-effective-defense", (scaledNumerator + SCALE / 2n) / SCALE],
  ].map(([rounding, effectiveDefense]) => {
    const observedWithEffect = defenseTransform(base, effectiveDefense, constant);
    const providerContribution = observedWithEffect - baseline;
    if (baseline + providerContribution !== observedWithEffect || providerContribution < 0n) {
      throw new Error("candidate redistribution does not conserve nonnegative ordinary damage");
    }
    return {
      rounding,
      effective_defense_raw: Number(effectiveDefense),
      predicted_observed_damage_with_effect: Number(observedWithEffect),
      recipient_counterfactual_damage_without_effect: Number(baseline),
      provider_candidate_contribution_damage: Number(providerContribution),
      provider_candidate_share_of_observed_damage: {
        numerator: Number(providerContribution),
        denominator: Number(observedWithEffect),
        parts_per_million_floor: Number(providerContribution * 1_000_000n / observedWithEffect),
      },
      conserved_ordinary_damage: Number(observedWithEffect),
    };
  });
  return {
    nonnegative_base: Number(base),
    target_physical_defense_raw_without_effect: Number(rawDefense),
    predicted_damage_without_effect: Number(baseline),
    variants,
    distinct_predicted_damage_with_effect: [...new Set(variants.map(
      (row) => row.predicted_observed_damage_with_effect,
    ))].sort((left, right) => left - right),
    formula_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function defenseTransform(base, defense, constant) {
  return base * constant / (constant + defense);
}
function ceilDiv(numerator, denominator) {
  return (numerator + denominator - 1n) / denominator;
}

function validateScalar(value, build) {
  if (Number(value?.schema_version) !== 24 ||
    value?.generated_by !== "tools/bpsr-blade-sweep-scalar-proof.mjs" ||
    String(value?.game_build) !== build || Number(value?.effect_id) !== EFFECT_ID ||
    value?.content_sha256 !== orderedContentHash(value) ||
    Number(value?.summary?.observed_runtime_tier) !== 5 ||
    Number(value?.summary?.observed_runtime_armor_penetration_basis_points) !== 650 ||
    Number(value?.summary?.observed_runtime_armor_penetration_percent) !== 6.5 ||
    value?.summary?.exact_provider_ownership_proven !== true ||
    value?.summary?.exact_damage_projection_proven !== false ||
    value?.summary?.formula_authority !== false ||
    value?.summary?.provider_rdps_credit_allowed !== false) {
    throw new Error("Blade Sweep scalar proof is not the exact schema-24 fail-closed frontier");
  }
}

function validateNearPair(value, build) {
  if (Number(value?.schema_version) !== 3 ||
    value?.generated_by !== "tools/bpsr-target-mitigation-near-pair-candidate-proof.mjs" ||
    String(value?.game_build) !== build || value?.content_sha256 !== stableContentHash(value) ||
    value?.status !== "exact-integer-candidate-compatible-status-confounded" ||
    Number(value?.exact_candidate_evaluation?.transformed_curve_constant) !== 22000 ||
    Number(value?.exact_candidate_evaluation?.transformed_curve_compatible_rows) !== 3 ||
    value?.exact_candidate_evaluation?.exact_target_mitigation_formula_proven !== false ||
    value?.confounders?.same_axis_status_invariance
      ?.target_status_can_change_damage_outside_raw_defense !== true ||
    value?.authority?.formula_authority !== false ||
    value?.authority?.provider_rdps_credit_allowed !== false) {
    throw new Error("near-pair proof is not the exact schema-3 status-confounded frontier");
  }
}

function validateAcquisition(value, build) {
  if (Number(value?.schema_version) !== 2 ||
    value?.generated_by !== "tools/bpsr-target-mitigation-acquisition-worklist.mjs" ||
    String(value?.game_build) !== build || Number(value?.effect_id) !== EFFECT_ID ||
    value?.content_sha256 !== stableContentHash(value) ||
    value?.status !== "acquisition-required-strict-controls-status-damage-relevance-observed" ||
    value?.policy?.structurally_unobservable_remote_player_packets_are_not_acquisition_requirements !== true ||
    value?.authority?.formula_authority !== false ||
    value?.authority?.provider_rdps_credit_allowed !== false) {
    throw new Error("acquisition worklist is not the exact schema-2 observable-evidence frontier");
  }
}

function validateActorSceneCohort(value, build) {
  if (Number(value?.schema_version) !== 41 ||
    value?.generated_by !== "rlogs-bpsr-state-scaling-damage-proof" ||
    String(value?.game_build) !== build || value?.policy?.formula_authority !== false ||
    !Array.isArray(value?.attribute_states) || !Array.isArray(value?.status_states) ||
    !Array.isArray(value?.samples) || value.samples.length !== 185 ||
    !Array.isArray(value?.inputs) || value.inputs.length !== 26) {
    throw new Error("actor-scene cohort is not the exact schema-41 diagnostic scope");
  }
  const abilityRows = value.samples.filter((sample) =>
    Number(sample.ability_id) === ABILITY_ID &&
    Number(sample.hit_event_id) === HIT_EVENT_ID &&
    Number(sample.damage_source) === DAMAGE_SOURCE_ID &&
    Number(sample.packet?.normal_value) === Number(sample.normal_value) &&
    Number(sample.amount) === Number(sample.normal_value));
  if (abilityRows.length !== 185) {
    throw new Error("actor-scene packet/component scope changed");
  }
  const defenseRows = abilityRows.flatMap((sample) => {
    const targetAttributes = value.attribute_states[Number(sample.target_attribute_state_id)];
    const physicalDefense = attributeValue(targetAttributes, PHYSICAL_DEFENSE_ATTRIBUTE_ID);
    if (physicalDefense === null) return [];
    const targetStatuses = value.status_states[Number(sample.target_status_state_id)];
    if (!Array.isArray(targetStatuses)) throw new Error("invalid target status-state reference");
    return [{
      rlog: String(sample.rlog),
      sequence: Number(sample.sequence),
      wire_capture_sequence: Number(sample.wire_capture_sequence),
      source_attribute_state_id: Number(sample.source_attribute_state_id),
      source_status_state_id: Number(sample.source_status_state_id),
      target_attribute_state_id: Number(sample.target_attribute_state_id),
      target_status_state_id: Number(sample.target_status_state_id),
      physical_defense_raw: physicalDefense,
      normal_value: Number(sample.normal_value),
      target_effect_ids: [...new Set(targetStatuses.map((status) => Number(status.effect_id)))]
        .sort((left, right) => left - right),
    }];
  });
  if (defenseRows.length !== 23 ||
    new Set(defenseRows.map((row) => row.source_attribute_state_id)).size !== 1 ||
    new Set(defenseRows.map((row) => row.source_status_state_id)).size !== 1) {
    throw new Error("actor-scene physical-defense witness scope changed");
  }
  const observedGroups = [...groupCounts(defenseRows, (row) =>
    `${row.physical_defense_raw}:${row.normal_value}`).entries()]
    .map(([key, count]) => {
      const [physicalDefenseRaw, normalValue] = key.split(":").map(Number);
      return { physical_defense_raw: physicalDefenseRaw, normal_value: normalValue, packet_rows: count };
    })
    .sort((left, right) => left.physical_defense_raw - right.physical_defense_raw ||
      left.normal_value - right.normal_value);
  const expectedGroups = [
    { physical_defense_raw: 5367, normal_value: 86020, packet_rows: 2 },
    { physical_defense_raw: 5370, normal_value: 59734, packet_rows: 1 },
    { physical_defense_raw: 5370, normal_value: 86011, packet_rows: 16 },
    { physical_defense_raw: 5907, normal_value: 84356, packet_rows: 4 },
  ];
  if (JSON.stringify(observedGroups) !== JSON.stringify(expectedGroups)) {
    throw new Error("actor-scene defense/output signatures changed");
  }
  const compatibleRows = defenseRows.filter((row) =>
    defenseTransform(107006n, BigInt(row.physical_defense_raw), 22000n) ===
      BigInt(row.normal_value));
  const statusConfoundedRows = defenseRows.filter((row) => !compatibleRows.includes(row));
  if (compatibleRows.length !== 22 || statusConfoundedRows.length !== 1 ||
    statusConfoundedRows[0].physical_defense_raw !== 5370 ||
    statusConfoundedRows[0].normal_value !== 59734) {
    throw new Error("actor-scene exact curve compatibility changed");
  }
  const selectedPoints = observedGroups.filter((row) => row.normal_value !== 59734);
  const compatibleEffectIds = [...new Set(compatibleRows.flatMap((row) => row.target_effect_ids))]
    .sort((left, right) => left - right);
  const commonCompatibleEffectIds = compatibleEffectIds.filter((effectId) =>
    compatibleRows.every((row) => row.target_effect_ids.includes(effectId)));
  const varyingCompatibleEffectIds = compatibleEffectIds.filter((effectId) =>
    !commonCompatibleEffectIds.includes(effectId));
  const sameInputGroups = [...groupRows(compatibleRows, (row) =>
    `${row.physical_defense_raw}:${row.normal_value}`).values()]
    .map((rows) => {
      const distinctStates = [...new Map(rows.map((row) => [
        row.target_status_state_id,
        row,
      ])).values()].sort((left, right) => left.target_status_state_id - right.target_status_state_id);
      const groupEffectIds = [...new Set(rows.flatMap((row) => row.target_effect_ids))]
        .sort((left, right) => left - right);
      const commonEffectIds = groupEffectIds.filter((effectId) =>
        rows.every((row) => row.target_effect_ids.includes(effectId)));
      const isolatedToggleReceipts = [];
      for (let left = 0; left < distinctStates.length; left += 1) {
        for (let right = left + 1; right < distinctStates.length; right += 1) {
          const symmetricDifference = symmetricDifferenceSorted(
            distinctStates[left].target_effect_ids,
            distinctStates[right].target_effect_ids,
          );
          if (symmetricDifference.length !== 1) continue;
          isolatedToggleReceipts.push({
            effect_id: symmetricDifference[0],
            left: eventReceipt(distinctStates[left]),
            right: eventReceipt(distinctStates[right]),
            exact_observed_damage_invariant_in_this_context: true,
            global_effect_irrelevance_proven: false,
          });
        }
      }
      return {
        physical_defense_raw: rows[0].physical_defense_raw,
        normal_value: rows[0].normal_value,
        packet_rows: rows.length,
        distinct_target_status_state_ids: distinctStates.length,
        common_effect_ids: commonEffectIds,
        varying_effect_ids: groupEffectIds.filter((effectId) => !commonEffectIds.includes(effectId)),
        isolated_single_effect_toggle_receipts: isolatedToggleReceipts,
      };
    })
    .sort((left, right) => left.physical_defense_raw - right.physical_defense_raw ||
      left.normal_value - right.normal_value);
  return {
    status: "three-distinct-defense-points-share-exact-integer-base-status-control-absent",
    ability_id: ABILITY_ID,
    hit_event_id: HIT_EVENT_ID,
    damage_source_id: DAMAGE_SOURCE_ID,
    target_attribute_id: PHYSICAL_DEFENSE_ATTRIBUTE_ID,
    packet_rows: abilityRows.length,
    target_physical_defense_rows: defenseRows.length,
    exact_curve_compatible_rows: compatibleRows.length,
    preserved_status_confounded_rows: statusConfoundedRows.length,
    selected_points: selectedPoints,
    selected_distinct_defense_values: selectedPoints.length,
    candidate_curve_constant: 22000,
    unique_shared_nonnegative_base: 107006,
    exact_integer_floor_compatibility_proven_for_selected_points: true,
    selected_points_share_exact_source_attribute_state: true,
    selected_points_share_exact_source_status_state: true,
    selected_points_share_exact_target_status_state: false,
    distinct_target_status_state_ids: new Set(defenseRows.map(
      (row) => row.target_status_state_id,
    )).size,
    same_input_status_invariance: {
      status: "observable-status-variation-bounded-common-confounders-remain",
      compatible_target_status_state_ids: new Set(compatibleRows.map(
        (row) => row.target_status_state_id,
      )).size,
      common_effect_ids_across_all_compatible_rows: commonCompatibleEffectIds,
      varying_effect_ids_across_all_compatible_rows: varyingCompatibleEffectIds,
      same_input_groups: sameInputGroups,
      isolated_single_effect_toggle_count: sameInputGroups.reduce((sum, row) =>
        sum + row.isolated_single_effect_toggle_receipts.length, 0),
      common_target_status_confounders_remain: true,
      target_status_control_proven: false,
      formula_authority: false,
      provider_rdps_credit_allowed: false,
    },
    incompatible_row_receipts: statusConfoundedRows,
    target_status_control_proven: false,
    exact_target_mitigation_formula_proven: false,
    exact_operation_order_and_integer_rounding_proven: false,
    formula_authority: false,
    runtime_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function validatePacketFormulaIdentity(activation, surface, route, build) {
  const activationAbility = activation?.observed_ability_result_kinds?.find(
    (row) => Number(row.ability_id) === ABILITY_ID,
  );
  const activationDamage = activation?.observed_damage_rows_by_id?.[String(DAMAGE_ATTR_ID)];
  if (Number(activation?.schema_version) !== 1 || String(activation?.game_build) !== build ||
    activation?.policy?.exact_packet_observation_index_only !== true ||
    activation?.policy?.static_identity_does_not_prove_transfer !== true ||
    Number(activationAbility?.packet_damage_results) !== 185 ||
    Number(activationAbility?.results_with_hit_event_id) !== 185 ||
    Number(activationDamage?.damage_id) !== DAMAGE_ATTR_ID ||
    Number(activationDamage?.type_enum) !== ABILITY_ID ||
    activationDamage?.damage_script !== "Attack" ||
    Number(activationDamage?.packet_damage_results) !== 185 ||
    Number(activationDamage?.packet_damage_value_shape?.amount_matches_normal_value) !== 185 ||
    JSON.stringify(activationDamage?.semantic_row?.PVEDamageRadio) !== JSON.stringify([25000])) {
    throw new Error("damage activation index does not bind the expected packet component");
  }
  const surfaceRow = surface?.rows?.[String(DAMAGE_ATTR_ID)];
  if (Number(surface?.schema_version) !== 1 || String(surface?.game_build) !== build ||
    surface?.policy?.exact_build_table_required !== true ||
    surface?.policy?.runtime_formula_authority !== false ||
    Number(surfaceRow?.damage_id) !== DAMAGE_ATTR_ID ||
    Number(surfaceRow?.linked_id) !== ABILITY_ID ||
    Number(surfaceRow?.hit_event_suffix_candidate) !== HIT_EVENT_ID ||
    JSON.stringify(surfaceRow?.int_array_pool_1_candidates_by_offset?.["28"]?.values) !==
      JSON.stringify([25000]) ||
    JSON.stringify(surfaceRow?.int_array_pool_1_candidates_by_offset?.["32"]?.values) !==
      JSON.stringify([])) {
    throw new Error("damage formula surface does not expose the expected exact-build row");
  }
  const routeKey = route?.keys?.find((row) => row.lookup_key === `${ABILITY_ID}:${HIT_EVENT_ID}`);
  const selectedRoute = routeKey?.selection_by_damage_source?.find(
    (row) => Number(row.damage_source_id) === DAMAGE_SOURCE_ID,
  );
  const candidate = routeKey?.candidates?.find(
    (row) => Number(row.damage_attr_id) === DAMAGE_ATTR_ID,
  );
  const exactRoute = candidate?.routes?.find((row) =>
    Number(row.damage_source_id) === DAMAGE_SOURCE_ID && row.owner_table === "BuffTable" &&
    Number(row.owner_id) === ABILITY_ID);
  if (Number(route?.schema_version) !== 9 ||
    route?.generated_by !== "rlogs-bpsr-damage-source-route-proof" ||
    String(route?.game_build) !== build || route?.policy?.exact_build_tables_required !== true ||
    route?.policy?.packet_damage_source_required !== true ||
    Number(selectedRoute?.damage_attr_id) !== DAMAGE_ATTR_ID || !exactRoute) {
    throw new Error("damage-source route proof does not select the expected exact-build row");
  }
  return {
    status: "exact-build-packet-occurrence-static-route-and-coefficient-bound",
    ability_id: ABILITY_ID,
    hit_event_id: HIT_EVENT_ID,
    packet_damage_source_id: DAMAGE_SOURCE_ID,
    damage_attr_id: DAMAGE_ATTR_ID,
    damage_script: "Attack",
    pve_damage_ratio_basis_points: [25000],
    fixed_parameter_by_level: [],
    packet_damage_results: 185,
    exact_packet_occurrence_proven: true,
    exact_static_damage_row_selection_proven: true,
    exact_coefficient_identity_proven: true,
    coefficient_to_pre_mitigation_base_formula_proven: false,
    defense_stage_operation_order_proven: false,
    formula_authority: false,
    runtime_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function attributeValue(attributes, id) {
  if (!Array.isArray(attributes)) return null;
  const row = attributes.find((entry) => Number(entry.attribute_id) === id);
  return row ? Number(row.value) : null;
}

function groupCounts(rows, key) {
  const counts = new Map();
  for (const row of rows) counts.set(key(row), (counts.get(key(row)) ?? 0) + 1);
  return counts;
}

function groupRows(rows, key) {
  const groups = new Map();
  for (const row of rows) {
    const groupKey = key(row);
    if (!groups.has(groupKey)) groups.set(groupKey, []);
    groups.get(groupKey).push(row);
  }
  return groups;
}

function symmetricDifferenceSorted(left, right) {
  return [
    ...left.filter((value) => !right.includes(value)),
    ...right.filter((value) => !left.includes(value)),
  ].sort((a, b) => a - b);
}

function eventReceipt(row) {
  return {
    rlog: row.rlog,
    sequence: row.sequence,
    wire_capture_sequence: row.wire_capture_sequence,
    target_status_state_id: row.target_status_state_id,
  };
}

function verifyCommand(parsed) {
  const input = path.resolve(required(parsed, "input"));
  const report = readJson(input, "counterfactual discriminants");
  verifyReport(report);
  verifyInputs(report);
  console.log(`verified ${input}`);
}

function verifyReport(report) {
  const rows = report?.exact_discriminant_rows;
  if (Number(report?.schema_version) !== SCHEMA_VERSION || report?.generated_by !== GENERATOR ||
    String(report?.game_build) !== "24687926" || Number(report?.effect_id) !== EFFECT_ID ||
    report?.content_sha256 !== stableContentHash(report) ||
    report?.status !== "exact-candidate-discriminants-awaiting-controlled-packet-proof" ||
    report?.policy?.candidate_rounding_variants_are_enumerated_not_selected !== true ||
    report?.policy?.exact_packet_component_and_static_coefficient_identity_are_proven !== true ||
    report?.policy?.coefficient_identity_does_not_prove_defense_curve_or_formula_stage !== true ||
    report?.policy?.three_point_integer_curve_compatibility_is_not_causal_formula_proof !== true ||
    report?.policy?.same_input_status_invariance_is_context_bounded_not_global_formula_authority !== true ||
    report?.policy?.ordinary_damage_and_candidate_redistribution_conserve_per_event !== true ||
    report?.policy?.structurally_unobservable_remote_player_packets_are_not_required !== true ||
    report?.policy?.formula_authority !== false || report?.policy?.runtime_authority !== false ||
    report?.policy?.ui_display_authority !== false ||
    report?.policy?.provider_rdps_credit_allowed !== false ||
    Number(report?.proven_inputs?.armor_penetration_basis_points) !== 650 ||
    report?.proven_inputs?.provider_ownership_proven !== true ||
    report?.packet_formula_identity?.status !==
      "exact-build-packet-occurrence-static-route-and-coefficient-bound" ||
    Number(report?.packet_formula_identity?.ability_id) !== ABILITY_ID ||
    Number(report?.packet_formula_identity?.hit_event_id) !== HIT_EVENT_ID ||
    Number(report?.packet_formula_identity?.packet_damage_source_id) !== DAMAGE_SOURCE_ID ||
    Number(report?.packet_formula_identity?.damage_attr_id) !== DAMAGE_ATTR_ID ||
    JSON.stringify(report?.packet_formula_identity?.pve_damage_ratio_basis_points) !==
      JSON.stringify([25000]) ||
    Number(report?.packet_formula_identity?.packet_damage_results) !== 185 ||
    report?.packet_formula_identity?.exact_packet_occurrence_proven !== true ||
    report?.packet_formula_identity?.exact_static_damage_row_selection_proven !== true ||
    report?.packet_formula_identity?.exact_coefficient_identity_proven !== true ||
    report?.packet_formula_identity?.coefficient_to_pre_mitigation_base_formula_proven !== false ||
    report?.packet_formula_identity?.defense_stage_operation_order_proven !== false ||
    report?.observed_baseline_curve?.status !==
      "three-distinct-defense-points-share-exact-integer-base-status-control-absent" ||
    Number(report?.observed_baseline_curve?.packet_rows) !== 185 ||
    Number(report?.observed_baseline_curve?.target_physical_defense_rows) !== 23 ||
    Number(report?.observed_baseline_curve?.exact_curve_compatible_rows) !== 22 ||
    Number(report?.observed_baseline_curve?.preserved_status_confounded_rows) !== 1 ||
    JSON.stringify(report?.observed_baseline_curve?.selected_points) !== JSON.stringify([
      { physical_defense_raw: 5367, normal_value: 86020, packet_rows: 2 },
      { physical_defense_raw: 5370, normal_value: 86011, packet_rows: 16 },
      { physical_defense_raw: 5907, normal_value: 84356, packet_rows: 4 },
    ]) ||
    Number(report?.observed_baseline_curve?.unique_shared_nonnegative_base) !== 107006 ||
    report?.observed_baseline_curve
      ?.exact_integer_floor_compatibility_proven_for_selected_points !== true ||
    report?.observed_baseline_curve?.selected_points_share_exact_target_status_state !== false ||
    Number(report?.observed_baseline_curve?.same_input_status_invariance
      ?.compatible_target_status_state_ids) !== 20 ||
    Number(report?.observed_baseline_curve?.same_input_status_invariance
      ?.common_effect_ids_across_all_compatible_rows?.length) !== 78 ||
    Number(report?.observed_baseline_curve?.same_input_status_invariance
      ?.varying_effect_ids_across_all_compatible_rows?.length) !== 36 ||
    JSON.stringify(report?.observed_baseline_curve?.same_input_status_invariance
      ?.same_input_groups?.map((row) => ({
        physical_defense_raw: row.physical_defense_raw,
        normal_value: row.normal_value,
        packet_rows: row.packet_rows,
        distinct_target_status_state_ids: row.distinct_target_status_state_ids,
        common_effect_ids: row.common_effect_ids.length,
        varying_effect_ids: row.varying_effect_ids.length,
      }))) !== JSON.stringify([
      { physical_defense_raw: 5367, normal_value: 86020, packet_rows: 2,
        distinct_target_status_state_ids: 1, common_effect_ids: 88, varying_effect_ids: 0 },
      { physical_defense_raw: 5370, normal_value: 86011, packet_rows: 16,
        distinct_target_status_state_ids: 15, common_effect_ids: 78, varying_effect_ids: 31 },
      { physical_defense_raw: 5907, normal_value: 84356, packet_rows: 4,
        distinct_target_status_state_ids: 4, common_effect_ids: 96, varying_effect_ids: 7 },
    ]) ||
    Number(report?.observed_baseline_curve?.same_input_status_invariance
      ?.isolated_single_effect_toggle_count) !== 1 ||
    Number(report?.observed_baseline_curve?.same_input_status_invariance
      ?.same_input_groups?.[1]?.isolated_single_effect_toggle_receipts?.[0]?.effect_id) !== 2203182 ||
    report?.observed_baseline_curve?.same_input_status_invariance
      ?.common_target_status_confounders_remain !== true ||
    report?.observed_baseline_curve?.same_input_status_invariance
      ?.target_status_control_proven !== false ||
    report?.observed_baseline_curve?.target_status_control_proven !== false ||
    report?.observed_baseline_curve?.exact_target_mitigation_formula_proven !== false ||
    Number(report?.candidate_transform?.defense_curve_constant) !== 22000 ||
    report?.candidate_transform?.hypothesis_proven !== false ||
    report?.candidate_transform?.operation_order_proven !== false ||
    report?.candidate_transform?.integer_rounding_proven !== false ||
    !Array.isArray(rows) || rows.length !== 2 ||
    JSON.stringify(rows.map((row) => row.target_physical_defense_raw_without_effect)) !==
      JSON.stringify([5907, 5370]) ||
    JSON.stringify(rows.map((row) => row.distinct_predicted_damage_with_effect)) !==
      JSON.stringify([[85530, 85533], [87122, 87125]]) ||
    rows.some((row) => row.variants.length !== 3 || row.variants.some((variant) =>
      Number(variant.recipient_counterfactual_damage_without_effect) +
        Number(variant.provider_candidate_contribution_damage) !==
        Number(variant.conserved_ordinary_damage))) ||
    report?.acquisition_contract?.remote_player_packet_dependency !== false ||
    report?.authority?.exact_damage_projection_proven !== false ||
    report?.authority?.formula_authority !== false || report?.authority?.runtime_authority !== false ||
    report?.authority?.ui_display_authority !== false ||
    report?.authority?.provider_rdps_credit_allowed !== false) {
    throw new Error("counterfactual discriminants violate their fail-closed schema");
  }
  for (const descriptor of Object.values(report.inputs ?? {})) validateDescriptor(descriptor);
}

function verifyInputs(report) {
  for (const descriptor of Object.values(report.inputs)) {
    const bytes = readFileSync(path.resolve(descriptor.path));
    if (bytes.length !== Number(descriptor.bytes) ||
      createHash("sha256").update(bytes).digest("hex") !== descriptor.sha256) {
      throw new Error(`input changed: ${descriptor.path}`);
    }
  }
}

function selfTest() {
  const high = counterfactualRow(107006n, 5907n, 22000n, 650n);
  const low = counterfactualRow(107006n, 5370n, 22000n, 650n);
  if (high.predicted_damage_without_effect !== 84356 ||
    JSON.stringify(high.distinct_predicted_damage_with_effect) !== JSON.stringify([85530, 85533]) ||
    low.predicted_damage_without_effect !== 86011 ||
    JSON.stringify(low.distinct_predicted_damage_with_effect) !== JSON.stringify([87122, 87125])) {
    throw new Error("exact candidate discriminant arithmetic changed");
  }
  console.log("bpsr-blade-sweep-counterfactual-discriminants self-test passed");
}

function fileDescriptor(file) {
  const bytes = readFileSync(file);
  return {
    path: file.replaceAll("\\", "/"),
    bytes: statSync(file).size,
    sha256: createHash("sha256").update(bytes).digest("hex"),
  };
}
function validateDescriptor(value) {
  if (!String(value?.path ?? "") || !Number.isSafeInteger(Number(value?.bytes)) ||
    Number(value.bytes) <= 0 || !/^[0-9a-f]{64}$/.test(String(value?.sha256 ?? ""))) {
    throw new Error("invalid exact file descriptor");
  }
}
function orderedContentHash(value) {
  const copy = structuredClone(value);
  delete copy.content_sha256;
  return createHash("sha256").update(JSON.stringify(copy)).digest("hex");
}
function stableContentHash(value) {
  const copy = structuredClone(value);
  delete copy.content_sha256;
  return createHash("sha256").update(stableStringify(copy)).digest("hex");
}
function stableStringify(value) {
  if (value === null || typeof value !== "object") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  return `{${Object.keys(value).sort().map((key) =>
    `${JSON.stringify(key)}:${stableStringify(value[key])}`
  ).join(",")}}`;
}
function readJson(file, label) {
  try { return JSON.parse(readFileSync(file, "utf8")); }
  catch (error) { throw new Error(`unable to read ${label} ${file}: ${error.message}`); }
}
function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index]?.replace(/^--/, "");
    const value = args[index + 1];
    if (!key || value === undefined) throw new Error(`invalid argument near ${args[index]}`);
    parsed[key] = value;
  }
  return parsed;
}
function required(parsed, key) {
  if (!parsed[key]) throw new Error(`missing --${key}`);
  return parsed[key];
}
function numericString(value, label) {
  if (!/^\d+$/.test(String(value))) throw new Error(`${label} must be numeric`);
  return String(value);
}
function usage(code) {
  console.log("Usage:\n  node tools/bpsr-blade-sweep-counterfactual-discriminants.mjs analyze --build <id> --scalar-proof <json> --near-pair-proof <json> --acquisition-worklist <json> --actor-scene-cohort <json> --damage-activation-index <json> --damage-formula-surface <json> --damage-source-route-proof <json> --output <json>\n  node tools/bpsr-blade-sweep-counterfactual-discriminants.mjs verify --input <json>\n  node tools/bpsr-blade-sweep-counterfactual-discriminants.mjs self-test");
  process.exit(code);
}
