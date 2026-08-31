import { createHash } from "node:crypto";
import { existsSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const SCHEMA_VERSION = 2;
// The schema-18 26-log cohort is 75,296,185 bytes because it retains one
// auditable input record per critical event. Keep the reader bounded above the
// measured artifact without allowing raw or unbounded cohort inputs.
const MAXIMUM_COHORT_BYTES = 96 * 1024 * 1024;
const EXPECTED_CANDIDATE_FAMILY = [
  "critical-only additive-bonus fixed-point stage under floor and nearest-half-up",
  "critical-only direct-total fixed-point stage under floor and nearest-half-up",
  "critical-plus-lucky additive-bonus nested stages in both orders under every floor/nearest-half-up combination",
  "critical-plus-lucky direct-total nested stages in both orders under every floor/nearest-half-up combination",
  "critical-plus-lucky additive-bonus single-product stage under floor and nearest-half-up",
  "critical-plus-lucky direct-total single-product stage under floor and nearest-half-up",
];

const command = process.argv[2];
try {
  if (command === "build") build(parseArgs(process.argv.slice(3)));
  else if (command === "verify") verify(resolve(required(parseArgs(process.argv.slice(3)), "input")));
  else if (command === "self-test") selfTest();
  else usage(command === undefined ? 0 : 1);
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}

function build(args) {
  const buildId = required(args, "build");
  const cohortPath = resolve(required(args, "cohort"));
  const outputPath = resolve(required(args, "output"));
  const cohort = readBoundedCohort(cohortPath);
  if (Number(cohort?.schema_version) !== 18) {
    throw new Error("Controlled-pair schema 2 requires an Inspiration schema-18 per-event cohort");
  }
  const analysis = analyzeCohort(cohort, buildId);
  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: "tools/bpsr-critical-factor-controlled-pair-discriminant.mjs",
    game_build: buildId,
    proof_state: "same-build-local-controlled-pair-discriminant-open-per-event-candidates-audited",
    policy: {
      exact_numeric_effect_and_build_identity_authoritative: true,
      localized_names_are_runtime_keys: false,
      remote_player_cast_packets_required: false,
      remote_player_cast_packets_treated_as_zero: false,
      remote_player_cast_packets_synthesized: false,
      current_or_historical_character_snapshots_substituted: false,
      per_event_stage_inputs_required: true,
      aggregate_compatibility_counts_are_formula_authority: false,
      exclusive_candidate_fit_counts_are_votes: false,
      unresolved_evidence_hidden: false,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
      ui_rdps_display_allowed: false,
    },
    input: fileDescriptor(cohortPath),
    bounded_processing: {
      maximum_input_bytes: MAXIMUM_COHORT_BYTES,
      input_bytes: statSync(cohortPath).size,
      whole_rlog_cohort_deserialized: false,
      source_is_compact_generated_cohort_only: true,
      recommended_node_heap_mib: 384,
    },
    ...analysis,
  };
  report.content_sha256 = contentHash(report);
  validateReport(report);
  writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  console.log(
    `Critical-factor controlled-pair discriminant built for ${buildId}: ` +
      `${analysis.observed_coverage.critical_stage_events} critical events, ` +
      `${analysis.controlled_pair_eligibility.eligible_controlled_pairs} eligible pairs; authority remains open.`,
  );
}

function verify(inputPath) {
  const report = readJson(inputPath, "controlled-pair discriminant");
  validateReport(report);
  const cohortPath = resolve(report.input.path);
  const descriptor = fileDescriptor(cohortPath);
  if (descriptor.bytes !== report.input.bytes || descriptor.sha256 !== report.input.sha256) {
    throw new Error("Controlled-pair discriminant input cohort changed");
  }
  const cohort = readBoundedCohort(cohortPath);
  const expected = analyzeCohort(cohort, String(report.game_build));
  if (stableStringify(expected) !== stableStringify(selectAnalysis(report))) {
    throw new Error("Controlled-pair discriminant does not reproduce from its cohort");
  }
  console.log(
    `Critical-factor controlled-pair discriminant verified for ${report.game_build}: ` +
      `${report.controlled_pair_eligibility.eligible_controlled_pairs} eligible pairs, no formula authority.`,
  );
}

