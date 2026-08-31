#!/usr/bin/env node

import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);
const SCHEMA_VERSION = 9;
const PHYSICAL_ATTACK_ATTRIBUTE_ID = 11_330;
const MAGICAL_ATTACK_ATTRIBUTE_ID = 11_340;
const MASTERY_ATTRIBUTE_ID = 11_940;
const FIXED_POINT_DENOMINATOR = 10_000n;

if (command === "analyze") analyzeCommand(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function analyzeCommand(parsed) {
  const input = path.resolve(required(parsed, "counterfactual-proof"));
  const damageAttrTable = path.resolve(required(parsed, "damage-attr-table"));
  const damageFormulaSurface = path.resolve(required(parsed, "damage-formula-surface"));
  const providerOwnershipProofPath = parsed["provider-ownership-proof"] === undefined
    ? null
    : path.resolve(parsed["provider-ownership-proof"]);
  const providerFormulaInputCoveragePath = parsed["provider-formula-input-coverage"] === undefined
    ? null
    : path.resolve(parsed["provider-formula-input-coverage"]);
  if ((providerOwnershipProofPath === null) !== (providerFormulaInputCoveragePath === null)) {
    throw new Error("--provider-ownership-proof and --provider-formula-input-coverage must be supplied together");
  }
  const spHealOperatorProofPath = parsed["spheal-operator-proof"] === undefined
    ? null
    : path.resolve(parsed["spheal-operator-proof"]);
  const nearControlledRollupPath = parsed["near-controlled-rollup"] === undefined
    ? null
    : path.resolve(parsed["near-controlled-rollup"]);
  const output = path.resolve(required(parsed, "output"));
  const effectId = positiveInteger(required(parsed, "effect"), "effect");
  const locus = parsed.locus ?? "target";
  if (!['source', 'target'].includes(locus)) throw new Error("--locus must be source or target");
  const maximumAbsoluteBasisPoints = parsed["maximum-absolute-basis-points"] === undefined
    ? 10_000
    : positiveInteger(parsed["maximum-absolute-basis-points"], "maximum-absolute-basis-points");
  if (maximumAbsoluteBasisPoints > 100_000) {
    throw new Error("--maximum-absolute-basis-points must not exceed 100000");
  }

  const frontier = readJson(input, "status-effect counterfactual proof");
  const decodedDamageAttrs = readJson(damageAttrTable, "current-build DamageAttrTable");
  const formulaSurface = readJson(damageFormulaSurface, "exact-build damage formula surface");
  const providerFormulaInputCoverage = providerOwnershipProofPath === null
    ? null
    : analyzeProviderFormulaInputCoverage(
      readJson(providerFormulaInputCoveragePath, "provider formula-input coverage"),
      readJson(providerOwnershipProofPath, "provider ownership proof"),
      {
        effectId,
        gameBuild: String(frontier.game_build ?? ""),
        coverageInput: fileDescriptor(providerFormulaInputCoveragePath),
        ownershipInput: fileDescriptor(providerOwnershipProofPath),
      },
    );
  const spHealOperatorEvidence = spHealOperatorProofPath === null
    ? null
    : analyzeSpHealOperatorEvidence(
      readJson(spHealOperatorProofPath, "SpHeal operator proof"),
      fileDescriptor(spHealOperatorProofPath),
      effectId,
      String(frontier.game_build ?? ""),
    );
  const nearControlledExhaustion = nearControlledRollupPath === null
    ? null
    : analyzeNearControlledRollup(
      readJson(nearControlledRollupPath, "near-controlled exhaustion rollup"),
      fileDescriptor(nearControlledRollupPath),
      effectId,
      String(frontier.game_build ?? ""),
    );
  const report = analyzeFrontier(frontier, {
    effectId,
    locus,
    maximumAbsoluteBasisPoints,
    input: fileDescriptor(input),
    providerFormulaInputCoverage,
    spHealOperatorEvidence,
    nearControlledExhaustion,
    staticFormulaEvidence: analyzeStaticFormulaCandidates(
      decodedDamageAttrs,
      formulaSurface,
      effectId,
      fileDescriptor(damageAttrTable),
      fileDescriptor(damageFormulaSurface),
      String(frontier.game_build ?? ""),
    ),
  });
  report.content_sha256 = contentHash(report);
  mkdirSync(path.dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  console.log(`wrote ${output}`);
}

function analyzeFrontier(frontier, context) {
  if (![3, 4, 5, 6].includes(frontier.schema_version) ||
    frontier.generated_by !== "rlogs-bpsr-status-effect-counterfactual-proof") {
    throw new Error("Unsupported status-effect counterfactual proof schema or generator");
  }
  const gameBuild = String(frontier.game_build ?? "");
  if (!/^\d+$/.test(gameBuild)) throw new Error("Counterfactual proof has no exact numeric build");
  if (frontier.policy?.formula_authority !== false ||
    frontier.policy?.runtime_authority !== false ||
    frontier.policy?.unresolved_evidence_is_hidden !== false) {
    throw new Error("Counterfactual proof authority policy is unsafe");
  }
  if (Number(frontier.schema_version) >= 5 &&
    frontier.policy?.candidate_projection_authority !== false) {
    throw new Error("Counterfactual proof candidate projection policy is unsafe");
  }
  if (Number(frontier.schema_version) >= 6 &&
    frontier.policy?.near_controlled_diagnostic_authority !== false) {
    throw new Error("Counterfactual proof near-controlled diagnostic policy is unsafe");
  }
  const effect = (frontier.effects ?? []).find((entry) =>
    Number(entry.effect_id) === context.effectId && String(entry.locus) === context.locus
  );
  if (!effect) throw new Error(`No ${context.locus} locus for exact effect ${context.effectId}`);
  const examples = effect.exact_recorded_inputs?.divergent_examples ?? [];
  if (!Array.isArray(examples) || examples.length === 0) {
    throw new Error(`Exact effect ${context.effectId} ${context.locus} has no divergent controlled example`);
  }

  const observations = examples.map((example, index) =>
    normalizeObservation(example, index, context)
  );
  if (context.providerFormulaInputCoverage) {
    const coveredRlogs = new Set(
      context.providerFormulaInputCoverage.capture_inputs.map((input) => input.rlog),
    );
    const provenProviders = new Set(
      context.providerFormulaInputCoverage.proven_provider_entity_uuids,
    );
    for (const observation of observations) {
      if (!coveredRlogs.has(observation.rlog)) {
        throw new Error(`Controlled observation RLOG ${observation.rlog} is absent from provider formula-input coverage`);
      }
      if (observation.providerContext.provider_entity_uuid !== undefined &&
        !provenProviders.has(observation.providerContext.provider_entity_uuid)) {
        throw new Error("Controlled observation provider is absent from exact provider ownership coverage");
      }
    }
  }
  const providerContextSummary = summarizeProviderContexts(
    observations,
    frontier.schema_version,
    context.providerFormulaInputCoverage ?? null,
    context.spHealOperatorEvidence ?? null,
  );
  const roundingModes = ["floor", "round_half_up", "ceil"];
  const multiplicativeCandidates = Object.fromEntries(roundingModes.map((mode) => [
    mode,
    intersectCandidateSets(observations.map((observation) =>
      compatibleBasisPoints(observation, mode, context.maximumAbsoluteBasisPoints)
    )),
  ]));
  const additiveDeltas = [...new Set(observations.map((observation) =>
    (observation.present - observation.absent).toString()
  ))].sort(compareIntegerText);
  const staticFormulaEvidence = attachStaticCompatibility(
    context.staticFormulaEvidence,
    observations,
    roundingModes,
  );
  const eventLocalCounterfactualConservation = buildEventLocalCounterfactualConservation(
    observations,
    context.providerFormulaInputCoverage,
    Number(frontier.schema_version),
  );

  return {
    schema_version: SCHEMA_VERSION,
    generated_by: "bpsr-rdps-integer-transform-constraints",
    game_build: gameBuild,
    effect_id: context.effectId,
    locus: context.locus,
    policy: {
      exact_numeric_effect_id_is_authoritative: true,
      exact_input_build_is_authoritative: true,
      localized_names_are_evidence_only: true,
      controlled_examples_are_never_cross_session_paired: true,
      analysis_scope: "final_observed_damage_integer_transform_constraints_only",
      candidate_models_are_compatibility_constraints_not_formula_proof: true,
      formula_stage_is_proven: false,
      operation_order_is_proven: false,
      stacking_is_proven: false,
      runtime_integer_rounding_is_proven: false,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
      unresolved_evidence_is_preserved: true,
      decoded_formula_inputs_are_candidates_not_status_transform_bindings: true,
      third_party_provider_attributes_are_controlled_when_embedded: true,
      exact_provider_formula_input_coverage_is_build_locked_when_supplied: true,
      exact_spheal_operator_evidence_is_fail_closed_when_supplied: true,
      event_local_counterfactual_arithmetic_never_grants_causal_formula_or_runtime_authority: true,
      near_controlled_target_diagnostics_never_grant_formula_or_runtime_authority: true,
    },
    input: context.input,
    observation_summary: {
      exact_divergent_controlled_examples: observations.length,
      distinct_sessions: new Set(observations.map((entry) => entry.session_id)).size,
      maximum_absolute_basis_points_tested: context.maximumAbsoluteBasisPoints,
    },
    observations: observations.map((observation) => ({
      session_id: observation.session_id,
      rlog: observation.rlog,
      absent_sequences: observation.absentSequences,
      present_sequences: observation.presentSequences,
      absent_damage: observation.absent.toString(),
      present_damage: observation.present.toString(),
      observed_delta: (observation.present - observation.absent).toString(),
      observed_ratio_reduced: reducedRatio(observation.present, observation.absent),
      provider_relationship: observation.providerRelationship,
      provider_formula_context: observation.providerContext,
      compatible_post_output_delta_basis_points: Object.fromEntries(roundingModes.map((mode) => [
        mode,
        compatibleBasisPoints(observation, mode, context.maximumAbsoluteBasisPoints),
      ])),
    })),
    compatible_model_intersection: {
      post_output_multiplicative_delta_basis_points: multiplicativeCandidates,
      fixed_additive_integer_delta: additiveDeltas.length === 1 ? additiveDeltas[0] : null,
    },
    static_formula_input_candidates: staticFormulaEvidence,
    provider_formula_context_summary: providerContextSummary,
    provider_formula_input_coverage: context.providerFormulaInputCoverage ?? null,
    spheal_operator_evidence: context.spHealOperatorEvidence ?? null,
    near_controlled_exhaustion: context.nearControlledExhaustion ?? null,
    event_local_counterfactual_conservation: eventLocalCounterfactualConservation,
    interpretation: {
      compatible_models_are_not_unique: countCompatibleModels(multiplicativeCandidates, additiveDeltas) > 1,
      exact_transform_proven: false,
      exact_static_status_transform_binding_proven: false,
      provider_formula_base_input_proven: false,
      independent_divergent_baseline_replication_proven:
        context.nearControlledExhaustion
          ?.interpretation?.independent_divergent_baseline_replication_proven ?? false,
      near_controlled_diagnostic_formula_authority: false,
      next_required_evidence: [
        "independent matching-build controlled-delta replication at a different baseline damage",
        "exact static or packet magnitude binding for this effect and level",
        "operation-stage and stacking-order isolation",
        "integer-rounding discrimination across boundary-sensitive examples",
        "canonical counterfactual replay conservation",
      ],
    },
  };
}

function normalizeObservation(example, index, context) {
  const absent = exactPositiveInteger(example.absent_outcome?.amount, `example ${index} absent amount`);
  const present = exactPositiveInteger(example.present_outcome?.amount, `example ${index} present amount`);
  if (absent === present) throw new Error(`Example ${index} is not divergent`);
  const sessionId = String(example.session_id ?? "");
  if (!sessionId) throw new Error(`Example ${index} has no session identity`);
  const absentSequences = exactSequenceList(example.absent_sequences, `example ${index} absent sequences`);
  const presentSequences = exactSequenceList(example.present_sequences, `example ${index} present sequences`);
  const status = {
    effect_id: exactPositiveNumber(example.status?.effect_id, `example ${index} status effect id`),
    source_entity_uuid: exactPositiveNumber(
      example.status?.source_entity_uuid,
      `example ${index} status provider entity UUID`,
    ),
    stacks: exactPositiveNumber(example.status?.stacks, `example ${index} status stacks`),
    level: exactPositiveNumber(example.status?.level, `example ${index} status level`),
    origin_source_type_id: nullablePositiveNumber(
      example.status?.origin_source_type_id,
      `example ${index} status origin source type id`,
    ),
    origin_source_config_id: nullablePositiveNumber(
      example.status?.origin_source_config_id,
      `example ${index} status origin source config id`,
    ),
  };
  if (status.effect_id !== context.effectId || String(example.locus ?? context.locus) !== context.locus) {
    throw new Error(`Example ${index} status identity does not match the selected effect locus`);
  }
  return {
    absent,
    present,
    session_id: sessionId,
    rlog: String(example.rlog ?? ""),
    runOrdinal: exactPositiveNumber(example.run_ordinal, `example ${index} run ordinal`),
    sourceEntityUuid: exactPositiveNumber(
      example.source_entity_uuid,
      `example ${index} damage source entity UUID`,
    ),
    targetEntityUuid: exactPositiveNumber(
      example.target_entity_uuid,
      `example ${index} damage target entity UUID`,
    ),
    absentSequences,
    presentSequences,
    providerRelationship: String(example.provider_relationship ?? "unresolved"),
    abilityId: exactPositiveNumber(example.ability_id, `example ${index} ability id`),
    status,
    providerContext: normalizeProviderContext(example, index, absentSequences, presentSequences),
  };
}

function normalizeProviderContext(example, index, absentSequences, presentSequences) {
  const present = example.present_formula_context;
  const absent = example.absent_formula_context;
  if (present === undefined && absent === undefined) {
    return {
      embedded: false,
      exact_provider_attribute_state_controlled: false,
      provider_formula_base_input_proven: false,
    };
  }
  if (!present || !absent) {
    throw new Error(`Example ${index} has only one embedded formula context`);
  }
  if (present.normalized_packet_input_sha256 !== absent.normalized_packet_input_sha256 ||
    JSON.stringify(present.normalized_packet_inputs) !== JSON.stringify(absent.normalized_packet_inputs) ||
    JSON.stringify(present.source_attributes) !== JSON.stringify(absent.source_attributes) ||
    JSON.stringify(present.target_attributes) !== JSON.stringify(absent.target_attributes) ||
    JSON.stringify(present.source_statuses) !== JSON.stringify(absent.source_statuses)) {
    throw new Error(`Example ${index} embedded formula inputs are not controlled`);
  }
  if (!/^sha256:[0-9a-f]{64}$/.test(String(present.normalized_packet_input_sha256 ?? "")) ||
    !presentSequences.includes(Number(present.representative_sequence)) ||
    !absentSequences.includes(Number(absent.representative_sequence))) {
    throw new Error(`Example ${index} embedded formula context identity is incomplete`);
  }
  const expectedAbsentTargetStatuses = (present.target_statuses ?? []).filter((status) =>
    !sameStatus(status, example.status)
  );
  if (JSON.stringify(expectedAbsentTargetStatuses) !== JSON.stringify(absent.target_statuses ?? [])) {
    throw new Error(`Example ${index} target statuses do not differ by exactly the selected status`);
  }
  if (JSON.stringify(present.status_provider_attributes) !==
    JSON.stringify(absent.status_provider_attributes)) {
    throw new Error(`Example ${index} status-provider attribute states are not controlled`);
  }
  const providerEntityUuid = exactPositiveNumber(
    example.status?.source_entity_uuid,
    `example ${index} provider entity UUID`,
  );
  const provider = (present.status_provider_attributes ?? []).find((entry) =>
    Number(entry.provider_entity_uuid) === providerEntityUuid
  );
  if (!provider) throw new Error(`Example ${index} has no embedded selected-provider state`);
  const attributes = exactAttributeState(
    provider.attributes,
    `example ${index} selected-provider attributes`,
  );
  const physicalAttack = attributes.find((entry) =>
    entry.attribute_id === PHYSICAL_ATTACK_ATTRIBUTE_ID
  )?.value ?? null;
  return {
    embedded: true,
    provider_entity_uuid: providerEntityUuid,
    attribute_state_observed: provider.attribute_state_observed === true,
    attribute_state_id: provider.attribute_state_id ?? null,
    exact_provider_attribute_state_controlled: true,
    retained_attributes: attributes,
    normalized_packet_input_sha256: String(present.normalized_packet_input_sha256),
    source_attribute_state_id: present.source_attribute_state_id ?? null,
    target_attribute_state_id: present.target_attribute_state_id ?? null,
    present_representative_sequence: Number(present.representative_sequence),
    absent_representative_sequence: Number(absent.representative_sequence),
    physical_attack_attribute_id: PHYSICAL_ATTACK_ATTRIBUTE_ID,
    physical_attack_value: physicalAttack,
    provider_formula_base_input_proven: false,
  };
}

function buildEventLocalCounterfactualConservation(observations, coverage, frontierSchemaVersion) {
  const providerIdentities = new Map((coverage?.proven_provider_identities ?? []).map((identity) => [
    identity.provider_entity_uuid,
    identity,
  ]));
  const events = observations.map((observation) => {
    const delta = observation.present - observation.absent;
    const providerIdentity = providerIdentities.get(observation.status.source_entity_uuid) ?? null;
    const exactInputsControlled = frontierSchemaVersion >= 5 &&
      observation.providerContext.embedded === true &&
      observation.providerContext.exact_provider_attribute_state_controlled === true;
    return {
      exact_build_locked: true,
      rlog: observation.rlog,
      session_id: observation.session_id,
      run_ordinal: observation.runOrdinal,
      damage_source_entity_uuid: observation.sourceEntityUuid,
      damage_target_entity_uuid: observation.targetEntityUuid,
      ability_id: observation.abilityId,
      status: structuredClone(observation.status),
      provider_relationship: observation.providerRelationship,
      provider_player_identity: providerIdentity === null ? null : structuredClone(providerIdentity),
      normalized_packet_input_sha256:
        observation.providerContext.normalized_packet_input_sha256 ?? null,
      source_attribute_state_id: observation.providerContext.source_attribute_state_id ?? null,
      target_attribute_state_id: observation.providerContext.target_attribute_state_id ?? null,
      absent_sequences: structuredClone(observation.absentSequences),
      present_sequences: structuredClone(observation.presentSequences),
      absent_damage: observation.absent.toString(),
      observed_controlled_delta: delta.toString(),
      present_damage: observation.present.toString(),
      arithmetic_equation: `${observation.absent} + ${delta} = ${observation.present}`,
      arithmetic_conservation_holds: observation.absent + delta === observation.present,
      exact_recorded_inputs_controlled: exactInputsControlled,
      provider_player_identity_proven: providerIdentity !== null,
      causal_provider_contribution_proven: false,
    };
  });
  return {
    scope: "single_exact_controlled_observed_damage_event_pairs",
    summary: {
      event_pairs: events.length,
      arithmetic_conservation_holds_for_every_pair:
        events.every((event) => event.arithmetic_conservation_holds === true),
      exact_recorded_inputs_controlled_for_every_pair:
        events.every((event) => event.exact_recorded_inputs_controlled === true),
      provider_player_identity_proven_for_every_pair:
        events.every((event) => event.provider_player_identity_proven === true),
    },
    events,
    interpretation: {
      event_local_counterfactual_arithmetic_conservation_proven:
        events.length > 0 && events.every((event) =>
          event.arithmetic_conservation_holds === true &&
          event.exact_recorded_inputs_controlled === true &&
          event.provider_player_identity_proven === true
        ),
      observed_controlled_delta_is_not_a_general_formula: true,
      causal_provider_contribution_proven: false,
      exact_transform_proven: false,
      formula_stage_and_operation_order_proven: false,
      runtime_integer_rounding_proven: false,
      canonical_party_replay_conservation_proven: false,
      formula_authority: false,
      runtime_authority: false,
      ui_authority: false,
      provider_rdps_credit_allowed: false,
      next_required_evidence: [
        "independent exact-build replication at a different baseline damage",
        "exact transform binding plus operation stage and stacking order",
        "boundary-sensitive integer-rounding discrimination",
        "canonical party replay conservation over observable protocol events",
      ],
    },
  };
}

function summarizeProviderContexts(observations, frontierSchemaVersion, coverage, spHealEvidence) {
  const contexts = observations.map((observation) => observation.providerContext);
  const embedded = contexts.every((context) => context.embedded === true);
  const providerEntityUuids = [...new Set(contexts
    .filter((context) => context.provider_entity_uuid !== undefined)
    .map((context) => context.provider_entity_uuid))].sort((left, right) => left - right);
  return {
    counterfactual_schema_version: frontierSchemaVersion,
    formula_context_embedded_for_every_example: embedded,
    exact_provider_attribute_state_controlled_for_every_example:
      embedded && contexts.every((context) => context.exact_provider_attribute_state_controlled === true),
    provider_attribute_state_observed_for_every_example:
      embedded && contexts.every((context) => context.attribute_state_observed === true),
    provider_entity_uuids: providerEntityUuids,
    physical_attack_attribute_id: PHYSICAL_ATTACK_ATTRIBUTE_ID,
    provider_physical_attack_observed_for_every_example:
      embedded && contexts.every((context) => context.physical_attack_value !== null),
    matching_capture_provider_formula_input_coverage_supplied: coverage !== null,
    matching_capture_provider_formula_input_coverage_proven:
      coverage?.exact_provider_and_input_identity_match === true,
    matching_capture_count: coverage?.capture_inputs?.length ?? 0,
    proven_effect_provider_count: coverage?.proven_provider_entity_uuids?.length ?? 0,
    matching_capture_physical_attack_observation_count:
      coverage?.audited_candidate_formula_inputs?.physical_attack?.observation_count ?? null,
    matching_capture_physical_attack_absent_for_every_proven_provider:
      coverage?.audited_candidate_formula_inputs?.physical_attack?.observation_count === 0,
    spheal_operator_evidence_supplied: spHealEvidence !== null,
    exact_effect_spheal_output_packet_observed:
      spHealEvidence?.summary?.exact_effect_output_packet_observed ?? null,
    exact_effect_spheal_occurrence_proof_capture_count:
      spHealEvidence?.summary?.exact_effect_occurrence_proof_rlogs ?? null,
    exact_effect_spheal_occurrence_proof_healing_events_scanned:
      spHealEvidence?.summary?.exact_effect_occurrence_proof_healing_events_scanned ?? null,
    exact_effect_spheal_output_absent_in_all_complete_matching_build_capture_inputs:
      spHealEvidence?.interpretation
        ?.exact_effect_output_absent_in_all_complete_matching_build_capture_inputs ?? null,
    spheal_family_wide_single_hp_ratio_proven:
      spHealEvidence?.summary?.spheal_family_wide_single_hp_ratio_proven ?? null,
    exact_effect_spheal_coefficient_to_hp_basis_binding_proven:
      spHealEvidence?.summary?.exact_effect_spheal_coefficient_to_hp_basis_binding_proven ?? null,
    provider_formula_base_input_proven: false,
    missing_or_unproven_inputs: uniqueStrings([
      ...(!embedded ? ["provider-at-event formula context is not embedded"] : []),
      ...(embedded && contexts.some((context) => context.attribute_state_observed !== true)
        ? ["selected provider attribute state is not observed"] : []),
      ...(embedded && contexts.some((context) => context.physical_attack_value === null)
        ? [`provider physical attack attribute ${PHYSICAL_ATTACK_ATTRIBUTE_ID} is not observed`] : []),
      ...(coverage?.audited_candidate_formula_inputs?.physical_attack?.observation_count === 0
        ? [`provider physical attack attribute ${PHYSICAL_ATTACK_ATTRIBUTE_ID} is absent across all exact-build effect-matching captures and proven providers`]
        : []),
      ...(spHealEvidence?.summary?.exact_effect_output_packet_observed === false
        ? [`exact effect SpHeal output row is absent across all ${spHealEvidence.summary.exact_effect_occurrence_proof_rlogs} complete exact-build capture inputs`] : []),
      ...(spHealEvidence?.summary?.damage_script_identity_alone_proves_operator === false
        ? ["SpHeal DamageScript identity does not prove a transferable operator"] : []),
      ...(spHealEvidence?.summary?.exact_effect_spheal_coefficient_to_hp_basis_binding_proven === false
        ? ["exact effect SpHeal coefficient-to-HP-basis binding is unproven"] : []),
      "SpHeal provider input contract is unproven",
      "effect-output coefficient to target-status transform binding is unproven",
    ]),
  };
}

function analyzeNearControlledRollup(document, input, effectId, gameBuild) {
  const summary = document?.summary ?? {};
  const interpretation = document?.interpretation ?? {};
  const countKeys = [
    "matching_capture_runs",
    "samples",
    "exact_controlled_groups",
    "exact_divergent_output_groups",
    "exact_divergent_capture_runs",
    "near_controlled_target_pairs",
    "near_controlled_target_divergent_pairs",
    "near_controlled_target_equal_pairs",
    "equal_output_status_bundle_examples",
  ];
  if (document?.schema_version !== 1 ||
    document?.generated_by !== "bpsr-status-effect-near-controlled-rollup" ||
    String(document?.game_build ?? "") !== gameBuild ||
    Number(document?.effect_id) !== effectId ||
    document?.policy?.exact_numeric_effect_id_is_authoritative !== true ||
    document?.policy?.exact_input_build_is_authoritative !== true ||
    document?.policy?.cross_session_pairing_allowed !== false ||
    document?.policy?.target_current_hp_relaxation_is_diagnostic_only !== true ||
    document?.policy?.every_target_attribute_and_status_co_transition_is_preserved !== true ||
    document?.policy?.near_controlled_pairs_never_grant_formula_or_runtime_authority !== true ||
    document?.policy?.formula_authority !== false ||
    document?.policy?.runtime_authority !== false ||
    document?.policy?.provider_rdps_credit_allowed !== false ||
    document?.policy?.unresolved_evidence_is_preserved !== true ||
    countKeys.some((key) => !Number.isSafeInteger(Number(summary[key])) || Number(summary[key]) < 0) ||
    summary.matching_capture_runs <= 0 ||
    summary.exact_divergent_capture_runs !== 1 ||
    summary.near_controlled_target_divergent_pairs !== 0 ||
    summary.equal_output_status_bundle_examples !== 1 ||
    summary.near_controlled_target_pairs !==
      summary.near_controlled_target_divergent_pairs + summary.near_controlled_target_equal_pairs ||
    interpretation.independent_divergent_baseline_replication_proven !== false ||
    interpretation.additional_near_controlled_divergent_replication_observed !== false ||
    interpretation.equal_output_status_bundle_diagnostic_observed !== true ||
    interpretation.equal_output_status_bundle_is_an_isolated_effect_zero_proof !== false ||
    interpretation.target_current_hp_is_controlled_in_equal_output_bundle !== false ||
    interpretation.candidate_status_is_isolated_in_equal_output_bundle !== false ||
    interpretation.exact_transform_proven !== false ||
    interpretation.operation_order_and_stacking_proven !== false ||
    interpretation.runtime_integer_rounding_proven !== false ||
    interpretation.canonical_party_conservation_proven !== false ||
    interpretation.formula_authority !== false ||
    interpretation.runtime_authority !== false ||
    interpretation.ui_authority !== false ||
    interpretation.provider_rdps_credit_allowed !== false ||
    contentHash(document) !== String(document.content_sha256 ?? "")) {
    throw new Error("Near-controlled exhaustion rollup is not the expected fail-closed exact-build frontier");
  }
  return {
    input,
    content_sha256: document.content_sha256,
    summary: structuredClone(summary),
    interpretation: structuredClone(interpretation),
  };
}

function analyzeSpHealOperatorEvidence(document, input, effectId, gameBuild) {
  if (document?.schema_version !== 2 ||
    document?.generated_by !== "tools/bpsr-spheal-operator-proof.mjs" ||
    String(document?.game_build ?? "") !== gameBuild ||
    Number(document?.effect_id) !== effectId ||
    document?.policy?.exact_input_build_and_hashes_are_authoritative !== true ||
    document?.policy?.damage_script_name_is_grouping_evidence_not_formula_authority !== true ||
    document?.policy?.packet_absence_is_not_zero !== true ||
    document?.policy?.candidate_hp_ratios_are_compatibility_constraints_not_operator_proof !== true ||
    document?.policy?.unobserved_effect_rows_are_never_backfilled_from_other_spheal_rows !== true ||
    document?.policy?.formula_authority !== false ||
    document?.policy?.runtime_authority !== false ||
    document?.policy?.provider_rdps_credit_allowed !== false ||
    document?.policy?.unresolved_evidence_is_preserved !== true ||
    document?.summary?.exact_effect_output_packet_observed !== false ||
    !Number.isSafeInteger(Number(document?.summary?.exact_effect_occurrence_proof_rlogs)) ||
    Number(document.summary.exact_effect_occurrence_proof_rlogs) <= 0 ||
    !Number.isSafeInteger(Number(document?.summary?.exact_effect_occurrence_proof_healing_events_scanned)) ||
    Number(document.summary.exact_effect_occurrence_proof_healing_events_scanned) < 0 ||
    document?.summary?.exact_effect_occurrence_proof_selected_events !== 0 ||
    document?.summary?.exact_effect_spheal_coefficient_to_hp_basis_binding_proven !== false ||
    document?.summary?.damage_script_identity_alone_proves_operator !== false ||
    document?.summary?.exact_effect_operator_proven !== false ||
    document?.interpretation?.exact_effect_output_occurrence_missing !== true ||
    document?.interpretation?.exact_effect_output_absent_in_all_complete_matching_build_capture_inputs !== true ||
    document?.interpretation?.family_name_transfer_to_exact_effect_allowed !== false ||
    document?.interpretation?.exact_effect_formula_authority !== false ||
    document?.interpretation?.exact_effect_runtime_authority !== false ||
    document?.interpretation?.provider_rdps_credit_allowed !== false ||
    !Array.isArray(document.exact_effect_spheal_rows) ||
    document.exact_effect_spheal_rows.length === 0 ||
    document.exact_effect_spheal_rows.some((row) =>
      Number(row.type_enum) !== effectId || row.damage_script !== "SpHeal"
    ) || !Array.isArray(document.inputs?.rlogs) || document.inputs.rlogs.length === 0 ||
    !Array.isArray(document.inputs?.exact_effect_occurrence_rlogs) ||
    document.inputs.exact_effect_occurrence_rlogs.length !==
      Number(document.summary.exact_effect_occurrence_proof_rlogs) ||
    document.inputs.rlogs.some((rlog) =>
      String(rlog.game_build ?? "") !== gameBuild ||
      !Number.isSafeInteger(Number(rlog.bytes)) || Number(rlog.bytes) <= 0 ||
      !/^sha256:[0-9a-f]{64}$/.test(String(rlog.sha256 ?? ""))
    ) || document.inputs.exact_effect_occurrence_rlogs.some((rlog) =>
      String(rlog.game_build ?? "") !== gameBuild ||
      !Number.isSafeInteger(Number(rlog.bytes)) || Number(rlog.bytes) <= 0 ||
      !/^sha256:[0-9a-f]{64}$/.test(String(rlog.sha256 ?? ""))
    ) || !document.inputs.rlogs.every((rlog) =>
      document.inputs.exact_effect_occurrence_rlogs.some((candidate) =>
        path.basename(String(candidate.path).replaceAll("\\", "/")) ===
          path.basename(String(rlog.path).replaceAll("\\", "/")) &&
        Number(candidate.bytes) === Number(rlog.bytes) &&
        candidate.sha256 === rlog.sha256 && candidate.game_build === rlog.game_build
      )
    ) || contentHash(document) !== String(document.content_sha256 ?? "")) {
    throw new Error("SpHeal operator proof is not exact-build fail-closed evidence for the selected effect");
  }
  return {
    proof: input,
    exact_build_identity: gameBuild,
    effect_id: effectId,
    exact_effect_static_rows: structuredClone(document.exact_effect_static_rows),
    summary: structuredClone(document.summary),
    interpretation: structuredClone(document.interpretation),
    input_rlogs: structuredClone(document.inputs.rlogs),
    exact_effect_occurrence_rlogs: structuredClone(document.inputs.exact_effect_occurrence_rlogs),
    formula_authority: false,
    runtime_authority: false,
    provider_rdps_credit_allowed: false,
  };
}

function analyzeProviderFormulaInputCoverage(coverage, ownership, context) {
  if (ownership?.schema_version !== 3 ||
    ownership?.tool !== "rlogs-bpsr-status-effect-provider-ownership-proof" ||
    String(ownership?.game_build ?? "") !== context.gameBuild ||
    ownership?.policy?.formula_authority !== false ||
    ownership?.policy?.runtime_authority !== false ||
    ownership?.policy?.provider_rdps_credit_allowed !== false ||
    ownership?.policy?.unknown_and_unresolved_events_preserved !== true ||
    !ownership?.selection?.effect_ids?.map(Number).includes(context.effectId)) {
    throw new Error("Provider ownership proof is not exact-build fail-closed evidence for the selected effect");
  }
  const effectOwnership = (ownership.effects ?? []).find((entry) =>
    Number(entry.effect_id) === context.effectId
  );
  if (!effectOwnership ||
    effectOwnership.player_actor_ownership_proven_for_every_sourced_event !== true ||
    effectOwnership.stable_player_character_id_proven_for_every_sourced_event !== true ||
    effectOwnership.formula_authority !== false ||
    effectOwnership.runtime_authority !== false) {
    throw new Error("Provider ownership proof does not close every selected sourced event");
  }
  const provenProviderEntityUuids = [...new Set((ownership.resolutions ?? [])
    .filter((entry) => Number(entry.effect_id) === context.effectId && entry.class === "direct_player")
    .map((entry) => exactPositiveNumber(
      entry.source?.entity_uuid,
      "provider ownership resolution source entity UUID",
    )))].sort((left, right) => left - right);
  if (provenProviderEntityUuids.length === 0 ||
    provenProviderEntityUuids.length !== Number(effectOwnership.unique_source_entities)) {
    throw new Error("Provider ownership proof has inconsistent provider entity coverage");
  }
  const providerIdentityByEntity = new Map();
  for (const resolution of (ownership.resolutions ?? []).filter((entry) =>
    Number(entry.effect_id) === context.effectId && entry.class === "direct_player"
  )) {
    const providerEntityUuid = exactPositiveNumber(
      resolution.source?.entity_uuid,
      "provider ownership identity entity UUID",
    );
    const characterId = String(resolution.source?.character_id ?? "");
    if (!/^\d+$/.test(characterId) || resolution.source?.kind !== "player") {
      throw new Error("Provider ownership proof has an invalid stable player identity");
    }
    const identity = {
      provider_entity_uuid: providerEntityUuid,
      character_id: characterId,
      character_id_source: String(resolution.source?.character_id_source ?? ""),
    };
    const existing = providerIdentityByEntity.get(providerEntityUuid);
    if (existing && JSON.stringify(existing) !== JSON.stringify(identity)) {
      throw new Error("Provider ownership proof has conflicting stable player identities");
    }
    providerIdentityByEntity.set(providerEntityUuid, identity);
  }
  const provenProviderIdentities = [...providerIdentityByEntity.values()].sort((left, right) =>
    left.provider_entity_uuid - right.provider_entity_uuid
  );
  if (JSON.stringify(provenProviderIdentities.map((entry) => entry.provider_entity_uuid)) !==
    JSON.stringify(provenProviderEntityUuids)) {
    throw new Error("Provider ownership proof identity coverage does not match provider coverage");
  }

  if (coverage?.schema_version !== 4 ||
    String(coverage?.expected_game_build ?? "") !== context.gameBuild ||
    JSON.stringify(coverage?.observed_game_builds ?? []) !== JSON.stringify([context.gameBuild]) ||
    typeof coverage?.policy !== "string" ||
    !coverage.policy.startsWith("build_locked_complete_packet_exact_aggregates") ||
    coverage?.actor_filter_semantics !==
      "an empty set selects every actor; otherwise only the exact numeric entity UUIDs listed are selected") {
    throw new Error("Provider formula-input coverage is not an exact-build schema-4 scalar audit");
  }
  const coverageProviders = exactPositiveNumberArray(
    coverage.actor_filters,
    "provider formula-input coverage actor filters",
  ).sort((left, right) => left - right);
  if (JSON.stringify(coverageProviders) !== JSON.stringify(provenProviderEntityUuids)) {
    throw new Error("Provider formula-input coverage actor filters do not exactly match proven providers");
  }

  if (!Array.isArray(ownership.inputs) || ownership.inputs.length === 0) {
    throw new Error("Provider ownership proof has no exact capture inputs");
  }
  const ownershipInputs = new Map(ownership.inputs.map((input) => {
    const basename = path.basename(String(input.path ?? "").replaceAll("\\", "/"));
    if (!basename || ownershipInputDuplicate(ownership.inputs, basename)) {
      throw new Error("Provider ownership proof has missing or duplicate input basenames");
    }
    return [basename, input];
  }));
  const captureInputs = (coverage.inputs ?? []).map((input) => {
    const basename = path.basename(String(input.path ?? "").replaceAll("\\", "/"));
    const expected = ownershipInputs.get(basename);
    if (!expected ||
      Number(input.bytes) !== Number(expected.bytes) ||
      String(input.sha256 ?? "") !== String(expected.sha256 ?? "") ||
      String(input.game_build ?? "") !== context.gameBuild ||
      String(input.session_id ?? "") !== String(expected.session_id ?? "") ||
      !Number.isSafeInteger(Number(input.canonical_events_scanned)) ||
      Number(input.canonical_events_scanned) <= 0 ||
      !Number.isSafeInteger(Number(input.selected_actor_attribute_events_scanned)) ||
      Number(input.selected_actor_attribute_events_scanned) <= 0) {
      throw new Error(`Provider formula-input coverage input ${basename || "<missing>"} is not identical to provider ownership evidence`);
    }
    return {
      rlog: basename,
      bytes: Number(input.bytes),
      sha256: String(input.sha256),
      session_id: String(input.session_id),
      game_build: String(input.game_build),
      canonical_events_scanned: Number(input.canonical_events_scanned),
      entity_attribute_events_scanned: Number(input.entity_attribute_events_scanned),
      selected_actor_attribute_events_scanned: Number(input.selected_actor_attribute_events_scanned),
    };
  });
  if (captureInputs.length !== ownershipInputs.size ||
    new Set(captureInputs.map((input) => input.rlog)).size !== captureInputs.length) {
    throw new Error("Provider formula-input coverage does not contain every ownership-proof input exactly once");
  }

  const auditedCandidateFormulaInputs = {
    physical_attack: normalizeCoverageAttribute(
      coverage.attributes?.[String(PHYSICAL_ATTACK_ATTRIBUTE_ID)],
      PHYSICAL_ATTACK_ATTRIBUTE_ID,
      coverageProviders,
      captureInputs,
    ),
    magical_attack: normalizeCoverageAttribute(
      coverage.attributes?.[String(MAGICAL_ATTACK_ATTRIBUTE_ID)],
      MAGICAL_ATTACK_ATTRIBUTE_ID,
      coverageProviders,
      captureInputs,
    ),
    mastery: normalizeCoverageAttribute(
      coverage.attributes?.[String(MASTERY_ATTRIBUTE_ID)],
      MASTERY_ATTRIBUTE_ID,
      coverageProviders,
      captureInputs,
    ),
  };
  return {
    inputs: {
      provider_ownership_proof: context.ownershipInput,
      provider_formula_input_coverage: context.coverageInput,
    },
    exact_build_identity: context.gameBuild,
    effect_id: context.effectId,
    exact_provider_and_input_identity_match: true,
    proven_provider_entity_uuids: provenProviderEntityUuids,
    proven_provider_identities: provenProviderIdentities,
    capture_inputs: captureInputs,
    audited_candidate_formula_inputs: auditedCandidateFormulaInputs,
    interpretation: {
      packet_absence_is_not_zero: true,
      unobserved_inputs_are_not_backfilled_or_derived: true,
      spheal_operator_contract_proven: false,
      effect_output_to_status_transform_binding_proven: false,
      provider_formula_base_input_proven: false,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
    },
  };
}

function normalizeCoverageAttribute(summary, attributeId, providerEntityUuids, captureInputs) {
  if (!summary || !Number.isSafeInteger(Number(summary.observation_count)) ||
    Number(summary.observation_count) < 0 || !summary.actors || !summary.rlogs ||
    !summary.update_kinds || !Array.isArray(summary.samples)) {
    throw new Error(`Provider formula-input coverage has no exact aggregate for attribute ${attributeId}`);
  }
  const observationCount = Number(summary.observation_count);
  const actorCounts = exactNonNegativeCountMap(summary.actors, `attribute ${attributeId} actors`);
  const rlogCounts = exactNonNegativeCountMap(summary.rlogs, `attribute ${attributeId} rlogs`);
  const updateKindCounts = exactNonNegativeCountMap(
    summary.update_kinds,
    `attribute ${attributeId} update kinds`,
  );
  if ([actorCounts, rlogCounts, updateKindCounts].some((counts) =>
    Object.values(counts).reduce((sum, count) => sum + count, 0) !== observationCount
  )) {
    throw new Error(`Provider formula-input coverage aggregate mismatch for attribute ${attributeId}`);
  }
  if (Object.keys(actorCounts).some((actor) =>
    !providerEntityUuids.includes(exactPositiveNumber(actor, `attribute ${attributeId} actor`))
  ) || Object.keys(rlogCounts).some((rlog) =>
    !captureInputs.some((input) => input.rlog === rlog)
  ) || Object.keys(updateKindCounts).some((kind) =>
    !["snapshot", "delta", "unknown"].includes(kind)
  )) {
    throw new Error(`Provider formula-input coverage contains an out-of-scope aggregate for attribute ${attributeId}`);
  }
  return {
    attribute_id: attributeId,
    observation_count: observationCount,
    actors: actorCounts,
    rlogs: rlogCounts,
    update_kinds: updateKindCounts,
  };
}

function exactNonNegativeCountMap(value, label) {
  if (!value || Array.isArray(value) || typeof value !== "object") {
    throw new Error(`${label} must be an object`);
  }
  return Object.fromEntries(Object.entries(value).map(([key, raw]) => {
    const count = Number(raw);
    if (!Number.isSafeInteger(count) || count < 0) {
      throw new Error(`${label} contains a non-count value`);
    }
    return [key, count];
  }));
}

function ownershipInputDuplicate(inputs, basename) {
  return inputs.filter((input) =>
    path.basename(String(input.path ?? "").replaceAll("\\", "/")) === basename
  ).length !== 1;
}

function sameStatus(left, right) {
  return [
    "effect_id",
    "source_entity_uuid",
    "stacks",
    "level",
    "origin_source_type_id",
    "origin_source_config_id",
  ].every((key) => (left?.[key] ?? null) === (right?.[key] ?? null));
}

function exactAttributeState(value, label) {
  if (!Array.isArray(value)) throw new Error(`${label} must be an array`);
  const normalized = value.map((entry) => ({
    attribute_id: exactPositiveNumber(entry?.attribute_id, `${label} attribute ID`),
    value: exactIntegerNumber(entry?.value, `${label} value`),
  }));
  if (normalized.some((entry, index) => index > 0 &&
    normalized[index - 1].attribute_id >= entry.attribute_id)) {
    throw new Error(`${label} must have unique ascending attribute IDs`);
  }
  return normalized;
}

function analyzeStaticFormulaCandidates(
  decodedDamageAttrs,
  formulaSurface,
  effectId,
  damageAttrInput,
  formulaSurfaceInput,
  expectedBuild,
) {
  if (!decodedDamageAttrs || Array.isArray(decodedDamageAttrs) || typeof decodedDamageAttrs !== "object") {
    throw new Error("Current-build DamageAttrTable must be a JSON object");
  }
  if (formulaSurface?.schema_version !== 1 ||
    formulaSurface?.generated_by !== "rlogs-bpsr-damage-attr-semantic-surface" ||
    String(formulaSurface?.game_build ?? "") !== expectedBuild ||
    formulaSurface?.policy?.runtime_formula_authority !== false ||
    formulaSurface?.policy?.semantic_decoded_bridge !== true ||
    formulaSurface?.policy?.exact_build_table_required !== true ||
    formulaSurface?.policy?.unresolved_rows_hidden !== false ||
    Number(formulaSurface?.input?.bytes) !== damageAttrInput.bytes ||
    String(formulaSurface?.input?.sha256 ?? "") !== damageAttrInput.sha256) {
    throw new Error("DamageAttrTable is not bound to the exact-build semantic formula surface");
  }
  const rows = Object.entries(decodedDamageAttrs)
    .filter(([, row]) => Number(row?.TypeEnum) === effectId)
    .map(([rowKey, row]) => {
      const damageAttrId = exactPositiveNumber(row?.Id, `DamageAttrTable row ${rowKey} Id`);
      if (String(damageAttrId) !== rowKey) {
        throw new Error(`DamageAttrTable row key ${rowKey} does not match Id ${damageAttrId}`);
      }
      const candidate = {
        damage_attr_id: damageAttrId,
        type_enum: effectId,
        damage_script: typeof row.DamageScript === "string" ? row.DamageScript : null,
        coefficient_basis_points_by_stage: exactIntegerArray(
          row.PVEDamageRadio,
          `DamageAttrTable row ${rowKey} PVEDamageRadio`,
        ),
        fixed_parameter_by_level: exactIntegerArray(
          row.PVEFixedParameter,
          `DamageAttrTable row ${rowKey} PVEFixedParameter`,
        ),
      };
      const semanticRow = formulaSurface.rows?.[rowKey];
      if (Number(semanticRow?.damage_id) !== damageAttrId ||
        Number(semanticRow?.linked_id) !== effectId ||
        JSON.stringify(semanticRow?.int_array_pool_1_candidates_by_offset?.["28"]?.values) !==
          JSON.stringify(candidate.coefficient_basis_points_by_stage) ||
        JSON.stringify(semanticRow?.int_array_pool_1_candidates_by_offset?.["32"]?.values) !==
          JSON.stringify(candidate.fixed_parameter_by_level)) {
        throw new Error(`DamageAttrTable row ${rowKey} does not match the exact-build semantic formula surface`);
      }
      return candidate;
    })
    .sort((left, right) => left.damage_attr_id - right.damage_attr_id);
  if (rows.length === 0) {
    throw new Error(`DamageAttrTable has no exact TypeEnum rows for effect ${effectId}`);
  }
  return {
    inputs: {
      damage_attr_table: damageAttrInput,
      exact_build_formula_surface: formulaSurfaceInput,
    },
    exact_build_identity: expectedBuild,
    exact_build_table_hash_match_proven: true,
    selected_rows_match_semantic_surface: true,
    selection_rule: "DamageAttrTable.TypeEnum equals the exact numeric effect ID",
    typed_field_role: "current-build candidate inputs for outputs owned by the effect; not evidence that the field modifies another ability's observed damage",
    rows,
  };
}

function attachStaticCompatibility(staticFormulaEvidence, observations, roundingModes) {
  const evidence = structuredClone(staticFormulaEvidence);
  const observedAbilityIds = [...new Set(observations.map((entry) => entry.abilityId))]
    .sort((left, right) => left - right);
  const coefficients = [...new Set(evidence.rows.flatMap((row) =>
    row.coefficient_basis_points_by_stage
  ))].sort((left, right) => left - right);
  evidence.observed_counterfactual_ability_ids = observedAbilityIds;
  evidence.observed_event_ability_matches_effect_output_ability =
    observedAbilityIds.every((abilityId) => abilityId === evidence.rows[0].type_enum);
  evidence.distinct_coefficient_basis_points = coefficients;
  evidence.hypothetical_post_output_delta_compatibility = coefficients.map((basisPoints) => ({
    basis_points: basisPoints,
    hypothesis: "treat the decoded coefficient as an additive post-output damage multiplier delta",
    compatible_rounding_modes: roundingModes.filter((mode) => observations.every((observation) => {
      const multiplier = FIXED_POINT_DENOMINATOR + BigInt(basisPoints);
      return roundedQuotient(
        observation.absent * multiplier,
        FIXED_POINT_DENOMINATOR,
        mode,
      ) === observation.present;
    })),
  }));
  evidence.interpretation = {
    exact_type_enum_output_link_proven: true,
    typed_current_build_inputs_preserved: true,
    coefficient_is_effect_output_formula_input_not_proven_status_modifier: true,
    incompatibility_with_hypothetical_post_output_delta_does_not_disprove_other_server_operations: true,
    exact_static_status_transform_binding_proven: false,
    formula_authority: false,
    runtime_authority: false,
  };
  return evidence;
}

function compatibleBasisPoints(observation, mode, maximumAbsoluteBasisPoints) {
  const candidates = [];
  for (let basisPoints = -maximumAbsoluteBasisPoints; basisPoints <= maximumAbsoluteBasisPoints; basisPoints += 1) {
    const multiplier = FIXED_POINT_DENOMINATOR + BigInt(basisPoints);
    if (multiplier < 0n) continue;
    const numerator = observation.absent * multiplier;
    if (roundedQuotient(numerator, FIXED_POINT_DENOMINATOR, mode) === observation.present) {
      candidates.push(basisPoints);
    }
  }
  return candidates;
}

function roundedQuotient(numerator, denominator, mode) {
  if (numerator < 0n || denominator <= 0n) throw new Error("Integer transform requires non-negative numerator and positive denominator");
  if (mode === "floor") return numerator / denominator;
  if (mode === "ceil") return (numerator + denominator - 1n) / denominator;
  if (mode === "round_half_up") return (numerator + denominator / 2n) / denominator;
  throw new Error(`Unsupported rounding mode ${mode}`);
}

function intersectCandidateSets(candidateSets) {
  const [first = [], ...rest] = candidateSets;
  return first.filter((candidate) => rest.every((set) => set.includes(candidate)));
}

function reducedRatio(numerator, denominator) {
  const divisor = greatestCommonDivisor(numerator, denominator);
  return {
    numerator: (numerator / divisor).toString(),
    denominator: (denominator / divisor).toString(),
  };
}

function greatestCommonDivisor(left, right) {
  let a = left < 0n ? -left : left;
  let b = right < 0n ? -right : right;
  while (b !== 0n) [a, b] = [b, a % b];
  return a;
}

function countCompatibleModels(multiplicative, additiveDeltas) {
  return Object.values(multiplicative).reduce((sum, values) => sum + values.length, 0) +
    (additiveDeltas.length === 1 ? 1 : 0);
}

function exactPositiveInteger(value, label) {
  if (!Number.isSafeInteger(Number(value)) || Number(value) <= 0) {
    throw new Error(`${label} must be a positive safe integer`);
  }
  return BigInt(String(value));
}

function exactPositiveNumber(value, label) {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed <= 0) {
    throw new Error(`${label} must be a positive safe integer`);
  }
  return parsed;
}

function nullablePositiveNumber(value, label) {
  return value === null || value === undefined ? null : exactPositiveNumber(value, label);
}

function exactIntegerNumber(value, label) {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed)) throw new Error(`${label} must be a safe integer`);
  return parsed;
}

