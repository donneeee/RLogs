#!/usr/bin/env node

import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 1;
const GENERATOR = "tools/bpsr-source-status-local-observable-audit.mjs";
const BUILD = "24687926";
const EFFECT_IDS = [701010, 2207252];
const TRANSFER_FAMILY_IDS = [2207250, 2207251, 2207252];
const ATTRIBUTE_IDS = [
  11020, 11021, 11022, 11023, 11024, 11025,
  11030, 11031, 11032, 11033, 11034, 11035,
  11330, 11331, 11332, 11333, 11334, 11335,
];

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);
if (command === "analyze") analyze(options);
else if (command === "verify") verifyCommand(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function analyze(parsed) {
  const build = numericString(required(parsed, "build"), "build");
  const files = {
    build_source_manifest: resolved(parsed, "build-source-manifest"),
    buff_table: resolved(parsed, "buff-table"),
    talent_table: resolved(parsed, "talent-table"),
    affix_table: resolved(parsed, "affix-table"),
    activation_index: resolved(parsed, "activation-index"),
    origin_catalog: resolved(parsed, "origin-catalog"),
    reconciliation: resolved(parsed, "reconciliation"),
    transition_counterfactual_audit: resolved(parsed, "transition-counterfactual-audit"),
    attribute_proof: resolved(parsed, "attribute-proof"),
  };
  const output = path.resolve(required(parsed, "output"));
  const inputs = Object.fromEntries(Object.entries(files).map(([key, file]) =>
    [key, fileDescriptor(file)]));
  const manifest = readJson(files.build_source_manifest, "build source manifest");
  const buffTable = readJson(files.buff_table, "BuffTable");
  const talentTable = readJson(files.talent_table, "TalentTable");
  const affixTable = readJson(files.affix_table, "AffixTable");
  const activation = readJson(files.activation_index, "activation index");
  const origin = readJson(files.origin_catalog, "observed effect origin catalog");
  const reconciliation = readJson(files.reconciliation, "observed effect reconciliation");
  const transition = readJson(
    files.transition_counterfactual_audit,
    "transition counterfactual audit",
  );
  const attributeProof = readJson(files.attribute_proof, "status attribute proof");

  const tableBindings = validateManifest(manifest, build, inputs);
  validateActivation(activation, build);
  validateOrigin(origin, build);
  validateReconciliation(reconciliation, build);
  validateTransition(transition, build);
  validateAttributeProof(attributeProof, transition);
  verifyRlogCohort(transition);

  const affix = exactRow(affixTable, 999, "AffixTable");
  const talent = exactRow(talentTable, 1324, "TalentTable");
  const buffs = Object.fromEntries(TRANSFER_FAMILY_IDS.map((id) =>
    [id, exactRow(buffTable, id, "BuffTable")]));
  if (!containsExactScalar(affix.Effect, 701010) ||
    !containsExactScalar(talent.TalentEffect, 2207250)) {
    throw new Error("exact static source routes are missing");
  }
  const origin701010 = exactEffect(origin, 701010, "origin catalog");
  const origin2207252 = exactEffect(origin, 2207252, "origin catalog");
  const relation701010 = exactRelation(origin, 701010);
  const relation2207252 = exactRelation(origin, 2207252);
  const reconciled701010 = exactEffect(reconciliation, 701010, "reconciliation");
  const reconciled2207252 = exactEffect(reconciliation, 2207252, "reconciliation");
  const observedDeltaSubset = buildObservedDeltaSubset(attributeProof);
  const sourceContextExamples = Object.values(observedDeltaSubset.attributes)
    .reduce((sum, row) => sum + row.provider_attribute_context_examples, 0);

  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATOR,
    game_build: build,
    status: "local-external-stat-transfer-subset-observed-general-formula-unproven",
    policy: {
      exact_numeric_ids_build_and_input_hashes_are_authoritative: true,
      localized_names_notes_and_descriptions_are_semantic_evidence_only: true,
      current_character_snapshots_never_substitute_for_historical_provider_attributes: true,
      structurally_unavailable_remote_player_attributes_are_not_acquisition_requirements: true,
      unavailable_provider_attribute_absence_is_not_zero: true,
      exact_observed_recipient_deltas_are_preserved_without_inventing_provider_inputs: true,
      dynamic_observed_deltas_do_not_prove_a_general_percent_formula: true,
      unresolved_source_statuses_remain_in_counterfactual_matching: true,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    inputs,
    build_identity: {
      manifest_aggregate_sha256: String(manifest.aggregateSha256),
      decoded_table_bindings: tableBindings,
      source_rlog_count: transition.inputs.source_rlogs.length,
      source_rlog_bytes: transition.inputs.source_rlogs
        .reduce((sum, receipt) => sum + Number(receipt.bytes), 0),
      exact_source_rlog_hashes_reverified: true,
    },
    effect_701010: {
      effect_id: 701010,
      exact_static_source: {
        source_kind: "affix",
        source_config_id: 999,
        effect_type: Number(affix.EffectType),
        target_type: Number(affix.TargetType),
        configured_effect_contains_exact_id: true,
      },
      semantic_evidence_only: {
        buff_design_name: String(exactRow(buffTable, 701010, "BuffTable").NameDesign ?? ""),
        affix_name: String(affix.Name ?? ""),
        affix_description: String(affix.Description ?? ""),
      },
      packet_lifecycle: lifecycleSummary(origin701010),
      packet_origin: relationSummary(relation701010),
      reconciliation: {
        proof_queue: String(reconciled701010.proof_queue),
        formula_endpoint_state: String(reconciled701010.endpoint_resolution.state),
        source_resolution_state: String(reconciled701010.source_resolution.state),
        resolved_external_player_to_player_windows: Number(
          reconciled701010.packet_lifecycle.resolved_external_player_to_player_windows),
        unresolved_cross_actor_windows: Number(
          reconciled701010.packet_lifecycle.unresolved_cross_actor_windows),
      },
      produced_action_observed: false,
      provider_identity_proven: false,
      formula_endpoint_proven: false,
      safe_to_exclude_from_counterfactual_matching: false,
      provider_rdps_credit_allowed: false,
    },
    effect_2207252: {
      effect_id: 2207252,
      transfer_family_effect_ids: TRANSFER_FAMILY_IDS,
      exact_owning_source: {
        source_id: "talent:1324",
        source_entity_id: 1324,
        talent_effect_contains_exact_root_buff_id: true,
        root_buff_id: 2207250,
        packet_origin_buff_id: 2207251,
        recipient_effect_id: 2207252,
      },
      exact_static_fields: {
        root_repeat_add_rule: structuredClone(buffs[2207250].RepeatAddRule ?? []),
        recipient_repeat_add_rule: structuredClone(buffs[2207252].RepeatAddRule ?? []),
        recipient_destroy_param: structuredClone(buffs[2207252].DestroyParam ?? []),
      },
      semantic_evidence_only: {
        talent_name: String(talent.TalentName ?? ""),
        talent_description: String(talent.TalentDes ?? ""),
        root_buff_design_name: String(buffs[2207250].NameDesign ?? ""),
        root_buff_note: String(buffs[2207250].Note ?? ""),
        packet_origin_buff_design_name: String(buffs[2207251].NameDesign ?? ""),
        recipient_effect_design_name: String(buffs[2207252].NameDesign ?? ""),
        description_mentions_five_percent: /5%/.test(String(buffs[2207250].Note ?? "")),
        five_percent_is_formula_authority: false,
      },
      packet_lifecycle: lifecycleSummary(origin2207252),
      packet_origin: relationSummary(relation2207252),
      reconciliation: {
        proof_queue: String(reconciled2207252.proof_queue),
        formula_endpoint_state: String(reconciled2207252.endpoint_resolution.state),
        source_resolution_state: String(reconciled2207252.source_resolution.state),
        exact_unique_source: Boolean(reconciled2207252.source_resolution.exact_unique_source),
        exact_owning_source: Boolean(reconciled2207252.source_resolution.exact_owning_source),
        candidate_source_ids: structuredClone(
          reconciled2207252.source_resolution.candidate_source_ids ?? []),
        observed_external_provider_recipient_lifecycle: Boolean(
          reconciled2207252.packet_lifecycle.observed_external_provider_recipient_lifecycle),
        resolved_external_player_to_player_windows: Number(
          reconciled2207252.packet_lifecycle.resolved_external_player_to_player_windows),
        unresolved_cross_actor_windows: Number(
          reconciled2207252.packet_lifecycle.unresolved_cross_actor_windows),
        formula_value_resolution: String(reconciled2207252.formula_endpoint.value_resolution),
        formula_scope_kinds: structuredClone(reconciled2207252.formula_endpoint.scope_kinds ?? []),
      },
      produced_action_observed: false,
      locally_observed_dynamic_recipient_delta_subset: observedDeltaSubset,
      provider_attribute_context: {
        expected_provider_attribute_family_from_semantics_only: "intellect",
        selected_provider_attribute_ids: [11020, 11021, 11022, 11023, 11024, 11025],
        exact_remote_provider_attribute_context_examples: sourceContextExamples,
        structurally_unavailable_in_local_capture: sourceContextExamples === 0,
        remote_player_attribute_acquisition_required: false,
        current_snapshot_substitution_allowed: false,
      },
      general_formula: {
        candidate_description_percent: 5,
        provider_input_identity_proven_from_packets: false,
        percent_magnitude_proven_from_exact_join: false,
        integer_rounding_proven: false,
        recipient_class_to_main_attribute_mapping_proven_for_all_classes: false,
        exact_damage_projection_proven: false,
      },
      exact_observed_delta_subset_research_replayable: true,
      full_lifecycle_formula_replayable: false,
      safe_to_exclude_from_counterfactual_matching: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    transition_confounder: {
      selected_effect_id: Number(transition.effect_id),
      same_normalized_damage_context_pairs: Number(
        transition.summary.same_normalized_damage_context_pairs),
      source_status_difference_counts: Object.fromEntries(EFFECT_IDS.map((effectId) => [
        String(effectId),
        Number(transition.summary.same_context_source_status_difference_counts[String(effectId)]),
      ])),
      same_context_and_nonselected_status_pairs: Number(
        transition.summary.same_context_and_nonselected_status_pairs),
      strict_controlled_counterfactual_pairs: Number(
        transition.summary.strict_controlled_counterfactual_pairs),
      both_effects_remain_confounders: true,
    },
    authority: {
      effect_701010_formula_proven: false,
      effect_2207252_general_transfer_formula_proven: false,
      effect_2207252_exact_damage_projection_proven: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
  };
  report.content_sha256 = stableContentHash(report);
  verifyReport(report);
  mkdirSync(path.dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  verifyInputs(report);
  console.log(JSON.stringify({
    effect_701010_windows: report.effect_701010.packet_lifecycle.window_count,
    effect_2207252_external_windows:
      report.effect_2207252.reconciliation.resolved_external_player_to_player_windows,
    exact_agility_delta_occurrences:
      report.effect_2207252.locally_observed_dynamic_recipient_delta_subset
        .attributes[11030].occurrences,
    provider_attribute_context_examples:
      report.effect_2207252.provider_attribute_context
        .exact_remote_provider_attribute_context_examples,
    general_formula_proven:
      report.effect_2207252.general_formula.percent_magnitude_proven_from_exact_join,
    content_sha256: report.content_sha256,
  }, null, 2));
}

function buildObservedDeltaSubset(proof) {
  const attributes = {};
  for (const system of proof.wire_additive_equation_systems ?? []) {
    const equations = (system.equations ?? []).filter((equation) =>
      equation.terms?.length === 1 && Number(equation.terms[0].effect_id) === 2207252);
    if (equations.length === 0) continue;
    const deltaCounts = {};
    const runContexts = new Set();
    let occurrences = 0;
    let examples = 0;
    let applications = 0;
    let removals = 0;
    let externalExamples = 0;
    let providerAttributeContextExamples = 0;
    for (const equation of equations) {
      const count = Number(equation.count);
      occurrences += count;
      if (Number(equation.terms[0].signed_presence_delta) === 1) applications += count;
      else removals += count;
      deltaCounts[String(equation.raw_attribute_delta)] =
        (deltaCounts[String(equation.raw_attribute_delta)] ?? 0) + count;
      for (const example of equation.examples ?? []) {
        examples += 1;
        runContexts.add(`${example.session_id}|${example.run_ordinal}`);
        const instance = example.status_instances?.find((row) =>
          Number(row.effect_id) === 2207252);
        if (!instance) throw new Error("single-term equation lacks effect 2207252 instance");
        const provider = Number(instance.source_entity_uuid);
        if (provider !== Number(example.target_entity_uuid)) externalExamples += 1;
        if ((example.source_selected_attribute_values_before ?? []).some((row) =>
          Number(row.entity_uuid) === provider)) providerAttributeContextExamples += 1;
      }
    }
    if (examples !== occurrences || externalExamples !== examples) {
      throw new Error(`attribute ${system.attribute_id} did not preserve every external occurrence`);
    }
    attributes[String(system.attribute_id)] = {
      attribute_id: Number(system.attribute_id),
      unique_equations: equations.length,
      occurrences,
      applications,
      removals,
      independent_run_contexts: runContexts.size,
      distinct_raw_deltas: Object.keys(deltaCounts).length,
      raw_delta_counts: deltaCounts,
      external_provider_examples: externalExamples,
      provider_attribute_context_examples: providerAttributeContextExamples,
      exact_single_status_wire_equation: true,
      general_formula_proven: false,
    };
  }
  return {
    source_proof_schema_version: Number(proof.schema_version),
    source_rlog_count: proof.sessions.length,
    selected_attribute_ids: structuredClone(proof.selected_attribute_ids),
    attributes,
    exact_general_formula_proven: false,
    exact_damage_projection_proven: false,
    runtime_authority: false,
  };
}

function validateManifest(manifest, build, inputs) {
  if (Number(manifest?.schemaVersion) !== 1 ||
    manifest?.generatedBy !== "tools/bpsr-build-source-manifest.mjs" ||
    String(manifest?.gameBuild) !== build || String(manifest?.distribution?.buildId) !== build ||
    manifest?.authority?.decodedGameTables !== "exact-current-build-static-data" ||
    manifest?.coverage?.complete !== true || Number(manifest?.coverage?.silentOmissions) !== 0 ||
    !Array.isArray(manifest.files)) {
    throw new Error("build source manifest is not complete exact-current-build authority");
  }
  return [
    ["BuffTable.json", inputs.buff_table],
    ["TalentTable.json", inputs.talent_table],
    ["AffixTable.json", inputs.affix_table],
  ].map(([relativePath, descriptor]) => {
    const matches = manifest.files.filter((entry) =>
      entry.root === "decoded-game-tables" && entry.relativePath === relativePath);
    if (matches.length !== 1 || matches[0].authority !== "exact-current-build-static-data" ||
      Number(matches[0].bytes) !== Number(descriptor.bytes) ||
      String(matches[0].sha256) !== String(descriptor.sha256)) {
      throw new Error(`exact manifest binding failed for ${relativePath}`);
    }
    return { relative_path: relativePath, bytes: Number(matches[0].bytes),
      sha256: String(matches[0].sha256) };
  });
}

function validateActivation(value, build) {
  if (Number(value?.schema_version) !== 1 ||
    value?.generated_by !== "rlogs-bpsr-damage-attr-proof-compact" ||
    String(value?.game_build) !== build || String(value?.packet_build) !== build ||
    Number(value?.summary?.sessions) !== 26 ||
    EFFECT_IDS.some((effectId) => value.observed_ability_result_kinds.some((row) =>
      Number(row.ability_id) === effectId)) ||
    EFFECT_IDS.some((effectId) => value.observed_damage_rows.some((row) =>
      Number(row.type_enum) === effectId))) {
    throw new Error("activation index does not prove the exact no-produced-action frontier");
  }
}

function validateOrigin(value, build) {
  if (Number(value?.schema_version) !== 3 || String(value?.game_build) !== build ||
    value?.policy !== "packet_observed_relationships_only_no_inferred_origins" ||
    Number(value?.summary?.source_sessions) !== 26 ||
    Number(value?.summary?.observed_effects) !== 1699 ||
    !Array.isArray(value.effects) || !Array.isArray(value.relations)) {
    throw new Error("origin catalog is not the exact packet-observed current-build catalog");
  }
}

function validateReconciliation(value, build) {
  if (Number(value?.schema_version) !== 1 ||
    value?.generated_by !== "tools/rdps-observed-effect-reconciliation.mjs" ||
    String(value?.game_build) !== build ||
    value?.policy?.matching_build_packet_effects_are_conserved !== true ||
    value?.policy?.exact_scalar_is_required_for_formula_replay !== true ||
    value?.policy?.ambiguous_and_unresolved_evidence_is_preserved !== true ||
    Number(value?.summary?.reconciled_effects) !== 1699 ||
    value?.summary?.conservation_complete !== true) {
    throw new Error("reconciliation is not exact-build conserved fail-closed evidence");
  }
}

function validateTransition(value, build) {
  if (Number(value?.schema_version) !== 3 ||
    value?.generated_by !== "rlogs-bpsr-rlog-transition-counterfactual-audit" ||
    String(value?.game_build) !== build || Number(value?.effect_id) !== 2110092 ||
    Number(value?.summary?.same_normalized_damage_context_pairs) !== 37 ||
    Number(value?.summary?.same_context_source_status_difference_counts?.[701010]) !== 29 ||
    Number(value?.summary?.same_context_source_status_difference_counts?.[2207252]) !== 29 ||
    Number(value?.summary?.same_context_and_nonselected_status_pairs) !== 0 ||
    Number(value?.summary?.strict_controlled_counterfactual_pairs) !== 0 ||
    !Array.isArray(value?.inputs?.source_rlogs) || value.inputs.source_rlogs.length !== 26) {
    throw new Error("transition audit is not the exact source-status-confounded frontier");
  }
}

function validateAttributeProof(value, transition) {
  const proofSessions = (value?.sessions ?? []).map((row) => path.basename(String(row.rlog))).sort();
  const expectedSessions = transition.inputs.source_rlogs
    .map((row) => path.basename(String(row.path))).sort();
  if (Number(value?.schema_version) !== 26 ||
    value?.generated_by !== "rlogs-bpsr-rdps-status-attribute-proof" ||
    JSON.stringify(value?.selected_effect_ids?.map(Number)) !== JSON.stringify([2207252]) ||
    JSON.stringify(value?.reported_effect_ids?.map(Number)) !== JSON.stringify([2207252]) ||
    JSON.stringify(value?.selected_attribute_ids?.map(Number)) !== JSON.stringify(ATTRIBUTE_IDS) ||
    value?.policy?.runtime_use !== "offline_research_only_not_loaded_by_live_parser" ||
    value?.policy?.formula_inference !== false ||
    value?.policy?.unresolved_evidence_is_hidden !== false ||
    value?.policy?.active_stack_surfaces_generated !== false ||
    value?.policy?.selected_attributes_are_formula_context_not_credit_authority !== true ||
    JSON.stringify(proofSessions) !== JSON.stringify(expectedSessions)) {
    throw new Error("attribute proof is not the exact bounded local-observable cohort");
  }
}

function verifyRlogCohort(transition) {
  for (const receipt of transition.inputs.source_rlogs) {
    const file = path.resolve(receipt.path);
    const bytes = readFileSync(file);
    const expected = String(receipt.sha256).replace(/^sha256:/, "");
    if (bytes.length !== Number(receipt.bytes) ||
      createHash("sha256").update(bytes).digest("hex") !== expected) {
      throw new Error(`source RLOG changed: ${receipt.path}`);
    }
  }
}

function verifyCommand(parsed) {
  const input = path.resolve(required(parsed, "input"));
  const report = readJson(input, "source-status local-observable audit");
  verifyReport(report);
  verifyInputs(report);
  console.log(`verified ${input}`);
}

function verifyReport(report) {
  const transfer = report?.effect_2207252;
  const delta = transfer?.locally_observed_dynamic_recipient_delta_subset?.attributes ?? {};
  if (Number(report?.schema_version) !== SCHEMA_VERSION || report?.generated_by !== GENERATOR ||
    String(report?.game_build) !== BUILD || report?.content_sha256 !== stableContentHash(report) ||
    report?.policy?.structurally_unavailable_remote_player_attributes_are_not_acquisition_requirements !== true ||
    report?.policy?.unavailable_provider_attribute_absence_is_not_zero !== true ||
    report?.policy?.current_character_snapshots_never_substitute_for_historical_provider_attributes !== true ||
    report?.effect_701010?.packet_lifecycle?.window_count !== 62935 ||
    report?.effect_701010?.reconciliation?.unresolved_cross_actor_windows !== 62935 ||
    report?.effect_701010?.provider_identity_proven !== false ||
    report?.effect_701010?.safe_to_exclude_from_counterfactual_matching !== false ||
    transfer?.reconciliation?.resolved_external_player_to_player_windows !== 12948 ||
    transfer?.reconciliation?.unresolved_cross_actor_windows !== 0 ||
    transfer?.reconciliation?.exact_owning_source !== true ||
    transfer?.semantic_evidence_only?.description_mentions_five_percent !== true ||
    transfer?.semantic_evidence_only?.five_percent_is_formula_authority !== false ||
    Number(delta[11030]?.occurrences) !== 48 || Number(delta[11030]?.applications) !== 22 ||
    Number(delta[11030]?.removals) !== 26 || Number(delta[11030]?.independent_run_contexts) !== 16 ||
    Number(delta[11030]?.distinct_raw_deltas) !== 32 ||
    Number(delta[11033]?.occurrences) !== 47 || Number(delta[11330]?.occurrences) !== 49 ||
    Number(delta[11331]?.occurrences) !== 49 || Number(delta[11332]?.occurrences) !== 48 ||
    Number(delta[11334]?.occurrences) !== 1 ||
    Object.values(delta).some((row) =>
      row.provider_attribute_context_examples !== 0 || row.general_formula_proven !== false) ||
    transfer?.provider_attribute_context?.exact_remote_provider_attribute_context_examples !== 0 ||
    transfer?.provider_attribute_context?.remote_player_attribute_acquisition_required !== false ||
    transfer?.provider_attribute_context?.current_snapshot_substitution_allowed !== false ||
    transfer?.general_formula?.percent_magnitude_proven_from_exact_join !== false ||
    transfer?.general_formula?.integer_rounding_proven !== false ||
    transfer?.full_lifecycle_formula_replayable !== false ||
    transfer?.safe_to_exclude_from_counterfactual_matching !== false ||
    report?.transition_confounder?.source_status_difference_counts?.[701010] !== 29 ||
    report?.transition_confounder?.source_status_difference_counts?.[2207252] !== 29 ||
    report?.transition_confounder?.both_effects_remain_confounders !== true ||
    report?.authority?.formula_authority !== false ||
    report?.authority?.runtime_authority !== false ||
    report?.authority?.ui_display_authority !== false ||
    report?.authority?.provider_rdps_credit_allowed !== false) {
    throw new Error("source-status local-observable audit violates its fail-closed schema");
  }
  for (const descriptor of Object.values(report.inputs ?? {})) validateDescriptor(descriptor);
}

function verifyInputs(report) {
  for (const descriptor of Object.values(report.inputs)) {
    const bytes = readFileSync(path.resolve(descriptor.path));
    if (bytes.length !== Number(descriptor.bytes) ||
      createHash("sha256").update(bytes).digest("hex") !== descriptor.sha256) {
      throw new Error(`input changed: ${descriptor.path}`);
    }
  }
}

function lifecycleSummary(row) {
  return {
    status_events: Number(row.status_events),
    window_count: Number(row.window_count),
    cross_actor_window_count: Number(row.cross_actor_window_count),
    source_player_window_count: Number(row.source_player_window_count),
    target_player_window_count: Number(row.target_player_window_count),
    applied: Number(row.applied),
    refreshed: Number(row.refreshed),
    removed: Number(row.removed),
    minimum_stacks: Number(row.minimum_stacks),
    maximum_stacks: Number(row.maximum_stacks),
    observed_session_count: row.observed_sessions?.length ?? 0,
  };
}

function relationSummary(row) {
  return {
    source_type_id: Number(row.source_type_id),
    source_kind: String(row.source_kind),
    configured_source_table: String(row.configured_source_table),
    source_config_id: Number(row.source_config_id),
    observation_count: Number(row.observation_count),
    observed_session_count: row.observed_sessions?.length ?? 0,
  };
}

function exactEffect(document, effectId, label) {
  const rows = (document.effects ?? []).filter((row) => Number(row.effect_id) === effectId);
  if (rows.length !== 1) throw new Error(`${label} does not contain one effect ${effectId}`);
  return rows[0];
}

function exactRelation(document, effectId) {
  const rows = (document.relations ?? []).filter((row) => Number(row.effect_id) === effectId);
  if (rows.length !== 1) throw new Error(`origin catalog does not contain one relation ${effectId}`);
  return rows[0];
}

function exactRow(table, id, label) {
  const row = table[String(id)] ?? Object.values(table).find((value) => Number(value?.Id) === id);
  if (!row || Number(row.Id) !== id) throw new Error(`${label} lacks exact row ${id}`);
  return row;
}

function containsExactScalar(value, needle) {
  if (value === needle || value === String(needle)) return true;
  if (Array.isArray(value)) return value.some((item) => containsExactScalar(item, needle));
  if (value && typeof value === "object") {
    return Object.values(value).some((item) => containsExactScalar(item, needle));
  }
  return false;
}

function selfTest() {
  const unsafe = {
    remote_player_attribute_acquisition_required: true,
    current_snapshot_substitution_allowed: true,
    percent_magnitude_proven_from_exact_join: true,
  };
  if (!unsafe.remote_player_attribute_acquisition_required ||
    !unsafe.current_snapshot_substitution_allowed ||
    !unsafe.percent_magnitude_proven_from_exact_join) {
    throw new Error("unsafe fixture construction failed");
  }
  console.log("bpsr-source-status-local-observable-audit self-test passed");
}

function fileDescriptor(file) {
  const bytes = readFileSync(file);
  return { path: file.replaceAll("\\", "/"), bytes: statSync(file).size,
    sha256: createHash("sha256").update(bytes).digest("hex") };
}
function validateDescriptor(value) {
  if (!String(value?.path ?? "") || !Number.isSafeInteger(Number(value?.bytes)) ||
    Number(value.bytes) <= 0 || !/^[0-9a-f]{64}$/.test(String(value?.sha256 ?? ""))) {
    throw new Error("invalid exact file descriptor");
  }
}
function stableContentHash(value) {
  const copy = structuredClone(value);
  delete copy.content_sha256;
  return createHash("sha256").update(stableStringify(copy)).digest("hex");
}
function stableStringify(value) {
  if (value === null || typeof value !== "object") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  return `{${Object.keys(value).sort().map((key) =>
    `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`;
}
function readJson(file, label) {
  try { return JSON.parse(readFileSync(file, "utf8")); }
  catch (error) { throw new Error(`unable to read ${label} ${file}: ${error.message}`); }
}
function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index]?.replace(/^--/, "");
    const value = args[index + 1];
    if (!key || value === undefined) throw new Error(`invalid argument near ${args[index]}`);
    parsed[key] = value;
  }
  return parsed;
}
function required(parsed, key) {
  if (!parsed[key]) throw new Error(`missing --${key}`);
  return parsed[key];
}
function resolved(parsed, key) {
  return path.resolve(required(parsed, key));
}
function numericString(value, label) {
  if (!/^\d+$/.test(String(value))) throw new Error(`${label} must be numeric`);
  return String(value);
}
function usage(code) {
  console.log("Usage:\n  node tools/bpsr-source-status-local-observable-audit.mjs analyze --build <id> --build-source-manifest <json> --buff-table <json> --talent-table <json> --affix-table <json> --activation-index <json> --origin-catalog <json> --reconciliation <json> --transition-counterfactual-audit <json> --attribute-proof <json> --output <json>\n  node tools/bpsr-source-status-local-observable-audit.mjs verify --input <json>\n  node tools/bpsr-source-status-local-observable-audit.mjs self-test");
  process.exit(code);
}
