#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const options = parseArgs(process.argv.slice(2));
const gameBuild = String(options.gameBuild);
assert(/^\d+$/.test(gameBuild), "--gameBuild must be a numeric client build");

const attributeAuditPath = resolvePath(options.attributeAudit);
const evaluatorProofPath = resolvePath(options.evaluatorProof);
const gapWindowAuditPath = resolvePath(options.gapWindowAudit);
const opportunityAuditPath = resolvePath(options.opportunityAudit);
const outputPath = resolvePath(options.output);
const attributeAudit = readJson(attributeAuditPath, "attribute-family packet audit");
const evaluatorProof = readJson(evaluatorProofPath, "fight-attribute evaluator proof");
const gapWindowAudit = readJson(gapWindowAuditPath, "gap-window audit");
const opportunityAudit = readJson(opportunityAuditPath, "haste-opportunity audit");

assert(Number(attributeAudit.schema_version) === 6,
  `attribute audit schema must be 6, observed ${attributeAudit.schema_version}`);
assert(String(attributeAudit.build_scope?.expected_game_build) === gameBuild,
  "attribute audit expected build does not match --gameBuild");
assert(attributeAudit.build_scope?.recording_build_identity_authority === false,
  "attribute audit unexpectedly claims authoritative recording build identity");
assert(Number(evaluatorProof.schema_version) === 2,
  `evaluator proof schema must be 2, observed ${evaluatorProof.schema_version}`);
assert(String(evaluatorProof.game_build) === gameBuild,
  "evaluator proof build does not match --gameBuild");
assert(evaluatorProof.proof_state === "exact-current-build-client-ui-evaluator",
  `unexpected evaluator proof state ${evaluatorProof.proof_state}`);
assert(evaluatorProof.policy?.attribute_to_transform_mapping_is_exact === true,
  "evaluator proof does not prove the transform-field mapping");
assert(evaluatorProof.policy?.raw_to_transformed_attribute_mapping_is_exact === true,
  "evaluator proof does not prove the raw-attribute mapping");
assert(evaluatorProof.policy?.formula_operation_order_is_exact === true,
  "evaluator proof does not prove operation order");

assert(Number(gapWindowAudit.schema_version) === 2,
  `gap-window audit schema must be 2, observed ${gapWindowAudit.schema_version}`);
assert(gapWindowAudit.generated_by === "rlogs-bpsr-rlog-gap-window-audit",
  "unexpected gap-window audit generator");
assert(String(gapWindowAudit.game_build) === gameBuild,
  "gap-window audit build does not match --gameBuild");
assert(Number(gapWindowAudit.effect_id) === 2207252,
  "gap-window audit does not select effect 2207252");
assert(gapWindowAudit.policy?.sealed_rlogs_are_streamed_one_event_at_a_time === true,
  "gap-window audit is not bounded-memory streaming evidence");
assert(gapWindowAudit.policy?.every_data_gap_and_recorder_pause_is_an_exclusion_boundary === true,
  "gap-window audit does not exclude every data-quality boundary");
assert(gapWindowAudit.policy?.status_lifecycles_never_cross_exclusion_or_run_boundaries === true,
  "gap-window audit allows status lifecycles to cross a boundary");
assert(gapWindowAudit.policy?.complete_gap_bounded_lifecycle_is_not_counterfactual_formula_proof === true,
  "gap-window audit incorrectly treats lifecycle completeness as formula proof");
assert(gapWindowAudit.policy?.packet_absence_is_not_zero === true,
  "gap-window audit treats packet absence as zero");
assert(gapWindowAudit.policy
  ?.structurally_unobservable_remote_player_packets_are_not_acquisition_requirements === true,
"gap-window audit requires structurally unobservable remote-player packets");
assert(gapWindowAudit.policy?.current_snapshots_are_never_backfilled_into_historical_windows === true,
  "gap-window audit permits current snapshot backfill");
assert(gapWindowAudit.policy?.formula_authority === false &&
  gapWindowAudit.policy?.runtime_authority === false &&
  gapWindowAudit.policy?.provider_rdps_credit_allowed === false,
"gap-window audit has unsafe authority flags");

