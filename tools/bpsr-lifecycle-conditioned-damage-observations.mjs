#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const diagnosticPath = process.argv[2] ? path.resolve(process.argv[2]) : null;
const ancestryPath = process.argv[3] ? path.resolve(process.argv[3]) : null;
const outputPath = process.argv[4] ? path.resolve(process.argv[4]) : null;
if (!diagnosticPath || !ancestryPath || !outputPath) {
  throw new Error(
    "Usage: node tools/bpsr-lifecycle-conditioned-damage-observations.mjs " +
      "<selected-action-diagnostic.json> <lifecycle-ancestry-proof.json> <output.json>",
  );
}
if (fs.existsSync(outputPath) || fs.existsSync(`${outputPath}.partial`)) {
  throw new Error(`Refusing to overwrite output or partial output: ${outputPath}`);
}

const diagnostic = readJson(diagnosticPath);
const ancestry = readJson(ancestryPath);
const observations = diagnostic.post_base_integer_factor_diagnostic
  ?.source_stage_order_diagnostic?.observations;
validateInputs(diagnostic, ancestry, observations);

const receiptByKey = new Map();
for (const receipt of ancestry.exact_damage_surface_receipts) {
  const key = observationKey(receipt.session_id, receipt.sequence);
  if (receiptByKey.has(key)) throw new Error(`Duplicate lifecycle receipt ${key}`);
  receiptByKey.set(key, receipt);
}

const joined = [];
const observedReceiptKeys = new Set();
const mismatchCounts = Object.create(null);
for (const observation of observations) {
  const key = observationKey(observation.session_id, observation.sequence);
  const receipt = receiptByKey.get(key);
  if (!receipt) continue;
  observedReceiptKeys.add(key);
  compareIdentity(observation, receipt, mismatchCounts);
  joined.push({
    ...observation,
    lifecycle: receipt.nearest_transition,
    nearest_consumed_lifecycle: receipt.nearest_consumed_transition,
  });
}

if (Object.keys(mismatchCounts).length > 0) {
  throw new Error(`Joined identity mismatch: ${JSON.stringify(mismatchCounts)}`);
}

const contextDiagnostics = {
  source_stage_vector_only: summarizeContexts(joined, (row) => [
    row.base,
    stableRawVector(row.raw_values_by_attribute_id),
  ]),
  source_stage_vector_plus_lifecycle: summarizeContexts(joined, (row) => [
    row.base,
    stableRawVector(row.raw_values_by_attribute_id),
    row.lifecycle?.source_config_id ?? "<null>",
    row.lifecycle?.status_state ?? "<null>",
    row.lifecycle?.status_stacks ?? "<null>",
  ]),
  source_stage_vector_lifecycle_and_target_identity: summarizeContexts(joined, (row) => [
    row.base,
    stableRawVector(row.raw_values_by_attribute_id),
    row.lifecycle?.source_config_id ?? "<null>",
    row.lifecycle?.status_state ?? "<null>",
    row.lifecycle?.status_stacks ?? "<null>",
    row.target_entity_uuid,
  ]),
  source_stage_vector_lifecycle_target_and_status_state_ids: summarizeContexts(
    joined,
    (row) => [
      row.base,
      stableRawVector(row.raw_values_by_attribute_id),
      row.lifecycle?.source_config_id ?? "<null>",
      row.lifecycle?.status_state ?? "<null>",
      row.lifecycle?.status_stacks ?? "<null>",
      row.target_entity_uuid,
      row.source_status_state_id,
      row.target_status_state_id,
    ],
  ),
  complete_retained_state_ids_plus_lifecycle: summarizeContexts(joined, (row) => [
    row.base,
    stableRawVector(row.raw_values_by_attribute_id),
    row.lifecycle?.source_config_id ?? "<null>",
    row.lifecycle?.status_state ?? "<null>",
    row.lifecycle?.status_stacks ?? "<null>",
    row.target_entity_uuid,
    row.source_attribute_state_id,
    row.target_attribute_state_id,
    row.source_status_state_id,
    row.target_status_state_id,
  ]),
  complete_retained_state_and_packet_calculation_context: summarizeContexts(
    joined,
    (row) => [
      row.base,
      stableRawVector(row.raw_values_by_attribute_id),
      row.lifecycle?.source_config_id ?? "<null>",
      row.lifecycle?.status_state ?? "<null>",
      row.lifecycle?.status_stacks ?? "<null>",
      row.target_entity_uuid,
      row.source_attribute_state_id,
      row.target_attribute_state_id,
      row.source_status_state_id,
      row.target_status_state_id,
      row.calculation_context,
      row.owner_stage_context,
    ],
  ),
};
const packetContextFieldDiagnostics = summarizePacketContextFields(joined);
const conflictingContextDiagnostics = summarizeConflictingContexts(joined);

