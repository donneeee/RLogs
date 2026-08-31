#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "build") build(resolveContext(options));
else if (command === "verify") verify(path.resolve(required(options, "input")));
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function resolveContext(parsed) {
  const buildId = required(parsed, "build");
  if (!/^\d+$/.test(buildId)) throw new Error("Build must contain only ASCII digits");
  return {
    build: buildId,
    selectionRouteProof: path.resolve(required(parsed, "selection-route-proof")),
    factorCatalog: path.resolve(required(parsed, "factor-catalog")),
    closure: path.resolve(required(parsed, "closure")),
    relationshipTable: path.resolve(required(parsed, "relationship-table")),
    damageChainBridge: path.resolve(required(parsed, "damage-chain-bridge")),
    effectSources: path.resolve(required(parsed, "effect-sources")),
    recountTable: path.resolve(required(parsed, "recount-table")),
    sourceManifest: path.resolve(required(parsed, "source-manifest")),
    output: path.resolve(required(parsed, "output")),
  };
}

function build(context) {
  for (const [label, file] of Object.entries({
    "selected-factor selection route proof": context.selectionRouteProof,
    "factor catalog": context.factorCatalog,
    "factor closure": context.closure,
    "modifier relationship table": context.relationshipTable,
    "skill damage-chain bridge": context.damageChainBridge,
    "effect sources": context.effectSources,
    "recount table": context.recountTable,
    "complete-build source manifest": context.sourceManifest,
  })) requireFile(file, label);

  const selection = readJson(context.selectionRouteProof, "selected-factor selection route proof");
  const factors = readJson(context.factorCatalog, "factor catalog");
  const closure = readJson(context.closure, "factor closure");
  const relationships = readJson(context.relationshipTable, "modifier relationship table");
  const damageBridge = readJson(context.damageChainBridge, "skill damage-chain bridge");
  const effectSources = readJson(context.effectSources, "effect sources");
  const recountTable = readJson(context.recountTable, "recount table");
  const manifest = readJson(context.sourceManifest, "complete-build source manifest");

  validateInputs(context, selection, factors, closure, relationships, damageBridge, effectSources, manifest);
  const closureByFamily = new Map(asArray(closure.families ?? closure.factor_families).map((entry) => [Number(entry.family_id ?? entry.familyId), entry]));
  const obligations = (selection.blocker_obligations ?? []).map((selectionObligation) => buildMechanicObligation({
    selectionObligation,
    factors,
    closureByFamily,
    relationships,
    damageBridge,
    effectSources,
    recountTable,
  })).sort((left, right) => compareText(left.obligation_id, right.obligation_id));
  assertCoverage(obligations);

  const report = {
    schema_version: 2,
    generated_by: "tools/bpsr-selected-factor-mechanic-route-proof.mjs",
    game_build: context.build,
    proof_state: "mechanics-candidates-indexed-runtime-proof-open",
    policy: {
      exact_relationship_edges_are_strict_routes: true,
      description_and_localized_name_routes_are_candidates_only: true,
      catalog_routes_are_retained_without_automatic_promotion: true,
      exact_damage_chain_membership_does_not_solve_child_allocation: true,
      exact_state_route_does_not_prove_runtime_activation: true,
      conflicting_broad_and_exact_transfer_labels_are_retained: true,
      declared_transfer_classification_is_retained_separately: true,
      static_owner_context_does_not_prove_self_only_recipient_scope: true,
      effective_transfer_classification_reopens_owner_local_context_without_packet_proof: true,
      runtime_obligation_matrix_is_a_proof_plan_not_runtime_proof: true,
      proof_receipt_does_not_promote_rdps_obligations: true,
      unresolved_evidence_is_never_hidden: true,
    },
    inputs: {
      selection_route_proof: fileDescriptor(context.selectionRouteProof),
      factor_catalog: fileDescriptor(context.factorCatalog),
      factor_closure: fileDescriptor(context.closure),
      modifier_relationship_table: fileDescriptor(context.relationshipTable),
      skill_damage_chain_bridge: fileDescriptor(context.damageChainBridge),
      effect_sources: fileDescriptor(context.effectSources),
      recount_table: fileDescriptor(context.recountTable),
      complete_build_source_manifest: fileDescriptor(context.sourceManifest),
    },
    route_contract: {
      strict_uid_route_source: "ModifierRelationshipTable.sourcesByRuleId[source_rule_id].uidEdges",
      strict_damage_route_edge: "edgeKind=target-damage-row and uidKind=damage",
      strict_recount_route_edge: "edgeKind=target-recount-row and uidKind=recount",
      exact_chain_join: "SkillDamageChainBridge.damageChains[damage_id] and recountChains[recount_id]",
      exact_recount_join: "RecountTable[recount_id]",
      effect_component_join: "EffectSources.buffIdToEffectSourceIds[buff_id] -> effectSourcesById[source_id]",
      grade_value_join: "SeasonPhantomFactors.factorsByBuffId[buff_id].modifierEvidence.gradeRows",
      retained_catalog_routes: "SeasonPhantomFactors affected* arrays and psychoscope offline closure",
      candidate_routes_never_become_strict_without_exact_relationship_edges: true,
      declared_self_only_formula_context_effective_route: "owner-local-formula-context-recipient-scope-open",
    },
    summary: summarize(obligations),
    blocker_obligations: obligations,
    still_required_runtime_gates: [
      "authoritative selected-grade snapshot bound to encounter time",
      "exact source-buff lifecycle and trigger event",
      "exact source and recipient ownership at each dependent event",
      "exact skill mutation before and after state where claimed",
      "exact energy generation or consumption event and amount where claimed",
      "multi-damage recount child allocation where the chain is not single-output",
      "integer counterfactual replay with the selected factor removed",
      "party damage conservation",
    ],
  };
  report.content_sha256 = contentHash(report);
  mkdirSync(path.dirname(context.output), { recursive: true });
  writeJson(context.output, report);
  verify(context.output);
  console.log(`Selected-factor mechanic route proof built for ${context.build}: ${report.summary.selector_obligations} obligations, ${report.summary.strict_unique_damage_ids} strict damage rows, ${report.summary.strict_unique_recount_ids} strict recount rows, ${report.summary.strict_exact_state_routes} exact state route(s), zero rDPS promotions.`);
}

