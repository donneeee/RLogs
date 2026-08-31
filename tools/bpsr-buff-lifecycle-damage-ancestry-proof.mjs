#!/usr/bin/env node

import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import readline from "node:readline";

const GENERATED_BY = "rlogs-bpsr-buff-lifecycle-damage-ancestry-proof";
const EXPECTED_BUILD = "24687926";
const EXPECTED_TIMELINE_SCHEMA = 10;
const DEFAULT_EFFECT_ID = 2203521;
const DEFAULT_ACTION_ID = 2203521;
const EXAMPLE_LIMIT = 24;

if (process.argv[2] === "--self-test") {
  selfTest();
  process.stdout.write("bpsr-buff-lifecycle-damage-ancestry-proof self-test passed\n");
  process.exit(0);
}

const inputPath = process.argv[2] ? path.resolve(process.argv[2]) : null;
const outputPath = process.argv[3] ? path.resolve(process.argv[3]) : null;
const effectId = parsePositiveInteger(process.argv[4] ?? DEFAULT_EFFECT_ID, "effect id");
const actionId = parsePositiveInteger(process.argv[5] ?? DEFAULT_ACTION_ID, "action id");
if (!inputPath || !outputPath) {
  throw new Error(
    "Usage: node tools/bpsr-buff-lifecycle-damage-ancestry-proof.mjs " +
      "<support-timeline.schema10.jsonl> <output.json> [effect-id] [action-id]",
  );
}
if (!fs.statSync(inputPath).isFile()) {
  throw new Error(`Input is not a file: ${inputPath}`);
}
if (fs.existsSync(outputPath) || fs.existsSync(`${outputPath}.partial`)) {
  throw new Error(`Refusing to overwrite output or partial output: ${outputPath}`);
}

const analyzer = createAnalyzer({ effectId, actionId });
const inputHash = crypto.createHash("sha256");
const inputStream = fs.createReadStream(inputPath);
inputStream.on("data", (chunk) => inputHash.update(chunk));
const lines = readline.createInterface({ input: inputStream, crlfDelay: Infinity });
let lineNumber = 0;
for await (const line of lines) {
  lineNumber += 1;
  if (!isRelevantLine(line, effectId, actionId)) continue;
  let row;
  try {
    row = JSON.parse(line);
  } catch (error) {
    throw new Error(`Invalid JSON at line ${lineNumber}: ${error.message}`);
  }
  analyzer.consume(row, lineNumber);
}

const proof = analyzer.finish({
  input_path: inputPath,
  input_bytes: fs.statSync(inputPath).size,
  input_sha256: inputHash.digest("hex"),
  input_line_count: lineNumber,
});
fs.mkdirSync(path.dirname(outputPath), { recursive: true });
const partialPath = `${outputPath}.partial`;
fs.writeFileSync(partialPath, `${JSON.stringify(proof, null, 2)}\n`, { flag: "wx" });
fs.renameSync(partialPath, outputPath);
process.stdout.write(`${JSON.stringify(proof.summary, null, 2)}\n`);

