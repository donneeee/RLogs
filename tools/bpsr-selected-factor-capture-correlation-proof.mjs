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
  const mechanicProof = parsed["mechanic-proof"] ? path.resolve(parsed["mechanic-proof"]) : null;
  const factorClosure = parsed["factor-closure"] ? path.resolve(parsed["factor-closure"]) : null;
  if (Boolean(mechanicProof) === Boolean(factorClosure)) throw new Error("Provide exactly one of --mechanic-proof or --factor-closure");
  return {
    build: buildId,
    mechanicProof,
    factorClosure,
    correlationBundles: asArray(required(parsed, "correlation-bundle")).map((file) => path.resolve(file)),
    output: path.resolve(required(parsed, "output")),
  };
}

function build(context) {
  const fullCatalog = Boolean(context.factorClosure);
  const sourceFile = fullCatalog ? context.factorClosure : context.mechanicProof;
  requireFile(sourceFile, fullCatalog ? "psychoscope factor offline closure" : "selected-factor mechanic proof");
  for (const file of context.correlationBundles) requireFile(file, "factor correlation bundle");
  const source = readJson(sourceFile, fullCatalog ? "psychoscope factor offline closure" : "selected-factor mechanic proof");
  const mechanics = fullCatalog ? mechanicsFromFactorClosure(source, context.build) : source.blocker_obligations;
  if (fullCatalog) validateFactorClosure(source, context.build);
  else validateMechanicProof(source, context.build);
  const bundles = context.correlationBundles.map((file) => ({ file, value: readJson(file, "factor correlation bundle") }));
  const reports = bundles.flatMap(({ file, value }) => validateCorrelationBundle(value, file, context.build)
    .map((report) => ({
      ...report,
      _correlation_bundle_path: file.replaceAll("\\", "/"),
      _correlation_bundle_sha256: hashFile(file),
    })));
  const obligations = mechanics.map((entry) => correlateObligation(entry, reports))
    .sort((left, right) => compareText(left.obligation_id, right.obligation_id));
  assertCoverage(obligations, mechanics.length);

  const report = {
    schema_version: fullCatalog ? 3 : 2,
    generated_by: "tools/bpsr-selected-factor-capture-correlation-proof.mjs",
    game_build: context.build,
    scope_kind: fullCatalog ? "full-current-runtime-factor-catalog" : "selected-factor-mechanic-worklist",
    proof_state: "canonical-capture-correlation-observed-runtime-gates-open",
    policy: {
      catalog_build_and_observed_event_build_are_distinct: true,
      observed_event_build_must_exactly_match_requested_build: true,
      packet_absence_is_negative_coverage_not_mechanic_disproof: true,
      exact_selection_and_lifecycle_are_retained_separately: true,
      selection_at_report_boundary_is_not_a_trigger_opportunity: true,
      adjacent_report_carry_requires_exact_lineage_time_build_digest_and_owner: true,
      no_runtime_gate_closed_without_exact_owner_binding: true,
      capture_correlation_does_not_prove_counterfactual_or_conservation: true,
      resource_id_and_value_arrays_are_retained_separately: true,
      resource_meanings_and_units_require_packet_proof: true,
      resource_correlation_does_not_promote_rdps: true,
      proof_receipt_does_not_promote_rdps_obligations: true,
      unresolved_evidence_is_never_hidden: true,
    },
    inputs: {
      ...(fullCatalog
        ? { psychoscope_factor_offline_closure: fileDescriptor(context.factorClosure) }
        : { selected_factor_mechanic_route_proof: fileDescriptor(context.mechanicProof) }),
      factor_correlation_bundles: context.correlationBundles.map(fileDescriptor),
    },
    correlation_contract: {
      catalog_build_route: "reports[].game_build",
      observed_event_build_route: "reports[].observed_client_builds",
      observed_protocol_pack_digest_route: "reports[].observed_protocol_pack_digests",
      selection_route: "reports[].selection_observations[].selected_factor_item_ids",
      trigger_coverage_route: "selection.observed_micros compared with its report and one exactly contiguous next report",
      effect_lifecycle_route: "reports[].windows[] matched by effect_id or factor_item_ids",
      owner_binding_route: "same-report analyzer ownership or exact BPSR entity_uuid>>16 character UID across one contiguous report boundary",
      report_continuation_route: "same bundle, same session lineage, adjacent run number, equal boundary timestamp, exact packet build, exact protocol digest",
      emitted_action_route: "reports[].windows[].action_damage[] matched by ability_id or recount_group_id",
      provider_recipient_route: "matching windows retain provider_entity_uuid and recipient_entity_uuid",
      resource_baseline_route: "reports[].windows[].resource_baselines[]",
      resource_transition_route: "reports[].windows[].resource_transitions[]",
      negative_coverage_is_bounded_to_supplied_bundles: true,
    },
    summary: summarize(obligations, bundles.length, reports.length),
    blocker_obligations: obligations,
    still_required_runtime_gates: uniqueText(obligations.flatMap((entry) => entry.still_required_runtime_gates)),
  };
  report.content_sha256 = contentHash(report);
  mkdirSync(path.dirname(context.output), { recursive: true });
  writeJson(context.output, report);
  verify(context.output);
  console.log(`Factor capture correlation built for ${context.build}: ${reports.length} sealed reports, ${obligations.length} factors, ${report.summary.selection_observations} selected-grade observations, ${report.summary.lifecycle_windows} matching lifecycle windows, zero rDPS promotions.`);
}

