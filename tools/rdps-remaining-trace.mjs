#!/usr/bin/env node

import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const options = parseArgs(process.argv.slice(2));
const inputPaths = Object.fromEntries(
  ["readiness", "ledger", "relationships", "componentBridge", "valueProof", "staticWorklist", "effectSources", "packetInventory", "providerAudit", "staticValueProof", "formulaModels", "factorClosure"]
    .map((key) => [key, resolvePath(options[key])]),
);
const outputPath = resolvePath(options.output);
const providerCandidateOutputPath = options.providerCandidateOutput
  ? resolvePath(options.providerCandidateOutput)
  : null;
const readiness = readJson(inputPaths.readiness, "offensive readiness");
const ledger = readJson(inputPaths.ledger, "recipient-scope ledger");
const relationships = readJson(inputPaths.relationships, "modifier relationship table");
const componentBridge = readJson(inputPaths.componentBridge, "current-build component relationship bridge");
const valueProof = readJson(inputPaths.valueProof, "modifier value proof runtime");
const staticWorklist = readJson(inputPaths.staticWorklist, "static rDPS worklist");
const effectSources = readJson(inputPaths.effectSources, "effect sources");
const packetInventory = readJson(inputPaths.packetInventory, "historical packet inventory");
const providerAudit = readJson(inputPaths.providerAudit, "exhaustive historical provider audit");
const staticValueProof = readJson(inputPaths.staticValueProof, "static value proof");
const formulaModels = readJson(inputPaths.formulaModels, "offline formula-model closure catalog");
const factorClosure = readJson(inputPaths.factorClosure, "Psychoscope factor offline closure ledger");

validateInputs();

const ledgerByRule = new Map(ledger.candidates.map((row) => [row.source_rule_id, row]));
const valuesByRule = indexValueProofByRule(valueProof.entriesByKey);
const staticWorklistByRule = new Map(
  [
    ...(staticWorklist.exact_produced_damage_candidates || []),
    ...(staticWorklist.formula_replay_candidates || []),
  ].map((row) => [row.source_rule_id, row]),
);
const packetByEffect = new Map(
  packetInventory.observed_effects.map((row) => [Number(row.effect_id), row]),
);
const packetRelationsBySource = indexRows(
  packetInventory.display_relations,
  (row) => `${Number(row.source_type_id)}:${Number(row.source_config_id)}`,
);
const providerAuditByEffect = indexProviderAudit(providerAudit);
const staticProofByRule = new Map(
  staticValueProof.sources.map((source) => [source.source_rule_id, source]),
);
const formulaModelById = new Map(formulaModels.models.map((model) => [model.model_id, model]));
const unresolved = readiness.candidates.filter((row) => row.state !== "runtime-active");
const traces = unresolved.map(traceCandidate);
const watch = buildWatchlist(traces);

const result = {
  schema_version: 1,
  generated_by: "tools/rdps-remaining-trace.mjs",
  game: "blue-protocol-star-resonance",
  static_game_build: String(readiness.static_game_build),
  historical_packet_build: String(ledger.historical_packet_build),
  policy: {
    exhaustive_offline_trace_before_next_capture: true,
    unresolved_evidence_hidden: false,
    static_relationship_is_not_runtime_attribution_authority: true,
    packet_occurrence_is_not_formula_or_provider_proof: true,
    capture_requested_only_after_offline_obligations_are_exhausted: true,
    capture_gate_is_global_not_subsystem_specific: true,
  },
  inputs: Object.fromEntries(
    Object.entries(inputPaths).map(([key, value]) => [key, relativePath(value)]),
  ),
  summary: summarize(traces, watch),
  final_validation_watchlist: watch,
  candidates: traces,
};

writeFileSync(outputPath, `${JSON.stringify(result, null, 2)}\n`, "utf8");
if (providerCandidateOutputPath) {
  const providerCandidateInventory = {
    schema_version: 1,
    game_build: String(ledger.historical_packet_build),
    static_game_build: String(readiness.static_game_build),
    generated_by: "tools/rdps-remaining-trace.mjs",
    policy: "complete retained-corpus provider and recipient audit for every unresolved offensive rDPS effect; no mechanic is hidden or promoted from occurrence alone",
    candidates: traces.map((trace) => ({
      source_rule_id: trace.source_rule_id,
      name: trace.source_name,
      effect_ids: providerAuditEffectIds(trace),
    })),
  };
  writeFileSync(
    providerCandidateOutputPath,
    `${JSON.stringify(providerCandidateInventory, null, 2)}\n`,
    "utf8",
  );
}

