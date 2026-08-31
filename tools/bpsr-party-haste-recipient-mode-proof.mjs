#!/usr/bin/env node

import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

const EXPECTED_PROOF_SCHEMA = 29;
const EXPECTED_GENERATOR = "rlogs-bpsr-rdps-status-attribute-proof";
const EXPECTED_DEPLOYMENT = "global";
const DEFAULT_EFFECT_ID = 31602;
const SPEED_ATTRIBUTES = new Map([
  [11720, "attack_speed_pct_final"],
  [11721, "attack_speed_pct_total"],
  [11722, "attack_speed_pct_add"],
  [11730, "cast_speed_pct_final"],
  [11731, "cast_speed_pct_total"],
  [11732, "cast_speed_pct_add"],
  [11740, "charge_speed_pct_final"],
  [11741, "charge_speed_pct_total"],
  [11742, "charge_speed_pct_add"],
  [11930, "haste_pct_final"],
  [11931, "haste_pct_total"],
  [11932, "haste_pct_add"],
]);

function fail(message) {
  throw new Error(message);
}

function takeValue(values, flag) {
  const index = values.indexOf(flag);
  if (index < 0 || index + 1 >= values.length) fail(`${flag} requires a value`);
  const value = values[index + 1];
  values.splice(index, 2);
  return value;
}

function parseArguments() {
  const values = process.argv.slice(2);
  const proof = path.resolve(takeValue(values, "--proof"));
  const output = path.resolve(takeValue(values, "--output"));
  const build = takeValue(values, "--build");
  const effectIndex = values.indexOf("--effect");
  let effectId = DEFAULT_EFFECT_ID;
  if (effectIndex >= 0) {
    if (effectIndex + 1 >= values.length) fail("--effect requires an integer");
    effectId = Number(values[effectIndex + 1]);
    values.splice(effectIndex, 2);
  }
  if (!Number.isSafeInteger(effectId) || values.length) {
    fail("usage: bpsr-party-haste-recipient-mode-proof --proof <schema29.json> --build <client-build> [--effect <id>] --output <audit.json>");
  }
  return { proof, output, build, effectId };
}

function descriptor(file, bytes) {
  return {
    file,
    bytes: bytes.length,
    sha256: createHash("sha256").update(bytes).digest("hex"),
  };
}

function compareText(left, right) {
  return String(left).localeCompare(String(right), "en");
}

function modeKey(example) {
  return JSON.stringify([
    example.target_kind ?? null,
    example.target_class_id ?? null,
    example.target_specialization_id ?? null,
  ]);
}

