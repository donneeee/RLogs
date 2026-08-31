#!/usr/bin/env node

import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const options = parseArgs(process.argv.slice(2));
const inputPath = resolvePath(options.input ||
  "plugins/games/blue-protocol-star-resonance/game-data/runtime/rdps-effect-origin-runtime.v1.json");
const outputPath = resolvePath(options.output ||
  "plugins/games/blue-protocol-star-resonance/research/game-file-inventory/global/steam-24687926/rdps-effect-origin-proof-worklist.v1.json");
const runtime = readJson(inputPath);

if (runtime.schema_version !== 1 || !runtime.effects_by_id) {
  throw new Error("unsupported effect-origin runtime schema");
}

const originRows = [];
const effectOnlyRows = [];

for (const [effectIdText, effect] of Object.entries(runtime.effects_by_id)) {
  const effectId = Number(effectIdText);
  for (const [originKey, origin] of Object.entries(effect.origins_by_key || {})) {
    if (origin.endpoint_resolution === "exact" && origin.owner_resolution === "exact") continue;
    const [sourceTypeId, sourceConfigId] = originKey.split(":").map(Number);
    const proof = describeRequiredProof(origin, {
      observedOrigin: true,
      formulaEndpointPresent: effect.formula_endpoint_present,
    });
    originRows.push({
      priority: originPriority(effect, origin),
      effect_id: effectId,
      display_name: effect.display_name,
      source_type_id: sourceTypeId,
      source_config_id: sourceConfigId,
      formula_endpoint_present: effect.formula_endpoint_present,
      transfer_proof_state: effect.transfer_proof_state,
      endpoint_resolution: origin.endpoint_resolution,
      owner_resolution: origin.owner_resolution,
      candidate_sources: origin.candidate_sources || [],
      unresolved_terminal_ids: origin.unresolved_terminal_ids || [],
      ...proof,
    });
  }

  if (Object.keys(effect.origins_by_key || {}).length > 0) continue;
  if (effect.fallback.endpoint_resolution === "exact" && effect.fallback.owner_resolution === "exact") continue;
  const proof = describeRequiredProof(effect.fallback, {
    observedOrigin: false,
    formulaEndpointPresent: effect.formula_endpoint_present,
  });
  effectOnlyRows.push({
    priority: effectOnlyPriority(effect),
    effect_id: effectId,
    display_name: effect.display_name,
    formula_endpoint_present: effect.formula_endpoint_present,
    transfer_proof_state: effect.transfer_proof_state,
    endpoint_resolution: effect.fallback.endpoint_resolution,
    owner_resolution: effect.fallback.owner_resolution,
    candidate_sources: effect.fallback.candidate_sources || [],
    unresolved_terminal_ids: effect.fallback.unresolved_terminal_ids || [],
    ...proof,
  });
}

originRows.sort(compareRows);
effectOnlyRows.sort(compareRows);

const result = {
  schema_version: 1,
  game: runtime.game,
  game_build: runtime.game_build,
  generated_by: "tools/rdps-effect-origin-proof-worklist.mjs",
  policy: {
    research_only_not_loaded_by_live_runtime: true,
    terminal_effect_and_packet_origin_are_the_runtime_fingerprint: true,
    ambiguity_is_preserved: true,
    no_observed_effect_is_hidden: true,
  },
  summary: {
    observed_origin_rows_requiring_proof: originRows.length,
    exact_endpoint_owner_unresolved: originRows.filter((row) =>
      row.endpoint_resolution === "exact" && row.owner_resolution !== "exact").length,
    ambiguous_endpoints: originRows.filter((row) => row.endpoint_resolution === "ambiguous").length,
    unresolved_endpoints: originRows.filter((row) => row.endpoint_resolution === "unresolved").length,
    effects_without_observed_origin_requiring_proof: effectOnlyRows.length,
    effect_only_exact_endpoint_owner_unresolved: effectOnlyRows.filter((row) =>
      row.endpoint_resolution === "exact" && row.owner_resolution !== "exact").length,
    effect_only_ambiguous: effectOnlyRows.filter((row) => row.endpoint_resolution === "ambiguous").length,
    effect_only_unresolved: effectOnlyRows.filter((row) => row.endpoint_resolution === "unresolved").length,
    proof_discriminators: countBy(
      [...originRows, ...effectOnlyRows],
      (row) => row.proof_discriminator,
    ),
  },
  observed_origins_requiring_proof: originRows,
  effects_without_observed_origin_requiring_proof: effectOnlyRows,
};

