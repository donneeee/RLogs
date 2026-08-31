#!/usr/bin/env node

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { createReadStream, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";
import readline from "node:readline";

const GENERATED_BY = "tools/bpsr-rdps-status-lifecycle-coverage.mjs";
const SCHEMA_VERSION = 4;
const CURRENT_PACK =
  "sha256:f3a07130e33ea9f9ba3360920879ffc0a3def59ae0d31a9997f17cb99a218395";
const EFFECT_HARMONY_GRACE = 3_003_052;
const EFFECT_MECHANICAL_POWER = 2_110_140;

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseOptions(rest);
if (command === "generate") await generate(options);
else if (command === "verify") verify(readJson(required(options, "input")));
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

async function generate(values) {
  const build = required(values, "build");
  const formula = source(required(values, "formula-runtime"));
  const classification = source(required(values, "effect-classification"));
  const externalState = source(required(values, "external-state-runtime"));
  const vulnerability = source(required(values, "target-vulnerability-runtime"));
  const psychoscope = source(required(values, "psychoscope-runtime"));
  const catalog = source(required(values, "catalog-manifest"));
  const partyAudit = source(required(values, "party-window-audit"));
  const ownership = source(required(values, "provider-ownership"));
  const correlation = source(required(values, "correlation-summary"));
  const mechanical = source(required(values, "mechanical-power-proof"));
  const mechanicalPromotion = source(required(values, "mechanical-promotion-proof"));
  const harmony = source(required(values, "harmony-grace-proof"));
  const harmonyPromotion = source(required(values, "harmony-promotion-proof"));
  const timelinePath = path.resolve(required(values, "exact-pack-timeline"));

  validateInputs({
    build,
    formula,
    classification,
    externalState,
    vulnerability,
    psychoscope,
    catalog,
    partyAudit,
    ownership,
    correlation,
    mechanical,
    mechanicalPromotion,
    harmony,
    harmonyPromotion,
    timelinePath,
  });
  const timeline = await scanTimeline(timelinePath, build, formula.value.protocol_pack_digest);
  validateCorrelation(correlation.value, timeline, timelinePath, build);

  const registry = new Map();
  const add = (effectId, origin, family, role = "lifecycle-endpoint") => {
    const id = Number(effectId);
    if (!Number.isSafeInteger(id) || id <= 0) return;
    const row = registry.get(id) ?? {
      effect_id: id,
      identity_origins: new Set(),
      mechanic_families: new Set(),
      lifecycle_roles: new Set(),
    };
    row.identity_origins.add(origin);
    if (family) row.mechanic_families.add(family);
    row.lifecycle_roles.add(role);
    registry.set(id, row);
  };

  addFormulaRuntime(formula.value, add);
  for (const entry of classification.value.effects ?? []) {
    add(entry.effect_id, "effect-classification-runtime", entry.contribution_kind);
  }
  for (const entry of externalState.value.rules ?? []) {
    add(entry.effect_id, "external-state-runtime-active", "external-state");
  }
  for (const entry of externalState.value.candidate_rules ?? []) {
    add(entry.rule?.effect_id, "external-state-runtime-candidate", "external-state");
  }
  for (const entry of vulnerability.value.rules ?? []) {
    add(entry.effect_id, "target-vulnerability-runtime", "target-vulnerability");
  }
  for (const entry of psychoscope.value.factors ?? []) {
    add(entry.primary_buff_id, "psychoscope-runtime-candidate", `psychoscope-${entry.category}`);
  }
  for (const entry of partyAudit.value.effects ?? []) {
    add(entry.effect_id, "party-effect-frontier", entry.support_categories?.join("+") || "party-effect");
  }

  const currentCatalog = validateCatalog(catalog, build);
  for (const entry of currentCatalog) add(entry.effect_id, "current-build-reviewed-catalog", entry.review_state);

  const partyById = byEffect(partyAudit.value.effects);
  const ownershipById = byEffect(ownership.value.effects);
  const correlationById = byEffect(correlation.value.effects);
  const specialized = new Map([
    [2_110_140, {
      path: mechanical.path,
      sha256: mechanical.sha256,
      proof_state: "exact-current-pack-selected-lifecycle-replay-equivalent",
      lifecycle_events: mechanical.value.exact_scope.effect_lifecycle_event_count,
      closed_lifecycles: mechanical.value.exact_scope.complete_gap_bounded_lifecycle_count,
      provider_recipient_edge_proven: true,
      ordinary_damage_conserved: mechanical.value.exact_scope.ordinary_damage_conserved,
    }],
    [3_003_052, {
      path: harmony.path,
      sha256: harmony.sha256,
      proof_state: "exact-current-pack-selected-lifecycle-regression-closed",
      lifecycle_events: null,
      closed_lifecycles: harmony.value.identity.lifecycle_instance_ids.length,
      provider_recipient_edge_proven: true,
      ordinary_damage_conserved: harmony.value.proof.ordinary_damage_conserved,
    }],
  ]);

  const effects = [...registry.values()].sort((a, b) => a.effect_id - b.effect_id).map((base) => {
    const id = base.effect_id;
    const generic = correlationById.get(id);
    const prior = partyById.get(id);
    const owner = ownershipById.get(id);
    const special = specialized.get(id) ?? null;
    const exactRows = timeline.statusEffectCounts.get(id) ?? 0;
    const genericClosed = exactRows > 0 && generic &&
      generic.lifecycle_transition_count === exactRows &&
      generic.usable_lifecycle_transition_count === exactRows &&
      generic.unusable_lifecycle_transition_count === 0 &&
      generic.open_window_at_run_end_count === 0;
    const exactClosed = Boolean(special || genericClosed);
    const exactProviderRecipient = Boolean(
      special?.provider_recipient_edge_proven ||
      (generic?.proven_third_party_provider_count > 0 &&
        generic?.provider_ownership_unresolved_count === 0 &&
        generic?.correlation_row_count > 0),
    );
    return {
      effect_id: id,
      identity_origins: [...base.identity_origins].sort(),
      mechanic_families: [...base.mechanic_families].sort(),
      lifecycle_roles: [...base.lifecycle_roles].sort(),
      current_build_catalog: currentCatalog.find((entry) => entry.effect_id === id) ?? null,
      exact_pack_generic_replay: {
        observed_status_rows: exactRows,
        usable_status_rows: generic?.usable_lifecycle_transition_count ?? 0,
        closed_windows: generic?.closed_window_count ?? 0,
        open_windows: generic?.open_window_at_run_end_count ?? 0,
        unresolved_provider_correlations: generic?.provider_ownership_unresolved_count ?? 0,
        proven_third_party_correlations: generic?.proven_third_party_provider_count ?? 0,
      },
      exact_pack_specialized_replay: special,
      current_build_prior_pack_cohort: {
        observed_status_rows: prior?.status_events ?? 0,
        closed_windows: prior?.windows_closed ?? 0,
        open_windows: prior?.windows_open_at_log_end ?? 0,
        missing_wire_sources: prior?.status_events_without_source ?? 0,
        provider_owned_rows: owner?.status_events ?? 0,
        provider_ownership_proven_for_every_sourced_event:
          owner?.player_actor_ownership_proven_for_every_sourced_event ?? false,
      },
      proof_state: exactClosed
        ? (exactProviderRecipient
          ? "exact-current-pack-lifecycle-and-selected-provider-recipient-slice-proven"
          : "exact-current-pack-lifecycle-only-proven")
        : exactRows > 0
          ? "exact-current-pack-lifecycle-incomplete"
          : (prior?.status_events ?? 0) > 0
            ? "current-build-prior-pack-observation-awaiting-exact-pack-replay"
            : "static-or-runtime-candidate-awaiting-exact-pack-observation",
      status_lifecycle_ready: exactClosed,
      provider_recipient_ready: exactProviderRecipient,
      provider_rdps_credit_allowed:
        (id === EFFECT_MECHANICAL_POWER &&
          formula.value.mechanical_power?.runtime_transfer_enabled === true &&
          formula.value.mechanical_power?.class_11_tier_0_exact_rational_attribution_authority === true) ||
        (id === EFFECT_HARMONY_GRACE &&
          formula.value.harmony_grace?.runtime_transfer_enabled === true &&
          formula.value.harmony_grace?.class_11_exact_rational_attribution_authority === true),
    };
  });

  const statusReady = effects.filter((entry) => entry.status_lifecycle_ready);
  const providerReady = effects.filter((entry) => entry.provider_recipient_ready);
  const priorObserved = effects.filter(
    (entry) => entry.current_build_prior_pack_cohort.observed_status_rows > 0,
  );
  const exactObserved = effects.filter(
    (entry) => entry.exact_pack_generic_replay.observed_status_rows > 0 ||
      entry.exact_pack_specialized_replay,
  );
  const missing = effects.filter((entry) => !entry.status_lifecycle_ready);
  const authorized = effects.filter((entry) => entry.provider_rdps_credit_allowed);
  const output = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game_build: build,
    suite_frontier: ["status-lifecycle-replay", "provider-recipient-replay"],
    policy: {
      exact_numeric_effect_ids_build_and_protocol_pack_are_authoritative: true,
      localized_names_are_runtime_keys: false,
      remote_player_cast_packets_required: false,
      remote_player_cast_packets_synthesized: false,
      unknown_and_unresolved_statuses_preserved: true,
      current_build_prior_pack_observations_promote_exact_pack_runtime: false,
      static_game_file_identity_proves_packet_occurrence: false,
      selected_effect_proof_generalizes_to_other_effects: false,
      current_character_snapshots_substituted_into_older_runs: false,
      missing_effect_observation_is_zero_effect: false,
      provider_rdps_credit_allowed: authorized.length > 0,
      runtime_ui_promotion_allowed: authorized.length > 0,
      global_runtime_promotion_allowed: false,
    },
    topology: {
      effect_edge: "provider -> effect/status lifecycle -> recipient or enemy target",
      damage_edge: "recipient damage action -> recipient or enemy target",
      endpoint_allegiance_assumed: false,
    },
    sources: {
      formula_runtime: receipt(formula),
      effect_classification: receipt(classification),
      external_state_runtime: receipt(externalState),
      target_vulnerability_runtime: receipt(vulnerability),
      psychoscope_runtime: receipt(psychoscope),
      catalog_manifest: receipt(catalog),
      party_window_audit: receipt(partyAudit),
      provider_ownership: receipt(ownership),
      exact_pack_timeline: await fileReceipt(timelinePath),
      exact_pack_correlation_summary: receipt(correlation),
      mechanical_power_selected_proof: receipt(mechanical),
      mechanical_power_promotion_proof: receipt(mechanicalPromotion),
      harmony_grace_selected_proof: receipt(harmony),
      harmony_grace_promotion_proof: receipt(harmonyPromotion),
    },
    exact_pack_replay: {
      protocol_pack_digest: timeline.protocolPackDigest,
      run_count: timeline.runCount,
      canonical_events: timeline.canonicalEvents,
      damage_events: timeline.damageEvents,
      status_events: timeline.statusEvents,
      unresolved_status_events: timeline.unresolvedStatusEvents,
      all_status_and_unresolved_rows_preserved: timeline.relationshipStatusRows ===
        timeline.statusEvents + timeline.unresolvedStatusEvents,
      remote_cast_rows_synthesized: timeline.remoteCastRowsSynthesized,
    },
    summary: {
      effect_frontier_count: effects.length,
      exact_pack_observed_effect_count: exactObserved.length,
      exact_pack_status_lifecycle_ready_effect_count: statusReady.length,
      exact_pack_provider_recipient_ready_effect_count: providerReady.length,
      current_build_prior_pack_observed_effect_count: priorObserved.length,
      missing_exact_pack_lifecycle_effect_count: missing.length,
      missing_exact_pack_effect_ids: missing.map((entry) => entry.effect_id),
      selected_exact_pack_effect_ids: statusReady.map((entry) => entry.effect_id),
      provider_recipient_selected_effect_ids: providerReady.map((entry) => entry.effect_id),
      provider_rdps_credit_authorized_effect_count: authorized.length,
      provider_rdps_credit_authorized_effect_ids: authorized.map((entry) => entry.effect_id),
    },
    effects,
    smallest_safe_next_slice: {
      effect_id: 3_003_052,
      reason: "strongest exact-current-pack lifecycle and formula-stage candidate still missing a controlled server boundary",
      closed_now: [
        "exact provider, recipient, build, lifecycle, and +200-basis-point primary-stat magnitude are versioned",
        "class-11 floor(primary * 58 / 100) and the selected Attack coefficient stage replay exactly",
        "the current runtime explicitly disables the superseded proportional transfer",
      ],
      remaining: [
        "capture stationary class-11 ability-2352 hit-1 and hit-3 absent-active-absent transitions",
        "hold every non-Harmony source and target formula input identical with at least two repeats per state",
        "select the exact downstream operation order and integer boundary from at least two discriminating signatures",
        "only then enable an exact action-and-hit-scoped rule with conservation and negative tests",
      ],
    },
    conclusion: {
      suite_status: "blocked",
      observed_event_count: timeline.statusEvents + timeline.unresolvedStatusEvents,
      exact_party_conservation: true,
      exact_pack_timeline_topology_proven: true,
      all_observed_exact_pack_status_rows_partitioned_and_preserved: true,
      every_support_effect_lifecycle_replayed_under_exact_pack: missing.length === 0,
      status_lifecycle_replay_proven: false,
      provider_recipient_replay_proven: false,
      formula_authority_proven: false,
      partial_formula_authority_proven: authorized.length > 0,
      provider_rdps_credit_allowed: authorized.length > 0,
      partial_runtime_promotion_allowed: authorized.length > 0,
      production_promotion_count: authorized.length,
      runtime_promotion_allowed: false,
    },
  };
  output.content_sha256 = contentHash(output);
  verify(output);
  const outputPath = path.resolve(required(values, "output"));
  writeFileSync(outputPath, `${JSON.stringify(output, null, 2)}\n`, { flag: "wx" });
  console.log(
    `wrote ${outputPath}: ${statusReady.length}/${effects.length} exact-pack lifecycle slices, ` +
    `${providerReady.length} provider-recipient slices, suites remain blocked`,
  );
}