function mechanicsFromFactorClosure(closure, buildId) {
  validateFactorClosure(closure, buildId);
  return closure.families.filter((family) => family.current_runtime_eligible === true).map((family) => {
    const sourceBuffIds = uniqueNumeric(family.source_buff_ids ?? []).filter((id) => id > 0);
    if (sourceBuffIds.length > 1) throw new Error(`Current factor family ${family.family_id} has multiple source effects and requires an explicit route split`);
    const effectId = sourceBuffIds[0] ?? null;
    return {
      obligation_id: `psychoscope-family:${family.family_id}#fullCatalogMechanics`,
      source_rule_id: `psychoscope-family:${family.family_id}`,
      source_id: effectId === null ? `psychoscope-family:${family.family_id}` : `phantom-factor:${effectId}`,
      factor_identity: {
        family_id: Number(family.family_id),
        buff_id: effectId,
        name: String(family.family_name ?? `Factor family ${family.family_id}`),
        slot_category: family.slot_category ?? null,
        runtime_role: family.runtime_role ?? null,
      },
      grade_routes: structuredClone(family.grade_routes ?? []),
      strict_relationship_routes: {
        target_damage_ids: uniqueNumeric(family.direct_damage_ids ?? []),
        target_recount_ids: uniqueNumeric(family.exact_recount_ids ?? []),
      },
      catalog_routes: {
        source_buff_ids: sourceBuffIds,
        mechanic_classes: structuredClone(family.mechanic_classes ?? []),
        generated_damage_families: structuredClone(family.generated_damage_families ?? []),
        generated_output_families: structuredClone(family.generated_output_families ?? []),
        state_routes: structuredClone(family.state_routes ?? []),
        runtime_selectors: structuredClone(family.runtime_selectors ?? []),
        energy_behaviors: structuredClone(family.energy_behaviors ?? []),
        final_validation_obligations: structuredClone(family.final_validation_obligations ?? []),
      },
    };
  });
}