const gapSummary = gapWindowAudit.summary || {};
const lifecycleAccounting =
  Number(gapSummary.selected_effect_complete_gap_bounded_lifecycle_count) +
  Number(gapSummary.selected_effect_lifecycles_cut_by_data_quality_boundary) +
  Number(gapSummary.selected_effect_lifecycles_cut_by_run_boundary) +
  Number(gapSummary.selected_effect_open_at_end_of_log);
assert(Number(gapSummary.source_rlog_count) > 0 &&
  Number(gapSummary.source_rlog_count) === Number(gapSummary.sealed_rlog_count),
"gap-window audit source/sealed RLOG accounting is incomplete");
assert(Number(gapSummary.selected_effect_applied_count) === lifecycleAccounting,
  "gap-window audit does not account for every selected-effect application");
assert(Number(gapSummary.selected_effect_terminal_count) ===
  Number(gapSummary.selected_effect_complete_gap_bounded_lifecycle_count) +
    Number(gapSummary.selected_effect_unmatched_terminal_events),
"gap-window audit does not account for every selected-effect terminal event");
assert(gapSummary.exact_damage_projection_proven === false &&
  gapSummary.exact_operation_order_proven === false &&
  gapSummary.exact_integer_rounding_proven === false &&
  gapSummary.packet_conservation_proven === false &&
  gapSummary.formula_authority === false &&
  gapSummary.runtime_authority === false &&
  gapSummary.provider_rdps_credit_allowed === false,
"gap-window audit summary has unsafe authority flags");

assert(Number(opportunityAudit.schema_version) === 3,
  `opportunity audit schema must be 3, observed ${opportunityAudit.schema_version}`);
assert(opportunityAudit.generated_by === "rlogs-bpsr-haste-opportunity-proof",
  "unexpected haste-opportunity audit generator");
assert(String(opportunityAudit.build_scope?.expected_game_build) === gameBuild,
  "haste-opportunity audit expected build does not match --gameBuild");
assert(opportunityAudit.build_scope?.recording_build_identity_authority === false,
  "haste-opportunity audit unexpectedly claims recording build identity authority");
assert(Number(opportunityAudit.effect_id) === 2207252,
  "haste-opportunity audit does not select effect 2207252");
assert(opportunityAudit.policy?.runtime_formula_authority === false &&
  opportunityAudit.policy?.current_build_static_metadata_is_runtime_authority === false &&
  opportunityAudit.policy?.unresolved_evidence_hidden === false &&
  opportunityAudit.policy?.remote_player_packets_required === false &&
  opportunityAudit.policy?.missing_provider_is_no_external_status === false,
"haste-opportunity audit policy is unsafe");
const opportunitySummary = opportunityAudit.summary || {};
assert(Number(opportunitySummary.sessions) === Number(gapSummary.source_rlog_count),
  "haste-opportunity and gap-window audits cover different session counts");
assert(Number(opportunitySummary.status_windows_started) ===
  Number(gapSummary.selected_effect_applied_count),
"haste-opportunity and gap-window audits disagree on selected-effect applications");
assert(Number(opportunitySummary.cast_start_events) === 0 &&
  opportunitySummary.local_cast_start_coverage_observed === false &&
  opportunitySummary.opportunity_proof_eligible === false &&
  opportunitySummary.provider_rdps_credit_allowed === false,
"haste-opportunity audit unexpectedly claims observable opportunity evidence");
assert(opportunitySummary.zero_cast_interpretation ===
  "unobserved action-start coverage; never zero actions, zero opportunity, or mechanic disproof",
"haste-opportunity zero-cast interpretation is unsafe");

const expectedExpression =
  "100 * raw * p3 / (raw * p2 + p1 + min(season_level * p4, p5) + min(role_level * p6, p7))";
assert(evaluatorProof.summary?.evaluator_formula === expectedExpression,
  "evaluator expression changed");

const hasteMapping = (evaluatorProof.numeric_attribute_mappings || [])
  .find((row) => row.transform_field === "HasteToHastePct");
assert(hasteMapping, "evaluator proof has no HasteToHastePct numeric mapping");
assert(Number(hasteMapping.raw_attribute_id) === 11120,
  `Haste raw attribute changed from 11120 to ${hasteMapping.raw_attribute_id}`);
