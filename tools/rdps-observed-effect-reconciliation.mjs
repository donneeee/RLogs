#!/usr/bin/env node

import { existsSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const options = parseArgs(process.argv.slice(2));
const auditPath = resolvePath(options.audit || "runtime-data/research/rdps/current-build-latest-origin-catalog.json");
const audit = readJson(auditPath, "packet-origin audit");
const gameBuild = String(audit.game_build || "").trim();
if (!/^\d+$/.test(gameBuild)) throw new Error("packet-origin audit has no numeric game_build");

const formulaPath = resolvePath(options.formulas || path.join(
  "..",
  "BPSR-UID-Extractors",
  `output-build-${gameBuild}-exact`,
  "ModifierFormulaTermTable.json",
));
const effectSourcesPath = resolvePath(options.sources || path.join(
  "..",
  "BPSR-UID-Extractors",
  `output-build-${gameBuild}-exact`,
  "EffectSources.json",
));
const damageFormulaSurfacePath = resolvePath(options.damageSurface || path.join(
  "..",
  "BPSR-UID-Extractors",
  `output-build-${gameBuild}-exact`,
  "DamageFormulaSurface.json",
));
const skillDamageChainPath = resolvePath(options.damageChains || path.join(
  "..",
  "BPSR-UID-Extractors",
  `output-build-${gameBuild}-exact`,
  "SkillDamageChainBridge.json",
));
const skillNamesPath = resolvePath(options.skillNames || path.join(
  "..",
  "BPSR-UID-Extractors",
  `output-build-${gameBuild}-exact`,
  "skillnames.json",
));
const dreamscopePath = resolvePath(options.dreamscope ||
  "plugins/games/blue-protocol-star-resonance/game-data/runtime/dreamscope-build-inference.v1.json");
const moduleTerminalMapPath = resolvePath(options.moduleTerminals || path.join(
  "plugins/games/blue-protocol-star-resonance/research/game-file-inventory/global",
  `steam-${gameBuild}`,
  "module-terminal-effect-map.v1.json",
));
const outputPath = resolvePath(options.output || path.join(
  "plugins/games/blue-protocol-star-resonance/research/game-file-inventory/global",
  `steam-${gameBuild}`,
  "rdps-observed-effect-reconciliation.v1.json",
));

if (options.verify) {
  verifyCatalog(readJson(resolvePath(options.verify), "reconciliation catalog"));
  console.log(`verified ${resolvePath(options.verify)}`);
  process.exit(0);
}

const formulas = readJson(formulaPath, "modifier formula table");
const effectSources = readJson(effectSourcesPath, "effect source index");
const damageFormulaSurface = readJson(damageFormulaSurfacePath, "damage formula surface");
const skillDamageChains = readJson(skillDamageChainPath, "skill damage chains");
const skillNames = readJson(skillNamesPath, "skill names");
const dreamscope = readJson(dreamscopePath, "Dreamscope inference catalog");
const moduleTerminalMap = readJson(moduleTerminalMapPath, "module terminal-effect map");
if (String(dreamscope.game_build) !== gameBuild) {
  throw new Error(`Dreamscope build ${dreamscope.game_build} does not match packet build ${gameBuild}`);
}
if (String(moduleTerminalMap.game_build) !== gameBuild
    || !moduleTerminalMap.policy?.capable_module_items_are_candidate_sources_not_equipped_proof) {
  throw new Error(`module terminal-effect map does not safely match packet build ${gameBuild}`);
}
if (!Array.isArray(audit.effects) || audit.effects.length === 0) {
  throw new Error("packet-origin audit has no effects");
}
if (!effectSources.effectSourcesById || !effectSources.buffIdToEffectSourceIds) {
  throw new Error("effect source index is missing its source or buff indexes");
}
if (!damageFormulaSurface.linked_hit_event_candidate_lookup || !skillDamageChains.damageChains) {
  throw new Error("damage-chain inputs are missing their current-build indexes");
}

const syntheticSourcesById = new Map();
const bulletDamageIdsByConfigId = buildBulletDamageIndex(
  damageFormulaSurface.linked_hit_event_candidate_lookup,
);

const relationsByEffectId = new Map();
for (const relation of audit.relations || []) {
  const effectId = integer(relation.effect_id, "relation effect_id");
  const relations = relationsByEffectId.get(effectId) || [];
  relations.push(relation);
  relationsByEffectId.set(effectId, relations);
}

const effects = [...audit.effects]
  .sort((left, right) => left.effect_id - right.effect_id)
  .map((observed) => reconcileEffect(observed));
const queueCounts = countBy(effects, (effect) => effect.proof_queue);
const endpointCounts = countBy(effects, (effect) => effect.endpoint_resolution.state);
const sourceCounts = countBy(effects, (effect) => effect.source_resolution.state);
const transferCounts = countBy(effects, (effect) => effect.transfer_proof.state);
const formulaEffects = effects.filter((effect) => effect.formula_endpoint !== null).length;
const runtimeBridges = effects.filter((effect) => effect.endpoint_resolution.state === "exact_runtime_bridge").length;
const moduleTerminalMatches = effects.filter((effect) =>
  effect.endpoint_resolution.state === "exact_module_terminal_effect").length;
const terminalMatches = effects.filter((effect) => ["exact_terminal_unique", "ambiguous_terminal"]
  .includes(effect.endpoint_resolution.state)).length;

const result = {
  schema_version: 1,
  game: "blue-protocol-star-resonance",
  game_build: gameBuild,
  generated_by: "tools/rdps-observed-effect-reconciliation.mjs",
  policy: {
    matching_build_packet_effects_are_conserved: true,
    formula_terminal_ids_are_join_keys_not_automatic_rdps_credit: true,
    provider_recipient_lifecycle_is_required_for_external_credit: true,
    exact_scalar_is_required_for_formula_replay: true,
    adjacent_numeric_ids_never_create_bridges: true,
    ambiguous_and_unresolved_evidence_is_preserved: true,
    historical_evidence_does_not_promote_current_build_proof: true,
  },
  inputs: {
    packet_origin_audit: relativePath(auditPath),
    modifier_formula_table: relativePath(formulaPath),
    effect_source_index: relativePath(effectSourcesPath),
    damage_formula_surface: relativePath(damageFormulaSurfacePath),
    skill_damage_chains: relativePath(skillDamageChainPath),
    skill_names: relativePath(skillNamesPath),
    dreamscope_inference: relativePath(dreamscopePath),
    module_terminal_effect_map: relativePath(moduleTerminalMapPath),
  },
  summary: {
    observed_effects: effects.length,
    reconciled_effects: effects.length,
    formula_endpoints: formulaEffects,
    effects_without_formula_endpoint: effects.length - formulaEffects,
    explicit_runtime_bridges: runtimeBridges,
    dreamscope_terminal_matches: terminalMatches,
    module_terminal_effect_matches: moduleTerminalMatches,
    endpoint_resolution_counts: endpointCounts,
    source_resolution_counts: sourceCounts,
    exact_unique_source_joins: effects.filter((effect) => effect.source_resolution.exact_unique_source).length,
    exact_owning_source_joins: effects.filter((effect) => effect.source_resolution.exact_owning_source).length,
    exact_packet_origin_endpoint_joins: effects
      .filter((effect) => effect.source_resolution.exact_per_packet_origin_endpoint).length,
    exact_per_packet_origin_joins: effects.filter((effect) => effect.source_resolution.exact_per_packet_origin).length,
    ambiguous_source_joins: effects.filter((effect) => effect.source_resolution.ambiguous).length,
    effects_without_static_source_join: effects.filter((effect) => effect.source_resolution.candidates.length === 0).length,
    transfer_proof_counts: transferCounts,
    proof_queue_counts: queueCounts,
    ready_for_external_rdps_replay: queueCounts.external_formula_lifecycle_and_scalar_proven || 0,
    still_requires_proof: effects.filter((effect) => effect.transfer_proof.state !== "external_complete").length,
    conservation_complete: true,
  },
  effects,
};

verifyCatalog(result);
writeFileSync(outputPath, `${JSON.stringify(result, null, 2)}\n`);
console.log(JSON.stringify({ output: outputPath, ...result.summary }, null, 2));

function reconcileEffect(observed) {
  const effectId = integer(observed.effect_id, "observed effect_id");
  const formula = formulas.entriesByKey?.[`buffs:${effectId}`] || null;
  const runtimeCandidates = dreamscope.candidates_by_runtime_effect_id?.[String(effectId)] || [];
  const terminalCandidates = dreamscope.candidates_by_terminal_effect_id?.[String(effectId)] || [];
  const runtimeLink = dreamscope.runtime_effect_links_by_id?.[String(effectId)] || null;
  const moduleTerminal = moduleTerminalMap.terminal_effects_by_id?.[String(effectId)] || null;
  const packetRelations = (relationsByEffectId.get(effectId) || [])
    .sort((left, right) => left.source_type_id - right.source_type_id
      || left.source_config_id - right.source_config_id);
  const endpointResolution = resolveEndpoint(
    runtimeCandidates,
    terminalCandidates,
    runtimeLink,
    moduleTerminal,
    formula,
  );
  const sourceResolution = resolveSources(effectId, runtimeLink, packetRelations);
  const formulaEndpoint = formula ? summarizeFormula(formula) : null;
  const lifecycle = summarizeLifecycle(observed);
  const external = externalFormulaProof(formulaEndpoint, runtimeLink);
  const transferProof = resolveTransferProof(formulaEndpoint, lifecycle, external);
  return {
    effect_id: effectId,
    display_name: formula?.name || null,
    proof_queue: queueFor(endpointResolution.state, transferProof.state),
    endpoint_resolution: endpointResolution,
    source_resolution: sourceResolution,
    transfer_proof: transferProof,
    packet_lifecycle: lifecycle,
    packet_origins: packetRelations,
    formula_endpoint: formulaEndpoint,
    module_terminal_effect: moduleTerminal,
  };
}

function resolveSources(effectId, runtimeLink, packetRelations) {
  const terminalEffectId = positiveIntegerOrNull(runtimeLink?.source_terminal_effect_id);
  const directIds = sourceIdsForBuffOrFormulaOwner(effectId);
  const terminalIds = terminalEffectId === null ? [] : sourceIdsForBuffOrFormulaOwner(terminalEffectId);
  const packetOriginRoutes = packetRelations.map((relation) => {
    const resolution = tracePacketOrigin(
      relation.source_config_id,
      relation.configured_source_table,
      new Set([`BuffTable.ctb:${effectId}`]),
    );
    return {
      source_type_id: relation.source_type_id,
      source_kind: relation.source_kind,
      configured_source_table: relation.configured_source_table,
      source_config_id: relation.source_config_id,
      candidate_source_ids: resolution.candidate_source_ids,
      unresolved_terminal_ids: resolution.unresolved_terminal_ids,
      origin_paths: resolution.origin_paths,
      endpoint_resolution_state: resolution.endpoint_resolution_state,
      owning_source_resolution_state: resolution.owning_source_resolution_state,
      observation_count: relation.observation_count,
    };
  });
  const mappedOriginRoutes = packetOriginRoutes.filter((route) => route.candidate_source_ids.length > 0);
  const packetOriginIds = [...new Set(mappedOriginRoutes.flatMap((route) => route.candidate_source_ids))].sort();
  const everyPacketOriginMapped = packetOriginRoutes.length > 0
    && mappedOriginRoutes.length === packetOriginRoutes.length;
  const everyPacketOriginEndpointUnique = everyPacketOriginMapped
    && packetOriginRoutes.every((route) => route.candidate_source_ids.length === 1);
  const everyPacketOriginOwnerExact = everyPacketOriginEndpointUnique
    && packetOriginRoutes.every((route) => route.owning_source_resolution_state === "exact");
  const selectedRoute = terminalIds.length > 0 ? "runtime_terminal_effect"
    : packetOriginIds.length > 0 ? "packet_configured_origin"
      : directIds.length > 0 ? "direct_observed_effect" : "none";
  const selectedIds = selectedRoute === "runtime_terminal_effect" ? terminalIds
    : selectedRoute === "packet_configured_origin" ? packetOriginIds : directIds;
  const candidates = selectedIds.map((sourceId) => summarizeSource(sourceId));
  const moduleFamilyCapabilityOnly = selectedRoute === "direct_observed_effect"
    && candidates.length > 0
    && candidates.every((candidate) => candidate.source_kind === "module-effect-family")
    && candidates.every((candidate) => !candidateHasExactOwner(candidate));
  const state = selectedRoute === "runtime_terminal_effect"
    ? (candidates.length === 1 ? "exact_unique_runtime_terminal_source" : "ambiguous_runtime_terminal_sources")
    : selectedRoute === "packet_configured_origin"
      ? (everyPacketOriginOwnerExact ? "exact_packet_origin_sources"
        : everyPacketOriginMapped ? "ambiguous_packet_origin_sources" : "partial_packet_origin_sources")
    : selectedRoute === "direct_observed_effect"
      ? (moduleFamilyCapabilityOnly ? "exact_module_effect_family_candidate_modules"
        : candidates.length === 1 ? "exact_unique_direct_source" : "ambiguous_direct_sources")
      : "no_static_source_join";
  const exactPerPacketOriginEndpoint = selectedRoute === "packet_configured_origin"
    && everyPacketOriginEndpointUnique;
  const exactPerPacketOrigin = selectedRoute === "packet_configured_origin"
    && everyPacketOriginOwnerExact;
  const exactOwningSource = selectedRoute === "packet_configured_origin"
    ? everyPacketOriginOwnerExact
    : candidates.length === 1 && candidateHasExactOwner(candidates[0]);
  return {
    state,
    route: selectedRoute,
    observed_effect_id: effectId,
    source_terminal_effect_id: terminalEffectId,
    exact_unique_source: candidates.length === 1,
    exact_owning_source: exactOwningSource,
    exact_per_packet_origin_endpoint: exactPerPacketOriginEndpoint,
    exact_per_packet_origin: exactPerPacketOrigin,
    ambiguous: selectedRoute === "packet_configured_origin"
      ? !everyPacketOriginOwnerExact : candidates.length > 1 || !exactOwningSource,
    candidate_source_ids: selectedIds,
    candidates,
    evidence_routes: {
      direct_observed_effect: directIds,
      runtime_terminal_effect: terminalIds,
      packet_configured_origins: packetOriginRoutes,
    },
    selected_grade_or_tier_proven: false,
  };
}

function tracePacketOrigin(configId, configuredTable, visited) {
  const sourceId = positiveIntegerOrNull(configId);
  if (sourceId === null) {
    return { candidate_source_ids: [], unresolved_terminal_ids: [], origin_paths: [],
      endpoint_resolution_state: "unresolved", owning_source_resolution_state: "unresolved" };
  }
  const sourceTable = configuredTable || "BuffTable.ctb";
  const visitKey = `${sourceTable}:${sourceId}`;
  if (visited.has(visitKey)) {
    return {
      candidate_source_ids: [],
      unresolved_terminal_ids: [sourceId],
      endpoint_resolution_state: "unresolved",
      owning_source_resolution_state: "unresolved",
      origin_paths: [{ state: "cycle", configured_ids: [sourceId], configured_tables: [sourceTable],
        candidate_source_ids: [] }],
    };
  }
  if (sourceTable === "BulletTable.ctb") return traceBulletOrigin(sourceId);
  if (sourceTable !== "BuffTable.ctb") {
    return {
      candidate_source_ids: [],
      unresolved_terminal_ids: [sourceId],
      endpoint_resolution_state: "unresolved",
      owning_source_resolution_state: "unresolved",
      origin_paths: [{ state: "unsupported_source_table", configured_ids: [sourceId],
        configured_tables: [sourceTable], candidate_source_ids: [] }],
    };
  }
  const direct = sourceIdsForBuffOrFormulaOwner(sourceId);
  if (direct.length > 0) {
    const directCandidates = direct.map((candidateSourceId) => summarizeSource(candidateSourceId));
    const everyOwnerExact = directCandidates.every(candidateHasExactOwner);
    return {
      candidate_source_ids: direct,
      unresolved_terminal_ids: [],
      endpoint_resolution_state: direct.length === 1 ? "exact" : "ambiguous",
      owning_source_resolution_state: direct.length === 1 && everyOwnerExact ? "exact" : "ambiguous",
      origin_paths: [{ state: direct.length === 1 && everyOwnerExact ? "exact_static_source"
        : direct.length === 1 ? "exact_endpoint_candidate_owner"
          : "ambiguous_static_source",
        configured_ids: [sourceId], configured_tables: [sourceTable], candidate_source_ids: direct }],
    };
  }
  const upstream = relationsByEffectId.get(sourceId) || [];
  if (upstream.length === 0) {
    return {
      candidate_source_ids: [],
      unresolved_terminal_ids: [sourceId],
      endpoint_resolution_state: "unresolved",
      owning_source_resolution_state: "unresolved",
      origin_paths: [{ state: "unresolved_terminal", configured_ids: [sourceId],
        configured_tables: [sourceTable], candidate_source_ids: [] }],
    };
  }
  const nextVisited = new Set(visited);
  nextVisited.add(visitKey);
  const branches = upstream.map((relation) => {
    const traced = tracePacketOrigin(
      relation.source_config_id,
      relation.configured_source_table,
      nextVisited,
    );
    return {
      relation,
      traced,
    };
  });
  const candidateSourceIds = [...new Set(branches
    .flatMap(({ traced }) => traced.candidate_source_ids))].sort();
  const allEndpointsExact = branches.length > 0 && branches.every(({ traced }) =>
    traced.endpoint_resolution_state === "exact");
  const allOwnersExact = allEndpointsExact && branches.every(({ traced }) =>
    traced.owning_source_resolution_state === "exact");
  return {
    candidate_source_ids: candidateSourceIds,
    unresolved_terminal_ids: [...new Set(branches
      .flatMap(({ traced }) => traced.unresolved_terminal_ids))].sort((left, right) => left - right),
    endpoint_resolution_state: allEndpointsExact && candidateSourceIds.length === 1 ? "exact"
      : candidateSourceIds.length > 0 ? "ambiguous" : "unresolved",
    owning_source_resolution_state: allOwnersExact && candidateSourceIds.length === 1 ? "exact"
      : candidateSourceIds.length > 0 ? "ambiguous" : "unresolved",
    origin_paths: branches.flatMap(({ relation, traced }) => traced.origin_paths.map((originPath) => ({
      state: originPath.state,
      configured_ids: [sourceId, ...originPath.configured_ids],
      configured_tables: [sourceTable, ...(originPath.configured_tables || [])],
      candidate_source_ids: originPath.candidate_source_ids,
      relation_source_type_ids: [relation.source_type_id, ...(originPath.relation_source_type_ids || [])],
    }))),
  };
}

function traceBulletOrigin(configId) {
  const damageIds = bulletDamageIdsByConfigId.get(configId) || [];
  const candidateIds = damageIds
    .map((damageId) => registerBulletChainSource(configId, damageId))
    .filter((sourceId) => sourceId !== null);
  const unresolvedDamageIds = damageIds.filter((damageId) =>
    !skillDamageChains.damageChains[String(damageId)]);
  const candidates = candidateIds.map((sourceId) => summarizeSource(sourceId));
  const endpointState = candidateIds.length === 1 ? "exact"
    : candidateIds.length > 1 ? "ambiguous" : "unresolved";
  const owningSourceState = endpointState === "exact" && candidateHasExactOwner(candidates[0])
    ? "exact" : candidateIds.length > 0 ? "ambiguous" : "unresolved";
  return {
    candidate_source_ids: candidateIds,
    unresolved_terminal_ids: candidateIds.length > 0 ? [] : [configId],
    endpoint_resolution_state: endpointState,
    owning_source_resolution_state: owningSourceState,
    origin_paths: [{
      state: endpointState === "exact" && owningSourceState === "exact"
        ? "exact_bullet_damage_chain_owner"
        : endpointState === "exact" ? "exact_bullet_damage_chain_owner_ambiguous"
        : candidateIds.length > 1 ? "ambiguous_bullet_damage_chains" : "unresolved_bullet_config",
      configured_ids: [configId],
      configured_tables: ["BulletTable.ctb"],
      damage_ids: damageIds,
      unresolved_damage_ids: unresolvedDamageIds,
      candidate_source_ids: candidateIds,
      endpoint_resolution_state: endpointState,
      owning_source_resolution_state: owningSourceState,
    }],
  };
}

function registerBulletChainSource(configId, damageId) {
  const chain = skillDamageChains.damageChains[String(damageId)];
  const sourceId = `bullet-chain:${configId}:${damageId}`;
  if (!chain) return null;
  if (!syntheticSourcesById.has(sourceId)) {
    const skillIds = [...new Set([
      positiveIntegerOrNull(chain.recountOwnerSkillId),
      positiveIntegerOrNull(chain.baseSkillId),
    ].filter((value) => value !== null))];
    const ownerResolutionState = skillIds.length === 1 ? "exact"
      : skillIds.length > 1 ? "ambiguous" : "unresolved";
    syntheticSourcesById.set(sourceId, {
      source_id: sourceId,
      source_kind: "bullet-damage-chain",
      source_type: chain.category || "damage-chain",
      source_name: chain.displayName || null,
      source_entity_id: skillIds.length === 1 ? skillIds[0] : positiveIntegerOrNull(damageId),
      runtime_detection: "packet-configured-bullet-origin",
      buff_ids: [],
      non_owning_candidate_buff_ids: [],
      icon_path: null,
      attribution: emptyAttribution(),
      owning_source_resolution_state: ownerResolutionState,
      current_build_origin: {
        configured_source_table: "BulletTable.ctb",
        configured_source_id: configId,
        damage_id: damageId,
        linked_source: chain.linkedSource || null,
        linked_id: positiveIntegerOrNull(chain.linkedId),
        base_skill_id: positiveIntegerOrNull(chain.baseSkillId),
        recount_owner_skill_id: positiveIntegerOrNull(chain.recountOwnerSkillId),
          candidate_skill_ids: skillIds,
        recount_parents: (chain.recountParents || []).map((parent) => ({
          id: positiveIntegerOrNull(parent.id),
          name: parent.name || null,
          damage_count: Number(parent.damageCount || 0),
        })),
        allocation: chain.allocation || null,
      },
    });
  }
  return sourceId;
}

function sourceIdsForBuff(effectId) {
  const ids = effectSources.buffIdToEffectSourceIds?.[String(effectId)] || [];
  return [...new Set(ids)].sort();
}

function sourceIdsForBuffOrFormulaOwner(effectId) {
  return normalizeOwnedRuntimeEndpointAliases([...new Set([
    ...sourceIdsForBuff(effectId),
    ...formulaOwnerSourceIds(effectId),
    ...dreamscopeTerminalSourceIds(effectId),
    ...moduleTerminalSourceIds(effectId),
  ])].sort());
}

function normalizeOwnedRuntimeEndpointAliases(sourceIds) {
  const candidates = sourceIds.map((sourceId) => summarizeSource(sourceId));
  const passiveOwners = candidates.filter((candidate) => candidate.source_kind === "talent-passive");
  const endpointAliases = candidates.filter((candidate) => candidate.source_kind === "talent-skill");
  const collapsedAliasIds = new Set();

  for (const endpoint of endpointAliases) {
    const endpointId = positiveIntegerOrNull(endpoint.source_entity_id);
    if (endpointId === null || !endpoint.source_name || !endpoint.icon_path) continue;
    const exactOwners = passiveOwners.filter((owner) => owner.source_name === endpoint.source_name
      && owner.icon_path === endpoint.icon_path
      && (owner.buff_ids || []).map(positiveIntegerOrNull).includes(endpointId));
    if (exactOwners.length === 1) collapsedAliasIds.add(endpoint.source_id);
  }

  return sourceIds.filter((sourceId) => !collapsedAliasIds.has(sourceId));
}

function dreamscopeTerminalSourceIds(effectId) {
  const candidates = dreamscope.candidates_by_terminal_effect_id?.[String(effectId)] || [];
  return candidates.map((candidate) => {
    const sourceKind = String(candidate.source_kind || "unknown");
    const sourceEntityId = positiveIntegerOrNull(candidate.source_id);
    if (sourceEntityId === null) {
      throw new Error(`Dreamscope terminal ${effectId} has a candidate without a source_id`);
    }
    const sourceId = `dreamscope-${sourceKind}:${sourceEntityId}:terminal:${effectId}`;
    if (!syntheticSourcesById.has(sourceId)) {
      const itemIds = [...new Set((candidate.item_ids || [])
        .map(positiveIntegerOrNull).filter((value) => value !== null))]
        .sort((left, right) => left - right);
      const grades = [...new Set((candidate.grades || [])
        .map(finiteOrNull).filter((value) => value !== null))]
        .sort((left, right) => left - right);
      const familyOnly = sourceKind === "factor_family" && (itemIds.length !== 1 || grades.length !== 1);
      syntheticSourcesById.set(sourceId, {
        source_id: sourceId,
        source_kind: `dreamscope-${sourceKind.replaceAll("_", "-")}`,
        source_type: "current-build-dreamscope-terminal-effect",
        source_name: candidate.name || null,
        source_entity_id: sourceEntityId,
        dreamscope_selector: {
          source_kind: sourceKind,
          source_id: sourceEntityId,
          candidate_item_ids: itemIds,
          candidate_grades: grades,
        },
        runtime_detection: familyOnly
          ? "packet-terminal-effect-plus-factor-loadout-evidence"
          : "packet-terminal-effect",
        buff_ids: [effectId],
        non_owning_candidate_buff_ids: [],
        icon_path: candidate.icon_path || null,
        attribution: emptyAttribution(),
        owning_source_resolution_state: familyOnly ? "candidate-factor-items-or-grades" : "exact",
        current_build_origin: {
          endpoint_effect_id: effectId,
          dreamscope_source_kind: sourceKind,
          dreamscope_source_id: sourceEntityId,
          template_id: positiveIntegerOrNull(candidate.template_id),
          candidate_item_ids: itemIds,
          candidate_grades: grades,
          exact_equipped_source_proven: !familyOnly,
        },
      });
    }
    return sourceId;
  });
}

function moduleTerminalSourceIds(effectId) {
  const terminal = moduleTerminalMap.terminal_effects_by_id?.[String(effectId)];
  if (!terminal) return [];
  const familyIds = [...new Set(terminal.candidate_mod_effect_family_ids || [])].sort((left, right) => left - right);
  return familyIds.map((familyId) => {
    const sourceId = `module-effect-family:${familyId}:terminal:${effectId}`;
    if (!syntheticSourcesById.has(sourceId)) {
      const routes = (terminal.routes || []).filter((route) => route.mod_effect_family_id === familyId);
      const levels = [...new Set(routes.map((route) => route.mod_effect_level))].sort((left, right) => left - right);
      const capableModules = [...new Map(routes.flatMap((route) => route.capable_module_items || [])
        .map((module) => [module.module_item_id, module])).values()]
        .sort((left, right) => left.module_item_id - right.module_item_id);
      syntheticSourcesById.set(sourceId, {
        source_id: sourceId,
        source_kind: "module-effect-family",
        source_type: "current-build-module-terminal-effect",
        source_name: routes.find((route) => route.display_name)?.display_name || null,
        source_entity_id: familyId,
        runtime_detection: "packet-terminal-effect-plus-module-loadout-evidence",
        buff_ids: [effectId],
        non_owning_candidate_buff_ids: [],
        icon_path: routes.find((route) => route.icon_path)?.icon_path || null,
        attribution: emptyAttribution(),
        owning_source_resolution_state: "candidate-capable-module-items",
        current_build_origin: {
          endpoint_effect_id: effectId,
          mod_effect_family_id: familyId,
          candidate_levels: levels,
          capable_module_items: capableModules,
          exact_equipped_source_proven: false,
          proof_state: terminal.endpoint_resolution_state,
        },
      });
    }
    return sourceId;
  });
}

function formulaOwnerSourceIds(effectId) {
  const formula = formulas.entriesByKey?.[`buffs:${effectId}`];
  if (!formula) return [];
  const evidenceByOwnerSkillId = new Map();
  for (const component of formula.componentValueHints || []) {
    for (const value of component.values || []) {
      const ownerSkillId = positiveIntegerOrNull(value.ownerSkillId);
      if (ownerSkillId === null) continue;
      const evidence = evidenceByOwnerSkillId.get(ownerSkillId) || [];
      evidence.push({
        component_key: component.componentKey || null,
        source_table: value.sourceTable || null,
        source_file: value.sourceFile || null,
        source_path: value.sourcePath || null,
        buff_id: positiveIntegerOrNull(value.buffId),
        tier: finiteOrNull(value.tier),
        parameter_index: finiteOrNull(value.parameterIndex),
      });
      evidenceByOwnerSkillId.set(ownerSkillId, evidence);
    }
  }
  return [...evidenceByOwnerSkillId.entries()].map(([ownerSkillId, evidence]) => {
    const sourceId = `formula-owner-skill:${ownerSkillId}`;
    if (!syntheticSourcesById.has(sourceId)) {
      const skill = skillNames[String(ownerSkillId)] || {};
      syntheticSourcesById.set(sourceId, {
        source_id: sourceId,
        source_kind: "formula-owner-skill",
        source_type: "current-build-formula-owner",
        source_name: skill.Name || formula.name || null,
        source_entity_id: ownerSkillId,
        runtime_detection: "formula-endpoint-owner-skill-id",
        buff_ids: [effectId],
        non_owning_candidate_buff_ids: [],
        icon_path: skill.IconPath || skill.Icon || null,
        attribution: emptyAttribution(),
        owning_source_resolution_state: "exact",
        current_build_origin: {
          endpoint_effect_id: effectId,
          owner_skill_id: ownerSkillId,
          evidence,
        },
      });
    }
    return sourceId;
  }).sort();
}

function buildBulletDamageIndex(lookup) {
  const result = new Map();
  for (const [key, values] of Object.entries(lookup || {})) {
    const separator = key.indexOf(":");
    const configId = positiveIntegerOrNull(separator === -1 ? key : key.slice(0, separator));
    if (configId === null) continue;
    const damageIds = result.get(configId) || [];
    for (const value of Array.isArray(values) ? values : [values]) {
      const damageId = positiveIntegerOrNull(value);
      if (damageId !== null) damageIds.push(damageId);
    }
    result.set(configId, [...new Set(damageIds)].sort((left, right) => left - right));
  }
  return result;
}

function summarizeSource(sourceId) {
  const synthetic = syntheticSourcesById.get(sourceId);
  if (synthetic) return synthetic;
  const source = effectSources.effectSourcesById[sourceId];
  if (!source) throw new Error(`effect source index references missing source ${sourceId}`);
  const attribution = source.attributionModel || {};
  return {
    source_id: source.sourceId || sourceId,
    source_kind: source.sourceKind || null,
    source_type: source.sourceType || null,
    source_name: source.sourceName || null,
    source_entity_id: positiveIntegerOrNull(source.sourceEntityId),
    equipment_suit_selector: source.sourceKind === "equipment-set" ? {
      map_key: positiveIntegerOrNull(source.familyId),
      attribute_key: positiveIntegerOrNull(source.equipmentAttributeVariantId),
      required_pieces: positiveIntegerOrNull(source.requiredPieces),
    } : null,
    runtime_detection: source.runtimeDetection || null,
    buff_ids: source.buffIds || [],
    non_owning_candidate_buff_ids: source.nonOwningCandidateBuffIds || [],
    icon_path: source.iconPath || null,
    owning_source_resolution_state: "exact",
    attribution: {
      status: attribution.status || null,
      confidence: attribution.confidence || null,
      formula_term_ids: attribution.formulaTermIds || [],
      contribution_groups: attribution.contributionGroups || [],
      relationship_kinds: attribution.relationshipKinds || [],
      required_runtime_evidence: attribution.requiredRuntimeEvidence || [],
      components: (attribution.components || []).map((component) => ({
        component_key: component.componentKey || null,
        effect_class: component.effectClass || null,
        contribution_scope: component.contributionScope || null,
        value_scope: component.valueScope || null,
        transfer_eligibility: component.transferEligibility || null,
        formula_term_ids: component.formulaTermIds || [],
        contribution_groups: component.contributionGroups || [],
        required_runtime_evidence: component.requiredRuntimeEvidence || [],
      })),
    },
  };
}

function candidateHasExactOwner(candidate) {
  return candidate?.owning_source_resolution_state === "exact";
}

function emptyAttribution() {
  return {
    status: null,
    confidence: null,
    formula_term_ids: [],
    contribution_groups: [],
    relationship_kinds: [],
    required_runtime_evidence: [],
    components: [],
  };
}

function resolveEndpoint(runtimeCandidates, terminalCandidates, runtimeLink, moduleTerminal, formula) {
  if (runtimeLink) {
    if (runtimeCandidates.length === 0) throw new Error(`runtime bridge ${runtimeLink.runtime_effect_id} has no candidates`);
    return {
      state: "exact_runtime_bridge",
      candidates: runtimeCandidates,
      runtime_link: runtimeLink,
    };
  }
  if (terminalCandidates.length === 1) {
    return { state: "exact_terminal_unique", candidates: terminalCandidates, runtime_link: null };
  }
  if (terminalCandidates.length > 1) {
    return { state: "ambiguous_terminal", candidates: terminalCandidates, runtime_link: null };
  }
  if (moduleTerminal) {
    return {
      state: "exact_module_terminal_effect",
      candidates: moduleTerminal.routes || [],
      runtime_link: null,
      module_terminal_effect: moduleTerminal,
    };
  }
  return {
    state: formula ? "formula_endpoint_only" : "no_formula_endpoint",
    candidates: [],
    runtime_link: null,
  };
}

function summarizeFormula(formula) {
  const components = (formula.componentValueHints || []).map((component) => ({
    source_rule_id: component.sourceRuleId || null,
    component_key: component.componentKey || null,
    label: component.label || null,
    effect_class: component.effectClass || null,
    contribution_scope: component.contributionScope || null,
    value_scope: component.valueScope || null,
    direction: component.direction || null,
    stat: component.stat || null,
    formula_term_ids: component.formulaTermIds || [],
    contribution_groups: component.contributionGroups || [],
    formula_zone_ids: component.formulaZoneIds || [],
    value_resolution: component.valueResolution || null,
    tier_selection_required: component.tierSelectionRequired === true,
    values: (component.values || []).map((value) => ({
      scope: value.scope || null,
      key: value.key || null,
      raw_text: value.rawText || null,
      unit: value.unit || null,
      value: finiteOrNull(value.value),
      decimal_value: finiteOrNull(value.decimalValue),
      formula_amount: value.formulaAmount === true,
      raw_table_value: finiteOrNull(value.rawTableValue),
      tier: finiteOrNull(value.tier),
      tier_kind: value.tierKind || null,
      inferred_from: value.inferredFrom || null,
      source_text: value.sourceText || null,
    })),
  }));
  return {
    key: formula.key,
    name: formula.name || null,
    runtime_kind: formula.runtimeKind || null,
    formula_readiness: formula.formulaReadiness || null,
    value_resolution: formula.valueResolution || null,
    formula_zone_ids: formula.formulaZoneIds || [],
    scope_kinds: formula.scopeKinds || [],
    stack_policy: formula.stackPolicy || null,
    runtime_proof_required: formula.runtimeProofRequired || [],
    source_rule_ids: formula.sourceRuleIds || [],
    relationships: formula.relationships || null,
    components,
  };
}

function summarizeLifecycle(observed) {
  const windows = observed.cross_actor_provider_recipient_windows || {};
  const resolvedPlayerToPlayer = Number(windows.resolved_player_to_player || 0);
  const hasOwnerResolvedSubdivision = Object.hasOwn(
    windows,
    "resolved_external_player_to_player",
  );
  const resolvedSameOwnerPlayerToPlayer = hasOwnerResolvedSubdivision
    ? Number(windows.resolved_same_owner_player_to_player || 0)
    : 0;
  const resolvedExternalPlayerToPlayer = hasOwnerResolvedSubdivision
    ? Number(windows.resolved_external_player_to_player || 0)
    : 0;
  return {
    status_events: Number(observed.status_events || 0),
    window_count: Number(observed.window_count || 0),
    cross_actor_window_count: Number(observed.cross_actor_window_count || 0),
    resolved_player_to_player_windows: resolvedPlayerToPlayer,
    resolved_same_owner_player_to_player_windows: resolvedSameOwnerPlayerToPlayer,
    resolved_external_player_to_player_windows: resolvedExternalPlayerToPlayer,
    owner_resolved_provider_recipient_subdivision_available: hasOwnerResolvedSubdivision,
    resolved_player_to_monster_windows: Number(windows.resolved_player_to_monster || 0),
    unresolved_cross_actor_windows: Number(windows.unresolved_to_player || 0)
      + Number(windows.unresolved_to_monster || 0)
      + Number(windows.unresolved_to_other || 0),
    observed_external_provider_recipient_lifecycle: resolvedExternalPlayerToPlayer > 0,
    applied: Number(observed.applied || 0),
    refreshed: Number(observed.refreshed || 0),
    stacked: Number(observed.stacked || 0),
    consumed: Number(observed.consumed || 0),
    removed: Number(observed.removed || 0),
    minimum_stacks: finiteOrNull(observed.minimum_stacks),
    maximum_stacks: finiteOrNull(observed.maximum_stacks),
    observed_sessions: observed.observed_sessions || [],
  };
}

function externalFormulaProof(formula, runtimeLink) {
  const bridgeMatches = runtimeLink?.formula_matches || [];
  const componentMatches = [];
  for (const component of formula?.components || []) {
    const partyValues = component.values.filter((value) => value.scope === "party");
    const explicitPartyScope = component.contribution_scope === "party"
      || component.value_scope === "party"
      || partyValues.length > 0;
    if (!explicitPartyScope) continue;
    componentMatches.push({ ...component, party_values: partyValues });
  }
  const scalarCandidates = [
    ...bridgeMatches.map((match) => ({
      component_key: match.component_key,
      scope: match.source_scope,
      unit: match.unit,
      decimal_value: finiteOrNull(match.decimal_value),
      proof: runtimeLink.proof_state,
    })),
    ...componentMatches.flatMap((component) => component.party_values.map((value) => ({
      component_key: component.component_key,
      scope: value.scope,
      unit: value.unit,
      decimal_value: value.decimal_value,
      proof: value.inferred_from ? "current_build_description_component" : "current_build_structured_component",
    }))),
  ];
  const explicitExternalScope = bridgeMatches.some((match) => match.source_scope === "party")
    || componentMatches.length > 0;
  const unresolvedScalar = explicitExternalScope && (scalarCandidates.length === 0
    || scalarCandidates.some((candidate) => candidate.decimal_value === null));
  const exactBridgeScalars = bridgeMatches
    .filter((match) => match.source_scope === "party")
    .map((match) => finiteOrNull(match.decimal_value));
  const exactScalarAvailable = runtimeLink?.proof_state === "exact_current_build_formula_match"
    && exactBridgeScalars.length > 0
    && exactBridgeScalars.every((value) => value !== null);
  return {
    explicit_external_scope: explicitExternalScope,
    external_components: componentMatches,
    scalar_candidates: scalarCandidates,
    candidate_scalar_available: explicitExternalScope && !unresolvedScalar,
    exact_scalar_available: exactScalarAvailable,
  };
}

function resolveTransferProof(formula, lifecycle, external) {
  if (external.explicit_external_scope) {
    if (lifecycle.observed_external_provider_recipient_lifecycle && external.exact_scalar_available) {
      return { state: "external_complete", ...external };
    }
    if (lifecycle.observed_external_provider_recipient_lifecycle) {
      return { state: "external_lifecycle_scalar_unresolved", ...external };
    }
    return { state: "external_formula_lifecycle_missing", ...external };
  }
  if (lifecycle.observed_external_provider_recipient_lifecycle) {
    return { state: "external_lifecycle_formula_scope_unresolved", ...external };
  }
  if (!formula) return { state: "no_formula_endpoint", ...external };
  const scopes = new Set([
    ...(formula.scope_kinds || []),
    ...formula.components.flatMap((component) => [
      component.contribution_scope,
      component.value_scope,
      ...component.values.map((value) => value.scope),
    ]),
  ].filter(Boolean));
  const explicitOwnerOnly = scopes.size > 0 && [...scopes].every((scope) => [
    "owner", "self", "produced-damage-row",
  ].includes(scope));
  return { state: explicitOwnerOnly ? "explicit_owner_only" : "scope_unproven", ...external };
}

function queueFor(endpointState, transferState) {
  if (transferState === "external_complete") return "external_formula_lifecycle_and_scalar_proven";
  if (transferState === "external_lifecycle_scalar_unresolved") return "external_lifecycle_scalar_unresolved";
  if (transferState === "external_formula_lifecycle_missing") return "external_formula_lifecycle_missing";
  if (transferState === "external_lifecycle_formula_scope_unresolved") return "external_lifecycle_formula_scope_unresolved";
  if (transferState === "explicit_owner_only") return "explicit_owner_only_no_external_credit";
  if (endpointState === "no_formula_endpoint") return "packet_effect_without_formula_endpoint";
  return "formula_endpoint_scope_unproven";
}

function verifyCatalog(catalog) {
  const effects = catalog.effects || [];
  if (String(catalog.game_build || "") === "") throw new Error("catalog has no game_build");
  if (effects.length === 0) throw new Error("catalog has no effects");
  const ids = effects.map((effect) => integer(effect.effect_id, "catalog effect_id"));
  if (new Set(ids).size !== ids.length) throw new Error("catalog contains duplicate effect IDs");
  if (catalog.summary.observed_effects !== effects.length
      || catalog.summary.reconciled_effects !== effects.length) {
    throw new Error("catalog conservation counts do not match effect rows");
  }
  const queueTotal = Object.values(catalog.summary.proof_queue_counts || {})
    .reduce((total, count) => total + Number(count), 0);
  if (queueTotal !== effects.length) throw new Error(`proof queues conserve ${queueTotal}/${effects.length} effects`);
  for (const effect of effects) {
    if (!effect.proof_queue || !effect.endpoint_resolution?.state || !effect.source_resolution?.state
        || !effect.transfer_proof?.state) {
      throw new Error(`effect ${effect.effect_id} is not fully classified`);
    }
    if (effect.source_resolution.exact_unique_source
        !== (effect.source_resolution.candidates.length === 1)) {
      throw new Error(`effect ${effect.effect_id} has inconsistent source cardinality`);
    }
    const packetRoutes = effect.source_resolution.evidence_routes?.packet_configured_origins || [];
    const exactPerPacketOriginEndpoint = effect.source_resolution.route === "packet_configured_origin"
      && packetRoutes.length > 0
      && packetRoutes.every((route) => route.candidate_source_ids.length === 1
        && (route.unresolved_terminal_ids || []).length === 0
        && route.endpoint_resolution_state === "exact");
    const exactPerPacketOrigin = effect.source_resolution.route === "packet_configured_origin"
      && packetRoutes.length > 0
      && packetRoutes.every((route) => route.candidate_source_ids.length === 1
        && (route.unresolved_terminal_ids || []).length === 0
        && route.owning_source_resolution_state === "exact");
    if (effect.source_resolution.exact_per_packet_origin_endpoint !== exactPerPacketOriginEndpoint) {
      throw new Error(`effect ${effect.effect_id} has inconsistent per-origin endpoint proof state`);
    }
    if (effect.source_resolution.exact_per_packet_origin !== exactPerPacketOrigin) {
      throw new Error(`effect ${effect.effect_id} has inconsistent per-origin proof state`);
    }
    const exactOwningSource = effect.source_resolution.route === "packet_configured_origin"
      ? exactPerPacketOrigin
      : effect.source_resolution.candidates.length === 1
        && candidateHasExactOwner(effect.source_resolution.candidates[0]);
    if (effect.source_resolution.exact_owning_source !== exactOwningSource) {
      throw new Error(`effect ${effect.effect_id} has inconsistent owning-source proof state`);
    }
    const candidateIds = [...new Set(effect.source_resolution.candidates
      .map((candidate) => candidate.source_id))].sort();
    if (JSON.stringify(candidateIds) !== JSON.stringify(effect.source_resolution.candidate_source_ids)) {
      throw new Error(`effect ${effect.effect_id} source IDs do not match summarized candidates`);
    }
  }
}

function countBy(values, keyFn) {
  return Object.fromEntries([...values.reduce((counts, value) => {
    const key = keyFn(value);
    counts.set(key, (counts.get(key) || 0) + 1);
    return counts;
  }, new Map()).entries()].sort(([left], [right]) => left.localeCompare(right)));
}

function parseArgs(args) {
  const result = {};
  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    if (!argument.startsWith("--")) throw new Error(`unexpected argument ${argument}`);
    const key = argument.slice(2);
    if (key === "verify") {
      result[key] = args[++index];
    } else {
      result[key] = args[++index];
    }
    if (!result[key]) throw new Error(`missing value for --${key}`);
  }
  return result;
}

function resolvePath(value) {
  return path.isAbsolute(value) ? value : path.resolve(repositoryRoot, value);
}

function relativePath(value) {
  const relative = path.relative(repositoryRoot, value).replaceAll("\\", "/");
  return relative.startsWith("../") ? path.basename(value) : relative;
}

function readJson(filePath, label) {
  if (!existsSync(filePath)) throw new Error(`${label} not found: ${filePath}`);
  return JSON.parse(readFileSync(filePath, "utf8"));
}

function integer(value, label) {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed <= 0) throw new Error(`${label} must be a positive integer`);
  return parsed;
}

function finiteOrNull(value) {
  if (value === null || value === undefined || value === "") return null;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

function positiveIntegerOrNull(value) {
  if (value === null || value === undefined || value === "") return null;
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) && parsed > 0 ? parsed : null;
}