function buildMechanicObligation({ selectionObligation, factors, closureByFamily, relationships, damageBridge, effectSources, recountTable }) {
  const sourceRuleId = String(selectionObligation.source_rule_id);
  const sourceId = String(selectionObligation.source_id);
  const buffIds = uniqueNumeric(selectionObligation.effect_ids ?? []);
  if (buffIds.length !== 1) throw new Error(`${sourceRuleId} must resolve to exactly one selected factor buff`);
  const buffId = buffIds[0];
  const factor = factors.factorsByBuffId?.[String(buffId)];
  if (!factor) throw new Error(`Factor catalog omits selected buff ${buffId}`);
  const familyId = Number(factor.familyId);
  const closureFamily = closureByFamily.get(familyId);
  if (!closureFamily) throw new Error(`Factor closure omits family ${familyId}`);
  const relationship = relationships.sourcesByRuleId?.[sourceRuleId];
  if (!relationship || String(relationship.sourceId) !== sourceId) throw new Error(`Relationship table omits or mismatches ${sourceRuleId}`);
  const strictEdges = (relationship.uidEdges ?? []).map(normalizeTransferRoute).sort(compareEdges);
  const strictDamageIds = edgeIds(strictEdges, "target-damage-row", "damage");
  const strictRecountIds = edgeIds(strictEdges, "target-recount-row", "recount");
  const formulaEdges = strictEdges.filter((edge) => edge.edgeKind === "formula-component");
  const runtimeBuffIds = uniqueNumeric(strictEdges.filter((edge) => edge.uidKind === "buff" && ["observed-buff", "runtime-buff"].includes(edge.edgeKind)).map((edge) => edge.uid));
  const factorEffectSourceIds = uniqueText((effectSources.buffIdToEffectSourceIds?.[String(buffId)] ?? []).map(String));
  const sourceRecords = factorEffectSourceIds
    .map((id) => effectSources.effectSourcesById?.[id])
    .filter(Boolean)
    .map(normalizeSourceRecord);
  const components = sourceRecords
    .flatMap((record) => record.effectComponents ?? record.attributionModel?.components ?? [])
    .map((component) => structuredClone(component));
  const transferConflicts = findTransferConflicts(formulaEdges, components);
  const recipientScopeProof = buildRecipientScopeProof(formulaEdges, components);
  const strictDamageChains = strictDamageIds.map((damageId) => ({
    damage_id: damageId,
    chain: structuredClone(damageBridge.damageChains?.[String(damageId)] ?? null),
  }));
  const strictRecountRows = strictRecountIds.map((recountId) => ({
    recount_id: recountId,
    row: structuredClone(recountTable[String(recountId)] ?? null),
    chain: structuredClone(damageBridge.recountChains?.[String(recountId)] ?? null),
  }));
  if (strictDamageChains.some((entry) => !entry.chain)) throw new Error(`${sourceRuleId} has a strict damage row absent from SkillDamageChainBridge`);
  if (strictRecountRows.some((entry) => !entry.row || !entry.chain)) throw new Error(`${sourceRuleId} has a strict recount row absent from an exact join`);

  const gradeByNumber = new Map((factor.modifierEvidence?.gradeRows ?? []).map((row) => [Number(row.grade), row]));
  const itemByGrade = new Map((factor.gradeItems ?? []).map((row) => [Number(row.grade), row]));
  const gradeRoutes = (selectionObligation.grade_item_routes ?? []).map((selectionRoute) => {
    const grade = Number(selectionRoute.grade);
    const gradeRow = gradeByNumber.get(grade);
    const item = itemByGrade.get(grade);
    if (!gradeRow || !item || Number(gradeRow.itemId) !== Number(selectionRoute.item_id) || Number(item.itemId) !== Number(selectionRoute.item_id)) throw new Error(`${sourceRuleId} grade ${grade} does not join exactly`);
    return {
      ...structuredClone(selectionRoute),
      parameter_values: structuredClone(gradeRow.parameterValues ?? item.parameterValues ?? []),
      value_texts: structuredClone(gradeRow.valueTexts ?? []),
      resolved_description_candidate: String(gradeRow.cleanResolvedDescription ?? item.energy?.description ?? ""),
      resolved_description_evidence_class: "localized-render-candidate-not-strict-relationship-proof",
      energy: item.energy ? structuredClone(item.energy) : null,
    };
  }).sort((left, right) => left.grade - right.grade);

  const retainedCatalogRoutes = {
    affected_damage_ids: uniqueNumeric(factor.affectedDamageIds ?? []),
    affected_recount_ids: uniqueNumeric(factor.affectedRecountIds ?? []),
    affected_runtime_selectors: structuredClone(factor.affectedRuntimeSelectors ?? []),
    affected_generated_damage_families: structuredClone(factor.affectedGeneratedDamageFamilies ?? []),
    affected_generated_output_families: structuredClone(factor.affectedGeneratedOutputFamilies ?? []),
    affected_state_routes: structuredClone(factor.affectedStateRoutes ?? []),
    retained_description_target_candidates: structuredClone(factor.retainedDescriptionTargetCandidates ?? []),
    closure_direct_damage_ids: uniqueNumeric(closureFamily.direct_damage_ids ?? []),
    closure_declared_recount_ids: uniqueNumeric(closureFamily.declared_recount_ids ?? []),
    closure_exact_recount_ids: uniqueNumeric(closureFamily.exact_recount_ids ?? []),
    closure_exact_skill_ids: uniqueNumeric(closureFamily.exact_skill_ids ?? []),
    closure_runtime_selectors: structuredClone(closureFamily.runtime_selectors ?? []),
    closure_generated_damage_families: structuredClone(closureFamily.generated_damage_families ?? []),
    closure_generated_output_families: structuredClone(closureFamily.generated_output_families ?? []),
    closure_state_routes: structuredClone(closureFamily.state_routes ?? []),
    evidence_class: "retained-catalog-route-not-strict-unless-also-present-in-exact-relationship-edges",
  };
  const strictStateRoutes = uniqueObjects([
    ...(factor.affectedStateRoutes ?? []),
    ...(closureFamily.state_routes ?? []),
  ].filter((route) => String(route.resolutionState ?? route.resolution_state) === "exact-current-build-state-route"));
  const candidateOnlyDamageIds = uniqueNumeric([
    ...retainedCatalogRoutes.affected_damage_ids,
    ...retainedCatalogRoutes.closure_direct_damage_ids,
  ].filter((id) => !strictDamageIds.includes(id)));
  const candidateOnlyRecountIds = uniqueNumeric([
    ...retainedCatalogRoutes.affected_recount_ids,
    ...retainedCatalogRoutes.closure_declared_recount_ids,
    ...retainedCatalogRoutes.closure_exact_recount_ids,
  ].filter((id) => !strictRecountIds.includes(id)));

  const unresolvedMechanicGates = buildOpenGates({ factor, closureFamily, strictDamageIds, strictRecountIds, strictStateRoutes, formulaEdges, gradeRoutes });
  const runtimeProofPlan = buildRuntimeProofPlan({
    selectionObligation,
    buffId,
    gradeRoutes,
    strictDamageIds,
    strictRecountIds,
    strictStateRoutes,
    retainedCatalogRoutes,
    recipientScopeProof,
  });
  return {
    obligation_id: `${sourceRuleId}#selectedFactorMechanics`,
    selection_obligation_id: String(selectionObligation.obligation_id),
    source_rule_id: sourceRuleId,
    source_id: sourceId,
    factor_identity: {
      family_id: familyId,
      buff_id: buffId,
      name: String(factor.familyNames?.en ?? factor.familyName ?? closureFamily.family_name ?? sourceId),
      class_gate_ids: uniqueNumeric(factor.classGateIds ?? closureFamily.class_gate_ids ?? []),
      slot_category: String(factor.slotCategory ?? closureFamily.slot_category ?? "unknown"),
      runtime_role: String(factor.runtimeRole ?? closureFamily.runtime_role ?? "unknown"),
    },
    grade_routes: gradeRoutes,
    candidate_descriptions: structuredClone(factor.descriptions ?? closureFamily.descriptions ?? {}),
    candidate_description_evidence_class: "localized-render-candidate",
    strict_relationship_routes: {
      uid_edges: strictEdges,
      formula_components: formulaEdges,
      runtime_buff_ids: runtimeBuffIds,
      target_damage_ids: strictDamageIds,
      target_recount_ids: strictRecountIds,
      exact_state_routes: strictStateRoutes,
    },
    strict_damage_chains: strictDamageChains,
    strict_recount_rows: strictRecountRows,
    retained_catalog_routes: retainedCatalogRoutes,
    candidate_only_damage_ids: candidateOnlyDamageIds,
    candidate_only_recount_ids: candidateOnlyRecountIds,
    effect_source_join: {
      effect_source_ids: factorEffectSourceIds,
      source_records: sourceRecords,
      components,
    },
    transfer_classification_conflicts: transferConflicts,
    recipient_scope_proof: recipientScopeProof,
    runtime_proof_plan: runtimeProofPlan,
    unresolved_mechanic_gates: unresolvedMechanicGates,
    route_status: "mechanics-candidates-indexed-runtime-proof-open",
    rdps_promoted: false,
    hidden_omissions: 0,
  };
}