function uniqueStrings(values) {
  return [...new Set(values.map(String))].sort();
}

function exactIntegerArray(value, label) {
  if (!Array.isArray(value) || value.some((entry) => !Number.isSafeInteger(Number(entry)))) {
    throw new Error(`${label} must be an exact integer array`);
  }
  return value.map(Number);
}

function exactPositiveNumberArray(value, label) {
  if (!Array.isArray(value) || value.length === 0) {
    throw new Error(`${label} must be a non-empty array`);
  }
  const normalized = value.map((entry) => exactPositiveNumber(entry, label));
  if (new Set(normalized).size !== normalized.length) {
    throw new Error(`${label} must contain unique values`);
  }
  return normalized;
}

function exactSequenceList(values, label) {
  if (!Array.isArray(values) || values.length === 0 ||
    values.some((value) => !Number.isSafeInteger(Number(value)) || Number(value) <= 0)) {
    throw new Error(`${label} must contain positive exact sequences`);
  }
  return values.map(Number);
}

function compareIntegerText(left, right) {
  const a = BigInt(left);
  const b = BigInt(right);
  return a < b ? -1 : a > b ? 1 : 0;
}

function contentHash(value) {
  const copy = structuredClone(value);
  delete copy.content_sha256;
  return createHash("sha256").update(JSON.stringify(copy)).digest("hex");
}

