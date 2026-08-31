#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import readline from "node:readline";

const selectedPath = process.argv[2] ? path.resolve(process.argv[2]) : null;
const timelinePath = process.argv[3] ? path.resolve(process.argv[3]) : null;
const outputPath = process.argv[4] ? path.resolve(process.argv[4]) : null;
if (!selectedPath || !timelinePath || !outputPath) {
  throw new Error(
    "Usage: node tools/bpsr-skill-effect-group-relative-diagnostic.mjs " +
      "<lifecycle-conditioned-observations.json> <support-timeline.jsonl> " +
      "<output.json>",
  );
}
if (fs.existsSync(outputPath) || fs.existsSync(`${outputPath}.partial`)) {
  throw new Error(`Refusing to overwrite output or partial output: ${outputPath}`);
}

const selected = readJson(selectedPath);
validateSelectedInput(selected);

const selectedByKey = new Map();
const baselineContexts = new Map();
for (const observation of selected.observations) {
  const key = observationKey(observation.session_id, observation.sequence);
  if (selectedByKey.has(key)) throw new Error(`Duplicate selected observation ${key}`);
  selectedByKey.set(key, observation);
  const contextKey = JSON.stringify(completeRetainedContextParts(observation));
  let rows = baselineContexts.get(contextKey);
  if (!rows) {
    rows = [];
    baselineContexts.set(contextKey, rows);
  }
  rows.push(observation);
}

const conflictingContextKeys = new Set(
  [...baselineContexts]
    .filter(([, rows]) => rows.length > 1 && distinctOutputs(rows).size > 1)
    .map(([key]) => key),
);

const timelineHash = crypto.createHash("sha256");
let timelineBytes = 0;
let timelineLineCount = 0;
let malformedLineCount = 0;
let manifest = null;
const runHeaders = [];
let currentCaptureKey = null;
let currentSessionId = null;
let currentCaptureSequence = null;
let currentRelationshipRows = [];
let currentDamageRowCount = 0;
let currentCombatRowCount = 0;
let maximumBufferedDamageRows = 0;
let maximumBufferedCombatRows = 0;
const joined = [];
const joinedKeys = new Set();

const input = fs.createReadStream(timelinePath);
input.on("data", (chunk) => {
  timelineHash.update(chunk);
  timelineBytes += chunk.length;
});
const lines = readline.createInterface({ input, crlfDelay: Infinity });
for await (const line of lines) {
  timelineLineCount += 1;
  if (!line) continue;
  let row;
  try {
    row = JSON.parse(line);
  } catch {
    malformedLineCount += 1;
    continue;
  }
  if (row.row_type === "manifest") {
    manifest = row;
    continue;
  }
  if (row.row_type === "run_header") {
    flushCapture();
    runHeaders.push(row);
    continue;
  }
  if (row.row_type !== "relationship") continue;
  const captureKey = `${row.session_id}|${row.capture_sequence}`;
  if (currentCaptureKey !== null && captureKey !== currentCaptureKey) flushCapture();
  if (currentCaptureKey === null) {
    currentCaptureKey = captureKey;
    currentSessionId = row.session_id;
    currentCaptureSequence = row.capture_sequence;
  }
  if (["damage", "healing", "status", "unresolved_status"].includes(row.event_kind)) {
    currentRelationshipRows.push(row);
  }
  if (row.event_kind === "damage" || row.event_kind === "healing") {
    currentCombatRowCount += 1;
    maximumBufferedCombatRows = Math.max(
      maximumBufferedCombatRows,
      currentCombatRowCount,
    );
  }
  if (row.event_kind === "damage") {
    currentDamageRowCount += 1;
    maximumBufferedDamageRows = Math.max(
      maximumBufferedDamageRows,
      currentDamageRowCount,
    );
  }
}
flushCapture();

if (malformedLineCount !== 0) {
  throw new Error(`Timeline contains ${malformedLineCount} malformed JSON lines`);
}
if (!manifest || manifest.schema_version !== 10) {
  throw new Error("Expected a schema-10 support timeline manifest");
}
if (String(manifest.policy?.damage_target_is_assumed_enemy) !== "false" ||
    String(manifest.policy?.status_target_is_projected_as_allegiance_neutral_affected_entity) !== "true") {
  throw new Error("Timeline does not preserve allegiance-neutral lifecycle and damage endpoints");
}