writeFileSync(outputPath, `${JSON.stringify(result, null, 2)}\n`);
console.log(JSON.stringify({ output: outputPath, ...result.summary }, null, 2));

function originPriority(effect, origin) {
  if (isExternalCandidate(effect.transfer_proof_state) && origin.endpoint_resolution === "exact") return 1;
  if (isExternalCandidate(effect.transfer_proof_state)) return 2;
  if (origin.endpoint_resolution === "exact") return 3;
  if (origin.endpoint_resolution === "ambiguous") return 4;
  return 5;
}

function effectOnlyPriority(effect) {
  if (isExternalCandidate(effect.transfer_proof_state)) return 2;
  if (effect.formula_endpoint_present && effect.fallback.endpoint_resolution === "ambiguous") return 4;
  return 5;
}

function isExternalCandidate(value) {
  return value === "external_formula_lifecycle_missing"
    || value === "external_lifecycle_formula_scope_unresolved"
    || value === "external_complete";
}

function describeRequiredProof(resolution, options) {
  const selectorEvidence = collectSelectorEvidence(resolution.candidate_sources || []);
  const hasFactorFamilies = selectorEvidence.factor_families.length > 0;
  const hasTreeNodes = selectorEvidence.tree_node_ids.length > 0;
  const hasAdvancedTreeEffects = selectorEvidence.advanced_tree_effect_ids.length > 0;
  const hasEquipmentSuits = selectorEvidence.equipment_suits.length > 0;
  const selectorKinds = [
    hasFactorFamilies && "factor_item",
    (hasTreeNodes || hasAdvancedTreeEffects) && "dreamscope_node",
    hasEquipmentSuits && "equipment_suit",
  ].filter(Boolean);

  let proofDiscriminator;
  if (selectorKinds.length > 1) proofDiscriminator = "mixed_exact_loadout";
  else if (hasFactorFamilies) proofDiscriminator = "exact_factor_item";
  else if (hasTreeNodes || hasAdvancedTreeEffects) proofDiscriminator = "exact_dreamscope_node";
  else if (hasEquipmentSuits) proofDiscriminator = "exact_equipment_suit";
  else if (resolution.endpoint_resolution === "exact") proofDiscriminator = "provider_source_snapshot";
  else if (resolution.endpoint_resolution === "ambiguous") proofDiscriminator = "source_lifecycle_branch";
  else proofDiscriminator = options.formulaEndpointPresent
    ? "terminal_source_route"
    : "missing_formula_route";

  const requiredPacketEvidence = [];
  if (!options.observedOrigin) {
    requiredPacketEvidence.push("status_origin_source_type_id", "status_origin_source_config_id");
  }
  if (hasFactorFamilies) requiredPacketEvidence.push("provider_exact_factor_item_ids_at_event_time");
  if (hasTreeNodes) requiredPacketEvidence.push("provider_exact_tree_node_ids_at_event_time");
  if (hasAdvancedTreeEffects) {
    requiredPacketEvidence.push("provider_exact_advanced_tree_effect_ids_at_event_time");
  }
  if (hasEquipmentSuits) requiredPacketEvidence.push("provider_exact_equipment_suit_at_event_time");
  requiredPacketEvidence.push(
    "provider_actor_id",
    "recipient_actor_id",
    "status_apply_refresh_remove_timestamps",
    "terminal_damage_events_inside_window",
  );

  return {
    proof_discriminator: proofDiscriminator,
    selector_evidence: selectorEvidence,
    required_packet_evidence: [...new Set(requiredPacketEvidence)],
    required_evidence: proofInstruction(proofDiscriminator, options.observedOrigin),
  };
}