function correlateObligation(mechanic, reports) {
  const gradeItemIds = uniqueNumeric((mechanic.grade_routes ?? []).map((entry) => entry.item_id));
  const gradeSet = new Set(gradeItemIds);
  const rawEffectId = mechanic.factor_identity?.buff_id;
  const effectId = rawEffectId === null || rawEffectId === undefined ? null : Number(rawEffectId);
  const hasEffectRoute = Number.isFinite(effectId) && effectId > 0;
  const strictDamageIds = uniqueNumeric(mechanic.strict_relationship_routes?.target_damage_ids ?? []);
  const strictDamageSet = new Set(strictDamageIds);
  const strictRecountIds = uniqueNumeric(mechanic.strict_relationship_routes?.target_recount_ids ?? []);
  const strictRecountSet = new Set(strictRecountIds);
  const selectionObservations = [];
  const lifecycleWindows = [];
  const ruleSummaries = [];
  const actionMatches = [];
  const continuationEdges = buildContinuationEdges(reports);
  const reportsBySession = new Map(reports.map((report) => [String(report.session_id ?? ""), report]));

  for (const runtimeReport of reports) {
    const sessionId = String(runtimeReport.session_id ?? "");
    for (const observation of runtimeReport.selection_observations ?? []) {
      const matchedItemIds = uniqueNumeric((observation.selected_factor_item_ids ?? []).filter((itemId) => gradeSet.has(Number(itemId))));
      if (!matchedItemIds.length) continue;
      const continuation = continuationEdges.get(sessionId) ?? null;
      const sourceReportLastObservedMicros = Number(runtimeReport.last_observed_micros ?? 0);
      const continuedReportLastObservedMicros = continuation
        ? Number(reportsBySession.get(continuation.target_session_id)?.last_observed_micros ?? sourceReportLastObservedMicros)
        : sourceReportLastObservedMicros;
      selectionObservations.push({
        session_id: sessionId,
        sequence: Number(observation.sequence ?? 0),
        observed_micros: Number(observation.observed_micros ?? 0),
        report_last_observed_micros: sourceReportLastObservedMicros,
        continued_into_session_id: continuation?.target_session_id ?? null,
        continued_report_last_observed_micros: continuation ? continuedReportLastObservedMicros : null,
        post_selection_observation_micros: Math.max(0, continuedReportLastObservedMicros - Number(observation.observed_micros ?? 0)),
        character_id: String(observation.character_id ?? ""),
        matched_factor_item_ids: matchedItemIds,
      });
    }
    for (const window of runtimeReport.windows ?? []) {
      const matchedFactorItemIds = uniqueNumeric((window.factor_item_ids ?? []).filter((itemId) => gradeSet.has(Number(itemId))));
      const matchesLifecycle = (hasEffectRoute && Number(window.effect_id) === effectId) || matchedFactorItemIds.length > 0;
      if (matchesLifecycle) lifecycleWindows.push(compactWindow(sessionId, window, matchedFactorItemIds));
      if (!matchesLifecycle) continue;
      for (const action of window.action_damage ?? []) {
        const abilityId = Number(action.ability_id ?? 0);
        const recountId = Number(action.recount_group_id ?? 0);
        if (!strictDamageSet.has(abilityId) && !strictRecountSet.has(recountId)) continue;
        actionMatches.push({
          session_id: sessionId,
          window_id: String(window.window_id ?? ""),
          effect_id: Number(window.effect_id ?? 0),
          ability_id: abilityId || null,
          recount_group_id: recountId || null,
          relation_kind: action.relation_kind ?? null,
          action_role: action.action_role ?? null,
          actor_relation: action.actor_relation ?? null,
          event_count: Number(action.totals?.event_count ?? 0),
          amount: Number(action.totals?.amount ?? 0),
          first_observed_micros: numberOrNull(action.totals?.first_observed_micros),
          last_observed_micros: numberOrNull(action.totals?.last_observed_micros),
        });
      }
    }
    for (const summary of runtimeReport.rule_summaries ?? []) {
      if ((!hasEffectRoute || Number(summary.effect_id) !== effectId) && !gradeSet.has(Number(summary.factor_item_id))) continue;
      ruleSummaries.push({ session_id: sessionId, ...structuredClone(summary) });
    }
  }

  const exactOwnerBindings = [];
  for (const selection of selectionObservations) {
    for (const window of lifecycleWindows) {
      const sameReport = window.session_id === selection.session_id;
      const continuation = continuationEdges.get(selection.session_id);
      const adjacentReport = continuation?.target_session_id === window.session_id;
      if (!sameReport && !adjacentReport) continue;
      const analyzerOwnerBound = sameReport
        && selection.sequence <= window.opened_sequence
        && (window.selected_owner_character_ids ?? []).includes(selection.character_id);
      const providerOwnerBound = window.provider_character_id === selection.character_id
        && (adjacentReport || selection.sequence <= window.opened_sequence);
      if (!analyzerOwnerBound && !providerOwnerBound) continue;
      if (adjacentReport && hasSupersedingSelection(reportsBySession.get(window.session_id), selection, window)) continue;
      exactOwnerBindings.push({
        selection_session_id: selection.session_id,
        window_session_id: window.session_id,
        character_id: selection.character_id,
        selection_sequence: selection.sequence,
        window_id: window.window_id,
        effect_id: window.effect_id,
        boundary_observed_micros: adjacentReport ? continuation.boundary_observed_micros : null,
        binding_evidence: adjacentReport
          ? "adjacent-report-provider-entity-uuid-character-uid"
          : analyzerOwnerBound
            ? "same-report-analyzer-selected-owner"
            : "same-report-provider-entity-uuid-character-uid",
      });
    }
  }
  const distinctProviderRecipientWindows = lifecycleWindows.filter((window) => window.provider_entity_uuid && window.recipient_entity_uuid && window.provider_entity_uuid !== window.recipient_entity_uuid);
  const observedDamageIds = uniqueNumeric(actionMatches.map((entry) => entry.ability_id).filter(Boolean));
  const observedRecountIds = uniqueNumeric(actionMatches.map((entry) => entry.recount_group_id).filter(Boolean));
  const resourceTransitionEvidence = lifecycleWindows.flatMap((window) =>
    (window.resource_transitions ?? []).map((transition) => ({
      session_id: window.session_id,
      window_id: window.window_id,
      effect_id: window.effect_id,
      ...structuredClone(transition),
    })));
  const hasEnergyRoute = (mechanic.catalog_routes?.energy_behaviors ?? []).length > 0;
  const postSelectionCoverageCount = selectionObservations.filter((entry) => entry.post_selection_observation_micros > 0).length;
  const coverageState = classifyCoverage(selectionObservations.length, postSelectionCoverageCount, lifecycleWindows.length, exactOwnerBindings.length);
  const stillRequiredRuntimeGates = buildOpenGates({ mechanic, hasEffectRoute, hasEnergyRoute, selectionObservations, lifecycleWindows, exactOwnerBindings, observedDamageIds, observedRecountIds, resourceTransitionEvidence });
  return {
    obligation_id: `${mechanic.source_rule_id}#selectedFactorCaptureCorrelation`,
    mechanic_obligation_id: mechanic.obligation_id,
    source_rule_id: mechanic.source_rule_id,
    source_id: mechanic.source_id,
    factor_identity: structuredClone(mechanic.factor_identity),
    catalog_routes: structuredClone(mechanic.catalog_routes ?? null),
    expected_routes: {
      grade_item_ids: gradeItemIds,
      effect_id: effectId,
      source_state_route: hasEffectRoute ? "status-effect-lifecycle" : "attribute-or-state-transition",
      strict_damage_ids: strictDamageIds,
      strict_recount_ids: strictRecountIds,
    },
    coverage_state: coverageState,
    selection_observations: selectionObservations.sort(compareObservation),
    lifecycle_windows: lifecycleWindows.sort(compareWindow),
    rule_summaries: ruleSummaries.sort((left, right) => compareText(`${left.session_id}:${left.effect_id}:${left.factor_item_id}`, `${right.session_id}:${right.effect_id}:${right.factor_item_id}`)),
    exact_owner_bindings: exactOwnerBindings.sort((left, right) => compareText(`${left.selection_session_id}:${left.window_session_id}:${left.window_id}`, `${right.selection_session_id}:${right.window_session_id}:${right.window_id}`)),
    emitted_action_matches: actionMatches.sort(compareAction),
    resource_transition_evidence: resourceTransitionEvidence.sort(compareResourceTransition),
    observed_strict_damage_ids: observedDamageIds,
    observed_strict_recount_ids: observedRecountIds,
    provider_recipient_evidence: {
      lifecycle_windows: lifecycleWindows.length,
      distinct_provider_recipient_windows: distinctProviderRecipientWindows.length,
      self_provider_recipient_windows: lifecycleWindows.filter((window) => window.provider_entity_uuid && window.provider_entity_uuid === window.recipient_entity_uuid).length,
      unknown_provider_or_recipient_windows: lifecycleWindows.filter((window) => !window.provider_entity_uuid || !window.recipient_entity_uuid).length,
    },
    lifecycle_totals: {
      apply_count: sum(lifecycleWindows, "apply_count"),
      refresh_count: sum(lifecycleWindows, "refresh_count"),
      stack_count: sum(lifecycleWindows, "stack_count"),
      consume_count: sum(lifecycleWindows, "consume_count"),
      remove_count: sum(lifecycleWindows, "remove_count"),
      emitted_action_event_count: actionMatches.reduce((total, entry) => total + entry.event_count, 0),
      emitted_action_amount: actionMatches.reduce((total, entry) => total + entry.amount, 0),
      resource_transition_count: resourceTransitionEvidence.length,
      resource_origin_energy_change_count: countTrue(resourceTransitionEvidence, "origin_energy_changed"),
      resource_ids_change_count: countTrue(resourceTransitionEvidence, "resource_ids_changed"),
      resource_values_change_count: countTrue(resourceTransitionEvidence, "resource_values_changed"),
      resource_cooldowns_change_count: countTrue(resourceTransitionEvidence, "cooldowns_changed"),
      resource_incomplete_state_after_count: resourceTransitionEvidence.filter((entry) => entry.state_after === null || entry.state_after === undefined).length,
    },
    negative_coverage: {
      supplied_capture_reports: reports.length,
      selected_grade_not_observed: selectionObservations.length === 0,
      selected_grade_observed_only_at_report_boundary: selectionObservations.length > 0 && postSelectionCoverageCount === 0,
      trigger_opportunity_after_selection_not_observed: selectionObservations.length > 0 && postSelectionCoverageCount === 0,
      effect_lifecycle_not_observed: hasEffectRoute ? lifecycleWindows.length === 0 : null,
      attribute_or_state_transition_not_observed: hasEffectRoute ? null : true,
      resource_transition_not_observed: hasEnergyRoute ? resourceTransitionEvidence.length === 0 : null,
      strict_damage_ids_not_observed: strictDamageIds.filter((id) => !observedDamageIds.includes(id)),
      strict_recount_ids_not_observed: strictRecountIds.filter((id) => !observedRecountIds.includes(id)),
      scope: "only-the-supplied-sealed-canonical-capture-reports",
    },
    runtime_provider_windows_proven: 0,
    counterfactual_projections_proven: 0,
    conservation_proofs: 0,
    rdps_promoted: false,
    hidden_omissions: 0,
    still_required_runtime_gates: stillRequiredRuntimeGates,
  };
}

