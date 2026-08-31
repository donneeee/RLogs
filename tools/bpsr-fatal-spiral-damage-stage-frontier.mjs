#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 17;
const GENERATED_BY = "tools/bpsr-fatal-spiral-damage-stage-frontier.mjs";
const GAME_BUILD = "24687926";
const EFFECT_ID = 2110125;
const PROVIDER_EFFECT_ID = 2110124;

function fail(message) {
  throw new Error(message);
}

function readJson(file, label) {
  try {
    return JSON.parse(fs.readFileSync(file, "utf8"));
  } catch (error) {
    fail(`Cannot read ${label} ${file}: ${error.message}`);
  }
}

function descriptor(file) {
  const bytes = fs.readFileSync(file);
  return {
    path: path.resolve(file).replaceAll("\\", "/"),
    bytes: bytes.length,
    sha256: crypto.createHash("sha256").update(bytes).digest("hex"),
  };
}

function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function digest(report) {
  const copy = structuredClone(report);
  delete copy.content_sha256;
  return crypto.createHash("sha256").update(canonical(copy)).digest("hex").toUpperCase();
}

function validateTierWindow(proof) {
  if (
    proof.schema_version !== 3 ||
    proof.generated_by !== "tools/bpsr-fatal-spiral-tier-window-proof.mjs" ||
    proof.game_build !== GAME_BUILD ||
    Number(proof.identity?.effect_id) !== EFFECT_ID ||
    Number(proof.identity?.provider_marker_effect_id) !== PROVIDER_EFFECT_ID ||
    proof.topology?.effect_edge !== "provider -> effect/status lifecycle -> recipient or enemy target" ||
    proof.topology?.damage_edge !== "recipient damage action -> recipient or enemy target" ||
    proof.topology?.source_side_join !== "effect endpoint equals damage actor" ||
    proof.topology?.target_side_join !== "effect endpoint equals damage target" ||
    proof.policy?.source_side_and_target_side_joins_are_independent !== true ||
    proof.policy?.endpoint_allegiance_is_assumed !== false ||
    proof.policy?.remote_cast_packets_are_required !== false ||
    proof.proof_closure?.exact_event_time_provider_tier_join_complete !== true ||
    proof.proof_closure?.exact_effect_lifecycle_window_selection_complete !== true ||
    proof.proof_closure?.source_side_affected_damage_selection_complete !== true ||
    proof.proof_closure?.combat_damage_stage_consumer_proven !== false ||
    proof.proof_closure?.integer_damage_counterfactual_projection_complete !== false ||
    proof.proof_closure?.runtime_rdps_credit_enabled !== false
  ) fail("Fatal Spiral tier/window proof is unsafe or incompatible");
}

function validateGapAudit(audit) {
  const summary = audit.summary ?? {};
  if (
    audit.schema_version !== 3 ||
    audit.generated_by !== "rlogs-bpsr-rlog-gap-window-audit" ||
    audit.game_build !== GAME_BUILD ||
    Number(audit.effect_id) !== EFFECT_ID ||
    audit.damage_relationship !== "source" ||
    audit.policy?.damage_relationship_is_explicit !== true ||
    audit.policy?.structurally_unobservable_remote_player_packets_are_not_acquisition_requirements !== true ||
    audit.policy?.packet_absence_is_not_zero !== true ||
    audit.policy?.formula_authority !== false ||
    audit.policy?.runtime_authority !== false ||
    audit.policy?.provider_rdps_credit_allowed !== false ||
    Number(summary.source_rlog_count) !== 6 ||
    Number(summary.selected_effect_status_event_count) !== 394 ||
    Number(summary.selected_effect_applied_count) !== 202 ||
    Number(summary.selected_effect_terminal_count) !== 192 ||
    Number(summary.selected_effect_complete_gap_bounded_lifecycle_count) !== 29 ||
    Number(summary.selected_effect_complete_windows_with_damage_count) !== 29 ||
    Number(summary.selected_effect_damage_events_while_active) !== 27238 ||
    Number(summary.selected_effect_lifecycles_cut_by_data_quality_boundary) !== 173 ||
    summary.formula_authority !== false ||
    summary.runtime_authority !== false ||
    summary.provider_rdps_credit_allowed !== false
  ) fail("Fatal Spiral source-side gap-window audit is unsafe or does not reproduce the reviewed corpus");
}

function validateGapSafeLifecycleActionSummary(proof, gapDescriptor) {
  const summary = proof.summary ?? {};
  if (
    proof.schema_version !== 1 ||
    proof.generated_by !== "tools/bpsr-gap-safe-lifecycle-action-ledger.mjs" ||
    proof.game_build !== GAME_BUILD ||
    Number(proof.effect_id) !== EFFECT_ID ||
    proof.damage_relationship !== "source" ||
    proof.inputs?.rlog_gap_window_audit?.sha256 !== gapDescriptor.sha256 ||
    proof.policy?.only_complete_gap_bounded_status_windows_are_selected !== true ||
    proof.policy?.gap_safe_temporal_membership_proves_causal_formula !== false ||
    Number(summary.gap_safe_window_count) !== 29 ||
    Number(summary.selected_correlation_rows) !== 27238 ||
    Number(summary.selected_third_party_provider_rows) !== 25679 ||
    Number(summary.selected_ownership_unresolved_rows) !== 0 ||
    Number(summary.unique_damage_event_count) !== 27238 ||
    Number(summary.duplicate_damage_membership_rows) !== 0 ||
    String(summary.selected_reported_amount_membership_sum) !== "3501998794" ||
    proof.conclusion?.exact_gap_safe_source_side_membership_conserved !== true ||
    proof.conclusion?.provider_ownership_proven_for_every_selected_row !== true ||
    proof.conclusion?.third_party_provider_gap_safe_correlations_available !== true ||
    proof.conclusion?.magnitude_or_formula_proven !== false ||
    proof.conclusion?.provider_rdps_credit_allowed !== false ||
    proof.conclusion?.runtime_promotion_allowed !== false ||
    proof.conclusion?.ui_rdps_display_allowed !== false
  ) fail("Fatal Spiral gap-safe lifecycle/action ownership receipt is unsafe or inconsistent");
  return {
    gap_safe_windows: Number(summary.gap_safe_window_count),
    damage_memberships: Number(summary.selected_correlation_rows),
    third_party_provider_memberships: Number(summary.selected_third_party_provider_rows),
    provider_self_memberships:
      Number(summary.selected_correlation_rows) - Number(summary.selected_third_party_provider_rows),
    ownership_unresolved_memberships: Number(summary.selected_ownership_unresolved_rows),
    unique_damage_events: Number(summary.unique_damage_event_count),
    reported_damage_membership_sum: String(summary.selected_reported_amount_membership_sum),
    source_protocol_pack_digests: structuredClone(proof.source_protocol_pack_digests ?? []),
  };
}

function validateStateProof(proof, gapDescriptor) {
  const filter = proof.gap_window_filter ?? {};
  const bundle = (proof.candidate_bundles ?? []).find(
    (entry) => entry.name === "fatal_spiral_generic_element_damage",
  );
  if (
    proof.schema_version !== 44 ||
    proof.generated_by !== "rlogs-bpsr-state-scaling-damage-proof" ||
    proof.game_build !== GAME_BUILD ||
    proof.selection?.effect_locus !== "source" ||
    JSON.stringify(proof.selection?.source_effect_ids) !== JSON.stringify([EFFECT_ID]) ||
    JSON.stringify(proof.selection?.target_effect_ids) !== "[]" ||
    proof.selection?.formula_authority !== false ||
    Number(proof.sample_count) !== 27001 ||
    Number(filter.effect_id) !== EFFECT_ID ||
    filter.effect_locus !== "source" ||
    filter.source_sha256?.toLowerCase() !== gapDescriptor.sha256 ||
    Number(filter.complete_gap_bounded_lifecycles) !== 29 ||
    Number(filter.audited_damage_events_while_active) !== 27238 ||
    Number(filter.matched_damage_events) !== 27238 ||
    Number(filter.matched_window_damage_memberships) !== 27238 ||
    filter.formula_authority !== false ||
    proof.input_determinism?.proof_authority !== true ||
    Number(proof.input_determinism?.input_groups) !== 27001 ||
    Number(proof.input_determinism?.repeated_input_groups) !== 0 ||
    !bundle ||
    bundle.locus !== "source" ||
    Number(bundle.primary_attribute_id) !== 13100 ||
    JSON.stringify(bundle.removed_attribute_ids) !== JSON.stringify([13100, 13101, 13102, 13103, 13104, 13105]) ||
    JSON.stringify(bundle.removed_source_status_effect_ids) !== JSON.stringify([EFFECT_ID]) ||
    Number(bundle.samples_with_primary_attribute) !== 6753 ||
    JSON.stringify(bundle.distinct_primary_values) !== JSON.stringify([316, 1316]) ||
    Number(bundle.strict_all_observed_state?.controlled_groups) !== 0 ||
    Number(bundle.target_current_hp_excluded_diagnostic?.controlled_groups) !== 0 ||
    Number(bundle.position_excluded_diagnostic?.controlled_groups) !== 0 ||
    Number(bundle.position_and_target_current_hp_excluded_diagnostic?.controlled_groups) !== 0 ||
    Number(bundle.position_hp_and_non_candidate_statuses_excluded_diagnostic?.controlled_groups) !== 0 ||
    Number(bundle.near_pair_diagnostics?.controlled_groups) !== 0 ||
    Number(bundle.basis_point_multiplier_check?.evaluated_normal_comparisons) !== 0 ||
    Number(bundle.basis_point_multiplier_check?.evaluated_amount_comparisons) !== 0
  ) fail("Fatal Spiral source-side state-scaling proof is unsafe or inconsistent");
  return bundle;
}