function buildRuntimeProofPlan({ selectionObligation, buffId, gradeRoutes, strictDamageIds, strictRecountIds, strictStateRoutes, retainedCatalogRoutes, recipientScopeProof }) {
  const energyGradeRoutes = gradeRoutes
    .filter((row) => row.energy && String(row.energy.behavior ?? "none-observed") !== "none-observed")
    .map((row) => ({
      grade: Number(row.grade),
      item_id: Number(row.item_id),
      behavior: String(row.energy.behavior),
      amount: Number.isFinite(Number(row.energy.amount)) ? Number(row.energy.amount) : null,
      amount_status: String(row.energy.amountStatus ?? "unresolved"),
      evidence_class: "localized-grade-derived-candidate-requires-packet-event-and-amount-proof",
    }));
  const runtimeSelectors = uniqueObjects([
    ...(retainedCatalogRoutes.affected_runtime_selectors ?? []),
    ...(retainedCatalogRoutes.closure_runtime_selectors ?? []),
  ]);
  const generatedDamageFamilies = uniqueObjects([
    ...(retainedCatalogRoutes.affected_generated_damage_families ?? []),
    ...(retainedCatalogRoutes.closure_generated_damage_families ?? []),
  ]);
  const generatedOutputFamilies = uniqueObjects([
    ...(retainedCatalogRoutes.affected_generated_output_families ?? []),
    ...(retainedCatalogRoutes.closure_generated_output_families ?? []),
  ]);
  const candidateStateRoutes = uniqueObjects([
    ...(retainedCatalogRoutes.affected_state_routes ?? []),
    ...(retainedCatalogRoutes.closure_state_routes ?? []),
  ]);
  const triggeredOutputRequired = strictDamageIds.length > 0
    || strictRecountIds.length > 0
    || generatedDamageFamilies.length > 0
    || generatedOutputFamilies.length > 0;
  const stateOrSkillMutationRequired = strictStateRoutes.length > 0 || candidateStateRoutes.length > 0 || runtimeSelectors.length > 0;

  return {
    proof_state: "runtime-obligations-indexed-no-runtime-closure",
    selection: {
      required: true,
      route: "profile-snapshot-selected-factor-item-and-grade-bound-to-encounter-time",
      selection_obligation_id: String(selectionObligation.obligation_id),
      grade_item_ids: uniqueNumeric(gradeRoutes.map((row) => row.item_id)),
      status: "exact-selection-route-known-encounter-time-binding-open",
    },
    lifecycle: {
      required: true,
      route: "canonical-status-effect-lifecycle",
      effect_ids: [buffId],
      required_events: ["apply", "refresh", "stack", "consume", "remove"],
      required_identity: ["provider_entity_uuid", "recipient_entity_uuid", "effect_id", "timestamp"],
      status: "selected-factor-matching-lifecycle-window-open",
    },
    energy: {
      required: energyGradeRoutes.length > 0,
      grade_amount_routes: energyGradeRoutes,
      required_event_fields: energyGradeRoutes.length ? ["provider_entity_uuid", "recipient_entity_uuid", "behavior", "amount", "before", "after", "timestamp"] : [],
      status: energyGradeRoutes.length ? "packet-energy-event-and-amount-proof-open" : "no-energy-route-declared-for-selected-family",
    },
    state_or_skill_mutation: {
      required: stateOrSkillMutationRequired,
      exact_state_routes: structuredClone(strictStateRoutes),
      retained_candidate_state_routes: candidateStateRoutes,
      runtime_selectors: runtimeSelectors,
      required_event_fields: stateOrSkillMutationRequired ? ["provider_entity_uuid", "recipient_entity_uuid", "before_state", "after_state", "trigger_event", "timestamp"] : [],
      status: stateOrSkillMutationRequired ? "runtime-predicate-and-output-proof-open" : "no-state-or-skill-mutation-route-indexed",
    },
    triggered_output: {
      required: triggeredOutputRequired,
      strict_damage_ids: structuredClone(strictDamageIds),
      strict_recount_ids: structuredClone(strictRecountIds),
      retained_generated_damage_families: generatedDamageFamilies,
      retained_generated_output_families: generatedOutputFamilies,
      multi_child_allocation_required: strictDamageIds.length > 1 || (strictDamageIds.length > 0 && strictRecountIds.length > 0),
      required_event_fields: triggeredOutputRequired ? ["provider_entity_uuid", "recipient_entity_uuid", "target_entity_uuid", "ability_id", "damage_id", "recount_id", "amount", "timestamp"] : [],
      status: triggeredOutputRequired ? "output-ownership-and-recount-conservation-open" : "no-triggered-output-route-indexed",
    },
    recipient_scope: {
      required: recipientScopeProof.requires_recipient_packet_proof === true,
      required_event_fields: ["provider_entity_uuid", "recipient_entity_uuid", "dependent_actor_entity_uuid", "timestamp"],
      effective_transfer_eligibility: structuredClone(recipientScopeProof.effective_transfer_eligibility),
      status: recipientScopeProof.requires_recipient_packet_proof ? "provider-recipient-window-proof-open" : "no-open-recipient-route-in-static-proof",
    },
    stacking_overlap: {
      required: true,
      route: "canonical-status-effect-lifecycle-overlap-by-provider-and-recipient",
      required_events: ["apply", "refresh", "stack", "consume", "remove"],
      required_outputs: ["stack_bounds", "concurrent_instances", "distinct_providers_per_recipient", "overwrite_or_coexistence_rule"],
      status: "stacking-and-overlap-proof-open",
    },
    counterfactual: {
      required: true,
      integer_only: true,
      route: "replay-observed-dependent-events-with-selected-factor-removed",
      required_inputs: ["selected_grade", "provider_recipient_windows", "base_terms", "rounding_order", "clamps", "caps"],
      status: "integer-counterfactual-open",
    },
    conservation: {
      required: true,
      route: "party-damage-before-and-after-attribution-conservation",
      invariant: "sum(raw_party_damage)==sum(adjusted_personal_damage)+sum(credited_transfer_damage)",
      status: "party-damage-conservation-open",
    },
  };
}