assert(Number(hasteMapping.target_attribute_id) === 11930,
  `HastePct target attribute changed from 11930 to ${hasteMapping.target_attribute_id}`);

const transformRows = (evaluatorProof.rows || []).map((row) => {
  const field = row.fields?.HasteToHastePct;
  assert(field?.state === "exact-current-build-parameter-array",
    `season ${row.season_id} HasteToHastePct is not an exact parameter array`);
  const parameters = (field.parameters || []).map(Number);
  assert(parameters.length === 7,
    `season ${row.season_id} HasteToHastePct must have seven parameters`);
  assert(parameters.every(Number.isSafeInteger),
    `season ${row.season_id} HasteToHastePct contains a non-integer parameter`);
  assert(parameters.slice(3).every((value) => value === 0),
    `season ${row.season_id} HasteToHastePct needs season/role levels that the packet audit does not contain`);
  assert(field.exact_expression === expectedExpression,
    `season ${row.season_id} HasteToHastePct expression changed`);
  return { season_id: Number(row.season_id), parameters };
});
assert(transformRows.length === 3, `expected three transform rows, observed ${transformRows.length}`);

const cross = attributeAudit.cross_family_transition_selection;
assert(cross, "attribute audit has no cross-family transition selection");
const requiredFamilies = [11120, 11720, 11730, 11930];
for (const family of requiredFamilies) {
  assert((cross.selected_family_base_ids || []).includes(family),
    `attribute audit did not select family ${family}`);
}

const patterns = cross.patterns || [];
const examples = patterns.flatMap((pattern) => (pattern.examples || []).map((example) => ({
  pattern_count: Number(pattern.count),
  all_selected_families_present: pattern.all_selected_families_present === true,
  ...example,
})));
const representedBatches = patterns.reduce((sum, pattern) => sum + Number(pattern.count || 0), 0);
const exampleCoverageComplete = representedBatches === examples.length;
assert(exampleCoverageComplete,
  `captured examples cover ${examples.length} of ${representedBatches} anchored batches; rerun with a larger example limit`);

const roundingModes = [
  "trunc_toward_zero",
  "floor",
  "ceil",
  "nearest_half_away_from_zero",
  "nearest_half_to_even",
];

const packetTransitions = examples.map((example) => {
  assert(example.all_selected_families_present,
    `sequence ${example.sequence} is missing a selected family`);
  const raw = currentTransition(example, 11120);
  const hastePct = currentTransition(example, 11930);
  const castSpeed = currentTransition(example, 11730);
  const attackSpeed = currentTransition(example, 11720);
  const rowMatches = [];
  const constantAdditiveResidualMatches = [];
  for (const row of transformRows) {
    const modes = [];
    const constantResidualModes = [];
    for (const rounding of roundingModes) {
      const predictedBefore = evaluateBasisPoints(raw.before, row.parameters, rounding);
      const predictedAfter = evaluateBasisPoints(raw.after, row.parameters, rounding);
      if (predictedBefore === hastePct.before && predictedAfter === hastePct.after) {
        modes.push(rounding);
      }
      const beforeResidual = hastePct.before - predictedBefore;
      const afterResidual = hastePct.after - predictedAfter;
      if (beforeResidual === afterResidual) {
        constantResidualModes.push({ rounding, additive_residual: beforeResidual });
      }
    }
    if (modes.length > 0) rowMatches.push({ season_id: row.season_id, rounding_modes: modes });
    if (constantResidualModes.length > 0) {
      constantAdditiveResidualMatches.push({
        season_id: row.season_id,
        rounding_modes: constantResidualModes,
      });
    }
  }
  return {
    session_id: example.session_id,
    rlog: example.rlog,
    run_ordinal: Number(example.run_ordinal),
    sequence: Number(example.sequence),
    observed_micros: Number(example.observed_micros),
    actor_entity_uuid: Number(example.actor_entity_uuid),
    matched_anchors: example.matched_anchors,
    raw_haste: raw,
    haste_pct: hastePct,
    cast_speed_pct: castSpeed,
    attack_speed_pct: attackSpeed,
    exact_row_and_rounding_matches: rowMatches,
    curve_plus_constant_additive_residual_matches: constantAdditiveResidualMatches,
    cast_speed_equals_haste_pct_before_and_after:
      castSpeed.before === hastePct.before && castSpeed.after === hastePct.after,
  };
});

