#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);
const ALLOWED_RECEIPT_STATES = new Set([
  "exact-current-build-offline-formula-proven",
  "exact-current-build-canonical-runtime-input-route-proven",
  "exact-current-build-local-full-snapshot-selection-route-proven-dirty-transition-open",
  "mechanics-candidates-indexed-runtime-proof-open",
  "canonical-capture-correlation-observed-runtime-gates-open",
]);

if (command === "build") build(resolveContext(options));
else if (command === "verify") verify(path.resolve(required(options, "input")));
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function resolveContext(parsed) {
  const buildId = required(parsed, "build");
  if (!/^\d+$/.test(buildId)) throw new Error("Build must contain only ASCII digits");
  return {
    build: buildId,
    fightAttributeProof: path.resolve(required(parsed, "fight-attribute-proof")),
    primaryStatProof: path.resolve(required(parsed, "primary-stat-proof")),
    primaryAttackRouteProof: path.resolve(required(parsed, "primary-attack-route-proof")),
    masteryRouteProof: path.resolve(required(parsed, "mastery-route-proof")),
    sourceHpRouteProof: path.resolve(required(parsed, "source-hp-route-proof")),
    allElementFamilyProof: path.resolve(required(parsed, "all-element-family-proof")),
    selectedFactorRouteProof: path.resolve(required(parsed, "selected-factor-route-proof")),
    selectedFactorMechanicProof: path.resolve(required(parsed, "selected-factor-mechanic-proof")),
    selectedFactorCaptureCorrelationProof: path.resolve(required(parsed, "selected-factor-capture-correlation-proof")),
    output: path.resolve(required(parsed, "output")),
  };
}

