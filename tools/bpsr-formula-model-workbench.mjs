#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);
const OFFLINE_FORMULA_PROOF_STATE = "exact-current-build-offline-formula-proven";
const CANONICAL_RUNTIME_INPUT_ROUTE_PROOF_STATE = "exact-current-build-canonical-runtime-input-route-proven";
const SELECTED_FACTOR_ROUTE_PROOF_STATE = "exact-current-build-local-full-snapshot-selection-route-proven-dirty-transition-open";
const SELECTED_FACTOR_MECHANIC_PROOF_STATE = "mechanics-candidates-indexed-runtime-proof-open";
const SELECTED_FACTOR_CAPTURE_CORRELATION_PROOF_STATE = "canonical-capture-correlation-observed-runtime-gates-open";
const GENERIC_RUNTIME_SELECTOR_SUMMARY_BLOCKER = "runtime selector evidence must choose one candidate value";
const ALLOWED_PROOF_RECEIPT_STATES = new Set([
  OFFLINE_FORMULA_PROOF_STATE,
  CANONICAL_RUNTIME_INPUT_ROUTE_PROOF_STATE,
  SELECTED_FACTOR_ROUTE_PROOF_STATE,
  SELECTED_FACTOR_MECHANIC_PROOF_STATE,
  SELECTED_FACTOR_CAPTURE_CORRELATION_PROOF_STATE,
]);