const rowRoundingChecks = [];
for (const row of transformRows) {
  for (const rounding of roundingModes) {
    let exactAbsoluteBatches = 0;
    let exactDeltaBatches = 0;
    let constantAdditiveResidualBatches = 0;
    const additiveResiduals = new Set();
    const evaluations = [];
    for (const packet of packetTransitions) {
      const predictedBefore = evaluateBasisPoints(packet.raw_haste.before, row.parameters, rounding);
      const predictedAfter = evaluateBasisPoints(packet.raw_haste.after, row.parameters, rounding);
      const predictedDelta = predictedAfter - predictedBefore;
      const observedDelta = packet.haste_pct.delta;
      const beforeResidual = packet.haste_pct.before - predictedBefore;
      const afterResidual = packet.haste_pct.after - predictedAfter;
      const deltaResidual = observedDelta - predictedDelta;
      if (beforeResidual === 0 && afterResidual === 0) exactAbsoluteBatches += 1;
      if (deltaResidual === 0) exactDeltaBatches += 1;
      if (beforeResidual === afterResidual) {
        constantAdditiveResidualBatches += 1;
        additiveResiduals.add(beforeResidual);
      }
      evaluations.push({
        session_id: packet.session_id,
        run_ordinal: packet.run_ordinal,
        sequence: packet.sequence,
        raw_before: packet.raw_haste.before,
        raw_after: packet.raw_haste.after,
        observed_before: packet.haste_pct.before,
        observed_after: packet.haste_pct.after,
        predicted_before: predictedBefore,
        predicted_after: predictedAfter,
        before_residual: beforeResidual,
        after_residual: afterResidual,
        observed_delta: observedDelta,
        predicted_delta: predictedDelta,
        delta_residual: deltaResidual,
      });
    }
    rowRoundingChecks.push({
      season_id: row.season_id,
      parameters: row.parameters,
      rounding,
      evaluable_batches: packetTransitions.length,
      exact_absolute_batches: exactAbsoluteBatches,
      exact_delta_batches: exactDeltaBatches,
      constant_additive_residual_batches: constantAdditiveResidualBatches,
      observed_additive_residuals: [...additiveResiduals].sort((left, right) => left - right),
      mismatched_absolute_batches: packetTransitions.length - exactAbsoluteBatches,
      evaluations,
    });
  }
}

const exactCastSpeedIdentityBatches = packetTransitions
  .filter((packet) => packet.cast_speed_equals_haste_pct_before_and_after).length;
const packetsWithAnyExactCurveMatch = packetTransitions
  .filter((packet) => packet.exact_row_and_rounding_matches.length > 0).length;
const rowThreeTruncation = rowRoundingChecks.find((check) =>
  check.season_id === 3 && check.rounding === "trunc_toward_zero");
assert(rowThreeTruncation, "season row 3 truncation check is missing");
const uniqueSessions = new Set(packetTransitions.map((packet) => packet.session_id)).size;
const actorRuns = new Set(packetTransitions
  .map((packet) => `${packet.session_id}:${packet.run_ordinal}:${packet.actor_entity_uuid}`)).size;