function analyzeCohort(cohort, buildId) {
  const cohortSchema = Number(cohort?.schema_version);
  if (![17, 18].includes(cohortSchema) ||
    cohort?.generated_by !== "rlogs-bpsr-inspiration-proc-attribution-proof" ||
    String(cohort?.game_build) !== buildId || Number(cohort?.effect_id) !== 2_202_041 ||
    cohort?.policy?.remote_player_packets_required !== false ||
    cohort?.policy?.remote_player_packets_treated_as_zero !== false ||
    cohort?.policy?.remote_player_packets_synthesized !== false ||
    cohort?.policy?.critical_damage_raw_interpretation_authority !== false ||
    cohort?.policy?.formula_authority !== false ||
    cohort?.policy?.provider_rdps_credit_authorized !== false) {
    throw new Error("Inspiration cohort identity or fail-closed policy is invalid");
  }
  const coverage = cohort.integer_stage_counterfactual_coverage;
  const groups = coverage?.damage_surface_join?.groups;
  const rows = coverage?.critical_factor_interpretation_breakdown;
  if (!coverage || !Array.isArray(groups) || !Array.isArray(rows) ||
    stableStringify(coverage.candidate_family) !== stableStringify(EXPECTED_CANDIDATE_FAMILY) ||
    coverage.critical_factor_interpretation_breakdown_authority !== false ||
    coverage.candidate_family_authority !== false || coverage.counterfactual_authority !== false) {
    throw new Error("Inspiration critical-stage coverage is incomplete or authoritative");
  }

  const criticalEvents = exactNonnegative(coverage.critical_stage_events, "critical stage events");
  const groupEventSum = groups.reduce(
    (sum, group) => sum + exactNonnegative(group?.events, "identity group events"), 0,
  );
  const interpretationEventSum = rows.reduce(
    (sum, row) => sum + exactNonnegative(row?.events, "interpretation row events"), 0,
  );
  if (groupEventSum !== criticalEvents || interpretationEventSum !== criticalEvents ||
    groups.length !== criticalEvents || rows.some((row) => row?.formula_authority !== false)) {
    throw new Error("Inspiration critical-stage evidence does not conserve events");
  }

  const metric = (predicate) => ({
    groups: groups.filter(predicate).length,
    events: groups.filter(predicate).reduce((sum, group) => sum + Number(group.events), 0),
  });
  const selectedSurface = (group) => {
    const candidates = group?.damage_surface_candidates;
    return group?.damage_surface_resolution === "exactly_one_exact_build_surface_row" &&
      Array.isArray(candidates) && candidates.length === 1 &&
      candidates[0]?.selected_pve_damage_ratio !== null &&
      candidates[0]?.selected_pve_damage_ratio !== undefined &&
      candidates[0]?.selected_pve_fixed_parameter !== null &&
      candidates[0]?.selected_pve_fixed_parameter !== undefined;
  };
  const multipleEvents = metric((group) => Number(group.events) > 1);
  const multipleCriticalValues = metric(
    (group) => Array.isArray(group.critical_damage_raw_values) &&
      group.critical_damage_raw_values.length > 1,
  );
  const sameWireInputs = metric((group) => group.stage_inputs_all_same_wire_as_damage === true);
  const zeroAgeInputs = metric((group) =>
    Number(group.oldest_stage_input_age_sequences) === 0 &&
    Number(group.oldest_stage_input_age_micros) === 0);
  const selectedSurfaceCoefficients = metric(selectedSurface);
  const aggregatePairCandidates = metric((group) =>
    Number(group.events) > 1 &&
    Array.isArray(group.critical_damage_raw_values) && group.critical_damage_raw_values.length > 1 &&
    group.stage_inputs_all_same_wire_as_damage === true && selectedSurface(group));
  const explicitPairs = coverage.controlled_pair_discriminants;
  const explicitPairRecordsPresent = Array.isArray(explicitPairs);
  const eligiblePairs = explicitPairRecordsPresent
    ? explicitPairs.filter(isEligibleExplicitPair).length
    : 0;

  const interpretationCounts = { both: 0, additive_only: 0, direct_only: 0, neither: 0 };
  for (const row of rows) {
    const compatibility = String(row.compatibility);
    if (!(compatibility in interpretationCounts)) {
      throw new Error(`Unknown interpretation compatibility ${compatibility}`);
    }
    interpretationCounts[compatibility] += Number(row.events);
  }

  if (cohortSchema === 17) return {
    observed_coverage: {
      critical_stage_events: criticalEvents,
      complete_stage_input_events: exactNonnegative(
        coverage.events_with_complete_stage_inputs, "complete stage input events",
      ),
      identity_groups: groups.length,
      group_event_sum: groupEventSum,
      interpretation_event_sum: interpretationEventSum,
      interpretation_compatibility_counts: interpretationCounts,
      compatibility_count_formula_authority: false,
    },
    retained_evidence_sufficiency: {
      multiple_event_identity_groups: multipleEvents,
      multiple_critical_damage_value_groups: multipleCriticalValues,
      all_stage_inputs_same_wire_as_damage: sameWireInputs,
      zero_age_stage_inputs: zeroAgeInputs,
      exact_surface_rows_with_selected_coefficients: selectedSurfaceCoefficients,
      aggregate_group_pair_candidates: aggregatePairCandidates,
      explicit_controlled_pair_records_present: explicitPairRecordsPresent,
      explicit_controlled_pair_records: explicitPairRecordsPresent ? explicitPairs.length : 0,
    },
    controlled_pair_contract: {
      required_equal_fields: [
        "deployment_id", "game_build", "protocol_pack_digest", "session_id", "run_ordinal",
        "source_entity_uuid", "target_entity_uuid", "ability_id", "hit_event_id",
        "damage_source", "damage_type", "type_flags", "reported_critical", "owner_level",
        "owner_stage", "normal_hit", "property", "passive_uuid", "rainbow", "damage_mode",
        "skill_effect_uuid", "skill_effect_group_index", "skill_effect_component_index",
        "skill_effect_component_count", "attack_preimage", "mitigation_preimage",
        "lucky_damage_raw", "all_noncritical_damage_stage_inputs",
      ],
      required_changed_fields: ["critical_damage_raw"],
      event_time_local_wire_snapshots_required: true,
      exact_surface_row_and_owner_stage_selection_required: true,
      both_candidate_integer_residuals_required: true,
      operation_order_and_rounding_enumerated: ["floor", "nearest-half-up"],
      remote_player_cast_packet_required: false,
      current_character_snapshot_substitution_allowed: false,
    },
    controlled_pair_eligibility: {
      eligible_controlled_pairs: eligiblePairs,
      additive_only_exact_residual_pairs: 0,
      direct_only_exact_residual_pairs: 0,
      both_exact_same_result_pairs: 0,
      both_exact_divergent_result_pairs: 0,
      neither_exact_pairs: 0,
      authoritative_interpretation: null,
      formula_authority: false,
      blocker: eligiblePairs === 0
        ? "no-same-build-local-event-time-controlled-pairs-retained"
        : "explicit-pair-residual-adjudication-not-implemented-for-this-schema",
    },
    required_next_evidence: [
      "retain per-event locally observed stage inputs rather than compatibility aggregates",
      "capture at least two otherwise-identical critical damage events with different critical_damage_raw",
      "bind each pair to one exact current-build damage surface row and owner-stage selection",
      "evaluate additive and direct interpretations with exact integer order and rounding residuals",
    ],
    runtime_decision: {
      provider_rdps_credit_allowed: false,
      runtime_catalog_promotion_allowed: false,
      ui_rdps_display_allowed: false,
      ordinary_damage_totals_unchanged: true,
    },
  };

  return analyzePerEventCohort({
    cohort,
    coverage,
    rows,
    criticalEvents,
    interpretationEventSum,
    interpretationCounts,
    selectedSurface,
  });
}

