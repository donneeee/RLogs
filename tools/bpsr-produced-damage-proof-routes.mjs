#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";

const DEPENDENCY = "produced-damage-without-packet-row";
const MAX_DEPTH = 7;
const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "build") build(resolveContext(options));
else if (command === "verify") verify(
  path.resolve(required(options, "input")),
  options.index ? path.resolve(options.index) : null,
  options["activation-index"] ? path.resolve(options["activation-index"]) : null,
);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function resolveContext(parsed) {
  const buildId = required(parsed, "build");
  if (!/^\d+$/.test(buildId)) throw new Error("Build must contain only ASCII digits");
  return {
    build: buildId,
    index: path.resolve(required(parsed, "index")),
    semanticClosure: path.resolve(required(parsed, "semantic-closure")),
    effectSources: path.resolve(required(parsed, "effect-sources")),
    battleImagines: path.resolve(required(parsed, "battle-imagines")),
    originCatalog: path.resolve(required(parsed, "origin-catalog")),
    recount: path.resolve(required(parsed, "recount")),
    activationIndex: path.resolve(required(parsed, "activation-index")),
    numberSemantics: path.resolve(required(parsed, "number-semantics")),
    output: path.resolve(required(parsed, "output")),
  };
}

function build(context) {
  const started = performance.now();
  for (const [label, file] of [
    ["semantic evidence index", context.index],
    ["semantic dependency closure", context.semanticClosure],
    ["effect sources", context.effectSources],
    ["battle Imagine descriptions", context.battleImagines],
    ["runtime effect origin catalog", context.originCatalog],
    ["exact recount table", context.recount],
    ["packet activation index", context.activationIndex],
    ["number-format semantics", context.numberSemantics],
  ]) requireFile(file, label);

  const closure = readJson(context.semanticClosure, "semantic dependency closure");
  const effectSources = readJson(context.effectSources, "effect sources");
  const battleImagines = readJson(context.battleImagines, "battle Imagine descriptions");
  const originCatalog = readJson(context.originCatalog, "runtime effect origin catalog");
  const recount = readJson(context.recount, "exact recount table");
  const activationIndex = readJson(context.activationIndex, "packet activation index");
  const numberSemantics = readJson(context.numberSemantics, "number-format semantics");
  requireBuild(closure, context.build, "semantic dependency closure", "game_build");
  requireBuild(activationIndex, context.build, "packet activation index", "game_build");
  requireBuild(activationIndex, context.build, "packet activation index", "packet_build");
  requireBuild(originCatalog, context.build, "runtime effect origin catalog", "game_build");
  requireBuild(numberSemantics, context.build, "number-format semantics", "game_build");
  if (activationIndex.summary?.unresolved_evidence_hidden !== false) {
    throw new Error("Packet activation index must explicitly preserve unresolved evidence");
  }

  const db = new DatabaseSync(context.index, { readOnly: true });
  let report;
  try {
    const metadata = readMetadata(db);
    if (metadata.game_build !== context.build) throw new Error(`Evidence index build ${metadata.game_build} does not match ${context.build}`);
    report = generateReport({
      build: context.build,
      closure,
      effectSources,
      battleImagines,
      originCatalog,
      recount,
      activationIndex,
      numberSemantics,
      db,
      inputs: {
        semantic_evidence_index: fileDescriptor(context.index),
        semantic_dependency_closure: fileDescriptor(context.semanticClosure),
        effect_sources: fileDescriptor(context.effectSources),
        battle_imagine_descriptions: fileDescriptor(context.battleImagines),
        runtime_effect_origin_catalog: fileDescriptor(context.originCatalog),
        exact_recount_table: fileDescriptor(context.recount),
        packet_activation_index: fileDescriptor(context.activationIndex),
        number_format_semantics: fileDescriptor(context.numberSemantics),
      },
    });
  } finally {
    db.close();
  }

  mkdirSync(path.dirname(context.output), { recursive: true });
  writeFileSync(context.output, `${JSON.stringify(report, null, 2)}\n`);
  verify(context.output, context.index, context.activationIndex);
  console.log(
    `Produced-damage proof routes built for ${context.build}: ${report.summary.blocked_sources} blocked sources, ` +
    `${report.summary.exact_routes} exact routes, ${report.summary.candidate_only} candidate-only, ` +
    `${report.summary.no_candidate} without a candidate in ${Math.round(performance.now() - started)} ms.`,
  );
}

function generateReport({ build, closure, effectSources, battleImagines, originCatalog, recount, activationIndex, numberSemantics, db, inputs }) {
  const blockers = (closure.mechanics ?? [])
    .filter((mechanic) => (mechanic.unresolved_dependencies ?? []).some((dependency) => dependency.kind === DEPENDENCY))
    .sort((left, right) => compareText(left.source_rule_id, right.source_rule_id));
  const sourceMap = effectSources.effectSourcesById ?? {};
  const recountRows = Object.values(recount).filter((row) => row && typeof row === "object");
  const recountIndex = buildRecountIndex(recountRows);
  const queries = createQueries(db);
  const selectorOutputRoutes = deriveSelectorOutputRoutes(battleImagines, originCatalog, sourceMap, activationIndex, numberSemantics, queries);

  const routes = blockers.map((mechanic) => {
    const source = sourceMap[mechanic.source_id] ?? null;
    const terms = extractProducedTerms(source, mechanic);
    const rejectedLookalikes = [];
    const candidateRoutes = findNameCandidates(terms, recountRows, recountIndex, rejectedLookalikes);
    const exactPaths = findExactPaths(mechanic, queries);
    const explicitRoutes = extractExplicitValidatedRoutes(source, recountIndex.byId);
    const exactTargetIds = new Set([
      ...exactPaths.map((entry) => entry.target_damage_id),
      ...explicitRoutes.flatMap((entry) => entry.damage_ids),
    ]);
    const exactRoutes = mergeExactRoutes(exactPaths, explicitRoutes, recountRows, exactTargetIds);
    const exactDamageIds = [...new Set(exactRoutes.flatMap((route) => route.damage_ids))].sort(compareIdentifiers);
    const exactPacketActivation = packetActivationEvidence(exactDamageIds, activationIndex);
    const enrichedCandidateRoutes = candidateRoutes.map((candidate) => ({
      ...candidate,
      packet_activation: packetActivationEvidence(candidate.damage_ids, activationIndex),
    }));
    const promotionEligible = exactRoutes.length > 0;
    const proofState = promotionEligible
      ? "current-build-exact-route"
      : candidateRoutes.length > 1
        ? "ambiguous-candidate"
        : candidateRoutes.length === 1
          ? "candidate-only"
          : "no-candidate";
    const missingEdges = promotionEligible ? [] : buildMissingEdges(mechanic, enrichedCandidateRoutes);
    const attributionState = deriveAttributionState(promotionEligible, exactPacketActivation, enrichedCandidateRoutes);
    return {
      source_rule_id: mechanic.source_rule_id,
      source_id: mechanic.source_id,
      source_name: mechanic.source_name,
      source_kind: mechanic.source_kind,
      seed_ids: [...new Set((mechanic.seeds ?? []).map((seed) => String(seed.id)))].sort(compareIdentifiers),
      produced_terms: terms,
      proof_state: proofState,
      promotion_eligible: promotionEligible,
      exact_routes: exactRoutes,
      exact_graph_paths: exactPaths,
      packet_activation: exactPacketActivation,
      candidate_routes: enrichedCandidateRoutes,
      rejected_lookalikes: rejectedLookalikes,
      missing_edges: missingEdges,
      attribution_state: attributionState,
      archived_recalculation_ready: false,
      deferred_attribution_preserved: true,
      next_proof_action: nextProofAction(proofState, mechanic, enrichedCandidateRoutes, attributionState),
    };
  });

  const states = countValues(routes.map((route) => route.proof_state));
  const obligations = groupObligations(routes);
  const report = {
    schema_version: 1,
    generated_by: "tools/bpsr-produced-damage-proof-routes.mjs",
    game_build: build,
    policy: {
      no_guessing: true,
      no_unresolved_evidence_hidden: true,
      exact_whole_name_matching_only: true,
      substring_matches_are_rejected_lookalikes: true,
      names_and_formulas_are_candidate_evidence_only: true,
      static_exact_graph_or_validated_route_required_for_promotion: true,
      packet_proof_still_required_for_runtime_activation_and_scope: true,
      packet_observation_does_not_prove_source_ownership: true,
      unobserved_definitions_remain_explicit_evidence_gaps: true,
      deferred_attribution_can_be_recalculated_from_archived_events: true,
      raw_evidence_is_immutable_and_derived_attribution_is_versioned: true,
      selector_output_identity_does_not_resolve_adjacent_source_aliases: true,
      selector_output_identity_does_not_prove_provider_recipient_stacking_or_formula: true,
    },
    inputs,
    summary: {
      blocked_sources: routes.length,
      exact_routes: routes.filter((route) => route.promotion_eligible).length,
      candidate_only: states["candidate-only"] ?? 0,
      ambiguous_candidates: states["ambiguous-candidate"] ?? 0,
      no_candidate: states["no-candidate"] ?? 0,
      rejected_lookalikes: routes.reduce((sum, route) => sum + route.rejected_lookalikes.length, 0),
      missing_edges: routes.reduce((sum, route) => sum + route.missing_edges.length, 0),
      exact_route_packet_observed: routes.filter((route) => route.promotion_eligible && route.packet_activation.observed).length,
      exact_route_not_observed_in_current_corpus: routes.filter((route) => route.promotion_eligible && !route.packet_activation.observed).length,
      packet_observed_quarantined_candidates: routes.filter((route) => !route.promotion_eligible && route.candidate_routes.some((candidate) => candidate.packet_activation.observed)).length,
      deferred_attribution_sources: routes.filter((route) => route.deferred_attribution_preserved && !route.archived_recalculation_ready).length,
      archived_recalculation_ready: routes.filter((route) => route.archived_recalculation_ready).length,
      exact_selector_definitions: selectorOutputRoutes.length,
      selector_output_identity_proven: selectorOutputRoutes.filter((route) => route.output_identity_proven).length,
      selector_output_packet_observed: selectorOutputRoutes.filter((route) => route.packet_emission_observed).length,
      unresolved_selector_output_routes: selectorOutputRoutes.filter((route) => !route.output_identity_proven).length,
      selector_deferred_attribution_sources: selectorOutputRoutes.filter((route) => route.deferred_attribution_preserved && !route.archived_recalculation_ready).length,
      irreducible_obligation_groups: obligations.length,
      zero_hidden_omissions: true,
    },
    irreducible_obligations: obligations,
    selector_output_routes: selectorOutputRoutes,
    routes,
  };
  report.content_sha256 = contentHash(report);
  return report;
}