function buildOpenGates({ factor, closureFamily, strictDamageIds, strictRecountIds, strictStateRoutes, formulaEdges, gradeRoutes }) {
  const gates = new Set(["selected-grade-snapshot-bound-to-encounter", "source-buff-lifecycle-and-trigger-event"]);
  const energyBehaviors = uniqueText(gradeRoutes.map((row) => row.energy?.behavior).filter(Boolean));
  if (energyBehaviors.some((value) => value !== "none-observed")) gates.add("energy-event-and-amount-runtime-proof");
  if (strictDamageIds.length || strictRecountIds.length) gates.add("output-event-ownership-and-recount-conservation");
  if (strictDamageIds.length && strictRecountIds.length) gates.add("multi-damage-child-allocation-if-applicable");
  if (strictStateRoutes.length) gates.add("state-route-runtime-predicate-and-output-proof");
  if ((factor.affectedRuntimeSelectors ?? closureFamily.runtime_selectors ?? []).length) gates.add("runtime-selector-resolution");
  if ((factor.affectedGeneratedDamageFamilies ?? closureFamily.generated_damage_families ?? []).length) gates.add("generated-damage-family-runtime-resolution");
  if ((factor.affectedGeneratedOutputFamilies ?? closureFamily.generated_output_families ?? []).length) gates.add("generated-output-family-runtime-resolution");
  if (!formulaEdges.length && !strictDamageIds.length && !strictRecountIds.length && !strictStateRoutes.length) gates.add("exact-mechanic-relationship-route");
  gates.add("integer-counterfactual-projection");
  gates.add("party-damage-conservation");
  return [...gates].sort(compareText);
}