function isEligibleExplicitPair(pair) {
  return pair?.same_build === true && pair?.local_event_time_inputs === true &&
    pair?.remote_player_cast_packet_required === false && pair?.only_critical_damage_raw_changed === true &&
    pair?.exact_surface_and_owner_stage === true && pair?.integer_residuals_complete === true;
}

function analyzePerEventCohort({
  cohort,
  coverage,
  rows,
  criticalEvents,
  interpretationEventSum,
  interpretationCounts,
}) {
  const records = coverage.critical_factor_event_records;
  if (!Array.isArray(records) || records.length !== criticalEvents ||
    records.some((record) => !String(record?.protocol_pack_digest ?? "") ||
      !String(record?.session_id ?? "") || record?.formula_authority !== false ||
      !Array.isArray(record?.damage_surface_candidates) ||
      !Array.isArray(record?.candidate_arithmetic))) {
    throw new Error("Inspiration schema-18 per-event critical-factor records are incomplete");
  }

  const pairGroups = new Map();
  for (const record of records) {
    const key = stableStringify(controlledPairIdentity(record));
    const group = pairGroups.get(key) ?? { records: [], criticalValues: new Set() };
    group.records.push(record);
    if (Number.isSafeInteger(Number(record?.critical_damage?.value))) {
      group.criticalValues.add(Number(record.critical_damage.value));
    }
    pairGroups.set(key, group);
  }
  const groups = [...pairGroups.values()];
  const groupMetric = (predicate) => {
    const selected = groups.filter(predicate);
    return {
      groups: selected.length,
      events: selected.reduce((sum, group) => sum + group.records.length, 0),
    };
  };
  const exactSurface = (record) => {
    const candidates = record?.damage_surface_candidates;
    return record?.damage_surface_resolution === "exactly_one_exact_build_surface_row" &&
      Array.isArray(candidates) && candidates.length === 1 &&
      candidates[0]?.selected_pve_damage_ratio !== null &&
      candidates[0]?.selected_pve_damage_ratio !== undefined &&
      candidates[0]?.selected_pve_fixed_parameter !== null &&
      candidates[0]?.selected_pve_fixed_parameter !== undefined;
  };
  const ownerStageAuthoritative = (record) => exactSurface(record) &&
    record.damage_surface_candidates[0]?.owner_stage_selection_authority === true;
  const requiredInputsSameWire = (record) =>
    record?.critical_damage?.same_wire_as_damage === true &&
    (record.path !== "combined_lucky_occurrence_and_critical_bonus" ||
      record?.lucky_damage?.same_wire_as_damage === true);
  const requiredInputsZeroAge = (record) =>
    Number(record?.critical_damage?.age_sequences) === 0 &&
    Number(record?.critical_damage?.age_micros) === 0 &&
    (record.path !== "combined_lucky_occurrence_and_critical_bonus" ||
      (Number(record?.lucky_damage?.age_sequences) === 0 &&
        Number(record?.lucky_damage?.age_micros) === 0));

  let derivedCandidatePairs = 0;
  let eligiblePairs = 0;
  const candidateExamples = [];
  for (const group of groups) {
    for (let leftIndex = 0; leftIndex < group.records.length; leftIndex += 1) {
      for (let rightIndex = leftIndex + 1; rightIndex < group.records.length; rightIndex += 1) {
        const left = group.records[leftIndex];
        const right = group.records[rightIndex];
        const leftCritical = Number(left?.critical_damage?.value);
        const rightCritical = Number(right?.critical_damage?.value);
        if (!Number.isSafeInteger(leftCritical) || !Number.isSafeInteger(rightCritical) ||
          leftCritical === rightCritical) continue;
        derivedCandidatePairs += 1;
        const eligible = pairHasCompleteAuthority(left, right, exactSurface, ownerStageAuthoritative);
        if (eligible) eligiblePairs += 1;
        if (candidateExamples.length < 32) {
          candidateExamples.push({
            session_id: left.session_id,
            run_ordinal: left.run_ordinal,
            source_entity_uuid: left.source_entity_uuid,
            target_entity_uuid: left.target_entity_uuid,
            ability_id: left.ability_id,
            hit_event_id: left.hit_event_id,
            left_damage_sequence: left.damage_sequence,
            right_damage_sequence: right.damage_sequence,
            left_observed_damage: left.observed_damage,
            right_observed_damage: right.observed_damage,
            left_critical_damage_raw: leftCritical,
            right_critical_damage_raw: rightCritical,
            local_event_time_inputs: left.event_time_local_state_authority === true &&
              right.event_time_local_state_authority === true,
            attack_preimages_complete: left.attack_preimage_complete === true &&
              right.attack_preimage_complete === true,
            mitigation_preimages_complete: left.mitigation_preimage_complete === true &&
              right.mitigation_preimage_complete === true,
            exact_surface_and_owner_stage: ownerStageAuthoritative(left) &&
              ownerStageAuthoritative(right),
            integer_candidate_arithmetic_present: left.candidate_arithmetic.length > 0 &&
              right.candidate_arithmetic.length > 0,
            eligible,
            formula_authority: false,
          });
        }
      }
    }
  }

  const multipleEvents = groupMetric((group) => group.records.length > 1);
  const multipleCriticalValues = groupMetric((group) => group.criticalValues.size > 1);
  const sameWireInputs = groupMetric((group) => group.records.every(requiredInputsSameWire));
  const zeroAgeInputs = groupMetric((group) => group.records.every(requiredInputsZeroAge));
  const selectedSurfaceCoefficients = groupMetric(
    (group) => group.records.every(exactSurface),
  );
  const localEventTimeAuthority = groupMetric(
    (group) => group.records.every((record) => record.event_time_local_state_authority === true),
  );
  const completeAttackMitigationPreimages = groupMetric((group) => group.records.every(
    (record) => record.attack_preimage_complete === true &&
      record.mitigation_preimage_complete === true,
  ));
  const exactSurfaceOwnerStageAuthority = groupMetric(
    (group) => group.records.every(ownerStageAuthoritative),
  );

  return {
    observed_coverage: {
      critical_stage_events: criticalEvents,
      complete_stage_input_events: exactNonnegative(
        coverage.events_with_complete_stage_inputs, "complete stage input events",
      ),
      identity_groups: groups.length,
      group_event_sum: records.length,
      interpretation_event_sum: interpretationEventSum,
      interpretation_compatibility_counts: interpretationCounts,
      compatibility_count_formula_authority: false,
      per_event_records_retained: records.length,
    },
    retained_evidence_sufficiency: {
      multiple_event_identity_groups: multipleEvents,
      multiple_critical_damage_value_groups: multipleCriticalValues,
      all_stage_inputs_same_wire_as_damage: sameWireInputs,
      zero_age_stage_inputs: zeroAgeInputs,
      exact_surface_rows_with_selected_coefficients: selectedSurfaceCoefficients,
      local_event_time_state_authority_groups: localEventTimeAuthority,
      complete_attack_mitigation_preimage_groups: completeAttackMitigationPreimages,
      exact_surface_owner_stage_authority_groups: exactSurfaceOwnerStageAuthority,
      aggregate_group_pair_candidates: {
        groups: multipleCriticalValues.groups,
        events: multipleCriticalValues.events,
        pairs: derivedCandidatePairs,
      },
      explicit_controlled_pair_records_present: true,
      explicit_controlled_pair_records: derivedCandidatePairs,
      candidate_examples: candidateExamples,
    },
    controlled_pair_contract: {
      required_equal_fields: [
        "deployment_id", "game_build", "protocol_pack_digest", "session_id", "run_ordinal",
        "source_entity_uuid", "target_entity_uuid", "ability_id", "hit_event_id",
        "damage_source", "damage_type", "type_flags", "reported_critical", "owner_level",
        "owner_stage", "normal_hit", "property", "passive_uuid", "rainbow", "damage_mode",
        "skill_effect_uuid", "skill_effect_group_index", "skill_effect_component_index",
        "skill_effect_component_count", "provider_window", "provider_magnitude",
        "damage_surface", "lucky_damage_raw",
        "attack_preimage", "mitigation_preimage", "all_noncritical_damage_stage_inputs",
      ],
      required_changed_fields: ["critical_damage_raw"],
      event_time_local_wire_snapshots_required: true,
      exact_surface_row_and_owner_stage_selection_required: true,
      both_candidate_integer_residuals_required: true,
      operation_order_and_rounding_enumerated: ["floor", "nearest-half-up"],
      remote_player_cast_packet_required: false,
      current_character_snapshot_substitution_allowed: false,
    },
    controlled_pair_eligibility: {
      derived_candidate_pairs: derivedCandidatePairs,
      eligible_controlled_pairs: eligiblePairs,
      additive_only_exact_residual_pairs: 0,
      direct_only_exact_residual_pairs: 0,
      both_exact_same_result_pairs: 0,
      both_exact_divergent_result_pairs: 0,
      neither_exact_pairs: 0,
      authoritative_interpretation: null,
      formula_authority: false,
      blocker: eligiblePairs === 0
        ? (derivedCandidatePairs === 0
          ? "no-same-build-local-event-time-controlled-pairs-retained"
          : "candidate-pairs-retained-but-local-event-time-or-complete-preimage-authority-missing")
        : "explicit-pair-residual-adjudication-not-implemented-for-this-schema",
    },
    required_next_evidence: [
      "identify the locally controlled player entity in canonical events without name inference",
      "retain complete attack and mitigation preimages at each candidate damage event",
      "prove exact owner-stage array selection for the bound current-build damage surface row",
      "evaluate retained eligible pairs under both interpretations and exact integer rounding",
    ],
    runtime_decision: {
      provider_rdps_credit_allowed: false,
      runtime_catalog_promotion_allowed: false,
      ui_rdps_display_allowed: false,
      ordinary_damage_totals_unchanged: true,
    },
  };
}

