#!/usr/bin/env node

import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const options = parseArgs(process.argv.slice(2));
const paths = Object.fromEntries(
  [
    "effectSources",
    "talentOwnership",
    "equipmentSets",
    "formulaModels",
    "damageLedger",
    "packetDamageScope",
    "semanticAudit",
    "remainingTrace",
    "reachability",
    "factorClosure",
    "aoyiLedger",
    "staticValueProof",
    "producedDamageReferenceScan",
    "formulaExecutionProof",
    "formulaApplicabilityProof",
    "luckyExecutorProof",
    "serverAuthoredExecutorProof",
    "missingScriptDispositionProof",
    "primaryStatAttackTransformProof",
    "targetMitigationOfflineProof",
    "masteryPropertyOfflineProof",
  ].map((key) => [key, resolvePath(options[key])]),
);
const outputPath = resolvePath(options.output);

const effectSources = readJson(paths.effectSources, "effect sources");
const talentOwnership = readJson(paths.talentOwnership, "talent ownership");
const equipmentSets = readJson(paths.equipmentSets, "equipment set effects");
const formulaModels = readJson(paths.formulaModels, "offline formula models");
const damageLedger = readJson(paths.damageLedger, "damage resolution ledger");
const packetDamageScope = readJson(paths.packetDamageScope, "packet-observed damage scope");
const semanticAudit = readJson(paths.semanticAudit, "static rDPS semantic audit");
const remainingTrace = readJson(paths.remainingTrace, "remaining offensive trace");
const reachability = readJson(paths.reachability, "current-build effect reachability");
const factorClosure = readJson(paths.factorClosure, "Psychoscope factor closure");
const aoyiLedger = readJson(paths.aoyiLedger, "Aoyi origin ledger");
const staticValueProof = readJson(paths.staticValueProof, "current-build static value proof");
const producedDamageReferenceScan = readJson(
  paths.producedDamageReferenceScan,
  "produced-damage decoded-table reference scan",
);
const formulaExecutionProof = readJson(paths.formulaExecutionProof, "packet formula execution proof");
const formulaApplicabilityProof = readJson(
  paths.formulaApplicabilityProof,
  "packet formula applicability proof",
);
const luckyExecutorProof = readJson(paths.luckyExecutorProof, "Lucky damage executor proof");
const serverAuthoredExecutorProof = readJson(
  paths.serverAuthoredExecutorProof,
  "server-authored damage executor proof",
);
const missingScriptDispositionProof = readJson(
  paths.missingScriptDispositionProof,
  "missing-script damage disposition proof",
);
const primaryStatAttackTransformProof = readJson(
  paths.primaryStatAttackTransformProof,
  "primary-stat attack transform proof",
);
const targetMitigationOfflineProof = readJson(
  paths.targetMitigationOfflineProof,
  "target-mitigation offline exhaustion proof",
);
const masteryPropertyOfflineProof = readJson(
  paths.masteryPropertyOfflineProof,
  "Mastery-property offline exhaustion proof",
);

validateBuilds();
const completeFormulaModelIds = validateCompletedFormulaModelProofs();
const offlineExhaustedFormulaModelIds = validateOfflineExhaustedFormulaModelProofs();

const obligations = new Map();
const equipmentSourceIds = new Set();
const observedAbilityIds = new Set(
  (packetDamageScope.observed_ability_ids || []).map(Number).filter(Number.isSafeInteger),
);
const observedDamageRows = (damageLedger.entries || []).filter((entry) =>
  observedAbilityIds.has(Number(entry.ability_id)),
);
const definitionOnlyUnreachableSourceIds = new Set(
  Object.values(effectSources.effectSourcesById || {})
    .filter((source) => {
      const effectIds = unique([
        ...(source.buffIds || []),
        source.sourceEntityId,
      ].map(Number).filter(Number.isSafeInteger));
      if (effectIds.length === 0) return false;
      return effectIds.every((effectId) =>
        reachability.effectsById?.[String(effectId)]?.reachabilityStatus
          === "definition-only-no-current-incoming-reference",
      );
    })
    .map((source) => String(source.sourceId || ""))
    .filter(Boolean),
);
const packetBoundProducedDamageRoutes = buildPacketBoundProducedDamageRoutes();
const packetBoundProducedDamageRuleIds = new Set(
  packetBoundProducedDamageRoutes.map((row) => row.source_rule_id),
);
const completeStaticMagnitudeProofsByRuleId = new Map(
  (staticValueProof.sources || [])
    .filter((row) => row.static_value_status === "complete-ladder" || row.static_value_status === "exact-formula")
    .map((row) => [String(row.source_rule_id || ""), row])
    .filter(([ruleId]) => ruleId),
);

for (const family of equipmentSets.families || []) {
  for (const threshold of family.thresholds || family.threshold_rows || []) {
    for (const reference of threshold.suit_attribute_references || threshold.attribute_references || threshold.attributeReferences || []) {
      if (!String(reference.attribute_library_status || "").startsWith("missing")) continue;
      const type = Number(reference.attr_type);
      const id = Number(reference.attribute_library_id);
      addObligation({
        key: `equipment-library:${type}:${id}`,
        domain: "equipment-sets",
        kind: "typed-effect-library-route",
        title: `Resolve equipment-set effect library type ${type}, id ${id}`,
        evidence: { family_id: family.suit_id || family.family_id, required_pieces: threshold.required_pieces },
      });
      equipmentSourceIds.add(`equipment-set:${family.suit_id || family.family_id}:${threshold.required_pieces}`);
    }
  }
}

const semanticBlockedSourceIds = new Set(
  (semanticAudit.findings || [])
    .filter((finding) => (finding.issues || []).some((issue) => issue.promotion_blocker))
    .map((finding) => String(finding.source_id || ""))
    .filter(Boolean),
);

for (const source of Object.values(effectSources.effectSourcesById || {})) {
  const status = source.attributionModel?.status || source.formulaAttribution?.status;
  if (!["needs-component-classification", "needs-formula-proof"].includes(status)) continue;
  if (equipmentSourceIds.has(source.sourceId)) continue;
  // A source whose components are already classified can still be marked
  // needs-formula-proof while the semantic audit holds the exact downstream
  // conservation/model leaf. Count that proof once under source-semantics;
  // retain effect-sources for genuinely unclassified components.
  if (status === "needs-formula-proof" && semanticBlockedSourceIds.has(String(source.sourceId))) continue;
  addObligation({
    key: `effect-source:${source.sourceId}:${status}`,
    domain: "effect-sources",
    kind: status,
    title: `${source.sourceName || source.sourceId}: ${status}`,
    evidence: {
      source_id: source.sourceId,
      source_kind: source.sourceKind,
      buff_ids: source.buffIds || [],
      component_keys: (source.effectComponents || []).map((row) => row.componentKey),
    },
  });
}