function compactWindow(sessionId, window, matchedFactorItemIds) {
  return {
    session_id: sessionId,
    window_id: String(window.window_id ?? ""),
    effect_id: Number(window.effect_id ?? 0),
    instance_id: String(window.instance_id ?? ""),
    matched_factor_item_ids: matchedFactorItemIds,
    selected_owner_character_ids: uniqueText((window.selected_owner_character_ids ?? []).map(String)),
    provider_entity_uuid: String(window.provider_entity_uuid ?? ""),
    provider_character_id: characterIdFromEntityUuid(window.provider_entity_uuid),
    recipient_entity_uuid: String(window.recipient_entity_uuid ?? ""),
    opened_sequence: Number(window.opened_sequence ?? 0),
    opened_observed_micros: Number(window.opened_observed_micros ?? 0),
    closed_sequence: numberOrNull(window.closed_sequence),
    closed_observed_micros: numberOrNull(window.closed_observed_micros),
    close_reason: window.close_reason ?? null,
    min_stacks: Number(window.min_stacks ?? 0),
    max_stacks: Number(window.max_stacks ?? 0),
    apply_count: Number(window.apply_count ?? 0),
    refresh_count: Number(window.refresh_count ?? 0),
    stack_count: Number(window.stack_count ?? 0),
    consume_count: Number(window.consume_count ?? 0),
    remove_count: Number(window.remove_count ?? 0),
    resource_baselines: structuredClone(window.resource_baselines ?? []),
    resource_transitions: structuredClone(window.resource_transitions ?? []),
  };
}

function buildContinuationEdges(reports) {
  const parsed = reports.map((report) => ({ report, lineage: parseRunLineage(report.session_id) }))
    .filter((entry) => entry.lineage !== null);
  const byIdentity = new Map();
  for (const entry of parsed) {
    const key = `${entry.report._correlation_bundle_sha256}:${entry.lineage.root}:${entry.lineage.run}`;
    if (byIdentity.has(key)) throw new Error(`Ambiguous sealed report lineage ${entry.report.session_id}`);
    byIdentity.set(key, entry.report);
  }
  const edges = new Map();
  for (const entry of parsed) {
    const next = byIdentity.get(`${entry.report._correlation_bundle_sha256}:${entry.lineage.root}:${entry.lineage.run + 1}`);
    if (!next) continue;
    const sourceSessionId = String(entry.report.session_id ?? "");
    const targetSessionId = String(next.session_id ?? "");
    const boundary = Number(entry.report.last_observed_micros ?? -1);
    if (!Number.isSafeInteger(boundary) || boundary < 0 || boundary !== Number(next.first_observed_micros ?? -2)) continue;
    if (stableStringify(uniqueText(entry.report.observed_client_builds ?? [])) !== stableStringify(uniqueText(next.observed_client_builds ?? []))) continue;
    if (stableStringify(uniqueText(entry.report.observed_protocol_pack_digests ?? [])) !== stableStringify(uniqueText(next.observed_protocol_pack_digests ?? []))) continue;
    edges.set(sourceSessionId, {
      source_session_id: sourceSessionId,
      target_session_id: targetSessionId,
      boundary_observed_micros: boundary,
      correlation_bundle_sha256: entry.report._correlation_bundle_sha256,
    });
  }
  return edges;
}

function parseRunLineage(sessionId) {
  const match = /^(.*)\.run-(\d+)$/.exec(String(sessionId ?? ""));
  if (!match) return null;
  const run = Number(match[2]);
  return Number.isSafeInteger(run) ? { root: match[1], run } : null;
}

function hasSupersedingSelection(report, carriedSelection, window) {
  return (report?.selection_observations ?? []).some((observation) =>
    String(observation.character_id ?? "") === carriedSelection.character_id
    && Number(observation.observed_micros ?? 0) <= window.opened_observed_micros);
}

function characterIdFromEntityUuid(value) {
  const text = String(value ?? "").trim();
  if (!/^\d+$/.test(text)) return null;
  const entityUuid = BigInt(text);
  const characterId = entityUuid >> 16n;
  return characterId > 0n ? characterId.toString() : null;
}