function createAnalyzer({ effectId, actionId }) {
  let manifest = null;
  const runHeaders = new Map();
  const latestByEndpoint = new Map();
  const latestConsumedByEndpoint = new Map();
  const previousByInstance = new Map();
  const statusStateCounts = Object.create(null);
  const sourceConfigCounts = Object.create(null);
  const relationshipCounts = Object.create(null);
  const nearestStateCounts = Object.create(null);
  const nearestConsumedStateCounts = Object.create(null);
  const providerEqualityCounts = Object.create(null);
  const exactSurfaceExclusionCounts = Object.create(null);
  const diagnosticBands = freshBands();
  const consumedDiagnosticBands = freshBands();
  const examples = [];
  const exactDamageSurfaceReceipts = [];
  let effectTransitionCount = 0;
  let actionRowCount = 0;
  let exactDamageSurfaceCount = 0;
  let exactDamageSurfaceMatchedCount = 0;
  let exactDamageSurfaceConsumedMatchedCount = 0;

  function consume(row, lineNumber) {
    if (row?.row_type === "manifest") {
      if (manifest) throw new Error(`Duplicate manifest at line ${lineNumber}`);
      validateManifest(row, lineNumber);
      manifest = row;
      return;
    }
    if (row?.row_type === "run_header") {
      validateRunHeader(row, lineNumber);
      if (runHeaders.has(row.session_id)) {
        throw new Error(`Duplicate run header for ${row.session_id} at line ${lineNumber}`);
      }
      runHeaders.set(row.session_id, row);
      return;
    }
    if (row?.row_type !== "relationship") return;
    validateRelationshipEnvelope(row, lineNumber, runHeaders);
    if (row.event_kind === "status" && Number(row.effect_id) === effectId) {
      consumeStatus(row, lineNumber);
      return;
    }
    if (row.event_kind === "damage" && Number(row.action_id) === actionId) {
      consumeDamage(row, lineNumber);
    }
  }

  function consumeStatus(row, lineNumber) {
    effectTransitionCount += 1;
    increment(statusStateCounts, row.status_state ?? "<null>");
    increment(sourceConfigCounts, row.source_config_id == null
      ? "<null>"
      : String(row.source_config_id));
    const endpoint = entityKey(row.affected_entity_uuid, row.affected_entity_actor_id);
    const provider = entityKey(row.provider_entity_uuid, row.provider_actor_id);
    if (!endpoint || !provider) {
      throw new Error(`Selected effect lacks provider or affected entity at line ${lineNumber}`);
    }
    const instanceKey = [row.session_id, provider, endpoint,
      row.status_instance_id ?? "<null>"].join("|");
    const previous = previousByInstance.get(instanceKey) ?? null;
    const transition = compactStatus(row, lineNumber, previous);
    previousByInstance.set(instanceKey, transition);
    setLatest(latestByEndpoint, row.session_id, endpoint, provider, transition);
    if (row.status_state === "consumed") {
      setLatest(latestConsumedByEndpoint, row.session_id, endpoint, provider, transition);
    }
  }

  function consumeDamage(row, lineNumber) {
    actionRowCount += 1;
    const exactSurface = isExactDamageSurface(row, actionId);
    if (exactSurface) {
      exactDamageSurfaceCount += 1;
    } else {
      for (const reason of exactDamageSurfaceExclusions(row, actionId)) {
        increment(exactSurfaceExclusionCounts, reason);
      }
    }
    const actor = entityKey(row.damage_actor_entity_uuid, row.damage_actor_id);
    const target = entityKey(row.damage_target_entity_uuid, row.damage_target_actor_id);
    const nearest = chooseNearest(row, actor, target, latestByEndpoint);
    const nearestConsumed = chooseNearest(row, actor, target, latestConsumedByEndpoint);
    const relationship = nearest?.roles.join("+") ?? "unmatched";
    increment(relationshipCounts, relationship);
    increment(nearestStateCounts, nearest?.transition.status_state ?? "<unmatched>");
    increment(nearestConsumedStateCounts,
      nearestConsumed?.transition.status_state ?? "<unmatched>");
    increment(providerEqualityCounts, providerEquality(nearest, actor));
    updateBands(diagnosticBands, row, nearest);
    updateBands(consumedDiagnosticBands, row, nearestConsumed);
    if (exactSurface && nearest) exactDamageSurfaceMatchedCount += 1;
    if (exactSurface && nearestConsumed) exactDamageSurfaceConsumedMatchedCount += 1;
    if (exactSurface) {
      exactDamageSurfaceReceipts.push(
        compactExactSurfaceReceipt(row, nearest, nearestConsumed),
      );
    }
    if (examples.length < EXAMPLE_LIMIT && (exactSurface || nearest)) {
      examples.push(compactReceipt(row, lineNumber, exactSurface, nearest, nearestConsumed));
    }
  }

  function finish(input) {
    if (!manifest) throw new Error("Timeline manifest was not observed");
    if (runHeaders.size !== Number(manifest.rlog_count)) {
      throw new Error(
        `Manifest declares ${manifest.rlog_count} runs; observed ${runHeaders.size} run headers`,
      );
    }
    const digests = [...new Set([...runHeaders.values()].map((row) => row.protocol_pack_digest))];
    const builds = [...new Set([...runHeaders.values()].map((row) => String(row.client_build)))];
    const sourceConfigs = sortedCounts(sourceConfigCounts);
    const result = {
      schema_version: 1,
      generated_by: GENERATED_BY,
      game_build: EXPECTED_BUILD,
      selection: {
        effect_id: effectId,
        action_id: actionId,
        exact_damage_surface: {
          hit_event_id: 5,
          damage_source: 2,
          property: 7,
          owner_id: actionId,
        },
      },
      input: {
        ...input,
        timeline_schema_version: manifest.schema_version,
        declared_rlog_count: manifest.rlog_count,
        observed_run_header_count: runHeaders.size,
        client_builds: builds,
        protocol_pack_digests: digests,
      },
      policy: {
        provider_to_effect_lifecycle_to_affected_entity_is_preserved: true,
        affected_entity_may_equal_damage_actor_or_damage_target: true,
        affected_entity_allegiance_is_assumed: false,
        damage_target_allegiance_is_assumed: false,
        remote_player_cast_packets_required: false,
        remote_player_cast_packets_synthesized: false,
        missing_cast_packets_treated_as_zero: false,
        nearest_preceding_transition_is_diagnostic_proximity_only: true,
        proximity_grants_causal_ancestry: false,
        formula_authority: false,
        runtime_authority: false,
        provider_rdps_credit_allowed: false,
        unknown_or_unmatched_relationships_are_preserved: true,
      },
      summary: {
        selected_effect_transition_count: effectTransitionCount,
        selected_action_row_count: actionRowCount,
        exact_damage_surface_count: exactDamageSurfaceCount,
        exact_damage_surface_with_preceding_matching_endpoint_count:
          exactDamageSurfaceMatchedCount,
        exact_damage_surface_with_preceding_consumed_matching_endpoint_count:
          exactDamageSurfaceConsumedMatchedCount,
        relationship_counts: sortedCounts(relationshipCounts),
        provider_equals_damage_actor_counts: sortedCounts(providerEqualityCounts),
        selected_status_state_counts: sortedCounts(statusStateCounts),
        selected_source_config_counts: sourceConfigs,
        exact_surface_exclusion_counts: sortedCounts(exactSurfaceExclusionCounts),
      },
      diagnostic_proximity: {
        nearest_preceding_transition: {
          state_counts: sortedCounts(nearestStateCounts),
          bands: diagnosticBands,
        },
        nearest_preceding_consumed_transition: {
          state_counts: sortedCounts(nearestConsumedStateCounts),
          bands: consumedDiagnosticBands,
        },
      },
      exact_damage_surface_receipts: exactDamageSurfaceReceipts,
      bounded_examples: examples,
      conclusion: {
        lifecycle_to_damage_relationship_receipt_available:
          exactDamageSurfaceMatchedCount > 0,
        consumed_stack_to_damage_relationship_receipt_available:
          exactDamageSurfaceConsumedMatchedCount > 0,
        exact_source_config_ids_observed: sourceConfigs
          .filter((entry) => entry.key !== "<null>")
          .map((entry) => Number(entry.key)),
        causal_ancestry_proven: false,
        exact_damage_formula_proven: false,
        provider_rdps_credit_allowed: false,
      },
    };
    return result;
  }

  return { consume, finish };
}