for (const talentId of talentOwnership.worklists?.ambiguousDpsTalentIds || []) {
  addTalentObligation(talentId, "ambiguous");
}
for (const talentId of talentOwnership.worklists?.specLeaningDpsTalentIds || []) {
  addTalentObligation(talentId, "spec-leaning");
}

for (const model of formulaModels.models || []) {
  if (model.state === "offline-formula-complete"
    || completeFormulaModelIds.has(String(model.model_id))
    || offlineExhaustedFormulaModelIds.has(String(model.model_id))) continue;
  addObligation({
    key: `formula-model:${model.model_id}`,
    domain: "core-formulas",
    kind: model.state || "offline-proof-needed",
    title: model.model_id,
    evidence: { missing_proof: model.missing_proof || null },
  });
}

for (const entry of observedDamageRows) {
  if (entry.source?.state !== "unresolved") continue;
  addObligation({
    key: `damage-source:${entry.lookup_key}`,
    domain: "damage-source-routing",
    kind: "missing-static-source-route",
    title: `Resolve source route for ${entry.lookup_key}`,
    evidence: compactDamageEntry(entry),
  });
}

const nonstandardFormulaFamilies = new Map();
const missingFormulaSignatures = new Map();
const healingOnlyFormulaRows = [];
const retainedIncomingOnlyFormulaRows = [];
const incomingOnlyDamageIds = new Set(
  (formulaApplicabilityProof.rows || [])
    .filter(
      (row) => row.rdps_formula_disposition
        === "retained-incoming-only-no-outgoing-counterfactual",
    )
    .map((row) => Number(row.damage_attr_id))
    .filter(Number.isSafeInteger),
);
const formulaExecutionRowsByDamageId = new Map(
  (formulaExecutionProof.observed_damage_rows || []).map((row) => [String(row.damage_id), row]),
);
const formulaExecutionKindsByFamily = new Map();
for (const row of formulaExecutionProof.observed_damage_rows || []) {
  const family = String(row.damage_script || "<missing>");
  const counts = formulaExecutionKindsByFamily.get(family) || { damage: 0, healing: 0, witness_damage_attr_ids: [] };
  counts.damage += Number(row.packet_damage_results || 0);
  counts.healing += Number(row.packet_healing_results || 0);
  if (Number(row.packet_damage_results || 0) > 0 || Number(row.packet_healing_results || 0) > 0) {
    counts.witness_damage_attr_ids.push(Number(row.damage_id));
  }
  formulaExecutionKindsByFamily.set(family, counts);
}
const completeLuckyExecutorsByFamily = validateLuckyExecutorProof();
const completeServerAuthoredExecutorsByFamily = validateServerAuthoredExecutorProof();
const completeMissingScriptDispositionsByDamageId = validateMissingScriptDispositionProof();
for (const entry of observedDamageRows) {
  if (entry.formula?.state !== "nonstandard-or-missing") continue;
  const family = entry.formula.family || entry.formula.candidate?.damage_script || "<missing>";
  const execution = formulaExecutionRowsByDamageId.get(String(entry.damage_attr_id));
  const familyExecution = formulaExecutionKindsByFamily.get(String(family));
  const luckyExecutor = completeLuckyExecutorsByFamily.get(String(family));
  const serverAuthoredExecutor = completeServerAuthoredExecutorsByFamily.get(String(family));
  const missingScriptDisposition = completeMissingScriptDispositionsByDamageId.get(
    Number(entry.damage_attr_id),
  );
  if (incomingOnlyDamageIds.has(Number(entry.damage_attr_id))) {
    retainedIncomingOnlyFormulaRows.push({
      lookup_key: entry.lookup_key,
      damage_attr_id: entry.damage_attr_id,
      ability_id: entry.ability_id,
      family,
      proof_authority: "exact-packet-row-result-kind-and-target-actor-domain",
      packet_damage_results: Number(execution?.packet_damage_results || 0),
      retained_metrics: ["raw-event", "damage-taken", "tps", "death-timeline"],
    });
    continue;
  }
  if (
    (
      execution
      && Number(execution.packet_healing_results || 0) > 0
      && Number(execution.packet_damage_results || 0) === 0
    )
    || (
      family !== "<missing>"
      && Number(familyExecution?.healing || 0) > 0
      && Number(familyExecution?.damage || 0) === 0
    )
  ) {
    healingOnlyFormulaRows.push({
      lookup_key: entry.lookup_key,
      damage_attr_id: entry.damage_attr_id,
      ability_id: entry.ability_id,
      family,
      proof_authority: execution && Number(execution.packet_healing_results || 0) > 0
        ? "exact-packet-row"
        : "same-damage-script-executor-with-exact-packet-witness",
      packet_healing_results: Number(execution?.packet_healing_results || 0),
      packet_damage_results: Number(execution?.packet_damage_results || 0),
      family_packet_healing_results: Number(familyExecution?.healing || 0),
      family_packet_damage_results: Number(familyExecution?.damage || 0),
      family_witness_damage_attr_ids: numbers(familyExecution?.witness_damage_attr_ids || []),
    });
    continue;
  }
  if (family === "<missing>" && missingScriptDisposition) {
    if (missingScriptDisposition.formula_signature_id
      !== String(entry.formula?.formula_signature_id || "")) {
      throw new Error(
        `missing-script proof signature mismatch for damage row ${entry.damage_attr_id}`,
      );
    }
    continue;
  }
  const completeExecutor = luckyExecutor || serverAuthoredExecutor;
  if (completeExecutor) {
    const signature = String(entry.formula.formula_signature_id || "");
    if (!completeExecutor.damage_attr_ids.has(Number(entry.damage_attr_id))) {
      throw new Error(`${family} executor proof does not cover damage row ${entry.damage_attr_id}`);
    }
    if (!completeExecutor.formula_signature_ids.has(signature)) {
      throw new Error(`${family} executor proof does not cover formula signature ${signature}`);
    }
    continue;
  }
  const signature = entry.formula.formula_signature_id || `row:${entry.lookup_key}:${entry.damage_attr_id}`;
  const rows = family === "<missing>" ? missingFormulaSignatures : nonstandardFormulaFamilies;
  const key = family === "<missing>" ? signature : family;
  const row = rows.get(key) || {
    family,
    signatures: [],
    signature,
    lookup_keys: [],
    damage_attr_ids: [],
  };
  row.signatures.push(signature);
  row.lookup_keys.push(entry.lookup_key);
  row.damage_attr_ids.push(entry.damage_attr_id);
  rows.set(key, row);
}
for (const row of nonstandardFormulaFamilies.values()) {
  addObligation({
    key: `damage-formula-family:${row.family}`,
    domain: "damage-formulas",
    kind: "unproven-damage-script-family-executor",
    title: `Prove the generic ${row.family} script executor and validate every current-build signature`,
    evidence: {
      formula_signatures: unique(row.signatures),
      lookup_keys: unique(row.lookup_keys),
      damage_attr_ids: numbers(row.damage_attr_ids),
    },
  });
}
for (const row of missingFormulaSignatures.values()) {
  addObligation({
    key: `damage-formula-missing-script:${row.signature}`,
    domain: "damage-formulas",
    kind: "missing-damage-script-signature-classification",
    title: `Classify missing-script formula ${row.signature}`,
    evidence: {
      lookup_keys: unique(row.lookup_keys),
      damage_attr_ids: numbers(row.damage_attr_ids),
    },
  });
}