function controlledPairIdentity(record) {
  const equalFields = [
    "protocol_pack_digest", "session_id", "run_ordinal", "source_entity_uuid",
    "target_entity_uuid", "ability_id", "hit_event_id", "damage_source", "damage_type",
    "type_flags", "reported_critical", "owner_level", "owner_stage", "normal_hit",
    "property", "passive_uuid", "rainbow", "damage_mode", "skill_effect_uuid",
    "skill_effect_group_index", "skill_effect_component_index", "skill_effect_component_count",
    "path", "provider_entity_uuid", "provider_instance_id",
    "provider_level", "provider_origin_source_type_id", "provider_origin_source_config_id",
    "provider_critical_raw_delta", "provider_lucky_raw_delta", "damage_surface_resolution",
    "damage_surface_candidates",
  ];
  return {
    ...Object.fromEntries(equalFields.map((field) => [field, record?.[field] ?? null])),
    lucky_damage_raw: record?.lucky_damage?.value ?? null,
  };
}

function pairHasCompleteAuthority(left, right, exactSurface, ownerStageAuthoritative) {
  return left?.event_time_local_state_authority === true &&
    right?.event_time_local_state_authority === true &&
    left?.attack_preimage_complete === true && right?.attack_preimage_complete === true &&
    left?.mitigation_preimage_complete === true && right?.mitigation_preimage_complete === true &&
    exactSurface(left) && exactSurface(right) &&
    ownerStageAuthoritative(left) && ownerStageAuthoritative(right) &&
    Array.isArray(left?.candidate_arithmetic) && left.candidate_arithmetic.length > 0 &&
    Array.isArray(right?.candidate_arithmetic) && right.candidate_arithmetic.length > 0;
}

