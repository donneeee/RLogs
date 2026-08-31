#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, "..");
const options = parseArgs(process.argv.slice(2));
const configPath = resolveRepoPath(options.config);
const config = JSON.parse(readFileSync(configPath, "utf8"));
const baselinePath = resolveRepoPath(options.baseline || config.baselineLedger);
const outputPath = resolveRepoPath(options.output || config.ledger.output);
const summaryPath = resolveRepoPath(options.summary || config.summaryOutput);

validateConfig(config);

if (!options.skipExtractors) {
  refreshDerivedExtractorTables(config);
}
runStaticWorklist(config);
runSemanticAudit(config);
runRecipientLedger(config, outputPath);
if (!options.dryRun) {
  refreshCurrentBuildReachability(config, outputPath);
  applyCurrentBuildReachability(config, outputPath);
}
if (options.dryRun) {
  console.log("Dry run completed; no ledger delta report was written.");
  process.exit(0);
}
writeDeltaReport(config, baselinePath, outputPath, summaryPath);
const proofCatalogPath = resolveRepoPath(config.proofCatalogOutput);
writeProofCatalog(config, outputPath, proofCatalogPath);
const attributeDependencyPath = resolveRepoPath(config.relationshipEvidence.attributeDependencyOutput);
writeAttributeTransformDependencyLedger(config, attributeDependencyPath);
const relationshipOverviewPath = resolveRepoPath(config.relationshipOverviewOutput);
writeDamageAttributionRelationshipOverview(
  config,
  proofCatalogPath,
  attributeDependencyPath,
  relationshipOverviewPath
);
const runtimeCompatibilityPath = resolveRepoPath(config.runtimeCompatibility.output);
writeRuntimeCompatibility(config, runtimeCompatibilityPath);
const staticValueProofPath = resolveRepoPath(config.staticValueProof.output);
writeStaticValueProof(config, outputPath, staticValueProofPath);
const currentComponentBridgePath = resolveRepoPath(config.currentComponentBridge.output);
writeCurrentComponentBridge(config, currentComponentBridgePath);
const factorClosurePath = resolveRepoPath(config.factorClosure.output);
writeFactorClosure(config, factorClosurePath);
const primaryStatAttackTransformProofPath = resolveRepoPath(
  config.primaryStatAttackTransformProof.output,
);
writePrimaryStatAttackTransformProof(config, primaryStatAttackTransformProofPath);
const targetMitigationOfflineProofPath = resolveRepoPath(
  config.targetMitigationOfflineProof.output,
);
writeTargetMitigationOfflineProof(config, targetMitigationOfflineProofPath);
const masteryPropertyOfflineProofPath = resolveRepoPath(
  config.masteryPropertyOfflineProof.output,
);
writeMasteryPropertyOfflineProof(config, masteryPropertyOfflineProofPath);
const offensiveReadinessPath = resolveRepoPath(config.offensiveReadiness.output);
writeOffensiveReadiness(
  config,
  outputPath,
  runtimeCompatibilityPath,
  staticValueProofPath,
  offensiveReadinessPath,
);
const remainingTracePath = resolveRepoPath(config.remainingTrace.output);
writeRemainingTrace(
  config,
  outputPath,
  offensiveReadinessPath,
  staticValueProofPath,
  remainingTracePath,
);
const globalOfflineGatePath = resolveRepoPath(config.globalOfflineGate.output);
writeGlobalOfflineGate(config, globalOfflineGatePath);
const matchingBuildValidationPath = resolveRepoPath(config.matchingBuildValidation.output);
const matchingBuildValidationRuntimePath = resolveRepoPath(config.matchingBuildValidation.runtimeOutput);
writeMatchingBuildValidationManifest(
  config,
  factorClosurePath,
  remainingTracePath,
  globalOfflineGatePath,
  targetMitigationOfflineProofPath,
  masteryPropertyOfflineProofPath,
  resolveRepoPath(config.matchingBuildValidation.damageStage),
  matchingBuildValidationPath,
  matchingBuildValidationRuntimePath,
);
auditValidationContract(matchingBuildValidationPath);
const runtimeProofWorklistPath = resolveRepoPath(config.runtimeProofWorklist.output);
writeRuntimeProofWorklist(
  config,
  matchingBuildValidationPath,
  runtimeProofWorklistPath,
);
writeBundleManifest(
  config,
  outputPath,
  summaryPath,
  proofCatalogPath,
  attributeDependencyPath,
  relationshipOverviewPath,
  runtimeCompatibilityPath,
  staticValueProofPath,
  offensiveReadinessPath,
  remainingTracePath,
  currentComponentBridgePath,
  factorClosurePath,
  globalOfflineGatePath,
  matchingBuildValidationPath,
  runtimeProofWorklistPath,
  resolveRepoPath(config.bundleManifestOutput)
);

function refreshDerivedExtractorTables(value) {
  const extractorRoot = resolveRepoPath(value.extractor.root);
  for (const script of value.extractor.derivedScripts) {
    run(process.execPath, [path.join(extractorRoot, script), "--output-dir", value.extractor.outputDirectory], {
      cwd: extractorRoot,
      label: `extractor:${script}`
    });
  }
}

function runStaticWorklist(value) {
  const args = [
    "run", "-p", "rlogs-game-bpsr", "--bin", "rlogs-bpsr-static-rdps-worklist", "--",
    "--classification", resolveRepoPath(value.staticWorklist.classification),
    "--contribution", resolveRepoPath(value.staticWorklist.contribution),
    "--recount", resolveRepoPath(value.staticWorklist.recount),
    "--value-proof", resolveRepoPath(value.staticWorklist.valueProof),
    "--build", String(value.gameBuild),
    "--output", resolveRepoPath(value.staticWorklist.output),
    "--watchlist-output", resolveRepoPath(value.staticWorklist.watchlistOutput),
    "--buff-table", resolveRepoPath(value.staticWorklist.buffTable)
  ];
  run("cargo", args, { cwd: repoRoot, label: "static-rdps-worklist" });
}

function runSemanticAudit(value) {
  const args = [
    "run", "-p", "rlogs-game-bpsr", "--bin", "rlogs-bpsr-static-rdps-semantic-audit", "--",
    "--worklist", resolveRepoPath(value.staticWorklist.output),
    "--effect-sources", resolveRepoPath(value.semanticAudit.effectSources),
    "--build", String(value.gameBuild),
    "--output", resolveRepoPath(value.semanticAudit.output)
  ];
  run("cargo", args, { cwd: repoRoot, label: "static-rdps-semantic-audit" });
}

function runRecipientLedger(value, ledgerOutput) {
  const args = [
    "run", "-p", "rlogs-game-bpsr", "--bin", "rlogs-bpsr-rdps-recipient-scope-ledger", "--",
    "--worklist", resolveRepoPath(value.staticWorklist.output),
    "--watchlist", resolveRepoPath(value.staticWorklist.watchlistOutput),
    "--semantic-audit", resolveRepoPath(value.semanticAudit.output),
    "--display", resolveRepoPath(value.ledger.display)
  ];
  for (const [name, input] of Object.entries(value.ledger.proofInputs)) {
    args.push(`--${name}`, resolveRepoPath(input));
  }
  args.push("--packet-build", String(value.historicalPacketBuild), "--output", ledgerOutput);
  run("cargo", args, { cwd: repoRoot, label: "rdps-recipient-scope-ledger" });
}

function refreshCurrentBuildReachability(value, ledgerPath) {
  const ledger = readJson(ledgerPath, "generated ledger before reachability enrichment");
  const unresolvedEffectIds = distinctNumbers(
    (ledger.candidates || [])
      .filter((candidate) => (candidate.component_scope_routes || []).some(isComponentRouteUnresolved))
      .flatMap((candidate) => candidate.effect_ids || [])
  );
  if (unresolvedEffectIds.length === 0) return;
  const extractorRoot = resolveRepoPath(value.extractor.root);
  run(process.execPath, [
    path.join(extractorRoot, value.reachability.script),
    "--output-dir", value.extractor.outputDirectory,
    "--build", String(value.gameBuild),
    "--ids", unresolvedEffectIds.join(","),
    "--out", resolveRepoPath(value.reachability.output)
  ], {
    cwd: extractorRoot,
    label: "current-build-effect-reachability"
  });
}

