#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

const FAMILY_ROUTES = {
  anti_cheat: {
    phase: "non-gameplay-compatibility",
    extractors: ["anti-cheat-version-inventory"],
    proofSuites: ["build-identity"],
    semanticCandidateRoutes: [],
  },
  client_support: {
    phase: "client-bootstrap-compatibility",
    extractors: ["client-support-config-scan"],
    proofSuites: ["build-identity", "schema-diff"],
    semanticCandidateRoutes: ["system-client-ui-config"],
  },
  container_auxiliary: {
    phase: "package-index-extraction",
    extractors: ["container-index-scan", "package-membership-diff"],
    proofSuites: ["asset-catalog-diff", "schema-diff"],
    semanticCandidateRoutes: ["all-derived-routes-until-row-diff"],
  },
  container_index: {
    phase: "package-index-extraction",
    extractors: ["container-index-scan", "package-membership-diff"],
    proofSuites: ["asset-catalog-diff", "schema-diff"],
    semanticCandidateRoutes: ["all-derived-routes-until-row-diff"],
  },
  container_package: {
    phase: "changed-package-extraction",
    extractors: ["changed-package-extraction", "decoded-table-regeneration", "extracted-artifact-regeneration"],
    proofSuites: ["asset-catalog-diff", "combat-table-diff", "localization-diff", "schema-diff"],
    semanticCandidateRoutes: ["all-derived-routes-until-row-diff"],
  },
  il2cpp_metadata: {
    phase: "il2cpp-schema-extraction",
    extractors: ["il2cpp-metadata-scan", "protobuf-schema-regeneration", "rpc-route-regeneration"],
    proofSuites: ["formula-surface-diff", "protobuf-coverage", "runtime-conservation"],
    semanticCandidateRoutes: ["combat-buffs-effects-formulas", "combat-skills-actions", "system-client-ui-config"],
  },
  il2cpp_native_code: {
    phase: "il2cpp-native-extraction",
    extractors: ["il2cpp-combat-surface", "protobuf-native-wire-proof", "formula-executor-rescan"],
    proofSuites: ["formula-surface-diff", "protobuf-coverage", "runtime-conservation"],
    semanticCandidateRoutes: ["combat-buffs-effects-formulas", "combat-skills-actions", "talents-seasonal-psychoscope"],
  },
  native_executable: {
    phase: "native-runtime-compatibility",
    extractors: ["native-runtime-identity-scan"],
    proofSuites: ["build-identity", "protobuf-coverage", "runtime-conservation"],
    semanticCandidateRoutes: ["system-client-ui-config"],
  },
  native_plugin: {
    phase: "native-plugin-compatibility",
    extractors: ["native-plugin-identity-scan"],
    proofSuites: ["build-identity", "runtime-conservation"],
    semanticCandidateRoutes: ["system-client-ui-config"],
  },
  protected_client_base: {
    phase: "protected-client-compatibility",
    extractors: ["protected-client-base-scan"],
    proofSuites: ["build-identity", "protobuf-coverage", "runtime-conservation"],
    semanticCandidateRoutes: ["combat-buffs-effects-formulas", "combat-skills-actions"],
  },
  unity_player_data: {
    phase: "unity-data-extraction",
    extractors: ["unity-asset-reference-extraction", "presentation-reference-regeneration"],
    proofSuites: ["asset-catalog-diff", "localization-diff", "schema-diff"],
    semanticCandidateRoutes: ["localization-presentation-assets", "system-client-ui-config"],
  },
};

const [command = "help", ...rest] = process.argv.slice(2);

if (command === "generate") {
  generate(parseOptions(rest));
} else if (command === "verify") {
  verify(parseOptions(rest));
} else if (command === "self-test") {
  selfTest();
} else {
  usage();
  process.exitCode = command === "help" ? 0 : 1;
}

