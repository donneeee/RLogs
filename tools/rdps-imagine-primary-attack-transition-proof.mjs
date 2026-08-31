#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const GENERATOR = "tools/rdps-imagine-primary-attack-transition-proof.mjs";
const SCHEMA_VERSION = 1;
const EFFECT_ID = 2110140;
const PRIMARY_CURRENT_ID = 11030;
const PRIMARY_TOTAL_ID = 11031;
const PRIMARY_PERCENT_ID = 11034;
const ATTACK_CURRENT_ID = 11330;
const ATTACK_TOTAL_ID = 11331;
const ATTACK_ADD_ID = 11332;
const JOIN_TOLERANCE_MICROS = 250_000;

const [command, ...args] = process.argv.slice(2);
if (command === "build") {
  build(parseArguments(args));
} else if (command === "verify") {
  verify(readJson(path.resolve(required(parseArguments(args), "input"))));
} else {
  usage();
  process.exitCode = 2;
}

function build(options) {
  const gameBuild = required(options, "build");
  const windowPath = path.resolve(required(options, "tier-window-inputs"));
  const familyPath = path.resolve(required(options, "attribute-family-proof"));
  const staticTransformPath = path.resolve(required(options, "static-transform-proof"));
  const outputPath = path.resolve(required(options, "output"));
  const windowInputs = readJson(windowPath);
  const familyProof = readJson(familyPath);
  const staticTransformProof = readJson(staticTransformPath);

  requireExact(windowInputs.schema_version === 1, "tier-window schema");
  requireExact(String(windowInputs.game_build) === gameBuild, "tier-window build");
  requireExact(Number(windowInputs.effect_id) === EFFECT_ID, "tier-window effect");
  requireExact(windowInputs.topology?.allegiance_assumptions === false, "neutral input topology");
  requireExact(familyProof.schema_version === 6, "attribute-family schema");
  requireExact(
    Number(familyProof.cross_family_transition_selection?.observed_batches) === 20 &&
      Number(familyProof.cross_family_transition_selection?.complete_selected_family_batches) === 20 &&
      Number(familyProof.cross_family_transition_selection?.incomplete_selected_family_batches) === 0,
    "complete cross-family transition selection",
  );
  requireExact(String(staticTransformProof.game_build) === gameBuild, "static-transform build");
  requireExact(
    staticTransformProof.generated_by === "tools/primary-stat-attack-transform-proof.mjs",
    "static-transform generator",
  );
  const staticClass11 = staticTransformProof.families?.find(
    (entry) => String(entry.transform_family_id) === "11030->11332",
  );
  requireExact(Number(staticClass11?.primary_attribute_id) === PRIMARY_CURRENT_ID, "static class-11 primary id");
  requireExact(Number(staticClass11?.attack_add_attribute_id) === ATTACK_ADD_ID, "static class-11 attack-add id");
  requireExact(
    Number(staticClass11?.coefficient_basis_points) === 1250 &&
      Number(staticClass11?.fixed_point_denominator) === 10000,
    "static class-11 1/8 claim",
  );

  const transitions = flattenCrossFamilyExamples(familyProof);
  requireExact(transitions.length === 20, "unique retained anchored transition count");
  const windows = (windowInputs.lifecycle_windows ?? []).map((window) =>
    buildWindowProof(window, transitions)
  );
  requireExact(windows.length === 8, "exact tier-window count");

  const boundaryProofs = windows.flatMap((window) => [window.activation, window.deactivation]);
  const exactCurrentTransformMatches = boundaryProofs.filter(
    (boundary) => boundary.packet_transform_checks.primary_current_to_attack_add_58_over_100.exact,
  ).length;
  const staticTransformMatches = boundaryProofs.filter(
    (boundary) => boundary.packet_transform_checks.static_class_11_1_over_8.exact,
  ).length;
  const transformConfounders = boundaryProofs.filter(
    (boundary) => !boundary.packet_transform_checks.primary_current_to_attack_add_58_over_100.exact,
  );
  const delayedActivations = windows.filter((window) => window.activation.delay_from_status_micros > 0);
  const candidateActions = windows.reduce((sum, window) => sum + window.damage_action_classification.candidate_count, 0);
  const eligibleActions = windows.reduce((sum, window) => sum + window.damage_action_classification.effective_count, 0);
  const excludedBefore = windows.reduce(
    (sum, window) => sum + window.damage_action_classification.excluded_before_activation_count,
    0,
  );
  const excludedAfter = windows.reduce(
    (sum, window) => sum + window.damage_action_classification.excluded_at_or_after_deactivation_count,
    0,
  );

  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATOR,
    game_build: gameBuild,
    effect_id: EFFECT_ID,
    imagine_skill_id: 3971,
    topology: {
      effect_edge: "provider -> effect/status lifecycle -> recipient or enemy target",
      damage_edge: "recipient damage action -> recipient or enemy target",
      source_side_join: "effect affected entity equals damage actor",
      damage_endpoint_allegiance: "unresolved",
      allegiance_assumptions: false,
    },
    policy: {
      exact_numeric_ids_and_build_are_authoritative: true,
      localized_names_are_evidence_only: true,
      remote_player_cast_packets_required: false,
      remote_player_cast_packets_synthesized: false,
      status_lifecycle_is_not_the_effective_stat_window: true,
      effective_stat_window_uses_exact_attribute_transition_boundaries: true,
      same_packet_unrelated_changes_are_retained_as_confounders: true,
      damage_actions_are_counterfactual_inputs_only: true,
      integer_damage_stage_order_and_rounding_proven: false,
      ordinary_damage_totals_changed: false,
      observed_damage_reassigned_to_provider: "0",
      provider_rdps_credit_allowed: false,
      runtime_authority: false,
      ui_display_authority: false,
    },
    inputs: {
      tier_window_inputs: receipt(windowPath),
      attribute_family_proof: receipt(familyPath),
      static_class_transform_proof: receipt(staticTransformPath),
    },
    join_contract: {
      transition_identity:
        "same session, affected actor entity, exact signed attribute 11034 tier delta, unique transition within 250000 microseconds of the lifecycle boundary",
      activation_order:
        "attribute transition time must equal or follow status application time; equal-time packet sequence may precede the status row",
      deactivation_order:
        "attribute transition time must equal or precede status removal time",
      effective_damage_action:
        "same session and recipient actor; canonical source RLOG sequence is strictly after activation transition and strictly before deactivation transition",
    },
    packet_transform_proof: {
      scope: "class 11, attributes 11030/11031/11034 -> 11332, sixteen exact effect 2110140 lifecycle boundaries in four current-build sessions",
      proven_expression:
        "delta(attack_add_11332) = floor(after_primary_current_11030 * 58 / 100) - floor(before_primary_current_11030 * 58 / 100)",
      rounding: "floor on nonnegative integer packet values at each side before subtraction",
      exact_boundary_count: boundaryProofs.length,
      exact_58_over_100_matches: exactCurrentTransformMatches,
      unresolved_same_packet_confounders: transformConfounders.length,
      static_1_over_8_matches: staticTransformMatches,
      current_formula_artifact_requires_correction: staticTransformMatches !== boundaryProofs.length,
      static_talent_opcode_disposition:
        "the 1/8 talent opcode may remain valid in its own static route; it is disproven only as the complete packet marginal for effect 2110140",
      authority:
        "exact only for the fifteen matching class-11 packet boundaries; the attack-percent-co-transition boundary and all other classes, downstream damage stages, runtime attribution, and UI display remain unresolved",
    },
    summary: {
      exact_lifecycle_windows: windows.length,
      exact_attribute_boundaries: boundaryProofs.length,
      unique_attribute_boundary_joins: boundaryProofs.filter((boundary) => boundary.join_candidate_count === 1).length,
      delayed_attribute_activations: delayedActivations.length,
      maximum_activation_delay_micros: Math.max(...windows.map((window) => window.activation.delay_from_status_micros)),
      candidate_status_window_damage_actions: candidateActions,
      effective_stat_window_damage_actions: eligibleActions,
      excluded_before_attribute_activation: excludedBefore,
      excluded_at_or_after_attribute_deactivation: excludedAfter,
      effective_stat_window_hp_loss: sumWindowField(windows, "effective_hp_loss"),
      effective_stat_window_reported_damage: sumWindowField(windows, "effective_reported_damage"),
      observed_damage_reassigned_to_provider: "0",
    },
    lifecycle_windows: windows,
    remaining_proof_obligations: [
      "correct the current-build formula artifact so a static talent route cannot be mistaken for the complete effect 2110140 packet marginal",
      "retain and prove each effective damage action's exact downstream damage-stage fields, operation order, stacking, and integer rounding",
      "separate the owned attack-add marginal from any same-packet attack-percent or other confounding transition",
      "resolve tiers and recipient snapshots independently for the other 128 effect 2110140 applications",
      "prove recipient debit equals provider credit while preserving ordinary damage totals",
      "satisfy canonical-replay-conservation and protocol-event-coverage with the exact-build protocol-pack identity",
    ],
  };

  verify(report);
  fs.writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`);
  console.log(
    `wrote ${outputPath}: ${boundaryProofs.length} exact stat boundaries, ${eligibleActions}/${candidateActions} effective-window damage actions, zero provider credit`,
  );
}

function buildWindowProof(window, transitions) {
  requireExact(window.lifecycle_state === "exact-apply-remove", "exact lifecycle input");
  requireExact(window.window_input_state === "complete", "complete tier-window input");
  requireExact(Number(window.recipient_formula_input_snapshot?.class_id) === 11, "class-11 recipient");
  const units = Number(window.exact_attribute_pair?.main_stat_raw_percent_units);
  requireExact([750, 1500].includes(units), "exact tier raw-percent units");
  const actor = String(window.affected_entity_uuid);
  const activation = joinBoundary({
    kind: "activation",
    sessionId: window.session_id,
    actor,
    statusSequence: Number(window.application_sequence),
    statusMicros: Number(window.application_observed_micros),
    expectedPercentDelta: units,
    transitions,
  });
  const deactivation = joinBoundary({
    kind: "deactivation",
    sessionId: window.session_id,
    actor,
    statusSequence: Number(window.removal_sequence),
    statusMicros: Number(window.removal_observed_micros),
    expectedPercentDelta: -units,
    transitions,
  });
  requireExact(activation.transition.sequence < deactivation.transition.sequence, "ordered stat boundaries");

  const actions = window.damage_actions ?? [];
  const effective = [];
  const before = [];
  const after = [];
  for (const action of actions) {
    requireExact(String(action.damage_actor_entity_uuid) === actor, "damage actor identity");
    const sequence = Number(action.canonical_source_rlog_sequence ?? action.sequence);
    if (sequence <= activation.transition.sequence) before.push(action);
    else if (sequence >= deactivation.transition.sequence) after.push(action);
    else effective.push(action);
  }
  requireExact(effective.length + before.length + after.length === actions.length, "action partition conservation");
  return {
    session_id: window.session_id,
    run_ordinal: Number(window.run_ordinal),
    effect_id: EFFECT_ID,
    status_instance_id: Number(window.status_instance_id),
    provider_entity_uuid: String(window.provider_entity_uuid),
    affected_entity_uuid: actor,
    loadout_tier: Number(window.loadout_tier),
    exact_attribute_pair: window.exact_attribute_pair,
    status_lifecycle: {
      application_sequence: Number(window.application_sequence),
      application_observed_micros: Number(window.application_observed_micros),
      removal_sequence: Number(window.removal_sequence),
      removal_observed_micros: Number(window.removal_observed_micros),
    },
    activation,
    deactivation,
    effective_stat_window: {
      first_exclusive_canonical_source_rlog_sequence: activation.transition.sequence,
      last_exclusive_canonical_source_rlog_sequence: deactivation.transition.sequence,
      activation_observed_micros: activation.transition.observed_micros,
      deactivation_observed_micros: deactivation.transition.observed_micros,
    },
    damage_action_classification: {
      candidate_count: actions.length,
      effective_count: effective.length,
      excluded_before_activation_count: before.length,
      excluded_at_or_after_deactivation_count: after.length,
      effective_canonical_source_rlog_sequences: effective.map(actionSequence),
      excluded_before_activation_sequences: before.map(actionSequence),
      excluded_at_or_after_deactivation_sequences: after.map(actionSequence),
      effective_hp_loss: sumIntegerField(effective, "hp_loss"),
      effective_reported_damage: sumIntegerField(effective, "reported_amount"),
    },
    counterfactual_damage_delta: null,
    provider_rdps_credit: "0",
    provider_rdps_credit_allowed: false,
    blocker:
      "downstream damage-stage fields, operation order, stacking, and integer rounding are not yet proven",
  };
}

function joinBoundary({
  kind,
  sessionId,
  actor,
  statusSequence,
  statusMicros,
  expectedPercentDelta,
  transitions,
}) {
  const candidates = transitions.filter((transition) => {
    if (transition.session_id !== sessionId || String(transition.actor_entity_uuid) !== actor) return false;
    if (member(transition, PRIMARY_PERCENT_ID)?.delta !== expectedPercentDelta) return false;
    const deltaMicros = Number(transition.observed_micros) - statusMicros;
    return kind === "activation"
      ? deltaMicros >= 0 && deltaMicros <= JOIN_TOLERANCE_MICROS
      : deltaMicros <= 0 && deltaMicros >= -JOIN_TOLERANCE_MICROS;
  });
  requireExact(candidates.length === 1, `${sessionId} ${statusSequence} unique ${kind} transition`);
  const transition = candidates[0];
  const primaryCurrent = requiredMember(transition, PRIMARY_CURRENT_ID);
  const primaryTotal = requiredMember(transition, PRIMARY_TOTAL_ID);
  const primaryPercent = requiredMember(transition, PRIMARY_PERCENT_ID);
  const attackCurrent = requiredMember(transition, ATTACK_CURRENT_ID);
  const attackTotal = requiredMember(transition, ATTACK_TOTAL_ID);
  const attackAdd = requiredMember(transition, ATTACK_ADD_ID);
  requireExact(primaryCurrent.delta === primaryTotal.delta, `${kind} primary current/total delta`);
  requireExact(primaryPercent.delta === expectedPercentDelta, `${kind} primary percent delta`);
  const exact58 = transformDelta(primaryCurrent, 58, 100) === attackAdd.delta;
  const exact18 = transformDelta(primaryCurrent, 1, 8) === attackAdd.delta;
  const otherSamePacketChanges = transition.member_transitions.filter(
    (entry) => ![
      PRIMARY_CURRENT_ID,
      PRIMARY_TOTAL_ID,
      PRIMARY_PERCENT_ID,
      ATTACK_CURRENT_ID,
      ATTACK_TOTAL_ID,
      ATTACK_ADD_ID,
    ].includes(Number(entry.attribute_id)),
  );
  requireExact(
    exact58 || otherSamePacketChanges.some((entry) => Number(entry.attribute_id) === 11334),
    `${kind} exact 58/100 transform or retained attack-percent confounder`,
  );
  return {
    lifecycle_boundary: kind,
    status_sequence: statusSequence,
    status_observed_micros: statusMicros,
    delay_from_status_micros: Number(transition.observed_micros) - statusMicros,
    join_candidate_count: candidates.length,
    transition: compactTransition(transition),
    retained_family_members: {
      primary_current: primaryCurrent,
      primary_total: primaryTotal,
      primary_percent: primaryPercent,
      attack_current: attackCurrent,
      attack_total: attackTotal,
      attack_add: attackAdd,
      other_same_packet_changes: otherSamePacketChanges,
    },
    packet_transform_checks: {
      primary_current_to_attack_add_58_over_100: {
        expression: "floor(after_primary_current * 58 / 100) - floor(before_primary_current * 58 / 100)",
        predicted_delta: transformDelta(primaryCurrent, 58, 100),
        observed_delta: attackAdd.delta,
        exact: exact58,
      },
      static_class_11_1_over_8: {
        expression: "floor(after_primary_current * 1 / 8) - floor(before_primary_current * 1 / 8)",
        predicted_delta: transformDelta(primaryCurrent, 1, 8),
        observed_delta: attackAdd.delta,
        exact: exact18,
        disposition: "static talent-route candidate; not the complete observed effect 2110140 packet marginal",
      },
    },
  };
}

function flattenCrossFamilyExamples(familyProof) {
  const byKey = new Map();
  for (const pattern of familyProof.cross_family_transition_selection?.patterns ?? []) {
    for (const example of pattern.examples ?? []) {
      const key = [example.session_id, example.run_ordinal, example.sequence, example.actor_entity_uuid].join("|");
      if (!byKey.has(key)) byKey.set(key, example);
      else requireExact(JSON.stringify(byKey.get(key)) === JSON.stringify(example), `duplicate transition ${key}`);
    }
  }
  return [...byKey.values()];
}

function compactTransition(transition) {
  return {
    session_id: String(transition.session_id),
    run_ordinal: Number(transition.run_ordinal),
    sequence: Number(transition.sequence),
    observed_micros: Number(transition.observed_micros),
    actor_entity_uuid: String(transition.actor_entity_uuid),
    matched_anchors: transition.matched_anchors,
    changed_members: transition.changed_members,
  };
}

function member(transition, attributeId) {
  return transition.member_transitions?.find((entry) => Number(entry.attribute_id) === attributeId);
}

function requiredMember(transition, attributeId) {
  const result = member(transition, attributeId);
  requireExact(Boolean(result), `transition ${transition.sequence} member ${attributeId}`);
  for (const field of ["before", "after", "delta"]) {
    requireExact(Number.isSafeInteger(Number(result[field])), `member ${attributeId} ${field}`);
  }
  requireExact(Number(result.after) - Number(result.before) === Number(result.delta), `member ${attributeId} delta`);
  return {
    attribute_id: Number(result.attribute_id),
    semantic_suffix: String(result.semantic_suffix),
    before: Number(result.before),
    after: Number(result.after),
    delta: Number(result.delta),
  };
}

function transformDelta(transition, numerator, denominator) {
  requireExact(transition.before >= 0 && transition.after >= 0, "nonnegative transform inputs");
  return Math.floor(transition.after * numerator / denominator) -
    Math.floor(transition.before * numerator / denominator);
}

function verify(report) {
  requireExact(report.schema_version === SCHEMA_VERSION, "report schema");
  requireExact(report.generated_by === GENERATOR, "report generator");
  requireExact(Number(report.effect_id) === EFFECT_ID, "report effect");
  requireExact(report.topology?.allegiance_assumptions === false, "neutral topology");
  requireExact(
    report.topology?.effect_edge === "provider -> effect/status lifecycle -> recipient or enemy target" &&
      report.topology?.damage_edge === "recipient damage action -> recipient or enemy target",
    "neutral topology edges",
  );
  requireExact(report.policy?.remote_player_cast_packets_required === false, "remote cast policy");
  requireExact(report.policy?.provider_rdps_credit_allowed === false, "credit policy");
  requireExact(report.policy?.ordinary_damage_totals_changed === false, "ordinary damage conservation");
  requireExact(report.policy?.integer_damage_stage_order_and_rounding_proven === false, "damage fail closed");
  requireExact(report.packet_transform_proof?.exact_boundary_count === 16, "boundary count");
  requireExact(report.packet_transform_proof?.exact_58_over_100_matches === 15, "58/100 exact matches");
  requireExact(report.packet_transform_proof?.unresolved_same_packet_confounders === 1, "transform confounder count");
  requireExact(report.packet_transform_proof?.static_1_over_8_matches === 0, "1/8 contradiction");
  requireExact(report.packet_transform_proof?.current_formula_artifact_requires_correction === true, "formula correction state");
  requireExact(Number(report.summary?.exact_lifecycle_windows) === 8, "window count");
  requireExact(Number(report.summary?.exact_attribute_boundaries) === 16, "attribute boundary count");
  requireExact(Number(report.summary?.unique_attribute_boundary_joins) === 16, "unique joins");
  requireExact(Number(report.summary?.delayed_attribute_activations) === 3, "delayed activation count");
  requireExact(Number(report.summary?.maximum_activation_delay_micros) === 90407, "maximum activation delay");
  const windows = report.lifecycle_windows ?? [];
  requireExact(windows.length === 8, "retained window count");
  let candidateCount = 0;
  let effectiveCount = 0;
  let beforeCount = 0;
  let afterCount = 0;
  for (const window of windows) {
    for (const boundary of [window.activation, window.deactivation]) {
      requireExact(boundary.join_candidate_count === 1, "unique boundary join");
      const exact58 =
        boundary.packet_transform_checks?.primary_current_to_attack_add_58_over_100?.exact === true;
      const retainedAttackPercentConfounder = boundary.retained_family_members?.other_same_packet_changes
        ?.some((entry) => Number(entry.attribute_id) === 11334);
      requireExact(exact58 || retainedAttackPercentConfounder, "exact 58/100 or retained confounded boundary");
      requireExact(
        boundary.packet_transform_checks?.static_class_11_1_over_8?.exact === false,
        "contradicted 1/8 boundary",
      );
    }
    const classification = window.damage_action_classification;
    const effective = classification.effective_canonical_source_rlog_sequences ?? [];
    const before = classification.excluded_before_activation_sequences ?? [];
    const after = classification.excluded_at_or_after_deactivation_sequences ?? [];
    requireExact(effective.length === classification.effective_count, "effective sequence count");
    requireExact(before.length === classification.excluded_before_activation_count, "before sequence count");
    requireExact(after.length === classification.excluded_at_or_after_deactivation_count, "after sequence count");
    requireExact(
      effective.length + before.length + after.length === classification.candidate_count,
      "classified action conservation",
    );
    requireExact(
      effective.every(
        (sequence) => sequence > window.effective_stat_window.first_exclusive_canonical_source_rlog_sequence &&
          sequence < window.effective_stat_window.last_exclusive_canonical_source_rlog_sequence,
      ),
      "effective action boundary order",
    );
    requireExact(window.provider_rdps_credit === "0" && window.provider_rdps_credit_allowed === false, "window credit state");
    candidateCount += classification.candidate_count;
    effectiveCount += classification.effective_count;
    beforeCount += classification.excluded_before_activation_count;
    afterCount += classification.excluded_at_or_after_deactivation_count;
  }
  requireExact(candidateCount === Number(report.summary.candidate_status_window_damage_actions), "candidate action total");
  requireExact(effectiveCount === Number(report.summary.effective_stat_window_damage_actions), "effective action total");
  requireExact(beforeCount === Number(report.summary.excluded_before_attribute_activation), "before action total");
  requireExact(afterCount === Number(report.summary.excluded_at_or_after_attribute_deactivation), "after action total");
  requireExact(report.summary.observed_damage_reassigned_to_provider === "0", "zero provider reassignment");
  console.log(
    `verified effect ${EFFECT_ID} class-11 transition proof for build ${report.game_build}: 15/16 exact 58/100 boundaries with one retained attack-percent confounder, ${effectiveCount}/${candidateCount} effective-window actions, zero provider credit`,
  );
  return report;
}

function actionSequence(action) {
  return Number(action.canonical_source_rlog_sequence ?? action.sequence);
}

function sumWindowField(windows, field) {
  return windows.reduce(
    (sum, window) => sum + BigInt(window.damage_action_classification[field] ?? "0"),
    0n,
  ).toString();
}

function sumIntegerField(rows, field) {
  return rows.reduce((sum, row) => sum + BigInt(row[field] ?? "0"), 0n).toString();
}

function receipt(filePath) {
  const bytes = fs.readFileSync(filePath);
  return {
    path: filePath.replaceAll("\\", "/"),
    bytes: bytes.length,
    sha256: crypto.createHash("sha256").update(bytes).digest("hex"),
  };
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function parseArguments(values) {
  const result = {};
  for (let index = 0; index < values.length; index += 2) {
    const key = values[index];
    const value = values[index + 1];
    if (!key?.startsWith("--") || value == null) {
      usage();
      process.exit(2);
    }
    result[key.slice(2)] = value;
  }
  return result;
}

function required(options, key) {
  if (!options[key]) throw new Error(`missing --${key}`);
  return options[key];
}

function requireExact(condition, label) {
  if (!condition) throw new Error(`${label} does not match the exact proof contract`);
}

function usage() {
  console.log(`Usage:
  node ${GENERATOR} build --build <id> --tier-window-inputs <json> --attribute-family-proof <json> --static-transform-proof <json> --output <json>
  node ${GENERATOR} verify --input <json>`);
}