function applyCurrentBuildReachability(value, ledgerPath) {
  const ledger = readJson(ledgerPath, "generated ledger before reachability enrichment");
  const reachabilityPath = resolveRepoPath(value.reachability.output);
  const reachability = readJson(reachabilityPath, "current-build effect reachability");
  if (reachability.generatedBy !== "EffectReachability.gen") {
    throw new Error("reachability input was not generated by EffectReachability.gen");
  }
  if (String(reachability.gameBuild) !== String(value.gameBuild)) {
    throw new Error("reachability game build differs from the rDPS batch game build");
  }
  const evidenceByEffect = reachability.effectsById || {};
  let preservedCandidates = 0;
  let preservedRoutes = 0;
  for (const candidate of ledger.candidates || []) {
    const effectEvidence = distinctNumbers(candidate.effect_ids || [])
      .map((effectId) => evidenceByEffect[String(effectId)])
      .filter(Boolean)
      .map((evidence) => ({
        effect_id: evidence.effectId,
        reachability_status: evidence.reachabilityStatus,
        table_definition_paths: (evidence.tableDefinitions || []).map((row) => `${row.file}:${row.jsonPath}`),
        incoming_table_references: evidence.tableIncomingReferences || [],
        client_asset_references: evidence.clientAssetReferences || [],
        semantic_code_references: evidence.semanticCodeTokenReferences || []
      }));
    if (effectEvidence.length === 0) continue;
    const allCandidateEffectsScanned = effectEvidence.length === distinctNumbers(candidate.effect_ids || []).length;
    const definitionOnly = allCandidateEffectsScanned && effectEvidence.every(
      (evidence) => evidence.reachability_status === "definition-only-no-current-incoming-reference"
    );
    candidate.current_build_reachability = {
      aggregate_status: definitionOnly
        ? "definition-only-no-current-incoming-reference"
        : "current-build-reference-present-or-mixed",
      evidence_artifact: relativeRepoPath(reachabilityPath),
      effects: effectEvidence
    };
    if (!definitionOnly) continue;
    const gate = preservedDefinitionOnlyGate();
    let changedRoute = false;
    candidate.component_scope_routes = (candidate.component_scope_routes || []).map((route) => {
      if (!isComponentRouteUnresolved(route)) return route;
      changedRoute = true;
      preservedRoutes += 1;
      return {
        ...route,
        scope_queue: "preserved-static-definition-unreachable-current-build",
        current_build_reachability: "definition-only-no-current-incoming-reference",
        transfer_gate: gate,
        current_build_promotion_eligible: false
      };
    });
    if (!changedRoute) continue;
    preservedCandidates += 1;
    candidate.scope_resolution = "exact-build-definition-preserved-no-runtime-incoming-reference";
    candidate.scope_queue = "preserved-static-definition-unreachable-current-build";
    candidate.transfer_gate = gate;
    candidate.current_build_promotion_eligible = false;
    candidate.remaining_requirement = "rescan exact-build tables, extracted assets, and IL2CPP references after a client update; require a new incoming reference before packet lifecycle and counterfactual promotion";
  }
  ledger.inputs.current_build_effect_reachability = relativeRepoPath(reachabilityPath);
  ledger.policy.definition_only_rows_preserved = true;
  ledger.policy.definition_only_rows_treated_as_runtime_mechanics = false;
  ledger.enrichment = {
    generated_by: "tools/rdps-research-batch.mjs",
    kind: "exact-build-effect-reachability",
    preserved_candidates: preservedCandidates,
    preserved_component_routes: preservedRoutes
  };
  ledger.summary.current_build_definition_only_unreachable_candidates = preservedCandidates;
  ledger.summary.current_build_definition_only_unreachable_component_routes = preservedRoutes;
  ledger.summary.scope_queues = countStrings((ledger.candidates || []).map((candidate) => candidate.scope_queue));
  ledger.summary.transfer_gate_kinds = countStrings(
    (ledger.candidates || []).map((candidate) => candidate.transfer_gate?.kind).filter(Boolean)
  );
  ledger.summary.component_scope_queues = countStrings(
    (ledger.candidates || []).flatMap((candidate) => candidate.component_scope_routes || []).map((route) => route.scope_queue)
  );
  ledger.summary.component_transfer_gate_kinds = countStrings(
    (ledger.candidates || []).flatMap((candidate) => candidate.component_scope_routes || []).map((route) => route.transfer_gate?.kind).filter(Boolean)
  );
  writeFileSync(ledgerPath, `${JSON.stringify(ledger, null, 2)}\n`, "utf8");
}

function preservedDefinitionOnlyGate() {
  return {
    kind: "current-build-definition-unreachable",
    attribution_route: "no runtime route in this exact build; preserve the source, effect, formula, and evidence for build diffs",
    authority: "exact-build decoded tables plus complete extracted-client asset and IL2CPP reference scans",
    runtime_credit_allowed: false,
    required_current_build_evidence: [
      "a semantic incoming table, client-asset, or code reference in a later exact build",
      "matching-build packet lifecycle only after reachability is re-established"
    ],
    forbidden_transfers: [
      "treating a definition-only table row as an active runtime mechanic",
      "deleting or hiding the preserved definition and its formula relationship",
      "credit inferred only from a historical-build packet"
    ]
  };
}