function analyze(proof, effectId) {
  const groups = new Map();
  let selectedEquationCount = 0;
  let selectedOccurrenceCount = 0;
  let occurrenceExamplesComplete = true;

  for (const report of proof.wire_additive_equation_systems ?? []) {
    const attributeId = Number(report.attribute_id);
    if (!SPEED_ATTRIBUTES.has(attributeId)) continue;
    for (const equation of report.equations ?? []) {
      if (equation.terms?.length !== 1 || Number(equation.terms[0]?.effect_id) !== effectId) {
        continue;
      }
      selectedEquationCount += 1;
      const occurrenceCount = Number(equation.count);
      const examples = equation.examples ?? [];
      selectedOccurrenceCount += occurrenceCount;
      if (examples.length !== occurrenceCount) occurrenceExamplesComplete = false;
      const sign = Number(equation.terms[0].signed_presence_delta);
      const coefficient = Number(equation.raw_attribute_delta) * sign;
      for (const example of examples) {
        const status = (example.status_instances ?? []).find(
          (entry) => Number(entry.effect_id) === effectId,
        );
        if (!status) fail(`equation example is missing effect ${effectId}`);
        const key = `${attributeId}:${modeKey(example)}`;
        let group = groups.get(key);
        if (!group) {
          group = {
            attribute_id: attributeId,
            attribute_surface: SPEED_ATTRIBUTES.get(attributeId),
            recipient_kind: example.target_kind ?? null,
            recipient_class_id: example.target_class_id ?? null,
            recipient_specialization_id: example.target_specialization_id ?? null,
            normalized_coefficient_counts: new Map(),
            apply_occurrences: 0,
            remove_occurrences: 0,
            independent_run_contexts: new Set(),
            target_entity_uuids: new Set(),
            source_entity_uuids: new Set(),
            cross_actor_occurrences: 0,
            examples: [],
          };
          groups.set(key, group);
        }
        group.normalized_coefficient_counts.set(
          coefficient,
          (group.normalized_coefficient_counts.get(coefficient) ?? 0) + 1,
        );
        if (sign > 0) group.apply_occurrences += 1;
        else group.remove_occurrences += 1;
        group.independent_run_contexts.add(`${example.session_id}:${example.run_ordinal}`);
        group.target_entity_uuids.add(String(example.target_entity_uuid));
        if (status.source_entity_uuid !== null && status.source_entity_uuid !== undefined) {
          group.source_entity_uuids.add(String(status.source_entity_uuid));
          if (String(status.source_entity_uuid) !== String(example.target_entity_uuid)) {
            group.cross_actor_occurrences += 1;
          }
        }
        if (group.examples.length < 4) {
          group.examples.push({
            session_id: example.session_id,
            run_ordinal: Number(example.run_ordinal),
            wire_capture_sequence: Number(example.wire_capture_sequence),
            target_actor_sequence: example.target_actor_sequence ?? null,
            target_entity_uuid: String(example.target_entity_uuid),
            source_entity_uuid:
              status.source_entity_uuid === null || status.source_entity_uuid === undefined
                ? null
                : String(status.source_entity_uuid),
            state: status.state,
            raw_attribute_delta: Number(equation.raw_attribute_delta),
            normalized_coefficient: coefficient,
          });
        }
      }
    }
  }

  const modes = [...groups.values()].map((group) => {
    const coefficientCounts = Object.fromEntries(
      [...group.normalized_coefficient_counts.entries()].sort((a, b) => a[0] - b[0]),
    );
    const coefficientIsConstant = group.normalized_coefficient_counts.size === 1;
    const mirrored = group.apply_occurrences > 0 && group.remove_occurrences > 0;
    const repeated = group.independent_run_contexts.size >= 2;
    const metadataComplete =
      group.recipient_kind !== null &&
      group.recipient_class_id !== null &&
      group.recipient_specialization_id !== null;
    const proofState = !occurrenceExamplesComplete
      ? "insufficient_truncated_occurrence_examples"
      : !metadataComplete
        ? "insufficient_recipient_metadata"
        : !coefficientIsConstant
          ? "contradicted_nonconstant_recipient_mode_coefficient"
          : !mirrored
            ? "insufficient_missing_apply_remove_mirror_for_recipient_mode"
            : !repeated
              ? "insufficient_independent_run_contexts_for_recipient_mode"
              : "proven_reversible_recipient_mode_coefficient";
    return {
      attribute_id: group.attribute_id,
      attribute_surface: group.attribute_surface,
      recipient_kind: group.recipient_kind,
      recipient_class_id: group.recipient_class_id,
      recipient_specialization_id: group.recipient_specialization_id,
      proof_state: proofState,
      proven_coefficient_units:
        proofState === "proven_reversible_recipient_mode_coefficient"
          ? Number(group.normalized_coefficient_counts.keys().next().value)
          : null,
      normalized_coefficient_counts: coefficientCounts,
      apply_occurrences: group.apply_occurrences,
      remove_occurrences: group.remove_occurrences,
      independent_run_contexts: group.independent_run_contexts.size,
      target_entity_uuids: [...group.target_entity_uuids].sort(compareText),
      source_entity_uuids: [...group.source_entity_uuids].sort(compareText),
      cross_actor_occurrences: group.cross_actor_occurrences,
      conditional_formula_authority: false,
      runtime_eligible_for_rdps: false,
      blocker:
        proofState === "proven_reversible_recipient_mode_coefficient"
          ? "the raw recipient-mode stat coefficient is exact, but class-conditioned game formula semantics, stage selection, operation order, stacking, integer rounding, and the damage opportunity counterfactual remain open"
          : "the exact recipient mode does not yet have a constant reversible coefficient repeated across at least two recording-run contexts",
      examples: group.examples,
    };
  });
  modes.sort((left, right) =>
    left.attribute_id - right.attribute_id ||
    Number(left.recipient_class_id ?? -1) - Number(right.recipient_class_id ?? -1) ||
    Number(left.recipient_specialization_id ?? -1) -
      Number(right.recipient_specialization_id ?? -1),
  );
  return {
    selected_single_term_equations: selectedEquationCount,
    selected_single_term_occurrences: selectedOccurrenceCount,
    all_selected_occurrences_have_retained_examples: occurrenceExamplesComplete,
    recipient_modes: modes,
  };
}