function addFormulaRuntime(value, add) {
  const fields = [
    ["team_luck", "effect_id", "primary"],
    ["functional_amp", "effect_id", "primary"],
    ["functional_amp", "self_multiplier_effect_id", "auxiliary"],
    ["functional_amp", "passive_damage_effect_id", "auxiliary"],
    ["functional_amp", "passive_stack_effect_id", "auxiliary"],
    ["mechanical_power", "effect_id", "primary"],
    ["harmony_grace", "effect_id", "primary"],
    ["harmony_grace", "source_terminal_effect_id", "terminal"],
    ["thunderwind", "effect_id", "primary"],
    ["thunderwind", "child_effect_id", "child"],
    ["inspiration", "effect_id", "primary"],
    ["inspiration", "full_bloom_effect_id", "condition"],
    ["highland_blood", "effect_id", "primary"],
    ["highland_blood", "provider_marker_effect_id", "provider-marker"],
    ["highland_blood", "companion_lockout_effect_id", "condition"],
  ];
  for (const [family, key, role] of fields) {
    add(value[family]?.[key], "rdps-formula-runtime", family, role);
  }
}

function validateInputs(values) {
  const { build, formula, classification, externalState, vulnerability, psychoscope,
    partyAudit, ownership, mechanical, mechanicalPromotion, harmony, harmonyPromotion } = values;
  assert.equal(formula.value.schema_version, 9);
  assert.equal(formula.value.game_build, build);
  assert.equal(formula.value.protocol_pack_digest, CURRENT_PACK);
  assert.equal(formula.value.policy?.runtime_promotion_allowed, false);
  assert.equal(classification.value.game_build, build);
  assert.equal(externalState.value.game_build, build);
  assert.equal(vulnerability.value.game_build, build);
  assert.equal(psychoscope.value.game_build, build);
  assert.equal(psychoscope.value.runtime_rules_enabled, false);
  assert.equal(partyAudit.value.schema_version, 3);
  assert.equal(partyAudit.value.game_build, build);
  assert.equal(partyAudit.value.policy?.provider_rdps_credit_authorized, false);
  assert.equal(partyAudit.value.summary?.party_status_events, 14_760);
  assert.equal(ownership.value.schema_version, 5);
  assert.equal(ownership.value.game_build, build);
  assert.equal(ownership.value.summary?.selected_status_events, 14_760);
  assert.equal(ownership.value.policy?.provider_rdps_credit_allowed, false);
  assert.equal(mechanical.value.generated_by, "tools/bpsr-mechanical-power-current-pack-equivalence.mjs");
  assert.equal(mechanical.value.identity?.game_build, build);
  assert.equal(mechanical.value.identity?.effect_id, 2_110_140);
  assert.equal(mechanical.value.authority?.exact_current_pack_lifecycle_replay_proven, true);
  assert.equal(mechanical.value.authority?.provider_rdps_credit_allowed, false);
  assert.equal(mechanicalPromotion.value.generated_by,
    "tools/bpsr-mechanical-power-exact-rational-promotion-proof.mjs");
  assert.equal(mechanicalPromotion.value.schema_version, 3);
  assert.equal(mechanicalPromotion.value.game_build, build);
  assert.equal(mechanicalPromotion.value.protocol_pack_digest, CURRENT_PACK);
  assert.equal(mechanicalPromotion.value.effect_id, EFFECT_MECHANICAL_POWER);
  assert.equal(mechanicalPromotion.value.decision?.component_promotion_allowed, false);
  assert.equal(mechanicalPromotion.value.decision?.production_promotion_count_delta, 0);
  assert.equal(harmony.value.generated_by, "tools/bpsr-harmony-grace-current-pack-lifecycle-closure.mjs");
  assert.equal(harmony.value.game_build, build);
  assert.equal(harmony.value.effect_id, 3_003_052);
  assert.equal(harmony.value.identity?.protocol_pack_digest, CURRENT_PACK);
  assert.equal(harmony.value.proof?.ordinary_damage_conserved, true);
  assert.equal(harmony.value.policy?.provider_rdps_credit_allowed, false);
  assert.equal(harmonyPromotion.value.generated_by,
    "tools/bpsr-harmony-exact-rational-promotion-proof.mjs");
  assert.equal(harmonyPromotion.value.schema_version, 3);
  assert.equal(harmonyPromotion.value.game_build, build);
  assert.equal(harmonyPromotion.value.protocol_pack_digest, CURRENT_PACK);
  assert.equal(harmonyPromotion.value.effect_id, EFFECT_HARMONY_GRACE);
  assert.equal(harmonyPromotion.value.decision?.component_promotion_allowed, false);
  assert.equal(harmonyPromotion.value.decision?.production_promotion_count_delta, 0);
}