function validateReport(report) {
  if (Number(report?.schema_version) !== SCHEMA_VERSION ||
    report?.generated_by !== "tools/bpsr-critical-factor-controlled-pair-discriminant.mjs" ||
    report?.proof_state !==
      "same-build-local-controlled-pair-discriminant-open-per-event-candidates-audited" ||
    !/^\d+$/.test(String(report?.game_build)) ||
    report?.content_sha256 !== contentHash(report) ||
    report?.policy?.remote_player_cast_packets_required !== false ||
    report?.policy?.remote_player_cast_packets_treated_as_zero !== false ||
    report?.policy?.remote_player_cast_packets_synthesized !== false ||
    report?.policy?.aggregate_compatibility_counts_are_formula_authority !== false ||
    report?.policy?.exclusive_candidate_fit_counts_are_votes !== false ||
    report?.policy?.provider_rdps_credit_allowed !== false ||
    report?.policy?.runtime_promotion_allowed !== false ||
    report?.policy?.ui_rdps_display_allowed !== false ||
    report?.bounded_processing?.maximum_input_bytes !== MAXIMUM_COHORT_BYTES ||
    report?.bounded_processing?.whole_rlog_cohort_deserialized !== false ||
    report?.controlled_pair_contract?.remote_player_cast_packet_required !== false ||
    report?.controlled_pair_contract?.current_character_snapshot_substitution_allowed !== false ||
    report?.controlled_pair_eligibility?.authoritative_interpretation !== null ||
    report?.controlled_pair_eligibility?.formula_authority !== false ||
    report?.runtime_decision?.provider_rdps_credit_allowed !== false ||
    report?.runtime_decision?.runtime_catalog_promotion_allowed !== false ||
    report?.runtime_decision?.ui_rdps_display_allowed !== false ||
    report?.runtime_decision?.ordinary_damage_totals_unchanged !== true ||
    !isDescriptor(report?.input)) {
    throw new Error("Controlled-pair discriminant report is invalid or unsafe");
  }
}