function deriveSelectorOutputRoutes(battleImagines, originCatalog, sourceMap, activationIndex, numberSemantics, queries) {
  const trustedEvidence = "SkillAoyi transformation kind 3 selects this buff and its 1-based BuffPar parameter set";
  const definitions = new Map();
  for (const entry of Object.values(battleImagines.entriesByUid ?? {})) {
    for (const record of entry.decisionParameterRecords ?? []) {
      if (record.evidence !== trustedEvidence || String(record.ownerSkillId) !== String(entry.uid)) continue;
      const key = `${record.ownerSkillId}:${record.buffId}`;
      if (!definitions.has(key)) definitions.set(key, {
        owner_skill_id: String(record.ownerSkillId),
        owner_skill_name: entry.name ?? null,
        selector_effect_id: String(record.buffId),
        description_scope_evidence: deriveDescriptionScopeEvidence(entry),
        selector_definition_records: [],
      });
      definitions.get(key).selector_definition_records.push({
        tier: record.tier,
        parameter_set_index: record.parameterSetIndex,
        parameter_values: record.parameterValues ?? [],
        source_table: record.sourceTable,
        source_offset: record.sourceOffset,
        row_id: record.rowId,
        evidence: record.evidence,
      });
    }
  }

  const relations = (originCatalog.relations ?? []).filter((relation) =>
    relation.source_type_id === 1 && relation.source_config_id !== undefined && relation.effect_id !== undefined
  );
  return [...definitions.values()].map((definition) => {
    const emissions = relations
      .filter((relation) => String(relation.source_config_id) === definition.selector_effect_id)
      .map((relation) => ({
        emitted_effect_id: String(relation.effect_id),
        observation_count: relation.observation_count ?? 0,
        observed_sessions: [...new Set(relation.observed_sessions ?? [])].sort(compareText),
        configured_source_table: relation.configured_source_table ?? null,
        source_kind: relation.source_kind ?? null,
      }))
      .sort((left, right) => compareIdentifiers(left.emitted_effect_id, right.emitted_effect_id));
    const outputs = emissions.flatMap((emission) => {
      const source = sourceMap[`buff-source:${emission.emitted_effect_id}`];
      const exactTargets = (source?.targets ?? []).filter((target) =>
        target.targetKind === "damage" && target.evidenceStatus === "current-build-direct-damage-attr-buff-link"
      );
      const damageIds = [...new Set(exactTargets.map((target) => String(target.damageId)))].sort(compareIdentifiers);
      const recountIds = [...new Set(exactTargets.map((target) => target.parentRecountId)
        .filter((value) => value !== undefined && value !== null).map(String))].sort(compareIdentifiers);
      return damageIds.length === 0 ? [] : [{
        emitted_effect_id: emission.emitted_effect_id,
        damage_ids: damageIds,
        recount_ids: recountIds,
        proof: "current-build-selector-emission-plus-direct-damage-attr-buff-link",
        packet_activation: packetActivationEvidence(damageIds, activationIndex),
      }];
    });
    const packetEmissionObserved = emissions.some((emission) => emission.observation_count > 0);
    const formulaFamily = deriveSelectorFormulaFamily(definition, emissions, originCatalog, numberSemantics, queries);
    const selfOnlyStatusProven = packetEmissionObserved
      && definition.description_scope_evidence?.scope === "self-only"
      && definition.description_scope_evidence?.proof_state === "current-build-multilocale-explicit-self-scope";
    const outputKind = outputs.length > 0
      ? "direct-produced-damage"
      : selfOnlyStatusProven ? "self-owned-source-modifier" : "unresolved";
    const outputIdentityProven = outputs.length > 0 || selfOnlyStatusProven;
    return {
      ...definition,
      selector_definition_records: definition.selector_definition_records.sort((left, right) => Number(left.tier) - Number(right.tier)),
      packet_emission_observed: packetEmissionObserved,
      emitted_effects: emissions,
      exact_outputs: outputs,
      output_kind: outputKind,
      proof_state: outputs.length > 0
        ? "current-build-exact-selector-emission-output-route"
        : selfOnlyStatusProven
          ? "current-build-exact-selector-emission-self-only-status-route"
        : packetEmissionObserved ? "emitted-effect-awaiting-exact-output-edge" : "exact-selector-awaiting-packet-emission",
      output_identity_proven: outputIdentityProven,
      transfer_scope_proven: selfOnlyStatusProven,
      transfer_scope: selfOnlyStatusProven ? "self-only" : "unresolved",
      rdps_transfer_eligible: selfOnlyStatusProven ? false : null,
      formula_family_evidence: formulaFamily,
      formula_family_proven: formulaFamily.formula_family_proven,
      formula_identity_proven: formulaFamily.formula_family_proven,
      tier_value_ladder_proven: formulaFamily.tier_value_ladder_proven,
      selected_runtime_tier_proven: false,
      normalization_semantics_proven: formulaFamily.decision_unmarkpercent_normalization_proven,
      stack_limit_proven: formulaFamily.stack_limit_proven,
      numeric_formula_ready: false,
      source_aliases_resolved: false,
      provider_recipient_stacking_formula_proven: false,
      archived_recalculation_ready: false,
      deferred_attribution_preserved: true,
      next_proof_action: outputs.length > 0
        ? "Prove provider, recipient, stacking, and coefficient semantics before enabling attribution; do not resolve adjacent aliases from this route."
        : selfOnlyStatusProven
          ? formulaFamily.formula_family_proven
            ? formulaFamily.decision_unmarkpercent_normalization_proven
              ? "Keep this source excluded from transferable rDPS. Prove the selected runtime tier and stack-at-hit state before numeric recalculation."
              : "Keep this source excluded from transferable rDPS. Prove the selected runtime tier, Decision.unmarkpercent normalization, and stack-at-hit state before numeric recalculation."
            : "Keep this source excluded from transferable rDPS. Prove its selector description, emitted buff definition, tier parameter ladder, and stack semantics before numeric recalculation."
        : packetEmissionObserved
          ? "Trace the emitted effect to its exact DamageAttrTable output without using names or numeric-family inference."
          : "Observe this exact selector in a matching-build packet capture, then trace the emitted effect to its exact output rows.",
    };
  }).sort((left, right) => compareIdentifiers(left.owner_skill_id, right.owner_skill_id) || compareIdentifiers(left.selector_effect_id, right.selector_effect_id));
}