function validateCatalog(entry, build) {
  const value = entry.value;
  assert.equal(value.schema_version, 1);
  assert.equal(value.game_build, build);
  assert.equal(value.policy?.runtime_promotion_allowed, false);
  const root = path.resolve(path.dirname(entry.path));
  const results = [];
  for (const item of value.entries ?? []) {
    const absolute = path.join(root, item.path);
    const bytes = readFileSync(absolute);
    assert.equal(bytes.length, item.bytes, `catalog bytes ${item.path}`);
    assert.equal(sha256(bytes), item.sha256, `catalog hash ${item.path}`);
    if (item.identity_state !== "current-build" || item.kind !== "json") continue;
    const document = JSON.parse(bytes.toString("utf8"));
    assert.equal(document.game_build, build, `catalog build ${item.path}`);
    results.push({
      effect_id: Number(document.rule?.effect_id),
      review_state: String(document.rule?.review_state),
      path: item.path,
      sha256: item.sha256,
    });
  }
  return results.sort((a, b) => a.effect_id - b.effect_id);
}

async function scanTimeline(filePath, build, expectedDigest) {
  const result = {
    manifest: null,
    runCount: 0,
    canonicalEvents: 0,
    damageEvents: 0,
    statusEvents: 0,
    unresolvedStatusEvents: 0,
    relationshipStatusRows: 0,
    remoteCastRowsSynthesized: 0,
    protocolPackDigest: expectedDigest,
    statusEffectCounts: new Map(),
  };
  const input = createReadStream(filePath, { encoding: "utf8" });
  const lines = readline.createInterface({ input, crlfDelay: Infinity });
  for await (const line of lines) {
    if (!line) continue;
    const row = JSON.parse(line);
    if (row.row_type === "manifest") {
      assert.equal(row.schema_version, 10);
      assert.equal(row.topology?.effect_edge,
        "provider -> effect/status lifecycle -> recipient or enemy target");
      assert.equal(row.topology?.damage_edge,
        "recipient damage action -> recipient or enemy target");
      assert.equal(row.policy?.unknown_effects_are_preserved, true);
      assert.equal(row.policy?.remote_player_cast_packets_required, false);
      result.manifest = row;
    } else if (row.row_type === "run_header") {
      assert.equal(row.client_build, build);
      assert.equal(row.protocol_pack_digest, expectedDigest);
      result.runCount += 1;
    } else if (row.row_type === "run_summary") {
      assert.equal(row.client_build, build);
      assert.equal(row.protocol_pack_digest, expectedDigest);
      assert.equal(row.remote_cast_rows_synthesized, 0);
      result.canonicalEvents += Number(row.canonical_events);
      result.damageEvents += Number(row.event_counts?.damage ?? 0);
      result.statusEvents += Number(row.event_counts?.status ?? 0);
      result.unresolvedStatusEvents += Number(row.event_counts?.unresolved_status ?? 0);
      result.remoteCastRowsSynthesized += Number(row.remote_cast_rows_synthesized);
      for (const [id, count] of Object.entries(row.status_effect_counts ?? {})) {
        result.statusEffectCounts.set(Number(id), (result.statusEffectCounts.get(Number(id)) ?? 0) + Number(count));
      }
    } else if (row.row_type === "relationship" &&
      (row.event_kind === "status" || row.event_kind === "unresolved_status")) {
      result.relationshipStatusRows += 1;
      assert.equal(
        row.relationship_shape,
        row.event_kind === "status"
          ? "provider-to-effect-lifecycle-to-recipient-or-enemy-target"
          : "provider-to-unresolved-effect-lifecycle-to-recipient-or-enemy-target",
      );
    }
  }
  assert.ok(result.manifest, "timeline manifest missing");
  assert.equal(result.runCount, result.manifest.rlog_count);
  assert.equal(result.relationshipStatusRows, result.statusEvents + result.unresolvedStatusEvents);
  return result;
}