for (const finding of semanticAudit.findings || []) {
  // Preserve dormant definitions and their unresolved semantics in the audit,
  // but do not let a row with no incoming table, asset, or code reference in
  // this exact build block validation of mechanics that can actually execute.
  if (definitionOnlyUnreachableSourceIds.has(String(finding.source_id || ""))) continue;
  for (const issue of finding.issues || []) {
    if (!issue.promotion_blocker) continue;
    if (
      issue.category === "produced-damage-without-packet-row"
      && packetBoundProducedDamageRuleIds.has(String(finding.source_rule_id || ""))
    ) {
      continue;
    }
    if (
      issue.category === "formula-magnitude-unresolved"
      && completeStaticMagnitudeProofsByRuleId.has(String(finding.source_rule_id || ""))
    ) {
      continue;
    }
    addObligation({
      key: `semantic:${finding.source_rule_id}:${issue.category}`,
      domain: "source-semantics",
      kind: issue.category,
      title: `${finding.source_name}: ${issue.category}`,
      evidence: {
        source_rule_id: finding.source_rule_id,
        source_id: finding.source_id,
        required_model: issue.required_model,
        target_damage_ids: finding.target_damage_ids || [],
      },
    });
  }
}

for (const candidate of remainingTrace.candidates || []) {
  for (const raw of candidate.offline_obligations || []) {
    const formulaMatch = /^prove-formula-model:([^:]+):/.exec(raw);
    if (formulaMatch) continue; // already represented by the global formula-model leaf.
    addObligation({
      key: `offensive-trace:${candidate.source_rule_id}:${raw}`,
      domain: "offensive-trace",
      kind: "source-specific-offline-trace",
      title: `${candidate.source_name}: ${raw}`,
      evidence: { source_rule_id: candidate.source_rule_id, raw_obligation: raw },
    });
  }
}

const rows = [...obligations.values()].sort(compareObligations);
const byDomain = countBy(rows, (row) => row.domain);
const byKind = countBy(rows, (row) => row.kind);
const coverage = buildCoverage();
const result = {
  schema_version: 1,
  generated_by: "tools/rdps-global-offline-gate.mjs",
  game: "blue-protocol-star-resonance",
  game_build: String(options.gameBuild),
  policy: {
    capture_gate_is_global: true,
    capture_is_discovery: false,
    capture_is_final_comprehensive_validation_only: true,
    unresolved_evidence_hidden: false,
    obligations_are_deduplicated_by_stable_leaf_key: true,
    named_damage_script_families_are_one_executor_proof_plus_exhaustive_row_validation: true,
    lucky_damage_script_families_use_packet_authoritative_component_executors: true,
    auto_and_special_attack_families_use_server_authored_normal_value_executors: true,
    missing_damage_script_signatures_remain_individual_classification_obligations: true,
    missing_damage_script_dispositions_are_exact_row_fail_closed_proofs: true,
    current_build_primary_stat_attack_transform_is_fail_closed_for_every_active_class_and_spec: true,
    packet_proven_healing_only_formula_rows_are_retained_outside_the_rdps_damage_gate: true,
    packet_proven_incoming_only_formula_rows_are_retained_outside_the_outgoing_rdps_formula_gate: true,
    packet_observed_ability_scope_gates_capture: true,
    all_candidate_hit_rows_for_every_observed_ability_gate_capture: true,
    unobserved_current_build_rows_remain_cataloged_but_do_not_gate_current_capture_validation: true,
    definition_only_current_build_rows_remain_cataloged_but_do_not_gate_current_capture_validation: true,
    row_coverage_units_overlap_and_are_not_a_percentage: true,
  },
  inputs: Object.fromEntries(Object.entries(paths).map(([key, value]) => [key, relative(value)])),
  summary: {
    capture_ready: rows.length === 0,
    offline_obligations_remaining: rows.length,
    obligations_by_domain: byDomain,
    obligations_by_kind: byKind,
    coverage,
    final_validation_obligations_held_until_offline_zero:
      Number(factorClosure.summary?.total_final_validation_obligations || 0)
      + Number(remainingTrace.summary?.failed_proof_gates || 0)
      + packetBoundProducedDamageRoutes.length
      + Number(targetMitigationOfflineProof.summary?.final_validation_obligations || 0)
      + Number(masteryPropertyOfflineProof.summary?.final_validation_obligations || 0),
    packet_bound_produced_damage_routes: {
      total: packetBoundProducedDamageRoutes.length,
      offline_reference_scan_exhausted: packetBoundProducedDamageRoutes.length,
      matching_build_packet_bindings_required: packetBoundProducedDamageRoutes.length,
    },
    packet_proven_healing_only_formula_rows: healingOnlyFormulaRows.length,
    packet_proven_incoming_only_formula_rows: retainedIncomingOnlyFormulaRows.length,
    packet_proven_missing_script_dispositions: completeMissingScriptDispositionsByDamageId.size,
    externally_completed_formula_models: [...completeFormulaModelIds].sort(),
    offline_exhausted_server_counterfactual_models:
      [...offlineExhaustedFormulaModelIds].sort(),
  },
  packet_bound_produced_damage_routes: packetBoundProducedDamageRoutes,
  retained_hps_formula_rows: healingOnlyFormulaRows,
  retained_incoming_formula_rows: retainedIncomingOnlyFormulaRows,
  obligations: rows,
};