function validateManifest(row, lineNumber) {
  const p = row.policy ?? {};
  if (
    Number(row.schema_version) !== EXPECTED_TIMELINE_SCHEMA ||
    row.projection !== "canonical-who-did-what-id-to-which-target-timeline" ||
    p.affected_entity_is_assumed_enemy !== false ||
    p.affected_entity_is_assumed_friendly !== false ||
    p.damage_target_is_assumed_enemy !== false ||
    p.damage_target_is_assumed_friendly !== false ||
    p.remote_player_cast_packets_required !== false ||
    p.remote_player_cast_packets_synthesized !== false ||
    p.remote_player_cast_packets_treated_as_zero !== false ||
    p.provider_credit_authorized_by_timeline_presence_alone !== false ||
    p.unknown_effects_are_preserved !== true
  ) {
    throw new Error(`Unsafe or incompatible timeline manifest at line ${lineNumber}`);
  }
}

function validateRunHeader(row, lineNumber) {
  if (
    Number(row.schema_version) !== EXPECTED_TIMELINE_SCHEMA ||
    String(row.client_build) !== EXPECTED_BUILD ||
    typeof row.session_id !== "string" ||
    !row.session_id ||
    typeof row.protocol_pack_digest !== "string" ||
    !row.protocol_pack_digest.startsWith("sha256:")
  ) {
    throw new Error(`Unsafe or incomplete run header at line ${lineNumber}`);
  }
}

