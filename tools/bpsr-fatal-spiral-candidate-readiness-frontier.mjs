#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 6;
const GENERATED_BY = "tools/bpsr-fatal-spiral-candidate-readiness-frontier.mjs";
const GAME_BUILD = "24687926";
const EFFECT_ID = 2110125;
const DIAGNOSTIC_ENDPOINT_ATTRIBUTE_IDS = [13100, 13101, 13102, 13103, 13104, 13105];
const SPATIAL_ATTRIBUTE_IDS = [52, 53];

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

function validateManifest(report, positiveControl) {
  const summary = report.summary ?? {};
  const policy = report.policy ?? {};
  if (
    report.schema_version !== 1 ||
    report.generated_by !== "rlogs-bpsr-sealed-rlog-candidate-manifest" ||
    report.game_build !== GAME_BUILD ||
    Number(report.effect_id) !== EFFECT_ID ||
    report.damage_relationship !== "source" ||
    policy.recursive_directory_discovery_is_bounded !== true ||
    policy.partial_rlog_names_are_excluded !== true ||
    policy.exact_build_header_is_required !== true ||
    policy.canonical_seal_replay_is_required !== true ||
    policy.sealed_rlogs_are_streamed_one_event_at_a_time !== true ||
    policy.data_gaps_pauses_and_run_boundaries_cut_effect_windows !== true ||
    policy.known_candidates_are_deduplicated_by_sealed_content_sha256 !== true ||
    policy.remote_player_cast_packets_are_required !== false ||
    policy.packet_absence_is_zero !== false ||
    policy.current_snapshots_may_rewrite_historical_runs !== false ||
    policy.candidate_manifest_is_controlled_pair_proof !== false ||
    policy.formula_authority !== false ||
    policy.runtime_authority !== false ||
    policy.ui_display_authority !== false ||
    policy.provider_rdps_credit_allowed !== false
  ) fail("Sealed RLOG candidate manifest is unsafe or incompatible");
  if (positiveControl) {
    if (
      Number(summary.discovered_sealed_name_candidates) !== 1 ||
      Number(summary.exact_build_sealed_rlogs) !== 1 ||
      Number(summary.candidate_rlogs) !== 1 ||
      Number(summary.known_candidate_rlogs) !== 0 ||
      Number(summary.new_candidate_rlogs) !== 1 ||
      report.next_stage?.refresh_required !== true ||
      report.inputs?.rlogs?.length !== 1
    ) fail("Sealed RLOG candidate positive control did not trigger refresh");
    return;
  }
  const candidates = report.candidate_rlogs ?? [];
  if (
    Number(summary.discovered_sealed_name_candidates) !== 55 ||
    Number(summary.exact_build_sealed_rlogs) !== 26 ||
    Number(summary.wrong_build_rlogs) !== 29 ||
    Number(summary.unsealed_or_unreadable_rlogs) !== 0 ||
    Number(summary.exact_build_rlogs_without_selected_effect) !== 20 ||
    Number(summary.exact_build_effect_rlogs_without_complete_damage_window) !== 3 ||
    Number(summary.candidate_rlogs) !== 3 ||
    Number(summary.known_candidate_rlogs) !== 3 ||
    Number(summary.new_candidate_rlogs) !== 0 ||
    report.next_stage?.refresh_required !== false ||
    report.next_stage?.source_manifest_json_pointer !== "/inputs/rlogs" ||
    report.inputs?.rlogs?.length !== 0 ||
    candidates.length !== 3 ||
    candidates.some((entry) => entry.known_sealed_content !== true || entry.new_candidate !== false) ||
    candidates.reduce((sum, entry) => sum + Number(entry.complete_gap_bounded_lifecycles), 0) !== 29 ||
    candidates.reduce((sum, entry) => sum + Number(entry.complete_windows_with_damage), 0) !== 29 ||
    candidates.reduce((sum, entry) => sum + Number(entry.damage_events_while_active), 0) !== 27238
  ) fail("Current sealed RLOG candidate manifest does not match the reviewed frontier");
}