writeFileSync(outputPath, `${JSON.stringify(result, null, 2)}\n`, "utf8");
console.log(JSON.stringify(result.summary, null, 2));

function addTalentObligation(talentId, ownershipState) {
  const talent = talentOwnership.talentsById?.[String(talentId)] || {};
  addObligation({
    key: `talent-ownership:${talentId}`,
    domain: "talent-ownership",
    kind: ownershipState,
    title: `${talent.name || talent.names?.en || `Talent ${talentId}`}: exact spec ownership`,
    evidence: {
      talent_id: Number(talentId),
      class_id: talent.classId ?? null,
      candidate_spec_ids: talent.candidateSpecIds || talent.ownership?.candidateSpecIds || [],
      spec_evidence: talent.specEvidence || talent.evidence || [],
    },
  });
}

function addObligation(row) {
  if (!row.key || obligations.has(row.key)) return;
  obligations.set(row.key, row);
}

function buildCoverage() {
  const sourceRows = Object.values(effectSources.effectSourcesById || {});
  const effectUnresolved = sourceRows.filter((row) =>
    ["needs-component-classification", "needs-formula-proof"].includes(row.attributionModel?.status || row.formulaAttribution?.status),
  ).length;
  const talentTotal = Number(talentOwnership.summary?.talentRows || 0);
  const talentReview = (talentOwnership.worklists?.ambiguousDpsTalentIds || []).length
    + (talentOwnership.worklists?.specLeaningDpsTalentIds || []).length;
  const formulaTotal = (formulaModels.models || []).length;
  const formulaRemaining = (formulaModels.models || []).filter((row) =>
    row.state !== "offline-formula-complete"
      && !completeFormulaModelIds.has(String(row.model_id))
      && !offlineExhaustedFormulaModelIds.has(String(row.model_id)),
  ).length;
  const damageRows = damageLedger.entries || [];
  const unobservedDamageRows = damageRows.filter((row) => !observedAbilityIds.has(Number(row.ability_id)));
  const semanticTotal = Number(semanticAudit.summary?.candidates_audited || 0);
  const semanticRemaining = (semanticAudit.findings || []).filter((finding) =>
    !definitionOnlyUnreachableSourceIds.has(String(finding.source_id || ""))
      && (finding.issues || []).some((issue) =>
        issue.promotion_blocker
          && !(
            issue.category === "produced-damage-without-packet-row"
            && packetBoundProducedDamageRuleIds.has(String(finding.source_rule_id || ""))
          )
          && !(
            issue.category === "formula-magnitude-unresolved"
            && completeStaticMagnitudeProofsByRuleId.has(String(finding.source_rule_id || ""))
          ),
      ),
  ).length;
  return {
    psychoscope_factor_routes: coverageRow(
      Number(factorClosure.summary?.current_runtime_families || 0),
      Number(factorClosure.summary?.total_offline_route_obligations || 0),
    ),
    aoyi_damage_chains: coverageRow(
      Number(aoyiLedger.summary?.current_aoyi_skills || 0),
      Number(aoyiLedger.summary?.missing_exact_damage_chain_ids || 0),
    ),
    effect_source_classification: coverageRow(sourceRows.length, effectUnresolved),
    talent_ownership_review: coverageRow(talentTotal, talentReview),
    equipment_typed_libraries: coverageRow(
      Number(equipmentSets.summary?.missing_attribute_library_ids?.length || 0),
      Number(equipmentSets.summary?.missing_attribute_library_ids?.length || 0),
    ),
    core_formula_models: coverageRow(formulaTotal, formulaRemaining),
    packet_observed_damage_source_routes: coverageRow(
      observedDamageRows.length,
      observedDamageRows.filter((row) => row.source?.state === "unresolved").length,
    ),
    packet_observed_damage_formula_rows: coverageRow(
      observedDamageRows.length,
      observedDamageRows.filter(
        (row) => row.formula?.state !== "standard-static-candidate"
          && !incomingOnlyDamageIds.has(Number(row.damage_attr_id))
          && !executorProofCovers(row)
          && !completeMissingScriptDispositionsByDamageId.has(Number(row.damage_attr_id))
          && !healingOnlyFormulaRows.some(
            (retained) => Number(retained.damage_attr_id) === Number(row.damage_attr_id),
          ),
      ).length,
    ),
    retained_unobserved_current_build_damage_rows: {
      total: unobservedDamageRows.length,
      standard_static_candidates: unobservedDamageRows.filter(
        (row) => row.formula?.state === "standard-static-candidate",
      ).length,
      nonstandard_or_missing: unobservedDamageRows.filter(
        (row) => row.formula?.state !== "standard-static-candidate",
      ).length,
      hidden: false,
      gates_current_capture: false,
    },
    semantic_candidates: coverageRow(semanticTotal, semanticRemaining),
  };
}

