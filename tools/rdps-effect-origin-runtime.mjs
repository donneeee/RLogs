#!/usr/bin/env node

import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const options = parseArgs(process.argv.slice(2));
const inputPath = resolvePath(options.input ||
  "plugins/games/blue-protocol-star-resonance/research/game-file-inventory/global/steam-24687926/rdps-observed-effect-reconciliation.v1.json");
const outputPath = resolvePath(options.output ||
  "plugins/games/blue-protocol-star-resonance/game-data/runtime/rdps-effect-origin-runtime.v1.json");
const reconciliation = readJson(inputPath);

if (reconciliation.schema_version !== 1 || !Array.isArray(reconciliation.effects)) {
  throw new Error("unsupported observed-effect reconciliation schema");
}

const effectsById = {};
let originFingerprints = 0;
let exactOriginEndpoints = 0;
let exactOriginOwners = 0;
let effectsWithObservedOrigins = 0;
let formulaEndpoints = 0;
const endpointResolutionCounts = { exact: 0, ambiguous: 0, unresolved: 0 };
const ownerResolutionCounts = { exact: 0, ambiguous: 0, unresolved: 0 };
const fallbackEndpointResolutionCounts = { exact: 0, ambiguous: 0, unresolved: 0 };
const fallbackOwnerResolutionCounts = { exact: 0, ambiguous: 0, unresolved: 0 };
const transferProofCounts = {};

for (const effect of [...reconciliation.effects].sort((a, b) => a.effect_id - b.effect_id)) {
  const source = effect.source_resolution || {};
  const candidatesById = new Map((source.candidates || []).map((candidate) => [
    candidate.source_id,
    reviewedCandidate(effect.effect_id, compactCandidate(candidate)),
  ]));
  const originsByKey = {};
  const routes = source.evidence_routes?.packet_configured_origins || [];
  if (routes.length > 0) effectsWithObservedOrigins += 1;
  const formulaEndpointPresent = reviewedFormulaEndpointPresent(effect);
  if (formulaEndpointPresent) formulaEndpoints += 1;

  for (const route of [...routes].sort((a, b) =>
    a.source_type_id - b.source_type_id || a.source_config_id - b.source_config_id)) {
    const key = `${route.source_type_id}:${route.source_config_id}`;
    if (originsByKey[key]) throw new Error(`duplicate origin fingerprint ${effect.effect_id}:${key}`);
    const candidateIds = [...new Set(route.candidate_source_ids || [])].sort();
    const rawCandidates = candidateIds.map((id) => candidatesById.get(id) || unknownCandidate(id));
    const candidateSources = collapseSemanticCandidates(rawCandidates);
    const aliasesCollapsedToOne = rawCandidates.length > 1 && candidateSources.length === 1;
    const endpointResolution = promoteSemanticAliasResolution(
      normalizeResolution(route.endpoint_resolution_state),
      aliasesCollapsedToOne,
    );
    const ownerResolution = promoteSemanticAliasResolution(
      normalizeResolution(route.owning_source_resolution_state),
      aliasesCollapsedToOne,
    );
    originsByKey[key] = {
      endpoint_resolution: endpointResolution,
      owner_resolution: ownerResolution,
      candidate_sources: candidateSources,
      unresolved_terminal_ids: [...new Set(route.unresolved_terminal_ids || [])].sort((a, b) => a - b),
    };
    originFingerprints += 1;
    endpointResolutionCounts[originsByKey[key].endpoint_resolution] += 1;
    ownerResolutionCounts[originsByKey[key].owner_resolution] += 1;
    if (originsByKey[key].endpoint_resolution === "exact") exactOriginEndpoints += 1;
    if (originsByKey[key].owner_resolution === "exact") exactOriginOwners += 1;
  }

  const expectedOrigins = effect.packet_origins || [];
  for (const origin of expectedOrigins) {
    const key = `${origin.source_type_id}:${origin.source_config_id}`;
    if (!originsByKey[key]) {
      throw new Error(`effect ${effect.effect_id} lost observed origin ${key}`);
    }
  }

  const fallbackIds = [...new Set(source.candidate_source_ids || [])].sort();
  const rawFallbackCandidates = fallbackIds.map((id) => candidatesById.get(id) || unknownCandidate(id));
  const fallbackCandidates = collapseSemanticCandidates(rawFallbackCandidates);
  const fallbackAliasesCollapsedToOne = rawFallbackCandidates.length > 1 && fallbackCandidates.length === 1;
  const fallbackEndpointResolution = source.exact_unique_source || fallbackAliasesCollapsedToOne ? "exact"
    : fallbackIds.length > 0 ? "ambiguous" : "unresolved";
  const fallbackOwnerResolution = source.exact_owning_source || fallbackAliasesCollapsedToOne ? "exact"
    : fallbackIds.length > 0 ? "ambiguous" : "unresolved";
  fallbackEndpointResolutionCounts[fallbackEndpointResolution] += 1;
  fallbackOwnerResolutionCounts[fallbackOwnerResolution] += 1;
  const transferProofState = reviewedTransferProofState(effect);
  transferProofCounts[transferProofState] = (transferProofCounts[transferProofState] || 0) + 1;
  effectsById[String(effect.effect_id)] = {
    display_name: effect.display_name || null,
    formula_endpoint_present: formulaEndpointPresent,
    transfer_proof_state: transferProofState,
    fallback: {
      endpoint_resolution: fallbackEndpointResolution,
      owner_resolution: fallbackOwnerResolution,
      candidate_sources: fallbackCandidates,
      unresolved_terminal_ids: [],
    },
    origins_by_key: originsByKey,
  };
}

