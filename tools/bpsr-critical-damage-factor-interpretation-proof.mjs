#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

const [command = "help", ...argv] = process.argv.slice(2);
const options = parseArgs(argv);

if (command === "build") build(resolveContext(options));
else if (command === "verify") verify(path.resolve(required(options, "input")));
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function resolveContext(parsed) {
  const buildId = required(parsed, "build");
  if (!/^\d+$/.test(buildId)) throw new Error("Build must contain only ASCII digits");
  return {
    buildId,
    buildManifest: path.resolve(required(parsed, "build-manifest")),
    fightAttrTable: path.resolve(required(parsed, "fight-attr-table")),
    combatSurface: path.resolve(required(parsed, "combat-surface")),
    cohortProof: path.resolve(required(parsed, "cohort-proof")),
    controlledPairDiscriminant: path.resolve(required(parsed, "controlled-pair-discriminant")),
    preflight: path.resolve(required(parsed, "preflight")),
    historicalStageLedger: path.resolve(required(parsed, "historical-stage-ledger")),
    runtimeConfig: path.resolve(required(parsed, "runtime-config")),
    runtimeSource: path.resolve(required(parsed, "runtime-source")),
    stateFormulaSource: path.resolve(required(parsed, "state-formula-source")),
    output: path.resolve(required(parsed, "output")),
  };
}