function buildPacketBoundProducedDamageRoutes() {
  if (producedDamageReferenceScan.policy?.exact_build_required !== true) {
    throw new Error("produced-damage reference scan is not exact-build gated");
  }
  if (producedDamageReferenceScan.policy?.direct_references_are_route_authority !== false) {
    throw new Error("produced-damage reference scan incorrectly treats scalar references as route authority");
  }
  if (Number(producedDamageReferenceScan.summary?.decoded_tables_scanned || 0) === 0) {
    throw new Error("produced-damage reference scan did not inspect decoded tables");
  }

  const scannedTargets = new Map(
    (producedDamageReferenceScan.targets || []).map((row) => [Number(row.value), row]),
  );
  const referencesByValue = new Map();
  for (const reference of producedDamageReferenceScan.references || []) {
    const value = Number(reference.value);
    const references = referencesByValue.get(value) || [];
    references.push(reference);
    referencesByValue.set(value, references);
  }

  const routes = [];
  for (const finding of semanticAudit.findings || []) {
    const producedDamageIssue = (finding.issues || []).find(
      (issue) => issue.promotion_blocker
        && issue.category === "produced-damage-without-packet-row",
    );
    if (!producedDamageIssue) continue;

    const source = effectSources.effectSourcesById?.[String(finding.source_id || "")];
    // The route-bearing runtime identity is the buff/effect ID. A talent or
    // rogue-entry sourceEntityId is its owner row key, not an emitted effect
    // identity, so requiring it in this effect-reference scan would conflate
    // owner identity with the server-selected output row.
    const effectIds = unique(
      (source?.buffIds || []).map(Number).filter(Number.isSafeInteger),
    );
    if (effectIds.length === 0) continue;

    const missingTargets = effectIds.filter((effectId) => !scannedTargets.has(effectId));
    if (missingTargets.length > 0) continue;

    const references = effectIds.flatMap((effectId) => referencesByValue.get(effectId) || []);
    if (references.length === 0 || references.some((reference) => !isIdentityOwnerOrGrantReference(reference))) {
      continue;
    }

    const formulaInputs = packetRouteFormulaInputs(finding);
    routes.push({
      source_rule_id: String(finding.source_rule_id),
      source_id: String(finding.source_id),
      source_name: String(finding.source_name || finding.source_id),
      effect_ids: effectIds,
      offline_route_state: "exhausted-current-build-no-static-output-binding",
      runtime_binding_state: "matching-build-packet-required",
      promotion_blocked_until_packet_binding: true,
      formula_inputs: formulaInputs,
      required_runtime_evidence: unique([
        "matching-build emitted damage row selected while the source mechanic is active",
        "provider and recipient identity for the emitted row",
        "source formula inputs at trigger time",
        "event-level conservation between emitted damage and the recounted source parent",
        ...(finding.existing_components || []).flatMap((component) => component.required_runtime_evidence || []),
      ]),
      static_reference_evidence: {
        decoded_tables_scanned: Number(producedDamageReferenceScan.summary.decoded_tables_scanned),
        decoded_rows_scanned: Number(producedDamageReferenceScan.summary.decoded_rows_scanned),
        references: references.map((reference) => ({
          table: reference.table,
          row_key: reference.row_key,
          json_pointer: reference.json_pointer,
        })),
      },
    });
  }
  return routes.sort((left, right) => left.source_rule_id.localeCompare(right.source_rule_id));
}

// These are the canonical current-value attributes emitted by the BPSR
// protocol, not localized-description numbers. ATK intentionally retains both
// physical and magical current values so matching-build packets, rather than a
// class-name guess, determine which value exists for the triggering actor.
function packetRouteFormulaInputs(finding) {
  const keys = unique(
    (finding.existing_components || [])
      .map((component) => String(component.component_key || ""))
      .filter((key) => key.startsWith("formula-input:"))
      .map((key) => key.slice("formula-input:".length)),
  );
  return keys.map((inputKey) => {
    const contract = packetFormulaInputAttributeContract(inputKey);
    if (!contract) {
      throw new Error(
        `packet route ${finding.source_rule_id} has unsupported structured formula input ${inputKey}`,
      );
    }
    return {
      input_key: inputKey,
      label: contract.label,
      actor_role: "source",
      completion: "any-current-value-observed-before-trigger",
      candidate_attribute_ids: [...contract.candidate_attribute_ids],
      evidence: [...contract.evidence],
    };
  });
}

// This is a function declaration rather than a later-initialized module
// constant because the gate is assembled at module startup above. Function
// declarations are available during that startup pass without a temporal
// dead-zone.
function packetFormulaInputAttributeContract(inputKey) {
  const contracts = {
    atk: {
      label: "current attack",
      candidate_attribute_ids: [11330, 11340],
      evidence: [
        "EAttrType.AttrAttack=11330",
        "current-build primary-stat attack transform proof",
        "current-build attribute-transform family nodes",
      ],
    },
    "max-hp": {
      label: "current maximum HP",
      candidate_attribute_ids: [11320],
      evidence: [
        "BPSR canonical ATTR_MAX_HP_FINAL=11320",
        "current-build Aoyi attribute-family ledger",
      ],
    },
    hp: {
      label: "current HP",
      candidate_attribute_ids: [11310],
      evidence: ["BPSR canonical ATTR_CURRENT_HP=11310"],
    },
    mastery: {
      label: "current effective Mastery percentage",
      candidate_attribute_ids: [11940],
      evidence: ["current-build Mastery percent attribute-transform family node"],
    },
  };
  return contracts[inputKey];
}

function isIdentityOwnerOrGrantReference(reference) {
  const table = String(reference.table || "");
  const pointer = String(reference.json_pointer || "");
  if (table === "BuffTable") return pointer === "/Id" || pointer === "/TipsDescription";
  if (table === "AttrDescription") return pointer === "/Id";
  if (table === "RogueEntryTable") return pointer === "/BuffId";
  if (table === "TalentTable") return /^\/TalentEffect\/\d+\/1$/.test(pointer);
  if (table === "InteractiveTable") {
    return /^\/(?:ActionStage|TriggerCondition)\/\d+\/\d+$/.test(pointer);
  }
  return false;
}

function coverageRow(total, remaining) {
  return { total, resolved: Math.max(0, total - remaining), remaining };
}

function compactDamageEntry(entry) {
  return {
    lookup_key: entry.lookup_key,
    ability_id: entry.ability_id,
    hit_event_id: entry.hit_event_id,
    damage_attr_id: entry.damage_attr_id,
    formula_family: entry.formula?.family || entry.formula?.candidate?.damage_script || entry.formula?.state,
    recount_owners: entry.recount?.owners || [],
    decoded_reference_leads: entry.decoded_reference_leads || [],
  };
}