function build(context) {
  requireFile(context.fightAttributeProof, "fight attribute transform proof");
  requireFile(context.primaryStatProof, "primary stat attack transform proof");
  requireFile(context.primaryAttackRouteProof, "primaryAttack runtime route proof");
  requireFile(context.masteryRouteProof, "mastery runtime route proof");
  requireFile(context.sourceHpRouteProof, "source HP runtime route proof");
  requireFile(context.allElementFamilyProof, "all-element fixed-point family proof");
  requireFile(context.selectedFactorRouteProof, "selected-factor runtime route proof");
  requireFile(context.selectedFactorMechanicProof, "selected-factor mechanic route proof");
  requireFile(context.selectedFactorCaptureCorrelationProof, "selected-factor capture correlation proof");
  const fight = readJson(context.fightAttributeProof, "fight attribute transform proof");
  const primary = readJson(context.primaryStatProof, "primary stat attack transform proof");
  const primaryAttack = readJson(context.primaryAttackRouteProof, "primaryAttack runtime route proof");
  const masteryRoute = readJson(context.masteryRouteProof, "mastery runtime route proof");
  const sourceHpRoute = readJson(context.sourceHpRouteProof, "source HP runtime route proof");
  const allElementFamily = readJson(context.allElementFamilyProof, "all-element fixed-point family proof");
  const selectedFactorRoute = readJson(context.selectedFactorRouteProof, "selected-factor runtime route proof");
  const selectedFactorMechanic = readJson(context.selectedFactorMechanicProof, "selected-factor mechanic route proof");
  const selectedFactorCaptureCorrelation = readJson(context.selectedFactorCaptureCorrelationProof, "selected-factor capture correlation proof");
  validateFightProof(fight, context.build);
  validatePrimaryProof(primary, context.build);
  validatePrimaryAttackRouteProof(primaryAttack, context.build);
  validateMasteryRouteProof(masteryRoute, context.build);
  validateSourceHpRouteProof(sourceHpRoute, context.build);
  validateAllElementFamilyProof(allElementFamily, context.build);
  validateSelectedFactorRouteProof(selectedFactorRoute, context.build);
  validateSelectedFactorMechanicProof(selectedFactorMechanic, context.build);
  validateSelectedFactorCaptureCorrelationProof(selectedFactorCaptureCorrelation, context.build);

  const proofReceipts = [
    {
      proof_id: "fight-attribute-transform:mastery-to-mastery-pct",
      state: "exact-current-build-offline-formula-proven",
      model_keys: ["stat-conversion:mastery", "stat-conversion:mastery-stat"],
      proven_scope: {
        evaluator_formula: fight.summary.evaluator_formula,
        row_selection: fight.summary.row_selection,
        operation_order_exact: true,
        current_season_parameters_exact: true,
        underlying_value_rounding: fight.summary.underlying_value_rounding,
        display_only_rounding: fight.summary.display_only_rounding,
      },
      still_required_runtime_gates: [
        "encounter-local-raw-mastery-input",
        "combat-damage-stage-consumer",
        "provider-recipient-window",
        "integer-counterfactual-projection",
        "party-damage-conservation",
      ],
      evidence: fileDescriptor(context.fightAttributeProof),
      evidence_contract: {
        proof_state: fight.proof_state,
        combat_damage_stage_authority: fight.policy.combat_damage_stage_authority,
        exact_consumers: fight.summary.exact_consumers,
        proven_transform_fields: fight.summary.proven_transform_fields,
      },
    },
    {
      proof_id: "primary-stat-attack-transform",
      state: "exact-current-build-offline-formula-proven",
      model_keys: ["stat-conversion:adaptive-primary-stat"],
      proven_scope: {
        active_classes_proven: primary.summary.active_classes_proven,
        active_specs_proven: primary.summary.active_specs_proven,
        primary_transform_families_proven: primary.summary.primary_transform_families_proven,
        remaining_supported_class_routes: primary.summary.remaining_supported_class_routes,
        transform_contract: primary.transform_contract,
      },
      still_required_runtime_gates: [
        "encounter-local-class-and-primary-stat-input",
        "downstream-combat-damage-stage-consumer",
        "provider-recipient-window",
        "integer-counterfactual-projection",
        "party-damage-conservation",
      ],
      evidence: fileDescriptor(context.primaryStatProof),
      evidence_contract: {
        proof_state: primary.proof_state,
        exact_build_required: primary.policy.exact_build_required,
        structural_and_description_agreement_required: primary.policy.formula_requires_structural_opcode_and_description_agreement,
      },
    },
    {
      proof_id: "primary-attack-canonical-runtime-input-route",
      state: "exact-current-build-canonical-runtime-input-route-proven",
      model_keys: ["runtime-input:primaryattack"],
      proven_scope: {
        routed_sources: primaryAttack.summary.routed_sources,
        route_components: primaryAttack.summary.route_components,
        atk_only_sources: primaryAttack.summary.atk_only_sources,
        matk_only_sources: primaryAttack.summary.matk_only_sources,
        dual_atk_matk_sources: primaryAttack.summary.dual_atk_matk_sources,
        canonical_code_contracts_satisfied: primaryAttack.summary.canonical_code_contracts_satisfied,
        route_contract: primaryAttack.route_contract,
      },
      still_required_runtime_gates: structuredClone(primaryAttack.still_required_runtime_gates),
      evidence: fileDescriptor(context.primaryAttackRouteProof),
      evidence_contract: {
        proof_state: primaryAttack.proof_state,
        runtime_provider_windows_proven: primaryAttack.summary.runtime_provider_windows_proven,
        observed_event_replays_proven: primaryAttack.summary.observed_event_replays_proven,
        counterfactual_projections_proven: primaryAttack.summary.counterfactual_projections_proven,
        conservation_proofs: primaryAttack.summary.conservation_proofs,
        rdps_obligations_promoted: primaryAttack.summary.rdps_obligations_promoted,
      },
    },
    {
      proof_id: "mastery-canonical-runtime-input-route",
      state: "exact-current-build-canonical-runtime-input-route-proven",
      model_keys: ["runtime-input:mastery-stat"],
      proven_scope: {
        blocker_obligations: masteryRoute.summary.blocker_obligations,
        unique_sources: masteryRoute.summary.unique_sources,
        unique_effect_ids: masteryRoute.summary.unique_effect_ids,
        canonical_code_contracts_satisfied: masteryRoute.summary.canonical_code_contracts_satisfied,
        attribute_families: masteryRoute.attribute_families,
        route_contract: masteryRoute.route_contract,
      },
      still_required_runtime_gates: structuredClone(masteryRoute.still_required_runtime_gates),
      evidence: fileDescriptor(context.masteryRouteProof),
      evidence_contract: {
        proof_state: masteryRoute.proof_state,
        combat_damage_stage_consumers_proven: masteryRoute.summary.combat_damage_stage_consumers_proven,
        runtime_provider_windows_proven: masteryRoute.summary.runtime_provider_windows_proven,
        observed_event_replays_proven: masteryRoute.summary.observed_event_replays_proven,
        counterfactual_projections_proven: masteryRoute.summary.counterfactual_projections_proven,
        conservation_proofs: masteryRoute.summary.conservation_proofs,
        rdps_obligations_promoted: masteryRoute.summary.rdps_obligations_promoted,
      },
    },
    {
      proof_id: "source-hp-basis-canonical-runtime-input-route",
      state: "exact-current-build-canonical-runtime-input-route-proven",
      model_keys: ["runtime-input:sourcehpbasis"],
      proven_scope: {
        blocker_obligations: sourceHpRoute.summary.blocker_obligations,
        unique_sources: sourceHpRoute.summary.unique_sources,
        unique_effect_ids: sourceHpRoute.summary.unique_effect_ids,
        current_hp_packet_routes_proven: sourceHpRoute.summary.current_hp_packet_routes_proven,
        max_hp_packet_routes_proven: sourceHpRoute.summary.max_hp_packet_routes_proven,
        max_hp_reducer_routes_proven: sourceHpRoute.summary.max_hp_reducer_routes_proven,
        source_hp_basis_selectors_proven: sourceHpRoute.summary.source_hp_basis_selectors_proven,
        canonical_code_contracts_satisfied: sourceHpRoute.summary.canonical_code_contracts_satisfied,
        attribute_families: sourceHpRoute.attribute_families,
        route_contract: sourceHpRoute.route_contract,
      },
      still_required_runtime_gates: structuredClone(sourceHpRoute.still_required_runtime_gates),
      evidence: fileDescriptor(context.sourceHpRouteProof),
      evidence_contract: {
        proof_state: sourceHpRoute.proof_state,
        source_hp_basis_selectors_proven: sourceHpRoute.summary.source_hp_basis_selectors_proven,
        coherent_hit_time_hp_snapshots_proven: sourceHpRoute.summary.coherent_hit_time_hp_snapshots_proven,
        runtime_formula_models_closed: sourceHpRoute.summary.runtime_formula_models_closed,
        runtime_provider_windows_proven: sourceHpRoute.summary.runtime_provider_windows_proven,
        observed_event_replays_proven: sourceHpRoute.summary.observed_event_replays_proven,
        counterfactual_projections_proven: sourceHpRoute.summary.counterfactual_projections_proven,
        conservation_proofs: sourceHpRoute.summary.conservation_proofs,
        rdps_obligations_promoted: sourceHpRoute.summary.rdps_obligations_promoted,
      },
    },
    {
      proof_id: "all-element-fixed-point-family-canonical-runtime-input-route",
      state: "exact-current-build-canonical-runtime-input-route-proven",
      model_keys: ["runtime-input:all-element-fixed-point-family"],
      proven_scope: {
        identity: structuredClone(allElementFamily.identity),
        fixed_point_family: structuredClone(allElementFamily.fixed_point_family),
        provider_scalar: structuredClone(allElementFamily.provider_scalar),
        proven_scope: structuredClone(allElementFamily.proven_scope),
      },
      still_required_runtime_gates: structuredClone(allElementFamily.still_required_runtime_gates),
      evidence: fileDescriptor(context.allElementFamilyProof),
      evidence_contract: {
        proof_state: allElementFamily.proof_state,
        event_time_state_only: allElementFamily.policy.event_time_state_only,
        newer_profile_snapshots_forbidden: allElementFamily.policy.newer_profile_snapshots_forbidden,
        unresolved_events_remain_visible: allElementFamily.policy.unresolved_events_remain_visible,
        runtime_transfer_enabled: allElementFamily.policy.runtime_transfer_enabled,
        packet_oracle_correlated_status_events: allElementFamily.summary.packet_oracle_correlated_status_events,
        runtime_gates_closed: allElementFamily.summary.runtime_gates_closed,
        rdps_obligations_promoted: allElementFamily.summary.rdps_obligations_promoted,
        hidden_omissions: allElementFamily.summary.hidden_omissions,
      },
    },
    {
      proof_id: "selected-factor-local-full-snapshot-item-grade-route",
      state: "exact-current-build-local-full-snapshot-selection-route-proven-dirty-transition-open",
      model_keys: [],
      source_rule_ids: selectedFactorRoute.blocker_obligations.map((entry) => entry.source_rule_id),
      obligation_ids: selectedFactorRoute.blocker_obligations.map((entry) => entry.obligation_id),
      proven_scope: {
        selector_obligations: selectedFactorRoute.summary.selector_obligations,
        unique_sources: selectedFactorRoute.summary.unique_sources,
        unique_factor_buff_ids: selectedFactorRoute.summary.unique_factor_buff_ids,
        grade_item_routes: selectedFactorRoute.summary.grade_item_routes,
        current_runtime_selector_catalog_items: selectedFactorRoute.summary.current_runtime_selector_catalog_items,
        exact_local_full_snapshot_routes_proven: selectedFactorRoute.summary.exact_local_full_snapshot_routes_proven,
        dirty_transition_routes_proven: selectedFactorRoute.summary.dirty_transition_routes_proven,
        remote_selection_routes_proven: selectedFactorRoute.summary.remote_selection_routes_proven,
        route_contract: selectedFactorRoute.route_contract,
      },
      still_required_runtime_gates: structuredClone(selectedFactorRoute.still_required_runtime_gates),
      evidence: fileDescriptor(context.selectedFactorRouteProof),
      evidence_contract: {
        proof_state: selectedFactorRoute.proof_state,
        selected_factor_identity_does_not_prove_mechanics: selectedFactorRoute.policy.selected_factor_identity_does_not_prove_mechanics,
        runtime_provider_windows_proven: selectedFactorRoute.summary.runtime_provider_windows_proven,
        observed_event_replays_proven: selectedFactorRoute.summary.observed_event_replays_proven,
        counterfactual_projections_proven: selectedFactorRoute.summary.counterfactual_projections_proven,
        conservation_proofs: selectedFactorRoute.summary.conservation_proofs,
        rdps_obligations_promoted: selectedFactorRoute.summary.rdps_obligations_promoted,
      },
    },
    {
      proof_id: "selected-factor-exact-mechanic-relationship-route",
      state: "mechanics-candidates-indexed-runtime-proof-open",
      model_keys: [],
      source_rule_ids: selectedFactorMechanic.blocker_obligations.map((entry) => entry.source_rule_id),
      obligation_ids: selectedFactorMechanic.blocker_obligations.map((entry) => entry.obligation_id),
      proven_scope: {
        selector_obligations: selectedFactorMechanic.summary.selector_obligations,
        unique_sources: selectedFactorMechanic.summary.unique_sources,
        grade_item_routes: selectedFactorMechanic.summary.grade_item_routes,
        strict_unique_damage_ids: selectedFactorMechanic.summary.strict_unique_damage_ids,
        strict_unique_recount_ids: selectedFactorMechanic.summary.strict_unique_recount_ids,
        strict_exact_state_routes: selectedFactorMechanic.summary.strict_exact_state_routes,
        obligations_with_strict_damage_routes: selectedFactorMechanic.summary.obligations_with_strict_damage_routes,
        obligations_with_strict_recount_routes: selectedFactorMechanic.summary.obligations_with_strict_recount_routes,
        obligations_with_transfer_conflicts_retained: selectedFactorMechanic.summary.obligations_with_transfer_conflicts_retained,
        route_contract: selectedFactorMechanic.route_contract,
      },
      still_required_runtime_gates: structuredClone(selectedFactorMechanic.still_required_runtime_gates),
      evidence: fileDescriptor(context.selectedFactorMechanicProof),
      evidence_contract: {
        proof_state: selectedFactorMechanic.proof_state,
        exact_relationship_edges_are_strict_routes: selectedFactorMechanic.policy.exact_relationship_edges_are_strict_routes,
        description_and_localized_name_routes_are_candidates_only: selectedFactorMechanic.policy.description_and_localized_name_routes_are_candidates_only,
        conflicting_broad_and_exact_transfer_labels_are_retained: selectedFactorMechanic.policy.conflicting_broad_and_exact_transfer_labels_are_retained,
        runtime_provider_windows_proven: selectedFactorMechanic.summary.runtime_provider_windows_proven,
        observed_event_replays_proven: selectedFactorMechanic.summary.observed_event_replays_proven,
        counterfactual_projections_proven: selectedFactorMechanic.summary.counterfactual_projections_proven,
        conservation_proofs: selectedFactorMechanic.summary.conservation_proofs,
        rdps_obligations_promoted: selectedFactorMechanic.summary.rdps_obligations_promoted,
      },
    },
    {
      proof_id: "selected-factor-canonical-capture-correlation",
      state: "canonical-capture-correlation-observed-runtime-gates-open",
      model_keys: [],
      source_rule_ids: selectedFactorCaptureCorrelation.blocker_obligations.map((entry) => entry.source_rule_id),
      obligation_ids: selectedFactorCaptureCorrelation.blocker_obligations.map((entry) => entry.obligation_id),
      proven_scope: {
        selector_obligations: selectedFactorCaptureCorrelation.summary.selector_obligations,
        sealed_capture_reports: selectedFactorCaptureCorrelation.summary.sealed_capture_reports,
        selection_observations: selectedFactorCaptureCorrelation.summary.selection_observations,
        lifecycle_windows: selectedFactorCaptureCorrelation.summary.lifecycle_windows,
        exact_owner_bindings: selectedFactorCaptureCorrelation.summary.exact_owner_bindings,
        adjacent_report_owner_bindings: selectedFactorCaptureCorrelation.summary.adjacent_report_owner_bindings,
        emitted_action_matches: selectedFactorCaptureCorrelation.summary.emitted_action_matches,
        distinct_provider_recipient_windows: selectedFactorCaptureCorrelation.summary.distinct_provider_recipient_windows,
        coverage_states: selectedFactorCaptureCorrelation.summary.coverage_states,
        correlation_contract: selectedFactorCaptureCorrelation.correlation_contract,
      },
      still_required_runtime_gates: structuredClone(selectedFactorCaptureCorrelation.still_required_runtime_gates),
      evidence: fileDescriptor(context.selectedFactorCaptureCorrelationProof),
      evidence_contract: {
        proof_state: selectedFactorCaptureCorrelation.proof_state,
        packet_absence_is_negative_coverage_not_mechanic_disproof: selectedFactorCaptureCorrelation.policy.packet_absence_is_negative_coverage_not_mechanic_disproof,
        adjacent_report_carry_requires_exact_lineage_time_build_digest_and_owner: selectedFactorCaptureCorrelation.policy.adjacent_report_carry_requires_exact_lineage_time_build_digest_and_owner,
        no_runtime_gate_closed_without_exact_owner_binding: selectedFactorCaptureCorrelation.policy.no_runtime_gate_closed_without_exact_owner_binding,
        capture_correlation_does_not_prove_counterfactual_or_conservation: selectedFactorCaptureCorrelation.policy.capture_correlation_does_not_prove_counterfactual_or_conservation,
        runtime_provider_windows_proven: selectedFactorCaptureCorrelation.summary.runtime_provider_windows_proven,
        counterfactual_projections_proven: selectedFactorCaptureCorrelation.summary.counterfactual_projections_proven,
        conservation_proofs: selectedFactorCaptureCorrelation.summary.conservation_proofs,
        rdps_obligations_promoted: selectedFactorCaptureCorrelation.summary.rdps_obligations_promoted,
      },
    },
  ];

  const report = {
    schema_version: 3,
    generated_by: "tools/bpsr-shared-formula-proof-registry.mjs",
    game_build: context.build,
    policy: {
      exact_current_build_only: true,
      offline_formula_proof_does_not_close_runtime_gates: true,
      canonical_runtime_input_route_proof_does_not_close_provider_projection_or_conservation_gates: true,
      selected_factor_mechanic_routes_do_not_close_provider_projection_or_conservation_gates: true,
      selected_factor_capture_correlations_do_not_close_counterfactual_or_conservation_gates: true,
      proof_receipts_do_not_promote_rdps_obligations: true,
      runtime_inputs_are_never_inferred_from_static_tables: true,
      counterfactual_projection_and_conservation_remain_required: true,
      unresolved_evidence_is_never_hidden: true,
    },
    inputs: {
      fight_attribute_transform_proof: fileDescriptor(context.fightAttributeProof),
      primary_stat_attack_transform_proof: fileDescriptor(context.primaryStatProof),
      primary_attack_runtime_route_proof: fileDescriptor(context.primaryAttackRouteProof),
      mastery_runtime_route_proof: fileDescriptor(context.masteryRouteProof),
      source_hp_runtime_route_proof: fileDescriptor(context.sourceHpRouteProof),
      all_element_fixed_point_family_proof: fileDescriptor(context.allElementFamilyProof),
      selected_factor_runtime_route_proof: fileDescriptor(context.selectedFactorRouteProof),
      selected_factor_mechanic_route_proof: fileDescriptor(context.selectedFactorMechanicProof),
      selected_factor_capture_correlation_proof: fileDescriptor(context.selectedFactorCaptureCorrelationProof),
    },
    summary: {
      proof_receipts: proofReceipts.length,
      covered_model_keys: new Set(proofReceipts.flatMap((entry) => entry.model_keys)).size,
      offline_formula_proof_receipts: proofReceipts.filter((entry) => entry.state === "exact-current-build-offline-formula-proven").length,
      canonical_runtime_input_route_proof_receipts: proofReceipts.filter((entry) => entry.state === "exact-current-build-canonical-runtime-input-route-proven").length,
      selected_factor_route_proof_receipts: proofReceipts.filter((entry) => entry.state === "exact-current-build-local-full-snapshot-selection-route-proven-dirty-transition-open").length,
      selected_factor_mechanic_proof_receipts: proofReceipts.filter((entry) => entry.state === "mechanics-candidates-indexed-runtime-proof-open").length,
      selected_factor_capture_correlation_proof_receipts: proofReceipts.filter((entry) => entry.state === "canonical-capture-correlation-observed-runtime-gates-open").length,
      targeted_proof_receipts: proofReceipts.filter((entry) => (entry.source_rule_ids?.length ?? 0) > 0 || (entry.obligation_ids?.length ?? 0) > 0).length,
      covered_source_rule_ids: new Set(proofReceipts.flatMap((entry) => entry.source_rule_ids ?? [])).size,
      covered_obligation_ids: new Set(proofReceipts.flatMap((entry) => entry.obligation_ids ?? [])).size,
      runtime_gates_closed: 0,
      rdps_obligations_promoted: 0,
      hidden_omissions: 0,
    },
    proof_receipts: proofReceipts,
  };
  report.content_sha256 = contentHash(report);
  mkdirSync(path.dirname(context.output), { recursive: true });
  writeJson(context.output, report);
  verify(context.output);
  console.log(`Shared formula proof registry built for ${context.build}: ${proofReceipts.length} receipts cover ${report.summary.covered_model_keys} shared models; zero runtime gates closed.`);
}