const missing = [...selectedByKey.keys()].filter((key) => !joinedKeys.has(key));
const joinedConflicts = joined.filter((row) => row.baseline_context_conflicting);
const fields = groupRelativeFields(joined);
const baselineSummary = summarizeContexts(joined, completeRetainedContextParts);
const conflictingBaselineSummary = summarizeContexts(
  joinedConflicts,
  completeRetainedContextParts,
);
const addOneField = fields.map((field) => ({
  field,
  ...summarizeContexts(joined, (row) => [
    ...completeRetainedContextParts(row),
    row.group_relative_context[field] ?? "<null>",
  ]),
}));
const addOneFieldWithinConflicts = fields.map((field) => ({
  field,
  ...summarizeContexts(joinedConflicts, (row) => [
    ...completeRetainedContextParts(row),
    row.group_relative_context[field] ?? "<null>",
  ]),
}));
const completePlausibleGroupRelativeFields = fields.filter(
  (field) => !field.includes("hp_loss") && !field.startsWith("capture_damage_ordinal"),
);
const fullGroupRelativeSummary = summarizeContexts(joined, (row) => [
  ...completeRetainedContextParts(row),
  ...completePlausibleGroupRelativeFields.map(
    (field) => row.group_relative_context[field] ?? "<null>",
  ),
]);
const sameCaptureLifecycleDiagnostics = {
  target_endpoint: summarizeContexts(joined, (row) => [
    ...completeRetainedContextParts(row),
    row.same_capture_lifecycle_context.target_endpoint_signature,
  ]),
  source_endpoint: summarizeContexts(joined, (row) => [
    ...completeRetainedContextParts(row),
    row.same_capture_lifecycle_context.source_endpoint_signature,
  ]),
  source_and_target_endpoints: summarizeContexts(joined, (row) => [
    ...completeRetainedContextParts(row),
    row.same_capture_lifecycle_context.source_endpoint_signature,
    row.same_capture_lifecycle_context.target_endpoint_signature,
  ]),
};

const report = {
  schema_version: 1,
  generated_by: "rlogs-bpsr-skill-effect-group-relative-diagnostic",
  game_build: "24687926",
  selection: selected.selection,
  inputs: {
    lifecycle_conditioned_observations: receipt(selectedPath),
    support_timeline: {
      path: timelinePath,
      bytes: timelineBytes,
      sha256: timelineHash.digest("hex").toUpperCase(),
      line_count: timelineLineCount,
      schema_version: manifest.schema_version,
      rlog_count: manifest.rlog_count,
      run_header_count: runHeaders.length,
    },
  },
  policy: {
    exact_numeric_ids_and_build_are_authoritative: true,
    lifecycle_affected_entity_is_allegiance_neutral: true,
    damage_target_is_allegiance_neutral: true,
    lifecycle_endpoint_and_damage_target_may_differ: true,
    remote_player_cast_packets_required: false,
    remote_player_cast_packets_synthesized: false,
    missing_selected_rows_are_preserved_not_zero_filled: true,
    group_relative_order_is_diagnostic_hidden_state_proxy_only: true,
    observed_prior_hp_loss_is_not_an_authoritative_formula_input: true,
    event_unique_fragmentation_is_formula_proof: false,
    formula_authority: false,
    runtime_authority: false,
    provider_rdps_credit_allowed: false,
  },
  summary: {
    selected_observation_count: selected.observations.length,
    joined_observation_count: joined.length,
    missing_selected_observation_count: missing.length,
    missing_selected_observation_keys: missing,
    baseline_conflicting_context_count: conflictingContextKeys.size,
    joined_observations_in_baseline_conflicting_contexts: joinedConflicts.length,
    maximum_buffered_damage_rows_in_one_capture: maximumBufferedDamageRows,
    maximum_buffered_combat_rows_in_one_capture: maximumBufferedCombatRows,
  },
  diagnostics: {
    baseline: baselineSummary,
    conflicting_baseline_subset: conflictingBaselineSummary,
    group_relative_field_count: fields.length,
    group_relative_fields: fields,
    add_one_field: addOneField,
    add_one_field_within_original_conflicts: addOneFieldWithinConflicts,
    complete_non_amount_group_relative_vector: {
      fields: completePlausibleGroupRelativeFields,
      ...fullGroupRelativeSummary,
    },
    same_capture_lifecycle: sameCaptureLifecycleDiagnostics,
    original_conflict_examples: summarizeConflictExamples(joinedConflicts),
  },
  observations: joined,
  conclusion: {
    every_selected_observation_joined: missing.length === 0,
    any_single_group_relative_field_eliminates_all_original_conflicts:
      addOneFieldWithinConflicts.some(
        (row) => row.conflicting_repeated_context_count === 0,
      ),
    fields_eliminating_all_original_conflicts: addOneFieldWithinConflicts
      .filter((row) => row.conflicting_repeated_context_count === 0)
      .map((row) => row.field),
    complete_non_amount_group_relative_vector_eliminates_all_conflicts:
      fullGroupRelativeSummary.conflicting_repeated_context_count === 0,
    same_capture_target_lifecycle_eliminates_all_conflicts:
      sameCaptureLifecycleDiagnostics.target_endpoint
        .conflicting_repeated_context_count === 0,
    same_capture_source_and_target_lifecycle_eliminates_all_conflicts:
      sameCaptureLifecycleDiagnostics.source_and_target_endpoints
        .conflicting_repeated_context_count === 0,
    group_relative_context_is_proven_formula_input: false,
    exact_damage_formula_proven: false,
    provider_rdps_credit_allowed: false,
  },
};

