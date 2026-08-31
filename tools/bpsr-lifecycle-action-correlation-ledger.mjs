#!/usr/bin/env node

import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import readline from "node:readline";

const GENERATED_BY = "tools/bpsr-lifecycle-action-correlation-ledger.mjs";
const SCHEMA_VERSION = 4;
const OPEN_WINDOW_FRONTIER_EXAMPLE_LIMIT = 100;
const EXPECTED_TIMELINE_SCHEMA = 10;
const DEFAULT_MAX_CORRELATION_ROWS = 5_000_000;

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "generate") await generateCommand(options);
else if (command === "verify") await verifyCommand(options);
else if (command === "self-test") await selfTest();
else usage(command === "help" ? 0 : 1);

async function generateCommand(values) {
  const build = required(values, "build");
  const timeline = path.resolve(required(values, "timeline"));
  const ownershipProofPath = path.resolve(required(values, "ownership-proof"));
  const ledger = path.resolve(required(values, "ledger"));
  const summary = path.resolve(required(values, "summary"));
  const maxCorrelationRows = parsePositiveInteger(
    values["max-correlation-rows"] ?? DEFAULT_MAX_CORRELATION_ROWS,
    "--max-correlation-rows",
  );
  if (!/^\d+$/.test(build)) throw new Error("--build must contain only ASCII digits");
  requireFile(timeline, "support timeline");
  const ownershipProof = loadProviderOwnershipProof(ownershipProofPath, build);
  refuseExisting([ledger, summary, `${ledger}.partial`, `${summary}.partial`]);
  fs.mkdirSync(path.dirname(ledger), { recursive: true });
  fs.mkdirSync(path.dirname(summary), { recursive: true });

  const result = await buildLedger({
    build,
    timeline,
    ledger,
    maxCorrelationRows,
    ownershipProof,
  });
  const report = buildSummary({
    build,
    timeline,
    ledger,
    maxCorrelationRows,
    ownershipProof,
    ...result,
  });
  report.content_sha256 = contentHash(report);
  const summaryPartial = `${summary}.partial`;
  fs.writeFileSync(summaryPartial, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
  fs.renameSync(summaryPartial, summary);
  await verifyArtifacts(summary, ledger);
  process.stdout.write(`${JSON.stringify(report.summary, null, 2)}\n`);
}

async function buildLedger({ build, timeline, ledger, maxCorrelationRows, ownershipProof }) {
  const inputHash = crypto.createHash("sha256");
  const inputStream = fs.createReadStream(timeline);
  inputStream.on("data", (chunk) => inputHash.update(chunk));
  const lines = readline.createInterface({ input: inputStream, crlfDelay: Infinity });
  const partial = `${ledger}.partial`;
  const output = fs.openSync(partial, "wx");
  const ledgerHash = crypto.createHash("sha256");
  let ledgerBytes = 0;
  function writeRow(row) {
    const text = `${JSON.stringify(row)}\n`;
    fs.writeSync(output, text);
    ledgerHash.update(text);
    ledgerBytes += Buffer.byteLength(text);
  }

  const analyzer = createAnalyzer({ build, maxCorrelationRows, ownershipProof, writeRow });
  let lineNumber = 0;
  try {
    for await (const line of lines) {
      lineNumber += 1;
      if (line.trim() === "") continue;
      let row;
      try {
        row = JSON.parse(line);
      } catch (error) {
        throw new Error(`Invalid timeline JSON at line ${lineNumber}: ${error.message}`);
      }
      analyzer.consume(row, lineNumber);
    }
    const analysis = analyzer.finish(lineNumber);
    fs.fsyncSync(output);
    fs.closeSync(output);
    fs.renameSync(partial, ledger);
    return {
      ...analysis,
      input_line_count: lineNumber,
      input_sha256: inputHash.digest("hex"),
      ledger_bytes: ledgerBytes,
      ledger_sha256: ledgerHash.digest("hex"),
    };
  } catch (error) {
    try { fs.closeSync(output); } catch {}
    throw error;
  }
}

function createAnalyzer({ build, maxCorrelationRows, ownershipProof, writeRow }) {
  let manifest = null;
  let currentRun = null;
  const runHeaders = new Map();
  const activeByEndpoint = new Map();
  const effects = new Map();
  const eventKindCounts = Object.create(null);
  const relationshipRoleCounts = Object.create(null);
  const providerRelationshipCounts = Object.create(null);
  const inactiveReasonCounts = Object.create(null);
  let lifecycleTransitionCount = 0;
  let usableLifecycleTransitionCount = 0;
  let damageActionCount = 0;
  let damageActionsWithCorrelation = 0;
  let damageActionsWithoutCorrelation = 0;
  let correlationRowCount = 0;
  let rawProviderDistinctCorrelationCount = 0;
  let providerOwnershipResolvedCorrelationCount = 0;
  let providerOwnershipUnresolvedCorrelationCount = 0;
  let provenThirdPartyProviderCorrelationCount = 0;

  function consume(row, lineNumber) {
    if (row?.row_type === "manifest") {
      if (manifest) throw new Error(`Duplicate timeline manifest at line ${lineNumber}`);
      validateManifest(row, build, lineNumber);
      manifest = row;
      writeRow({
        schema_version: SCHEMA_VERSION,
        generated_by: GENERATED_BY,
        row_type: "manifest",
        game_build: build,
        source_timeline_schema_version: EXPECTED_TIMELINE_SCHEMA,
        topology: {
          effect_edge: "provider -> effect/status lifecycle -> recipient or enemy target",
          damage_edge: "recipient damage action -> recipient or enemy target",
          source_side_join: "effect endpoint equals damage actor",
          target_side_join: "effect endpoint equals damage target",
        },
        policy: policy(),
        provider_ownership_proof: ownershipProof.descriptor,
      });
      return;
    }
    if (row?.row_type === "run_header") {
      if (!manifest) throw new Error(`Run header precedes manifest at line ${lineNumber}`);
      finishCurrentRun();
      validateRunHeader(row, build, lineNumber);
      if (runHeaders.has(row.session_id)) {
        throw new Error(`Duplicate run header for ${row.session_id} at line ${lineNumber}`);
      }
      runHeaders.set(row.session_id, row);
      currentRun = row;
      writeRow({
        schema_version: SCHEMA_VERSION,
        row_type: "run_header",
        game_build: build,
        session_id: row.session_id,
        protocol_pack_digest: row.protocol_pack_digest,
        source_path: row.source_path,
      });
      return;
    }
    if (row?.row_type !== "relationship") return;
    validateRelationship(row, currentRun, lineNumber);
    increment(eventKindCounts, row.event_kind ?? "<null>");
    if (row.event_kind === "status") consumeStatus(row, lineNumber);
    else if (row.event_kind === "damage") consumeDamage(row, lineNumber);
  }

  function consumeStatus(row, lineNumber) {
    lifecycleTransitionCount += 1;
    const effectId = positiveIntegerOrNull(row.effect_id);
    const provider = entityIdentity(row.provider_entity_uuid, row.provider_actor_id);
    const endpoint = entityIdentity(row.affected_entity_uuid, row.affected_entity_actor_id);
    const instanceId = integerOrNull(row.status_instance_id);
    const stats = effectStats(effectId ?? "<unresolved>");
    stats.lifecycle_transition_count += 1;
    increment(stats.status_state_counts, row.status_state ?? "<null>");
    addIfPresent(stats.source_config_ids, row.source_config_id);
    addIdentity(stats.providers, provider);
    addIdentity(stats.endpoints, endpoint);
    if (effectId == null || !provider || !endpoint || instanceId == null) {
      stats.unusable_lifecycle_transition_count += 1;
      increment(inactiveReasonCounts, lifecycleIdentityReason(effectId, provider, endpoint, instanceId));
      return;
    }
    usableLifecycleTransitionCount += 1;
    const instanceKey = [currentRun.session_id, effectId, provider.key, endpoint.key, instanceId].join("|");
    const endpointKey = `${currentRun.session_id}|${endpoint.key}`;
    const active = activeByEndpoint.get(endpointKey) ?? new Map();
    activeByEndpoint.set(endpointKey, active);
    const previous = active.get(instanceKey) ?? null;
    const stacks = integerOrNull(row.status_stacks);
    const state = row.status_state ?? null;
    if (state === "removed" || (state === "consumed" && (stacks == null || stacks <= 0))) {
      if (previous) {
        active.delete(instanceKey);
        stats.closed_window_count += 1;
        stats.closed_window_correlation_count += previous.correlation_count;
        stats.closed_window_proven_third_party_correlation_count +=
          previous.proven_third_party_correlation_count;
        stats.closed_window_proven_third_party_source_side_correlation_count +=
          previous.proven_third_party_source_side_correlation_count;
        stats.closed_window_proven_third_party_target_side_correlation_count +=
          previous.proven_third_party_target_side_correlation_count;
      } else {
        stats.terminal_without_active_window_count += 1;
      }
      if (state === "consumed" && stacks == null) {
        stats.ambiguous_consumed_transition_count += 1;
        increment(inactiveReasonCounts, "consumed-without-positive-stack-proof");
      }
      return;
    }
    if (!["applied", "refreshed", "stacked", "consumed"].includes(state)) {
      stats.unusable_lifecycle_transition_count += 1;
      increment(inactiveReasonCounts, "unsupported-status-state");
      return;
    }
    if (state === "consumed" && stacks <= 0) return;
    const transition = compactStatus(row, lineNumber, provider, endpoint, previous);
    active.set(instanceKey, transition);
    stats.active_transition_count += 1;
    if (!previous) stats.opened_window_count += 1;
  }

  function consumeDamage(row, lineNumber) {
    damageActionCount += 1;
    const actor = entityIdentity(row.damage_actor_entity_uuid, row.damage_actor_id);
    const target = entityIdentity(row.damage_target_entity_uuid, row.damage_target_actor_id);
    const candidates = new Map();
    addActiveCandidates(candidates, actor, "source-side");
    addActiveCandidates(candidates, target, "target-side");
    if (candidates.size === 0) {
      damageActionsWithoutCorrelation += 1;
      return;
    }
    damageActionsWithCorrelation += 1;
    for (const candidate of candidates.values()) {
      correlationRowCount += 1;
      if (correlationRowCount > maxCorrelationRows) {
        throw new Error(
          `Correlation row limit ${maxCorrelationRows} exceeded; refusing hidden truncation`,
        );
      }
      const roles = [...new Set(candidate.roles)].sort();
      const rawProviderRelationship = classifyRawProviderRelationship(
        candidate.transition.provider,
        actor,
        target,
      );
      const ownershipResolution = resolveProviderOwnership(
        ownershipProof,
        currentRun.session_id,
        candidate.transition.effect_id,
        candidate.transition.provider,
      );
      const effectiveProvider = ownershipResolution?.resolved_owner_identity ??
        candidate.transition.provider;
      const providerRelationship = classifyProviderRelationship(effectiveProvider, actor, target);
      const stats = effectStats(candidate.transition.effect_id);
      candidate.transition.correlation_count += 1;
      if (roles.includes("source-side")) candidate.transition.source_side_correlation_count += 1;
      if (roles.includes("target-side")) candidate.transition.target_side_correlation_count += 1;
      stats.correlation_row_count += 1;
      if (providerRelationship === "provider-equals-damage-actor") {
        stats.provider_equals_damage_actor_count += 1;
      } else if (providerRelationship === "provider-equals-damage-target") {
        stats.provider_equals_damage_target_count += 1;
      }
      if (roles.includes("source-side")) stats.source_side_correlation_count += 1;
      if (roles.includes("target-side")) stats.target_side_correlation_count += 1;
      if (roles.length === 2) stats.both_side_correlation_count += 1;
      if (rawProviderRelationship === "raw-provider-distinct-from-damage-actor-and-target") {
        rawProviderDistinctCorrelationCount += 1;
        stats.provider_distinct_from_damage_endpoints_count += 1;
        if (ownershipResolution) {
          providerOwnershipResolvedCorrelationCount += 1;
          stats.provider_ownership_resolved_count += 1;
          if (providerRelationship === "provider-distinct-from-damage-actor-and-target") {
            provenThirdPartyProviderCorrelationCount += 1;
            stats.proven_third_party_provider_count += 1;
            candidate.transition.proven_third_party_correlation_count += 1;
            if (roles.includes("source-side")) {
              candidate.transition.proven_third_party_source_side_correlation_count += 1;
            }
            if (roles.includes("target-side")) {
              candidate.transition.proven_third_party_target_side_correlation_count += 1;
            }
          }
        } else {
          providerOwnershipUnresolvedCorrelationCount += 1;
          stats.provider_ownership_unresolved_count += 1;
        }
      }
      addIfPresent(stats.action_ids, row.action_id);
      increment(relationshipRoleCounts, roles.join("+"));
      increment(providerRelationshipCounts, providerRelationship);
      writeRow(compactCorrelation({
        build,
        row,
        lineNumber,
        actor,
        target,
        transition: candidate.transition,
        roles,
        rawProviderRelationship,
        providerRelationship,
        ownershipResolution,
      }));
    }
  }

  function addActiveCandidates(candidates, endpoint, role) {
    if (!endpoint) return;
    const active = activeByEndpoint.get(`${currentRun.session_id}|${endpoint.key}`);
    if (!active) return;
    for (const [instanceKey, transition] of active) {
      const existing = candidates.get(instanceKey);
      if (existing) existing.roles.push(role);
      else candidates.set(instanceKey, { transition, roles: [role] });
    }
  }

  function finishCurrentRun() {
    if (!currentRun) return;
    for (const active of activeByEndpoint.values()) {
      for (const transition of active.values()) {
        const stats = effectStats(transition.effect_id);
        stats.open_window_at_run_end_count += 1;
        stats.open_window_correlation_count += transition.correlation_count;
        stats.open_window_proven_third_party_correlation_count +=
          transition.proven_third_party_correlation_count;
        stats.open_window_proven_third_party_source_side_correlation_count +=
          transition.proven_third_party_source_side_correlation_count;
        stats.open_window_proven_third_party_target_side_correlation_count +=
          transition.proven_third_party_target_side_correlation_count;
        if (stats.open_window_frontier.length < OPEN_WINDOW_FRONTIER_EXAMPLE_LIMIT) {
          stats.open_window_frontier.push(compactOpenWindow(transition));
        } else {
          stats.open_window_frontier_omitted_count += 1;
        }
      }
    }
    activeByEndpoint.clear();
    currentRun = null;
  }

  function finish(inputLineCount) {
    if (!manifest) throw new Error("Timeline manifest was not observed");
    finishCurrentRun();
    if (runHeaders.size !== Number(manifest.rlog_count)) {
      throw new Error(
        `Timeline declares ${manifest.rlog_count} runs but ${runHeaders.size} headers were observed`,
      );
    }
    const effectRows = [...effects.values()]
      .map(finalizeEffectStats)
      .sort((a, b) => b.correlation_row_count - a.correlation_row_count ||
        compareEffectIds(a.effect_id, b.effect_id));
    return {
      manifest,
      input_line_count: inputLineCount,
      run_count: runHeaders.size,
      protocol_pack_digests: [...new Set([...runHeaders.values()]
        .map((row) => row.protocol_pack_digest))].sort(),
      lifecycle_transition_count: lifecycleTransitionCount,
      usable_lifecycle_transition_count: usableLifecycleTransitionCount,
      damage_action_count: damageActionCount,
      damage_actions_with_correlation: damageActionsWithCorrelation,
      damage_actions_without_correlation: damageActionsWithoutCorrelation,
      correlation_row_count: correlationRowCount,
      raw_provider_distinct_correlation_count: rawProviderDistinctCorrelationCount,
      provider_ownership_resolved_correlation_count: providerOwnershipResolvedCorrelationCount,
      provider_ownership_unresolved_correlation_count: providerOwnershipUnresolvedCorrelationCount,
      proven_third_party_provider_correlation_count: provenThirdPartyProviderCorrelationCount,
      event_kind_counts: sortedCounts(eventKindCounts),
      relationship_role_counts: sortedCounts(relationshipRoleCounts),
      provider_relationship_counts: sortedCounts(providerRelationshipCounts),
      inactive_reason_counts: sortedCounts(inactiveReasonCounts),
      effects: effectRows,
    };
  }

  function effectStats(effectId) {
    const key = String(effectId);
    let stats = effects.get(key);
    if (!stats) {
      stats = {
        effect_id: effectId,
        lifecycle_transition_count: 0,
        usable_lifecycle_transition_count: 0,
        unusable_lifecycle_transition_count: 0,
        active_transition_count: 0,
        opened_window_count: 0,
        closed_window_count: 0,
        closed_window_correlation_count: 0,
        closed_window_proven_third_party_correlation_count: 0,
        closed_window_proven_third_party_source_side_correlation_count: 0,
        closed_window_proven_third_party_target_side_correlation_count: 0,
        open_window_at_run_end_count: 0,
        open_window_correlation_count: 0,
        open_window_proven_third_party_correlation_count: 0,
        open_window_proven_third_party_source_side_correlation_count: 0,
        open_window_proven_third_party_target_side_correlation_count: 0,
        open_window_frontier: [],
        open_window_frontier_omitted_count: 0,
        terminal_without_active_window_count: 0,
        ambiguous_consumed_transition_count: 0,
        correlation_row_count: 0,
        source_side_correlation_count: 0,
        target_side_correlation_count: 0,
        both_side_correlation_count: 0,
        provider_distinct_from_damage_endpoints_count: 0,
        provider_ownership_resolved_count: 0,
        provider_ownership_unresolved_count: 0,
        proven_third_party_provider_count: 0,
        provider_equals_damage_actor_count: 0,
        provider_equals_damage_target_count: 0,
        status_state_counts: Object.create(null),
        source_config_ids: new Set(),
        action_ids: new Set(),
        providers: new Set(),
        endpoints: new Set(),
      };
      effects.set(key, stats);
    }
    return stats;
  }

  return { consume, finish };
}

function buildSummary({
  build,
  timeline,
  ledger,
  maxCorrelationRows,
  ownershipProof,
  input_line_count,
  input_sha256,
  ledger_bytes,
  ledger_sha256,
  run_count,
  protocol_pack_digests,
  lifecycle_transition_count,
  usable_lifecycle_transition_count,
  damage_action_count,
  damage_actions_with_correlation,
  damage_actions_without_correlation,
  correlation_row_count,
  raw_provider_distinct_correlation_count,
  provider_ownership_resolved_correlation_count,
  provider_ownership_unresolved_correlation_count,
  proven_third_party_provider_correlation_count,
  event_kind_counts,
  relationship_role_counts,
  provider_relationship_counts,
  inactive_reason_counts,
  effects,
}) {
  const correlated = effects.filter((row) => row.correlation_row_count > 0);
  const providerDistinct = correlated.filter((row) =>
    row.provider_distinct_from_damage_endpoints_count > 0);
  const closed = correlated.filter((row) => row.open_window_at_run_end_count === 0);
  const closedWindowCorrelations = effects.reduce(
    (sum, row) => sum + row.closed_window_correlation_count,
    0,
  );
  const openWindowCorrelations = effects.reduce(
    (sum, row) => sum + row.open_window_correlation_count,
    0,
  );
  const closedThirdPartyCorrelations = effects.reduce(
    (sum, row) => sum + row.closed_window_proven_third_party_correlation_count,
    0,
  );
  const openThirdPartyCorrelations = effects.reduce(
    (sum, row) => sum + row.open_window_proven_third_party_correlation_count,
    0,
  );
  const closedThirdPartySourceSideCorrelations = effects.reduce(
    (sum, row) => sum + row.closed_window_proven_third_party_source_side_correlation_count,
    0,
  );
  const closedThirdPartyTargetSideCorrelations = effects.reduce(
    (sum, row) => sum + row.closed_window_proven_third_party_target_side_correlation_count,
    0,
  );
  return {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game_build: build,
    inputs: {
      support_timeline: {
        path: timeline,
        bytes: fs.statSync(timeline).size,
        sha256: input_sha256,
        line_count: input_line_count,
        schema_version: EXPECTED_TIMELINE_SCHEMA,
      },
      provider_ownership_proof: ownershipProof.descriptor,
    },
    output: {
      correlation_ledger: {
        path: ledger,
        bytes: ledger_bytes,
        sha256: ledger_sha256,
        schema_version: SCHEMA_VERSION,
      },
    },
    policy: {
      ...policy(),
      maximum_correlation_rows: maxCorrelationRows,
      row_limit_exhaustion_behavior: "fail-without-output-promotion",
    },
    topology: {
      effect_edge: "provider -> effect/status lifecycle -> recipient or enemy target",
      damage_edge: "recipient damage action -> recipient or enemy target",
      source_side_join: "effect endpoint equals damage actor",
      target_side_join: "effect endpoint equals damage target",
    },
    summary: {
      run_count,
      protocol_pack_digests,
      lifecycle_transition_count,
      usable_lifecycle_transition_count,
      unusable_lifecycle_transition_count:
        lifecycle_transition_count - usable_lifecycle_transition_count,
      observed_numeric_effect_count: effects.filter((row) => row.effect_id !== "<unresolved>").length,
      damage_action_count,
      damage_actions_with_active_lifecycle_correlation: damage_actions_with_correlation,
      damage_actions_without_active_lifecycle_correlation: damage_actions_without_correlation,
      correlation_row_count,
      closed_window_correlation_count: closedWindowCorrelations,
      open_at_run_end_window_correlation_count: openWindowCorrelations,
      closed_window_proven_third_party_correlation_count: closedThirdPartyCorrelations,
      open_at_run_end_window_proven_third_party_correlation_count: openThirdPartyCorrelations,
      closed_window_proven_third_party_source_side_correlation_count:
        closedThirdPartySourceSideCorrelations,
      closed_window_proven_third_party_target_side_correlation_count:
        closedThirdPartyTargetSideCorrelations,
      raw_provider_distinct_correlation_count,
      provider_ownership_resolved_correlation_count,
      provider_ownership_unresolved_correlation_count,
      proven_third_party_provider_correlation_count,
      correlated_effect_count: correlated.length,
      correlated_effects_with_raw_provider_distinct_from_damage_endpoints_count:
        providerDistinct.length,
      correlated_effects_with_all_observed_windows_closed_count: closed.length,
      event_kind_counts,
      relationship_role_counts,
      provider_relationship_counts,
      inactive_reason_counts,
      formula_models_proven: 0,
      provider_rdps_credits_authorized: 0,
    },
    effects,
    conclusion: {
      exact_entity_and_observed_window_correlation_available: correlation_row_count > 0,
      provider_distinct_entity_correlations_available: providerDistinct.length > 0,
      provider_distinct_entity_correlations_prove_third_party_ownership: false,
      provider_ownership_proof_applied: true,
      all_raw_provider_distinct_correlations_ownership_resolved:
        raw_provider_distinct_correlation_count > 0 &&
        provider_ownership_unresolved_correlation_count === 0,
      proven_third_party_provider_correlations_available:
        proven_third_party_provider_correlation_count > 0,
      all_correlated_lifecycle_windows_closed: correlated.length > 0 && closed.length === correlated.length,
      temporal_overlap_proves_causal_attribution: false,
      magnitude_or_formula_proven: false,
      operation_order_stacking_and_rounding_proven: false,
      closed_lifecycle_canonical_conservation_proven: false,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
    },
  };
}

function compactStatus(row, lineNumber, provider, endpoint, previous) {
  return {
    source_timeline_line: lineNumber,
    session_id: row.session_id,
    effect_id: Number(row.effect_id),
    provider,
    endpoint,
    source_config_id: integerOrNull(row.source_config_id),
    source_type_id: integerOrNull(row.source_type_id),
    status_instance_id: Number(row.status_instance_id),
    status_state: row.status_state,
    status_stacks: integerOrNull(row.status_stacks),
    status_level: integerOrNull(row.status_level),
    status_count: integerOrNull(row.status_count),
    status_duration_millis: integerOrNull(row.status_duration_millis),
    status_created_at_millis: integerOrNull(row.status_created_at_millis),
    window_start_sequence: previous?.window_start_sequence ??
      Number(row.canonical_source_rlog_sequence),
    latest_transition_sequence: Number(row.canonical_source_rlog_sequence),
    latest_capture_sequence: integerOrNull(row.capture_sequence),
    latest_game_time_millis: integerOrNull(row.game_time_millis),
    latest_observed_micros: integerOrNull(row.observed_micros),
    correlation_count: previous?.correlation_count ?? 0,
    source_side_correlation_count: previous?.source_side_correlation_count ?? 0,
    target_side_correlation_count: previous?.target_side_correlation_count ?? 0,
    proven_third_party_correlation_count:
      previous?.proven_third_party_correlation_count ?? 0,
    proven_third_party_source_side_correlation_count:
      previous?.proven_third_party_source_side_correlation_count ?? 0,
    proven_third_party_target_side_correlation_count:
      previous?.proven_third_party_target_side_correlation_count ?? 0,
  };
}

function compactOpenWindow(transition) {
  return {
    session_id: transition.session_id,
    effect_id: transition.effect_id,
    provider_actor_id: transition.provider.actor_id,
    provider_entity_uuid: transition.provider.entity_uuid,
    affected_entity_actor_id: transition.endpoint.actor_id,
    affected_entity_uuid: transition.endpoint.entity_uuid,
    status_instance_id: transition.status_instance_id,
    window_start_sequence: transition.window_start_sequence,
    latest_transition_sequence: transition.latest_transition_sequence,
    correlation_count: transition.correlation_count,
    source_side_correlation_count: transition.source_side_correlation_count,
    target_side_correlation_count: transition.target_side_correlation_count,
    proven_third_party_correlation_count: transition.proven_third_party_correlation_count,
    proven_third_party_source_side_correlation_count:
      transition.proven_third_party_source_side_correlation_count,
    proven_third_party_target_side_correlation_count:
      transition.proven_third_party_target_side_correlation_count,
    source_timeline_rows_remain_authoritative: true,
  };
}

function compactCorrelation({
  build,
  row,
  lineNumber,
  actor,
  target,
  transition,
  roles,
  rawProviderRelationship,
  providerRelationship,
  ownershipResolution,
}) {
  return {
    schema_version: SCHEMA_VERSION,
    row_type: "lifecycle_damage_correlation",
    game_build: build,
    session_id: row.session_id,
    relationship_roles: roles,
    raw_provider_relationship: rawProviderRelationship,
    provider_relationship: providerRelationship,
    provider_ownership: ownershipResolution ? {
      resolution_class: ownershipResolution.resolution_class,
      resolved_owner_actor_id: ownershipResolution.resolved_owner_identity.actor_id,
      resolved_owner_entity_uuid: ownershipResolution.resolved_owner_identity.entity_uuid,
      resolved_owner_character_id: ownershipResolution.resolved_owner_character_id,
      ownership_chain: ownershipResolution.ownership_chain,
    } : null,
    lifecycle: {
      effect_id: transition.effect_id,
      provider_actor_id: transition.provider.actor_id,
      provider_entity_uuid: transition.provider.entity_uuid,
      affected_entity_actor_id: transition.endpoint.actor_id,
      affected_entity_uuid: transition.endpoint.entity_uuid,
      source_config_id: transition.source_config_id,
      source_type_id: transition.source_type_id,
      status_instance_id: transition.status_instance_id,
      status_state: transition.status_state,
      status_stacks: transition.status_stacks,
      status_level: transition.status_level,
      status_count: transition.status_count,
      status_duration_millis: transition.status_duration_millis,
      status_created_at_millis: transition.status_created_at_millis,
      window_start_sequence: transition.window_start_sequence,
      latest_transition_sequence: transition.latest_transition_sequence,
      latest_capture_sequence: transition.latest_capture_sequence,
      latest_game_time_millis: transition.latest_game_time_millis,
      latest_observed_micros: transition.latest_observed_micros,
    },
    damage: {
      source_timeline_line: lineNumber,
      canonical_source_rlog_sequence: Number(row.canonical_source_rlog_sequence),
      capture_sequence: integerOrNull(row.capture_sequence),
      game_time_millis: integerOrNull(row.game_time_millis),
      observed_micros: integerOrNull(row.observed_micros),
      action_id: integerOrNull(row.action_id),
      action_instance_id: integerOrNull(row.action_instance_id),
      hit_event_id: integerOrNull(row.hit_event_id),
      damage_source: integerOrNull(row.damage_source),
      damage_type: integerOrNull(row.damage_type),
      property: integerOrNull(row.property),
      owner_id: integerOrNull(row.owner_id),
      owner_level: integerOrNull(row.owner_level),
      owner_stage: integerOrNull(row.owner_stage),
      reported_amount: integerOrNull(row.reported_amount),
      actual_amount: integerOrNull(row.actual_amount),
      actor_id: actor?.actor_id ?? null,
      actor_entity_uuid: actor?.entity_uuid ?? null,
      target_actor_id: target?.actor_id ?? null,
      target_entity_uuid: target?.entity_uuid ?? null,
    },
    distance: {
      canonical_sequence_gap: difference(
        row.canonical_source_rlog_sequence,
        transition.latest_transition_sequence,
      ),
      capture_sequence_gap: difference(row.capture_sequence, transition.latest_capture_sequence),
      game_time_gap_millis: difference(row.game_time_millis, transition.latest_game_time_millis),
      observed_gap_micros: difference(row.observed_micros, transition.latest_observed_micros),
    },
    authority: {
      exact_entity_endpoint_join: true,
      observed_active_window_overlap: true,
      provider_ownership_proven: ownershipResolution != null ||
        rawProviderRelationship !== "raw-provider-distinct-from-damage-actor-and-target",
      third_party_provider_ownership_proven: ownershipResolution != null &&
        providerRelationship === "provider-distinct-from-damage-actor-and-target",
      causal_attribution_proven: false,
      magnitude_or_formula_proven: false,
      provider_rdps_credit_allowed: false,
    },
  };
}

function finalizeEffectStats(stats) {
  return {
    effect_id: stats.effect_id,
    lifecycle_transition_count: stats.lifecycle_transition_count,
    usable_lifecycle_transition_count:
      stats.lifecycle_transition_count - stats.unusable_lifecycle_transition_count,
    unusable_lifecycle_transition_count: stats.unusable_lifecycle_transition_count,
    active_transition_count: stats.active_transition_count,
    opened_window_count: stats.opened_window_count,
    closed_window_count: stats.closed_window_count,
    closed_window_correlation_count: stats.closed_window_correlation_count,
    closed_window_proven_third_party_correlation_count:
      stats.closed_window_proven_third_party_correlation_count,
    closed_window_proven_third_party_source_side_correlation_count:
      stats.closed_window_proven_third_party_source_side_correlation_count,
    closed_window_proven_third_party_target_side_correlation_count:
      stats.closed_window_proven_third_party_target_side_correlation_count,
    open_window_at_run_end_count: stats.open_window_at_run_end_count,
    open_window_correlation_count: stats.open_window_correlation_count,
    open_window_proven_third_party_correlation_count:
      stats.open_window_proven_third_party_correlation_count,
    open_window_proven_third_party_source_side_correlation_count:
      stats.open_window_proven_third_party_source_side_correlation_count,
    open_window_proven_third_party_target_side_correlation_count:
      stats.open_window_proven_third_party_target_side_correlation_count,
    open_window_frontier: stats.open_window_frontier,
    open_window_frontier_omitted_count: stats.open_window_frontier_omitted_count,
    terminal_without_active_window_count: stats.terminal_without_active_window_count,
    ambiguous_consumed_transition_count: stats.ambiguous_consumed_transition_count,
    correlation_row_count: stats.correlation_row_count,
    source_side_correlation_count: stats.source_side_correlation_count,
    target_side_correlation_count: stats.target_side_correlation_count,
    both_side_correlation_count: stats.both_side_correlation_count,
    provider_distinct_from_damage_endpoints_count:
      stats.provider_distinct_from_damage_endpoints_count,
    provider_ownership_resolved_count: stats.provider_ownership_resolved_count,
    provider_ownership_unresolved_count: stats.provider_ownership_unresolved_count,
    proven_third_party_provider_count: stats.proven_third_party_provider_count,
    provider_equals_damage_actor_count: stats.provider_equals_damage_actor_count,
    provider_equals_damage_target_count: stats.provider_equals_damage_target_count,
    status_state_counts: sortedCounts(stats.status_state_counts),
    source_config_ids: sortedNumbers(stats.source_config_ids),
    action_ids: sortedNumbers(stats.action_ids),
    provider_count: stats.providers.size,
    affected_endpoint_count: stats.endpoints.size,
    proof_disposition: disposition(stats),
  };
}

function disposition(stats) {
  if (stats.correlation_row_count === 0) return "no-observed-active-window-damage-correlation";
  if (stats.proven_third_party_provider_count > 0 &&
    stats.target_side_correlation_count > 0) {
    return "third-party-provider-proven-target-side-formula-candidate-only";
  }
  if (stats.proven_third_party_provider_count > 0 &&
    stats.source_side_correlation_count > 0) {
    return "third-party-provider-proven-source-side-formula-candidate-only";
  }
  if (stats.provider_ownership_unresolved_count > 0 &&
    stats.target_side_correlation_count > 0) {
    return "provider-distinct-target-side-ownership-unresolved-formula-candidate-only";
  }
  if (stats.provider_ownership_unresolved_count > 0 &&
    stats.source_side_correlation_count > 0) {
    return "provider-distinct-source-side-ownership-unresolved-formula-candidate-only";
  }
  if (stats.target_side_correlation_count > 0 &&
    stats.provider_equals_damage_actor_count === stats.correlation_row_count) {
    return "target-side-provider-owner-equals-damage-actor-carrier-candidate-only";
  }
  if (stats.target_side_correlation_count > 0) return "target-side-formula-candidate-only";
  return "source-side-self-or-owner-state-formula-candidate-only";
}

async function verifyCommand(values) {
  const summary = path.resolve(required(values, "summary"));
  const ledger = path.resolve(required(values, "ledger"));
  await verifyArtifacts(summary, ledger);
  const report = readJson(summary);
  process.stdout.write(
    `Lifecycle/action correlation ledger verified for build ${report.game_build}: ` +
      `${report.summary.correlation_row_count} exact temporal correlation rows, ` +
      `zero formula or provider-credit promotions.\n`,
  );
}

async function verifyArtifacts(summaryPath, ledgerPath) {
  const report = readJson(summaryPath);
  if (
    Number(report.schema_version) !== SCHEMA_VERSION ||
    report.generated_by !== GENERATED_BY ||
    report.content_sha256 !== contentHash(report) ||
    report.policy?.provider_rdps_credit_allowed !== false ||
    report.policy?.temporal_overlap_grants_causal_attribution !== false ||
    report.policy?.raw_provider_entity_inequality_proves_third_party_ownership !== false ||
    report.policy?.summon_projectile_and_child_ownership_must_be_resolved_before_provider_credit !== true ||
    report.policy?.exact_build_provider_ownership_proof_is_applied_before_provider_comparison !== true ||
    report.policy?.missing_or_incomplete_windows_are_zero_filled !== false ||
    report.policy?.open_run_boundary_windows_are_identified_for_fail_closed_exclusion !== true ||
    report.conclusion?.provider_ownership_proof_applied !== true ||
    report.conclusion?.magnitude_or_formula_proven !== false ||
    report.conclusion?.provider_rdps_credit_allowed !== false ||
    report.conclusion?.runtime_promotion_allowed !== false
  ) {
    throw new Error("Unsafe or invalid lifecycle/action summary");
  }
  const ownershipDescriptor = report.inputs?.provider_ownership_proof;
  const ownershipPath = path.resolve(ownershipDescriptor?.path ?? "");
  requireFile(ownershipPath, "provider ownership proof");
  if (
    Number(ownershipDescriptor?.schema_version) !== 5 ||
    ownershipDescriptor?.tool !== "rlogs-bpsr-status-effect-provider-ownership-proof" ||
    Number(ownershipDescriptor?.bytes) !== fs.statSync(ownershipPath).size ||
    ownershipDescriptor?.sha256 !== await fileSha256(ownershipPath)
  ) {
    throw new Error("Provider ownership proof descriptor mismatch");
  }
  loadProviderOwnershipProof(ownershipPath, String(report.game_build));
  requireFile(ledgerPath, "correlation ledger");
  const descriptor = report.output?.correlation_ledger;
  if (
    path.resolve(descriptor?.path ?? "") !== ledgerPath ||
    Number(descriptor?.bytes) !== fs.statSync(ledgerPath).size ||
    descriptor?.sha256 !== await fileSha256(ledgerPath)
  ) {
    throw new Error("Correlation ledger descriptor mismatch");
  }
  const counts = new Map();
  let manifestCount = 0;
  let runHeaderCount = 0;
  let correlationCount = 0;
  let rawDistinctCount = 0;
  let ownershipResolvedCount = 0;
  let ownershipUnresolvedCount = 0;
  let provenThirdPartyCount = 0;
  const lines = readline.createInterface({
    input: fs.createReadStream(ledgerPath),
    crlfDelay: Infinity,
  });
  for await (const line of lines) {
    const row = JSON.parse(line);
    if (row.row_type === "manifest") {
      manifestCount += 1;
      if (row.policy?.provider_rdps_credit_allowed !== false ||
        row.provider_ownership_proof?.sha256 !== ownershipDescriptor.sha256) {
        throw new Error("Ledger manifest authorizes provider credit");
      }
    } else if (row.row_type === "run_header") {
      runHeaderCount += 1;
    } else if (row.row_type === "lifecycle_damage_correlation") {
      correlationCount += 1;
      if (
        row.authority?.exact_entity_endpoint_join !== true ||
        row.authority?.observed_active_window_overlap !== true ||
        row.authority?.causal_attribution_proven !== false ||
        row.authority?.magnitude_or_formula_proven !== false ||
        row.authority?.provider_rdps_credit_allowed !== false ||
        typeof row.authority?.provider_ownership_proven !== "boolean" ||
        typeof row.authority?.third_party_provider_ownership_proven !== "boolean" ||
        !Array.isArray(row.relationship_roles) ||
        row.relationship_roles.length === 0
      ) {
        throw new Error("Unsafe or incomplete correlation row");
      }
      const rawDistinct = row.raw_provider_relationship ===
        "raw-provider-distinct-from-damage-actor-and-target";
      if (rawDistinct) {
        rawDistinctCount += 1;
        if (row.provider_ownership == null) ownershipUnresolvedCount += 1;
        else ownershipResolvedCount += 1;
      }
      if (row.authority.third_party_provider_ownership_proven) provenThirdPartyCount += 1;
      if (rawDistinct && row.provider_ownership == null &&
        row.authority.provider_ownership_proven !== false) {
        throw new Error("Unresolved raw provider was marked ownership-proven");
      }
      if (row.authority.third_party_provider_ownership_proven !==
        (row.provider_ownership != null &&
          row.provider_relationship === "provider-distinct-from-damage-actor-and-target")) {
        throw new Error("Third-party provider authority mismatch");
      }
      const key = String(row.lifecycle?.effect_id);
      counts.set(key, (counts.get(key) ?? 0) + 1);
    } else {
      throw new Error(`Unknown ledger row type ${row.row_type}`);
    }
  }
  if (
    manifestCount !== 1 ||
    runHeaderCount !== Number(report.summary?.run_count) ||
    correlationCount !== Number(report.summary?.correlation_row_count) ||
    rawDistinctCount !== Number(report.summary?.raw_provider_distinct_correlation_count) ||
    ownershipResolvedCount !== Number(report.summary?.provider_ownership_resolved_correlation_count) ||
    ownershipUnresolvedCount !== Number(report.summary?.provider_ownership_unresolved_correlation_count) ||
    provenThirdPartyCount !== Number(report.summary?.proven_third_party_provider_correlation_count)
  ) {
    throw new Error("Ledger row conservation failed");
  }
  for (const effect of report.effects ?? []) {
    if ((counts.get(String(effect.effect_id)) ?? 0) !== Number(effect.correlation_row_count)) {
      throw new Error(`Effect ${effect.effect_id} correlation count mismatch`);
    }
    if (Number(effect.closed_window_correlation_count) +
      Number(effect.open_window_correlation_count) !== Number(effect.correlation_row_count)) {
      throw new Error(`Effect ${effect.effect_id} window correlation conservation failed`);
    }
    if (Number(effect.closed_window_proven_third_party_correlation_count) +
      Number(effect.open_window_proven_third_party_correlation_count) !==
      Number(effect.proven_third_party_provider_count)) {
      throw new Error(`Effect ${effect.effect_id} third-party window conservation failed`);
    }
    if (Number(effect.open_window_frontier?.length ?? 0) +
      Number(effect.open_window_frontier_omitted_count) !== Number(effect.open_window_at_run_end_count)) {
      throw new Error(`Effect ${effect.effect_id} open-window frontier conservation failed`);
    }
  }
  const closedCorrelationCount = (report.effects ?? []).reduce(
    (sum, effect) => sum + Number(effect.closed_window_correlation_count),
    0,
  );
  const openCorrelationCount = (report.effects ?? []).reduce(
    (sum, effect) => sum + Number(effect.open_window_correlation_count),
    0,
  );
  const closedThirdPartyCount = (report.effects ?? []).reduce(
    (sum, effect) => sum + Number(effect.closed_window_proven_third_party_correlation_count),
    0,
  );
  const openThirdPartyCount = (report.effects ?? []).reduce(
    (sum, effect) => sum + Number(effect.open_window_proven_third_party_correlation_count),
    0,
  );
  const closedThirdPartySourceSideCount = (report.effects ?? []).reduce(
    (sum, effect) => sum +
      Number(effect.closed_window_proven_third_party_source_side_correlation_count),
    0,
  );
  const closedThirdPartyTargetSideCount = (report.effects ?? []).reduce(
    (sum, effect) => sum +
      Number(effect.closed_window_proven_third_party_target_side_correlation_count),
    0,
  );
  if (closedCorrelationCount !== Number(report.summary?.closed_window_correlation_count) ||
    openCorrelationCount !== Number(report.summary?.open_at_run_end_window_correlation_count) ||
    closedThirdPartyCount !==
      Number(report.summary?.closed_window_proven_third_party_correlation_count) ||
    openThirdPartyCount !==
      Number(report.summary?.open_at_run_end_window_proven_third_party_correlation_count) ||
    closedThirdPartySourceSideCount !==
      Number(report.summary?.closed_window_proven_third_party_source_side_correlation_count) ||
    closedThirdPartyTargetSideCount !==
      Number(report.summary?.closed_window_proven_third_party_target_side_correlation_count)) {
    throw new Error("Summary window correlation conservation failed");
  }
}

function validateManifest(row, build, lineNumber) {
  const p = row.policy ?? {};
  const topology = row.topology ?? {};
  if (
    Number(row.schema_version) !== EXPECTED_TIMELINE_SCHEMA ||
    row.projection !== "canonical-who-did-what-id-to-which-target-timeline" ||
    topology.effect_edge !== "provider -> effect/status lifecycle -> recipient or enemy target" ||
    topology.damage_edge !== "recipient damage action -> recipient or enemy target" ||
    topology.source_side_join !== "effect endpoint equals damage actor" ||
    topology.target_side_join !== "effect endpoint equals damage target" ||
    p.exact_numeric_ids_and_build_are_authoritative !== true ||
    p.remote_player_cast_packets_required !== false ||
    p.remote_player_cast_packets_synthesized !== false ||
    p.remote_player_cast_packets_treated_as_zero !== false ||
    p.provider_credit_authorized_by_timeline_presence_alone !== false ||
    p.unknown_effects_are_preserved !== true
  ) {
    throw new Error(`Unsafe or incompatible timeline manifest at line ${lineNumber}`);
  }
  if (String(p.team_attribute_interpretation_build) !== build) {
    throw new Error(`Timeline build policy mismatch at line ${lineNumber}`);
  }
}

function validateRunHeader(row, build, lineNumber) {
  if (
    Number(row.schema_version) !== EXPECTED_TIMELINE_SCHEMA ||
    String(row.client_build) !== build ||
    typeof row.session_id !== "string" ||
    !row.session_id ||
    typeof row.protocol_pack_digest !== "string" ||
    !row.protocol_pack_digest.startsWith("sha256:")
  ) {
    throw new Error(`Unsafe or incomplete run header at line ${lineNumber}`);
  }
}

function validateRelationship(row, currentRun, lineNumber) {
  if (
    !currentRun ||
    row.session_id !== currentRun.session_id ||
    Number(row.schema_version) !== EXPECTED_TIMELINE_SCHEMA ||
    row.canonical_event_payload_retained_in_source_rlog !== true ||
    !Number.isSafeInteger(Number(row.canonical_source_rlog_sequence))
  ) {
    throw new Error(`Unsafe relationship envelope at line ${lineNumber}`);
  }
}

function policy() {
  return {
    exact_numeric_effect_action_and_build_ids_are_authoritative: true,
    localized_names_are_runtime_keys: false,
    source_side_and_target_side_joins_are_independent: true,
    endpoint_allegiance_is_assumed: false,
    remote_player_cast_packets_required: false,
    remote_player_cast_packets_synthesized: false,
    missing_remote_cast_packets_are_zero: false,
    only_observed_positive_active_status_state_is_joinable: true,
    removed_or_unproven_consumed_status_is_not_active: true,
    incomplete_windows_are_preserved_in_summary: true,
    open_run_boundary_windows_are_identified_for_fail_closed_exclusion: true,
    missing_or_incomplete_windows_are_zero_filled: false,
    temporal_overlap_grants_causal_attribution: false,
    temporal_overlap_grants_formula_authority: false,
    raw_provider_entity_inequality_proves_third_party_ownership: false,
    summon_projectile_and_child_ownership_must_be_resolved_before_provider_credit: true,
    exact_build_provider_ownership_proof_is_applied_before_provider_comparison: true,
    current_character_snapshots_substituted_into_older_runs: false,
    provider_rdps_credit_allowed: false,
    runtime_authority: false,
  };
}

function loadProviderOwnershipProof(file, build) {
  const proof = readJson(file);
  const p = proof.policy ?? {};
  if (
    Number(proof.schema_version) !== 5 ||
    proof.tool !== "rlogs-bpsr-status-effect-provider-ownership-proof" ||
    String(proof.game_build) !== build ||
    p.scope !== "provider_ownership_only" ||
    p.exact_numeric_effect_ids_authoritative !== true ||
    p.exact_input_build_authoritative !== true ||
    p.localized_names_are_evidence_only !== true ||
    p.actor_kind_or_packet_proven_ancestry_required_for_player_ownership !== true ||
    p.future_actor_snapshots_may_backfill_prior_status_events !== false ||
    p.unknown_and_unresolved_events_preserved !== true ||
    p.formula_authority !== false ||
    p.runtime_authority !== false ||
    p.provider_rdps_credit_allowed !== false
  ) {
    throw new Error("Unsafe or incompatible provider ownership proof");
  }
  const inputSessions = new Set();
  for (const input of proof.inputs ?? []) {
    if (String(input.game_build) !== build || typeof input.session_id !== "string" ||
      !input.session_id || typeof input.sha256 !== "string" || !input.sha256.startsWith("sha256:")) {
      throw new Error("Provider ownership proof has an invalid exact input descriptor");
    }
    inputSessions.add(input.session_id);
  }
  if (inputSessions.size === 0) throw new Error("Provider ownership proof has no exact inputs");

  const resolutions = new Map();
  const playerOwnedClasses = new Set([
    "direct_player",
    "owned_by_player",
    "same_wire_packet_owned_by_player",
    "prior_status_instance_player",
  ]);
  for (const row of proof.resolutions ?? []) {
    if (!playerOwnedClasses.has(row.class)) continue;
    if (!inputSessions.has(row.session_id)) {
      throw new Error("Provider ownership resolution references an unknown input session");
    }
    const effectId = positiveIntegerOrNull(row.effect_id);
    const source = entityIdentity(row.source?.entity_uuid, row.source?.actor_id);
    const resolvedOwner = entityIdentity(
      row.resolved_owner?.entity_uuid ?? row.source?.entity_uuid,
      row.resolved_owner?.actor_id ?? row.source?.actor_id,
    );
    const ownerKind = row.resolved_owner?.kind ?? row.source?.kind;
    const characterId = row.resolved_owner?.character_id ?? row.source?.character_id;
    if (effectId == null || !source || !resolvedOwner || ownerKind !== "player" ||
      typeof characterId !== "string" || !characterId) {
      throw new Error("Provider ownership resolution lacks a stable proven player owner");
    }
    const key = providerOwnershipKey(row.session_id, effectId, source);
    const candidate = {
      resolution_class: row.class,
      resolved_owner_identity: resolvedOwner,
      resolved_owner_character_id: characterId,
      ownership_chain: row.ownership_chain ?? [],
    };
    const previous = resolutions.get(key);
    if (previous && (previous.resolved_owner_identity.key !== resolvedOwner.key ||
      previous.resolved_owner_character_id !== characterId)) {
      throw new Error(`Conflicting provider ownership resolutions for ${key}`);
    }
    if (!previous) resolutions.set(key, candidate);
  }
  return {
    descriptor: {
      path: file,
      bytes: fs.statSync(file).size,
      sha256: crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex"),
      schema_version: Number(proof.schema_version),
      tool: proof.tool,
    },
    resolutions,
  };
}

function providerOwnershipKey(sessionId, effectId, provider) {
  return `${sessionId}|${effectId}|${provider.key}`;
}

function resolveProviderOwnership(ownershipProof, sessionId, effectId, provider) {
  return ownershipProof.resolutions.get(providerOwnershipKey(sessionId, effectId, provider)) ?? null;
}

function classifyRawProviderRelationship(provider, actor, target) {
  const relationship = classifyProviderRelationship(provider, actor, target);
  return relationship === "provider-distinct-from-damage-actor-and-target"
    ? "raw-provider-distinct-from-damage-actor-and-target"
    : relationship.replace(/^provider-/, "raw-provider-");
}

function classifyProviderRelationship(provider, actor, target) {
  if (actor && provider.key === actor.key) return "provider-equals-damage-actor";
  if (target && provider.key === target.key) return "provider-equals-damage-target";
  return "provider-distinct-from-damage-actor-and-target";
}

function entityIdentity(entityUuid, actorId) {
  if (entityUuid != null) {
    return { key: `uuid:${entityUuid}`, entity_uuid: String(entityUuid), actor_id: actorId ?? null };
  }
  if (actorId != null) {
    return { key: `actor:${actorId}`, entity_uuid: null, actor_id: String(actorId) };
  }
  return null;
}

function lifecycleIdentityReason(effectId, provider, endpoint, instanceId) {
  if (effectId == null) return "missing-numeric-effect-id";
  if (!provider) return "missing-provider-identity";
  if (!endpoint) return "missing-affected-endpoint-identity";
  if (instanceId == null) return "missing-status-instance-id";
  return "unknown-lifecycle-identity-gap";
}

function addIdentity(set, identity) {
  if (identity) set.add(identity.key);
}

function addIfPresent(set, value) {
  const number = integerOrNull(value);
  if (number != null) set.add(number);
}

function positiveIntegerOrNull(value) {
  const number = integerOrNull(value);
  return number != null && number > 0 ? number : null;
}

function integerOrNull(value) {
  if (value == null) return null;
  const number = Number(value);
  return Number.isSafeInteger(number) ? number : null;
}

function difference(left, right) {
  const a = integerOrNull(left);
  const b = integerOrNull(right);
  return a == null || b == null ? null : a - b;
}

function sortedNumbers(values) {
  return [...values].sort((a, b) => a - b);
}

function sortedCounts(counts) {
  return Object.entries(counts)
    .sort(([a], [b]) => a.localeCompare(b, "en"))
    .map(([key, count]) => ({ key, count }));
}

function compareEffectIds(left, right) {
  if (left === "<unresolved>") return 1;
  if (right === "<unresolved>") return -1;
  return Number(left) - Number(right);
}

function increment(counts, key) {
  counts[key] = (counts[key] ?? 0) + 1;
}

function contentHash(value) {
  const clone = structuredClone(value);
  delete clone.content_sha256;
  return crypto.createHash("sha256").update(stableStringify(clone)).digest("hex");
}

function stableStringify(value) {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) =>
      `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function fileSha256(file) {
  return new Promise((resolve, reject) => {
    const hash = crypto.createHash("sha256");
    const input = fs.createReadStream(file);
    input.on("data", (chunk) => hash.update(chunk));
    input.on("error", reject);
    input.on("end", () => resolve(hash.digest("hex")));
  });
}

function readJson(file) {
  requireFile(file, "JSON artifact");
  return JSON.parse(fs.readFileSync(file, "utf8"));
}

function requireFile(file, label) {
  if (!fs.existsSync(file) || !fs.statSync(file).isFile()) {
    throw new Error(`Missing ${label}: ${file}`);
  }
}

function refuseExisting(files) {
  for (const file of files) if (fs.existsSync(file)) throw new Error(`Refusing to overwrite ${file}`);
}

function required(values, key) {
  if (!values[key]) throw new Error(`Missing --${key}`);
  return String(values[key]);
}

function parsePositiveInteger(value, label) {
  const number = Number(value);
  if (!Number.isSafeInteger(number) || number <= 0) {
    throw new Error(`${label} must be a positive safe integer`);
  }
  return number;
}

function parseArgs(values) {
  const result = {};
  for (let index = 0; index < values.length; index += 2) {
    if (!values[index]?.startsWith("--") || values[index + 1] == null) {
      throw new Error("Options must be --name value pairs");
    }
    result[values[index].slice(2)] = values[index + 1];
  }
  return result;
}

async function selfTest() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "bpsr-lifecycle-action-correlation-"));
  try {
    const timeline = path.join(root, "timeline.jsonl");
    const ownershipProofPath = path.join(root, "ownership-proof.json");
    const ledger = path.join(root, "ledger.jsonl");
    const summary = path.join(root, "summary.json");
    const base = {
      row_type: "relationship",
      schema_version: EXPECTED_TIMELINE_SCHEMA,
      session_id: "test-run",
      canonical_event_payload_retained_in_source_rlog: true,
    };
    const rows = [
      {
        row_type: "manifest",
        schema_version: EXPECTED_TIMELINE_SCHEMA,
        projection: "canonical-who-did-what-id-to-which-target-timeline",
        rlog_count: 1,
        topology: {
          effect_edge: "provider -> effect/status lifecycle -> recipient or enemy target",
          damage_edge: "recipient damage action -> recipient or enemy target",
          source_side_join: "effect endpoint equals damage actor",
          target_side_join: "effect endpoint equals damage target",
        },
        policy: {
          exact_numeric_ids_and_build_are_authoritative: true,
          team_attribute_interpretation_build: "24687926",
          remote_player_cast_packets_required: false,
          remote_player_cast_packets_synthesized: false,
          remote_player_cast_packets_treated_as_zero: false,
          provider_credit_authorized_by_timeline_presence_alone: false,
          unknown_effects_are_preserved: true,
        },
      },
      {
        row_type: "run_header",
        schema_version: EXPECTED_TIMELINE_SCHEMA,
        client_build: "24687926",
        session_id: "test-run",
        protocol_pack_digest: "sha256:test",
        source_path: "test.rlog",
      },
      {
        ...base,
        event_kind: "status",
        canonical_source_rlog_sequence: 10,
        capture_sequence: 4,
        observed_micros: 1_000_000,
        effect_id: 123,
        provider_entity_uuid: "P",
        provider_actor_id: "1",
        affected_entity_uuid: "R",
        affected_entity_actor_id: "2",
        status_instance_id: 9,
        status_state: "applied",
        status_stacks: 1,
      },
      {
        ...base,
        event_kind: "damage",
        canonical_source_rlog_sequence: 11,
        capture_sequence: 5,
        observed_micros: 1_100_000,
        action_id: 456,
        reported_amount: 100,
        damage_actor_entity_uuid: "R",
        damage_actor_id: "2",
        damage_target_entity_uuid: "T",
        damage_target_actor_id: "3",
      },
      {
        ...base,
        event_kind: "damage",
        canonical_source_rlog_sequence: 12,
        capture_sequence: 6,
        observed_micros: 1_200_000,
        action_id: 789,
        reported_amount: 50,
        damage_actor_entity_uuid: "A",
        damage_actor_id: "4",
        damage_target_entity_uuid: "R",
        damage_target_actor_id: "2",
      },
      {
        ...base,
        event_kind: "status",
        canonical_source_rlog_sequence: 13,
        capture_sequence: 7,
        observed_micros: 1_300_000,
        effect_id: 123,
        provider_entity_uuid: "P",
        provider_actor_id: "1",
        affected_entity_uuid: "R",
        affected_entity_actor_id: "2",
        status_instance_id: 9,
        status_state: "removed",
        status_stacks: 0,
      },
      {
        ...base,
        event_kind: "damage",
        canonical_source_rlog_sequence: 14,
        capture_sequence: 8,
        observed_micros: 1_400_000,
        action_id: 456,
        reported_amount: 75,
        damage_actor_entity_uuid: "R",
        damage_actor_id: "2",
        damage_target_entity_uuid: "T",
        damage_target_actor_id: "3",
      },
    ];
    fs.writeFileSync(timeline, `${rows.map((row) => JSON.stringify(row)).join("\n")}\n`);
    fs.writeFileSync(ownershipProofPath, `${JSON.stringify({
      schema_version: 5,
      tool: "rlogs-bpsr-status-effect-provider-ownership-proof",
      game_build: "24687926",
      policy: {
        scope: "provider_ownership_only",
        exact_numeric_effect_ids_authoritative: true,
        exact_input_build_authoritative: true,
        localized_names_are_evidence_only: true,
        actor_kind_or_packet_proven_ancestry_required_for_player_ownership: true,
        future_actor_snapshots_may_backfill_prior_status_events: false,
        unknown_and_unresolved_events_preserved: true,
        formula_authority: false,
        runtime_authority: false,
        provider_rdps_credit_allowed: false,
      },
      inputs: [{
        path: "test.rlog",
        bytes: 1,
        sha256: "sha256:test",
        session_id: "test-run",
        game_build: "24687926",
      }],
      resolutions: [{
        session_id: "test-run",
        run_ordinal: 0,
        effect_id: 123,
        class: "owned_by_player",
        source: { actor_id: 1, entity_uuid: "P", kind: "unknown:0" },
        resolved_owner: {
          actor_id: 4,
          entity_uuid: "A",
          kind: "player",
          character_id: "character-A",
        },
        ownership_chain: [{
          child_actor_id: 1,
          child_entity_uuid: "P",
          owner_actor_id: 4,
          owner_entity_uuid: "A",
          attributed_combat_source: true,
          confirmed_entity_attributes: false,
        }],
      }],
    }, null, 2)}\n`);
    const ownershipProof = loadProviderOwnershipProof(ownershipProofPath, "24687926");
    const result = await buildLedger({
      build: "24687926",
      timeline,
      ledger,
      maxCorrelationRows: 100,
      ownershipProof,
    });
    const report = buildSummary({
      build: "24687926",
      timeline,
      ledger,
      maxCorrelationRows: 100,
      ownershipProof,
      ...result,
    });
    report.content_sha256 = contentHash(report);
    fs.writeFileSync(summary, `${JSON.stringify(report, null, 2)}\n`);
    await verifyArtifacts(summary, ledger);
    assert.equal(report.summary.damage_action_count, 3);
    assert.equal(report.summary.damage_actions_with_active_lifecycle_correlation, 2);
    assert.equal(report.summary.damage_actions_without_active_lifecycle_correlation, 1);
    assert.equal(report.summary.correlation_row_count, 2);
    assert.equal(report.summary.raw_provider_distinct_correlation_count, 2);
    assert.equal(report.summary.provider_ownership_resolved_correlation_count, 2);
    assert.equal(report.summary.provider_ownership_unresolved_correlation_count, 0);
    assert.equal(report.summary.proven_third_party_provider_correlation_count, 1);
    assert.deepEqual(report.summary.relationship_role_counts, [
      { key: "source-side", count: 1 },
      { key: "target-side", count: 1 },
    ]);
    assert.equal(report.effects[0].closed_window_count, 1);
    assert.equal(report.effects[0].closed_window_correlation_count, 2);
    assert.equal(report.effects[0].closed_window_proven_third_party_correlation_count, 1);
    assert.equal(
      report.effects[0].closed_window_proven_third_party_source_side_correlation_count,
      1,
    );
    assert.equal(report.effects[0].open_window_at_run_end_count, 0);
    assert.equal(report.effects[0].open_window_correlation_count, 0);
    assert.equal(report.conclusion.provider_rdps_credit_allowed, false);
    process.stdout.write("bpsr lifecycle/action correlation ledger self-test passed\n");
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
}

function usage(code) {
  process.stdout.write(
    "Usage:\n" +
      "  node tools/bpsr-lifecycle-action-correlation-ledger.mjs generate " +
      "--build <id> --timeline <support-timeline.jsonl> " +
      "--ownership-proof <provider-ownership-proof.json> --ledger <ledger.jsonl> " +
      "--summary <summary.json> [--max-correlation-rows <n>]\n" +
      "  node tools/bpsr-lifecycle-action-correlation-ledger.mjs verify " +
      "--summary <summary.json> --ledger <ledger.jsonl>\n" +
      "  node tools/bpsr-lifecycle-action-correlation-ledger.mjs self-test\n",
  );
  process.exit(code);
}