function validateTransitionAudit(report, manifest) {
  const summary = report.summary ?? {};
  if (
    report.schema_version !== 9 ||
    report.generated_by !== "rlogs-bpsr-rlog-transition-counterfactual-audit" ||
    report.game_build !== GAME_BUILD ||
    Number(report.effect_id) !== EFFECT_ID ||
    report.damage_relationship !== "source" ||
    report.policy?.damage_relationship_is_explicit !== true ||
    report.policy?.sealed_rlogs_are_streamed_one_event_at_a_time !== true ||
    report.policy?.every_data_gap_pause_and_run_boundary_resets_all_observed_state !== true ||
    report.policy?.configured_endpoint_attribute_exclusion_is_diagnostic_only !== true ||
    report.policy?.relative_spatial_relations_are_diagnostic_only !== true ||
    report.policy?.structurally_unobservable_remote_player_packets_are_not_acquisition_requirements !== true ||
    report.policy?.candidate_pairs_never_grant_formula_or_runtime_authority !== true ||
    report.policy?.formula_authority !== false ||
    report.policy?.runtime_authority !== false ||
    report.policy?.ui_display_authority !== false ||
    report.policy?.provider_rdps_credit_allowed !== false ||
    report.search_contract?.selected_effect_endpoint_role !== "damage_actor" ||
    JSON.stringify(report.search_contract?.diagnostic_endpoint_attribute_ids) !==
      JSON.stringify(DIAGNOSTIC_ENDPOINT_ATTRIBUTE_IDS) ||
    report.search_contract?.remote_player_packet_dependency !== false ||
    report.inputs?.gap_window_audit?.sha256 !== manifest.known_artifacts?.[0]?.sha256 ||
    Number(summary.source_rlog_count) !== 6 ||
    Number(summary.canonical_event_count) !== 3092247 ||
    Number(summary.damage_events) !== 356339 ||
    Number(summary.damage_events_with_selected_effect_active) !== 41372 ||
    Number(summary.damage_events_with_selected_effect_absent) !== 314967 ||
    Number(summary.opposite_state_recent_comparisons) !== 695592 ||
    Number(summary.same_normalized_damage_context_pairs) !== 229 ||
    Number(summary.minimum_residual_observed_state_dimensions_after_443_474_and_target_current_hp_exclusion) !== 14 ||
    Number(summary.minimum_residual_observed_state_dimensions_after_configured_endpoint_443_474_and_target_current_hp_exclusion) !== 13 ||
    Number(summary.configured_endpoint_transition_pairs) !== 69 ||
    Object.keys(summary.configured_endpoint_transition_action_identity_counts ?? {}).length !== 36 ||
    Object.keys(summary.configured_endpoint_transition_spatial_relations ?? {}).length !== 6 ||
    canonical(summary.configured_endpoint_attribute_transition_counts) !== canonical({ 13100: 69, 13101: 69, 13102: 69 }) ||
    Number(summary.configured_endpoint_transition_source_residual_attribute_difference_counts?.[52]) !== 67 ||
    Number(summary.configured_endpoint_transition_source_residual_attribute_difference_counts?.[53]) !== 67 ||
    Number(summary.configured_endpoint_transition_target_residual_attribute_difference_counts?.[52]) !== 10 ||
    Number(summary.configured_endpoint_transition_target_residual_attribute_difference_counts?.[53]) !== 12 ||
    Object.entries(summary.configured_endpoint_transition_residual_dimension_count_distribution ?? {})
      .reduce((sum, [dimensions, count]) => sum + (Number(dimensions) >= 0 ? Number(count) : 0), 0) !== 69 ||
    Number(summary.configured_endpoint_transition_pairs_with_attribute_snapshot_flag_difference) !== 0 ||
    Number(summary.configured_endpoint_transition_pairs_with_temporary_snapshot_flag_difference) !== 0 ||
    Number(summary.minimum_residual_observed_state_dimensions_among_configured_endpoint_transition_pairs) !== 13 ||
    Number(summary.same_context_pairs_after_configured_endpoint_transition_and_diagnostic_exclusions_with_equal_statuses) !== 0 ||
    Number(summary.exact_observed_input_candidate_pairs) !== 0 ||
    Number(summary.strict_controlled_counterfactual_pairs) !== 0 ||
    summary.exact_operation_order_proven !== false ||
    summary.exact_integer_rounding_proven !== false ||
    summary.packet_conservation_proven !== false ||
    summary.formula_authority !== false ||
    summary.provider_rdps_credit_allowed !== false
  ) fail("Source-side transition candidate audit is unsafe or incompatible");
}