function validateFightProof(proof, build) {
  requireBuild(proof, build, "fight attribute transform proof");
  if (proof.schema_version !== 1 || proof.proof_state !== "exact-current-build-client-ui-evaluator") throw new Error("Fight attribute proof is not the required exact evaluator proof");
  if (proof.policy?.formula_operation_order_is_exact !== true || proof.policy?.table_parameter_values_are_exact !== true) throw new Error("Fight attribute proof lacks exact operation/parameter authority");
  if (proof.policy?.combat_damage_stage_authority !== false) throw new Error("Fight attribute proof must explicitly deny combat damage-stage authority");
  if (proof.summary?.exact_consumers !== 2 || proof.summary?.proven_transform_fields !== 6) throw new Error("Fight attribute proof coverage changed; regenerate and review it");
  const seasonThree = (proof.rows ?? []).find((entry) => Number(entry.season_id) === 3);
  const mastery = seasonThree?.fields?.MasteryToMasteryPct;
  if (mastery?.state !== "exact-current-build-parameter-array" || !Array.isArray(mastery.parameters) || mastery.parameters.length !== 7) throw new Error("Current-season MasteryToMasteryPct parameters are not exact");
}

function validatePrimaryProof(proof, build) {
  requireBuild(proof, build, "primary stat attack transform proof");
  if (proof.schema_version !== 1 || proof.proof_state !== "offline-primary-stat-attack-transform-complete") throw new Error("Primary-stat proof is not complete");
  if (proof.policy?.exact_build_required !== true || proof.policy?.formula_requires_structural_opcode_and_description_agreement !== true) throw new Error("Primary-stat proof lacks its exact structural evidence policy");
  if (proof.summary?.active_classes_proven !== 9 || proof.summary?.active_specs_proven !== 18 || proof.summary?.primary_transform_families_proven !== 3) throw new Error("Primary-stat proof coverage changed; regenerate and review it");
  if (proof.summary?.remaining_supported_class_routes !== 0) throw new Error("Primary-stat proof still has supported class routes open");
}

