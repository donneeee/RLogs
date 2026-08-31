#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const groupPath = process.argv[2] ? path.resolve(process.argv[2]) : null;
const ledgerPath = process.argv[3] ? path.resolve(process.argv[3]) : null;
const outputPath = process.argv[4] ? path.resolve(process.argv[4]) : null;
if (!groupPath || !ledgerPath || !outputPath) {
  throw new Error(
    "Usage: node tools/bpsr-selected-hit-interval-closed-hp-diagnostic.mjs " +
      "<group-relative-diagnostic.json> <hp-ledger-proof.json> <output.json>",
  );
}
if (fs.existsSync(outputPath) || fs.existsSync(`${outputPath}.partial`)) {
  throw new Error(`Refusing to overwrite output or partial output: ${outputPath}`);
}

const group = readJson(groupPath);
const ledger = readJson(ledgerPath);
validateInputs(group, ledger);
const ledgerRows = ledger.selected_action_hp_context.observations;
const ledgerByKey = new Map(ledgerRows.map((row) => [key(row), row]));
if (ledgerByKey.size !== ledgerRows.length) {
  throw new Error("HP ledger contains duplicate session/sequence keys");
}

const missingKeys = [];
const rows = group.observations.map((row) => {
  const hp = ledgerByKey.get(key(row));
  if (!hp) missingKeys.push(key(row));
  if (hp && (Number(hp.run_ordinal) !== Number(row.run_ordinal) ||
      Number(hp.target_entity_uuid) !== Number(row.target_entity_uuid))) {
    throw new Error(`HP ledger topology mismatch for ${key(row)}`);
  }
  return { ...row, target_hp_context: hp ?? null };
});
if (missingKeys.length !== 0 || rows.length !== ledgerRows.length) {
  throw new Error(`Selected/ledger mismatch: ${missingKeys.length} missing`);
}

const conflicts = rows.filter((row) => row.baseline_context_conflicting);
const candidates = rows.filter((row) =>
  row.target_hp_context.candidate_pre_hit_current_hp !== null);
const eligible = rows.filter((row) => row.target_hp_context.formula_context_eligible);
const eligibleConflicts = eligible.filter((row) => row.baseline_context_conflicting);
const candidateSummary = summarizeContexts(candidates, (row) => [
  ...completeRetainedContextParts(row),
  row.target_hp_context.candidate_pre_hit_current_hp,
  row.target_hp_context.max_hp,
]);
const eligibleSummary = summarizeContexts(eligible, (row) => [
  ...completeRetainedContextParts(row),
  row.target_hp_context.predicted_pre_hit_current_hp,
  row.target_hp_context.max_hp,
]);
const eligibleConflictSummary = summarizeContexts(eligibleConflicts, (row) => [
  ...completeRetainedContextParts(row),
  row.target_hp_context.predicted_pre_hit_current_hp,
  row.target_hp_context.max_hp,
]);

const report = {
  schema_version: 1,
  generated_by: "rlogs-bpsr-selected-hit-interval-closed-hp-diagnostic",
  game_build: "24687926",
  selection: group.selection,
  inputs: {
    group_relative_diagnostic: receipt(groupPath),
    hp_ledger_proof: receipt(ledgerPath),
  },
  policy: {
    exact_numeric_ids_and_build_are_authoritative: true,
    lifecycle_affected_entity_is_allegiance_neutral: true,
    damage_target_is_allegiance_neutral: true,
    remote_player_cast_packets_required: false,
    latest_explicit_current_and_max_hp_are_candidate_baselines_only: true,
    complete_snapshot_interval_must_close_with_zero_residual: true,
    missing_hp_loss_or_effective_healing_invalidates_the_interval: true,
    nonclosing_and_mismatched_intervals_are_preserved_as_unresolved: true,
    candidate_hp_correlation_grants_formula_authority: false,
    runtime_authority: false,
    provider_rdps_credit_allowed: false,
  },
  summary: {
    selected_observations: rows.length,
    baseline_conflicting_observations: conflicts.length,
    candidate_pre_hit_hp_observations: candidates.length,
    interval_closed_exact_hp_observations: eligible.length,
    baseline_conflicting_observations_with_interval_closed_exact_hp:
      eligibleConflicts.length,
    unresolved_reason_counts: countValues(
      rows.flatMap((row) => row.target_hp_context.unresolved_reasons),
    ),
  },
  diagnostics: {
    all_observations_baseline: summarizeContexts(rows, completeRetainedContextParts),
    candidate_hp_diagnostic_only: candidateSummary,
    interval_closed_exact_hp_subset: eligibleSummary,
    original_conflict_interval_closed_exact_hp_subset: eligibleConflictSummary,
  },
  observations: rows,
  conclusion: {
    all_selected_actions_matched: true,
    snapshot_transition_model_globally_validated:
      ledger.aggregate.eligible_intervals > 0 &&
      ledger.aggregate.eligible_exact === ledger.aggregate.eligible_intervals,
    interval_closed_exact_hp_available_for_every_observation: eligible.length === rows.length,
    interval_closed_exact_hp_available_for_every_original_conflict:
      eligibleConflicts.length === conflicts.length,
    interval_closed_exact_hp_retains_repeated_conflict_controls:
      eligibleConflictSummary.repeated_context_count > 0,
    exact_hp_threshold_or_curve_proven: false,
    exact_damage_formula_proven: false,
    provider_rdps_credit_allowed: false,
  },
};

