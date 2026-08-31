#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, "..");
const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "generate") generate(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function generate(options) {
  const buildRoot = resolvePath(required(options, "build-root"));
  const referenceGraphPath = resolvePath(required(options, "reference-graph"));
  const semanticFieldSchemaPath = resolvePath(
    options["semantic-field-schema"]
      ?? path.join(path.dirname(referenceGraphPath), "DecodedTableReferenceGraph.semantic-field-schema.v1.json"),
  );
  const decodedFieldSchemaPath = resolvePath(
    options["decoded-field-schema"]
      ?? path.join(path.dirname(referenceGraphPath), "DecodedTableReferenceGraph.decoded-field-schema.v1.json"),
  );
  const outputPath = resolvePath(options.output ?? path.join(buildRoot, "current-build-mapping-completeness.v1.json"));
  const inputs = loadInputs(buildRoot, referenceGraphPath, semanticFieldSchemaPath, decodedFieldSchemaPath);
  const report = buildReport(inputs, buildRoot);
  writeJson(outputPath, report);

  console.log(`Current build ${report.game_build} mapping inventory complete.`);
  console.log(`Static definitions: ${report.summary.static_domains_complete}/${report.summary.static_domains_total} domains complete.`);
  console.log(`Exact relationship blockers: ${report.summary.exact_relationship_blockers}.`);
  console.log(`Semantic mapping findings: ${report.summary.semantic_mapping_findings}.`);
  console.log(`Runtime observation backlog: ${report.summary.runtime_observation_backlog}.`);
  console.log(`Runtime proof gates: ${report.summary.runtime_proof_gates}.`);
  console.log(`Protocol blockers: ${report.summary.protocol_blockers}.`);
  console.log(`Wrote ${relativeRepo(outputPath)}`);
}

function loadInputs(buildRoot, referenceGraphPath, semanticFieldSchemaPath, decodedFieldSchemaPath) {
  const paths = {
    distribution: "steam-distribution-snapshot.v1.json",
    installedClient: "installed-client-file-manifest.v1.json",
    sourceManifest: "complete-build-source-manifest.v1.json",
    ctbDiff: "ctb-build-diff-v2.json",
    semanticDiff: "extractor-semantic-diff.v1.json",
    coverage: "combat-domain-coverage-audit.v1.json",
    damageLedger: "damage-resolution-ledger.v2.json",
    scriptFamilies: "damage-script-family-worklist.v6.json",
    routeProof: "damage-source-route-proof.candidate.v9.json",
    formulaLedger: "formula-magnitude-gap-ledger.v11.json",
    scopeLedger: "rdps-recipient-scope-ledger.v2.json",
    preflight: "rdps-build-preflight.v3.json",
    carryForward: "formula-proof-carry-forward.v2.json",
    seasonal: "seasonal-domains/index.v1.json",
    equipmentReachability: "equipment-set-child-buff-reachability.v1.json",
    effectActivation: "effect-activation-ledger.v1.json",
    damageActivation: "unrouted-damage-activation-ledger.v1.json",
    protocol: "protocol-decode-recordings-v2/protocol-pack-promotion-audit.v2.json",
    staticAudit: "static-rdps-semantic-audit.json",
    semanticRefresh: "current-build-semantic-refresh.v1.json",
    unmappedCatalog: "current-build-unmapped-catalog.v1.json",
  };
  const inputs = Object.fromEntries(Object.entries(paths).map(([key, relativePath]) => {
    const filePath = path.join(buildRoot, relativePath);
    if (!existsSync(filePath)) throw new Error(`Missing completeness input ${relativePath}`);
    return [key, { path: filePath, value: readJson(filePath) }];
  }));
  if (!existsSync(referenceGraphPath)) throw new Error(`Missing decoded reference graph ${referenceGraphPath}`);
  inputs.referenceGraph = { path: referenceGraphPath, value: readJson(referenceGraphPath) };
  if (!existsSync(semanticFieldSchemaPath)) throw new Error(`Missing semantic field-schema ledger ${semanticFieldSchemaPath}`);
  inputs.semanticFieldSchema = { path: semanticFieldSchemaPath, value: readJson(semanticFieldSchemaPath) };
  if (!existsSync(decodedFieldSchemaPath)) throw new Error(`Missing decoded field-schema manifest ${decodedFieldSchemaPath}`);
  inputs.decodedFieldSchema = { path: decodedFieldSchemaPath, value: readJson(decodedFieldSchemaPath) };
  return inputs;
}

function buildReport(inputs, buildRoot) {
  const value = Object.fromEntries(Object.entries(inputs).map(([key, input]) => [key, input.value]));
  const buildId = String(value.distribution.app.buildId);
  validateBuildIdentity(value, buildId, inputs);

  const semanticFieldSummary = value.semanticFieldSchema.summary;
  const decodedFieldSummary = value.decodedFieldSchema.summary;
  const dormantSemanticFieldGroups = Number(
    semanticFieldSummary.evidence_states?.["dormant-zero-only-identifier"] ?? 0,
  );
  const activeOpenSemanticFieldGroups = Number(semanticFieldSummary.open_field_groups) - dormantSemanticFieldGroups;

  const coverage = value.coverage;
  const seasonalDomains = value.seasonal.domains.map((domain) => ({
    domain: domain.domain,
    rows: domain.rowCount,
    sources: domain.sourceCount,
    missing_required_inputs: domain.missingRequiredCount,
    missing_optional_inputs: domain.missingOptionalCount,
    aggregate_sha256: domain.aggregateSha256,
    complete: domain.missingRequiredCount === 0,
  }));
  const relationshipGaps = {
    damage_rows_without_explicit_action_parent: idsFrom(coverage.worklists?.damage_rows_without_explicit_action_parent),
    damage_packet_observed_or_currently_referenced_without_static_source_route: value.damageActivation.entries
      .filter((entry) => entry.blocks_exact_current_build_relationship === true)
      .map((entry) => String(entry.lookup_key))
      .sort(),
    damage_route_keys_with_overlapping_sources: value.routeProof.summary.keys_with_overlapping_source_routes,
    nonstandard_or_missing_formula_candidates: value.damageLedger.summary.nonstandard_or_missing_formula_candidates,
    script_family_candidates_without_static_route: value.scriptFamilies.summary.candidates_without_static_route,
    equipment_child_buff_packet_observed_without_current_build_relationship_proof: value.effectActivation.effects
      .filter((entry) => entry.blocks_exact_current_build_relationship === true)
      .map((entry) => Number(entry.effect_id))
      .sort((a, b) => a - b),
    decoded_declared_reference_targets_missing: value.referenceGraph.missing_targets,
  };
  const preservedDormantDefinitions = {
    equipment_child_buff_definition_only_unobserved: value.effectActivation.effects
      .filter((entry) => entry.activation_status === "definition-only-unobserved-in-indexed-packet-corpus")
      .map((entry) => Number(entry.effect_id))
      .sort((a, b) => a - b),
    damage_definition_only_unobserved: value.damageActivation.entries
      .filter((entry) => entry.activation_status === "definition-only-unobserved-in-indexed-packet-corpus")
      .map((entry) => String(entry.lookup_key))
      .sort(),
    policy: "Current-build definitions with no incoming client reference and no indexed packet observation remain visible and diffable, but do not block an active exact relationship or feed runtime rDPS.",
  };
  const clientRecountGroupingReviews = {
    partial_action_ids: idsFrom(coverage.worklists?.client_recount_partial_reviews ?? coverage.worklists?.partial_action_recounts),
    ambiguous_action_ids: idsFrom(coverage.worklists?.client_recount_ambiguous_reviews ?? coverage.worklists?.ambiguous_action_recounts),
    policy: "DamageAttr.LinkedId is the exact action parent. Client RecountTable grouping is preserved independently as presentation and aggregation evidence.",
  };

  const runtimeGaps = {
    formula_candidates_without_matching_packet_observations: value.formulaLedger.summary.candidates_without_packet_observations,
    formula_candidates_eligible_for_current_build_promotion: value.formulaLedger.summary.candidates_eligible_for_current_build_promotion,
    recipient_scope_candidates_eligible_for_current_build_promotion: value.scopeLedger.summary.candidates_eligible_for_current_build_promotion,
    unresolved_provider_recipient_candidates: value.scopeLedger.summary.effective_transfer_eligibilities["recipient-scope-unresolved"] ?? 0,
    matching_build_damage_coefficient_proof_present: value.preflight.inputs.some((entry) => entry.id === "build-locked-damage-attribute-coefficient-evidence" && entry.status === "present"),
    matching_build_healing_scaling_proof_present: value.preflight.inputs.some((entry) => entry.id === "build-locked-healing-state-scaling-evidence" && entry.status === "present"),
  };
  const semanticMappingGaps = {
    candidates_audited: value.staticAudit.summary.candidates_audited,
    candidates_with_exact_effect_source: value.staticAudit.summary.candidates_with_effect_source,
    candidates_without_exact_effect_source: value.staticAudit.summary.candidates_without_effect_source,
    candidates_with_findings: value.staticAudit.summary.candidates_with_findings,
    promotion_blocked_candidates: value.staticAudit.summary.promotion_blocked_candidates,
    findings_by_category: value.staticAudit.summary.findings_by_category,
    findings: value.staticAudit.findings.map((finding) => ({
      source_rule_id: finding.source_rule_id,
      source_id: finding.source_id,
      source_name: finding.source_name,
      categories: [...new Set(finding.issues.map((issue) => issue.category))].sort(),
    })),
  };

  const presentationGaps = {
    skills_without_user_facing_english: coverage.skills_and_actions.design_only_or_missing_english,
    effect_sources_without_user_facing_english: coverage.buffs_and_effects.effect_sources_missing_user_facing_english,
    equipment_effect_sources_without_user_facing_english: coverage.equipment_set_effects.missing_user_facing_english,
    policy: "Presentation gaps remain visible as IDs and do not erase or rename canonical mechanics evidence.",
  };

  const decodedMissingMechanicsTargets = relationshipGaps.decoded_declared_reference_targets_missing
    .filter((entry) => entry.blocks_mechanics !== false);
  const decodedMissingPresentationTargets = relationshipGaps.decoded_declared_reference_targets_missing
    .filter((entry) => entry.blocks_mechanics === false);

  const exactRelationshipBlockers = relationshipGaps.damage_rows_without_explicit_action_parent.length
    + relationshipGaps.damage_packet_observed_or_currently_referenced_without_static_source_route.length
    + relationshipGaps.equipment_child_buff_packet_observed_without_current_build_relationship_proof.length
    + decodedMissingMechanicsTargets.length;
  const runtimeProofBlockers = runtimeGaps.formula_candidates_without_matching_packet_observations
    + runtimeGaps.unresolved_provider_recipient_candidates
    + Number(!runtimeGaps.matching_build_damage_coefficient_proof_present)
    + Number(!runtimeGaps.matching_build_healing_scaling_proof_present);
  const runtimeProofGates = runtimeGaps.unresolved_provider_recipient_candidates
    + Number(!runtimeGaps.matching_build_damage_coefficient_proof_present)
    + Number(!runtimeGaps.matching_build_healing_scaling_proof_present);

  return {
    schema_version: 3,
    generated_by: "tools/bpsr-current-build-completeness.mjs",
    game: "blue-protocol-star-resonance",
    deployment: "global",
    channel: "steam",
    game_build: buildId,
    distribution_identity: {
      app_id: value.distribution.app.appId,
      build_id: buildId,
      depots: value.distribution.installedDepots,
      routing_fingerprint_sha256: value.distribution.routingFingerprintSha256,
    },
    policy: {
      steam_manifest_is_change_detector_not_semantic_authority: true,
      current_client_files_and_packet_evidence_are_semantic_authority: true,
      every_unresolved_definition_or_relationship_remains_visible: true,
      dormant_definitions_are_preserved_but_not_counted_as_active_relationship_blockers: true,
      client_recount_grouping_is_independent_from_exact_damage_action_parent: true,
      localization_gaps_are_not_mechanics_gaps: true,
      static_identity_does_not_substitute_for_runtime_formula_proof: true,
      unchanged_domain_hashes_may_skip_reextraction_but_not_missing_runtime_proof: true,
      every_decoded_field_path_is_profiled_and_patch_diffed: true,
      field_name_routing_is_not_semantic_or_formula_proof: true,
      runtime_rdps_promotion_is_fail_closed: true,
    },
    summary: {
      installed_client_files_discovered: value.installedClient.coverage.physicalFilesDiscovered,
      installed_client_files_hashed: value.installedClient.coverage.physicalFilesHashed,
      installed_client_bytes_hashed: value.installedClient.coverage.physicalBytesHashed,
      installed_depot_authored_files: value.installedClient.coverage.depotAuthoredFiles,
      installed_depot_authored_bytes: value.installedClient.coverage.depotAuthoredBytes,
      installed_client_generated_volatile_files: value.installedClient.coverage.clientGeneratedVolatileFiles,
      installed_client_manifest_complete: value.installedClient.coverage.complete
        && value.installedClient.coverage.silentOmissions === 0
        && value.installedClient.coverage.unstableFiles === 0,
      derived_source_files_discovered: value.sourceManifest.coverage.filesDiscovered,
      derived_source_files_hashed: value.sourceManifest.coverage.filesHashed,
      derived_source_bytes_hashed: value.sourceManifest.coverage.bytesHashed,
      derived_source_manifest_complete: value.sourceManifest.coverage.complete
        && value.sourceManifest.coverage.silentOmissions === 0,
      // Compatibility aliases retained for existing report readers. These refer
      // only to decoded/generated sources, never the installed client tree.
      source_files_discovered: value.sourceManifest.coverage.filesDiscovered,
      source_files_hashed: value.sourceManifest.coverage.filesHashed,
      source_bytes_hashed: value.sourceManifest.coverage.bytesHashed,
      source_manifest_complete: value.sourceManifest.coverage.complete
        && value.sourceManifest.coverage.silentOmissions === 0,
      static_domains_total: seasonalDomains.length,
      static_domains_complete: seasonalDomains.filter((domain) => domain.complete).length,
      exact_relationship_blockers: exactRelationshipBlockers,
      semantic_mapping_findings: semanticMappingGaps.candidates_with_findings,
      formula_magnitude_findings: semanticMappingGaps.findings_by_category["formula-magnitude-unresolved"] ?? 0,
      recipient_scope_findings: semanticMappingGaps.findings_by_category["formula-recipient-scope-unresolved"] ?? 0,
      produced_damage_row_findings: semanticMappingGaps.findings_by_category["produced-damage-without-packet-row"] ?? 0,
      runtime_observation_backlog: runtimeGaps.formula_candidates_without_matching_packet_observations,
      runtime_proof_gates: runtimeProofGates,
      // Compatibility total retained for older readers. It combines the
      // observation backlog with actual proof gates and must not be presented
      // as a count of unmapped client definitions.
      runtime_proof_blockers: runtimeProofBlockers,
      protocol_blockers: value.protocol.blockers.length,
      catalog_open_items: value.unmappedCatalog.summary.open_catalog_entries,
      catalog_mechanics_findings: value.unmappedCatalog.summary.by_blocking_class["mechanics-blocker"] ?? 0,
      catalog_runtime_observation_rows: value.unmappedCatalog.summary.by_blocking_class["runtime-observation"] ?? 0,
      catalog_runtime_proof_gates: value.unmappedCatalog.summary.by_blocking_class["runtime-proof"] ?? 0,
      catalog_presentation_rows: value.unmappedCatalog.summary.by_blocking_class.presentation ?? 0,
      catalog_dormant_definitions: value.unmappedCatalog.summary.by_blocking_class["dormant-definition"] ?? 0,
      catalog_review_only_rows: value.unmappedCatalog.summary.by_blocking_class["review-only"] ?? 0,
      catalog_field_semantic_reviews: value.unmappedCatalog.summary.by_blocking_class["field-semantic-review"] ?? 0,
      decoded_rows_inventoried: value.referenceGraph.summary.decoded_rows,
      decoded_exact_relationship_edges: value.referenceGraph.summary.exact_edges,
      decoded_exact_field_schemas: value.referenceGraph.summary.exact_field_schemas,
      decoded_current_build_callsite_proven_field_schemas:
        value.referenceGraph.summary.current_build_callsite_proven_field_schemas,
      decoded_exact_missing_targets: value.referenceGraph.summary.exact_edges_with_missing_target,
      decoded_exact_missing_mechanics_targets: decodedMissingMechanicsTargets.length,
      decoded_exact_missing_presentation_targets: decodedMissingPresentationTargets.length,
      decoded_semantic_field_groups: semanticFieldSummary.semantic_field_groups,
      decoded_semantic_field_groups_closed: semanticFieldSummary.closed_field_groups,
      decoded_semantic_field_groups_open: semanticFieldSummary.open_field_groups,
      decoded_semantic_field_evidence_states: semanticFieldSummary.evidence_states,
      decoded_unproven_reference_field_groups: activeOpenSemanticFieldGroups,
      decoded_dormant_semantic_field_groups: dormantSemanticFieldGroups,
      decoded_raw_reference_like_field_groups: value.referenceGraph.summary.ambiguous_reference_field_groups,
      decoded_unproven_reference_occurrences: value.referenceGraph.summary.ambiguous_reference_occurrences,
      decoded_reference_candidate_field_groups: value.referenceGraph.summary.reference_candidate_field_groups,
      decoded_reference_candidate_full_coverage_field_groups: value.referenceGraph.summary.reference_candidate_full_coverage_field_groups,
      decoded_reference_candidate_zero_only_field_groups: value.referenceGraph.summary.reference_candidate_zero_only_field_groups,
      decoded_field_paths: decodedFieldSummary.decoded_field_paths,
      decoded_scalar_field_paths: decodedFieldSummary.scalar_field_paths,
      decoded_array_field_paths: decodedFieldSummary.array_field_paths,
      decoded_object_field_paths: decodedFieldSummary.object_field_paths,
      decoded_mechanics_sensitive_field_paths: decodedFieldSummary.mechanics_sensitive_field_paths,
      decoded_field_structural_inventory_complete: decodedFieldSummary.structural_inventory_complete,
      presentation_gaps: presentationGaps.skills_without_user_facing_english
        + presentationGaps.effect_sources_without_user_facing_english
        + presentationGaps.equipment_effect_sources_without_user_facing_english,
      current_build_static_inventory_complete: seasonalDomains.every((domain) => domain.complete)
        && value.installedClient.coverage.complete === true
        && value.installedClient.coverage.silentOmissions === 0
        && value.installedClient.coverage.unstableFiles === 0
        && value.sourceManifest.coverage.complete === true
        && value.sourceManifest.coverage.silentOmissions === 0
        && decodedFieldSummary.structural_inventory_complete === true,
      current_build_reference_inventory_complete: value.unmappedCatalog.summary.current_build_reference_inventory_complete === true,
      current_build_semantic_reference_mapping_complete: value.unmappedCatalog.summary.current_build_semantic_reference_mapping_complete === true,
      current_build_mechanics_field_semantics_complete: value.unmappedCatalog.summary.current_build_mechanics_field_semantics_complete === true,
      current_build_runtime_rdps_complete: false,
      protocol_pack_promotable: value.protocol.promotion_ready,
    },
    patch_diff: {
      installed_client: {
        aggregate_sha256: value.installedClient.aggregateSha256,
        cached_depot_manifest: value.installedClient.cachedDepotManifest,
        families: value.installedClient.families,
        coverage: value.installedClient.coverage,
      },
      complete_source_manifest: {
        aggregate_sha256: value.sourceManifest.aggregateSha256,
        roots: value.sourceManifest.roots,
        routes: value.sourceManifest.routeSummary,
        related_routes: value.sourceManifest.relatedRouteSummary,
        coverage: value.sourceManifest.coverage,
      },
      decoded_tables: value.ctbDiff.summary,
      generated_semantics: value.semanticDiff.summary,
      seasonal_domains: seasonalDomains,
      decoded_reference_graph: {
        aggregate_domains: value.referenceGraph.domains,
        decoded_rows: value.referenceGraph.summary.decoded_rows,
        exact_edges: value.referenceGraph.summary.exact_edges,
        exact_field_schemas: value.referenceGraph.summary.exact_field_schemas,
        current_build_callsite_proven_field_schemas:
          value.referenceGraph.summary.current_build_callsite_proven_field_schemas,
        callsite_proof_artifact: callsiteProofMetadata(inputs.referenceGraph),
        exact_missing_targets: value.referenceGraph.summary.exact_edges_with_missing_target,
        exact_missing_mechanics_targets: decodedMissingMechanicsTargets.length,
        exact_missing_presentation_targets: decodedMissingPresentationTargets.length,
        raw_reference_like_field_groups: value.referenceGraph.summary.ambiguous_reference_field_groups,
        unproven_reference_occurrences: value.referenceGraph.summary.ambiguous_reference_occurrences,
        candidate_field_groups: value.referenceGraph.summary.reference_candidate_field_groups,
        candidate_full_coverage_field_groups: value.referenceGraph.summary.reference_candidate_full_coverage_field_groups,
        candidate_zero_only_field_groups: value.referenceGraph.summary.reference_candidate_zero_only_field_groups,
        semantic_field_schema: semanticFieldSchemaMetadata(inputs.semanticFieldSchema),
        semantic_field_groups: semanticFieldSummary.semantic_field_groups,
        closed_semantic_field_groups: semanticFieldSummary.closed_field_groups,
        active_open_semantic_field_groups: activeOpenSemanticFieldGroups,
        dormant_open_semantic_field_groups: dormantSemanticFieldGroups,
      },
      decoded_field_schema: decodedFieldSchemaMetadata(inputs.decodedFieldSchema),
    },
    static_inventory: {
      skills_and_actions: coverage.skills_and_actions,
      damage_actions: coverage.damage_actions,
      recount: coverage.recount,
      talents: coverage.talents,
      buffs_and_effects: coverage.buffs_and_effects,
      equipment_set_effects: coverage.equipment_set_effects,
      deep_slumber_psychoscope: coverage.deep_slumber_psychoscope,
      decoded_reference_inventory: {
        graph: relativeRepo(inputs.referenceGraph.path),
        summary: value.referenceGraph.summary,
        ambiguous_occurrence_artifact: referenceOccurrenceMetadata(inputs.referenceGraph),
        reference_candidate_artifact: referenceCandidateMetadata(inputs.referenceGraph),
        callsite_proof_artifact: callsiteProofMetadata(inputs.referenceGraph),
        semantic_field_schema_artifact: semanticFieldSchemaMetadata(inputs.semanticFieldSchema),
        decoded_field_schema_artifact: decodedFieldSchemaMetadata(inputs.decodedFieldSchema),
      },
    },
    exact_relationship_gaps: relationshipGaps,
    semantic_mapping_gaps: semanticMappingGaps,
    preserved_dormant_definitions: preservedDormantDefinitions,
    client_recount_grouping_reviews: clientRecountGroupingReviews,
    runtime_proof_gaps: runtimeGaps,
    protocol_state: {
      exact_world_service_id: value.protocol.exact_world_service_id,
      observed_exact_world_route_count: value.protocol.observed_exact_world_route_count,
      migrated_decoder_route_count: value.protocol.migrated_decoder_route_count,
      validated_migrated_decoder_route_count: value.protocol.validated_migrated_decoder_route_count,
      capture_gap_count: value.protocol.capture_gap_count,
      promotion_ready: value.protocol.promotion_ready,
      blockers: value.protocol.blockers,
    },
    presentation_gaps: presentationGaps,
    unmapped_catalog: {
      artifact: relativeRepo(inputs.unmappedCatalog.path),
      summary: value.unmappedCatalog.summary,
      shards: value.unmappedCatalog.shards,
    },
    carry_forward_state: {
      artifact: relativeRepo(inputs.carryForward.path),
      retained_historical_proofs: value.carryForward.proofs.length,
      runtime_enabled_proofs: value.carryForward.proofs.filter((proof) => proof.current_build_runtime_enabled === true).length,
      blockers: value.carryForward.promotion_blockers,
    },
    preflight: {
      artifact: relativeRepo(inputs.preflight.path),
      summary: value.preflight.summary,
      missing: value.preflight.inputs.filter((entry) => entry.status !== "present").map((entry) => ({
        id: entry.id,
        required: entry.required,
        role: entry.role,
      })),
      ready_for_snapshot: value.preflight.ready_for_snapshot,
      runtime_promotion_allowed: value.preflight.runtime_promotion_allowed,
    },
    inputs: Object.fromEntries(Object.entries(inputs).map(([key, input]) => [key, relativeRepo(input.path)])),
    build_root: relativeRepo(buildRoot),
  };
}

function validateBuildIdentity(value, buildId, inputs) {
  const buildFields = [
    value.installedClient.gameBuild,
    value.coverage.client_build,
    value.sourceManifest.gameBuild,
    value.damageLedger.game_build,
    value.scriptFamilies.game_build,
    value.routeProof.game_build,
    value.formulaLedger.static_game_build,
    value.scopeLedger.static_game_build,
    value.preflight.game_build,
    value.carryForward.build_id,
    value.seasonal.gameBuild,
    value.protocol.build_id,
    value.equipmentReachability.gameBuild,
    value.effectActivation.game_build,
    value.damageActivation.game_build,
    value.staticAudit.game_build,
    value.semanticRefresh.game_build,
    value.unmappedCatalog.game_build,
    value.referenceGraph.game_build,
    value.semanticFieldSchema.game_build,
    value.decodedFieldSchema.game_build,
  ];
  for (const observed of buildFields) assert(String(observed) === buildId, `Build identity mismatch: expected ${buildId}, got ${observed}`);
  assert(value.seasonal.missingRequiredInputs.length === 0, "Seasonal-domain inventory has missing required inputs");
  assert(value.installedClient.coverage.complete === true, "Installed-client manifest is not complete");
  assert(value.installedClient.coverage.silentOmissions === 0, "Installed-client manifest has silent omissions");
  assert(value.installedClient.coverage.unstableFiles === 0, "Installed-client manifest has unstable files");
  assert(value.installedClient.coverage.depotAuthoredBytes === value.installedClient.coverage.installedDepotBytesExpected, "Installed-client depot byte coverage is incomplete");
  assert(value.sourceManifest.coverage.complete === true, "Complete source manifest is not complete");
  assert(value.sourceManifest.coverage.silentOmissions === 0, "Complete source manifest has silent omissions");
  assert(value.semanticRefresh.generated_by === "tools/bpsr-current-build-semantic-refresh.mjs", "Semantic refresh report has an unexpected generator");
  assert(value.semanticRefresh.summary.hidden_omissions === 0, "Semantic refresh report contains hidden omissions");
  validateSemanticRefreshArtifacts(value.semanticRefresh);
  assert(value.equipmentReachability.summary.effectIds === Object.keys(value.equipmentReachability.effectsById).length, "Equipment reachability summary disagrees with entries");
  assert(value.effectActivation.summary.effects === value.effectActivation.effects.length, "Effect activation summary disagrees with entries");
  assert(value.damageActivation.summary.unresolved_static_route_definitions === value.damageActivation.entries.length, "Damage activation summary disagrees with entries");
  assert(value.unmappedCatalog.summary.by_blocking_class["mechanics-blocker"] === value.staticAudit.summary.candidates_with_findings, "Unmapped catalog mechanics findings disagree with semantic audit");
  assert(value.unmappedCatalog.summary.by_blocking_class["runtime-observation"] === value.formulaLedger.summary.candidates_without_packet_observations, "Unmapped catalog observation backlog disagrees with formula ledger");
  assert(value.unmappedCatalog.summary.by_blocking_class.protocol === value.protocol.blockers.length, "Unmapped catalog protocol blockers disagree with protocol audit");
  assert(value.referenceGraph.generated_by === "DecodedTableReferenceGraph.gen", "Decoded reference graph has an unexpected generator");
  const graphMechanicsReferenceGaps = value.referenceGraph.missing_targets
    .filter((entry) => entry.blocks_mechanics !== false).length;
  const graphPresentationReferenceGaps = value.referenceGraph.missing_targets
    .filter((entry) => entry.blocks_mechanics === false).length;
  const catalogMechanicsReferenceGaps = value.unmappedCatalog.summary.by_blocking_class["static-reference-gap"] ?? 0;
  const catalogPresentationReferenceGaps = value.unmappedCatalog.summary.by_blocking_class["presentation-localization-gap"] ?? 0;
  assert(
    catalogMechanicsReferenceGaps + catalogPresentationReferenceGaps
      === value.referenceGraph.summary.exact_edges_with_missing_target,
    "Unmapped catalog exact reference gaps disagree with decoded graph",
  );
  assert(catalogMechanicsReferenceGaps === graphMechanicsReferenceGaps, "Unmapped catalog mechanics reference gaps disagree with decoded graph");
  assert(catalogPresentationReferenceGaps === graphPresentationReferenceGaps, "Unmapped catalog presentation reference gaps disagree with decoded graph");
  assert(value.semanticFieldSchema.generated_by === "tools/bpsr-semantic-field-schema-ledger.mjs", "Semantic field-schema ledger has an unexpected generator");
  assert(value.semanticFieldSchema.fields.length === value.semanticFieldSchema.summary.semantic_field_groups, "Semantic field-schema ledger row count mismatch");
  const semanticOpenFields = value.semanticFieldSchema.fields.filter((entry) => entry.resolution_state === "open");
  const dormantSemanticFields = semanticOpenFields.filter((entry) => entry.evidence_state === "dormant-zero-only-identifier");
  const activeSemanticFields = semanticOpenFields.filter((entry) => entry.evidence_state !== "dormant-zero-only-identifier");
  assert(semanticOpenFields.length === value.semanticFieldSchema.summary.open_field_groups, "Semantic field-schema ledger open count mismatch");
  assert(
    (value.unmappedCatalog.summary.by_blocking_class["reference-review"] ?? 0) === activeSemanticFields.length,
    "Unmapped catalog active semantic reference reviews disagree with field-schema ledger",
  );
  assert(
    (value.unmappedCatalog.summary.by_category["dormant-semantic-fields"] ?? 0) === dormantSemanticFields.length,
    "Unmapped catalog dormant semantic fields disagree with field-schema ledger",
  );
  assert(value.decodedFieldSchema.generated_by === "tools/bpsr-decoded-field-schema-manifest.mjs", "Decoded field-schema manifest has an unexpected generator");
  assert(value.decodedFieldSchema.fields.length === value.decodedFieldSchema.summary.decoded_field_paths, "Decoded field-schema manifest row count mismatch");
  assert(value.decodedFieldSchema.summary.structural_inventory_complete === true, "Decoded field-schema structural inventory is incomplete");
  assert(
    (value.unmappedCatalog.summary.by_blocking_class["field-semantic-review"] ?? 0)
      === value.decodedFieldSchema.summary.mechanics_sensitive_field_paths,
    "Unmapped catalog mechanics field reviews disagree with decoded field-schema manifest",
  );
  validateReferenceOccurrenceArtifact(inputs.referenceGraph);
  validateReferenceCandidateArtifact(inputs.referenceGraph);
  validateCallsiteProofArtifact(inputs.referenceGraph);
}

function validateReferenceOccurrenceArtifact(input) {
  const graph = input.value;
  const artifact = graph.ambiguous_reference_occurrence_artifact;
  assert(artifact && artifact.path, "Decoded reference graph has no ambiguous occurrence artifact");
  const filePath = path.resolve(path.dirname(input.path), artifact.path);
  assert(existsSync(filePath), `Decoded reference occurrence artifact is missing: ${filePath}`);
  assert(statSync(filePath).size === Number(artifact.bytes), "Decoded reference occurrence byte count mismatch");
  assert(sha256File(filePath) === artifact.sha256, "Decoded reference occurrence artifact hash mismatch");
  assert(Number(artifact.rows) === Number(graph.summary.ambiguous_reference_occurrences), "Decoded reference occurrence row count mismatch");
}

function referenceOccurrenceMetadata(input) {
  const artifact = input.value.ambiguous_reference_occurrence_artifact;
  return {
    path: relativeRepo(path.resolve(path.dirname(input.path), artifact.path)),
    rows: artifact.rows,
    bytes: artifact.bytes,
    sha256: artifact.sha256,
    format: artifact.format,
  };
}

function validateReferenceCandidateArtifact(input) {
  const graph = input.value;
  const artifact = graph.reference_candidate_artifact;
  assert(Number(graph.schema_version) >= 3, "Decoded reference graph does not provide the schema-v3 candidate ledger");
  assert(artifact && artifact.path, "Decoded reference graph has no reference candidate artifact");
  const filePath = path.resolve(path.dirname(input.path), artifact.path);
  assert(existsSync(filePath), `Decoded reference candidate artifact is missing: ${filePath}`);
  assert(statSync(filePath).size === Number(artifact.bytes), "Decoded reference candidate byte count mismatch");
  assert(sha256File(filePath) === artifact.sha256, "Decoded reference candidate artifact hash mismatch");
  assert(
    Number(artifact.rows) === Number(
      graph.summary.reference_candidate_ledger_rows ?? graph.summary.ambiguous_reference_field_groups,
    ),
    "Decoded reference candidate row count mismatch",
  );
}

function validateCallsiteProofArtifact(input) {
  const graph = input.value;
  assert(Number(graph.schema_version) >= 4, "Decoded reference graph does not provide schema-v4 current-build proof lineage");
  const artifact = graph.callsite_proof_artifact;
  assert(artifact && artifact.path, "Decoded reference graph has no current-build callsite proof artifact");
  const filePath = path.resolve(path.dirname(input.path), artifact.path);
  assert(existsSync(filePath), `Decoded reference callsite proof artifact is missing: ${filePath}`);
  assert(statSync(filePath).size === Number(artifact.bytes), "Decoded reference callsite proof byte count mismatch");
  assert(sha256File(filePath) === artifact.sha256, "Decoded reference callsite proof artifact hash mismatch");
  const proof = readJson(filePath);
  assert(Number(proof.schema_version) === 3, "Decoded reference callsite proof schema is unsupported");
  assert(String(proof.game_build) === String(graph.game_build), "Decoded reference callsite proof build mismatch");
  assert(
    Number(artifact.promoted_field_schemas) === Number(graph.summary.current_build_callsite_proven_field_schemas),
    "Decoded reference promoted callsite proof count mismatch",
  );
}

function callsiteProofMetadata(input) {
  const artifact = input.value.callsite_proof_artifact;
  return {
    path: relativeRepo(path.resolve(path.dirname(input.path), artifact.path)),
    rows: artifact.promoted_field_schemas,
    bytes: artifact.bytes,
    sha256: artifact.sha256,
    schema_version: artifact.schema_version,
    game_build: artifact.game_build,
    inputs: artifact.inputs,
  };
}

function semanticFieldSchemaMetadata(input) {
  return {
    path: relativeRepo(input.path),
    rows: input.value.summary.semantic_field_groups,
    closed_rows: input.value.summary.closed_field_groups,
    open_rows: input.value.summary.open_field_groups,
    evidence_states: input.value.summary.evidence_states,
    sha256: sha256File(input.path),
    semantic_sha256: input.value.semantic_sha256,
  };
}

function decodedFieldSchemaMetadata(input) {
  return {
    path: relativeRepo(input.path),
    rows: input.value.summary.decoded_field_paths,
    scalar_rows: input.value.summary.scalar_field_paths,
    array_rows: input.value.summary.array_field_paths,
    object_rows: input.value.summary.object_field_paths,
    mechanics_sensitive_rows: input.value.summary.mechanics_sensitive_field_paths,
    structural_inventory_complete: input.value.summary.structural_inventory_complete,
    sha256: sha256File(input.path),
    semantic_sha256: input.value.semantic_sha256,
  };
}

function referenceCandidateMetadata(input) {
  const artifact = input.value.reference_candidate_artifact;
  return {
    path: relativeRepo(path.resolve(path.dirname(input.path), artifact.path)),
    rows: artifact.rows,
    bytes: artifact.bytes,
    sha256: artifact.sha256,
    format: artifact.format,
  };
}

function validateSemanticRefreshArtifacts(report) {
  for (const artifact of report.artifacts) {
    const filePath = path.resolve(repoRoot, artifact.path);
    assert(existsSync(filePath), `Semantic refresh artifact is missing: ${artifact.path}`);
    assert(sha256File(filePath) === artifact.sha256, `Semantic refresh artifact is stale: ${artifact.path}`);
  }
}

function sha256File(filePath) {
  return createHash("sha256").update(readFileSync(filePath)).digest("hex");
}

function idsFrom(entries = []) {
  return entries.map((entry) => Number(typeof entry === "number" || typeof entry === "string" ? entry : entry.action_id ?? entry.actionId ?? entry.id)).filter(Number.isFinite).sort((a, b) => a - b);
}

function selfTest() {
  assert(idsFrom([{ action_id: "3" }, { id: 1 }]).join(",") === "1,3", "ID normalization failed");
  assert(idsFrom([]).length === 0, "Empty worklist normalization failed");
  console.log("bpsr-current-build-completeness self-test passed");
}

function parseArgs(args) {
  const output = {};
  for (let index = 0; index < args.length; index += 1) {
    const token = args[index];
    if (!token.startsWith("--")) throw new Error(`Unexpected argument ${token}`);
    const key = token.slice(2);
    const next = args[index + 1];
    if (!next || next.startsWith("--")) throw new Error(`Missing value for --${key}`);
    output[key] = next;
    index += 1;
  }
  return output;
}

function required(value, key) {
  if (!value[key]) throw new Error(`Missing --${key}`);
  return value[key];
}

function resolvePath(value) {
  return path.isAbsolute(value) ? value : path.resolve(repoRoot, value);
}

function relativeRepo(value) {
  return path.relative(repoRoot, value).replaceAll("\\", "/");
}

function readJson(filePath) {
  return JSON.parse(readFileSync(filePath, "utf8"));
}

function writeJson(filePath, value) {
  mkdirSync(path.dirname(filePath), { recursive: true });
  writeFileSync(filePath, `${JSON.stringify(value, null, 2)}\n`, "utf8");
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function usage(exitCode) {
  console.log("Usage:");
  console.log("  node tools/bpsr-current-build-completeness.mjs generate --build-root <directory> --reference-graph <json> [--semantic-field-schema <json>] [--decoded-field-schema <json>] [--output <json>]");
  console.log("  node tools/bpsr-current-build-completeness.mjs self-test");
  process.exit(exitCode);
}