function buildOpenGates({ mechanic, hasEffectRoute, hasEnergyRoute, selectionObservations, lifecycleWindows, exactOwnerBindings, observedDamageIds, observedRecountIds, resourceTransitionEvidence }) {
  const gates = new Set();
  if (!selectionObservations.length) gates.add("selected-grade-observation-in-sealed-canonical-capture");
  if (hasEffectRoute) {
    if (!lifecycleWindows.length) gates.add("source-effect-lifecycle-in-sealed-canonical-capture");
    if (!exactOwnerBindings.length) gates.add("exact-selected-owner-to-effect-window-binding");
  } else {
    gates.add("attribute-or-state-transition-in-sealed-canonical-capture");
    gates.add("exact-selected-owner-to-attribute-state-binding");
  }
  const missingDamage = (mechanic.strict_relationship_routes?.target_damage_ids ?? []).filter((id) => !observedDamageIds.includes(Number(id)));
  const missingRecount = (mechanic.strict_relationship_routes?.target_recount_ids ?? []).filter((id) => !observedRecountIds.includes(Number(id)));
  if (missingDamage.length) gates.add("strict-emitted-damage-event-observation");
  if (missingRecount.length) gates.add("strict-recount-parent-event-observation");
  if (hasEnergyRoute) {
    if (!resourceTransitionEvidence.length) gates.add("packet-resource-transition-in-sealed-canonical-capture");
    gates.add("exact-resource-identity-direction-and-unit-proof");
  } else if (resourceTransitionEvidence.length) {
    gates.add("unexpected-resource-transition-mechanic-review");
  }
  gates.add("exact-provider-and-recipient-ownership-at-dependent-event");
  gates.add("integer-counterfactual-projection");
  gates.add("party-damage-conservation");
  return [...gates].sort(compareText);
}

function classifyCoverage(selectionCount, postSelectionCoverageCount, windowCount, bindingCount) {
  if (!selectionCount && !windowCount) return "no-selected-grade-or-runtime-effect-observed";
  if (selectionCount && !postSelectionCoverageCount && !windowCount) return "selection-observed-after-trigger-coverage-ended";
  if (selectionCount && !windowCount) return "selection-observed-effect-not-observed";
  if (!selectionCount && windowCount) return "effect-observed-selection-not-bound";
  if (!bindingCount) return "selection-and-effect-observed-not-exact-owner-bound";
  return "selection-and-effect-correlated-runtime-projection-open";
}

function summarize(obligations, bundleCount, reportCount) {
  const states = Object.fromEntries([...new Set(obligations.map((entry) => entry.coverage_state))].sort(compareText).map((state) => [state, obligations.filter((entry) => entry.coverage_state === state).length]));
  return {
    correlation_bundles: bundleCount,
    sealed_capture_reports: reportCount,
    selector_obligations: obligations.length,
    unique_sources: new Set(obligations.map((entry) => entry.source_rule_id)).size,
    selection_observations: obligations.reduce((total, entry) => total + entry.selection_observations.length, 0),
    selection_observations_at_report_boundary: obligations.reduce((total, entry) => total + entry.selection_observations.filter((observation) => observation.post_selection_observation_micros === 0).length, 0),
    lifecycle_windows: obligations.reduce((total, entry) => total + entry.lifecycle_windows.length, 0),
    exact_owner_bindings: obligations.reduce((total, entry) => total + entry.exact_owner_bindings.length, 0),
    adjacent_report_owner_bindings: obligations.reduce((total, entry) => total + entry.exact_owner_bindings.filter((binding) => binding.selection_session_id !== binding.window_session_id).length, 0),
    emitted_action_matches: obligations.reduce((total, entry) => total + entry.emitted_action_matches.length, 0),
    resource_transitions: obligations.reduce((total, entry) => total + entry.resource_transition_evidence.length, 0),
    resource_origin_energy_changes: obligations.reduce((total, entry) => total + entry.lifecycle_totals.resource_origin_energy_change_count, 0),
    resource_id_changes: obligations.reduce((total, entry) => total + entry.lifecycle_totals.resource_ids_change_count, 0),
    resource_value_changes: obligations.reduce((total, entry) => total + entry.lifecycle_totals.resource_values_change_count, 0),
    resource_cooldown_changes: obligations.reduce((total, entry) => total + entry.lifecycle_totals.resource_cooldowns_change_count, 0),
    distinct_provider_recipient_windows: obligations.reduce((total, entry) => total + entry.provider_recipient_evidence.distinct_provider_recipient_windows, 0),
    coverage_states: states,
    negative_coverage_obligations: obligations.filter((entry) => entry.coverage_state === "no-selected-grade-or-runtime-effect-observed").length,
    runtime_provider_windows_proven: 0,
    counterfactual_projections_proven: 0,
    conservation_proofs: 0,
    rdps_obligations_promoted: 0,
    hidden_omissions: obligations.reduce((total, entry) => total + entry.hidden_omissions, 0),
  };
}

function validateMechanicProof(proof, buildId) {
  if (String(proof.game_build) !== buildId || proof.schema_version !== 2 || proof.generated_by !== "tools/bpsr-selected-factor-mechanic-route-proof.mjs" || proof.proof_state !== "mechanics-candidates-indexed-runtime-proof-open") throw new Error("Selected-factor mechanic proof is incompatible");
  if (proof.content_sha256 !== contentHash(proof)) throw new Error("Selected-factor mechanic proof content hash mismatch");
  if (proof.policy?.static_owner_context_does_not_prove_self_only_recipient_scope !== true || proof.policy?.effective_transfer_classification_reopens_owner_local_context_without_packet_proof !== true || proof.policy?.proof_receipt_does_not_promote_rdps_obligations !== true || proof.policy?.unresolved_evidence_is_never_hidden !== true) throw new Error("Selected-factor mechanic proof has unsafe policy");
}