fs.mkdirSync(path.dirname(outputPath), { recursive: true });
const partialPath = `${outputPath}.partial`;
fs.writeFileSync(partialPath, `${JSON.stringify(report, null, 2)}\n`);
fs.renameSync(partialPath, outputPath);
console.log(JSON.stringify({
  output: outputPath,
  summary: report.summary,
  conclusion: report.conclusion,
}, null, 2));

function flushCapture() {
  if (currentCaptureKey === null) return;
  annotateCapture(
    currentSessionId,
    currentCaptureSequence,
    currentRelationshipRows,
    selectedByKey,
    conflictingContextKeys,
    joined,
    joinedKeys,
  );
  currentCaptureKey = null;
  currentSessionId = null;
  currentCaptureSequence = null;
  currentRelationshipRows = [];
  currentDamageRowCount = 0;
  currentCombatRowCount = 0;
}

function annotateCapture(
  sessionId,
  captureSequence,
  relationshipRows,
  observationsByKey,
  conflictingKeys,
  output,
  outputKeys,
) {
  const combatRows = relationshipRows.filter((row) =>
    row.event_kind === "damage" || row.event_kind === "healing");
  const statusRows = relationshipRows
    .filter((row) => row.event_kind === "status" || row.event_kind === "unresolved_status")
    .sort((left, right) =>
      Number(left.canonical_source_rlog_sequence) -
      Number(right.canonical_source_rlog_sequence));
  const captureRows = combatRows
    .filter((row) => row.event_kind === "damage")
    .sort(wireOrder);
  const combatGroups = new Map();
  for (const row of combatRows) {
    const groupKey = String(row.skill_effect_group_index ?? "<null>");
    let group = combatGroups.get(groupKey);
    if (!group) {
      group = [];
      combatGroups.set(groupKey, group);
    }
    group.push(row);
  }
  const groups = new Map(
    [...combatGroups].map(([key, rows]) => [
      key,
      rows.filter((row) => row.event_kind === "damage").sort(wireOrder),
    ]),
  );
  for (const group of groups.values()) group.sort(wireOrder);

  const captureMetrics = relativeMetrics(captureRows);
  const captureHpMetrics = hpTransitionMetrics(combatRows);
  for (const [groupKey, group] of groups) {
    const groupMetrics = relativeMetrics(group);
    const groupHpMetrics = hpTransitionMetrics(combatGroups.get(groupKey));
    for (const row of group) {
      const key = observationKey(sessionId, row.canonical_source_rlog_sequence);
      const observation = observationsByKey.get(key);
      if (!observation) continue;
      if (outputKeys.has(key)) throw new Error(`Timeline repeats selected row ${key}`);
      verifyJoinedIdentity(observation, row);
      const contextKey = JSON.stringify(completeRetainedContextParts(observation));
      const baselineContextConflicting = conflictingKeys.has(contextKey);
      const lifecycleContext = sameCaptureLifecycleContext(row, statusRows);
      output.push({
        ...observation,
        capture_sequence: captureSequence,
        baseline_context_conflicting: baselineContextConflicting,
        group_relative_context: {
          skill_effect_group_index: scalar(row.skill_effect_group_index),
          skill_effect_component_index: scalar(row.skill_effect_component_index),
          skill_effect_component_count: scalar(row.skill_effect_component_count),
          ...prefixMetrics("capture", captureMetrics.get(row)),
          ...prefixMetrics("group", groupMetrics.get(row)),
          ...prefixMetrics("capture", captureHpMetrics.get(row)),
          ...prefixMetrics("group", groupHpMetrics.get(row)),
          group_key: groupKey,
        },
        same_capture_lifecycle_context: {
          target_endpoint_transition_count: lifecycleContext.target.length,
          target_endpoint_signature: stableSignature(lifecycleContext.target),
          source_endpoint_transition_count: lifecycleContext.source.length,
          source_endpoint_signature: stableSignature(lifecycleContext.source),
          ...(baselineContextConflicting ? {
            target_endpoint_transitions: lifecycleContext.target,
            source_endpoint_transitions: lifecycleContext.source,
          } : {}),
        },
      });
      outputKeys.add(key);
    }
  }
}