function providerAuditEffectIds(trace) {
  const effectIds = [
    ...trace.declared_effect_ids,
    ...trace.runtime_family_effect_ids,
  ];
  for (const rows of Object.values(trace.relationship?.uid_edges || {})) {
    for (const row of rows) {
      if (row.uid_kind === "buff") effectIds.push(Number(row.uid));
    }
  }
  for (const source of trace.effect_source_relations) {
    effectIds.push(...source.buff_ids, ...source.activation_buff_ids);
  }
  return numbers(effectIds);
}
console.log(JSON.stringify(result.summary));

function traceCandidate(candidate) {
  const ledgerRow = ledgerByRule.get(candidate.source_rule_id) || null;
  const relationship = relationships.sourcesByRuleId?.[candidate.source_rule_id]
    || componentBridge.sourcesByRuleId?.[candidate.source_rule_id]
    || null;
  const relationshipAuthority = relationships.sourcesByRuleId?.[candidate.source_rule_id]
    ? "modifier-relationship-table"
    : componentBridge.sourcesByRuleId?.[candidate.source_rule_id]
      ? "current-build-component-bridge"
      : null;
  const valueRows = [
    ...(valuesByRule.get(candidate.source_rule_id) || []),
    ...exactComponentValueProofRows(staticWorklistByRule.get(candidate.source_rule_id)),
  ];
  const relationshipEdges = relationship?.uidEdges || [];
  const declaredEffects = numbers(candidate.effect_ids);
  const runtimeFamilyEffects = numbers(ledgerRow?.runtime_related_effect_ids);
  const relatedEffects = numbers([...declaredEffects, ...runtimeFamilyEffects]);
  const packetRows = relatedEffects
    .map((effectId) => packetByEffect.get(effectId))
    .filter(Boolean)
    .map(compactPacketRow);
  const sourceRelations = effectSourceRelations(candidate, relatedEffects);
  const structuredEdges = compactEdges(relationshipEdges);
  const providerAuditEffectIds = numbers([
    ...relatedEffects,
    ...relationshipEdges
      .filter((edge) => edge.uidKind === "buff")
      .map((edge) => edge.uid),
    ...sourceRelations.flatMap((source) => [
      ...source.buff_ids,
      ...source.activation_buff_ids,
    ]),
  ]);
  const providerAuditRows = providerAuditEffectIds
    .map((effectId) => providerAuditByEffect.get(effectId))
    .filter(Boolean);
  const providerAuditSummary = summarizeProviderAudit(providerAuditRows);
  const proofGates = candidate.proof_gates;
  const failedGateNames = Object.entries(proofGates.gates || {})
    .filter(([, gate]) => gate.status === "needed")
    .map(([name]) => name);
  const valueState = summarizeValues(valueRows);
  const staticProof = staticProofByRule.get(candidate.source_rule_id) || null;
  const offlineObligations = deriveOfflineObligations({
    relationship,
    valueState,
    sourceRelations,
    packetRows,
    failedGateNames,
    staticProof,
    sourceRuleId: candidate.source_rule_id,
  });
  return {
    source_rule_id: candidate.source_rule_id,
    source_id: candidate.source_id,
    source_name: candidate.source_name,
    readiness_state: candidate.state,
    declared_effect_ids: declaredEffects,
    runtime_family_effect_ids: runtimeFamilyEffects,
    packet_observed_effect_ids: numbers([
      ...packetRows.map((row) => row.effect_id),
      ...providerAuditSummary.observed_effect_ids,
    ]),
    formula_routes: candidate.offensive_routes,
    proof_gates: proofGates,
    relationship: relationship ? {
      source_id: relationship.sourceId,
      authority: relationshipAuthority,
      relationship_status: relationship.relationshipStatus || null,
      talent_ownership: relationship.talentOwnership || null,
      uid_edges: structuredEdges,
      component_routes: relationship.componentRoutes || [],
    } : null,
    value_proof: valueState,
    static_value_proof: staticProof,
    formula_model_closure: formulaModelClosure(candidate.source_rule_id),
    effect_source_relations: sourceRelations,
    historical_packet_occurrence: packetRows,
    historical_provider_recipient_audit: providerAuditSummary,
    offline_obligations: offlineObligations,
    final_validation_obligations: failedGateNames.filter((name) =>
      ["packet_occurrence", "provider_recipient_identity", "lifecycle", "counterfactual_replay", "party_conservation"]
        .includes(name)),
  };
}