function findTransferConflicts(formulaEdges, components) {
  const exact = uniqueText(formulaEdges.map(declaredTransferEligibility).filter(Boolean));
  const broad = uniqueText(components.map(declaredTransferEligibility).filter(Boolean));
  const conflicts = [];
  for (const exactValue of exact) for (const broadValue of broad) {
    if (exactValue !== broadValue) conflicts.push({
      exact_relationship_transfer: exactValue,
      effect_source_component_transfer: broadValue,
      resolution: "retain-both-route-identity-is-strict-recipient-scope-remains-packet-gated",
    });
  }
  return uniqueObjects(conflicts);
}

function normalizeSourceRecord(record) {
  const normalized = structuredClone(record);
  if (Array.isArray(normalized.effectComponents)) normalized.effectComponents = normalized.effectComponents.map(normalizeTransferRoute);
  if (Array.isArray(normalized.attributionModel?.components)) normalized.attributionModel.components = normalized.attributionModel.components.map(normalizeTransferRoute);
  return normalized;
}

function normalizeTransferRoute(route) {
  const normalized = structuredClone(route);
  const declared = String(normalized.transferEligibility ?? "");
  if (!declared) return normalized;
  normalized.declaredTransferEligibility = declared;
  if (declared === "self-only-formula-context") {
    normalized.transferEligibility = "owner-local-formula-context-recipient-scope-open";
    normalized.recipientScopeProofState = "open-owner-local-context-is-not-recipient-proof";
  } else {
    normalized.recipientScopeProofState = declared.includes("external-")
      ? "open-external-recipient-candidate"
      : "retained-declaration-no-additional-recipient-proof-in-this-artifact";
  }
  return normalized;
}

function declaredTransferEligibility(route) {
  return String(route.declaredTransferEligibility ?? route.transferEligibility ?? "");
}

function buildRecipientScopeProof(formulaEdges, components) {
  const routes = [...formulaEdges, ...components];
  const declared = uniqueText(routes.map(declaredTransferEligibility).filter(Boolean));
  const effective = uniqueText(routes.map((route) => route.transferEligibility).filter(Boolean));
  const reopened = routes.filter((route) => route.transferEligibility === "owner-local-formula-context-recipient-scope-open").length;
  const open = effective.some((value) => value === "owner-local-formula-context-recipient-scope-open" || value.includes("external-recipient") || value.includes("external-target"));
  return {
    declared_transfer_eligibility: declared,
    effective_transfer_eligibility: effective,
    owner_local_context_routes_reopened: reopened,
    exact_self_only_recipient_routes_proven: 0,
    requires_recipient_packet_proof: open,
    proof_state: open ? "recipient-scope-open" : "no-open-recipient-route-in-selected-factor-static-proof",
  };
}

function validateInputs(context, selection, factors, closure, relationships, damageBridge, effectSources, manifest) {
  for (const [label, value] of [["selection route proof", selection], ["factor closure", closure]]) {
    if (String(value.game_build) !== context.build) throw new Error(`${label} build ${value.game_build} does not match ${context.build}`);
  }
  if (selection.proof_state !== "exact-current-build-local-full-snapshot-selection-route-proven-dirty-transition-open") throw new Error("Selection route proof state changed");
  if (selection.content_sha256 !== contentHash(selection)) throw new Error("Selection route proof content hash mismatch");
  if (String(manifest.gameBuild) !== context.build) throw new Error(`Source manifest build ${manifest.gameBuild} does not match ${context.build}`);
  if (!factors.factorsByBuffId || !relationships.sourcesByRuleId || !damageBridge.damageChains || !effectSources.effectSourcesById) throw new Error("A required current-build relationship index is malformed");
  validateManifestFile(manifest, "generated-research:SeasonPhantomFactors.json", context.factorCatalog);
  validateManifestFile(manifest, "generated-research:ModifierRelationshipTable.json", context.relationshipTable);
  validateManifestFile(manifest, "generated-research:SkillDamageChainBridge.json", context.damageChainBridge);
  validateManifestFile(manifest, "generated-research:EffectSources.json", context.effectSources);
}