function sameCaptureLifecycleContext(damage, statusRows) {
  const damageSequence = Number(damage.canonical_source_rlog_sequence);
  const target = String(damage.damage_target_entity_uuid);
  const source = String(damage.damage_actor_entity_uuid);
  const preceding = statusRows.filter((status) =>
    Number(status.canonical_source_rlog_sequence) < damageSequence);
  return {
    target: preceding
      .filter((status) => String(status.affected_entity_uuid) === target)
      .map(statusTransition),
    source: preceding
      .filter((status) => String(status.affected_entity_uuid) === source)
      .map(statusTransition),
  };
}

function statusTransition(row) {
  return {
    sequence: row.canonical_source_rlog_sequence,
    event_kind: row.event_kind,
    effect_id: row.effect_id,
    provider_entity_uuid: row.provider_entity_uuid,
    affected_entity_uuid: row.affected_entity_uuid,
    source_type_id: row.source_type_id,
    source_config_id: row.source_config_id,
    status_instance_id: row.status_instance_id,
    status_state: row.status_state,
    status_stacks: row.status_stacks,
    status_level: row.status_level,
    status_count: row.status_count,
    source_resolution: row.source_resolution,
    unresolved_status_reason: row.unresolved_status_reason,
  };
}

function stableSignature(value) {
  return crypto.createHash("sha256")
    .update(JSON.stringify(value))
    .digest("hex")
    .toUpperCase();
}

function hpTransitionMetrics(rows) {
  const result = new Map();
  const damageEvents = new Map();
  const hpLoss = new Map();
  const healingEvents = new Map();
  const healingActualAmount = new Map();
  const healingWithoutActualAmount = new Map();
  for (const row of [...rows].sort(wireOrder)) {
    const target = String(row.damage_target_entity_uuid);
    result.set(row, {
      preceding_damage_events_same_target: damageEvents.get(target) ?? 0,
      preceding_hp_loss_same_target: hpLoss.get(target) ?? 0,
      preceding_healing_events_same_target: healingEvents.get(target) ?? 0,
      preceding_healing_actual_amount_same_target:
        healingActualAmount.get(target) ?? 0,
      preceding_healing_without_actual_amount_same_target:
        healingWithoutActualAmount.get(target) ?? 0,
    });
    if (row.event_kind === "damage") {
      damageEvents.set(target, (damageEvents.get(target) ?? 0) + 1);
      hpLoss.set(target, (hpLoss.get(target) ?? 0) + Number(row.hp_loss ?? 0));
    } else if (row.event_kind === "healing") {
      healingEvents.set(target, (healingEvents.get(target) ?? 0) + 1);
      if (row.actual_amount === null || row.actual_amount === undefined) {
        healingWithoutActualAmount.set(
          target,
          (healingWithoutActualAmount.get(target) ?? 0) + 1,
        );
      } else {
        healingActualAmount.set(
          target,
          (healingActualAmount.get(target) ?? 0) + Number(row.actual_amount),
        );
      }
    }
  }
  return result;
}