if (command === "build") build(resolveContext(options));
else if (command === "verify") verify(path.resolve(required(options, "input")));
else if (command === "inspect") inspect(path.resolve(required(options, "input")), options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function resolveContext(parsed) {
  const buildId = required(parsed, "build");
  if (!/^\d+$/.test(buildId)) throw new Error("Build must contain only ASCII digits");
  return {
    build: buildId,
    staticFormulaEvidence: path.resolve(required(parsed, "static-formula-evidence")),
    valueProof: path.resolve(required(parsed, "value-proof")),
    proofRegistry: path.resolve(required(parsed, "proof-registry")),
    output: path.resolve(required(parsed, "output")),
  };
}

function build(context) {
  const started = performance.now();
  requireFile(context.staticFormulaEvidence, "static formula evidence");
  requireFile(context.valueProof, "modifier value proof");
  requireFile(context.proofRegistry, "shared formula proof registry");
  const evidence = readJson(context.staticFormulaEvidence, "static formula evidence");
  const valueProof = readJson(context.valueProof, "modifier value proof");
  const proofRegistry = readJson(context.proofRegistry, "shared formula proof registry");
  requireBuild(evidence, context.build, "game_build", "static formula evidence");
  const proofReceiptIndexes = validateProofRegistry(proofRegistry, context.build);
  const valueEntriesBySource = indexValueProofEntries(valueProof.entriesByKey ?? {});

  const obligations = [];
  const subsumedBlockers = [];
  for (const source of evidence.sources ?? []) {
    if (source.static_gate_resolved) continue;
    const explicitBlockers = source.remaining_static_blockers ?? [];
    const blockers = explicitBlockers.length ? explicitBlockers : inferMissingBlockers(source);
    const typedSiblingObligationIds = blockers
      .map((blocker, blockerIndex) => ({ blocker: String(blocker), obligation_id: `${source.source_rule_id}#${blockerIndex}` }))
      .filter((entry) => entry.blocker !== GENERIC_RUNTIME_SELECTOR_SUMMARY_BLOCKER)
      .map((entry) => entry.obligation_id);
    for (const [blockerIndex, blocker] of blockers.entries()) {
      if (String(blocker) === GENERIC_RUNTIME_SELECTOR_SUMMARY_BLOCKER && typedSiblingObligationIds.length > 0) {
        subsumedBlockers.push({
          blocker_evidence_id: `${source.source_rule_id}#${blockerIndex}`,
          source_rule_id: source.source_rule_id,
          source_id: source.source_id,
          blocker: GENERIC_RUNTIME_SELECTOR_SUMMARY_BLOCKER,
          blocker_origin: "emitted-selector-state-summary",
          subsumed_by_obligation_ids: typedSiblingObligationIds,
          reason: "The source's typed selector blockers preserve the actionable proof contracts; this emitted sentence preserves the aggregate no-selected-value state without creating an unrelated general formula model.",
        });
        continue;
      }
      const model = classifyBlocker(blocker);
      const componentEvidence = buildComponentEvidence(source.source_rule_id, blocker, model.component, valueEntriesBySource.get(source.source_rule_id) ?? []);
      const proofModelIds = [...new Set(componentEvidence.flatMap((entry) => entry.selectors.flatMap((selector) => selector.model_id ? [selector.model_id] : [])))].sort(compareText);
      obligations.push({
        obligation_id: `${source.source_rule_id}#${blockerIndex}`,
        source_rule_id: source.source_rule_id,
        source_id: source.source_id,
        source_name: source.source_name,
        source_classification: source.classification,
        blocker: String(blocker),
        blocker_origin: explicitBlockers.length ? "emitted" : "derived-from-unresolved-formula-term",
        model_key: model.key,
        model_family: model.family,
        component_key: model.component,
        proof_contract: model.contract,
        proof_model_ids: proofModelIds,
        component_evidence_status: summarizeEvidenceStatus(componentEvidence),
        manual_component_binding_required: componentEvidence.some((entry) => entry.manual_component_binding_required),
        runtime_selector_required: componentEvidence.some((entry) => entry.runtime_selector_required),
        component_evidence: componentEvidence,
        formula_term_ids: source.formula_term_ids ?? [],
        effect_ids: source.effect_ids ?? [],
        accepted_terms: source.accepted_terms ?? [],
        evidence_sha256: source.evidence_sha256,
      });
    }
  }

  obligations.sort(compareObligations);
  for (const obligation of obligations) {
    obligation.targeted_proof_receipts = targetedProofReceiptsForObligation(proofReceiptIndexes, obligation);
  }
  const groupsByKey = new Map();
  for (const obligation of obligations) {
    if (!groupsByKey.has(obligation.model_key)) {
      groupsByKey.set(obligation.model_key, {
        model_key: obligation.model_key,
        model_family: obligation.model_family,
        component_key: obligation.component_key,
        proof_contract: obligation.proof_contract,
        source_rule_ids: new Set(),
        blocker_texts: new Set(),
        classifications: new Set(),
        formula_term_ids: new Set(),
        effect_ids: new Set(),
        obligation_ids: [],
        evidence_sha256: new Set(),
        proof_model_ids: new Set(),
        value_proof_entry_keys: new Set(),
        exact_component_obligations: 0,
        entry_associated_component_obligations: 0,
        selector_only_obligations: 0,
        unmatched_component_obligations: 0,
        manual_component_binding_obligations: 0,
        runtime_selector_obligations: 0,
        targeted_proof_receipts: new Map(),
        targeted_obligation_ids: new Set(),
      });
    }
    const group = groupsByKey.get(obligation.model_key);
    group.source_rule_ids.add(obligation.source_rule_id);
    group.blocker_texts.add(obligation.blocker);
    group.classifications.add(obligation.source_classification);
    for (const term of obligation.formula_term_ids) group.formula_term_ids.add(String(term));
    for (const effect of obligation.effect_ids) group.effect_ids.add(String(effect));
    group.obligation_ids.push(obligation.obligation_id);
    group.evidence_sha256.add(obligation.evidence_sha256);
    for (const modelId of obligation.proof_model_ids) group.proof_model_ids.add(modelId);
    for (const componentEvidence of obligation.component_evidence) {
      for (const entry of componentEvidence.evidence_entries) group.value_proof_entry_keys.add(entry.entry_key);
    }
    if (obligation.component_evidence_status === "exact-source-rule") group.exact_component_obligations += 1;
    else if (obligation.component_evidence_status === "entry-associated") group.entry_associated_component_obligations += 1;
    else if (obligation.component_evidence_status === "selector-only") group.selector_only_obligations += 1;
    else group.unmatched_component_obligations += 1;
    if (obligation.manual_component_binding_required) group.manual_component_binding_obligations += 1;
    if (obligation.runtime_selector_required) group.runtime_selector_obligations += 1;
    for (const receipt of obligation.targeted_proof_receipts) {
      group.targeted_proof_receipts.set(receipt.proof_id, receipt);
      group.targeted_obligation_ids.add(obligation.obligation_id);
    }
  }

  const staticModelGroups = [...groupsByKey.values()].map(finalizeGroup).map((group) => ({
    ...group,
    registry_only_proof_route: false,
  }));
  const representedModelKeys = new Set(staticModelGroups.map((group) => group.model_key));
  const registryOnlyModelGroups = [...proofReceiptIndexes.receiptsByModel.keys()]
    .filter((modelKey) => !representedModelKeys.has(modelKey))
    .map(registryOnlyProofRouteGroup);
  const modelGroups = [...staticModelGroups, ...registryOnlyModelGroups].map((group) => {
    const proofReceipts = proofReceiptIndexes.receiptsByModel.get(group.model_key) ?? [];
    const proofStates = uniqueSorted(proofReceipts.map((receipt) => receipt.state));
    return {
      ...group,
      shared_proof_status: proofReceipts.length
        ? "exact-current-build-shared-proof-received-downstream-runtime-open"
        : "proof-open",
      offline_formula_proof_status: proofStates.includes(OFFLINE_FORMULA_PROOF_STATE)
        ? "exact-current-build-offline-formula-proven-runtime-open"
        : "proof-open",
      canonical_runtime_input_route_proof_status: proofStates.includes(CANONICAL_RUNTIME_INPUT_ROUTE_PROOF_STATE)
        ? "exact-current-build-canonical-runtime-input-route-proven-downstream-runtime-open"
        : "proof-open",
      proof_states: proofStates,
      proof_receipts: proofReceipts,
    };
  }).sort(compareGroups);
  const proofReceivedModels = modelGroups.filter((group) => group.proof_receipts.length > 0);
  const offlineProvenModels = modelGroups.filter((group) => group.proof_states.includes(OFFLINE_FORMULA_PROOF_STATE));
  const runtimeRouteProvenModels = modelGroups.filter((group) => group.proof_states.includes(CANONICAL_RUNTIME_INPUT_ROUTE_PROOF_STATE));
  const targetedProofObligations = obligations.filter((entry) => entry.targeted_proof_receipts.length > 0);
  const targetedProofReceiptIds = new Set(targetedProofObligations.flatMap((entry) => entry.targeted_proof_receipts.map((receipt) => receipt.proof_id)));
  subsumedBlockers.sort((left, right) => compareText(left.blocker_evidence_id, right.blocker_evidence_id));
  const pendingSources = new Set([
    ...obligations.map((entry) => entry.source_rule_id),
    ...subsumedBlockers.map((entry) => entry.source_rule_id),
  ]);
  const report = {
    schema_version: 3,
    generated_by: "tools/bpsr-formula-model-workbench.mjs",
    game_build: context.build,
    policy: {
      shared_model_proof_closes_repeated_work: true,
      exact_current_build_evidence_only: true,
      no_runtime_selector_is_guessed: true,
      no_static_blocker_is_hidden: true,
      unresolved_sources_without_explicit_blockers_are_routed: true,
      sources_may_require_multiple_models: true,
      component_proof_is_linked_before_source_level_research: true,
      entry_associated_components_are_not_promoted_as_exact_bindings: true,
      promotion_still_requires_runtime_counterfactual_conservation: true,
      offline_formula_proofs_do_not_close_runtime_or_conservation_gates: true,
      canonical_runtime_input_route_proofs_do_not_close_provider_projection_or_conservation_gates: true,
      selected_factor_identity_routes_do_not_close_mechanics_provider_projection_or_conservation_gates: true,
      selected_factor_mechanic_routes_do_not_close_provider_projection_or_conservation_gates: true,
      selected_factor_capture_correlations_do_not_close_counterfactual_or_conservation_gates: true,
      all_shared_proof_receipts_preserve_downstream_runtime_gates: true,
      registry_only_proof_routes_are_preserved_without_fabricated_obligations: true,
      generic_selector_state_summaries_are_preserved_as_subsumed_evidence: true,
      selector_summary_subsumption_requires_same_source_typed_obligations: true,
    },
    inputs: {
      static_formula_evidence: fileDescriptor(context.staticFormulaEvidence),
      modifier_value_proof: fileDescriptor(context.valueProof),
      shared_formula_proof_registry: fileDescriptor(context.proofRegistry),
    },
    summary: {
      pending_sources: pendingSources.size,
      blocker_obligations: obligations.length,
      subsumed_blocker_evidence: subsumedBlockers.length,
      preserved_blocker_evidence: obligations.length + subsumedBlockers.length,
      shared_model_groups: modelGroups.length,
      shared_registry_model_keys: proofReceiptIndexes.receiptsByModel.size,
      registry_only_proof_route_models: registryOnlyModelGroups.length,
      exact_current_build_shared_proof_received_models: proofReceivedModels.length,
      exact_current_build_offline_formula_proven_models: offlineProvenModels.length,
      exact_current_build_canonical_runtime_input_route_proven_models: runtimeRouteProvenModels.length,
      exact_current_build_targeted_proof_received_obligations: targetedProofObligations.length,
      targeted_proof_receipts: targetedProofReceiptIds.size,
      runtime_formula_models_closed_by_offline_proofs: 0,
      rdps_obligations_closed_by_shared_proof_receipts: 0,
      source_investigations_avoided_if_proved_by_group: Math.max(0, obligations.length - staticModelGroups.length),
      model_family_counts: countBy(modelGroups, (group) => group.model_family),
      model_family_source_counts: familySourceCounts(modelGroups),
      component_evidence_counts: countBy(obligations, (entry) => entry.component_evidence_status),
      exact_component_obligations: obligations.filter((entry) => entry.component_evidence_status === "exact-source-rule").length,
      manual_component_binding_obligations: obligations.filter((entry) => entry.manual_component_binding_required).length,
      runtime_selector_obligations: obligations.filter((entry) => entry.runtime_selector_required).length,
      value_proof_entries_reused: new Set(obligations.flatMap((entry) => entry.component_evidence.flatMap((component) => component.evidence_entries.map((evidenceEntry) => evidenceEntry.entry_key)))).size,
      hidden_blocker_obligations: 0,
      zero_hidden_omissions: true,
    },
    model_groups: modelGroups,
    expected_value_models: valueProof.expectedValueModels ?? {},
    subsumed_blockers: subsumedBlockers,
    obligations,
  };
  report.content_sha256 = contentHash(report);
  mkdirSync(path.dirname(context.output), { recursive: true });
  writeFileSync(context.output, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  verify(context.output);
  console.log(`Formula model workbench built for ${context.build}: ${pendingSources.size} sources, ${obligations.length} actionable blocker obligations, ${subsumedBlockers.length} preserved selector summaries, ${modelGroups.length} shared proof models, zero hidden omissions in ${Math.round(performance.now() - started)} ms.`);
}

function validateProofRegistry(registry, buildId) {
  requireBuild(registry, buildId, "game_build", "shared formula proof registry");
  if (registry.schema_version !== 3 || registry.generated_by !== "tools/bpsr-shared-formula-proof-registry.mjs") {
    throw new Error("Shared formula proof registry has an unsupported schema or generator");
  }
  if (registry.content_sha256 !== contentHash(registry)) throw new Error("Shared formula proof registry content hash mismatch");
  if (registry.policy?.offline_formula_proof_does_not_close_runtime_gates !== true ||
    registry.policy?.canonical_runtime_input_route_proof_does_not_close_provider_projection_or_conservation_gates !== true ||
    registry.policy?.selected_factor_mechanic_routes_do_not_close_provider_projection_or_conservation_gates !== true ||
    registry.policy?.selected_factor_capture_correlations_do_not_close_counterfactual_or_conservation_gates !== true ||
    registry.policy?.proof_receipts_do_not_promote_rdps_obligations !== true ||
    registry.summary?.runtime_gates_closed !== 0 || registry.summary?.rdps_obligations_promoted !== 0) {
    throw new Error("Shared formula proof registry has an unsafe promotion policy");
  }
  const receiptsByModel = new Map();
  const receiptsBySourceRule = new Map();
  const receiptsByObligation = new Map();
  const proofIds = new Set();
  for (const receipt of registry.proof_receipts ?? []) {
    if (!receipt?.proof_id || proofIds.has(receipt.proof_id)) throw new Error(`Duplicate or missing proof receipt ${receipt?.proof_id}`);
    proofIds.add(receipt.proof_id);
    if (!ALLOWED_PROOF_RECEIPT_STATES.has(receipt.state) || !receipt.still_required_runtime_gates?.length) {
      throw new Error(`Unsafe shared formula proof receipt ${receipt.proof_id}`);
    }
    for (const modelKey of receipt.model_keys ?? []) {
      if (receiptsByModel.has(modelKey)) throw new Error(`Shared model ${modelKey} has multiple proof receipts`);
      receiptsByModel.set(modelKey, [structuredClone(receipt)]);
    }
    for (const sourceRuleId of receipt.source_rule_ids ?? []) {
      if (!receiptsBySourceRule.has(sourceRuleId)) receiptsBySourceRule.set(sourceRuleId, []);
      receiptsBySourceRule.get(sourceRuleId).push(structuredClone(receipt));
    }
    for (const obligationId of receipt.obligation_ids ?? []) {
      if (!receiptsByObligation.has(obligationId)) receiptsByObligation.set(obligationId, []);
      receiptsByObligation.get(obligationId).push(structuredClone(receipt));
    }
  }
  if (proofIds.size !== Number(registry.summary?.proof_receipts) ||
    receiptsByModel.size !== Number(registry.summary?.covered_model_keys) ||
    receiptsBySourceRule.size !== Number(registry.summary?.covered_source_rule_ids) ||
    receiptsByObligation.size !== Number(registry.summary?.covered_obligation_ids)) {
    throw new Error("Shared formula proof registry summary mismatch");
  }
  return { receiptsByModel, receiptsBySourceRule, receiptsByObligation };
}

function targetedProofReceiptsForObligation(indexes, obligation) {
  const receipts = [
    ...(indexes.receiptsBySourceRule.get(obligation.source_rule_id) ?? []),
    ...(indexes.receiptsByObligation.get(obligation.obligation_id) ?? []),
  ];
  const unique = new Map();
  for (const receipt of receipts) unique.set(receipt.proof_id, receipt);
  return [...unique.values()].sort((left, right) => compareText(left.proof_id, right.proof_id));
}

function classifyBlocker(raw) {
  const blocker = String(raw);
  const normalized = blocker.toLowerCase().replace(/[-_:]+/g, " ").replace(/\s+/g, " ").trim();
  const component = extractComponent(blocker);
  const componentKey = slug(component ?? "general");

  if (/expected value model required/.test(normalized)) {
    return model("expected-value", componentKey,
      "Prove the current-build probability conversion and outcome multiplier, then validate predicted versus observed event distributions without assuming a localized description is executable math.");
  }
  if (/stat conversion model required|formula input.*conversion|required.*stat conversion/.test(normalized)) {
    return model("stat-conversion", componentKey,
      "Prove the exact current-build raw-stat to combat-coefficient conversion, including caps, breakpoints, level scaling, and rounding order, against controlled packet-observed outputs.");
  }
  if (/timing model required|cooldown|attack speed|haste|resource timing/.test(normalized)) {
    return model("timing", componentKey,
      "Prove the exact event-time duration, cooldown, rate, resource, tick, and rounding behavior from timestamps and state transitions; do not infer damage attribution from tooltip timing alone.");
  }
  if (/target window proof required|target vulnerability|recipient window/.test(normalized)) {
    return model("target-window", componentKey,
      "Correlate apply, refresh, stack, consume, and remove lifecycle events with provider, recipient, target, and damage timestamps to prove the exact active window.");
  }
  if (/hit count model required|hit count|strike count|tick count/.test(normalized)) {
    return model("hit-count", componentKey,
      "Prove the exact number and identity of packet-observed hits or ticks per activation, including multi-target and proc children, before aggregation.");
  }
  if (/ambiguous scoped value|scope required|scope unresolved/.test(normalized)) {
    return model("value-scope", componentKey,
      "Disambiguate which decoded value belongs to this component and whether it is self, party, target, or encounter scoped using exact references and packet-observed ownership.");
  }
  if (/selected factor grade|selector|required tier|value ladder|ramp|threshold|choose candidate/.test(normalized)) {
    return model("runtime-selector", componentKey,
      "Preserve the full typed ladder and prove the active tier, grade, stack, ramp, threshold, or candidate from packet state at the event timestamp.");
  }
  if (/runtime formula input|runtime input/.test(normalized)) {
    return model("runtime-input", componentKey,
      "Identify every runtime formula operand from packet state at the event timestamp and validate the decoded operation and rounding order against observed output.");
  }
  if (/no generated value proof|missing value|value proof required/.test(normalized)) {
    return model("missing-value-evidence", componentKey,
      "Trace the source through exact decoded references and packet-observed fields until a typed magnitude is proven; preserve all unresolved tokens and candidates.");
  }
  if (/unclassified unresolved static gate|no explicit blocker emitted/.test(normalized)) {
    return model("unclassified-static-gate", componentKey,
      "Trace why the source remains unresolved despite having no emitted blocker, then emit a typed blocker or close the static gate with an exact proof receipt.");
  }
  return model("other-static-proof", slug(normalized),
    "Resolve this exact current-build blocker from decoded references and packet-observed evidence without name-based inference or omission.");
}

function inferMissingBlockers(source) {
  const terms = [...new Set(source.formula_term_ids ?? [])];
  if (terms.length === 0) return ["unclassified-unresolved-static-gate:no-explicit-blocker-emitted"];
  return terms.map((term) => {
    const accepted = source.accepted_terms ?? [];
    if (accepted.length === 0) return `component:${term}:missing-value-evidence`;
    return `component:${term}:runtime-formula-input-model-required`;
  });
}

function model(family, component, contract) {
  return { family, component, key: `${family}:${component}`, contract };
}

function extractComponent(blocker) {
  const blockerText = String(blocker);
  const componentMarker = blockerText.toLowerCase().indexOf("component:");
  if (componentMarker >= 0) {
    const pathSegments = blockerText
      .slice(componentMarker + "component:".length)
      .split(":")
      .map(slug);
    const wrappers = new Set(["equipment-set-attribute", "formula-input"]);
    const proofSuffixes = new Set([
      "all",
      "ambiguous-scoped-value",
      "ambiguous-value-selection-required",
      "description-parameter-source-required",
      "encounter-selected-factor-grade-required",
      "expected-value-model-required",
      "missing-formula-value",
      "missing-value-evidence",
      "runtime-formula-input-model-required",
      "runtime-formula-inputs-required",
      "stat-conversion-model-required",
      "target-window-proof-required",
      "threshold-state-selector-required",
      "tier-or-level-selection-required",
      "timing-model-required",
      "value-ladder-selection-required",
    ]);
    const semanticSegments = pathSegments.filter((segment) => (
      !wrappers.has(segment)
      && !proofSuffixes.has(segment)
      && !/^\d+$/.test(segment)
    ));
    if (semanticSegments.length) return semanticSegments.at(-1);
    if (pathSegments.length) return pathSegments[0];
  }
  const normalized = blockerText.toLowerCase();
  for (const token of ["critical-rate", "lucky-rate", "mastery", "adaptive-primary-stat", "attack-speed", "haste", "atk", "hp", "target-vulnerability", "cooldown", "resource", "hit-count"]) {
    if (normalized.includes(token)) return token;
  }
  return null;
}

function indexValueProofEntries(entriesByKey) {
  const index = new Map();
  for (const [entryKey, entry] of Object.entries(entriesByKey)) {
    for (const sourceRuleId of entry.sourceRuleIds ?? []) {
      if (!index.has(sourceRuleId)) index.set(sourceRuleId, []);
      index.get(sourceRuleId).push({ entry_key: entryKey, ...entry });
    }
  }
  for (const entries of index.values()) entries.sort((left, right) => compareText(left.entry_key, right.entry_key));
  return index;
}

function buildComponentEvidence(sourceRuleId, blocker, componentKey, entries) {
  const fragments = [];
  for (const entry of entries) {
    const exactBlocker = (entry.valueBlockers ?? []).includes(String(blocker));
    const matchingSelectors = (entry.valueSelectors ?? []).filter((selector) => selectorMatchesBlocker(selector, blocker, componentKey));
    if (!exactBlocker && matchingSelectors.length === 0) continue;

    const componentValues = collectComponentValues(entry, matchingSelectors, componentKey);
    const matchingComponentSelectors = (entry.valueSelectors ?? []).filter((selector) => componentsEqual(selector.componentKey, componentKey));
    const directValues = componentValues.filter((value) => value.source_rule_id === sourceRuleId);
    const associatedValues = componentValues.filter((value) => !value.source_rule_id);
    const bindingStatus = directValues.length > 0
      ? "exact-source-rule"
      : associatedValues.length > 0
        ? "entry-associated"
        : "selector-only";
    const distinctEntryComponents = new Set([
      ...(entry.selectedValues ?? []).map((value) => normalizeComponent(value.componentKey)),
      ...(entry.valueSelectors ?? []).map((selector) => normalizeComponent(selector.componentKey)),
    ].filter(Boolean));

    fragments.push({
      binding_status: bindingStatus,
      manual_component_binding_required: bindingStatus === "entry-associated" && distinctEntryComponents.size > 1,
      runtime_selector_required: matchingComponentSelectors.some(selectorRequiresRuntimeSelection),
      selected_values: componentValues.sort(compareStableObjects),
      selectors: matchingComponentSelectors.map(compactSelector).sort(compareStableObjects),
      evidence_entry: {
        entry_key: entry.entry_key,
        uid: String(entry.uid ?? ""),
        category: entry.category ?? "",
        source_label: entry.sourceLabel ?? "",
        direct_source_rule: (entry.directSourceRuleIds ?? []).includes(sourceRuleId),
        matched_by: [
          ...(exactBlocker ? ["exact-value-blocker"] : []),
          ...(matchingSelectors.length ? ["component-selector"] : []),
        ],
        value_proof_status: entry.valueProofStatus ?? "",
      },
    });
  }

  const consolidated = new Map();
  for (const fragment of fragments) {
    const fingerprint = stableStringify({
      binding_status: fragment.binding_status,
      manual_component_binding_required: fragment.manual_component_binding_required,
      runtime_selector_required: fragment.runtime_selector_required,
      selected_values: fragment.selected_values,
      selectors: fragment.selectors,
    });
    if (!consolidated.has(fingerprint)) {
      consolidated.set(fingerprint, {
        component_key: componentKey,
        binding_status: fragment.binding_status,
        manual_component_binding_required: fragment.manual_component_binding_required,
        runtime_selector_required: fragment.runtime_selector_required,
        selected_values: fragment.selected_values,
        selectors: fragment.selectors,
        evidence_entries: [],
      });
    }
    consolidated.get(fingerprint).evidence_entries.push(fragment.evidence_entry);
  }
  return [...consolidated.values()].map((entry) => ({
    ...entry,
    evidence_entries: entry.evidence_entries.sort((left, right) => compareText(left.entry_key, right.entry_key)),
  })).sort((left, right) => compareText(left.binding_status, right.binding_status) || compareText(left.evidence_entries[0]?.entry_key, right.evidence_entries[0]?.entry_key));
}

function selectorMatchesBlocker(selector, blocker, componentKey) {
  if (!componentsEqual(selector.componentKey, componentKey)) return false;
  const normalized = String(blocker).toLowerCase();
  const kind = String(selector.kind ?? "").toLowerCase();
  if (normalized.includes("expected-value-model-required")) return kind.includes("expected-value");
  if (normalized.includes("stat-conversion-model-required")) return kind === "stat-conversion-model";
  if (normalized.includes("timing-model-required")) return kind === "timing-cadence-model";
  if (normalized.includes("hit-count-model-required")) return kind === "hit-count-model";
  if (normalized.includes("target-window-proof-required")) return kind === "target-window-state";
  if (/tier|grade|ladder|ramp|threshold|selection-required|selector-required/.test(normalized)) {
    return /selector|tier|stack|threshold|ramp|stage/.test(kind);
  }
  if (/scope|required-runtime-scope|ambiguous-scoped-value/.test(normalized)) return kind === "runtime-scope";
  return true;
}

function collectComponentValues(entry, selectors, componentKey) {
  const values = [
    ...(entry.selectedValues ?? []),
    ...selectors.flatMap((selector) => [
      ...(selector.selectedValues ?? []),
      ...(selector.candidateValues ?? []),
      ...Object.values(selector.candidatesByScope ?? {}).flat(),
    ]),
  ].filter((value) => componentsEqual(value.componentKey ?? componentKey, componentKey));
  const unique = new Map();
  for (const value of values) {
    const compact = compactValue({ ...value, componentKey: value.componentKey ?? componentKey });
    unique.set(stableStringify(compact), compact);
  }
  return [...unique.values()];
}

function compactValue(value) {
  return Object.fromEntries(Object.entries({
    component_key: value.componentKey,
    source_rule_id: value.sourceRuleId,
    effect_class: value.effectClass,
    formula_term_ids: value.formulaTermIds,
    contribution_groups: value.contributionGroups,
    scope: value.scope,
    raw_scope: value.rawScope,
    value: value.value,
    decimal_value: value.decimalValue,
    unit: value.unit,
    raw_text: value.rawText,
    source_text: value.sourceText,
    tier: value.tier,
    tier_kind: value.tierKind,
    key: value.key,
    raw_table_value: value.rawTableValue,
  }).filter(([, fieldValue]) => fieldValue !== undefined));
}

function compactSelector(selector) {
  return Object.fromEntries(Object.entries({
    kind: selector.kind,
    status: selector.status,
    component_key: selector.componentKey,
    model_id: selector.modelId,
    model_status: selector.modelStatus,
    contribution_ready: selector.contributionReady,
    value_resolution: selector.valueResolution,
    required_runtime_fields: selector.requiredRuntimeFields,
    required_inputs: selector.requiredInputs,
    output_fields: selector.outputFields,
    proof_policy: selector.proofPolicy,
    required_runtime_proof: selector.requiredRuntimeProof,
    validation_policy: selector.validationPolicy,
    formula_sketch: selector.formulaSketch,
    delta_formula_sketch: selector.deltaFormulaSketch,
  }).filter(([, fieldValue]) => fieldValue !== undefined));
}

function selectorRequiresRuntimeSelection(selector) {
  const kind = String(selector.kind ?? "");
  return /runtime|selector|tier|stack|threshold|ramp|stage|target-window/.test(kind)
    && !/expected-value|stat-conversion|timing-cadence|hit-count/.test(kind);
}

function summarizeEvidenceStatus(componentEvidence) {
  if (componentEvidence.some((entry) => entry.binding_status === "exact-source-rule")) return "exact-source-rule";
  if (componentEvidence.some((entry) => entry.binding_status === "entry-associated")) return "entry-associated";
  if (componentEvidence.some((entry) => entry.binding_status === "selector-only")) return "selector-only";
  return "unmatched-preserved";
}

function normalizeComponent(value) { return value === undefined || value === null ? "" : slug(value); }
function componentsEqual(left, right) { return normalizeComponent(left) === normalizeComponent(right); }
function compareStableObjects(left, right) { return compareText(stableStringify(left), stableStringify(right)); }

function finalizeGroup(group) {
  const sourceRuleIds = [...group.source_rule_ids].sort(compareText);
  const obligationIds = [...group.obligation_ids].sort(compareText);
  return {
    model_key: group.model_key,
    model_family: group.model_family,
    component_key: group.component_key,
    proof_contract: group.proof_contract,
    source_count: sourceRuleIds.length,
    obligation_count: obligationIds.length,
    source_rule_ids: sourceRuleIds,
    obligation_ids: obligationIds,
    blocker_texts: [...group.blocker_texts].sort(compareText),
    source_classifications: [...group.classifications].sort(compareText),
    formula_term_ids: [...group.formula_term_ids].sort(compareText),
    effect_ids: [...group.effect_ids].sort(compareIdentifiers),
    evidence_sha256: [...group.evidence_sha256].sort(compareText),
    proof_model_ids: [...group.proof_model_ids].sort(compareText),
    value_proof_entry_keys: [...group.value_proof_entry_keys].sort(compareText),
    component_evidence_counts: {
      exact_source_rule: group.exact_component_obligations,
      entry_associated: group.entry_associated_component_obligations,
      selector_only: group.selector_only_obligations,
      unmatched_preserved: group.unmatched_component_obligations,
    },
    manual_component_binding_obligations: group.manual_component_binding_obligations,
    runtime_selector_obligations: group.runtime_selector_obligations,
    targeted_proof_receipts: [...group.targeted_proof_receipts.values()].sort((left, right) => compareText(left.proof_id, right.proof_id)),
    targeted_obligation_ids: [...group.targeted_obligation_ids].sort(compareText),
  };
}

function registryOnlyProofRouteGroup(modelKey) {
  const separator = modelKey.indexOf(":");
  if (separator <= 0 || separator === modelKey.length - 1) {
    throw new Error(`Invalid registry-only model key ${modelKey}`);
  }
  return {
    model_key: modelKey,
    model_family: modelKey.slice(0, separator),
    component_key: modelKey.slice(separator + 1),
    proof_contract: "Preserve the exact current-build shared proof as a deferred-attribution route and close every receipt-listed runtime gate before rDPS promotion.",
    registry_only_proof_route: true,
    source_count: 0,
    obligation_count: 0,
    source_rule_ids: [],
    obligation_ids: [],
    blocker_texts: [],
    source_classifications: [],
    formula_term_ids: [],
    effect_ids: [],
    evidence_sha256: [],
    proof_model_ids: [],
    value_proof_entry_keys: [],
    component_evidence_counts: {
      exact_source_rule: 0,
      entry_associated: 0,
      selector_only: 0,
      unmatched_preserved: 0,
    },
    manual_component_binding_obligations: 0,
    runtime_selector_obligations: 0,
    targeted_proof_receipts: [],
    targeted_obligation_ids: [],
  };
}

function verify(input) {
  const report = readJson(input, "formula model workbench");
  if (report.schema_version !== 3) throw new Error("Formula model workbench schema_version must be 3");
  if (report.content_sha256 !== contentHash(report)) throw new Error("Formula model workbench content hash mismatch");
  if (!report.summary?.zero_hidden_omissions || report.summary.hidden_blocker_obligations !== 0) {
    throw new Error("Formula model workbench must preserve every blocker obligation");
  }
  if (report.summary.blocker_obligations !== report.obligations.length) throw new Error("Blocker obligation count mismatch");
  if (report.summary.subsumed_blocker_evidence !== (report.subsumed_blockers?.length ?? 0) ||
    report.summary.preserved_blocker_evidence !== report.obligations.length + (report.subsumed_blockers?.length ?? 0)) {
    throw new Error("Preserved blocker evidence count mismatch");
  }
  if (report.summary.shared_model_groups !== report.model_groups.length) throw new Error("Model group count mismatch");
  if (report.policy?.offline_formula_proofs_do_not_close_runtime_or_conservation_gates !== true ||
    report.policy?.canonical_runtime_input_route_proofs_do_not_close_provider_projection_or_conservation_gates !== true ||
    report.policy?.selected_factor_identity_routes_do_not_close_mechanics_provider_projection_or_conservation_gates !== true ||
    report.policy?.selected_factor_mechanic_routes_do_not_close_provider_projection_or_conservation_gates !== true ||
    report.policy?.selected_factor_capture_correlations_do_not_close_counterfactual_or_conservation_gates !== true ||
    report.policy?.all_shared_proof_receipts_preserve_downstream_runtime_gates !== true ||
    report.policy?.registry_only_proof_routes_are_preserved_without_fabricated_obligations !== true ||
    report.policy?.generic_selector_state_summaries_are_preserved_as_subsumed_evidence !== true ||
    report.policy?.selector_summary_subsumption_requires_same_source_typed_obligations !== true ||
    report.summary?.runtime_formula_models_closed_by_offline_proofs !== 0 ||
    report.summary?.rdps_obligations_closed_by_shared_proof_receipts !== 0) {
    throw new Error("Formula workbench improperly treats a shared proof receipt as downstream runtime closure");
  }

  const obligations = new Map();
  for (const obligation of report.obligations) {
    if (obligations.has(obligation.obligation_id)) throw new Error(`Duplicate obligation ${obligation.obligation_id}`);
    obligations.set(obligation.obligation_id, obligation);
    if (!Array.isArray(obligation.component_evidence)) throw new Error(`Missing component evidence for ${obligation.obligation_id}`);
    if (!["exact-source-rule", "entry-associated", "selector-only", "unmatched-preserved"].includes(obligation.component_evidence_status)) {
      throw new Error(`Invalid component evidence status for ${obligation.obligation_id}`);
    }
    const targetedProofIds = new Set();
    for (const receipt of obligation.targeted_proof_receipts ?? []) {
      if (!ALLOWED_PROOF_RECEIPT_STATES.has(receipt.state) || !receipt.still_required_runtime_gates?.length || targetedProofIds.has(receipt.proof_id)) {
        throw new Error(`Invalid targeted proof receipt on ${obligation.obligation_id}`);
      }
      if (!(receipt.source_rule_ids ?? []).includes(obligation.source_rule_id) && !(receipt.obligation_ids ?? []).includes(obligation.obligation_id)) {
        throw new Error(`Targeted proof receipt ${receipt.proof_id} does not cover ${obligation.obligation_id}`);
      }
      targetedProofIds.add(receipt.proof_id);
    }
  }
  const subsumedEvidenceIds = new Set();
  for (const subsumed of report.subsumed_blockers ?? []) {
    if (!subsumed.blocker_evidence_id || subsumedEvidenceIds.has(subsumed.blocker_evidence_id) || obligations.has(subsumed.blocker_evidence_id)) {
      throw new Error(`Duplicate subsumed blocker evidence ${subsumed.blocker_evidence_id}`);
    }
    subsumedEvidenceIds.add(subsumed.blocker_evidence_id);
    if (subsumed.blocker !== GENERIC_RUNTIME_SELECTOR_SUMMARY_BLOCKER || !subsumed.subsumed_by_obligation_ids?.length) {
      throw new Error(`Invalid subsumed selector summary ${subsumed.blocker_evidence_id}`);
    }
    for (const obligationId of subsumed.subsumed_by_obligation_ids) {
      const obligation = obligations.get(obligationId);
      if (!obligation || obligation.source_rule_id !== subsumed.source_rule_id || obligation.blocker === GENERIC_RUNTIME_SELECTOR_SUMMARY_BLOCKER) {
        throw new Error(`Selector summary ${subsumed.blocker_evidence_id} has an invalid typed sibling ${obligationId}`);
      }
    }
  }
  const grouped = [];
  const modelKeys = new Set();
  for (const group of report.model_groups) {
    if (modelKeys.has(group.model_key)) throw new Error(`Duplicate model key ${group.model_key}`);
    modelKeys.add(group.model_key);
    if (group.model_key !== `${group.model_family}:${group.component_key}`) throw new Error(`Invalid model key ${group.model_key}`);
    if (group.obligation_count !== group.obligation_ids.length) throw new Error(`Obligation count mismatch in ${group.model_key}`);
    if (group.source_count !== new Set(group.source_rule_ids).size) throw new Error(`Source count mismatch in ${group.model_key}`);
    const receiptCount = group.proof_receipts?.length ?? 0;
    const receiptStates = uniqueSorted((group.proof_receipts ?? []).map((receipt) => receipt.state));
    const expectedSharedStatus = receiptCount ? "exact-current-build-shared-proof-received-downstream-runtime-open" : "proof-open";
    const expectedOfflineStatus = receiptStates.includes(OFFLINE_FORMULA_PROOF_STATE)
      ? "exact-current-build-offline-formula-proven-runtime-open"
      : "proof-open";
    const expectedRouteStatus = receiptStates.includes(CANONICAL_RUNTIME_INPUT_ROUTE_PROOF_STATE)
      ? "exact-current-build-canonical-runtime-input-route-proven-downstream-runtime-open"
      : "proof-open";
    if (group.shared_proof_status !== expectedSharedStatus ||
      group.offline_formula_proof_status !== expectedOfflineStatus ||
      group.canonical_runtime_input_route_proof_status !== expectedRouteStatus ||
      JSON.stringify(group.proof_states ?? []) !== JSON.stringify(receiptStates)) {
      throw new Error(`Shared proof status mismatch in ${group.model_key}`);
    }
    for (const receipt of group.proof_receipts ?? []) {
      if (!ALLOWED_PROOF_RECEIPT_STATES.has(receipt.state) || !receipt.model_keys?.includes(group.model_key) || !receipt.still_required_runtime_gates?.length) {
        throw new Error(`Invalid shared proof receipt in ${group.model_key}`);
      }
    }
    if (group.registry_only_proof_route === true) {
      if (group.source_count !== 0 || group.obligation_count !== 0 || group.source_rule_ids.length !== 0 ||
        group.obligation_ids.length !== 0 || receiptCount === 0) {
        throw new Error(`Registry-only proof route ${group.model_key} fabricated static obligations or lost its receipt`);
      }
    } else if (group.registry_only_proof_route !== false) {
      throw new Error(`Static model ${group.model_key} must declare registry_only_proof_route=false`);
    }
    const groupTargetedProofIds = uniqueSorted((group.targeted_proof_receipts ?? []).map((receipt) => receipt.proof_id));
    const expectedTargetedObligations = group.obligation_ids.filter((obligationId) => (obligations.get(obligationId)?.targeted_proof_receipts?.length ?? 0) > 0).sort(compareText);
    const expectedTargetedProofIds = uniqueSorted(expectedTargetedObligations.flatMap((obligationId) => obligations.get(obligationId).targeted_proof_receipts.map((receipt) => receipt.proof_id)));
    if (JSON.stringify(group.targeted_obligation_ids ?? []) !== JSON.stringify(expectedTargetedObligations) ||
      JSON.stringify(groupTargetedProofIds) !== JSON.stringify(expectedTargetedProofIds)) {
      throw new Error(`Targeted proof aggregation mismatch in ${group.model_key}`);
    }
    for (const obligationId of group.obligation_ids) {
      const obligation = obligations.get(obligationId);
      if (!obligation) throw new Error(`Unknown obligation ${obligationId}`);
      if (obligation.model_key !== group.model_key) throw new Error(`Model mismatch for ${obligationId}`);
      grouped.push(obligationId);
    }
  }
  if (grouped.length !== obligations.size || new Set(grouped).size !== obligations.size) {
    throw new Error("Every static blocker obligation must occur in exactly one shared model group");
  }
  const sources = new Set([
    ...report.obligations.map((entry) => entry.source_rule_id),
    ...(report.subsumed_blockers ?? []).map((entry) => entry.source_rule_id),
  ]);
  if (sources.size !== report.summary.pending_sources) throw new Error("Pending source count mismatch");
  const componentEvidenceCount = Object.values(report.summary.component_evidence_counts ?? {}).reduce((sum, count) => sum + count, 0);
  if (componentEvidenceCount !== obligations.size) throw new Error("Component evidence summary count mismatch");
  const proofReceivedModelCount = report.model_groups.filter((group) => (group.proof_receipts?.length ?? 0) > 0).length;
  const offlineProvenModelCount = report.model_groups.filter((group) => (group.proof_states ?? []).includes(OFFLINE_FORMULA_PROOF_STATE)).length;
  const routeProvenModelCount = report.model_groups.filter((group) => (group.proof_states ?? []).includes(CANONICAL_RUNTIME_INPUT_ROUTE_PROOF_STATE)).length;
  const registryOnlyModelCount = report.model_groups.filter((group) => group.registry_only_proof_route === true).length;
  const receivedRegistryKeys = new Set(report.model_groups.flatMap((group) =>
    (group.proof_receipts ?? []).flatMap((receipt) => receipt.model_keys ?? [])));
  if (proofReceivedModelCount !== report.summary.exact_current_build_shared_proof_received_models ||
    offlineProvenModelCount !== report.summary.exact_current_build_offline_formula_proven_models ||
    routeProvenModelCount !== report.summary.exact_current_build_canonical_runtime_input_route_proven_models ||
    registryOnlyModelCount !== report.summary.registry_only_proof_route_models ||
    receivedRegistryKeys.size !== report.summary.shared_registry_model_keys) {
    throw new Error("Shared proof receipt summary mismatch");
  }
  const targetedProofObligations = report.obligations.filter((entry) => (entry.targeted_proof_receipts?.length ?? 0) > 0);
  const targetedProofReceiptIds = new Set(targetedProofObligations.flatMap((entry) => entry.targeted_proof_receipts.map((receipt) => receipt.proof_id)));
  if (targetedProofObligations.length !== report.summary.exact_current_build_targeted_proof_received_obligations ||
    targetedProofReceiptIds.size !== report.summary.targeted_proof_receipts) {
    throw new Error("Targeted proof receipt summary mismatch");
  }
  console.log(`Formula model workbench verified for build ${report.game_build}: ${sources.size} sources, ${obligations.size} actionable blocker obligations, ${subsumedEvidenceIds.size} preserved selector summaries, ${report.model_groups.length} shared proof models, ${targetedProofObligations.length} targeted proof obligations, zero hidden omissions.`);
  return report;
}

function inspect(input, parsed) {
  const report = verify(input);
  const requestedModel = parsed.model;
  const requestedFamily = parsed.family;
  const limit = parsed.limit === undefined ? 20 : positiveInteger(parsed.limit, "limit");
  let groups = report.model_groups;
  if (requestedModel) groups = groups.filter((group) => group.model_key === requestedModel);
  if (requestedFamily) groups = groups.filter((group) => group.model_family === requestedFamily);
  if (requestedModel && groups.length === 0) throw new Error(`Unknown model key ${requestedModel}`);
  if (requestedFamily && groups.length === 0) throw new Error(`No model groups in family ${requestedFamily}`);

  const selected = groups.slice(0, limit);
  console.log(`\nStatic formula proof models for build ${report.game_build}: showing ${selected.length} of ${groups.length}`);
  for (const group of selected) {
    console.log(`\n${group.model_key} | ${group.source_count} sources | ${group.obligation_count} obligations`);
    console.log(`  Contract: ${group.proof_contract}`);
    console.log(`  Component evidence: ${JSON.stringify(group.component_evidence_counts)} | manual bindings ${group.manual_component_binding_obligations} | runtime selectors ${group.runtime_selector_obligations}`);
    if (group.proof_model_ids.length) console.log(`  Extracted models: ${group.proof_model_ids.join(", ")}`);
    console.log(`  Sources: ${group.source_rule_ids.join(", ")}`);
    if (group.formula_term_ids.length) console.log(`  Terms: ${group.formula_term_ids.join(", ")}`);
    if (group.effect_ids.length) console.log(`  Effects: ${group.effect_ids.join(", ")}`);
    console.log(`  Blockers: ${group.blocker_texts.join(" | ")}`);
  }
}

function selfTest() {
  const root = mkdtempSync(path.join(tmpdir(), "rlogs-formula-workbench-test-"));
  try {
    const input = path.join(root, "static.json");
    const valueProofInput = path.join(root, "value-proof.json");
    const proofRegistryInput = path.join(root, "proof-registry.json");
    const output = path.join(root, "workbench.json");
    writeJson(input, {
      schema_version: 1,
      game_build: "1",
      sources: [
        { source_rule_id: "a", source_id: "a", source_name: "A", classification: "complete-single-value", static_gate_resolved: false, remaining_static_blockers: ["component:critical-rate:expected-value-model-required"], formula_term_ids: ["crit"], effect_ids: [1], accepted_terms: [], evidence_sha256: "a" },
        { source_rule_id: "b", source_id: "b", source_name: "B", classification: "unit-or-formula-model-required", static_gate_resolved: false, remaining_static_blockers: ["component:critical-rate:expected-value-model-required", "component:mastery:stat-conversion-model-required"], formula_term_ids: ["crit", "mastery"], effect_ids: [2], accepted_terms: [], evidence_sha256: "b" },
        { source_rule_id: "c", source_id: "c", source_name: "C", classification: "missing-value-evidence", static_gate_resolved: false, remaining_static_blockers: [], formula_term_ids: [], effect_ids: [3], accepted_terms: [], evidence_sha256: "c" },
        { source_rule_id: "d", source_id: "d", source_name: "D", classification: "unit-or-formula-model-required", static_gate_resolved: false, remaining_static_blockers: ["component:equipment-set-attribute:5001:475:2405160:adaptive-primary-stat:stat-conversion-model-required"], formula_term_ids: ["adaptive-primary-stat"], effect_ids: [4], accepted_terms: [], evidence_sha256: "d" },
        { source_rule_id: "e", source_id: "e", source_name: "E", classification: "unit-or-formula-model-required", static_gate_resolved: false, remaining_static_blockers: ["component:adaptive-primary-stat:stat-conversion-model-required"], formula_term_ids: ["adaptive-primary-stat"], effect_ids: [5], accepted_terms: [], evidence_sha256: "e" },
        { source_rule_id: "f", source_id: "f", source_name: "F", classification: "unit-or-formula-model-required", static_gate_resolved: false, remaining_static_blockers: ["component:equipment-set-attribute:4001:459:2405150:formula-input:mastery:stat-conversion-model-required"], formula_term_ids: ["mastery"], effect_ids: [6], accepted_terms: [], evidence_sha256: "f" },
        { source_rule_id: "g", source_id: "g", source_name: "G", classification: "unit-or-formula-model-required", static_gate_resolved: false, remaining_static_blockers: ["component:atk:runtime-formula-inputs-required", GENERIC_RUNTIME_SELECTOR_SUMMARY_BLOCKER], formula_term_ids: ["primaryAttack"], effect_ids: [7], accepted_terms: [], evidence_sha256: "g" },
        { source_rule_id: "closed", static_gate_resolved: true, remaining_static_blockers: [] },
      ],
    });
    writeJson(valueProofInput, {
      schemaVersion: 1,
      expectedValueModels: {
        "critical-expected-v1": { id: "critical-expected-v1", status: "contract-ready-unvalidated", contributionReady: false },
      },
      entriesByKey: {
        "buffs:1": {
          uid: "1",
          category: "buffs",
          sourceLabel: "A",
          valueProofStatus: "needs-expected-model",
          sourceRuleIds: ["a"],
          directSourceRuleIds: ["a"],
          selectedValues: [{ componentKey: "critical-rate", sourceRuleId: "a", value: 5, decimalValue: 0.05, unit: "percent" }],
          valueSelectors: [{ kind: "critical-expected-value", componentKey: "critical-rate", modelId: "critical-expected-v1", contributionReady: false }],
          valueBlockers: ["component:critical-rate:expected-value-model-required"],
        },
        "talents:2": {
          uid: "2",
          category: "talents",
          sourceLabel: "B",
          valueProofStatus: "needs-expected-model",
          sourceRuleIds: ["b"],
          directSourceRuleIds: ["b"],
          selectedValues: [
            { componentKey: "critical-rate", value: 3, decimalValue: 0.03, unit: "percent" },
            { componentKey: "mastery", sourceRuleId: "b", value: 3, decimalValue: 0.03, unit: "percent" },
          ],
          valueSelectors: [
            { kind: "critical-expected-value", componentKey: "critical-rate", modelId: "critical-expected-v1", contributionReady: false },
            { kind: "stat-conversion-model", componentKey: "mastery", contributionReady: false },
          ],
          valueBlockers: ["component:critical-rate:expected-value-model-required", "component:mastery:stat-conversion-model-required"],
        },
      },
    });
    const proofRegistry = {
      schema_version: 3,
      generated_by: "tools/bpsr-shared-formula-proof-registry.mjs",
      game_build: "1",
      policy: { offline_formula_proof_does_not_close_runtime_gates: true, canonical_runtime_input_route_proof_does_not_close_provider_projection_or_conservation_gates: true, selected_factor_mechanic_routes_do_not_close_provider_projection_or_conservation_gates: true, selected_factor_capture_correlations_do_not_close_counterfactual_or_conservation_gates: true, proof_receipts_do_not_promote_rdps_obligations: true },
      summary: { proof_receipts: 5, covered_model_keys: 2, covered_source_rule_ids: 1, covered_obligation_ids: 3, runtime_gates_closed: 0, rdps_obligations_promoted: 0 },
      proof_receipts: [
        { proof_id: "test-mastery", state: "exact-current-build-offline-formula-proven", model_keys: ["stat-conversion:mastery"], still_required_runtime_gates: ["runtime-input"] },
        { proof_id: "test-runtime-family", state: CANONICAL_RUNTIME_INPUT_ROUTE_PROOF_STATE, model_keys: ["runtime-input:test-family"], still_required_runtime_gates: ["provider", "projection", "conservation"] },
        { proof_id: "test-selected-factor", state: SELECTED_FACTOR_ROUTE_PROOF_STATE, model_keys: [], source_rule_ids: ["g"], obligation_ids: ["g#selectedFactorGrade"], still_required_runtime_gates: ["dirty-transition", "mechanics"] },
        { proof_id: "test-selected-factor-capture", state: SELECTED_FACTOR_CAPTURE_CORRELATION_PROOF_STATE, model_keys: [], source_rule_ids: ["g"], obligation_ids: ["g#selectedFactorCaptureCorrelation"], still_required_runtime_gates: ["provider", "projection", "conservation"] },
        { proof_id: "test-selected-factor-mechanic", state: SELECTED_FACTOR_MECHANIC_PROOF_STATE, model_keys: [], source_rule_ids: ["g"], obligation_ids: ["g#selectedFactorMechanics"], still_required_runtime_gates: ["provider", "projection", "conservation"] },
      ],
    };
    proofRegistry.content_sha256 = contentHash(proofRegistry);
    writeJson(proofRegistryInput, proofRegistry);
    build({ build: "1", staticFormulaEvidence: input, valueProof: valueProofInput, proofRegistry: proofRegistryInput, output });
    const report = verify(output);
    if (report.summary.pending_sources !== 7 || report.summary.blocker_obligations !== 8 || report.summary.subsumed_blocker_evidence !== 1 ||
      report.summary.preserved_blocker_evidence !== 9 || report.summary.shared_model_groups !== 6 ||
      report.summary.registry_only_proof_route_models !== 1 || report.summary.shared_registry_model_keys !== 2) {
      throw new Error("Self-test grouping counts failed");
    }
    if (report.model_groups.some((group) => group.model_key === "runtime-selector:general") ||
      report.subsumed_blockers?.[0]?.subsumed_by_obligation_ids?.[0] !== "g#0") {
      throw new Error("Self-test did not preserve the generic selector summary as same-source subsumed evidence");
    }
    if (report.model_groups.find((group) => group.model_key === "expected-value:critical-rate")?.source_count !== 2) {
      throw new Error("Self-test did not reuse the shared critical-rate model");
    }
    if (report.summary.exact_component_obligations !== 2 || report.summary.manual_component_binding_obligations !== 1) {
      throw new Error("Self-test component binding classification failed");
    }
    if (report.model_groups.find((group) => group.model_key === "stat-conversion:adaptive-primary-stat")?.source_count !== 2) {
      throw new Error("Self-test did not collapse wrapped and direct adaptive-primary-stat models");
    }
    if (report.model_groups.find((group) => group.model_key === "stat-conversion:mastery")?.source_count !== 2) {
      throw new Error("Self-test did not collapse wrapped and direct mastery models");
    }
    if (report.model_groups.find((group) => group.model_key === "stat-conversion:mastery")?.offline_formula_proof_status !==
      "exact-current-build-offline-formula-proven-runtime-open" ||
      report.summary.exact_current_build_offline_formula_proven_models !== 1 ||
      report.summary.exact_current_build_shared_proof_received_models !== 2 ||
      report.summary.exact_current_build_canonical_runtime_input_route_proven_models !== 1 ||
      report.summary.exact_current_build_targeted_proof_received_obligations !== 1 ||
      report.summary.targeted_proof_receipts !== 3 ||
      report.obligations.find((entry) => entry.source_rule_id === "g")?.targeted_proof_receipts?.map((entry) => entry.proof_id).join(",") !== "test-selected-factor,test-selected-factor-capture,test-selected-factor-mechanic") {
      throw new Error("Self-test did not attach the exact offline proof while preserving runtime-open status");
    }
    const runtimeFamily = report.model_groups.find((group) => group.model_key === "runtime-input:test-family");
    if (!runtimeFamily?.registry_only_proof_route || runtimeFamily.source_count !== 0 || runtimeFamily.obligation_count !== 0 ||
      runtimeFamily.proof_receipts?.[0]?.proof_id !== "test-runtime-family") {
      throw new Error("Self-test did not preserve the registry-only runtime-input proof route");
    }
    console.log("bpsr-formula-model-workbench self-test passed");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

function familySourceCounts(groups) {
  const families = new Map();
  for (const group of groups) {
    if (!families.has(group.model_family)) families.set(group.model_family, new Set());
    for (const source of group.source_rule_ids) families.get(group.model_family).add(source);
  }
  return Object.fromEntries([...families.entries()].sort(([left], [right]) => compareText(left, right)).map(([family, sources]) => [family, sources.size]));
}

function countBy(values, selector) {
  const counts = {};
  for (const value of values) {
    const key = selector(value);
    counts[key] = (counts[key] ?? 0) + 1;
  }
  return Object.fromEntries(Object.entries(counts).sort(([left], [right]) => compareText(left, right)));
}

function compareObligations(left, right) {
  return compareText(left.model_key, right.model_key) || compareText(left.source_rule_id, right.source_rule_id) || compareText(left.obligation_id, right.obligation_id);
}

function compareGroups(left, right) {
  return right.source_count - left.source_count || right.obligation_count - left.obligation_count || compareText(left.model_key, right.model_key);
}

function compareIdentifiers(left, right) {
  const leftNumber = Number(left);
  const rightNumber = Number(right);
  if (Number.isSafeInteger(leftNumber) && Number.isSafeInteger(rightNumber) && leftNumber !== rightNumber) return leftNumber - rightNumber;
  return compareText(left, right);
}

function compareText(left, right) { return String(left).localeCompare(String(right), "en"); }
function uniqueSorted(values, comparator = compareText) { return [...new Set(values.map((value) => String(value)))].sort(comparator); }
function slug(value) { return String(value).toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "general"; }

function fileDescriptor(file) { return { path: file.replaceAll("\\", "/"), bytes: statSync(file).size, sha256: hashFile(file) }; }
function contentHash(value) { const clone = structuredClone(value); delete clone.content_sha256; return hashText(stableStringify(clone)); }
function stableStringify(value) { if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`; if (value && typeof value === "object") return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`; return JSON.stringify(value); }
function hashFile(file) { return createHash("sha256").update(readFileSync(file)).digest("hex"); }
function hashText(value) { return createHash("sha256").update(value).digest("hex"); }
function requireBuild(value, build, field, label) { if (String(value[field]) !== String(build)) throw new Error(`${label} build ${value[field]} does not match ${build}`); }
function requireFile(file, label) { if (!existsSync(file) || !statSync(file).isFile()) throw new Error(`Missing ${label}: ${file}`); }
function readJson(file, label) { try { return JSON.parse(readFileSync(file, "utf8")); } catch (error) { throw new Error(`Unable to read ${label} ${file}: ${error.message}`); } }
function writeJson(file, value) { writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`, "utf8"); }
function parseArgs(args) { const parsed = {}; for (let index = 0; index < args.length; index += 1) { const arg = args[index]; if (!arg.startsWith("--")) throw new Error(`Unexpected argument ${arg}`); const key = arg.slice(2); const value = args[index + 1]; if (value === undefined || value.startsWith("--")) throw new Error(`Missing value for --${key}`); parsed[key] = value; index += 1; } return parsed; }
function required(value, key) { if (!value[key]) throw new Error(`Missing --${key}`); return value[key]; }
function positiveInteger(value, label) { const parsed = Number(value); if (!Number.isSafeInteger(parsed) || parsed <= 0) throw new Error(`--${label} must be a positive integer`); return parsed; }
function usage(exitCode) { console.log("Usage:\n  node tools/bpsr-formula-model-workbench.mjs build --build <id> --static-formula-evidence <json> --value-proof <json> --proof-registry <json> --output <json>\n  node tools/bpsr-formula-model-workbench.mjs verify --input <json>\n  node tools/bpsr-formula-model-workbench.mjs inspect --input <json> [--model <key>] [--family <name>] [--limit <count>]\n  node tools/bpsr-formula-model-workbench.mjs self-test"); process.exit(exitCode); }