const expectedEffectCount = reconciliation.effects.length;
const expectedObservedOrigins = reconciliation.effects.reduce(
  (total, effect) => total + (effect.packet_origins || []).length,
  0,
);
if (Object.keys(effectsById).length !== expectedEffectCount) {
  throw new Error(`effect fingerprint catalog lost ${expectedEffectCount - Object.keys(effectsById).length} effects`);
}
if (originFingerprints !== expectedObservedOrigins) {
  throw new Error(`effect fingerprint catalog retained ${originFingerprints}/${expectedObservedOrigins} packet origins`);
}

const result = {
  schema_version: 1,
  game: reconciliation.game,
  game_build: String(reconciliation.game_build),
  generated_by: "tools/rdps-effect-origin-runtime.mjs",
  policy: {
    terminal_effect_is_runtime_fingerprint: true,
    packet_origin_precedes_effect_only_fallback: true,
    endpoint_and_equipped_owner_certainty_are_separate: true,
    ambiguous_and_unresolved_evidence_is_preserved: true,
  },
  summary: {
    effects: Object.keys(effectsById).length,
    formula_endpoints: formulaEndpoints,
    effects_without_formula_endpoint: expectedEffectCount - formulaEndpoints,
    effects_with_observed_origins: effectsWithObservedOrigins,
    origin_fingerprints: originFingerprints,
    exact_origin_endpoints: exactOriginEndpoints,
    exact_origin_owners: exactOriginOwners,
    endpoint_resolution_counts: endpointResolutionCounts,
    owner_resolution_counts: ownerResolutionCounts,
    fallback_endpoint_resolution_counts: fallbackEndpointResolutionCounts,
    fallback_owner_resolution_counts: fallbackOwnerResolutionCounts,
    transfer_proof_counts: sortedRecord(transferProofCounts),
    zero_omission_audit: {
      expected_effects: expectedEffectCount,
      retained_effects: Object.keys(effectsById).length,
      expected_packet_origins: expectedObservedOrigins,
      retained_packet_origins: originFingerprints,
      complete: true,
    },
  },
  effects_by_id: effectsById,
};

writeFileSync(outputPath, `${JSON.stringify(result, null, 2)}\n`);
console.log(JSON.stringify({ output: outputPath, ...result.summary }, null, 2));

function compactCandidate(candidate) {
  return {
    source_id: candidate.source_id,
    source_kind: candidate.source_kind || "unknown",
    source_name: candidate.source_name || null,
    source_entity_id: Number.isSafeInteger(candidate.source_entity_id)
      ? candidate.source_entity_id : null,
    equipment_suit_selector: compactEquipmentSuitSelector(candidate.equipment_suit_selector),
    dreamscope_selector: compactDreamscopeSelector(candidate.dreamscope_selector),
  };
}