function validateLuckyExecutorProof() {
  if (String(luckyExecutorProof.game_build) !== String(options.gameBuild)) {
    throw new Error(
      `Lucky executor proof build ${luckyExecutorProof.game_build} differs from requested build ${options.gameBuild}`,
    );
  }
  if (String(luckyExecutorProof.packet_build) !== String(options.packetBuild)) {
    throw new Error(
      `Lucky executor proof packet build ${luckyExecutorProof.packet_build} differs from requested packet build ${options.packetBuild}`,
    );
  }
  if (luckyExecutorProof.summary?.proof_state !== "offline-rdps-executor-complete") {
    throw new Error("Lucky executor proof is not complete");
  }
  if (luckyExecutorProof.policy?.unresolved_evidence_is_hidden !== false) {
    throw new Error("Lucky executor proof does not preserve unresolved evidence");
  }
  if (luckyExecutorProof.policy?.packet_amount_is_authoritative_lucky_component !== true) {
    throw new Error("Lucky executor proof lacks packet-authoritative component identity");
  }
  if (luckyExecutorProof.summary?.explicit_lucky_value_conservation !== true) {
    throw new Error("Lucky executor proof does not conserve explicit lucky_value components");
  }
  const result = new Map();
  for (const family of luckyExecutorProof.families || []) {
    if (family.proof_state !== "offline-rdps-executor-complete") continue;
    const name = String(family.formula_family || "");
    if (!["AttackLucky", "MAttackLucky"].includes(name)) continue;
    const damageAttrIds = new Set(numbers(family.current_build_damage_attr_ids || []));
    const formulaSignatureIds = new Set(
      unique((family.current_build_formula_signature_ids || []).map(String)),
    );
    if (damageAttrIds.size === 0 || formulaSignatureIds.size === 0) {
      throw new Error(`${name} executor proof has no exact row/signature coverage`);
    }
    result.set(name, {
      damage_attr_ids: damageAttrIds,
      formula_signature_ids: formulaSignatureIds,
    });
  }
  return result;
}

function validateServerAuthoredExecutorProof() {
  if (String(serverAuthoredExecutorProof.game_build) !== String(options.gameBuild)) {
    throw new Error(
      `server-authored executor proof build ${serverAuthoredExecutorProof.game_build} differs from requested build ${options.gameBuild}`,
    );
  }
  if (String(serverAuthoredExecutorProof.packet_build) !== String(options.packetBuild)) {
    throw new Error(
      `server-authored executor proof packet build ${serverAuthoredExecutorProof.packet_build} differs from requested packet build ${options.packetBuild}`,
    );
  }
  if (serverAuthoredExecutorProof.summary?.proof_state
    !== "offline-packet-output-executor-complete") {
    throw new Error("server-authored executor proof is not complete");
  }
  if (serverAuthoredExecutorProof.policy?.unresolved_evidence_is_hidden !== false) {
    throw new Error("server-authored executor proof does not preserve unresolved evidence");
  }
  if (serverAuthoredExecutorProof.policy?.packet_normal_value_is_authoritative_component
    !== true) {
    throw new Error("server-authored executor proof lacks normal_value component identity");
  }
  if (serverAuthoredExecutorProof.policy?.base_formula_reconstruction_claimed !== false
    || serverAuthoredExecutorProof.policy?.shared_provider_counterfactuals_remain_separate
      !== true) {
    throw new Error("server-authored executor proof overclaims its packet-output boundary");
  }
  if (serverAuthoredExecutorProof.summary?.normal_value_conservation !== true
    || Number(serverAuthoredExecutorProof.summary?.componentless_nonzero_results || 0) !== 0) {
    throw new Error("server-authored executor proof does not exactly conserve normal_value");
  }
  const result = new Map();
  for (const family of serverAuthoredExecutorProof.families || []) {
    if (family.proof_state !== "offline-packet-output-executor-complete") continue;
    const name = String(family.formula_family || "");
    if (!["AutoAttack", "SpAttack"].includes(name)) continue;
    const damageAttrIds = new Set(numbers(family.current_build_damage_attr_ids || []));
    const formulaSignatureIds = new Set(
      unique((family.current_build_formula_signature_ids || []).map(String)),
    );
    if (damageAttrIds.size === 0 || formulaSignatureIds.size === 0) {
      throw new Error(`${name} server-authored executor proof has no exact row/signature coverage`);
    }
    result.set(name, {
      damage_attr_ids: damageAttrIds,
      formula_signature_ids: formulaSignatureIds,
    });
  }
  return result;
}

function validateMissingScriptDispositionProof() {
  if (String(missingScriptDispositionProof.game_build) !== String(options.gameBuild)) {
    throw new Error(
      `missing-script disposition proof build ${missingScriptDispositionProof.game_build} differs from requested build ${options.gameBuild}`,
    );
  }
  if (String(missingScriptDispositionProof.packet_build) !== String(options.packetBuild)) {
    throw new Error(
      `missing-script disposition proof packet build ${missingScriptDispositionProof.packet_build} differs from requested packet build ${options.packetBuild}`,
    );
  }
  if (missingScriptDispositionProof.summary?.proof_state
    !== "offline-missing-script-classification-complete"
    || Number(missingScriptDispositionProof.summary?.remaining_missing_script_rows || 0) !== 0) {
    throw new Error("missing-script disposition proof is not complete");
  }
  if (missingScriptDispositionProof.policy?.unresolved_evidence_is_hidden !== false
    || missingScriptDispositionProof.policy?.unexecuted_rows_remain_cataloged !== true
    || missingScriptDispositionProof.policy?.newly_observed_exact_lookup_key_reopens_gate !== true
    || missingScriptDispositionProof.policy?.executed_packet_output_is_retained !== true) {
    throw new Error("missing-script disposition proof violates retention/fail-closed policy");
  }
  const rows = missingScriptDispositionProof.rows || [];
  if (rows.length !== 3 || Number(missingScriptDispositionProof.summary.classified_rows) !== rows.length) {
    throw new Error("missing-script disposition proof does not contain the exact three gate rows");
  }
  const expected = new Map([
    [3100970200, "retained-unexecuted-bullet-definition"],
    [124100104, "retained-unexecuted-secondary-skill-definition"],
    [2305444006, "executed-source-owned-current-hp-factor-output"],
  ]);
  const result = new Map();
  for (const row of rows) {
    const damageAttrId = Number(row.damage_attr_id);
    if (row.disposition !== expected.get(damageAttrId)) {
      throw new Error(`unexpected missing-script disposition for damage row ${damageAttrId}`);
    }
    if (!["offline-classification-complete", "offline-formula-boundary-complete"]
      .includes(String(row.proof_state))) {
      throw new Error(`missing-script damage row ${damageAttrId} is not offline-complete`);
    }
    result.set(damageAttrId, {
      formula_signature_id: String(row.formula_signature_id || ""),
      disposition: String(row.disposition),
    });
  }
  if (result.size !== expected.size) {
    throw new Error("missing-script disposition proof contains duplicate or missing rows");
  }
  return result;
}