function validateCounterfactual(proof, cohortDescriptor) {
  const summary = proof.summary ?? {};
  const effect = (proof.effects ?? []).find((entry) => Number(entry.effect_id) === EFFECT_ID);
  const transitionEffect = (proof.cross_entity_source_transition_diagnostic ?? [])
    .find((entry) => Number(entry.effect_id) === EFFECT_ID && entry.locus === "source");
  const candidateProjections = (transitionEffect?.variants ?? [])
    .map((entry) => entry.all_element_damage_candidate_projection)
    .filter(Boolean);
  const zeroFields = [
    "exact_controlled_groups",
    "relaxed_controlled_groups",
    "near_controlled_target_pairs",
    "near_controlled_source_pairs",
    "cross_entity_formula_state_controlled_groups",
    "cross_entity_source_transition_controlled_pairs",
  ];
  if (
    proof.schema_version !== 17 ||
    proof.generated_by !== "rlogs-bpsr-status-effect-counterfactual-proof" ||
    proof.game_build !== GAME_BUILD ||
    proof.policy?.formula_authority !== false ||
    proof.policy?.runtime_authority !== false ||
    proof.policy?.all_element_damage_candidate_projection_authority !== false ||
    !String(proof.policy?.all_element_damage_candidate_projection_rule ?? "")
      .includes("effect 2110125") ||
    proof.policy?.structurally_absent_remote_skill_cast_packets_required !== false ||
    Number(summary.samples) !== 27001 ||
    Number(summary.distinct_effect_loci) !== 1 ||
    zeroFields.some((field) => Number(summary[field]) !== 0) ||
    Number(proof.processing?.memory_limit_mib) !== 512 ||
    proof.processing?.cross_entity_formula_state_diagnostic_enabled !== true ||
    JSON.stringify(proof.processing?.selected_source_transition_attribute_ids) !==
      JSON.stringify([13100, 13101, 13102, 13103, 13104, 13105]) ||
    proof.processing?.measured_peak_within_configured_limit !== true ||
    proof.input?.sha256?.replace(/^sha256:/, "").toLowerCase() !== cohortDescriptor.sha256 ||
    !effect || effect.locus !== "source" ||
    Number(effect.observation?.observed_samples) !== 27001 ||
    Number(effect.exact_recorded_inputs?.controlled_groups) !== 0 ||
    Number(effect.target_current_hp_excluded_diagnostic?.controlled_groups) !== 0 ||
    !transitionEffect ||
    candidateProjections.length === 0 ||
    candidateProjections.length !== (transitionEffect.variants ?? []).length ||
    candidateProjections.some((projection) =>
      projection.model_id !== "effect-2110125-source-all-element-current-final-multiplier-candidate" ||
      Number(projection.effect_id) !== EFFECT_ID ||
      Number(projection.current_attribute_id) !== 13100 ||
      Number(projection.fixed_point_denominator) !== 10000 ||
      Number(projection.deterministic_pairs) !== 0 ||
      Number(projection.deterministic_divergent_pairs) !== 0 ||
      Number(projection.pairs_with_current_attribute_transition) !== 0 ||
      Number(projection.pairs_missing_current_attribute_transition) !== 0 ||
      Number(projection.pairs_with_invalid_inputs) !== 0 ||
      JSON.stringify(projection.variants) !== JSON.stringify([
        { rounding: "floor", compatible_pairs: 0, rejected_pairs: 0 },
        { rounding: "nearest-half-up", compatible_pairs: 0, rejected_pairs: 0 },
      ]) ||
      projection.candidate_selected !== false ||
      projection.exact_damage_stage_binding_proven !== false ||
      projection.exact_operation_order_proven !== false ||
      projection.exact_integer_rounding_proven !== false ||
      projection.conservation_proven !== false ||
      projection.formula_authority !== false ||
      projection.runtime_authority !== false ||
      projection.ui_display_authority !== false ||
      projection.provider_rdps_credit_allowed !== false)
  ) fail("Fatal Spiral counterfactual frontier is unsafe or inconsistent");
  return {
    variant_count: candidateProjections.length,
    deterministic_pairs: candidateProjections.reduce(
      (sum, projection) => sum + Number(projection.deterministic_pairs), 0,
    ),
    compatible_floor_pairs: candidateProjections.reduce(
      (sum, projection) => sum + Number(projection.variants[0].compatible_pairs), 0,
    ),
    compatible_nearest_half_up_pairs: candidateProjections.reduce(
      (sum, projection) => sum + Number(projection.variants[1].compatible_pairs), 0,
    ),
  };
}

function validateConsumerFrontier(proof) {
  const closure = proof.proof_closure ?? {};
  if (
    proof.schema_version !== 3 ||
    proof.generated_by !== "tools/bpsr-all-element-damage-consumer-frontier.mjs" ||
    proof.game_build !== GAME_BUILD ||
    JSON.stringify(proof.identity?.attribute_family) !==
      JSON.stringify([13100, 13101, 13102, 13103, 13104, 13105]) ||
    Number(proof.identity?.fixed_point_denominator) !== 10000 ||
    proof.policy?.absence_of_direct_calls_proves_no_indirect_consumer !== false ||
    proof.policy?.packet_state_equations_are_damage_stage_equations !== false ||
    proof.policy?.provider_rdps_credit_allowed !== false ||
    closure.exact_current_build_family_identity_proven !== true ||
    closure.exact_fixed_point_state_equations_proven !== true ||
    closure.generated_lua_name_search_exhausted !== true ||
    closure.selected_native_direct_call_search_exhausted !== true ||
    closure.server_damage_operator_present_in_reviewed_client_static_inventory !== false ||
    closure.executable_all_element_damage_consumer_proven !== false ||
    closure.exact_native_immediate_family_search_exhausted !== true ||
    closure.combat_relevant_exact_family_immediate_consumer_found !== false ||
    closure.exact_build_generic_instantiation_indexed !== true ||
    closure.bounded_direct_getter_call_search_exhausted !== true ||
    closure.combat_relevant_literal_attribute_getter_consumer_found !== false ||
    closure.exact_method_pointer_slot_inventory_complete !== true ||
    closure.exact_rip_relative_slot_reference_search_exhausted !== true ||
    closure.indexed_metadata_dispatch_or_protected_consumer_excluded !== false ||
    closure.computed_indirect_table_driven_or_protected_consumer_excluded !== false ||
    closure.operation_order_proven !== false ||
    closure.integer_rounding_proven !== false ||
    closure.formula_authority !== false ||
    closure.runtime_authority !== false ||
    closure.ui_display_authority !== false ||
    closure.provider_rdps_credit_allowed !== false ||
    proof.acquisition_frontier?.structurally_absent_remote_cast_packets_required !== false
  ) fail("All-element damage-consumer frontier is unsafe or inconsistent");
}

function validateControlledPairWorklist(proof, consumerDescriptor) {
  const closure = proof.proof_closure ?? {};
  if (
    proof.schema_version !== 3 ||
    proof.generated_by !== "tools/bpsr-fatal-spiral-controlled-pair-worklist.mjs" ||
    proof.game_build !== GAME_BUILD ||
    Number(proof.identity?.effect_id) !== EFFECT_ID ||
    Number(proof.identity?.provider_marker_effect_id) !== PROVIDER_EFFECT_ID ||
    JSON.stringify(proof.identity?.attribute_family) !==
      JSON.stringify([13100, 13101, 13102, 13103, 13104, 13105]) ||
    proof.inputs?.all_element_damage_consumer_frontier?.sha256 !== consumerDescriptor.sha256 ||
    proof.topology?.source_side_join !== "effect endpoint equals damage actor" ||
    proof.topology?.target_allegiance_assumed !== false ||
    proof.policy?.remote_player_cast_packets_required !== false ||
    proof.policy?.missing_remote_cast_packets_are_zero !== false ||
    proof.policy?.compatibility_with_a_candidate_is_formula_authority !== false ||
    proof.policy?.provider_rdps_credit_allowed !== false ||
    Number(proof.current_evidence?.controlled_pairs_available) !== 0 ||
    proof.current_evidence?.bounded_direct_getter_call_search_exhausted !== true ||
    proof.current_evidence?.combat_relevant_literal_attribute_getter_consumer_found !== false ||
    proof.current_evidence?.exact_method_pointer_slot_inventory_complete !== true ||
    proof.current_evidence?.exact_rip_relative_slot_reference_search_exhausted !== true ||
    proof.current_evidence?.indexed_metadata_dispatch_or_protected_consumer_excluded !== false ||
    Number(proof.primary_capture_variant?.absent?.expected_attribute_13100) !== 316 ||
    Number(proof.primary_capture_variant?.present?.expected_attribute_13100) !== 1316 ||
    Number(proof.exact_integer_discriminator?.baseline_factor) !== 10316 ||
    Number(proof.exact_integer_discriminator?.tier_5_present_factor) !== 11316 ||
    closure.exact_capture_contract_defined !== true ||
    closure.exact_integer_candidate_discriminator_defined !== true ||
    closure.current_controlled_pairs_available !== false ||
    closure.combat_damage_stage_consumer_proven !== false ||
    closure.exact_operation_order_proven !== false ||
    closure.exact_integer_rounding_proven !== false ||
    closure.formula_authority !== false ||
    closure.runtime_authority !== false ||
    closure.ui_display_authority !== false ||
    closure.provider_rdps_credit_allowed !== false
  ) fail("Fatal Spiral controlled-pair worklist is unsafe or inconsistent");
}