function generate(options) {
  const manifestDiffPath = required(options, "manifest-diff");
  const installedManifestPath = required(options, "installed-manifest");
  const physicalDiffPath = required(options, "physical-diff");
  const sourceManifestPath = required(options, "source-manifest");
  const semanticDiffPath = required(options, "semantic-diff");
  const referenceGraphDiffPath = options["reference-graph-diff"] ?? null;
  const semanticFieldDiffPath = options["semantic-field-diff"] ?? null;
  const decodedFieldDiffPath = options["decoded-field-diff"] ?? null;
  const mechanicDependencyDiffPath = options["mechanic-dependency-diff"] ?? null;
  const outputPath = required(options, "output");

  const manifestDiff = readJson(manifestDiffPath);
  const installedManifest = readJson(installedManifestPath);
  const physicalDiff = readJson(physicalDiffPath);
  const sourceManifest = readJson(sourceManifestPath);
  const semanticDiff = readJson(semanticDiffPath);
  const referenceGraphDiff = referenceGraphDiffPath ? readJson(referenceGraphDiffPath) : null;
  const semanticFieldDiff = semanticFieldDiffPath ? readJson(semanticFieldDiffPath) : null;
  const decodedFieldDiff = decodedFieldDiffPath ? readJson(decodedFieldDiffPath) : null;
  const mechanicDependencyDiff = mechanicDependencyDiffPath ? readJson(mechanicDependencyDiffPath) : null;
  const plan = buildPlan({
    manifestDiff,
    installedManifest,
    physicalDiff,
    sourceManifest,
    semanticDiff,
    referenceGraphDiff,
    semanticFieldDiff,
    decodedFieldDiff,
    mechanicDependencyDiff,
    inputs: {
      manifestDiff: evidence(manifestDiffPath),
      installedManifest: evidence(installedManifestPath),
      physicalDiff: evidence(physicalDiffPath),
      sourceManifest: evidence(sourceManifestPath),
      semanticDiff: evidence(semanticDiffPath),
      ...(referenceGraphDiffPath ? { referenceGraphDiff: evidence(referenceGraphDiffPath) } : {}),
      ...(semanticFieldDiffPath ? { semanticFieldDiff: evidence(semanticFieldDiffPath) } : {}),
      ...(decodedFieldDiffPath ? { decodedFieldDiff: evidence(decodedFieldDiffPath) } : {}),
      ...(mechanicDependencyDiffPath ? { mechanicDependencyDiff: evidence(mechanicDependencyDiffPath) } : {}),
    },
  });

  mkdirSync(path.dirname(path.resolve(outputPath)), { recursive: true });
  writeFileSync(outputPath, `${JSON.stringify(plan, null, 2)}\n`);
  console.log(summaryLine(plan));
}

function verify(options) {
  const inputPath = required(options, "input");
  const plan = readJson(inputPath);
  assertPlan(plan);
  console.log(summaryLine(plan));
}