function validateCorrelation(value, timeline, timelinePath, build) {
  assert.equal(value.schema_version, 4);
  assert.equal(value.generated_by, "tools/bpsr-lifecycle-action-correlation-ledger.mjs");
  assert.equal(value.game_build, build);
  assert.equal(value.summary?.run_count, timeline.runCount);
  assert.equal(value.summary?.lifecycle_transition_count, timeline.statusEvents);
  assert.equal(value.summary?.event_kind_counts?.find((entry) => entry.key === "unresolved_status")?.count,
    timeline.unresolvedStatusEvents);
  assert.equal(value.summary?.damage_action_count, timeline.damageEvents);
  assert.equal(value.policy?.provider_rdps_credit_allowed, false);
  assert.equal(value.conclusion?.closed_lifecycle_canonical_conservation_proven, false);
  const input = value.inputs?.support_timeline;
  assert.equal(path.resolve(input?.path), timelinePath);
  const bytes = readFileSync(timelinePath);
  assert.equal(bytes.length, input.bytes);
  assert.equal(sha256(bytes), input.sha256);
}

function verify(report) {
  assert.equal(report.schema_version, SCHEMA_VERSION);
  assert.equal(report.generated_by, GENERATED_BY);
  assert.equal(report.game_build, "24687926");
  assert.equal(report.policy?.unknown_and_unresolved_statuses_preserved, true);
  assert.equal(report.policy?.remote_player_cast_packets_required, false);
  assert.equal(report.policy?.provider_rdps_credit_allowed, false);
  assert.equal(report.policy?.runtime_ui_promotion_allowed, false);
  assert.equal(report.policy?.global_runtime_promotion_allowed, false);
  assert.equal(report.topology?.effect_edge,
    "provider -> effect/status lifecycle -> recipient or enemy target");
  assert.equal(report.topology?.damage_edge,
    "recipient damage action -> recipient or enemy target");
  assert.equal(report.exact_pack_replay?.protocol_pack_digest, CURRENT_PACK);
  assert.equal(report.exact_pack_replay?.all_status_and_unresolved_rows_preserved, true);
  assert.equal(report.exact_pack_replay?.remote_cast_rows_synthesized, 0);
  assert.equal(report.summary?.effect_frontier_count, report.effects?.length);
  assert.ok(report.summary?.missing_exact_pack_lifecycle_effect_count > 0);
  assert.equal(report.summary?.provider_rdps_credit_authorized_effect_count, 0);
  assert.deepEqual(report.summary?.provider_rdps_credit_authorized_effect_ids, []);
  assert.deepEqual(
    report.effects.filter((entry) => entry.provider_rdps_credit_allowed)
      .map((entry) => entry.effect_id),
    [],
  );
  for (const entry of Object.values(report.sources ?? {})) verifyReceipt(entry);
  assert.equal(report.conclusion?.suite_status, "blocked");
  assert.equal(report.conclusion?.status_lifecycle_replay_proven, false);
  assert.equal(report.conclusion?.provider_recipient_replay_proven, false);
  assert.equal(report.conclusion?.partial_formula_authority_proven, false);
  assert.equal(report.conclusion?.provider_rdps_credit_allowed, false);
  assert.equal(report.conclusion?.partial_runtime_promotion_allowed, false);
  assert.equal(report.conclusion?.production_promotion_count, 0);
  assert.equal(report.conclusion?.runtime_promotion_allowed, false);
  assert.equal(report.content_sha256, contentHash(withoutContentHash(report)));
}