function indexProviderAudit(audit) {
  const indexed = new Map();
  for (const report of audit.reports || []) {
    for (const effect of report.effects || []) {
      const effectId = Number(effect.effect_id);
      const row = indexed.get(effectId) || {
        effect_id: effectId,
        reports: 0,
        status_events: 0,
        opened_windows: 0,
        closed_windows: 0,
        cross_actor_windows: 0,
        source_missing_windows: 0,
        player_recipient_windows: 0,
        monster_recipient_windows: 0,
        other_player_overlap_damage_events: 0,
        other_player_overlap_damage_amount: 0,
        provider_resolutions: new Set(),
      };
      row.reports += 1;
      row.status_events += Number(effect.lifecycle?.status_events || 0);
      row.opened_windows += Number(effect.lifecycle?.opened_windows || 0);
      row.closed_windows += Number(effect.lifecycle?.closed_windows || 0);
      row.cross_actor_windows += Number(effect.lifecycle?.cross_actor_windows || 0);
      row.source_missing_windows += Number(effect.lifecycle?.source_missing_windows || 0);
      row.player_recipient_windows += Number(effect.recipient_scope?.player || 0);
      row.monster_recipient_windows += Number(effect.recipient_scope?.monster || 0);
      row.other_player_overlap_damage_events += Number(
        effect.overlap_damage?.monster_incoming_from_other_players?.events || 0,
      );
      row.other_player_overlap_damage_amount += Number(
        effect.overlap_damage?.monster_incoming_from_other_players?.amount || 0,
      );
      for (const provider of effect.providers || []) {
        if (provider.resolution) row.provider_resolutions.add(provider.resolution);
      }
      indexed.set(effectId, row);
    }
  }
  return indexed;
}

function summarizeProviderAudit(rows) {
  const observed = rows.filter((row) => row.status_events > 0);
  return {
    effect_rows: rows.map((row) => ({
      ...row,
      provider_resolutions: [...row.provider_resolutions].sort(),
    })),
    observed_effect_ids: observed.map((row) => row.effect_id),
    status_events: observed.reduce((sum, row) => sum + row.status_events, 0),
    opened_windows: observed.reduce((sum, row) => sum + row.opened_windows, 0),
    cross_actor_windows: observed.reduce((sum, row) => sum + row.cross_actor_windows, 0),
    other_player_overlap_damage_events: observed.reduce(
      (sum, row) => sum + row.other_player_overlap_damage_events,
      0,
    ),
  };
}

function effectSourceRelations(candidate, effectIds) {
  const sourceIds = new Set();
  if (candidate.source_id) sourceIds.add(String(candidate.source_id));
  for (const effectId of effectIds) {
    for (const sourceId of effectSources.buffIdToEffectSourceIds?.[String(effectId)] || []) {
      sourceIds.add(String(sourceId));
    }
  }
  return [...sourceIds]
    .map((sourceId) => effectSources.effectSourcesById?.[sourceId])
    .filter(Boolean)
    .map((source) => ({
      source_id: source.sourceId,
      source_kind: source.sourceKind,
      source_type: source.sourceType,
      source_entity_id: source.sourceEntityId,
      runtime_detection: source.runtimeDetection,
      buff_ids: numbers(source.buffIds),
      activation_buff_ids: numbers(source.activationBuffIds),
      targets: (source.targets || []).map((target) => ({
        target_kind: target.targetKind,
        damage_id: integerOrNull(target.damageId),
        recount_id: integerOrNull(target.recountId),
        relationship_kind: target.relationshipKind,
        produced_output_kind: target.producedOutputKind,
      })),
      formula_attribution: source.formulaAttribution || source.attributionModel || null,
      packet_display_relations: (packetRelationsBySource.get(
        `${source.sourceType === "buff" ? 1 : 0}:${Number(source.sourceEntityId)}`,
      ) || []).map((row) => ({
        effect_id: Number(row.effect_id),
        observation_count: Number(row.observation_count || 0),
        display_resolution: row.direct_buff_display_resolution,
      })),
    }));
}