function buildPlan({ manifestDiff, installedManifest, physicalDiff, sourceManifest, semanticDiff, referenceGraphDiff = null, semanticFieldDiff = null, decodedFieldDiff = null, mechanicDependencyDiff = null, inputs }) {
  const previousGid = String(manifestDiff.previousManifest?.manifestGid ?? "");
  const currentGid = String(manifestDiff.currentManifest?.manifestGid ?? "");
  if (!/^\d+$/.test(previousGid) || !/^\d+$/.test(currentGid)) {
    throw new Error("Depot manifest IDs must be exact decimal strings.");
  }
  if (String(installedManifest.cachedDepotManifest?.manifestId ?? "") !== currentGid) {
    throw new Error("Installed-client manifest does not match the candidate depot manifest ID.");
  }
  if (String(physicalDiff.baseline_build_id ?? "") !== String(semanticDiff.baselineBuild ?? "")
    || String(physicalDiff.build_id ?? "") !== String(installedManifest.gameBuild ?? "")) {
    throw new Error("Physical, semantic, or installed-client build identities do not describe the same transition.");
  }
  if (referenceGraphDiff && (
    String(referenceGraphDiff.baseline_build ?? "") !== String(semanticDiff.baselineBuild ?? "")
    || String(referenceGraphDiff.candidate_build ?? "") !== String(installedManifest.gameBuild ?? "")
  )) {
    throw new Error("Decoded reference graph diff does not describe the same build transition.");
  }
  if (semanticFieldDiff && (
    String(semanticFieldDiff.baseline_build ?? "") !== String(semanticDiff.baselineBuild ?? "")
    || String(semanticFieldDiff.candidate_build ?? "") !== String(installedManifest.gameBuild ?? "")
  )) {
    throw new Error("Semantic field-schema diff does not describe the same build transition.");
  }
  if (decodedFieldDiff && (
    String(decodedFieldDiff.baseline_build ?? "") !== String(semanticDiff.baselineBuild ?? "")
    || String(decodedFieldDiff.candidate_build ?? "") !== String(installedManifest.gameBuild ?? "")
  )) {
    throw new Error("Decoded field-schema diff does not describe the same build transition.");
  }
  if (mechanicDependencyDiff && (
    String(mechanicDependencyDiff.baseline_build ?? "") !== String(semanticDiff.baselineBuild ?? "")
    || String(mechanicDependencyDiff.candidate_build ?? "") !== String(installedManifest.gameBuild ?? "")
  )) {
    throw new Error("Semantic mechanic dependency diff does not describe the same build transition.");
  }

  const currentFamilies = new Map(
    installedManifest.files.map((file) => [normalize(file.relativePath), file.family]),
  );
  const physicalFamilies = new Map();
  for (const key of ["added", "removed", "changed", "unstable"]) {
    for (const file of physicalDiff[key] ?? []) {
      physicalFamilies.set(normalize(file.relative_path), file.family);
    }
  }

  const candidates = [];
  for (const [change, records] of [
    ["added", manifestDiff.added ?? []],
    ["removed", manifestDiff.removed ?? []],
    ["changed", manifestDiff.changed ?? []],
  ]) {
    for (const record of records) {
      const raw = change === "changed" ? record.current : record;
      const relativePath = normalize(raw.path ?? record.path ?? "");
      const family = currentFamilies.get(relativePath)
        ?? physicalFamilies.get(relativePath)
        ?? classifyPath(relativePath);
      const route = FAMILY_ROUTES[family] ?? null;
      candidates.push({
        relativePath,
        change,
        family,
        bytesBefore: change === "added" ? null : Number(change === "changed" ? record.previous.size : record.size),
        bytesAfter: change === "removed" ? null : Number(raw.size),
        reasons: change === "changed" ? record.reasons : [change],
        routing: route,
      });
    }
  }
  candidates.sort((a, b) => a.relativePath.localeCompare(b.relativePath));

  const physicalCandidatePaths = new Set();
  for (const key of ["added", "removed", "changed"]) {
    for (const file of physicalDiff[key] ?? []) physicalCandidatePaths.add(normalize(file.relative_path));
  }
  const manifestCandidatePaths = new Set(candidates.map((candidate) => candidate.relativePath));
  const onlyInManifest = [...manifestCandidatePaths].filter((value) => !physicalCandidatePaths.has(value)).sort();
  const onlyInPhysicalDiff = [...physicalCandidatePaths].filter((value) => !manifestCandidatePaths.has(value)).sort();
  if (onlyInManifest.length > 0 || onlyInPhysicalDiff.length > 0) {
    throw new Error(
      `Cached depot manifest and local SHA-256 candidate paths disagree: manifest-only ${onlyInManifest.length}, physical-only ${onlyInPhysicalDiff.length}.`,
    );
  }

  const unclassified = candidates.filter((candidate) => !candidate.routing);
  const byFamily = countBy(candidates, (candidate) => candidate.family);
  const allDerivedRoutes = Object.keys(sourceManifest.routeSummary ?? {}).sort();
  const extractionGroups = groupRoutes(candidates, allDerivedRoutes);
  const changedDomains = (semanticDiff.changedDomains ?? []).map((domain) => ({
    domain: domain.domain,
    addedSources: domain.addedSources?.length ?? 0,
    removedSources: domain.removedSources?.length ?? 0,
    addedRows: domain.addedRows?.length ?? 0,
    removedRows: domain.removedRows?.length ?? 0,
    changedRows: domain.changedRows?.length ?? 0,
    proofSuites: domain.proofSuites ?? [],
    changeActions: domain.changeActions ?? [],
  }));
  const referenceDomains = (referenceGraphDiff?.affected_domains ?? []).map((domain) => ({
    domain: domain.domain,
    status: domain.status,
    changedTables: domain.changed_tables ?? [],
    baselineRows: domain.baseline_rows ?? 0,
    candidateRows: domain.candidate_rows ?? 0,
  }));
  const exactFieldSchemaChanges = referenceGraphDiff?.exact_field_schema_changes ?? [];
  const semanticFieldSchemaChanges = semanticFieldDiff ? {
    added: semanticFieldDiff.added ?? [],
    removed: semanticFieldDiff.removed ?? [],
    changed: semanticFieldDiff.changed ?? [],
  } : null;
  const semanticFieldSchemaChangeCount = semanticFieldSchemaChanges
    ? semanticFieldSchemaChanges.added.length + semanticFieldSchemaChanges.removed.length + semanticFieldSchemaChanges.changed.length
    : 0;
  const decodedFieldSchemaChanges = decodedFieldDiff ? {
    added: decodedFieldDiff.added ?? [],
    removed: decodedFieldDiff.removed ?? [],
    changed: decodedFieldDiff.changed ?? [],
    affectedTables: decodedFieldDiff.affected_tables ?? [],
  } : null;
  const decodedFieldSchemaChangeCount = decodedFieldSchemaChanges
    ? decodedFieldSchemaChanges.added.length + decodedFieldSchemaChanges.removed.length + decodedFieldSchemaChanges.changed.length
    : 0;
  const focusedDecodedTables = decodedFieldSchemaChanges?.affectedTables ?? [];
  const mechanicDependencyChanges = mechanicDependencyDiff ? {
    added: mechanicDependencyDiff.added ?? [],
    removed: mechanicDependencyDiff.removed ?? [],
    changed: mechanicDependencyDiff.changed ?? [],
    unchanged: mechanicDependencyDiff.unchanged ?? [],
    changedDependencyTables: mechanicDependencyDiff.changed_dependency_tables ?? [],
  } : null;
  const changedMechanicCount = mechanicDependencyChanges
    ? mechanicDependencyChanges.added.length + mechanicDependencyChanges.removed.length + mechanicDependencyChanges.changed.length
    : 0;
  const proofDomains = [...new Set(exactFieldSchemaChanges.map((entry) => entry.domain).filter(Boolean))].sort();
  const focusedDomains = [...new Set([
    ...changedDomains.map((domain) => domain.domain),
    ...referenceDomains.map((domain) => domain.domain),
    ...proofDomains,
  ])].sort();

  const plan = {
    schemaVersion: "rlogs.bpsr-patch-rescan-plan.v2",
    generatedBy: "tools/bpsr-patch-rescan-plan.mjs",
    game: installedManifest.game,
    deployment: installedManifest.deployment,
    channel: installedManifest.channel,
    baselineBuild: Number(semanticDiff.baselineBuild),
    candidateBuild: Number(installedManifest.gameBuild),
    depotTransition: {
      depotId: manifestDiff.currentManifest.depotId,
      previousManifestId: previousGid,
      currentManifestId: currentGid,
    },
    authority: {
      manifestCandidates: "offline Steam cached depot manifests",
      byteVerification: "local SHA-256 installed-client manifest and physical diff",
      derivedEvidence: "complete extracted and decoded source manifest",
      semanticEvidence: "row-level seasonal-domain diff",
      relationshipEvidence: referenceGraphDiff
        ? "decoded row hashes, declared relationship edges, exact semantic field schemas, build-locked IL2CPP callsite proofs, missing targets, and untyped reference groups"
        : "legacy transition has no decoded reference graph baseline",
      semanticFieldEvidence: semanticFieldDiff
        ? "complete per-field decoded value domains, IL2CPP property/getter schemas, accepted targets, and explicit open reasons"
        : "legacy transition has no semantic field-schema ledger baseline",
      decodedFieldEvidence: decodedFieldDiff
        ? "complete scalar, array, nested-object, type, value-profile, IL2CPP schema, mechanics-routing, and relationship-profile diffs"
        : "legacy transition has no universal decoded field-schema baseline",
      mechanicDependencyEvidence: mechanicDependencyDiff
        ? "per-mechanic decoded rows, sensitive fields, exact relationships, candidate evidence, seeds, and unresolved proof requirements"
        : "legacy transition has no semantic mechanic dependency baseline",
      runtimeMechanics: "packet replay and captured lifecycle proof",
    },
    policy: {
      steamDbRole: "public manifest-history index and update alarm only",
      noSilentOmissions: true,
      unknownCandidatesBlockFastPath: true,
      unchangedSemanticDomainsRetainProof: true,
      changedSemanticDomainsRequireFocusedReproof: true,
      currentBuildCallsiteProofChangesRequireReproof: true,
      changedSemanticFieldSchemasRequireFocusedReproof: true,
      changedDecodedFieldSchemasRequireFocusedReproof: true,
      changedMechanicDependenciesRequireFocusedReproof: true,
      runtimeRulesAreNeverPromotedFromStaticDiffAlone: true,
    },
    summary: {
      depotRecords: manifestDiff.summary.added + manifestDiff.summary.removed + manifestDiff.summary.changed + manifestDiff.summary.unchanged,
      depotAuthoredFiles: installedManifest.coverage.depotAuthoredFiles,
      candidateFiles: candidates.length,
      physicalCandidateFiles: physicalCandidatePaths.size,
      candidatePathDisagreements: onlyInManifest.length + onlyInPhysicalDiff.length,
      addedFiles: candidates.filter((candidate) => candidate.change === "added").length,
      removedFiles: candidates.filter((candidate) => candidate.change === "removed").length,
      changedFiles: candidates.filter((candidate) => candidate.change === "changed").length,
      classifiedCandidates: candidates.length - unclassified.length,
      unclassifiedCandidates: unclassified.length,
      physicalFamilies: byFamily,
      derivedSourceFiles: sourceManifest.coverage.filesHashed,
      derivedRoutes: allDerivedRoutes.length,
      changedSemanticDomains: changedDomains.length,
      changedReferenceDomains: referenceDomains.length,
      changedReferenceProofDomains: proofDomains.length,
      focusedSemanticDomains: focusedDomains.length,
      decodedChangedTables: referenceGraphDiff?.summary?.changed_tables ?? 0,
      decodedAddedRows: referenceGraphDiff?.summary?.added_rows ?? 0,
      decodedRemovedRows: referenceGraphDiff?.summary?.removed_rows ?? 0,
      decodedChangedRows: referenceGraphDiff?.summary?.changed_rows ?? 0,
      decodedRelationshipEdgeChanges: referenceGraphDiff
        ? referenceGraphDiff.summary.added_exact_edges + referenceGraphDiff.summary.removed_exact_edges
        : 0,
      decodedAddedExactFieldSchemas: referenceGraphDiff?.summary?.added_exact_field_schemas ?? 0,
      decodedRemovedExactFieldSchemas: referenceGraphDiff?.summary?.removed_exact_field_schemas ?? 0,
      decodedChangedExactFieldSchemas: referenceGraphDiff?.summary?.changed_exact_field_schemas ?? 0,
      decodedChangedCurrentBuildCallsiteProofs: referenceGraphDiff?.summary?.changed_current_build_callsite_proofs ?? 0,
      decodedCallsiteProofInputsChanged: referenceGraphDiff?.summary?.callsite_proof_inputs_changed ?? false,
      decodedReferenceComparisonAvailable: Boolean(referenceGraphDiff),
      semanticFieldSchemaComparisonAvailable: Boolean(semanticFieldDiff),
      semanticFieldSchemaChanges: semanticFieldSchemaChangeCount,
      decodedFieldSchemaComparisonAvailable: Boolean(decodedFieldDiff),
      decodedFieldSchemaChanges: decodedFieldSchemaChangeCount,
      focusedDecodedTables: focusedDecodedTables.length,
      mechanicDependencyComparisonAvailable: Boolean(mechanicDependencyDiff),
      changedMechanics: changedMechanicCount,
      unchangedMechanics: mechanicDependencyChanges?.unchanged.length ?? 0,
      changedMechanicDependencyTables: mechanicDependencyChanges?.changedDependencyTables.length ?? 0,
      unchangedSemanticDomains: semanticDiff.unchangedDomains?.length ?? 0,
      comparisonGaps: semanticDiff.missingManifests?.length ?? 0,
      fastPathReady: unclassified.length === 0,
    },
    rescanStages: [
      {
        stage: 1,
        name: "manifest-candidate-selection",
        inputCount: manifestDiff.summary.candidateFiles,
        outputCount: candidates.length,
        invariant: "Every added, removed, or changed depot record is retained exactly once.",
      },
      {
        stage: 2,
        name: "local-byte-and-family-verification",
        inputCount: candidates.length,
        outputCount: candidates.length - unclassified.length,
        invariant: "Local SHA-256 and physical-family evidence verify candidate identity; unknown paths fail closed.",
      },
      {
        stage: 3,
        name: "targeted-extraction",
        inputCount: extractionGroups.length,
        outputCount: sourceManifest.coverage.filesHashed,
        invariant: "Only candidate physical families are reacquired; every derived artifact is re-manifested after extraction.",
      },
      {
        stage: 4,
        name: "row-level-semantic-diff",
        inputCount: allDerivedRoutes.length,
        outputCount: focusedDomains.length + focusedDecodedTables.length,
        invariant: "Package containers remain broad candidates only until decoded rows, exact field schemas, complete semantic field domains, relationship edges, and build-locked callsite proof fingerprints identify exact changed work.",
      },
      {
        stage: 5,
        name: "focused-proof-invalidation",
        inputCount: focusedDomains.length + focusedDecodedTables.length + semanticFieldSchemaChangeCount + decodedFieldSchemaChangeCount + changedMechanicCount,
        outputCount: changedDomains.reduce((set, domain) => {
          for (const suite of domain.proofSuites) set.add(suite);
          if (referenceGraphDiff?.summary?.callsite_proof_inputs_changed) set.add("il2cpp-table-reference-callsite-proof");
          return set;
        }, new Set()).size,
        invariant: "Changed semantic domains, universal decoded field profiles, ID field schemas, or build-locked callsite proof inputs invalidate only their affected proofs; runtime attribution still requires packet evidence.",
      },
    ],
    allDerivedRoutes,
    extractionGroups,
    changedSemanticDomains: changedDomains,
    decodedReferenceDomains: referenceDomains,
    focusedSemanticDomains: focusedDomains,
    decodedReferenceChanges: referenceGraphDiff ? {
      tableChanges: referenceGraphDiff.table_changes,
      exactEdgeChanges: referenceGraphDiff.exact_edge_changes,
      missingTargetChanges: referenceGraphDiff.missing_target_changes,
      semanticFieldChanges: referenceGraphDiff.semantic_field_changes,
      exactFieldSchemaChanges,
      callsiteProofChanges: referenceGraphDiff.callsite_proof_changes,
      ambiguousFieldChanges: referenceGraphDiff.ambiguous_field_changes,
    } : null,
    semanticFieldSchemaChanges,
    decodedFieldSchemaChanges,
    focusedDecodedTables,
    mechanicDependencyChanges,
    unchangedSemanticDomains: (semanticDiff.unchangedDomains ?? []).map((domain) => domain.domain ?? domain),
    comparisonGaps: semanticDiff.missingManifests ?? [],
    candidateReconciliation: {
      manifestOnly: onlyInManifest,
      physicalDiffOnly: onlyInPhysicalDiff,
    },
    unclassifiedCandidates: unclassified.map((candidate) => candidate.relativePath),
    candidates,
    inputs,
  };
  assertPlan(plan);
  return plan;
}