function validateRelationshipEnvelope(row, lineNumber, runHeaders) {
  if (
    Number(row.schema_version) !== EXPECTED_TIMELINE_SCHEMA ||
    !runHeaders.has(row.session_id) ||
    row.canonical_event_payload_retained_in_source_rlog !== true ||
    !Number.isSafeInteger(Number(row.canonical_source_rlog_sequence))
  ) {
    throw new Error(`Unsafe relationship envelope at line ${lineNumber}`);
  }
}

function setLatest(index, sessionId, endpoint, provider, transition) {
  const endpointKey = `${sessionId}|${endpoint}`;
  let byProvider = index.get(endpointKey);
  if (!byProvider) {
    byProvider = new Map();
    index.set(endpointKey, byProvider);
  }
  byProvider.set(provider, transition);
}

function chooseNearest(damage, actor, target, index) {
  const candidates = new Map();
  addCandidates(candidates, index, damage.session_id, actor, "affected-entity-equals-damage-actor");
  addCandidates(candidates, index, damage.session_id, target, "affected-entity-equals-damage-target");
  const damageSequence = Number(damage.canonical_source_rlog_sequence);
  let nearest = null;
  for (const candidate of candidates.values()) {
    const transitionSequence = Number(candidate.transition.canonical_source_rlog_sequence);
    if (transitionSequence > damageSequence) continue;
    if (!nearest || transitionSequence > Number(nearest.transition.canonical_source_rlog_sequence)) {
      nearest = candidate;
    } else if (nearest && transitionSequence === Number(nearest.transition.canonical_source_rlog_sequence)) {
      nearest.roles = [...new Set([...nearest.roles, ...candidate.roles])].sort();
    }
  }
  return nearest;
}

function addCandidates(candidates, index, sessionId, endpoint, role) {
  if (!endpoint) return;
  const byProvider = index.get(`${sessionId}|${endpoint}`);
  if (!byProvider) return;
  for (const [provider, transition] of byProvider) {
    const key = `${provider}|${transition.canonical_source_rlog_sequence}|${transition.status_instance_id}`;
    const existing = candidates.get(key);
    if (existing) {
      existing.roles.push(role);
    } else {
      candidates.set(key, { provider, transition, roles: [role] });
    }
  }
}