function validateCompletedFormulaModelProofs() {
  if (String(primaryStatAttackTransformProof.game_build) !== String(options.gameBuild)) {
    throw new Error(
      `primary-stat transform proof build ${primaryStatAttackTransformProof.game_build} differs from requested build ${options.gameBuild}`,
    );
  }
  if (primaryStatAttackTransformProof.proof_state
    !== "offline-primary-stat-attack-transform-complete") {
    throw new Error("primary-stat transform proof is not offline-complete");
  }
  if (primaryStatAttackTransformProof.policy?.exact_build_required !== true
    || primaryStatAttackTransformProof.policy?.descriptions_alone_are_formula_authority !== false
    || primaryStatAttackTransformProof.policy?.formula_requires_structural_opcode_and_description_agreement !== true
    || primaryStatAttackTransformProof.policy?.every_active_class_and_spec_is_fail_closed !== true
    || primaryStatAttackTransformProof.policy?.special_class_tree_conversions_do_not_replace_base_attack_conversion !== true
    || primaryStatAttackTransformProof.policy?.unresolved_evidence_is_hidden !== false
    || primaryStatAttackTransformProof.policy?.future_active_class_or_changed_route_reopens_gate !== true) {
    throw new Error("primary-stat transform proof violates fail-closed proof policy");
  }
  const summary = primaryStatAttackTransformProof.summary || {};
  if (Number(summary.active_classes_proven) !== 9
    || Number(summary.active_specs_proven) !== 18
    || Number(summary.primary_transform_families_proven) !== 3
    || Number(summary.structural_talent_witnesses) !== 6
    || Number(summary.localized_ratio_witnesses) !== 6
    || Number(summary.special_non_attack_conversions_separated) !== 3
    || Number(summary.remaining_supported_class_routes) !== 0) {
    throw new Error("primary-stat transform proof coverage changed");
  }
  const families = primaryStatAttackTransformProof.families || [];
  const expectedFamilies = new Map([
    [11010, [11330, 11332, 1250, "1/8"]],
    [11020, [11340, 11342, 1000, "1/10"]],
    [11030, [11330, 11332, 1250, "1/8"]],
  ]);
  if (families.length !== expectedFamilies.size) {
    throw new Error("primary-stat transform proof family count changed");
  }
  for (const family of families) {
    const expected = expectedFamilies.get(Number(family.primary_attribute_id));
    if (!expected
      || Number(family.attack_attribute_id) !== expected[0]
      || Number(family.attack_add_attribute_id) !== expected[1]
      || Number(family.coefficient_basis_points) !== expected[2]
      || String(family.exact_ratio) !== expected[3]
      || Number(family.fixed_point_denominator) !== 10000
      || !(family.selected_active_class_ids || []).length) {
      throw new Error(`primary-stat transform family ${family.primary_attribute_id} changed`);
    }
  }
  const routes = primaryStatAttackTransformProof.active_class_routes || [];
  if (routes.length !== 9
    || new Set(routes.map((row) => Number(row.class_id))).size !== 9
    || routes.reduce((sum, row) => sum + (row.spec_stage_ids || []).length, 0) !== 18) {
    throw new Error("primary-stat transform class/spec routes are incomplete");
  }
  return new Set(["primary-stat-to-attack-transform"]);
}

function validateOfflineExhaustedFormulaModelProofs() {
  if (String(targetMitigationOfflineProof.game_build) !== String(options.gameBuild)) {
    throw new Error(
      `target-mitigation proof build ${targetMitigationOfflineProof.game_build} differs from requested build ${options.gameBuild}`,
    );
  }
  if (String(targetMitigationOfflineProof.packet_build) !== String(options.packetBuild)) {
    throw new Error(
      `target-mitigation proof packet build ${targetMitigationOfflineProof.packet_build} differs from requested packet build ${options.packetBuild}`,
    );
  }
  if (targetMitigationOfflineProof.proof_state
    !== "offline-client-and-archive-exhausted-final-validation-required") {
    throw new Error("target-mitigation client/archive proof is not offline-exhausted");
  }
  const policy = targetMitigationOfflineProof.policy || {};
  if (policy.exact_build_required !== true
    || policy.unresolved_evidence_is_hidden !== false
    || policy.candidate_constants_are_combat_formula_authority !== false
    || policy.character_sheet_transform_is_combat_formula_authority !== false
    || policy.absence_of_direct_calls_proves_absence_of_indirect_consumers !== false
    || policy.no_formula_is_promoted_without_controlled_packet_counterfactuals !== true
    || policy.archived_zero_pair_result_is_not_formula_proof !== true
    || policy.matching_build_packet_validation_is_required !== true
    || policy.new_client_or_packet_evidence_reopens_offline_exhaustion !== true) {
    throw new Error("target-mitigation proof violates fail-closed evidence policy");
  }
  const summary = targetMitigationOfflineProof.summary || {};
  const expected = new Set([
    "target-physical-armor-counterfactual",
    "elemental-resistance-counterfactual",
  ]);
  const actual = new Set((summary.offline_exhausted_model_ids || []).map(String));
  if (actual.size !== expected.size || [...expected].some((id) => !actual.has(id))
    || Number(summary.final_validation_obligations) !== 2
    || Number(summary.lua_files_scanned) < 4000
    || Number(summary.lua_files_with_attack_simply_names) !== 1
    || Number(summary.native_direct_callsites) !== 0
    || Number(summary.exact_character_sheet_consumers) !== 2
    || Number(summary.packet_axes_audited) !== 12
    || Number(summary.controlled_counterfactual_pairs) !== 0
    || Number(summary.promoted_combat_formulas) !== 0) {
    throw new Error("target-mitigation offline-exhaustion coverage changed");
  }
  if ((targetMitigationOfflineProof.final_validation || []).length !== 2) {
    throw new Error("target-mitigation proof does not preserve both final validation obligations");
  }
  validateMasteryPropertyOfflineProof(actual);
  return actual;
}