function validateActionSpatialFrontier(report, transitionDescriptor) {
  const summary = report.summary ?? {};
  if (
    report.schema_version !== 3 ||
    report.generated_by !== "tools/bpsr-fatal-spiral-transition-action-spatial-frontier.mjs" ||
    report.game_build !== GAME_BUILD ||
    Number(report.effect_id) !== EFFECT_ID ||
    report.damage_relationship !== "source" ||
    report.inputs?.transition_audit?.sha256 !== transitionDescriptor.sha256 ||
    report.source_identity?.exact_build_table_manifest_bindings_complete !== true ||
    report.policy?.structurally_unobservable_remote_player_cast_packets_are_required !== false ||
    report.policy?.static_table_values_are_server_operator_proof !== false ||
    report.policy?.packet_source_mismatches_are_preserved_as_unresolved !== true ||
    report.policy?.future_capture_capability_is_historical_observation_proof !== false ||
    report.policy?.enclosing_aoi_entity_is_provider_without_separate_proof !== false ||
    report.policy?.unresolved_evidence_hidden !== false ||
    Number(summary.component_action_identities) !== 36 ||
    Number(summary.exact_action_selectors) !== 7 ||
    Number(summary.observed_transition_pairs) !== 69 ||
    Number(summary.packet_source_route_matched_selectors) !== 6 ||
    Number(summary.packet_source_route_matched_transition_pairs) !== 68 ||
    Number(summary.packet_source_route_rejected_selectors) !== 1 ||
    Number(summary.packet_source_route_rejected_transition_pairs) !== 1 ||
    Number(summary.exact_build_static_formula_candidates) !== 6 ||
    Number(summary.direct_spatial_relation_complete_transition_pairs) !== 66 ||
    Number(summary.direct_spatial_relation_exact_transition_pairs) !== 2 ||
    Number(summary.direct_spatial_relation_nonexact_transition_pairs) !== 64 ||
    Number(summary.direct_spatial_relation_outside_one_raw_coordinate_unit_transition_pairs) !== 12 ||
    Number(summary.fake_bullet_exact_wire_fields) !== 7 ||
    summary.fake_bullet_future_capture_join_keys_retained !== true ||
    Number(summary.fake_bullet_current_build_observed_lifecycle_records) !== 0 ||
    summary.fake_bullet_source4_damage_route_resolved !== false ||
    summary.fake_bullet_provider_ownership_proven !== false ||
    summary.spatial_state_safe_to_exclude_from_counterfactual_matching !== false ||
    Number(summary.strict_controlled_counterfactual_pairs) !== 0 ||
    summary.formula_authority !== false ||
    summary.ui_display_authority !== false ||
    summary.provider_rdps_credit_allowed !== false
  ) fail("Fatal Spiral action/spatial frontier is unsafe or incompatible");
}

function validateAttributeEnumProof(report) {
  const attrType = report.types?.find((entry) => entry.namespace === "Zproto" && entry.name === "EAttrType");
  const enumValues = Object.fromEntries((attrType?.enum_values ?? []).map((entry) => [entry.name, Number(entry.value)]));
  if (
    report.schema_version !== 2 ||
    report.generated_by !== "rlogs-bpsr-il2cpp-combat-surface" ||
    report.game !== "blue-protocol-star-resonance" ||
    report.deployment !== "global" ||
    report.channel !== "steam" ||
    report.build_id !== GAME_BUILD ||
    report.policy?.offline_research_only !== true ||
    report.policy?.runtime_formula_authority !== false ||
    report.policy?.unresolved_evidence_hidden !== false ||
    report.policy?.exact_build_packet_replay_required_for_promotion !== true ||
    attrType?.kind !== "enum" ||
    enumValues.AttrPos !== 52 ||
    enumValues.AttrTargetPos !== 53
  ) fail("Exact-build entity attribute enum proof is unsafe or incompatible");
}