function compactStatus(row, lineNumber, previous) {
  const previousStacks = safeIntegerOrNull(previous?.status_stacks);
  const currentStacks = safeIntegerOrNull(row.status_stacks);
  const consumedStackDelta = row.status_state === "consumed" &&
    previousStacks != null && currentStacks != null && previousStacks >= currentStacks
    ? previousStacks - currentStacks
    : null;
  return {
    line_number: lineNumber,
    session_id: row.session_id,
    canonical_source_rlog_sequence: Number(row.canonical_source_rlog_sequence),
    capture_sequence: safeIntegerOrNull(row.capture_sequence),
    game_time_millis: safeIntegerOrNull(row.game_time_millis),
    observed_micros: safeIntegerOrNull(row.observed_micros),
    effect_id: Number(row.effect_id),
    provider_actor_id: row.provider_actor_id,
    provider_entity_uuid: row.provider_entity_uuid,
    affected_entity_actor_id: row.affected_entity_actor_id,
    affected_entity_uuid: row.affected_entity_uuid,
    source_config_id: row.source_config_id,
    source_type_id: row.source_type_id,
    status_instance_id: row.status_instance_id,
    status_state: row.status_state,
    status_stacks: currentStacks,
    previous_status_stacks: previousStacks,
    consumed_stack_delta: consumedStackDelta,
  };
}

function compactReceipt(damage, lineNumber, exactSurface, nearest, nearestConsumed) {
  return {
    session_id: damage.session_id,
    damage: {
      line_number: lineNumber,
      canonical_source_rlog_sequence: Number(damage.canonical_source_rlog_sequence),
      capture_sequence: safeIntegerOrNull(damage.capture_sequence),
      game_time_millis: safeIntegerOrNull(damage.game_time_millis),
      observed_micros: safeIntegerOrNull(damage.observed_micros),
      action_id: damage.action_id,
      hit_event_id: damage.hit_event_id,
      damage_source: damage.damage_source,
      property: damage.property,
      owner_id: damage.owner_id,
      reported_amount: damage.reported_amount,
      damage_actor_actor_id: damage.damage_actor_id,
      damage_actor_entity_uuid: damage.damage_actor_entity_uuid,
      damage_target_actor_id: damage.damage_target_actor_id,
      damage_target_entity_uuid: damage.damage_target_entity_uuid,
      exact_damage_surface: exactSurface,
    },
    nearest_preceding_transition: compactMatch(damage, nearest),
    nearest_preceding_consumed_transition: compactMatch(damage, nearestConsumed),
    causal_ancestry_proven: false,
  };
}

function compactExactSurfaceReceipt(damage, nearest, nearestConsumed) {
  return {
    session_id: damage.session_id,
    sequence: Number(damage.canonical_source_rlog_sequence),
    reported_amount: damage.reported_amount,
    damage_actor_entity_uuid: damage.damage_actor_entity_uuid,
    damage_target_entity_uuid: damage.damage_target_entity_uuid,
    nearest_transition: compactExactTransition(damage, nearest),
    nearest_consumed_transition: compactExactTransition(damage, nearestConsumed),
  };
}

function compactExactTransition(damage, match) {
  if (!match) return null;
  return {
    relationship_roles: [...new Set(match.roles)].sort(),
    provider_entity_uuid: match.transition.provider_entity_uuid,
    provider_equals_damage_actor:
      match.provider === entityKey(damage.damage_actor_entity_uuid, damage.damage_actor_id),
    sequence: match.transition.canonical_source_rlog_sequence,
    sequence_gap: difference(damage.canonical_source_rlog_sequence,
      match.transition.canonical_source_rlog_sequence),
    capture_sequence_gap: difference(damage.capture_sequence,
      match.transition.capture_sequence),
    game_time_gap_millis: difference(damage.game_time_millis,
      match.transition.game_time_millis),
    observed_gap_micros: difference(damage.observed_micros,
      match.transition.observed_micros),
    source_config_id: match.transition.source_config_id,
    source_type_id: match.transition.source_type_id,
    status_instance_id: match.transition.status_instance_id,
    status_state: match.transition.status_state,
    status_stacks: match.transition.status_stacks,
    previous_status_stacks: match.transition.previous_status_stacks,
    consumed_stack_delta: match.transition.consumed_stack_delta,
  };
}