function deriveSelectorFormulaFamily(definition, emissions, originCatalog, numberSemantics, queries) {
  const selectorRow = readDecodedRow(queries, "BuffTable", definition.selector_effect_id);
  const descriptionId = selectorRow?.TipsDescription === undefined || selectorRow?.TipsDescription === null
    ? null : String(selectorRow.TipsDescription);
  const descriptionRow = descriptionId ? readDecodedRow(queries, "AttrDescription", descriptionId) : null;
  const description = typeof descriptionRow?.Description === "string" ? descriptionRow.Description : "";
  const parameterIndices = [...new Set([...description.matchAll(/Decision\.unmarkpercent\((\d+)\)/g)]
    .map((match) => Number(match[1])).filter((value) => Number.isInteger(value) && value > 0))].sort((a, b) => a - b);
  const records = definition.selector_definition_records ?? [];
  const tierValueLadder = records.map((record) => ({
    tier: record.tier,
    parameter_set_index: record.parameter_set_index,
    values_by_description_parameter: Object.fromEntries(parameterIndices.map((index) => [String(index), record.parameter_values?.[index - 1] ?? null])),
    all_description_parameters_present: parameterIndices.every((index) => record.parameter_values?.[index - 1] !== undefined),
  }));
  const emittedDefinitions = emissions.map((emission) => {
    const row = readDecodedRow(queries, "BuffTable", emission.emitted_effect_id);
    return {
      emitted_effect_id: emission.emitted_effect_id,
      definition_found: Boolean(row),
      name: row?.Name ?? null,
      description: row?.Desc ?? null,
      repeat_add_rule: Array.isArray(row?.RepeatAddRule) ? row.RepeatAddRule : [],
      static_maximum_stacks: Array.isArray(row?.RepeatAddRule) && row.RepeatAddRule.length > 1 ? row.RepeatAddRule[1] : null,
      destroy_param: Array.isArray(row?.DestroyParam) ? row.DestroyParam : [],
      static_lifetime_seconds: extractStaticLifetimeSeconds(row?.DestroyParam),
    };
  });
  const effectIndex = new Map((originCatalog.effects ?? []).map((effect) => [String(effect.effect_id), effect]));
  const packetLifecycles = emissions.map((emission) => {
    const effect = effectIndex.get(emission.emitted_effect_id) ?? null;
    return {
      emitted_effect_id: emission.emitted_effect_id,
      packet_observed: Boolean(effect && Number(effect.status_events ?? 0) > 0),
      status_events: effect?.status_events ?? 0,
      window_count: effect?.window_count ?? 0,
      cross_actor_window_count: effect?.cross_actor_window_count ?? 0,
      source_missing_window_count: effect?.source_missing_window_count ?? 0,
      minimum_stacks: effect?.minimum_stacks ?? null,
      maximum_stacks: effect?.maximum_stacks ?? null,
      applied: effect?.applied ?? 0,
      refreshed: effect?.refreshed ?? 0,
      stacked: effect?.stacked ?? 0,
      consumed: effect?.consumed ?? 0,
      removed: effect?.removed ?? 0,
      observed_sessions: [...new Set(effect?.observed_sessions ?? [])].sort(compareText),
    };
  });
  const tierValueLadderProven = parameterIndices.length > 0 && records.length > 0
    && tierValueLadder.every((record) => record.all_description_parameters_present);
  const formulaFamilyProven = Boolean(selectorRow)
    && descriptionId === definition.selector_effect_id
    && Boolean(descriptionRow)
    && tierValueLadderProven
    && emissions.length > 0
    && emittedDefinitions.every((entry) => entry.definition_found);
  const staticStackLimits = [...new Set(emittedDefinitions.map((entry) => entry.static_maximum_stacks)
    .filter((value) => Number.isFinite(Number(value))).map(Number))];
  const observedStackLimits = [...new Set(packetLifecycles.filter((entry) => entry.packet_observed)
    .map((entry) => entry.maximum_stacks).filter((value) => Number.isFinite(Number(value))).map(Number))];
  const stackLimitProven = staticStackLimits.length === 1 && observedStackLimits.length > 0
    && observedStackLimits.every((value) => value === staticStackLimits[0]);
  const normalization = numberSemantics?.normalization ?? {};
  const decisionUnmarkpercentNormalizationProven = parameterIndices.length > 0
    && numberSemantics?.game_build !== undefined
    && normalization.proof_state === "current-build-exact-client-code"
    && normalization.semantics_proven === true
    && normalization.raw_to_display_percent_divisor === 100
    && normalization.raw_to_fractional_ratio_divisor === 10000;
  return {
    formula_family_proven: formulaFamilyProven,
    selector_definition_found: Boolean(selectorRow),
    selector_description_id: descriptionId,
    selector_description_found: Boolean(descriptionRow),
    selector_description: description || null,
    description_parameter_indices: parameterIndices,
    tier_value_ladder_proven: tierValueLadderProven,
    tier_value_ladder: tierValueLadder,
    emitted_buff_definitions: emittedDefinitions,
    packet_lifecycles: packetLifecycles,
    stack_limit_proven: stackLimitProven,
    static_stack_limit: staticStackLimits.length === 1 ? staticStackLimits[0] : null,
    packet_stack_limits: observedStackLimits,
    static_lifetime_is_definition_only: true,
    selected_runtime_tier_proven: false,
    decision_unmarkpercent_normalization_proven: decisionUnmarkpercentNormalizationProven,
    normalization_evidence: decisionUnmarkpercentNormalizationProven ? {
      proof_state: normalization.proof_state,
      proof_scope: normalization.proof_scope,
      raw_to_display_percent_divisor: normalization.raw_to_display_percent_divisor,
      raw_to_fractional_ratio_divisor: normalization.raw_to_fractional_ratio_divisor,
      runtime_formula_order_proven: normalization.runtime_formula_order_proven === true,
      client_bytecode_sha256: numberSemantics?.source?.bytecode_sha256 ?? null,
      decompiled_source_sha256: numberSemantics?.source?.decompiled_source_sha256 ?? null,
    } : null,
    stack_at_damage_event_proven: false,
    numeric_formula_ready: false,
  };
}