function build(context) {
  for (const [label, file] of [
    ["build source manifest", context.buildManifest],
    ["FightAttrTable", context.fightAttrTable],
    ["IL2CPP combat surface", context.combatSurface],
    ["Inspiration cohort proof", context.cohortProof],
    ["controlled-pair discriminant", context.controlledPairDiscriminant],
    ["current-build preflight", context.preflight],
    ["historical damage-stage ledger", context.historicalStageLedger],
    ["rDPS runtime config", context.runtimeConfig],
    ["rDPS runtime source", context.runtimeSource],
    ["state formula source", context.stateFormulaSource],
  ]) requireFile(file, label);

  const manifest = readJson(context.buildManifest, "build source manifest");
  const table = readJson(context.fightAttrTable, "FightAttrTable");
  const combat = readJson(context.combatSurface, "IL2CPP combat surface");
  const cohort = readJson(context.cohortProof, "Inspiration cohort proof");
  const controlledPairDiscriminant = readJson(
    context.controlledPairDiscriminant,
    "controlled-pair discriminant",
  );
  const preflight = readJson(context.preflight, "current-build preflight");
  const historical = readJson(context.historicalStageLedger, "historical damage-stage ledger");
  const runtimeConfig = readJson(context.runtimeConfig, "rDPS runtime config");
  const runtimeSource = readFileSync(context.runtimeSource, "utf8");
  const stateFormulaSource = readFileSync(context.stateFormulaSource, "utf8");

  const tableDescriptor = descriptor(context.fightAttrTable);
  validateBuildManifest(manifest, context.buildId, tableDescriptor);
  const family = validateFightAttributeFamily(table, combat, context.buildId);
  const cohortAudit = validateCohort(cohort, context.buildId);
  const controlledPairAudit = validateControlledPairDiscriminant(
    controlledPairDiscriminant,
    context.buildId,
    descriptor(context.cohortProof),
    cohortAudit,
  );
  const preflightAudit = validatePreflight(preflight, context.buildId);
  const historicalLead = validateHistoricalLead(historical, context.buildId);
  const runtimeGate = validateRuntimeGate(
    runtimeConfig,
    runtimeSource,
    stateFormulaSource,
    context.buildId,
  );

  const report = {
    schema_version: 4,
    generated_by: "tools/bpsr-critical-damage-factor-interpretation-proof.mjs",
    game: "blue-protocol-star-resonance",
    deployment_id: "global",
    channel: "steam",
    game_build: context.buildId,
    proof_state:
      "exact-current-build-critical-damage-family-identity-and-local-sync-scope-proven-damage-factor-interpretation-open",
    policy: {
      exact_numeric_attribute_ids_and_build_are_authoritative: true,
      enum_and_localized_names_are_evidence_only: true,
      remote_player_cast_packets_required: false,
      remote_player_cast_packets_treated_as_zero: false,
      remote_player_cast_packets_synthesized: false,
      remote_recipient_attribute_snapshots_required: false,
      current_player_attribute_snapshots_substituted_for_remote_players: false,
      historical_packet_formula_substituted_into_current_build: false,
      compatible_candidate_count_is_formula_authority: false,
      damage_factor_interpretation_authority: false,
      runtime_config_interpretation_and_authority_must_advance_together: true,
      candidate_arithmetic_is_runtime_authority: false,
      unresolved_evidence_is_hidden: false,
      provider_rdps_credit_authorized: false,
      runtime_promotion_allowed: false,
      ui_display_allowed: false,
    },
    exact_build_identity: {
      build_manifest_entry_id: "decoded-game-tables:FightAttrTable.json",
      build_manifest_authority: "exact-current-build-static-data",
      fight_attr_table_bytes: tableDescriptor.bytes,
      fight_attr_table_sha256: tableDescriptor.sha256,
      il2cpp_surface_build_matches: true,
      cohort_build_matches: true,
    },
    critical_damage_attribute_family: family,
    client_consumer_boundary: {
      authoritative_damage_formula_location: combat.findings.authoritative_damage_formula_location,
      client_damage_entrypoint: combat.findings.client_damage_entrypoint,
      client_damage_entrypoint_state: combat.findings.client_damage_entrypoint_state,
      current_client_damage_factor_operator_present: false,
      server_operation_order_and_integer_rounding_proven: false,
      runtime_formula_authority: false,
    },
    current_build_cohort: cohortAudit,
    controlled_pair_discriminant: controlledPairAudit,
    historical_lead: historicalLead,
    runtime_interpretation_gate: runtimeGate,
    production_gates: preflightAudit,
    interpretation_resolution: {
      authoritative_interpretation: null,
      retained_candidates: ["additive_bonus", "direct_total"],
      additive_candidate_expression:
        "round(base * (10000 + attribute_12510) / 10000)",
      direct_candidate_expression: "round(base * attribute_12510 / 10000)",
      reason_unresolved:
        "The exact current client exposes family identity and local state but not the server damage operator; cohort compatibility is non-unique and contains rows compatible with each interpretation and with neither.",
      formula_authority: false,
    },
    required_next_evidence: [
      "same-build locally observed controlled critical damage pairs with identical ability, hit, target, damage-script row, owner stage, noncritical formula inputs, and exact attribute-12510 transition",
      "exact current-build damage consumer operation order and integer rounding, or equivalent controlled packet replay that uniquely rejects one interpretation",
      "canonical replay conservation over every attributed packet row",
      "current-build protocol-pack identity and protocol-event coverage gates",
    ],
    runtime_decision: {
      provider_rdps_credit_allowed: false,
      runtime_catalog_promotion_allowed: false,
      ui_rdps_display_allowed: false,
      ordinary_damage_totals_unchanged: true,
    },
    inputs: {
      build_manifest: descriptor(context.buildManifest),
      fight_attr_table: tableDescriptor,
      il2cpp_combat_surface: descriptor(context.combatSurface),
      inspiration_cohort_proof: descriptor(context.cohortProof),
      controlled_pair_discriminant: descriptor(context.controlledPairDiscriminant),
      current_build_preflight: descriptor(context.preflight),
      historical_stage_ledger: descriptor(context.historicalStageLedger),
      runtime_config: descriptor(context.runtimeConfig),
      runtime_source: descriptor(context.runtimeSource),
      state_formula_source: descriptor(context.stateFormulaSource),
    },
  };
  report.content_sha256 = contentHash(report);
  validateReport(report);
  writeFileSync(context.output, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  console.log(
    `Critical-damage factor interpretation proof built for ${context.buildId}; ` +
      `${cohortAudit.critical_stage_events} critical-stage events retained and formula authority remains false.`,
  );
}

function verify(input) {
  requireFile(input, "critical-damage factor interpretation proof");
  const report = readJson(input, "critical-damage factor interpretation proof");
  validateReport(report);
  console.log(
    `Critical-damage factor interpretation proof verified for build ${report.game_build}; ` +
      "additive/direct interpretation remains open and runtime/UI credit remains disabled.",
  );
}

function validateBuildManifest(manifest, buildId, tableDescriptor) {
  if (Number(manifest?.schemaVersion) !== 1 || String(manifest?.gameBuild) !== buildId) {
    throw new Error("Build source manifest identity mismatch");
  }
  const entry = (manifest.files ?? []).find(
    (row) => row?.id === "decoded-game-tables:FightAttrTable.json",
  );
  if (!entry || entry.authority !== "exact-current-build-static-data" ||
    entry.relativePath !== "FightAttrTable.json" ||
    Number(entry.bytes) !== tableDescriptor.bytes ||
    String(entry.sha256) !== tableDescriptor.sha256 ||
    !Array.isArray(entry.proofSuites) || !entry.proofSuites.includes("formula-stage-replay")) {
    throw new Error("FightAttrTable is not bound to the exact build manifest");
  }
}

function validateFightAttributeFamily(table, combat, buildId) {
  const row = (Array.isArray(table) ? table : Object.values(table)).find(
    (entry) => Number(entry?.Id) === 12510,
  );
  const ids = [row?.Id, row?.AttrTotal, row?.AttrAdd, row?.AttrExAdd, row?.AttrPer,
    row?.AttrExPer].map(Number);
  if (JSON.stringify(ids) !== JSON.stringify([12510, 12511, 12512, 12513, 12514, 12515]) ||
    row?.Type !== "int32" || Number(row?.AttrNumType) !== 1 || Number(row?.BaseAttr) !== 0 ||
    row?.IsSyncMe !== true || row?.IsSyncAoi !== false) {
    throw new Error("Exact critical-damage FightAttr family identity changed");
  }
  if (Number(combat?.schema_version) !== 2 || String(combat?.build_id) !== buildId ||
    combat?.policy?.runtime_formula_authority !== false ||
    combat?.findings?.authoritative_damage_formula_location !==
      "server-side; absent from the client IL2CPP surface") {
    throw new Error("Current IL2CPP damage-consumer boundary is not fail-closed");
  }
  const nativeFamily = (combat.fight_attribute_families ?? []).find(
    (entry) => Number(entry?.base_id) === 12510,
  );
  if (!nativeFamily || JSON.stringify((nativeFamily.members ?? []).map((entry) => Number(entry.value))) !==
      JSON.stringify(ids)) {
    throw new Error("Current IL2CPP enum family does not match FightAttrTable");
  }
  return {
    current_attribute_id: ids[0],
    total_attribute_id: ids[1],
    add_attribute_id: ids[2],
    extra_add_attribute_id: ids[3],
    percent_attribute_id: ids[4],
    extra_percent_attribute_id: ids[5],
    numeric_type: row.AttrNumType,
    storage_type: row.Type,
    base_attribute_value: row.BaseAttr,
    sync_to_local_player: row.IsSyncMe,
    sync_to_area_of_interest: row.IsSyncAoi,
    exact_static_sync_scope: "local-player-only-not-AOI",
    enum_name_evidence: row.EnumName,
    official_name_evidence: row.OfficialName,
    description_evidence: row.AttrDes,
    family_member_enum_evidence: nativeFamily.members,
    names_are_runtime_keys: false,
    damage_consumer_semantics_proven: false,
  };
}

function validateCohort(cohort, buildId) {
  if (Number(cohort?.schema_version) !== 18 || String(cohort?.game_build) !== buildId ||
    cohort?.deployment_id !== "global" || Number(cohort?.effect_id) !== 2202041 ||
    cohort?.policy?.remote_player_packets_required !== false ||
    cohort?.policy?.remote_player_packets_treated_as_zero !== false ||
    cohort?.policy?.remote_player_packets_synthesized !== false ||
    cohort?.policy?.critical_damage_raw_interpretation_authority !== false) {
    throw new Error("Current-build Inspiration cohort policy is unsafe or mismatched");
  }
  const coverage = cohort.integer_stage_counterfactual_coverage;
  const rows = coverage?.critical_factor_interpretation_breakdown;
  const validRelations = new Map([
    ["both", new Set(["same_exact", "divergent_exact", "within_interpretation_unresolved"])],
    ["additive_only", new Set(["single_interpretation_exact", "within_interpretation_unresolved"])],
    ["direct_only", new Set(["single_interpretation_exact", "within_interpretation_unresolved"])],
    ["neither", new Set(["no_compatible_interpretation"])],
  ]);
  if (!coverage || coverage.candidate_family_authority !== false ||
    coverage.counterfactual_authority !== false ||
    coverage.critical_factor_interpretation_breakdown_authority !== false ||
    !Array.isArray(coverage.candidate_family) || coverage.candidate_family.length !== 6 ||
    !Array.isArray(rows) || rows.length === 0 ||
    !Array.isArray(coverage.critical_factor_event_records)) {
    throw new Error("Critical-factor candidate coverage is incomplete");
  }
  const criticalEvents = exactCount(coverage.critical_stage_events, "critical stage events");
  if (coverage.critical_factor_event_records.length !== criticalEvents) {
    throw new Error("Per-event critical-factor evidence does not conserve critical events");
  }
  const compatible = exactCount(
    coverage.events_with_at_least_one_compatible_candidate,
    "compatible events",
  );
  const noCompatible = exactCount(
    coverage.events_without_compatible_candidates,
    "events without compatible candidates",
  );
  const exact = exactCount(coverage.exact_stage_independent_events, "exact events");
  const unresolved = exactCount(
    coverage.unresolved_stage_or_rounding_events,
    "unresolved events",
  );
  if (compatible + noCompatible !== criticalEvents || exact + unresolved !== compatible) {
    throw new Error("Critical-factor cohort totals do not conserve events");
  }
  const seen = new Set();
  const compatibility = { both: 0, additive_only: 0, direct_only: 0, neither: 0 };
  for (const row of rows) {
    const pathName = String(row?.path ?? "");
    const className = String(row?.compatibility ?? "");
    const relation = String(row?.counterfactual_relation ?? "");
    const key = JSON.stringify([pathName, className, relation]);
    const events = exactCount(row?.events, "interpretation row events", true);
    if (!["critical_proc_bonus", "combined_lucky_occurrence_and_critical_bonus"].includes(pathName) ||
      !validRelations.get(className)?.has(relation) || seen.has(key) ||
      row?.formula_authority !== false) {
      throw new Error("Critical-factor interpretation row is invalid");
    }
    seen.add(key);
    if (pathName === "critical_proc_bonus") compatibility[className] += events;
  }
  if (rows.reduce((sum, row) => sum + Number(row.events), 0) !== criticalEvents) {
    throw new Error("Critical-factor interpretation rows do not conserve events");
  }
  return {
    exact_rlogs: cohort.rlogs.length,
    critical_stage_events: criticalEvents,
    events_with_complete_stage_inputs: exactCount(
      coverage.events_with_complete_stage_inputs,
      "complete stage inputs",
    ),
    events_with_compatible_interpretation: compatible,
    events_without_compatible_interpretation: noCompatible,
    interpretation_stable_exact_counterfactual_events: exact,
    unresolved_order_rounding_or_interpretation_events: unresolved,
    critical_only_compatibility_counts: compatibility,
    interpretation_breakdown: structuredClone(rows),
    candidate_family: structuredClone(coverage.candidate_family),
    formula_authority: false,
  };
}

function validateControlledPairDiscriminant(report, buildId, cohortDescriptor, cohortAudit) {
  if (Number(report?.schema_version) !== 2 ||
    report?.generated_by !== "tools/bpsr-critical-factor-controlled-pair-discriminant.mjs" ||
    String(report?.game_build) !== buildId ||
    report?.content_sha256 !== stableContentHash(report) ||
    Number(report?.input?.bytes) !== cohortDescriptor.bytes ||
    String(report?.input?.sha256) !== cohortDescriptor.sha256 ||
    report?.policy?.remote_player_cast_packets_required !== false ||
    report?.policy?.remote_player_cast_packets_treated_as_zero !== false ||
    report?.policy?.remote_player_cast_packets_synthesized !== false ||
    report?.policy?.aggregate_compatibility_counts_are_formula_authority !== false ||
    report?.policy?.exclusive_candidate_fit_counts_are_votes !== false ||
    report?.policy?.provider_rdps_credit_allowed !== false ||
    Number(report?.observed_coverage?.critical_stage_events) !==
      Number(cohortAudit.critical_stage_events) ||
    Number(report?.controlled_pair_eligibility?.eligible_controlled_pairs) !== 0 ||
    report?.controlled_pair_eligibility?.authoritative_interpretation !== null ||
    report?.controlled_pair_eligibility?.formula_authority !== false ||
    report?.controlled_pair_contract?.remote_player_cast_packet_required !== false ||
    report?.controlled_pair_contract?.current_character_snapshot_substitution_allowed !== false ||
    report?.runtime_decision?.provider_rdps_credit_allowed !== false ||
    report?.runtime_decision?.runtime_catalog_promotion_allowed !== false ||
    report?.runtime_decision?.ui_rdps_display_allowed !== false) {
    throw new Error("Controlled-pair discriminant is not bound, reproducible, and fail-closed");
  }
  const retained = report.retained_evidence_sufficiency ?? {};
  const eligibility = report.controlled_pair_eligibility ?? {};
  const derivedCandidatePairs = exactCount(
    eligibility.derived_candidate_pairs,
    "derived controlled-pair candidates",
    true,
  );
  if (derivedCandidatePairs === 0 ||
    Number(retained.aggregate_group_pair_candidates?.pairs) !== derivedCandidatePairs ||
    retained.explicit_controlled_pair_records_present !== true ||
    Number(retained.explicit_controlled_pair_records) !== derivedCandidatePairs ||
    exactCount(retained.local_event_time_state_authority_groups?.groups,
      "local event-time authority groups") !== 0 ||
    exactCount(retained.complete_attack_mitigation_preimage_groups?.groups,
      "complete preimage groups") !== 0 ||
    exactCount(retained.exact_surface_owner_stage_authority_groups?.groups,
      "surface owner-stage authority groups") !== 0 ||
    eligibility.blocker !==
      "candidate-pairs-retained-but-local-event-time-or-complete-preimage-authority-missing") {
    throw new Error("Controlled-pair candidate evidence is not retained fail-closed");
  }
  return {
    proof_schema_version: 2,
    eligible_controlled_pairs: 0,
    derived_candidate_pairs: derivedCandidatePairs,
    multiple_event_identity_groups:
      Number(retained.multiple_event_identity_groups?.groups),
    multiple_critical_damage_value_groups:
      Number(retained.multiple_critical_damage_value_groups?.groups),
    all_stage_inputs_same_wire_groups:
      Number(retained.all_stage_inputs_same_wire_as_damage?.groups),
    zero_age_stage_input_groups:
      Number(retained.zero_age_stage_inputs?.groups),
    exact_surface_rows_with_selected_coefficients:
      Number(retained.exact_surface_rows_with_selected_coefficients?.events),
    local_event_time_state_authority_groups:
      Number(retained.local_event_time_state_authority_groups?.groups),
    complete_attack_mitigation_preimage_groups:
      Number(retained.complete_attack_mitigation_preimage_groups?.groups),
    exact_surface_owner_stage_authority_groups:
      Number(retained.exact_surface_owner_stage_authority_groups?.groups),
    blocker: report.controlled_pair_eligibility.blocker,
    authoritative_interpretation: null,
    formula_authority: false,
  };
}

function validatePreflight(preflight, buildId) {
  const missingRequired = (preflight?.inputs ?? []).filter(
    (entry) => entry?.required === true && entry?.status === "missing",
  );
  if (Number(preflight?.schema_version) !== 1 || String(preflight?.game_build) !== buildId ||
    Number(preflight?.summary?.planned_inputs) !== 48 ||
    Number(preflight?.summary?.present_required_inputs) !== 45 ||
    Number(preflight?.summary?.missing_required_inputs) !== 1 ||
    preflight?.ready_for_snapshot !== false || preflight?.runtime_promotion_allowed !== false ||
    missingRequired.length !== 1 || missingRequired[0].id !== "protocol-pack-identity" ||
    !String(missingRequired[0].path).endsWith(`/steam-${buildId}/pack.json`) ||
    JSON.stringify(preflight.required_proof_suites_from_missing_inputs) !==
      JSON.stringify(["canonical-replay-conservation", "protocol-event-coverage"])) {
    throw new Error("Current-build production preflight no longer matches the fail-closed boundary");
  }
  return {
    ready_for_snapshot: false,
    runtime_promotion_allowed: false,
    missing_required_input: structuredClone(missingRequired[0]),
    required_proof_suites: structuredClone(preflight.required_proof_suites_from_missing_inputs),
  };
}

function validateHistoricalLead(historical, currentBuild) {
  const critical = (historical?.proven_stages ?? []).find(
    (entry) => entry?.stage_id === "critical_outcome_modifier",
  );
  const combined = (historical?.proven_stages ?? []).find(
    (entry) => entry?.stage_id === "inspiration_combined_critical_lucky_occurrence",
  );
  if (String(historical?.client_build) === currentBuild ||
    historical?.policy?.runtime_formula_authority !== false ||
    historical?.policy?.missing_packet_attribute_is_zero !== false ||
    historical?.policy?.unresolved_evidence_is_hidden !== false ||
    Number(critical?.attribute_id) !== 12510 || critical?.generic_stage_order_proven !== false ||
    !String(combined?.integer_formula ?? "").includes("(10000+cd)")) {
    throw new Error("Historical additive formula lead is missing or incorrectly current-scoped");
  }
  return {
    historical_build: String(historical.client_build),
    interpretation_lead: "additive_bonus",
    historical_formula: combined.integer_formula,
    current_build_formula_authority: false,
    substitution_into_current_build_allowed: false,
  };
}

function validateRuntimeGate(runtime, runtimeSource, stateFormulaSource, buildId) {
  const requiredRuntimeMarkers = [
    "const RDPS_RUNTIME_SCHEMA_VERSION: u16 = 5;",
    "promotion_blockers",
    "critical_damage_factor_interpretation_authority",
    "critical_damage_interpretation_is_consistent",
    "CriticalDamageFactorInterpretation::Unresolved",
  ];
  const requiredFormulaMarkers = [
    "pub enum CriticalDamageFactorInterpretation",
    "Unresolved",
    "AdditiveBonus",
    "DirectTotal",
    "fn factor_and_bonus",
    "interpretation: CriticalDamageFactorInterpretation",
  ];
  const expectedPromotionBlockers = [
    "protocol-pack-identity",
    "canonical-replay-conservation",
    "protocol-event-coverage",
    "critical-damage-factor-interpretation-authority",
    "party-support-formula-frontier",
  ];
  if (Number(runtime?.schema_version) !== 5 || String(runtime?.game_build) !== buildId ||
    runtime?.promotion_state !== "blocked-current-build-proof-gates-open" ||
    JSON.stringify(runtime?.promotion_blockers) !== JSON.stringify(expectedPromotionBlockers) ||
    runtime?.critical_damage_factor_interpretation !== "unresolved" ||
    runtime?.policy?.critical_damage_factor_interpretation_authority !== false ||
    runtime?.policy?.candidate_rules_enabled !== false ||
    runtime?.policy?.runtime_promotion_allowed !== false ||
    runtime?.inspiration?.runtime_transfer_enabled !== false ||
    requiredRuntimeMarkers.some((marker) => !runtimeSource.includes(marker)) ||
    requiredFormulaMarkers.some((marker) => !stateFormulaSource.includes(marker))) {
    throw new Error("Current-build critical-damage runtime interpretation gate is not fail-closed");
  }
  return {
    runtime_schema_version: 5,
    promotion_blockers: expectedPromotionBlockers,
    configured_interpretation: "unresolved",
    configured_interpretation_authority: false,
    candidate_rules_enabled: false,
    runtime_promotion_allowed: false,
    inspiration_runtime_transfer_enabled: false,
    interpretation_and_authority_must_advance_together: true,
    unresolved_interpretation_blocks_critical_dependent_projection: true,
    retained_candidate_arithmetic_implemented: ["additive_bonus", "direct_total"],
    candidate_arithmetic_formula_authority: false,
  };
}

function validateReport(report) {
  if (Number(report?.schema_version) !== 4 ||
    report?.generated_by !== "tools/bpsr-critical-damage-factor-interpretation-proof.mjs" ||
    report?.proof_state !==
      "exact-current-build-critical-damage-family-identity-and-local-sync-scope-proven-damage-factor-interpretation-open" ||
    !/^\d+$/.test(String(report?.game_build)) ||
    report?.content_sha256 !== contentHash(report)) {
    throw new Error("Critical-damage interpretation proof identity or content hash is invalid");
  }
  const policy = report.policy ?? {};
  if (policy.exact_numeric_attribute_ids_and_build_are_authoritative !== true ||
    policy.enum_and_localized_names_are_evidence_only !== true ||
    policy.remote_player_cast_packets_required !== false ||
    policy.remote_player_cast_packets_treated_as_zero !== false ||
    policy.remote_player_cast_packets_synthesized !== false ||
    policy.remote_recipient_attribute_snapshots_required !== false ||
    policy.current_player_attribute_snapshots_substituted_for_remote_players !== false ||
    policy.historical_packet_formula_substituted_into_current_build !== false ||
    policy.compatible_candidate_count_is_formula_authority !== false ||
    policy.damage_factor_interpretation_authority !== false ||
    policy.runtime_config_interpretation_and_authority_must_advance_together !== true ||
    policy.candidate_arithmetic_is_runtime_authority !== false ||
    policy.unresolved_evidence_is_hidden !== false ||
    policy.provider_rdps_credit_authorized !== false ||
    policy.runtime_promotion_allowed !== false || policy.ui_display_allowed !== false) {
    throw new Error("Critical-damage interpretation proof policy is unsafe");
  }
  const family = report.critical_damage_attribute_family ?? {};
  if (JSON.stringify([
    family.current_attribute_id, family.total_attribute_id, family.add_attribute_id,
    family.extra_add_attribute_id, family.percent_attribute_id,
    family.extra_percent_attribute_id,
  ]) !== JSON.stringify([12510, 12511, 12512, 12513, 12514, 12515]) ||
    family.sync_to_local_player !== true || family.sync_to_area_of_interest !== false ||
    family.names_are_runtime_keys !== false || family.damage_consumer_semantics_proven !== false) {
    throw new Error("Critical-damage family report is not exact or fail-closed");
  }
  const cohort = report.current_build_cohort ?? {};
  if (cohort.events_with_compatible_interpretation +
      cohort.events_without_compatible_interpretation !== cohort.critical_stage_events ||
    cohort.interpretation_stable_exact_counterfactual_events +
      cohort.unresolved_order_rounding_or_interpretation_events !==
        cohort.events_with_compatible_interpretation || cohort.formula_authority !== false ||
    !Array.isArray(cohort.interpretation_breakdown) ||
    cohort.interpretation_breakdown.reduce((sum, row) => sum + Number(row.events), 0) !==
      cohort.critical_stage_events || !Array.isArray(cohort.candidate_family) ||
    cohort.candidate_family.length !== 6) {
    throw new Error("Critical-damage cohort report does not conserve its evidence");
  }
  const controlledPair = report.controlled_pair_discriminant ?? {};
  if (Number(controlledPair.proof_schema_version) !== 2 ||
    Number(controlledPair.eligible_controlled_pairs) !== 0 ||
    !Number.isSafeInteger(Number(controlledPair.derived_candidate_pairs)) ||
    Number(controlledPair.derived_candidate_pairs) <= 0 ||
    !Number.isSafeInteger(Number(controlledPair.multiple_event_identity_groups)) ||
    Number(controlledPair.multiple_event_identity_groups) <= 0 ||
    !Number.isSafeInteger(Number(controlledPair.multiple_critical_damage_value_groups)) ||
    Number(controlledPair.multiple_critical_damage_value_groups) <= 0 ||
    Number(controlledPair.all_stage_inputs_same_wire_groups) !== 0 ||
    Number(controlledPair.zero_age_stage_input_groups) !== 0 ||
    Number(controlledPair.local_event_time_state_authority_groups) !== 0 ||
    Number(controlledPair.complete_attack_mitigation_preimage_groups) !== 0 ||
    Number(controlledPair.exact_surface_owner_stage_authority_groups) !== 0 ||
    controlledPair.blocker !==
      "candidate-pairs-retained-but-local-event-time-or-complete-preimage-authority-missing" ||
    controlledPair.authoritative_interpretation !== null ||
    controlledPair.formula_authority !== false) {
    throw new Error("Critical-damage controlled-pair audit is missing or unsafe");
  }
  const runtimeGate = report.runtime_interpretation_gate ?? {};
  if (Number(runtimeGate.runtime_schema_version) !== 5 ||
    JSON.stringify(runtimeGate.promotion_blockers) !== JSON.stringify([
      "protocol-pack-identity",
      "canonical-replay-conservation",
      "protocol-event-coverage",
      "critical-damage-factor-interpretation-authority",
      "party-support-formula-frontier",
    ]) ||
    runtimeGate.configured_interpretation !== "unresolved" ||
    runtimeGate.configured_interpretation_authority !== false ||
    runtimeGate.candidate_rules_enabled !== false ||
    runtimeGate.runtime_promotion_allowed !== false ||
    runtimeGate.inspiration_runtime_transfer_enabled !== false ||
    runtimeGate.interpretation_and_authority_must_advance_together !== true ||
    runtimeGate.unresolved_interpretation_blocks_critical_dependent_projection !== true ||
    JSON.stringify(runtimeGate.retained_candidate_arithmetic_implemented) !==
      JSON.stringify(["additive_bonus", "direct_total"]) ||
    runtimeGate.candidate_arithmetic_formula_authority !== false) {
    throw new Error("Critical-damage runtime interpretation gate receipt is unsafe");
  }
  if (report.client_consumer_boundary?.current_client_damage_factor_operator_present !== false ||
    report.client_consumer_boundary?.server_operation_order_and_integer_rounding_proven !== false ||
    report.interpretation_resolution?.authoritative_interpretation !== null ||
    report.interpretation_resolution?.formula_authority !== false ||
    JSON.stringify(report.interpretation_resolution?.retained_candidates) !==
      JSON.stringify(["additive_bonus", "direct_total"]) ||
    report.historical_lead?.current_build_formula_authority !== false ||
    report.historical_lead?.substitution_into_current_build_allowed !== false ||
    report.production_gates?.ready_for_snapshot !== false ||
    report.production_gates?.runtime_promotion_allowed !== false ||
    report.production_gates?.missing_required_input?.id !== "protocol-pack-identity" ||
    report.runtime_decision?.provider_rdps_credit_allowed !== false ||
    report.runtime_decision?.runtime_catalog_promotion_allowed !== false ||
    report.runtime_decision?.ui_rdps_display_allowed !== false ||
    report.runtime_decision?.ordinary_damage_totals_unchanged !== true ||
    !Array.isArray(report.required_next_evidence) || report.required_next_evidence.length !== 4) {
    throw new Error("Critical-damage interpretation proof promoted unresolved evidence");
  }
  for (const input of Object.values(report.inputs ?? {})) {
    if (!String(input?.path ?? "") || !Number.isSafeInteger(Number(input?.bytes)) ||
      Number(input.bytes) <= 0 || !/^[0-9a-f]{64}$/.test(String(input?.sha256 ?? ""))) {
      throw new Error("Critical-damage interpretation proof input descriptor is invalid");
    }
  }
}

function exactCount(value, label, positive = false) {
  const number = Number(value);
  if (!Number.isSafeInteger(number) || number < (positive ? 1 : 0)) {
    throw new Error(`${label} is not a safe ${positive ? "positive" : "nonnegative"} integer`);
  }
  return number;
}

function descriptor(file) {
  const bytes = readFileSync(file);
  return {
    path: path.relative(process.cwd(), file),
    bytes: bytes.length,
    sha256: createHash("sha256").update(bytes).digest("hex"),
  };
}

function contentHash(report) {
  const copy = structuredClone(report);
  delete copy.content_sha256;
  return createHash("sha256").update(JSON.stringify(copy)).digest("hex");
}

function stableContentHash(report) {
  const copy = structuredClone(report);
  delete copy.content_sha256;
  return createHash("sha256").update(stableStringify(copy)).digest("hex");
}

function stableStringify(value) {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) =>
      `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function selfTest() {
  const valid = {
    schema_version: 4,
    generated_by: "tools/bpsr-critical-damage-factor-interpretation-proof.mjs",
    game_build: "24687926",
    proof_state:
      "exact-current-build-critical-damage-family-identity-and-local-sync-scope-proven-damage-factor-interpretation-open",
    policy: {
      exact_numeric_attribute_ids_and_build_are_authoritative: true,
      enum_and_localized_names_are_evidence_only: true,
      remote_player_cast_packets_required: false,
      remote_player_cast_packets_treated_as_zero: false,
      remote_player_cast_packets_synthesized: false,
      remote_recipient_attribute_snapshots_required: false,
      current_player_attribute_snapshots_substituted_for_remote_players: false,
      historical_packet_formula_substituted_into_current_build: false,
      compatible_candidate_count_is_formula_authority: false,
      damage_factor_interpretation_authority: false,
      runtime_config_interpretation_and_authority_must_advance_together: true,
      candidate_arithmetic_is_runtime_authority: false,
      unresolved_evidence_is_hidden: false,
      provider_rdps_credit_authorized: false,
      runtime_promotion_allowed: false,
      ui_display_allowed: false,
    },
    critical_damage_attribute_family: {
      current_attribute_id: 12510, total_attribute_id: 12511, add_attribute_id: 12512,
      extra_add_attribute_id: 12513, percent_attribute_id: 12514,
      extra_percent_attribute_id: 12515, sync_to_local_player: true,
      sync_to_area_of_interest: false, names_are_runtime_keys: false,
      damage_consumer_semantics_proven: false,
    },
    client_consumer_boundary: {
      current_client_damage_factor_operator_present: false,
      server_operation_order_and_integer_rounding_proven: false,
    },
    current_build_cohort: {
      critical_stage_events: 4,
      events_with_compatible_interpretation: 3,
      events_without_compatible_interpretation: 1,
      interpretation_stable_exact_counterfactual_events: 2,
      unresolved_order_rounding_or_interpretation_events: 1,
      interpretation_breakdown: [
        { path: "critical_proc_bonus", compatibility: "additive_only",
          counterfactual_relation: "single_interpretation_exact", events: 2,
          formula_authority: false },
        { path: "critical_proc_bonus", compatibility: "both",
          counterfactual_relation: "divergent_exact", events: 1,
          formula_authority: false },
        { path: "critical_proc_bonus", compatibility: "neither",
          counterfactual_relation: "no_compatible_interpretation", events: 1,
          formula_authority: false },
      ],
      candidate_family: ["a", "b", "c", "d", "e", "f"],
      formula_authority: false,
    },
    controlled_pair_discriminant: {
      proof_schema_version: 2,
      eligible_controlled_pairs: 0,
      derived_candidate_pairs: 1,
      multiple_event_identity_groups: 1,
      multiple_critical_damage_value_groups: 1,
      all_stage_inputs_same_wire_groups: 0,
      zero_age_stage_input_groups: 0,
      exact_surface_rows_with_selected_coefficients: 1,
      local_event_time_state_authority_groups: 0,
      complete_attack_mitigation_preimage_groups: 0,
      exact_surface_owner_stage_authority_groups: 0,
      blocker: "candidate-pairs-retained-but-local-event-time-or-complete-preimage-authority-missing",
      authoritative_interpretation: null,
      formula_authority: false,
    },
    historical_lead: {
      current_build_formula_authority: false,
      substitution_into_current_build_allowed: false,
    },
    runtime_interpretation_gate: {
      runtime_schema_version: 5,
      promotion_blockers: [
        "protocol-pack-identity",
        "canonical-replay-conservation",
        "protocol-event-coverage",
        "critical-damage-factor-interpretation-authority",
        "party-support-formula-frontier",
      ],
      configured_interpretation: "unresolved",
      configured_interpretation_authority: false,
      candidate_rules_enabled: false,
      runtime_promotion_allowed: false,
      inspiration_runtime_transfer_enabled: false,
      interpretation_and_authority_must_advance_together: true,
      unresolved_interpretation_blocks_critical_dependent_projection: true,
      retained_candidate_arithmetic_implemented: ["additive_bonus", "direct_total"],
      candidate_arithmetic_formula_authority: false,
    },
    production_gates: {
      ready_for_snapshot: false,
      runtime_promotion_allowed: false,
      missing_required_input: { id: "protocol-pack-identity" },
    },
    interpretation_resolution: {
      authoritative_interpretation: null,
      retained_candidates: ["additive_bonus", "direct_total"],
      formula_authority: false,
    },
    required_next_evidence: ["a", "b", "c", "d"],
    runtime_decision: {
      provider_rdps_credit_allowed: false,
      runtime_catalog_promotion_allowed: false,
      ui_rdps_display_allowed: false,
      ordinary_damage_totals_unchanged: true,
    },
    inputs: {
      one: { path: "one", bytes: 1, sha256: "a".repeat(64) },
    },
  };
  valid.content_sha256 = contentHash(valid);
  validateReport(valid);
  const unsafe = structuredClone(valid);
  unsafe.policy.remote_player_cast_packets_treated_as_zero = true;
  unsafe.content_sha256 = contentHash(unsafe);
  let rejected = false;
  try { validateReport(unsafe); } catch { rejected = true; }
  if (!rejected) throw new Error("Self-test failed to reject synthesized remote evidence");
  console.log("Critical-damage factor interpretation proof self-test passed.");
}

function parseArgs(values) {
  const parsed = {};
  for (let index = 0; index < values.length; index += 2) {
    const key = values[index];
    if (!key?.startsWith("--") || values[index + 1] === undefined) {
      throw new Error(`Invalid argument ${key ?? "<missing>"}`);
    }
    parsed[key.slice(2)] = values[index + 1];
  }
  return parsed;
}

function required(parsed, key) {
  if (!parsed[key]) throw new Error(`Missing --${key}`);
  return parsed[key];
}

function readJson(file, label) {
  try { return JSON.parse(readFileSync(file, "utf8")); }
  catch (error) { throw new Error(`${label} is not valid JSON: ${error.message}`); }
}

function requireFile(file, label) {
  if (!existsSync(file)) throw new Error(`${label} does not exist: ${file}`);
}

function usage(exitCode) {
  console.log(
    "Usage: node tools/bpsr-critical-damage-factor-interpretation-proof.mjs build " +
      "--build <id> --build-manifest <json> --fight-attr-table <json> " +
      "--combat-surface <json> --cohort-proof <json> --preflight <json> " +
      "--controlled-pair-discriminant <json> " +
      "--historical-stage-ledger <json> --runtime-config <json> --runtime-source <rs> " +
      "--state-formula-source <rs> --output <json> | verify --input <json> | self-test",
  );
  process.exit(exitCode);
}