function compactMatch(damage, match) {
  if (!match) return null;
  return {
    relationship_roles: [...new Set(match.roles)].sort(),
    provider_equals_damage_actor:
      match.provider === entityKey(damage.damage_actor_entity_uuid, damage.damage_actor_id),
    sequence_gap: difference(damage.canonical_source_rlog_sequence,
      match.transition.canonical_source_rlog_sequence),
    capture_sequence_gap: difference(damage.capture_sequence, match.transition.capture_sequence),
    game_time_gap_millis: difference(damage.game_time_millis,
      match.transition.game_time_millis),
    observed_gap_micros: difference(damage.observed_micros, match.transition.observed_micros),
    transition: match.transition,
  };
}

function updateBands(bands, damage, match) {
  if (!match) {
    bands.unmatched += 1;
    return;
  }
  const sequenceGap = difference(damage.canonical_source_rlog_sequence,
    match.transition.canonical_source_rlog_sequence);
  const captureGap = difference(damage.capture_sequence, match.transition.capture_sequence);
  const gameGap = difference(damage.game_time_millis, match.transition.game_time_millis);
  const observedGap = difference(damage.observed_micros, match.transition.observed_micros);
  if (sequenceGap === 0) bands.same_canonical_sequence += 1;
  if (captureGap === 0) bands.same_capture_sequence += 1;
  if (gameGap === 0) bands.same_game_time_millis += 1;
  if (observedGap != null && observedGap >= 0) {
    if (observedGap <= 100_000) bands.within_100_millis += 1;
    if (observedGap <= 250_000) bands.within_250_millis += 1;
    if (observedGap <= 1_000_000) bands.within_1000_millis += 1;
    if (observedGap > 1_000_000) bands.over_1000_millis += 1;
  } else {
    bands.missing_or_negative_observed_gap += 1;
  }
}

function freshBands() {
  return {
    unmatched: 0,
    same_canonical_sequence: 0,
    same_capture_sequence: 0,
    same_game_time_millis: 0,
    within_100_millis: 0,
    within_250_millis: 0,
    within_1000_millis: 0,
    over_1000_millis: 0,
    missing_or_negative_observed_gap: 0,
  };
}

function providerEquality(match, actor) {
  if (!match || !actor) return "<unresolved>";
  return match.provider === actor ? "true" : "false";
}

function isExactDamageSurface(row, actionId) {
  return Number(row.hit_event_id) === 5 &&
    Number(row.damage_source) === 2 &&
    Number(row.property) === 7 &&
    Number(row.owner_id) === actionId;
}

function exactDamageSurfaceExclusions(row, actionId) {
  const reasons = [];
  if (Number(row.hit_event_id) !== 5) reasons.push("hit_event_id_not_5");
  if (Number(row.damage_source) !== 2) reasons.push("damage_source_not_2");
  if (Number(row.property) !== 7) reasons.push("property_not_7");
  if (Number(row.owner_id) !== actionId) reasons.push("owner_id_not_action_id");
  return reasons;
}

function isRelevantLine(line, effectId, actionId) {
  return line.includes('"row_type":"manifest"') ||
    line.includes('"row_type":"run_header"') ||
    line.includes(`"effect_id":${effectId}`) ||
    line.includes(`"action_id":${actionId}`);
}

function entityKey(entityUuid, actorId) {
  if (entityUuid != null) return `uuid:${entityUuid}`;
  if (actorId != null) return `actor:${actorId}`;
  return null;
}

function difference(left, right) {
  const a = safeIntegerOrNull(left);
  const b = safeIntegerOrNull(right);
  return a == null || b == null ? null : a - b;
}

function safeIntegerOrNull(value) {
  if (value == null) return null;
  const number = Number(value);
  return Number.isSafeInteger(number) ? number : null;
}

function parsePositiveInteger(value, label) {
  const number = Number(value);
  if (!Number.isSafeInteger(number) || number <= 0) {
    throw new Error(`${label} must be a positive safe integer`);
  }
  return number;
}

function increment(counts, key) {
  counts[key] = (counts[key] ?? 0) + 1;
}