function validateComparisonExhaustion(proof, stateDescriptor) {
  const coverage = proof.coverage ?? {};
  const closure = proof.proof_closure ?? {};
  if (
    proof.schema_version !== 1 ||
    proof.generated_by !== "tools/bpsr-fatal-spiral-comparison-exhaustion.mjs" ||
    proof.game_build !== GAME_BUILD ||
    Number(proof.identity?.effect_id) !== EFFECT_ID ||
    JSON.stringify(proof.identity?.all_element_attribute_ids) !==
      JSON.stringify([13100, 13101, 13102, 13103, 13104, 13105]) ||
    Number(proof.identity?.observed_action_ids?.length) !== 92 ||
    Number(proof.identity?.high_volume_action_ids?.length) !== 10 ||
    Number(proof.identity?.remaining_action_ids?.length) !== 82 ||
    proof.inputs?.state_scaling_proof?.sha256 !== stateDescriptor.sha256 ||
    proof.policy?.remote_player_cast_packets_required !== false ||
    proof.policy?.packet_absence_is_zero !== false ||
    proof.policy?.unrelated_status_transitions_are_ignored !== false ||
    proof.policy?.comparison_compatibility_is_formula_authority !== false ||
    proof.policy?.provider_rdps_credit_allowed !== false ||
    Number(coverage.reviewed_current_build_rlogs) !== 26 ||
    Number(coverage.effect_observed_rlogs) !== 6 ||
    Number(coverage.observed_action_ids) !== 92 ||
    JSON.stringify(coverage.action_partition_sizes) !== JSON.stringify([10, 82]) ||
    Number(coverage.effect_observed_rlog_samples) !== 318602 ||
    Number(coverage.all_reviewed_rlog_samples) !== 488546 ||
    Number(coverage.additional_absent_search_samples) !== 169944 ||
    Number(coverage.exact_effect_present_groups) !== 68110 ||
    Number(coverage.broad_diagnostic_absent_pairs) !== 12176 ||
    Number(coverage.controlled_pairs) !== 0 ||
    Number(coverage.evaluated_integer_candidate_pairs) !== 0 ||
    Number(coverage.maximum_counterfactual_working_set_mib) > 512 ||
    closure.exact_numeric_action_inventory_partitioned_without_omission !== true ||
    closure.active_and_absent_damage_states_retained !== true ||
    closure.all_reviewed_current_build_rlogs_searched !== true ||
    closure.additional_twenty_rlogs_added_new_structural_absent_candidates !== false ||
    closure.retained_current_build_capture_frontier_exhausted !== true ||
    closure.current_controlled_pairs_available !== false ||
    closure.automatic_integer_candidate_evaluator_exercised_on_real_pair !== false ||
    closure.exact_operation_order_proven !== false ||
    closure.exact_integer_rounding_proven !== false ||
    closure.formula_authority !== false ||
    closure.runtime_authority !== false ||
    closure.ui_display_authority !== false ||
    closure.provider_rdps_credit_allowed !== false ||
    Number(closure.observed_damage_reassigned_to_provider) !== 0
  ) fail("Fatal Spiral comparison-exhaustion receipt is unsafe or inconsistent");
  return coverage;
}

function validatePartialPrefixFrontier(proof) {
  const coverage = proof.coverage ?? {};
  const state = proof.proof_state ?? {};
  if (
    proof.schema_version !== 1 ||
    proof.generated_by !== "tools/bpsr-fatal-spiral-partial-prefix-frontier.mjs" ||
    proof.game_build !== GAME_BUILD ||
    Number(proof.effect_id) !== EFFECT_ID ||
    proof.policy?.original_partial_rlogs_are_read_only !== true ||
    proof.policy?.recovered_seals_authenticate_transformation_only !== true ||
    proof.policy?.packet_absence_is_zero !== false ||
    proof.policy?.formula_authority !== false ||
    proof.policy?.provider_rdps_credit_allowed !== false ||
    Number(coverage.partial_input_count) !== 10 ||
    Number(coverage.recovered_nonempty_input_count) !== 9 ||
    Number(coverage.validated_prefix_events) !== 1039616 ||
    Number(coverage.derived_terminal_gap_events) !== 9 ||
    Number(coverage.recovered_canonical_events) !== 1039625 ||
    Number(coverage.complete_gap_bounded_lifecycles) !== 23 ||
    Number(coverage.safe_source_damage_memberships) !== 16376 ||
    Number(coverage.selected_damage_action_id_count) !== 89 ||
    Number(coverage.comparison_samples) !== 92161 ||
    Number(coverage.controlled_pairs) !== 0 ||
    Number(coverage.review_band_pairs) !== 57 ||
    Number(coverage.review_band_rejected_without_source_attribute_transition) !== 57 ||
    state.retained_partial_prefix_search_exhausted !== true ||
    state.source_capture_integrity_seal_authority !== false ||
    state.controlled_counterfactual_pair_found !== false ||
    state.formula_proven !== false ||
    state.runtime_authority !== false ||
    state.provider_rdps_credit_allowed !== false
  ) fail("Fatal Spiral recovered partial-prefix frontier is unsafe or inconsistent");
  return coverage;
}

function validateControlledCaptureClientFrontier(proof, consumerProof) {
  const closure = proof.proof_closure ?? {};
  if (
    proof.schema_version !== 1 ||
    proof.generated_by !==
      "tools/bpsr-fatal-spiral-controlled-capture-client-frontier.mjs" ||
    proof.game_build !== GAME_BUILD ||
    Number(proof.identity?.effect_id) !== EFFECT_ID ||
    proof.identity?.game_assembly_sha256 !== consumerProof.identity?.game_assembly_sha256 ||
    proof.policy?.hidden_client_controls_are_user_authorization !== false ||
    proof.policy?.blocked_gm_routes_may_be_bypassed !== false ||
    proof.policy?.server_acceptance_is_assumed !== false ||
    proof.policy?.provider_rdps_credit_allowed !== false ||
    closure.exact_build_hidden_damage_control_surface_present !== true ||
    closure.exact_buff_attribute_target_and_skill_controls_present !== true ||
    closure.server_gm_command_submission_route_present !== true ||
    closure.shipping_client_blocks_gm_submission !== true ||
    closure.ordinary_production_account_server_authorization_proven !== false ||
    closure.controlled_capture_currently_executable !== false ||
    closure.bypass_of_client_or_server_guards_authorized !== false ||
    closure.current_controlled_pairs_available !== false ||
    closure.formula_authority !== false ||
    closure.runtime_authority !== false ||
    closure.ui_display_authority !== false ||
    closure.provider_rdps_credit_allowed !== false ||
    Number(closure.observed_damage_reassigned_to_provider) !== 0
  ) fail("Fatal Spiral controlled-capture client frontier is unsafe or inconsistent");
  return proof.reviewed_client_surface;
}

function validateTrainingSceneAccessFrontier(proof) {
  const closure = proof.proof_closure ?? {};
  if (
    proof.schema_version !== 1 ||
    proof.generated_by !== "tools/bpsr-training-scene-access-frontier.mjs" ||
    proof.game_build !== GAME_BUILD ||
    JSON.stringify(proof.identity?.training_scene_ids) !== JSON.stringify([10001, 10002]) ||
    proof.policy?.absence_of_a_reviewed_route_is_server_denial_proof !== false ||
    proof.policy?.empty_map_entry_conditions_prove_ordinary_access !== false ||
    proof.policy?.hidden_gm_controls_are_user_authorization !== false ||
    proof.policy?.client_or_server_guards_may_be_bypassed !== false ||
    proof.policy?.remote_player_cast_packets_required !== false ||
    proof.policy?.provider_rdps_credit_allowed !== false ||
    Number(proof.reviewed_client_evidence?.lua_chunks_scanned) !== 4821 ||
    Number(proof.reviewed_client_evidence?.lua_parse_failures) !== 0 ||
    JSON.stringify(proof.reviewed_client_evidence?.training_hall_identifier_files) !==
      JSON.stringify(["dmg_control_view.lua", "Global.lua"]) ||
    proof.acquisition_decision?.scenes_10001_10002_are_currently_executable_capture_routes !== false ||
    closure.exact_build_training_scene_identity_proven !== true ||
    closure.decoded_dungeon_entry_route_found !== false ||
    closure.ordinary_ui_or_service_lua_entry_route_found !== false ||
    closure.ordinary_production_access_proven !== false ||
    closure.hidden_gm_entry_route_present !== true ||
    closure.shipping_client_blocks_hidden_route !== true ||
    closure.authorized_controlled_capture_route_currently_executable !== false ||
    closure.formula_authority !== false ||
    closure.runtime_authority !== false ||
    closure.ui_display_authority !== false ||
    closure.provider_rdps_credit_allowed !== false ||
    Number(closure.observed_damage_reassigned_to_provider) !== 0
  ) fail("Training-scene access frontier is unsafe or inconsistent");
  return {
    training_scene_ids: [10001, 10002],
    lua_chunks_scanned: 4821,
    lua_parse_failures: 0,
    training_hall_identifier_files: ["dmg_control_view.lua", "Global.lua"],
    decoded_dungeon_entry_route_found: false,
    ordinary_ui_or_service_lua_entry_route_found: false,
    ordinary_production_access_proven: false,
    hidden_gm_entry_route_present: true,
    shipping_client_blocks_hidden_route: true,
    authorized_controlled_capture_route_currently_executable: false,
  };
}