function reviewedCandidate(effectId, candidate) {
  // Build 24687926 packet source type 1 is EFightSourceBuff. Effect 2202113 is
  // the 30-second internal Overhealing cooldown marker emitted by BuffTable row
  // 21423 (Symbiotic Mark), not an active-skill endpoint of its own.
  if (effectId === 2_202_113 && candidate.source_id === "buff-source:21423") {
    return {
      ...candidate,
      source_kind: "buff",
      source_name: "Symbiotic Mark",
      source_entity_id: 21_423,
    };
  }
  return candidate;
}

function reviewedFormulaEndpointPresent(effect) {
  if (effect.effect_id === 2_202_113) return false;
  return effect.formula_endpoint !== null;
}

function reviewedTransferProofState(effect) {
  if (effect.effect_id === 2_202_113) return "non_damage_internal_marker";
  return effect.transfer_proof?.state || "unresolved";
}

function unknownCandidate(sourceId) {
  return {
    source_id: sourceId,
    source_kind: "unknown",
    source_name: null,
    source_entity_id: null,
    equipment_suit_selector: null,
    dreamscope_selector: null,
  };
}

function compactEquipmentSuitSelector(selector) {
  if (!selector
      || !Number.isSafeInteger(selector.map_key)
      || !Number.isSafeInteger(selector.attribute_key)) {
    return null;
  }
  return {
    map_key: selector.map_key,
    attribute_key: selector.attribute_key,
    required_pieces: Number.isSafeInteger(selector.required_pieces)
      ? selector.required_pieces : null,
  };
}

function compactDreamscopeSelector(selector) {
  if (!selector
      || !["tree_node", "advanced_tree_effect", "factor_family"].includes(selector.source_kind)
      || !Number.isSafeInteger(selector.source_id)) {
    return null;
  }
  return {
    source_kind: selector.source_kind,
    source_id: selector.source_id,
    candidate_item_ids: [...new Set((selector.candidate_item_ids || [])
      .filter(Number.isSafeInteger))].sort((left, right) => left - right),
    candidate_grades: [...new Set((selector.candidate_grades || [])
      .filter(Number.isSafeInteger))].sort((left, right) => left - right),
  };
}

function collapseSemanticCandidates(candidates) {
  const retained = [];
  for (const candidate of candidates) {
    const aliasIndex = retained.findIndex((existing) => areDreamscopeTreeAliases(existing, candidate));
    if (aliasIndex < 0) {
      retained.push(candidate);
      continue;
    }
    if (candidate.source_kind === "dreamscope-tree-node") retained[aliasIndex] = candidate;
  }
  return retained;
}

function areDreamscopeTreeAliases(left, right) {
  const kinds = new Set([left.source_kind, right.source_kind]);
  return kinds.size === 2
    && kinds.has("dreamscope-tree-node")
    && kinds.has("season-talent-node")
    && left.source_entity_id !== null
    && left.source_entity_id === right.source_entity_id
    && left.source_name !== null
    && left.source_name === right.source_name;
}

function promoteSemanticAliasResolution(resolution, aliasesCollapsedToOne) {
  return aliasesCollapsedToOne && resolution === "ambiguous" ? "exact" : resolution;
}

function normalizeResolution(value) {
  if (value === "exact") return "exact";
  if (value === "ambiguous" || value === "candidate-factor-items-or-grades"
      || value === "multiple-capable-module-items") return "ambiguous";
  return "unresolved";
}

function sortedRecord(value) {
  return Object.fromEntries(Object.entries(value).sort(([left], [right]) => left.localeCompare(right)));
}

function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    if (!argument.startsWith("--")) throw new Error(`unexpected argument ${argument}`);
    const key = argument.slice(2);
    const value = args[index + 1];
    if (!value || value.startsWith("--")) throw new Error(`missing value for ${argument}`);
    parsed[key] = value;
    index += 1;
  }
  return parsed;
}

function resolvePath(value) {
  return path.isAbsolute(value) ? value : path.resolve(repositoryRoot, value);
}

function readJson(filePath) {
  return JSON.parse(readFileSync(filePath, "utf8"));
}