function sortedCounts(counts) {
  return Object.entries(counts)
    .sort(([a], [b]) => a.localeCompare(b, "en"))
    .map(([key, count]) => ({ key, count }));
}

function selfTest() {
  const analyzer = createAnalyzer({ effectId: DEFAULT_EFFECT_ID, actionId: DEFAULT_ACTION_ID });
  analyzer.consume({
    row_type: "manifest",
    schema_version: 10,
    projection: "canonical-who-did-what-id-to-which-target-timeline",
    rlog_count: 1,
    policy: {
      affected_entity_is_assumed_enemy: false,
      affected_entity_is_assumed_friendly: false,
      damage_target_is_assumed_enemy: false,
      damage_target_is_assumed_friendly: false,
      remote_player_cast_packets_required: false,
      remote_player_cast_packets_synthesized: false,
      remote_player_cast_packets_treated_as_zero: false,
      provider_credit_authorized_by_timeline_presence_alone: false,
      unknown_effects_are_preserved: true,
    },
  }, 1);
  analyzer.consume({
    row_type: "run_header", schema_version: 10, client_build: EXPECTED_BUILD,
    session_id: "test", protocol_pack_digest: "sha256:test",
  }, 2);
  const envelope = {
    row_type: "relationship", schema_version: 10, session_id: "test",
    canonical_event_payload_retained_in_source_rlog: true,
  };
  analyzer.consume({
    ...envelope, event_kind: "status", canonical_source_rlog_sequence: 10,
    capture_sequence: 3, game_time_millis: 1000, observed_micros: 1_000_000,
    effect_id: DEFAULT_EFFECT_ID, provider_actor_id: "1", provider_entity_uuid: "P",
    affected_entity_actor_id: "2", affected_entity_uuid: "R", source_config_id: 2203520,
    source_type_id: 1, status_instance_id: 9, status_state: "stacked", status_stacks: 4,
  }, 3);
  analyzer.consume({
    ...envelope, event_kind: "status", canonical_source_rlog_sequence: 11,
    capture_sequence: 4, game_time_millis: 1010, observed_micros: 1_010_000,
    effect_id: DEFAULT_EFFECT_ID, provider_actor_id: "1", provider_entity_uuid: "P",
    affected_entity_actor_id: "2", affected_entity_uuid: "R", source_config_id: 2203520,
    source_type_id: 1, status_instance_id: 9, status_state: "consumed", status_stacks: 3,
  }, 4);
  analyzer.consume({
    ...envelope, event_kind: "damage", canonical_source_rlog_sequence: 12,
    capture_sequence: 4, game_time_millis: 1010, observed_micros: 1_010_000,
    action_id: DEFAULT_ACTION_ID, hit_event_id: 5, damage_source: 2, property: 7,
    owner_id: DEFAULT_ACTION_ID, reported_amount: 123,
    damage_actor_id: "1", damage_actor_entity_uuid: "P",
    damage_target_actor_id: "2", damage_target_entity_uuid: "R",
  }, 5);
  const result = analyzer.finish({ input_line_count: 5 });
  assert.equal(result.summary.exact_damage_surface_count, 1);
  assert.equal(result.summary.exact_damage_surface_with_preceding_matching_endpoint_count, 1);
  assert.equal(result.summary.exact_damage_surface_with_preceding_consumed_matching_endpoint_count, 1);
  assert.deepEqual(result.summary.relationship_counts, [
    { key: "affected-entity-equals-damage-target", count: 1 },
  ]);
  assert.equal(result.bounded_examples[0].nearest_preceding_consumed_transition
    .transition.consumed_stack_delta, 1);
  assert.equal(result.bounded_examples[0].nearest_preceding_consumed_transition
    .provider_equals_damage_actor, true);
  assert.equal(result.exact_damage_surface_receipts.length, 1);
  assert.equal(result.exact_damage_surface_receipts[0].nearest_transition.status_stacks, 3);
  assert.equal(result.policy.provider_rdps_credit_allowed, false);
}