function validateManifestFile(manifest, id, file) {
  const record = (manifest.files ?? []).find((entry) => entry.id === id);
  if (!record) throw new Error(`Source manifest omits ${id}`);
  const actual = fileDescriptor(file);
  if (Number(record.bytes) !== actual.bytes || String(record.sha256) !== actual.sha256) throw new Error(`${id} does not match complete-build source manifest`);
}

function summarize(obligations) {
  const damageIds = uniqueNumeric(obligations.flatMap((entry) => entry.strict_relationship_routes.target_damage_ids));
  const recountIds = uniqueNumeric(obligations.flatMap((entry) => entry.strict_relationship_routes.target_recount_ids));
  const stateRoutes = obligations.flatMap((entry) => entry.strict_relationship_routes.exact_state_routes);
  return {
    selector_obligations: obligations.length,
    unique_sources: new Set(obligations.map((entry) => entry.source_rule_id)).size,
    grade_item_routes: obligations.reduce((sum, entry) => sum + entry.grade_routes.length, 0),
    strict_unique_damage_ids: damageIds.length,
    strict_unique_recount_ids: recountIds.length,
    strict_exact_state_routes: stateRoutes.length,
    obligations_with_strict_damage_routes: obligations.filter((entry) => entry.strict_relationship_routes.target_damage_ids.length).length,
    obligations_with_strict_recount_routes: obligations.filter((entry) => entry.strict_relationship_routes.target_recount_ids.length).length,
    obligations_with_transfer_conflicts_retained: obligations.filter((entry) => entry.transfer_classification_conflicts.length).length,
    obligations_requiring_recipient_packet_proof: obligations.filter((entry) => entry.recipient_scope_proof.requires_recipient_packet_proof).length,
    owner_local_context_routes_reopened: obligations.reduce((sum, entry) => sum + entry.recipient_scope_proof.owner_local_context_routes_reopened, 0),
    exact_self_only_recipient_routes_proven: 0,
    candidate_only_damage_ids_retained: uniqueNumeric(obligations.flatMap((entry) => entry.candidate_only_damage_ids)).length,
    candidate_only_recount_ids_retained: uniqueNumeric(obligations.flatMap((entry) => entry.candidate_only_recount_ids)).length,
    obligations_with_lifecycle_routes: obligations.filter((entry) => entry.runtime_proof_plan?.lifecycle?.required).length,
    obligations_with_energy_routes: obligations.filter((entry) => entry.runtime_proof_plan?.energy?.required).length,
    energy_grade_routes: obligations.reduce((sum, entry) => sum + (entry.runtime_proof_plan?.energy?.grade_amount_routes?.length ?? 0), 0),
    obligations_with_state_or_skill_mutation_routes: obligations.filter((entry) => entry.runtime_proof_plan?.state_or_skill_mutation?.required).length,
    obligations_with_triggered_output_routes: obligations.filter((entry) => entry.runtime_proof_plan?.triggered_output?.required).length,
    obligations_with_multi_child_allocation: obligations.filter((entry) => entry.runtime_proof_plan?.triggered_output?.multi_child_allocation_required).length,
    obligations_with_stacking_overlap_proof_required: obligations.filter((entry) => entry.runtime_proof_plan?.stacking_overlap?.required).length,
    obligations_with_counterfactual_required: obligations.filter((entry) => entry.runtime_proof_plan?.counterfactual?.required).length,
    obligations_with_conservation_required: obligations.filter((entry) => entry.runtime_proof_plan?.conservation?.required).length,
    runtime_provider_windows_proven: 0,
    observed_event_replays_proven: 0,
    counterfactual_projections_proven: 0,
    conservation_proofs: 0,
    rdps_obligations_promoted: 0,
    hidden_omissions: obligations.reduce((sum, entry) => sum + entry.hidden_omissions, 0),
  };
}

function assertCoverage(obligations) {
  if (obligations.length !== 10) throw new Error(`Selected-factor mechanic coverage changed from 10 to ${obligations.length}`);
  if (new Set(obligations.map((entry) => entry.source_rule_id)).size !== 10) throw new Error("Selected-factor mechanic source coverage changed from 10");
  if (obligations.reduce((sum, entry) => sum + entry.grade_routes.length, 0) !== 100) throw new Error("Selected-factor mechanic grade coverage changed from 100");
  if (obligations.some((entry) => entry.rdps_promoted || entry.hidden_omissions !== 0)) throw new Error("Selected-factor mechanic proof promoted or hid evidence");
}

