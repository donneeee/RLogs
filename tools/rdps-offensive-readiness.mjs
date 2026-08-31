#!/usr/bin/env node

import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const options = parseArgs(process.argv.slice(2));
const ledgerPath = resolvePath(options.ledger);
const compatibilityPath = resolvePath(options.compatibility);
const gapPath = resolvePath(options.formulaGaps);
const proofGatePath = resolvePath(options.proofGates);
const staticValueProofPath = resolvePath(options.staticValueProof);
const outputPath = resolvePath(options.output);

const ledger = readJson(ledgerPath, "recipient-scope ledger");
const compatibility = readJson(compatibilityPath, "runtime compatibility report");
const formulaGaps = readJson(gapPath, "formula-gap ledger");
const proofGateConfig = readJson(proofGatePath, "offensive proof-gate config");
const staticValueProof = readJson(staticValueProofPath, "static value proof");

validateInputs(ledger, compatibility, formulaGaps, proofGateConfig, staticValueProof);

const proofGatesByRule = new Map(
  proofGateConfig.candidates.map((candidate) => [candidate.source_rule_id, candidate]),
);

const runtimeRules = compatibility.rules.map((rule) => ({
  rule_id: rule.rule_id,
  status: rule.status,
  runtime_behavior: rule.runtime_behavior,
  identity_ids: rule.exact_identity_references
    .filter((reference) => isRuntimeMechanicIdentity(reference.path))
    .map((reference) => reference.value)
    .filter(unique),
}));
const runtimeRuleByIdentity = new Map();
for (const rule of runtimeRules) {
  for (const identity of rule.identity_ids) {
    const key = String(identity);
    const rules = runtimeRuleByIdentity.get(key) || [];
    rules.push(rule);
    runtimeRuleByIdentity.set(key, rules);
  }
}

const formulaGapByRule = new Map(
  formulaGaps.candidates.map((candidate) => [candidate.source_rule_id, candidate]),
);
const candidates = [];
const nonOffensiveSupport = [];
const nonRdpsMechanics = [];
const staticProofByRule = new Map(
  staticValueProof.sources.map((source) => [source.source_rule_id, source]),
);

for (const source of ledger.candidates) {
  const staticProof = staticProofByRule.get(source.source_rule_id) || null;
  if (staticProof && staticProof.disposition !== "external-rdps-candidate") {
    nonRdpsMechanics.push({
      source_rule_id: source.source_rule_id,
      source_id: source.source_id,
      source_name: source.source_name,
      effect_ids: source.effect_ids || [],
      disposition: staticProof.disposition,
      static_value_proof: staticProof,
    });
    continue;
  }
  const sourceIdentities = [...(source.effect_ids || []), ...(source.runtime_related_effect_ids || [])]
    .filter(unique);
  const matchedRuntimeRules = sourceIdentities
    .flatMap((identity) => runtimeRuleByIdentity.get(String(identity)) || [])
    .filter((rule, index, values) => values.findIndex((other) => other.rule_id === rule.rule_id) === index);

  const routes = (source.component_scope_routes || []).filter((route) =>
    isExternalGate(route.transfer_gate?.kind),
  );
  const sourceExternal = isExternalGate(source.transfer_gate?.kind);
  const externalRoutes = routes.length > 0
    ? routes
    : sourceExternal
      ? [sourceLevelRoute(source)]
      : [];
  if (externalRoutes.length === 0 && matchedRuntimeRules.length === 0) continue;

  const offensiveRoutes = externalRoutes
    .filter(isOffensiveRoute)
    .filter((route) => !rejectedComponents(staticProof).has(route.component_key));
  const defensiveRoutes = externalRoutes.filter((route) => !isOffensiveRoute(route));
  if (defensiveRoutes.length > 0) {
    nonOffensiveSupport.push(compactRouteSource(source, defensiveRoutes));
  }
  if (offensiveRoutes.length === 0 && matchedRuntimeRules.length === 0) continue;

  const historical = source.historical_packet_evidence || {};
  const formulaGap = formulaGapByRule.get(source.source_rule_id);
  const configuredProofGate = proofGatesByRule.get(source.source_rule_id);
  const state = matchedRuntimeRules.length > 0
    ? "runtime-active"
    : (historical.authoritative_status_events || 0) > 0
      ? "packet-observed-formula-proof-needed"
      : "packet-occurrence-and-formula-proof-needed";
  candidates.push({
    source_rule_id: source.source_rule_id,
    source_id: source.source_id,
    source_name: source.source_name,
    state,
    effect_ids: source.effect_ids || [],
    runtime_rule_ids: matchedRuntimeRules.map((rule) => rule.rule_id),
    runtime_statuses: matchedRuntimeRules.map((rule) => rule.status).filter(unique),
    formula_gap_outcome: formulaGap?.outcome || null,
    static_value_proof: staticProof,
    proof_gates: applyStaticMagnitudeProof(
      configuredProofGate
        ? compactProofGates(configuredProofGate, "configured")
        : deriveProofGates(source, formulaGap, state),
      staticProof,
    ),
    historical_packet_evidence: {
      authoritative_status_events: historical.authoritative_status_events || 0,
      opened_windows: historical.opened_windows || 0,
      cross_actor_windows: historical.cross_actor_windows || 0,
      player_recipient_windows: historical.player_recipient_windows || 0,
      monster_recipient_windows: historical.monster_recipient_windows || 0,
      provider_is_recipient_observed: historical.provider_is_recipient_observed || false,
      provider_differs_from_recipient_observed: historical.provider_differs_from_recipient_observed || false,
    },
    offensive_routes: offensiveRoutes.map(compactRoute),
    remaining_requirement: state === "runtime-active"
      ? "none for display; retain provisional build warning until dependencies are re-audited or exact-build proof replaces it"
      : source.remaining_requirement,
  });
}