function main() {
  const args = parseArguments();
  const bytes = readFileSync(args.proof);
  const proof = JSON.parse(bytes);
  if (
    Number(proof.schema_version) !== EXPECTED_PROOF_SCHEMA ||
    proof.generated_by !== EXPECTED_GENERATOR
  ) {
    fail(`proof must be ${EXPECTED_GENERATOR} schema ${EXPECTED_PROOF_SCHEMA}`);
  }
  if (
    proof.expected_deployment_id !== EXPECTED_DEPLOYMENT ||
    String(proof.expected_game_build) !== args.build
  ) {
    fail("proof deployment/build identity does not match the requested exact build");
  }
  if (
    JSON.stringify(proof.selected_effect_ids?.map(Number)) !== JSON.stringify([args.effectId]) ||
    JSON.stringify(proof.reported_effect_ids?.map(Number)) !== JSON.stringify([args.effectId])
  ) {
    fail("proof does not select exactly the requested effect ID");
  }
  const analysis = analyze(proof, args.effectId);
  const proven = analysis.recipient_modes.filter(
    (mode) => mode.proof_state === "proven_reversible_recipient_mode_coefficient",
  );
  const result = {
    schema_version: 1,
    generated_by: "tools/bpsr-party-haste-recipient-mode-proof.mjs",
    deployment_id: EXPECTED_DEPLOYMENT,
    game_build: args.build,
    effect_id: args.effectId,
    input: descriptor(args.proof, bytes),
    policy: {
      exact_numeric_effect_and_attribute_ids_are_authoritative: true,
      localized_names_are_runtime_keys: false,
      actor_metadata_is_event_time_and_recording_local: true,
      cross_recording_snapshot_substitution_allowed: false,
      remote_player_cast_packets_required: false,
      missing_or_unobserved_values_are_zero: false,
      formula_inference: false,
      ordinary_damage_is_retained: true,
      provider_rdps_credit_allowed: false,
    },
    ...analysis,
    summary: {
      recipient_mode_count: analysis.recipient_modes.length,
      proven_reversible_recipient_mode_count: proven.length,
      proven_reversible_recipient_modes: proven.map((mode) => ({
        attribute_id: mode.attribute_id,
        recipient_class_id: mode.recipient_class_id,
        recipient_specialization_id: mode.recipient_specialization_id,
        coefficient_units: mode.proven_coefficient_units,
      })),
      exact_class_conditioned_formula_semantics_proven: false,
      exact_damage_opportunity_counterfactual_proven: false,
      runtime_promotion_allowed: false,
      observed_damage_reassigned_to_provider: 0,
    },
  };
  writeFileSync(args.output, `${JSON.stringify(result, null, 2)}\n`);
  process.stdout.write(`wrote ${args.output}\n`);
}

main();