function verify(input) {
  const report = readJson(input, "selected-factor mechanic route proof");
  if (report.schema_version !== 2 || report.generated_by !== "tools/bpsr-selected-factor-mechanic-route-proof.mjs" || report.proof_state !== "mechanics-candidates-indexed-runtime-proof-open") throw new Error("Invalid selected-factor mechanic route proof schema/generator/state");
  if (report.content_sha256 !== contentHash(report)) throw new Error("Selected-factor mechanic route proof content hash mismatch");
  if (report.policy?.exact_relationship_edges_are_strict_routes !== true || report.policy?.description_and_localized_name_routes_are_candidates_only !== true || report.policy?.catalog_routes_are_retained_without_automatic_promotion !== true || report.policy?.exact_damage_chain_membership_does_not_solve_child_allocation !== true || report.policy?.declared_transfer_classification_is_retained_separately !== true || report.policy?.static_owner_context_does_not_prove_self_only_recipient_scope !== true || report.policy?.effective_transfer_classification_reopens_owner_local_context_without_packet_proof !== true || report.policy?.runtime_obligation_matrix_is_a_proof_plan_not_runtime_proof !== true || report.policy?.proof_receipt_does_not_promote_rdps_obligations !== true || report.policy?.unresolved_evidence_is_never_hidden !== true) throw new Error("Selected-factor mechanic route proof has an unsafe policy");
  assertCoverage(report.blocker_obligations ?? []);
  for (const entry of report.blocker_obligations) {
    const strictDamage = entry.strict_relationship_routes?.target_damage_ids ?? [];
    const strictRecount = entry.strict_relationship_routes?.target_recount_ids ?? [];
    if ((entry.candidate_only_damage_ids ?? []).some((id) => strictDamage.includes(id))) throw new Error(`${entry.obligation_id} promotes a candidate-only damage ID`);
    if ((entry.candidate_only_recount_ids ?? []).some((id) => strictRecount.includes(id))) throw new Error(`${entry.obligation_id} promotes a candidate-only recount ID`);
    const routes = [...(entry.strict_relationship_routes?.formula_components ?? []), ...(entry.effect_source_join?.components ?? [])];
    if (routes.some((route) => route.transferEligibility === "self-only-formula-context")) throw new Error(`${entry.obligation_id} leaves a static owner context falsely closed as self-only`);
    for (const route of routes.filter((route) => route.declaredTransferEligibility === "self-only-formula-context")) {
      if (route.transferEligibility !== "owner-local-formula-context-recipient-scope-open") throw new Error(`${entry.obligation_id} does not reopen a declared owner-local context`);
    }
    if (entry.recipient_scope_proof?.requires_recipient_packet_proof !== true || entry.recipient_scope_proof?.exact_self_only_recipient_routes_proven !== 0) throw new Error(`${entry.obligation_id} prematurely closes recipient scope`);
    const plan = entry.runtime_proof_plan;
    if (plan?.proof_state !== "runtime-obligations-indexed-no-runtime-closure" || plan.selection?.required !== true || plan.lifecycle?.required !== true || plan.stacking_overlap?.required !== true || plan.counterfactual?.required !== true || plan.conservation?.required !== true) throw new Error(`${entry.obligation_id} has an incomplete runtime proof plan`);
    if (stableStringify(plan.lifecycle.effect_ids) !== stableStringify([entry.factor_identity.buff_id])) throw new Error(`${entry.obligation_id} lifecycle plan does not retain its selected buff`);
    if (stableStringify(plan.triggered_output.strict_damage_ids) !== stableStringify(strictDamage) || stableStringify(plan.triggered_output.strict_recount_ids) !== stableStringify(strictRecount)) throw new Error(`${entry.obligation_id} runtime output plan diverges from strict relationship routes`);
    if (plan.counterfactual.status !== "integer-counterfactual-open" || plan.conservation.status !== "party-damage-conservation-open") throw new Error(`${entry.obligation_id} prematurely closes runtime arithmetic proof`);
  }
  for (const key of ["runtime_provider_windows_proven", "observed_event_replays_proven", "counterfactual_projections_proven", "conservation_proofs", "rdps_obligations_promoted", "hidden_omissions"]) {
    if (report.summary?.[key] !== 0) throw new Error(`Selected-factor mechanic proof improperly closes ${key}`);
  }
  if (!report.still_required_runtime_gates?.length) throw new Error("Selected-factor mechanic proof omits remaining runtime gates");
  console.log(`Selected-factor mechanic route proof verified for build ${report.game_build}: ${report.summary.selector_obligations} obligations, ${report.summary.strict_unique_damage_ids} strict damage rows, ${report.summary.strict_unique_recount_ids} strict recount rows, zero rDPS promotions.`);
  return report;
}