function extractStaticLifetimeSeconds(destroyParam) {
  if (!Array.isArray(destroyParam)) return null;
  const values = destroyParam.filter((entry) => Array.isArray(entry) && Number(entry[0]) === 0 && Number.isFinite(Number(entry[1])))
    .map((entry) => Number(entry[1]));
  return values.length === 1 ? values[0] : null;
}

function readDecodedRow(queries, table, id) {
  if (!queries?.decodedRow) return null;
  const row = queries.decodedRow.get(table, String(id));
  if (!row?.row_json) return null;
  try { return JSON.parse(row.row_json); } catch { return null; }
}

function deriveDescriptionScopeEvidence(entry) {
  const descriptions = entry.cleanDescriptions ?? {};
  const selfPatterns = {
    en: /\b(?:your|yourself|self)\b/i,
    "zh-CN": /(?:你的|自身)/u,
    "zh-TW": /(?:你的|自身)/u,
    ja: /自身/u,
    "ko-KR": /(?:시전자|자신)/u,
    es: /\b(?:tu|tus)\b/iu,
    "pt-BR": /\b(?:seu|sua|seus|suas)\b/iu,
    th: /ของคุณ/u,
    id: /(?:milikmu|dirimu)/iu,
  };
  const externalPatterns = {
    en: /\b(?:party|team|all(?:y|ies))\b/i,
    "zh-CN": /(?:队伍|队友|友方)/u,
    "zh-TW": /(?:隊伍|隊友|友方)/u,
    ja: /(?:パーティ|味方)/u,
    "ko-KR": /(?:파티|아군)/u,
    es: /\b(?:equipo|aliad[oa]s?)\b/iu,
    "pt-BR": /\b(?:equipe|aliad[oa]s?)\b/iu,
    th: /(?:ปาร์ตี้|พันธมิตร)/u,
    id: /(?:party|tim|sekutu)/iu,
  };
  const selfLocales = [];
  const externalLocales = [];
  for (const [locale, description] of Object.entries(descriptions)) {
    if (typeof description !== "string" || description.length === 0) continue;
    if (selfPatterns[locale]?.test(description)) selfLocales.push(locale);
    if (externalPatterns[locale]?.test(description)) externalLocales.push(locale);
  }
  const scopeProven = selfLocales.length >= 3 && externalLocales.length === 0;
  return {
    proof_state: scopeProven ? "current-build-multilocale-explicit-self-scope" : "scope-unresolved",
    scope: scopeProven ? "self-only" : "unresolved",
    explicit_self_locales: selfLocales.sort(compareText),
    external_recipient_locales: externalLocales.sort(compareText),
    proof_rule: "At least three exact current-build locales must explicitly identify the caster/self, and no checked locale may identify party, team, allies, or friendly recipients.",
  };
}

function findExactPaths(mechanic, queries) {
  const startNodes = new Map();
  for (const row of mechanic.decoded_rows ?? []) {
    const table = row.table ?? row.table_name;
    if (table && row.row_id !== undefined) startNodes.set(`${table}\0${row.row_id}`, { table, id: String(row.row_id), basis: "dependency-closure-decoded-row" });
  }
  for (const edge of mechanic.exact_reference_edges ?? []) {
    const table = edge.source_table;
    const id = edge.source_id;
    if (table && id !== undefined) startNodes.set(`${table}\0${id}`, { table, id: String(id), basis: "dependency-closure-exact-edge" });
  }

  const found = [];
  const queue = [...startNodes.values()].map((node) => ({ node, path: [], namespace_proven: true }));
  const visited = new Set(queue.map((entry) => `${entry.node.table}\0${entry.node.id}\0depth:0`));
  while (queue.length > 0) {
    const current = queue.shift();
    if (current.path.length >= MAX_DEPTH) continue;
    for (const row of queries.outgoing.all(current.node.table, current.node.id)) {
      const edge = {
        source_table: row.source_table,
        source_id: row.source_id,
        source_field: row.source_field,
        source_pointer: row.source_pointer,
        relationship: row.relationship,
        target_table: row.target_table,
        target_id: row.target_id,
        proof: row.proof,
      };
      const nextPath = [...current.path, edge];
      if (normalizeTable(row.target_table) === "damageattrtable") {
        found.push({
          target_damage_id: String(row.target_id),
          namespace_proven: current.namespace_proven,
          path_length: nextPath.length,
          path: nextPath,
        });
        continue;
      }
      const visitKey = `${row.target_table}\0${row.target_id}\0${nextPath.length}`;
      if (!visited.has(visitKey)) {
        visited.add(visitKey);
        queue.push({ node: { table: row.target_table, id: String(row.target_id) }, path: nextPath, namespace_proven: current.namespace_proven });
      }
    }
  }
  return dedupe(found, (entry) => `${entry.target_damage_id}\0${JSON.stringify(entry.path)}`)
    .sort((left, right) => left.path_length - right.path_length || compareIdentifiers(left.target_damage_id, right.target_damage_id));
}

function extractExplicitValidatedRoutes(source, recountById) {
  const routes = [];
  walk(source, [], (key, value, parent) => {
    if (key !== "evidenceStatus" || value !== "current-build-exact-defined-output") return;
    const damageIds = collectNamedIdentifiers(parent, /^(targetDamageIds|damageIds)$/);
    const recountIds = collectNamedIdentifiers(parent, /^(targetRecountIds|recountIds)$/);
    const expandedDamageIds = [...damageIds];
    for (const recountId of recountIds) {
      const row = recountById.get(recountId);
      if (row) expandedDamageIds.push(...(row.DamageId ?? []).map(String));
    }
    if (expandedDamageIds.length > 0) {
      routes.push({
        proof: "current-build-exact-defined-output",
        recount_ids: [...new Set(recountIds)].sort(compareIdentifiers),
        damage_ids: [...new Set(expandedDamageIds)].sort(compareIdentifiers),
      });
    }
  });
  return dedupe(routes, (route) => JSON.stringify(route));
}

function mergeExactRoutes(exactPaths, explicitRoutes, recountRows) {
  const rowsByDamage = new Map();
  for (const row of recountRows) {
    for (const damageId of row.DamageId ?? []) {
      const key = String(damageId);
      if (!rowsByDamage.has(key)) rowsByDamage.set(key, []);
      rowsByDamage.get(key).push(String(row.Id));
    }
  }
  const routes = [];
  for (const pathEntry of exactPaths.filter((entry) => entry.namespace_proven)) {
    routes.push({
      proof: "current-build-exact-graph",
      damage_ids: [pathEntry.target_damage_id],
      recount_ids: rowsByDamage.get(pathEntry.target_damage_id) ?? [],
      exact_path_count: 1,
    });
  }
  routes.push(...explicitRoutes);
  return dedupe(routes, (route) => `${route.proof}\0${route.damage_ids.join(",")}\0${route.recount_ids.join(",")}`);
}