const result = {
  schema_version: 2,
  generated_by: "tools/haste-transform-packet-proof.mjs",
  game: "blue-protocol-star-resonance",
  game_build: gameBuild,
  proof_state: "current-build-local-packet-curve-diagnostic-not-runtime-authority",
  inputs: {
    attribute_family_packet_audit: artifactIdentity(attributeAuditPath),
    fight_attribute_transform_evaluator_proof: artifactIdentity(evaluatorProofPath),
    selected_effect_gap_window_audit: artifactIdentity(gapWindowAuditPath),
    haste_opportunity_audit: artifactIdentity(opportunityAuditPath),
  },
  policy: {
    exact_numeric_attribute_ids_are_authoritative: true,
    localized_names_are_runtime_keys: false,
    missing_or_unobserved_values_are_zero: false,
    remote_player_packets_required: false,
    remote_player_packet_policy:
      "permanently absent remote-player packets remain unobservable and are never synthesized or replaced with current snapshots",
    ordinary_damage_is_retained: true,
    client_ui_evaluator_formula_authority: true,
    recording_build_identity_authority: false,
    exact_season_row_selection_for_each_recording: false,
    combat_transform_formula_authority: false,
    gap_bounded_lifecycle_is_formula_authority: false,
    missing_action_start_packets_mean_zero_actions: false,
    hypothetical_extra_actions_or_damage_may_be_invented: false,
    support_effect_causal_authority: false,
    provider_attribution_authority: false,
    runtime_authority: false,
    rdps_ui_display_authority: false,
    promotion_requirement:
      "prove recording protocol-pack identity, exact season row per run, any co-effect stages, combat-side operation order and integer rounding, provider ownership, recipient/action scope, and damage conservation",
  },
  transform_contract: {
    raw_attribute_id: 11120,
    transformed_attribute_id: 11930,
    client_ui_expression: expectedExpression,
    packet_fixed_point_scale: 10000,
    transform_rows: transformRows,
    rounding_modes_tested: roundingModes,
  },
  opportunity_contract: {
    effect_id: 2207252,
    evidence_state:
      "exact-stat-transform-and-gap-bounded-lifecycles-proven-action-opportunity-unobservable",
    source_rlogs: Number(gapSummary.source_rlog_count),
    canonical_events: Number(gapSummary.canonical_event_count),
    data_quality_boundaries: Number(gapSummary.data_gap_count) +
      Number(gapSummary.recorder_pause_count),
    status_windows_started: Number(gapSummary.selected_effect_applied_count),
    complete_gap_bounded_lifecycles:
      Number(gapSummary.selected_effect_complete_gap_bounded_lifecycle_count),
    complete_windows_with_observed_damage:
      Number(gapSummary.selected_effect_complete_windows_with_damage_count),
    observed_damage_events_while_active:
      Number(gapSummary.selected_effect_damage_events_while_active),
    lifecycles_cut_by_data_quality_boundary:
      Number(gapSummary.selected_effect_lifecycles_cut_by_data_quality_boundary),
    observed_action_start_events: Number(opportunitySummary.cast_start_events),
    local_action_start_coverage_observed: false,
    zero_action_inference_allowed: false,
    extra_action_counterfactual_proven: false,
    observed_damage_reassigned_to_provider: 0,
    formula_authority: false,
    runtime_authority: false,
    rdps_ui_display_authority: false,
    provider_rdps_credit_allowed: false,
  },
  summary: {
    anchored_batches: Number(cross.observed_batches),
    represented_batches: representedBatches,
    example_coverage_complete: exampleCoverageComplete,
    complete_selected_family_batches: Number(cross.complete_selected_family_batches),
    incomplete_selected_family_batches: Number(cross.incomplete_selected_family_batches),
    independent_sessions: uniqueSessions,
    actor_runs: actorRuns,
    packets_with_any_exact_curve_match: packetsWithAnyExactCurveMatch,
    packets_without_exact_curve_match: packetTransitions.length - packetsWithAnyExactCurveMatch,
    row_3_truncation_exact_delta_batches: rowThreeTruncation.exact_delta_batches,
    row_3_truncation_constant_additive_residual_batches:
      rowThreeTruncation.constant_additive_residual_batches,
    row_3_truncation_observed_additive_residuals:
      rowThreeTruncation.observed_additive_residuals,
    row_3_rounding_resolution:
      "truncation and floor are observationally identical for the positive packet inputs; exact absolute values reject ceil and nearest rounding",
    exact_cast_speed_haste_pct_identity_batches: exactCastSpeedIdentityBatches,
    attack_speed_transform_state:
      "observed before/after values retained; no formula inferred because the same Haste anchor produces state-dependent AttackSpeed deltas",
    gap_bounded_effect_lifecycles:
      Number(gapSummary.selected_effect_complete_gap_bounded_lifecycle_count),
    gap_bounded_effect_windows_with_damage:
      Number(gapSummary.selected_effect_complete_windows_with_damage_count),
    observed_damage_events_in_gap_bounded_effect_windows:
      Number(gapSummary.selected_effect_damage_events_while_active),
    action_start_events_observed: Number(opportunitySummary.cast_start_events),
    haste_opportunity_formula_state:
      "unresolved because action-start coverage is structurally unavailable; never interpreted as zero opportunity",
  },
  packet_transitions: packetTransitions,
  row_rounding_checks: rowRoundingChecks,
};