function relativeMetrics(rows) {
  const result = new Map();
  const actionCounts = countValues(rows, (row) => row.action_id);
  const targetCounts = countValues(rows, (row) => row.damage_target_entity_uuid);
  const actionTargetCounts = countValues(
    rows,
    (row) => `${row.action_id}|${row.damage_target_entity_uuid}`,
  );
  const allTargets = new Set(rows.map((row) => String(row.damage_target_entity_uuid)));
  const actionTargets = new Map();
  for (const row of rows) {
    const action = String(row.action_id);
    let targets = actionTargets.get(action);
    if (!targets) {
      targets = new Set();
      actionTargets.set(action, targets);
    }
    targets.add(String(row.damage_target_entity_uuid));
  }

  const seenActions = new Map();
  const seenTargets = new Map();
  const seenActionTargets = new Map();
  const seenDistinctTargets = new Set();
  const seenDistinctTargetsByAction = new Map();
  const precedingHpLossByTarget = new Map();
  const precedingHpLossByActionTarget = new Map();
  for (let index = 0; index < rows.length; index += 1) {
    const row = rows[index];
    const action = String(row.action_id);
    const target = String(row.damage_target_entity_uuid);
    const actionTarget = `${action}|${target}`;
    let actionDistinctTargets = seenDistinctTargetsByAction.get(action);
    if (!actionDistinctTargets) {
      actionDistinctTargets = new Set();
      seenDistinctTargetsByAction.set(action, actionDistinctTargets);
    }
    const actionOrdinal = (seenActions.get(action) ?? 0) + 1;
    const targetOrdinal = (seenTargets.get(target) ?? 0) + 1;
    const actionTargetOrdinal = (seenActionTargets.get(actionTarget) ?? 0) + 1;
    result.set(row, {
      damage_ordinal: index + 1,
      damage_count: rows.length,
      same_action_ordinal: actionOrdinal,
      same_action_count: actionCounts.get(action),
      same_target_ordinal: targetOrdinal,
      same_target_count: targetCounts.get(target),
      same_action_same_target_ordinal: actionTargetOrdinal,
      same_action_same_target_count: actionTargetCounts.get(actionTarget),
      preceding_distinct_target_count: seenDistinctTargets.size,
      total_distinct_target_count: allTargets.size,
      preceding_same_action_distinct_target_count: actionDistinctTargets.size,
      total_same_action_distinct_target_count: actionTargets.get(action).size,
      preceding_hp_loss_same_target: precedingHpLossByTarget.get(target) ?? 0,
      preceding_hp_loss_same_action_same_target:
        precedingHpLossByActionTarget.get(actionTarget) ?? 0,
    });
    seenActions.set(action, actionOrdinal);
    seenTargets.set(target, targetOrdinal);
    seenActionTargets.set(actionTarget, actionTargetOrdinal);
    seenDistinctTargets.add(target);
    actionDistinctTargets.add(target);
    const hpLoss = Number(row.hp_loss ?? 0);
    precedingHpLossByTarget.set(
      target,
      (precedingHpLossByTarget.get(target) ?? 0) + hpLoss,
    );
    precedingHpLossByActionTarget.set(
      actionTarget,
      (precedingHpLossByActionTarget.get(actionTarget) ?? 0) + hpLoss,
    );
  }
  return result;
}

