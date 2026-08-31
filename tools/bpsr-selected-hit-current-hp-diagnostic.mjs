#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const groupDiagnosticPath = process.argv[2] ? path.resolve(process.argv[2]) : null;
const cohortPath = process.argv[3] ? path.resolve(process.argv[3]) : null;
const outputPath = process.argv[4] ? path.resolve(process.argv[4]) : null;
if (!groupDiagnosticPath || !cohortPath || !outputPath) {
  throw new Error(
    "Usage: node tools/bpsr-selected-hit-current-hp-diagnostic.mjs " +
      "<group-relative-diagnostic.json> <formula-cohort.json> <output.json>",
  );
}
if (fs.existsSync(outputPath) || fs.existsSync(`${outputPath}.partial`)) {
  throw new Error(`Refusing to overwrite output or partial output: ${outputPath}`);
}

const groupDiagnostic = readJson(groupDiagnosticPath);
validateInput(groupDiagnostic);
const cohortHeader = readCohortHeader(cohortPath);
if (cohortHeader.schema_version !== 39 || cohortHeader.game_build !== "24687926") {
  throw new Error(
    `Expected schema-39 build-24687926 cohort, got ${JSON.stringify(cohortHeader)}`,
  );
}
const requestedStateIds = new Set(
  groupDiagnostic.observations.map((row) => Number(row.target_attribute_state_id)),
);
const extractedStates = new Map();
const scan = await scanSelectedArrayItems(
  cohortPath,
  "attribute_states",
  requestedStateIds,
  (index, item) => extractedStates.set(index, JSON.parse(item)),
);
const cohortReceipt = await streamingReceipt(cohortPath);

const rows = groupDiagnostic.observations.map((row) => {
  const attributes = extractedStates.get(Number(row.target_attribute_state_id));
  const currentHpAtWireStart = attributeValue(attributes, 11310);
  const maxHpAtWireStart = attributeValue(attributes, 11320);
  const relative = row.group_relative_context ?? {};
  const priorHpLoss = finiteNumber(relative.capture_preceding_hp_loss_same_target);
  const priorHealingEvents = finiteNumber(
    relative.capture_preceding_healing_events_same_target,
  );
  const unresolvedReasons = [];
  if (!attributes) unresolvedReasons.push("target-attribute-state-not-extracted");
  if (currentHpAtWireStart === null) unresolvedReasons.push("wire-start-current-hp-absent");
  if (maxHpAtWireStart === null) unresolvedReasons.push("wire-start-max-hp-absent");
  if (priorHpLoss === null) unresolvedReasons.push("preceding-hp-loss-unavailable");
  if (priorHealingEvents === null) unresolvedReasons.push("preceding-healing-count-unavailable");
  if (priorHealingEvents !== null && priorHealingEvents !== 0) {
    unresolvedReasons.push("preceding-healing-requires-effective-amount-proof");
  }
  let reconstructedPreHitCurrentHp = null;
  if (unresolvedReasons.length === 0) {
    reconstructedPreHitCurrentHp = currentHpAtWireStart - priorHpLoss;
    if (reconstructedPreHitCurrentHp < 0) {
      unresolvedReasons.push("reconstructed-current-hp-negative");
      reconstructedPreHitCurrentHp = null;
    } else if (reconstructedPreHitCurrentHp > maxHpAtWireStart) {
      unresolvedReasons.push("reconstructed-current-hp-exceeds-max-hp");
      reconstructedPreHitCurrentHp = null;
    }
  }
  return {
    ...row,
    target_hp_context: {
      current_hp_attribute_id: 11310,
      max_hp_attribute_id: 11320,
      current_hp_at_wire_message_start: currentHpAtWireStart,
      max_hp_at_wire_message_start: maxHpAtWireStart,
      preceding_same_capture_hp_loss: priorHpLoss,
      preceding_same_capture_healing_events: priorHealingEvents,
      reconstructed_pre_hit_current_hp: reconstructedPreHitCurrentHp,
      reconstructed_pre_hit_hp_fraction: reconstructedPreHitCurrentHp === null
        ? null
        : reducedFraction(reconstructedPreHitCurrentHp, maxHpAtWireStart),
      unresolved_reasons: unresolvedReasons,
      reconstruction_available: unresolvedReasons.length === 0,
    },
  };
});