const representedRuntimeRules = new Set(candidates.flatMap((candidate) => candidate.runtime_rule_ids));
for (const rule of runtimeRules) {
  if (representedRuntimeRules.has(rule.rule_id)) continue;
  candidates.push({
    source_rule_id: null,
    source_id: `runtime:${rule.rule_id}`,
    source_name: rule.rule_id,
    state: "runtime-active",
    effect_ids: rule.identity_ids,
    runtime_rule_ids: [rule.rule_id],
    runtime_statuses: [rule.status],
    formula_gap_outcome: null,
    historical_packet_evidence: null,
    offensive_routes: [],
    remaining_requirement:
      "none for display; retain provisional build warning until dependencies are re-audited or exact-build proof replaces it",
  });
}

candidates.sort((left, right) =>
  stateOrder(left.state) - stateOrder(right.state)
  || left.source_name.localeCompare(right.source_name)
  || String(left.source_rule_id).localeCompare(String(right.source_rule_id)),
);

const active = candidates.filter((candidate) => candidate.state === "runtime-active");
const packetObserved = candidates.filter((candidate) =>
  candidate.state === "packet-observed-formula-proof-needed",
);
const occurrenceNeeded = candidates.filter((candidate) =>
  candidate.state === "packet-occurrence-and-formula-proof-needed",
);

const result = {
  schema_version: 1,
  generated_by: "tools/rdps-offensive-readiness.mjs",
  game: "blue-protocol-star-resonance",
  deployment_id: compatibility.deployment_id,
  static_game_build: String(ledger.static_game_build),
  runtime_authored_build: String(compatibility.authored_build),
  runtime_observed_build: String(compatibility.observed_build),
  policy: {
    queue_contains_offensive_external_attribution_only: true,
    self_only_formula_context_is_not_an_rdps_transfer_blocker: true,
    defensive_support_is_reported_separately: true,
    same_deployment_runtime_rules_continue_provisionally: true,
    packet_observed_unproven_rules_are_never_silently_enabled: true,
    unresolved_evidence_hidden: false,
  },
  inputs: {
    recipient_scope_ledger: relativePath(ledgerPath),
    runtime_compatibility: relativePath(compatibilityPath),
    formula_gap_ledger: relativePath(gapPath),
    offensive_proof_gates: relativePath(proofGatePath),
    static_value_proof: relativePath(staticValueProofPath),
  },
  summary: {
    runtime_active_formula_families: new Set(active.flatMap((candidate) => candidate.runtime_rule_ids)).size,
    offensive_external_sources: candidates.length,
    offensive_external_routes: candidates.reduce(
      (sum, candidate) => sum + candidate.offensive_routes.length,
      0,
    ),
    packet_observed_sources_needing_formula_proof: packetObserved.length,
    sources_needing_packet_occurrence_and_formula_proof: occurrenceNeeded.length,
    offensive_sources_remaining: packetObserved.length + occurrenceNeeded.length,
    packet_observed_failed_proof_gates: packetObserved.reduce(
      (sum, candidate) => sum + candidate.proof_gates.failed_gate_count,
      0,
    ),
    unresolved_sources_without_proof_gates: candidates.filter(
      (candidate) => candidate.state !== "runtime-active" && !candidate.proof_gates,
    ).length,
    non_offensive_external_support_sources: nonOffensiveSupport.length,
    proven_non_rdps_mechanics_preserved: nonRdpsMechanics.length,
    self_only_sources_excluded_from_rdps_blocker_count:
      ledger.summary?.scope_queues?.["self-only-formula-context-no-transfer"] || 0,
  },
  candidates,
  non_offensive_support: nonOffensiveSupport,
  non_rdps_mechanics: nonRdpsMechanics,
};