function validateCandidateReadinessFrontier(proof) {
  const closure = proof.proof_closure ?? {};
  const evidence = proof.reviewed_evidence ?? {};
  if (
    proof.schema_version !== 6 ||
    proof.generated_by !== "tools/bpsr-fatal-spiral-candidate-readiness-frontier.mjs" ||
    proof.game_build !== GAME_BUILD || Number(proof.identity?.effect_id) !== EFFECT_ID ||
    proof.identity?.damage_relationship !== "source" ||
    proof.identity?.effect_endpoint_role !== "damage_actor" ||
    proof.policy?.remote_player_cast_packets_are_required !== false ||
    proof.policy?.packet_absence_is_zero !== false ||
    proof.policy?.new_sealed_candidate_is_controlled_pair_proof !== false ||
    proof.policy?.source_transition_near_pair_is_formula_proof !== false ||
    proof.policy?.configured_endpoint_attribute_family_exclusion_is_diagnostic_only !== true ||
    proof.policy?.exact_build_spatial_attribute_names_are_evidence_only !== true ||
    proof.policy?.spatial_attributes_are_not_excluded_from_counterfactual_matching !== true ||
    proof.policy?.component_index_and_count_are_formula_identity !== false ||
    proof.policy?.packet_source_mismatches_are_preserved_as_unresolved !== true ||
    proof.policy?.future_capture_capability_is_historical_observation_proof !== false ||
    proof.policy?.enclosing_aoi_entity_is_provider_without_separate_proof !== false ||
    proof.policy?.relative_spatial_relation_tolerances_are_diagnostic_only !== true ||
    proof.policy?.relative_spatial_relation_equality_is_not_full_spatial_equivalence_proof !== true ||
    proof.policy?.provider_rdps_credit_allowed !== false ||
    Number(evidence.complete_gap_bounded_lifecycles) !== 29 ||
    Number(evidence.damage_events_while_active) !== 27238 ||
    Number(evidence.current_new_candidate_rlogs) !== 0 ||
    Number(evidence.positive_control_new_candidate_rlogs) !== 1 ||
    evidence.positive_control_refresh_required !== true ||
    Number(evidence.source_transition_same_context_pairs) !== 229 ||
    Number(evidence.configured_endpoint_transition_pairs) !== 69 ||
    canonical(evidence.configured_endpoint_attribute_transition_counts) !==
      canonical({ 13100: 69, 13101: 69, 13102: 69 }) ||
    Number(evidence.configured_endpoint_transition_minimum_residual_dimensions) !== 13 ||
    canonical(evidence.exact_build_spatial_attribute_identities) !==
      canonical({ 52: "AttrPos", 53: "AttrTargetPos" }) ||
    canonical(evidence.spatial_attribute_observations) !==
      canonical({ 52: 132297, 53: 142877 }) ||
    canonical(evidence.spatial_attribute_position_decodes) !==
      canonical({ 52: 131558, 53: 142130 }) ||
    evidence.spatial_attributes_safe_to_exclude_from_counterfactual_matching !== false ||
    Number(evidence.source_transition_minimum_residual_observed_state_dimensions_before_configured_endpoint_diagnostic) !== 14 ||
    Number(evidence.source_transition_minimum_residual_observed_state_dimensions) !== 13 ||
    Number(evidence.source_transition_pairs_explained_only_by_configured_endpoint_and_diagnostic_exclusions) !== 0 ||
    Number(evidence.strict_controlled_counterfactual_pairs) !== 0 ||
    Number(evidence.exact_action_selectors) !== 7 ||
    Number(evidence.packet_source_route_matched_transition_pairs) !== 68 ||
    Number(evidence.packet_source_route_rejected_transition_pairs) !== 1 ||
    Number(evidence.exact_build_static_formula_candidates) !== 6 ||
    Number(evidence.direct_spatial_relation_complete_transition_pairs) !== 66 ||
    Number(evidence.direct_spatial_relation_exact_transition_pairs) !== 2 ||
    Number(evidence.direct_spatial_relation_nonexact_transition_pairs) !== 64 ||
    Number(evidence.direct_spatial_relation_outside_one_raw_coordinate_unit_transition_pairs) !== 12 ||
    Number(evidence.fake_bullet_exact_wire_fields) !== 7 ||
    evidence.fake_bullet_future_capture_join_keys_retained !== true ||
    Number(evidence.fake_bullet_current_build_observed_lifecycle_records) !== 0 ||
    evidence.fake_bullet_source4_damage_route_resolved !== false ||
    evidence.fake_bullet_provider_ownership_proven !== false ||
    closure.recursive_sealed_rlog_discovery_bounded !== true ||
    closure.exact_build_and_canonical_seal_required !== true ||
    closure.known_seal_deduplication_complete !== true ||
    closure.unseen_seal_positive_control_triggers_refresh !== true ||
    closure.source_side_effect_endpoint_join_complete !== true ||
    Number(closure.current_new_candidate_rlogs) !== 0 ||
    closure.source_transition_candidate_search_complete !== true ||
    closure.configured_endpoint_attribute_family_diagnostic_complete !== true ||
    closure.configured_endpoint_transition_residual_ranking_complete !== true ||
    closure.exact_build_spatial_attribute_identity_proof_complete !== true ||
    closure.retained_spatial_raw_value_replay_complete !== true ||
    closure.exact_build_action_selector_roster_complete !== true ||
    Number(closure.packet_source_compatible_static_formula_candidates) !== 6 ||
    closure.packet_source_mismatch_preserved_unresolved !== true ||
    closure.relative_spatial_relation_audit_complete !== true ||
    closure.direct_source_to_target_geometry_equal_for_all_complete_pairs !== false ||
    closure.spatial_attributes_safe_to_exclude_from_counterfactual_matching !== false ||
    closure.fake_bullet_exact_wire_contract_complete !== true ||
    closure.fake_bullet_future_capture_timeline_preservation_complete !== true ||
    Number(closure.fake_bullet_current_build_observed_lifecycle_records) !== 0 ||
    closure.fake_bullet_historical_canonical_logs_backfilled !== false ||
    closure.fake_bullet_source4_damage_route_resolved !== false ||
    closure.fake_bullet_provider_ownership_proven !== false ||
    Number(closure.current_strict_controlled_pairs) !== 0 ||
    closure.exact_operation_order_proven !== false ||
    closure.exact_integer_rounding_proven !== false ||
    closure.packet_conservation_proven !== false ||
    closure.formula_authority !== false || closure.runtime_authority !== false ||
    closure.ui_display_authority !== false || closure.provider_rdps_credit_allowed !== false
  ) fail("Fatal Spiral candidate-readiness frontier is unsafe or inconsistent");
  return {
    recursive_sealed_rlog_discovery_bounded: true,
    exact_build_and_canonical_seal_required: true,
    known_seal_deduplication_complete: true,
    unseen_seal_positive_control_triggers_refresh: true,
    source_side_effect_endpoint_join_complete: true,
    configured_endpoint_attribute_family_diagnostic_complete: true,
    configured_endpoint_transition_residual_ranking_complete: true,
    exact_build_spatial_attribute_identity_proof_complete: true,
    retained_spatial_raw_value_replay_complete: true,
    exact_build_action_selector_roster_complete: true,
    packet_source_compatible_static_formula_candidates: 6,
    packet_source_mismatch_preserved_unresolved: true,
    relative_spatial_relation_audit_complete: true,
    direct_source_to_target_geometry_equal_for_all_complete_pairs: false,
    spatial_attributes_safe_to_exclude_from_counterfactual_matching: false,
    current_new_candidate_rlogs: 0,
    source_transition_same_context_pairs: 229,
    configured_endpoint_transition_pairs: 69,
    configured_endpoint_attribute_transition_counts: { 13100: 69, 13101: 69, 13102: 69 },
    configured_endpoint_transition_minimum_residual_dimensions: 13,
    exact_build_spatial_attribute_identities: { 52: "AttrPos", 53: "AttrTargetPos" },
    spatial_attribute_observations: { 52: 132297, 53: 142877 },
    spatial_attribute_position_decodes: { 52: 131558, 53: 142130 },
    source_transition_minimum_residual_observed_state_dimensions_before_configured_endpoint_diagnostic: 14,
    source_transition_minimum_residual_observed_state_dimensions: 13,
    source_transition_pairs_explained_only_by_configured_endpoint_and_diagnostic_exclusions: 0,
    current_strict_controlled_pairs: 0,
    exact_action_selectors: 7,
    packet_source_route_matched_transition_pairs: 68,
    packet_source_route_rejected_transition_pairs: 1,
    direct_spatial_relation_complete_transition_pairs: 66,
    direct_spatial_relation_exact_transition_pairs: 2,
    direct_spatial_relation_nonexact_transition_pairs: 64,
    direct_spatial_relation_outside_one_raw_coordinate_unit_transition_pairs: 12,
    fake_bullet_exact_wire_fields: 7,
    fake_bullet_future_capture_join_keys_retained: true,
    fake_bullet_current_build_observed_lifecycle_records: 0,
    fake_bullet_source4_damage_route_resolved: false,
    fake_bullet_provider_ownership_proven: false,
  };
}