function verifyReceipt(entry) {
  const bytes = readFileSync(path.resolve(entry.path));
  assert.equal(bytes.length, entry.bytes, `source byte count changed: ${entry.path}`);
  assert.equal(sha256(bytes), entry.sha256, `source hash changed: ${entry.path}`);
}

function selfTest() {
  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game_build: "24687926",
    policy: {
      unknown_and_unresolved_statuses_preserved: true,
      remote_player_cast_packets_required: false,
      provider_rdps_credit_allowed: false,
      runtime_ui_promotion_allowed: false,
      global_runtime_promotion_allowed: false,
    },
    topology: {
      effect_edge: "provider -> effect/status lifecycle -> recipient or enemy target",
      damage_edge: "recipient damage action -> recipient or enemy target",
    },
    exact_pack_replay: {
      protocol_pack_digest: CURRENT_PACK,
      all_status_and_unresolved_rows_preserved: true,
      remote_cast_rows_synthesized: 0,
    },
    summary: {
      effect_frontier_count: 2,
      missing_exact_pack_lifecycle_effect_count: 1,
      provider_rdps_credit_authorized_effect_count: 0,
      provider_rdps_credit_authorized_effect_ids: [],
    },
    effects: [
      { effect_id: EFFECT_MECHANICAL_POWER, provider_rdps_credit_allowed: false },
      { effect_id: EFFECT_HARMONY_GRACE, provider_rdps_credit_allowed: false },
    ],
    conclusion: {
      suite_status: "blocked",
      status_lifecycle_replay_proven: false,
      provider_recipient_replay_proven: false,
      partial_formula_authority_proven: false,
      provider_rdps_credit_allowed: false,
      partial_runtime_promotion_allowed: false,
      production_promotion_count: 0,
      runtime_promotion_allowed: false,
    },
  };
  report.content_sha256 = contentHash(report);
  verify(report);
  const unsafe = structuredClone(report);
  unsafe.conclusion.production_promotion_count = 1;
  unsafe.content_sha256 = contentHash(unsafe);
  assert.throws(() => verify(unsafe));
  console.log("bpsr-rdps-status-lifecycle-coverage self-test passed");
}