function prefixMetrics(prefix, metrics) {
  return Object.fromEntries(
    Object.entries(metrics).map(([key, value]) => [`${prefix}_${key}`, value]),
  );
}

function wireOrder(left, right) {
  return nullLastNumber(left.skill_effect_group_index) -
      nullLastNumber(right.skill_effect_group_index) ||
    nullLastNumber(left.skill_effect_component_index) -
      nullLastNumber(right.skill_effect_component_index) ||
    Number(left.canonical_source_rlog_sequence) -
      Number(right.canonical_source_rlog_sequence);
}

function nullLastNumber(value) {
  return value === null || value === undefined ? Number.MAX_SAFE_INTEGER : Number(value);
}

function verifyJoinedIdentity(observation, row) {
  const expected = {
    action_id: Number(selected.selection.action_id),
    damage_actor_entity_uuid: String(observation.source_entity_uuid),
    damage_target_entity_uuid: String(observation.target_entity_uuid),
    output: Number(observation.output),
    component_index: Number(observation.calculation_context?.skill_effect_component_index),
    group_index: Number(observation.calculation_context?.skill_effect_group_index),
  };
  const actual = {
    action_id: Number(row.action_id),
    damage_actor_entity_uuid: String(row.damage_actor_entity_uuid),
    damage_target_entity_uuid: String(row.damage_target_entity_uuid),
    output: Number(row.reported_amount),
    component_index: Number(row.skill_effect_component_index),
    group_index: Number(row.skill_effect_group_index),
  };
  for (const field of Object.keys(expected)) {
    if (expected[field] !== actual[field]) {
      throw new Error(
        `Selected/timeline identity mismatch ${observation.session_id}|` +
          `${observation.sequence} ${field}: ${expected[field]} != ${actual[field]}`,
      );
    }
  }
}

function summarizeContexts(rows, keyOf) {
  const contexts = new Map();
  for (const row of rows) {
    const key = JSON.stringify(keyOf(row));
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

function summarizeConflictExamples(rows) {
  const contexts = new Map();
  for (const row of rows) {
    const key = JSON.stringify(completeRetainedContextParts(row));
    let values = contexts.get(key);
    if (!values) {
      values = [];
      contexts.set(key, values);
    }
    values.push(row);
  }
  return [...contexts.values()]
    .filter((values) => distinctOutputs(values).size > 1)
    .sort((left, right) => right.length - left.length)
    .slice(0, 12)
    .map((values) => ({
      observation_count: values.length,
      outputs: [...distinctOutputs(values)].sort((a, b) => a - b),
      rows: values
        .sort((left, right) =>
          Number(left.capture_sequence) - Number(right.capture_sequence) ||
          Number(left.sequence) - Number(right.sequence))
        .map((row) => ({
          session_id: row.session_id,
          sequence: row.sequence,
          output: row.output,
          target_entity_uuid: row.target_entity_uuid,
          group_relative_context: row.group_relative_context,
        })),
    }));
}

function groupRelativeFields(rows) {
  return [...new Set(rows.flatMap((row) =>
    Object.keys(row.group_relative_context ?? {})))].sort((a, b) =>
    a.localeCompare(b, "en"));
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

function stableRawVector(raw) {
  return Object.entries(raw ?? {})
    .sort(([a], [b]) => Number(a) - Number(b))
    .map(([key, value]) => [Number(key), Number(value)]);
}

function distinctOutputs(rows) {
  return new Set(rows.map((row) => Number(row.output)));
}

function countValues(rows, keyOf) {
  const counts = new Map();
  for (const row of rows) {
    const key = String(keyOf(row));
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  return counts;
}

function scalar(value) {
  return value ?? null;
}

function observationKey(sessionId, sequence) {
  return `${sessionId}|${sequence}`;
}

function validateSelectedInput(value) {
  if (value?.schema_version !== 1 || !Array.isArray(value.observations)) {
    throw new Error("Expected schema-1 lifecycle-conditioned observations");
  }
  if (value.game_build !== "24687926" || Number(value.selection?.action_id) !== 2203521) {
    throw new Error("Expected exact build 24687926 action 2203521 input");
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