function build(options) {
  const tierFile = path.resolve(options.tierWindowProof);
  const gapFile = path.resolve(options.gapWindowAudit);
  const gapSafeLifecycleActionFile = path.resolve(options.gapSafeLifecycleActionSummary);
  const stateFile = path.resolve(options.stateScalingProof);
  const counterFile = path.resolve(options.counterfactualFrontier);
  const cohortFile = path.resolve(options.formulaCohort);
  const consumerFile = path.resolve(options.damageConsumerFrontier);
  const worklistFile = path.resolve(options.controlledPairWorklist);
  const comparisonFile = path.resolve(options.comparisonExhaustion);
  const partialPrefixFile = path.resolve(options.partialPrefixFrontier);
  const controlledCaptureFile = path.resolve(options.controlledCaptureClientFrontier);
  const trainingSceneAccessFile = path.resolve(options.trainingSceneAccessFrontier);
  const candidateReadinessFile = path.resolve(options.candidateReadinessFrontier);
  const tier = readJson(tierFile, "tier/window proof");
  const gap = readJson(gapFile, "gap-window audit");
  const gapSafeLifecycleAction = readJson(
    gapSafeLifecycleActionFile,
    "gap-safe lifecycle/action ownership summary",
  );
  const state = readJson(stateFile, "state-scaling proof");
  const counter = readJson(counterFile, "counterfactual frontier");
  const consumer = readJson(consumerFile, "all-element damage-consumer frontier");
  const worklist = readJson(worklistFile, "Fatal Spiral controlled-pair worklist");
  const comparison = readJson(comparisonFile, "Fatal Spiral comparison-exhaustion receipt");
  const partialPrefix = readJson(partialPrefixFile, "Fatal Spiral partial-prefix frontier");
  const controlledCapture = readJson(
    controlledCaptureFile,
    "Fatal Spiral controlled-capture client frontier",
  );
  const trainingSceneAccess = readJson(
    trainingSceneAccessFile,
    "training-scene access frontier",
  );
  const candidateReadiness = readJson(
    candidateReadinessFile,
    "Fatal Spiral candidate-readiness frontier",
  );
  const descriptors = {
    tier_window_proof: descriptor(tierFile),
    source_gap_window_audit: descriptor(gapFile),
    gap_safe_lifecycle_action_summary: descriptor(gapSafeLifecycleActionFile),
    source_state_scaling_proof: descriptor(stateFile),
    source_formula_cohort: descriptor(cohortFile),
    source_counterfactual_frontier: descriptor(counterFile),
    all_element_damage_consumer_frontier: descriptor(consumerFile),
    controlled_pair_acquisition_worklist: descriptor(worklistFile),
    retained_capture_comparison_exhaustion: descriptor(comparisonFile),
    recovered_partial_prefix_frontier: descriptor(partialPrefixFile),
    controlled_capture_client_frontier: descriptor(controlledCaptureFile),
    training_scene_access_frontier: descriptor(trainingSceneAccessFile),
    candidate_readiness_frontier: descriptor(candidateReadinessFile),
  };
  validateTierWindow(tier);
  validateGapAudit(gap);
  const gapSafeLifecycleActionReceipt = validateGapSafeLifecycleActionSummary(
    gapSafeLifecycleAction,
    descriptors.source_gap_window_audit,
  );
  const bundle = validateStateProof(state, descriptors.source_gap_window_audit);
  const candidateEvaluation = validateCounterfactual(counter, descriptors.source_formula_cohort);
  validateConsumerFrontier(consumer);
  validateControlledPairWorklist(worklist, descriptors.all_element_damage_consumer_frontier);
  const comparisonCoverage = validateComparisonExhaustion(
    comparison,
    descriptors.source_state_scaling_proof,
  );
  const partialPrefixCoverage = validatePartialPrefixFrontier(partialPrefix);
  const controlledCaptureSurface = validateControlledCaptureClientFrontier(
    controlledCapture,
    consumer,
  );
  const trainingSceneAccessReceipt = validateTrainingSceneAccessFrontier(trainingSceneAccess);
  const candidateReadinessReceipt = validateCandidateReadinessFrontier(candidateReadiness);
  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game: "blue-protocol-star-resonance",
    game_build: GAME_BUILD,
    identity: {
      imagine_skill_id: 3957,
      component_id: "fatal-spiral-shared-all-element-bonus",
      provider_marker_effect_id: PROVIDER_EFFECT_ID,
      effect_id: EFFECT_ID,
      generic_element_attribute_id: 13100,
      generic_element_attribute_family: [13100, 13101, 13102, 13103, 13104, 13105],
      fixed_point_denominator: 10000,
    },
    topology: tier.topology,
    policy: {
      source_and_target_endpoints_are_independent: true,
      endpoint_allegiance_is_inferred: false,
      remote_player_cast_packets_are_required: false,
      packet_absence_is_zero: false,
      current_snapshots_may_rewrite_historical_runs: false,
      lifecycle_completeness_is_formula_authority: false,
      observed_attribute_consistency_is_formula_authority: false,
      exact_build_spatial_attribute_names_are_evidence_only: true,
      spatial_attributes_are_not_excluded_from_counterfactual_matching: true,
      component_index_and_count_are_formula_identity: false,
      packet_source_mismatches_are_preserved_as_unresolved: true,
      future_capture_capability_is_historical_observation_proof: false,
      enclosing_aoi_entity_is_provider_without_separate_proof: false,
      relative_spatial_relation_tolerances_are_diagnostic_only: true,
      relative_spatial_relation_equality_is_not_full_spatial_equivalence_proof: true,
      provider_rdps_credit_allowed: false,
    },
    inputs: descriptors,
    reviewed_evidence: {
      event_time_tier_window_frontier: {
        status_rows: tier.summary.status_rows,
        applied_rows: tier.summary.applied_rows,
        removed_rows: tier.summary.removed_rows,
        closed_windows: tier.summary.closed_windows,
        open_windows: tier.summary.open_windows,
        unique_source_side_damage_events: tier.summary.unique_source_side_damage_events,
        strict_single_external_elemental_candidate_events:
          tier.summary.strict_single_external_elemental_candidate_events,
      },
      gap_bounded_source_lifecycles: {
        complete_lifecycles: gap.summary.selected_effect_complete_gap_bounded_lifecycle_count,
        lifecycles_cut_by_data_quality_boundary:
          gap.summary.selected_effect_lifecycles_cut_by_data_quality_boundary,
        audited_damage_event_memberships: gap.summary.selected_effect_damage_events_while_active,
        replay_matched_damage_event_memberships: state.gap_window_filter.matched_window_damage_memberships,
      },
      ownership_resolved_gap_safe_source_memberships: gapSafeLifecycleActionReceipt,
      source_formula_cohort: {
        samples: state.sample_count,
        samples_with_generic_element_attribute: bundle.samples_with_primary_attribute,
        observed_generic_element_attribute_values: bundle.distinct_primary_values,
        input_groups: state.input_determinism.input_groups,
        repeated_input_groups: state.input_determinism.repeated_input_groups,
      },
      counterfactual_exhaustion: {
        samples: counter.summary.samples,
        exact_controlled_groups: counter.summary.exact_controlled_groups,
        relaxed_controlled_groups: counter.summary.relaxed_controlled_groups,
        near_controlled_target_pairs: counter.summary.near_controlled_target_pairs,
        near_controlled_source_pairs: counter.summary.near_controlled_source_pairs,
        measured_peak_working_set_mib: counter.processing.measured_peak_working_set_mib,
        configured_memory_limit_mib: counter.processing.memory_limit_mib,
      },
      automated_integer_candidate_evaluation: {
        analyzer_schema_version: counter.schema_version,
        model_id: "effect-2110125-source-all-element-current-final-multiplier-candidate",
        fixed_point_denominator: 10000,
        evaluated_variant_count: candidateEvaluation.variant_count,
        deterministic_pairs: candidateEvaluation.deterministic_pairs,
        compatible_floor_pairs: candidateEvaluation.compatible_floor_pairs,
        compatible_nearest_half_up_pairs:
          candidateEvaluation.compatible_nearest_half_up_pairs,
        candidate_selected: false,
      },
      exact_build_consumer_search: {
        exact_current_build_family_identity_proven:
          consumer.proof_closure.exact_current_build_family_identity_proven,
        exact_fixed_point_state_equations_proven:
          consumer.proof_closure.exact_fixed_point_state_equations_proven,
        generated_lua_name_search_exhausted:
          consumer.proof_closure.generated_lua_name_search_exhausted,
        selected_native_direct_call_search_exhausted:
          consumer.proof_closure.selected_native_direct_call_search_exhausted,
        server_damage_operator_present_in_reviewed_client_static_inventory:
          consumer.proof_closure.server_damage_operator_present_in_reviewed_client_static_inventory,
        executable_all_element_damage_consumer_proven:
          consumer.proof_closure.executable_all_element_damage_consumer_proven,
        exact_native_immediate_family_search_exhausted:
          consumer.proof_closure.exact_native_immediate_family_search_exhausted,
        combat_relevant_exact_family_immediate_consumer_found:
          consumer.proof_closure.combat_relevant_exact_family_immediate_consumer_found,
        exact_build_generic_instantiation_indexed:
          consumer.proof_closure.exact_build_generic_instantiation_indexed,
        bounded_direct_getter_call_search_exhausted:
          consumer.proof_closure.bounded_direct_getter_call_search_exhausted,
        combat_relevant_literal_attribute_getter_consumer_found:
          consumer.proof_closure.combat_relevant_literal_attribute_getter_consumer_found,
        exact_method_pointer_slot_inventory_complete:
          consumer.proof_closure.exact_method_pointer_slot_inventory_complete,
        exact_rip_relative_slot_reference_search_exhausted:
          consumer.proof_closure.exact_rip_relative_slot_reference_search_exhausted,
        indexed_metadata_dispatch_or_protected_consumer_excluded:
          consumer.proof_closure.indexed_metadata_dispatch_or_protected_consumer_excluded,
        computed_indirect_table_driven_or_protected_consumer_excluded:
          consumer.proof_closure.computed_indirect_table_driven_or_protected_consumer_excluded,
      },
      controlled_pair_acquisition: {
        exact_capture_contract_defined:
          worklist.proof_closure.exact_capture_contract_defined,
        exact_integer_candidate_discriminator_defined:
          worklist.proof_closure.exact_integer_candidate_discriminator_defined,
        current_controlled_pairs_available:
          worklist.proof_closure.current_controlled_pairs_available,
        primary_absent_attribute_value:
          worklist.primary_capture_variant.absent.expected_attribute_13100,
        primary_present_attribute_value:
          worklist.primary_capture_variant.present.expected_attribute_13100,
        primary_attribute_delta:
          worklist.primary_capture_variant.expected_attribute_delta.delta_each,
      },
      retained_capture_comparison_exhaustion: {
        reviewed_current_build_rlogs: comparisonCoverage.reviewed_current_build_rlogs,
        effect_observed_rlogs: comparisonCoverage.effect_observed_rlogs,
        observed_action_ids: comparisonCoverage.observed_action_ids,
        effect_observed_rlog_samples: comparisonCoverage.effect_observed_rlog_samples,
        all_reviewed_rlog_samples: comparisonCoverage.all_reviewed_rlog_samples,
        additional_absent_search_samples: comparisonCoverage.additional_absent_search_samples,
        exact_effect_present_groups: comparisonCoverage.exact_effect_present_groups,
        broad_diagnostic_absent_pairs: comparisonCoverage.broad_diagnostic_absent_pairs,
        controlled_pairs: comparisonCoverage.controlled_pairs,
        evaluated_integer_candidate_pairs:
          comparisonCoverage.evaluated_integer_candidate_pairs,
        maximum_counterfactual_working_set_mib:
          comparisonCoverage.maximum_counterfactual_working_set_mib,
      },
      recovered_partial_prefix_comparison_exhaustion: {
        partial_input_count: partialPrefixCoverage.partial_input_count,
        recovered_nonempty_input_count: partialPrefixCoverage.recovered_nonempty_input_count,
        validated_prefix_events: partialPrefixCoverage.validated_prefix_events,
        derived_terminal_gap_events: partialPrefixCoverage.derived_terminal_gap_events,
        recovered_canonical_events: partialPrefixCoverage.recovered_canonical_events,
        complete_gap_bounded_lifecycles:
          partialPrefixCoverage.complete_gap_bounded_lifecycles,
        safe_source_damage_memberships:
          partialPrefixCoverage.safe_source_damage_memberships,
        selected_damage_action_id_count:
          partialPrefixCoverage.selected_damage_action_id_count,
        comparison_samples: partialPrefixCoverage.comparison_samples,
        controlled_pairs: partialPrefixCoverage.controlled_pairs,
        review_band_pairs: partialPrefixCoverage.review_band_pairs,
        review_band_rejected_without_source_attribute_transition:
          partialPrefixCoverage.review_band_rejected_without_source_attribute_transition,
        source_capture_integrity_seal_authority: false,
      },
      controlled_capture_client_frontier: {
        exact_server_command_templates:
          controlledCaptureSurface.exact_server_command_templates,
        selectable_control_axes: controlledCaptureSurface.selectable_control_axes,
        submission_route: controlledCaptureSurface.submission_route,
        client_gate: controlledCaptureSurface.client_gate,
        shipping_client_blocks_gm_submission: true,
        ordinary_production_account_server_authorization_proven: false,
        controlled_capture_currently_executable: false,
      },
      training_scene_access_frontier: trainingSceneAccessReceipt,
      sealed_candidate_readiness_frontier: candidateReadinessReceipt,
    },
    proof_closure: {
      exact_event_time_provider_tier_join_complete: true,
      exact_source_side_effect_recipient_to_damage_actor_join_complete: true,
      exact_gap_bounded_lifecycle_replay_complete: true,
      audited_damage_membership_selection_conserved: true,
      exact_gap_safe_lifecycle_action_membership_join_complete: true,
      provider_ownership_proven_for_all_gap_safe_memberships: true,
      gap_safe_damage_memberships: gapSafeLifecycleActionReceipt.damage_memberships,
      gap_safe_third_party_provider_memberships:
        gapSafeLifecycleActionReceipt.third_party_provider_memberships,
      gap_safe_provider_self_memberships: gapSafeLifecycleActionReceipt.provider_self_memberships,
      gap_safe_ownership_unresolved_memberships:
        gapSafeLifecycleActionReceipt.ownership_unresolved_memberships,
      exact_numeric_attribute_family_preserved: true,
      controlled_pair_search_exhausted_for_retained_cohort: true,
      exact_build_client_consumer_search_exhausted: true,
      exact_build_server_operator_absence_recorded: true,
      exact_native_immediate_family_search_exhausted: true,
      combat_relevant_exact_family_immediate_consumer_found: false,
      exact_build_generic_instantiation_indexed: true,
      bounded_direct_getter_call_search_exhausted: true,
      combat_relevant_literal_attribute_getter_consumer_found: false,
      exact_method_pointer_slot_inventory_complete: true,
      exact_rip_relative_slot_reference_search_exhausted: true,
      indexed_metadata_dispatch_or_protected_consumer_excluded: false,
      computed_indirect_table_driven_or_protected_consumer_excluded: false,
      exact_controlled_pair_acquisition_contract_defined: true,
      exact_integer_candidate_discriminator_defined: true,
      automatic_integer_candidate_evaluator_integrated: true,
      retained_current_build_present_and_absent_capture_frontier_exhausted: true,
      retained_recovered_partial_prefix_frontier_exhausted: true,
      recovered_partial_prefix_source_capture_integrity_seal_authority: false,
      exact_build_hidden_controlled_capture_surface_identified: true,
      shipping_client_blocks_controlled_capture_submission: true,
      ordinary_production_account_controlled_capture_authorized: false,
      exact_build_training_scene_access_frontier_reviewed: true,
      ordinary_training_scene_entry_route_proven: false,
      training_scene_controlled_capture_currently_executable: false,
      current_controlled_pairs_available: false,
      recursive_sealed_rlog_candidate_discovery_bounded: true,
      exact_build_and_canonical_seal_candidate_gate_complete: true,
      known_candidate_seal_deduplication_complete: true,
      unseen_seal_positive_control_triggers_refresh: true,
      source_transition_candidate_search_complete: true,
      configured_endpoint_attribute_family_diagnostic_complete: true,
      configured_endpoint_transition_residual_ranking_complete: true,
      exact_build_spatial_attribute_identity_proof_complete: true,
      retained_spatial_raw_value_replay_complete: true,
      exact_build_action_selector_roster_complete: true,
      packet_source_compatible_static_formula_candidates: 6,
      packet_source_mismatch_preserved_unresolved: true,
      relative_spatial_relation_audit_complete: true,
      direct_source_to_target_geometry_equal_for_all_complete_pairs: false,
      spatial_attributes_safe_to_exclude_from_counterfactual_matching: false,
      fake_bullet_exact_wire_contract_complete: true,
      fake_bullet_future_capture_timeline_preservation_complete: true,
      fake_bullet_current_build_observed_lifecycle_records: 0,
      fake_bullet_historical_canonical_logs_backfilled: false,
      fake_bullet_source4_damage_route_resolved: false,
      fake_bullet_provider_ownership_proven: false,
      current_new_sealed_candidate_rlogs: 0,
      current_source_transition_same_context_pairs: 229,
      current_configured_endpoint_transition_pairs: 69,
      current_configured_endpoint_transition_minimum_residual_dimensions: 13,
      current_source_transition_minimum_residual_observed_state_dimensions_before_configured_endpoint_diagnostic: 14,
      current_source_transition_minimum_residual_observed_state_dimensions: 13,
      current_source_transition_pairs_explained_only_by_configured_endpoint_and_diagnostic_exclusions: 0,
      current_source_transition_strict_controlled_pairs: 0,
      current_exact_action_selectors: 7,
      current_packet_source_route_matched_transition_pairs: 68,
      current_packet_source_route_rejected_transition_pairs: 1,
      current_direct_spatial_relation_complete_transition_pairs: 66,
      current_direct_spatial_relation_exact_transition_pairs: 2,
      current_direct_spatial_relation_nonexact_transition_pairs: 64,
      current_direct_spatial_relation_outside_one_raw_coordinate_unit_transition_pairs: 12,
      combat_damage_stage_consumer_proven: false,
      exact_multiplier_application_proven: false,
      exact_operation_order_proven: false,
      exact_integer_rounding_proven: false,
      multi_provider_stacking_and_split_proven: false,
      integer_damage_counterfactual_projection_complete: false,
      recipient_debit_provider_credit_conservation_complete: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
      observed_damage_reassigned_to_provider: 0,
    },
    remaining_proof_obligations: [
      "obtain the authoritative server damage operator or an instruction-level equivalent consumer for generic element attributes 13100 through 13105",
      "do not promote recovered partial-prefix evidence: its derived seals authenticate the transformation, not the original capture",
      "obtain an explicitly authorized internal/QA session for the exact-build damage-control surface or an organic same-build controlled effect-present/effect-absent pair",
      "rerun the sealed-candidate manifest when new RLOGs arrive; any unseen exact-build seal with a complete source-side damage window must traverse the gap-window and transition audits before exact-integer evaluation",
      "retain AttrPos 52 and AttrTargetPos 53 as exact matching dimensions until distance, direction, area, falloff, and other spatial damage consequences are independently excluded for the compared action",
      "capture exact-build FakeBullets beside source-4 action 220101 damage and prove the enclosing AOI entity provider relation before treating the preserved lifecycle as an attribution edge",
      "do not treat scenes 10001 or 10002 as executable capture routes unless ordinary or explicitly authorized access is independently observed",
      "prove multiplier operation order and integer rounding from that controlled pair or an equivalent executable consumer",
      "prove overlapping-provider stacking and provider split semantics",
      "project eligible damage with and without the provider contribution using exact integers",
      "replay equal recipient debit and provider credit without changing ordinary damage totals",
      "keep the exact-build protocol-pack conservation and event-coverage gates satisfied while formula, rounding, and attribution proofs remain fail-closed",
    ],
    content_sha256: "",
  };
  report.content_sha256 = digest(report);
  verify(report);
  return report;
}