function validatePrimaryAttackRouteProof(proof, build) {
  requireBuild(proof, build, "primaryAttack runtime route proof");
  if (proof.schema_version !== 1 || proof.generated_by !== "tools/bpsr-primary-attack-runtime-route-proof.mjs" ||
    proof.proof_state !== "exact-current-build-canonical-runtime-input-route-proven") {
    throw new Error("PrimaryAttack proof is not the required exact canonical runtime route proof");
  }
  if (proof.policy?.proof_receipt_does_not_promote_rdps_obligations !== true || proof.policy?.unresolved_evidence_is_never_hidden !== true) {
    throw new Error("PrimaryAttack route proof lacks its non-promotion policy");
  }
  if (proof.summary?.routed_sources !== 79 || proof.summary?.route_components !== 80 ||
    proof.summary?.canonical_code_contracts_satisfied !== proof.summary?.canonical_code_contracts) {
    throw new Error("PrimaryAttack route proof coverage changed; regenerate and review it");
  }
  if (proof.summary?.runtime_provider_windows_proven !== 0 || proof.summary?.observed_event_replays_proven !== 0 ||
    proof.summary?.counterfactual_projections_proven !== 0 || proof.summary?.conservation_proofs !== 0 ||
    proof.summary?.rdps_obligations_promoted !== 0 || !proof.still_required_runtime_gates?.length) {
    throw new Error("PrimaryAttack route proof improperly closes downstream runtime gates");
  }
}

function validateMasteryRouteProof(proof, build) {
  requireBuild(proof, build, "mastery runtime route proof");
  if (proof.schema_version !== 1 || proof.generated_by !== "tools/bpsr-mastery-runtime-route-proof.mjs" ||
    proof.proof_state !== "exact-current-build-canonical-runtime-input-route-proven") {
    throw new Error("Mastery proof is not the required exact canonical runtime route proof");
  }
  if (proof.policy?.combat_damage_stage_authority_remains_unproven !== true ||
    proof.policy?.proof_receipt_does_not_promote_rdps_obligations !== true ||
    proof.policy?.unresolved_evidence_is_never_hidden !== true) {
    throw new Error("Mastery route proof lacks its non-promotion policy");
  }
  if (proof.summary?.blocker_obligations !== 59 || proof.summary?.unique_sources !== 54 ||
    proof.summary?.unique_effect_ids !== 49 ||
    proof.summary?.canonical_code_contracts_satisfied !== proof.summary?.canonical_code_contracts) {
    throw new Error("Mastery route proof coverage changed; regenerate and review it");
  }
  if (proof.summary?.combat_damage_stage_consumers_proven !== 0 ||
    proof.summary?.runtime_provider_windows_proven !== 0 || proof.summary?.observed_event_replays_proven !== 0 ||
    proof.summary?.counterfactual_projections_proven !== 0 || proof.summary?.conservation_proofs !== 0 ||
    proof.summary?.rdps_obligations_promoted !== 0 || !proof.still_required_runtime_gates?.length) {
    throw new Error("Mastery route proof improperly closes downstream runtime gates");
  }
}

function validateSourceHpRouteProof(proof, build) {
  requireBuild(proof, build, "source HP runtime route proof");
  if (proof.schema_version !== 1 || proof.generated_by !== "tools/bpsr-source-hp-runtime-route-proof.mjs" ||
    proof.proof_state !== "exact-current-build-canonical-runtime-input-route-proven") {
    throw new Error("Source HP proof is not the required exact canonical runtime route proof");
  }
  if (proof.policy?.current_max_or_missing_hp_selector_is_never_inferred_from_text !== true ||
    proof.policy?.current_hp_reducer_retention_is_route_only_not_selector_proof !== true ||
    proof.policy?.proof_receipt_does_not_promote_rdps_obligations !== true ||
    proof.policy?.unresolved_evidence_is_never_hidden !== true) {
    throw new Error("Source HP route proof lacks its non-inference/non-promotion policy");
  }
  if (proof.summary?.blocker_obligations !== 25 || proof.summary?.unique_sources !== 25 ||
    proof.summary?.unique_source_ids !== 25 || proof.summary?.unique_effect_ids !== 23 ||
    proof.summary?.canonical_code_contracts_satisfied !== proof.summary?.canonical_code_contracts) {
    throw new Error("Source HP route proof coverage changed; regenerate and review it");
  }
  for (const key of ["source_hp_basis_selectors_proven", "coherent_hit_time_hp_snapshots_proven", "runtime_formula_models_closed", "runtime_provider_windows_proven", "observed_event_replays_proven", "counterfactual_projections_proven", "conservation_proofs", "rdps_obligations_promoted", "hidden_omissions"]) {
    if (proof.summary?.[key] !== 0) throw new Error(`Source HP route proof improperly closes ${key}`);
  }
  if (!proof.still_required_runtime_gates?.length) throw new Error("Source HP route proof omits remaining runtime gates");
}