function validateFactorClosure(closure, buildId) {
  if (String(closure.game_build) !== buildId || closure.schema_version !== 1 || closure.generated_by !== "tools/psychoscope-factor-closure.mjs" || !Array.isArray(closure.families)) throw new Error("Psychoscope factor offline closure is incompatible");
  if (closure.policy?.descriptions_identify_candidates_only !== true || closure.policy?.exact_ids_or_packet_events_are_runtime_authority !== true || closure.policy?.unmatched_evidence_hidden !== false || closure.policy?.guessed_recount_relationships_allowed !== false || closure.policy?.capture_gate_is_global_not_factor_specific !== true) throw new Error("Psychoscope factor offline closure has unsafe policy");
  const current = closure.families.filter((family) => family.current_runtime_eligible === true);
  if (!current.length || Number(closure.summary?.current_runtime_families) !== current.length) throw new Error("Psychoscope factor current-runtime family coverage mismatch");
  if (new Set(current.map((family) => Number(family.family_id))).size !== current.length) throw new Error("Psychoscope factor current-runtime family IDs are not unique");
}

function validateCorrelationBundle(bundle, file, buildId) {
  if (Number(bundle.schema_version) < 6 || !Array.isArray(bundle.reports)) throw new Error(`Unsupported factor correlation bundle ${file}; schema 6 or newer is required for packet-build provenance`);
  for (const report of bundle.reports) {
    if (Number(report.schema_version) < 6) throw new Error(`Correlation report ${report.session_id} lacks packet-build provenance`);
    if (String(report.game_build) !== buildId) throw new Error(`Correlation report ${report.session_id} catalog build ${report.game_build} does not match ${buildId}`);
    const observedBuilds = uniqueText(report.observed_client_builds ?? []).filter(Boolean);
    if (observedBuilds.length !== 1 || observedBuilds[0] !== buildId) {
      throw new Error(`Correlation report ${report.session_id} observed packet build(s) ${observedBuilds.join(",") || "none"} do not exactly match ${buildId}`);
    }
    const observedDigests = uniqueText(report.observed_protocol_pack_digests ?? []).filter(Boolean);
    if (!observedDigests.length) throw new Error(`Correlation report ${report.session_id} has no observed protocol-pack digest`);
    if (report.rdps_attribution_enabled !== false) throw new Error(`Correlation report ${report.session_id} unexpectedly enables rDPS attribution`);
  }
  return bundle.reports;
}

function assertCoverage(obligations, expected) {
  if (obligations.length !== expected || new Set(obligations.map((entry) => entry.source_rule_id)).size !== expected) throw new Error("Selected-factor capture correlation source coverage mismatch");
  if (obligations.some((entry) => entry.rdps_promoted || entry.hidden_omissions !== 0 || !entry.still_required_runtime_gates.length)) throw new Error("Selected-factor capture correlation promoted, hid, or prematurely closed evidence");
}

function verify(input) {
  const report = readJson(input, "selected-factor capture correlation proof");
  if (![2, 3].includes(report.schema_version) || report.generated_by !== "tools/bpsr-selected-factor-capture-correlation-proof.mjs" || report.proof_state !== "canonical-capture-correlation-observed-runtime-gates-open") throw new Error("Invalid factor capture correlation schema/generator/state");
  if (report.schema_version === 2 && (report.scope_kind !== "selected-factor-mechanic-worklist" || !report.inputs?.selected_factor_mechanic_route_proof)) throw new Error("Selected-factor capture correlation scope/input mismatch");
  if (report.schema_version === 3 && (report.scope_kind !== "full-current-runtime-factor-catalog" || !report.inputs?.psychoscope_factor_offline_closure)) throw new Error("Full factor capture correlation scope/input mismatch");
  if (report.content_sha256 !== contentHash(report)) throw new Error("Selected-factor capture correlation content hash mismatch");
  if (report.policy?.catalog_build_and_observed_event_build_are_distinct !== true || report.policy?.observed_event_build_must_exactly_match_requested_build !== true || report.policy?.packet_absence_is_negative_coverage_not_mechanic_disproof !== true || report.policy?.exact_selection_and_lifecycle_are_retained_separately !== true || report.policy?.selection_at_report_boundary_is_not_a_trigger_opportunity !== true || report.policy?.adjacent_report_carry_requires_exact_lineage_time_build_digest_and_owner !== true || report.policy?.no_runtime_gate_closed_without_exact_owner_binding !== true || report.policy?.capture_correlation_does_not_prove_counterfactual_or_conservation !== true || report.policy?.proof_receipt_does_not_promote_rdps_obligations !== true || report.policy?.unresolved_evidence_is_never_hidden !== true) throw new Error("Selected-factor capture correlation has unsafe policy");
  if (report.correlation_contract?.catalog_build_route !== "reports[].game_build" || report.correlation_contract?.observed_event_build_route !== "reports[].observed_client_builds" || report.correlation_contract?.observed_protocol_pack_digest_route !== "reports[].observed_protocol_pack_digests" || !String(report.correlation_contract?.owner_binding_route ?? "").includes("entity_uuid>>16")) throw new Error("Selected-factor capture correlation lacks exact packet-build or owner provenance routes");
  assertCoverage(report.blocker_obligations ?? [], report.summary?.selector_obligations ?? -1);
  const expected = summarize(report.blocker_obligations, report.summary.correlation_bundles, report.summary.sealed_capture_reports);
  if (stableStringify(expected) !== stableStringify(report.summary)) throw new Error("Selected-factor capture correlation summary mismatch");
  for (const key of ["runtime_provider_windows_proven", "counterfactual_projections_proven", "conservation_proofs", "rdps_obligations_promoted", "hidden_omissions"]) if (report.summary[key] !== 0) throw new Error(`Capture correlation improperly closes ${key}`);
  console.log(`Factor capture correlation verified for build ${report.game_build}: ${report.summary.sealed_capture_reports} reports, ${report.summary.selector_obligations} factors, ${report.summary.lifecycle_windows} lifecycle windows, zero rDPS promotions.`);
  return report;
}