function verify(report) {
  if (
    report.schema_version !== SCHEMA_VERSION ||
    report.generated_by !== GENERATED_BY ||
    report.game_build !== GAME_BUILD ||
    Number(report.identity?.effect_id) !== EFFECT_ID ||
    report.policy?.exact_build_spatial_attribute_names_are_evidence_only !== true ||
    report.policy?.spatial_attributes_are_not_excluded_from_counterfactual_matching !== true ||
    report.policy?.component_index_and_count_are_formula_identity !== false ||
    report.policy?.packet_source_mismatches_are_preserved_as_unresolved !== true ||
    report.policy?.future_capture_capability_is_historical_observation_proof !== false ||
    report.policy?.enclosing_aoi_entity_is_provider_without_separate_proof !== false ||
    report.policy?.relative_spatial_relation_tolerances_are_diagnostic_only !== true ||
    report.policy?.relative_spatial_relation_equality_is_not_full_spatial_equivalence_proof !== true ||
    report.policy?.provider_rdps_credit_allowed !== false ||
    report.proof_closure?.audited_damage_membership_selection_conserved !== true ||
    report.proof_closure?.exact_gap_safe_lifecycle_action_membership_join_complete !== true ||
    report.proof_closure?.provider_ownership_proven_for_all_gap_safe_memberships !== true ||
    Number(report.proof_closure?.gap_safe_damage_memberships) !== 27238 ||
    Number(report.proof_closure?.gap_safe_third_party_provider_memberships) !== 25679 ||
    Number(report.proof_closure?.gap_safe_provider_self_memberships) !== 1559 ||
    Number(report.proof_closure?.gap_safe_ownership_unresolved_memberships) !== 0 ||
    report.proof_closure?.exact_build_client_consumer_search_exhausted !== true ||
    report.proof_closure?.exact_build_server_operator_absence_recorded !== true ||
    report.proof_closure?.exact_native_immediate_family_search_exhausted !== true ||
    report.proof_closure?.combat_relevant_exact_family_immediate_consumer_found !== false ||
    report.proof_closure?.exact_build_generic_instantiation_indexed !== true ||
    report.proof_closure?.bounded_direct_getter_call_search_exhausted !== true ||
    report.proof_closure?.combat_relevant_literal_attribute_getter_consumer_found !== false ||
    report.proof_closure?.exact_method_pointer_slot_inventory_complete !== true ||
    report.proof_closure?.exact_rip_relative_slot_reference_search_exhausted !== true ||
    report.proof_closure?.indexed_metadata_dispatch_or_protected_consumer_excluded !== false ||
    report.proof_closure?.computed_indirect_table_driven_or_protected_consumer_excluded !== false ||
    report.proof_closure?.exact_controlled_pair_acquisition_contract_defined !== true ||
    report.proof_closure?.exact_integer_candidate_discriminator_defined !== true ||
    report.proof_closure?.automatic_integer_candidate_evaluator_integrated !== true ||
    report.proof_closure?.retained_current_build_present_and_absent_capture_frontier_exhausted !== true ||
    report.proof_closure?.retained_recovered_partial_prefix_frontier_exhausted !== true ||
    report.proof_closure?.recovered_partial_prefix_source_capture_integrity_seal_authority !== false ||
    report.proof_closure?.exact_build_hidden_controlled_capture_surface_identified !== true ||
    report.proof_closure?.shipping_client_blocks_controlled_capture_submission !== true ||
    report.proof_closure?.ordinary_production_account_controlled_capture_authorized !== false ||
    report.proof_closure?.exact_build_training_scene_access_frontier_reviewed !== true ||
    report.proof_closure?.ordinary_training_scene_entry_route_proven !== false ||
    report.proof_closure?.training_scene_controlled_capture_currently_executable !== false ||
    report.proof_closure?.current_controlled_pairs_available !== false ||
    report.proof_closure?.recursive_sealed_rlog_candidate_discovery_bounded !== true ||
    report.proof_closure?.exact_build_and_canonical_seal_candidate_gate_complete !== true ||
    report.proof_closure?.known_candidate_seal_deduplication_complete !== true ||
    report.proof_closure?.unseen_seal_positive_control_triggers_refresh !== true ||
    report.proof_closure?.source_transition_candidate_search_complete !== true ||
    report.proof_closure?.configured_endpoint_attribute_family_diagnostic_complete !== true ||
    report.proof_closure?.configured_endpoint_transition_residual_ranking_complete !== true ||
    report.proof_closure?.exact_build_spatial_attribute_identity_proof_complete !== true ||
    report.proof_closure?.retained_spatial_raw_value_replay_complete !== true ||
    report.proof_closure?.exact_build_action_selector_roster_complete !== true ||
    Number(report.proof_closure?.packet_source_compatible_static_formula_candidates) !== 6 ||
    report.proof_closure?.packet_source_mismatch_preserved_unresolved !== true ||
    report.proof_closure?.relative_spatial_relation_audit_complete !== true ||
    report.proof_closure?.direct_source_to_target_geometry_equal_for_all_complete_pairs !== false ||
    report.proof_closure?.spatial_attributes_safe_to_exclude_from_counterfactual_matching !== false ||
    Number(report.proof_closure?.current_new_sealed_candidate_rlogs) !== 0 ||
    Number(report.proof_closure?.current_source_transition_same_context_pairs) !== 229 ||
    Number(report.proof_closure?.current_configured_endpoint_transition_pairs) !== 69 ||
    Number(report.proof_closure?.current_configured_endpoint_transition_minimum_residual_dimensions) !== 13 ||
    Number(report.proof_closure?.current_source_transition_minimum_residual_observed_state_dimensions_before_configured_endpoint_diagnostic) !== 14 ||
    Number(report.proof_closure?.current_source_transition_minimum_residual_observed_state_dimensions) !== 13 ||
    Number(report.proof_closure?.current_source_transition_pairs_explained_only_by_configured_endpoint_and_diagnostic_exclusions) !== 0 ||
    Number(report.proof_closure?.current_source_transition_strict_controlled_pairs) !== 0 ||
    Number(report.proof_closure?.current_exact_action_selectors) !== 7 ||
    Number(report.proof_closure?.current_packet_source_route_matched_transition_pairs) !== 68 ||
    Number(report.proof_closure?.current_packet_source_route_rejected_transition_pairs) !== 1 ||
    Number(report.proof_closure?.current_direct_spatial_relation_complete_transition_pairs) !== 66 ||
    Number(report.proof_closure?.current_direct_spatial_relation_exact_transition_pairs) !== 2 ||
    Number(report.proof_closure?.current_direct_spatial_relation_nonexact_transition_pairs) !== 64 ||
    Number(report.proof_closure?.current_direct_spatial_relation_outside_one_raw_coordinate_unit_transition_pairs) !== 12 ||
    report.proof_closure?.fake_bullet_exact_wire_contract_complete !== true ||
    report.proof_closure?.fake_bullet_future_capture_timeline_preservation_complete !== true ||
    Number(report.proof_closure?.fake_bullet_current_build_observed_lifecycle_records) !== 0 ||
    report.proof_closure?.fake_bullet_historical_canonical_logs_backfilled !== false ||
    report.proof_closure?.fake_bullet_source4_damage_route_resolved !== false ||
    report.proof_closure?.fake_bullet_provider_ownership_proven !== false ||
    report.proof_closure?.combat_damage_stage_consumer_proven !== false ||
    report.proof_closure?.exact_operation_order_proven !== false ||
    report.proof_closure?.exact_integer_rounding_proven !== false ||
    report.proof_closure?.formula_authority !== false ||
    report.proof_closure?.runtime_authority !== false ||
    report.proof_closure?.ui_display_authority !== false ||
    report.proof_closure?.provider_rdps_credit_allowed !== false ||
    Number(report.proof_closure?.observed_damage_reassigned_to_provider) !== 0 ||
    report.content_sha256 !== digest(report)
  ) fail("Fatal Spiral damage-stage frontier is unsafe or has an invalid digest");
}