function validateSpatialAudit(report, manifest) {
  const summary = report.summary ?? {};
  const attr52 = summary.attributes?.[52] ?? {};
  const attr53 = summary.attributes?.[53] ?? {};
  if (
    report.schema_version !== 4 ||
    report.generated_by !== "rlogs-bpsr-rlog-opaque-attribute-audit" ||
    report.game_build !== GAME_BUILD ||
    Number(report.gap_window_effect_id) !== EFFECT_ID ||
    report.gap_window_damage_relationship !== "source" ||
    canonical(report.attribute_ids) !== canonical(SPATIAL_ATTRIBUTE_IDS) ||
    report.policy?.sealed_rlogs_are_streamed_one_event_at_a_time !== true ||
    report.policy?.every_data_gap_pause_and_run_boundary_resets_prior_attribute_state !== true ||
    report.policy?.gap_window_damage_relationship_is_explicit_and_scope_only !== true ||
    report.policy?.retained_raw_bytes_are_redecoded_with_the_current_exact_id_allowlist !== true ||
    report.policy?.opaque_attributes_are_not_excluded_without_semantic_proof !== true ||
    report.policy?.structurally_unobservable_remote_player_packets_are_not_acquisition_requirements !== true ||
    report.policy?.remote_player_packet_dependency !== false ||
    report.policy?.safe_to_exclude_from_counterfactual_matching !== false ||
    report.policy?.formula_authority !== false ||
    report.policy?.runtime_authority !== false ||
    report.policy?.ui_display_authority !== false ||
    report.policy?.provider_rdps_credit_allowed !== false ||
    report.inputs?.gap_window_audit?.sha256 !== manifest.known_artifacts?.[0]?.sha256 ||
    Number(summary.source_rlog_count) !== 6 ||
    Number(summary.canonical_event_count) !== 3092247 ||
    Number(attr52.observation_count) !== 132297 ||
    Number(attr52.canonical_decoder_position_count) !== 131558 ||
    Number(attr52.canonical_decoder_unresolved_count) !== 739 ||
    attr52.safe_to_exclude_from_counterfactual_matching !== false ||
    Number(attr53.observation_count) !== 142877 ||
    Number(attr53.canonical_decoder_position_count) !== 142130 ||
    Number(attr53.canonical_decoder_unresolved_count) !== 747 ||
    attr53.safe_to_exclude_from_counterfactual_matching !== false ||
    summary.safe_to_exclude_from_counterfactual_matching !== false ||
    summary.formula_authority !== false
  ) fail("Spatial residual attribute audit is unsafe or incompatible");
}