function selectAnalysis(report) {
  return {
    observed_coverage: report.observed_coverage,
    retained_evidence_sufficiency: report.retained_evidence_sufficiency,
    controlled_pair_contract: report.controlled_pair_contract,
    controlled_pair_eligibility: report.controlled_pair_eligibility,
    required_next_evidence: report.required_next_evidence,
    runtime_decision: report.runtime_decision,
  };
}

function selfTest() {
  const cohort = {
    schema_version: 18,
    generated_by: "rlogs-bpsr-inspiration-proc-attribution-proof",
    game_build: "1",
    effect_id: 2_202_041,
    policy: {
      remote_player_packets_required: false,
      remote_player_packets_treated_as_zero: false,
      remote_player_packets_synthesized: false,
      critical_damage_raw_interpretation_authority: false,
      formula_authority: false,
      provider_rdps_credit_authorized: false,
    },
    integer_stage_counterfactual_coverage: {
      candidate_family: EXPECTED_CANDIDATE_FAMILY,
      critical_stage_events: 1,
      events_with_complete_stage_inputs: 1,
      critical_factor_interpretation_breakdown_authority: false,
      candidate_family_authority: false,
      counterfactual_authority: false,
      critical_factor_event_records: [{
        protocol_pack_digest: "sha256:test",
        session_id: "session",
        run_ordinal: 1,
        damage_sequence: 2,
        source_entity_uuid: 1,
        target_entity_uuid: 2,
        ability_id: 3,
        hit_event_id: 4,
        observed_damage: 100,
        path: "critical_proc_bonus",
        critical_damage: {
          value: 20_128,
          age_sequences: 1,
          age_micros: 1,
          same_wire_as_damage: false,
        },
        provider_entity_uuid: 9,
        provider_instance_id: 10,
        provider_level: 1,
        provider_origin_source_type_id: 1,
        provider_origin_source_config_id: 2_202_040,
        provider_critical_raw_delta: 300,
        provider_lucky_raw_delta: 300,
        damage_surface_resolution: "exactly_one_exact_build_surface_row",
        damage_surface_candidates: [{
          selected_pve_damage_ratio: 10_000,
          selected_pve_fixed_parameter: 0,
          owner_stage_selection_authority: false,
        }],
        candidate_arithmetic: [{ critical_factor_interpretation: "additive_bonus" }],
        event_time_local_state_authority: false,
        attack_preimage_complete: false,
        mitigation_preimage_complete: false,
        formula_authority: false,
      }],
      critical_factor_interpretation_breakdown: [{
        compatibility: "both", counterfactual_relation: "divergent_exact",
        events: 1, formula_authority: false,
      }],
      damage_surface_join: { groups: [{
        events: 1,
        critical_damage_raw_values: [20_128],
        oldest_stage_input_age_sequences: 1,
        oldest_stage_input_age_micros: 1,
        stage_inputs_all_same_wire_as_damage: false,
        damage_surface_resolution: "exactly_one_exact_build_surface_row",
        damage_surface_candidates: [{
          selected_pve_damage_ratio: [10_000], selected_pve_fixed_parameter: [0],
        }],
      }] },
    },
  };
  const analysis = analyzeCohort(cohort, "1");
  if (analysis.controlled_pair_eligibility.eligible_controlled_pairs !== 0 ||
    analysis.retained_evidence_sufficiency.exact_surface_rows_with_selected_coefficients.events !== 1) {
    throw new Error("Controlled-pair discriminant self-test produced unsafe eligibility");
  }
  console.log("Critical-factor controlled-pair discriminant self-test passed.");
}