function compactEdges(edges) {
  const byKind = new Map();
  for (const edge of edges) {
    const rows = byKind.get(edge.edgeKind) || [];
    rows.push({
      uid_kind: edge.uidKind,
      uid: edge.uid,
      role: edge.role,
      source: edge.source,
      source_table: edge.sourceTable,
      scope: edge.scope,
      component_key: edge.componentKey,
      component_class: edge.componentClass,
      direction: edge.direction,
      contribution_scope: edge.contributionScope,
      transfer_eligibility: edge.transferEligibility,
      formula_term_ids: edge.formulaTermIds,
      predicate_tags: edge.predicateTags,
      relationship_kind: edge.relationshipKind,
      status: edge.status,
    });
    byKind.set(edge.edgeKind, rows);
  }
  return Object.fromEntries([...byKind.entries()].sort(([left], [right]) => left.localeCompare(right)));
}

function summarizeValues(rows) {
  const selectedValues = dedupe(rows.flatMap((row) => row.selectedValues || []));
  const selectors = dedupe(rows.flatMap((row) => row.valueSelectors || []));
  const blockers = [...new Set(rows.flatMap((row) => row.valueBlockers || []))].sort();
  const requirements = [...new Set(rows.flatMap((row) => row.proofRequirements || []))].sort();
  return {
    entries: rows.map((row) => ({
      key: row.key,
      uid: row.uid,
      category: row.category,
      runtime_kind: row.runtimeKind,
      formula_readiness: row.formulaReadiness,
      value_proof_status: row.valueProofStatus,
      formula_zone_ids: row.formulaZoneIds,
    })),
    selected_values: selectedValues,
    selectors,
    blockers,
    proof_requirements: requirements,
    exact_selected_value_count: selectedValues.filter(
      (row) => Number.isFinite(row.value) && row.unit,
    ).length,
  };
}

function exactComponentValueProofRows(worklistRow) {
  return (worklistRow?.value_proofs || [])
    .filter((proof) => proof.value_proof_status === "exact-bound-relationship-component")
    .filter((proof) => Number(proof.selected_value_count || 0) > 0)
    .filter((proof) => (proof.blockers || []).length === 0)
    .filter((proof) => (proof.selected_values || []).every(
      (value) => Number.isFinite(Number(value.value)) && Boolean(value.unit),
    ))
    .map((proof) => ({
      key: proof.key,
      uid: null,
      category: "relationship-component",
      runtimeKind: "component-bound-static-proof",
      formulaReadiness: proof.formula_readiness,
      valueProofStatus: proof.value_proof_status,
      formulaZoneIds: worklistRow.formula_zone_ids || [],
      selectedValues: proof.selected_values || [],
      valueSelectors: proof.value_selectors || [],
      valueBlockers: proof.value_blockers || [],
      proofRequirements: proof.proof_requirements || [],
    }));
}

function deriveOfflineObligations(context) {
  const obligations = [];
  if (!context.relationship) obligations.push("join-source-to-current-build-relationship-table");
  const hasStaticMagnitude = ["exact-formula", "complete-ladder"]
    .includes(context.staticProof?.static_value_status);
  if (context.valueState.entries.length === 0 && !hasStaticMagnitude) {
    obligations.push("join-source-to-current-build-value-proof-rows");
  }
  if (context.valueState.selected_values.length === 0
    && !["exact-formula", "complete-ladder"].includes(context.staticProof?.static_value_status)) {
    obligations.push("derive-exact-component-value-and-unit");
  }
  const modelClosure = formulaModelClosure(context.sourceRuleId);
  if (modelClosure.route_missing) obligations.push("route-source-through-offline-formula-model-catalog");
  for (const model of modelClosure.models) {
    if (model.state !== "offline-formula-complete") {
      obligations.push(`prove-formula-model:${model.model_id}:${model.missing_proof || model.state}`);
    }
  }
  if (!hasStaticMagnitude && context.valueState.blockers.length > 0) {
    obligations.push(...context.valueState.blockers.filter(
      (blocker) => !valueBlockerCoveredByFormulaRoute(blocker, modelClosure.models),
    ));
  }
  if (context.sourceRelations.length === 0 && !context.relationship) {
    obligations.push("trace-source-through-effect-source-index-or-current-component-bridge");
  }
  return [...new Set(obligations)].sort();
}