function writeDeltaReport(value, beforePath, afterPath, destination) {
  const before = readJson(beforePath, "baseline ledger");
  const after = readJson(afterPath, "generated ledger");
  const beforeCounts = ledgerCounts(before);
  const afterCounts = ledgerCounts(after);
  const beforeBySource = uniqueCandidateMap(before.candidates, "baseline ledger");
  const afterBySource = uniqueCandidateMap(after.candidates, "generated ledger");
  const newlyClassified = after.candidates
    .filter((row) => isUnresolved(beforeBySource.get(candidateIdentity(row))) && !isUnresolved(row))
    .map(classificationChange);
  const newlyUnresolved = after.candidates
    .filter((row) => {
      const previous = beforeBySource.get(candidateIdentity(row));
      return previous && !isUnresolved(previous) && isUnresolved(row);
    })
    .map(classificationChange);
  const rekeyedRows = after.candidates
    .filter((row) => {
      const previous = beforeBySource.get(candidateIdentity(row));
      return previous && previous.source_rule_id !== row.source_rule_id;
    })
    .map((row) => ({
      source_identity: candidateIdentity(row),
      before: compactCandidate(beforeBySource.get(candidateIdentity(row))),
      after: compactCandidate(row)
    }));
  const changedRows = after.candidates
    .filter((row) => {
      const previous = beforeBySource.get(candidateIdentity(row));
      return previous && candidateEvidenceDigest(previous) !== candidateEvidenceDigest(row);
    })
    .map((row) => ({
      source_identity: candidateIdentity(row),
      before: compactCandidate(beforeBySource.get(candidateIdentity(row))),
      after: compactCandidate(row)
    }));
  const removedSources = before.candidates
    .filter((row) => !afterBySource.has(candidateIdentity(row)))
    .map(compactCandidate);
  const addedSources = after.candidates
    .filter((row) => !beforeBySource.has(candidateIdentity(row)))
    .map(compactCandidate);
  const report = {
    schema_version: 1,
    generated_by: "tools/rdps-research-batch.mjs",
    game_build: String(value.gameBuild),
    historical_packet_build: String(value.historicalPacketBuild),
    policy: {
      unresolved_evidence_hidden: false,
      semantic_resolution_enables_runtime_rdps: false,
      matching_build_packet_proof_required: true,
      purpose: "Batch static classification and proof-queue reduction without promoting unproven rDPS attribution."
    },
    inputs: {
      config: relativeRepoPath(configPath),
      baseline_ledger: artifactReference(beforePath),
      generated_ledger: artifactReference(afterPath)
    },
    before: beforeCounts,
    after: afterCounts,
    delta: {
      unresolved_mechanics: afterCounts.unresolved_mechanics - beforeCounts.unresolved_mechanics,
      unresolved_effect_ids: afterCounts.unresolved_effect_ids - beforeCounts.unresolved_effect_ids,
      resolved_awaiting_current_build_proof:
        afterCounts.resolved_awaiting_current_build_proof - beforeCounts.resolved_awaiting_current_build_proof
    },
    newly_semantically_classified: newlyClassified,
    newly_unresolved_regressions: newlyUnresolved,
    rekeyed_rows: rekeyedRows,
    changed_existing_sources: changedRows,
    removed_sources: removedSources,
    added_sources: addedSources,
    unresolved_remaining: after.candidates
      .filter(isUnresolved)
      .map((row) => ({
        source_rule_id: row.source_rule_id,
        source_id: row.source_id,
        source_name: row.source_name,
        effect_ids: row.effect_ids,
        remaining_requirement: row.remaining_requirement
      }))
  };
  writeFileSync(destination, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  console.log(JSON.stringify({
    before: beforeCounts,
    after: afterCounts,
    delta: report.delta,
    source_changes: {
      newly_semantically_classified: newlyClassified.length,
      newly_unresolved_regressions: newlyUnresolved.length,
      rekeyed_rows: rekeyedRows.length,
      changed_existing_sources: changedRows.length,
      removed_sources: removedSources.length,
      added_sources: addedSources.length
    }
  }, null, 2));
  console.log(`Wrote ${relativeRepoPath(destination)}`);
}

function writeProofCatalog(value, ledgerPath, destination) {
  const ledger = readJson(ledgerPath, "generated ledger");
  const worklist = readJson(resolveRepoPath(value.staticWorklist.output), "static rDPS worklist");
  const semanticAudit = readJson(resolveRepoPath(value.semanticAudit.output), "static semantic audit");
  const effectSources = readJson(resolveRepoPath(value.semanticAudit.effectSources), "current-build effect sources");
  const worklistByRule = uniqueRuleMap([
    ...(worklist.exact_produced_damage_candidates || []),
    ...(worklist.formula_replay_candidates || [])
  ], "static rDPS worklist");
  const semanticByRule = uniqueRuleMap(semanticAudit.findings || [], "static semantic audit");
  const effectIndex = new Map();
  const sources = ledger.candidates.map((candidate) => {
    const work = worklistByRule.get(candidate.source_rule_id) || {};
    const semantic = semanticByRule.get(candidate.source_rule_id) || {};
    const unresolved = isUnresolved(candidate);
    const componentRoutes = candidate.component_scope_routes || [];
    const partialComponentResolution = hasPartialComponentResolution(candidate);
    const definitionOnlyUnreachable = candidate.current_build_reachability?.aggregate_status
      === "definition-only-no-current-incoming-reference";
    const proofState = definitionOnlyUnreachable
      ? "preserved-current-build-definition-unreachable"
      : candidate.current_build_promotion_eligible
      ? "matching-build-runtime-proven"
      : partialComponentResolution
        ? "component-scoped-partially-resolved-awaiting-matching-build-runtime-proof"
      : unresolved
        ? "unresolved-visible-blocked"
        : "semantically-resolved-awaiting-matching-build-runtime-proof";
    const selectedValues = distinctObjects(
      (work.value_proofs || []).flatMap((proof) => proof.selected_values || [])
    );
    const relationshipComponents = work.relationship_components || [];
    const knownMagnitudes = distinctObjects(
      relationshipComponents.flatMap((component) => (component.values || []).map((magnitude) => ({
        component_key: component.componentKey,
        effect_class: component.effectClass,
        stat: component.stat,
        formula_replay_status: component.formulaReplayStatus,
        proof_binding: component.proofBinding,
        ...magnitude
      })))
    );
    const requiredEvidence = distinctStrings([
      ...(work.required_runtime_evidence || []),
      ...(candidate.transfer_gate?.required_current_build_evidence || []),
      ...componentRoutes.flatMap((route) => [
        ...(route.required_runtime_evidence || []),
        ...(route.transfer_gate?.required_current_build_evidence || [])
      ]),
      ...(work.value_proofs || []).flatMap((proof) => proof.proof_requirements || [])
    ]);
    const blockers = distinctStrings([
      ...(work.static_blockers || []),
      ...(work.value_proofs || []).flatMap((proof) => [
        ...(proof.value_blockers || []),
        ...(proof.blockers || [])
      ]),
      ...(semantic.issues || []).filter((issue) => issue.promotion_blocker).map((issue) => issue.evidence),
      ...(!candidate.current_build_promotion_eligible ? [candidate.remaining_requirement] : [])
    ]);
    const sourceIdentity = candidateIdentity(candidate);
    const rawEffectSource = effectSources.effectSourcesById?.[candidate.source_id] || {};
    const nonOwningCandidateEffectIds = distinctNumbers(rawEffectSource.nonOwningCandidateBuffIds || []);
    const linkedTooltipDependencies = (rawEffectSource.linkTextTooltips || []).map((tooltip) => ({
      link_text_id: tooltip.linkTextId,
      name: tooltip.name,
      relationship: tooltip.relationship || "referenced-mechanic-unresolved",
      direct_reference_clauses: tooltip.directReferenceClauses || [],
      current_build_runtime_authority: false,
      attributed_damage_delta: null
    }));
    const declared = new Set((candidate.declared_effect_ids || []).map(String));
    const runtimeRelated = new Set((candidate.runtime_related_effect_ids || []).map(String));
    for (const effectId of candidate.effect_ids || []) {
      const key = String(effectId);
      const row = effectIndex.get(key) || { effect_id: effectId, sources: [] };
      row.sources.push({
        source_identity: sourceIdentity,
        source_rule_id: candidate.source_rule_id,
        source_name: candidate.source_name,
        relationship: declared.has(key)
          ? "declared-source-effect"
          : runtimeRelated.has(key)
            ? "runtime-related-child-effect"
            : "catalogued-effect"
      });
      effectIndex.set(key, row);
    }
    for (const effectId of nonOwningCandidateEffectIds) {
      const key = String(effectId);
      const row = effectIndex.get(key) || { effect_id: effectId, sources: [] };
      row.sources.push({
        source_identity: sourceIdentity,
        source_rule_id: candidate.source_rule_id,
        source_name: candidate.source_name,
        relationship: "non-owning-candidate-reference",
        current_build_runtime_authority: false
      });
      effectIndex.set(key, row);
    }
    return {
      source_identity: sourceIdentity,
      source_rule_id: candidate.source_rule_id,
      source: {
        id: candidate.source_id,
        type: sourceType(candidate.source_id),
        name: candidate.source_name,
        description: candidate.description
      },
      mechanic: {
        primary_role: candidate.primary_role,
        report_domains: candidate.report_domains,
        contribution_mode: candidate.contribution_mode,
        contribution_tier: work.contribution_tier,
        confidence: work.confidence,
        formula_term_ids: candidate.formula_term_ids,
        formula_zone_ids: candidate.formula_zone_ids,
        contribution_groups: work.contribution_groups || [],
        predicate_tags: work.predicate_tags || [],
        selected_values: selectedValues,
        relationship_components: relationshipComponents
      },
      relationships: {
        declared_effect_ids: candidate.declared_effect_ids,
        runtime_related_effect_ids: candidate.runtime_related_effect_ids,
        all_effect_ids: candidate.effect_ids,
        non_owning_candidate_effect_ids: nonOwningCandidateEffectIds,
        linked_tooltip_dependencies: linkedTooltipDependencies,
        target_damage_ids: work.runtime_matcher?.target_damage_ids || [],
        target_recount_ids: work.runtime_matcher?.target_recount_ids || [],
        runtime_matcher: work.runtime_matcher,
        runtime_effect_family_evidence: candidate.runtime_effect_family_evidence,
        current_component_evidence: candidate.current_component_evidence,
        current_build_reachability: candidate.current_build_reachability || null
      },
      provider_recipient_scope: {
        declared: candidate.transfer_eligibilities,
        effective: candidate.effective_transfer_eligibilities,
        resolution: candidate.scope_resolution,
        queue: candidate.scope_queue,
        component_routes: componentRoutes
      },
      proof: {
        state: proofState,
        current_build_runtime_promoted: candidate.current_build_promotion_eligible,
        current_build_reachability: candidate.current_build_reachability || null,
        historical_packet_evidence: candidate.historical_packet_evidence,
        semantic_issues: semantic.issues || [],
        required_evidence: requiredEvidence,
        blockers,
        rejected_interpretations: distinctStrings([
          ...(candidate.transfer_gate?.forbidden_transfers || []),
          ...componentRoutes.flatMap((route) => route.transfer_gate?.forbidden_transfers || [])
        ]),
        transfer_gate: candidate.transfer_gate
      },
      attribution_readiness: {
        catalog_state: attributionCatalogState(candidate, work, blockers),
        affected_damage_ids: work.runtime_matcher?.target_damage_ids || [],
        affected_recount_ids: work.runtime_matcher?.target_recount_ids || [],
        static_magnitudes: knownMagnitudes,
        candidate_formula_inputs: selectedValues,
        attributed_amount_state: candidate.current_build_promotion_eligible === true
          ? "matching-build-counterfactual-ready"
          : "not-computable-with-current-proof",
        safe_to_attribute: candidate.current_build_promotion_eligible === true,
        note: "Every relationship remains visible. Per-damage-ID contribution is blocked until its formula, provider/recipient window, and matching-build runtime inputs are proven."
      }
    };
  });

  const catalog = {
    schema_version: 6,
    generated_by: "tools/rdps-research-batch.mjs",
    static_game_build: ledger.static_game_build,
    historical_packet_build: ledger.historical_packet_build,
    policy: {
      unresolved_evidence_hidden: false,
      uncertain_relationships_removed: false,
      matching_build_packet_proof_required_for_simulation: true,
      purpose: "Source/effect/formula/damage-ID relationship catalog for rDPS attribution and a future damage-attribution overview. Build planning is only a possible downstream consumer."
    },
    inputs: ledger.inputs,
    summary: {
      sources: sources.length,
      effects: effectIndex.size,
      unresolved_sources: sources.filter((row) => row.proof.state === "unresolved-visible-blocked").length,
      preserved_definition_unreachable_current_build: sources.filter(
        (row) => row.proof.state === "preserved-current-build-definition-unreachable"
      ).length,
      partially_resolved_component_scoped_sources: sources.filter(
        (row) => row.proof.state === "component-scoped-partially-resolved-awaiting-matching-build-runtime-proof"
      ).length,
      component_scope_routes: sources.reduce(
        (total, row) => total + (row.provider_recipient_scope?.component_routes?.length || 0),
        0
      ),
      attribution_ready_sources: sources.filter((row) => row.attribution_readiness.safe_to_attribute).length
    },
    sources,
    effects: [...effectIndex.values()].sort((left, right) => Number(left.effect_id) - Number(right.effect_id))
  };
  writeFileSync(destination, `${JSON.stringify(catalog, null, 2)}\n`, "utf8");
  console.log(`Wrote ${relativeRepoPath(destination)}`);
}

function writeAttributeTransformDependencyLedger(value, destination) {
  const historicalPath = resolveRepoPath(value.relationshipEvidence.attributeFamilyFormulaProof);
  const currentSurfacePath = resolveRepoPath(value.relationshipEvidence.fightAttributeTransformSurface);
  const historical = readJson(historicalPath, "historical attribute-family formula proof");
  const currentSurface = readJson(currentSurfacePath, "current-build FightAttrTran surface");
  if (String(currentSurface.game_build) !== String(value.gameBuild)) {
    throw new Error(
      `FightAttrTran build ${currentSurface.game_build} does not match configured game build ${value.gameBuild}`
    );
  }

  const familyNames = new Map([
    [11010, "Strength"],
    [11020, "Intellect"],
    [11030, "Agility"],
    [11040, "Endurance"],
    [11110, "Crit points"],
    [11120, "Haste points"],
    [11130, "Luck points"],
    [11140, "Mastery points"],
    [11150, "Versatility points"],
    [11330, "Attack"],
    [11340, "Magic Attack"],
    [11710, "Crit"],
    [11780, "Lucky Strike probability"],
    [11930, "Haste percent"],
    [11940, "Mastery percent"],
    [11950, "Versatility percent"]
  ]);
  const exactHistoricalChecks = [];
  const equalityInvariants = [];
  const familyNodes = (historical.families || []).map((family) => {
    for (const check of family.formula_checks || []) {
      if (check.evaluable_snapshots > 0 && check.mismatches === 0) {
        const row = {
          packet_build: String(value.historicalPacketBuild),
          base_attribute_id: family.base_attribute_id,
          family_name: familyNames.get(family.base_attribute_id) || null,
          proof_kind: "absolute-packet-invariant",
          expression: check.expression,
          scale: check.scale,
          evaluable_observations: check.evaluable_snapshots,
          exact_matches: check.exact_matches,
          within_one_packet_unit: 0,
          mismatches_beyond_one_packet_unit: 0,
          residual_min: check.residual_min,
          residual_max: check.residual_max,
          current_build_runtime_authority: false
        };
        exactHistoricalChecks.push(row);
        equalityInvariants.push(row);
      }
    }
    for (const check of family.transition_formula_checks || []) {
      if (check.evaluable_transitions > 0 && check.mismatches_beyond_one_packet_unit === 0) {
        exactHistoricalChecks.push({
          packet_build: String(value.historicalPacketBuild),
          base_attribute_id: family.base_attribute_id,
          family_name: familyNames.get(family.base_attribute_id) || null,
          proof_kind: "packet-transition-equation",
          stage: check.stage,
          expression: check.expression,
          scale: check.scale,
          evaluable_observations: check.evaluable_transitions,
          exact_matches: check.exact_matches,
          within_one_packet_unit: check.within_one_packet_unit,
          mismatches_beyond_one_packet_unit: check.mismatches_beyond_one_packet_unit,
          residual_min: check.residual_min,
          residual_max: check.residual_max,
          actors: check.actors,
          current_build_runtime_authority: false
        });
      }
    }
    return {
      base_attribute_id: family.base_attribute_id,
      family_name: familyNames.get(family.base_attribute_id) || null,
      members: family.members,
      attribute_events: family.attribute_events,
      actors: family.actors,
      actor_runs: family.actor_runs,
      complete_packet_batches: family.complete_packet_batches,
      incomplete_packet_batches: family.incomplete_packet_batches,
      matching_build_packet_observations: 0,
      current_build_runtime_authority: false
    };
  });

  const currentTransformCandidates = [];
  const transformFields = [
    ["CriToCrit", 11110, 11710],
    ["HasteToHastePct", 11120, 11930],
    ["LuckToLuckyStrikeProb", 11130, 11780],
    ["MasteryToMasteryPct", 11140, 11940],
    ["VersatilityToVersatilityPct", 11150, 11950],
    ["PhyPowerToDam", 11330, null],
    ["MagPowerToDam", 11340, null]
  ];
  for (const [rowId, row] of Object.entries(currentSurface.rows || {})) {
    for (const [field, inputFamilyId, outputFamilyId] of transformFields) {
      currentTransformCandidates.push({
        static_game_build: String(value.gameBuild),
        transform_row_id: Number(rowId),
        field,
        input_family_id: inputFamilyId,
        output_family_id: outputFamilyId,
        decoded_parameters: row[field],
        relationship: outputFamilyId === null ? "attribute-to-damage-stage" : "attribute-family-to-derived-family",
        exact_row_selection_proven: false,
        curve_semantics_proven: false,
        rounding_proven_for_current_build: false,
        current_build_runtime_authority: false
      });
    }
  }

  const exactTransitionChecks = exactHistoricalChecks.filter(
    (row) => row.proof_kind === "packet-transition-equation"
  );
  const ledger = {
    schema_version: 1,
    generated_by: "tools/rdps-research-batch.mjs",
    static_game_build: String(value.gameBuild),
    packet_evidence_build: String(value.historicalPacketBuild),
    policy: {
      unresolved_evidence_hidden: false,
      historical_equations_are_current_runtime_authority: false,
      current_static_transform_parameters_are_formula_authority: false,
      matching_build_packet_replay_required: true,
      amount_default_when_unproven: null,
      purpose: "Preserve packet-observed stat-family equations and current static transform dependencies for rDPS counterfactual replay and damage-attribution relationships."
    },
    inputs: {
      historical_attribute_family_formula_proof: artifactReference(historicalPath),
      current_fight_attribute_transform_surface: artifactReference(currentSurfacePath)
    },
    summary: {
      family_nodes: familyNodes.length,
      exact_historical_invariants: equalityInvariants.length,
      exact_historical_transition_equations: exactTransitionChecks.length,
      current_static_transform_candidates: currentTransformCandidates.length,
      matching_build_runtime_equations: 0
    },
    family_nodes: familyNodes,
    exact_historical_checks: exactHistoricalChecks,
    current_static_transform_candidates: currentTransformCandidates
  };
  writeFileSync(destination, `${JSON.stringify(ledger, null, 2)}\n`, "utf8");
  console.log(`Wrote ${relativeRepoPath(destination)}`);
}

function writeDamageAttributionRelationshipOverview(
  value,
  proofCatalogPath,
  attributeDependencyPath,
  destination
) {
  const catalog = readJson(proofCatalogPath, "mechanic proof catalog");
  const attributeDependencies = readJson(attributeDependencyPath, "attribute transform dependency ledger");
  const sourceNodes = [];
  const sourceEffectEdges = [];
  const sourceDamageEdges = [];
  const sourceRecountEdges = [];
  const sourceAttributeEdges = [];
  const sourceMechanicDependencyEdges = [];
  const sourceComponentEdges = [];
  const statFamilyCandidates = new Map([
    ["strength", [11010]],
    ["intellect", [11020]],
    ["agility", [11030]],
    ["endurance", [11040]],
    ["atk", [11330]],
    ["matk", [11340]],
    ["crit", [11110, 11710]],
    ["haste", [11120, 11930]],
    ["luck", [11130, 11780]],
    ["mastery", [11140, 11940]],
    ["versatility", [11150, 11950]]
  ]);

  for (const row of catalog.sources || []) {
    const relationship = row.relationships || {};
    const readiness = row.attribution_readiness || {};
    const effectIds = relationship.all_effect_ids || [];
    const nonOwningCandidateEffectIds = relationship.non_owning_candidate_effect_ids || [];
    const linkedTooltipDependencies = relationship.linked_tooltip_dependencies || [];
    const damageIds = readiness.affected_damage_ids || [];
    const recountIds = readiness.affected_recount_ids || [];
    const components = (readiness.static_magnitudes || []).map((component) => ({
      component_key: component.component_key,
      effect_class: component.effect_class,
      stat: component.stat,
      value: component.value,
      unit: component.unit,
      formula_replay_status: component.formula_replay_status,
      proof_binding: component.proof_binding
    }));
    const componentRoutes = row.provider_recipient_scope?.component_routes || [];
      sourceNodes.push({
      source_identity: row.source_identity,
      source_rule_id: row.source_rule_id,
      source: row.source,
      proof_state: row.proof?.state,
      safe_to_attribute: readiness.safe_to_attribute === true,
      relationship_state: damageIds.length > 0
        ? "damage-targets-catalogued"
        : "damage-target-relationship-unresolved-visible",
      provider_recipient_scope: row.provider_recipient_scope,
      current_build_reachability: row.relationships?.current_build_reachability || null,
      formula_term_ids: row.mechanic?.formula_term_ids || [],
      formula_zone_ids: row.mechanic?.formula_zone_ids || [],
      components
    });

    componentRoutes.forEach((route, componentRouteIndex) => {
      sourceComponentEdges.push({
        source_identity: row.source_identity,
        component_route_index: componentRouteIndex,
        component_key: route.component_key,
        effect_class: route.effect_class,
        direction: route.direction,
        contribution_scope: route.contribution_scope,
        contribution_groups: route.contribution_groups || [],
        formula_term_ids: route.formula_term_ids || [],
        transfer_eligibility: route.transfer_eligibility,
        scope_queue: route.scope_queue,
        rdps_relevance: route.rdps_relevance,
        value_resolution: route.value_resolution,
        required_runtime_evidence: route.required_runtime_evidence || [],
        transfer_gate: route.transfer_gate,
        affected_damage_ids: damageIds,
        current_build_runtime_authority: route.current_build_promotion_eligible === true,
        attributed_damage_delta: null,
        proof_state: row.proof?.state
      });
    });

    for (const component of components) {
      const candidateFamilyIds = statFamilyCandidates.get(String(component.stat || "").toLowerCase()) || [];
      for (const familyId of candidateFamilyIds) {
        sourceAttributeEdges.push({
          source_identity: row.source_identity,
          component_key: component.component_key,
          stat: component.stat,
          value: component.value,
          unit: component.unit,
          candidate_attribute_family_id: familyId,
          binding_state: candidateFamilyIds.length === 1
            ? "semantic-family-candidate-awaiting-matching-build-packet-binding"
            : "ambiguous-points-or-derived-family-awaiting-matching-build-packet-binding",
          current_build_runtime_authority: false,
          attributed_damage_delta: null
        });
      }
    }

    for (const effectId of effectIds) {
      const effectKey = String(effectId);
      sourceEffectEdges.push({
        source_identity: row.source_identity,
        effect_id: effectId,
        relationship: (relationship.declared_effect_ids || []).map(String).includes(effectKey)
          ? "declared-source-effect"
          : (relationship.runtime_related_effect_ids || []).map(String).includes(effectKey)
            ? "runtime-related-child-effect"
            : "catalogued-effect",
        proof_state: row.proof?.state,
        current_build_reachability: row.relationships?.current_build_reachability || null
      });
    }
    for (const effectId of nonOwningCandidateEffectIds) {
      sourceEffectEdges.push({
        source_identity: row.source_identity,
        effect_id: effectId,
        relationship: "non-owning-candidate-reference",
        current_build_runtime_authority: false,
        attributed_damage_delta: null,
        proof_state: row.proof?.state
      });
    }
    for (const dependency of linkedTooltipDependencies) {
      sourceMechanicDependencyEdges.push({
        source_identity: row.source_identity,
        dependency_kind: "linked-tooltip",
        dependency_identity: `linktext:${dependency.link_text_id}`,
        link_text_id: dependency.link_text_id,
        name: dependency.name,
        relationship: dependency.relationship,
        direct_reference_clauses: dependency.direct_reference_clauses || [],
        current_build_runtime_authority: false,
        attributed_damage_delta: null,
        proof_state: row.proof?.state
      });
    }
    for (const damageId of damageIds) {
      sourceDamageEdges.push({
        source_identity: row.source_identity,
        affected_damage_id: damageId,
        via_effect_ids: effectIds,
        recount_parent_ids: recountIds,
        component_keys: components.map((component) => component.component_key),
        binding_state: readiness.safe_to_attribute === true
          ? "matching-build-counterfactual-ready"
          : "catalogued-not-yet-computable",
        amount: {
          state: readiness.attributed_amount_state,
          observed_damage: null,
          counterfactual_damage_without_source: null,
          attributed_damage_delta: null,
          unit: "damage"
        },
        proof_state: row.proof?.state,
        blockers: row.proof?.blockers || []
      });
    }
    for (const recountId of recountIds) {
      sourceRecountEdges.push({
        source_identity: row.source_identity,
        affected_recount_id: recountId,
        via_effect_ids: effectIds,
        proof_state: row.proof?.state
      });
    }
  }

  const overview = {
    schema_version: 4,
    generated_by: "tools/rdps-research-batch.mjs",
    static_game_build: catalog.static_game_build,
    historical_packet_build: catalog.historical_packet_build,
    purpose: "Queryable combat-influence graph shared by rDPS and the forensic overview of which source changed which damage ID, for which recipient, and by how much.",
    policy: {
      unresolved_evidence_hidden: false,
      missing_amounts_default_to_zero: false,
      static_relationships_are_not_runtime_amounts: true,
      matching_build_counterfactual_required_for_attributed_damage: true,
      build_planning_is_downstream_only: true,
      primary_consumers: ["rdps-attribution", "combat-influence-overview"],
      optional_downstream_consumers: ["build-planner"]
    },
    amount_contract: {
      equation: "attributed_damage_delta = observed_damage - counterfactual_damage_without_source",
      required_runtime_keys: [
        "run_id",
        "event_id",
        "timestamp",
        "provider_character_uid",
        "recipient_character_uid",
        "source_identity",
        "effect_id",
        "affected_damage_id",
        "formula_stage",
        "formula_inputs_with_source",
        "formula_inputs_without_source",
        "observed_damage",
        "counterfactual_damage_without_source",
        "attributed_damage_delta",
        "proof_references"
      ],
      aggregation_keys: [
        "run_id",
        "segment_id",
        "provider_character_uid",
        "recipient_character_uid",
        "target_entity_id",
        "source_identity",
        "effect_id",
        "affected_damage_id",
        "formula_stage"
      ],
      exact_record_shape: {
        run_id: "string",
        segment_id: "string",
        event_id: "canonical timeline sequence",
        timestamp: "observed monotonic timestamp",
        game_build: "exact client build",
        provider_character_uid: "public character UID or null when unresolved",
        recipient_character_uid: "public character UID receiving the modifier",
        target_entity_id: "entity receiving the damage",
        source_identity: "stable mechanic/source rule identity",
        effect_id: "packet-observed status/effect ID",
        effect_instance_id: "packet-observed instance ID or null",
        effect_window: "exact apply/refresh/stack/consume/remove interval",
        affected_damage_id: "packet-observed damage or ability ID",
        recount_parent_id: "reviewed parent ID or null",
        formula_stage: "reviewed stage in the exact damage formula",
        formula_inputs_with_source: "fixed-point inputs used for observed replay",
        formula_inputs_without_source: "same inputs with only this provider contribution removed",
        observed_damage: "packet-observed final damage",
        counterfactual_damage_without_source: "exact integer or exact rational result",
        attributed_damage_delta: "observed minus provider-removed counterfactual",
        proof_references: "build-stamped packet and static evidence references"
      },
      storage_policy: {
        canonical_events_remain_authoritative: true,
        hot_path_retains_every_per_hit_trace: false,
        hot_path_aggregation: "bounded by distinct run/segment/provider/recipient/target/source/effect/damage/stage tuples",
        drilldown_strategy: "materialize per-hit influence records on demand from the canonical timeline and build-stamped formula pack",
        unproven_record_behavior: "preserve relationship and blockers with null amount"
      }
    },
    influence_view_contract: {
      primary_question: "What affected this damage ID, for whom, during which interval, and by exactly how much?",
      required_drilldowns: [
        "damage ID -> contributing source/effect rows",
        "source/effect -> affected damage IDs",
        "provider -> recipients and attributed deltas",
        "recipient -> providers and received deltas",
        "effect window -> included canonical damage events",
        "recount parent -> child damage IDs without hiding child rows"
      ],
      unresolved_display: "show stable IDs, relationship candidates, and explicit blockers; never synthesize an amount"
    },
    summary: {
      sources: sourceNodes.length,
      source_effect_edges: sourceEffectEdges.length,
      source_non_owning_effect_edges: sourceEffectEdges.filter((row) => row.relationship === "non-owning-candidate-reference").length,
      source_mechanic_dependency_edges: sourceMechanicDependencyEdges.length,
      source_component_edges: sourceComponentEdges.length,
      source_attribute_edges: sourceAttributeEdges.length,
      source_damage_edges: sourceDamageEdges.length,
      source_recount_edges: sourceRecountEdges.length,
      sources_with_catalogued_damage_targets: sourceNodes.filter((row) => row.relationship_state === "damage-targets-catalogued").length,
      sources_with_unresolved_damage_targets: sourceNodes.filter((row) => row.relationship_state === "damage-target-relationship-unresolved-visible").length,
      matching_build_counterfactual_ready_edges: sourceDamageEdges.filter((row) => row.binding_state === "matching-build-counterfactual-ready").length
    },
    attribute_transform_dependencies: attributeDependencies,
    sources: sourceNodes,
    source_effect_edges: sourceEffectEdges,
    source_mechanic_dependency_edges: sourceMechanicDependencyEdges,
    source_component_edges: sourceComponentEdges,
    source_attribute_edges: sourceAttributeEdges,
    source_damage_edges: sourceDamageEdges,
    source_recount_edges: sourceRecountEdges
  };
  validateDamageAttributionRelationshipOverview(overview);
  writeFileSync(destination, `${JSON.stringify(overview, null, 2)}\n`, "utf8");
  console.log(`Wrote ${relativeRepoPath(destination)}`);
}

function validateDamageAttributionRelationshipOverview(overview) {
  const sourceIds = new Set();
  for (const source of overview.sources) {
    if (!source.source_identity || sourceIds.has(source.source_identity)) {
      throw new Error(`combat-influence graph has a missing or duplicate source identity: ${source.source_identity}`);
    }
    sourceIds.add(source.source_identity);
  }

  const familyIds = new Set(
    (overview.attribute_transform_dependencies.family_nodes || []).map((row) => row.base_attribute_id)
  );
  for (const edge of overview.source_attribute_edges) {
    if (!sourceIds.has(edge.source_identity) || !familyIds.has(edge.candidate_attribute_family_id)) {
      throw new Error(`combat-influence attribute edge is not joined to a known source and family: ${JSON.stringify(edge)}`);
    }
    if (edge.current_build_runtime_authority !== false || edge.attributed_damage_delta !== null) {
      throw new Error(`unproven combat-influence attribute edge contains runtime authority or an invented amount: ${JSON.stringify(edge)}`);
    }
  }

  for (const edge of overview.source_effect_edges) {
    if (!sourceIds.has(edge.source_identity) || !Number.isSafeInteger(edge.effect_id) || edge.effect_id <= 0) {
      throw new Error(`combat-influence effect edge is invalid: ${JSON.stringify(edge)}`);
    }
    if (edge.relationship === "non-owning-candidate-reference"
      && (edge.current_build_runtime_authority !== false || edge.attributed_damage_delta !== null)) {
      throw new Error(`non-owning combat-influence effect edge gained authority or an invented amount: ${JSON.stringify(edge)}`);
    }
  }
  for (const edge of overview.source_mechanic_dependency_edges) {
    if (!sourceIds.has(edge.source_identity) || !edge.dependency_identity) {
      throw new Error(`combat-influence mechanic dependency edge is invalid: ${JSON.stringify(edge)}`);
    }
    if (edge.current_build_runtime_authority !== false || edge.attributed_damage_delta !== null) {
      throw new Error(`unproven combat-influence mechanic dependency edge gained authority or an invented amount: ${JSON.stringify(edge)}`);
    }
  }
  for (const edge of overview.source_component_edges) {
    if (!sourceIds.has(edge.source_identity) || !edge.component_key || !edge.scope_queue) {
      throw new Error(`combat-influence component edge is invalid: ${JSON.stringify(edge)}`);
    }
    if (edge.current_build_runtime_authority !== false || edge.attributed_damage_delta !== null) {
      throw new Error(`unproven combat-influence component edge gained authority or an invented amount: ${JSON.stringify(edge)}`);
    }
  }
  for (const edge of overview.source_damage_edges) {
    if (!sourceIds.has(edge.source_identity) || !Number.isSafeInteger(edge.affected_damage_id) || edge.affected_damage_id <= 0) {
      throw new Error(`combat-influence damage edge is invalid: ${JSON.stringify(edge)}`);
    }
    const amount = edge.amount || {};
    if (edge.binding_state !== "matching-build-counterfactual-ready"
      && (amount.observed_damage !== null
        || amount.counterfactual_damage_without_source !== null
        || amount.attributed_damage_delta !== null)) {
      throw new Error(`unproven combat-influence damage edge contains an invented amount: ${JSON.stringify(edge)}`);
    }
  }
  for (const edge of overview.source_recount_edges) {
    if (!sourceIds.has(edge.source_identity) || !Number.isSafeInteger(edge.affected_recount_id) || edge.affected_recount_id <= 0) {
      throw new Error(`combat-influence recount edge is invalid: ${JSON.stringify(edge)}`);
    }
  }

  const summary = overview.summary;
  const expectedCounts = {
    sources: overview.sources.length,
    source_effect_edges: overview.source_effect_edges.length,
    source_non_owning_effect_edges: overview.source_effect_edges.filter(
      (row) => row.relationship === "non-owning-candidate-reference"
    ).length,
    source_mechanic_dependency_edges: overview.source_mechanic_dependency_edges.length,
    source_component_edges: overview.source_component_edges.length,
    source_attribute_edges: overview.source_attribute_edges.length,
    source_damage_edges: overview.source_damage_edges.length,
    source_recount_edges: overview.source_recount_edges.length
  };
  for (const [key, expected] of Object.entries(expectedCounts)) {
    if (summary[key] !== expected) {
      throw new Error(`combat-influence summary ${key}=${summary[key]} does not match generated count ${expected}`);
    }
  }
}

function writeBundleManifest(
  value,
  ledgerPath,
  summaryReportPath,
  proofCatalogPath,
  attributeDependencyPath,
  relationshipOverviewPath,
  runtimeCompatibilityPath,
  staticValueProofPath,
  offensiveReadinessPath,
  remainingTracePath,
  currentComponentBridgePath,
  factorClosurePath,
  globalOfflineGatePath,
  matchingBuildValidationPath,
  runtimeProofWorklistPath,
  destination
) {
  const artifacts = [
    [configPath, "pipeline-config"],
    [resolveRepoPath(value.staticWorklist.classification), "modifier-classification"],
    [resolveRepoPath(value.staticWorklist.contribution), "modifier-contribution"],
    [resolveRepoPath(value.staticWorklist.recount), "modifier-recount"],
    [resolveRepoPath(value.staticWorklist.valueProof), "modifier-value-proof"],
    [resolveRepoPath(value.staticWorklist.buffTable), "current-build-buff-table"],
    [resolveRepoPath(value.semanticAudit.effectSources), "current-build-effect-sources"],
    [resolveRepoPath(value.reachability.output), "current-build-effect-reachability"],
    [resolveRepoPath(value.staticWorklist.output), "static-rdps-worklist"],
    [resolveRepoPath(value.staticWorklist.watchlistOutput), "magnitude-proof-watchlist"],
    [resolveRepoPath(value.semanticAudit.output), "semantic-audit"],
    [ledgerPath, "recipient-scope-ledger"],
    [summaryReportPath, "batch-delta-report"],
    [proofCatalogPath, "shared-proof-catalog"],
    [attributeDependencyPath, "attribute-transform-dependency-ledger"],
    [relationshipOverviewPath, "damage-attribution-relationship-overview"],
    [runtimeCompatibilityPath, "runtime-build-compatibility"],
    [staticValueProofPath, "current-build-static-value-proof"],
    [offensiveReadinessPath, "offensive-rdps-readiness"],
    [remainingTracePath, "remaining-rdps-trace-and-final-validation-watchlist"],
    [currentComponentBridgePath, "current-build-typed-component-relationship-bridge"],
    [factorClosurePath, "current-build-psychoscope-factor-offline-closure"],
    [resolveRepoPath(value.primaryStatAttackTransformProof.output), "current-build-primary-stat-attack-transform-proof"],
    [resolveRepoPath(value.targetMitigationOfflineProof.output), "target-mitigation-offline-exhaustion-proof"],
    [resolveRepoPath(value.masteryPropertyOfflineProof.output), "mastery-property-offline-exhaustion-proof"],
    [globalOfflineGatePath, "global-offline-capture-gate"],
    [matchingBuildValidationPath, "matching-build-validation-manifest"],
    [runtimeProofWorklistPath, "runtime-proof-worklist"],
    ...Object.entries(value.ledger.proofInputs).map(([name, input]) => [resolveRepoPath(input), `proof-input:${name}`])
  ];
  const manifest = {
    schema_version: 1,
    generated_by: "tools/rdps-research-batch.mjs",
    game_build: String(value.gameBuild),
    historical_packet_build: String(value.historicalPacketBuild),
    policy: {
      contains_account_credentials: false,
      unresolved_evidence_preserved: true,
      purpose: "Version-stamped minimal research inputs and outputs needed to reproduce the rDPS proof queue."
    },
    artifacts: artifacts.map(([filePath, role]) => ({ role, ...artifactReference(filePath) }))
  };
  writeFileSync(destination, `${JSON.stringify(manifest, null, 2)}\n`, "utf8");
  console.log(`Wrote ${relativeRepoPath(destination)}`);
}

function writeMatchingBuildValidationManifest(
  value,
  factorClosurePath,
  remainingTracePath,
  globalOfflineGatePath,
  targetMitigationPath,
  masteryPath,
  damageStagePath,
  destination,
  runtimeDestination,
) {
  run(process.execPath, [
    path.join(repoRoot, "tools", "rdps-matching-build-validation-manifest.mjs"),
    "--factorClosure", factorClosurePath,
    "--remainingTrace", remainingTracePath,
    "--globalGate", globalOfflineGatePath,
    "--targetMitigation", targetMitigationPath,
    "--mastery", masteryPath,
    "--damageStage", damageStagePath,
    "--reportSchema", String(value.runtimeProofWorklist.reportSchema),
    "--output", destination,
    "--runtimeOutput", runtimeDestination,
  ], { cwd: repoRoot, label: "rdps-matching-build-validation-manifest" });
}

function writeRuntimeProofWorklist(value, manifestPath, destination) {
  run(process.execPath, [
    path.join(repoRoot, "tools", "rdps-runtime-proof-worklist.mjs"),
    "--build", String(value.gameBuild),
    "--manifest", manifestPath,
    "--reportSchema", String(value.runtimeProofWorklist.reportSchema),
    "--output", destination,
  ], { cwd: repoRoot, label: "rdps-runtime-proof-worklist" });
}

function auditValidationContract(manifestPath) {
  run(process.execPath, [
    path.join(repoRoot, "tools", "rdps-validation-contract-audit.mjs"),
    "--manifest", manifestPath,
  ], { cwd: repoRoot, label: "rdps-validation-contract-audit" });
}

function writeRuntimeCompatibility(value, destination) {
  run(process.execPath, [
    path.join(repoRoot, "tools", "rdps-runtime-compatibility.mjs"),
    "--runtime", resolveRepoPath(value.runtimeCompatibility.runtime),
    "--carryForward", resolveRepoPath(value.runtimeCompatibility.carryForward),
    "--rowDiff", resolveRepoPath(value.runtimeCompatibility.rowDiff),
    "--output", destination,
  ], { cwd: repoRoot, label: "rdps-runtime-compatibility" });
}

function writeStaticValueProof(value, ledgerPath, destination) {
  run(process.execPath, [
    path.join(repoRoot, "tools", "rdps-static-value-proof.mjs"),
    "--ledger", ledgerPath,
    "--factors", resolveRepoPath(value.staticValueProof.factors),
    "--battleImagines", resolveRepoPath(value.staticValueProof.battleImagines),
    "--rogueDescriptions", resolveRepoPath(value.staticValueProof.rogueDescriptions),
    "--attributeDescriptions", resolveRepoPath(value.staticValueProof.attributeDescriptions),
    "--textDescriptions", resolveRepoPath(value.staticValueProof.textDescriptions),
    "--talents", resolveRepoPath(value.staticValueProof.talents),
    "--buffs", resolveRepoPath(value.staticValueProof.buffs),
    "--aoyiStars", resolveRepoPath(value.staticValueProof.aoyiStars),
    "--skillEffects", resolveRepoPath(value.staticValueProof.skillEffects),
    "--originLedger", resolveRepoPath(value.currentComponentBridge.originLedger),
    "--output", destination,
  ], { cwd: repoRoot, label: "rdps-static-value-proof" });
}

function writeOffensiveReadiness(value, ledgerPath, compatibilityPath, staticValueProofPath, destination) {
  run(process.execPath, [
    path.join(repoRoot, "tools", "rdps-offensive-readiness.mjs"),
    "--ledger", ledgerPath,
    "--compatibility", compatibilityPath,
    "--formulaGaps", resolveRepoPath(value.offensiveReadiness.formulaGaps),
    "--proofGates", resolveRepoPath(value.offensiveReadiness.proofGates),
    "--staticValueProof", staticValueProofPath,
    "--output", destination,
  ], { cwd: repoRoot, label: "rdps-offensive-readiness" });
}

function writeRemainingTrace(value, ledgerPath, readinessPath, staticValueProofPath, destination) {
  run(process.execPath, [
    path.join(repoRoot, "tools", "rdps-remaining-trace.mjs"),
    "--readiness", readinessPath,
    "--ledger", ledgerPath,
    "--relationships", resolveRepoPath(value.remainingTrace.relationships),
    "--componentBridge", resolveRepoPath(value.remainingTrace.componentBridge),
    "--valueProof", resolveRepoPath(value.remainingTrace.valueProof),
    "--staticWorklist", resolveRepoPath(value.staticWorklist.output),
    "--effectSources", resolveRepoPath(value.remainingTrace.effectSources),
    "--packetInventory", resolveRepoPath(value.remainingTrace.packetInventory),
    "--providerAudit", resolveRepoPath(value.remainingTrace.providerAudit),
    "--formulaModels", resolveRepoPath(value.remainingTrace.formulaModels),
    "--factorClosure", resolveRepoPath(value.remainingTrace.factorClosure),
    "--staticValueProof", staticValueProofPath,
    "--providerCandidateOutput", resolveRepoPath(value.remainingTrace.providerCandidateOutput),
    "--output", destination,
  ], { cwd: repoRoot, label: "rdps-remaining-trace" });
}

function writeGlobalOfflineGate(value, destination) {
  const gate = value.globalOfflineGate;
  run(process.execPath, [
    path.join(repoRoot, "tools", "rdps-global-offline-gate.mjs"),
    "--gameBuild", String(value.gameBuild),
    "--packetBuild", String(value.historicalPacketBuild),
    "--effectSources", resolveRepoPath(gate.effectSources),
    "--talentOwnership", resolveRepoPath(gate.talentOwnership),
    "--equipmentSets", resolveRepoPath(gate.equipmentSets),
    "--formulaModels", resolveRepoPath(gate.formulaModels),
    "--damageLedger", resolveRepoPath(gate.damageLedger),
    "--packetDamageScope", resolveRepoPath(gate.packetDamageScope),
    "--semanticAudit", resolveRepoPath(gate.semanticAudit),
    "--remainingTrace", resolveRepoPath(gate.remainingTrace),
    "--reachability", resolveRepoPath(gate.reachability),
    "--factorClosure", resolveRepoPath(gate.factorClosure),
    "--aoyiLedger", resolveRepoPath(gate.aoyiLedger),
    "--staticValueProof", resolveRepoPath(gate.staticValueProof),
    "--producedDamageReferenceScan", resolveRepoPath(gate.producedDamageReferenceScan),
    "--formulaExecutionProof", resolveRepoPath(gate.formulaExecutionProof),
    "--formulaApplicabilityProof", resolveRepoPath(gate.formulaApplicabilityProof),
    "--luckyExecutorProof", resolveRepoPath(gate.luckyExecutorProof),
    "--serverAuthoredExecutorProof", resolveRepoPath(gate.serverAuthoredExecutorProof),
    "--missingScriptDispositionProof", resolveRepoPath(gate.missingScriptDispositionProof),
    "--primaryStatAttackTransformProof", resolveRepoPath(gate.primaryStatAttackTransformProof),
    "--targetMitigationOfflineProof", resolveRepoPath(gate.targetMitigationOfflineProof),
    "--masteryPropertyOfflineProof", resolveRepoPath(gate.masteryPropertyOfflineProof),
    "--output", destination,
  ], { cwd: repoRoot, label: "rdps-global-offline-gate" });
}

function writePrimaryStatAttackTransformProof(value, destination) {
  const proof = value.primaryStatAttackTransformProof;
  run(process.execPath, [
    path.join(repoRoot, "tools", "primary-stat-attack-transform-proof.mjs"),
    "--gameBuild", String(value.gameBuild),
    "--professionTable", resolveRepoPath(proof.professionTable),
    "--talentTable", resolveRepoPath(proof.talentTable),
    "--talentStageTable", resolveRepoPath(proof.talentStageTable),
    "--fightAttrTable", resolveRepoPath(proof.fightAttrTable),
    "--attrDescription", resolveRepoPath(proof.attrDescription),
    "--output", destination,
  ], { cwd: repoRoot, label: "primary-stat-attack-transform-proof" });
}

function writeTargetMitigationOfflineProof(value, destination) {
  const proof = value.targetMitigationOfflineProof;
  run(process.execPath, [
    path.join(repoRoot, "tools", "target-mitigation-offline-exhaustion-proof.mjs"),
    "--gameBuild", String(value.gameBuild),
    "--packetBuild", String(value.historicalPacketBuild),
    "--luaDefinitionAudit", resolveRepoPath(proof.luaDefinitionAudit),
    "--luaConsumerAudit", resolveRepoPath(proof.luaConsumerAudit),
    "--directCallsiteAudit", resolveRepoPath(proof.directCallsiteAudit),
    "--fightAttributeProof", resolveRepoPath(proof.fightAttributeProof),
    "--packetPairProof", resolveRepoPath(proof.packetPairProof),
    "--output", destination,
  ], { cwd: repoRoot, label: "target-mitigation-offline-exhaustion-proof" });
}

function writeMasteryPropertyOfflineProof(value, destination) {
  const proof = value.masteryPropertyOfflineProof;
  run(process.execPath, [
    path.join(repoRoot, "tools", "mastery-property-offline-exhaustion-proof.mjs"),
    "--gameBuild", String(value.gameBuild),
    "--packetBuild", String(value.historicalPacketBuild),
    "--inventory", resolveRepoPath(proof.inventory),
    "--fightAttributeProof", resolveRepoPath(proof.fightAttributeProof),
    "--historicalFalconryProof", resolveRepoPath(proof.historicalFalconryProof),
    "--snapshotProof", resolveRepoPath(proof.snapshotProof),
    "--castSnapshotProof", resolveRepoPath(proof.castSnapshotProof),
    "--skills", resolveRepoPath(proof.skills),
    "--buffs", resolveRepoPath(proof.buffs),
    "--skillTable", resolveRepoPath(proof.skillTable),
    "--effectSources", resolveRepoPath(proof.effectSources),
    "--output", destination,
  ], { cwd: repoRoot, label: "mastery-property-offline-exhaustion-proof" });
}

function writeFactorClosure(value, destination) {
  run(process.execPath, [
    path.join(repoRoot, "tools", "psychoscope-factor-closure.mjs"),
    "--factors", resolveRepoPath(value.factorClosure.factors),
    "--recount", resolveRepoPath(value.factorClosure.recount),
    "--skills", resolveRepoPath(value.factorClosure.skills),
    "--build", String(value.gameBuild),
    "--output", destination,
  ], { cwd: repoRoot, label: "psychoscope-factor-offline-closure" });
}

function writeCurrentComponentBridge(value, destination) {
  run(process.execPath, [
    path.join(repoRoot, "tools", "rdps-current-component-bridge.mjs"),
    "--originLedger", resolveRepoPath(value.currentComponentBridge.originLedger),
    "--decodedRoot", resolveRepoPath(value.currentComponentBridge.decodedRoot),
    "--gameBuild", String(value.gameBuild),
    "--output", destination,
  ], { cwd: repoRoot, label: "current-component-relationship-bridge" });
}

function compactCandidate(row) {
  return {
    source_rule_id: row.source_rule_id,
    source_id: row.source_id,
    source_name: row.source_name,
    effect_ids: row.effect_ids,
    scope_queue: row.scope_queue,
    component_scope_routes: row.component_scope_routes,
    effective_transfer_eligibilities: row.effective_transfer_eligibilities,
    current_build_promotion_eligible: row.current_build_promotion_eligible,
    current_build_reachability: row.current_build_reachability,
    remaining_requirement: row.remaining_requirement
  };
}

function classificationChange(row) {
  return {
    ...compactCandidate(row),
    runtime_effect_family_evidence: row.runtime_effect_family_evidence
  };
}

function candidateIdentity(row) {
  const identity = row?.source_id || row?.source_rule_id;
  if (!identity) throw new Error("rDPS ledger candidate is missing both source_id and source_rule_id");
  return String(identity);
}

function uniqueCandidateMap(candidates, label) {
  const result = new Map();
  for (const candidate of candidates) {
    const identity = candidateIdentity(candidate);
    if (result.has(identity)) {
      throw new Error(`${label} has duplicate stable source identity ${identity}`);
    }
    result.set(identity, candidate);
  }
  return result;
}

function candidateEvidenceDigest(row) {
  return JSON.stringify({
    source_rule_id: row.source_rule_id,
    effect_ids: row.effect_ids,
    scope_queue: row.scope_queue,
    component_scope_routes: row.component_scope_routes,
    effective_transfer_eligibilities: row.effective_transfer_eligibilities,
    current_build_promotion_eligible: row.current_build_promotion_eligible,
    remaining_requirement: row.remaining_requirement,
    runtime_effect_family_evidence: row.runtime_effect_family_evidence
  });
}

function uniqueRuleMap(rows, label) {
  const result = new Map();
  for (const row of rows) {
    if (!row.source_rule_id) throw new Error(`${label} row is missing source_rule_id`);
    if (result.has(row.source_rule_id)) {
      throw new Error(`${label} has duplicate source_rule_id ${row.source_rule_id}`);
    }
    result.set(row.source_rule_id, row);
  }
  return result;
}

function sourceType(sourceId) {
  if (!sourceId) return "unresolved-source-type";
  const separator = String(sourceId).indexOf(":");
  return separator === -1 ? String(sourceId) : String(sourceId).slice(0, separator);
}

function distinctStrings(values) {
  return [...new Set(values.filter((value) => typeof value === "string" && value.length > 0))].sort();
}

function distinctNumbers(values) {
  return [...new Set((values || [])
    .map(Number)
    .filter((value) => Number.isSafeInteger(value) && value > 0))]
    .sort((left, right) => left - right);
}

function countStrings(values) {
  return (values || []).reduce((counts, value) => {
    const key = String(value);
    counts[key] = (counts[key] || 0) + 1;
    return counts;
  }, {});
}

function distinctObjects(values) {
  const byValue = new Map();
  for (const value of values) {
    const key = JSON.stringify(value);
    if (!byValue.has(key)) byValue.set(key, value);
  }
  return [...byValue.values()];
}

function attributionCatalogState(candidate, work, blockers) {
  if (candidate.current_build_reachability?.aggregate_status === "definition-only-no-current-incoming-reference") {
    return "preserved-definition-unreachable-in-current-build";
  }
  if (isUnresolved(candidate)) return "blocked-unresolved-mechanic-or-scope";
  if (hasPartialComponentResolution(candidate)) {
    return "component-scoped-partial-proof-unresolved-lanes-preserved";
  }
  if (candidate.current_build_promotion_eligible) return "matching-build-simulation-ready";
  if (blockers.length > 0 || work.static_value_state?.includes("blocker")) {
    return "catalogued-semantics-blocked-by-explicit-proof-requirements";
  }
  return "catalogued-semantics-awaiting-matching-build-runtime-proof";
}

function ledgerCounts(ledger) {
  const unresolved = ledger.candidates.filter(isUnresolved);
  const componentRoutes = ledger.candidates.flatMap((row) => row.component_scope_routes || []);
  const effectIds = new Set(unresolved.flatMap((row) => row.effect_ids.map(String)));
  const definitionOnlyUnreachable = ledger.candidates.filter(
    (row) => row.current_build_reachability?.aggregate_status === "definition-only-no-current-incoming-reference"
  );
  const awaitingProof = ledger.candidates.filter(
    (row) => !isUnresolved(row)
      && row.current_build_promotion_eligible === false
      && row.current_build_reachability?.aggregate_status !== "definition-only-no-current-incoming-reference"
  );
  return {
    candidates: ledger.candidates.length,
    unresolved_mechanics: unresolved.length,
    unresolved_effect_ids: effectIds.size,
    unresolved_component_routes: componentRoutes.filter(isComponentRouteUnresolved).length,
    resolved_component_routes: componentRoutes.filter((row) => !isComponentRouteUnresolved(row)).length,
    partially_resolved_component_scoped_sources: ledger.candidates.filter(hasPartialComponentResolution).length,
    preserved_definition_unreachable_current_build: definitionOnlyUnreachable.length,
    resolved_awaiting_current_build_proof: awaitingProof.length,
    runtime_rdps_promoted: ledger.candidates.filter((row) => row.current_build_promotion_eligible).length
  };
}

function isUnresolved(row) {
  if (!row) return false;
  const componentRoutes = row.component_scope_routes || [];
  if (componentRoutes.length > 0) {
    return componentRoutes.every(isComponentRouteUnresolved);
  }
  return isOpenRecipientScopeQueue(row.scope_queue)
    || row.effective_transfer_eligibilities?.includes("recipient-scope-unresolved");
}

function isComponentRouteUnresolved(row) {
  return isOpenRecipientScopeQueue(row?.scope_queue);
}

function isOpenRecipientScopeQueue(queue) {
  return queue === "unresolved-provider-recipient"
    || queue === "unresolved-target-filtered-provider-recipient"
    || queue === "owner-local-formula-context-requires-recipient-proof"
    || queue === "mixed-source-output-and-open-owner-context"
    || queue === "mixed-or-unclassified-scope"
    || queue === "component-scoped-mixed";
}

function hasPartialComponentResolution(row) {
  const routes = row?.component_scope_routes || [];
  return routes.some(isComponentRouteUnresolved)
    && routes.some((route) => !isComponentRouteUnresolved(route));
}

function artifactReference(filePath) {
  const content = readFileSync(filePath);
  return {
    path: relativeRepoPath(filePath),
    sha256: createHash("sha256").update(content).digest("hex")
  };
}

function run(command, args, { cwd, label }) {
  if (options.dryRun) {
    console.log(`[dry-run:${label}] ${command} ${args.join(" ")}`);
    return;
  }
  console.log(`\n=== ${label} ===`);
  const result = spawnSync(command, args, { cwd, stdio: "inherit", shell: false });
  if (result.status !== 0) {
    process.exit(result.status || 1);
  }
}

function readJson(filePath, label) {
  if (!existsSync(filePath)) throw new Error(`${label} not found: ${filePath}`);
  return JSON.parse(readFileSync(filePath, "utf8"));
}

function resolveRepoPath(input) {
  if (!input) throw new Error("A required path is missing.");
  return path.isAbsolute(input) ? input : path.resolve(repoRoot, input);
}

function relativeRepoPath(input) {
  return path.relative(repoRoot, input).replaceAll("\\", "/");
}

function validateConfig(value) {
  for (const field of [
    "gameBuild",
    "historicalPacketBuild",
    "baselineLedger",
    "summaryOutput",
    "proofCatalogOutput",
    "relationshipOverviewOutput",
    "bundleManifestOutput"
  ]) {
    if (value[field] === undefined || value[field] === "") throw new Error(`config.${field} is required`);
  }
  if (!value.matchingBuildValidation?.output) {
    throw new Error("config.matchingBuildValidation.output is required");
  }
  if (!value.runtimeProofWorklist?.output || !value.runtimeProofWorklist?.reportSchema) {
    throw new Error("config.runtimeProofWorklist.output and reportSchema are required");
  }
  for (const field of [
    "inventory", "fightAttributeProof", "historicalFalconryProof",
    "snapshotProof", "castSnapshotProof", "skills", "buffs", "skillTable", "effectSources", "output",
  ]) {
    if (!value.masteryPropertyOfflineProof?.[field]) {
      throw new Error(`config.masteryPropertyOfflineProof.${field} is required`);
    }
  }
  for (const field of [
    "luaDefinitionAudit", "luaConsumerAudit", "directCallsiteAudit",
    "fightAttributeProof", "packetPairProof", "output",
  ]) {
    if (!value.targetMitigationOfflineProof?.[field]) {
      throw new Error(`config.targetMitigationOfflineProof.${field} is required`);
    }
  }
  if (!Array.isArray(value.extractor?.derivedScripts) || value.extractor.derivedScripts.length === 0) {
    throw new Error("config.extractor.derivedScripts must not be empty");
  }
  if (!value.ledger?.proofInputs || Object.keys(value.ledger.proofInputs).length === 0) {
    throw new Error("config.ledger.proofInputs must not be empty");
  }
  if (!value.reachability?.script || !value.reachability?.output) {
    throw new Error("config.reachability.script and config.reachability.output are required");
  }
  for (const field of ["runtime", "carryForward", "rowDiff", "output"]) {
    if (!value.runtimeCompatibility?.[field]) {
      throw new Error(`config.runtimeCompatibility.${field} is required`);
    }
  }
  for (const field of ["formulaGaps", "proofGates", "output"]) {
    if (!value.offensiveReadiness?.[field]) {
      throw new Error(`config.offensiveReadiness.${field} is required`);
    }
  }
  for (const field of ["factors", "battleImagines", "rogueDescriptions", "attributeDescriptions", "textDescriptions", "talents", "buffs", "aoyiStars", "skillEffects", "output"]) {
    if (!value.staticValueProof?.[field]) {
      throw new Error(`config.staticValueProof.${field} is required`);
    }
  }
  for (const field of ["relationships", "componentBridge", "valueProof", "effectSources", "packetInventory", "providerAudit", "formulaModels", "factorClosure", "providerCandidateOutput", "output"]) {
    if (!value.remainingTrace?.[field]) {
      throw new Error(`config.remainingTrace.${field} is required`);
    }
  }
  for (const field of ["factors", "recount", "skills", "output"]) {
    if (!value.factorClosure?.[field]) {
      throw new Error(`config.factorClosure.${field} is required`);
    }
  }
  for (const field of [
    "professionTable", "talentTable", "talentStageTable", "fightAttrTable",
    "attrDescription", "output",
  ]) {
    if (!value.primaryStatAttackTransformProof?.[field]) {
      throw new Error(`config.primaryStatAttackTransformProof.${field} is required`);
    }
  }
  for (const field of ["originLedger", "decodedRoot", "output"]) {
    if (!value.currentComponentBridge?.[field]) {
      throw new Error(`config.currentComponentBridge.${field} is required`);
    }
  }
  for (const field of [
    "attributeFamilyFormulaProof",
    "fightAttributeTransformSurface",
    "attributeDependencyOutput"
  ]) {
    if (!value.relationshipEvidence?.[field]) {
      throw new Error(`config.relationshipEvidence.${field} is required`);
    }
  }
}

function parseArgs(argv) {
  const result = { config: "", baseline: "", output: "", summary: "", skipExtractors: false, dryRun: false };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === "--skip-extractors") result.skipExtractors = true;
    else if (argument === "--dry-run") result.dryRun = true;
    else if (["--config", "--baseline", "--output", "--summary"].includes(argument)) {
      const value = argv[index + 1];
      if (!value) throw new Error(`Missing value for ${argument}`);
      result[argument.slice(2)] = value;
      index += 1;
    } else throw new Error(`Unknown argument: ${argument}`);
  }
  if (!result.config) {
    throw new Error("Usage: node tools/rdps-research-batch.mjs --config <config.json> [--skip-extractors] [--dry-run]");
  }
  return result;
}
