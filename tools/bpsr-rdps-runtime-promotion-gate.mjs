#!/usr/bin/env node

import { createHash } from "node:crypto";
import {
  existsSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, "..");
const RDPS_RUNTIME_SCHEMA_VERSION = 36;
const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "generate") generate(options);
else if (command === "verify") verifyFile(resolvePath(required(options, "input")), true);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function generate(args) {
  const build = required(args, "build");
  if (!/^\d+$/.test(build)) throw new Error("Build must contain only ASCII digits");
  const files = {
    preflight: resolvePath(required(args, "preflight")),
    protocol_status: resolvePath(required(args, "protocol-status")),
    proof_closure: resolvePath(required(args, "proof-closure")),
    conservation_boundary: resolvePath(required(args, "conservation-boundary")),
    local_observable_frontier: resolvePath(required(args, "local-observable-frontier")),
    runtime_config: resolvePath(required(args, "runtime-config")),
    runtime_overrides: resolvePath(required(args, "runtime-overrides")),
  };
  const outputFile = resolvePath(required(args, "output"));
  const audit = buildAudit(build, files);
  mkdirSync(path.dirname(outputFile), { recursive: true });
  writeFileSync(outputFile, `${JSON.stringify(audit, null, 2)}\n`, "utf8");
  verifyFile(outputFile, true);
  console.log(
    `rDPS runtime promotion gate for build ${build}: ${audit.decision}; `
      + `${audit.blockers.length} blockers; remote-player packet acquisition required=false.`,
  );
}

