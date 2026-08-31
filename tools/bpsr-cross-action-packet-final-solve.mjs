#!/usr/bin/env node

import { createHash } from "node:crypto";
import {
  closeSync, createReadStream, existsSync, mkdirSync, openSync, readFileSync, readSync, statSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 1;
const GENERATOR = "tools/bpsr-cross-action-packet-final-solve.mjs";
// Schema 39 retains the same canonical sample, packet, attribute-state, and
// status-state fields consumed below. It is the bounded 26-log current-build
// cohort and is intentionally accepted so inverse solves are not restricted
// to later single-run exports.
const SUPPORTED_COHORT_SCHEMAS = new Set([39, 43, 46, 47]);
const MAX_SMALL_JSON_BYTES = 32 * 1024 * 1024;
const MAX_SAMPLES = 2_000_000;
const MAX_BASE_STRATA = 500_000;
const MAX_RECEIPTS = 100;
const ROUNDING_MODES = ["floor", "ceil", "positive_half_up"];
const PACKET_ACTION_OR_OUTPUT_KEYS = new Set([
  "ability_id", "action_id", "amount", "damage", "damage_array_count", "damage_array_index",
  "damage_attr_id", "hit_event_id", "skill_id", "owner_id", "attacker_uuid", "normal_value",
  "lucky_value", "skill_effect_uuid", "skill_effect_group_index", "skill_effect_component_index",
  "skill_effect_component_count", "skill_effect_total_damage",
]);

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);
if (command === "build") await build(options);
else if (command === "verify") verifyCommand(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

async function build(parsed) {
  const cohortPath = resolved(required(parsed, "cohort"));
  const stagePath = resolved(required(parsed, "damage-stage"));
  const output = resolved(required(parsed, "output"));
  if (existsSync(output)) throw new Error(`Refusing to overwrite existing output: ${output}`);
  // Normalize values through the exact JSON representation that will be
  // written. Large schema-39 cohorts may retain signed zero coordinates;
  // JSON serializes -0 as 0, so hashing the pre-serialization object would
  // produce a receipt that cannot verify after it is reopened.
  const report = JSON.parse(JSON.stringify(await buildReport(cohortPath, stagePath)));
  report.content_sha256 = contentHash(report);
  verifyReport(report);
  mkdirSync(path.dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
  console.log(`wrote ${output}`);
}

async function buildReport(cohortPath, stagePath) {
  const header = readHeader(cohortPath);
  assert(SUPPORTED_COHORT_SCHEMAS.has(header.schema_version),
    `Unsupported formula cohort schema ${header.schema_version}`);
  assert(String(header.game_build).length > 0, "Formula cohort game_build is required");
  const stage = readSmallJson(stagePath, "damage-stage catalog");
  assert(String(stage.game_build) === String(header.game_build), "Input build identity mismatch");
  assert(Array.isArray(stage.rules), "Damage-stage rules are required");
  const ruleIndex = indexRules(stage.rules);

  const baseStrata = new Map();
  const positionRelaxedStrata = new Map();
  const counters = {
    samples_scanned: 0,
    samples_with_exact_single_damage_rule: 0,
    rejected_missing_damage_rule: 0,
    rejected_ambiguous_damage_rule: 0,
    rejected_ambiguous_coefficient_stage: 0,
    rejected_missing_or_invalid_owner_level_for_fixed_ladder: 0,
    rejected_invalid_packet_or_identity: 0,
  };
  const observedSampleKeys = new Set();
  const observedPacketKeys = new Set();
  const scan = await scanObjectArray(cohortPath, "samples", (sample) => {
    counters.samples_scanned += 1;
    assert(counters.samples_scanned <= MAX_SAMPLES, `Sample cap ${MAX_SAMPLES} exceeded`);
    if (counters.samples_scanned === 1) {
      Object.keys(sample ?? {}).forEach((key) => observedSampleKeys.add(key));
      Object.keys(sample?.packet ?? {}).forEach((key) => observedPacketKeys.add(key));
    }
    const resolvedRule = resolveRule(sample, ruleIndex, counters);
    if (!resolvedRule) return;
    let identity;
    try {
      identity = sampleIdentity(sample);
    } catch {
      counters.rejected_invalid_packet_or_identity += 1;
      return;
    }
    counters.samples_with_exact_single_damage_rule += 1;
    const baseKey = stableStringify(identity.base);
    let base = baseStrata.get(baseKey);
    if (!base) {
      assert(baseStrata.size < MAX_BASE_STRATA, `Base-stratum cap ${MAX_BASE_STRATA} exceeded`);
      base = { identity: identity.base, variants: new Map(), allActions: new Set() };
      baseStrata.set(baseKey, base);
    }
    base.allActions.add(sample.ability_id);
    const variantKey = stableStringify(identity.hidden);
    let variant = base.variants.get(variantKey);
    if (!variant) {
      variant = { hidden: identity.hidden, bodies: new Map(), actions: new Set() };
      base.variants.set(variantKey, variant);
    }
    variant.actions.add(sample.ability_id);
    const bodyKey = `${sample.ability_id}:${sample.hit_event_id}:${resolvedRule.damage_attr_id}:` +
      `${resolvedRule.coefficient_basis_points}:${resolvedRule.fixed_parameter}`;
    let body = variant.bodies.get(bodyKey);
    if (!body) {
      body = {
        ability_id: sample.ability_id,
        hit_event_id: sample.hit_event_id,
        damage_attr_id: resolvedRule.damage_attr_id,
        coefficient_basis_points: resolvedRule.coefficient_basis_points,
        fixed_parameter: resolvedRule.fixed_parameter,
        amounts: new Set(),
        sequences: [],
      };
      variant.bodies.set(bodyKey, body);
    }
    body.amounts.add(safeInteger(sample.amount, "sample.amount"));
    if (body.sequences.length < 5) body.sequences.push(sample.sequence);

    const relaxedKey = stableStringify({ base: identity.base, hidden: identity.formulaHidden });
    let relaxed = positionRelaxedStrata.get(relaxedKey);
    if (!relaxed) {
      relaxed = {
        identity: identity.base,
        hidden: identity.formulaHidden,
        bodies: new Map(),
        actions: new Set(),
      };
      positionRelaxedStrata.set(relaxedKey, relaxed);
    }
    relaxed.actions.add(sample.ability_id);
    let relaxedBody = relaxed.bodies.get(bodyKey);
    if (!relaxedBody) {
      relaxedBody = {
        ability_id: sample.ability_id,
        hit_event_id: sample.hit_event_id,
        damage_attr_id: resolvedRule.damage_attr_id,
        coefficient_basis_points: resolvedRule.coefficient_basis_points,
        fixed_parameter: resolvedRule.fixed_parameter,
        amounts: new Set(),
        sequences: [],
      };
      relaxed.bodies.set(bodyKey, relaxedBody);
    }
    relaxedBody.amounts.add(safeInteger(sample.amount, "sample.amount"));
    if (relaxedBody.sequences.length < 5) relaxedBody.sequences.push(sample.sequence);
  });

  const receipts = [];
  const twoBodyReceipts = [];
  const nonsingularTwoBodyReceipts = [];
  const hiddenOutputVariationReceipts = [];
  const closest = [];
  const summary = {
    ...counters,
    base_strata: baseStrata.size,
    base_strata_with_two_or_more_actions: 0,
    base_strata_split_by_hidden_variation: 0,
    strict_strata_with_two_or_more_actions: 0,
    strict_strata_with_two_or_more_bodies: 0,
    strict_strata_rejected_nonconstant_packet_final_within_body: 0,
    strict_two_body_nonsingular_systems: 0,
    strict_two_body_proportional_or_singular_systems: 0,
    strict_singular_systems_compatible_with_one_shared_product_after_integer_rounding: 0,
    strict_singular_systems_incompatible_with_one_shared_product_after_integer_rounding: 0,
    strict_overdetermined_strata: 0,
    strict_overdetermined_training_solves: 0,
    held_out_exact_unrounded_matches: 0,
    held_out_consistent_rounding_matches: 0,
    held_out_prediction_failures: 0,
    validated_overdetermined_strata: 0,
    position_relaxed_strata_with_two_or_more_actions: 0,
    position_relaxed_strata_rejected_nonconstant_packet_final_within_body: 0,
    position_relaxed_nonsingular_pairs: 0,
    position_relaxed_same_ability_nonsingular_pairs: 0,
  };
  const positionRelaxedNonsingularReceipts = [];
  const positionRelaxedSameAbilityReceipts = [];

  for (const base of baseStrata.values()) {
    if (base.allActions.size < 2) continue;
    summary.base_strata_with_two_or_more_actions += 1;
    if (![...base.variants.values()].some((variant) => variant.actions.size >= 2)) {
      summary.base_strata_split_by_hidden_variation += 1;
    }
    closest.push({
      identity: base.identity,
      actions: [...base.allActions].sort(numberCompare),
      hidden_variants: base.variants.size,
      maximum_actions_in_one_hidden_variant: Math.max(...[...base.variants.values()]
        .map((variant) => variant.actions.size)),
      maximum_bodies_in_one_hidden_variant: Math.max(...[...base.variants.values()]
        .map((variant) => variant.bodies.size)),
      hidden_signature_examples: [...base.variants.values()].slice(0, 3)
        .map((variant) => variant.hidden),
    });
    for (const variant of base.variants.values()) {
      if (variant.actions.size < 2) continue;
      summary.strict_strata_with_two_or_more_actions += 1;
      if (variant.bodies.size < 2) continue;
      summary.strict_strata_with_two_or_more_bodies += 1;
      const bodies = [...variant.bodies.values()].sort(bodyCompare);
      if (bodies.some((body) => body.amounts.size !== 1)) {
        summary.strict_strata_rejected_nonconstant_packet_final_within_body += 1;
        if (hiddenOutputVariationReceipts.length < 20) {
          hiddenOutputVariationReceipts.push({
            identity: base.identity,
            hidden_signature: variant.hidden,
            bodies: bodies.map((body) => ({
              ability_id: body.ability_id,
              hit_event_id: body.hit_event_id,
              damage_attr_id: body.damage_attr_id,
              coefficient_basis_points: body.coefficient_basis_points,
              fixed_parameter: body.fixed_parameter,
              packet_final_integers: [...body.amounts].sort(numberCompare),
              sequences: body.sequences,
            })),
            rejection: "same exact body has multiple packet-final integers",
          });
        }
        continue;
      }
      const rows = bodies.map((body) => ({
        ...body,
        packet_final_integer: [...body.amounts][0],
        amounts: undefined,
      }));
      const pairKinds = classifyPairs(rows);
      summary.strict_two_body_nonsingular_systems += pairKinds.nonsingular;
      summary.strict_two_body_proportional_or_singular_systems += pairKinds.singular;
      if (rows.length === 2 && pairKinds.nonsingular === 1 &&
        nonsingularTwoBodyReceipts.length < MAX_RECEIPTS) {
        const solved = solveTwo(rows[0], rows[1]);
        nonsingularTwoBodyReceipts.push({
          identity: base.identity,
          hidden_signature: variant.hidden,
          bodies: rows.map(compactBody),
          exact_packet_integer_equation_solution: solved == null ? null : {
            attack_stage_product: rationalJson(solved.x),
            later_multiplier: rationalJson(solved.y),
          },
          packet_final_rounding_is_a_latent_constraint_not_a_required_server_field: true,
          held_out_body_available: false,
          runtime_authority: false,
        });
      }
      if (rows.length === 2 && twoBodyReceipts.length < 50) {
        const determinant = BigInt(rows[0].coefficient_basis_points) *
          BigInt(rows[1].fixed_parameter) - BigInt(rows[1].coefficient_basis_points) *
          BigInt(rows[0].fixed_parameter);
        const proportional = determinant === 0n ? proportionalCompatibility(rows) : null;
        if (proportional?.compatible) {
          summary.strict_singular_systems_compatible_with_one_shared_product_after_integer_rounding += 1;
        } else if (proportional) {
          summary.strict_singular_systems_incompatible_with_one_shared_product_after_integer_rounding += 1;
        }
        twoBodyReceipts.push({
          identity: base.identity,
          hidden_signature: variant.hidden,
          bodies: rows.map(compactBody),
          coefficient_fixed_determinant: determinant.toString(),
          attack_stage_body_and_later_multiplier_separately_identifiable: determinant !== 0n,
          shared_product_integer_rounding_test: proportional,
          held_out_body_available: false,
          runtime_authority: false,
        });
      }
      else if (rows.length === 2 && pairKinds.singular === 1) {
        const proportional = proportionalCompatibility(rows);
        if (proportional.compatible) {
          summary.strict_singular_systems_compatible_with_one_shared_product_after_integer_rounding += 1;
        } else {
          summary.strict_singular_systems_incompatible_with_one_shared_product_after_integer_rounding += 1;
        }
      }
      if (rows.length < 3) continue;
      summary.strict_overdetermined_strata += 1;
      const evaluation = evaluateHeldOut(rows);
      summary.strict_overdetermined_training_solves += evaluation.training_solves;
      summary.held_out_exact_unrounded_matches += evaluation.exact_unrounded_matches;
      summary.held_out_consistent_rounding_matches += evaluation.consistent_rounding_matches;
      summary.held_out_prediction_failures += evaluation.prediction_failures;
      if (evaluation.validated) summary.validated_overdetermined_strata += 1;
      if (receipts.length < MAX_RECEIPTS) {
        receipts.push({
          identity: base.identity,
          hidden_signature: variant.hidden,
          bodies: rows.map(compactBody),
          evaluation,
          runtime_authority: false,
        });
      }
    }
  }
  for (const relaxed of positionRelaxedStrata.values()) {
    if (relaxed.actions.size < 2 || relaxed.bodies.size < 2) continue;
    summary.position_relaxed_strata_with_two_or_more_actions += 1;
    const bodies = [...relaxed.bodies.values()].sort(bodyCompare);
    if (bodies.some((body) => body.amounts.size !== 1)) {
      summary.position_relaxed_strata_rejected_nonconstant_packet_final_within_body += 1;
      continue;
    }
    const rows = bodies.map((body) => ({
      ...body,
      packet_final_integer: [...body.amounts][0],
      amounts: undefined,
    }));
    for (let left = 0; left < rows.length; left += 1) {
      for (let right = left + 1; right < rows.length; right += 1) {
        const solved = solveTwo(rows[left], rows[right]);
        if (!solved) continue;
        summary.position_relaxed_nonsingular_pairs += 1;
        if (rows[left].ability_id === rows[right].ability_id) {
          summary.position_relaxed_same_ability_nonsingular_pairs += 1;
          if (positionRelaxedSameAbilityReceipts.length < MAX_RECEIPTS) {
            positionRelaxedSameAbilityReceipts.push({
              identity: relaxed.identity,
              position_relaxed_hidden_signature: relaxed.hidden,
              training_bodies: [compactBody(rows[left]), compactBody(rows[right])],
              exact_packet_integer_equation_solution: {
                attack_stage_product: rationalJson(solved.x),
                later_multiplier: rationalJson(solved.y),
              },
              position_relaxation_scope: "source, direct-source, target, and packet positions only",
              packet_final_rounding_is_a_latent_constraint_not_a_required_server_field: true,
              runtime_authority: false,
            });
          }
        }
        if (positionRelaxedNonsingularReceipts.length >= MAX_RECEIPTS) continue;
        const heldOut = rows.filter((_, index) => index !== left && index !== right)
          .map((row) => predictRow(row, solved));
        positionRelaxedNonsingularReceipts.push({
          identity: relaxed.identity,
          position_relaxed_hidden_signature: relaxed.hidden,
          training_bodies: [compactBody(rows[left]), compactBody(rows[right])],
          exact_packet_integer_equation_solution: {
            attack_stage_product: rationalJson(solved.x),
            later_multiplier: rationalJson(solved.y),
          },
          held_out_predictions: heldOut,
          position_relaxation_scope: "source, direct-source, target, and packet positions only",
          packet_final_rounding_is_a_latent_constraint_not_a_required_server_field: true,
          runtime_authority: false,
        });
      }
    }
  }
  closest.sort((left, right) =>
    right.maximum_bodies_in_one_hidden_variant - left.maximum_bodies_in_one_hidden_variant ||
    right.maximum_actions_in_one_hidden_variant - left.maximum_actions_in_one_hidden_variant ||
    compareText(stableStringify(left.identity), stableStringify(right.identity)));

  const selectedAttributeStateIds = new Set();
  const selectedStatusStateIds = new Set();
  for (const receipt of [...nonsingularTwoBodyReceipts, ...positionRelaxedNonsingularReceipts,
    ...positionRelaxedSameAbilityReceipts]) {
    selectedAttributeStateIds.add(receipt.identity.source_attribute_state_id);
    selectedAttributeStateIds.add(receipt.identity.target_attribute_state_id);
    selectedStatusStateIds.add(receipt.identity.source_status_state_id);
    selectedStatusStateIds.add(receipt.identity.target_status_state_id);
  }
  const selectedAttributeStates = await collectIndexedStates(
    cohortPath, "attribute_states", selectedAttributeStateIds);
  const selectedStatusStates = await collectIndexedStates(
    cohortPath, "status_states", selectedStatusStateIds);

  return {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATOR,
    game_build: String(header.game_build),
    policy: {
      exact_numeric_ids_and_build_identity_authoritative: true,
      cohort_streamed_not_fully_deserialized: true,
      exact_source_target_attribute_and_status_state_ids_required: true,
      actor_position_and_opaque_packet_variation_split_not_ignored: true,
      multiple_packet_final_values_for_one_body_rejected_as_hidden_variation: true,
      training_packet_integers_are_zero_width_diagnostic_equations_not_pre_round_server_values: true,
      held_out_rounding_modes_are_diagnostics_not_server_authority: true,
      arbitrary_residual_to_mechanic_inference_allowed: false,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
    },
    inputs: {
      cohort: descriptor(cohortPath, scan.sha256),
      damage_stage: descriptor(stagePath),
    },
    observed_shape: {
      first_sample_keys: [...observedSampleKeys].sort(),
      first_packet_keys: [...observedPacketKeys].sort(),
    },
    processing: {
      maximum_samples: MAX_SAMPLES,
      maximum_base_strata: MAX_BASE_STRATA,
      maximum_receipts: MAX_RECEIPTS,
      scan,
      selected_attribute_state_scan: selectedAttributeStates.scan,
      selected_status_state_scan: selectedStatusStates.scan,
    },
    summary,
    closest_cross_action_base_strata: closest.slice(0, 25),
    two_body_receipts: twoBodyReceipts,
    nonsingular_two_body_receipts: nonsingularTwoBodyReceipts,
    position_relaxed_nonsingular_receipts: positionRelaxedNonsingularReceipts,
    position_relaxed_same_ability_nonsingular_receipts: positionRelaxedSameAbilityReceipts,
    selected_attribute_states: selectedAttributeStates.values,
    selected_status_states: selectedStatusStates.values,
    hidden_output_variation_receipts: hiddenOutputVariationReceipts,
    overdetermined_receipts: receipts,
    conclusion: conclusion(summary),
    content_sha256: null,
  };
}

function resolveRule(sample, index, counters) {
  let rules = index.get(`${sample?.ability_id}:${sample?.hit_event_id}`) ?? [];
  if (rules.length === 0) {
    counters.rejected_missing_damage_rule += 1;
    return null;
  }
  if (rules.length > 1) {
    const packetDamageSource = sample?.damage_source ?? sample?.packet?.damage_source;
    const sourceSpecific = rules.filter((rule) =>
      rule.damage_source !== undefined && rule.damage_source !== null &&
      Number(rule.damage_source) === Number(packetDamageSource));
    const generic = rules.filter((rule) =>
      rule.damage_source === undefined || rule.damage_source === null);
    rules = sourceSpecific.length > 0 ? sourceSpecific : generic;
  }
  if (rules.length !== 1) {
    counters.rejected_ambiguous_damage_rule += 1;
    return null;
  }
  const rule = rules[0];
  if (!Array.isArray(rule.coefficient_basis_points_by_stage) ||
    rule.coefficient_basis_points_by_stage.length === 0) {
    counters.rejected_ambiguous_coefficient_stage += 1;
    return null;
  }
  const ownerStage = sample?.packet?.owner_stage == null ? 0 : Number(sample.packet.owner_stage);
  const coefficientIndex = rule.coefficient_basis_points_by_stage.length === 1 ? 0 : ownerStage;
  if (!Number.isSafeInteger(coefficientIndex) || coefficientIndex < 0 ||
    coefficientIndex >= rule.coefficient_basis_points_by_stage.length) {
    counters.rejected_ambiguous_coefficient_stage += 1;
    return null;
  }
  const coefficient = safeInteger(
    rule.coefficient_basis_points_by_stage[coefficientIndex], "coefficient");
  let fixed = 0;
  if ((rule.fixed_parameter_by_level ?? []).length > 0) {
    const level = Number(sample?.packet?.owner_level);
    if (!Number.isSafeInteger(level) || level < 1 || level > rule.fixed_parameter_by_level.length) {
      counters.rejected_missing_or_invalid_owner_level_for_fixed_ladder += 1;
      return null;
    }
    fixed = safeInteger(rule.fixed_parameter_by_level[level - 1], "fixed parameter");
  }
  return {
    damage_attr_id: safeInteger(rule.damage_attr_id, "damage_attr_id"),
    coefficient_basis_points: coefficient,
    fixed_parameter: fixed,
  };
}

function sampleIdentity(sample) {
  // Older current-build cohort schema 39 legitimately retains an unknown
  // scene_id as null. Session plus exact source/target entity identity still
  // prevents cross-run or cross-actor grouping, so null is a preserved state
  // value rather than a reason to discard the packet equation.
  for (const key of ["session_id", "source_entity_uuid", "target_entity_uuid",
    "source_attribute_state_id", "source_status_state_id", "target_attribute_state_id",
    "target_status_state_id", "amount", "ability_id", "hit_event_id"]) {
    assert(sample?.[key] !== undefined && sample?.[key] !== null, `Missing sample ${key}`);
  }
  const packet = sample.packet ?? {};
  const packetCommon = Object.fromEntries(Object.entries(packet)
    .filter(([key]) => !PACKET_ACTION_OR_OUTPUT_KEYS.has(key))
    .sort(([left], [right]) => compareText(left, right)));
  const positionRelaxedPacketCommon = Object.fromEntries(Object.entries(packetCommon)
    .filter(([key]) => key !== "position"));
  return {
    base: {
      session_id: sample.session_id,
      scene_id: sample.scene_id,
      source_entity_uuid: sample.source_entity_uuid,
      target_entity_uuid: sample.target_entity_uuid,
      source_attribute_state_id: sample.source_attribute_state_id,
      source_status_state_id: sample.source_status_state_id,
      target_attribute_state_id: sample.target_attribute_state_id,
      target_status_state_id: sample.target_status_state_id,
      critical: Boolean(sample.critical),
      lucky: Boolean(sample.lucky),
    },
    hidden: {
      direct_source_entity_uuid: sample.direct_source_entity_uuid ?? null,
      source_actor_identity: sample.source_actor_identity ?? null,
      direct_source_actor_identity: sample.direct_source_actor_identity ?? null,
      target_actor_identity: sample.target_actor_identity ?? null,
      source_position_at_wire_message_start: sample.source_position_at_wire_message_start ?? null,
      direct_source_position_at_wire_message_start:
        sample.direct_source_position_at_wire_message_start ?? null,
      target_position_at_wire_message_start: sample.target_position_at_wire_message_start ?? null,
      damage_source: sample.damage_source ?? null,
      damage_type: sample.damage_type ?? null,
      passive_uuid: sample.passive_uuid ?? null,
      status_provider_attribute_states: sample.status_provider_attribute_states ?? [],
      packet_common: packetCommon,
    },
    formulaHidden: {
      direct_source_entity_uuid: sample.direct_source_entity_uuid ?? null,
      source_actor_identity: sample.source_actor_identity ?? null,
      direct_source_actor_identity: sample.direct_source_actor_identity ?? null,
      target_actor_identity: sample.target_actor_identity ?? null,
      damage_source: sample.damage_source ?? null,
      damage_type: sample.damage_type ?? null,
      passive_uuid: sample.passive_uuid ?? null,
      status_provider_attribute_states: sample.status_provider_attribute_states ?? [],
      packet_common: positionRelaxedPacketCommon,
    },
  };
}

function evaluateHeldOut(rows) {
  let trainingSolves = 0;
  let exactMatches = 0;
  let roundedMatches = 0;
  let failures = 0;
  const solves = [];
  let validated = false;
  for (let left = 0; left < rows.length; left += 1) {
    for (let right = left + 1; right < rows.length; right += 1) {
      const solved = solveTwo(rows[left], rows[right]);
      if (!solved) continue;
      trainingSolves += 1;
      const heldOut = rows.filter((_, index) => index !== left && index !== right);
      const predictions = heldOut.map((row) => predictRow(row, solved));
      const exactAll = predictions.every((row) => row.exact_unrounded_match);
      if (exactAll) exactMatches += predictions.length;
      const consistentModes = ROUNDING_MODES.filter((mode) =>
        predictions.every((row) => row.rounded_matches[mode]));
      if (consistentModes.length > 0) roundedMatches += predictions.length;
      else failures += predictions.length;
      if (heldOut.length > 0 && (exactAll || consistentModes.length > 0)) validated = true;
      if (solves.length < 20) {
        solves.push({
          training_body_indexes: [left, right],
          attack_stage_product: rationalJson(solved.x),
          later_multiplier: rationalJson(solved.y),
          held_out_predictions: predictions,
          consistent_final_rounding_modes: consistentModes,
          zero_width_training_integer_assumption: true,
        });
      }
    }
  }
  return {
    training_solves: trainingSolves,
    exact_unrounded_matches: exactMatches,
    consistent_rounding_matches: roundedMatches,
    prediction_failures: failures,
    validated,
    runtime_authority: false,
    solves,
  };
}

function solveTwo(left, right) {
  const c1 = BigInt(left.coefficient_basis_points);
  const c2 = BigInt(right.coefficient_basis_points);
  const f1 = BigInt(left.fixed_parameter);
  const f2 = BigInt(right.fixed_parameter);
  const d1 = BigInt(left.packet_final_integer);
  const d2 = BigInt(right.packet_final_integer);
  const determinant = c1 * f2 - c2 * f1;
  if (determinant === 0n) return null;
  const x = rational(d1 * f2 - d2 * f1, determinant);
  const y = rational(c1 * d2 - c2 * d1, determinant);
  if (x.numerator <= 0n || y.numerator <= 0n) return null;
  return { x, y };
}

function predictRow(row, solved) {
  const predicted = add(multiply(integerRational(row.coefficient_basis_points), solved.x),
    multiply(integerRational(row.fixed_parameter), solved.y));
  const observed = BigInt(row.packet_final_integer);
  const roundedMatches = Object.fromEntries(ROUNDING_MODES.map((mode) =>
    [mode, roundRational(predicted, mode) === observed]));
  return {
    body: compactBody(row),
    predicted_rational: rationalJson(predicted),
    exact_unrounded_match: predicted.denominator === 1n && predicted.numerator === observed,
    rounded_matches: roundedMatches,
  };
}

function classifyPairs(rows) {
  let nonsingular = 0;
  let singular = 0;
  for (let left = 0; left < rows.length; left += 1) {
    for (let right = left + 1; right < rows.length; right += 1) {
      const determinant = BigInt(rows[left].coefficient_basis_points) *
        BigInt(rows[right].fixed_parameter) - BigInt(rows[right].coefficient_basis_points) *
        BigInt(rows[left].fixed_parameter);
      if (determinant === 0n) singular += 1;
      else nonsingular += 1;
    }
  }
  return { nonsingular, singular };
}

function proportionalCompatibility(rows) {
  assert(rows.length >= 2, "Proportional compatibility requires at least two rows");
  const scales = rows.map((row) => {
    if (row.coefficient_basis_points !== 0) return BigInt(row.coefficient_basis_points);
    assert(row.fixed_parameter !== 0, "Singular zero formula body has no usable scale");
    return BigInt(row.fixed_parameter);
  });
  const modes = [];
  const productIntervals = {};
  for (const mode of ROUNDING_MODES) {
    let intersection = null;
    for (let index = 0; index < rows.length; index += 1) {
      const interval = inverseRoundingInterval(BigInt(rows[index].packet_final_integer),
        scales[index], mode);
      intersection = intersection ? intersectIntervals(intersection, interval) : interval;
      if (!intersection) break;
    }
    if (intersection) {
      modes.push(mode);
      productIntervals[mode] = {
        lower: rationalJson(intersection.lower),
        lower_inclusive: intersection.lowerInclusive,
        upper: rationalJson(intersection.upper),
        upper_inclusive: intersection.upperInclusive,
      };
    }
  }
  return {
    compatible: modes.length > 0,
    consistent_final_rounding_modes: modes,
    shared_product_intervals: productIntervals,
    separates_attack_stage_body_from_later_multiplier: false,
  };
}

function inverseRoundingInterval(observed, positiveScale, mode) {
  assert(positiveScale > 0n, "Formula scale must be positive");
  if (mode === "floor") return {
    lower: rational(observed, positiveScale), lowerInclusive: true,
    upper: rational(observed + 1n, positiveScale), upperInclusive: false,
  };
  if (mode === "ceil") return {
    lower: rational(observed - 1n, positiveScale), lowerInclusive: false,
    upper: rational(observed, positiveScale), upperInclusive: true,
  };
  if (mode === "positive_half_up") return {
    lower: rational(observed * 2n - 1n, positiveScale * 2n), lowerInclusive: true,
    upper: rational(observed * 2n + 1n, positiveScale * 2n), upperInclusive: false,
  };
  throw new Error(`Unsupported rounding mode ${mode}`);
}

function intersectIntervals(left, right) {
  const lowerComparison = compareRational(left.lower, right.lower);
  const lower = lowerComparison >= 0 ? left.lower : right.lower;
  const lowerInclusive = lowerComparison > 0 ? left.lowerInclusive : lowerComparison < 0 ?
    right.lowerInclusive : left.lowerInclusive && right.lowerInclusive;
  const upperComparison = compareRational(left.upper, right.upper);
  const upper = upperComparison <= 0 ? left.upper : right.upper;
  const upperInclusive = upperComparison < 0 ? left.upperInclusive : upperComparison > 0 ?
    right.upperInclusive : left.upperInclusive && right.upperInclusive;
  const comparison = compareRational(lower, upper);
  if (comparison > 0 || (comparison === 0 && !(lowerInclusive && upperInclusive))) return null;
  return { lower, lowerInclusive, upper, upperInclusive };
}

function compareRational(left, right) {
  const difference = left.numerator * right.denominator - right.numerator * left.denominator;
  return difference < 0n ? -1 : difference > 0n ? 1 : 0;
}

function compactBody(row) {
  return {
    ability_id: row.ability_id,
    hit_event_id: row.hit_event_id,
    damage_attr_id: row.damage_attr_id,
    coefficient_basis_points: row.coefficient_basis_points,
    fixed_parameter: row.fixed_parameter,
    packet_final_integer: row.packet_final_integer,
    sequences: row.sequences,
  };
}

function conclusion(summary) {
  if (summary.validated_overdetermined_strata > 0) {
    return {
      disposition: "diagnostic-held-out-match-only",
      explanation: "At least one two-equation solve predicts a held-out body under a consistent final-rounding constraint. Hidden server intermediates are latent variables to solve or bound; remaining authority depends on discriminating the complete client formula and operation order.",
    };
  }
  if (summary.strict_overdetermined_strata > 0) {
    if (summary.strict_overdetermined_training_solves === 0) {
      return {
        disposition: "overdetermined-bodies-found-but-all-training-pairs-singular",
        explanation: "Three-or-more exact action bodies occur in a shared packet-state stratum, but every known coefficient/fixed pair has zero determinant. The encounter-local Attack body and later multiplier cannot be separated, so no held-out prediction is admitted.",
      };
    }
    return {
      disposition: "overdetermined-strata-found-but-held-out-validation-failed",
      explanation: "Exact packet state strata exist, but no accepted two-equation solve predicts the held-out packet finals under one tested final rounding mode.",
    };
  }
  if (summary.strict_strata_with_two_or_more_bodies > 0) {
    return {
      disposition: "two-body-only-or-hidden-output-variation",
      explanation: "There is no third exact body for held-out validation, or repeated identical bodies have divergent packet finals and were rejected.",
    };
  }
  return {
    disposition: "no-exact-cross-action-formula-stratum",
    explanation: "No exact recipient/target attribute, status, actor, position, packet-flag stratum contains two independently known coefficient/fixed bodies.",
  };
}

function indexRules(rules) {
  const result = new Map();
  for (const rule of rules) {
    const key = `${rule.ability_id}:${rule.hit_event_id}`;
    if (!result.has(key)) result.set(key, []);
    result.get(key).push(rule);
  }
  return result;
}

async function collectIndexedStates(file, propertyName, selectedIds) {
  const values = {};
  const scan = await scanObjectArray(file, propertyName, (value, index) => {
    if (selectedIds.has(index)) values[String(index)] = value;
  }, true);
  assert(Object.keys(values).length === selectedIds.size,
    `Missing selected ${propertyName}: expected ${selectedIds.size}, found ${Object.keys(values).length}`);
  return { values, scan };
}

async function scanObjectArray(file, propertyName, onItem, stopAfterProperty = false) {
  const marker = `"${propertyName}":[`;
  let markerOffset = 0;
  let found = false;
  let complete = false;
  let started = false;
  let depth = 0;
  let inString = false;
  let escaped = false;
  let itemText = "";
  let items = 0;
  let maximumItemBytes = 0;
  let bytesRead = 0;
  const hash = createHash("sha256");
  const stream = createReadStream(file, { encoding: "utf8", highWaterMark: 1024 * 1024 });
  outer: for await (const chunk of stream) {
    bytesRead += Buffer.byteLength(chunk);
    hash.update(chunk);
    for (const character of chunk) {
      if (!found) {
        if (character === marker[markerOffset]) {
          markerOffset += 1;
          if (markerOffset === marker.length) found = true;
        } else markerOffset = character === marker[0] ? 1 : 0;
        continue;
      }
      if (complete) {
        if (stopAfterProperty) break outer;
        continue;
      }
      if (!started) {
        if (/\s|,/.test(character)) continue;
        if (character === "]") { complete = true; continue; }
        assert(character === "{" || character === "[",
          `Expected ${propertyName} object or array item`);
        started = true;
        depth = 1;
        itemText = character;
        continue;
      }
      itemText += character;
      if (inString) {
        if (escaped) escaped = false;
        else if (character === "\\") escaped = true;
        else if (character === "\"") inString = false;
        continue;
      }
      if (character === "\"") inString = true;
      else if (character === "{" || character === "[") depth += 1;
      else if (character === "}" || character === "]") {
        depth -= 1;
        if (depth === 0) {
          maximumItemBytes = Math.max(maximumItemBytes, Buffer.byteLength(itemText));
          onItem(JSON.parse(itemText), items);
          items += 1;
          started = false;
          itemText = "";
        }
      }
    }
  }
  assert(found && complete, `Property ${propertyName} was not completely scanned`);
  return {
    property: propertyName,
    bytes_read: bytesRead,
    array_items_scanned: items,
    maximum_single_item_bytes: maximumItemBytes,
    bounded_item_retention: true,
    sha256: `sha256:${hash.digest("hex")}`,
  };
}

function readHeader(file) {
  const buffer = Buffer.alloc(65536);
  const descriptor = openSync(file, "r");
  let bytesRead;
  try { bytesRead = readSync(descriptor, buffer, 0, buffer.length, 0); }
  finally { closeSync(descriptor); }
  const text = buffer.toString("utf8", 0, bytesRead);
  const schema = text.match(/"schema_version":\s*(\d+)/);
  const build = text.match(/"game_build":\s*"?(\d+)"?/);
  assert(schema && build, "Unable to read formula cohort header");
  return { schema_version: Number(schema[1]), game_build: build[1] };
}

function readSmallJson(file, label) {
  assert(statSync(file).size <= MAX_SMALL_JSON_BYTES, `${label} exceeds bounded input limit`);
  return JSON.parse(readFileSync(file, "utf8"));
}

function verifyCommand(parsed) {
  const input = resolved(required(parsed, "input"));
  const report = readSmallJson(input, "cross-action solve report");
  verifyReport(report);
  console.log(`verified ${input}`);
}

function verifyReport(report) {
  assert(report?.schema_version === SCHEMA_VERSION && report?.generated_by === GENERATOR,
    "Report identity mismatch");
  assert(report?.content_sha256 === contentHash(report), "Report content hash mismatch");
  assert(report?.policy?.cohort_streamed_not_fully_deserialized === true &&
    report?.policy?.formula_authority === false && report?.policy?.runtime_authority === false &&
    report?.policy?.provider_rdps_credit_allowed === false,
  "Report fail-closed policy mismatch");
  assert(report?.summary?.samples_scanned === report?.processing?.scan?.array_items_scanned,
    "Report sample count mismatch");
  return report;
}

function selfTest() {
  const rows = [
    { coefficient_basis_points: 10000, fixed_parameter: 0, packet_final_integer: 200 },
    { coefficient_basis_points: 20000, fixed_parameter: 10, packet_final_integer: 430 },
    { coefficient_basis_points: 15000, fixed_parameter: 5, packet_final_integer: 315 },
  ];
  const solved = solveTwo(rows[0], rows[1]);
  assert(solved && equalRational(solved.x, rational(1n, 50n)) &&
    equalRational(solved.y, rational(3n, 1n)), "Two-equation solve changed");
  const prediction = predictRow(rows[2], solved);
  assert(prediction.exact_unrounded_match && ROUNDING_MODES.every((mode) =>
    prediction.rounded_matches[mode]), "Held-out prediction changed");
  assert(solveTwo(
    { coefficient_basis_points: 12000, fixed_parameter: 480, packet_final_integer: 100 },
    { coefficient_basis_points: 8000, fixed_parameter: 320, packet_final_integer: 67 },
  ) === null, "Proportional formula bodies must remain singular");
  assert(proportionalCompatibility([
    { coefficient_basis_points: 20000, fixed_parameter: 0, packet_final_integer: 189958 },
    { coefficient_basis_points: 10000, fixed_parameter: 0, packet_final_integer: 94979 },
  ]).compatible, "Adjacent integer proportional bodies must retain a feasible rounding interval");
  assert(!proportionalCompatibility([
    { coefficient_basis_points: 20000, fixed_parameter: 0, packet_final_integer: 125233 },
    { coefficient_basis_points: 10000, fixed_parameter: 0, packet_final_integer: 72032 },
  ]).compatible, "Divergent proportional bodies must fail the shared-product interval test");
  console.log("bpsr-cross-action-packet-final-solve self-test passed");
}

function rational(numerator, denominator) {
  assert(denominator !== 0n, "Zero rational denominator");
  if (denominator < 0n) { numerator = -numerator; denominator = -denominator; }
  const divisor = gcd(numerator, denominator);
  return { numerator: numerator / divisor, denominator: denominator / divisor };
}
function integerRational(value) { return rational(BigInt(value), 1n); }
function add(left, right) {
  return rational(left.numerator * right.denominator + right.numerator * left.denominator,
    left.denominator * right.denominator);
}
function multiply(left, right) {
  return rational(left.numerator * right.numerator, left.denominator * right.denominator);
}
function roundRational(value, mode) {
  assert(value.numerator >= 0n, "Packet-final hypothesis must be nonnegative");
  const q = value.numerator / value.denominator;
  const r = value.numerator % value.denominator;
  if (mode === "floor") return q;
  if (mode === "ceil") return q + (r === 0n ? 0n : 1n);
  if (mode === "positive_half_up") return q + (r * 2n >= value.denominator ? 1n : 0n);
  throw new Error(`Unsupported rounding mode ${mode}`);
}
function rationalJson(value) {
  return { numerator: value.numerator.toString(), denominator: value.denominator.toString() };
}
function equalRational(left, right) {
  return left.numerator === right.numerator && left.denominator === right.denominator;
}
function gcd(left, right) {
  let a = left < 0n ? -left : left;
  let b = right < 0n ? -right : right;
  while (b !== 0n) [a, b] = [b, a % b];
  return a === 0n ? 1n : a;
}

function descriptor(file, knownHash = null) {
  return {
    path: file.replaceAll("\\", "/"), bytes: statSync(file).size,
    sha256: knownHash ?? `sha256:${createHash("sha256").update(readFileSync(file)).digest("hex")}`,
  };
}
function contentHash(value) {
  const clone = structuredClone(value);
  delete clone.content_sha256;
  return `sha256:${createHash("sha256").update(stableStringify(clone)).digest("hex")}`;
}
function stableStringify(value) {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  if (value && typeof value === "object") return `{${Object.keys(value).sort().map((key) =>
    `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`;
  return JSON.stringify(value);
}
function bodyCompare(left, right) {
  return left.ability_id - right.ability_id || left.hit_event_id - right.hit_event_id ||
    left.coefficient_basis_points - right.coefficient_basis_points ||
    left.fixed_parameter - right.fixed_parameter;
}
function numberCompare(left, right) { return left - right; }
function compareText(left, right) { return String(left).localeCompare(String(right), "en"); }
function safeInteger(value, label) {
  const result = Number(value);
  assert(Number.isSafeInteger(result), `${label} must be a safe integer`);
  return result;
}
function parseArgs(args) {
  const result = {};
  for (let index = 0; index < args.length; index += 1) {
    const key = args[index];
    assert(key.startsWith("--"), `Unexpected argument ${key}`);
    const value = args[index + 1];
    assert(value !== undefined && !value.startsWith("--"), `Missing value for ${key}`);
    result[key.slice(2)] = value;
    index += 1;
  }
  return result;
}
function required(value, key) { assert(value[key], `Missing --${key}`); return value[key]; }
function resolved(value) { return path.resolve(value); }
function assert(condition, message) { if (!condition) throw new Error(message); }
function usage(code) {
  console.log("Usage:\n  node tools/bpsr-cross-action-packet-final-solve.mjs build --cohort <json> --damage-stage <json> --output <json>\n  node tools/bpsr-cross-action-packet-final-solve.mjs verify --input <json>\n  node tools/bpsr-cross-action-packet-final-solve.mjs self-test");
  process.exit(code);
}