assert(!existsSync(outputPath), `refusing to overwrite ${outputPath}`);
mkdirSync(path.dirname(outputPath), { recursive: true });
writeFileSync(outputPath, `${JSON.stringify(result, null, 2)}\n`, "utf8");
console.log(JSON.stringify(result.summary, null, 2));

function currentTransition(example, attributeId) {
  const matches = (example.member_transitions || [])
    .filter((row) => Number(row.attribute_id) === attributeId && Number(row.offset) === 0);
  assert(matches.length === 1,
    `sequence ${example.sequence} expected one current transition for ${attributeId}, found ${matches.length}`);
  const row = matches[0];
  const before = Number(row.before);
  const after = Number(row.after);
  const delta = Number(row.delta);
  assert(Number.isSafeInteger(before) && Number.isSafeInteger(after) && Number.isSafeInteger(delta),
    `sequence ${example.sequence} transition ${attributeId} is outside safe integer range`);
  assert(after - before === delta,
    `sequence ${example.sequence} transition ${attributeId} delta does not equal after-before`);
  return { before, after, delta };
}

function evaluateBasisPoints(raw, parameters, rounding) {
  const [p1, p2, p3, p4, p5, p6, p7] = parameters.map(BigInt);
  const rawValue = BigInt(raw);
  const denominator = rawValue * p2 + p1 + minBigInt(0n * p4, p5) + minBigInt(0n * p6, p7);
  assert(denominator > 0n, `non-positive Haste transform denominator ${denominator}`);
  const numerator = 10000n * rawValue * p3;
  return Number(divideWithRounding(numerator, denominator, rounding));
}

function divideWithRounding(numerator, denominator, mode) {
  const quotient = numerator / denominator;
  const remainder = numerator % denominator;
  if (remainder === 0n || mode === "trunc_toward_zero") return quotient;
  if (mode === "floor") return numerator < 0n ? quotient - 1n : quotient;
  if (mode === "ceil") return numerator > 0n ? quotient + 1n : quotient;
  const doubled = absBigInt(remainder) * 2n;
  if (doubled < denominator) return quotient;
  if (doubled > denominator) return quotient + (numerator < 0n ? -1n : 1n);
  if (mode === "nearest_half_away_from_zero") {
    return quotient + (numerator < 0n ? -1n : 1n);
  }
  assert(mode === "nearest_half_to_even", `unknown rounding mode ${mode}`);
  return quotient % 2n === 0n ? quotient : quotient + (numerator < 0n ? -1n : 1n);
}

function artifactIdentity(value) {
  const bytes = readFileSync(value);
  return {
    file: relative(value),
    bytes: statSync(value).size,
    sha256: `sha256:${createHash("sha256").update(bytes).digest("hex")}`,
  };
}

function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index];
    if (!key?.startsWith("--") || args[index + 1] === undefined) {
      throw new Error(`invalid argument near ${key ?? "<end>"}`);
    }
    parsed[key.slice(2)] = args[index + 1];
  }
  for (const required of [
    "gameBuild",
    "attributeAudit",
    "evaluatorProof",
    "gapWindowAudit",
    "opportunityAudit",
    "output",
  ]) {
    if (!parsed[required]) throw new Error(`--${required} is required`);
  }
  return parsed;
}

function resolvePath(value) {
  return path.isAbsolute(value) ? value : path.resolve(repoRoot, value);
}

function relative(value) {
  const normalized = path.relative(repoRoot, value).replaceAll("\\", "/");
  return normalized.startsWith("../") ? value.replaceAll("\\", "/") : normalized;
}

function readJson(value, label) {
  try {
    return JSON.parse(readFileSync(value, "utf8"));
  } catch (error) {
    throw new Error(`failed to read ${label} at ${value}: ${error.message}`);
  }
}

function minBigInt(left, right) {
  return left < right ? left : right;
}

function absBigInt(value) {
  return value < 0n ? -value : value;
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}