function groupRoutes(candidates, allDerivedRoutes) {
  const groups = new Map();
  for (const candidate of candidates) {
    const key = candidate.routing?.phase ?? "unclassified";
    const group = groups.get(key) ?? {
      phase: key,
      candidateFiles: 0,
      families: new Set(),
      extractors: new Set(),
      proofSuites: new Set(),
      semanticCandidateRoutes: new Set(),
    };
    group.candidateFiles += 1;
    group.families.add(candidate.family);
    for (const value of candidate.routing?.extractors ?? []) group.extractors.add(value);
    for (const value of candidate.routing?.proofSuites ?? []) group.proofSuites.add(value);
    for (const value of candidate.routing?.semanticCandidateRoutes ?? []) {
      if (value === "all-derived-routes-until-row-diff") {
        for (const route of allDerivedRoutes) group.semanticCandidateRoutes.add(route);
      } else {
        group.semanticCandidateRoutes.add(value);
      }
    }
    groups.set(key, group);
  }
  return [...groups.values()].map((group) => ({
    phase: group.phase,
    candidateFiles: group.candidateFiles,
    families: [...group.families].sort(),
    extractors: [...group.extractors].sort(),
    proofSuites: [...group.proofSuites].sort(),
    semanticCandidateRoutes: [...group.semanticCandidateRoutes].sort(),
  })).sort((a, b) => a.phase.localeCompare(b.phase));
}