function collectSelectorEvidence(candidateSources) {
  const factorFamilies = new Map();
  const treeNodeIds = new Set();
  const advancedTreeEffectIds = new Set();
  const equipmentSuits = new Map();

  for (const candidate of candidateSources) {
    const dreamscope = candidate.dreamscope_selector;
    if (dreamscope?.source_kind === "factor_family") {
      const itemIds = numericList(dreamscope.candidate_item_ids);
      const grades = numericList(dreamscope.candidate_grades);
      const key = String(dreamscope.source_id);
      factorFamilies.set(key, {
        source_id: dreamscope.source_id,
        source_name: candidate.source_name || null,
        candidate_item_ids: itemIds,
        candidate_grades: grades,
      });
    } else if (dreamscope?.source_kind === "tree_node") {
      treeNodeIds.add(dreamscope.source_id);
    } else if (dreamscope?.source_kind === "advanced_tree_effect") {
      advancedTreeEffectIds.add(dreamscope.source_id);
    }

    const suit = candidate.equipment_suit_selector;
    if (suit && Number.isSafeInteger(suit.map_key) && Number.isSafeInteger(suit.attribute_key)) {
      const key = `${suit.map_key}:${suit.attribute_key}:${suit.required_pieces || 0}`;
      equipmentSuits.set(key, {
        map_key: suit.map_key,
        attribute_key: suit.attribute_key,
        required_pieces: Number.isSafeInteger(suit.required_pieces) ? suit.required_pieces : null,
        source_name: candidate.source_name || null,
      });
    }
  }

  return {
    factor_families: [...factorFamilies.values()].sort((left, right) => left.source_id - right.source_id),
    tree_node_ids: [...treeNodeIds].sort((left, right) => left - right),
    advanced_tree_effect_ids: [...advancedTreeEffectIds].sort((left, right) => left - right),
    equipment_suits: [...equipmentSuits.values()].sort((left, right) =>
      left.map_key - right.map_key || left.attribute_key - right.attribute_key),
  };
}

function proofInstruction(discriminator, observedOrigin) {
  const originPrefix = observedOrigin
    ? "retain the observed source_type_id/source_config_id and "
    : "capture source_type_id/source_config_id, then ";
  switch (discriminator) {
    case "exact_factor_item":
      return `${originPrefix}match the provider's exact equipped factor item at the event timestamp; the item ID proves family and grade`;
    case "exact_dreamscope_node":
      return `${originPrefix}match the provider's exact active Dreamscope tree or advanced-effect node at the event timestamp`;
    case "exact_equipment_suit":
      return `${originPrefix}match the provider's exact equipment-suit map key, attribute key, and required piece count at the event timestamp`;
    case "mixed_exact_loadout":
      return `${originPrefix}match every candidate against the provider's exact factor, tree, and equipment snapshot without selecting by display name`;
    case "provider_source_snapshot":
      return `${originPrefix}correlate the proven terminal endpoint with the provider's exact equipped source snapshot at the event timestamp`;
    case "source_lifecycle_branch":
      return `${originPrefix}correlate source cast, status application, recipient window, and terminal damage to select one candidate branch`;
    case "terminal_source_route":
      return `${originPrefix}decode the terminal source route and preserve its complete provider/recipient lifecycle`;
    default:
      return `${originPrefix}connect the packet-observed child effect to its terminal formula route without discarding the event`;
  }
}

function numericList(values) {
  return [...new Set((values || []).filter(Number.isSafeInteger))].sort((left, right) => left - right);
}

function countBy(values, keyForValue) {
  const counts = {};
  for (const value of values) {
    const key = keyForValue(value);
    counts[key] = (counts[key] || 0) + 1;
  }
  return Object.fromEntries(Object.entries(counts).sort(([left], [right]) => left.localeCompare(right)));
}

function compareRows(left, right) {
  return left.priority - right.priority
    || left.effect_id - right.effect_id
    || (left.source_type_id || 0) - (right.source_type_id || 0)
    || (left.source_config_id || 0) - (right.source_config_id || 0);
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