function extractProducedTerms(source, mechanic) {
  const terms = [];
  addTerm(terms, source?.sourceName ?? mechanic.source_name, "source-name");
  const raw = source?.descriptions?.en ?? "";
  const clean = source?.cleanDescriptions?.en ?? raw;
  for (const match of raw.matchAll(/<linktext=[^>]+>\s*(?:<[^>]+>)*\s*([^<]+?)\s*(?:<\/[^>]+>)*\s*<\/linktext>/gi)) addTerm(terms, match[1], "localized-linktext");
  for (const match of clean.matchAll(/\b(?:triggers?|triggering|apply|applies|applying|summons?|summoned|casts?|launches?)\s+(?:an?\s+)?([A-Z][A-Za-z0-9' !&-]{2,60}?)(?=,|\.| dealing| that| for| to| once| is|$)/g)) addTerm(terms, match[1], "produced-action-phrase");
  for (const match of clean.matchAll(/\b([A-Z][A-Za-z0-9' !&-]{2,60}):/g)) addTerm(terms, match[1], "defined-term");
  return dedupe(terms.filter((term) => !isGenericTerm(term.normalized)), (term) => `${term.normalized}\0${term.origin}`)
    .sort((left, right) => compareText(left.normalized, right.normalized) || compareText(left.origin, right.origin));
}

function addTerm(target, value, origin) {
  const label = stripMarkup(String(value ?? "")).trim();
  const normalized = normalizeName(label);
  if (normalized.length >= 3) target.push({ label, normalized, origin });
}

function findNameCandidates(terms, recountRows, recountIndex, rejectedLookalikes) {
  const candidates = new Map();
  const explicitProducedTerms = terms.filter((term) => term.origin === "produced-action-phrase");
  const candidateTerms = explicitProducedTerms.length > 0 ? explicitProducedTerms : terms;
  for (const term of candidateTerms) {
    for (const row of recountIndex.byName.get(term.normalized) ?? []) {
      if (row.IsCatchAll) continue;
      const key = String(row.Id);
      if (!candidates.has(key)) {
        candidates.set(key, {
          recount_id: key,
          recount_name: row.RecountName ?? row.Name ?? null,
          damage_ids: [...new Set((row.DamageId ?? []).map(String))].sort(compareIdentifiers),
          match_kind: "exact-normalized-whole-name",
          proof_state: "name-only-candidate",
          entire_recount_parent_proven: false,
          matching_terms: [],
        });
      }
      candidates.get(key).matching_terms.push(term);
    }
    for (const row of recountRows) {
      if (row.IsCatchAll) continue;
      for (const label of recountLabels(row)) {
        const normalized = normalizeName(label);
        if (normalized === term.normalized || normalized.length < 3) continue;
        if (normalized.includes(term.normalized) || term.normalized.includes(normalized)) {
          rejectedLookalikes.push({
            term: term.label,
            recount_id: String(row.Id),
            recount_name: row.RecountName ?? row.Name ?? label,
            reason: "substring-or-superset-name-is-not-ownership-proof",
          });
        }
      }
    }
  }
  const uniqueLookalikes = dedupe(rejectedLookalikes, (entry) => `${normalizeName(entry.term)}\0${entry.recount_id}\0${entry.reason}`);
  rejectedLookalikes.splice(0, rejectedLookalikes.length, ...uniqueLookalikes);
  return [...candidates.values()].map((candidate) => ({
    ...candidate,
    matching_terms: dedupe(candidate.matching_terms, (term) => `${term.normalized}\0${term.origin}`),
    scope_warning: candidate.damage_ids.length > 1 ? "recount-parent-contains-multiple-damage-rows" : null,
  })).sort((left, right) => compareIdentifiers(left.recount_id, right.recount_id));
}

function buildRecountIndex(rows) {
  const byId = new Map();
  const byName = new Map();
  for (const row of rows) {
    byId.set(String(row.Id), row);
    for (const label of recountLabels(row)) {
      const normalized = normalizeName(label);
      if (!normalized) continue;
      if (!byName.has(normalized)) byName.set(normalized, []);
      byName.get(normalized).push(row);
    }
  }
  return { byId, byName };
}

function recountLabels(row) {
  return [...new Set([row.RecountName, row.Name, row.Names?.en].filter(Boolean).map(String))];
}

function buildMissingEdges(mechanic, candidates) {
  if (candidates.length === 0) return [{
    required_edge: `${mechanic.source_id} -> exact current-build DamageAttrTable row`,
    proof_required: "current-build table relationship or matching-build runtime event with source window and damage row",
  }];
  if (candidates.length === 1) return [{
    required_edge: `${mechanic.source_id} -> recount:${candidates[0].recount_id} damage row set`,
    proof_required: candidates[0].damage_ids.length > 1
      ? "exact child damage row subset plus ownership; entire recount parent cannot be assigned"
      : "current-build relationship or matching-build source-window runtime event",
  }];
  return [{
    required_edge: `${mechanic.source_id} -> one exact damage row among ${candidates.length} whole-name candidates`,
    proof_required: "disambiguating current-build relationship or matching-build runtime event",
  }];
}

function packetActivationEvidence(damageIds, activationIndex) {
  const requestedDamageIds = [...new Set(damageIds.map(String))].sort(compareIdentifiers);
  const observedById = activationIndex?.observed_damage_rows_by_id ?? {};
  const observedRows = requestedDamageIds
    .filter((damageId) => observedById[damageId])
    .map((damageId) => {
      const row = observedById[damageId];
      return {
        damage_id: damageId,
        damage_script: row.damage_script ?? null,
        type_enum: row.type_enum ?? null,
        packet_damage_results: row.packet_damage_results ?? 0,
        packet_healing_results: row.packet_healing_results ?? 0,
        packet_damage_source_actor_kinds: row.packet_damage_source_actor_kinds ?? {},
        packet_damage_target_actor_kinds: row.packet_damage_target_actor_kinds ?? {},
        packet_damage_value_shape: row.packet_damage_value_shape ?? null,
      };
    });
  const observedIds = new Set(observedRows.map((row) => row.damage_id));
  return {
    requested_damage_ids: requestedDamageIds,
    observed: observedRows.length > 0,
    observed_damage_ids: observedRows.map((row) => row.damage_id),
    unobserved_damage_ids: requestedDamageIds.filter((damageId) => !observedIds.has(damageId)),
    observed_rows: observedRows,
  };
}

function deriveAttributionState(promotionEligible, exactPacketActivation, candidates) {
  if (promotionEligible && exactPacketActivation.observed) return "exact-route-packet-observed-awaiting-scope-formula";
  if (promotionEligible) return "exact-route-not-observed-in-current-corpus";
  if (candidates.some((candidate) => candidate.packet_activation.observed)) return "packet-observed-candidate-awaiting-source-edge";
  return "no-current-packet-activation-proof";
}

function nextProofAction(state, mechanic, candidates, attributionState) {
  if (attributionState === "exact-route-packet-observed-awaiting-scope-formula") {
    return "Prove matching-build provider/recipient scope, stacking, formula, and counterfactual conservation before recalculating archived attribution.";
  }
  if (attributionState === "exact-route-not-observed-in-current-corpus") {
    return `Retain the exact route for ${mechanic.source_id} and obtain a matching-build activation capture; absence in this corpus is not evidence of inactivity.`;
  }
  if (attributionState === "packet-observed-candidate-awaiting-source-edge") {
    return `Prove the exact source edge from ${mechanic.source_id} to the observed child damage row; packet activation alone must not assign a candidate recount parent.`;
  }
  if (state === "current-build-exact-route") return "Verify matching-build activation, provider/recipient scope, stacking, and counterfactual conservation.";
  if (state === "candidate-only") {
    const candidate = candidates[0];
    return candidate.damage_ids.length > 1
      ? `Trace ${mechanic.source_id} to the exact child subset inside recount ${candidate.recount_id}; do not credit the whole parent.`
      : `Prove the edge from ${mechanic.source_id} to damage row ${candidate.damage_ids[0] ?? "unlisted"}.`;
  }
  if (state === "ambiguous-candidate") return `Trace ${mechanic.source_id} through its emitted runtime or current-build row to disambiguate ${candidates.length} candidates.`;
  return `Identify the exact DamageAttrTable output emitted while ${mechanic.source_id} is active.`;
}

function groupObligations(routes) {
  const groups = new Map();
  for (const route of routes.filter((entry) => !entry.promotion_eligible)) {
    const key = route.proof_state;
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(route.source_rule_id);
  }
  return [...groups.entries()].map(([proofState, sourceRuleIds]) => ({
    proof_state: proofState,
    source_count: sourceRuleIds.length,
    source_rule_ids: sourceRuleIds.sort(compareText),
  })).sort((left, right) => compareText(left.proof_state, right.proof_state));
}

function verify(input, indexPath, activationIndexPath) {
  const report = readJson(input, "produced-damage proof routes");
  if (report.schema_version !== 1) throw new Error("Produced-damage route schema_version must be 1");
  if (report.content_sha256 !== contentHash(report)) throw new Error("Produced-damage route content hash mismatch");
  if (report.summary.blocked_sources !== report.routes.length) throw new Error("Blocked-source count mismatch");
  if (report.summary.exact_selector_definitions !== report.selector_output_routes.length) throw new Error("Exact-selector count mismatch");
  if (!report.summary.zero_hidden_omissions) throw new Error("zero_hidden_omissions must remain true");
  const sourceRules = new Set();
  for (const route of report.routes) {
    if (sourceRules.has(route.source_rule_id)) throw new Error(`Duplicate route ${route.source_rule_id}`);
    sourceRules.add(route.source_rule_id);
    if (route.promotion_eligible && route.exact_routes.length === 0) throw new Error(`Route ${route.source_rule_id} promoted without exact evidence`);
    if (!route.promotion_eligible && route.missing_edges.length === 0) throw new Error(`Route ${route.source_rule_id} hides its missing edge`);
    if (!route.packet_activation || typeof route.packet_activation.observed !== "boolean") throw new Error(`Route ${route.source_rule_id} lacks packet activation evidence`);
    if (route.deferred_attribution_preserved !== true) throw new Error(`Route ${route.source_rule_id} dropped deferred attribution`);
    if (route.archived_recalculation_ready !== false) throw new Error(`Route ${route.source_rule_id} became recalculation-ready without complete scope and formula proof`);
    for (const candidate of route.candidate_routes) {
      if (!Array.isArray(candidate.damage_ids)) throw new Error(`Candidate ${candidate.recount_id} dropped its damage rows`);
      if (candidate.proof_state !== "name-only-candidate") throw new Error(`Candidate ${candidate.recount_id} is not quarantined`);
      if (!candidate.packet_activation || typeof candidate.packet_activation.observed !== "boolean") throw new Error(`Candidate ${candidate.recount_id} lacks packet activation evidence`);
      if (candidate.packet_activation.observed && !route.promotion_eligible && route.proof_state === "current-build-exact-route") {
        throw new Error(`Candidate ${candidate.recount_id} packet evidence caused an invalid promotion`);
      }
    }
  }
  const selectorKeys = new Set();
  for (const route of report.selector_output_routes) {
    const key = `${route.owner_skill_id}:${route.selector_effect_id}`;
    if (selectorKeys.has(key)) throw new Error(`Duplicate selector route ${key}`);
    selectorKeys.add(key);
    if (route.deferred_attribution_preserved !== true) throw new Error(`Selector route ${key} dropped deferred attribution`);
    if (route.archived_recalculation_ready !== false) throw new Error(`Selector route ${key} became recalculation-ready without complete attribution proof`);
    if (route.source_aliases_resolved !== false) throw new Error(`Selector route ${key} guessed an adjacent source alias`);
    if (route.output_identity_proven && route.exact_outputs.length === 0
      && !(route.output_kind === "self-owned-source-modifier" && route.transfer_scope_proven && route.transfer_scope === "self-only" && route.emitted_effects.length > 0)) {
      throw new Error(`Selector route ${key} claims output identity without an exact output or exact self-only emitted-status proof`);
    }
    if (route.rdps_transfer_eligible === false && route.transfer_scope !== "self-only") throw new Error(`Selector route ${key} excludes transferable rDPS without self-only proof`);
    if (route.transfer_scope === "self-only" && route.rdps_transfer_eligible !== false) throw new Error(`Selector route ${key} could grant transferable rDPS to a self-only source`);
    if (route.formula_identity_proven !== route.formula_family_proven) throw new Error(`Selector route ${key} has inconsistent formula-family proof`);
    if (route.formula_family_proven && (!route.formula_family_evidence?.selector_definition_found
      || !route.formula_family_evidence?.selector_description_found
      || !route.formula_family_evidence?.tier_value_ladder_proven
      || route.formula_family_evidence?.emitted_buff_definitions?.some((entry) => !entry.definition_found))) {
      throw new Error(`Selector route ${key} claims formula-family proof without exact decoded rows`);
    }
    if (route.normalization_semantics_proven && (!route.formula_family_evidence?.decision_unmarkpercent_normalization_proven
      || route.formula_family_evidence?.normalization_evidence?.proof_state !== "current-build-exact-client-code"
      || route.formula_family_evidence?.normalization_evidence?.raw_to_display_percent_divisor !== 100
      || route.formula_family_evidence?.normalization_evidence?.raw_to_fractional_ratio_divisor !== 10000)) {
      throw new Error(`Selector route ${key} claims normalization proof without exact current-build client-code evidence`);
    }
    if (route.numeric_formula_ready !== false || route.archived_recalculation_ready !== false) throw new Error(`Selector route ${key} enabled numeric attribution before selected-tier, normalization, and stack-at-hit proof`);
    if (!route.output_identity_proven && !route.next_proof_action) throw new Error(`Selector route ${key} hides its missing evidence`);
  }
  const expectedSummary = {
    exact_route_packet_observed: report.routes.filter((route) => route.promotion_eligible && route.packet_activation.observed).length,
    exact_route_not_observed_in_current_corpus: report.routes.filter((route) => route.promotion_eligible && !route.packet_activation.observed).length,
    packet_observed_quarantined_candidates: report.routes.filter((route) => !route.promotion_eligible && route.candidate_routes.some((candidate) => candidate.packet_activation.observed)).length,
    deferred_attribution_sources: report.routes.filter((route) => route.deferred_attribution_preserved && !route.archived_recalculation_ready).length,
    archived_recalculation_ready: report.routes.filter((route) => route.archived_recalculation_ready).length,
    selector_output_identity_proven: report.selector_output_routes.filter((route) => route.output_identity_proven).length,
    selector_output_packet_observed: report.selector_output_routes.filter((route) => route.packet_emission_observed).length,
    unresolved_selector_output_routes: report.selector_output_routes.filter((route) => !route.output_identity_proven).length,
    selector_deferred_attribution_sources: report.selector_output_routes.filter((route) => route.deferred_attribution_preserved && !route.archived_recalculation_ready).length,
  };
  for (const [field, value] of Object.entries(expectedSummary)) {
    if (report.summary[field] !== value) throw new Error(`${field} summary mismatch`);
  }
  if (indexPath) {
    requireFile(indexPath, "semantic evidence index");
    const db = new DatabaseSync(indexPath, { readOnly: true });
    try {
      if (readMetadata(db).game_build !== String(report.game_build)) throw new Error("Evidence index build mismatch");
    } finally { db.close(); }
  }
  if (activationIndexPath) {
    requireFile(activationIndexPath, "packet activation index");
    const activationIndex = readJson(activationIndexPath, "packet activation index");
    requireBuild(activationIndex, report.game_build, "packet activation index", "game_build");
    requireBuild(activationIndex, report.game_build, "packet activation index", "packet_build");
    if (activationIndex.summary?.unresolved_evidence_hidden !== false) throw new Error("Packet activation index hides unresolved evidence");
  }
  console.log(`Produced-damage proof routes verified for build ${report.game_build}: ${report.routes.length} blockers, zero hidden omissions.`);
  return report;
}

function selfTest() {
  const root = mkdtempSync(path.join(tmpdir(), "rlogs-produced-route-test-"));
  try {
    const db = new DatabaseSync(path.join(root, "index.sqlite"));
    db.exec(`
      CREATE TABLE metadata(key TEXT PRIMARY KEY,value TEXT NOT NULL) WITHOUT ROWID;
      CREATE TABLE exact_edges(source_table TEXT,source_id TEXT,source_field TEXT,source_pointer TEXT,relationship TEXT,target_table TEXT,target_id TEXT,proof TEXT,edge_json TEXT);
      CREATE TABLE decoded_rows(table_name TEXT NOT NULL,storage_key TEXT NOT NULL,row_id TEXT,row_sha256 TEXT NOT NULL,row_json TEXT NOT NULL,PRIMARY KEY(table_name,storage_key)) WITHOUT ROWID;
      INSERT INTO metadata VALUES('game_build','1'),('source_fingerprint','test'),('counts','{}');
      INSERT INTO exact_edges VALUES('BuffTable','10','DamageId','/DamageId','exact-reference','DamageAttrTable','900','exact-field','{}');
      INSERT INTO decoded_rows VALUES('BuffTable','420','420','fixture','{"Id":420,"TipsDescription":420,"RepeatAddRule":[0,1],"DestroyParam":[]}');
      INSERT INTO decoded_rows VALUES('AttrDescription','420','420','fixture','{"Id":420,"Description":"Your Crit DMG increases by {*Decision.unmarkpercent(1)*}; Crits add {*Decision.unmarkpercent(2)*}, up to 5 stacks for 2s."}');
      INSERT INTO decoded_rows VALUES('BuffTable','421','421','fixture','{"Id":421,"Name":"Self stack","Desc":"Each stack increases Crit DMG.","RepeatAddRule":[2,5],"DestroyParam":[[0,2]]}');
    `);
    const closure = { mechanics: [
      mechanic("mrs:exact", "buff-source:10", "Exact Proc", "BuffTable", "10"),
      mechanic("mrs:name", "talent:20", "Bleed", "TalentTable", "20"),
      mechanic("mrs:none", "talent:30", "Lightning Strike", "TalentTable", "30"),
    ] };
    const effectSources = { effectSourcesById: {
      "buff-source:10": { sourceName: "Exact Proc", descriptions: { en: "Exact Proc." } },
      "talent:20": { sourceName: "Bleed", descriptions: { en: "Applies Bleed." } },
      "talent:30": { sourceName: "Lightning Strike", descriptions: { en: "Triggers Lightning Strike." } },
    } };
    const recount = {
      1: { Id: 1, Name: "Bleed", RecountName: "Bleed", DamageId: [800, 801], IsCatchAll: false },
      2: { Id: 2, Name: "Falcon Lightning Strike", RecountName: "Falcon Lightning Strike", DamageId: [700], IsCatchAll: false },
    };
    const activationIndex = {
      game_build: "1",
      packet_build: "1",
      summary: { unresolved_evidence_hidden: false },
      observed_damage_rows_by_id: {
        900: { damage_id: "900", damage_script: "Attack", type_enum: 9, packet_damage_results: 3, packet_healing_results: 0, packet_damage_source_actor_kinds: { player: 3 }, packet_damage_target_actor_kinds: { monster: 3 }, packet_damage_value_shape: { results: 3 } },
        800: { damage_id: "800", damage_script: "Attack", type_enum: 8, packet_damage_results: 2, packet_healing_results: 0, packet_damage_source_actor_kinds: { player: 2 }, packet_damage_target_actor_kinds: { monster: 2 }, packet_damage_value_shape: { results: 2 } },
      },
    };
    const battleImagines = { entriesByUid: {
      40: { uid: 40, name: "Exact Imagine", decisionParameterRecords: [{ ownerSkillId: 40, buffId: 400, tier: 1, parameterSetIndex: 1, parameterValues: [], sourceTable: "SkillAoyiTable.ctb", sourceOffset: 1, rowId: 1, evidence: "SkillAoyi transformation kind 3 selects this buff and its 1-based BuffPar parameter set" }] },
      41: { uid: 41, name: "Deferred Imagine", decisionParameterRecords: [{ ownerSkillId: 41, buffId: 410, tier: 1, parameterSetIndex: 1, parameterValues: [], sourceTable: "SkillAoyiTable.ctb", sourceOffset: 2, rowId: 2, evidence: "SkillAoyi transformation kind 3 selects this buff and its 1-based BuffPar parameter set" }] },
      42: { uid: 42, name: "Self Imagine", cleanDescriptions: { en: "After casting, your Crit increases.", "zh-CN": "释放后，你的暴击提升。", ja: "発動後、自身の会心がアップする。", "ko-KR": "시전자의 치명타가 증가합니다." }, decisionParameterRecords: [
        { ownerSkillId: 42, buffId: 420, tier: 1, parameterSetIndex: 1, parameterValues: [560, 166], sourceTable: "SkillAoyiTable.ctb", sourceOffset: 3, rowId: 3, evidence: "SkillAoyi transformation kind 3 selects this buff and its 1-based BuffPar parameter set" },
        { ownerSkillId: 42, buffId: 420, tier: 2, parameterSetIndex: 1, parameterValues: [728, 216], sourceTable: "SkillAoyiTable.ctb", sourceOffset: 4, rowId: 4, evidence: "SkillAoyi transformation kind 3 selects this buff and its 1-based BuffPar parameter set" },
      ] },
    } };
    effectSources.effectSourcesById["buff-source:401"] = { targets: [{ targetKind: "damage", damageId: 900, parentRecountId: 9, evidenceStatus: "current-build-direct-damage-attr-buff-link" }] };
    const originCatalog = { game_build: "1", effects: [{ effect_id: 421, status_events: 3, window_count: 1, cross_actor_window_count: 0, source_missing_window_count: 0, minimum_stacks: 1, maximum_stacks: 5, applied: 1, refreshed: 1, stacked: 1, consumed: 0, removed: 0, observed_sessions: ["fixture"] }], relations: [
      { source_type_id: 1, source_config_id: 400, effect_id: 401, observation_count: 2, observed_sessions: ["fixture"] },
      { source_type_id: 1, source_config_id: 420, effect_id: 421, observation_count: 3, observed_sessions: ["fixture"] },
    ] };
    const numberSemantics = {
      game_build: "1",
      source: { bytecode_sha256: "fixture-bytecode", decompiled_source_sha256: "fixture-source" },
      normalization: {
        proof_state: "current-build-exact-client-code",
        semantics_proven: true,
        raw_to_display_percent_divisor: 100,
        raw_to_fractional_ratio_divisor: 10000,
        proof_scope: "Decision.markpercent and Decision.unmarkpercent description rendering",
        runtime_formula_order_proven: false,
      },
    };
    const report = generateReport({ build: "1", closure, effectSources, battleImagines, originCatalog, recount, activationIndex, numberSemantics, db, inputs: {} });
    db.close();
    const exact = report.routes.find((route) => route.source_rule_id === "mrs:exact");
    if (!exact?.promotion_eligible || exact.attribution_state !== "exact-route-packet-observed-awaiting-scope-formula") throw new Error("Exact graph route was not promoted with packet activation");
    const named = report.routes.find((route) => route.source_rule_id === "mrs:name");
    if (named?.promotion_eligible || named?.candidate_routes.length !== 1 || named.candidate_routes[0].damage_ids.length !== 2 || !named.candidate_routes[0].packet_activation.observed || named.attribution_state !== "packet-observed-candidate-awaiting-source-edge") throw new Error("Name-only candidate escaped quarantine, lost children, or lost packet evidence");
    const none = report.routes.find((route) => route.source_rule_id === "mrs:none");
    if (none?.candidate_routes.length !== 0 || none?.rejected_lookalikes.length === 0) throw new Error("Substring lookalike was not rejected");
    const selectorExact = report.selector_output_routes.find((route) => route.owner_skill_id === "40");
    const selectorDeferred = report.selector_output_routes.find((route) => route.owner_skill_id === "41");
    const selectorSelf = report.selector_output_routes.find((route) => route.owner_skill_id === "42");
    if (!selectorExact?.output_identity_proven || selectorExact.source_aliases_resolved || selectorExact.exact_outputs[0]?.damage_ids[0] !== "900") throw new Error("Exact selector output route was not preserved independently from aliases");
    if (selectorDeferred?.output_identity_proven || selectorDeferred?.proof_state !== "exact-selector-awaiting-packet-emission") throw new Error("Unobserved selector route was not kept as an explicit obligation");
    if (!selectorSelf?.output_identity_proven || selectorSelf.output_kind !== "self-owned-source-modifier" || selectorSelf.transfer_scope !== "self-only" || selectorSelf.rdps_transfer_eligible !== false || selectorSelf.formula_identity_proven !== true || selectorSelf.formula_family_proven !== true || selectorSelf.stack_limit_proven !== true || selectorSelf.normalization_semantics_proven !== true || selectorSelf.selected_runtime_tier_proven !== false || selectorSelf.numeric_formula_ready !== false || selectorSelf.archived_recalculation_ready !== false) throw new Error("Exact self-only selector formula family was not separated from its unresolved numeric attribution");
    const output = path.join(root, "report.json");
    writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`);
    verify(output);
    console.log("Produced-damage proof route self-test passed: exact routes promote, names remain candidates, lookalikes remain rejected.");
  } finally { rmSync(root, { recursive: true, force: true }); }
}

function mechanic(sourceRuleId, sourceId, sourceName, table, rowId) {
  return {
    source_rule_id: sourceRuleId,
    source_id: sourceId,
    source_name: sourceName,
    source_kind: sourceId.split(":")[0],
    seeds: [{ id: rowId }],
    decoded_rows: [{ table, row_id: rowId }],
    exact_reference_edges: [],
    unresolved_dependencies: [{ kind: DEPENDENCY }],
  };
}

function createQueries(db) {
  return {
    outgoing: db.prepare(`SELECT source_table,source_id,source_field,source_pointer,relationship,target_table,target_id,proof FROM exact_edges WHERE source_table=? AND source_id=? ORDER BY target_table,target_id,source_pointer`),
    decodedRow: db.prepare(`SELECT row_json FROM decoded_rows WHERE table_name=? AND row_id=? ORDER BY storage_key LIMIT 1`),
  };
}

function readMetadata(db) {
  return Object.fromEntries(db.prepare("SELECT key,value FROM metadata ORDER BY key").all().map((row) => [row.key, row.value]));
}

function normalizeTable(value) { return String(value ?? "").replaceAll(/[^a-z0-9]/gi, "").toLowerCase(); }
function normalizeName(value) { return stripMarkup(String(value ?? "")).normalize("NFKC").toLowerCase().replaceAll(/[’‘]/g, "'").replaceAll(/[^\p{L}\p{N}]+/gu, " ").trim().replaceAll(/\s+/g, " "); }
function stripMarkup(value) { return value.replaceAll(/<[^>]+>/g, " ").replaceAll(/\s+/g, " "); }
function isGenericTerm(value) { return new Set(["atk", "dmg", "attack dmg", "lucky strike", "basic attack", "basic attacks", "max hp", "hp", "hard", "nightmare"]).has(value); }

function collectNamedIdentifiers(value, keyPattern) {
  const result = [];
  walk(value, [], (key, item) => {
    if (keyPattern.test(key)) result.push(...extractIdentifiers(item));
  });
  return [...new Set(result)].sort(compareIdentifiers);
}

function extractIdentifiers(value) {
  if (typeof value === "string" || typeof value === "number" || typeof value === "bigint") return /^-?\d+$/.test(String(value)) ? [String(value)] : [];
  if (Array.isArray(value)) return value.flatMap(extractIdentifiers);
  return [];
}

function walk(value, pathParts, visitor) {
  if (!value || typeof value !== "object") return;
  for (const [key, item] of Object.entries(value)) {
    visitor(key, item, value, pathParts);
    if (item && typeof item === "object") walk(item, [...pathParts, key], visitor);
  }
}

function dedupe(values, keyFn) { const seen = new Set(); return values.filter((value) => { const key = keyFn(value); if (seen.has(key)) return false; seen.add(key); return true; }); }
function countValues(values) { const result = {}; for (const value of values) result[value] = (result[value] ?? 0) + 1; return result; }
function compareText(left, right) { return String(left ?? "").localeCompare(String(right ?? ""), "en", { numeric: true }); }
function compareIdentifiers(left, right) { try { const a = BigInt(left); const b = BigInt(right); return a < b ? -1 : a > b ? 1 : 0; } catch { return compareText(left, right); } }
function contentHash(value) { const copy = structuredClone(value); delete copy.content_sha256; return createHash("sha256").update(JSON.stringify(copy)).digest("hex"); }
function fileDescriptor(file) { return { path: file.replaceAll("\\", "/"), bytes: readFileSync(file).length, sha256: createHash("sha256").update(readFileSync(file)).digest("hex") }; }
function requireBuild(value, build, label, field) { if (String(value?.[field]) !== String(build)) throw new Error(`${label} build ${value?.[field]} does not match ${build}`); }
function requireFile(file, label) { if (!existsSync(file)) throw new Error(`Missing ${label}: ${file}`); }
function readJson(file, label) { try { return JSON.parse(readFileSync(file, "utf8")); } catch (error) { throw new Error(`Unable to read ${label} ${file}: ${error.message}`); } }

function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (!arg.startsWith("--")) throw new Error(`Unexpected argument ${arg}`);
    const key = arg.slice(2);
    const value = args[index + 1];
    if (value === undefined || value.startsWith("--")) throw new Error(`Missing value for --${key}`);
    parsed[key] = value;
    index += 1;
  }
  return parsed;
}

function required(value, key) { if (!value[key]) throw new Error(`Missing --${key}`); return value[key]; }
function usage(exitCode) {
  console.log(`Usage:
  node tools/bpsr-produced-damage-proof-routes.mjs build --build <id> --index <sqlite> --semantic-closure <json> --effect-sources <json> --battle-imagines <json> --origin-catalog <json> --recount <json> --activation-index <json> --number-semantics <json> --output <json>
  node tools/bpsr-produced-damage-proof-routes.mjs verify --input <json> [--index <sqlite>] [--activation-index <json>]
  node tools/bpsr-produced-damage-proof-routes.mjs self-test`);
  process.exit(exitCode);
}