writeFileSync(outputPath, `${JSON.stringify(result, null, 2)}\n`, "utf8");
console.log(JSON.stringify(result.summary));

function compactRouteSource(source, routes) {
  return {
    source_rule_id: source.source_rule_id,
    source_id: source.source_id,
    source_name: source.source_name,
    effect_ids: source.effect_ids || [],
    routes: routes.map(compactRoute),
  };
}

function compactProofGates(candidate, origin) {
  const gates = Object.fromEntries(
    Object.entries(candidate.gates).map(([name, gate]) => [name, {
      status: gate.status,
      evidence: gate.evidence || [],
      detail: gate.detail,
    }]),
  );
  return {
    origin,
    evidence_build: String(candidate.evidence_build),
    compatibility_behavior: candidate.compatibility_behavior,
    failed_gate_count: Object.values(gates).filter((gate) => gate.status === "needed").length,
    gates,
  };
}

function applyStaticMagnitudeProof(proofGates, staticProof) {
  if (!staticProof || !["exact-formula", "complete-ladder"].includes(staticProof.static_value_status)) {
    return proofGates;
  }
  proofGates.gates.magnitude_formula = {
    status: "proven",
    evidence: staticProof.evidence || [],
    detail: staticProof.static_value_status === "complete-ladder"
      ? "The complete current-build value ladder is proven; runtime selection remains a separate gate."
      : "The exact current-build formula is proven.",
  };
  proofGates.failed_gate_count = Object.values(proofGates.gates)
    .filter((gate) => gate.status === "needed").length;
  return proofGates;
}

function rejectedComponents(staticProof) {
  return new Set([
    ...(staticProof?.rejected_components || []),
    ...(staticProof?.rejected_component ? [staticProof.rejected_component] : []),
  ]);
}

function deriveProofGates(source, formulaGap, state) {
  if (state === "runtime-active") {
    return {
      origin: "runtime-rule",
      evidence_build: null,
      compatibility_behavior: "runtime rule remains active under the compatibility report",
      failed_gate_count: 0,
      gates: {},
    };
  }

  const historical = source.historical_packet_evidence || {};
  const packetObserved = (historical.authoritative_status_events || 0) > 0;
  const lifecycleObserved = (historical.opened_windows || 0) > 0;
  const externalProviderObserved = historical.resolved_external_player_provider_observed === true
    || historical.provider_differs_from_recipient_observed === true;
  const formulaProven = formulaGap?.current_build_promotion_eligible === true;
  const gates = {
    packet_occurrence: derivedGate(
      packetObserved ? "proven-historical" : "needed",
      packetObserved
        ? `${historical.authoritative_status_events} authoritative status events occur in the retained packet inventory.`
        : "No occurrence exists in the retained packet inventory; exact identity remains on the final validation watchlist.",
    ),
    provider_recipient_identity: derivedGate(
      externalProviderObserved ? "proven-historical" : "needed",
      externalProviderObserved
        ? "A resolved external player provider and distinct recipient occur in the retained provider audit."
        : "No resolved provider-to-distinct-recipient edge has been proven for transferred credit.",
    ),
    lifecycle: derivedGate(
      lifecycleObserved ? "proven-historical" : "needed",
      lifecycleObserved
        ? `${historical.opened_windows} effect windows were reconstructed from retained lifecycle events.`
        : "Apply, refresh, stack, consume, and remove boundaries are not yet reconstructed as an attributable window.",
    ),
    magnitude_formula: derivedGate(
      formulaProven ? "proven" : "needed",
      formulaProven
        ? "The matching-build formula-gap ledger marks this source eligible for promotion."
        : "Exact matching-build value selection, formula placement, predicates, and overlap arbitration are not all proven.",
      formulaGap ? [relativePath(gapPath)] : [],
    ),
    counterfactual_replay: derivedGate(
      "needed",
      "No source-specific observed-versus-counterfactual recipient damage replay is promoted.",
    ),
    party_conservation: derivedGate(
      "needed",
      "No source-specific replay proves that credited provider damage is exactly removed from recipients and conserves party damage.",
    ),
  };
  return {
    origin: "systematic-derived",
    evidence_build: packetObserved ? String(ledger.historical_packet_build || "historical") : null,
    compatibility_behavior:
      "keep the source visible and unresolved; never enable transferred credit until every gate is proven",
    failed_gate_count: Object.values(gates).filter((gate) => gate.status === "needed").length,
    gates,
  };
}