const conflictRows = rows.filter((row) => row.baseline_context_conflicting);
const baseline = summarizeContexts(rows, completeRetainedContextParts);
const withWireStartHp = summarizeContexts(rows, (row) => [
  ...completeRetainedContextParts(row),
  row.target_hp_context.current_hp_at_wire_message_start ?? "<null>",
  row.target_hp_context.max_hp_at_wire_message_start ?? "<null>",
]);
const withReconstructedPreHitHp = summarizeContexts(rows, (row) => [
  ...completeRetainedContextParts(row),
  row.target_hp_context.reconstructed_pre_hit_current_hp ?? "<null>",
  row.target_hp_context.max_hp_at_wire_message_start ?? "<null>",
]);
const conflictSubsetWithPreHitHp = summarizeContexts(conflictRows, (row) => [
  ...completeRetainedContextParts(row),
  row.target_hp_context.reconstructed_pre_hit_current_hp ?? "<null>",
  row.target_hp_context.max_hp_at_wire_message_start ?? "<null>",
]);
const availableRows = rows.filter((row) => row.target_hp_context.reconstruction_available);
const availableConflictRows = conflictRows.filter((row) =>
  row.target_hp_context.reconstruction_available);
const availableWithPreHitHp = summarizeContexts(availableRows, (row) => [
  ...completeRetainedContextParts(row),
  row.target_hp_context.reconstructed_pre_hit_current_hp,
  row.target_hp_context.max_hp_at_wire_message_start,
]);
const availableConflictSubsetWithPreHitHp = summarizeContexts(
  availableConflictRows,
  (row) => [
    ...completeRetainedContextParts(row),
    row.target_hp_context.reconstructed_pre_hit_current_hp,
    row.target_hp_context.max_hp_at_wire_message_start,
  ],
);