function classifyPath(value) {
  const lower = normalize(value).toLowerCase();
  if (lower.includes("/streamingassets/container/") && lower.endsWith(".pkg")) return "container_package";
  if (lower.endsWith("/streamingassets/container/meta.pkg")) return "container_index";
  if (lower.endsWith("/streamingassets/container/resources.meta3")) return "container_auxiliary";
  if (lower.endsWith("/gameassembly.dll")) return "il2cpp_native_code";
  if (lower.endsWith("/global-metadata.dat")) return "il2cpp_metadata";
  if (lower.endsWith(".assets") || lower.includes("globalgamemanagers")) return "unity_player_data";
  if (lower.includes("/anticheatexpert/")) return "anti_cheat";
  if (lower.endsWith(".exe") || lower.endsWith("/unityplayer.dll") || lower.endsWith("/baselib.dll")) return "native_executable";
  if (lower.endsWith(".dll")) return "native_plugin";
  if (lower.endsWith("boot.config") || lower.endsWith("files.meta3")) return "client_support";
  return "unclassified";
}

function assertPlan(plan) {
  if (plan.schemaVersion !== "rlogs.bpsr-patch-rescan-plan.v2") throw new Error("Unexpected plan schema.");
  if (!/^\d+$/.test(plan.depotTransition.previousManifestId) || !/^\d+$/.test(plan.depotTransition.currentManifestId)) {
    throw new Error("Plan manifest IDs lost integer precision.");
  }
  if (plan.summary.candidateFiles !== plan.candidates.length) throw new Error("Candidate count mismatch.");
  if (new Set(plan.candidates.map((candidate) => candidate.relativePath)).size !== plan.candidates.length) {
    throw new Error("Duplicate candidate path in plan.");
  }
  if (plan.summary.classifiedCandidates + plan.summary.unclassifiedCandidates !== plan.summary.candidateFiles) {
    throw new Error("Candidate classification is not conserved.");
  }
  if (plan.summary.physicalCandidateFiles !== plan.summary.candidateFiles
    || plan.summary.candidatePathDisagreements !== 0
    || plan.candidateReconciliation.manifestOnly.length !== 0
    || plan.candidateReconciliation.physicalDiffOnly.length !== 0) {
    throw new Error("Cached depot manifest and local SHA-256 candidate sets are not reconciled.");
  }
  if (plan.summary.fastPathReady !== (plan.summary.unclassifiedCandidates === 0)) {
    throw new Error("Fast-path readiness disagrees with unclassified candidate count.");
  }
  const schemaChanges = plan.decodedReferenceChanges?.exactFieldSchemaChanges ?? [];
  const expectedSchemaChanges = plan.summary.decodedAddedExactFieldSchemas
    + plan.summary.decodedRemovedExactFieldSchemas
    + plan.summary.decodedChangedExactFieldSchemas;
  if (schemaChanges.length !== expectedSchemaChanges) throw new Error("Exact field schema change count mismatch.");
  const semanticFieldChanges = plan.semanticFieldSchemaChanges;
  const expectedSemanticFieldChanges = semanticFieldChanges
    ? semanticFieldChanges.added.length + semanticFieldChanges.removed.length + semanticFieldChanges.changed.length
    : 0;
  if (expectedSemanticFieldChanges !== plan.summary.semanticFieldSchemaChanges) {
    throw new Error("Semantic field-schema change count mismatch.");
  }
  if (plan.summary.semanticFieldSchemaComparisonAvailable !== Boolean(semanticFieldChanges)) {
    throw new Error("Semantic field-schema comparison availability mismatch.");
  }
  const decodedFieldChanges = plan.decodedFieldSchemaChanges;
  const expectedDecodedFieldChanges = decodedFieldChanges
    ? decodedFieldChanges.added.length + decodedFieldChanges.removed.length + decodedFieldChanges.changed.length
    : 0;
  if (expectedDecodedFieldChanges !== plan.summary.decodedFieldSchemaChanges) {
    throw new Error("Decoded field-schema change count mismatch.");
  }
  if (plan.summary.decodedFieldSchemaComparisonAvailable !== Boolean(decodedFieldChanges)) {
    throw new Error("Decoded field-schema comparison availability mismatch.");
  }
  if ((decodedFieldChanges?.affectedTables.length ?? 0) !== plan.summary.focusedDecodedTables) {
    throw new Error("Decoded field-schema affected table count mismatch.");
  }
  const mechanicChanges = plan.mechanicDependencyChanges;
  const expectedMechanicChanges = mechanicChanges
    ? mechanicChanges.added.length + mechanicChanges.removed.length + mechanicChanges.changed.length
    : 0;
  if (expectedMechanicChanges !== plan.summary.changedMechanics
    || plan.summary.mechanicDependencyComparisonAvailable !== Boolean(mechanicChanges)
    || (mechanicChanges?.unchanged.length ?? 0) !== plan.summary.unchangedMechanics
    || (mechanicChanges?.changedDependencyTables.length ?? 0) !== plan.summary.changedMechanicDependencyTables) {
    throw new Error("Semantic mechanic dependency change counts are inconsistent.");
  }
}