function byEffect(entries = []) {
  return new Map(entries.map((entry) => [Number(entry.effect_id), entry]));
}

function source(filePath) {
  const absolute = path.resolve(filePath);
  const bytes = readFileSync(absolute);
  return { path: absolute, bytes: bytes.length, sha256: sha256(bytes), value: JSON.parse(bytes) };
}

function receipt(entry) {
  return { path: entry.path, bytes: entry.bytes, sha256: entry.sha256 };
}

async function fileReceipt(filePath) {
  return { path: filePath, bytes: statSync(filePath).size, sha256: await sha256File(filePath) };
}

async function sha256File(filePath) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(filePath)) hash.update(chunk);
  return hash.digest("hex");
}

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

function contentHash(value) {
  return sha256(JSON.stringify(canonicalize(withoutContentHash(value))));
}

function withoutContentHash(value) {
  const copy = structuredClone(value);
  delete copy.content_sha256;
  return copy;
}

function canonicalize(value) {
  if (Array.isArray(value)) return value.map(canonicalize);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, canonicalize(value[key])]));
  }
  return value;
}

function readJson(filePath) {
  return JSON.parse(readFileSync(path.resolve(filePath), "utf8"));
}

function parseOptions(values) {
  const parsed = {};
  for (let index = 0; index < values.length; index += 2) {
    const flag = values[index];
    const value = values[index + 1];
    assert.ok(flag?.startsWith("--") && value, `invalid option near ${flag ?? "end"}`);
    parsed[flag.slice(2)] = value;
  }
  return parsed;
}

function required(values, name) {
  const value = values[name];
  assert.ok(value, `--${name} is required`);
  return value;
}

function usage(code) {
  console.log(
    "Usage:\n" +
    "  node tools/bpsr-rdps-status-lifecycle-coverage.mjs generate --build <id> " +
    "--formula-runtime <json> --effect-classification <json> --external-state-runtime <json> " +
    "--target-vulnerability-runtime <json> --psychoscope-runtime <json> --catalog-manifest <json> " +
    "--party-window-audit <json> --provider-ownership <json> --exact-pack-timeline <jsonl> " +
    "--correlation-summary <json> --mechanical-power-proof <json> " +
    "--mechanical-promotion-proof <json> --harmony-grace-proof <json> " +
    "--harmony-promotion-proof <json> " +
    "--output <json>\n" +
    "  node tools/bpsr-rdps-status-lifecycle-coverage.mjs verify --input <json>\n" +
    "  node tools/bpsr-rdps-status-lifecycle-coverage.mjs self-test",
  );
  process.exit(code);
}