function readBoundedCohort(path) {
  if (!existsSync(path) || !statSync(path).isFile()) throw new Error(`Missing cohort ${path}`);
  const bytes = statSync(path).size;
  if (bytes <= 0 || bytes > MAXIMUM_COHORT_BYTES) {
    throw new Error(`Cohort size ${bytes} exceeds bounded input limit ${MAXIMUM_COHORT_BYTES}`);
  }
  return readJson(path, "Inspiration cohort");
}

function exactNonnegative(value, label) {
  const number = Number(value);
  if (!Number.isSafeInteger(number) || number < 0) throw new Error(`${label} is invalid`);
  return number;
}

function fileDescriptor(path) {
  return { path: path.replaceAll("\\", "/"), bytes: statSync(path).size, sha256: hashFile(path) };
}
function isDescriptor(value) {
  return typeof value?.path === "string" && value.path.length > 0 &&
    Number.isSafeInteger(Number(value.bytes)) && Number(value.bytes) > 0 &&
    /^[0-9a-f]{64}$/.test(String(value.sha256));
}
function readJson(path, label) {
  try { return JSON.parse(readFileSync(path, "utf8")); }
  catch (error) { throw new Error(`${label} is not valid JSON: ${error.message}`); }
}
function contentHash(value) {
  const clone = structuredClone(value); delete clone.content_sha256;
  return createHash("sha256").update(stableStringify(clone)).digest("hex");
}
function hashFile(path) { return createHash("sha256").update(readFileSync(path)).digest("hex"); }
function stableStringify(value) {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) =>
      `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}
function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index]; const value = args[index + 1];
    if (!key?.startsWith("--") || value === undefined) throw new Error(`Invalid argument ${key}`);
    parsed[key.slice(2)] = value;
  }
  return parsed;
}
function required(args, key) { if (!args[key]) throw new Error(`Missing --${key}`); return args[key]; }
function usage(code) {
  console.log(
    "Usage: node tools/bpsr-critical-factor-controlled-pair-discriminant.mjs " +
      "build --build <id> --cohort <json> --output <json> | verify --input <json> | self-test",
  );
  process.exit(code);
}