function fileDescriptor(file) {
  const bytes = readFileSync(file);
  return {
    path: file.replaceAll("\\", "/"),
    bytes: statSync(file).size,
    sha256: createHash("sha256").update(bytes).digest("hex"),
  };
}

function readJson(file, label) {
  try {
    return JSON.parse(readFileSync(file, "utf8"));
  } catch (error) {
    throw new Error(`Unable to read ${label} ${file}: ${error.message}`);
  }
}

function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index];
    const value = args[index + 1];
    if (!key?.startsWith("--") || value === undefined || value.startsWith("--")) {
      throw new Error(`Invalid argument near ${key ?? "<end>"}`);
    }
    parsed[key.slice(2)] = value;
  }
  return parsed;
}

function required(parsed, key) {
  if (!parsed[key]) throw new Error(`Missing --${key}`);
  return parsed[key];
}

function positiveInteger(value, label) {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed <= 0) throw new Error(`--${label} must be a positive integer`);
  return parsed;
}

function selfTest() {
  const frontier = {
    schema_version: 3,
    generated_by: "rlogs-bpsr-status-effect-counterfactual-proof",
    game_build: "1",
    policy: {
      formula_authority: false,
      runtime_authority: false,
      unresolved_evidence_is_hidden: false,
    },
    effects: [{
      effect_id: 2206241,
      locus: "target",
      exact_recorded_inputs: {
        divergent_examples: [{
          session_id: "session-1",
          rlog: "capture.rlog",
          run_ordinal: 1,
          source_entity_uuid: 5288320959104,
          target_entity_uuid: 7203061824,
          provider_relationship: "third_party",
          ability_id: 230401,
          locus: "target",
          status: {
            effect_id: 2206241,
            source_entity_uuid: 349530030720,
            stacks: 1,
            level: 1,
            origin_source_type_id: 1,
            origin_source_config_id: 2206240,
          },
          absent_outcome: { amount: 10550 },
          present_outcome: { amount: 10935 },
          absent_sequences: [179314],
          present_sequences: [180141],
        }],
      },
    }],
  };
  const report = analyzeFrontier(frontier, {
    effectId: 2206241,
    locus: "target",
    maximumAbsoluteBasisPoints: 10_000,
    input: { path: "fixture.json", bytes: 1, sha256: "0".repeat(64) },
    staticFormulaEvidence: analyzeStaticFormulaCandidates({
      "2220624103": {
        Id: 2220624103,
        TypeEnum: 2206241,
        DamageScript: "SpHeal",
        PVEDamageRadio: [2000],
        PVEFixedParameter: [],
      },
      "2220624105": {
        Id: 2220624105,
        TypeEnum: 2206241,
        DamageScript: "Attack",
        PVEDamageRadio: [2000],
        PVEFixedParameter: [],
      },
    }, {
      schema_version: 1,
      generated_by: "rlogs-bpsr-damage-attr-semantic-surface",
      game_build: "1",
      input: { bytes: 2, sha256: "1".repeat(64) },
      policy: {
        runtime_formula_authority: false,
        semantic_decoded_bridge: true,
        exact_build_table_required: true,
        unresolved_rows_hidden: false,
      },
      rows: {
        "2220624103": {
          damage_id: 2220624103,
          linked_id: 2206241,
          int_array_pool_1_candidates_by_offset: {
            "28": { values: [2000] },
            "32": { values: [] },
          },
        },
        "2220624105": {
          damage_id: 2220624105,
          linked_id: 2206241,
          int_array_pool_1_candidates_by_offset: {
            "28": { values: [2000] },
            "32": { values: [] },
          },
        },
      },
    }, 2206241,
    { path: "DamageAttrTable.json", bytes: 2, sha256: "1".repeat(64) },
    { path: "damage-formula-surface.json", bytes: 3, sha256: "2".repeat(64) },
    "1"),
  });
  const models = report.compatible_model_intersection;
  if (models.post_output_multiplicative_delta_basis_points.floor.join(",") !== "365" ||
    models.post_output_multiplicative_delta_basis_points.round_half_up.join(",") !== "365" ||
    models.post_output_multiplicative_delta_basis_points.ceil.join(",") !== "364" ||
    models.fixed_additive_integer_delta !== "385" || report.interpretation.exact_transform_proven !== false ||
    report.static_formula_input_candidates.distinct_coefficient_basis_points.join(",") !== "2000" ||
    report.static_formula_input_candidates.exact_build_table_hash_match_proven !== true ||
    report.static_formula_input_candidates.selected_rows_match_semantic_surface !== true ||
    report.static_formula_input_candidates.observed_event_ability_matches_effect_output_ability !== false ||
    report.static_formula_input_candidates.hypothetical_post_output_delta_compatibility[0]
      .compatible_rounding_modes.length !== 0 ||
    report.provider_formula_context_summary.formula_context_embedded_for_every_example !== false ||
    report.interpretation.exact_static_status_transform_binding_proven !== false ||
    report.policy.formula_authority !== false || report.policy.runtime_authority !== false) {
    throw new Error("Integer transform constraint self-test failed");
  }
  const selectedStatus = {
    effect_id: 2206241,
    source_entity_uuid: 349530030720,
    stacks: 1,
    level: 1,
    origin_source_type_id: 1,
    origin_source_config_id: 2206240,
  };
  const providerAttributes = [{ attribute_id: 10030, value: 63342 }];
  const providerStates = [{
    provider_entity_uuid: 349530030720,
    attribute_state_observed: true,
    attribute_state_id: 7,
    attributes: providerAttributes,
  }];
  const commonFormulaContext = {
    normalized_packet_input_sha256: `sha256:${"3".repeat(64)}`,
    normalized_packet_inputs: { owner_id: 230401, owner_level: 30 },
    source_attributes: [{ attribute_id: 10030, value: 65304 }],
    target_attributes: [{ attribute_id: 10030, value: 63945 }],
    source_statuses: [],
    status_provider_attributes: providerStates,
  };
  const providerContext = normalizeProviderContext({
    status: selectedStatus,
    present_formula_context: {
      ...commonFormulaContext,
      representative_sequence: 180141,
      target_statuses: [selectedStatus],
    },
    absent_formula_context: {
      ...commonFormulaContext,
      representative_sequence: 179314,
      target_statuses: [],
    },
  }, 0, [179314], [180141]);
  if (providerContext.embedded !== true ||
    providerContext.exact_provider_attribute_state_controlled !== true ||
    providerContext.attribute_state_observed !== true ||
    providerContext.physical_attack_value !== null ||
    providerContext.provider_formula_base_input_proven !== false) {
    throw new Error("Provider formula context self-test failed");
  }
  const emptyCoverageAttribute = {
    observation_count: 0,
    actors: {},
    rlogs: {},
    update_kinds: {},
    raw_values: {},
    samples: [],
    dropped_samples: 0,
  };
  const providerFormulaInputCoverage = analyzeProviderFormulaInputCoverage({
    schema_version: 4,
    policy: "build_locked_complete_packet_exact_aggregates_fixture",
    expected_game_build: "1",
    observed_game_builds: ["1"],
    actor_filters: [349530030720],
    actor_filter_semantics:
      "an empty set selects every actor; otherwise only the exact numeric entity UUIDs listed are selected",
    inputs: [{
      path: "capture.rlog",
      bytes: 10,
      sha256: `sha256:${"a".repeat(64)}`,
      session_id: "session-1",
      game_build: "1",
      canonical_events_scanned: 100,
      entity_attribute_events_scanned: 10,
      selected_actor_attribute_events_scanned: 5,
    }],
    attributes: {
      [PHYSICAL_ATTACK_ATTRIBUTE_ID]: emptyCoverageAttribute,
      [MAGICAL_ATTACK_ATTRIBUTE_ID]: emptyCoverageAttribute,
      [MASTERY_ATTRIBUTE_ID]: emptyCoverageAttribute,
    },
  }, {
    schema_version: 3,
    tool: "rlogs-bpsr-status-effect-provider-ownership-proof",
    game_build: "1",
    policy: {
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
      unknown_and_unresolved_events_preserved: true,
    },
    selection: { effect_ids: [2206241] },
    inputs: [{
      path: "C:/capture.rlog",
      bytes: 10,
      sha256: `sha256:${"a".repeat(64)}`,
      session_id: "session-1",
    }],
    effects: [{
      effect_id: 2206241,
      unique_source_entities: 1,
      player_actor_ownership_proven_for_every_sourced_event: true,
      stable_player_character_id_proven_for_every_sourced_event: true,
      formula_authority: false,
      runtime_authority: false,
    }],
    resolutions: [{
      effect_id: 2206241,
      class: "direct_player",
      source: {
        entity_uuid: 349530030720,
        kind: "player",
        character_id: "5333405",
        character_id_source: "bpsr_player_entity_uuid_contract",
      },
    }],
  }, {
    effectId: 2206241,
    gameBuild: "1",
    coverageInput: { path: "coverage.json", bytes: 1, sha256: "0".repeat(64) },
    ownershipInput: { path: "ownership.json", bytes: 1, sha256: "1".repeat(64) },
  });
  if (providerFormulaInputCoverage.exact_provider_and_input_identity_match !== true ||
    providerFormulaInputCoverage.capture_inputs.length !== 1 ||
    providerFormulaInputCoverage.proven_provider_entity_uuids.join(",") !== "349530030720" ||
    providerFormulaInputCoverage.proven_provider_identities[0]?.character_id !== "5333405" ||
    providerFormulaInputCoverage.audited_candidate_formula_inputs.physical_attack
      .observation_count !== 0 ||
    providerFormulaInputCoverage.interpretation.provider_formula_base_input_proven !== false) {
    throw new Error("Provider formula-input coverage self-test failed");
  }
  const spHealFixture = {
    schema_version: 2,
    generated_by: "tools/bpsr-spheal-operator-proof.mjs",
    game_build: "1",
    effect_id: 2206241,
    policy: {
      exact_input_build_and_hashes_are_authoritative: true,
      damage_script_name_is_grouping_evidence_not_formula_authority: true,
      packet_absence_is_not_zero: true,
      candidate_hp_ratios_are_compatibility_constraints_not_operator_proof: true,
      unobserved_effect_rows_are_never_backfilled_from_other_spheal_rows: true,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
      unresolved_evidence_is_preserved: true,
    },
    inputs: {
      exact_effect_occurrence_proof: {
        path: "occurrence.json",
        bytes: 1,
        sha256: "3".repeat(64),
      },
      rlogs: [{
        path: "capture.rlog",
        bytes: 1,
        sha256: `sha256:${"a".repeat(64)}`,
        game_build: "1",
      }],
      exact_effect_occurrence_rlogs: [{
        path: "capture.rlog",
        bytes: 1,
        sha256: `sha256:${"a".repeat(64)}`,
        game_build: "1",
      }],
    },
    exact_effect_static_rows: [{ type_enum: 2206241, damage_script: "SpHeal" }],
    exact_effect_spheal_rows: [{ type_enum: 2206241, damage_script: "SpHeal" }],
    summary: {
      exact_effect_output_packet_observed: false,
      exact_effect_occurrence_proof_rlogs: 1,
      exact_effect_occurrence_proof_healing_events_scanned: 42,
      exact_effect_occurrence_proof_selected_events: 0,
      exact_effect_spheal_coefficient_to_hp_basis_binding_proven: false,
      damage_script_identity_alone_proves_operator: false,
      exact_effect_operator_proven: false,
    },
    interpretation: {
      exact_effect_output_occurrence_missing: true,
      exact_effect_output_absent_in_all_complete_matching_build_capture_inputs: true,
      family_name_transfer_to_exact_effect_allowed: false,
      exact_effect_formula_authority: false,
      exact_effect_runtime_authority: false,
      provider_rdps_credit_allowed: false,
    },
  };
  spHealFixture.content_sha256 = contentHash(spHealFixture);
  const spHealEvidence = analyzeSpHealOperatorEvidence(
    spHealFixture,
    { path: "spheal.json", bytes: 1, sha256: "2".repeat(64) },
    2206241,
    "1",
  );
  if (spHealEvidence.summary.exact_effect_output_packet_observed !== false ||
    spHealEvidence.interpretation.family_name_transfer_to_exact_effect_allowed !== false ||
    spHealEvidence.formula_authority !== false) {
    throw new Error("SpHeal operator evidence ingestion self-test failed");
  }
  console.log("bpsr-rdps-integer-transform-constraints self-test passed");
}

function usage(exitCode) {
  console.log("Usage:\n  node tools/bpsr-rdps-integer-transform-constraints.mjs analyze --counterfactual-proof <json> --damage-attr-table <current-build DamageAttrTable.json> --damage-formula-surface <exact-build semantic surface.json> [--provider-ownership-proof <json> --provider-formula-input-coverage <json>] [--spheal-operator-proof <json>] [--near-controlled-rollup <json>] --effect <exact-id> [--locus target|source] [--maximum-absolute-basis-points <count>] --output <json>\n  node tools/bpsr-rdps-integer-transform-constraints.mjs self-test");
  process.exit(exitCode);
}