function validateAllElementFamilyProof(proof, build) {
  requireBuild(proof, build, "all-element fixed-point family proof");
  if (proof.schema_version !== 1 || proof.generated_by !== "tools/bpsr-all-element-fixed-point-family-proof.mjs" ||
    proof.proof_state !== "exact-current-build-fixed-point-attribute-family-proven-damage-stage-open") {
    throw new Error("All-element proof is not the required exact fixed-point family receipt");
  }
  if (proof.policy?.event_time_state_only !== true ||
    proof.policy?.newer_profile_snapshots_forbidden !== true ||
    proof.policy?.unresolved_events_remain_visible !== true ||
    proof.policy?.proof_receipt_does_not_promote_rdps_attribution !== true ||
    proof.policy?.runtime_transfer_enabled !== false) {
    throw new Error("All-element proof lacks its event-time, retention, and non-promotion policy");
  }
  const family = proof.fixed_point_family;
  const members = (family?.family_members ?? []).map((entry) => Number(entry.id));
  const expectedMembers = [13100, 13101, 13102, 13103, 13104, 13105];
  if (family?.denominator !== 10000 || family?.table_storage !== "single-materialized-root-with-referenced-family-member-ids" ||
    members.length !== expectedMembers.length || members.some((value, index) => value !== expectedMembers[index]) ||
    family?.provider_cross_term_preserved !== true) {
    throw new Error("All-element fixed-point family identity or storage contract changed; regenerate and review it");
  }
  if (proof.summary?.family_members_proven !== 6 || proof.summary?.tier_scalars_proven !== 5 ||
    proof.summary?.packet_oracle_correlated_status_events < 1 ||
    JSON.stringify(proof.provider_scalar?.tier_basis_points) !== JSON.stringify([600, 700, 800, 900, 1000])) {
    throw new Error("All-element family or Fatal Spiral scalar coverage changed; regenerate and review it");
  }
  const expectedGates = [
    "combat-damage-stage-consumer",
    "affected-damage-property-coverage",
    "integer-damage-counterfactual-projection",
    "matching-window-conservation-replay",
  ];
  if (JSON.stringify(proof.still_required_runtime_gates) !== JSON.stringify(expectedGates)) {
    throw new Error("All-element proof does not retain every required runtime attribution gate");
  }
  for (const key of ["runtime_gates_closed", "rdps_obligations_promoted", "hidden_omissions"]) {
    if (proof.summary?.[key] !== 0) throw new Error(`All-element proof improperly closes or hides ${key}`);
  }
}

function validateSelectedFactorRouteProof(proof, build) {
  requireBuild(proof, build, "selected-factor runtime route proof");
  if (proof.schema_version !== 1 || proof.generated_by !== "tools/bpsr-selected-factor-runtime-route-proof.mjs" ||
    proof.proof_state !== "exact-current-build-local-full-snapshot-selection-route-proven-dirty-transition-open") {
    throw new Error("Selected-factor proof is not the required exact local full-snapshot route proof");
  }
  if (proof.policy?.exact_local_full_snapshot_selection_is_proven !== true ||
    proof.policy?.dirty_live_transition_selection_remains_open !== true ||
    proof.policy?.remote_player_exact_selection_remains_open !== true ||
    proof.policy?.selected_factor_identity_does_not_prove_mechanics !== true ||
    proof.policy?.proof_receipt_does_not_promote_rdps_obligations !== true ||
    proof.policy?.unresolved_evidence_is_never_hidden !== true) {
    throw new Error("Selected-factor route proof lacks its bounded non-promotion policy");
  }
  if (proof.summary?.selector_obligations !== 10 || proof.summary?.unique_sources !== 10 ||
    proof.summary?.unique_factor_buff_ids !== 10 || proof.summary?.grade_item_routes !== 100 ||
    proof.summary?.current_runtime_selector_catalog_items !== 3830 ||
    proof.summary?.canonical_code_contracts_satisfied !== proof.summary?.canonical_code_contracts) {
    throw new Error("Selected-factor route proof coverage changed; regenerate and review it");
  }
  for (const key of ["dirty_transition_routes_proven", "remote_selection_routes_proven", "runtime_provider_windows_proven", "observed_event_replays_proven", "counterfactual_projections_proven", "conservation_proofs", "rdps_obligations_promoted", "hidden_omissions"]) {
    if (proof.summary?.[key] !== 0) throw new Error(`Selected-factor route proof improperly closes ${key}`);
  }
  if (!proof.still_required_runtime_gates?.length || proof.blocker_obligations?.length !== 10) throw new Error("Selected-factor route proof omits bounded obligations or remaining gates");
}

function validateSelectedFactorMechanicProof(proof, build) {
  requireBuild(proof, build, "selected-factor mechanic route proof");
  if (proof.schema_version !== 2 || proof.generated_by !== "tools/bpsr-selected-factor-mechanic-route-proof.mjs" ||
    proof.proof_state !== "mechanics-candidates-indexed-runtime-proof-open") {
    throw new Error("Selected-factor mechanic proof is not the required exact relationship-route proof");
  }
  if (proof.policy?.exact_relationship_edges_are_strict_routes !== true ||
    proof.policy?.description_and_localized_name_routes_are_candidates_only !== true ||
    proof.policy?.catalog_routes_are_retained_without_automatic_promotion !== true ||
    proof.policy?.conflicting_broad_and_exact_transfer_labels_are_retained !== true ||
    proof.policy?.declared_transfer_classification_is_retained_separately !== true ||
    proof.policy?.static_owner_context_does_not_prove_self_only_recipient_scope !== true ||
    proof.policy?.effective_transfer_classification_reopens_owner_local_context_without_packet_proof !== true ||
    proof.policy?.proof_receipt_does_not_promote_rdps_obligations !== true ||
    proof.policy?.unresolved_evidence_is_never_hidden !== true) {
    throw new Error("Selected-factor mechanic proof lacks its strict-route and non-promotion policy");
  }
  if (proof.summary?.selector_obligations !== 10 || proof.summary?.unique_sources !== 10 ||
    proof.summary?.grade_item_routes !== 100 || proof.summary?.strict_unique_damage_ids !== 4 ||
    proof.summary?.strict_unique_recount_ids !== 1 || proof.summary?.strict_exact_state_routes !== 1 ||
    proof.summary?.obligations_with_strict_damage_routes !== 1 ||
    proof.summary?.obligations_with_strict_recount_routes !== 1 ||
    proof.summary?.obligations_with_transfer_conflicts_retained !== 1) {
    throw new Error("Selected-factor mechanic proof coverage changed; regenerate and review it");
  }
  for (const key of ["runtime_provider_windows_proven", "observed_event_replays_proven", "counterfactual_projections_proven", "conservation_proofs", "rdps_obligations_promoted", "hidden_omissions"]) {
    if (proof.summary?.[key] !== 0) throw new Error(`Selected-factor mechanic proof improperly closes ${key}`);
  }
  if (!proof.still_required_runtime_gates?.length || proof.blocker_obligations?.length !== 10) throw new Error("Selected-factor mechanic proof omits bounded obligations or remaining gates");
}