function selfTest() {
  const root = mkdtempSync(path.join(tmpdir(), "rlogs-selected-factor-capture-correlation-test-"));
  try {
    const mechanicFile = path.join(root, "mechanic.json");
    const correlationFile = path.join(root, "correlation.json");
    const output = path.join(root, "proof.json");
    const mechanic = {
      schema_version: 2, generated_by: "tools/bpsr-selected-factor-mechanic-route-proof.mjs", game_build: "1", proof_state: "mechanics-candidates-indexed-runtime-proof-open",
      policy: { static_owner_context_does_not_prove_self_only_recipient_scope: true, effective_transfer_classification_reopens_owner_local_context_without_packet_proof: true, proof_receipt_does_not_promote_rdps_obligations: true, unresolved_evidence_is_never_hidden: true },
      blocker_obligations: [
        { obligation_id: "mrs:a#selectedFactorMechanics", source_rule_id: "mrs:a", source_id: "phantom-factor:9", factor_identity: { family_id: 1, buff_id: 9, name: "A" }, grade_routes: [{ item_id: 101 }], strict_relationship_routes: { target_damage_ids: [201], target_recount_ids: [31] } },
        { obligation_id: "mrs:b#selectedFactorMechanics", source_rule_id: "mrs:b", source_id: "phantom-factor:10", factor_identity: { family_id: 2, buff_id: 10, name: "B" }, grade_routes: [{ item_id: 102 }], strict_relationship_routes: { target_damage_ids: [], target_recount_ids: [] } },
      ],
    };
    mechanic.content_sha256 = contentHash(mechanic);
    writeJson(mechanicFile, mechanic);
    writeJson(correlationFile, {
      schema_version: 6,
      reports: [{
        schema_version: 6, game_build: "1", observed_client_builds: ["1"], observed_protocol_pack_digests: ["sha256:test"], session_id: "s", rdps_attribution_enabled: false, first_observed_micros: 1, last_observed_micros: 10,
        selection_observations: [{ sequence: 1, observed_micros: 2, character_id: "7", selected_factor_item_ids: [101] }],
        windows: [{ window_id: "w", effect_id: 9, instance_id: "i", factor_item_ids: [101], selected_owner_character_ids: ["7"], provider_entity_uuid: "p", recipient_entity_uuid: "r", opened_sequence: 2, opened_observed_micros: 3, apply_count: 1, refresh_count: 0, stack_count: 0, consume_count: 0, remove_count: 1, action_damage: [{ ability_id: 201, recount_group_id: 31, totals: { event_count: 2, amount: 30 } }], resource_transitions: [{ sequence: 4, observed_micros: 5, origin_energy_changed: true, resource_ids_changed: false, resource_values_changed: true, cooldowns_changed: false, state_after: { resource_ids: [8], resource_values: [3] } }] }],
        rule_summaries: [{ factor_item_id: 101, effect_id: 9, window_count: 1 }],
      }],
    });
    build({ build: "1", mechanicProof: mechanicFile, correlationBundles: [correlationFile], output });
    const report = verify(output);
    if (report.summary.exact_owner_bindings !== 1 || report.summary.adjacent_report_owner_bindings !== 0 || report.summary.negative_coverage_obligations !== 1 || report.summary.emitted_action_matches !== 1 || report.summary.resource_transitions !== 1) throw new Error("Self-test correlation coverage mismatch");

    const adjacentCorrelationFile = path.join(root, "correlation-adjacent.json");
    const adjacentOutput = path.join(root, "proof-adjacent.json");
    writeJson(adjacentCorrelationFile, {
      schema_version: 6,
      reports: [
        {
          schema_version: 6, game_build: "1", observed_client_builds: ["1"], observed_protocol_pack_digests: ["sha256:test"], session_id: "capture.run-1", rdps_attribution_enabled: false, first_observed_micros: 1, last_observed_micros: 10,
          selection_observations: [{ sequence: 9, observed_micros: 10, character_id: "7", selected_factor_item_ids: [101] }], windows: [], rule_summaries: [],
        },
        {
          schema_version: 6, game_build: "1", observed_client_builds: ["1"], observed_protocol_pack_digests: ["sha256:test"], session_id: "capture.run-2", rdps_attribution_enabled: false, first_observed_micros: 10, last_observed_micros: 20,
          selection_observations: [],
          windows: [{ window_id: "next", effect_id: 9, instance_id: "i", factor_item_ids: [101], selected_owner_character_ids: [], provider_entity_uuid: "458752", recipient_entity_uuid: "458752", opened_sequence: 1, opened_observed_micros: 11, apply_count: 1, refresh_count: 0, stack_count: 0, consume_count: 0, remove_count: 1, action_damage: [] }],
          rule_summaries: [],
        },
      ],
    });
    build({ build: "1", mechanicProof: mechanicFile, correlationBundles: [adjacentCorrelationFile], output: adjacentOutput });
    const adjacentReport = verify(adjacentOutput);
    if (adjacentReport.summary.exact_owner_bindings !== 1 || adjacentReport.summary.adjacent_report_owner_bindings !== 1 || adjacentReport.blocker_obligations[0].selection_observations[0].post_selection_observation_micros !== 10) throw new Error("Self-test adjacent report owner binding mismatch");
    const mismatchedCorrelationFile = path.join(root, "correlation-mismatched-build.json");
    writeJson(mismatchedCorrelationFile, {
      schema_version: 6,
      reports: [{
        schema_version: 6, game_build: "1", observed_client_builds: ["0"], observed_protocol_pack_digests: ["sha256:old"], session_id: "old", rdps_attribution_enabled: false,
      }],
    });
    let rejectedMismatchedObservedBuild = false;
    try {
      build({ build: "1", mechanicProof: mechanicFile, correlationBundles: [mismatchedCorrelationFile], output: path.join(root, "must-not-build.json") });
    } catch (error) {
      rejectedMismatchedObservedBuild = /observed packet build/.test(String(error?.message));
    }
    if (!rejectedMismatchedObservedBuild) throw new Error("Self-test accepted a correlation report from a mismatched observed packet build");

    const closureFile = path.join(root, "closure.json");
    const closureOutput = path.join(root, "proof-full.json");
    writeJson(closureFile, {
      schema_version: 1,
      generated_by: "tools/psychoscope-factor-closure.mjs",
      game_build: "1",
      policy: {
        descriptions_identify_candidates_only: true,
        exact_ids_or_packet_events_are_runtime_authority: true,
        unmatched_evidence_hidden: false,
        guessed_recount_relationships_allowed: false,
        capture_gate_is_global_not_factor_specific: true,
      },
      summary: { current_runtime_families: 2 },
      families: [
        { family_id: 1, family_name: "Effect factor", current_runtime_eligible: true, source_buff_ids: [9], grade_routes: [{ item_id: 101 }], direct_damage_ids: [201], exact_recount_ids: [31] },
        { family_id: 2, family_name: "Attribute factor", current_runtime_eligible: true, source_buff_ids: [], grade_routes: [{ item_id: 102 }], direct_damage_ids: [], exact_recount_ids: [] },
        { family_id: 3, family_name: "Archived factor", current_runtime_eligible: false, source_buff_ids: [11], grade_routes: [{ item_id: 103 }] },
      ],
    });
    build({ build: "1", mechanicProof: null, factorClosure: closureFile, correlationBundles: [correlationFile], output: closureOutput });
    const fullReport = verify(closureOutput);
    const attributeObligation = fullReport.blocker_obligations.find((entry) => entry.factor_identity.family_id === 2);
    if (fullReport.schema_version !== 3 || fullReport.summary.selector_obligations !== 2 || attributeObligation.expected_routes.source_state_route !== "attribute-or-state-transition" || attributeObligation.lifecycle_windows.length !== 0 || !attributeObligation.still_required_runtime_gates.includes("attribute-or-state-transition-in-sealed-canonical-capture")) throw new Error("Self-test full catalog attribute route mismatch");
    console.log("bpsr-selected-factor-capture-correlation-proof self-test passed");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

function sum(entries, key) { return entries.reduce((total, entry) => total + Number(entry[key] ?? 0), 0); }
function countTrue(entries, key) { return entries.reduce((total, entry) => total + (entry[key] === true ? 1 : 0), 0); }
function numberOrNull(value) { return value === null || value === undefined ? null : Number(value); }
function compareObservation(left, right) { return compareText(`${left.session_id}:${String(left.sequence).padStart(20, "0")}`, `${right.session_id}:${String(right.sequence).padStart(20, "0")}`); }
function compareWindow(left, right) { return compareText(`${left.session_id}:${String(left.opened_sequence).padStart(20, "0")}:${left.window_id}`, `${right.session_id}:${String(right.opened_sequence).padStart(20, "0")}:${right.window_id}`); }
function compareAction(left, right) { return compareText(`${left.session_id}:${left.window_id}:${left.ability_id}:${left.recount_group_id}`, `${right.session_id}:${right.window_id}:${right.ability_id}:${right.recount_group_id}`); }
function compareResourceTransition(left, right) {
  return compareText(
    `${left.session_id}:${left.window_id}:${String(left.sequence ?? 0).padStart(20, "0")}:${String(left.observed_micros ?? 0).padStart(20, "0")}:${stableStringify(left)}`,
    `${right.session_id}:${right.window_id}:${String(right.sequence ?? 0).padStart(20, "0")}:${String(right.observed_micros ?? 0).padStart(20, "0")}:${stableStringify(right)}`,
  );
}
function compareText(left, right) { return String(left).localeCompare(String(right), "en"); }
function uniqueText(values) { return [...new Set(values.map(String))].sort(compareText); }
function uniqueNumeric(values) { return [...new Set(values.map(Number).filter(Number.isFinite))].sort((left, right) => left - right); }
function asArray(value) { return Array.isArray(value) ? value : [value]; }
function requireFile(file, label) { if (!existsSync(file) || !statSync(file).isFile()) throw new Error(`Missing ${label}: ${file}`); }
function fileDescriptor(file) { return { path: file.replaceAll("\\", "/"), bytes: statSync(file).size, sha256: hashFile(file) }; }
function contentHash(value) { const clone = structuredClone(value); delete clone.content_sha256; return hashText(stableStringify(clone)); }
function stableStringify(value) { if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`; if (value && typeof value === "object") return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`; return JSON.stringify(value); }
function hashFile(file) { return createHash("sha256").update(readFileSync(file)).digest("hex"); }
function hashText(value) { return createHash("sha256").update(value).digest("hex"); }
function readJson(file, label) { try { return JSON.parse(readFileSync(file, "utf8")); } catch (error) { throw new Error(`Unable to read ${label} ${file}: ${error.message}`); } }
function writeJson(file, value) { writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`, "utf8"); }
function parseArgs(args) { const parsed = {}; for (let index = 0; index < args.length; index += 1) { const arg = args[index]; if (!arg.startsWith("--")) throw new Error(`Unexpected argument ${arg}`); const key = arg.slice(2); const value = args[index + 1]; if (value === undefined || value.startsWith("--")) throw new Error(`Missing value for --${key}`); if (key === "correlation-bundle") parsed[key] = [...asArray(parsed[key] ?? []), value]; else parsed[key] = value; index += 1; } return parsed; }
function required(value, key) { if (!value[key] || (Array.isArray(value[key]) && !value[key].length)) throw new Error(`Missing --${key}`); return value[key]; }
function usage(exitCode) { console.log("Usage:\n  node tools/bpsr-selected-factor-capture-correlation-proof.mjs build --build <id> (--mechanic-proof <json> | --factor-closure <json>) --correlation-bundle <json> [--correlation-bundle <json> ...] --output <json>\n  node tools/bpsr-selected-factor-capture-correlation-proof.mjs verify --input <json>\n  node tools/bpsr-selected-factor-capture-correlation-proof.mjs self-test"); process.exit(exitCode); }