function parse(argv) {
  const args = {};
  for (let i = 0; i < argv.length; i += 2) {
    const flag = argv[i];
    const value = argv[i + 1];
    if (!flag?.startsWith("--") || value == null) fail(`Invalid argument ${flag ?? "<missing>"}`);
    args[flag.slice(2)] = value;
  }
  return args;
}

function required(args, name) {
  if (!args[name]) fail(`Missing --${name.replaceAll(/[A-Z]/g, (letter) => `-${letter.toLowerCase()}`)}`);
  return args[name];
}

function selfTest() {
  const sample = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game_build: GAME_BUILD,
    identity: { effect_id: EFFECT_ID },
    policy: {
      exact_build_spatial_attribute_names_are_evidence_only: true,
      spatial_attributes_are_not_excluded_from_counterfactual_matching: true,
      component_index_and_count_are_formula_identity: false,
      packet_source_mismatches_are_preserved_as_unresolved: true,
      future_capture_capability_is_historical_observation_proof: false,
      enclosing_aoi_entity_is_provider_without_separate_proof: false,
      relative_spatial_relation_tolerances_are_diagnostic_only: true,
      relative_spatial_relation_equality_is_not_full_spatial_equivalence_proof: true,
      provider_rdps_credit_allowed: false,
    },
    proof_closure: {
      audited_damage_membership_selection_conserved: true,
      exact_gap_safe_lifecycle_action_membership_join_complete: true,
      provider_ownership_proven_for_all_gap_safe_memberships: true,
      gap_safe_damage_memberships: 27238,
      gap_safe_third_party_provider_memberships: 25679,
      gap_safe_provider_self_memberships: 1559,
      gap_safe_ownership_unresolved_memberships: 0,
      exact_build_client_consumer_search_exhausted: true,
      exact_build_server_operator_absence_recorded: true,
      exact_native_immediate_family_search_exhausted: true,
      combat_relevant_exact_family_immediate_consumer_found: false,
      exact_build_generic_instantiation_indexed: true,
      bounded_direct_getter_call_search_exhausted: true,
      combat_relevant_literal_attribute_getter_consumer_found: false,
      exact_method_pointer_slot_inventory_complete: true,
      exact_rip_relative_slot_reference_search_exhausted: true,
      indexed_metadata_dispatch_or_protected_consumer_excluded: false,
      computed_indirect_table_driven_or_protected_consumer_excluded: false,
      exact_controlled_pair_acquisition_contract_defined: true,
      exact_integer_candidate_discriminator_defined: true,
      automatic_integer_candidate_evaluator_integrated: true,
      retained_current_build_present_and_absent_capture_frontier_exhausted: true,
      retained_recovered_partial_prefix_frontier_exhausted: true,
      recovered_partial_prefix_source_capture_integrity_seal_authority: false,
      exact_build_hidden_controlled_capture_surface_identified: true,
      shipping_client_blocks_controlled_capture_submission: true,
      ordinary_production_account_controlled_capture_authorized: false,
      exact_build_training_scene_access_frontier_reviewed: true,
      ordinary_training_scene_entry_route_proven: false,
      training_scene_controlled_capture_currently_executable: false,
      current_controlled_pairs_available: false,
      recursive_sealed_rlog_candidate_discovery_bounded: true,
      exact_build_and_canonical_seal_candidate_gate_complete: true,
      known_candidate_seal_deduplication_complete: true,
      unseen_seal_positive_control_triggers_refresh: true,
      source_transition_candidate_search_complete: true,
      configured_endpoint_attribute_family_diagnostic_complete: true,
      configured_endpoint_transition_residual_ranking_complete: true,
      exact_build_spatial_attribute_identity_proof_complete: true,
      retained_spatial_raw_value_replay_complete: true,
      exact_build_action_selector_roster_complete: true,
      packet_source_compatible_static_formula_candidates: 6,
      packet_source_mismatch_preserved_unresolved: true,
      relative_spatial_relation_audit_complete: true,
      direct_source_to_target_geometry_equal_for_all_complete_pairs: false,
      spatial_attributes_safe_to_exclude_from_counterfactual_matching: false,
      current_new_sealed_candidate_rlogs: 0,
      current_source_transition_same_context_pairs: 229,
      current_configured_endpoint_transition_pairs: 69,
      current_configured_endpoint_transition_minimum_residual_dimensions: 13,
      current_source_transition_minimum_residual_observed_state_dimensions_before_configured_endpoint_diagnostic: 14,
      current_source_transition_minimum_residual_observed_state_dimensions: 13,
      current_source_transition_pairs_explained_only_by_configured_endpoint_and_diagnostic_exclusions: 0,
      current_source_transition_strict_controlled_pairs: 0,
      current_exact_action_selectors: 7,
      current_packet_source_route_matched_transition_pairs: 68,
      current_packet_source_route_rejected_transition_pairs: 1,
      current_direct_spatial_relation_complete_transition_pairs: 66,
      current_direct_spatial_relation_exact_transition_pairs: 2,
      current_direct_spatial_relation_nonexact_transition_pairs: 64,
      current_direct_spatial_relation_outside_one_raw_coordinate_unit_transition_pairs: 12,
      fake_bullet_exact_wire_contract_complete: true,
      fake_bullet_future_capture_timeline_preservation_complete: true,
      fake_bullet_current_build_observed_lifecycle_records: 0,
      fake_bullet_historical_canonical_logs_backfilled: false,
      fake_bullet_source4_damage_route_resolved: false,
      fake_bullet_provider_ownership_proven: false,
      combat_damage_stage_consumer_proven: false,
      exact_operation_order_proven: false,
      exact_integer_rounding_proven: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
      observed_damage_reassigned_to_provider: 0,
    },
    content_sha256: "",
  };
  sample.content_sha256 = digest(sample);
  verify(sample);
  sample.proof_closure.provider_rdps_credit_allowed = true;
  try {
    verify(sample);
    fail("self-test accepted provider credit");
  } catch (error) {
    if (error.message === "self-test accepted provider credit") throw error;
  }
  console.log("bpsr-fatal-spiral-damage-stage-frontier self-test passed");
}