function validateMasteryPropertyOfflineProof(result) {
  if (String(masteryPropertyOfflineProof.game_build) !== String(options.gameBuild)
    || String(masteryPropertyOfflineProof.packet_build) !== String(options.packetBuild)) {
    throw new Error("Mastery-property proof build does not match the gate builds");
  }
  if (masteryPropertyOfflineProof.proof_state
    !== "offline-mastery-client-and-archive-exhausted-final-validation-required") {
    throw new Error("Mastery-property proof is not offline-exhausted");
  }
  const policy = masteryPropertyOfflineProof.policy || {};
  if (policy.exact_build_required !== true
    || policy.unresolved_evidence_is_hidden !== false
    || policy.localized_descriptions_are_runtime_formula_authority !== false
    || policy.character_sheet_mastery_curve_is_combat_stage_authority !== false
    || policy.historical_transition_is_current_build_authority !== false
    || policy.isolated_transition_is_absolute_property_formula_authority !== false
    || policy.latest_serialized_attribute_is_action_snapshot_authority !== false
    || policy.candidate_components_are_executable !== false
    || policy.no_component_is_omitted_because_it_is_non_damage !== true
    || policy.matching_build_packet_validation_is_required !== true
    || policy.future_active_spec_or_changed_description_reopens_gate !== true) {
    throw new Error("Mastery-property proof violates fail-closed evidence policy");
  }
  const summary = masteryPropertyOfflineProof.summary || {};
  if (Number(summary.active_classes) !== 9
    || Number(summary.active_specs) !== 18
    || Number(summary.candidate_components) !== 26
    || Number(summary.final_validation_obligations) !== 26
    || Number(summary.damage_or_action_components) !== 16
    || Number(summary.non_damage_components_retained) !== 10
    || Number(summary.inactive_or_unreleased_descriptors_retained) !== 8
    || Number(summary.historical_falconry_delayed_exact_matches) !== 143
    || Number(summary.historical_falconry_delayed_mismatches) !== 0
    || Number(summary.delayed_snapshot_gap_damage_events) !== 374
    || Number(summary.nearby_same_calculation_identity_controls) !== 264
    || Number(summary.strict_state_control_pairs) !== 0
    || Number(summary.promoted_runtime_components) !== 0) {
    throw new Error("Mastery-property offline-exhaustion coverage changed");
  }
  if ((masteryPropertyOfflineProof.components || []).length !== 26
    || (masteryPropertyOfflineProof.final_validation || []).length !== 26
    || (masteryPropertyOfflineProof.inactive_or_unreleased_descriptors || []).length !== 8) {
    throw new Error("Mastery-property proof does not preserve its exact component inventory");
  }
  const modelIds = masteryPropertyOfflineProof.summary?.offline_exhausted_model_ids || [];
  if (modelIds.length !== 1 || modelIds[0] !== "mastery-property-transform") {
    throw new Error("Mastery-property proof model identity changed");
  }
  result.add("mastery-property-transform");
}

function executorProofCovers(entry) {
  if (entry.formula?.state !== "nonstandard-or-missing") return false;
  const family = String(
    entry.formula?.family || entry.formula?.candidate?.damage_script || "<missing>",
  );
  const proof = completeLuckyExecutorsByFamily.get(family)
    || completeServerAuthoredExecutorsByFamily.get(family);
  if (!proof) return false;
  return proof.damage_attr_ids.has(Number(entry.damage_attr_id))
    && proof.formula_signature_ids.has(String(entry.formula?.formula_signature_id || ""));
}

function validateBuilds() {
  const expected = String(options.gameBuild);
  for (const [label, value] of [
    ["formula models", formulaModels.game_build],
    ["damage ledger", damageLedger.game_build],
    ["semantic audit", semanticAudit.game_build],
    ["effect reachability", reachability.gameBuild],
    ["factor closure", factorClosure.game_build],
    ["Aoyi ledger", aoyiLedger.game_build],
    ["static value proof", staticValueProof.static_game_build],
    ["produced-damage reference scan", producedDamageReferenceScan.build_id],
    ["formula execution proof", formulaExecutionProof.game_build],
    ["missing-script disposition proof", missingScriptDispositionProof.game_build],
    ["primary-stat attack transform proof", primaryStatAttackTransformProof.game_build],
    ["target-mitigation offline proof", targetMitigationOfflineProof.game_build],
    ["Mastery-property offline proof", masteryPropertyOfflineProof.game_build],
  ]) {
    if (value !== undefined && String(value) !== expected) {
      throw new Error(`${label} build ${value} differs from requested build ${expected}`);
    }
  }
  if (String(packetDamageScope.packet_build) !== String(options.packetBuild)) {
    throw new Error(
      `packet damage scope build ${packetDamageScope.packet_build} differs from requested packet build ${options.packetBuild}`,
    );
  }
  if (String(formulaExecutionProof.packet_build) !== String(options.packetBuild)) {
    throw new Error(
      `formula execution proof packet build ${formulaExecutionProof.packet_build} differs from requested packet build ${options.packetBuild}`,
    );
  }
}

function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index]?.replace(/^--/, "");
    const value = args[index + 1];
    if (!key || value === undefined) throw new Error(`invalid argument near ${args[index]}`);
    parsed[key] = value;
  }
  for (const required of [
    "gameBuild", "packetBuild", "effectSources", "talentOwnership", "equipmentSets", "formulaModels",
    "damageLedger", "packetDamageScope", "semanticAudit", "remainingTrace", "reachability", "factorClosure", "aoyiLedger",
    "staticValueProof", "producedDamageReferenceScan", "formulaExecutionProof", "output",
    "luckyExecutorProof",
    "serverAuthoredExecutorProof",
    "missingScriptDispositionProof",
    "primaryStatAttackTransformProof",
    "targetMitigationOfflineProof",
    "masteryPropertyOfflineProof",
  ]) {
    if (!parsed[required]) throw new Error(`missing --${required}`);
  }
  return parsed;
}

function resolvePath(value) {
  return path.isAbsolute(value) ? value : path.resolve(repoRoot, value);
}

function readJson(filePath, label) {
  try {
    return JSON.parse(readFileSync(filePath, "utf8"));
  } catch (error) {
    throw new Error(`failed to read ${label} at ${filePath}: ${error.message}`);
  }
}

function relative(filePath) {
  return path.relative(repoRoot, filePath).replaceAll("\\", "/");
}

function unique(values) {
  return [...new Set(values)];
}

function numbers(values) {
  return [...new Set(values.map(Number).filter(Number.isSafeInteger))].sort((left, right) => left - right);
}

function countBy(values, keyOf) {
  const counts = {};
  for (const value of values) {
    const key = keyOf(value);
    counts[key] = (counts[key] || 0) + 1;
  }
  return Object.fromEntries(Object.entries(counts).sort(([left], [right]) => left.localeCompare(right)));
}

function compareObligations(left, right) {
  return left.domain.localeCompare(right.domain) || left.key.localeCompare(right.key);
}