function validateSelectedFactorCaptureCorrelationProof(proof, build) {
  requireBuild(proof, build, "selected-factor capture correlation proof");
  if (proof.schema_version !== 2 || proof.generated_by !== "tools/bpsr-selected-factor-capture-correlation-proof.mjs" ||
    proof.proof_state !== "canonical-capture-correlation-observed-runtime-gates-open") {
    throw new Error("Selected-factor capture correlation proof is not the required canonical-capture receipt");
  }
  if (proof.policy?.packet_absence_is_negative_coverage_not_mechanic_disproof !== true ||
    proof.policy?.exact_selection_and_lifecycle_are_retained_separately !== true ||
    proof.policy?.adjacent_report_carry_requires_exact_lineage_time_build_digest_and_owner !== true ||
    proof.policy?.no_runtime_gate_closed_without_exact_owner_binding !== true ||
    proof.policy?.capture_correlation_does_not_prove_counterfactual_or_conservation !== true ||
    proof.policy?.proof_receipt_does_not_promote_rdps_obligations !== true ||
    proof.policy?.unresolved_evidence_is_never_hidden !== true) {
    throw new Error("Selected-factor capture correlation proof lacks its bounded negative-coverage and non-promotion policy");
  }
  if (proof.summary?.selector_obligations !== 10 || proof.summary?.unique_sources !== 10 ||
    proof.summary?.sealed_capture_reports < 1 || proof.summary?.correlation_bundles < 1) {
    throw new Error("Selected-factor capture correlation proof coverage changed; regenerate and review it");
  }
  for (const key of ["runtime_provider_windows_proven", "counterfactual_projections_proven", "conservation_proofs", "rdps_obligations_promoted", "hidden_omissions"]) {
    if (proof.summary?.[key] !== 0) throw new Error(`Selected-factor capture correlation proof improperly closes ${key}`);
  }
  if (!proof.still_required_runtime_gates?.length || proof.blocker_obligations?.length !== 10) throw new Error("Selected-factor capture correlation proof omits bounded obligations or remaining gates");
}

function verify(input) {
  const report = readJson(input, "shared formula proof registry");
  if (report.schema_version !== 3) throw new Error("Shared formula proof registry schema_version must be 3");
  if (report.content_sha256 !== contentHash(report)) throw new Error("Shared formula proof registry content hash mismatch");
  if (!/^\d+$/.test(String(report.game_build))) throw new Error("Shared formula proof registry has invalid build");
  if (report.policy?.offline_formula_proof_does_not_close_runtime_gates !== true ||
    report.policy?.selected_factor_mechanic_routes_do_not_close_provider_projection_or_conservation_gates !== true ||
    report.policy?.selected_factor_capture_correlations_do_not_close_counterfactual_or_conservation_gates !== true ||
    report.policy?.proof_receipts_do_not_promote_rdps_obligations !== true) throw new Error("Shared formula proof registry has an unsafe policy");
  const proofIds = new Set();
  const modelKeys = new Set();
  const sourceRuleIds = new Set();
  const obligationIds = new Set();
  for (const receipt of report.proof_receipts ?? []) {
    if (proofIds.has(receipt.proof_id)) throw new Error(`Duplicate proof receipt ${receipt.proof_id}`);
    proofIds.add(receipt.proof_id);
    if (!ALLOWED_RECEIPT_STATES.has(receipt.state)) throw new Error(`Invalid receipt state ${receipt.proof_id}`);
    if (!receipt.still_required_runtime_gates?.length) throw new Error(`Receipt ${receipt.proof_id} improperly closes every runtime gate`);
    if (!(receipt.model_keys?.length ?? 0) && !(receipt.source_rule_ids?.length ?? 0) && !(receipt.obligation_ids?.length ?? 0)) throw new Error(`Receipt ${receipt.proof_id} covers no model or targeted obligation`);
    for (const modelKey of receipt.model_keys ?? []) {
      if (modelKeys.has(modelKey)) throw new Error(`Model ${modelKey} is covered by multiple proof receipts`);
      modelKeys.add(modelKey);
    }
    for (const sourceRuleId of receipt.source_rule_ids ?? []) {
      sourceRuleIds.add(sourceRuleId);
    }
    for (const obligationId of receipt.obligation_ids ?? []) {
      obligationIds.add(obligationId);
    }
  }
  if (proofIds.size !== report.summary?.proof_receipts || modelKeys.size !== report.summary?.covered_model_keys || sourceRuleIds.size !== report.summary?.covered_source_rule_ids || obligationIds.size !== report.summary?.covered_obligation_ids) throw new Error("Shared formula proof registry summary mismatch");
  if (report.summary?.offline_formula_proof_receipts !== [...proofIds].filter((proofId) => report.proof_receipts.find((entry) => entry.proof_id === proofId)?.state === "exact-current-build-offline-formula-proven").length ||
    report.summary?.canonical_runtime_input_route_proof_receipts !== [...proofIds].filter((proofId) => report.proof_receipts.find((entry) => entry.proof_id === proofId)?.state === "exact-current-build-canonical-runtime-input-route-proven").length) {
    throw new Error("Shared formula proof receipt-state summary mismatch");
  }
  const targetedReceipts = (report.proof_receipts ?? []).filter((entry) => (entry.source_rule_ids?.length ?? 0) > 0 || (entry.obligation_ids?.length ?? 0) > 0);
  const selectedFactorReceipts = (report.proof_receipts ?? []).filter((entry) => entry.state === "exact-current-build-local-full-snapshot-selection-route-proven-dirty-transition-open");
  const selectedFactorMechanicReceipts = (report.proof_receipts ?? []).filter((entry) => entry.state === "mechanics-candidates-indexed-runtime-proof-open");
  const selectedFactorCaptureCorrelationReceipts = (report.proof_receipts ?? []).filter((entry) => entry.state === "canonical-capture-correlation-observed-runtime-gates-open");
  if (targetedReceipts.length !== report.summary?.targeted_proof_receipts ||
    selectedFactorReceipts.length !== report.summary?.selected_factor_route_proof_receipts ||
    selectedFactorMechanicReceipts.length !== report.summary?.selected_factor_mechanic_proof_receipts ||
    selectedFactorCaptureCorrelationReceipts.length !== report.summary?.selected_factor_capture_correlation_proof_receipts ||
    targetedReceipts.length !== 3 || selectedFactorReceipts.length !== 1 || selectedFactorMechanicReceipts.length !== 1 ||
    selectedFactorCaptureCorrelationReceipts.length !== 1 || sourceRuleIds.size !== 10 || obligationIds.size !== 30) throw new Error("Shared formula targeted receipt summary mismatch");
  if (report.summary?.runtime_gates_closed !== 0 || report.summary?.rdps_obligations_promoted !== 0 || report.summary?.hidden_omissions !== 0) throw new Error("Offline registry must not close runtime gates, promote obligations, or hide evidence");
  console.log(`Shared formula proof registry verified for build ${report.game_build}: ${proofIds.size} receipts, ${modelKeys.size} models, ${sourceRuleIds.size} targeted sources, zero runtime promotions.`);
  return report;
}