fs.mkdirSync(path.dirname(outputPath), { recursive: true });
const partialPath = `${outputPath}.partial`;
fs.writeFileSync(partialPath, `${JSON.stringify(report, null, 2)}\n`);
fs.renameSync(partialPath, outputPath);
console.log(JSON.stringify({ output: outputPath, summary: report.summary, conclusion: report.conclusion }, null, 2));

function key(row) {
  return `${row.session_id}:${row.sequence}`;
}

function completeRetainedContextParts(row) {
  return [
    row.base,
    Object.entries(row.raw_values_by_attribute_id ?? {})
      .sort(([a], [b]) => Number(a) - Number(b))
      .map(([id, value]) => [Number(id), Number(value)]),
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

function summarizeContexts(rows, keyOf) {
  const contexts = new Map();
  for (const row of rows) {
    const contextKey = JSON.stringify(keyOf(row));
    const context = contexts.get(contextKey) ?? { observations: 0, outputs: new Set() };
    context.observations += 1;
    context.outputs.add(Number(row.output));
    contexts.set(contextKey, context);
  }
  let repeated = 0;
  let repeatedObservations = 0;
  let conflicting = 0;
  let conflictingObservations = 0;
  let maximumOutputs = 0;
  for (const context of contexts.values()) {
    maximumOutputs = Math.max(maximumOutputs, context.outputs.size);
    if (context.observations < 2) continue;
    repeated += 1;
    repeatedObservations += context.observations;
    if (context.outputs.size > 1) {
      conflicting += 1;
      conflictingObservations += context.observations;
    }
  }
  return {
    context_count: contexts.size,
    repeated_context_count: repeated,
    repeated_observation_count: repeatedObservations,
    conflicting_repeated_context_count: conflicting,
    conflicting_repeated_observation_count: conflictingObservations,
    maximum_distinct_outputs_in_one_context: maximumOutputs,
  };
}

function countValues(values) {
  const counts = new Map();
  for (const value of values) counts.set(value, (counts.get(value) ?? 0) + 1);
  return [...counts].sort(([left], [right]) => left.localeCompare(right, "en"))
    .map(([value, count]) => ({ value, count }));
}

function validateInputs(group, ledger) {
  if (group?.schema_version !== 1 || group.game_build !== "24687926" ||
      Number(group.selection?.action_id) !== 2203521) {
    throw new Error("Expected exact-build schema-1 action-2203521 group diagnostic");
  }
  if (group.policy?.lifecycle_affected_entity_is_allegiance_neutral !== true ||
      group.policy?.damage_target_is_allegiance_neutral !== true) {
    throw new Error("Group input does not preserve allegiance-neutral endpoints");
  }
  if (ledger?.schema_version !== 2 ||
      ledger.generated_by !== "rlogs-bpsr-hp-state-ledger-proof" ||
      !Array.isArray(ledger.selected_action_hp_context?.observations)) {
    throw new Error("Expected schema-2 HP ledger with selected action context");
  }
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function receipt(filePath) {
  const bytes = fs.readFileSync(filePath);
  return {
    path: filePath,
    bytes: bytes.length,
    sha256: crypto.createHash("sha256").update(bytes).digest("hex").toUpperCase(),
  };
}