function normalizeGapWindowAudit(source, requireWindows = true) {
  assert(source?.status === "exact-gap-bounded-lifecycles-found-counterfactual-unproven",
    "gap-bounded lifecycle audit has an unsupported status");
  assert(Number(source?.source_rlog_count) === 26, "gap-bounded lifecycle RLOG count changed");
  assert(Number(source?.canonical_event_count) === 6411565,
    "gap-bounded lifecycle canonical event count changed");
  assert(Number(source?.data_gap_count) === 16181, "gap-bounded lifecycle data-gap count changed");
  assert(Number(source?.rlogs_with_data_gaps) === 26,
    "gap-bounded lifecycle RLOG data-gap coverage changed");
  assert(Number(source?.complete_gap_bounded_lifecycle_count) === 39,
    "gap-bounded complete lifecycle count changed");
  assert(Number(source?.complete_windows_with_damage_count) === 39,
    "gap-bounded damage-window count changed");
  assert(Number(source?.damage_events_while_active) === 2277,
    "gap-bounded active damage-event count changed");
  assert(Number(source?.lifecycles_cut_by_data_quality_boundary) === 51,
    "data-quality-cut lifecycle count changed");
  if (requireWindows) {
    assert(Array.isArray(source?.complete_gap_bounded_windows)
      && source.complete_gap_bounded_windows.length === 39
      && source.complete_gap_bounded_windows.every((window) =>
        window?.gap_bounded === true
        && Number(window?.damage_events_while_active) > 0
        && window?.controlled_counterfactual_pair_proven === false
        && window?.formula_authority === false),
    "gap-bounded lifecycle windows lost their fail-closed contract");
  } else {
    assert(Number(source?.controlled_counterfactual_pairs) === 0,
      "gap-bounded lifecycle summary gained a controlled counterfactual pair");
  }
  assert(source?.exact_damage_projection_proven === false
    && source?.exact_operation_order_proven === false
    && source?.exact_integer_rounding_proven === false
    && source?.packet_conservation_proven === false
    && source?.formula_authority === false
    && source?.runtime_authority === false
    && source?.ui_display_authority === false
    && source?.provider_rdps_credit_allowed === false,
  "gap-bounded lifecycle evidence gained unsupported authority");
  return {
    status: source.status,
    source_rlog_count: 26,
    canonical_event_count: 6411565,
    data_gap_count: 16181,
    rlogs_with_data_gaps: 26,
    complete_gap_bounded_lifecycle_count: 39,
    complete_windows_with_damage_count: 39,
    damage_events_while_active: 2277,
    lifecycles_cut_by_data_quality_boundary: 51,
    controlled_counterfactual_pairs: 0,
    exact_damage_projection_proven: false,
    exact_operation_order_proven: false,
    exact_integer_rounding_proven: false,
    packet_conservation_proven: false,
    formula_authority: false,
    runtime_authority: false,
    ui_display_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function buildAudit(build, files) {
  const values = Object.fromEntries(
    Object.entries(files).map(([key, file]) => {
      requireFile(file, key.replaceAll("_", " "));
      return [key, readJson(file, key.replaceAll("_", " "))];
    }),
  );
  const {
    preflight,
    protocol_status: protocol,
    proof_closure: closure,
    conservation_boundary: conservation,
  } = values;
  const frontier = values.local_observable_frontier;
  const runtime = values.runtime_config;
  const overrides = values.runtime_overrides;

  for (const [label, value] of Object.entries({ preflight, protocol, closure, conservation, frontier, runtime })) {
    assert(String(value.game_build ?? "") === build, `${label} build does not match ${build}`);
  }
  assert(preflight.deployment === "global", "preflight deployment must be global");
  assert(protocol.status === "blocked" || protocol.status === "promoted", "unsupported protocol status");
  assert(runtime.deployment_id === "global", "runtime deployment must be global");
  assert(overrides.deployment_id === "global", "runtime overrides deployment must be global");
  assert(runtime.schema_version === RDPS_RUNTIME_SCHEMA_VERSION,
    `runtime config must use schema ${RDPS_RUNTIME_SCHEMA_VERSION}`);
  assert(overrides.schema_version === RDPS_RUNTIME_SCHEMA_VERSION,
    `runtime overrides must use schema ${RDPS_RUNTIME_SCHEMA_VERSION}`);
  assert(Number(closure.schema_version) >= 30,
    "proof closure predates the party-support formula frontier");
  assert(Number(conservation.schema_version) === 1
    && conservation.generated_by === "tools/bpsr-current-pack-conservation-boundary.mjs",
  "unsupported current-pack conservation boundary");
  assert(
    runtime.policy?.same_deployment_build_mismatch === "exact-build-only",
    "runtime config must reject same-deployment build fallback",
  );
  const exactBuildOverrides = Array.isArray(overrides.builds)
    ? overrides.builds.filter((entry) =>
      String(entry.game_build ?? "") === build
        && entry.protocol_pack_digest === runtime.protocol_pack_digest)
    : [];
  assert(exactBuildOverrides.length <= 1, "runtime overrides duplicate the exact base build");
  assert(
    exactBuildOverrides.every((entry) =>
      isFailClosedExactBuildOverride(entry, build, runtime.protocol_pack_digest)),
    "an exact-base-build runtime override expands authority or changes a non-authority value",
  );

  const plannedInputs = Array.isArray(preflight.inputs) ? preflight.inputs : [];
  const missingRequired = plannedInputs.filter((entry) => entry.required === true && entry.status !== "present");
  const missingRequiredPaths = missingRequired.map((entry) => String(entry.path ?? "")).sort();
  const missingProofSuites = unique(missingRequired.flatMap((entry) => entry.proof_suites ?? [])).sort();
  assert(
    Number(preflight.summary?.planned_inputs ?? -1) === plannedInputs.length,
    "preflight planned-input count does not conserve",
  );
  assert(
    Number(preflight.summary?.missing_required_inputs ?? -1) === missingRequired.length,
    "preflight missing-required count does not conserve",
  );
  assert(
    sameStrings(missingProofSuites, preflight.required_proof_suites_from_missing_inputs ?? []),
    "preflight missing-input proof suites do not conserve",
  );

  const expectedPackPath = `plugins/games/blue-protocol-star-resonance/protocol-packs/global/steam-${build}/pack.json`;
  const protocolPackMissing = missingRequiredPaths.includes(expectedPackPath);
  assert(
    String(protocol.promoted_pack?.expected_path ?? "") === expectedPackPath,
    "protocol status expected pack path does not match the exact build",
  );
  const protocolEventCoverageProven = protocol.status === "promoted"
    && protocol.promoted_pack?.present === true
    && protocol.promoted_pack?.build_matches === true
    && protocol.promoted_pack?.byte_identical_to_candidate === true
    && protocol.audit?.promotion_ready === true
    && Number(protocol.audit?.capture_gap_count) === 0
    && Number(protocol.audit?.observable_migrated_decoder_route_count) > 0
    && Number(protocol.audit?.validated_observable_migrated_decoder_route_count)
      === Number(protocol.audit?.observable_migrated_decoder_route_count)
    && conservation.conclusion?.protocol_event_coverage_proven === true;
  const canonicalReplayConservationProven =
    conservation.conclusion?.exact_pack_closed_lifecycle_canonical_replay_conservation_proven === true
    && conservation.conclusion?.formula_specific_counterfactual_conservation_proven === true;
  const requiredProofSuites = unique([
    ...missingProofSuites,
    ...(closure.production_readiness?.required_proof_suites ?? []),
  ]).filter((suite) =>
    !(suite === "protocol-event-coverage" && protocolEventCoverageProven)
      && !(suite === "canonical-replay-conservation" && canonicalReplayConservationProven)
  ).sort();
  const remotePolicyExact =
    frontier.policy?.structurally_unobservable_remote_player_packets_are_not_formula_acquisition_requirements === true
    && frontier.counterfactual_discriminants?.acquisition_contract?.remote_player_packet_dependency === false;
  assert(remotePolicyExact, "local-observable frontier lost the structural remote-player packet boundary");
  assert(Number(frontier.schema_version) >= 18,
    "local-observable frontier predates the gap-bounded lifecycle audit");
  assert(
    frontier.policy
      ?.complete_gap_bounded_lifecycle_windows_do_not_make_counterfactual_formula_authority === true,
    "local-observable frontier lost the gap-bounded lifecycle authority boundary",
  );
  const gapWindowAudit = normalizeGapWindowAudit(frontier.rlog_gap_window_audit);
  const protocolStructuralNonObligations = protocol.schema_version >= 2
    ? normalizeStructuralRoutes(protocol.audit?.structural_non_obligation_routes ?? [])
    : [];
  const closureStructuralNonObligations = normalizeStructuralRoutes(
    closure.production_readiness?.structural_remote_packet_non_obligations ?? [],
  );
  const exactStructuralBoundary = protocol.schema_version >= 2
    ? protocol.policy
      ?.structurally_unobservable_remote_player_packets_are_not_acquisition_requirements === true
      && protocol.policy?.structural_non_obligations_are_not_packet_absence_as_zero === true
      && protocol.policy?.structural_non_obligations_never_synthesize_canonical_events === true
      && closure.production_readiness
        ?.structural_remote_packet_non_obligations_excluded_from_acquisition === true
      && closure.production_readiness?.protocol_event_coverage_scope ===
        "locally-observable-exact-routes"
      && stableJson(protocolStructuralNonObligations) ===
        stableJson(closureStructuralNonObligations)
    : closureStructuralNonObligations.length === 0;
  assert(exactStructuralBoundary, "protocol and closure structural non-obligations disagree");

  const checks = {
    preflight_ready_for_snapshot: preflight.ready_for_snapshot === true,
    preflight_runtime_promotion_allowed: preflight.runtime_promotion_allowed === true,
    exact_build_protocol_pack_present: protocol.promoted_pack?.present === true && !protocolPackMissing,
    exact_build_protocol_pack_identity_matches:
      protocol.promoted_pack?.build_matches === true
      && protocol.promoted_pack?.byte_identical_to_candidate === true,
    protocol_pack_promoted: protocol.status === "promoted",
    protocol_event_coverage_proven: protocolEventCoverageProven,
    exact_pack_closed_lifecycle_canonical_replay_conservation_proven:
      canonicalReplayConservationProven,
    strict_rdps_proof_complete: closure.production_readiness?.strict_rdps_proof_complete === true,
    closure_runtime_promotion_allowed: closure.production_readiness?.runtime_promotion_allowed === true,
    closure_production_runtime_ready: closure.production_readiness?.production_runtime_ready === true,
    party_support_formula_frontier_complete:
      closure.production_readiness?.party_skill_static_frontier_complete === true,
    local_observable_formula_authority: frontier.summary?.formula_authority === true,
    local_observable_runtime_authority: frontier.summary?.runtime_authority === true,
    local_observable_provider_credit_allowed: frontier.summary?.provider_rdps_credit_allowed === true,
    local_observable_exact_damage_projection: frontier.summary?.exact_damage_projection_proven === true,
    local_observable_packet_conservation: frontier.summary?.packet_conservation_proven === true,
    exact_base_build_overrides_only_narrow_authority: exactBuildOverrides.every((entry) =>
      isFailClosedExactBuildOverride(entry, build, runtime.protocol_pack_digest)),
    structural_remote_player_packets_excluded_from_acquisition:
      remotePolicyExact && exactStructuralBoundary,
  };
  const promotionChecks = Object.entries(checks)
    .filter(([key]) => ![
      "exact_base_build_overrides_only_narrow_authority",
      "structural_remote_player_packets_excluded_from_acquisition",
    ].includes(key))
    .map(([, passed]) => passed);
  const evidenceAllowsPromotion = promotionChecks.every(Boolean);
  const runtimeAllowsPromotion = runtime.policy?.runtime_promotion_allowed === true;
  assert(
    runtime.policy?.party_support_formula_frontier_complete ===
      checks.party_support_formula_frontier_complete,
    "runtime party-support formula policy disagrees with the proof closure",
  );
  assert(
    runtime.promotion_blockers?.includes("party-support-formula-frontier") ===
      !checks.party_support_formula_frontier_complete,
    "runtime party-support formula blocker disagrees with the proof closure",
  );
  assert(
    runtimeAllowsPromotion === evidenceAllowsPromotion,
    "runtime promotion policy disagrees with the current-build evidence gates",
  );
  assert(
    runtime.promotion_state
      === (runtimeAllowsPromotion ? "approved" : "blocked-current-build-proof-gates-open"),
    "runtime promotion state disagrees with runtime_promotion_allowed",
  );

  const blockers = [];
  if (!checks.exact_build_protocol_pack_present) {
    blockers.push(`exact-build protocol-pack identity is missing for global steam build ${build}`);
  }
  for (const suite of requiredProofSuites) {
    blockers.push(`${suite} proof suite is incomplete`);
  }
  if (!checks.strict_rdps_proof_complete) blockers.push("strict rDPS proof closure is incomplete");
  if (!checks.party_support_formula_frontier_complete) {
    blockers.push("party-skill and team-entry formula frontier is incomplete");
  }
  if (!checks.local_observable_exact_damage_projection) {
    blockers.push(`effect ${frontier.effect_id} exact damage projection is unproven`);
  }
  if (!checks.local_observable_packet_conservation) {
    blockers.push(`effect ${frontier.effect_id} packet conservation is unproven`);
  }
  assert(
    blockers.every((blocker) => !/remote[- ]player packet/i.test(blocker)),
    "structurally unobservable remote-player packets must not become acquisition blockers",
  );
  if (evidenceAllowsPromotion) assert(blockers.length === 0, "promotion-ready evidence retained blockers");
  else assert(blockers.length > 0, "blocked evidence must retain blockers");

  const audit = {
    schema_version: 5,
    generated_by: "tools/bpsr-rdps-runtime-promotion-gate.mjs",
    generated_at: new Date().toISOString(),
    game_build: build,
    deployment_id: "global",
    policy: {
      exact_build_identity_required: true,
      candidate_or_static_formula_evidence_never_grants_runtime_authority: true,
      structurally_unobservable_remote_player_packets_are_not_acquisition_requirements: true,
      structural_non_obligations_are_exact_route_bound_and_stale_if_observed: true,
      unknown_and_unresolved_canonical_events_are_retained: true,
      ordinary_damage_must_be_conserved: true,
      current_character_snapshots_must_not_be_substituted_into_older_runs: true,
      complete_gap_bounded_lifecycle_windows_never_grant_formula_or_runtime_authority: true,
      party_skill_and_team_entry_formula_frontier_must_be_complete: true,
      protocol_event_coverage_and_canonical_conservation_are_separate_gates: true,
      exact_pack_segment_ordinary_damage_conservation_is_not_closed_lifecycle_formula_conservation: true,
      exact_base_build_overrides_may_only_narrow_runtime_authority: true,
    },
    inputs: Object.fromEntries(
      Object.entries(files).map(([key, file]) => [key, fileIdentity(file)]),
    ),
    required_proof_suites: requiredProofSuites,
    preflight: {
      planned_inputs: plannedInputs.length,
      present_required_inputs: Number(preflight.summary?.present_required_inputs ?? 0),
      missing_required_inputs: missingRequired.length,
      missing_required_paths: missingRequiredPaths,
      ready_for_snapshot: preflight.ready_for_snapshot === true,
      runtime_promotion_allowed: preflight.runtime_promotion_allowed === true,
    },
    checks,
    structural_non_obligations: [
      "remote-player packet families that the local client never receives",
    ],
    structural_non_obligation_routes: protocolStructuralNonObligations,
    local_observable_gap_window_evidence: gapWindowAudit,
    collectable_proof_obligations: [
      ...requiredProofSuites.map((suite) => `${suite} using locally observable exact-build evidence`),
      `controlled local-observable counterfactual projection for effect ${frontier.effect_id}`,
    ],
    blockers,
    decision: evidenceAllowsPromotion ? "runtime-promotion-allowed" : "runtime-promotion-blocked",
    runtime_promotion_allowed: runtimeAllowsPromotion,
    ui_rdps_display_allowed: runtimeAllowsPromotion,
    saved_history_rdps_replay_allowed: runtimeAllowsPromotion,
  };
  audit.content_sha256 = hashJson(audit);
  verifyAudit(audit, false);
  return audit;
}

function verifyFile(file, verifyInputs) {
  const audit = readJson(file, "runtime promotion audit");
  verifyAudit(audit, verifyInputs);
  console.log(`rDPS runtime promotion audit verified for build ${audit.game_build}: ${audit.decision}.`);
}

function verifyAudit(audit, verifyInputs) {
  assert(
    [1, 2, 3, 4, 5].includes(audit.schema_version)
      && audit.generated_by === "tools/bpsr-rdps-runtime-promotion-gate.mjs",
    "unsupported runtime promotion audit schema or generator",
  );
  assert(/^\d+$/.test(String(audit.game_build ?? "")), "runtime promotion audit lacks a valid build");
  assert(audit.deployment_id === "global", "runtime promotion audit deployment must be global");
  assert(
    audit.policy?.structurally_unobservable_remote_player_packets_are_not_acquisition_requirements === true,
    "runtime promotion audit lost the remote-player packet policy",
  );
  assert(
    audit.policy?.structural_non_obligations_are_exact_route_bound_and_stale_if_observed === true,
    "runtime promotion audit lost the exact-route structural non-obligation policy",
  );
  assert(
    audit.checks?.structural_remote_player_packets_excluded_from_acquisition === true,
    "runtime promotion audit requires structurally unavailable remote-player packets",
  );
  if (audit.schema_version >= 2) {
    assert(
      audit.policy
        ?.complete_gap_bounded_lifecycle_windows_never_grant_formula_or_runtime_authority === true,
      "runtime promotion audit lost the gap-bounded lifecycle authority boundary",
    );
    normalizeGapWindowAudit(audit.local_observable_gap_window_evidence, false);
  }
  if (audit.schema_version >= 3) {
    assert(
      audit.policy?.party_skill_and_team_entry_formula_frontier_must_be_complete === true,
      "runtime promotion audit lost the party-support formula frontier gate",
    );
    assert(
      audit.checks?.party_support_formula_frontier_complete ===
        !audit.blockers.includes("party-skill and team-entry formula frontier is incomplete"),
      "runtime promotion audit party-support frontier blocker is inconsistent",
    );
  }
  if (audit.schema_version >= 4) {
    assert(
      audit.policy?.protocol_event_coverage_and_canonical_conservation_are_separate_gates === true
        && audit.policy?.exact_pack_segment_ordinary_damage_conservation_is_not_closed_lifecycle_formula_conservation === true,
      "runtime promotion audit merged protocol coverage with formula conservation",
    );
    assert(
      audit.checks?.protocol_event_coverage_proven ===
        !audit.required_proof_suites.includes("protocol-event-coverage"),
      "runtime promotion audit protocol-event coverage suite is inconsistent",
    );
    assert(
      audit.checks?.exact_pack_closed_lifecycle_canonical_replay_conservation_proven ===
        !audit.required_proof_suites.includes("canonical-replay-conservation"),
      "runtime promotion audit canonical conservation suite is inconsistent",
    );
  }
  if (audit.schema_version >= 5) {
    assert(
      audit.policy?.exact_base_build_overrides_may_only_narrow_runtime_authority === true
        && audit.checks?.exact_base_build_overrides_only_narrow_authority === true,
      "runtime promotion audit allows an exact-build override to expand authority",
    );
  }
  assert(Array.isArray(audit.blockers), "runtime promotion audit blockers must be an array");
  assert(Array.isArray(audit.structural_non_obligation_routes),
    "runtime promotion audit structural routes must be an array");
  for (const route of audit.structural_non_obligation_routes) {
    assert(
      Number.isSafeInteger(Number(route.service_id)) && Number(route.service_id) > 0
        && Number.isSafeInteger(Number(route.method_id)) && Number(route.method_id) > 0
        && Number(route.packet_count) === 0 && Number(route.decoded_records) === 0
        && route.promotion_requirement_satisfied === true && String(route.reason ?? ""),
      "runtime promotion audit contains an unsafe structural route",
    );
  }
  assert(
    audit.blockers.every((blocker) => !/remote[- ]player packet/i.test(String(blocker))),
    "runtime promotion blockers contain a structural remote-player packet non-obligation",
  );
  const allowed = audit.runtime_promotion_allowed === true;
  assert(audit.ui_rdps_display_allowed === allowed, "UI gate disagrees with runtime gate");
  assert(audit.saved_history_rdps_replay_allowed === allowed, "history gate disagrees with runtime gate");
  assert(
    audit.decision === (allowed ? "runtime-promotion-allowed" : "runtime-promotion-blocked"),
    "runtime promotion decision disagrees with its boolean gate",
  );
  assert(allowed || audit.blockers.length > 0, "blocked runtime promotion must retain blockers");
  const { content_sha256: recordedHash, ...withoutHash } = audit;
  assert(recordedHash === hashJson(withoutHash), "runtime promotion audit content hash is invalid");
  if (verifyInputs) {
    for (const [label, input] of Object.entries(audit.inputs ?? {})) {
      const file = resolvePath(input.path);
      requireFile(file, label.replaceAll("_", " "));
      assert(statSync(file).size === input.bytes, `${label} byte length changed`);
      assert(sha256(file) === input.sha256, `${label} content hash changed`);
    }
  }
}

function selfTest() {
  const safeExactOverride = {
    game_build: "123",
    protocol_pack_digest: "sha256:test",
    patch: {
      game_build: "123",
      protocol_pack_digest: "sha256:test",
      mechanic: { runtime_transfer_enabled: false },
    },
  };
  assert(
    isFailClosedExactBuildOverride(safeExactOverride, "123", "sha256:test"),
    "self-test rejected a fail-closed exact-build override",
  );
  const unsafeExactOverride = structuredClone(safeExactOverride);
  unsafeExactOverride.patch.mechanic.runtime_transfer_enabled = true;
  assert(
    !isFailClosedExactBuildOverride(unsafeExactOverride, "123", "sha256:test"),
    "self-test accepted an authority-expanding exact-build override",
  );
  const root = mkdtempSync(path.join(tmpdir(), "rlogs-rdps-runtime-gate-"));
  try {
    const blocked = fixture(root, "blocked", false);
    const blockedAudit = buildAudit("123", blocked);
    assert(blockedAudit.decision === "runtime-promotion-blocked", "blocked fixture was promoted");
    assert(blockedAudit.blockers.length === 7, "blocked fixture lost proof blockers");
    assert(
      blockedAudit.blockers.every((blocker) => !/remote[- ]player packet/i.test(blocker)),
      "blocked fixture converted a structural non-obligation into a blocker",
    );
    const promoted = fixture(root, "promoted", true);
    const promotedAudit = buildAudit("123", promoted);
    assert(promotedAudit.decision === "runtime-promotion-allowed", "complete fixture was blocked");
    assert(promotedAudit.blockers.length === 0, "complete fixture retained blockers");
    console.log("rDPS runtime promotion gate self-test passed.");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

function fixture(root, name, allowed) {
  const fixtureRoot = path.join(root, name);
  mkdirSync(fixtureRoot, { recursive: true });
  const packPath = "plugins/games/blue-protocol-star-resonance/protocol-packs/global/steam-123/pack.json";
  const files = {
    preflight: path.join(fixtureRoot, "preflight.json"),
    protocol_status: path.join(fixtureRoot, "protocol.json"),
    proof_closure: path.join(fixtureRoot, "closure.json"),
    conservation_boundary: path.join(fixtureRoot, "conservation.json"),
    local_observable_frontier: path.join(fixtureRoot, "frontier.json"),
    runtime_config: path.join(fixtureRoot, "runtime.json"),
    runtime_overrides: path.join(fixtureRoot, "overrides.json"),
  };
  writeJson(files.preflight, {
    game_build: "123",
    deployment: "global",
    summary: {
      planned_inputs: 1,
      present_required_inputs: allowed ? 1 : 0,
      missing_required_inputs: allowed ? 0 : 1,
    },
    inputs: [{ required: true, status: allowed ? "present" : "missing", path: packPath, proof_suites: [
      "canonical-replay-conservation", "protocol-event-coverage",
    ] }],
    required_proof_suites_from_missing_inputs: allowed
      ? []
      : ["canonical-replay-conservation", "protocol-event-coverage"],
    ready_for_snapshot: allowed,
    runtime_promotion_allowed: allowed,
  });
  writeJson(files.protocol_status, {
    schema_version: 1,
    game_build: "123",
    status: allowed ? "promoted" : "blocked",
    promoted_pack: {
      expected_path: packPath,
      present: allowed,
      build_matches: allowed,
      byte_identical_to_candidate: allowed,
    },
    audit: {
      promotion_ready: allowed,
      capture_gap_count: allowed ? 0 : 1,
      observable_migrated_decoder_route_count: 1,
      validated_observable_migrated_decoder_route_count: allowed ? 1 : 0,
    },
  });
  writeJson(files.proof_closure, {
    schema_version: 30,
    game_build: "123",
    production_readiness: {
      required_proof_suites: allowed ? [] : ["canonical-replay-conservation", "protocol-event-coverage"],
      strict_rdps_proof_complete: allowed,
      runtime_promotion_allowed: allowed,
      production_runtime_ready: allowed,
      party_skill_static_frontier_complete: allowed,
    },
  });
  writeJson(files.conservation_boundary, {
    schema_version: 1,
    generated_by: "tools/bpsr-current-pack-conservation-boundary.mjs",
    game_build: "123",
    conclusion: {
      protocol_event_coverage_proven: allowed,
      exact_pack_closed_lifecycle_canonical_replay_conservation_proven: allowed,
      formula_specific_counterfactual_conservation_proven: allowed,
    },
  });
  writeJson(files.local_observable_frontier, {
    schema_version: 18,
    game_build: "123",
    effect_id: 2110092,
    policy: {
      structurally_unobservable_remote_player_packets_are_not_formula_acquisition_requirements: true,
      complete_gap_bounded_lifecycle_windows_do_not_make_counterfactual_formula_authority: true,
    },
    counterfactual_discriminants: { acquisition_contract: { remote_player_packet_dependency: false } },
    rlog_gap_window_audit: {
      status: "exact-gap-bounded-lifecycles-found-counterfactual-unproven",
      source_rlog_count: 26,
      canonical_event_count: 6411565,
      data_gap_count: 16181,
      rlogs_with_data_gaps: 26,
      complete_gap_bounded_lifecycle_count: 39,
      complete_windows_with_damage_count: 39,
      damage_events_while_active: 2277,
      lifecycles_cut_by_data_quality_boundary: 51,
      complete_gap_bounded_windows: Array.from({ length: 39 }, (_, index) => ({
        instance_id: index + 1,
        damage_events_while_active: 1,
        gap_bounded: true,
        controlled_counterfactual_pair_proven: false,
        formula_authority: false,
      })),
      exact_damage_projection_proven: false,
      exact_operation_order_proven: false,
      exact_integer_rounding_proven: false,
      packet_conservation_proven: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    summary: {
      formula_authority: allowed,
      runtime_authority: allowed,
      provider_rdps_credit_allowed: allowed,
      exact_damage_projection_proven: allowed,
      packet_conservation_proven: allowed,
    },
  });
  writeJson(files.runtime_config, {
    schema_version: RDPS_RUNTIME_SCHEMA_VERSION,
    deployment_id: "global",
    game_build: "123",
    promotion_state: allowed ? "approved" : "blocked-current-build-proof-gates-open",
    promotion_blockers: allowed ? [] : ["party-support-formula-frontier"],
    policy: {
      runtime_promotion_allowed: allowed,
      party_support_formula_frontier_complete: allowed,
      same_deployment_build_mismatch: "exact-build-only",
    },
  });
  writeJson(files.runtime_overrides, {
    schema_version: RDPS_RUNTIME_SCHEMA_VERSION,
    deployment_id: "global",
    builds: [],
  });
  return files;
}

function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 1) {
    const token = args[index];
    if (!token.startsWith("--")) throw new Error(`Unexpected argument: ${token}`);
    const key = token.slice(2);
    const value = args[index + 1];
    if (!value || value.startsWith("--")) throw new Error(`Missing value for --${key}`);
    parsed[key] = value;
    index += 1;
  }
  return parsed;
}

function required(args, key) {
  if (!args[key]) throw new Error(`Missing required --${key}`);
  return args[key];
}

function resolvePath(value) {
  return path.isAbsolute(value) ? value : path.resolve(repoRoot, value);
}

function relativePath(file) {
  return path.relative(repoRoot, file).replaceAll(path.sep, "/");
}

function requireFile(file, label) {
  if (!existsSync(file) || !statSync(file).isFile()) throw new Error(`Missing ${label}: ${file}`);
}

function readJson(file, label) {
  try {
    return JSON.parse(readFileSync(file, "utf8"));
  } catch (error) {
    throw new Error(`Invalid ${label} JSON at ${file}: ${error.message}`);
  }
}

function writeJson(file, value) {
  writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`, "utf8");
}

function fileIdentity(file) {
  return { path: relativePath(file), bytes: statSync(file).size, sha256: sha256(file) };
}

function sha256(file) {
  return `sha256:${createHash("sha256").update(readFileSync(file)).digest("hex")}`;
}

function hashJson(value) {
  return `sha256:${createHash("sha256").update(JSON.stringify(value)).digest("hex")}`;
}

function normalizeStructuralRoutes(routes) {
  return routes.map((route) => ({
    direction: String(route.direction ?? ""),
    fragment: String(route.fragment ?? ""),
    service_id: Number(route.service_id),
    method_id: Number(route.method_id),
    packet_count: Number(route.packet_count),
    decoded_records: Number(route.decoded_records),
    promotion_requirement_satisfied: route.promotion_requirement_satisfied === true,
    reason: String(route.reason ?? ""),
  })).sort((left, right) =>
    left.direction.localeCompare(right.direction) ||
    left.fragment.localeCompare(right.fragment) ||
    left.service_id - right.service_id || left.method_id - right.method_id
  );
}

function stableJson(value) {
  return JSON.stringify(value);
}

function unique(values) {
  return [...new Set(values.map(String))];
}

function isFailClosedExactBuildOverride(entry, build, protocolPackDigest) {
  if (String(entry?.game_build ?? "") !== build
    || entry?.protocol_pack_digest !== protocolPackDigest
    || entry?.patch === null
    || typeof entry?.patch !== "object"
    || Array.isArray(entry.patch)
    || String(entry.patch.game_build ?? "") !== build
    || entry.patch.protocol_pack_digest !== protocolPackDigest) {
    return false;
  }

  const visit = (value, pathParts) => {
    const key = pathParts.at(-1) ?? "";
    if (pathParts.length === 1 && key === "game_build") return String(value) === build;
    if (pathParts.length === 1 && key === "protocol_pack_digest") {
      return value === protocolPackDigest;
    }
    if (Array.isArray(value)) return value.length === 0;
    if (value !== null && typeof value === "object") {
      return Object.entries(value).every(([childKey, childValue]) =>
        visit(childValue, [...pathParts, childKey]));
    }
    return value === false && /(?:authority|enabled|allowed)$/.test(key);
  };

  return Object.entries(entry.patch).every(([key, value]) => visit(value, [key]));
}

function sameStrings(left, right) {
  return JSON.stringify(unique(left).sort()) === JSON.stringify(unique(right).sort());
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function usage(exitCode) {
  console.log(`Usage:
  node tools/bpsr-rdps-runtime-promotion-gate.mjs generate --build <id> \\
    --preflight <json> --protocol-status <json> --proof-closure <json> \\
    --conservation-boundary <json> \\
    --local-observable-frontier <json> --runtime-config <json> \\
    --runtime-overrides <json> --output <json>
  node tools/bpsr-rdps-runtime-promotion-gate.mjs verify --input <json>
  node tools/bpsr-rdps-runtime-promotion-gate.mjs self-test`);
  process.exit(exitCode);
}