function derivedGate(status, detail, evidence = []) {
  return { status, evidence, detail };
}

function compactRoute(route) {
  return {
    component_key: route.component_key,
    effect_class: route.effect_class,
    direction: route.direction,
    contribution_scope: route.contribution_scope,
    rdps_relevance: route.rdps_relevance,
    transfer_gate_kind: route.transfer_gate?.kind,
    attribution_route: route.transfer_gate?.attribution_route,
    required_current_build_evidence: route.transfer_gate?.required_current_build_evidence || [],
  };
}

function sourceLevelRoute(source) {
  return {
    component_key: "<source-level>",
    effect_class: null,
    direction: source.transfer_gate?.kind === "external-target-state-counterfactual"
      ? "target-mitigation"
      : "external-recipient",
    contribution_scope: null,
    rdps_relevance: "direct-damage-formula-component",
    transfer_gate: source.transfer_gate,
  };
}

function isExternalGate(kind) {
  return kind === "external-recipient-counterfactual"
    || kind === "external-target-state-counterfactual";
}

function isOffensiveRoute(route) {
  return !["defense", "damage-dealt-reduction"].includes(route.direction);
}

function isRuntimeMechanicIdentity(referencePath) {
  const leaf = referencePath.split(".").at(-1)?.replace(/\[\d+\]$/, "");
  return [
    "effect_id",
    "child_effect_id",
    "full_bloom_effect_id",
    "source_config_id",
    "child_source_config_id",
    "full_bloom_source_config_id",
  ].includes(leaf);
}

function stateOrder(state) {
  return {
    "packet-observed-formula-proof-needed": 0,
    "packet-occurrence-and-formula-proof-needed": 1,
    "runtime-active": 2,
  }[state] ?? 9;
}

function unique(value, index, values) {
  return values.indexOf(value) === index;
}

function validateInputs(ledgerValue, compatibilityValue, gapsValue, proofGateValue, staticProofValue) {
  if (!Array.isArray(ledgerValue.candidates) || !Array.isArray(compatibilityValue.rules)) {
    throw new Error("readiness inputs do not contain candidate and runtime rule arrays");
  }
  if (String(ledgerValue.static_game_build) !== String(compatibilityValue.observed_build)
    || String(gapsValue.static_game_build) !== String(compatibilityValue.observed_build)) {
    throw new Error("readiness inputs do not describe the same current game build");
  }
  if (!Array.isArray(proofGateValue.candidates)) {
    throw new Error("offensive proof-gate config does not contain a candidates array");
  }
  if (!Array.isArray(staticProofValue.sources)
    || String(staticProofValue.static_game_build) !== String(compatibilityValue.observed_build)) {
    throw new Error("static value proof is missing or describes a different current game build");
  }
  const validStatuses = new Set(["proven", "proven-historical", "needed"]);
  const requiredGates = [
    "packet_occurrence",
    "provider_recipient_identity",
    "lifecycle",
    "magnitude_formula",
    "counterfactual_replay",
    "party_conservation",
  ];
  for (const candidate of proofGateValue.candidates) {
    if (!candidate.source_rule_id || !candidate.evidence_build || !candidate.gates) {
      throw new Error("each proof-gate candidate needs source_rule_id, evidence_build, and gates");
    }
    for (const gate of requiredGates) {
      if (!validStatuses.has(candidate.gates[gate]?.status)) {
        throw new Error(`proof gate ${candidate.source_rule_id}.${gate} has an invalid or missing status`);
      }
    }
  }
}

function parseArgs(args) {
  const result = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index];
    const value = args[index + 1];
    if (!key?.startsWith("--") || !value) throw new Error(`invalid argument near ${key || "end"}`);
    result[key.slice(2)] = value;
  }
  for (const key of ["ledger", "compatibility", "formulaGaps", "proofGates", "staticValueProof", "output"]) {
    if (!result[key]) throw new Error(`--${key} is required`);
  }
  return result;
}

function readJson(filePath, label) {
  try {
    return JSON.parse(readFileSync(filePath, "utf8"));
  } catch (error) {
    throw new Error(`could not read ${label} at ${filePath}: ${error.message}`);
  }
}

function resolvePath(filePath) {
  return path.isAbsolute(filePath) ? filePath : path.resolve(repoRoot, filePath);
}

function relativePath(filePath) {
  return path.relative(repoRoot, filePath).replaceAll("\\", "/");
}