function selfTest() {
  const currentGid = "6121914903992649526";
  const plan = buildPlan({
    manifestDiff: {
      previousManifest: { depotId: 1, manifestGid: "8164724872393010075" },
      currentManifest: { depotId: 1, manifestGid: currentGid },
      summary: { added: 0, removed: 1, changed: 1, unchanged: 1, candidateFiles: 2 },
      added: [],
      removed: [{ path: "bpsr/data/container/m2.pkg", size: 10 }],
      changed: [{ reasons: ["content-sha1"], previous: { path: "bpsr/GameAssembly.dll", size: 20 }, current: { path: "bpsr/GameAssembly.dll", size: 21 } }],
    },
    installedManifest: {
      game: "test", deployment: "test", channel: "test", gameBuild: 2,
      cachedDepotManifest: { manifestId: currentGid },
      coverage: { depotAuthoredFiles: 2, files: 2 },
      files: [{ relativePath: "bpsr/GameAssembly.dll", family: "il2cpp_native_code" }],
    },
    physicalDiff: {
      baseline_build_id: "1",
      build_id: "2",
      added: [],
      removed: [{ relative_path: "bpsr/data/container/m2.pkg", family: "container_package" }],
      changed: [{ relative_path: "bpsr/GameAssembly.dll", family: "il2cpp_native_code" }],
      unstable: [],
    },
    sourceManifest: { coverage: { filesHashed: 3 }, routeSummary: { combat: 2, scenes: 1 } },
    semanticDiff: { baselineBuild: 1, changedDomains: [{ domain: "combat", changedRows: ["x"], proofSuites: ["proof"] }], unchangedDomains: [{ domain: "scenes" }], missingManifests: [] },
    referenceGraphDiff: {
      baseline_build: "1", candidate_build: "2",
      summary: {
        changed_tables: 1, added_rows: 1, removed_rows: 0, changed_rows: 0,
        added_exact_edges: 1, removed_exact_edges: 0,
        added_exact_field_schemas: 1, removed_exact_field_schemas: 0, changed_exact_field_schemas: 0,
        changed_current_build_callsite_proofs: 1, callsite_proof_inputs_changed: true,
      },
      affected_domains: [{ domain: "skills-actions", status: "changed", changed_tables: ["SkillTable"], baseline_rows: 1, candidate_rows: 2 }],
      table_changes: [], exact_edge_changes: { added: [], removed: [] }, missing_target_changes: { added: [], removed: [] },
      semantic_field_changes: [],
      exact_field_schema_changes: [{ source_table: "SkillTable", domain: "skills-actions", field: "OwnerId", status: "added", current_build_proof_changed: true }],
      callsite_proof_changes: { changed: true },
      ambiguous_field_changes: [],
    },
    semanticFieldDiff: {
      baseline_build: "1", candidate_build: "2",
      added: [{ key: "SkillTable/NewId/$[].NewId", source_table: "SkillTable", field: "NewId" }],
      removed: [],
      changed: [{ key: "BuffTable/EffectId/$[].EffectId", candidate: { source_table: "BuffTable", field: "EffectId" } }],
    },
    decodedFieldDiff: {
      baseline_build: "1", candidate_build: "2",
      summary: { added: 1, removed: 0, changed: 1, affected_tables: 2 },
      affected_tables: ["BuffTable", "SkillTable"],
      added: [{ key: "SkillTable/NewRatio", source_table: "SkillTable", path_pattern: "/NewRatio" }],
      removed: [],
      changed: [{ key: "BuffTable/Duration", candidate: { source_table: "BuffTable", path_pattern: "/Duration" } }],
    },
    mechanicDependencyDiff: {
      baseline_build: "1", candidate_build: "2",
      added: [], removed: [],
      changed: [{ source_id: "buff-source:42", changed_components: ["decoded_rows"] }],
      unchanged: [{ source_id: "talent:7" }],
      changed_dependency_tables: ["BuffTable"],
    },
    inputs: {},
  });
  if (!plan.summary.fastPathReady || plan.summary.candidateFiles !== 2
    || plan.summary.changedReferenceProofDomains !== 1
    || plan.summary.semanticFieldSchemaChanges !== 2
    || plan.summary.decodedFieldSchemaChanges !== 2
    || plan.summary.focusedDecodedTables !== 2
    || plan.summary.changedMechanics !== 1
    || plan.summary.unchangedMechanics !== 1
    || plan.summary.changedMechanicDependencyTables !== 1
    || plan.summary.decodedChangedCurrentBuildCallsiteProofs !== 1
    || plan.summary.decodedCallsiteProofInputsChanged !== true) throw new Error("Self-test failed.");
  console.log("bpsr-patch-rescan-plan self-test passed");
}