function valueBlockerCoveredByFormulaRoute(blocker, models) {
  const modelIds = new Set(models.map((model) => model.model_id));
  if (String(blocker).endsWith(":stat-conversion-model-required")) {
    return modelIds.has("packet-attribute-family-transform")
      && modelIds.has("primary-stat-to-attack-transform")
      && modelIds.has("combined-provider-stage-conservation");
  }
  if (String(blocker).endsWith(":target-window-proof-required")) {
    return modelIds.has("fixed-point-multiplier-counterfactual")
      && modelIds.has("nonstacking-provider-arbitration")
      && modelIds.has("combined-provider-stage-conservation");
  }
  return false;
}

function formulaModelClosure(sourceRuleId) {
  const route = formulaModels.source_model_routes?.[sourceRuleId] || [];
  return {
    route_missing: route.length === 0,
    models: route.map((modelId) => formulaModelById.get(modelId) || {
      model_id: modelId,
      state: "catalog-entry-missing",
      missing_proof: "referenced model is absent from catalog",
    }),
  };
}

function buildWatchlist(traces) {
  const effects = new Set();
  const skills = new Set();
  const damage = new Set();
  const recount = new Set();
  const attributes = new Set();
  for (const trace of traces) {
    for (const value of [...trace.declared_effect_ids, ...trace.runtime_family_effect_ids]) effects.add(value);
    for (const [kind, rows] of Object.entries(trace.relationship?.uid_edges || {})) {
      for (const row of rows) {
        if (row.uid_kind === "buff") effects.add(Number(row.uid));
        else if (row.uid_kind === "skill") skills.add(Number(row.uid));
        else if (row.uid_kind === "damage") damage.add(Number(row.uid));
        else if (row.uid_kind === "recount") recount.add(Number(row.uid));
        else if (row.uid_kind === "attribute") attributes.add(Number(row.uid));
      }
    }
    for (const source of trace.effect_source_relations) {
      for (const target of source.targets) {
        if (target.damage_id) damage.add(target.damage_id);
        if (target.recount_id) recount.add(target.recount_id);
      }
    }
  }
  return {
    effect_ids: sortedNumbers(effects),
    skill_ids: sortedNumbers(skills),
    damage_ids: sortedNumbers(damage),
    recount_ids: sortedNumbers(recount),
    attribute_ids: sortedNumbers(attributes),
    required_canonical_fields: [
      "event timestamp and packet sequence",
      "effect lifecycle operation, effect id, layer, count, source actor, recipient actor",
      "damage id, skill id, source actor, target actor, amount, crit/lucky flags",
      "provider and recipient combat attributes at the event boundary",
      "selected talent, imagine, factor, equipment-set, and role-slot identities",
      "dungeon and segment boundaries",
    ],
  };
}

function summarize(traces, watch) {
  return {
    unresolved_sources: traces.length,
    packet_observed_sources: traces.filter((row) => row.packet_observed_effect_ids.length > 0).length,
    sources_without_current_relationship_join: traces.filter((row) => !row.relationship).length,
    sources_without_value_proof_rows: traces.filter((row) => row.value_proof.entries.length === 0).length,
    sources_without_exact_selected_values: traces.filter((row) => row.value_proof.exact_selected_value_count === 0).length,
    sources_without_static_formula_or_ladder: traces.filter((row) =>
      !["exact-formula", "complete-ladder"].includes(row.static_value_proof?.static_value_status),
    ).length,
    sources_with_static_formula_but_runtime_selector_needed: traces.filter((row) =>
      row.static_value_proof?.remaining_runtime_selector,
    ).length,
    sources_with_static_value_blockers: traces.filter((row) => row.value_proof.blockers.length > 0).length,
    sources_with_offline_obligations: traces.filter((row) => row.offline_obligations.length > 0).length,
    offline_obligations: traces.reduce((sum, row) => sum + row.offline_obligations.length, 0),
    offline_formula_models_needed: formulaModels.models.filter((model) =>
      model.state !== "offline-formula-complete",
    ).length,
    psychoscope_factor_families: Number(factorClosure.summary.factor_families || 0),
    psychoscope_offline_route_obligations: Number(factorClosure.summary.total_offline_route_obligations || 0),
    global_known_offline_obligations: traces.reduce((sum, row) => sum + row.offline_obligations.length, 0)
      + formulaModels.models.filter((model) => model.state !== "offline-formula-complete").length
      + Number(factorClosure.summary.total_offline_route_obligations || 0),
    failed_proof_gates: traces.reduce((sum, row) => sum + row.proof_gates.failed_gate_count, 0),
    watch_effect_ids: watch.effect_ids.length,
    watch_skill_ids: watch.skill_ids.length,
    watch_damage_ids: watch.damage_ids.length,
    watch_recount_ids: watch.recount_ids.length,
    watch_attribute_ids: watch.attribute_ids.length,
  };
}