function selfTest() {
  const root = mkdtempSync(path.join(tmpdir(), "rlogs-shared-formula-proof-test-"));
  try {
    const fight = path.join(root, "fight.json");
    const primary = path.join(root, "primary.json");
    const primaryAttack = path.join(root, "primary-attack.json");
    const masteryRoute = path.join(root, "mastery-route.json");
    const sourceHpRoute = path.join(root, "source-hp-route.json");
    const allElementFamily = path.join(root, "all-element-family.json");
    const selectedFactorRoute = path.join(root, "selected-factor-route.json");
    const selectedFactorMechanic = path.join(root, "selected-factor-mechanic.json");
    const selectedFactorCaptureCorrelation = path.join(root, "selected-factor-capture-correlation.json");
    const output = path.join(root, "registry.json");
    writeJson(fight, {
      schema_version: 1, game_build: "1", proof_state: "exact-current-build-client-ui-evaluator",
      policy: { formula_operation_order_is_exact: true, table_parameter_values_are_exact: true, combat_damage_stage_authority: false },
      summary: { exact_consumers: 2, proven_transform_fields: 6, evaluator_formula: "f", row_selection: "r", underlying_value_rounding: "none", display_only_rounding: "display" },
      rows: [{ season_id: 3, fields: { MasteryToMasteryPct: { state: "exact-current-build-parameter-array", parameters: [1, 1, 1, 0, 0, 0, 0] } } }],
    });
    writeJson(primary, {
      schema_version: 1, game_build: "1", proof_state: "offline-primary-stat-attack-transform-complete",
      policy: { exact_build_required: true, formula_requires_structural_opcode_and_description_agreement: true },
      summary: { active_classes_proven: 9, active_specs_proven: 18, primary_transform_families_proven: 3, remaining_supported_class_routes: 0 },
      transform_contract: { input: "primary", output: "attack" },
    });
    writeJson(primaryAttack, {
      schema_version: 1, generated_by: "tools/bpsr-primary-attack-runtime-route-proof.mjs", game_build: "1",
      proof_state: "exact-current-build-canonical-runtime-input-route-proven",
      policy: { proof_receipt_does_not_promote_rdps_obligations: true, unresolved_evidence_is_never_hidden: true },
      route_contract: { input: "attributes", output: "ATK/MATK" },
      summary: { routed_sources: 79, route_components: 80, atk_only_sources: 58, matk_only_sources: 20, dual_atk_matk_sources: 1, canonical_code_contracts: 4, canonical_code_contracts_satisfied: 4, runtime_provider_windows_proven: 0, observed_event_replays_proven: 0, counterfactual_projections_proven: 0, conservation_proofs: 0, rdps_obligations_promoted: 0 },
      still_required_runtime_gates: ["provider", "projection", "conservation"],
    });
    writeJson(masteryRoute, {
      schema_version: 1, generated_by: "tools/bpsr-mastery-runtime-route-proof.mjs", game_build: "1",
      proof_state: "exact-current-build-canonical-runtime-input-route-proven",
      policy: { combat_damage_stage_authority_remains_unproven: true, proof_receipt_does_not_promote_rdps_obligations: true, unresolved_evidence_is_never_hidden: true },
      attribute_families: { mastery: [11940, 11942] }, route_contract: { input: "attributes", output: "mastery" },
      summary: { blocker_obligations: 59, unique_sources: 54, unique_effect_ids: 49, canonical_code_contracts: 4, canonical_code_contracts_satisfied: 4, combat_damage_stage_consumers_proven: 0, runtime_provider_windows_proven: 0, observed_event_replays_proven: 0, counterfactual_projections_proven: 0, conservation_proofs: 0, rdps_obligations_promoted: 0 },
      still_required_runtime_gates: ["consumer", "provider", "projection", "conservation"],
    });
    writeJson(sourceHpRoute, {
      schema_version: 1, generated_by: "tools/bpsr-source-hp-runtime-route-proof.mjs", game_build: "1",
      proof_state: "exact-current-build-canonical-runtime-input-route-proven",
      policy: { current_max_or_missing_hp_selector_is_never_inferred_from_text: true, current_hp_reducer_retention_is_route_only_not_selector_proof: true, proof_receipt_does_not_promote_rdps_obligations: true, unresolved_evidence_is_never_hidden: true },
      attribute_families: { current_hp: { lane_ids: [11310] }, max_hp: { lane_ids: [11320, 11321, 11322, 11323, 11324, 11325] } },
      route_contract: { current_hp: { exact_packet_route_proven: true }, max_hp: { exact_packet_route_proven: true } },
      summary: { blocker_obligations: 25, unique_sources: 25, unique_source_ids: 25, unique_effect_ids: 23, current_hp_packet_routes_proven: 1, max_hp_packet_routes_proven: 1, max_hp_reducer_routes_proven: 1, canonical_code_contracts: 4, canonical_code_contracts_satisfied: 4, source_hp_basis_selectors_proven: 0, coherent_hit_time_hp_snapshots_proven: 0, runtime_formula_models_closed: 0, runtime_provider_windows_proven: 0, observed_event_replays_proven: 0, counterfactual_projections_proven: 0, conservation_proofs: 0, rdps_obligations_promoted: 0, hidden_omissions: 0 },
      still_required_runtime_gates: ["selector", "coherence", "projection", "conservation"],
    });
    writeJson(allElementFamily, {
      schema_version: 1,
      generated_by: "tools/bpsr-all-element-fixed-point-family-proof.mjs",
      game_build: "1",
      proof_state: "exact-current-build-fixed-point-attribute-family-proven-damage-stage-open",
      policy: {
        event_time_state_only: true,
        newer_profile_snapshots_forbidden: true,
        unresolved_events_remain_visible: true,
        proof_receipt_does_not_promote_rdps_attribution: true,
        runtime_transfer_enabled: false,
      },
      identity: { imagine_skill_id: 3957, effect_id: 2110125, provider_marker_effect_id: 2110124 },
      fixed_point_family: {
        denominator: 10000,
        provider_cross_term_preserved: true,
        table_storage: "single-materialized-root-with-referenced-family-member-ids",
        family_members: [13100, 13101, 13102, 13103, 13104, 13105].map((id) => ({ id })),
      },
      provider_scalar: {
        tier_basis_points: [600, 700, 800, 900, 1000],
        packet_attribute_oracle: { correlated_status_events: 1 },
      },
      proven_scope: { fixed_point_units: true, packet_family_replay_equation: true },
      still_required_runtime_gates: [
        "combat-damage-stage-consumer",
        "affected-damage-property-coverage",
        "integer-damage-counterfactual-projection",
        "matching-window-conservation-replay",
      ],
      summary: {
        family_members_proven: 6,
        tier_scalars_proven: 5,
        packet_oracle_correlated_status_events: 1,
        runtime_gates_closed: 0,
        rdps_obligations_promoted: 0,
        hidden_omissions: 0,
      },
    });
    writeJson(selectedFactorRoute, {
      schema_version: 1, generated_by: "tools/bpsr-selected-factor-runtime-route-proof.mjs", game_build: "1",
      proof_state: "exact-current-build-local-full-snapshot-selection-route-proven-dirty-transition-open",
      policy: { exact_local_full_snapshot_selection_is_proven: true, dirty_live_transition_selection_remains_open: true, remote_player_exact_selection_remains_open: true, selected_factor_identity_does_not_prove_mechanics: true, proof_receipt_does_not_promote_rdps_obligations: true, unresolved_evidence_is_never_hidden: true },
      route_contract: { input: "profile middle-node item", output: "factor family and grade" },
      summary: { selector_obligations: 10, unique_sources: 10, unique_factor_buff_ids: 10, grade_item_routes: 100, current_runtime_selector_catalog_items: 3830, exact_local_full_snapshot_routes_proven: 1, dirty_transition_routes_proven: 0, remote_selection_routes_proven: 0, canonical_code_contracts: 4, canonical_code_contracts_satisfied: 4, runtime_provider_windows_proven: 0, observed_event_replays_proven: 0, counterfactual_projections_proven: 0, conservation_proofs: 0, rdps_obligations_promoted: 0, hidden_omissions: 0 },
      blocker_obligations: Array.from({ length: 10 }, (_, index) => ({ obligation_id: `mrs:${index}#selectedFactorGrade`, source_rule_id: `mrs:${index}` })),
      still_required_runtime_gates: ["dirty transition", "remote selection", "mechanics", "projection", "conservation"],
    });
    writeJson(selectedFactorMechanic, {
      schema_version: 2,
      generated_by: "tools/bpsr-selected-factor-mechanic-route-proof.mjs",
      game_build: "1",
      proof_state: "mechanics-candidates-indexed-runtime-proof-open",
      policy: {
        exact_relationship_edges_are_strict_routes: true,
        description_and_localized_name_routes_are_candidates_only: true,
        catalog_routes_are_retained_without_automatic_promotion: true,
        conflicting_broad_and_exact_transfer_labels_are_retained: true,
        declared_transfer_classification_is_retained_separately: true,
        static_owner_context_does_not_prove_self_only_recipient_scope: true,
        effective_transfer_classification_reopens_owner_local_context_without_packet_proof: true,
        proof_receipt_does_not_promote_rdps_obligations: true,
        unresolved_evidence_is_never_hidden: true,
      },
      route_contract: { strict_uid_route_source: "fixture" },
      summary: {
        selector_obligations: 10,
        unique_sources: 10,
        grade_item_routes: 100,
        strict_unique_damage_ids: 4,
        strict_unique_recount_ids: 1,
        strict_exact_state_routes: 1,
        obligations_with_strict_damage_routes: 1,
        obligations_with_strict_recount_routes: 1,
        obligations_with_transfer_conflicts_retained: 1,
        runtime_provider_windows_proven: 0,
        observed_event_replays_proven: 0,
        counterfactual_projections_proven: 0,
        conservation_proofs: 0,
        rdps_obligations_promoted: 0,
        hidden_omissions: 0,
      },
      blocker_obligations: Array.from({ length: 10 }, (_, index) => ({ obligation_id: `mrs:${index}#selectedFactorMechanics`, source_rule_id: `mrs:${index}` })),
      still_required_runtime_gates: ["provider", "projection", "conservation"],
    });
    writeJson(selectedFactorCaptureCorrelation, {
      schema_version: 2,
      generated_by: "tools/bpsr-selected-factor-capture-correlation-proof.mjs",
      game_build: "1",
      proof_state: "canonical-capture-correlation-observed-runtime-gates-open",
      policy: {
        packet_absence_is_negative_coverage_not_mechanic_disproof: true,
        exact_selection_and_lifecycle_are_retained_separately: true,
        adjacent_report_carry_requires_exact_lineage_time_build_digest_and_owner: true,
        no_runtime_gate_closed_without_exact_owner_binding: true,
        capture_correlation_does_not_prove_counterfactual_or_conservation: true,
        proof_receipt_does_not_promote_rdps_obligations: true,
        unresolved_evidence_is_never_hidden: true,
      },
      correlation_contract: { selection_route: "fixture", effect_lifecycle_route: "fixture" },
      summary: {
        correlation_bundles: 1,
        sealed_capture_reports: 1,
        selector_obligations: 10,
        unique_sources: 10,
        selection_observations: 1,
        lifecycle_windows: 0,
        exact_owner_bindings: 0,
        adjacent_report_owner_bindings: 0,
        emitted_action_matches: 0,
        distinct_provider_recipient_windows: 0,
        coverage_states: { "selection-observed-effect-not-observed": 1, "no-selected-grade-or-runtime-effect-observed": 9 },
        runtime_provider_windows_proven: 0,
        counterfactual_projections_proven: 0,
        conservation_proofs: 0,
        rdps_obligations_promoted: 0,
        hidden_omissions: 0,
      },
      blocker_obligations: Array.from({ length: 10 }, (_, index) => ({ obligation_id: `mrs:${index}#selectedFactorCaptureCorrelation`, source_rule_id: `mrs:${index}` })),
      still_required_runtime_gates: ["provider", "projection", "conservation"],
    });
    build({ build: "1", fightAttributeProof: fight, primaryStatProof: primary, primaryAttackRouteProof: primaryAttack, masteryRouteProof: masteryRoute, sourceHpRouteProof: sourceHpRoute, allElementFamilyProof: allElementFamily, selectedFactorRouteProof: selectedFactorRoute, selectedFactorMechanicProof: selectedFactorMechanic, selectedFactorCaptureCorrelationProof: selectedFactorCaptureCorrelation, output });
    const report = verify(output);
    if (report.summary.proof_receipts !== 9 || report.summary.covered_model_keys !== 7 || report.summary.canonical_runtime_input_route_proof_receipts !== 4 || report.summary.covered_source_rule_ids !== 10 || report.summary.covered_obligation_ids !== 30 || report.summary.selected_factor_route_proof_receipts !== 1 || report.summary.selected_factor_mechanic_proof_receipts !== 1 || report.summary.selected_factor_capture_correlation_proof_receipts !== 1) throw new Error("Self-test model or targeted coverage mismatch");
    console.log("Shared formula proof registry self-test passed.");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

function requireBuild(value, build, label) { if (String(value.game_build) !== String(build)) throw new Error(`${label} build ${value.game_build} does not match ${build}`); }
function requireFile(file, label) { if (!existsSync(file) || !statSync(file).isFile()) throw new Error(`Missing ${label}: ${file}`); }
function fileDescriptor(file) { return { path: file.replaceAll("\\", "/"), bytes: statSync(file).size, sha256: hashFile(file) }; }
function contentHash(value) { const clone = structuredClone(value); delete clone.content_sha256; return hashText(stableStringify(clone)); }
function stableStringify(value) { if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`; if (value && typeof value === "object") return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`; return JSON.stringify(value); }
function hashFile(file) { return createHash("sha256").update(readFileSync(file)).digest("hex"); }
function hashText(value) { return createHash("sha256").update(value).digest("hex"); }
function readJson(file, label) { try { return JSON.parse(readFileSync(file, "utf8")); } catch (error) { throw new Error(`Unable to read ${label} ${file}: ${error.message}`); } }
function writeJson(file, value) { writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`, "utf8"); }
function parseArgs(args) { const parsed = {}; for (let index = 0; index < args.length; index += 1) { const arg = args[index]; if (!arg.startsWith("--")) throw new Error(`Unexpected argument ${arg}`); const key = arg.slice(2); const value = args[index + 1]; if (value === undefined || value.startsWith("--")) throw new Error(`Missing value for --${key}`); parsed[key] = value; index += 1; } return parsed; }
function required(value, key) { if (!value[key]) throw new Error(`Missing --${key}`); return value[key]; }
function usage(exitCode) { console.log("Usage:\n  node tools/bpsr-shared-formula-proof-registry.mjs build --build <id> --fight-attribute-proof <json> --primary-stat-proof <json> --primary-attack-route-proof <json> --mastery-route-proof <json> --source-hp-route-proof <json> --all-element-family-proof <json> --selected-factor-route-proof <json> --selected-factor-mechanic-proof <json> --selected-factor-capture-correlation-proof <json> --output <json>\n  node tools/bpsr-shared-formula-proof-registry.mjs verify --input <json>\n  node tools/bpsr-shared-formula-proof-registry.mjs self-test"); process.exit(exitCode); }