const [command = "help", ...argv] = process.argv.slice(2);
try {
  if (command === "self-test") selfTest();
  else if (command === "verify") {
    const args = parse(argv);
    verify(readJson(path.resolve(required(args, "input")), "frontier"));
    console.log("Fatal Spiral damage-stage frontier verified");
  } else if (command === "build") {
    const args = parse(argv);
    const output = path.resolve(required(args, "output"));
    if (fs.existsSync(output)) fail(`Refusing to overwrite ${output}`);
    const report = build({
      tierWindowProof: required(args, "tier-window-proof"),
      gapWindowAudit: required(args, "gap-window-audit"),
      gapSafeLifecycleActionSummary:
        required(args, "gap-safe-lifecycle-action-summary"),
      stateScalingProof: required(args, "state-scaling-proof"),
      formulaCohort: required(args, "formula-cohort"),
      counterfactualFrontier: required(args, "counterfactual-frontier"),
      damageConsumerFrontier: required(args, "damage-consumer-frontier"),
      controlledPairWorklist: required(args, "controlled-pair-worklist"),
      comparisonExhaustion: required(args, "comparison-exhaustion"),
      partialPrefixFrontier: required(args, "partial-prefix-frontier"),
      controlledCaptureClientFrontier:
        required(args, "controlled-capture-client-frontier"),
      trainingSceneAccessFrontier:
        required(args, "training-scene-access-frontier"),
      candidateReadinessFrontier:
        required(args, "candidate-readiness-frontier"),
    });
    fs.writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
    console.log(JSON.stringify({ output, proof_closure: report.proof_closure }, null, 2));
  } else {
    console.log("Usage:\n  node tools/bpsr-fatal-spiral-damage-stage-frontier.mjs build --tier-window-proof <json> --gap-window-audit <json> --gap-safe-lifecycle-action-summary <json> --state-scaling-proof <json> --formula-cohort <json> --counterfactual-frontier <json> --damage-consumer-frontier <json> --controlled-pair-worklist <json> --comparison-exhaustion <json> --partial-prefix-frontier <json> --controlled-capture-client-frontier <json> --training-scene-access-frontier <json> --candidate-readiness-frontier <json> --output <json>\n  node tools/bpsr-fatal-spiral-damage-stage-frontier.mjs verify --input <json>\n  node tools/bpsr-fatal-spiral-damage-stage-frontier.mjs self-test");
    process.exitCode = command === "help" ? 0 : 1;
  }
} catch (error) {
  console.error(error.stack ?? String(error));
  process.exitCode = 1;
}