function compactPacketRow(row) {
  return {
    effect_id: Number(row.effect_id),
    status_events: Number(row.status_events || 0),
    packet_origin_observations: Number(row.packet_origin_observations || 0),
    source_relation_count: Number(row.source_relation_count || 0),
    buff_table_resolution: row.buff_table_resolution,
  };
}

function indexValueProofByRule(entries) {
  const result = new Map();
  for (const row of Object.values(entries || {})) {
    for (const ruleId of [...(row.sourceRuleIds || []), ...(row.directSourceRuleIds || [])]) {
      const rows = result.get(ruleId) || [];
      if (!rows.some((existing) => existing.key === row.key)) rows.push(row);
      result.set(ruleId, rows);
    }
  }
  return result;
}

function indexRows(rows, keyFn) {
  const result = new Map();
  for (const row of rows || []) {
    const key = keyFn(row);
    const values = result.get(key) || [];
    values.push(row);
    result.set(key, values);
  }
  return result;
}

function validateInputs() {
  if (!Array.isArray(readiness.candidates) || !Array.isArray(ledger.candidates)) {
    throw new Error("readiness and ledger must contain candidate arrays");
  }
  if (!relationships.sourcesByRuleId || !valueProof.entriesByKey || !effectSources.effectSourcesById) {
    throw new Error("current-build relationship, value-proof, or effect-source indexes are missing");
  }
  if ((!Array.isArray(staticWorklist.exact_produced_damage_candidates)
      && !Array.isArray(staticWorklist.formula_replay_candidates))
    || String(staticWorklist.game_build) !== String(ledger.static_game_build)) {
    throw new Error("static rDPS worklist is missing or describes a different current game build");
  }
  if (!Array.isArray(packetInventory.observed_effects)) {
    throw new Error("packet inventory does not contain observed_effects");
  }
  if (String(readiness.static_game_build) !== String(ledger.static_game_build)) {
    throw new Error("readiness and recipient ledger builds differ");
  }
  if (!Array.isArray(staticValueProof.sources)
    || String(staticValueProof.static_game_build) !== String(ledger.static_game_build)) {
    throw new Error("static value proof is missing or describes a different current game build");
  }
  if (!Array.isArray(formulaModels.models)
    || String(formulaModels.game_build) !== String(ledger.static_game_build)) {
    throw new Error("offline formula-model closure catalog is missing or describes a different current game build");
  }
  if (!Array.isArray(factorClosure.families)
    || String(factorClosure.game_build) !== String(ledger.static_game_build)) {
    throw new Error("Psychoscope factor closure ledger is missing or describes a different current game build");
  }
  for (const candidate of readiness.candidates.filter((row) => row.state !== "runtime-active")) {
    if (!candidate.proof_gates) throw new Error(`unresolved source ${candidate.source_name} lacks proof gates`);
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
  for (const key of ["readiness", "ledger", "relationships", "valueProof", "staticWorklist", "effectSources", "packetInventory", "providerAudit", "staticValueProof", "formulaModels", "factorClosure", "output"]) {
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

function numbers(values = []) {
  return [...new Set(values.map(Number).filter((value) => Number.isSafeInteger(value) && value > 0))]
    .sort((left, right) => left - right);
}

function sortedNumbers(values) {
  return [...values].filter(Number.isSafeInteger).sort((left, right) => left - right);
}

function integerOrNull(value) {
  const number = Number(value);
  return Number.isSafeInteger(number) && number > 0 ? number : null;
}

function dedupe(values) {
  return [...new Map(values.map((value) => [JSON.stringify(value), value])).values()];
}