const report = {
  schema_version: 1,
  generated_by: "rlogs-bpsr-lifecycle-conditioned-damage-observations",
  game_build: "24687926",
  selection: {
    damage_attr_id: 2220352105,
    action_id: 2203521,
    hit_event_id: 5,
    coefficient_basis_points: 20000,
    effect_id: 2203521,
  },
  inputs: {
    selected_action_diagnostic: receipt(diagnosticPath),
    lifecycle_damage_ancestry_proof: receipt(ancestryPath),
  },
  policy: {
    exact_session_and_canonical_sequence_join_only: true,
    source_and_target_identity_must_match: true,
    reported_damage_amount_must_match: true,
    affected_entity_and_damage_target_allegiance_are_not_assumed: true,
    missing_lifecycle_receipts_are_omitted_not_zero_filled: true,
    nearest_lifecycle_transition_is_diagnostic_proximity_only: true,
    lifecycle_proximity_grants_causal_ancestry: false,
    formula_authority: false,
    runtime_authority: false,
    provider_rdps_credit_allowed: false,
    packet_container_component_index_grants_formula_authority: false,
  },
  summary: {
    source_stage_observation_count: observations.length,
    lifecycle_receipt_count: receiptByKey.size,
    exact_identity_join_count: joined.length,
    source_stage_observations_without_lifecycle_receipt_count:
      observations.length - joined.length,
    lifecycle_receipts_without_source_stage_observation_count:
      receiptByKey.size - observedReceiptKeys.size,
    lifecycle_status_state_counts: countBy(joined,
      (row) => row.lifecycle?.status_state ?? "<null>"),
    lifecycle_source_config_counts: countBy(joined,
      (row) => row.lifecycle?.source_config_id ?? "<null>"),
    lifecycle_relationship_role_counts: countBy(joined,
      (row) => row.lifecycle?.relationship_roles?.join("+") ?? "<null>"),
    lifecycle_provider_equals_damage_actor_counts: countBy(joined,
      (row) => String(row.lifecycle?.provider_equals_damage_actor ?? "<null>")),
  },
  context_diagnostics: contextDiagnostics,
  packet_context_field_diagnostics: packetContextFieldDiagnostics,
  conflicting_context_diagnostics: conflictingContextDiagnostics,
  observations: joined,
  conclusion: {
    exact_event_time_lifecycle_context_joined: joined.length > 0,
    lifecycle_context_eliminates_all_repeated_context_output_conflicts:
      contextDiagnostics.source_stage_vector_plus_lifecycle
        .conflicting_repeated_context_count === 0,
    target_identity_eliminates_all_repeated_context_output_conflicts:
      contextDiagnostics.source_stage_vector_lifecycle_and_target_identity
        .conflicting_repeated_context_count === 0,
    retained_status_state_ids_eliminate_all_repeated_context_output_conflicts:
      contextDiagnostics.source_stage_vector_lifecycle_target_and_status_state_ids
        .conflicting_repeated_context_count === 0,
    complete_retained_state_ids_eliminate_all_repeated_context_output_conflicts:
      contextDiagnostics.complete_retained_state_ids_plus_lifecycle
        .conflicting_repeated_context_count === 0,
    packet_calculation_context_eliminates_all_repeated_context_output_conflicts:
      contextDiagnostics.complete_retained_state_and_packet_calculation_context
        .conflicting_repeated_context_count === 0,
    packet_calculation_context_repeated_control_witnesses_available:
      contextDiagnostics.complete_retained_state_and_packet_calculation_context
        .repeated_context_count > 0,
    event_unique_context_fragmentation_is_formula_proof: false,
    individually_discriminating_packet_context_fields:
      packetContextFieldDiagnostics.add_one_field
        .filter((row) => row.conflicting_repeated_context_count <
          contextDiagnostics.complete_retained_state_ids_plus_lifecycle
            .conflicting_repeated_context_count)
        .map((row) => row.field),
    fields_whose_omission_restores_repeated_contexts:
      packetContextFieldDiagnostics.leave_one_field_out
        .filter((row) => row.repeated_context_count > 0)
        .map((row) => row.field),
    component_index_is_proven_damage_formula_input: false,
    causal_lifecycle_to_damage_formula_proven: false,
    exact_damage_formula_proven: false,
    provider_rdps_credit_allowed: false,
  },
};