const report = {
  schema_version: 1,
  generated_by: "rlogs-bpsr-selected-hit-current-hp-diagnostic",
  game_build: "24687926",
  selection: groupDiagnostic.selection,
  inputs: {
    group_relative_diagnostic: receipt(groupDiagnosticPath),
    formula_cohort: {
      ...cohortReceipt,
      schema_version: cohortHeader.schema_version,
      game_build: cohortHeader.game_build,
      attribute_states_scan: scan,
    },
  },
  policy: {
    exact_numeric_ids_and_build_are_authoritative: true,
    formula_cohort_is_streamed_not_fully_deserialized: true,
    only_requested_attribute_states_are_retained: true,
    wire_message_start_state_is_not_replaced_by_a_current_character_snapshot: true,
    same_capture_hp_loss_is_applied_in_exact_wire_container_order: true,
    preceding_healing_requires_effective_amount_proof: true,
    invalid_or_incomplete_hp_reconstruction_is_preserved_as_unresolved: true,
    reconstructed_hp_is_diagnostic_not_formula_authority: true,
    hp_threshold_or_curve_is_not_inferred_from_output_correlation: true,
    runtime_authority: false,
    provider_rdps_credit_allowed: false,
  },
  summary: {
    selected_observation_count: rows.length,
    requested_target_attribute_state_count: requestedStateIds.size,
    extracted_target_attribute_state_count: extractedStates.size,
    observations_with_wire_start_current_hp: rows.filter((row) =>
      row.target_hp_context.current_hp_at_wire_message_start !== null).length,
    observations_with_wire_start_max_hp: rows.filter((row) =>
      row.target_hp_context.max_hp_at_wire_message_start !== null).length,
    observations_with_reconstructed_pre_hit_hp: rows.filter((row) =>
      row.target_hp_context.reconstruction_available).length,
    conflicting_observations_with_reconstructed_pre_hit_hp: conflictRows.filter((row) =>
      row.target_hp_context.reconstruction_available).length,
    unresolved_reason_counts: countValues(
      rows.flatMap((row) => row.target_hp_context.unresolved_reasons),
    ),
  },
  diagnostics: {
    baseline,
    wire_start_current_and_max_hp: withWireStartHp,
    reconstructed_pre_hit_current_and_max_hp: withReconstructedPreHitHp,
    original_conflict_subset_with_reconstructed_pre_hit_hp:
      conflictSubsetWithPreHitHp,
    reconstruction_available_subset_with_pre_hit_hp: availableWithPreHitHp,
    original_conflict_reconstruction_available_subset_with_pre_hit_hp:
      availableConflictSubsetWithPreHitHp,
    output_transition_boundaries: outputTransitionBoundaries(conflictRows),
  },
  observations: rows,
  conclusion: {
    every_requested_attribute_state_extracted:
      extractedStates.size === requestedStateIds.size,
    wire_start_hp_alone_eliminates_all_conflicts:
      withWireStartHp.conflicting_repeated_context_count === 0,
    reconstructed_pre_hit_hp_eliminates_all_conflicts:
      withReconstructedPreHitHp.conflicting_repeated_context_count === 0,
    reconstructed_pre_hit_hp_eliminates_all_original_conflicts:
      conflictSubsetWithPreHitHp.conflicting_repeated_context_count === 0,
    reconstructed_pre_hit_hp_available_rows_have_no_remaining_conflicts:
      availableConflictSubsetWithPreHitHp.conflicting_repeated_context_count === 0,
    reconstructed_pre_hit_hp_available_rows_retain_repeated_controls:
      availableConflictSubsetWithPreHitHp.repeated_context_count > 0,
    reconstructed_pre_hit_hp_is_proven_server_formula_input: false,
    exact_hp_threshold_or_curve_proven: false,
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

async function scanSelectedArrayItems(filePath, propertyName, selectedIndexes, onItem) {
  const marker = `"${propertyName}":[`;
  let markerOffset = 0;
  let found = false;
  let complete = false;
  let index = 0;
  let itemDepth = 0;
  let itemStarted = false;
  let inString = false;
  let escaped = false;
  let capture = false;
  let itemText = "";
  let bytesRead = 0;
  let maximumCapturedItemBytes = 0;
  let retainedItems = 0;
  const stream = fs.createReadStream(filePath, { encoding: "utf8" });
  for await (const chunk of stream) {
    bytesRead += Buffer.byteLength(chunk);
    for (const character of chunk) {
      if (!found) {
        if (character === marker[markerOffset]) {
          markerOffset += 1;
          if (markerOffset === marker.length) found = true;
        } else {
          markerOffset = character === marker[0] ? 1 : 0;
        }
        continue;
      }
      if (complete) break;
      if (!itemStarted) {
        if (/\s|,/.test(character)) continue;
        if (character === "]") {
          complete = true;
          break;
        }
        if (character !== "[") {
          throw new Error(
            `Expected ${propertyName}[${index}] to begin with [, got ${character}`,
          );
        }
        itemStarted = true;
        itemDepth = 1;
        capture = selectedIndexes.has(index);
        itemText = capture ? character : "";
        continue;
      }
      if (capture) itemText += character;
      if (inString) {
        if (escaped) escaped = false;
        else if (character === "\\") escaped = true;
        else if (character === "\"") inString = false;
        continue;
      }
      if (character === "\"") {
        inString = true;
      } else if (character === "[" || character === "{") {
        itemDepth += 1;
      } else if (character === "]" || character === "}") {
        itemDepth -= 1;
        if (itemDepth === 0) {
          if (capture) {
            maximumCapturedItemBytes = Math.max(
              maximumCapturedItemBytes,
              Buffer.byteLength(itemText),
            );
            onItem(index, itemText);
            retainedItems += 1;
          }
          index += 1;
          itemStarted = false;
          itemText = "";
          capture = false;
          if ([...selectedIndexes].every((selected) => selected < index)) {
            complete = true;
            break;
          }
        }
      }
    }
    if (complete) break;
  }
  stream.destroy();
  if (!found) throw new Error(`Property ${propertyName} not found in ${filePath}`);
  return {
    property: propertyName,
    bytes_read_through_last_requested_state: bytesRead,
    array_items_scanned: index,
    requested_items: selectedIndexes.size,
    retained_items: retainedItems,
    maximum_retained_item_bytes: maximumCapturedItemBytes,
    bounded_prefix_scan: true,
  };
}

function outputTransitionBoundaries(rows) {
  const captures = new Map();
  for (const row of rows.filter((candidate) =>
    candidate.target_hp_context.reconstruction_available)) {
    const key = `${row.session_id}|${row.capture_sequence}|` +
      `${row.group_relative_context.skill_effect_group_index}|${row.target_entity_uuid}`;
    let group = captures.get(key);
    if (!group) {
      group = [];
      captures.set(key, group);
    }
    group.push(row);
  }
  const transitions = [];
  for (const group of captures.values()) {
    group.sort((left, right) =>
      Number(left.group_relative_context.skill_effect_component_index) -
      Number(right.group_relative_context.skill_effect_component_index));
    for (let index = 1; index < group.length; index += 1) {
      const previous = group[index - 1];
      const current = group[index];
      if (Number(previous.output) === Number(current.output)) continue;
      transitions.push({
        session_id: current.session_id,
        capture_sequence: current.capture_sequence,
        target_entity_uuid: current.target_entity_uuid,
        previous_component_index:
          previous.group_relative_context.skill_effect_component_index,
        component_index: current.group_relative_context.skill_effect_component_index,
        output_from: previous.output,
        output_to: current.output,
        pre_hit_current_hp_from:
          previous.target_hp_context.reconstructed_pre_hit_current_hp,
        pre_hit_current_hp_to:
          current.target_hp_context.reconstructed_pre_hit_current_hp,
        max_hp: current.target_hp_context.max_hp_at_wire_message_start,
        pre_hit_hp_fraction_from:
          previous.target_hp_context.reconstructed_pre_hit_hp_fraction,
        pre_hit_hp_fraction_to:
          current.target_hp_context.reconstructed_pre_hit_hp_fraction,
      });
    }
  }
  return {
    count: transitions.length,
    transitions: transitions.slice(0, 200),
    omitted_transition_count: Math.max(0, transitions.length - 200),
  };
}

function attributeValue(attributes, id) {
  if (!Array.isArray(attributes)) return null;
  const row = attributes.find((attribute) => Number(attribute.attribute_id) === id);
  const value = Number(row?.value);
  return Number.isSafeInteger(value) ? value : null;
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

function summarizeContexts(rows, keyOf) {
  const contexts = new Map();
  for (const row of rows) {
    const key = JSON.stringify(keyOf(row));
    let context = contexts.get(key);
    if (!context) {
      context = { observations: 0, outputs: new Set() };
      contexts.set(key, context);
    }
    context.observations += 1;
    context.outputs.add(Number(row.output));
  }
  let repeatedContexts = 0;
  let repeatedObservations = 0;
  let conflictingContexts = 0;
  let conflictingObservations = 0;
  let maximumDistinctOutputs = 0;
  for (const context of contexts.values()) {
    maximumDistinctOutputs = Math.max(maximumDistinctOutputs, context.outputs.size);
    if (context.observations < 2) continue;
    repeatedContexts += 1;
    repeatedObservations += context.observations;
    if (context.outputs.size > 1) {
      conflictingContexts += 1;
      conflictingObservations += context.observations;
    }
  }
  return {
    context_count: contexts.size,
    repeated_context_count: repeatedContexts,
    repeated_observation_count: repeatedObservations,
    conflicting_repeated_context_count: conflictingContexts,
    conflicting_repeated_observation_count: conflictingObservations,
    maximum_distinct_outputs_in_one_context: maximumDistinctOutputs,
  };
}

function reducedFraction(numerator, denominator) {
  const divisor = gcd(numerator, denominator);
  return { numerator: numerator / divisor, denominator: denominator / divisor };
}

function gcd(left, right) {
  let a = Math.abs(Number(left));
  let b = Math.abs(Number(right));
  while (b !== 0) [a, b] = [b, a % b];
  return a || 1;
}

function finiteNumber(value) {
  const number = Number(value);
  return Number.isSafeInteger(number) ? number : null;
}

function countValues(values) {
  const counts = Object.create(null);
  for (const value of values) counts[value] = (counts[value] ?? 0) + 1;
  return Object.entries(counts)
    .sort(([left], [right]) => left.localeCompare(right, "en"))
    .map(([value, count]) => ({ value, count }));
}

function validateInput(value) {
  if (value?.schema_version !== 1 || value.game_build !== "24687926") {
    throw new Error("Expected exact-build schema-1 group-relative diagnostic");
  }
  if (!Array.isArray(value.observations) || Number(value.selection?.action_id) !== 2203521) {
    throw new Error("Expected action 2203521 observations");
  }
  if (value.policy?.lifecycle_affected_entity_is_allegiance_neutral !== true ||
      value.policy?.damage_target_is_allegiance_neutral !== true) {
    throw new Error("Input does not retain allegiance-neutral endpoints");
  }
}

function readCohortHeader(filePath) {
  const file = fs.openSync(filePath, "r");
  try {
    const buffer = Buffer.alloc(65_536);
    const bytes = fs.readSync(file, buffer, 0, buffer.length, 0);
    const prefix = buffer.subarray(0, bytes).toString("utf8");
    const schemaVersion = /"schema_version":(\d+)/.exec(prefix);
    const gameBuild = /"game_build":"([^"]+)"/.exec(prefix);
    return {
      schema_version: schemaVersion ? Number(schemaVersion[1]) : null,
      game_build: gameBuild?.[1] ?? null,
    };
  } finally {
    fs.closeSync(file);
  }
}

async function streamingReceipt(filePath) {
  const hash = crypto.createHash("sha256");
  let bytes = 0;
  for await (const chunk of fs.createReadStream(filePath)) {
    hash.update(chunk);
    bytes += chunk.length;
  }
  return {
    path: filePath,
    bytes,
    sha256: hash.digest("hex").toUpperCase(),
  };
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