function selfTest() {
  const root = mkdtempSync(path.join(tmpdir(), "rlogs-selected-factor-mechanics-test-"));
  try {
    const selectionObligation = {
      obligation_id: "mrs:test#selectedFactorGrade", source_rule_id: "mrs:test", source_id: "phantom-factor:9", effect_ids: [9],
      grade_item_routes: Array.from({ length: 10 }, (_, index) => ({ item_id: 100 + index, family_id: 77, grade: index + 1, primary_buff_id: 9 })),
    };
    const result = buildMechanicObligation({
      selectionObligation,
      factors: { factorsByBuffId: { "9": { familyId: 77, familyName: "Test", modifierEvidence: { gradeRows: Array.from({ length: 10 }, (_, index) => ({ grade: index + 1, itemId: 100 + index, parameterValues: [index], cleanResolvedDescription: "candidate" })) }, gradeItems: Array.from({ length: 10 }, (_, index) => ({ grade: index + 1, itemId: 100 + index })), affectedDamageIds: [999], affectedRecountIds: [998] } } },
      closureByFamily: new Map([[77, { family_id: 77, direct_damage_ids: [997], exact_recount_ids: [996] }]]),
      relationships: { sourcesByRuleId: { "mrs:test": { sourceId: "phantom-factor:9", uidEdges: [{ edgeKind: "target-damage-row", uidKind: "damage", uid: 111 }, { edgeKind: "target-recount-row", uidKind: "recount", uid: 12 }] } } },
      damageBridge: { damageChains: { "111": { damageId: 111, recountParents: [12] } }, recountChains: { "12": { id: 12, damageIds: [111], allocationStatus: "single-damage-parent" } } },
      effectSources: { buffIdToEffectSourceIds: { "9": ["phantom-factor:9"] }, effectSourcesById: { "phantom-factor:9": { effectComponents: [{ componentKey: "owner-context", transferEligibility: "self-only-formula-context" }] } } },
      recountTable: { "12": { Id: 12, Name: "Strict" } },
    });
    if (stableStringify(result.strict_relationship_routes.target_damage_ids) !== "[111]" || stableStringify(result.strict_relationship_routes.target_recount_ids) !== "[12]") throw new Error("Self-test failed to retain strict relationship IDs");
    if (stableStringify(result.candidate_only_damage_ids) !== "[997,999]" || stableStringify(result.candidate_only_recount_ids) !== "[996,998]") throw new Error("Self-test promoted or lost candidate-only IDs");
    const obligations = Array.from({ length: 10 }, (_, index) => ({ ...structuredClone(result), obligation_id: `mrs:${index}#selectedFactorMechanics`, source_rule_id: `mrs:${index}`, grade_routes: structuredClone(result.grade_routes), rdps_promoted: false, hidden_omissions: 0 }));
    const report = {
      schema_version: 2, generated_by: "tools/bpsr-selected-factor-mechanic-route-proof.mjs", game_build: "1", proof_state: "mechanics-candidates-indexed-runtime-proof-open",
      policy: { exact_relationship_edges_are_strict_routes: true, description_and_localized_name_routes_are_candidates_only: true, catalog_routes_are_retained_without_automatic_promotion: true, exact_damage_chain_membership_does_not_solve_child_allocation: true, declared_transfer_classification_is_retained_separately: true, static_owner_context_does_not_prove_self_only_recipient_scope: true, effective_transfer_classification_reopens_owner_local_context_without_packet_proof: true, runtime_obligation_matrix_is_a_proof_plan_not_runtime_proof: true, proof_receipt_does_not_promote_rdps_obligations: true, unresolved_evidence_is_never_hidden: true },
      summary: { ...summarize(obligations), runtime_provider_windows_proven: 0, observed_event_replays_proven: 0, counterfactual_projections_proven: 0, conservation_proofs: 0, rdps_obligations_promoted: 0, hidden_omissions: 0 },
      blocker_obligations: obligations, still_required_runtime_gates: ["runtime proof"],
    };
    report.content_sha256 = contentHash(report);
    const output = path.join(root, "proof.json");
    writeJson(output, report);
    verify(output);
    console.log("Selected-factor mechanic route proof self-test passed.");
  } finally { rmSync(root, { recursive: true, force: true }); }
}

function edgeIds(edges, edgeKind, uidKind) { return uniqueNumeric(edges.filter((edge) => edge.edgeKind === edgeKind && edge.uidKind === uidKind).map((edge) => edge.uid)); }
function compareEdges(left, right) { return compareText(`${left.edgeKind}|${left.uidKind}|${left.uid}|${left.source ?? ""}`, `${right.edgeKind}|${right.uidKind}|${right.uid}|${right.source ?? ""}`); }
function asArray(value) { return Array.isArray(value) ? value : Object.values(value ?? {}); }
function uniqueNumeric(values) { return [...new Set(values.map(Number).filter(Number.isFinite))].sort((left, right) => left - right); }
function uniqueText(values) { return [...new Set(values.map(String))].sort(compareText); }
function uniqueObjects(values) { const seen = new Set(); return values.filter((value) => { const key = stableStringify(value); if (seen.has(key)) return false; seen.add(key); return true; }); }
function requireFile(file, label) { if (!existsSync(file) || !statSync(file).isFile()) throw new Error(`Missing ${label}: ${file}`); }
function fileDescriptor(file) { return { path: file.replaceAll("\\", "/"), bytes: statSync(file).size, sha256: hashFile(file) }; }
function contentHash(value) { const clone = structuredClone(value); delete clone.content_sha256; return hashText(stableStringify(clone)); }
function stableStringify(value) { if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`; if (value && typeof value === "object") return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`; return JSON.stringify(value); }
function hashFile(file) { return createHash("sha256").update(readFileSync(file)).digest("hex"); }
function hashText(value) { return createHash("sha256").update(value).digest("hex"); }
function readJson(file, label) { try { return JSON.parse(readFileSync(file, "utf8")); } catch (error) { throw new Error(`Unable to read ${label} ${file}: ${error.message}`); } }
function writeJson(file, value) { writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`, "utf8"); }
function compareText(left, right) { return String(left).localeCompare(String(right), "en"); }
function parseArgs(args) { const parsed = {}; for (let index = 0; index < args.length; index += 1) { const arg = args[index]; if (!arg.startsWith("--")) throw new Error(`Unexpected argument ${arg}`); const key = arg.slice(2); const value = args[index + 1]; if (value === undefined || value.startsWith("--")) throw new Error(`Missing value for --${key}`); parsed[key] = value; index += 1; } return parsed; }
function required(value, key) { if (!value[key]) throw new Error(`Missing --${key}`); return value[key]; }
function usage(exitCode) { console.log("Usage:\n  node tools/bpsr-selected-factor-mechanic-route-proof.mjs build --build <id> --selection-route-proof <json> --factor-catalog <json> --closure <json> --relationship-table <json> --damage-chain-bridge <json> --effect-sources <json> --recount-table <json> --source-manifest <json> --output <json>\n  node tools/bpsr-selected-factor-mechanic-route-proof.mjs verify --input <json>\n  node tools/bpsr-selected-factor-mechanic-route-proof.mjs self-test"); process.exit(exitCode); }