function build(options) {
  const files = Object.fromEntries(Object.entries(options).map(([key, value]) => [key, path.resolve(value)]));
  const manifest = readJson(files.candidateManifest, "candidate manifest");
  const positive = readJson(files.positiveControl, "candidate manifest positive control");
  const transition = readJson(files.transitionAudit, "source transition candidate audit");
  const attributeEnum = readJson(files.attributeEnumProof, "exact-build entity attribute enum proof");
  const spatial = readJson(files.spatialAudit, "spatial residual attribute audit");
  const actionSpatial = readJson(files.actionSpatialFrontier, "action/spatial frontier");
  validateManifest(manifest, false);
  validateManifest(positive, true);
  validateTransitionAudit(transition, manifest);
  validateAttributeEnumProof(attributeEnum);
  validateSpatialAudit(spatial, manifest);
  validateActionSpatialFrontier(actionSpatial, descriptor(files.transitionAudit));
  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game: "blue-protocol-star-resonance",
    game_build: GAME_BUILD,
    identity: {
      effect_id: EFFECT_ID,
      damage_relationship: "source",
      effect_endpoint_role: "damage_actor",
    },
    policy: {
      exact_numeric_ids_and_build_are_authoritative: true,
      localized_names_are_evidence_only: true,
      remote_player_cast_packets_are_required: false,
      packet_absence_is_zero: false,
      new_sealed_candidate_is_controlled_pair_proof: false,
      source_transition_near_pair_is_formula_proof: false,
      configured_endpoint_attribute_family_exclusion_is_diagnostic_only: true,
      exact_build_spatial_attribute_names_are_evidence_only: true,
      spatial_attributes_are_not_excluded_from_counterfactual_matching: true,
      relative_spatial_relation_tolerances_are_diagnostic_only: true,
      relative_spatial_relation_equality_is_not_full_spatial_equivalence_proof: true,
      component_index_and_count_are_formula_identity: false,
      packet_source_mismatches_are_preserved_as_unresolved: true,
      future_capture_capability_is_historical_observation_proof: false,
      enclosing_aoi_entity_is_provider_without_separate_proof: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    inputs: Object.fromEntries(Object.entries(files).map(([key, file]) => [key, descriptor(file)])),
    reviewed_evidence: {
      discovered_sealed_name_candidates: manifest.summary.discovered_sealed_name_candidates,
      exact_build_sealed_rlogs: manifest.summary.exact_build_sealed_rlogs,
      exact_build_effect_rlogs: 6,
      complete_damage_window_candidate_rlogs: manifest.summary.candidate_rlogs,
      complete_gap_bounded_lifecycles: manifest.candidate_rlogs.reduce(
        (sum, entry) => sum + Number(entry.complete_gap_bounded_lifecycles), 0,
      ),
      damage_events_while_active: manifest.candidate_rlogs.reduce(
        (sum, entry) => sum + Number(entry.damage_events_while_active), 0,
      ),
      current_new_candidate_rlogs: manifest.summary.new_candidate_rlogs,
      positive_control_new_candidate_rlogs: positive.summary.new_candidate_rlogs,
      positive_control_refresh_required: positive.next_stage.refresh_required,
      source_transition_opposite_state_comparisons: transition.summary.opposite_state_recent_comparisons,
      source_transition_same_context_pairs: transition.summary.same_normalized_damage_context_pairs,
      configured_endpoint_transition_pairs: transition.summary.configured_endpoint_transition_pairs,
      configured_endpoint_attribute_transition_counts:
        transition.summary.configured_endpoint_attribute_transition_counts,
      configured_endpoint_transition_minimum_residual_dimensions:
        transition.summary.minimum_residual_observed_state_dimensions_among_configured_endpoint_transition_pairs,
      configured_endpoint_transition_source_spatial_difference_counts: {
        52: transition.summary.configured_endpoint_transition_source_residual_attribute_difference_counts[52],
        53: transition.summary.configured_endpoint_transition_source_residual_attribute_difference_counts[53],
      },
      configured_endpoint_transition_target_spatial_difference_counts: {
        52: transition.summary.configured_endpoint_transition_target_residual_attribute_difference_counts[52],
        53: transition.summary.configured_endpoint_transition_target_residual_attribute_difference_counts[53],
      },
      exact_build_spatial_attribute_identities: {
        52: "AttrPos",
        53: "AttrTargetPos",
      },
      spatial_attribute_observations: {
        52: spatial.summary.attributes[52].observation_count,
        53: spatial.summary.attributes[53].observation_count,
      },
      spatial_attribute_position_decodes: {
        52: spatial.summary.attributes[52].canonical_decoder_position_count,
        53: spatial.summary.attributes[53].canonical_decoder_position_count,
      },
      spatial_attributes_safe_to_exclude_from_counterfactual_matching:
        spatial.summary.safe_to_exclude_from_counterfactual_matching,
      source_transition_minimum_residual_observed_state_dimensions_before_configured_endpoint_diagnostic:
        transition.summary.minimum_residual_observed_state_dimensions_after_443_474_and_target_current_hp_exclusion,
      source_transition_minimum_residual_observed_state_dimensions:
        transition.summary.minimum_residual_observed_state_dimensions_after_configured_endpoint_443_474_and_target_current_hp_exclusion,
      source_transition_pairs_explained_only_by_configured_endpoint_and_diagnostic_exclusions:
        transition.summary.same_context_pairs_after_configured_endpoint_transition_and_diagnostic_exclusions_with_equal_statuses,
      exact_observed_input_candidate_pairs: transition.summary.exact_observed_input_candidate_pairs,
      strict_controlled_counterfactual_pairs: transition.summary.strict_controlled_counterfactual_pairs,
      exact_action_selectors: actionSpatial.summary.exact_action_selectors,
      packet_source_route_matched_transition_pairs:
        actionSpatial.summary.packet_source_route_matched_transition_pairs,
      packet_source_route_rejected_transition_pairs:
        actionSpatial.summary.packet_source_route_rejected_transition_pairs,
      exact_build_static_formula_candidates:
        actionSpatial.summary.exact_build_static_formula_candidates,
      direct_spatial_relation_complete_transition_pairs:
        actionSpatial.summary.direct_spatial_relation_complete_transition_pairs,
      direct_spatial_relation_exact_transition_pairs:
        actionSpatial.summary.direct_spatial_relation_exact_transition_pairs,
      direct_spatial_relation_nonexact_transition_pairs:
        actionSpatial.summary.direct_spatial_relation_nonexact_transition_pairs,
      direct_spatial_relation_outside_one_raw_coordinate_unit_transition_pairs:
        actionSpatial.summary.direct_spatial_relation_outside_one_raw_coordinate_unit_transition_pairs,
      fake_bullet_exact_wire_fields: actionSpatial.summary.fake_bullet_exact_wire_fields,
      fake_bullet_future_capture_join_keys_retained:
        actionSpatial.summary.fake_bullet_future_capture_join_keys_retained,
      fake_bullet_current_build_observed_lifecycle_records:
        actionSpatial.summary.fake_bullet_current_build_observed_lifecycle_records,
      fake_bullet_source4_damage_route_resolved:
        actionSpatial.summary.fake_bullet_source4_damage_route_resolved,
      fake_bullet_provider_ownership_proven:
        actionSpatial.summary.fake_bullet_provider_ownership_proven,
    },
    proof_closure: {
      recursive_sealed_rlog_discovery_bounded: true,
      exact_build_and_canonical_seal_required: true,
      known_seal_deduplication_complete: true,
      unseen_seal_positive_control_triggers_refresh: true,
      source_side_effect_endpoint_join_complete: true,
      current_new_candidate_rlogs: 0,
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
      current_strict_controlled_pairs: 0,
      exact_operation_order_proven: false,
      exact_integer_rounding_proven: false,
      packet_conservation_proven: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    next_action:
      "capture exact-build FakeBullets beside source-4 action 220101 damage to prove the container/provider join; retain source and target positions and seek a same-build 13100-family pair with equal remaining state before exact-integer evaluation",
    content_sha256: "",
  };
  report.content_sha256 = digest(report);
  verify(report);
  return report;
}

function verify(report) {
  const closure = report.proof_closure ?? {};
  if (
    report.schema_version !== SCHEMA_VERSION ||
    report.generated_by !== GENERATED_BY ||
    report.game_build !== GAME_BUILD ||
    Number(report.identity?.effect_id) !== EFFECT_ID ||
    report.identity?.damage_relationship !== "source" ||
    report.identity?.effect_endpoint_role !== "damage_actor" ||
    report.policy?.remote_player_cast_packets_are_required !== false ||
    report.policy?.packet_absence_is_zero !== false ||
    report.policy?.new_sealed_candidate_is_controlled_pair_proof !== false ||
    report.policy?.source_transition_near_pair_is_formula_proof !== false ||
    report.policy?.configured_endpoint_attribute_family_exclusion_is_diagnostic_only !== true ||
    report.policy?.exact_build_spatial_attribute_names_are_evidence_only !== true ||
    report.policy?.spatial_attributes_are_not_excluded_from_counterfactual_matching !== true ||
    report.policy?.relative_spatial_relation_tolerances_are_diagnostic_only !== true ||
    report.policy?.relative_spatial_relation_equality_is_not_full_spatial_equivalence_proof !== true ||
    report.policy?.component_index_and_count_are_formula_identity !== false ||
    report.policy?.packet_source_mismatches_are_preserved_as_unresolved !== true ||
    report.policy?.future_capture_capability_is_historical_observation_proof !== false ||
    report.policy?.enclosing_aoi_entity_is_provider_without_separate_proof !== false ||
    report.policy?.provider_rdps_credit_allowed !== false ||
    Number(report.reviewed_evidence?.complete_gap_bounded_lifecycles) !== 29 ||
    Number(report.reviewed_evidence?.damage_events_while_active) !== 27238 ||
    Number(report.reviewed_evidence?.current_new_candidate_rlogs) !== 0 ||
    Number(report.reviewed_evidence?.positive_control_new_candidate_rlogs) !== 1 ||
    report.reviewed_evidence?.positive_control_refresh_required !== true ||
    Number(report.reviewed_evidence?.source_transition_same_context_pairs) !== 229 ||
    Number(report.reviewed_evidence?.configured_endpoint_transition_pairs) !== 69 ||
    canonical(report.reviewed_evidence?.configured_endpoint_attribute_transition_counts) !==
      canonical({ 13100: 69, 13101: 69, 13102: 69 }) ||
    Number(report.reviewed_evidence?.configured_endpoint_transition_minimum_residual_dimensions) !== 13 ||
    canonical(report.reviewed_evidence?.exact_build_spatial_attribute_identities) !==
      canonical({ 52: "AttrPos", 53: "AttrTargetPos" }) ||
    canonical(report.reviewed_evidence?.spatial_attribute_observations) !==
      canonical({ 52: 132297, 53: 142877 }) ||
    canonical(report.reviewed_evidence?.spatial_attribute_position_decodes) !==
      canonical({ 52: 131558, 53: 142130 }) ||
    report.reviewed_evidence?.spatial_attributes_safe_to_exclude_from_counterfactual_matching !== false ||
    Number(report.reviewed_evidence?.source_transition_minimum_residual_observed_state_dimensions_before_configured_endpoint_diagnostic) !== 14 ||
    Number(report.reviewed_evidence?.source_transition_minimum_residual_observed_state_dimensions) !== 13 ||
    Number(report.reviewed_evidence?.source_transition_pairs_explained_only_by_configured_endpoint_and_diagnostic_exclusions) !== 0 ||
    Number(report.reviewed_evidence?.strict_controlled_counterfactual_pairs) !== 0 ||
    Number(report.reviewed_evidence?.exact_action_selectors) !== 7 ||
    Number(report.reviewed_evidence?.packet_source_route_matched_transition_pairs) !== 68 ||
    Number(report.reviewed_evidence?.packet_source_route_rejected_transition_pairs) !== 1 ||
    Number(report.reviewed_evidence?.exact_build_static_formula_candidates) !== 6 ||
    Number(report.reviewed_evidence?.direct_spatial_relation_complete_transition_pairs) !== 66 ||
    Number(report.reviewed_evidence?.direct_spatial_relation_exact_transition_pairs) !== 2 ||
    Number(report.reviewed_evidence?.direct_spatial_relation_nonexact_transition_pairs) !== 64 ||
    Number(report.reviewed_evidence?.direct_spatial_relation_outside_one_raw_coordinate_unit_transition_pairs) !== 12 ||
    Number(report.reviewed_evidence?.fake_bullet_exact_wire_fields) !== 7 ||
    report.reviewed_evidence?.fake_bullet_future_capture_join_keys_retained !== true ||
    Number(report.reviewed_evidence?.fake_bullet_current_build_observed_lifecycle_records) !== 0 ||
    report.reviewed_evidence?.fake_bullet_source4_damage_route_resolved !== false ||
    report.reviewed_evidence?.fake_bullet_provider_ownership_proven !== false ||
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
    closure.formula_authority !== false ||
    closure.runtime_authority !== false ||
    closure.ui_display_authority !== false ||
    closure.provider_rdps_credit_allowed !== false ||
    report.content_sha256 !== digest(report)
  ) fail("Fatal Spiral candidate-readiness frontier is unsafe or has an invalid digest");
}

function parse(argv) {
  const args = {};
  for (let index = 0; index < argv.length; index += 2) {
    const flag = argv[index];
    const value = argv[index + 1];
    if (!flag?.startsWith("--") || value == null) fail(`Invalid argument ${flag ?? "<missing>"}`);
    args[flag.slice(2)] = value;
  }
  return args;
}

function required(args, name) {
  if (!args[name]) fail(`Missing --${name}`);
  return args[name];
}

function selfTest() {
  const sample = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game_build: GAME_BUILD,
    identity: { effect_id: EFFECT_ID, damage_relationship: "source", effect_endpoint_role: "damage_actor" },
    policy: {
      remote_player_cast_packets_are_required: false,
      packet_absence_is_zero: false,
      new_sealed_candidate_is_controlled_pair_proof: false,
      source_transition_near_pair_is_formula_proof: false,
      configured_endpoint_attribute_family_exclusion_is_diagnostic_only: true,
      exact_build_spatial_attribute_names_are_evidence_only: true,
      spatial_attributes_are_not_excluded_from_counterfactual_matching: true,
      relative_spatial_relation_tolerances_are_diagnostic_only: true,
      relative_spatial_relation_equality_is_not_full_spatial_equivalence_proof: true,
      component_index_and_count_are_formula_identity: false,
      packet_source_mismatches_are_preserved_as_unresolved: true,
      future_capture_capability_is_historical_observation_proof: false,
      enclosing_aoi_entity_is_provider_without_separate_proof: false,
      provider_rdps_credit_allowed: false,
    },
    reviewed_evidence: {
      complete_gap_bounded_lifecycles: 29,
      damage_events_while_active: 27238,
      current_new_candidate_rlogs: 0,
      positive_control_new_candidate_rlogs: 1,
      positive_control_refresh_required: true,
      source_transition_same_context_pairs: 229,
      configured_endpoint_transition_pairs: 69,
      configured_endpoint_attribute_transition_counts: { 13100: 69, 13101: 69, 13102: 69 },
      configured_endpoint_transition_minimum_residual_dimensions: 13,
      exact_build_spatial_attribute_identities: { 52: "AttrPos", 53: "AttrTargetPos" },
      spatial_attribute_observations: { 52: 132297, 53: 142877 },
      spatial_attribute_position_decodes: { 52: 131558, 53: 142130 },
      spatial_attributes_safe_to_exclude_from_counterfactual_matching: false,
      source_transition_minimum_residual_observed_state_dimensions_before_configured_endpoint_diagnostic: 14,
      source_transition_minimum_residual_observed_state_dimensions: 13,
      source_transition_pairs_explained_only_by_configured_endpoint_and_diagnostic_exclusions: 0,
      strict_controlled_counterfactual_pairs: 0,
      exact_action_selectors: 7,
      packet_source_route_matched_transition_pairs: 68,
      packet_source_route_rejected_transition_pairs: 1,
      exact_build_static_formula_candidates: 6,
      direct_spatial_relation_complete_transition_pairs: 66,
      direct_spatial_relation_exact_transition_pairs: 2,
      direct_spatial_relation_nonexact_transition_pairs: 64,
      direct_spatial_relation_outside_one_raw_coordinate_unit_transition_pairs: 12,
      fake_bullet_exact_wire_fields: 7,
      fake_bullet_future_capture_join_keys_retained: true,
      fake_bullet_current_build_observed_lifecycle_records: 0,
      fake_bullet_source4_damage_route_resolved: false,
      fake_bullet_provider_ownership_proven: false,
    },
    proof_closure: {
      recursive_sealed_rlog_discovery_bounded: true,
      exact_build_and_canonical_seal_required: true,
      known_seal_deduplication_complete: true,
      unseen_seal_positive_control_triggers_refresh: true,
      source_side_effect_endpoint_join_complete: true,
      current_new_candidate_rlogs: 0,
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
      current_strict_controlled_pairs: 0,
      exact_operation_order_proven: false,
      exact_integer_rounding_proven: false,
      packet_conservation_proven: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
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
  console.log("bpsr-fatal-spiral-candidate-readiness-frontier self-test passed");
}

const [command = "help", ...argv] = process.argv.slice(2);
try {
  if (command === "self-test") selfTest();
  else if (command === "verify") {
    const args = parse(argv);
    verify(readJson(path.resolve(required(args, "input")), "candidate-readiness frontier"));
    console.log("Fatal Spiral candidate-readiness frontier verified");
  } else if (command === "build") {
    const args = parse(argv);
    const output = path.resolve(required(args, "output"));
    if (fs.existsSync(output)) fail(`Refusing to overwrite ${output}`);
    const report = build({
      candidateManifest: required(args, "candidate-manifest"),
      positiveControl: required(args, "positive-control"),
      transitionAudit: required(args, "transition-audit"),
      attributeEnumProof: required(args, "attribute-enum-proof"),
      spatialAudit: required(args, "spatial-audit"),
      actionSpatialFrontier: required(args, "action-spatial-frontier"),
    });
    fs.writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
    console.log(JSON.stringify({ output, proof_closure: report.proof_closure }, null, 2));
  } else {
    console.log("Usage:\n  node tools/bpsr-fatal-spiral-candidate-readiness-frontier.mjs build --candidate-manifest <json> --positive-control <json> --transition-audit <json> --attribute-enum-proof <json> --spatial-audit <json> --action-spatial-frontier <json> --output <json>\n  node tools/bpsr-fatal-spiral-candidate-readiness-frontier.mjs verify --input <json>\n  node tools/bpsr-fatal-spiral-candidate-readiness-frontier.mjs self-test");
    process.exitCode = command === "help" ? 0 : 1;
  }
} catch (error) {
  console.error(error.stack ?? String(error));
  process.exitCode = 1;
}