fs.mkdirSync(path.dirname(outputPath), { recursive: true });
const partialPath = `${outputPath}.partial`;
fs.writeFileSync(partialPath, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
fs.renameSync(partialPath, outputPath);
process.stdout.write(`${JSON.stringify({
  summary: report.summary,
  context_diagnostics: report.context_diagnostics,
  conclusion: report.conclusion,
}, null, 2)}\n`);

function validateInputs(diagnostic, ancestry, observations) {
  const stage = diagnostic.post_base_integer_factor_diagnostic
    ?.source_stage_order_diagnostic;
  if (
    Number(diagnostic.schema_version) !== 2 ||
    diagnostic.generated_by !==
      "rlogs-bpsr-target-mitigation-transform-proof:selected-ability-diagnostic" ||
    String(diagnostic.game_build) !== "24687926" ||
    JSON.stringify(diagnostic.selection?.ability_ids) !== JSON.stringify([2203521]) ||
    Number(diagnostic.selection?.hit_event_id) !== 5 ||
    Number(diagnostic.selection?.coefficient_basis_points) !== 20000 ||
    stage?.formula_authority !== false ||
    stage?.runtime_authority !== false ||
    stage?.provider_rdps_credit_allowed !== false ||
    !Array.isArray(observations) ||
    observations.length !== Number(stage?.observation_count) ||
    observations.some((row) =>
      !Number.isSafeInteger(Number(row.source_attribute_state_id)) ||
      !Number.isSafeInteger(Number(row.target_attribute_state_id)) ||
      !Number.isSafeInteger(Number(row.source_status_state_id)) ||
      !Number.isSafeInteger(Number(row.target_status_state_id)) ||
      row.calculation_context == null ||
      row.owner_stage_context == null)
  ) {
    throw new Error("Selected-action diagnostic is unsafe or incomplete");
  }
  if (
    Number(ancestry.schema_version) !== 1 ||
    ancestry.generated_by !== "rlogs-bpsr-buff-lifecycle-damage-ancestry-proof" ||
    String(ancestry.game_build) !== "24687926" ||
    Number(ancestry.selection?.effect_id) !== 2203521 ||
    Number(ancestry.selection?.action_id) !== 2203521 ||
    Number(ancestry.selection?.exact_damage_surface?.hit_event_id) !== 5 ||
    ancestry.policy?.affected_entity_allegiance_is_assumed !== false ||
    ancestry.policy?.damage_target_allegiance_is_assumed !== false ||
    ancestry.policy?.proximity_grants_causal_ancestry !== false ||
    ancestry.policy?.formula_authority !== false ||
    ancestry.policy?.runtime_authority !== false ||
    ancestry.policy?.provider_rdps_credit_allowed !== false ||
    !Array.isArray(ancestry.exact_damage_surface_receipts) ||
    ancestry.exact_damage_surface_receipts.length !==
      Number(ancestry.summary?.exact_damage_surface_count)
  ) {
    throw new Error("Lifecycle ancestry proof is unsafe or incomplete");
  }
}

function compareIdentity(observation, receipt, mismatches) {
  if (Number(observation.output) !== Number(receipt.reported_amount)) {
    increment(mismatches, "reported_amount");
  }
  if (String(observation.source_entity_uuid) !== String(receipt.damage_actor_entity_uuid)) {
    increment(mismatches, "source_entity_uuid");
  }
  if (String(observation.target_entity_uuid) !== String(receipt.damage_target_entity_uuid)) {
    increment(mismatches, "target_entity_uuid");
  }
}

function summarizeContexts(rows, parts) {
  const contexts = new Map();
  for (const row of rows) {
    const key = JSON.stringify(parts(row));
    let context = contexts.get(key);
    if (!context) {
      context = { observation_count: 0, outputs: new Set() };
      contexts.set(key, context);
    }
    context.observation_count += 1;
    context.outputs.add(Number(row.output));
  }
  let repeatedContextCount = 0;
  let repeatedObservationCount = 0;
  let conflictingRepeatedContextCount = 0;
  let conflictingRepeatedObservationCount = 0;
  let maximumDistinctOutputs = 0;
  for (const context of contexts.values()) {
    maximumDistinctOutputs = Math.max(maximumDistinctOutputs, context.outputs.size);
    if (context.observation_count < 2) continue;
    repeatedContextCount += 1;
    repeatedObservationCount += context.observation_count;
    if (context.outputs.size > 1) {
      conflictingRepeatedContextCount += 1;
      conflictingRepeatedObservationCount += context.observation_count;
    }
  }
  return {
    context_count: contexts.size,
    repeated_context_count: repeatedContextCount,
    repeated_observation_count: repeatedObservationCount,
    conflicting_repeated_context_count: conflictingRepeatedContextCount,
    conflicting_repeated_observation_count: conflictingRepeatedObservationCount,
    maximum_distinct_outputs_in_one_context: maximumDistinctOutputs,
  };
}

function summarizePacketContextFields(rows) {
  const fields = [...new Set(rows.flatMap((row) => [
    ...Object.keys(row.calculation_context ?? {})
      .map((key) => `calculation_context.${key}`),
    ...Object.keys(row.owner_stage_context ?? {})
      .map((key) => `owner_stage_context.${key}`),
  ]))].sort((a, b) => a.localeCompare(b, "en"));
  const addOneField = fields.map((field) => ({
    field,
    ...summarizeContexts(rows, (row) => [
      ...completeRetainedContextParts(row),
      packetContextValue(row, field),
    ]),
  }));
  const leaveOneFieldOut = fields.map((omittedField) => ({
    field: omittedField,
    ...summarizeContexts(rows, (row) => [
      ...completeRetainedContextParts(row),
      ...fields
        .filter((field) => field !== omittedField)
        .map((field) => packetContextValue(row, field)),
    ]),
  }));
  return {
    field_count: fields.length,
    fields,
    baseline_without_packet_context:
      summarizeContexts(rows, completeRetainedContextParts),
    add_one_field: addOneField,
    leave_one_field_out: leaveOneFieldOut,
  };
}

function summarizeConflictingContexts(rows) {
  const contexts = new Map();
  for (const row of rows) {
    const key = JSON.stringify(completeRetainedContextParts(row));
    let context = contexts.get(key);
    if (!context) {
      context = [];
      contexts.set(key, context);
    }
    context.push(row);
  }
  let conflictingContextCount = 0;
  let contextsWithDistinctComponentIndexes = 0;
  let positiveAdjacentOutputDeltas = 0;
  let negativeAdjacentOutputDeltas = 0;
  let zeroAdjacentOutputDeltas = 0;
  const outputRatioCounts = new Map();
  const examples = [];
  for (const context of contexts.values()) {
    const outputs = new Set(context.map((row) => Number(row.output)));
    if (context.length < 2 || outputs.size < 2) continue;
    conflictingContextCount += 1;
    const indexes = context.map((row) =>
      row.calculation_context?.skill_effect_component_index ?? null);
    if (new Set(indexes.map(String)).size === context.length) {
      contextsWithDistinctComponentIndexes += 1;
    }
    const ordered = [...context].sort((left, right) =>
      Number(left.calculation_context?.skill_effect_component_index ?? -1) -
      Number(right.calculation_context?.skill_effect_component_index ?? -1));
    for (let index = 1; index < ordered.length; index += 1) {
      const delta = Number(ordered[index].output) - Number(ordered[index - 1].output);
      if (delta > 0) positiveAdjacentOutputDeltas += 1;
      else if (delta < 0) negativeAdjacentOutputDeltas += 1;
      else zeroAdjacentOutputDeltas += 1;
    }
    const distinctOutputs = [...outputs].sort((a, b) => a - b);
    for (let left = 0; left < distinctOutputs.length; left += 1) {
      for (let right = left + 1; right < distinctOutputs.length; right += 1) {
        const divisor = greatestCommonDivisor(distinctOutputs[left], distinctOutputs[right]);
        const key = `${distinctOutputs[left] / divisor}/${distinctOutputs[right] / divisor}`;
        outputRatioCounts.set(key, (outputRatioCounts.get(key) ?? 0) + 1);
      }
    }
    if (examples.length < 16) {
      examples.push({
        base: context[0].base,
        raw_values_by_attribute_id: context[0].raw_values_by_attribute_id,
        target_entity_uuid: context[0].target_entity_uuid,
        lifecycle: context[0].lifecycle,
        source_attribute_state_id: context[0].source_attribute_state_id,
        target_attribute_state_id: context[0].target_attribute_state_id,
        source_status_state_id: context[0].source_status_state_id,
        target_status_state_id: context[0].target_status_state_id,
        rows: ordered.map((row) => ({
          session_id: row.session_id,
          sequence: row.sequence,
          output: row.output,
          skill_effect_group_index:
            row.calculation_context?.skill_effect_group_index ?? null,
          skill_effect_component_index:
            row.calculation_context?.skill_effect_component_index ?? null,
          skill_effect_component_count:
            row.calculation_context?.skill_effect_component_count ?? null,
        })),
      });
    }
  }
  return {
    conflicting_context_count: conflictingContextCount,
    contexts_where_every_component_index_is_distinct:
      contextsWithDistinctComponentIndexes,
    adjacent_output_delta_direction_by_component_index: {
      positive: positiveAdjacentOutputDeltas,
      negative: negativeAdjacentOutputDeltas,
      zero: zeroAdjacentOutputDeltas,
    },
    distinct_output_reduced_ratio_histogram: [...outputRatioCounts]
      .map(([ratio, count]) => ({ ratio, count }))
      .sort((left, right) => right.count - left.count ||
        left.ratio.localeCompare(right.ratio, "en"))
      .slice(0, 64),
    bounded_examples: examples,
    component_index_formula_authority: false,
  };
}

function greatestCommonDivisor(left, right) {
  let a = Math.abs(Number(left));
  let b = Math.abs(Number(right));
  while (b !== 0) {
    const remainder = a % b;
    a = b;
    b = remainder;
  }
  return a || 1;
}

function completeRetainedContextParts(row) {
  return [
    row.base,
    stableRawVector(row.raw_values_by_attribute_id),
    row.lifecycle?.source_config_id ?? "<null>",
    row.lifecycle?.status_state ?? "<null>",
    row.lifecycle?.status_stacks ?? "<null>",
    row.target_entity_uuid,
    row.source_attribute_state_id,
    row.target_attribute_state_id,
    row.source_status_state_id,
    row.target_status_state_id,
  ];
}

function packetContextValue(row, field) {
  const separator = field.indexOf(".");
  const objectName = field.slice(0, separator);
  const propertyName = field.slice(separator + 1);
  return row[objectName]?.[propertyName] ?? "<null>";
}

function stableRawVector(raw) {
  return Object.entries(raw ?? {})
    .sort(([a], [b]) => Number(a) - Number(b))
    .map(([key, value]) => [Number(key), Number(value)]);
}

function countBy(rows, keyOf) {
  const counts = Object.create(null);
  for (const row of rows) increment(counts, String(keyOf(row)));
  return Object.entries(counts)
    .sort(([a], [b]) => a.localeCompare(b, "en"))
    .map(([key, count]) => ({ key, count }));
}

function observationKey(sessionId, sequence) {
  return `${sessionId}|${sequence}`;
}

function increment(counts, key) {
  counts[key] = (counts[key] ?? 0) + 1;
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function receipt(filePath) {
  const bytes = fs.readFileSync(filePath);
  return {
    path: filePath,
    bytes: bytes.length,
    sha256: crypto.createHash("sha256").update(bytes).digest("hex"),
  };
}