function countBy(values, keyOf) {
  const counts = {};
  for (const value of values) {
    const key = keyOf(value);
    counts[key] = (counts[key] ?? 0) + 1;
  }
  return Object.fromEntries(Object.entries(counts).sort(([a], [b]) => a.localeCompare(b)));
}

function normalize(value) {
  return String(value).replaceAll("\\", "/").replace(/^\.\//, "");
}

function evidence(filePath) {
  const bytes = readFileSync(filePath);
  return {
    file: path.basename(filePath),
    bytes: bytes.length,
    sha256: createHash("sha256").update(bytes).digest("hex"),
  };
}

function readJson(filePath) {
  if (!existsSync(filePath)) throw new Error(`Missing input: ${filePath}`);
  return JSON.parse(readFileSync(filePath, "utf8"));
}

function parseOptions(args) {
  const options = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index];
    if (!key?.startsWith("--") || args[index + 1] === undefined) throw new Error(`Invalid option near ${key ?? "end of command"}.`);
    options[key.slice(2)] = args[index + 1];
  }
  return options;
}

function required(options, key) {
  const value = options[key];
  if (!value) throw new Error(`Missing --${key}.`);
  return value;
}

function summaryLine(plan) {
  return `BPSR ${plan.baselineBuild} -> ${plan.candidateBuild}: ${plan.summary.candidateFiles} manifest candidates, ${plan.summary.classifiedCandidates} classified, ${plan.summary.focusedSemanticDomains} focused semantic/proof domains, ${plan.summary.decodedChangedCurrentBuildCallsiteProofs} changed callsite proofs, fast path ${plan.summary.fastPathReady ? "ready" : "blocked"}.`;
}

function usage() {
  console.log("Usage:");
  console.log("  node tools/bpsr-patch-rescan-plan.mjs generate --manifest-diff <json> --installed-manifest <json> --physical-diff <json> --source-manifest <json> --semantic-diff <json> [--reference-graph-diff <json>] [--semantic-field-diff <json>] [--decoded-field-diff <json>] [--mechanic-dependency-diff <json>] --output <json>");
  console.log("  node tools/bpsr-patch-rescan-plan.mjs verify --input <json>");
  console.log("  node tools/bpsr-patch-rescan-plan.mjs self-test");
}
